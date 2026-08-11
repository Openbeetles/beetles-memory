mod support;

use std::path::{Path, PathBuf};

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryKind, MemoryPrivacyClass};
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{
    StoreBackendConfig, StoreEngine, StorePathBudget, StoreRepairPolicy,
};
use serde_json::Value;
use support::seed_scoped_long_term;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-file-store-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn assert_path_budget(root: &Path, path: &Path, budget: StorePathBudget) {
    if path != root {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("file store path has utf-8 name");
        let component_limit = if path.is_dir() {
            budget.max_directory_name_bytes
        } else {
            budget.max_file_name_bytes
        };
        assert!(
            name.len() <= component_limit,
            "component {name:?} exceeds {component_limit} bytes"
        );
        let relative = path
            .strip_prefix(root)
            .expect("file store path is under root")
            .to_string_lossy();
        assert!(
            relative.len() <= budget.max_relative_path_bytes,
            "relative path {relative:?} exceeds {} bytes",
            budget.max_relative_path_bytes
        );
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path).expect("read file store path") {
            let entry = entry.expect("read file store entry");
            assert_path_budget(root, &entry.path(), budget);
        }
    }
}

fn find_one_file(root: &Path, extension: &str) -> PathBuf {
    let mut matches = Vec::new();
    collect_files_with_extension(root, extension, &mut matches);
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one .{extension} file under {} but found {matches:?}",
        root.display()
    );
    matches.remove(0)
}

fn collect_files_with_extension(root: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }
    for entry in std::fs::read_dir(root).expect("read test directory") {
        let entry = entry.expect("read test entry");
        let path = entry.path();
        if path.is_dir() {
            collect_files_with_extension(&path, extension, out);
        } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
            out.push(path);
        }
    }
    out.sort();
}

fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time after epoch")
        .as_secs()
}

#[test]
fn file_store_persists_core_runtime_paths_across_reopen() {
    let root = temp_root("persist");
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap();

    let skill = {
        let platform = support::open_store(config.clone()).unwrap();
        platform
            .state_fs()
            .write("runtime/state.json", b"state")
            .unwrap();
        let skill = support::seed_runtime_skill(&platform, "runtime_skill__alpha");
        platform
            .session_store()
            .append("chat-a", "user", "hello")
            .unwrap();
        seed_scoped_long_term(
            &platform,
            "space:test",
            &LongTermMemoryDraft {
                kind: LongTermMemoryKind::Fact,
                privacy: MemoryPrivacyClass::SharedWithSubject,
                topic: "lens".to_string(),
                content: "Use a fast prime lens indoors".to_string(),
                keywords: vec!["lens".to_string()],
                source_chat_id: Some("chat-a".to_string()),
                source_type: None,
                source_scope: None,
                confidence: None,
                freshness: None,
                stale_hint: None,
                supporting_citations: Vec::new(),
                canonical_entities: Vec::new(),
                evidence_count: None,
                observed_at: None,
                last_confirmed_at: None,
                source_revision: None,
            },
            100,
        );
        assert!(platform
            .read_events()
            .unwrap()
            .iter()
            .any(|event| event.kind_name == "memory.write"));
        skill
    };

    let reopened = support::open_store(config).unwrap();
    assert_eq!(
        reopened.state_fs().read("runtime/state.json").unwrap(),
        Some(b"state".to_vec())
    );
    assert_eq!(
        support::read_runtime_skill_owner(&reopened, &skill.physical_key),
        skill
    );
    assert_eq!(
        reopened.session_store().load_recent("chat-a", 1).unwrap()[0].content,
        "hello"
    );
    assert_eq!(
        reopened
            .memory_space_long_term_memory_read_store("space:test")
            .expect("scoped long-term read store")
            .recall("lens", Some("chat-a"), 4)
            .unwrap()
            .len(),
        1
    );
    assert!(reopened.read_events().unwrap().len() >= 5);
}

#[test]
fn file_snapshot_import_does_not_remove_preexisting_stage_directories() {
    let root = temp_root("snapshot-stage-collision");
    let target = support::open_store(
        StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap(),
    )
    .unwrap();
    let source = support::open_store(
        StoreBackendConfig::in_memory(support::native_persistent_profile()).unwrap(),
    )
    .unwrap();
    source
        .session_store()
        .append("chat-stage", "user", "snapshot stage")
        .unwrap();
    let snapshot = source.export_store_snapshot().unwrap();

    let before = current_unix_secs();
    while current_unix_secs() == before {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let start = current_unix_secs();
    let mut markers = Vec::new();
    for offset in 0..=3 {
        for prefix in [".snapshot-import", ".snapshot-backup"] {
            let marker_dir = root.join(format!(
                "{prefix}-{}-{}",
                start + offset,
                std::process::id()
            ));
            std::fs::create_dir_all(&marker_dir).unwrap();
            let marker = marker_dir.join("owned-by-another-import");
            std::fs::write(&marker, b"do not delete").unwrap();
            markers.push(marker);
        }
    }

    target.import_store_snapshot(&snapshot).unwrap();

    for marker in markers {
        assert!(
            marker.exists(),
            "snapshot import removed pre-existing stage marker {}",
            marker.display()
        );
    }
}

#[test]
fn file_store_open_retains_repair_report_on_platform() {
    let root = temp_root("open-report");
    let tmp = root.join("kv").join("session").join("orphan.tmp");
    std::fs::create_dir_all(tmp.parent().unwrap()).unwrap();
    std::fs::write(&tmp, b"partial").unwrap();

    let platform = support::open_store(
        StoreBackendConfig::file(&root, support::native_persistent_profile())
            .unwrap()
            .with_repair_policy(StoreRepairPolicy::ReportOnly),
    )
    .unwrap();
    let report = platform.open_report();
    assert_eq!(report.backend, "file");
    assert!(report.repair.checked);
    assert!(!report.repair.repaired);
    assert!(
        report
            .repair
            .findings
            .iter()
            .any(|finding| finding.contains("orphan.tmp")),
        "{report:?}"
    );
}

#[test]
fn file_store_maps_long_logical_keys_to_profile_bounded_physical_paths() {
    let root = temp_root("bounded-physical-paths");
    let config = StoreBackendConfig::file(
        &root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .unwrap();
    let budget = config.path_budget();
    let platform = support::open_store(config).unwrap();
    let logical_key = format!("work-room/{}", "long-input-segment-".repeat(32));

    platform.state_fs().write(&logical_key, b"state").unwrap();

    assert_eq!(
        platform.state_fs().read(&logical_key).unwrap(),
        Some(b"state".to_vec())
    );
    assert!(platform
        .state_fs()
        .list_dir("")
        .unwrap()
        .contains(&logical_key));
    assert_path_budget(&root, &root, budget);
}

#[test]
fn file_store_rejects_profile_mismatch_on_existing_root() {
    let root = temp_root("manifest-profile-mismatch");
    support::open_store(
        StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap(),
    )
    .unwrap();

    let err = match support::open_store(
        StoreBackendConfig::file(
            &root,
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    ) {
        Ok(_) => panic!("existing store root must reject a different profile"),
        Err(error) => error,
    };

    assert_eq!(err.stage(), "file_store_manifest");
    assert!(err.to_string().contains("profile"));
}

#[test]
fn file_store_fails_closed_when_v2_blob_data_is_missing_but_index_remains() {
    let root = temp_root("missing-v2-blob-data");
    let platform = support::open_store(
        StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap(),
    )
    .unwrap();
    platform
        .state_fs()
        .write("runtime/state.json", b"state")
        .unwrap();

    let data = find_one_file(&root.join("blob").join("state_fs").join("_v2"), "bin");
    std::fs::remove_file(&data).unwrap();

    let err = platform
        .state_fs()
        .read("runtime/state.json")
        .expect_err("missing v2 data with present index must be corruption, not None");

    assert_eq!(err.stage(), "file_store_blob_read");
    assert!(err.to_string().contains("missing physical data"));
}

#[test]
fn file_store_list_rejects_corrupt_sidecar_key() {
    let root = temp_root("corrupt-sidecar-key");
    let platform = support::open_store(
        StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap(),
    )
    .unwrap();
    platform
        .state_fs()
        .write("runtime/state.json", b"state")
        .unwrap();

    let index = find_one_file(&root.join("blob").join("state_fs").join("_keys"), "json");
    let mut value: Value = serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
    value["key"] = Value::String("runtime/other.json".to_string());
    std::fs::write(&index, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let err = platform
        .state_fs()
        .list_dir("")
        .expect_err("corrupt sidecar key must not produce a ghost logical key");

    assert_eq!(err.stage(), "file_store_list");
    assert!(err.to_string().contains("physical key"));
}

#[test]
fn file_store_delete_refuses_corrupt_sidecar_without_removing_evidence() {
    let root = temp_root("delete-corrupt-sidecar");
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap();
    let (engine, _, _) = support::open_file_engine(&config).unwrap();
    engine
        .put_blob("state_fs", "runtime/state.json", b"state")
        .unwrap();

    let index = find_one_file(&root.join("blob").join("state_fs").join("_keys"), "json");
    let data = find_one_file(&root.join("blob").join("state_fs").join("_v2"), "bin");
    let mut value: Value = serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
    value["key"] = Value::String("runtime/other.json".to_string());
    std::fs::write(&index, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let err = engine
        .delete_blob("state_fs", "runtime/state.json")
        .expect_err("delete must not wipe corrupt physical evidence");

    assert_eq!(err.stage(), "file_store_blob_delete");
    assert!(index.exists());
    assert!(data.exists());
}

#[test]
fn file_store_list_errors_when_keys_path_is_not_directory() {
    let root = temp_root("keys-path-not-directory");
    let platform = support::open_store(
        StoreBackendConfig::file(&root, support::native_persistent_profile()).unwrap(),
    )
    .unwrap();
    let key_dir = root.join("blob").join("state_fs").join("_keys");
    std::fs::create_dir_all(key_dir.parent().unwrap()).unwrap();
    std::fs::write(&key_dir, b"not a directory").unwrap();

    let err = platform
        .state_fs()
        .list_dir("")
        .expect_err("non-directory _keys path must be reported");

    assert_eq!(err.stage(), "file_store_list");
}
