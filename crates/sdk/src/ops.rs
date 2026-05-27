use bm_core::memory::IngressKind;
use bm_core::memory::{
    CanonicalTurnDelta, DeferredGovernanceQueueReport, MemoryHygieneInspection,
    MemoryHygieneOutcome, PostTurnMemoryGovernanceReport, PostTurnSemanticGovernanceReport,
    PrivateEchoGuardReport, PrivateMaterialRedactionReport, ProceduralMemoryPromotionInput,
    ProceduralMemoryPromotionReport, ProjectionFaithfulnessCheck, SkillEvolutionReport,
    SubjectProjectionReport, VaultManifest, VaultMigrationPreflight,
};
use bm_core::memory::{CompactMemoryGraph, GraphRecallRerankReport, TemporalMemoryGraphGateReport};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemorySkillOrigin {
    UserProvided,
    RuntimeLearned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemorySkillKind {
    RuntimeSkill,
    ManualDocument,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySkillListRequest {
    pub query: Option<String>,
    pub include_disabled: bool,
    pub include_retired: bool,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySkillSummary {
    pub name: String,
    pub kind: MemorySkillKind,
    pub origin: MemorySkillOrigin,
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
pub struct MemorySkillListReport {
    pub total: usize,
    pub active: usize,
    pub disabled: usize,
    pub runtime_learned: usize,
    pub user_provided: usize,
    pub skills: Vec<MemorySkillSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySkillDetailRequest {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySkillDetailReport {
    pub summary: MemorySkillSummary,
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
pub struct MemorySkillUpsertRequest {
    pub name: Option<String>,
    pub title: String,
    pub topic: String,
    pub summary: String,
    pub procedure: String,
    pub citations: Vec<String>,
    pub source_chat_id: Option<String>,
    pub observed_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySkillMutationReport {
    pub accepted: bool,
    pub changed: bool,
    pub name: String,
    pub operation: &'static str,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySkillSetEnabledRequest {
    pub name: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorySkillDeleteRequest {
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryWriteReport {
    pub accepted: bool,
    pub changed: usize,
    pub operation: &'static str,
    pub reason: String,
    pub lifecycle_report: RuntimeLifecycleReport,
    pub semantic_governance: Option<PostTurnSemanticGovernanceReport>,
    pub procedural_evolution: Option<SkillEvolutionReport>,
    pub procedural_promotions: Vec<ProceduralMemoryPromotionReport>,
}

#[derive(Clone, Debug)]
pub struct MemoryRecallRequest {
    pub query: String,
    pub limit: usize,
}

#[derive(Clone, Debug)]
pub struct MemoryRecallReport {
    pub query: String,
    pub procedural_hits: Vec<RuntimeSkillHit>,
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
}

pub struct MemoryProjectionReport {
    pub system_memory_block: String,
    pub context: PromptMemoryContext,
    pub audit: MemoryProjectionAuditReport,
    pub subject_projection: SubjectProjectionReport,
    pub projection_faithfulness: ProjectionFaithfulnessCheck,
    pub private_echo_guard: PrivateEchoGuardReport,
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
    pub allowed: bool,
    pub privacy_policy_allowed: bool,
    pub lifecycle_private_depth_allowed: bool,
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
    pub conversation_id: Option<String>,
    pub source_budget_chars: usize,
    pub render_budget_chars: usize,
    pub system_memory_chars: usize,
    pub injected: bool,
    pub truncated: bool,
    pub private_gate: MemoryProjectionPrivateGateAudit,
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
