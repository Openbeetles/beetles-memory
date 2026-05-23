use std::collections::HashSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::budget::{
    compile_runtime_budget, ProviderModelContextLimit, RuntimeBudgetInput, RuntimeBudgetReport,
    StaticPlatformManifest,
};
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
use bm_core::resource::RuntimeResourceSnapshot;
use bm_core::runtime::{
    build_runtime_lifecycle_diagnosis, ensure_platform_soul_kernel_recovery,
    RuntimeLifecycleDisposition, RuntimeLifecycleEffect, RuntimeLifecycleEngine,
    RuntimeLifecycleEvent, RuntimeLifecycleEventKind, RuntimeLifecycleModeInput,
    RuntimeLifecycleOperation, RuntimeLifecycleReport, RuntimeLifecycleTrigger,
};
use bm_core::skills::{
    delete_skill as delete_skill_record, get_disabled_skills, get_skill_content, get_skills_order,
    is_runtime_skill_name, list_runtime_skill_records, list_skill_names,
    retrieve_runtime_skill_hits, set_skill_enabled as set_skill_enabled_record, set_skills_order,
    write_governed_runtime_skills, RuntimeSkillOrigin as CoreRuntimeSkillOrigin,
    RuntimeSkillRecord, RuntimeSkillStatus,
};
use bm_store::StorePlatform;

use crate::{
    resolve_memory_capabilities, Error, LlmClient, MemoryCapabilityCatalog, MemoryCapabilityPolicy,
    MemoryCloseReport, MemoryCloseRequest, MemoryExportReport, MemoryExportRequest,
    MemoryImportReport, MemoryImportRequest, MemoryInspectionReport, MemoryInspectionRequest,
    MemoryMaintenanceReport, MemoryMaintenanceRequest, MemoryOperationVisibility,
    MemoryPrivacyPolicy, MemoryProfile, MemoryProjectionReport, MemoryProjectionRequest,
    MemoryRecallReport, MemoryRecallRequest, MemoryRecoverReport, MemoryRecoverRequest,
    MemoryReplayReport, MemoryReplayRequest, MemoryRuntimeSystemKind, MemorySkillDeleteRequest,
    MemorySkillDetailReport, MemorySkillDetailRequest, MemorySkillKind, MemorySkillListReport,
    MemorySkillListRequest, MemorySkillMutationReport, MemorySkillOrigin,
    MemorySkillSetEnabledRequest, MemorySkillSummary, MemorySkillUpsertRequest, MemoryWriteReport,
    MemoryWriteRequest, PressureLevel, Result, RuntimeOperatorAction, RuntimeOperatorActionReport,
    RuntimeSkillWrite, RuntimeSkillWriteSource,
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
    pub runtime_budget: RuntimeBudgetReport,
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

    pub fn runtime_budget(&self) -> &RuntimeBudgetReport {
        &self.config.runtime_budget
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
                    lifecycle_report: self.finish_lifecycle_success_with_payload(
                        lifecycle,
                        RuntimeLifecycleEventKind::RuntimeLifecycle,
                        RuntimeLifecycleEffect::RunMaintenance,
                        outcome.changed > 0,
                        "write.procedural",
                        &[("changed_count", outcome.changed.to_string())],
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
                    lifecycle_report: self.finish_lifecycle_success_with_payload(
                        lifecycle,
                        RuntimeLifecycleEventKind::RuntimeLifecycle,
                        RuntimeLifecycleEffect::RequestLongTermRefresh,
                        changed > 0,
                        "write.long_term_extraction",
                        &[("changed_count", changed.to_string())],
                    )?,
                }
            }
        };
        self.audit("write", true, &report.reason);
        Ok(report)
    }

    pub fn list_skills(&self, request: MemorySkillListRequest) -> Result<MemorySkillListReport> {
        self.ensure_visible("inspect.skills", self.capabilities.inspection)?;
        let platform = self.config.platform.as_ref();
        let storage = platform.skill_storage();
        let meta_store = platform.skill_meta_store();
        let disabled: HashSet<String> = get_disabled_skills(meta_store.as_ref())
            .into_iter()
            .collect();
        let runtime_records = list_runtime_skill_records(storage.as_ref());
        let runtime_names: HashSet<String> = runtime_records
            .iter()
            .map(|record| record.name.clone())
            .collect();
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

        for name in list_skill_names(storage.as_ref()) {
            if runtime_names.contains(&name) || is_runtime_skill_name(&name) {
                continue;
            }
            let Some(content) = get_skill_content(storage.as_ref(), &name) else {
                continue;
            };
            let enabled = !disabled.contains(&name);
            let summary = manual_skill_summary(&name, &content, enabled);
            if !request.include_disabled && !summary.enabled {
                continue;
            }
            if !skill_matches_query(&summary, Some(&content), None, query.as_deref()) {
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
        let runtime_learned = rows
            .iter()
            .filter(|skill| skill.origin == MemorySkillOrigin::RuntimeLearned)
            .count();
        let user_provided = rows
            .iter()
            .filter(|skill| skill.origin == MemorySkillOrigin::UserProvided)
            .count();
        let skills = if request.limit == 0 {
            Vec::new()
        } else {
            rows.into_iter().take(request.limit).collect()
        };
        self.audit("inspect.skills", true, "skill_list_completed");
        Ok(MemorySkillListReport {
            total,
            active,
            disabled: disabled_count,
            runtime_learned,
            user_provided,
            skills,
        })
    }

    pub fn get_skill(&self, request: MemorySkillDetailRequest) -> Result<MemorySkillDetailReport> {
        self.ensure_visible("inspect.skills", self.capabilities.inspection)?;
        let name = checked_skill_name(&request.name, "skill_detail")?;
        let platform = self.config.platform.as_ref();
        let storage = platform.skill_storage();
        if !list_skill_names(storage.as_ref())
            .iter()
            .any(|value| value == name)
        {
            self.audit("inspect.skills", false, "skill_not_found");
            return Err(Error::config("skill_detail", "skill not found"));
        }
        let raw_content = get_skill_content(storage.as_ref(), name)
            .ok_or_else(|| Error::config("skill_detail", "skill not found"))?;
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
            self.audit("inspect.skills", true, "skill_detail_completed");
            return Ok(MemorySkillDetailReport {
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

        let summary = manual_skill_summary(name, &raw_content, !disabled.contains(name));
        self.audit("inspect.skills", true, "skill_detail_completed");
        Ok(MemorySkillDetailReport {
            summary,
            summary_text: summarize_manual_skill(&raw_content),
            procedure_text: raw_content.clone(),
            raw_content,
            citations: Vec::new(),
            source_chat_id: None,
            lineage: Vec::new(),
            strategy_diffs: Vec::new(),
            last_outcome_note: String::new(),
        })
    }

    pub fn upsert_skill(
        &self,
        request: MemorySkillUpsertRequest,
    ) -> Result<MemorySkillMutationReport> {
        self.ensure_visible("write.skills", self.capabilities.write)?;
        let title = checked_non_empty(&request.title, "skill_upsert", "title must not be empty")?;
        let topic = checked_non_empty(&request.topic, "skill_upsert", "topic must not be empty")?;
        let summary = checked_non_empty(
            &request.summary,
            "skill_upsert",
            "summary must not be empty",
        )?;
        let procedure = checked_non_empty(
            &request.procedure,
            "skill_upsert",
            "procedure must not be empty",
        )?;
        let name = request
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| sdk_runtime_skill_name(topic));
        let write = RuntimeSkillWrite {
            name,
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
            .ok_or_else(|| Error::config("skill_upsert", "skill write missing"))?;
        let stored_name = normalized.name.clone();
        let storage = self.config.platform.skill_storage();
        let outcome = write_governed_runtime_skills(
            storage.as_ref(),
            &[normalized],
            RuntimeSkillWriteSource::Manual,
        )?;
        let accepted = outcome.accepted > 0;
        let reason = format!(
            "submitted={}, accepted={}, rejected={}",
            outcome.submitted, outcome.accepted, outcome.rejected
        );
        self.audit("write.skills", accepted, &reason);
        Ok(MemorySkillMutationReport {
            accepted,
            changed: outcome.changed > 0,
            name: stored_name,
            operation: "skill.upsert",
            reason,
        })
    }

    pub fn set_skill_enabled(
        &self,
        request: MemorySkillSetEnabledRequest,
    ) -> Result<MemorySkillMutationReport> {
        self.ensure_visible("write.skills", self.capabilities.write)?;
        let name = checked_skill_name(&request.name, "skill_set_enabled")?;
        let platform = self.config.platform.as_ref();
        let storage = platform.skill_storage();
        if !list_skill_names(storage.as_ref())
            .iter()
            .any(|value| value == name)
        {
            self.audit("write.skills", false, "skill_not_found");
            return Err(Error::config("skill_set_enabled", "skill not found"));
        }
        let meta_store = platform.skill_meta_store();
        let was_disabled = get_disabled_skills(meta_store.as_ref())
            .into_iter()
            .any(|value| value == name);
        set_skill_enabled_record(meta_store.as_ref(), name, request.enabled)?;
        let changed = was_disabled == request.enabled;
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
        Ok(MemorySkillMutationReport {
            accepted: true,
            changed,
            name: name.to_string(),
            operation,
            reason: reason.to_string(),
        })
    }

    pub fn delete_skill(
        &self,
        request: MemorySkillDeleteRequest,
    ) -> Result<MemorySkillMutationReport> {
        self.ensure_visible("write.skills", self.capabilities.write)?;
        let name = checked_skill_name(&request.name, "skill_delete")?;
        let platform = self.config.platform.as_ref();
        let storage = platform.skill_storage();
        if !list_skill_names(storage.as_ref())
            .iter()
            .any(|value| value == name)
        {
            self.audit("write.skills", false, "skill_not_found");
            return Err(Error::config("skill_delete", "skill not found"));
        }
        delete_skill_record(storage.as_ref(), name)?;
        let meta_store = platform.skill_meta_store();
        set_skill_enabled_record(meta_store.as_ref(), name, true)?;
        let mut order = get_skills_order(meta_store.as_ref());
        let before_len = order.len();
        order.retain(|value| value != name);
        if before_len != order.len() {
            set_skills_order(meta_store.as_ref(), &order)?;
        }
        self.audit("write.skills", true, "skill_deleted");
        Ok(MemorySkillMutationReport {
            accepted: true,
            changed: true,
            name: name.to_string(),
            operation: "skill.delete",
            reason: "skill_deleted".to_string(),
        })
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
        let hit_count = procedural_hits
            .len()
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
            working,
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
        let system_memory_block = render_sdk_projection_block(&context, render_max_chars);
        let hit_count = prompt_context_hit_count(&context);
        let system_memory_chars = system_memory_block.chars().count();
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
        input.pressure = max_pressure(
            pressure,
            self.config.runtime_budget.resource_snapshot.pressure,
        );
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

    fn record_lifecycle_event(
        &self,
        kind: RuntimeLifecycleEventKind,
        effect: RuntimeLifecycleEffect,
        report: &RuntimeLifecycleReport,
        extra_payload: &[(&str, String)],
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

fn render_sdk_projection_block(context: &crate::PromptMemoryContext, max_len: usize) -> String {
    let mut parts = Vec::new();
    for value in sdk_projection_text_parts(context) {
        push_projection_part(&mut parts, value);
    }
    let joined = parts.join("\n\n");
    truncate_to_char_boundary(&joined, max_len)
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

fn sdk_projection_text_parts(context: &crate::PromptMemoryContext) -> [Option<&str>; 28] {
    [
        context.summary_text.as_deref(),
        context.message_summary_text.as_deref(),
        context.personality_governance_gate_text.as_deref(),
        context.self_authored_core_text.as_deref(),
        context.relationship_constitution_text.as_deref(),
        context.persona_priority_text.as_deref(),
        context.long_term_memory_text.as_deref(),
        context.continuity_capsule_text.as_deref(),
        context.archive_evidence_text.as_deref(),
        context.runtime_skill_text.as_deref(),
        context.recent_turn_observation_text.as_deref(),
        context.work_continuity_text.as_deref(),
        context.execution_state_text.as_deref(),
        context.task_workspace_text.as_deref(),
        context.task_recall_text.as_deref(),
        context.world_snapshot_text.as_deref(),
        context.world_sense_text.as_deref(),
        context.self_state_text.as_deref(),
        context.relationship_portfolio_text.as_deref(),
        context.self_model_text.as_deref(),
        context.autonomy_strategy_text.as_deref(),
        context.outer_voice_text.as_deref(),
        context.inner_life_text.as_deref(),
        context.self_continuity_text.as_deref(),
        context.private_workspace_text.as_deref(),
        context.private_garden_text.as_deref(),
        context.mental_privacy_text.as_deref(),
        context.mental_privacy_adjudication_text.as_deref(),
    ]
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

fn runtime_skill_summary(record: &RuntimeSkillRecord, enabled: bool) -> MemorySkillSummary {
    MemorySkillSummary {
        name: record.name.clone(),
        kind: MemorySkillKind::RuntimeSkill,
        origin: sdk_skill_origin(record.origin),
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

fn manual_skill_summary(name: &str, content: &str, enabled: bool) -> MemorySkillSummary {
    MemorySkillSummary {
        name: name.to_string(),
        kind: MemorySkillKind::ManualDocument,
        origin: MemorySkillOrigin::UserProvided,
        title: manual_skill_title(name, content),
        topic: name.to_string(),
        status: if enabled { "active" } else { "disabled" }.to_string(),
        enabled,
        quality_score: None,
        use_count: 0,
        validated_success_count: 0,
        mismatch_count: 0,
        revision_pending: false,
        updated_at: 0,
        last_used_at: None,
    }
}

fn sdk_skill_origin(origin: CoreRuntimeSkillOrigin) -> MemorySkillOrigin {
    match origin {
        CoreRuntimeSkillOrigin::RuntimeLearned => MemorySkillOrigin::RuntimeLearned,
        CoreRuntimeSkillOrigin::UserProvided => MemorySkillOrigin::UserProvided,
    }
}

fn manual_skill_title(name: &str, content: &str) -> String {
    content
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix('#').map(str::trim))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| name.to_string())
}

fn summarize_manual_skill(content: &str) -> String {
    let text = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    truncate_to_char_boundary(text.trim(), 240)
}

fn skill_matches_query(
    summary: &MemorySkillSummary,
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
            runtime_resource_snapshot: None,
            static_platform_manifest: None,
            provider_model_context_limit: None,
            runtime_budget: None,
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
            runtime_budget,
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
