//! Public SDK facade for Beetle Memory.

pub use bm_core::agent::{ActiveWorkKind, ActiveWorkRecord, ActiveWorkStore, ForegroundWorkStatus};
pub use bm_core::llm::{LlmClient, LlmHttpClient};
pub use bm_core::memory::{
    apply_long_term_memory_extraction, build_long_term_memory_extraction_input,
    inspect_archive_recall, inspect_continuity_capsule_recall, inspect_memory_hygiene,
    inspect_personality_governance, inspect_runtime_skill_recall, inspect_shared_factual_recall,
    inspect_task_recall, inspect_working_recall, load_prompt_memory_context,
    recall_long_term_memory_block, run_post_reply_memory_maintenance,
    search_archive_records_detailed, AutonomyStrategyStore, ContinuityCapsuleMaintenanceOutcome,
    ContinuityCapsuleStore, CoreRevisionLedgerStore, ExecutionStateStore, FeltSignificanceStore,
    InnerConflictStore, InnerLifeStore, LongTermMemoryDraft, LongTermMemoryEntry,
    LongTermMemoryExtractionStateStore, LongTermMemoryQuery, LongTermMemoryStore, MemoryProfile,
    MemoryStore, MemorySystemKind as MemoryRuntimeSystemKind, MentalPrivacyStore, OuterVoiceStore,
    PostReplyMemoryMaintenanceContext, PostReplyMemoryMaintenanceInput,
    PostReplyMemoryMaintenanceOutcome, PrivateDocStore, PrivateGardenStore, PromptMemoryContext,
    PromptMemoryContextParams, PromptParticipationPlan, RecallCandidate, RecallPlane, RecallQuery,
    RecallSelectionReport, RelationshipConstitutionStore, RelationshipPortfolioStore,
    RelationshipTopologyStore, RemindAtStore, SelfAuthoredCoreStore, SelfContinuityStore,
    SelfModelStore, SessionStore, SessionSummaryStore, TemperamentContinuityStore,
    TurnContinuityEvidenceStore, TurnLedgerStore, WorkingRecallInspection, WorldSenseStore,
};
pub use bm_core::orchestrator::PressureLevel;
pub use bm_core::platform::{
    build_memory_operator_surface as build_operator_surface,
    MemorySystemKind as PlatformMemorySystemKind, Platform, SkillMetaStore, SkillStorage, StateFs,
};
pub use bm_core::runtime::{
    ensure_platform_soul_kernel_recovery, inspect_platform_soul_kernel, RuntimeModeSnapshot,
    SoulKernelRecoveryReport, SoulKernelStatus,
};
pub use bm_core::skills::{
    build_runtime_skill_recall_block, export_capability_atom_exchange_envelope,
    govern_runtime_skills, import_capability_atom_exchange_envelope, record_runtime_skill_outcomes,
    retrieve_runtime_skill_hits, sync_capability_atoms_from_runtime_skills,
    CapabilityAtomImportOutcome, CapabilityAtomSyncOutcome, RuntimeSkillGovernanceOutcome,
    RuntimeSkillHit, RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteOutcome,
    RuntimeSkillWriteSource,
};
pub use bm_core::task::{TaskItem, TaskPriority, TaskQuery, TaskStatus, TaskStore};
pub use bm_core::task_execution::{
    TaskArtifactStore, TaskExecutionLedgerStore, TaskLearningStore, TaskRunStore,
};
pub use bm_core::{Error, Result};

pub fn write_procedural_memory(
    storage: &dyn SkillStorage,
    writes: &[RuntimeSkillWrite],
    source: RuntimeSkillWriteSource,
) -> Result<RuntimeSkillWriteOutcome> {
    bm_core::skills::write_governed_runtime_skills(storage, writes, source)
}

pub fn recall_procedural_memory(
    storage: &dyn SkillStorage,
    query: &str,
    source_chat_id: Option<&str>,
    now_secs: u64,
    limit: usize,
) -> Vec<RuntimeSkillHit> {
    retrieve_runtime_skill_hits(storage, query, source_chat_id, now_secs, limit)
}

pub fn govern_procedural_memory(
    storage: &dyn SkillStorage,
    now_secs: u64,
) -> Result<RuntimeSkillGovernanceOutcome> {
    govern_runtime_skills(storage, now_secs)
}
