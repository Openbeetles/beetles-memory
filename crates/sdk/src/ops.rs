use bm_core::memory::IngressKind;

use crate::{
    ContinuitySnapshot, ContinuitySnapshotImportMode, ContinuitySnapshotImportOutcome,
    IntelligenceReplayInspection, MemoryCapabilityCatalog, ParsedLongTermMemoryExtraction,
    PostReplyMemoryMaintenanceOutcome, PromptMemoryContext, RuntimeSkillHit,
    RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteOutcome, RuntimeSkillWriteSource,
    WorkingRecallInspection,
};

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
}

#[derive(Clone, Debug)]
pub struct MemoryProjectionRequest {
    pub user_query: String,
    pub system_max_len: usize,
    pub recent_messages_limit: usize,
}

pub struct MemoryProjectionReport {
    pub system_memory_block: String,
    pub context: PromptMemoryContext,
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
}

pub struct MemoryMaintenanceReport {
    pub report: PostReplyMemoryMaintenanceOutcome,
    pub long_term_refresh_enqueued: bool,
}

#[derive(Clone, Debug)]
pub struct MemoryInspectionRequest {
    pub query: String,
    pub system_max_len: usize,
}

pub struct MemoryInspectionReport {
    pub working: WorkingRecallInspection,
    pub capabilities: MemoryCapabilityCatalog,
}

pub struct MemoryReplayRequest {
    pub chat_id: String,
    pub limit: usize,
}

#[derive(Clone, Debug)]
pub struct MemoryReplayReport {
    pub chat_id: String,
    pub inspection: IntelligenceReplayInspection,
}

#[derive(Clone, Debug)]
pub struct MemoryExportRequest {
    pub chat_id: String,
}

#[derive(Clone, Debug)]
pub struct MemoryExportReport {
    pub snapshot: ContinuitySnapshot,
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
}

#[derive(Clone, Debug)]
pub struct MemoryProceduralWriteReport {
    pub outcome: RuntimeSkillWriteOutcome,
}
