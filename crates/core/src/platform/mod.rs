use crate::agent::ActiveWorkStore;
use crate::error::Result;
use crate::memory::{
    AutonomyStrategyStore, ContinuityCapsuleStore, ConversationTranscriptStore,
    CoreRevisionLedgerStore, ExecutionStateStore, FeltSignificanceStore, InnerConflictStore,
    InnerLifeStore, LongTermMemoryExtractionStateStore, MemoryStore, MentalPrivacyStore,
    OuterVoiceStore, PrivateDocStore, PrivateGardenStore, RelationshipConstitutionStore,
    RelationshipPortfolioStore, RelationshipTopologyStore, RemindAtStore, SelfAuthoredCoreStore,
    SelfContinuityStore, SelfModelStore, SessionStore, SessionSummaryStore,
    TemperamentContinuityStore, TurnContinuityEvidenceStore, TurnLedgerStore, WorldSenseStore,
};
use crate::task::TaskStore;
use crate::task_execution::{TaskArtifactStore, TaskLearningStore, TaskRunStore};
use std::path::PathBuf;
use std::sync::Arc;

mod memory_operator_surface;
pub use memory_operator_surface::*;

#[derive(Debug)]
pub enum ResponseBody {
    Heap(Vec<u8>),
}

impl ResponseBody {
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Heap(bytes) => bytes.as_slice(),
        }
    }
}

impl AsRef<[u8]> for ResponseBody {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

pub trait SkillMetaStore: Send + Sync {
    fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)>;
    fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()>;
}

pub trait SkillStorage: Send + Sync {
    fn list_names(&self) -> Result<Vec<String>>;
    fn read(&self, name: &str) -> Result<Vec<u8>>;
    fn write(&self, name: &str, content: &[u8]) -> Result<()>;
    fn remove(&self, name: &str) -> Result<()>;
}

pub trait StateFs: Send + Sync {
    fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>>;
    fn write(&self, rel_path: &str, data: &[u8]) -> Result<()>;
    fn remove(&self, rel_path: &str) -> Result<()>;
    fn list_dir(&self, _rel_path: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemorySystemKind {
    Standalone,
    SdkEmbedded,
}

impl MemorySystemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::SdkEmbedded => "sdk_embedded",
        }
    }
}

pub trait Platform: Send + Sync {
    fn memory_system_kind(&self) -> MemorySystemKind {
        MemorySystemKind::Standalone
    }

    fn runtime_lifecycle_event_sink(&self) -> Arc<dyn crate::runtime::RuntimeLifecycleEventSink> {
        Arc::new(crate::runtime::NoopRuntimeLifecycleEventSink)
    }

    fn runtime_resource_probe(&self) -> Arc<dyn crate::resource::RuntimeResourceProbe> {
        Arc::new(crate::resource::UnavailableRuntimeResourceProbe::default())
    }

    fn state_fs(&self) -> Arc<dyn StateFs>;
    fn skill_storage(&self) -> Arc<dyn SkillStorage>;
    fn skill_meta_store(&self) -> Arc<dyn SkillMetaStore>;
    fn active_work_store(&self) -> Arc<dyn ActiveWorkStore>;
    fn memory_store(&self) -> Arc<dyn MemoryStore>;
    fn session_store(&self) -> Arc<dyn SessionStore>;
    fn conversation_transcript_store(&self) -> Arc<dyn ConversationTranscriptStore>;
    fn session_summary_store(&self) -> Arc<dyn SessionSummaryStore>;
    fn long_term_memory_extraction_state_store(
        &self,
    ) -> Arc<dyn LongTermMemoryExtractionStateStore>;
    fn continuity_capsule_store(&self) -> Arc<dyn ContinuityCapsuleStore>;
    fn turn_ledger_store(&self) -> Arc<dyn TurnLedgerStore>;
    fn self_model_store(&self) -> Arc<dyn SelfModelStore>;
    fn self_authored_core_store(&self) -> Arc<dyn SelfAuthoredCoreStore>;
    fn core_revision_ledger_store(&self) -> Arc<dyn CoreRevisionLedgerStore>;
    fn self_continuity_store(&self) -> Arc<dyn SelfContinuityStore>;
    fn relationship_constitution_store(&self) -> Arc<dyn RelationshipConstitutionStore>;
    fn relationship_portfolio_store(&self) -> Arc<dyn RelationshipPortfolioStore>;
    fn relationship_topology_store(&self) -> Arc<dyn RelationshipTopologyStore>;
    fn execution_state_store(&self) -> Arc<dyn ExecutionStateStore>;
    fn world_sense_store(&self) -> Arc<dyn WorldSenseStore>;
    fn outer_voice_store(&self) -> Arc<dyn OuterVoiceStore>;
    fn autonomy_strategy_store(&self) -> Arc<dyn AutonomyStrategyStore>;
    fn inner_life_store(&self) -> Arc<dyn InnerLifeStore>;
    fn felt_significance_store(&self) -> Arc<dyn FeltSignificanceStore>;
    fn temperament_continuity_store(&self) -> Arc<dyn TemperamentContinuityStore>;
    fn inner_conflict_store(&self) -> Arc<dyn InnerConflictStore>;
    fn mental_privacy_store(&self) -> Arc<dyn MentalPrivacyStore>;
    fn private_doc_store(&self) -> Arc<dyn PrivateDocStore>;
    fn private_garden_store(&self) -> Arc<dyn PrivateGardenStore>;
    fn turn_continuity_evidence_store(&self) -> Arc<dyn TurnContinuityEvidenceStore>;
    fn remind_at_store(&self) -> Arc<dyn RemindAtStore>;
    fn task_store(&self) -> Arc<dyn TaskStore>;
    fn task_run_store(&self) -> Arc<dyn TaskRunStore>;
    fn task_artifact_store(&self) -> Arc<dyn TaskArtifactStore>;
    fn task_learning_store(&self) -> Arc<dyn TaskLearningStore>;
}

pub fn state_mount_path() -> PathBuf {
    if let Some(path) = std::env::var_os("BEETLE_MEMORY_STATE_DIR") {
        return PathBuf::from(path);
    }
    #[cfg(test)]
    {
        let thread_id = format!("{:?}", std::thread::current().id())
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
            .collect::<String>();
        std::env::temp_dir().join(format!(
            "beetle-memory-core-test-{}-{}",
            std::process::id(),
            thread_id
        ))
    }
    #[cfg(not(test))]
    {
        PathBuf::from(".")
    }
}

#[cfg(feature = "sqlite-index")]
pub fn sqlite_index_state_dir() -> crate::error::Result<Option<PathBuf>> {
    let Some(path) = std::env::var_os("BEETLE_MEMORY_STATE_DIR") else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(crate::error::Error::config(
            "sqlite_index_state_dir",
            "BEETLE_MEMORY_STATE_DIR must be an absolute path when sqlite-index is enabled",
        ));
    }
    Ok(Some(path))
}

pub mod task_wdt {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TaskWdtThreadPolicy {
        Owner,
        FeedOnly,
        None,
    }

    pub fn feed_current_task() {}
    pub fn register_current_task_to_task_wdt() {}
    pub fn unregister_current_task_from_task_wdt() {}
    pub fn thread_policy_for_name(_name: &str) -> TaskWdtThreadPolicy {
        match _name {
            "agent_loop" | "wifi_worker" | "audio_io_worker" | "runtime_bootstrap" => {
                TaskWdtThreadPolicy::Owner
            }
            "http_snapshot_exec"
            | "http_chat_history_exec"
            | "http_config_exec"
            | "http_diag_exec"
            | "dispatch"
            | "os_outbound"
            | "bg_timer"
            | "heartbeat"
            | "external_channel_ws"
            | "external_channel_stream"
            | "external_channel_bot"
            | "external_channel_sender"
            | "external_channel_poll"
            | "voice_session"
            | "voice_session_worker"
            | "voice_realtime"
            | "voice_realtime_connect"
            | "display" => TaskWdtThreadPolicy::FeedOnly,
            _ => TaskWdtThreadPolicy::None,
        }
    }
}

pub mod time {
    pub fn monotonic_ms() -> u64 {
        crate::util::current_unix_ms()
    }

    pub fn wall_clock_is_trustworthy() -> bool {
        true
    }

    pub fn uptime_secs() -> u64 {
        crate::util::current_unix_secs()
    }
}

pub mod task_affinity {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TaskCore {
        Core0,
        Core1,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum TaskSpawnSurface {
        HostThread,
        StdThreadCompat,
        EspNativeTask,
    }

    pub type TaskHandle = std::thread::JoinHandle<()>;

    pub fn planned_spawn_surface(_name: &str) -> TaskSpawnSurface {
        TaskSpawnSurface::StdThreadCompat
    }

    pub fn spawn_named_with_affinity<F>(
        name: String,
        _stack_size: usize,
        _core: Option<TaskCore>,
        f: F,
    ) -> std::io::Result<TaskHandle>
    where
        F: FnOnce() + Send + 'static,
    {
        std::thread::Builder::new().name(name).spawn(f)
    }
}
