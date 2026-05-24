use crate::bus::{PcMsg, SystemInboundTx};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub mod soul_kernel;
pub use soul_kernel::{
    ensure_platform_soul_kernel_recovery, inspect_platform_soul_kernel, SoulKernelPromptProjection,
    SoulKernelRecoveryReport, SoulKernelStatus,
};

pub mod lifecycle;
pub mod lifecycle_diagnosis;
pub use lifecycle::{
    NoopRuntimeLifecycleEventSink, RuntimeLifecycleAdmission, RuntimeLifecycleDisposition,
    RuntimeLifecycleEffect, RuntimeLifecycleEngine, RuntimeLifecycleEvent,
    RuntimeLifecycleEventKind, RuntimeLifecycleEventSink, RuntimeLifecycleModeInput,
    RuntimeLifecycleOperation, RuntimeLifecycleReport, RuntimeLifecycleTrigger,
    POST_REPLY_LIGHTWEIGHT_AFTER_MS,
};
pub use lifecycle_diagnosis::{
    build_runtime_lifecycle_diagnosis, RuntimeLifecycleDiagnosisReport, RuntimeLifecycleEvidence,
    RuntimeLifecycleFinding, RuntimeLifecycleRecommendedAction, RuntimeLifecycleRootCause,
};

pub mod continuity_flush {
    use crate::memory::ContinuitySnapshot;
    use serde::{Deserialize, Serialize};

    pub const REL_PATH_REBOOT_CONTINUITY_BUNDLE: &str =
        "memory/continuity_snapshots/runtime/latest_reboot_bundle.json";

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ContinuitySnapshotBundle {
        pub version: u32,
        pub reason: String,
        pub flushed_at: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub primary_chat_id: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub snapshots: Vec<ContinuitySnapshot>,
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    #[default]
    Normal,
    Booting,
    Pairing,
    ConfigActive,
    VoiceExclusive,
    Maintenance,
    RecoverySafeMode,
}

impl RuntimeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Booting => "booting",
            Self::Pairing => "pairing",
            Self::ConfigActive => "config_active",
            Self::VoiceExclusive => "voice_exclusive",
            Self::Maintenance => "maintenance",
            Self::RecoverySafeMode => "recovery_safe_mode",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigActivityPhase {
    #[default]
    Idle,
    Starting,
    Active,
    Persisting,
    Success,
    Fail,
    Stopping,
    Cleanup,
}

impl ConfigActivityPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Active => "active",
            Self::Persisting => "persisting",
            Self::Success => "success",
            Self::Fail => "fail",
            Self::Stopping => "stopping",
            Self::Cleanup => "cleanup",
        }
    }

    pub fn blocks_new_non_voice_network_work(self) -> bool {
        matches!(self, Self::Persisting | Self::Stopping)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeForegroundOverlay {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub active_count: usize,
    #[serde(default)]
    pub primary_source: Option<RuntimeForegroundSource>,
    #[serde(default)]
    pub age_ms: Option<u64>,
    #[serde(default)]
    pub resume_after_ms: Option<u64>,
    #[serde(default)]
    pub recovery_active: bool,
    #[serde(default)]
    pub recovery_source: Option<RuntimeForegroundSource>,
    #[serde(default)]
    pub recovery_age_ms: Option<u64>,
    #[serde(default)]
    pub recovery_resume_after_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeModeActionBudget {
    pub allow_periodic_maintenance: bool,
    pub allow_due_user_timers: bool,
    pub allow_heartbeat_injection: bool,
    pub allow_best_effort_delayed_tasks: bool,
    pub allow_idle_self_runtime: bool,
    pub allow_non_voice_outbound: bool,
    pub allow_realtime_voice_connect: bool,
    pub allow_external_wss_connect: bool,
    pub require_external_wss_suspended: bool,
}

impl Default for RuntimeModeActionBudget {
    fn default() -> Self {
        Self {
            allow_periodic_maintenance: true,
            allow_due_user_timers: true,
            allow_heartbeat_injection: true,
            allow_best_effort_delayed_tasks: true,
            allow_idle_self_runtime: true,
            allow_non_voice_outbound: true,
            allow_realtime_voice_connect: true,
            allow_external_wss_connect: true,
            require_external_wss_suspended: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeForegroundSource {
    #[default]
    User,
    ExternalUserMessage,
    LocalAppChat,
    RealtimeVoiceSession,
    VoiceFallbackInteraction,
    ManualOperatorAction,
    System,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeObservation {
    #[serde(default)]
    pub foreground_source: Option<RuntimeForegroundSource>,
    #[serde(default)]
    pub streaming_response: bool,
    #[serde(default)]
    pub critical_turn: bool,
    #[serde(default)]
    pub network_recovery_active: bool,
    #[serde(default)]
    pub body_pressure_bytes: Option<u64>,
    #[serde(default)]
    pub queue_pressure_items: Option<u64>,
    #[serde(default)]
    pub pressure: crate::orchestrator::PressureLevel,
}

impl RuntimeObservation {
    pub fn foreground(source: RuntimeForegroundSource) -> Self {
        Self {
            foreground_source: Some(source),
            ..Self::default()
        }
    }

    pub fn with_streaming_response(mut self, streaming_response: bool) -> Self {
        self.streaming_response = streaming_response;
        self
    }

    pub fn with_critical_turn(mut self, critical_turn: bool) -> Self {
        self.critical_turn = critical_turn;
        self
    }

    pub fn with_pressure(mut self, pressure: crate::orchestrator::PressureLevel) -> Self {
        self.pressure = pressure;
        self
    }

    pub fn to_mode_input(self) -> RuntimeLifecycleModeInput {
        let mut input = RuntimeLifecycleModeInput::default();
        input.pressure = if self.critical_turn {
            crate::orchestrator::PressureLevel::Critical
        } else {
            self.pressure
        };
        if let Some(source) = self.foreground_source {
            input.foreground = RuntimeForegroundOverlay {
                active: true,
                active_count: 1,
                primary_source: Some(source),
                ..RuntimeForegroundOverlay::default()
            };
            input.voice_exclusive_active = matches!(
                source,
                RuntimeForegroundSource::RealtimeVoiceSession
                    | RuntimeForegroundSource::VoiceFallbackInteraction
            );
        }
        if self.network_recovery_active {
            input.foreground.recovery_active = true;
            input.foreground.recovery_source = self.foreground_source;
        }
        input
    }
}

impl RuntimeLifecycleModeInput {
    pub fn from_observation(observation: RuntimeObservation) -> Self {
        observation.to_mode_input()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeModeSnapshot {
    pub current_mode: RuntimeMode,
    pub wifi_sta_connected: bool,
    pub boot_phase_active: bool,
    pub pairing_required: bool,
    pub pairing_state_known: bool,
    pub voice_exclusive_active: bool,
    pub background_maintenance_active: bool,
    pub config_plane_alive: bool,
    pub config_active: bool,
    pub config_activity_phase: ConfigActivityPhase,
    pub channel_plane_alive: bool,
    pub voice_plane_alive: bool,
    pub agent_plane_alive: bool,
    pub external_wss_managed_present: bool,
    pub external_wss_suspend_requested: bool,
    pub external_wss_suspended: bool,
    pub recovery_safe_mode_active: bool,
    pub runtime_foreground: RuntimeForegroundOverlay,
    pub action_budget: RuntimeModeActionBudget,
}

impl Default for RuntimeModeSnapshot {
    fn default() -> Self {
        Self {
            current_mode: RuntimeMode::Normal,
            wifi_sta_connected: true,
            boot_phase_active: false,
            pairing_required: false,
            pairing_state_known: true,
            voice_exclusive_active: false,
            background_maintenance_active: false,
            config_plane_alive: true,
            config_active: false,
            config_activity_phase: ConfigActivityPhase::Idle,
            channel_plane_alive: true,
            voice_plane_alive: false,
            agent_plane_alive: true,
            external_wss_managed_present: false,
            external_wss_suspend_requested: false,
            external_wss_suspended: false,
            recovery_safe_mode_active: false,
            runtime_foreground: RuntimeForegroundOverlay::default(),
            action_budget: RuntimeModeActionBudget::default(),
        }
    }
}

impl RuntimeModeSnapshot {
    pub fn allows_prompt_governed_recall(
        &self,
        _pressure: crate::orchestrator::PressureLevel,
    ) -> bool {
        matches!(self.current_mode, RuntimeMode::Normal)
    }

    pub fn allows_prompt_background_governance(
        &self,
        _pressure: crate::orchestrator::PressureLevel,
    ) -> bool {
        self.action_budget.allow_periodic_maintenance
            && matches!(self.current_mode, RuntimeMode::Normal)
    }

    pub fn allows_prompt_private_depth(
        &self,
        _pressure: crate::orchestrator::PressureLevel,
    ) -> bool {
        matches!(self.current_mode, RuntimeMode::Normal)
    }

    pub fn mode_block_reason(&self) -> Option<&'static str> {
        match self.current_mode {
            RuntimeMode::Booting => Some("boot_phase_active"),
            RuntimeMode::Pairing => Some("pairing_required"),
            RuntimeMode::Normal => None,
            RuntimeMode::ConfigActive => Some("config_active"),
            RuntimeMode::VoiceExclusive => Some("voice_exclusive_active"),
            RuntimeMode::Maintenance => Some("background_maintenance_active"),
            RuntimeMode::RecoverySafeMode => Some("recovery_safe_mode"),
        }
    }
}

pub mod mode {
    pub use super::{
        ConfigActivityPhase, RuntimeForegroundOverlay, RuntimeMode, RuntimeModeActionBudget,
        RuntimeModeSnapshot,
    };

    #[derive(Clone, Copy, Debug, Default)]
    pub struct RuntimeModeSource {
        pub wifi_sta_connected: bool,
        pub boot_phase_active: bool,
        pub pairing_required: bool,
        pub pairing_state_known: bool,
        pub voice_exclusive_active: bool,
        pub background_maintenance_active: bool,
        pub config_plane_alive: bool,
        pub config_active: bool,
        pub config_activity_phase: ConfigActivityPhase,
        pub channel_plane_alive: bool,
        pub voice_plane_alive: bool,
        pub agent_plane_alive: bool,
        pub external_wss_managed_present: bool,
        pub external_wss_suspend_requested: bool,
        pub external_wss_suspended: bool,
        pub recovery_safe_mode_active: bool,
        pub runtime_foreground: RuntimeForegroundOverlay,
    }

    pub fn snapshot_from_source(source: RuntimeModeSource) -> RuntimeModeSnapshot {
        let current_mode = derive_mode(&source);
        let action_budget = action_budget_for_source(current_mode, source.config_activity_phase);
        RuntimeModeSnapshot {
            current_mode,
            wifi_sta_connected: source.wifi_sta_connected,
            boot_phase_active: source.boot_phase_active,
            pairing_required: source.pairing_required,
            pairing_state_known: source.pairing_state_known,
            voice_exclusive_active: source.voice_exclusive_active,
            background_maintenance_active: source.background_maintenance_active,
            config_plane_alive: source.config_plane_alive,
            config_active: source.config_active,
            config_activity_phase: source.config_activity_phase,
            channel_plane_alive: source.channel_plane_alive,
            voice_plane_alive: source.voice_plane_alive,
            agent_plane_alive: source.agent_plane_alive,
            external_wss_managed_present: source.external_wss_managed_present,
            external_wss_suspend_requested: source.external_wss_suspend_requested,
            external_wss_suspended: source.external_wss_suspended,
            recovery_safe_mode_active: source.recovery_safe_mode_active,
            runtime_foreground: source.runtime_foreground,
            action_budget,
        }
    }

    fn derive_mode(source: &RuntimeModeSource) -> RuntimeMode {
        if source.recovery_safe_mode_active {
            RuntimeMode::RecoverySafeMode
        } else if source.boot_phase_active {
            RuntimeMode::Booting
        } else if source.voice_exclusive_active {
            RuntimeMode::VoiceExclusive
        } else if source.config_active {
            RuntimeMode::ConfigActive
        } else if source.background_maintenance_active {
            RuntimeMode::Maintenance
        } else if source.pairing_state_known && source.pairing_required {
            RuntimeMode::Pairing
        } else {
            RuntimeMode::Normal
        }
    }

    fn action_budget_for_source(
        mode: RuntimeMode,
        config_phase: ConfigActivityPhase,
    ) -> RuntimeModeActionBudget {
        let mut budget = base_action_budget_for_mode(mode);
        if mode == RuntimeMode::ConfigActive && config_phase.blocks_new_non_voice_network_work() {
            budget.allow_non_voice_outbound = false;
            budget.allow_external_wss_connect = false;
            budget.require_external_wss_suspended = true;
        }
        budget
    }

    fn base_action_budget_for_mode(mode: RuntimeMode) -> RuntimeModeActionBudget {
        match mode {
            RuntimeMode::Booting | RuntimeMode::Pairing => RuntimeModeActionBudget {
                allow_periodic_maintenance: false,
                allow_due_user_timers: false,
                allow_heartbeat_injection: false,
                allow_best_effort_delayed_tasks: false,
                allow_idle_self_runtime: false,
                allow_non_voice_outbound: false,
                allow_realtime_voice_connect: false,
                allow_external_wss_connect: false,
                require_external_wss_suspended: false,
            },
            RuntimeMode::Normal => RuntimeModeActionBudget::default(),
            RuntimeMode::ConfigActive => RuntimeModeActionBudget {
                allow_periodic_maintenance: false,
                allow_due_user_timers: true,
                allow_heartbeat_injection: false,
                allow_best_effort_delayed_tasks: false,
                allow_idle_self_runtime: false,
                allow_non_voice_outbound: true,
                allow_realtime_voice_connect: false,
                allow_external_wss_connect: true,
                require_external_wss_suspended: false,
            },
            RuntimeMode::VoiceExclusive => RuntimeModeActionBudget {
                allow_periodic_maintenance: false,
                allow_due_user_timers: false,
                allow_heartbeat_injection: false,
                allow_best_effort_delayed_tasks: false,
                allow_idle_self_runtime: false,
                allow_non_voice_outbound: false,
                allow_realtime_voice_connect: true,
                allow_external_wss_connect: false,
                require_external_wss_suspended: true,
            },
            RuntimeMode::Maintenance => RuntimeModeActionBudget {
                allow_periodic_maintenance: false,
                allow_due_user_timers: true,
                allow_heartbeat_injection: false,
                allow_best_effort_delayed_tasks: false,
                allow_idle_self_runtime: false,
                allow_non_voice_outbound: true,
                allow_realtime_voice_connect: true,
                allow_external_wss_connect: true,
                require_external_wss_suspended: false,
            },
            RuntimeMode::RecoverySafeMode => RuntimeModeActionBudget {
                allow_periodic_maintenance: false,
                allow_due_user_timers: false,
                allow_heartbeat_injection: false,
                allow_best_effort_delayed_tasks: false,
                allow_idle_self_runtime: false,
                allow_non_voice_outbound: true,
                allow_realtime_voice_connect: false,
                allow_external_wss_connect: false,
                require_external_wss_suspended: false,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowKind {
    InitiativeTick,
    UpcomingReminderNudge,
    ResumeTaskCheckIn,
    LongTermMemoryRefresh,
    PostReplyMaintenance,
    IdleMemoryForge,
    RebootRecovery,
    SelfRuntimePostReply,
    SelfRuntimeIdleTick,
    OperatorMaintenance,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowTrigger {
    CronTick,
    DelayedDue,
    BootRecovery,
    PostReply,
    ModeTransition,
    OperatorRequested,
    StateDelta,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowDisposition {
    NoTrigger,
    Suppress,
    Cancel,
    DeferUntil,
    ExecuteNow,
    ExecuteFailed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowEffect {
    Noop,
    EnqueueSystemJob,
    SendOutboundNudge,
    RunRepairPass,
    PersistRecoveryIntent,
    ReplayRecovery,
    RollbackRelease,
    RequestRestart,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowRecoveryPolicy {
    DropOnModeExit,
    RetryAfterModeResume,
    ReplayAfterBoot,
    OperatorAckRequired,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowAuditRecord {
    pub workflow: WorkflowKind,
    pub trigger: WorkflowTrigger,
    pub disposition: WorkflowDisposition,
    pub effect: WorkflowEffect,
    pub recovery_policy: WorkflowRecoveryPolicy,
    pub rationale: String,
    pub recorded_at: u64,
    pub channel: Option<String>,
    pub chat_id: Option<String>,
    pub primary_chat_id: Option<String>,
    pub suppression_reason: Option<String>,
}

impl WorkflowAuditRecord {
    pub fn new(
        workflow: WorkflowKind,
        trigger: WorkflowTrigger,
        disposition: WorkflowDisposition,
        effect: WorkflowEffect,
        recovery_policy: WorkflowRecoveryPolicy,
        rationale: &str,
        recorded_at: u64,
    ) -> Self {
        Self {
            workflow,
            trigger,
            disposition,
            effect,
            recovery_policy,
            rationale: rationale.to_string(),
            recorded_at,
            channel: None,
            chat_id: None,
            primary_chat_id: None,
            suppression_reason: None,
        }
    }

    pub fn with_target(
        mut self,
        channel: Option<&str>,
        chat_id: Option<&str>,
        primary_chat_id: Option<&str>,
    ) -> Self {
        self.channel = channel.map(ToString::to_string);
        self.chat_id = chat_id.map(ToString::to_string);
        self.primary_chat_id = primary_chat_id.map(ToString::to_string);
        self
    }

    pub fn with_suppression_reason(mut self, reason: Option<&str>) -> Self {
        self.suppression_reason = reason.map(ToString::to_string);
        self
    }
}

fn workflow_audit_log() -> &'static Mutex<Vec<WorkflowAuditRecord>> {
    static LOG: OnceLock<Mutex<Vec<WorkflowAuditRecord>>> = OnceLock::new();
    LOG.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn append_workflow_audit(record: WorkflowAuditRecord) {
    let mut log = workflow_audit_log()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if log.len() >= 128 {
        log.remove(0);
    }
    log.push(record);
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowAuditSummary {
    pub total_retained: usize,
    pub executed: usize,
    pub deferred: usize,
    pub suppressed: usize,
    pub canceled: usize,
    pub no_trigger: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowAuditSnapshot {
    pub summary: WorkflowAuditSummary,
    pub recent_records: Vec<WorkflowAuditRecord>,
}

pub mod workflow {
    pub fn recent_workflow_audits(limit: usize) -> Vec<super::WorkflowAuditRecord> {
        let mut records = super::workflow_audit_log()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if records.len() > limit {
            records = records.split_off(records.len() - limit);
        }
        records
    }

    pub fn workflow_audit_snapshot(limit: usize) -> super::WorkflowAuditSnapshot {
        let recent_records = recent_workflow_audits(limit);
        let mut summary = super::WorkflowAuditSummary {
            total_retained: recent_records.len(),
            ..super::WorkflowAuditSummary::default()
        };
        for record in &recent_records {
            match record.disposition {
                super::WorkflowDisposition::ExecuteNow => summary.executed += 1,
                super::WorkflowDisposition::DeferUntil => summary.deferred += 1,
                super::WorkflowDisposition::Suppress => summary.suppressed += 1,
                super::WorkflowDisposition::Cancel => summary.canceled += 1,
                super::WorkflowDisposition::NoTrigger => summary.no_trigger += 1,
                super::WorkflowDisposition::ExecuteFailed => summary.failed += 1,
            }
        }
        super::WorkflowAuditSnapshot {
            summary,
            recent_records,
        }
    }

    pub fn reset_workflow_audit_for_tests() {
        super::workflow_audit_log()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }
}

pub fn workflow_audit_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

pub use workflow::{recent_workflow_audits, workflow_audit_snapshot};

pub fn schedule_bounded_keyed_system_inbound_msg(
    _due_at: Instant,
    tx: SystemInboundTx,
    msg: PcMsg,
    _poll_interval: Duration,
    _tag: &str,
    _key: String,
    _max_defer: Duration,
) -> bool {
    tx.send(msg).is_ok()
}

pub mod thread_registry {
    pub fn runtime_mode_snapshot() -> super::RuntimeModeSnapshot {
        super::mode::snapshot_from_source(super::mode::RuntimeModeSource {
            wifi_sta_connected: crate::state::wifi_sta_connected(),
            pairing_state_known: true,
            voice_exclusive_active: crate::state::voice_exclusive_active(),
            background_maintenance_active: crate::state::background_maintenance_active(),
            channel_plane_alive: true,
            agent_plane_alive: true,
            ..super::mode::RuntimeModeSource::default()
        })
    }

    pub fn register_thread(
        _tag: &str,
        _stack_size: usize,
        _core: Option<crate::util::SpawnCore>,
        _role: crate::util::HttpThreadRole,
        _surface: crate::platform::task_affinity::TaskSpawnSurface,
    ) {
    }

    pub fn mark_thread_stopped(_tag: &str) {}
}

pub mod delayed_task {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    pub fn delayed_task_test_scope() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
        static A: OnceLock<Mutex<()>> = OnceLock::new();
        static B: OnceLock<Mutex<()>> = OnceLock::new();
        (
            A.get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            B.get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    }
}
