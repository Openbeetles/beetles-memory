use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::feature_gate::ProfileId;
use bm_core::llm::{LlmClient as CoreLlmClient, LlmHttpClient};
use bm_core::memory::{
    apply_long_term_memory_extraction, export_continuity_snapshot, import_continuity_snapshot,
    inspect_intelligence_replay, inspect_working_recall, load_prompt_memory_context,
    run_post_reply_memory_maintenance, ContinuitySnapshotExportContext,
    ContinuitySnapshotImportContext, ContinuitySnapshotMode, PostReplyMemoryMaintenanceContext,
    PostReplyMemoryMaintenanceInput, PromptMemoryContextParams, PromptParticipationPlan,
    PromptRecallIntent, WorkingRecallInspectionInput,
};
use bm_core::platform::Platform;
use bm_core::runtime::{
    build_runtime_lifecycle_diagnosis, ensure_platform_soul_kernel_recovery,
    RuntimeLifecycleDisposition, RuntimeLifecycleEffect, RuntimeLifecycleEngine,
    RuntimeLifecycleEvent, RuntimeLifecycleEventKind, RuntimeLifecycleModeInput,
    RuntimeLifecycleOperation, RuntimeLifecycleReport, RuntimeLifecycleTrigger,
};
use bm_core::skills::{
    is_runtime_skill_name, retrieve_runtime_skill_hits, write_governed_runtime_skills,
};
use bm_store::StorePlatform;

use crate::{
    resolve_memory_capabilities, Error, LlmClient, MemoryCapabilityCatalog, MemoryCapabilityPolicy,
    MemoryCloseReport, MemoryCloseRequest, MemoryExportReport, MemoryExportRequest,
    MemoryImportReport, MemoryImportRequest, MemoryInspectionReport, MemoryInspectionRequest,
    MemoryMaintenanceReport, MemoryMaintenanceRequest, MemoryOperationVisibility,
    MemoryPrivacyPolicy, MemoryProfile, MemoryProjectionReport, MemoryProjectionRequest,
    MemoryRecallReport, MemoryRecallRequest, MemoryRecoverReport, MemoryRecoverRequest,
    MemoryReplayReport, MemoryReplayRequest, MemoryRuntimeSystemKind, MemoryWriteReport,
    MemoryWriteRequest, PressureLevel, Result, RuntimeOperatorAction, RuntimeOperatorActionReport,
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
        Ok(Self { channel, chat_id })
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
    pub scope: MemoryScope,
    pub allowed: bool,
    pub reason: String,
}

pub struct NoopMemoryAuditSink;

impl MemoryAuditSink for NoopMemoryAuditSink {
    fn record(&self, _event: MemoryAuditEvent) {}
}

pub struct MemoryRuntimeConfig {
    pub identity: MemoryIdentity,
    pub scope: MemoryScope,
    pub profile: ProfileId,
    pub(crate) platform: Arc<dyn Platform>,
    pub llm: Option<Arc<dyn LlmClient>>,
    pub clock: Arc<dyn MemoryClock>,
    pub capability_policy: MemoryCapabilityPolicy,
    pub privacy_policy: MemoryPrivacyPolicy,
    pub audit_sink: Arc<dyn MemoryAuditSink>,
}

pub struct MemoryRuntime {
    pub(crate) config: MemoryRuntimeConfig,
    pub(crate) capabilities: MemoryCapabilityCatalog,
    lifecycle: RuntimeLifecycleEngine,
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

    pub fn scope(&self) -> &MemoryScope {
        &self.config.scope
    }

    pub fn capabilities(&self) -> &MemoryCapabilityCatalog {
        &self.capabilities
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
                let storage = self.config.platform.skill_storage();
                let writes = normalize_runtime_skill_write_names(writes);
                let outcome = write_governed_runtime_skills(storage.as_ref(), &writes, source)?;
                MemoryWriteReport {
                    accepted: outcome.accepted > 0 || outcome.rejected == 0,
                    changed: outcome.changed,
                    operation: "write.procedural",
                    reason: format!(
                        "submitted={}, accepted={}, rejected={}",
                        outcome.submitted, outcome.accepted, outcome.rejected
                    ),
                    lifecycle_report: self.finish_lifecycle_success(
                        lifecycle,
                        RuntimeLifecycleEventKind::RuntimeLifecycle,
                        RuntimeLifecycleEffect::RunMaintenance,
                        outcome.changed > 0,
                        "write.procedural",
                    )?,
                }
            }
            MemoryWriteRequest::LongTermExtraction { extraction } => {
                let store = self.config.platform.long_term_memory_store();
                let skill_storage = self.config.platform.skill_storage();
                let changed = apply_long_term_memory_extraction(
                    store.as_ref(),
                    skill_storage.as_ref(),
                    &extraction,
                    now_secs,
                )?;
                MemoryWriteReport {
                    accepted: true,
                    changed,
                    operation: "write.long_term_extraction",
                    reason: "long_term_extraction_applied".to_string(),
                    lifecycle_report: self.finish_lifecycle_success(
                        lifecycle,
                        RuntimeLifecycleEventKind::RuntimeLifecycle,
                        RuntimeLifecycleEffect::RequestLongTermRefresh,
                        changed > 0,
                        "write.long_term_extraction",
                    )?,
                }
            }
        };
        self.audit("write", true, &report.reason);
        Ok(report)
    }

    pub fn recall(&self, request: MemoryRecallRequest) -> Result<MemoryRecallReport> {
        self.ensure_visible("recall", self.capabilities.recall)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Inspect,
            RuntimeLifecycleTrigger::SdkCall,
            RuntimeLifecycleModeInput::default(),
        );
        let platform = self.config.platform.as_ref();
        let session_store = platform.session_store();
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
        let working = inspect_working_recall(WorkingRecallInspectionInput {
            chat_id: &self.config.scope.chat_id,
            query: &request.query,
            summary_text: summary.as_deref(),
            recent: &recent,
            system_max_len: 4096,
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
        self.audit("recall", true, "recall_completed");
        Ok(MemoryRecallReport {
            query: request.query,
            procedural_hits,
            working,
            lifecycle_report: self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::Inspect,
                false,
                "recall_completed",
            )?,
        })
    }

    pub fn project(&self, request: MemoryProjectionRequest) -> Result<MemoryProjectionReport> {
        self.ensure_visible("project", self.capabilities.projection)?;
        let lifecycle = self.start_lifecycle(
            RuntimeLifecycleOperation::Project,
            RuntimeLifecycleTrigger::SdkCall,
            self.mode_input_for_request(request.mode_input, request.pressure),
        );
        let platform = self.config.platform.as_ref();
        let session_store = platform.session_store();
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
        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: &self.config.scope.chat_id,
            current_channel: &self.config.scope.channel,
            user_query: &request.user_query,
            memory_system_kind,
            system_max_len: request.system_max_len,
            now_secs: self.config.clock.now_secs(),
            participation_plan: self.prompt_participation_plan(),
            recent_messages_limit: request.recent_messages_limit,
            load_long_term_memory: true,
            include_private_garden_projection: self
                .config
                .privacy_policy
                .private_plane_projection_allowed
                && lifecycle.admission.private_depth_allowed,
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
        });
        let system_memory_block = render_sdk_projection_block(&context, request.system_max_len);
        self.audit("project", true, "projection_completed");
        Ok(MemoryProjectionReport {
            system_memory_block,
            context,
            lifecycle_report: self.finish_lifecycle_success(
                lifecycle,
                RuntimeLifecycleEventKind::RuntimeLifecycle,
                RuntimeLifecycleEffect::RefreshProjection,
                false,
                "projection_completed",
            )?,
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
        let session_store = platform.session_store();
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
            user_content: &request.user_content,
            reply_content: &request.reply_content,
            pressure: request.pressure,
            memory_profile: self.memory_profile(),
            tool_calls: request.tool_calls,
            external_content_used: request.external_content_used,
            prompt_recall_intent: PromptRecallIntent::Factual,
            runtime_skill_selected_ids: request.runtime_skill_selected_ids,
            task_learning_selected_ids: request.task_learning_selected_ids,
            reuse_outcome: request.reuse_outcome,
            reuse_outcome_note: &request.reuse_outcome_note,
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
        let lifecycle_report = self.finish_lifecycle_success(
            lifecycle,
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::RunMaintenance,
            changed,
            "maintenance_completed",
        )?;
        Ok(MemoryMaintenanceReport {
            report: Some(report),
            long_term_refresh_enqueued,
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
        let session_store = platform.session_store();
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
            system_max_len: request.system_max_len,
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
        input.pressure = pressure;
        input
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
        let finished = report.finish_success(self.config.clock.now_secs(), changed, summary);
        self.record_lifecycle_event(kind, effect, &finished)?;
        Ok(finished)
    }

    fn record_lifecycle_event(
        &self,
        kind: RuntimeLifecycleEventKind,
        effect: RuntimeLifecycleEffect,
        report: &RuntimeLifecycleReport,
    ) -> Result<()> {
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
                );
        if kind == RuntimeLifecycleEventKind::OperatorAction {
            event = event
                .with_payload(
                    "action",
                    RuntimeOperatorAction::InspectMemoryStatus.as_str(),
                )
                .with_payload("accepted", report.success.to_string());
        }
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

    fn audit(&self, operation: &str, allowed: bool, reason: &str) {
        self.config.audit_sink.record(MemoryAuditEvent {
            operation: operation.to_string(),
            profile: self.config.profile,
            scope: self.config.scope.clone(),
            allowed,
            reason: reason.to_string(),
        });
    }

    fn memory_profile(&self) -> MemoryProfile {
        match self.config.profile {
            ProfileId::EspStandaloneMemory | ProfileId::EspEmbeddedSdk => MemoryProfile::Embedded,
            ProfileId::LinuxDeviceStandaloneMemory
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

fn render_sdk_projection_block(context: &crate::PromptMemoryContext, max_len: usize) -> String {
    let mut parts = Vec::new();
    push_projection_part(&mut parts, context.summary_text.as_deref());
    push_projection_part(&mut parts, context.message_summary_text.as_deref());
    push_projection_part(
        &mut parts,
        context.personality_governance_gate_text.as_deref(),
    );
    push_projection_part(&mut parts, context.self_authored_core_text.as_deref());
    push_projection_part(
        &mut parts,
        context.relationship_constitution_text.as_deref(),
    );
    push_projection_part(&mut parts, context.persona_priority_text.as_deref());
    push_projection_part(&mut parts, context.long_term_memory_text.as_deref());
    push_projection_part(&mut parts, context.continuity_capsule_text.as_deref());
    push_projection_part(&mut parts, context.archive_evidence_text.as_deref());
    push_projection_part(&mut parts, context.runtime_skill_text.as_deref());
    push_projection_part(&mut parts, context.recent_turn_observation_text.as_deref());
    push_projection_part(&mut parts, context.work_continuity_text.as_deref());
    push_projection_part(&mut parts, context.execution_state_text.as_deref());
    push_projection_part(&mut parts, context.task_workspace_text.as_deref());
    push_projection_part(&mut parts, context.task_recall_text.as_deref());
    push_projection_part(&mut parts, context.world_snapshot_text.as_deref());
    push_projection_part(&mut parts, context.world_sense_text.as_deref());
    push_projection_part(&mut parts, context.self_state_text.as_deref());
    push_projection_part(&mut parts, context.relationship_portfolio_text.as_deref());
    push_projection_part(&mut parts, context.self_model_text.as_deref());
    push_projection_part(&mut parts, context.autonomy_strategy_text.as_deref());
    push_projection_part(&mut parts, context.outer_voice_text.as_deref());
    push_projection_part(&mut parts, context.inner_life_text.as_deref());
    push_projection_part(&mut parts, context.self_continuity_text.as_deref());
    push_projection_part(&mut parts, context.mental_privacy_text.as_deref());
    push_projection_part(
        &mut parts,
        context.mental_privacy_adjudication_text.as_deref(),
    );
    let joined = parts.join("\n\n");
    truncate_to_char_boundary(&joined, max_len)
}

fn push_projection_part(parts: &mut Vec<String>, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
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
    scope: Option<MemoryScope>,
    profile: ProfileId,
    platform: Option<Arc<dyn Platform>>,
    llm: Option<Arc<dyn LlmClient>>,
    clock: Option<Arc<dyn MemoryClock>>,
    capability_policy: MemoryCapabilityPolicy,
    privacy_policy: MemoryPrivacyPolicy,
    audit_sink: Option<Arc<dyn MemoryAuditSink>>,
}

impl Default for MemoryRuntimeBuilder {
    fn default() -> Self {
        Self {
            identity: None,
            scope: None,
            profile: ProfileId::ServerLinuxDevFull,
            platform: None,
            llm: None,
            clock: Some(Arc::new(SystemMemoryClock)),
            capability_policy: MemoryCapabilityPolicy::strict_profile(),
            privacy_policy: MemoryPrivacyPolicy::standard_private_boundary(),
            audit_sink: Some(Arc::new(NoopMemoryAuditSink)),
        }
    }
}

impl MemoryRuntimeBuilder {
    pub fn identity(mut self, identity: MemoryIdentity) -> Self {
        self.identity = Some(identity);
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
        self.platform = Some(platform.into_arc());
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

    pub fn build(self) -> Result<MemoryRuntime> {
        let identity = self
            .identity
            .ok_or_else(|| Error::config("memory_runtime_config", "identity must be configured"))?;
        let scope = self
            .scope
            .ok_or_else(|| Error::config("memory_runtime_config", "scope must be configured"))?;
        let platform = self
            .platform
            .ok_or_else(|| Error::config("memory_runtime_config", "platform must be configured"))?;
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
        let config = MemoryRuntimeConfig {
            identity,
            scope,
            profile: self.profile,
            platform,
            llm: self.llm,
            clock,
            capability_policy: self.capability_policy,
            privacy_policy: self.privacy_policy,
            audit_sink,
        };
        let runtime = MemoryRuntime {
            config,
            capabilities,
            lifecycle: RuntimeLifecycleEngine,
        };
        let lifecycle = runtime.start_lifecycle(
            RuntimeLifecycleOperation::Open,
            RuntimeLifecycleTrigger::SdkCall,
            RuntimeLifecycleModeInput::default(),
        );
        runtime.finish_lifecycle_success(
            lifecycle,
            RuntimeLifecycleEventKind::RuntimeLifecycle,
            RuntimeLifecycleEffect::Noop,
            false,
            "runtime_opened",
        )?;
        Ok(runtime)
    }
}
