use bm_core::feature_gate::ProfileId;
use bm_core::platform::Platform;
use bm_store::{
    MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig, StoreEventLog, StoreEventScope,
    StorePlatform, StoreRepairPolicy,
};

#[test]
fn file_store_returns_structured_error_for_corrupt_json() {
    let root =
        std::env::temp_dir().join(format!("beetle-memory-corrupt-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let platform = StorePlatform::open(
        StoreBackendConfig::file(&root, ProfileId::DesktopMacosEmbeddedSdk).unwrap(),
    )
    .unwrap();
    platform
        .session_store()
        .append("chat-a", "user", "hello")
        .unwrap();

    let session_dir = root.join("kv").join("session");
    let corrupt_file = std::fs::read_dir(&session_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&corrupt_file, b"{not-json").unwrap();

    let err = platform
        .session_store()
        .load_recent("chat-a", 1)
        .expect_err("corrupt json must not be silently converted into empty state");
    assert_eq!(err.stage(), "file_store_json_read");
}

#[test]
fn file_store_reports_or_repairs_only_safe_orphan_tmp_files() {
    let root =
        std::env::temp_dir().join(format!("beetle-memory-orphan-tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let tmp = root.join("kv").join("session").join("orphan.tmp");
    std::fs::create_dir_all(tmp.parent().unwrap()).unwrap();
    std::fs::write(&tmp, b"partial").unwrap();

    let (_, report_only) = StorePlatform::open_with_report(
        StoreBackendConfig::file(&root, ProfileId::DesktopMacosEmbeddedSdk)
            .unwrap()
            .with_repair_policy(StoreRepairPolicy::ReportOnly),
    )
    .unwrap();
    assert!(report_only.repair.checked);
    assert!(!report_only.repair.repaired);
    assert!(tmp.exists());

    let (_, repaired) = StorePlatform::open_with_report(
        StoreBackendConfig::file(&root, ProfileId::DesktopMacosEmbeddedSdk)
            .unwrap()
            .with_repair_policy(StoreRepairPolicy::RepairSafe),
    )
    .unwrap();
    assert!(repaired.repair.repaired);
    assert!(!tmp.exists());
}

#[test]
fn file_store_rejects_truncated_jsonl_and_duplicate_events() {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-corrupt-jsonl-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let platform = StorePlatform::open(
        StoreBackendConfig::file(&root, ProfileId::DesktopMacosEmbeddedSdk).unwrap(),
    )
    .unwrap();
    let event = MemoryStoreEvent::new(
        "evt-file-dup",
        MemoryStoreEventKind::RuntimeLifecycle,
        StoreEventScope::system("dup"),
        1,
    );
    platform.append_event(event.clone()).unwrap();
    let err = platform
        .append_event(event)
        .expect_err("duplicate event id must be rejected");
    assert_eq!(err.stage(), "store_event_log");

    std::fs::write(root.join("events").join("events.jsonl"), b"{not-json\n").unwrap();
    let err = platform
        .read_events()
        .expect_err("truncated jsonl must not be silently ignored");
    assert_eq!(err.stage(), "store_event_log");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_store_rejects_unknown_schema_and_duplicate_events() {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-sqlite-corrupt-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("memory.sqlite3");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE bm_schema (
                    schema_id TEXT PRIMARY KEY,
                    schema_version INTEGER NOT NULL,
                    manifest_json TEXT NOT NULL
                );
                INSERT INTO bm_schema(schema_id, schema_version, manifest_json)
                VALUES ('old_schema', 1, '{}');
                "#,
            )
            .unwrap();
    }

    let err = match StorePlatform::open(
        StoreBackendConfig::sqlite(&path, ProfileId::ServerLinuxMemoryGateway).unwrap(),
    ) {
        Ok(_) => panic!("unknown sqlite schema must be rejected"),
        Err(error) => error,
    };
    assert_eq!(err.stage(), "sqlite_store_schema");

    let valid_path = root.join("valid.sqlite3");
    let platform = StorePlatform::open(
        StoreBackendConfig::sqlite(&valid_path, ProfileId::ServerLinuxMemoryGateway).unwrap(),
    )
    .unwrap();
    let event = MemoryStoreEvent::new(
        "evt-sqlite-dup",
        MemoryStoreEventKind::RuntimeLifecycle,
        StoreEventScope::system("dup"),
        1,
    );
    platform.append_event(event.clone()).unwrap();
    let err = platform
        .append_event(event)
        .expect_err("duplicate event id must be rejected");
    assert_eq!(err.stage(), "store_event_log");
}

#[test]
fn embedded_store_returns_budget_error_instead_of_dropping_data() {
    let mut config = StoreBackendConfig::embedded(ProfileId::EspEmbeddedSdk).unwrap();
    config.capacity.blob_max_bytes = 1;
    let platform = StorePlatform::open(config).unwrap();

    let err = platform
        .state_fs()
        .write("too-large", b"12")
        .expect_err("embedded budget overflow must be explicit");
    assert_eq!(err.stage(), "store_budget_exceeded");
}

#[test]
fn embedded_store_bounds_event_ring_without_dropping_state() {
    let mut config = StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory).unwrap();
    config.capacity.event_log_max_items = 1;
    let platform = StorePlatform::open(config).unwrap();

    platform.state_fs().write("one", b"1").unwrap();
    platform.state_fs().write("two", b"2").unwrap();

    assert_eq!(platform.read_events().unwrap().len(), 1);
    assert_eq!(
        platform.state_fs().read("one").unwrap(),
        Some(b"1".to_vec())
    );
    assert_eq!(
        platform.state_fs().read("two").unwrap(),
        Some(b"2".to_vec())
    );
}
