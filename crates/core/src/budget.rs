use crate::feature_gate::ProfileId;
use crate::orchestrator::PressureLevel;
use crate::resource::{
    RuntimeResourceProbeSource, RuntimeResourceSnapshot, RuntimeResourceUnavailableReason,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticPlatformManifest {
    pub manifest_id: String,
    pub profile: ProfileId,
    pub deployment_role: RuntimeDeploymentRole,
    pub memory_floor_bytes: u64,
    pub storage_floor_bytes: u64,
    pub notes: Vec<String>,
}

impl StaticPlatformManifest {
    pub fn for_profile(profile: ProfileId) -> Self {
        let ceiling = profile_budget_ceiling(profile);
        Self {
            manifest_id: format!("static-manifest:{}", profile.as_str()),
            profile,
            deployment_role: RuntimeDeploymentRole::from_profile(profile),
            memory_floor_bytes: ceiling.memory_floor_bytes,
            storage_floor_bytes: ceiling.storage_floor_bytes,
            notes: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDeploymentRole {
    StandaloneMemory,
    EmbeddedSdk,
    MemoryGateway,
    DevFull,
}

impl RuntimeDeploymentRole {
    pub const fn from_profile(profile: ProfileId) -> Self {
        match profile {
            ProfileId::EspEmbeddedSdk
            | ProfileId::DesktopMacosEmbeddedSdk
            | ProfileId::DesktopWindowsEmbeddedSdk => Self::EmbeddedSdk,
            ProfileId::ServerLinuxMemoryGateway => Self::MemoryGateway,
            ProfileId::ServerLinuxDevFull => Self::DevFull,
            ProfileId::EspStandaloneMemory
            | ProfileId::LinuxDeviceStandaloneMemory
            | ProfileId::DesktopMacosStandaloneMemory => Self::StandaloneMemory,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StandaloneMemory => "standalone_memory",
            Self::EmbeddedSdk => "embedded_sdk",
            Self::MemoryGateway => "memory_gateway",
            Self::DevFull => "dev_full",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelContextLimit {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub max_context_tokens: Option<usize>,
    pub max_prompt_chars: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeBudgetInput {
    pub profile: ProfileId,
    pub resource_snapshot: RuntimeResourceSnapshot,
    pub static_platform_manifest: StaticPlatformManifest,
    pub provider_model_context_limit: Option<ProviderModelContextLimit>,
}

impl RuntimeBudgetInput {
    pub fn static_for_profile(profile: ProfileId) -> Self {
        let now_secs = crate::util::current_unix_secs();
        Self {
            profile,
            resource_snapshot: RuntimeResourceSnapshot::unavailable(
                now_secs,
                RuntimeResourceProbeSource::StaticManifest,
                RuntimeResourceUnavailableReason::ProbeNotConfigured,
            ),
            static_platform_manifest: StaticPlatformManifest::for_profile(profile),
            provider_model_context_limit: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCoreBudget {
    pub profile_max_records: usize,
    pub recall_working_set_max_items: usize,
    pub long_term_scan_max_items: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreRuntimeBudget {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterRuntimeBudget {
    pub http_header_max_bytes: usize,
    pub http_body_max_bytes: usize,
    pub wss_frame_max_bytes: usize,
    pub wss_max_subscriptions: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionSourceBudget {
    pub context_assembly_max_chars: usize,
    pub recent_messages_limit: usize,
    pub recall_candidate_max_items: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionRenderBudget {
    pub system_block_max_chars: usize,
    pub provider_prompt_max_chars: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceBudget {
    pub user_input_max_chars: usize,
    pub user_input_max_bytes: usize,
    pub reply_input_max_chars: usize,
    pub reply_input_max_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeJobBudget {
    pub max_concurrent_jobs: usize,
    pub max_background_jobs: usize,
    pub maintenance_batch_max_items: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmGatewayBudget {
    pub runtime_cache_max_runtimes: usize,
    pub projection_render_max_chars: usize,
    pub recent_messages_limit: usize,
    pub maintenance_user_max_chars: usize,
    pub maintenance_reply_max_chars: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptGovernanceBudget {
    pub transcript_page_size: usize,
    pub host_refs_per_turn: usize,
    pub max_attrs_per_turn: usize,
    pub max_attrs_per_message: usize,
    pub redaction_items_per_page: usize,
    pub derived_refs_per_report: usize,
    pub repair_issues_per_report: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBudgetReport {
    pub report_id: String,
    pub profile: ProfileId,
    pub deployment_role: RuntimeDeploymentRole,
    pub resource_snapshot: RuntimeResourceSnapshot,
    pub static_platform_manifest: StaticPlatformManifest,
    pub provider_model_context_limit: Option<ProviderModelContextLimit>,
    pub memory_core_budget: MemoryCoreBudget,
    pub store_budget: StoreRuntimeBudget,
    pub adapter_budget: AdapterRuntimeBudget,
    pub projection_source_budget: ProjectionSourceBudget,
    pub projection_render_budget: ProjectionRenderBudget,
    pub maintenance_budget: MaintenanceBudget,
    pub runtime_job_budget: RuntimeJobBudget,
    pub llm_gateway_budget: LlmGatewayBudget,
    pub transcript_governance_budget: TranscriptGovernanceBudget,
    pub limited_by: Vec<String>,
    pub unavailable_reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTranscriptRetentionPolicy {
    pub max_recent_turns: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryRetentionPolicy {
    pub refresh_after_turns: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaneQuotaPolicy {
    pub plane: String,
    pub max_records: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreCompactionPolicy {
    pub store_snapshot_max_bytes: usize,
    pub compact_when_pressure: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRetentionQuotaReport {
    pub owner: String,
    pub session_transcript: SessionTranscriptRetentionPolicy,
    pub session_summary: SessionSummaryRetentionPolicy,
    pub long_term_quota: PlaneQuotaPolicy,
    pub archive_quota: PlaneQuotaPolicy,
    pub procedural_quota: PlaneQuotaPolicy,
    pub private_garden_quota: PlaneQuotaPolicy,
    pub compaction: StoreCompactionPolicy,
    pub migration_import_pressure_report: bool,
    pub host_direct_deletion_allowed: Option<bool>,
    pub fail_closed_repair: bool,
}

impl RuntimeBudgetReport {
    pub fn static_for_profile(profile: ProfileId) -> Self {
        compile_runtime_budget(RuntimeBudgetInput::static_for_profile(profile))
    }

    pub fn projection_render_chars_for_request(
        &self,
        request_system_max_len: usize,
        provider_limit: Option<&ProviderModelContextLimit>,
    ) -> usize {
        let mut limit = self.projection_render_budget.system_block_max_chars;
        if request_system_max_len > 0 {
            limit = limit.min(request_system_max_len);
        }
        if let Some(provider_limit) = provider_limit {
            if let Some(max_prompt_chars) = provider_limit.max_prompt_chars {
                limit = limit.min(max_prompt_chars);
            }
        }
        limit.max(1)
    }

    pub fn retention_quota_report(&self) -> RuntimeRetentionQuotaReport {
        let max_records = self.memory_core_budget.profile_max_records.max(1);
        let recent_turns = self
            .projection_source_budget
            .recent_messages_limit
            .saturating_mul(2)
            .max(1);
        RuntimeRetentionQuotaReport {
            owner: "sdk.runtime".to_string(),
            session_transcript: SessionTranscriptRetentionPolicy {
                max_recent_turns: recent_turns,
            },
            session_summary: SessionSummaryRetentionPolicy {
                refresh_after_turns: self.runtime_job_budget.maintenance_batch_max_items.max(1),
            },
            long_term_quota: PlaneQuotaPolicy {
                plane: "long_term".to_string(),
                max_records,
            },
            archive_quota: PlaneQuotaPolicy {
                plane: "archive".to_string(),
                max_records: max_records / 2,
            },
            procedural_quota: PlaneQuotaPolicy {
                plane: "procedural".to_string(),
                max_records: self.store_budget.kv_max_entries.max(1),
            },
            private_garden_quota: PlaneQuotaPolicy {
                plane: "private_garden".to_string(),
                max_records: max_records / 4,
            },
            compaction: StoreCompactionPolicy {
                store_snapshot_max_bytes: self.store_budget.snapshot_max_bytes,
                compact_when_pressure: !self.limited_by.is_empty(),
            },
            migration_import_pressure_report: true,
            host_direct_deletion_allowed: None,
            fail_closed_repair: true,
        }
    }
}

pub fn compile_runtime_budget(input: RuntimeBudgetInput) -> RuntimeBudgetReport {
    let ceiling = profile_budget_ceiling(input.profile);
    let mut limited_by = Vec::new();
    let mut unavailable_reasons = Vec::new();
    if let Some(reason) = input.resource_snapshot.unavailable_reason {
        unavailable_reasons.push(reason.as_str().to_string());
        limited_by.push("runtime_resource_snapshot_unavailable".to_string());
    }
    if input.resource_snapshot.stale {
        limited_by.push("runtime_resource_snapshot_stale".to_string());
    }
    let pressure = input.resource_snapshot.pressure;
    if pressure != PressureLevel::Normal {
        limited_by.push(format!("resource_pressure:{}", pressure.as_str()));
    }
    let memory_scale = memory_scale(&input.resource_snapshot, &input.static_platform_manifest);
    if memory_scale < 100 {
        limited_by.push(format!("memory_scale:{memory_scale}"));
    }
    let storage_scale = storage_scale(&input.resource_snapshot, &input.static_platform_manifest);
    if storage_scale < 100 {
        limited_by.push(format!("storage_scale:{storage_scale}"));
    }
    let pressure_scale = pressure_scale(pressure);
    let source_scale = memory_scale.min(pressure_scale);
    let store_scale = storage_scale.min(pressure_scale.max(50));
    let render_provider_cap = input
        .provider_model_context_limit
        .as_ref()
        .and_then(|limit| limit.max_prompt_chars);
    if render_provider_cap.is_some() {
        limited_by.push("provider_model_context_limit".to_string());
    }

    let memory_core_budget = MemoryCoreBudget {
        profile_max_records: scale_usize(
            ceiling.memory_core_budget.profile_max_records,
            source_scale,
        )
        .max(ceiling.p0_min_records),
        recall_working_set_max_items: scale_usize(
            ceiling.memory_core_budget.recall_working_set_max_items,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
        long_term_scan_max_items: scale_usize(
            ceiling.memory_core_budget.long_term_scan_max_items,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
    };
    let store_budget = StoreRuntimeBudget {
        event_log_max_items: scale_usize(ceiling.store_budget.event_log_max_items, store_scale)
            .max(ceiling.p0_min_events),
        kv_max_entries: scale_usize(ceiling.store_budget.kv_max_entries, store_scale)
            .max(ceiling.p0_min_records),
        blob_max_bytes: scale_usize(ceiling.store_budget.blob_max_bytes, store_scale)
            .max(ceiling.p0_min_blob_bytes),
        snapshot_max_bytes: scale_usize(ceiling.store_budget.snapshot_max_bytes, store_scale)
            .max(ceiling.p0_min_snapshot_bytes),
        logical_namespace_max_bytes: ceiling.store_budget.logical_namespace_max_bytes,
        logical_key_max_bytes: scale_usize(ceiling.store_budget.logical_key_max_bytes, store_scale)
            .max(128),
        event_record_key_max_bytes: scale_usize(
            ceiling.store_budget.event_record_key_max_bytes,
            store_scale,
        )
        .max(128),
        export_max_bytes: scale_usize(ceiling.store_budget.export_max_bytes, store_scale)
            .max(ceiling.p0_min_snapshot_bytes),
        import_max_bytes: scale_usize(ceiling.store_budget.import_max_bytes, store_scale)
            .max(ceiling.p0_min_snapshot_bytes),
    };
    let adapter_budget = AdapterRuntimeBudget {
        http_header_max_bytes: ceiling.adapter_budget.http_header_max_bytes,
        http_body_max_bytes: scale_usize(ceiling.adapter_budget.http_body_max_bytes, source_scale)
            .max(ceiling.p0_min_http_body_bytes),
        wss_frame_max_bytes: scale_usize(ceiling.adapter_budget.wss_frame_max_bytes, source_scale)
            .max(ceiling.p0_min_wss_frame_bytes),
        wss_max_subscriptions: scale_usize(
            ceiling.adapter_budget.wss_max_subscriptions,
            source_scale,
        )
        .max(1),
    };
    let projection_source_budget = ProjectionSourceBudget {
        context_assembly_max_chars: scale_usize(
            ceiling.projection_source_budget.context_assembly_max_chars,
            source_scale,
        )
        .max(ceiling.p0_min_projection_source_chars),
        recent_messages_limit: scale_usize(
            ceiling.projection_source_budget.recent_messages_limit,
            source_scale,
        )
        .max(1),
        recall_candidate_max_items: scale_usize(
            ceiling.projection_source_budget.recall_candidate_max_items,
            source_scale,
        )
        .max(1),
    };
    let render_max = scale_usize(
        ceiling.projection_render_budget.system_block_max_chars,
        pressure_scale,
    )
    .max(ceiling.p0_min_projection_render_chars);
    let projection_render_budget = ProjectionRenderBudget {
        system_block_max_chars: render_provider_cap.map_or(render_max, |cap| render_max.min(cap)),
        provider_prompt_max_chars: render_provider_cap,
    };
    let maintenance_budget = MaintenanceBudget {
        user_input_max_chars: scale_usize(
            ceiling.maintenance_budget.user_input_max_chars,
            source_scale,
        )
        .max(ceiling.p0_min_maintenance_chars),
        user_input_max_bytes: scale_usize(
            ceiling.maintenance_budget.user_input_max_bytes,
            source_scale,
        )
        .max(ceiling.p0_min_maintenance_bytes),
        reply_input_max_chars: scale_usize(
            ceiling.maintenance_budget.reply_input_max_chars,
            source_scale,
        )
        .max(ceiling.p0_min_maintenance_chars),
        reply_input_max_bytes: scale_usize(
            ceiling.maintenance_budget.reply_input_max_bytes,
            source_scale,
        )
        .max(ceiling.p0_min_maintenance_bytes),
    };
    let runtime_job_budget = RuntimeJobBudget {
        max_concurrent_jobs: scale_usize(
            ceiling.runtime_job_budget.max_concurrent_jobs,
            source_scale,
        )
        .max(1),
        max_background_jobs: scale_usize(
            ceiling.runtime_job_budget.max_background_jobs,
            source_scale,
        )
        .max(1),
        maintenance_batch_max_items: scale_usize(
            ceiling.runtime_job_budget.maintenance_batch_max_items,
            source_scale,
        )
        .max(1),
    };
    let llm_gateway_budget = LlmGatewayBudget {
        runtime_cache_max_runtimes: scale_usize(
            ceiling.llm_gateway_budget.runtime_cache_max_runtimes,
            source_scale,
        )
        .max(1),
        projection_render_max_chars: projection_render_budget.system_block_max_chars,
        recent_messages_limit: projection_source_budget.recent_messages_limit,
        maintenance_user_max_chars: maintenance_budget.user_input_max_chars,
        maintenance_reply_max_chars: maintenance_budget.reply_input_max_chars,
    };
    let transcript_governance_budget = TranscriptGovernanceBudget {
        transcript_page_size: scale_usize(
            ceiling.transcript_governance_budget.transcript_page_size,
            source_scale,
        )
        .max(1),
        host_refs_per_turn: scale_usize(
            ceiling.transcript_governance_budget.host_refs_per_turn,
            source_scale,
        )
        .max(1),
        max_attrs_per_turn: scale_usize(
            ceiling.transcript_governance_budget.max_attrs_per_turn,
            source_scale,
        )
        .max(1),
        max_attrs_per_message: scale_usize(
            ceiling.transcript_governance_budget.max_attrs_per_message,
            source_scale,
        )
        .max(1),
        redaction_items_per_page: scale_usize(
            ceiling
                .transcript_governance_budget
                .redaction_items_per_page,
            source_scale,
        )
        .max(1),
        derived_refs_per_report: scale_usize(
            ceiling.transcript_governance_budget.derived_refs_per_report,
            source_scale,
        )
        .max(1),
        repair_issues_per_report: scale_usize(
            ceiling
                .transcript_governance_budget
                .repair_issues_per_report,
            source_scale,
        )
        .max(1),
    };
    let report_id = report_id(
        input.profile,
        input.resource_snapshot.source,
        input.resource_snapshot.observed_at_unix_secs,
        memory_scale,
        storage_scale,
        pressure,
    );
    RuntimeBudgetReport {
        report_id,
        profile: input.profile,
        deployment_role: RuntimeDeploymentRole::from_profile(input.profile),
        resource_snapshot: input.resource_snapshot,
        static_platform_manifest: input.static_platform_manifest,
        provider_model_context_limit: input.provider_model_context_limit,
        memory_core_budget,
        store_budget,
        adapter_budget,
        projection_source_budget,
        projection_render_budget,
        maintenance_budget,
        runtime_job_budget,
        llm_gateway_budget,
        transcript_governance_budget,
        limited_by,
        unavailable_reasons,
    }
}

fn scale_usize(value: usize, percent: u32) -> usize {
    value.saturating_mul(percent as usize) / 100
}

fn pressure_scale(pressure: PressureLevel) -> u32 {
    match pressure {
        PressureLevel::Normal => 100,
        PressureLevel::Cautious => 60,
        PressureLevel::Critical => 35,
    }
}

fn memory_scale(snapshot: &RuntimeResourceSnapshot, manifest: &StaticPlatformManifest) -> u32 {
    let Some(available) = snapshot
        .memory_available_bytes
        .or(snapshot.internal_heap_free_bytes)
        .or(snapshot.psram_free_bytes)
    else {
        return 50;
    };
    if available <= manifest.memory_floor_bytes {
        return 50;
    }
    if available <= manifest.memory_floor_bytes.saturating_mul(2) {
        return 75;
    }
    100
}

fn storage_scale(snapshot: &RuntimeResourceSnapshot, manifest: &StaticPlatformManifest) -> u32 {
    let Some(available) = snapshot.storage_available_bytes else {
        return 60;
    };
    if available <= manifest.storage_floor_bytes {
        return 50;
    }
    if available <= manifest.storage_floor_bytes.saturating_mul(2) {
        return 75;
    }
    100
}

fn report_id(
    profile: ProfileId,
    source: RuntimeResourceProbeSource,
    observed_at_unix_secs: u64,
    memory_scale: u32,
    storage_scale: u32,
    pressure: PressureLevel,
) -> String {
    let mut hasher = DefaultHasher::new();
    profile.as_str().hash(&mut hasher);
    source.as_str().hash(&mut hasher);
    observed_at_unix_secs.hash(&mut hasher);
    memory_scale.hash(&mut hasher);
    storage_scale.hash(&mut hasher);
    pressure.as_str().hash(&mut hasher);
    format!("rtb-{hash:016x}", hash = hasher.finish())
}

#[derive(Clone, Copy)]
struct ProfileBudgetCeiling {
    memory_floor_bytes: u64,
    storage_floor_bytes: u64,
    memory_core_budget: MemoryCoreBudget,
    store_budget: StoreRuntimeBudget,
    adapter_budget: AdapterRuntimeBudget,
    projection_source_budget: ProjectionSourceBudget,
    projection_render_budget: ProjectionRenderBudget,
    maintenance_budget: MaintenanceBudget,
    runtime_job_budget: RuntimeJobBudget,
    llm_gateway_budget: LlmGatewayBudget,
    transcript_governance_budget: TranscriptGovernanceBudget,
    p0_min_records: usize,
    p0_min_recall_items: usize,
    p0_min_events: usize,
    p0_min_blob_bytes: usize,
    p0_min_snapshot_bytes: usize,
    p0_min_http_body_bytes: usize,
    p0_min_wss_frame_bytes: usize,
    p0_min_projection_source_chars: usize,
    p0_min_projection_render_chars: usize,
    p0_min_maintenance_chars: usize,
    p0_min_maintenance_bytes: usize,
}

const fn profile_budget_ceiling(profile: ProfileId) -> ProfileBudgetCeiling {
    match profile {
        ProfileId::EspEmbeddedSdk => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 128 * MB,
            storage_floor_bytes: 8 * MB,
            records: 512,
            events: 256,
            blob_max_bytes: 1024 * 1024,
            snapshot_max_bytes: 256 * 1024,
            http_body_max_bytes: 8 * 1024,
            source_chars: 1024,
            render_chars: 2048,
            maintenance_chars: 1024,
            runtime_cache_max_runtimes: 8,
            wss_subscriptions: 4,
        }),
        ProfileId::EspStandaloneMemory => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 256 * MB,
            storage_floor_bytes: 16 * MB,
            records: 4096,
            events: 2048,
            blob_max_bytes: 4 * 1024 * 1024,
            snapshot_max_bytes: 1024 * 1024,
            http_body_max_bytes: 16 * 1024,
            source_chars: 2048,
            render_chars: 4096,
            maintenance_chars: 2048,
            runtime_cache_max_runtimes: 16,
            wss_subscriptions: 8,
        }),
        ProfileId::LinuxDeviceStandaloneMemory => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 512 * MB,
            storage_floor_bytes: 256 * MB,
            records: 12_000,
            events: 4096,
            blob_max_bytes: 16 * 1024 * 1024,
            snapshot_max_bytes: 4 * 1024 * 1024,
            http_body_max_bytes: 64 * 1024,
            source_chars: 4096,
            render_chars: 8192,
            maintenance_chars: 4096,
            runtime_cache_max_runtimes: 32,
            wss_subscriptions: 16,
        }),
        ProfileId::DesktopMacosEmbeddedSdk | ProfileId::DesktopWindowsEmbeddedSdk => {
            profile_budget(ProfileBudgetSpec {
                memory_floor_bytes: 512 * MB,
                storage_floor_bytes: 256 * MB,
                records: 4096,
                events: 2048,
                blob_max_bytes: 8 * 1024 * 1024,
                snapshot_max_bytes: 2 * 1024 * 1024,
                http_body_max_bytes: 32 * 1024,
                source_chars: 2048,
                render_chars: 4096,
                maintenance_chars: 2048,
                runtime_cache_max_runtimes: 16,
                wss_subscriptions: 8,
            })
        }
        ProfileId::DesktopMacosStandaloneMemory => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 1024 * MB,
            storage_floor_bytes: 512 * MB,
            records: 20_000,
            events: 8192,
            blob_max_bytes: 32 * 1024 * 1024,
            snapshot_max_bytes: 8 * 1024 * 1024,
            http_body_max_bytes: 96 * 1024,
            source_chars: 8192,
            render_chars: 12_288,
            maintenance_chars: 8192,
            runtime_cache_max_runtimes: 64,
            wss_subscriptions: 32,
        }),
        ProfileId::ServerLinuxMemoryGateway => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 1024 * MB,
            storage_floor_bytes: 1024 * MB,
            records: 40_000,
            events: 16_384,
            blob_max_bytes: 64 * 1024 * 1024,
            snapshot_max_bytes: 16 * 1024 * 1024,
            http_body_max_bytes: 128 * 1024,
            source_chars: 8192,
            render_chars: 16_384,
            maintenance_chars: 8192,
            runtime_cache_max_runtimes: 128,
            wss_subscriptions: 64,
        }),
        ProfileId::ServerLinuxDevFull => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 2048 * MB,
            storage_floor_bytes: 2048 * MB,
            records: 80_000,
            events: 32_768,
            blob_max_bytes: 128 * 1024 * 1024,
            snapshot_max_bytes: 32 * 1024 * 1024,
            http_body_max_bytes: 256 * 1024,
            source_chars: 16_384,
            render_chars: 32_768,
            maintenance_chars: 16_384,
            runtime_cache_max_runtimes: 256,
            wss_subscriptions: 128,
        }),
    }
}

const MB: u64 = 1024 * 1024;

struct ProfileBudgetSpec {
    memory_floor_bytes: u64,
    storage_floor_bytes: u64,
    records: usize,
    events: usize,
    blob_max_bytes: usize,
    snapshot_max_bytes: usize,
    http_body_max_bytes: usize,
    source_chars: usize,
    render_chars: usize,
    maintenance_chars: usize,
    runtime_cache_max_runtimes: usize,
    wss_subscriptions: usize,
}

const fn profile_budget(spec: ProfileBudgetSpec) -> ProfileBudgetCeiling {
    ProfileBudgetCeiling {
        memory_floor_bytes: spec.memory_floor_bytes,
        storage_floor_bytes: spec.storage_floor_bytes,
        memory_core_budget: MemoryCoreBudget {
            profile_max_records: spec.records,
            recall_working_set_max_items: max_usize(spec.records / 16, 16),
            long_term_scan_max_items: max_usize(spec.records / 8, 32),
        },
        store_budget: StoreRuntimeBudget {
            event_log_max_items: spec.events,
            kv_max_entries: spec.records,
            blob_max_bytes: spec.blob_max_bytes,
            snapshot_max_bytes: spec.snapshot_max_bytes,
            logical_namespace_max_bytes: 96,
            logical_key_max_bytes: max_usize(spec.snapshot_max_bytes / 1024, 512),
            event_record_key_max_bytes: max_usize(spec.snapshot_max_bytes / 1024, 512),
            export_max_bytes: spec.snapshot_max_bytes,
            import_max_bytes: spec.snapshot_max_bytes,
        },
        adapter_budget: AdapterRuntimeBudget {
            http_header_max_bytes: 16 * 1024,
            http_body_max_bytes: spec.http_body_max_bytes,
            wss_frame_max_bytes: spec.http_body_max_bytes,
            wss_max_subscriptions: spec.wss_subscriptions,
        },
        projection_source_budget: ProjectionSourceBudget {
            context_assembly_max_chars: spec.source_chars,
            recent_messages_limit: max_usize(spec.source_chars / 256, 4),
            recall_candidate_max_items: max_usize(spec.source_chars / 256, 4),
        },
        projection_render_budget: ProjectionRenderBudget {
            system_block_max_chars: spec.render_chars,
            provider_prompt_max_chars: None,
        },
        maintenance_budget: MaintenanceBudget {
            user_input_max_chars: spec.maintenance_chars,
            user_input_max_bytes: spec.maintenance_chars * 2,
            reply_input_max_chars: spec.maintenance_chars,
            reply_input_max_bytes: spec.maintenance_chars * 2,
        },
        runtime_job_budget: RuntimeJobBudget {
            max_concurrent_jobs: max_usize(spec.runtime_cache_max_runtimes / 4, 1),
            max_background_jobs: max_usize(spec.runtime_cache_max_runtimes / 8, 1),
            maintenance_batch_max_items: max_usize(spec.records / 64, 4),
        },
        llm_gateway_budget: LlmGatewayBudget {
            runtime_cache_max_runtimes: spec.runtime_cache_max_runtimes,
            projection_render_max_chars: spec.render_chars,
            recent_messages_limit: max_usize(spec.source_chars / 256, 4),
            maintenance_user_max_chars: spec.maintenance_chars,
            maintenance_reply_max_chars: spec.maintenance_chars,
        },
        transcript_governance_budget: TranscriptGovernanceBudget {
            transcript_page_size: max_usize(spec.source_chars / 256, 4),
            host_refs_per_turn: max_usize(spec.http_body_max_bytes / (8 * 1024), 1),
            max_attrs_per_turn: max_usize(spec.source_chars / 512, 2),
            max_attrs_per_message: max_usize(spec.source_chars / 512, 2),
            redaction_items_per_page: max_usize(spec.source_chars / 128, 8),
            derived_refs_per_report: max_usize(spec.records / 512, 4),
            repair_issues_per_report: max_usize(spec.events / 512, 4),
        },
        p0_min_records: 64,
        p0_min_recall_items: 4,
        p0_min_events: 64,
        p0_min_blob_bytes: 64 * 1024,
        p0_min_snapshot_bytes: 64 * 1024,
        p0_min_http_body_bytes: 4 * 1024,
        p0_min_wss_frame_bytes: 4 * 1024,
        p0_min_projection_source_chars: 1024,
        p0_min_projection_render_chars: 512,
        p0_min_maintenance_chars: 512,
        p0_min_maintenance_bytes: 1024,
    }
}

const fn max_usize(left: usize, right: usize) -> usize {
    if left >= right {
        left
    } else {
        right
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_limit_only_caps_render_budget() {
        let mut input = RuntimeBudgetInput::static_for_profile(ProfileId::ServerLinuxMemoryGateway);
        input.provider_model_context_limit = Some(ProviderModelContextLimit {
            provider: Some("local".to_string()),
            model: Some("qwen".to_string()),
            max_context_tokens: None,
            max_prompt_chars: Some(2048),
        });
        let report = compile_runtime_budget(input);
        assert_eq!(report.projection_render_budget.system_block_max_chars, 2048);
        assert!(report.projection_source_budget.context_assembly_max_chars > 2048);
        assert!(report
            .limited_by
            .contains(&"provider_model_context_limit".to_string()));
    }

    #[test]
    fn eight_profiles_have_distinct_budget_reports() {
        let profiles = [
            ProfileId::EspStandaloneMemory,
            ProfileId::EspEmbeddedSdk,
            ProfileId::LinuxDeviceStandaloneMemory,
            ProfileId::DesktopMacosStandaloneMemory,
            ProfileId::DesktopMacosEmbeddedSdk,
            ProfileId::DesktopWindowsEmbeddedSdk,
            ProfileId::ServerLinuxMemoryGateway,
            ProfileId::ServerLinuxDevFull,
        ];
        let mut render_budgets = Vec::new();
        for profile in profiles {
            render_budgets.push(
                RuntimeBudgetReport::static_for_profile(profile)
                    .projection_render_budget
                    .system_block_max_chars,
            );
        }
        render_budgets.sort_unstable();
        render_budgets.dedup();
        assert!(render_budgets.len() >= 6);
    }

    #[test]
    fn transcript_governance_budget_is_profile_specific() {
        let compact = RuntimeBudgetReport::static_for_profile(ProfileId::EspEmbeddedSdk)
            .transcript_governance_budget;
        let server = RuntimeBudgetReport::static_for_profile(ProfileId::ServerLinuxDevFull)
            .transcript_governance_budget;

        assert!(compact.transcript_page_size > 0);
        assert!(compact.host_refs_per_turn > 0);
        assert!(compact.max_attrs_per_turn > 0);
        assert!(compact.max_attrs_per_message > 0);
        assert!(compact.redaction_items_per_page > 0);
        assert!(compact.derived_refs_per_report > 0);
        assert!(compact.repair_issues_per_report > 0);
        assert!(compact.transcript_page_size < server.transcript_page_size);
        assert!(compact.max_attrs_per_turn < server.max_attrs_per_turn);
        assert!(compact.max_attrs_per_message < server.max_attrs_per_message);
        assert!(compact.derived_refs_per_report < server.derived_refs_per_report);
        assert!(compact.repair_issues_per_report < server.repair_issues_per_report);
    }
}
