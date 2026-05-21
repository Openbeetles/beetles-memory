use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::feature_gate::ProfileId;
use crate::orchestrator::PressureLevel;
use crate::runtime::{
    mode, ConfigActivityPhase, RuntimeForegroundOverlay, RuntimeMode, RuntimeModeSnapshot,
};

static LIFECYCLE_EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

const DEFAULT_DEFER_RETRY_AFTER_MS: u64 = 1_000;
pub const POST_REPLY_LIGHTWEIGHT_AFTER_MS: u64 = 30_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLifecycleOperation {
    Open,
    Close,
    Recover,
    Maintain,
    Project,
    Inspect,
    Export,
    Import,
    Replay,
}

impl RuntimeLifecycleOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Close => "close",
            Self::Recover => "recover",
            Self::Maintain => "maintain",
            Self::Project => "project",
            Self::Inspect => "inspect",
            Self::Export => "export",
            Self::Import => "import",
            Self::Replay => "replay",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLifecycleTrigger {
    SdkCall,
    BootRecovery,
    PostReply,
    DeferredDue,
    OperatorRequested,
    SnapshotTransfer,
    ReplayInspection,
}

impl RuntimeLifecycleTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SdkCall => "sdk_call",
            Self::BootRecovery => "boot_recovery",
            Self::PostReply => "post_reply",
            Self::DeferredDue => "deferred_due",
            Self::OperatorRequested => "operator_requested",
            Self::SnapshotTransfer => "snapshot_transfer",
            Self::ReplayInspection => "replay_inspection",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLifecycleDisposition {
    ExecuteNow,
    Defer,
    Skip,
    Reject,
    Failed,
}

impl RuntimeLifecycleDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExecuteNow => "execute_now",
            Self::Defer => "defer",
            Self::Skip => "skip",
            Self::Reject => "reject",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLifecycleEffect {
    Noop,
    RunMaintenance,
    RequestLongTermRefresh,
    RefreshProjection,
    Inspect,
    ExportSnapshot,
    ImportSnapshot,
    RunReplayInspection,
    RecordOperatorAction,
    RecoverSoulKernel,
}

impl RuntimeLifecycleEffect {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Noop => "noop",
            Self::RunMaintenance => "run_maintenance",
            Self::RequestLongTermRefresh => "request_long_term_refresh",
            Self::RefreshProjection => "refresh_projection",
            Self::Inspect => "inspect",
            Self::ExportSnapshot => "export_snapshot",
            Self::ImportSnapshot => "import_snapshot",
            Self::RunReplayInspection => "run_replay_inspection",
            Self::RecordOperatorAction => "record_operator_action",
            Self::RecoverSoulKernel => "recover_soul_kernel",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLifecycleEventKind {
    RuntimeLifecycle,
    OperatorAction,
}

impl RuntimeLifecycleEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeLifecycle => "runtime_lifecycle",
            Self::OperatorAction => "operator_action",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLifecycleModeInput {
    pub profile: ProfileId,
    pub booting: bool,
    pub pairing_required: bool,
    pub pairing_state_known: bool,
    pub foreground: RuntimeForegroundOverlay,
    pub voice_exclusive_active: bool,
    pub config_active: bool,
    pub config_activity_phase: ConfigActivityPhase,
    pub maintenance_active: bool,
    pub recovery_safe_mode_active: bool,
    pub pressure: PressureLevel,
    pub post_reply_defer_elapsed_ms: Option<u64>,
}

impl Default for RuntimeLifecycleModeInput {
    fn default() -> Self {
        Self {
            profile: ProfileId::ServerLinuxDevFull,
            booting: false,
            pairing_required: false,
            pairing_state_known: true,
            foreground: RuntimeForegroundOverlay::default(),
            voice_exclusive_active: false,
            config_active: false,
            config_activity_phase: ConfigActivityPhase::Idle,
            maintenance_active: false,
            recovery_safe_mode_active: false,
            pressure: PressureLevel::Normal,
            post_reply_defer_elapsed_ms: None,
        }
    }
}

impl RuntimeLifecycleModeInput {
    pub fn runtime_mode_snapshot(self) -> RuntimeModeSnapshot {
        mode::snapshot_from_source(mode::RuntimeModeSource {
            wifi_sta_connected: true,
            boot_phase_active: self.booting,
            pairing_required: self.pairing_required,
            pairing_state_known: self.pairing_state_known,
            voice_exclusive_active: self.voice_exclusive_active,
            background_maintenance_active: self.maintenance_active,
            config_plane_alive: true,
            config_active: self.config_active,
            config_activity_phase: self.config_activity_phase,
            channel_plane_alive: true,
            voice_plane_alive: self.voice_exclusive_active,
            agent_plane_alive: true,
            external_wss_managed_present: false,
            external_wss_suspend_requested: false,
            external_wss_suspended: false,
            recovery_safe_mode_active: self.recovery_safe_mode_active,
            runtime_foreground: self.foreground,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLifecycleAdmission {
    pub operation: RuntimeLifecycleOperation,
    pub trigger: RuntimeLifecycleTrigger,
    pub disposition: RuntimeLifecycleDisposition,
    pub profile: ProfileId,
    pub mode: RuntimeModeSnapshot,
    pub pressure: PressureLevel,
    pub reason: String,
    pub retry_after_ms: Option<u64>,
    pub lightweight_allowed: bool,
    pub private_depth_allowed: bool,
}

impl RuntimeLifecycleAdmission {
    fn admitted(
        operation: RuntimeLifecycleOperation,
        trigger: RuntimeLifecycleTrigger,
        input: RuntimeLifecycleModeInput,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            trigger,
            disposition: RuntimeLifecycleDisposition::ExecuteNow,
            profile: input.profile,
            mode: input.runtime_mode_snapshot(),
            pressure: input.pressure,
            reason: reason.into(),
            retry_after_ms: None,
            lightweight_allowed: false,
            private_depth_allowed: true,
        }
    }

    fn defer(
        operation: RuntimeLifecycleOperation,
        trigger: RuntimeLifecycleTrigger,
        input: RuntimeLifecycleModeInput,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            trigger,
            disposition: RuntimeLifecycleDisposition::Defer,
            profile: input.profile,
            mode: input.runtime_mode_snapshot(),
            pressure: input.pressure,
            reason: reason.into(),
            retry_after_ms: Some(DEFAULT_DEFER_RETRY_AFTER_MS),
            lightweight_allowed: false,
            private_depth_allowed: false,
        }
    }

    fn reject(
        operation: RuntimeLifecycleOperation,
        trigger: RuntimeLifecycleTrigger,
        input: RuntimeLifecycleModeInput,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            trigger,
            disposition: RuntimeLifecycleDisposition::Reject,
            profile: input.profile,
            mode: input.runtime_mode_snapshot(),
            pressure: input.pressure,
            reason: reason.into(),
            retry_after_ms: None,
            lightweight_allowed: false,
            private_depth_allowed: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLifecycleReport {
    pub operation: RuntimeLifecycleOperation,
    pub trigger: RuntimeLifecycleTrigger,
    pub admission: RuntimeLifecycleAdmission,
    pub event_id: String,
    pub started_at_unix_secs: u64,
    pub finished_at_unix_secs: u64,
    pub success: bool,
    pub changed: bool,
    pub result_summary: String,
    pub error_stage: Option<String>,
}

impl RuntimeLifecycleReport {
    pub fn from_admission(admission: RuntimeLifecycleAdmission, started_at_unix_secs: u64) -> Self {
        Self {
            operation: admission.operation,
            trigger: admission.trigger,
            admission,
            event_id: next_lifecycle_event_id(),
            started_at_unix_secs,
            finished_at_unix_secs: started_at_unix_secs,
            success: false,
            changed: false,
            result_summary: String::new(),
            error_stage: None,
        }
    }

    pub fn finish_success(
        mut self,
        finished_at_unix_secs: u64,
        changed: bool,
        result_summary: impl Into<String>,
    ) -> Self {
        self.finished_at_unix_secs = finished_at_unix_secs;
        self.success = true;
        self.changed = changed;
        self.result_summary = result_summary.into();
        self
    }

    pub fn finish_failure(
        mut self,
        finished_at_unix_secs: u64,
        error_stage: impl Into<String>,
        result_summary: impl Into<String>,
    ) -> Self {
        self.finished_at_unix_secs = finished_at_unix_secs;
        self.success = false;
        self.error_stage = Some(error_stage.into());
        self.result_summary = result_summary.into();
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeLifecycleEvent {
    pub event_id: String,
    pub kind: RuntimeLifecycleEventKind,
    pub operation: RuntimeLifecycleOperation,
    pub trigger: RuntimeLifecycleTrigger,
    pub disposition: RuntimeLifecycleDisposition,
    pub effect: RuntimeLifecycleEffect,
    pub profile: ProfileId,
    pub mode: RuntimeMode,
    pub pressure: PressureLevel,
    pub reason: String,
    pub result: String,
    pub error_stage: Option<String>,
    pub timestamp_unix_secs: u64,
    #[serde(default)]
    pub payload: BTreeMap<String, String>,
}

impl RuntimeLifecycleEvent {
    pub fn from_report(
        kind: RuntimeLifecycleEventKind,
        effect: RuntimeLifecycleEffect,
        report: &RuntimeLifecycleReport,
        timestamp_unix_secs: u64,
    ) -> Self {
        Self {
            event_id: report.event_id.clone(),
            kind,
            operation: report.operation,
            trigger: report.trigger,
            disposition: report.admission.disposition,
            effect,
            profile: report.admission.profile,
            mode: report.admission.mode.current_mode,
            pressure: report.admission.pressure,
            reason: report.admission.reason.clone(),
            result: if report.success { "ok" } else { "failed" }.to_string(),
            error_stage: report.error_stage.clone(),
            timestamp_unix_secs,
            payload: BTreeMap::new(),
        }
    }

    pub fn with_payload(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.payload.insert(key.into(), value.into());
        self
    }
}

pub trait RuntimeLifecycleEventSink: Send + Sync {
    fn record_lifecycle_event(&self, event: RuntimeLifecycleEvent) -> Result<()>;
}

#[derive(Clone, Debug, Default)]
pub struct NoopRuntimeLifecycleEventSink;

impl RuntimeLifecycleEventSink for NoopRuntimeLifecycleEventSink {
    fn record_lifecycle_event(&self, _event: RuntimeLifecycleEvent) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeLifecycleEngine;

impl RuntimeLifecycleEngine {
    pub fn admit(
        &self,
        operation: RuntimeLifecycleOperation,
        trigger: RuntimeLifecycleTrigger,
        input: RuntimeLifecycleModeInput,
    ) -> RuntimeLifecycleAdmission {
        match operation {
            RuntimeLifecycleOperation::Open | RuntimeLifecycleOperation::Close => {
                RuntimeLifecycleAdmission::admitted(operation, trigger, input, "admitted")
            }
            RuntimeLifecycleOperation::Recover => {
                RuntimeLifecycleAdmission::admitted(operation, trigger, input, "recovery_admitted")
            }
            RuntimeLifecycleOperation::Inspect => {
                let mode = input.runtime_mode_snapshot();
                let reason = if mode.current_mode == RuntimeMode::RecoverySafeMode {
                    "safe_mode_inspection"
                } else {
                    "admitted"
                };
                RuntimeLifecycleAdmission::admitted(operation, trigger, input, reason)
            }
            RuntimeLifecycleOperation::Maintain => {
                self.admit_maintenance(operation, trigger, input)
            }
            RuntimeLifecycleOperation::Project => self.admit_projection(operation, trigger, input),
            RuntimeLifecycleOperation::Export | RuntimeLifecycleOperation::Import => {
                self.admit_snapshot_transfer(operation, trigger, input)
            }
            RuntimeLifecycleOperation::Replay => {
                RuntimeLifecycleAdmission::admitted(operation, trigger, input, "inspection_only")
            }
        }
    }

    fn admit_maintenance(
        &self,
        operation: RuntimeLifecycleOperation,
        trigger: RuntimeLifecycleTrigger,
        input: RuntimeLifecycleModeInput,
    ) -> RuntimeLifecycleAdmission {
        let mode = input.runtime_mode_snapshot();
        if mode.current_mode != RuntimeMode::Normal {
            return RuntimeLifecycleAdmission::defer(
                operation,
                trigger,
                input,
                mode.mode_block_reason().unwrap_or("runtime_mode_blocked"),
            );
        }
        if input.pressure == PressureLevel::Normal {
            return RuntimeLifecycleAdmission::admitted(operation, trigger, input, "admitted");
        }
        if is_embedded_profile(input.profile)
            && matches!(trigger, RuntimeLifecycleTrigger::PostReply)
            && input
                .post_reply_defer_elapsed_ms
                .is_some_and(|elapsed| elapsed >= POST_REPLY_LIGHTWEIGHT_AFTER_MS)
        {
            let mut admission = RuntimeLifecycleAdmission::admitted(
                operation,
                trigger,
                input,
                "bounded_lightweight_maintenance",
            );
            admission.lightweight_allowed = true;
            return admission;
        }
        RuntimeLifecycleAdmission::defer(
            operation,
            trigger,
            input,
            match input.pressure {
                PressureLevel::Normal => "admitted",
                PressureLevel::Cautious => "cautious_pressure",
                PressureLevel::Critical => "critical_pressure",
            },
        )
    }

    fn admit_projection(
        &self,
        operation: RuntimeLifecycleOperation,
        trigger: RuntimeLifecycleTrigger,
        input: RuntimeLifecycleModeInput,
    ) -> RuntimeLifecycleAdmission {
        let mode = input.runtime_mode_snapshot();
        let mut admission =
            RuntimeLifecycleAdmission::admitted(operation, trigger, input, "admitted");
        if !mode.allows_prompt_private_depth(input.pressure) {
            admission.private_depth_allowed = false;
            admission.reason = "private_depth_blocked_by_mode".to_string();
        }
        admission
    }

    fn admit_snapshot_transfer(
        &self,
        operation: RuntimeLifecycleOperation,
        trigger: RuntimeLifecycleTrigger,
        input: RuntimeLifecycleModeInput,
    ) -> RuntimeLifecycleAdmission {
        let mode = input.runtime_mode_snapshot();
        if mode.current_mode == RuntimeMode::RecoverySafeMode {
            return RuntimeLifecycleAdmission::reject(
                operation,
                trigger,
                input,
                "recovery_safe_mode_blocks_snapshot_transfer",
            );
        }
        RuntimeLifecycleAdmission::admitted(operation, trigger, input, "admitted")
    }
}

fn is_embedded_profile(profile: ProfileId) -> bool {
    matches!(
        profile,
        ProfileId::EspEmbeddedSdk
            | ProfileId::DesktopMacosEmbeddedSdk
            | ProfileId::DesktopWindowsEmbeddedSdk
    )
}

pub fn next_lifecycle_event_id() -> String {
    let seq = LIFECYCLE_EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("rl{nanos:032x}_{:08x}_{seq:016x}", std::process::id())
}
