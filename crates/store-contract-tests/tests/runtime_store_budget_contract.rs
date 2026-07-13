use bm_core::budget::StoreRuntimeBudget;
use bm_core::feature_gate::ProfileId;
use bm_core::platform::Platform as _;
use bm_sdk::nonproduction_replay_harness::{
    InMemoryStoreEngine, MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig,
    StoreBackendKind, StoreCapacityBudget, StoreEngine, StoreEventLog, StoreEventScope,
    StorePlatform, StoreSnapshot, StoreSnapshotBlob,
};

fn tiny_store_budget() -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        event_log_max_items: 8,
        kv_max_entries: 8,
        blob_max_bytes: 4,
        snapshot_max_bytes: 128,
        logical_namespace_max_bytes: 64,
        logical_key_max_bytes: 32,
        event_record_key_max_bytes: 32,
        export_max_bytes: 128,
        import_max_bytes: 128,
    }
}

#[test]
fn in_memory_store_consumes_compiled_blob_budget() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(tiny_store_budget());
    let platform = StorePlatform::open(config).expect("store");

    let err = platform
        .state_fs()
        .write("too-large.bin", b"12345")
        .expect_err("blob budget must reject oversized writes");

    assert_eq!(err.stage(), "store_budget_exceeded");
}

#[test]
fn file_store_consumes_compiled_snapshot_budget() {
    let root = std::env::temp_dir().join(format!(
        "bm-store-budget-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = StoreBackendConfig::file(&root, ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(tiny_store_budget());
    let platform = StorePlatform::open(config).expect("store");
    platform
        .memory_store()
        .set_memory("1234")
        .expect("seed small blob");

    let err = platform
        .export_store_snapshot()
        .expect_err("snapshot budget must apply to file backend");
    assert_eq!(err.stage(), "store_budget_exceeded");
}

#[test]
fn static_store_config_capacity_comes_from_runtime_budget_compiler() {
    let config = StoreBackendConfig::in_memory(ProfileId::EspEmbeddedSdk).expect("config");
    assert!(config.capacity.snapshot_max_bytes <= 256 * 1024);
    assert_eq!(config.backend, StoreBackendKind::InMemory);
}

#[test]
fn store_platform_rejects_logical_keys_that_exceed_runtime_budget() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(tiny_store_budget());
    let platform = StorePlatform::open(config).expect("store");
    let oversized_key = "k".repeat(33);

    let err = platform
        .state_fs()
        .write(&oversized_key, b"1")
        .expect_err("logical key budget must reject oversized keys before persistence");

    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("logical key"));
    assert!(!platform
        .state_fs()
        .list_dir("")
        .unwrap()
        .contains(&oversized_key));
}

#[test]
fn in_memory_store_platform_rejects_event_log_overflow_without_persisting_state() {
    let mut budget = tiny_store_budget();
    budget.event_log_max_items = 2;
    let config = StoreBackendConfig::in_memory(ProfileId::EspEmbeddedSdk)
        .expect("config")
        .with_runtime_store_budget(budget);
    let platform = StorePlatform::open(config).expect("store");

    platform
        .state_fs()
        .write("first", b"1")
        .expect("open event plus one write reaches the cap");
    let err = platform
        .state_fs()
        .write("second", b"2")
        .expect_err("event log cap must reject the write before mutating state");

    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("event log"));
    assert_eq!(platform.state_fs().read("second").unwrap(), None);
}

#[test]
fn direct_in_memory_engine_consumes_capacity_budget() {
    let capacity = StoreCapacityBudget {
        event_log_max_items: 1,
        kv_max_entries: 1,
        blob_max_bytes: 1,
        snapshot_max_bytes: 256,
        logical_namespace_max_bytes: 16,
        logical_key_max_bytes: 16,
        event_record_key_max_bytes: 16,
        export_max_bytes: 256,
        import_max_bytes: 256,
    };
    let engine = InMemoryStoreEngine::new(capacity);

    engine
        .put_blob("state_fs", "small", b"1")
        .expect("first byte fits budget");
    let err = engine
        .put_blob("state_fs", "second", b"1")
        .expect_err("direct backend API must enforce cumulative blob budget");
    assert_eq!(err.stage(), "store_budget_exceeded");

    engine
        .append_event(MemoryStoreEvent::new(
            "evt-1",
            MemoryStoreEventKind::RuntimeLifecycle,
            StoreEventScope::system("direct"),
            1,
        ))
        .expect("first event fits budget");
    let err = engine
        .append_event(MemoryStoreEvent::new(
            "evt-2",
            MemoryStoreEventKind::RuntimeLifecycle,
            StoreEventScope::system("direct"),
            2,
        ))
        .expect_err("direct backend API must enforce event budget");
    assert_eq!(err.stage(), "store_budget_exceeded");
}

#[test]
fn export_and_import_use_dedicated_runtime_budgets() {
    let source = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("source");
    source
        .state_fs()
        .write("payload", b"payload large enough for snapshot budget split")
        .expect("seed source");
    let snapshot = source.export_store_snapshot().expect("snapshot");

    let mut export_budget = tiny_store_budget();
    export_budget.blob_max_bytes = 1024 * 1024;
    export_budget.snapshot_max_bytes = 1024 * 1024;
    export_budget.export_max_bytes = 64;
    let export_config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(export_budget);
    let export_target = StorePlatform::open(export_config).expect("export target");
    export_target
        .state_fs()
        .write("payload", b"payload large enough for snapshot budget split")
        .expect("seed target");
    let err = export_target
        .export_store_snapshot()
        .expect_err("export must use export_max_bytes, not snapshot_max_bytes");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("export"));

    let mut import_budget = tiny_store_budget();
    import_budget.blob_max_bytes = 1024 * 1024;
    import_budget.snapshot_max_bytes = 1024 * 1024;
    import_budget.import_max_bytes = 64;
    let import_config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(import_budget);
    let import_target = StorePlatform::open(import_config).expect("import target");
    let err = import_target
        .import_store_snapshot(&snapshot)
        .expect_err("import must use import_max_bytes, not snapshot_max_bytes");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("import"));
}

#[test]
fn snapshot_import_rejects_oversized_logical_key_before_replacing_state() {
    let mut budget = tiny_store_budget();
    budget.blob_max_bytes = 1024 * 1024;
    budget.snapshot_max_bytes = 1024 * 1024;
    budget.import_max_bytes = 1024 * 1024;
    budget.export_max_bytes = 1024 * 1024;
    let target = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
            .expect("config")
            .with_runtime_store_budget(budget),
    )
    .expect("target");
    target
        .state_fs()
        .write("keep", b"state")
        .expect("seed state before failed import");
    let before = target.export_store_snapshot().expect("before");

    let snapshot = StoreSnapshot::new(
        before.schema_manifest.clone(),
        Vec::new(),
        vec![StoreSnapshotBlob {
            namespace: "state_fs".to_string(),
            key: "x".repeat(33),
            value: b"1".to_vec(),
        }],
        Vec::new(),
    );

    let err = target
        .import_store_snapshot(&snapshot)
        .expect_err("oversized import key must be rejected before replacement");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("logical key"));
    assert_eq!(
        target.state_fs().read("keep").unwrap(),
        Some(b"state".to_vec())
    );
}
