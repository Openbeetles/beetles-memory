#[test]
fn p8_quality_cutover_keeps_v1_raw_constructors_off_the_library_surface() {
    let library = include_str!("../src/lib.rs");
    let quality = include_str!("../src/p8_quality/mod.rs");
    let semantic = include_str!("../src/p8_semantic.rs");

    assert!(library
        .lines()
        .any(|line| line.trim() == "pub mod p8_quality;"));
    assert!(library
        .lines()
        .any(|line| line.trim() == "mod p8_semantic;"));
    assert!(!library
        .lines()
        .any(|line| line.trim() == "pub mod p8_semantic;"));
    assert!(!library.contains("pub use p8_quality"));
    assert!(!library.contains("pub use p8_semantic"));
    let public_reexports = library
        .lines()
        .filter(|line| line.trim_start().starts_with("pub use "))
        .collect::<Vec<_>>()
        .join("\n");

    for forbidden in [
        "P8SemanticQuestionDetailInputV1",
        "P8SemanticProducerIdentityV1",
        "P8SemanticRunPlanV1",
        "P8ReaderReceiptV1",
        "P8JudgeReceiptV1",
        "P8BenchmarkJoinReceiptV1",
        "P8SemanticShardManifestV1",
        "P8SemanticShardSubmissionV1",
        "produce_p8_semantic_summary",
        "publish_p8_semantic_bundle_no_clobber",
    ] {
        assert!(
            !public_reexports.contains(forbidden),
            "raw P8 semantic V1 symbol remains public: {forbidden}"
        );
    }

    for forbidden in ["from_v1", "upgrade_v1", "dual_read", "compat_v1"] {
        assert!(
            !quality.contains(forbidden),
            "P8 quality introduced a forbidden V1 compatibility path: {forbidden}"
        );
    }

    for raw_builder in [
        "pub(crate) fn build(",
        "pub(crate) fn new(",
        "pub(crate) fn from_single_read(",
        "pub(crate) fn produce_p8_semantic_summary(",
        "pub(crate) fn publish_p8_semantic_bundle_no_clobber(",
    ] {
        assert!(
            semantic.contains(raw_builder),
            "V1 raw constructor was not narrowed to crate-private: {raw_builder}"
        );
    }
}

#[test]
fn fixed_role_stdout_cannot_mint_production_gate_or_publication_evidence() {
    let source_release = include_str!("../src/p8_quality/source_release.rs");
    let trusted_execution = include_str!("../src/p8_quality/trusted_execution.rs");
    let publisher_entry = include_str!("../src/bin/bm-p8-quality-source-publisher.rs");
    let supervisor_entry = include_str!("../src/bin/bm-p8-quality-supervisor.rs");

    assert!(!source_release.contains("from_supervisor_output"));
    assert!(!source_release.contains("SealedSupervisor"));
    assert!(trusted_execution.contains("#[cfg(test)]\npub(crate) struct P8SealedProcessLauncher"));
    assert!(!publisher_entry.contains("source_publisher.rs"));
    assert!(!supervisor_entry.contains("source_publisher.rs"));
    assert!(!publisher_entry.contains("commit_harness_release"));
    assert!(!supervisor_entry.contains("commit_harness_release"));
}

#[test]
fn fixture_runner_and_operator_binaries_reach_only_fixture_scoped_session_entries() {
    let runner = include_str!("../src/bin/bm-p8-quality-runner.rs");
    let operator = include_str!("../src/bin/bm-p8-quality-operator.rs");
    let quality = include_str!("../src/p8_quality/mod.rs");

    assert!(runner.contains("try_run_fixture_runner_session_entry"));
    assert!(operator.contains("try_run_fixture_operator_session_entry"));
    assert!(!runner.contains("run_trusted_supervisor_parent_session"));
    assert!(!operator.contains("run_trusted_supervisor_parent_session"));
    let runner_session = quality
        .split("fn run_fixture_runner_session")
        .nth(1)
        .and_then(|tail| tail.split("fn run_fixture_operator_session").next())
        .expect("fixture runner session owner");
    assert!(runner_session.contains("admit_supervisor_binding"));
    assert!(!runner_session.contains("mint_for_supervisor"));
}

#[cfg(not(target_os = "linux"))]
#[test]
fn parent_owned_trusted_entry_is_reachable_and_returns_typed_na_before_path_access() {
    use std::{path::PathBuf, time::Duration};

    use bm_replay::p8_quality::{
        run_trusted_supervisor_parent_session, P8TrustedSupervisorParentPlan,
        P8TrustedSupervisorParentResult,
    };

    let missing = || PathBuf::from("/definitely-missing-p8-trusted-input");
    let result = run_trusted_supervisor_parent_session(P8TrustedSupervisorParentPlan {
        source_root: missing(),
        releases_root: missing(),
        source_publisher_executable: missing(),
        quality_runner_executable: missing(),
        quality_operator_executable: missing(),
        trusted_supervisor_executable: missing(),
        cargo_executable: missing(),
        rustc_executable: missing(),
        rustdoc_executable: missing(),
        rustfmt_executable: missing(),
        cargo_fmt_executable: missing(),
        cargo_clippy_executable: missing(),
        clippy_driver_executable: missing(),
        rust_lld_executable: missing(),
        target_root: missing(),
        rust_sysroot_root: missing(),
        cargo_dependency_cache_root: missing(),
        timeout: Duration::from_secs(1),
        stdout_bytes: 1,
        stderr_bytes: 1,
        total_bytes: 2,
    })
    .expect("non-Linux parent entry returns typed availability before path access");

    assert_eq!(
        result,
        P8TrustedSupervisorParentResult::NotApplicableOnThisPlatform
    );
}
