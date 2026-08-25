use bm_core::feature_gate::{
    compiled_feature_report, profile_capability_catalog, CompiledFeatureReport,
    ProfileCapabilityCatalogEntry, ProfileId, RoleFeature, TargetFeature,
};

use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryCapabilityPolicy {
    pub write_enabled: bool,
    pub recall_enabled: bool,
    pub projection_enabled: bool,
    pub recover_enabled: bool,
    pub maintenance_enabled: bool,
    pub inspection_enabled: bool,
    pub transcript_replay_enabled: bool,
    pub transcript_search_enabled: bool,
    pub transcript_activity_enabled: bool,
    pub replay_enabled: bool,
    pub export_enabled: bool,
    pub import_enabled: bool,
    pub replay_harness_enabled: bool,
    pub evolution_sandbox_enabled: bool,
    pub communication_adapter_enabled: bool,
    pub long_term_control_inspect_enabled: bool,
    pub long_term_control_mutation_enabled: bool,
    pub long_term_control_policy_enabled: bool,
    pub long_term_control_bulk_forget_enabled: bool,
    pub adapter: MemoryAdapterCapabilityPolicy,
}

impl MemoryCapabilityPolicy {
    pub const fn strict_profile() -> Self {
        Self {
            write_enabled: true,
            recall_enabled: true,
            projection_enabled: true,
            recover_enabled: true,
            maintenance_enabled: true,
            inspection_enabled: true,
            transcript_replay_enabled: true,
            transcript_search_enabled: true,
            transcript_activity_enabled: true,
            replay_enabled: true,
            export_enabled: true,
            import_enabled: true,
            replay_harness_enabled: true,
            evolution_sandbox_enabled: true,
            communication_adapter_enabled: false,
            long_term_control_inspect_enabled: true,
            long_term_control_mutation_enabled: true,
            long_term_control_policy_enabled: true,
            long_term_control_bulk_forget_enabled: true,
            adapter: MemoryAdapterCapabilityPolicy::all_enabled(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryAdapterCapabilityPolicy {
    pub cli_enabled: bool,
    pub http_enabled: bool,
    pub wss_enabled: bool,
    pub mcp_enabled: bool,
    pub a2a_enabled: bool,
}

impl MemoryAdapterCapabilityPolicy {
    pub const fn all_enabled() -> Self {
        Self {
            cli_enabled: true,
            http_enabled: true,
            wss_enabled: true,
            mcp_enabled: true,
            a2a_enabled: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryPrivacyPolicy {
    pub prompt_projection_allowed: bool,
    pub private_plane_projection_allowed: bool,
    pub governance_model_disclosure_allowed: bool,
    pub operator_inspection_allowed: bool,
    pub export_allowed: bool,
    pub import_allowed: bool,
}

impl MemoryPrivacyPolicy {
    pub const fn standard_private_boundary() -> Self {
        Self {
            prompt_projection_allowed: true,
            private_plane_projection_allowed: false,
            governance_model_disclosure_allowed: true,
            operator_inspection_allowed: true,
            export_allowed: true,
            import_allowed: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryOperationVisibility {
    pub profile_allowed: bool,
    pub compiled: bool,
    pub config_enabled: bool,
    pub permission_allowed: bool,
    pub privacy_allowed: bool,
    pub visible: bool,
}

impl MemoryOperationVisibility {
    pub const fn hidden() -> Self {
        Self {
            profile_allowed: false,
            compiled: false,
            config_enabled: false,
            permission_allowed: false,
            privacy_allowed: false,
            visible: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryIndexedRecallVisibility {
    pub archive: MemoryOperationVisibility,
    pub continuity_capsule: MemoryOperationVisibility,
    pub runtime_skill: MemoryOperationVisibility,
    pub task_learning: MemoryOperationVisibility,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillRecallTransport {
    IndexedSqlite,
    CompactTypedDirect,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedStateMemoryCapability {
    pub dynamic_state_recall: MemoryOperationVisibility,
    pub historical_as_of_recall: MemoryOperationVisibility,
    pub procedural_recall: MemoryOperationVisibility,
    pub environment_premise_evaluation: MemoryOperationVisibility,
    pub update_lineage_inspection: MemoryOperationVisibility,
    pub runtime_skill_recall_transport: RuntimeSkillRecallTransport,
}

impl GovernedStateMemoryCapability {
    pub const fn hidden() -> Self {
        Self {
            dynamic_state_recall: MemoryOperationVisibility::hidden(),
            historical_as_of_recall: MemoryOperationVisibility::hidden(),
            procedural_recall: MemoryOperationVisibility::hidden(),
            environment_premise_evaluation: MemoryOperationVisibility::hidden(),
            update_lineage_inspection: MemoryOperationVisibility::hidden(),
            runtime_skill_recall_transport: RuntimeSkillRecallTransport::Unavailable,
        }
    }
}

impl MemoryIndexedRecallVisibility {
    pub const fn hidden() -> Self {
        Self {
            archive: MemoryOperationVisibility::hidden(),
            continuity_capsule: MemoryOperationVisibility::hidden(),
            runtime_skill: MemoryOperationVisibility::hidden(),
            task_learning: MemoryOperationVisibility::hidden(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRuntimeLifecycleCapability {
    pub recover: MemoryOperationVisibility,
    pub maintain_full: MemoryOperationVisibility,
    pub maintain_lightweight: MemoryOperationVisibility,
    pub operator_diagnosis: MemoryOperationVisibility,
    pub export_snapshot: MemoryOperationVisibility,
    pub import_snapshot: MemoryOperationVisibility,
    pub replay_inspection: MemoryOperationVisibility,
}

impl MemoryRuntimeLifecycleCapability {
    pub const fn hidden() -> Self {
        Self {
            recover: MemoryOperationVisibility::hidden(),
            maintain_full: MemoryOperationVisibility::hidden(),
            maintain_lightweight: MemoryOperationVisibility::hidden(),
            operator_diagnosis: MemoryOperationVisibility::hidden(),
            export_snapshot: MemoryOperationVisibility::hidden(),
            import_snapshot: MemoryOperationVisibility::hidden(),
            replay_inspection: MemoryOperationVisibility::hidden(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterTransportVisibility {
    pub profile_allowed: bool,
    pub compiled: bool,
    pub config_enabled: bool,
    pub permission_allowed: bool,
    pub privacy_allowed: bool,
    pub client_allowed: bool,
    pub server_allowed: bool,
    pub private_data_allowed: bool,
    pub visible: bool,
}

impl AdapterTransportVisibility {
    pub const fn hidden() -> Self {
        Self {
            profile_allowed: false,
            compiled: false,
            config_enabled: false,
            permission_allowed: false,
            privacy_allowed: false,
            client_allowed: false,
            server_allowed: false,
            private_data_allowed: false,
            visible: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryAdapterCapabilityCatalog {
    pub cli: AdapterTransportVisibility,
    pub http: AdapterTransportVisibility,
    pub wss: AdapterTransportVisibility,
    pub mcp: AdapterTransportVisibility,
    pub a2a: AdapterTransportVisibility,
}

impl MemoryAdapterCapabilityCatalog {
    pub const fn hidden() -> Self {
        Self {
            cli: AdapterTransportVisibility::hidden(),
            http: AdapterTransportVisibility::hidden(),
            wss: AdapterTransportVisibility::hidden(),
            mcp: AdapterTransportVisibility::hidden(),
            a2a: AdapterTransportVisibility::hidden(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryEntryRuntimeCapabilityCatalog {
    pub cli: AdapterTransportVisibility,
    pub http_server: AdapterTransportVisibility,
    pub wss_client: AdapterTransportVisibility,
    pub wss_server: AdapterTransportVisibility,
    pub mcp_server: AdapterTransportVisibility,
    pub a2a_bridge: AdapterTransportVisibility,
    pub llm_gateway_server: AdapterTransportVisibility,
}

impl MemoryEntryRuntimeCapabilityCatalog {
    pub const fn hidden() -> Self {
        Self {
            cli: AdapterTransportVisibility::hidden(),
            http_server: AdapterTransportVisibility::hidden(),
            wss_client: AdapterTransportVisibility::hidden(),
            wss_server: AdapterTransportVisibility::hidden(),
            mcp_server: AdapterTransportVisibility::hidden(),
            a2a_bridge: AdapterTransportVisibility::hidden(),
            llm_gateway_server: AdapterTransportVisibility::hidden(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryValidationCapability {
    pub compact_replay_fixture: MemoryOperationVisibility,
    pub memory_harness: MemoryOperationVisibility,
    pub full_replay_suite: MemoryOperationVisibility,
    pub benchmark_gate: MemoryOperationVisibility,
    pub proposal_preview: MemoryOperationVisibility,
    pub compact_proposal_sandbox: MemoryOperationVisibility,
    pub full_proposal_sandbox: MemoryOperationVisibility,
    pub proposal_submission: MemoryOperationVisibility,
}

impl MemoryValidationCapability {
    pub const fn hidden() -> Self {
        Self {
            compact_replay_fixture: MemoryOperationVisibility::hidden(),
            memory_harness: MemoryOperationVisibility::hidden(),
            full_replay_suite: MemoryOperationVisibility::hidden(),
            benchmark_gate: MemoryOperationVisibility::hidden(),
            proposal_preview: MemoryOperationVisibility::hidden(),
            compact_proposal_sandbox: MemoryOperationVisibility::hidden(),
            full_proposal_sandbox: MemoryOperationVisibility::hidden(),
            proposal_submission: MemoryOperationVisibility::hidden(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryCapabilityCatalog {
    pub profile: ProfileId,
    pub target: TargetFeature,
    pub role: RoleFeature,
    pub compiled: CompiledFeatureReport,
    pub write: MemoryOperationVisibility,
    pub recall: MemoryOperationVisibility,
    pub projection: MemoryOperationVisibility,
    pub maintenance: MemoryOperationVisibility,
    pub inspection: MemoryOperationVisibility,
    pub transcript_replay: MemoryOperationVisibility,
    pub transcript_search: MemoryOperationVisibility,
    pub transcript_activity: MemoryOperationVisibility,
    pub transcript_export: MemoryOperationVisibility,
    pub replay: MemoryOperationVisibility,
    pub export: MemoryOperationVisibility,
    pub import: MemoryOperationVisibility,
    pub long_term_control_inspect: MemoryOperationVisibility,
    pub long_term_control_mutation: MemoryOperationVisibility,
    pub long_term_control_policy: MemoryOperationVisibility,
    pub long_term_control_bulk_forget: MemoryOperationVisibility,
    pub communication_adapter: MemoryOperationVisibility,
    pub adapter: MemoryAdapterCapabilityCatalog,
    pub entry: MemoryEntryRuntimeCapabilityCatalog,
    pub sqlite_index_recall: MemoryIndexedRecallVisibility,
    pub governed_state: GovernedStateMemoryCapability,
    pub lifecycle: MemoryRuntimeLifecycleCapability,
    pub validation: MemoryValidationCapability,
}

impl MemoryCapabilityCatalog {
    pub fn empty(profile: ProfileId, target: TargetFeature, role: RoleFeature) -> Self {
        Self {
            profile,
            target,
            role,
            compiled: compiled_feature_report(),
            write: MemoryOperationVisibility::hidden(),
            recall: MemoryOperationVisibility::hidden(),
            projection: MemoryOperationVisibility::hidden(),
            maintenance: MemoryOperationVisibility::hidden(),
            inspection: MemoryOperationVisibility::hidden(),
            transcript_replay: MemoryOperationVisibility::hidden(),
            transcript_search: MemoryOperationVisibility::hidden(),
            transcript_activity: MemoryOperationVisibility::hidden(),
            transcript_export: MemoryOperationVisibility::hidden(),
            replay: MemoryOperationVisibility::hidden(),
            export: MemoryOperationVisibility::hidden(),
            import: MemoryOperationVisibility::hidden(),
            long_term_control_inspect: MemoryOperationVisibility::hidden(),
            long_term_control_mutation: MemoryOperationVisibility::hidden(),
            long_term_control_policy: MemoryOperationVisibility::hidden(),
            long_term_control_bulk_forget: MemoryOperationVisibility::hidden(),
            communication_adapter: MemoryOperationVisibility::hidden(),
            adapter: MemoryAdapterCapabilityCatalog::hidden(),
            entry: MemoryEntryRuntimeCapabilityCatalog::hidden(),
            sqlite_index_recall: MemoryIndexedRecallVisibility::hidden(),
            governed_state: GovernedStateMemoryCapability::hidden(),
            lifecycle: MemoryRuntimeLifecycleCapability::hidden(),
            validation: MemoryValidationCapability::hidden(),
        }
    }

    fn from_entry(
        entry: ProfileCapabilityCatalogEntry,
        compiled: CompiledFeatureReport,
        policy: &MemoryCapabilityPolicy,
        privacy: &MemoryPrivacyPolicy,
        governed_state_runtime_compiled: bool,
        runtime_skill_recall_transport: RuntimeSkillRecallTransport,
    ) -> Self {
        let profile_kind = profile_kind(entry.profile);
        let indexed_runtime_skill = indexed_visible(
            entry.indexed_runtime_skill_recall_allowed,
            compiled,
            policy.recall_enabled,
        );
        Self {
            profile: entry.profile,
            target: entry.target,
            role: entry.role,
            compiled,
            write: visible(true, true, policy.write_enabled, true, true),
            recall: visible(true, true, policy.recall_enabled, true, true),
            projection: visible(
                true,
                true,
                policy.projection_enabled,
                true,
                privacy.prompt_projection_allowed,
            ),
            maintenance: visible(
                profile_kind.maintenance_allowed,
                true,
                policy.maintenance_enabled,
                true,
                true,
            ),
            inspection: visible(
                profile_kind.inspection_allowed,
                true,
                policy.inspection_enabled,
                true,
                privacy.operator_inspection_allowed,
            ),
            transcript_replay: visible(
                profile_kind.transcript_replay_allowed,
                true,
                policy.transcript_replay_enabled,
                true,
                true,
            ),
            transcript_search: visible(
                profile_kind.transcript_search_allowed,
                true,
                policy.transcript_search_enabled,
                true,
                true,
            ),
            transcript_activity: visible(
                profile_kind.transcript_activity_allowed,
                true,
                policy.transcript_activity_enabled,
                true,
                true,
            ),
            transcript_export: visible(
                profile_kind.transcript_export_allowed,
                true,
                policy.export_enabled,
                true,
                privacy.export_allowed,
            ),
            replay: visible(
                profile_kind.replay_allowed,
                true,
                policy.replay_enabled,
                true,
                true,
            ),
            export: visible(
                profile_kind.export_allowed,
                true,
                policy.export_enabled,
                true,
                privacy.export_allowed,
            ),
            import: visible(
                profile_kind.import_allowed,
                true,
                policy.import_enabled,
                true,
                privacy.import_allowed,
            ),
            long_term_control_inspect: visible(
                profile_kind.long_term_control_inspect_allowed,
                true,
                policy.long_term_control_inspect_enabled,
                true,
                privacy.operator_inspection_allowed,
            ),
            long_term_control_mutation: visible(
                profile_kind.long_term_control_mutation_allowed,
                true,
                policy.long_term_control_mutation_enabled,
                true,
                true,
            ),
            long_term_control_policy: visible(
                profile_kind.long_term_control_policy_allowed,
                true,
                policy.long_term_control_policy_enabled,
                true,
                true,
            ),
            long_term_control_bulk_forget: visible(
                profile_kind.long_term_control_bulk_forget_allowed,
                true,
                policy.long_term_control_bulk_forget_enabled,
                true,
                true,
            ),
            communication_adapter: visible(
                entry.communication_adapter_allowed,
                true,
                policy.communication_adapter_enabled,
                true,
                true,
            ),
            adapter: MemoryAdapterCapabilityCatalog {
                cli: adapter_visible(
                    entry.adapter.cli,
                    policy.communication_adapter_enabled && policy.adapter.cli_enabled,
                    true,
                ),
                http: adapter_visible(
                    entry.adapter.http,
                    policy.communication_adapter_enabled && policy.adapter.http_enabled,
                    true,
                ),
                wss: adapter_visible(
                    entry.adapter.wss,
                    policy.communication_adapter_enabled && policy.adapter.wss_enabled,
                    true,
                ),
                mcp: adapter_visible(
                    entry.adapter.mcp,
                    policy.communication_adapter_enabled && policy.adapter.mcp_enabled,
                    true,
                ),
                a2a: adapter_visible(
                    entry.adapter.a2a,
                    policy.communication_adapter_enabled && policy.adapter.a2a_enabled,
                    true,
                ),
            },
            entry: MemoryEntryRuntimeCapabilityCatalog {
                cli: entry_visible(
                    entry.adapter.cli,
                    policy.communication_adapter_enabled && policy.adapter.cli_enabled,
                    EntryMode::Local,
                ),
                http_server: entry_visible(
                    entry.adapter.http,
                    policy.communication_adapter_enabled && policy.adapter.http_enabled,
                    EntryMode::Server,
                ),
                wss_client: entry_visible(
                    entry.adapter.wss,
                    policy.communication_adapter_enabled && policy.adapter.wss_enabled,
                    EntryMode::Client,
                ),
                wss_server: entry_visible(
                    entry.adapter.wss,
                    policy.communication_adapter_enabled && policy.adapter.wss_enabled,
                    EntryMode::Server,
                ),
                mcp_server: entry_visible(
                    entry.adapter.mcp,
                    policy.communication_adapter_enabled && policy.adapter.mcp_enabled,
                    EntryMode::Server,
                ),
                a2a_bridge: entry_visible(
                    entry.adapter.a2a,
                    policy.communication_adapter_enabled && policy.adapter.a2a_enabled,
                    EntryMode::Server,
                ),
                llm_gateway_server: entry_server_surface_visible(
                    entry.llm_gateway_server_allowed,
                    policy.communication_adapter_enabled,
                ),
            },
            lifecycle: MemoryRuntimeLifecycleCapability {
                recover: visible(
                    profile_kind.recover_allowed,
                    true,
                    policy.recover_enabled,
                    true,
                    true,
                ),
                maintain_full: visible(
                    profile_kind.maintenance_full_allowed,
                    true,
                    policy.maintenance_enabled,
                    true,
                    true,
                ),
                maintain_lightweight: visible(
                    profile_kind.maintenance_lightweight_allowed,
                    true,
                    policy.maintenance_enabled,
                    true,
                    true,
                ),
                operator_diagnosis: visible(
                    profile_kind.inspection_allowed,
                    true,
                    policy.inspection_enabled,
                    true,
                    privacy.operator_inspection_allowed,
                ),
                export_snapshot: visible(
                    profile_kind.export_allowed,
                    true,
                    policy.export_enabled,
                    true,
                    privacy.export_allowed,
                ),
                import_snapshot: visible(
                    profile_kind.import_allowed,
                    true,
                    policy.import_enabled,
                    true,
                    privacy.import_allowed,
                ),
                replay_inspection: visible(
                    profile_kind.replay_allowed,
                    true,
                    policy.replay_enabled,
                    true,
                    true,
                ),
            },
            validation: MemoryValidationCapability {
                compact_replay_fixture: visible(
                    profile_kind.compact_replay_fixture_allowed,
                    compiled.replay_harness_compiled,
                    policy.replay_harness_enabled,
                    true,
                    true,
                ),
                memory_harness: visible(
                    profile_kind.memory_harness_allowed,
                    compiled.replay_harness_compiled,
                    policy.replay_harness_enabled,
                    true,
                    true,
                ),
                full_replay_suite: visible(
                    profile_kind.full_replay_suite_allowed,
                    compiled.replay_harness_compiled,
                    policy.replay_harness_enabled,
                    true,
                    true,
                ),
                benchmark_gate: visible(
                    profile_kind.benchmark_gate_allowed,
                    compiled.replay_harness_compiled,
                    policy.replay_harness_enabled,
                    true,
                    true,
                ),
                proposal_preview: visible(
                    profile_kind.proposal_preview_allowed,
                    true,
                    policy.evolution_sandbox_enabled,
                    true,
                    true,
                ),
                compact_proposal_sandbox: visible(
                    profile_kind.compact_proposal_sandbox_allowed,
                    true,
                    policy.evolution_sandbox_enabled,
                    true,
                    true,
                ),
                full_proposal_sandbox: visible(
                    profile_kind.full_proposal_sandbox_allowed,
                    true,
                    policy.evolution_sandbox_enabled,
                    true,
                    true,
                ),
                proposal_submission: visible(
                    profile_kind.proposal_submission_allowed,
                    true,
                    policy.evolution_sandbox_enabled,
                    true,
                    true,
                ),
            },
            governed_state: GovernedStateMemoryCapability {
                dynamic_state_recall: visible(
                    entry.dynamic_state_recall_allowed,
                    governed_state_runtime_compiled,
                    policy.recall_enabled,
                    true,
                    privacy.prompt_projection_allowed,
                ),
                historical_as_of_recall: visible(
                    entry.historical_as_of_recall_allowed,
                    governed_state_runtime_compiled,
                    policy.recall_enabled,
                    true,
                    privacy.prompt_projection_allowed,
                ),
                procedural_recall: visible(
                    entry.procedural_recall_allowed,
                    governed_state_runtime_compiled
                        && runtime_skill_recall_transport
                            != RuntimeSkillRecallTransport::Unavailable,
                    policy.recall_enabled,
                    true,
                    privacy.prompt_projection_allowed,
                ),
                environment_premise_evaluation: visible(
                    entry.environment_premise_evaluation_allowed,
                    governed_state_runtime_compiled,
                    policy.recall_enabled,
                    true,
                    privacy.prompt_projection_allowed,
                ),
                update_lineage_inspection: visible(
                    entry.update_lineage_inspection_allowed,
                    governed_state_runtime_compiled,
                    policy.inspection_enabled,
                    true,
                    privacy.operator_inspection_allowed,
                ),
                runtime_skill_recall_transport,
            },
            sqlite_index_recall: MemoryIndexedRecallVisibility {
                archive: indexed_visible(
                    entry.indexed_archive_recall_allowed,
                    compiled,
                    policy.recall_enabled,
                ),
                continuity_capsule: indexed_visible(
                    entry.indexed_continuity_capsule_recall_allowed,
                    compiled,
                    policy.recall_enabled,
                ),
                runtime_skill: indexed_runtime_skill,
                task_learning: indexed_visible(
                    entry.indexed_task_learning_recall_allowed,
                    compiled,
                    policy.recall_enabled,
                ),
            },
        }
    }
}

pub fn resolve_memory_capabilities(
    profile: ProfileId,
    policy: &MemoryCapabilityPolicy,
    privacy: &MemoryPrivacyPolicy,
) -> Result<MemoryCapabilityCatalog> {
    let entry = profile_capability_catalog()
        .iter()
        .find(|entry| entry.profile == profile)
        .copied()
        .ok_or_else(|| Error::config("memory_capability_catalog", profile.as_str()))?;

    Ok(MemoryCapabilityCatalog::from_entry(
        entry,
        compiled_feature_report(),
        policy,
        privacy,
        false,
        RuntimeSkillRecallTransport::Unavailable,
    ))
}

pub(crate) fn resolve_memory_capabilities_for_runtime(
    profile: ProfileId,
    policy: &MemoryCapabilityPolicy,
    privacy: &MemoryPrivacyPolicy,
    runtime_skill_recall_transport: RuntimeSkillRecallTransport,
) -> Result<MemoryCapabilityCatalog> {
    let entry = profile_capability_catalog()
        .iter()
        .find(|entry| entry.profile == profile)
        .copied()
        .ok_or_else(|| Error::config("memory_capability_catalog", profile.as_str()))?;
    Ok(MemoryCapabilityCatalog::from_entry(
        entry,
        compiled_feature_report(),
        policy,
        privacy,
        true,
        runtime_skill_recall_transport,
    ))
}

fn visible(
    profile_allowed: bool,
    compiled: bool,
    config_enabled: bool,
    permission_allowed: bool,
    privacy_allowed: bool,
) -> MemoryOperationVisibility {
    MemoryOperationVisibility {
        profile_allowed,
        compiled,
        config_enabled,
        permission_allowed,
        privacy_allowed,
        visible: profile_allowed
            && compiled
            && config_enabled
            && permission_allowed
            && privacy_allowed,
    }
}

fn indexed_visible(
    profile_allowed: bool,
    compiled: CompiledFeatureReport,
    config_enabled: bool,
) -> MemoryOperationVisibility {
    visible(
        profile_allowed,
        compiled.sqlite_index_compiled,
        config_enabled,
        true,
        true,
    )
}

fn adapter_visible(
    profile: bm_core::feature_gate::ProfileAdapterTransportCapability,
    config_enabled: bool,
    compiled: bool,
) -> AdapterTransportVisibility {
    AdapterTransportVisibility {
        profile_allowed: profile.allowed,
        compiled,
        config_enabled,
        permission_allowed: true,
        privacy_allowed: true,
        client_allowed: profile.client_allowed,
        server_allowed: profile.server_allowed,
        private_data_allowed: profile.private_data_allowed,
        visible: profile.allowed && compiled && config_enabled,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryMode {
    Local,
    Client,
    Server,
}

fn entry_visible(
    profile: bm_core::feature_gate::ProfileAdapterTransportCapability,
    config_enabled: bool,
    mode: EntryMode,
) -> AdapterTransportVisibility {
    let mode_allowed = match mode {
        EntryMode::Local => profile.allowed && !profile.client_allowed && !profile.server_allowed,
        EntryMode::Client => profile.client_allowed,
        EntryMode::Server => profile.server_allowed,
    };
    AdapterTransportVisibility {
        profile_allowed: profile.allowed && mode_allowed,
        compiled: true,
        config_enabled,
        permission_allowed: true,
        privacy_allowed: true,
        client_allowed: matches!(mode, EntryMode::Client) && profile.client_allowed,
        server_allowed: matches!(mode, EntryMode::Server) && profile.server_allowed,
        private_data_allowed: profile.private_data_allowed,
        visible: profile.allowed && mode_allowed && config_enabled,
    }
}

fn entry_server_surface_visible(
    profile_allowed: bool,
    config_enabled: bool,
) -> AdapterTransportVisibility {
    AdapterTransportVisibility {
        profile_allowed,
        compiled: true,
        config_enabled,
        permission_allowed: true,
        privacy_allowed: true,
        client_allowed: false,
        server_allowed: profile_allowed,
        private_data_allowed: false,
        visible: profile_allowed && config_enabled,
    }
}

#[derive(Clone, Copy)]
struct ProfileOperationDefaults {
    recover_allowed: bool,
    maintenance_allowed: bool,
    maintenance_full_allowed: bool,
    maintenance_lightweight_allowed: bool,
    inspection_allowed: bool,
    transcript_replay_allowed: bool,
    transcript_search_allowed: bool,
    transcript_activity_allowed: bool,
    transcript_export_allowed: bool,
    replay_allowed: bool,
    export_allowed: bool,
    import_allowed: bool,
    long_term_control_inspect_allowed: bool,
    long_term_control_mutation_allowed: bool,
    long_term_control_policy_allowed: bool,
    long_term_control_bulk_forget_allowed: bool,
    compact_replay_fixture_allowed: bool,
    memory_harness_allowed: bool,
    full_replay_suite_allowed: bool,
    benchmark_gate_allowed: bool,
    proposal_preview_allowed: bool,
    compact_proposal_sandbox_allowed: bool,
    full_proposal_sandbox_allowed: bool,
    proposal_submission_allowed: bool,
}

fn profile_kind(profile: ProfileId) -> ProfileOperationDefaults {
    match profile {
        ProfileId::EspStandaloneMemory
        | ProfileId::LinuxDeviceStandaloneMemory
        | ProfileId::DesktopMacosStandaloneMemory => ProfileOperationDefaults {
            recover_allowed: true,
            maintenance_allowed: true,
            maintenance_full_allowed: !matches!(profile, ProfileId::EspStandaloneMemory),
            maintenance_lightweight_allowed: true,
            inspection_allowed: true,
            transcript_replay_allowed: true,
            transcript_search_allowed: matches!(profile, ProfileId::DesktopMacosStandaloneMemory),
            transcript_activity_allowed: matches!(profile, ProfileId::DesktopMacosStandaloneMemory),
            transcript_export_allowed: true,
            replay_allowed: false,
            export_allowed: true,
            import_allowed: true,
            long_term_control_inspect_allowed: true,
            long_term_control_mutation_allowed: true,
            long_term_control_policy_allowed: true,
            long_term_control_bulk_forget_allowed: matches!(
                profile,
                ProfileId::DesktopMacosStandaloneMemory
            ),
            compact_replay_fixture_allowed: true,
            memory_harness_allowed: true,
            full_replay_suite_allowed: !matches!(profile, ProfileId::EspStandaloneMemory),
            benchmark_gate_allowed: false,
            proposal_preview_allowed: true,
            compact_proposal_sandbox_allowed: true,
            full_proposal_sandbox_allowed: !matches!(profile, ProfileId::EspStandaloneMemory),
            proposal_submission_allowed: true,
        },
        ProfileId::EspEmbeddedSdk => ProfileOperationDefaults {
            recover_allowed: false,
            maintenance_allowed: false,
            maintenance_full_allowed: false,
            maintenance_lightweight_allowed: false,
            inspection_allowed: true,
            transcript_replay_allowed: true,
            transcript_search_allowed: false,
            transcript_activity_allowed: false,
            transcript_export_allowed: false,
            replay_allowed: false,
            export_allowed: false,
            import_allowed: false,
            long_term_control_inspect_allowed: true,
            long_term_control_mutation_allowed: true,
            long_term_control_policy_allowed: true,
            long_term_control_bulk_forget_allowed: !matches!(profile, ProfileId::EspEmbeddedSdk),
            compact_replay_fixture_allowed: false,
            memory_harness_allowed: false,
            full_replay_suite_allowed: false,
            benchmark_gate_allowed: false,
            proposal_preview_allowed: true,
            compact_proposal_sandbox_allowed: false,
            full_proposal_sandbox_allowed: false,
            proposal_submission_allowed: false,
        },
        ProfileId::DesktopMacosEmbeddedSdk
        | ProfileId::DesktopLinuxEmbeddedSdk
        | ProfileId::DesktopWindowsEmbeddedSdk => ProfileOperationDefaults {
            recover_allowed: false,
            maintenance_allowed: true,
            maintenance_full_allowed: true,
            maintenance_lightweight_allowed: true,
            inspection_allowed: true,
            transcript_replay_allowed: true,
            transcript_search_allowed: true,
            transcript_activity_allowed: true,
            transcript_export_allowed: true,
            replay_allowed: false,
            export_allowed: false,
            import_allowed: false,
            long_term_control_inspect_allowed: true,
            long_term_control_mutation_allowed: true,
            long_term_control_policy_allowed: true,
            long_term_control_bulk_forget_allowed: true,
            compact_replay_fixture_allowed: false,
            memory_harness_allowed: false,
            full_replay_suite_allowed: false,
            benchmark_gate_allowed: false,
            proposal_preview_allowed: true,
            compact_proposal_sandbox_allowed: false,
            full_proposal_sandbox_allowed: false,
            proposal_submission_allowed: false,
        },
        ProfileId::ServerLinuxMemoryGateway => ProfileOperationDefaults {
            recover_allowed: true,
            maintenance_allowed: true,
            maintenance_full_allowed: true,
            maintenance_lightweight_allowed: true,
            inspection_allowed: true,
            transcript_replay_allowed: true,
            transcript_search_allowed: true,
            transcript_activity_allowed: true,
            transcript_export_allowed: true,
            replay_allowed: false,
            export_allowed: true,
            import_allowed: true,
            long_term_control_inspect_allowed: true,
            long_term_control_mutation_allowed: true,
            long_term_control_policy_allowed: true,
            long_term_control_bulk_forget_allowed: true,
            compact_replay_fixture_allowed: true,
            memory_harness_allowed: true,
            full_replay_suite_allowed: true,
            benchmark_gate_allowed: false,
            proposal_preview_allowed: true,
            compact_proposal_sandbox_allowed: true,
            full_proposal_sandbox_allowed: true,
            proposal_submission_allowed: true,
        },
        ProfileId::DesktopMacosDevFull
        | ProfileId::DesktopWindowsDevFull
        | ProfileId::ServerLinuxDevFull => ProfileOperationDefaults {
            recover_allowed: true,
            maintenance_allowed: true,
            maintenance_full_allowed: true,
            maintenance_lightweight_allowed: true,
            inspection_allowed: true,
            transcript_replay_allowed: true,
            transcript_search_allowed: true,
            transcript_activity_allowed: true,
            transcript_export_allowed: true,
            replay_allowed: true,
            export_allowed: true,
            import_allowed: true,
            long_term_control_inspect_allowed: true,
            long_term_control_mutation_allowed: true,
            long_term_control_policy_allowed: true,
            long_term_control_bulk_forget_allowed: true,
            compact_replay_fixture_allowed: true,
            memory_harness_allowed: true,
            full_replay_suite_allowed: true,
            benchmark_gate_allowed: true,
            proposal_preview_allowed: true,
            compact_proposal_sandbox_allowed: true,
            full_proposal_sandbox_allowed: true,
            proposal_submission_allowed: true,
        },
    }
}
