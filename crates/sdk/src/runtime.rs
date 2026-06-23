use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::budget::{
    compile_runtime_budget, ProviderModelContextLimit, RuntimeBudgetInput, RuntimeBudgetReport,
    StaticPlatformManifest, TranscriptGovernanceBudget,
};
use bm_core::feature_gate::ProfileId;
use bm_core::llm::{LlmClient as CoreLlmClient, LlmHttpClient};
use bm_core::memory::{
    apply_long_term_memory_control_mutation, apply_long_term_memory_governance_policy_mutation,
    build_deferred_governance_queue_report, build_long_term_memory_facet_index_doc,
    build_memory_graph_recall_index_docs, build_temporal_memory_graph_from_evidence,
    build_temporal_memory_graph_from_parts, commit_canonical_turn_delta_with_transcript,
    compile_inhabited_subject_projection, default_agent_subject_id, default_memory_space_id,
    export_continuity_snapshot, filter_host_refs_for_transcript_view, govern_write_candidates,
    import_continuity_snapshot, inspect_intelligence_replay, inspect_memory_hygiene,
    inspect_working_recall, load_prompt_memory_context, memory_graph_backlink_key,
    normalize_private_garden_doc_path, plan_governed_shared_memory_in_space,
    plan_temporal_memory_graph_write, promote_task_experience_to_procedure, relationship_scope,
    rerank_recall_with_temporal_graph, run_long_term_memory_refresh,
    run_memory_retention_compaction, run_post_reply_memory_maintenance,
    run_private_garden_governance, CanonicalTurnDelta, CompactMemoryGraph,
    ContinuitySnapshotExportContext, ContinuitySnapshotImportContext, ContinuitySnapshotMode,
    ConversationKey, ConversationTranscriptStore, DeferredGovernanceJob,
    DeferredGovernanceJobStatus, DeferredGovernanceQueueReport, DerivedMemoryPlane,
    DerivedMemoryRef, DroppedProjectionCandidate, EvidenceBacklink, GovernedWriteDecision,
    GraphRecallCandidateScore, GraphRecallExpansionBudget, GraphRecallRerankReport, IngressKind,
    InhabitedSubjectProjection, InhabitedSubjectProjectionInput, LongTermMemoryControlAuditEvent,
    LongTermMemoryControlDetailRequest as CoreLongTermMemoryControlDetailRequest,
    LongTermMemoryControlListRequest as CoreLongTermMemoryControlListRequest,
    LongTermMemoryControlMutationRequest as CoreLongTermMemoryControlMutationRequest,
    LongTermMemoryControlRevision, LongTermMemoryControlStore, LongTermMemoryDraft,
    LongTermMemoryDraftAdmissionPolicy, LongTermMemoryEntry, LongTermMemoryExtractionState,
    LongTermMemoryKind, LongTermMemoryRefreshContext, LongTermMemoryRefreshOutcome,
    LongTermMemoryRefreshRequestOutcome, LongTermMemorySourceScope, LongTermMemoryStore,
    LongTermMemoryTombstone, MemoryCandidateTarget, MemoryEvidenceAuthority, MemoryGraphEdge,
    MemoryGraphEvidence, MemoryGraphNode, MemoryGraphNodeKind, MemoryGraphRecallIndexDoc,
    MemoryGraphWritePlan, MemoryHygieneContext, MemoryLongTermGovernancePolicy,
    MemoryLongTermMutation, MemoryPlaneGovernanceReport, MemoryWriteAuthority,
    MemoryWriteCandidate, MemoryWriteDomain, ParsedLongTermMemoryExtraction,
    PostReplyMemoryMaintenanceContext, PostReplyMemoryMaintenanceInput,
    PostTurnPrivateGardenReport, PostTurnSemanticGovernanceReport, PrivateGardenDoc,
    PrivateGardenDocRecord, PrivateGardenGovernanceContext, PrivateGardenGovernanceInput,
    PrivateGardenGovernanceManifestEntry, PrivateGardenGovernanceOutcome, PrivateGardenStore,
    ProceduralMemoryPromotionPolicy, ProceduralMemoryPromotionReport, ProjectionBudgetDecision,
    ProjectionFaithfulnessCheck, ProjectionPrivacyDecision, PromptMemoryContextParams,
    PromptParticipationPlan, PromptProjectionSource, PromptProjectionSurfaceRole,
    PromptRecallIntent, RecallCandidate, RecallSelectionReport, RedactedTranscriptSlice,
    SessionMessage, SessionMessageRecord, SessionStore, SharedFactWriteGovernanceContext,
    SharedMemoryWriteAction, SharedMemoryWriteOutcome, SharedMemoryWriteSource,
    SkillEvolutionReport, SubjectProjectionBoundaryProtocolReport, SubjectProjectionMountReport,
    SubjectProjectionReport, SubjectProjectionWorkIntegrityReport, SubjectRegistry,
    SubjectRelationshipGraph, SubjectScopedRuntime, TemporalMemoryGraphBuildReport,
    TemporalMemoryGraphGateReport, TranscriptAttrEnvelope, TranscriptAttrWriteRejection,
    TranscriptAttrWriteReport, TranscriptConversationAlias, TranscriptEvidenceRef,
    TranscriptLifecycleReport as CoreTranscriptLifecycleReport,
    TranscriptLifecycleRequest as CoreTranscriptLifecycleRequest, TranscriptLifecycleTransition,
    TranscriptRedactionReason, TranscriptRedactionReportItem,
    TranscriptRepairReport as CoreTranscriptRepairReport, TranscriptReplayView,
    WorkingRecallInspectionInput, LONG_TERM_CONTROL_AUDIT_NAMESPACE,
    LONG_TERM_CONTROL_REVISION_NAMESPACE, LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
    LONG_TERM_GOVERNANCE_POLICY_NAMESPACE, MEMORY_FACET_INDEX_NAMESPACE,
    PRIVATE_GARDEN_MAX_DOC_BYTES,
};
use bm_core::metrics::{OperatorReadinessReport, RuntimeMetricEvent, RuntimeMetricsReport};
use bm_core::platform::{Platform, SkillStorage};
use bm_core::resource::RuntimeResourceSnapshot;
use bm_core::runtime::{
    build_runtime_lifecycle_diagnosis, ensure_platform_soul_kernel_recovery,
    RuntimeLifecycleDisposition, RuntimeLifecycleEffect, RuntimeLifecycleEngine,
    RuntimeLifecycleEvent, RuntimeLifecycleEventKind, RuntimeLifecycleModeInput,
    RuntimeLifecycleOperation, RuntimeLifecycleReport, RuntimeLifecycleTrigger,
};
use bm_core::skills::{
    build_agent_skill_registry_snapshot, build_agent_tool_registry_report,
    build_projected_agent_skill_hints, get_disabled_skills, govern_agent_tool_usage_feedback,
    is_runtime_skill_name, list_agent_tool_experience_records, list_runtime_skill_records,
    plan_agent_tool_experience_record, plan_governed_runtime_skills, retrieve_agent_skill_hits,
    retrieve_runtime_skill_hits, select_agent_tool_hints, validate_agent_tool_registry_snapshot,
    AgentSkillDirConfig, AgentSkillProjectionAudit, AgentSkillRegistrySnapshot,
    AgentToolProjectionAudit, AgentToolRegistryReport, AgentToolRegistrySnapshot,
    RuntimeSkillRecord, RuntimeSkillStatus, RuntimeSkillStorageMutation, RuntimeSkillWriteAction,
};
use bm_store::{
    MemoryStoreEvent, MemoryStoreEventKind, StoreEventScope, StoreMutation, StoreMutationBatch,
    StoreMutationBatchReport, StorePlatform,
};
use serde::de::DeserializeOwned;

use crate::{
    resolve_memory_capabilities, Error, LLMRuntimeProjectionEnvelope, LlmClient,
    MemoryCapabilityCatalog, MemoryCapabilityPolicy, MemoryCloseReport, MemoryCloseRequest,
    MemoryDeferredGovernanceRunReport, MemoryDeferredGovernanceRunRequest, MemoryEvalRecallAtK,
    MemoryEvalRecallBenchmarkContext, MemoryEvalRecallCandidate,
    MemoryEvalRecallEvidenceRefIndexEntry, MemoryEvalRecallFacetStageDiagnostics,
    MemoryEvalRecallGoldRank, MemoryEvalRecallGraphDistanceToGold, MemoryEvalRecallMetrics,
    MemoryEvalRecallPrivacyReport, MemoryEvalRecallReport, MemoryEvalRecallRequest,
    MemoryEvalRecallStageDiagnostics, MemoryEvalRecallStageEvidenceRefs, MemoryExportReport,
    MemoryExportRequest, MemoryFacetRecallIndexReport, MemoryGovernancePolicyMutationReport,
    MemoryGraphRecallIndexReport, MemoryImportReport, MemoryImportRequest, MemoryInspectionReport,
    MemoryInspectionRequest, MemoryLongTermDetailReport, MemoryLongTermDetailRequest,
    MemoryLongTermListReport, MemoryLongTermListRequest, MemoryLongTermMutationReport,
    MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest, MemoryMaintenanceReport,
    MemoryMaintenanceRequest, MemoryOperationVisibility, MemoryPrivacyPolicy, MemoryProfile,
    MemoryProjectionAuditReport, MemoryProjectionPrivateGateAudit, MemoryProjectionReport,
    MemoryProjectionRequest, MemoryProjectionSectionAudit, MemoryProjectionSourceAudit,
    MemoryRecallReport, MemoryRecallRequest, MemoryRecoverReport, MemoryRecoverRequest,
    MemoryReplayReport, MemoryReplayRequest, MemoryRetentionCompactionReport,
    MemoryRetentionCompactionRequest, MemoryRuntimeSystemKind, MemorySpaceExportReport,
    MemorySpaceExportRequest, MemorySpaceImportReport, MemorySpaceImportRequest,
    MemorySpaceMigrateApplyReport, MemorySpaceMigrateApplyRequest, MemorySpaceMigratePreviewReport,
    MemorySpaceMigratePreviewRequest, MemoryTranscriptAttrWriteReport,
    MemoryTranscriptAttrWriteRequest, MemoryTranscriptCommitReport, MemoryTranscriptCommitRequest,
    MemoryTranscriptExportReport, MemoryTranscriptExportRequest, MemoryTranscriptLifecycleReport,
    MemoryTranscriptLifecycleRequest, MemoryTranscriptRepairReport, MemoryTranscriptRepairRequest,
    MemoryTranscriptReplayReport, MemoryTranscriptReplayRequest, MemoryTurnFinalizeReport,
    MemoryTurnFinalizeRequest, MemoryWriteReport, MemoryWriteRequest, MemoryWriteTransactionReport,
    PressureLevel, PrivateDisclosureIntegrityReport, Result, RuntimeDisclosureProtocolReport,
    RuntimeOperatorAction, RuntimeOperatorActionReport, RuntimeProjectionSourceBlock,
    RuntimeSkillDeleteRequest, RuntimeSkillDetailReport, RuntimeSkillDetailRequest,
    RuntimeSkillEditRequest, RuntimeSkillListReport, RuntimeSkillListRequest,
    RuntimeSkillMutationReport, RuntimeSkillReuseOutcome, RuntimeSkillSetEnabledRequest,
    RuntimeSkillSummary, RuntimeSkillWrite, RuntimeSkillWriteSource, SoulLifeProjectionReport,
    TemporalMemoryGraphMutationReport, TemporalMemoryGraphWriteRequest, WorkIntegrityReport,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryIdentity {
    pub agent_id: String,
    pub owner_id: String,
}

impl MemoryIdentity {
    pub fn new(agent_id: impl Into<String>, owner_id: impl Into<String>) -> Result<Self> {
        let agent_id = agent_id.into();
        let owner_id = owner_id.into();
        if agent_id.trim().is_empty() {
            return Err(Error::config(
                "memory_identity",
                "agent_id must not be empty",
            ));
        }
        if owner_id.trim().is_empty() {
            return Err(Error::config(
                "memory_identity",
                "owner_id must not be empty",
            ));
        }
        Ok(Self { agent_id, owner_id })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryScope {
    pub channel: String,
    pub chat_id: String,
    pub conversation_id: Option<String>,
}

impl MemoryScope {
    pub fn new(channel: impl Into<String>, chat_id: impl Into<String>) -> Result<Self> {
        let channel = channel.into();
        let chat_id = chat_id.into();
        if channel.trim().is_empty() {
            return Err(Error::config("memory_scope", "channel must not be empty"));
        }
        if chat_id.trim().is_empty() {
            return Err(Error::config("memory_scope", "chat_id must not be empty"));
        }
        Ok(Self {
            channel,
            chat_id,
            conversation_id: None,
        })
    }

    pub fn with_conversation_id(mut self, conversation_id: impl Into<String>) -> Result<Self> {
        let conversation_id = conversation_id.into();
        if conversation_id.trim().is_empty() {
            return Err(Error::config(
                "memory_scope",
                "conversation_id must not be empty",
            ));
        }
        self.conversation_id = Some(conversation_id.trim().to_string());
        Ok(self)
    }

    pub fn conversation_id_or_chat_id(&self) -> &str {
        self.conversation_id.as_deref().unwrap_or(&self.chat_id)
    }
}

pub trait MemoryClock: Send + Sync {
    fn now_secs(&self) -> u64;
}

pub struct SystemMemoryClock;

impl MemoryClock for SystemMemoryClock {
    fn now_secs(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
    }
}

pub trait MemoryAuditSink: Send + Sync {
    fn record(&self, event: MemoryAuditEvent);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryAuditEvent {
    pub operation: String,
    pub profile: ProfileId,
    pub identity: MemoryIdentity,
    pub scope: MemoryScope,
    pub memory_space_id: String,
    pub subject_id: String,
    pub conversation_id: Option<String>,
    pub allowed: bool,
    pub reason: String,
}

impl MemoryAuditEvent {
    pub fn for_runtime_operation(
        operation: impl Into<String>,
        profile: ProfileId,
        identity: MemoryIdentity,
        scope: MemoryScope,
        subject_id: impl Into<String>,
        allowed: bool,
        reason: impl Into<String>,
    ) -> Self {
        let memory_space_id = default_memory_space_id(&identity.owner_id);
        Self::for_scoped_runtime_operation(
            operation,
            profile,
            identity,
            scope,
            memory_space_id,
            subject_id,
            allowed,
            reason,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn for_scoped_runtime_operation(
        operation: impl Into<String>,
        profile: ProfileId,
        identity: MemoryIdentity,
        scope: MemoryScope,
        memory_space_id: impl Into<String>,
        subject_id: impl Into<String>,
        allowed: bool,
        reason: impl Into<String>,
    ) -> Self {
        let memory_space_id = memory_space_id.into();
        let subject_id = subject_id.into();
        let conversation_id = Some(scope.conversation_id_or_chat_id().to_string());
        Self {
            operation: operation.into(),
            profile,
            identity,
            scope,
            memory_space_id,
            subject_id,
            conversation_id,
            allowed,
            reason: reason.into(),
        }
    }
}

pub struct NoopMemoryAuditSink;

impl MemoryAuditSink for NoopMemoryAuditSink {
    fn record(&self, _event: MemoryAuditEvent) {}
}

pub struct MemoryRuntimeConfig {
    pub identity: MemoryIdentity,
    pub memory_space_id: String,
    pub subject_id: String,
    pub scoped_runtime: SubjectScopedRuntime,
    pub subject_registry: SubjectRegistry,
    pub subject_relationship_graph: SubjectRelationshipGraph,
    pub scope: MemoryScope,
    pub profile: ProfileId,
    pub(crate) platform: Arc<dyn Platform>,
    store_platform: Option<StorePlatform>,
    pub llm: Option<Arc<dyn LlmClient>>,
    pub clock: Arc<dyn MemoryClock>,
    pub capability_policy: MemoryCapabilityPolicy,
    pub privacy_policy: MemoryPrivacyPolicy,
    pub audit_sink: Arc<dyn MemoryAuditSink>,
    pub runtime_budget: RuntimeBudgetReport,
    pub agent_skill_registry: AgentSkillRegistrySnapshot,
}

pub struct MemoryRuntime {
    pub(crate) config: MemoryRuntimeConfig,
    pub(crate) capabilities: MemoryCapabilityCatalog,
    lifecycle: RuntimeLifecycleEngine,
    agent_tool_registries: Mutex<Vec<AgentToolRegistrySnapshot>>,
    last_conversation_id: Mutex<Option<String>>,
}

impl MemoryRuntime {
    pub fn builder() -> MemoryRuntimeBuilder {
        MemoryRuntimeBuilder::default()
    }

    pub fn config(&self) -> &MemoryRuntimeConfig {
        &self.config
    }

    pub fn identity(&self) -> &MemoryIdentity {
        &self.config.identity
    }

    pub fn memory_space_id(&self) -> &str {
        &self.config.memory_space_id
    }

    pub fn subject_id(&self) -> &str {
        &self.config.subject_id
    }

    pub fn scoped_runtime(&self) -> &SubjectScopedRuntime {
        &self.config.scoped_runtime
    }

    pub fn subject_registry(&self) -> &SubjectRegistry {
        &self.config.subject_registry
    }

    pub fn subject_relationship_graph(&self) -> &SubjectRelationshipGraph {
        &self.config.subject_relationship_graph
    }

    pub fn scope(&self) -> &MemoryScope {
        &self.config.scope
    }

    pub fn capabilities(&self) -> &MemoryCapabilityCatalog {
        &self.capabilities
    }

    pub fn runtime_budget(&self) -> &RuntimeBudgetReport {
        &self.config.runtime_budget
    }

    pub fn agent_tool_registries(&self) -> Vec<AgentToolRegistrySnapshot> {
        self.agent_tool_registries
            .lock()
            .expect("agent tool registry state poisoned")
            .clone()
    }

    pub fn list_long_term_memory(
        &self,
        request: MemoryLongTermListRequest,
    ) -> Result<MemoryLongTermListReport> {
        self.ensure_visible(
            "long_term_control.inspect",
            self.capabilities.long_term_control_inspect,
        )?;
        let store = self.config.platform.long_term_memory_store();
        let control_store = self.config.platform.long_term_memory_control_store();
        bm_core::memory::list_long_term_memory_control_page(
            store.as_ref(),
            control_store.as_ref(),
            CoreLongTermMemoryControlListRequest {
                query: request.query,
                cursor: request.cursor,
                limit: request.limit,
                view: request.view,
            },
        )
    }

    pub fn get_long_term_memory(
        &self,
        request: MemoryLongTermDetailRequest,
    ) -> Result<MemoryLongTermDetailReport> {
        self.ensure_visible(
            "long_term_control.inspect",
            self.capabilities.long_term_control_inspect,
        )?;
        let store = self.config.platform.long_term_memory_store();
        let control_store = self.config.platform.long_term_memory_control_store();
        bm_core::memory::get_long_term_memory_control_detail(
            store.as_ref(),
            control_store.as_ref(),
            CoreLongTermMemoryControlDetailRequest {
                target: request.target,
                view: request.view,
            },
        )
    }

    pub fn mutate_long_term_memory(
        &self,
        request: MemoryLongTermMutationRequest,
    ) -> Result<MemoryLongTermMutationReport> {
        let visibility = self.long_term_mutation_visibility(&request.operation);
        self.ensure_visible("long_term_control.mutation", visibility)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::OperatorAction,
            RuntimeLifecycleTrigger::OperatorRequested,
            request.mode_input,
        );
        let core_request = CoreLongTermMemoryControlMutationRequest {
            operation: request.operation,
            reason: request.reason,
            dry_run: request.dry_run,
            actor_subject_id: Some(self.config.scoped_runtime.actor_subject_id.clone()),
            memory_space_id: Some(self.config.memory_space_id.clone()),
            now_secs: self.config.clock.now_secs(),
        };
        let (core_report, mutations) = self.plan_long_term_control_mutation(core_request)?;
        let changed = !core_report.dry_run && !core_report.affected_records.is_empty();
        let lifecycle_summary = format!(
            "{} accepted={} affected={}",
            core_report.operation,
            core_report.accepted,
            core_report.affected_records.len()
        );
        let lifecycle_payload = [
            (
                "action",
                RuntimeOperatorAction::LongTermMemoryControl
                    .as_str()
                    .to_string(),
            ),
            ("operation", core_report.operation.to_string()),
            ("accepted", core_report.accepted.to_string()),
            (
                "affected_records",
                core_report.affected_records.len().to_string(),
            ),
        ];
        let lifecycle_report = if mutations.is_empty() {
            self.finish_lifecycle_success_with_payload(
                lifecycle,
                RuntimeLifecycleEventKind::OperatorAction,
                RuntimeLifecycleEffect::RecordOperatorAction,
                changed,
                lifecycle_summary,
                &lifecycle_payload,
            )?
        } else {
            self.commit_memory_write_transaction(
                lifecycle,
                "long_term_control.mutation",
                RuntimeLifecycleEventKind::OperatorAction,
                RuntimeLifecycleEffect::RecordOperatorAction,
                changed,
                lifecycle_summary,
                &lifecycle_payload,
                mutations,
                core_report.affected_records.len(),
            )?
            .0
        };
        Ok(memory_long_term_mutation_report_from_core(
            core_report,
            lifecycle_report,
        ))
    }

    pub fn mutate_memory_governance_policy(
        &self,
        request: MemoryLongTermPolicyRequest,
    ) -> Result<MemoryGovernancePolicyMutationReport> {
        self.ensure_visible(
            "long_term_control.policy",
            self.capabilities.long_term_control_policy,
        )?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::OperatorAction,
            RuntimeLifecycleTrigger::OperatorRequested,
            request.mode_input,
        );
        let (core_report, mutations) = self.plan_memory_governance_policy_mutation(
            request.operation,
            request.reason,
            request.dry_run,
            self.config.clock.now_secs(),
        )?;
        let changed = !core_report.dry_run && core_report.accepted;
        let lifecycle_summary = format!(
            "{} accepted={} policy={}",
            core_report.operation,
            core_report.accepted,
            core_report.policy_id.as_deref().unwrap_or("")
        );
        let lifecycle_payload = [
            (
                "action",
                RuntimeOperatorAction::LongTermMemoryPolicyControl
                    .as_str()
                    .to_string(),
            ),
            ("operation", core_report.operation.to_string()),
            ("accepted", core_report.accepted.to_string()),
            (
                "policy_id",
                core_report.policy_id.clone().unwrap_or_default(),
            ),
        ];
        let lifecycle_report = if mutations.is_empty() {
            self.finish_lifecycle_success_with_payload(
                lifecycle,
                RuntimeLifecycleEventKind::OperatorAction,
                RuntimeLifecycleEffect::RecordOperatorAction,
                changed,
                lifecycle_summary,
                &lifecycle_payload,
            )?
        } else {
            self.commit_memory_write_transaction(
                lifecycle,
                "long_term_control.policy",
                RuntimeLifecycleEventKind::OperatorAction,
                RuntimeLifecycleEffect::RecordOperatorAction,
                changed,
                lifecycle_summary,
                &lifecycle_payload,
                mutations,
                usize::from(changed),
            )?
            .0
        };
        Ok(memory_governance_policy_report_from_core(
            core_report,
            lifecycle_report,
        ))
    }

    pub fn upsert_agent_tool_registry(
        &self,
        registry: AgentToolRegistrySnapshot,
    ) -> Result<AgentToolRegistryReport> {
        validate_agent_tool_registry_snapshot(self.config.profile, &registry)
            .map_err(|error| Error::config(error.stage(), error.to_string()))?;
        {
            let mut registries = self
                .agent_tool_registries
                .lock()
                .expect("agent tool registry state poisoned");
            if let Some(existing) = registries
                .iter_mut()
                .find(|existing| existing.registry_id == registry.registry_id)
            {
                *existing = registry;
            } else {
                registries.push(registry);
            }
        }
        self.agent_tool_registry_report()
    }

    pub fn delete_agent_tool_registry(&self, registry_id: &str) -> Result<AgentToolRegistryReport> {
        self.agent_tool_registries
            .lock()
            .expect("agent tool registry state poisoned")
            .retain(|registry| registry.registry_id != registry_id);
        self.agent_tool_registry_report()
    }

    pub fn agent_tool_registry_report(&self) -> Result<AgentToolRegistryReport> {
        let registries = self.agent_tool_registries();
        let skill_storage = self.config.platform.skill_storage();
        Ok(build_agent_tool_registry_report(
            self.config.profile,
            &registries,
            &list_agent_tool_experience_records(skill_storage.as_ref()),
        ))
    }

    pub fn runtime_metrics_report_from_events(
        &self,
        events: &[MemoryStoreEvent],
    ) -> RuntimeMetricsReport {
        bm_core::metrics::build_runtime_metrics_report(
            events.iter().map(|event| RuntimeMetricEvent {
                kind_name: event.kind_name.clone(),
                timestamp_unix_secs: event.timestamp_unix_secs,
                payload: event.payload.clone(),
            }),
            self.config.runtime_budget.report_id.clone(),
        )
    }

    pub fn operator_readiness_report(&self) -> OperatorReadinessReport {
        OperatorReadinessReport::sdk_ready(self.config.runtime_budget.unavailable_reasons.clone())
    }

    pub fn retention_quota_report(&self) -> bm_core::budget::RuntimeRetentionQuotaReport {
        self.config.runtime_budget.retention_quota_report()
    }

    fn remember_conversation_id_from_delta(&self, turn: &CanonicalTurnDelta) -> Result<()> {
        let conversation_id = turn
            .conversation
            .conversation_id
            .as_deref()
            .unwrap_or(turn.conversation.chat_id.as_str())
            .trim();
        if conversation_id.is_empty() {
            return Ok(());
        }
        let alias = TranscriptConversationAlias::new(
            self.config.memory_space_id.clone(),
            turn.conversation.channel.clone(),
            turn.conversation.chat_id.clone(),
            conversation_id.to_string(),
            self.config.clock.now_secs(),
        )?;
        self.config
            .platform
            .conversation_transcript_store()
            .remember_conversation_alias(&alias)?;
        *self
            .last_conversation_id
            .lock()
            .expect("last conversation id state poisoned") = Some(conversation_id.to_string());
        Ok(())
    }

    fn active_transcript_key(
        &self,
        transcript_store: &dyn ConversationTranscriptStore,
    ) -> Result<ConversationKey> {
        let last_conversation_id = self
            .last_conversation_id
            .lock()
            .expect("last conversation id state poisoned")
            .clone();
        let conversation_id = if let Some(conversation_id) = last_conversation_id {
            conversation_id
        } else if let Some(conversation_id) = self.config.scope.conversation_id.clone() {
            conversation_id
        } else if let Some(conversation_id) = transcript_store.resolve_conversation_alias(
            &self.config.memory_space_id,
            &self.config.scope.channel,
            &self.config.scope.chat_id,
        )? {
            conversation_id
        } else {
            self.config.scope.chat_id.clone()
        };
        ConversationKey::new(
            self.config.memory_space_id.clone(),
            self.config.scope.channel.clone(),
            conversation_id,
        )
    }

    fn transcript_backed_session_store(
        &self,
        fallback: Arc<dyn SessionStore>,
        view: TranscriptReplayView,
    ) -> Arc<dyn SessionStore> {
        let transcript_store = self.config.platform.conversation_transcript_store();
        match self.active_transcript_key(transcript_store.as_ref()) {
            Ok(key) => Arc::new(TranscriptBackedSessionStore {
                fallback,
                transcript_store,
                key,
                view,
            }),
            Err(error) => Arc::new(TranscriptKeyUnavailableSessionStore {
                fallback,
                reason: error.to_string(),
            }),
        }
    }

    pub fn write(&self, request: MemoryWriteRequest) -> Result<MemoryWriteReport> {
        self.ensure_visible("write", self.capabilities.write)?;
        let now_secs = self.config.clock.now_secs();
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Maintain,
            RuntimeLifecycleTrigger::SdkCall,
            RuntimeLifecycleModeInput::default(),
        );
        let report = match request {
            MemoryWriteRequest::Procedural { writes, source } => {
                if runtime_skill_write_source_requires_promotion(source) {
                    let rejected = writes
                        .iter()
                        .map(|write| {
                            if write.name.trim().is_empty() {
                                sdk_runtime_skill_name(&write.topic)
                            } else {
                                write.name.trim().to_string()
                            }
                        })
                        .collect::<Vec<_>>();
                    let procedural_evolution = Some(SkillEvolutionReport {
                        rejected,
                        reasons: vec![
                            "runtime_learned_procedural_write_requires_promotion".to_string()
                        ],
                        ..SkillEvolutionReport::default()
                    });
                    let lifecycle_report = self.finish_lifecycle_success(
                        lifecycle,
                        RuntimeLifecycleEventKind::RuntimeLifecycle,
                        RuntimeLifecycleEffect::Noop,
                        false,
                        "runtime_learned_procedural_write_requires_promotion",
                    )?;
                    MemoryWriteReport {
                        accepted: false,
                        changed: 0,
                        operation: "write.procedural",
                        reason: "runtime_learned_procedural_write_requires_promotion".to_string(),
                        lifecycle_report,
                        transaction: None,
                        semantic_governance: None,
                        shared_fact_governance: None,
                        procedural_evolution,
                        procedural_promotions: Vec::new(),
                        agent_tool_experience: None,
                    }
                } else {
                    let storage = self.config.platform.skill_storage();
                    let writes = normalize_runtime_skill_write_names(writes);
                    let plan = plan_governed_runtime_skills(storage.as_ref(), &writes, source)?;
                    let outcome = plan.outcome;
                    let procedural_evolution = Some(
                        build_skill_evolution_report_from_write_outcome(&writes, &outcome),
                    );
                    let changed = outcome.changed;
                    let mutations =
                        runtime_skill_storage_mutations_to_store_mutations(&plan.mutations);
                    let (lifecycle_report, transaction) = self.commit_memory_write_transaction(
                        lifecycle,
                        "write.procedural",
                        RuntimeLifecycleEventKind::RuntimeLifecycle,
                        RuntimeLifecycleEffect::RunMaintenance,
                        changed > 0,
                        "write.procedural",
                        &[("changed_count", changed.to_string())],
                        mutations,
                        changed,
                    )?;
                    MemoryWriteReport {
                        accepted: outcome.accepted > 0 || outcome.rejected == 0,
                        changed,
                        operation: "write.procedural",
                        reason: format!(
                            "submitted={}, accepted={}, rejected={}",
                            outcome.submitted, outcome.accepted, outcome.rejected
                        ),
                        lifecycle_report,
                        transaction: Some(transaction),
                        semantic_governance: None,
                        shared_fact_governance: None,
                        procedural_evolution,
                        procedural_promotions: Vec::new(),
                        agent_tool_experience: None,
                    }
                }
            }
            MemoryWriteRequest::ProceduralPromotions { promotions, source } => {
                let promotion_reports = promotions
                    .into_iter()
                    .map(|input| {
                        promote_task_experience_to_procedure(
                            input,
                            ProceduralMemoryPromotionPolicy::default(),
                        )
                    })
                    .collect::<Vec<_>>();
                let writes = normalize_runtime_skill_write_names(
                    promotion_reports
                        .iter()
                        .filter_map(|report| {
                            runtime_skill_write_from_promotion_report(
                                report,
                                Some(&self.config.scope.chat_id),
                                now_secs,
                            )
                        })
                        .collect::<Vec<_>>(),
                );
                let storage = self.config.platform.skill_storage();
                let transaction_plan = if writes.is_empty() {
                    None
                } else {
                    Some(plan_governed_runtime_skills(
                        storage.as_ref(),
                        &writes,
                        source,
                    )?)
                };
                let outcome = transaction_plan
                    .as_ref()
                    .map(|plan| plan.outcome.clone())
                    .unwrap_or_else(|| crate::RuntimeSkillWriteOutcome {
                        source,
                        submitted: promotion_reports.len(),
                        rejected: promotion_reports
                            .iter()
                            .filter(|report| !report.promoted)
                            .count(),
                        ..crate::RuntimeSkillWriteOutcome::default()
                    });
                let procedural_evolution = Some(merge_promotion_and_write_evolution(
                    &promotion_reports,
                    &writes,
                    &outcome,
                ));
                let blocked_reasons = promotion_reports
                    .iter()
                    .flat_map(|report| report.blocked_reasons.iter().cloned())
                    .collect::<Vec<_>>();
                let changed = outcome.changed;
                let (lifecycle_report, transaction) = if let Some(plan) = transaction_plan {
                    let mutations =
                        runtime_skill_storage_mutations_to_store_mutations(&plan.mutations);
                    let (lifecycle_report, transaction) = self.commit_memory_write_transaction(
                        lifecycle,
                        "write.procedural_promotions",
                        RuntimeLifecycleEventKind::RuntimeLifecycle,
                        RuntimeLifecycleEffect::RunMaintenance,
                        changed > 0,
                        "write.procedural_promotions",
                        &[("changed_count", changed.to_string())],
                        mutations,
                        changed,
                    )?;
                    (lifecycle_report, Some(transaction))
                } else {
                    (
                        self.finish_lifecycle_success_with_payload(
                            lifecycle,
                            RuntimeLifecycleEventKind::RuntimeLifecycle,
                            RuntimeLifecycleEffect::RunMaintenance,
                            changed > 0,
                            "write.procedural_promotions",
                            &[("changed_count", changed.to_string())],
                        )?,
                        None,
                    )
                };
                MemoryWriteReport {
                    accepted: !writes.is_empty()
                        && blocked_reasons.is_empty()
                        && outcome.accepted == writes.len(),
                    changed,
                    operation: "write.procedural_promotions",
                    reason: format!(
                        "submitted={}, promoted={}, accepted={}, rejected={}, blocked={}",
                        promotion_reports.len(),
                        promotion_reports
                            .iter()
                            .filter(|report| report.promoted)
                            .count(),
                        outcome.accepted,
                        outcome.rejected,
                        blocked_reasons.join("|")
                    ),
                    lifecycle_report,
                    transaction,
                    semantic_governance: None,
                    shared_fact_governance: None,
                    procedural_evolution,
                    procedural_promotions: promotion_reports,
                    agent_tool_experience: None,
                }
            }
            MemoryWriteRequest::LongTermExtraction { extraction } => {
                let (upserts, suppressed_long_term_policy_ids, suppressed_draft_count) =
                    self.filter_long_term_drafts_by_policy(extraction.upserts.clone(), now_secs)?;
                let extraction = ParsedLongTermMemoryExtraction {
                    upserts,
                    deletes: extraction.deletes,
                    skill_writes: extraction.skill_writes,
                };
                let extraction_plan =
                    self.plan_long_term_extraction_transaction(&extraction, now_secs)?;
                let changed = extraction_plan.changed;
                let shared_fact_governance = extraction_plan.shared_fact_governance.clone();
                let procedural_evolution = extraction_plan.procedural_evolution.clone();
                let (lifecycle_report, transaction) = self.commit_memory_write_transaction(
                    lifecycle,
                    "write.long_term_extraction",
                    RuntimeLifecycleEventKind::RuntimeLifecycle,
                    RuntimeLifecycleEffect::RequestLongTermRefresh,
                    changed > 0,
                    "write.long_term_extraction",
                    &[("changed_count", changed.to_string())],
                    extraction_plan.mutations,
                    changed,
                )?;
                let policy_reason = if suppressed_draft_count > 0 {
                    format!(
                        "; suppressed_by_long_term_policy={}, policy_ids={}",
                        suppressed_draft_count,
                        suppressed_long_term_policy_ids.join("|")
                    )
                } else {
                    String::new()
                };
                MemoryWriteReport {
                    accepted: true,
                    changed,
                    operation: "write.long_term_extraction",
                    reason: format!("long_term_extraction_applied{policy_reason}"),
                    lifecycle_report,
                    transaction: Some(transaction),
                    semantic_governance: None,
                    shared_fact_governance,
                    procedural_evolution,
                    procedural_promotions: Vec::new(),
                    agent_tool_experience: None,
                }
            }
            MemoryWriteRequest::Candidates { candidates } => {
                self.write_candidates_transactional(candidates, lifecycle, now_secs)?
            }
            MemoryWriteRequest::AgentToolUsageFeedback { feedback } => {
                let storage = self.config.platform.skill_storage();
                let agent_tool_registries = self.agent_tool_registries();
                let governance =
                    govern_agent_tool_usage_feedback(&agent_tool_registries, &feedback, now_secs);
                let (changed, lifecycle_report, transaction) = if let Some(experience) =
                    governance.experience.as_ref()
                {
                    let mutations = if let Some(mutation) =
                        plan_agent_tool_experience_record(storage.as_ref(), experience)?
                    {
                        runtime_skill_storage_mutations_to_store_mutations(&[mutation])
                    } else {
                        Vec::new()
                    };
                    let changed = usize::from(!mutations.is_empty());
                    let (lifecycle_report, transaction) = self.commit_memory_write_transaction(
                        lifecycle,
                        "write.agent_tool_usage_feedback",
                        RuntimeLifecycleEventKind::RuntimeLifecycle,
                        RuntimeLifecycleEffect::RunMaintenance,
                        changed > 0,
                        "write.agent_tool_usage_feedback",
                        &[("changed_count", changed.to_string())],
                        mutations,
                        changed,
                    )?;
                    (changed, lifecycle_report, Some(transaction))
                } else {
                    let lifecycle_report = self.finish_lifecycle_success_with_payload(
                        lifecycle,
                        RuntimeLifecycleEventKind::RuntimeLifecycle,
                        RuntimeLifecycleEffect::RunMaintenance,
                        false,
                        "write.agent_tool_usage_feedback",
                        &[("changed_count", "0".to_string())],
                    )?;
                    (0, lifecycle_report, None)
                };
                MemoryWriteReport {
                    accepted: governance.accepted,
                    changed,
                    operation: "write.agent_tool_usage_feedback",
                    reason: governance.reason.clone(),
                    lifecycle_report,
                    transaction,
                    semantic_governance: None,
                    shared_fact_governance: None,
                    procedural_evolution: None,
                    procedural_promotions: Vec::new(),
                    agent_tool_experience: Some(governance),
                }
            }
        };
        self.audit("write", true, &report.reason);
        Ok(report)
    }

    fn write_candidates_transactional(
        &self,
        candidates: Vec<MemoryWriteCandidate>,
        lifecycle: RuntimeLifecycleReport,
        now_secs: u64,
    ) -> Result<MemoryWriteReport> {
        let store_platform = self.config.store_platform.as_ref().ok_or_else(|| {
            Error::config(
                "memory_write_transaction_unavailable",
                "transactional memory writes require StorePlatform-backed runtime",
            )
        })?;
        let transaction_id = format!("memory_write_txn_{}", lifecycle.event_id);
        let operation = "write.candidates";
        let semantic_governance = govern_write_candidates(&candidates);
        let accepted_candidates = candidates
            .iter()
            .filter(|candidate| {
                candidate_semantically_accepted(candidate, &semantic_governance.plane_reports)
            })
            .collect::<Vec<_>>();
        let accepted_draft_pairs = accepted_candidates
            .iter()
            .filter_map(|candidate| {
                let target = candidate.governed_target().unwrap_or(&candidate.target);
                candidate
                    .to_long_term_draft_for_target(target, &self.config.scope.chat_id, now_secs)
                    .map(|draft| (*candidate, draft))
            })
            .collect::<Vec<_>>();
        let (accepted_draft_pairs, suppressed_long_term_policy_ids, suppressed_draft_count) =
            self.filter_long_term_draft_pairs_by_policy(accepted_draft_pairs, now_secs)?;
        let accepted_drafts = accepted_draft_pairs
            .iter()
            .map(|(_, draft)| draft.clone())
            .collect::<Vec<_>>();
        let accepted_skill_pairs = accepted_candidates
            .iter()
            .filter_map(|candidate| {
                let target = candidate.governed_target().unwrap_or(&candidate.target);
                candidate
                    .to_runtime_skill_write_for_target(target, &self.config.scope.chat_id, now_secs)
                    .map(|write| (*candidate, write))
            })
            .collect::<Vec<_>>();
        let accepted_skill_writes = accepted_skill_pairs
            .iter()
            .map(|(_, write)| write.clone())
            .collect::<Vec<_>>();
        let accepted_skill_writes = normalize_runtime_skill_write_names(accepted_skill_writes);
        let accepted_normalized_skill_pairs = accepted_skill_pairs
            .iter()
            .zip(accepted_skill_writes.iter())
            .map(|((candidate, _), write)| (*candidate, write.clone()))
            .collect::<Vec<_>>();

        let mut mutations = Vec::new();
        let shared_fact_governance = if accepted_drafts.is_empty() {
            None
        } else {
            let store = self.config.platform.long_term_memory_store();
            let mut context = SharedFactWriteGovernanceContext::new(
                self.config.memory_space_id.clone(),
                self.config.scoped_runtime.mounted_subject_id.clone(),
                self.config.scoped_runtime.actor_subject_id.clone(),
                SharedMemoryWriteSource::ManualTool,
            );
            context.relationship_id = self
                .config
                .scoped_runtime
                .relationship_scope
                .as_ref()
                .map(|scope| scope.relationship_id.clone());
            let plan = plan_governed_shared_memory_in_space(
                store.as_ref(),
                &accepted_drafts,
                now_secs,
                context,
            )?;
            for entry in &plan.accepted_entries {
                mutations.push(StoreMutation::PutJson {
                    namespace: "long_term".to_string(),
                    key: entry.id.clone(),
                    value: serde_json::to_value(entry).map_err(|error| {
                        Error::config("memory_write_transaction_plan", error.to_string())
                    })?,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "long_term".to_string(),
                    record_key: entry.id.clone(),
                });
            }
            mutations
                .extend(self.plan_long_term_facet_index_upsert_mutations(&plan.accepted_entries)?);
            Some(plan.outcome)
        };
        let long_term_changed = shared_fact_governance
            .as_ref()
            .map(|outcome| outcome.changed)
            .unwrap_or(0);
        let governed_draft_pairs = shared_fact_governance
            .as_ref()
            .map(|outcome| {
                accepted_draft_pairs
                    .iter()
                    .zip(outcome.reports.iter())
                    .filter(|&((_candidate, _draft), report)| {
                        matches!(report.action, SharedMemoryWriteAction::Accepted)
                    })
                    .map(|((candidate, draft), _report)| (*candidate, draft.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let (skill_changed, procedural_evolution, governed_skill_pairs) =
            if accepted_skill_writes.is_empty() {
                (0, None, Vec::new())
            } else {
                let storage = self.config.platform.skill_storage();
                let plan = plan_governed_runtime_skills(
                    storage.as_ref(),
                    &accepted_skill_writes,
                    RuntimeSkillWriteSource::Manual,
                )?;
                for mutation in &plan.mutations {
                    match mutation {
                        RuntimeSkillStorageMutation::Upsert { name, content } => {
                            mutations.push(StoreMutation::PutBlob {
                                namespace: "skills".to_string(),
                                key: name.clone(),
                                value: content.clone(),
                                event_kind: MemoryStoreEventKind::MemoryWrite,
                                plane: "skills".to_string(),
                                record_key: name.clone(),
                            });
                        }
                        RuntimeSkillStorageMutation::Delete { name } => {
                            mutations.push(StoreMutation::DeleteBlob {
                                namespace: "skills".to_string(),
                                key: name.clone(),
                                event_kind: MemoryStoreEventKind::MemoryDelete,
                                plane: "skills".to_string(),
                                record_key: name.clone(),
                            });
                        }
                    }
                }
                let governed_skill_pairs = accepted_normalized_skill_pairs
                    .iter()
                    .zip(plan.outcome.reports.iter())
                    .filter(|&((_candidate, _write), report)| {
                        matches!(report.action, RuntimeSkillWriteAction::Accepted)
                    })
                    .map(|((candidate, write), _report)| (*candidate, write.clone()))
                    .collect::<Vec<_>>();
                let procedural_evolution = build_skill_evolution_report_from_write_outcome(
                    &accepted_skill_writes,
                    &plan.outcome,
                );
                (
                    plan.outcome.changed,
                    Some(procedural_evolution),
                    governed_skill_pairs,
                )
            };

        mutations.extend(plan_candidate_derived_memory_ref_mutations(
            &self.config.subject_id,
            &governed_draft_pairs,
            &governed_skill_pairs,
            now_secs,
        )?);
        mutations.extend(plan_soul_handoff_derived_memory_ref_mutations(
            &self.config.subject_id,
            &candidates,
            now_secs,
        )?);

        let changed = long_term_changed + skill_changed;
        let lifecycle_report = lifecycle.finish_success(
            self.config.clock.now_secs(),
            changed > 0,
            "write.candidates",
        );
        mutations.push(StoreMutation::AppendEvent {
            event: self.planned_lifecycle_store_event(
                &transaction_id,
                operation,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::RunMaintenance,
                &lifecycle_report,
                &[("changed_count", changed.to_string())],
            )?,
        });

        let planned_mutations = mutations.len();
        let store_report = store_platform.commit_mutation_batch(StoreMutationBatch {
            transaction_id: transaction_id.clone(),
            operation: operation.to_string(),
            scope: self.memory_write_transaction_scope(),
            mutations,
        })?;
        let transaction =
            memory_write_transaction_report(operation, planned_mutations, changed, store_report);
        let policy_reason = if suppressed_draft_count > 0 {
            format!(
                ", suppressed_by_long_term_policy={}, policy_ids={}",
                suppressed_draft_count,
                suppressed_long_term_policy_ids.join("|")
            )
        } else {
            String::new()
        };

        Ok(MemoryWriteReport {
            accepted: semantic_governance.accepted_count > 0
                && semantic_governance.rejected_count == 0,
            changed,
            operation,
            reason: format!(
                "submitted={}, accepted={}, rejected={}, deferred={}{}",
                semantic_governance.proposal_count,
                semantic_governance.accepted_count,
                semantic_governance.rejected_count,
                semantic_governance.deferred_count,
                policy_reason
            ),
            lifecycle_report,
            transaction: Some(transaction),
            semantic_governance: Some(semantic_governance),
            shared_fact_governance,
            procedural_evolution,
            procedural_promotions: Vec::new(),
            agent_tool_experience: None,
        })
    }

    fn memory_write_transaction_scope(&self) -> StoreEventScope {
        StoreEventScope::new(
            self.config.identity.agent_id.clone(),
            self.config.identity.owner_id.clone(),
            self.config.scope.channel.clone(),
            self.config.scope.chat_id.clone(),
        )
        .with_memory_space(self.config.memory_space_id.clone())
        .with_subject(self.config.subject_id.clone())
        .with_conversation(self.config.scope.chat_id.clone())
    }

    fn long_term_facet_index_subject_ids(&self) -> Vec<String> {
        let mut subject_ids = BTreeSet::new();
        for subject_id in [
            self.config.subject_id.as_str(),
            self.config.scoped_runtime.mounted_subject_id.as_str(),
            self.config.scoped_runtime.actor_subject_id.as_str(),
        ] {
            let subject_id = subject_id.trim();
            if !subject_id.is_empty() {
                subject_ids.insert(subject_id.to_string());
            }
        }
        subject_ids.into_iter().collect()
    }

    fn long_term_facet_index_upsert_mutation(
        &self,
        entry: &LongTermMemoryEntry,
    ) -> Result<StoreMutation> {
        let facet_index_revision = entry.source_revision.max(1);
        let doc = build_long_term_memory_facet_index_doc(
            entry,
            self.config.memory_space_id.clone(),
            self.long_term_facet_index_subject_ids(),
            facet_index_revision,
        );
        let key = doc.store_key();
        Ok(StoreMutation::PutJson {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: key.clone(),
            value: serde_json::to_value(&doc)
                .map_err(|error| Error::config("memory_facet_index_plan", error.to_string()))?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            record_key: key,
        })
    }

    fn long_term_facet_index_delete_mutation(
        &self,
        owner_record_id: &str,
    ) -> Option<StoreMutation> {
        let owner_record_id = owner_record_id.trim();
        if owner_record_id.is_empty() {
            return None;
        }
        let key = format!("facet-index:{owner_record_id}");
        Some(StoreMutation::DeleteJson {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: key.clone(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            record_key: key,
        })
    }

    fn plan_long_term_facet_index_upsert_mutations(
        &self,
        entries: &[LongTermMemoryEntry],
    ) -> Result<Vec<StoreMutation>> {
        entries
            .iter()
            .map(|entry| self.long_term_facet_index_upsert_mutation(entry))
            .collect()
    }

    fn plan_long_term_facet_index_delete_mutations(
        &self,
        owner_record_ids: &[String],
    ) -> Vec<StoreMutation> {
        owner_record_ids
            .iter()
            .filter_map(|owner_record_id| {
                self.long_term_facet_index_delete_mutation(owner_record_id)
            })
            .collect()
    }

    fn plan_long_term_facet_index_mutations_for_store_mutations(
        &self,
        long_term_mutations: &[StoreMutation],
    ) -> Result<Vec<StoreMutation>> {
        let mut facet_mutations = Vec::new();
        for mutation in long_term_mutations {
            match mutation {
                StoreMutation::PutJson {
                    namespace, value, ..
                } if namespace == "long_term" => {
                    let entry = serde_json::from_value::<LongTermMemoryEntry>(value.clone())
                        .map_err(|error| {
                            Error::config("memory_facet_index_plan", error.to_string())
                        })?;
                    facet_mutations.push(self.long_term_facet_index_upsert_mutation(&entry)?);
                }
                StoreMutation::DeleteJson { namespace, key, .. } if namespace == "long_term" => {
                    if let Some(mutation) = self.long_term_facet_index_delete_mutation(key) {
                        facet_mutations.push(mutation);
                    }
                }
                _ => {}
            }
        }
        Ok(facet_mutations)
    }

    fn ensure_transcript_lifecycle_has_facet_impact_or_fails_closed(
        &self,
        key: &ConversationKey,
        turn_id: Option<&str>,
        transition: TranscriptLifecycleTransition,
    ) -> Result<()> {
        if !matches!(
            transition,
            TranscriptLifecycleTransition::Mask | TranscriptLifecycleTransition::DeleteRaw
        ) {
            return Ok(());
        }
        let Some(store_platform) = self.config.store_platform.as_ref() else {
            return Ok(());
        };
        let facet_docs = store_platform.read_json_namespace(MEMORY_FACET_INDEX_NAMESPACE)?;
        if !facet_docs
            .iter()
            .any(|doc| facet_doc_references_transcript_source(&doc.value, key, turn_id))
        {
            return Ok(());
        }
        Err(Error::config(
            "transcript_lifecycle_facet_preflight",
            "transcript_source_ref_redacted_facet_update_not_supported",
        ))
    }

    fn commit_memory_write_transaction(
        &self,
        lifecycle: RuntimeLifecycleReport,
        operation: &'static str,
        lifecycle_kind: RuntimeLifecycleEventKind,
        lifecycle_effect: RuntimeLifecycleEffect,
        changed: bool,
        summary: impl Into<String>,
        extra_payload: &[(&str, String)],
        mut mutations: Vec<StoreMutation>,
        changed_count: usize,
    ) -> Result<(RuntimeLifecycleReport, MemoryWriteTransactionReport)> {
        let store_platform = self.config.store_platform.as_ref().ok_or_else(|| {
            Error::config(
                "memory_write_transaction_unavailable",
                "transactional memory writes require StorePlatform-backed runtime",
            )
        })?;
        let transaction_id = format!("memory_write_txn_{}", lifecycle.event_id);
        let lifecycle_report =
            lifecycle.finish_success(self.config.clock.now_secs(), changed, summary);
        mutations.push(StoreMutation::AppendEvent {
            event: self.planned_lifecycle_store_event(
                &transaction_id,
                operation,
                lifecycle_kind,
                lifecycle_effect,
                &lifecycle_report,
                extra_payload,
            )?,
        });
        let planned_mutations = mutations.len();
        let store_report = store_platform.commit_mutation_batch(StoreMutationBatch {
            transaction_id,
            operation: operation.to_string(),
            scope: self.memory_write_transaction_scope(),
            mutations,
        })?;
        Ok((
            lifecycle_report,
            memory_write_transaction_report(
                operation,
                planned_mutations,
                changed_count,
                store_report,
            ),
        ))
    }

    fn commit_memory_mutation_batch(
        &self,
        operation: &str,
        mutations: Vec<StoreMutation>,
    ) -> Result<StoreMutationBatchReport> {
        let store_platform = self.config.store_platform.as_ref().ok_or_else(|| {
            Error::config(
                "memory_write_transaction_unavailable",
                "transactional memory writes require StorePlatform-backed runtime",
            )
        })?;
        let transaction_id = format!(
            "memory_write_txn_{}",
            stable_hash_hex(&(operation, self.config.clock.now_secs(), mutations.len()))
        );
        store_platform.commit_mutation_batch(StoreMutationBatch {
            transaction_id,
            operation: operation.to_string(),
            scope: self.memory_write_transaction_scope(),
            mutations,
        })
    }

    fn run_long_term_memory_refresh_transactional(
        &self,
        http: &mut (dyn LlmHttpClient + '_),
        llm: &(dyn CoreLlmClient + Send + Sync + '_),
        ctx: LongTermMemoryRefreshContext<'_>,
        chat_id: &str,
        pressure: PressureLevel,
        profile: MemoryProfile,
    ) -> Result<LongTermMemoryRefreshOutcome> {
        let planning_long_term = PlanningLongTermMemoryStore::new(ctx.long_term_memory_store);
        let planning_skills = PlanningSkillStorage::new(ctx.skill_storage);
        let outcome = run_long_term_memory_refresh(
            http,
            llm,
            LongTermMemoryRefreshContext {
                memory_store: ctx.memory_store,
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                long_term_memory_store: &planning_long_term,
                extraction_state_store: ctx.extraction_state_store,
                turn_ledger_store: ctx.turn_ledger_store,
                skill_storage: &planning_skills,
                draft_admission_policy: ctx.draft_admission_policy,
            },
            chat_id,
            pressure,
            profile,
        );
        let mut mutations = Vec::new();
        match &outcome {
            LongTermMemoryRefreshOutcome::Deferred {
                previous_state,
                next_state,
            }
            | LongTermMemoryRefreshOutcome::Failed {
                previous_state,
                next_state,
                ..
            } => {
                if let Some(mutation) = long_term_extraction_state_mutation(
                    chat_id,
                    previous_state.as_ref(),
                    next_state,
                )? {
                    mutations.push(mutation);
                }
            }
            LongTermMemoryRefreshOutcome::Processed {
                previous_state,
                next_state,
                apply_report,
                ..
            } => {
                let long_term_mutations = planning_long_term.into_mutations()?;
                let facet_index_mutations = self
                    .plan_long_term_facet_index_mutations_for_store_mutations(
                        &long_term_mutations,
                    )?;
                mutations.extend(long_term_mutations);
                mutations.extend(facet_index_mutations);
                mutations.extend(runtime_skill_storage_mutations_to_store_mutations(
                    &planning_skills.into_mutations()?,
                ));
                mutations.extend(plan_long_term_extraction_derived_memory_ref_mutations(
                    &self.config.subject_id,
                    &apply_report.accepted_upserts,
                    &apply_report.accepted_skill_writes,
                    self.config.clock.now_secs(),
                )?);
                if let Some(mutation) = long_term_extraction_state_mutation(
                    chat_id,
                    previous_state.as_ref(),
                    next_state,
                )? {
                    mutations.push(mutation);
                }
            }
        }
        if !mutations.is_empty() {
            self.commit_memory_mutation_batch("post_turn.long_term_refresh", mutations)?;
        }
        Ok(outcome)
    }

    fn plan_long_term_extraction_transaction(
        &self,
        extraction: &ParsedLongTermMemoryExtraction,
        now_secs: u64,
    ) -> Result<MemoryLongTermExtractionTransactionPlan> {
        let store = self.config.platform.long_term_memory_store();
        let skill_storage = self.config.platform.skill_storage();
        let mut mutations = Vec::new();
        let mut changed = 0usize;

        for slot in &extraction.deletes {
            let Some(id) = slot.stable_id() else {
                continue;
            };
            if store.get(&id)?.is_some() {
                mutations.push(StoreMutation::DeleteJson {
                    namespace: "long_term".to_string(),
                    key: id.clone(),
                    event_kind: MemoryStoreEventKind::MemoryDelete,
                    plane: "long_term".to_string(),
                    record_key: id.clone(),
                });
                mutations.extend(self.plan_long_term_facet_index_delete_mutations(&[id.clone()]));
                changed = changed.saturating_add(1);
            }
        }

        let mut accepted_upserts = Vec::new();
        let shared_fact_governance = if extraction.upserts.is_empty() {
            None
        } else {
            let mut context = SharedFactWriteGovernanceContext::new(
                self.config.memory_space_id.clone(),
                self.config.scoped_runtime.mounted_subject_id.clone(),
                self.config.scoped_runtime.actor_subject_id.clone(),
                SharedMemoryWriteSource::Extraction,
            );
            context.relationship_id = self
                .config
                .scoped_runtime
                .relationship_scope
                .as_ref()
                .map(|scope| scope.relationship_id.clone());
            let plan = plan_governed_shared_memory_in_space(
                store.as_ref(),
                &extraction.upserts,
                now_secs,
                context,
            )?;
            for entry in &plan.accepted_entries {
                mutations.push(StoreMutation::PutJson {
                    namespace: "long_term".to_string(),
                    key: entry.id.clone(),
                    value: serde_json::to_value(entry).map_err(|error| {
                        Error::config("memory_write_transaction_plan", error.to_string())
                    })?,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "long_term".to_string(),
                    record_key: entry.id.clone(),
                });
            }
            mutations
                .extend(self.plan_long_term_facet_index_upsert_mutations(&plan.accepted_entries)?);
            changed = changed.saturating_add(plan.outcome.changed);
            accepted_upserts = plan.accepted_drafts;
            Some(plan.outcome)
        };

        let mut accepted_skill_writes = Vec::new();
        let procedural_evolution = if extraction.skill_writes.is_empty() {
            None
        } else {
            let skill_plan = plan_governed_runtime_skills(
                skill_storage.as_ref(),
                &extraction.skill_writes,
                RuntimeSkillWriteSource::Extraction,
            )?;
            for mutation in &skill_plan.mutations {
                mutations.extend(runtime_skill_storage_mutations_to_store_mutations(
                    std::slice::from_ref(mutation),
                ));
            }
            changed = changed.saturating_add(skill_plan.outcome.changed);
            let accepted_topics = skill_plan
                .outcome
                .reports
                .iter()
                .filter(|report| matches!(report.action, RuntimeSkillWriteAction::Accepted))
                .map(|report| report.topic.trim().to_string())
                .collect::<HashSet<_>>();
            accepted_skill_writes.extend(
                extraction
                    .skill_writes
                    .iter()
                    .filter(|write| accepted_topics.contains(write.topic.trim()))
                    .cloned(),
            );
            Some(build_skill_evolution_report_from_write_outcome(
                &extraction.skill_writes,
                &skill_plan.outcome,
            ))
        };

        mutations.extend(plan_long_term_extraction_derived_memory_ref_mutations(
            &self.config.subject_id,
            &accepted_upserts,
            &accepted_skill_writes,
            now_secs,
        )?);

        Ok(MemoryLongTermExtractionTransactionPlan {
            changed,
            shared_fact_governance,
            procedural_evolution,
            mutations,
        })
    }

    fn plan_long_term_control_mutation(
        &self,
        request: CoreLongTermMemoryControlMutationRequest,
    ) -> Result<(
        bm_core::memory::MemoryLongTermMutationReport,
        Vec<StoreMutation>,
    )> {
        let store = self.config.platform.long_term_memory_store();
        let control_store = self.config.platform.long_term_memory_control_store();
        let planning_store = PlanningLongTermMemoryStore::new(store.as_ref());
        let planning_control_store = PlanningLongTermControlStore::new(control_store.as_ref());
        let report = apply_long_term_memory_control_mutation(
            &planning_store,
            &planning_control_store,
            request,
        )?;
        let mut mutations = Vec::new();
        if report.accepted && !report.dry_run {
            let long_term_mutations = planning_store.into_mutations()?;
            let facet_index_mutations = self
                .plan_long_term_facet_index_mutations_for_store_mutations(&long_term_mutations)?;
            mutations.extend(long_term_mutations);
            mutations.extend(facet_index_mutations);
            mutations.extend(planning_control_store.into_mutations()?);
        }
        Ok((report, mutations))
    }

    fn plan_memory_governance_policy_mutation(
        &self,
        operation: bm_core::memory::MemoryGovernancePolicyMutation,
        reason: String,
        dry_run: bool,
        now_secs: u64,
    ) -> Result<(
        bm_core::memory::MemoryGovernancePolicyMutationReport,
        Vec<StoreMutation>,
    )> {
        let control_store = self.config.platform.long_term_memory_control_store();
        let planning_control_store = PlanningLongTermControlStore::new(control_store.as_ref());
        let report = apply_long_term_memory_governance_policy_mutation(
            &planning_control_store,
            operation,
            reason,
            dry_run,
            now_secs,
        )?;
        let mutations = if report.accepted && !report.dry_run {
            planning_control_store.into_mutations()?
        } else {
            Vec::new()
        };
        Ok((report, mutations))
    }

    pub fn list_runtime_skills(
        &self,
        request: RuntimeSkillListRequest,
    ) -> Result<RuntimeSkillListReport> {
        self.ensure_visible("inspect.skills", self.capabilities.inspection)?;
        let platform = self.config.platform.as_ref();
        let storage = platform.skill_storage();
        let meta_store = platform.skill_meta_store();
        let disabled: HashSet<String> = get_disabled_skills(meta_store.as_ref())
            .into_iter()
            .collect();
        let runtime_records = list_runtime_skill_records(storage.as_ref());
        let mut rows = Vec::new();
        let query = request
            .query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);

        for record in runtime_records {
            let enabled = !disabled.contains(&record.name);
            let summary = runtime_skill_summary(&record, enabled);
            if !request.include_disabled && !summary.enabled {
                continue;
            }
            if !request.include_retired && matches!(record.status, RuntimeSkillStatus::Retired) {
                continue;
            }
            if !skill_matches_query(
                &summary,
                Some(&record.summary),
                Some(&record.procedure),
                query.as_deref(),
            ) {
                continue;
            }
            rows.push(summary);
        }

        rows.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.name.cmp(&right.name))
        });

        let total = rows.len();
        let active = rows.iter().filter(|skill| skill.enabled).count();
        let disabled_count = rows.iter().filter(|skill| !skill.enabled).count();
        let skills = if request.limit == 0 {
            Vec::new()
        } else {
            rows.into_iter().take(request.limit).collect()
        };
        self.audit("inspect.skills", true, "skill_list_completed");
        Ok(RuntimeSkillListReport {
            total,
            active,
            disabled: disabled_count,
            runtime_skills: total,
            skills,
        })
    }

    pub fn get_runtime_skill(
        &self,
        request: RuntimeSkillDetailRequest,
    ) -> Result<RuntimeSkillDetailReport> {
        self.ensure_visible("inspect.skills", self.capabilities.inspection)?;
        let name = checked_skill_name(&request.name, "skill_detail")?;
        if !is_runtime_skill_name(name) {
            self.audit("inspect.skills", false, "runtime_skill_not_found");
            return Err(Error::config("skill_detail", "runtime_skill_not_found"));
        }
        let platform = self.config.platform.as_ref();
        let storage = platform.skill_storage();
        let meta_store = platform.skill_meta_store();
        let disabled: HashSet<String> = get_disabled_skills(meta_store.as_ref())
            .into_iter()
            .collect();
        if let Some(record) = list_runtime_skill_records(storage.as_ref())
            .into_iter()
            .find(|record| record.name == name)
        {
            let summary = runtime_skill_summary(&record, !disabled.contains(name));
            let lineage = render_runtime_skill_lineage(&record);
            let strategy_diffs = render_runtime_skill_strategy_diffs(&record);
            let raw_content = render_runtime_skill_detail_content(&record);
            self.audit("inspect.skills", true, "skill_detail_completed");
            return Ok(RuntimeSkillDetailReport {
                summary,
                summary_text: record.summary,
                procedure_text: record.procedure,
                raw_content,
                citations: record.citations,
                lineage,
                strategy_diffs,
                source_chat_id: record.source_chat_id,
                last_outcome_note: record.last_outcome_note,
            });
        }

        self.audit("inspect.skills", false, "runtime_skill_not_found");
        Err(Error::config("skill_detail", "runtime_skill_not_found"))
    }

    pub fn edit_runtime_skill(
        &self,
        request: RuntimeSkillEditRequest,
    ) -> Result<RuntimeSkillMutationReport> {
        self.ensure_visible("write.skills", self.capabilities.write)?;
        let name = checked_skill_name(&request.name, "skill_edit")?;
        if !is_runtime_skill_name(name) {
            self.audit("write.skills", false, "runtime_skill_create_forbidden");
            return Err(Error::config("skill_edit", "runtime_skill_not_found"));
        }
        let storage = self.config.platform.skill_storage();
        let existing = list_runtime_skill_records(storage.as_ref())
            .into_iter()
            .find(|record| record.name == name);
        if existing.is_none() {
            self.audit("write.skills", false, "runtime_skill_create_forbidden");
            return Err(Error::config("skill_edit", "runtime_skill_not_found"));
        }
        let title = checked_non_empty(&request.title, "skill_edit", "title must not be empty")?;
        let topic = checked_non_empty(&request.topic, "skill_edit", "topic must not be empty")?;
        let summary =
            checked_non_empty(&request.summary, "skill_edit", "summary must not be empty")?;
        let procedure = checked_non_empty(
            &request.procedure,
            "skill_edit",
            "procedure must not be empty",
        )?;
        let write = RuntimeSkillWrite {
            name: name.to_string(),
            title: title.to_string(),
            topic: topic.to_string(),
            summary: summary.to_string(),
            content: procedure.to_string(),
            citations: request.citations,
            source_chat_id: request.source_chat_id,
            observed_at: request.observed_at,
        };
        let normalized = normalize_runtime_skill_write_names(vec![write])
            .into_iter()
            .next()
            .ok_or_else(|| Error::config("skill_edit", "skill write missing"))?;
        let stored_name = normalized.name.clone();
        let plan = plan_governed_runtime_skills(
            storage.as_ref(),
            &[normalized],
            RuntimeSkillWriteSource::Manual,
        )?;
        let outcome = plan.outcome;
        let mutations = runtime_skill_storage_mutations_to_store_mutations(&plan.mutations);
        if !mutations.is_empty() {
            self.commit_memory_mutation_batch("runtime_skill.edit", mutations)?;
        }
        let accepted = outcome.accepted > 0;
        let reason = format!(
            "submitted={}, accepted={}, rejected={}",
            outcome.submitted, outcome.accepted, outcome.rejected
        );
        self.audit("write.skills", accepted, &reason);
        Ok(RuntimeSkillMutationReport {
            accepted,
            changed: outcome.changed > 0,
            name: stored_name,
            operation: "runtime_skill.edit",
            reason,
        })
    }

    pub fn set_runtime_skill_enabled(
        &self,
        request: RuntimeSkillSetEnabledRequest,
    ) -> Result<RuntimeSkillMutationReport> {
        self.ensure_visible("write.skills", self.capabilities.write)?;
        let name = checked_skill_name(&request.name, "skill_set_enabled")?;
        if !is_runtime_skill_name(name) {
            self.audit("write.skills", false, "runtime_skill_not_found");
            return Err(Error::config(
                "skill_set_enabled",
                "runtime_skill_not_found",
            ));
        }
        let platform = self.config.platform.as_ref();
        let storage = platform.skill_storage();
        if !list_runtime_skill_records(storage.as_ref())
            .iter()
            .any(|record| record.name == name)
        {
            self.audit("write.skills", false, "runtime_skill_not_found");
            return Err(Error::config(
                "skill_set_enabled",
                "runtime_skill_not_found",
            ));
        }
        let meta_store = platform.skill_meta_store();
        let (_order, mut disabled) = meta_store.read_meta()?;
        let was_disabled = disabled.iter().any(|value| value == name);
        if request.enabled {
            disabled.retain(|value| value != name);
        } else if !disabled.iter().any(|value| value == name) {
            disabled.push(name.to_string());
        }
        let changed = was_disabled == request.enabled;
        if changed {
            self.commit_memory_mutation_batch(
                "runtime_skill.set_enabled",
                vec![StoreMutation::PutJson {
                    namespace: "skill_meta".to_string(),
                    key: "disabled".to_string(),
                    value: serde_json::to_value(&disabled).map_err(|error| {
                        Error::config("runtime_skill.set_enabled", error.to_string())
                    })?,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "skill_meta".to_string(),
                    record_key: "disabled".to_string(),
                }],
            )?;
        }
        let operation = if request.enabled {
            "skill.enable"
        } else {
            "skill.disable"
        };
        let reason = if changed {
            "skill_enabled_state_changed"
        } else {
            "skill_enabled_state_unchanged"
        };
        self.audit("write.skills", true, reason);
        Ok(RuntimeSkillMutationReport {
            accepted: true,
            changed,
            name: name.to_string(),
            operation,
            reason: reason.to_string(),
        })
    }

    pub fn delete_runtime_skill(
        &self,
        request: RuntimeSkillDeleteRequest,
    ) -> Result<RuntimeSkillMutationReport> {
        self.ensure_visible("write.skills", self.capabilities.write)?;
        let name = checked_skill_name(&request.name, "skill_delete")?;
        if !is_runtime_skill_name(name) {
            self.audit("write.skills", false, "runtime_skill_not_found");
            return Err(Error::config("skill_delete", "runtime_skill_not_found"));
        }
        let platform = self.config.platform.as_ref();
        let storage = platform.skill_storage();
        if !list_runtime_skill_records(storage.as_ref())
            .iter()
            .any(|record| record.name == name)
        {
            self.audit("write.skills", false, "runtime_skill_not_found");
            return Err(Error::config("skill_delete", "runtime_skill_not_found"));
        }
        let meta_store = platform.skill_meta_store();
        let (mut order, mut disabled) = meta_store.read_meta()?;
        disabled.retain(|value| value != name);
        let before_len = order.len();
        order.retain(|value| value != name);
        let mut mutations = vec![StoreMutation::DeleteBlob {
            namespace: "skills".to_string(),
            key: name.to_string(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: "skills".to_string(),
            record_key: name.to_string(),
        }];
        mutations.push(StoreMutation::PutJson {
            namespace: "skill_meta".to_string(),
            key: "disabled".to_string(),
            value: serde_json::to_value(&disabled)
                .map_err(|error| Error::config("runtime_skill.delete", error.to_string()))?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "skill_meta".to_string(),
            record_key: "disabled".to_string(),
        });
        if before_len != order.len() {
            mutations.push(StoreMutation::PutJson {
                namespace: "skill_meta".to_string(),
                key: "order".to_string(),
                value: serde_json::to_value(&order)
                    .map_err(|error| Error::config("runtime_skill.delete", error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skill_meta".to_string(),
                record_key: "order".to_string(),
            });
        }
        self.commit_memory_mutation_batch("runtime_skill.delete", mutations)?;
        self.audit("write.skills", true, "skill_deleted");
        Ok(RuntimeSkillMutationReport {
            accepted: true,
            changed: true,
            name: name.to_string(),
            operation: "runtime_skill.delete",
            reason: "runtime_skill_deleted".to_string(),
        })
    }

    pub fn recall(&self, request: MemoryRecallRequest) -> Result<MemoryRecallReport> {
        self.ensure_visible("recall", self.capabilities.recall)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Recall,
            RuntimeLifecycleTrigger::SdkCall,
            RuntimeLifecycleModeInput::default(),
        );
        let platform = self.config.platform.as_ref();
        let session_store = self.transcript_backed_session_store(
            platform.session_store(),
            TranscriptReplayView::ModelContext,
        );
        let memory_store = platform.memory_store();
        let session_summary_store = platform.session_summary_store();
        let long_term_memory_store = platform.long_term_memory_store();
        let active_work_store = platform.active_work_store();
        let continuity_capsule_store = platform.continuity_capsule_store();
        let turn_ledger_store = platform.turn_ledger_store();
        let skill_storage = platform.skill_storage();
        let task_run_store = platform.task_run_store();
        let task_learning_store = platform.task_learning_store();

        let recent = session_store.load_recent(&self.config.scope.chat_id, request.limit.max(1))?;
        let summary = session_summary_store.get(&self.config.scope.chat_id)?;
        let procedural_hits = retrieve_runtime_skill_hits(
            skill_storage.as_ref(),
            &request.query,
            Some(&self.config.scope.chat_id),
            self.config.clock.now_secs(),
            request.limit.max(1),
        );
        let agent_skill_hits = retrieve_agent_skill_hits(
            &self.config.agent_skill_registry,
            &request.query,
            request.limit.max(1),
        );
        let agent_tool_experiences = list_agent_tool_experience_records(skill_storage.as_ref());
        let agent_tool_registries = self.agent_tool_registries();
        let agent_tool_selection = select_agent_tool_hints(
            &agent_tool_registries,
            &agent_tool_experiences,
            &request.tool_registry_refs,
            request.limit.max(1),
        );
        let source_max_chars = self
            .config
            .runtime_budget
            .projection_source_budget
            .context_assembly_max_chars;
        let working = inspect_working_recall(WorkingRecallInspectionInput {
            chat_id: &self.config.scope.chat_id,
            query: &request.query,
            summary_text: summary.as_deref(),
            recent: &recent,
            system_max_len: source_max_chars,
            profile: self.memory_profile(),
            current_channel: Some(&self.config.scope.channel),
            session_store: session_store.as_ref(),
            memory_store: memory_store.as_ref(),
            long_term_memory_store: long_term_memory_store.as_ref(),
            active_work_store: Some(active_work_store.as_ref()),
            continuity_capsule_store: continuity_capsule_store.as_ref(),
            turn_ledger_store: turn_ledger_store.as_ref(),
            skill_storage: Some(skill_storage.as_ref()),
            task_run_store: Some(task_run_store.as_ref()),
            task_learning_store: Some(task_learning_store.as_ref()),
        });
        let source_evidence = runtime_recall_graph_source_evidence(
            &procedural_hits,
            &working,
            self.config.clock.now_secs(),
        );
        let source_candidate_ids = recall_graph_candidate_ids(&source_evidence);
        let graph_anchor_evidence = runtime_recall_graph_anchor_evidence(
            &procedural_hits,
            &working,
            self.config.clock.now_secs(),
            self.config
                .runtime_budget
                .graph_expansion_budget
                .max_seed_candidates,
        );
        let graph_anchor_candidate_ids = recall_graph_candidate_ids(&graph_anchor_evidence);
        let persistent_graph = self.load_persistent_recall_graph(&graph_anchor_candidate_ids);
        let graph = build_recall_graph_report(
            &request.query,
            graph_anchor_evidence,
            &graph_anchor_candidate_ids,
            &persistent_graph,
            GraphRecallExpansionBudget::from_runtime_budget(
                &self.config.runtime_budget.graph_expansion_budget,
            ),
        );
        let facet_index_report = build_memory_facet_index_report(&source_candidate_ids);
        let hit_count = procedural_hits
            .len()
            .saturating_add(agent_skill_hits.len())
            .saturating_add(agent_tool_selection.tool_hints.len())
            .saturating_add(working_recall_hit_count(&working));
        let telemetry_payload = [
            ("memory_hit", (hit_count > 0).to_string()),
            ("hit_count", hit_count.to_string()),
            (
                "budget_report_id",
                self.config.runtime_budget.report_id.clone(),
            ),
            (
                "budget_limited_by",
                self.config.runtime_budget.limited_by.join(","),
            ),
        ];
        self.audit("recall", true, "recall_completed");
        Ok(MemoryRecallReport {
            query: request.query,
            procedural_hits,
            agent_skill_hits,
            agent_tool_hints: agent_tool_selection.tool_hints,
            tool_experience_status: agent_tool_selection.tool_experience_status,
            working,
            source_candidate_ids,
            graph_anchor_candidate_ids,
            graph_index_report: graph.index_report,
            facet_index_report,
            graph_rerank: graph.rerank,
            graph_gate: graph.gate,
            graph_candidate_evidence_ref_index: graph.candidate_evidence_ref_index,
            compact_graph: graph.compact_graph,
            lifecycle_report: self.finish_lifecycle_success_with_payload(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::Inspect,
                false,
                "recall_completed",
                &telemetry_payload,
            )?,
        })
    }

    pub fn eval_recall(&self, request: MemoryEvalRecallRequest) -> Result<MemoryEvalRecallReport> {
        self.ensure_visible("eval_recall", self.capabilities.recall)?;
        let recall_limit = request.k.max(50).max(1);
        let recall = self.recall(MemoryRecallRequest {
            query: request.query.clone(),
            limit: recall_limit,
            tool_registry_refs: request.tool_registry_refs.clone(),
        })?;
        let source_candidates = build_eval_recall_candidates(
            &recall.source_candidate_ids,
            &recall,
            request.include_graph_neighbors,
            request.include_score_breakdown,
        );
        let graph_anchor_candidates = build_eval_recall_candidates(
            &recall.graph_anchor_candidate_ids,
            &recall,
            request.include_graph_neighbors,
            request.include_score_breakdown,
        );
        let all_expanded_candidates = build_eval_recall_candidates(
            &recall.graph_rerank.expanded_candidate_ids,
            &recall,
            request.include_graph_neighbors,
            request.include_score_breakdown,
        );
        let expanded_candidates = if request.include_expanded_candidates {
            all_expanded_candidates.clone()
        } else {
            Vec::new()
        };
        let reranked_candidates = build_eval_recall_candidates(
            &recall.graph_rerank.selected_ids,
            &recall,
            request.include_graph_neighbors,
            request.include_score_breakdown,
        );
        let selected_candidates = reranked_candidates
            .iter()
            .take(request.k.max(1))
            .cloned()
            .collect::<Vec<_>>();
        let rendered_candidates = selected_candidates
            .iter()
            .filter(|candidate| {
                recall
                    .source_candidate_ids
                    .iter()
                    .any(|source_id| source_id == &candidate.candidate_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let eval_candidate_pool = build_eval_candidate_pool(
            &source_candidates,
            &graph_anchor_candidates,
            &all_expanded_candidates,
            &reranked_candidates,
        );
        let evidence_ref_index = build_eval_recall_evidence_ref_index(&eval_candidate_pool);
        let stage_diagnostics = build_eval_recall_stage_diagnostics(
            request.benchmark_context.as_ref(),
            &source_candidates,
            &graph_anchor_candidates,
            &all_expanded_candidates,
            &reranked_candidates,
            &selected_candidates,
            &rendered_candidates,
            &recall,
        );
        let ablation_report = stage_diagnostics.ablation_report.clone();
        let metrics = build_eval_recall_metrics(
            &request,
            source_candidates.len(),
            all_expanded_candidates.len(),
            selected_candidates.len(),
            rendered_candidates.len(),
            &reranked_candidates,
        );
        let missing_evidence_refs = if request.include_missing_evidence {
            missing_evidence_for_k(
                request
                    .benchmark_context
                    .as_ref()
                    .map(|context| context.expected_evidence_refs.as_slice())
                    .unwrap_or(&[]),
                &reranked_candidates,
                request.k.max(1),
            )
        } else {
            Vec::new()
        };
        let privacy_report = build_eval_recall_privacy_report(&recall);

        Ok(MemoryEvalRecallReport {
            query: request.query,
            benchmark_context: request.benchmark_context,
            source_candidates,
            graph_anchor_candidates,
            expanded_candidates,
            eval_candidate_pool,
            graph_neighbors: if request.include_graph_neighbors {
                recall.graph_rerank.graph_neighbor_ids.clone()
            } else {
                Vec::new()
            },
            reranked_candidates,
            selected_candidates: selected_candidates.clone(),
            rendered_candidates: rendered_candidates.clone(),
            rendered_block_preview: render_eval_recall_block_preview(&rendered_candidates),
            evidence_ref_index,
            stage_diagnostics,
            metrics,
            missing_evidence_refs,
            budget_report: self.config.runtime_budget.clone(),
            privacy_report,
            facet_index_report: recall.facet_index_report,
            ablation_report,
            graph_index_report: recall.graph_index_report,
            graph_rerank: recall.graph_rerank,
            graph_gate: recall.graph_gate,
            compact_graph: recall.compact_graph,
            lifecycle_report: recall.lifecycle_report,
        })
    }

    fn load_persistent_recall_graph(
        &self,
        source_candidate_ids: &[String],
    ) -> PersistentRecallGraphLoadReport {
        let Some(store_platform) = self.config.store_platform.as_ref() else {
            return PersistentRecallGraphLoadReport::default();
        };
        let mut failures = Vec::new();
        let mut index_failures = Vec::new();
        let index_keys = source_candidate_ids
            .iter()
            .map(|candidate_id| format!("graph_index:{candidate_id}"))
            .collect::<Vec<_>>();
        let indexes = read_persistent_graph_docs_by_keys::<MemoryGraphRecallIndexDoc>(
            store_platform,
            "memory_graph_indexes",
            &index_keys,
            &mut index_failures,
        );
        let limits = &self.config.runtime_budget.graph_expansion_budget;
        let node_keys = bounded_persistent_graph_keys(
            indexed_node_keys(source_candidate_ids, &indexes),
            limits.max_graph_nodes_loaded,
            "memory_graph_nodes_loaded_budget_exceeded",
            &mut failures,
        );
        let edge_keys = bounded_persistent_graph_keys(
            indexed_edge_keys(&indexes),
            limits.max_graph_edges_loaded,
            "memory_graph_edges_loaded_budget_exceeded",
            &mut failures,
        );
        let backlink_keys = bounded_persistent_graph_keys(
            indexed_backlink_keys(&indexes),
            limits.max_backlinks_loaded,
            "memory_graph_backlinks_loaded_budget_exceeded",
            &mut failures,
        );

        let nodes = read_persistent_graph_docs_by_keys::<MemoryGraphNode>(
            store_platform,
            "memory_graph_nodes",
            &node_keys,
            &mut failures,
        );
        let edges = read_persistent_graph_docs_by_keys::<MemoryGraphEdge>(
            store_platform,
            "memory_graph_edges",
            &edge_keys,
            &mut failures,
        );
        let backlinks = read_persistent_graph_docs_by_keys::<EvidenceBacklink>(
            store_platform,
            "memory_graph_backlinks",
            &backlink_keys,
            &mut failures,
        );

        if !failures.is_empty() {
            return PersistentRecallGraphLoadReport {
                graph: Some(build_temporal_memory_graph_from_parts(
                    nodes, edges, backlinks,
                )),
                indexes,
                failures,
                index_failures,
            };
        }
        if nodes.is_empty() && edges.is_empty() && backlinks.is_empty() {
            return PersistentRecallGraphLoadReport::default();
        }

        PersistentRecallGraphLoadReport {
            graph: Some(build_temporal_memory_graph_from_parts(
                nodes, edges, backlinks,
            )),
            indexes,
            failures,
            index_failures,
        }
    }

    pub fn write_temporal_memory_graph(
        &self,
        request: TemporalMemoryGraphWriteRequest,
    ) -> Result<TemporalMemoryGraphMutationReport> {
        self.ensure_visible("write.memory_graph", self.capabilities.write)?;
        let operation = if request.operation.trim().is_empty() {
            "memory_graph.write"
        } else {
            request.operation.trim()
        };
        if operation != "memory_graph.write" {
            return Err(Error::config(
                "temporal_memory_graph_write",
                format!("unsupported temporal memory graph operation {operation}"),
            ));
        }
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Maintain,
            RuntimeLifecycleTrigger::SdkCall,
            RuntimeLifecycleModeInput::default(),
        );
        let plan = plan_temporal_memory_graph_write(
            "memory_graph.write",
            request.nodes,
            request.edges,
            request.backlinks,
        );
        if !plan.accepted {
            let lifecycle_report = self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::Noop,
                false,
                "temporal_memory_graph_write_rejected",
            )?;
            return Ok(TemporalMemoryGraphMutationReport {
                accepted: false,
                operation: plan.operation,
                node_count: plan.node_count,
                edge_count: plan.edge_count,
                backlink_count: plan.backlink_count,
                revision_count: plan.revision_count,
                index_count: 0,
                index_revision: None,
                gate_failures: plan.gate_failures,
                transaction: None,
                lifecycle_report,
            });
        }

        let planned = temporal_memory_graph_plan_mutations(
            &plan,
            &self.config.memory_space_id,
            &self.config.subject_id,
        )?;
        let TemporalMemoryGraphPlannedMutations {
            mutations,
            index_count,
            index_revision,
        } = planned;
        let changed_count = plan
            .node_count
            .saturating_add(plan.edge_count)
            .saturating_add(plan.backlink_count)
            .saturating_add(plan.revision_count)
            .saturating_add(index_count);
        let extra_payload = [
            ("node_count", plan.node_count.to_string()),
            ("edge_count", plan.edge_count.to_string()),
            ("backlink_count", plan.backlink_count.to_string()),
            ("revision_count", plan.revision_count.to_string()),
            ("index_count", index_count.to_string()),
            ("index_revision", index_revision.clone()),
            ("gate_failures", plan.gate_failures.join(",")),
        ];
        let (lifecycle_report, transaction) = self.commit_memory_write_transaction(
            lifecycle,
            "memory_graph.write",
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::RunMaintenance,
            true,
            "temporal_memory_graph_write_committed",
            &extra_payload,
            mutations,
            changed_count,
        )?;

        Ok(TemporalMemoryGraphMutationReport {
            accepted: true,
            operation: plan.operation,
            node_count: plan.node_count,
            edge_count: plan.edge_count,
            backlink_count: plan.backlink_count,
            revision_count: plan.revision_count,
            index_count,
            index_revision: Some(index_revision),
            gate_failures: plan.gate_failures,
            transaction: Some(transaction),
            lifecycle_report,
        })
    }

    pub fn project(&self, request: MemoryProjectionRequest) -> Result<MemoryProjectionReport> {
        self.ensure_visible("project", self.capabilities.projection)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Project,
            RuntimeLifecycleTrigger::SdkCall,
            self.mode_input_for_request(request.mode_input, request.pressure),
        );
        let context = self.load_projection_context(&request, &lifecycle);
        let render_max_chars = self
            .config
            .runtime_budget
            .projection_render_chars_for_request(request.system_max_len, None);
        let runtime_awareness = render_runtime_awareness_block(
            self.config.profile,
            request.pressure,
            self.config.runtime_budget.resource_snapshot.pressure,
        );
        let runtime_private_context_allowed =
            self.config.privacy_policy.private_plane_projection_allowed
                && lifecycle.admission.private_depth_allowed;
        let inhabited_subject_projection =
            compile_inhabited_subject_projection(InhabitedSubjectProjectionInput {
                context: &context,
                now_secs: self.config.clock.now_secs(),
                platform: runtime_awareness_profile_label(self.config.profile),
                device_identity: self.config.subject_id.as_str(),
                channel: self.config.scope.channel.as_str(),
                chat_id: self.config.scope.chat_id.as_str(),
                pressure: request.pressure,
                render_budget_chars: render_max_chars,
                runtime_private_context_allowed,
                foreground_disclosure_allowed: false,
                user_query: request.user_query.as_str(),
            });
        let agent_skill_hits =
            retrieve_agent_skill_hits(&self.config.agent_skill_registry, &request.user_query, 4);
        let agent_skill_projection_budget = render_max_chars.saturating_div(6).clamp(320, 1600);
        let (agent_skill_hints, agent_skill_audit) = build_projected_agent_skill_hints(
            &self.config.agent_skill_registry,
            &agent_skill_hits,
            agent_skill_projection_budget,
        );
        let skill_storage = self.config.platform.skill_storage();
        let agent_tool_experiences = list_agent_tool_experience_records(skill_storage.as_ref());
        let agent_tool_registries = self.agent_tool_registries();
        let agent_tool_selection = select_agent_tool_hints(
            &agent_tool_registries,
            &agent_tool_experiences,
            &request.tool_registry_refs,
            5,
        );
        let mut runtime_projection = build_llm_runtime_projection_envelope(
            projection_id(self, &request),
            &context,
            &runtime_awareness,
            &inhabited_subject_projection,
            render_max_chars,
        );
        attach_agent_skill_hints_to_runtime_projection(
            &mut runtime_projection,
            agent_skill_hints,
            render_max_chars,
        );
        attach_agent_tool_hints_to_runtime_projection(
            &mut runtime_projection,
            agent_tool_selection.tool_hints.clone(),
            render_max_chars,
        );
        let system_memory_block = runtime_projection.rendered_block.clone();
        let hit_count = prompt_context_hit_count(&context)
            .saturating_add(agent_skill_hits.len())
            .saturating_add(agent_tool_selection.tool_hints.len());
        let system_memory_chars = system_memory_block.chars().count();
        let projection_audit = build_projection_audit(ProjectionAuditInput {
            runtime: self,
            context: &context,
            lifecycle: &lifecycle,
            render_budget_chars: render_max_chars,
            system_memory_chars,
            injected: !system_memory_block.trim().is_empty(),
            runtime_projection: &runtime_projection,
            agent_skill_audit,
            agent_tool_audit: agent_tool_selection.audit,
        });
        let subject_projection = build_subject_projection_report(
            &projection_audit,
            &request,
            &system_memory_block,
            &runtime_projection,
            Some(&inhabited_subject_projection),
        );
        let projection_faithfulness = build_projection_faithfulness_check(
            &subject_projection,
            &runtime_projection,
            &system_memory_block,
        );
        let private_disclosure_integrity =
            build_private_disclosure_integrity_report(&projection_audit, &runtime_projection);
        let telemetry_payload = [
            ("memory_hit", (hit_count > 0).to_string()),
            ("hit_count", hit_count.to_string()),
            ("system_memory_chars", system_memory_chars.to_string()),
            (
                "projection_source_max_chars",
                self.config
                    .runtime_budget
                    .projection_source_budget
                    .context_assembly_max_chars
                    .to_string(),
            ),
            ("projection_render_max_chars", render_max_chars.to_string()),
            (
                "budget_report_id",
                self.config.runtime_budget.report_id.clone(),
            ),
            (
                "budget_limited_by",
                self.config.runtime_budget.limited_by.join(","),
            ),
            (
                "projection_injected",
                (!system_memory_block.trim().is_empty()).to_string(),
            ),
        ];
        self.audit("project", true, "projection_completed");
        Ok(MemoryProjectionReport {
            system_memory_block,
            context,
            audit: projection_audit,
            life_projection: runtime_projection.subject_mount.clone(),
            work_integrity: runtime_projection.work_integrity.clone(),
            runtime_projection,
            subject_projection,
            projection_faithfulness,
            private_disclosure_integrity,
            lifecycle_report: self.finish_lifecycle_success_with_payload(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::RefreshProjection,
                false,
                "projection_completed",
                &telemetry_payload,
            )?,
        })
    }

    fn load_projection_context(
        &self,
        request: &MemoryProjectionRequest,
        lifecycle: &RuntimeLifecycleReport,
    ) -> crate::PromptMemoryContext {
        let platform = self.config.platform.as_ref();
        let session_store = self.transcript_backed_session_store(
            platform.session_store(),
            TranscriptReplayView::ModelContext,
        );
        let memory_store = platform.memory_store();
        let session_summary_store = platform.session_summary_store();
        let long_term_memory_store = platform.long_term_memory_store();
        let execution_state_store = platform.execution_state_store();
        let active_work_store = platform.active_work_store();
        let task_run_store = platform.task_run_store();
        let task_artifact_store = platform.task_artifact_store();
        let task_learning_store = platform.task_learning_store();
        let self_model_store = platform.self_model_store();
        let self_authored_core_store = platform.self_authored_core_store();
        let relationship_constitution_store = platform.relationship_constitution_store();
        let relationship_portfolio_store = platform.relationship_portfolio_store();
        let relationship_topology_store = platform.relationship_topology_store();
        let world_sense_store = platform.world_sense_store();
        let autonomy_strategy_store = platform.autonomy_strategy_store();
        let outer_voice_store = platform.outer_voice_store();
        let inner_life_store = platform.inner_life_store();
        let self_continuity_store = platform.self_continuity_store();
        let felt_significance_store = platform.felt_significance_store();
        let temperament_continuity_store = platform.temperament_continuity_store();
        let inner_conflict_store = platform.inner_conflict_store();
        let private_doc_store = platform.private_doc_store();
        let private_garden_store = platform.private_garden_store();
        let mental_privacy_store = platform.mental_privacy_store();
        let remind_store = platform.remind_at_store();
        let task_store = platform.task_store();
        let turn_continuity_evidence_store = platform.turn_continuity_evidence_store();
        let turn_ledger_store = platform.turn_ledger_store();
        let skill_storage = platform.skill_storage();
        let continuity_capsule_store = platform.continuity_capsule_store();
        let memory_system_kind = self.memory_profile().memory_system_kind();
        let include_private_runtime_projection =
            self.config.privacy_policy.private_plane_projection_allowed
                && lifecycle.admission.private_depth_allowed;
        load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: &self.config.scope.chat_id,
            current_channel: &self.config.scope.channel,
            user_query: &request.user_query,
            memory_system_kind,
            system_max_len: self
                .config
                .runtime_budget
                .projection_source_budget
                .context_assembly_max_chars,
            now_secs: self.config.clock.now_secs(),
            participation_plan: self.prompt_participation_plan(),
            recent_messages_limit: request.recent_messages_limit.min(
                self.config
                    .runtime_budget
                    .projection_source_budget
                    .recent_messages_limit,
            ),
            load_long_term_memory: true,
            include_private_runtime_projection,
            include_private_garden_projection: include_private_runtime_projection,
            session_store: session_store.as_ref(),
            memory_store: memory_store.as_ref(),
            session_summary_store: session_summary_store.as_ref(),
            long_term_memory_store: long_term_memory_store.as_ref(),
            execution_state_store: execution_state_store.as_ref(),
            active_work_store: active_work_store.as_ref(),
            task_run_store: task_run_store.as_ref(),
            task_artifact_store: task_artifact_store.as_ref(),
            task_learning_store: task_learning_store.as_ref(),
            self_model_store: self_model_store.as_ref(),
            self_authored_core_store: self_authored_core_store.as_ref(),
            relationship_constitution_store: relationship_constitution_store.as_ref(),
            relationship_portfolio_store: relationship_portfolio_store.as_ref(),
            relationship_topology_store: relationship_topology_store.as_ref(),
            world_sense_store: world_sense_store.as_ref(),
            autonomy_strategy_store: autonomy_strategy_store.as_ref(),
            outer_voice_store: outer_voice_store.as_ref(),
            inner_life_store: inner_life_store.as_ref(),
            self_continuity_store: self_continuity_store.as_ref(),
            felt_significance_store: felt_significance_store.as_ref(),
            temperament_continuity_store: temperament_continuity_store.as_ref(),
            inner_conflict_store: inner_conflict_store.as_ref(),
            private_doc_store: private_doc_store.as_ref(),
            private_garden_store: private_garden_store.as_ref(),
            mental_privacy_store: mental_privacy_store.as_ref(),
            remind_store: remind_store.as_ref(),
            task_store: task_store.as_ref(),
            turn_continuity_evidence_store: turn_continuity_evidence_store.as_ref(),
            turn_ledger_store: turn_ledger_store.as_ref(),
            skill_storage: skill_storage.as_ref(),
            continuity_capsule_store: continuity_capsule_store.as_ref(),
        })
    }

    pub fn maintain(
        &self,
        http: &mut dyn LlmHttpClient,
        llm: &(dyn CoreLlmClient + Send + Sync),
        request: MemoryMaintenanceRequest,
    ) -> Result<MemoryMaintenanceReport> {
        self.ensure_visible("maintain", self.capabilities.maintenance)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Maintain,
            RuntimeLifecycleTrigger::PostReply,
            self.mode_input_for_request(request.mode_input, request.pressure),
        );
        if lifecycle.admission.disposition != RuntimeLifecycleDisposition::ExecuteNow {
            let lifecycle_report = self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::Noop,
                false,
                "maintenance_not_executed",
            )?;
            return Ok(MemoryMaintenanceReport {
                report: None,
                long_term_refresh_enqueued: false,
                lifecycle_report,
            });
        }
        let platform = self.config.platform.as_ref();
        let session_store = self.transcript_backed_session_store(
            platform.session_store(),
            TranscriptReplayView::RawOwnerOnly,
        );
        let memory_store = platform.memory_store();
        let session_summary_store = platform.session_summary_store();
        let execution_state_store = platform.execution_state_store();
        let active_work_store = platform.active_work_store();
        let long_term_memory_store = platform.long_term_memory_store();
        let continuity_capsule_store = platform.continuity_capsule_store();
        let extraction_state_store = platform.long_term_memory_extraction_state_store();
        let turn_ledger_store = platform.turn_ledger_store();
        let skill_storage = platform.skill_storage();
        let task_run_store = platform.task_run_store();
        let task_artifact_store = platform.task_artifact_store();
        let task_learning_store = platform.task_learning_store();
        let maintenance_budget = self.config.runtime_budget.maintenance_budget;
        let user_content = bound_text_for_budget(
            &request.user_content,
            maintenance_budget.user_input_max_chars,
            maintenance_budget.user_input_max_bytes,
        );
        let reply_content = bound_text_for_budget(
            &request.reply_content,
            maintenance_budget.reply_input_max_chars,
            maintenance_budget.reply_input_max_bytes,
        );
        let reuse_outcome_note = bound_text_for_budget(
            &request.reuse_outcome_note,
            maintenance_budget.reply_input_max_chars.min(1024),
            maintenance_budget.reply_input_max_bytes.min(2048),
        );
        let ctx = PostReplyMemoryMaintenanceContext {
            session_store: session_store.as_ref(),
            memory_store: memory_store.as_ref(),
            session_summary_store: session_summary_store.as_ref(),
            execution_state_store: execution_state_store.as_ref(),
            active_work_store: active_work_store.as_ref(),
            long_term_memory_store: long_term_memory_store.as_ref(),
            continuity_capsule_store: continuity_capsule_store.as_ref(),
            extraction_state_store: extraction_state_store.as_ref(),
            turn_ledger_store: turn_ledger_store.as_ref(),
            skill_storage: skill_storage.as_ref(),
            task_run_store: task_run_store.as_ref(),
            task_artifact_store: task_artifact_store.as_ref(),
            task_learning_store: task_learning_store.as_ref(),
        };
        let input = PostReplyMemoryMaintenanceInput {
            chat_id: &self.config.scope.chat_id,
            ingress: request.ingress,
            channel: &self.config.scope.channel,
            user_content: &user_content,
            reply_content: &reply_content,
            pressure: request.pressure,
            memory_profile: self.memory_profile(),
            tool_calls: request.tool_calls,
            external_content_used: request.external_content_used,
            prompt_recall_intent: PromptRecallIntent::Factual,
            runtime_skill_selected_ids: request.runtime_skill_selected_ids,
            task_learning_selected_ids: request.task_learning_selected_ids,
            reuse_outcome: request.reuse_outcome,
            reuse_outcome_note: &reuse_outcome_note,
            now_secs: self.config.clock.now_secs(),
        };
        let mut long_term_refresh_enqueued = false;
        let llm = self.config.llm.as_deref().unwrap_or(llm);
        let report = run_post_reply_memory_maintenance(http, llm, ctx, input, || {
            long_term_refresh_enqueued = true;
            true
        });
        long_term_refresh_enqueued = long_term_refresh_enqueued
            || matches!(
                report.extraction_request_outcome,
                bm_core::memory::LongTermMemoryRefreshRequestOutcome::Requested
            );
        self.audit("maintain", true, "maintenance_completed");
        let changed = report.after_count > 0
            || report.factual_refresh_suggested
            || !matches!(
                report.extraction_request_outcome,
                bm_core::memory::LongTermMemoryRefreshRequestOutcome::NotRequested
            );
        let maintenance_payload = [
            (
                "budget_report_id",
                self.config.runtime_budget.report_id.clone(),
            ),
            (
                "budget_limited_by",
                self.config.runtime_budget.limited_by.join(","),
            ),
            (
                "maintenance_user_max_chars",
                maintenance_budget.user_input_max_chars.to_string(),
            ),
            (
                "maintenance_reply_max_chars",
                maintenance_budget.reply_input_max_chars.to_string(),
            ),
        ];
        let lifecycle_report = self.finish_lifecycle_success_with_payload(
            lifecycle,
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::RunMaintenance,
            changed,
            "maintenance_completed",
            &maintenance_payload,
        )?;
        Ok(MemoryMaintenanceReport {
            report: Some(report),
            long_term_refresh_enqueued,
            lifecycle_report,
        })
    }

    pub fn finalize_turn_and_maintain(
        &self,
        http: Option<&mut (dyn LlmHttpClient + '_)>,
        llm: Option<&(dyn CoreLlmClient + Send + Sync + '_)>,
        request: MemoryTurnFinalizeRequest,
    ) -> Result<MemoryTurnFinalizeReport> {
        self.ensure_visible("write.turn", self.capabilities.write)?;
        validate_turn_scope(&self.config.scope, &self.config.subject_id, &request.turn)?;
        self.remember_conversation_id_from_delta(&request.turn)?;
        let platform = self.config.platform.as_ref();
        let session_store = platform.session_store();
        let transcript_store = platform.conversation_transcript_store();
        let core_report = commit_canonical_turn_delta_with_transcript(
            session_store.as_ref(),
            transcript_store.as_ref(),
            &self.config.memory_space_id,
            &request.turn,
            Vec::new(),
            self.config.clock.now_secs(),
        )?;
        let transcript_commit = core_report.transcript_commit;
        let transcript_committed = transcript_commit
            .as_ref()
            .is_some_and(|report| report.committed);
        let session_commit = core_report.session_commit;

        if !session_commit.committed && !transcript_committed {
            let lifecycle = self.start_lifecycle(
                RuntimeLifecycleOperation::Maintain,
                RuntimeLifecycleTrigger::PostReply,
                self.mode_input_for_request(request.mode_input, request.pressure),
            );
            let lifecycle_report = self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::Noop,
                false,
                session_commit
                    .skipped_reason
                    .as_deref()
                    .unwrap_or("turn_not_committed"),
            )?;
            return Ok(MemoryTurnFinalizeReport {
                session_commit,
                transcript_commit,
                maintenance: None,
                private_garden_self_work: PostTurnPrivateGardenReport::skipped(
                    "turn_not_committed",
                ),
                semantic_governance: PostTurnSemanticGovernanceReport::skipped(
                    "turn_not_committed",
                ),
                lifecycle_report,
            });
        }

        let Some(http) = http else {
            return self.finalize_turn_without_maintenance(
                session_commit,
                transcript_commit,
                &request,
                "maintenance_http_unavailable",
            );
        };
        let governance_llm: &(dyn CoreLlmClient + Send + Sync) =
            if let Some(config_llm) = self.config.llm.as_deref() {
                config_llm
            } else if let Some(llm) = llm {
                llm
            } else {
                return self.finalize_turn_without_maintenance(
                    session_commit,
                    transcript_commit,
                    &request,
                    "maintenance_llm_unavailable",
                );
            };
        if !self.capabilities.maintenance.visible {
            return self.finalize_turn_without_maintenance(
                session_commit,
                transcript_commit,
                &request,
                "maintenance_not_visible",
            );
        }

        let (maintenance, private_garden_self_work, semantic_governance) =
            self.run_post_turn_governance_after_commit(http, governance_llm, &request)?;
        let lifecycle_report = maintenance.lifecycle_report.clone();
        Ok(MemoryTurnFinalizeReport {
            session_commit,
            transcript_commit,
            maintenance: Some(maintenance),
            private_garden_self_work,
            semantic_governance,
            lifecycle_report,
        })
    }

    fn run_post_turn_governance_after_commit(
        &self,
        http: &mut (dyn LlmHttpClient + '_),
        governance_llm: &(dyn CoreLlmClient + Send + Sync + '_),
        request: &MemoryTurnFinalizeRequest,
    ) -> Result<(
        MemoryMaintenanceReport,
        PostTurnPrivateGardenReport,
        PostTurnSemanticGovernanceReport,
    )> {
        let platform = self.config.platform.as_ref();
        let session_store = self.transcript_backed_session_store(
            platform.session_store(),
            TranscriptReplayView::RawOwnerOnly,
        );
        let session_summary_store = platform.session_summary_store();
        let finalize_user_content = latest_user_content(&request.turn);
        let finalize_assistant_content = assistant_content(&request.turn);
        let turn_ingress = request.turn.source.ingress;
        let turn_channel = request.turn.source.channel.clone();
        let external_content_used = request.turn.external_content_used
            || request
                .turn
                .tool_observations
                .iter()
                .any(|observation| observation.external_content);
        let pressure = request.pressure;

        let maintenance = self.maintain(
            http,
            governance_llm,
            MemoryMaintenanceRequest {
                ingress: request.turn.source.ingress,
                user_content: finalize_user_content.clone(),
                reply_content: finalize_assistant_content.clone().unwrap_or_default(),
                tool_calls: request.tool_calls,
                external_content_used,
                runtime_skill_selected_ids: request.runtime_skill_selected_ids.clone(),
                task_learning_selected_ids: request.task_learning_selected_ids.clone(),
                reuse_outcome: RuntimeSkillReuseOutcome::Neutral,
                reuse_outcome_note: request.reuse_outcome_note.clone(),
                pressure: request.pressure,
                mode_input: request.mode_input,
            },
        )?;
        let execution_state_store = platform.execution_state_store();
        let self_model_store = platform.self_model_store();
        let private_doc_store = platform.private_doc_store();
        let private_garden_store = platform.private_garden_store();
        let planning_private_garden_store =
            PlanningPrivateGardenStore::new(private_garden_store.as_ref());
        let private_garden_self_work = match run_private_garden_governance(
            http,
            governance_llm,
            PrivateGardenGovernanceContext {
                session_store: session_store.as_ref(),
                session_summary_store: session_summary_store.as_ref(),
                execution_state_store: execution_state_store.as_ref(),
                self_model_store: self_model_store.as_ref(),
                private_doc_store: private_doc_store.as_ref(),
                private_garden_store: &planning_private_garden_store,
            },
            PrivateGardenGovernanceInput {
                chat_id: &self.config.scope.chat_id,
                ingress: turn_ingress,
                channel: &turn_channel,
                user_content: &finalize_user_content,
                reply_content: finalize_assistant_content.as_deref().unwrap_or_default(),
                pressure,
                tool_calls: request.tool_calls,
                now_secs: self.config.clock.now_secs(),
            },
            self.memory_profile(),
        )? {
            PrivateGardenGovernanceOutcome::Skipped => {
                PostTurnPrivateGardenReport::no_change("policy_skipped_or_no_private_change")
            }
            PrivateGardenGovernanceOutcome::Updated {
                writes,
                moves,
                deletes,
                manifest,
            } => {
                let mut mutations = planning_private_garden_store.into_mutations()?;
                mutations.extend(plan_private_garden_derived_memory_ref_mutations(
                    &self.config.memory_space_id,
                    &self.config.subject_id,
                    &request.turn,
                    &manifest,
                    self.config.clock.now_secs(),
                )?);
                if !mutations.is_empty() {
                    self.commit_memory_mutation_batch("post_turn.private_garden", mutations)?;
                }
                PostTurnPrivateGardenReport::applied_with_manifest(writes, moves, deletes, manifest)
            }
        };
        let memory_store = platform.memory_store();
        let long_term_memory_store = platform.long_term_memory_store();
        let extraction_state_store = platform.long_term_memory_extraction_state_store();
        let turn_ledger_store = platform.turn_ledger_store();
        let skill_storage = platform.skill_storage();
        let semantic_refresh_allowed = turn_ingress == IngressKind::User
            && turn_channel != "cron"
            && !external_content_used
            && maintenance.long_term_refresh_enqueued
            && !finalize_user_content.trim().is_empty()
            && finalize_assistant_content
                .as_deref()
                .is_some_and(|content| !content.trim().is_empty());
        let long_term_refresh = if semantic_refresh_allowed {
            let draft_admission_policy =
                self.long_term_draft_admission_policy(self.config.clock.now_secs())?;
            let outcome = self.run_long_term_memory_refresh_transactional(
                http,
                governance_llm,
                LongTermMemoryRefreshContext {
                    memory_store: memory_store.as_ref(),
                    session_store: session_store.as_ref(),
                    session_summary_store: session_summary_store.as_ref(),
                    long_term_memory_store: long_term_memory_store.as_ref(),
                    extraction_state_store: extraction_state_store.as_ref(),
                    turn_ledger_store: turn_ledger_store.as_ref(),
                    skill_storage: skill_storage.as_ref(),
                    draft_admission_policy: draft_admission_policy
                        .as_ref()
                        .map(|policy| policy as &dyn LongTermMemoryDraftAdmissionPolicy),
                },
                &self.config.scope.chat_id,
                pressure,
                self.memory_profile(),
            )?;
            Some(outcome)
        } else {
            None
        };
        let semantic_governance =
            semantic_report_from_maintenance(&maintenance, long_term_refresh.as_ref());
        Ok((maintenance, private_garden_self_work, semantic_governance))
    }

    fn finalize_turn_without_maintenance(
        &self,
        session_commit: bm_core::memory::SessionTurnCommitReport,
        transcript_commit: Option<bm_core::memory::TranscriptCommitReport>,
        request: &MemoryTurnFinalizeRequest,
        reason: &'static str,
    ) -> Result<MemoryTurnFinalizeReport> {
        enqueue_deferred_governance_job(
            self.config.platform.as_ref(),
            &self.config.scope,
            &session_commit,
            &self.config.memory_space_id,
            request,
            reason,
            self.config.clock.now_secs(),
        )?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Maintain,
            RuntimeLifecycleTrigger::PostReply,
            self.mode_input_for_request(request.mode_input, request.pressure),
        );
        let lifecycle_report = self.finish_lifecycle_success_with_payload(
            lifecycle,
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::Noop,
            session_commit.committed
                || transcript_commit
                    .as_ref()
                    .is_some_and(|report| report.committed),
            reason,
            &[
                ("finalize_request", "true".to_string()),
                ("finalize_committed", session_commit.committed.to_string()),
                (
                    "transcript_committed",
                    transcript_commit
                        .as_ref()
                        .is_some_and(|report| report.committed)
                        .to_string(),
                ),
                ("deferred_governance_job", "true".to_string()),
            ],
        )?;
        Ok(MemoryTurnFinalizeReport {
            session_commit,
            transcript_commit,
            maintenance: None,
            private_garden_self_work: PostTurnPrivateGardenReport::skipped(reason),
            semantic_governance: PostTurnSemanticGovernanceReport::deferred(
                reason,
                "post_turn_governance",
            ),
            lifecycle_report,
        })
    }

    pub fn run_due_governance(
        &self,
        http: &mut (dyn LlmHttpClient + '_),
        llm: Option<&(dyn CoreLlmClient + Send + Sync + '_)>,
        request: MemoryDeferredGovernanceRunRequest,
    ) -> Result<MemoryDeferredGovernanceRunReport> {
        self.ensure_visible("maintain", self.capabilities.maintenance)?;
        let governance_llm: &(dyn CoreLlmClient + Send + Sync) =
            if let Some(config_llm) = self.config.llm.as_deref() {
                config_llm
            } else {
                llm.ok_or_else(|| Error::config("deferred_governance", "llm_unavailable"))?
            };
        let limit = request.limit.max(1);
        let mut jobs = read_deferred_governance_jobs(self.config.platform.as_ref())?;
        let mut attempted = 0usize;
        let mut succeeded = 0usize;
        let mut failed = 0usize;

        for job in jobs.iter_mut() {
            if attempted >= limit {
                break;
            }
            if !matches!(
                job.status,
                DeferredGovernanceJobStatus::Pending | DeferredGovernanceJobStatus::Retrying
            ) || !deferred_governance_job_matches_runtime(job, &self.config)
            {
                continue;
            }
            attempted = attempted.saturating_add(1);
            let Some(turn) = job.turn.clone() else {
                failed = failed.saturating_add(1);
                job.status = DeferredGovernanceJobStatus::Failed;
                job.attempts = job.attempts.saturating_add(1);
                job.last_error = Some("missing_canonical_turn_delta".to_string());
                continue;
            };
            let finalize_request = MemoryTurnFinalizeRequest {
                turn,
                tool_calls: job.tool_calls,
                runtime_skill_selected_ids: job.runtime_skill_selected_ids.clone(),
                task_learning_selected_ids: job.task_learning_selected_ids.clone(),
                reuse_outcome_note: job.reuse_outcome_note.clone(),
                tool_usage_feedback: None,
                pressure: job.pressure,
                mode_input: job.mode_input,
            };
            match self.run_post_turn_governance_after_commit(
                http,
                governance_llm,
                &finalize_request,
            ) {
                Ok((_maintenance, _private_garden, _semantic)) => {
                    succeeded = succeeded.saturating_add(1);
                    job.status = DeferredGovernanceJobStatus::Terminal;
                    job.attempts = job.attempts.saturating_add(1);
                    job.last_error = None;
                }
                Err(error) => {
                    failed = failed.saturating_add(1);
                    job.status = DeferredGovernanceJobStatus::Retrying;
                    job.attempts = job.attempts.saturating_add(1);
                    job.last_error = Some(error.to_string());
                }
            }
        }
        let remaining_pending = jobs
            .iter()
            .filter(|job| {
                matches!(
                    job.status,
                    DeferredGovernanceJobStatus::Pending | DeferredGovernanceJobStatus::Retrying
                ) && deferred_governance_job_matches_runtime(job, &self.config)
            })
            .count();
        write_deferred_governance_jobs(self.config.platform.as_ref(), &jobs)?;
        let scoped_jobs = scoped_deferred_governance_jobs(&jobs, &self.config);
        let queue = build_deferred_governance_queue_report(&scoped_jobs, 16);
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Maintain,
            RuntimeLifecycleTrigger::DeferredDue,
            RuntimeLifecycleModeInput::default(),
        );
        let lifecycle_report = self.finish_lifecycle_success_with_payload(
            lifecycle,
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::RunMaintenance,
            attempted > 0,
            "deferred_governance_completed",
            &[
                ("attempted", attempted.to_string()),
                ("succeeded", succeeded.to_string()),
                ("failed", failed.to_string()),
                ("remaining_pending", remaining_pending.to_string()),
            ],
        )?;
        Ok(MemoryDeferredGovernanceRunReport {
            attempted,
            succeeded,
            failed,
            remaining_pending,
            queue,
            lifecycle_report,
        })
    }

    pub fn deferred_governance_report(&self) -> Result<DeferredGovernanceQueueReport> {
        self.ensure_visible("inspect.deferred_governance", self.capabilities.inspection)?;
        let jobs = read_deferred_governance_jobs(self.config.platform.as_ref())?;
        let scoped_jobs = scoped_deferred_governance_jobs(&jobs, &self.config);
        Ok(build_deferred_governance_queue_report(&scoped_jobs, 16))
    }

    pub fn run_retention_compaction(
        &self,
        request: MemoryRetentionCompactionRequest,
    ) -> Result<MemoryRetentionCompactionReport> {
        self.ensure_visible(
            "maintain.retention_compaction",
            self.capabilities.maintenance,
        )?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Maintain,
            RuntimeLifecycleTrigger::OperatorRequested,
            self.mode_input_for_request(request.mode_input, request.pressure),
        );
        if lifecycle.admission.disposition != RuntimeLifecycleDisposition::ExecuteNow {
            let lifecycle_report = self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::Noop,
                false,
                "retention_compaction_not_executed",
            )?;
            let retention_quota = self.retention_quota_report();
            return Ok(MemoryRetentionCompactionReport {
                owner: "sdk.runtime".to_string(),
                executed: false,
                hygiene: Default::default(),
                long_term_records_before: 0,
                long_term_records_after: 0,
                destructive_deletes_performed: false,
                host_direct_deletion_allowed: Some(false),
                fail_closed_repair: retention_quota.fail_closed_repair,
                retention_quota,
                lifecycle_report,
            });
        }
        let platform = self.config.platform.as_ref();
        let session_store = self.transcript_backed_session_store(
            platform.session_store(),
            TranscriptReplayView::OperatorAudit,
        );
        let memory_store = platform.memory_store();
        let session_summary_store = platform.session_summary_store();
        let turn_ledger_store = platform.turn_ledger_store();
        let long_term_memory_store = platform.long_term_memory_store();
        let skill_storage = platform.skill_storage();
        let before_count = long_term_memory_store.count().unwrap_or(0);
        let hygiene = run_memory_retention_compaction(
            MemoryHygieneContext {
                session_store: session_store.as_ref(),
                session_summary_store: session_summary_store.as_ref(),
                memory_store: memory_store.as_ref(),
                turn_ledger_store: turn_ledger_store.as_ref(),
                long_term_memory_store: long_term_memory_store.as_ref(),
                skill_storage: skill_storage.as_ref(),
            },
            &self.config.scope.chat_id,
            self.memory_profile(),
            self.config.clock.now_secs(),
        );
        let after_count = long_term_memory_store.count().unwrap_or(before_count);
        let changed = hygiene.daily_notes_aggregated > 0
            || hygiene.transcripts_rolled_up > 0
            || hygiene.factual_metadata_updates > 0
            || hygiene.factual_evidence_compacted > 0
            || hygiene.archive_index_maintained
            || hygiene.runtime_skill_governance.merged > 0
            || hygiene.runtime_skill_governance.pruned > 0
            || hygiene.runtime_skill_governance.stale_marked > 0
            || hygiene.runtime_skill_governance.low_value_marked > 0
            || hygiene.runtime_skill_governance.retired_marked > 0;
        let lifecycle_report = self.finish_lifecycle_success_with_payload(
            lifecycle,
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::RunMaintenance,
            changed,
            "retention_compaction_completed",
            &[
                ("long_term_records_before", before_count.to_string()),
                ("long_term_records_after", after_count.to_string()),
                (
                    "factual_evidence_compacted",
                    hygiene.factual_evidence_compacted.to_string(),
                ),
                ("host_direct_deletion_allowed", "false".to_string()),
            ],
        )?;
        let retention_quota = self.retention_quota_report();
        Ok(MemoryRetentionCompactionReport {
            owner: "sdk.runtime".to_string(),
            executed: true,
            retention_quota: retention_quota.clone(),
            hygiene,
            long_term_records_before: before_count,
            long_term_records_after: after_count,
            destructive_deletes_performed: false,
            host_direct_deletion_allowed: Some(false),
            fail_closed_repair: retention_quota.fail_closed_repair,
            lifecycle_report,
        })
    }

    pub fn inspect(&self, request: MemoryInspectionRequest) -> Result<MemoryInspectionReport> {
        self.ensure_visible("inspect", self.capabilities.inspection)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Inspect,
            RuntimeLifecycleTrigger::OperatorRequested,
            self.mode_input_for_request(request.mode_input, request.pressure),
        );
        let platform = self.config.platform.as_ref();
        let session_store = self.transcript_backed_session_store(
            platform.session_store(),
            TranscriptReplayView::OperatorAudit,
        );
        let memory_store = platform.memory_store();
        let session_summary_store = platform.session_summary_store();
        let long_term_memory_store = platform.long_term_memory_store();
        let active_work_store = platform.active_work_store();
        let continuity_capsule_store = platform.continuity_capsule_store();
        let turn_ledger_store = platform.turn_ledger_store();
        let skill_storage = platform.skill_storage();
        let task_run_store = platform.task_run_store();
        let task_learning_store = platform.task_learning_store();
        let recent = session_store.load_recent(&self.config.scope.chat_id, 16)?;
        let summary = session_summary_store.get(&self.config.scope.chat_id)?;
        let working = inspect_working_recall(WorkingRecallInspectionInput {
            chat_id: &self.config.scope.chat_id,
            query: &request.query,
            summary_text: summary.as_deref(),
            recent: &recent,
            system_max_len: self
                .config
                .runtime_budget
                .projection_source_budget
                .context_assembly_max_chars,
            profile: self.memory_profile(),
            current_channel: Some(&self.config.scope.channel),
            session_store: session_store.as_ref(),
            memory_store: memory_store.as_ref(),
            long_term_memory_store: long_term_memory_store.as_ref(),
            active_work_store: Some(active_work_store.as_ref()),
            continuity_capsule_store: continuity_capsule_store.as_ref(),
            turn_ledger_store: turn_ledger_store.as_ref(),
            skill_storage: Some(skill_storage.as_ref()),
            task_run_store: Some(task_run_store.as_ref()),
            task_learning_store: Some(task_learning_store.as_ref()),
        });
        let hygiene = inspect_memory_hygiene(
            MemoryHygieneContext {
                session_store: session_store.as_ref(),
                session_summary_store: session_summary_store.as_ref(),
                memory_store: memory_store.as_ref(),
                turn_ledger_store: turn_ledger_store.as_ref(),
                long_term_memory_store: long_term_memory_store.as_ref(),
                skill_storage: skill_storage.as_ref(),
            },
            &self.config.scope.chat_id,
            self.memory_profile(),
            self.config.clock.now_secs(),
        );
        let deferred_jobs = read_deferred_governance_jobs(platform)?;
        let scoped_deferred_jobs = scoped_deferred_governance_jobs(&deferred_jobs, &self.config);
        let deferred_governance = build_deferred_governance_queue_report(&scoped_deferred_jobs, 16);
        self.audit("inspect", true, "inspection_completed");
        let surface = bm_core::platform::build_memory_operator_surface_with_capabilities(
            platform,
            self.capabilities.export.visible || self.capabilities.import.visible,
            None,
        )?;
        let diagnosis = build_runtime_lifecycle_diagnosis(&surface);
        let lifecycle_report = self.finish_lifecycle_success(
            lifecycle,
            RuntimeLifecycleEventKind::OperatorAction,
            RuntimeLifecycleEffect::Inspect,
            false,
            "inspection_completed",
        )?;
        let safe_actions_available = diagnosis.safe_actions_available.clone();
        let operator_action_report = RuntimeOperatorActionReport {
            action: RuntimeOperatorAction::InspectMemoryStatus,
            accepted: true,
            lifecycle: lifecycle_report.clone(),
            surface,
            diagnosis,
            safe_actions_available,
        };
        Ok(MemoryInspectionReport {
            working,
            hygiene,
            deferred_governance,
            agent_skill_directory: self.config.agent_skill_registry.report(),
            agent_tool_registry: self.agent_tool_registry_report()?,
            capabilities: self.capabilities.clone(),
            operator_action_report,
            lifecycle_report,
        })
    }

    pub fn replay(&self, request: MemoryReplayRequest) -> Result<MemoryReplayReport> {
        self.ensure_visible("replay", self.capabilities.replay)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Replay,
            RuntimeLifecycleTrigger::ReplayInspection,
            RuntimeLifecycleModeInput::default(),
        );
        let turn_ledger_store = self.config.platform.turn_ledger_store();
        let inspection = inspect_intelligence_replay(
            turn_ledger_store.as_ref(),
            &request.chat_id,
            request.limit.max(1),
        )?;
        self.audit("replay", true, "replay_completed");
        Ok(MemoryReplayReport {
            chat_id: inspection.chat_id.clone(),
            inspection,
            lifecycle_report: self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::RunReplayInspection,
                false,
                "replay_completed",
            )?,
        })
    }

    pub fn commit_transcript(
        &self,
        request: MemoryTranscriptCommitRequest,
    ) -> Result<MemoryTranscriptCommitReport> {
        self.ensure_visible("write.transcript", self.capabilities.write)?;
        validate_turn_scope(&self.config.scope, &self.config.subject_id, &request.turn)?;
        self.remember_conversation_id_from_delta(&request.turn)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Maintain,
            RuntimeLifecycleTrigger::SdkCall,
            RuntimeLifecycleModeInput::default(),
        );
        let key = ConversationKey::from_delta(&self.config.memory_space_id, &request.turn)?;
        let platform = self.config.platform.as_ref();
        let session_store = platform.session_store();
        let transcript_store = platform.conversation_transcript_store();
        let core_report = commit_canonical_turn_delta_with_transcript(
            session_store.as_ref(),
            transcript_store.as_ref(),
            &self.config.memory_space_id,
            &request.turn,
            request.host_refs,
            self.config.clock.now_secs(),
        )?;
        let changed = core_report.session_commit.committed
            || core_report
                .transcript_commit
                .as_ref()
                .is_some_and(|report| report.committed);
        self.audit("write.transcript", true, "transcript_commit_completed");
        Ok(MemoryTranscriptCommitReport {
            key,
            session_commit: core_report.session_commit,
            transcript_commit: core_report.transcript_commit,
            lifecycle_report: self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::RunMaintenance,
                changed,
                "transcript_commit_completed",
            )?,
        })
    }

    pub fn replay_transcript(
        &self,
        request: MemoryTranscriptReplayRequest,
    ) -> Result<MemoryTranscriptReplayReport> {
        self.ensure_visible(
            "replay.transcript",
            self.transcript_replay_visibility(request.view),
        )?;
        self.ensure_runtime_memory_space("replay.transcript", &request.memory_space_id)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Replay,
            RuntimeLifecycleTrigger::ReplayInspection,
            RuntimeLifecycleModeInput::default(),
        );
        let key = ConversationKey::new(
            request.memory_space_id,
            request.channel_id,
            request.conversation_id,
        )?;
        let transcript_store = self.config.platform.conversation_transcript_store();
        let limit = transcript_replay_limit(&self.config.runtime_budget, request.limit);
        let (mut slice, next_cursor, has_more) = transcript_store.redacted_replay_page(
            &key,
            request.cursor.as_deref(),
            limit,
            request.view,
        )?;
        apply_transcript_governance_budget_to_slice(
            &mut slice,
            self.config.runtime_budget.transcript_governance_budget,
        );
        self.audit("replay.transcript", true, "transcript_replay_completed");
        Ok(MemoryTranscriptReplayReport {
            slice,
            next_cursor,
            has_more,
            lifecycle_report: self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::RunReplayInspection,
                false,
                "transcript_replay_completed",
            )?,
        })
    }

    pub fn record_transcript_attrs(
        &self,
        request: MemoryTranscriptAttrWriteRequest,
    ) -> Result<MemoryTranscriptAttrWriteReport> {
        self.ensure_visible("write.transcript_attrs", self.capabilities.write)?;
        self.ensure_runtime_memory_space("write.transcript_attrs", &request.memory_space_id)?;
        if request
            .idempotency_key
            .as_deref()
            .is_some_and(|key| key.trim().is_empty())
        {
            return Err(Error::config(
                "write.transcript_attrs",
                "idempotency_key must not be empty when provided",
            ));
        }
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Maintain,
            RuntimeLifecycleTrigger::SdkCall,
            RuntimeLifecycleModeInput::default(),
        );
        let key = ConversationKey::new(
            request.memory_space_id,
            request.channel_id,
            request.conversation_id,
        )?;
        let transcript_store = self.config.platform.conversation_transcript_store();
        let transcript = if request.dry_run {
            validate_transcript_attr_write_request(transcript_store.as_ref(), &key, &request.attrs)?
        } else {
            transcript_store.upsert_transcript_attrs(&key, &request.attrs)?
        };
        let (redactions_preview, profile_budget_applied) = transcript_attr_write_redactions_preview(
            &request.attrs,
            &transcript.rejected_attrs,
            self.config
                .runtime_budget
                .transcript_governance_budget
                .redaction_items_per_page,
        );
        let changed = !request.dry_run && !transcript.accepted_attrs.is_empty();
        let reason = if request.dry_run {
            "transcript_attrs_dry_run_completed"
        } else {
            "transcript_attrs_recorded"
        };
        self.audit("write.transcript_attrs", true, reason);
        let lifecycle_report = self.finish_lifecycle_success(
            lifecycle,
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::RunMaintenance,
            changed,
            reason,
        )?;
        Ok(MemoryTranscriptAttrWriteReport {
            key: transcript.key,
            accepted_attrs: transcript.accepted_attrs,
            rejected_attrs: transcript.rejected_attrs,
            redactions_preview,
            profile_budget_applied,
            audit_event_id: Some(lifecycle_report.event_id.clone()),
            dry_run: request.dry_run,
            lifecycle_report,
        })
    }

    pub fn export_transcript(
        &self,
        request: MemoryTranscriptExportRequest,
    ) -> Result<MemoryTranscriptExportReport> {
        self.ensure_visible("export.transcript", self.capabilities.export)?;
        self.ensure_runtime_memory_space("export.transcript", &request.memory_space_id)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Export,
            RuntimeLifecycleTrigger::SnapshotTransfer,
            RuntimeLifecycleModeInput::default(),
        );
        let key = ConversationKey::new(
            request.memory_space_id,
            request.channel_id,
            request.conversation_id,
        )?;
        let transcript_store = self.config.platform.conversation_transcript_store();
        let limit = transcript_replay_limit(&self.config.runtime_budget, request.limit);
        let (mut slice, next_cursor, has_more) = transcript_store.redacted_replay_page(
            &key,
            request.cursor.as_deref(),
            limit,
            TranscriptReplayView::Export,
        )?;
        apply_transcript_governance_budget_to_slice(
            &mut slice,
            self.config.runtime_budget.transcript_governance_budget,
        );
        self.audit("export.transcript", true, "transcript_export_completed");
        Ok(MemoryTranscriptExportReport {
            slice,
            next_cursor,
            has_more,
            lifecycle_report: self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::ExportSnapshot,
                false,
                "transcript_export_completed",
            )?,
        })
    }

    pub fn request_transcript_lifecycle(
        &self,
        request: MemoryTranscriptLifecycleRequest,
    ) -> Result<MemoryTranscriptLifecycleReport> {
        self.ensure_visible("transcript.lifecycle", self.capabilities.write)?;
        self.ensure_runtime_memory_space("transcript.lifecycle", &request.memory_space_id)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Maintain,
            RuntimeLifecycleTrigger::OperatorRequested,
            RuntimeLifecycleModeInput::default(),
        );
        let key = ConversationKey::new(
            request.memory_space_id,
            request.channel_id,
            request.conversation_id,
        )?;
        self.ensure_transcript_lifecycle_has_facet_impact_or_fails_closed(
            &key,
            request.turn_id.as_deref(),
            request.transition,
        )?;
        let transcript_store = self.config.platform.conversation_transcript_store();
        let mut transcript =
            transcript_store.apply_lifecycle_request(&CoreTranscriptLifecycleRequest {
                key,
                turn_id: request.turn_id,
                transition: request.transition,
                reason: request.reason,
                requested_by: self.config.identity.owner_id.clone(),
                requested_at: self.config.clock.now_secs(),
            })?;
        sanitize_transcript_lifecycle_report_for_view(
            &mut transcript,
            TranscriptReplayView::OperatorAudit,
        );
        apply_transcript_lifecycle_budget(
            &mut transcript,
            self.config.runtime_budget.transcript_governance_budget,
        );
        let transcript_changed = transcript.affected_turns > 0;
        self.audit(
            "transcript.lifecycle",
            true,
            "transcript_lifecycle_completed",
        );
        Ok(MemoryTranscriptLifecycleReport {
            transcript,
            lifecycle_report: self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::RunMaintenance,
                transcript_changed,
                "transcript_lifecycle_completed",
            )?,
        })
    }

    pub fn repair_transcript(
        &self,
        request: MemoryTranscriptRepairRequest,
    ) -> Result<MemoryTranscriptRepairReport> {
        self.ensure_visible("repair.transcript", self.capabilities.inspection)?;
        self.ensure_runtime_memory_space("repair.transcript", &request.memory_space_id)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Inspect,
            RuntimeLifecycleTrigger::ReplayInspection,
            RuntimeLifecycleModeInput::default(),
        );
        let key = ConversationKey::new(
            request.memory_space_id,
            request.channel_id,
            request.conversation_id,
        )?;
        let transcript_store = self.config.platform.conversation_transcript_store();
        let mut transcript = transcript_store.repair_report(&key)?;
        apply_transcript_repair_budget(
            &mut transcript,
            self.config.runtime_budget.transcript_governance_budget,
        );
        self.audit("repair.transcript", true, "transcript_repair_completed");
        Ok(MemoryTranscriptRepairReport {
            transcript,
            lifecycle_report: self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::Inspect,
                false,
                "transcript_repair_completed",
            )?,
        })
    }

    pub fn export(&self, request: MemoryExportRequest) -> Result<MemoryExportReport> {
        self.ensure_visible("export", self.capabilities.export)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Export,
            RuntimeLifecycleTrigger::SnapshotTransfer,
            RuntimeLifecycleModeInput::default(),
        );
        let platform = self.config.platform.as_ref();
        let long_term_memory_store = platform.long_term_memory_store();
        let session_summary_store = platform.session_summary_store();
        let execution_state_store = platform.execution_state_store();
        let self_model_store = platform.self_model_store();
        let self_authored_core_store = platform.self_authored_core_store();
        let core_revision_ledger_store = platform.core_revision_ledger_store();
        let self_continuity_store = platform.self_continuity_store();
        let relationship_constitution_store = platform.relationship_constitution_store();
        let relationship_portfolio_store = platform.relationship_portfolio_store();
        let relationship_topology_store = platform.relationship_topology_store();
        let ctx = ContinuitySnapshotExportContext {
            long_term_memory_store: long_term_memory_store.as_ref(),
            session_summary_store: session_summary_store.as_ref(),
            execution_state_store: execution_state_store.as_ref(),
            self_model_store: self_model_store.as_ref(),
            self_authored_core_store: self_authored_core_store.as_ref(),
            core_revision_ledger_store: core_revision_ledger_store.as_ref(),
            self_continuity_store: self_continuity_store.as_ref(),
            relationship_constitution_store: relationship_constitution_store.as_ref(),
            relationship_portfolio_store: relationship_portfolio_store.as_ref(),
            relationship_topology_store: relationship_topology_store.as_ref(),
        };
        let snapshot = export_continuity_snapshot(
            ctx,
            &request.chat_id,
            ContinuitySnapshotMode::FullRestore,
            self.config.clock.now_secs(),
        )?;
        self.audit("export", true, "export_completed");
        Ok(MemoryExportReport {
            snapshot,
            lifecycle_report: self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::ExportSnapshot,
                false,
                "export_completed",
            )?,
        })
    }

    pub fn import(&self, request: MemoryImportRequest) -> Result<MemoryImportReport> {
        self.ensure_visible("import", self.capabilities.import)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Import,
            RuntimeLifecycleTrigger::SnapshotTransfer,
            RuntimeLifecycleModeInput::default(),
        );
        let platform = self.config.platform.as_ref();
        let long_term_memory_store = platform.long_term_memory_store();
        let session_summary_store = platform.session_summary_store();
        let execution_state_store = platform.execution_state_store();
        let self_model_store = platform.self_model_store();
        let self_authored_core_store = platform.self_authored_core_store();
        let core_revision_ledger_store = platform.core_revision_ledger_store();
        let self_continuity_store = platform.self_continuity_store();
        let relationship_constitution_store = platform.relationship_constitution_store();
        let relationship_portfolio_store = platform.relationship_portfolio_store();
        let ctx = ContinuitySnapshotImportContext {
            long_term_memory_store: long_term_memory_store.as_ref(),
            session_summary_store: session_summary_store.as_ref(),
            execution_state_store: execution_state_store.as_ref(),
            self_model_store: self_model_store.as_ref(),
            self_authored_core_store: self_authored_core_store.as_ref(),
            core_revision_ledger_store: core_revision_ledger_store.as_ref(),
            self_continuity_store: self_continuity_store.as_ref(),
            relationship_constitution_store: relationship_constitution_store.as_ref(),
            relationship_portfolio_store: relationship_portfolio_store.as_ref(),
        };
        let outcome = import_continuity_snapshot(
            ctx,
            &request.target_chat_id,
            &request.snapshot,
            request.mode,
        )?;
        self.audit("import", true, "import_completed");
        Ok(MemoryImportReport {
            outcome,
            lifecycle_report: self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::ImportSnapshot,
                true,
                "import_completed",
            )?,
        })
    }

    pub fn export_memory_space(
        &self,
        request: MemorySpaceExportRequest,
    ) -> Result<MemorySpaceExportReport> {
        self.ensure_visible("export.memory_space", self.capabilities.export)?;
        let platform = self.store_platform_for_memory_space("export.memory_space")?;
        let report = crate::export_memory_space(platform, request)?;
        self.audit("export.memory_space", true, "memory_space_export_completed");
        Ok(report)
    }

    pub fn import_memory_space(
        &self,
        request: MemorySpaceImportRequest,
    ) -> Result<MemorySpaceImportReport> {
        self.ensure_visible("import.memory_space", self.capabilities.import)?;
        let platform = self.store_platform_for_memory_space("import.memory_space")?;
        let report = crate::import_memory_space(platform, request)?;
        self.audit("import.memory_space", true, "memory_space_import_completed");
        Ok(report)
    }

    pub fn preview_memory_space_migration(
        &self,
        request: MemorySpaceMigratePreviewRequest,
    ) -> Result<MemorySpaceMigratePreviewReport> {
        self.ensure_visible("preview.memory_space", self.capabilities.export)?;
        let report = crate::preview_memory_space_migration(request);
        self.audit(
            "preview.memory_space",
            report.vault_preflight.passed,
            "memory_space_migration_preview_completed",
        );
        Ok(report)
    }

    pub fn apply_memory_space_migration(
        &self,
        request: MemorySpaceMigrateApplyRequest,
    ) -> Result<MemorySpaceMigrateApplyReport> {
        self.ensure_visible("apply.memory_space", self.capabilities.import)?;
        let platform = self.store_platform_for_memory_space("apply.memory_space")?;
        let report = crate::apply_memory_space_migration(platform, request)?;
        self.audit(
            "apply.memory_space",
            true,
            "memory_space_migration_apply_completed",
        );
        Ok(report)
    }

    pub fn recover(&self, request: MemoryRecoverRequest) -> Result<MemoryRecoverReport> {
        self.ensure_visible("recover", self.capabilities.lifecycle.recover)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Recover,
            request.trigger,
            self.mode_input_for_request(request.mode_input, PressureLevel::Normal),
        );
        let report = ensure_platform_soul_kernel_recovery(
            self.config.platform.as_ref(),
            self.config.clock.now_secs(),
        );
        let changed = report.restore_attempted && !report.restored_layers.is_empty();
        let lifecycle_report = self.finish_lifecycle_success(
            lifecycle,
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::RecoverSoulKernel,
            changed,
            format!(
                "recover action={:?} restored_layers={}",
                report.action,
                report.restored_layers.len()
            ),
        )?;
        Ok(MemoryRecoverReport {
            report,
            lifecycle_report,
        })
    }

    pub fn close(&self, request: MemoryCloseRequest) -> Result<MemoryCloseReport> {
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Close,
            RuntimeLifecycleTrigger::SdkCall,
            RuntimeLifecycleModeInput::default(),
        );
        let lifecycle_report = self.finish_lifecycle_success(
            lifecycle,
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::Noop,
            false,
            if request.reason.trim().is_empty() {
                "close_requested"
            } else {
                request.reason.trim()
            },
        )?;
        Ok(MemoryCloseReport { lifecycle_report })
    }

    fn mode_input_for_request(
        &self,
        mut input: RuntimeLifecycleModeInput,
        pressure: PressureLevel,
    ) -> RuntimeLifecycleModeInput {
        input.profile = self.config.profile;
        input.pressure = max_pressure(
            pressure,
            self.config.runtime_budget.resource_snapshot.pressure,
        );
        input
    }

    fn store_platform_for_memory_space(&self, operation: &'static str) -> Result<&StorePlatform> {
        self.config.store_platform.as_ref().ok_or_else(|| {
            Error::config(
                operation,
                "memory-space snapshot operations require StorePlatform-backed runtime",
            )
        })
    }

    fn start_lifecycle(
        &self,
        operation: RuntimeLifecycleOperation,
        trigger: RuntimeLifecycleTrigger,
        mut input: RuntimeLifecycleModeInput,
    ) -> RuntimeLifecycleReport {
        input.profile = self.config.profile;
        let admission = self.lifecycle.admit(operation, trigger, input);
        RuntimeLifecycleReport::from_admission(admission, self.config.clock.now_secs())
    }

    fn finish_lifecycle_success(
        &self,
        report: RuntimeLifecycleReport,
        kind: RuntimeLifecycleEventKind,
        effect: RuntimeLifecycleEffect,
        changed: bool,
        summary: impl Into<String>,
    ) -> Result<RuntimeLifecycleReport> {
        self.finish_lifecycle_success_with_payload(report, kind, effect, changed, summary, &[])
    }

    fn finish_lifecycle_success_with_payload(
        &self,
        report: RuntimeLifecycleReport,
        kind: RuntimeLifecycleEventKind,
        effect: RuntimeLifecycleEffect,
        changed: bool,
        summary: impl Into<String>,
        extra_payload: &[(&str, String)],
    ) -> Result<RuntimeLifecycleReport> {
        let finished = report.finish_success(self.config.clock.now_secs(), changed, summary);
        self.record_lifecycle_event(kind, effect, &finished, extra_payload)?;
        Ok(finished)
    }

    fn planned_lifecycle_store_event(
        &self,
        transaction_id: &str,
        transaction_operation: &str,
        kind: RuntimeLifecycleEventKind,
        effect: RuntimeLifecycleEffect,
        report: &RuntimeLifecycleReport,
        extra_payload: &[(&str, String)],
    ) -> Result<MemoryStoreEvent> {
        let event = self.build_lifecycle_event(kind, effect, report, extra_payload);
        let event_value = serde_json::to_value(&event)
            .map_err(|error| Error::config("runtime_lifecycle_event", error.to_string()))?;
        let content_hash = stable_hash_json(&event_value)?;
        let kind = match event.kind {
            RuntimeLifecycleEventKind::RuntimeLifecycle => MemoryStoreEventKind::RuntimeLifecycle,
            RuntimeLifecycleEventKind::OperatorAction => MemoryStoreEventKind::OperatorAction,
        };
        let mut store_event = MemoryStoreEvent::new(
            event.event_id,
            kind,
            StoreEventScope::system(event.operation.as_str()),
            event.timestamp_unix_secs,
        )
        .with_plane("runtime_lifecycle")
        .with_record_key(event.operation.as_str())
        .with_content_hash(content_hash)
        .with_payload("runtime_operation", event.operation.as_str())
        .with_payload("operation", transaction_operation)
        .with_payload("trigger", event.trigger.as_str())
        .with_payload("disposition", event.disposition.as_str())
        .with_payload("effect", event.effect.as_str())
        .with_payload("profile", event.profile.as_str())
        .with_payload("mode", event.mode.as_str())
        .with_payload(
            "pressure",
            format!("{:?}", event.pressure).to_ascii_lowercase(),
        )
        .with_payload("reason", event.reason)
        .with_payload("result", event.result)
        .with_payload("error_stage", event.error_stage.unwrap_or_default())
        .with_payload("transaction_id", transaction_id);
        for (key, value) in event.payload {
            if key == "operation" || key == "transaction_id" {
                continue;
            }
            store_event = store_event.with_payload(key, value);
        }
        Ok(store_event)
    }

    fn build_lifecycle_event(
        &self,
        kind: RuntimeLifecycleEventKind,
        effect: RuntimeLifecycleEffect,
        report: &RuntimeLifecycleReport,
        extra_payload: &[(&str, String)],
    ) -> RuntimeLifecycleEvent {
        let mut event =
            RuntimeLifecycleEvent::from_report(kind, effect, report, self.config.clock.now_secs())
                .with_payload("changed", report.changed.to_string())
                .with_payload("success", report.success.to_string())
                .with_payload("result_summary", report.result_summary.clone())
                .with_payload(
                    "retry_after_ms",
                    report
                        .admission
                        .retry_after_ms
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                )
                .with_payload(
                    "lightweight_allowed",
                    report.admission.lightweight_allowed.to_string(),
                )
                .with_payload(
                    "private_depth_allowed",
                    report.admission.private_depth_allowed.to_string(),
                )
                .with_payload(
                    "budget_report_id",
                    self.config.runtime_budget.report_id.clone(),
                )
                .with_payload(
                    "resource_source",
                    self.config.runtime_budget.resource_snapshot.source.as_str(),
                )
                .with_payload(
                    "budget_limited_by",
                    self.config.runtime_budget.limited_by.join(","),
                )
                .with_payload(
                    "budget_unavailable_reasons",
                    self.config.runtime_budget.unavailable_reasons.join(","),
                )
                .with_payload("agent_id", self.config.identity.agent_id.clone())
                .with_payload("owner_id", self.config.identity.owner_id.clone())
                .with_payload("channel", self.config.scope.channel.clone())
                .with_payload("chat_id", self.config.scope.chat_id.clone())
                .with_payload("memory_space_id", self.config.memory_space_id.clone())
                .with_payload("subject_id", self.config.subject_id.clone())
                .with_payload(
                    "mounted_subject_id",
                    self.config.scoped_runtime.mounted_subject_id.clone(),
                )
                .with_payload(
                    "actor_subject_id",
                    self.config.scoped_runtime.actor_subject_id.clone(),
                )
                .with_payload("conversation_id", self.config.scope.chat_id.clone())
                .with_payload(
                    "projection_source_max_chars",
                    self.config
                        .runtime_budget
                        .projection_source_budget
                        .context_assembly_max_chars
                        .to_string(),
                )
                .with_payload(
                    "projection_render_max_chars",
                    self.config
                        .runtime_budget
                        .projection_render_budget
                        .system_block_max_chars
                        .to_string(),
                );
        if kind == RuntimeLifecycleEventKind::OperatorAction {
            event = event
                .with_payload(
                    "action",
                    RuntimeOperatorAction::InspectMemoryStatus.as_str(),
                )
                .with_payload("accepted", report.success.to_string());
        }
        for (key, value) in extra_payload {
            event = event.with_payload(*key, value.clone());
        }
        event
    }

    fn record_lifecycle_event(
        &self,
        kind: RuntimeLifecycleEventKind,
        effect: RuntimeLifecycleEffect,
        report: &RuntimeLifecycleReport,
        extra_payload: &[(&str, String)],
    ) -> Result<()> {
        let event = self.build_lifecycle_event(kind, effect, report, extra_payload);
        self.config
            .platform
            .runtime_lifecycle_event_sink()
            .record_lifecycle_event(event)
    }

    fn ensure_visible(
        &self,
        operation: &'static str,
        visibility: MemoryOperationVisibility,
    ) -> Result<()> {
        if visibility.visible {
            return Ok(());
        }
        self.audit(operation, false, "operation_not_visible_for_profile");
        Err(Error::config(
            "memory_runtime_operation",
            format!("{operation} is not visible for current profile"),
        ))
    }

    fn transcript_replay_visibility(
        &self,
        view: TranscriptReplayView,
    ) -> MemoryOperationVisibility {
        match view {
            TranscriptReplayView::HostUi => self.capabilities.transcript_replay,
            TranscriptReplayView::ModelContext => self.capabilities.projection,
            TranscriptReplayView::OperatorAudit => self.capabilities.inspection,
            TranscriptReplayView::Export | TranscriptReplayView::RawOwnerOnly => {
                self.capabilities.export
            }
        }
    }

    fn long_term_mutation_visibility(
        &self,
        operation: &MemoryLongTermMutation,
    ) -> MemoryOperationVisibility {
        match operation {
            MemoryLongTermMutation::ForgetByQuery { .. } => {
                self.capabilities.long_term_control_bulk_forget
            }
            MemoryLongTermMutation::Correct { .. }
            | MemoryLongTermMutation::Supersede { .. }
            | MemoryLongTermMutation::Delete { .. }
            | MemoryLongTermMutation::MarkStale { .. }
            | MemoryLongTermMutation::ChangeScope { .. } => {
                self.capabilities.long_term_control_mutation
            }
        }
    }

    fn filter_long_term_draft_pairs_by_policy<'a>(
        &self,
        pairs: Vec<(&'a MemoryWriteCandidate, LongTermMemoryDraft)>,
        now_secs: u64,
    ) -> Result<(
        Vec<(&'a MemoryWriteCandidate, LongTermMemoryDraft)>,
        Vec<String>,
        usize,
    )> {
        if pairs.is_empty() {
            return Ok((pairs, Vec::new(), 0));
        }
        let control_store = self.config.platform.long_term_memory_control_store();
        let policies = control_store.list_long_term_governance_policies(usize::MAX)?;
        if policies.is_empty() {
            return Ok((pairs, Vec::new(), 0));
        }
        let mut kept = Vec::with_capacity(pairs.len());
        let mut policy_ids = HashSet::new();
        let mut blocked = 0usize;
        for (candidate, draft) in pairs {
            if let Some(policy) = self.matching_long_term_write_policy(&policies, &draft, now_secs)
            {
                blocked += 1;
                policy_ids.insert(policy.policy_id.clone());
                continue;
            }
            kept.push((candidate, draft));
        }
        let mut policy_ids = policy_ids.into_iter().collect::<Vec<_>>();
        policy_ids.sort();
        Ok((kept, policy_ids, blocked))
    }

    fn filter_long_term_drafts_by_policy(
        &self,
        drafts: Vec<LongTermMemoryDraft>,
        now_secs: u64,
    ) -> Result<(Vec<LongTermMemoryDraft>, Vec<String>, usize)> {
        if drafts.is_empty() {
            return Ok((drafts, Vec::new(), 0));
        }
        let control_store = self.config.platform.long_term_memory_control_store();
        let policies = control_store.list_long_term_governance_policies(usize::MAX)?;
        if policies.is_empty() {
            return Ok((drafts, Vec::new(), 0));
        }
        let mut kept = Vec::with_capacity(drafts.len());
        let mut policy_ids = HashSet::new();
        let mut blocked = 0usize;
        for draft in drafts {
            if let Some(policy) = self.matching_long_term_write_policy(&policies, &draft, now_secs)
            {
                blocked += 1;
                policy_ids.insert(policy.policy_id.clone());
                continue;
            }
            kept.push(draft);
        }
        let mut policy_ids = policy_ids.into_iter().collect::<Vec<_>>();
        policy_ids.sort();
        Ok((kept, policy_ids, blocked))
    }

    fn matching_long_term_write_policy<'a>(
        &self,
        policies: &'a [MemoryLongTermGovernancePolicy],
        draft: &LongTermMemoryDraft,
        now_secs: u64,
    ) -> Option<&'a MemoryLongTermGovernancePolicy> {
        let normalized = draft.normalized()?;
        let source_scope = runtime_inferred_long_term_source_scope(&normalized);
        let subject_ids = [
            self.config.subject_id.as_str(),
            self.config.scoped_runtime.mounted_subject_id.as_str(),
            self.config.scoped_runtime.actor_subject_id.as_str(),
        ];
        policies.iter().find(|policy| {
            long_term_governance_policy_blocks_future_write(policy, now_secs)
                && subject_ids.iter().any(|subject_id| {
                    policy.matches_candidate(
                        Some(self.config.memory_space_id.as_str()),
                        Some(*subject_id),
                        &normalized.kind,
                        &normalized.topic,
                        normalized.source_chat_id.as_deref(),
                        source_scope,
                    )
                })
        })
    }

    fn long_term_draft_admission_policy(
        &self,
        now_secs: u64,
    ) -> Result<Option<RuntimeLongTermDraftAdmissionPolicy>> {
        let control_store = self.config.platform.long_term_memory_control_store();
        let policies = control_store.list_long_term_governance_policies(usize::MAX)?;
        if policies
            .iter()
            .all(|policy| !long_term_governance_policy_blocks_future_write(policy, now_secs))
        {
            return Ok(None);
        }
        let mut subject_ids = vec![
            self.config.subject_id.clone(),
            self.config.scoped_runtime.mounted_subject_id.clone(),
            self.config.scoped_runtime.actor_subject_id.clone(),
        ];
        subject_ids.sort();
        subject_ids.dedup();
        Ok(Some(RuntimeLongTermDraftAdmissionPolicy {
            memory_space_id: self.config.memory_space_id.clone(),
            subject_ids,
            policies,
            now_secs,
        }))
    }

    fn ensure_runtime_memory_space(
        &self,
        operation: &'static str,
        memory_space_id: &str,
    ) -> Result<()> {
        if memory_space_id.trim() == self.config.memory_space_id {
            return Ok(());
        }
        self.audit(operation, false, "memory_space_id_mismatch");
        Err(Error::config(
            "memory_runtime_memory_space",
            format!("{operation} memory_space_id must match runtime memory_space_id"),
        ))
    }

    fn audit(&self, operation: &str, allowed: bool, reason: &str) {
        self.config
            .audit_sink
            .record(MemoryAuditEvent::for_scoped_runtime_operation(
                operation,
                self.config.profile,
                self.config.identity.clone(),
                self.config.scope.clone(),
                self.config.memory_space_id.clone(),
                self.config.subject_id.clone(),
                allowed,
                reason,
            ));
    }

    fn memory_profile(&self) -> MemoryProfile {
        match self.config.profile {
            ProfileId::EspStandaloneMemory | ProfileId::EspEmbeddedSdk => MemoryProfile::Embedded,
            ProfileId::LinuxDeviceStandaloneMemory
            | ProfileId::DesktopMacosStandaloneMemory
            | ProfileId::DesktopMacosEmbeddedSdk
            | ProfileId::DesktopWindowsEmbeddedSdk
            | ProfileId::ServerLinuxMemoryGateway
            | ProfileId::ServerLinuxDevFull => MemoryProfile::Standard,
        }
    }

    fn prompt_participation_plan(&self) -> PromptParticipationPlan {
        match self.memory_profile().memory_system_kind() {
            MemoryRuntimeSystemKind::EspCompact => {
                PromptParticipationPlan::embedded_first_turn_default()
            }
            MemoryRuntimeSystemKind::LinuxFull => PromptParticipationPlan::full(),
        }
    }
}

struct RuntimeLongTermDraftAdmissionPolicy {
    memory_space_id: String,
    subject_ids: Vec<String>,
    policies: Vec<MemoryLongTermGovernancePolicy>,
    now_secs: u64,
}

impl LongTermMemoryDraftAdmissionPolicy for RuntimeLongTermDraftAdmissionPolicy {
    fn accepts_long_term_draft(&self, draft: &LongTermMemoryDraft) -> bool {
        !self.policies.iter().any(|policy| {
            long_term_governance_policy_blocks_future_write(policy, self.now_secs)
                && runtime_long_term_policy_matches_draft(
                    policy,
                    &self.memory_space_id,
                    &self.subject_ids,
                    draft,
                )
        })
    }
}

fn long_term_governance_policy_blocks_future_write(
    policy: &MemoryLongTermGovernancePolicy,
    now_secs: u64,
) -> bool {
    match policy.kind.as_str() {
        "pause" => policy
            .expires_at
            .is_none_or(|expires_at| expires_at > now_secs),
        "suppress" => policy
            .duration
            .as_ref()
            .is_some_and(|duration| match duration {
                bm_core::memory::MemoryGovernanceSuppressionDuration::UntilManualResume => true,
                bm_core::memory::MemoryGovernanceSuppressionDuration::UntilUnixSecs(expires_at) => {
                    *expires_at > now_secs
                }
            }),
        _ => false,
    }
}

fn runtime_long_term_policy_matches_draft(
    policy: &MemoryLongTermGovernancePolicy,
    memory_space_id: &str,
    subject_ids: &[String],
    draft: &LongTermMemoryDraft,
) -> bool {
    let Some(normalized) = draft.normalized() else {
        return false;
    };
    let source_scope = runtime_inferred_long_term_source_scope(&normalized);
    subject_ids.iter().any(|subject_id| {
        policy.matches_candidate(
            Some(memory_space_id),
            Some(subject_id.as_str()),
            &normalized.kind,
            &normalized.topic,
            normalized.source_chat_id.as_deref(),
            source_scope,
        )
    })
}

fn runtime_inferred_long_term_source_scope(
    draft: &LongTermMemoryDraft,
) -> LongTermMemorySourceScope {
    match draft.source_scope {
        Some(LongTermMemorySourceScope::Chat) if draft.source_chat_id.is_some() => {
            LongTermMemorySourceScope::Chat
        }
        Some(LongTermMemorySourceScope::Chat) => match draft.kind {
            LongTermMemoryKind::Fact => LongTermMemorySourceScope::World,
            LongTermMemoryKind::Preference
            | LongTermMemoryKind::Profile
            | LongTermMemoryKind::Relationship
            | LongTermMemoryKind::Constraint
            | LongTermMemoryKind::Project
            | LongTermMemoryKind::Task => LongTermMemorySourceScope::User,
        },
        Some(scope) => scope,
        None => match draft.kind {
            LongTermMemoryKind::Project | LongTermMemoryKind::Task
                if draft.source_chat_id.is_some() =>
            {
                LongTermMemorySourceScope::Chat
            }
            LongTermMemoryKind::Fact => LongTermMemorySourceScope::World,
            LongTermMemoryKind::Preference
            | LongTermMemoryKind::Profile
            | LongTermMemoryKind::Relationship
            | LongTermMemoryKind::Constraint
            | LongTermMemoryKind::Project
            | LongTermMemoryKind::Task => LongTermMemorySourceScope::User,
        },
    }
}

fn memory_long_term_mutation_report_from_core(
    core_report: bm_core::memory::MemoryLongTermMutationReport,
    lifecycle_report: RuntimeLifecycleReport,
) -> MemoryLongTermMutationReport {
    MemoryLongTermMutationReport {
        accepted: core_report.accepted,
        dry_run: core_report.dry_run,
        operation: core_report.operation,
        target_report: core_report.target_report.clone(),
        affected_records: core_report.affected_records.clone(),
        tombstones: core_report.tombstones.clone(),
        evidence_refs: core_report.evidence_refs.clone(),
        transcript_refs: core_report.transcript_refs.clone(),
        policy_decision: core_report.policy_decision.clone(),
        projection_impact: core_report.projection_impact.clone(),
        deferred_governance_impact: core_report.deferred_governance_impact.clone(),
        lifecycle_report,
        audit_event_id: core_report.audit_event_id.clone(),
        reason: core_report.reason.clone(),
        core_report,
    }
}

fn memory_governance_policy_report_from_core(
    core_report: bm_core::memory::MemoryGovernancePolicyMutationReport,
    lifecycle_report: RuntimeLifecycleReport,
) -> MemoryGovernancePolicyMutationReport {
    MemoryGovernancePolicyMutationReport {
        accepted: core_report.accepted,
        dry_run: core_report.dry_run,
        operation: core_report.operation,
        policy_id: core_report.policy_id.clone(),
        affected_future_writes: core_report.affected_future_writes.clone(),
        policy_decision: core_report.policy_decision.clone(),
        lifecycle_report,
        audit_event_id: core_report.audit_event_id.clone(),
        reason: core_report.reason.clone(),
        core_report,
    }
}

fn build_llm_runtime_projection_envelope(
    projection_id: String,
    context: &crate::PromptMemoryContext,
    runtime_awareness: &str,
    inhabited_subject_projection: &InhabitedSubjectProjection,
    max_len: usize,
) -> LLMRuntimeProjectionEnvelope {
    let source_authority = context.classified_projection_sources();
    let subject_mount = soul_life_projection_report(inhabited_subject_projection);
    let boundary_protocol = runtime_disclosure_protocol_report(inhabited_subject_projection);
    let work_integrity = work_integrity_report(inhabited_subject_projection);
    let governed_memory_evidence = runtime_projection_source_blocks(
        context,
        &source_authority,
        PromptProjectionSurfaceRole::PublicGrounding,
        "public_grounding",
        false,
    );
    let procedural_evidence = runtime_projection_source_blocks(
        context,
        &source_authority,
        PromptProjectionSurfaceRole::ProceduralEvidence,
        "procedural_evidence",
        false,
    );
    let protected_private_runtime_context = inhabited_subject_projection
        .soul_private_runtime_context
        .iter()
        .map(|item| RuntimeProjectionSourceBlock {
            source_id: item.source_id.clone(),
            role: item.role.clone(),
            content: compact_runtime_projection_content(&item.content, 420),
            evidence_refs: vec![format!("source:{}", item.source_id)],
            protected: true,
        })
        .filter(|item| !item.content.trim().is_empty())
        .collect::<Vec<_>>();
    let operator_audit_excluded_source_ids = source_authority
        .iter()
        .filter(|source| {
            source
                .surface_roles
                .contains(&PromptProjectionSurfaceRole::OperatorAudit)
                || (!source.raw_audit_plaintext_allowed
                    && source.authorities.iter().any(|authority| {
                        matches!(
                            authority,
                            bm_core::memory::ProjectionSourceAuthority::PrivateInternal
                        )
                    }))
        })
        .map(|source| source.source_id.clone())
        .collect::<Vec<_>>();
    let mut section_names = vec![
        "subject_mount".to_string(),
        "governed_memory_evidence".to_string(),
        "boundary_and_disclosure_protocol".to_string(),
        "runtime_constraints".to_string(),
        "work_integrity_covenant".to_string(),
        "procedural_evidence".to_string(),
        "protected_private_runtime_context".to_string(),
    ];
    section_names.sort();
    section_names.dedup();

    let mut envelope = LLMRuntimeProjectionEnvelope {
        projection_id,
        runtime_awareness: compact_runtime_projection_content(runtime_awareness, 700),
        subject_mount,
        boundary_protocol,
        protected_private_runtime_context,
        governed_memory_evidence,
        procedural_evidence,
        runtime_constraints: vec![
            "Keep technical diagnostics on operator surfaces unless the user explicitly asks for diagnostics."
                .to_string(),
            "Use runtime limits only to size, prioritize, and trim the current reply.".to_string(),
        ],
        work_integrity,
        operator_audit_excluded_source_ids,
        agent_skill_hints: Vec::new(),
        agent_tool_hints: Vec::new(),
        section_names,
        rendered_block: String::new(),
    };
    let rendered = render_llm_runtime_projection_envelope(&envelope);
    envelope.rendered_block = truncate_to_char_boundary(rendered.trim(), max_len)
        .trim()
        .to_string();
    envelope
}

fn attach_agent_skill_hints_to_runtime_projection(
    envelope: &mut LLMRuntimeProjectionEnvelope,
    hints: Vec<bm_core::skills::ProjectedAgentSkillHint>,
    max_len: usize,
) {
    envelope.agent_skill_hints = hints;
    if !envelope.agent_skill_hints.is_empty()
        && !envelope
            .section_names
            .iter()
            .any(|name| name == "agent_skill_hints")
    {
        envelope.section_names.push("agent_skill_hints".to_string());
        envelope.section_names.sort();
        envelope.section_names.dedup();
    }
    let rendered = render_llm_runtime_projection_envelope(envelope);
    envelope.rendered_block = truncate_to_char_boundary(rendered.trim(), max_len)
        .trim()
        .to_string();
}

fn attach_agent_tool_hints_to_runtime_projection(
    envelope: &mut LLMRuntimeProjectionEnvelope,
    hints: Vec<bm_core::skills::AgentToolHint>,
    max_len: usize,
) {
    envelope.agent_tool_hints = hints;
    if !envelope.agent_tool_hints.is_empty()
        && !envelope
            .section_names
            .iter()
            .any(|name| name == "agent_tool_hints")
    {
        envelope.section_names.push("agent_tool_hints".to_string());
        envelope.section_names.sort();
        envelope.section_names.dedup();
    }
    let rendered = render_llm_runtime_projection_envelope(envelope);
    envelope.rendered_block = truncate_to_char_boundary(rendered.trim(), max_len)
        .trim()
        .to_string();
}

struct ProjectionAuditInput<'a> {
    runtime: &'a MemoryRuntime,
    context: &'a crate::PromptMemoryContext,
    lifecycle: &'a RuntimeLifecycleReport,
    render_budget_chars: usize,
    system_memory_chars: usize,
    injected: bool,
    runtime_projection: &'a LLMRuntimeProjectionEnvelope,
    agent_skill_audit: AgentSkillProjectionAudit,
    agent_tool_audit: AgentToolProjectionAudit,
}

fn build_projection_audit(input: ProjectionAuditInput<'_>) -> MemoryProjectionAuditReport {
    let runtime = input.runtime;
    let context = input.context;
    let runtime_projection = input.runtime_projection;
    let render_budget_chars = input.render_budget_chars;
    let system_memory_chars = input.system_memory_chars;
    let injected = input.injected;
    let agent_skill_audit = input.agent_skill_audit;
    let agent_tool_audit = input.agent_tool_audit;
    let private_policy_allowed = input
        .runtime
        .config
        .privacy_policy
        .private_plane_projection_allowed;
    let private_depth_allowed = input.lifecycle.admission.private_depth_allowed;
    let runtime_private_context_allowed = private_policy_allowed && private_depth_allowed;
    let foreground_disclosure_allowed = false;
    let private_reason = if runtime_private_context_allowed {
        "runtime_private_context_allowed_foreground_disclosure_requires_protocol"
    } else if !private_policy_allowed {
        "privacy_policy_denied"
    } else {
        "lifecycle_private_depth_denied"
    };
    let source_budget_chars = runtime
        .config
        .runtime_budget
        .projection_source_budget
        .context_assembly_max_chars;
    MemoryProjectionAuditReport {
        projection_id: runtime_projection.projection_id.clone(),
        operation: "project".to_string(),
        profile: runtime.config.profile,
        identity: runtime.config.identity.clone(),
        scope: runtime.config.scope.clone(),
        memory_space_id: runtime.config.memory_space_id.clone(),
        subject_id: runtime.config.subject_id.clone(),
        scoped_runtime: runtime.config.scoped_runtime.clone(),
        conversation_id: Some(runtime.config.scope.chat_id.clone()),
        source_budget_chars,
        render_budget_chars,
        system_memory_chars,
        injected,
        truncated: system_memory_chars >= render_budget_chars && render_budget_chars > 0,
        private_gate: MemoryProjectionPrivateGateAudit {
            privacy_policy_allowed: private_policy_allowed,
            lifecycle_private_depth_allowed: private_depth_allowed,
            runtime_private_context_allowed,
            foreground_disclosure_allowed,
            reason: private_reason.to_string(),
        },
        source_authority: context.classified_projection_sources(),
        agent_skills: agent_skill_audit,
        agent_tools: agent_tool_audit,
        sources: projection_source_audits(
            context,
            &runtime.config.agent_skill_registry,
            &runtime.agent_tool_registries(),
            runtime_projection,
        ),
        sections: projection_section_audits(runtime_projection),
    }
}

fn soul_life_projection_report(
    projection: &InhabitedSubjectProjection,
) -> SoulLifeProjectionReport {
    SoulLifeProjectionReport {
        identity_mount: projection.subject_mount.identity_mount.clone(),
        relationship_position: projection.subject_mount.relationship_position.clone(),
        situated_now: projection.subject_mount.situated_now.clone(),
        current_reasoning_basis: projection.subject_mount.current_reasoning_basis.clone(),
        reply_stance: projection.subject_mount.reply_stance.clone(),
        initiative_posture: projection.subject_mount.initiative_posture.clone(),
        boundary_mode: projection.subject_mount.boundary_mode.clone(),
        degraded_reason: projection.subject_mount.degraded_reason.clone(),
        evidence_refs: projection.evidence_refs.clone(),
    }
}

fn runtime_disclosure_protocol_report(
    projection: &InhabitedSubjectProjection,
) -> RuntimeDisclosureProtocolReport {
    RuntimeDisclosureProtocolReport {
        runtime_private_context_allowed: projection
            .boundary_and_disclosure_protocol
            .runtime_private_context_allowed,
        foreground_disclosure_allowed: projection
            .boundary_and_disclosure_protocol
            .foreground_disclosure_allowed,
        protected_sources: projection
            .boundary_and_disclosure_protocol
            .protected_sources
            .clone(),
        disclosure_rule: projection
            .boundary_and_disclosure_protocol
            .disclosure_rule
            .clone(),
        final_llm_privacy_judge_allowed: projection
            .boundary_and_disclosure_protocol
            .final_llm_privacy_judge_allowed,
    }
}

fn work_integrity_report(projection: &InhabitedSubjectProjection) -> WorkIntegrityReport {
    WorkIntegrityReport {
        task_goal: projection.work_integrity_covenant.task_goal.clone(),
        evidence_ceiling: projection.work_integrity_covenant.evidence_ceiling.clone(),
        tool_permission_boundary: projection
            .work_integrity_covenant
            .tool_permission_boundary
            .clone(),
        uncertainty_rule: projection.work_integrity_covenant.uncertainty_rule.clone(),
        no_obstruction_rule: projection
            .work_integrity_covenant
            .no_obstruction_rule
            .clone(),
    }
}

fn runtime_projection_source_blocks(
    context: &crate::PromptMemoryContext,
    sources: &[PromptProjectionSource],
    role: PromptProjectionSurfaceRole,
    rendered_role: &str,
    protected: bool,
) -> Vec<RuntimeProjectionSourceBlock> {
    sources
        .iter()
        .filter(|source| source.loaded && source.surface_roles.contains(&role))
        .filter_map(|source| {
            projection_source_runtime_text(context, &source.source_id).map(|content| {
                let evidence_refs = if source.evidence_refs.is_empty() {
                    vec![format!("source:{}", source.source_id)]
                } else {
                    source
                        .evidence_refs
                        .iter()
                        .map(|evidence| format!("{}:{evidence}", source.source_id))
                        .collect()
                };
                RuntimeProjectionSourceBlock {
                    source_id: source.source_id.clone(),
                    role: rendered_role.to_string(),
                    content: compact_runtime_projection_content(content, 420),
                    evidence_refs,
                    protected,
                }
            })
        })
        .filter(|block| !block.content.trim().is_empty())
        .collect()
}

fn render_llm_runtime_projection_envelope(envelope: &LLMRuntimeProjectionEnvelope) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## LLM Runtime Projection Envelope");
    let _ = writeln!(out, "- Projection: {}", envelope.projection_id);

    let _ = writeln!(out);
    let _ = writeln!(out, "## Subject Mount");
    let _ = writeln!(out, "- Identity: {}", envelope.subject_mount.identity_mount);
    let _ = writeln!(
        out,
        "- Relationship: {}",
        render_runtime_optional(&envelope.subject_mount.relationship_position)
    );
    let _ = writeln!(
        out,
        "- Situated now: {}",
        render_runtime_optional(&envelope.subject_mount.situated_now)
    );
    let _ = writeln!(
        out,
        "- Reasoning basis: {}",
        render_runtime_optional(&envelope.subject_mount.current_reasoning_basis)
    );
    let _ = writeln!(
        out,
        "- Reply stance: {}",
        render_runtime_optional(&envelope.subject_mount.reply_stance)
    );
    let _ = writeln!(
        out,
        "- Initiative posture: {}",
        render_runtime_optional(&envelope.subject_mount.initiative_posture)
    );
    let _ = writeln!(
        out,
        "- Boundary mode: {}",
        render_runtime_optional(&envelope.subject_mount.boundary_mode)
    );
    if let Some(reason) = envelope.subject_mount.degraded_reason.as_deref() {
        let _ = writeln!(out, "- Degraded reason: {reason}");
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "## Governed Memory Evidence");
    render_runtime_source_blocks(&mut out, &envelope.governed_memory_evidence);

    let _ = writeln!(out);
    let _ = writeln!(out, "## Boundary And Disclosure Protocol");
    let _ = writeln!(
        out,
        "- Runtime private context: {}",
        allowed_label(envelope.boundary_protocol.runtime_private_context_allowed)
    );
    let _ = writeln!(
        out,
        "- Foreground private disclosure: {}",
        allowed_label(envelope.boundary_protocol.foreground_disclosure_allowed)
    );
    let _ = writeln!(
        out,
        "- Final LLM privacy judge: {}",
        allowed_label(envelope.boundary_protocol.final_llm_privacy_judge_allowed)
    );
    let _ = writeln!(
        out,
        "- Rule: {}",
        envelope.boundary_protocol.disclosure_rule
    );
    if !envelope.boundary_protocol.protected_sources.is_empty() {
        let _ = writeln!(
            out,
            "- Protected sources: {}",
            envelope.boundary_protocol.protected_sources.join(", ")
        );
    }
    let _ = writeln!(
        out,
        "- Allowed disclosure forms: summary, redacted_excerpt, explain_without_quote, refuse, defer"
    );
    let _ = writeln!(
        out,
        "- Forbidden disclosure forms: raw_dump, source_path, internal_title, private_raw_quote"
    );

    let _ = writeln!(out);
    let _ = writeln!(out, "## Runtime Constraints");
    let _ = writeln!(
        out,
        "- Awareness: {}",
        render_runtime_optional(&envelope.runtime_awareness)
    );
    for constraint in &envelope.runtime_constraints {
        let _ = writeln!(out, "- {constraint}");
    }

    if !envelope.protected_private_runtime_context.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "## Soul Private Runtime Context");
        let _ = writeln!(
            out,
            "- Runtime private context: {}",
            allowed_label(envelope.boundary_protocol.runtime_private_context_allowed)
        );
        let _ = writeln!(
            out,
            "- Foreground disclosure remains: {}",
            allowed_label(envelope.boundary_protocol.foreground_disclosure_allowed)
        );
        render_runtime_source_blocks(&mut out, &envelope.protected_private_runtime_context);
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "## Work Integrity Covenant");
    let _ = writeln!(
        out,
        "- Task goal: {}",
        render_runtime_optional(&envelope.work_integrity.task_goal)
    );
    let _ = writeln!(
        out,
        "- Evidence ceiling: {}",
        envelope.work_integrity.evidence_ceiling
    );
    let _ = writeln!(
        out,
        "- Tool boundary: {}",
        envelope.work_integrity.tool_permission_boundary
    );
    let _ = writeln!(
        out,
        "- Uncertainty rule: {}",
        envelope.work_integrity.uncertainty_rule
    );
    let _ = writeln!(
        out,
        "- No obstruction: {}",
        envelope.work_integrity.no_obstruction_rule
    );

    if !envelope.procedural_evidence.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "## Procedural Evidence");
        render_runtime_source_blocks(&mut out, &envelope.procedural_evidence);
    }

    if !envelope.agent_skill_hints.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "## Agent Skill Hints");
        let _ = writeln!(
            out,
            "- These are read-only host-provided Agent Skill hints. The memory runtime may recall them, but execution and file/resource access remain host-owned."
        );
        for hint in &envelope.agent_skill_hints {
            let _ = writeln!(
                out,
                "- {} [{} refs=agent_skill:{} fp={}]: {}",
                hint.name, hint.reason, hint.package_id, hint.fingerprint, hint.prompt_snippet
            );
        }
    }
    if !envelope.agent_tool_hints.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "## Agent Tool Experience Hints");
        let _ = writeln!(
            out,
            "- These are governed tool-use experience hints only. Host runtime must still choose exposed tools, provide full schemas, validate permissions, and execute tools."
        );
        for hint in &envelope.agent_tool_hints {
            let constraints = if hint.constraints.is_empty() {
                "none".to_string()
            } else {
                hint.constraints.join("; ")
            };
            let _ = writeln!(
                out,
                "- {} [registry={} experience={} fp={} confidence={:?} host_execution_required={}]: {} Constraints: {}",
                hint.tool_id,
                hint.registry_id,
                hint.experience_id,
                hint.schema_fingerprint,
                hint.confidence,
                hint.host_execution_required,
                hint.reason,
                constraints
            );
        }
    }
    out
}

fn render_runtime_source_blocks(out: &mut String, blocks: &[RuntimeProjectionSourceBlock]) {
    if blocks.is_empty() {
        let _ = writeln!(out, "- none");
        return;
    }
    for block in blocks {
        let refs = if block.evidence_refs.is_empty() {
            "unscoped".to_string()
        } else {
            block.evidence_refs.join(",")
        };
        let protection = if block.protected { " protected" } else { "" };
        let _ = writeln!(
            out,
            "- {} [{}{} refs={}]: {}",
            block.source_id, block.role, protection, refs, block.content
        );
    }
}

fn render_runtime_optional(value: &str) -> String {
    if value.trim().is_empty() {
        "unavailable".to_string()
    } else {
        value.trim().to_string()
    }
}

fn allowed_label(allowed: bool) -> &'static str {
    if allowed {
        "allowed"
    } else {
        "not_allowed"
    }
}

fn build_subject_projection_report(
    audit: &MemoryProjectionAuditReport,
    request: &MemoryProjectionRequest,
    system_memory_block: &str,
    runtime_projection: &LLMRuntimeProjectionEnvelope,
    inhabited_subject_projection: Option<&InhabitedSubjectProjection>,
) -> SubjectProjectionReport {
    let mut evidence_refs = Vec::new();
    if let Some(projection) = inhabited_subject_projection {
        evidence_refs.extend(projection.evidence_refs.iter().cloned());
    }
    for block in runtime_projection
        .governed_memory_evidence
        .iter()
        .chain(runtime_projection.procedural_evidence.iter())
        .chain(runtime_projection.protected_private_runtime_context.iter())
    {
        evidence_refs.extend(block.evidence_refs.iter().cloned());
    }
    evidence_refs.extend(
        runtime_projection
            .agent_skill_hints
            .iter()
            .map(|hint| format!("agent_skill:{}", hint.package_id)),
    );
    for source in &audit.sources {
        for selected_id in &source.selected_ids {
            evidence_refs.push(format!("{}:{selected_id}", source.plane));
        }
    }
    for section in &audit.sections {
        if section.included {
            evidence_refs.push(format!("section:{}", section.name));
        }
    }
    evidence_refs.sort();
    evidence_refs.dedup();
    if evidence_refs.is_empty() && !system_memory_block.trim().is_empty() {
        evidence_refs.push("synthesized:runtime_awareness".to_string());
    }

    let mut dropped_candidates = Vec::new();
    if let Some(projection) = inhabited_subject_projection {
        dropped_candidates.extend(projection.dropped_candidates.iter().map(|candidate| {
            DroppedProjectionCandidate {
                candidate_id: candidate.candidate_id.clone(),
                reason: candidate.reason.clone(),
            }
        }));
    }
    for source in &audit.sources {
        let dropped = source.candidate_count.saturating_sub(source.selected_count);
        if dropped > 0 {
            dropped_candidates.push(DroppedProjectionCandidate {
                candidate_id: format!("{}:unselected:{dropped}", source.plane),
                reason: source
                    .miss_reason
                    .clone()
                    .unwrap_or_else(|| "budget_or_recall_rerank".to_string()),
            });
        }
    }
    if !audit.private_gate.runtime_private_context_allowed {
        dropped_candidates.push(DroppedProjectionCandidate {
            candidate_id: "private_depth".to_string(),
            reason: audit.private_gate.reason.clone(),
        });
    }
    if audit.truncated {
        dropped_candidates.push(DroppedProjectionCandidate {
            candidate_id: "render_tail".to_string(),
            reason: "projection_render_budget".to_string(),
        });
    }

    let profile_trim_reason = if let Some(reason) =
        inhabited_subject_projection.and_then(|projection| projection.profile_trim_reason.clone())
    {
        reason
    } else if audit.truncated {
        "projection_render_budget".to_string()
    } else {
        String::new()
    };
    let identity_mount = inhabited_subject_projection
        .map(|projection| {
            format!(
                "{} | agent:{} owner:{} subject:{}",
                projection.subject_mount.identity_mount,
                audit.identity.agent_id,
                audit.identity.owner_id,
                audit.subject_id
            )
        })
        .unwrap_or_else(|| {
            format!(
                "agent:{} owner:{} subject:{}",
                audit.identity.agent_id, audit.identity.owner_id, audit.subject_id
            )
        });
    let relationship_position = inhabited_subject_projection
        .map(|projection| {
            format!(
                "{} | scope:{} chat:{}",
                projection.subject_mount.relationship_position,
                audit.scope.channel,
                audit.scope.chat_id
            )
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("scope:{} chat:{}", audit.scope.channel, audit.scope.chat_id));
    let situated_now = inhabited_subject_projection
        .map(|projection| projection.subject_mount.situated_now.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| request.user_query.clone());
    let subject_mount = inhabited_subject_projection
        .map(|projection| SubjectProjectionMountReport {
            identity_mount: projection.subject_mount.identity_mount.clone(),
            relationship_position: projection.subject_mount.relationship_position.clone(),
            situated_now: projection.subject_mount.situated_now.clone(),
            current_reasoning_basis: projection.subject_mount.current_reasoning_basis.clone(),
            reply_stance: projection.subject_mount.reply_stance.clone(),
            initiative_posture: projection.subject_mount.initiative_posture.clone(),
            boundary_mode: projection.subject_mount.boundary_mode.clone(),
            degraded_reason: projection.subject_mount.degraded_reason.clone(),
        })
        .unwrap_or_else(|| SubjectProjectionMountReport {
            identity_mount: identity_mount.clone(),
            relationship_position: relationship_position.clone(),
            situated_now: situated_now.clone(),
            current_reasoning_basis: "projection_unavailable".to_string(),
            reply_stance: "work_first".to_string(),
            initiative_posture: "do_not_obstruct_user_work".to_string(),
            boundary_mode: "privacy_policy_default".to_string(),
            degraded_reason: Some("subject_projection_unavailable".to_string()),
        });
    let boundary_protocol = inhabited_subject_projection
        .map(|projection| SubjectProjectionBoundaryProtocolReport {
            runtime_private_context_allowed: projection
                .boundary_and_disclosure_protocol
                .runtime_private_context_allowed,
            foreground_disclosure_allowed: projection
                .boundary_and_disclosure_protocol
                .foreground_disclosure_allowed,
            protected_sources: projection
                .boundary_and_disclosure_protocol
                .protected_sources
                .clone(),
            disclosure_rule: projection
                .boundary_and_disclosure_protocol
                .disclosure_rule
                .clone(),
            final_llm_privacy_judge_allowed: projection
                .boundary_and_disclosure_protocol
                .final_llm_privacy_judge_allowed,
        })
        .unwrap_or_else(|| SubjectProjectionBoundaryProtocolReport {
            runtime_private_context_allowed: audit.private_gate.runtime_private_context_allowed,
            foreground_disclosure_allowed: audit.private_gate.foreground_disclosure_allowed,
            protected_sources: Vec::new(),
            disclosure_rule: audit.private_gate.reason.clone(),
            final_llm_privacy_judge_allowed: false,
        });
    let work_integrity = inhabited_subject_projection
        .map(|projection| SubjectProjectionWorkIntegrityReport {
            task_goal: projection.work_integrity_covenant.task_goal.clone(),
            evidence_ceiling: projection.work_integrity_covenant.evidence_ceiling.clone(),
            tool_permission_boundary: projection
                .work_integrity_covenant
                .tool_permission_boundary
                .clone(),
            uncertainty_rule: projection.work_integrity_covenant.uncertainty_rule.clone(),
            no_obstruction_rule: projection
                .work_integrity_covenant
                .no_obstruction_rule
                .clone(),
        })
        .unwrap_or_else(|| SubjectProjectionWorkIntegrityReport {
            task_goal: request.user_query.clone(),
            evidence_ceiling: "only use available governed memory evidence".to_string(),
            tool_permission_boundary: "respect runtime capability policy".to_string(),
            uncertainty_rule: "state uncertainty instead of inventing memory".to_string(),
            no_obstruction_rule: "complete the user work directly and stay task-focused"
                .to_string(),
        });

    SubjectProjectionReport {
        projection_id: audit.projection_id.clone(),
        profile: audit.profile,
        subject_mount,
        boundary_protocol,
        work_integrity,
        identity_mount,
        relationship_position,
        situated_now,
        evidence_refs,
        budget_decisions: vec![
            ProjectionBudgetDecision {
                surface: "source_context".to_string(),
                budget_chars: audit.source_budget_chars,
                used_chars: audit
                    .sections
                    .iter()
                    .map(|section| section.chars)
                    .sum::<usize>(),
                reason: "runtime_projection_source_budget".to_string(),
            },
            ProjectionBudgetDecision {
                surface: "prompt".to_string(),
                budget_chars: audit.render_budget_chars,
                used_chars: audit.system_memory_chars,
                reason: "runtime_projection_render_budget".to_string(),
            },
        ],
        privacy_decisions: vec![ProjectionPrivacyDecision {
            source_id: "private_depth".to_string(),
            allowed: audit.private_gate.foreground_disclosure_allowed,
            reason: audit.private_gate.reason.clone(),
        }],
        dropped_candidates,
        profile_trim_reason,
    }
}

fn build_projection_faithfulness_check(
    report: &SubjectProjectionReport,
    runtime_projection: &LLMRuntimeProjectionEnvelope,
    system_memory_block: &str,
) -> ProjectionFaithfulnessCheck {
    let checked_claims = projection_faithfulness_claims(report, runtime_projection);
    let mut unsupported_claims =
        if system_memory_block.trim().is_empty() && !report.evidence_refs.is_empty() {
            vec!["projection_report_has_evidence_without_rendered_block".to_string()]
        } else {
            Vec::new()
        };
    if report.evidence_refs.is_empty() {
        unsupported_claims.push("projection_report_missing_evidence_refs".to_string());
    }
    for block in runtime_projection
        .governed_memory_evidence
        .iter()
        .chain(runtime_projection.procedural_evidence.iter())
        .chain(runtime_projection.protected_private_runtime_context.iter())
    {
        if block.evidence_refs.is_empty() && !block.content.trim().is_empty() {
            unsupported_claims.push(format!("{}:missing_evidence_ref", block.source_id));
        }
    }
    for hint in &runtime_projection.agent_skill_hints {
        if hint.package_id.trim().is_empty() || hint.fingerprint.trim().is_empty() {
            unsupported_claims.push(format!("{}:missing_agent_skill_ref", hint.name));
        }
    }
    unsupported_claims.sort();
    unsupported_claims.dedup();
    ProjectionFaithfulnessCheck {
        projection_id: report.projection_id.clone(),
        checked_refs: report.evidence_refs.clone(),
        checked_claims,
        passed: unsupported_claims.is_empty() && !report.evidence_refs.is_empty(),
        unsupported_claims,
    }
}

fn build_private_disclosure_integrity_report(
    audit: &MemoryProjectionAuditReport,
    runtime_projection: &LLMRuntimeProjectionEnvelope,
) -> PrivateDisclosureIntegrityReport {
    let mut blocked_source_ids = if audit.private_gate.foreground_disclosure_allowed {
        Vec::new()
    } else {
        vec!["private_depth".to_string()]
    };
    blocked_source_ids.extend(
        audit
            .source_authority
            .iter()
            .filter(|source| source.loaded && !source.foreground_disclosure_allowed)
            .map(|source| source.source_id.clone()),
    );
    blocked_source_ids.sort();
    blocked_source_ids.dedup();
    let mut redacted_source_ids = runtime_projection
        .protected_private_runtime_context
        .iter()
        .map(|item| item.source_id.clone())
        .collect::<Vec<_>>();
    redacted_source_ids.extend(
        runtime_projection
            .operator_audit_excluded_source_ids
            .clone(),
    );
    redacted_source_ids.sort();
    redacted_source_ids.dedup();
    let raw_private_violation_count =
        raw_private_violation_count(&runtime_projection.rendered_block);
    PrivateDisclosureIntegrityReport {
        checked_surfaces: vec![
            "prompt".to_string(),
            "ui_api".to_string(),
            "operator_raw".to_string(),
            "gateway_raw_audit".to_string(),
            "shared_fact_surface".to_string(),
        ],
        blocked_source_ids,
        redacted_source_ids,
        raw_private_violation_count,
        passed: raw_private_violation_count == 0,
    }
}

fn projection_faithfulness_claims(
    report: &SubjectProjectionReport,
    runtime_projection: &LLMRuntimeProjectionEnvelope,
) -> Vec<String> {
    let mut claims = vec![
        "subject_mount.identity_mount".to_string(),
        "subject_mount.relationship_position".to_string(),
        "subject_mount.situated_now".to_string(),
        "boundary_protocol.disclosure_rule".to_string(),
        "work_integrity.task_goal".to_string(),
    ];
    claims.extend(
        runtime_projection
            .governed_memory_evidence
            .iter()
            .map(|block| format!("governed_memory_evidence.{}", block.source_id)),
    );
    claims.extend(
        runtime_projection
            .procedural_evidence
            .iter()
            .map(|block| format!("procedural_evidence.{}", block.source_id)),
    );
    if report.subject_mount.degraded_reason.is_some() {
        claims.push("subject_mount.degraded_reason".to_string());
    }
    claims.sort();
    claims.dedup();
    claims
}

fn raw_private_violation_count(system_memory_block: &str) -> u32 {
    let lowered = system_memory_block.to_ascii_lowercase();
    let private_markers = [
        "private_raw:",
        "private-garden-raw:",
        "private garden raw:",
        "<private_raw>",
    ];
    private_markers
        .iter()
        .filter(|marker| lowered.contains(**marker))
        .count() as u32
}

struct RuntimeRecallGraphReport {
    rerank: GraphRecallRerankReport,
    gate: TemporalMemoryGraphGateReport,
    compact_graph: CompactMemoryGraph,
    index_report: MemoryGraphRecallIndexReport,
    candidate_evidence_ref_index: Vec<MemoryEvalRecallEvidenceRefIndexEntry>,
}

#[derive(Default)]
struct PersistentRecallGraphLoadReport {
    graph: Option<TemporalMemoryGraphBuildReport>,
    indexes: Vec<MemoryGraphRecallIndexDoc>,
    failures: Vec<String>,
    index_failures: Vec<String>,
}

fn build_recall_graph_report(
    query: &str,
    mut evidence: Vec<MemoryGraphEvidence>,
    graph_anchor_candidate_ids: &[String],
    persistent_graph: &PersistentRecallGraphLoadReport,
    expansion_budget: GraphRecallExpansionBudget,
) -> RuntimeRecallGraphReport {
    let candidate_ids = retain_unique_recall_graph_evidence(&mut evidence);
    let graph = build_temporal_memory_graph_from_evidence(evidence);
    let mut index_report = build_memory_graph_index_report(
        &persistent_graph.indexes,
        &persistent_graph.index_failures,
        graph_anchor_candidate_ids,
        persistent_graph.graph.is_some(),
    );
    append_index_report_failures(&mut index_report, &persistent_graph.failures);
    if let Some(persistent) = persistent_graph.graph.as_ref() {
        if graph_anchor_candidate_ids.iter().any(|candidate_id| {
            persistent
                .nodes
                .iter()
                .any(|node| node.node_id == *candidate_id)
        }) {
            let indexed_graph = if index_report.used {
                filter_temporal_graph_for_index(
                    persistent,
                    graph_anchor_candidate_ids,
                    &index_report.expanded_node_ids,
                )
            } else {
                persistent.clone()
            };
            record_index_graph_input_counts(&mut index_report, &indexed_graph);
            let rerank = rerank_recall_with_temporal_graph(
                query,
                graph_anchor_candidate_ids.to_vec(),
                &indexed_graph,
                expansion_budget,
            );
            let mut gate = indexed_graph.gate.clone();
            gate.failures.extend(persistent_graph.failures.clone());
            gate.failures.sort();
            gate.failures.dedup();
            gate.high_confidence_projection_allowed =
                gate.high_confidence_projection_allowed && gate.failures.is_empty();
            return RuntimeRecallGraphReport {
                rerank,
                gate,
                candidate_evidence_ref_index: graph_candidate_evidence_ref_index(&indexed_graph),
                compact_graph: indexed_graph.compact_graph,
                index_report,
            };
        }
    }

    let rerank = rerank_recall_with_temporal_graph(query, candidate_ids, &graph, expansion_budget);
    record_index_graph_input_counts(&mut index_report, &graph);
    let candidate_evidence_ref_index = graph_candidate_evidence_ref_index(&graph);
    let mut gate = graph.gate;
    gate.high_confidence_projection_allowed = false;
    if !gate
        .failures
        .iter()
        .any(|failure| failure == "runtime_recall_graph_preview_not_persistent")
    {
        gate.failures
            .push("runtime_recall_graph_preview_not_persistent".to_string());
    }
    if persistent_graph.graph.is_some()
        && !gate
            .failures
            .iter()
            .any(|failure| failure == "persistent_recall_graph_source_anchor_missing")
    {
        gate.failures
            .push("persistent_recall_graph_source_anchor_missing".to_string());
    }
    gate.failures.extend(persistent_graph.failures.clone());
    gate.failures.sort();
    gate.failures.dedup();
    RuntimeRecallGraphReport {
        rerank,
        gate,
        candidate_evidence_ref_index,
        compact_graph: graph.compact_graph,
        index_report,
    }
}

fn runtime_recall_graph_source_evidence(
    procedural_hits: &[crate::RuntimeSkillHit],
    working: &crate::WorkingRecallInspection,
    now_secs: u64,
) -> Vec<MemoryGraphEvidence> {
    let mut evidence = Vec::new();
    for hit in procedural_hits {
        push_recall_graph_evidence(
            &mut evidence,
            hit.record.name.clone(),
            MemoryGraphNodeKind::Procedure,
            hit.record.title.clone(),
            "procedural_runtime_skill",
            format!("runtime_skill:{}", hit.record.name),
            hit.record
                .updated_at
                .max(hit.record.observed_at)
                .max(now_secs),
        );
    }
    append_recall_report_graph_evidence(&mut evidence, &working.shared_factual_report, now_secs);
    append_recall_report_graph_evidence(
        &mut evidence,
        &working.continuity_capsule_report,
        now_secs,
    );
    append_recall_report_graph_evidence(&mut evidence, &working.archive_recall_report, now_secs);
    append_recall_report_graph_evidence(&mut evidence, &working.runtime_skill_report, now_secs);
    if let Some(report) = working.task_recall_report.as_ref() {
        append_recall_report_graph_evidence(&mut evidence, report, now_secs);
    }
    evidence
}

fn runtime_recall_graph_anchor_evidence(
    procedural_hits: &[crate::RuntimeSkillHit],
    working: &crate::WorkingRecallInspection,
    now_secs: u64,
    limit: usize,
) -> Vec<MemoryGraphEvidence> {
    let mut evidence = runtime_recall_graph_source_evidence(procedural_hits, working, now_secs);
    append_recall_report_graph_anchor_evidence(
        &mut evidence,
        &working.shared_factual_report,
        now_secs,
    );
    append_recall_report_graph_anchor_evidence(
        &mut evidence,
        &working.continuity_capsule_report,
        now_secs,
    );
    append_recall_report_graph_anchor_evidence(
        &mut evidence,
        &working.archive_recall_report,
        now_secs,
    );
    append_recall_report_graph_anchor_evidence(
        &mut evidence,
        &working.runtime_skill_report,
        now_secs,
    );
    if let Some(report) = working.task_recall_report.as_ref() {
        append_recall_report_graph_anchor_evidence(&mut evidence, report, now_secs);
    }
    let mut candidate_ids = Vec::new();
    evidence.retain(|item| {
        if item.node_id.trim().is_empty() {
            return false;
        }
        if candidate_ids
            .iter()
            .any(|candidate| candidate == &item.node_id)
        {
            return false;
        }
        candidate_ids.push(item.node_id.clone());
        limit == 0 || candidate_ids.len() <= limit
    });
    evidence
}

fn recall_graph_candidate_ids(evidence: &[MemoryGraphEvidence]) -> Vec<String> {
    let mut candidate_ids = Vec::new();
    for item in evidence {
        if item.node_id.trim().is_empty()
            || candidate_ids
                .iter()
                .any(|candidate| candidate == &item.node_id)
        {
            continue;
        }
        candidate_ids.push(item.node_id.clone());
    }
    candidate_ids
}

fn retain_unique_recall_graph_evidence(evidence: &mut Vec<MemoryGraphEvidence>) -> Vec<String> {
    let mut candidate_ids = Vec::new();
    evidence.retain(|item| {
        if candidate_ids
            .iter()
            .any(|candidate| candidate == &item.node_id)
        {
            false
        } else {
            candidate_ids.push(item.node_id.clone());
            true
        }
    });
    candidate_ids
}

fn build_memory_graph_index_report(
    indexes: &[MemoryGraphRecallIndexDoc],
    index_failures: &[String],
    candidate_ids: &[String],
    persistent_graph_available: bool,
) -> MemoryGraphRecallIndexReport {
    let mut failures = index_failures.to_vec();
    let matched = indexes
        .iter()
        .filter(|index| {
            candidate_ids
                .iter()
                .any(|candidate_id| candidate_id == &index.source_anchor_id)
        })
        .collect::<Vec<_>>();
    if persistent_graph_available && indexes.is_empty() {
        failures.push("memory_graph_index_missing".to_string());
    } else if persistent_graph_available && matched.is_empty() {
        failures.push("memory_graph_index_source_anchor_missing".to_string());
    }

    failures.sort();
    failures.dedup();

    let mut source_anchor_ids = matched
        .iter()
        .map(|index| index.source_anchor_id.clone())
        .collect::<Vec<_>>();
    source_anchor_ids.sort();
    source_anchor_ids.dedup();
    let mut expanded_node_ids = matched
        .iter()
        .flat_map(|index| index.neighbor_node_ids.iter().cloned())
        .collect::<Vec<_>>();
    expanded_node_ids.sort();
    expanded_node_ids.dedup();
    let mut unmatched_source_anchor_ids = candidate_ids
        .iter()
        .filter(|candidate_id| {
            !source_anchor_ids
                .iter()
                .any(|source_anchor_id| source_anchor_id == *candidate_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    unmatched_source_anchor_ids.sort();
    unmatched_source_anchor_ids.dedup();
    let mut revisions = matched
        .iter()
        .map(|index| index.revision_id.clone())
        .collect::<Vec<_>>();
    revisions.sort();
    revisions.dedup();
    let used = !source_anchor_ids.is_empty() && index_failures.is_empty();

    MemoryGraphRecallIndexReport {
        owner: "bm-sdk::MemoryRuntime".to_string(),
        used,
        fallback_full_scan: persistent_graph_available && !used,
        source_candidate_count: candidate_ids.len(),
        matched_source_anchor_count: source_anchor_ids.len(),
        source_anchor_ids,
        unmatched_source_anchor_ids,
        indexed_neighbor_count: expanded_node_ids.len(),
        expanded_node_ids,
        index_doc_count: indexes.len(),
        index_revision: revisions.first().cloned(),
        filtered_node_count: 0,
        filtered_edge_count: 0,
        filtered_backlink_count: 0,
        failures,
    }
}

fn build_memory_facet_index_report(
    source_candidate_ids: &[String],
) -> MemoryFacetRecallIndexReport {
    MemoryFacetRecallIndexReport {
        owner: "bm-sdk::MemoryRuntime".to_string(),
        used: false,
        report_only: true,
        fallback_full_scan: false,
        source_candidate_count: source_candidate_ids.len(),
        matched_source_candidate_count: 0,
        exact_facet_doc_count: 0,
        expanded_facet_doc_count: 0,
        exact_facet_candidate_ids: Vec::new(),
        expanded_facet_candidate_ids: Vec::new(),
        index_revision: None,
        failures: vec!["memory_facet_index_not_loaded".to_string()],
    }
}

fn record_index_graph_input_counts(
    index_report: &mut MemoryGraphRecallIndexReport,
    graph: &TemporalMemoryGraphBuildReport,
) {
    index_report.filtered_node_count = graph.nodes.len();
    index_report.filtered_edge_count = graph.edges.len();
    index_report.filtered_backlink_count = graph.backlinks.len();
}

fn graph_candidate_evidence_ref_index(
    graph: &TemporalMemoryGraphBuildReport,
) -> Vec<MemoryEvalRecallEvidenceRefIndexEntry> {
    graph
        .nodes
        .iter()
        .map(|node| MemoryEvalRecallEvidenceRefIndexEntry {
            candidate_id: node.node_id.clone(),
            evidence_refs: node.evidence_refs.clone(),
        })
        .collect()
}

fn append_index_report_failures(
    index_report: &mut MemoryGraphRecallIndexReport,
    failures: &[String],
) {
    index_report.failures.extend(failures.iter().cloned());
    index_report.failures.sort();
    index_report.failures.dedup();
}

fn indexed_node_keys(
    source_candidate_ids: &[String],
    indexes: &[MemoryGraphRecallIndexDoc],
) -> Vec<String> {
    let mut keys = source_candidate_ids.to_vec();
    for index in indexes {
        push_unique_string(&mut keys, index.source_anchor_id.clone());
        for neighbor in &index.neighbor_node_ids {
            push_unique_string(&mut keys, neighbor.clone());
        }
    }
    keys
}

fn indexed_edge_keys(indexes: &[MemoryGraphRecallIndexDoc]) -> Vec<String> {
    let mut keys = Vec::new();
    for index in indexes {
        for edge_id in &index.edge_ids {
            push_unique_string(&mut keys, edge_id.clone());
        }
    }
    keys
}

fn indexed_backlink_keys(indexes: &[MemoryGraphRecallIndexDoc]) -> Vec<String> {
    let mut keys = Vec::new();
    for index in indexes {
        for key in &index.evidence_backlink_keys {
            push_unique_string(&mut keys, key.clone());
        }
    }
    keys
}

fn bounded_persistent_graph_keys(
    keys: Vec<String>,
    max_loaded: usize,
    failure: &str,
    failures: &mut Vec<String>,
) -> Vec<String> {
    let mut unique = Vec::new();
    let mut seen = BTreeSet::new();
    for key in keys {
        if seen.insert(key.clone()) {
            unique.push(key);
        }
    }
    if unique.len() > max_loaded {
        failures.push(failure.to_string());
        unique.truncate(max_loaded);
    }
    unique
}

fn filter_temporal_graph_for_index(
    graph: &TemporalMemoryGraphBuildReport,
    candidate_ids: &[String],
    expanded_node_ids: &[String],
) -> TemporalMemoryGraphBuildReport {
    let mut allowed_node_ids = candidate_ids.to_vec();
    for node_id in expanded_node_ids {
        if !allowed_node_ids.iter().any(|existing| existing == node_id) {
            allowed_node_ids.push(node_id.clone());
        }
    }

    let nodes = graph
        .nodes
        .iter()
        .filter(|node| {
            allowed_node_ids
                .iter()
                .any(|allowed| allowed == &node.node_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    let edges = graph
        .edges
        .iter()
        .filter(|edge| {
            allowed_node_ids
                .iter()
                .any(|allowed| allowed == &edge.from_node_id)
                && allowed_node_ids
                    .iter()
                    .any(|allowed| allowed == &edge.to_node_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    let evidence_refs = nodes
        .iter()
        .flat_map(|node| node.evidence_refs.iter())
        .chain(edges.iter().flat_map(|edge| edge.evidence_refs.iter()))
        .cloned()
        .collect::<Vec<_>>();
    let backlinks = graph
        .backlinks
        .iter()
        .filter(|backlink| {
            evidence_refs
                .iter()
                .any(|evidence_ref| evidence_ref == &backlink.source_id)
        })
        .cloned()
        .collect::<Vec<_>>();

    build_temporal_memory_graph_from_parts(nodes, edges, backlinks)
}

fn read_persistent_graph_docs_by_keys<T>(
    store_platform: &StorePlatform,
    namespace: &str,
    keys: &[String],
    failures: &mut Vec<String>,
) -> Vec<T>
where
    T: DeserializeOwned,
{
    let docs = match store_platform.read_json_docs_by_keys(namespace, keys) {
        Ok(docs) => docs,
        Err(error) => {
            failures.push(format!(
                "persistent_graph_namespace_read_failed:{namespace}:{}",
                error.stage()
            ));
            return Vec::new();
        }
    };
    let mut records = Vec::new();
    for doc in docs {
        let key = doc.key;
        match serde_json::from_value::<T>(doc.value) {
            Ok(record) => records.push(record),
            Err(error) => failures.push(format!(
                "persistent_graph_doc_decode_failed:{namespace}:{key}:{error}"
            )),
        }
    }
    records
}

fn push_unique_string(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn append_recall_report_graph_evidence(
    evidence: &mut Vec<MemoryGraphEvidence>,
    report: &RecallSelectionReport,
    now_secs: u64,
) {
    for candidate in selected_recall_candidates(report) {
        let node_id = candidate.candidate_id.trim();
        if node_id.is_empty() {
            continue;
        }
        let label = first_non_empty(&[&candidate.title, &candidate.excerpt, node_id]);
        push_recall_graph_evidence(
            evidence,
            node_id.to_string(),
            MemoryGraphNodeKind::MemoryRecord,
            label.to_string(),
            report.plane.label(),
            format!("{}:{node_id}", report.plane.label()),
            candidate.observed_at.unwrap_or(now_secs),
        );
    }
    for selected_id in &report.selected_ids {
        if selected_id.trim().is_empty()
            || evidence
                .iter()
                .any(|item| item.node_id == selected_id.trim())
        {
            continue;
        }
        push_recall_graph_evidence(
            evidence,
            selected_id.trim().to_string(),
            MemoryGraphNodeKind::MemoryRecord,
            selected_id.trim().to_string(),
            report.plane.label(),
            format!("{}:{}", report.plane.label(), selected_id.trim()),
            now_secs,
        );
    }
}

fn append_recall_report_graph_anchor_evidence(
    evidence: &mut Vec<MemoryGraphEvidence>,
    report: &RecallSelectionReport,
    now_secs: u64,
) {
    for candidate in &report.candidates {
        let node_id = candidate.candidate_id.trim();
        if node_id.is_empty() {
            continue;
        }
        let label = first_non_empty(&[&candidate.title, &candidate.excerpt, node_id]);
        push_recall_graph_evidence(
            evidence,
            node_id.to_string(),
            MemoryGraphNodeKind::MemoryRecord,
            label.to_string(),
            report.plane.label(),
            format!("{}:{node_id}", report.plane.label()),
            candidate.observed_at.unwrap_or(now_secs),
        );
    }
}

fn selected_recall_candidates(report: &RecallSelectionReport) -> Vec<&RecallCandidate> {
    report
        .candidates
        .iter()
        .filter(|candidate| {
            candidate.selected
                || report
                    .selected_ids
                    .iter()
                    .any(|selected_id| selected_id == &candidate.candidate_id)
        })
        .collect()
}

fn push_recall_graph_evidence(
    evidence: &mut Vec<MemoryGraphEvidence>,
    node_id: String,
    kind: MemoryGraphNodeKind,
    label: String,
    source_kind: &str,
    source_id: String,
    observed_at: u64,
) {
    if node_id.trim().is_empty() || source_id.trim().is_empty() {
        return;
    }
    let fingerprint_seed = format!("{source_kind}:{source_id}:{node_id}:{observed_at}");
    evidence.push(MemoryGraphEvidence {
        node_id,
        kind,
        label,
        source_kind: source_kind.to_string(),
        source_id,
        fingerprint: format!("{:016x}", fnv1a64(fingerprint_seed.as_bytes())),
        observed_at,
        supports: Vec::new(),
        supersedes: None,
    });
}

fn build_eval_recall_candidates(
    candidate_ids: &[String],
    recall: &MemoryRecallReport,
    include_graph_neighbors: bool,
    include_score_breakdown: bool,
) -> Vec<MemoryEvalRecallCandidate> {
    candidate_ids
        .iter()
        .map(|candidate_id| {
            build_eval_recall_candidate(
                candidate_id,
                recall,
                include_graph_neighbors,
                include_score_breakdown,
            )
        })
        .collect()
}

fn build_eval_candidate_pool(
    source_candidates: &[MemoryEvalRecallCandidate],
    graph_anchor_candidates: &[MemoryEvalRecallCandidate],
    expanded_candidates: &[MemoryEvalRecallCandidate],
    reranked_candidates: &[MemoryEvalRecallCandidate],
) -> Vec<MemoryEvalRecallCandidate> {
    let mut pool = Vec::new();
    for candidate in source_candidates
        .iter()
        .chain(graph_anchor_candidates.iter())
        .chain(expanded_candidates.iter())
        .chain(reranked_candidates.iter())
    {
        push_unique_eval_candidate(&mut pool, candidate.clone());
    }
    pool
}

fn push_unique_eval_candidate(
    candidates: &mut Vec<MemoryEvalRecallCandidate>,
    candidate: MemoryEvalRecallCandidate,
) {
    if !candidates
        .iter()
        .any(|existing| existing.candidate_id == candidate.candidate_id)
    {
        candidates.push(candidate);
    }
}

fn build_eval_recall_evidence_ref_index(
    candidates: &[MemoryEvalRecallCandidate],
) -> Vec<MemoryEvalRecallEvidenceRefIndexEntry> {
    candidates
        .iter()
        .map(|candidate| MemoryEvalRecallEvidenceRefIndexEntry {
            candidate_id: candidate.candidate_id.clone(),
            evidence_refs: candidate.evidence_refs.clone(),
        })
        .collect()
}

fn build_eval_recall_stage_diagnostics(
    benchmark_context: Option<&MemoryEvalRecallBenchmarkContext>,
    source_candidates: &[MemoryEvalRecallCandidate],
    graph_anchor_candidates: &[MemoryEvalRecallCandidate],
    expanded_candidates: &[MemoryEvalRecallCandidate],
    reranked_candidates: &[MemoryEvalRecallCandidate],
    selected_candidates: &[MemoryEvalRecallCandidate],
    rendered_candidates: &[MemoryEvalRecallCandidate],
    recall: &MemoryRecallReport,
) -> MemoryEvalRecallStageDiagnostics {
    let gold_evidence_refs = benchmark_context
        .map(|context| context.expected_evidence_refs.clone())
        .unwrap_or_default();
    let stages = [
        ("source", source_candidates),
        ("expanded", expanded_candidates),
        ("reranked", reranked_candidates),
        ("selected", selected_candidates),
        ("rendered", rendered_candidates),
    ];
    let mut matched_gold_by_stage = Vec::new();
    let mut missing_gold_by_stage = Vec::new();
    let mut gold_rank_by_stage = Vec::new();
    let mut first_any_hit_stage = None;
    let mut first_all_hit_stage = None;
    let mut expanded_missing = gold_evidence_refs.clone();

    for (stage, candidates) in stages {
        let matched_evidence_refs =
            matched_evidence_for_k(&gold_evidence_refs, candidates, candidates.len().max(1));
        let missing_evidence_refs =
            missing_from_expected(&gold_evidence_refs, &matched_evidence_refs);
        if stage == "expanded" {
            expanded_missing = missing_evidence_refs.clone();
        }
        if first_any_hit_stage.is_none() && !matched_evidence_refs.is_empty() {
            first_any_hit_stage = Some(stage.to_string());
        }
        if first_all_hit_stage.is_none()
            && !gold_evidence_refs.is_empty()
            && missing_evidence_refs.is_empty()
        {
            first_all_hit_stage = Some(stage.to_string());
        }
        matched_gold_by_stage.push(MemoryEvalRecallStageEvidenceRefs {
            stage: stage.to_string(),
            evidence_refs: matched_evidence_refs,
        });
        missing_gold_by_stage.push(MemoryEvalRecallStageEvidenceRefs {
            stage: stage.to_string(),
            evidence_refs: missing_evidence_refs,
        });
        for evidence_ref in &gold_evidence_refs {
            gold_rank_by_stage.push(MemoryEvalRecallGoldRank {
                stage: stage.to_string(),
                evidence_ref: evidence_ref.clone(),
                rank: evidence_rank_in_candidates(evidence_ref, candidates),
            });
        }
    }

    let mut source_anchor_ids = recall.graph_index_report.source_anchor_ids.clone();
    if source_anchor_ids.is_empty() {
        source_anchor_ids = source_candidates
            .iter()
            .map(|candidate| candidate.candidate_id.clone())
            .collect();
    }
    let graph_anchor_candidate_ids = graph_anchor_candidates
        .iter()
        .map(|candidate| candidate.candidate_id.clone())
        .collect::<Vec<_>>();
    let expanded_node_ids = if recall.graph_index_report.expanded_node_ids.is_empty() {
        recall.graph_rerank.graph_neighbor_ids.clone()
    } else {
        recall.graph_index_report.expanded_node_ids.clone()
    };
    let mut rendered_evidence_refs = Vec::new();
    for candidate in rendered_candidates {
        for evidence_ref in &candidate.evidence_refs {
            push_unique_string(&mut rendered_evidence_refs, evidence_ref.clone());
        }
    }
    let facet_stage = build_eval_recall_facet_stage_diagnostics(
        recall,
        !gold_evidence_refs.is_empty() && !expanded_missing.is_empty(),
        expanded_missing.clone(),
        rendered_candidates.len(),
    );
    let ablation_report = build_eval_recall_ablation_report(&facet_stage);

    MemoryEvalRecallStageDiagnostics {
        suite: benchmark_context
            .map(|context| context.suite.clone())
            .unwrap_or_default(),
        question_id: benchmark_context
            .map(|context| context.question_id.clone())
            .unwrap_or_default(),
        question_type: benchmark_context
            .map(|context| context.question_type.clone())
            .unwrap_or_default(),
        evidence_count: gold_evidence_refs.len(),
        gold_evidence_refs: gold_evidence_refs.clone(),
        first_any_hit_stage,
        first_all_hit_stage,
        matched_gold_by_stage,
        missing_gold_by_stage,
        gold_rank_by_stage,
        miss_after_expanded: !gold_evidence_refs.is_empty() && !expanded_missing.is_empty(),
        source_anchor_ids,
        graph_anchor_candidate_ids,
        expanded_node_ids,
        graph_neighbor_ids: recall.graph_rerank.graph_neighbor_ids.clone(),
        graph_distance_to_gold: graph_distance_to_gold(
            &gold_evidence_refs,
            source_candidates,
            graph_anchor_candidates,
            expanded_candidates,
            &recall.graph_rerank.graph_neighbor_ids,
        ),
        facet_stage,
        ablation_report,
        expansion_budget: recall.graph_rerank.expansion_budget.clone(),
        truncated_count: recall
            .graph_rerank
            .expansion_budget
            .truncated_candidate_count,
        blocked_reasons: recall.graph_rerank.expansion_budget.blocked_reasons.clone(),
        selected_candidate_ids: selected_candidates
            .iter()
            .map(|candidate| candidate.candidate_id.clone())
            .collect(),
        rendered_candidate_ids: rendered_candidates
            .iter()
            .map(|candidate| candidate.candidate_id.clone())
            .collect(),
        rendered_evidence_refs,
    }
}

fn build_eval_recall_facet_stage_diagnostics(
    recall: &MemoryRecallReport,
    miss_after_expanded: bool,
    expanded_missing_evidence_refs: Vec<String>,
    rendered_candidate_count: usize,
) -> MemoryEvalRecallFacetStageDiagnostics {
    let mut blocked_reasons = recall.facet_index_report.failures.clone();
    if recall.facet_index_report.report_only {
        blocked_reasons.push("memory_facet_report_only".to_string());
    }
    if !recall.facet_index_report.used {
        blocked_reasons.push("memory_facet_stage_not_used".to_string());
    }
    blocked_reasons.sort();
    blocked_reasons.dedup();

    MemoryEvalRecallFacetStageDiagnostics {
        used: recall.facet_index_report.used,
        report_only: recall.facet_index_report.report_only,
        miss_after_expanded,
        expanded_missing_evidence_refs,
        exact_facet_candidate_ids: recall.facet_index_report.exact_facet_candidate_ids.clone(),
        expanded_facet_candidate_ids: recall
            .facet_index_report
            .expanded_facet_candidate_ids
            .clone(),
        source_candidate_count: recall.facet_index_report.source_candidate_count,
        matched_source_candidate_count: recall.facet_index_report.matched_source_candidate_count,
        rendered_candidate_count,
        blocked_reasons,
    }
}

fn build_eval_recall_ablation_report(
    facet_stage: &MemoryEvalRecallFacetStageDiagnostics,
) -> crate::MemoryEvalRecallAblationReport {
    let required_slices = [
        "facet_off",
        "lexical_sparse_off",
        "graph_propagation_off",
        "rank_fusion_off",
        "coverage_selection_off",
    ]
    .iter()
    .map(|name| (*name).to_string())
    .collect::<Vec<_>>();
    let mut blocked_reasons = Vec::new();
    if facet_stage.report_only {
        blocked_reasons.push("memory_facet_ablation_report_only".to_string());
    }
    if !facet_stage.used {
        blocked_reasons.push("memory_facet_index_not_used".to_string());
    }
    blocked_reasons.sort();
    blocked_reasons.dedup();

    let slices = required_slices
        .iter()
        .map(|name| {
            let is_facet = name == "facet_off";
            crate::MemoryEvalRecallAblationSlice {
                name: name.clone(),
                feature_enabled: false,
                report_available: true,
                contribution_proven: is_facet
                    && facet_stage.used
                    && !facet_stage.expanded_missing_evidence_refs.is_empty(),
                affected_candidate_count: if is_facet {
                    facet_stage
                        .exact_facet_candidate_ids
                        .len()
                        .saturating_add(facet_stage.expanded_facet_candidate_ids.len())
                } else {
                    0
                },
                render_growth: 0,
                blocked_reasons: if is_facet {
                    blocked_reasons.clone()
                } else {
                    Vec::new()
                },
            }
        })
        .collect::<Vec<_>>();

    crate::MemoryEvalRecallAblationReport {
        required_slices,
        contribution_proven: slices.iter().any(|slice| slice.contribution_proven),
        render_growth: slices.iter().map(|slice| slice.render_growth).sum(),
        slices,
        blocked_reasons,
    }
}

fn evidence_rank_in_candidates(
    evidence_ref: &str,
    candidates: &[MemoryEvalRecallCandidate],
) -> Option<usize> {
    candidates
        .iter()
        .position(|candidate| {
            candidate
                .evidence_refs
                .iter()
                .any(|actual| actual == evidence_ref)
        })
        .map(|index| index + 1)
}

fn graph_distance_to_gold(
    expected_evidence_refs: &[String],
    source_candidates: &[MemoryEvalRecallCandidate],
    graph_anchor_candidates: &[MemoryEvalRecallCandidate],
    expanded_candidates: &[MemoryEvalRecallCandidate],
    graph_neighbor_ids: &[String],
) -> Vec<MemoryEvalRecallGraphDistanceToGold> {
    let mut distances = Vec::new();
    for candidate in expanded_candidates {
        for evidence_ref in expected_evidence_refs {
            if !candidate
                .evidence_refs
                .iter()
                .any(|actual| actual == evidence_ref)
            {
                continue;
            }
            distances.push(MemoryEvalRecallGraphDistanceToGold {
                candidate_id: candidate.candidate_id.clone(),
                evidence_ref: evidence_ref.clone(),
                distance: graph_candidate_distance(
                    candidate,
                    source_candidates,
                    graph_anchor_candidates,
                    graph_neighbor_ids,
                ),
            });
        }
    }
    distances
}

fn graph_candidate_distance(
    candidate: &MemoryEvalRecallCandidate,
    source_candidates: &[MemoryEvalRecallCandidate],
    graph_anchor_candidates: &[MemoryEvalRecallCandidate],
    graph_neighbor_ids: &[String],
) -> Option<u8> {
    if source_candidates
        .iter()
        .any(|source| source.candidate_id == candidate.candidate_id)
    {
        Some(0)
    } else if graph_neighbor_ids
        .iter()
        .any(|neighbor| neighbor == &candidate.candidate_id)
    {
        Some(1)
    } else if candidate.graph_neighbor_ids.iter().any(|neighbor| {
        graph_anchor_candidates
            .iter()
            .any(|anchor| anchor.candidate_id == *neighbor)
    }) {
        Some(1)
    } else {
        None
    }
}

fn build_eval_recall_candidate(
    candidate_id: &str,
    recall: &MemoryRecallReport,
    include_graph_neighbors: bool,
    include_score_breakdown: bool,
) -> MemoryEvalRecallCandidate {
    let node = recall
        .compact_graph
        .nodes
        .iter()
        .find(|node| node.node_id == candidate_id);
    let evidence_refs = recall
        .graph_candidate_evidence_ref_index
        .iter()
        .find(|entry| entry.candidate_id == candidate_id)
        .map(|entry| entry.evidence_refs.clone())
        .or_else(|| node.map(|node| node.evidence_refs.clone()))
        .unwrap_or_default();
    let graph_neighbor_ids = if include_graph_neighbors {
        recall
            .compact_graph
            .edges
            .iter()
            .filter_map(|edge| {
                if edge.from_node_id == candidate_id {
                    Some(edge.to_node_id.clone())
                } else if edge.to_node_id == candidate_id {
                    Some(edge.from_node_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let score_breakdown = if include_score_breakdown {
        recall
            .graph_rerank
            .score_breakdown
            .iter()
            .find(|score| score.candidate_id == candidate_id)
            .cloned()
            .unwrap_or_else(|| GraphRecallCandidateScore {
                candidate_id: candidate_id.to_string(),
                ..GraphRecallCandidateScore::default()
            })
    } else {
        GraphRecallCandidateScore {
            candidate_id: candidate_id.to_string(),
            ..GraphRecallCandidateScore::default()
        }
    };
    MemoryEvalRecallCandidate {
        candidate_id: candidate_id.to_string(),
        source: evidence_refs
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        evidence_refs,
        graph_neighbor_ids,
        score_breakdown,
    }
}

fn build_eval_recall_metrics(
    request: &MemoryEvalRecallRequest,
    source_candidate_count: usize,
    expanded_candidate_count: usize,
    selected_candidate_count: usize,
    rendered_candidate_count: usize,
    reranked_candidates: &[MemoryEvalRecallCandidate],
) -> MemoryEvalRecallMetrics {
    let expected_evidence_refs = request
        .benchmark_context
        .as_ref()
        .map(|context| context.expected_evidence_refs.as_slice())
        .unwrap_or(&[]);
    let recall_at_k = [5_usize, 10, 20, 50]
        .into_iter()
        .map(|k| {
            let matched_evidence_refs =
                matched_evidence_for_k(expected_evidence_refs, reranked_candidates, k);
            let missing_evidence_refs =
                missing_from_expected(expected_evidence_refs, &matched_evidence_refs);
            MemoryEvalRecallAtK {
                k,
                any_evidence_hit: !matched_evidence_refs.is_empty(),
                all_evidence_hit: !expected_evidence_refs.is_empty()
                    && missing_evidence_refs.is_empty(),
                matched_evidence_refs,
                missing_evidence_refs,
            }
        })
        .collect::<Vec<_>>();
    let requested_matched = matched_evidence_for_k(
        expected_evidence_refs,
        reranked_candidates,
        request.k.max(1),
    );
    let requested_missing = missing_from_expected(expected_evidence_refs, &requested_matched);

    MemoryEvalRecallMetrics {
        requested_k: request.k.max(1),
        source_candidate_count,
        expanded_candidate_count,
        selected_candidate_count,
        rendered_candidate_count,
        expected_evidence_count: expected_evidence_refs.len(),
        recall_at_k,
        any_evidence_hit: !requested_matched.is_empty(),
        all_evidence_hit: !expected_evidence_refs.is_empty() && requested_missing.is_empty(),
        mrr_bps: mean_reciprocal_rank_bps(expected_evidence_refs, reranked_candidates),
        question_type: request
            .benchmark_context
            .as_ref()
            .map(|context| context.question_type.clone())
            .unwrap_or_default(),
    }
}

fn matched_evidence_for_k(
    expected_evidence_refs: &[String],
    candidates: &[MemoryEvalRecallCandidate],
    k: usize,
) -> Vec<String> {
    let mut matched = Vec::new();
    for expected in expected_evidence_refs {
        if candidates.iter().take(k).any(|candidate| {
            candidate
                .evidence_refs
                .iter()
                .any(|actual| actual == expected)
        }) {
            matched.push(expected.clone());
        }
    }
    matched
}

fn missing_evidence_for_k(
    expected_evidence_refs: &[String],
    candidates: &[MemoryEvalRecallCandidate],
    k: usize,
) -> Vec<String> {
    let matched = matched_evidence_for_k(expected_evidence_refs, candidates, k);
    missing_from_expected(expected_evidence_refs, &matched)
}

fn missing_from_expected(expected_evidence_refs: &[String], matched: &[String]) -> Vec<String> {
    expected_evidence_refs
        .iter()
        .filter(|expected| !matched.iter().any(|item| item == *expected))
        .cloned()
        .collect()
}

fn mean_reciprocal_rank_bps(
    expected_evidence_refs: &[String],
    candidates: &[MemoryEvalRecallCandidate],
) -> u32 {
    if expected_evidence_refs.is_empty() {
        return 0;
    }
    candidates
        .iter()
        .position(|candidate| {
            candidate.evidence_refs.iter().any(|actual| {
                expected_evidence_refs
                    .iter()
                    .any(|expected| expected == actual)
            })
        })
        .map(|index| 10_000_u32 / (index as u32 + 1))
        .unwrap_or(0)
}

fn build_eval_recall_privacy_report(recall: &MemoryRecallReport) -> MemoryEvalRecallPrivacyReport {
    let failures = recall
        .graph_gate
        .failures
        .iter()
        .filter(|failure| failure.contains("raw_soul_private"))
        .cloned()
        .collect::<Vec<_>>();
    MemoryEvalRecallPrivacyReport {
        passed: failures.is_empty(),
        private_raw_candidate_count: failures.len() as u32,
        redacted_candidate_ids: Vec::new(),
        failures,
    }
}

fn render_eval_recall_block_preview(candidates: &[MemoryEvalRecallCandidate]) -> String {
    candidates
        .iter()
        .map(|candidate| {
            format!(
                "{} [{}]",
                candidate.candidate_id,
                candidate.evidence_refs.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct TemporalMemoryGraphPlannedMutations {
    mutations: Vec<StoreMutation>,
    index_count: usize,
    index_revision: String,
}

fn temporal_memory_graph_plan_mutations(
    plan: &MemoryGraphWritePlan,
    memory_space_id: &str,
    subject_id: &str,
) -> Result<TemporalMemoryGraphPlannedMutations> {
    let mut mutations = Vec::new();
    for node in &plan.nodes {
        mutations.push(StoreMutation::PutJson {
            namespace: "memory_graph_nodes".to_string(),
            key: node.node_id.clone(),
            value: serde_json::to_value(node)
                .map_err(|error| Error::config("memory_graph_node_serialize", error.to_string()))?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "memory_graph".to_string(),
            record_key: node.node_id.clone(),
        });
    }
    for edge in &plan.edges {
        mutations.push(StoreMutation::PutJson {
            namespace: "memory_graph_edges".to_string(),
            key: edge.edge_id.clone(),
            value: serde_json::to_value(edge)
                .map_err(|error| Error::config("memory_graph_edge_serialize", error.to_string()))?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "memory_graph".to_string(),
            record_key: edge.edge_id.clone(),
        });
    }
    for backlink in &plan.backlinks {
        mutations.push(StoreMutation::PutJson {
            namespace: "memory_graph_backlinks".to_string(),
            key: memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id),
            value: serde_json::to_value(backlink).map_err(|error| {
                Error::config("memory_graph_backlink_serialize", error.to_string())
            })?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "memory_graph".to_string(),
            record_key: backlink.source_id.clone(),
        });
    }
    let revision = serde_json::json!({
        "operation": plan.operation,
        "memory_space_id": memory_space_id,
        "subject_id": subject_id,
        "node_count": plan.node_count,
        "edge_count": plan.edge_count,
        "backlink_count": plan.backlink_count,
        "gate_failures": plan.gate_failures,
    });
    let index_revision = format!(
        "revision:{}",
        stable_hash_json(&revision).map_err(|error| {
            Error::config("memory_graph_revision_fingerprint", error.to_string())
        })?
    );
    mutations.push(StoreMutation::PutJson {
        namespace: "memory_graph_revisions".to_string(),
        key: index_revision.clone(),
        value: revision,
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: "memory_graph".to_string(),
        record_key: "memory_graph_revision".to_string(),
    });
    let index_docs = build_memory_graph_recall_index_docs(
        "bm-sdk::MemoryRuntime",
        &index_revision,
        memory_space_id,
        subject_id,
        &plan.nodes,
        &plan.edges,
        &plan.backlinks,
    );
    let index_count = index_docs.len();
    for index_doc in index_docs {
        mutations.push(StoreMutation::PutJson {
            namespace: "memory_graph_indexes".to_string(),
            key: index_doc.index_id.clone(),
            value: serde_json::to_value(index_doc).map_err(|error| {
                Error::config("memory_graph_index_serialize", error.to_string())
            })?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "memory_graph".to_string(),
            record_key: "memory_graph_index".to_string(),
        });
    }
    Ok(TemporalMemoryGraphPlannedMutations {
        mutations,
        index_count,
        index_revision,
    })
}

fn first_non_empty<'a>(values: &[&'a str]) -> &'a str {
    values
        .iter()
        .find_map(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .unwrap_or("")
}

fn build_skill_evolution_report_from_write_outcome(
    writes: &[RuntimeSkillWrite],
    outcome: &crate::RuntimeSkillWriteOutcome,
) -> SkillEvolutionReport {
    let mut report = SkillEvolutionReport::default();
    for (idx, item) in outcome.reports.iter().enumerate() {
        let write_name = writes
            .get(idx)
            .map(|write| write.name.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| item.topic.clone());
        match item.action {
            RuntimeSkillWriteAction::Accepted => report.added.push(write_name),
            RuntimeSkillWriteAction::Rejected => report.rejected.push(write_name),
        }
        report.reasons.push(format!(
            "{}:{}:{}",
            item.source.label(),
            item.reason.label(),
            item.detail
        ));
    }
    if outcome.accepted > 0 && outcome.changed == 0 {
        report
            .reasons
            .push("accepted_without_store_delta_existing_record".to_string());
    }
    report.added.sort();
    report.added.dedup();
    report.rejected.sort();
    report.rejected.dedup();
    report.reasons.sort();
    report.reasons.dedup();
    report
}

fn runtime_skill_write_from_promotion_report(
    report: &ProceduralMemoryPromotionReport,
    source_chat_id: Option<&str>,
    now_secs: u64,
) -> Option<RuntimeSkillWrite> {
    let record = report.record.as_ref()?;
    Some(RuntimeSkillWrite {
        name: sdk_runtime_skill_name(&record.trigger),
        topic: record.trigger.clone(),
        title: record.trigger.clone(),
        summary: record.procedure.clone(),
        content: render_promoted_procedure(record),
        citations: record.evidence_refs.clone(),
        source_chat_id: source_chat_id.map(str::to_string),
        observed_at: now_secs,
    })
}

fn render_promoted_procedure(record: &bm_core::memory::ProceduralMemoryRecordV2) -> String {
    let mut lines = Vec::new();
    lines.push(record.procedure.clone());
    if !record.constraints.is_empty() {
        lines.push(format!("Constraints: {}", record.constraints.join("; ")));
    }
    if !record.failure_modes.is_empty() {
        lines.push(format!(
            "Failure modes: {}",
            record.failure_modes.join("; ")
        ));
    }
    if !record.counterfactual_fix.trim().is_empty() {
        lines.push(format!("Counterfactual fix: {}", record.counterfactual_fix));
    }
    lines.join("\n")
}

fn merge_promotion_and_write_evolution(
    promotions: &[ProceduralMemoryPromotionReport],
    writes: &[RuntimeSkillWrite],
    outcome: &crate::RuntimeSkillWriteOutcome,
) -> SkillEvolutionReport {
    let mut report = build_skill_evolution_report_from_write_outcome(writes, outcome);
    for promotion in promotions {
        report
            .reasons
            .extend(promotion.evolution.reasons.iter().cloned());
        report
            .added
            .extend(promotion.evolution.added.iter().cloned());
        report
            .rejected
            .extend(promotion.evolution.rejected.iter().cloned());
        report
            .merged
            .extend(promotion.evolution.merged.iter().cloned());
        report
            .retired
            .extend(promotion.evolution.retired.iter().cloned());
        report
            .demoted
            .extend(promotion.evolution.demoted.iter().cloned());
    }
    report.added.sort();
    report.added.dedup();
    report.rejected.sort();
    report.rejected.dedup();
    report.merged.sort();
    report.merged.dedup();
    report.retired.sort();
    report.retired.dedup();
    report.demoted.sort();
    report.demoted.dedup();
    report.reasons.sort();
    report.reasons.dedup();
    report
}

fn runtime_skill_write_source_requires_promotion(source: RuntimeSkillWriteSource) -> bool {
    matches!(
        source,
        RuntimeSkillWriteSource::TaskLearning | RuntimeSkillWriteSource::ProgrammableReasoning
    )
}

fn projection_id(runtime: &MemoryRuntime, request: &MemoryProjectionRequest) -> String {
    let seed = format!(
        "{}:{}:{}:{}",
        runtime.config.memory_space_id,
        runtime.config.scope.channel,
        runtime.config.scope.chat_id,
        request.user_query
    );
    format!("projection-{:016x}", fnv1a64(seed.as_bytes()))
}

fn projection_source_audits(
    context: &crate::PromptMemoryContext,
    agent_skill_registry: &AgentSkillRegistrySnapshot,
    agent_tool_registries: &[AgentToolRegistrySnapshot],
    runtime_projection: &LLMRuntimeProjectionEnvelope,
) -> Vec<MemoryProjectionSourceAudit> {
    let mut sources = vec![
        projection_source_audit(&context.shared_factual_recall_report),
        projection_source_audit(&context.continuity_capsule_report),
        projection_source_audit(&context.archive_recall_report),
        projection_source_audit(&context.runtime_skill_recall_report),
    ];
    if let Some(report) = context.task_recall_report.as_ref() {
        sources.push(projection_source_audit(report));
    }
    let agent_skill_report = agent_skill_registry.report();
    let selected_ids = runtime_projection
        .agent_skill_hints
        .iter()
        .map(|hint| hint.package_id.clone())
        .collect::<Vec<_>>();
    sources.push(MemoryProjectionSourceAudit {
        plane: "agent_skill".to_string(),
        backend: "host_directory_read_only".to_string(),
        candidate_count: agent_skill_report.active_packages,
        selected_count: selected_ids.len(),
        selected_ids,
        miss_reason: if runtime_projection.agent_skill_hints.is_empty()
            && agent_skill_report.active_packages > 0
        {
            Some("no_agent_skill_recall_match".to_string())
        } else if agent_skill_report.active_packages == 0 {
            Some("agent_skill_directory_empty_or_not_mounted".to_string())
        } else {
            None
        },
    });
    let agent_tool_candidate_count = agent_tool_registries
        .iter()
        .map(|registry| registry.tools.len())
        .sum::<usize>();
    let agent_tool_selected_ids = runtime_projection
        .agent_tool_hints
        .iter()
        .map(|hint| format!("{}:{}", hint.registry_id, hint.tool_id))
        .collect::<Vec<_>>();
    sources.push(MemoryProjectionSourceAudit {
        plane: "agent_tool_experience".to_string(),
        backend: "host_registry_experience_only".to_string(),
        candidate_count: agent_tool_candidate_count,
        selected_count: agent_tool_selected_ids.len(),
        selected_ids: agent_tool_selected_ids,
        miss_reason: if runtime_projection.agent_tool_hints.is_empty()
            && agent_tool_candidate_count > 0
        {
            Some("no_governed_tool_experience".to_string())
        } else if agent_tool_candidate_count == 0 {
            Some("agent_tool_registry_empty_or_not_registered".to_string())
        } else {
            None
        },
    });
    sources
}

fn projection_source_audit(report: &RecallSelectionReport) -> MemoryProjectionSourceAudit {
    MemoryProjectionSourceAudit {
        plane: report.plane.label().to_string(),
        backend: report.backend.clone(),
        candidate_count: report.candidate_count,
        selected_count: report.selected_count,
        selected_ids: report.selected_ids.clone(),
        miss_reason: report.miss_reason.clone(),
    }
}

fn projection_section_audits(
    runtime_projection: &LLMRuntimeProjectionEnvelope,
) -> Vec<MemoryProjectionSectionAudit> {
    runtime_projection
        .section_names
        .iter()
        .map(|name| MemoryProjectionSectionAudit {
            name: name.clone(),
            chars: runtime_projection_section_chars(runtime_projection, name),
            included: true,
        })
        .collect()
}

fn runtime_projection_section_chars(
    runtime_projection: &LLMRuntimeProjectionEnvelope,
    name: &str,
) -> usize {
    match name {
        "subject_mount" => {
            runtime_projection
                .subject_mount
                .identity_mount
                .chars()
                .count()
                + runtime_projection
                    .subject_mount
                    .relationship_position
                    .chars()
                    .count()
                + runtime_projection
                    .subject_mount
                    .situated_now
                    .chars()
                    .count()
        }
        "governed_memory_evidence" => runtime_projection
            .governed_memory_evidence
            .iter()
            .map(|block| block.content.chars().count())
            .sum(),
        "boundary_and_disclosure_protocol" => runtime_projection
            .boundary_protocol
            .disclosure_rule
            .chars()
            .count(),
        "runtime_constraints" => {
            runtime_projection.runtime_awareness.chars().count()
                + runtime_projection
                    .runtime_constraints
                    .iter()
                    .map(|constraint| constraint.chars().count())
                    .sum::<usize>()
        }
        "work_integrity_covenant" => {
            runtime_projection.work_integrity.task_goal.chars().count()
                + runtime_projection
                    .work_integrity
                    .evidence_ceiling
                    .chars()
                    .count()
        }
        "procedural_evidence" => runtime_projection
            .procedural_evidence
            .iter()
            .map(|block| block.content.chars().count())
            .sum(),
        "protected_private_runtime_context" => runtime_projection
            .protected_private_runtime_context
            .iter()
            .map(|block| block.content.chars().count())
            .sum(),
        "agent_skill_hints" => runtime_projection
            .agent_skill_hints
            .iter()
            .map(|hint| hint.prompt_snippet.chars().count())
            .sum(),
        _ => 0,
    }
}

fn render_runtime_awareness_block(
    profile: ProfileId,
    request_pressure: PressureLevel,
    observed_pressure: PressureLevel,
) -> String {
    let pressure = max_pressure(request_pressure, observed_pressure);
    format!(
        "## Runtime Awareness\n- Beetle Memory supplies memory context for this turn.\n- The sections below are current-turn grounding for the reply.\n- Resource pressure: {}.\n- Runtime profile: {}.",
        runtime_awareness_pressure_label(pressure),
        runtime_awareness_profile_label(profile),
    )
}

fn runtime_awareness_pressure_label(pressure: PressureLevel) -> &'static str {
    match pressure {
        PressureLevel::Normal => "normal",
        PressureLevel::Cautious => "cautious",
        PressureLevel::Critical => "critical",
    }
}

fn runtime_awareness_profile_label(profile: ProfileId) -> &'static str {
    match profile {
        ProfileId::EspStandaloneMemory => "embedded standalone memory",
        ProfileId::EspEmbeddedSdk => "embedded SDK memory",
        ProfileId::LinuxDeviceStandaloneMemory => "Linux device standalone memory",
        ProfileId::DesktopMacosStandaloneMemory => "macOS desktop standalone memory",
        ProfileId::DesktopMacosEmbeddedSdk => "macOS desktop SDK memory",
        ProfileId::DesktopWindowsEmbeddedSdk => "Windows desktop SDK memory",
        ProfileId::ServerLinuxMemoryGateway => "server memory gateway",
        ProfileId::ServerLinuxDevFull => "server development runtime",
    }
}

fn prompt_context_hit_count(context: &crate::PromptMemoryContext) -> usize {
    context
        .shared_factual_recall_report
        .selected_count
        .saturating_add(context.continuity_capsule_report.selected_count)
        .saturating_add(context.archive_recall_report.selected_count)
        .saturating_add(context.runtime_skill_recall_report.selected_count)
        .saturating_add(
            context
                .task_recall_report
                .as_ref()
                .map(|report| report.selected_count)
                .unwrap_or(0),
        )
        .saturating_add(usize::from(context.long_term_memory_text.is_some()))
        .saturating_add(usize::from(context.continuity_capsule_text.is_some()))
        .saturating_add(usize::from(context.archive_evidence_text.is_some()))
        .saturating_add(usize::from(context.runtime_skill_text.is_some()))
        .saturating_add(usize::from(context.task_recall_text.is_some()))
}

fn working_recall_hit_count(working: &crate::WorkingRecallInspection) -> usize {
    working
        .shared_factual_report
        .selected_count
        .saturating_add(working.continuity_capsule_report.selected_count)
        .saturating_add(working.archive_recall_report.selected_count)
        .saturating_add(working.runtime_skill_report.selected_count)
        .saturating_add(
            working
                .task_recall_report
                .as_ref()
                .map(|report| report.selected_count)
                .unwrap_or(0),
        )
        .saturating_add(usize::from(working.long_term_memory_text.is_some()))
        .saturating_add(usize::from(working.continuity_capsule_text.is_some()))
        .saturating_add(usize::from(working.archive_evidence_text.is_some()))
        .saturating_add(usize::from(working.runtime_skill_text.is_some()))
        .saturating_add(usize::from(working.task_recall_text.is_some()))
}

fn projection_source_runtime_text<'a>(
    context: &'a crate::PromptMemoryContext,
    source_id: &str,
) -> Option<&'a str> {
    match source_id {
        "summary" => context.summary_text.as_deref(),
        "message_summary" => context.message_summary_text.as_deref(),
        "long_term_memory" => context.long_term_memory_text.as_deref(),
        "continuity_capsule" => context.continuity_capsule_text.as_deref(),
        "archive_evidence" => context.archive_evidence_text.as_deref(),
        "runtime_skill" => context.runtime_skill_text.as_deref(),
        "recent_turn_observation" => context.recent_turn_observation_text.as_deref(),
        "work_continuity" => context.work_continuity_text.as_deref(),
        "execution_state" => context.execution_state_text.as_deref(),
        "task_workspace" => context.task_workspace_text.as_deref(),
        "task_recall" => context.task_recall_text.as_deref(),
        "world_snapshot" => context.world_snapshot_text.as_deref(),
        "world_sense" => context.world_sense_text.as_deref(),
        "self_state" => context.self_state_text.as_deref(),
        "self_model" => context.self_model_text.as_deref(),
        "inner_life" => context.inner_life_text.as_deref(),
        "self_continuity" => context.self_continuity_text.as_deref(),
        "private_workspace" => context.private_workspace_text.as_deref(),
        "private_garden" => context.private_garden_text.as_deref(),
        "mental_privacy" => context.mental_privacy_text.as_deref(),
        "mental_privacy_adjudication" => context.mental_privacy_adjudication_text.as_deref(),
        _ => None,
    }
}

fn compact_runtime_projection_content(value: &str, max_len: usize) -> String {
    let compact = value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    truncate_to_char_boundary(compact.trim(), max_len)
}

fn truncate_to_char_boundary(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let mut end = max_len;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn validate_turn_scope(
    scope: &MemoryScope,
    subject_id: &str,
    turn: &CanonicalTurnDelta,
) -> Result<()> {
    if turn.turn_id.trim().is_empty() {
        return Err(Error::config(
            "canonical_turn_delta",
            "turn_id must not be empty",
        ));
    }
    if turn.conversation.channel.trim() != scope.channel {
        return Err(Error::config(
            "canonical_turn_delta",
            "turn conversation channel must match runtime scope",
        ));
    }
    if turn.conversation.chat_id.trim() != scope.chat_id {
        return Err(Error::config(
            "canonical_turn_delta",
            "turn conversation chat_id must match runtime scope",
        ));
    }
    if let Some(scope_conversation_id) = scope.conversation_id.as_deref() {
        let turn_conversation_id = turn
            .conversation
            .conversation_id
            .as_deref()
            .unwrap_or(turn.conversation.chat_id.as_str())
            .trim();
        if turn_conversation_id != scope_conversation_id {
            return Err(Error::config(
                "canonical_turn_delta",
                "turn conversation_id must match runtime scope",
            ));
        }
    }
    if turn.subject.trim().is_empty() {
        return Err(Error::config(
            "canonical_turn_delta",
            "turn subject must not be empty",
        ));
    }
    if turn.subject.trim() != subject_id {
        return Err(Error::config(
            "canonical_turn_delta",
            "turn subject must match runtime subject",
        ));
    }
    Ok(())
}

fn latest_user_content(turn: &CanonicalTurnDelta) -> String {
    turn.input_messages
        .iter()
        .rev()
        .find(|message| {
            message.role.eq_ignore_ascii_case("user") && !message.content.trim().is_empty()
        })
        .map(|message| message.content.trim().to_string())
        .unwrap_or_default()
}

fn assistant_content(turn: &CanonicalTurnDelta) -> Option<String> {
    turn.assistant_message
        .as_ref()
        .filter(|message| message.role.eq_ignore_ascii_case("assistant"))
        .map(|message| message.content.trim().to_string())
        .filter(|content| !content.is_empty())
}

fn semantic_report_from_maintenance(
    maintenance: &MemoryMaintenanceReport,
    long_term_refresh: Option<&LongTermMemoryRefreshOutcome>,
) -> PostTurnSemanticGovernanceReport {
    let Some(report) = maintenance.report.as_ref() else {
        return PostTurnSemanticGovernanceReport::skipped("maintenance_report_unavailable");
    };
    let extraction_requested = matches!(
        report.extraction_request_outcome,
        LongTermMemoryRefreshRequestOutcome::Requested
    );
    let extraction_failed = matches!(
        report.extraction_request_outcome,
        LongTermMemoryRefreshRequestOutcome::RequestFailed
    );
    let factual_signal = report.factual_refresh_suggested
        || extraction_requested
        || extraction_failed
        || report.factual_coordination_summary.is_some();
    let mut plane_reports = Vec::new();
    if factual_signal || long_term_refresh.is_some() {
        let (decision, authority, accepted_count, reason) = match long_term_refresh {
            Some(LongTermMemoryRefreshOutcome::Processed { changed_count, .. })
                if *changed_count > 0 =>
            {
                (
                    GovernedWriteDecision::Accepted,
                    MemoryWriteAuthority::LlmGovernedSemantic,
                    *changed_count,
                    "long_term_extraction_applied".to_string(),
                )
            }
            Some(LongTermMemoryRefreshOutcome::Processed { .. }) => (
                GovernedWriteDecision::NotApplicable,
                MemoryWriteAuthority::LlmGovernedSemantic,
                0,
                "long_term_extraction_noop".to_string(),
            ),
            Some(LongTermMemoryRefreshOutcome::Deferred { .. }) => (
                GovernedWriteDecision::Deferred,
                MemoryWriteAuthority::RuntimeDeterministic,
                0,
                "long_term_extraction_deferred".to_string(),
            ),
            Some(LongTermMemoryRefreshOutcome::Failed { .. }) => (
                GovernedWriteDecision::Rejected,
                MemoryWriteAuthority::LlmGovernedSemantic,
                0,
                "long_term_extraction_failed".to_string(),
            ),
            None if extraction_requested || report.factual_refresh_suggested => (
                GovernedWriteDecision::Deferred,
                MemoryWriteAuthority::RuntimeDeterministic,
                0,
                if extraction_requested {
                    "long_term_extraction_requested".to_string()
                } else {
                    "factual_refresh_suggested".to_string()
                },
            ),
            None if extraction_failed => (
                GovernedWriteDecision::Rejected,
                MemoryWriteAuthority::RuntimeDeterministic,
                0,
                "long_term_extraction_request_failed".to_string(),
            ),
            None => (
                GovernedWriteDecision::NotApplicable,
                MemoryWriteAuthority::RuntimeDeterministic,
                0,
                "factual_plane_checked".to_string(),
            ),
        };
        plane_reports.push(MemoryPlaneGovernanceReport {
            domain: MemoryWriteDomain::Program,
            plane: "long_term_memory".to_string(),
            authority,
            decision,
            reason,
            evidence_refs: Vec::new(),
            privacy_decision: "runtime_policy".to_string(),
            profile_decision: "profile_capability_checked".to_string(),
        });
        let proposal_count = if accepted_count > 0 {
            accepted_count
        } else if plane_reports
            .last()
            .is_some_and(|plane| plane.decision != GovernedWriteDecision::NotApplicable)
        {
            1
        } else {
            0
        };
        let rejected_count = usize::from(
            plane_reports
                .last()
                .is_some_and(|plane| plane.decision == GovernedWriteDecision::Rejected),
        );
        let deferred_count = usize::from(
            plane_reports
                .last()
                .is_some_and(|plane| plane.decision == GovernedWriteDecision::Deferred),
        );
        return PostTurnSemanticGovernanceReport {
            attempted: true,
            executed: true,
            skipped_reason: None,
            proposal_count,
            accepted_count,
            rejected_count,
            deferred_count,
            plane_reports,
            soul_candidate_handoffs: Vec::new(),
        };
    }

    PostTurnSemanticGovernanceReport {
        attempted: true,
        executed: true,
        skipped_reason: None,
        proposal_count: 0,
        accepted_count: 0,
        rejected_count: 0,
        deferred_count: 0,
        plane_reports,
        soul_candidate_handoffs: Vec::new(),
    }
}

const REL_PATH_DEFERRED_GOVERNANCE_JOBS: &str = "memory/governance_jobs/pending.json";

fn enqueue_deferred_governance_job(
    platform: &dyn Platform,
    scope: &MemoryScope,
    session_commit: &bm_core::memory::SessionTurnCommitReport,
    memory_space_id: &str,
    request: &MemoryTurnFinalizeRequest,
    reason: &'static str,
    now_secs: u64,
) -> Result<()> {
    if !session_commit.committed {
        return Ok(());
    }
    let mut jobs = read_deferred_governance_jobs(platform)?;
    let memory_space_id = memory_space_id.trim();
    let subject_id = request.turn.subject.trim();
    let idempotency_key = format!(
        "{}:{}:{}:{}:{}",
        memory_space_id,
        subject_id,
        scope.channel,
        session_commit.chat_id,
        if request.turn.turn_id.trim().is_empty() {
            session_commit.after_count.to_string()
        } else {
            request.turn.turn_id.trim().to_string()
        }
    );
    if jobs
        .iter()
        .any(|job| job.idempotency_key == idempotency_key)
    {
        return Ok(());
    }
    jobs.push(DeferredGovernanceJob {
        job_id: format!("governance-{:016x}", fnv1a64(idempotency_key.as_bytes())),
        idempotency_key,
        status: DeferredGovernanceJobStatus::Pending,
        memory_space_id: memory_space_id.to_string(),
        subject_id: subject_id.to_string(),
        channel: scope.channel.clone(),
        chat_id: session_commit.chat_id.clone(),
        conversation_id: request.turn.conversation.conversation_id.clone(),
        turn_id: request.turn.turn_id.trim().to_string(),
        candidate_ids: request.turn.candidate_ids.clone(),
        reason: reason.to_string(),
        retry_policy: "standard_backoff".to_string(),
        created_at: now_secs,
        attempts: 0,
        turn: Some(request.turn.clone()),
        tool_calls: request.tool_calls,
        runtime_skill_selected_ids: request.runtime_skill_selected_ids.clone(),
        task_learning_selected_ids: request.task_learning_selected_ids.clone(),
        reuse_outcome_note: request.reuse_outcome_note.clone(),
        pressure: request.pressure,
        mode_input: request.mode_input,
        last_error: None,
    });
    write_deferred_governance_jobs(platform, &jobs)
}

fn deferred_governance_job_matches_runtime(
    job: &DeferredGovernanceJob,
    config: &MemoryRuntimeConfig,
) -> bool {
    job.memory_space_id.trim() == config.memory_space_id
        && job.subject_id.trim() == config.subject_id
        && job.channel.trim() == config.scope.channel
        && job.chat_id.trim() == config.scope.chat_id
}

fn scoped_deferred_governance_jobs(
    jobs: &[DeferredGovernanceJob],
    config: &MemoryRuntimeConfig,
) -> Vec<DeferredGovernanceJob> {
    jobs.iter()
        .filter(|job| deferred_governance_job_matches_runtime(job, config))
        .cloned()
        .collect()
}

fn read_deferred_governance_jobs(platform: &dyn Platform) -> Result<Vec<DeferredGovernanceJob>> {
    let state_fs = platform.state_fs();
    match state_fs.read(REL_PATH_DEFERRED_GOVERNANCE_JOBS)? {
        Some(bytes) if !bytes.is_empty() => {
            serde_json::from_slice::<Vec<DeferredGovernanceJob>>(&bytes)
                .map_err(|error| Error::config("deferred_governance_jobs", error.to_string()))
        }
        _ => Ok(Vec::new()),
    }
}

fn write_deferred_governance_jobs(
    platform: &dyn Platform,
    jobs: &[DeferredGovernanceJob],
) -> Result<()> {
    let state_fs = platform.state_fs();
    let bytes = serde_json::to_vec_pretty(&jobs)
        .map_err(|error| Error::config("deferred_governance_jobs", error.to_string()))?;
    state_fs.write(REL_PATH_DEFERRED_GOVERNANCE_JOBS, &bytes)?;
    Ok(())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn bound_text_for_budget(value: &str, max_chars: usize, max_bytes: usize) -> String {
    let mut out = value.chars().take(max_chars).collect::<String>();
    if out.len() > max_bytes {
        out = truncate_to_char_boundary(&out, max_bytes);
    }
    out
}

fn max_pressure(left: PressureLevel, right: PressureLevel) -> PressureLevel {
    match (left, right) {
        (PressureLevel::Critical, _) | (_, PressureLevel::Critical) => PressureLevel::Critical,
        (PressureLevel::Cautious, _) | (_, PressureLevel::Cautious) => PressureLevel::Cautious,
        _ => PressureLevel::Normal,
    }
}

fn checked_non_empty<'a>(value: &'a str, stage: &'static str, message: &str) -> Result<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::config(stage, message));
    }
    Ok(trimmed)
}

fn checked_skill_name<'a>(value: &'a str, stage: &'static str) -> Result<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return Err(Error::config(stage, "skill name empty or contains .. / \\"));
    }
    Ok(trimmed)
}

fn transcript_replay_limit(runtime_budget: &RuntimeBudgetReport, requested_limit: usize) -> usize {
    requested_limit.max(1).min(
        runtime_budget
            .transcript_governance_budget
            .transcript_page_size
            .max(1),
    )
}

fn validate_transcript_attr_write_request(
    transcript_store: &dyn ConversationTranscriptStore,
    key: &ConversationKey,
    attrs: &[TranscriptAttrEnvelope],
) -> Result<TranscriptAttrWriteReport> {
    let mut accepted_attrs = Vec::new();
    let mut rejected_attrs = Vec::new();
    for attr in attrs {
        if attr.target.key != *key {
            rejected_attrs.push(transcript_attr_rejection(
                attr,
                "transcript_attr_target_key_mismatch",
            ));
            continue;
        }
        let Some(turn) = transcript_store.get_turn(key, &attr.target.turn_id)? else {
            rejected_attrs.push(transcript_attr_rejection(
                attr,
                "transcript_attr_target_turn_missing",
            ));
            continue;
        };
        match attr.validate_for_record(&turn) {
            Ok(()) => accepted_attrs.push(attr.clone()),
            Err(error) => rejected_attrs.push(transcript_attr_rejection(attr, error.to_string())),
        }
    }
    Ok(TranscriptAttrWriteReport {
        key: key.clone(),
        accepted_attrs,
        rejected_attrs,
    })
}

fn transcript_attr_rejection(
    attr: &TranscriptAttrEnvelope,
    reason: impl Into<String>,
) -> TranscriptAttrWriteRejection {
    TranscriptAttrWriteRejection {
        attr_id: attr.attr_id.clone(),
        attr_key: attr.key.clone(),
        reason: reason.into(),
    }
}

fn transcript_attr_write_redactions_preview(
    attrs: &[TranscriptAttrEnvelope],
    rejections: &[TranscriptAttrWriteRejection],
    redaction_items_limit: usize,
) -> (Vec<TranscriptRedactionReportItem>, bool) {
    let mut redactions = Vec::new();
    for rejection in rejections {
        let attr = attrs
            .iter()
            .find(|attr| attr.attr_id == rejection.attr_id && attr.key == rejection.attr_key);
        let reason = transcript_attr_write_redaction_reason(&rejection.reason);
        redactions.push(TranscriptRedactionReportItem {
            turn_id: attr
                .map(|attr| attr.target.turn_id.clone())
                .unwrap_or_else(|| "*".to_string()),
            message_id: attr.and_then(|attr| attr.target.message_id.clone()),
            host_ref_index: None,
            attr_id: Some(rejection.attr_id.clone()),
            attr_key: Some(rejection.attr_key.clone()),
            reason,
            source_authority: None,
            view: TranscriptReplayView::OperatorAudit,
        });
    }
    let limit = redaction_items_limit.max(1);
    let profile_budget_applied = redactions.len() > limit;
    if profile_budget_applied {
        redactions.truncate(limit);
    }
    (redactions, profile_budget_applied)
}

fn transcript_attr_write_redaction_reason(reason: &str) -> TranscriptRedactionReason {
    if reason.contains("max_value_bytes")
        || reason.contains("value exceeds")
        || reason.contains("value_kind")
    {
        TranscriptRedactionReason::AttrValueBudget
    } else {
        TranscriptRedactionReason::AttrVisibility
    }
}

fn apply_transcript_governance_budget_to_slice(
    slice: &mut RedactedTranscriptSlice,
    budget: TranscriptGovernanceBudget,
) {
    let host_ref_limit = budget.host_refs_per_turn.max(1);
    let turn_attr_limit = budget.max_attrs_per_turn.max(1);
    let message_attr_limit = budget.max_attrs_per_message.max(1);
    let mut profile_budget_limited = false;
    for turn in &mut slice.turns {
        let original_len = turn.host_refs.len();
        if original_len > host_ref_limit {
            for index in host_ref_limit..original_len {
                slice.redactions.push(TranscriptRedactionReportItem {
                    turn_id: turn.turn_id.clone(),
                    message_id: None,
                    host_ref_index: Some(index),
                    attr_id: None,
                    attr_key: None,
                    reason: TranscriptRedactionReason::ProfileBudget,
                    source_authority: None,
                    view: slice.view,
                });
            }
            turn.host_refs.truncate(host_ref_limit);
            slice.audit.redacted_host_refs = slice
                .audit
                .redacted_host_refs
                .saturating_add(original_len.saturating_sub(host_ref_limit));
            profile_budget_limited = true;
        }
        if truncate_transcript_attrs_for_budget(
            &turn.turn_id,
            None,
            &mut turn.attrs,
            turn_attr_limit,
            slice.view,
            &mut slice.redactions,
        ) {
            profile_budget_limited = true;
        }
        for message in &mut turn.input_messages {
            if truncate_transcript_attrs_for_budget(
                &turn.turn_id,
                Some(&message.message_id),
                &mut message.attrs,
                message_attr_limit,
                slice.view,
                &mut slice.redactions,
            ) {
                profile_budget_limited = true;
            }
        }
        if let Some(message) = turn.assistant_message.as_mut() {
            if truncate_transcript_attrs_for_budget(
                &turn.turn_id,
                Some(&message.message_id),
                &mut message.attrs,
                message_attr_limit,
                slice.view,
                &mut slice.redactions,
            ) {
                profile_budget_limited = true;
            }
        }
    }

    let redaction_limit = budget.redaction_items_per_page.max(1);
    if slice.redactions.len() > redaction_limit {
        slice.redactions.truncate(redaction_limit);
        profile_budget_limited = true;
    }
    slice.audit.redaction_reasons = transcript_redaction_reasons(&slice.redactions);
    if profile_budget_limited
        && !slice
            .audit
            .redaction_reasons
            .contains(&TranscriptRedactionReason::ProfileBudget)
    {
        slice
            .audit
            .redaction_reasons
            .push(TranscriptRedactionReason::ProfileBudget);
    }
}

fn truncate_transcript_attrs_for_budget(
    turn_id: &str,
    message_id: Option<&str>,
    attrs: &mut Vec<TranscriptAttrEnvelope>,
    limit: usize,
    view: TranscriptReplayView,
    redactions: &mut Vec<TranscriptRedactionReportItem>,
) -> bool {
    let original_len = attrs.len();
    if original_len <= limit {
        return false;
    }
    for attr in attrs.iter().skip(limit) {
        redactions.push(TranscriptRedactionReportItem {
            turn_id: turn_id.to_string(),
            message_id: message_id.map(str::to_string),
            host_ref_index: None,
            attr_id: Some(attr.attr_id.clone()),
            attr_key: Some(attr.key.clone()),
            reason: TranscriptRedactionReason::AttrValueBudget,
            source_authority: None,
            view,
        });
    }
    attrs.truncate(limit);
    true
}

fn apply_transcript_lifecycle_budget(
    report: &mut CoreTranscriptLifecycleReport,
    budget: TranscriptGovernanceBudget,
) {
    let host_ref_limit = budget.host_refs_per_turn.max(1);
    if report.affected_host_refs.len() > host_ref_limit {
        for index in host_ref_limit..report.affected_host_refs.len() {
            report
                .host_ref_redactions
                .push(TranscriptRedactionReportItem {
                    turn_id: report
                        .affected_turn_ids
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "*".to_string()),
                    message_id: None,
                    host_ref_index: Some(index),
                    attr_id: None,
                    attr_key: None,
                    reason: TranscriptRedactionReason::ProfileBudget,
                    source_authority: None,
                    view: TranscriptReplayView::OperatorAudit,
                });
        }
        report.redacted_host_refs = report.redacted_host_refs.saturating_add(
            report
                .affected_host_refs
                .len()
                .saturating_sub(host_ref_limit),
        );
        report.affected_host_refs.truncate(host_ref_limit);
        report.profile_budget_applied = true;
    }
    let derived_ref_limit = budget.derived_refs_per_report.max(1);
    if report.derived_memory_refs.len() > derived_ref_limit {
        report.derived_memory_refs.truncate(derived_ref_limit);
        report.profile_budget_applied = true;
    }
    let redaction_limit = budget.redaction_items_per_page.max(1);
    if report.host_ref_redactions.len() > redaction_limit {
        report.host_ref_redactions.truncate(redaction_limit);
        report.profile_budget_applied = true;
    }
}

fn sanitize_transcript_lifecycle_report_for_view(
    report: &mut CoreTranscriptLifecycleReport,
    view: TranscriptReplayView,
) {
    if report.affected_host_refs.is_empty() {
        return;
    }
    let turn_id = report
        .affected_turn_ids
        .first()
        .cloned()
        .unwrap_or_else(|| "*".to_string());
    let (host_refs, mut redactions, redacted_count) =
        filter_host_refs_for_transcript_view(&turn_id, &report.affected_host_refs, view);
    report.affected_host_refs = host_refs;
    report.redacted_host_refs = report.redacted_host_refs.saturating_add(redacted_count);
    report.host_ref_redactions.append(&mut redactions);
}

fn facet_doc_references_transcript_source(
    facet_doc: &serde_json::Value,
    key: &ConversationKey,
    turn_id: Option<&str>,
) -> bool {
    let conversation_ref = format!(
        "transcript:{}/{}/{}",
        key.memory_space_id, key.channel_id, key.conversation_id
    );
    facet_doc
        .get("canonical_evidence_refs")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|evidence| evidence.get("source_ref"))
        .filter_map(serde_json::Value::as_str)
        .any(|source_ref| {
            let source_ref = source_ref.trim();
            if !source_ref.starts_with(&conversation_ref) {
                return false;
            }
            match turn_id {
                Some(turn_id) => source_ref.contains(&format!("#turn={}", turn_id.trim())),
                None => true,
            }
        })
}

fn apply_transcript_repair_budget(
    report: &mut CoreTranscriptRepairReport,
    budget: TranscriptGovernanceBudget,
) {
    let issue_limit = budget.repair_issues_per_report.max(1);
    if report.issues.len() > issue_limit {
        report.issues.truncate(issue_limit);
        report.profile_budget_applied = true;
    }
}

fn transcript_redaction_reasons(
    redactions: &[TranscriptRedactionReportItem],
) -> Vec<TranscriptRedactionReason> {
    let mut reasons = Vec::new();
    for redaction in redactions {
        if !reasons.contains(&redaction.reason) {
            reasons.push(redaction.reason);
        }
    }
    reasons
}

struct TranscriptBackedSessionStore {
    fallback: Arc<dyn SessionStore>,
    transcript_store: Arc<dyn ConversationTranscriptStore>,
    key: ConversationKey,
    view: TranscriptReplayView,
}

impl TranscriptBackedSessionStore {
    fn load_transcript_messages(&self, limit: usize) -> Result<Option<Vec<SessionMessage>>> {
        if limit == 0 {
            return Ok(Some(Vec::new()));
        }
        let slice = self
            .transcript_store
            .redacted_replay(&self.key, limit, self.view)?;
        if slice.audit.source_turns == 0 {
            return Ok(None);
        }
        let mut messages = redacted_transcript_slice_to_session_messages(&slice);
        if messages.len() > limit {
            let keep_from = messages.len().saturating_sub(limit);
            messages = messages.split_off(keep_from);
        }
        Ok(Some(messages))
    }

    fn load_transcript_records(&self, limit: usize) -> Result<Option<Vec<SessionMessageRecord>>> {
        if limit == 0 {
            return Ok(Some(Vec::new()));
        }
        let slice = self
            .transcript_store
            .redacted_replay(&self.key, limit, self.view)?;
        if slice.audit.source_turns == 0 {
            return Ok(None);
        }
        let mut records = redacted_transcript_slice_to_session_records(&slice, &self.key);
        if records.len() > limit {
            let keep_from = records.len().saturating_sub(limit);
            records = records.split_off(keep_from);
        }
        Ok(Some(records))
    }
}

struct TranscriptKeyUnavailableSessionStore {
    fallback: Arc<dyn SessionStore>,
    reason: String,
}

impl TranscriptKeyUnavailableSessionStore {
    fn unavailable<T>(&self) -> Result<T> {
        Err(Error::config(
            "conversation_transcript_key_unavailable",
            self.reason.clone(),
        ))
    }
}

impl SessionStore for TranscriptKeyUnavailableSessionStore {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        self.fallback.append(chat_id, role, content)
    }

    fn append_batch(&self, chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
        self.fallback.append_batch(chat_id, messages)
    }

    fn load_recent(&self, _chat_id: &str, _n: usize) -> Result<Vec<SessionMessage>> {
        self.unavailable()
    }

    fn load_recent_records(&self, _chat_id: &str, _n: usize) -> Result<Vec<SessionMessageRecord>> {
        self.unavailable()
    }

    fn message_count(&self, _chat_id: &str) -> Result<usize> {
        self.unavailable()
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.fallback.clear(chat_id)
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        self.unavailable()
    }

    fn delete(&self, chat_id: &str) -> Result<()> {
        self.fallback.delete(chat_id)
    }
}

impl SessionStore for TranscriptBackedSessionStore {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        self.fallback.append(chat_id, role, content)
    }

    fn append_batch(&self, chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
        self.fallback.append_batch(chat_id, messages)
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        match self.load_transcript_messages(n)? {
            Some(messages) => Ok(messages),
            None => self.fallback.load_recent(chat_id, n),
        }
    }

    fn load_recent_records(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessageRecord>> {
        match self.load_transcript_records(n)? {
            Some(records) => Ok(records),
            None => self.fallback.load_recent_records(chat_id, n),
        }
    }

    fn message_count(&self, chat_id: &str) -> Result<usize> {
        match self.load_transcript_messages(usize::MAX)? {
            Some(messages) => Ok(messages.len()),
            None => self.fallback.message_count(chat_id),
        }
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.fallback.clear(chat_id)
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        self.fallback.list_chat_ids()
    }

    fn delete(&self, chat_id: &str) -> Result<()> {
        self.fallback.delete(chat_id)
    }
}

fn redacted_transcript_slice_to_session_messages(
    slice: &bm_core::memory::RedactedTranscriptSlice,
) -> Vec<SessionMessage> {
    let mut messages = Vec::new();
    for turn in &slice.turns {
        for message in &turn.input_messages {
            if let Some(content) = message.content.as_deref() {
                messages.push(session_message_from_transcript_message(message, content));
            }
        }
        if let Some(message) = turn.assistant_message.as_ref() {
            if let Some(content) = message.content.as_deref() {
                messages.push(session_message_from_transcript_message(message, content));
            }
        }
    }
    messages
}

fn redacted_transcript_slice_to_session_records(
    slice: &bm_core::memory::RedactedTranscriptSlice,
    key: &ConversationKey,
) -> Vec<SessionMessageRecord> {
    let mut records = Vec::new();
    for turn in &slice.turns {
        for message in &turn.input_messages {
            if let Some(content) = message.content.as_deref() {
                records.push(session_record_from_transcript_message(
                    key,
                    &turn.turn_id,
                    message,
                    content,
                ));
            }
        }
        if let Some(message) = turn.assistant_message.as_ref() {
            if let Some(content) = message.content.as_deref() {
                records.push(session_record_from_transcript_message(
                    key,
                    &turn.turn_id,
                    message,
                    content,
                ));
            }
        }
    }
    records
}

fn session_message_from_transcript_message(
    message: &bm_core::memory::RedactedTranscriptMessage,
    content: &str,
) -> SessionMessage {
    SessionMessage::new(
        message.message_id.clone(),
        message.role.clone(),
        content.to_string(),
        message.observed_at,
        message.observed_at,
        message.actor.speaker_id.clone(),
        message.actor.speaker_kind.clone(),
    )
}

fn session_record_from_transcript_message(
    key: &ConversationKey,
    turn_id: &str,
    message: &bm_core::memory::RedactedTranscriptMessage,
    content: &str,
) -> SessionMessageRecord {
    let mut record =
        SessionMessageRecord::from(session_message_from_transcript_message(message, content));
    record.transcript_ref = Some(TranscriptEvidenceRef {
        memory_space_id: key.memory_space_id.clone(),
        channel_id: key.channel_id.clone(),
        conversation_id: key.conversation_id.clone(),
        turn_id: turn_id.to_string(),
        message_id: Some(message.message_id.clone()),
        subject_id: None,
        authority: Some(message.authority),
    });
    record
}

fn candidate_semantically_accepted(
    candidate: &MemoryWriteCandidate,
    plane_reports: &[MemoryPlaneGovernanceReport],
) -> bool {
    let candidate_id = candidate.candidate_id.trim();
    !candidate_id.is_empty()
        && plane_reports.iter().any(|report| {
            report.decision == GovernedWriteDecision::Accepted
                && report
                    .evidence_refs
                    .iter()
                    .any(|evidence_ref| evidence_ref == candidate_id)
        })
}

fn long_term_extraction_derived_plane(draft: &LongTermMemoryDraft) -> DerivedMemoryPlane {
    if draft.kind == LongTermMemoryKind::Fact
        || matches!(draft.source_scope, Some(LongTermMemorySourceScope::World))
    {
        DerivedMemoryPlane::SharedFact
    } else {
        DerivedMemoryPlane::LongTerm
    }
}

fn plan_private_garden_derived_memory_ref_mutations(
    memory_space_id: &str,
    subject_id: &str,
    turn: &CanonicalTurnDelta,
    manifest: &[PrivateGardenGovernanceManifestEntry],
    now_secs: u64,
) -> Result<Vec<StoreMutation>> {
    if manifest.is_empty() {
        return Ok(Vec::new());
    }
    let conversation_id = turn
        .conversation
        .conversation_id
        .clone()
        .unwrap_or_else(|| turn.conversation.chat_id.clone());
    let source = TranscriptEvidenceRef {
        memory_space_id: memory_space_id.to_string(),
        channel_id: turn.conversation.channel.clone(),
        conversation_id,
        turn_id: turn.turn_id.clone(),
        message_id: None,
        subject_id: Some(subject_id.to_string()),
        authority: Some(MemoryEvidenceAuthority::PrivateGardenInternal),
    };
    manifest
        .iter()
        .map(|entry| {
            candidate_derived_memory_ref_mutation(
                DerivedMemoryPlane::PrivateGarden,
                &entry.store_key,
                subject_id,
                source.clone(),
                now_secs,
            )
        })
        .collect()
}

fn plan_long_term_extraction_derived_memory_ref_mutations(
    subject_id: &str,
    accepted_upserts: &[LongTermMemoryDraft],
    accepted_skill_writes: &[RuntimeSkillWrite],
    now_secs: u64,
) -> Result<Vec<StoreMutation>> {
    let mut mutations = Vec::new();
    for draft in accepted_upserts {
        let Some(stable_id) = draft.stable_id() else {
            continue;
        };
        let plane = long_term_extraction_derived_plane(draft);
        let store_key = match plane {
            DerivedMemoryPlane::SharedFact => format!("shared_fact:{stable_id}"),
            _ => format!("long_term:{stable_id}"),
        };
        for source in transcript_evidence_refs_from_display_citations(
            &draft.supporting_citations,
            subject_id,
            None,
        ) {
            mutations.push(candidate_derived_memory_ref_mutation(
                plane, &store_key, subject_id, source, now_secs,
            )?);
        }
    }
    for write in accepted_skill_writes {
        let name = write.name.trim();
        if name.is_empty() {
            continue;
        }
        let store_key = format!("runtime_skill:{name}");
        for source in
            transcript_evidence_refs_from_display_citations(&write.citations, subject_id, None)
        {
            mutations.push(candidate_derived_memory_ref_mutation(
                DerivedMemoryPlane::ProceduralSkill,
                &store_key,
                subject_id,
                source,
                now_secs,
            )?);
        }
    }
    Ok(mutations)
}

fn plan_candidate_derived_memory_ref_mutations(
    subject_id: &str,
    accepted_draft_pairs: &[(&MemoryWriteCandidate, LongTermMemoryDraft)],
    accepted_skill_pairs: &[(&MemoryWriteCandidate, RuntimeSkillWrite)],
    now_secs: u64,
) -> Result<Vec<StoreMutation>> {
    let mut mutations = Vec::new();
    for (candidate, draft) in accepted_draft_pairs {
        let Some(stable_id) = draft.stable_id() else {
            continue;
        };
        let target = candidate.governed_target().unwrap_or(&candidate.target);
        let plane = if target.domain() == MemoryWriteDomain::Program {
            DerivedMemoryPlane::SharedFact
        } else {
            DerivedMemoryPlane::LongTerm
        };
        let store_key = match plane {
            DerivedMemoryPlane::SharedFact => format!("shared_fact:{stable_id}"),
            _ => format!("long_term:{stable_id}"),
        };
        for source in candidate_transcript_evidence_refs(candidate, subject_id) {
            mutations.push(candidate_derived_memory_ref_mutation(
                plane, &store_key, subject_id, source, now_secs,
            )?);
        }
    }
    for (candidate, write) in accepted_skill_pairs {
        let name = write.name.trim();
        if name.is_empty() {
            continue;
        }
        let store_key = format!("runtime_skill:{name}");
        for source in candidate_transcript_evidence_refs(candidate, subject_id) {
            mutations.push(candidate_derived_memory_ref_mutation(
                DerivedMemoryPlane::ProceduralSkill,
                &store_key,
                subject_id,
                source,
                now_secs,
            )?);
        }
    }
    Ok(mutations)
}

fn plan_soul_handoff_derived_memory_ref_mutations(
    subject_id: &str,
    candidates: &[MemoryWriteCandidate],
    now_secs: u64,
) -> Result<Vec<StoreMutation>> {
    let mut mutations = Vec::new();
    for candidate in candidates {
        let target = candidate.governed_target().unwrap_or(&candidate.target);
        let surface = match target {
            MemoryCandidateTarget::Soul { surface } => surface.trim(),
            _ => continue,
        };
        if surface.is_empty() {
            continue;
        }
        let candidate_id = candidate.candidate_id.trim();
        let store_key = if candidate_id.is_empty() {
            format!("soul_handoff:{surface}")
        } else {
            format!("soul_handoff:{surface}:{candidate_id}")
        };
        for source in candidate_transcript_evidence_refs(candidate, subject_id) {
            mutations.push(candidate_derived_memory_ref_mutation(
                DerivedMemoryPlane::SoulCandidateHandoff,
                &store_key,
                subject_id,
                source,
                now_secs,
            )?);
        }
    }
    Ok(mutations)
}

fn candidate_derived_memory_ref_mutation(
    plane: DerivedMemoryPlane,
    store_key: &str,
    subject_id: &str,
    source: TranscriptEvidenceRef,
    now_secs: u64,
) -> Result<StoreMutation> {
    let key = ConversationKey::new(
        source.memory_space_id.clone(),
        source.channel_id.clone(),
        source.conversation_id.clone(),
    )?;
    let derived = DerivedMemoryRef {
        plane,
        store_key: store_key.to_string(),
        subject_id: Some(subject_id.to_string()),
        source,
        created_at: now_secs,
    };
    validate_planned_derived_ref_matches_key(&key, &derived)?;
    let record_key = transcript_derived_ref_storage_key(&key, &derived)?;
    Ok(StoreMutation::PutJson {
        namespace: "conversation_transcript_derived_ref".to_string(),
        key: record_key.clone(),
        value: serde_json::to_value(&derived).map_err(|error| {
            Error::config("conversation_transcript_derived_ref", error.to_string())
        })?,
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: "conversation_transcript_derived_ref".to_string(),
        record_key,
    })
}

fn validate_planned_derived_ref_matches_key(
    key: &ConversationKey,
    derived: &DerivedMemoryRef,
) -> Result<()> {
    if derived.source.memory_space_id != key.memory_space_id
        || derived.source.channel_id != key.channel_id
        || derived.source.conversation_id != key.conversation_id
    {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref source does not match conversation key",
        ));
    }
    if derived.source.turn_id.trim().is_empty() {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref turn_id must not be empty",
        ));
    }
    if derived.store_key.trim().is_empty() {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref store_key must not be empty",
        ));
    }
    Ok(())
}

fn transcript_derived_ref_storage_key(
    key: &ConversationKey,
    derived: &DerivedMemoryRef,
) -> Result<String> {
    let payload = serde_json::to_string(derived)
        .map_err(|error| Error::config("conversation_transcript_derived_ref", error.to_string()))?;
    Ok(format!(
        "{}__derived_ref__{}",
        key.storage_key(),
        stable_hash_hex(&payload)
    ))
}

fn memory_write_transaction_report(
    operation: &str,
    planned_mutations: usize,
    changed_count: usize,
    report: StoreMutationBatchReport,
) -> MemoryWriteTransactionReport {
    MemoryWriteTransactionReport {
        transaction_id: report.transaction_id,
        operation: operation.to_string(),
        planned_mutations,
        committed_mutations: report.mutations,
        event_ids: report.event_ids,
        budget_report: report.budget_report,
        changed_count,
        partial_write: false,
    }
}

struct MemoryLongTermExtractionTransactionPlan {
    changed: usize,
    shared_fact_governance: Option<SharedMemoryWriteOutcome>,
    procedural_evolution: Option<SkillEvolutionReport>,
    mutations: Vec<StoreMutation>,
}

struct PlanningLongTermMemoryStore<'a> {
    base: &'a dyn LongTermMemoryStore,
    changes: Mutex<BTreeMap<String, Option<LongTermMemoryEntry>>>,
}

impl<'a> PlanningLongTermMemoryStore<'a> {
    fn new(base: &'a dyn LongTermMemoryStore) -> Self {
        Self {
            base,
            changes: Mutex::new(BTreeMap::new()),
        }
    }

    fn into_mutations(self) -> Result<Vec<StoreMutation>> {
        let changes = self
            .changes
            .into_inner()
            .map_err(|_| Error::config("long_term_control_plan", "planning store poisoned"))?;
        changes
            .into_iter()
            .map(|(id, value)| match value {
                Some(entry) => Ok(StoreMutation::PutJson {
                    namespace: "long_term".to_string(),
                    key: id.clone(),
                    value: serde_json::to_value(&entry).map_err(|error| {
                        Error::config("long_term_control_plan", error.to_string())
                    })?,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "long_term".to_string(),
                    record_key: id,
                }),
                None => Ok(StoreMutation::DeleteJson {
                    namespace: "long_term".to_string(),
                    key: id.clone(),
                    event_kind: MemoryStoreEventKind::MemoryDelete,
                    plane: "long_term".to_string(),
                    record_key: id,
                }),
            })
            .collect()
    }
}

impl LongTermMemoryStore for PlanningLongTermMemoryStore<'_> {
    fn upsert_many(&self, drafts: &[LongTermMemoryDraft], now_secs: u64) -> Result<usize> {
        let mut changed = 0usize;
        for draft in drafts {
            let Some(normalized) = draft.normalized() else {
                continue;
            };
            let Some(id) = normalized.stable_id() else {
                continue;
            };
            let prior = LongTermMemoryStore::get(self, &id)?;
            let observed_at = normalized.observed_at.unwrap_or(now_secs);
            let last_confirmed_at = normalized
                .last_confirmed_at
                .unwrap_or(observed_at)
                .max(observed_at);
            let entry = LongTermMemoryEntry {
                id: id.clone(),
                kind: normalized.kind,
                topic: normalized.topic,
                content: normalized.content,
                keywords: normalized.keywords,
                source_chat_id: normalized.source_chat_id,
                source_type: normalized.source_type.unwrap_or_default(),
                source_scope: normalized.source_scope.unwrap_or_default(),
                confidence: normalized.confidence.unwrap_or_default(),
                freshness: normalized.freshness.unwrap_or_default(),
                stale_hint: normalized.stale_hint.unwrap_or_default(),
                supporting_citations: normalized.supporting_citations,
                evidence_count: normalized.evidence_count.unwrap_or(0),
                created_at: prior
                    .as_ref()
                    .map(|entry| entry.created_at)
                    .unwrap_or(now_secs),
                updated_at: now_secs,
                observed_at,
                last_confirmed_at,
                source_revision: normalized.source_revision.unwrap_or(0),
                last_used_at: prior.as_ref().map(|entry| entry.last_used_at).unwrap_or(0),
            };
            self.changes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(id, Some(entry));
            changed = changed.saturating_add(1);
        }
        Ok(changed)
    }

    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = LongTermMemoryStore::list(self, usize::MAX)?;
        let query = query.trim().to_lowercase();
        if !query.is_empty() {
            entries.retain(|entry| {
                entry.topic.to_lowercase().contains(&query)
                    || entry.content.to_lowercase().contains(&query)
                    || entry
                        .keywords
                        .iter()
                        .any(|keyword| keyword.to_lowercase().contains(&query))
            });
        }
        entries.sort_by(|left, right| {
            let left_scope = usize::from(left.source_chat_id.as_deref() == source_chat_id);
            let right_scope = usize::from(right.source_chat_id.as_deref() == source_chat_id);
            right_scope
                .cmp(&left_scope)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
        });
        if limit > 0 {
            entries.truncate(limit);
        }
        Ok(entries)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        if let Some(value) = self
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(id)
            .cloned()
        {
            return Ok(value);
        }
        self.base.get(id)
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let mut map = self
            .base
            .list(usize::MAX)?
            .into_iter()
            .map(|entry| (entry.id.clone(), entry))
            .collect::<BTreeMap<_, _>>();
        for (id, value) in self
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
        {
            match value {
                Some(entry) => {
                    map.insert(id.clone(), entry.clone());
                }
                None => {
                    map.remove(id);
                }
            }
        }
        let mut entries = map.into_values().collect::<Vec<_>>();
        entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        if limit > 0 {
            entries.truncate(limit);
        }
        Ok(entries)
    }

    fn delete(&self, id: &str) -> Result<bool> {
        if LongTermMemoryStore::get(self, id)?.is_some() {
            self.changes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(id.to_string(), None);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn delete_slot(&self, slot: &bm_core::memory::LongTermMemorySlot) -> Result<bool> {
        let Some(id) = slot.stable_id() else {
            return Ok(false);
        };
        LongTermMemoryStore::delete(self, &id)
    }

    fn count(&self) -> Result<usize> {
        Ok(LongTermMemoryStore::list(self, usize::MAX)?.len())
    }
}

struct PlanningSkillStorage<'a> {
    base: &'a dyn SkillStorage,
    changes: Mutex<BTreeMap<String, Option<Vec<u8>>>>,
}

impl<'a> PlanningSkillStorage<'a> {
    fn new(base: &'a dyn SkillStorage) -> Self {
        Self {
            base,
            changes: Mutex::new(BTreeMap::new()),
        }
    }

    fn into_mutations(self) -> Result<Vec<RuntimeSkillStorageMutation>> {
        let changes = self.changes.into_inner().map_err(|_| {
            Error::config("skill_transaction_plan", "planning skill store poisoned")
        })?;
        Ok(changes
            .into_iter()
            .map(|(name, value)| match value {
                Some(content) => RuntimeSkillStorageMutation::Upsert { name, content },
                None => RuntimeSkillStorageMutation::Delete { name },
            })
            .collect())
    }
}

impl SkillStorage for PlanningSkillStorage<'_> {
    fn list_names(&self) -> Result<Vec<String>> {
        let mut names = self
            .base
            .list_names()?
            .into_iter()
            .map(|name| (name, ()))
            .collect::<BTreeMap<_, ()>>();
        for (name, value) in self
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
        {
            if value.is_some() {
                names.insert(name.clone(), ());
            } else {
                names.remove(name);
            }
        }
        Ok(names.into_keys().collect())
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        if let Some(value) = self
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(name)
        {
            return Ok(value.clone().unwrap_or_default());
        }
        self.base.read(name)
    }

    fn write(&self, name: &str, content: &[u8]) -> Result<()> {
        self.changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(name.to_string(), Some(content.to_vec()));
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<()> {
        self.changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(name.to_string(), None);
        Ok(())
    }
}

struct PlanningPrivateGardenStore<'a> {
    base: &'a dyn PrivateGardenStore,
    changes: Mutex<BTreeMap<String, Option<PrivateGardenDoc>>>,
}

impl<'a> PlanningPrivateGardenStore<'a> {
    fn new(base: &'a dyn PrivateGardenStore) -> Self {
        Self {
            base,
            changes: Mutex::new(BTreeMap::new()),
        }
    }

    fn into_mutations(self) -> Result<Vec<StoreMutation>> {
        let changes = self
            .changes
            .into_inner()
            .map_err(|_| Error::config("private_garden_plan", "planning store poisoned"))?;
        changes
            .into_iter()
            .map(|(key, value)| match value {
                Some(doc) => Ok(StoreMutation::PutJson {
                    namespace: "private_garden".to_string(),
                    key: key.clone(),
                    value: serde_json::to_value(&doc)
                        .map_err(|error| Error::config("private_garden_plan", error.to_string()))?,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "private_garden".to_string(),
                    record_key: key,
                }),
                None => Ok(StoreMutation::DeleteJson {
                    namespace: "private_garden".to_string(),
                    key: key.clone(),
                    event_kind: MemoryStoreEventKind::MemoryDelete,
                    plane: "private_garden".to_string(),
                    record_key: key,
                }),
            })
            .collect()
    }
}

impl PrivateGardenStore for PlanningPrivateGardenStore<'_> {
    fn list(&self, chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
        let mut records = self
            .base
            .list(chat_id, usize::MAX)?
            .into_iter()
            .map(|record| (record.path.clone(), record))
            .collect::<BTreeMap<_, _>>();
        let prefix = private_garden_storage_key_prefix(chat_id);
        for (key, value) in self
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix))
        {
            let path = key.trim_start_matches(&prefix).to_string();
            match value {
                Some(doc) => {
                    records.insert(path, private_garden_doc_record(doc));
                }
                None => {
                    records.remove(&path);
                }
            }
        }
        let mut rows = records.into_values().collect::<Vec<_>>();
        rows.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        if limit > 0 {
            rows.truncate(limit);
        }
        Ok(rows)
    }

    fn read(&self, chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
        let path = normalize_private_garden_doc_path(doc_path)?;
        let key = private_garden_storage_key(chat_id, &path);
        if let Some(value) = self
            .changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&key)
            .cloned()
        {
            return Ok(value);
        }
        self.base.read(chat_id, &path)
    }

    fn write(
        &self,
        chat_id: &str,
        doc_path: &str,
        content: &str,
        now_secs: u64,
    ) -> Result<PrivateGardenDocRecord> {
        if content.len() > PRIVATE_GARDEN_MAX_DOC_BYTES {
            return Err(Error::config(
                "private_garden_write",
                format!(
                    "document size {} exceeds {}",
                    content.len(),
                    PRIVATE_GARDEN_MAX_DOC_BYTES
                ),
            ));
        }
        let path = normalize_private_garden_doc_path(doc_path)?;
        let revision = PrivateGardenStore::read(self, chat_id, &path)?
            .map(|doc| doc.revision.saturating_add(1))
            .unwrap_or(1);
        let doc = PrivateGardenDoc {
            path: path.clone(),
            content: content.to_string(),
            updated_at: now_secs,
            revision,
        };
        self.changes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                private_garden_storage_key(chat_id, &path),
                Some(doc.clone()),
            );
        Ok(private_garden_doc_record(&doc))
    }

    fn move_doc(
        &self,
        chat_id: &str,
        from_path: &str,
        to_path: &str,
        now_secs: u64,
    ) -> Result<Option<PrivateGardenDocRecord>> {
        let from = normalize_private_garden_doc_path(from_path)?;
        let to = normalize_private_garden_doc_path(to_path)?;
        let Some(doc) = PrivateGardenStore::read(self, chat_id, &from)? else {
            return Ok(None);
        };
        PrivateGardenStore::delete(self, chat_id, &from)?;
        Ok(Some(PrivateGardenStore::write(
            self,
            chat_id,
            &to,
            &doc.content,
            now_secs,
        )?))
    }

    fn delete(&self, chat_id: &str, doc_path: &str) -> Result<bool> {
        let path = normalize_private_garden_doc_path(doc_path)?;
        if PrivateGardenStore::read(self, chat_id, &path)?.is_some() {
            self.changes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(private_garden_storage_key(chat_id, &path), None);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn private_garden_storage_key_prefix(chat_id: &str) -> String {
    format!("{chat_id}::")
}

fn private_garden_storage_key(chat_id: &str, doc_path: &str) -> String {
    format!("{}{doc_path}", private_garden_storage_key_prefix(chat_id))
}

fn private_garden_doc_record(doc: &PrivateGardenDoc) -> PrivateGardenDocRecord {
    PrivateGardenDocRecord {
        path: doc.path.clone(),
        updated_at: doc.updated_at,
        revision: doc.revision,
        bytes: doc.content.len(),
        preview: doc.content.chars().take(160).collect(),
    }
}

struct PlanningLongTermControlStore<'a> {
    base: &'a dyn LongTermMemoryControlStore,
    revisions: Mutex<BTreeMap<String, LongTermMemoryControlRevision>>,
    tombstones: Mutex<BTreeMap<String, LongTermMemoryTombstone>>,
    policies: Mutex<BTreeMap<String, Option<MemoryLongTermGovernancePolicy>>>,
    audits: Mutex<BTreeMap<String, LongTermMemoryControlAuditEvent>>,
}

impl<'a> PlanningLongTermControlStore<'a> {
    fn new(base: &'a dyn LongTermMemoryControlStore) -> Self {
        Self {
            base,
            revisions: Mutex::new(BTreeMap::new()),
            tombstones: Mutex::new(BTreeMap::new()),
            policies: Mutex::new(BTreeMap::new()),
            audits: Mutex::new(BTreeMap::new()),
        }
    }

    fn into_mutations(self) -> Result<Vec<StoreMutation>> {
        let mut mutations = Vec::new();
        for (id, revision) in self
            .revisions
            .into_inner()
            .map_err(|_| Error::config("long_term_control_plan", "revision plan poisoned"))?
        {
            mutations.push(planned_json_put(
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                id,
                &revision,
                MemoryStoreEventKind::MemoryControl,
            )?);
        }
        for (record_id, tombstone) in self
            .tombstones
            .into_inner()
            .map_err(|_| Error::config("long_term_control_plan", "tombstone plan poisoned"))?
        {
            mutations.push(planned_json_put(
                LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
                record_id,
                &tombstone,
                MemoryStoreEventKind::MemoryDelete,
            )?);
        }
        for (policy_id, value) in self
            .policies
            .into_inner()
            .map_err(|_| Error::config("long_term_control_plan", "policy plan poisoned"))?
        {
            match value {
                Some(policy) => mutations.push(planned_json_put(
                    LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
                    policy_id,
                    &policy,
                    MemoryStoreEventKind::MemoryPolicy,
                )?),
                None => mutations.push(StoreMutation::DeleteJson {
                    namespace: LONG_TERM_GOVERNANCE_POLICY_NAMESPACE.to_string(),
                    key: policy_id.clone(),
                    event_kind: MemoryStoreEventKind::MemoryPolicy,
                    plane: LONG_TERM_GOVERNANCE_POLICY_NAMESPACE.to_string(),
                    record_key: policy_id,
                }),
            }
        }
        for (id, audit) in self
            .audits
            .into_inner()
            .map_err(|_| Error::config("long_term_control_plan", "audit plan poisoned"))?
        {
            mutations.push(planned_json_put(
                LONG_TERM_CONTROL_AUDIT_NAMESPACE,
                id,
                &audit,
                MemoryStoreEventKind::MemoryControl,
            )?);
        }
        Ok(mutations)
    }
}

impl LongTermMemoryControlStore for PlanningLongTermControlStore<'_> {
    fn put_long_term_control_revision(
        &self,
        revision: &LongTermMemoryControlRevision,
    ) -> Result<()> {
        self.revisions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(revision.revision_id.clone(), revision.clone());
        Ok(())
    }

    fn list_long_term_control_revisions(
        &self,
        record_id: &str,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlRevision>> {
        let mut revisions = self
            .base
            .list_long_term_control_revisions(record_id, usize::MAX)?;
        revisions.extend(
            self.revisions
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .filter(|revision| revision.record_id == record_id)
                .cloned(),
        );
        revisions.sort_by(|left, right| {
            right
                .revision
                .cmp(&left.revision)
                .then_with(|| right.created_at.cmp(&left.created_at))
        });
        if limit > 0 {
            revisions.truncate(limit);
        }
        Ok(revisions)
    }

    fn put_long_term_control_tombstone(&self, tombstone: &LongTermMemoryTombstone) -> Result<()> {
        self.tombstones
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(tombstone.record_id.clone(), tombstone.clone());
        Ok(())
    }

    fn get_long_term_control_tombstone(
        &self,
        record_id: &str,
    ) -> Result<Option<LongTermMemoryTombstone>> {
        if let Some(tombstone) = self
            .tombstones
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(record_id)
            .cloned()
        {
            return Ok(Some(tombstone));
        }
        self.base.get_long_term_control_tombstone(record_id)
    }

    fn list_long_term_control_tombstones(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryTombstone>> {
        let mut tombstones = self.base.list_long_term_control_tombstones(usize::MAX)?;
        tombstones.extend(
            self.tombstones
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .cloned(),
        );
        tombstones.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        tombstones.dedup_by(|left, right| left.record_id == right.record_id);
        if limit > 0 {
            tombstones.truncate(limit);
        }
        Ok(tombstones)
    }

    fn put_long_term_governance_policy(
        &self,
        policy: &MemoryLongTermGovernancePolicy,
    ) -> Result<()> {
        self.policies
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(policy.policy_id.clone(), Some(policy.clone()));
        Ok(())
    }

    fn delete_long_term_governance_policy(&self, policy_id: &str) -> Result<bool> {
        let existed = self
            .list_long_term_governance_policies(usize::MAX)?
            .iter()
            .any(|policy| policy.policy_id == policy_id);
        if existed {
            self.policies
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(policy_id.to_string(), None);
        }
        Ok(existed)
    }

    fn list_long_term_governance_policies(
        &self,
        limit: usize,
    ) -> Result<Vec<MemoryLongTermGovernancePolicy>> {
        let mut map = self
            .base
            .list_long_term_governance_policies(usize::MAX)?
            .into_iter()
            .map(|policy| (policy.policy_id.clone(), policy))
            .collect::<BTreeMap<_, _>>();
        for (policy_id, value) in self
            .policies
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
        {
            match value {
                Some(policy) => {
                    map.insert(policy_id.clone(), policy.clone());
                }
                None => {
                    map.remove(policy_id);
                }
            }
        }
        let mut policies = map.into_values().collect::<Vec<_>>();
        policies.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        if limit > 0 {
            policies.truncate(limit);
        }
        Ok(policies)
    }

    fn put_long_term_control_audit(&self, event: &LongTermMemoryControlAuditEvent) -> Result<()> {
        self.audits
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(event.event_id.clone(), event.clone());
        Ok(())
    }

    fn list_long_term_control_audit(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlAuditEvent>> {
        let mut events = self.base.list_long_term_control_audit(usize::MAX)?;
        events.extend(
            self.audits
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .cloned(),
        );
        events.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        if limit > 0 {
            events.truncate(limit);
        }
        Ok(events)
    }
}

fn planned_json_put<T: serde::Serialize>(
    namespace: &str,
    key: String,
    value: &T,
    event_kind: MemoryStoreEventKind,
) -> Result<StoreMutation> {
    Ok(StoreMutation::PutJson {
        namespace: namespace.to_string(),
        key: key.clone(),
        value: serde_json::to_value(value)
            .map_err(|error| Error::config("long_term_control_plan", error.to_string()))?,
        event_kind,
        plane: namespace.to_string(),
        record_key: key,
    })
}

fn long_term_extraction_state_mutation(
    chat_id: &str,
    previous: Option<&LongTermMemoryExtractionState>,
    next: &LongTermMemoryExtractionState,
) -> Result<Option<StoreMutation>> {
    if previous == Some(next) {
        return Ok(None);
    }
    if next == &LongTermMemoryExtractionState::default() {
        return Ok(Some(StoreMutation::DeleteJson {
            namespace: "long_term_extraction_state".to_string(),
            key: chat_id.to_string(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: "long_term_extraction_state".to_string(),
            record_key: chat_id.to_string(),
        }));
    }
    Ok(Some(StoreMutation::PutJson {
        namespace: "long_term_extraction_state".to_string(),
        key: chat_id.to_string(),
        value: serde_json::to_value(next)
            .map_err(|error| Error::config("long_term_extraction_state", error.to_string()))?,
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: "long_term_extraction_state".to_string(),
        record_key: chat_id.to_string(),
    }))
}

fn runtime_skill_storage_mutations_to_store_mutations(
    mutations: &[RuntimeSkillStorageMutation],
) -> Vec<StoreMutation> {
    mutations
        .iter()
        .map(|mutation| match mutation {
            RuntimeSkillStorageMutation::Upsert { name, content } => StoreMutation::PutBlob {
                namespace: "skills".to_string(),
                key: name.clone(),
                value: content.clone(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skills".to_string(),
                record_key: name.clone(),
            },
            RuntimeSkillStorageMutation::Delete { name } => StoreMutation::DeleteBlob {
                namespace: "skills".to_string(),
                key: name.clone(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "skills".to_string(),
                record_key: name.clone(),
            },
        })
        .collect()
}

fn stable_hash_json(value: &serde_json::Value) -> Result<String> {
    let encoded = serde_json::to_string(value)
        .map_err(|error| Error::config("stable_hash_json", error.to_string()))?;
    Ok(stable_hash_hex(&encoded))
}

fn stable_hash_hex<T: Hash>(value: &T) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn candidate_transcript_evidence_refs(
    candidate: &MemoryWriteCandidate,
    subject_id: &str,
) -> Vec<TranscriptEvidenceRef> {
    transcript_evidence_refs_from_display_citations(
        &candidate.evidence_refs,
        subject_id,
        Some(candidate.authority),
    )
}

fn transcript_evidence_refs_from_display_citations(
    citations: &[String],
    subject_id: &str,
    authority: Option<MemoryEvidenceAuthority>,
) -> Vec<TranscriptEvidenceRef> {
    let mut sources = Vec::new();
    for value in citations {
        let Some(mut source) = TranscriptEvidenceRef::parse_display_citation(value) else {
            continue;
        };
        if source.subject_id.is_none() {
            source.subject_id = Some(subject_id.to_string());
        }
        if source.authority.is_none() {
            source.authority = authority;
        }
        if !sources.iter().any(|existing| existing == &source) {
            sources.push(source);
        }
    }
    sources
}

fn runtime_skill_summary(record: &RuntimeSkillRecord, enabled: bool) -> RuntimeSkillSummary {
    RuntimeSkillSummary {
        name: record.name.clone(),
        title: record.title.clone(),
        topic: record.topic.clone(),
        status: record.status.label().to_string(),
        enabled,
        quality_score: Some(record.quality_score),
        use_count: record.use_count,
        validated_success_count: record.validated_success_count,
        mismatch_count: record.mismatch_count,
        revision_pending: record.revision_pending,
        updated_at: record.updated_at,
        last_used_at: record.last_used_at,
    }
}

fn render_runtime_skill_detail_content(record: &RuntimeSkillRecord) -> String {
    format!(
        "<!-- beetle:runtime-skill -->\n# {}\n\nType: procedural_runtime_skill\nOrigin: runtime_learned\nTopic: {}\nStatus: {}\n\n## Summary\n{}\n\n## Procedure\n{}\n",
        record.title,
        record.topic,
        record.status.label(),
        record.summary,
        record.procedure
    )
}

fn skill_matches_query(
    summary: &RuntimeSkillSummary,
    summary_text: Option<&str>,
    procedure_text: Option<&str>,
    query: Option<&str>,
) -> bool {
    let Some(query) = query else {
        return true;
    };
    let mut haystack = String::new();
    haystack.push_str(&summary.name);
    haystack.push('\n');
    haystack.push_str(&summary.title);
    haystack.push('\n');
    haystack.push_str(&summary.topic);
    if let Some(value) = summary_text {
        haystack.push('\n');
        haystack.push_str(value);
    }
    if let Some(value) = procedure_text {
        haystack.push('\n');
        haystack.push_str(value);
    }
    haystack.to_ascii_lowercase().contains(query)
}

fn render_runtime_skill_lineage(record: &RuntimeSkillRecord) -> Vec<String> {
    record
        .genome_lineage
        .iter()
        .map(|node| {
            format!(
                "{} | {:?} | {} | {}",
                node.node_id, node.disposition, node.recorded_at, node.summary
            )
        })
        .collect()
}

fn render_runtime_skill_strategy_diffs(record: &RuntimeSkillRecord) -> Vec<String> {
    record
        .strategy_diffs
        .iter()
        .map(|diff| {
            format!(
                "{} -> {} | {:?} | {} | {}",
                diff.from_node_id,
                diff.to_node_id,
                diff.change_kind,
                diff.recorded_at,
                diff.summary
            )
        })
        .collect()
}

fn normalize_runtime_skill_write_names(
    writes: Vec<crate::RuntimeSkillWrite>,
) -> Vec<crate::RuntimeSkillWrite> {
    writes
        .into_iter()
        .map(|mut write| {
            let name = write.name.trim();
            if name.is_empty() || !is_runtime_skill_name(name) {
                write.name =
                    sdk_runtime_skill_name(if name.is_empty() { &write.topic } else { name });
            } else if name != write.name {
                write.name = name.to_string();
            }
            write
        })
        .collect()
}

fn sdk_runtime_skill_name(seed: &str) -> String {
    let mut slug = seed
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while slug.contains("__") {
        slug = slug.replace("__", "_");
    }
    let slug = slug.trim_matches('_');
    let suffix = if slug.is_empty() { "skill" } else { slug };
    format!(
        "runtime_skill__{}",
        suffix.chars().take(40).collect::<String>()
    )
}

pub struct MemoryRuntimeBuilder {
    identity: Option<MemoryIdentity>,
    subject_id: Option<String>,
    subject_registry: Option<SubjectRegistry>,
    subject_relationship_graph: Option<SubjectRelationshipGraph>,
    scoped_runtime: Option<SubjectScopedRuntime>,
    scope: Option<MemoryScope>,
    profile: ProfileId,
    platform: Option<Arc<dyn Platform>>,
    llm: Option<Arc<dyn LlmClient>>,
    clock: Option<Arc<dyn MemoryClock>>,
    capability_policy: MemoryCapabilityPolicy,
    privacy_policy: MemoryPrivacyPolicy,
    audit_sink: Option<Arc<dyn MemoryAuditSink>>,
    runtime_resource_snapshot: Option<RuntimeResourceSnapshot>,
    static_platform_manifest: Option<StaticPlatformManifest>,
    provider_model_context_limit: Option<ProviderModelContextLimit>,
    runtime_budget: Option<RuntimeBudgetReport>,
    store_platform: Option<StorePlatform>,
    agent_skill_dirs: Vec<AgentSkillDirConfig>,
    agent_tool_registries: Vec<AgentToolRegistrySnapshot>,
}

impl Default for MemoryRuntimeBuilder {
    fn default() -> Self {
        Self {
            identity: None,
            subject_id: None,
            subject_registry: None,
            subject_relationship_graph: None,
            scoped_runtime: None,
            scope: None,
            profile: ProfileId::ServerLinuxDevFull,
            platform: None,
            llm: None,
            clock: Some(Arc::new(SystemMemoryClock)),
            capability_policy: MemoryCapabilityPolicy::strict_profile(),
            privacy_policy: MemoryPrivacyPolicy::standard_private_boundary(),
            audit_sink: Some(Arc::new(NoopMemoryAuditSink)),
            runtime_resource_snapshot: None,
            static_platform_manifest: None,
            provider_model_context_limit: None,
            runtime_budget: None,
            store_platform: None,
            agent_skill_dirs: Vec::new(),
            agent_tool_registries: Vec::new(),
        }
    }
}

impl MemoryRuntimeBuilder {
    pub fn identity(mut self, identity: MemoryIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    pub fn subject_id(mut self, subject_id: impl Into<String>) -> Self {
        self.subject_id = Some(subject_id.into());
        self
    }

    pub fn subject_registry(mut self, registry: SubjectRegistry) -> Self {
        self.subject_registry = Some(registry);
        self
    }

    pub fn subject_relationship_graph(mut self, graph: SubjectRelationshipGraph) -> Self {
        self.subject_relationship_graph = Some(graph);
        self
    }

    pub fn scoped_runtime(mut self, scoped_runtime: SubjectScopedRuntime) -> Self {
        self.scoped_runtime = Some(scoped_runtime);
        self
    }

    pub fn scope(mut self, scope: MemoryScope) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn profile(mut self, profile: ProfileId) -> Self {
        self.profile = profile;
        self
    }

    pub fn store_platform(mut self, platform: StorePlatform) -> Self {
        self.store_platform = Some(platform.clone());
        self.platform = Some(platform.into_arc());
        self
    }

    pub fn platform(mut self, platform: Arc<dyn Platform>) -> Self {
        self.platform = Some(platform);
        self.store_platform = None;
        self
    }

    pub fn llm(mut self, llm: Arc<dyn LlmClient>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn clock(mut self, clock: Arc<dyn MemoryClock>) -> Self {
        self.clock = Some(clock);
        self
    }

    pub fn capability_policy(mut self, policy: MemoryCapabilityPolicy) -> Self {
        self.capability_policy = policy;
        self
    }

    pub fn privacy_policy(mut self, policy: MemoryPrivacyPolicy) -> Self {
        self.privacy_policy = policy;
        self
    }

    pub fn audit_sink(mut self, audit_sink: Arc<dyn MemoryAuditSink>) -> Self {
        self.audit_sink = Some(audit_sink);
        self
    }

    pub fn runtime_resource_snapshot(mut self, snapshot: RuntimeResourceSnapshot) -> Self {
        self.runtime_resource_snapshot = Some(snapshot);
        self
    }

    pub fn static_platform_manifest(mut self, manifest: StaticPlatformManifest) -> Self {
        self.static_platform_manifest = Some(manifest);
        self
    }

    pub fn provider_model_context_limit(mut self, limit: ProviderModelContextLimit) -> Self {
        self.provider_model_context_limit = Some(limit);
        self
    }

    pub fn runtime_budget(mut self, report: RuntimeBudgetReport) -> Self {
        self.runtime_budget = Some(report);
        self
    }

    pub fn agent_skill_dirs(mut self, dirs: Vec<AgentSkillDirConfig>) -> Self {
        self.agent_skill_dirs = dirs;
        self
    }

    pub fn add_agent_skill_dir(mut self, dir: AgentSkillDirConfig) -> Self {
        self.agent_skill_dirs.push(dir);
        self
    }

    pub fn agent_tool_registries(mut self, registries: Vec<AgentToolRegistrySnapshot>) -> Self {
        self.agent_tool_registries = registries;
        self
    }

    pub fn agent_tool_registry(mut self, registry: AgentToolRegistrySnapshot) -> Self {
        self.agent_tool_registries.push(registry);
        self
    }

    pub fn build(self) -> Result<MemoryRuntime> {
        let identity = self
            .identity
            .ok_or_else(|| Error::config("memory_runtime_config", "identity must be configured"))?;
        let requested_subject_id = self
            .subject_id
            .unwrap_or_else(|| default_agent_subject_id(&identity.agent_id))
            .trim()
            .to_string();
        if requested_subject_id.is_empty() {
            return Err(Error::config(
                "memory_runtime_config",
                "subject_id must not be empty",
            ));
        }
        let scope = self
            .scope
            .ok_or_else(|| Error::config("memory_runtime_config", "scope must be configured"))?;
        let platform = self
            .platform
            .ok_or_else(|| Error::config("memory_runtime_config", "platform must be configured"))?;
        let memory_space_id = default_memory_space_id(&identity.owner_id);
        let subject_registry = match self.subject_registry {
            Some(registry) => registry,
            None => SubjectRegistry::single_agent_default_with_subject(
                &identity.owner_id,
                &identity.agent_id,
                None,
                &requested_subject_id,
            )
            .map_err(|reason| Error::config("memory_runtime_config", reason))?,
        };
        let validation = subject_registry.validate_contract();
        if !validation.accepted {
            return Err(Error::config("memory_runtime_config", validation.reason));
        }
        if subject_registry.memory_space_id != memory_space_id {
            return Err(Error::config(
                "memory_runtime_config",
                "subject_registry_memory_space_mismatch",
            ));
        }
        let scoped_runtime = match self.scoped_runtime {
            Some(scoped_runtime) => scoped_runtime,
            None => SubjectScopedRuntime {
                memory_space_id: memory_space_id.clone(),
                mounted_subject_id: requested_subject_id.clone(),
                actor_subject_id: requested_subject_id.clone(),
                agent_id: identity.agent_id.clone(),
                relationship_scope: Some(relationship_scope(
                    &scope.channel,
                    &scope.chat_id,
                    Some(scope.conversation_id_or_chat_id().to_string()),
                )),
                projection_policy: "subject_aware_default".to_string(),
                write_policy: "subject_candidate_then_space_governance".to_string(),
            },
        };
        let runtime_validation = scoped_runtime.validate_against_registry(&subject_registry);
        if !runtime_validation.accepted {
            return Err(Error::config(
                "memory_runtime_config",
                runtime_validation.reason,
            ));
        }
        let subject_id = scoped_runtime.mounted_subject_id.trim().to_string();
        if subject_id.is_empty() {
            return Err(Error::config(
                "memory_runtime_config",
                "runtime_mounted_subject_empty",
            ));
        }
        let subject_relationship_graph = match self.subject_relationship_graph {
            Some(graph) => graph,
            None => SubjectRelationshipGraph::single_agent_default_for_subject(
                &subject_registry,
                &subject_id,
            )
            .map_err(|reason| Error::config("memory_runtime_config", reason))?,
        };
        let graph_validation =
            subject_relationship_graph.validate_against_registry(&subject_registry);
        if !graph_validation.accepted {
            return Err(Error::config(
                "memory_runtime_config",
                graph_validation.reason,
            ));
        }
        let clock = self
            .clock
            .ok_or_else(|| Error::config("memory_runtime_config", "clock must be configured"))?;
        let audit_sink = self.audit_sink.ok_or_else(|| {
            Error::config("memory_runtime_config", "audit_sink must be configured")
        })?;
        let capabilities = resolve_memory_capabilities(
            self.profile,
            &self.capability_policy,
            &self.privacy_policy,
        )?;
        let runtime_budget = match self.runtime_budget {
            Some(report) => report,
            None => {
                let now_secs = clock.now_secs();
                let resource_snapshot = match self.runtime_resource_snapshot {
                    Some(snapshot) => snapshot,
                    None => platform.runtime_resource_probe().probe(now_secs)?,
                };
                compile_runtime_budget(RuntimeBudgetInput {
                    profile: self.profile,
                    resource_snapshot,
                    static_platform_manifest: self
                        .static_platform_manifest
                        .unwrap_or_else(|| StaticPlatformManifest::for_profile(self.profile)),
                    provider_model_context_limit: self.provider_model_context_limit,
                })
            }
        };
        let agent_skill_registry = build_agent_skill_registry_snapshot(
            self.profile,
            &self.agent_skill_dirs,
            clock.now_secs(),
        )
        .map_err(|error| Error::config(error.stage(), error.to_string()))?;
        for registry in &self.agent_tool_registries {
            validate_agent_tool_registry_snapshot(self.profile, registry)
                .map_err(|error| Error::config(error.stage(), error.to_string()))?;
        }
        let config = MemoryRuntimeConfig {
            identity,
            memory_space_id,
            subject_id,
            scoped_runtime,
            subject_registry,
            subject_relationship_graph,
            scope,
            profile: self.profile,
            platform,
            store_platform: self.store_platform,
            llm: self.llm,
            clock,
            capability_policy: self.capability_policy,
            privacy_policy: self.privacy_policy,
            audit_sink,
            runtime_budget,
            agent_skill_registry,
        };
        let runtime = MemoryRuntime {
            config,
            capabilities,
            lifecycle: RuntimeLifecycleEngine,
            agent_tool_registries: Mutex::new(self.agent_tool_registries),
            last_conversation_id: Mutex::new(None),
        };
        let lifecycle = runtime.start_lifecycle(
            RuntimeLifecycleOperation::Open,
            RuntimeLifecycleTrigger::SdkCall,
            RuntimeLifecycleModeInput::default(),
        );
        let open_payload = [
            (
                "budget_report_id",
                runtime.config.runtime_budget.report_id.clone(),
            ),
            (
                "resource_source",
                runtime
                    .config
                    .runtime_budget
                    .resource_snapshot
                    .source
                    .as_str()
                    .to_string(),
            ),
            (
                "budget_limited_by",
                runtime.config.runtime_budget.limited_by.join(","),
            ),
        ];
        runtime.finish_lifecycle_success_with_payload(
            lifecycle,
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::Noop,
            false,
            "runtime_opened",
            &open_payload,
        )?;
        Ok(runtime)
    }
}
