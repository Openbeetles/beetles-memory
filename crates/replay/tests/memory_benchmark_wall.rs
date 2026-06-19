use bm_replay::{
    evaluate_w4_external_noisy_wall, load_memory_benchmark_fixture_dir, run_memory_benchmark_wall,
    w4_external_noisy_summary_with_provenance, MemoryBenchmarkClass, MemoryBenchmarkEvalRecall,
    MemoryBenchmarkEvalRecallAtK, MemoryBenchmarkEvalRecallMetrics, MemoryBenchmarkMode,
    MemoryBenchmarkSemanticDimension, W4ExternalNoisyBenchmarkSummary,
};
use bm_sdk::ProfileId;
use std::{fs, process::Command};

#[test]
fn memory_benchmark_wall_reports_all_next_gen_metrics() {
    let fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");

    let report = run_memory_benchmark_wall(&fixtures);

    assert!(report.passed, "{:#?}", report.failures);
    assert_eq!(report.total_fixtures, 29);
    assert_eq!(report.passed_fixtures, 29);
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
    assert!(
        report.w4_eval_recall_judge.release_gate_passed,
        "{:#?}",
        report.w4_eval_recall_judge.blocked_reasons
    );
    assert!(report.w4_eval_recall_judge.fixture_count >= 1);
    assert!(report.w4_eval_recall_judge.required_k_covered);
    assert!(report.w4_eval_recall_judge.missing_evidence_reported);
    assert!(
        report
            .w4_eval_recall_judge
            .source_expanded_selected_split_covered
    );
    assert!(report.w4_eval_recall_judge.noisy_external_wall_required);
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
    assert_eq!(full.len(), 21);
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

#[test]
fn memory_benchmark_wall_rejects_incomplete_w4_eval_recall_fixture() {
    let fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");
    let mut target = fixtures
        .iter()
        .find(|fixture| fixture.fixture_id == "recall-multisession-full-baseline")
        .expect("recall multisession full baseline")
        .clone();
    target.fixture_id = "w4-eval-recall-incomplete-full".to_string();
    target
        .semantic_contract
        .provided_keys
        .push("w4_eval_recall".to_string());
    target.eval_recall = Some(MemoryBenchmarkEvalRecall {
        suite: "locomo10".to_string(),
        split: "full".to_string(),
        question_id: "q1".to_string(),
        question_type: "temporal_update".to_string(),
        expected_evidence_refs: vec!["D1:1|session_1".to_string()],
        source_candidates: vec!["D1:1|session_1".to_string()],
        expanded_candidates: Vec::new(),
        selected_candidates: Vec::new(),
        rendered_block_preview: String::new(),
        missing_evidence_refs: Vec::new(),
        metrics: MemoryBenchmarkEvalRecallMetrics {
            recall_at_k: vec![MemoryBenchmarkEvalRecallAtK {
                k: 5,
                any_evidence_hit: false,
                all_evidence_hit: false,
                matched_evidence_refs: Vec::new(),
            }],
            mrr_bps: 0,
        },
    });

    let report = run_memory_benchmark_wall(&[target]);

    assert!(!report.passed);
    assert!(report.failures.iter().any(|failure| {
        failure.stage == "w4_eval_recall_contract"
            && failure.reason.contains("expanded_candidates")
            && failure.reason.contains("recall_at_k:50")
            && failure.reason.contains("missing_evidence_refs")
    }));
}

#[test]
fn memory_benchmark_wall_blocks_w4_when_eval_recall_fixture_is_absent() {
    let mut fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");
    for fixture in &mut fixtures {
        fixture
            .semantic_contract
            .provided_keys
            .retain(|key| key != "w4_eval_recall");
        fixture
            .semantic_contract
            .required_keys
            .retain(|key| key != "w4_eval_recall");
        fixture.eval_recall = None;
    }

    let report = run_memory_benchmark_wall(&fixtures);

    assert!(!report.passed);
    assert!(!report.w4_eval_recall_judge.release_gate_passed);
    assert!(report
        .w4_eval_recall_judge
        .blocked_reasons
        .contains(&"w4_eval_recall_fixture_missing".to_string()));
}

#[test]
fn w4_external_noisy_wall_records_baseline_without_treating_oracle_as_release() {
    let report = evaluate_w4_external_noisy_wall(&[
        external_summary("locomo", 10, 1986, 1982, 21, 13),
        external_summary("longmemeval_oracle", 500, 500, 500, 494, 491),
        external_summary("longmemeval_s_cleaned", 500, 500, 500, 111, 26),
        external_summary("longmemeval_m_cleaned", 500, 500, 500, 10, 3),
    ]);

    assert!(report.summary_attached);
    assert!(
        report.required_suites_covered,
        "{:#?}",
        report.blocked_reasons
    );
    assert!(report.noisy_splits_covered, "{:#?}", report.blocked_reasons);
    assert!(report.completed, "{:#?}", report.blocked_reasons);
    assert!(report.no_runner_errors, "{:#?}", report.blocked_reasons);
    assert!(report.row_counts_covered, "{:#?}", report.blocked_reasons);
    assert!(report.oracle_sanity_only);
    assert!(!report.release_gate_passed);
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_improvement_not_proven".to_string()));
    let m_cleaned = report
        .suite_reports
        .iter()
        .find(|suite| suite.suite == "longmemeval_m_cleaned")
        .expect("m_cleaned suite report");
    assert_eq!(m_cleaned.any_evidence_hit, 10);
    assert_eq!(m_cleaned.all_evidence_hit, 3);
}

#[test]
fn w4_external_noisy_wall_rejects_oracle_only_summary() {
    let report = evaluate_w4_external_noisy_wall(&[external_summary(
        "longmemeval_oracle",
        500,
        500,
        500,
        494,
        491,
    )]);

    assert!(report.summary_attached);
    assert!(!report.required_suites_covered);
    assert!(!report.noisy_splits_covered);
    assert!(!report.release_gate_passed);
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_required_suites_missing".to_string()));
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_noisy_splits_missing".to_string()));
}

#[test]
fn w4_external_noisy_wall_fails_incomplete_or_error_summaries() {
    let mut locomo = external_summary("locomo", 10, 1986, 1982, 21, 13);
    locomo.completed = false;
    let mut s_cleaned = external_summary("longmemeval_s_cleaned", 500, 499, 499, 111, 26);
    s_cleaned.recall_errors = 1;

    let report = evaluate_w4_external_noisy_wall(&[
        locomo,
        external_summary("longmemeval_oracle", 500, 500, 500, 494, 491),
        s_cleaned,
        external_summary("longmemeval_m_cleaned", 500, 500, 500, 10, 3),
    ]);

    assert!(!report.completed);
    assert!(!report.no_runner_errors);
    assert!(!report.row_counts_covered);
    assert!(!report.release_gate_passed);
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_incomplete".to_string()));
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_runner_errors".to_string()));
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_row_counts_invalid".to_string()));
}

#[test]
fn w4_external_noisy_summary_deserializes_current_merged_shape_without_hash_provenance() {
    let summary: W4ExternalNoisyBenchmarkSummary = serde_json::from_str(
        r#"{
          "suite": "locomo",
          "completed": true,
          "shards": ["locomo.shard-0-of-1.summary.json"],
          "samples": 10,
          "questions": 1986,
          "evidence_questions": 1982,
          "any_evidence_hit": 21,
          "all_evidence_hit": 13,
          "write_errors": 0,
          "recall_errors": 0,
          "max_shard_elapsed_secs": 37.253610833
        }"#,
    )
    .expect("current locomo merged summary shape");

    assert_eq!(summary.suite, "locomo");
    assert!(summary.summary_sha256.is_none());
    assert!(summary.runner_source_sha256.is_none());
}

#[test]
fn w4_external_noisy_wall_reports_stage_hit_diagnostics_when_runner_uses_eval_recall() {
    let locomo: W4ExternalNoisyBenchmarkSummary = serde_json::from_str(
        r#"{
          "suite": "locomo",
          "completed": true,
          "shards": ["locomo.shard-0-of-1.summary.json"],
          "samples": 10,
          "questions": 1986,
          "evidence_questions": 1982,
          "any_evidence_hit": 21,
          "all_evidence_hit": 13,
          "write_errors": 0,
          "recall_errors": 0,
          "stage_hit_counts": {
            "source_any_evidence_hit": 41,
            "source_all_evidence_hit": 20,
            "expanded_any_evidence_hit": 48,
            "expanded_all_evidence_hit": 23,
            "reranked_any_evidence_hit": 34,
            "reranked_all_evidence_hit": 18,
            "selected_any_evidence_hit": 21,
            "selected_all_evidence_hit": 13,
            "rendered_any_evidence_hit": 21,
            "rendered_all_evidence_hit": 13
          }
        }"#,
    )
    .expect("summary with stage diagnostics");
    let mut oracle = external_summary("longmemeval_oracle", 500, 500, 500, 494, 491);
    oracle.stage_hit_counts = locomo.stage_hit_counts.clone();
    let mut s_cleaned = external_summary("longmemeval_s_cleaned", 500, 500, 500, 111, 26);
    s_cleaned.stage_hit_counts = locomo.stage_hit_counts.clone();
    let mut m_cleaned = external_summary("longmemeval_m_cleaned", 500, 500, 500, 10, 3);
    m_cleaned.shards = (0..8)
        .map(|index| format!("longmemeval_m_cleaned.shard-{index}-of-8.summary.json"))
        .collect();
    m_cleaned.stage_hit_counts = locomo.stage_hit_counts.clone();

    let report = evaluate_w4_external_noisy_wall(&[locomo, oracle, s_cleaned, m_cleaned]);

    assert!(report.stage_diagnostics_attached);
    assert!(!report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_stage_diagnostics_missing".to_string()));
    let locomo_report = report
        .suite_reports
        .iter()
        .find(|suite| suite.suite == "locomo")
        .expect("locomo report");
    let stage = locomo_report
        .stage_hit_counts
        .as_ref()
        .expect("stage hit counts");
    assert_eq!(stage.source_any_evidence_hit, 41);
    assert_eq!(stage.expanded_any_evidence_hit, 48);
    assert_eq!(stage.reranked_any_evidence_hit, 34);
    assert_eq!(stage.selected_any_evidence_hit, 21);
    assert_eq!(stage.rendered_any_evidence_hit, 21);
}

#[test]
fn w4_external_noisy_wall_requires_index_diagnostics_for_noisy_index_effect_proof() {
    let locomo: W4ExternalNoisyBenchmarkSummary = serde_json::from_str(
        r#"{
          "suite": "locomo",
          "completed": true,
          "shards": ["locomo.shard-0-of-1.summary.json"],
          "samples": 10,
          "questions": 1986,
          "evidence_questions": 1982,
          "any_evidence_hit": 21,
          "all_evidence_hit": 13,
          "write_errors": 0,
          "recall_errors": 0,
          "stage_hit_counts": {
            "source_any_evidence_hit": 41,
            "source_all_evidence_hit": 20,
            "expanded_any_evidence_hit": 48,
            "expanded_all_evidence_hit": 23,
            "reranked_any_evidence_hit": 34,
            "reranked_all_evidence_hit": 18,
            "selected_any_evidence_hit": 21,
            "selected_all_evidence_hit": 13,
            "rendered_any_evidence_hit": 21,
            "rendered_all_evidence_hit": 13
          },
          "index_diagnostics": {
            "questions_with_index_report": 1986,
            "index_used_questions": 120,
            "fallback_full_scan_questions": 1866,
            "source_candidate_count": 2100,
            "matched_source_anchor_count": 130,
            "unmatched_source_anchor_count": 1970,
            "indexed_neighbor_count": 480,
            "filtered_node_count": 240,
            "filtered_edge_count": 220,
            "filtered_backlink_count": 240,
            "failure_count": 1866
          }
        }"#,
    )
    .expect("summary with index diagnostics");
    let mut oracle = external_summary("longmemeval_oracle", 500, 500, 500, 494, 491);
    oracle.stage_hit_counts = locomo.stage_hit_counts.clone();
    oracle.index_diagnostics = locomo.index_diagnostics.clone();
    let mut s_cleaned = external_summary("longmemeval_s_cleaned", 500, 500, 500, 111, 26);
    s_cleaned.stage_hit_counts = locomo.stage_hit_counts.clone();
    s_cleaned.index_diagnostics = locomo.index_diagnostics.clone();
    let mut m_cleaned = external_summary("longmemeval_m_cleaned", 500, 500, 500, 10, 3);
    m_cleaned.shards = (0..8)
        .map(|index| format!("longmemeval_m_cleaned.shard-{index}-of-8.summary.json"))
        .collect();
    m_cleaned.stage_hit_counts = locomo.stage_hit_counts.clone();
    m_cleaned.index_diagnostics = locomo.index_diagnostics.clone();

    let report = evaluate_w4_external_noisy_wall(&[locomo, oracle, s_cleaned, m_cleaned]);

    assert!(report.index_diagnostics_attached);
    assert!(!report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_index_diagnostics_missing".to_string()));
    let locomo_report = report
        .suite_reports
        .iter()
        .find(|suite| suite.suite == "locomo")
        .expect("locomo report");
    let index = locomo_report
        .index_diagnostics
        .as_ref()
        .expect("index diagnostics");
    assert_eq!(index.questions_with_index_report, 1986);
    assert_eq!(index.index_used_questions, 120);
    assert_eq!(index.fallback_full_scan_questions, 1866);
    assert_eq!(index.matched_source_anchor_count, 130);
    assert_eq!(index.unmatched_source_anchor_count, 1970);
    assert_eq!(index.indexed_neighbor_count, 480);
}

#[test]
fn w4_external_noisy_wall_passes_only_when_improvement_has_stage_and_index_attribution() {
    let locomo = external_summary_with_stage_and_index(
        "locomo", 10, 1986, 1982, 21, 13, 21, 13, 21, 13, 21, 13, 40, 10, 30, 120, 30,
    );
    let oracle = external_summary_with_stage_and_index(
        "longmemeval_oracle",
        500,
        500,
        500,
        494,
        491,
        494,
        491,
        494,
        491,
        494,
        491,
        120,
        80,
        40,
        240,
        40,
    );
    let s_cleaned = external_summary_with_stage_and_index(
        "longmemeval_s_cleaned",
        500,
        500,
        500,
        111,
        26,
        111,
        26,
        111,
        26,
        111,
        26,
        100,
        20,
        80,
        160,
        80,
    );
    let m_cleaned = external_summary_with_stage_and_index(
        "longmemeval_m_cleaned",
        500,
        500,
        500,
        12,
        4,
        10,
        3,
        12,
        4,
        12,
        4,
        90,
        30,
        60,
        180,
        60,
    );

    let report = evaluate_w4_external_noisy_wall(&[locomo, oracle, s_cleaned, m_cleaned]);

    assert!(report.noisy_improvement_proven);
    assert!(report.stage_attributed_improvement_proven);
    assert!(report.index_effect_proven);
    assert!(report.release_gate_passed, "{:#?}", report.blocked_reasons);
    let m_report = report
        .suite_reports
        .iter()
        .find(|suite| suite.suite == "longmemeval_m_cleaned")
        .expect("m_cleaned report");
    assert!(m_report.stage_attributed_improvement);
    assert!(m_report.index_effect_proven);
}

#[test]
fn w4_external_noisy_wall_blocks_final_improvement_without_stage_or_index_effect() {
    let locomo = external_summary_with_stage_and_index(
        "locomo", 10, 1986, 1982, 21, 13, 21, 13, 21, 13, 21, 13, 40, 10, 30, 120, 30,
    );
    let oracle = external_summary_with_stage_and_index(
        "longmemeval_oracle",
        500,
        500,
        500,
        494,
        491,
        494,
        491,
        494,
        491,
        494,
        491,
        120,
        80,
        40,
        240,
        40,
    );
    let s_cleaned = external_summary_with_stage_and_index(
        "longmemeval_s_cleaned",
        500,
        500,
        500,
        111,
        26,
        111,
        26,
        111,
        26,
        111,
        26,
        100,
        20,
        80,
        160,
        80,
    );
    let m_cleaned = external_summary_with_stage_and_index(
        "longmemeval_m_cleaned",
        500,
        500,
        500,
        12,
        4,
        12,
        4,
        12,
        4,
        12,
        4,
        0,
        0,
        500,
        0,
        500,
    );

    let report = evaluate_w4_external_noisy_wall(&[locomo, oracle, s_cleaned, m_cleaned]);

    assert!(report.noisy_improvement_proven);
    assert!(!report.stage_attributed_improvement_proven);
    assert!(!report.index_effect_proven);
    assert!(!report.release_gate_passed);
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_stage_attribution_not_proven".to_string()));
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_index_effect_not_proven".to_string()));
}

#[test]
fn w4_external_noisy_operator_attaches_provenance_without_changing_baseline_status() {
    let runner_hash = "167bbfd23f445e511b375410d0e9e424bc56e9b410ed2cbf47b7654b17cecd40";
    let locomo = w4_external_noisy_summary_with_provenance(
        r#"{
          "suite": "locomo",
          "completed": true,
          "shards": ["locomo.shard-0-of-1.summary.json"],
          "samples": 10,
          "questions": 1986,
          "evidence_questions": 1982,
          "any_evidence_hit": 21,
          "all_evidence_hit": 13,
          "write_errors": 0,
          "recall_errors": 0
        }"#,
        "70ae9075f0bd2d0153c24d1cf20c2d8ed6573811b9157bf3f60848c96a1dc0f8",
        runner_hash,
    )
    .expect("locomo summary with provenance");
    let oracle = w4_external_noisy_summary_with_provenance(
        r#"{
          "suite": "longmemeval_oracle",
          "completed": true,
          "shards": ["longmemeval_oracle.shard-0-of-1.summary.json"],
          "samples": 500,
          "questions": 500,
          "evidence_questions": 500,
          "any_evidence_hit": 494,
          "all_evidence_hit": 491,
          "write_errors": 0,
          "recall_errors": 0
        }"#,
        "cca1c2f5a3299dbd498b5e9586a3bfb059df3d15470c09d5a079564d8b91a08f",
        runner_hash,
    )
    .expect("oracle summary with provenance");
    let s_cleaned = w4_external_noisy_summary_with_provenance(
        r#"{
          "suite": "longmemeval_s_cleaned",
          "completed": true,
          "shards": ["longmemeval_s_cleaned.shard-0-of-1.summary.json"],
          "samples": 500,
          "questions": 500,
          "evidence_questions": 500,
          "any_evidence_hit": 111,
          "all_evidence_hit": 26,
          "write_errors": 0,
          "recall_errors": 0
        }"#,
        "5b0da9f6b9b12907ba4b28b2e421c01db590ddea6b7549b477f1d74d583e4a34",
        runner_hash,
    )
    .expect("s_cleaned summary with provenance");
    let m_cleaned = w4_external_noisy_summary_with_provenance(
        r#"{
          "suite": "longmemeval_m_cleaned",
          "completed": true,
          "shards": [
            "longmemeval_m_cleaned.shard-0-of-8.summary.json",
            "longmemeval_m_cleaned.shard-1-of-8.summary.json",
            "longmemeval_m_cleaned.shard-2-of-8.summary.json",
            "longmemeval_m_cleaned.shard-3-of-8.summary.json",
            "longmemeval_m_cleaned.shard-4-of-8.summary.json",
            "longmemeval_m_cleaned.shard-5-of-8.summary.json",
            "longmemeval_m_cleaned.shard-6-of-8.summary.json",
            "longmemeval_m_cleaned.shard-7-of-8.summary.json"
          ],
          "samples": 500,
          "questions": 500,
          "evidence_questions": 500,
          "any_evidence_hit": 10,
          "all_evidence_hit": 3,
          "write_errors": 0,
          "recall_errors": 0
        }"#,
        "d3d80a25ca4a292720e3136249c6e56e425870bad576e70524855862dbe3281b",
        runner_hash,
    )
    .expect("m_cleaned summary with provenance");

    let report = evaluate_w4_external_noisy_wall(&[locomo, oracle, s_cleaned, m_cleaned]);

    assert!(report.provenance_attached, "{:#?}", report.blocked_reasons);
    assert!(!report.stage_diagnostics_attached);
    assert!(!report.release_gate_passed);
    assert!(!report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_provenance_missing".to_string()));
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_stage_diagnostics_missing".to_string()));
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_improvement_not_proven".to_string()));
}

#[test]
fn w4_external_noisy_operator_cli_reads_only_summary_files_and_reports_current_baseline_blocker() {
    let root =
        std::env::temp_dir().join(format!("bm-w4-external-noisy-wall-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temp operator root");
    let locomo = write_external_summary_file(
        &root,
        "locomo.merged.summary.json",
        "locomo",
        &["locomo.shard-0-of-1.summary.json"],
        10,
        1986,
        1982,
        21,
        13,
    );
    let oracle = write_external_summary_file(
        &root,
        "longmemeval_oracle.merged.summary.json",
        "longmemeval_oracle",
        &["longmemeval_oracle.shard-0-of-1.summary.json"],
        500,
        500,
        500,
        494,
        491,
    );
    let s_cleaned = write_external_summary_file(
        &root,
        "longmemeval_s_cleaned.merged.summary.json",
        "longmemeval_s_cleaned",
        &["longmemeval_s_cleaned.shard-0-of-1.summary.json"],
        500,
        500,
        500,
        111,
        26,
    );
    let m_shards = (0..8)
        .map(|index| format!("longmemeval_m_cleaned.shard-{index}-of-8.summary.json"))
        .collect::<Vec<_>>();
    let m_shards_refs = m_shards.iter().map(String::as_str).collect::<Vec<_>>();
    let m_cleaned = write_external_summary_file(
        &root,
        "longmemeval_m_cleaned.merged.summary.json",
        "longmemeval_m_cleaned",
        &m_shards_refs,
        500,
        500,
        500,
        10,
        3,
    );

    let operator =
        std::env::var("CARGO_BIN_EXE_bm-w4-external-noisy-wall").expect("operator binary path");
    let output = Command::new(operator)
        .arg("--runner-source-sha256")
        .arg("167bbfd23f445e511b375410d0e9e424bc56e9b410ed2cbf47b7654b17cecd40")
        .arg("--summary")
        .arg("70ae9075f0bd2d0153c24d1cf20c2d8ed6573811b9157bf3f60848c96a1dc0f8")
        .arg(&locomo)
        .arg("--summary")
        .arg("cca1c2f5a3299dbd498b5e9586a3bfb059df3d15470c09d5a079564d8b91a08f")
        .arg(&oracle)
        .arg("--summary")
        .arg("5b0da9f6b9b12907ba4b28b2e421c01db590ddea6b7549b477f1d74d583e4a34")
        .arg(&s_cleaned)
        .arg("--summary")
        .arg("d3d80a25ca4a292720e3136249c6e56e425870bad576e70524855862dbe3281b")
        .arg(&m_cleaned)
        .output()
        .expect("run W4 external noisy operator");

    assert_eq!(output.status.code(), Some(10), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("operator stdout utf8");
    assert!(stdout.contains(r#""provenance_attached": true"#));
    assert!(stdout.contains(r#""stage_diagnostics_attached": false"#));
    assert!(stdout.contains("w4_external_noisy_wall_stage_diagnostics_missing"));
    assert!(stdout.contains("w4_external_noisy_wall_improvement_not_proven"));
    assert!(!stdout.contains("w4_external_noisy_wall_provenance_missing"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn w4_external_noisy_wall_reports_shards_bps_and_missing_provenance() {
    let mut locomo = external_summary("locomo", 10, 1986, 1982, 21, 13);
    locomo.shards = vec!["locomo.shard-0-of-1.summary.json".to_string()];
    let mut oracle = external_summary("longmemeval_oracle", 500, 500, 500, 494, 491);
    oracle.shards = vec!["longmemeval_oracle.shard-0-of-1.summary.json".to_string()];
    let mut s_cleaned = external_summary("longmemeval_s_cleaned", 500, 500, 500, 111, 26);
    s_cleaned.shards = vec!["longmemeval_s_cleaned.shard-0-of-1.summary.json".to_string()];
    let mut m_cleaned = external_summary("longmemeval_m_cleaned", 500, 500, 500, 10, 3);
    m_cleaned.shards = (0..8)
        .map(|index| format!("longmemeval_m_cleaned.shard-{index}-of-8.summary.json"))
        .collect();

    let report = evaluate_w4_external_noisy_wall(&[locomo, oracle, s_cleaned, m_cleaned]);

    assert!(!report.provenance_attached);
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_provenance_missing".to_string()));
    let m_cleaned = report
        .suite_reports
        .iter()
        .find(|suite| suite.suite == "longmemeval_m_cleaned")
        .expect("m_cleaned suite report");
    assert_eq!(m_cleaned.shard_count, 8);
    assert_eq!(m_cleaned.expected_shard_count, Some(8));
    assert!(m_cleaned.shards_valid);
    assert_eq!(m_cleaned.any_evidence_hit_bps, 200);
    assert_eq!(m_cleaned.all_evidence_hit_bps, 60);
}

#[allow(clippy::too_many_arguments)]
fn write_external_summary_file(
    root: &std::path::Path,
    file_name: &str,
    suite: &str,
    shards: &[&str],
    samples: usize,
    questions: usize,
    evidence_questions: usize,
    any_evidence_hit: usize,
    all_evidence_hit: usize,
) -> std::path::PathBuf {
    let path = root.join(file_name);
    let body = serde_json::json!({
        "suite": suite,
        "completed": true,
        "shards": shards,
        "samples": samples,
        "questions": questions,
        "evidence_questions": evidence_questions,
        "any_evidence_hit": any_evidence_hit,
        "all_evidence_hit": all_evidence_hit,
        "write_errors": 0,
        "recall_errors": 0
    });
    fs::write(
        &path,
        serde_json::to_string_pretty(&body).expect("summary json"),
    )
    .expect("write summary file");
    path
}

fn external_summary(
    suite: &str,
    samples: usize,
    questions: usize,
    evidence_questions: usize,
    any_evidence_hit: usize,
    all_evidence_hit: usize,
) -> W4ExternalNoisyBenchmarkSummary {
    W4ExternalNoisyBenchmarkSummary {
        suite: suite.to_string(),
        completed: true,
        shards: vec![format!("{suite}.merged.summary.json")],
        summary_sha256: None,
        runner_source_sha256: None,
        samples,
        questions,
        evidence_questions,
        any_evidence_hit,
        all_evidence_hit,
        write_errors: 0,
        recall_errors: 0,
        stage_hit_counts: None,
        index_diagnostics: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn external_summary_with_stage_and_index(
    suite: &str,
    samples: usize,
    questions: usize,
    evidence_questions: usize,
    any_evidence_hit: usize,
    all_evidence_hit: usize,
    source_any: usize,
    source_all: usize,
    expanded_any: usize,
    expanded_all: usize,
    selected_any: usize,
    selected_all: usize,
    questions_with_index_report: usize,
    index_used_questions: usize,
    fallback_full_scan_questions: usize,
    indexed_neighbor_count: usize,
    failure_count: usize,
) -> W4ExternalNoisyBenchmarkSummary {
    let shards = if suite == "longmemeval_m_cleaned" {
        (0..8)
            .map(|index| format!("longmemeval_m_cleaned.shard-{index}-of-8.summary.json"))
            .collect::<Vec<_>>()
    } else {
        vec![format!("{suite}.shard-0-of-1.summary.json")]
    };
    serde_json::from_value(serde_json::json!({
        "suite": suite,
        "completed": true,
        "shards": shards,
        "summary_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "runner_source_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "samples": samples,
        "questions": questions,
        "evidence_questions": evidence_questions,
        "any_evidence_hit": any_evidence_hit,
        "all_evidence_hit": all_evidence_hit,
        "write_errors": 0,
        "recall_errors": 0,
        "stage_hit_counts": {
            "source_any_evidence_hit": source_any,
            "source_all_evidence_hit": source_all,
            "expanded_any_evidence_hit": expanded_any,
            "expanded_all_evidence_hit": expanded_all,
            "reranked_any_evidence_hit": expanded_any,
            "reranked_all_evidence_hit": expanded_all,
            "selected_any_evidence_hit": selected_any,
            "selected_all_evidence_hit": selected_all,
            "rendered_any_evidence_hit": selected_any,
            "rendered_all_evidence_hit": selected_all
        },
        "index_diagnostics": {
            "questions_with_index_report": questions_with_index_report,
            "index_used_questions": index_used_questions,
            "fallback_full_scan_questions": fallback_full_scan_questions,
            "source_candidate_count": questions_with_index_report,
            "matched_source_anchor_count": index_used_questions,
            "unmatched_source_anchor_count": fallback_full_scan_questions,
            "indexed_neighbor_count": indexed_neighbor_count,
            "filtered_node_count": indexed_neighbor_count,
            "filtered_edge_count": indexed_neighbor_count,
            "filtered_backlink_count": indexed_neighbor_count,
            "failure_count": failure_count
        }
    }))
    .expect("summary with stage and index diagnostics")
}

fn fixture_root() -> String {
    format!(
        "{}/../../fixtures/memory-benchmark-wall",
        env!("CARGO_MANIFEST_DIR")
    )
}
