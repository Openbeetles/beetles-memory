use std::path::PathBuf;

use bm_core::feature_gate::ProfileId;
use bm_core::platform::MemorySystemKind;
use bm_core::{Error, Result};

use crate::event::StoreEventScope;
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
}

impl StoreCapacityBudget {
    pub const fn full() -> Self {
        Self {
            event_log_max_items: 20_000,
            kv_max_entries: 20_000,
            blob_max_bytes: 64 * 1024 * 1024,
            snapshot_max_bytes: 16 * 1024 * 1024,
        }
    }

    pub const fn embedded_standalone() -> Self {
        Self {
            event_log_max_items: 2_048,
            kv_max_entries: 4_096,
            blob_max_bytes: 4 * 1024 * 1024,
            snapshot_max_bytes: 1024 * 1024,
        }
    }

    pub const fn embedded_sdk() -> Self {
        Self {
            event_log_max_items: 256,
            kv_max_entries: 512,
            blob_max_bytes: 1024 * 1024,
            snapshot_max_bytes: 256 * 1024,
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

    fn new(
        backend: StoreBackendKind,
        profile: ProfileId,
        data_path: Option<PathBuf>,
    ) -> Result<Self> {
        validate_backend_profile(backend, profile)?;
        let memory_system_kind = profile_memory_system_kind(profile);
        let capacity = default_capacity(backend, profile);
        Ok(Self {
            backend,
            profile,
            memory_system_kind,
            event_scope: StoreEventScope::system(memory_system_kind.as_str()),
            data_path,
            capacity,
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

fn default_capacity(backend: StoreBackendKind, profile: ProfileId) -> StoreCapacityBudget {
    match (backend, profile) {
        (StoreBackendKind::Embedded, ProfileId::EspStandaloneMemory) => {
            StoreCapacityBudget::embedded_standalone()
        }
        (StoreBackendKind::Embedded, ProfileId::EspEmbeddedSdk) => {
            StoreCapacityBudget::embedded_sdk()
        }
        (_, ProfileId::EspEmbeddedSdk) => StoreCapacityBudget::embedded_sdk(),
        (_, ProfileId::EspStandaloneMemory) => StoreCapacityBudget::embedded_standalone(),
        _ => StoreCapacityBudget::full(),
    }
}
