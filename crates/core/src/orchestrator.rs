use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum PressureLevel {
    #[default]
    Normal = 0,
    Cautious = 1,
    Critical = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpThreadRole {
    Interactive,
    Io,
    Background,
}

pub fn set_current_http_thread_role(_role: HttpThreadRole) {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCapabilityStatus {
    #[default]
    Online,
    Degraded,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCapabilityReason {
    #[default]
    Nominal,
    NotConfigured,
    RuntimeNotInitialized,
    DeviceMissing,
    DeviceDisconnected,
    WorkerDead,
    DriverError,
    PermissionDenied,
    UpstreamUnavailable,
    RecoveryStabilizing,
    OperatorDisabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeCapabilityUpdate {
    pub id: &'static str,
    pub status: RuntimeCapabilityStatus,
    pub reason: RuntimeCapabilityReason,
    pub observed_at_secs: u32,
    pub recovery_hint: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCapabilityState {
    pub id: &'static str,
    pub status: RuntimeCapabilityStatus,
    pub reason: RuntimeCapabilityReason,
    pub epoch: u32,
    pub changed_at_secs: u32,
    pub observed_at_secs: u32,
    pub active_calls: u32,
    pub draining: bool,
    pub last_transition_uptime_ms: u64,
    pub drain_denied_total: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<&'static str>,
}

impl RuntimeCapabilityState {
    fn reset(id: &'static str) -> Self {
        Self {
            id,
            status: RuntimeCapabilityStatus::Offline,
            reason: RuntimeCapabilityReason::RuntimeNotInitialized,
            epoch: 0,
            changed_at_secs: 0,
            observed_at_secs: 0,
            active_calls: 0,
            draining: false,
            last_transition_uptime_ms: 0,
            drain_denied_total: 0,
            recovery_hint: None,
        }
    }
}

pub const RUNTIME_CAPABILITY_AUDIO_INPUT: &str = "audio_input";
pub const RUNTIME_CAPABILITY_AUDIO_OUTPUT: &str = "audio_output";

const RUNTIME_CAPABILITY_IDS: [&str; 2] = [
    RUNTIME_CAPABILITY_AUDIO_INPUT,
    RUNTIME_CAPABILITY_AUDIO_OUTPUT,
];

fn runtime_capability_state() -> &'static Mutex<HashMap<&'static str, RuntimeCapabilityState>> {
    static STATE: OnceLock<Mutex<HashMap<&'static str, RuntimeCapabilityState>>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(
            RUNTIME_CAPABILITY_IDS
                .iter()
                .copied()
                .map(|id| (id, RuntimeCapabilityState::reset(id)))
                .collect(),
        )
    })
}

pub mod runtime_capability {
    pub static RUNTIME_CAPABILITY_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
}

pub fn update_runtime_capability(update: RuntimeCapabilityUpdate) {
    let mut state = runtime_capability_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let current = state
        .entry(update.id)
        .or_insert_with(|| RuntimeCapabilityState::reset(update.id));
    let changed =
        current.epoch == 0 || current.status != update.status || current.reason != update.reason;
    if changed {
        current.epoch = current.epoch.saturating_add(1).max(1);
        current.changed_at_secs = update.observed_at_secs;
    } else {
        current.epoch = current.epoch.max(1);
    }
    current.status = update.status;
    current.reason = update.reason;
    current.observed_at_secs = update.observed_at_secs;
    current.recovery_hint = update.recovery_hint;
}

pub fn get_runtime_capability(name: &str) -> Option<RuntimeCapabilityState> {
    runtime_capability_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(name)
        .copied()
}

pub fn runtime_capability_snapshot() -> Vec<RuntimeCapabilityState> {
    let mut values = runtime_capability_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .values()
        .copied()
        .collect::<Vec<_>>();
    values.sort_by_key(|state| state.id);
    values
}

#[cfg(test)]
pub fn reset_runtime_capabilities_for_tests() {
    let mut state = runtime_capability_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state.clear();
    for id in RUNTIME_CAPABILITY_IDS {
        state.insert(id, RuntimeCapabilityState::reset(id));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StorageContentionRisk {
    #[default]
    Healthy,
    Cautious,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OrchestratorSnapshot {
    pub pressure: PressureLevel,
    pub heap_free_internal: u32,
    pub heap_free_spiram: u32,
    pub heap_largest_block_internal: u32,
    pub storage_used_kb: u32,
    pub storage_total_kb: u32,
    pub inbound_depth: u32,
    pub outbound_depth: u32,
    pub active_http_count: u32,
    pub active_wss_count: u32,
    pub active_agent_tasks: u32,
    pub audio_recording: bool,
    pub audio_playing: bool,
    pub storage_contention_risk: StorageContentionRisk,
}

impl Default for OrchestratorSnapshot {
    fn default() -> Self {
        Self {
            pressure: PressureLevel::Normal,
            heap_free_internal: 1024 * 1024,
            heap_free_spiram: 0,
            heap_largest_block_internal: 1024 * 1024,
            storage_used_kb: 0,
            storage_total_kb: 0,
            inbound_depth: 0,
            outbound_depth: 0,
            active_http_count: 0,
            active_wss_count: 0,
            active_agent_tasks: 0,
            audio_recording: false,
            audio_playing: false,
            storage_contention_risk: StorageContentionRisk::Healthy,
        }
    }
}

pub fn snapshot() -> OrchestratorSnapshot {
    OrchestratorSnapshot::default()
}

pub fn refresh_heap_if_stale() -> PressureLevel {
    PressureLevel::Normal
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Interactive,
    Background,
}

pub struct HttpPermitGuard;

pub fn request_http_permit(
    _priority: Priority,
    _timeout: std::time::Duration,
) -> crate::Result<HttpPermitGuard> {
    Ok(HttpPermitGuard)
}

impl PressureLevel {
    pub fn from_byte(value: u8) -> Self {
        match value {
            0 => Self::Normal,
            1 => Self::Cautious,
            _ => Self::Critical,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Cautious => "Cautious",
            Self::Critical => "Critical",
        }
    }
}
