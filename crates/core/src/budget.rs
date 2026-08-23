use crate::feature_gate::{ProfileId, RoleFeature, TargetFeature};
use crate::memory::{
    MAX_EVIDENCE_DOCUMENT_FACET_LEXICAL_TERMS, MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES,
    MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES, MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS,
    MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES,
};
use crate::orchestrator::PressureLevel;
use crate::resource::{
    HostRuntimeResourceProbe, HostStorageObservation, RuntimeResourceProbe,
    RuntimeResourceProbeRegistration, RuntimeResourceProbeSource, RuntimeResourceSnapshot,
    RuntimeResourceSnapshotCache,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex, RwLock};

// Lexical postings consume the first half; the chunk-sized half reserves the owner/index/graph closure.
const EVIDENCE_DOCUMENT_KV_ENTRY_ENVELOPE_PER_DOCUMENT: usize =
    MAX_EVIDENCE_DOCUMENT_FACET_LEXICAL_TERMS + MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS + 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticPlatformManifest {
    pub manifest_id: String,
    pub profile: ProfileId,
    pub deployment_target: TargetFeature,
    pub deployment_role: RoleFeature,
    pub store_medium: RuntimeStoreMedium,
    pub memory_floor_bytes: u64,
    pub storage_floor_bytes: u64,
    pub notes: Vec<String>,
}

impl StaticPlatformManifest {
    pub fn for_profile(profile: ProfileId, store_medium: RuntimeStoreMedium) -> Self {
        let ceiling = profile_budget_ceiling(profile);
        Self {
            manifest_id: format!(
                "static-manifest:{}:{}",
                profile.as_str(),
                store_medium.as_str()
            ),
            profile,
            deployment_target: profile.target(),
            deployment_role: profile.role(),
            store_medium,
            memory_floor_bytes: ceiling.memory_floor_bytes,
            storage_floor_bytes: ceiling.storage_floor_bytes,
            notes: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStoreMedium {
    VolatileMemory,
    PersistentFilesystem,
    EmbeddedFlash,
}

impl RuntimeStoreMedium {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VolatileMemory => "volatile_memory",
            Self::PersistentFilesystem => "persistent_filesystem",
            Self::EmbeddedFlash => "embedded_flash",
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCoreBudget {
    pub profile_max_records: usize,
    pub recall_working_set_max_items: usize,
    pub long_term_scan_max_items: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphExpansionRuntimeBudget {
    pub max_hops: u8,
    pub max_seed_candidates: usize,
    pub max_expanded_candidates: usize,
    pub max_neighbors_per_candidate: usize,
    pub max_graph_nodes_loaded: usize,
    pub max_graph_edges_loaded: usize,
    pub max_backlinks_loaded: usize,
    pub compact_graph_node_limit: usize,
    pub compact_graph_edge_limit: usize,
    pub default_recall_multi_hop_allowed: bool,
    pub eval_recall_multi_hop_allowed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetRecallRuntimeBudget {
    pub max_query_facets: usize,
    pub max_facet_index_docs_read: usize,
    pub max_facet_anchor_candidates: usize,
    pub max_facet_expanded_candidates: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallDeliveryRuntimeBudget {
    pub max_selected_candidates: usize,
    pub max_rendered_capsules: usize,
    pub max_capsule_chars: usize,
    pub max_loss_ledger_entries: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GovernedStateRuntimeBudget {
    pub max_validity_joins: usize,
    pub max_lineage_depth: usize,
    pub max_retained_long_term_revisions_per_owner: usize,
    pub max_retained_runtime_skill_owners_per_scope: usize,
    pub max_runtime_skill_lineage_depth: usize,
    pub max_as_of_candidates: usize,
    pub max_obsolete_decisions: usize,
    pub max_procedural_candidates: usize,
    pub max_premises_per_skill: usize,
    pub max_premise_evidence_reads: usize,
    pub max_state_transitions_per_write: usize,
    pub max_subject_soul_manifest_entries: usize,
    pub max_subject_soul_revisions_per_generation: usize,
    pub max_subject_soul_generation_tombstones: usize,
    pub max_subject_soul_transaction_mutations: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceDocumentRuntimeBudget {
    pub max_document_bytes: usize,
    pub max_document_body_bytes: usize,
    pub max_chunk_bytes: usize,
    pub max_chunks_per_document: usize,
    pub max_documents_per_transaction: usize,
    pub max_documents_per_read: usize,
    pub max_total_bytes_per_transaction: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreRuntimeBudget {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterRuntimeBudget {
    pub http_header_max_bytes: usize,
    pub http_body_max_bytes: usize,
    pub wss_frame_max_bytes: usize,
    pub wss_max_subscriptions: usize,
    pub wss_session_max_frames: usize,
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
    pub buffered_json_max_bytes: usize,
    pub stream_chunk_max_bytes: usize,
    pub stream_event_max_bytes: usize,
    pub stream_max_events: usize,
    pub response_body_max_bytes: usize,
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
// runtime-budget-public-surface: authority-only-report
pub struct RuntimeBudgetReport {
    pub report_id: String,
    pub profile: ProfileId,
    pub deployment_target: TargetFeature,
    pub deployment_role: RoleFeature,
    pub store_medium: RuntimeStoreMedium,
    pub resource_snapshot: RuntimeResourceSnapshot,
    pub static_platform_manifest: StaticPlatformManifest,
    pub provider_model_context_limit: Option<ProviderModelContextLimit>,
    pub memory_core_budget: MemoryCoreBudget,
    pub graph_expansion_budget: GraphExpansionRuntimeBudget,
    pub facet_recall_budget: FacetRecallRuntimeBudget,
    pub recall_delivery_budget: RecallDeliveryRuntimeBudget,
    pub governed_state_budget: GovernedStateRuntimeBudget,
    pub evidence_document_budget: EvidenceDocumentRuntimeBudget,
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

impl RuntimeBudgetReport {
    pub fn validate_for_admission(&self, now_secs: u64) -> Result<()> {
        if self.resource_snapshot.stale || self.resource_snapshot.is_expired(now_secs) {
            return Err(Error::config(
                "runtime_budget_admission",
                "runtime budget report is stale or expired",
            ));
        }
        let expected = finalize_runtime_budget_report_id(self.clone());
        if expected.report_id != self.report_id {
            return Err(Error::config(
                "runtime_budget_admission",
                "runtime budget report identity does not cover its payload",
            ));
        }
        Ok(())
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchmarkStoreCapacityExtension {
    report_id: String,
    capacity: StoreRuntimeBudget,
}

#[cfg(feature = "nonproduction-replay-harness")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NonproductionRuntimeBudgetLimits {
    graph_expansion_budget: Option<GraphExpansionRuntimeBudget>,
    facet_recall_budget: Option<FacetRecallRuntimeBudget>,
    recall_delivery_budget: Option<RecallDeliveryRuntimeBudget>,
    transcript_governance_budget: Option<TranscriptGovernanceBudget>,
    store_budget_limit: Option<StoreRuntimeBudget>,
}

#[cfg(feature = "nonproduction-replay-harness")]
impl NonproductionRuntimeBudgetLimits {
    pub const fn new() -> Self {
        Self {
            graph_expansion_budget: None,
            facet_recall_budget: None,
            recall_delivery_budget: None,
            transcript_governance_budget: None,
            store_budget_limit: None,
        }
    }

    pub fn try_with_graph_expansion_budget(
        mut self,
        budget: GraphExpansionRuntimeBudget,
    ) -> Result<Self> {
        validate_graph_expansion_budget(budget)?;
        self.graph_expansion_budget = Some(budget);
        Ok(self)
    }

    pub fn try_with_facet_recall_budget(
        mut self,
        budget: FacetRecallRuntimeBudget,
    ) -> Result<Self> {
        validate_facet_recall_budget(budget)?;
        self.facet_recall_budget = Some(budget);
        Ok(self)
    }

    pub fn try_with_recall_delivery_budget(
        mut self,
        budget: RecallDeliveryRuntimeBudget,
    ) -> Result<Self> {
        validate_recall_delivery_budget(budget)?;
        self.recall_delivery_budget = Some(budget);
        Ok(self)
    }

    pub fn try_with_transcript_governance_budget(
        mut self,
        budget: TranscriptGovernanceBudget,
    ) -> Result<Self> {
        validate_transcript_governance_budget(budget)?;
        self.transcript_governance_budget = Some(budget);
        Ok(self)
    }

    pub fn try_with_store_budget_limit(mut self, budget: StoreRuntimeBudget) -> Result<Self> {
        validate_store_budget(budget, "runtime_budget_nonproduction_limits")?;
        self.store_budget_limit = Some(budget);
        Ok(self)
    }

    fn validate(&self) -> Result<()> {
        if let Some(budget) = self.graph_expansion_budget {
            validate_graph_expansion_budget(budget)?;
        }
        if let Some(budget) = self.facet_recall_budget {
            validate_facet_recall_budget(budget)?;
        }
        if let Some(budget) = self.recall_delivery_budget {
            validate_recall_delivery_budget(budget)?;
        }
        if let Some(budget) = self.transcript_governance_budget {
            validate_transcript_governance_budget(budget)?;
        }
        if let Some(budget) = self.store_budget_limit {
            validate_store_budget(budget, "runtime_budget_nonproduction_limits")?;
        }
        Ok(())
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
impl BenchmarkStoreCapacityExtension {
    pub fn try_new(report: &RuntimeBudgetReport, capacity: StoreRuntimeBudget) -> Result<Self> {
        validate_store_budget(capacity, "runtime_budget_benchmark_capacity")?;
        if !store_budget_is_at_least(capacity, report.store_budget) {
            return Err(Error::config(
                "runtime_budget_benchmark_capacity",
                "capacity_is_below_runtime_store_budget",
            ));
        }
        Ok(Self {
            report_id: report.report_id.clone(),
            capacity,
        })
    }

    pub fn report_id(&self) -> &str {
        &self.report_id
    }

    pub const fn capacity(&self) -> StoreRuntimeBudget {
        self.capacity
    }
}

#[derive(Debug)]
pub struct RuntimeBudgetAuthority {
    profile: ProfileId,
    static_platform_manifest: StaticPlatformManifest,
    provider_model_context_limit: Option<ProviderModelContextLimit>,
    probe_registration: RuntimeResourceProbeRegistration,
    compile_mode: RuntimeBudgetCompileMode,
    admission_store_ceiling: StoreRuntimeBudget,
    refresh_guard: Mutex<()>,
    state: RwLock<RuntimeBudgetAuthorityState>,
}

#[derive(Debug)]
struct RuntimeBudgetAuthorityState {
    resource_cache: RuntimeResourceSnapshotCache,
    report: RuntimeBudgetReport,
}

#[derive(Clone, Debug)]
enum RuntimeBudgetCompileMode {
    Production,
    #[cfg(feature = "nonproduction-replay-harness")]
    Nonproduction(Box<NonproductionRuntimeBudgetLimits>),
}

impl RuntimeBudgetAuthority {
    pub fn with_host_probe(
        profile: ProfileId,
        static_platform_manifest: StaticPlatformManifest,
        provider_model_context_limit: Option<ProviderModelContextLimit>,
        probe: HostRuntimeResourceProbe,
        now_secs: u64,
    ) -> Result<Self> {
        Self::new_with_mode(
            profile,
            static_platform_manifest,
            provider_model_context_limit,
            RuntimeResourceProbeRegistration::host(probe),
            RuntimeBudgetCompileMode::Production,
            now_secs,
        )
    }

    pub fn with_default_host_probe(
        profile: ProfileId,
        static_platform_manifest: StaticPlatformManifest,
        provider_model_context_limit: Option<ProviderModelContextLimit>,
        now_secs: u64,
    ) -> Result<Self> {
        Self::with_host_probe(
            profile,
            static_platform_manifest,
            provider_model_context_limit,
            HostRuntimeResourceProbe::for_volatile_memory(),
            now_secs,
        )
    }

    pub fn with_firmware_probe(
        profile: ProfileId,
        static_platform_manifest: StaticPlatformManifest,
        provider_model_context_limit: Option<ProviderModelContextLimit>,
        probe: Arc<dyn RuntimeResourceProbe>,
        now_secs: u64,
    ) -> Result<Self> {
        Self::new_with_mode(
            profile,
            static_platform_manifest,
            provider_model_context_limit,
            RuntimeResourceProbeRegistration::firmware(probe),
            RuntimeBudgetCompileMode::Production,
            now_secs,
        )
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn with_default_probe_nonproduction(
        profile: ProfileId,
        static_platform_manifest: StaticPlatformManifest,
        provider_model_context_limit: Option<ProviderModelContextLimit>,
        limits: NonproductionRuntimeBudgetLimits,
        now_secs: u64,
    ) -> Result<Self> {
        limits.validate()?;
        let probe_registration = default_probe_registration(profile, &static_platform_manifest)?;
        Self::new_with_mode(
            profile,
            static_platform_manifest,
            provider_model_context_limit,
            probe_registration,
            RuntimeBudgetCompileMode::Nonproduction(Box::new(limits)),
            now_secs,
        )
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn with_nonproduction_host_probe(
        profile: ProfileId,
        static_platform_manifest: StaticPlatformManifest,
        provider_model_context_limit: Option<ProviderModelContextLimit>,
        probe: Arc<dyn RuntimeResourceProbe>,
        limits: NonproductionRuntimeBudgetLimits,
        now_secs: u64,
    ) -> Result<Self> {
        limits.validate()?;
        let host_storage_observation =
            host_storage_observation_for_medium(static_platform_manifest.store_medium)?;
        Self::new_with_mode(
            profile,
            static_platform_manifest,
            provider_model_context_limit,
            RuntimeResourceProbeRegistration::nonproduction_host(probe, host_storage_observation),
            RuntimeBudgetCompileMode::Nonproduction(Box::new(limits)),
            now_secs,
        )
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn with_nonproduction_firmware_probe(
        profile: ProfileId,
        static_platform_manifest: StaticPlatformManifest,
        provider_model_context_limit: Option<ProviderModelContextLimit>,
        probe: Arc<dyn RuntimeResourceProbe>,
        limits: NonproductionRuntimeBudgetLimits,
        now_secs: u64,
    ) -> Result<Self> {
        limits.validate()?;
        Self::new_with_mode(
            profile,
            static_platform_manifest,
            provider_model_context_limit,
            RuntimeResourceProbeRegistration::firmware(probe),
            RuntimeBudgetCompileMode::Nonproduction(Box::new(limits)),
            now_secs,
        )
    }

    fn new_with_mode(
        profile: ProfileId,
        static_platform_manifest: StaticPlatformManifest,
        provider_model_context_limit: Option<ProviderModelContextLimit>,
        probe_registration: RuntimeResourceProbeRegistration,
        compile_mode: RuntimeBudgetCompileMode,
        now_secs: u64,
    ) -> Result<Self> {
        validate_authority_configuration(profile, &static_platform_manifest, &probe_registration)?;
        let snapshot = probe_registration
            .probe_snapshot(now_secs)
            .map_err(|error| {
                Error::config(
                    "runtime_budget_resource_probe",
                    format!("probe_failed:{error}"),
                )
            })?;
        validate_authority_snapshot(
            profile,
            probe_registration.attested_source(),
            static_platform_manifest.store_medium,
            &snapshot,
            now_secs,
        )?;
        let report = compile_authority_report(
            profile,
            &static_platform_manifest,
            provider_model_context_limit.as_ref(),
            &compile_mode,
            snapshot.clone(),
            None,
        );
        let admission_store_ceiling = report.store_budget;
        Ok(Self {
            profile,
            static_platform_manifest,
            provider_model_context_limit,
            probe_registration,
            compile_mode,
            admission_store_ceiling,
            refresh_guard: Mutex::new(()),
            state: RwLock::new(RuntimeBudgetAuthorityState {
                resource_cache: RuntimeResourceSnapshotCache::new(snapshot),
                report,
            }),
        })
    }

    pub const fn profile(&self) -> ProfileId {
        self.profile
    }

    pub const fn admission_store_ceiling(&self) -> StoreRuntimeBudget {
        self.admission_store_ceiling
    }

    pub fn current_report(&self, now_secs: u64) -> RuntimeBudgetReport {
        {
            let state = self.state.read().unwrap_or_else(|error| error.into_inner());
            if !state.resource_cache.requires_refresh(now_secs) {
                return state.report.clone();
            }
        }

        let _refresh_guard = self
            .refresh_guard
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        {
            let state = self.state.read().unwrap_or_else(|error| error.into_inner());
            if !state.resource_cache.requires_refresh(now_secs) {
                return state.report.clone();
            }
        }

        let snapshot = self
            .probe_registration
            .probe_snapshot(now_secs)
            .and_then(|snapshot| {
                validate_authority_snapshot(
                    self.profile,
                    self.probe_registration.attested_source(),
                    self.static_platform_manifest.store_medium,
                    &snapshot,
                    now_secs,
                )?;
                Ok(snapshot)
            })
            .unwrap_or_else(|error| {
                RuntimeResourceSnapshot::unavailable(
                    now_secs,
                    self.probe_registration.attested_source(),
                    crate::resource::RuntimeResourceUnavailableReason::ProbeFailed,
                )
                .with_unavailable_detail(format!("automatic_refresh_failed:{error}"))
            });
        let report = self.compile_report(snapshot.clone());
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        state.resource_cache.replace(snapshot);
        state.report = report.clone();
        report
    }

    pub fn current_snapshot(&self, now_secs: u64) -> RuntimeResourceSnapshot {
        self.current_report(now_secs).resource_snapshot
    }

    pub fn refresh(&self, now_secs: u64) -> Result<RuntimeBudgetReport> {
        let _refresh_guard = self
            .refresh_guard
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let snapshot = self
            .probe_registration
            .probe_snapshot(now_secs)
            .map_err(|error| {
                Error::config(
                    "runtime_budget_resource_probe",
                    format!("probe_failed:{error}"),
                )
            })?;
        validate_authority_snapshot(
            self.profile,
            self.probe_registration.attested_source(),
            self.static_platform_manifest.store_medium,
            &snapshot,
            now_secs,
        )?;
        let report = self.compile_report(snapshot.clone());
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        state.resource_cache.replace(snapshot);
        state.report = report.clone();
        Ok(report)
    }

    fn compile_report(&self, snapshot: RuntimeResourceSnapshot) -> RuntimeBudgetReport {
        compile_authority_report(
            self.profile,
            &self.static_platform_manifest,
            self.provider_model_context_limit.as_ref(),
            &self.compile_mode,
            snapshot,
            Some(self.admission_store_ceiling),
        )
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationOperationReceiptRetentionPolicy {
    PinnedUntilCapacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationOperationReceiptCapacityExhaustion {
    FailClosed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MutationOperationReceiptQuota {
    pub retention: MutationOperationReceiptRetentionPolicy,
    pub automatic_eviction: bool,
    pub capacity_exhaustion: MutationOperationReceiptCapacityExhaustion,
    pub durable_json_entries_per_operation: usize,
    pub durable_events_per_operation: usize,
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
    pub mutation_operation_receipts: MutationOperationReceiptQuota,
    pub migration_import_pressure_report: bool,
    pub host_direct_deletion_allowed: Option<bool>,
    pub fail_closed_repair: bool,
}

impl RuntimeBudgetReport {
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
            mutation_operation_receipts: MutationOperationReceiptQuota {
                retention: MutationOperationReceiptRetentionPolicy::PinnedUntilCapacity,
                automatic_eviction: false,
                capacity_exhaustion: MutationOperationReceiptCapacityExhaustion::FailClosed,
                durable_json_entries_per_operation: 2,
                durable_events_per_operation: 2,
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
    if input.static_platform_manifest.store_medium != RuntimeStoreMedium::VolatileMemory
        && storage_scale < 100
    {
        limited_by.push(format!("storage_scale:{storage_scale}"));
    }
    let pressure_scale = pressure_scale(pressure);
    let source_scale = memory_scale.min(pressure_scale);
    let store_scale = match input.static_platform_manifest.store_medium {
        RuntimeStoreMedium::VolatileMemory => memory_scale.min(pressure_scale),
        RuntimeStoreMedium::PersistentFilesystem => storage_scale.min(pressure_scale),
        RuntimeStoreMedium::EmbeddedFlash => memory_scale.min(storage_scale).min(pressure_scale),
    };
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
    let graph_expansion_budget = GraphExpansionRuntimeBudget {
        max_hops: ceiling.graph_expansion_budget.max_hops.clamp(1, 2),
        max_seed_candidates: scale_usize(
            ceiling.graph_expansion_budget.max_seed_candidates,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
        max_expanded_candidates: scale_usize(
            ceiling.graph_expansion_budget.max_expanded_candidates,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
        max_neighbors_per_candidate: scale_usize(
            ceiling.graph_expansion_budget.max_neighbors_per_candidate,
            source_scale,
        )
        .max(1),
        max_graph_nodes_loaded: scale_usize(
            ceiling.graph_expansion_budget.max_graph_nodes_loaded,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
        max_graph_edges_loaded: scale_usize(
            ceiling.graph_expansion_budget.max_graph_edges_loaded,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
        max_backlinks_loaded: scale_usize(
            ceiling.graph_expansion_budget.max_backlinks_loaded,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
        compact_graph_node_limit: scale_usize(
            ceiling.graph_expansion_budget.compact_graph_node_limit,
            source_scale,
        )
        .max(1),
        compact_graph_edge_limit: scale_usize(
            ceiling.graph_expansion_budget.compact_graph_edge_limit,
            source_scale,
        )
        .max(1),
        default_recall_multi_hop_allowed: ceiling
            .graph_expansion_budget
            .default_recall_multi_hop_allowed,
        eval_recall_multi_hop_allowed: ceiling.graph_expansion_budget.eval_recall_multi_hop_allowed,
    };
    let facet_recall_budget = FacetRecallRuntimeBudget {
        max_query_facets: scale_usize(ceiling.facet_recall_budget.max_query_facets, source_scale)
            .max(ceiling.p0_min_recall_items),
        max_facet_index_docs_read: scale_usize(
            ceiling.facet_recall_budget.max_facet_index_docs_read,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
        max_facet_anchor_candidates: scale_usize(
            ceiling.facet_recall_budget.max_facet_anchor_candidates,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
        max_facet_expanded_candidates: scale_usize(
            ceiling.facet_recall_budget.max_facet_expanded_candidates,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
    };
    let recall_delivery_budget = RecallDeliveryRuntimeBudget {
        max_selected_candidates: scale_usize(
            ceiling.recall_delivery_budget.max_selected_candidates,
            source_scale,
        )
        .max(ceiling.p0_min_recall_items),
        max_rendered_capsules: scale_usize(
            ceiling.recall_delivery_budget.max_rendered_capsules,
            source_scale,
        )
        .max(1),
        max_capsule_chars: scale_usize(
            ceiling.recall_delivery_budget.max_capsule_chars,
            source_scale,
        )
        .max(128),
        max_loss_ledger_entries: scale_usize(
            ceiling.recall_delivery_budget.max_loss_ledger_entries,
            source_scale,
        )
        .max(1),
    };
    let governed_state_budget = GovernedStateRuntimeBudget {
        max_validity_joins: scale_usize(
            ceiling.governed_state_budget.max_validity_joins,
            source_scale,
        )
        .max(1),
        max_lineage_depth: scale_usize(
            ceiling.governed_state_budget.max_lineage_depth,
            source_scale,
        )
        .max(1),
        max_retained_long_term_revisions_per_owner: scale_usize(
            ceiling
                .governed_state_budget
                .max_retained_long_term_revisions_per_owner,
            source_scale,
        )
        .max(1),
        max_retained_runtime_skill_owners_per_scope: scale_usize(
            ceiling
                .governed_state_budget
                .max_retained_runtime_skill_owners_per_scope,
            source_scale,
        )
        .max(1),
        max_runtime_skill_lineage_depth: scale_usize(
            ceiling
                .governed_state_budget
                .max_runtime_skill_lineage_depth,
            source_scale,
        )
        .max(1),
        max_as_of_candidates: scale_usize(
            ceiling.governed_state_budget.max_as_of_candidates,
            source_scale,
        )
        .max(1),
        max_obsolete_decisions: scale_usize(
            ceiling.governed_state_budget.max_obsolete_decisions,
            source_scale,
        )
        .max(1),
        max_procedural_candidates: scale_usize(
            ceiling.governed_state_budget.max_procedural_candidates,
            source_scale,
        )
        .max(1),
        max_premises_per_skill: scale_usize(
            ceiling.governed_state_budget.max_premises_per_skill,
            source_scale,
        )
        .max(1),
        max_premise_evidence_reads: scale_usize(
            ceiling.governed_state_budget.max_premise_evidence_reads,
            source_scale,
        )
        .max(1),
        max_state_transitions_per_write: scale_usize(
            ceiling
                .governed_state_budget
                .max_state_transitions_per_write,
            source_scale,
        )
        .max(1),
        max_subject_soul_manifest_entries: scale_usize(
            ceiling
                .governed_state_budget
                .max_subject_soul_manifest_entries,
            source_scale,
        )
        .max(8),
        max_subject_soul_revisions_per_generation: scale_usize(
            ceiling
                .governed_state_budget
                .max_subject_soul_revisions_per_generation,
            source_scale,
        )
        .max(1),
        max_subject_soul_generation_tombstones: scale_usize(
            ceiling
                .governed_state_budget
                .max_subject_soul_generation_tombstones,
            source_scale,
        )
        .max(1),
        max_subject_soul_transaction_mutations: scale_usize(
            ceiling
                .governed_state_budget
                .max_subject_soul_transaction_mutations,
            source_scale,
        )
        .max(8),
    };
    let store_budget = StoreRuntimeBudget {
        metric_source_max_items: scale_usize(
            ceiling.store_budget.metric_source_max_items,
            store_scale,
        )
        .max(1),
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
        wss_session_max_frames: scale_usize(
            ceiling.adapter_budget.wss_session_max_frames,
            source_scale,
        )
        .max(4),
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
        buffered_json_max_bytes: scale_usize(
            ceiling.llm_gateway_budget.buffered_json_max_bytes,
            source_scale,
        )
        .max(ceiling.p0_min_http_body_bytes),
        stream_chunk_max_bytes: scale_usize(
            ceiling.llm_gateway_budget.stream_chunk_max_bytes,
            source_scale,
        )
        .max(ceiling.p0_min_http_body_bytes),
        stream_event_max_bytes: scale_usize(
            ceiling.llm_gateway_budget.stream_event_max_bytes,
            source_scale,
        )
        .max(ceiling.p0_min_http_body_bytes),
        stream_max_events: scale_usize(ceiling.llm_gateway_budget.stream_max_events, source_scale)
            .max(1),
        response_body_max_bytes: scale_usize(
            ceiling.llm_gateway_budget.response_body_max_bytes,
            source_scale,
        )
        .max(ceiling.p0_min_http_body_bytes),
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
    let evidence_document_budget =
        compile_evidence_document_budget(memory_core_budget, runtime_job_budget, store_budget);
    finalize_runtime_budget_report_id(RuntimeBudgetReport {
        report_id: String::new(),
        profile: input.profile,
        deployment_target: input.profile.target(),
        deployment_role: input.profile.role(),
        store_medium: input.static_platform_manifest.store_medium,
        resource_snapshot: input.resource_snapshot,
        static_platform_manifest: input.static_platform_manifest,
        provider_model_context_limit: input.provider_model_context_limit,
        memory_core_budget,
        graph_expansion_budget,
        facet_recall_budget,
        recall_delivery_budget,
        governed_state_budget,
        evidence_document_budget,
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
    })
}

fn compile_authority_report(
    profile: ProfileId,
    static_platform_manifest: &StaticPlatformManifest,
    provider_model_context_limit: Option<&ProviderModelContextLimit>,
    compile_mode: &RuntimeBudgetCompileMode,
    resource_snapshot: RuntimeResourceSnapshot,
    admission_store_ceiling: Option<StoreRuntimeBudget>,
) -> RuntimeBudgetReport {
    let report = compile_runtime_budget(RuntimeBudgetInput {
        profile,
        resource_snapshot,
        static_platform_manifest: static_platform_manifest.clone(),
        provider_model_context_limit: provider_model_context_limit.cloned(),
    });
    let report = match compile_mode {
        RuntimeBudgetCompileMode::Production => report,
        #[cfg(feature = "nonproduction-replay-harness")]
        RuntimeBudgetCompileMode::Nonproduction(limits) => {
            apply_nonproduction_runtime_budget_limits(report, limits)
        }
    };
    match admission_store_ceiling {
        Some(ceiling) => {
            apply_store_budget_ceiling(report, ceiling, "runtime_store_admission_ceiling")
        }
        None => report,
    }
}

fn validate_authority_configuration(
    profile: ProfileId,
    manifest: &StaticPlatformManifest,
    probe_registration: &RuntimeResourceProbeRegistration,
) -> Result<()> {
    if manifest.profile != profile {
        return Err(Error::config(
            "runtime_budget_authority_config",
            "manifest_profile_mismatch",
        ));
    }
    if manifest.deployment_target != profile.target() {
        return Err(Error::config(
            "runtime_budget_authority_config",
            "manifest_deployment_target_mismatch",
        ));
    }
    if manifest.deployment_role != profile.role() {
        return Err(Error::config(
            "runtime_budget_authority_config",
            "manifest_deployment_role_mismatch",
        ));
    }
    if profile.role() == RoleFeature::DevFull && !cfg!(feature = "nonproduction-replay-harness") {
        return Err(Error::config(
            "runtime_budget_authority_config",
            "dev_full_requires_nonproduction_replay_harness",
        ));
    }

    let source = probe_registration.attested_source();
    if !target_accepts_source(profile.target(), source) {
        return Err(Error::config(
            "runtime_budget_authority_config",
            format!(
                "deployment_target_source_mismatch:target={}:source={}",
                profile.target().as_str(),
                source.as_str()
            ),
        ));
    }

    match (
        source,
        probe_registration.host_storage_observation(),
        manifest.store_medium,
    ) {
        (
            RuntimeResourceProbeSource::FirmwareManifest,
            None,
            RuntimeStoreMedium::VolatileMemory | RuntimeStoreMedium::EmbeddedFlash,
        )
        | (
            RuntimeResourceProbeSource::HostMacos
            | RuntimeResourceProbeSource::HostLinux
            | RuntimeResourceProbeSource::HostWindows
            | RuntimeResourceProbeSource::HostOther,
            Some(HostStorageObservation::VolatileMemory),
            RuntimeStoreMedium::VolatileMemory,
        )
        | (
            RuntimeResourceProbeSource::HostMacos
            | RuntimeResourceProbeSource::HostLinux
            | RuntimeResourceProbeSource::HostWindows
            | RuntimeResourceProbeSource::HostOther,
            Some(HostStorageObservation::PersistentFilesystem),
            RuntimeStoreMedium::PersistentFilesystem,
        ) => Ok(()),
        _ => Err(Error::config(
            "runtime_budget_authority_config",
            "store_medium_probe_registration_mismatch",
        )),
    }
}

fn validate_authority_snapshot(
    profile: ProfileId,
    attested_source: RuntimeResourceProbeSource,
    store_medium: RuntimeStoreMedium,
    snapshot: &RuntimeResourceSnapshot,
    now_secs: u64,
) -> Result<()> {
    if snapshot.ttl_ms == 0 {
        return Err(Error::config(
            "runtime_budget_resource_snapshot",
            "snapshot_ttl_must_be_nonzero",
        ));
    }
    if snapshot.observed_at_unix_secs > now_secs {
        return Err(Error::config(
            "runtime_budget_resource_snapshot",
            "snapshot_observed_at_is_in_the_future",
        ));
    }
    if snapshot.stale {
        return Err(Error::config(
            "runtime_budget_resource_snapshot",
            "probe_returned_stale_snapshot",
        ));
    }
    if snapshot.source != attested_source {
        return Err(Error::config(
            "runtime_budget_resource_source",
            format!(
                "snapshot_source_mismatch:expected={}:actual={}",
                attested_source.as_str(),
                snapshot.source.as_str()
            ),
        ));
    }
    if profile.target() == TargetFeature::Esp {
        validate_esp_resource_snapshot(snapshot)
    } else {
        validate_host_resource_snapshot(snapshot, store_medium)
    }
}

const fn target_accepts_source(target: TargetFeature, source: RuntimeResourceProbeSource) -> bool {
    matches!(
        (target, source),
        (
            TargetFeature::Esp,
            RuntimeResourceProbeSource::FirmwareManifest
        ) | (
            TargetFeature::LinuxDevice,
            RuntimeResourceProbeSource::HostLinux
        ) | (
            TargetFeature::DesktopMacos,
            RuntimeResourceProbeSource::HostMacos
        ) | (
            TargetFeature::DesktopLinux,
            RuntimeResourceProbeSource::HostLinux
        ) | (
            TargetFeature::DesktopWindows,
            RuntimeResourceProbeSource::HostWindows
        ) | (
            TargetFeature::ServerLinux,
            RuntimeResourceProbeSource::HostLinux
        )
    )
}

fn validate_esp_resource_snapshot(snapshot: &RuntimeResourceSnapshot) -> Result<()> {
    if snapshot.memory_total_bytes.is_some() || snapshot.memory_available_bytes.is_some() {
        return Err(resource_snapshot_error("esp_snapshot_contains_host_memory"));
    }
    if snapshot.unavailable_reason.is_some() && snapshot.available_parallelism.is_some() {
        return Err(resource_snapshot_error(
            "esp_unavailable_snapshot_contains_cpu_fact",
        ));
    }

    let heap = (
        snapshot.internal_heap_free_bytes,
        snapshot.internal_heap_minimum_free_bytes,
        snapshot.internal_heap_largest_block_bytes,
    );
    if snapshot.unavailable_reason.is_none()
        || heap.0.is_some()
        || heap.1.is_some()
        || heap.2.is_some()
    {
        let (Some(free), Some(minimum_free), Some(largest_block)) = heap else {
            return Err(resource_snapshot_error(
                "esp_internal_heap_facts_incomplete",
            ));
        };
        if minimum_free > free || largest_block > free {
            return Err(resource_snapshot_error(
                "esp_internal_heap_relationship_invalid",
            ));
        }
    }

    let psram = (
        snapshot.psram_total_bytes,
        snapshot.psram_free_bytes,
        snapshot.psram_largest_block_bytes,
    );
    if psram.0.is_some() || psram.1.is_some() || psram.2.is_some() {
        let (Some(total), Some(free), Some(largest_block)) = psram else {
            return Err(resource_snapshot_error("esp_psram_facts_incomplete"));
        };
        if free > total || largest_block > free {
            return Err(resource_snapshot_error("esp_psram_relationship_invalid"));
        }
    }

    let storage = (
        snapshot.storage_total_bytes,
        snapshot.storage_available_bytes,
    );
    if snapshot.unavailable_reason.is_none() || storage.0.is_some() || storage.1.is_some() {
        let (Some(total), Some(available)) = storage else {
            return Err(resource_snapshot_error("esp_storage_facts_incomplete"));
        };
        if available > total {
            return Err(resource_snapshot_error("esp_storage_relationship_invalid"));
        }
    }
    Ok(())
}

fn validate_host_resource_snapshot(
    snapshot: &RuntimeResourceSnapshot,
    store_medium: RuntimeStoreMedium,
) -> Result<()> {
    if snapshot.internal_heap_free_bytes.is_some()
        || snapshot.internal_heap_minimum_free_bytes.is_some()
        || snapshot.internal_heap_largest_block_bytes.is_some()
        || snapshot.psram_total_bytes.is_some()
        || snapshot.psram_free_bytes.is_some()
        || snapshot.psram_largest_block_bytes.is_some()
    {
        return Err(resource_snapshot_error(
            "host_snapshot_contains_firmware_memory",
        ));
    }
    if let (Some(total), Some(available)) =
        (snapshot.memory_total_bytes, snapshot.memory_available_bytes)
    {
        if available > total {
            return Err(resource_snapshot_error("host_memory_relationship_invalid"));
        }
    }
    let storage = (
        snapshot.storage_total_bytes,
        snapshot.storage_available_bytes,
    );
    if store_medium == RuntimeStoreMedium::PersistentFilesystem
        && (snapshot.unavailable_reason.is_none() || storage.0.is_some() || storage.1.is_some())
    {
        let (Some(total), Some(available)) = storage else {
            return Err(resource_snapshot_error("host_storage_facts_incomplete"));
        };
        if available > total {
            return Err(resource_snapshot_error("host_storage_relationship_invalid"));
        }
    }
    Ok(())
}

fn resource_snapshot_error(message: &'static str) -> Error {
    Error::config("runtime_budget_resource_snapshot", message)
}

fn default_probe_registration(
    profile: ProfileId,
    manifest: &StaticPlatformManifest,
) -> Result<RuntimeResourceProbeRegistration> {
    if profile.target() == TargetFeature::Esp {
        return Ok(RuntimeResourceProbeRegistration::firmware_unavailable());
    }
    match manifest.store_medium {
        RuntimeStoreMedium::VolatileMemory => Ok(RuntimeResourceProbeRegistration::host(
            HostRuntimeResourceProbe::for_volatile_memory(),
        )),
        RuntimeStoreMedium::PersistentFilesystem => Err(Error::config(
            "runtime_budget_authority_config",
            "persistent_filesystem_requires_path_bound_host_probe",
        )),
        RuntimeStoreMedium::EmbeddedFlash => Err(Error::config(
            "runtime_budget_authority_config",
            "host_profile_cannot_use_embedded_flash",
        )),
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
fn host_storage_observation_for_medium(
    store_medium: RuntimeStoreMedium,
) -> Result<HostStorageObservation> {
    match store_medium {
        RuntimeStoreMedium::VolatileMemory => Ok(HostStorageObservation::VolatileMemory),
        RuntimeStoreMedium::PersistentFilesystem => {
            Ok(HostStorageObservation::PersistentFilesystem)
        }
        RuntimeStoreMedium::EmbeddedFlash => Err(Error::config(
            "runtime_budget_authority_config",
            "host_profile_cannot_use_embedded_flash",
        )),
    }
}

fn apply_store_budget_ceiling(
    mut report: RuntimeBudgetReport,
    ceiling: StoreRuntimeBudget,
    limited_by: &'static str,
) -> RuntimeBudgetReport {
    let constrained = min_store_budget(report.store_budget, ceiling);
    if constrained != report.store_budget {
        report.store_budget = constrained;
        report.limited_by.push(limited_by.to_string());
        recompile_store_derived_budgets(&mut report);
        report.limited_by.sort();
        report.limited_by.dedup();
        return finalize_runtime_budget_report_id(report);
    }
    report
}

fn recompile_store_derived_budgets(report: &mut RuntimeBudgetReport) {
    report.evidence_document_budget = compile_evidence_document_budget(
        report.memory_core_budget,
        report.runtime_job_budget,
        report.store_budget,
    );
}

fn min_store_budget(left: StoreRuntimeBudget, right: StoreRuntimeBudget) -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        metric_source_max_items: left
            .metric_source_max_items
            .min(right.metric_source_max_items),
        event_log_max_items: left.event_log_max_items.min(right.event_log_max_items),
        kv_max_entries: left.kv_max_entries.min(right.kv_max_entries),
        blob_max_bytes: left.blob_max_bytes.min(right.blob_max_bytes),
        snapshot_max_bytes: left.snapshot_max_bytes.min(right.snapshot_max_bytes),
        logical_namespace_max_bytes: left
            .logical_namespace_max_bytes
            .min(right.logical_namespace_max_bytes),
        logical_key_max_bytes: left.logical_key_max_bytes.min(right.logical_key_max_bytes),
        event_record_key_max_bytes: left
            .event_record_key_max_bytes
            .min(right.event_record_key_max_bytes),
        export_max_bytes: left.export_max_bytes.min(right.export_max_bytes),
        import_max_bytes: left.import_max_bytes.min(right.import_max_bytes),
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
pub fn compile_nonproduction_runtime_budget(
    input: RuntimeBudgetInput,
    limits: NonproductionRuntimeBudgetLimits,
) -> Result<RuntimeBudgetReport> {
    limits.validate()?;
    Ok(apply_nonproduction_runtime_budget_limits(
        compile_runtime_budget(input),
        &limits,
    ))
}

#[cfg(feature = "nonproduction-replay-harness")]
fn apply_nonproduction_runtime_budget_limits(
    mut report: RuntimeBudgetReport,
    limits: &NonproductionRuntimeBudgetLimits,
) -> RuntimeBudgetReport {
    if let Some(ceiling) = limits.graph_expansion_budget {
        let constrained = min_graph_expansion_budget(report.graph_expansion_budget, ceiling);
        if constrained != report.graph_expansion_budget {
            report.graph_expansion_budget = constrained;
            report
                .limited_by
                .push("nonproduction_graph_expansion_limit".to_string());
        }
    }
    if let Some(ceiling) = limits.facet_recall_budget {
        let constrained = min_facet_recall_budget(report.facet_recall_budget, ceiling);
        if constrained != report.facet_recall_budget {
            report.facet_recall_budget = constrained;
            report
                .limited_by
                .push("nonproduction_facet_recall_limit".to_string());
        }
    }
    if let Some(ceiling) = limits.recall_delivery_budget {
        let constrained = min_recall_delivery_budget(report.recall_delivery_budget, ceiling);
        if constrained != report.recall_delivery_budget {
            report.recall_delivery_budget = constrained;
            report
                .limited_by
                .push("nonproduction_recall_delivery_limit".to_string());
        }
    }
    if let Some(ceiling) = limits.transcript_governance_budget {
        let constrained =
            min_transcript_governance_budget(report.transcript_governance_budget, ceiling);
        if constrained != report.transcript_governance_budget {
            report.transcript_governance_budget = constrained;
            report
                .limited_by
                .push("nonproduction_transcript_governance_limit".to_string());
        }
    }
    if let Some(ceiling) = limits.store_budget_limit {
        let constrained = min_store_budget(report.store_budget, ceiling);
        if constrained != report.store_budget {
            report.store_budget = constrained;
            report
                .limited_by
                .push("nonproduction_store_budget_limit".to_string());
        }
    }
    recompile_store_derived_budgets(&mut report);
    report.limited_by.sort();
    report.limited_by.dedup();
    finalize_runtime_budget_report_id(report)
}

#[cfg(feature = "nonproduction-replay-harness")]
fn min_graph_expansion_budget(
    compiled: GraphExpansionRuntimeBudget,
    ceiling: GraphExpansionRuntimeBudget,
) -> GraphExpansionRuntimeBudget {
    GraphExpansionRuntimeBudget {
        max_hops: compiled.max_hops.min(ceiling.max_hops),
        max_seed_candidates: compiled
            .max_seed_candidates
            .min(ceiling.max_seed_candidates),
        max_expanded_candidates: compiled
            .max_expanded_candidates
            .min(ceiling.max_expanded_candidates),
        max_neighbors_per_candidate: compiled
            .max_neighbors_per_candidate
            .min(ceiling.max_neighbors_per_candidate),
        max_graph_nodes_loaded: compiled
            .max_graph_nodes_loaded
            .min(ceiling.max_graph_nodes_loaded),
        max_graph_edges_loaded: compiled
            .max_graph_edges_loaded
            .min(ceiling.max_graph_edges_loaded),
        max_backlinks_loaded: compiled
            .max_backlinks_loaded
            .min(ceiling.max_backlinks_loaded),
        compact_graph_node_limit: compiled
            .compact_graph_node_limit
            .min(ceiling.compact_graph_node_limit),
        compact_graph_edge_limit: compiled
            .compact_graph_edge_limit
            .min(ceiling.compact_graph_edge_limit),
        default_recall_multi_hop_allowed: compiled.default_recall_multi_hop_allowed
            && ceiling.default_recall_multi_hop_allowed,
        eval_recall_multi_hop_allowed: compiled.eval_recall_multi_hop_allowed
            && ceiling.eval_recall_multi_hop_allowed,
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
fn min_facet_recall_budget(
    compiled: FacetRecallRuntimeBudget,
    ceiling: FacetRecallRuntimeBudget,
) -> FacetRecallRuntimeBudget {
    FacetRecallRuntimeBudget {
        max_query_facets: compiled.max_query_facets.min(ceiling.max_query_facets),
        max_facet_index_docs_read: compiled
            .max_facet_index_docs_read
            .min(ceiling.max_facet_index_docs_read),
        max_facet_anchor_candidates: compiled
            .max_facet_anchor_candidates
            .min(ceiling.max_facet_anchor_candidates),
        max_facet_expanded_candidates: compiled
            .max_facet_expanded_candidates
            .min(ceiling.max_facet_expanded_candidates),
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
fn min_recall_delivery_budget(
    compiled: RecallDeliveryRuntimeBudget,
    ceiling: RecallDeliveryRuntimeBudget,
) -> RecallDeliveryRuntimeBudget {
    RecallDeliveryRuntimeBudget {
        max_selected_candidates: compiled
            .max_selected_candidates
            .min(ceiling.max_selected_candidates),
        max_rendered_capsules: compiled
            .max_rendered_capsules
            .min(ceiling.max_rendered_capsules),
        max_capsule_chars: compiled.max_capsule_chars.min(ceiling.max_capsule_chars),
        max_loss_ledger_entries: compiled
            .max_loss_ledger_entries
            .min(ceiling.max_loss_ledger_entries),
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
fn min_transcript_governance_budget(
    compiled: TranscriptGovernanceBudget,
    ceiling: TranscriptGovernanceBudget,
) -> TranscriptGovernanceBudget {
    TranscriptGovernanceBudget {
        transcript_page_size: compiled
            .transcript_page_size
            .min(ceiling.transcript_page_size),
        host_refs_per_turn: compiled.host_refs_per_turn.min(ceiling.host_refs_per_turn),
        max_attrs_per_turn: compiled.max_attrs_per_turn.min(ceiling.max_attrs_per_turn),
        max_attrs_per_message: compiled
            .max_attrs_per_message
            .min(ceiling.max_attrs_per_message),
        redaction_items_per_page: compiled
            .redaction_items_per_page
            .min(ceiling.redaction_items_per_page),
        derived_refs_per_report: compiled
            .derived_refs_per_report
            .min(ceiling.derived_refs_per_report),
        repair_issues_per_report: compiled
            .repair_issues_per_report
            .min(ceiling.repair_issues_per_report),
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
fn validate_graph_expansion_budget(budget: GraphExpansionRuntimeBudget) -> Result<()> {
    if budget.max_hops == 0
        || budget.max_hops > 2
        || budget.max_seed_candidates == 0
        || budget.max_expanded_candidates == 0
        || budget.max_neighbors_per_candidate == 0
        || budget.max_graph_nodes_loaded == 0
        || budget.max_graph_edges_loaded == 0
        || budget.max_backlinks_loaded == 0
        || budget.compact_graph_node_limit == 0
        || budget.compact_graph_edge_limit == 0
    {
        return Err(nonproduction_limits_error(
            "graph_expansion_budget_has_zero_or_out_of_range_field",
        ));
    }
    if budget.max_hops == 1
        && (budget.default_recall_multi_hop_allowed || budget.eval_recall_multi_hop_allowed)
    {
        return Err(nonproduction_limits_error(
            "graph_expansion_multi_hop_requires_two_hops",
        ));
    }
    Ok(())
}

#[cfg(feature = "nonproduction-replay-harness")]
fn validate_facet_recall_budget(budget: FacetRecallRuntimeBudget) -> Result<()> {
    if budget.max_query_facets == 0
        || budget.max_facet_index_docs_read == 0
        || budget.max_facet_anchor_candidates == 0
        || budget.max_facet_expanded_candidates == 0
    {
        return Err(nonproduction_limits_error(
            "facet_recall_budget_has_zero_field",
        ));
    }
    Ok(())
}

#[cfg(feature = "nonproduction-replay-harness")]
fn validate_recall_delivery_budget(budget: RecallDeliveryRuntimeBudget) -> Result<()> {
    if budget.max_selected_candidates == 0
        || budget.max_rendered_capsules == 0
        || budget.max_capsule_chars == 0
        || budget.max_loss_ledger_entries == 0
    {
        return Err(nonproduction_limits_error(
            "recall_delivery_budget_has_zero_field",
        ));
    }
    if budget.max_rendered_capsules > budget.max_selected_candidates {
        return Err(nonproduction_limits_error(
            "rendered_capsules_exceed_selected_candidates",
        ));
    }
    Ok(())
}

#[cfg(feature = "nonproduction-replay-harness")]
fn validate_transcript_governance_budget(budget: TranscriptGovernanceBudget) -> Result<()> {
    if budget.transcript_page_size == 0
        || budget.host_refs_per_turn == 0
        || budget.max_attrs_per_turn == 0
        || budget.max_attrs_per_message == 0
        || budget.redaction_items_per_page == 0
        || budget.derived_refs_per_report == 0
        || budget.repair_issues_per_report == 0
    {
        return Err(nonproduction_limits_error(
            "transcript_governance_budget_has_zero_field",
        ));
    }
    if budget.max_attrs_per_message > budget.max_attrs_per_turn {
        return Err(nonproduction_limits_error(
            "message_attrs_exceed_turn_attrs",
        ));
    }
    Ok(())
}

#[cfg(feature = "nonproduction-replay-harness")]
fn validate_store_budget(budget: StoreRuntimeBudget, stage: &'static str) -> Result<()> {
    if budget.metric_source_max_items == 0
        || budget.event_log_max_items == 0
        || budget.kv_max_entries == 0
        || budget.blob_max_bytes == 0
        || budget.snapshot_max_bytes < 2
        || budget.logical_namespace_max_bytes == 0
        || budget.logical_key_max_bytes == 0
        || budget.event_record_key_max_bytes == 0
        || budget.export_max_bytes == 0
        || budget.import_max_bytes == 0
    {
        return Err(Error::config(
            stage,
            "store_budget_has_zero_or_invalid_field",
        ));
    }
    if budget.logical_key_max_bytes < budget.logical_namespace_max_bytes
        || budget.event_record_key_max_bytes < budget.logical_namespace_max_bytes
    {
        return Err(Error::config(
            stage,
            "store_key_budget_is_smaller_than_namespace_budget",
        ));
    }
    if budget.kv_max_entries < EVIDENCE_DOCUMENT_KV_ENTRY_ENVELOPE_PER_DOCUMENT {
        return Err(Error::config(
            stage,
            "store_kv_budget_cannot_hold_evidence_document_envelope",
        ));
    }
    Ok(())
}

#[cfg(feature = "nonproduction-replay-harness")]
fn nonproduction_limits_error(reason: &'static str) -> Error {
    Error::config("runtime_budget_nonproduction_limits", reason)
}

#[cfg(feature = "nonproduction-replay-harness")]
fn store_budget_is_at_least(candidate: StoreRuntimeBudget, floor: StoreRuntimeBudget) -> bool {
    candidate.metric_source_max_items >= floor.metric_source_max_items
        && candidate.event_log_max_items >= floor.event_log_max_items
        && candidate.kv_max_entries >= floor.kv_max_entries
        && candidate.blob_max_bytes >= floor.blob_max_bytes
        && candidate.snapshot_max_bytes >= floor.snapshot_max_bytes
        && candidate.logical_namespace_max_bytes >= floor.logical_namespace_max_bytes
        && candidate.logical_key_max_bytes >= floor.logical_key_max_bytes
        && candidate.event_record_key_max_bytes >= floor.event_record_key_max_bytes
        && candidate.export_max_bytes >= floor.export_max_bytes
        && candidate.import_max_bytes >= floor.import_max_bytes
}

fn compile_evidence_document_budget(
    memory_core_budget: MemoryCoreBudget,
    runtime_job_budget: RuntimeJobBudget,
    store_budget: StoreRuntimeBudget,
) -> EvidenceDocumentRuntimeBudget {
    let max_total_bytes_per_transaction = store_budget.snapshot_max_bytes / 2;
    let max_document_bytes =
        max_total_bytes_per_transaction.min(MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES);
    let max_document_body_bytes = max_document_bytes.min(MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES);
    let max_documents_per_transaction = runtime_job_budget
        .maintenance_batch_max_items
        .min(store_budget.event_log_max_items)
        .min(store_budget.kv_max_entries / EVIDENCE_DOCUMENT_KV_ENTRY_ENVELOPE_PER_DOCUMENT)
        .max(1);
    EvidenceDocumentRuntimeBudget {
        max_document_bytes,
        max_document_body_bytes,
        max_chunk_bytes: max_document_body_bytes.min(MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES),
        max_chunks_per_document: memory_core_budget
            .recall_working_set_max_items
            .min(MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS),
        max_documents_per_transaction,
        // Exact post-write closure verifies one admitted transaction through one
        // immutable snapshot. Facet expansion keeps its own independent budget.
        max_documents_per_read: max_documents_per_transaction,
        max_total_bytes_per_transaction,
    }
}

fn finalize_runtime_budget_report_id(mut report: RuntimeBudgetReport) -> RuntimeBudgetReport {
    report.report_id.clear();
    let payload = serde_json::to_vec(&report)
        .expect("RuntimeBudgetReport serialization is a compiler invariant");
    let mut hasher = Sha256::new();
    hasher.update(b"beetle-memory:runtime-budget-report:v2");
    hasher.update((payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    report.report_id = format!("rtb-v2-{:x}", hasher.finalize());
    report
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

#[derive(Clone, Copy)]
struct ProfileBudgetCeiling {
    memory_floor_bytes: u64,
    storage_floor_bytes: u64,
    memory_core_budget: MemoryCoreBudget,
    graph_expansion_budget: GraphExpansionRuntimeBudget,
    facet_recall_budget: FacetRecallRuntimeBudget,
    recall_delivery_budget: RecallDeliveryRuntimeBudget,
    governed_state_budget: GovernedStateRuntimeBudget,
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
            retained_long_term_revisions_per_owner: 4,
            retained_runtime_skill_owners_per_scope: 16,
            runtime_skill_lineage_depth: 4,
            events: 256,
            metric_source_max_items: 1,
            blob_max_bytes: 1024 * 1024,
            snapshot_max_bytes: 256 * 1024,
            http_body_max_bytes: 8 * 1024,
            llm_response_max_bytes: 64 * 1024,
            source_chars: 1024,
            render_chars: 2048,
            maintenance_chars: 1024,
            runtime_cache_max_runtimes: 8,
            wss_subscriptions: 4,
            graph_max_hops: 1,
            graph_default_recall_multi_hop_allowed: false,
            graph_eval_recall_multi_hop_allowed: false,
        }),
        ProfileId::EspStandaloneMemory => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 256 * MB,
            storage_floor_bytes: 16 * MB,
            records: 4096,
            retained_long_term_revisions_per_owner: 4,
            retained_runtime_skill_owners_per_scope: 64,
            runtime_skill_lineage_depth: 4,
            events: 2048,
            metric_source_max_items: 1,
            blob_max_bytes: 4 * 1024 * 1024,
            snapshot_max_bytes: 1024 * 1024,
            http_body_max_bytes: 16 * 1024,
            llm_response_max_bytes: 256 * 1024,
            source_chars: 2048,
            render_chars: 4096,
            maintenance_chars: 2048,
            runtime_cache_max_runtimes: 16,
            wss_subscriptions: 8,
            graph_max_hops: 1,
            graph_default_recall_multi_hop_allowed: false,
            graph_eval_recall_multi_hop_allowed: false,
        }),
        ProfileId::LinuxDeviceStandaloneMemory => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 512 * MB,
            storage_floor_bytes: 256 * MB,
            records: 12_000,
            retained_long_term_revisions_per_owner: 8,
            retained_runtime_skill_owners_per_scope: 256,
            runtime_skill_lineage_depth: 8,
            events: 4096,
            metric_source_max_items: 2,
            blob_max_bytes: 16 * 1024 * 1024,
            snapshot_max_bytes: 4 * 1024 * 1024,
            http_body_max_bytes: 64 * 1024,
            llm_response_max_bytes: 1024 * 1024,
            source_chars: 4096,
            render_chars: 8192,
            maintenance_chars: 4096,
            runtime_cache_max_runtimes: 32,
            wss_subscriptions: 16,
            graph_max_hops: 1,
            graph_default_recall_multi_hop_allowed: false,
            graph_eval_recall_multi_hop_allowed: false,
        }),
        ProfileId::DesktopMacosEmbeddedSdk
        | ProfileId::DesktopLinuxEmbeddedSdk
        | ProfileId::DesktopWindowsEmbeddedSdk => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 512 * MB,
            storage_floor_bytes: 256 * MB,
            records: 4096,
            retained_long_term_revisions_per_owner: 8,
            retained_runtime_skill_owners_per_scope: 128,
            runtime_skill_lineage_depth: 8,
            events: 8192,
            metric_source_max_items: 2,
            blob_max_bytes: 8 * 1024 * 1024,
            snapshot_max_bytes: 2 * 1024 * 1024,
            http_body_max_bytes: 32 * 1024,
            llm_response_max_bytes: 512 * 1024,
            source_chars: 2048,
            render_chars: 4096,
            maintenance_chars: 2048,
            runtime_cache_max_runtimes: 16,
            wss_subscriptions: 8,
            graph_max_hops: 1,
            graph_default_recall_multi_hop_allowed: false,
            graph_eval_recall_multi_hop_allowed: false,
        }),
        ProfileId::DesktopMacosStandaloneMemory => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 1024 * MB,
            storage_floor_bytes: 512 * MB,
            records: 20_000,
            retained_long_term_revisions_per_owner: 16,
            retained_runtime_skill_owners_per_scope: 512,
            runtime_skill_lineage_depth: 16,
            events: 8192,
            metric_source_max_items: 2,
            blob_max_bytes: 32 * 1024 * 1024,
            snapshot_max_bytes: 8 * 1024 * 1024,
            http_body_max_bytes: 96 * 1024,
            llm_response_max_bytes: 2 * 1024 * 1024,
            source_chars: 8192,
            render_chars: 12_288,
            maintenance_chars: 8192,
            runtime_cache_max_runtimes: 64,
            wss_subscriptions: 32,
            graph_max_hops: 1,
            graph_default_recall_multi_hop_allowed: false,
            graph_eval_recall_multi_hop_allowed: false,
        }),
        ProfileId::ServerLinuxMemoryGateway => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 1024 * MB,
            storage_floor_bytes: 1024 * MB,
            records: 40_000,
            retained_long_term_revisions_per_owner: 32,
            retained_runtime_skill_owners_per_scope: 1024,
            runtime_skill_lineage_depth: 32,
            events: 16_384,
            metric_source_max_items: 2,
            blob_max_bytes: 64 * 1024 * 1024,
            snapshot_max_bytes: 16 * 1024 * 1024,
            http_body_max_bytes: 128 * 1024,
            llm_response_max_bytes: 8 * 1024 * 1024,
            source_chars: 8192,
            render_chars: 16_384,
            maintenance_chars: 8192,
            runtime_cache_max_runtimes: 128,
            wss_subscriptions: 64,
            graph_max_hops: 2,
            graph_default_recall_multi_hop_allowed: true,
            graph_eval_recall_multi_hop_allowed: true,
        }),
        ProfileId::DesktopMacosDevFull
        | ProfileId::DesktopWindowsDevFull
        | ProfileId::ServerLinuxDevFull => profile_budget(ProfileBudgetSpec {
            memory_floor_bytes: 2048 * MB,
            storage_floor_bytes: 2048 * MB,
            records: 80_000,
            retained_long_term_revisions_per_owner: 32,
            retained_runtime_skill_owners_per_scope: 2048,
            runtime_skill_lineage_depth: 32,
            events: 32_768,
            metric_source_max_items: 2,
            blob_max_bytes: 128 * 1024 * 1024,
            snapshot_max_bytes: 32 * 1024 * 1024,
            http_body_max_bytes: 256 * 1024,
            llm_response_max_bytes: 16 * 1024 * 1024,
            source_chars: 16_384,
            render_chars: 32_768,
            maintenance_chars: 16_384,
            runtime_cache_max_runtimes: 256,
            wss_subscriptions: 128,
            graph_max_hops: 2,
            graph_default_recall_multi_hop_allowed: true,
            graph_eval_recall_multi_hop_allowed: true,
        }),
    }
}

const MB: u64 = 1024 * 1024;

struct ProfileBudgetSpec {
    memory_floor_bytes: u64,
    storage_floor_bytes: u64,
    records: usize,
    retained_long_term_revisions_per_owner: usize,
    retained_runtime_skill_owners_per_scope: usize,
    runtime_skill_lineage_depth: usize,
    events: usize,
    metric_source_max_items: usize,
    blob_max_bytes: usize,
    snapshot_max_bytes: usize,
    http_body_max_bytes: usize,
    llm_response_max_bytes: usize,
    source_chars: usize,
    render_chars: usize,
    maintenance_chars: usize,
    runtime_cache_max_runtimes: usize,
    wss_subscriptions: usize,
    graph_max_hops: u8,
    graph_default_recall_multi_hop_allowed: bool,
    graph_eval_recall_multi_hop_allowed: bool,
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
        graph_expansion_budget: GraphExpansionRuntimeBudget {
            max_hops: spec.graph_max_hops,
            max_seed_candidates: max_usize(spec.records / 64, 4),
            max_expanded_candidates: max_usize(spec.records / 128, 8),
            max_neighbors_per_candidate: max_usize(spec.records / 512, 2),
            max_graph_nodes_loaded: max_usize(spec.records / 16, 16),
            max_graph_edges_loaded: max_usize(spec.records / 8, 32),
            max_backlinks_loaded: max_usize(spec.records / 8, 32),
            compact_graph_node_limit: if spec.records >= 4096 { 32 } else { 16 },
            compact_graph_edge_limit: if spec.records >= 4096 { 32 } else { 16 },
            default_recall_multi_hop_allowed: spec.graph_default_recall_multi_hop_allowed,
            eval_recall_multi_hop_allowed: spec.graph_eval_recall_multi_hop_allowed,
        },
        facet_recall_budget: FacetRecallRuntimeBudget {
            max_query_facets: max_usize(spec.source_chars / 256, 4),
            max_facet_index_docs_read: max_usize(spec.records / 8, 32),
            max_facet_anchor_candidates: max_usize(spec.records / 128, 8),
            max_facet_expanded_candidates: max_usize(spec.records / 256, 8),
        },
        recall_delivery_budget: RecallDeliveryRuntimeBudget {
            max_selected_candidates: max_usize(spec.source_chars / 128, 8),
            max_rendered_capsules: max_usize(spec.source_chars / 512, 2),
            max_capsule_chars: max_usize(spec.render_chars / 3, 256),
            max_loss_ledger_entries: max_usize(spec.source_chars / 256, 4),
        },
        governed_state_budget: GovernedStateRuntimeBudget {
            max_validity_joins: max_usize(spec.records / 256, 2),
            max_lineage_depth: if spec.records >= 4096 { 16 } else { 4 },
            max_retained_long_term_revisions_per_owner: spec.retained_long_term_revisions_per_owner,
            max_retained_runtime_skill_owners_per_scope: spec
                .retained_runtime_skill_owners_per_scope,
            max_runtime_skill_lineage_depth: spec.runtime_skill_lineage_depth,
            max_as_of_candidates: max_usize(spec.records / 512, 2),
            max_obsolete_decisions: max_usize(spec.records / 256, 2),
            max_procedural_candidates: max_usize(spec.source_chars / 256, 2),
            max_premises_per_skill: max_usize(spec.source_chars / 1024, 2),
            max_premise_evidence_reads: max_usize(spec.source_chars / 512, 2),
            max_state_transitions_per_write: if spec.records >= 4096 { 8 } else { 2 },
            max_subject_soul_manifest_entries: max_usize(spec.records / 16, 16),
            max_subject_soul_revisions_per_generation: spec.retained_long_term_revisions_per_owner,
            max_subject_soul_generation_tombstones: if spec.records >= 4096 { 8 } else { 2 },
            max_subject_soul_transaction_mutations: max_usize(spec.records / 128, 16),
        },
        store_budget: StoreRuntimeBudget {
            metric_source_max_items: spec.metric_source_max_items,
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
            wss_session_max_frames: max_usize(spec.wss_subscriptions.saturating_mul(8), 8),
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
            buffered_json_max_bytes: spec.llm_response_max_bytes,
            stream_chunk_max_bytes: spec.http_body_max_bytes,
            stream_event_max_bytes: spec.http_body_max_bytes,
            stream_max_events: spec.events,
            response_body_max_bytes: spec.llm_response_max_bytes,
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
    use crate::resource::{RuntimeResourceObservation, RuntimeResourceUnavailableReason};

    fn compiler_fixture(profile: ProfileId) -> RuntimeBudgetInput {
        let store_medium = if profile.target() == TargetFeature::Esp {
            RuntimeStoreMedium::EmbeddedFlash
        } else {
            RuntimeStoreMedium::VolatileMemory
        };
        RuntimeBudgetInput {
            profile,
            resource_snapshot: RuntimeResourceSnapshot::unavailable(
                10,
                RuntimeResourceProbeSource::StaticManifest,
                RuntimeResourceUnavailableReason::ProbeNotConfigured,
            ),
            static_platform_manifest: StaticPlatformManifest::for_profile(profile, store_medium),
            provider_model_context_limit: None,
        }
    }

    fn full_resource_compiler_fixture(profile: ProfileId) -> RuntimeBudgetInput {
        let store_medium = if profile.target() == TargetFeature::Esp {
            RuntimeStoreMedium::EmbeddedFlash
        } else {
            RuntimeStoreMedium::VolatileMemory
        };
        let manifest = StaticPlatformManifest::for_profile(profile, store_medium);
        let observation = RuntimeResourceObservation {
            observed_at_unix_secs: 10,
            ttl_ms: 30_000,
            stale: false,
            pressure: PressureLevel::Normal,
            available_parallelism: Some(16),
            memory_total_bytes: Some(manifest.memory_floor_bytes.saturating_mul(4)),
            memory_available_bytes: Some(manifest.memory_floor_bytes.saturating_mul(3)),
            internal_heap_free_bytes: None,
            internal_heap_minimum_free_bytes: None,
            internal_heap_largest_block_bytes: None,
            psram_total_bytes: None,
            psram_free_bytes: None,
            psram_largest_block_bytes: None,
            storage_total_bytes: Some(manifest.storage_floor_bytes.saturating_mul(4)),
            storage_available_bytes: Some(manifest.storage_floor_bytes.saturating_mul(3)),
            unavailable_reason: None,
            unavailable_detail: None,
        };
        RuntimeBudgetInput {
            profile,
            resource_snapshot: RuntimeResourceSnapshot::from_observation(
                RuntimeResourceProbeSource::StaticManifest,
                observation,
            ),
            static_platform_manifest: manifest,
            provider_model_context_limit: None,
        }
    }

    #[test]
    fn provider_limit_only_caps_render_budget() {
        let mut input = compiler_fixture(ProfileId::ServerLinuxMemoryGateway);
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
    fn report_identity_covers_compiler_metadata_and_complete_payload() {
        let report = compile_runtime_budget(compiler_fixture(ProfileId::ServerLinuxDevFull));
        let mut changed = report.clone();
        changed.report_id = "forged".to_string();
        changed
            .unavailable_reasons
            .push("synthetic_unavailable_reason".to_string());
        let changed = finalize_runtime_budget_report_id(changed);

        assert!(report.report_id.starts_with("rtb-v2-"));
        assert!(changed.report_id.starts_with("rtb-v2-"));
        assert_ne!(report.report_id, changed.report_id);
    }

    #[test]
    fn admission_rejects_stale_expired_and_payload_tampered_reports() {
        let report = compile_runtime_budget(compiler_fixture(ProfileId::ServerLinuxDevFull));
        assert!(report.validate_for_admission(10).is_ok());

        let mut stale = report.clone();
        stale.resource_snapshot = stale.resource_snapshot.mark_stale();
        let stale = finalize_runtime_budget_report_id(stale);
        assert_eq!(
            stale
                .validate_for_admission(10)
                .expect_err("stale report must not admit work")
                .stage(),
            "runtime_budget_admission"
        );

        assert_eq!(
            report
                .validate_for_admission(40)
                .expect_err("expired report must not admit work")
                .stage(),
            "runtime_budget_admission"
        );

        let mut tampered = report;
        tampered.store_budget.kv_max_entries =
            tampered.store_budget.kv_max_entries.saturating_add(1);
        assert!(tampered
            .validate_for_admission(10)
            .expect_err("report identity must cover admission capacity")
            .to_string()
            .contains("identity does not cover its payload"));
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn nonproduction_store_ceiling_clamps_each_field_and_recompiles_derivatives() {
        let input = compiler_fixture(ProfileId::ServerLinuxDevFull);
        let compiled = compile_runtime_budget(input.clone()).store_budget;
        let mut requested = compiled;
        requested.event_log_max_items = 2;
        requested.logical_namespace_max_bytes = compiled.logical_namespace_max_bytes + 32;
        let limits = NonproductionRuntimeBudgetLimits::new()
            .try_with_store_budget_limit(requested)
            .unwrap();

        let report = compile_nonproduction_runtime_budget(input, limits).unwrap();

        assert_eq!(report.store_budget.event_log_max_items, 2);
        assert_eq!(
            report.store_budget.logical_namespace_max_bytes,
            compiled.logical_namespace_max_bytes
        );
        assert_eq!(report.store_budget.kv_max_entries, compiled.kv_max_entries);
        assert_eq!(
            report
                .evidence_document_budget
                .max_documents_per_transaction,
            2
        );
    }

    #[test]
    fn eleven_profiles_have_distinct_budget_reports() {
        let profiles = [
            ProfileId::EspStandaloneMemory,
            ProfileId::EspEmbeddedSdk,
            ProfileId::LinuxDeviceStandaloneMemory,
            ProfileId::DesktopMacosStandaloneMemory,
            ProfileId::DesktopMacosEmbeddedSdk,
            ProfileId::DesktopMacosDevFull,
            ProfileId::DesktopLinuxEmbeddedSdk,
            ProfileId::DesktopWindowsEmbeddedSdk,
            ProfileId::DesktopWindowsDevFull,
            ProfileId::ServerLinuxMemoryGateway,
            ProfileId::ServerLinuxDevFull,
        ];
        let mut render_budgets = Vec::new();
        for profile in profiles {
            render_budgets.push(
                compile_runtime_budget(compiler_fixture(profile))
                    .projection_render_budget
                    .system_block_max_chars,
            );
        }
        render_budgets.sort_unstable();
        render_budgets.dedup();
        assert!(render_budgets.len() >= 6);
    }

    #[test]
    fn desktop_embedded_sdks_share_an_exact_8192_event_ceiling() {
        for profile in [
            ProfileId::DesktopMacosEmbeddedSdk,
            ProfileId::DesktopLinuxEmbeddedSdk,
            ProfileId::DesktopWindowsEmbeddedSdk,
        ] {
            let report = compile_runtime_budget(full_resource_compiler_fixture(profile));
            assert_eq!(report.store_budget.event_log_max_items, 8192, "{profile:?}");
        }

        let esp = compile_runtime_budget(full_resource_compiler_fixture(ProfileId::EspEmbeddedSdk));
        assert_eq!(esp.store_budget.event_log_max_items, 256);
    }

    #[test]
    fn metric_source_count_is_explicitly_owned_by_each_profile_budget() {
        for (profile, expected) in [
            (ProfileId::EspEmbeddedSdk, 1),
            (ProfileId::EspStandaloneMemory, 1),
            (ProfileId::LinuxDeviceStandaloneMemory, 2),
            (ProfileId::DesktopMacosEmbeddedSdk, 2),
            (ProfileId::DesktopLinuxEmbeddedSdk, 2),
            (ProfileId::DesktopWindowsEmbeddedSdk, 2),
            (ProfileId::DesktopMacosStandaloneMemory, 2),
            (ProfileId::ServerLinuxMemoryGateway, 2),
            (ProfileId::DesktopMacosDevFull, 2),
            (ProfileId::DesktopWindowsDevFull, 2),
            (ProfileId::ServerLinuxDevFull, 2),
        ] {
            assert_eq!(
                compile_runtime_budget(full_resource_compiler_fixture(profile))
                    .store_budget
                    .metric_source_max_items,
                expected,
                "{profile:?}"
            );
        }
    }

    #[test]
    fn retained_long_term_revision_cap_is_explicitly_profile_owned() {
        for (profile, expected) in [
            (ProfileId::EspStandaloneMemory, 4),
            (ProfileId::EspEmbeddedSdk, 4),
            (ProfileId::LinuxDeviceStandaloneMemory, 8),
            (ProfileId::DesktopMacosStandaloneMemory, 16),
            (ProfileId::DesktopMacosEmbeddedSdk, 8),
            (ProfileId::DesktopLinuxEmbeddedSdk, 8),
            (ProfileId::DesktopMacosDevFull, 32),
            (ProfileId::DesktopWindowsEmbeddedSdk, 8),
            (ProfileId::DesktopWindowsDevFull, 32),
            (ProfileId::ServerLinuxMemoryGateway, 32),
            (ProfileId::ServerLinuxDevFull, 32),
        ] {
            let ceiling = profile_budget_ceiling(profile)
                .governed_state_budget
                .max_retained_long_term_revisions_per_owner;
            assert_eq!(ceiling, expected, "{profile:?} retention ceiling drifted");
            let compiled = compile_runtime_budget(compiler_fixture(profile))
                .governed_state_budget
                .max_retained_long_term_revisions_per_owner;
            assert!(compiled > 0);
            assert!(
                compiled <= expected,
                "{profile:?} compiled retention cap exceeded its profile ceiling"
            );
        }
    }

    #[test]
    fn runtime_skill_scope_and_lineage_caps_are_explicitly_profile_owned() {
        for (profile, expected_owners, expected_lineage) in [
            (ProfileId::EspStandaloneMemory, 64, 4),
            (ProfileId::EspEmbeddedSdk, 16, 4),
            (ProfileId::LinuxDeviceStandaloneMemory, 256, 8),
            (ProfileId::DesktopMacosStandaloneMemory, 512, 16),
            (ProfileId::DesktopMacosEmbeddedSdk, 128, 8),
            (ProfileId::DesktopLinuxEmbeddedSdk, 128, 8),
            (ProfileId::DesktopMacosDevFull, 2048, 32),
            (ProfileId::DesktopWindowsEmbeddedSdk, 128, 8),
            (ProfileId::DesktopWindowsDevFull, 2048, 32),
            (ProfileId::ServerLinuxMemoryGateway, 1024, 32),
            (ProfileId::ServerLinuxDevFull, 2048, 32),
        ] {
            let ceiling = profile_budget_ceiling(profile).governed_state_budget;
            assert_eq!(
                ceiling.max_retained_runtime_skill_owners_per_scope, expected_owners,
                "{profile:?} runtime skill owner ceiling drifted"
            );
            assert_eq!(
                ceiling.max_runtime_skill_lineage_depth, expected_lineage,
                "{profile:?} runtime skill lineage ceiling drifted"
            );
            let compiled = compile_runtime_budget(compiler_fixture(profile)).governed_state_budget;
            assert!(compiled.max_retained_runtime_skill_owners_per_scope > 0);
            assert!(
                compiled.max_retained_runtime_skill_owners_per_scope <= expected_owners,
                "{profile:?} compiled runtime skill owner cap exceeded its profile ceiling"
            );
            assert!(compiled.max_runtime_skill_lineage_depth > 0);
            assert!(
                compiled.max_runtime_skill_lineage_depth <= expected_lineage,
                "{profile:?} compiled runtime skill lineage cap exceeded its profile ceiling"
            );
        }
    }

    #[test]
    fn subject_soul_lifecycle_caps_are_positive_and_profile_owned() {
        for profile in [
            ProfileId::EspStandaloneMemory,
            ProfileId::EspEmbeddedSdk,
            ProfileId::LinuxDeviceStandaloneMemory,
            ProfileId::DesktopMacosStandaloneMemory,
            ProfileId::DesktopMacosEmbeddedSdk,
            ProfileId::DesktopLinuxEmbeddedSdk,
            ProfileId::DesktopMacosDevFull,
            ProfileId::DesktopWindowsEmbeddedSdk,
            ProfileId::DesktopWindowsDevFull,
            ProfileId::ServerLinuxMemoryGateway,
            ProfileId::ServerLinuxDevFull,
        ] {
            let ceiling = profile_budget_ceiling(profile).governed_state_budget;
            let compiled = compile_runtime_budget(compiler_fixture(profile)).governed_state_budget;
            for (name, value, maximum) in [
                (
                    "manifest_entries",
                    compiled.max_subject_soul_manifest_entries,
                    ceiling.max_subject_soul_manifest_entries,
                ),
                (
                    "revisions_per_generation",
                    compiled.max_subject_soul_revisions_per_generation,
                    ceiling.max_subject_soul_revisions_per_generation,
                ),
                (
                    "generation_tombstones",
                    compiled.max_subject_soul_generation_tombstones,
                    ceiling.max_subject_soul_generation_tombstones,
                ),
                (
                    "transaction_mutations",
                    compiled.max_subject_soul_transaction_mutations,
                    ceiling.max_subject_soul_transaction_mutations,
                ),
            ] {
                assert!(value > 0, "{profile:?} {name} must be positive");
                assert!(
                    value <= maximum,
                    "{profile:?} {name} exceeded its profile ceiling"
                );
            }
        }
    }

    #[test]
    fn governed_state_full_resource_budget_matrix_is_exact_for_all_profiles() {
        for (profile, expected) in [
            (
                ProfileId::EspStandaloneMemory,
                (16, 16, 4, 64, 4, 8, 16, 8, 2, 4, 8),
            ),
            (
                ProfileId::EspEmbeddedSdk,
                (2, 4, 4, 16, 4, 2, 2, 4, 2, 2, 2),
            ),
            (
                ProfileId::LinuxDeviceStandaloneMemory,
                (46, 16, 8, 256, 8, 23, 46, 16, 4, 8, 8),
            ),
            (
                ProfileId::DesktopMacosStandaloneMemory,
                (78, 16, 16, 512, 16, 39, 78, 32, 8, 16, 8),
            ),
            (
                ProfileId::DesktopMacosEmbeddedSdk,
                (16, 16, 8, 128, 8, 8, 16, 8, 2, 4, 8),
            ),
            (
                ProfileId::DesktopMacosDevFull,
                (312, 16, 32, 2048, 32, 156, 312, 64, 16, 32, 8),
            ),
            (
                ProfileId::DesktopLinuxEmbeddedSdk,
                (16, 16, 8, 128, 8, 8, 16, 8, 2, 4, 8),
            ),
            (
                ProfileId::DesktopWindowsEmbeddedSdk,
                (16, 16, 8, 128, 8, 8, 16, 8, 2, 4, 8),
            ),
            (
                ProfileId::DesktopWindowsDevFull,
                (312, 16, 32, 2048, 32, 156, 312, 64, 16, 32, 8),
            ),
            (
                ProfileId::ServerLinuxMemoryGateway,
                (156, 16, 32, 1024, 32, 78, 156, 32, 8, 16, 8),
            ),
            (
                ProfileId::ServerLinuxDevFull,
                (312, 16, 32, 2048, 32, 156, 312, 64, 16, 32, 8),
            ),
        ] {
            let budget = compile_runtime_budget(full_resource_compiler_fixture(profile))
                .governed_state_budget;
            let actual = (
                budget.max_validity_joins,
                budget.max_lineage_depth,
                budget.max_retained_long_term_revisions_per_owner,
                budget.max_retained_runtime_skill_owners_per_scope,
                budget.max_runtime_skill_lineage_depth,
                budget.max_as_of_candidates,
                budget.max_obsolete_decisions,
                budget.max_procedural_candidates,
                budget.max_premises_per_skill,
                budget.max_premise_evidence_reads,
                budget.max_state_transitions_per_write,
            );
            assert_eq!(actual, expected, "{profile:?} governed budget drifted");
        }
    }

    #[test]
    fn evidence_document_exact_read_closes_one_admitted_transaction() {
        for profile in [
            ProfileId::EspStandaloneMemory,
            ProfileId::EspEmbeddedSdk,
            ProfileId::LinuxDeviceStandaloneMemory,
            ProfileId::DesktopMacosStandaloneMemory,
            ProfileId::DesktopMacosEmbeddedSdk,
            ProfileId::DesktopMacosDevFull,
            ProfileId::DesktopLinuxEmbeddedSdk,
            ProfileId::DesktopWindowsEmbeddedSdk,
            ProfileId::DesktopWindowsDevFull,
            ProfileId::ServerLinuxMemoryGateway,
            ProfileId::ServerLinuxDevFull,
        ] {
            let budget = compile_runtime_budget(compiler_fixture(profile)).evidence_document_budget;
            assert!(budget.max_documents_per_transaction > 0);
            assert_eq!(
                budget.max_documents_per_read, budget.max_documents_per_transaction,
                "{profile:?} cannot close one admitted evidence transaction in one snapshot"
            );
        }
    }

    #[test]
    fn transcript_governance_budget_is_profile_specific() {
        let compact = compile_runtime_budget(compiler_fixture(ProfileId::EspEmbeddedSdk))
            .transcript_governance_budget;
        let server = compile_runtime_budget(compiler_fixture(ProfileId::ServerLinuxDevFull))
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
