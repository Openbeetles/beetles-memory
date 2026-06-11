use std::path::PathBuf;

use bm_core::budget::{RuntimeBudgetReport, StoreRuntimeBudget};
use bm_core::feature_gate::ProfileId;
use bm_core::platform::MemorySystemKind;
use bm_core::{Error, Result};

use crate::event::{MemoryStoreEvent, StoreEventScope};
use crate::schema::STORE_SCHEMA_ID;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreBackendKind {
    InMemory,
    File,
    Sqlite,
    Embedded,
}

impl StoreBackendKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InMemory => "in_memory",
            Self::File => "file",
            Self::Sqlite => "sqlite",
            Self::Embedded => "embedded",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreRepairPolicy {
    ReportOnly,
    RepairSafe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreCapacityBudget {
    pub event_log_max_items: usize,
    pub kv_max_entries: usize,
    pub blob_max_bytes: usize,
    pub snapshot_max_bytes: usize,
    pub logical_namespace_max_bytes: usize,
    pub logical_key_max_bytes: usize,
    pub event_record_key_max_bytes: usize,
    pub export_max_bytes: usize,
    pub import_max_bytes: usize,
}

impl StoreCapacityBudget {
    pub const fn from_runtime_budget(budget: StoreRuntimeBudget) -> Self {
        Self {
            event_log_max_items: budget.event_log_max_items,
            kv_max_entries: budget.kv_max_entries,
            blob_max_bytes: budget.blob_max_bytes,
            snapshot_max_bytes: budget.snapshot_max_bytes,
            logical_namespace_max_bytes: budget.logical_namespace_max_bytes,
            logical_key_max_bytes: budget.logical_key_max_bytes,
            event_record_key_max_bytes: budget.event_record_key_max_bytes,
            export_max_bytes: budget.export_max_bytes,
            import_max_bytes: budget.import_max_bytes,
        }
    }

    pub const fn full() -> Self {
        Self {
            event_log_max_items: 20_000,
            kv_max_entries: 20_000,
            blob_max_bytes: 64 * 1024 * 1024,
            snapshot_max_bytes: 16 * 1024 * 1024,
            logical_namespace_max_bytes: 128,
            logical_key_max_bytes: 8192,
            event_record_key_max_bytes: 8192,
            export_max_bytes: 16 * 1024 * 1024,
            import_max_bytes: 16 * 1024 * 1024,
        }
    }

    pub const fn embedded_standalone() -> Self {
        Self {
            event_log_max_items: 2_048,
            kv_max_entries: 4_096,
            blob_max_bytes: 4 * 1024 * 1024,
            snapshot_max_bytes: 1024 * 1024,
            logical_namespace_max_bytes: 96,
            logical_key_max_bytes: 1024,
            event_record_key_max_bytes: 1024,
            export_max_bytes: 1024 * 1024,
            import_max_bytes: 1024 * 1024,
        }
    }

    pub const fn embedded_sdk() -> Self {
        Self {
            event_log_max_items: 256,
            kv_max_entries: 512,
            blob_max_bytes: 1024 * 1024,
            snapshot_max_bytes: 256 * 1024,
            logical_namespace_max_bytes: 96,
            logical_key_max_bytes: 512,
            event_record_key_max_bytes: 512,
            export_max_bytes: 256 * 1024,
            import_max_bytes: 256 * 1024,
        }
    }
}

pub(crate) fn store_budget_error(message: impl Into<String>) -> Error {
    Error::config("store_budget_exceeded", message.into())
}

pub(crate) fn enforce_logical_key_budget(
    capacity: StoreCapacityBudget,
    namespace: &str,
    key: &str,
    label: &'static str,
) -> Result<()> {
    if namespace.len() > capacity.logical_namespace_max_bytes {
        return Err(store_budget_error(format!(
            "{label} namespace bytes {} exceed {}",
            namespace.len(),
            capacity.logical_namespace_max_bytes
        )));
    }
    if key.len() > capacity.logical_key_max_bytes {
        return Err(store_budget_error(format!(
            "{label} logical key bytes {} exceed {}",
            key.len(),
            capacity.logical_key_max_bytes
        )));
    }
    Ok(())
}

pub(crate) fn enforce_event_key_budget(
    capacity: StoreCapacityBudget,
    event: &MemoryStoreEvent,
    label: &'static str,
) -> Result<()> {
    if event.record_key.len() > capacity.event_record_key_max_bytes {
        return Err(store_budget_error(format!(
            "{label} event record key bytes {} exceed {}",
            event.record_key.len(),
            capacity.event_record_key_max_bytes
        )));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorePathBudget {
    pub max_file_name_bytes: usize,
    pub max_directory_name_bytes: usize,
    pub max_relative_path_bytes: usize,
    pub physical_key_digest_hex_chars: usize,
}

impl StorePathBudget {
    pub const fn esp_compact() -> Self {
        Self {
            max_file_name_bytes: 32,
            max_directory_name_bytes: 48,
            max_relative_path_bytes: 160,
            physical_key_digest_hex_chars: 24,
        }
    }

    pub const fn linux_device() -> Self {
        Self {
            max_file_name_bytes: 64,
            max_directory_name_bytes: 64,
            max_relative_path_bytes: 384,
            physical_key_digest_hex_chars: 24,
        }
    }

    pub const fn desktop_macos() -> Self {
        Self {
            max_file_name_bytes: 96,
            max_directory_name_bytes: 96,
            max_relative_path_bytes: 512,
            physical_key_digest_hex_chars: 24,
        }
    }

    pub const fn desktop_windows() -> Self {
        Self {
            max_file_name_bytes: 64,
            max_directory_name_bytes: 64,
            max_relative_path_bytes: 240,
            physical_key_digest_hex_chars: 24,
        }
    }

    pub const fn server_linux() -> Self {
        Self {
            max_file_name_bytes: 128,
            max_directory_name_bytes: 128,
            max_relative_path_bytes: 1024,
            physical_key_digest_hex_chars: 24,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreBackendConfig {
    pub backend: StoreBackendKind,
    pub profile: ProfileId,
    pub memory_system_kind: MemorySystemKind,
    pub event_scope: StoreEventScope,
    pub data_path: Option<PathBuf>,
    pub capacity: StoreCapacityBudget,
    pub path_budget: StorePathBudget,
    pub repair_policy: StoreRepairPolicy,
    pub fsync: bool,
    pub schema_id: &'static str,
}

impl StoreBackendConfig {
    pub fn in_memory(profile: ProfileId) -> Result<Self> {
        Self::new(StoreBackendKind::InMemory, profile, None)
    }

    pub fn file(root: impl Into<PathBuf>, profile: ProfileId) -> Result<Self> {
        Self::new(StoreBackendKind::File, profile, Some(root.into()))
    }

    pub fn sqlite(path: impl Into<PathBuf>, profile: ProfileId) -> Result<Self> {
        Self::new(StoreBackendKind::Sqlite, profile, Some(path.into()))
    }

    pub fn embedded(profile: ProfileId) -> Result<Self> {
        Self::new(StoreBackendKind::Embedded, profile, None)
    }

    pub fn with_repair_policy(mut self, repair_policy: StoreRepairPolicy) -> Self {
        self.repair_policy = repair_policy;
        self
    }

    pub fn with_fsync(mut self, fsync: bool) -> Self {
        self.fsync = fsync;
        self
    }

    pub fn with_event_scope(mut self, event_scope: StoreEventScope) -> Self {
        self.event_scope = event_scope;
        self
    }

    pub fn with_runtime_store_budget(mut self, budget: StoreRuntimeBudget) -> Self {
        self.capacity = StoreCapacityBudget::from_runtime_budget(budget);
        self
    }

    fn new(
        backend: StoreBackendKind,
        profile: ProfileId,
        data_path: Option<PathBuf>,
    ) -> Result<Self> {
        validate_backend_profile(backend, profile)?;
        let memory_system_kind = profile_memory_system_kind(profile);
        let capacity = default_capacity(backend, profile);
        let path_budget = default_path_budget(profile);
        Ok(Self {
            backend,
            profile,
            memory_system_kind,
            event_scope: StoreEventScope::system(memory_system_kind.as_str()),
            data_path,
            capacity,
            path_budget,
            repair_policy: StoreRepairPolicy::ReportOnly,
            fsync: true,
            schema_id: STORE_SCHEMA_ID,
        })
    }
}

fn validate_backend_profile(backend: StoreBackendKind, profile: ProfileId) -> Result<()> {
    if profile_is_esp(profile) {
        match backend {
            StoreBackendKind::Sqlite => {
                return Err(Error::config(
                    "store_backend_config",
                    "target-esp profiles must not use sqlite store",
                ));
            }
            StoreBackendKind::File => {
                return Err(Error::config(
                    "store_backend_config",
                    "target-esp profiles must use embedded or in-memory store",
                ));
            }
            StoreBackendKind::InMemory | StoreBackendKind::Embedded => {}
        }
    }
    Ok(())
}

pub const fn profile_memory_system_kind(profile: ProfileId) -> MemorySystemKind {
    match profile {
        ProfileId::EspEmbeddedSdk
        | ProfileId::DesktopMacosEmbeddedSdk
        | ProfileId::DesktopWindowsEmbeddedSdk => MemorySystemKind::SdkEmbedded,
        ProfileId::EspStandaloneMemory
        | ProfileId::LinuxDeviceStandaloneMemory
        | ProfileId::DesktopMacosStandaloneMemory
        | ProfileId::ServerLinuxMemoryGateway
        | ProfileId::ServerLinuxDevFull => MemorySystemKind::Standalone,
    }
}

pub const fn profile_is_esp(profile: ProfileId) -> bool {
    matches!(
        profile,
        ProfileId::EspStandaloneMemory | ProfileId::EspEmbeddedSdk
    )
}

fn default_capacity(_backend: StoreBackendKind, profile: ProfileId) -> StoreCapacityBudget {
    StoreCapacityBudget::from_runtime_budget(
        RuntimeBudgetReport::static_for_profile(profile).store_budget,
    )
}

pub const fn default_path_budget(profile: ProfileId) -> StorePathBudget {
    match profile {
        ProfileId::EspStandaloneMemory | ProfileId::EspEmbeddedSdk => {
            StorePathBudget::esp_compact()
        }
        ProfileId::LinuxDeviceStandaloneMemory => StorePathBudget::linux_device(),
        ProfileId::DesktopMacosStandaloneMemory | ProfileId::DesktopMacosEmbeddedSdk => {
            StorePathBudget::desktop_macos()
        }
        ProfileId::DesktopWindowsEmbeddedSdk => StorePathBudget::desktop_windows(),
        ProfileId::ServerLinuxMemoryGateway | ProfileId::ServerLinuxDevFull => {
            StorePathBudget::server_linux()
        }
    }
}
