mod support;
use bm_core::feature_gate::ProfileId;
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::StoreBackendConfig;

#[test]
fn embedded_store_rejects_transaction_before_append_only_audit_overflow() {
    let mut budget =
        support::open_store(StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory).unwrap())
            .unwrap()
            .capacity()
            .into_runtime_budget();
    budget.event_log_max_items = 2;
    let config = StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory)
        .unwrap()
        .try_with_nonproduction_store_budget_limit(budget)
        .expect("append-only audit budget must be a valid semantic contraction");

    let platform = support::open_store(config).unwrap();
    platform.state_fs().write("a", b"1").unwrap();
    let error = platform
        .state_fs()
        .write("b", b"2")
        .expect_err("owner and audit event must fail atomically at capacity");

    let events = platform.read_events().unwrap();
    assert_eq!(error.stage(), "store_budget_exceeded");
    assert_eq!(events.len(), 2);
    assert_eq!(platform.state_fs().read("a").unwrap(), Some(b"1".to_vec()));
    assert_eq!(platform.state_fs().read("b").unwrap(), None);
}

#[test]
fn embedded_sdk_store_keeps_lightweight_runtime_paths_available() {
    let platform =
        support::open_store(StoreBackendConfig::embedded(ProfileId::EspEmbeddedSdk).unwrap())
            .unwrap();

    platform
        .session_store()
        .append("chat-a", "user", "hello")
        .unwrap();
    let skill = support::seed_runtime_skill(&platform, "runtime-lite");

    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 1);
    assert_eq!(
        platform.skill_storage().read("runtime-lite").unwrap(),
        skill
    );
}

#[test]
fn embedded_store_enforces_snapshot_runtime_budget() {
    let mut budget =
        support::open_store(StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory).unwrap())
            .unwrap()
            .capacity()
            .into_runtime_budget();
    budget.export_max_bytes = 64;
    let config = StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory)
        .unwrap()
        .try_with_nonproduction_store_budget_limit(budget)
        .expect("snapshot budget must be a valid semantic contraction");

    let platform = support::open_store(config).unwrap();
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
    let source = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
    )
    .unwrap();
    source.state_fs().write("a", b"1").unwrap();
    source.state_fs().write("b", b"2").unwrap();
    source.state_fs().write("c", b"3").unwrap();
    let snapshot = source.export_store_snapshot().unwrap();
    assert!(snapshot.events.len() > 2);

    let mut budget =
        support::open_store(StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory).unwrap())
            .unwrap()
            .capacity()
            .into_runtime_budget();
    budget.event_log_max_items = 2;
    budget.snapshot_max_bytes = 1024 * 1024;
    let config = StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory)
        .unwrap()
        .try_with_nonproduction_store_budget_limit(budget)
        .expect("import budget must be a valid semantic contraction");
    let target = support::open_store(config).unwrap();
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
