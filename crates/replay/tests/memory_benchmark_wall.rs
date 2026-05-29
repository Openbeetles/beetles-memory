use bm_replay::{
    load_memory_benchmark_fixture_dir, run_memory_benchmark_wall, MemoryBenchmarkClass,
    MemoryBenchmarkMode, MemoryBenchmarkSemanticDimension,
};
use bm_sdk::ProfileId;

#[test]
fn memory_benchmark_wall_reports_all_next_gen_metrics() {
    let fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");

    let report = run_memory_benchmark_wall(&fixtures);

    assert!(report.passed, "{:#?}", report.failures);
    assert_eq!(report.total_fixtures, 28);
    assert_eq!(report.passed_fixtures, 28);
    assert!(report.missing_classes.is_empty());
    assert!(report.semantic_failures.is_empty());
    assert!(report.baseline.accuracy_bps >= 9000);
    assert!(report.baseline.evidence_precision_bps >= 8500);
    assert!(report.baseline.projection_faithfulness_bps >= 8500);
    assert_eq!(report.baseline.privacy_violation_count, 0);
    assert_eq!(report.baseline.soul_regression_count, 0);
    assert!(report
        .class_coverage
        .iter()
        .all(|coverage| coverage.compact_fixtures >= 1 && coverage.full_fixtures >= 1));
    assert!(report
        .semantic_coverage
        .iter()
        .all(|coverage| coverage.fixture_count >= 1));
    assert!(
        report.soul_kernel_judge.release_gate_passed,
        "{:#?}",
        report.soul_kernel_judge.blocked_reasons
    );
    assert!(
        report.subject_projection_judge.release_gate_passed,
        "{:#?}",
        report.subject_projection_judge.blocked_reasons
    );
    assert!(
        report
            .subject_projection_judge
            .cross_surface_consistency_passed
    );
    assert!(
        report
            .subject_projection_judge
            .gateway_raw_audit_redaction_covered
    );
    assert!(
        report.agent_tool_experience_judge.release_gate_passed,
        "{:#?}",
        report.agent_tool_experience_judge.blocked_reasons
    );
    assert!(
        report
            .agent_tool_experience_judge
            .no_experience_empty_hints_covered
    );
    assert!(
        report
            .agent_tool_experience_judge
            .governed_experience_hint_covered
    );
    assert!(
        report
            .agent_tool_experience_judge
            .compact_registry_forbidden_covered
    );
}

#[test]
fn memory_benchmark_wall_enforces_compact_and_full_profile_sets() {
    let fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");

    let compact = fixtures
        .iter()
        .filter(|fixture| fixture.mode == MemoryBenchmarkMode::Compact)
        .collect::<Vec<_>>();
    let full = fixtures
        .iter()
        .filter(|fixture| fixture.mode == MemoryBenchmarkMode::Full)
        .collect::<Vec<_>>();

    assert_eq!(compact.len(), 8);
    assert_eq!(full.len(), 20);
    assert!(compact
        .iter()
        .all(|fixture| fixture.profile == ProfileId::EspStandaloneMemory));
    assert!(full
        .iter()
        .all(|fixture| fixture.profile == ProfileId::ServerLinuxDevFull));

    for class in MemoryBenchmarkClass::ALL {
        assert!(compact.iter().any(|fixture| fixture.class == class));
        assert!(full.iter().any(|fixture| fixture.class == class));
    }
}

#[test]
fn memory_benchmark_wall_covers_agent_tool_experience_fixtures() {
    let fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");
    let report = run_memory_benchmark_wall(&fixtures);

    assert!(report.passed, "{:#?}", report.agent_tool_experience_judge);
    for fixture_id in [
        "agent-tool-experience-agent-tool-registry-forbidden-compact",
        "agent-tool-experience-no-experience-empty-hints-full",
        "agent-tool-experience-governed-experience-hint-full",
        "agent-tool-experience-schema-drift-stales-experience-full",
        "agent-tool-experience-private-observation-not-public-full",
        "agent-tool-experience-gateway-host-tools-no-cold-route-full",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.fixture_id == fixture_id),
            "missing {fixture_id}"
        );
    }

    assert!(
        report
            .agent_tool_experience_judge
            .schema_drift_rejection_covered
    );
    assert!(
        report
            .agent_tool_experience_judge
            .private_observation_not_public_covered
    );
    assert!(
        report
            .agent_tool_experience_judge
            .gateway_no_cold_route_covered
    );
    assert!(
        report
            .agent_tool_experience_judge
            .host_execution_boundary_covered
    );
}

#[test]
fn memory_benchmark_wall_covers_inhabited_subject_phase0_fixtures() {
    let fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");
    let report = run_memory_benchmark_wall(&fixtures);

    assert!(report.passed, "{:#?}", report.semantic_failures);
    for fixture_id in [
        "subject-projection-inhabited-subject-mount-full",
        "subject-projection-inhabited-subject-mount-compact",
        "subject-projection-protected-private-runtime-envelope-full",
        "soul-regression-no-roleplay-host-mount-full",
        "soul-regression-soul-life-slot-continuity-full",
        "soul-regression-work-integrity-no-obstruction-full",
        "privacy-refusal-private-disclosure-adjudication-full",
        "privacy-refusal-no-final-llm-privacy-judge-full",
        "privacy-refusal-disclosure-protocol-in-main-runtime-full",
        "privacy-refusal-raw-audit-redacted-private-envelope-full",
    ] {
        assert!(
            fixtures
                .iter()
                .any(|fixture| fixture.fixture_id == fixture_id),
            "missing {fixture_id}"
        );
    }

    for dimension in MemoryBenchmarkSemanticDimension::ALL {
        assert!(
            report
                .semantic_coverage
                .iter()
                .any(|coverage| coverage.dimension == dimension && coverage.fixture_count > 0),
            "missing semantic coverage for {dimension:?}"
        );
    }
}

#[test]
fn memory_benchmark_wall_reports_semantic_required_key_failures() {
    let fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");
    let mut target = fixtures
        .iter()
        .find(|fixture| fixture.fixture_id == "subject-projection-inhabited-subject-mount-full")
        .expect("phase0 subject mount fixture")
        .clone();
    target
        .semantic_contract
        .provided_keys
        .retain(|key| key != "subject_mount");

    let report = run_memory_benchmark_wall(&[target]);

    assert!(!report.passed);
    assert!(report.semantic_failures.iter().any(|failure| {
        failure.stage == "semantic_required_key" && failure.reason.contains("subject_mount")
    }));
}

#[test]
fn memory_benchmark_wall_reports_semantic_forbidden_marker_failures() {
    let fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");
    let mut target = fixtures
        .iter()
        .find(|fixture| fixture.fixture_id == "privacy-refusal-no-final-llm-privacy-judge-full")
        .expect("phase0 no-final privacy judge fixture")
        .clone();
    target
        .semantic_contract
        .observed_markers
        .push("second LLM rewrites final reply".to_string());

    let report = run_memory_benchmark_wall(&[target]);

    assert!(!report.passed);
    assert!(report.semantic_failures.iter().any(|failure| {
        failure.stage == "semantic_forbidden_marker"
            && failure.reason.contains("second LLM rewrites final reply")
    }));
}

#[test]
fn memory_benchmark_wall_blocks_w2_when_soul_release_artifacts_are_missing() {
    let mut fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");
    let target = fixtures
        .iter_mut()
        .find(|fixture| fixture.fixture_id == "soul-regression-full-baseline")
        .expect("soul full baseline");
    target
        .semantic_contract
        .provided_keys
        .retain(|key| key != "soul_growth_proposal");
    target
        .semantic_contract
        .required_keys
        .retain(|key| key != "soul_growth_proposal");
    target
        .scenario
        .expected_surfaces
        .retain(|surface| surface != "SoulGrowthProposal");

    let report = run_memory_benchmark_wall(&fixtures);

    assert!(!report.passed);
    assert!(!report.soul_kernel_judge.release_gate_passed);
    assert!(report
        .soul_kernel_judge
        .blocked_reasons
        .contains(&"soul_growth_proposal_contract_missing".to_string()));
}

#[test]
fn memory_benchmark_wall_blocks_w3_when_cross_surface_judge_is_missing() {
    let mut fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");
    for fixture in fixtures.iter_mut().filter(|fixture| {
        fixture.class == MemoryBenchmarkClass::SubjectProjection
            || fixture.class == MemoryBenchmarkClass::PrivacyRefusal
    }) {
        fixture
            .semantic_contract
            .provided_keys
            .retain(|key| key != "cross_surface_consistency");
        fixture
            .semantic_contract
            .required_keys
            .retain(|key| key != "cross_surface_consistency");
    }

    let report = run_memory_benchmark_wall(&fixtures);

    assert!(!report.passed);
    assert!(!report.subject_projection_judge.release_gate_passed);
    assert!(report
        .subject_projection_judge
        .blocked_reasons
        .contains(&"cross_surface_consistency_missing".to_string()));
}

#[test]
fn memory_benchmark_wall_fails_missing_or_regressed_classes() {
    let mut fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");
    fixtures.retain(|fixture| fixture.class != MemoryBenchmarkClass::PrivacyRefusal);
    fixtures[0].metrics.privacy_violation_count = 1;

    let report = run_memory_benchmark_wall(&fixtures);

    assert!(!report.passed);
    assert!(report
        .missing_classes
        .iter()
        .any(|missing| missing.class == MemoryBenchmarkClass::PrivacyRefusal));
    assert!(report
        .failures
        .iter()
        .any(|failure| failure.stage == "privacy_violation_count"));
}

fn fixture_root() -> String {
    format!(
        "{}/../../fixtures/memory-benchmark-wall",
        env!("CARGO_MANIFEST_DIR")
    )
}
