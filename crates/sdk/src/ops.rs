use crate::store_internal::{
    StoreMutationBudgetReport, StoreSnapshotExportReport, StoreSnapshotImportReport,
};
use bm_core::memory::IngressKind;
use bm_core::memory::{
    CanonicalTurnDelta, ConversationKey, DeferredGovernanceQueueReport, DerivedMemoryRef,
    HostOpaqueRef, LongTermMemoryQuery, MemoryGovernancePolicyMutation,
    MemoryGovernancePolicyMutationReport as CoreMemoryGovernancePolicyMutationReport,
    MemoryHygieneInspection, MemoryHygieneOutcome, MemoryLongTermAffectedFacetDoc,
    MemoryLongTermAffectedRecord, MemoryLongTermControlDecision, MemoryLongTermControlView,
    MemoryLongTermDetailReport as CoreMemoryLongTermDetailReport,
    MemoryLongTermListReport as CoreMemoryLongTermListReport, MemoryLongTermMutation,
    MemoryLongTermMutationReport as CoreMemoryLongTermMutationReport, MemoryLongTermTarget,
    MemoryLongTermTargetResolutionReport, MemoryLongTermTombstoneRef, MemoryProjectionImpactReport,
    PostTurnPrivateGardenReport, PostTurnSemanticGovernanceReport, PrivateMaterialRedactionReport,
    ProceduralMemoryPromotionInput, ProceduralMemoryPromotionReport, ProjectionFaithfulnessCheck,
    QueryFacetInput, RedactedTranscriptSlice, SessionTurnCommitReport, SkillEvolutionReport,
    SubjectProjectionReport, SubjectScopedRuntime, TranscriptAttrEnvelope,
    TranscriptAttrWriteRejection, TranscriptCommitReport, TranscriptEvidenceRef,
    TranscriptLifecycleReport, TranscriptLifecycleTransition, TranscriptRedactionReportItem,
    TranscriptRepairReport, TranscriptReplayView, VaultManifest, VaultMigrationPreflight,
};
use bm_core::memory::{
    CompactMemoryGraph, EvidenceBacklink, FacetCoverageSelectionReport, FacetRankFusionReport,
    GraphRecallCandidateScore, GraphRecallExpansionBudgetReport, GraphRecallRerankReport,
    MemoryGraphEdge, MemoryGraphNode, RecallDeliverySelectionDropReason,
    TemporalMemoryGraphGateReport,
};
use bm_core::skills::{
    AgentSkillDirectoryReport, AgentSkillProjectionAudit, AgentSkillRecallHit,
    AgentToolExperienceGovernanceReport, AgentToolExperienceStatusReport, AgentToolHint,
    AgentToolProjectionAudit, AgentToolRegistryRef, AgentToolRegistryReport,
    AgentToolUsageFeedback, ProjectedAgentSkillHint,
};
use bm_core::{
    budget::{RuntimeBudgetReport, RuntimeRetentionQuotaReport},
    feature_gate::ProfileId,
};

use crate::{
    ContinuitySnapshot, ContinuitySnapshotImportMode, ContinuitySnapshotImportOutcome,
    IntelligenceReplayInspection, MemoryCapabilityCatalog, ParsedLongTermMemoryExtraction,
    PostReplyMemoryMaintenanceOutcome, PromptMemoryContext, RuntimeSkillHit,
    RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteOutcome, RuntimeSkillWriteSource,
    StoreSnapshot, WorkingRecallInspection,
};
use crate::{
    RuntimeLifecycleDiagnosisReport, RuntimeLifecycleModeInput, RuntimeLifecycleReport,
    RuntimeLifecycleTrigger,
};
use bm_core::memory::PromptProjectionSource;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillListRequest {
    pub query: Option<String>,
    pub include_disabled: bool,
    pub include_retired: bool,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillSummary {
    pub name: String,
    pub title: String,
    pub topic: String,
    pub status: String,
    pub enabled: bool,
    pub quality_score: Option<u8>,
    pub use_count: u32,
    pub validated_success_count: u32,
    pub mismatch_count: u32,
    pub revision_pending: bool,
    pub updated_at: u64,
    pub last_used_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillListReport {
    pub total: usize,
    pub active: usize,
    pub disabled: usize,
    pub runtime_skills: usize,
    pub skills: Vec<RuntimeSkillSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillDetailRequest {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillDetailReport {
    pub summary: RuntimeSkillSummary,
    pub summary_text: String,
    pub procedure_text: String,
    pub raw_content: String,
    pub citations: Vec<String>,
    pub source_chat_id: Option<String>,
    pub lineage: Vec<String>,
    pub strategy_diffs: Vec<String>,
    pub last_outcome_note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillEditRequest {
    pub name: String,
    pub title: String,
    pub topic: String,
    pub summary: String,
    pub procedure: String,
    pub citations: Vec<String>,
    pub source_chat_id: Option<String>,
    pub edit_reason: String,
    pub observed_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillMutationReport {
    pub accepted: bool,
    pub changed: bool,
    pub name: String,
    pub operation: &'static str,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillSetEnabledRequest {
    pub name: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillDeleteRequest {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermListRequest {
    pub query: LongTermMemoryQuery,
    pub cursor: Option<String>,
    pub limit: usize,
    pub view: MemoryLongTermControlView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermDetailRequest {
    pub target: MemoryLongTermTarget,
    pub view: MemoryLongTermControlView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermMutationRequest {
    pub operation: MemoryLongTermMutation,
    pub reason: String,
    pub dry_run: bool,
    pub mode_input: RuntimeLifecycleModeInput,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLongTermPolicyRequest {
    pub operation: MemoryGovernancePolicyMutation,
    pub reason: String,
    pub dry_run: bool,
    pub mode_input: RuntimeLifecycleModeInput,
}

pub type MemoryLongTermListReport = CoreMemoryLongTermListReport;
pub type MemoryLongTermDetailReport = CoreMemoryLongTermDetailReport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLongTermMutationReport {
    pub accepted: bool,
    pub dry_run: bool,
    pub operation: &'static str,
    pub target_report: MemoryLongTermTargetResolutionReport,
    pub affected_records: Vec<MemoryLongTermAffectedRecord>,
    pub affected_facet_docs: Vec<MemoryLongTermAffectedFacetDoc>,
    pub tombstones: Vec<MemoryLongTermTombstoneRef>,
    pub evidence_refs: Vec<DerivedMemoryRef>,
    pub transcript_refs: Vec<TranscriptEvidenceRef>,
    pub policy_decision: MemoryLongTermControlDecision,
    pub projection_impact: MemoryProjectionImpactReport,
    pub deferred_governance_impact: bm_core::memory::MemoryDeferredGovernanceImpactReport,
    pub lifecycle_report: RuntimeLifecycleReport,
    pub audit_event_id: Option<String>,
    pub reason: String,
    pub core_report: CoreMemoryLongTermMutationReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernancePolicyMutationReport {
    pub accepted: bool,
    pub dry_run: bool,
    pub operation: &'static str,
    pub policy_id: Option<String>,
    pub affected_future_writes: String,
    pub policy_decision: MemoryLongTermControlDecision,
    pub lifecycle_report: RuntimeLifecycleReport,
    pub audit_event_id: Option<String>,
    pub reason: String,
    pub core_report: CoreMemoryGovernancePolicyMutationReport,
}

#[derive(Clone, Debug)]
pub enum MemoryWriteRequest {
    Procedural {
        writes: Vec<RuntimeSkillWrite>,
        source: RuntimeSkillWriteSource,
    },
    ProceduralPromotions {
        promotions: Vec<ProceduralMemoryPromotionInput>,
        source: RuntimeSkillWriteSource,
    },
    LongTermExtraction {
        extraction: ParsedLongTermMemoryExtraction,
    },
    Candidates {
        candidates: Vec<bm_core::memory::MemoryWriteCandidate>,
    },
    AgentToolUsageFeedback {
        feedback: AgentToolUsageFeedback,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryWriteReport {
    pub accepted: bool,
    pub changed: usize,
    pub operation: &'static str,
    pub reason: String,
    pub lifecycle_report: RuntimeLifecycleReport,
    pub transaction: Option<MemoryWriteTransactionReport>,
    pub semantic_governance: Option<PostTurnSemanticGovernanceReport>,
    pub shared_fact_governance: Option<bm_core::memory::SharedMemoryWriteOutcome>,
    pub procedural_evolution: Option<SkillEvolutionReport>,
    pub procedural_promotions: Vec<ProceduralMemoryPromotionReport>,
    pub agent_tool_experience: Option<AgentToolExperienceGovernanceReport>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryWriteTransactionReport {
    pub transaction_id: String,
    pub operation: String,
    pub planned_mutations: usize,
    pub committed_mutations: usize,
    pub event_ids: Vec<String>,
    pub budget_report: StoreMutationBudgetReport,
    pub changed_count: usize,
    pub partial_write: bool,
}

#[derive(Clone, Debug)]
pub struct MemoryRecallRequest {
    pub query: String,
    pub limit: usize,
    pub structured_query_facets: Vec<QueryFacetInput>,
    pub tool_registry_refs: Vec<AgentToolRegistryRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallBenchmarkContext {
    pub suite: String,
    pub question_id: String,
    pub question_type: String,
    pub expected_evidence_refs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct MemoryEvalRecallRequest {
    pub query: String,
    pub k: usize,
    pub include_expanded_candidates: bool,
    pub include_graph_neighbors: bool,
    pub include_score_breakdown: bool,
    pub include_missing_evidence: bool,
    pub benchmark_context: Option<MemoryEvalRecallBenchmarkContext>,
    pub structured_query_facets: Vec<QueryFacetInput>,
    pub tool_registry_refs: Vec<AgentToolRegistryRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallCandidate {
    pub candidate_id: String,
    pub source: String,
    pub evidence_refs: Vec<String>,
    pub graph_neighbor_ids: Vec<String>,
    pub score_breakdown: GraphRecallCandidateScore,
}

pub const MEMORY_RECALL_DELIVERY_SCHEMA_VERSION: u32 = 2;
pub const MEMORY_PROJECTION_DELIVERY_DIGEST_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryEvidenceRefVisibility {
    PublicCitation,
    GovernedOpaque,
    Redacted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryEvidenceRefView {
    pub visibility: MemoryEvidenceRefVisibility,
    pub reference: Option<String>,
    pub reason: Option<String>,
}

pub type MemoryRecallSelectionDropReason = RecallDeliverySelectionDropReason;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryRecallRenderDropReason {
    CapsuleBudgetExhausted,
    CitationMissing,
    DuplicateCapsuleGroup,
    OwnerRecordUnavailable,
    PrivateRawRedacted,
    RenderBudgetExhausted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRecallSelectionDecision {
    pub candidate_id: String,
    pub canonical_evidence_groups: Vec<String>,
    pub evidence_family_groups: Vec<String>,
    pub selected: bool,
    pub drop_reason: Option<MemoryRecallSelectionDropReason>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRenderedEvidenceCapsule {
    pub candidate_id: String,
    pub content: String,
    pub evidence_ref_views: Vec<MemoryEvidenceRefView>,
    pub visible_evidence_refs: Vec<String>,
    pub canonical_evidence_groups: Vec<String>,
    pub source_locator_view: MemoryEvidenceRefView,
    pub observed_at: u64,
    pub valid_from: Option<u64>,
    pub valid_until: Option<u64>,
    pub facet_summary: String,
    pub redaction_state: String,
    pub shared_fact_surface_allowed: bool,
    pub rendered_chars: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRecallRenderDecision {
    pub candidate_id: String,
    pub rendered: bool,
    pub drop_reason: Option<MemoryRecallRenderDropReason>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryRecallDeliveryReport {
    pub schema_version: u32,
    pub owner: String,
    pub selection_strategy: String,
    pub render_strategy: String,
    pub requested_limit: usize,
    pub profile_selected_ceiling: usize,
    pub loss_ledger_entry_ceiling: usize,
    pub governed_candidate_count: usize,
    pub redacted_candidate_count: usize,
    pub selected_candidate_ids: Vec<String>,
    pub selection_decisions: Vec<MemoryRecallSelectionDecision>,
    pub covered_evidence_family_groups: Vec<String>,
    pub rendered_capsules: Vec<MemoryRenderedEvidenceCapsule>,
    pub render_decisions: Vec<MemoryRecallRenderDecision>,
    pub render_budget_chars: usize,
    pub rendered_chars: usize,
    pub render_growth: usize,
    pub integrity_failures: Vec<String>,
    pub delivery_drop_reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryEvalRecallLossEntry {
    pub evidence_ref: MemoryEvidenceRefView,
    pub canonical_evidence_group: String,
    pub expanded_rank: Option<usize>,
    pub reranked_rank: Option<usize>,
    pub selected_rank: Option<usize>,
    pub rendered_rank: Option<usize>,
    pub selection_drop_reason: Option<MemoryRecallSelectionDropReason>,
    pub render_drop_reason: Option<MemoryRecallRenderDropReason>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallEvidenceGroupCoverage {
    pub selected_groups: Vec<String>,
    pub rendered_groups: Vec<String>,
    pub missing_selected_groups: Vec<String>,
    pub missing_rendered_groups: Vec<String>,
    pub duplicate_selected_group_count: usize,
    pub duplicate_rendered_group_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallLossLedger {
    pub expanded_hit_selected_miss: Vec<MemoryEvalRecallLossEntry>,
    pub selected_hit_rendered_miss: Vec<MemoryEvalRecallLossEntry>,
    pub canonical_evidence_group_coverage: MemoryEvalRecallEvidenceGroupCoverage,
    pub render_budget_chars: usize,
    pub rendered_chars: usize,
    pub truncated_count: usize,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallEvidenceRefIndexEntry {
    pub candidate_id: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallStageEvidenceRefs {
    pub stage: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallGoldRank {
    pub stage: String,
    pub evidence_ref: String,
    pub rank: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallGraphDistanceToGold {
    pub candidate_id: String,
    pub evidence_ref: String,
    pub distance: Option<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryFacetRecallIndexReport {
    pub owner: String,
    pub used: bool,
    pub report_only: bool,
    pub fallback_full_scan: bool,
    pub source_candidate_count: usize,
    pub matched_source_candidate_count: usize,
    pub posting_key_lookup_count: usize,
    pub manifest_matched_posting_count: usize,
    pub posting_doc_read_count: usize,
    pub owner_key_lookup_count: usize,
    pub owner_doc_read_count: usize,
    pub exact_facet_match_count: usize,
    pub expanded_facet_match_count: usize,
    pub exact_facet_candidate_ids: Vec<String>,
    pub expanded_facet_candidate_ids: Vec<String>,
    pub manifest_owner_doc_count: usize,
    pub manifest_posting_doc_count: usize,
    pub manifest_integrity_verified: bool,
    pub index_revision: Option<String>,
    pub render_growth: usize,
    pub failures: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallFacetStageDiagnostics {
    pub used: bool,
    pub report_only: bool,
    pub miss_after_expanded: bool,
    pub expanded_missing_evidence_refs: Vec<String>,
    pub exact_facet_candidate_ids: Vec<String>,
    pub expanded_facet_candidate_ids: Vec<String>,
    pub source_candidate_count: usize,
    pub matched_source_candidate_count: usize,
    pub rendered_candidate_count: usize,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallCandidateEvidenceBinding {
    pub candidate_id: String,
    pub canonical_evidence_groups: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallAblationSlice {
    pub name: String,
    pub feature_enabled: bool,
    pub report_available: bool,
    pub delivery_contribution_proven: bool,
    pub delivery_affected_candidate_count: usize,
    pub selected_evidence_hit_delta: i64,
    pub rendered_evidence_hit_delta: i64,
    pub selected_all_hit_lost: bool,
    pub rendered_all_hit_lost: bool,
    pub baseline_selected_evidence_refs: Vec<String>,
    pub off_run_selected_evidence_refs: Vec<String>,
    pub baseline_rendered_evidence_refs: Vec<String>,
    pub off_run_rendered_evidence_refs: Vec<String>,
    pub baseline_selected_candidate_ids: Vec<String>,
    pub off_run_selected_candidate_ids: Vec<String>,
    pub baseline_rendered_candidate_ids: Vec<String>,
    pub off_run_rendered_candidate_ids: Vec<String>,
    pub baseline_selected_candidate_bindings: Vec<MemoryEvalRecallCandidateEvidenceBinding>,
    pub off_run_selected_candidate_bindings: Vec<MemoryEvalRecallCandidateEvidenceBinding>,
    pub baseline_rendered_candidate_bindings: Vec<MemoryEvalRecallCandidateEvidenceBinding>,
    pub off_run_rendered_candidate_bindings: Vec<MemoryEvalRecallCandidateEvidenceBinding>,
    pub baseline_expanded_candidate_count: usize,
    pub off_run_expanded_candidate_count: usize,
    pub baseline_selected_candidate_count: usize,
    pub off_run_selected_candidate_count: usize,
    pub baseline_rendered_candidate_count: usize,
    pub off_run_rendered_candidate_count: usize,
    pub baseline_rendered_chars: usize,
    pub off_run_rendered_chars: usize,
    pub baseline_render_growth: usize,
    pub off_run_render_growth: usize,
    pub expanded_candidate_delta: i64,
    pub selected_candidate_delta: i64,
    pub rendered_candidate_delta: i64,
    pub rendered_char_delta: i64,
    pub render_growth: usize,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallAblationReport {
    pub method: String,
    pub required_slices: Vec<String>,
    pub slices: Vec<MemoryEvalRecallAblationSlice>,
    pub delivery_contribution_proven: bool,
    pub render_growth: usize,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallStageDiagnostics {
    pub suite: String,
    pub question_id: String,
    pub question_type: String,
    pub evidence_count: usize,
    pub gold_evidence_refs: Vec<String>,
    pub first_any_hit_stage: Option<String>,
    pub first_all_hit_stage: Option<String>,
    pub matched_gold_by_stage: Vec<MemoryEvalRecallStageEvidenceRefs>,
    pub missing_gold_by_stage: Vec<MemoryEvalRecallStageEvidenceRefs>,
    pub gold_rank_by_stage: Vec<MemoryEvalRecallGoldRank>,
    pub miss_after_expanded: bool,
    pub source_anchor_ids: Vec<String>,
    pub graph_anchor_candidate_ids: Vec<String>,
    pub expanded_node_ids: Vec<String>,
    pub graph_neighbor_ids: Vec<String>,
    pub graph_distance_to_gold: Vec<MemoryEvalRecallGraphDistanceToGold>,
    pub facet_stage: MemoryEvalRecallFacetStageDiagnostics,
    pub ablation_report: MemoryEvalRecallAblationReport,
    pub expansion_budget: GraphRecallExpansionBudgetReport,
    pub truncated_count: usize,
    pub blocked_reasons: Vec<String>,
    pub selected_candidate_ids: Vec<String>,
    pub rendered_candidate_ids: Vec<String>,
    pub rendered_evidence_refs: Vec<String>,
    pub loss_ledger: MemoryEvalRecallLossLedger,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallAtK {
    pub k: usize,
    pub matched_evidence_refs: Vec<String>,
    pub missing_evidence_refs: Vec<String>,
    pub any_evidence_hit: bool,
    pub all_evidence_hit: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallMetrics {
    pub requested_k: usize,
    pub source_candidate_count: usize,
    pub expanded_candidate_count: usize,
    pub selected_candidate_count: usize,
    pub rendered_candidate_count: usize,
    pub expected_evidence_count: usize,
    pub recall_at_k: Vec<MemoryEvalRecallAtK>,
    pub any_evidence_hit: bool,
    pub all_evidence_hit: bool,
    pub mrr_bps: u32,
    pub question_type: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalRecallPrivacyReport {
    pub passed: bool,
    pub private_raw_candidate_count: u32,
    pub redacted_candidate_ids: Vec<String>,
    pub failures: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryGraphRecallIndexReport {
    pub owner: String,
    pub used: bool,
    pub fallback_full_scan: bool,
    pub manifest_contract_verified: bool,
    pub selected_dependency_chain_verified: bool,
    pub full_scope_closure_verified: bool,
    pub manifest_generation: Option<u64>,
    pub graph_revision: Option<String>,
    pub scope_digest: String,
    pub maintenance_required: bool,
    pub incident_token: Option<String>,
    pub read_path_mutation_delta: usize,
    pub source_candidate_count: usize,
    pub matched_source_anchor_count: usize,
    pub source_anchor_ids: Vec<String>,
    pub unmatched_source_anchor_ids: Vec<String>,
    pub expanded_node_ids: Vec<String>,
    pub indexed_neighbor_count: usize,
    pub index_doc_count: usize,
    pub filtered_node_count: usize,
    pub filtered_edge_count: usize,
    pub filtered_backlink_count: usize,
    pub failures: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct MemoryEvalRecallReport {
    pub query: String,
    pub benchmark_context: Option<MemoryEvalRecallBenchmarkContext>,
    pub source_candidates: Vec<MemoryEvalRecallCandidate>,
    pub graph_anchor_candidates: Vec<MemoryEvalRecallCandidate>,
    pub expanded_candidates: Vec<MemoryEvalRecallCandidate>,
    pub eval_candidate_pool: Vec<MemoryEvalRecallCandidate>,
    pub graph_neighbors: Vec<String>,
    pub reranked_candidates: Vec<MemoryEvalRecallCandidate>,
    pub selected_candidates: Vec<MemoryEvalRecallCandidate>,
    pub rendered_candidates: Vec<MemoryEvalRecallCandidate>,
    pub rendered_block_preview: String,
    pub delivery_report: MemoryRecallDeliveryReport,
    pub evidence_ref_index: Vec<MemoryEvalRecallEvidenceRefIndexEntry>,
    pub stage_diagnostics: MemoryEvalRecallStageDiagnostics,
    pub metrics: MemoryEvalRecallMetrics,
    pub missing_evidence_refs: Vec<String>,
    pub budget_report: RuntimeBudgetReport,
    pub privacy_report: MemoryEvalRecallPrivacyReport,
    pub facet_index_report: MemoryFacetRecallIndexReport,
    pub rank_fusion_report: FacetRankFusionReport,
    pub coverage_selection_report: FacetCoverageSelectionReport,
    pub ablation_report: MemoryEvalRecallAblationReport,
    pub graph_index_report: MemoryGraphRecallIndexReport,
    pub graph_rerank: GraphRecallRerankReport,
    pub graph_gate: TemporalMemoryGraphGateReport,
    pub compact_graph: CompactMemoryGraph,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct TemporalMemoryGraphWriteRequest {
    pub operation: String,
    pub nodes: Vec<MemoryGraphNode>,
    pub edges: Vec<MemoryGraphEdge>,
    pub backlinks: Vec<EvidenceBacklink>,
}

#[derive(Clone, Debug)]
pub struct TemporalMemoryGraphMutationReport {
    pub accepted: bool,
    pub operation: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub backlink_count: usize,
    pub revision_count: usize,
    pub index_count: usize,
    pub manifest_generation: Option<u64>,
    pub graph_revision: Option<String>,
    pub scope_digest: String,
    pub gate_failures: Vec<String>,
    pub transaction: Option<MemoryWriteTransactionReport>,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryGraphIntegrityMaintenanceRequest {
    pub expected_manifest_generation: u64,
    pub incident_token: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MemoryGraphIntegrityMaintenanceReport {
    pub accepted: bool,
    pub committed: bool,
    pub manifest_integrity_verified: bool,
    pub expected_manifest_generation: u64,
    pub observed_manifest_generation: Option<u64>,
    pub manifest_generation: Option<u64>,
    pub graph_revision: Option<String>,
    pub scope_digest: String,
    pub maintenance_required: bool,
    pub incident_token: Option<String>,
    pub removed_node_count: usize,
    pub removed_edge_count: usize,
    pub removed_backlink_count: usize,
    pub retained_shared_backlink_count: usize,
    pub failures: Vec<String>,
    pub transaction: Option<MemoryWriteTransactionReport>,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct MemoryRecallReport {
    pub query: String,
    pub procedural_hits: Vec<RuntimeSkillHit>,
    pub agent_skill_hits: Vec<AgentSkillRecallHit>,
    pub agent_tool_hints: Vec<AgentToolHint>,
    pub tool_experience_status: AgentToolExperienceStatusReport,
    pub working: WorkingRecallInspection,
    pub source_candidate_ids: Vec<String>,
    pub graph_anchor_candidate_ids: Vec<String>,
    pub graph_index_report: MemoryGraphRecallIndexReport,
    pub facet_index_report: MemoryFacetRecallIndexReport,
    pub rank_fusion_report: FacetRankFusionReport,
    pub coverage_selection_report: FacetCoverageSelectionReport,
    pub graph_rerank: GraphRecallRerankReport,
    pub graph_gate: TemporalMemoryGraphGateReport,
    pub graph_candidate_evidence_ref_index: Vec<MemoryEvalRecallEvidenceRefIndexEntry>,
    pub compact_graph: CompactMemoryGraph,
    pub delivery_report: MemoryRecallDeliveryReport,
    pub store_snapshot_consistent: bool,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct MemoryProjectionRequest {
    pub user_query: String,
    pub system_max_len: usize,
    pub recent_messages_limit: usize,
    pub pressure: crate::PressureLevel,
    pub mode_input: RuntimeLifecycleModeInput,
    pub structured_query_facets: Vec<QueryFacetInput>,
    pub tool_registry_refs: Vec<AgentToolRegistryRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LLMRuntimeProjectionEnvelope {
    pub projection_id: String,
    pub runtime_awareness: String,
    pub subject_mount: SoulLifeProjectionReport,
    pub boundary_protocol: RuntimeDisclosureProtocolReport,
    pub protected_private_runtime_context: Vec<RuntimeProjectionSourceBlock>,
    pub governed_memory_evidence: Vec<RuntimeProjectionSourceBlock>,
    pub procedural_evidence: Vec<RuntimeProjectionSourceBlock>,
    pub agent_skill_hints: Vec<ProjectedAgentSkillHint>,
    pub agent_tool_hints: Vec<AgentToolHint>,
    pub runtime_constraints: Vec<String>,
    pub work_integrity: WorkIntegrityReport,
    pub operator_audit_excluded_source_ids: Vec<String>,
    pub section_names: Vec<String>,
    pub rendered_block: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryProjectionSurfaceSet {
    pub prompt: String,
    pub ui_api: String,
    pub operator_raw: String,
    pub gateway_raw_audit: String,
    pub shared_fact_surface: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrivateDisclosureSurfaceReport {
    pub surface: String,
    pub protected_exact_echo_count: u32,
    pub forbidden_marker_count: u32,
    pub violation_count: u32,
    pub passed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoulLifeProjectionReport {
    pub identity_mount: String,
    pub relationship_position: String,
    pub situated_now: String,
    pub current_reasoning_basis: String,
    pub reply_stance: String,
    pub initiative_posture: String,
    pub boundary_mode: String,
    pub degraded_reason: Option<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeDisclosureProtocolReport {
    pub runtime_private_context_allowed: bool,
    pub foreground_disclosure_allowed: bool,
    pub protected_sources: Vec<String>,
    pub disclosure_rule: String,
    pub final_llm_privacy_judge_allowed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeProjectionSourceBlock {
    pub source_id: String,
    pub role: String,
    pub content: String,
    pub evidence_refs: Vec<String>,
    pub protected: bool,
    pub shared_fact_surface_allowed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryProjectionDeliveryDigestContentEntry {
    pub candidate_id: String,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryProjectionDeliveryDigestEntry {
    pub candidate_id: String,
    pub source_block_sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryProjectionDeliveryDigestManifest {
    pub schema_version: u32,
    pub system_memory_block_sha256: String,
    pub capsule_entries: Vec<MemoryProjectionDeliveryDigestContentEntry>,
    pub governed_block_entries: Vec<MemoryProjectionDeliveryDigestContentEntry>,
    pub prompt_visible_entries: Vec<MemoryProjectionDeliveryDigestContentEntry>,
    pub deterministic_envelope_sha256: String,
    pub exact_render_match: bool,
    pub candidate_receipts: Vec<MemoryProjectionDeliveryDigestEntry>,
    pub integrity_failures: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkIntegrityReport {
    pub task_goal: String,
    pub evidence_ceiling: String,
    pub tool_permission_boundary: String,
    pub uncertainty_rule: String,
    pub no_obstruction_rule: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrivateDisclosureIntegrityReport {
    pub checked_surfaces: Vec<String>,
    pub surface_reports: Vec<PrivateDisclosureSurfaceReport>,
    pub blocked_source_ids: Vec<String>,
    pub redacted_source_ids: Vec<String>,
    pub raw_private_violation_count: u32,
    pub passed: bool,
}

pub struct MemoryProjectionReport {
    pub system_memory_block: String,
    pub context: PromptMemoryContext,
    pub audit: MemoryProjectionAuditReport,
    pub runtime_projection: LLMRuntimeProjectionEnvelope,
    pub projection_surfaces: MemoryProjectionSurfaceSet,
    pub life_projection: SoulLifeProjectionReport,
    pub work_integrity: WorkIntegrityReport,
    pub subject_projection: SubjectProjectionReport,
    pub projection_faithfulness: ProjectionFaithfulnessCheck,
    pub delivery_digest_manifest: MemoryProjectionDeliveryDigestManifest,
    pub private_disclosure_integrity: PrivateDisclosureIntegrityReport,
    pub recall_delivery_report: MemoryRecallDeliveryReport,
    pub graph_index_report: MemoryGraphRecallIndexReport,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProjectionSourceAudit {
    pub plane: String,
    pub backend: String,
    pub candidate_count: usize,
    pub selected_count: usize,
    pub selected_ids: Vec<String>,
    pub miss_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProjectionSectionAudit {
    pub name: String,
    pub chars: usize,
    pub included: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProjectionPrivateGateAudit {
    pub privacy_policy_allowed: bool,
    pub lifecycle_private_depth_allowed: bool,
    pub runtime_private_context_allowed: bool,
    pub foreground_disclosure_allowed: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProjectionAuditReport {
    pub projection_id: String,
    pub operation: String,
    pub profile: ProfileId,
    pub identity: crate::MemoryIdentity,
    pub scope: crate::MemoryScope,
    pub memory_space_id: String,
    pub subject_id: String,
    pub scoped_runtime: SubjectScopedRuntime,
    pub conversation_id: Option<String>,
    pub source_budget_chars: usize,
    pub render_budget_chars: usize,
    pub system_memory_chars: usize,
    pub injected: bool,
    pub truncated: bool,
    pub private_gate: MemoryProjectionPrivateGateAudit,
    pub source_authority: Vec<PromptProjectionSource>,
    pub agent_skills: AgentSkillProjectionAudit,
    pub agent_tools: AgentToolProjectionAudit,
    pub sources: Vec<MemoryProjectionSourceAudit>,
    pub sections: Vec<MemoryProjectionSectionAudit>,
}

#[derive(Clone, Debug)]
pub struct MemoryMaintenanceRequest {
    pub ingress: IngressKind,
    pub user_content: String,
    pub reply_content: String,
    pub tool_calls: u32,
    pub external_content_used: bool,
    pub runtime_skill_selected_ids: Vec<String>,
    pub task_learning_selected_ids: Vec<String>,
    pub reuse_outcome: RuntimeSkillReuseOutcome,
    pub reuse_outcome_note: String,
    pub pressure: crate::PressureLevel,
    pub mode_input: RuntimeLifecycleModeInput,
}

pub struct MemoryMaintenanceReport {
    pub report: Option<PostReplyMemoryMaintenanceOutcome>,
    pub long_term_refresh_enqueued: bool,
    pub transaction: Option<MemoryWriteTransactionReport>,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct MemoryTurnFinalizeRequest {
    pub turn: CanonicalTurnDelta,
    pub tool_calls: u32,
    pub runtime_skill_selected_ids: Vec<String>,
    pub task_learning_selected_ids: Vec<String>,
    pub reuse_outcome_note: String,
    pub tool_usage_feedback: Option<AgentToolUsageFeedback>,
    pub pressure: crate::PressureLevel,
    pub mode_input: RuntimeLifecycleModeInput,
}

pub struct MemoryTurnFinalizeReport {
    pub session_commit: SessionTurnCommitReport,
    pub transcript_commit: Option<TranscriptCommitReport>,
    pub maintenance: Option<MemoryMaintenanceReport>,
    pub private_garden_self_work: PostTurnPrivateGardenReport,
    pub semantic_governance: PostTurnSemanticGovernanceReport,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct MemoryDeferredGovernanceRunRequest {
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryDeferredGovernanceRunReport {
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub remaining_pending: usize,
    pub queue: DeferredGovernanceQueueReport,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct MemoryRetentionCompactionRequest {
    pub pressure: crate::PressureLevel,
    pub mode_input: RuntimeLifecycleModeInput,
}

#[derive(Clone, Debug)]
pub struct MemoryRetentionCompactionReport {
    pub owner: String,
    pub executed: bool,
    pub retention_quota: RuntimeRetentionQuotaReport,
    pub hygiene: MemoryHygieneOutcome,
    pub long_term_records_before: usize,
    pub long_term_records_after: usize,
    pub destructive_deletes_performed: bool,
    pub host_direct_deletion_allowed: Option<bool>,
    pub fail_closed_repair: bool,
    pub transaction: Option<MemoryWriteTransactionReport>,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct MemoryInspectionRequest {
    pub query: String,
    pub system_max_len: usize,
    pub pressure: crate::PressureLevel,
    pub mode_input: RuntimeLifecycleModeInput,
}

pub struct MemoryInspectionReport {
    pub working: WorkingRecallInspection,
    pub hygiene: MemoryHygieneInspection,
    pub deferred_governance: DeferredGovernanceQueueReport,
    pub agent_skill_directory: AgentSkillDirectoryReport,
    pub agent_tool_registry: AgentToolRegistryReport,
    pub capabilities: MemoryCapabilityCatalog,
    pub operator_action_report: RuntimeOperatorActionReport,
    pub lifecycle_report: RuntimeLifecycleReport,
}

pub struct MemoryReplayRequest {
    pub chat_id: String,
    pub limit: usize,
}

#[derive(Clone, Debug)]
pub struct MemoryReplayReport {
    pub chat_id: String,
    pub inspection: IntelligenceReplayInspection,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptCommitRequest {
    pub turn: CanonicalTurnDelta,
    pub host_refs: Vec<HostOpaqueRef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptCommitReport {
    pub key: ConversationKey,
    pub session_commit: SessionTurnCommitReport,
    pub transcript_commit: Option<TranscriptCommitReport>,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptReplayRequest {
    pub memory_space_id: String,
    pub channel_id: String,
    pub conversation_id: String,
    pub limit: usize,
    pub cursor: Option<String>,
    pub view: TranscriptReplayView,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptReplayReport {
    pub slice: RedactedTranscriptSlice,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptAttrWriteRequest {
    pub memory_space_id: String,
    pub channel_id: String,
    pub conversation_id: String,
    pub attrs: Vec<TranscriptAttrEnvelope>,
    pub idempotency_key: Option<String>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptAttrWriteReport {
    pub key: ConversationKey,
    pub accepted_attrs: Vec<TranscriptAttrEnvelope>,
    pub rejected_attrs: Vec<TranscriptAttrWriteRejection>,
    pub redactions_preview: Vec<TranscriptRedactionReportItem>,
    pub profile_budget_applied: bool,
    pub audit_event_id: Option<String>,
    pub dry_run: bool,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptLifecycleRequest {
    pub memory_space_id: String,
    pub channel_id: String,
    pub conversation_id: String,
    pub turn_id: Option<String>,
    pub transition: TranscriptLifecycleTransition,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptLifecycleReport {
    pub transcript: TranscriptLifecycleReport,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptRepairRequest {
    pub memory_space_id: String,
    pub channel_id: String,
    pub conversation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptRepairReport {
    pub transcript: TranscriptRepairReport,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptExportRequest {
    pub memory_space_id: String,
    pub channel_id: String,
    pub conversation_id: String,
    pub limit: usize,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryTranscriptExportReport {
    pub slice: RedactedTranscriptSlice,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct MemoryExportRequest {
    pub chat_id: String,
}

#[derive(Clone, Debug)]
pub struct MemoryExportReport {
    pub snapshot: ContinuitySnapshot,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct MemoryImportRequest {
    pub snapshot: ContinuitySnapshot,
    pub target_chat_id: String,
    pub mode: ContinuitySnapshotImportMode,
}

#[derive(Clone, Debug)]
pub struct MemoryImportReport {
    pub outcome: ContinuitySnapshotImportOutcome,
    pub transaction: Option<MemoryWriteTransactionReport>,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpaceExportRequest {
    pub memory_space_id: String,
    pub include_private: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemorySpaceExportReport {
    pub memory_space_id: String,
    pub archive: MemorySpaceArchive,
    pub export_report: StoreSnapshotExportReport,
    pub privacy_redactions: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemorySpaceArchive {
    snapshot: StoreSnapshot,
}

impl MemorySpaceArchive {
    pub(crate) fn from_snapshot(snapshot: StoreSnapshot) -> Self {
        Self { snapshot }
    }

    pub(crate) fn snapshot(&self) -> &StoreSnapshot {
        &self.snapshot
    }

    pub(crate) fn into_snapshot(self) -> StoreSnapshot {
        self.snapshot
    }

    pub fn contains_json_namespace(&self, namespace: &str) -> bool {
        self.snapshot
            .json_docs
            .iter()
            .any(|doc| doc.namespace == namespace)
    }

    pub fn json_doc_count(&self) -> usize {
        self.snapshot.json_docs.len()
    }

    pub fn contains_event_plane(&self, plane: &str) -> bool {
        self.snapshot
            .events
            .iter()
            .any(|event| event.plane == plane)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemorySpaceImportRequest {
    pub memory_space_id: String,
    pub archive: MemorySpaceArchive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpaceImportReport {
    pub memory_space_id: String,
    pub import_report: StoreSnapshotImportReport,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemorySpaceMigratePreviewRequest {
    pub source_memory_space_id: String,
    pub target_memory_space_id: String,
    pub source_profile: ProfileId,
    pub target_profile: ProfileId,
    pub archive: MemorySpaceArchive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpaceMigrationPlaneReport {
    pub plane: String,
    pub records: usize,
    pub privacy_class: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpaceMigrationPrivacyReport {
    pub privacy_class: String,
    pub records: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpaceSubjectRemapReport {
    pub required: bool,
    pub applied: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpaceMigrationManifest {
    pub source_memory_space_id: String,
    pub target_memory_space_id: String,
    pub schema_id: String,
    pub whole_space_snapshot: bool,
    pub subject_remap: MemorySpaceSubjectRemapReport,
    pub planes: Vec<MemorySpaceMigrationPlaneReport>,
    pub privacy: Vec<MemorySpaceMigrationPrivacyReport>,
    pub conflict_risk: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemorySpaceMigratePreviewReport {
    pub source_memory_space_id: String,
    pub target_memory_space_id: String,
    pub schema_id: String,
    pub json_docs: usize,
    pub blobs: usize,
    pub events: usize,
    pub state_fingerprint: String,
    pub event_fingerprint: String,
    pub privacy_redactions: usize,
    pub loss_risk: bool,
    pub manifest: MemorySpaceMigrationManifest,
    pub vault_manifest: VaultManifest,
    pub vault_redaction: PrivateMaterialRedactionReport,
    pub vault_preflight: VaultMigrationPreflight,
    pub plan: MemorySpaceMigrationPlan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemorySpaceMigrationPlan {
    pub(crate) target_memory_space_id: String,
    pub(crate) snapshot: StoreSnapshot,
    pub(crate) preflight: VaultMigrationPreflight,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemorySpaceMigrateApplyRequest {
    pub plan: MemorySpaceMigrationPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpaceMigrateApplyReport {
    pub target_memory_space_id: String,
    pub import_report: StoreSnapshotImportReport,
}

#[derive(Clone, Debug)]
pub struct MemoryProceduralWriteReport {
    pub outcome: RuntimeSkillWriteOutcome,
}

#[derive(Clone, Debug)]
pub struct MemoryRecoverRequest {
    pub trigger: RuntimeLifecycleTrigger,
    pub mode_input: RuntimeLifecycleModeInput,
}

pub struct MemoryRecoverReport {
    pub report: crate::SoulKernelRecoveryReport,
    pub transaction: Option<MemoryWriteTransactionReport>,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct MemoryCloseRequest {
    pub reason: String,
}

pub struct MemoryCloseReport {
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeOperatorAction {
    InspectMemoryStatus,
    LongTermMemoryControl,
    LongTermMemoryPolicyControl,
    RecoverSoulKernel,
    CloseRuntime,
}

impl RuntimeOperatorAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InspectMemoryStatus => "inspect_memory_status",
            Self::LongTermMemoryControl => "long_term_memory_control",
            Self::LongTermMemoryPolicyControl => "long_term_memory_policy_control",
            Self::RecoverSoulKernel => "recover_soul_kernel",
            Self::CloseRuntime => "close_runtime",
        }
    }
}

pub struct RuntimeOperatorActionReport {
    pub action: RuntimeOperatorAction,
    pub accepted: bool,
    pub lifecycle: RuntimeLifecycleReport,
    pub surface: crate::MemoryOperatorSurfaceSummary,
    pub diagnosis: RuntimeLifecycleDiagnosisReport,
    pub safe_actions_available: Vec<String>,
}
