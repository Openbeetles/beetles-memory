//! Compile-time feature and profile capability contracts.

use serde::{Deserialize, Serialize};

#[cfg(any(
    all(feature = "target-esp", feature = "target-linux-device"),
    all(feature = "target-esp", feature = "target-desktop-macos"),
    all(feature = "target-esp", feature = "target-desktop-windows"),
    all(feature = "target-esp", feature = "target-server-linux"),
    all(feature = "target-linux-device", feature = "target-desktop-macos"),
    all(feature = "target-linux-device", feature = "target-desktop-windows"),
    all(feature = "target-linux-device", feature = "target-server-linux"),
    all(feature = "target-desktop-macos", feature = "target-desktop-windows"),
    all(feature = "target-desktop-macos", feature = "target-server-linux"),
    all(feature = "target-desktop-windows", feature = "target-server-linux")
))]
compile_error!("Beetle Memory feature contract requires at most one target-* feature per build.");

#[cfg(any(
    all(feature = "role-standalone-memory", feature = "role-embedded-sdk"),
    all(feature = "role-standalone-memory", feature = "role-memory-gateway"),
    all(feature = "role-standalone-memory", feature = "role-dev-full"),
    all(feature = "role-embedded-sdk", feature = "role-memory-gateway"),
    all(feature = "role-embedded-sdk", feature = "role-dev-full"),
    all(feature = "role-memory-gateway", feature = "role-dev-full")
))]
compile_error!("Beetle Memory feature contract requires at most one role-* feature per build.");

#[cfg(all(feature = "target-esp", feature = "sqlite-index"))]
compile_error!(
    "target-esp builds must not enable sqlite-index; use heuristic/compact recall backends."
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetFeature {
    Esp,
    LinuxDevice,
    DesktopMacos,
    DesktopWindows,
    ServerLinux,
}

impl TargetFeature {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Esp => "target-esp",
            Self::LinuxDevice => "target-linux-device",
            Self::DesktopMacos => "target-desktop-macos",
            Self::DesktopWindows => "target-desktop-windows",
            Self::ServerLinux => "target-server-linux",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoleFeature {
    StandaloneMemory,
    EmbeddedSdk,
    MemoryGateway,
    DevFull,
}

impl RoleFeature {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StandaloneMemory => "role-standalone-memory",
            Self::EmbeddedSdk => "role-embedded-sdk",
            Self::MemoryGateway => "role-memory-gateway",
            Self::DevFull => "role-dev-full",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileId {
    EspStandaloneMemory,
    EspEmbeddedSdk,
    LinuxDeviceStandaloneMemory,
    DesktopMacosEmbeddedSdk,
    DesktopWindowsEmbeddedSdk,
    ServerLinuxMemoryGateway,
    ServerLinuxDevFull,
}

impl ProfileId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EspStandaloneMemory => "profile-esp-standalone-memory",
            Self::EspEmbeddedSdk => "profile-esp-embedded-sdk",
            Self::LinuxDeviceStandaloneMemory => "target-linux-device+role-standalone-memory",
            Self::DesktopMacosEmbeddedSdk => "target-desktop-macos+role-embedded-sdk",
            Self::DesktopWindowsEmbeddedSdk => "target-desktop-windows+role-embedded-sdk",
            Self::ServerLinuxMemoryGateway => "target-server-linux+role-memory-gateway",
            Self::ServerLinuxDevFull => "target-server-linux+role-dev-full",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileCapabilityCatalogEntry {
    pub profile: ProfileId,
    pub target: TargetFeature,
    pub role: RoleFeature,
    pub sqlite_index_allowed: bool,
    pub lexical_archive_recall: bool,
    pub heuristic_runtime_skill_recall: bool,
    pub heuristic_task_learning_recall: bool,
    pub indexed_archive_recall_allowed: bool,
    pub indexed_continuity_capsule_recall_allowed: bool,
    pub indexed_runtime_skill_recall_allowed: bool,
    pub indexed_task_learning_recall_allowed: bool,
    pub communication_adapter_allowed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledFeatureReport {
    pub target_esp: bool,
    pub target_linux_device: bool,
    pub target_desktop_macos: bool,
    pub target_desktop_windows: bool,
    pub target_server_linux: bool,
    pub role_standalone_memory: bool,
    pub role_embedded_sdk: bool,
    pub role_memory_gateway: bool,
    pub role_dev_full: bool,
    pub profile_esp_standalone_memory: bool,
    pub profile_esp_embedded_sdk: bool,
    pub profile_linux_device_standalone_memory: bool,
    pub profile_desktop_macos_embedded_sdk: bool,
    pub profile_desktop_windows_embedded_sdk: bool,
    pub profile_server_linux_memory_gateway: bool,
    pub profile_server_linux_dev_full: bool,
    pub replay_harness_compiled: bool,
    pub sqlite_index_compiled: bool,
    pub rusqlite_dependency_compiled: bool,
}

pub const fn compiled_feature_report() -> CompiledFeatureReport {
    CompiledFeatureReport {
        target_esp: cfg!(feature = "target-esp"),
        target_linux_device: cfg!(feature = "target-linux-device"),
        target_desktop_macos: cfg!(feature = "target-desktop-macos"),
        target_desktop_windows: cfg!(feature = "target-desktop-windows"),
        target_server_linux: cfg!(feature = "target-server-linux"),
        role_standalone_memory: cfg!(feature = "role-standalone-memory"),
        role_embedded_sdk: cfg!(feature = "role-embedded-sdk"),
        role_memory_gateway: cfg!(feature = "role-memory-gateway"),
        role_dev_full: cfg!(feature = "role-dev-full"),
        profile_esp_standalone_memory: cfg!(feature = "profile-esp-standalone-memory"),
        profile_esp_embedded_sdk: cfg!(feature = "profile-esp-embedded-sdk"),
        profile_linux_device_standalone_memory: cfg!(
            feature = "profile-linux-device-standalone-memory"
        ),
        profile_desktop_macos_embedded_sdk: cfg!(feature = "profile-desktop-macos-embedded-sdk"),
        profile_desktop_windows_embedded_sdk: cfg!(
            feature = "profile-desktop-windows-embedded-sdk"
        ),
        profile_server_linux_memory_gateway: cfg!(feature = "profile-server-linux-memory-gateway"),
        profile_server_linux_dev_full: cfg!(feature = "profile-server-linux-dev-full"),
        replay_harness_compiled: cfg!(feature = "replay-harness"),
        sqlite_index_compiled: sqlite_index_compiled(),
        rusqlite_dependency_compiled: sqlite_index_compiled(),
    }
}

pub const fn sqlite_index_compiled() -> bool {
    cfg!(feature = "sqlite-index")
}

pub const fn profile_capability_catalog() -> &'static [ProfileCapabilityCatalogEntry] {
    &PROFILE_CAPABILITY_CATALOG
}

const PROFILE_CAPABILITY_CATALOG: [ProfileCapabilityCatalogEntry; 7] = [
    ProfileCapabilityCatalogEntry {
        profile: ProfileId::EspStandaloneMemory,
        target: TargetFeature::Esp,
        role: RoleFeature::StandaloneMemory,
        sqlite_index_allowed: false,
        lexical_archive_recall: true,
        heuristic_runtime_skill_recall: true,
        heuristic_task_learning_recall: true,
        indexed_archive_recall_allowed: false,
        indexed_continuity_capsule_recall_allowed: false,
        indexed_runtime_skill_recall_allowed: false,
        indexed_task_learning_recall_allowed: false,
        communication_adapter_allowed: false,
    },
    ProfileCapabilityCatalogEntry {
        profile: ProfileId::EspEmbeddedSdk,
        target: TargetFeature::Esp,
        role: RoleFeature::EmbeddedSdk,
        sqlite_index_allowed: false,
        lexical_archive_recall: true,
        heuristic_runtime_skill_recall: true,
        heuristic_task_learning_recall: true,
        indexed_archive_recall_allowed: false,
        indexed_continuity_capsule_recall_allowed: false,
        indexed_runtime_skill_recall_allowed: false,
        indexed_task_learning_recall_allowed: false,
        communication_adapter_allowed: false,
    },
    ProfileCapabilityCatalogEntry {
        profile: ProfileId::LinuxDeviceStandaloneMemory,
        target: TargetFeature::LinuxDevice,
        role: RoleFeature::StandaloneMemory,
        sqlite_index_allowed: true,
        lexical_archive_recall: true,
        heuristic_runtime_skill_recall: true,
        heuristic_task_learning_recall: true,
        indexed_archive_recall_allowed: true,
        indexed_continuity_capsule_recall_allowed: true,
        indexed_runtime_skill_recall_allowed: true,
        indexed_task_learning_recall_allowed: true,
        communication_adapter_allowed: false,
    },
    ProfileCapabilityCatalogEntry {
        profile: ProfileId::DesktopMacosEmbeddedSdk,
        target: TargetFeature::DesktopMacos,
        role: RoleFeature::EmbeddedSdk,
        sqlite_index_allowed: true,
        lexical_archive_recall: true,
        heuristic_runtime_skill_recall: true,
        heuristic_task_learning_recall: true,
        indexed_archive_recall_allowed: true,
        indexed_continuity_capsule_recall_allowed: true,
        indexed_runtime_skill_recall_allowed: true,
        indexed_task_learning_recall_allowed: true,
        communication_adapter_allowed: false,
    },
    ProfileCapabilityCatalogEntry {
        profile: ProfileId::DesktopWindowsEmbeddedSdk,
        target: TargetFeature::DesktopWindows,
        role: RoleFeature::EmbeddedSdk,
        sqlite_index_allowed: true,
        lexical_archive_recall: true,
        heuristic_runtime_skill_recall: true,
        heuristic_task_learning_recall: true,
        indexed_archive_recall_allowed: true,
        indexed_continuity_capsule_recall_allowed: true,
        indexed_runtime_skill_recall_allowed: true,
        indexed_task_learning_recall_allowed: true,
        communication_adapter_allowed: false,
    },
    ProfileCapabilityCatalogEntry {
        profile: ProfileId::ServerLinuxMemoryGateway,
        target: TargetFeature::ServerLinux,
        role: RoleFeature::MemoryGateway,
        sqlite_index_allowed: true,
        lexical_archive_recall: true,
        heuristic_runtime_skill_recall: true,
        heuristic_task_learning_recall: true,
        indexed_archive_recall_allowed: true,
        indexed_continuity_capsule_recall_allowed: true,
        indexed_runtime_skill_recall_allowed: true,
        indexed_task_learning_recall_allowed: true,
        communication_adapter_allowed: true,
    },
    ProfileCapabilityCatalogEntry {
        profile: ProfileId::ServerLinuxDevFull,
        target: TargetFeature::ServerLinux,
        role: RoleFeature::DevFull,
        sqlite_index_allowed: true,
        lexical_archive_recall: true,
        heuristic_runtime_skill_recall: true,
        heuristic_task_learning_recall: true,
        indexed_archive_recall_allowed: true,
        indexed_continuity_capsule_recall_allowed: true,
        indexed_runtime_skill_recall_allowed: true,
        indexed_task_learning_recall_allowed: true,
        communication_adapter_allowed: true,
    },
];
