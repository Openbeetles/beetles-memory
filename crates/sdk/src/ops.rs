use bm_core::memory::IngressKind;

use crate::{
    ContinuitySnapshot, ContinuitySnapshotImportMode, ContinuitySnapshotImportOutcome,
    IntelligenceReplayInspection, MemoryCapabilityCatalog, ParsedLongTermMemoryExtraction,
    PostReplyMemoryMaintenanceOutcome, PromptMemoryContext, RuntimeSkillHit,
    RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteOutcome, RuntimeSkillWriteSource,
    WorkingRecallInspection,
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
    LongTermExtraction {
        extraction: ParsedLongTermMemoryExtraction,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryWriteReport {
    pub accepted: bool,
    pub changed: usize,
    pub operation: &'static str,
    pub reason: String,
    pub lifecycle_report: RuntimeLifecycleReport,
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
    pub lifecycle_report: RuntimeLifecycleReport,
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
pub struct MemoryInspectionRequest {
    pub query: String,
    pub system_max_len: usize,
    pub pressure: crate::PressureLevel,
    pub mode_input: RuntimeLifecycleModeInput,
}

pub struct MemoryInspectionReport {
    pub working: WorkingRecallInspection,
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
