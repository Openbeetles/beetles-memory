use bm_replay::{build_sdk_memory_harness_fixture, run_replay_fixture, ReplayRunnerConfig};
use bm_sdk::{ProfileId, StoreBackendConfig};

#[test]
fn fixture_runner_executes_sdk_operations_across_store_backends() {
    let fixture =
        build_sdk_memory_harness_fixture(ProfileId::ServerLinuxDevFull).expect("harness fixture");
    let in_memory = run_replay_fixture(
        &fixture,
        ReplayRunnerConfig::for_backend(
            StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
        )
        .unwrap(),
    )
    .expect("in-memory replay");
    assert!(in_memory.passed, "{:?}", in_memory.failures);
    assert_eq!(in_memory.operations_run, fixture.operations.len());

    let root = std::env::temp_dir().join(format!(
        "beetle-memory-replay-fixture-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let file = run_replay_fixture(
        &fixture,
        ReplayRunnerConfig::for_backend(
            StoreBackendConfig::file(root, ProfileId::ServerLinuxDevFull).unwrap(),
        )
        .unwrap(),
    )
    .expect("file replay");
    assert!(file.passed, "{:?}", file.failures);
    assert_eq!(file.operations_run, fixture.operations.len());
    assert_eq!(file.state_fingerprint, in_memory.state_fingerprint);
}

#[test]
fn fixture_runner_rejects_profile_backend_mismatch_before_runtime_work() {
    let fixture =
        build_sdk_memory_harness_fixture(ProfileId::ServerLinuxDevFull).expect("harness fixture");
    let report = run_replay_fixture(
        &fixture,
        ReplayRunnerConfig::for_backend(
            StoreBackendConfig::in_memory(ProfileId::LinuxDeviceStandaloneMemory).unwrap(),
        )
        .unwrap(),
    )
    .expect("mismatch report");

    assert!(!report.passed);
    assert_eq!(report.operations_run, 0);
    assert_eq!(report.failures[0].stage, "fixture_profile");
}
