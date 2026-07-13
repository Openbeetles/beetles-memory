//! Public SDK facade for Beetle Memory.
//!
//! Raw persistence capabilities are deliberately absent from the host API.
//!
//! ```compile_fail
//! use bm_store::StoreMutationBatch;
//!
//! fn main() {
//!     let _: Option<StoreMutationBatch> = None;
//! }
//! ```
//!
//! ```compile_fail
//! use bm_sdk::{Platform, StorePlatform, StoreSnapshot};
//!
//! fn main() {
//!     let _: Option<(StorePlatform, StoreSnapshot)> = None;
//! }
//! ```
//!
//! ```compile_fail
//! use bm_sdk::MemorySpaceMigrateApplyRequest;
//!
//! fn main() {
//!     let MemorySpaceMigrateApplyRequest { snapshot, .. } = todo!();
//!     let _ = snapshot;
//! }
//! ```

#[cfg(all(
    feature = "nonproduction-replay-harness",
    any(
        feature = "profile-esp-standalone-memory",
        feature = "profile-esp-embedded-sdk",
        feature = "profile-linux-device-standalone-memory",
        feature = "profile-desktop-macos-standalone-memory",
        feature = "profile-desktop-macos-embedded-sdk",
        feature = "profile-desktop-windows-embedded-sdk",
        feature = "profile-server-linux-memory-gateway"
    )
))]
compile_error!("nonproduction-replay-harness cannot be combined with a production SDK profile");

mod capability;
mod capability_snapshot;
mod ops;
mod runtime;
mod store;
mod store_internal;

pub(crate) use store_internal::*;

use std::collections::BTreeMap;

use bm_core::memory::MEMORY_FACET_INDEX_NAMESPACE;
use bm_core::platform::Platform as _;
use store_internal::{StorePlatform, StoreSnapshot};

pub use bm_core::agent::{ActiveWorkKind, ActiveWorkRecord, ForegroundWorkStatus};
pub use bm_core::budget::{
    compile_runtime_budget, AdapterRuntimeBudget, FacetRecallRuntimeBudget,
    GraphExpansionRuntimeBudget, LlmGatewayBudget, MaintenanceBudget, MemoryCoreBudget,
    ProjectionRenderBudget, ProjectionSourceBudget, ProviderModelContextLimit,
    RecallDeliveryRuntimeBudget, RuntimeBudgetInput, RuntimeBudgetReport, RuntimeDeploymentRole,
    RuntimeJobBudget, StaticPlatformManifest, StoreRuntimeBudget, TranscriptGovernanceBudget,
};
pub use bm_core::feature_gate::{ProfileId, RoleFeature, TargetFeature};
pub use bm_core::llm::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, Message, StopReason, ToolChoicePolicy,
    ToolSpec,
};
pub use bm_core::memory::{
    board_subject_scope_id, default_agent_subject_id, default_memory_space_id,
    primary_human_subject_id, private_garden_scope_id, system_governor_subject_id,
    ActorAttribution, CanonicalTurnDelta, CommittedSessionMessage, ConversationKey,
    ConversationScope, DeferredGovernanceJob, DeferredGovernanceJobStatus,
    DeferredGovernanceJobSummary, DeferredGovernanceQueueReport, DerivedMemoryPlane,
    DerivedMemoryRef, GovernedWriteDecision, HostOpaqueRef, HostRefRelation, HostRefVisibility,
    MemoryCandidateContent, MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment,
    MemoryCandidateTarget, MemoryEvidenceAuthority, MemoryGovernancePolicyMutation,
    MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration, MemoryLongTermControlView,
    MemoryLongTermGovernancePolicy, MemoryLongTermMutation, MemoryLongTermSelector,
    MemoryLongTermTarget, MemoryPlaneGovernanceReport, MemoryPrivacyClass,
    MemorySemanticJudgmentSource, MemorySubjectVisibilityPolicy, MemoryTurnDeliveryStatus,
    MemoryTurnProtocol, MemoryTurnSource, MemoryWriteAuthority, MemoryWriteCandidate,
    MemoryWriteDomain, PostTurnPrivateGardenReport, PostTurnSemanticGovernanceReport,
    PrivateDocEntry, PrivateDocWorkspace, PrivateGardenAdmissionDecision,
    PrivateGardenGovernanceManifestAction, PrivateGardenGovernanceManifestEntry,
    RedactedTranscriptSlice, SessionTurnCommitReport, SharedFactWriteGovernanceContext,
    SharedMemoryWriteOutcome, SoulCandidateDisposition, SoulCandidateHandoffReport,
    SubjectDescriptor, SubjectKind, SubjectLifecycleState, SubjectRegistry,
    SubjectRelationshipEdge, SubjectRelationshipGraph, SubjectRelationshipKind,
    SubjectScopedRuntime, SubjectSoulBinding, SubjectSoulSurface, SubjectVisibility,
    TranscriptAttrEnvelope, TranscriptAttrGovernance, TranscriptAttrLink,
    TranscriptAttrRedactionPolicy, TranscriptAttrScope, TranscriptAttrSource,
    TranscriptAttrSourceKind, TranscriptAttrTarget, TranscriptAttrValueKind,
    TranscriptAttrWriteRejection, TranscriptAttrWriteReport, TranscriptCommitReport,
    TranscriptConversationAlias, TranscriptEvidenceRef, TranscriptInputMessage,
    TranscriptLifecycleState, TranscriptLifecycleTransition, TranscriptRedactionReason,
    TranscriptRedactionReportItem, TranscriptRedactionState, TranscriptRepairIssue,
    TranscriptRepairIssueKind, TranscriptRepairReport, TranscriptReplayAudit, TranscriptReplayView,
    TranscriptTurnPage, TranscriptTurnRecord,
};
pub use bm_core::memory::{
    build_core_revision_diff_from_record, build_memory_graph_persistence_plan,
    build_relationship_boundary_audit_from_constitution_audit, build_soul_compact_digest,
    build_soul_feedback_report_from_turn_ledger,
    build_soul_growth_proposal_from_core_revision_record,
    build_soul_growth_proposals_from_core_revision_ledger, build_soul_kernel2_gate_report,
    build_soul_regression_suite_report, build_temporal_memory_graph_from_evidence,
    build_vault_migration_preflight, compile_edge_memory_budget_report,
    plan_memory_autopilot_for_profile, promote_task_experience_to_procedure,
    rerank_recall_with_temporal_graph, rerank_recall_with_temporal_graph_and_facets,
    CompactMemoryGraph, DroppedProjectionCandidate, EvidenceBacklink, FacetCoverageSelectionReport,
    FacetRankFusionCandidateReport, FacetRankFusionReport, GraphFacetPropagationContext,
    GraphRecallCandidateScore, GraphRecallExpansionBudget, GraphRecallExpansionBudgetReport,
    GraphRecallRerankReport, MemoryAutopilotInput, MemoryGraphBacklinkMembership,
    MemoryGraphDependencyRef, MemoryGraphEdge, MemoryGraphEdgeKind, MemoryGraphEdgeMembership,
    MemoryGraphEvidence, MemoryGraphNode, MemoryGraphNodeKind, MemoryGraphNodeMembership,
    MemoryGraphOwnerBinding, MemoryGraphPersistencePlan, MemoryGraphReadChainValidation,
    MemoryGraphRecallIndexDoc, MemoryGraphRevisionDoc, MemoryGraphScopeManifest,
    PrivateDisclosureIntegrityGuard, PrivateMaterialRedactionReport,
    ProceduralMemoryPromotionInput, ProceduralMemoryPromotionPolicy,
    ProceduralMemoryPromotionReport, ProjectionBudgetDecision, ProjectionFaithfulnessCheck,
    ProjectionPrivacyDecision, QueryFacetInput, RelationshipBoundaryAudit, SkillEvolutionReport,
    SoulCompactDigest, SoulFeedbackReport, SoulGrowthDecision, SoulGrowthProposal,
    SoulKernel2GateReport, SoulRegressionSuite, SubjectProjectionBoundaryProtocolReport,
    SubjectProjectionMountReport, SubjectProjectionReport, SubjectProjectionWorkIntegrityReport,
    TemporalMemoryGraphBuildReport, TemporalMemoryGraphGateReport, TemporalValidity, VaultManifest,
    VaultMigrationPreflight, WorkbenchApiMap, WorkbenchSurface, MEMORY_GRAPH_SCHEMA_VERSION,
};
pub use bm_core::memory::{
    build_long_term_memory_extraction_input, inspect_archive_recall,
    inspect_continuity_capsule_recall, inspect_memory_hygiene, inspect_personality_governance,
    inspect_runtime_skill_recall, inspect_shared_factual_recall, inspect_task_recall,
    inspect_working_recall, load_prompt_memory_context, recall_long_term_memory_block,
    run_post_reply_memory_maintenance, search_archive_records_detailed,
    ContinuityCapsuleMaintenanceOutcome, ContinuitySnapshot, ContinuitySnapshotImportMode,
    ContinuitySnapshotImportOutcome, ContinuitySnapshotMode, IngressKind,
    IntelligenceReplayInspection, LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryKind,
    LongTermMemoryQuery, LongTermMemorySourceScope, MemoryHygieneInspection, MemoryProfile,
    MemorySystemKind as MemoryRuntimeSystemKind, ParsedLongTermMemoryExtraction,
    PostReplyMemoryMaintenanceContext, PostReplyMemoryMaintenanceInput,
    PostReplyMemoryMaintenanceOutcome, ProjectionSourceAuthority, PromptMemoryContext,
    PromptMemoryContextParams, PromptParticipationPlan, PromptProjectionSource,
    PromptProjectionSurfaceRole, PromptRecallIntent, RecallCandidate, RecallPlane, RecallQuery,
    RecallSelectionReport, WorkingRecallInspection,
};
pub use bm_core::memory::{FacetReportView, MemoryFacetOwnerPlane, MemoryLongTermAffectedFacetDoc};
pub use bm_core::orchestrator::PressureLevel;
pub use bm_core::platform::build_memory_operator_surface as build_operator_surface;
pub use bm_core::platform::{MemoryOperatorSurfaceSummary, ResponseBody};
pub use bm_core::resource::{
    probe_host_runtime_resource, HostRuntimeResourceProbe, RuntimeResourceProbe,
    RuntimeResourceProbeSource, RuntimeResourceSnapshot, RuntimeResourceSnapshotCache,
    RuntimeResourceUnavailableReason, StaticRuntimeResourceProbe, UnavailableRuntimeResourceProbe,
};
pub use bm_core::runtime::{
    RuntimeForegroundSource, RuntimeLifecycleAdmission, RuntimeLifecycleDiagnosisReport,
    RuntimeLifecycleDisposition, RuntimeLifecycleModeInput, RuntimeLifecycleOperation,
    RuntimeLifecycleReport, RuntimeLifecycleTrigger, RuntimeModeSnapshot, RuntimeObservation,
    SoulKernelRecoveryReport, SoulKernelStatus,
};
pub use bm_core::skills::{
    fingerprint_agent_tool_registry, AgentSkillAccess, AgentSkillDirConfig,
    AgentSkillDirectoryReport, AgentSkillDirectoryWarning, AgentSkillMountReport,
    AgentSkillPackageRecord, AgentSkillPackageStatus, AgentSkillPackageWarning,
    AgentSkillProjectionAudit, AgentSkillProjectionRejection, AgentSkillProjectionSource,
    AgentSkillRecallHit, AgentSkillRefreshPolicy, AgentSkillRegistrySnapshot,
    AgentSkillResourceSummary, AgentSkillScope, AgentSkillTrust, AgentToolDescriptor,
    AgentToolExperienceConfidence, AgentToolExperienceGovernanceDecision,
    AgentToolExperienceGovernanceReport, AgentToolExperienceRecord, AgentToolExperienceStatus,
    AgentToolExperienceStatusReport, AgentToolHint, AgentToolObservationDigest, AgentToolOutcome,
    AgentToolProjectionAudit, AgentToolProjectionRejection, AgentToolRegistryOwner,
    AgentToolRegistryRef, AgentToolRegistryReport, AgentToolRegistryScope,
    AgentToolRegistrySnapshot, AgentToolSelectionReport, AgentToolUsageFeedback,
    CapabilityAtomImportOutcome, CapabilityAtomSyncOutcome, ProjectedAgentSkillHint,
    RuntimeSkillGovernanceOutcome, RuntimeSkillHit, RuntimeSkillOrigin, RuntimeSkillReuseOutcome,
    RuntimeSkillWrite, RuntimeSkillWriteOutcome, RuntimeSkillWriteSource,
    AGENT_TOOL_NO_EXPERIENCE_REASON, AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH,
    AGENT_TOOL_REGISTRY_FORBIDDEN_BY_PROFILE,
};
pub use bm_core::task::{TaskItem, TaskPriority, TaskQuery, TaskStatus};
pub use bm_core::{Error, Result};
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
    LLMRuntimeProjectionEnvelope, MemoryCloseReport, MemoryCloseRequest,
    MemoryDeferredGovernanceRunReport, MemoryDeferredGovernanceRunRequest,
    MemoryEvalRecallAblationReport, MemoryEvalRecallAblationSlice, MemoryEvalRecallAtK,
    MemoryEvalRecallBenchmarkContext, MemoryEvalRecallCandidate,
    MemoryEvalRecallCandidateEvidenceBinding, MemoryEvalRecallEvidenceGroupCoverage,
    MemoryEvalRecallEvidenceRefIndexEntry, MemoryEvalRecallFacetStageDiagnostics,
    MemoryEvalRecallGoldRank, MemoryEvalRecallGraphDistanceToGold, MemoryEvalRecallLossEntry,
    MemoryEvalRecallLossLedger, MemoryEvalRecallMetrics, MemoryEvalRecallPrivacyReport,
    MemoryEvalRecallReport, MemoryEvalRecallRequest, MemoryEvalRecallStageDiagnostics,
    MemoryEvalRecallStageEvidenceRefs, MemoryEvidenceRefView, MemoryEvidenceRefVisibility,
    MemoryExportReport, MemoryExportRequest, MemoryFacetRecallIndexReport,
    MemoryGovernancePolicyMutationReport, MemoryGraphIntegrityMaintenanceReport,
    MemoryGraphIntegrityMaintenanceRequest, MemoryGraphRecallIndexReport, MemoryImportReport,
    MemoryImportRequest, MemoryInspectionReport, MemoryInspectionRequest,
    MemoryLongTermDetailReport, MemoryLongTermDetailRequest, MemoryLongTermListReport,
    MemoryLongTermListRequest, MemoryLongTermMutationReport, MemoryLongTermMutationRequest,
    MemoryLongTermPolicyRequest, MemoryMaintenanceReport, MemoryMaintenanceRequest,
    MemoryProceduralWriteReport, MemoryProjectionAuditReport,
    MemoryProjectionDeliveryDigestContentEntry, MemoryProjectionDeliveryDigestEntry,
    MemoryProjectionDeliveryDigestManifest, MemoryProjectionPrivateGateAudit,
    MemoryProjectionReport, MemoryProjectionRequest, MemoryProjectionSectionAudit,
    MemoryProjectionSourceAudit, MemoryProjectionSurfaceSet, MemoryRecallDeliveryReport,
    MemoryRecallRenderDecision, MemoryRecallRenderDropReason, MemoryRecallReport,
    MemoryRecallRequest, MemoryRecallSelectionDecision, MemoryRecallSelectionDropReason,
    MemoryRecoverReport, MemoryRecoverRequest, MemoryRenderedEvidenceCapsule, MemoryReplayReport,
    MemoryReplayRequest, MemoryRetentionCompactionReport, MemoryRetentionCompactionRequest,
    MemorySpaceArchive, MemorySpaceExportReport, MemorySpaceExportRequest, MemorySpaceImportReport,
    MemorySpaceImportRequest, MemorySpaceMigrateApplyReport, MemorySpaceMigrateApplyRequest,
    MemorySpaceMigratePreviewReport, MemorySpaceMigratePreviewRequest,
    MemorySpaceMigrationManifest, MemorySpaceMigrationPlan, MemorySpaceMigrationPlaneReport,
    MemorySpaceMigrationPrivacyReport, MemorySpaceSubjectRemapReport,
    MemoryTranscriptAttrWriteReport, MemoryTranscriptAttrWriteRequest,
    MemoryTranscriptCommitReport, MemoryTranscriptCommitRequest, MemoryTranscriptExportReport,
    MemoryTranscriptExportRequest, MemoryTranscriptLifecycleReport,
    MemoryTranscriptLifecycleRequest, MemoryTranscriptRepairReport, MemoryTranscriptRepairRequest,
    MemoryTranscriptReplayReport, MemoryTranscriptReplayRequest, MemoryTurnFinalizeReport,
    MemoryTurnFinalizeRequest, MemoryWriteReport, MemoryWriteRequest, MemoryWriteTransactionReport,
    PrivateDisclosureIntegrityReport, PrivateDisclosureSurfaceReport,
    RuntimeDisclosureProtocolReport, RuntimeOperatorAction, RuntimeOperatorActionReport,
    RuntimeProjectionSourceBlock, RuntimeSkillDeleteRequest, RuntimeSkillDetailReport,
    RuntimeSkillDetailRequest, RuntimeSkillEditRequest, RuntimeSkillListReport,
    RuntimeSkillListRequest, RuntimeSkillMutationReport, RuntimeSkillSetEnabledRequest,
    RuntimeSkillSummary, SoulLifeProjectionReport, TemporalMemoryGraphMutationReport,
    TemporalMemoryGraphWriteRequest, WorkIntegrityReport,
    MEMORY_PROJECTION_DELIVERY_DIGEST_SCHEMA_VERSION, MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
};
pub use runtime::{
    MemoryAuditEvent, MemoryAuditSink, MemoryClock, MemoryIdentity, MemoryRuntime,
    MemoryRuntimeBuilder, MemoryRuntimeConfig, MemoryScope, NoopMemoryAuditSink, SystemMemoryClock,
};
pub use store::{
    profile_memory_system_kind, MemoryStoreHandle, MemoryStoreTelemetryReport, StoreBackendConfig,
    StoreBackendKind, StoreCapacityBudget, StoreOpenReport, StorePathBudget, StoreRepairPolicy,
    StoreRepairReport,
};

#[cfg(feature = "nonproduction-replay-harness")]
pub mod nonproduction_replay_harness {
    pub use crate::store::ReplayStoreHarness;
    #[cfg(feature = "sqlite-store")]
    pub use crate::store_internal::SqliteStoreEngine;
    pub use crate::store_internal::{
        EmbeddedStoreEngine, FileStoreEngine, GovernedRecallSnapshot, InMemoryStoreEngine,
        MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig, StoreBackendKind,
        StoreBlobAddress, StoreCapacityBudget, StoreConsistentBlobRead, StoreConsistentJsonRead,
        StoreConsistentNamespaceReadRequest, StoreConsistentNamespaceReadResult,
        StoreConsistentReadRequest, StoreConsistentReadResult, StoreEngine, StoreEngineMutation,
        StoreEventLog, StoreEventScope, StoreJsonAddress, StoreJsonPrecondition, StoreMutation,
        StoreMutationBatch, StoreMutationBatchReport, StoreMutationBudgetReport, StoreOpenReport,
        StorePathBudget, StorePlatform, StoreReadReceipt, StoreRepairPolicy, StoreRepairReport,
        StoreSchemaManifest, StoreSnapshot, StoreSnapshotBlob, StoreSnapshotExportReport,
        StoreSnapshotImportReport, StoreSnapshotJsonDoc, StoreSnapshotReplaceReport,
        StoreTransactionReport, StoreTransactionRequest, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION,
    };
}

pub fn recall_procedural_memory(
    handle: &MemoryStoreHandle,
    query: &str,
    source_chat_id: Option<&str>,
    now_secs: u64,
    limit: usize,
) -> Vec<RuntimeSkillHit> {
    let storage = handle.platform().skill_storage();
    bm_core::skills::retrieve_runtime_skill_hits(
        storage.as_ref(),
        query,
        source_chat_id,
        now_secs,
        limit,
    )
}

pub fn export_memory_space(
    handle: &MemoryStoreHandle,
    request: MemorySpaceExportRequest,
) -> Result<MemorySpaceExportReport> {
    export_memory_space_from_platform(handle.platform(), request)
}

pub(crate) fn export_memory_space_from_platform(
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
        archive: MemorySpaceArchive::from_snapshot(snapshot),
        export_report,
        privacy_redactions,
    })
}

pub fn import_memory_space(
    handle: &MemoryStoreHandle,
    request: MemorySpaceImportRequest,
) -> Result<MemorySpaceImportReport> {
    import_memory_space_from_platform(handle.platform(), request)
}

pub(crate) fn import_memory_space_from_platform(
    platform: &StorePlatform,
    request: MemorySpaceImportRequest,
) -> Result<MemorySpaceImportReport> {
    ensure_memory_space_import_has_no_unremapped_facet_index(request.archive.snapshot())?;
    let import_report = platform.import_store_snapshot_with_report(request.archive.snapshot())?;
    Ok(MemorySpaceImportReport {
        memory_space_id: request.memory_space_id,
        import_report,
    })
}

pub fn preview_memory_space_migration(
    request: MemorySpaceMigratePreviewRequest,
) -> MemorySpaceMigratePreviewReport {
    let snapshot = request.archive.snapshot();
    let privacy_redactions = count_private_snapshot_entries(snapshot);
    let loss_risk = snapshot.schema_id != crate::store_internal::STORE_SCHEMA_ID;
    let report = snapshot.export_report();
    let vault_manifest = VaultManifest {
        identity_id: request.source_memory_space_id.clone(),
        profile: request.source_profile,
        store_backend: "store_snapshot".to_string(),
        snapshot_fingerprint: report.state_fingerprint.clone(),
        event_fingerprint: report.event_fingerprint.clone(),
        privacy_policy_fingerprint: privacy_policy_fingerprint(privacy_redactions, loss_risk),
    };
    let vault_redaction = build_vault_redaction_report(snapshot);
    let mut vault_preflight = build_vault_migration_preflight(
        vault_manifest.clone(),
        request.target_profile,
        vault_redaction.clone(),
        &snapshot.schema_id,
        crate::store_internal::STORE_SCHEMA_ID,
    );
    if snapshot_requires_facet_index_remap(snapshot) {
        vault_preflight.passed = false;
    }
    let manifest = build_memory_space_migration_manifest(
        &request.source_memory_space_id,
        &request.target_memory_space_id,
        snapshot,
        loss_risk,
    );
    let plan = MemorySpaceMigrationPlan {
        target_memory_space_id: request.target_memory_space_id.clone(),
        snapshot: request.archive.into_snapshot(),
        preflight: vault_preflight.clone(),
    };
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
        manifest,
        vault_manifest,
        vault_redaction,
        vault_preflight,
        plan,
    }
}

pub fn apply_memory_space_migration(
    handle: &MemoryStoreHandle,
    request: MemorySpaceMigrateApplyRequest,
) -> Result<MemorySpaceMigrateApplyReport> {
    apply_memory_space_migration_from_platform(handle.platform(), request)
}

pub(crate) fn apply_memory_space_migration_from_platform(
    platform: &StorePlatform,
    request: MemorySpaceMigrateApplyRequest,
) -> Result<MemorySpaceMigrateApplyReport> {
    let plan = request.plan;
    if !plan.preflight.passed {
        return Err(Error::config(
            "memory_space_migration",
            "vault migration preflight failed",
        ));
    }
    let expected_preflight = vault_preflight_for_snapshot(
        &plan.snapshot,
        plan.preflight.source_profile,
        plan.preflight.target_profile,
    );
    if plan.preflight != expected_preflight {
        return Err(Error::config(
            "memory_space_migration",
            "vault migration preflight does not match snapshot",
        ));
    }
    ensure_memory_space_import_has_no_unremapped_facet_index(&plan.snapshot)?;
    let import_report = platform.import_store_snapshot_with_report(&plan.snapshot)?;
    Ok(MemorySpaceMigrateApplyReport {
        target_memory_space_id: plan.target_memory_space_id,
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

fn snapshot_requires_facet_index_remap(snapshot: &StoreSnapshot) -> bool {
    snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == MEMORY_FACET_INDEX_NAMESPACE)
        || snapshot
            .events
            .iter()
            .any(|event| event.plane == MEMORY_FACET_INDEX_NAMESPACE)
}

fn ensure_memory_space_import_has_no_unremapped_facet_index(
    snapshot: &StoreSnapshot,
) -> Result<()> {
    if snapshot_requires_facet_index_remap(snapshot) {
        return Err(Error::config(
            "memory_space_import",
            "facet_index_remap_required",
        ));
    }
    Ok(())
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

fn build_vault_redaction_report(snapshot: &StoreSnapshot) -> PrivateMaterialRedactionReport {
    let mut checked_refs = Vec::new();
    let mut redacted_refs = Vec::new();
    for doc in &snapshot.json_docs {
        let record_ref = format!("json:{}:{}", doc.namespace, doc.key);
        checked_refs.push(record_ref.clone());
        if is_private_snapshot_namespace(&doc.namespace) {
            redacted_refs.push(record_ref);
        }
    }
    for blob in &snapshot.blobs {
        let record_ref = format!("blob:{}:{}", blob.namespace, blob.key);
        checked_refs.push(record_ref.clone());
        if is_private_snapshot_namespace(&blob.namespace) {
            redacted_refs.push(record_ref);
        }
    }
    for event in &snapshot.events {
        let record_ref = format!("event:{}:{}", event.plane, event.record_key);
        checked_refs.push(record_ref.clone());
        if is_private_snapshot_namespace(&event.plane)
            || is_private_snapshot_key(event.record_key.as_str())
        {
            redacted_refs.push(record_ref);
        }
    }
    PrivateMaterialRedactionReport {
        surface: "memory_space_migration_preview".to_string(),
        checked_refs,
        redacted_refs,
        raw_private_leak_count: 0,
    }
}

fn vault_preflight_for_snapshot(
    snapshot: &StoreSnapshot,
    source_profile: ProfileId,
    target_profile: ProfileId,
) -> VaultMigrationPreflight {
    let report = snapshot.export_report();
    let privacy_redactions = count_private_snapshot_entries(snapshot);
    let loss_risk = snapshot.schema_id != crate::store_internal::STORE_SCHEMA_ID;
    let mut preflight = build_vault_migration_preflight(
        VaultManifest {
            identity_id: "memory-space-preview".to_string(),
            profile: source_profile,
            store_backend: "store_snapshot".to_string(),
            snapshot_fingerprint: report.state_fingerprint,
            event_fingerprint: report.event_fingerprint,
            privacy_policy_fingerprint: privacy_policy_fingerprint(privacy_redactions, loss_risk),
        },
        target_profile,
        build_vault_redaction_report(snapshot),
        &snapshot.schema_id,
        crate::store_internal::STORE_SCHEMA_ID,
    );
    if snapshot_requires_facet_index_remap(snapshot) {
        preflight.passed = false;
    }
    preflight
}

fn privacy_policy_fingerprint(privacy_redactions: usize, loss_risk: bool) -> String {
    format!(
        "privacy-redactions:{privacy_redactions}:schema-loss-risk:{}",
        u8::from(loss_risk)
    )
}

fn build_memory_space_migration_manifest(
    source_memory_space_id: &str,
    target_memory_space_id: &str,
    snapshot: &StoreSnapshot,
    loss_risk: bool,
) -> MemorySpaceMigrationManifest {
    let mut plane_counts = BTreeMap::<String, (usize, String)>::new();
    let mut privacy_counts = BTreeMap::<String, usize>::new();
    for doc in &snapshot.json_docs {
        accumulate_migration_record(&mut plane_counts, &mut privacy_counts, &doc.namespace);
    }
    for blob in &snapshot.blobs {
        accumulate_migration_record(&mut plane_counts, &mut privacy_counts, &blob.namespace);
    }
    for event in &snapshot.events {
        accumulate_migration_record(&mut plane_counts, &mut privacy_counts, &event.plane);
    }
    let planes = plane_counts
        .into_iter()
        .map(
            |(plane, (records, privacy_class))| MemorySpaceMigrationPlaneReport {
                plane,
                records,
                privacy_class,
            },
        )
        .collect::<Vec<_>>();
    let privacy = privacy_counts
        .into_iter()
        .map(
            |(privacy_class, records)| MemorySpaceMigrationPrivacyReport {
                privacy_class,
                records,
            },
        )
        .collect::<Vec<_>>();
    let remap_required = source_memory_space_id != target_memory_space_id;
    MemorySpaceMigrationManifest {
        source_memory_space_id: source_memory_space_id.to_string(),
        target_memory_space_id: target_memory_space_id.to_string(),
        schema_id: snapshot.schema_id.clone(),
        whole_space_snapshot: true,
        subject_remap: MemorySpaceSubjectRemapReport {
            required: remap_required,
            applied: false,
            reason: if remap_required {
                "whole_space_snapshot_does_not_rewrite_subject_scope"
            } else {
                "source_and_target_space_match"
            }
            .to_string(),
        },
        planes,
        privacy,
        conflict_risk: loss_risk,
    }
}

fn accumulate_migration_record(
    plane_counts: &mut BTreeMap<String, (usize, String)>,
    privacy_counts: &mut BTreeMap<String, usize>,
    plane: &str,
) {
    let privacy_class = if is_private_snapshot_namespace(plane) {
        "private"
    } else {
        "shared"
    };
    let entry = plane_counts
        .entry(plane.to_string())
        .or_insert_with(|| (0, privacy_class.to_string()));
    entry.0 = entry.0.saturating_add(1);
    *privacy_counts.entry(privacy_class.to_string()).or_insert(0) += 1;
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
            | "conversation_transcript"
            | "conversation_transcript_alias"
            | "conversation_transcript_attr"
            | "conversation_transcript_derived_ref"
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
        || key.contains("conversation_transcript")
        || key.contains("conversation_transcript_alias")
        || key.contains("conversation_transcript_derived_ref")
}
