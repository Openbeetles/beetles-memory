use bm_replay::run_sdk_memory_harness;
use bm_sdk::{ProfileId, StoreBackendConfig};

#[test]
fn sdk_memory_harness_runs_through_runtime_on_host_native_profile() {
    let profile = ProfileId::native_dev_full().expect("host-native dev-full profile");
    let report = run_sdk_memory_harness(StoreBackendConfig::in_memory(profile).unwrap())
        .expect("harness report");

    assert!(report.run.passed, "{:?}", report.run.failures);
    assert!(report
        .run
        .report_fragments
        .iter()
        .any(|fragment| fragment.contains("runtime_skill__release_guard")));
}

#[test]
fn esp_standalone_runs_compact_sdk_harness_without_sqlite() {
    let report = run_sdk_memory_harness(
        StoreBackendConfig::in_memory(ProfileId::EspStandaloneMemory).unwrap(),
    )
    .expect("compact harness report");

    assert!(report.run.passed, "{:?}", report.run.failures);
    assert_eq!(report.run.profile, ProfileId::EspStandaloneMemory);
}
