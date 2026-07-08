use bm_core::memory::{
    run_persona_governance_replay_suite, run_recall_benchmark_suite, PersonaGovernanceReplayCase,
    RecallBenchmarkCase,
};
use bm_core::{Error, Result};
use bm_sdk::ProfileId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BenchmarkGateReport {
    pub suite: String,
    pub cases: usize,
    pub passed_cases: usize,
    pub failed_cases: Vec<String>,
    pub passed: bool,
}

pub fn run_recall_benchmark_gate(cases: &[RecallBenchmarkCase]) -> BenchmarkGateReport {
    let results = run_recall_benchmark_suite(cases);
    let failed_cases = results
        .iter()
        .filter(|result| !result.passed)
        .map(|result| result.case_name.to_string())
        .collect::<Vec<_>>();
    BenchmarkGateReport {
        suite: "recall".to_string(),
        cases: results.len(),
        passed_cases: results.len().saturating_sub(failed_cases.len()),
        passed: failed_cases.is_empty(),
        failed_cases,
    }
}

pub fn run_persona_governance_benchmark_gate(
    cases: &[PersonaGovernanceReplayCase],
) -> BenchmarkGateReport {
    let results = run_persona_governance_replay_suite(cases);
    let failed_cases = results
        .iter()
        .filter(|result| !result.passed)
        .map(|result| result.case_name.to_string())
        .collect::<Vec<_>>();
    BenchmarkGateReport {
        suite: "persona_governance".to_string(),
        cases: results.len(),
        passed_cases: results.len().saturating_sub(failed_cases.len()),
        passed: failed_cases.is_empty(),
        failed_cases,
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkClass {
    RecallMultisession,
    TemporalUpdate,
    SubjectProjection,
    SoulRegression,
    ProceduralReuse,
    PrivacyRefusal,
    AgentToolExperience,
}

impl MemoryBenchmarkClass {
    pub const ALL: [Self; 7] = [
        Self::RecallMultisession,
        Self::TemporalUpdate,
        Self::SubjectProjection,
        Self::SoulRegression,
        Self::ProceduralReuse,
        Self::PrivacyRefusal,
        Self::AgentToolExperience,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RecallMultisession => "recall_multisession",
            Self::TemporalUpdate => "temporal_update",
            Self::SubjectProjection => "subject_projection",
            Self::SoulRegression => "soul_regression",
            Self::ProceduralReuse => "procedural_reuse",
            Self::PrivacyRefusal => "privacy_refusal",
            Self::AgentToolExperience => "agent_tool_experience",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkMode {
    Compact,
    Full,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkFixture {
    #[serde(default)]
    pub schema: String,
    pub fixture_id: String,
    pub class: MemoryBenchmarkClass,
    pub profile: ProfileId,
    pub mode: MemoryBenchmarkMode,
    #[serde(default)]
    pub evaluation_source: MemoryBenchmarkEvaluationSource,
    pub description: String,
    #[serde(default)]
    pub scenario: MemoryBenchmarkScenario,
    #[serde(default)]
    pub semantic_contract: MemoryBenchmarkSemanticContract,
    pub metrics: MemoryBenchmarkMetrics,
    pub thresholds: MemoryBenchmarkThresholds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_recall: Option<MemoryBenchmarkEvalRecall>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkEvaluationSource {
    #[default]
    ContractBaseline,
    RuntimeReplay,
    GoldenJudge,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkScenario {
    #[serde(default)]
    pub user_goal: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub expected_surfaces: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecall {
    #[serde(default)]
    pub suite: String,
    #[serde(default)]
    pub split: String,
    #[serde(default)]
    pub question_id: String,
    #[serde(default)]
    pub question_type: String,
    #[serde(default)]
    pub expected_evidence_refs: Vec<String>,
    #[serde(default)]
    pub source_candidates: Vec<String>,
    #[serde(default)]
    pub graph_anchor_candidates: Vec<String>,
    #[serde(default)]
    pub expanded_candidates: Vec<String>,
    #[serde(default)]
    pub eval_candidate_pool: Vec<String>,
    #[serde(default)]
    pub selected_candidates: Vec<String>,
    #[serde(default)]
    pub rendered_candidates: Vec<String>,
    #[serde(default)]
    pub rendered_block_preview: String,
    #[serde(default)]
    pub rendered_evidence_refs: Vec<String>,
    #[serde(default)]
    pub evidence_ref_index: Vec<MemoryBenchmarkEvalRecallEvidenceRefIndexEntry>,
    #[serde(default)]
    pub missing_evidence_refs: Vec<String>,
    #[serde(default)]
    pub diagnostics: MemoryBenchmarkEvalRecallDiagnostics,
    #[serde(default)]
    pub metrics: MemoryBenchmarkEvalRecallMetrics,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallEvidenceRefIndexEntry {
    #[serde(default)]
    pub candidate_id: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallStageEvidenceRefs {
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallGoldRank {
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub evidence_ref: String,
    #[serde(default)]
    pub rank: Option<usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallGraphDistanceToGold {
    #[serde(default)]
    pub candidate_id: String,
    #[serde(default)]
    pub evidence_ref: String,
    #[serde(default)]
    pub distance: Option<u8>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallDiagnostics {
    #[serde(default)]
    pub evidence_count: usize,
    #[serde(default)]
    pub first_any_hit_stage: String,
    #[serde(default)]
    pub first_all_hit_stage: String,
    #[serde(default)]
    pub matched_gold_by_stage: Vec<MemoryBenchmarkEvalRecallStageEvidenceRefs>,
    #[serde(default)]
    pub missing_gold_by_stage: Vec<MemoryBenchmarkEvalRecallStageEvidenceRefs>,
    #[serde(default)]
    pub gold_rank_by_stage: Vec<MemoryBenchmarkEvalRecallGoldRank>,
    #[serde(default)]
    pub miss_after_expanded: bool,
    #[serde(default)]
    pub source_anchor_ids: Vec<String>,
    #[serde(default)]
    pub graph_anchor_candidate_ids: Vec<String>,
    #[serde(default)]
    pub expanded_node_ids: Vec<String>,
    #[serde(default)]
    pub graph_neighbor_ids: Vec<String>,
    #[serde(default)]
    pub graph_distance_to_gold: Vec<MemoryBenchmarkEvalRecallGraphDistanceToGold>,
    #[serde(default)]
    pub truncated_count: usize,
    #[serde(default)]
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallMetrics {
    #[serde(default)]
    pub recall_at_k: Vec<MemoryBenchmarkEvalRecallAtK>,
    #[serde(default)]
    pub mrr_bps: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkEvalRecallAtK {
    pub k: usize,
    pub any_evidence_hit: bool,
    pub all_evidence_hit: bool,
    #[serde(default)]
    pub matched_evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkMetrics {
    pub accuracy_bps: u16,
    pub evidence_precision_bps: u16,
    pub projection_faithfulness_bps: u16,
    pub privacy_violation_count: u32,
    pub stale_memory_false_positive_count: u32,
    pub procedural_reuse_success_bps: u16,
    pub soul_regression_count: u32,
    pub latency_ms: u32,
    pub token_budget: u32,
    pub memory_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkThresholds {
    #[serde(default)]
    pub min_accuracy_bps: u16,
    #[serde(default)]
    pub min_evidence_precision_bps: u16,
    #[serde(default)]
    pub min_projection_faithfulness_bps: u16,
    #[serde(default)]
    pub max_privacy_violation_count: u32,
    #[serde(default)]
    pub max_stale_memory_false_positive_count: u32,
    #[serde(default)]
    pub min_procedural_reuse_success_bps: u16,
    #[serde(default)]
    pub max_soul_regression_count: u32,
    #[serde(default)]
    pub max_latency_ms: Option<u32>,
    #[serde(default)]
    pub max_token_budget: Option<u32>,
    #[serde(default)]
    pub max_memory_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBenchmarkSemanticDimension {
    ProjectionShape,
    PrivacyRuntimeSemantics,
    SoulLifeSemantics,
    WorkIntegritySemantics,
    AgentToolExperienceSemantics,
    W4EvalRecallSemantics,
}

impl MemoryBenchmarkSemanticDimension {
    pub const ALL: [Self; 6] = [
        Self::ProjectionShape,
        Self::PrivacyRuntimeSemantics,
        Self::SoulLifeSemantics,
        Self::WorkIntegritySemantics,
        Self::AgentToolExperienceSemantics,
        Self::W4EvalRecallSemantics,
    ];
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkSemanticContract {
    #[serde(default)]
    pub dimensions: Vec<MemoryBenchmarkSemanticDimension>,
    #[serde(default)]
    pub provided_keys: Vec<String>,
    #[serde(default)]
    pub required_keys: Vec<String>,
    #[serde(default)]
    pub forbidden_keys: Vec<String>,
    #[serde(default)]
    pub observed_markers: Vec<String>,
    #[serde(default)]
    pub required_markers: Vec<String>,
    #[serde(default)]
    pub forbidden_markers: Vec<String>,
}

impl MemoryBenchmarkSemanticContract {
    fn is_empty(&self) -> bool {
        self.dimensions.is_empty()
            && self.provided_keys.is_empty()
            && self.required_keys.is_empty()
            && self.forbidden_keys.is_empty()
            && self.observed_markers.is_empty()
            && self.required_markers.is_empty()
            && self.forbidden_markers.is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkBaseline {
    pub accuracy_bps: u16,
    pub evidence_precision_bps: u16,
    pub projection_faithfulness_bps: u16,
    pub privacy_violation_count: u32,
    pub stale_memory_false_positive_count: u32,
    pub procedural_reuse_success_bps: u16,
    pub soul_regression_count: u32,
    pub latency_ms: u32,
    pub token_budget: u32,
    pub memory_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkClassCoverage {
    pub class: MemoryBenchmarkClass,
    pub compact_fixtures: usize,
    pub full_fixtures: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkMissingClass {
    pub class: MemoryBenchmarkClass,
    pub mode: MemoryBenchmarkMode,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkSemanticCoverage {
    pub dimension: MemoryBenchmarkSemanticDimension,
    pub fixture_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkFailure {
    pub fixture_id: String,
    pub class: MemoryBenchmarkClass,
    pub mode: MemoryBenchmarkMode,
    pub profile: ProfileId,
    pub stage: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkSemanticFailure {
    pub fixture_id: String,
    pub dimension: Option<MemoryBenchmarkSemanticDimension>,
    pub stage: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBenchmarkReport {
    pub suite: String,
    pub total_fixtures: usize,
    pub passed_fixtures: usize,
    pub baseline: MemoryBenchmarkBaseline,
    pub class_coverage: Vec<MemoryBenchmarkClassCoverage>,
    pub missing_classes: Vec<MemoryBenchmarkMissingClass>,
    pub semantic_coverage: Vec<MemoryBenchmarkSemanticCoverage>,
    pub soul_kernel_judge: SoulKernelBenchmarkJudgeReport,
    pub subject_projection_judge: SubjectProjectionBenchmarkJudgeReport,
    pub agent_tool_experience_judge: AgentToolExperienceBenchmarkJudgeReport,
    pub w4_eval_recall_judge: W4EvalRecallBenchmarkJudgeReport,
    pub failures: Vec<MemoryBenchmarkFailure>,
    pub semantic_failures: Vec<MemoryBenchmarkSemanticFailure>,
    pub passed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulKernelBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub growth_proposal_contract_covered: bool,
    pub regression_suite_covered: bool,
    pub feedback_report_covered: bool,
    pub compact_digest_covered: bool,
    pub no_roleplay_gate_passed: bool,
    pub life_slot_gate_passed: bool,
    pub work_integrity_gate_passed: bool,
    pub privacy_zero_gate_passed: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubjectProjectionBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub projection_report_covered: bool,
    pub budget_compiler_covered: bool,
    pub faithfulness_gate_passed: bool,
    pub private_disclosure_integrity_gate_passed: bool,
    pub gateway_raw_audit_redaction_covered: bool,
    pub raw_audit_disabled_reason_covered: bool,
    pub cross_surface_consistency_passed: bool,
    pub benchmark_judge_attached: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentToolExperienceBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub no_experience_empty_hints_covered: bool,
    pub governed_experience_hint_covered: bool,
    pub schema_drift_rejection_covered: bool,
    pub private_observation_not_public_covered: bool,
    pub gateway_no_cold_route_covered: bool,
    pub compact_registry_forbidden_covered: bool,
    pub host_execution_boundary_covered: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4EvalRecallBenchmarkJudgeReport {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub fixture_count: usize,
    pub required_k_covered: bool,
    pub missing_evidence_reported: bool,
    pub source_expanded_selected_split_covered: bool,
    pub w4_1_diagnostic_schema_covered: bool,
    pub w4_1_candidate_pool_split_covered: bool,
    pub mrr_covered: bool,
    pub noisy_external_wall_required: bool,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyBenchmarkSummary {
    #[serde(default)]
    pub suite: String,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub shards: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_source_sha256: Option<String>,
    #[serde(default)]
    pub samples: usize,
    #[serde(default)]
    pub questions: usize,
    #[serde(default)]
    pub evidence_questions: usize,
    #[serde(default)]
    pub any_evidence_hit: usize,
    #[serde(default)]
    pub all_evidence_hit: usize,
    #[serde(default)]
    pub write_errors: usize,
    #[serde(default)]
    pub recall_errors: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_hit_counts: Option<W4ExternalNoisyStageHitCounts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_diagnostics: Option<W4ExternalNoisyIndexDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w4_1_diagnostics: Option<W4ExternalNoisyW41Diagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facet_ablation: Option<W4ExternalNoisyFacetAblationDiagnostics>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyStageHitCounts {
    #[serde(default)]
    pub source_any_evidence_hit: usize,
    #[serde(default)]
    pub source_all_evidence_hit: usize,
    #[serde(default)]
    pub expanded_any_evidence_hit: usize,
    #[serde(default)]
    pub expanded_all_evidence_hit: usize,
    #[serde(default)]
    pub reranked_any_evidence_hit: usize,
    #[serde(default)]
    pub reranked_all_evidence_hit: usize,
    #[serde(default)]
    pub selected_any_evidence_hit: usize,
    #[serde(default)]
    pub selected_all_evidence_hit: usize,
    #[serde(default)]
    pub rendered_any_evidence_hit: usize,
    #[serde(default)]
    pub rendered_all_evidence_hit: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyIndexDiagnostics {
    #[serde(default)]
    pub questions_with_index_report: usize,
    #[serde(default)]
    pub index_used_questions: usize,
    #[serde(default)]
    pub fallback_full_scan_questions: usize,
    #[serde(default)]
    pub source_candidate_count: usize,
    #[serde(default)]
    pub matched_source_anchor_count: usize,
    #[serde(default)]
    pub unmatched_source_anchor_count: usize,
    #[serde(default)]
    pub indexed_neighbor_count: usize,
    #[serde(default)]
    pub filtered_node_count: usize,
    #[serde(default)]
    pub filtered_edge_count: usize,
    #[serde(default)]
    pub filtered_backlink_count: usize,
    #[serde(default)]
    pub failure_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyW41Diagnostics {
    #[serde(default)]
    pub questions_with_w4_1_diagnostics: usize,
    #[serde(default)]
    pub first_any_hit_stage_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub first_all_hit_stage_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub missing_gold_by_stage_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub miss_after_expanded_count: usize,
    #[serde(default)]
    pub gold_rank_found_count: usize,
    #[serde(default)]
    pub gold_rank_missing_count: usize,
    #[serde(default)]
    pub gold_rank_sum: usize,
    #[serde(default)]
    pub truncated_count: usize,
    #[serde(default)]
    pub blocked_reason_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub question_type_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub evidence_count_buckets: BTreeMap<String, usize>,
    #[serde(default)]
    pub source_signature_count: usize,
    #[serde(default)]
    pub repeated_source_signature_questions: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyFacetAblationDiagnostics {
    #[serde(default)]
    pub questions_with_ablation_report: usize,
    #[serde(default)]
    pub method_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub contribution_proven_questions: usize,
    #[serde(default)]
    pub render_growth: usize,
    #[serde(default)]
    pub required_slice_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub report_available_slice_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub contribution_proven_slice_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub affected_candidate_count: usize,
    #[serde(default)]
    pub blocked_reason_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisySuiteReport {
    pub suite: String,
    pub completed: bool,
    pub samples: usize,
    pub questions: usize,
    pub evidence_questions: usize,
    pub any_evidence_hit: usize,
    pub all_evidence_hit: usize,
    pub write_errors: usize,
    pub recall_errors: usize,
    pub shard_count: usize,
    pub expected_shard_count: Option<usize>,
    pub shards_valid: bool,
    pub expected_samples: Option<usize>,
    pub expected_questions: Option<usize>,
    pub expected_evidence_questions: Option<usize>,
    pub row_counts_valid: bool,
    pub summary_sha256: Option<String>,
    pub runner_source_sha256: Option<String>,
    pub any_evidence_hit_bps: u32,
    pub all_evidence_hit_bps: u32,
    pub noisy_split: bool,
    pub oracle_sanity_only: bool,
    pub baseline_any_evidence_hit: Option<usize>,
    pub baseline_all_evidence_hit: Option<usize>,
    pub regressed_against_baseline: bool,
    pub improved_against_baseline: bool,
    pub stage_hit_counts: Option<W4ExternalNoisyStageHitCounts>,
    pub index_diagnostics: Option<W4ExternalNoisyIndexDiagnostics>,
    pub w4_1_diagnostics: Option<W4ExternalNoisyW41Diagnostics>,
    pub facet_ablation: Option<W4ExternalNoisyFacetAblationDiagnostics>,
    pub stage_attributed_improvement: bool,
    pub index_effect_proven: bool,
    pub facet_ablation_effect_proven: bool,
    pub facet_ablation_no_render_growth: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyWallReport {
    pub release_gate_passed: bool,
    pub summary_attached: bool,
    pub required_suites_covered: bool,
    pub noisy_splits_covered: bool,
    pub completed: bool,
    pub no_runner_errors: bool,
    pub row_counts_covered: bool,
    pub shards_valid: bool,
    pub provenance_attached: bool,
    pub stage_diagnostics_attached: bool,
    pub index_diagnostics_attached: bool,
    pub index_no_full_scan: bool,
    pub w4_1_diagnostics_attached: bool,
    pub facet_ablation_attached: bool,
    pub oracle_sanity_only: bool,
    pub noisy_improvement_proven: bool,
    pub stage_attributed_improvement_proven: bool,
    pub index_effect_proven: bool,
    pub facet_ablation_effect_proven: bool,
    pub facet_ablation_no_render_growth: bool,
    pub suite_reports: Vec<W4ExternalNoisySuiteReport>,
    pub blocked_reasons: Vec<String>,
}

pub fn evaluate_w4_external_noisy_wall(
    summaries: &[W4ExternalNoisyBenchmarkSummary],
) -> W4ExternalNoisyWallReport {
    let suite_reports = summaries
        .iter()
        .map(w4_external_noisy_suite_report)
        .collect::<Vec<_>>();
    let summary_attached = !summaries.is_empty();
    let required_suites = [
        "locomo",
        "longmemeval_oracle",
        "longmemeval_s_cleaned",
        "longmemeval_m_cleaned",
    ];
    let noisy_suites = ["locomo", "longmemeval_s_cleaned", "longmemeval_m_cleaned"];
    let required_suites_covered = required_suites
        .iter()
        .all(|suite| summaries.iter().any(|summary| summary.suite == *suite));
    let noisy_splits_covered = noisy_suites
        .iter()
        .all(|suite| summaries.iter().any(|summary| summary.suite == *suite));
    let completed = summary_attached
        && required_suites_covered
        && required_suites.iter().all(|suite| {
            summaries
                .iter()
                .find(|summary| summary.suite == *suite)
                .is_some_and(|summary| summary.completed)
        });
    let no_runner_errors = summaries
        .iter()
        .all(|summary| summary.write_errors == 0 && summary.recall_errors == 0);
    let row_counts_covered = required_suites_covered
        && suite_reports
            .iter()
            .filter(|report| required_suites.iter().any(|suite| report.suite == *suite))
            .all(|report| report.row_counts_valid);
    let shards_valid = required_suites_covered
        && suite_reports
            .iter()
            .filter(|report| required_suites.iter().any(|suite| report.suite == *suite))
            .all(|report| report.shards_valid);
    let provenance_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| {
                summary
                    .summary_sha256
                    .as_deref()
                    .is_some_and(|hash| !hash.trim().is_empty())
                    && summary
                        .runner_source_sha256
                        .as_deref()
                        .is_some_and(|hash| !hash.trim().is_empty())
            });
    let stage_diagnostics_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| summary.stage_hit_counts.is_some());
    let index_diagnostics_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| summary.index_diagnostics.is_some());
    let index_no_full_scan = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(w4_external_index_diagnostics_no_full_scan);
    let w4_1_diagnostics_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(w4_external_w41_diagnostics_cover_summary);
    let facet_ablation_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(w4_external_facet_ablation_covers_summary);
    let facet_ablation_no_render_growth = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| {
                summary
                    .facet_ablation
                    .as_ref()
                    .is_none_or(|diagnostics| diagnostics.render_growth == 0)
            });
    let oracle_sanity_only = true;
    let noisy_reports = suite_reports
        .iter()
        .filter(|report| report.noisy_split)
        .collect::<Vec<_>>();
    let noisy_improvement_proven = noisy_splits_covered
        && noisy_reports
            .iter()
            .all(|report| report.improved_against_baseline);
    let stage_attributed_improvement_proven = suite_reports
        .iter()
        .find(|report| report.suite == "longmemeval_m_cleaned")
        .is_some_and(|report| report.stage_attributed_improvement);
    let index_effect_proven = suite_reports
        .iter()
        .find(|report| report.suite == "longmemeval_m_cleaned")
        .is_some_and(|report| report.index_effect_proven);
    let facet_ablation_effect_proven = suite_reports
        .iter()
        .find(|report| report.suite == "longmemeval_m_cleaned")
        .is_some_and(|report| report.facet_ablation_effect_proven);

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        summary_attached,
        "w4_external_noisy_wall_summary_missing",
    );
    push_missing(
        &mut blocked_reasons,
        required_suites_covered,
        "w4_external_noisy_wall_required_suites_missing",
    );
    push_missing(
        &mut blocked_reasons,
        noisy_splits_covered,
        "w4_external_noisy_wall_noisy_splits_missing",
    );
    push_missing(
        &mut blocked_reasons,
        completed,
        "w4_external_noisy_wall_incomplete",
    );
    push_missing(
        &mut blocked_reasons,
        no_runner_errors,
        "w4_external_noisy_wall_runner_errors",
    );
    push_missing(
        &mut blocked_reasons,
        row_counts_covered,
        "w4_external_noisy_wall_row_counts_invalid",
    );
    push_missing(
        &mut blocked_reasons,
        shards_valid,
        "w4_external_noisy_wall_shards_invalid",
    );
    push_missing(
        &mut blocked_reasons,
        provenance_attached,
        "w4_external_noisy_wall_provenance_missing",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || stage_diagnostics_attached,
        "w4_external_noisy_wall_stage_diagnostics_missing",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || index_diagnostics_attached,
        "w4_external_noisy_wall_index_diagnostics_missing",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || index_no_full_scan,
        "w4_external_noisy_wall_index_full_scan_detected",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || w4_1_diagnostics_attached,
        "w4_external_noisy_wall_w4_1_diagnostics_missing",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || facet_ablation_attached,
        "w4_external_noisy_wall_facet_ablation_missing",
    );
    push_missing(
        &mut blocked_reasons,
        !required_suites_covered || facet_ablation_no_render_growth,
        "w4_external_noisy_wall_render_growth_detected",
    );
    push_missing(
        &mut blocked_reasons,
        noisy_improvement_proven,
        "w4_external_noisy_wall_improvement_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        stage_attributed_improvement_proven,
        "w4_external_noisy_wall_stage_attribution_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        index_effect_proven,
        "w4_external_noisy_wall_index_effect_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        facet_ablation_effect_proven,
        "w4_external_noisy_wall_facet_ablation_effect_not_proven",
    );
    blocked_reasons.sort();
    blocked_reasons.dedup();

    W4ExternalNoisyWallReport {
        release_gate_passed: blocked_reasons.is_empty(),
        summary_attached,
        required_suites_covered,
        noisy_splits_covered,
        completed,
        no_runner_errors,
        row_counts_covered,
        shards_valid,
        provenance_attached,
        stage_diagnostics_attached,
        index_diagnostics_attached,
        index_no_full_scan,
        w4_1_diagnostics_attached,
        facet_ablation_attached,
        oracle_sanity_only,
        noisy_improvement_proven,
        stage_attributed_improvement_proven,
        index_effect_proven,
        facet_ablation_effect_proven,
        facet_ablation_no_render_growth,
        suite_reports,
        blocked_reasons,
    }
}

pub fn w4_external_noisy_summary_with_provenance(
    summary_json: &str,
    summary_sha256: impl Into<String>,
    runner_source_sha256: impl Into<String>,
) -> Result<W4ExternalNoisyBenchmarkSummary> {
    let summary_sha256 = normalize_sha256(summary_sha256.into(), "summary_sha256")?;
    let runner_source_sha256 =
        normalize_sha256(runner_source_sha256.into(), "runner_source_sha256")?;
    let mut summary = serde_json::from_str::<W4ExternalNoisyBenchmarkSummary>(summary_json)
        .map_err(|source| Error::Other {
            source: Box::new(source),
            stage: "w4_external_noisy_summary_json",
        })?;
    summary.summary_sha256 = Some(summary_sha256);
    summary.runner_source_sha256 = Some(runner_source_sha256);
    Ok(summary)
}

fn normalize_sha256(value: String, field: &'static str) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::config(
            "w4_external_noisy_provenance",
            format!("{field} must be a 64-character hex sha256"),
        ));
    }
    Ok(normalized)
}

fn w4_external_noisy_suite_report(
    summary: &W4ExternalNoisyBenchmarkSummary,
) -> W4ExternalNoisySuiteReport {
    let expected = w4_external_suite_expectation(&summary.suite);
    let shard_count = summary.shards.len();
    let shards_valid = expected.is_some_and(|expected| {
        let expected_names = (0..expected.shard_count)
            .map(|index| {
                format!(
                    "{}.shard-{index}-of-{}.summary.json",
                    summary.suite, expected.shard_count
                )
            })
            .collect::<BTreeSet<_>>();
        let actual_names = summary
            .shards
            .iter()
            .map(|shard| shard.trim().to_string())
            .collect::<BTreeSet<_>>();
        actual_names == expected_names
    });
    let row_counts_valid = expected.is_some_and(|expected| {
        summary.samples == expected.samples
            && summary.questions == expected.questions
            && summary.evidence_questions == expected.evidence_questions
    });
    let baseline = w4_external_suite_baseline(&summary.suite);
    let regressed_against_baseline = baseline.is_some_and(|baseline| {
        summary.any_evidence_hit < baseline.any_evidence_hit
            || summary.all_evidence_hit < baseline.all_evidence_hit
    });
    let improved_against_baseline = baseline.is_some_and(|baseline| {
        summary.any_evidence_hit > baseline.any_evidence_hit
            && summary.all_evidence_hit > baseline.all_evidence_hit
    });
    let stage_attributed_improvement =
        improved_against_baseline && stage_counts_show_graph_attributed_gain(summary, baseline);
    let index_effect_proven = stage_attributed_improvement
        && summary
            .index_diagnostics
            .as_ref()
            .is_some_and(index_diagnostics_show_index_effect);
    W4ExternalNoisySuiteReport {
        suite: summary.suite.clone(),
        completed: summary.completed,
        samples: summary.samples,
        questions: summary.questions,
        evidence_questions: summary.evidence_questions,
        any_evidence_hit: summary.any_evidence_hit,
        all_evidence_hit: summary.all_evidence_hit,
        write_errors: summary.write_errors,
        recall_errors: summary.recall_errors,
        shard_count,
        expected_shard_count: expected.map(|expected| expected.shard_count),
        shards_valid,
        expected_samples: expected.map(|expected| expected.samples),
        expected_questions: expected.map(|expected| expected.questions),
        expected_evidence_questions: expected.map(|expected| expected.evidence_questions),
        row_counts_valid,
        summary_sha256: summary.summary_sha256.clone(),
        runner_source_sha256: summary.runner_source_sha256.clone(),
        any_evidence_hit_bps: evidence_hit_bps(
            summary.any_evidence_hit,
            summary.evidence_questions,
        ),
        all_evidence_hit_bps: evidence_hit_bps(
            summary.all_evidence_hit,
            summary.evidence_questions,
        ),
        noisy_split: matches!(
            summary.suite.as_str(),
            "locomo" | "longmemeval_s_cleaned" | "longmemeval_m_cleaned"
        ),
        oracle_sanity_only: summary.suite == "longmemeval_oracle",
        baseline_any_evidence_hit: baseline.map(|baseline| baseline.any_evidence_hit),
        baseline_all_evidence_hit: baseline.map(|baseline| baseline.all_evidence_hit),
        regressed_against_baseline,
        improved_against_baseline,
        stage_hit_counts: summary.stage_hit_counts.clone(),
        index_diagnostics: summary.index_diagnostics.clone(),
        w4_1_diagnostics: summary.w4_1_diagnostics.clone(),
        facet_ablation: summary.facet_ablation.clone(),
        stage_attributed_improvement,
        index_effect_proven,
        facet_ablation_effect_proven: w4_external_facet_ablation_proves_effect(summary),
        facet_ablation_no_render_growth: summary
            .facet_ablation
            .as_ref()
            .is_some_and(|diagnostics| diagnostics.render_growth == 0),
    }
}

fn w4_external_w41_diagnostics_cover_summary(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(diagnostics) = summary.w4_1_diagnostics.as_ref() else {
        return false;
    };
    diagnostics.questions_with_w4_1_diagnostics == summary.questions
        && diagnostics.questions_with_w4_1_diagnostics > 0
        && !diagnostics.first_any_hit_stage_counts.is_empty()
        && !diagnostics.missing_gold_by_stage_counts.is_empty()
        && !diagnostics.question_type_counts.is_empty()
        && !diagnostics.evidence_count_buckets.is_empty()
        && diagnostics
            .gold_rank_found_count
            .saturating_add(diagnostics.gold_rank_missing_count)
            > 0
        && diagnostics.source_signature_count > 0
}

fn w4_external_facet_ablation_covers_summary(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(diagnostics) = summary.facet_ablation.as_ref() else {
        return false;
    };
    let required_slices = ["facet_off", "rank_fusion_off", "coverage_selection_off"];
    diagnostics.questions_with_ablation_report == summary.questions
        && diagnostics
            .method_counts
            .get("sdk_eval_recall_off_run_v1")
            .copied()
            .unwrap_or(0)
            == summary.questions
        && required_slices.iter().all(|slice| {
            diagnostics
                .required_slice_counts
                .get(*slice)
                .copied()
                .unwrap_or(0)
                == summary.questions
                && diagnostics
                    .report_available_slice_counts
                    .get(*slice)
                    .copied()
                    .unwrap_or(0)
                    == summary.questions
        })
}

fn w4_external_facet_ablation_proves_effect(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(diagnostics) = summary.facet_ablation.as_ref() else {
        return false;
    };
    w4_external_facet_ablation_covers_summary(summary)
        && diagnostics.contribution_proven_questions > 0
        && diagnostics.blocked_reason_counts.is_empty()
        && diagnostics
            .contribution_proven_slice_counts
            .get("facet_off")
            .copied()
            .unwrap_or(0)
            > 0
}

fn stage_counts_show_graph_attributed_gain(
    summary: &W4ExternalNoisyBenchmarkSummary,
    baseline: Option<W4ExternalSuiteBaseline>,
) -> bool {
    let Some(baseline) = baseline else {
        return false;
    };
    let Some(stage) = summary.stage_hit_counts.as_ref() else {
        return false;
    };
    let any_gain_after_source = stage.expanded_any_evidence_hit > stage.source_any_evidence_hit
        || stage.reranked_any_evidence_hit > stage.source_any_evidence_hit
        || stage.selected_any_evidence_hit > stage.source_any_evidence_hit
        || stage.rendered_any_evidence_hit > stage.source_any_evidence_hit;
    let all_gain_after_source = stage.expanded_all_evidence_hit > stage.source_all_evidence_hit
        || stage.reranked_all_evidence_hit > stage.source_all_evidence_hit
        || stage.selected_all_evidence_hit > stage.source_all_evidence_hit
        || stage.rendered_all_evidence_hit > stage.source_all_evidence_hit;
    stage.selected_any_evidence_hit > baseline.any_evidence_hit
        && stage.selected_all_evidence_hit > baseline.all_evidence_hit
        && stage.rendered_any_evidence_hit > baseline.any_evidence_hit
        && stage.rendered_all_evidence_hit > baseline.all_evidence_hit
        && any_gain_after_source
        && all_gain_after_source
}

fn index_diagnostics_show_index_effect(diagnostics: &W4ExternalNoisyIndexDiagnostics) -> bool {
    diagnostics.questions_with_index_report > 0
        && diagnostics.index_used_questions > 0
        && diagnostics.index_used_questions <= diagnostics.questions_with_index_report
        && diagnostics.fallback_full_scan_questions == 0
        && diagnostics.matched_source_anchor_count > 0
        && diagnostics.indexed_neighbor_count > 0
        && diagnostics.failure_count == 0
}

fn w4_external_index_diagnostics_no_full_scan(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(diagnostics) = summary.index_diagnostics.as_ref() else {
        return true;
    };
    diagnostics.questions_with_index_report == summary.questions
        && diagnostics.index_used_questions > 0
        && diagnostics.fallback_full_scan_questions == 0
        && diagnostics.failure_count == 0
        && diagnostics.matched_source_anchor_count > 0
        && diagnostics.indexed_neighbor_count > 0
}

#[derive(Clone, Copy)]
struct W4ExternalSuiteExpectation {
    samples: usize,
    questions: usize,
    evidence_questions: usize,
    shard_count: usize,
}

#[derive(Clone, Copy)]
struct W4ExternalSuiteBaseline {
    any_evidence_hit: usize,
    all_evidence_hit: usize,
}

fn w4_external_suite_expectation(suite: &str) -> Option<W4ExternalSuiteExpectation> {
    match suite {
        "locomo" => Some(W4ExternalSuiteExpectation {
            samples: 10,
            questions: 1986,
            evidence_questions: 1982,
            shard_count: 10,
        }),
        "longmemeval_oracle" | "longmemeval_s_cleaned" | "longmemeval_m_cleaned" => {
            Some(W4ExternalSuiteExpectation {
                samples: 500,
                questions: 500,
                evidence_questions: 500,
                shard_count: if suite == "longmemeval_m_cleaned" {
                    8
                } else {
                    1
                },
            })
        }
        _ => None,
    }
}

fn evidence_hit_bps(hits: usize, evidence_questions: usize) -> u32 {
    if evidence_questions == 0 {
        return 0;
    }
    ((hits.saturating_mul(10_000)) / evidence_questions).min(u32::MAX as usize) as u32
}

fn w4_external_suite_baseline(suite: &str) -> Option<W4ExternalSuiteBaseline> {
    match suite {
        "locomo" => Some(W4ExternalSuiteBaseline {
            any_evidence_hit: 297,
            all_evidence_hit: 189,
        }),
        "longmemeval_s_cleaned" => Some(W4ExternalSuiteBaseline {
            any_evidence_hit: 451,
            all_evidence_hit: 353,
        }),
        "longmemeval_m_cleaned" => Some(W4ExternalSuiteBaseline {
            any_evidence_hit: 104,
            all_evidence_hit: 33,
        }),
        _ => None,
    }
}

pub fn load_memory_benchmark_fixture_dir(
    root: impl AsRef<Path>,
) -> Result<Vec<MemoryBenchmarkFixture>> {
    let mut files = Vec::new();
    collect_json_files(root.as_ref(), &mut files)?;
    files.sort();

    let mut fixtures = Vec::with_capacity(files.len());
    for file in files {
        let body = fs::read_to_string(&file)
            .map_err(|source| Error::io("memory_benchmark_fixture_read", source))?;
        let fixture = serde_json::from_str::<MemoryBenchmarkFixture>(&body).map_err(|source| {
            Error::Other {
                source: Box::new(source),
                stage: "memory_benchmark_fixture_parse",
            }
        })?;
        fixtures.push(fixture);
    }
    Ok(fixtures)
}

pub fn run_memory_benchmark_wall(fixtures: &[MemoryBenchmarkFixture]) -> MemoryBenchmarkReport {
    let class_coverage = MemoryBenchmarkClass::ALL
        .iter()
        .copied()
        .map(|class| MemoryBenchmarkClassCoverage {
            class,
            compact_fixtures: fixtures
                .iter()
                .filter(|fixture| {
                    fixture.class == class && fixture.mode == MemoryBenchmarkMode::Compact
                })
                .count(),
            full_fixtures: fixtures
                .iter()
                .filter(|fixture| {
                    fixture.class == class && fixture.mode == MemoryBenchmarkMode::Full
                })
                .count(),
        })
        .collect::<Vec<_>>();

    let semantic_coverage = MemoryBenchmarkSemanticDimension::ALL
        .iter()
        .copied()
        .map(|dimension| MemoryBenchmarkSemanticCoverage {
            dimension,
            fixture_count: fixtures
                .iter()
                .filter(|fixture| fixture.semantic_contract.dimensions.contains(&dimension))
                .count(),
        })
        .collect::<Vec<_>>();

    let mut missing_classes = Vec::new();
    for coverage in &class_coverage {
        if coverage.compact_fixtures == 0 {
            missing_classes.push(MemoryBenchmarkMissingClass {
                class: coverage.class,
                mode: MemoryBenchmarkMode::Compact,
            });
        }
        if coverage.full_fixtures == 0 {
            missing_classes.push(MemoryBenchmarkMissingClass {
                class: coverage.class,
                mode: MemoryBenchmarkMode::Full,
            });
        }
    }

    let mut semantic_failures = fixtures
        .iter()
        .flat_map(validate_memory_benchmark_semantics)
        .collect::<Vec<_>>();
    for coverage in &semantic_coverage {
        if coverage.fixture_count == 0 {
            semantic_failures.push(memory_benchmark_suite_semantic_failure(
                Some(coverage.dimension),
                "semantic_dimension_coverage",
                "expected at least one fixture covering semantic dimension",
            ));
        }
    }

    let failures = fixtures
        .iter()
        .flat_map(validate_memory_benchmark_fixture)
        .collect::<Vec<_>>();
    let mut failed_fixture_ids = failures
        .iter()
        .map(|failure| failure.fixture_id.as_str())
        .collect::<BTreeSet<_>>();
    failed_fixture_ids.extend(
        semantic_failures
            .iter()
            .filter(|failure| failure.fixture_id != "__suite__")
            .map(|failure| failure.fixture_id.as_str()),
    );
    let failed_fixture_count = failed_fixture_ids.len();
    let soul_kernel_judge = build_soul_kernel_benchmark_judge(fixtures);
    let subject_projection_judge = build_subject_projection_benchmark_judge(fixtures);
    let agent_tool_experience_judge = build_agent_tool_experience_benchmark_judge(fixtures);
    let w4_eval_recall_judge = build_w4_eval_recall_benchmark_judge(fixtures);
    let passed = failures.is_empty()
        && semantic_failures.is_empty()
        && soul_kernel_judge.release_gate_passed
        && subject_projection_judge.release_gate_passed
        && agent_tool_experience_judge.release_gate_passed
        && w4_eval_recall_judge.release_gate_passed;

    MemoryBenchmarkReport {
        suite: "memory_benchmark_wall".to_string(),
        total_fixtures: fixtures.len(),
        passed_fixtures: fixtures.len().saturating_sub(failed_fixture_count),
        baseline: calculate_memory_benchmark_baseline(fixtures),
        class_coverage,
        missing_classes,
        semantic_coverage,
        soul_kernel_judge,
        subject_projection_judge,
        agent_tool_experience_judge,
        w4_eval_recall_judge,
        failures,
        semantic_failures,
        passed,
    }
    .with_missing_class_gate()
}

fn build_soul_kernel_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> SoulKernelBenchmarkJudgeReport {
    let soul_fixtures = fixtures
        .iter()
        .filter(|fixture| fixture.class == MemoryBenchmarkClass::SoulRegression)
        .collect::<Vec<_>>();
    let fixture_ids = soul_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let growth_proposal_contract_covered = any_fixture_has_key_or_surface(
        &soul_fixtures,
        "soul_growth_proposal",
        "SoulGrowthProposal",
    );
    let regression_suite_covered = any_fixture_has_key_or_surface(
        &soul_fixtures,
        "soul_regression_suite",
        "SoulRegressionSuite",
    );
    let feedback_report_covered = any_fixture_has_key_or_surface(
        &soul_fixtures,
        "soul_feedback_report",
        "SoulFeedbackReport",
    );
    let compact_digest_covered =
        any_fixture_has_key_or_surface(&soul_fixtures, "soul_compact_digest", "SoulCompactDigest");
    let no_roleplay_gate_passed = any_fixture_has_key(&soul_fixtures, "roleplay_prompt_rejected")
        && !any_fixture_has_marker(&soul_fixtures, "append persona prompt")
        && !any_fixture_has_marker(&soul_fixtures, "just pretend to be");
    let life_slot_gate_passed = any_fixture_has_key(&soul_fixtures, "soul_life_facets")
        && any_fixture_has_key(&soul_fixtures, "self_owned_update_candidates");
    let work_integrity_gate_passed = any_fixture_has_key(&soul_fixtures, "work_integrity_covenant")
        || any_fixture_has_surface(&soul_fixtures, "Work Integrity Covenant");
    let privacy_zero_gate_passed = fixtures.iter().all(|fixture| {
        fixture.metrics.privacy_violation_count <= fixture.thresholds.max_privacy_violation_count
            && fixture.metrics.soul_regression_count <= fixture.thresholds.max_soul_regression_count
    });

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        growth_proposal_contract_covered,
        "soul_growth_proposal_contract_missing",
    );
    push_missing(
        &mut blocked_reasons,
        regression_suite_covered,
        "soul_regression_suite_missing",
    );
    push_missing(
        &mut blocked_reasons,
        feedback_report_covered,
        "soul_feedback_report_missing",
    );
    push_missing(
        &mut blocked_reasons,
        compact_digest_covered,
        "soul_compact_digest_missing",
    );
    push_missing(
        &mut blocked_reasons,
        no_roleplay_gate_passed,
        "no_roleplay_host_mount_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        life_slot_gate_passed,
        "soul_life_slot_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        work_integrity_gate_passed,
        "work_integrity_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        privacy_zero_gate_passed,
        "soul_privacy_zero_gate_failed",
    );

    SoulKernelBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        growth_proposal_contract_covered,
        regression_suite_covered,
        feedback_report_covered,
        compact_digest_covered,
        no_roleplay_gate_passed,
        life_slot_gate_passed,
        work_integrity_gate_passed,
        privacy_zero_gate_passed,
        blocked_reasons,
    }
}

fn build_agent_tool_experience_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> AgentToolExperienceBenchmarkJudgeReport {
    let tool_fixtures = fixtures
        .iter()
        .filter(|fixture| fixture.class == MemoryBenchmarkClass::AgentToolExperience)
        .collect::<Vec<_>>();
    let fixture_ids = tool_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let no_experience_empty_hints_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_no_experience_empty_hints")
            && any_fixture_has_marker(&tool_fixtures, "no_governed_tool_experience");
    let governed_experience_hint_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_governed_hint")
            && any_fixture_has_key(&tool_fixtures, "agent_tool_hints");
    let schema_drift_rejection_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_registry_fingerprint_mismatch")
            || any_fixture_has_key(&tool_fixtures, "agent_tool_experience_stale_schema");
    let private_observation_not_public_covered =
        any_fixture_has_key(&tool_fixtures, "agent_tool_private_observation_excluded")
            && any_fixture_has_marker(&tool_fixtures, "private observation not projected");
    let gateway_no_cold_route_covered =
        any_fixture_has_key(&tool_fixtures, "gateway_host_tools_no_cold_route")
            && any_fixture_has_marker(&tool_fixtures, "host fallback required");
    let compact_registry_forbidden_covered = tool_fixtures.iter().any(|fixture| {
        fixture.mode == MemoryBenchmarkMode::Compact
            && any_fixture_has_key(&[*fixture], "agent_tool_registry_forbidden_by_profile")
    });
    let host_execution_boundary_covered =
        any_fixture_has_key(&tool_fixtures, "host_execution_required")
            && any_fixture_has_surface(&tool_fixtures, "HostToolRegistry");

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        no_experience_empty_hints_covered,
        "agent_tool_no_experience_empty_hints_missing",
    );
    push_missing(
        &mut blocked_reasons,
        governed_experience_hint_covered,
        "agent_tool_governed_hint_missing",
    );
    push_missing(
        &mut blocked_reasons,
        schema_drift_rejection_covered,
        "agent_tool_schema_drift_rejection_missing",
    );
    push_missing(
        &mut blocked_reasons,
        private_observation_not_public_covered,
        "agent_tool_private_observation_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        gateway_no_cold_route_covered,
        "gateway_host_tools_no_cold_route_missing",
    );
    push_missing(
        &mut blocked_reasons,
        compact_registry_forbidden_covered,
        "agent_tool_compact_registry_forbidden_missing",
    );
    push_missing(
        &mut blocked_reasons,
        host_execution_boundary_covered,
        "agent_tool_host_execution_boundary_missing",
    );

    AgentToolExperienceBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        no_experience_empty_hints_covered,
        governed_experience_hint_covered,
        schema_drift_rejection_covered,
        private_observation_not_public_covered,
        gateway_no_cold_route_covered,
        compact_registry_forbidden_covered,
        host_execution_boundary_covered,
        blocked_reasons,
    }
}

fn build_w4_eval_recall_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> W4EvalRecallBenchmarkJudgeReport {
    let w4_fixtures = fixtures
        .iter()
        .filter(|fixture| fixture_declares_w4_eval_recall(fixture))
        .collect::<Vec<_>>();
    let fixture_ids = w4_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let fixture_count = w4_fixtures.len();
    let required_k = [5_usize, 10, 20, 50];
    let required_k_covered = !w4_fixtures.is_empty()
        && w4_fixtures.iter().all(|fixture| {
            fixture.eval_recall.as_ref().is_some_and(|eval| {
                required_k
                    .iter()
                    .all(|k| eval.metrics.recall_at_k.iter().any(|entry| entry.k == *k))
            })
        });
    let missing_evidence_reported = !w4_fixtures.is_empty()
        && w4_fixtures
            .iter()
            .all(|fixture| w4_missing_evidence_contract_holds(fixture));
    let source_expanded_selected_split_covered = w4_fixtures.iter().any(|fixture| {
        fixture
            .eval_recall
            .as_ref()
            .is_some_and(w4_eval_has_report_split)
    });
    let w4_1_diagnostic_schema_covered = !w4_fixtures.is_empty()
        && w4_fixtures.iter().all(|fixture| {
            fixture
                .eval_recall
                .as_ref()
                .is_some_and(w4_1_diagnostic_contract_holds)
        });
    let w4_1_candidate_pool_split_covered = !w4_fixtures.is_empty()
        && w4_fixtures.iter().all(|fixture| {
            fixture
                .eval_recall
                .as_ref()
                .is_some_and(w4_1_candidate_pool_split_holds)
        });
    let mrr_covered = !w4_fixtures.is_empty()
        && w4_fixtures.iter().all(|fixture| {
            fixture.eval_recall.as_ref().is_some_and(|eval| {
                eval.metrics.mrr_bps > 0
                    || eval.metrics.recall_at_k.iter().all(|entry| {
                        !entry.any_evidence_hit && entry.matched_evidence_refs.is_empty()
                    })
            })
        });
    let noisy_external_wall_required = true;

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        fixture_count > 0,
        "w4_eval_recall_fixture_missing",
    );
    push_missing(
        &mut blocked_reasons,
        required_k_covered,
        "w4_eval_recall_required_k_missing",
    );
    push_missing(
        &mut blocked_reasons,
        missing_evidence_reported,
        "w4_eval_recall_missing_evidence_report_missing",
    );
    push_missing(
        &mut blocked_reasons,
        source_expanded_selected_split_covered,
        "w4_eval_recall_report_split_missing",
    );
    push_missing(
        &mut blocked_reasons,
        w4_1_diagnostic_schema_covered,
        "w4_eval_recall_w4_1_diagnostics_missing",
    );
    push_missing(
        &mut blocked_reasons,
        w4_1_candidate_pool_split_covered,
        "w4_eval_recall_candidate_pool_split_missing",
    );
    push_missing(
        &mut blocked_reasons,
        mrr_covered,
        "w4_eval_recall_mrr_missing",
    );
    blocked_reasons.sort();
    blocked_reasons.dedup();

    W4EvalRecallBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        fixture_count,
        required_k_covered,
        missing_evidence_reported,
        source_expanded_selected_split_covered,
        w4_1_diagnostic_schema_covered,
        w4_1_candidate_pool_split_covered,
        mrr_covered,
        noisy_external_wall_required,
        blocked_reasons,
    }
}

fn fixture_declares_w4_eval_recall(fixture: &MemoryBenchmarkFixture) -> bool {
    fixture
        .semantic_contract
        .provided_keys
        .iter()
        .chain(fixture.semantic_contract.required_keys.iter())
        .any(|key| key == "w4_eval_recall")
        || fixture.eval_recall.is_some()
}

fn w4_eval_has_report_split(eval: &MemoryBenchmarkEvalRecall) -> bool {
    !eval.source_candidates.is_empty()
        && !eval.expanded_candidates.is_empty()
        && !eval.selected_candidates.is_empty()
        && eval.expanded_candidates.iter().any(|candidate| {
            !eval
                .source_candidates
                .iter()
                .any(|source| source == candidate)
        })
}

fn w4_1_candidate_pool_split_holds(eval: &MemoryBenchmarkEvalRecall) -> bool {
    !eval.graph_anchor_candidates.is_empty()
        && !eval.eval_candidate_pool.is_empty()
        && !eval.rendered_candidates.is_empty()
        && eval.eval_candidate_pool.iter().all(|candidate| {
            eval.source_candidates
                .iter()
                .any(|source| source == candidate)
                || eval
                    .graph_anchor_candidates
                    .iter()
                    .any(|anchor| anchor == candidate)
                || eval
                    .expanded_candidates
                    .iter()
                    .any(|expanded| expanded == candidate)
                || eval
                    .selected_candidates
                    .iter()
                    .any(|selected| selected == candidate)
        })
        && eval.eval_candidate_pool.iter().any(|candidate| {
            !eval
                .rendered_candidates
                .iter()
                .any(|rendered| rendered == candidate)
        })
}

fn w4_1_diagnostic_contract_holds(eval: &MemoryBenchmarkEvalRecall) -> bool {
    let expected = &eval.expected_evidence_refs;
    let diagnostics = &eval.diagnostics;
    let rendered_refs_match = !eval.rendered_evidence_refs.is_empty()
        && eval.rendered_evidence_refs.iter().all(|evidence_ref| {
            eval.evidence_ref_index.iter().any(|entry| {
                eval.rendered_candidates
                    .iter()
                    .any(|candidate| candidate == &entry.candidate_id)
                    && entry
                        .evidence_refs
                        .iter()
                        .any(|indexed_ref| indexed_ref == evidence_ref)
            })
        });
    diagnostics.evidence_count == expected.len()
        && !expected.is_empty()
        && !diagnostics.first_any_hit_stage.trim().is_empty()
        && !diagnostics.first_all_hit_stage.trim().is_empty()
        && stage_evidence_refs_cover(&diagnostics.matched_gold_by_stage, "expanded", expected)
        && diagnostics
            .missing_gold_by_stage
            .iter()
            .any(|stage| !stage.stage.trim().is_empty())
        && expected.iter().all(|evidence_ref| {
            diagnostics.gold_rank_by_stage.iter().any(|rank| {
                rank.evidence_ref == *evidence_ref
                    && !rank.stage.trim().is_empty()
                    && rank.rank.is_some()
            })
        })
        && !diagnostics.source_anchor_ids.is_empty()
        && !diagnostics.graph_anchor_candidate_ids.is_empty()
        && !diagnostics.expanded_node_ids.is_empty()
        && !diagnostics.graph_neighbor_ids.is_empty()
        && expected.iter().all(|evidence_ref| {
            diagnostics.graph_distance_to_gold.iter().any(|distance| {
                distance.evidence_ref == *evidence_ref && distance.distance.is_some()
            })
        })
        && rendered_refs_match
}

fn stage_evidence_refs_cover(
    stages: &[MemoryBenchmarkEvalRecallStageEvidenceRefs],
    stage: &str,
    expected_evidence_refs: &[String],
) -> bool {
    stages.iter().any(|entry| {
        entry.stage == stage
            && expected_evidence_refs
                .iter()
                .all(|expected| entry.evidence_refs.iter().any(|actual| actual == expected))
    })
}

fn w4_missing_evidence_contract_holds(fixture: &MemoryBenchmarkFixture) -> bool {
    let Some(eval) = fixture.eval_recall.as_ref() else {
        return false;
    };
    let matched = eval
        .metrics
        .recall_at_k
        .iter()
        .flat_map(|entry| entry.matched_evidence_refs.iter())
        .collect::<BTreeSet<_>>();
    let unmatched_expected = eval
        .expected_evidence_refs
        .iter()
        .filter(|expected| {
            !matched
                .iter()
                .any(|actual| actual.as_str() == expected.as_str())
        })
        .collect::<Vec<_>>();
    unmatched_expected.is_empty() || !eval.missing_evidence_refs.is_empty()
}

fn build_subject_projection_benchmark_judge(
    fixtures: &[MemoryBenchmarkFixture],
) -> SubjectProjectionBenchmarkJudgeReport {
    let projection_fixtures = fixtures
        .iter()
        .filter(|fixture| {
            fixture.class == MemoryBenchmarkClass::SubjectProjection
                || fixture.class == MemoryBenchmarkClass::PrivacyRefusal
        })
        .collect::<Vec<_>>();
    let fixture_ids = projection_fixtures
        .iter()
        .map(|fixture| fixture.fixture_id.clone())
        .collect::<Vec<_>>();
    let projection_report_covered =
        any_fixture_has_key_or_surface(
            &projection_fixtures,
            "subject_projection_report",
            "SubjectProjectionReport",
        ) || any_fixture_has_surface(&projection_fixtures, "MemoryProjectionReport");
    let budget_compiler_covered = any_fixture_has_key(&projection_fixtures, "budget_decisions")
        || any_fixture_has_key(&projection_fixtures, "projection_budget_compiler")
        || any_fixture_has_surface(&projection_fixtures, "ProjectionBudgetCompiler");
    let faithfulness_gate_passed =
        projection_fixtures.iter().all(|fixture| {
            fixture.metrics.projection_faithfulness_bps
                >= fixture.thresholds.min_projection_faithfulness_bps
        }) && (any_fixture_has_key(&projection_fixtures, "projection_faithfulness_check")
            || any_fixture_has_surface(&projection_fixtures, "ProjectionFaithfulnessCheck"));
    let private_disclosure_integrity_gate_passed =
        any_fixture_has_key(&projection_fixtures, "private_disclosure_integrity_report")
            || any_fixture_has_key(&projection_fixtures, "private_raw_absent")
            || any_fixture_has_surface(&projection_fixtures, "PrivateDisclosureIntegrityReport");
    let gateway_raw_audit_redaction_covered =
        any_fixture_has_key(&projection_fixtures, "gateway_raw_audit_redacted")
            || any_fixture_has_key(&projection_fixtures, "redacted_private_envelope")
            || any_fixture_has_surface(&projection_fixtures, "RawProjectionAudit");
    let raw_audit_disabled_reason_covered =
        any_fixture_has_key(&projection_fixtures, "raw_audit_disabled_reason")
            || any_fixture_has_marker(
                &projection_fixtures,
                "raw audit unavailable reason when disabled",
            )
            || any_fixture_has_marker(&projection_fixtures, "raw_projection_recording_disabled");
    let cross_surface_consistency_passed =
        any_fixture_has_key(&projection_fixtures, "cross_surface_consistency")
            && all_projection_fixtures_have_required_surface_context(&projection_fixtures);
    let benchmark_judge_attached = projection_fixtures.iter().any(|fixture| {
        matches!(
            fixture.evaluation_source,
            MemoryBenchmarkEvaluationSource::RuntimeReplay
                | MemoryBenchmarkEvaluationSource::GoldenJudge
        )
    });

    let mut blocked_reasons = Vec::new();
    push_missing(
        &mut blocked_reasons,
        projection_report_covered,
        "subject_projection_report_missing",
    );
    push_missing(
        &mut blocked_reasons,
        budget_compiler_covered,
        "projection_budget_compiler_missing",
    );
    push_missing(
        &mut blocked_reasons,
        faithfulness_gate_passed,
        "projection_faithfulness_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        private_disclosure_integrity_gate_passed,
        "private_disclosure_integrity_gate_missing",
    );
    push_missing(
        &mut blocked_reasons,
        gateway_raw_audit_redaction_covered,
        "gateway_raw_audit_redaction_missing",
    );
    push_missing(
        &mut blocked_reasons,
        raw_audit_disabled_reason_covered,
        "raw_audit_disabled_reason_missing",
    );
    push_missing(
        &mut blocked_reasons,
        cross_surface_consistency_passed,
        "cross_surface_consistency_missing",
    );
    push_missing(
        &mut blocked_reasons,
        benchmark_judge_attached,
        "runtime_or_golden_benchmark_judge_missing",
    );

    SubjectProjectionBenchmarkJudgeReport {
        release_gate_passed: blocked_reasons.is_empty(),
        fixture_ids,
        projection_report_covered,
        budget_compiler_covered,
        faithfulness_gate_passed,
        private_disclosure_integrity_gate_passed,
        gateway_raw_audit_redaction_covered,
        raw_audit_disabled_reason_covered,
        cross_surface_consistency_passed,
        benchmark_judge_attached,
        blocked_reasons,
    }
}

fn collect_json_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(root).map_err(|source| Error::io("memory_benchmark_fixture_list", source))?
    {
        let entry =
            entry.map_err(|source| Error::io("memory_benchmark_fixture_list_entry", source))?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            files.push(path);
        }
    }
    Ok(())
}

fn validate_memory_benchmark_fixture(
    fixture: &MemoryBenchmarkFixture,
) -> Vec<MemoryBenchmarkFailure> {
    let mut failures = Vec::new();
    push_min_bps_failure(
        &mut failures,
        fixture,
        "accuracy_bps",
        fixture.metrics.accuracy_bps,
        fixture.thresholds.min_accuracy_bps,
    );
    push_min_bps_failure(
        &mut failures,
        fixture,
        "evidence_precision_bps",
        fixture.metrics.evidence_precision_bps,
        fixture.thresholds.min_evidence_precision_bps,
    );
    push_min_bps_failure(
        &mut failures,
        fixture,
        "projection_faithfulness_bps",
        fixture.metrics.projection_faithfulness_bps,
        fixture.thresholds.min_projection_faithfulness_bps,
    );
    push_max_u32_failure(
        &mut failures,
        fixture,
        "privacy_violation_count",
        fixture.metrics.privacy_violation_count,
        fixture.thresholds.max_privacy_violation_count,
    );
    push_max_u32_failure(
        &mut failures,
        fixture,
        "stale_memory_false_positive_count",
        fixture.metrics.stale_memory_false_positive_count,
        fixture.thresholds.max_stale_memory_false_positive_count,
    );
    push_min_bps_failure(
        &mut failures,
        fixture,
        "procedural_reuse_success_bps",
        fixture.metrics.procedural_reuse_success_bps,
        fixture.thresholds.min_procedural_reuse_success_bps,
    );
    push_max_u32_failure(
        &mut failures,
        fixture,
        "soul_regression_count",
        fixture.metrics.soul_regression_count,
        fixture.thresholds.max_soul_regression_count,
    );
    if let Some(max_latency_ms) = fixture.thresholds.max_latency_ms {
        push_max_u32_failure(
            &mut failures,
            fixture,
            "latency_ms",
            fixture.metrics.latency_ms,
            max_latency_ms,
        );
    }
    if let Some(max_token_budget) = fixture.thresholds.max_token_budget {
        push_max_u32_failure(
            &mut failures,
            fixture,
            "token_budget",
            fixture.metrics.token_budget,
            max_token_budget,
        );
    }
    if let Some(max_memory_bytes) = fixture.thresholds.max_memory_bytes {
        if fixture.metrics.memory_bytes > max_memory_bytes {
            failures.push(memory_benchmark_failure(
                fixture,
                "memory_bytes",
                format!(
                    "expected at most {}, got {}",
                    max_memory_bytes, fixture.metrics.memory_bytes
                ),
            ));
        }
    }
    failures.extend(validate_w4_eval_recall_fixture(fixture));
    failures
}

fn validate_w4_eval_recall_fixture(
    fixture: &MemoryBenchmarkFixture,
) -> Vec<MemoryBenchmarkFailure> {
    if !fixture_declares_w4_eval_recall(fixture) {
        return Vec::new();
    }

    let Some(eval) = fixture.eval_recall.as_ref() else {
        return vec![memory_benchmark_failure(
            fixture,
            "w4_eval_recall_contract",
            "w4_eval_recall fixture is declared but eval_recall payload is missing",
        )];
    };

    let mut missing = Vec::new();
    push_w4_missing(&mut missing, !eval.suite.trim().is_empty(), "suite");
    push_w4_missing(&mut missing, !eval.split.trim().is_empty(), "split");
    push_w4_missing(
        &mut missing,
        !eval.question_id.trim().is_empty(),
        "question_id",
    );
    push_w4_missing(
        &mut missing,
        !eval.question_type.trim().is_empty(),
        "question_type",
    );
    push_w4_missing(
        &mut missing,
        !eval.expected_evidence_refs.is_empty(),
        "expected_evidence_refs",
    );
    push_w4_missing(
        &mut missing,
        !eval.source_candidates.is_empty(),
        "source_candidates",
    );
    push_w4_missing(
        &mut missing,
        !eval.graph_anchor_candidates.is_empty(),
        "graph_anchor_candidates",
    );
    push_w4_missing(
        &mut missing,
        !eval.expanded_candidates.is_empty(),
        "expanded_candidates",
    );
    push_w4_missing(
        &mut missing,
        !eval.eval_candidate_pool.is_empty(),
        "eval_candidate_pool",
    );
    push_w4_missing(
        &mut missing,
        !eval.selected_candidates.is_empty(),
        "selected_candidates",
    );
    push_w4_missing(
        &mut missing,
        !eval.rendered_candidates.is_empty(),
        "rendered_candidates",
    );
    push_w4_missing(
        &mut missing,
        !eval.rendered_block_preview.trim().is_empty(),
        "rendered_block_preview",
    );
    push_w4_missing(
        &mut missing,
        !eval.rendered_evidence_refs.is_empty(),
        "rendered_evidence_refs",
    );
    push_w4_missing(
        &mut missing,
        !eval.evidence_ref_index.is_empty(),
        "evidence_ref_index",
    );
    let required_k = [5_usize, 10, 20, 50];
    for k in required_k {
        push_w4_missing(
            &mut missing,
            eval.metrics.recall_at_k.iter().any(|entry| entry.k == k),
            format!("recall_at_k:{k}"),
        );
    }
    push_w4_missing(
        &mut missing,
        w4_missing_evidence_contract_holds(fixture),
        "missing_evidence_refs",
    );
    push_w4_missing(
        &mut missing,
        eval.metrics.mrr_bps > 0
            || eval
                .metrics
                .recall_at_k
                .iter()
                .all(|entry| !entry.any_evidence_hit && entry.matched_evidence_refs.is_empty()),
        "mrr_bps",
    );
    push_w4_missing(
        &mut missing,
        w4_1_candidate_pool_split_holds(eval),
        "w4_1_candidate_pool_split",
    );
    push_w4_missing(
        &mut missing,
        w4_1_diagnostic_contract_holds(eval),
        "w4_1_diagnostics",
    );

    if missing.is_empty() {
        Vec::new()
    } else {
        vec![memory_benchmark_failure(
            fixture,
            "w4_eval_recall_contract",
            format!("missing or invalid {}", missing.join(", ")),
        )]
    }
}

fn push_w4_missing(missing: &mut Vec<String>, condition: bool, field: impl Into<String>) {
    if !condition {
        missing.push(field.into());
    }
}

fn validate_memory_benchmark_semantics(
    fixture: &MemoryBenchmarkFixture,
) -> Vec<MemoryBenchmarkSemanticFailure> {
    let contract = &fixture.semantic_contract;
    if contract.is_empty() {
        return Vec::new();
    }

    let mut failures = Vec::new();
    if contract.dimensions.is_empty() {
        failures.push(memory_benchmark_semantic_failure(
            fixture,
            None,
            "semantic_dimension",
            "semantic contract must declare at least one gate dimension",
        ));
    }

    for required_key in &contract.required_keys {
        if !contract.provided_keys.contains(required_key) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_required_key",
                format!("missing required key {required_key}"),
            ));
        }
    }
    for forbidden_key in &contract.forbidden_keys {
        if contract.provided_keys.contains(forbidden_key) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_forbidden_key",
                format!("forbidden key {forbidden_key} is present"),
            ));
        }
    }
    for required_marker in &contract.required_markers {
        if !contract.observed_markers.contains(required_marker) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_required_marker",
                format!("missing required marker {required_marker}"),
            ));
        }
    }
    for forbidden_marker in &contract.forbidden_markers {
        if contract.observed_markers.contains(forbidden_marker) {
            failures.push(memory_benchmark_semantic_failure(
                fixture,
                None,
                "semantic_forbidden_marker",
                format!("forbidden marker {forbidden_marker} is present"),
            ));
        }
    }

    failures
}

fn any_fixture_has_key(fixtures: &[&MemoryBenchmarkFixture], key: &str) -> bool {
    fixtures.iter().any(|fixture| {
        fixture
            .semantic_contract
            .provided_keys
            .iter()
            .chain(fixture.semantic_contract.required_keys.iter())
            .any(|candidate| candidate == key)
    })
}

fn any_fixture_has_surface(fixtures: &[&MemoryBenchmarkFixture], surface: &str) -> bool {
    fixtures.iter().any(|fixture| {
        fixture
            .scenario
            .expected_surfaces
            .iter()
            .any(|candidate| candidate == surface)
    })
}

fn any_fixture_has_key_or_surface(
    fixtures: &[&MemoryBenchmarkFixture],
    key: &str,
    surface: &str,
) -> bool {
    any_fixture_has_key(fixtures, key) || any_fixture_has_surface(fixtures, surface)
}

fn any_fixture_has_marker(fixtures: &[&MemoryBenchmarkFixture], marker: &str) -> bool {
    fixtures.iter().any(|fixture| {
        fixture
            .semantic_contract
            .observed_markers
            .iter()
            .chain(fixture.semantic_contract.required_markers.iter())
            .any(|candidate| candidate == marker)
    })
}

fn all_projection_fixtures_have_required_surface_context(
    fixtures: &[&MemoryBenchmarkFixture],
) -> bool {
    fixtures.iter().all(|fixture| {
        !fixture.scenario.expected_surfaces.is_empty()
            && !fixture.scenario.evidence_refs.is_empty()
            && fixture.metrics.projection_faithfulness_bps
                >= fixture.thresholds.min_projection_faithfulness_bps
            && fixture.metrics.privacy_violation_count
                <= fixture.thresholds.max_privacy_violation_count
    })
}

fn push_missing(blocked_reasons: &mut Vec<String>, condition: bool, reason: &str) {
    if !condition {
        blocked_reasons.push(reason.to_string());
    }
}

fn calculate_memory_benchmark_baseline(
    fixtures: &[MemoryBenchmarkFixture],
) -> MemoryBenchmarkBaseline {
    if fixtures.is_empty() {
        return MemoryBenchmarkBaseline::default();
    }
    let len = fixtures.len() as u32;
    MemoryBenchmarkBaseline {
        accuracy_bps: average_bps(fixtures.iter().map(|fixture| fixture.metrics.accuracy_bps)),
        evidence_precision_bps: average_bps(
            fixtures
                .iter()
                .map(|fixture| fixture.metrics.evidence_precision_bps),
        ),
        projection_faithfulness_bps: average_bps(
            fixtures
                .iter()
                .map(|fixture| fixture.metrics.projection_faithfulness_bps),
        ),
        privacy_violation_count: fixtures
            .iter()
            .map(|fixture| fixture.metrics.privacy_violation_count)
            .sum(),
        stale_memory_false_positive_count: fixtures
            .iter()
            .map(|fixture| fixture.metrics.stale_memory_false_positive_count)
            .sum(),
        procedural_reuse_success_bps: average_bps(
            fixtures
                .iter()
                .map(|fixture| fixture.metrics.procedural_reuse_success_bps),
        ),
        soul_regression_count: fixtures
            .iter()
            .map(|fixture| fixture.metrics.soul_regression_count)
            .sum(),
        latency_ms: fixtures
            .iter()
            .map(|fixture| fixture.metrics.latency_ms)
            .sum::<u32>()
            / len,
        token_budget: fixtures
            .iter()
            .map(|fixture| fixture.metrics.token_budget)
            .sum::<u32>()
            / len,
        memory_bytes: fixtures
            .iter()
            .map(|fixture| fixture.metrics.memory_bytes)
            .sum::<u64>()
            / fixtures.len() as u64,
    }
}

fn average_bps(values: impl Iterator<Item = u16>) -> u16 {
    let mut total = 0u32;
    let mut count = 0u32;
    for value in values {
        total += u32::from(value);
        count += 1;
    }
    if count == 0 {
        return 0;
    }
    (total / count) as u16
}

fn push_min_bps_failure(
    failures: &mut Vec<MemoryBenchmarkFailure>,
    fixture: &MemoryBenchmarkFixture,
    stage: &str,
    got: u16,
    min: u16,
) {
    if got < min {
        failures.push(memory_benchmark_failure(
            fixture,
            stage,
            format!("expected at least {min}, got {got}"),
        ));
    }
}

fn push_max_u32_failure(
    failures: &mut Vec<MemoryBenchmarkFailure>,
    fixture: &MemoryBenchmarkFixture,
    stage: &str,
    got: u32,
    max: u32,
) {
    if got > max {
        failures.push(memory_benchmark_failure(
            fixture,
            stage,
            format!("expected at most {max}, got {got}"),
        ));
    }
}

fn memory_benchmark_failure(
    fixture: &MemoryBenchmarkFixture,
    stage: impl Into<String>,
    reason: impl Into<String>,
) -> MemoryBenchmarkFailure {
    MemoryBenchmarkFailure {
        fixture_id: fixture.fixture_id.clone(),
        class: fixture.class,
        mode: fixture.mode,
        profile: fixture.profile,
        stage: stage.into(),
        reason: reason.into(),
    }
}

fn memory_benchmark_semantic_failure(
    fixture: &MemoryBenchmarkFixture,
    dimension: Option<MemoryBenchmarkSemanticDimension>,
    stage: impl Into<String>,
    reason: impl Into<String>,
) -> MemoryBenchmarkSemanticFailure {
    MemoryBenchmarkSemanticFailure {
        fixture_id: fixture.fixture_id.clone(),
        dimension,
        stage: stage.into(),
        reason: reason.into(),
    }
}

fn memory_benchmark_suite_semantic_failure(
    dimension: Option<MemoryBenchmarkSemanticDimension>,
    stage: impl Into<String>,
    reason: impl Into<String>,
) -> MemoryBenchmarkSemanticFailure {
    MemoryBenchmarkSemanticFailure {
        fixture_id: "__suite__".to_string(),
        dimension,
        stage: stage.into(),
        reason: reason.into(),
    }
}

impl MemoryBenchmarkReport {
    fn with_missing_class_gate(mut self) -> Self {
        if !self.missing_classes.is_empty() {
            self.passed = false;
        }
        self
    }
}
