//! Public SDK facade for Beetle Memory.

mod capability;
mod capability_snapshot;
mod ops;
mod runtime;

pub use bm_core::agent::{ActiveWorkKind, ActiveWorkRecord, ForegroundWorkStatus};
pub use bm_core::feature_gate::{ProfileId, RoleFeature, TargetFeature};
pub use bm_core::llm::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, Message, StopReason, ToolChoicePolicy,
    ToolSpec,
};
pub use bm_core::memory::{
    apply_long_term_memory_extraction, build_long_term_memory_extraction_input,
    inspect_archive_recall, inspect_continuity_capsule_recall, inspect_memory_hygiene,
    inspect_personality_governance, inspect_runtime_skill_recall, inspect_shared_factual_recall,
    inspect_task_recall, inspect_working_recall, load_prompt_memory_context,
    recall_long_term_memory_block, run_post_reply_memory_maintenance,
    search_archive_records_detailed, ContinuityCapsuleMaintenanceOutcome, ContinuitySnapshot,
    ContinuitySnapshotImportMode, ContinuitySnapshotImportOutcome, ContinuitySnapshotMode,
    IngressKind, IntelligenceReplayInspection, LongTermMemoryDraft, LongTermMemoryEntry,
    LongTermMemoryQuery, MemoryProfile, MemorySystemKind as MemoryRuntimeSystemKind,
    ParsedLongTermMemoryExtraction, PostReplyMemoryMaintenanceContext,
    PostReplyMemoryMaintenanceInput, PostReplyMemoryMaintenanceOutcome, PromptMemoryContext,
    PromptMemoryContextParams, PromptParticipationPlan, PromptRecallIntent, RecallCandidate,
    RecallPlane, RecallQuery, RecallSelectionReport, WorkingRecallInspection,
};
pub use bm_core::orchestrator::PressureLevel;
pub use bm_core::platform::build_memory_operator_surface as build_operator_surface;
pub use bm_core::platform::{MemoryOperatorSurfaceSummary, ResponseBody};
pub use bm_core::runtime::{
    ensure_platform_soul_kernel_recovery, inspect_platform_soul_kernel, RuntimeLifecycleAdmission,
    RuntimeLifecycleDiagnosisReport, RuntimeLifecycleDisposition, RuntimeLifecycleModeInput,
    RuntimeLifecycleOperation, RuntimeLifecycleReport, RuntimeLifecycleTrigger,
    RuntimeModeSnapshot, SoulKernelRecoveryReport, SoulKernelStatus,
};
pub use bm_core::skills::{
    CapabilityAtomImportOutcome, CapabilityAtomSyncOutcome, RuntimeSkillGovernanceOutcome,
    RuntimeSkillHit, RuntimeSkillOrigin, RuntimeSkillReuseOutcome, RuntimeSkillWrite,
    RuntimeSkillWriteOutcome, RuntimeSkillWriteSource,
};
pub use bm_core::task::{TaskItem, TaskPriority, TaskQuery, TaskStatus};
pub use bm_core::{Error, Result};
pub use bm_store::{
    profile_memory_system_kind, StoreBackendConfig, StoreBackendKind, StoreCapacityBudget,
    StoreOpenReport, StorePlatform, StoreRepairPolicy, StoreRepairReport,
};
pub use capability::{
    resolve_memory_capabilities, AdapterTransportVisibility, MemoryAdapterCapabilityCatalog,
    MemoryAdapterCapabilityPolicy, MemoryCapabilityCatalog, MemoryCapabilityPolicy,
    MemoryEntryRuntimeCapabilityCatalog, MemoryIndexedRecallVisibility, MemoryOperationVisibility,
    MemoryPrivacyPolicy, MemoryRuntimeLifecycleCapability, MemoryValidationCapability,
};
pub use capability_snapshot::{
    platform_capability_snapshot, platform_capability_snapshot_file_name,
    platform_profile_feature_id, PlatformAdapterSnapshot, PlatformAdapterTransportSnapshot,
    PlatformCapabilitySnapshot, PlatformCompiledFeatureSnapshot, PlatformEntryRuntimeSnapshot,
    PlatformIndexedRecallSnapshot, PlatformLifecycleSnapshot, PlatformMemoryOperationSnapshot,
    PlatformValidationSnapshot, PLATFORM_CAPABILITY_SNAPSHOT_SCHEMA,
};
pub use ops::{
    MemoryCloseReport, MemoryCloseRequest, MemoryExportReport, MemoryExportRequest,
    MemoryImportReport, MemoryImportRequest, MemoryInspectionReport, MemoryInspectionRequest,
    MemoryMaintenanceReport, MemoryMaintenanceRequest, MemoryProceduralWriteReport,
    MemoryProjectionReport, MemoryProjectionRequest, MemoryRecallReport, MemoryRecallRequest,
    MemoryRecoverReport, MemoryRecoverRequest, MemoryReplayReport, MemoryReplayRequest,
    MemorySkillDeleteRequest, MemorySkillDetailReport, MemorySkillDetailRequest, MemorySkillKind,
    MemorySkillListReport, MemorySkillListRequest, MemorySkillMutationReport, MemorySkillOrigin,
    MemorySkillSetEnabledRequest, MemorySkillSummary, MemorySkillUpsertRequest, MemoryWriteReport,
    MemoryWriteRequest, RuntimeOperatorAction, RuntimeOperatorActionReport,
};
pub use runtime::{
    MemoryAuditEvent, MemoryAuditSink, MemoryClock, MemoryIdentity, MemoryRuntime,
    MemoryRuntimeBuilder, MemoryRuntimeConfig, MemoryScope, NoopMemoryAuditSink, SystemMemoryClock,
};

use bm_core::platform::Platform as _;

pub fn write_procedural_memory(
    platform: &StorePlatform,
    writes: &[RuntimeSkillWrite],
    source: RuntimeSkillWriteSource,
) -> Result<RuntimeSkillWriteOutcome> {
    let storage = platform.skill_storage();
    bm_core::skills::write_governed_runtime_skills(storage.as_ref(), writes, source)
}

pub fn recall_procedural_memory(
    platform: &StorePlatform,
    query: &str,
    source_chat_id: Option<&str>,
    now_secs: u64,
    limit: usize,
) -> Vec<RuntimeSkillHit> {
    let storage = platform.skill_storage();
    bm_core::skills::retrieve_runtime_skill_hits(
        storage.as_ref(),
        query,
        source_chat_id,
        now_secs,
        limit,
    )
}

pub fn govern_procedural_memory(
    platform: &StorePlatform,
    now_secs: u64,
) -> Result<RuntimeSkillGovernanceOutcome> {
    let storage = platform.skill_storage();
    bm_core::skills::govern_runtime_skills(storage.as_ref(), now_secs)
}
