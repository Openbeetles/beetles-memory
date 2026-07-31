use bm_replay::{
    evaluate_w4_external_noisy_wall, load_memory_benchmark_fixture_dir, run_memory_benchmark_wall,
    verify_p7_wall_input_context, w4_external_noisy_summary_with_provenance, MemoryBenchmarkClass,
    MemoryBenchmarkEvalRecall, MemoryBenchmarkEvalRecallAtK, MemoryBenchmarkEvalRecallDiagnostics,
    MemoryBenchmarkEvalRecallEvidenceRefIndexEntry, MemoryBenchmarkEvalRecallGoldRank,
    MemoryBenchmarkEvalRecallGraphDistanceToGold, MemoryBenchmarkEvalRecallMetrics,
    MemoryBenchmarkMode, MemoryBenchmarkSemanticDimension, P7RetainedFile,
    W4ExternalNoisyBenchmarkSummary, W4ExternalNoisyIndexDiagnostics,
    W4ExternalNoisyStageHitCounts,
};
use bm_sdk::ProfileId;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const P7_AUTHORITY_PROBE_SCHEMA_VERSION: &str = "p7_runner_authority_probe_v1";
const P7_AUTHORITY_PROBE_EXECUTABLE_NAME: &str = "beetle-memory-external-bench-runner";
const P7_AUTHORITY_PROBE_MANIFEST_NAME: &str = "authority-probe.json";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct P7RunnerAuthorityProbeReport {
    canonical_path: PathBuf,
    executable_sha256: String,
    manifest_sha256: String,
    publish_outcome: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct P7RunnerAuthorityProbeManifest {
    schema_version: String,
    executable_file_name: String,
    executable_sha256: String,
    executable_bytes: u64,
}

fn p7_test_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn p7_is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn verify_p7_runner_authority_probe(root: &Path, report_json: &[u8]) -> Result<(), String> {
    let root = fs::canonicalize(root)
        .map_err(|error| format!("failed to canonicalize authority probe root: {error}"))?;
    let report: P7RunnerAuthorityProbeReport = serde_json::from_slice(report_json)
        .map_err(|error| format!("invalid authority probe report JSON: {error}"))?;
    if !p7_is_lowercase_sha256(&report.executable_sha256)
        || !p7_is_lowercase_sha256(&report.manifest_sha256)
    {
        return Err("authority probe report contains an invalid SHA256".to_string());
    }
    if !matches!(
        report.publish_outcome.as_str(),
        "published" | "reused-identical"
    ) {
        return Err("authority probe report contains an invalid publish outcome".to_string());
    }

    let expected_directory = root
        .join("runner/authority-probes")
        .join(&report.executable_sha256);
    let expected_executable = expected_directory.join(P7_AUTHORITY_PROBE_EXECUTABLE_NAME);
    let canonical_executable = fs::canonicalize(&expected_executable)
        .map_err(|error| format!("failed to retain authority probe executable: {error}"))?;
    if report.canonical_path != canonical_executable
        || canonical_executable.parent() != Some(expected_directory.as_path())
        || expected_directory
            .file_name()
            .and_then(|name| name.to_str())
            != Some(report.executable_sha256.as_str())
    {
        return Err(
            "authority probe canonical path escaped its content-addressed directory".to_string(),
        );
    }

    let executable_identity = P7RetainedFile::open_executable(&canonical_executable)
        .and_then(P7RetainedFile::hash_once)
        .map_err(|error| format!("failed to retain authority probe executable: {error}"))?;
    if executable_identity.sha256 != report.executable_sha256 {
        return Err("authority probe executable digest differs from its report".to_string());
    }

    let manifest_path = expected_directory.join(P7_AUTHORITY_PROBE_MANIFEST_NAME);
    let retained_manifest = P7RetainedFile::open(&manifest_path)
        .map_err(|error| format!("failed to retain authority probe manifest: {error}"))?;
    if retained_manifest.admitted_len() > 64 * 1024 {
        return Err("authority probe manifest exceeds its byte budget".to_string());
    }
    let (manifest_bytes, manifest_identity) = retained_manifest
        .consume_once(|reader| {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes)?;
            Ok(bytes)
        })
        .map_err(|error| format!("failed to read authority probe manifest bytes: {error}"))?;
    if manifest_identity.sha256 != report.manifest_sha256 {
        return Err("authority probe manifest digest differs from its report".to_string());
    }
    let manifest: P7RunnerAuthorityProbeManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("invalid authority probe manifest JSON: {error}"))?;
    if manifest.schema_version != P7_AUTHORITY_PROBE_SCHEMA_VERSION
        || manifest.executable_file_name != P7_AUTHORITY_PROBE_EXECUTABLE_NAME
        || manifest.executable_sha256 != report.executable_sha256
        || manifest.executable_bytes != executable_identity.byte_len
    {
        return Err("authority probe manifest does not bind the executable bytes".to_string());
    }
    Ok(())
}

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
        ..MemoryBenchmarkEvalRecall::default()
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
fn memory_benchmark_wall_rejects_w41_eval_recall_without_diagnostics_and_pool_split() {
    let fixtures =
        load_memory_benchmark_fixture_dir(fixture_root()).expect("memory benchmark wall fixtures");
    let mut target = fixtures
        .iter()
        .find(|fixture| fixture.fixture_id == "recall-multisession-full-baseline")
        .expect("recall multisession full baseline")
        .clone();
    target.fixture_id = "w41-eval-recall-diagnostic-missing-full".to_string();
    target
        .semantic_contract
        .provided_keys
        .push("w4_eval_recall".to_string());
    target.eval_recall = Some(MemoryBenchmarkEvalRecall {
        suite: "internal_w41_contract".to_string(),
        split: "synthetic_diagnostic".to_string(),
        question_id: "q-w41-missing".to_string(),
        question_type: "temporal_update".to_string(),
        expected_evidence_refs: vec!["turn:release-manifest".to_string()],
        source_candidates: vec!["runtime_skill__release_guard".to_string()],
        graph_anchor_candidates: Vec::new(),
        expanded_candidates: vec![
            "runtime_skill__release_guard".to_string(),
            "graph:release_manifest_check".to_string(),
        ],
        eval_candidate_pool: Vec::new(),
        selected_candidates: vec!["graph:release_manifest_check".to_string()],
        rendered_candidates: vec!["graph:release_manifest_check".to_string()],
        rendered_block_preview: "graph:release_manifest_check [turn:release-manifest]".to_string(),
        rendered_evidence_refs: Vec::new(),
        evidence_ref_index: vec![MemoryBenchmarkEvalRecallEvidenceRefIndexEntry {
            candidate_id: "graph:release_manifest_check".to_string(),
            evidence_refs: vec!["turn:release-manifest".to_string()],
        }],
        missing_evidence_refs: Vec::new(),
        diagnostics: MemoryBenchmarkEvalRecallDiagnostics {
            evidence_count: 0,
            first_any_hit_stage: String::new(),
            first_all_hit_stage: String::new(),
            matched_gold_by_stage: Vec::new(),
            missing_gold_by_stage: Vec::new(),
            gold_rank_by_stage: vec![MemoryBenchmarkEvalRecallGoldRank {
                stage: "expanded".to_string(),
                evidence_ref: "turn:release-manifest".to_string(),
                rank: Some(2),
            }],
            miss_after_expanded: false,
            source_anchor_ids: Vec::new(),
            graph_anchor_candidate_ids: Vec::new(),
            expanded_node_ids: Vec::new(),
            graph_neighbor_ids: Vec::new(),
            graph_distance_to_gold: vec![MemoryBenchmarkEvalRecallGraphDistanceToGold {
                candidate_id: "graph:release_manifest_check".to_string(),
                evidence_ref: "turn:release-manifest".to_string(),
                distance: Some(1),
            }],
            truncated_count: 0,
            blocked_reasons: Vec::new(),
        },
        metrics: MemoryBenchmarkEvalRecallMetrics {
            recall_at_k: vec![
                MemoryBenchmarkEvalRecallAtK {
                    k: 5,
                    any_evidence_hit: true,
                    all_evidence_hit: true,
                    matched_evidence_refs: vec!["turn:release-manifest".to_string()],
                },
                MemoryBenchmarkEvalRecallAtK {
                    k: 10,
                    any_evidence_hit: true,
                    all_evidence_hit: true,
                    matched_evidence_refs: vec!["turn:release-manifest".to_string()],
                },
                MemoryBenchmarkEvalRecallAtK {
                    k: 20,
                    any_evidence_hit: true,
                    all_evidence_hit: true,
                    matched_evidence_refs: vec!["turn:release-manifest".to_string()],
                },
                MemoryBenchmarkEvalRecallAtK {
                    k: 50,
                    any_evidence_hit: true,
                    all_evidence_hit: true,
                    matched_evidence_refs: vec!["turn:release-manifest".to_string()],
                },
            ],
            mrr_bps: 10_000,
        },
    });

    let report = run_memory_benchmark_wall(&[target]);

    assert!(!report.passed);
    assert!(report.failures.iter().any(|failure| {
        failure.stage == "w4_eval_recall_contract"
            && failure.reason.contains("graph_anchor_candidates")
            && failure.reason.contains("eval_candidate_pool")
            && failure.reason.contains("rendered_evidence_refs")
            && failure.reason.contains("w4_1_diagnostics")
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
          "shards": [
            "locomo.shard-0-of-10.summary.json",
            "locomo.shard-1-of-10.summary.json",
            "locomo.shard-2-of-10.summary.json",
            "locomo.shard-3-of-10.summary.json",
            "locomo.shard-4-of-10.summary.json",
            "locomo.shard-5-of-10.summary.json",
            "locomo.shard-6-of-10.summary.json",
            "locomo.shard-7-of-10.summary.json",
            "locomo.shard-8-of-10.summary.json",
            "locomo.shard-9-of-10.summary.json"
          ],
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
          "shards": [
            "locomo.shard-0-of-10.summary.json",
            "locomo.shard-1-of-10.summary.json",
            "locomo.shard-2-of-10.summary.json",
            "locomo.shard-3-of-10.summary.json",
            "locomo.shard-4-of-10.summary.json",
            "locomo.shard-5-of-10.summary.json",
            "locomo.shard-6-of-10.summary.json",
            "locomo.shard-7-of-10.summary.json",
            "locomo.shard-8-of-10.summary.json",
            "locomo.shard-9-of-10.summary.json"
          ],
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

    let report = evaluate_w4_external_noisy_wall(&[
        attach_facet_ablation(attach_w41_diagnostics(locomo.clone()), 0),
        attach_facet_ablation(attach_w41_diagnostics(oracle.clone()), 0),
        attach_facet_ablation(attach_w41_diagnostics(s_cleaned.clone()), 0),
        attach_facet_ablation(attach_w41_diagnostics(m_cleaned.clone()), 0),
    ]);

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
          "shards": [
            "locomo.shard-0-of-10.summary.json",
            "locomo.shard-1-of-10.summary.json",
            "locomo.shard-2-of-10.summary.json",
            "locomo.shard-3-of-10.summary.json",
            "locomo.shard-4-of-10.summary.json",
            "locomo.shard-5-of-10.summary.json",
            "locomo.shard-6-of-10.summary.json",
            "locomo.shard-7-of-10.summary.json",
            "locomo.shard-8-of-10.summary.json",
            "locomo.shard-9-of-10.summary.json"
          ],
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
            "failure_count": 1866,
            "graph_manifest_contract_verified_questions": 120,
            "graph_selected_dependency_chain_verified_questions": 120,
            "graph_full_scope_closure_verified_questions": 0,
            "graph_manifest_generation_present_questions": 120,
            "graph_revision_present_questions": 120,
            "graph_scope_digest_present_questions": 120,
            "graph_maintenance_required_questions": 0,
            "graph_incident_questions": 0,
            "graph_read_path_mutation_delta": 0
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

    let report = evaluate_w4_external_noisy_wall(&[
        attach_facet_ablation(attach_w41_diagnostics(locomo.clone()), 0),
        attach_facet_ablation(attach_w41_diagnostics(oracle.clone()), 0),
        attach_facet_ablation(attach_w41_diagnostics(s_cleaned.clone()), 0),
        attach_facet_ablation(attach_w41_diagnostics(m_cleaned.clone()), 0),
    ]);

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
        "locomo", 10, 1986, 1982, 685, 553, 85, 57, 1914, 1874, 685, 553, 1986, 1986, 0, 1_111_121,
        0,
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
        500,
        494,
        0,
        772,
        0,
    );
    let s_cleaned = external_summary_with_stage_and_index(
        "longmemeval_s_cleaned",
        500,
        500,
        500,
        475,
        405,
        246,
        95,
        475,
        405,
        475,
        405,
        500,
        500,
        0,
        17_632,
        0,
    );
    let m_cleaned = external_summary_with_stage_and_index(
        "longmemeval_m_cleaned",
        500,
        500,
        500,
        201,
        107,
        20,
        7,
        319,
        191,
        201,
        107,
        500,
        500,
        0,
        79_298,
        0,
    );

    let report = evaluate_w4_external_noisy_wall(&[
        attach_facet_ablation(attach_w41_diagnostics(locomo.clone()), 0),
        attach_facet_ablation(attach_w41_diagnostics(oracle.clone()), 0),
        attach_facet_ablation(attach_w41_diagnostics(s_cleaned.clone()), 0),
        attach_facet_ablation(attach_w41_diagnostics(m_cleaned.clone()), 0),
    ]);

    assert!(report.noisy_improvement_proven);
    assert!(report.stage_attributed_improvement_proven);
    assert!(report.index_effect_proven);
    assert!(!report.release_gate_passed);
    assert!(!report.p7_loss_ledger_attached);
    let m_report = report
        .suite_reports
        .iter()
        .find(|suite| suite.suite == "longmemeval_m_cleaned")
        .expect("m_cleaned report");
    assert!(m_report.stage_attributed_improvement);
    assert!(m_report.index_effect_proven);

    let mut rendered_gap_m_cleaned = attach_facet_ablation(attach_w41_diagnostics(m_cleaned), 0);
    let rendered_gap_stage = rendered_gap_m_cleaned
        .stage_hit_counts
        .as_mut()
        .expect("stage counts");
    rendered_gap_stage.rendered_any_evidence_hit = 14;
    rendered_gap_stage.rendered_all_evidence_hit = 5;
    let rendered_gap_report = evaluate_w4_external_noisy_wall(&[
        attach_facet_ablation(attach_w41_diagnostics(locomo), 0),
        attach_facet_ablation(attach_w41_diagnostics(oracle), 0),
        attach_facet_ablation(attach_w41_diagnostics(s_cleaned), 0),
        rendered_gap_m_cleaned,
    ]);
    assert!(!rendered_gap_report.stage_attributed_improvement_proven);
    assert!(rendered_gap_report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_stage_attribution_not_proven".to_string()));
}

#[test]
fn w4_external_noisy_wall_requires_w41_summary_diagnostics_for_current_release() {
    let locomo = external_summary_with_stage_and_index(
        "locomo", 10, 1986, 1982, 685, 553, 85, 57, 1914, 1874, 685, 553, 1986, 1986, 0, 1_111_121,
        0,
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
        500,
        494,
        0,
        772,
        0,
    );
    let s_cleaned = external_summary_with_stage_and_index(
        "longmemeval_s_cleaned",
        500,
        500,
        500,
        475,
        405,
        246,
        95,
        475,
        405,
        475,
        405,
        500,
        500,
        0,
        17_632,
        0,
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

    assert!(!report.w4_1_diagnostics_attached);
    assert!(!report.release_gate_passed);
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_w4_1_diagnostics_missing".to_string()));
}

#[test]
fn w4_external_noisy_wall_requires_facet_ablation_and_no_render_growth() {
    let locomo = attach_w41_diagnostics(external_summary_with_stage_and_index(
        "locomo", 10, 1986, 1982, 685, 553, 85, 57, 1914, 1874, 685, 553, 1986, 1986, 0, 1_111_121,
        0,
    ));
    let oracle = attach_w41_diagnostics(external_summary_with_stage_and_index(
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
        500,
        494,
        0,
        772,
        0,
    ));
    let s_cleaned = attach_w41_diagnostics(external_summary_with_stage_and_index(
        "longmemeval_s_cleaned",
        500,
        500,
        500,
        475,
        405,
        246,
        95,
        475,
        405,
        475,
        405,
        500,
        500,
        0,
        17_632,
        0,
    ));
    let m_cleaned = attach_w41_diagnostics(external_summary_with_stage_and_index(
        "longmemeval_m_cleaned",
        500,
        500,
        500,
        201,
        107,
        20,
        7,
        319,
        191,
        201,
        107,
        500,
        500,
        0,
        79_298,
        0,
    ));

    let missing = evaluate_w4_external_noisy_wall(&[
        locomo.clone(),
        oracle.clone(),
        s_cleaned.clone(),
        m_cleaned.clone(),
    ]);
    assert!(!missing.facet_ablation_attached);
    assert!(!missing.release_gate_passed);
    assert!(missing
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_facet_ablation_missing".to_string()));

    let growth = evaluate_w4_external_noisy_wall(&[
        attach_facet_ablation(locomo.clone(), 0),
        attach_facet_ablation(oracle.clone(), 0),
        attach_facet_ablation(s_cleaned.clone(), 0),
        attach_facet_ablation(m_cleaned.clone(), 1),
    ]);
    assert!(growth.facet_ablation_attached);
    assert!(!growth.facet_ablation_no_render_growth);
    assert!(growth
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_render_growth_detected".to_string()));

    let mut partial_ablation_m_cleaned = attach_facet_ablation(m_cleaned.clone(), 0);
    partial_ablation_m_cleaned
        .facet_ablation
        .as_mut()
        .expect("facet ablation")
        .report_available_slice_counts
        .insert("facet_off".to_string(), 499);
    let partial_ablation = evaluate_w4_external_noisy_wall(&[
        attach_facet_ablation(locomo.clone(), 0),
        attach_facet_ablation(oracle.clone(), 0),
        attach_facet_ablation(s_cleaned.clone(), 0),
        partial_ablation_m_cleaned,
    ]);
    assert!(!partial_ablation.facet_ablation_attached);
    assert!(partial_ablation
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_facet_ablation_missing".to_string()));

    let mut synthetic_ablation_m_cleaned = attach_facet_ablation(m_cleaned.clone(), 0);
    synthetic_ablation_m_cleaned
        .facet_ablation
        .as_mut()
        .expect("facet ablation")
        .method_counts
        .clear();
    let synthetic_ablation = evaluate_w4_external_noisy_wall(&[
        attach_facet_ablation(locomo.clone(), 0),
        attach_facet_ablation(oracle.clone(), 0),
        attach_facet_ablation(s_cleaned.clone(), 0),
        synthetic_ablation_m_cleaned,
    ]);
    assert!(!synthetic_ablation.facet_ablation_attached);
    assert!(synthetic_ablation
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_facet_ablation_missing".to_string()));

    let mut blocked_ablation_m_cleaned = attach_facet_ablation(m_cleaned.clone(), 0);
    blocked_ablation_m_cleaned
        .facet_ablation
        .as_mut()
        .expect("facet ablation")
        .blocked_reason_counts
        .insert("memory_facet_index_not_used".to_string(), 1);
    let blocked_ablation = evaluate_w4_external_noisy_wall(&[
        attach_facet_ablation(locomo.clone(), 0),
        attach_facet_ablation(oracle.clone(), 0),
        attach_facet_ablation(s_cleaned.clone(), 0),
        blocked_ablation_m_cleaned,
    ]);
    assert!(blocked_ablation.facet_ablation_attached);
    assert!(!blocked_ablation.facet_ablation_effect_proven);
    assert!(blocked_ablation
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_facet_ablation_effect_not_proven".to_string()));

    let ready = evaluate_w4_external_noisy_wall(&[
        attach_facet_ablation(locomo, 0),
        attach_facet_ablation(oracle, 0),
        attach_facet_ablation(s_cleaned, 0),
        attach_facet_ablation(m_cleaned, 0),
    ]);
    assert!(ready.facet_ablation_attached);
    assert!(ready.facet_ablation_effect_proven);
    assert!(ready.facet_ablation_no_render_growth);
    assert!(!ready.release_gate_passed);
    assert!(!ready.p7_loss_ledger_attached);
}

#[test]
fn external_noisy_wall_requires_p7_selection_render_and_production_proof() {
    let locomo = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "locomo", 10, 1986, 1982, 931, 818, 85, 57, 1914, 1874, 931, 818, 1986, 1986, 0,
            1_111_121, 0,
        )),
        0,
    );
    let oracle = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
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
            500,
            494,
            0,
            772,
            0,
        )),
        0,
    );
    let s_cleaned = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "longmemeval_s_cleaned",
            500,
            500,
            500,
            475,
            405,
            246,
            95,
            475,
            405,
            475,
            405,
            500,
            500,
            0,
            17_632,
            0,
        )),
        0,
    );
    let m_cleaned = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "longmemeval_m_cleaned",
            500,
            500,
            500,
            225,
            124,
            20,
            7,
            319,
            191,
            225,
            124,
            500,
            500,
            0,
            79_298,
            0,
        )),
        0,
    );

    let report = evaluate_w4_external_noisy_wall(&[locomo, oracle, s_cleaned, m_cleaned]);

    assert!(!report.p7_loss_ledger_attached);
    assert!(!report.p7_ablation_effect_proven);
    assert!(!report.p7_production_delivery_proven);
    assert!(!report.p7_provenance_valid);
    assert!(!report.release_gate_passed);
    for reason in [
        "p7_loss_ledger_missing",
        "p7_ablation_effect_not_proven",
        "p7_production_delivery_not_proven",
        "p7_provenance_invalid",
    ] {
        assert!(report
            .blocked_reasons
            .iter()
            .any(|blocked| blocked == reason));
    }
}

#[test]
fn external_noisy_wall_rejects_complete_but_unverified_p7_release_evidence() {
    let locomo = attach_p7_release_evidence(attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "locomo", 10, 1986, 1982, 931, 818, 85, 57, 1914, 1874, 931, 818, 1986, 1986, 0,
            1_111_121, 0,
        )),
        0,
    ));
    let oracle = attach_p7_release_evidence(attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
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
            500,
            494,
            0,
            772,
            0,
        )),
        0,
    ));
    let s_cleaned = attach_p7_release_evidence(attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "longmemeval_s_cleaned",
            500,
            500,
            500,
            475,
            405,
            246,
            95,
            475,
            405,
            475,
            405,
            500,
            500,
            0,
            17_632,
            0,
        )),
        0,
    ));
    let m_cleaned = attach_p7_release_evidence(attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "longmemeval_m_cleaned",
            500,
            500,
            500,
            225,
            124,
            20,
            7,
            319,
            191,
            225,
            124,
            500,
            500,
            0,
            79_298,
            0,
        )),
        0,
    ));

    let report = evaluate_w4_external_noisy_wall(&[locomo, oracle, s_cleaned, m_cleaned]);

    assert!(report.p7_loss_ledger_attached);
    assert!(report.p7_selection_loss_reduced);
    assert!(report.p7_render_loss_reduced);
    assert!(report.p7_ablation_effect_proven);
    assert!(report.p7_no_render_growth);
    assert!(report.p7_index_no_full_scan);
    assert!(report.p7_no_privacy_regression);
    assert!(!report.p7_soul_regression_gate.passed);
    assert!(report.p7_no_p6_regression);
    assert!(report.p7_production_delivery_proven);
    assert!(report
        .suite_reports
        .iter()
        .filter_map(|suite| suite.facet_ablation.as_ref())
        .all(|ablation| ablation
            .evidence_family_rotation_selected_all_hit_loss_count
            .is_empty()));
    assert!(!report.p7_provenance_valid);
    assert!(!report.release_gate_passed);
    assert!(report
        .blocked_reasons
        .contains(&"p7_provenance_invalid".to_string()));
}

#[test]
fn external_noisy_wall_rejects_mixed_run_cohorts() {
    let mut summaries = [
        external_summary("locomo", 10, 1986, 1982, 0, 0),
        external_summary("longmemeval_oracle", 500, 500, 500, 0, 0),
        external_summary("longmemeval_s_cleaned", 500, 500, 500, 0, 0),
        external_summary("longmemeval_m_cleaned", 500, 500, 500, 0, 0),
    ];
    for (index, summary) in summaries.iter_mut().enumerate() {
        let run_id = if index == 3 { "run-b" } else { "run-a" };
        summary.run_id = run_id.to_string();
        summary.p7_provenance = Some(bm_replay::P7MergedProvenance {
            run_id: run_id.to_string(),
            ..bm_replay::P7MergedProvenance::default()
        });
    }

    let report = evaluate_w4_external_noisy_wall(&summaries);

    assert!(!report.cohort_valid);
    assert_eq!(report.run_id, None);
    assert!(report
        .blocked_reasons
        .contains(&"p7_run_cohort_invalid".to_string()));
}

#[test]
fn p7_runner_preflight_cli_rejects_untrusted_runner_before_wall() {
    let root = std::env::temp_dir().join(format!("bm-p7-preflight-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let runner = root.join("runner");
    fs::create_dir_all(runner.join("src")).expect("runner src");
    fs::create_dir_all(runner.join("target/release")).expect("runner target");
    fs::write(runner.join("Cargo.toml"), "[package]\nname='fixture'\n").expect("manifest");
    fs::write(runner.join("Cargo.lock"), "lock\n").expect("lock");
    fs::write(runner.join("build.rs"), "fn main() {}\n").expect("build script");
    fs::write(runner.join("src/main.rs"), "fn main() {}\n").expect("runner source");
    let executable = runner.join("target/release/beetle-memory-external-bench-runner");
    let executed_marker = root.join("identity-command-executed");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\n: > '{}'\nprintf '%s\\n' '{{\"sdk_build_fingerprint\":\"{}\",\"runner_build_fingerprint\":\"{}\",\"runner_lock_fingerprint\":\"{}\",\"build_profile\":\"release\",\"executable_sha256\":\"{}\"}}'\n",
            executed_marker.display(),
            "0".repeat(64),
            "1".repeat(64),
            "2".repeat(64),
            "3".repeat(64),
        ),
    )
    .expect("stale runner executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&executable)
            .expect("runner metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("runner executable permissions");
    }

    let operator =
        std::env::var("CARGO_BIN_EXE_bm-w4-external-noisy-wall").expect("operator binary path");
    let output = Command::new(operator)
        .arg("--preflight")
        .arg("--benchmark-root")
        .arg(&root)
        .output()
        .expect("run standalone P7 preflight");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("operator stderr utf8");
    assert!(stderr.contains("preflight"), "{stderr}");
    assert!(!stderr.contains("unknown argument"), "{stderr}");
    assert!(
        !executed_marker.exists(),
        "untrusted runner must not be executed"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn w4_external_noisy_wall_requires_every_noisy_split_to_improve_against_w43_baseline() {
    let locomo_at_baseline = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "locomo", 10, 1986, 1982, 297, 189, 85, 57, 1914, 1874, 297, 189, 1986, 1986, 0,
            1_111_121, 0,
        )),
        0,
    );
    let oracle = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
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
            500,
            494,
            0,
            772,
            0,
        )),
        0,
    );
    let s_cleaned = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "longmemeval_s_cleaned",
            500,
            500,
            500,
            475,
            405,
            246,
            95,
            475,
            405,
            475,
            405,
            500,
            500,
            0,
            17_632,
            0,
        )),
        0,
    );
    let m_cleaned = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "longmemeval_m_cleaned",
            500,
            500,
            500,
            201,
            107,
            20,
            7,
            319,
            191,
            201,
            107,
            500,
            500,
            0,
            79_298,
            0,
        )),
        0,
    );

    let report =
        evaluate_w4_external_noisy_wall(&[locomo_at_baseline, oracle, s_cleaned, m_cleaned]);

    assert!(!report.noisy_improvement_proven);
    assert!(report.stage_attributed_improvement_proven);
    assert!(report.index_effect_proven);
    assert!(report.facet_ablation_effect_proven);
    assert!(!report.release_gate_passed);
    assert!(report
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_improvement_not_proven".to_string()));
}

#[test]
fn w4_external_noisy_wall_rejects_full_scan_and_wrong_shard_total() {
    let mut locomo_full_scan = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "locomo", 10, 1986, 1982, 685, 553, 85, 57, 1914, 1874, 685, 553, 1986, 1985, 1,
            1_111_121, 1,
        )),
        0,
    );
    let oracle = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
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
            500,
            494,
            0,
            772,
            0,
        )),
        0,
    );
    let s_cleaned = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "longmemeval_s_cleaned",
            500,
            500,
            500,
            475,
            405,
            246,
            95,
            475,
            405,
            475,
            405,
            500,
            500,
            0,
            17_632,
            0,
        )),
        0,
    );
    let m_cleaned = attach_facet_ablation(
        attach_w41_diagnostics(external_summary_with_stage_and_index(
            "longmemeval_m_cleaned",
            500,
            500,
            500,
            201,
            107,
            20,
            7,
            319,
            191,
            201,
            107,
            500,
            500,
            0,
            79_298,
            0,
        )),
        0,
    );

    let full_scan = evaluate_w4_external_noisy_wall(&[
        locomo_full_scan.clone(),
        oracle.clone(),
        s_cleaned.clone(),
        m_cleaned.clone(),
    ]);
    assert!(!full_scan.index_no_full_scan);
    assert!(!full_scan.release_gate_passed);
    assert!(full_scan
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_index_full_scan_detected".to_string()));

    locomo_full_scan
        .index_diagnostics
        .as_mut()
        .expect("index diagnostics")
        .fallback_full_scan_questions = 0;
    locomo_full_scan
        .index_diagnostics
        .as_mut()
        .expect("index diagnostics")
        .failure_count = 0;
    locomo_full_scan
        .index_diagnostics
        .as_mut()
        .expect("index diagnostics")
        .facet_posting_key_lookup_count = 0;
    let missing_exact_posting_proof = evaluate_w4_external_noisy_wall(&[
        locomo_full_scan.clone(),
        oracle.clone(),
        s_cleaned.clone(),
        m_cleaned.clone(),
    ]);
    assert!(!missing_exact_posting_proof.index_no_full_scan);
    assert!(!missing_exact_posting_proof.release_gate_passed);
    locomo_full_scan
        .index_diagnostics
        .as_mut()
        .expect("index diagnostics")
        .facet_posting_key_lookup_count = 1986;

    let mut zero_posting_question = locomo_full_scan.clone();
    zero_posting_question
        .index_diagnostics
        .as_mut()
        .expect("index diagnostics")
        .facet_zero_posting_key_lookup_questions = 1;
    let zero_posting_report = evaluate_w4_external_noisy_wall(&[
        zero_posting_question,
        oracle.clone(),
        s_cleaned.clone(),
        m_cleaned.clone(),
    ]);
    assert!(!zero_posting_report.p7_index_no_full_scan);

    let mut clean_zero_hit_question = locomo_full_scan.clone();
    clean_zero_hit_question
        .index_diagnostics
        .as_mut()
        .expect("index diagnostics")
        .facet_clean_zero_hit_questions = 1;
    let clean_zero_hit_report = evaluate_w4_external_noisy_wall(&[
        clean_zero_hit_question,
        oracle.clone(),
        s_cleaned.clone(),
        m_cleaned.clone(),
    ]);
    assert!(clean_zero_hit_report.p7_index_no_full_scan);

    let mut manifest_integrity_failure = locomo_full_scan.clone();
    manifest_integrity_failure
        .index_diagnostics
        .as_mut()
        .expect("index diagnostics")
        .facet_manifest_integrity_failure_count = 1;
    let manifest_integrity_report = evaluate_w4_external_noisy_wall(&[
        manifest_integrity_failure,
        oracle.clone(),
        s_cleaned.clone(),
        m_cleaned.clone(),
    ]);
    assert!(!manifest_integrity_report.p7_index_no_full_scan);

    locomo_full_scan.shards = vec!["locomo.shard-0-of-1.summary.json".to_string()];
    let wrong_shard_total =
        evaluate_w4_external_noisy_wall(&[locomo_full_scan, oracle, s_cleaned, m_cleaned]);
    assert!(!wrong_shard_total.shards_valid);
    assert!(!wrong_shard_total.release_gate_passed);
    assert!(wrong_shard_total
        .blocked_reasons
        .contains(&"w4_external_noisy_wall_shards_invalid".to_string()));
}

#[test]
fn w4_external_noisy_wall_blocks_final_improvement_without_stage_or_index_effect() {
    let locomo = external_summary_with_stage_and_index(
        "locomo", 10, 1986, 1982, 685, 553, 85, 57, 1914, 1874, 685, 553, 1986, 1986, 0, 1_111_121,
        0,
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
        500,
        494,
        0,
        772,
        0,
    );
    let s_cleaned = external_summary_with_stage_and_index(
        "longmemeval_s_cleaned",
        500,
        500,
        500,
        475,
        405,
        246,
        95,
        475,
        405,
        475,
        405,
        500,
        500,
        0,
        17_632,
        0,
    );
    let m_cleaned = external_summary_with_stage_and_index(
        "longmemeval_m_cleaned",
        500,
        500,
        500,
        201,
        107,
        201,
        107,
        201,
        107,
        201,
        107,
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
fn w4_external_noisy_operator_hashes_actual_bytes_without_blessing_old_provenance() {
    let locomo = w4_external_noisy_summary_with_provenance(
        r#"{
          "suite": "locomo",
          "completed": true,
          "shards": [
            "locomo.shard-0-of-10.summary.json",
            "locomo.shard-1-of-10.summary.json",
            "locomo.shard-2-of-10.summary.json",
            "locomo.shard-3-of-10.summary.json",
            "locomo.shard-4-of-10.summary.json",
            "locomo.shard-5-of-10.summary.json",
            "locomo.shard-6-of-10.summary.json",
            "locomo.shard-7-of-10.summary.json",
            "locomo.shard-8-of-10.summary.json",
            "locomo.shard-9-of-10.summary.json"
          ],
          "samples": 10,
          "questions": 1986,
          "evidence_questions": 1982,
          "any_evidence_hit": 21,
          "all_evidence_hit": 13,
          "write_errors": 0,
          "recall_errors": 0
        }"#,
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
    )
    .expect("m_cleaned summary with provenance");

    let report = evaluate_w4_external_noisy_wall(&[locomo, oracle, s_cleaned, m_cleaned]);

    assert!(!report.provenance_attached);
    assert!(!report.stage_diagnostics_attached);
    assert!(!report.release_gate_passed);
    assert!(report
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
fn w4_external_noisy_operator_rejects_self_consistent_untrusted_fingerprints() {
    let root = std::env::temp_dir().join(format!("bm-w4-p7-trust-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("results")).expect("temp results root");
    let merged_path = root
        .join("results")
        .join("longmemeval_oracle.merged.summary.json");
    let producer_identity = fixture_recorded_producer_identity();
    let merged_json = serde_json::json!({
        "suite": "longmemeval_oracle",
        "run_id": "run-1",
        "completed": true,
        "shards": ["longmemeval_oracle.shard-0-of-1.summary.json"],
        "p7_provenance": {
        "schema_version": bm_replay::P7_MERGED_PROVENANCE_SCHEMA_VERSION,
        "run_id": "run-1",
        "contract_version": "p7_recall_delivery_v2",
        "sdk_report_schema_version": bm_sdk::MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
        "sdk_build_fingerprint": "a".repeat(64),
        "runner_build_fingerprint": "b".repeat(64),
        "runner_lock_fingerprint": "c".repeat(64),
        "executable_sha256": "d".repeat(64),
        "gate_attestation_sha256": "1".repeat(64),
        "release_metadata_sha256": "3".repeat(64),
        "gate_source_fingerprint": "2".repeat(64),
        "gate_source_manifest_sha256": "4".repeat(64),
        "gate_ids": bm_replay::P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec(),
        "cohort_admission_sha256": "5".repeat(64),
        "build_profile": "release",
        "input_sha256": "821a2034d219ab45846873dd14c14f12cfe7776e73527a483f9dac095d38620c",
        "detail_schema_version": "p7_question_detail_v1",
        "producer_identity": producer_identity,
        "merged_detail_sha256": "e".repeat(64),
        "ordered_shard_digest_manifest": [{
            "shard": "longmemeval_oracle.shard-0-of-1.summary.json",
            "summary_sha256": "f".repeat(64),
            "detail_sha256": "0".repeat(64),
        }]
    }});
    let merged_body = serde_json::to_string(&merged_json).expect("serialize merged summary");
    fs::write(&merged_path, &merged_body).expect("write merged summary");
    let summary =
        w4_external_noisy_summary_with_provenance(&merged_body).expect("parse merged summary");
    assert!(verify_p7_wall_input_context(
        std::slice::from_ref(&merged_path),
        &root.join("results/runs/run-1/preflight-report.json"),
    )
    .is_err());
    assert!(!summary.operator_content_hash_verified());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn w4_external_noisy_operator_rejects_changed_dataset_hash_claim() {
    let producer_identity = fixture_recorded_producer_identity();
    let body = serde_json::json!({
        "suite": "longmemeval_oracle",
        "run_id": "run-1",
        "shards": ["longmemeval_oracle.shard-0-of-1.summary.json"],
        "p7_provenance": {
            "schema_version": bm_replay::P7_MERGED_PROVENANCE_SCHEMA_VERSION,
            "run_id": "run-1",
            "contract_version": "p7_recall_delivery_v2",
            "sdk_report_schema_version": bm_sdk::MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
            "sdk_build_fingerprint": "a".repeat(64),
            "runner_build_fingerprint": "b".repeat(64),
            "runner_lock_fingerprint": "c".repeat(64),
            "executable_sha256": "d".repeat(64),
            "gate_attestation_sha256": "1".repeat(64),
            "release_metadata_sha256": "3".repeat(64),
            "gate_source_fingerprint": "2".repeat(64),
            "gate_source_manifest_sha256": "4".repeat(64),
            "gate_ids": bm_replay::P7_REQUIRED_RELEASE_GATE_IDS.map(str::to_string).to_vec(),
            "cohort_admission_sha256": "5".repeat(64),
            "build_profile": "release",
            "input_sha256": "0".repeat(64),
            "detail_schema_version": "p7_question_detail_v1",
            "producer_identity": producer_identity,
            "merged_detail_sha256": "e".repeat(64),
            "ordered_shard_digest_manifest": []
        }
    })
    .to_string();
    let summary = w4_external_noisy_summary_with_provenance(&body).expect("parse summary");
    let path = std::env::temp_dir().join("longmemeval_oracle.merged.summary.json");

    assert!(verify_p7_wall_input_context(
        std::slice::from_ref(&path),
        &std::env::temp_dir().join("results/runs/run-1/preflight-report.json"),
    )
    .is_err());
    assert!(!summary.operator_content_hash_verified());
}

#[test]
fn w4_external_noisy_operator_cli_requires_typed_preflight_before_reading_summaries() {
    let root =
        std::env::temp_dir().join(format!("bm-w4-external-noisy-wall-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temp operator root");
    let locomo_shards = expected_external_shards("locomo");
    let locomo_shard_refs = locomo_shards.iter().map(String::as_str).collect::<Vec<_>>();
    let locomo = write_external_summary_file(
        &root,
        "locomo.merged.summary.json",
        "locomo",
        &locomo_shard_refs,
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
        .arg("--summary")
        .arg(&locomo)
        .arg("--summary")
        .arg(&oracle)
        .arg("--summary")
        .arg(&s_cleaned)
        .arg("--summary")
        .arg(&m_cleaned)
        .output()
        .expect("run W4 external noisy operator");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("operator stderr utf8");
    assert!(stderr.contains("--preflight-report"), "{stderr}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn w4_external_noisy_operator_script_has_no_blocked_success_override() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let operator =
        fs::read_to_string(repo_root.join("scripts/check_w4_external_noisy_wall_operator.sh"))
            .expect("operator script");
    let wall = fs::read_to_string(repo_root.join("scripts/check_memory_benchmark_wall.sh"))
        .expect("memory wall script");
    let operator_cli =
        fs::read_to_string(repo_root.join("crates/replay/src/bin/bm-w4-external-noisy-wall.rs"))
            .expect("operator CLI");
    let preflight =
        fs::read_to_string(repo_root.join("scripts/check_w4_external_noisy_wall_preflight.sh"))
            .expect("preflight script");

    for obsolete in [
        "BM_W4_EXTERNAL_EXPECT_BLOCKED",
        "baseline blocked as expected",
        "is_expected_current_baseline_block",
    ] {
        assert!(
            !operator.contains(obsolete),
            "obsolete operator bypass: {obsolete}"
        );
        assert!(!wall.contains(obsolete), "obsolete wall bypass: {obsolete}");
        assert!(
            !operator_cli.contains(obsolete),
            "obsolete operator CLI classification: {obsolete}"
        );
    }
    assert!(operator.contains("exit \"$status\""));
    assert!(operator.contains("--preflight-report"));
    assert!(!operator.contains("rg "));
    assert!(!preflight.contains("rg "));
    assert!(preflight.contains("bm-p7-retained-launch"));
    assert!(preflight.contains("\"$launcher\" --executable \"$publisher\" --"));
    assert!(preflight.contains("--publish-verifier-release"));
    assert!(preflight.contains("\"$launcher\" --executable \"$verifier\" --"));
    assert!(!operator_cli.contains("ExitCode::from(10)"));
}

#[test]
fn memory_write_transaction_gate_runs_durability_and_concurrency_contracts() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let gate =
        fs::read_to_string(repo_root.join("scripts/check_memory_write_transaction_contract.sh"))
            .expect("memory write transaction gate");

    for required in [
        "--test file_transaction_recovery_contract",
        "--test file_primitive_concurrency_contract",
        "--test store_concurrency_contract",
        "--features sqlite-store --test sqlite_multiprocess_transaction_contract",
    ] {
        assert!(
            gate.contains(required),
            "missing transaction gate: {required}"
        );
    }
}

#[cfg(unix)]
#[test]
fn p7_operator_rejects_invalid_run_id_before_building_cohort_paths() {
    use std::os::unix::fs::PermissionsExt;

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let script = repo_root.join("scripts/check_w4_external_noisy_wall_operator.sh");
    let root = std::env::temp_dir().join(format!(
        "bm-p7-operator-run-id-contract-{}",
        std::process::id()
    ));
    let fake_bin = root.join("bin");
    let bench_root = root.join("bench");
    let cargo_marker = root.join("cargo-called");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&fake_bin).expect("create fake bin");
    fs::create_dir_all(&bench_root).expect("create bench root");
    let fake_cargo = fake_bin.join("cargo");
    fs::write(
        &fake_cargo,
        format!("#!/usr/bin/env bash\ntouch {:?}\nexit 99\n", cargo_marker),
    )
    .expect("write fake cargo");
    let mut permissions = fs::metadata(&fake_cargo)
        .expect("fake cargo metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_cargo, permissions).expect("fake cargo permissions");
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").expect("test PATH")
    );

    for run_id in [
        ".",
        "..",
        "../escape",
        "nested/run",
        "run id",
        "run:1",
        "\u{8fd0}\u{884c}",
    ] {
        let output = Command::new("bash")
            .arg(&script)
            .env("BM_W4_EXTERNAL_BENCH_ROOT", &bench_root)
            .env("BM_P7_RUN_ID", run_id)
            .env("PATH", &path)
            .output()
            .expect("run operator script");
        assert_eq!(
            output.status.code(),
            Some(2),
            "run_id={run_id:?}: {output:?}"
        );
        let stderr = String::from_utf8(output.stderr).expect("operator stderr utf8");
        assert!(
            stderr
                .contains("BM_P7_RUN_ID must match ASCII [A-Za-z0-9._-]+ and must not be . or .."),
            "run_id={run_id:?}: {stderr}"
        );
        assert!(!cargo_marker.exists(), "run_id={run_id:?} reached cargo");
    }

    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn p7_operator_routes_verifier_publisher_through_retained_launcher() {
    use std::os::unix::fs::PermissionsExt;

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let script = repo_root.join("scripts/check_w4_external_noisy_wall_operator.sh");
    let preflight_script = repo_root.join("scripts/check_w4_external_noisy_wall_preflight.sh");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bm-p7-retained-publisher-contract-{}-{nonce}",
        std::process::id()
    ));
    let fake_bin = root.join("bin");
    let target_release = root.join("target/release");
    let bench_root = root.join("bench");
    let launcher_log = root.join("launcher.log");
    let direct_publisher_marker = root.join("publisher-directly-executed");
    let fake_cargo = fake_bin.join("cargo");
    let publisher = target_release.join("bm-w4-external-noisy-wall");
    let launcher = target_release.join("bm-p7-retained-launch");
    let verifier = root.join("published-verifier");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&fake_bin).expect("create fake bin");
    fs::create_dir_all(&target_release).expect("create fake target");
    fs::create_dir_all(&bench_root).expect("create benchmark root");

    fs::write(&fake_cargo, "#!/bin/sh\nexit 0\n").expect("write fake cargo");
    fs::write(
        &publisher,
        "#!/bin/sh\ntouch \"$BM_P7_DIRECT_PUBLISHER_MARKER\"\nexit 97\n",
    )
    .expect("write direct publisher trap");
    fs::write(
        &launcher,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$BM_P7_LAUNCHER_LOG"
[ "$1" = "--executable" ]
executable="$2"
shift 2
[ "$1" = "--" ]
shift
if [ "$executable" = "$BM_P7_EXPECTED_PUBLISHER" ]; then
  [ "$1" = "--publish-verifier-release" ]
  printf '%s\n' "$BM_P7_FAKE_VERIFIER"
  exit 0
fi
[ "$executable" = "$BM_P7_FAKE_VERIFIER" ]
[ "$1" = "--preflight-report" ] || [ "$1" = "--preflight" ]
exit 0
"#,
    )
    .expect("write fake retained launcher");
    fs::write(&verifier, "#!/bin/sh\nexit 98\n").expect("write fake verifier");
    for executable in [&fake_cargo, &publisher, &launcher, &verifier] {
        let mut permissions = fs::metadata(executable)
            .expect("fake executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(executable, permissions).expect("fake executable permissions");
    }
    let publisher = fs::canonicalize(publisher).expect("canonical fake publisher");
    let verifier = fs::canonicalize(verifier).expect("canonical fake verifier");
    let bench_root = fs::canonicalize(bench_root).expect("canonical benchmark root");

    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").expect("test PATH")
    );
    let output = Command::new("bash")
        .arg(&script)
        .env("BM_W4_EXTERNAL_BENCH_ROOT", &bench_root)
        .env("BM_P7_RUN_ID", "retained-publisher-contract")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("BM_P7_LAUNCHER_LOG", &launcher_log)
        .env("BM_P7_DIRECT_PUBLISHER_MARKER", &direct_publisher_marker)
        .env("BM_P7_EXPECTED_PUBLISHER", &publisher)
        .env("BM_P7_FAKE_VERIFIER", &verifier)
        .env("PATH", path)
        .output()
        .expect("run operator script");

    assert!(output.status.success(), "{output:?}");
    assert!(
        !direct_publisher_marker.exists(),
        "publisher bypassed retained launcher"
    );
    let launches = fs::read_to_string(&launcher_log).expect("retained launcher log");
    let launches = launches.lines().collect::<Vec<_>>();
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert_eq!(
        launches[0],
        format!(
            "--executable {} -- --publish-verifier-release --benchmark-root {}",
            publisher.display(),
            bench_root.display()
        )
    );
    let results = bench_root.join("results/runs/retained-publisher-contract");
    assert_eq!(
        launches[1],
        format!(
            "--executable {} -- --preflight-report {} --summary {} --summary {} --summary {} --summary {}",
            verifier.display(),
            results.join("preflight-report.json").display(),
            results.join("locomo.merged.summary.json").display(),
            results.join("longmemeval_oracle.merged.summary.json").display(),
            results.join("longmemeval_s_cleaned.merged.summary.json").display(),
            results.join("longmemeval_m_cleaned.merged.summary.json").display()
        )
    );

    fs::write(&launcher_log, "").expect("reset retained launcher log");
    let preflight_output = Command::new("bash")
        .arg(&preflight_script)
        .env("BM_W4_EXTERNAL_BENCH_ROOT", &bench_root)
        .env("BM_P7_RUN_ID", "retained-publisher-contract")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("BM_P7_LAUNCHER_LOG", &launcher_log)
        .env("BM_P7_DIRECT_PUBLISHER_MARKER", &direct_publisher_marker)
        .env("BM_P7_EXPECTED_PUBLISHER", &publisher)
        .env("BM_P7_FAKE_VERIFIER", &verifier)
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_bin.display(),
                std::env::var("PATH").expect("test PATH")
            ),
        )
        .output()
        .expect("run preflight script");
    assert!(preflight_output.status.success(), "{preflight_output:?}");
    assert!(!direct_publisher_marker.exists());
    let preflight_launches = fs::read_to_string(&launcher_log).expect("preflight launcher log");
    let preflight_launches = preflight_launches.lines().collect::<Vec<_>>();
    assert_eq!(preflight_launches.len(), 2, "{preflight_launches:?}");
    assert_eq!(preflight_launches[0], launches[0]);
    assert_eq!(
        preflight_launches[1],
        format!(
            "--executable {} -- --preflight --benchmark-root {} --run-id retained-publisher-contract",
            verifier.display(),
            bench_root.display()
        )
    );

    let _ = fs::remove_dir_all(&root);
}

#[cfg(not(target_os = "linux"))]
#[test]
fn p7_direct_publisher_fails_before_external_write_without_execution_broker() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bm-p7-direct-publisher-fail-closed-{}-{nonce}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir(&root).expect("create direct publisher benchmark root");
    let root = fs::canonicalize(root).expect("canonical direct publisher benchmark root");
    let operator = fs::canonicalize(
        std::env::var("CARGO_BIN_EXE_bm-w4-external-noisy-wall").expect("operator binary path"),
    )
    .expect("canonical operator binary");

    let output = Command::new(&operator)
        .args(["--publish-verifier-release", "--benchmark-root"])
        .arg(&root)
        .env("BM_P7_RETAINED_EXECUTABLE_PATH", &operator)
        .env_remove("BM_P7_RETAINED_EXECUTABLE_FD")
        .env_remove("BM_P7_RETAINED_EXECUTABLE_SHA256")
        .output()
        .expect("run direct publisher without execution broker");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("execution authority"),
        "{output:?}"
    );
    assert!(
        !root.join("verifier").exists(),
        "failed authority admission must precede release store creation"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn p7_runner_authority_probe_verifier_recomputes_artifact_identities() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bm-p7-runner-authority-probe-fixture-{}-{nonce}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir(&root).expect("create authority probe fixture root");
    let root = fs::canonicalize(root).expect("canonical authority probe fixture root");

    let executable_bytes = b"sealed-runner-authority-probe";
    let executable_sha256 = p7_test_sha256(executable_bytes);
    let release_directory = root
        .join("runner/authority-probes")
        .join(&executable_sha256);
    fs::create_dir_all(&release_directory).expect("create authority probe release directory");
    let executable_path = release_directory.join(P7_AUTHORITY_PROBE_EXECUTABLE_NAME);
    fs::write(&executable_path, executable_bytes).expect("write authority probe executable");
    let executable_path =
        fs::canonicalize(executable_path).expect("canonical authority probe executable");

    let mut manifest_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": P7_AUTHORITY_PROBE_SCHEMA_VERSION,
        "executable_file_name": P7_AUTHORITY_PROBE_EXECUTABLE_NAME,
        "executable_sha256": executable_sha256,
        "executable_bytes": executable_bytes.len(),
    }))
    .expect("serialize authority probe manifest");
    manifest_bytes.push(b'\n');
    fs::write(
        release_directory.join(P7_AUTHORITY_PROBE_MANIFEST_NAME),
        &manifest_bytes,
    )
    .expect("write authority probe manifest");
    let report = serde_json::to_vec_pretty(&serde_json::json!({
        "canonical_path": executable_path,
        "executable_sha256": executable_sha256,
        "manifest_sha256": p7_test_sha256(&manifest_bytes),
        "publish_outcome": "published",
    }))
    .expect("serialize authority probe report");

    verify_p7_runner_authority_probe(&root, &report)
        .expect("independently verify authority probe fixture");
    fs::write(
        release_directory.join(P7_AUTHORITY_PROBE_EXECUTABLE_NAME),
        b"tampered-runner",
    )
    .expect("tamper authority probe executable");
    let error = verify_p7_runner_authority_probe(&root, &report)
        .expect_err("tampered executable must fail independent verification");
    assert!(error.contains("executable digest differs"), "{error}");

    let _ = fs::remove_dir_all(root);
}

#[test]
#[ignore = "requires the trusted Linux release-profile execution gate"]
fn p7_linux_real_runner_authority_probe_binds_sealed_execution_bytes() {
    let runner = fs::canonicalize(
        std::env::var_os("BM_P7_AUTHORITY_PROBE_RUNNER")
            .expect("BM_P7_AUTHORITY_PROBE_RUNNER must name the release runner"),
    )
    .expect("canonical release runner");
    let launcher = fs::canonicalize(
        std::env::var_os("BM_P7_AUTHORITY_PROBE_LAUNCHER")
            .expect("BM_P7_AUTHORITY_PROBE_LAUNCHER must name the retained launcher"),
    )
    .expect("canonical retained launcher");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bm-p7-real-runner-authority-probe-{}-{nonce}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir(&root).expect("create real runner authority probe root");
    let root = fs::canonicalize(root).expect("canonical real runner authority probe root");

    let result = (|| -> Result<(), String> {
        let direct = Command::new(&runner)
            .arg("--publish-release")
            .arg("--root")
            .arg(&root)
            .output()
            .map_err(|error| format!("failed to run direct pathname runner: {error}"))?;
        if direct.status.success() {
            return Err("direct pathname runner unexpectedly acquired execution authority".into());
        }
        if root.join("runner").exists() || root.join("results").exists() {
            return Err("direct pathname runner wrote artifacts before authority admission".into());
        }

        let sealed = Command::new(&launcher)
            .arg("--executable")
            .arg(&runner)
            .arg("--")
            .arg("--publish-authority-probe")
            .arg("--root")
            .arg(&root)
            .output()
            .map_err(|error| format!("failed to run sealed runner authority probe: {error}"))?;
        if !sealed.status.success() {
            return Err(format!(
                "sealed runner authority probe failed: status={:?} stderr={}",
                sealed.status.code(),
                String::from_utf8_lossy(&sealed.stderr)
            ));
        }
        verify_p7_runner_authority_probe(&root, &sealed.stdout)?;
        if root.join("runner/releases").exists() || root.join("results").exists() {
            return Err("authority probe polluted formal release or cohort artifacts".into());
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(root);
    result.expect("verify real sealed runner authority probe");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires the trusted Linux release-profile execution gate"]
fn p7_linux_real_publisher_and_verifier_use_sealed_execution_authority() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bm-p7-real-publisher-{}-{nonce}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir(&root).expect("create publisher benchmark root");
    let root = fs::canonicalize(root).expect("canonical publisher benchmark root");
    let publisher = fs::canonicalize(
        std::env::var("CARGO_BIN_EXE_bm-w4-external-noisy-wall").expect("operator binary path"),
    )
    .expect("canonical publisher");
    let launcher = fs::canonicalize(
        std::env::var("CARGO_BIN_EXE_bm-p7-retained-launch").expect("launcher binary path"),
    )
    .expect("canonical launcher");

    let first = Command::new(&launcher)
        .arg("--executable")
        .arg(&publisher)
        .arg("--")
        .arg("--publish-verifier-release")
        .arg("--benchmark-root")
        .arg(&root)
        .output()
        .expect("run sealed verifier publisher");
    assert!(first.status.success(), "{first:?}");
    let published = String::from_utf8(first.stdout)
        .expect("publisher stdout UTF-8")
        .trim()
        .to_string();
    let published = fs::canonicalize(published).expect("canonical published verifier");
    assert!(published.starts_with(root.join("verifier/releases")));
    let (identity, receipt) =
        bm_replay::verify_p7_verifier_release_manifest_with_receipt(&published)
            .expect("verify published release from retained disk handles");
    assert_eq!(identity.build_profile, "release");
    assert!(receipt.passed);

    let cohort = root.join("results/runs/verifier-smoke");
    fs::create_dir_all(&cohort).expect("create verifier smoke cohort");
    let summary = cohort.join("invalid.merged.summary.json");
    fs::write(&summary, b"{invalid\n").expect("write invalid summary after identity boundary");
    let verifier = Command::new(&launcher)
        .arg("--executable")
        .arg(&published)
        .arg("--")
        .arg("--preflight-report")
        .arg(cohort.join("preflight-report.json"))
        .arg("--summary")
        .arg(&summary)
        .output()
        .expect("run sealed production verifier mode");
    assert_eq!(verifier.status.code(), Some(2), "{verifier:?}");
    let stderr = String::from_utf8(verifier.stderr).expect("verifier stderr UTF-8");
    assert!(stderr.contains("p7_wall_parse_merged_summary"), "{stderr}");
    assert!(!stderr.contains("execution authority failed"), "{stderr}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn p7_operator_and_runner_scripts_preserve_immutable_cohort_artifacts() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let operator =
        fs::read_to_string(repo_root.join("scripts/check_w4_external_noisy_wall_operator.sh"))
            .expect("operator script");
    let operator_cli =
        fs::read_to_string(repo_root.join("crates/replay/src/bin/bm-w4-external-noisy-wall.rs"))
            .expect("operator CLI");
    let replay_bench = fs::read_to_string(repo_root.join("crates/replay/src/bench.rs"))
        .expect("replay benchmark verifier");
    let replay_lib = fs::read_to_string(repo_root.join("crates/replay/src/lib.rs"))
        .expect("replay public contract");
    let secure_fs = fs::read_to_string(repo_root.join("crates/replay/src/p7_secure_fs.rs"))
        .expect("retained cohort filesystem owner");
    let sealed_execution =
        fs::read_to_string(repo_root.join("crates/replay/src/sealed_execution.rs"))
            .expect("generation-neutral sealed execution owner");
    let frozen_identity_owner = fs::read_to_string(
        repo_root
            .join("crates/replay/src/bin/bm-w4-external-noisy-wall/p7_frozen_runner_identity.rs"),
    )
    .expect("operator-private frozen runner identity owner");
    let runner_wall = fs::read_to_string(
        repo_root
            .parent()
            .expect("hardware root")
            .join(".beetle-memory-external-bench/runner/run_full_p7_wall.sh"),
    )
    .expect("external runner wall script");
    let runner_rss = fs::read_to_string(
        repo_root
            .parent()
            .expect("hardware root")
            .join(".beetle-memory-external-bench/runner/run_p7_max_rss.sh"),
    )
    .expect("external runner maximum RSS script");
    let runner_main = fs::read_to_string(
        repo_root
            .parent()
            .expect("hardware root")
            .join(".beetle-memory-external-bench/runner/src/main.rs"),
    )
    .expect("external runner release contract");
    let linux_execution_authority =
        fs::read_to_string(repo_root.join("scripts/check_p7_linux_execution_authority.sh"))
            .expect("Linux execution authority gate");
    let recursive_gate_scripts = [
        "scripts/check_next_gen_memory_plan.sh",
        "scripts/check_memory_benchmark_wall.sh",
        "scripts/check_memory_write_transaction_contract.sh",
        "scripts/check_sdk_host_integration_readiness.sh",
    ]
    .map(|relative| {
        fs::read_to_string(repo_root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"))
    });

    assert!(!operator.contains("BM_W4_EXTERNAL_REPORT_PATH"));
    assert!(!operator.contains("operator-report.json.tmp-"));
    assert!(!operator.contains("mv -n"));
    assert!(operator_cli
        .contains("cohort_dir.join(format!(\"operator-report-{verifier_digest}.json\"))"));
    assert!(operator_cli.contains("verify_p7_wall_input_context"));
    assert!(!operator_cli.contains("fs::read_to_string(&path)"));
    assert!(operator_cli.contains("publish_immutable_report"));
    assert!(operator_cli.contains("run_p7_soul_regression_gate(sdk_root)"));
    assert!(!operator_cli.contains("release_attested_soul_gate"));
    assert!(!operator_cli.contains("inspect_contract_passed: true"));
    assert!(operator_cli.matches(".open_cohort(").count() >= 4);
    assert!(!operator_cli.contains("open_p7_cohort_artifact_owner"));
    assert!(operator_cli.contains("publish_immutable_bytes"));
    assert!(!operator_cli.contains("fs::hard_link"));
    assert!(!operator_cli.contains("OpenOptions::new()"));
    let wall_verifier = replay_bench
        .split("pub fn verify_p7_wall_input_context_with_authority")
        .nth(1)
        .and_then(|source| {
            source
                .split("fn validate_p7_release_identity_against_disk")
                .next()
        })
        .expect("final wall verifier body");
    assert!(wall_verifier.contains("verify_p7_wall_cohort_evidence_in_session"));
    assert!(runner_wall.contains("BM_P7_LAUNCHER_BIN"));
    assert!(runner_wall.contains("--executable \"$orchestrator\" --"));
    assert!(runner_rss.contains("BM_P7_LAUNCHER_BIN"));
    assert!(runner_rss.contains("--executable \"$orchestrator\" --"));
    let cohort_verifier = replay_bench
        .split("fn verify_p7_wall_cohort_evidence_in_session")
        .nth(1)
        .and_then(|source| source.split("pub fn verify_p7_cohort_admission").next())
        .expect("cohort evidence verifier body");
    assert!(cohort_verifier.contains("verify_p7_maximum_rss_evidence_in_session"));
    for required in [
        "p7_verifier_performance_v2",
        "unique_artifact_count",
        "full_read_pass_count",
        "admitted_artifact_bytes",
        "artifact_bytes_read",
        "detail_artifact_bytes_read",
        "duplicate_artifact_count",
        "P7_VERIFIER_MAX_GLOBAL_ARTIFACT_BYTES",
        "checked_add",
        "verify_retained_handle_unchanged",
    ] {
        assert!(
            replay_bench.contains(required),
            "missing verifier single-read performance contract: {required}"
        );
    }
    assert!(!replay_lib.contains("verify_w4_external_noisy_summary_files"));
    assert!(replay_lib.contains("verify_p7_wall_cohort_evidence_with_receipt"));
    assert!(!replay_lib.contains("verify_p7_wall_cohort_evidence,"));
    for required in [
        "p7_maximum_rss_evidence_v3",
        "p7_maximum_rss_measurement_v3",
        "maximum-rss-measurement.json",
        "child_exit_status",
        "measurement_elapsed_millis",
        "supervisor_receipt",
        "runner_stderr_sha256",
    ] {
        assert!(
            replay_bench.contains(required),
            "missing replay-owned maximum RSS artifact contract: {required}"
        );
    }
    assert!(replay_lib.contains("P7MaximumRssMeasurementReport"));
    assert!(!replay_lib.contains("mod p7_frozen_runner_identity;"));
    assert!(replay_lib.contains("P7_FROZEN_RUNNER_IDENTITY_RELATIVE_PATH"));
    assert!(replay_bench
        .contains("crates/replay/src/bin/bm-w4-external-noisy-wall/p7_frozen_runner_identity.rs"));
    assert!(!replay_bench.contains("const P7_FROZEN_RUNNER_IDENTITY:"));
    assert!(frozen_identity_owner.contains("const P7_FROZEN_RUNNER_IDENTITY:"));
    assert!(!frozen_identity_owner.contains("fn "));
    assert!(!frozen_identity_owner.contains("validate"));
    assert!(replay_bench.contains("P7_AGENT_MEMORY_RELEASE_GATE_EXCLUDED_DIRECTORIES"));
    assert!(replay_bench.contains("P7_RUNNER_RELEASE_GATE_EXCLUDED_DIRECTORIES"));
    assert!(runner_main.contains("bm_replay::p7_release_gate_source_manifest"));
    assert!(runner_main.contains("P7ProcessExecutionAuthority::claim()"));
    assert!(runner_main.contains("run_with_execution_authority"));
    assert!(runner_main.contains("execution.verify_retained()?"));
    assert!(runner_main.contains("AuthorityBoundArtifactTransaction"));
    assert!(runner_main.contains("AuthorityBoundReleaseTransaction"));
    assert!(runner_main.contains("--publish-authority-probe"));
    assert!(runner_main.contains("p7_runner_authority_probe_v1"));
    assert!(linux_execution_authority
        .contains("p7_linux_real_runner_authority_probe_binds_sealed_execution_bytes"));
    assert!(linux_execution_authority.contains("BM_P7_AUTHORITY_PROBE_RUNNER"));
    assert!(linux_execution_authority.contains("BM_P7_AUTHORITY_PROBE_LAUNCHER"));
    assert!(linux_execution_authority
        .contains("cargo build --release --locked --no-default-features --manifest-path"));
    for source in recursive_gate_scripts {
        assert!(source.contains("local has_locked=0"));
        assert!(source.contains("local has_no_default_features=0"));
        assert!(
            source.contains("[[ \"$has_locked\" -eq 1 && \"$has_no_default_features\" -eq 1 ]]")
        );
        assert!(source.contains("command cargo \"$subcommand\" --no-default-features \"$@\""));
        assert!(source.contains("command cargo \"$subcommand\" --locked \"$@\""));
        assert!(
            source.contains("command cargo \"$subcommand\" --locked --no-default-features \"$@\"")
        );
    }
    for forbidden in ["node ", "sha256sum", "awk ", "mktemp", "realpath"] {
        assert!(
            !linux_execution_authority.contains(forbidden),
            "Linux execution authority gate uses unattested parser or digest tool: {forbidden}"
        );
    }
    assert!(secure_fs.contains("ClaimedSealedExecution::claim_typed(P7_SEALED_EXECUTION_DOMAIN)"));
    assert!(!secure_fs.contains("Mutex<P7ExecutionAuthorityClaimState>"));
    assert!(sealed_execution.contains("static CLAIMED_DOMAINS"));
    assert!(sealed_execution.contains("F_DUPFD_CLOEXEC"));
    assert!(sealed_execution.contains("F_GET_SEALS"));
    assert!(secure_fs.contains("open_or_create_p7_runner_authority_probe_store_with_authority"));
    assert!(secure_fs.contains("pub(crate) struct P7CohortArtifactOwner"));
    assert!(!replay_lib.contains("P7CohortArtifactOwner,"));
    for forbidden in [
        "BM_P7_RETAINED_EXECUTABLE_FD",
        "BM_P7_RETAINED_EXECUTABLE_PATH",
        "BM_P7_RETAINED_EXECUTABLE_SHA256",
        "BM_P7_RETAINED_EXECUTABLE_HELD",
        "env::current_exe",
    ] {
        assert!(
            !runner_main.contains(forbidden),
            "external runner bypasses the opaque execution authority: {forbidden}"
        );
    }
    assert!(runner_main
        .contains("crates/replay/src/bin/bm-w4-external-noisy-wall/p7_frozen_runner_identity.rs"));
    assert!(replay_bench.contains("p7_release_gate_attestation_v2"));
    assert!(replay_bench.contains("p7_content_addressed_release_metadata_v2"));
    assert!(replay_bench.contains("p7_release_gate_source_sha256_v3"));
    for required in ["openat", "linkat", "O_NOFOLLOW", "publish_immutable_bytes"] {
        assert!(
            secure_fs.contains(required),
            "retained cohort publisher is missing {required}"
        );
    }
    assert!(!operator_cli.contains("fs::write(&report_path"));
    assert!(!runner_wall.contains("--overwrite"));
    assert!(!runner_wall.contains("BM_W4_EXTERNAL_REPORT_PATH"));
    assert!(operator.contains("BM_P7_VERIFY_MAX_RSS"));
    assert!(operator.contains("--verify-max-rss"));
    assert!(runner_rss.contains("BM_P7_ORCHESTRATOR_BIN"));
    assert!(runner_rss.contains("--orchestrate-max-rss"));
    assert!(runner_wall.contains("BM_P7_ORCHESTRATOR_BIN"));
    assert!(runner_wall.contains("--orchestrate-full-wall"));
    assert!(!runner_rss.contains("cargo "));
    assert!(!runner_wall.contains("cargo "));
    assert!(runner_rss.contains("BM_P7_RUN_ID must match ASCII [A-Za-z0-9._-]+"));
    assert!(!runner_rss.contains("maximum-rss.report"));
    assert!(!runner_rss.contains("/usr/bin/plutil"));
}

#[test]
fn w4_external_noisy_wall_reports_shards_bps_and_missing_provenance() {
    let mut locomo = external_summary("locomo", 10, 1986, 1982, 21, 13);
    locomo.shards = expected_external_shards("locomo");
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
    assert!(report.shards_valid);
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
    let mut summary = W4ExternalNoisyBenchmarkSummary::default();
    summary.suite = suite.to_string();
    summary.completed = true;
    summary.shards = expected_external_shards(suite);
    summary.samples = samples;
    summary.questions = questions;
    summary.evidence_questions = evidence_questions;
    summary.no_gold_questions = questions.saturating_sub(evidence_questions);
    summary.any_evidence_hit = any_evidence_hit;
    summary.all_evidence_hit = all_evidence_hit;
    summary
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
    let mut summary = external_summary(
        suite,
        samples,
        questions,
        evidence_questions,
        any_evidence_hit,
        all_evidence_hit,
    );
    summary.summary_sha256 =
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
    summary.runner_source_sha256 =
        Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string());
    summary.stage_hit_counts = Some(W4ExternalNoisyStageHitCounts {
        source_any_evidence_hit: source_any,
        source_all_evidence_hit: source_all,
        expanded_any_evidence_hit: expanded_any,
        expanded_all_evidence_hit: expanded_all,
        reranked_any_evidence_hit: expanded_any,
        reranked_all_evidence_hit: expanded_all,
        selected_any_evidence_hit: selected_any,
        selected_all_evidence_hit: selected_all,
        projection_selected_any_evidence_hit: selected_any,
        projection_selected_all_evidence_hit: selected_all,
        rendered_any_evidence_hit: selected_any,
        rendered_all_evidence_hit: selected_all,
    });
    summary.index_diagnostics = Some(W4ExternalNoisyIndexDiagnostics {
        questions_with_index_report,
        index_used_questions,
        fallback_full_scan_questions,
        source_candidate_count: questions_with_index_report,
        matched_source_anchor_count: index_used_questions,
        unmatched_source_anchor_count: fallback_full_scan_questions,
        indexed_neighbor_count,
        filtered_node_count: indexed_neighbor_count,
        filtered_edge_count: indexed_neighbor_count,
        filtered_backlink_count: indexed_neighbor_count,
        failure_count,
        graph_manifest_contract_verified_questions: index_used_questions,
        graph_selected_dependency_chain_verified_questions: index_used_questions,
        graph_full_scope_closure_verified_questions: 0,
        graph_manifest_generation_present_questions: index_used_questions,
        graph_revision_present_questions: index_used_questions,
        graph_scope_digest_present_questions: index_used_questions,
        graph_maintenance_required_questions: 0,
        graph_incident_questions: 0,
        graph_read_path_mutation_delta: 0,
        facet_questions_with_index_report: questions_with_index_report,
        facet_index_used_questions: questions_with_index_report,
        facet_report_only_questions: 0,
        facet_fallback_full_scan_questions: 0,
        facet_source_candidate_count: questions_with_index_report,
        facet_matched_source_candidate_count: questions_with_index_report,
        facet_posting_key_lookup_count: questions_with_index_report,
        facet_manifest_matched_posting_count: questions_with_index_report,
        facet_posting_doc_read_count: questions_with_index_report,
        facet_owner_key_lookup_count: questions_with_index_report,
        facet_owner_doc_read_count: questions_with_index_report,
        facet_zero_posting_key_lookup_questions: 0,
        facet_clean_zero_hit_questions: 0,
        facet_manifest_integrity_verified_questions: questions_with_index_report,
        facet_manifest_integrity_failure_count: 0,
        facet_exact_match_count: questions_with_index_report,
        facet_expanded_match_count: questions_with_index_report,
        facet_failure_count: 0,
    });
    summary
}

fn expected_external_shards(suite: &str) -> Vec<String> {
    let shard_total = match suite {
        "locomo" => 10,
        "longmemeval_m_cleaned" => 8,
        _ => 1,
    };
    (0..shard_total)
        .map(|index| format!("{suite}.shard-{index}-of-{shard_total}.summary.json"))
        .collect()
}

fn attach_w41_diagnostics(
    mut summary: W4ExternalNoisyBenchmarkSummary,
) -> W4ExternalNoisyBenchmarkSummary {
    summary.w4_1_diagnostics = Some(bm_replay::W4ExternalNoisyW41Diagnostics {
        questions_with_w4_1_diagnostics: summary.questions,
        first_any_hit_stage_counts: [("expanded".to_string(), summary.questions)]
            .into_iter()
            .collect(),
        first_all_hit_stage_counts: [("selected".to_string(), summary.all_evidence_hit)]
            .into_iter()
            .collect(),
        missing_gold_by_stage_counts: [(
            "source".to_string(),
            summary
                .evidence_questions
                .saturating_sub(summary.any_evidence_hit),
        )]
        .into_iter()
        .collect(),
        miss_after_expanded_count: summary
            .evidence_questions
            .saturating_sub(summary.any_evidence_hit),
        gold_rank_found_count: summary.any_evidence_hit,
        gold_rank_missing_count: summary
            .evidence_questions
            .saturating_sub(summary.any_evidence_hit),
        gold_rank_sum: summary.any_evidence_hit,
        truncated_count: 0,
        blocked_reason_counts: Default::default(),
        question_type_counts: [("external_noisy".to_string(), summary.questions)]
            .into_iter()
            .collect(),
        evidence_count_buckets: [("1".to_string(), summary.questions)].into_iter().collect(),
        source_signature_count: summary.questions.max(1),
        repeated_source_signature_questions: 0,
    });
    summary
}

fn attach_facet_ablation(
    mut summary: W4ExternalNoisyBenchmarkSummary,
    render_growth: usize,
) -> W4ExternalNoisyBenchmarkSummary {
    let applicable_questions = summary.evidence_questions;
    summary.facet_ablation = Some(bm_replay::W4ExternalNoisyFacetAblationDiagnostics {
        questions_with_ablation_report: applicable_questions,
        method_counts: [(
            "sdk_eval_recall_off_run_v1".to_string(),
            applicable_questions,
        )]
        .into_iter()
        .collect(),
        delivery_contribution_proven_questions: applicable_questions,
        render_growth,
        required_slice_counts: [
            ("facet_off".to_string(), applicable_questions),
            ("rank_fusion_off".to_string(), applicable_questions),
            ("coverage_selection_off".to_string(), applicable_questions),
            (
                "delivery_relevance_fusion_off".to_string(),
                applicable_questions,
            ),
            (
                "evidence_family_rotation_off".to_string(),
                applicable_questions,
            ),
            ("render_capsule_off".to_string(), applicable_questions),
            ("capsule_dedupe_off".to_string(), applicable_questions),
        ]
        .into_iter()
        .collect(),
        report_available_slice_counts: [
            ("facet_off".to_string(), applicable_questions),
            ("rank_fusion_off".to_string(), applicable_questions),
            ("coverage_selection_off".to_string(), applicable_questions),
            (
                "delivery_relevance_fusion_off".to_string(),
                applicable_questions,
            ),
            (
                "evidence_family_rotation_off".to_string(),
                applicable_questions,
            ),
            ("render_capsule_off".to_string(), applicable_questions),
            ("capsule_dedupe_off".to_string(), applicable_questions),
        ]
        .into_iter()
        .collect(),
        delivery_contribution_proven_slice_counts: [
            ("facet_off".to_string(), applicable_questions),
            ("rank_fusion_off".to_string(), applicable_questions),
            ("coverage_selection_off".to_string(), applicable_questions),
            (
                "delivery_relevance_fusion_off".to_string(),
                applicable_questions,
            ),
            (
                "evidence_family_rotation_off".to_string(),
                applicable_questions,
            ),
            ("render_capsule_off".to_string(), applicable_questions),
            ("capsule_dedupe_off".to_string(), applicable_questions),
        ]
        .into_iter()
        .collect(),
        delivery_affected_candidate_occurrences: summary.any_evidence_hit,
        selected_evidence_hit_delta: Default::default(),
        rendered_evidence_hit_delta: Default::default(),
        selected_all_hit_loss_count: Default::default(),
        evidence_family_rotation_selected_all_hit_loss_count: Default::default(),
        rendered_all_hit_loss_count: Default::default(),
        expanded_candidate_delta: Default::default(),
        selected_candidate_delta: Default::default(),
        rendered_candidate_delta: Default::default(),
        rendered_char_delta: Default::default(),
        blocked_reason_counts: Default::default(),
    });
    summary
}

fn attach_p7_release_evidence(
    mut summary: W4ExternalNoisyBenchmarkSummary,
) -> W4ExternalNoisyBenchmarkSummary {
    summary.run_id = "test-run".to_string();
    let questions = summary.questions;
    let ablation = summary
        .facet_ablation
        .as_mut()
        .expect("P7 release evidence requires ablation");
    ablation
        .selected_evidence_hit_delta
        .insert("delivery_relevance_fusion_off".to_string(), 1);
    ablation
        .rendered_evidence_hit_delta
        .insert("render_capsule_off".to_string(), 1);
    ablation
        .rendered_char_delta
        .insert("render_capsule_off".to_string(), 128);
    summary.p7_loss_ledger = Some(bm_replay::W4ExternalNoisyP7LossDiagnostics {
        questions_with_loss_ledger: questions,
        expanded_hit_selected_miss_questions: 0,
        eval_selected_hit_rendered_miss_questions: 0,
        expanded_hit_selected_miss_evidence: 0,
        eval_selected_hit_rendered_miss_evidence: 0,
        eval_selected_hit_projection_selected_miss_questions: 0,
        eval_selected_hit_projection_selected_miss_evidence: 0,
        selected_hit_final_rendered_miss_questions: 0,
        selected_hit_final_rendered_miss_evidence: 0,
        eval_truncated_count: 0,
        eval_blocked_reason_counts: Default::default(),
    });
    summary.p7_production_delivery =
        Some(bm_replay::W4ExternalNoisyP7ProductionDeliveryDiagnostics {
            questions_with_delivery_report: questions,
            eval_selected_matches_delivery_questions: questions,
            eval_rendered_matches_delivery_questions: questions,
            projection_selected_sources_proven_questions: questions,
            projection_delivery_proof_questions: questions,
            final_projection_integrity_questions: questions,
            final_projection_integrity_passed_questions: questions,
            final_projection_raw_private_violation_count: 0,
            final_projection_blocked_source_count: 0,
            final_projection_redacted_source_count: 0,
            schema_version_counts: [(
                bm_sdk::MEMORY_RECALL_DELIVERY_SCHEMA_VERSION.to_string(),
                questions,
            )]
            .into_iter()
            .collect(),
            render_growth: 0,
            privacy_leak_count: 0,
            cross_subject_leak_count: 0,
            raw_soul_private_material_count: 0,
            blocked_reason_counts: Default::default(),
            delivery_drop_reason_counts: Default::default(),
        });
    summary.summary_sha256 = Some("a".repeat(64));
    summary.runner_source_sha256 = Some("b".repeat(64));
    let producer_identity = bm_replay::P7ProducerIdentity {
        schema_version: "p7_producer_identity_v2".to_string(),
        contract_version: "p7_recall_delivery_v2".to_string(),
        sdk_report_schema_version: bm_sdk::MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
        sdk_build_fingerprint: "c".repeat(64),
        runner_build_fingerprint: "b".repeat(64),
        runner_lock_fingerprint: "9".repeat(64),
        executable_sha256: "8".repeat(64),
        build_profile: "release".to_string(),
        input_sha256: "7".repeat(64),
        detail_schema_version: "p7_question_detail_v1".to_string(),
    };
    summary.p7_provenance = Some(bm_replay::P7MergedProvenance {
        schema_version: bm_replay::P7_MERGED_PROVENANCE_SCHEMA_VERSION.to_string(),
        run_id: summary.run_id.clone(),
        contract_version: "p7_recall_delivery_v2".to_string(),
        sdk_report_schema_version: bm_sdk::MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
        sdk_build_fingerprint: "c".repeat(64),
        runner_build_fingerprint: "b".repeat(64),
        runner_lock_fingerprint: "9".repeat(64),
        executable_sha256: "8".repeat(64),
        gate_attestation_sha256: "6".repeat(64),
        release_metadata_sha256: "4".repeat(64),
        gate_source_fingerprint: "5".repeat(64),
        gate_source_manifest_sha256: "3".repeat(64),
        gate_ids: bm_replay::P7_REQUIRED_RELEASE_GATE_IDS
            .map(str::to_string)
            .to_vec(),
        build_profile: "release".to_string(),
        cohort_admission_sha256: "2".repeat(64),
        input_sha256: "7".repeat(64),
        detail_schema_version: "p7_question_detail_v1".to_string(),
        producer_identity: bm_replay::P7RecordedProducerIdentity::record(&producer_identity)
            .expect("record producer identity"),
        merged_detail_sha256: "d".repeat(64),
        ordered_shard_digest_manifest: summary
            .shards
            .iter()
            .map(|shard| bm_replay::W4ExternalNoisyP7ShardDigest {
                run_id: summary.run_id.clone(),
                shard: shard.clone(),
                summary_sha256: "e".repeat(64),
                detail_sha256: "f".repeat(64),
            })
            .collect(),
    });
    summary
}

fn fixture_recorded_producer_identity() -> bm_replay::P7RecordedProducerIdentity {
    bm_replay::P7RecordedProducerIdentity::record(&bm_replay::P7ProducerIdentity {
        schema_version: "p7_producer_identity_v2".to_string(),
        contract_version: "p7_recall_delivery_v2".to_string(),
        sdk_report_schema_version: bm_sdk::MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
        sdk_build_fingerprint: "a".repeat(64),
        runner_build_fingerprint: "b".repeat(64),
        runner_lock_fingerprint: "c".repeat(64),
        executable_sha256: "d".repeat(64),
        build_profile: "release".to_string(),
        input_sha256: "7".repeat(64),
        detail_schema_version: "p7_question_detail_v1".to_string(),
    })
    .expect("record fixture producer identity")
}

fn fixture_root() -> String {
    format!(
        "{}/../../fixtures/memory-benchmark-wall",
        env!("CARGO_MANIFEST_DIR")
    )
}
