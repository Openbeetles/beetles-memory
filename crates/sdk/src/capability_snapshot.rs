use serde::Serialize;

use crate::{MemoryCapabilityCatalog, ProfileId};

pub const PLATFORM_CAPABILITY_SNAPSHOT_SCHEMA: &str = "beetle-memory.platform.capability.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformCapabilitySnapshot {
    pub schema: &'static str,
    pub profile: &'static str,
    pub target: &'static str,
    pub role: &'static str,
    pub compiled: PlatformCompiledFeatureSnapshot,
    pub memory: PlatformMemoryOperationSnapshot,
    pub lifecycle: PlatformLifecycleSnapshot,
    pub validation: PlatformValidationSnapshot,
    pub adapter: PlatformAdapterSnapshot,
    pub entry: PlatformEntryRuntimeSnapshot,
    pub indexed_recall: PlatformIndexedRecallSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformCompiledFeatureSnapshot {
    pub target_esp: bool,
    pub target_linux_device: bool,
    pub target_desktop_macos: bool,
    pub target_desktop_windows: bool,
    pub target_server_linux: bool,
    pub role_standalone_memory: bool,
    pub role_embedded_sdk: bool,
    pub role_memory_gateway: bool,
    pub role_dev_full: bool,
    pub replay_harness_compiled: bool,
    pub sqlite_index_compiled: bool,
    pub rusqlite_dependency_compiled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformMemoryOperationSnapshot {
    pub write: bool,
    pub recall: bool,
    pub projection: bool,
    pub maintenance: bool,
    pub inspection: bool,
    pub replay: bool,
    pub export: bool,
    pub import: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformLifecycleSnapshot {
    pub recover: bool,
    pub maintain_full: bool,
    pub maintain_lightweight: bool,
    pub operator_diagnosis: bool,
    pub export_snapshot: bool,
    pub import_snapshot: bool,
    pub replay_inspection: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformValidationSnapshot {
    pub compact_replay_fixture: bool,
    pub memory_harness: bool,
    pub full_replay_suite: bool,
    pub benchmark_gate: bool,
    pub proposal_preview: bool,
    pub compact_proposal_sandbox: bool,
    pub full_proposal_sandbox: bool,
    pub proposal_submission: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformAdapterSnapshot {
    pub cli: PlatformAdapterTransportSnapshot,
    pub http: PlatformAdapterTransportSnapshot,
    pub wss: PlatformAdapterTransportSnapshot,
    pub mcp: PlatformAdapterTransportSnapshot,
    pub a2a: PlatformAdapterTransportSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformEntryRuntimeSnapshot {
    pub cli: PlatformAdapterTransportSnapshot,
    pub http_server: PlatformAdapterTransportSnapshot,
    pub wss_client: PlatformAdapterTransportSnapshot,
    pub wss_server: PlatformAdapterTransportSnapshot,
    pub mcp_server: PlatformAdapterTransportSnapshot,
    pub a2a_bridge: PlatformAdapterTransportSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformAdapterTransportSnapshot {
    pub visible: bool,
    pub client_allowed: bool,
    pub server_allowed: bool,
    pub private_data_allowed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlatformIndexedRecallSnapshot {
    pub archive: bool,
    pub continuity_capsule: bool,
    pub runtime_skill: bool,
    pub task_learning: bool,
}

pub const fn platform_profile_feature_id(profile: ProfileId) -> &'static str {
    match profile {
        ProfileId::EspStandaloneMemory => "profile-esp-standalone-memory",
        ProfileId::EspEmbeddedSdk => "profile-esp-embedded-sdk",
        ProfileId::LinuxDeviceStandaloneMemory => "profile-linux-device-standalone-memory",
        ProfileId::DesktopMacosStandaloneMemory => "profile-desktop-macos-standalone-memory",
        ProfileId::DesktopMacosEmbeddedSdk => "profile-desktop-macos-embedded-sdk",
        ProfileId::DesktopWindowsEmbeddedSdk => "profile-desktop-windows-embedded-sdk",
        ProfileId::ServerLinuxMemoryGateway => "profile-server-linux-memory-gateway",
        ProfileId::ServerLinuxDevFull => "profile-server-linux-dev-full",
    }
}

pub const fn platform_capability_snapshot_file_name(profile: ProfileId) -> &'static str {
    platform_profile_feature_id(profile)
}

pub fn platform_capability_snapshot(
    catalog: &MemoryCapabilityCatalog,
) -> PlatformCapabilitySnapshot {
    PlatformCapabilitySnapshot {
        schema: PLATFORM_CAPABILITY_SNAPSHOT_SCHEMA,
        profile: platform_profile_feature_id(catalog.profile),
        target: catalog.target.as_str(),
        role: catalog.role.as_str(),
        compiled: PlatformCompiledFeatureSnapshot {
            target_esp: catalog.compiled.target_esp,
            target_linux_device: catalog.compiled.target_linux_device,
            target_desktop_macos: catalog.compiled.target_desktop_macos,
            target_desktop_windows: catalog.compiled.target_desktop_windows,
            target_server_linux: catalog.compiled.target_server_linux,
            role_standalone_memory: catalog.compiled.role_standalone_memory,
            role_embedded_sdk: catalog.compiled.role_embedded_sdk,
            role_memory_gateway: catalog.compiled.role_memory_gateway,
            role_dev_full: catalog.compiled.role_dev_full,
            replay_harness_compiled: catalog.compiled.replay_harness_compiled,
            sqlite_index_compiled: catalog.compiled.sqlite_index_compiled,
            rusqlite_dependency_compiled: catalog.compiled.rusqlite_dependency_compiled,
        },
        memory: PlatformMemoryOperationSnapshot {
            write: catalog.write.visible,
            recall: catalog.recall.visible,
            projection: catalog.projection.visible,
            maintenance: catalog.maintenance.visible,
            inspection: catalog.inspection.visible,
            replay: catalog.replay.visible,
            export: catalog.export.visible,
            import: catalog.import.visible,
        },
        lifecycle: PlatformLifecycleSnapshot {
            recover: catalog.lifecycle.recover.visible,
            maintain_full: catalog.lifecycle.maintain_full.visible,
            maintain_lightweight: catalog.lifecycle.maintain_lightweight.visible,
            operator_diagnosis: catalog.lifecycle.operator_diagnosis.visible,
            export_snapshot: catalog.lifecycle.export_snapshot.visible,
            import_snapshot: catalog.lifecycle.import_snapshot.visible,
            replay_inspection: catalog.lifecycle.replay_inspection.visible,
        },
        validation: PlatformValidationSnapshot {
            compact_replay_fixture: catalog.validation.compact_replay_fixture.visible,
            memory_harness: catalog.validation.memory_harness.visible,
            full_replay_suite: catalog.validation.full_replay_suite.visible,
            benchmark_gate: catalog.validation.benchmark_gate.visible,
            proposal_preview: catalog.validation.proposal_preview.visible,
            compact_proposal_sandbox: catalog.validation.compact_proposal_sandbox.visible,
            full_proposal_sandbox: catalog.validation.full_proposal_sandbox.visible,
            proposal_submission: catalog.validation.proposal_submission.visible,
        },
        adapter: PlatformAdapterSnapshot {
            cli: adapter_snapshot(catalog.adapter.cli),
            http: adapter_snapshot(catalog.adapter.http),
            wss: adapter_snapshot(catalog.adapter.wss),
            mcp: adapter_snapshot(catalog.adapter.mcp),
            a2a: adapter_snapshot(catalog.adapter.a2a),
        },
        entry: PlatformEntryRuntimeSnapshot {
            cli: adapter_snapshot(catalog.entry.cli),
            http_server: adapter_snapshot(catalog.entry.http_server),
            wss_client: adapter_snapshot(catalog.entry.wss_client),
            wss_server: adapter_snapshot(catalog.entry.wss_server),
            mcp_server: adapter_snapshot(catalog.entry.mcp_server),
            a2a_bridge: adapter_snapshot(catalog.entry.a2a_bridge),
        },
        indexed_recall: PlatformIndexedRecallSnapshot {
            archive: catalog.sqlite_index_recall.archive.visible,
            continuity_capsule: catalog.sqlite_index_recall.continuity_capsule.visible,
            runtime_skill: catalog.sqlite_index_recall.runtime_skill.visible,
            task_learning: catalog.sqlite_index_recall.task_learning.visible,
        },
    }
}

fn adapter_snapshot(
    transport: crate::AdapterTransportVisibility,
) -> PlatformAdapterTransportSnapshot {
    PlatformAdapterTransportSnapshot {
        visible: transport.visible,
        client_allowed: transport.client_allowed,
        server_allowed: transport.server_allowed,
        private_data_allowed: transport.private_data_allowed,
    }
}
