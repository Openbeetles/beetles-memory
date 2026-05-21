use bm_replay::run_sdk_memory_harness;
use bm_sdk::{ProfileId, StoreBackendConfig};

#[test]
fn sdk_memory_harness_runs_through_runtime_on_server_profile() {
    let report = run_sdk_memory_harness(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
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
