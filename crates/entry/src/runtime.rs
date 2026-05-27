use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use bm_adapter::{
    dispatch_adapter_command_with_services, AdapterCommand, AdapterEnvelope, AdapterErrorKey,
    AdapterResponse, AdapterRuntimeServices,
};
use bm_replay::{load_memory_benchmark_fixture_dir, run_memory_benchmark_wall};
use bm_sdk::{
    compile_runtime_budget, probe_host_runtime_resource, resolve_memory_capabilities, Error,
    MemoryCapabilityPolicy, MemoryCloseRequest, MemoryIdentity, MemoryInspectionRequest,
    MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemorySkillDeleteRequest, MemorySkillDetailRequest, MemorySkillListRequest,
    MemorySkillSetEnabledRequest, MemorySkillUpsertRequest, MemorySpaceExportRequest,
    MemorySpaceMigratePreviewRequest, NoopMemoryAuditSink, PressureLevel, ProfileId, Result,
    RuntimeBudgetInput, RuntimeBudgetReport, RuntimeLifecycleModeInput, StaticPlatformManifest,
    StoreBackendConfig, StoreBackendKind, StorePlatform, WorkbenchApiMap, WorkbenchSurface,
};

use crate::config::{enabled_capability_policy, privacy_policy};
use crate::console::EntryConsoleTelemetrySnapshot;
use crate::{
    EntryAuthConfig, EntryCapabilityView, EntryConsoleDevice, EntryConsoleDeviceCreate,
    EntryConsoleDeviceKeyReport, EntryConsoleDeviceUpdate, EntryConsoleMemoryBenchmarkReport,
    EntryConsoleOverview, EntryConsoleSession, EntryConsoleSkillDetail, EntryConsoleSkillList,
    EntryConsoleSkillMutation, EntryConsoleSkillSetEnabled, EntryConsoleSkillUpsert,
    EntryConsoleState, EntryConsoleTransport, EntryConsoleTransportUpdate,
    EntryConsoleWorkbenchBenchmarkWall, EntryConsoleWorkbenchProceduralEvolution,
    EntryConsoleWorkbenchProjectionInspector, EntryConsoleWorkbenchRecallInspector,
    EntryConsoleWorkbenchReport, EntryConsoleWorkbenchSkillRef, EntryConsoleWorkbenchSoulHealth,
    EntryConsoleWorkbenchStatus, EntryConsoleWorkbenchVaultMigration, EntryIdempotencyCache,
    EntryIdempotencyConfig, EntryIdentity, EntryResponse, EntryScope, EntryStoreConfig,
    EntryTransportConfig, EntryTransportContext,
};

pub const DEFAULT_SCOPED_RUNTIME_CACHE_LIMIT: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryRuntimeBaseConfig {
    pub profile: ProfileId,
    pub store: EntryStoreConfig,
    pub transports: EntryTransportConfig,
    pub auth: EntryAuthConfig,
    pub idempotency: EntryIdempotencyConfig,
    pub privacy: MemoryPrivacyPolicy,
    pub capability: MemoryCapabilityPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntryRuntimeScope {
    pub identity: EntryIdentity,
    pub scope: EntryScope,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryRuntimeConfig {
    pub profile: ProfileId,
    pub identity: EntryIdentity,
    pub scope: EntryScope,
    pub store: EntryStoreConfig,
    pub transports: EntryTransportConfig,
    pub auth: EntryAuthConfig,
    pub idempotency: EntryIdempotencyConfig,
    pub privacy: MemoryPrivacyPolicy,
    pub capability: MemoryCapabilityPolicy,
}

impl EntryRuntimeConfig {
    pub fn base_config(&self) -> EntryRuntimeBaseConfig {
        EntryRuntimeBaseConfig {
            profile: self.profile,
            store: self.store.clone(),
            transports: self.transports.clone(),
            auth: self.auth.clone(),
            idempotency: self.idempotency.clone(),
            privacy: self.privacy.clone(),
            capability: self.capability.clone(),
        }
    }

    pub fn runtime_scope(&self) -> EntryRuntimeScope {
        EntryRuntimeScope {
            identity: self.identity.clone(),
            scope: self.scope.clone(),
        }
    }
}

pub struct EntryRuntimeFactory {
    base: EntryRuntimeBaseConfig,
    store: StorePlatform,
    runtime_budget: RuntimeBudgetReport,
}

impl EntryRuntimeFactory {
    pub fn open(base: EntryRuntimeBaseConfig) -> Result<Self> {
        let runtime_budget = compile_entry_runtime_budget(base.profile);
        let store = open_store(&base.store, base.profile, &runtime_budget)?;
        Ok(Self {
            base,
            store,
            runtime_budget,
        })
    }

    pub fn runtime_for_scope(&self, scope: EntryRuntimeScope) -> Result<EntryRuntime> {
        let config = EntryRuntimeConfig {
            profile: self.base.profile,
            identity: scope.identity,
            scope: scope.scope,
            store: self.base.store.clone(),
            transports: self.base.transports.clone(),
            auth: self.base.auth.clone(),
            idempotency: self.base.idempotency.clone(),
            privacy: self.base.privacy.clone(),
            capability: self.base.capability.clone(),
        };
        EntryRuntime::from_store_platform(config, self.store.clone(), self.runtime_budget.clone())
    }
}

pub struct EntryRuntimeManager {
    factory: EntryRuntimeFactory,
    max_runtimes: usize,
    state: Mutex<EntryRuntimeManagerState>,
}

#[derive(Default)]
struct EntryRuntimeManagerState {
    cached: HashMap<EntryRuntimeScope, Arc<EntryRuntime>>,
    active_evicted: HashMap<EntryRuntimeScope, Weak<EntryRuntime>>,
    lru: VecDeque<EntryRuntimeScope>,
}

impl EntryRuntimeManager {
    pub fn open(base: EntryRuntimeBaseConfig) -> Result<Self> {
        Self::with_max_runtimes(base, DEFAULT_SCOPED_RUNTIME_CACHE_LIMIT)
    }

    pub fn with_max_runtimes(base: EntryRuntimeBaseConfig, max_runtimes: usize) -> Result<Self> {
        if max_runtimes == 0 {
            return Err(Error::config(
                "entry_runtime_manager",
                "max_runtimes must be greater than zero",
            ));
        }
        Ok(Self {
            factory: EntryRuntimeFactory::open(base)?,
            max_runtimes,
            state: Mutex::new(EntryRuntimeManagerState::default()),
        })
    }

    pub fn runtime_for_scope(&self, scope: EntryRuntimeScope) -> Result<Arc<EntryRuntime>> {
        let mut close_after_unlock = Vec::new();
        let mut state = self
            .state
            .lock()
            .expect("entry runtime manager cache poisoned");
        state.prune_dead_active_evicted();
        if let Some(runtime) = state.cached.get(&scope).cloned() {
            state.touch(&scope);
            return Ok(Arc::clone(&runtime));
        }
        if let Some(runtime) = state.active_evicted.get(&scope).and_then(Weak::upgrade) {
            return Ok(runtime);
        }
        state.active_evicted.remove(&scope);

        let runtime = Arc::new(self.factory.runtime_for_scope(scope.clone())?);
        while state.cached.len() >= self.max_runtimes {
            let Some(oldest) = state.lru.pop_front() else {
                break;
            };
            if let Some(evicted) = state.cached.remove(&oldest) {
                if Arc::strong_count(&evicted) == 1 {
                    close_after_unlock.push(evicted);
                } else {
                    state
                        .active_evicted
                        .insert(oldest, Arc::downgrade(&evicted));
                }
            }
        }
        state.lru.push_back(scope.clone());
        state.cached.insert(scope, Arc::clone(&runtime));
        drop(state);
        for evicted in close_after_unlock {
            evicted.runtime.close(MemoryCloseRequest {
                reason: "entry_runtime_manager_evicted".to_string(),
            })?;
        }
        Ok(runtime)
    }
}

impl EntryRuntimeManagerState {
    fn touch(&mut self, scope: &EntryRuntimeScope) {
        if let Some(index) = self.lru.iter().position(|candidate| candidate == scope) {
            self.lru.remove(index);
        }
        self.lru.push_back(scope.clone());
    }

    fn prune_dead_active_evicted(&mut self) {
        self.active_evicted
            .retain(|_, runtime| runtime.strong_count() > 0);
    }
}

pub struct EntryRuntime {
    config: EntryRuntimeConfig,
    store: StorePlatform,
    runtime: MemoryRuntime,
    runtime_budget: RuntimeBudgetReport,
    capability: EntryCapabilityView,
    idempotency: EntryIdempotencyCache,
    console: EntryConsoleState,
}

impl EntryRuntime {
    pub fn open(config: EntryRuntimeConfig) -> Result<Self> {
        let factory = EntryRuntimeFactory::open(config.base_config())?;
        factory.runtime_for_scope(config.runtime_scope())
    }

    fn from_store_platform(
        config: EntryRuntimeConfig,
        store: StorePlatform,
        runtime_budget: RuntimeBudgetReport,
    ) -> Result<Self> {
        let capability_policy = enabled_capability_policy(config.capability.clone());
        let privacy = privacy_policy(config.privacy.clone());
        let runtime = MemoryRuntime::builder()
            .identity(MemoryIdentity::new(
                config.identity.agent_id.clone(),
                config.identity.owner_id.clone(),
            )?)
            .scope(MemoryScope::new(
                config.scope.channel.clone(),
                config.scope.chat_id.clone(),
            )?)
            .profile(config.profile)
            .store_platform(store.clone())
            .runtime_budget(runtime_budget.clone())
            .capability_policy(capability_policy.clone())
            .privacy_policy(privacy.clone())
            .audit_sink(Arc::new(NoopMemoryAuditSink))
            .build()?;
        let capability = entry_capability_view(
            config.profile,
            &capability_policy,
            &privacy,
            &config.transports,
        )?;
        let idempotency = EntryIdempotencyCache::new(config.idempotency.max_keys);
        let console = EntryConsoleState::new(&config);
        Ok(Self {
            config,
            store,
            runtime,
            runtime_budget,
            capability,
            idempotency,
            console,
        })
    }

    pub fn runtime(&self) -> &MemoryRuntime {
        &self.runtime
    }

    pub fn runtime_budget(&self) -> &RuntimeBudgetReport {
        &self.runtime_budget
    }

    pub fn uses_local_default_scope_policy(&self) -> bool {
        !self.config.auth.require_auth
    }

    pub fn capability(&self) -> &EntryCapabilityView {
        &self.capability
    }

    pub fn console_overview(&self) -> EntryConsoleOverview {
        self.console_overview_with_event_store_paths(&[])
    }

    pub fn console_overview_with_event_store_paths(
        &self,
        event_store_paths: &[PathBuf],
    ) -> EntryConsoleOverview {
        let telemetry = self.console_telemetry_snapshot(event_store_paths);
        let deferred_governance = self
            .runtime
            .deferred_governance_report()
            .unwrap_or_default();
        self.console.overview_with_telemetry_and_budget(
            telemetry,
            &self.runtime_budget,
            deferred_governance,
        )
    }

    pub fn console_workbench_api_map(&self) -> WorkbenchApiMap {
        WorkbenchApiMap {
            surfaces: vec![
                WorkbenchSurface {
                    surface_id: "home".to_string(),
                    report_api: "entry.console.overview".to_string(),
                    private_raw_allowed: false,
                },
                WorkbenchSurface {
                    surface_id: "recall_inspector".to_string(),
                    report_api: "sdk.recall.working_inspection".to_string(),
                    private_raw_allowed: false,
                },
                WorkbenchSurface {
                    surface_id: "projection_inspector".to_string(),
                    report_api: "sdk.project.subject_projection".to_string(),
                    private_raw_allowed: false,
                },
                WorkbenchSurface {
                    surface_id: "soul_health".to_string(),
                    report_api: "sdk.inspect.operator_surface.soul_governance".to_string(),
                    private_raw_allowed: false,
                },
                WorkbenchSurface {
                    surface_id: "procedural_evolution".to_string(),
                    report_api: "sdk.skills.skill_evolution_report".to_string(),
                    private_raw_allowed: false,
                },
                WorkbenchSurface {
                    surface_id: "replay_diff".to_string(),
                    report_api: "sdk.replay.memory_benchmark_report".to_string(),
                    private_raw_allowed: false,
                },
                WorkbenchSurface {
                    surface_id: "vault_migration".to_string(),
                    report_api: "sdk.vault.redaction_preflight".to_string(),
                    private_raw_allowed: false,
                },
            ],
            missing_report_apis: Vec::new(),
        }
    }

    pub fn console_workbench_report(&self) -> EntryConsoleWorkbenchReport {
        EntryConsoleWorkbenchReport {
            api_map: self.console_workbench_api_map(),
            benchmark_wall: self.console_workbench_benchmark_wall(),
            recall_inspector: self.console_workbench_recall_inspector(),
            projection_inspector: self.console_workbench_projection_inspector(),
            procedural_evolution: self.console_workbench_procedural_evolution(),
            vault_migration: self.console_workbench_vault_migration(),
            soul_health: self.console_workbench_soul_health(),
        }
    }

    fn console_workbench_benchmark_wall(&self) -> EntryConsoleWorkbenchBenchmarkWall {
        let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("fixtures/memory-benchmark-wall");
        let fixture_root_display = fixture_root.display().to_string();
        match load_memory_benchmark_fixture_dir(&fixture_root) {
            Ok(fixtures) => {
                let report = run_memory_benchmark_wall(&fixtures);
                let status = if report.passed {
                    EntryConsoleWorkbenchStatus::ready("memory_benchmark_wall_passed")
                } else {
                    EntryConsoleWorkbenchStatus::limited("memory_benchmark_wall_regression")
                };
                EntryConsoleWorkbenchBenchmarkWall {
                    status,
                    fixture_root: fixture_root_display,
                    report: Some(EntryConsoleMemoryBenchmarkReport::from_report(report)),
                }
            }
            Err(error) => EntryConsoleWorkbenchBenchmarkWall {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                fixture_root: fixture_root_display,
                report: None,
            },
        }
    }

    fn console_workbench_recall_inspector(&self) -> EntryConsoleWorkbenchRecallInspector {
        let query = "workbench memory inspection".to_string();
        match self.runtime.recall(MemoryRecallRequest {
            query: query.clone(),
            limit: 6,
        }) {
            Ok(report) => {
                let working_selected_surfaces = usize::from(report.working.summary_text.is_some())
                    + usize::from(report.working.work_continuity_text.is_some())
                    + usize::from(report.working.long_term_memory_text.is_some())
                    + usize::from(report.working.archive_evidence_text.is_some())
                    + usize::from(report.working.continuity_capsule_text.is_some())
                    + usize::from(report.working.runtime_skill_text.is_some())
                    + usize::from(report.working.task_recall_text.is_some());
                EntryConsoleWorkbenchRecallInspector {
                    status: EntryConsoleWorkbenchStatus::ready("sdk_recall_report_available"),
                    query,
                    procedural_hits: report.procedural_hits.len(),
                    runtime_skill_selected: report.working.runtime_skill_report.selected_count,
                    working_selected_surfaces,
                    graph_nodes: report.graph_gate.nodes,
                    graph_edges: report.graph_gate.edges,
                    evidence_backlinks: report.graph_gate.evidence_backlinks,
                    high_confidence_projection_allowed: report
                        .graph_gate
                        .high_confidence_projection_allowed,
                    graph_failures: report.graph_gate.failures,
                    graph_selected_ids: report.graph_rerank.selected_ids,
                    stale_false_positive_count: report.graph_rerank.stale_false_positive_count,
                }
            }
            Err(error) => EntryConsoleWorkbenchRecallInspector {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                query,
                procedural_hits: 0,
                runtime_skill_selected: 0,
                working_selected_surfaces: 0,
                graph_nodes: 0,
                graph_edges: 0,
                evidence_backlinks: 0,
                high_confidence_projection_allowed: false,
                graph_failures: Vec::new(),
                graph_selected_ids: Vec::new(),
                stale_false_positive_count: 0,
            },
        }
    }

    fn console_workbench_projection_inspector(&self) -> EntryConsoleWorkbenchProjectionInspector {
        let query = "Show operator-safe memory workbench context.".to_string();
        match self.runtime.project(MemoryProjectionRequest {
            user_query: query.clone(),
            system_max_len: self
                .runtime_budget
                .projection_render_budget
                .system_block_max_chars,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        }) {
            Ok(report) => EntryConsoleWorkbenchProjectionInspector {
                status: EntryConsoleWorkbenchStatus::ready("sdk_projection_report_available"),
                query,
                system_memory_chars: report.audit.system_memory_chars,
                source_budget_chars: report.audit.source_budget_chars,
                render_budget_chars: report.audit.render_budget_chars,
                injected: report.audit.injected,
                truncated: report.audit.truncated,
                private_gate_allowed: report.audit.private_gate.allowed,
                private_gate_reason: report.audit.private_gate.reason,
                evidence_refs: report.subject_projection.evidence_refs.len(),
                budget_decisions: report.subject_projection.budget_decisions.len(),
                privacy_decisions: report.subject_projection.privacy_decisions.len(),
                dropped_candidates: report.subject_projection.dropped_candidates.len(),
                faithfulness_passed: report.projection_faithfulness.passed,
                unsupported_claims: report.projection_faithfulness.unsupported_claims,
                private_echo_guard_passed: report.private_echo_guard.passed,
                private_echo_count: report.private_echo_guard.private_echo_count,
            },
            Err(error) => EntryConsoleWorkbenchProjectionInspector {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                query,
                system_memory_chars: 0,
                source_budget_chars: 0,
                render_budget_chars: 0,
                injected: false,
                truncated: false,
                private_gate_allowed: false,
                private_gate_reason: "projection_unavailable".to_string(),
                evidence_refs: 0,
                budget_decisions: 0,
                privacy_decisions: 0,
                dropped_candidates: 0,
                faithfulness_passed: false,
                unsupported_claims: Vec::new(),
                private_echo_guard_passed: false,
                private_echo_count: 0,
            },
        }
    }

    fn console_workbench_procedural_evolution(&self) -> EntryConsoleWorkbenchProceduralEvolution {
        match self.runtime.list_skills(MemorySkillListRequest {
            query: None,
            include_disabled: true,
            include_retired: true,
            limit: 8,
        }) {
            Ok(report) => EntryConsoleWorkbenchProceduralEvolution {
                status: EntryConsoleWorkbenchStatus::ready("sdk_skill_evolution_surface_available"),
                total_skills: report.total,
                active_skills: report.active,
                runtime_learned: report.runtime_learned,
                user_provided: report.user_provided,
                disabled: report.disabled,
                top_skills: report
                    .skills
                    .into_iter()
                    .take(5)
                    .map(|skill| EntryConsoleWorkbenchSkillRef {
                        name: skill.name,
                        title: skill.title,
                        topic: skill.topic,
                        status: skill.status,
                        quality_score: skill.quality_score,
                    })
                    .collect(),
            },
            Err(error) => EntryConsoleWorkbenchProceduralEvolution {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                total_skills: 0,
                active_skills: 0,
                runtime_learned: 0,
                user_provided: 0,
                disabled: 0,
                top_skills: Vec::new(),
            },
        }
    }

    fn console_workbench_vault_migration(&self) -> EntryConsoleWorkbenchVaultMigration {
        let source_memory_space_id = self.config.scope.chat_id.clone();
        let target_memory_space_id = format!("{source_memory_space_id}-vault-preview");
        match self.runtime.export_memory_space(MemorySpaceExportRequest {
            memory_space_id: source_memory_space_id.clone(),
            include_private: false,
        }) {
            Ok(export) => {
                let preview =
                    self.runtime
                        .preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
                            source_memory_space_id: source_memory_space_id.clone(),
                            target_memory_space_id: target_memory_space_id.clone(),
                            source_profile: self.config.profile,
                            target_profile: self.config.profile,
                            snapshot: export.snapshot,
                        });
                match preview {
                    Ok(report) => {
                        let status = if report.vault_preflight.passed {
                            EntryConsoleWorkbenchStatus::ready("vault_migration_preflight_passed")
                        } else {
                            EntryConsoleWorkbenchStatus::limited(
                                "vault_migration_preflight_blocked",
                            )
                        };
                        EntryConsoleWorkbenchVaultMigration {
                            status,
                            source_memory_space_id,
                            target_memory_space_id,
                            json_docs: report.json_docs,
                            blobs: report.blobs,
                            events: report.events,
                            privacy_redactions: report.privacy_redactions,
                            loss_risk: report.loss_risk,
                            preflight_passed: report.vault_preflight.passed,
                            preflight_failures: vault_preflight_failures(
                                report.vault_preflight.schema_allowed,
                                report.vault_preflight.capability_allowed,
                                report.vault_preflight.privacy_allowed,
                                report.vault_preflight.lineage_allowed,
                            ),
                            snapshot_fingerprint: report.state_fingerprint,
                            event_fingerprint: report.event_fingerprint,
                        }
                    }
                    Err(error) => EntryConsoleWorkbenchVaultMigration {
                        status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                        source_memory_space_id,
                        target_memory_space_id,
                        json_docs: 0,
                        blobs: 0,
                        events: 0,
                        privacy_redactions: 0,
                        loss_risk: false,
                        preflight_passed: false,
                        preflight_failures: Vec::new(),
                        snapshot_fingerprint: String::new(),
                        event_fingerprint: String::new(),
                    },
                }
            }
            Err(error) => EntryConsoleWorkbenchVaultMigration {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                source_memory_space_id,
                target_memory_space_id,
                json_docs: 0,
                blobs: 0,
                events: 0,
                privacy_redactions: 0,
                loss_risk: false,
                preflight_passed: false,
                preflight_failures: Vec::new(),
                snapshot_fingerprint: String::new(),
                event_fingerprint: String::new(),
            },
        }
    }

    fn console_workbench_soul_health(&self) -> EntryConsoleWorkbenchSoulHealth {
        match self.runtime.inspect(MemoryInspectionRequest {
            query: "workbench soul health".to_string(),
            system_max_len: self
                .runtime_budget
                .projection_render_budget
                .system_block_max_chars,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        }) {
            Ok(report) => EntryConsoleWorkbenchSoulHealth {
                status: EntryConsoleWorkbenchStatus::ready("sdk_inspection_report_available"),
                profile: report.hygiene.profile,
                hygiene_summary: report.hygiene.summary,
                runtime_skill_records: report.hygiene.runtime_skill_records,
                deferred_total: report.deferred_governance.total,
                deferred_pending: report.deferred_governance.pending,
                deferred_failed: report.deferred_governance.failed,
                safe_actions: report.operator_action_report.safe_actions_available,
            },
            Err(error) => EntryConsoleWorkbenchSoulHealth {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                profile: self.config.profile.as_str().to_string(),
                hygiene_summary: "inspection_unavailable".to_string(),
                runtime_skill_records: 0,
                deferred_total: 0,
                deferred_pending: 0,
                deferred_failed: 0,
                safe_actions: Vec::new(),
            },
        }
    }

    pub fn console_transports(&self) -> Vec<EntryConsoleTransport> {
        self.console.transports()
    }

    pub fn console_update_transport(
        &self,
        id: &str,
        update: EntryConsoleTransportUpdate,
    ) -> Option<EntryConsoleTransport> {
        self.console.update_transport(id, update)
    }

    pub fn console_llm_gateway(&self) -> crate::console::EntryConsoleLlmGateway {
        self.console.llm_gateway()
    }

    pub fn console_run_llm_gateway_smoke_check(
        &self,
        id: &str,
    ) -> Option<crate::console::EntryConsoleLlmGatewaySmokeRunReport> {
        self.console.run_llm_gateway_smoke_check(id)
    }

    pub fn console_devices(&self) -> Vec<EntryConsoleDevice> {
        self.console.devices()
    }

    pub fn console_add_device(
        &self,
        request: EntryConsoleDeviceCreate,
    ) -> std::result::Result<EntryConsoleDeviceKeyReport, &'static str> {
        self.console.add_device(request)
    }

    pub fn console_update_device(
        &self,
        device_id: &str,
        update: EntryConsoleDeviceUpdate,
    ) -> Option<EntryConsoleDevice> {
        self.console.update_device(device_id, update)
    }

    pub fn console_rotate_device_key(
        &self,
        device_id: &str,
    ) -> Option<EntryConsoleDeviceKeyReport> {
        self.console.rotate_device_key(device_id)
    }

    pub fn console_session(&self) -> EntryConsoleSession {
        self.console.session()
    }

    pub fn console_skills(&self, query: Option<String>) -> Result<EntryConsoleSkillList> {
        self.runtime
            .list_skills(MemorySkillListRequest {
                query,
                include_disabled: true,
                include_retired: true,
                limit: 512,
            })
            .map(Into::into)
    }

    pub fn console_skill_detail(&self, name: &str) -> Result<Option<EntryConsoleSkillDetail>> {
        match self.runtime.get_skill(MemorySkillDetailRequest {
            name: name.to_string(),
        }) {
            Ok(report) => Ok(Some(report.into())),
            Err(error) if error.stage() == "skill_detail" => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn console_upsert_skill(
        &self,
        payload: EntryConsoleSkillUpsert,
    ) -> Result<EntryConsoleSkillMutation> {
        let existed = payload
            .name
            .as_deref()
            .and_then(|name| self.console_skill_detail(name).ok().flatten())
            .is_some();
        let report = self.runtime.upsert_skill(MemorySkillUpsertRequest {
            name: payload.name,
            title: payload.title,
            topic: payload.topic,
            summary: payload.summary,
            procedure: payload.procedure,
            citations: payload.citations,
            source_chat_id: payload
                .source_chat_id
                .or_else(|| Some(self.config.scope.chat_id.clone())),
            observed_at: current_unix_secs(),
        })?;
        let mutation: EntryConsoleSkillMutation = report.into();
        if mutation.accepted {
            self.console.record_skill_mutation(
                &mutation.name,
                if existed { "updated" } else { "imported" },
            );
        }
        Ok(mutation)
    }

    pub fn console_set_skill_enabled(
        &self,
        name: &str,
        payload: EntryConsoleSkillSetEnabled,
    ) -> Result<Option<EntryConsoleSkillMutation>> {
        match self
            .runtime
            .set_skill_enabled(MemorySkillSetEnabledRequest {
                name: name.to_string(),
                enabled: payload.enabled,
            }) {
            Ok(report) => {
                let mutation: EntryConsoleSkillMutation = report.into();
                if mutation.accepted {
                    self.console.record_skill_mutation(
                        &mutation.name,
                        if payload.enabled {
                            "enabled"
                        } else {
                            "disabled"
                        },
                    );
                }
                Ok(Some(mutation))
            }
            Err(error) if error.stage() == "skill_set_enabled" => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn console_delete_skill(&self, name: &str) -> Result<Option<EntryConsoleSkillMutation>> {
        match self.runtime.delete_skill(MemorySkillDeleteRequest {
            name: name.to_string(),
        }) {
            Ok(report) => {
                let mutation: EntryConsoleSkillMutation = report.into();
                if mutation.accepted {
                    self.console
                        .record_skill_mutation(&mutation.name, "deleted");
                }
                Ok(Some(mutation))
            }
            Err(error) if error.stage() == "skill_delete" => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn handle(
        &self,
        context: EntryTransportContext,
        command: AdapterCommand,
    ) -> Result<EntryResponse> {
        self.handle_with_services(context, command, AdapterRuntimeServices::none())
    }

    pub fn handle_with_services(
        &self,
        context: EntryTransportContext,
        command: AdapterCommand,
        services: AdapterRuntimeServices<'_>,
    ) -> Result<EntryResponse> {
        if let Some(reason) = auth_rejection_reason(&self.config.auth, &context.auth) {
            return Ok(EntryResponse::from_adapter(AdapterResponse::Rejected {
                request_id: context.request_id,
                audit_id: context.audit_id,
                error_key: AdapterErrorKey::Unauthorized,
                reason,
            }));
        }
        if is_mutation(command.operation()) && !self.idempotency.remember(&context.idempotency_key)
        {
            return Ok(EntryResponse::from_adapter(AdapterResponse::Duplicated {
                request_id: context.request_id,
                audit_id: context.audit_id,
                idempotency_key: context.idempotency_key,
            }));
        }

        let operation = command.operation();
        let source = context.source(&self.config.identity, &self.config.scope);
        let auth = context.auth.into_adapter();
        let envelope = AdapterEnvelope {
            request_id: context.request_id,
            transport: context.transport,
            mode: context.mode,
            operation: context.operation,
            source,
            auth,
            idempotency_key: context.idempotency_key,
            audit_id: context.audit_id,
            payload: command,
        };
        let response = dispatch_adapter_command_with_services(&self.runtime, envelope, services)
            .map(EntryResponse::from_adapter)?;
        self.console
            .record_adapter_response(operation, &response.adapter);
        Ok(response)
    }

    fn console_telemetry_snapshot(
        &self,
        event_store_paths: &[PathBuf],
    ) -> EntryConsoleTelemetrySnapshot {
        let mut events = Vec::new();
        let mut seen = HashSet::new();
        for event in self.store.read_events().unwrap_or_default() {
            if seen.insert(event.event_id.clone()) {
                events.push(event);
            }
        }
        for path in event_store_paths {
            for event in StorePlatform::read_file_store_events(path).unwrap_or_default() {
                if seen.insert(event.event_id.clone()) {
                    events.push(event);
                }
            }
        }
        EntryConsoleTelemetrySnapshot::from_events(&events)
    }
}

fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn vault_preflight_failures(
    schema_allowed: bool,
    capability_allowed: bool,
    privacy_allowed: bool,
    lineage_allowed: bool,
) -> Vec<String> {
    let mut failures = Vec::new();
    if !schema_allowed {
        failures.push("schema_not_allowed".to_string());
    }
    if !capability_allowed {
        failures.push("capability_not_allowed".to_string());
    }
    if !privacy_allowed {
        failures.push("privacy_not_allowed".to_string());
    }
    if !lineage_allowed {
        failures.push("lineage_not_allowed".to_string());
    }
    failures
}

pub fn entry_capability_view(
    profile: ProfileId,
    policy: &MemoryCapabilityPolicy,
    privacy: &MemoryPrivacyPolicy,
    transports: &EntryTransportConfig,
) -> Result<EntryCapabilityView> {
    let catalog = resolve_memory_capabilities(profile, policy, privacy)?;
    Ok(EntryCapabilityView::from_catalog(
        profile, &catalog, transports,
    ))
}

fn compile_entry_runtime_budget(profile: ProfileId) -> RuntimeBudgetReport {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    compile_runtime_budget(RuntimeBudgetInput {
        profile,
        resource_snapshot: probe_host_runtime_resource(now_secs),
        static_platform_manifest: StaticPlatformManifest::for_profile(profile),
        provider_model_context_limit: None,
    })
}

fn open_store(
    config: &EntryStoreConfig,
    profile: ProfileId,
    runtime_budget: &RuntimeBudgetReport,
) -> Result<StorePlatform> {
    let store_config = match config.backend {
        StoreBackendKind::InMemory => StoreBackendConfig::in_memory(profile)?,
        StoreBackendKind::Embedded => StoreBackendConfig::embedded(profile)?,
        StoreBackendKind::File => {
            let path = config.data_path.clone().ok_or_else(|| {
                Error::config("entry_store_config", "file store requires data_path")
            })?;
            StoreBackendConfig::file(path, profile)?
        }
        StoreBackendKind::Sqlite => {
            let path = config.data_path.clone().ok_or_else(|| {
                Error::config("entry_store_config", "sqlite store requires data_path")
            })?;
            StoreBackendConfig::sqlite(path, profile)?
        }
    }
    .with_fsync(config.fsync)
    .with_runtime_store_budget(runtime_budget.store_budget);
    StorePlatform::open(store_config)
}

fn auth_rejection_reason(
    auth_config: &EntryAuthConfig,
    decision: &crate::EntryAuthDecision,
) -> Option<String> {
    if !auth_config.require_auth {
        return None;
    }
    if decision.local_loopback {
        return Some("entry auth rejected loopback for remote/auth-required profile".to_string());
    }
    if !decision.authenticated {
        return Some(format!(
            "entry auth rejected request: {}",
            decision
                .rejection_reason
                .as_deref()
                .unwrap_or("unauthenticated")
        ));
    }
    if let Some(expected) = auth_config.token_fingerprint() {
        if decision.token_fingerprint.as_deref() != Some(expected.as_str()) {
            return Some("entry auth rejected request: token_fingerprint mismatch".to_string());
        }
    }
    None
}

const fn is_mutation(operation: bm_adapter::AdapterOperation) -> bool {
    matches!(
        operation,
        bm_adapter::AdapterOperation::Write
            | bm_adapter::AdapterOperation::Maintain
            | bm_adapter::AdapterOperation::Recover
            | bm_adapter::AdapterOperation::Import
            | bm_adapter::AdapterOperation::Close
    )
}
