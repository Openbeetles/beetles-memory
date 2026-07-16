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

#[cfg(all(
    feature = "role-dev-full",
    not(feature = "nonproduction-replay-harness")
))]
compile_error!(
    "role-dev-full requires nonproduction-replay-harness; production builds must fail closed."
);

#[cfg(all(feature = "target-esp", feature = "sqlite-index"))]
compile_error!(
    "target-esp builds must not enable sqlite-index; use heuristic/compact recall backends."
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetFeature {
    #[serde(rename = "target-esp")]
    Esp,
    #[serde(rename = "target-linux-device")]
    LinuxDevice,
    #[serde(rename = "target-desktop-macos")]
    DesktopMacos,
    #[serde(rename = "target-desktop-windows")]
    DesktopWindows,
    #[serde(rename = "target-server-linux")]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoleFeature {
    #[serde(rename = "role-standalone-memory")]
    StandaloneMemory,
    #[serde(rename = "role-embedded-sdk")]
    EmbeddedSdk,
    #[serde(rename = "role-memory-gateway")]
    MemoryGateway,
    #[serde(rename = "role-dev-full")]
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
pub enum ProfileId {
    #[serde(rename = "target-esp+role-standalone-memory")]
    EspStandaloneMemory,
    #[serde(rename = "target-esp+role-embedded-sdk")]
    EspEmbeddedSdk,
    #[serde(rename = "target-linux-device+role-standalone-memory")]
    LinuxDeviceStandaloneMemory,
    #[serde(rename = "target-desktop-macos+role-standalone-memory")]
    DesktopMacosStandaloneMemory,
    #[serde(rename = "target-desktop-macos+role-embedded-sdk")]
    DesktopMacosEmbeddedSdk,
    #[serde(rename = "target-desktop-macos+role-dev-full")]
    DesktopMacosDevFull,
    #[serde(rename = "target-desktop-windows+role-embedded-sdk")]
    DesktopWindowsEmbeddedSdk,
    #[serde(rename = "target-desktop-windows+role-dev-full")]
    DesktopWindowsDevFull,
    #[serde(rename = "target-server-linux+role-memory-gateway")]
    ServerLinuxMemoryGateway,
    #[serde(rename = "target-server-linux+role-dev-full")]
    ServerLinuxDevFull,
}

impl ProfileId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EspStandaloneMemory => "target-esp+role-standalone-memory",
            Self::EspEmbeddedSdk => "target-esp+role-embedded-sdk",
            Self::LinuxDeviceStandaloneMemory => "target-linux-device+role-standalone-memory",
            Self::DesktopMacosStandaloneMemory => "target-desktop-macos+role-standalone-memory",
            Self::DesktopMacosEmbeddedSdk => "target-desktop-macos+role-embedded-sdk",
            Self::DesktopMacosDevFull => "target-desktop-macos+role-dev-full",
            Self::DesktopWindowsEmbeddedSdk => "target-desktop-windows+role-embedded-sdk",
            Self::DesktopWindowsDevFull => "target-desktop-windows+role-dev-full",
            Self::ServerLinuxMemoryGateway => "target-server-linux+role-memory-gateway",
            Self::ServerLinuxDevFull => "target-server-linux+role-dev-full",
        }
    }

    pub const fn target(self) -> TargetFeature {
        match self {
            Self::EspStandaloneMemory | Self::EspEmbeddedSdk => TargetFeature::Esp,
            Self::LinuxDeviceStandaloneMemory => TargetFeature::LinuxDevice,
            Self::DesktopMacosStandaloneMemory
            | Self::DesktopMacosEmbeddedSdk
            | Self::DesktopMacosDevFull => TargetFeature::DesktopMacos,
            Self::DesktopWindowsEmbeddedSdk | Self::DesktopWindowsDevFull => {
                TargetFeature::DesktopWindows
            }
            Self::ServerLinuxMemoryGateway | Self::ServerLinuxDevFull => TargetFeature::ServerLinux,
        }
    }

    pub const fn role(self) -> RoleFeature {
        match self {
            Self::EspStandaloneMemory
            | Self::LinuxDeviceStandaloneMemory
            | Self::DesktopMacosStandaloneMemory => RoleFeature::StandaloneMemory,
            Self::EspEmbeddedSdk
            | Self::DesktopMacosEmbeddedSdk
            | Self::DesktopWindowsEmbeddedSdk => RoleFeature::EmbeddedSdk,
            Self::ServerLinuxMemoryGateway => RoleFeature::MemoryGateway,
            Self::DesktopMacosDevFull | Self::DesktopWindowsDevFull | Self::ServerLinuxDevFull => {
                RoleFeature::DevFull
            }
        }
    }

    pub const fn native_dev_full() -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            Some(Self::DesktopMacosDevFull)
        }
        #[cfg(target_os = "windows")]
        {
            Some(Self::DesktopWindowsDevFull)
        }
        #[cfg(target_os = "linux")]
        {
            Some(Self::ServerLinuxDevFull)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            None
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
    pub llm_gateway_server_allowed: bool,
    pub adapter: ProfileAdapterCapabilityCatalog,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileAdapterTransportCapability {
    pub allowed: bool,
    pub client_allowed: bool,
    pub server_allowed: bool,
    pub private_data_allowed: bool,
}

impl ProfileAdapterTransportCapability {
    pub const fn forbidden() -> Self {
        Self {
            allowed: false,
            client_allowed: false,
            server_allowed: false,
            private_data_allowed: false,
        }
    }

    pub const fn local(private_data_allowed: bool) -> Self {
        Self {
            allowed: true,
            client_allowed: false,
            server_allowed: false,
            private_data_allowed,
        }
    }

    pub const fn client(private_data_allowed: bool) -> Self {
        Self {
            allowed: true,
            client_allowed: true,
            server_allowed: false,
            private_data_allowed,
        }
    }

    pub const fn server(private_data_allowed: bool) -> Self {
        Self {
            allowed: true,
            client_allowed: false,
            server_allowed: true,
            private_data_allowed,
        }
    }

    pub const fn bidirectional(private_data_allowed: bool) -> Self {
        Self {
            allowed: true,
            client_allowed: true,
            server_allowed: true,
            private_data_allowed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileAdapterCapabilityCatalog {
    pub cli: ProfileAdapterTransportCapability,
    pub http: ProfileAdapterTransportCapability,
    pub wss: ProfileAdapterTransportCapability,
    pub mcp: ProfileAdapterTransportCapability,
    pub a2a: ProfileAdapterTransportCapability,
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
    pub profile_desktop_macos_standalone_memory: bool,
    pub profile_desktop_macos_embedded_sdk: bool,
    pub profile_desktop_macos_dev_full: bool,
    pub profile_desktop_windows_embedded_sdk: bool,
    pub profile_desktop_windows_dev_full: bool,
    pub profile_server_linux_memory_gateway: bool,
    pub profile_server_linux_dev_full: bool,
    pub replay_harness_compiled: bool,
    pub sqlite_index_compiled: bool,
    pub rusqlite_dependency_compiled: bool,
}

impl CompiledFeatureReport {
    pub const fn profile_compiled(self, profile: ProfileId) -> bool {
        match profile {
            ProfileId::EspStandaloneMemory => self.profile_esp_standalone_memory,
            ProfileId::EspEmbeddedSdk => self.profile_esp_embedded_sdk,
            ProfileId::LinuxDeviceStandaloneMemory => self.profile_linux_device_standalone_memory,
            ProfileId::DesktopMacosStandaloneMemory => self.profile_desktop_macos_standalone_memory,
            ProfileId::DesktopMacosEmbeddedSdk => self.profile_desktop_macos_embedded_sdk,
            ProfileId::DesktopMacosDevFull => self.profile_desktop_macos_dev_full,
            ProfileId::DesktopWindowsEmbeddedSdk => self.profile_desktop_windows_embedded_sdk,
            ProfileId::DesktopWindowsDevFull => self.profile_desktop_windows_dev_full,
            ProfileId::ServerLinuxMemoryGateway => self.profile_server_linux_memory_gateway,
            ProfileId::ServerLinuxDevFull => self.profile_server_linux_dev_full,
        }
    }
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
        profile_desktop_macos_standalone_memory: cfg!(
            feature = "profile-desktop-macos-standalone-memory"
        ),
        profile_desktop_macos_embedded_sdk: cfg!(feature = "profile-desktop-macos-embedded-sdk"),
        profile_desktop_macos_dev_full: cfg!(feature = "profile-desktop-macos-dev-full"),
        profile_desktop_windows_embedded_sdk: cfg!(
            feature = "profile-desktop-windows-embedded-sdk"
        ),
        profile_desktop_windows_dev_full: cfg!(feature = "profile-desktop-windows-dev-full"),
        profile_server_linux_memory_gateway: cfg!(feature = "profile-server-linux-memory-gateway"),
        profile_server_linux_dev_full: cfg!(feature = "profile-server-linux-dev-full"),
        replay_harness_compiled: cfg!(feature = "nonproduction-replay-harness"),
        sqlite_index_compiled: sqlite_index_compiled(),
        rusqlite_dependency_compiled: sqlite_index_compiled(),
    }
}

pub const fn sqlite_index_compiled() -> bool {
    cfg!(feature = "sqlite-index")
}

pub const fn profile_compiled(profile: ProfileId) -> bool {
    compiled_feature_report().profile_compiled(profile)
}

pub const fn profile_capability_catalog() -> &'static [ProfileCapabilityCatalogEntry] {
    &PROFILE_CAPABILITY_CATALOG
}

const PROFILE_CAPABILITY_CATALOG: [ProfileCapabilityCatalogEntry; 10] = [
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
        communication_adapter_allowed: true,
        llm_gateway_server_allowed: false,
        adapter: ProfileAdapterCapabilityCatalog {
            cli: ProfileAdapterTransportCapability::local(false),
            http: ProfileAdapterTransportCapability::forbidden(),
            wss: ProfileAdapterTransportCapability::client(false),
            mcp: ProfileAdapterTransportCapability::forbidden(),
            a2a: ProfileAdapterTransportCapability::forbidden(),
        },
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
        llm_gateway_server_allowed: false,
        adapter: ProfileAdapterCapabilityCatalog {
            cli: ProfileAdapterTransportCapability::forbidden(),
            http: ProfileAdapterTransportCapability::forbidden(),
            wss: ProfileAdapterTransportCapability::forbidden(),
            mcp: ProfileAdapterTransportCapability::forbidden(),
            a2a: ProfileAdapterTransportCapability::forbidden(),
        },
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
        communication_adapter_allowed: true,
        llm_gateway_server_allowed: false,
        adapter: ProfileAdapterCapabilityCatalog {
            cli: ProfileAdapterTransportCapability::local(false),
            http: ProfileAdapterTransportCapability::server(false),
            wss: ProfileAdapterTransportCapability::bidirectional(false),
            mcp: ProfileAdapterTransportCapability::forbidden(),
            a2a: ProfileAdapterTransportCapability::forbidden(),
        },
    },
    ProfileCapabilityCatalogEntry {
        profile: ProfileId::DesktopMacosStandaloneMemory,
        target: TargetFeature::DesktopMacos,
        role: RoleFeature::StandaloneMemory,
        sqlite_index_allowed: true,
        lexical_archive_recall: true,
        heuristic_runtime_skill_recall: true,
        heuristic_task_learning_recall: true,
        indexed_archive_recall_allowed: true,
        indexed_continuity_capsule_recall_allowed: true,
        indexed_runtime_skill_recall_allowed: true,
        indexed_task_learning_recall_allowed: true,
        communication_adapter_allowed: true,
        llm_gateway_server_allowed: true,
        adapter: ProfileAdapterCapabilityCatalog {
            cli: ProfileAdapterTransportCapability::local(true),
            http: ProfileAdapterTransportCapability::server(true),
            wss: ProfileAdapterTransportCapability::bidirectional(true),
            mcp: ProfileAdapterTransportCapability::bidirectional(true),
            a2a: ProfileAdapterTransportCapability::forbidden(),
        },
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
        communication_adapter_allowed: true,
        llm_gateway_server_allowed: false,
        adapter: ProfileAdapterCapabilityCatalog {
            cli: ProfileAdapterTransportCapability::local(true),
            http: ProfileAdapterTransportCapability::server(true),
            wss: ProfileAdapterTransportCapability::bidirectional(true),
            mcp: ProfileAdapterTransportCapability::bidirectional(true),
            a2a: ProfileAdapterTransportCapability::forbidden(),
        },
    },
    dev_full_catalog_entry(ProfileId::DesktopMacosDevFull, TargetFeature::DesktopMacos),
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
        communication_adapter_allowed: true,
        llm_gateway_server_allowed: false,
        adapter: ProfileAdapterCapabilityCatalog {
            cli: ProfileAdapterTransportCapability::local(true),
            http: ProfileAdapterTransportCapability::server(true),
            wss: ProfileAdapterTransportCapability::bidirectional(true),
            mcp: ProfileAdapterTransportCapability::bidirectional(true),
            a2a: ProfileAdapterTransportCapability::forbidden(),
        },
    },
    dev_full_catalog_entry(
        ProfileId::DesktopWindowsDevFull,
        TargetFeature::DesktopWindows,
    ),
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
        llm_gateway_server_allowed: true,
        adapter: ProfileAdapterCapabilityCatalog {
            cli: ProfileAdapterTransportCapability::local(true),
            http: ProfileAdapterTransportCapability::server(true),
            wss: ProfileAdapterTransportCapability::bidirectional(false),
            mcp: ProfileAdapterTransportCapability::server(false),
            a2a: ProfileAdapterTransportCapability::bidirectional(false),
        },
    },
    dev_full_catalog_entry(ProfileId::ServerLinuxDevFull, TargetFeature::ServerLinux),
];

const fn dev_full_catalog_entry(
    profile: ProfileId,
    target: TargetFeature,
) -> ProfileCapabilityCatalogEntry {
    ProfileCapabilityCatalogEntry {
        profile,
        target,
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
        llm_gateway_server_allowed: true,
        adapter: ProfileAdapterCapabilityCatalog {
            cli: ProfileAdapterTransportCapability::local(true),
            http: ProfileAdapterTransportCapability::server(true),
            wss: ProfileAdapterTransportCapability::bidirectional(false),
            mcp: ProfileAdapterTransportCapability::server(false),
            a2a: ProfileAdapterTransportCapability::bidirectional(false),
        },
    }
}
