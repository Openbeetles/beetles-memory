use bm_core::budget::StoreRuntimeBudget;
use bm_core::feature_gate::ProfileId;
use bm_core::platform::Platform as _;
use bm_store::{StoreBackendConfig, StoreBackendKind, StorePlatform};

fn tiny_store_budget() -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        event_log_max_items: 8,
        kv_max_entries: 8,
        blob_max_bytes: 4,
        snapshot_max_bytes: 128,
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
