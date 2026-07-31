#![cfg(feature = "sqlite-store")]

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bm_core::budget::StoreRuntimeBudget;
use bm_sdk::nonproduction_replay_harness::StoreBackendConfig;

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
enum FootprintEntry {
    File(Vec<u8>),
    Symlink(PathBuf),
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "beetle-memory-sqlite-open-preflight-{label}-{}-{}",
        std::process::id(),
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn footprint(root: &Path) -> BTreeMap<PathBuf, FootprintEntry> {
    let mut entries = BTreeMap::new();
    for entry in std::fs::read_dir(root).expect("read fixture root") {
        let entry = entry.expect("read fixture entry");
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("fixture entry under root")
            .to_path_buf();
        let metadata = std::fs::symlink_metadata(&path).expect("read fixture entry metadata");
        if metadata.file_type().is_symlink() {
            entries.insert(
                relative,
                FootprintEntry::Symlink(std::fs::read_link(&path).expect("read fixture symlink")),
            );
        } else if metadata.is_file() {
            entries.insert(
                relative,
                FootprintEntry::File(std::fs::read(&path).expect("read fixture file")),
            );
        }
    }
    entries
}

fn constrained_budget() -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        metric_source_max_items: 1,
        event_log_max_items: 2,
        kv_max_entries: 256,
        blob_max_bytes: 1024,
        snapshot_max_bytes: 1024 * 1024,
        logical_namespace_max_bytes: 64,
        logical_key_max_bytes: 64,
        event_record_key_max_bytes: 64,
        export_max_bytes: 1024 * 1024,
        import_max_bytes: 1024 * 1024,
    }
}

fn constrained_config(path: &Path) -> StoreBackendConfig {
    StoreBackendConfig::sqlite(path, support::native_persistent_profile())
        .expect("sqlite config")
        .try_with_nonproduction_store_budget_limit(constrained_budget())
        .expect("valid constrained store budget")
}

fn initialized_database(path: &Path) {
    support::open_store(
        StoreBackendConfig::sqlite(path, support::native_persistent_profile())
            .expect("sqlite config"),
    )
    .expect("initialize sqlite fixture");
}

#[test]
fn sqlite_open_rejects_entry_count_before_decoding_any_overflow_row() {
    let root = temp_root("entry-count");
    std::fs::create_dir_all(&root).expect("create fixture root");
    let path = root.join("memory.sqlite3");
    initialized_database(&path);

    let mut connection = rusqlite::Connection::open(&path).expect("open sqlite fixture");
    let transaction = connection.transaction().expect("start fixture transaction");
    for sequence in 0..=256 {
        let raw = if sequence == 256 { "{" } else { "{}" };
        transaction
            .execute(
                "INSERT INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, ?3)",
                rusqlite::params!["overflow_fixture", format!("key-{sequence:03}"), raw],
            )
            .expect("insert overflow fixture row");
    }
    transaction.commit().expect("commit overflow fixture");
    drop(connection);
    let before = footprint(&root);

    let error = support::open_store(constrained_config(&path))
        .err()
        .expect("entry overflow must fail before malformed overflow row decoding");
    assert_eq!(error.stage(), "store_budget_exceeded");
    assert_eq!(footprint(&root), before);

    std::fs::remove_dir_all(root).expect("remove fixture root");
}

#[test]
fn sqlite_open_rejects_oversized_address_before_decoding_its_value() {
    for (label, namespace, key) in [
        ("namespace", "n".repeat(65), "key".to_string()),
        ("key", "oversized_fixture".to_string(), "k".repeat(65)),
    ] {
        let root = temp_root(&format!("address-{label}"));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let path = root.join("memory.sqlite3");
        initialized_database(&path);

        let connection = rusqlite::Connection::open(&path).expect("open sqlite fixture");
        connection
            .execute(
                "INSERT INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, '{')",
                rusqlite::params![namespace, key],
            )
            .expect("insert oversized address fixture");
        drop(connection);
        let before = footprint(&root);

        let error = support::open_store(constrained_config(&path))
            .err()
            .expect("oversized address must fail before malformed value decoding");
        assert_eq!(error.stage(), "store_budget_exceeded");
        assert_eq!(footprint(&root), before);

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }
}

#[test]
fn sqlite_open_rejects_event_count_before_decoding_any_overflow_event() {
    let root = temp_root("event-count");
    std::fs::create_dir_all(&root).expect("create fixture root");
    let path = root.join("memory.sqlite3");
    initialized_database(&path);

    let connection = rusqlite::Connection::open(&path).expect("open sqlite fixture");
    connection
        .execute(
            "INSERT INTO bm_event_log(event_id, event_json) VALUES ('overflow-event-a', '{')",
            [],
        )
        .expect("insert malformed overflow event");
    connection
        .execute(
            "INSERT INTO bm_event_log(event_id, event_json) VALUES ('overflow-event-b', '{')",
            [],
        )
        .expect("insert second malformed overflow event");
    drop(connection);
    let before = footprint(&root);

    let error = support::open_store(constrained_config(&path))
        .err()
        .expect("event overflow must fail before malformed overflow event decoding");
    assert_eq!(error.stage(), "store_budget_exceeded");
    assert_eq!(footprint(&root), before);

    std::fs::remove_dir_all(root).expect("remove fixture root");
}

#[test]
fn sqlite_open_rejects_oversized_event_id_before_decoding_its_event() {
    let root = temp_root("event-id-length");
    std::fs::create_dir_all(&root).expect("create fixture root");
    let path = root.join("memory.sqlite3");
    initialized_database(&path);

    let connection = rusqlite::Connection::open(&path).expect("open sqlite fixture");
    connection
        .execute(
            "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, '{')",
            ["e".repeat(65)],
        )
        .expect("insert oversized event id fixture");
    drop(connection);
    let before = footprint(&root);

    let error = support::open_store(constrained_config(&path))
        .err()
        .expect("oversized event id must fail before malformed event decoding");
    assert_eq!(error.stage(), "store_budget_exceeded");
    assert_eq!(footprint(&root), before);

    std::fs::remove_dir_all(root).expect("remove fixture root");
}

#[test]
fn sqlite_open_never_initializes_an_existing_header_only_database() {
    let root = temp_root("header-only");
    std::fs::create_dir_all(&root).expect("create fixture root");
    let path = root.join("memory.sqlite3");
    let connection = rusqlite::Connection::open(&path).expect("open header-only sqlite fixture");
    connection
        .execute_batch("VACUUM;")
        .expect("persist SQLite header");
    drop(connection);
    assert!(
        std::fs::metadata(&path).expect("header metadata").len() >= 100,
        "fixture must be a physical SQLite database, not a zero-byte placeholder"
    );
    let before = footprint(&root);

    let error = support::open_store(constrained_config(&path))
        .err()
        .expect("existing header-only database must not be initialized as fresh");
    assert_eq!(error.stage(), "sqlite_store_open_preflight");
    assert_eq!(footprint(&root), before);

    std::fs::remove_dir_all(root).expect("remove fixture root");
}

#[cfg(unix)]
#[test]
fn sqlite_open_rejects_dangling_sidecar_symlinks_without_creating_main() {
    use std::os::unix::fs::symlink;

    for suffix in ["-wal", "-shm", "-journal"] {
        let root = temp_root(&format!("dangling-sidecar-{}", &suffix[1..]));
        std::fs::create_dir_all(&root).expect("create fixture root");
        let path = root.join("memory.sqlite3");
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        symlink(root.join("missing-sidecar-target"), &sidecar)
            .expect("create dangling sidecar symlink");
        let before = footprint(&root);

        let error = support::open_store(constrained_config(&path))
            .err()
            .expect("dangling sidecar must fail closed");
        assert_eq!(error.stage(), "sqlite_store_open_preflight");
        assert_eq!(footprint(&root), before);
        assert!(!path.exists());

        std::fs::remove_dir_all(root).expect("remove fixture root");
    }
}

#[cfg(unix)]
#[test]
fn sqlite_open_rejects_a_dangling_main_symlink_without_creating_its_target() {
    use std::os::unix::fs::symlink;

    let root = temp_root("dangling-main");
    std::fs::create_dir_all(&root).expect("create fixture root");
    let path = root.join("memory.sqlite3");
    let target = root.join("missing-main-target");
    symlink(&target, &path).expect("create dangling main symlink");
    let before = footprint(&root);

    let error = support::open_store(constrained_config(&path))
        .err()
        .expect("dangling main symlink must fail closed");
    assert_eq!(error.stage(), "sqlite_store_open_preflight");
    assert_eq!(footprint(&root), before);
    assert!(!target.exists());

    std::fs::remove_dir_all(root).expect("remove fixture root");
}
