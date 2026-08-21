use crate::store_internal::{StoreMutationBudgetReport, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION};
use bm_core::memory::IngressKind;
use bm_core::memory::{
    CanonicalTurnDelta, ConversationKey, DeferredGovernanceQueueReport, DerivedMemoryRef,
    GovernedEvidenceDocumentChunk, GovernedEvidenceDocumentDraft,
    GovernedEvidenceDocumentSourceKind, GovernedMemoryOwnerRef, HostOpaqueRef, LongTermMemoryQuery,
    MemoryEvidenceAuthority, MemoryGovernancePolicyMutation,
    MemoryGovernancePolicyMutationReport as CoreMemoryGovernancePolicyMutationReport,
    MemoryHygieneInspection, MemoryHygieneOutcome, MemoryLongTermAffectedFacetDoc,
    MemoryLongTermAffectedRecord, MemoryLongTermControlDecision, MemoryLongTermControlView,
    MemoryLongTermDetailReport as CoreMemoryLongTermDetailReport,
    MemoryLongTermListReport as CoreMemoryLongTermListReport, MemoryLongTermMutation,
    MemoryLongTermMutationReport as CoreMemoryLongTermMutationReport, MemoryLongTermTarget,
    MemoryLongTermTargetResolutionReport, MemoryLongTermTombstoneRef, MemoryMutationReceipt,
    MemoryPrivacyClass, MemoryProjectionImpactReport, PostTurnGovernanceAttemptAuthorityV2,
    PostTurnGovernanceErrorClassV2, PostTurnGovernanceJobV2, PostTurnPrivateGardenReport,
    PostTurnSemanticGovernanceReport, ProceduralMemoryPromotionInput,
    ProceduralMemoryPromotionReport, QueryFacetInput, RedactedTranscriptSlice, SessionMessage,
    SessionTurnCommitReport, SkillEvolutionReport, SubjectScopedRuntime, TranscriptAttrEnvelope,
    TranscriptAttrWriteRejection, TranscriptCommitReport, TranscriptEvidenceRef,
    TranscriptLifecycleReport, TranscriptLifecycleTransition, TranscriptRedactionReportItem,
    TranscriptRepairReport, TranscriptReplayView,
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
    AgentToolUsageFeedback, ProjectedAgentSkillHint, RuntimeSkillCreationRef,
    RuntimeSkillDeliveryDropReason, RuntimeSkillOwnerLocator, RuntimeSkillOwningScope,
};
use bm_core::{
    budget::{RuntimeBudgetReport, RuntimeRetentionQuotaReport},
    feature_gate::ProfileId,
    Error, Result,
};

use crate::{
    IntelligenceReplayInspection, MemoryCapabilityCatalog, ParsedLongTermMemoryExtraction,
    PostReplyMemoryMaintenanceOutcome, RuntimeSkillReuseOutcome, RuntimeSkillWrite,
    RuntimeSkillWriteOutcome, RuntimeSkillWriteSource, StoreSnapshot, WorkingRecallInspection,
};
use crate::{
    RuntimeLifecycleDiagnosisReport, RuntimeLifecycleModeInput, RuntimeLifecycleReport,
    RuntimeLifecycleTrigger,
};
use bm_core::memory::PromptProjectionSource;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillListRequest {
    pub owning_scope: RuntimeSkillOwningScope,
    pub query: Option<String>,
    pub include_disabled: bool,
    pub include_retired: bool,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillSummary {
    pub locator: RuntimeSkillOwnerLocator,
    pub owner_id: String,
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
    pub locator: RuntimeSkillOwnerLocator,
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
    pub locator: RuntimeSkillOwnerLocator,
    pub title: String,
    pub topic: String,
    pub summary: String,
    pub procedure: String,
    pub edit_reason: String,
    pub observed_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillMutationReport {
    pub accepted: bool,
    pub changed: bool,
    pub previous_locator: RuntimeSkillOwnerLocator,
    pub current_locator: RuntimeSkillOwnerLocator,
    pub owner_id: String,
    pub operation: &'static str,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillSetEnabledRequest {
    pub locator: RuntimeSkillOwnerLocator,
    pub enabled: bool,
    pub observed_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillRetireRequest {
    pub locator: RuntimeSkillOwnerLocator,
    pub observed_at: u64,
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
pub enum MemoryMutationExecution<T> {
    Committed {
        report: T,
        receipt: MemoryMutationReceipt,
    },
    Replayed {
        receipt: MemoryMutationReceipt,
    },
    Rejected {
        report: T,
    },
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum MemoryEvidenceDocumentMutation {
    Upsert {
        draft: Box<GovernedEvidenceDocumentDraft>,
    },
    Delete {
        document_id: String,
        expected_owner_revision: u64,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryWriteRequest {
    Procedural {
        writes: Vec<GovernedRuntimeSkillWriteInput>,
        owning_scope: RuntimeSkillOwningScope,
        source: RuntimeSkillWriteSource,
    },
    ProceduralPromotions {
        promotions: Vec<ProceduralMemoryPromotionInput>,
        owning_scope: RuntimeSkillOwningScope,
        source: RuntimeSkillWriteSource,
    },
    LongTermExtraction {
        extraction: ParsedLongTermMemoryExtraction,
        governed_skill_writes: Vec<GovernedRuntimeSkillWriteInput>,
        runtime_skill_owning_scope: Option<RuntimeSkillOwningScope>,
    },
    Candidates {
        candidates: Vec<bm_core::memory::MemoryWriteCandidate>,
        runtime_skill_owning_scope: Option<RuntimeSkillOwningScope>,
    },
    GovernedEvidenceDocuments {
        mutations: Vec<MemoryEvidenceDocumentMutation>,
    },
    AgentToolUsageFeedback {
        feedback: AgentToolUsageFeedback,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GovernedRuntimeSkillWriteInput {
    pub write: RuntimeSkillWrite,
    pub creation_ref: RuntimeSkillCreationRef,
    pub privacy_class: MemoryPrivacyClass,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvidenceDocumentWriteSummary {
    pub submitted: usize,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub deleted: usize,
    pub owner_refs: Vec<GovernedMemoryOwnerRef>,
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
    pub evidence_documents: Option<MemoryEvidenceDocumentWriteSummary>,
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
    pub mutation_receipt: Option<MemoryMutationReceipt>,
    pub mutation_replayed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryRecallTemporalOperation {
    Current,
    HistoricalAsOf { as_of_time: u64 },
}

impl<'de> Deserialize<'de> for MemoryRecallTemporalOperation {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum StrictTemporalOperation {
            Current {},
            HistoricalAsOf { as_of_time: u64 },
        }

        match StrictTemporalOperation::deserialize(deserializer)? {
            StrictTemporalOperation::Current {} => Ok(Self::Current),
            StrictTemporalOperation::HistoricalAsOf { as_of_time } => {
                Ok(Self::HistoricalAsOf { as_of_time })
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct MemoryRecallRequest {
    pub query: String,
    pub limit: usize,
    pub structured_query_facets: Vec<QueryFacetInput>,
    pub tool_registry_refs: Vec<AgentToolRegistryRef>,
    pub temporal_operation: MemoryRecallTemporalOperation,
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

pub const MEMORY_RECALL_DELIVERY_SCHEMA_VERSION: u32 = 4;
pub const MEMORY_PROJECTION_DELIVERY_DIGEST_SCHEMA_VERSION: u32 = 3;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryEvidenceDocumentReadRequest {
    pub memory_space_id: String,
    pub document_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryEvidenceDocumentView {
    pub owner_ref: GovernedMemoryOwnerRef,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub source_kind: GovernedEvidenceDocumentSourceKind,
    pub source_locator_view: MemoryEvidenceRefView,
    pub canonical_evidence_group: String,
    pub source_revision: u64,
    pub owner_revision: u64,
    pub content_digest: String,
    pub authority: MemoryEvidenceAuthority,
    pub privacy: MemoryPrivacyClass,
    pub body: String,
    pub chunks: Vec<GovernedEvidenceDocumentChunk>,
    pub observed_at: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub shared_fact_surface_allowed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryEvidenceDocumentReadReport {
    pub documents: Vec<MemoryEvidenceDocumentView>,
    pub missing_document_ids: Vec<String>,
    pub store_snapshot_consistent: bool,
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
    pub owner_ref: Option<GovernedMemoryOwnerRef>,
    pub candidate_id: String,
    pub canonical_evidence_groups: Vec<String>,
    pub evidence_family_groups: Vec<String>,
    pub renderable_evidence_groups: Vec<String>,
    pub selected: bool,
    pub drop_reason: Option<MemoryRecallSelectionDropReason>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRenderedEvidenceCapsule {
    pub owner_ref: GovernedMemoryOwnerRef,
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
pub struct MemoryEvalRecallStageCandidateMatch {
    pub candidate_id: String,
    pub rank: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryEvalRecallCandidateSelectionLoss {
    pub candidate_id: String,
    pub drop_reason: MemoryRecallSelectionDropReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryEvalRecallCandidateRenderLoss {
    pub candidate_id: String,
    pub drop_reason: MemoryRecallRenderDropReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryEvalRecallLossEntry {
    pub evidence_ref: MemoryEvidenceRefView,
    pub canonical_evidence_group: String,
    pub expanded_matches: Vec<MemoryEvalRecallStageCandidateMatch>,
    pub reranked_matches: Vec<MemoryEvalRecallStageCandidateMatch>,
    pub selected_matches: Vec<MemoryEvalRecallStageCandidateMatch>,
    pub rendered_matches: Vec<MemoryEvalRecallStageCandidateMatch>,
    pub selection_losses: Vec<MemoryEvalRecallCandidateSelectionLoss>,
    pub render_losses: Vec<MemoryEvalRecallCandidateRenderLoss>,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoryEvalEvidenceApplicability {
    #[default]
    Evidence,
    NoGold,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEvalQuestionEvaluation {
    pub canonical_gold_count: usize,
    pub applicability: MemoryEvalEvidenceApplicability,
}

impl MemoryEvalQuestionEvaluation {
    pub fn from_canonical_gold_count(canonical_gold_count: usize) -> Self {
        Self {
            canonical_gold_count,
            applicability: if canonical_gold_count == 0 {
                MemoryEvalEvidenceApplicability::NoGold
            } else {
                MemoryEvalEvidenceApplicability::Evidence
            },
        }
    }
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
    pub checked_long_term_owner_count: u32,
    pub checked_evidence_document_owner_count: u32,
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
    pub question_evaluation: MemoryEvalQuestionEvaluation,
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
pub struct TemporalMemoryGraphNodeOwnerRef {
    pub node_id: String,
    pub owner_ref: GovernedMemoryOwnerRef,
}

#[derive(Clone, Debug)]
pub struct TemporalMemoryGraphWriteRequest {
    pub operation: String,
    pub nodes: Vec<MemoryGraphNode>,
    pub node_owners: Vec<TemporalMemoryGraphNodeOwnerRef>,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralMemoryDeliveryView {
    pub candidate_ref: String,
    pub matched: bool,
    pub selected: bool,
    pub rendered: bool,
    pub drop_reasons: Vec<RuntimeSkillDeliveryDropReason>,
}

#[derive(Clone, Debug)]
pub struct MemoryRecallReport {
    pub query: String,
    pub temporal_operation: MemoryRecallTemporalOperation,
    pub procedural_delivery_reports: Vec<ProceduralMemoryDeliveryView>,
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
    pub privacy_report: MemoryEvalRecallPrivacyReport,
    pub store_snapshot_consistent: bool,
    pub lifecycle_report: RuntimeLifecycleReport,
    pub(crate) governed_public_report: Option<crate::GovernedRecallPublicReportV1>,
    pub(crate) governed_operator_report: Option<crate::GovernedRecallOperatorReportV1>,
}

impl MemoryRecallReport {
    pub fn governed_public_report(&self) -> &crate::GovernedRecallPublicReportV1 {
        self.governed_public_report
            .as_ref()
            .expect("production recall must finalize its governed public report")
    }

    pub fn governed_operator_report(&self) -> &crate::GovernedRecallOperatorReportV1 {
        self.governed_operator_report
            .as_ref()
            .expect("production recall must finalize its governed operator report")
    }
}

#[derive(Clone, Debug)]
pub struct MemoryProjectionRequest {
    pub temporal_operation: MemoryRecallTemporalOperation,
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
    pub owner_ref: Option<GovernedMemoryOwnerRef>,
    pub source_id: String,
    pub role: String,
    pub content: String,
    pub evidence_refs: Vec<String>,
    pub protected: bool,
    pub shared_fact_surface_allowed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryProjectionDeliveryDigestContentEntry {
    pub owner_ref: GovernedMemoryOwnerRef,
    pub candidate_id: String,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryProjectionDeliveryDigestEntry {
    pub owner_ref: GovernedMemoryOwnerRef,
    pub candidate_id: String,
    pub source_block_sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemoryProjectionProceduralDigestContentEntry {
    pub candidate_ref: String,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemoryProjectionProceduralDigestReceipt {
    pub candidate_ref: String,
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
    pub(crate) procedural_block_entries: Vec<MemoryProjectionProceduralDigestContentEntry>,
    pub(crate) procedural_prompt_visible_entries: Vec<MemoryProjectionProceduralDigestContentEntry>,
    pub(crate) procedural_candidate_receipts: Vec<MemoryProjectionProceduralDigestReceipt>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProjectionSafeAuditReport {
    pub projection_id: String,
    pub source_budget_chars: usize,
    pub render_budget_chars: usize,
    pub injected: bool,
    pub truncated: bool,
    pub runtime_private_context_allowed: bool,
    pub foreground_disclosure_allowed: bool,
    pub private_gate_reason: String,
    pub evidence_ref_count: usize,
    pub budget_decision_count: usize,
    pub privacy_decision_count: usize,
    pub dropped_candidate_count: usize,
    pub faithfulness_passed: bool,
    pub unsupported_claim_count: usize,
    pub disclosure_integrity_passed: bool,
    pub raw_private_violation_count: u32,
    pub graph_used: bool,
    pub graph_maintenance_required: bool,
    pub graph_read_path_mutation_delta: usize,
    pub delivery_digest_verified: bool,
    pub delivery_digest_candidate_count: usize,
    pub agent_skill_selected_count: usize,
    pub agent_tool_selected_count: usize,
    pub agent_tool_cold_start_selection_used: bool,
    pub agent_tool_rejection_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProjectionGatewayAuditView {
    pub projection_id: String,
    pub provider_projection_chars: usize,
    pub block: String,
    pub redacted: bool,
    pub redacted_source_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRecallDeliverySafeView {
    pub selected_count: usize,
    pub rendered_count: usize,
    pub redacted_candidate_count: usize,
    pub rendered_chars: usize,
    pub render_growth: usize,
    pub integrity_failures: Vec<String>,
    pub delivery_drop_reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderProjectionMaintenanceCarry {
    pub(crate) runtime_skill_selected_ids: Vec<String>,
    pub(crate) task_learning_selected_ids: Vec<String>,
}

impl ProviderProjectionMaintenanceCarry {
    pub fn runtime_skill_selected_ids(&self) -> &[String] {
        &self.runtime_skill_selected_ids
    }

    pub fn task_learning_selected_ids(&self) -> &[String] {
        &self.task_learning_selected_ids
    }
}

#[derive(Clone, Debug)]
pub struct ProviderProjectionPayload {
    pub(crate) system_memory_block: String,
    pub(crate) recent_messages: Vec<SessionMessage>,
    pub(crate) agent_tool_hints: Vec<AgentToolHint>,
    pub(crate) maintenance_carry: ProviderProjectionMaintenanceCarry,
}

impl ProviderProjectionPayload {
    pub fn system_memory_block(&self) -> &str {
        &self.system_memory_block
    }

    pub fn recent_messages(&self) -> &[SessionMessage] {
        &self.recent_messages
    }

    pub fn agent_tool_hints(&self) -> &[AgentToolHint] {
        &self.agent_tool_hints
    }

    pub fn maintenance_carry(&self) -> &ProviderProjectionMaintenanceCarry {
        &self.maintenance_carry
    }
}

/// Public projection evidence intentionally excludes provider/private material.
///
/// ```compile_fail
/// fn provider_prompt_is_not_public(report: &bm_sdk::MemoryProjectionReport) {
///     let _ = report.system_memory_block();
/// }
/// ```
///
/// ```compile_fail
/// fn raw_context_is_not_public(report: &bm_sdk::MemoryProjectionReport) {
///     let _ = report.context;
/// }
/// ```
///
/// ```compile_fail
/// fn runtime_envelope_is_not_public(report: &bm_sdk::MemoryProjectionReport) {
///     let _ = report.runtime_projection;
/// }
/// ```
///
/// ```compile_fail
/// fn digest_manifest_is_not_public(report: &bm_sdk::MemoryProjectionReport) {
///     let _ = report.delivery_digest_manifest;
/// }
/// ```
pub struct MemoryProjectionReport {
    pub(crate) temporal_operation: MemoryRecallTemporalOperation,
    pub(crate) ui_api_projection: String,
    pub(crate) ui_api_chars: usize,
    pub(crate) operator_projection: String,
    pub(crate) shared_fact_projection: String,
    pub(crate) agent_tool_hints: Vec<AgentToolHint>,
    pub(crate) audit: MemoryProjectionSafeAuditReport,
    pub(crate) gateway_audit: MemoryProjectionGatewayAuditView,
    pub(crate) procedural_delivery_reports: Vec<ProceduralMemoryDeliveryView>,
    pub(crate) recall_delivery: MemoryRecallDeliverySafeView,
    pub(crate) lifecycle_report: RuntimeLifecycleReport,
    pub(crate) governed_public_report: crate::GovernedRecallPublicReportV1,
    pub(crate) governed_operator_report: crate::GovernedRecallOperatorReportV1,
}

impl MemoryProjectionReport {
    pub const fn temporal_operation(&self) -> MemoryRecallTemporalOperation {
        self.temporal_operation
    }

    pub fn ui_api_projection(&self) -> &str {
        &self.ui_api_projection
    }

    pub const fn ui_api_chars(&self) -> usize {
        self.ui_api_chars
    }

    pub fn operator_projection(&self) -> &str {
        &self.operator_projection
    }

    pub fn shared_fact_projection(&self) -> &str {
        &self.shared_fact_projection
    }

    pub fn agent_tool_hints(&self) -> &[AgentToolHint] {
        &self.agent_tool_hints
    }

    pub fn audit(&self) -> &MemoryProjectionSafeAuditReport {
        &self.audit
    }

    pub fn gateway_audit(&self) -> &MemoryProjectionGatewayAuditView {
        &self.gateway_audit
    }

    pub fn procedural_delivery_reports(&self) -> &[ProceduralMemoryDeliveryView] {
        &self.procedural_delivery_reports
    }

    pub fn recall_delivery(&self) -> &MemoryRecallDeliverySafeView {
        &self.recall_delivery
    }

    pub fn governed_public_report(&self) -> &crate::GovernedRecallPublicReportV1 {
        &self.governed_public_report
    }

    pub fn governed_operator_report(&self) -> &crate::GovernedRecallOperatorReportV1 {
        &self.governed_operator_report
    }

    pub fn lifecycle_report(&self) -> &RuntimeLifecycleReport {
        &self.lifecycle_report
    }
}

pub struct MemoryProjectionOutput {
    pub(crate) provider_payload: ProviderProjectionPayload,
    pub(crate) report: MemoryProjectionReport,
}

impl MemoryProjectionOutput {
    pub fn provider_payload(&self) -> &ProviderProjectionPayload {
        &self.provider_payload
    }

    pub fn report(&self) -> &MemoryProjectionReport {
        &self.report
    }

    pub fn into_parts(self) -> (ProviderProjectionPayload, MemoryProjectionReport) {
        (self.provider_payload, self.report)
    }
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

#[derive(Clone, Debug, Serialize)]
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
    pub memory_consolidation: MemoryConsolidationReport,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryConsolidationState {
    NotScheduled,
    Queued,
    Succeeded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryConsolidationReport {
    pub state: MemoryConsolidationState,
    pub job_id: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobStatusRequest {
    pub job_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobStatusReport {
    pub job: PostTurnGovernanceJobV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceActiveJobsRequest {
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceActiveJobsReport {
    pub jobs: Vec<PostTurnGovernanceJobV2>,
    pub has_more: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceAttemptAuthorityRequest {
    pub job_id: String,
    pub binding_id: String,
    pub config_revision: u64,
    pub model_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceAttemptAuthorityReport {
    pub authority: PostTurnGovernanceAttemptAuthorityV2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryGovernanceBlockKind {
    Configuration,
    Capability,
    Policy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobBlockRequest {
    pub job_id: String,
    pub kind: MemoryGovernanceBlockKind,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobBlockReport {
    pub job: PostTurnGovernanceJobV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceClaimedJobBlockRequest {
    pub job_id: String,
    pub lease_owner: String,
    pub lease_epoch: u64,
    pub kind: MemoryGovernanceBlockKind,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceClaimedJobBlockReport {
    pub job: PostTurnGovernanceJobV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobResumeRequest {
    pub job_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobResumeReport {
    pub job: PostTurnGovernanceJobV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobFailRequest {
    pub job_id: String,
    pub lease_owner: String,
    pub lease_epoch: u64,
    pub error_class: PostTurnGovernanceErrorClassV2,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobFailReport {
    pub job: PostTurnGovernanceJobV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceReconcileRequest {
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceReconcileReport {
    pub inspected: usize,
    pub created: usize,
    pub cursor_sequence: u64,
    pub has_more: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobClaimRequest {
    pub job_id: String,
    pub lease_owner: String,
    pub lease_until: u64,
    pub authority: PostTurnGovernanceAttemptAuthorityV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobClaimReport {
    pub job: PostTurnGovernanceJobV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobRenewRequest {
    pub job_id: String,
    pub lease_owner: String,
    pub lease_epoch: u64,
    pub lease_until: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobRenewReport {
    pub job: PostTurnGovernanceJobV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobRetryRequest {
    pub job_id: String,
    pub lease_owner: String,
    pub lease_epoch: u64,
    pub error_class: PostTurnGovernanceErrorClassV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobRetryReport {
    pub job: PostTurnGovernanceJobV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobRunRequest {
    pub job_id: String,
    pub lease_owner: String,
    pub lease_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryGovernanceJobRunReport {
    pub job: PostTurnGovernanceJobV2,
    pub private_garden_self_work: PostTurnPrivateGardenReport,
    pub semantic_governance: PostTurnSemanticGovernanceReport,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemorySpaceScope {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryArchiveScope {
    Subject {
        memory_space_id: String,
        mounted_subject_id: String,
    },
    SharedProgram {
        memory_space_id: String,
    },
}

impl MemoryArchiveScope {
    pub fn subject(
        memory_space_id: impl Into<String>,
        mounted_subject_id: impl Into<String>,
    ) -> Result<Self> {
        let scope = Self::Subject {
            memory_space_id: memory_space_id.into(),
            mounted_subject_id: mounted_subject_id.into(),
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn shared_program(memory_space_id: impl Into<String>) -> Result<Self> {
        let scope = Self::SharedProgram {
            memory_space_id: memory_space_id.into(),
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate_exact_identity(&self, actual: &Self) -> Result<()> {
        self.validate()?;
        actual.validate()?;
        if self == actual {
            Ok(())
        } else {
            Err(Error::config(
                "memory_archive_scope",
                "archive scope kind or identity does not match exactly",
            ))
        }
    }

    pub fn memory_space_id(&self) -> &str {
        match self {
            Self::Subject {
                memory_space_id, ..
            }
            | Self::SharedProgram { memory_space_id } => memory_space_id,
        }
    }

    pub fn mounted_subject_id(&self) -> Option<&str> {
        match self {
            Self::Subject {
                mounted_subject_id, ..
            } => Some(mounted_subject_id),
            Self::SharedProgram { .. } => None,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Subject {
                memory_space_id,
                mounted_subject_id,
            } => {
                require_canonical_archive_identity(memory_space_id, "memory_space_id")?;
                require_canonical_archive_identity(mounted_subject_id, "mounted_subject_id")?;
            }
            Self::SharedProgram { memory_space_id } => {
                require_canonical_archive_identity(memory_space_id, "memory_space_id")?;
            }
        }
        Ok(())
    }

    fn digest_fields(&self) -> (&'static str, &str, Option<&str>) {
        match self {
            Self::Subject {
                memory_space_id,
                mounted_subject_id,
            } => ("subject", memory_space_id, Some(mounted_subject_id)),
            Self::SharedProgram { memory_space_id } => ("shared_program", memory_space_id, None),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum GovernedScopeArchiveEntryKind {
    Json,
    Event,
}

impl GovernedScopeArchiveEntryKind {
    const fn discriminant(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Event => "event",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedScopeArchiveEntry {
    kind: GovernedScopeArchiveEntryKind,
    namespace_or_plane: String,
    key_or_event_id: String,
    canonical_bytes: usize,
    content_sha256: String,
}

impl GovernedScopeArchiveEntry {
    pub fn json(namespace: &str, key: &str, value: &Value) -> Result<Self> {
        Self::from_value(GovernedScopeArchiveEntryKind::Json, namespace, key, value)
    }

    pub fn event(plane: &str, event_id: &str, value: &Value) -> Result<Self> {
        Self::from_value(GovernedScopeArchiveEntryKind::Event, plane, event_id, value)
    }

    fn from_value(
        kind: GovernedScopeArchiveEntryKind,
        namespace_or_plane: &str,
        key_or_event_id: &str,
        value: &Value,
    ) -> Result<Self> {
        require_canonical_archive_identity(namespace_or_plane, "namespace_or_plane")?;
        require_canonical_archive_identity(key_or_event_id, "key_or_event_id")?;
        let canonical = canonical_archive_json(value)?;
        Ok(Self {
            kind,
            namespace_or_plane: namespace_or_plane.to_string(),
            key_or_event_id: key_or_event_id.to_string(),
            canonical_bytes: canonical.len(),
            content_sha256: format!("{:x}", Sha256::digest(&canonical)),
        })
    }
}

pub const GOVERNED_SCOPE_ARCHIVE_ROOT_SCHEMA_VERSION: u32 = 1;
const GOVERNED_SCOPE_ARCHIVE_ROOT_DIGEST_DOMAIN: &str =
    "beetle_memory_governed_scope_archive_root_v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedScopeArchiveRootV1 {
    pub schema_version: u32,
    pub store_schema_id: String,
    pub store_schema_version: u32,
    pub scope: MemoryArchiveScope,
    pub private_material_policy: MemorySpacePrivateMaterialPolicy,
    pub json_doc_count: u64,
    pub event_count: u64,
    pub json_bytes: u64,
    pub event_bytes: u64,
    pub closure_sha256: String,
}

impl GovernedScopeArchiveRootV1 {
    pub fn build(
        scope: MemoryArchiveScope,
        private_material_policy: MemorySpacePrivateMaterialPolicy,
        entries: impl IntoIterator<Item = GovernedScopeArchiveEntry>,
    ) -> Result<Self> {
        scope.validate()?;
        let mut entries = entries.into_iter().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.namespace_or_plane.cmp(&right.namespace_or_plane))
                .then_with(|| left.key_or_event_id.cmp(&right.key_or_event_id))
        });
        if entries.windows(2).any(|pair| {
            pair[0].kind == pair[1].kind
                && pair[0].namespace_or_plane == pair[1].namespace_or_plane
                && pair[0].key_or_event_id == pair[1].key_or_event_id
        }) {
            return Err(Error::config(
                "governed_scope_archive_root",
                "archive entries contain a duplicate canonical address",
            ));
        }

        let mut json_doc_count = 0_usize;
        let mut event_count = 0_usize;
        let mut json_bytes = 0_usize;
        let mut event_bytes = 0_usize;
        for entry in &entries {
            match entry.kind {
                GovernedScopeArchiveEntryKind::Json => {
                    json_doc_count = json_doc_count.checked_add(1).ok_or_else(|| {
                        Error::config("governed_scope_archive_root", "JSON count overflow")
                    })?;
                    json_bytes =
                        json_bytes
                            .checked_add(entry.canonical_bytes)
                            .ok_or_else(|| {
                                Error::config(
                                    "governed_scope_archive_root",
                                    "JSON byte count overflow",
                                )
                            })?;
                }
                GovernedScopeArchiveEntryKind::Event => {
                    event_count = event_count.checked_add(1).ok_or_else(|| {
                        Error::config("governed_scope_archive_root", "event count overflow")
                    })?;
                    event_bytes =
                        event_bytes
                            .checked_add(entry.canonical_bytes)
                            .ok_or_else(|| {
                                Error::config(
                                    "governed_scope_archive_root",
                                    "event byte count overflow",
                                )
                            })?;
                }
            }
        }

        let closure_sha256 = archive_root_digest(
            &scope,
            private_material_policy,
            json_doc_count,
            event_count,
            json_bytes,
            event_bytes,
            &entries,
        )?;
        Ok(Self {
            schema_version: GOVERNED_SCOPE_ARCHIVE_ROOT_SCHEMA_VERSION,
            store_schema_id: STORE_SCHEMA_ID.to_string(),
            store_schema_version: STORE_SCHEMA_VERSION,
            scope,
            private_material_policy,
            json_doc_count: bounded_archive_count("json_doc_count", json_doc_count)?,
            event_count: bounded_archive_count("event_count", event_count)?,
            json_bytes: bounded_archive_count("json_bytes", json_bytes)?,
            event_bytes: bounded_archive_count("event_bytes", event_bytes)?,
            closure_sha256,
        })
    }
}

fn require_canonical_archive_identity(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value != value.trim() {
        return Err(Error::config(
            "memory_archive_scope",
            format!("{field} must be a canonical non-empty value"),
        ));
    }
    Ok(())
}

fn bounded_archive_count(field: &str, value: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_| {
        Error::config(
            "governed_scope_archive_root",
            format!("{field} cannot be represented by the fixed-size archive root"),
        )
    })
}

fn canonical_archive_json(value: &Value) -> Result<Vec<u8>> {
    fn write(value: &Value, output: &mut Vec<u8>) -> Result<()> {
        match value {
            Value::Array(values) => {
                output.push(b'[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(b',');
                    }
                    write(value, output)?;
                }
                output.push(b']');
            }
            Value::Object(values) => {
                output.push(b'{');
                let mut keys = values.keys().collect::<Vec<_>>();
                keys.sort();
                for (index, key) in keys.into_iter().enumerate() {
                    if index != 0 {
                        output.push(b',');
                    }
                    serde_json::to_writer(&mut *output, key).map_err(|error| {
                        Error::config("governed_scope_archive_json", error.to_string())
                    })?;
                    output.push(b':');
                    write(&values[key], output)?;
                }
                output.push(b'}');
            }
            _ => serde_json::to_writer(&mut *output, value)
                .map_err(|error| Error::config("governed_scope_archive_json", error.to_string()))?,
        }
        Ok(())
    }

    let mut output = Vec::new();
    write(value, &mut output)?;
    Ok(output)
}

fn archive_root_digest(
    scope: &MemoryArchiveScope,
    private_material_policy: MemorySpacePrivateMaterialPolicy,
    json_doc_count: usize,
    event_count: usize,
    json_bytes: usize,
    event_bytes: usize,
    entries: &[GovernedScopeArchiveEntry],
) -> Result<String> {
    let mut hasher = Sha256::new();
    hash_archive_field(
        &mut hasher,
        GOVERNED_SCOPE_ARCHIVE_ROOT_DIGEST_DOMAIN.as_bytes(),
    );
    hash_archive_field(
        &mut hasher,
        &GOVERNED_SCOPE_ARCHIVE_ROOT_SCHEMA_VERSION.to_be_bytes(),
    );
    hash_archive_field(&mut hasher, STORE_SCHEMA_ID.as_bytes());
    hash_archive_field(&mut hasher, &STORE_SCHEMA_VERSION.to_be_bytes());
    let (scope_kind, memory_space_id, mounted_subject_id) = scope.digest_fields();
    hash_archive_field(&mut hasher, scope_kind.as_bytes());
    hash_archive_field(&mut hasher, memory_space_id.as_bytes());
    hash_archive_field(
        &mut hasher,
        mounted_subject_id.unwrap_or_default().as_bytes(),
    );
    hash_archive_field(
        &mut hasher,
        match private_material_policy {
            MemorySpacePrivateMaterialPolicy::ExcludePrivate => b"exclude_private",
            MemorySpacePrivateMaterialPolicy::IncludePrivate => b"include_private",
        },
    );
    for count in [json_doc_count, event_count, json_bytes, event_bytes] {
        let count = u64::try_from(count).map_err(|_| {
            Error::config(
                "governed_scope_archive_root",
                "archive count cannot be represented canonically",
            )
        })?;
        hash_archive_field(&mut hasher, &count.to_be_bytes());
    }
    for entry in entries {
        hash_archive_field(&mut hasher, entry.kind.discriminant().as_bytes());
        hash_archive_field(&mut hasher, entry.namespace_or_plane.as_bytes());
        hash_archive_field(&mut hasher, entry.key_or_event_id.as_bytes());
        let canonical_bytes = u64::try_from(entry.canonical_bytes).map_err(|_| {
            Error::config(
                "governed_scope_archive_root",
                "entry byte count cannot be represented canonically",
            )
        })?;
        hash_archive_field(&mut hasher, &canonical_bytes.to_be_bytes());
        hash_archive_field(&mut hasher, entry.content_sha256.as_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_archive_field(hasher: &mut Sha256, field: &[u8]) {
    hasher.update((field.len() as u64).to_be_bytes());
    hasher.update(field);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpaceProjectionScope {
    pub scope: MemoryArchiveScope,
    pub private_material_policy: MemorySpacePrivateMaterialPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpaceExportRequest {
    pub scope: MemoryArchiveScope,
    pub private_material_policy: MemorySpacePrivateMaterialPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySpacePrivateMaterialPolicy {
    ExcludePrivate,
    IncludePrivate,
}

impl MemorySpacePrivateMaterialPolicy {
    pub const fn includes_private(self) -> bool {
        matches!(self, Self::IncludePrivate)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemorySpaceExportReport {
    pub projection_scope: MemorySpaceProjectionScope,
    pub archive: MemorySpaceArchive,
    pub privacy_redactions: usize,
}

#[derive(Clone, PartialEq)]
pub struct MemorySpaceArchive {
    root: GovernedScopeArchiveRootV1,
    snapshot: StoreSnapshot,
}

impl std::fmt::Debug for MemorySpaceArchive {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let report = self.snapshot.export_report();
        formatter
            .debug_struct("MemorySpaceArchive")
            .field("root", &self.root)
            .field("diagnostic_schema_id", &report.schema_id)
            .field("diagnostic_json_doc_count", &report.json_docs)
            .field("blob_count", &report.blobs)
            .field("diagnostic_event_count", &report.events)
            .finish()
    }
}

impl MemorySpaceArchive {
    pub(crate) fn from_snapshot(root: GovernedScopeArchiveRootV1, snapshot: StoreSnapshot) -> Self {
        Self { root, snapshot }
    }

    pub fn root(&self) -> &GovernedScopeArchiveRootV1 {
        &self.root
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn with_replaced_root_for_nonproduction_harness(
        &self,
        root: GovernedScopeArchiveRootV1,
    ) -> Self {
        Self {
            root,
            snapshot: self.snapshot.clone(),
        }
    }

    pub(crate) fn snapshot(&self) -> &StoreSnapshot {
        &self.snapshot
    }

    pub fn contains_json_namespace(&self, namespace: &str) -> bool {
        self.snapshot
            .json_docs
            .iter()
            .any(|doc| doc.namespace == namespace)
    }

    pub fn contains_json_address(&self, namespace: &str, key: &str) -> bool {
        self.snapshot
            .json_docs
            .iter()
            .any(|doc| doc.namespace == namespace && doc.key == key)
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
    pub scope: MemoryArchiveScope,
    pub expected_private_material_policy: MemorySpacePrivateMaterialPolicy,
    pub archive: MemorySpaceArchive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySpaceImportReport {
    pub imported_scope: MemoryArchiveScope,
    pub archive_root: GovernedScopeArchiveRootV1,
    pub deleted_json_docs: usize,
    pub inserted_json_docs: usize,
    pub deleted_events: usize,
    pub inserted_events: usize,
}

#[derive(Clone, Debug)]
pub struct MemoryProceduralWriteReport {
    pub outcome: RuntimeSkillWriteOutcome,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryRecoverRequest {
    pub trigger: RuntimeLifecycleTrigger,
    pub mode_input: RuntimeLifecycleModeInput,
}

pub struct MemoryRecoverReport {
    pub report: crate::SoulKernelRecoveryReport,
    pub transaction: Option<MemoryWriteTransactionReport>,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug, Serialize)]
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
