use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bm_adapter::{AdapterOperation, AdapterResponse, AdapterSdkReport};
#[cfg(feature = "nonproduction-replay-harness")]
use bm_replay::{MemoryBenchmarkMode, MemoryBenchmarkReport};
use bm_sdk::{
    DeferredGovernanceQueueReport, MemoryStoreTelemetryReport, RuntimeBudgetReport,
    RuntimeSkillDetailReport, RuntimeSkillListReport, RuntimeSkillMutationReport,
    RuntimeSkillSummary, StoreBackendKind, WorkbenchApiMap,
};
use serde::{Deserialize, Serialize};

use crate::{EntryRuntimeConfig, EntryTransportConfig};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleRuntimeShape {
    pub profile: String,
    pub name: String,
    pub store: String,
    pub shell: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleSystemInfo {
    pub name: String,
    pub cpu: String,
    pub memory: String,
    pub time_unix_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleMetric {
    pub value: String,
    pub desc: String,
    pub progress: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleEvent {
    pub time: String,
    pub text: String,
    pub tone: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleCapabilityRow {
    pub title: String,
    pub status: String,
    pub desc: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleKv {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleOverview {
    pub runtime_shape: EntryConsoleRuntimeShape,
    pub system_info: EntryConsoleSystemInfo,
    pub runtime_budget: EntryConsoleRuntimeBudget,
    pub storage: EntryConsoleMetric,
    pub writes_today: EntryConsoleMetric,
    pub recall: EntryConsoleMetric,
    pub projection: EntryConsoleMetric,
    pub deferred_governance: DeferredGovernanceQueueReport,
    pub devices: EntryConsoleMetric,
    pub recent_events: Vec<EntryConsoleEvent>,
    pub capabilities: Vec<EntryConsoleCapabilityRow>,
    pub kernel: Vec<EntryConsoleKv>,
    pub session: Vec<EntryConsoleKv>,
    pub memory_context: Vec<EntryConsoleKv>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleRuntimeBudget {
    pub report_id: String,
    pub profile: String,
    pub deployment_target: String,
    pub deployment_role: String,
    pub store_medium: String,
    pub resource_source: String,
    pub stale: bool,
    pub limited_by: Vec<String>,
    pub unavailable_reasons: Vec<String>,
    pub store_snapshot_max_bytes: usize,
    pub http_body_max_bytes: usize,
    pub wss_frame_max_bytes: usize,
    pub projection_source_max_chars: usize,
    pub projection_render_max_chars: usize,
    pub maintenance_user_max_chars: usize,
    pub maintenance_reply_max_chars: usize,
}

impl EntryConsoleRuntimeBudget {
    fn from_report(report: &RuntimeBudgetReport) -> Self {
        Self {
            report_id: report.report_id.clone(),
            profile: report.profile.as_str().to_string(),
            deployment_target: report.deployment_target.as_str().to_string(),
            deployment_role: report.deployment_role.as_str().to_string(),
            store_medium: report.store_medium.as_str().to_string(),
            resource_source: report.resource_snapshot.source.as_str().to_string(),
            stale: report.resource_snapshot.stale,
            limited_by: report.limited_by.clone(),
            unavailable_reasons: report.unavailable_reasons.clone(),
            store_snapshot_max_bytes: report.store_budget.snapshot_max_bytes,
            http_body_max_bytes: report.adapter_budget.http_body_max_bytes,
            wss_frame_max_bytes: report.adapter_budget.wss_frame_max_bytes,
            projection_source_max_chars: report.projection_source_budget.context_assembly_max_chars,
            projection_render_max_chars: report.projection_render_budget.system_block_max_chars,
            maintenance_user_max_chars: report.maintenance_budget.user_input_max_chars,
            maintenance_reply_max_chars: report.maintenance_budget.reply_input_max_chars,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleWorkbenchStatus {
    pub available: bool,
    pub status: String,
    pub reason: String,
}

impl EntryConsoleWorkbenchStatus {
    pub fn ready(reason: impl Into<String>) -> Self {
        Self {
            available: true,
            status: "ready".to_string(),
            reason: reason.into(),
        }
    }

    pub fn limited(reason: impl Into<String>) -> Self {
        Self {
            available: true,
            status: "limited".to_string(),
            reason: reason.into(),
        }
    }

    pub fn blocked(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            status: "blocked".to_string(),
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleWorkbenchReport {
    pub api_map: WorkbenchApiMap,
    pub benchmark_wall: EntryConsoleWorkbenchBenchmarkWall,
    pub recall_inspector: EntryConsoleWorkbenchRecallInspector,
    pub facet_inspector: EntryConsoleWorkbenchFacetInspector,
    pub projection_inspector: EntryConsoleWorkbenchProjectionInspector,
    pub procedural_evolution: EntryConsoleWorkbenchProceduralEvolution,
    pub vault_migration: EntryConsoleWorkbenchVaultMigration,
    pub soul_health: EntryConsoleWorkbenchSoulHealth,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleWorkbenchBenchmarkWall {
    pub status: EntryConsoleWorkbenchStatus,
    pub fixture_root: String,
    pub report: Option<EntryConsoleMemoryBenchmarkReport>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleMemoryBenchmarkReport {
    pub suite: String,
    pub total_fixtures: usize,
    pub passed_fixtures: usize,
    pub baseline: EntryConsoleMemoryBenchmarkBaseline,
    pub class_coverage: Vec<EntryConsoleMemoryBenchmarkClassCoverage>,
    pub missing_classes: Vec<EntryConsoleMemoryBenchmarkMissingClass>,
    pub soul_kernel_judge: EntryConsoleMemoryBenchmarkPhaseJudge,
    pub subject_projection_judge: EntryConsoleMemoryBenchmarkPhaseJudge,
    pub agent_tool_experience_judge: EntryConsoleMemoryBenchmarkPhaseJudge,
    pub failures: Vec<EntryConsoleMemoryBenchmarkFailure>,
    pub passed: bool,
}

impl EntryConsoleMemoryBenchmarkReport {
    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn from_report(report: MemoryBenchmarkReport) -> Self {
        Self {
            suite: report.suite,
            total_fixtures: report.total_fixtures,
            passed_fixtures: report.passed_fixtures,
            baseline: EntryConsoleMemoryBenchmarkBaseline {
                accuracy_bps: report.baseline.accuracy_bps,
                evidence_precision_bps: report.baseline.evidence_precision_bps,
                projection_faithfulness_bps: report.baseline.projection_faithfulness_bps,
                privacy_violation_count: report.baseline.privacy_violation_count,
                stale_memory_false_positive_count: report
                    .baseline
                    .stale_memory_false_positive_count,
                procedural_reuse_success_bps: report.baseline.procedural_reuse_success_bps,
                soul_regression_count: report.baseline.soul_regression_count,
                latency_ms: report.baseline.latency_ms,
                token_budget: report.baseline.token_budget,
                memory_bytes: report.baseline.memory_bytes,
            },
            class_coverage: report
                .class_coverage
                .into_iter()
                .map(|coverage| EntryConsoleMemoryBenchmarkClassCoverage {
                    class: coverage.class.as_str().to_string(),
                    compact_fixtures: coverage.compact_fixtures,
                    full_fixtures: coverage.full_fixtures,
                })
                .collect(),
            missing_classes: report
                .missing_classes
                .into_iter()
                .map(|missing| EntryConsoleMemoryBenchmarkMissingClass {
                    class: missing.class.as_str().to_string(),
                    mode: memory_benchmark_mode(missing.mode).to_string(),
                })
                .collect(),
            soul_kernel_judge: EntryConsoleMemoryBenchmarkPhaseJudge {
                release_gate_passed: report.soul_kernel_judge.release_gate_passed,
                fixture_ids: report.soul_kernel_judge.fixture_ids,
                blocked_reasons: report.soul_kernel_judge.blocked_reasons,
            },
            subject_projection_judge: EntryConsoleMemoryBenchmarkPhaseJudge {
                release_gate_passed: report.subject_projection_judge.release_gate_passed,
                fixture_ids: report.subject_projection_judge.fixture_ids,
                blocked_reasons: report.subject_projection_judge.blocked_reasons,
            },
            agent_tool_experience_judge: EntryConsoleMemoryBenchmarkPhaseJudge {
                release_gate_passed: report.agent_tool_experience_judge.release_gate_passed,
                fixture_ids: report.agent_tool_experience_judge.fixture_ids,
                blocked_reasons: report.agent_tool_experience_judge.blocked_reasons,
            },
            failures: report
                .failures
                .into_iter()
                .map(|failure| EntryConsoleMemoryBenchmarkFailure {
                    fixture_id: failure.fixture_id,
                    class: failure.class.as_str().to_string(),
                    mode: memory_benchmark_mode(failure.mode).to_string(),
                    profile: failure.profile.as_str().to_string(),
                    stage: failure.stage,
                    reason: failure.reason,
                })
                .collect(),
            passed: report.passed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleMemoryBenchmarkPhaseJudge {
    pub release_gate_passed: bool,
    pub fixture_ids: Vec<String>,
    pub blocked_reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleMemoryBenchmarkBaseline {
    pub accuracy_bps: u16,
    pub evidence_precision_bps: u16,
    pub projection_faithfulness_bps: u16,
    pub privacy_violation_count: u32,
    pub stale_memory_false_positive_count: u32,
    pub procedural_reuse_success_bps: u16,
    pub soul_regression_count: u32,
    pub latency_ms: u32,
    pub token_budget: u32,
    pub memory_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleMemoryBenchmarkClassCoverage {
    pub class: String,
    pub compact_fixtures: usize,
    pub full_fixtures: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleMemoryBenchmarkMissingClass {
    pub class: String,
    pub mode: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleMemoryBenchmarkFailure {
    pub fixture_id: String,
    pub class: String,
    pub mode: String,
    pub profile: String,
    pub stage: String,
    pub reason: String,
}

#[cfg(feature = "nonproduction-replay-harness")]
const fn memory_benchmark_mode(mode: MemoryBenchmarkMode) -> &'static str {
    match mode {
        MemoryBenchmarkMode::Compact => "compact",
        MemoryBenchmarkMode::Full => "full",
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleWorkbenchRecallInspector {
    pub status: EntryConsoleWorkbenchStatus,
    pub query: String,
    pub procedural_hits: usize,
    pub runtime_skill_selected: usize,
    pub working_selected_surfaces: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub evidence_backlinks: usize,
    pub high_confidence_projection_allowed: bool,
    pub graph_failures: Vec<String>,
    pub graph_selected_ids: Vec<String>,
    pub stale_false_positive_count: u32,
    pub agent_tool_hints: usize,
    pub tool_experience_reason: String,
    pub host_fallback_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleWorkbenchFacetInspector {
    pub status: EntryConsoleWorkbenchStatus,
    pub owner: String,
    pub used: bool,
    pub report_only: bool,
    pub fallback_full_scan: bool,
    pub source_candidate_count: usize,
    pub matched_source_candidate_count: usize,
    pub exact_facet_match_count: usize,
    pub expanded_facet_match_count: usize,
    pub index_revision: Option<String>,
    pub render_growth: usize,
    pub failures: Vec<String>,
    pub direct_mutation_allowed: bool,
    pub audit_markdown_format: String,
    pub audit_markdown_preview: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleWorkbenchProjectionInspector {
    pub status: EntryConsoleWorkbenchStatus,
    pub query: String,
    pub system_memory_chars: usize,
    pub source_budget_chars: usize,
    pub render_budget_chars: usize,
    pub injected: bool,
    pub truncated: bool,
    pub runtime_private_context_allowed: bool,
    pub foreground_disclosure_allowed: bool,
    pub private_gate_reason: String,
    pub evidence_refs: usize,
    pub budget_decisions: usize,
    pub privacy_decisions: usize,
    pub dropped_candidates: usize,
    pub faithfulness_passed: bool,
    pub unsupported_claim_count: usize,
    pub disclosure_integrity_passed: bool,
    pub raw_private_violation_count: u32,
    pub agent_tool_hints: usize,
    pub agent_tool_rejections: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleWorkbenchProceduralEvolution {
    pub status: EntryConsoleWorkbenchStatus,
    pub total_skills: usize,
    pub active_skills: usize,
    pub runtime_learned: usize,
    pub disabled: usize,
    pub top_skills: Vec<EntryConsoleWorkbenchSkillRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleWorkbenchSkillRef {
    pub name: String,
    pub title: String,
    pub topic: String,
    pub status: String,
    pub quality_score: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleWorkbenchVaultMigration {
    pub status: EntryConsoleWorkbenchStatus,
    pub source_memory_space_id: String,
    pub target_memory_space_id: String,
    pub json_docs: usize,
    pub blobs: usize,
    pub events: usize,
    pub privacy_redactions: usize,
    pub loss_risk: bool,
    pub preflight_passed: bool,
    pub preflight_failures: Vec<String>,
    pub snapshot_fingerprint: String,
    pub event_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleWorkbenchSoulHealth {
    pub status: EntryConsoleWorkbenchStatus,
    pub profile: String,
    pub hygiene_summary: String,
    pub runtime_skill_records: usize,
    pub deferred_total: usize,
    pub deferred_pending: usize,
    pub deferred_failed: usize,
    pub safe_actions: Vec<String>,
    pub agent_tool_registries: usize,
    pub agent_tool_registry_tools: usize,
    pub agent_tool_experiences: usize,
    pub agent_tool_stale_experiences: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleTransport {
    pub id: String,
    pub enabled: bool,
    pub status: String,
    pub endpoint: String,
    pub editable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleLlmGateway {
    pub enabled: bool,
    pub status: String,
    pub endpoint: String,
    pub openai_base_url: String,
    pub ollama_base_url: String,
    pub provider_capabilities_url: String,
    pub mcp_streamable_http_url: String,
    pub protocols: Vec<EntryConsoleLlmGatewayProtocol>,
    pub rule_exports: Vec<EntryConsoleLlmGatewayRuleExport>,
    pub smoke_checks: Vec<EntryConsoleLlmGatewaySmokeCheck>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleLlmGatewayProtocol {
    pub id: String,
    pub title: String,
    pub status: String,
    pub endpoint: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleLlmGatewayRuleExport {
    pub target: String,
    pub label: String,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleLlmGatewaySmokeCheck {
    pub id: String,
    pub label: String,
    pub status: String,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleLlmGatewaySmokeRunReport {
    pub id: String,
    pub label: String,
    pub status: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub started_at_unix_secs: u64,
    pub cwd: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleDevice {
    pub device_id: String,
    pub label: String,
    pub app_key_fingerprint: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleSession {
    pub account: String,
    pub owner: String,
    pub memory_scope: String,
    pub session_state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleDeviceKeyReport {
    pub device: EntryConsoleDevice,
    pub app_key_once: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleSkillSummary {
    pub name: String,
    pub title: String,
    pub topic: String,
    pub status: String,
    pub enabled: bool,
    pub quality_score: Option<u8>,
    pub use_count: u32,
    pub validated_success_count: u32,
    pub mismatch_count: u32,
    pub revision_pending: bool,
    pub updated_at: u64,
    pub last_used_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleSkillList {
    pub total: usize,
    pub active: usize,
    pub disabled: usize,
    pub runtime_learned: usize,
    pub skills: Vec<EntryConsoleSkillSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleSkillDetail {
    pub summary: EntryConsoleSkillSummary,
    pub summary_text: String,
    pub procedure_text: String,
    pub raw_content: String,
    pub citations: Vec<String>,
    pub source_chat_id: Option<String>,
    pub lineage: Vec<String>,
    pub strategy_diffs: Vec<String>,
    pub last_outcome_note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleRuntimeSkillEdit {
    pub title: String,
    pub topic: String,
    pub summary: String,
    pub procedure: String,
    #[serde(default)]
    pub citations: Vec<String>,
    #[serde(default)]
    pub source_chat_id: Option<String>,
    #[serde(default)]
    pub edit_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleSkillSetEnabled {
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleSkillMutation {
    pub accepted: bool,
    pub changed: bool,
    pub name: String,
    pub operation: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleTransportUpdate {
    pub enabled: Option<bool>,
    pub endpoint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleDeviceCreate {
    pub device_id: Option<String>,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryConsoleDeviceUpdate {
    pub label: Option<String>,
    pub status: Option<String>,
}

pub struct EntryConsoleState {
    inner: Mutex<EntryConsoleInner>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntryConsoleTelemetrySnapshot {
    pub writes_today: u64,
    pub recall_requests: u64,
    pub recall_hits: u64,
    pub projection_requests: u64,
    pub last_projection_chars: usize,
}

impl EntryConsoleTelemetrySnapshot {
    pub fn from_store_telemetry(report: MemoryStoreTelemetryReport) -> Self {
        Self {
            writes_today: report.writes_since,
            recall_requests: report.recall_requests,
            recall_hits: report.recall_hits,
            projection_requests: report.projection_requests,
            last_projection_chars: report.last_projection_chars,
        }
    }

    pub fn merge(&mut self, other: Self) {
        self.writes_today = self.writes_today.saturating_add(other.writes_today);
        self.recall_requests = self.recall_requests.saturating_add(other.recall_requests);
        self.recall_hits = self.recall_hits.saturating_add(other.recall_hits);
        self.projection_requests = self
            .projection_requests
            .saturating_add(other.projection_requests);
        if other.last_projection_chars > 0 {
            self.last_projection_chars = other.last_projection_chars;
        }
    }
}

#[derive(Clone, Debug)]
struct EntryConsoleInner {
    runtime_shape: EntryConsoleRuntimeShape,
    storage_path: Option<PathBuf>,
    agent_id: String,
    channel: String,
    transports: Vec<EntryConsoleTransport>,
    devices: Vec<EntryConsoleDevice>,
    session: EntryConsoleSession,
    writes_today: u64,
    recall_requests: u64,
    recall_hits: u64,
    projection_requests: u64,
    last_projection_chars: usize,
    events: Vec<EntryConsoleEvent>,
    api_key_counter: u64,
}

impl EntryConsoleState {
    pub fn new(config: &EntryRuntimeConfig, runtime_budget: &RuntimeBudgetReport) -> Self {
        Self {
            inner: Mutex::new(EntryConsoleInner {
                runtime_shape: runtime_shape(config, runtime_budget),
                storage_path: config.store.data_path().map(Path::to_path_buf),
                agent_id: config.identity.agent_id.clone(),
                channel: config.scope.channel.clone(),
                transports: transports(&config.transports),
                devices: default_devices(config),
                session: EntryConsoleSession {
                    account: "operator".to_string(),
                    owner: config.identity.owner_id.clone(),
                    memory_scope: config.scope.chat_id.clone(),
                    session_state: "paired".to_string(),
                },
                writes_today: 0,
                recall_requests: 0,
                recall_hits: 0,
                projection_requests: 0,
                last_projection_chars: 0,
                events: vec![EntryConsoleEvent {
                    time: "boot".to_string(),
                    text: "Console runtime opened".to_string(),
                    tone: "ready".to_string(),
                }],
                api_key_counter: 1,
            }),
        }
    }

    pub fn overview_with_telemetry_and_budget(
        &self,
        telemetry: EntryConsoleTelemetrySnapshot,
        runtime_budget: &RuntimeBudgetReport,
        deferred_governance: DeferredGovernanceQueueReport,
    ) -> EntryConsoleOverview {
        let inner = self.inner.lock().expect("console state lock");
        let active_devices = inner
            .devices
            .iter()
            .filter(|device| device.status != "disabled")
            .count();
        let enabled_transports = inner
            .transports
            .iter()
            .filter(|transport| transport.enabled)
            .count();
        let writes_today = inner.writes_today.max(telemetry.writes_today);
        let recall_requests = inner.recall_requests.max(telemetry.recall_requests);
        let recall_hits = inner.recall_hits.max(telemetry.recall_hits);
        let projection_requests = inner.projection_requests.max(telemetry.projection_requests);
        let last_projection_chars = if telemetry.last_projection_chars > 0 {
            telemetry.last_projection_chars
        } else {
            inner.last_projection_chars
        };
        let recall_rate = percentage_value(recall_hits, recall_requests);
        EntryConsoleOverview {
            runtime_shape: inner.runtime_shape.clone(),
            system_info: system_info(runtime_budget),
            runtime_budget: EntryConsoleRuntimeBudget::from_report(runtime_budget),
            storage: storage_metric(runtime_budget, inner.storage_path.as_deref()),
            writes_today: EntryConsoleMetric {
                value: writes_today.to_string(),
                desc: "Accepted memory writes recorded by the runtime event stream".to_string(),
                progress: None,
            },
            recall: EntryConsoleMetric {
                value: format!("{recall_rate:.1}%"),
                desc: format!(
                    "{} recall requests / {} with hits",
                    recall_requests, recall_hits
                ),
                progress: Some(recall_rate),
            },
            projection: EntryConsoleMetric {
                value: if projection_requests == 0 {
                    "0".to_string()
                } else {
                    format!("{last_projection_chars} characters")
                },
                desc: format!(
                    "{projection_requests} conversations received memory context / current limit {} characters",
                    runtime_budget
                        .projection_render_budget
                        .system_block_max_chars
                ),
                progress: None,
            },
            deferred_governance,
            devices: EntryConsoleMetric {
                value: format!("{active_devices}/{}", inner.devices.len()),
                desc: "Allowed device access state".to_string(),
                progress: percentage(active_devices, inner.devices.len()),
            },
            recent_events: recent_events(&inner, enabled_transports),
            capabilities: vec![
                EntryConsoleCapabilityRow {
                    title: "Write governance".to_string(),
                    status: "ready".to_string(),
                    desc: "All writes go through the unified memory runtime".to_string(),
                },
                EntryConsoleCapabilityRow {
                    title: "Soul and subject memory".to_string(),
                    status: "ready".to_string(),
                    desc: "Projection and subject memory are active".to_string(),
                },
                EntryConsoleCapabilityRow {
                    title: "Device allowlist".to_string(),
                    status: "ready".to_string(),
                    desc: format!("{} devices configured", inner.devices.len()),
                },
            ],
            kernel: vec![
                EntryConsoleKv {
                    label: "Profile".to_string(),
                    value: inner.runtime_shape.profile.clone(),
                },
                EntryConsoleKv {
                    label: "Store backend".to_string(),
                    value: inner.runtime_shape.store.clone(),
                },
                EntryConsoleKv {
                    label: "Console shell".to_string(),
                    value: inner.runtime_shape.shell.clone(),
                },
            ],
            session: vec![
                EntryConsoleKv {
                    label: "Account".to_string(),
                    value: inner.session.account.clone(),
                },
                EntryConsoleKv {
                    label: "Owner".to_string(),
                    value: inner.session.owner.clone(),
                },
                EntryConsoleKv {
                    label: "Memory scope".to_string(),
                    value: inner.session.memory_scope.clone(),
                },
                EntryConsoleKv {
                    label: "Session state".to_string(),
                    value: inner.session.session_state.clone(),
                },
            ],
            memory_context: memory_context_rows(&inner),
        }
    }

    pub fn transports(&self) -> Vec<EntryConsoleTransport> {
        self.inner
            .lock()
            .expect("console state lock")
            .transports
            .clone()
    }

    pub fn llm_gateway(&self) -> EntryConsoleLlmGateway {
        let inner = self.inner.lock().expect("console state lock");
        let gateway = inner
            .transports
            .iter()
            .find(|transport| transport.id == "llm-gateway")
            .cloned()
            .unwrap_or_else(|| transport("llm-gateway", false, "127.0.0.1:8787"));
        let mcp = inner
            .transports
            .iter()
            .find(|transport| transport.id == "mcp")
            .cloned()
            .unwrap_or_else(|| transport("mcp", false, "stdio"));
        let base_url = http_base_url(&gateway.endpoint, "127.0.0.1:8787");
        let openai_base_url = join_url(&base_url, "v1");
        let ollama_base_url = join_url(&base_url, "api");
        let provider_capabilities_url = join_url(&openai_base_url, "bm/provider-capabilities");
        let mcp_streamable_http_url = mcp_streamable_http_url(&mcp.endpoint);
        let gateway_status = if gateway.enabled { "ready" } else { "draft" }.to_string();
        let mcp_status = if mcp.enabled { "ready" } else { "draft" }.to_string();

        EntryConsoleLlmGateway {
            enabled: gateway.enabled,
            status: gateway_status.clone(),
            endpoint: gateway.endpoint,
            openai_base_url: openai_base_url.clone(),
            ollama_base_url: ollama_base_url.clone(),
            provider_capabilities_url: provider_capabilities_url.clone(),
            mcp_streamable_http_url: mcp_streamable_http_url.clone(),
            protocols: vec![
                llm_protocol(
                    "openai-compatible",
                    "OpenAI-compatible",
                    &gateway_status,
                    &openai_base_url,
                    "Models, chat completions, responses, embeddings, and provider capability report",
                ),
                llm_protocol(
                    "ollama-native",
                    "Ollama native",
                    &gateway_status,
                    &ollama_base_url,
                    "Native tags, version, chat, generate, embeddings, and show passthrough",
                ),
                llm_protocol(
                    "mcp-streamable-http",
                    "MCP Streamable HTTP",
                    &mcp_status,
                    &mcp_streamable_http_url,
                    "Explicit recall, projection preview, inspection, and governed write candidates",
                ),
            ],
            rule_exports: rule_exports(&openai_base_url, &mcp_streamable_http_url),
            smoke_checks: vec![
                smoke_check(
                    "provider-capabilities",
                    "Provider capabilities",
                    &gateway_status,
                    format!("curl -fsS {provider_capabilities_url}"),
                ),
                smoke_check(
                    "release-integrations",
                    "Release integration gate",
                    "ready",
                    "bash scripts/check_llm_gateway_release_integrations.sh".to_string(),
                ),
                smoke_check(
                    "ollama-native",
                    "Ollama native live smoke",
                    "draft",
                    "BM_LLM_GATEWAY_OLLAMA_SMOKE=1 bash scripts/check_llm_gateway_release_integrations.sh".to_string(),
                ),
            ],
        }
    }

    pub fn run_llm_gateway_smoke_check(
        &self,
        id: &str,
    ) -> Option<EntryConsoleLlmGatewaySmokeRunReport> {
        let spec = {
            let inner = self.inner.lock().expect("console state lock");
            llm_gateway_smoke_command(&inner, id)?
        };
        let report = run_console_smoke_command(spec);
        let mut inner = self.inner.lock().expect("console state lock");
        push_event(
            &mut inner,
            format!("LLM Gateway smoke {} {}", report.id, report.status),
            report.status.as_str(),
        );
        Some(report)
    }

    pub fn update_transport(
        &self,
        id: &str,
        update: EntryConsoleTransportUpdate,
    ) -> Option<EntryConsoleTransport> {
        let mut inner = self.inner.lock().expect("console state lock");
        let updated = {
            let transport = inner.transports.iter_mut().find(|item| item.id == id)?;
            if transport.editable {
                if let Some(enabled) = update.enabled {
                    transport.enabled = enabled;
                }
                if let Some(endpoint) = update.endpoint {
                    transport.endpoint = endpoint.trim().to_string();
                }
            }
            transport.status = if transport.enabled { "ready" } else { "draft" }.to_string();
            transport.clone()
        };
        push_event(
            &mut inner,
            format!("Transport {} updated", updated.id),
            if updated.enabled { "ready" } else { "limited" },
        );
        Some(updated)
    }

    pub fn devices(&self) -> Vec<EntryConsoleDevice> {
        self.inner
            .lock()
            .expect("console state lock")
            .devices
            .clone()
    }

    pub fn add_device(
        &self,
        request: EntryConsoleDeviceCreate,
    ) -> Result<EntryConsoleDeviceKeyReport, &'static str> {
        let mut inner = self.inner.lock().expect("console state lock");
        let label = request.label.trim();
        if label.is_empty() {
            return Err("device label is required");
        }
        let device_id = request
            .device_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("bm-device-{}", inner.api_key_counter));
        if inner
            .devices
            .iter()
            .any(|device| device.device_id == device_id)
        {
            return Err("device id already exists");
        }
        let app_key_once = issue_app_key(&mut inner);
        let device = EntryConsoleDevice {
            device_id,
            label: label.to_string(),
            app_key_fingerprint: fingerprint(&app_key_once),
            status: "allowed".to_string(),
        };
        inner.devices.push(device.clone());
        push_event(
            &mut inner,
            format!("Device {} added", device.device_id),
            "ready",
        );
        Ok(EntryConsoleDeviceKeyReport {
            device,
            app_key_once,
        })
    }

    pub fn update_device(
        &self,
        device_id: &str,
        update: EntryConsoleDeviceUpdate,
    ) -> Option<EntryConsoleDevice> {
        let mut inner = self.inner.lock().expect("console state lock");
        let updated = {
            let device = inner
                .devices
                .iter_mut()
                .find(|device| device.device_id == device_id)?;
            if let Some(label) = update
                .label
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                device.label = label.to_string();
            }
            if let Some(status) = update.status.as_deref() {
                if matches!(status, "allowed" | "disabled") {
                    device.status = status.to_string();
                }
            }
            device.clone()
        };
        push_event(
            &mut inner,
            format!("Device {} updated", device_id),
            if updated.status == "disabled" {
                "blocked"
            } else {
                "ready"
            },
        );
        Some(updated)
    }

    pub fn rotate_device_key(&self, device_id: &str) -> Option<EntryConsoleDeviceKeyReport> {
        let mut inner = self.inner.lock().expect("console state lock");
        let index = inner
            .devices
            .iter()
            .position(|device| device.device_id == device_id)?;
        let app_key_once = issue_app_key(&mut inner);
        inner.devices[index].app_key_fingerprint = fingerprint(&app_key_once);
        let device_id = inner.devices[index].device_id.clone();
        push_event(
            &mut inner,
            format!("Device {device_id} key rotated"),
            "ready",
        );
        Some(EntryConsoleDeviceKeyReport {
            device: inner.devices[index].clone(),
            app_key_once,
        })
    }

    pub fn session(&self) -> EntryConsoleSession {
        self.inner
            .lock()
            .expect("console state lock")
            .session
            .clone()
    }

    pub fn record_skill_mutation(&self, name: &str, action: &str) {
        let mut inner = self.inner.lock().expect("console state lock");
        push_event(
            &mut inner,
            format!("Skill {} {}", name, action),
            if action == "deleted" {
                "limited"
            } else {
                "ready"
            },
        );
    }

    pub fn record_adapter_response(
        &self,
        operation: AdapterOperation,
        response: &AdapterResponse<AdapterSdkReport>,
    ) {
        let mut inner = self.inner.lock().expect("console state lock");
        let AdapterResponse::Accepted { report, .. } = response else {
            return;
        };
        match (operation, report) {
            (AdapterOperation::Write, AdapterSdkReport::Write(report)) => {
                if report.accepted {
                    inner.writes_today = inner.writes_today.saturating_add(report.changed as u64);
                }
                push_event(
                    &mut inner,
                    format!("Memory write accepted, changed {}", report.changed),
                    "ready",
                );
            }
            (AdapterOperation::Recall, AdapterSdkReport::Recall(report)) => {
                inner.recall_requests = inner.recall_requests.saturating_add(1);
                if !report.procedural_hits.is_empty() {
                    inner.recall_hits = inner.recall_hits.saturating_add(1);
                }
                push_event(
                    &mut inner,
                    format!(
                        "Recall served for '{}' with {} hits",
                        report.query,
                        report.procedural_hits.len()
                    ),
                    if report.procedural_hits.is_empty() {
                        "limited"
                    } else {
                        "ready"
                    },
                );
            }
            (AdapterOperation::Project, AdapterSdkReport::Project(report)) => {
                inner.projection_requests = inner.projection_requests.saturating_add(1);
                inner.last_projection_chars = report.chars;
                let chars = inner.last_projection_chars;
                push_event(
                    &mut inner,
                    format!("Memory context added, {chars} characters"),
                    "ready",
                );
            }
            _ => {}
        }
    }
}

impl From<RuntimeSkillListReport> for EntryConsoleSkillList {
    fn from(report: RuntimeSkillListReport) -> Self {
        Self {
            total: report.total,
            active: report.active,
            disabled: report.disabled,
            runtime_learned: report.runtime_skills,
            skills: report.skills.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<RuntimeSkillDetailReport> for EntryConsoleSkillDetail {
    fn from(report: RuntimeSkillDetailReport) -> Self {
        Self {
            summary: report.summary.into(),
            summary_text: report.summary_text,
            procedure_text: report.procedure_text,
            raw_content: report.raw_content,
            citations: report.citations,
            source_chat_id: report.source_chat_id,
            lineage: report.lineage,
            strategy_diffs: report.strategy_diffs,
            last_outcome_note: report.last_outcome_note,
        }
    }
}

impl From<RuntimeSkillSummary> for EntryConsoleSkillSummary {
    fn from(summary: RuntimeSkillSummary) -> Self {
        Self {
            name: summary.name,
            title: summary.title,
            topic: summary.topic,
            status: summary.status,
            enabled: summary.enabled,
            quality_score: summary.quality_score,
            use_count: summary.use_count,
            validated_success_count: summary.validated_success_count,
            mismatch_count: summary.mismatch_count,
            revision_pending: summary.revision_pending,
            updated_at: summary.updated_at,
            last_used_at: summary.last_used_at,
        }
    }
}

impl From<RuntimeSkillMutationReport> for EntryConsoleSkillMutation {
    fn from(report: RuntimeSkillMutationReport) -> Self {
        Self {
            accepted: report.accepted,
            changed: report.changed,
            name: report.name,
            operation: report.operation.to_string(),
            reason: report.reason,
        }
    }
}

fn system_info(runtime_budget: &RuntimeBudgetReport) -> EntryConsoleSystemInfo {
    EntryConsoleSystemInfo {
        name: system_name().to_string(),
        cpu: runtime_budget
            .resource_snapshot
            .available_parallelism
            .map(|threads| format!("{} / {threads} threads", std::env::consts::ARCH))
            .unwrap_or_else(|| std::env::consts::ARCH.to_string()),
        memory: match (
            runtime_budget.resource_snapshot.memory_available_bytes,
            runtime_budget.resource_snapshot.memory_total_bytes,
        ) {
            (Some(available), Some(total)) => {
                format!(
                    "{} available / {} total",
                    format_bytes(available),
                    format_bytes(total)
                )
            }
            (Some(available), None) => format!("{} available", format_bytes(available)),
            _ => "unavailable".to_string(),
        },
        time_unix_secs: current_unix_secs(),
    }
}

fn system_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        "espidf" => "ESP-IDF",
        other => other,
    }
}

fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn runtime_shape(
    config: &EntryRuntimeConfig,
    runtime_budget: &RuntimeBudgetReport,
) -> EntryConsoleRuntimeShape {
    EntryConsoleRuntimeShape {
        profile: runtime_budget.profile.as_str().to_string(),
        name: match runtime_budget.profile {
            bm_sdk::ProfileId::LinuxDeviceStandaloneMemory => "Linux device standalone".to_string(),
            bm_sdk::ProfileId::DesktopMacosStandaloneMemory => {
                "macOS desktop standalone".to_string()
            }
            bm_sdk::ProfileId::ServerLinuxMemoryGateway => {
                "Linux server memory gateway".to_string()
            }
            bm_sdk::ProfileId::EspStandaloneMemory => "ESP standalone memory".to_string(),
            bm_sdk::ProfileId::EspEmbeddedSdk => "ESP embedded SDK".to_string(),
            bm_sdk::ProfileId::DesktopMacosEmbeddedSdk => "macOS embedded SDK".to_string(),
            bm_sdk::ProfileId::DesktopWindowsEmbeddedSdk => "Windows embedded SDK".to_string(),
            bm_sdk::ProfileId::DesktopMacosDevFull => "macOS development runtime".to_string(),
            bm_sdk::ProfileId::DesktopWindowsDevFull => "Windows development runtime".to_string(),
            bm_sdk::ProfileId::ServerLinuxDevFull => "Linux development gateway".to_string(),
        },
        store: store_label(config.store.backend()).to_string(),
        shell: "HTTP console".to_string(),
    }
}

fn store_label(backend: StoreBackendKind) -> &'static str {
    match backend {
        StoreBackendKind::InMemory => "in-memory",
        StoreBackendKind::Embedded => "embedded",
        StoreBackendKind::File => "file",
        StoreBackendKind::Sqlite => "sqlite",
    }
}

fn transports(config: &EntryTransportConfig) -> Vec<EntryConsoleTransport> {
    vec![
        transport("http", config.http_server, "0.0.0.0:8718"),
        transport("llm-gateway", config.llm_gateway_server, "127.0.0.1:8787"),
        transport(
            "wss",
            config.wss_server || config.wss_client,
            "/memory/events",
        ),
        transport("mcp", config.mcp_server, "stdio"),
        transport("a2a", config.a2a_bridge, "http://127.0.0.1:8720/a2a"),
    ]
}

fn http_base_url(endpoint: &str, fallback: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    let endpoint = if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    };
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_string()
    } else {
        format!("http://{endpoint}")
    }
}

fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn mcp_streamable_http_url(endpoint: &str) -> String {
    let trimmed = endpoint.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        "http://127.0.0.1:8788/mcp".to_string()
    }
}

fn memory_context_rows(inner: &EntryConsoleInner) -> Vec<EntryConsoleKv> {
    vec![
        EntryConsoleKv {
            label: "Store".to_string(),
            value: match inner.storage_path.as_deref() {
                Some(path) => format!("{}:{}", inner.runtime_shape.store, path.display()),
                None => inner.runtime_shape.store.clone(),
            },
        },
        EntryConsoleKv {
            label: "Owner".to_string(),
            value: inner.session.owner.clone(),
        },
        EntryConsoleKv {
            label: "Agent".to_string(),
            value: inner.agent_id.clone(),
        },
        EntryConsoleKv {
            label: "Channel".to_string(),
            value: inner.channel.clone(),
        },
        EntryConsoleKv {
            label: "Chat".to_string(),
            value: inner.session.memory_scope.clone(),
        },
    ]
}

fn llm_protocol(
    id: &str,
    title: &str,
    status: &str,
    endpoint: &str,
    detail: &str,
) -> EntryConsoleLlmGatewayProtocol {
    EntryConsoleLlmGatewayProtocol {
        id: id.to_string(),
        title: title.to_string(),
        status: status.to_string(),
        endpoint: endpoint.to_string(),
        detail: detail.to_string(),
    }
}

fn rule_exports(openai_base_url: &str, mcp_url: &str) -> Vec<EntryConsoleLlmGatewayRuleExport> {
    [
        ("continue", "Continue"),
        ("cline", "Cline"),
        ("aider", "Aider"),
        ("zed", "Zed"),
        ("opencode", "OpenCode"),
        ("open-webui", "Open WebUI"),
        ("vscode", "VS Code / VSCodium"),
    ]
    .into_iter()
    .map(|(target, label)| EntryConsoleLlmGatewayRuleExport {
        target: target.to_string(),
        label: label.to_string(),
        command: format!(
            "bm agent-rules export --target {target} --gateway-url {openai_base_url} --mcp-url {mcp_url}"
        ),
    })
    .collect()
}

fn smoke_check(
    id: &str,
    label: &str,
    status: &str,
    command: String,
) -> EntryConsoleLlmGatewaySmokeCheck {
    EntryConsoleLlmGatewaySmokeCheck {
        id: id.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        command,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EntryConsoleSmokeCommand {
    id: String,
    label: String,
    command: String,
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    timeout: Duration,
}

fn llm_gateway_smoke_command(
    inner: &EntryConsoleInner,
    id: &str,
) -> Option<EntryConsoleSmokeCommand> {
    let gateway = inner
        .transports
        .iter()
        .find(|item| item.id == "llm-gateway")
        .cloned()
        .unwrap_or_else(|| transport("llm-gateway", false, "http://127.0.0.1:8787"));
    let base_url = http_base_url(&gateway.endpoint, "127.0.0.1:8787");
    let openai_base_url = join_url(&base_url, "v1");
    let provider_capabilities_url = join_url(&openai_base_url, "bm/provider-capabilities");
    match id {
        "provider-capabilities" => Some(EntryConsoleSmokeCommand {
            id: id.to_string(),
            label: "Provider capabilities".to_string(),
            command: format!("curl -fsS {provider_capabilities_url}"),
            program: "curl".to_string(),
            args: vec!["-fsS".to_string(), provider_capabilities_url],
            env: Vec::new(),
            timeout: Duration::from_secs(10),
        }),
        "release-integrations" => Some(EntryConsoleSmokeCommand {
            id: id.to_string(),
            label: "Release integration gate".to_string(),
            command: "bash scripts/check_llm_gateway_release_integrations.sh".to_string(),
            program: "bash".to_string(),
            args: vec!["scripts/check_llm_gateway_release_integrations.sh".to_string()],
            env: Vec::new(),
            timeout: Duration::from_secs(120),
        }),
        "ollama-native" => Some(EntryConsoleSmokeCommand {
            id: id.to_string(),
            label: "Ollama native live smoke".to_string(),
            command: "BM_LLM_GATEWAY_OLLAMA_SMOKE=1 bash scripts/check_llm_gateway_release_integrations.sh"
                .to_string(),
            program: "bash".to_string(),
            args: vec!["scripts/check_llm_gateway_release_integrations.sh".to_string()],
            env: vec![("BM_LLM_GATEWAY_OLLAMA_SMOKE".to_string(), "1".to_string())],
            timeout: Duration::from_secs(120),
        }),
        _ => None,
    }
}

const CONSOLE_SMOKE_OUTPUT_LIMIT: usize = 24 * 1024;

fn run_console_smoke_command(
    spec: EntryConsoleSmokeCommand,
) -> EntryConsoleLlmGatewaySmokeRunReport {
    let started_at_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    let started = Instant::now();
    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return EntryConsoleLlmGatewaySmokeRunReport {
                id: spec.id,
                label: spec.label,
                status: "blocked".to_string(),
                command: spec.command,
                exit_code: None,
                stdout: String::new(),
                stderr: error.to_string(),
                duration_ms: started.elapsed().as_millis() as u64,
                timed_out: false,
                started_at_unix_secs,
                cwd,
            };
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_handle = thread::spawn(move || read_capped_output(stdout));
    let stderr_handle = thread::spawn(move || read_capped_output(stderr));
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(done)) => {
                break Some(done);
            }
            Ok(None) => {
                if started.elapsed() >= spec.timeout {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait().ok();
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                let _ = child.kill();
                let mut report =
                    command_report(spec, started, started_at_unix_secs, cwd, None, timed_out);
                report.status = "blocked".to_string();
                report.stderr = error.to_string();
                return report;
            }
        }
    };
    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();
    let mut report = command_report(
        spec,
        started,
        started_at_unix_secs,
        cwd,
        status.as_ref().and_then(|value| value.code()),
        timed_out,
    );
    report.stdout = stdout;
    report.stderr = stderr;
    report
}

fn command_report(
    spec: EntryConsoleSmokeCommand,
    started: Instant,
    started_at_unix_secs: u64,
    cwd: String,
    exit_code: Option<i32>,
    timed_out: bool,
) -> EntryConsoleLlmGatewaySmokeRunReport {
    let status = if timed_out {
        "limited"
    } else if exit_code == Some(0) {
        "ready"
    } else {
        "blocked"
    };
    EntryConsoleLlmGatewaySmokeRunReport {
        id: spec.id,
        label: spec.label,
        status: status.to_string(),
        command: spec.command,
        exit_code,
        stdout: String::new(),
        stderr: String::new(),
        duration_ms: started.elapsed().as_millis() as u64,
        timed_out,
        started_at_unix_secs,
        cwd,
    }
}

fn read_capped_output<R: Read + Send + 'static>(reader: Option<R>) -> String {
    let Some(mut reader) = reader else {
        return String::new();
    };
    let mut out = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if out.len() < CONSOLE_SMOKE_OUTPUT_LIMIT {
                    let remaining = CONSOLE_SMOKE_OUTPUT_LIMIT - out.len();
                    let take = remaining.min(read);
                    out.extend_from_slice(&buffer[..take]);
                    truncated |= take < read;
                } else {
                    truncated = true;
                }
            }
            Err(error) => {
                if out.len() < CONSOLE_SMOKE_OUTPUT_LIMIT {
                    out.extend_from_slice(error.to_string().as_bytes());
                }
                break;
            }
        }
    }
    let mut text = String::from_utf8_lossy(&out).to_string();
    if truncated {
        text.push_str("\n[output truncated]");
    }
    text
}

fn transport(id: &str, enabled: bool, endpoint: &str) -> EntryConsoleTransport {
    EntryConsoleTransport {
        id: id.to_string(),
        enabled,
        status: if enabled { "ready" } else { "draft" }.to_string(),
        endpoint: endpoint.to_string(),
        editable: true,
    }
}

fn default_devices(config: &EntryRuntimeConfig) -> Vec<EntryConsoleDevice> {
    vec![EntryConsoleDevice {
        device_id: config.identity.agent_id.clone(),
        label: "Runtime owner device".to_string(),
        app_key_fingerprint: fingerprint(&format!(
            "{}:{}",
            config.identity.owner_id, config.identity.agent_id
        )),
        status: "allowed".to_string(),
    }]
}

fn storage_metric(
    runtime_budget: &RuntimeBudgetReport,
    storage_path: Option<&Path>,
) -> EntryConsoleMetric {
    let store_used = storage_path
        .and_then(|path| storage_path_bytes(path).ok())
        .unwrap_or(0);
    let total = runtime_budget.resource_snapshot.storage_total_bytes;
    EntryConsoleMetric {
        value: match total {
            Some(total) => format!("{} / {}", format_bytes(store_used), format_bytes(total)),
            None => format!("{} / unknown", format_bytes(store_used)),
        },
        desc: format!(
            "Memory store usage / host total storage; store snapshot budget {}",
            format_bytes(runtime_budget.store_budget.snapshot_max_bytes as u64)
        ),
        progress: total.and_then(|total| {
            if total == 0 || store_used == 0 {
                None
            } else {
                Some((store_used as f32 / total as f32) * 100.0)
            }
        }),
    }
}

fn storage_path_bytes(path: &Path) -> std::io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        total = total.saturating_add(storage_path_bytes(&entry.path())?);
    }
    Ok(total)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

fn percentage_value(value: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (value as f32 / total as f32) * 100.0
    }
}

fn recent_events(inner: &EntryConsoleInner, enabled_transports: usize) -> Vec<EntryConsoleEvent> {
    let mut events = vec![EntryConsoleEvent {
        time: "now".to_string(),
        text: format!(
            "{enabled_transports}/{} communication entries enabled",
            inner.transports.len()
        ),
        tone: if enabled_transports == inner.transports.len() {
            "ready"
        } else {
            "limited"
        }
        .to_string(),
    }];
    events.extend(inner.events.iter().rev().take(5).cloned());
    events
}

fn push_event(inner: &mut EntryConsoleInner, text: String, tone: &str) {
    inner.events.push(EntryConsoleEvent {
        time: "now".to_string(),
        text,
        tone: tone.to_string(),
    });
    const MAX_EVENTS: usize = 16;
    if inner.events.len() > MAX_EVENTS {
        let drop_count = inner.events.len() - MAX_EVENTS;
        inner.events.drain(0..drop_count);
    }
}

fn issue_app_key(inner: &mut EntryConsoleInner) -> String {
    let counter = inner.api_key_counter;
    inner.api_key_counter = inner.api_key_counter.saturating_add(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("bm-api-{counter:04x}-{nanos:x}")
}

fn fingerprint(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fp:{:04x}:{:04x}", (hash >> 16) & 0xffff, hash & 0xffff)
}

fn percentage(value: usize, total: usize) -> Option<f32> {
    if total == 0 {
        None
    } else {
        Some((value as f32 / total as f32) * 100.0)
    }
}
