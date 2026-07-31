mod support;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bm_core::budget::StoreRuntimeBudget;
use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryKind, MEMORY_FACET_INDEX_NAMESPACE};
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{
    StoreBackendConfig, StorePlatform, StoreRepairPolicy, RUNTIME_SKILL_RECORD_NAMESPACE,
};
use bm_sdk::MemoryPrivacyClass;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
enum PersistentStateKind {
    Kv,
    Blob,
    Event,
    Snapshot,
}

impl PersistentStateKind {
    const ALL: [Self; 4] = [Self::Kv, Self::Blob, Self::Event, Self::Snapshot];

    const fn name(self) -> &'static str {
        match self {
            Self::Kv => "kv",
            Self::Blob => "blob",
            Self::Event => "event",
            Self::Snapshot => "snapshot",
        }
    }
}

static TEMP_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temp_root(backend: &str, scenario: &str, state: PersistentStateKind) -> PathBuf {
    let sequence = TEMP_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-{backend}-manifest-admission-{scenario}-{}-{}-{sequence}",
        state.name(),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn directory_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = std::fs::read_dir(current)
            .unwrap_or_else(|error| panic!("read {}: {error}", current.display()))
            .map(|entry| entry.expect("read directory entry"))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                collect(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root)
                        .expect("path under root")
                        .to_path_buf(),
                    std::fs::read(&path)
                        .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FootprintEntry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum JournalState {
    Committed,
}

#[derive(Serialize)]
struct JournalJsonValue {
    namespace: String,
    key: String,
    value: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct JournalBlobValue {
    namespace: String,
    key: String,
    value: Option<Vec<u8>>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JournalEventsImage {
    Append {
        prefix_len: u64,
        events: Vec<serde_json::Value>,
    },
}

#[derive(Serialize)]
struct JournalImage {
    json: Vec<JournalJsonValue>,
    blobs: Vec<JournalBlobValue>,
    events: JournalEventsImage,
}

#[derive(Serialize)]
struct Journal {
    schema_version: u64,
    transaction_id: String,
    state: JournalState,
    before: JournalImage,
    after: JournalImage,
    checksum: String,
}

fn write_committed_runtime_skill_journal(
    root: &Path,
    physical_key: &str,
    before_value: serde_json::Value,
    after_value: serde_json::Value,
) {
    write_committed_runtime_skill_journal_with_prefix(
        root,
        physical_key,
        before_value,
        after_value,
        None,
    );
}

fn write_committed_runtime_skill_journal_with_prefix(
    root: &Path,
    physical_key: &str,
    before_value: serde_json::Value,
    after_value: serde_json::Value,
    event_prefix_len: Option<u64>,
) {
    let event_prefix_len = event_prefix_len.unwrap_or_else(|| {
        std::fs::metadata(root.join("events").join("events.jsonl"))
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    });
    let image = |value| JournalImage {
        json: vec![JournalJsonValue {
            namespace: RUNTIME_SKILL_RECORD_NAMESPACE.to_string(),
            key: physical_key.to_string(),
            value: Some(value),
        }],
        blobs: Vec::new(),
        events: JournalEventsImage::Append {
            prefix_len: event_prefix_len,
            events: Vec::new(),
        },
    };
    let transaction_id = "file-open-preflight-committed-journal".to_string();
    let state = JournalState::Committed;
    let before = image(before_value);
    let after = image(after_value);
    let checksum_input = serde_json::to_vec(&(2_u64, &transaction_id, state, &before, &after))
        .expect("serialize file journal checksum input");
    let journal = Journal {
        schema_version: 2,
        transaction_id,
        state,
        before,
        after,
        checksum: format!("{:x}", Sha256::digest(checksum_input)),
    };
    std::fs::write(
        root.join(".beetle-memory.transaction"),
        serde_json::to_vec(&journal).expect("serialize committed file journal"),
    )
    .expect("write committed file journal");
}

fn directory_footprint(root: &Path) -> BTreeMap<PathBuf, FootprintEntry> {
    fn collect(root: &Path, current: &Path, entries: &mut BTreeMap<PathBuf, FootprintEntry>) {
        let mut children = std::fs::read_dir(current)
            .unwrap_or_else(|error| panic!("read {}: {error}", current.display()))
            .map(|entry| entry.expect("read directory entry"))
            .collect::<Vec<_>>();
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let path = child.path();
            let relative = path
                .strip_prefix(root)
                .expect("path under root")
                .to_path_buf();
            let metadata = std::fs::symlink_metadata(&path)
                .unwrap_or_else(|error| panic!("metadata {}: {error}", path.display()));
            if metadata.file_type().is_symlink() {
                entries.insert(
                    relative,
                    FootprintEntry::Symlink(
                        std::fs::read_link(&path).unwrap_or_else(|error| {
                            panic!("read link {}: {error}", path.display())
                        }),
                    ),
                );
            } else if metadata.is_dir() {
                entries.insert(relative, FootprintEntry::Directory);
                collect(root, &path, entries);
            } else {
                entries.insert(
                    relative,
                    FootprintEntry::File(
                        std::fs::read(&path)
                            .unwrap_or_else(|error| panic!("read {}: {error}", path.display())),
                    ),
                );
            }
        }
    }

    let mut entries = BTreeMap::new();
    collect(root, root, &mut entries);
    entries
}

fn hex_encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn seed_and_tamper_facet(platform: &StorePlatform) {
    support::seed_scoped_long_term(
        platform,
        "space:test",
        &LongTermMemoryDraft {
            kind: LongTermMemoryKind::Fact,
            privacy: MemoryPrivacyClass::SharedWithSubject,
            topic: "open preflight facet".to_string(),
            content: "The complete store validator must own facet closure.".to_string(),
            keywords: vec!["facet".to_string()],
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
    let facets = platform
        .read_json_namespace(MEMORY_FACET_INDEX_NAMESPACE)
        .expect("read seeded facet documents");
    let facet = facets
        .first()
        .expect("seeded owner must have a facet document");
    platform
        .tamper_json_document_for_nonproduction_harness(
            MEMORY_FACET_INDEX_NAMESPACE,
            &facet.key,
            serde_json::json!({"unexpected": true}),
        )
        .expect("tamper facet document");
}

fn exact_open_event_budget() -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        metric_source_max_items: 1,
        event_log_max_items: 2,
        kv_max_entries: 256,
        blob_max_bytes: 4,
        snapshot_max_bytes: 1024 * 1024,
        logical_namespace_max_bytes: 64,
        logical_key_max_bytes: 64,
        event_record_key_max_bytes: 64,
        export_max_bytes: 1024 * 1024,
        import_max_bytes: 1024 * 1024,
    }
}

fn exact_snapshot_budget(snapshot_max_bytes: usize) -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        metric_source_max_items: 1,
        event_log_max_items: 8,
        kv_max_entries: 256,
        blob_max_bytes: 4,
        snapshot_max_bytes,
        logical_namespace_max_bytes: 64,
        logical_key_max_bytes: 64,
        event_record_key_max_bytes: 64,
        export_max_bytes: 1024 * 1024,
        import_max_bytes: 1024 * 1024,
    }
}

#[cfg(feature = "sqlite-store")]
fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

fn seed_file_state_without_manifest(root: &Path, state: PersistentStateKind) {
    let platform = support::open_store(
        StoreBackendConfig::file(root, support::native_persistent_profile()).expect("file config"),
    )
    .expect("initialize file store");
    match state {
        PersistentStateKind::Kv => platform
            .session_store()
            .append("manifest-admission", "user", "persisted kv state")
            .expect("seed kv state"),
        PersistentStateKind::Blob => platform
            .state_fs()
            .write("manifest-admission.bin", b"persisted blob state")
            .expect("seed blob state"),
        PersistentStateKind::Event | PersistentStateKind::Snapshot => {}
    }
    drop(platform);

    if !matches!(state, PersistentStateKind::Event) {
        let events = root.join("events").join("events.jsonl");
        if events.exists() {
            std::fs::remove_file(events).expect("remove unrelated events");
        }
    }
    if matches!(state, PersistentStateKind::Snapshot) {
        std::fs::write(
            root.join("snapshots").join("persisted.snapshot"),
            b"persisted snapshot state",
        )
        .expect("seed snapshot state");
    }
    std::fs::remove_file(root.join("manifest.json")).expect("remove file manifest");
}

#[test]
fn file_store_rejects_missing_manifest_when_any_persistent_state_exists_without_mutation() {
    for state in PersistentStateKind::ALL {
        let root = temp_root("file", "missing", state);
        seed_file_state_without_manifest(&root, state);
        let before = directory_bytes(&root);

        let error = match support::open_store(
            StoreBackendConfig::file(&root, support::native_persistent_profile())
                .expect("file config"),
        ) {
            Ok(_) => panic!(
                "file store must reject missing manifest with {} state",
                state.name()
            ),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "file_store_manifest", "state={state:?}");
        assert!(!root.join("manifest.json").exists(), "state={state:?}");
        assert_eq!(directory_bytes(&root), before, "state={state:?}");
        std::fs::remove_dir_all(root).expect("remove file test root");
    }
}

#[test]
fn file_store_rejects_unknown_manifest_fields_without_rewriting_bytes() {
    let root = temp_root("file", "unknown-field", PersistentStateKind::Kv);
    support::open_store(
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config"),
    )
    .expect("initialize file store");
    let manifest_path = root.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read initialized manifest"))
            .expect("decode initialized manifest");
    manifest
        .as_object_mut()
        .expect("manifest object")
        .insert("unknownAuthorityField".to_string(), serde_json::json!(true));
    let tampered = serde_json::to_vec_pretty(&manifest).expect("encode unknown-field manifest");
    std::fs::write(&manifest_path, &tampered).expect("write unknown-field manifest");

    let error = match support::open_store(
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config"),
    ) {
        Ok(_) => panic!("unknown manifest fields must fail closed"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_manifest");
    assert_eq!(
        std::fs::read(&manifest_path).expect("read rejected manifest"),
        tampered,
        "admission failure must not normalize or rewrite unknown schema"
    );
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_rejects_malformed_typed_owner_before_any_open_mutation() {
    let root = temp_root("file", "malformed-typed-owner", PersistentStateKind::Kv);
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile())
        .expect("file config")
        .with_repair_policy(StoreRepairPolicy::RepairSafe);
    let platform = support::open_store(config.clone()).expect("initialize file store");
    let owner = support::seed_runtime_skill(&platform, "file-open-preflight");
    platform
        .tamper_json_document_for_nonproduction_harness(
            RUNTIME_SKILL_RECORD_NAMESPACE,
            &owner.physical_key,
            serde_json::json!({"unexpected": true}),
        )
        .expect("tamper typed runtime skill owner");
    drop(platform);
    let orphan_tmp = root
        .join("kv")
        .join(RUNTIME_SKILL_RECORD_NAMESPACE)
        .join("orphan.tmp");
    std::fs::write(&orphan_tmp, b"must survive rejected preflight")
        .expect("write repair-safe orphan fixture");
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("malformed typed owner must fail before file store open mutates state"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_open_preflight");
    assert_eq!(
        directory_footprint(&root),
        before,
        "failed open admission must preserve the complete file-store footprint"
    );
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_rejects_malformed_typed_journal_overlay_before_recovery_mutation() {
    let root = temp_root("file", "malformed-journal-overlay", PersistentStateKind::Kv);
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    let platform = support::open_store(config.clone()).expect("initialize file store");
    let owner = support::seed_runtime_skill(&platform, "file-journal-preflight");
    let owner_value = serde_json::to_value(&owner).expect("serialize typed runtime skill owner");
    drop(platform);
    write_committed_runtime_skill_journal(
        &root,
        &owner.physical_key,
        owner_value,
        serde_json::json!({"unexpected": true}),
    );
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("malformed typed journal overlay must fail before recovery"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_open_preflight");
    assert_eq!(
        directory_footprint(&root),
        before,
        "failed journal admission must preserve the complete file-store footprint"
    );
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_journal_cannot_mask_a_malformed_physical_typed_owner() {
    let root = temp_root("file", "journal-mask", PersistentStateKind::Kv);
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    let platform = support::open_store(config.clone()).expect("initialize file store");
    let owner = support::seed_runtime_skill(&platform, "file-journal-physical-mask");
    let owner_value = serde_json::to_value(&owner).expect("serialize typed runtime skill owner");
    platform
        .tamper_json_document_for_nonproduction_harness(
            RUNTIME_SKILL_RECORD_NAMESPACE,
            &owner.physical_key,
            serde_json::json!({"unexpected": true}),
        )
        .expect("tamper physical typed owner");
    drop(platform);
    write_committed_runtime_skill_journal(
        &root,
        &owner.physical_key,
        owner_value.clone(),
        owner_value,
    );
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("valid journal images must not mask malformed physical typed bytes"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_open_preflight");
    assert_eq!(directory_footprint(&root), before);
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_rejects_a_missing_fixed_lock_without_creating_it() {
    let root = temp_root("file", "missing-fixed-lock", PersistentStateKind::Kv);
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    support::open_store(config.clone()).expect("initialize file store");
    let lock = root.join(".beetle-memory.lock");
    std::fs::remove_file(&lock).expect("remove fixed lock fixture");
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("existing store without its fixed lock owner must fail closed"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_open_preflight");
    assert_eq!(directory_footprint(&root), before);
    assert!(!lock.exists());
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_rejects_reopen_without_open_event_capacity_and_preserves_bytes() {
    let root = temp_root("file", "open-event-capacity", PersistentStateKind::Event);
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile())
        .expect("file config")
        .try_with_nonproduction_store_budget_limit(exact_open_event_budget())
        .expect("valid exact open-event budget");
    let platform = support::open_store(config.clone()).expect("initialize file store");
    platform
        .state_fs()
        .write("a", b"1")
        .expect("fill the final event slot");
    drop(platform);
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("reopen must reserve its required lifecycle event before mutation"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_open_preflight");
    assert_eq!(directory_footprint(&root), before);
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_rejects_reopen_without_open_event_byte_capacity_and_preserves_bytes() {
    let root = temp_root(
        "file",
        "open-event-byte-capacity",
        PersistentStateKind::Event,
    );
    let base_config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    support::open_store(base_config.clone()).expect("initialize file store");
    let event_log = root.join("events").join("events.jsonl");
    let first_open_event_bytes = usize::try_from(
        std::fs::metadata(&event_log)
            .expect("event log metadata")
            .len(),
    )
    .expect("event log length");
    assert!(first_open_event_bytes > 1);
    let config = base_config
        .try_with_nonproduction_store_budget_limit(exact_snapshot_budget(
            first_open_event_bytes
                .checked_mul(2)
                .and_then(|value| value.checked_sub(1))
                .expect("exact event byte budget"),
        ))
        .expect("valid exact event byte budget");
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("reopen must reserve the required lifecycle event bytes before mutation"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_open_preflight");
    assert_eq!(directory_footprint(&root), before);
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_rejects_journal_event_prefix_between_json_and_newline_without_mutation() {
    let root = temp_root(
        "file",
        "journal-event-prefix-boundary",
        PersistentStateKind::Event,
    );
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    let platform = support::open_store(config.clone()).expect("initialize file store");
    let owner = support::seed_runtime_skill(&platform, "file-journal-event-prefix");
    let owner_value = serde_json::to_value(&owner).expect("serialize runtime skill owner");
    drop(platform);
    let events_path = root.join("events").join("events.jsonl");
    let events = std::fs::read(&events_path).expect("read event log");
    assert_eq!(events.last(), Some(&b'\n'));
    write_committed_runtime_skill_journal_with_prefix(
        &root,
        &owner.physical_key,
        owner_value.clone(),
        owner_value,
        Some(u64::try_from(events.len() - 1).expect("event prefix length")),
    );
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("journal event prefix must end on an exact JSONL record boundary"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_open_preflight");
    assert_eq!(directory_footprint(&root), before);
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_rejects_unbounded_preflight_directory_entries_without_mutation() {
    let root = temp_root(
        "file",
        "preflight-directory-entry-budget",
        PersistentStateKind::Snapshot,
    );
    let config = StoreBackendConfig::file(&root, support::native_persistent_profile())
        .expect("file config")
        .try_with_nonproduction_store_budget_limit(exact_snapshot_budget(1024 * 1024))
        .expect("bounded preflight directory budget");
    support::open_store(config.clone()).expect("initialize file store");
    let snapshots = root.join("snapshots");
    for sequence in 0..300 {
        std::fs::write(
            snapshots.join(format!("orphan-{sequence:04}.tmp")),
            b"orphan",
        )
        .expect("write orphan tmp");
    }
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("preflight must bound directory entries before collecting them"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_open_preflight");
    assert_eq!(directory_footprint(&root), before);
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_rejects_legacy_physical_json_address_before_any_open_mutation() {
    let root = temp_root("file", "legacy-physical-json", PersistentStateKind::Kv);
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    let platform = support::open_store(config.clone()).expect("initialize file store");
    let owner = support::seed_runtime_skill(&platform, "file-physical-preflight");
    drop(platform);
    let legacy_path = root
        .join("kv")
        .join(RUNTIME_SKILL_RECORD_NAMESPACE)
        .join(format!("{}.json", hex_encode(&owner.physical_key)));
    std::fs::write(
        &legacy_path,
        serde_json::to_vec(&owner).expect("serialize duplicate legacy owner"),
    )
    .expect("write legacy physical JSON address");
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("legacy physical JSON address must fail before file store open"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_open_preflight");
    assert_eq!(
        directory_footprint(&root),
        before,
        "legacy addressing rejection must preserve the complete file-store footprint"
    );
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_rejects_malformed_facet_before_any_open_mutation() {
    let root = temp_root("file", "malformed-facet", PersistentStateKind::Kv);
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    let platform = support::open_store(config.clone()).expect("initialize file store");
    seed_and_tamper_facet(&platform);
    drop(platform);
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("malformed facet must fail complete-store open admission"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "file_store_open_preflight");
    assert_eq!(directory_footprint(&root), before);
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[test]
fn file_store_rejects_unknown_journal_fields_before_any_open_mutation() {
    let root = temp_root("file", "unknown-journal-field", PersistentStateKind::Kv);
    let config =
        StoreBackendConfig::file(&root, support::native_persistent_profile()).expect("file config");
    let platform = support::open_store(config.clone()).expect("initialize file store");
    let owner = support::seed_runtime_skill(&platform, "file-journal-exact-schema");
    let owner_value = serde_json::to_value(&owner).expect("serialize runtime skill owner");
    drop(platform);
    write_committed_runtime_skill_journal(
        &root,
        &owner.physical_key,
        owner_value.clone(),
        owner_value,
    );
    let marker = root.join(".beetle-memory.transaction");
    let mut journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&marker).expect("read journal"))
            .expect("decode journal");
    journal
        .as_object_mut()
        .expect("journal object")
        .insert("unknown_authority".to_string(), serde_json::json!(true));
    std::fs::write(
        &marker,
        serde_json::to_vec(&journal).expect("encode unknown-field journal"),
    )
    .expect("write unknown-field journal");
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("unknown journal fields must fail exact-schema admission"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "memory_write_transaction_repair_required");
    assert_eq!(directory_footprint(&root), before);
    std::fs::remove_dir_all(root).expect("remove file test root");
}

#[cfg(feature = "sqlite-store")]
fn seed_sqlite_state_without_schema(path: &Path, state: PersistentStateKind) {
    support::open_store(
        StoreBackendConfig::sqlite(path, support::native_persistent_profile())
            .expect("sqlite config"),
    )
    .expect("initialize sqlite store");

    let connection = rusqlite::Connection::open(path).expect("open sqlite fixture");
    connection
        .execute_batch(
            r#"
            BEGIN IMMEDIATE;
            DELETE FROM bm_kv;
            DELETE FROM bm_blob;
            DELETE FROM bm_event_log;
            DELETE FROM bm_snapshot_manifest;
            "#,
        )
        .expect("clear sqlite state tables");
    match state {
        PersistentStateKind::Kv => connection
            .execute(
                "INSERT INTO bm_kv(namespace, key, value_json) VALUES ('test', 'kv', '{}')",
                [],
            )
            .expect("seed sqlite kv"),
        PersistentStateKind::Blob => connection
            .execute(
                "INSERT INTO bm_blob(namespace, key, value_blob) VALUES ('test', 'blob', X'01')",
                [],
            )
            .expect("seed sqlite blob"),
        PersistentStateKind::Event => connection
            .execute(
                "INSERT INTO bm_event_log(event_id, event_json) VALUES ('test-event', '{}')",
                [],
            )
            .expect("seed sqlite event"),
        PersistentStateKind::Snapshot => connection
            .execute(
                "INSERT INTO bm_snapshot_manifest(snapshot_id, manifest_json) VALUES ('test', '{}')",
                [],
            )
            .expect("seed sqlite snapshot manifest"),
    };
    connection
        .execute("DELETE FROM bm_schema", [])
        .expect("remove sqlite schema row");
    connection.execute_batch("COMMIT;").expect("commit fixture");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_missing_schema_when_any_persistent_state_exists_without_mutation() {
    for state in PersistentStateKind::ALL {
        let root = temp_root("sqlite", "missing", state);
        std::fs::create_dir_all(&root).expect("create sqlite test root");
        let path = root.join("memory.sqlite3");
        seed_sqlite_state_without_schema(&path, state);
        let before = directory_bytes(&root);

        let error = match support::open_store(
            StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
                .expect("sqlite config"),
        ) {
            Ok(_) => panic!(
                "sqlite store must reject missing schema with {} state",
                state.name()
            ),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "sqlite_store_schema", "state={state:?}");
        assert_eq!(directory_bytes(&root), before, "state={state:?}");

        let connection = rusqlite::Connection::open(&path).expect("reopen sqlite fixture");
        let schema_rows: usize = connection
            .query_row("SELECT COUNT(*) FROM bm_schema", [], |row| row.get(0))
            .expect("count schema rows");
        let state_rows: usize = connection
            .query_row(
                r#"
                SELECT
                    (SELECT COUNT(*) FROM bm_kv) +
                    (SELECT COUNT(*) FROM bm_blob) +
                    (SELECT COUNT(*) FROM bm_event_log) +
                    (SELECT COUNT(*) FROM bm_snapshot_manifest)
                "#,
                [],
                |row| row.get(0),
            )
            .expect("count persistent state rows");
        assert_eq!(schema_rows, 0, "state={state:?}");
        assert_eq!(state_rows, 1, "state={state:?}");
        drop(connection);
        std::fs::remove_dir_all(root).expect("remove sqlite test root");
    }
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_unknown_manifest_fields_without_rewriting_schema_row() {
    let root = temp_root("sqlite", "unknown-field", PersistentStateKind::Kv);
    std::fs::create_dir_all(&root).expect("create sqlite test root");
    let path = root.join("memory.sqlite3");
    support::open_store(
        StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
            .expect("sqlite config"),
    )
    .expect("initialize sqlite store");

    let connection = rusqlite::Connection::open(&path).expect("open sqlite fixture");
    let initialized: String = connection
        .query_row("SELECT manifest_json FROM bm_schema", [], |row| row.get(0))
        .expect("read initialized schema manifest");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&initialized).expect("decode schema manifest");
    manifest
        .as_object_mut()
        .expect("manifest object")
        .insert("unknownAuthorityField".to_string(), serde_json::json!(true));
    let tampered = serde_json::to_string(&manifest).expect("encode unknown-field manifest");
    connection
        .execute("UPDATE bm_schema SET manifest_json = ?1", [&tampered])
        .expect("tamper schema manifest");
    drop(connection);

    let error = match support::open_store(
        StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
            .expect("sqlite config"),
    ) {
        Ok(_) => panic!("unknown schema manifest fields must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.stage(), "sqlite_store_schema");

    let connection = rusqlite::Connection::open(&path).expect("reopen rejected sqlite fixture");
    let after: String = connection
        .query_row("SELECT manifest_json FROM bm_schema", [], |row| row.get(0))
        .expect("read rejected schema manifest");
    assert_eq!(after, tampered, "admission failure must not rewrite schema");
    drop(connection);
    std::fs::remove_dir_all(root).expect("remove sqlite test root");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_malformed_typed_owner_before_any_open_mutation() {
    let root = temp_root("sqlite", "malformed-typed-owner", PersistentStateKind::Kv);
    std::fs::create_dir_all(&root).expect("create sqlite test root");
    let path = root.join("memory.sqlite3");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    let platform = support::open_store(config.clone()).expect("initialize sqlite store");
    let owner = support::seed_runtime_skill(&platform, "sqlite-open-preflight");
    drop(platform);

    let connection = rusqlite::Connection::open(&path).expect("open sqlite fixture");
    connection
        .execute(
            "UPDATE bm_kv SET value_json = ?1 WHERE namespace = ?2 AND key = ?3",
            rusqlite::params![
                serde_json::json!({"unexpected": true}).to_string(),
                RUNTIME_SKILL_RECORD_NAMESPACE,
                &owner.physical_key,
            ],
        )
        .expect("tamper typed sqlite owner");
    drop(connection);
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("malformed typed owner must fail before SQLite store open mutates state"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "sqlite_store_open_preflight");
    assert_eq!(
        directory_footprint(&root),
        before,
        "failed SQLite admission must preserve the complete database footprint"
    );
    std::fs::remove_dir_all(root).expect("remove sqlite test root");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_sidecars_before_opening_the_database() {
    for suffix in ["-wal", "-shm", "-journal"] {
        let root = temp_root(
            "sqlite",
            &format!("sidecar-{}", suffix.trim_start_matches('-')),
            PersistentStateKind::Kv,
        );
        std::fs::create_dir_all(&root).expect("create sqlite test root");
        let path = root.join("memory.sqlite3");
        let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
            .expect("sqlite config");
        support::open_store(config.clone()).expect("initialize sqlite store");
        let sidecar = sqlite_sidecar_path(&path, suffix);
        std::fs::write(&sidecar, format!("forbidden{suffix}").as_bytes())
            .expect("write forbidden SQLite sidecar");
        let before = directory_footprint(&root);

        let error = match support::open_store(config) {
            Ok(_) => panic!("{suffix} must fail before SQLite opens the database"),
            Err(error) => error,
        };

        assert_eq!(
            error.stage(),
            "sqlite_store_open_preflight",
            "suffix={suffix}: {error}"
        );
        assert_eq!(
            directory_footprint(&root),
            before,
            "sidecar rejection must not change any SQLite footprint bytes"
        );
        std::fs::remove_dir_all(root).expect("remove sqlite test root");
    }
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_sidecars_even_when_main_database_is_missing() {
    for suffix in ["-wal", "-shm", "-journal"] {
        let root = temp_root(
            "sqlite",
            &format!("missing-main-{}", suffix.trim_start_matches('-')),
            PersistentStateKind::Kv,
        );
        std::fs::create_dir_all(&root).expect("create sqlite test root");
        let path = root.join("memory.sqlite3");
        let sidecar = sqlite_sidecar_path(&path, suffix);
        std::fs::write(&sidecar, b"forbidden orphan sidecar").expect("write orphan sidecar");
        let before = directory_footprint(&root);
        let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
            .expect("sqlite config");

        let error = match support::open_store(config) {
            Ok(_) => panic!("orphan {suffix} must fail before a main database is created"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "sqlite_store_open_preflight");
        assert_eq!(directory_footprint(&root), before);
        assert!(!path.exists());
        std::fs::remove_dir_all(root).expect("remove sqlite test root");
    }
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_an_existing_zero_byte_database_without_mutation() {
    let root = temp_root("sqlite", "zero-byte-main", PersistentStateKind::Kv);
    std::fs::create_dir_all(&root).expect("create sqlite test root");
    let path = root.join("memory.sqlite3");
    std::fs::write(&path, []).expect("write zero-byte existing database");
    let before = directory_footprint(&root);
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");

    let error = match support::open_store(config) {
        Ok(_) => panic!("an existing zero-byte database must not be initialized in place"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "sqlite_store_open_preflight");
    assert_eq!(directory_footprint(&root), before);
    std::fs::remove_dir_all(root).expect("remove sqlite test root");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_reopen_without_open_event_capacity_and_preserves_bytes() {
    let root = temp_root("sqlite", "open-event-capacity", PersistentStateKind::Event);
    std::fs::create_dir_all(&root).expect("create sqlite test root");
    let path = root.join("memory.sqlite3");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config")
        .try_with_nonproduction_store_budget_limit(exact_open_event_budget())
        .expect("valid exact open-event budget");
    let platform = support::open_store(config.clone()).expect("initialize sqlite store");
    platform
        .state_fs()
        .write("a", b"1")
        .expect("fill the final event slot");
    drop(platform);
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("reopen must reserve its required lifecycle event before mutation"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "sqlite_store_open_preflight");
    assert_eq!(directory_footprint(&root), before);
    std::fs::remove_dir_all(root).expect("remove sqlite test root");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_malformed_facet_before_any_open_mutation() {
    let root = temp_root("sqlite", "malformed-facet", PersistentStateKind::Kv);
    std::fs::create_dir_all(&root).expect("create sqlite test root");
    let path = root.join("memory.sqlite3");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    let platform = support::open_store(config.clone()).expect("initialize sqlite store");
    seed_and_tamper_facet(&platform);
    drop(platform);
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("malformed facet must fail complete-store open admission"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "sqlite_store_open_preflight");
    assert_eq!(directory_footprint(&root), before);
    std::fs::remove_dir_all(root).expect("remove sqlite test root");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_noncanonical_ddl_before_any_open_mutation() {
    let root = temp_root("sqlite", "noncanonical-ddl", PersistentStateKind::Kv);
    std::fs::create_dir_all(&root).expect("create sqlite test root");
    let path = root.join("memory.sqlite3");
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .expect("sqlite config");
    support::open_store(config.clone()).expect("initialize sqlite store");
    let connection = rusqlite::Connection::open(&path).expect("open sqlite fixture");
    connection
        .execute_batch("CREATE TABLE unexpected_owner(id TEXT PRIMARY KEY);")
        .expect("add noncanonical table");
    drop(connection);
    let before = directory_footprint(&root);

    let error = match support::open_store(config) {
        Ok(_) => panic!("noncanonical SQLite DDL must fail before store open"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "sqlite_store_open_preflight");
    assert_eq!(
        directory_footprint(&root),
        before,
        "DDL rejection must preserve the complete SQLite footprint"
    );
    std::fs::remove_dir_all(root).expect("remove sqlite test root");
}
