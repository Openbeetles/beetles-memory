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
//!
//! Production callers must use the runtime-owned memory-space operations.
//!
//! ```compile_fail
//! use bm_sdk::recall_procedural_memory;
//! fn main() {}
//! ```
//!
//! ```compile_fail
//! use bm_sdk::export_memory_space;
//! fn main() {}
//! ```
//!
//! ```compile_fail
//! use bm_sdk::import_memory_space;
//! fn main() {}
//! ```
//!
//! ```compile_fail
//! use bm_sdk::apply_memory_space_migration;
//! fn main() {}
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

use std::collections::{BTreeMap, BTreeSet};

use store_internal::{StorePlatform, StoreSnapshot};

pub use bm_core::agent::{ActiveWorkKind, ActiveWorkRecord, ForegroundWorkStatus};
#[cfg(feature = "nonproduction-replay-harness")]
pub use bm_core::budget::NonproductionRuntimeBudgetLimits;
pub use bm_core::budget::{
    AdapterRuntimeBudget, FacetRecallRuntimeBudget, GraphExpansionRuntimeBudget, LlmGatewayBudget,
    MaintenanceBudget, MemoryCoreBudget, ProjectionRenderBudget, ProjectionSourceBudget,
    ProviderModelContextLimit, RecallDeliveryRuntimeBudget, RuntimeBudgetReport, RuntimeJobBudget,
    StoreRuntimeBudget, TranscriptGovernanceBudget,
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
    ContinuityCapsuleMaintenanceOutcome, IngressKind, IntelligenceReplayInspection,
    LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryKind, LongTermMemoryQuery,
    LongTermMemorySlot, LongTermMemorySourceScope, LongTermMemorySourceType,
    MemoryHygieneInspection, MemoryProfile, MemorySystemKind as MemoryRuntimeSystemKind,
    ParsedLongTermMemoryExtraction, PostReplyMemoryMaintenanceContext,
    PostReplyMemoryMaintenanceInput, PostReplyMemoryMaintenanceOutcome, ProjectionSourceAuthority,
    PromptMemoryContext, PromptMemoryContextParams, PromptParticipationPlan,
    PromptProjectionSource, PromptProjectionSurfaceRole, PromptRecallIntent, RecallCandidate,
    RecallPlane, RecallQuery, RecallSelectionReport, WorkingRecallInspection,
};
pub use bm_core::memory::{
    governed_evidence_document_content_digest, governed_evidence_source_locator_digest,
    FacetReportView, GovernedEvidenceDocumentChunk, GovernedEvidenceDocumentDraft,
    GovernedEvidenceDocumentSourceKind, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    MemoryLongTermAffectedFacetDoc, GOVERNED_EVIDENCE_DOCUMENT_SCHEMA_VERSION,
};
pub use bm_core::orchestrator::PressureLevel;
pub use bm_core::platform::build_memory_operator_surface as build_operator_surface;
pub use bm_core::platform::{MemoryOperatorSurfaceSummary, ResponseBody};
pub use bm_core::resource::{
    RuntimeResourceObservation, RuntimeResourceProbe, RuntimeResourceProbeSource,
    RuntimeResourceSnapshot, RuntimeResourceUnavailableReason,
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
    MemoryEvalEvidenceApplicability, MemoryEvalQuestionEvaluation, MemoryEvalRecallAblationReport,
    MemoryEvalRecallAblationSlice, MemoryEvalRecallAtK, MemoryEvalRecallBenchmarkContext,
    MemoryEvalRecallCandidate, MemoryEvalRecallCandidateEvidenceBinding,
    MemoryEvalRecallCandidateRenderLoss, MemoryEvalRecallCandidateSelectionLoss,
    MemoryEvalRecallEvidenceGroupCoverage, MemoryEvalRecallEvidenceRefIndexEntry,
    MemoryEvalRecallFacetStageDiagnostics, MemoryEvalRecallGoldRank,
    MemoryEvalRecallGraphDistanceToGold, MemoryEvalRecallLossEntry, MemoryEvalRecallLossLedger,
    MemoryEvalRecallMetrics, MemoryEvalRecallPrivacyReport, MemoryEvalRecallReport,
    MemoryEvalRecallRequest, MemoryEvalRecallStageCandidateMatch, MemoryEvalRecallStageDiagnostics,
    MemoryEvalRecallStageEvidenceRefs, MemoryEvidenceDocumentMutation,
    MemoryEvidenceDocumentReadReport, MemoryEvidenceDocumentReadRequest,
    MemoryEvidenceDocumentView, MemoryEvidenceDocumentWriteSummary, MemoryEvidenceRefView,
    MemoryEvidenceRefVisibility, MemoryFacetRecallIndexReport,
    MemoryGovernancePolicyMutationReport, MemoryGraphIntegrityMaintenanceReport,
    MemoryGraphIntegrityMaintenanceRequest, MemoryGraphRecallIndexReport, MemoryInspectionReport,
    MemoryInspectionRequest, MemoryLongTermDetailReport, MemoryLongTermDetailRequest,
    MemoryLongTermListReport, MemoryLongTermListRequest, MemoryLongTermMutationReport,
    MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest, MemoryMaintenanceReport,
    MemoryMaintenanceRequest, MemoryProceduralWriteReport, MemoryProjectionAuditReport,
    MemoryProjectionDeliveryDigestContentEntry, MemoryProjectionDeliveryDigestEntry,
    MemoryProjectionDeliveryDigestManifest, MemoryProjectionPrivateGateAudit,
    MemoryProjectionReport, MemoryProjectionRequest, MemoryProjectionSectionAudit,
    MemoryProjectionSourceAudit, MemoryProjectionSurfaceSet, MemoryRecallDeliveryReport,
    MemoryRecallRenderDecision, MemoryRecallRenderDropReason, MemoryRecallReport,
    MemoryRecallRequest, MemoryRecallSelectionDecision, MemoryRecallSelectionDropReason,
    MemoryRecoverReport, MemoryRecoverRequest, MemoryRenderedEvidenceCapsule, MemoryReplayReport,
    MemoryReplayRequest, MemoryRetentionCompactionReport, MemoryRetentionCompactionRequest,
    MemorySpaceArchive, MemorySpaceExportReport, MemorySpaceExportRequest,
    MemorySpaceIdentityRemapReport, MemorySpaceImportReport, MemorySpaceImportRequest,
    MemorySpaceMigrateApplyReport, MemorySpaceMigrateApplyRequest, MemorySpaceMigratePreviewReport,
    MemorySpaceMigratePreviewRequest, MemorySpaceMigrationManifest, MemorySpaceMigrationPlan,
    MemorySpaceMigrationPlaneReport, MemorySpaceMigrationPrivacyReport,
    MemorySpacePrivateMaterialPolicy, MemorySpaceProjectionScope, MemorySpaceScope,
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
    TemporalMemoryGraphNodeOwnerRef, TemporalMemoryGraphWriteRequest, WorkIntegrityReport,
    MEMORY_PROJECTION_DELIVERY_DIGEST_SCHEMA_VERSION, MEMORY_RECALL_DELIVERY_SCHEMA_VERSION,
};
pub use runtime::{
    MemoryAuditEvent, MemoryAuditSink, MemoryClock, MemoryIdentity, MemoryRuntime,
    MemoryRuntimeBuilder, MemoryRuntimeConfig, MemoryScope, NoopMemoryAuditSink,
    RuntimeBudgetLease, SystemMemoryClock,
};
#[cfg(feature = "nonproduction-replay-harness")]
pub use store::NonproductionStorePreparation;
pub use store::{
    profile_memory_system_kind, MemoryStoreHandle, MemoryStoreTelemetryReport, StoreBackendConfig,
    StoreBackendKind, StoreCapacityBudget, StoreOpenReport, StorePathBudget, StoreRepairPolicy,
    StoreRepairReport,
};

#[cfg(feature = "nonproduction-replay-harness")]
pub mod nonproduction_replay_harness {
    pub use crate::store::ReplayStoreHarness;
    pub use crate::store_internal::schema::{
        GovernedEvidenceOwnerClaimBinding, GovernedEvidenceSourceClaimManifest,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
    };
    #[cfg(feature = "sqlite-store")]
    pub use crate::store_internal::SqliteStoreEngine;
    pub use crate::store_internal::{
        EmbeddedStoreEngine, FileStoreEngine, InMemoryStoreEngine, MemoryStoreEvent,
        MemoryStoreEventKind, StoreBackendConfig, StoreBackendKind, StoreBlobAddress,
        StoreCapacityBudget, StoreConsistentBlobRead, StoreConsistentJsonRead,
        StoreConsistentReadRequest, StoreConsistentReadResult, StoreEngine, StoreEngineMutation,
        StoreEventLog, StoreEventScope, StoreJsonAddress, StoreJsonPrecondition, StoreMutation,
        StoreMutationBatch, StoreMutationBatchReport, StoreMutationBudgetReport, StoreOpenReport,
        StorePathBudget, StorePlatform, StoreRepairPolicy, StoreRepairReport, StoreSchemaManifest,
        StoreScopedProjectionReplaceReport, StoreScopedProjectionReplaceRequest,
        StoreScopedProjectionScope, StoreSnapshot, StoreSnapshotBlob, StoreSnapshotExportReport,
        StoreSnapshotImportReport, StoreSnapshotJsonDoc, StoreSnapshotReplaceReport,
        StoreTransactionReport, StoreTransactionRequest, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION,
    };
}

#[cfg(feature = "nonproduction-replay-harness")]
pub fn recall_procedural_memory(
    handle: &MemoryStoreHandle,
    query: &str,
    source_chat_id: Option<&str>,
    now_secs: u64,
    limit: usize,
) -> Vec<RuntimeSkillHit> {
    use bm_core::platform::Platform as _;

    let storage = handle.platform().skill_storage();
    bm_core::skills::retrieve_runtime_skill_hits(
        storage.as_ref(),
        query,
        source_chat_id,
        now_secs,
        limit,
    )
}

#[cfg(feature = "nonproduction-replay-harness")]
pub fn export_memory_space(
    handle: &MemoryStoreHandle,
    request: MemorySpaceExportRequest,
) -> Result<MemorySpaceExportReport> {
    export_memory_space_from_platform_with_budget(handle.platform(), request, None)
}

pub(crate) fn export_memory_space_from_platform_with_budget(
    platform: &StorePlatform,
    request: MemorySpaceExportRequest,
    runtime_budget: Option<&RuntimeBudgetReport>,
) -> Result<MemorySpaceExportReport> {
    let scope = validate_memory_space_scope(request.scope, "memory_space_export")?;
    let (mut snapshot, projection_report) = platform.export_memory_space_projection_with_report(
        &scope.memory_space_id,
        &scope.mounted_subject_id,
        runtime_budget,
    )?;
    let mut privacy_redactions = project_memory_space_snapshot(
        &mut snapshot,
        &scope,
        request.include_private,
        platform.capacity(),
    )?;
    if !request.include_private {
        privacy_redactions =
            privacy_redactions.saturating_add(projection_report.omitted_private_entries);
    }
    let export_report = snapshot.export_report();
    Ok(MemorySpaceExportReport {
        projection_scope: MemorySpaceProjectionScope {
            scope,
            includes_private: request.include_private,
        },
        archive: MemorySpaceArchive::from_snapshot(snapshot),
        export_report,
        privacy_redactions,
    })
}

#[cfg(feature = "nonproduction-replay-harness")]
pub fn import_memory_space(
    handle: &MemoryStoreHandle,
    request: MemorySpaceImportRequest,
) -> Result<MemorySpaceImportReport> {
    import_memory_space_from_platform_with_budget(handle.platform(), request, None)
}

pub(crate) fn import_memory_space_from_platform_with_budget(
    platform: &StorePlatform,
    request: MemorySpaceImportRequest,
    runtime_budget: Option<&RuntimeBudgetReport>,
) -> Result<MemorySpaceImportReport> {
    let scope = validate_memory_space_scope(request.scope, "memory_space_import")?;
    ensure_archive_memory_space_identity_and_policy(
        request.archive.snapshot(),
        &scope,
        request.expected_private_material_policy,
    )?;
    let import_report = platform.replace_memory_space_projection_with_report(
        &scope.memory_space_id,
        &scope.mounted_subject_id,
        request.archive.snapshot(),
        runtime_budget,
    )?;
    Ok(MemorySpaceImportReport {
        imported_scope: scope,
        import_report,
    })
}

pub fn preview_memory_space_migration(
    request: MemorySpaceMigratePreviewRequest,
) -> Result<MemorySpaceMigratePreviewReport> {
    let snapshot = request.archive.snapshot();
    let source_scope =
        validate_memory_space_scope(request.source_scope, "memory_space_migration_preview")?;
    let target_scope =
        validate_memory_space_scope(request.target_scope, "memory_space_migration_preview")?;
    let archive_identity_matches = archive_memory_space_scope(snapshot)
        .is_some_and(|archive_scope| archive_scope.scope == source_scope);
    let archive_policy_matches =
        archive_memory_space_scope(snapshot).is_some_and(|archive_scope| {
            archive_scope.includes_private
                == request.expected_private_material_policy.includes_private()
        });
    let privacy_redactions = count_private_snapshot_entries(snapshot);
    let loss_risk = snapshot.schema_id != crate::store_internal::STORE_SCHEMA_ID;
    let report = snapshot.export_report();
    let vault_manifest = VaultManifest {
        identity_id: format!(
            "{}:{}",
            source_scope.memory_space_id, source_scope.mounted_subject_id
        ),
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
    if !archive_identity_matches || !archive_policy_matches {
        vault_preflight.passed = false;
    }
    if source_scope != target_scope {
        // This archive is intentionally identity-bound. A future migration remapper must
        // produce a new typed projection before apply; snapshot import cannot relabel it.
        vault_preflight.passed = false;
    }
    let manifest =
        build_memory_space_migration_manifest(&source_scope, &target_scope, snapshot, loss_risk);
    let plan = MemorySpaceMigrationPlan {
        target_scope: target_scope.clone(),
        expected_private_material_policy: request.expected_private_material_policy,
        snapshot: request.archive.into_snapshot(),
        preflight: vault_preflight.clone(),
    };
    Ok(MemorySpaceMigratePreviewReport {
        source_scope,
        target_scope,
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
    })
}

#[cfg(feature = "nonproduction-replay-harness")]
pub fn apply_memory_space_migration(
    handle: &MemoryStoreHandle,
    request: MemorySpaceMigrateApplyRequest,
) -> Result<MemorySpaceMigrateApplyReport> {
    apply_memory_space_migration_from_platform_with_budget(handle.platform(), request, None)
}

pub(crate) fn apply_memory_space_migration_from_platform_with_budget(
    platform: &StorePlatform,
    request: MemorySpaceMigrateApplyRequest,
    runtime_budget: Option<&RuntimeBudgetReport>,
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
    ensure_archive_memory_space_identity_and_policy(
        &plan.snapshot,
        &plan.target_scope,
        plan.expected_private_material_policy,
    )?;
    let import_report = platform.replace_memory_space_projection_with_report(
        &plan.target_scope.memory_space_id,
        &plan.target_scope.mounted_subject_id,
        &plan.snapshot,
        runtime_budget,
    )?;
    Ok(MemorySpaceMigrateApplyReport {
        target_scope: plan.target_scope,
        import_report,
    })
}

fn validate_memory_space_scope(
    scope: MemorySpaceScope,
    stage: &'static str,
) -> Result<MemorySpaceScope> {
    if scope.memory_space_id.trim().is_empty()
        || scope.mounted_subject_id.trim().is_empty()
        || scope.memory_space_id != scope.memory_space_id.trim()
        || scope.mounted_subject_id != scope.mounted_subject_id.trim()
    {
        return Err(Error::config(
            stage,
            "memory_space_id and mounted_subject_id must be canonical non-empty values",
        ));
    }
    Ok(scope)
}

fn project_memory_space_snapshot(
    snapshot: &mut StoreSnapshot,
    scope: &MemorySpaceScope,
    include_private: bool,
    capacity: StoreCapacityBudget,
) -> Result<usize> {
    let privacy_redactions = (!include_private).then(|| count_private_snapshot_entries(snapshot));

    validate_memory_space_projection_ownership(snapshot, scope, capacity)?;
    snapshot.events.retain(|event| {
        event.scope.memory_space_id == scope.memory_space_id
            && event.scope.subject_id == scope.mounted_subject_id
    });

    // Blob namespaces currently have no typed memory-space owner. Omitting them is the only
    // fail-closed projection until their persisted schema carries an explicit scope owner.
    snapshot.blobs.clear();

    if !include_private {
        redact_private_snapshot_entries(snapshot, scope, capacity.kv_max_entries)?;
    }
    validate_projected_evidence_source_claim_closure(
        snapshot,
        &scope.memory_space_id,
        capacity.kv_max_entries,
    )?;
    store_internal::validate_scoped_projection_governed_closure(
        snapshot,
        &scope.memory_space_id,
        &scope.mounted_subject_id,
    )?;
    snapshot.schema_manifest.projection_scope =
        store_internal::schema::StoreProjectionScope::MemorySpace {
            memory_space_id: scope.memory_space_id.clone(),
            mounted_subject_id: scope.mounted_subject_id.clone(),
            includes_private: include_private,
        };
    Ok(privacy_redactions.unwrap_or(0))
}

fn validate_memory_space_projection_ownership(
    snapshot: &StoreSnapshot,
    scope: &MemorySpaceScope,
    capacity: StoreCapacityBudget,
) -> Result<()> {
    let scoped_store_scope = store_internal::StoreScopedProjectionScope {
        memory_space_id: scope.memory_space_id.clone(),
        mounted_subject_id: scope.mounted_subject_id.clone(),
    };
    let docs = snapshot
        .json_docs
        .iter()
        .map(|doc| ((doc.namespace.clone(), doc.key.clone()), doc.value.clone()))
        .collect::<BTreeMap<_, _>>();
    let namespaces = docs
        .keys()
        .map(|(namespace, _)| namespace.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let owned_addresses = store_internal::scoped_projection_json_addresses(
        &namespaces,
        &docs,
        &scoped_store_scope,
        capacity,
    )?;
    for doc in &snapshot.json_docs {
        let address = (doc.namespace.clone(), doc.key.clone());
        if !owned_addresses.contains(&address) {
            return Err(Error::config(
                "memory_space_export",
                format!(
                    "projection contains an unowned or cross-scope document: {}/{}",
                    doc.namespace, doc.key
                ),
            ));
        }
    }
    Ok(())
}

#[derive(Default)]
struct ProjectedEvidenceSourceClaimScope {
    owner_keys: Vec<String>,
    expected_claim_keys: Vec<String>,
    actual_claim_keys: Vec<String>,
    manifest: Option<store_internal::schema::GovernedEvidenceSourceClaimManifest>,
}

fn validate_projected_evidence_source_claim_closure(
    snapshot: &StoreSnapshot,
    memory_space_id: &str,
    max_scope_entries: usize,
) -> Result<()> {
    let mut scopes = BTreeMap::<String, ProjectedEvidenceSourceClaimScope>::new();
    for doc in &snapshot.json_docs {
        if doc.namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE {
            let owner = serde_json::from_value::<bm_core::memory::GovernedEvidenceDocument>(
                doc.value.clone(),
            )
            .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
            if owner.memory_space_id != memory_space_id || owner.physical_key != doc.key {
                return Err(Error::config(
                    "memory_space_export",
                    "evidence owner does not match projected memory-space identity",
                ));
            }
            let expected_claim =
                bm_core::memory::governed_evidence_source_ref_from_document(&owner)
                    .map_err(|error| Error::config("memory_space_export", format!("{error:?}")))?;
            let scope = scopes.entry(owner.mounted_subject_id.clone()).or_default();
            scope.owner_keys.push(owner.physical_key);
            scope.expected_claim_keys.push(expected_claim.physical_key);
        } else if doc.namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE {
            let claim = serde_json::from_value::<bm_core::memory::GovernedEvidenceSourceRef>(
                doc.value.clone(),
            )
            .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
            if claim.memory_space_id != memory_space_id || claim.physical_key != doc.key {
                return Err(Error::config(
                    "memory_space_export",
                    "evidence source claim does not match projected memory-space identity",
                ));
            }
            scopes
                .entry(claim.mounted_subject_id)
                .or_default()
                .actual_claim_keys
                .push(claim.physical_key);
        } else if doc.namespace
            == store_internal::schema::GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE
        {
            let manifest = serde_json::from_value::<
                store_internal::schema::GovernedEvidenceSourceClaimManifest,
            >(doc.value.clone())
            .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
            if manifest.memory_space_id != memory_space_id || manifest.physical_key != doc.key {
                return Err(Error::config(
                    "memory_space_export",
                    "evidence claim manifest does not match projected memory-space identity",
                ));
            }
            let scope = scopes
                .entry(manifest.mounted_subject_id.clone())
                .or_default();
            if scope.manifest.replace(manifest).is_some() {
                return Err(Error::config(
                    "memory_space_export",
                    "duplicate evidence claim manifest for projected subject",
                ));
            }
        }
    }

    for (subject_id, scope) in scopes {
        let manifest = scope.manifest.as_ref().ok_or_else(|| {
            Error::config(
                "memory_space_export",
                "evidence source claim scope manifest is missing",
            )
        })?;
        let mut owner_keys = scope.owner_keys;
        let mut expected_claim_keys = scope.expected_claim_keys;
        let mut actual_claim_keys = scope.actual_claim_keys;
        owner_keys.sort();
        expected_claim_keys.sort();
        actual_claim_keys.sort();
        if owner_keys != manifest.owner_keys
            || expected_claim_keys != manifest.claim_keys
            || actual_claim_keys != manifest.claim_keys
        {
            return Err(Error::config(
                "memory_space_export",
                "projected evidence owner/claim key index does not match typed manifest",
            ));
        }
        store_internal::schema::validate_governed_evidence_source_claim_scope_closure(
            Some(manifest),
            memory_space_id,
            &subject_id,
            manifest.owner_claim_bindings.clone(),
            max_scope_entries,
        )?;
    }
    Ok(())
}

fn archive_memory_space_scope(snapshot: &StoreSnapshot) -> Option<MemorySpaceProjectionScope> {
    match &snapshot.schema_manifest.projection_scope {
        store_internal::schema::StoreProjectionScope::MemorySpace {
            memory_space_id,
            mounted_subject_id,
            includes_private,
        } => Some(MemorySpaceProjectionScope {
            scope: MemorySpaceScope {
                memory_space_id: memory_space_id.clone(),
                mounted_subject_id: mounted_subject_id.clone(),
            },
            includes_private: *includes_private,
        }),
        store_internal::schema::StoreProjectionScope::FullStore => None,
    }
}

fn ensure_archive_memory_space_identity(
    snapshot: &StoreSnapshot,
    expected_scope: &MemorySpaceScope,
) -> Result<()> {
    match archive_memory_space_scope(snapshot) {
        Some(actual) if actual.scope == *expected_scope => Ok(()),
        Some(actual) => Err(Error::config(
            "memory_space_import",
            format!(
                "archive scope mismatch: archive={:?}, request={expected_scope:?}",
                actual.scope
            ),
        )),
        None => Err(Error::config(
            "memory_space_import",
            "archive is not a typed memory-space projection",
        )),
    }
}

fn ensure_archive_memory_space_identity_and_policy(
    snapshot: &StoreSnapshot,
    expected_scope: &MemorySpaceScope,
    expected_policy: MemorySpacePrivateMaterialPolicy,
) -> Result<()> {
    ensure_archive_memory_space_identity(snapshot, expected_scope)?;
    let actual = archive_memory_space_scope(snapshot).ok_or_else(|| {
        Error::config(
            "memory_space_import",
            "archive is not a typed memory-space projection",
        )
    })?;
    if actual.includes_private != expected_policy.includes_private() {
        return Err(Error::config(
            "memory_space_import",
            format!(
                "archive private-material policy mismatch: archive_includes_private={}, expected_includes_private={}",
                actual.includes_private,
                expected_policy.includes_private()
            ),
        ));
    }
    Ok(())
}

fn redact_private_snapshot_entries(
    snapshot: &mut StoreSnapshot,
    scope: &MemorySpaceScope,
    max_scope_entries: usize,
) -> Result<usize> {
    let before_entries = snapshot
        .json_docs
        .len()
        .saturating_add(snapshot.blobs.len())
        .saturating_add(snapshot.events.len());
    let governed_owners_before = governed_snapshot_owner_refs(snapshot)?;
    snapshot
        .json_docs
        .retain(|doc| !snapshot_doc_requires_private_export(doc));
    snapshot
        .blobs
        .retain(|blob| !is_private_snapshot_namespace(&blob.namespace));
    snapshot.events.retain(|event| {
        !is_private_snapshot_namespace(&event.plane)
            && !is_private_snapshot_key(event.record_key.as_str())
    });
    let governed_owners_after = governed_snapshot_owner_refs(snapshot)?;
    let governed_owner_removed = governed_owners_before != governed_owners_after;
    rebuild_projected_evidence_source_claim_closure(snapshot, scope, max_scope_entries)?;
    rebuild_projected_facet_closure(snapshot, &governed_owners_after)?;
    if governed_owner_removed {
        rebuild_projected_graph_closure(snapshot, scope, &governed_owners_after)?;
        snapshot.events.retain(|event| {
            !is_governed_owner_or_derived_namespace(&event.plane)
                && !is_memory_graph_namespace(&event.plane)
        });
    }
    let after_entries = snapshot
        .json_docs
        .len()
        .saturating_add(snapshot.blobs.len())
        .saturating_add(snapshot.events.len());
    Ok(before_entries.saturating_sub(after_entries))
}

fn governed_snapshot_owner_refs(
    snapshot: &StoreSnapshot,
) -> Result<BTreeSet<bm_core::memory::GovernedMemoryOwnerRef>> {
    let mut owners = BTreeSet::new();
    for doc in &snapshot.json_docs {
        let owner_ref = if doc.namespace == "long_term" {
            let owner =
                serde_json::from_value::<bm_core::memory::LongTermMemoryEntry>(doc.value.clone())
                    .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
            Some(bm_core::memory::GovernedMemoryOwnerRef::new(
                bm_core::memory::GovernedMemoryOwnerPlane::LongTerm,
                owner.id,
            ))
        } else if doc.namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE {
            let owner = serde_json::from_value::<bm_core::memory::GovernedEvidenceDocument>(
                doc.value.clone(),
            )
            .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
            Some(bm_core::memory::GovernedMemoryOwnerRef::new(
                bm_core::memory::GovernedMemoryOwnerPlane::EvidenceDocument,
                owner.document_id,
            ))
        } else {
            None
        };
        if let Some(owner_ref) = owner_ref {
            owners.insert(owner_ref);
        }
    }
    Ok(owners)
}

fn rebuild_projected_evidence_source_claim_closure(
    snapshot: &mut StoreSnapshot,
    scope: &MemorySpaceScope,
    max_scope_entries: usize,
) -> Result<()> {
    let owners = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE)
        .map(|doc| {
            serde_json::from_value::<bm_core::memory::GovernedEvidenceDocument>(doc.value.clone())
                .map_err(|error| Error::config("memory_space_export", error.to_string()))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut claims = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE)
        .map(|doc| {
            let claim = serde_json::from_value::<bm_core::memory::GovernedEvidenceSourceRef>(
                doc.value.clone(),
            )
            .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
            if claim.physical_key != doc.key {
                return Err(Error::config(
                    "memory_space_export",
                    "evidence source claim physical key mismatch",
                ));
            }
            Ok((claim.physical_key.clone(), claim))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut retained_claims = Vec::with_capacity(owners.len());
    let mut bindings = Vec::with_capacity(owners.len());
    for owner in &owners {
        if owner.memory_space_id != scope.memory_space_id
            || owner.mounted_subject_id != scope.mounted_subject_id
        {
            return Err(Error::config(
                "memory_space_export",
                "retained evidence owner is outside the projected scope",
            ));
        }
        let expected = bm_core::memory::governed_evidence_source_ref_from_document(owner)
            .map_err(|error| Error::config("memory_space_export", format!("{error:?}")))?;
        let claim = claims.remove(&expected.physical_key).ok_or_else(|| {
            Error::config(
                "memory_space_export",
                "retained evidence owner is missing its exact source claim",
            )
        })?;
        bm_core::memory::validate_governed_evidence_source_ref(owner, &claim)
            .map_err(|error| Error::config("memory_space_export", format!("{error:?}")))?;
        bindings.push(
            store_internal::GovernedEvidenceOwnerClaimBinding::from_document_claim(owner, &claim)?,
        );
        retained_claims.push(claim);
    }

    snapshot.json_docs.retain(|doc| {
        doc.namespace != GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE
            && doc.namespace != GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE
    });
    for claim in retained_claims {
        snapshot.json_docs.push(StoreSnapshotJsonDoc {
            namespace: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
            key: claim.physical_key.clone(),
            value: serde_json::to_value(claim)
                .map_err(|error| Error::config("memory_space_export", error.to_string()))?,
        });
    }
    if !owners.is_empty() {
        let manifest = store_internal::GovernedEvidenceSourceClaimManifest::build(
            &scope.memory_space_id,
            &scope.mounted_subject_id,
            bindings,
            max_scope_entries,
        )?;
        snapshot.json_docs.push(StoreSnapshotJsonDoc {
            namespace: GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
            key: manifest.physical_key.clone(),
            value: serde_json::to_value(manifest)
                .map_err(|error| Error::config("memory_space_export", error.to_string()))?,
        });
    }
    Ok(())
}

fn rebuild_projected_facet_closure(
    snapshot: &mut StoreSnapshot,
    retained_owners: &BTreeSet<bm_core::memory::GovernedMemoryOwnerRef>,
) -> Result<()> {
    let mut owner_versions = Vec::new();
    let mut posting_revisions = Vec::new();
    let mut rebuilt = Vec::with_capacity(snapshot.json_docs.len());
    let mut manifest_doc = None;

    for mut doc in snapshot.json_docs.drain(..) {
        match doc.namespace.as_str() {
            bm_core::memory::MEMORY_FACET_INDEX_NAMESPACE => {
                let facet = serde_json::from_value::<bm_core::memory::MemoryFacetIndexDoc>(
                    doc.value.clone(),
                )
                .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
                if retained_owners.contains(&facet.owner_ref) {
                    owner_versions.push(bm_core::memory::MemoryFacetOwnerVersion {
                        owner_ref: facet.owner_ref.clone(),
                        owner_revision: facet.owner_revision,
                        facet_index_revision: facet.facet_index_revision,
                    });
                    rebuilt.push(doc);
                }
            }
            bm_core::memory::MEMORY_FACET_POSTING_NAMESPACE => {
                if serde_json::from_value::<bm_core::memory::MemoryFacetIndexManifest>(
                    doc.value.clone(),
                )
                .is_ok()
                {
                    if manifest_doc.replace(doc).is_some() {
                        return Err(Error::config(
                            "memory_space_export",
                            "projected facet closure contains duplicate manifests",
                        ));
                    }
                    continue;
                }
                let mut posting = serde_json::from_value::<bm_core::memory::MemoryFacetPostingDoc>(
                    doc.value.clone(),
                )
                .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
                posting
                    .owner_versions
                    .retain(|version| retained_owners.contains(&version.owner_ref));
                if !posting.owner_versions.is_empty() {
                    posting.owner_versions.sort();
                    doc.value = serde_json::to_value(&posting)
                        .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
                    posting_revisions.push(bm_core::memory::MemoryFacetPostingRevision {
                        posting_key: posting.posting_key,
                        revision: posting.revision,
                    });
                    rebuilt.push(doc);
                }
            }
            _ => rebuilt.push(doc),
        }
    }

    owner_versions.sort();
    posting_revisions.sort();
    if !owner_versions.is_empty() {
        let mut manifest_doc = manifest_doc.ok_or_else(|| {
            Error::config(
                "memory_space_export",
                "retained governed owners require a facet manifest",
            )
        })?;
        let mut manifest = serde_json::from_value::<bm_core::memory::MemoryFacetIndexManifest>(
            manifest_doc.value.clone(),
        )
        .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
        manifest.owner_versions = owner_versions;
        manifest.posting_revisions = posting_revisions;
        manifest.owner_doc_count = manifest.owner_versions.len();
        manifest.posting_doc_count = manifest.posting_revisions.len();
        manifest_doc.value = serde_json::to_value(manifest)
            .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
        rebuilt.push(manifest_doc);
    }
    snapshot.json_docs = rebuilt;
    Ok(())
}

fn rebuild_projected_graph_closure(
    snapshot: &mut StoreSnapshot,
    scope: &MemorySpaceScope,
    retained_owners: &BTreeSet<bm_core::memory::GovernedMemoryOwnerRef>,
) -> Result<()> {
    let graph_docs = snapshot
        .json_docs
        .iter()
        .filter(|doc| is_memory_graph_namespace(&doc.namespace))
        .collect::<Vec<_>>();
    if graph_docs.is_empty() {
        return Ok(());
    }
    let manifests = graph_docs
        .iter()
        .filter(|doc| doc.namespace == bm_core::memory::MEMORY_GRAPH_MANIFEST_NAMESPACE)
        .map(|doc| {
            serde_json::from_value::<bm_core::memory::MemoryGraphScopeManifest>(doc.value.clone())
                .map_err(|error| Error::config("memory_space_export", error.to_string()))
        })
        .collect::<Result<Vec<_>>>()?;
    if manifests.len() != 1 {
        return Err(Error::config(
            "memory_space_export",
            "projected graph closure requires exactly one scope manifest",
        ));
    }
    let manifest = &manifests[0];
    if manifest.memory_space_id != scope.memory_space_id
        || manifest.mounted_subject_id != scope.mounted_subject_id
    {
        return Err(Error::config(
            "memory_space_export",
            "projected graph manifest is outside the requested scope",
        ));
    }

    let memberships = graph_docs
        .iter()
        .filter(|doc| doc.namespace == bm_core::memory::MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE)
        .map(|doc| {
            serde_json::from_value::<bm_core::memory::MemoryGraphNodeMembership>(doc.value.clone())
                .map_err(|error| Error::config("memory_space_export", error.to_string()))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|membership| retained_owners.contains(&membership.owner_ref))
        .collect::<Vec<_>>();
    if memberships.is_empty() {
        snapshot
            .json_docs
            .retain(|doc| !is_memory_graph_namespace(&doc.namespace));
        return Ok(());
    }
    let retained_node_ids = memberships
        .iter()
        .map(|membership| membership.node_id.clone())
        .collect::<BTreeSet<_>>();
    let nodes_by_id = graph_docs
        .iter()
        .filter(|doc| doc.namespace == bm_core::memory::MEMORY_GRAPH_NODE_NAMESPACE)
        .map(|doc| {
            let node =
                serde_json::from_value::<bm_core::memory::MemoryGraphNode>(doc.value.clone())
                    .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
            Ok((node.node_id.clone(), node))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let nodes = retained_node_ids
        .iter()
        .map(|node_id| {
            nodes_by_id.get(node_id).cloned().ok_or_else(|| {
                Error::config(
                    "memory_space_export",
                    "retained graph node membership is missing its document",
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let edges = graph_docs
        .iter()
        .filter(|doc| doc.namespace == bm_core::memory::MEMORY_GRAPH_EDGE_NAMESPACE)
        .map(|doc| {
            serde_json::from_value::<bm_core::memory::MemoryGraphEdge>(doc.value.clone())
                .map_err(|error| Error::config("memory_space_export", error.to_string()))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|edge| {
            retained_node_ids.contains(&edge.from_node_id)
                && retained_node_ids.contains(&edge.to_node_id)
        })
        .collect::<Vec<_>>();
    let required_backlink_sources = nodes
        .iter()
        .flat_map(|node| node.evidence_refs.iter().cloned())
        .chain(
            edges
                .iter()
                .flat_map(|edge| edge.evidence_refs.iter().cloned()),
        )
        .collect::<BTreeSet<_>>();
    let backlinks_by_source = graph_docs
        .iter()
        .filter(|doc| doc.namespace == bm_core::memory::MEMORY_GRAPH_BACKLINK_NAMESPACE)
        .map(|doc| {
            let backlink =
                serde_json::from_value::<bm_core::memory::EvidenceBacklink>(doc.value.clone())
                    .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
            Ok((backlink.source_id.clone(), backlink))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let backlinks = required_backlink_sources
        .iter()
        .map(|source_id| {
            backlinks_by_source.get(source_id).cloned().ok_or_else(|| {
                Error::config(
                    "memory_space_export",
                    "retained graph evidence is missing its backlink",
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let owner_revisions = governed_snapshot_owner_revisions(snapshot)?;
    let owner_bindings = memberships
        .iter()
        .map(|membership| {
            let owner_revision = owner_revisions
                .get(&membership.owner_ref)
                .copied()
                .ok_or_else(|| {
                    Error::config(
                        "memory_space_export",
                        "retained graph membership owner is missing",
                    )
                })?;
            Ok(bm_core::memory::MemoryGraphOwnerBinding {
                node_id: membership.node_id.clone(),
                owner_ref: membership.owner_ref.clone(),
                owner_revision,
                visible: true,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let persistence = bm_core::memory::build_memory_graph_persistence_plan(
        scope.memory_space_id.clone(),
        scope.mounted_subject_id.clone(),
        manifest.manifest_generation,
        nodes.clone(),
        edges.clone(),
        backlinks.clone(),
        owner_bindings,
    );
    if !persistence.accepted {
        return Err(Error::config(
            "memory_space_export",
            format!(
                "projected graph closure rebuild failed: {}",
                persistence.failures.join(",")
            ),
        ));
    }

    snapshot
        .json_docs
        .retain(|doc| !is_memory_graph_namespace(&doc.namespace));
    append_projected_graph_closure(snapshot, nodes, edges, backlinks, persistence)?;
    Ok(())
}

fn governed_snapshot_owner_revisions(
    snapshot: &StoreSnapshot,
) -> Result<BTreeMap<bm_core::memory::GovernedMemoryOwnerRef, u64>> {
    let mut revisions = BTreeMap::new();
    for doc in &snapshot.json_docs {
        let owner = if doc.namespace == "long_term" {
            let owner =
                serde_json::from_value::<bm_core::memory::LongTermMemoryEntry>(doc.value.clone())
                    .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
            Some((
                bm_core::memory::GovernedMemoryOwnerRef::new(
                    bm_core::memory::GovernedMemoryOwnerPlane::LongTerm,
                    owner.id,
                ),
                owner.owner_revision,
            ))
        } else if doc.namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE {
            let owner = serde_json::from_value::<bm_core::memory::GovernedEvidenceDocument>(
                doc.value.clone(),
            )
            .map_err(|error| Error::config("memory_space_export", error.to_string()))?;
            Some((
                bm_core::memory::GovernedMemoryOwnerRef::new(
                    bm_core::memory::GovernedMemoryOwnerPlane::EvidenceDocument,
                    owner.document_id,
                ),
                owner.owner_revision,
            ))
        } else {
            None
        };
        if let Some((owner_ref, revision)) = owner {
            if revisions.insert(owner_ref, revision).is_some() {
                return Err(Error::config(
                    "memory_space_export",
                    "duplicate governed owner in projected snapshot",
                ));
            }
        }
    }
    Ok(revisions)
}

fn append_projected_graph_closure(
    snapshot: &mut StoreSnapshot,
    nodes: Vec<bm_core::memory::MemoryGraphNode>,
    edges: Vec<bm_core::memory::MemoryGraphEdge>,
    backlinks: Vec<bm_core::memory::EvidenceBacklink>,
    persistence: bm_core::memory::MemoryGraphPersistencePlan,
) -> Result<()> {
    let node_memberships = persistence
        .node_memberships
        .iter()
        .map(|membership| (membership.node_id.clone(), membership))
        .collect::<BTreeMap<_, _>>();
    let edge_memberships = persistence
        .edge_memberships
        .iter()
        .map(|membership| (membership.edge_id.clone(), membership))
        .collect::<BTreeMap<_, _>>();
    let backlink_memberships = persistence
        .backlink_memberships
        .iter()
        .map(|membership| (membership.backlink_key.clone(), membership))
        .collect::<BTreeMap<_, _>>();
    for node in nodes {
        let membership = node_memberships.get(&node.node_id).ok_or_else(|| {
            Error::config(
                "memory_space_export",
                "rebuilt graph node membership is missing",
            )
        })?;
        push_projected_json(
            snapshot,
            bm_core::memory::MEMORY_GRAPH_NODE_NAMESPACE,
            &membership.document_key,
            node,
        )?;
    }
    for edge in edges {
        let membership = edge_memberships.get(&edge.edge_id).ok_or_else(|| {
            Error::config(
                "memory_space_export",
                "rebuilt graph edge membership is missing",
            )
        })?;
        push_projected_json(
            snapshot,
            bm_core::memory::MEMORY_GRAPH_EDGE_NAMESPACE,
            &membership.document_key,
            edge,
        )?;
    }
    for backlink in backlinks {
        let backlink_key =
            bm_core::memory::memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id);
        let membership = backlink_memberships.get(&backlink_key).ok_or_else(|| {
            Error::config(
                "memory_space_export",
                "rebuilt graph backlink membership is missing",
            )
        })?;
        push_projected_json(
            snapshot,
            bm_core::memory::MEMORY_GRAPH_BACKLINK_NAMESPACE,
            &membership.document_key,
            backlink,
        )?;
    }
    for membership in persistence.node_memberships {
        push_projected_json(
            snapshot,
            bm_core::memory::MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
            &membership.membership_key.clone(),
            membership,
        )?;
    }
    for membership in persistence.edge_memberships {
        push_projected_json(
            snapshot,
            bm_core::memory::MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
            &membership.membership_key.clone(),
            membership,
        )?;
    }
    for membership in persistence.backlink_memberships {
        push_projected_json(
            snapshot,
            bm_core::memory::MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
            &membership.membership_key.clone(),
            membership,
        )?;
    }
    for index in persistence.recall_indexes {
        push_projected_json(
            snapshot,
            bm_core::memory::MEMORY_GRAPH_INDEX_NAMESPACE,
            &index.index_key.clone(),
            index,
        )?;
    }
    let revision = persistence
        .revision
        .ok_or_else(|| Error::config("memory_space_export", "rebuilt graph revision is missing"))?;
    push_projected_json(
        snapshot,
        bm_core::memory::MEMORY_GRAPH_REVISION_NAMESPACE,
        &revision.revision_key.clone(),
        revision,
    )?;
    let manifest = persistence
        .scope_manifest
        .ok_or_else(|| Error::config("memory_space_export", "rebuilt graph manifest is missing"))?;
    let manifest_key = bm_core::memory::memory_graph_scope_manifest_key(
        &manifest.memory_space_id,
        &manifest.mounted_subject_id,
    );
    push_projected_json(
        snapshot,
        bm_core::memory::MEMORY_GRAPH_MANIFEST_NAMESPACE,
        &manifest_key,
        manifest,
    )?;
    snapshot.json_docs.sort_by(|left, right| {
        left.namespace
            .cmp(&right.namespace)
            .then_with(|| left.key.cmp(&right.key))
    });
    Ok(())
}

fn push_projected_json(
    snapshot: &mut StoreSnapshot,
    namespace: &str,
    key: &str,
    value: impl serde::Serialize,
) -> Result<()> {
    snapshot.json_docs.push(StoreSnapshotJsonDoc {
        namespace: namespace.to_string(),
        key: key.to_string(),
        value: serde_json::to_value(value)
            .map_err(|error| Error::config("memory_space_export", error.to_string()))?,
    });
    Ok(())
}

fn is_memory_graph_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        bm_core::memory::MEMORY_GRAPH_MANIFEST_NAMESPACE
            | bm_core::memory::MEMORY_GRAPH_REVISION_NAMESPACE
            | bm_core::memory::MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE
            | bm_core::memory::MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE
            | bm_core::memory::MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE
            | bm_core::memory::MEMORY_GRAPH_INDEX_NAMESPACE
            | bm_core::memory::MEMORY_GRAPH_NODE_NAMESPACE
            | bm_core::memory::MEMORY_GRAPH_EDGE_NAMESPACE
            | bm_core::memory::MEMORY_GRAPH_BACKLINK_NAMESPACE
    )
}

fn is_governed_owner_or_derived_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        "long_term"
            | GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
            | GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE
            | GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE
            | bm_core::memory::MEMORY_FACET_INDEX_NAMESPACE
            | bm_core::memory::MEMORY_FACET_POSTING_NAMESPACE
    )
}

fn count_private_snapshot_entries(snapshot: &StoreSnapshot) -> usize {
    snapshot
        .json_docs
        .iter()
        .filter(|doc| snapshot_doc_requires_private_export(doc))
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

fn snapshot_doc_requires_private_export(doc: &StoreSnapshotJsonDoc) -> bool {
    match doc.namespace.as_str() {
        "long_term" => {
            serde_json::from_value::<bm_core::memory::LongTermMemoryEntry>(doc.value.clone())
                .map(|owner| !owner.privacy.projection_content_allowed())
                .unwrap_or(true)
        }
        GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE => {
            serde_json::from_value::<bm_core::memory::GovernedEvidenceDocument>(doc.value.clone())
                .map(|document| !document.privacy.projection_content_allowed())
                .unwrap_or(true)
        }
        GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE
        | GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE
        | bm_core::memory::MEMORY_FACET_INDEX_NAMESPACE
        | bm_core::memory::MEMORY_FACET_POSTING_NAMESPACE
        | bm_core::memory::MEMORY_GRAPH_MANIFEST_NAMESPACE
        | bm_core::memory::MEMORY_GRAPH_REVISION_NAMESPACE
        | bm_core::memory::MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE
        | bm_core::memory::MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE
        | bm_core::memory::MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE
        | bm_core::memory::MEMORY_GRAPH_INDEX_NAMESPACE
        | bm_core::memory::MEMORY_GRAPH_NODE_NAMESPACE
        | bm_core::memory::MEMORY_GRAPH_EDGE_NAMESPACE
        | bm_core::memory::MEMORY_GRAPH_BACKLINK_NAMESPACE => false,
        _ => snapshot_json_requires_private_export(&doc.namespace, &doc.value),
    }
}

fn build_vault_redaction_report(snapshot: &StoreSnapshot) -> PrivateMaterialRedactionReport {
    let mut checked_refs = Vec::new();
    let mut redacted_refs = Vec::new();
    for doc in &snapshot.json_docs {
        let record_ref = format!("json:{}:{}", doc.namespace, doc.key);
        checked_refs.push(record_ref.clone());
        if snapshot_doc_requires_private_export(doc) {
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
    build_vault_migration_preflight(
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
    )
}

fn privacy_policy_fingerprint(privacy_redactions: usize, loss_risk: bool) -> String {
    format!(
        "privacy-redactions:{privacy_redactions}:schema-loss-risk:{}",
        u8::from(loss_risk)
    )
}

fn build_memory_space_migration_manifest(
    source_scope: &MemorySpaceScope,
    target_scope: &MemorySpaceScope,
    snapshot: &StoreSnapshot,
    loss_risk: bool,
) -> MemorySpaceMigrationManifest {
    let mut plane_counts = BTreeMap::<String, (usize, String)>::new();
    let mut privacy_counts = BTreeMap::<String, usize>::new();
    for doc in &snapshot.json_docs {
        accumulate_migration_record(
            &mut plane_counts,
            &mut privacy_counts,
            &doc.namespace,
            snapshot_doc_requires_private_export(doc),
        );
    }
    for blob in &snapshot.blobs {
        accumulate_migration_record(
            &mut plane_counts,
            &mut privacy_counts,
            &blob.namespace,
            is_private_snapshot_namespace(&blob.namespace),
        );
    }
    for event in &snapshot.events {
        accumulate_migration_record(
            &mut plane_counts,
            &mut privacy_counts,
            &event.plane,
            is_private_snapshot_namespace(&event.plane)
                || is_private_snapshot_key(&event.record_key),
        );
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
    let projection_scope =
        archive_memory_space_scope(snapshot).unwrap_or_else(|| MemorySpaceProjectionScope {
            scope: source_scope.clone(),
            includes_private: false,
        });
    let remap_required = source_scope != target_scope;
    MemorySpaceMigrationManifest {
        projection_scope,
        target_scope: target_scope.clone(),
        schema_id: snapshot.schema_id.clone(),
        identity_remap: MemorySpaceIdentityRemapReport {
            required: remap_required,
            applied: false,
            reason: if remap_required {
                "scoped_projection_does_not_rewrite_identity"
            } else {
                "source_and_target_scope_match"
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
    private: bool,
) {
    let privacy_class = if private { "private" } else { "shared" };
    let entry = plane_counts
        .entry(plane.to_string())
        .or_insert_with(|| (0, privacy_class.to_string()));
    entry.0 = entry.0.saturating_add(1);
    *privacy_counts.entry(privacy_class.to_string()).or_insert(0) += 1;
}

fn is_private_snapshot_namespace(namespace: &str) -> bool {
    snapshot_namespace_requires_private_export(namespace)
}

fn is_private_snapshot_key(key: &str) -> bool {
    snapshot_key_requires_private_export(key)
}

#[cfg(test)]
mod p7_6_memory_space_projection_tests {
    use super::*;

    fn scope(space: &str, subject: &str) -> MemorySpaceScope {
        MemorySpaceScope {
            memory_space_id: space.to_string(),
            mounted_subject_id: subject.to_string(),
        }
    }

    fn projection_capacity() -> StoreCapacityBudget {
        let mut capacity = StoreCapacityBudget::full();
        capacity.kv_max_entries = 64;
        capacity
    }

    fn snapshot_with_docs() -> StoreSnapshot {
        StoreSnapshot::new(
            StoreSchemaManifest::new(
                StoreBackendKind::InMemory,
                ProfileId::DesktopMacosEmbeddedSdk,
                1,
            ),
            vec![
                StoreSnapshotJsonDoc {
                    namespace: "turn_ledger".to_string(),
                    key: "space-a-turn".to_string(),
                    value: serde_json::json!({
                        "memory_space_id": "space:a",
                        "subject_id": "subject:a",
                        "body": "space a"
                    }),
                },
                StoreSnapshotJsonDoc {
                    namespace: "turn_ledger".to_string(),
                    key: "space-b-turn".to_string(),
                    value: serde_json::json!({
                        "memory_space_id": "space:b",
                        "subject_id": "subject:b",
                        "body": "space b"
                    }),
                },
                StoreSnapshotJsonDoc {
                    namespace: "conversation_transcript".to_string(),
                    key: "space-a-private-transcript".to_string(),
                    value: serde_json::json!({
                        "memory_space_id": "space:a",
                        "subject_id": "subject:a",
                        "source_locator": "raw://must-not-leak",
                        "body": "raw evidence"
                    }),
                },
                StoreSnapshotJsonDoc {
                    namespace: "session".to_string(),
                    key: "unowned".to_string(),
                    value: serde_json::json!({"content": "legacy unowned"}),
                },
            ],
            vec![StoreSnapshotBlob {
                namespace: "memory".to_string(),
                key: "unowned-blob".to_string(),
                value: b"raw".to_vec(),
            }],
            Vec::new(),
        )
    }

    #[test]
    fn memory_space_projection_rejects_unowned_records_and_keeps_typed_owned_records() {
        let mut snapshot = snapshot_with_docs();
        let source_scope = scope("space:a", "subject:a");
        assert!(
            project_memory_space_snapshot(
                &mut snapshot,
                &source_scope,
                false,
                projection_capacity(),
            )
            .is_err(),
            "backend projection must fail closed instead of passing a mixed-scope snapshot"
        );
        let mut snapshot = snapshot_with_docs();
        snapshot.json_docs.retain(|doc| doc.key == "space-a-turn");
        let redactions = project_memory_space_snapshot(
            &mut snapshot,
            &source_scope,
            false,
            projection_capacity(),
        )
        .unwrap();

        assert_eq!(redactions, 0);
        assert_eq!(snapshot.json_docs.len(), 1);
        assert_eq!(snapshot.json_docs[0].key, "space-a-turn");
        assert!(snapshot.blobs.is_empty());
        assert_eq!(
            archive_memory_space_scope(&snapshot),
            Some(MemorySpaceProjectionScope {
                scope: source_scope,
                includes_private: false,
            })
        );
    }

    #[test]
    fn memory_space_import_identity_is_exact_and_full_store_snapshots_are_rejected() {
        let mut projection = snapshot_with_docs();
        let source_scope = scope("space:a", "subject:a");
        projection.json_docs.retain(|doc| doc.key == "space-a-turn");
        project_memory_space_snapshot(&mut projection, &source_scope, false, projection_capacity())
            .unwrap();
        ensure_archive_memory_space_identity(&projection, &source_scope).unwrap();
        assert!(
            ensure_archive_memory_space_identity(&projection, &scope("space:a", "subject:b"))
                .is_err()
        );

        let full_store = snapshot_with_docs();
        assert!(ensure_archive_memory_space_identity(&full_store, &source_scope).is_err());
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn memory_space_import_rejects_old_and_unknown_archive_schema_before_write() {
        let profile = ProfileId::DesktopMacosEmbeddedSdk;
        let target = StorePlatform::open_in_memory(
            StoreBackendConfig::in_memory(profile).expect("in-memory target config"),
        )
        .expect("open in-memory target");
        let source_scope = scope("space:a", "subject:a");
        let mut projection = snapshot_with_docs();
        projection.json_docs.retain(|doc| doc.key == "space-a-turn");
        project_memory_space_snapshot(&mut projection, &source_scope, false, projection_capacity())
            .unwrap();

        for rejected_schema in [
            "beetle_memory_store_schema_v3",
            "unknown_memory_store_schema_v999",
        ] {
            let mut rejected_projection = projection.clone();
            rejected_projection.schema_id = rejected_schema.to_string();
            rejected_projection.schema_manifest.schema_id = rejected_schema.to_string();
            let before = target
                .export_store_snapshot()
                .expect("snapshot before reject");
            let error = import_memory_space_from_platform_with_budget(
                &target,
                MemorySpaceImportRequest {
                    scope: source_scope.clone(),
                    expected_private_material_policy:
                        MemorySpacePrivateMaterialPolicy::ExcludePrivate,
                    archive: MemorySpaceArchive::from_snapshot(rejected_projection),
                },
                None,
            )
            .expect_err("old or unknown migration archive schema must fail closed");
            assert_eq!(error.stage(), "store_snapshot_import");
            let after = target
                .export_store_snapshot()
                .expect("snapshot after reject");
            assert_eq!(after.state_fingerprint(), before.state_fingerprint());
            assert_eq!(after.event_fingerprint(), before.event_fingerprint());
        }
    }
}
