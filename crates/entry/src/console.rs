use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bm_adapter::{AdapterOperation, AdapterResponse, AdapterSdkReport};
use bm_sdk::{
    MemorySkillDetailReport, MemorySkillKind, MemorySkillListReport, MemorySkillMutationReport,
    MemorySkillOrigin, MemorySkillSummary, StoreBackendKind,
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
    pub storage: EntryConsoleMetric,
    pub writes_today: EntryConsoleMetric,
    pub recall: EntryConsoleMetric,
    pub projection: EntryConsoleMetric,
    pub devices: EntryConsoleMetric,
    pub recent_events: Vec<EntryConsoleEvent>,
    pub capabilities: Vec<EntryConsoleCapabilityRow>,
    pub kernel: Vec<EntryConsoleKv>,
    pub session: Vec<EntryConsoleKv>,
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
    pub kind: String,
    pub origin: String,
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
    pub user_provided: usize,
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
pub struct EntryConsoleSkillUpsert {
    pub name: Option<String>,
    pub title: String,
    pub topic: String,
    pub summary: String,
    pub procedure: String,
    #[serde(default)]
    pub citations: Vec<String>,
    #[serde(default)]
    pub source_chat_id: Option<String>,
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

#[derive(Clone, Debug)]
struct EntryConsoleInner {
    runtime_shape: EntryConsoleRuntimeShape,
    storage_path: Option<PathBuf>,
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
    pub fn new(config: &EntryRuntimeConfig) -> Self {
        Self {
            inner: Mutex::new(EntryConsoleInner {
                runtime_shape: runtime_shape(config),
                storage_path: config.store.data_path.clone(),
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

    pub fn overview(&self) -> EntryConsoleOverview {
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
        EntryConsoleOverview {
            runtime_shape: inner.runtime_shape.clone(),
            system_info: system_info(),
            storage: storage_metric(&inner),
            writes_today: EntryConsoleMetric {
                value: inner.writes_today.to_string(),
                desc: "Accepted memory writes recorded by this runtime".to_string(),
                progress: None,
            },
            recall: EntryConsoleMetric {
                value: format!("{:.1}%", recall_rate(&inner)),
                desc: format!(
                    "{} recall requests / {} with hits",
                    inner.recall_requests, inner.recall_hits
                ),
                progress: Some(recall_rate(&inner)),
            },
            projection: EntryConsoleMetric {
                value: if inner.projection_requests == 0 {
                    "0".to_string()
                } else {
                    format!("{} chars", inner.last_projection_chars)
                },
                desc: format!("{} projection requests served", inner.projection_requests),
                progress: None,
            },
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
        }
    }

    pub fn transports(&self) -> Vec<EntryConsoleTransport> {
        self.inner
            .lock()
            .expect("console state lock")
            .transports
            .clone()
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
                    inner.writes_today = inner
                        .writes_today
                        .saturating_add(report.changed.max(1) as u64);
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
                inner.last_projection_chars = report.system_memory_block.chars().count();
                let chars = inner.last_projection_chars;
                push_event(
                    &mut inner,
                    format!("Projection served, {chars} chars"),
                    "ready",
                );
            }
            _ => {}
        }
    }
}

impl From<MemorySkillListReport> for EntryConsoleSkillList {
    fn from(report: MemorySkillListReport) -> Self {
        Self {
            total: report.total,
            active: report.active,
            disabled: report.disabled,
            runtime_learned: report.runtime_learned,
            user_provided: report.user_provided,
            skills: report.skills.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<MemorySkillDetailReport> for EntryConsoleSkillDetail {
    fn from(report: MemorySkillDetailReport) -> Self {
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

impl From<MemorySkillSummary> for EntryConsoleSkillSummary {
    fn from(summary: MemorySkillSummary) -> Self {
        Self {
            name: summary.name,
            kind: skill_kind_label(summary.kind).to_string(),
            origin: skill_origin_label(summary.origin).to_string(),
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

impl From<MemorySkillMutationReport> for EntryConsoleSkillMutation {
    fn from(report: MemorySkillMutationReport) -> Self {
        Self {
            accepted: report.accepted,
            changed: report.changed,
            name: report.name,
            operation: report.operation.to_string(),
            reason: report.reason,
        }
    }
}

fn skill_origin_label(origin: MemorySkillOrigin) -> &'static str {
    match origin {
        MemorySkillOrigin::UserProvided => "user_provided",
        MemorySkillOrigin::RuntimeLearned => "runtime_learned",
    }
}

fn skill_kind_label(kind: MemorySkillKind) -> &'static str {
    match kind {
        MemorySkillKind::RuntimeSkill => "runtime_skill",
        MemorySkillKind::ManualDocument => "manual_document",
    }
}

fn system_info() -> EntryConsoleSystemInfo {
    EntryConsoleSystemInfo {
        name: system_name().to_string(),
        cpu: cpu_label(),
        memory: memory_label(),
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

fn cpu_label() -> String {
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(0);
    let brand = cpu_brand().unwrap_or_else(|| std::env::consts::ARCH.to_string());
    if threads == 0 {
        brand
    } else {
        format!("{brand} / {threads} threads")
    }
}

fn cpu_brand() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        command_output("sysctl", &["-n", "machdep.cpu.brand_string"])
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find_map(|line| line.strip_prefix("model name"))
                    .and_then(|line| {
                        line.split_once(':')
                            .map(|(_, value)| value.trim().to_string())
                    })
            })
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("PROCESSOR_IDENTIFIER")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

fn memory_label() -> String {
    memory_bytes()
        .map(format_bytes)
        .unwrap_or_else(|| "unknown".to_string())
}

fn memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        command_output("sysctl", &["-n", "hw.memsize"]).and_then(|value| value.parse().ok())
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|content| {
                content.lines().find_map(|line| {
                    line.strip_prefix("MemTotal:")
                        .and_then(|value| value.split_whitespace().next())
                        .and_then(|kb| kb.parse::<u64>().ok())
                        .map(|kb| kb.saturating_mul(1024))
                })
            })
    }
    #[cfg(target_os = "windows")]
    {
        None
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_output(program: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn runtime_shape(config: &EntryRuntimeConfig) -> EntryConsoleRuntimeShape {
    EntryConsoleRuntimeShape {
        profile: config.profile.as_str().to_string(),
        name: match config.profile {
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
            bm_sdk::ProfileId::ServerLinuxDevFull => "Linux development gateway".to_string(),
        },
        store: store_label(config.store.backend).to_string(),
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

fn storage_metric(inner: &EntryConsoleInner) -> EntryConsoleMetric {
    let used = inner.storage_path.as_deref().map(path_size).unwrap_or(0);
    let available = storage_available_bytes(inner.storage_path.as_deref());
    EntryConsoleMetric {
        value: match available {
            Some(available) => format!("{} / {}", format_bytes(used), format_bytes(available)),
            None => format!("{} / unknown", format_bytes(used)),
        },
        desc: "Current storage usage / system available storage".to_string(),
        progress: available.and_then(|available| {
            let total = used.saturating_add(available);
            if total == 0 {
                None
            } else {
                Some((used as f32 / total as f32) * 100.0)
            }
        }),
    }
}

fn storage_available_bytes(path: Option<&Path>) -> Option<u64> {
    let owned_current;
    let path = match path {
        Some(path) => path,
        None => {
            owned_current = std::env::current_dir().ok()?;
            owned_current.as_path()
        }
    };
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        command_output("df", &["-k", path.to_str()?])
            .and_then(|output| output.lines().last().map(str::to_string))
            .and_then(|line| {
                line.split_whitespace()
                    .nth(3)
                    .and_then(|kb| kb.parse::<u64>().ok())
                    .map(|kb| kb.saturating_mul(1024))
            })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        None
    }
}

fn path_size(path: &Path) -> u64 {
    let Ok(metadata) = std::fs::metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| path_size(&entry.path()))
        .sum()
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

fn recall_rate(inner: &EntryConsoleInner) -> f32 {
    if inner.recall_requests == 0 {
        0.0
    } else {
        (inner.recall_hits as f32 / inner.recall_requests as f32) * 100.0
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
