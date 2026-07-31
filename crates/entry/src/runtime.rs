use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use bm_adapter::{
    dispatch_adapter_command_with_services, project_adapter_report, AdapterCommand,
    AdapterEnvelope, AdapterErrorKey, AdapterResponse, AdapterRuntimeServices,
};
#[cfg(feature = "nonproduction-replay-harness")]
use bm_replay::{load_memory_benchmark_fixture_dir, run_memory_benchmark_wall};
use bm_sdk::{
    default_agent_subject_id, resolve_memory_capabilities, AgentSkillDirConfig, Error,
    MemoryArchiveScope, MemoryCapabilityPolicy, MemoryCloseRequest, MemoryFacetRecallIndexReport,
    MemoryIdentity, MemoryInspectionRequest, MemoryPrivacyPolicy, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryRuntime, MemoryScope, MemorySpaceExportRequest,
    MemorySpacePrivateMaterialPolicy, MemoryStoreHandle, NoopMemoryAuditSink, PressureLevel,
    ProfileId, Result, RuntimeBudgetLease, RuntimeBudgetReport, RuntimeLifecycleModeInput,
    RuntimeMetricsQuery, RuntimeSkillDetailRequest, RuntimeSkillEditRequest,
    RuntimeSkillListRequest, RuntimeSkillOwnerLocator, RuntimeSkillOwningScope,
    RuntimeSkillRetireRequest, RuntimeSkillSetEnabledRequest, StoreBackendConfig, StoreOpenReport,
    WorkbenchApiMap, WorkbenchSurface,
};
use sha2::{Digest, Sha256};

use crate::config::{enabled_capability_policy, privacy_policy};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::EntryConsoleMemoryBenchmarkReport;
use crate::{
    EntryAuthConfig, EntryCapabilityView, EntryConsoleDevice, EntryConsoleDeviceCreate,
    EntryConsoleDeviceKeyReport, EntryConsoleDeviceUpdate, EntryConsoleOverview,
    EntryConsoleRuntimeSkillEdit, EntryConsoleSession, EntryConsoleSkillDetail,
    EntryConsoleSkillList, EntryConsoleSkillMutation, EntryConsoleSkillSetEnabled,
    EntryConsoleState, EntryConsoleTransport, EntryConsoleTransportUpdate,
    EntryConsoleWorkbenchArchiveRestore, EntryConsoleWorkbenchBenchmarkWall,
    EntryConsoleWorkbenchFacetInspector, EntryConsoleWorkbenchProceduralEvolution,
    EntryConsoleWorkbenchProjectionInspector, EntryConsoleWorkbenchRecallInspector,
    EntryConsoleWorkbenchReport, EntryConsoleWorkbenchSkillRef, EntryConsoleWorkbenchSoulHealth,
    EntryConsoleWorkbenchStatus, EntryIdempotencyCache, EntryIdempotencyConfig, EntryIdentity,
    EntryResponse, EntryScope, EntryTransportConfig, EntryTransportContext,
};

const FACET_AUDIT_MARKDOWN_FORMAT: &str = "obsidian-style-facet-audit-markdown";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryRuntimeBaseConfig {
    pub store: StoreBackendConfig,
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
    pub identity: EntryIdentity,
    pub scope: EntryScope,
    pub store: StoreBackendConfig,
    pub transports: EntryTransportConfig,
    pub auth: EntryAuthConfig,
    pub idempotency: EntryIdempotencyConfig,
    pub privacy: MemoryPrivacyPolicy,
    pub capability: MemoryCapabilityPolicy,
}

impl EntryRuntimeConfig {
    pub fn base_config(&self) -> EntryRuntimeBaseConfig {
        EntryRuntimeBaseConfig {
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
    store: MemoryStoreHandle,
}

impl EntryRuntimeFactory {
    pub fn open(base: EntryRuntimeBaseConfig) -> Result<Self> {
        let store = MemoryStoreHandle::open(base.store.clone())?;
        Ok(Self { base, store })
    }

    pub fn runtime_for_scope(&self, scope: EntryRuntimeScope) -> Result<EntryRuntime> {
        let config = EntryRuntimeConfig {
            identity: scope.identity,
            scope: scope.scope,
            store: self.base.store.clone(),
            transports: self.base.transports.clone(),
            auth: self.base.auth.clone(),
            idempotency: self.base.idempotency.clone(),
            privacy: self.base.privacy.clone(),
            capability: self.base.capability.clone(),
        };
        EntryRuntime::from_store_handle(config, self.store.clone())
    }

    pub fn runtime_budget(&self) -> RuntimeBudgetReport {
        self.store.runtime_budget()
    }

    pub fn acquire_budget_lease(&self) -> Result<EntryRuntimeBudgetLease> {
        self.store
            .acquire_runtime_budget_lease()
            .map(|inner| EntryRuntimeBudgetLease { inner })
    }

    pub fn execute_with_budget_lease<T>(
        &self,
        lease: &EntryRuntimeBudgetLease,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.store
            .execute_with_runtime_budget_lease(&lease.inner, operation)
    }
}

pub struct EntryRuntimeManager {
    factory: EntryRuntimeFactory,
    requested_max_runtimes: Option<usize>,
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
        let factory = EntryRuntimeFactory::open(base)?;
        Ok(Self {
            factory,
            requested_max_runtimes: None,
            state: Mutex::new(EntryRuntimeManagerState::default()),
        })
    }

    pub fn open_with_requested_max_runtimes(
        base: EntryRuntimeBaseConfig,
        requested_max_runtimes: usize,
    ) -> Result<Self> {
        if requested_max_runtimes == 0 {
            return Err(Error::config(
                "entry_runtime_manager",
                "requested_max_runtimes must be greater than zero",
            ));
        }
        let factory = EntryRuntimeFactory::open(base)?;
        Ok(Self {
            factory,
            requested_max_runtimes: Some(requested_max_runtimes),
            state: Mutex::new(EntryRuntimeManagerState::default()),
        })
    }

    pub fn runtime_budget(&self) -> RuntimeBudgetReport {
        self.factory.runtime_budget()
    }

    pub fn acquire_budget_lease(&self) -> Result<EntryRuntimeBudgetLease> {
        self.factory.acquire_budget_lease()
    }

    pub fn execute_with_budget_lease<T>(
        &self,
        lease: &EntryRuntimeBudgetLease,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.factory.execute_with_budget_lease(lease, operation)
    }

    pub fn max_runtimes(&self) -> usize {
        self.max_runtimes_for_report(&self.factory.runtime_budget())
    }

    pub fn runtime_for_scope(&self, scope: EntryRuntimeScope) -> Result<Arc<EntryRuntime>> {
        let report = self.factory.runtime_budget();
        if report.resource_snapshot.stale {
            return Err(Error::config(
                "entry_runtime_manager",
                "runtime cache admission requires a fresh resource report",
            ));
        }
        let max_runtimes = self.max_runtimes_for_report(&report);
        let mut close_after_unlock = Vec::new();
        let mut state = self
            .state
            .lock()
            .expect("entry runtime manager cache poisoned");
        state.prune_dead_active_evicted();
        state.evict_to_limit(max_runtimes, &mut close_after_unlock);
        if let Some(runtime) = state.cached.get(&scope).cloned() {
            state.touch(&scope);
            drop(state);
            close_entry_runtimes(close_after_unlock)?;
            return Ok(Arc::clone(&runtime));
        }
        if let Some(runtime) = state.active_evicted.get(&scope).and_then(Weak::upgrade) {
            drop(state);
            close_entry_runtimes(close_after_unlock)?;
            return Ok(runtime);
        }
        state.active_evicted.remove(&scope);

        let runtime = match self.factory.runtime_for_scope(scope.clone()) {
            Ok(runtime) => Arc::new(runtime),
            Err(error) => {
                drop(state);
                close_entry_runtimes(close_after_unlock)?;
                return Err(error);
            }
        };
        state.evict_to_limit(max_runtimes.saturating_sub(1), &mut close_after_unlock);
        state.lru.push_back(scope.clone());
        state.cached.insert(scope, Arc::clone(&runtime));
        drop(state);
        close_entry_runtimes(close_after_unlock)?;
        Ok(runtime)
    }

    fn max_runtimes_for_report(&self, report: &RuntimeBudgetReport) -> usize {
        effective_runtime_cache_limit(
            self.requested_max_runtimes,
            report.llm_gateway_budget.runtime_cache_max_runtimes,
        )
    }
}

fn effective_runtime_cache_limit(requested: Option<usize>, current_report_limit: usize) -> usize {
    let current = current_report_limit.max(1);
    requested.map_or(current, |value| value.min(current))
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

    fn evict_to_limit(&mut self, limit: usize, close_after_unlock: &mut Vec<Arc<EntryRuntime>>) {
        while self.cached.len() > limit {
            let Some(oldest) = self.lru.pop_front() else {
                break;
            };
            if let Some(evicted) = self.cached.remove(&oldest) {
                if Arc::strong_count(&evicted) == 1 {
                    close_after_unlock.push(evicted);
                } else {
                    self.active_evicted.insert(oldest, Arc::downgrade(&evicted));
                }
            }
        }
    }
}

fn close_entry_runtimes(runtimes: Vec<Arc<EntryRuntime>>) -> Result<()> {
    for runtime in runtimes {
        runtime.runtime.close(MemoryCloseRequest {
            reason: "entry_runtime_manager_evicted".to_string(),
        })?;
    }
    Ok(())
}

pub struct EntryRuntime {
    config: EntryRuntimeConfig,
    store: MemoryStoreHandle,
    runtime: MemoryRuntime,
    capability: EntryCapabilityView,
    idempotency: EntryIdempotencyCache,
    console: EntryConsoleState,
}

#[derive(Debug)]
pub struct EntryRuntimeBudgetLease {
    inner: RuntimeBudgetLease,
}

impl EntryRuntimeBudgetLease {
    pub fn report(&self) -> &RuntimeBudgetReport {
        self.inner.report()
    }

    pub fn report_id(&self) -> &str {
        self.inner.report_id()
    }
}

impl EntryRuntime {
    fn runtime_skill_subject_scope(&self) -> RuntimeSkillOwningScope {
        RuntimeSkillOwningScope::Subject {
            mounted_subject_id: default_agent_subject_id(&self.config.identity.agent_id),
        }
    }

    pub fn open(config: EntryRuntimeConfig) -> Result<Self> {
        let factory = EntryRuntimeFactory::open(config.base_config())?;
        factory.runtime_for_scope(config.runtime_scope())
    }

    fn from_store_handle(config: EntryRuntimeConfig, store: MemoryStoreHandle) -> Result<Self> {
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
            .store(store.clone())
            .agent_skill_dirs(agent_skill_dirs_from_env())
            .capability_policy(capability_policy.clone())
            .privacy_policy(privacy.clone())
            .audit_sink(Arc::new(NoopMemoryAuditSink))
            .build()?;
        let runtime_budget = runtime.runtime_budget();
        let capability = entry_capability_view(
            runtime_budget.profile,
            &capability_policy,
            &privacy,
            &config.transports,
        )?;
        let idempotency = EntryIdempotencyCache::new(config.idempotency.max_keys);
        let console = EntryConsoleState::new(&config, &runtime_budget);
        Ok(Self {
            config,
            store,
            runtime,
            capability,
            idempotency,
            console,
        })
    }

    pub fn runtime(&self) -> &MemoryRuntime {
        &self.runtime
    }

    pub fn runtime_budget(&self) -> RuntimeBudgetReport {
        self.runtime.runtime_budget()
    }

    pub fn acquire_budget_lease(&self) -> Result<EntryRuntimeBudgetLease> {
        self.runtime
            .acquire_runtime_budget_lease()
            .map(|inner| EntryRuntimeBudgetLease { inner })
    }

    pub fn execute_with_budget_lease<T>(
        &self,
        lease: &EntryRuntimeBudgetLease,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.runtime
            .execute_with_runtime_budget_lease(&lease.inner, operation)
    }

    pub fn uses_local_default_scope_policy(&self) -> bool {
        !self.config.auth.requires_auth()
    }

    pub fn has_bearer_verifier(&self) -> bool {
        self.config.auth.has_bearer_verifier()
    }

    pub fn authenticate_accepted_tcp_stream(
        &self,
        accepted: &crate::EntryAcceptedTcpStream,
        authorization: Option<&str>,
        loopback_principal: &str,
    ) -> crate::EntryAuthDecision {
        let decision = self.config.auth.authenticate_accepted_tcp_stream(
            accepted,
            authorization,
            loopback_principal,
        );
        if decision
            .bearer_principal()
            .is_some_and(|principal| principal.owner_id() != self.config.identity.owner_id.trim())
        {
            return self
                .config
                .auth
                .verify_bearer_for_owner(authorization, &self.config.identity.owner_id);
        }
        decision
    }

    pub fn authenticate_remote_bearer(
        &self,
        authorization: Option<&str>,
    ) -> crate::EntryAuthDecision {
        self.config
            .auth
            .verify_bearer_for_owner(authorization, &self.config.identity.owner_id)
    }

    pub fn authenticate_local_transport(
        &self,
        transport: crate::EntryLocalTransport,
        principal: &str,
    ) -> crate::EntryAuthDecision {
        self.config
            .auth
            .authenticate_local_transport(transport, principal)
    }

    pub fn accepted_at(&self) -> u64 {
        self.runtime.config().clock.now_secs()
    }

    pub fn capability(&self) -> &EntryCapabilityView {
        &self.capability
    }

    pub fn store_open_report(&self) -> &StoreOpenReport {
        self.store.open_report()
    }

    pub fn console_overview(&self) -> Result<EntryConsoleOverview> {
        self.console_overview_with_event_store_paths(&[])
    }

    pub fn console_overview_with_event_store_paths(
        &self,
        event_store_paths: &[PathBuf],
    ) -> Result<EntryConsoleOverview> {
        const SECS_PER_DAY: u64 = 24 * 60 * 60;
        let today_start = (self.accepted_at() / SECS_PER_DAY) * SECS_PER_DAY;
        let metrics = self.runtime.runtime_metrics_report_with_file_stores(
            RuntimeMetricsQuery {
                write_since_unix_secs: Some(today_start),
            },
            event_store_paths,
        )?;
        let deferred_governance = self.runtime.deferred_governance_report()?;
        let runtime_budget = self.runtime_budget();
        Ok(self.console.overview_with_runtime_metrics_and_budget(
            &metrics,
            &runtime_budget,
            deferred_governance,
        ))
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
                    surface_id: "facet_inspector".to_string(),
                    report_api: "sdk.recall.facet_index_report".to_string(),
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
                    surface_id: "archive_restore".to_string(),
                    report_api: "sdk.archive.typed_scope_root".to_string(),
                    private_raw_allowed: false,
                },
            ],
            missing_report_apis: {
                #[cfg(feature = "nonproduction-replay-harness")]
                {
                    Vec::new()
                }
                #[cfg(not(feature = "nonproduction-replay-harness"))]
                {
                    vec!["sdk.replay.memory_benchmark_report".to_string()]
                }
            },
        }
    }

    pub fn console_workbench_report(&self) -> EntryConsoleWorkbenchReport {
        EntryConsoleWorkbenchReport {
            api_map: self.console_workbench_api_map(),
            benchmark_wall: self.console_workbench_benchmark_wall(),
            recall_inspector: self.console_workbench_recall_inspector(),
            facet_inspector: self.console_workbench_facet_inspector(),
            projection_inspector: self.console_workbench_projection_inspector(),
            procedural_evolution: self.console_workbench_procedural_evolution(),
            archive_restore: self.console_workbench_archive_restore(),
            soul_health: self.console_workbench_soul_health(),
        }
    }

    fn console_workbench_benchmark_wall(&self) -> EntryConsoleWorkbenchBenchmarkWall {
        #[cfg(not(feature = "nonproduction-replay-harness"))]
        {
            EntryConsoleWorkbenchBenchmarkWall {
                status: EntryConsoleWorkbenchStatus::limited("replay_harness_not_compiled"),
                fixture_root: "fixtures/memory-benchmark-wall".to_string(),
                report: None,
            }
        }

        #[cfg(feature = "nonproduction-replay-harness")]
        {
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
    }

    fn console_workbench_recall_inspector(&self) -> EntryConsoleWorkbenchRecallInspector {
        let query = "workbench memory inspection".to_string();
        match self.runtime.recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: query.clone(),
            limit: 6,
            tool_registry_refs: Vec::new(),
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
                    procedural_delivery_reports: report
                        .procedural_delivery_reports
                        .iter()
                        .filter(|delivery| delivery.selected)
                        .count(),
                    runtime_skill_selected: report.working.runtime_skill_report.selected_count,
                    working_selected_surfaces,
                    graph_nodes: report.graph_gate.nodes,
                    graph_edges: report.graph_gate.edges,
                    evidence_backlinks: report.graph_gate.evidence_backlinks,
                    high_confidence_projection_allowed: report
                        .graph_gate
                        .high_confidence_projection_allowed,
                    graph_failures: report.graph_gate.failures,
                    graph_selected_ids: report.graph_rerank.reranked_candidate_ids,
                    stale_false_positive_count: report.graph_rerank.stale_false_positive_count,
                    agent_tool_hints: report.agent_tool_hints.len(),
                    tool_experience_reason: report.tool_experience_status.reason,
                    host_fallback_required: report.tool_experience_status.host_fallback_required,
                }
            }
            Err(error) => EntryConsoleWorkbenchRecallInspector {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                query,
                procedural_delivery_reports: 0,
                runtime_skill_selected: 0,
                working_selected_surfaces: 0,
                graph_nodes: 0,
                graph_edges: 0,
                evidence_backlinks: 0,
                high_confidence_projection_allowed: false,
                graph_failures: Vec::new(),
                graph_selected_ids: Vec::new(),
                stale_false_positive_count: 0,
                agent_tool_hints: 0,
                tool_experience_reason: "recall_unavailable".to_string(),
                host_fallback_required: true,
            },
        }
    }

    fn console_workbench_facet_inspector(&self) -> EntryConsoleWorkbenchFacetInspector {
        let query = "workbench facet index inspection".to_string();
        match self.runtime.recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query,
            limit: 6,
            tool_registry_refs: Vec::new(),
        }) {
            Ok(report) => {
                let facet = report.facet_index_report;
                let direct_mutation_allowed = false;
                EntryConsoleWorkbenchFacetInspector {
                    status: EntryConsoleWorkbenchStatus::ready(
                        "sdk_recall_facet_index_report_available",
                    ),
                    owner: facet.owner.clone(),
                    used: facet.used,
                    report_only: true,
                    fallback_full_scan: facet.fallback_full_scan,
                    source_candidate_count: facet.source_candidate_count,
                    matched_source_candidate_count: facet.matched_source_candidate_count,
                    exact_facet_match_count: facet.exact_facet_match_count,
                    expanded_facet_match_count: facet.expanded_facet_match_count,
                    index_revision: facet.index_revision.clone(),
                    render_growth: facet.render_growth,
                    failures: facet.failures.clone(),
                    direct_mutation_allowed,
                    audit_markdown_format: FACET_AUDIT_MARKDOWN_FORMAT.to_string(),
                    audit_markdown_preview: facet_audit_markdown_preview(
                        &facet,
                        direct_mutation_allowed,
                    ),
                }
            }
            Err(error) => EntryConsoleWorkbenchFacetInspector {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                owner: "bm-sdk::MemoryRuntime".to_string(),
                used: false,
                report_only: true,
                fallback_full_scan: false,
                source_candidate_count: 0,
                matched_source_candidate_count: 0,
                exact_facet_match_count: 0,
                expanded_facet_match_count: 0,
                index_revision: None,
                render_growth: 0,
                failures: vec!["facet_index_report_unavailable".to_string()],
                direct_mutation_allowed: false,
                audit_markdown_format: FACET_AUDIT_MARKDOWN_FORMAT.to_string(),
                audit_markdown_preview: String::new(),
            },
        }
    }

    fn console_workbench_projection_inspector(&self) -> EntryConsoleWorkbenchProjectionInspector {
        let query = "Show operator-safe memory workbench context.".to_string();
        let runtime_budget = self.runtime_budget();
        match project_adapter_report(
            &self.runtime,
            MemoryProjectionRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: query.clone(),
                system_max_len: runtime_budget
                    .projection_render_budget
                    .system_block_max_chars,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: Vec::new(),
            },
        ) {
            Ok(report) => EntryConsoleWorkbenchProjectionInspector {
                status: EntryConsoleWorkbenchStatus::ready("sdk_projection_report_available"),
                query,
                system_memory_chars: report.chars,
                source_budget_chars: report.audit.source_budget_chars,
                render_budget_chars: report.audit.render_budget_chars,
                injected: report.audit.injected,
                truncated: report.audit.truncated,
                runtime_private_context_allowed: report.audit.runtime_private_context_allowed,
                foreground_disclosure_allowed: report.audit.foreground_disclosure_allowed,
                private_gate_reason: report.audit.private_gate_reason,
                evidence_refs: report.audit.evidence_ref_count,
                budget_decisions: report.audit.budget_decision_count,
                privacy_decisions: report.audit.privacy_decision_count,
                dropped_candidates: report.audit.dropped_candidate_count,
                faithfulness_passed: report.audit.faithfulness_passed,
                unsupported_claim_count: report.audit.unsupported_claim_count,
                disclosure_integrity_passed: report.audit.disclosure_integrity_passed,
                raw_private_violation_count: report.audit.raw_private_violation_count,
                agent_tool_hints: report.agent_tool_hints.len(),
                agent_tool_rejections: report.audit.agent_tool_rejection_count,
            },
            Err(error) => EntryConsoleWorkbenchProjectionInspector {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                query,
                system_memory_chars: 0,
                source_budget_chars: 0,
                render_budget_chars: 0,
                injected: false,
                truncated: false,
                runtime_private_context_allowed: false,
                foreground_disclosure_allowed: false,
                private_gate_reason: "projection_unavailable".to_string(),
                evidence_refs: 0,
                budget_decisions: 0,
                privacy_decisions: 0,
                dropped_candidates: 0,
                faithfulness_passed: false,
                unsupported_claim_count: 0,
                disclosure_integrity_passed: false,
                raw_private_violation_count: 0,
                agent_tool_hints: 0,
                agent_tool_rejections: 0,
            },
        }
    }

    fn console_workbench_procedural_evolution(&self) -> EntryConsoleWorkbenchProceduralEvolution {
        match self.runtime.list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: self.runtime_skill_subject_scope(),
            query: None,
            include_disabled: true,
            include_retired: true,
            limit: 8,
        }) {
            Ok(report) => EntryConsoleWorkbenchProceduralEvolution {
                status: EntryConsoleWorkbenchStatus::ready("sdk_skill_evolution_surface_available"),
                total_skills: report.total,
                active_skills: report.active,
                runtime_learned: report.runtime_skills,
                disabled: report.disabled,
                top_skills: report
                    .skills
                    .into_iter()
                    .take(5)
                    .map(|skill| EntryConsoleWorkbenchSkillRef {
                        locator: skill.locator,
                        owner_id: skill.owner_id,
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
                disabled: 0,
                top_skills: Vec::new(),
            },
        }
    }

    fn console_workbench_archive_restore(&self) -> EntryConsoleWorkbenchArchiveRestore {
        let scope =
            MemoryArchiveScope::subject(self.runtime.memory_space_id(), self.runtime.subject_id())
                .expect("validated runtime identity must form a Subject archive scope");
        let private_material_policy = MemorySpacePrivateMaterialPolicy::ExcludePrivate;
        match self.runtime.export_memory_space(MemorySpaceExportRequest {
            scope: scope.clone(),
            private_material_policy,
        }) {
            Ok(export) => EntryConsoleWorkbenchArchiveRestore {
                status: EntryConsoleWorkbenchStatus::ready("typed_archive_export_ready"),
                scope,
                private_material_policy,
                archive_root: Some(export.archive.root().clone()),
            },
            Err(error) => EntryConsoleWorkbenchArchiveRestore {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                scope,
                private_material_policy,
                archive_root: None,
            },
        }
    }

    fn console_workbench_soul_health(&self) -> EntryConsoleWorkbenchSoulHealth {
        let runtime_budget = self.runtime_budget();
        match self.runtime.inspect(MemoryInspectionRequest {
            query: "workbench soul health".to_string(),
            system_max_len: runtime_budget
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
                agent_tool_registries: report.agent_tool_registry.registries,
                agent_tool_registry_tools: report.agent_tool_registry.tools,
                agent_tool_experiences: report.agent_tool_registry.governed_experiences,
                agent_tool_stale_experiences: report.agent_tool_registry.stale_experiences,
            },
            Err(error) => EntryConsoleWorkbenchSoulHealth {
                status: EntryConsoleWorkbenchStatus::blocked(error.to_string()),
                profile: runtime_budget.profile.as_str().to_string(),
                hygiene_summary: "inspection_unavailable".to_string(),
                runtime_skill_records: 0,
                deferred_total: 0,
                deferred_pending: 0,
                deferred_failed: 0,
                safe_actions: Vec::new(),
                agent_tool_registries: 0,
                agent_tool_registry_tools: 0,
                agent_tool_experiences: 0,
                agent_tool_stale_experiences: 0,
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
        self.console_skills_in_scope(self.runtime_skill_subject_scope(), query)
    }

    pub fn console_skills_in_scope(
        &self,
        owning_scope: RuntimeSkillOwningScope,
        query: Option<String>,
    ) -> Result<EntryConsoleSkillList> {
        self.runtime
            .list_runtime_skills(RuntimeSkillListRequest {
                owning_scope,
                query,
                include_disabled: true,
                include_retired: true,
                limit: 512,
            })
            .map(Into::into)
    }

    pub fn console_skill_detail(
        &self,
        locator: RuntimeSkillOwnerLocator,
    ) -> Result<EntryConsoleSkillDetail> {
        self.runtime
            .get_runtime_skill(RuntimeSkillDetailRequest { locator })
            .map(Into::into)
    }

    pub fn console_edit_runtime_skill(
        &self,
        payload: EntryConsoleRuntimeSkillEdit,
    ) -> Result<EntryConsoleSkillMutation> {
        let report = self.runtime.edit_runtime_skill(RuntimeSkillEditRequest {
            locator: payload.locator,
            title: payload.title,
            topic: payload.topic,
            summary: payload.summary,
            procedure: payload.procedure,
            edit_reason: payload
                .edit_reason
                .unwrap_or_else(|| "operator_runtime_skill_edit".to_string()),
            observed_at: self.accepted_at(),
        })?;
        let mutation: EntryConsoleSkillMutation = report.into();
        if mutation.accepted {
            self.console
                .record_skill_mutation(&mutation.owner_id, "updated");
        }
        Ok(mutation)
    }

    pub fn console_set_skill_enabled(
        &self,
        payload: EntryConsoleSkillSetEnabled,
    ) -> Result<EntryConsoleSkillMutation> {
        let report = self
            .runtime
            .set_runtime_skill_enabled(RuntimeSkillSetEnabledRequest {
                locator: payload.locator,
                enabled: payload.enabled,
                observed_at: self.accepted_at(),
            })?;
        let mutation: EntryConsoleSkillMutation = report.into();
        if mutation.accepted {
            self.console.record_skill_mutation(
                &mutation.owner_id,
                if payload.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
            );
        }
        Ok(mutation)
    }

    pub fn console_retire_skill(
        &self,
        locator: RuntimeSkillOwnerLocator,
    ) -> Result<EntryConsoleSkillMutation> {
        let report = self
            .runtime
            .retire_runtime_skill(RuntimeSkillRetireRequest {
                locator,
                observed_at: self.accepted_at(),
            })?;
        let mutation: EntryConsoleSkillMutation = report.into();
        if mutation.accepted {
            self.console
                .record_skill_mutation(&mutation.owner_id, "retired");
        }
        Ok(mutation)
    }

    pub fn handle(
        &self,
        context: EntryTransportContext,
        command: AdapterCommand,
    ) -> Result<EntryResponse> {
        let lease = self.acquire_budget_lease()?;
        self.handle_with_budget_lease(context, command, &lease)
    }

    pub fn handle_with_services(
        &self,
        context: EntryTransportContext,
        command: AdapterCommand,
        services: AdapterRuntimeServices<'_>,
    ) -> Result<EntryResponse> {
        let lease = self.acquire_budget_lease()?;
        self.handle_with_budget_lease_and_services(context, command, services, &lease)
    }

    pub fn handle_with_budget_lease(
        &self,
        context: EntryTransportContext,
        command: AdapterCommand,
        lease: &EntryRuntimeBudgetLease,
    ) -> Result<EntryResponse> {
        self.handle_with_budget_lease_and_services(
            context,
            command,
            AdapterRuntimeServices::none(),
            lease,
        )
    }

    pub fn handle_with_budget_lease_and_services(
        &self,
        context: EntryTransportContext,
        command: AdapterCommand,
        services: AdapterRuntimeServices<'_>,
        lease: &EntryRuntimeBudgetLease,
    ) -> Result<EntryResponse> {
        self.execute_with_budget_lease(lease, || {
            self.handle_in_budget_lease(context, command, services, lease)
        })
    }

    fn handle_in_budget_lease(
        &self,
        context: EntryTransportContext,
        command: AdapterCommand,
        services: AdapterRuntimeServices<'_>,
        lease: &EntryRuntimeBudgetLease,
    ) -> Result<EntryResponse> {
        if context.operation() != command.operation() {
            return Ok(EntryResponse::from_adapter(
                AdapterResponse::Rejected {
                    request_id: context.request_id().to_string(),
                    audit_id: context.audit_id().to_string(),
                    error_key: AdapterErrorKey::OperationMismatch,
                    reason: "entry operation context does not match decoded command".to_string(),
                },
                lease.report().clone(),
            ));
        }
        if let Some(reason) = auth_rejection_reason(
            &self.config.auth,
            &self.config.identity.owner_id,
            context.auth(),
        ) {
            return Ok(EntryResponse::from_adapter(
                AdapterResponse::Rejected {
                    request_id: context.request_id().to_string(),
                    audit_id: context.audit_id().to_string(),
                    error_key: AdapterErrorKey::Unauthorized,
                    reason,
                },
                lease.report().clone(),
            ));
        }
        let required_capability =
            crate::EntryOperationCapability::for_adapter_operation(command.operation());
        if !context.auth().allows(required_capability) {
            return Ok(EntryResponse::from_adapter(
                AdapterResponse::Rejected {
                    request_id: context.request_id().to_string(),
                    audit_id: context.audit_id().to_string(),
                    error_key: AdapterErrorKey::Forbidden,
                    reason: format!(
                        "entry principal lacks required operation capability: {}",
                        required_capability.as_str()
                    ),
                },
                lease.report().clone(),
            ));
        }
        let idempotency_reservation = if command.operation().requires_idempotency() {
            let fingerprint_material = command
                .idempotency_fingerprint_material()
                .map_err(|error| bm_sdk::Error::config("entry_idempotency", error.to_string()))?;
            let digest = format!("{:x}", Sha256::digest(&fingerprint_material));
            match self.idempotency.reserve(context.idempotency_key(), &digest) {
                crate::idempotency::EntryIdempotencyReservationOutcome::Reserved(reservation) => {
                    Some(reservation)
                }
                crate::idempotency::EntryIdempotencyReservationOutcome::DuplicateCommitted => {
                    return Ok(EntryResponse::from_adapter(
                        AdapterResponse::Duplicated {
                            request_id: context.request_id().to_string(),
                            audit_id: context.audit_id().to_string(),
                            idempotency_key: context.idempotency_key().to_string(),
                        },
                        lease.report().clone(),
                    ));
                }
                crate::idempotency::EntryIdempotencyReservationOutcome::InFlight => {
                    return Ok(EntryResponse::from_adapter(
                        AdapterResponse::Rejected {
                            request_id: context.request_id().to_string(),
                            audit_id: context.audit_id().to_string(),
                            error_key: AdapterErrorKey::Duplicated,
                            reason: "idempotency key is reserved by an in-flight mutation"
                                .to_string(),
                        },
                        lease.report().clone(),
                    ));
                }
                crate::idempotency::EntryIdempotencyReservationOutcome::Conflict => {
                    return Ok(EntryResponse::from_adapter(
                        AdapterResponse::Rejected {
                            request_id: context.request_id().to_string(),
                            audit_id: context.audit_id().to_string(),
                            error_key: AdapterErrorKey::Duplicated,
                            reason:
                                "idempotency key is reserved or committed for a different payload"
                                    .to_string(),
                        },
                        lease.report().clone(),
                    ));
                }
                crate::idempotency::EntryIdempotencyReservationOutcome::CapacityExhausted => {
                    return Ok(EntryResponse::from_adapter(
                        AdapterResponse::Rejected {
                            request_id: context.request_id().to_string(),
                            audit_id: context.audit_id().to_string(),
                            error_key: AdapterErrorKey::RuntimeRejected,
                            reason: "idempotency reservation capacity is exhausted by in-flight mutations"
                                .to_string(),
                        },
                        lease.report().clone(),
                    ));
                }
            }
        } else {
            None
        };

        let operation = command.operation();
        let source = context.source(&self.config.identity, &self.config.scope);
        let context = context.into_parts();
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
        let response =
            dispatch_adapter_command_with_services(&self.runtime, &lease.inner, envelope, services)
                .map(|adapter| EntryResponse::from_adapter(adapter, lease.report().clone()))?;
        if matches!(
            &response.adapter,
            AdapterResponse::Accepted { .. } | AdapterResponse::Queued { .. }
        ) {
            if let Some(reservation) = idempotency_reservation {
                reservation.commit();
            }
        }
        self.console
            .record_adapter_response(operation, &response.adapter);
        Ok(response)
    }
}

fn agent_skill_dirs_from_env() -> Vec<AgentSkillDirConfig> {
    std::env::var_os("BM_AGENT_SKILL_DIRS")
        .map(|value| {
            std::env::split_paths(&value)
                .enumerate()
                .map(|(index, path)| {
                    AgentSkillDirConfig::read_only(path, format!("env_agent_skill_{index}"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn facet_audit_markdown_preview(
    report: &MemoryFacetRecallIndexReport,
    direct_mutation_allowed: bool,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "---");
    let _ = writeln!(out, "beetle_view: facet_index_audit");
    let _ = writeln!(out, "format: {FACET_AUDIT_MARKDOWN_FORMAT}");
    let _ = writeln!(out, "report_only: true");
    let _ = writeln!(out, "direct_mutation_allowed: {direct_mutation_allowed}");
    let _ = writeln!(out, "hot_path: false");
    let _ = writeln!(out, "---");
    let _ = writeln!(out);
    let _ = writeln!(out, "# Facet Index Audit");
    let _ = writeln!(out);
    let _ = writeln!(out, "- Owner: {}", report.owner);
    let _ = writeln!(out, "- Used: {}", report.used);
    let _ = writeln!(out, "- Fallback full scan: {}", report.fallback_full_scan);
    let _ = writeln!(
        out,
        "- Source candidates: {}",
        report.source_candidate_count
    );
    let _ = writeln!(
        out,
        "- Matched source candidates: {}",
        report.matched_source_candidate_count
    );
    let _ = writeln!(
        out,
        "- Exact facet matches: {}",
        report.exact_facet_match_count
    );
    let _ = writeln!(
        out,
        "- Expanded facet matches: {}",
        report.expanded_facet_match_count
    );
    let _ = writeln!(
        out,
        "- Index revision: {}",
        report.index_revision.as_deref().unwrap_or("none")
    );
    let _ = writeln!(out, "- Render growth: {}", report.render_growth);
    if report.failures.is_empty() {
        let _ = writeln!(out, "- Failures: none");
    } else {
        let _ = writeln!(out, "- Failures:");
        for failure in &report.failures {
            let _ = writeln!(out, "  - {failure}");
        }
    }
    out
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

fn auth_rejection_reason(
    auth_config: &EntryAuthConfig,
    expected_owner_id: &str,
    decision: &crate::EntryAuthDecision,
) -> Option<String> {
    if !auth_config.requires_auth() {
        return None;
    }
    if decision.is_loopback() {
        return Some("entry auth rejected loopback for remote/auth-required profile".to_string());
    }
    if !decision.is_authenticated() {
        return Some(format!(
            "entry auth rejected request: {}",
            decision.rejection_reason().unwrap_or("unauthenticated")
        ));
    }
    let Some(principal) = decision.bearer_principal() else {
        return Some(
            "entry auth rejected request: configured bearer principal is required".to_string(),
        );
    };
    if principal.owner_id() != expected_owner_id.trim() {
        return Some("entry auth rejected request: bearer owner binding mismatch".to_string());
    }
    None
}

#[cfg(test)]
mod runtime_cache_budget_tests {
    use super::effective_runtime_cache_limit;

    #[test]
    fn cache_limit_tracks_current_report_and_never_preserves_a_larger_open_time_limit() {
        assert_eq!(effective_runtime_cache_limit(None, 8), 8);
        assert_eq!(effective_runtime_cache_limit(None, 2), 2);
        assert_eq!(effective_runtime_cache_limit(Some(6), 8), 6);
        assert_eq!(effective_runtime_cache_limit(Some(6), 2), 2);
        assert_eq!(effective_runtime_cache_limit(Some(6), 0), 1);
    }
}
