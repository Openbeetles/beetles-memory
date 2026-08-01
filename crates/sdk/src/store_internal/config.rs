use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "nonproduction-replay-harness")]
use bm_core::budget::NonproductionRuntimeBudgetLimits;
use bm_core::budget::{
    RuntimeBudgetAuthority, RuntimeStoreMedium, StaticPlatformManifest, StoreRuntimeBudget,
};
use bm_core::feature_gate::{ProfileId, TargetFeature};
use bm_core::platform::MemorySystemKind;
use bm_core::resource::{
    HostRuntimeResourceProbe, RuntimeResourceProbe, RuntimeResourceUnavailableReason,
    UnavailableRuntimeResourceProbe,
};
use bm_core::{Error, Result};

use crate::store_internal::event::{MemoryStoreEvent, StoreEventScope};
use crate::store_internal::schema::STORE_SCHEMA_ID;

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
    pub metric_source_max_items: usize,
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
            metric_source_max_items: budget.metric_source_max_items,
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

    pub const fn into_runtime_budget(self) -> StoreRuntimeBudget {
        StoreRuntimeBudget {
            metric_source_max_items: self.metric_source_max_items,
            event_log_max_items: self.event_log_max_items,
            kv_max_entries: self.kv_max_entries,
            blob_max_bytes: self.blob_max_bytes,
            snapshot_max_bytes: self.snapshot_max_bytes,
            logical_namespace_max_bytes: self.logical_namespace_max_bytes,
            logical_key_max_bytes: self.logical_key_max_bytes,
            event_record_key_max_bytes: self.event_record_key_max_bytes,
            export_max_bytes: self.export_max_bytes,
            import_max_bytes: self.import_max_bytes,
        }
    }

    pub const fn admits_runtime_budget(self, budget: StoreRuntimeBudget) -> bool {
        self.metric_source_max_items >= budget.metric_source_max_items
            && self.event_log_max_items >= budget.event_log_max_items
            && self.kv_max_entries >= budget.kv_max_entries
            && self.blob_max_bytes >= budget.blob_max_bytes
            && self.snapshot_max_bytes >= budget.snapshot_max_bytes
            && self.logical_namespace_max_bytes >= budget.logical_namespace_max_bytes
            && self.logical_key_max_bytes >= budget.logical_key_max_bytes
            && self.event_record_key_max_bytes >= budget.event_record_key_max_bytes
            && self.export_max_bytes >= budget.export_max_bytes
            && self.import_max_bytes >= budget.import_max_bytes
    }

    pub const fn full() -> Self {
        Self {
            metric_source_max_items: 8,
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
            metric_source_max_items: 1,
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
            metric_source_max_items: 1,
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

    pub const fn desktop_linux() -> Self {
        Self {
            max_file_name_bytes: 128,
            max_directory_name_bytes: 128,
            max_relative_path_bytes: 1024,
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
    pub(crate) backend: StoreBackendKind,
    pub(crate) profile: ProfileId,
    pub(crate) memory_system_kind: MemorySystemKind,
    pub(crate) event_scope: StoreEventScope,
    pub(crate) data_path: Option<PathBuf>,
    pub(crate) path_budget: StorePathBudget,
    pub(crate) repair_policy: StoreRepairPolicy,
    pub(crate) fsync: bool,
    pub(crate) lock_timeout: Duration,
    pub(crate) schema_id: &'static str,
    #[cfg(feature = "nonproduction-replay-harness")]
    nonproduction_runtime_budget_limits: NonproductionRuntimeBudgetLimits,
}

impl StoreBackendConfig {
    pub fn for_backend(
        backend: StoreBackendKind,
        data_path: Option<PathBuf>,
        profile: ProfileId,
    ) -> Result<Self> {
        match (backend, data_path) {
            (StoreBackendKind::InMemory, None) => Self::in_memory(profile),
            (StoreBackendKind::File, Some(path)) => Self::file(path, profile),
            (StoreBackendKind::Sqlite, Some(path)) => Self::sqlite(path, profile),
            (StoreBackendKind::Embedded, None) => Self::embedded(profile),
            (StoreBackendKind::InMemory | StoreBackendKind::Embedded, Some(_)) => Err(
                Error::config("store_backend_config", "backend_rejects_data_path"),
            ),
            (StoreBackendKind::File | StoreBackendKind::Sqlite, None) => Err(Error::config(
                "store_backend_config",
                "persistent_backend_requires_data_path",
            )),
        }
    }

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

    pub const fn backend(&self) -> StoreBackendKind {
        self.backend
    }

    pub const fn profile(&self) -> ProfileId {
        self.profile
    }

    pub fn data_path(&self) -> Option<&std::path::Path> {
        self.data_path.as_deref()
    }

    pub const fn memory_system_kind(&self) -> MemorySystemKind {
        self.memory_system_kind
    }

    pub fn event_scope(&self) -> &StoreEventScope {
        &self.event_scope
    }

    pub const fn path_budget(&self) -> StorePathBudget {
        self.path_budget
    }

    pub const fn repair_policy(&self) -> StoreRepairPolicy {
        self.repair_policy
    }

    pub const fn fsync(&self) -> bool {
        self.fsync
    }

    pub const fn lock_timeout(&self) -> Duration {
        self.lock_timeout
    }

    pub const fn schema_id(&self) -> &'static str {
        self.schema_id
    }

    pub fn with_repair_policy(mut self, repair_policy: StoreRepairPolicy) -> Self {
        self.repair_policy = repair_policy;
        self
    }

    pub fn with_fsync(mut self, fsync: bool) -> Self {
        self.fsync = fsync;
        self
    }

    pub fn with_lock_timeout(mut self, lock_timeout: Duration) -> Self {
        self.lock_timeout = lock_timeout;
        self
    }

    pub fn with_event_scope(mut self, event_scope: StoreEventScope) -> Self {
        self.event_scope = event_scope;
        self
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn with_nonproduction_runtime_budget_limits(
        mut self,
        limits: NonproductionRuntimeBudgetLimits,
    ) -> Self {
        self.nonproduction_runtime_budget_limits = limits;
        self
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn try_with_nonproduction_store_budget_limit(
        mut self,
        budget: StoreRuntimeBudget,
    ) -> Result<Self> {
        self.nonproduction_runtime_budget_limits = self
            .nonproduction_runtime_budget_limits
            .try_with_store_budget_limit(budget)?;
        Ok(self)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn nonproduction_runtime_budget_limits(&self) -> NonproductionRuntimeBudgetLimits {
        self.nonproduction_runtime_budget_limits.clone()
    }

    fn new(
        backend: StoreBackendKind,
        profile: ProfileId,
        data_path: Option<PathBuf>,
    ) -> Result<Self> {
        validate_backend_profile(backend, profile)?;
        validate_data_path(backend, data_path.as_deref())?;
        let memory_system_kind = profile_memory_system_kind(profile);
        let path_budget = default_path_budget(profile);
        Ok(Self {
            backend,
            profile,
            memory_system_kind,
            event_scope: StoreEventScope::system(memory_system_kind.as_str()),
            data_path,
            path_budget,
            repair_policy: StoreRepairPolicy::ReportOnly,
            fsync: true,
            lock_timeout: Duration::from_secs(5),
            schema_id: STORE_SCHEMA_ID,
            #[cfg(feature = "nonproduction-replay-harness")]
            nonproduction_runtime_budget_limits: NonproductionRuntimeBudgetLimits::new(),
        })
    }
}

fn validate_data_path(
    backend: StoreBackendKind,
    data_path: Option<&std::path::Path>,
) -> Result<()> {
    match (backend, data_path) {
        (StoreBackendKind::File | StoreBackendKind::Sqlite, Some(path)) if !path.is_absolute() => {
            Err(Error::config(
                "store_backend_config",
                "persistent store data_path must be absolute",
            ))
        }
        _ => Ok(()),
    }
}

pub(crate) fn open_runtime_budget_authority(
    config: &StoreBackendConfig,
    firmware_probe: Option<Arc<dyn RuntimeResourceProbe>>,
    now_secs: u64,
) -> Result<RuntimeBudgetAuthority> {
    let store_medium = runtime_store_medium(config.backend);
    let manifest = StaticPlatformManifest::for_profile(config.profile, store_medium);
    #[cfg(not(feature = "nonproduction-replay-harness"))]
    if config.profile.target() != TargetFeature::Esp && firmware_probe.is_some() {
        return Err(Error::config(
            "runtime_budget_authority_config",
            "firmware_probe_requires_esp_profile",
        ));
    }
    #[cfg(feature = "nonproduction-replay-harness")]
    {
        let limits = config.nonproduction_runtime_budget_limits();
        if config.profile.target() == TargetFeature::Esp {
            RuntimeBudgetAuthority::with_nonproduction_firmware_probe(
                config.profile,
                manifest,
                None,
                firmware_probe.unwrap_or_else(unavailable_firmware_probe),
                limits,
                now_secs,
            )
        } else {
            RuntimeBudgetAuthority::with_nonproduction_host_probe(
                config.profile,
                manifest,
                None,
                firmware_probe.unwrap_or(Arc::new(host_resource_probe(config)?)),
                limits,
                now_secs,
            )
        }
    }
    #[cfg(not(feature = "nonproduction-replay-harness"))]
    if config.profile.target() == TargetFeature::Esp {
        RuntimeBudgetAuthority::with_firmware_probe(
            config.profile,
            manifest,
            None,
            firmware_probe.unwrap_or_else(unavailable_firmware_probe),
            now_secs,
        )
    } else {
        RuntimeBudgetAuthority::with_host_probe(
            config.profile,
            manifest,
            None,
            host_resource_probe(config)?,
            now_secs,
        )
    }
}

const fn runtime_store_medium(backend: StoreBackendKind) -> RuntimeStoreMedium {
    match backend {
        StoreBackendKind::InMemory => RuntimeStoreMedium::VolatileMemory,
        StoreBackendKind::File | StoreBackendKind::Sqlite => {
            RuntimeStoreMedium::PersistentFilesystem
        }
        StoreBackendKind::Embedded => RuntimeStoreMedium::EmbeddedFlash,
    }
}

fn host_resource_probe(config: &StoreBackendConfig) -> Result<HostRuntimeResourceProbe> {
    match runtime_store_medium(config.backend) {
        RuntimeStoreMedium::VolatileMemory => Ok(HostRuntimeResourceProbe::for_volatile_memory()),
        RuntimeStoreMedium::PersistentFilesystem => {
            HostRuntimeResourceProbe::for_persistent_filesystem(
                config.data_path.as_ref().ok_or_else(|| {
                    Error::config(
                        "runtime_budget_authority_config",
                        "persistent_store_requires_data_path",
                    )
                })?,
            )
        }
        RuntimeStoreMedium::EmbeddedFlash => Err(Error::config(
            "runtime_budget_authority_config",
            "host_profile_cannot_use_embedded_flash",
        )),
    }
}

fn unavailable_firmware_probe() -> Arc<dyn RuntimeResourceProbe> {
    Arc::new(UnavailableRuntimeResourceProbe::new(
        RuntimeResourceUnavailableReason::ProbeNotConfigured,
    ))
}

pub(crate) fn resolve_store_capacity(
    authority: &RuntimeBudgetAuthority,
) -> Result<StoreCapacityBudget> {
    Ok(StoreCapacityBudget::from_runtime_budget(
        authority.admission_store_ceiling(),
    ))
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
        | ProfileId::DesktopLinuxEmbeddedSdk
        | ProfileId::DesktopWindowsEmbeddedSdk => MemorySystemKind::SdkEmbedded,
        ProfileId::EspStandaloneMemory
        | ProfileId::LinuxDeviceStandaloneMemory
        | ProfileId::DesktopMacosStandaloneMemory
        | ProfileId::DesktopMacosDevFull
        | ProfileId::DesktopWindowsDevFull
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

pub const fn default_path_budget(profile: ProfileId) -> StorePathBudget {
    match profile {
        ProfileId::EspStandaloneMemory | ProfileId::EspEmbeddedSdk => {
            StorePathBudget::esp_compact()
        }
        ProfileId::LinuxDeviceStandaloneMemory => StorePathBudget::linux_device(),
        ProfileId::DesktopMacosStandaloneMemory
        | ProfileId::DesktopMacosEmbeddedSdk
        | ProfileId::DesktopMacosDevFull => StorePathBudget::desktop_macos(),
        ProfileId::DesktopLinuxEmbeddedSdk => StorePathBudget::desktop_linux(),
        ProfileId::DesktopWindowsEmbeddedSdk | ProfileId::DesktopWindowsDevFull => {
            StorePathBudget::desktop_windows()
        }
        ProfileId::ServerLinuxMemoryGateway | ProfileId::ServerLinuxDevFull => {
            StorePathBudget::server_linux()
        }
    }
}
