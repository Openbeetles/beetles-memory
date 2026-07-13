use bm_core::feature_gate::ProfileId;
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{StoreBackendConfig, StorePlatform};

#[test]
fn embedded_store_uses_bounded_event_ring_without_sqlite() {
    let mut config = StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory).unwrap();
    config.capacity.event_log_max_items = 2;

    let platform = StorePlatform::open(config).unwrap();
    platform.state_fs().write("a", b"1").unwrap();
    platform.state_fs().write("b", b"2").unwrap();
    platform.state_fs().write("c", b"3").unwrap();

    let events = platform.read_events().unwrap();
    assert!(events.len() <= 2);
    assert_eq!(platform.state_fs().read("c").unwrap(), Some(b"3".to_vec()));
}

#[test]
fn embedded_sdk_store_keeps_lightweight_runtime_paths_available() {
    let platform =
        StorePlatform::open(StoreBackendConfig::embedded(ProfileId::EspEmbeddedSdk).unwrap())
            .unwrap();

    platform
        .session_store()
        .append("chat-a", "user", "hello")
        .unwrap();
    platform
        .skill_storage()
        .write("runtime-lite", b"body")
        .unwrap();

    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 1);
    assert_eq!(
        platform.skill_storage().read("runtime-lite").unwrap(),
        b"body"
    );
}

#[test]
fn embedded_store_enforces_snapshot_runtime_budget() {
    let mut config = StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory).unwrap();
    config.capacity.export_max_bytes = 64;

    let platform = StorePlatform::open(config).unwrap();
    platform
        .state_fs()
        .write(
            "runtime/state.json",
            b"payload larger than the tiny snapshot budget",
        )
        .unwrap();

    let err = platform
        .export_store_snapshot()
        .expect_err("oversized embedded snapshot must be rejected");

    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("snapshot"));
}

#[test]
fn embedded_snapshot_import_rejects_event_lineage_that_exceeds_ring() {
    let source = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    source.state_fs().write("a", b"1").unwrap();
    source.state_fs().write("b", b"2").unwrap();
    source.state_fs().write("c", b"3").unwrap();
    let snapshot = source.export_store_snapshot().unwrap();
    assert!(snapshot.events.len() > 2);

    let mut config = StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory).unwrap();
    config.capacity.event_log_max_items = 2;
    config.capacity.snapshot_max_bytes = 1024 * 1024;
    let target = StorePlatform::open(config).unwrap();
    target.state_fs().write("keep", b"target").unwrap();
    let before = target.export_store_snapshot().unwrap();

    let err = target
        .import_store_snapshot(&snapshot)
        .expect_err("embedded import must not truncate snapshot event lineage");

    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("event lineage"));
    let after = target.export_store_snapshot().unwrap();
    assert_eq!(after.state_fingerprint(), before.state_fingerprint());
    assert_eq!(after.event_fingerprint(), before.event_fingerprint());
}
