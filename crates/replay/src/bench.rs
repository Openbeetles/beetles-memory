use bm_core::memory::{
    run_persona_governance_replay_suite, run_recall_benchmark_suite, PersonaGovernanceReplayCase,
    RecallBenchmarkCase,
};
use bm_core::{Error, Result};
use bm_sdk::{
    ProfileId, MEMORY_PROJECTION_DELIVERY_DIGEST_SCHEMA_VERSION,
    MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::Command,
};

const P7_CONTRACT_VERSION: &str = "p7_recall_delivery_v1";
const P7_RUNNER_PROJECTION_DIGEST_OBSERVATION_SCHEMA_VERSION: &str =
    "p7_runner_projection_digest_observation_v1";
const P7_TRUSTED_SDK_BUILD_FINGERPRINT: &str = env!("BM_P7_TRUSTED_SDK_BUILD_FINGERPRINT");
const P7_SDK_BUILD_FINGERPRINT_CONTRACT: &str = "p7_sdk_build_inputs_sha256_v2";
const P7_SDK_BUILD_INPUTS: [&str; 6] = [
    "Cargo.toml",
    "Cargo.lock",
    "crates/core/Cargo.toml",
    "crates/core/src",
    "crates/sdk/Cargo.toml",
    "crates/sdk/src",
];
const P7_RUNNER_RELEASE_BINARY: &str = "runner/target/release/beetle-memory-external-bench-runner";
const P7_RUNNER_BUILD_FINGERPRINT_CONTRACT: &str = "p7_runner_build_inputs_sha256_v2";
const P7_RUNNER_BUILD_INPUTS: [&str; 4] = ["Cargo.toml", "Cargo.lock", "build.rs", "src"];
const P7_ABLATION_METHOD: &str = "sdk_eval_recall_off_run_v1";
const P7_REQUIRED_ABLATION_SLICES: [&str; 7] = [
    "facet_off",
    "rank_fusion_off",
    "coverage_selection_off",
    "delivery_relevance_fusion_off",
    "evidence_family_rotation_off",
    "render_capsule_off",
    "capsule_dedupe_off",
];

// Freezing this identity is an explicit release action after source, lockfile, SDK,
// and the release executable are final. SHA256 is a reproducible identity check, not
// a promise to resist a same-user local attacker who can rewrite all benchmark files.
const P7_TRUSTED_RUNNER_RELEASE: Option<P7TrustedRunnerRelease> = None;

#[derive(Clone, Copy)]
struct P7TrustedRunnerRelease {
    runner_build_fingerprint: &'static str,
    runner_lock_fingerprint: &'static str,
    executable_sha256: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7RunnerDiskIdentity {
    runner_build_fingerprint: String,
    runner_lock_fingerprint: String,
    executable_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct P7RunnerPreflightReport {
    pub run_id: String,
    pub sdk_build_fingerprint: String,
    pub runner_build_fingerprint: String,
    pub runner_lock_fingerprint: String,
    pub executable_sha256: String,
    pub build_profile: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct P7RunnerBuildIdentity {
    sdk_build_fingerprint: String,
    runner_build_fingerprint: String,
    runner_lock_fingerprint: String,
    executable_sha256: String,
    build_profile: String,
}

#[derive(Clone, Copy)]
struct P7TrustedDataset {
    suite: &'static str,
    file_name: &'static str,
    input_sha256: &'static str,
}

const P7_TRUSTED_DATASETS: [P7TrustedDataset; 4] = [
    P7TrustedDataset {
        suite: "locomo",
        file_name: "locomo10.json",
        input_sha256: "79fa87e90f04081343b8c8debecb80a9a6842b76a7aa537dc9fdf651ea698ff4",
    },
    P7TrustedDataset {
        suite: "longmemeval_oracle",
        file_name: "longmemeval_oracle.json",
        input_sha256: "821a2034d219ab45846873dd14c14f12cfe7776e73527a483f9dac095d38620c",
    },
    P7TrustedDataset {
        suite: "longmemeval_s_cleaned",
        file_name: "longmemeval_s_cleaned.json",
        input_sha256: "d6f21ea9d60a0d56f34a05b609c79c88a451d2ae03597821ea3d5a9678c3a442",
    },
    P7TrustedDataset {
        suite: "longmemeval_m_cleaned",
        file_name: "longmemeval_m_cleaned.json",
        input_sha256: "9d79e5524794a2e6900a3aa9cb7d9152c5a3e8319c9a87c25494ba1eacee495f",
    },
];

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
    pub run_id: String,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub shards: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_source_sha256: Option<String>,
    #[serde(skip)]
    operator_content_hash_verified: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p7_loss_ledger: Option<W4ExternalNoisyP7LossDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p7_production_delivery: Option<W4ExternalNoisyP7ProductionDeliveryDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p7_provenance: Option<W4ExternalNoisyP7Provenance>,
}

impl W4ExternalNoisyBenchmarkSummary {
    pub fn operator_content_hash_verified(&self) -> bool {
        self.operator_content_hash_verified
    }
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
    pub projection_selected_any_evidence_hit: usize,
    #[serde(default)]
    pub projection_selected_all_evidence_hit: usize,
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
    pub graph_manifest_contract_verified_questions: usize,
    pub graph_selected_dependency_chain_verified_questions: usize,
    pub graph_full_scope_closure_verified_questions: usize,
    pub graph_manifest_generation_present_questions: usize,
    pub graph_revision_present_questions: usize,
    pub graph_scope_digest_present_questions: usize,
    pub graph_maintenance_required_questions: usize,
    pub graph_incident_questions: usize,
    pub graph_read_path_mutation_delta: usize,
    #[serde(default)]
    pub facet_questions_with_index_report: usize,
    #[serde(default)]
    pub facet_index_used_questions: usize,
    #[serde(default)]
    pub facet_report_only_questions: usize,
    #[serde(default)]
    pub facet_fallback_full_scan_questions: usize,
    #[serde(default)]
    pub facet_source_candidate_count: usize,
    #[serde(default)]
    pub facet_matched_source_candidate_count: usize,
    #[serde(default)]
    pub facet_posting_key_lookup_count: usize,
    #[serde(default)]
    pub facet_manifest_matched_posting_count: usize,
    #[serde(default)]
    pub facet_posting_doc_read_count: usize,
    #[serde(default)]
    pub facet_owner_key_lookup_count: usize,
    #[serde(default)]
    pub facet_owner_doc_read_count: usize,
    #[serde(default)]
    pub facet_zero_posting_key_lookup_questions: usize,
    #[serde(default)]
    pub facet_clean_zero_hit_questions: usize,
    #[serde(default)]
    pub facet_manifest_integrity_verified_questions: usize,
    #[serde(default)]
    pub facet_manifest_integrity_failure_count: usize,
    #[serde(default)]
    pub facet_exact_match_count: usize,
    #[serde(default)]
    pub facet_expanded_match_count: usize,
    #[serde(default)]
    pub facet_failure_count: usize,
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
    pub delivery_contribution_proven_questions: usize,
    #[serde(default)]
    pub render_growth: usize,
    #[serde(default)]
    pub required_slice_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub report_available_slice_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub delivery_contribution_proven_slice_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub delivery_affected_candidate_occurrences: usize,
    #[serde(default)]
    pub selected_evidence_hit_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub rendered_evidence_hit_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub selected_all_hit_loss_count: BTreeMap<String, usize>,
    #[serde(default)]
    pub evidence_family_rotation_selected_all_hit_loss_count: BTreeMap<String, usize>,
    #[serde(default)]
    pub rendered_all_hit_loss_count: BTreeMap<String, usize>,
    #[serde(default)]
    pub expanded_candidate_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub selected_candidate_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub rendered_candidate_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub rendered_char_delta: BTreeMap<String, i64>,
    #[serde(default)]
    pub blocked_reason_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyP7LossDiagnostics {
    #[serde(default)]
    pub questions_with_loss_ledger: usize,
    #[serde(default)]
    pub expanded_hit_selected_miss_questions: usize,
    #[serde(default)]
    pub eval_selected_hit_rendered_miss_questions: usize,
    #[serde(default)]
    pub expanded_hit_selected_miss_evidence: usize,
    #[serde(default)]
    pub eval_selected_hit_rendered_miss_evidence: usize,
    #[serde(default)]
    pub eval_selected_hit_projection_selected_miss_questions: usize,
    #[serde(default)]
    pub eval_selected_hit_projection_selected_miss_evidence: usize,
    #[serde(default)]
    pub selected_hit_final_rendered_miss_questions: usize,
    #[serde(default)]
    pub selected_hit_final_rendered_miss_evidence: usize,
    #[serde(default)]
    pub eval_truncated_count: usize,
    #[serde(default)]
    pub eval_blocked_reason_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyP7ProductionDeliveryDiagnostics {
    #[serde(default)]
    pub questions_with_delivery_report: usize,
    #[serde(default)]
    pub eval_selected_matches_delivery_questions: usize,
    #[serde(default)]
    pub eval_rendered_matches_delivery_questions: usize,
    #[serde(default)]
    pub projection_selected_sources_proven_questions: usize,
    #[serde(default)]
    pub projection_delivery_proof_questions: usize,
    #[serde(default)]
    pub final_projection_integrity_questions: usize,
    #[serde(default)]
    pub final_projection_integrity_passed_questions: usize,
    #[serde(default)]
    pub final_projection_raw_private_violation_count: usize,
    #[serde(default)]
    pub final_projection_blocked_source_count: usize,
    #[serde(default)]
    pub final_projection_redacted_source_count: usize,
    #[serde(default)]
    pub schema_version_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub render_growth: usize,
    #[serde(default)]
    pub privacy_leak_count: usize,
    #[serde(default)]
    pub cross_subject_leak_count: usize,
    #[serde(default)]
    pub raw_soul_private_material_count: usize,
    #[serde(default)]
    pub blocked_reason_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub delivery_drop_reason_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyP7ShardDigest {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub shard: String,
    #[serde(default)]
    pub summary_sha256: String,
    #[serde(default)]
    pub detail_sha256: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyP7Provenance {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub contract_version: String,
    #[serde(default)]
    pub sdk_report_schema_version: u32,
    #[serde(default)]
    pub sdk_build_fingerprint: String,
    #[serde(default)]
    pub runner_build_fingerprint: String,
    #[serde(default)]
    pub runner_lock_fingerprint: String,
    #[serde(default)]
    pub executable_sha256: String,
    #[serde(default)]
    pub build_profile: String,
    #[serde(default)]
    pub input_sha256: String,
    #[serde(default)]
    pub merged_detail_sha256: String,
    #[serde(default)]
    pub ordered_shard_digest_manifest: Vec<W4ExternalNoisyP7ShardDigest>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisySuiteReport {
    pub suite: String,
    pub run_id: String,
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
    pub p7_loss_ledger: Option<W4ExternalNoisyP7LossDiagnostics>,
    pub p7_production_delivery: Option<W4ExternalNoisyP7ProductionDeliveryDiagnostics>,
    pub p7_provenance: Option<W4ExternalNoisyP7Provenance>,
    pub stage_attributed_improvement: bool,
    pub index_effect_proven: bool,
    pub facet_ablation_effect_proven: bool,
    pub facet_ablation_no_render_growth: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct W4ExternalNoisyWallReport {
    pub release_gate_passed: bool,
    pub run_id: Option<String>,
    pub cohort_valid: bool,
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
    pub p7_loss_ledger_attached: bool,
    pub p7_selection_loss_reduced: bool,
    pub p7_render_loss_reduced: bool,
    pub p7_ablation_effect_proven: bool,
    pub p7_no_render_growth: bool,
    pub p7_index_no_full_scan: bool,
    pub p7_no_privacy_or_soul_regression: bool,
    pub p7_no_p6_regression: bool,
    pub p7_production_delivery_proven: bool,
    pub p7_provenance_valid: bool,
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
    let required_suites_covered = summaries.len() == required_suites.len()
        && required_suites.iter().all(|suite| {
            summaries
                .iter()
                .filter(|summary| summary.suite == *suite)
                .count()
                == 1
        });
    let cohort_run_ids = summaries
        .iter()
        .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
        .filter_map(|summary| {
            summary.p7_provenance.as_ref().and_then(|provenance| {
                (p7_valid_run_id(&summary.run_id) && provenance.run_id == summary.run_id)
                    .then(|| summary.run_id.clone())
            })
        })
        .collect::<BTreeSet<_>>();
    let cohort_valid = required_suites_covered
        && cohort_run_ids.len() == 1
        && summaries.iter().all(|summary| {
            summary
                .p7_provenance
                .as_ref()
                .is_some_and(|provenance| provenance.run_id == summary.run_id)
        });
    let run_id = cohort_valid
        .then(|| cohort_run_ids.iter().next().cloned())
        .flatten();
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
    let index_no_full_scan = index_diagnostics_attached
        && required_suites_covered
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
    let facet_ablation_no_render_growth = facet_ablation_attached
        && required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| {
                summary
                    .facet_ablation
                    .as_ref()
                    .is_some_and(|diagnostics| diagnostics.render_growth == 0)
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
    let p7_loss_ledger_attached = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(p7_loss_ledger_covers_summary);
    let p7_selection_loss_reduced = noisy_suites
        .iter()
        .all(|suite| p7_suite_quality_threshold_met(summaries, suite, true));
    let p7_render_loss_reduced = noisy_suites
        .iter()
        .all(|suite| p7_suite_quality_threshold_met(summaries, suite, false));
    let p7_ablation_effect_proven = noisy_suites
        .iter()
        .all(|suite| p7_ablation_proves_suite_effect(summaries, suite));
    let p7_no_render_growth = facet_ablation_no_render_growth
        && required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(|summary| {
                summary
                    .p7_production_delivery
                    .as_ref()
                    .is_some_and(|diagnostics| diagnostics.render_growth == 0)
            });
    let p7_index_no_full_scan = index_diagnostics_attached && index_no_full_scan;
    let p7_no_privacy_or_soul_regression = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(p7_production_delivery_has_no_privacy_or_soul_regression);
    let p7_no_p6_regression = shards_valid
        && noisy_improvement_proven
        && stage_attributed_improvement_proven
        && index_effect_proven
        && facet_ablation_effect_proven
        && facet_ablation_no_render_growth;
    let p7_production_delivery_proven = required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(p7_production_delivery_covers_summary);
    let p7_provenance_valid = cohort_valid
        && required_suites_covered
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .all(p7_provenance_valid_for_summary)
        && summaries
            .iter()
            .filter(|summary| required_suites.iter().any(|suite| summary.suite == *suite))
            .filter_map(|summary| summary.p7_provenance.as_ref())
            .map(|provenance| {
                (
                    provenance.run_id.clone(),
                    provenance.contract_version.clone(),
                    provenance.sdk_report_schema_version,
                    provenance.sdk_build_fingerprint.clone(),
                    provenance.runner_build_fingerprint.clone(),
                    provenance.runner_lock_fingerprint.clone(),
                    provenance.executable_sha256.clone(),
                    provenance.build_profile.clone(),
                )
            })
            .collect::<BTreeSet<_>>()
            .len()
            == 1;

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
    push_missing(
        &mut blocked_reasons,
        p7_loss_ledger_attached,
        "p7_loss_ledger_missing",
    );
    push_missing(
        &mut blocked_reasons,
        p7_selection_loss_reduced,
        "p7_selection_loss_not_reduced",
    );
    push_missing(
        &mut blocked_reasons,
        p7_render_loss_reduced,
        "p7_render_loss_not_reduced",
    );
    push_missing(
        &mut blocked_reasons,
        p7_ablation_effect_proven,
        "p7_ablation_effect_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        p7_no_render_growth,
        "p7_render_growth_detected",
    );
    push_missing(
        &mut blocked_reasons,
        p7_index_no_full_scan,
        "p7_index_full_scan_detected",
    );
    push_missing(
        &mut blocked_reasons,
        p7_no_privacy_or_soul_regression,
        "p7_privacy_or_soul_regression",
    );
    push_missing(
        &mut blocked_reasons,
        p7_no_p6_regression,
        "p7_p6_regression",
    );
    push_missing(
        &mut blocked_reasons,
        p7_production_delivery_proven,
        "p7_production_delivery_not_proven",
    );
    push_missing(
        &mut blocked_reasons,
        p7_provenance_valid,
        "p7_provenance_invalid",
    );
    push_missing(&mut blocked_reasons, cohort_valid, "p7_run_cohort_invalid");
    blocked_reasons.sort();
    blocked_reasons.dedup();

    W4ExternalNoisyWallReport {
        release_gate_passed: blocked_reasons.is_empty(),
        run_id,
        cohort_valid,
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
        p7_loss_ledger_attached,
        p7_selection_loss_reduced,
        p7_render_loss_reduced,
        p7_ablation_effect_proven,
        p7_no_render_growth,
        p7_index_no_full_scan,
        p7_no_privacy_or_soul_regression,
        p7_no_p6_regression,
        p7_production_delivery_proven,
        p7_provenance_valid,
        suite_reports,
        blocked_reasons,
    }
}

pub fn w4_external_noisy_summary_with_provenance(
    summary_json: &str,
) -> Result<W4ExternalNoisyBenchmarkSummary> {
    let mut summary = serde_json::from_str::<W4ExternalNoisyBenchmarkSummary>(summary_json)
        .map_err(|source| Error::Other {
            source: Box::new(source),
            stage: "w4_external_noisy_summary_json",
        })?;
    summary.summary_sha256 = Some(format!("{:x}", Sha256::digest(summary_json.as_bytes())));
    summary.runner_source_sha256 = summary
        .p7_provenance
        .as_ref()
        .map(|provenance| provenance.runner_build_fingerprint.clone());
    Ok(summary)
}

pub fn verify_w4_external_noisy_summary_files(
    summary: &mut W4ExternalNoisyBenchmarkSummary,
    merged_summary_path: &Path,
) -> Result<()> {
    summary.operator_content_hash_verified = false;
    let Some(provenance) = summary.p7_provenance.as_ref() else {
        return Ok(());
    };
    if summary.run_id != provenance.run_id {
        return Err(p7_provenance_error(
            "merged summary and provenance run_id mismatch",
        ));
    }
    let expected_merged_name = format!("{}.merged.summary.json", summary.suite);
    if merged_summary_path
        .file_name()
        .and_then(|name| name.to_str())
        != Some(expected_merged_name.as_str())
    {
        return Err(p7_provenance_error("unexpected merged summary file name"));
    }
    let benchmark_root = p7_benchmark_root_for_run(merged_summary_path, &summary.run_id)?;
    let trusted_dataset = p7_trusted_dataset(&summary.suite)
        .ok_or_else(|| p7_provenance_error("unknown release suite"))?;
    let runner_preflight = preflight_p7_runner_release(&benchmark_root, &summary.run_id)?;
    let runner_disk_identity = P7RunnerDiskIdentity {
        runner_build_fingerprint: runner_preflight.runner_build_fingerprint,
        runner_lock_fingerprint: runner_preflight.runner_lock_fingerprint,
        executable_sha256: runner_preflight.executable_sha256,
    };
    validate_p7_runner_disk_provenance(provenance, &runner_disk_identity)?;
    let trusted_runner = P7_TRUSTED_RUNNER_RELEASE
        .ok_or_else(|| p7_provenance_error("trusted P7 runner release is not frozen"))?;
    validate_p7_release_identity(
        provenance,
        trusted_dataset,
        trusted_runner,
        &runner_disk_identity.executable_sha256,
    )?;
    let parent = merged_summary_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let expectation = w4_external_suite_expectation(&summary.suite)
        .ok_or_else(|| p7_provenance_error("unknown release suite"))?;
    if expectation.shard_count != summary.shards.len()
        || provenance.ordered_shard_digest_manifest.len() != summary.shards.len()
    {
        return Err(p7_provenance_error("release shard count mismatch"));
    }
    let dataset_path = benchmark_root.join("data").join(trusted_dataset.file_name);
    let expected_dataset =
        load_p7_dataset_expectation(&dataset_path, trusted_dataset, expectation.shard_count)?;
    if expected_dataset.input_sha256 != provenance.input_sha256 {
        return Err(p7_provenance_error("input dataset digest mismatch"));
    }

    let mut merged_detail_hasher = Sha256::new();
    let mut additive_aggregate = serde_json::Map::new();
    let mut recomputed_aggregate = P7DetailAggregate::default();
    let mut seen_question_ids = BTreeSet::new();
    let mut seen_identities = BTreeSet::new();
    for (shard_index, (shard_name, digest)) in summary
        .shards
        .iter()
        .zip(&provenance.ordered_shard_digest_manifest)
        .enumerate()
    {
        let expected_shard_name = format!(
            "{}.shard-{shard_index}-of-{}.summary.json",
            summary.suite, expectation.shard_count
        );
        let shard_path_fragment = Path::new(&digest.shard);
        if shard_name != &expected_shard_name
            || shard_name != &digest.shard
            || digest.run_id != summary.run_id
            || shard_path_fragment
                .file_name()
                .and_then(|name| name.to_str())
                != Some(digest.shard.as_str())
        {
            return Err(p7_provenance_error("unsafe or mismatched shard path"));
        }
        let shard_path = parent.join(shard_path_fragment);
        let shard_bytes = fs::read(&shard_path).map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_read_shard_summary",
        })?;
        if format!("{:x}", Sha256::digest(&shard_bytes)) != digest.summary_sha256 {
            return Err(p7_provenance_error("shard summary digest mismatch"));
        }
        let shard_json =
            serde_json::from_slice::<serde_json::Value>(&shard_bytes).map_err(|source| {
                Error::Other {
                    source: Box::new(source),
                    stage: "p7_provenance_parse_shard_summary",
                }
            })?;
        if shard_json.get("suite").and_then(serde_json::Value::as_str)
            != Some(summary.suite.as_str())
            || shard_json
                .get("shard_index")
                .and_then(serde_json::Value::as_u64)
                != Some(shard_index as u64)
            || shard_json
                .get("shard_total")
                .and_then(serde_json::Value::as_u64)
                != Some(expectation.shard_count as u64)
            || shard_json
                .get("completed")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            || shard_json.get("run_id").and_then(serde_json::Value::as_str)
                != Some(summary.run_id.as_str())
        {
            return Err(p7_provenance_error("shard release coordinates mismatch"));
        }
        validate_p7_release_shard_full_run(&shard_json)?;
        if shard_json
            .get("input_sha256")
            .and_then(serde_json::Value::as_str)
            != Some(trusted_dataset.input_sha256)
        {
            return Err(p7_provenance_error("shard input dataset digest mismatch"));
        }
        accumulate_p7_shard_summary(&mut additive_aggregate, &shard_json)?;
        let producer = shard_json
            .get("producer")
            .ok_or_else(|| p7_provenance_error("shard summary is missing producer provenance"))?;
        if producer.get("run_id").and_then(serde_json::Value::as_str)
            != Some(summary.run_id.as_str())
            || producer
                .get("contract_version")
                .and_then(serde_json::Value::as_str)
                != Some(provenance.contract_version.as_str())
            || producer
                .get("sdk_report_schema_version")
                .and_then(serde_json::Value::as_u64)
                != Some(u64::from(provenance.sdk_report_schema_version))
            || producer
                .get("sdk_build_fingerprint")
                .and_then(serde_json::Value::as_str)
                != Some(provenance.sdk_build_fingerprint.as_str())
            || producer
                .get("runner_build_fingerprint")
                .and_then(serde_json::Value::as_str)
                != Some(provenance.runner_build_fingerprint.as_str())
            || producer
                .get("runner_lock_fingerprint")
                .and_then(serde_json::Value::as_str)
                != Some(provenance.runner_lock_fingerprint.as_str())
            || producer
                .get("executable_sha256")
                .and_then(serde_json::Value::as_str)
                != Some(provenance.executable_sha256.as_str())
            || producer
                .get("build_profile")
                .and_then(serde_json::Value::as_str)
                != Some(provenance.build_profile.as_str())
            || producer
                .get("input_sha256")
                .and_then(serde_json::Value::as_str)
                != Some(provenance.input_sha256.as_str())
            || producer
                .get("detail_sha256")
                .and_then(serde_json::Value::as_str)
                != Some(digest.detail_sha256.as_str())
        {
            return Err(p7_provenance_error("shard producer provenance mismatch"));
        }

        let detail_name = digest
            .shard
            .strip_suffix(".summary.json")
            .map(|prefix| format!("{prefix}.jsonl"))
            .ok_or_else(|| p7_provenance_error("invalid shard summary file name"))?;
        let shard_recomputed = validate_p7_detail_file(
            &parent.join(detail_name),
            &digest.detail_sha256,
            P7DetailValidationContext {
                suite: &summary.suite,
                run_id: &summary.run_id,
                expected_questions: &expected_dataset.questions_by_shard[shard_index],
                expected_samples: expected_dataset.samples_by_shard[shard_index],
            },
            &mut seen_question_ids,
            &mut seen_identities,
        )?;
        validate_p7_shard_against_detail(&shard_json, &shard_recomputed)?;
        recomputed_aggregate.add_assign(&shard_recomputed)?;
        merged_detail_hasher.update(digest.detail_sha256.as_bytes());
        merged_detail_hasher.update([0]);
    }
    if format!("{:x}", merged_detail_hasher.finalize()) != provenance.merged_detail_sha256 {
        return Err(p7_provenance_error("merged detail digest mismatch"));
    }
    validate_p7_additive_merge(summary, &additive_aggregate)?;
    validate_p7_summary_against_detail(summary, &recomputed_aggregate)?;
    summary.operator_content_hash_verified = true;
    Ok(())
}

fn validate_p7_release_identity(
    provenance: &W4ExternalNoisyP7Provenance,
    dataset: P7TrustedDataset,
    runner: P7TrustedRunnerRelease,
    actual_executable_sha256: &str,
) -> Result<()> {
    if provenance.contract_version != P7_CONTRACT_VERSION
        || provenance.sdk_report_schema_version != MEMORY_RECALL_DELIVERY_SCHEMA_VERSION
        || provenance.sdk_build_fingerprint != P7_TRUSTED_SDK_BUILD_FINGERPRINT
        || provenance.runner_build_fingerprint != runner.runner_build_fingerprint
        || provenance.runner_lock_fingerprint != runner.runner_lock_fingerprint
        || provenance.executable_sha256 != runner.executable_sha256
        || actual_executable_sha256 != runner.executable_sha256
        || !is_sha256(actual_executable_sha256)
        || provenance.build_profile != "release"
        || provenance.input_sha256 != dataset.input_sha256
        || !is_sha256(&provenance.merged_detail_sha256)
    {
        return Err(p7_provenance_error("untrusted P7 release provenance"));
    }
    Ok(())
}

fn validate_p7_runner_disk_provenance(
    provenance: &W4ExternalNoisyP7Provenance,
    disk: &P7RunnerDiskIdentity,
) -> Result<()> {
    if provenance.runner_build_fingerprint != disk.runner_build_fingerprint
        || provenance.runner_lock_fingerprint != disk.runner_lock_fingerprint
        || provenance.executable_sha256 != disk.executable_sha256
        || !is_sha256(&disk.runner_build_fingerprint)
        || !is_sha256(&disk.runner_lock_fingerprint)
        || !is_sha256(&disk.executable_sha256)
    {
        return Err(p7_provenance_error(
            "runner source, lock, or executable differs from producer provenance",
        ));
    }
    Ok(())
}

fn validate_p7_release_shard_full_run(shard: &serde_json::Value) -> Result<()> {
    for field in ["limit", "question_limit", "question_index"] {
        if !shard.get(field).is_some_and(serde_json::Value::is_null) {
            return Err(p7_provenance_error(
                "release shard contains a diagnostic question filter",
            ));
        }
    }
    Ok(())
}

pub fn preflight_p7_runner_release(
    benchmark_root: &Path,
    run_id: &str,
) -> Result<P7RunnerPreflightReport> {
    if !p7_valid_run_id(run_id) {
        return Err(p7_preflight_error("invalid or missing P7 run_id"));
    }
    let trusted = P7_TRUSTED_RUNNER_RELEASE
        .ok_or_else(|| p7_preflight_error("trusted P7 runner release is not frozen"))?;
    let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| p7_preflight_error("bm-replay is not under the SDK workspace root"))?;
    preflight_p7_runner_release_with_trusted(benchmark_root, sdk_root, trusted, run_id)
}

pub fn validate_p7_runner_preflight_report(
    benchmark_root: &Path,
    run_id: &str,
    report: &P7RunnerPreflightReport,
) -> Result<()> {
    if report.run_id != run_id {
        return Err(p7_preflight_error(
            "P7 preflight report run_id differs from cohort",
        ));
    }
    let fresh = preflight_p7_runner_release(benchmark_root, run_id)?;
    if report != &fresh {
        return Err(p7_preflight_error(
            "P7 preflight report differs from current trusted preflight",
        ));
    }
    Ok(())
}

fn preflight_p7_runner_release_with_trusted(
    benchmark_root: &Path,
    sdk_root: &Path,
    trusted: P7TrustedRunnerRelease,
    run_id: &str,
) -> Result<P7RunnerPreflightReport> {
    let sdk_inputs = p7_fingerprint_inputs(sdk_root, &P7_SDK_BUILD_INPUTS)?;
    let sdk_build_fingerprint = p7_fingerprint_files_with_contract(
        sdk_root,
        &sdk_inputs,
        P7_SDK_BUILD_FINGERPRINT_CONTRACT,
    )?;
    let disk = p7_runner_disk_identity_at_root(benchmark_root)?;
    if sdk_build_fingerprint != P7_TRUSTED_SDK_BUILD_FINGERPRINT
        || disk.runner_build_fingerprint != trusted.runner_build_fingerprint
        || disk.runner_lock_fingerprint != trusted.runner_lock_fingerprint
        || disk.executable_sha256 != trusted.executable_sha256
        || !is_sha256(&sdk_build_fingerprint)
        || !is_sha256(&disk.runner_build_fingerprint)
        || !is_sha256(&disk.runner_lock_fingerprint)
        || !is_sha256(&disk.executable_sha256)
    {
        return Err(p7_preflight_error(
            "P7 SDK or frozen runner disk identity drifted",
        ));
    }
    let executable = benchmark_root.join(P7_RUNNER_RELEASE_BINARY);
    let output = Command::new(&executable)
        .arg("--print-build-identity")
        .output()
        .map_err(|source| Error::Io {
            source,
            stage: "p7_runner_preflight_execute_identity",
        })?;
    if !output.status.success() {
        return Err(p7_preflight_error(
            "frozen runner rejected --print-build-identity",
        ));
    }
    let embedded =
        serde_json::from_slice::<P7RunnerBuildIdentity>(&output.stdout).map_err(|source| {
            Error::Other {
                source: Box::new(source),
                stage: "p7_runner_preflight_parse_identity",
            }
        })?;
    if embedded.sdk_build_fingerprint != sdk_build_fingerprint
        || embedded.runner_build_fingerprint != disk.runner_build_fingerprint
        || embedded.runner_lock_fingerprint != disk.runner_lock_fingerprint
        || embedded.executable_sha256 != disk.executable_sha256
        || embedded.build_profile != "release"
        || !is_sha256(&embedded.sdk_build_fingerprint)
        || !is_sha256(&embedded.runner_build_fingerprint)
        || !is_sha256(&embedded.runner_lock_fingerprint)
        || !is_sha256(&embedded.executable_sha256)
    {
        return Err(p7_preflight_error(
            "P7 SDK, runner source, lock, profile, or executable identity drifted",
        ));
    }
    Ok(P7RunnerPreflightReport {
        run_id: run_id.to_string(),
        sdk_build_fingerprint,
        runner_build_fingerprint: disk.runner_build_fingerprint,
        runner_lock_fingerprint: disk.runner_lock_fingerprint,
        executable_sha256: disk.executable_sha256,
        build_profile: embedded.build_profile,
    })
}

fn p7_benchmark_root_for_run(merged_summary_path: &Path, run_id: &str) -> Result<PathBuf> {
    if !p7_valid_run_id(run_id) {
        return Err(p7_provenance_error("invalid or missing P7 run_id"));
    }
    let run_dir = merged_summary_path
        .parent()
        .ok_or_else(|| p7_provenance_error("merged summary has no run directory"))?;
    let runs_dir = run_dir
        .parent()
        .ok_or_else(|| p7_provenance_error("P7 run directory has no runs parent"))?;
    let results_dir = runs_dir
        .parent()
        .ok_or_else(|| p7_provenance_error("P7 runs directory has no results parent"))?;
    if run_dir.file_name().and_then(|name| name.to_str()) != Some(run_id)
        || runs_dir.file_name().and_then(|name| name.to_str()) != Some("runs")
        || results_dir.file_name().and_then(|name| name.to_str()) != Some("results")
    {
        return Err(p7_provenance_error(
            "merged summary must be under results/runs/<run-id>",
        ));
    }
    results_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| p7_provenance_error("results directory has no benchmark root"))
}

fn p7_valid_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id != "."
        && run_id != ".."
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn p7_runner_disk_identity_at_root(benchmark_root: &Path) -> Result<P7RunnerDiskIdentity> {
    let runner_root = benchmark_root.join("runner");
    let runner_inputs = p7_fingerprint_inputs(&runner_root, &P7_RUNNER_BUILD_INPUTS)?;
    Ok(P7RunnerDiskIdentity {
        runner_build_fingerprint: p7_fingerprint_files_with_contract(
            &runner_root,
            &runner_inputs,
            P7_RUNNER_BUILD_FINGERPRINT_CONTRACT,
        )?,
        runner_lock_fingerprint: p7_sha256_file(&runner_root.join("Cargo.lock"))?,
        executable_sha256: p7_sha256_file(&benchmark_root.join(P7_RUNNER_RELEASE_BINARY))?,
    })
}

fn p7_fingerprint_inputs(root: &Path, relatives: &[&str]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for relative in relatives {
        p7_collect_regular_files(&root.join(relative), &mut files)?;
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn p7_collect_regular_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries = fs::read_dir(path)
        .map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_read_runner_build_input",
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_read_runner_build_input",
        })?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let child = entry.path();
        if child.is_dir() {
            p7_collect_regular_files(&child, files)?;
        } else if child.is_file() {
            files.push(child);
        }
    }
    Ok(())
}

fn p7_fingerprint_files_with_contract(
    root: &Path,
    files: &[PathBuf],
    contract: &str,
) -> Result<String> {
    let mut hasher = Sha256::new();
    p7_hash_fingerprint_field(&mut hasher, contract.as_bytes())?;
    let file_count = u64::try_from(files.len())
        .map_err(|_| p7_provenance_error("build input count overflow"))?;
    hasher.update(file_count.to_le_bytes());
    for file in files {
        let relative = file
            .strip_prefix(root)
            .map_err(|_| p7_provenance_error("build input is outside the fingerprint root"))?;
        p7_hash_fingerprint_field(&mut hasher, relative.to_string_lossy().as_bytes())?;
        let content = fs::read(file).map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_hash_runner_build_input",
        })?;
        p7_hash_fingerprint_field(&mut hasher, &content)?;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn p7_hash_fingerprint_field(hasher: &mut Sha256, value: &[u8]) -> Result<()> {
    let len = u64::try_from(value.len())
        .map_err(|_| p7_provenance_error("runner build fingerprint field overflow"))?;
    hasher.update(len.to_le_bytes());
    hasher.update(value);
    Ok(())
}

fn p7_sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path).map_err(|source| Error::Io {
        source,
        stage: "p7_provenance_read_runner_release_binary",
    })?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_hash_runner_release_binary",
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7ExpectedQuestionIdentity {
    case_id: String,
    dataset_index: usize,
    question_index: usize,
    question_id: String,
    question: String,
    gold_sources: Vec<String>,
}

struct P7ExpectedDataset {
    input_sha256: String,
    samples_by_shard: Vec<usize>,
    questions_by_shard: Vec<Vec<P7ExpectedQuestionIdentity>>,
}

fn load_p7_dataset_expectation(
    path: &Path,
    dataset: P7TrustedDataset,
    shard_count: usize,
) -> Result<P7ExpectedDataset> {
    let file = File::open(path).map_err(|source| Error::Io {
        source,
        stage: "p7_provenance_open_input_dataset",
    })?;
    let mut stream = P7JsonArrayObjectStream::new(file);
    let mut samples_by_shard = vec![0_usize; shard_count];
    let mut questions_by_shard = vec![Vec::new(); shard_count];
    let mut seen_question_ids = BTreeSet::new();
    let mut dataset_index = 0_usize;
    while let Some(item) = stream.next_object()? {
        let shard_index = dataset_index % shard_count;
        samples_by_shard[shard_index] = samples_by_shard[shard_index].saturating_add(1);
        let questions = if dataset.suite == "locomo" {
            p7_locomo_expected_questions(&item, dataset_index)?
        } else {
            vec![p7_longmemeval_expected_question(&item, dataset_index)?]
        };
        for question in questions {
            if !seen_question_ids.insert(question.question_id.clone()) {
                return Err(p7_provenance_error("dataset question_id is not unique"));
            }
            questions_by_shard[shard_index].push(question);
        }
        dataset_index = dataset_index.saturating_add(1);
    }
    let input_sha256 = stream.finish_sha256()?;
    if input_sha256 != dataset.input_sha256 {
        return Err(p7_provenance_error("trusted input dataset bytes changed"));
    }
    Ok(P7ExpectedDataset {
        input_sha256,
        samples_by_shard,
        questions_by_shard,
    })
}

fn p7_locomo_expected_questions(
    item: &serde_json::Value,
    dataset_index: usize,
) -> Result<Vec<P7ExpectedQuestionIdentity>> {
    let case_id = p7_required_str(item, "sample_id", "LoCoMo sample_id missing")?;
    let questions = item
        .get("qa")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| p7_provenance_error("LoCoMo qa array missing"))?;
    questions
        .iter()
        .enumerate()
        .map(|(question_index, question)| {
            Ok(P7ExpectedQuestionIdentity {
                case_id: case_id.to_string(),
                dataset_index,
                question_index,
                question_id: format!("{case_id}__q{question_index}"),
                question: p7_required_str(question, "question", "LoCoMo question missing")?
                    .to_string(),
                gold_sources: p7_string_array(
                    question
                        .get("evidence")
                        .ok_or_else(|| p7_provenance_error("LoCoMo evidence missing"))?,
                    "LoCoMo evidence must be a string array",
                )?,
            })
        })
        .collect()
}

fn p7_longmemeval_expected_question(
    item: &serde_json::Value,
    dataset_index: usize,
) -> Result<P7ExpectedQuestionIdentity> {
    let question_id = p7_required_str(item, "question_id", "LongMemEval question_id missing")?;
    Ok(P7ExpectedQuestionIdentity {
        case_id: question_id.to_string(),
        dataset_index,
        question_index: 0,
        question_id: question_id.to_string(),
        question: p7_required_str(item, "question", "LongMemEval question missing")?.to_string(),
        gold_sources: p7_string_array(
            item.get("answer_session_ids")
                .ok_or_else(|| p7_provenance_error("LongMemEval gold sources missing"))?,
            "LongMemEval gold sources must be a string array",
        )?,
    })
}

struct P7HashingReader<R> {
    inner: R,
    hasher: Sha256,
}

impl<R> P7HashingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    fn finish_sha256(self) -> String {
        format!("{:x}", self.hasher.finalize())
    }
}

impl<R: Read> Read for P7HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

struct P7JsonArrayObjectStream {
    reader: BufReader<P7HashingReader<File>>,
    started: bool,
    finished: bool,
}

impl P7JsonArrayObjectStream {
    fn new(file: File) -> Self {
        Self {
            reader: BufReader::new(P7HashingReader::new(file)),
            started: false,
            finished: false,
        }
    }

    fn next_object(&mut self) -> Result<Option<serde_json::Value>> {
        if self.finished {
            return Ok(None);
        }
        let mut byte = [0_u8; 1];
        if !self.started {
            loop {
                if self.read_byte(&mut byte)? == 0 {
                    return Err(p7_provenance_error("input dataset is empty"));
                }
                if byte[0].is_ascii_whitespace() {
                    continue;
                }
                if byte[0] != b'[' {
                    return Err(p7_provenance_error("input dataset root is not an array"));
                }
                self.started = true;
                break;
            }
        }
        loop {
            if self.read_byte(&mut byte)? == 0 {
                return Err(p7_provenance_error("unexpected input dataset EOF"));
            }
            match byte[0] {
                b',' | b' ' | b'\n' | b'\r' | b'\t' => continue,
                b']' => {
                    self.finished = true;
                    return Ok(None);
                }
                b'{' => break,
                _ => return Err(p7_provenance_error("input dataset item is not an object")),
            }
        }
        let mut bytes = vec![b'{'];
        let mut depth = 1_i32;
        let mut in_string = false;
        let mut escaped = false;
        while depth > 0 {
            if self.read_byte(&mut byte)? == 0 {
                return Err(p7_provenance_error("unexpected EOF inside dataset object"));
            }
            let current = byte[0];
            bytes.push(current);
            if in_string {
                if escaped {
                    escaped = false;
                } else if current == b'\\' {
                    escaped = true;
                } else if current == b'"' {
                    in_string = false;
                }
                continue;
            }
            match current {
                b'"' => in_string = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth -= 1,
                _ => {}
            }
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| Error::Other {
                source: Box::new(source),
                stage: "p7_provenance_parse_input_dataset",
            })
    }

    fn finish_sha256(mut self) -> Result<String> {
        if !self.finished {
            return Err(p7_provenance_error("input dataset was not fully consumed"));
        }
        let mut trailing = Vec::new();
        self.reader
            .read_to_end(&mut trailing)
            .map_err(|source| Error::Io {
                source,
                stage: "p7_provenance_hash_input_dataset",
            })?;
        if trailing.iter().any(|byte| !byte.is_ascii_whitespace()) {
            return Err(p7_provenance_error("input dataset has trailing content"));
        }
        Ok(self.reader.into_inner().finish_sha256())
    }

    fn read_byte(&mut self, byte: &mut [u8; 1]) -> Result<usize> {
        self.reader.read(byte).map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_stream_input_dataset",
        })
    }
}

const P7_ADDITIVE_SUMMARY_FIELDS: &[&str] = &[
    "samples",
    "questions",
    "evidence_questions",
    "any_evidence_hit",
    "all_evidence_hit",
    "write_errors",
    "recall_errors",
    "stage_hit_counts",
    "index_diagnostics",
    "w4_1_diagnostics",
    "facet_ablation",
    "p7_loss_ledger",
    "p7_production_delivery",
];

fn accumulate_p7_shard_summary(
    aggregate: &mut serde_json::Map<String, serde_json::Value>,
    shard: &serde_json::Value,
) -> Result<()> {
    for field in P7_ADDITIVE_SUMMARY_FIELDS {
        let value = shard
            .get(*field)
            .ok_or_else(|| p7_provenance_error("shard additive field missing"))?;
        let target = aggregate
            .entry((*field).to_string())
            .or_insert(serde_json::Value::Null);
        add_p7_additive_json(target, value)?;
    }
    Ok(())
}

fn add_p7_additive_json(target: &mut serde_json::Value, value: &serde_json::Value) -> Result<()> {
    if target.is_null() {
        *target = value.clone();
        return Ok(());
    }
    match (target, value) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            let left_value = left
                .as_i64()
                .ok_or_else(|| p7_provenance_error("non-integer additive value"))?;
            let right_value = right
                .as_i64()
                .ok_or_else(|| p7_provenance_error("non-integer additive value"))?;
            *left = (left_value.saturating_add(right_value)).into();
            Ok(())
        }
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            for (key, right_value) in right {
                let left_value = left.entry(key.clone()).or_insert(serde_json::Value::Null);
                add_p7_additive_json(left_value, right_value)?;
            }
            Ok(())
        }
        _ => Err(p7_provenance_error("non-additive shard summary field")),
    }
}

fn validate_p7_additive_merge(
    summary: &W4ExternalNoisyBenchmarkSummary,
    aggregate: &serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    let merged = serde_json::to_value(summary).map_err(|source| Error::Other {
        source: Box::new(source),
        stage: "p7_provenance_serialize_merged_summary",
    })?;
    for field in P7_ADDITIVE_SUMMARY_FIELDS {
        if merged.get(*field) != aggregate.get(*field) {
            return Err(p7_provenance_error(
                "merged summary is not an exact shard merge",
            ));
        }
    }
    Ok(())
}

#[derive(Default)]
struct P7DetailAggregate {
    samples: usize,
    questions: usize,
    evidence_questions: usize,
    any_evidence_hit: usize,
    all_evidence_hit: usize,
    write_errors: usize,
    recall_errors: usize,
    stage_hit_counts: W4ExternalNoisyStageHitCounts,
    index_diagnostics: W4ExternalNoisyIndexDiagnostics,
    w4_1_diagnostics: W4ExternalNoisyW41Diagnostics,
    facet_ablation: W4ExternalNoisyFacetAblationDiagnostics,
    p7_loss_ledger: W4ExternalNoisyP7LossDiagnostics,
    p7_production_delivery: W4ExternalNoisyP7ProductionDeliveryDiagnostics,
    source_signature_counts: BTreeMap<String, usize>,
}

impl P7DetailAggregate {
    fn add_assign(&mut self, other: &Self) -> Result<()> {
        self.samples = self.samples.saturating_add(other.samples);
        self.questions = self.questions.saturating_add(other.questions);
        self.evidence_questions = self
            .evidence_questions
            .saturating_add(other.evidence_questions);
        self.any_evidence_hit = self.any_evidence_hit.saturating_add(other.any_evidence_hit);
        self.all_evidence_hit = self.all_evidence_hit.saturating_add(other.all_evidence_hit);
        self.write_errors = self.write_errors.saturating_add(other.write_errors);
        self.recall_errors = self.recall_errors.saturating_add(other.recall_errors);
        add_stage_hit_counts(&mut self.stage_hit_counts, &other.stage_hit_counts);
        add_index_diagnostics(&mut self.index_diagnostics, &other.index_diagnostics);
        add_w41_diagnostics(&mut self.w4_1_diagnostics, &other.w4_1_diagnostics);
        add_facet_ablation(&mut self.facet_ablation, &other.facet_ablation);
        add_p7_loss(&mut self.p7_loss_ledger, &other.p7_loss_ledger);
        add_p7_production_delivery(
            &mut self.p7_production_delivery,
            &other.p7_production_delivery,
        );
        Ok(())
    }

    fn refresh_source_signature_diagnostics(&mut self) {
        self.w4_1_diagnostics.source_signature_count = self.source_signature_counts.len();
        self.w4_1_diagnostics.repeated_source_signature_questions = self
            .source_signature_counts
            .values()
            .map(|count| count.saturating_sub(1))
            .sum();
    }
}

struct P7DetailValidationContext<'a> {
    suite: &'a str,
    run_id: &'a str,
    expected_questions: &'a [P7ExpectedQuestionIdentity],
    expected_samples: usize,
}

fn validate_p7_detail_file(
    path: &Path,
    expected_sha256: &str,
    context: P7DetailValidationContext<'_>,
    seen_question_ids: &mut BTreeSet<String>,
    seen_identities: &mut BTreeSet<(String, usize, usize, String)>,
) -> Result<P7DetailAggregate> {
    let file = File::open(path).map_err(|source| Error::Io {
        source,
        stage: "p7_provenance_read_shard_detail",
    })?;
    let mut reader = BufReader::new(P7HashingReader::new(file));
    let mut aggregate = P7DetailAggregate {
        samples: context.expected_samples,
        ..P7DetailAggregate::default()
    };
    let mut line = String::new();
    let mut row_index = 0_usize;
    loop {
        line.clear();
        let read = reader.read_line(&mut line).map_err(|source| Error::Io {
            source,
            stage: "p7_provenance_read_detail_row",
        })?;
        if read == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let expected = context
            .expected_questions
            .get(row_index)
            .ok_or_else(|| p7_provenance_error("detail contains an unexpected question"))?;
        let row =
            serde_json::from_str::<serde_json::Value>(&line).map_err(|source| Error::Other {
                source: Box::new(source),
                stage: "p7_provenance_parse_detail_row",
            })?;
        validate_p7_detail_identity(
            &row,
            context.suite,
            context.run_id,
            expected,
            seen_question_ids,
            seen_identities,
        )?;
        accumulate_p7_detail_row(&mut aggregate, &row, expected)?;
        row_index = row_index.saturating_add(1);
    }
    if row_index != context.expected_questions.len() {
        return Err(p7_provenance_error("detail row count mismatch"));
    }
    let actual_sha256 = reader.into_inner().finish_sha256();
    if actual_sha256 != expected_sha256 {
        return Err(p7_provenance_error("shard detail digest mismatch"));
    }
    Ok(aggregate)
}

fn validate_p7_detail_identity(
    row: &serde_json::Value,
    suite: &str,
    run_id: &str,
    expected: &P7ExpectedQuestionIdentity,
    seen_question_ids: &mut BTreeSet<String>,
    seen_identities: &mut BTreeSet<(String, usize, usize, String)>,
) -> Result<()> {
    let case_id = p7_required_str(row, "case_id", "detail case_id missing")?;
    let dataset_index = p7_required_usize(row, "dataset_index", "detail dataset_index missing")?;
    let question_index = p7_required_usize(row, "question_index", "detail question_index missing")?;
    let question_id = p7_required_str(row, "question_id", "detail question_id missing")?;
    let detail_gold = p7_string_array(
        row.get("gold_sources")
            .ok_or_else(|| p7_provenance_error("detail gold_sources missing"))?,
        "detail gold_sources must be a string array",
    )?;
    let detail_gold_groups = p7_canonical_groups(&detail_gold);
    if detail_gold.len() != detail_gold_groups.len()
        || detail_gold
            .iter()
            .any(|group| group != &bm_core::memory::canonical_recall_evidence_group(group))
    {
        return Err(p7_provenance_error(
            "detail gold_sources are not unique opaque canonical ids",
        ));
    }
    if p7_required_str(row, "suite", "detail suite missing")? != suite
        || p7_required_str(row, "run_id", "detail run_id missing")? != run_id
        || case_id != expected.case_id
        || dataset_index != expected.dataset_index
        || question_index != expected.question_index
        || question_id != expected.question_id
        || p7_required_str(row, "question", "detail question missing")? != expected.question
        || detail_gold_groups != p7_canonical_groups(&expected.gold_sources)
    {
        return Err(p7_provenance_error("detail question identity mismatch"));
    }
    if !seen_question_ids.insert(question_id.to_string())
        || !seen_identities.insert((
            case_id.to_string(),
            dataset_index,
            question_index,
            question_id.to_string(),
        ))
    {
        return Err(p7_provenance_error(
            "detail question identity is not unique",
        ));
    }
    Ok(())
}

fn accumulate_p7_detail_row(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
    expected: &P7ExpectedQuestionIdentity,
) -> Result<()> {
    validate_p7_no_external_locators(row)?;
    for field in [
        "index_diagnostics",
        "graph_index_report",
        "facet_index_report",
        "stage_diagnostics",
        "ablation_report",
        "p7_loss_ledger",
        "eval_delivery_report",
        "final_projection_delivery_report",
        "sdk_projection_delivery_manifest",
        "runner_projection_digest_observation",
        "final_projection_integrity",
        "privacy_report",
    ] {
        if row.get(field).is_none_or(serde_json::Value::is_null) {
            return Err(p7_provenance_error("P7 detail proof field missing"));
        }
    }
    validate_p7_stage_candidate_reports(row)?;
    aggregate.questions = aggregate.questions.saturating_add(1);
    if !expected.gold_sources.is_empty() {
        aggregate.evidence_questions = aggregate.evidence_questions.saturating_add(1);
    }
    aggregate.write_errors = aggregate.write_errors.saturating_add(usize::from(
        !row.get("write_error")
            .is_none_or(serde_json::Value::is_null),
    ));
    aggregate.recall_errors = aggregate.recall_errors.saturating_add(usize::from(
        !row.get("recall_error")
            .is_none_or(serde_json::Value::is_null),
    ));

    let final_delivery = row
        .get("final_projection_delivery_report")
        .ok_or_else(|| p7_provenance_error("final projection delivery report missing"))?;
    let rendered_candidates = p7_rendered_candidates_from_delivery(final_delivery)?;
    let matched_gold = p7_match_gold_groups(&expected.gold_sources, &rendered_candidates);
    let gold_group_count = p7_canonical_groups(&expected.gold_sources).len();
    let any_hit = !matched_gold.is_empty();
    let all_hit = gold_group_count > 0 && matched_gold.len() == gold_group_count;
    if p7_required_bool(row, "any_evidence_hit", "detail any hit missing")? != any_hit
        || p7_required_bool(row, "all_evidence_hit", "detail all hit missing")? != all_hit
    {
        return Err(p7_provenance_error(
            "detail final rendered hit fact mismatch",
        ));
    }
    aggregate.any_evidence_hit = aggregate
        .any_evidence_hit
        .saturating_add(usize::from(any_hit));
    aggregate.all_evidence_hit = aggregate
        .all_evidence_hit
        .saturating_add(usize::from(all_hit));

    accumulate_p7_stage_hits(aggregate, row, expected)?;
    accumulate_p7_index_diagnostics(aggregate, row)?;
    accumulate_p7_w41_diagnostics(aggregate, row, expected)?;
    accumulate_p7_ablation(aggregate, row, expected)?;
    accumulate_p7_loss(aggregate, row, expected)?;
    accumulate_p7_production_delivery(aggregate, row)?;
    Ok(())
}

fn validate_p7_no_external_locators(value: &serde_json::Value) -> Result<()> {
    match value {
        serde_json::Value::String(value) if value.contains("external_eval:") => Err(
            p7_provenance_error("P7 detail exposes a raw external evaluation locator"),
        ),
        serde_json::Value::Array(values) => {
            for value in values {
                validate_p7_no_external_locators(value)?;
            }
            Ok(())
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                validate_p7_no_external_locators(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn accumulate_p7_stage_hits(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
    expected: &P7ExpectedQuestionIdentity,
) -> Result<()> {
    let final_delivery = row
        .get("final_projection_delivery_report")
        .ok_or_else(|| p7_provenance_error("final projection delivery report missing"))?;
    let rendered_sources = p7_row_string_array(row, "rendered_sources")?;
    if rendered_sources != p7_rendered_sources_from_delivery(final_delivery)? {
        return Err(p7_provenance_error(
            "rendered sources are not owned by final projection delivery",
        ));
    }
    let projection_selected_sources = p7_row_string_array(row, "projection_selected_sources")?;
    if projection_selected_sources != p7_selected_sources_from_delivery(final_delivery)? {
        return Err(p7_provenance_error(
            "projection selected sources are not owned by final projection delivery",
        ));
    }
    let stages = [
        ("source", "source"),
        ("expanded", "expanded"),
        ("reranked", "reranked"),
        ("eval_selected", "selected"),
        ("projection_selected", "projection_selected"),
        ("final_rendered", "rendered"),
    ];
    for (field, stage) in stages {
        let candidates = p7_stage_candidates(row, field)?;
        let matched = p7_matched_gold_group_set(&expected.gold_sources, &candidates);
        let gold_count = p7_canonical_groups(&expected.gold_sources).len();
        let any = usize::from(!matched.is_empty());
        let all = usize::from(gold_count > 0 && matched.len() == gold_count);
        match stage {
            "source" => {
                aggregate.stage_hit_counts.source_any_evidence_hit += any;
                aggregate.stage_hit_counts.source_all_evidence_hit += all;
            }
            "expanded" => {
                aggregate.stage_hit_counts.expanded_any_evidence_hit += any;
                aggregate.stage_hit_counts.expanded_all_evidence_hit += all;
            }
            "reranked" => {
                aggregate.stage_hit_counts.reranked_any_evidence_hit += any;
                aggregate.stage_hit_counts.reranked_all_evidence_hit += all;
            }
            "selected" => {
                aggregate.stage_hit_counts.selected_any_evidence_hit += any;
                aggregate.stage_hit_counts.selected_all_evidence_hit += all;
            }
            "projection_selected" => {
                aggregate
                    .stage_hit_counts
                    .projection_selected_any_evidence_hit += any;
                aggregate
                    .stage_hit_counts
                    .projection_selected_all_evidence_hit += all;
            }
            "rendered" => {
                aggregate.stage_hit_counts.rendered_any_evidence_hit += any;
                aggregate.stage_hit_counts.rendered_all_evidence_hit += all;
            }
            _ => unreachable!(),
        }
    }
    let canonical_groups = p7_canonical_groups(&expected.gold_sources);
    let expected_question_type = if canonical_groups.len() >= 2 {
        "multi_gold"
    } else {
        "single_gold"
    };
    if row
        .get("stage_diagnostics")
        .and_then(|diagnostics| diagnostics.get("question_type"))
        .and_then(serde_json::Value::as_str)
        != Some(expected_question_type)
    {
        return Err(p7_provenance_error("detail question type mismatch"));
    }
    Ok(())
}

fn accumulate_p7_index_diagnostics(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
) -> Result<()> {
    let claimed = row
        .get("index_diagnostics")
        .ok_or_else(|| p7_provenance_error("detail index diagnostics missing"))?;
    let derived = p7_index_diagnostics_from_raw_reports(row)?;
    let derived_value = serde_json::to_value(&derived).map_err(|source| Error::Other {
        source: Box::new(source),
        stage: "p7_provenance_serialize_recomputed_index_diagnostics",
    })?;
    if claimed != &derived_value {
        return Err(p7_provenance_error(
            "per-question index diagnostics do not match raw SDK reports",
        ));
    }
    add_index_diagnostics(&mut aggregate.index_diagnostics, &derived);
    Ok(())
}

fn p7_index_diagnostics_from_raw_reports(
    row: &serde_json::Value,
) -> Result<W4ExternalNoisyIndexDiagnostics> {
    let graph = row
        .get("graph_index_report")
        .ok_or_else(|| p7_provenance_error("raw graph index report missing"))?;
    let facet = row
        .get("facet_index_report")
        .ok_or_else(|| p7_provenance_error("raw facet index report missing"))?;
    validate_p7_safe_graph_index_report(graph)?;
    p7_required_usize(
        graph,
        "index_doc_count",
        "graph index document count missing",
    )?;
    let matched_source_anchor_count = p7_required_usize(
        graph,
        "matched_source_anchor_count",
        "graph matched source anchor count missing",
    )?;
    let indexed_neighbor_count = p7_required_usize(
        graph,
        "indexed_neighbor_count",
        "graph indexed neighbor count missing",
    )?;
    let manifest_contract_verified = p7_required_bool(
        graph,
        "manifest_contract_verified",
        "graph manifest contract verification missing",
    )?;
    let selected_dependency_chain_verified = p7_required_bool(
        graph,
        "selected_dependency_chain_verified",
        "graph selected dependency chain verification missing",
    )?;
    let full_scope_closure_verified = p7_required_bool(
        graph,
        "full_scope_closure_verified",
        "graph full-scope closure verification missing",
    )?;
    let manifest_generation_present = p7_required_bool(
        graph,
        "manifest_generation_present",
        "graph manifest generation presence missing",
    )?;
    let graph_revision_present = p7_required_bool(
        graph,
        "graph_revision_present",
        "graph revision presence missing",
    )?;
    let scope_digest_present = p7_required_bool(
        graph,
        "scope_digest_present",
        "graph scope digest presence missing",
    )?;
    let maintenance_required = p7_required_bool(
        graph,
        "maintenance_required",
        "graph maintenance requirement missing",
    )?;
    let incident_present =
        p7_required_bool(graph, "incident_present", "graph incident presence missing")?;
    let read_path_mutation_delta = p7_required_usize(
        graph,
        "read_path_mutation_delta",
        "graph read-path mutation delta missing",
    )?;
    let graph_used = p7_required_bool(graph, "used", "graph index used claim missing")?;

    let posting_key_lookup_count = p7_required_usize(
        facet,
        "posting_key_lookup_count",
        "facet posting key lookup count missing",
    )?;
    let manifest_matched_posting_count = p7_required_usize(
        facet,
        "manifest_matched_posting_count",
        "facet manifest-matched posting count missing",
    )?;
    let posting_doc_read_count = p7_required_usize(
        facet,
        "posting_doc_read_count",
        "facet posting read count missing",
    )?;
    let owner_key_lookup_count = p7_required_usize(
        facet,
        "owner_key_lookup_count",
        "facet owner key lookup count missing",
    )?;
    let owner_doc_read_count = p7_required_usize(
        facet,
        "owner_doc_read_count",
        "facet owner read count missing",
    )?;
    p7_required_usize(
        facet,
        "manifest_owner_doc_count",
        "facet manifest owner document count missing",
    )?;
    p7_required_usize(
        facet,
        "manifest_posting_doc_count",
        "facet manifest posting document count missing",
    )?;
    p7_required_usize(facet, "render_growth", "facet render growth claim missing")?;
    let manifest_integrity_verified = p7_required_bool(
        facet,
        "manifest_integrity_verified",
        "facet manifest integrity claim missing",
    )?;
    let facet_used = p7_required_bool(facet, "used", "facet index used claim missing")?;
    let facet_failure_count =
        p7_required_usize(facet, "failure_count", "facet failure count missing")?;
    let facet_integrity_failure_count = p7_required_usize(
        facet,
        "integrity_failure_count",
        "facet integrity failure count missing",
    )?;
    if facet_integrity_failure_count > facet_failure_count {
        return Err(p7_provenance_error(
            "facet integrity failure count exceeds raw failures",
        ));
    }
    if facet_used
        && (posting_key_lookup_count == 0
            || manifest_matched_posting_count > posting_key_lookup_count)
    {
        return Err(p7_provenance_error(
            "used facet report has an invalid posting lookup proof",
        ));
    }
    if facet_used
        && manifest_integrity_verified
        && (posting_doc_read_count != manifest_matched_posting_count
            || owner_doc_read_count != owner_key_lookup_count
            || (manifest_matched_posting_count == 0
                && (owner_key_lookup_count != 0 || owner_doc_read_count != 0))
            || (manifest_matched_posting_count > 0 && owner_key_lookup_count == 0))
    {
        return Err(p7_provenance_error(
            "verified facet report has an inconsistent bounded read proof",
        ));
    }
    Ok(W4ExternalNoisyIndexDiagnostics {
        questions_with_index_report: 1,
        index_used_questions: usize::from(graph_used),
        fallback_full_scan_questions: usize::from(p7_required_bool(
            graph,
            "fallback_full_scan",
            "graph fallback claim missing",
        )?),
        source_candidate_count: p7_required_usize(
            graph,
            "source_candidate_count",
            "graph source candidate count missing",
        )?,
        matched_source_anchor_count,
        unmatched_source_anchor_count: p7_required_usize(
            graph,
            "unmatched_source_anchor_count",
            "graph unmatched source anchor count missing",
        )?,
        indexed_neighbor_count,
        filtered_node_count: p7_required_usize(
            graph,
            "filtered_node_count",
            "graph filtered node count missing",
        )?,
        filtered_edge_count: p7_required_usize(
            graph,
            "filtered_edge_count",
            "graph filtered edge count missing",
        )?,
        filtered_backlink_count: p7_required_usize(
            graph,
            "filtered_backlink_count",
            "graph filtered backlink count missing",
        )?,
        failure_count: p7_required_usize(graph, "failure_count", "graph failure count missing")?,
        graph_manifest_contract_verified_questions: usize::from(
            graph_used && manifest_contract_verified,
        ),
        graph_selected_dependency_chain_verified_questions: usize::from(
            graph_used && selected_dependency_chain_verified,
        ),
        graph_full_scope_closure_verified_questions: usize::from(
            graph_used && full_scope_closure_verified,
        ),
        graph_manifest_generation_present_questions: usize::from(
            graph_used && manifest_generation_present,
        ),
        graph_revision_present_questions: usize::from(graph_used && graph_revision_present),
        graph_scope_digest_present_questions: usize::from(graph_used && scope_digest_present),
        graph_maintenance_required_questions: usize::from(maintenance_required),
        graph_incident_questions: usize::from(incident_present),
        graph_read_path_mutation_delta: read_path_mutation_delta,
        facet_questions_with_index_report: 1,
        facet_index_used_questions: usize::from(facet_used),
        facet_report_only_questions: usize::from(p7_required_bool(
            facet,
            "report_only",
            "facet report-only claim missing",
        )?),
        facet_fallback_full_scan_questions: usize::from(p7_required_bool(
            facet,
            "fallback_full_scan",
            "facet fallback claim missing",
        )?),
        facet_source_candidate_count: p7_required_usize(
            facet,
            "source_candidate_count",
            "facet source candidate count missing",
        )?,
        facet_matched_source_candidate_count: p7_required_usize(
            facet,
            "matched_source_candidate_count",
            "facet matched source candidate count missing",
        )?,
        facet_posting_key_lookup_count: posting_key_lookup_count,
        facet_manifest_matched_posting_count: manifest_matched_posting_count,
        facet_posting_doc_read_count: posting_doc_read_count,
        facet_owner_key_lookup_count: owner_key_lookup_count,
        facet_owner_doc_read_count: owner_doc_read_count,
        facet_zero_posting_key_lookup_questions: usize::from(
            facet_used && posting_key_lookup_count == 0,
        ),
        facet_clean_zero_hit_questions: usize::from(
            facet_used
                && manifest_integrity_verified
                && manifest_matched_posting_count == 0
                && posting_doc_read_count == 0
                && owner_key_lookup_count == 0
                && owner_doc_read_count == 0,
        ),
        facet_manifest_integrity_verified_questions: usize::from(
            facet_used && manifest_integrity_verified,
        ),
        facet_manifest_integrity_failure_count: usize::from(
            facet_used && !manifest_integrity_verified,
        ),
        facet_exact_match_count: p7_required_usize(
            facet,
            "exact_facet_match_count",
            "facet exact match count missing",
        )?,
        facet_expanded_match_count: p7_required_usize(
            facet,
            "expanded_facet_match_count",
            "facet expanded match count missing",
        )?,
        facet_failure_count: facet_integrity_failure_count,
    })
}

fn validate_p7_safe_graph_index_report(report: &serde_json::Value) -> Result<()> {
    let fields = report
        .as_object()
        .ok_or_else(|| p7_provenance_error("raw graph index report must be an object"))?;
    const FORBIDDEN_RAW_FIELDS: [&str; 8] = [
        "owner",
        "manifest_generation",
        "graph_revision",
        "scope_digest",
        "incident_token",
        "source_anchor_ids",
        "unmatched_source_anchor_ids",
        "expanded_node_ids",
    ];
    if fields.keys().any(|field| {
        FORBIDDEN_RAW_FIELDS.contains(&field.as_str())
            || field.ends_with("_id")
            || field.ends_with("_ids")
    }) {
        return Err(p7_provenance_error(
            "raw graph index report exposes a digest, token, or raw id",
        ));
    }
    if fields
        .values()
        .any(|value| !value.is_boolean() && value.as_u64().is_none())
    {
        return Err(p7_provenance_error(
            "raw graph index report must contain only safe booleans and counters",
        ));
    }
    Ok(())
}

fn accumulate_p7_w41_diagnostics(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
    expected: &P7ExpectedQuestionIdentity,
) -> Result<()> {
    const STAGES: [&str; 5] = ["source", "expanded", "reranked", "selected", "rendered"];
    let diagnostics = row
        .get("stage_diagnostics")
        .ok_or_else(|| p7_provenance_error("stage diagnostics missing"))?;
    let gold_groups = p7_canonical_groups(&expected.gold_sources);
    let question_type = if gold_groups.len() >= 2 {
        "multi_gold"
    } else {
        "single_gold"
    };
    let diagnostic_gold = p7_string_array(
        diagnostics
            .get("gold_evidence_refs")
            .ok_or_else(|| p7_provenance_error("W4.1 gold evidence refs missing"))?,
        "W4.1 gold evidence refs must be strings",
    )?;
    if p7_required_str(diagnostics, "suite", "W4.1 suite missing")?
        != p7_required_str(row, "suite", "detail suite missing")?
        || p7_required_str(diagnostics, "question_id", "W4.1 question_id missing")?
            != expected.question_id
        || p7_required_str(diagnostics, "question_type", "W4.1 question type missing")?
            != question_type
        || p7_required_usize(diagnostics, "evidence_count", "W4.1 evidence count missing")?
            != expected.gold_sources.len()
        || p7_canonical_groups(&diagnostic_gold) != gold_groups
    {
        return Err(p7_provenance_error("W4.1 detail identity mismatch"));
    }

    let stage_candidates = [
        ("source", p7_stage_candidates(row, "source")?),
        ("expanded", p7_stage_candidates(row, "expanded")?),
        ("reranked", p7_stage_candidates(row, "reranked")?),
        ("selected", p7_stage_candidates(row, "eval_selected")?),
        ("rendered", p7_stage_candidates(row, "eval_rendered")?),
    ];
    let mut derived_matched = BTreeMap::new();
    let mut derived_missing = BTreeMap::new();
    let mut derived_ranks = BTreeMap::new();
    let mut first_any = None;
    let mut first_all = None;
    for (stage, candidates) in &stage_candidates {
        let matches = p7_match_gold_groups(&expected.gold_sources, candidates);
        let matched = matches
            .iter()
            .map(|(_, group)| group.clone())
            .collect::<BTreeSet<_>>();
        let missing = gold_groups
            .difference(&matched)
            .cloned()
            .collect::<BTreeSet<_>>();
        let ranks = matches
            .into_iter()
            .filter_map(|(candidate_id, group)| {
                candidates
                    .iter()
                    .position(|candidate| candidate.candidate_id == candidate_id)
                    .map(|rank| (group, rank + 1))
            })
            .collect::<BTreeMap<_, _>>();
        if first_any.is_none() && !matched.is_empty() {
            first_any = Some((*stage).to_string());
        }
        if first_all.is_none() && !gold_groups.is_empty() && missing.is_empty() {
            first_all = Some((*stage).to_string());
        }
        derived_matched.insert((*stage).to_string(), matched);
        derived_missing.insert((*stage).to_string(), missing);
        derived_ranks.insert((*stage).to_string(), ranks);
    }
    if p7_optional_str(
        diagnostics,
        "first_any_hit_stage",
        "W4.1 first any-hit stage missing",
    )? != first_any.as_deref()
        || p7_optional_str(
            diagnostics,
            "first_all_hit_stage",
            "W4.1 first all-hit stage missing",
        )? != first_all.as_deref()
        || p7_stage_evidence_groups(diagnostics, "matched_gold_by_stage")? != derived_matched
        || p7_stage_evidence_groups(diagnostics, "missing_gold_by_stage")? != derived_missing
    {
        return Err(p7_provenance_error(
            "W4.1 stage claims do not match detail stage sources",
        ));
    }
    let miss_after_expanded = !gold_groups.is_empty()
        && derived_missing
            .get("expanded")
            .is_some_and(|missing| !missing.is_empty());
    if p7_required_bool(
        diagnostics,
        "miss_after_expanded",
        "W4.1 expanded miss claim missing",
    )? != miss_after_expanded
    {
        return Err(p7_provenance_error(
            "W4.1 expanded miss claim does not match detail stages",
        ));
    }

    let ranks = p7_required_array(diagnostics, "gold_rank_by_stage", "W4.1 gold ranks missing")?;
    let mut seen_ranks = BTreeSet::new();
    let mut found_count = 0_usize;
    let mut missing_count = 0_usize;
    let mut rank_sum = 0_usize;
    for rank in ranks {
        let stage = p7_required_str(rank, "stage", "W4.1 gold rank stage missing")?;
        if !STAGES.contains(&stage) {
            return Err(p7_provenance_error("W4.1 gold rank stage is unknown"));
        }
        let evidence_ref =
            p7_required_str(rank, "evidence_ref", "W4.1 gold rank evidence ref missing")?;
        let evidence_groups = p7_canonical_groups(&[evidence_ref.to_string()]);
        if evidence_groups.len() != 1 {
            return Err(p7_provenance_error(
                "W4.1 gold rank evidence ref is not exact",
            ));
        }
        let evidence_group = evidence_groups
            .iter()
            .next()
            .expect("single canonical evidence group")
            .clone();
        if !gold_groups.contains(&evidence_group)
            || !seen_ranks.insert((stage.to_string(), evidence_group.clone()))
        {
            return Err(p7_provenance_error(
                "W4.1 gold rank identity is missing or duplicated",
            ));
        }
        let rank_value = match rank.get("rank") {
            Some(value) if value.is_null() => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or_else(|| p7_provenance_error("W4.1 gold rank is invalid"))?,
            ),
            None => return Err(p7_provenance_error("W4.1 gold rank value missing")),
        };
        let expected_rank = derived_ranks
            .get(stage)
            .and_then(|ranks| ranks.get(&evidence_group))
            .copied();
        if expected_rank != rank_value {
            return Err(p7_provenance_error(
                "W4.1 gold rank does not match candidate-bound stage evidence",
            ));
        }
        if let Some(rank_value) = rank_value {
            found_count = found_count.saturating_add(1);
            rank_sum = rank_sum.saturating_add(rank_value);
        } else {
            missing_count = missing_count.saturating_add(1);
        }
    }
    if seen_ranks.len() != STAGES.len().saturating_mul(gold_groups.len()) {
        return Err(p7_provenance_error("W4.1 gold rank matrix is incomplete"));
    }

    let w41 = &mut aggregate.w4_1_diagnostics;
    w41.questions_with_w4_1_diagnostics = w41.questions_with_w4_1_diagnostics.saturating_add(1);
    if let Some(stage) = first_any {
        p7_increment(&mut w41.first_any_hit_stage_counts, &stage, 1);
    }
    if let Some(stage) = first_all {
        p7_increment(&mut w41.first_all_hit_stage_counts, &stage, 1);
    }
    for stage in STAGES {
        p7_increment(
            &mut w41.missing_gold_by_stage_counts,
            stage,
            derived_missing.get(stage).map_or(0, BTreeSet::len),
        );
    }
    w41.miss_after_expanded_count = w41
        .miss_after_expanded_count
        .saturating_add(usize::from(miss_after_expanded));
    w41.gold_rank_found_count = w41.gold_rank_found_count.saturating_add(found_count);
    w41.gold_rank_missing_count = w41.gold_rank_missing_count.saturating_add(missing_count);
    w41.gold_rank_sum = w41.gold_rank_sum.saturating_add(rank_sum);
    w41.truncated_count = w41.truncated_count.saturating_add(p7_required_usize(
        diagnostics,
        "truncated_count",
        "W4.1 truncated count missing",
    )?);
    for reason in p7_string_array(
        diagnostics
            .get("blocked_reasons")
            .ok_or_else(|| p7_provenance_error("W4.1 blocked reasons missing"))?,
        "W4.1 blocked reasons must be strings",
    )? {
        p7_increment(&mut w41.blocked_reason_counts, &reason, 1);
    }
    p7_increment(&mut w41.question_type_counts, question_type, 1);
    p7_increment(
        &mut w41.evidence_count_buckets,
        p7_evidence_count_bucket(expected.gold_sources.len()),
        1,
    );
    let signature = p7_stage_candidates(row, "source")?
        .into_iter()
        .map(|candidate| {
            format!(
                "{}:{}",
                candidate.candidate_id,
                candidate.canonical_evidence_groups.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    p7_increment(&mut aggregate.source_signature_counts, &signature, 1);
    aggregate.refresh_source_signature_diagnostics();
    Ok(())
}

fn p7_stage_evidence_groups(
    diagnostics: &serde_json::Value,
    field: &str,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    const STAGES: [&str; 5] = ["source", "expanded", "reranked", "selected", "rendered"];
    let mut by_stage = BTreeMap::new();
    for entry in p7_required_array(diagnostics, field, "W4.1 stage evidence matrix missing")? {
        let stage = p7_required_str(entry, "stage", "W4.1 stage evidence name missing")?;
        if !STAGES.contains(&stage) {
            return Err(p7_provenance_error("W4.1 stage evidence name is unknown"));
        }
        let evidence_refs = p7_string_array(
            entry
                .get("evidence_refs")
                .ok_or_else(|| p7_provenance_error("W4.1 stage evidence refs missing"))?,
            "W4.1 stage evidence refs must be strings",
        )?;
        if by_stage
            .insert(stage.to_string(), p7_canonical_groups(&evidence_refs))
            .is_some()
        {
            return Err(p7_provenance_error("W4.1 stage evidence is duplicated"));
        }
    }
    if by_stage.len() != STAGES.len() {
        return Err(p7_provenance_error(
            "W4.1 stage evidence matrix is incomplete",
        ));
    }
    Ok(by_stage)
}

fn p7_evidence_count_bucket(count: usize) -> &'static str {
    match count {
        0 => "0",
        1 => "1",
        2 | 3 => "2_3",
        _ => "4_plus",
    }
}

fn accumulate_p7_ablation(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
    expected: &P7ExpectedQuestionIdentity,
) -> Result<()> {
    let report = row
        .get("ablation_report")
        .ok_or_else(|| p7_provenance_error("detail ablation report missing"))?;
    if p7_required_str(report, "method", "ablation method missing")? != P7_ABLATION_METHOD {
        return Err(p7_provenance_error("unexpected ablation method"));
    }
    let required_slices = p7_row_string_array(report, "required_slices")?;
    let expected_slice_set = P7_REQUIRED_ABLATION_SLICES
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let required_slice_set = required_slices.iter().cloned().collect::<BTreeSet<_>>();
    if required_slices.len() != P7_REQUIRED_ABLATION_SLICES.len()
        || required_slice_set != expected_slice_set
    {
        return Err(p7_provenance_error(
            "ablation required slices are not the exact P7 set",
        ));
    }
    let slices = report
        .get("slices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| p7_provenance_error("detail ablation slices missing"))?;
    if slices.len() != P7_REQUIRED_ABLATION_SLICES.len() {
        return Err(p7_provenance_error(
            "ablation slice count does not match the P7 contract",
        ));
    }
    let mut actual_slice_set = BTreeSet::new();
    for slice in slices {
        let name = p7_required_str(slice, "name", "ablation slice name missing")?;
        if !actual_slice_set.insert(name.to_string()) || !expected_slice_set.contains(name) {
            return Err(p7_provenance_error(
                "ablation slices are duplicated or outside the P7 contract",
            ));
        }
        if p7_required_bool(slice, "feature_enabled", "ablation feature state missing")?
            || !p7_required_bool(
                slice,
                "report_available",
                "ablation report availability missing",
            )?
        {
            return Err(p7_provenance_error(
                "ablation slice is not a complete disabled off-run",
            ));
        }
        if !p7_required_bool(
            slice,
            "candidate_boundary_proven",
            "ablation candidate boundary proof missing",
        )? {
            return Err(p7_provenance_error(
                "ablation candidate boundary is not proven",
            ));
        }
    }
    if actual_slice_set != expected_slice_set {
        return Err(p7_provenance_error(
            "ablation slices are not the exact P7 set",
        ));
    }
    let diagnostics = &mut aggregate.facet_ablation;
    diagnostics.questions_with_ablation_report += 1;
    p7_increment(&mut diagnostics.method_counts, P7_ABLATION_METHOD, 1);
    let claimed_report_contribution = p7_required_bool(
        report,
        "delivery_contribution_proven",
        "ablation contribution flag missing",
    )?;
    let claimed_report_render_growth =
        p7_required_usize(report, "render_growth", "ablation render growth missing")?;
    for required in required_slices {
        p7_increment(&mut diagnostics.required_slice_counts, &required, 1);
    }
    for reason in p7_row_string_array(report, "blocked_reasons")? {
        p7_increment(&mut diagnostics.blocked_reason_counts, &reason, 1);
    }
    let gold_groups = p7_canonical_groups(&expected.gold_sources);
    let evidence_index = p7_candidate_evidence_array(
        row.get("evidence_ref_index")
            .ok_or_else(|| p7_provenance_error("SDK safe evidence index missing"))?,
    )?;
    let evidence_by_candidate = p7_candidate_evidence_map(&evidence_index);
    let baseline_selected_owner =
        p7_candidate_evidence_map(&p7_stage_candidates(row, "eval_selected")?);
    let baseline_rendered_owner =
        p7_candidate_evidence_map(&p7_stage_candidates(row, "eval_rendered")?);
    let mut computed_report_contribution = false;
    let mut computed_report_render_growth = 0_usize;
    for slice in slices {
        let name = p7_required_str(slice, "name", "ablation slice name missing")?;
        let report_available = p7_required_bool(
            slice,
            "report_available",
            "ablation report availability missing",
        )?;
        if report_available {
            p7_increment(&mut diagnostics.report_available_slice_counts, name, 1);
        }
        let baseline_selected = p7_ablation_candidates(slice, "baseline_selected_candidates")?;
        let off_selected = p7_ablation_candidates(slice, "off_run_selected_candidates")?;
        let baseline_rendered = p7_ablation_candidates(slice, "baseline_rendered_candidates")?;
        let off_rendered = p7_ablation_candidates(slice, "off_run_rendered_candidates")?;
        if p7_candidate_evidence_map(&baseline_selected) != baseline_selected_owner
            || p7_candidate_evidence_map(&baseline_rendered) != baseline_rendered_owner
        {
            return Err(p7_provenance_error(
                "ablation baseline candidates differ from raw SDK stage candidates",
            ));
        }
        for candidates in [&off_selected, &off_rendered] {
            for candidate in candidates {
                if evidence_by_candidate.get(&candidate.candidate_id)
                    != Some(
                        &candidate
                            .canonical_evidence_groups
                            .iter()
                            .cloned()
                            .collect::<BTreeSet<_>>(),
                    )
                {
                    return Err(p7_provenance_error(
                        "ablation off-run candidate differs from the safe evidence index",
                    ));
                }
            }
        }
        for (field, candidates) in [
            ("baseline_selected_candidate_ids", &baseline_selected),
            ("off_run_selected_candidate_ids", &off_selected),
            ("baseline_rendered_candidate_ids", &baseline_rendered),
            ("off_run_rendered_candidate_ids", &off_rendered),
        ] {
            let claimed_ids = p7_row_string_array(slice, field)?;
            let candidate_ids = candidates
                .iter()
                .map(|candidate| candidate.candidate_id.clone())
                .collect::<Vec<_>>();
            if claimed_ids != candidate_ids {
                return Err(p7_provenance_error(
                    "SDK ablation candidate identity claim differs from candidate-bound report",
                ));
            }
        }
        for (field, candidates) in [
            ("baseline_selected_evidence_refs", &baseline_selected),
            ("off_run_selected_evidence_refs", &off_selected),
            ("baseline_rendered_evidence_refs", &baseline_rendered),
            ("off_run_rendered_evidence_refs", &off_rendered),
        ] {
            let claims = p7_row_string_array(slice, field)?;
            if claims
                .iter()
                .any(|group| group != &bm_core::memory::canonical_recall_evidence_group(group))
                || p7_canonical_groups(&claims)
                    != p7_candidate_evidence_groups(candidates)
                        .into_iter()
                        .collect()
            {
                return Err(p7_provenance_error(
                    "ablation flat evidence claim differs from candidate-bound report",
                ));
            }
        }
        if baseline_selected.len()
            != p7_required_usize(
                slice,
                "baseline_selected_candidate_count",
                "ablation baseline selected candidate count missing",
            )?
            || off_selected.len()
                != p7_required_usize(
                    slice,
                    "off_run_selected_candidate_count",
                    "ablation off-run selected candidate count missing",
                )?
            || baseline_rendered.len()
                != p7_required_usize(
                    slice,
                    "baseline_rendered_candidate_count",
                    "ablation baseline rendered candidate count missing",
                )?
            || off_rendered.len()
                != p7_required_usize(
                    slice,
                    "off_run_rendered_candidate_count",
                    "ablation off-run rendered candidate count missing",
                )?
        {
            return Err(p7_provenance_error(
                "ablation candidate counts differ from candidate-bound reports",
            ));
        }
        let delivery_affected_candidate_ids = p7_affected_ablation_candidate_ids(
            &baseline_selected,
            &off_selected,
            &baseline_rendered,
            &off_rendered,
        );
        let claimed_affected_ids = p7_row_string_array(slice, "delivery_affected_candidate_ids")?
            .into_iter()
            .collect::<BTreeSet<_>>();
        if claimed_affected_ids != delivery_affected_candidate_ids
            || p7_required_usize(
                slice,
                "delivery_affected_candidate_count",
                "ablation affected count missing",
            )? != delivery_affected_candidate_ids.len()
        {
            return Err(p7_provenance_error(
                "ablation affected candidate claim differs from candidate-bound reports",
            ));
        }
        let sdk_delivery_affected_candidate_count_claim = p7_required_usize(
            slice,
            "sdk_delivery_affected_candidate_count_claim",
            "SDK ablation affected candidate count claim missing",
        )?;
        if sdk_delivery_affected_candidate_count_claim != delivery_affected_candidate_ids.len() {
            return Err(p7_provenance_error(
                "SDK ablation affected candidate count differs from candidate-bound facts",
            ));
        }
        diagnostics.delivery_affected_candidate_occurrences = diagnostics
            .delivery_affected_candidate_occurrences
            .saturating_add(delivery_affected_candidate_ids.len());
        let baseline_selected_matches =
            p7_match_gold_groups(&expected.gold_sources, &baseline_selected);
        let off_selected_matches = p7_match_gold_groups(&expected.gold_sources, &off_selected);
        let baseline_rendered_matches =
            p7_match_gold_groups(&expected.gold_sources, &baseline_rendered);
        let off_rendered_matches = p7_match_gold_groups(&expected.gold_sources, &off_rendered);
        let selected_delta =
            baseline_selected_matches.len() as i64 - off_selected_matches.len() as i64;
        let rendered_delta =
            baseline_rendered_matches.len() as i64 - off_rendered_matches.len() as i64;
        let selected_all_hit_lost = !gold_groups.is_empty()
            && baseline_selected_matches.len() == gold_groups.len()
            && off_selected_matches.len() != gold_groups.len();
        let rendered_all_hit_lost = !gold_groups.is_empty()
            && baseline_rendered_matches.len() == gold_groups.len()
            && off_rendered_matches.len() != gold_groups.len();
        if p7_required_i64(
            slice,
            "selected_evidence_hit_delta",
            "ablation selected hit delta missing",
        )? != selected_delta
            || p7_required_bool(
                slice,
                "selected_all_hit_lost",
                "ablation selected all-hit flag missing",
            )? != selected_all_hit_lost
            || p7_required_i64(
                slice,
                "rendered_evidence_hit_delta",
                "ablation rendered hit delta missing",
            )? != rendered_delta
            || p7_required_bool(
                slice,
                "rendered_all_hit_lost",
                "ablation rendered all-hit flag missing",
            )? != rendered_all_hit_lost
        {
            return Err(p7_provenance_error(
                "ablation evidence facts do not match canonical exact refs",
            ));
        }

        let expanded_candidate_delta = p7_signed_usize_delta(
            p7_required_usize(
                slice,
                "baseline_expanded_candidate_count",
                "ablation baseline expanded candidate count missing",
            )?,
            p7_required_usize(
                slice,
                "off_run_expanded_candidate_count",
                "ablation off-run expanded candidate count missing",
            )?,
        )?;
        let selected_candidate_delta = p7_signed_usize_delta(
            p7_required_usize(
                slice,
                "baseline_selected_candidate_count",
                "ablation baseline selected candidate count missing",
            )?,
            p7_required_usize(
                slice,
                "off_run_selected_candidate_count",
                "ablation off-run selected candidate count missing",
            )?,
        )?;
        let rendered_candidate_delta = p7_signed_usize_delta(
            p7_required_usize(
                slice,
                "baseline_rendered_candidate_count",
                "ablation baseline rendered candidate count missing",
            )?,
            p7_required_usize(
                slice,
                "off_run_rendered_candidate_count",
                "ablation off-run rendered candidate count missing",
            )?,
        )?;
        let rendered_char_delta = p7_signed_usize_delta(
            p7_required_usize(
                slice,
                "baseline_rendered_chars",
                "ablation baseline rendered chars missing",
            )?,
            p7_required_usize(
                slice,
                "off_run_rendered_chars",
                "ablation off-run rendered chars missing",
            )?,
        )?;
        let render_growth = p7_required_usize(
            slice,
            "baseline_render_growth",
            "ablation baseline render growth missing",
        )?
        .max(p7_required_usize(
            slice,
            "off_run_render_growth",
            "ablation off-run render growth missing",
        )?);
        for (field, expected_delta) in [
            ("expanded_candidate_delta", expanded_candidate_delta),
            ("selected_candidate_delta", selected_candidate_delta),
            ("rendered_candidate_delta", rendered_candidate_delta),
            ("rendered_char_delta", rendered_char_delta),
        ] {
            if p7_required_i64(slice, field, "ablation numeric delta missing")? != expected_delta {
                return Err(p7_provenance_error(
                    "ablation numeric delta does not match raw facts",
                ));
            }
        }
        if p7_required_usize(
            slice,
            "render_growth",
            "ablation slice render growth missing",
        )? != render_growth
        {
            return Err(p7_provenance_error(
                "ablation render growth does not match raw facts",
            ));
        }
        let slice_blocked_reasons = p7_row_string_array(slice, "blocked_reasons")?;
        let delivery_contribution_proven = report_available
            && slice_blocked_reasons.is_empty()
            && (selected_delta > 0
                || rendered_delta > 0
                || selected_all_hit_lost
                || rendered_all_hit_lost);
        if p7_required_bool(
            slice,
            "delivery_contribution_proven",
            "ablation contribution proof missing",
        )? != delivery_contribution_proven
        {
            return Err(p7_provenance_error(
                "ablation contribution proof does not match raw evidence facts",
            ));
        }
        if delivery_contribution_proven {
            computed_report_contribution = true;
            p7_increment(
                &mut diagnostics.delivery_contribution_proven_slice_counts,
                name,
                1,
            );
        }
        computed_report_render_growth = computed_report_render_growth
            .checked_add(render_growth)
            .ok_or_else(|| p7_provenance_error("ablation render growth overflow"))?;

        p7_increment_i64(
            &mut diagnostics.selected_evidence_hit_delta,
            name,
            selected_delta,
        );
        p7_increment_i64(
            &mut diagnostics.rendered_evidence_hit_delta,
            name,
            rendered_delta,
        );
        if selected_all_hit_lost {
            p7_increment(&mut diagnostics.selected_all_hit_loss_count, name, 1);
            if name == "evidence_family_rotation_off" {
                p7_increment(
                    &mut diagnostics.evidence_family_rotation_selected_all_hit_loss_count,
                    name,
                    1,
                );
            }
        }
        if rendered_all_hit_lost {
            p7_increment(&mut diagnostics.rendered_all_hit_loss_count, name, 1);
        }
        p7_increment_i64(
            &mut diagnostics.expanded_candidate_delta,
            name,
            expanded_candidate_delta,
        );
        p7_increment_i64(
            &mut diagnostics.selected_candidate_delta,
            name,
            selected_candidate_delta,
        );
        p7_increment_i64(
            &mut diagnostics.rendered_candidate_delta,
            name,
            rendered_candidate_delta,
        );
        p7_increment_i64(
            &mut diagnostics.rendered_char_delta,
            name,
            rendered_char_delta,
        );
        for reason in slice_blocked_reasons {
            p7_increment(&mut diagnostics.blocked_reason_counts, &reason, 1);
        }
    }
    if claimed_report_contribution != computed_report_contribution
        || claimed_report_render_growth != computed_report_render_growth
    {
        return Err(p7_provenance_error(
            "ablation report aggregate does not match recomputed slices",
        ));
    }
    diagnostics.delivery_contribution_proven_questions += usize::from(computed_report_contribution);
    diagnostics.render_growth = diagnostics
        .render_growth
        .checked_add(computed_report_render_growth)
        .ok_or_else(|| p7_provenance_error("ablation aggregate render growth overflow"))?;
    Ok(())
}

fn accumulate_p7_loss(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
    expected: &P7ExpectedQuestionIdentity,
) -> Result<()> {
    let ledger = row
        .get("p7_loss_ledger")
        .ok_or_else(|| p7_provenance_error("detail P7 loss ledger missing"))?;
    let claimed_expanded = p7_required_array(
        ledger,
        "expanded_hit_selected_miss",
        "expanded-selected loss entries missing",
    )?;
    let claimed_eval_rendered = p7_required_array(
        ledger,
        "selected_hit_rendered_miss",
        "selected-rendered loss entries missing",
    )?;
    let gold_groups = p7_canonical_groups(&expected.gold_sources);
    let expanded_groups = p7_matched_gold_group_set(
        &expected.gold_sources,
        &p7_stage_candidates(row, "expanded")?,
    );
    let selected_groups = p7_matched_gold_group_set(
        &expected.gold_sources,
        &p7_stage_candidates(row, "eval_selected")?,
    );
    let eval_rendered_groups = p7_matched_gold_group_set(
        &expected.gold_sources,
        &p7_stage_candidates(row, "eval_rendered")?,
    );
    let projection_selected_groups = p7_matched_gold_group_set(
        &expected.gold_sources,
        &p7_stage_candidates(row, "projection_selected")?,
    );
    let final_rendered_groups = p7_matched_gold_group_set(
        &expected.gold_sources,
        &p7_stage_candidates(row, "final_rendered")?,
    );
    let expanded_selected_loss =
        p7_stage_loss_groups(&gold_groups, &expanded_groups, &selected_groups);
    let eval_selected_rendered_loss =
        p7_stage_loss_groups(&gold_groups, &selected_groups, &eval_rendered_groups);
    let eval_selected_projection_selected_loss =
        p7_stage_loss_groups(&gold_groups, &selected_groups, &projection_selected_groups);
    let final_selected_rendered_loss = p7_stage_loss_groups(
        &gold_groups,
        &projection_selected_groups,
        &final_rendered_groups,
    );
    if p7_loss_entry_groups(claimed_expanded)? != expanded_selected_loss
        || p7_loss_entry_groups(claimed_eval_rendered)? != eval_selected_rendered_loss
    {
        return Err(p7_provenance_error(
            "SDK loss ledger does not match independently recomputed eval stages",
        ));
    }
    let diagnostics = &mut aggregate.p7_loss_ledger;
    diagnostics.questions_with_loss_ledger += 1;
    diagnostics.expanded_hit_selected_miss_questions +=
        usize::from(!expanded_selected_loss.is_empty());
    diagnostics.eval_selected_hit_rendered_miss_questions +=
        usize::from(!eval_selected_rendered_loss.is_empty());
    diagnostics.eval_selected_hit_projection_selected_miss_questions +=
        usize::from(!eval_selected_projection_selected_loss.is_empty());
    diagnostics.selected_hit_final_rendered_miss_questions +=
        usize::from(!final_selected_rendered_loss.is_empty());
    diagnostics.expanded_hit_selected_miss_evidence = diagnostics
        .expanded_hit_selected_miss_evidence
        .saturating_add(expanded_selected_loss.len());
    diagnostics.eval_selected_hit_rendered_miss_evidence = diagnostics
        .eval_selected_hit_rendered_miss_evidence
        .saturating_add(eval_selected_rendered_loss.len());
    diagnostics.eval_selected_hit_projection_selected_miss_evidence = diagnostics
        .eval_selected_hit_projection_selected_miss_evidence
        .saturating_add(eval_selected_projection_selected_loss.len());
    diagnostics.selected_hit_final_rendered_miss_evidence = diagnostics
        .selected_hit_final_rendered_miss_evidence
        .saturating_add(final_selected_rendered_loss.len());
    diagnostics.eval_truncated_count =
        diagnostics
            .eval_truncated_count
            .saturating_add(p7_required_usize(
                ledger,
                "truncated_count",
                "P7 loss truncation count missing",
            )?);
    for reason in p7_row_string_array(ledger, "blocked_reasons")? {
        p7_increment(&mut diagnostics.eval_blocked_reason_counts, &reason, 1);
    }
    Ok(())
}

fn accumulate_p7_production_delivery(
    aggregate: &mut P7DetailAggregate,
    row: &serde_json::Value,
) -> Result<()> {
    let eval_delivery = row
        .get("eval_delivery_report")
        .ok_or_else(|| p7_provenance_error("eval delivery report missing"))?;
    let final_delivery = row
        .get("final_projection_delivery_report")
        .ok_or_else(|| p7_provenance_error("final projection delivery report missing"))?;
    let privacy = row
        .get("privacy_report")
        .ok_or_else(|| p7_provenance_error("detail privacy report missing"))?;
    let diagnostics = &mut aggregate.p7_production_delivery;
    diagnostics.questions_with_delivery_report += 1;

    let eval_selected = p7_stage_candidates(row, "eval_selected")?
        .into_iter()
        .map(|candidate| candidate.candidate_id)
        .collect::<BTreeSet<_>>();
    let delivery_selected = p7_row_string_array(eval_delivery, "selected_candidate_ids")?
        .into_iter()
        .collect::<BTreeSet<_>>();
    diagnostics.eval_selected_matches_delivery_questions +=
        usize::from(eval_selected == delivery_selected);
    diagnostics.projection_selected_sources_proven_questions += usize::from(
        p7_candidate_evidence_map(&p7_stage_candidates(row, "projection_selected")?)
            == p7_candidate_evidence_map(&p7_selected_candidates_from_delivery(final_delivery)?),
    );
    let eval_rendered = p7_stage_candidates(row, "eval_rendered")?
        .into_iter()
        .map(|candidate| candidate.candidate_id)
        .collect::<BTreeSet<_>>();
    let delivery_rendered = p7_delivery_candidate_ids(eval_delivery)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    diagnostics.eval_rendered_matches_delivery_questions +=
        usize::from(eval_rendered == delivery_rendered);

    let projection_proven = p7_required_bool(
        row,
        "projection_delivery_proven",
        "projection delivery proof flag missing",
    )?;
    let sdk_manifest = row
        .get("sdk_projection_delivery_manifest")
        .ok_or_else(|| p7_provenance_error("SDK projection delivery manifest missing"))?;
    validate_p7_sdk_projection_delivery_manifest(sdk_manifest, final_delivery)?;
    validate_p7_runner_projection_digest_observation(
        row.get("runner_projection_digest_observation")
            .ok_or_else(|| p7_provenance_error("runner projection digest observation missing"))?,
        sdk_manifest,
    )?;
    if !projection_proven {
        return Err(p7_provenance_error(
            "runner projection delivery flag contradicts the SDK manifest",
        ));
    }
    diagnostics.projection_delivery_proof_questions += 1;
    let integrity = row
        .get("final_projection_integrity")
        .ok_or_else(|| p7_provenance_error("final projection integrity missing"))?;
    let checked_surfaces = p7_row_string_array(integrity, "checked_surfaces")?;
    let checked_surface_set = checked_surfaces.iter().cloned().collect::<BTreeSet<_>>();
    let expected_surfaces = [
        "prompt",
        "ui_api",
        "operator_raw",
        "gateway_raw_audit",
        "shared_fact_surface",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    let surface_reports = p7_required_array(
        integrity,
        "surface_reports",
        "final projection surface reports missing",
    )?;
    let mut reported_surfaces = BTreeSet::new();
    let mut recomputed_violation_count = 0_usize;
    for report in surface_reports {
        let surface = p7_required_str(
            report,
            "surface",
            "final projection integrity surface name missing",
        )?;
        if !reported_surfaces.insert(surface.to_string()) {
            return Err(p7_provenance_error(
                "final projection integrity surface is duplicated",
            ));
        }
        let protected_exact_echo_count = p7_required_usize(
            report,
            "protected_exact_echo_count",
            "surface protected exact echo count missing",
        )?;
        let forbidden_marker_count = p7_required_usize(
            report,
            "forbidden_marker_count",
            "surface forbidden marker count missing",
        )?;
        let violation_count =
            p7_required_usize(report, "violation_count", "surface violation count missing")?;
        let recomputed_surface_violations = protected_exact_echo_count
            .checked_add(forbidden_marker_count)
            .ok_or_else(|| p7_provenance_error("surface violation count overflow"))?;
        if violation_count != recomputed_surface_violations
            || p7_required_bool(report, "passed", "surface integrity pass flag missing")?
                != (violation_count == 0)
        {
            return Err(p7_provenance_error(
                "surface integrity facts are internally inconsistent",
            ));
        }
        recomputed_violation_count = recomputed_violation_count
            .checked_add(violation_count)
            .ok_or_else(|| p7_provenance_error("top-level violation count overflow"))?;
    }
    if checked_surfaces.len() != checked_surface_set.len()
        || checked_surface_set != expected_surfaces
        || surface_reports.len() != expected_surfaces.len()
        || reported_surfaces != expected_surfaces
    {
        return Err(p7_provenance_error(
            "final projection integrity surfaces do not match the SDK contract",
        ));
    }
    let integrity_passed = p7_required_bool(
        integrity,
        "passed",
        "final projection integrity pass flag missing",
    )?;
    let raw_private_violation_count = p7_required_usize(
        integrity,
        "raw_private_violation_count",
        "final projection raw private violation count missing",
    )?;
    if raw_private_violation_count != recomputed_violation_count
        || integrity_passed != (recomputed_violation_count == 0)
    {
        return Err(p7_provenance_error(
            "final projection integrity aggregate contradicts surface reports",
        ));
    }
    diagnostics.final_projection_integrity_questions += 1;
    diagnostics.final_projection_integrity_passed_questions += usize::from(integrity_passed);
    diagnostics.final_projection_raw_private_violation_count = diagnostics
        .final_projection_raw_private_violation_count
        .saturating_add(raw_private_violation_count);
    diagnostics.final_projection_blocked_source_count = diagnostics
        .final_projection_blocked_source_count
        .saturating_add(p7_required_usize(
            integrity,
            "blocked_source_count",
            "final projection blocked source count missing",
        )?);
    diagnostics.final_projection_redacted_source_count = diagnostics
        .final_projection_redacted_source_count
        .saturating_add(p7_required_usize(
            integrity,
            "redacted_source_count",
            "final projection redacted source count missing",
        )?);
    if !integrity_passed {
        p7_increment(
            &mut diagnostics.blocked_reason_counts,
            "final_projection_private_disclosure_integrity_failed",
            1,
        );
    }
    p7_increment(
        &mut diagnostics.schema_version_counts,
        &p7_required_usize(
            final_delivery,
            "schema_version",
            "final delivery schema version missing",
        )?
        .to_string(),
        1,
    );
    diagnostics.render_growth = diagnostics.render_growth.saturating_add(p7_required_usize(
        final_delivery,
        "render_growth",
        "final delivery render growth missing",
    )?);

    let private_raw = p7_required_usize(
        privacy,
        "private_raw_candidate_count",
        "privacy private raw count missing",
    )?;
    diagnostics.raw_soul_private_material_count = diagnostics
        .raw_soul_private_material_count
        .saturating_add(private_raw);
    if !p7_required_bool(privacy, "passed", "privacy pass flag missing")? {
        diagnostics.privacy_leak_count += 1;
    }
    for failure in p7_row_string_array(privacy, "failures")? {
        if failure.contains("cross_subject") {
            diagnostics.cross_subject_leak_count += 1;
        }
        if failure.contains("raw_soul_private") {
            diagnostics.raw_soul_private_material_count += 1;
        }
    }
    for capsule in p7_required_array(
        final_delivery,
        "rendered_capsules",
        "final rendered capsules missing",
    )? {
        let redaction = p7_required_str(
            capsule,
            "redaction_state",
            "capsule redaction state missing",
        )?;
        let shared_fact_surface_allowed = p7_required_bool(
            capsule,
            "shared_fact_surface_allowed",
            "capsule shared fact surface eligibility missing",
        )?;
        let redacted_reference_exposed = p7_required_array(
            capsule,
            "evidence_ref_views",
            "capsule evidence ref views missing",
        )?
        .iter()
        .any(|view| {
            view.get("visibility").and_then(serde_json::Value::as_str) == Some("redacted")
                && !view.get("reference").is_none_or(serde_json::Value::is_null)
        });
        if matches!(
            redaction,
            "private_garden" | "soul_private" | "operator_diagnostic"
        ) || (shared_fact_surface_allowed && redaction != "public_runtime")
            || redacted_reference_exposed
        {
            diagnostics.privacy_leak_count += 1;
        }
    }
    for failure in p7_row_string_array(final_delivery, "integrity_failures")? {
        p7_increment(&mut diagnostics.blocked_reason_counts, &failure, 1);
    }
    for reason in p7_row_string_array(final_delivery, "delivery_drop_reasons")? {
        p7_increment(&mut diagnostics.delivery_drop_reason_counts, &reason, 1);
    }
    Ok(())
}

fn validate_p7_sdk_projection_delivery_manifest(
    manifest: &serde_json::Value,
    final_delivery: &serde_json::Value,
) -> Result<()> {
    if p7_required_usize(
        manifest,
        "schema_version",
        "projection manifest schema version missing",
    )? != usize::try_from(MEMORY_PROJECTION_DELIVERY_DIGEST_SCHEMA_VERSION).unwrap_or(usize::MAX)
    {
        return Err(p7_provenance_error(
            "SDK projection delivery manifest contract mismatch",
        ));
    }
    let capsule_entries = p7_projection_manifest_entry_set(
        manifest,
        "capsule_entries",
        "SDK delivery capsule manifest entries are duplicated",
    )?;
    let governed_entries = p7_projection_manifest_entry_set(
        manifest,
        "governed_block_entries",
        "SDK governed projection block manifest entries are duplicated",
    )?;
    let prompt_entries = p7_projection_manifest_entry_set(
        manifest,
        "prompt_visible_entries",
        "SDK final prompt manifest entries are duplicated",
    )?;
    let final_capsules = p7_required_array(
        final_delivery,
        "rendered_capsules",
        "final rendered capsules missing",
    )?;
    let final_candidate_ids = final_capsules
        .iter()
        .map(|capsule| {
            p7_required_str(
                capsule,
                "candidate_id",
                "final rendered capsule candidate id missing",
            )
            .map(str::to_string)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let manifest_candidate_ids = capsule_entries
        .iter()
        .map(|(candidate_id, _)| candidate_id.clone())
        .collect::<BTreeSet<_>>();
    if final_candidate_ids.len() != final_capsules.len()
        || manifest_candidate_ids.len() != capsule_entries.len()
        || manifest_candidate_ids != final_candidate_ids
        || capsule_entries != governed_entries
        || capsule_entries != prompt_entries
    {
        return Err(p7_provenance_error(
            "SDK projection delivery digest sets are not bidirectionally exact",
        ));
    }
    if !p7_required_bool(
        manifest,
        "exact_render_match",
        "SDK deterministic projection render proof missing",
    )? || !p7_row_string_array(manifest, "integrity_failures")?.is_empty()
    {
        return Err(p7_provenance_error(
            "SDK deterministic projection render integrity failed",
        ));
    }
    let receipt_entries = p7_projection_manifest_receipt_set(manifest)?;
    let receipt_candidate_ids = receipt_entries
        .iter()
        .map(|(candidate_id, _)| candidate_id.clone())
        .collect::<BTreeSet<_>>();
    if receipt_candidate_ids.len() != receipt_entries.len()
        || receipt_candidate_ids != final_candidate_ids
    {
        return Err(p7_provenance_error(
            "SDK projection renderer receipts are not bidirectionally exact",
        ));
    }
    let system_digest = p7_required_str(
        manifest,
        "system_memory_block_sha256",
        "system memory block digest missing",
    )?;
    let deterministic_digest = p7_required_str(
        manifest,
        "deterministic_envelope_sha256",
        "deterministic projection envelope digest missing",
    )?;
    if !is_sha256(system_digest) || !is_sha256(deterministic_digest) {
        return Err(p7_provenance_error(
            "SDK projection envelope digest is invalid",
        ));
    }
    Ok(())
}

fn validate_p7_runner_projection_digest_observation(
    observation: &serde_json::Value,
    manifest: &serde_json::Value,
) -> Result<()> {
    let object = observation.as_object().ok_or_else(|| {
        p7_provenance_error("runner projection digest observation is not an object")
    })?;
    let expected_fields = [
        "schema_version",
        "system_memory_block_sha256",
        "runtime_envelope_sha256",
        "capsule_entries",
        "governed_block_entries",
        "prompt_visible_entries",
        "candidate_receipts",
    ];
    if object.len() != expected_fields.len()
        || expected_fields
            .iter()
            .any(|field| !object.contains_key(*field))
    {
        return Err(p7_provenance_error(
            "runner projection digest observation contains an unexpected or missing field",
        ));
    }
    if p7_required_str(
        observation,
        "schema_version",
        "runner projection digest observation schema missing",
    )? != P7_RUNNER_PROJECTION_DIGEST_OBSERVATION_SCHEMA_VERSION
    {
        return Err(p7_provenance_error(
            "runner projection digest observation schema mismatch",
        ));
    }

    let observed_capsules = p7_projection_observation_entry_set(observation, "capsule_entries")?;
    let observed_governed =
        p7_projection_observation_entry_set(observation, "governed_block_entries")?;
    let observed_prompt =
        p7_projection_observation_entry_set(observation, "prompt_visible_entries")?;
    let observed_receipts = p7_projection_observation_receipt_set(observation)?;
    let manifest_capsules = p7_projection_manifest_entry_set(
        manifest,
        "capsule_entries",
        "SDK delivery capsule manifest entries are duplicated",
    )?;
    let manifest_governed = p7_projection_manifest_entry_set(
        manifest,
        "governed_block_entries",
        "SDK governed projection block manifest entries are duplicated",
    )?;
    let manifest_prompt = p7_projection_manifest_entry_set(
        manifest,
        "prompt_visible_entries",
        "SDK final prompt manifest entries are duplicated",
    )?;
    let manifest_receipts = p7_projection_manifest_receipt_set(manifest)?;
    if observed_capsules != manifest_capsules
        || observed_governed != manifest_governed
        || observed_prompt != manifest_prompt
        || observed_receipts != manifest_receipts
    {
        return Err(p7_provenance_error(
            "runner content digest observation differs from the SDK projection manifest",
        ));
    }

    let observed_system = p7_required_str(
        observation,
        "system_memory_block_sha256",
        "runner system memory block digest missing",
    )?;
    let observed_envelope = p7_required_str(
        observation,
        "runtime_envelope_sha256",
        "runner runtime envelope digest missing",
    )?;
    let manifest_system = p7_required_str(
        manifest,
        "system_memory_block_sha256",
        "system memory block digest missing",
    )?;
    let manifest_envelope = p7_required_str(
        manifest,
        "deterministic_envelope_sha256",
        "deterministic projection envelope digest missing",
    )?;
    if !is_sha256(observed_system)
        || !is_sha256(observed_envelope)
        || observed_system != manifest_system
        || observed_envelope != manifest_envelope
    {
        return Err(p7_provenance_error(
            "runner system or envelope digest differs from the SDK projection manifest",
        ));
    }
    Ok(())
}

fn p7_projection_observation_entry_set(
    observation: &serde_json::Value,
    field: &str,
) -> Result<BTreeSet<(String, String)>> {
    let entries = p7_required_array(
        observation,
        field,
        "runner projection digest entries missing",
    )?;
    let mut entry_set = BTreeSet::new();
    for entry in entries {
        let object = entry.as_object().ok_or_else(|| {
            p7_provenance_error("runner projection digest entry is not an object")
        })?;
        if object.len() != 2
            || !object.contains_key("candidate_id")
            || !object.contains_key("content_sha256")
        {
            return Err(p7_provenance_error(
                "runner projection digest entry contains raw or unexpected data",
            ));
        }
        let candidate_id = p7_required_str(
            entry,
            "candidate_id",
            "runner projection digest candidate id missing",
        )?;
        let content_sha256 = p7_required_str(
            entry,
            "content_sha256",
            "runner projection content digest missing",
        )?;
        if !is_sha256(content_sha256)
            || !entry_set.insert((candidate_id.to_string(), content_sha256.to_string()))
        {
            return Err(p7_provenance_error(
                "runner projection digest entries are invalid or duplicated",
            ));
        }
    }
    Ok(entry_set)
}

fn p7_projection_observation_receipt_set(
    observation: &serde_json::Value,
) -> Result<BTreeSet<(String, String)>> {
    let entries = p7_required_array(
        observation,
        "candidate_receipts",
        "runner projection renderer receipts missing",
    )?;
    let mut entry_set = BTreeSet::new();
    for entry in entries {
        let object = entry.as_object().ok_or_else(|| {
            p7_provenance_error("runner projection renderer receipt is not an object")
        })?;
        if object.len() != 2
            || !object.contains_key("candidate_id")
            || !object.contains_key("source_block_sha256")
        {
            return Err(p7_provenance_error(
                "runner projection renderer receipt contains raw or unexpected data",
            ));
        }
        let candidate_id = p7_required_str(
            entry,
            "candidate_id",
            "runner projection renderer receipt candidate id missing",
        )?;
        let source_block_sha256 = p7_required_str(
            entry,
            "source_block_sha256",
            "runner projection renderer source block digest missing",
        )?;
        if !is_sha256(source_block_sha256)
            || !entry_set.insert((candidate_id.to_string(), source_block_sha256.to_string()))
        {
            return Err(p7_provenance_error(
                "runner projection renderer receipts are invalid or duplicated",
            ));
        }
    }
    Ok(entry_set)
}

fn p7_projection_manifest_receipt_set(
    manifest: &serde_json::Value,
) -> Result<BTreeSet<(String, String)>> {
    let entries = p7_required_array(
        manifest,
        "candidate_receipts",
        "SDK projection renderer receipts missing",
    )?;
    let mut entry_set = BTreeSet::new();
    for entry in entries {
        let candidate_id = p7_required_str(
            entry,
            "candidate_id",
            "SDK projection renderer receipt candidate id missing",
        )?;
        let source_block_sha256 = p7_required_str(
            entry,
            "source_block_sha256",
            "SDK projection renderer source block digest missing",
        )?;
        if !is_sha256(source_block_sha256)
            || !entry_set.insert((candidate_id.to_string(), source_block_sha256.to_string()))
        {
            return Err(p7_provenance_error(
                "SDK projection renderer receipts are invalid or duplicated",
            ));
        }
    }
    if entry_set.len() != entries.len() {
        return Err(p7_provenance_error(
            "SDK projection renderer receipts are invalid or duplicated",
        ));
    }
    Ok(entry_set)
}

fn p7_projection_manifest_entry_set(
    manifest: &serde_json::Value,
    field: &str,
    duplicate_error: &'static str,
) -> Result<BTreeSet<(String, String)>> {
    let entries = p7_required_array(
        manifest,
        field,
        "SDK projection delivery manifest entries missing",
    )?;
    let mut entry_set = BTreeSet::new();
    for entry in entries {
        let candidate_id = p7_required_str(
            entry,
            "candidate_id",
            "SDK projection delivery candidate id missing",
        )?;
        let content_sha256 = p7_required_str(
            entry,
            "content_sha256",
            "SDK projection delivery content digest missing",
        )?;
        if !is_sha256(content_sha256) {
            return Err(p7_provenance_error(
                "SDK projection delivery manifest contains an invalid content digest",
            ));
        }
        if !entry_set.insert((candidate_id.to_string(), content_sha256.to_string())) {
            return Err(p7_provenance_error(duplicate_error));
        }
    }
    if entry_set.len() != entries.len() {
        return Err(p7_provenance_error(duplicate_error));
    }
    Ok(entry_set)
}

fn validate_p7_shard_against_detail(
    shard: &serde_json::Value,
    aggregate: &P7DetailAggregate,
) -> Result<()> {
    validate_p7_detail_metrics(shard, aggregate)
}

fn validate_p7_summary_against_detail(
    summary: &W4ExternalNoisyBenchmarkSummary,
    aggregate: &P7DetailAggregate,
) -> Result<()> {
    let value = serde_json::to_value(summary).map_err(|source| Error::Other {
        source: Box::new(source),
        stage: "p7_provenance_serialize_verified_summary",
    })?;
    validate_p7_detail_metrics(&value, aggregate)
}

fn validate_p7_detail_metrics(
    claimed: &serde_json::Value,
    aggregate: &P7DetailAggregate,
) -> Result<()> {
    let scalar_claims = [
        ("samples", aggregate.samples),
        ("questions", aggregate.questions),
        ("evidence_questions", aggregate.evidence_questions),
        ("any_evidence_hit", aggregate.any_evidence_hit),
        ("all_evidence_hit", aggregate.all_evidence_hit),
        ("write_errors", aggregate.write_errors),
        ("recall_errors", aggregate.recall_errors),
    ];
    for (field, expected) in scalar_claims {
        if claimed.get(field).and_then(serde_json::Value::as_u64) != Some(expected as u64) {
            return Err(p7_provenance_error(
                "summary scalar does not match detail recomputation",
            ));
        }
    }
    let recomputed = [
        (
            "stage_hit_counts",
            serde_json::to_value(&aggregate.stage_hit_counts),
        ),
        (
            "index_diagnostics",
            serde_json::to_value(&aggregate.index_diagnostics),
        ),
        (
            "w4_1_diagnostics",
            serde_json::to_value(&aggregate.w4_1_diagnostics),
        ),
        (
            "facet_ablation",
            serde_json::to_value(&aggregate.facet_ablation),
        ),
        (
            "p7_loss_ledger",
            serde_json::to_value(&aggregate.p7_loss_ledger),
        ),
        (
            "p7_production_delivery",
            serde_json::to_value(&aggregate.p7_production_delivery),
        ),
    ];
    for (field, expected) in recomputed {
        let expected = expected.map_err(|source| Error::Other {
            source: Box::new(source),
            stage: "p7_provenance_serialize_recomputed_detail",
        })?;
        if claimed.get(field) != Some(&expected) {
            return Err(p7_provenance_error(
                "summary diagnostics do not match detail recomputation",
            ));
        }
    }
    Ok(())
}

fn p7_rendered_sources_from_delivery(report: &serde_json::Value) -> Result<Vec<String>> {
    Ok(p7_candidate_evidence_groups(
        &p7_rendered_candidates_from_delivery(report)?,
    ))
}

fn p7_rendered_candidates_from_delivery(
    report: &serde_json::Value,
) -> Result<Vec<P7CandidateEvidence>> {
    let mut candidates = Vec::new();
    let mut seen_candidate_ids = BTreeSet::new();
    for capsule in p7_required_array(
        report,
        "rendered_capsules",
        "final rendered capsules missing",
    )? {
        let candidate_id = p7_required_str(
            capsule,
            "candidate_id",
            "final rendered capsule candidate id missing",
        )?;
        if !seen_candidate_ids.insert(candidate_id.to_string()) {
            return Err(p7_provenance_error(
                "final rendered capsule candidate ids are duplicated",
            ));
        }
        candidates.push(P7CandidateEvidence {
            candidate_id: candidate_id.to_string(),
            canonical_evidence_groups: p7_canonical_groups(&p7_row_string_array(
                capsule,
                "canonical_evidence_groups",
            )?)
            .into_iter()
            .collect(),
        });
    }
    Ok(candidates)
}

fn p7_candidate_evidence_groups(candidates: &[P7CandidateEvidence]) -> Vec<String> {
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();
    for candidate in candidates {
        for group in &candidate.canonical_evidence_groups {
            let group = bm_core::memory::canonical_recall_evidence_group(group);
            if !group.is_empty() && seen.insert(group.clone()) {
                sources.push(group);
            }
        }
    }
    sources
}

fn p7_selected_sources_from_delivery(report: &serde_json::Value) -> Result<Vec<String>> {
    Ok(p7_candidate_evidence_groups(
        &p7_selected_candidates_from_delivery(report)?,
    ))
}

fn p7_selected_candidates_from_delivery(
    report: &serde_json::Value,
) -> Result<Vec<P7CandidateEvidence>> {
    let selected_ids = p7_row_string_array(report, "selected_candidate_ids")?;
    let selected_id_set = selected_ids.iter().cloned().collect::<BTreeSet<_>>();
    if selected_id_set.len() != selected_ids.len() {
        return Err(p7_provenance_error(
            "final delivery selected candidate ids are not unique",
        ));
    }
    let mut decision_ids = BTreeSet::new();
    let mut candidates = Vec::new();
    for decision in p7_required_array(
        report,
        "selection_decisions",
        "final delivery selection decisions missing",
    )? {
        if !p7_required_bool(decision, "selected", "selection decision flag missing")? {
            continue;
        }
        let candidate_id = p7_required_str(
            decision,
            "candidate_id",
            "selection decision candidate id missing",
        )?;
        if !decision_ids.insert(candidate_id.to_string()) {
            return Err(p7_provenance_error(
                "final delivery selected decision ids are not unique",
            ));
        }
        candidates.push(P7CandidateEvidence {
            candidate_id: candidate_id.to_string(),
            canonical_evidence_groups: p7_canonical_groups(&p7_row_string_array(
                decision,
                "canonical_evidence_groups",
            )?)
            .into_iter()
            .collect(),
        });
    }
    if decision_ids != selected_id_set {
        return Err(p7_provenance_error(
            "final delivery selected ids do not match selected decisions",
        ));
    }
    Ok(candidates)
}

fn p7_delivery_candidate_ids(report: &serde_json::Value) -> Result<Vec<String>> {
    p7_required_array(
        report,
        "rendered_capsules",
        "delivery rendered capsules missing",
    )?
    .iter()
    .map(|capsule| {
        p7_required_str(capsule, "candidate_id", "delivery candidate id missing")
            .map(str::to_string)
    })
    .collect()
}

fn p7_required_str<'a>(
    value: &'a serde_json::Value,
    field: &str,
    message: &'static str,
) -> Result<&'a str> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| p7_provenance_error(message))
}

fn p7_optional_str<'a>(
    value: &'a serde_json::Value,
    field: &str,
    message: &'static str,
) -> Result<Option<&'a str>> {
    match value.get(field) {
        Some(value) if value.is_null() => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| p7_provenance_error(message)),
        None => Err(p7_provenance_error(message)),
    }
}

fn p7_required_usize(
    value: &serde_json::Value,
    field: &str,
    message: &'static str,
) -> Result<usize> {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| p7_provenance_error(message))
}

fn p7_required_i64(value: &serde_json::Value, field: &str, message: &'static str) -> Result<i64> {
    value
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| p7_provenance_error(message))
}

fn p7_signed_usize_delta(baseline: usize, off_run: usize) -> Result<i64> {
    let baseline = i128::try_from(baseline)
        .map_err(|_| p7_provenance_error("ablation baseline count exceeds signed range"))?;
    let off_run = i128::try_from(off_run)
        .map_err(|_| p7_provenance_error("ablation off-run count exceeds signed range"))?;
    i64::try_from(baseline - off_run)
        .map_err(|_| p7_provenance_error("ablation numeric delta exceeds i64"))
}

fn p7_required_bool(value: &serde_json::Value, field: &str, message: &'static str) -> Result<bool> {
    value
        .get(field)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| p7_provenance_error(message))
}

fn p7_required_array<'a>(
    value: &'a serde_json::Value,
    field: &str,
    message: &'static str,
) -> Result<&'a Vec<serde_json::Value>> {
    value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| p7_provenance_error(message))
}

fn p7_string_array(value: &serde_json::Value, message: &'static str) -> Result<Vec<String>> {
    value
        .as_array()
        .ok_or_else(|| p7_provenance_error(message))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| p7_provenance_error(message))
        })
        .collect()
}

fn p7_row_string_array(value: &serde_json::Value, field: &str) -> Result<Vec<String>> {
    p7_string_array(
        value
            .get(field)
            .ok_or_else(|| p7_provenance_error("detail string array field missing"))?,
        "detail field must be a string array",
    )
}

#[cfg(test)]
fn p7_any_gold_hit(gold: &[String], actual: &[String]) -> bool {
    let gold_groups = p7_canonical_groups(gold);
    let actual_groups = p7_canonical_groups(actual);
    !gold_groups.is_empty() && !gold_groups.is_disjoint(&actual_groups)
}

#[cfg(test)]
fn p7_all_gold_hit(gold: &[String], actual: &[String]) -> bool {
    let gold_groups = p7_canonical_groups(gold);
    let actual_groups = p7_canonical_groups(actual);
    !gold_groups.is_empty() && gold_groups.is_subset(&actual_groups)
}

fn p7_canonical_groups(sources: &[String]) -> BTreeSet<String> {
    sources
        .iter()
        .map(|source| {
            let source = source.trim();
            let direct = bm_core::memory::canonical_recall_evidence_group(source);
            if source.starts_with("external_eval:") || direct == source.to_ascii_lowercase() {
                direct
            } else {
                bm_core::memory::canonical_recall_evidence_group(&format!("external_eval:{source}"))
            }
        })
        .filter(|group| !group.is_empty())
        .collect()
}

fn p7_stage_loss_groups(
    gold_groups: &BTreeSet<String>,
    upstream_groups: &BTreeSet<String>,
    downstream_groups: &BTreeSet<String>,
) -> BTreeSet<String> {
    gold_groups
        .intersection(upstream_groups)
        .filter(|group| !downstream_groups.contains(*group))
        .cloned()
        .collect()
}

fn p7_loss_entry_groups(entries: &[serde_json::Value]) -> Result<BTreeSet<String>> {
    let raw_groups = entries
        .iter()
        .map(|entry| {
            p7_required_str(
                entry,
                "canonical_evidence_group",
                "P7 loss entry canonical evidence group missing",
            )
            .map(str::to_string)
        })
        .collect::<Result<Vec<_>>>()?;
    let groups = raw_groups.iter().cloned().collect::<BTreeSet<_>>();
    if groups.len() != entries.len()
        || raw_groups
            .iter()
            .any(|group| group != &bm_core::memory::canonical_recall_evidence_group(group))
    {
        return Err(p7_provenance_error(
            "P7 loss ledger groups are not unique opaque canonical ids",
        ));
    }
    Ok(groups)
}

#[cfg(test)]
fn p7_gold_group_hit_count(gold_groups: &BTreeSet<String>, refs: &[String]) -> usize {
    let actual_groups = p7_canonical_groups(refs);
    gold_groups.intersection(&actual_groups).count()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct P7CandidateEvidence {
    candidate_id: String,
    canonical_evidence_groups: Vec<String>,
}

fn p7_stage_candidates(row: &serde_json::Value, field: &str) -> Result<Vec<P7CandidateEvidence>> {
    let reports = row
        .get("sdk_stage_candidates")
        .ok_or_else(|| p7_provenance_error("SDK stage candidate reports missing"))?;
    p7_candidate_evidence_array(
        reports
            .get(field)
            .ok_or_else(|| p7_provenance_error("SDK stage candidate report missing"))?,
    )
}

fn p7_ablation_candidates(
    slice: &serde_json::Value,
    field: &str,
) -> Result<Vec<P7CandidateEvidence>> {
    p7_candidate_evidence_array(
        slice
            .get(field)
            .ok_or_else(|| p7_provenance_error("ablation candidate-bound report missing"))?,
    )
}

fn p7_candidate_evidence_array(value: &serde_json::Value) -> Result<Vec<P7CandidateEvidence>> {
    let entries = value
        .as_array()
        .ok_or_else(|| p7_provenance_error("SDK candidate report must be an array"))?;
    let mut candidates = Vec::with_capacity(entries.len());
    let mut seen_candidate_ids = BTreeSet::new();
    for entry in entries {
        let candidate_id =
            p7_required_str(entry, "candidate_id", "SDK candidate report id missing")?;
        if !seen_candidate_ids.insert(candidate_id.to_string()) {
            return Err(p7_provenance_error("SDK candidate report id is duplicated"));
        }
        let raw_groups = p7_row_string_array(entry, "canonical_evidence_groups")?;
        let groups = raw_groups
            .iter()
            .map(|group| bm_core::memory::canonical_recall_evidence_group(group))
            .collect::<BTreeSet<_>>();
        if groups.len() != raw_groups.len()
            || raw_groups
                .iter()
                .any(|group| group != &bm_core::memory::canonical_recall_evidence_group(group))
        {
            return Err(p7_provenance_error(
                "SDK candidate report groups are not unique opaque canonical ids",
            ));
        }
        candidates.push(P7CandidateEvidence {
            candidate_id: candidate_id.to_string(),
            canonical_evidence_groups: groups.into_iter().collect(),
        });
    }
    Ok(candidates)
}

fn p7_candidate_evidence_map(
    candidates: &[P7CandidateEvidence],
) -> BTreeMap<String, BTreeSet<String>> {
    candidates
        .iter()
        .map(|candidate| {
            (
                candidate.candidate_id.clone(),
                candidate
                    .canonical_evidence_groups
                    .iter()
                    .cloned()
                    .collect(),
            )
        })
        .collect()
}

fn p7_affected_ablation_candidate_ids(
    baseline_selected: &[P7CandidateEvidence],
    off_selected: &[P7CandidateEvidence],
    baseline_rendered: &[P7CandidateEvidence],
    off_rendered: &[P7CandidateEvidence],
) -> BTreeSet<String> {
    let baseline_selected = p7_candidate_evidence_map(baseline_selected);
    let off_selected = p7_candidate_evidence_map(off_selected);
    let baseline_rendered = p7_candidate_evidence_map(baseline_rendered);
    let off_rendered = p7_candidate_evidence_map(off_rendered);
    baseline_selected
        .keys()
        .chain(off_selected.keys())
        .chain(baseline_rendered.keys())
        .chain(off_rendered.keys())
        .filter(|candidate_id| {
            baseline_selected.get(*candidate_id) != off_selected.get(*candidate_id)
                || baseline_rendered.get(*candidate_id) != off_rendered.get(*candidate_id)
        })
        .cloned()
        .collect()
}

fn p7_matched_gold_group_set(
    gold: &[String],
    candidates: &[P7CandidateEvidence],
) -> BTreeSet<String> {
    p7_match_gold_groups(gold, candidates)
        .into_iter()
        .map(|(_, group)| group)
        .collect()
}

fn validate_p7_stage_candidate_reports(row: &serde_json::Value) -> Result<()> {
    let evidence_index = p7_candidate_evidence_array(
        row.get("evidence_ref_index")
            .ok_or_else(|| p7_provenance_error("SDK safe evidence index missing"))?,
    )?;
    let evidence_by_candidate = p7_candidate_evidence_map(&evidence_index);
    for field in [
        "source",
        "expanded",
        "reranked",
        "eval_selected",
        "eval_rendered",
    ] {
        for candidate in p7_stage_candidates(row, field)? {
            if evidence_by_candidate.get(&candidate.candidate_id)
                != Some(
                    &candidate
                        .canonical_evidence_groups
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>(),
                )
            {
                return Err(p7_provenance_error(
                    "SDK stage candidate differs from the safe evidence index",
                ));
            }
        }
    }

    let eval_delivery = row
        .get("eval_delivery_report")
        .ok_or_else(|| p7_provenance_error("eval delivery report missing"))?;
    let final_delivery = row
        .get("final_projection_delivery_report")
        .ok_or_else(|| p7_provenance_error("final projection delivery report missing"))?;
    for (field, expected) in [
        (
            "eval_selected",
            p7_selected_candidates_from_delivery(eval_delivery)?,
        ),
        (
            "eval_rendered",
            p7_rendered_candidates_from_delivery(eval_delivery)?,
        ),
        (
            "projection_selected",
            p7_selected_candidates_from_delivery(final_delivery)?,
        ),
        (
            "final_rendered",
            p7_rendered_candidates_from_delivery(final_delivery)?,
        ),
    ] {
        if p7_candidate_evidence_map(&p7_stage_candidates(row, field)?)
            != p7_candidate_evidence_map(&expected)
        {
            return Err(p7_provenance_error(
                "SDK stage candidate report differs from delivery owner",
            ));
        }
    }

    for (field, stage) in [
        ("source_sources", "source"),
        ("expanded_sources", "expanded"),
        ("reranked_sources", "reranked"),
        ("selected_sources", "eval_selected"),
        ("projection_selected_sources", "projection_selected"),
        ("rendered_sources", "final_rendered"),
    ] {
        let diagnostic_groups = p7_row_string_array(row, field)?;
        if diagnostic_groups
            .iter()
            .any(|group| group != &bm_core::memory::canonical_recall_evidence_group(group))
            || p7_canonical_groups(&diagnostic_groups)
                != p7_candidate_evidence_groups(&p7_stage_candidates(row, stage)?)
                    .into_iter()
                    .collect()
        {
            return Err(p7_provenance_error(
                "diagnostic stage groups differ from raw SDK candidate reports",
            ));
        }
    }
    Ok(())
}

fn p7_match_gold_groups(
    gold: &[String],
    candidates: &[P7CandidateEvidence],
) -> Vec<(String, String)> {
    let gold_groups = p7_canonical_groups(gold).into_iter().collect::<Vec<_>>();
    let mut groups_by_candidate = BTreeMap::<String, BTreeSet<String>>::new();
    for candidate in candidates {
        if candidate.candidate_id.trim().is_empty() {
            continue;
        }
        let groups = groups_by_candidate
            .entry(candidate.candidate_id.clone())
            .or_default();
        for group in &candidate.canonical_evidence_groups {
            let group = bm_core::memory::canonical_recall_evidence_group(group);
            if !group.is_empty() {
                groups.insert(group);
            }
        }
    }
    let candidates = groups_by_candidate.into_iter().collect::<Vec<_>>();
    let mut owner_by_gold = vec![None; gold_groups.len()];
    for candidate_index in 0..candidates.len() {
        let mut visited_gold = vec![false; gold_groups.len()];
        p7_augment_gold_match(
            candidate_index,
            &candidates,
            &gold_groups,
            &mut owner_by_gold,
            &mut visited_gold,
        );
    }
    owner_by_gold
        .into_iter()
        .enumerate()
        .filter_map(|(gold_index, candidate_index)| {
            candidate_index.map(|candidate_index| {
                (
                    candidates[candidate_index].0.clone(),
                    gold_groups[gold_index].clone(),
                )
            })
        })
        .collect()
}

fn p7_augment_gold_match(
    candidate_index: usize,
    candidates: &[(String, BTreeSet<String>)],
    gold_groups: &[String],
    owner_by_gold: &mut [Option<usize>],
    visited_gold: &mut [bool],
) -> bool {
    for (gold_index, gold_group) in gold_groups.iter().enumerate() {
        if visited_gold[gold_index] || !candidates[candidate_index].1.contains(gold_group) {
            continue;
        }
        visited_gold[gold_index] = true;
        if owner_by_gold[gold_index].is_none_or(|owner| {
            p7_augment_gold_match(owner, candidates, gold_groups, owner_by_gold, visited_gold)
        }) {
            owner_by_gold[gold_index] = Some(candidate_index);
            return true;
        }
    }
    false
}

fn p7_increment(map: &mut BTreeMap<String, usize>, key: &str, value: usize) {
    let entry = map.entry(key.to_string()).or_default();
    *entry = entry.saturating_add(value);
}

fn p7_increment_i64(map: &mut BTreeMap<String, i64>, key: &str, value: i64) {
    let entry = map.entry(key.to_string()).or_default();
    *entry = entry.saturating_add(value);
}

fn add_usize_map(target: &mut BTreeMap<String, usize>, source: &BTreeMap<String, usize>) {
    for (key, value) in source {
        p7_increment(target, key, *value);
    }
}

fn add_i64_map(target: &mut BTreeMap<String, i64>, source: &BTreeMap<String, i64>) {
    for (key, value) in source {
        p7_increment_i64(target, key, *value);
    }
}

fn add_stage_hit_counts(
    target: &mut W4ExternalNoisyStageHitCounts,
    source: &W4ExternalNoisyStageHitCounts,
) {
    target.source_any_evidence_hit += source.source_any_evidence_hit;
    target.source_all_evidence_hit += source.source_all_evidence_hit;
    target.expanded_any_evidence_hit += source.expanded_any_evidence_hit;
    target.expanded_all_evidence_hit += source.expanded_all_evidence_hit;
    target.reranked_any_evidence_hit += source.reranked_any_evidence_hit;
    target.reranked_all_evidence_hit += source.reranked_all_evidence_hit;
    target.selected_any_evidence_hit += source.selected_any_evidence_hit;
    target.selected_all_evidence_hit += source.selected_all_evidence_hit;
    target.projection_selected_any_evidence_hit += source.projection_selected_any_evidence_hit;
    target.projection_selected_all_evidence_hit += source.projection_selected_all_evidence_hit;
    target.rendered_any_evidence_hit += source.rendered_any_evidence_hit;
    target.rendered_all_evidence_hit += source.rendered_all_evidence_hit;
}

fn add_index_diagnostics(
    target: &mut W4ExternalNoisyIndexDiagnostics,
    source: &W4ExternalNoisyIndexDiagnostics,
) {
    macro_rules! add_fields {
        ($($field:ident),+ $(,)?) => {
            $(target.$field = target.$field.saturating_add(source.$field);)+
        };
    }
    add_fields!(
        questions_with_index_report,
        index_used_questions,
        fallback_full_scan_questions,
        source_candidate_count,
        matched_source_anchor_count,
        unmatched_source_anchor_count,
        indexed_neighbor_count,
        filtered_node_count,
        filtered_edge_count,
        filtered_backlink_count,
        failure_count,
        graph_manifest_contract_verified_questions,
        graph_selected_dependency_chain_verified_questions,
        graph_full_scope_closure_verified_questions,
        graph_manifest_generation_present_questions,
        graph_revision_present_questions,
        graph_scope_digest_present_questions,
        graph_maintenance_required_questions,
        graph_incident_questions,
        graph_read_path_mutation_delta,
        facet_questions_with_index_report,
        facet_index_used_questions,
        facet_report_only_questions,
        facet_fallback_full_scan_questions,
        facet_source_candidate_count,
        facet_matched_source_candidate_count,
        facet_posting_key_lookup_count,
        facet_manifest_matched_posting_count,
        facet_posting_doc_read_count,
        facet_owner_key_lookup_count,
        facet_owner_doc_read_count,
        facet_zero_posting_key_lookup_questions,
        facet_clean_zero_hit_questions,
        facet_manifest_integrity_verified_questions,
        facet_manifest_integrity_failure_count,
        facet_exact_match_count,
        facet_expanded_match_count,
        facet_failure_count,
    );
}

fn add_w41_diagnostics(
    target: &mut W4ExternalNoisyW41Diagnostics,
    source: &W4ExternalNoisyW41Diagnostics,
) {
    target.questions_with_w4_1_diagnostics = target
        .questions_with_w4_1_diagnostics
        .saturating_add(source.questions_with_w4_1_diagnostics);
    add_usize_map(
        &mut target.first_any_hit_stage_counts,
        &source.first_any_hit_stage_counts,
    );
    add_usize_map(
        &mut target.first_all_hit_stage_counts,
        &source.first_all_hit_stage_counts,
    );
    add_usize_map(
        &mut target.missing_gold_by_stage_counts,
        &source.missing_gold_by_stage_counts,
    );
    target.miss_after_expanded_count = target
        .miss_after_expanded_count
        .saturating_add(source.miss_after_expanded_count);
    target.gold_rank_found_count = target
        .gold_rank_found_count
        .saturating_add(source.gold_rank_found_count);
    target.gold_rank_missing_count = target
        .gold_rank_missing_count
        .saturating_add(source.gold_rank_missing_count);
    target.gold_rank_sum = target.gold_rank_sum.saturating_add(source.gold_rank_sum);
    target.truncated_count = target
        .truncated_count
        .saturating_add(source.truncated_count);
    add_usize_map(
        &mut target.blocked_reason_counts,
        &source.blocked_reason_counts,
    );
    add_usize_map(
        &mut target.question_type_counts,
        &source.question_type_counts,
    );
    add_usize_map(
        &mut target.evidence_count_buckets,
        &source.evidence_count_buckets,
    );
    target.source_signature_count = target
        .source_signature_count
        .saturating_add(source.source_signature_count);
    target.repeated_source_signature_questions = target
        .repeated_source_signature_questions
        .saturating_add(source.repeated_source_signature_questions);
}

fn add_facet_ablation(
    target: &mut W4ExternalNoisyFacetAblationDiagnostics,
    source: &W4ExternalNoisyFacetAblationDiagnostics,
) {
    target.questions_with_ablation_report += source.questions_with_ablation_report;
    add_usize_map(&mut target.method_counts, &source.method_counts);
    target.delivery_contribution_proven_questions += source.delivery_contribution_proven_questions;
    target.render_growth += source.render_growth;
    add_usize_map(
        &mut target.required_slice_counts,
        &source.required_slice_counts,
    );
    add_usize_map(
        &mut target.report_available_slice_counts,
        &source.report_available_slice_counts,
    );
    add_usize_map(
        &mut target.delivery_contribution_proven_slice_counts,
        &source.delivery_contribution_proven_slice_counts,
    );
    target.delivery_affected_candidate_occurrences +=
        source.delivery_affected_candidate_occurrences;
    add_i64_map(
        &mut target.selected_evidence_hit_delta,
        &source.selected_evidence_hit_delta,
    );
    add_i64_map(
        &mut target.rendered_evidence_hit_delta,
        &source.rendered_evidence_hit_delta,
    );
    add_usize_map(
        &mut target.selected_all_hit_loss_count,
        &source.selected_all_hit_loss_count,
    );
    add_usize_map(
        &mut target.evidence_family_rotation_selected_all_hit_loss_count,
        &source.evidence_family_rotation_selected_all_hit_loss_count,
    );
    add_usize_map(
        &mut target.rendered_all_hit_loss_count,
        &source.rendered_all_hit_loss_count,
    );
    add_i64_map(
        &mut target.expanded_candidate_delta,
        &source.expanded_candidate_delta,
    );
    add_i64_map(
        &mut target.selected_candidate_delta,
        &source.selected_candidate_delta,
    );
    add_i64_map(
        &mut target.rendered_candidate_delta,
        &source.rendered_candidate_delta,
    );
    add_i64_map(&mut target.rendered_char_delta, &source.rendered_char_delta);
    add_usize_map(
        &mut target.blocked_reason_counts,
        &source.blocked_reason_counts,
    );
}

fn add_p7_loss(
    target: &mut W4ExternalNoisyP7LossDiagnostics,
    source: &W4ExternalNoisyP7LossDiagnostics,
) {
    target.questions_with_loss_ledger += source.questions_with_loss_ledger;
    target.expanded_hit_selected_miss_questions += source.expanded_hit_selected_miss_questions;
    target.eval_selected_hit_rendered_miss_questions +=
        source.eval_selected_hit_rendered_miss_questions;
    target.expanded_hit_selected_miss_evidence += source.expanded_hit_selected_miss_evidence;
    target.eval_selected_hit_rendered_miss_evidence +=
        source.eval_selected_hit_rendered_miss_evidence;
    target.eval_selected_hit_projection_selected_miss_questions +=
        source.eval_selected_hit_projection_selected_miss_questions;
    target.eval_selected_hit_projection_selected_miss_evidence +=
        source.eval_selected_hit_projection_selected_miss_evidence;
    target.selected_hit_final_rendered_miss_questions +=
        source.selected_hit_final_rendered_miss_questions;
    target.selected_hit_final_rendered_miss_evidence +=
        source.selected_hit_final_rendered_miss_evidence;
    target.eval_truncated_count += source.eval_truncated_count;
    add_usize_map(
        &mut target.eval_blocked_reason_counts,
        &source.eval_blocked_reason_counts,
    );
}

fn add_p7_production_delivery(
    target: &mut W4ExternalNoisyP7ProductionDeliveryDiagnostics,
    source: &W4ExternalNoisyP7ProductionDeliveryDiagnostics,
) {
    target.questions_with_delivery_report += source.questions_with_delivery_report;
    target.eval_selected_matches_delivery_questions +=
        source.eval_selected_matches_delivery_questions;
    target.eval_rendered_matches_delivery_questions +=
        source.eval_rendered_matches_delivery_questions;
    target.projection_selected_sources_proven_questions +=
        source.projection_selected_sources_proven_questions;
    target.projection_delivery_proof_questions += source.projection_delivery_proof_questions;
    target.final_projection_integrity_questions += source.final_projection_integrity_questions;
    target.final_projection_integrity_passed_questions +=
        source.final_projection_integrity_passed_questions;
    target.final_projection_raw_private_violation_count +=
        source.final_projection_raw_private_violation_count;
    target.final_projection_blocked_source_count += source.final_projection_blocked_source_count;
    target.final_projection_redacted_source_count += source.final_projection_redacted_source_count;
    add_usize_map(
        &mut target.schema_version_counts,
        &source.schema_version_counts,
    );
    target.render_growth += source.render_growth;
    target.privacy_leak_count += source.privacy_leak_count;
    target.cross_subject_leak_count += source.cross_subject_leak_count;
    target.raw_soul_private_material_count += source.raw_soul_private_material_count;
    add_usize_map(
        &mut target.blocked_reason_counts,
        &source.blocked_reason_counts,
    );
    add_usize_map(
        &mut target.delivery_drop_reason_counts,
        &source.delivery_drop_reason_counts,
    );
}

#[cfg(test)]
mod p7_operator_unit_tests {
    use super::*;

    fn expected_question(question_id: &str, question_index: usize) -> P7ExpectedQuestionIdentity {
        P7ExpectedQuestionIdentity {
            case_id: "case-1".to_string(),
            dataset_index: 0,
            question_index,
            question_id: question_id.to_string(),
            question: format!("question-{question_index}"),
            gold_sources: vec!["D1:1".to_string(), "D2:1".to_string()],
        }
    }

    fn detail_row(expected: &P7ExpectedQuestionIdentity) -> serde_json::Value {
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        let stage_diagnostics = serde_json::json!({
            "suite": "test_suite",
            "question_id": expected.question_id,
            "question_type": "multi_gold",
            "evidence_count": 2,
            "gold_evidence_refs": [first_group.clone(), second_group.clone()],
            "first_any_hit_stage": "expanded",
            "first_all_hit_stage": "expanded",
            "matched_gold_by_stage": [
                {"stage": "source", "evidence_refs": []},
                {"stage": "expanded", "evidence_refs": [first_group.clone(), second_group.clone()]},
                {"stage": "reranked", "evidence_refs": [first_group.clone(), second_group.clone()]},
                {"stage": "selected", "evidence_refs": [first_group.clone(), second_group.clone()]},
                {"stage": "rendered", "evidence_refs": [first_group.clone(), second_group.clone()]}
            ],
            "missing_gold_by_stage": [
                {"stage": "source", "evidence_refs": [first_group.clone(), second_group.clone()]},
                {"stage": "expanded", "evidence_refs": []},
                {"stage": "reranked", "evidence_refs": []},
                {"stage": "selected", "evidence_refs": []},
                {"stage": "rendered", "evidence_refs": []}
            ],
            "gold_rank_by_stage": [
                {"stage": "source", "evidence_ref": first_group.clone(), "rank": null},
                {"stage": "source", "evidence_ref": second_group.clone(), "rank": null},
                {"stage": "expanded", "evidence_ref": first_group.clone(), "rank": 1},
                {"stage": "expanded", "evidence_ref": second_group.clone(), "rank": 2},
                {"stage": "reranked", "evidence_ref": first_group.clone(), "rank": 1},
                {"stage": "reranked", "evidence_ref": second_group.clone(), "rank": 2},
                {"stage": "selected", "evidence_ref": first_group.clone(), "rank": 1},
                {"stage": "selected", "evidence_ref": second_group.clone(), "rank": 2},
                {"stage": "rendered", "evidence_ref": first_group.clone(), "rank": 1},
                {"stage": "rendered", "evidence_ref": second_group.clone(), "rank": 2}
            ],
            "miss_after_expanded": false,
            "truncated_count": 0,
            "blocked_reasons": [],
            "selected_candidate_ids": ["candidate-1", "candidate-2"],
            "rendered_candidate_ids": ["candidate-1", "candidate-2"]
        });
        let required_slices = [
            "facet_off",
            "rank_fusion_off",
            "coverage_selection_off",
            "delivery_relevance_fusion_off",
            "evidence_family_rotation_off",
            "render_capsule_off",
            "capsule_dedupe_off",
        ];
        let ablation_slices = required_slices
            .iter()
            .map(|name| ablation_slice(name))
            .collect::<Vec<_>>();
        let ablation_report = serde_json::json!({
            "method": "sdk_eval_recall_off_run_v1",
            "required_slices": required_slices,
            "delivery_contribution_proven": true,
            "render_growth": 0,
            "blocked_reasons": [],
            "slices": ablation_slices
        });
        let loss_ledger = serde_json::json!({
            "expanded_hit_selected_miss": [],
            "selected_hit_rendered_miss": [],
            "truncated_count": 0,
            "blocked_reasons": []
        });
        let graph_index_report = serde_json::json!({
            "used": false,
            "fallback_full_scan": false,
            "manifest_contract_verified": false,
            "selected_dependency_chain_verified": false,
            "full_scope_closure_verified": false,
            "manifest_generation_present": false,
            "graph_revision_present": false,
            "scope_digest_present": false,
            "maintenance_required": false,
            "incident_present": false,
            "read_path_mutation_delta": 0,
            "source_candidate_count": 0,
            "matched_source_anchor_count": 0,
            "unmatched_source_anchor_count": 0,
            "indexed_neighbor_count": 0,
            "index_doc_count": 0,
            "filtered_node_count": 0,
            "filtered_edge_count": 0,
            "filtered_backlink_count": 0,
            "failure_count": 0
        });
        let facet_index_report = serde_json::json!({
            "used": true,
            "report_only": false,
            "fallback_full_scan": false,
            "source_candidate_count": 0,
            "matched_source_candidate_count": 0,
            "posting_key_lookup_count": 1,
            "manifest_matched_posting_count": 1,
            "posting_doc_read_count": 1,
            "owner_key_lookup_count": 1,
            "owner_doc_read_count": 1,
            "exact_facet_match_count": 0,
            "expanded_facet_match_count": 0,
            "manifest_owner_doc_count": 0,
            "manifest_posting_doc_count": 0,
            "manifest_integrity_verified": true,
            "render_growth": 0,
            "failure_count": 0,
            "integrity_failure_count": 0
        });
        let index_diagnostics = serde_json::json!({
            "questions_with_index_report": 1,
            "index_used_questions": 0,
            "fallback_full_scan_questions": 0,
            "source_candidate_count": 0,
            "matched_source_anchor_count": 0,
            "unmatched_source_anchor_count": 0,
            "indexed_neighbor_count": 0,
            "filtered_node_count": 0,
            "filtered_edge_count": 0,
            "filtered_backlink_count": 0,
            "failure_count": 0,
            "graph_manifest_contract_verified_questions": 0,
            "graph_selected_dependency_chain_verified_questions": 0,
            "graph_full_scope_closure_verified_questions": 0,
            "graph_manifest_generation_present_questions": 0,
            "graph_revision_present_questions": 0,
            "graph_scope_digest_present_questions": 0,
            "graph_maintenance_required_questions": 0,
            "graph_incident_questions": 0,
            "graph_read_path_mutation_delta": 0,
            "facet_questions_with_index_report": 1,
            "facet_index_used_questions": 1,
            "facet_report_only_questions": 0,
            "facet_fallback_full_scan_questions": 0,
            "facet_source_candidate_count": 0,
            "facet_matched_source_candidate_count": 0,
            "facet_posting_key_lookup_count": 1,
            "facet_manifest_matched_posting_count": 1,
            "facet_posting_doc_read_count": 1,
            "facet_owner_key_lookup_count": 1,
            "facet_owner_doc_read_count": 1,
            "facet_zero_posting_key_lookup_questions": 0,
            "facet_clean_zero_hit_questions": 0,
            "facet_manifest_integrity_verified_questions": 1,
            "facet_manifest_integrity_failure_count": 0,
            "facet_exact_match_count": 0,
            "facet_expanded_match_count": 0,
            "facet_failure_count": 0
        });
        serde_json::json!({
            "suite": "test_suite",
            "run_id": "test-run",
            "case_id": expected.case_id,
            "dataset_index": expected.dataset_index,
            "question_index": expected.question_index,
            "question_id": expected.question_id,
            "question": expected.question,
            "gold_sources": [first_group.clone(), second_group.clone()],
            "selected_sources": [first_group.clone(), second_group.clone()],
            "projection_selected_sources": [first_group.clone(), second_group.clone()],
            "candidate_sources": [first_group.clone(), second_group.clone()],
            "source_sources": [],
            "expanded_sources": [first_group.clone(), second_group.clone()],
            "reranked_sources": [first_group.clone(), second_group.clone()],
            "rendered_sources": [first_group.clone(), second_group.clone()],
            "sdk_stage_candidates": {
                "source": [],
                "expanded": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ],
                "reranked": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ],
                "eval_selected": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ],
                "eval_rendered": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ],
                "projection_selected": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ],
                "final_rendered": [
                    {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                    {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
                ]
            },
            "graph_index_report": graph_index_report,
            "facet_index_report": facet_index_report,
            "index_diagnostics": index_diagnostics,
            "stage_diagnostics": stage_diagnostics,
            "ablation_report": ablation_report,
            "p7_loss_ledger": loss_ledger,
            "eval_delivery_report": delivery_report(),
            "final_projection_delivery_report": delivery_report(),
            "sdk_projection_delivery_manifest": projection_delivery_manifest(),
            "runner_projection_digest_observation": projection_delivery_observation(),
            "final_projection_integrity": {
                "checked_surfaces": ["prompt", "ui_api", "operator_raw", "gateway_raw_audit", "shared_fact_surface"],
                "surface_reports": [
                    {"surface": "prompt", "protected_exact_echo_count": 0, "forbidden_marker_count": 0, "violation_count": 0, "passed": true},
                    {"surface": "ui_api", "protected_exact_echo_count": 0, "forbidden_marker_count": 0, "violation_count": 0, "passed": true},
                    {"surface": "operator_raw", "protected_exact_echo_count": 0, "forbidden_marker_count": 0, "violation_count": 0, "passed": true},
                    {"surface": "gateway_raw_audit", "protected_exact_echo_count": 0, "forbidden_marker_count": 0, "violation_count": 0, "passed": true},
                    {"surface": "shared_fact_surface", "protected_exact_echo_count": 0, "forbidden_marker_count": 0, "violation_count": 0, "passed": true}
                ],
                "blocked_source_count": 0,
                "redacted_source_count": 0,
                "raw_private_violation_count": 0,
                "passed": true
            },
            "privacy_report": {
                "passed": true,
                "private_raw_candidate_count": 0,
                "failures": []
            },
            "projection_delivery_proven": true,
            "evidence_ref_index": [
                {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group]},
                {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group]}
            ],
            "candidate_score_breakdown": [],
            "any_evidence_hit": true,
            "all_evidence_hit": true,
            "write_error": null,
            "recall_error": null
        })
    }

    fn ablation_slice(name: &str) -> serde_json::Value {
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        serde_json::json!({
            "name": name,
            "feature_enabled": false,
            "report_available": true,
            "delivery_contribution_proven": true,
            "candidate_boundary_proven": true,
            "delivery_affected_candidate_ids": ["candidate-2"],
            "delivery_affected_candidate_count": 1,
            "sdk_delivery_affected_candidate_count_claim": 1,
            "baseline_selected_evidence_refs": [first_group.clone(), second_group.clone()],
            "off_run_selected_evidence_refs": [first_group.clone()],
            "baseline_rendered_evidence_refs": [first_group.clone(), second_group.clone()],
            "off_run_rendered_evidence_refs": [first_group.clone()],
            "baseline_selected_candidate_ids": ["candidate-1", "candidate-2"],
            "off_run_selected_candidate_ids": ["candidate-1"],
            "baseline_rendered_candidate_ids": ["candidate-1", "candidate-2"],
            "off_run_rendered_candidate_ids": ["candidate-1"],
            "baseline_selected_candidates": [
                {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
            ],
            "off_run_selected_candidates": [
                {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]}
            ],
            "baseline_rendered_candidates": [
                {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group.clone()]},
                {"candidate_id": "candidate-2", "canonical_evidence_groups": [second_group.clone()]}
            ],
            "off_run_rendered_candidates": [
                {"candidate_id": "candidate-1", "canonical_evidence_groups": [first_group]}
            ],
            "baseline_expanded_candidate_count": 2,
            "off_run_expanded_candidate_count": 2,
            "baseline_selected_candidate_count": 2,
            "off_run_selected_candidate_count": 1,
            "baseline_rendered_candidate_count": 2,
            "off_run_rendered_candidate_count": 1,
            "baseline_rendered_chars": 64,
            "off_run_rendered_chars": 32,
            "baseline_render_growth": 0,
            "off_run_render_growth": 0,
            "selected_evidence_hit_delta": 1,
            "rendered_evidence_hit_delta": 1,
            "selected_all_hit_lost": true,
            "rendered_all_hit_lost": true,
            "expanded_candidate_delta": 0,
            "selected_candidate_delta": 1,
            "rendered_candidate_delta": 1,
            "rendered_char_delta": 32,
            "render_growth": 0,
            "blocked_reasons": []
        })
    }

    fn delivery_report() -> serde_json::Value {
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        serde_json::json!({
            "schema_version": MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
            "owner": "sdk_recall_delivery",
            "selection_strategy": "profile_bounded_exact_evidence_coverage_with_relevance_fusion_v2",
            "render_strategy": "governed_evidence_capsule_v1",
            "selected_candidate_ids": ["candidate-1", "candidate-2"],
            "selection_decisions": [
                {
                    "candidate_id": "candidate-1",
                    "canonical_evidence_groups": [first_group.clone()],
                    "evidence_family_groups": [],
                    "selected": true,
                    "drop_reason": null
                },
                {
                    "candidate_id": "candidate-2",
                    "canonical_evidence_groups": [second_group.clone()],
                    "evidence_family_groups": [],
                    "selected": true,
                    "drop_reason": null
                }
            ],
            "rendered_capsules": [
                {
                    "candidate_id": "candidate-1",
                    "evidence_ref_views": [],
                    "visible_evidence_refs": [first_group.clone()],
                    "canonical_evidence_groups": [first_group],
                    "source_locator_view": {},
                    "redaction_state": "public_runtime",
                    "shared_fact_surface_allowed": true,
                    "rendered_chars": 10
                },
                {
                    "candidate_id": "candidate-2",
                    "evidence_ref_views": [],
                    "visible_evidence_refs": [second_group.clone()],
                    "canonical_evidence_groups": [second_group],
                    "source_locator_view": {},
                    "redaction_state": "public_runtime",
                    "shared_fact_surface_allowed": true,
                    "rendered_chars": 10
                }
            ],
            "covered_evidence_family_groups": [],
            "render_decisions": [],
            "render_budget_chars": 100,
            "rendered_chars": 20,
            "render_growth": 0,
            "integrity_failures": [],
            "delivery_drop_reasons": []
        })
    }

    fn projection_delivery_manifest() -> serde_json::Value {
        serde_json::json!({
            "schema_version": MEMORY_PROJECTION_DELIVERY_DIGEST_SCHEMA_VERSION,
            "system_memory_block_sha256": "3".repeat(64),
            "deterministic_envelope_sha256": "4".repeat(64),
            "exact_render_match": true,
            "capsule_entries": [
                {"candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "governed_block_entries": [
                {"candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "prompt_visible_entries": [
                {"candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "candidate_receipts": [
                {"candidate_id": "candidate-1", "source_block_sha256": "5".repeat(64)},
                {"candidate_id": "candidate-2", "source_block_sha256": "6".repeat(64)}
            ],
            "integrity_failures": []
        })
    }

    fn projection_delivery_observation() -> serde_json::Value {
        serde_json::json!({
            "schema_version": "p7_runner_projection_digest_observation_v1",
            "system_memory_block_sha256": "3".repeat(64),
            "runtime_envelope_sha256": "4".repeat(64),
            "capsule_entries": [
                {"candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "governed_block_entries": [
                {"candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "prompt_visible_entries": [
                {"candidate_id": "candidate-1", "content_sha256": "1".repeat(64)},
                {"candidate_id": "candidate-2", "content_sha256": "2".repeat(64)}
            ],
            "candidate_receipts": [
                {"candidate_id": "candidate-1", "source_block_sha256": "5".repeat(64)},
                {"candidate_id": "candidate-2", "source_block_sha256": "6".repeat(64)}
            ]
        })
    }

    fn remove_projection_manifest_entry(row: &mut serde_json::Value, index: usize) {
        for field in [
            "capsule_entries",
            "governed_block_entries",
            "prompt_visible_entries",
            "candidate_receipts",
        ] {
            row["sdk_projection_delivery_manifest"][field]
                .as_array_mut()
                .expect("projection delivery manifest entries")
                .remove(index);
        }
    }

    fn remove_projection_observation_entry(row: &mut serde_json::Value, index: usize) {
        for field in [
            "capsule_entries",
            "governed_block_entries",
            "prompt_visible_entries",
            "candidate_receipts",
        ] {
            row["runner_projection_digest_observation"][field]
                .as_array_mut()
                .expect("runner projection digest observation entries")
                .remove(index);
        }
    }

    fn write_detail(rows: &[serde_json::Value], name: &str) -> (PathBuf, String) {
        let path =
            std::env::temp_dir().join(format!("bm-p7-detail-{}-{name}.jsonl", std::process::id()));
        let mut bytes = Vec::new();
        for row in rows {
            bytes.extend(serde_json::to_vec(row).expect("serialize detail row"));
            bytes.push(b'\n');
        }
        fs::write(&path, &bytes).expect("write detail fixture");
        let digest = format!("{:x}", Sha256::digest(&bytes));
        (path, digest)
    }

    fn verify_rows(
        rows: &[serde_json::Value],
        expected: &[P7ExpectedQuestionIdentity],
        name: &str,
    ) -> Result<P7DetailAggregate> {
        let (path, digest) = write_detail(rows, name);
        let result = validate_p7_detail_file(
            &path,
            &digest,
            P7DetailValidationContext {
                suite: "test_suite",
                run_id: "test-run",
                expected_questions: expected,
                expected_samples: 1,
            },
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        );
        let _ = fs::remove_file(path);
        result
    }

    #[test]
    fn detail_recomputation_accepts_exact_final_projection_facts() {
        let expected = expected_question("q-1", 0);
        let aggregate = verify_rows(&[detail_row(&expected)], &[expected], "valid")
            .expect("exact detail should verify");

        assert_eq!(aggregate.questions, 1);
        assert_eq!(aggregate.stage_hit_counts.rendered_all_evidence_hit, 1);
        assert_eq!(
            aggregate
                .facet_ablation
                .evidence_family_rotation_selected_all_hit_loss_count
                .get("evidence_family_rotation_off"),
            Some(&1)
        );
        assert_eq!(
            aggregate
                .p7_production_delivery
                .projection_delivery_proof_questions,
            1
        );
    }

    #[test]
    fn detail_recomputation_rejects_cross_subject_shared_fact_eligibility() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["rendered_capsules"][0]["redaction_state"] =
            serde_json::json!("shared_with_subject");

        let aggregate = verify_rows(&[row], &[expected], "forged-shared-fact-eligibility")
            .expect("privacy violation must remain measurable for the release gate");

        assert_eq!(aggregate.p7_production_delivery.privacy_leak_count, 1);
    }

    #[test]
    fn detail_recomputation_requires_typed_shared_fact_eligibility() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["rendered_capsules"][0]
            .as_object_mut()
            .expect("rendered capsule")
            .remove("shared_fact_surface_allowed");

        assert!(verify_rows(&[row], &[expected], "missing-shared-fact-eligibility").is_err());
    }

    #[test]
    fn detail_recomputation_attributes_projection_only_loss_to_final_rendered() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["rendered_sources"] =
            serde_json::json!([bm_core::memory::canonical_recall_evidence_group(
                "external_eval:D1:1"
            )]);
        row["sdk_stage_candidates"]["final_rendered"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [
                bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1")
            ]
        }]);
        row["final_projection_delivery_report"]["rendered_capsules"]
            .as_array_mut()
            .expect("final rendered capsules")
            .pop();
        remove_projection_manifest_entry(&mut row, 1);
        remove_projection_observation_entry(&mut row, 1);
        row["all_evidence_hit"] = serde_json::json!(false);

        let aggregate = verify_rows(&[row], &[expected], "projection-only-loss")
            .expect("projection-only loss should be independently attributed");

        assert_eq!(
            aggregate
                .p7_loss_ledger
                .eval_selected_hit_rendered_miss_evidence,
            0
        );
        assert_eq!(
            aggregate
                .p7_loss_ledger
                .selected_hit_final_rendered_miss_questions,
            1
        );
        assert_eq!(
            aggregate
                .p7_loss_ledger
                .selected_hit_final_rendered_miss_evidence,
            1
        );
    }

    #[test]
    fn detail_recomputation_separates_eval_to_projection_selection_loss() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        row["projection_selected_sources"] = serde_json::json!([first_group.clone()]);
        row["rendered_sources"] = serde_json::json!([first_group.clone()]);
        let one_projection_candidate = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group]
        }]);
        row["sdk_stage_candidates"]["projection_selected"] = one_projection_candidate.clone();
        row["sdk_stage_candidates"]["final_rendered"] = one_projection_candidate;
        row["final_projection_delivery_report"]["selected_candidate_ids"] =
            serde_json::json!(["candidate-1"]);
        row["final_projection_delivery_report"]["selection_decisions"][1]["selected"] =
            serde_json::json!(false);
        row["final_projection_delivery_report"]["selection_decisions"][1]["drop_reason"] =
            serde_json::json!("profile_budget_exhausted");
        row["final_projection_delivery_report"]["rendered_capsules"]
            .as_array_mut()
            .expect("final rendered capsules")
            .pop();
        remove_projection_manifest_entry(&mut row, 1);
        remove_projection_observation_entry(&mut row, 1);
        row["all_evidence_hit"] = serde_json::json!(false);

        let aggregate = verify_rows(&[row], &[expected], "projection-selection-loss")
            .expect("projection selection loss should be independently attributed");

        assert_eq!(
            aggregate
                .p7_loss_ledger
                .eval_selected_hit_projection_selected_miss_evidence,
            1
        );
        assert_eq!(
            aggregate
                .p7_loss_ledger
                .selected_hit_final_rendered_miss_evidence,
            0
        );
    }

    #[test]
    fn detail_recomputation_rejects_duplicate_and_missing_identity() {
        let first = expected_question("q-1", 0);
        let second = expected_question("q-2", 1);
        let row = detail_row(&first);

        assert!(verify_rows(
            &[row.clone(), row.clone()],
            &[first.clone(), second.clone()],
            "duplicate"
        )
        .is_err());
        assert!(verify_rows(&[row], &[first, second], "missing").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_forged_multi_gold_refs() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["ablation_report"]["slices"][0]["off_run_selected_evidence_refs"] =
            serde_json::json!(["external_eval:D1:1", "external_eval:D2:1"]);

        assert!(verify_rows(&[row], &[expected], "multi-gold-forgery").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_one_ablation_candidate_claiming_two_golds() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        let forged_candidate = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group.clone(), second_group.clone()]
        }]);
        row["evidence_ref_index"][0]["canonical_evidence_groups"] =
            serde_json::json!([first_group.clone(), second_group.clone()]);
        let slice = &mut row["ablation_report"]["slices"][0];
        slice["off_run_selected_candidates"] = forged_candidate.clone();
        slice["off_run_rendered_candidates"] = forged_candidate;
        slice["off_run_selected_evidence_refs"] =
            serde_json::json!([first_group.clone(), second_group.clone()]);
        slice["off_run_rendered_evidence_refs"] = serde_json::json!([first_group, second_group]);
        slice["selected_evidence_hit_delta"] = serde_json::json!(0);
        slice["rendered_evidence_hit_delta"] = serde_json::json!(0);
        slice["selected_all_hit_lost"] = serde_json::json!(false);
        slice["rendered_all_hit_lost"] = serde_json::json!(false);
        slice["delivery_contribution_proven"] = serde_json::json!(false);
        slice["delivery_affected_candidate_ids"] =
            serde_json::json!(["candidate-1", "candidate-2"]);
        slice["delivery_affected_candidate_count"] = serde_json::json!(2);

        assert!(
            accumulate_p7_ablation(&mut P7DetailAggregate::default(), &row, &expected,).is_err()
        );
    }

    #[test]
    fn loss_recomputation_rejects_one_selected_candidate_claiming_two_golds() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        row["sdk_stage_candidates"]["eval_selected"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group, second_group]
        }]);

        assert!(accumulate_p7_loss(&mut P7DetailAggregate::default(), &row, &expected).is_err());
    }

    #[test]
    fn detail_recomputation_rejects_one_candidate_claiming_two_gold_groups() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        row["final_projection_delivery_report"]["selected_candidate_ids"] =
            serde_json::json!(["candidate-1"]);
        row["final_projection_delivery_report"]["selection_decisions"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group.clone(), second_group.clone()],
            "evidence_family_groups": [],
            "selected": true,
            "drop_reason": null
        }]);
        row["final_projection_delivery_report"]["rendered_capsules"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "evidence_ref_views": [],
            "visible_evidence_refs": [],
            "canonical_evidence_groups": [first_group.clone(), second_group.clone()],
            "source_locator_view": {},
            "redaction_state": "public_runtime",
            "shared_fact_surface_allowed": true,
            "rendered_chars": 20
        }]);
        row["projection_selected_sources"] =
            serde_json::json!([first_group.clone(), second_group.clone()]);
        row["rendered_sources"] = serde_json::json!([first_group, second_group]);
        remove_projection_manifest_entry(&mut row, 1);

        assert!(verify_rows(&[row], &[expected], "one-candidate-two-golds").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_forged_stage_arrays_when_raw_sdk_candidates_disagree() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["sdk_stage_candidates"]["eval_selected"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [
                bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1")
            ]
        }]);

        assert!(verify_rows(&[row], &[expected], "forged-stage-array").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_multi_gold_stage_claim_from_one_candidate() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        let one_candidate = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group.clone(), second_group.clone()]
        }]);
        row["evidence_ref_index"] = one_candidate.clone();
        for stage in ["expanded", "reranked", "eval_selected", "eval_rendered"] {
            row["sdk_stage_candidates"][stage] = one_candidate.clone();
        }
        for field in ["expanded_sources", "reranked_sources", "selected_sources"] {
            row[field] = serde_json::json!([first_group.clone(), second_group.clone()]);
        }
        row["eval_delivery_report"]["selected_candidate_ids"] = serde_json::json!(["candidate-1"]);
        row["eval_delivery_report"]["selection_decisions"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "canonical_evidence_groups": [first_group.clone(), second_group.clone()],
            "evidence_family_groups": [],
            "selected": true,
            "drop_reason": null
        }]);
        row["eval_delivery_report"]["rendered_capsules"] = serde_json::json!([{
            "candidate_id": "candidate-1",
            "evidence_ref_views": [],
            "visible_evidence_refs": [],
            "canonical_evidence_groups": [first_group, second_group],
            "source_locator_view": {},
            "redaction_state": "public_runtime",
            "shared_fact_surface_allowed": true,
            "rendered_chars": 20
        }]);
        row["stage_diagnostics"]["selected_candidate_ids"] = serde_json::json!(["candidate-1"]);
        row["stage_diagnostics"]["rendered_candidate_ids"] = serde_json::json!(["candidate-1"]);

        assert!(verify_rows(&[row], &[expected], "one-candidate-stage-all-hit").is_err());
    }

    #[test]
    fn detail_recomputation_requires_exact_disabled_ablation_slice_set() {
        let expected = expected_question("q-1", 0);

        let mut duplicate = detail_row(&expected);
        duplicate["ablation_report"]["slices"][6]["name"] = serde_json::json!("facet_off");
        assert!(verify_rows(
            &[duplicate],
            std::slice::from_ref(&expected),
            "duplicate-ablation-slice"
        )
        .is_err());

        let mut enabled = detail_row(&expected);
        enabled["ablation_report"]["slices"][0]["feature_enabled"] = serde_json::json!(true);
        assert!(verify_rows(
            &[enabled],
            std::slice::from_ref(&expected),
            "enabled-ablation-slice"
        )
        .is_err());

        let mut missing_raw_chars = detail_row(&expected);
        missing_raw_chars["ablation_report"]["slices"][0]
            .as_object_mut()
            .expect("ablation slice")
            .remove("off_run_rendered_chars");
        assert!(verify_rows(
            &[missing_raw_chars],
            &[expected],
            "missing-ablation-raw-chars"
        )
        .is_err());
    }

    #[test]
    fn canonical_exact_group_does_not_expand_one_composite_locator_into_two_golds() {
        let gold = vec!["D1:1".to_string(), "D2:1".to_string()];
        let composite = vec!["external_eval:D1:1|D2:1".to_string()];

        assert!(p7_any_gold_hit(&gold, &composite));
        assert!(!p7_all_gold_hit(&gold, &composite));
        assert_eq!(
            p7_gold_group_hit_count(&p7_canonical_groups(&gold), &composite),
            1
        );
    }

    #[test]
    fn final_delivery_join_keeps_opaque_canonical_groups_without_recovering_locators() {
        let opaque_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let report = serde_json::json!({
            "rendered_capsules": [{
                "candidate_id": "candidate-1",
                "canonical_evidence_groups": [opaque_group.clone()]
            }]
        });

        assert_eq!(
            p7_rendered_sources_from_delivery(&report).expect("opaque final delivery"),
            vec![opaque_group]
        );
    }

    #[test]
    fn deterministic_gold_matching_consumes_each_candidate_and_gold_at_most_once() {
        let gold = vec!["D1:1".to_string(), "D2:1".to_string()];
        let first_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D1:1");
        let second_group = bm_core::memory::canonical_recall_evidence_group("external_eval:D2:1");
        let multi_group_candidate = P7CandidateEvidence {
            candidate_id: "candidate-b".to_string(),
            canonical_evidence_groups: vec![first_group.clone(), second_group.clone()],
        };

        let one_candidate =
            p7_match_gold_groups(&gold, std::slice::from_ref(&multi_group_candidate));
        assert_eq!(one_candidate.len(), 1);

        let candidates = vec![
            multi_group_candidate,
            P7CandidateEvidence {
                candidate_id: "candidate-a".to_string(),
                canonical_evidence_groups: vec![first_group],
            },
        ];
        let mut reversed = candidates.clone();
        reversed.reverse();
        let expected = p7_match_gold_groups(&gold, &candidates);
        assert_eq!(expected.len(), 2);
        assert_eq!(p7_match_gold_groups(&gold, &reversed), expected);
    }

    #[test]
    fn detail_recomputation_rejects_forged_rendered_ablation_and_raw_numeric_facts() {
        let expected = expected_question("q-1", 0);
        let mut forged_rendered = detail_row(&expected);
        forged_rendered["ablation_report"]["slices"][0]["rendered_evidence_hit_delta"] =
            serde_json::json!(0);
        assert!(verify_rows(
            &[forged_rendered],
            std::slice::from_ref(&expected),
            "rendered-forgery"
        )
        .is_err());

        let mut forged_count = detail_row(&expected);
        forged_count["ablation_report"]["slices"][0]["off_run_rendered_candidate_count"] =
            serde_json::json!(2);
        assert!(verify_rows(
            &[forged_count],
            std::slice::from_ref(&expected),
            "count-forgery"
        )
        .is_err());

        let mut forged_growth = detail_row(&expected);
        forged_growth["ablation_report"]["slices"][0]["off_run_render_growth"] =
            serde_json::json!(1);
        assert!(verify_rows(&[forged_growth], &[expected], "growth-forgery").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_tampered_sdk_ablation_candidate_identity() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["ablation_report"]["slices"][0]["off_run_selected_candidate_ids"] =
            serde_json::json!(["candidate-2"]);

        assert!(verify_rows(
            &[row],
            std::slice::from_ref(&expected),
            "candidate-id-forgery"
        )
        .is_err());

        let mut count_claim = detail_row(&expected);
        count_claim["ablation_report"]["slices"][0]
            ["sdk_delivery_affected_candidate_count_claim"] = serde_json::json!(2);
        assert!(verify_rows(&[count_claim], &[expected], "candidate-count-forgery").is_err());
    }

    #[test]
    fn detail_recomputation_rejects_tampered_raw_index_report() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row)
            .expect("untampered raw reports");
        row["graph_index_report"]["source_candidate_count"] = serde_json::json!(999);

        assert!(accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row).is_err());
    }

    #[test]
    fn detail_recomputation_rejects_tampered_graph_v2_integrity_facts() {
        let expected = expected_question("q-1", 0);
        let mut selected_chain = detail_row(&expected);
        selected_chain["graph_index_report"]["used"] = serde_json::json!(true);
        selected_chain["index_diagnostics"]["index_used_questions"] = serde_json::json!(1);
        accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &selected_chain)
            .expect("untampered graph v2 diagnostics");
        selected_chain["graph_index_report"]["selected_dependency_chain_verified"] =
            serde_json::json!(true);
        assert!(accumulate_p7_index_diagnostics(
            &mut P7DetailAggregate::default(),
            &selected_chain
        )
        .is_err());

        let mut read_mutation = detail_row(&expected);
        read_mutation["graph_index_report"]["read_path_mutation_delta"] = serde_json::json!(1);
        assert!(
            accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &read_mutation)
                .is_err()
        );
    }

    #[test]
    fn unused_graph_metadata_is_not_counted_as_used_graph_proof() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["graph_index_report"]["manifest_contract_verified"] = serde_json::json!(true);
        row["graph_index_report"]["selected_dependency_chain_verified"] = serde_json::json!(true);
        row["graph_index_report"]["full_scope_closure_verified"] = serde_json::json!(true);
        row["graph_index_report"]["manifest_generation_present"] = serde_json::json!(true);
        row["graph_index_report"]["graph_revision_present"] = serde_json::json!(true);
        row["graph_index_report"]["scope_digest_present"] = serde_json::json!(true);

        accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row)
            .expect("unused graph metadata is informational, not a used-graph proof");
    }

    #[test]
    fn unused_facet_metadata_is_not_counted_as_used_facet_proof() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["facet_index_report"]["used"] = serde_json::json!(false);
        row["facet_index_report"]["posting_doc_read_count"] = serde_json::json!(0);
        row["facet_index_report"]["owner_doc_read_count"] = serde_json::json!(0);
        row["index_diagnostics"]["facet_index_used_questions"] = serde_json::json!(0);
        row["index_diagnostics"]["facet_posting_doc_read_count"] = serde_json::json!(0);
        row["index_diagnostics"]["facet_owner_doc_read_count"] = serde_json::json!(0);
        row["index_diagnostics"]["facet_manifest_integrity_verified_questions"] =
            serde_json::json!(0);

        accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row)
            .expect("unused facet metadata is informational, not a used-facet proof");
    }

    #[test]
    fn facet_zero_hit_requires_lookup_and_exact_manifest_read_proof() {
        let expected = expected_question("q-1", 0);
        let mut clean_zero_hit = detail_row(&expected);
        clean_zero_hit["facet_index_report"]["manifest_matched_posting_count"] =
            serde_json::json!(0);
        clean_zero_hit["facet_index_report"]["posting_doc_read_count"] = serde_json::json!(0);
        clean_zero_hit["facet_index_report"]["owner_key_lookup_count"] = serde_json::json!(0);
        clean_zero_hit["facet_index_report"]["owner_doc_read_count"] = serde_json::json!(0);
        clean_zero_hit["index_diagnostics"]["facet_manifest_matched_posting_count"] =
            serde_json::json!(0);
        clean_zero_hit["index_diagnostics"]["facet_posting_doc_read_count"] = serde_json::json!(0);
        clean_zero_hit["index_diagnostics"]["facet_owner_key_lookup_count"] = serde_json::json!(0);
        clean_zero_hit["index_diagnostics"]["facet_owner_doc_read_count"] = serde_json::json!(0);
        clean_zero_hit["index_diagnostics"]["facet_clean_zero_hit_questions"] =
            serde_json::json!(1);
        accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &clean_zero_hit)
            .expect("manifest-verified zero hit remains a used bounded lookup");

        let mut missing_manifest_posting = clean_zero_hit.clone();
        missing_manifest_posting["facet_index_report"]["manifest_matched_posting_count"] =
            serde_json::json!(1);
        assert!(accumulate_p7_index_diagnostics(
            &mut P7DetailAggregate::default(),
            &missing_manifest_posting,
        )
        .is_err());

        let mut no_lookup = clean_zero_hit;
        no_lookup["facet_index_report"]["posting_key_lookup_count"] = serde_json::json!(0);
        assert!(
            accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &no_lookup).is_err()
        );
    }

    #[test]
    fn graph_v2_raw_index_report_requires_every_safe_integrity_fact() {
        let expected = expected_question("q-1", 0);
        for field in [
            "manifest_contract_verified",
            "selected_dependency_chain_verified",
            "full_scope_closure_verified",
            "manifest_generation_present",
            "graph_revision_present",
            "scope_digest_present",
            "maintenance_required",
            "incident_present",
            "read_path_mutation_delta",
        ] {
            let mut row = detail_row(&expected);
            row["graph_index_report"]
                .as_object_mut()
                .expect("graph report")
                .remove(field);
            assert!(
                accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row).is_err(),
                "missing {field} must fail closed"
            );
        }
    }

    #[test]
    fn graph_v2_raw_index_report_rejects_sensitive_values() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["graph_index_report"]["scope_digest"] = serde_json::json!("not-safe-to-expose");

        assert!(accumulate_p7_index_diagnostics(&mut P7DetailAggregate::default(), &row).is_err());
    }

    #[test]
    fn graph_v2_index_diagnostics_are_additive() {
        let source = W4ExternalNoisyIndexDiagnostics {
            graph_manifest_contract_verified_questions: 1,
            graph_selected_dependency_chain_verified_questions: 2,
            graph_full_scope_closure_verified_questions: 3,
            graph_manifest_generation_present_questions: 4,
            graph_revision_present_questions: 5,
            graph_scope_digest_present_questions: 6,
            graph_maintenance_required_questions: 7,
            graph_incident_questions: 8,
            graph_read_path_mutation_delta: 9,
            ..W4ExternalNoisyIndexDiagnostics::default()
        };
        let mut aggregate = source.clone();

        add_index_diagnostics(&mut aggregate, &source);

        assert_eq!(aggregate.graph_manifest_contract_verified_questions, 2);
        assert_eq!(
            aggregate.graph_selected_dependency_chain_verified_questions,
            4
        );
        assert_eq!(aggregate.graph_full_scope_closure_verified_questions, 6);
        assert_eq!(aggregate.graph_manifest_generation_present_questions, 8);
        assert_eq!(aggregate.graph_revision_present_questions, 10);
        assert_eq!(aggregate.graph_scope_digest_present_questions, 12);
        assert_eq!(aggregate.graph_maintenance_required_questions, 14);
        assert_eq!(aggregate.graph_incident_questions, 16);
        assert_eq!(aggregate.graph_read_path_mutation_delta, 18);
    }

    #[test]
    fn graph_v2_index_diagnostics_reject_legacy_shape() {
        assert!(
            serde_json::from_value::<W4ExternalNoisyIndexDiagnostics>(serde_json::json!({
                "questions_with_index_report": 1,
                "index_used_questions": 1
            }))
            .is_err()
        );
    }

    #[test]
    fn summary_recomputation_rejects_tampered_graph_v2_aggregate() {
        let expected = expected_question("q-1", 0);
        let aggregate = verify_rows(&[detail_row(&expected)], &[expected], "graph-v2-summary")
            .expect("exact graph v2 detail");
        let mut claimed = serde_json::json!({
            "samples": aggregate.samples,
            "questions": aggregate.questions,
            "evidence_questions": aggregate.evidence_questions,
            "any_evidence_hit": aggregate.any_evidence_hit,
            "all_evidence_hit": aggregate.all_evidence_hit,
            "write_errors": aggregate.write_errors,
            "recall_errors": aggregate.recall_errors,
            "stage_hit_counts": aggregate.stage_hit_counts,
            "index_diagnostics": aggregate.index_diagnostics,
            "w4_1_diagnostics": aggregate.w4_1_diagnostics,
            "facet_ablation": aggregate.facet_ablation,
            "p7_loss_ledger": aggregate.p7_loss_ledger,
            "p7_production_delivery": aggregate.p7_production_delivery,
        });
        validate_p7_detail_metrics(&claimed, &aggregate).expect("exact graph v2 aggregate");
        claimed["index_diagnostics"]["graph_selected_dependency_chain_verified_questions"] =
            serde_json::json!(1);

        assert!(validate_p7_detail_metrics(&claimed, &aggregate).is_err());
    }

    #[test]
    fn graph_v2_release_conditions_require_selected_chain_but_not_full_scope() {
        let diagnostics_value = serde_json::json!({
            "questions_with_index_report": 2,
            "index_used_questions": 2,
            "fallback_full_scan_questions": 0,
            "source_candidate_count": 2,
            "matched_source_anchor_count": 2,
            "unmatched_source_anchor_count": 0,
            "indexed_neighbor_count": 2,
            "filtered_node_count": 0,
            "filtered_edge_count": 0,
            "filtered_backlink_count": 0,
            "failure_count": 0,
            "graph_manifest_contract_verified_questions": 2,
            "graph_selected_dependency_chain_verified_questions": 2,
            "graph_full_scope_closure_verified_questions": 0,
            "graph_manifest_generation_present_questions": 2,
            "graph_revision_present_questions": 2,
            "graph_scope_digest_present_questions": 2,
            "graph_maintenance_required_questions": 0,
            "graph_incident_questions": 0,
            "graph_read_path_mutation_delta": 0,
            "facet_questions_with_index_report": 2,
            "facet_index_used_questions": 2,
            "facet_report_only_questions": 0,
            "facet_fallback_full_scan_questions": 0,
            "facet_source_candidate_count": 2,
            "facet_matched_source_candidate_count": 2,
            "facet_posting_key_lookup_count": 2,
            "facet_manifest_matched_posting_count": 2,
            "facet_posting_doc_read_count": 2,
            "facet_owner_key_lookup_count": 2,
            "facet_owner_doc_read_count": 2,
            "facet_zero_posting_key_lookup_questions": 0,
            "facet_clean_zero_hit_questions": 0,
            "facet_manifest_integrity_verified_questions": 2,
            "facet_manifest_integrity_failure_count": 0,
            "facet_exact_match_count": 2,
            "facet_expanded_match_count": 0,
            "facet_failure_count": 0
        });
        let diagnostics =
            serde_json::from_value::<W4ExternalNoisyIndexDiagnostics>(diagnostics_value.clone())
                .expect("graph v2 diagnostics");
        let mut summary = W4ExternalNoisyBenchmarkSummary {
            questions: 2,
            index_diagnostics: Some(diagnostics.clone()),
            ..W4ExternalNoisyBenchmarkSummary::default()
        };

        assert!(index_diagnostics_show_index_effect(&diagnostics));
        assert!(w4_external_index_diagnostics_no_full_scan(&summary));

        for field in [
            "graph_manifest_contract_verified_questions",
            "graph_selected_dependency_chain_verified_questions",
            "graph_manifest_generation_present_questions",
            "graph_revision_present_questions",
            "graph_scope_digest_present_questions",
        ] {
            let mut invalid_value = diagnostics_value.clone();
            invalid_value[field] = serde_json::json!(1);
            let invalid: W4ExternalNoisyIndexDiagnostics =
                serde_json::from_value(invalid_value).expect("invalid graph diagnostics");
            summary.index_diagnostics = Some(invalid.clone());
            assert!(!index_diagnostics_show_index_effect(&invalid));
            assert!(!w4_external_index_diagnostics_no_full_scan(&summary));
        }

        for field in [
            "graph_maintenance_required_questions",
            "graph_incident_questions",
            "graph_read_path_mutation_delta",
        ] {
            let mut invalid_value = diagnostics_value.clone();
            invalid_value[field] = serde_json::json!(1);
            let invalid: W4ExternalNoisyIndexDiagnostics =
                serde_json::from_value(invalid_value).expect("unsafe graph diagnostics");
            summary.index_diagnostics = Some(invalid.clone());
            assert!(!index_diagnostics_show_index_effect(&invalid));
            assert!(!w4_external_index_diagnostics_no_full_scan(&summary));
        }

        let mut oracle_diagnostics = diagnostics;
        oracle_diagnostics.facet_index_used_questions = 1;
        oracle_diagnostics.facet_manifest_integrity_verified_questions = 1;
        let oracle = W4ExternalNoisyBenchmarkSummary {
            suite: "longmemeval_oracle".to_string(),
            questions: 2,
            index_diagnostics: Some(oracle_diagnostics),
            ..W4ExternalNoisyBenchmarkSummary::default()
        };
        assert!(w4_external_index_diagnostics_no_full_scan(&oracle));
    }

    #[test]
    fn detail_recomputation_rejects_tampered_w41_claim() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        accumulate_p7_w41_diagnostics(&mut P7DetailAggregate::default(), &row, &expected)
            .expect("untampered W4.1 detail");
        row["stage_diagnostics"]["first_any_hit_stage"] = serde_json::json!("source");

        assert!(
            accumulate_p7_w41_diagnostics(&mut P7DetailAggregate::default(), &row, &expected)
                .is_err()
        );
    }

    #[test]
    fn detail_recomputation_rejects_mixed_run_id() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["run_id"] = serde_json::json!("other-run");

        assert!(validate_p7_detail_identity(
            &row,
            "test_suite",
            "test-run",
            &expected,
            &mut BTreeSet::new(),
            &mut BTreeSet::new()
        )
        .is_err());
    }

    #[test]
    fn summary_recomputation_rejects_tampered_w41_aggregate() {
        let expected = expected_question("q-1", 0);
        let mut aggregate = P7DetailAggregate {
            samples: 1,
            questions: 1,
            evidence_questions: 1,
            ..P7DetailAggregate::default()
        };
        accumulate_p7_w41_diagnostics(&mut aggregate, &detail_row(&expected), &expected)
            .expect("exact W4.1 detail");
        let mut claimed = serde_json::json!({
            "samples": aggregate.samples,
            "questions": aggregate.questions,
            "evidence_questions": aggregate.evidence_questions,
            "any_evidence_hit": aggregate.any_evidence_hit,
            "all_evidence_hit": aggregate.all_evidence_hit,
            "write_errors": aggregate.write_errors,
            "recall_errors": aggregate.recall_errors,
            "stage_hit_counts": aggregate.stage_hit_counts,
            "index_diagnostics": aggregate.index_diagnostics,
            "w4_1_diagnostics": aggregate.w4_1_diagnostics,
            "facet_ablation": aggregate.facet_ablation,
            "p7_loss_ledger": aggregate.p7_loss_ledger,
            "p7_production_delivery": aggregate.p7_production_delivery,
        });
        validate_p7_detail_metrics(&claimed, &aggregate).expect("exact W4.1 aggregate");
        claimed["w4_1_diagnostics"]["gold_rank_sum"] = serde_json::json!(999);

        assert!(validate_p7_detail_metrics(&claimed, &aggregate).is_err());
    }

    #[test]
    fn sdk_build_fingerprint_uses_length_prefixed_contract_and_file_count() {
        let root =
            std::env::temp_dir().join(format!("bm-p7-sdk-fingerprint-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fingerprint root");
        let first = root.join("a");
        let second = root.join("bc");
        fs::write(&first, b"bc").expect("first input");
        fs::write(&second, b"a").expect("second input");

        let fingerprint = p7_fingerprint_files_with_contract(
            &root,
            &[first, second],
            P7_SDK_BUILD_FINGERPRINT_CONTRACT,
        )
        .expect("length-prefixed SDK fingerprint");

        assert_eq!(
            fingerprint,
            "032c6e5efb6729d27492bcca15f7938b6b50a265b6a527d44e6534911c5cbdbd"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compiled_sdk_fingerprint_matches_independent_disk_recomputation() {
        let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("SDK workspace root");
        let inputs =
            p7_fingerprint_inputs(sdk_root, &P7_SDK_BUILD_INPUTS).expect("SDK build inputs");
        let disk = p7_fingerprint_files_with_contract(
            sdk_root,
            &inputs,
            P7_SDK_BUILD_FINGERPRINT_CONTRACT,
        )
        .expect("SDK disk fingerprint");

        assert_eq!(disk, P7_TRUSTED_SDK_BUILD_FINGERPRINT);
    }

    #[test]
    fn merged_summary_path_is_bound_to_results_runs_cohort() {
        let root = std::env::temp_dir().join(format!("bm-p7-run-path-{}", std::process::id()));
        let valid = root
            .join("results/runs/run-a")
            .join("locomo.merged.summary.json");
        assert_eq!(
            p7_benchmark_root_for_run(&valid, "run-a").expect("valid run path"),
            root
        );
        assert!(p7_benchmark_root_for_run(&valid, "run-b").is_err());
        assert!(p7_benchmark_root_for_run(
            &root.join("results/locomo.merged.summary.json"),
            "run-a"
        )
        .is_err());
        assert!(p7_benchmark_root_for_run(&valid, "../run-a").is_err());
    }

    #[test]
    fn delivery_integrity_failures_and_drop_reasons_are_kept_separate() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["final_projection_delivery_report"]["integrity_failures"] =
            serde_json::json!(["projection_capsule_mismatch"]);
        row["final_projection_delivery_report"]["delivery_drop_reasons"] =
            serde_json::json!(["render_budget_exhausted"]);

        let aggregate = verify_rows(&[row], &[expected], "delivery-reasons")
            .expect("typed delivery reasons should verify");

        assert_eq!(
            aggregate
                .p7_production_delivery
                .blocked_reason_counts
                .get("projection_capsule_mismatch"),
            Some(&1)
        );
        assert_eq!(
            aggregate
                .p7_production_delivery
                .delivery_drop_reason_counts
                .get("render_budget_exhausted"),
            Some(&1)
        );
    }

    #[test]
    fn detail_recomputation_rejects_forged_final_projection_integrity() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        row["final_projection_integrity"]["raw_private_violation_count"] = serde_json::json!(1);

        assert!(verify_rows(&[row], &[expected], "projection-integrity-forgery").is_err());
    }

    #[test]
    fn detail_recomputation_independently_validates_sdk_projection_delivery_manifest() {
        let expected = expected_question("q-1", 0);

        let mut mismatched_prompt_set = detail_row(&expected);
        mismatched_prompt_set["sdk_projection_delivery_manifest"]["prompt_visible_entries"][1]
            ["content_sha256"] = serde_json::json!("4".repeat(64));
        assert!(verify_rows(
            &[mismatched_prompt_set],
            std::slice::from_ref(&expected),
            "projection-token-set-mismatch"
        )
        .is_err());

        let mut duplicate_capsule = detail_row(&expected);
        let first_capsule_entry =
            duplicate_capsule["sdk_projection_delivery_manifest"]["capsule_entries"][0].clone();
        duplicate_capsule["sdk_projection_delivery_manifest"]["capsule_entries"][1] =
            first_capsule_entry;
        assert!(verify_rows(
            &[duplicate_capsule],
            std::slice::from_ref(&expected),
            "projection-token-duplicate"
        )
        .is_err());

        let mut mismatched_final_delivery = detail_row(&expected);
        mismatched_final_delivery["sdk_projection_delivery_manifest"]["capsule_entries"][1]
            ["candidate_id"] = serde_json::json!("candidate-3");
        assert!(verify_rows(
            &[mismatched_final_delivery],
            std::slice::from_ref(&expected),
            "projection-final-delivery-mismatch"
        )
        .is_err());

        let mut forged_render_receipt = detail_row(&expected);
        forged_render_receipt["sdk_projection_delivery_manifest"]["exact_render_match"] =
            serde_json::json!(false);
        assert!(verify_rows(
            &[forged_render_receipt],
            std::slice::from_ref(&expected),
            "projection-render-receipt-forgery"
        )
        .is_err());

        let mut duplicate_receipt = detail_row(&expected);
        let first_receipt =
            duplicate_receipt["sdk_projection_delivery_manifest"]["candidate_receipts"][0].clone();
        duplicate_receipt["sdk_projection_delivery_manifest"]["candidate_receipts"][1] =
            first_receipt;
        assert!(verify_rows(
            &[duplicate_receipt],
            std::slice::from_ref(&expected),
            "projection-render-receipt-duplicate"
        )
        .is_err());

        let mut forged_runner_bool = detail_row(&expected);
        forged_runner_bool["projection_delivery_proven"] = serde_json::json!(false);
        assert!(verify_rows(
            &[forged_runner_bool],
            &[expected],
            "projection-runner-bool-forgery"
        )
        .is_err());
    }

    #[test]
    fn detail_recomputation_rejects_self_consistent_sdk_manifest_without_runner_content_proof() {
        let expected = expected_question("q-1", 0);
        let mut row = detail_row(&expected);
        for field in [
            "capsule_entries",
            "governed_block_entries",
            "prompt_visible_entries",
        ] {
            row["sdk_projection_delivery_manifest"][field][0]["content_sha256"] =
                serde_json::json!("7".repeat(64));
            row["sdk_projection_delivery_manifest"][field][1]["content_sha256"] =
                serde_json::json!("8".repeat(64));
        }
        row["sdk_projection_delivery_manifest"]["candidate_receipts"][0]["source_block_sha256"] =
            serde_json::json!("9".repeat(64));
        row["sdk_projection_delivery_manifest"]["candidate_receipts"][1]["source_block_sha256"] =
            serde_json::json!("a".repeat(64));
        row["sdk_projection_delivery_manifest"]["system_memory_block_sha256"] =
            serde_json::json!("b".repeat(64));
        row["sdk_projection_delivery_manifest"]["deterministic_envelope_sha256"] =
            serde_json::json!("c".repeat(64));

        assert!(verify_rows(
            &[row],
            std::slice::from_ref(&expected),
            "projection-self-consistent-sdk-manifest-forgery"
        )
        .is_err());

        let mut missing_observation = detail_row(&expected);
        missing_observation
            .as_object_mut()
            .expect("detail row")
            .remove("runner_projection_digest_observation");
        assert!(verify_rows(
            &[missing_observation],
            std::slice::from_ref(&expected),
            "projection-runner-observation-missing"
        )
        .is_err());

        let mut raw_observation = detail_row(&expected);
        raw_observation["runner_projection_digest_observation"]["raw_content"] =
            serde_json::json!("private capsule content");
        assert!(verify_rows(
            &[raw_observation],
            &[expected],
            "projection-runner-observation-raw-content"
        )
        .is_err());
    }

    #[test]
    fn detail_recomputation_rejects_forged_or_duplicate_surface_integrity() {
        let expected = expected_question("q-1", 0);
        let mut forged_arithmetic = detail_row(&expected);
        forged_arithmetic["final_projection_integrity"]["surface_reports"][0]
            ["protected_exact_echo_count"] = serde_json::json!(1);
        assert!(verify_rows(
            &[forged_arithmetic],
            std::slice::from_ref(&expected),
            "surface-arithmetic-forgery"
        )
        .is_err());

        let mut duplicate_surface = detail_row(&expected);
        duplicate_surface["final_projection_integrity"]["surface_reports"][1]["surface"] =
            serde_json::json!("prompt");
        assert!(verify_rows(
            &[duplicate_surface],
            &[expected],
            "duplicate-integrity-surface"
        )
        .is_err());
    }

    #[test]
    fn detail_recomputation_rejects_forged_summary_counter() {
        let expected = expected_question("q-1", 0);
        let aggregate = verify_rows(&[detail_row(&expected)], &[expected], "summary")
            .expect("exact detail should verify");
        let mut claimed = serde_json::json!({
            "samples": 1,
            "questions": 1,
            "evidence_questions": 1,
            "any_evidence_hit": 2,
            "all_evidence_hit": 1,
            "write_errors": 0,
            "recall_errors": 0,
            "stage_hit_counts": aggregate.stage_hit_counts,
            "w4_1_diagnostics": aggregate.w4_1_diagnostics,
            "facet_ablation": aggregate.facet_ablation,
            "p7_loss_ledger": aggregate.p7_loss_ledger,
            "p7_production_delivery": aggregate.p7_production_delivery,
            "index_diagnostics": aggregate.index_diagnostics,
        });

        assert!(validate_p7_detail_metrics(&claimed, &aggregate).is_err());
        claimed["any_evidence_hit"] = serde_json::json!(1);
        assert!(validate_p7_detail_metrics(&claimed, &aggregate).is_ok());
    }

    #[test]
    fn production_gate_requires_projection_proof_for_every_question() {
        let mut summary = W4ExternalNoisyBenchmarkSummary {
            questions: 2,
            p7_production_delivery: Some(W4ExternalNoisyP7ProductionDeliveryDiagnostics {
                questions_with_delivery_report: 2,
                eval_selected_matches_delivery_questions: 2,
                eval_rendered_matches_delivery_questions: 2,
                projection_selected_sources_proven_questions: 2,
                projection_delivery_proof_questions: 1,
                final_projection_integrity_questions: 2,
                final_projection_integrity_passed_questions: 2,
                schema_version_counts: [(MEMORY_RECALL_DELIVERY_SCHEMA_VERSION.to_string(), 2)]
                    .into_iter()
                    .collect(),
                ..W4ExternalNoisyP7ProductionDeliveryDiagnostics::default()
            }),
            ..W4ExternalNoisyBenchmarkSummary::default()
        };

        assert!(!p7_production_delivery_covers_summary(&summary));
        summary
            .p7_production_delivery
            .as_mut()
            .expect("delivery diagnostics")
            .projection_delivery_proof_questions = 2;
        assert!(p7_production_delivery_covers_summary(&summary));
    }

    #[test]
    fn release_identity_accepts_only_the_supplied_trust_anchor() {
        let dataset = P7TrustedDataset {
            suite: "test_suite",
            file_name: "test.json",
            input_sha256: "1f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
        };
        let runner = P7TrustedRunnerRelease {
            runner_build_fingerprint:
                "2f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            runner_lock_fingerprint:
                "3f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
            executable_sha256: "4f3d4f4c5e6a708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
        };
        let mut provenance = W4ExternalNoisyP7Provenance {
            run_id: "test-run".to_string(),
            contract_version: P7_CONTRACT_VERSION.to_string(),
            sdk_report_schema_version: MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
            sdk_build_fingerprint: P7_TRUSTED_SDK_BUILD_FINGERPRINT.to_string(),
            runner_build_fingerprint: runner.runner_build_fingerprint.to_string(),
            runner_lock_fingerprint: runner.runner_lock_fingerprint.to_string(),
            executable_sha256: runner.executable_sha256.to_string(),
            build_profile: "release".to_string(),
            input_sha256: dataset.input_sha256.to_string(),
            merged_detail_sha256: "5".repeat(64),
            ordered_shard_digest_manifest: Vec::new(),
        };

        assert!(validate_p7_release_identity(
            &provenance,
            dataset,
            runner,
            runner.executable_sha256,
        )
        .is_ok());
        assert!(
            validate_p7_release_identity(&provenance, dataset, runner, &"6".repeat(64)).is_err()
        );
        provenance.runner_build_fingerprint = "6".repeat(64);
        assert!(validate_p7_release_identity(
            &provenance,
            dataset,
            runner,
            runner.executable_sha256,
        )
        .is_err());
    }

    #[test]
    fn p7_preflight_report_requires_run_id() {
        let report = serde_json::json!({
            "sdk_build_fingerprint": "a".repeat(64),
            "runner_build_fingerprint": "b".repeat(64),
            "runner_lock_fingerprint": "c".repeat(64),
            "executable_sha256": "d".repeat(64),
            "build_profile": "release"
        });

        assert!(serde_json::from_value::<P7RunnerPreflightReport>(report).is_err());
    }

    #[test]
    fn runner_disk_identity_tracks_exact_build_inputs_lock_and_executable() {
        let root =
            std::env::temp_dir().join(format!("bm-p7-runner-identity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let runner = root.join("runner");
        fs::create_dir_all(runner.join("src")).expect("runner src");
        fs::create_dir_all(runner.join("target/release")).expect("runner release target");
        fs::write(runner.join("Cargo.toml"), b"[package]\nname='fixture'\n")
            .expect("runner manifest");
        fs::write(runner.join("Cargo.lock"), b"lock-v1\n").expect("runner lock");
        fs::write(runner.join("build.rs"), b"fn main() {}\n").expect("runner build script");
        fs::write(runner.join("src/main.rs"), b"fn main() {}\n").expect("runner source");
        fs::write(
            runner.join("target/release/beetle-memory-external-bench-runner"),
            b"binary-v1\n",
        )
        .expect("runner executable");
        let original = p7_runner_disk_identity_at_root(&root).expect("initial runner identity");
        assert_eq!(
            original.runner_build_fingerprint,
            "786c4a5f4d6d141bbb063b41347b3082031c4ef98b67b9fc7a7367b689c4db74"
        );
        assert_eq!(
            original.runner_lock_fingerprint,
            format!("{:x}", Sha256::digest(b"lock-v1\n"))
        );
        let producer = W4ExternalNoisyP7Provenance {
            runner_build_fingerprint: original.runner_build_fingerprint.clone(),
            runner_lock_fingerprint: original.runner_lock_fingerprint.clone(),
            executable_sha256: original.executable_sha256.clone(),
            ..W4ExternalNoisyP7Provenance::default()
        };
        validate_p7_runner_disk_provenance(&producer, &original)
            .expect("producer must match the exact runner bytes");

        fs::create_dir_all(runner.join("target/debug")).expect("debug target");
        fs::write(runner.join("target/debug/noise"), b"not a build input").expect("target noise");
        assert_eq!(
            p7_runner_disk_identity_at_root(&root).expect("identity after target noise"),
            original
        );

        fs::write(
            runner.join("src/main.rs"),
            b"fn main() { println!(\"v2\"); }\n",
        )
        .expect("changed runner source");
        let source_changed = p7_runner_disk_identity_at_root(&root).expect("source identity");
        assert_ne!(
            source_changed.runner_build_fingerprint,
            original.runner_build_fingerprint
        );
        assert_eq!(
            source_changed.runner_lock_fingerprint,
            original.runner_lock_fingerprint
        );
        assert_eq!(source_changed.executable_sha256, original.executable_sha256);
        assert!(validate_p7_runner_disk_provenance(&producer, &source_changed).is_err());

        fs::write(runner.join("Cargo.lock"), b"lock-v2\n").expect("changed runner lock");
        let lock_changed = p7_runner_disk_identity_at_root(&root).expect("lock identity");
        assert_ne!(
            lock_changed.runner_build_fingerprint,
            source_changed.runner_build_fingerprint
        );
        assert_ne!(
            lock_changed.runner_lock_fingerprint,
            source_changed.runner_lock_fingerprint
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn runner_preflight_rejects_stale_embedded_executable_identity() {
        use std::os::unix::fs::PermissionsExt;

        let root =
            std::env::temp_dir().join(format!("bm-p7-stale-identity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let runner = root.join("runner");
        fs::create_dir_all(runner.join("src")).expect("runner src");
        fs::create_dir_all(runner.join("target/release")).expect("runner target");
        fs::write(runner.join("Cargo.toml"), b"[package]\nname='fixture'\n")
            .expect("runner manifest");
        fs::write(runner.join("Cargo.lock"), b"lock-v1\n").expect("runner lock");
        fs::write(runner.join("build.rs"), b"fn main() {}\n").expect("runner build script");
        fs::write(runner.join("src/main.rs"), b"fn main() {}\n").expect("runner source");
        let inputs =
            p7_fingerprint_inputs(&runner, &P7_RUNNER_BUILD_INPUTS).expect("runner build inputs");
        let runner_build = p7_fingerprint_files_with_contract(
            &runner,
            &inputs,
            P7_RUNNER_BUILD_FINGERPRINT_CONTRACT,
        )
        .expect("runner build fingerprint");
        let runner_lock = p7_sha256_file(&runner.join("Cargo.lock")).expect("runner lock digest");
        let marker = root.join("identity-command-executed");
        let executable = runner.join("target/release/beetle-memory-external-bench-runner");
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\n: > '{}'\nprintf '%s\\n' '{{\"sdk_build_fingerprint\":\"{}\",\"runner_build_fingerprint\":\"{}\",\"runner_lock_fingerprint\":\"{}\",\"build_profile\":\"release\",\"executable_sha256\":\"{}\"}}'\n",
                marker.display(),
                P7_TRUSTED_SDK_BUILD_FINGERPRINT,
                runner_build,
                runner_lock,
                "0".repeat(64),
            ),
        )
        .expect("stale identity executable");
        let mut permissions = fs::metadata(&executable)
            .expect("runner metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("runner executable permissions");
        let disk = p7_runner_disk_identity_at_root(&root).expect("runner disk identity");
        let trusted = P7TrustedRunnerRelease {
            runner_build_fingerprint: Box::leak(
                disk.runner_build_fingerprint.clone().into_boxed_str(),
            ),
            runner_lock_fingerprint: Box::leak(
                disk.runner_lock_fingerprint.clone().into_boxed_str(),
            ),
            executable_sha256: Box::leak(disk.executable_sha256.into_boxed_str()),
        };
        let sdk_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("SDK root");

        assert!(
            preflight_p7_runner_release_with_trusted(&root, sdk_root, trusted, "test-run").is_err()
        );
        assert!(marker.is_file(), "trusted runner identity was not executed");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn release_shard_full_run_coordinates_reject_question_index() {
        let mut shard = serde_json::json!({
            "limit": null,
            "question_limit": null,
            "question_index": null
        });
        validate_p7_release_shard_full_run(&shard).expect("full release shard");

        shard["question_index"] = serde_json::json!(0);
        assert!(validate_p7_release_shard_full_run(&shard).is_err());
    }

    #[test]
    fn trusted_runner_anchor_is_structurally_valid_when_frozen() {
        if let Some(anchor) = P7_TRUSTED_RUNNER_RELEASE {
            assert!(is_sha256(anchor.runner_build_fingerprint));
            assert!(is_sha256(anchor.runner_lock_fingerprint));
            assert!(is_sha256(anchor.executable_sha256));
        }
    }

    #[test]
    fn dataset_stream_rebuilds_identity_and_hashes_exact_bytes() {
        let path = std::env::temp_dir().join(format!("bm-p7-dataset-{}.json", std::process::id()));
        let bytes = br#"[{"question_id":"q-1","question":"Q","answer_session_ids":["D1"]}]"#;
        fs::write(&path, bytes).expect("write dataset fixture");
        let dataset = P7TrustedDataset {
            suite: "longmemeval_oracle",
            file_name: "fixture.json",
            input_sha256: "84c317644a0265265c91c7e13510dc5cd36c6634532904ee1484f2dbbc26bc00",
        };

        let expected =
            load_p7_dataset_expectation(&path, dataset, 1).expect("dataset should verify");
        assert_eq!(expected.samples_by_shard, vec![1]);
        assert_eq!(expected.questions_by_shard[0][0].question_id, "q-1");

        fs::write(&path, [bytes.as_slice(), b"\n"].concat()).expect("tamper dataset bytes");
        assert!(load_p7_dataset_expectation(&path, dataset, 1).is_err());
        let _ = fs::remove_file(path);
    }
}

fn p7_provenance_error(message: &'static str) -> Error {
    Error::Io {
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, message),
        stage: "p7_provenance_verify_files",
    }
}

fn p7_preflight_error(message: &'static str) -> Error {
    Error::Io {
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, message),
        stage: "p7_runner_preflight",
    }
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
            .collect::<Vec<_>>();
        let actual_names = summary
            .shards
            .iter()
            .map(|shard| shard.trim().to_string())
            .collect::<Vec<_>>();
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
        run_id: summary.run_id.clone(),
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
        p7_loss_ledger: summary.p7_loss_ledger.clone(),
        p7_production_delivery: summary.p7_production_delivery.clone(),
        p7_provenance: summary.p7_provenance.clone(),
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
    diagnostics.questions_with_ablation_report == summary.questions
        && diagnostics
            .method_counts
            .get(P7_ABLATION_METHOD)
            .copied()
            .unwrap_or(0)
            == summary.questions
        && P7_REQUIRED_ABLATION_SLICES.iter().all(|slice| {
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

fn p7_loss_ledger_covers_summary(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    summary.p7_loss_ledger.as_ref().is_some_and(|diagnostics| {
        diagnostics.questions_with_loss_ledger == summary.questions
            && diagnostics.questions_with_loss_ledger > 0
            && diagnostics.eval_truncated_count == 0
            && diagnostics.eval_blocked_reason_counts.is_empty()
    })
}

fn p7_suite_quality_threshold_met(
    summaries: &[W4ExternalNoisyBenchmarkSummary],
    suite: &str,
    selected_stage: bool,
) -> bool {
    let Some(summary) = summaries.iter().find(|summary| summary.suite == suite) else {
        return false;
    };
    let Some(stage) = summary.stage_hit_counts.as_ref() else {
        return false;
    };
    let (minimum_any, minimum_all) = match (suite, selected_stage) {
        ("locomo", true) => (931, 818),
        ("longmemeval_s_cleaned", true) => (475, 405),
        ("longmemeval_m_cleaned", true) => (225, 124),
        ("locomo", false) => (139, 111),
        ("longmemeval_s_cleaned", false) => (281, 142),
        ("longmemeval_m_cleaned", false) => (43, 21),
        _ => return false,
    };
    let (actual_any, actual_all) = if selected_stage {
        (
            stage.projection_selected_any_evidence_hit,
            stage.projection_selected_all_evidence_hit,
        )
    } else {
        (
            stage.rendered_any_evidence_hit,
            stage.rendered_all_evidence_hit,
        )
    };
    actual_any >= minimum_any && actual_all >= minimum_all
}

fn p7_ablation_proves_suite_effect(
    summaries: &[W4ExternalNoisyBenchmarkSummary],
    suite: &str,
) -> bool {
    let Some(summary) = summaries.iter().find(|summary| summary.suite == suite) else {
        return false;
    };
    let Some(diagnostics) = summary.facet_ablation.as_ref() else {
        return false;
    };
    if !w4_external_facet_ablation_covers_summary(summary)
        || !diagnostics.blocked_reason_counts.is_empty()
        || diagnostics.render_growth != 0
        || diagnostics
            .rendered_evidence_hit_delta
            .get("render_capsule_off")
            .copied()
            .unwrap_or(0)
            <= 0
    {
        return false;
    }
    if matches!(suite, "locomo" | "longmemeval_m_cleaned") {
        diagnostics
            .selected_evidence_hit_delta
            .get("delivery_relevance_fusion_off")
            .copied()
            .unwrap_or(0)
            > 0
    } else {
        true
    }
}

fn p7_production_delivery_covers_summary(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    summary
        .p7_production_delivery
        .as_ref()
        .is_some_and(|diagnostics| {
            diagnostics.questions_with_delivery_report == summary.questions
                && diagnostics.eval_selected_matches_delivery_questions == summary.questions
                && diagnostics.eval_rendered_matches_delivery_questions == summary.questions
                && diagnostics.projection_selected_sources_proven_questions == summary.questions
                && diagnostics.projection_delivery_proof_questions == summary.questions
                && diagnostics.final_projection_integrity_questions == summary.questions
                && diagnostics.final_projection_integrity_passed_questions == summary.questions
                && diagnostics
                    .schema_version_counts
                    .get(&MEMORY_RECALL_DELIVERY_SCHEMA_VERSION.to_string())
                    .copied()
                    .unwrap_or(0)
                    == summary.questions
                && diagnostics.blocked_reason_counts.is_empty()
        })
}

fn p7_production_delivery_has_no_privacy_or_soul_regression(
    summary: &W4ExternalNoisyBenchmarkSummary,
) -> bool {
    summary
        .p7_production_delivery
        .as_ref()
        .is_some_and(|diagnostics| {
            diagnostics.privacy_leak_count == 0
                && diagnostics.cross_subject_leak_count == 0
                && diagnostics.raw_soul_private_material_count == 0
                && diagnostics.final_projection_raw_private_violation_count == 0
        })
}

fn p7_provenance_valid_for_summary(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(provenance) = summary.p7_provenance.as_ref() else {
        return false;
    };
    let Some(dataset) = p7_trusted_dataset(&summary.suite) else {
        return false;
    };
    let Some(runner_release) = P7_TRUSTED_RUNNER_RELEASE else {
        return false;
    };
    if provenance.contract_version != P7_CONTRACT_VERSION
        || !p7_valid_run_id(&summary.run_id)
        || provenance.run_id != summary.run_id
        || provenance.sdk_report_schema_version != MEMORY_RECALL_DELIVERY_SCHEMA_VERSION
        || provenance.sdk_build_fingerprint != P7_TRUSTED_SDK_BUILD_FINGERPRINT
        || provenance.runner_build_fingerprint != runner_release.runner_build_fingerprint
        || provenance.runner_lock_fingerprint != runner_release.runner_lock_fingerprint
        || provenance.executable_sha256 != runner_release.executable_sha256
        || provenance.build_profile != "release"
        || provenance.input_sha256 != dataset.input_sha256
        || !is_sha256(&provenance.merged_detail_sha256)
        || !summary.summary_sha256.as_deref().is_some_and(is_sha256)
        || summary.runner_source_sha256.as_deref()
            != Some(provenance.runner_build_fingerprint.as_str())
        || !summary.operator_content_hash_verified
        || provenance.ordered_shard_digest_manifest.len() != summary.shards.len()
    {
        return false;
    }
    provenance
        .ordered_shard_digest_manifest
        .iter()
        .zip(summary.shards.iter())
        .all(|(digest, shard)| {
            digest.run_id == summary.run_id
                && digest.shard == *shard
                && is_sha256(&digest.summary_sha256)
                && is_sha256(&digest.detail_sha256)
        })
}

fn p7_trusted_dataset(suite: &str) -> Option<P7TrustedDataset> {
    P7_TRUSTED_DATASETS
        .iter()
        .copied()
        .find(|dataset| dataset.suite == suite)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn w4_external_facet_ablation_proves_effect(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(diagnostics) = summary.facet_ablation.as_ref() else {
        return false;
    };
    w4_external_facet_ablation_covers_summary(summary)
        && diagnostics.delivery_contribution_proven_questions > 0
        && diagnostics.blocked_reason_counts.is_empty()
        && diagnostics
            .delivery_contribution_proven_slice_counts
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
        && graph_v2_index_release_conditions_hold(diagnostics)
        && diagnostics.facet_questions_with_index_report > 0
        && diagnostics.facet_index_used_questions == diagnostics.facet_questions_with_index_report
        && diagnostics.facet_report_only_questions == 0
        && diagnostics.facet_fallback_full_scan_questions == 0
        && diagnostics.facet_posting_key_lookup_count >= diagnostics.facet_index_used_questions
        && diagnostics.facet_manifest_matched_posting_count
            == diagnostics.facet_posting_doc_read_count
        && diagnostics.facet_owner_key_lookup_count == diagnostics.facet_owner_doc_read_count
        && diagnostics.facet_zero_posting_key_lookup_questions == 0
        && diagnostics.facet_clean_zero_hit_questions <= diagnostics.facet_index_used_questions
        && diagnostics.facet_manifest_integrity_verified_questions
            == diagnostics.facet_questions_with_index_report
        && diagnostics.facet_manifest_integrity_failure_count == 0
        && diagnostics.facet_failure_count == 0
}

fn w4_external_index_diagnostics_no_full_scan(summary: &W4ExternalNoisyBenchmarkSummary) -> bool {
    let Some(diagnostics) = summary.index_diagnostics.as_ref() else {
        return true;
    };
    let facet_used_requirement_holds = if summary.suite == "longmemeval_oracle" {
        diagnostics.facet_index_used_questions > 0
            && diagnostics.facet_index_used_questions <= summary.questions
    } else {
        diagnostics.facet_index_used_questions == summary.questions
    };
    diagnostics.questions_with_index_report == summary.questions
        && diagnostics.index_used_questions > 0
        && diagnostics.fallback_full_scan_questions == 0
        && diagnostics.failure_count == 0
        && diagnostics.matched_source_anchor_count > 0
        && diagnostics.indexed_neighbor_count > 0
        && graph_v2_index_release_conditions_hold(diagnostics)
        && diagnostics.facet_questions_with_index_report == summary.questions
        && facet_used_requirement_holds
        && diagnostics.facet_report_only_questions == 0
        && diagnostics.facet_fallback_full_scan_questions == 0
        && diagnostics.facet_posting_key_lookup_count >= diagnostics.facet_index_used_questions
        && diagnostics.facet_manifest_matched_posting_count
            == diagnostics.facet_posting_doc_read_count
        && diagnostics.facet_owner_key_lookup_count == diagnostics.facet_owner_doc_read_count
        && diagnostics.facet_zero_posting_key_lookup_questions == 0
        && diagnostics.facet_clean_zero_hit_questions <= diagnostics.facet_index_used_questions
        && diagnostics.facet_manifest_integrity_verified_questions
            == diagnostics.facet_index_used_questions
        && diagnostics.facet_manifest_integrity_failure_count == 0
        && diagnostics.facet_failure_count == 0
}

fn graph_v2_index_release_conditions_hold(diagnostics: &W4ExternalNoisyIndexDiagnostics) -> bool {
    diagnostics.graph_manifest_contract_verified_questions == diagnostics.index_used_questions
        && diagnostics.graph_selected_dependency_chain_verified_questions
            == diagnostics.index_used_questions
        && diagnostics.graph_manifest_generation_present_questions
            == diagnostics.index_used_questions
        && diagnostics.graph_revision_present_questions == diagnostics.index_used_questions
        && diagnostics.graph_scope_digest_present_questions == diagnostics.index_used_questions
        && diagnostics.graph_maintenance_required_questions == 0
        && diagnostics.graph_incident_questions == 0
        && diagnostics.graph_read_path_mutation_delta == 0
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
