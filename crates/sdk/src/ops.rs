use bm_core::memory::IngressKind;
use bm_core::memory::{
    CanonicalTurnDelta, DeferredGovernanceQueueReport, MemoryHygieneInspection,
    MemoryHygieneOutcome, PostTurnMemoryGovernanceReport, PostTurnSemanticGovernanceReport,
    PrivateMaterialRedactionReport, ProceduralMemoryPromotionInput,
    ProceduralMemoryPromotionReport, ProjectionFaithfulnessCheck, SkillEvolutionReport,
    SubjectProjectionReport, SubjectScopedRuntime, VaultManifest, VaultMigrationPreflight,
};
use bm_core::memory::{CompactMemoryGraph, GraphRecallRerankReport, TemporalMemoryGraphGateReport};
use bm_core::skills::{
    AgentSkillDirectoryReport, AgentSkillProjectionAudit, AgentSkillRecallHit,
    AgentToolExperienceGovernanceReport, AgentToolExperienceStatusReport, AgentToolHint,
    AgentToolProjectionAudit, AgentToolRegistryRef, AgentToolRegistryReport,
    AgentToolUsageFeedback, ProjectedAgentSkillHint,
};
use bm_core::{budget::RuntimeRetentionQuotaReport, feature_gate::ProfileId};

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
    pub semantic_governance: Option<PostTurnSemanticGovernanceReport>,
    pub shared_fact_governance: Option<bm_core::memory::SharedMemoryWriteOutcome>,
    pub procedural_evolution: Option<SkillEvolutionReport>,
    pub procedural_promotions: Vec<ProceduralMemoryPromotionReport>,
    pub agent_tool_experience: Option<AgentToolExperienceGovernanceReport>,
}

#[derive(Clone, Debug)]
pub struct MemoryRecallRequest {
    pub query: String,
    pub limit: usize,
    pub tool_registry_refs: Vec<AgentToolRegistryRef>,
}

#[derive(Clone, Debug)]
pub struct MemoryRecallReport {
    pub query: String,
    pub procedural_hits: Vec<RuntimeSkillHit>,
    pub agent_skill_hits: Vec<AgentSkillRecallHit>,
    pub agent_tool_hints: Vec<AgentToolHint>,
    pub tool_experience_status: AgentToolExperienceStatusReport,
    pub working: WorkingRecallInspection,
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

pub type MemoryTurnFinalizeReport =
    PostTurnMemoryGovernanceReport<MemoryMaintenanceReport, RuntimeLifecycleReport>;

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
    RecoverSoulKernel,
    CloseRuntime,
}

impl RuntimeOperatorAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InspectMemoryStatus => "inspect_memory_status",
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
