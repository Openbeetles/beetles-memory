use bm_core::memory::IngressKind;
use bm_core::memory::{
    CanonicalTurnDelta, ConversationKey, DeferredGovernanceQueueReport, DerivedMemoryRef,
    HostOpaqueRef, LongTermMemoryQuery, MemoryGovernancePolicyMutation,
    MemoryGovernancePolicyMutationReport as CoreMemoryGovernancePolicyMutationReport,
    MemoryHygieneInspection, MemoryHygieneOutcome, MemoryLongTermAffectedRecord,
    MemoryLongTermControlDecision, MemoryLongTermControlView,
    MemoryLongTermDetailReport as CoreMemoryLongTermDetailReport,
    MemoryLongTermListReport as CoreMemoryLongTermListReport, MemoryLongTermMutation,
    MemoryLongTermMutationReport as CoreMemoryLongTermMutationReport, MemoryLongTermTarget,
    MemoryLongTermTargetResolutionReport, MemoryLongTermTombstoneRef, MemoryProjectionImpactReport,
    PostTurnPrivateGardenReport, PostTurnSemanticGovernanceReport, PrivateMaterialRedactionReport,
    ProceduralMemoryPromotionInput, ProceduralMemoryPromotionReport, ProjectionFaithfulnessCheck,
    RedactedTranscriptSlice, SessionTurnCommitReport, SkillEvolutionReport,
    SubjectProjectionReport, SubjectScopedRuntime, TranscriptAttrEnvelope,
    TranscriptAttrWriteRejection, TranscriptCommitReport, TranscriptEvidenceRef,
    TranscriptLifecycleReport, TranscriptLifecycleTransition, TranscriptRedactionReportItem,
    TranscriptRepairReport, TranscriptReplayView, VaultManifest, VaultMigrationPreflight,
};
use bm_core::memory::{
    CompactMemoryGraph, EvidenceBacklink, GraphRecallCandidateScore, GraphRecallRerankReport,
    MemoryGraphEdge, MemoryGraphNode, TemporalMemoryGraphGateReport,
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
use bm_store::StoreMutationBudgetReport;

use crate::{
    ContinuitySnapshot, ContinuitySnapshotImportMode, ContinuitySnapshotImportOutcome,
    IntelligenceReplayInspection, MemoryCapabilityCatalog, ParsedLongTermMemoryExtraction,
    PostReplyMemoryMaintenanceOutcome, PromptMemoryContext, RuntimeSkillHit,
    RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteOutcome, RuntimeSkillWriteSource,
    StoreSnapshot, StoreSnapshotExportReport, StoreSnapshotImportReport, WorkingRecallInspection,
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
    pub source_candidate_count: usize,
    pub matched_source_anchor_count: usize,
    pub source_anchor_ids: Vec<String>,
    pub unmatched_source_anchor_ids: Vec<String>,
    pub expanded_node_ids: Vec<String>,
    pub indexed_neighbor_count: usize,
    pub index_doc_count: usize,
    pub index_revision: Option<String>,
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
    pub expanded_candidates: Vec<MemoryEvalRecallCandidate>,
    pub graph_neighbors: Vec<String>,
    pub reranked_candidates: Vec<MemoryEvalRecallCandidate>,
    pub selected_candidates: Vec<MemoryEvalRecallCandidate>,
    pub rendered_block_preview: String,
    pub metrics: MemoryEvalRecallMetrics,
    pub missing_evidence_refs: Vec<String>,
    pub budget_report: RuntimeBudgetReport,
    pub privacy_report: MemoryEvalRecallPrivacyReport,
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
    pub index_revision: Option<String>,
    pub gate_failures: Vec<String>,
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
    pub graph_index_report: MemoryGraphRecallIndexReport,
    pub graph_rerank: GraphRecallRerankReport,
    pub graph_gate: TemporalMemoryGraphGateReport,
    pub compact_graph: CompactMemoryGraph,
    pub lifecycle_report: RuntimeLifecycleReport,
}

#[derive(Clone, Debug)]
pub struct MemoryProjectionRequest {
    pub user_query: String,
    pub system_max_len: usize,
    pub recent_messages_limit: usize,
    pub pressure: crate::PressureLevel,
    pub mode_input: RuntimeLifecycleModeInput,
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
    pub life_projection: SoulLifeProjectionReport,
    pub work_integrity: WorkIntegrityReport,
    pub subject_projection: SubjectProjectionReport,
    pub projection_faithfulness: ProjectionFaithfulnessCheck,
    pub private_disclosure_integrity: PrivateDisclosureIntegrityReport,
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
    pub snapshot: StoreSnapshot,
    pub export_report: StoreSnapshotExportReport,
    pub privacy_redactions: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemorySpaceImportRequest {
    pub memory_space_id: String,
    pub snapshot: StoreSnapshot,
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
    pub snapshot: StoreSnapshot,
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

#[derive(Clone, Debug, PartialEq, Eq)]
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemorySpaceMigrateApplyRequest {
    pub target_memory_space_id: String,
    pub snapshot: StoreSnapshot,
    pub preflight: VaultMigrationPreflight,
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
