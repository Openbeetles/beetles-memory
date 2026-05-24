//! Public SDK facade for Beetle Memory.

mod capability;
mod capability_snapshot;
mod ops;
mod runtime;

pub use bm_core::agent::{ActiveWorkKind, ActiveWorkRecord, ForegroundWorkStatus};
pub use bm_core::budget::{
    compile_runtime_budget, AdapterRuntimeBudget, LlmGatewayBudget, MaintenanceBudget,
    MemoryCoreBudget, ProjectionRenderBudget, ProjectionSourceBudget, ProviderModelContextLimit,
    RuntimeBudgetInput, RuntimeBudgetReport, RuntimeDeploymentRole, RuntimeJobBudget,
    StaticPlatformManifest, StoreRuntimeBudget,
};
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
    LongTermMemoryKind, LongTermMemoryQuery, MemoryHygieneInspection, MemoryProfile,
    MemorySystemKind as MemoryRuntimeSystemKind, ParsedLongTermMemoryExtraction,
    PostReplyMemoryMaintenanceContext, PostReplyMemoryMaintenanceInput,
    PostReplyMemoryMaintenanceOutcome, PromptMemoryContext, PromptMemoryContextParams,
    PromptParticipationPlan, PromptRecallIntent, RecallCandidate, RecallPlane, RecallQuery,
    RecallSelectionReport, WorkingRecallInspection,
};
pub use bm_core::memory::{
    CommittedSessionMessage, ConversationScope, DeferredGovernanceJob, DeferredGovernanceJobStatus,
    GovernedWriteDecision, MemoryCandidateContent, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPlaneGovernanceReport, MemoryPrivacyClass, MemoryTurnDeliveryStatus, MemoryTurnProtocol,
    MemoryTurnSource, MemoryWriteAuthority, MemoryWriteCandidate, MemoryWriteDomain,
    PostTurnPrivateGardenReport, PostTurnSemanticGovernanceReport, PrivateGardenAdmissionDecision,
    SessionTurnCommitReport, SoulCandidateDisposition, SoulCandidateHandoffReport,
    TranscriptInputMessage,
};
pub use bm_core::orchestrator::PressureLevel;
pub use bm_core::platform::build_memory_operator_surface as build_operator_surface;
pub use bm_core::platform::{MemoryOperatorSurfaceSummary, Platform, ResponseBody};
pub use bm_core::resource::{
    probe_host_runtime_resource, HostRuntimeResourceProbe, RuntimeResourceProbe,
    RuntimeResourceProbeSource, RuntimeResourceSnapshot, RuntimeResourceSnapshotCache,
    RuntimeResourceUnavailableReason, StaticRuntimeResourceProbe, UnavailableRuntimeResourceProbe,
};
pub use bm_core::runtime::{
    ensure_platform_soul_kernel_recovery, inspect_platform_soul_kernel, RuntimeForegroundSource,
    RuntimeLifecycleAdmission, RuntimeLifecycleDiagnosisReport, RuntimeLifecycleDisposition,
    RuntimeLifecycleModeInput, RuntimeLifecycleOperation, RuntimeLifecycleReport,
    RuntimeLifecycleTrigger, RuntimeModeSnapshot, RuntimeObservation, SoulKernelRecoveryReport,
    SoulKernelStatus,
};
pub use bm_core::skills::{
    CapabilityAtomImportOutcome, CapabilityAtomSyncOutcome, RuntimeSkillGovernanceOutcome,
    RuntimeSkillHit, RuntimeSkillOrigin, RuntimeSkillReuseOutcome, RuntimeSkillWrite,
    RuntimeSkillWriteOutcome, RuntimeSkillWriteSource,
};
pub use bm_core::task::{TaskItem, TaskPriority, TaskQuery, TaskStatus};
pub use bm_core::{Error, Result};
pub use bm_store::{
    profile_memory_system_kind, MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig,
    StoreBackendKind, StoreCapacityBudget, StoreEventLog, StoreEventScope, StoreOpenReport,
    StorePlatform, StoreRepairPolicy, StoreRepairReport, StoreSnapshot, StoreSnapshotExportReport,
    StoreSnapshotImportReport,
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
    MemorySkillSetEnabledRequest, MemorySkillSummary, MemorySkillUpsertRequest,
    MemorySpaceExportReport, MemorySpaceExportRequest, MemorySpaceImportReport,
    MemorySpaceImportRequest, MemorySpaceMigrateApplyReport, MemorySpaceMigrateApplyRequest,
    MemorySpaceMigratePreviewReport, MemorySpaceMigratePreviewRequest, MemoryTurnFinalizeReport,
    MemoryTurnFinalizeRequest, MemoryWriteReport, MemoryWriteRequest, RuntimeOperatorAction,
    RuntimeOperatorActionReport,
};
pub use runtime::{
    MemoryAuditEvent, MemoryAuditSink, MemoryClock, MemoryIdentity, MemoryRuntime,
    MemoryRuntimeBuilder, MemoryRuntimeConfig, MemoryScope, NoopMemoryAuditSink, SystemMemoryClock,
};

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

pub fn export_memory_space(
    platform: &StorePlatform,
    request: MemorySpaceExportRequest,
) -> Result<MemorySpaceExportReport> {
    let (mut snapshot, _raw_export_report) = platform.export_store_snapshot_with_report()?;
    let privacy_redactions = if request.include_private {
        0
    } else {
        redact_private_snapshot_entries(&mut snapshot)
    };
    let export_report = snapshot.export_report();
    Ok(MemorySpaceExportReport {
        memory_space_id: request.memory_space_id,
        snapshot,
        export_report,
        privacy_redactions,
    })
}

pub fn import_memory_space(
    platform: &StorePlatform,
    request: MemorySpaceImportRequest,
) -> Result<MemorySpaceImportReport> {
    let import_report = platform.import_store_snapshot_with_report(&request.snapshot)?;
    Ok(MemorySpaceImportReport {
        memory_space_id: request.memory_space_id,
        import_report,
    })
}

pub fn preview_memory_space_migration(
    request: MemorySpaceMigratePreviewRequest,
) -> MemorySpaceMigratePreviewReport {
    let privacy_redactions = count_private_snapshot_entries(&request.snapshot);
    let loss_risk = request.snapshot.schema_id != bm_store::STORE_SCHEMA_ID;
    let report = request.snapshot.export_report();
    MemorySpaceMigratePreviewReport {
        source_memory_space_id: request.source_memory_space_id,
        target_memory_space_id: request.target_memory_space_id,
        schema_id: report.schema_id,
        json_docs: report.json_docs,
        blobs: report.blobs,
        events: report.events,
        state_fingerprint: report.state_fingerprint,
        event_fingerprint: report.event_fingerprint,
        privacy_redactions,
        loss_risk,
    }
}

pub fn apply_memory_space_migration(
    platform: &StorePlatform,
    request: MemorySpaceMigrateApplyRequest,
) -> Result<MemorySpaceMigrateApplyReport> {
    let import_report = platform.import_store_snapshot_with_report(&request.snapshot)?;
    Ok(MemorySpaceMigrateApplyReport {
        target_memory_space_id: request.target_memory_space_id,
        import_report,
    })
}

fn redact_private_snapshot_entries(snapshot: &mut StoreSnapshot) -> usize {
    let redactions = count_private_snapshot_entries(snapshot);
    snapshot
        .json_docs
        .retain(|doc| !is_private_snapshot_namespace(&doc.namespace));
    snapshot
        .blobs
        .retain(|blob| !is_private_snapshot_namespace(&blob.namespace));
    snapshot.events.retain(|event| {
        !is_private_snapshot_namespace(&event.plane)
            && !is_private_snapshot_key(event.record_key.as_str())
    });
    redactions
}

fn count_private_snapshot_entries(snapshot: &StoreSnapshot) -> usize {
    snapshot
        .json_docs
        .iter()
        .filter(|doc| is_private_snapshot_namespace(&doc.namespace))
        .count()
        + snapshot
            .blobs
            .iter()
            .filter(|blob| is_private_snapshot_namespace(&blob.namespace))
            .count()
        + snapshot
            .events
            .iter()
            .filter(|event| {
                is_private_snapshot_namespace(&event.plane)
                    || is_private_snapshot_key(event.record_key.as_str())
            })
            .count()
}

fn is_private_snapshot_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        "self_model"
            | "self_authored_core"
            | "core_revision_ledger"
            | "self_continuity"
            | "relationship_constitution"
            | "relationship_portfolio"
            | "relationship_topology"
            | "outer_voice"
            | "inner_life"
            | "felt_significance"
            | "temperament_continuity"
            | "inner_conflict"
            | "mental_privacy"
            | "private_doc"
            | "private_garden"
    )
}

fn is_private_snapshot_key(key: &str) -> bool {
    key.contains("private_garden")
        || key.contains("private_doc")
        || key.contains("mental_privacy")
        || key.contains("inner_life")
        || key.contains("self_model")
        || key.contains("self_continuity")
}
