mod support;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::StoreBackendConfig;

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
