use std::path::PathBuf;

use bm_core::feature_gate::ProfileId;
use bm_core::platform::MemorySystemKind;
use bm_sdk::nonproduction_replay_harness::{
    StoreBackendConfig, StoreBackendKind, StoreRepairPolicy, STORE_SCHEMA_ID,
};

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "beetle-memory-store-config-{name}-{}",
        std::process::id()
    ))
}

#[test]
fn sqlite_backend_is_rejected_for_esp_profiles() {
    let err = StoreBackendConfig::sqlite(
        temp_root("esp-sqlite").join("memory.sqlite3"),
        ProfileId::EspStandaloneMemory,
    )
    .expect_err("esp standalone memory must not accept sqlite store");

    assert_eq!(err.stage(), "store_backend_config");
    assert!(err.to_string().contains("sqlite"));

    let err = StoreBackendConfig::sqlite(
        temp_root("esp-embedded-sqlite").join("memory.sqlite3"),
        ProfileId::EspEmbeddedSdk,
    )
    .expect_err("esp embedded sdk must not accept sqlite store");

    assert_eq!(err.stage(), "store_backend_config");
    assert!(err.to_string().contains("sqlite"));
}

#[test]
fn file_backend_is_rejected_for_esp_profiles() {
    let err = StoreBackendConfig::file(temp_root("esp-file"), ProfileId::EspStandaloneMemory)
        .expect_err("esp standalone memory must not accept file store");

    assert_eq!(err.stage(), "store_backend_config");
    assert!(err.to_string().contains("embedded or in-memory"));

    let err = StoreBackendConfig::file(temp_root("esp-embedded-file"), ProfileId::EspEmbeddedSdk)
        .expect_err("esp embedded sdk must not accept file store");

    assert_eq!(err.stage(), "store_backend_config");
    assert!(err.to_string().contains("embedded or in-memory"));
}

#[test]
fn desktop_and_server_profiles_accept_file_and_sqlite_backends() {
    let file = StoreBackendConfig::file(
        temp_root("desktop-file"),
        ProfileId::DesktopMacosEmbeddedSdk,
    )
    .expect("desktop profile should accept file store");
    assert_eq!(file.backend, StoreBackendKind::File);
    assert_eq!(file.schema_id, STORE_SCHEMA_ID);
    assert_eq!(file.repair_policy, StoreRepairPolicy::ReportOnly);

    let sqlite = StoreBackendConfig::sqlite(
        temp_root("server-sqlite").join("memory.sqlite3"),
        ProfileId::ServerLinuxMemoryGateway,
    )
    .expect("server memory gateway should accept sqlite store");
    assert_eq!(sqlite.backend, StoreBackendKind::Sqlite);
    assert_eq!(sqlite.schema_id, STORE_SCHEMA_ID);
}

#[test]
fn path_budget_is_profile_specific_and_shorter_for_embedded_profiles() {
    let esp = StoreBackendConfig::embedded(ProfileId::EspEmbeddedSdk)
        .expect("esp embedded sdk should accept embedded store");
    let desktop = StoreBackendConfig::file(
        temp_root("desktop-path-budget"),
        ProfileId::DesktopMacosEmbeddedSdk,
    )
    .expect("desktop profile should accept file store");
    let server = StoreBackendConfig::sqlite(
        temp_root("server-path-budget").join("memory.sqlite3"),
        ProfileId::ServerLinuxDevFull,
    )
    .expect("server profile should accept sqlite store");

    assert!(esp.path_budget.max_file_name_bytes < desktop.path_budget.max_file_name_bytes);
    assert!(desktop.path_budget.max_file_name_bytes < server.path_budget.max_file_name_bytes);
    assert!(esp.path_budget.max_relative_path_bytes < desktop.path_budget.max_relative_path_bytes);
}

#[test]
fn esp_standalone_and_embedded_sdk_have_different_store_budgets() {
    let standalone = StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory)
        .expect("esp standalone memory should accept embedded store");
    let embedded = StoreBackendConfig::embedded(ProfileId::EspEmbeddedSdk)
        .expect("esp embedded sdk should accept embedded store");

    assert_eq!(standalone.backend, StoreBackendKind::Embedded);
    assert_eq!(standalone.memory_system_kind, MemorySystemKind::Standalone);
    assert_eq!(embedded.memory_system_kind, MemorySystemKind::SdkEmbedded);

    assert!(standalone.capacity.event_log_max_items > embedded.capacity.event_log_max_items);
    assert!(standalone.capacity.snapshot_max_bytes > embedded.capacity.snapshot_max_bytes);
    assert!(standalone.capacity.kv_max_entries > embedded.capacity.kv_max_entries);
}
