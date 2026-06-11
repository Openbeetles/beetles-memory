use std::path::{Path, PathBuf};

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryKind};
use bm_core::platform::Platform;
use bm_store::{StoreBackendConfig, StorePathBudget, StorePlatform};

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

#[test]
fn file_store_persists_core_runtime_paths_across_reopen() {
    let root = temp_root("persist");
    let config = StoreBackendConfig::file(&root, ProfileId::DesktopMacosEmbeddedSdk).unwrap();

    {
        let platform = StorePlatform::open(config.clone()).unwrap();
        platform
            .state_fs()
            .write("runtime/state.json", b"state")
            .unwrap();
        platform.skill_storage().write("alpha", b"skill").unwrap();
        platform
            .session_store()
            .append("chat-a", "user", "hello")
            .unwrap();
        platform
            .long_term_memory_store()
            .upsert_many(
                &[LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Fact,
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
                    evidence_count: None,
                    observed_at: None,
                    last_confirmed_at: None,
                    source_revision: None,
                }],
                100,
            )
            .unwrap();
        assert!(platform
            .read_events()
            .unwrap()
            .iter()
            .any(|event| event.kind_name == "memory.write"));
    }

    let reopened = StorePlatform::open(config).unwrap();
    assert_eq!(
        reopened.state_fs().read("runtime/state.json").unwrap(),
        Some(b"state".to_vec())
    );
    assert_eq!(reopened.skill_storage().read("alpha").unwrap(), b"skill");
    assert_eq!(
        reopened.session_store().load_recent("chat-a", 1).unwrap()[0].content,
        "hello"
    );
    assert_eq!(
        reopened
            .long_term_memory_store()
            .recall("lens", Some("chat-a"), 4)
            .unwrap()
            .len(),
        1
    );
    assert!(reopened.read_events().unwrap().len() >= 5);
}

#[test]
fn file_store_maps_long_logical_keys_to_profile_bounded_physical_paths() {
    let root = temp_root("bounded-physical-paths");
    let config = StoreBackendConfig::file(&root, ProfileId::DesktopWindowsEmbeddedSdk).unwrap();
    let budget = config.path_budget;
    let platform = StorePlatform::open(config).unwrap();
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
    StorePlatform::open(
        StoreBackendConfig::file(&root, ProfileId::DesktopMacosEmbeddedSdk).unwrap(),
    )
    .unwrap();

    let err = match StorePlatform::open(
        StoreBackendConfig::file(&root, ProfileId::ServerLinuxDevFull).unwrap(),
    ) {
        Ok(_) => panic!("existing store root must reject a different profile"),
        Err(error) => error,
    };

    assert_eq!(err.stage(), "file_store_manifest");
    assert!(err.to_string().contains("profile"));
}
