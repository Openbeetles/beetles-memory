use bm_replay::{
    load_memory_benchmark_fixture_dir, run_memory_benchmark_wall, MemoryBenchmarkClass,
    MemoryBenchmarkMode,
};
use bm_sdk::ProfileId;

#[test]
fn memory_benchmark_wall_reports_all_next_gen_metrics() {
    let fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");

    let report = run_memory_benchmark_wall(&fixtures);

    assert!(report.passed, "{:#?}", report.failures);
    assert_eq!(report.total_fixtures, 12);
    assert_eq!(report.passed_fixtures, 12);
    assert!(report.missing_classes.is_empty());
    assert!(report.baseline.accuracy_bps >= 9000);
    assert!(report.baseline.evidence_precision_bps >= 8500);
    assert!(report.baseline.projection_faithfulness_bps >= 8500);
    assert_eq!(report.baseline.privacy_violation_count, 0);
    assert_eq!(report.baseline.soul_regression_count, 0);
    assert!(report
        .class_coverage
        .iter()
        .all(|coverage| coverage.compact_fixtures >= 1 && coverage.full_fixtures >= 1));
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

    assert_eq!(compact.len(), 6);
    assert_eq!(full.len(), 6);
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
