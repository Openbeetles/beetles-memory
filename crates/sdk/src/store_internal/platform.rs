use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::agent::{ActiveWorkRecord, ActiveWorkStore};
#[cfg(feature = "nonproduction-replay-harness")]
use bm_core::budget::{BenchmarkStoreCapacityExtension, StoreRuntimeBudget};
use bm_core::budget::{RuntimeBudgetAuthority, RuntimeBudgetReport};
use bm_core::memory::*;
use bm_core::platform::{MemorySystemKind, Platform, SkillMetaStore, SkillStorage, StateFs};
use bm_core::resource::RuntimeResourceProbe;
use bm_core::runtime::{
    RuntimeLifecycleEffect, RuntimeLifecycleEvent, RuntimeLifecycleEventKind,
    RuntimeLifecycleEventSink, RuntimeLifecycleOperation, RuntimeLifecycleTrigger,
};
use bm_core::skills::runtime_skill_owner_updated_at;
use bm_core::task::{normalize_task_item, TaskItem, TaskQuery, TaskStore};
use bm_core::task_execution::{
    TaskArtifactRecord, TaskArtifactStore, TaskLearningRecord, TaskLearningStore, TaskRunRecord,
    TaskRunStore,
};
use bm_core::{Error, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::store_internal::config::{open_runtime_budget_authority, resolve_store_capacity};
use crate::store_internal::recall_index::{
    decode_typed_recall_index, next_entry_revision, remove_recall_index_address,
    replace_recall_index_address, ActiveTaskRunByChatIndex, ArchiveRecallManifest,
    ContinuityCapsuleScopeIndex, ConversationRecallManifest, RecallIndexAddress,
    RecallIndexAddressKind, RuntimeSkillRecallManifest, TaskLearningByChatIndex, TypedRecallIndex,
    ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE, ARCHIVE_RECALL_MANIFEST_NAMESPACE,
    CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE, CONVERSATION_RECALL_MANIFEST_NAMESPACE,
    RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE, TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
};
use crate::store_internal::recall_read::RecallImmutableReadContext;
use crate::store_internal::schema::{
    control_plane_scope_manifest_key, governed_evidence_source_claim_manifest_key,
    recall_owner_scope_binding_key, ControlPlaneScopeEntry, ControlPlaneScopeManifest,
    GovernedEvidenceSourceClaimManifest, RecallOwnerScopeBinding,
    CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE, GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
    RECALL_OWNER_SCOPE_BINDING_NAMESPACE,
};
#[cfg(feature = "sqlite-store")]
use crate::store_internal::sqlite::SqliteStoreEngine;
use crate::store_internal::transaction::{
    read_governed_evidence_exact_in_session, BackendTransactionState,
    ConditionalDeleteEventTemplate, GraphRepairAuthority, StoreAdmissionAuthority,
    StoreGovernedEvidenceExactReadRequest, StoreGovernedEvidenceExactReadResult,
};
use crate::{
    enforce_event_key_budget, enforce_logical_key_budget, store_budget_error,
    store_internal::embedded::EmbeddedStoreEngine, store_internal::file::FileStoreEngine,
    InMemoryStoreEngine, MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig,
    StoreBackendKind, StoreCapacityBudget, StoreEngine, StoreEngineMutation, StoreEventLog,
    StoreEventScope, StoreJsonPrecondition, StoreMutation, StoreMutationBatch,
    StoreMutationBatchReport, StoreOpenReport, StoreReadReceipt, StoreRepairReport,
    StoreSchemaManifest, StoreScopedProjectionReplaceRequest, StoreScopedProjectionRequest,
    StoreScopedProjectionScope, StoreSnapshot, StoreSnapshotImportReport, StoreSnapshotJsonDoc,
    StoreTransactionAdmission, StoreTransactionRequest, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreSnapshotBlob, StoreSnapshotExportReport};

static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

type RecallIndexMutationPlan = (
    &'static str,
    String,
    serde_json::Value,
    Option<serde_json::Value>,
);

pub(crate) const GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE: &str = "governed_evidence_documents";
pub(crate) const GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE: &str = "governed_evidence_source_refs";

#[derive(Clone)]
pub struct StorePlatform {
    config: StoreBackendConfig,
    capacity: StoreCapacityBudget,
    engine: Arc<dyn StoreEngine>,
    transaction_mutex: Arc<Mutex<()>>,
    schema_manifest: StoreSchemaManifest,
    open_report: StoreOpenReport,
    runtime_budget_authority: Arc<RuntimeBudgetAuthority>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct StoreMemorySpaceProjectionReport {
    pub omitted_private_entries: usize,
}

pub(crate) struct StorePlatformPreparation {
    config: StoreBackendConfig,
    runtime_budget_authority: Arc<RuntimeBudgetAuthority>,
    runtime_budget: RuntimeBudgetReport,
    report_id: String,
    probe_source: bm_core::resource::RuntimeResourceProbeSource,
    store_medium: bm_core::budget::RuntimeStoreMedium,
    consumption: Arc<AtomicU8>,
}

impl StorePlatformPreparation {
    fn prepare(
        config: StoreBackendConfig,
        firmware_probe: Option<Arc<dyn RuntimeResourceProbe>>,
    ) -> Result<Self> {
        Self::prepare_at(config, firmware_probe, current_unix_secs())
    }

    fn prepare_at(
        config: StoreBackendConfig,
        firmware_probe: Option<Arc<dyn RuntimeResourceProbe>>,
        now_secs: u64,
    ) -> Result<Self> {
        let runtime_budget_authority = Arc::new(open_runtime_budget_authority(
            &config,
            firmware_probe,
            now_secs,
        )?);
        let runtime_budget = runtime_budget_authority.current_report(now_secs);
        if runtime_budget.resource_snapshot.stale
            || runtime_budget.resource_snapshot.is_expired(now_secs)
        {
            return Err(Error::config(
                "store_prepare_admission",
                "prepared store requires a fresh runtime budget report",
            ));
        }
        Ok(Self {
            config,
            runtime_budget_authority,
            report_id: runtime_budget.report_id.clone(),
            probe_source: runtime_budget.resource_snapshot.source,
            store_medium: runtime_budget.store_medium,
            runtime_budget,
            consumption: Arc::new(AtomicU8::new(0)),
        })
    }

    fn consume(&self) -> Result<()> {
        self.consumption
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| {
                Error::config(
                    "store_prepared_admission_consumed",
                    "prepared store admission is a one-shot capability and was already consumed",
                )
            })
    }

    #[cfg(test)]
    fn duplicate_for_consumption_contract_test(&self) -> Self {
        Self {
            config: self.config.clone(),
            runtime_budget_authority: Arc::clone(&self.runtime_budget_authority),
            runtime_budget: self.runtime_budget.clone(),
            report_id: self.report_id.clone(),
            probe_source: self.probe_source,
            store_medium: self.store_medium,
            consumption: Arc::clone(&self.consumption),
        }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn runtime_budget(&self) -> &RuntimeBudgetReport {
        &self.runtime_budget
    }

    fn open(self) -> Result<(StorePlatform, StoreOpenReport)> {
        let capacity = resolve_store_capacity(&self.runtime_budget_authority)?;
        StorePlatform::open_prepared_at(self, capacity, current_unix_secs())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn open_with_benchmark_store_capacity(
        self,
        capacity: StoreRuntimeBudget,
    ) -> Result<StorePlatform> {
        let extension = BenchmarkStoreCapacityExtension::try_new(&self.runtime_budget, capacity)?;
        StorePlatform::open_prepared_at(
            self,
            StoreCapacityBudget::from_runtime_budget(extension.capacity()),
            current_unix_secs(),
        )
        .map(|(platform, _report)| platform)
    }
}

#[derive(Clone)]
pub(crate) struct ScopedLongTermMemoryStore {
    platform: StorePlatform,
    memory_space_id: String,
    key_prefix: String,
}

#[derive(Clone)]
pub(crate) struct ScopedLongTermMemoryControlReadStore {
    platform: StorePlatform,
    memory_space_id: String,
    mounted_subject_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct SessionSummaryRecord {
    summary: String,
    message_count: usize,
}

fn push_continuity_snapshot_json_write<T: Serialize>(
    mutations: &mut Vec<StoreMutation>,
    preconditions: &mut Vec<StoreJsonPrecondition>,
    namespace: &str,
    key: &str,
    observed: Option<&T>,
    next: &T,
) -> Result<()> {
    let observed = observed
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| Error::config("continuity_snapshot_import_plan", error.to_string()))?;
    preconditions.push(match observed {
        Some(value) => StoreJsonPrecondition::Exact {
            namespace: namespace.to_string(),
            key: key.to_string(),
            value,
        },
        None => StoreJsonPrecondition::Absent {
            namespace: namespace.to_string(),
            key: key.to_string(),
        },
    });
    mutations.push(StoreMutation::PutJson {
        namespace: namespace.to_string(),
        key: key.to_string(),
        value: serde_json::to_value(next)
            .map_err(|error| Error::config("continuity_snapshot_import_plan", error.to_string()))?,
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: namespace.to_string(),
        record_key: key.to_string(),
    });
    Ok(())
}

impl StorePlatform {
    pub fn open(config: StoreBackendConfig) -> Result<Self> {
        StorePlatformPreparation::prepare(config, None)?
            .open()
            .map(|(platform, _report)| platform)
    }

    pub fn open_with_firmware_resource_probe(
        config: StoreBackendConfig,
        probe: Arc<dyn RuntimeResourceProbe>,
    ) -> Result<Self> {
        StorePlatformPreparation::prepare(config, Some(probe))?
            .open()
            .map(|(platform, _report)| platform)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn tamper_json_document_for_nonproduction_harness(
        &self,
        namespace: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<()> {
        self.engine.put_json_value(namespace, key, value)
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    #[allow(dead_code)] // SDK unit/replay internals may supply an attested firmware observation.
    pub(crate) fn open_with_nonproduction_probe(
        config: StoreBackendConfig,
        probe: Arc<dyn RuntimeResourceProbe>,
    ) -> Result<Self> {
        StorePlatformPreparation::prepare(config, Some(probe))?
            .open()
            .map(|(platform, _report)| platform)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn prepare_for_nonproduction_harness(
        config: StoreBackendConfig,
    ) -> Result<StorePlatformPreparation> {
        StorePlatformPreparation::prepare(config, None)
    }

    pub fn scoped_long_term_memory_read_store(
        &self,
        memory_space_id: &str,
    ) -> Result<Arc<dyn bm_core::memory::LongTermMemoryReadStore>> {
        let memory_space_id = memory_space_id.trim().to_string();
        let key_prefix = scoped_long_term_memory_storage_prefix(&memory_space_id)?;
        Ok(Arc::new(ScopedLongTermMemoryStore {
            platform: self.clone(),
            memory_space_id,
            key_prefix,
        }))
    }

    pub fn scoped_long_term_memory_control_read_store(
        &self,
        memory_space_id: &str,
    ) -> Result<Arc<dyn LongTermMemoryControlReadStore>> {
        scoped_long_term_control_storage_prefix(
            memory_space_id,
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
        )?;
        Ok(Arc::new(ScopedLongTermMemoryControlReadStore {
            platform: self.clone(),
            memory_space_id: memory_space_id.trim().to_string(),
            mounted_subject_id: self.config.event_scope.subject_id.clone(),
        }))
    }

    pub fn plan_continuity_snapshot_import_mutations(
        &self,
        plan: &ContinuitySnapshotImportPlan,
    ) -> Result<(Vec<StoreMutation>, Vec<StoreJsonPrecondition>)> {
        let mut mutations = Vec::new();
        let mut preconditions = Vec::new();
        if let Some(write) = plan.writes.summary.as_ref() {
            let observed =
                write
                    .observed
                    .as_ref()
                    .map(|(summary, message_count)| SessionSummaryRecord {
                        summary: summary.clone(),
                        message_count: *message_count,
                    });
            push_continuity_snapshot_json_write(
                &mut mutations,
                &mut preconditions,
                "session_summary",
                &write.chat_id,
                observed.as_ref(),
                &SessionSummaryRecord {
                    summary: write.summary.clone(),
                    message_count: write.message_count,
                },
            )?;
        }
        macro_rules! push_write {
            ($field:ident, $namespace:literal) => {
                if let Some(write) = plan.writes.$field.as_ref() {
                    push_continuity_snapshot_json_write(
                        &mut mutations,
                        &mut preconditions,
                        $namespace,
                        &write.key,
                        write.observed.as_ref(),
                        &write.next,
                    )?;
                }
            };
        }
        push_write!(self_model, "self_model");
        push_write!(self_authored_core, "self_authored_core");
        push_write!(core_revision_ledger, "core_revision_ledger");
        push_write!(self_continuity, "self_continuity");
        push_write!(relationship_constitution, "relationship_constitution");
        push_write!(relationship_portfolio, "relationship_portfolio");
        push_write!(execution_state, "execution_state");
        Ok((mutations, preconditions))
    }

    fn open_prepared_at(
        preparation: StorePlatformPreparation,
        capacity: StoreCapacityBudget,
        now_secs: u64,
    ) -> Result<(Self, StoreOpenReport)> {
        preparation.consume()?;
        let current_report = preparation
            .runtime_budget_authority
            .current_report(now_secs);
        if current_report.resource_snapshot.stale
            || current_report.resource_snapshot.is_expired(now_secs)
            || current_report.report_id != preparation.report_id
            || current_report.report_id != preparation.runtime_budget.report_id
            || current_report.resource_snapshot.source != preparation.probe_source
            || current_report.store_medium != preparation.store_medium
        {
            return Err(Error::config(
                "store_prepared_admission_invalid",
                "prepared store report identity, freshness, probe source, or medium changed before open",
            ));
        }
        let StorePlatformPreparation {
            config,
            runtime_budget_authority,
            runtime_budget: _,
            report_id: _,
            probe_source: _,
            store_medium: _,
            consumption: _,
        } = preparation;
        let (engine, repair, schema_manifest): (
            Arc<dyn StoreEngine>,
            StoreRepairReport,
            StoreSchemaManifest,
        ) = {
            let admission_authority = StoreAdmissionAuthority::new();
            match config.backend {
                StoreBackendKind::InMemory => (
                    Arc::new(InMemoryStoreEngine::new_with_admission_authority(
                        capacity,
                        admission_authority.clone(),
                    )),
                    StoreRepairReport::clean(),
                    StoreSchemaManifest::new(config.backend, config.profile, now_secs),
                ),
                StoreBackendKind::Embedded => (
                    Arc::new(EmbeddedStoreEngine::new_with_admission_authority(
                        capacity,
                        admission_authority.clone(),
                    )),
                    StoreRepairReport::clean(),
                    StoreSchemaManifest::new(config.backend, config.profile, now_secs),
                ),
                StoreBackendKind::File => {
                    let (engine, repair, manifest) =
                        FileStoreEngine::open_with_capacity_and_authority(
                            &config,
                            capacity,
                            admission_authority.clone(),
                        )?;
                    (Arc::new(engine), repair, manifest)
                }
                StoreBackendKind::Sqlite => {
                    let (engine, manifest) =
                        sqlite_engine(&config, capacity, admission_authority.clone())?;
                    (engine, StoreRepairReport::clean(), manifest)
                }
            }
        };
        let report = StoreOpenReport {
            backend: config.backend.as_str().to_string(),
            schema_id: STORE_SCHEMA_ID.to_string(),
            repair,
        };
        let platform = Self {
            config,
            capacity,
            engine,
            transaction_mutex: Arc::new(Mutex::new(())),
            schema_manifest,
            open_report: report.clone(),
            runtime_budget_authority,
        };
        platform.emit_runtime_event("open")?;
        Ok((platform, report))
    }

    pub fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        self.engine.read_events()
    }

    fn lock_transaction(&self, stage: &'static str) -> Result<std::sync::MutexGuard<'_, ()>> {
        self.transaction_mutex
            .lock()
            .map_err(|_| Error::config(stage, "transaction mutex poisoned"))
    }

    pub fn read_file_store_events(
        root: impl AsRef<Path>,
        capacity: StoreCapacityBudget,
    ) -> Result<Vec<MemoryStoreEvent>> {
        crate::store_internal::file::read_events_from_root(root.as_ref(), capacity)
    }

    pub(crate) fn current_runtime_budget(&self, now_secs: u64) -> RuntimeBudgetReport {
        self.runtime_budget_authority.current_report(now_secs)
    }

    #[allow(
        dead_code,
        reason = "foundation API consumed by the production recall integration"
    )]
    pub(crate) fn with_recall_immutable_read_session<T>(
        &self,
        runtime_budget: &RuntimeBudgetReport,
        read: impl FnOnce(&mut RecallImmutableReadContext<'_>) -> Result<T>,
    ) -> Result<(T, StoreReadReceipt)> {
        let now_secs = current_unix_secs();
        runtime_budget.validate_for_admission(now_secs)?;
        let active_report = crate::RuntimeBudgetLease::active_report(
            &self.runtime_budget_authority,
        )
        .ok_or_else(|| {
            Error::config(
                "recall_immutable_read_session_admission",
                "recall requires an active request-scoped runtime budget lease",
            )
        })?;
        if active_report.report_id != runtime_budget.report_id {
            return Err(Error::config(
                "recall_immutable_read_session_admission",
                "recall store session budget identity differs from the active request lease",
            ));
        }
        let capacity = StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        let session = self.engine.open_immutable_read_session(capacity)?;
        let mut context = RecallImmutableReadContext::new(session);
        let output = read(&mut context)?;
        let receipt = context.receipt()?;
        Ok((output, receipt))
    }

    pub(crate) fn refresh_runtime_resource_snapshot(
        &self,
        now_secs: u64,
    ) -> Result<RuntimeBudgetReport> {
        self.runtime_budget_authority.refresh(now_secs)
    }

    pub(crate) fn refresh_runtime_resource_snapshot_if_stale(
        &self,
        now_secs: u64,
    ) -> Result<RuntimeBudgetReport> {
        let current = self.current_runtime_budget(now_secs);
        if current.resource_snapshot.stale {
            return self.refresh_runtime_resource_snapshot(now_secs);
        }
        Ok(current)
    }

    pub fn open_in_memory(config: StoreBackendConfig) -> Result<Self> {
        if config.backend != StoreBackendKind::InMemory {
            return Err(Error::config(
                "store_backend_config",
                "open_in_memory requires in-memory backend config",
            ));
        }
        Self::open(config)
    }

    pub fn config(&self) -> &StoreBackendConfig {
        &self.config
    }

    pub const fn capacity(&self) -> StoreCapacityBudget {
        self.capacity
    }

    pub(crate) fn runtime_budget_authority(&self) -> Arc<RuntimeBudgetAuthority> {
        self.runtime_budget_authority.clone()
    }

    pub fn open_report(&self) -> &StoreOpenReport {
        &self.open_report
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn commit_governed_memory_transaction(
        &self,
        batch: StoreMutationBatch,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_with_preconditions(batch, &[])
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn commit_governed_memory_transaction_with_preconditions(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(batch, preconditions, None, None, None)
    }

    pub(crate) fn commit_governed_memory_transaction_with_runtime_budget(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        runtime_budget: &RuntimeBudgetReport,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(
            batch,
            preconditions,
            None,
            Some(runtime_budget),
            None,
        )
    }

    pub(crate) fn commit_governed_memory_transaction_with_runtime_budget_at(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        runtime_budget: &RuntimeBudgetReport,
        runtime_timestamp_unix_secs: u64,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(
            batch,
            preconditions,
            None,
            Some(runtime_budget),
            Some(runtime_timestamp_unix_secs),
        )
    }

    pub(crate) fn commit_governed_graph_repair_transaction_with_runtime_budget(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        authority: GraphRepairAuthority,
        runtime_budget: &RuntimeBudgetReport,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(
            batch,
            preconditions,
            Some(authority),
            Some(runtime_budget),
            None,
        )
    }

    fn commit_governed_memory_transaction_authorized(
        &self,
        mut batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        graph_repair_authority: Option<GraphRepairAuthority>,
        pinned_runtime_budget: Option<&RuntimeBudgetReport>,
        runtime_timestamp_unix_secs: Option<u64>,
    ) -> Result<StoreMutationBatchReport> {
        let mut preconditions = preconditions.to_vec();
        let transaction_timestamp =
            canonical_transaction_timestamp(&batch, runtime_timestamp_unix_secs)?;
        self.append_runtime_skill_recall_index_closure(
            &mut batch,
            &mut preconditions,
            transaction_timestamp,
        )?;
        self.append_conversation_derived_ref_recall_index_closure(
            &mut batch,
            &mut preconditions,
            transaction_timestamp,
        )?;
        self.append_recall_owner_scope_binding_closure(&mut batch, &mut preconditions)?;
        self.append_control_plane_scope_manifest_closure(&mut batch, &mut preconditions)?;
        if batch.transaction_id.trim().is_empty() {
            return Err(Error::config(
                "memory_write_transaction_preflight_failed",
                "transaction_id is required",
            ));
        }
        if batch.operation.trim().is_empty() {
            return Err(Error::config(
                "memory_write_transaction_preflight_failed",
                "operation is required",
            ));
        }

        let owned_runtime_budget;
        let runtime_budget = if let Some(runtime_budget) = pinned_runtime_budget {
            runtime_budget
        } else {
            owned_runtime_budget = self.current_runtime_budget(current_unix_secs());
            &owned_runtime_budget
        };
        if runtime_budget.resource_snapshot.stale {
            return Err(Error::config(
                "memory_write_transaction_resource_admission",
                "store transaction requires a fresh runtime budget report",
            ));
        }
        let operation_capacity =
            StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);

        validate_batch_mutation_namespaces(&batch)?;
        validate_protected_json_mutation_preconditions(&batch, &preconditions)?;
        validate_recall_index_mutation_closure(
            &batch,
            |namespace, key| self.engine.get_json_value(namespace, key),
            |namespace, key| self.engine.get_blob(namespace, key),
        )?;
        validate_evidence_effect_address_closure(&batch, &preconditions)?;
        validate_governed_owner_facet_closure(&batch, &preconditions)?;
        validate_graph_manifest_closure(&batch)?;
        validate_control_audit_closure(&batch, &preconditions)?;
        validate_evidence_lifecycle_closure(&batch)?;

        let mut engine_mutations = Vec::new();

        for mutation in &batch.mutations {
            match mutation {
                StoreMutation::PutJson {
                    namespace,
                    key,
                    value,
                    event_kind,
                    plane,
                    record_key,
                } => {
                    ensure_batch_json_namespace(namespace)?;
                    enforce_logical_key_budget(
                        operation_capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    let event = self.build_batch_event(
                        &batch,
                        transaction_timestamp,
                        event_kind.clone(),
                        plane,
                        record_key,
                        stable_hash_json(value)
                            .map_err(memory_write_transaction_preflight_error)?,
                    );
                    enforce_event_key_budget(
                        operation_capacity,
                        &event,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    engine_mutations.push(StoreEngineMutation::PutJson {
                        namespace: namespace.clone(),
                        key: key.clone(),
                        value: value.clone(),
                    });
                    engine_mutations.push(StoreEngineMutation::AppendEvent {
                        event: Box::new(event),
                    });
                }
                StoreMutation::DeleteJson {
                    namespace,
                    key,
                    event_kind,
                    plane,
                    record_key,
                } => {
                    ensure_batch_json_namespace(namespace)?;
                    enforce_logical_key_budget(
                        operation_capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    let event_template = self.build_batch_event_template(
                        &batch,
                        transaction_timestamp,
                        event_kind.clone(),
                        plane,
                        record_key,
                    );
                    engine_mutations.push(StoreEngineMutation::DeleteJsonIfPresent {
                        namespace: namespace.clone(),
                        key: key.clone(),
                        event_template: Box::new(event_template),
                    });
                }
                StoreMutation::PutBlob {
                    namespace,
                    key,
                    value,
                    event_kind,
                    plane,
                    record_key,
                } => {
                    ensure_batch_blob_namespace(namespace)?;
                    enforce_logical_key_budget(
                        operation_capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    let event = self.build_batch_event(
                        &batch,
                        transaction_timestamp,
                        event_kind.clone(),
                        plane,
                        record_key,
                        stable_hash_hex(value),
                    );
                    enforce_event_key_budget(
                        operation_capacity,
                        &event,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    engine_mutations.push(StoreEngineMutation::PutBlob {
                        namespace: namespace.clone(),
                        key: key.clone(),
                        value: value.clone(),
                    });
                    engine_mutations.push(StoreEngineMutation::AppendEvent {
                        event: Box::new(event),
                    });
                }
                StoreMutation::DeleteBlob {
                    namespace,
                    key,
                    event_kind,
                    plane,
                    record_key,
                } => {
                    ensure_batch_blob_namespace(namespace)?;
                    enforce_logical_key_budget(
                        operation_capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    let event_template = self.build_batch_event_template(
                        &batch,
                        transaction_timestamp,
                        event_kind.clone(),
                        plane,
                        record_key,
                    );
                    engine_mutations.push(StoreEngineMutation::DeleteBlobIfPresent {
                        namespace: namespace.clone(),
                        key: key.clone(),
                        event_template: Box::new(event_template),
                    });
                }
                StoreMutation::AppendEvent { event } => {
                    enforce_event_key_budget(operation_capacity, event, "memory_write_transaction")
                        .map_err(memory_write_transaction_preflight_error)?;
                    engine_mutations.push(StoreEngineMutation::AppendEvent {
                        event: Box::new(event.clone()),
                    });
                }
            }
        }

        let governed_json_reads =
            governed_transaction_dependency_json_reads(&batch, &preconditions)?;
        let mut request = StoreTransactionRequest::new(
            batch.transaction_id.clone(),
            preconditions,
            engine_mutations,
            Some(Box::new(batch.clone())),
        )
        .include_governed_json_reads(governed_json_reads);
        if batch.mutations.iter().any(|mutation| {
            matches!(
                mutation,
                StoreMutation::PutJson { namespace, .. }
                    | StoreMutation::DeleteJson { namespace, .. }
                    if matches!(
                        namespace.as_str(),
                        MEMORY_GRAPH_MANIFEST_NAMESPACE
                            | MEMORY_GRAPH_REVISION_NAMESPACE
                            | MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE
                            | MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE
                            | MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE
                            | MEMORY_GRAPH_INDEX_NAMESPACE
                            | MEMORY_GRAPH_NODE_NAMESPACE
                            | MEMORY_GRAPH_EDGE_NAMESPACE
                            | MEMORY_GRAPH_BACKLINK_NAMESPACE
                    )
            )
        }) {
            let scope_digest =
                memory_graph_scope_digest(&batch.scope.memory_space_id, &batch.scope.subject_id);
            let prefix = format!("scope:{scope_digest}:doc:");
            request = request.include_governed_json_prefix_reads(
                [
                    MEMORY_GRAPH_MANIFEST_NAMESPACE,
                    MEMORY_GRAPH_REVISION_NAMESPACE,
                    MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
                    MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
                    MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
                    MEMORY_GRAPH_INDEX_NAMESPACE,
                    MEMORY_GRAPH_NODE_NAMESPACE,
                    MEMORY_GRAPH_EDGE_NAMESPACE,
                    MEMORY_GRAPH_BACKLINK_NAMESPACE,
                ]
                .into_iter()
                .map(|namespace| (namespace.to_string(), prefix.clone())),
            );
        }
        if let Some(authority) = graph_repair_authority {
            request = request.authorize_graph_repair(authority);
        }
        let admission = self.store_transaction_admission_for_report(runtime_budget)?;
        let engine_report = self
            .engine
            .commit_transaction_admitted(&request, &admission)
            .map_err(memory_write_transaction_commit_error)?;

        Ok(StoreMutationBatchReport {
            transaction_id: batch.transaction_id,
            admitted: true,
            committed: true,
            mutations: batch.mutations.len(),
            events: engine_report.appended_events,
            changed_json: engine_report.changed_json,
            changed_blobs: engine_report.changed_blobs,
            budget_report: engine_report.budget_report,
            event_ids: engine_report.event_ids,
        })
    }

    fn append_recall_owner_scope_binding_closure(
        &self,
        batch: &mut StoreMutationBatch,
        preconditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<()> {
        let mut plans = Vec::new();
        let mut seen = BTreeSet::new();
        for mutation in &batch.mutations {
            let plan = match mutation {
                StoreMutation::PutJson {
                    namespace,
                    key,
                    value,
                    ..
                } if recall_index_namespace_for_json_owner(namespace).is_some() => {
                    let digest =
                        RecallIndexAddress::json(namespace, key, 1, 0, value)?.content_sha256;
                    Some(("json", namespace.as_str(), key.as_str(), Some(digest)))
                }
                StoreMutation::DeleteJson { namespace, key, .. }
                    if recall_index_namespace_for_json_owner(namespace).is_some() =>
                {
                    Some(("json", namespace.as_str(), key.as_str(), None))
                }
                StoreMutation::PutBlob {
                    namespace,
                    key,
                    value,
                    ..
                } if recall_index_namespace_for_blob_owner(namespace).is_some() => {
                    let digest =
                        RecallIndexAddress::blob(namespace, key, 1, 0, value)?.content_sha256;
                    Some(("blob", namespace.as_str(), key.as_str(), Some(digest)))
                }
                StoreMutation::DeleteBlob { namespace, key, .. }
                    if recall_index_namespace_for_blob_owner(namespace).is_some() =>
                {
                    Some(("blob", namespace.as_str(), key.as_str(), None))
                }
                _ => None,
            };
            let Some((owner_kind, owner_namespace, owner_key, content_digest)) = plan else {
                continue;
            };
            let identity = (
                owner_kind.to_string(),
                owner_namespace.to_string(),
                owner_key.to_string(),
            );
            if !seen.insert(identity.clone()) {
                return Err(Error::config(
                    "recall_owner_scope_binding",
                    "one transaction cannot mutate the same recall owner twice",
                ));
            }
            plans.push((identity.0, identity.1, identity.2, content_digest));
        }
        for (owner_kind, owner_namespace, owner_key, content_digest) in plans {
            let binding_key =
                recall_owner_scope_binding_key(&owner_kind, &owner_namespace, &owner_key)?;
            let previous = self
                .engine
                .get_json_value(RECALL_OWNER_SCOPE_BINDING_NAMESPACE, &binding_key)?;
            preconditions.push(match previous.clone() {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                    key: binding_key.clone(),
                    value,
                },
                None => StoreJsonPrecondition::Absent {
                    namespace: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                    key: binding_key.clone(),
                },
            });
            match content_digest {
                Some(content_digest) => {
                    if let Some(previous) = previous {
                        let previous = serde_json::from_value::<RecallOwnerScopeBinding>(previous)
                            .map_err(|error| {
                                Error::config(
                                    "recall_owner_scope_binding",
                                    format!("existing binding decode failed: {error}"),
                                )
                            })?;
                        previous.validate()?;
                        if previous.memory_space_id != batch.scope.memory_space_id
                            || previous.mounted_subject_id != batch.scope.subject_id
                        {
                            return Err(Error::config(
                                "recall_owner_scope_binding",
                                "recall owner is already bound to another exact subject scope",
                            ));
                        }
                    }
                    let binding = RecallOwnerScopeBinding::build(
                        &batch.scope.memory_space_id,
                        &batch.scope.subject_id,
                        &owner_kind,
                        &owner_namespace,
                        &owner_key,
                        &content_digest,
                    )?;
                    batch.mutations.push(StoreMutation::PutJson {
                        namespace: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                        key: binding_key.clone(),
                        value: serde_json::to_value(binding).map_err(|error| {
                            Error::config("recall_owner_scope_binding", error.to_string())
                        })?,
                        event_kind: MemoryStoreEventKind::MemoryMaintenance,
                        plane: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                        record_key: binding_key,
                    });
                }
                None => {
                    let previous = previous.ok_or_else(|| {
                        Error::config(
                            "recall_owner_scope_binding",
                            "recall owner delete requires an existing exact scope binding",
                        )
                    })?;
                    let previous = serde_json::from_value::<RecallOwnerScopeBinding>(previous)
                        .map_err(|error| {
                            Error::config(
                                "recall_owner_scope_binding",
                                format!("existing binding decode failed: {error}"),
                            )
                        })?;
                    previous.validate()?;
                    if previous.memory_space_id != batch.scope.memory_space_id
                        || previous.mounted_subject_id != batch.scope.subject_id
                    {
                        return Err(Error::config(
                            "recall_owner_scope_binding",
                            "recall owner delete scope differs from its exact binding",
                        ));
                    }
                    batch.mutations.push(StoreMutation::DeleteJson {
                        namespace: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                        key: binding_key.clone(),
                        event_kind: MemoryStoreEventKind::MemoryMaintenance,
                        plane: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                        record_key: binding_key,
                    });
                }
            }
        }
        Ok(())
    }

    fn append_control_plane_scope_manifest_closure(
        &self,
        batch: &mut StoreMutationBatch,
        preconditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<()> {
        const CONTROL_NAMESPACES: &[&str] = &[
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
            LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
            LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
            LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        ];
        let control_mutations = batch
            .mutations
            .iter()
            .filter(|mutation| match mutation {
                StoreMutation::PutJson { namespace, .. }
                | StoreMutation::DeleteJson { namespace, .. } => {
                    CONTROL_NAMESPACES.contains(&namespace.as_str())
                }
                _ => false,
            })
            .cloned()
            .collect::<Vec<_>>();
        if control_mutations.is_empty() {
            return Ok(());
        }
        let manifest_key = control_plane_scope_manifest_key(
            &batch.scope.memory_space_id,
            &batch.scope.subject_id,
        )?;
        let previous_value = self
            .engine
            .get_json_value(CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE, &manifest_key)?;
        let previous = previous_value
            .clone()
            .map(|value| {
                serde_json::from_value::<ControlPlaneScopeManifest>(value).map_err(|error| {
                    Error::config(
                        "control_plane_scope_manifest",
                        format!("existing manifest decode failed: {error}"),
                    )
                })
            })
            .transpose()?;
        if let Some(previous) = previous.as_ref() {
            previous.validate(self.capacity.kv_max_entries)?;
            if previous.physical_key != manifest_key
                || previous.memory_space_id != batch.scope.memory_space_id
                || previous.mounted_subject_id != batch.scope.subject_id
            {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "existing control-plane manifest differs from the transaction scope",
                ));
            }
            for entry in &previous.entries {
                let value = self
                    .engine
                    .get_json_value(&entry.namespace, &entry.key)?
                    .ok_or_else(|| {
                        Error::config(
                            "control_plane_scope_manifest",
                            "manifested control-plane document is missing",
                        )
                    })?;
                entry.validate_value(&value)?;
                validate_control_document_for_scope(
                    &entry.namespace,
                    &entry.key,
                    &value,
                    &batch.scope.memory_space_id,
                    &batch.scope.subject_id,
                )?;
            }
        }
        let mut entries = previous
            .as_ref()
            .map(|manifest| {
                manifest
                    .entries
                    .iter()
                    .cloned()
                    .map(|entry| ((entry.namespace.clone(), entry.key.clone()), entry))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        for mutation in control_mutations {
            match mutation {
                StoreMutation::PutJson {
                    namespace,
                    key,
                    value,
                    ..
                } => {
                    validate_control_document_for_scope(
                        &namespace,
                        &key,
                        &value,
                        &batch.scope.memory_space_id,
                        &batch.scope.subject_id,
                    )?;
                    entries.insert(
                        (namespace.clone(), key.clone()),
                        ControlPlaneScopeEntry::from_json(&namespace, &key, &value)?,
                    );
                }
                StoreMutation::DeleteJson { namespace, key, .. } => {
                    if entries.remove(&(namespace, key)).is_none() {
                        return Err(Error::config(
                            "control_plane_scope_manifest",
                            "control-plane delete is outside the exact subject manifest",
                        ));
                    }
                }
                _ => unreachable!("filtered control-plane JSON mutation"),
            }
        }
        preconditions.push(match previous_value {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace: CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                key: manifest_key.clone(),
                value,
            },
            None => StoreJsonPrecondition::Absent {
                namespace: CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                key: manifest_key.clone(),
            },
        });
        let next = ControlPlaneScopeManifest::build(
            previous
                .as_ref()
                .map(|manifest| manifest.revision.saturating_add(1))
                .unwrap_or(1),
            &batch.scope.memory_space_id,
            &batch.scope.subject_id,
            entries.into_values(),
            self.capacity.kv_max_entries,
        )?;
        batch.mutations.push(StoreMutation::PutJson {
            namespace: CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
            key: manifest_key.clone(),
            value: serde_json::to_value(next).map_err(|error| {
                Error::config("control_plane_scope_manifest", error.to_string())
            })?,
            event_kind: MemoryStoreEventKind::MemoryMaintenance,
            plane: CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
            record_key: manifest_key,
        });
        Ok(())
    }

    pub fn into_arc(self) -> Arc<dyn Platform> {
        Arc::new(self)
    }

    pub(crate) fn with_runtime_event_scope(mut self, event_scope: StoreEventScope) -> Self {
        self.config.event_scope = event_scope;
        self
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn export_store_snapshot(&self) -> Result<StoreSnapshot> {
        self.export_store_snapshot_with_report()
            .map(|(snapshot, _report)| snapshot)
    }

    pub fn read_json_namespace(&self, namespace: &str) -> Result<Vec<StoreSnapshotJsonDoc>> {
        ensure_json_snapshot_namespace(namespace, "store_json_namespace_read")?;
        let mut docs = Vec::new();
        for key in self.engine.list_json_keys(namespace)? {
            if let Some(value) = self.engine.get_json_value(namespace, &key)? {
                docs.push(StoreSnapshotJsonDoc {
                    namespace: namespace.to_string(),
                    key,
                    value,
                });
            }
        }
        Ok(docs)
    }

    pub(crate) fn read_governed_evidence_exact(
        &self,
        runtime_budget: &RuntimeBudgetReport,
        request: StoreGovernedEvidenceExactReadRequest,
    ) -> Result<StoreGovernedEvidenceExactReadResult> {
        let stage = "governed_evidence_exact_read_admission";
        runtime_budget.validate_for_admission(current_unix_secs())?;
        let active_report = crate::RuntimeBudgetLease::active_report(
            &self.runtime_budget_authority,
        )
        .ok_or_else(|| {
            Error::config(
                stage,
                "exact evidence read requires an active request-scoped runtime budget lease",
            )
        })?;
        if active_report.report_id != runtime_budget.report_id {
            return Err(Error::config(
                stage,
                "exact evidence read budget identity differs from the active request lease",
            ));
        }
        let capacity = StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        let mut session = self.engine.open_immutable_read_session(capacity)?;
        read_governed_evidence_exact_in_session(session.as_mut(), &request)
    }

    pub fn read_json_docs_by_keys(
        &self,
        namespace: &str,
        keys: &[String],
    ) -> Result<Vec<StoreSnapshotJsonDoc>> {
        ensure_json_snapshot_namespace(namespace, "store_json_namespace_read")?;
        let addresses = keys
            .iter()
            .map(|key| (namespace.to_string(), key.clone()))
            .collect::<Vec<_>>();
        self.read_json_docs_by_addresses(&addresses)
    }

    pub fn read_json_docs_by_addresses(
        &self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreSnapshotJsonDoc>> {
        let mut seen = BTreeSet::new();
        let addresses = addresses
            .iter()
            .filter(|(namespace, key)| seen.insert((namespace.as_str(), key.as_str())))
            .map(|(namespace, key)| {
                ensure_json_snapshot_namespace(namespace, "store_json_namespace_read")?;
                Ok((namespace.clone(), key.clone()))
            })
            .collect::<Result<Vec<_>>>()?;
        let runtime_budget = self.current_runtime_budget(current_unix_secs());
        if runtime_budget.resource_snapshot.stale {
            return Err(Error::config(
                "store_json_known_key_read",
                "known-key read requires a fresh runtime budget report",
            ));
        }
        let result = self.engine.read_consistent_known_keys(
            &addresses,
            &[],
            false,
            StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget),
        )?;
        Ok(result
            .json
            .into_iter()
            .filter_map(|read| {
                read.value.map(|value| StoreSnapshotJsonDoc {
                    namespace: read.namespace,
                    key: read.key,
                    value,
                })
            })
            .collect())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn export_store_snapshot_with_report(
        &self,
    ) -> Result<(StoreSnapshot, StoreSnapshotExportReport)> {
        let mut json_docs = Vec::new();
        for namespace in JSON_SNAPSHOT_NAMESPACES {
            for key in self.engine.list_json_keys(namespace)? {
                if let Some(value) = self.engine.get_json_value(namespace, &key)? {
                    json_docs.push(StoreSnapshotJsonDoc {
                        namespace: (*namespace).to_string(),
                        key,
                        value,
                    });
                }
            }
        }
        let mut blobs = Vec::new();
        for namespace in BLOB_SNAPSHOT_NAMESPACES {
            for key in self.engine.list_blob_keys(namespace)? {
                if let Some(value) = self.engine.get_blob(namespace, &key)? {
                    blobs.push(StoreSnapshotBlob {
                        namespace: (*namespace).to_string(),
                        key,
                        value,
                    });
                }
            }
        }
        let snapshot = StoreSnapshot::new(
            self.schema_manifest.clone(),
            json_docs,
            blobs,
            self.read_events()?,
        );
        self.enforce_snapshot_budget(&snapshot, self.capacity.export_max_bytes, "export")?;
        let report = snapshot.export_report();
        Ok((snapshot, report))
    }

    pub(crate) fn export_memory_space_projection_with_report(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        pinned_runtime_budget: Option<&RuntimeBudgetReport>,
    ) -> Result<(StoreSnapshot, StoreMemorySpaceProjectionReport)> {
        let owned_runtime_budget;
        let runtime_budget = if let Some(runtime_budget) = pinned_runtime_budget {
            runtime_budget
        } else {
            owned_runtime_budget = self.current_runtime_budget(current_unix_secs());
            &owned_runtime_budget
        };
        let operation_capacity =
            StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        let projection = self.engine.read_scoped_projection(
            &StoreScopedProjectionRequest {
                scope: StoreScopedProjectionScope::new(memory_space_id, mounted_subject_id)?,
                json_namespaces: JSON_SNAPSHOT_NAMESPACES
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                include_events: true,
            },
            operation_capacity,
        )?;
        let snapshot = StoreSnapshot::new(
            self.schema_manifest.clone(),
            projection.json_docs,
            Vec::new(),
            projection.events,
        );
        self.enforce_snapshot_budget(
            &snapshot,
            self.capacity.snapshot_max_bytes,
            "memory_space_export",
        )?;
        Ok((
            snapshot,
            StoreMemorySpaceProjectionReport {
                omitted_private_entries: 0,
            },
        ))
    }

    pub(crate) fn replace_memory_space_projection_with_report(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        snapshot: &StoreSnapshot,
        pinned_runtime_budget: Option<&RuntimeBudgetReport>,
    ) -> Result<StoreSnapshotImportReport> {
        validate_snapshot_import_contract(snapshot)?;
        validate_scoped_projection_governed_closure(snapshot, memory_space_id, mounted_subject_id)?;
        if !snapshot.blobs.is_empty() {
            return Err(Error::config(
                "memory_space_import",
                "typed memory-space archive must not contain unowned blobs",
            ));
        }
        let owned_runtime_budget;
        let runtime_budget = if let Some(runtime_budget) = pinned_runtime_budget {
            runtime_budget
        } else {
            owned_runtime_budget = self.current_runtime_budget(current_unix_secs());
            &owned_runtime_budget
        };
        let admission = self.store_transaction_admission_for_report(runtime_budget)?;
        let replace = self.engine.replace_scoped_projection(
            &StoreScopedProjectionReplaceRequest {
                scope: StoreScopedProjectionScope::new(memory_space_id, mounted_subject_id)?,
                json_namespaces: JSON_SNAPSHOT_NAMESPACES
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                json_docs: snapshot.json_docs.clone(),
                events: snapshot.events.clone(),
            },
            &admission,
        )?;
        Ok(StoreSnapshotImportReport {
            schema_id: snapshot.schema_id.clone(),
            json_docs: replace.inserted_json,
            blobs: 0,
            json_deleted: replace.deleted_json,
            blobs_deleted: 0,
            events_imported: replace.inserted_events,
            events_skipped: 0,
            state_fingerprint: snapshot.state_fingerprint(),
            event_fingerprint: snapshot.event_fingerprint(),
        })
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn import_store_snapshot(&self, snapshot: &StoreSnapshot) -> Result<()> {
        self.import_store_snapshot_with_report(snapshot)
            .map(|_report| ())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn import_store_snapshot_with_report(
        &self,
        snapshot: &StoreSnapshot,
    ) -> Result<StoreSnapshotImportReport> {
        validate_snapshot_import_contract(snapshot)?;
        enforce_snapshot_logical_budget(self.capacity, snapshot)?;
        self.enforce_snapshot_budget(snapshot, self.capacity.import_max_bytes, "import")?;
        let _transaction_guard = self.lock_transaction("store_snapshot_import")?;
        let replace_report = self.engine.replace_snapshot(
            JSON_SNAPSHOT_NAMESPACES,
            BLOB_SNAPSHOT_NAMESPACES,
            &snapshot.json_docs,
            &snapshot.blobs,
            &snapshot.events,
        )?;
        Ok(StoreSnapshotImportReport {
            schema_id: snapshot.schema_id.clone(),
            json_docs: snapshot.json_docs.len(),
            blobs: snapshot.blobs.len(),
            json_deleted: replace_report.json_deleted,
            blobs_deleted: replace_report.blobs_deleted,
            events_imported: replace_report.events_imported,
            events_skipped: 0,
            state_fingerprint: snapshot.state_fingerprint(),
            event_fingerprint: snapshot.event_fingerprint(),
        })
    }

    fn json_get<T>(&self, namespace: &str, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.engine
            .get_json_value(namespace, key)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| Error::config("store_json_decode", error.to_string()))
    }

    fn load_typed_recall_index<T: TypedRecallIndex>(
        &self,
        physical_key: &str,
    ) -> Result<(Option<T>, Option<serde_json::Value>)> {
        let value = self.engine.get_json_value(T::NAMESPACE, physical_key)?;
        let index = value
            .clone()
            .map(|value| decode_typed_recall_index::<T>(physical_key, value))
            .transpose()?;
        Ok((index, value))
    }

    fn commit_recall_indexed_mutations(
        &self,
        operation: &str,
        scope: StoreEventScope,
        owner_mutations: Vec<StoreMutation>,
        indexes: Vec<(
            &'static str,
            String,
            serde_json::Value,
            Option<serde_json::Value>,
        )>,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_recall_indexed_mutations_at(operation, scope, owner_mutations, indexes, None)
    }

    fn commit_recall_indexed_mutations_at(
        &self,
        operation: &str,
        scope: StoreEventScope,
        mut owner_mutations: Vec<StoreMutation>,
        indexes: Vec<RecallIndexMutationPlan>,
        runtime_timestamp_unix_secs: Option<u64>,
    ) -> Result<StoreMutationBatchReport> {
        let mut preconditions = Vec::with_capacity(indexes.len());
        for (namespace, key, value, before) in indexes {
            preconditions.push(match before {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace: namespace.to_string(),
                    key: key.clone(),
                    value,
                },
                None => StoreJsonPrecondition::Absent {
                    namespace: namespace.to_string(),
                    key: key.clone(),
                },
            });
            owner_mutations.push(StoreMutation::PutJson {
                namespace: namespace.to_string(),
                key: key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: namespace.to_string(),
                record_key: key,
            });
        }
        self.commit_governed_memory_transaction_authorized(
            StoreMutationBatch {
                transaction_id: format!("recall-index:{operation}:{}", current_unix_nanos()),
                operation: operation.to_string(),
                scope,
                mutations: owner_mutations,
            },
            &preconditions,
            None,
            None,
            runtime_timestamp_unix_secs,
        )
    }

    fn recall_scope(&self) -> StoreEventScope {
        self.config.event_scope.clone()
    }

    fn plan_archive_index_upsert(
        &self,
        address: RecallIndexAddress,
    ) -> Result<RecallIndexMutationPlan> {
        self.plan_archive_index_upsert_for_scope(
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            address,
        )
    }

    fn plan_archive_index_upsert_for_scope(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        address: RecallIndexAddress,
    ) -> Result<RecallIndexMutationPlan> {
        let key = ArchiveRecallManifest::build(
            1,
            memory_space_id,
            mounted_subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, before) = self.load_typed_recall_index::<ArchiveRecallManifest>(&key)?;
        let revision = previous
            .as_ref()
            .map(|index| index.revision.saturating_add(1))
            .unwrap_or(1);
        let entries = replace_recall_index_address(
            previous
                .as_ref()
                .map(|index| index.entries.as_slice())
                .unwrap_or(&[]),
            address,
        );
        let next =
            ArchiveRecallManifest::build(revision, memory_space_id, mounted_subject_id, entries)?;
        Ok((
            ArchiveRecallManifest::NAMESPACE,
            key,
            serde_json::to_value(next)
                .map_err(|error| Error::config("archive_recall_manifest", error.to_string()))?,
            before,
        ))
    }

    fn plan_archive_index_remove(
        &self,
        kind: RecallIndexAddressKind,
        namespace: &str,
        owner_key: &str,
    ) -> Result<RecallIndexMutationPlan> {
        let scope = &self.config.event_scope;
        let key = ArchiveRecallManifest::build(
            1,
            &scope.memory_space_id,
            &scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, before) = self.load_typed_recall_index::<ArchiveRecallManifest>(&key)?;
        let previous = previous.ok_or_else(|| {
            Error::config(
                "archive_recall_manifest",
                "owner exists without its required archive recall manifest",
            )
        })?;
        let entries = remove_recall_index_address(&previous.entries, kind, namespace, owner_key);
        let next = ArchiveRecallManifest::build(
            previous.revision.saturating_add(1),
            &scope.memory_space_id,
            &scope.subject_id,
            entries,
        )?;
        Ok((
            ArchiveRecallManifest::NAMESPACE,
            key,
            serde_json::to_value(next)
                .map_err(|error| Error::config("archive_recall_manifest", error.to_string()))?,
            before,
        ))
    }

    fn put_archive_json_owner<T: Serialize>(
        &self,
        operation: &str,
        namespace: &str,
        key: &str,
        value: &T,
    ) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_json_owner_write")?;
        let value = serde_json::to_value(value)
            .map_err(|error| Error::config("archive_recall_manifest", error.to_string()))?;
        let root_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&root_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            namespace,
            key,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                namespace,
                key,
            ),
            current_unix_secs(),
            &value,
        )?;
        let index = self.plan_archive_index_upsert(address)?;
        self.commit_recall_indexed_mutations(
            operation,
            self.recall_scope(),
            vec![StoreMutation::PutJson {
                namespace: namespace.to_string(),
                key: key.to_string(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: namespace.to_string(),
                record_key: key.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn delete_archive_json_owner(&self, operation: &str, namespace: &str, key: &str) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_json_owner_delete")?;
        if self.engine.get_json_value(namespace, key)?.is_none() {
            return Ok(());
        }
        let index = self.plan_archive_index_remove(RecallIndexAddressKind::Json, namespace, key)?;
        self.commit_recall_indexed_mutations(
            operation,
            self.recall_scope(),
            vec![StoreMutation::DeleteJson {
                namespace: namespace.to_string(),
                key: key.to_string(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: namespace.to_string(),
                record_key: key.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn put_archive_blob_owner(
        &self,
        operation: &str,
        namespace: &str,
        key: &str,
        value: &[u8],
    ) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_blob_owner_write")?;
        let root_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&root_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::blob(
            namespace,
            key,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Blob,
                namespace,
                key,
            ),
            current_unix_secs(),
            value,
        )?;
        let index = self.plan_archive_index_upsert(address)?;
        self.commit_recall_indexed_mutations(
            operation,
            self.recall_scope(),
            vec![StoreMutation::PutBlob {
                namespace: namespace.to_string(),
                key: key.to_string(),
                value: value.to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: namespace.to_string(),
                record_key: key.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn plan_runtime_skill_index_upsert(
        &self,
        name: &str,
        content: &[u8],
        updated_at: u64,
    ) -> Result<RecallIndexMutationPlan> {
        let scope = &self.config.event_scope;
        let key = RuntimeSkillRecallManifest::build(
            1,
            &scope.memory_space_id,
            &scope.agent_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, before) =
            self.load_typed_recall_index::<RuntimeSkillRecallManifest>(&key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::blob(
            "skills",
            name,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Blob,
                "skills",
                name,
            ),
            updated_at,
            content,
        )?;
        let next = RuntimeSkillRecallManifest::build(
            previous
                .as_ref()
                .map(|index| index.revision.saturating_add(1))
                .unwrap_or(1),
            &scope.memory_space_id,
            &scope.agent_id,
            replace_recall_index_address(previous_entries, address),
        )?;
        Ok((
            RuntimeSkillRecallManifest::NAMESPACE,
            key,
            serde_json::to_value(next).map_err(|error| {
                Error::config("runtime_skill_recall_manifest", error.to_string())
            })?,
            before,
        ))
    }

    fn plan_runtime_skill_index_remove(&self, name: &str) -> Result<RecallIndexMutationPlan> {
        let scope = &self.config.event_scope;
        let key = RuntimeSkillRecallManifest::build(
            1,
            &scope.memory_space_id,
            &scope.agent_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, before) =
            self.load_typed_recall_index::<RuntimeSkillRecallManifest>(&key)?;
        let previous = previous.ok_or_else(|| {
            Error::config(
                "runtime_skill_recall_manifest",
                "skill exists without its required recall manifest",
            )
        })?;
        let next = RuntimeSkillRecallManifest::build(
            previous.revision.saturating_add(1),
            &scope.memory_space_id,
            &scope.agent_id,
            remove_recall_index_address(
                &previous.entries,
                RecallIndexAddressKind::Blob,
                "skills",
                name,
            ),
        )?;
        Ok((
            RuntimeSkillRecallManifest::NAMESPACE,
            key,
            serde_json::to_value(next).map_err(|error| {
                Error::config("runtime_skill_recall_manifest", error.to_string())
            })?,
            before,
        ))
    }

    fn append_runtime_skill_recall_index_closure(
        &self,
        batch: &mut StoreMutationBatch,
        preconditions: &mut Vec<StoreJsonPrecondition>,
        canonical_updated_at: u64,
    ) -> Result<()> {
        let skill_mutations = batch
            .mutations
            .iter()
            .filter(|mutation| {
                matches!(mutation,
                StoreMutation::PutBlob { namespace, .. }
                | StoreMutation::DeleteBlob { namespace, .. } if namespace == "skills")
            })
            .cloned()
            .collect::<Vec<_>>();
        if skill_mutations.is_empty()
            || batch.mutations.iter().any(|mutation| {
                matches!(mutation,
                StoreMutation::PutJson { namespace, .. }
                | StoreMutation::DeleteJson { namespace, .. }
                    if namespace == RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE)
            })
        {
            return Ok(());
        }
        let key = RuntimeSkillRecallManifest::build(
            1,
            &batch.scope.memory_space_id,
            &batch.scope.agent_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, before) =
            self.load_typed_recall_index::<RuntimeSkillRecallManifest>(&key)?;
        let mut entries = previous
            .as_ref()
            .map(|index| index.entries.clone())
            .unwrap_or_default();
        for mutation in skill_mutations {
            match mutation {
                StoreMutation::PutBlob { key, value, .. } => {
                    let address = RecallIndexAddress::blob(
                        "skills",
                        &key,
                        next_entry_revision(&entries, RecallIndexAddressKind::Blob, "skills", &key),
                        canonical_updated_at,
                        &value,
                    )?;
                    entries = replace_recall_index_address(&entries, address);
                }
                StoreMutation::DeleteBlob { key, .. } => {
                    if previous.is_none() {
                        return Err(Error::config(
                            "runtime_skill_recall_manifest",
                            "skill delete requires an existing typed recall manifest",
                        ));
                    }
                    entries = remove_recall_index_address(
                        &entries,
                        RecallIndexAddressKind::Blob,
                        "skills",
                        &key,
                    );
                }
                _ => unreachable!("filtered skill mutations"),
            }
        }
        let next = RuntimeSkillRecallManifest::build(
            previous
                .as_ref()
                .map(|index| index.revision.saturating_add(1))
                .unwrap_or(1),
            &batch.scope.memory_space_id,
            &batch.scope.agent_id,
            entries,
        )?;
        preconditions.push(match before {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace: RuntimeSkillRecallManifest::NAMESPACE.to_string(),
                key: key.clone(),
                value,
            },
            None => StoreJsonPrecondition::Absent {
                namespace: RuntimeSkillRecallManifest::NAMESPACE.to_string(),
                key: key.clone(),
            },
        });
        batch.mutations.push(StoreMutation::PutJson {
            namespace: RuntimeSkillRecallManifest::NAMESPACE.to_string(),
            key: key.clone(),
            value: serde_json::to_value(next).map_err(|error| {
                Error::config("runtime_skill_recall_manifest", error.to_string())
            })?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: RuntimeSkillRecallManifest::NAMESPACE.to_string(),
            record_key: key,
        });
        Ok(())
    }

    fn append_conversation_derived_ref_recall_index_closure(
        &self,
        batch: &mut StoreMutationBatch,
        preconditions: &mut Vec<StoreJsonPrecondition>,
        canonical_updated_at: u64,
    ) -> Result<()> {
        let mut groups =
            BTreeMap::<(String, String, String), Vec<(String, serde_json::Value)>>::new();
        for mutation in &batch.mutations {
            let StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } = mutation
            else {
                continue;
            };
            if namespace != "conversation_transcript_derived_ref" {
                continue;
            }
            let derived =
                serde_json::from_value::<DerivedMemoryRef>(value.clone()).map_err(|error| {
                    Error::config("conversation_recall_manifest", error.to_string())
                })?;
            let subject_id = derived
                .subject_id
                .as_deref()
                .or(derived.source.subject_id.as_deref())
                .ok_or_else(|| {
                    Error::config(
                        "conversation_recall_manifest",
                        "derived memory ref requires an exact subject owner",
                    )
                })?;
            if derived
                .subject_id
                .as_deref()
                .zip(derived.source.subject_id.as_deref())
                .is_some_and(|(owner, source_owner)| owner != source_owner)
                || derived.source.memory_space_id != batch.scope.memory_space_id
                || subject_id != batch.scope.subject_id
            {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "derived memory ref owner does not match the transaction scope",
                ));
            }
            groups
                .entry((
                    derived.source.memory_space_id,
                    derived.source.channel_id,
                    derived.source.conversation_id,
                ))
                .or_default()
                .push((key.clone(), value.clone()));
        }

        for ((memory_space_id, channel_id, conversation_id), owners) in groups {
            let conversation_key =
                ConversationKey::new(memory_space_id, channel_id, conversation_id)?;
            let manifest_key = ConversationRecallManifest::build(
                1,
                &conversation_key.memory_space_id,
                &batch.scope.subject_id,
                &conversation_key.channel_id,
                &conversation_key.conversation_id,
                std::iter::empty(),
            )?
            .physical_key;
            let existing_position = batch.mutations.iter().position(|mutation| {
                matches!(mutation,
                    StoreMutation::PutJson { namespace, key, .. }
                        if namespace == ConversationRecallManifest::NAMESPACE && key == &manifest_key)
            });

            let (before, revision, mut entries) = if let Some(position) = existing_position {
                let StoreMutation::PutJson { value, .. } = &batch.mutations[position] else {
                    unreachable!("matched conversation recall manifest put")
                };
                let pending = decode_typed_recall_index::<ConversationRecallManifest>(
                    &manifest_key,
                    value.clone(),
                )?;
                (None, pending.revision, pending.entries)
            } else {
                let (previous, before) =
                    self.load_typed_recall_index::<ConversationRecallManifest>(&manifest_key)?;
                if let Some(previous) = previous.as_ref() {
                    self.validate_conversation_manifest_subject(previous, &batch.scope.subject_id)?;
                }
                let revision = previous
                    .as_ref()
                    .map(|index| index.revision.saturating_add(1))
                    .unwrap_or(1);
                let entries = previous
                    .as_ref()
                    .map(|index| index.entries.clone())
                    .unwrap_or_default();
                (before, revision, entries)
            };

            for (owner_key, value) in owners {
                let address = RecallIndexAddress::json(
                    "conversation_transcript_derived_ref",
                    &owner_key,
                    next_entry_revision(
                        &entries,
                        RecallIndexAddressKind::Json,
                        "conversation_transcript_derived_ref",
                        &owner_key,
                    ),
                    canonical_updated_at,
                    &value,
                )?;
                entries = replace_recall_index_address(&entries, address);
            }
            let next = ConversationRecallManifest::build(
                revision,
                &conversation_key.memory_space_id,
                &batch.scope.subject_id,
                &conversation_key.channel_id,
                &conversation_key.conversation_id,
                entries,
            )?;
            let next_value = serde_json::to_value(next).map_err(|error| {
                Error::config("conversation_recall_manifest", error.to_string())
            })?;
            if let Some(position) = existing_position {
                let StoreMutation::PutJson { value, .. } = &mut batch.mutations[position] else {
                    unreachable!("matched conversation recall manifest put")
                };
                *value = next_value;
            } else {
                preconditions.push(match before {
                    Some(value) => StoreJsonPrecondition::Exact {
                        namespace: ConversationRecallManifest::NAMESPACE.to_string(),
                        key: manifest_key.clone(),
                        value,
                    },
                    None => StoreJsonPrecondition::Absent {
                        namespace: ConversationRecallManifest::NAMESPACE.to_string(),
                        key: manifest_key.clone(),
                    },
                });
                batch.mutations.push(StoreMutation::PutJson {
                    namespace: ConversationRecallManifest::NAMESPACE.to_string(),
                    key: manifest_key.clone(),
                    value: next_value,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: ConversationRecallManifest::NAMESPACE.to_string(),
                    record_key: manifest_key,
                });
            }
        }
        Ok(())
    }

    fn load_conversation_recall_manifest(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
    ) -> Result<(
        String,
        Option<ConversationRecallManifest>,
        Option<serde_json::Value>,
    )> {
        let physical_key = ConversationRecallManifest::build(
            1,
            &key.memory_space_id,
            mounted_subject_id,
            &key.channel_id,
            &key.conversation_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (manifest, value) =
            self.load_typed_recall_index::<ConversationRecallManifest>(&physical_key)?;
        Ok((physical_key, manifest, value))
    }

    fn validate_conversation_manifest_subject(
        &self,
        manifest: &ConversationRecallManifest,
        mounted_subject_id: &str,
    ) -> Result<()> {
        if manifest.mounted_subject_id != mounted_subject_id {
            return Err(Error::config(
                "conversation_recall_manifest",
                "conversation manifest subject differs from the requested owner",
            ));
        }
        for entry in &manifest.entries {
            let owner_subject = match entry.namespace.as_str() {
                "conversation_transcript" => self
                    .engine
                    .get_json_value(&entry.namespace, &entry.key)?
                    .map(|value| {
                        serde_json::from_value::<TranscriptTurnRecord>(value)
                            .map(|record| record.subject)
                    })
                    .transpose()
                    .map_err(|error| {
                        Error::config("conversation_recall_manifest", error.to_string())
                    })?,
                "conversation_transcript_derived_ref" => self
                    .engine
                    .get_json_value(&entry.namespace, &entry.key)?
                    .map(|value| {
                        serde_json::from_value::<DerivedMemoryRef>(value).and_then(|derived| {
                            derived
                                .subject_id
                                .or(derived.source.subject_id)
                                .ok_or_else(|| {
                                    serde::de::Error::custom(
                                        "derived memory ref has no subject owner",
                                    )
                                })
                        })
                    })
                    .transpose()
                    .map_err(|error| {
                        Error::config("conversation_recall_manifest", error.to_string())
                    })?,
                "conversation_transcript_attr" => self
                    .engine
                    .get_json_value(&entry.namespace, &entry.key)?
                    .map(|value| {
                        serde_json::from_value::<TranscriptAttrEnvelope>(value).and_then(|attr| {
                            let turn_key = transcript_turn_storage_key(
                                &attr.target.key,
                                mounted_subject_id,
                                &attr.target.turn_id,
                            );
                            self.engine
                                .get_json_value("conversation_transcript", &turn_key)
                                .map_err(serde::de::Error::custom)?
                                .ok_or_else(|| {
                                    serde::de::Error::custom(
                                        "transcript attr target owner is missing",
                                    )
                                })
                                .and_then(|turn| {
                                    serde_json::from_value::<TranscriptTurnRecord>(turn)
                                        .map(|record| record.subject)
                                })
                        })
                    })
                    .transpose()
                    .map_err(|error| {
                        Error::config("conversation_recall_manifest", error.to_string())
                    })?,
                _ => {
                    return Err(Error::config(
                        "conversation_recall_manifest",
                        "conversation manifest contains a non-conversation owner namespace",
                    ));
                }
            };
            let owner_subject = owner_subject.ok_or_else(|| {
                Error::config(
                    "conversation_recall_manifest",
                    "conversation manifest owner is missing",
                )
            })?;
            if owner_subject != mounted_subject_id {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "conversation manifest owner subject differs from its root scope",
                ));
            }
        }
        Ok(())
    }

    fn plan_conversation_index(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        previous: Option<&ConversationRecallManifest>,
        before: Option<serde_json::Value>,
        entries: Vec<RecallIndexAddress>,
    ) -> Result<RecallIndexMutationPlan> {
        let next = ConversationRecallManifest::build(
            previous
                .map(|manifest| manifest.revision.saturating_add(1))
                .unwrap_or(1),
            &key.memory_space_id,
            mounted_subject_id,
            &key.channel_id,
            &key.conversation_id,
            entries,
        )?;
        let physical_key = next.physical_key.clone();
        Ok((
            ConversationRecallManifest::NAMESPACE,
            physical_key,
            serde_json::to_value(next).map_err(|error| {
                Error::config("conversation_recall_manifest", error.to_string())
            })?,
            before,
        ))
    }

    fn json_put<T>(&self, namespace: &str, key: &str, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        self.json_put_with_event_kind(namespace, key, value, MemoryStoreEventKind::MemoryWrite)
    }

    fn json_put_with_event_kind<T>(
        &self,
        namespace: &str,
        key: &str,
        value: &T,
        event_kind: MemoryStoreEventKind,
    ) -> Result<()>
    where
        T: Serialize,
    {
        self.json_put_with_event_kind_and_record_key(namespace, key, key, value, event_kind)
    }

    fn json_put_with_event_kind_and_record_key<T>(
        &self,
        namespace: &str,
        key: &str,
        record_key: &str,
        value: &T,
        event_kind: MemoryStoreEventKind,
    ) -> Result<()>
    where
        T: Serialize,
    {
        let value = serde_json::to_value(value)
            .map_err(|error| Error::config("store_json_encode", error.to_string()))?;
        let content_hash = stable_hash_json(&value)?;
        let event = self.build_memory_event(event_kind, namespace, record_key, content_hash);
        let admission = self.current_store_transaction_admission()?;
        self.engine
            .put_json_value_and_event(namespace, key, value, event, &admission)
    }

    fn json_delete(&self, namespace: &str, key: &str) -> Result<bool> {
        self.json_delete_with_event_kind(namespace, key, MemoryStoreEventKind::MemoryDelete)
    }

    fn json_delete_with_event_kind(
        &self,
        namespace: &str,
        key: &str,
        event_kind: MemoryStoreEventKind,
    ) -> Result<bool> {
        self.json_delete_with_event_kind_and_record_key(namespace, key, key, event_kind)
    }

    fn json_delete_with_event_kind_and_record_key(
        &self,
        namespace: &str,
        key: &str,
        record_key: &str,
        event_kind: MemoryStoreEventKind,
    ) -> Result<bool> {
        let _transaction_guard = self.lock_transaction("store_json_delete")?;
        enforce_logical_key_budget(self.capacity, namespace, key, "store_json_delete")?;
        let event_template = self.build_memory_event_template(event_kind, namespace, record_key);
        let admission = self.current_store_transaction_admission()?;
        self.engine.delete_json_value_and_materialize_event(
            namespace,
            key,
            event_template,
            &admission,
        )
    }

    fn json_list<T>(&self, namespace: &str, limit: usize) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let mut values = Vec::new();
        for key in self.engine.list_json_keys(namespace)? {
            if limit > 0 && values.len() >= limit {
                break;
            }
            if let Some(value) = self.json_get(namespace, &key)? {
                values.push(value);
            }
        }
        Ok(values)
    }

    fn blob_put(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        let content_hash = stable_hash_hex(value);
        let event = self.build_memory_event(
            MemoryStoreEventKind::MemoryWrite,
            namespace,
            key,
            content_hash,
        );
        let admission = self.current_store_transaction_admission()?;
        self.engine
            .put_blob_and_event(namespace, key, value, event, &admission)
    }

    fn blob_delete(&self, namespace: &str, key: &str) -> Result<bool> {
        let _transaction_guard = self.lock_transaction("store_blob_delete")?;
        enforce_logical_key_budget(self.capacity, namespace, key, "store_blob_delete")?;
        let event_template =
            self.build_memory_event_template(MemoryStoreEventKind::MemoryDelete, namespace, key);
        let admission = self.current_store_transaction_admission()?;
        self.engine
            .delete_blob_and_materialize_event(namespace, key, event_template, &admission)
    }

    fn build_memory_event(
        &self,
        kind: MemoryStoreEventKind,
        plane: &str,
        record_key: &str,
        content_hash: String,
    ) -> MemoryStoreEvent {
        MemoryStoreEvent::new(
            next_event_id(),
            kind,
            self.config.event_scope.clone(),
            current_unix_secs(),
        )
        .with_plane(plane)
        .with_record_key(record_key)
        .with_content_hash(content_hash)
    }

    fn build_memory_event_template(
        &self,
        kind: MemoryStoreEventKind,
        plane: &str,
        record_key: &str,
    ) -> ConditionalDeleteEventTemplate {
        StoreEngineMutation::conditional_delete_event_template(
            next_event_id(),
            kind,
            self.config.event_scope.clone(),
            current_unix_secs(),
        )
        .with_plane(plane)
        .with_record_key(record_key)
    }

    fn current_store_transaction_admission(&self) -> Result<StoreTransactionAdmission> {
        let runtime_budget = self.current_runtime_budget(current_unix_secs());
        if runtime_budget.resource_snapshot.stale {
            return Err(Error::config(
                "memory_write_transaction_resource_admission",
                "store transaction requires a fresh runtime budget report",
            ));
        }
        self.store_transaction_admission_for_report(&runtime_budget)
    }

    fn store_transaction_admission_for_report(
        &self,
        runtime_budget: &RuntimeBudgetReport,
    ) -> Result<StoreTransactionAdmission> {
        let now_secs = current_unix_secs();
        runtime_budget.validate_for_admission(now_secs)?;
        let owner_report = crate::RuntimeBudgetLease::active_report(&self.runtime_budget_authority)
            .unwrap_or_else(|| self.runtime_budget_authority.current_report(now_secs));
        if owner_report.report_id != runtime_budget.report_id {
            return Err(Error::config(
                "memory_write_transaction_resource_admission",
                "runtime budget report was not issued by the current store authority boundary",
            ));
        }
        StoreTransactionAdmission::from_runtime_budget(
            runtime_budget,
            Arc::clone(&self.runtime_budget_authority),
            self.engine.admission_authority(),
        )
    }

    fn build_batch_event(
        &self,
        batch: &StoreMutationBatch,
        transaction_timestamp: u64,
        kind: MemoryStoreEventKind,
        plane: &str,
        record_key: &str,
        content_hash: String,
    ) -> MemoryStoreEvent {
        MemoryStoreEvent::new(
            next_event_id(),
            kind,
            batch.scope.clone(),
            transaction_timestamp,
        )
        .with_plane(plane)
        .with_record_key(record_key)
        .with_content_hash(content_hash)
        .with_payload("transaction_id", batch.transaction_id.as_str())
        .with_payload("operation", batch.operation.as_str())
    }

    fn build_batch_event_template(
        &self,
        batch: &StoreMutationBatch,
        transaction_timestamp: u64,
        kind: MemoryStoreEventKind,
        plane: &str,
        record_key: &str,
    ) -> ConditionalDeleteEventTemplate {
        StoreEngineMutation::conditional_delete_event_template(
            next_event_id(),
            kind,
            batch.scope.clone(),
            transaction_timestamp,
        )
        .with_plane(plane)
        .with_record_key(record_key)
        .with_payload("transaction_id", batch.transaction_id.as_str())
        .with_payload("operation", batch.operation.as_str())
    }

    fn emit_runtime_event(&self, operation: &str) -> Result<()> {
        let event = MemoryStoreEvent::new(
            next_event_id(),
            MemoryStoreEventKind::RuntimeLifecycle,
            StoreEventScope::system(operation),
            current_unix_secs(),
        )
        .with_payload("backend", self.config.backend.as_str())
        .with_payload("profile", self.config.profile.as_str())
        .with_payload("result", "ok");
        self.append_validated_event(event)
    }

    fn append_validated_event(&self, event: MemoryStoreEvent) -> Result<()> {
        let _transaction_guard = self.lock_transaction("store_event_log")?;
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&self.runtime_budget_authority))?;
        lease.execute(&self.runtime_budget_authority, || {
            let admission = self.store_transaction_admission_for_report(lease.report())?;
            let request = StoreTransactionRequest::new(
                format!("{}:event-only", event.event_id),
                Vec::new(),
                vec![StoreEngineMutation::AppendEvent {
                    event: Box::new(event),
                }],
                None,
            );
            self.engine
                .commit_transaction_admitted(&request, &admission)
                .map(|_| ())
        })
    }

    fn enforce_snapshot_budget(
        &self,
        snapshot: &StoreSnapshot,
        max_bytes: usize,
        operation: &'static str,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(snapshot)
            .map_err(|error| Error::config("store_snapshot_budget", error.to_string()))?;
        if bytes.len() > max_bytes {
            return Err(store_budget_error(format!(
                "{operation} snapshot bytes {} exceed {}",
                bytes.len(),
                max_bytes
            )));
        }
        Ok(())
    }
}

fn validate_protected_json_mutation_preconditions(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    const PROTECTED_NAMESPACES: &[&str] = &[
        "long_term",
        GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
        GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
        MEMORY_FACET_INDEX_NAMESPACE,
        MEMORY_FACET_POSTING_NAMESPACE,
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
        "memory_graph_manifests",
        CONVERSATION_RECALL_MANIFEST_NAMESPACE,
        ARCHIVE_RECALL_MANIFEST_NAMESPACE,
        RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE,
        CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE,
        ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE,
        TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
        RECALL_OWNER_SCOPE_BINDING_NAMESPACE,
    ];
    let mut by_address = BTreeMap::<(String, String), &StoreJsonPrecondition>::new();
    for precondition in preconditions {
        let address = match precondition {
            StoreJsonPrecondition::Absent { namespace, key }
            | StoreJsonPrecondition::Exact { namespace, key, .. } => {
                (namespace.clone(), key.clone())
            }
        };
        if let Some(existing) = by_address.insert(address.clone(), precondition) {
            if existing != precondition {
                return Err(Error::config(
                    "memory_write_transaction_precondition_conflict",
                    format!(
                        "conflicting JSON preconditions for namespace {}, key {}",
                        address.0, address.1
                    ),
                ));
            }
        }
    }
    for mutation in &batch.mutations {
        let (namespace, key) = match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. } => (namespace, key),
            _ => continue,
        };
        if PROTECTED_NAMESPACES.contains(&namespace.as_str())
            && !by_address.contains_key(&(namespace.clone(), key.clone()))
        {
            return Err(Error::config(
                "memory_write_transaction_precondition_missing",
                format!(
                    "protected JSON mutation requires a read-set precondition for namespace {namespace}, key {key}"
                ),
            ));
        }
    }
    Ok(())
}

fn validate_recall_index_mutation_closure(
    batch: &StoreMutationBatch,
    read_before_json: impl Fn(&str, &str) -> Result<Option<serde_json::Value>>,
    read_before_blob: impl Fn(&str, &str) -> Result<Option<Vec<u8>>>,
) -> Result<()> {
    let index_namespaces = [
        CONVERSATION_RECALL_MANIFEST_NAMESPACE,
        ARCHIVE_RECALL_MANIFEST_NAMESPACE,
        RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE,
        CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE,
        ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE,
        TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
    ];
    let mut indexes = BTreeMap::<(String, String), Vec<RecallIndexAddress>>::new();
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } if index_namespaces.contains(&namespace.as_str()) => {
                if indexes.contains_key(&(namespace.clone(), key.clone())) {
                    return Err(Error::config(
                        "recall_index_mutation_closure",
                        format!("duplicate typed recall index mutation for {namespace}"),
                    ));
                }
                let entries = match namespace.as_str() {
                    CONVERSATION_RECALL_MANIFEST_NAMESPACE => {
                        let index = decode_typed_recall_index::<ConversationRecallManifest>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id
                            || index.mounted_subject_id != batch.scope.subject_id
                            || index.channel_id != batch.scope.channel
                            || batch.scope.conversation_id.as_deref()
                                != Some(index.conversation_id.as_str())
                        {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "conversation index scope differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    ARCHIVE_RECALL_MANIFEST_NAMESPACE => {
                        let index =
                            decode_typed_recall_index::<ArchiveRecallManifest>(key, value.clone())?;
                        if index.memory_space_id != batch.scope.memory_space_id
                            || index.mounted_subject_id != batch.scope.subject_id
                        {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "archive index scope differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE => {
                        let index = decode_typed_recall_index::<RuntimeSkillRecallManifest>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id
                            || index.agent_id != batch.scope.agent_id
                        {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "runtime skill index scope differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE => {
                        let index = decode_typed_recall_index::<ContinuityCapsuleScopeIndex>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "continuity index memory space differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE => {
                        let index = decode_typed_recall_index::<ActiveTaskRunByChatIndex>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "task-run index memory space differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE => {
                        let index = decode_typed_recall_index::<TaskLearningByChatIndex>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "task-learning index memory space differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    _ => unreachable!("guarded recall index namespace"),
                };
                indexes.insert((namespace.clone(), key.clone()), entries);
            }
            StoreMutation::DeleteJson { namespace, .. }
                if index_namespaces.contains(&namespace.as_str()) =>
            {
                return Err(Error::config(
                    "recall_index_mutation_closure",
                    "typed recall index roots must remain explicit when empty",
                ));
            }
            _ => {}
        }
    }

    for mutation in &batch.mutations {
        let (
            owner_kind,
            owner_namespace,
            owner_key,
            expected_index,
            expected_root,
            expected_digest,
            must_exist,
            previous_root,
        ) = match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } => {
                let Some(expected_index) = recall_index_namespace_for_json_owner(namespace) else {
                    continue;
                };
                let (expected_root, must_exist, previous_root) = expected_json_owner_recall_roots(
                    batch,
                    namespace,
                    key,
                    Some(value),
                    read_before_json(namespace, key)?.as_ref(),
                )?;
                let digest = RecallIndexAddress::json(namespace, key, 1, 0, value)?.content_sha256;
                (
                    RecallIndexAddressKind::Json,
                    namespace.as_str(),
                    key.as_str(),
                    expected_index,
                    expected_root,
                    Some(digest),
                    must_exist,
                    previous_root,
                )
            }
            StoreMutation::DeleteJson { namespace, key, .. } => {
                let Some(expected_index) = recall_index_namespace_for_json_owner(namespace) else {
                    continue;
                };
                let before = read_before_json(namespace, key)?;
                let (expected_root, must_exist, previous_root) =
                    expected_json_owner_recall_roots(batch, namespace, key, None, before.as_ref())?;
                (
                    RecallIndexAddressKind::Json,
                    namespace.as_str(),
                    key.as_str(),
                    expected_index,
                    expected_root,
                    None,
                    must_exist,
                    previous_root,
                )
            }
            StoreMutation::PutBlob {
                namespace,
                key,
                value,
                ..
            } => {
                let Some(expected_index) = recall_index_namespace_for_blob_owner(namespace) else {
                    continue;
                };
                let digest = RecallIndexAddress::blob(namespace, key, 1, 0, value)?.content_sha256;
                let expected_root = expected_blob_owner_recall_root(batch, namespace)?;
                (
                    RecallIndexAddressKind::Blob,
                    namespace.as_str(),
                    key.as_str(),
                    expected_index,
                    expected_root,
                    Some(digest),
                    true,
                    None,
                )
            }
            StoreMutation::DeleteBlob { namespace, key, .. } => {
                let Some(expected_index) = recall_index_namespace_for_blob_owner(namespace) else {
                    continue;
                };
                let expected_root = expected_blob_owner_recall_root(batch, namespace)?;
                let previous_root = expected_root.clone();
                (
                    RecallIndexAddressKind::Blob,
                    namespace.as_str(),
                    key.as_str(),
                    expected_index,
                    expected_root,
                    None,
                    false,
                    Some(previous_root),
                )
            }
            StoreMutation::AppendEvent { .. } => continue,
        };
        let entries = indexes
            .get(&(expected_index.to_string(), expected_root.clone()))
            .ok_or_else(|| {
            Error::config(
                "recall_index_mutation_closure",
                format!("owner mutation {owner_namespace}/{owner_key} lacks exact root {expected_index}/{expected_root}"),
            )
        })?;
        let matches = entries
            .iter()
            .filter(|entry| {
                entry.kind == owner_kind
                    && entry.namespace == owner_namespace
                    && entry.key == owner_key
            })
            .collect::<Vec<_>>();
        if must_exist {
            if matches.len() != 1
                || expected_digest
                    .as_ref()
                    .is_none_or(|digest| matches[0].content_sha256 != *digest)
            {
                return Err(Error::config(
                    "recall_index_mutation_closure",
                    format!("owner mutation {owner_namespace}/{owner_key} is not exactly bound"),
                ));
            }
        } else if !matches.is_empty() {
            return Err(Error::config(
                "recall_index_mutation_closure",
                format!("removed/inactive owner {owner_namespace}/{owner_key} remains indexed"),
            ));
        }
        if let Some(previous_root) = previous_root.filter(|root| root != &expected_root) {
            let old_entries = indexes
                .get(&(expected_index.to_string(), previous_root.clone()))
                .ok_or_else(|| {
                    Error::config(
                        "recall_index_mutation_closure",
                        format!("scope transfer for {owner_namespace}/{owner_key} omits old root {previous_root}"),
                    )
                })?;
            if old_entries.iter().any(|entry| {
                entry.kind == owner_kind
                    && entry.namespace == owner_namespace
                    && entry.key == owner_key
            }) {
                return Err(Error::config(
                    "recall_index_mutation_closure",
                    format!("scope transfer for {owner_namespace}/{owner_key} remains in old root"),
                ));
            }
        }
        let containing_roots = indexes
            .iter()
            .filter(|((namespace, _), entries)| {
                namespace == expected_index
                    && entries.iter().any(|entry| {
                        entry.kind == owner_kind
                            && entry.namespace == owner_namespace
                            && entry.key == owner_key
                    })
            })
            .map(|((_, root), _)| root)
            .collect::<Vec<_>>();
        if must_exist {
            if containing_roots != vec![&expected_root] {
                return Err(Error::config(
                    "recall_index_mutation_closure",
                    format!("owner {owner_namespace}/{owner_key} is bound to a wrong or duplicate typed root"),
                ));
            }
        } else if !containing_roots.is_empty() {
            return Err(Error::config(
                "recall_index_mutation_closure",
                format!(
                    "removed/inactive owner {owner_namespace}/{owner_key} remains in a typed root"
                ),
            ));
        }
    }
    let scope =
        StoreScopedProjectionScope::new(&batch.scope.memory_space_id, &batch.scope.subject_id)?;
    for mutation in &batch.mutations {
        let StoreMutation::PutJson {
            namespace,
            key,
            value,
            ..
        } = mutation
        else {
            continue;
        };
        if !index_namespaces.contains(&namespace.as_str()) {
            continue;
        }
        crate::store_internal::transaction::validate_typed_recall_manifest_closure(
            namespace,
            key,
            value,
            &scope,
            |owner_namespace, owner_key| {
                transaction_post_json_value(batch, owner_namespace, owner_key, &read_before_json)
            },
            |owner_namespace, owner_key| {
                transaction_post_blob_value(batch, owner_namespace, owner_key, &read_before_blob)
            },
        )?;
    }
    Ok(())
}

fn transaction_post_json_value(
    batch: &StoreMutationBatch,
    namespace: &str,
    key: &str,
    read_before: &impl Fn(&str, &str) -> Result<Option<serde_json::Value>>,
) -> Result<Option<serde_json::Value>> {
    for mutation in batch.mutations.iter().rev() {
        match mutation {
            StoreMutation::PutJson {
                namespace: candidate_namespace,
                key: candidate_key,
                value,
                ..
            } if candidate_namespace == namespace && candidate_key == key => {
                return Ok(Some(value.clone()));
            }
            StoreMutation::DeleteJson {
                namespace: candidate_namespace,
                key: candidate_key,
                ..
            } if candidate_namespace == namespace && candidate_key == key => return Ok(None),
            _ => {}
        }
    }
    read_before(namespace, key)
}

fn transaction_post_blob_value(
    batch: &StoreMutationBatch,
    namespace: &str,
    key: &str,
    read_before: &impl Fn(&str, &str) -> Result<Option<Vec<u8>>>,
) -> Result<Option<Vec<u8>>> {
    for mutation in batch.mutations.iter().rev() {
        match mutation {
            StoreMutation::PutBlob {
                namespace: candidate_namespace,
                key: candidate_key,
                value,
                ..
            } if candidate_namespace == namespace && candidate_key == key => {
                return Ok(Some(value.clone()));
            }
            StoreMutation::DeleteBlob {
                namespace: candidate_namespace,
                key: candidate_key,
                ..
            } if candidate_namespace == namespace && candidate_key == key => return Ok(None),
            _ => {}
        }
    }
    read_before(namespace, key)
}

fn expected_json_owner_recall_roots(
    batch: &StoreMutationBatch,
    namespace: &str,
    key: &str,
    next: Option<&serde_json::Value>,
    before: Option<&serde_json::Value>,
) -> Result<(String, bool, Option<String>)> {
    let root_for = |value: &serde_json::Value| -> Result<(String, bool)> {
        match namespace {
            "conversation_transcript" => {
                let record = serde_json::from_value::<TranscriptTurnRecord>(value.clone())
                    .map_err(|error| {
                        Error::config("recall_index_mutation_closure", error.to_string())
                    })?;
                Ok((
                    ConversationRecallManifest::build(
                        1,
                        &record.key.memory_space_id,
                        &record.subject,
                        &record.key.channel_id,
                        &record.key.conversation_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            "conversation_transcript_attr" => {
                let attr = serde_json::from_value::<TranscriptAttrEnvelope>(value.clone())
                    .map_err(|error| {
                        Error::config("recall_index_mutation_closure", error.to_string())
                    })?;
                Ok((
                    ConversationRecallManifest::build(
                        1,
                        &attr.target.key.memory_space_id,
                        &batch.scope.subject_id,
                        &attr.target.key.channel_id,
                        &attr.target.key.conversation_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            "conversation_transcript_derived_ref" => {
                let derived =
                    serde_json::from_value::<DerivedMemoryRef>(value.clone()).map_err(|error| {
                        Error::config("recall_index_mutation_closure", error.to_string())
                    })?;
                let subject_id = exact_derived_owner_subject(&derived)?;
                Ok((
                    ConversationRecallManifest::build(
                        1,
                        &derived.source.memory_space_id,
                        subject_id,
                        &derived.source.channel_id,
                        &derived.source.conversation_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            "conversation_transcript_alias" => {
                let alias = serde_json::from_value::<TranscriptConversationAlias>(value.clone())
                    .map_err(|error| {
                        Error::config("recall_index_mutation_closure", error.to_string())
                    })?;
                Ok((
                    ArchiveRecallManifest::build(
                        1,
                        &alias.memory_space_id,
                        &alias.mounted_subject_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            "session" | "session_summary" | "active_work" | "turn_ledger" => Ok((
                ArchiveRecallManifest::build(
                    1,
                    &batch.scope.memory_space_id,
                    &batch.scope.subject_id,
                    std::iter::empty(),
                )?
                .physical_key,
                true,
            )),
            "continuity_capsule" => {
                let capsule = serde_json::from_value::<ContinuityCapsule>(value.clone()).map_err(
                    |error| Error::config("recall_index_mutation_closure", error.to_string()),
                )?;
                Ok((
                    ContinuityCapsuleScopeIndex::build(
                        1,
                        &batch.scope.memory_space_id,
                        capsule.scope_kind.label(),
                        &capsule.scope_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            "task_run" => {
                let record =
                    serde_json::from_value::<TaskRunRecord>(value.clone()).map_err(|error| {
                        Error::config("recall_index_mutation_closure", error.to_string())
                    })?;
                Ok((
                    ActiveTaskRunByChatIndex::build(
                        1,
                        &batch.scope.memory_space_id,
                        &record.run.source_channel,
                        &record.run.source_chat_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    record.run.status.is_active(),
                ))
            }
            "task_learning" => {
                let record = serde_json::from_value::<TaskLearningRecord>(value.clone()).map_err(
                    |error| Error::config("recall_index_mutation_closure", error.to_string()),
                )?;
                Ok((
                    TaskLearningByChatIndex::build(
                        1,
                        &batch.scope.memory_space_id,
                        &record.source_channel,
                        &record.source_chat_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            _ => Err(Error::config(
                "recall_index_mutation_closure",
                format!("unsupported indexed JSON owner {namespace}/{key}"),
            )),
        }
    };
    let next_root = next.map(root_for).transpose()?;
    let before_root = before.map(root_for).transpose()?.map(|(root, _)| root);
    match next_root {
        Some((root, must_exist)) => Ok((root, must_exist, before_root)),
        None => before_root
            .clone()
            .map(|root| (root, false, before_root))
            .ok_or_else(|| {
                Error::config(
                    "recall_index_mutation_closure",
                    format!("delete owner {namespace}/{key} has no typed before-image"),
                )
            }),
    }
}

fn expected_blob_owner_recall_root(batch: &StoreMutationBatch, namespace: &str) -> Result<String> {
    match namespace {
        "daily" | "memory" => Ok(ArchiveRecallManifest::build(
            1,
            &batch.scope.memory_space_id,
            &batch.scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key),
        "skills" => Ok(RuntimeSkillRecallManifest::build(
            1,
            &batch.scope.memory_space_id,
            &batch.scope.agent_id,
            std::iter::empty(),
        )?
        .physical_key),
        _ => Err(Error::config(
            "recall_index_mutation_closure",
            format!("unsupported indexed blob owner {namespace}"),
        )),
    }
}

fn recall_index_namespace_for_json_owner(namespace: &str) -> Option<&'static str> {
    match namespace {
        "conversation_transcript"
        | "conversation_transcript_attr"
        | "conversation_transcript_derived_ref" => Some(CONVERSATION_RECALL_MANIFEST_NAMESPACE),
        "session"
        | "session_summary"
        | "active_work"
        | "turn_ledger"
        | "conversation_transcript_alias" => Some(ARCHIVE_RECALL_MANIFEST_NAMESPACE),
        "continuity_capsule" => Some(CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE),
        "task_run" => Some(ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE),
        "task_learning" => Some(TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE),
        _ => None,
    }
}

fn recall_index_namespace_for_blob_owner(namespace: &str) -> Option<&'static str> {
    match namespace {
        "daily" | "memory" => Some(ARCHIVE_RECALL_MANIFEST_NAMESPACE),
        "skills" => Some(RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE),
        _ => None,
    }
}

fn validate_evidence_effect_address_closure(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    let mut actual_addresses = BTreeSet::new();
    let mut expected_addresses = BTreeSet::new();
    let mut owner_ids = BTreeSet::new();

    for mutation in &batch.mutations {
        let (namespace, key) = match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. }
                if matches!(
                    namespace.as_str(),
                    GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
                        | GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE
                        | GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE
                ) =>
            {
                (namespace, key)
            }
            _ => continue,
        };
        if !actual_addresses.insert((namespace.clone(), key.clone())) {
            return Err(Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                format!("duplicate evidence mutation effect address {namespace}/{key}"),
            ));
        }

        if namespace != GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE {
            continue;
        }
        let document = evidence_owner_document_for_mutation(mutation, preconditions)?;
        validate_governed_evidence_document(&document).map_err(|error| {
            Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                format!("invalid evidence owner mutation: {error:?}"),
            )
        })?;
        if document.memory_space_id != batch.scope.memory_space_id
            || document.mounted_subject_id != batch.scope.subject_id
        {
            return Err(Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                "evidence owner mutation scope does not match transaction scope",
            ));
        }
        let (mutation_key, plane, record_key) = match mutation {
            StoreMutation::PutJson {
                key,
                plane,
                record_key,
                ..
            }
            | StoreMutation::DeleteJson {
                key,
                plane,
                record_key,
                ..
            } => (key, plane, record_key),
            _ => unreachable!("evidence owner closure only accepts JSON mutations"),
        };
        let owner_ref = GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::EvidenceDocument,
            document.document_id.clone(),
        );
        if plane != GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
            || record_key != &document.document_id
            || mutation_key != &document.physical_key
            || !owner_ids.insert(document.document_id.clone())
        {
            return Err(Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                "evidence owner mutation metadata or logical identity is not exact",
            ));
        }

        let owner_key = scoped_governed_evidence_document_key(
            &batch.scope.memory_space_id,
            &document.document_id,
        )?;
        expected_addresses.insert((GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(), owner_key));
        let source_ref_key = governed_evidence_source_ref_from_document(&document)?.physical_key;
        expected_addresses.insert((
            GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
            source_ref_key,
        ));
        if matches!(mutation, StoreMutation::PutJson { .. }) {
            if let Some(before_document) = evidence_owner_before_document_for_scope(
                &document.physical_key,
                &batch.scope.memory_space_id,
                &batch.scope.subject_id,
                &owner_ref,
                preconditions,
            )? {
                let before_source_ref_key =
                    governed_evidence_source_ref_from_document(&before_document)?.physical_key;
                expected_addresses.insert((
                    GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                    before_source_ref_key,
                ));
            }
        }
    }

    if !owner_ids.is_empty() {
        expected_addresses.insert((
            GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
            governed_evidence_source_claim_manifest_key(
                &batch.scope.memory_space_id,
                &batch.scope.subject_id,
            )?,
        ));
    }

    if actual_addresses == expected_addresses {
        return Ok(());
    }
    let missing = expected_addresses
        .difference(&actual_addresses)
        .map(|(namespace, key)| format!("{namespace}/{key}"))
        .collect::<Vec<_>>();
    let extra = actual_addresses
        .difference(&expected_addresses)
        .map(|(namespace, key)| format!("{namespace}/{key}"))
        .collect::<Vec<_>>();
    Err(Error::config(
        "memory_write_transaction_evidence_source_ref_closure_invalid",
        format!(
            "evidence mutation effect address closure drift: missing [{}], extra [{}]",
            missing.join(","),
            extra.join(",")
        ),
    ))
}

fn evidence_owner_before_document_for_scope(
    physical_key: &str,
    memory_space_id: &str,
    mounted_subject_id: &str,
    owner_ref: &GovernedMemoryOwnerRef,
    preconditions: &[StoreJsonPrecondition],
) -> Result<Option<GovernedEvidenceDocument>> {
    for precondition in preconditions {
        let StoreJsonPrecondition::Exact {
            namespace,
            key,
            value,
        } = precondition
        else {
            continue;
        };
        if namespace != GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE || key != physical_key {
            continue;
        }
        let document =
            serde_json::from_value::<GovernedEvidenceDocument>(value.clone()).map_err(|error| {
                Error::config(
                    "memory_write_transaction_evidence_source_ref_closure_invalid",
                    format!("invalid evidence owner precondition: {error}"),
                )
            })?;
        validate_governed_evidence_document(&document).map_err(|error| {
            Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                format!("invalid evidence owner precondition: {error:?}"),
            )
        })?;
        let precondition_owner_ref = GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::EvidenceDocument,
            document.document_id.clone(),
        );
        if document.physical_key != *physical_key
            || document.memory_space_id != memory_space_id
            || document.mounted_subject_id != mounted_subject_id
            || precondition_owner_ref != *owner_ref
        {
            return Err(Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                "evidence owner precondition does not match canonical typed owner scope",
            ));
        }
        return Ok(Some(document));
    }
    Ok(None)
}

fn evidence_owner_document_for_mutation(
    mutation: &StoreMutation,
    preconditions: &[StoreJsonPrecondition],
) -> Result<GovernedEvidenceDocument> {
    let value = match mutation {
        StoreMutation::PutJson { value, .. } => value,
        StoreMutation::DeleteJson { namespace, key, .. } => preconditions
            .iter()
            .find_map(|precondition| match precondition {
                StoreJsonPrecondition::Exact {
                    namespace: expected_namespace,
                    key: expected_key,
                    value,
                } if expected_namespace == namespace && expected_key == key => Some(value),
                _ => None,
            })
            .ok_or_else(|| {
                Error::config(
                    "memory_write_transaction_evidence_source_ref_closure_invalid",
                    "evidence owner delete requires an exact typed precondition",
                )
            })?,
        _ => {
            return Err(Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                "evidence owner closure requires a JSON mutation",
            ));
        }
    };
    serde_json::from_value(value.clone()).map_err(|error| {
        Error::config(
            "memory_write_transaction_evidence_source_ref_closure_invalid",
            format!("invalid evidence owner document: {error}"),
        )
    })
}

fn validate_evidence_lifecycle_closure(batch: &StoreMutationBatch) -> Result<()> {
    let owner_mutated = batch.mutations.iter().any(|mutation| {
        matches!(
            mutation,
            StoreMutation::PutJson { namespace, .. }
                | StoreMutation::DeleteJson { namespace, .. }
                if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
        )
    });
    if !owner_mutated {
        return Ok(());
    }

    let lifecycle_events = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::AppendEvent { event } => Some(event),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [event] = lifecycle_events.as_slice() else {
        return Err(Error::config(
            "memory_write_transaction_evidence_lifecycle_closure_invalid",
            "evidence owner mutation requires exactly one lifecycle AppendEvent",
        ));
    };

    let expected_operation = RuntimeLifecycleOperation::Maintain.as_str();
    let expected_effect = RuntimeLifecycleEffect::RunMaintenance.as_str();
    let expected_trigger = RuntimeLifecycleTrigger::SdkCall.as_str();
    let exact_binding = event.kind == MemoryStoreEventKind::RuntimeLifecycle
        && event.kind_name == MemoryStoreEventKind::RuntimeLifecycle.as_str()
        && event.plane == "runtime_lifecycle"
        && event.record_key == expected_operation
        && event.scope == StoreEventScope::system(expected_operation)
        && event.payload.get("runtime_operation").map(String::as_str) == Some(expected_operation)
        && event.payload.get("operation").map(String::as_str) == Some(batch.operation.as_str())
        && event.payload.get("transaction_id").map(String::as_str)
            == Some(batch.transaction_id.as_str())
        && event.payload.get("effect").map(String::as_str) == Some(expected_effect)
        && event.payload.get("trigger").map(String::as_str) == Some(expected_trigger)
        && !event.event_id.trim().is_empty()
        && !event.content_hash.trim().is_empty();
    if exact_binding {
        Ok(())
    } else {
        Err(Error::config(
            "memory_write_transaction_evidence_lifecycle_closure_invalid",
            "evidence owner lifecycle AppendEvent is not exactly bound to the transaction",
        ))
    }
}

pub(crate) fn validate_governed_transaction_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    graph_repair_authorized: bool,
) -> Result<()> {
    validate_evidence_source_ref_post_image(batch, before, after)?;
    validate_facet_post_image(batch, before, after)?;
    validate_graph_post_image(batch, before, after, graph_repair_authorized)?;
    validate_control_post_image(batch, before, after)
}

fn governed_transaction_dependency_json_reads(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<BTreeSet<(String, String)>> {
    let mut documents = Vec::new();
    for mutation in &batch.mutations {
        if let StoreMutation::PutJson {
            namespace,
            key,
            value,
            ..
        } = mutation
        {
            documents.push((namespace.as_str(), key.as_str(), value));
        }
    }
    for precondition in preconditions {
        if let StoreJsonPrecondition::Exact {
            namespace,
            key,
            value,
        } = precondition
        {
            documents.push((namespace.as_str(), key.as_str(), value));
        }
    }

    let mut reads = BTreeSet::new();
    for (namespace, _key, value) in documents {
        match namespace {
            GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE => {
                let manifest = decode_transaction_dependency::<GovernedEvidenceSourceClaimManifest>(
                    value,
                    "evidence source claim manifest",
                )?;
                reads.extend(
                    manifest
                        .owner_keys
                        .into_iter()
                        .map(|key| (GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(), key)),
                );
                reads.extend(
                    manifest
                        .claim_keys
                        .into_iter()
                        .map(|key| (GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(), key)),
                );
            }
            GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE => {
                let owner = decode_transaction_dependency::<GovernedEvidenceDocument>(
                    value,
                    "evidence owner",
                )?;
                let claim = governed_evidence_source_ref_from_document(&owner)?;
                reads.insert((
                    GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                    claim.physical_key,
                ));
            }
            GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE => {
                let claim = decode_transaction_dependency::<GovernedEvidenceSourceRef>(
                    value,
                    "evidence source claim",
                )?;
                reads.insert(governed_owner_storage_address(
                    &batch.scope.memory_space_id,
                    &claim.owner_ref,
                )?);
            }
            MEMORY_FACET_POSTING_NAMESPACE => {
                if let Ok(manifest) =
                    serde_json::from_value::<MemoryFacetIndexManifest>(value.clone())
                {
                    for owner in manifest.owner_versions {
                        reads.insert((
                            MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                            scoped_memory_facet_owner_storage_key(
                                &batch.scope.memory_space_id,
                                &batch.scope.subject_id,
                                &owner.owner_ref,
                            )
                            .map_err(|error| {
                                Error::config(
                                    "memory_write_transaction_dependency_read_set",
                                    format!("facet owner key: {error:?}"),
                                )
                            })?,
                        ));
                        reads.insert(governed_owner_storage_address(
                            &batch.scope.memory_space_id,
                            &owner.owner_ref,
                        )?);
                    }
                    reads.extend(manifest.posting_revisions.into_iter().map(|posting| {
                        (
                            MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                            posting.posting_key,
                        )
                    }));
                }
            }
            MEMORY_FACET_INDEX_NAMESPACE => {
                let facet = decode_transaction_dependency::<MemoryFacetIndexDoc>(
                    value,
                    "memory facet owner",
                )?;
                reads.insert(governed_owner_storage_address(
                    &batch.scope.memory_space_id,
                    &facet.owner_ref,
                )?);
            }
            MEMORY_GRAPH_MANIFEST_NAMESPACE => {
                let manifest = decode_transaction_dependency::<MemoryGraphScopeManifest>(
                    value,
                    "memory graph manifest",
                )?;
                reads.insert((
                    MEMORY_GRAPH_REVISION_NAMESPACE.to_string(),
                    manifest.revision.storage_key,
                ));
                reads.extend(manifest.node_memberships.into_iter().map(|dependency| {
                    (
                        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE.to_string(),
                        dependency.storage_key,
                    )
                }));
                reads.extend(manifest.edge_memberships.into_iter().map(|dependency| {
                    (
                        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE.to_string(),
                        dependency.storage_key,
                    )
                }));
                reads.extend(manifest.backlink_memberships.into_iter().map(|dependency| {
                    (
                        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE.to_string(),
                        dependency.storage_key,
                    )
                }));
                reads.extend(manifest.recall_indexes.into_iter().map(|dependency| {
                    (
                        MEMORY_GRAPH_INDEX_NAMESPACE.to_string(),
                        dependency.storage_key,
                    )
                }));
            }
            MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE => {
                let membership = decode_transaction_dependency::<MemoryGraphNodeMembership>(
                    value,
                    "memory graph node membership",
                )?;
                reads.insert((
                    MEMORY_GRAPH_NODE_NAMESPACE.to_string(),
                    membership.document_key,
                ));
                reads.insert((
                    MEMORY_GRAPH_INDEX_NAMESPACE.to_string(),
                    membership.index_key,
                ));
                reads.extend(
                    membership
                        .backlink_membership_keys
                        .into_iter()
                        .map(|key| (MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE.to_string(), key)),
                );
                reads.insert(governed_owner_storage_address(
                    &batch.scope.memory_space_id,
                    &membership.owner_ref,
                )?);
            }
            MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE => {
                let membership = decode_transaction_dependency::<MemoryGraphEdgeMembership>(
                    value,
                    "memory graph edge membership",
                )?;
                reads.insert((
                    MEMORY_GRAPH_EDGE_NAMESPACE.to_string(),
                    membership.document_key,
                ));
                reads.extend(
                    membership
                        .backlink_membership_keys
                        .into_iter()
                        .map(|key| (MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE.to_string(), key)),
                );
            }
            MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE => {
                let membership = decode_transaction_dependency::<MemoryGraphBacklinkMembership>(
                    value,
                    "memory graph backlink membership",
                )?;
                reads.insert((
                    MEMORY_GRAPH_BACKLINK_NAMESPACE.to_string(),
                    membership.document_key,
                ));
            }
            LONG_TERM_CONTROL_REVISION_NAMESPACE => {
                let revision = decode_transaction_dependency::<LongTermMemoryControlRevision>(
                    value,
                    "long-term control revision",
                )?;
                reads.insert((
                    "long_term".to_string(),
                    scoped_long_term_memory_storage_key(
                        &batch.scope.memory_space_id,
                        &revision.record_id,
                    )?,
                ));
                if let Some(successor) = revision.successor_record_id {
                    reads.insert((
                        "long_term".to_string(),
                        scoped_long_term_memory_storage_key(
                            &batch.scope.memory_space_id,
                            &successor,
                        )?,
                    ));
                }
            }
            LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE => {
                let tombstone = decode_transaction_dependency::<LongTermMemoryTombstone>(
                    value,
                    "long-term control tombstone",
                )?;
                reads.insert((
                    "long_term".to_string(),
                    scoped_long_term_memory_storage_key(
                        &batch.scope.memory_space_id,
                        &tombstone.record_id,
                    )?,
                ));
            }
            _ => {}
        }
    }
    Ok(reads)
}

fn governed_owner_storage_address(
    memory_space_id: &str,
    owner_ref: &GovernedMemoryOwnerRef,
) -> Result<(String, String)> {
    match owner_ref.owner_plane {
        GovernedMemoryOwnerPlane::LongTerm => Ok((
            "long_term".to_string(),
            scoped_long_term_memory_storage_key(memory_space_id, &owner_ref.owner_id)?,
        )),
        GovernedMemoryOwnerPlane::EvidenceDocument => Ok((
            GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
            scoped_governed_evidence_document_key(memory_space_id, &owner_ref.owner_id)?,
        )),
        _ => Err(Error::config(
            "memory_write_transaction_dependency_read_set",
            format!(
                "unsupported governed owner plane {}",
                owner_ref.owner_plane.as_str()
            ),
        )),
    }
}

fn decode_transaction_dependency<T: DeserializeOwned>(
    value: &serde_json::Value,
    label: &str,
) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|error| {
        Error::config(
            "memory_write_transaction_dependency_read_set",
            format!("invalid {label}: {error}"),
        )
    })
}

fn validate_evidence_source_ref_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<()> {
    let stage = "memory_write_transaction_evidence_source_ref_post_image_invalid";
    let mut owner_keys = BTreeSet::new();
    let mut source_ref_keys = BTreeSet::new();
    let mut manifest_touched = false;
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE =>
            {
                owner_keys.insert(key.clone());
            }
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE =>
            {
                source_ref_keys.insert(key.clone());
            }
            StoreMutation::PutJson { namespace, .. }
            | StoreMutation::DeleteJson { namespace, .. }
                if namespace == GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE =>
            {
                manifest_touched = true;
            }
            _ => {}
        }
    }
    if owner_keys.is_empty() && source_ref_keys.is_empty() && !manifest_touched {
        return Ok(());
    }

    for source_ref_key in &source_ref_keys {
        let source_ref_address = (
            GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
            source_ref_key.clone(),
        );
        for image in [before, after] {
            let Some(source_ref) = decode_optional_governed_doc::<GovernedEvidenceSourceRef>(
                image.json.get(&source_ref_address),
                stage,
            )?
            else {
                continue;
            };
            owner_keys.insert(validate_evidence_source_ref_image(
                &source_ref,
                source_ref_key,
                batch,
                image,
                stage,
            )?);
        }
    }

    for owner_key in owner_keys {
        let owner_address = (GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(), owner_key);
        let before_owner = decode_optional_governed_doc::<GovernedEvidenceDocument>(
            before.json.get(&owner_address),
            stage,
        )?;
        let after_owner = decode_optional_governed_doc::<GovernedEvidenceDocument>(
            after.json.get(&owner_address),
            stage,
        )?;

        if let Some(before_owner) = before_owner.as_ref() {
            validate_evidence_owner_source_ref_image(
                before_owner,
                &owner_address.1,
                batch,
                before,
                stage,
            )?;
        }
        if let Some(after_owner) = after_owner.as_ref() {
            validate_evidence_owner_source_ref_image(
                after_owner,
                &owner_address.1,
                batch,
                after,
                stage,
            )?;
        }
    }
    validate_evidence_source_claim_manifest_image(before, batch, false, stage)?;
    validate_evidence_source_claim_manifest_image(after, batch, true, stage)?;
    Ok(())
}

fn validate_evidence_source_claim_manifest_image(
    image: &BackendTransactionState,
    batch: &StoreMutationBatch,
    required: bool,
    stage: &'static str,
) -> Result<()> {
    let manifest_key = governed_evidence_source_claim_manifest_key(
        &batch.scope.memory_space_id,
        &batch.scope.subject_id,
    )?;
    let manifest = decode_optional_governed_doc::<GovernedEvidenceSourceClaimManifest>(
        image.json.get(&(
            GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
            manifest_key,
        )),
        stage,
    )?;
    let Some(manifest) = manifest else {
        return if required {
            Err(Error::config(
                stage,
                "evidence source claim scope manifest is missing",
            ))
        } else {
            Ok(())
        };
    };
    manifest.validate_exact(
        &batch.scope.memory_space_id,
        &batch.scope.subject_id,
        manifest.owner_claim_bindings.clone(),
        manifest.owner_count.max(1),
    )?;
    for owner_key in &manifest.owner_keys {
        let owner = decode_optional_governed_doc::<GovernedEvidenceDocument>(
            image.json.get(&(
                GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                owner_key.clone(),
            )),
            stage,
        )?
        .ok_or_else(|| Error::config(stage, "manifest evidence owner is missing"))?;
        validate_evidence_owner_source_ref_image(&owner, owner_key, batch, image, stage)?;
    }
    for claim_key in &manifest.claim_keys {
        let claim = decode_optional_governed_doc::<GovernedEvidenceSourceRef>(
            image.json.get(&(
                GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                claim_key.clone(),
            )),
            stage,
        )?
        .ok_or_else(|| Error::config(stage, "manifest evidence source claim is missing"))?;
        validate_evidence_source_ref_image(&claim, claim_key, batch, image, stage)?;
    }
    Ok(())
}

fn evidence_source_ref_owner_key(
    source_ref: &GovernedEvidenceSourceRef,
    source_ref_key: &str,
    batch: &StoreMutationBatch,
    stage: &'static str,
) -> Result<String> {
    if source_ref.physical_key != source_ref_key
        || source_ref.memory_space_id != batch.scope.memory_space_id
        || source_ref.mounted_subject_id != batch.scope.subject_id
        || source_ref.owner_ref.owner_plane != GovernedMemoryOwnerPlane::EvidenceDocument
    {
        return Err(Error::config(
            stage,
            "touched evidence source claim does not match canonical typed owner scope",
        ));
    }
    scoped_governed_evidence_document_key(
        &source_ref.memory_space_id,
        &source_ref.owner_ref.owner_id,
    )
    .map_err(|error| Error::config(stage, format!("evidence source claim owner key: {error:?}")))
}

fn validate_evidence_source_ref_image(
    source_ref: &GovernedEvidenceSourceRef,
    source_ref_key: &str,
    batch: &StoreMutationBatch,
    image: &BackendTransactionState,
    stage: &'static str,
) -> Result<String> {
    let owner_key = evidence_source_ref_owner_key(source_ref, source_ref_key, batch, stage)?;
    let owner_address = (
        GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
        owner_key.clone(),
    );
    let owner = decode_optional_governed_doc::<GovernedEvidenceDocument>(
        image.json.get(&owner_address),
        stage,
    )?
    .ok_or_else(|| Error::config(stage, "evidence source claim image owner is missing"))?;
    validate_evidence_owner_source_ref_image(&owner, &owner_key, batch, image, stage)?;
    validate_governed_evidence_source_ref(&owner, source_ref)
        .map_err(|error| Error::config(stage, format!("{error:?}")))?;
    Ok(owner_key)
}

fn validate_evidence_owner_source_ref_image(
    owner: &GovernedEvidenceDocument,
    owner_key: &str,
    batch: &StoreMutationBatch,
    image: &BackendTransactionState,
    stage: &'static str,
) -> Result<()> {
    validate_governed_evidence_document(owner).map_err(|error| {
        Error::config(stage, format!("invalid evidence owner image: {error:?}"))
    })?;
    let owner_ref = GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::EvidenceDocument,
        owner.document_id.clone(),
    );
    let expected_owner_key =
        scoped_governed_evidence_document_key(&owner.memory_space_id, &owner_ref.owner_id)
            .map_err(|error| Error::config(stage, format!("evidence owner key: {error:?}")))?;
    if owner.physical_key != owner_key
        || expected_owner_key != owner_key
        || owner.memory_space_id != batch.scope.memory_space_id
        || owner.mounted_subject_id != batch.scope.subject_id
    {
        return Err(Error::config(
            stage,
            "evidence owner image does not match canonical typed owner scope",
        ));
    }
    let expected_source_ref = governed_evidence_source_ref_from_document(owner)?;
    let source_ref_address = (
        GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
        expected_source_ref.physical_key.clone(),
    );
    let source_ref = decode_optional_governed_doc::<GovernedEvidenceSourceRef>(
        image.json.get(&source_ref_address),
        stage,
    )?
    .ok_or_else(|| {
        Error::config(
            stage,
            "evidence owner image is missing its typed source claim",
        )
    })?;
    if source_ref.owner_ref != owner_ref {
        return Err(Error::config(
            stage,
            "evidence source claim owner ref does not close back to its owner",
        ));
    }
    validate_governed_evidence_source_ref(owner, &source_ref)
        .map_err(|error| Error::config(stage, format!("{error:?}")))
}

fn decode_optional_governed_doc<T: DeserializeOwned>(
    value: Option<&serde_json::Value>,
    stage: &'static str,
) -> Result<Option<T>> {
    value
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| Error::config(stage, error.to_string()))
}

fn governed_image<T: DeserializeOwned>(
    namespace: &str,
    key: &str,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<GovernedDocumentImage<T>> {
    let decode = |value: Option<&serde_json::Value>| {
        value
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| {
                Error::config(
                    "memory_write_transaction_post_image_decode_failed",
                    format!("{namespace}/{key}: {error}"),
                )
            })
    };
    Ok(GovernedDocumentImage {
        physical_key: key.to_string(),
        before: decode(before.json.get(&(namespace.to_string(), key.to_string())))?,
        after: decode(after.json.get(&(namespace.to_string(), key.to_string())))?,
    })
}

fn ensure_post_image_validation(
    stage: &'static str,
    validation: GovernedPostImageValidation,
) -> Result<()> {
    if validation.accepted {
        return Ok(());
    }
    Err(Error::config(stage, validation.failures.join(",")))
}

fn batch_mutates_namespace(batch: &StoreMutationBatch, namespace: &str) -> bool {
    batch.mutations.iter().any(|mutation| match mutation {
        StoreMutation::PutJson {
            namespace: mutated, ..
        }
        | StoreMutation::DeleteJson {
            namespace: mutated, ..
        } => mutated == namespace,
        _ => false,
    })
}

fn batch_json_keys(batch: &StoreMutationBatch, namespace: &str) -> BTreeSet<String> {
    batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace: mutated,
                key,
                ..
            }
            | StoreMutation::DeleteJson {
                namespace: mutated,
                key,
                ..
            } if mutated == namespace => Some(key.clone()),
            _ => None,
        })
        .collect()
}

fn scoped_graph_state_keys(
    state: &BackendTransactionState,
    namespace: &str,
    scope_digest: &str,
) -> BTreeSet<String> {
    let prefix = format!("scope:{scope_digest}:doc:");
    state
        .json
        .keys()
        .filter(|(candidate_namespace, key)| {
            candidate_namespace == namespace && key.starts_with(&prefix)
        })
        .map(|(_, key)| key.clone())
        .collect()
}

fn validate_facet_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<()> {
    let facet_touched = batch_mutates_namespace(batch, "long_term")
        || batch_mutates_namespace(batch, GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE)
        || batch_mutates_namespace(batch, MEMORY_FACET_INDEX_NAMESPACE)
        || batch_mutates_namespace(batch, MEMORY_FACET_POSTING_NAMESPACE);
    if !facet_touched {
        return Ok(());
    }
    let memory_space_id = batch.scope.memory_space_id.as_str();
    let subject_id = batch.scope.subject_id.as_str();
    let manifest_key = memory_facet_manifest_key(memory_space_id, subject_id).map_err(|error| {
        Error::config(
            "memory_write_transaction_owner_facet_post_image_invalid",
            format!("facet manifest key: {error:?}"),
        )
    })?;
    let manifest = governed_image::<MemoryFacetIndexManifest>(
        MEMORY_FACET_POSTING_NAMESPACE,
        &manifest_key,
        before,
        after,
    )?;

    let mut owner_refs = BTreeSet::new();
    let mut posting_keys = BTreeSet::new();
    for value in [manifest.before.as_ref(), manifest.after.as_ref()]
        .into_iter()
        .flatten()
    {
        owner_refs.extend(
            value
                .owner_versions
                .iter()
                .map(|owner| owner.owner_ref.clone()),
        );
        posting_keys.extend(
            value
                .posting_revisions
                .iter()
                .map(|posting| posting.posting_key.clone()),
        );
    }

    let mut facet_keys = batch_json_keys(batch, MEMORY_FACET_INDEX_NAMESPACE);
    for owner_ref in owner_refs.clone() {
        facet_keys.insert(
            scoped_memory_facet_owner_storage_key(memory_space_id, subject_id, &owner_ref)
                .map_err(|error| {
                    Error::config(
                        "memory_write_transaction_owner_facet_post_image_invalid",
                        format!("facet owner key: {error:?}"),
                    )
                })?,
        );
    }
    let mut facet_owners = Vec::new();
    for key in facet_keys {
        let image = governed_image::<MemoryFacetIndexDoc>(
            MEMORY_FACET_INDEX_NAMESPACE,
            &key,
            before,
            after,
        )?;
        if let Some(doc) = image.after.as_ref().or(image.before.as_ref()) {
            owner_refs.insert(doc.owner_ref.clone());
        }
        facet_owners.push(image);
    }

    posting_keys.extend(batch_json_keys(batch, MEMORY_FACET_POSTING_NAMESPACE));
    posting_keys.remove(&manifest_key);
    let postings = posting_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryFacetPostingDoc>(
                MEMORY_FACET_POSTING_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace,
                value,
                record_key,
                ..
            } if namespace == "long_term" => {
                let owner = serde_json::from_value::<LongTermMemoryEntry>(value.clone()).map_err(
                    |error| {
                        Error::config(
                            "memory_write_transaction_post_image_decode_failed",
                            error.to_string(),
                        )
                    },
                )?;
                owner_refs.insert(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::LongTerm,
                    owner.id,
                ));
                owner_refs.insert(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::LongTerm,
                    record_key.clone(),
                ));
            }
            StoreMutation::DeleteJson {
                namespace,
                record_key,
                ..
            } if namespace == "long_term" => {
                owner_refs.insert(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::LongTerm,
                    record_key.clone(),
                ));
            }
            StoreMutation::PutJson {
                namespace,
                value,
                record_key,
                ..
            } if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE => {
                let owner = serde_json::from_value::<GovernedEvidenceDocument>(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "memory_write_transaction_post_image_decode_failed",
                            error.to_string(),
                        )
                    })?;
                owner_refs.insert(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    owner.document_id,
                ));
                owner_refs.insert(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    record_key.clone(),
                ));
            }
            StoreMutation::DeleteJson {
                namespace,
                record_key,
                ..
            } if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE => {
                owner_refs.insert(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    record_key.clone(),
                ));
            }
            _ => {}
        }
    }
    let mut long_term_owners = Vec::new();
    let mut evidence_document_owners = Vec::new();
    for owner_ref in owner_refs {
        match owner_ref.owner_plane {
            GovernedMemoryOwnerPlane::LongTerm => {
                let key =
                    scoped_long_term_memory_storage_key(memory_space_id, &owner_ref.owner_id)?;
                long_term_owners.push(governed_image::<LongTermMemoryEntry>(
                    "long_term",
                    &key,
                    before,
                    after,
                )?);
            }
            GovernedMemoryOwnerPlane::EvidenceDocument => {
                let key =
                    scoped_governed_evidence_document_key(memory_space_id, &owner_ref.owner_id)?;
                evidence_document_owners.push(governed_image::<GovernedEvidenceDocument>(
                    GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
                    &key,
                    before,
                    after,
                )?);
            }
            _ => {
                return Err(Error::config(
                    "memory_write_transaction_owner_facet_post_image_invalid",
                    format!(
                        "unsupported governed facet owner plane {}",
                        owner_ref.owner_plane.as_str()
                    ),
                ));
            }
        }
    }

    ensure_post_image_validation(
        "memory_write_transaction_owner_facet_post_image_invalid",
        validate_memory_facet_post_image(&MemoryFacetPostImageClosure {
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: subject_id.to_string(),
            long_term_owners,
            evidence_document_owners,
            facet_owners,
            postings,
            manifest,
        }),
    )
}

#[derive(Default)]
struct GraphDependencyKeys {
    revisions: BTreeSet<String>,
    node_memberships: BTreeSet<String>,
    edge_memberships: BTreeSet<String>,
    backlink_memberships: BTreeSet<String>,
    indexes: BTreeSet<String>,
}

fn graph_dependency_keys(manifests: &[&Option<MemoryGraphScopeManifest>]) -> GraphDependencyKeys {
    let mut keys = GraphDependencyKeys::default();
    for manifest in manifests.iter().filter_map(|manifest| (*manifest).as_ref()) {
        keys.revisions.insert(manifest.revision.storage_key.clone());
        keys.node_memberships.extend(
            manifest
                .node_memberships
                .iter()
                .map(|item| item.storage_key.clone()),
        );
        keys.edge_memberships.extend(
            manifest
                .edge_memberships
                .iter()
                .map(|item| item.storage_key.clone()),
        );
        keys.backlink_memberships.extend(
            manifest
                .backlink_memberships
                .iter()
                .map(|item| item.storage_key.clone()),
        );
        keys.indexes.extend(
            manifest
                .recall_indexes
                .iter()
                .map(|item| item.storage_key.clone()),
        );
    }
    keys
}

fn validate_graph_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    graph_repair_authorized: bool,
) -> Result<()> {
    let graph_namespaces = [
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        MEMORY_GRAPH_REVISION_NAMESPACE,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_INDEX_NAMESPACE,
        MEMORY_GRAPH_NODE_NAMESPACE,
        MEMORY_GRAPH_EDGE_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_NAMESPACE,
    ];
    let graph_touched = graph_namespaces
        .iter()
        .any(|namespace| batch_mutates_namespace(batch, namespace));
    let evidence_owner_touched =
        batch_mutates_namespace(batch, GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE);
    let memory_space_id = batch.scope.memory_space_id.as_str();
    let subject_id = batch.scope.subject_id.as_str();
    let scope_digest = memory_graph_scope_digest(memory_space_id, subject_id);
    let manifest_key = memory_graph_scope_manifest_key(memory_space_id, subject_id);
    let manifest = governed_image::<MemoryGraphScopeManifest>(
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        &manifest_key,
        before,
        after,
    )?;
    let scoped_after_manifest_keys =
        scoped_graph_state_keys(after, MEMORY_GRAPH_MANIFEST_NAMESPACE, &scope_digest);
    let expected_after_manifest_keys = manifest
        .after
        .as_ref()
        .map(|_| BTreeSet::from([manifest_key.clone()]))
        .unwrap_or_default();
    if scoped_after_manifest_keys != expected_after_manifest_keys {
        return Err(Error::config(
            "memory_write_transaction_graph_post_image_invalid",
            "graph after-state manifest keys must exactly match the transaction scope closure",
        ));
    }
    let GraphDependencyKeys {
        revisions: expected_after_revision_keys,
        node_memberships: expected_after_node_membership_keys,
        edge_memberships: expected_after_edge_membership_keys,
        backlink_memberships: expected_after_backlink_membership_keys,
        indexes: expected_after_index_keys,
    } = graph_dependency_keys(&[&manifest.after]);
    for (namespace, expected) in [
        (
            MEMORY_GRAPH_REVISION_NAMESPACE,
            expected_after_revision_keys,
        ),
        (
            MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
            expected_after_node_membership_keys,
        ),
        (
            MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
            expected_after_edge_membership_keys,
        ),
        (
            MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
            expected_after_backlink_membership_keys,
        ),
        (MEMORY_GRAPH_INDEX_NAMESPACE, expected_after_index_keys),
    ] {
        if scoped_graph_state_keys(after, namespace, &scope_digest) != expected {
            return Err(Error::config(
                "memory_write_transaction_graph_post_image_invalid",
                format!("graph after-state {namespace} keys do not exactly match the manifest"),
            ));
        }
    }
    let scoped_graph_after_present = graph_namespaces
        .iter()
        .any(|namespace| !scoped_graph_state_keys(after, namespace, &scope_digest).is_empty());
    if !graph_touched
        && !evidence_owner_touched
        && manifest.before.is_none()
        && manifest.after.is_none()
        && !scoped_graph_after_present
    {
        return Ok(());
    }
    if graph_touched
        && batch_json_keys(batch, MEMORY_GRAPH_MANIFEST_NAMESPACE)
            != BTreeSet::from([manifest_key.clone()])
    {
        return Err(Error::config(
            "memory_write_transaction_graph_post_image_invalid",
            "graph manifest mutations must exactly match the transaction scope manifest",
        ));
    }

    let GraphDependencyKeys {
        revisions: mut revision_keys,
        node_memberships: mut node_membership_keys,
        edge_memberships: mut edge_membership_keys,
        backlink_memberships: mut backlink_membership_keys,
        indexes: mut index_keys,
    } = graph_dependency_keys(&[&manifest.before, &manifest.after]);
    revision_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_REVISION_NAMESPACE,
        &scope_digest,
    ));
    node_membership_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        &scope_digest,
    ));
    edge_membership_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        &scope_digest,
    ));
    backlink_membership_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        &scope_digest,
    ));
    index_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_INDEX_NAMESPACE,
        &scope_digest,
    ));
    if graph_touched && batch_json_keys(batch, MEMORY_GRAPH_REVISION_NAMESPACE) != revision_keys {
        return Err(Error::config(
            "memory_write_transaction_graph_post_image_invalid",
            "graph revision mutations must exactly match the scope manifest closure",
        ));
    }
    node_membership_keys.extend(batch_json_keys(
        batch,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
    ));
    edge_membership_keys.extend(batch_json_keys(
        batch,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
    ));
    backlink_membership_keys.extend(batch_json_keys(
        batch,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
    ));
    index_keys.extend(batch_json_keys(batch, MEMORY_GRAPH_INDEX_NAMESPACE));

    if revision_keys.len() != 1 {
        return Err(Error::config(
            "memory_write_transaction_graph_post_image_invalid",
            "graph closure must resolve to exactly one revision key",
        ));
    }
    let revision_key = revision_keys
        .into_iter()
        .next()
        .expect("revision key cardinality checked");
    let revision = governed_image::<MemoryGraphRevisionDoc>(
        MEMORY_GRAPH_REVISION_NAMESPACE,
        &revision_key,
        before,
        after,
    )?;
    let node_memberships = node_membership_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphNodeMembership>(
                MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let edge_memberships = edge_membership_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphEdgeMembership>(
                MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let backlink_memberships = backlink_membership_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphBacklinkMembership>(
                MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let indexes = index_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphRecallIndexDoc>(
                MEMORY_GRAPH_INDEX_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    let expected_after_node_keys = node_memberships
        .iter()
        .filter_map(|image| image.after.as_ref().map(|value| value.document_key.clone()))
        .collect::<BTreeSet<_>>();
    let expected_after_edge_keys = edge_memberships
        .iter()
        .filter_map(|image| image.after.as_ref().map(|value| value.document_key.clone()))
        .collect::<BTreeSet<_>>();
    let expected_after_backlink_keys = backlink_memberships
        .iter()
        .filter_map(|image| image.after.as_ref().map(|value| value.document_key.clone()))
        .collect::<BTreeSet<_>>();
    for (namespace, expected) in [
        (MEMORY_GRAPH_NODE_NAMESPACE, expected_after_node_keys),
        (MEMORY_GRAPH_EDGE_NAMESPACE, expected_after_edge_keys),
        (
            MEMORY_GRAPH_BACKLINK_NAMESPACE,
            expected_after_backlink_keys,
        ),
    ] {
        if scoped_graph_state_keys(after, namespace, &scope_digest) != expected {
            return Err(Error::config(
                "memory_write_transaction_graph_post_image_invalid",
                format!("graph after-state {namespace} keys do not exactly match memberships"),
            ));
        }
    }

    let mut owner_refs = BTreeSet::new();
    for key in batch_json_keys(batch, GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE) {
        let image = governed_image::<GovernedEvidenceDocument>(
            GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
            &key,
            before,
            after,
        )?;
        if let Some(owner) = image.after.as_ref().or(image.before.as_ref()) {
            owner_refs.insert(GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::EvidenceDocument,
                owner.document_id.clone(),
            ));
        }
    }
    let mut node_keys = batch_json_keys(batch, MEMORY_GRAPH_NODE_NAMESPACE);
    node_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_NODE_NAMESPACE,
        &scope_digest,
    ));
    for membership in &node_memberships {
        for value in membership.before.iter().chain(membership.after.iter()) {
            owner_refs.insert(value.owner_ref.clone());
        }
        if let Some(value) = membership.after.as_ref().or(membership.before.as_ref()) {
            node_keys.insert(value.document_key.clone());
        }
    }
    let mut edge_keys = batch_json_keys(batch, MEMORY_GRAPH_EDGE_NAMESPACE);
    edge_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_EDGE_NAMESPACE,
        &scope_digest,
    ));
    for membership in &edge_memberships {
        if let Some(value) = membership.after.as_ref().or(membership.before.as_ref()) {
            edge_keys.insert(value.document_key.clone());
        }
    }
    let mut backlink_keys = batch_json_keys(batch, MEMORY_GRAPH_BACKLINK_NAMESPACE);
    backlink_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_BACKLINK_NAMESPACE,
        &scope_digest,
    ));
    for membership in &backlink_memberships {
        if let Some(value) = membership.after.as_ref().or(membership.before.as_ref()) {
            backlink_keys.insert(value.document_key.clone());
        }
    }
    let nodes = node_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphNode>(MEMORY_GRAPH_NODE_NAMESPACE, &key, before, after)
        })
        .collect::<Result<Vec<_>>>()?;
    let edges = edge_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphEdge>(MEMORY_GRAPH_EDGE_NAMESPACE, &key, before, after)
        })
        .collect::<Result<Vec<_>>>()?;
    let backlinks = backlink_keys
        .into_iter()
        .map(|key| {
            governed_image::<EvidenceBacklink>(MEMORY_GRAPH_BACKLINK_NAMESPACE, &key, before, after)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut long_term_owners = Vec::new();
    let mut evidence_document_owners = Vec::new();
    for owner_ref in owner_refs {
        match owner_ref.owner_plane {
            GovernedMemoryOwnerPlane::LongTerm => {
                let key =
                    scoped_long_term_memory_storage_key(memory_space_id, &owner_ref.owner_id)?;
                long_term_owners.push(governed_image::<LongTermMemoryEntry>(
                    "long_term",
                    &key,
                    before,
                    after,
                )?);
            }
            GovernedMemoryOwnerPlane::EvidenceDocument => {
                let key =
                    scoped_governed_evidence_document_key(memory_space_id, &owner_ref.owner_id)?;
                evidence_document_owners.push(governed_image::<GovernedEvidenceDocument>(
                    GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
                    &key,
                    before,
                    after,
                )?);
            }
            _ => {
                return Err(Error::config(
                    "memory_write_transaction_graph_post_image_invalid",
                    format!(
                        "unsupported governed graph owner plane {}",
                        owner_ref.owner_plane.as_str()
                    ),
                ));
            }
        }
    }

    ensure_post_image_validation(
        "memory_write_transaction_graph_post_image_invalid",
        validate_memory_graph_post_image(&MemoryGraphPostImageClosure {
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: subject_id.to_string(),
            allow_missing_before_owners: graph_repair_authorized,
            validate_transition_successors: true,
            long_term_owners,
            evidence_document_owners,
            manifest,
            revision,
            node_memberships,
            edge_memberships,
            backlink_memberships,
            indexes,
            nodes,
            edges,
            backlinks,
        }),
    )
}

fn validate_control_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<()> {
    let control_namespaces = [
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
    ];
    if !control_namespaces
        .iter()
        .any(|namespace| batch_mutates_namespace(batch, namespace))
    {
        return Ok(());
    }
    let memory_space_id = batch.scope.memory_space_id.as_str();
    let revisions = batch_json_keys(batch, LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .into_iter()
        .map(|key| {
            governed_image::<LongTermMemoryControlRevision>(
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let tombstones = batch_json_keys(batch, LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE)
        .into_iter()
        .map(|key| {
            governed_image::<LongTermMemoryTombstone>(
                LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let policies = batch_json_keys(batch, LONG_TERM_GOVERNANCE_POLICY_NAMESPACE)
        .into_iter()
        .map(|key| {
            governed_image::<MemoryLongTermGovernancePolicy>(
                LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let audits = batch_json_keys(batch, LONG_TERM_CONTROL_AUDIT_NAMESPACE)
        .into_iter()
        .map(|key| {
            governed_image::<LongTermMemoryControlAuditEvent>(
                LONG_TERM_CONTROL_AUDIT_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let operation = audits
        .iter()
        .find_map(|image| image.after.as_ref().map(|audit| audit.operation))
        .ok_or_else(|| {
            Error::config(
                "memory_write_transaction_control_post_image_invalid",
                "control transaction has no typed audit operation",
            )
        })?;
    let actor_subject_id = audits
        .iter()
        .find_map(|image| image.after.as_ref())
        .and_then(|audit| audit.actor_subject_id.clone());
    let mut owner_ids = BTreeSet::new();
    for image in &revisions {
        if let Some(value) = image.after.as_ref().or(image.before.as_ref()) {
            owner_ids.insert(value.record_id.clone());
            if let Some(successor_record_id) = value.successor_record_id.as_ref() {
                owner_ids.insert(successor_record_id.clone());
            }
        }
    }
    for image in &tombstones {
        if let Some(value) = image.after.as_ref().or(image.before.as_ref()) {
            owner_ids.insert(value.record_id.clone());
        }
    }
    let owner_records = owner_ids
        .into_iter()
        .map(|owner_id| {
            let key = scoped_long_term_memory_storage_key(memory_space_id, &owner_id)?;
            governed_image::<LongTermMemoryEntry>("long_term", &key, before, after)
        })
        .collect::<Result<Vec<_>>>()?;
    ensure_post_image_validation(
        "memory_write_transaction_control_post_image_invalid",
        validate_long_term_control_post_image(&LongTermControlPostImageClosure {
            transaction_id: batch.transaction_id.clone(),
            operation,
            memory_space_id: memory_space_id.to_string(),
            owner_subject_id: batch.scope.subject_id.clone(),
            actor_subject_id,
            owner_records,
            revisions,
            tombstones,
            policies,
            audits,
        }),
    )
}

pub(crate) fn validate_control_document_for_scope(
    namespace: &str,
    physical_key: &str,
    value: &serde_json::Value,
    memory_space_id: &str,
    mounted_subject_id: &str,
) -> Result<()> {
    let logical_key = match namespace {
        LONG_TERM_CONTROL_REVISION_NAMESPACE => {
            let revision = serde_json::from_value::<LongTermMemoryControlRevision>(value.clone())
                .map_err(|error| {
                Error::config(
                    "control_plane_scope_manifest",
                    format!("control revision decode failed: {error}"),
                )
            })?;
            if revision.memory_space_id.as_deref() != Some(memory_space_id)
                || revision.owner_subject_id != mounted_subject_id
            {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "control revision owner scope differs from the manifest scope",
                ));
            }
            revision.revision_id
        }
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE => {
            let tombstone = serde_json::from_value::<LongTermMemoryTombstone>(value.clone())
                .map_err(|error| {
                    Error::config(
                        "control_plane_scope_manifest",
                        format!("control tombstone decode failed: {error}"),
                    )
                })?;
            if tombstone.memory_space_id.as_deref() != Some(memory_space_id)
                || tombstone.owner_subject_id != mounted_subject_id
            {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "control tombstone owner scope differs from the manifest scope",
                ));
            }
            tombstone.record_id
        }
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE => {
            let policy = serde_json::from_value::<MemoryLongTermGovernancePolicy>(value.clone())
                .map_err(|error| {
                    Error::config(
                        "control_plane_scope_manifest",
                        format!("governance policy decode failed: {error}"),
                    )
                })?;
            if policy.memory_space_id != memory_space_id
                || policy.selector.memory_space_id.as_deref() != Some(memory_space_id)
                || policy.selector.subject_id.as_deref() != Some(mounted_subject_id)
            {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "governance policy must declare the exact memory-space and subject closure",
                ));
            }
            policy.policy_id
        }
        LONG_TERM_CONTROL_AUDIT_NAMESPACE => {
            let audit = serde_json::from_value::<LongTermMemoryControlAuditEvent>(value.clone())
                .map_err(|error| {
                    Error::config(
                        "control_plane_scope_manifest",
                        format!("control audit decode failed: {error}"),
                    )
                })?;
            if audit.memory_space_id.as_deref() != Some(memory_space_id)
                || audit.owner_subject_id != mounted_subject_id
                || audit.effects.iter().any(|effect| match effect {
                    ControlEffectRef::Revision {
                        owner_subject_id, ..
                    }
                    | ControlEffectRef::Tombstone {
                        owner_subject_id, ..
                    }
                    | ControlEffectRef::Policy {
                        owner_subject_id, ..
                    } => owner_subject_id != mounted_subject_id,
                })
            {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "control audit owner closure differs from the manifest scope",
                ));
            }
            audit.event_id
        }
        _ => {
            return Err(Error::config(
                "control_plane_scope_manifest",
                format!("unsupported control-plane namespace {namespace}"),
            ));
        }
    };
    let expected = scoped_long_term_control_storage_key(memory_space_id, namespace, &logical_key)?;
    if expected != physical_key {
        return Err(Error::config(
            "control_plane_scope_manifest",
            "control-plane document physical key is not canonical",
        ));
    }
    Ok(())
}

fn validate_graph_manifest_closure(batch: &StoreMutationBatch) -> Result<()> {
    const GRAPH_NAMESPACES: &[&str] = &[
        "memory_graph_nodes",
        "memory_graph_edges",
        "memory_graph_backlinks",
        "memory_graph_indexes",
        "memory_graph_revisions",
        "memory_graph_manifests",
        "memory_graph_node_memberships",
        "memory_graph_edge_memberships",
        "memory_graph_backlink_memberships",
    ];
    let graph_mutations = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. }
                if GRAPH_NAMESPACES.contains(&namespace.as_str()) =>
            {
                Some((namespace.as_str(), key.as_str()))
            }
            _ => None,
        });
    let graph_mutations = graph_mutations.collect::<Vec<_>>();
    let evidence_owner_mutated = batch.mutations.iter().any(|mutation| {
        matches!(
            mutation,
            StoreMutation::PutJson { namespace, .. }
                | StoreMutation::DeleteJson { namespace, .. }
                if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
        )
    });
    if graph_mutations.is_empty() && !evidence_owner_mutated {
        return Ok(());
    }
    if graph_mutations.is_empty() {
        return Err(Error::config(
            "memory_write_transaction_graph_manifest_closure_missing",
            "evidence owner mutations require an explicit same-transaction graph closure",
        ));
    }

    let scope_digest = memory_graph_scope_digest(
        batch.scope.memory_space_id.as_str(),
        batch.scope.subject_id.as_str(),
    );
    let expected_prefix = format!("scope:{scope_digest}:doc:");
    for (namespace, key) in &graph_mutations {
        let Some(document_digest) = key.strip_prefix(&expected_prefix) else {
            return Err(Error::config(
                "memory_write_transaction_graph_scope_mismatch",
                format!(
                    "graph mutation {namespace}/{key} is outside transaction scope {scope_digest}"
                ),
            ));
        };
        let Some(document_digest) = document_digest.strip_prefix("sha256:") else {
            return Err(Error::config(
                "memory_write_transaction_graph_physical_key_invalid",
                format!("graph mutation {namespace}/{key} has an invalid physical key"),
            ));
        };
        if document_digest.len() != 64
            || !document_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::config(
                "memory_write_transaction_graph_physical_key_invalid",
                format!("graph mutation {namespace}/{key} has an invalid physical key"),
            ));
        }
    }

    let expected_manifest_key =
        memory_graph_scope_manifest_key(&batch.scope.memory_space_id, &batch.scope.subject_id);
    let manifest_keys = graph_mutations
        .iter()
        .filter_map(|(namespace, key)| {
            (*namespace == MEMORY_GRAPH_MANIFEST_NAMESPACE).then_some(*key)
        })
        .collect::<BTreeSet<_>>();
    if manifest_keys == BTreeSet::from([expected_manifest_key.as_str()]) {
        Ok(())
    } else {
        Err(Error::config(
            "memory_write_transaction_graph_manifest_closure_missing",
            "graph mutations require exactly the transaction scope manifest mutation",
        ))
    }
}

fn validate_control_audit_closure(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    let control_mutated = batch.mutations.iter().any(|mutation| match mutation {
        StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. } => {
            matches!(
                namespace.as_str(),
                LONG_TERM_CONTROL_REVISION_NAMESPACE
                    | LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE
                    | LONG_TERM_GOVERNANCE_POLICY_NAMESPACE
            )
        }
        _ => false,
    });
    let audit_mutated = batch.mutations.iter().any(|mutation| match mutation {
        StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. } => {
            namespace == LONG_TERM_CONTROL_AUDIT_NAMESPACE
        }
        _ => false,
    });
    if control_mutated != audit_mutated {
        return Err(Error::config(
            "memory_write_transaction_control_audit_closure_missing",
            "control metadata and its audit event must mutate in the same transaction",
        ));
    }
    if !control_mutated {
        return Ok(());
    }

    let mut required_record_ids = BTreeSet::new();
    let mut required_policy_ids = BTreeSet::new();
    let mut audited_record_ids = BTreeSet::new();
    let mut audited_policy_ids = BTreeSet::new();

    for mutation in &batch.mutations {
        let (namespace, record_key) = match mutation {
            StoreMutation::PutJson {
                namespace,
                record_key,
                ..
            }
            | StoreMutation::DeleteJson {
                namespace,
                record_key,
                ..
            } => (namespace.as_str(), record_key.as_str()),
            _ => continue,
        };
        match namespace {
            LONG_TERM_CONTROL_REVISION_NAMESPACE => {
                let revision = serde_json::from_value::<LongTermMemoryControlRevision>(
                    control_mutation_value(mutation, preconditions)?,
                )
                .map_err(|error| {
                    Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        format!("invalid control revision: {error}"),
                    )
                })?;
                if revision.revision_id != record_key {
                    return Err(Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        "control revision record_key does not match revision_id",
                    ));
                }
                required_record_ids.insert(revision.record_id);
            }
            LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE => {
                let tombstone = serde_json::from_value::<LongTermMemoryTombstone>(
                    control_mutation_value(mutation, preconditions)?,
                )
                .map_err(|error| {
                    Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        format!("invalid control tombstone: {error}"),
                    )
                })?;
                if tombstone.record_id != record_key {
                    return Err(Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        "control tombstone record_key does not match record_id",
                    ));
                }
                required_record_ids.insert(tombstone.record_id);
            }
            LONG_TERM_GOVERNANCE_POLICY_NAMESPACE => {
                let policy = serde_json::from_value::<MemoryLongTermGovernancePolicy>(
                    control_mutation_value(mutation, preconditions)?,
                )
                .map_err(|error| {
                    Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        format!("invalid governance policy: {error}"),
                    )
                })?;
                if policy.policy_id != record_key {
                    return Err(Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        "governance policy record_key does not match policy_id",
                    ));
                }
                required_policy_ids.insert(policy.policy_id);
            }
            LONG_TERM_CONTROL_AUDIT_NAMESPACE => {
                let StoreMutation::PutJson { value, .. } = mutation else {
                    return Err(Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        "control audit history is append-only",
                    ));
                };
                let audit =
                    serde_json::from_value::<LongTermMemoryControlAuditEvent>(value.clone())
                        .map_err(|error| {
                            Error::config(
                                "memory_write_transaction_control_audit_binding_invalid",
                                format!("invalid control audit event: {error}"),
                            )
                        })?;
                if audit.event_id != record_key {
                    return Err(Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        "control audit record_key does not match event_id",
                    ));
                }
                for effect in audit.effects {
                    match effect {
                        ControlEffectRef::Revision { record_id, .. }
                        | ControlEffectRef::Tombstone { record_id, .. } => {
                            audited_record_ids.insert(record_id);
                        }
                        ControlEffectRef::Policy { policy_id, .. } => {
                            audited_policy_ids.insert(policy_id);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let missing_records = required_record_ids
        .difference(&audited_record_ids)
        .cloned()
        .collect::<Vec<_>>();
    let missing_policies = required_policy_ids
        .difference(&audited_policy_ids)
        .cloned()
        .collect::<Vec<_>>();
    if missing_records.is_empty() && missing_policies.is_empty() {
        return Ok(());
    }
    Err(Error::config(
        "memory_write_transaction_control_audit_binding_invalid",
        format!(
            "control audit does not cover records [{}] and policies [{}]",
            missing_records.join(","),
            missing_policies.join(",")
        ),
    ))
}

fn control_mutation_value(
    mutation: &StoreMutation,
    preconditions: &[StoreJsonPrecondition],
) -> Result<serde_json::Value> {
    match mutation {
        StoreMutation::PutJson { value, .. } => Ok(value.clone()),
        StoreMutation::DeleteJson { namespace, key, .. } => preconditions
            .iter()
            .find_map(|precondition| match precondition {
                StoreJsonPrecondition::Exact {
                    namespace: expected_namespace,
                    key: expected_key,
                    value,
                } if expected_namespace == namespace && expected_key == key => Some(value.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                Error::config(
                    "memory_write_transaction_control_audit_binding_invalid",
                    "control delete requires an exact precondition for audit binding",
                )
            }),
        _ => Err(Error::config(
            "memory_write_transaction_control_audit_binding_invalid",
            "control audit binding requires a JSON mutation",
        )),
    }
}

fn validate_batch_mutation_namespaces(batch: &StoreMutationBatch) -> Result<()> {
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson { namespace, .. }
            | StoreMutation::DeleteJson { namespace, .. } => {
                ensure_batch_json_namespace(namespace)?;
            }
            StoreMutation::PutBlob { namespace, .. }
            | StoreMutation::DeleteBlob { namespace, .. } => {
                ensure_batch_blob_namespace(namespace)?;
            }
            StoreMutation::AppendEvent { .. } => {}
        }
    }
    Ok(())
}

fn validate_governed_owner_facet_closure(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    let owner_refs = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace,
                record_key,
                ..
            }
            | StoreMutation::DeleteJson {
                namespace,
                record_key,
                ..
            } if namespace == "long_term" => Some(GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::LongTerm,
                record_key.clone(),
            )),
            StoreMutation::PutJson {
                namespace,
                record_key,
                ..
            }
            | StoreMutation::DeleteJson {
                namespace,
                record_key,
                ..
            } if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE => {
                Some(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    record_key.clone(),
                ))
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if owner_refs.is_empty() {
        return Ok(());
    }

    let facet_owner_refs = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == MEMORY_FACET_INDEX_NAMESPACE => {
                serde_json::from_value::<MemoryFacetIndexDoc>(value.clone())
                    .ok()
                    .map(|doc| doc.owner_ref)
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == MEMORY_FACET_INDEX_NAMESPACE =>
            {
                preconditions
                    .iter()
                    .find_map(|precondition| match precondition {
                        StoreJsonPrecondition::Exact {
                            namespace,
                            key: precondition_key,
                            value,
                        } if namespace == MEMORY_FACET_INDEX_NAMESPACE
                            && precondition_key == key =>
                        {
                            serde_json::from_value::<MemoryFacetIndexDoc>(value.clone())
                                .ok()
                                .map(|doc| doc.owner_ref)
                        }
                        _ => None,
                    })
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let missing = owner_refs
        .difference(&facet_owner_refs)
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(Error::config(
        "memory_write_transaction_owner_facet_closure_missing",
        format!(
            "governed owner mutations require same-transaction facet owner closure: {}",
            missing
                .iter()
                .map(|owner_ref| format!(
                    "{}:{}",
                    owner_ref.owner_plane.as_str(),
                    owner_ref.owner_id
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
    ))
}

fn ensure_batch_json_namespace(namespace: &str) -> Result<()> {
    ensure_json_snapshot_namespace(namespace, "memory_write_transaction_preflight_failed")
}

fn ensure_json_snapshot_namespace(namespace: &str, stage: &'static str) -> Result<()> {
    if JSON_SNAPSHOT_NAMESPACES.contains(&namespace) {
        return Ok(());
    }
    Err(Error::config(
        stage,
        format!("unsupported json namespace {namespace}"),
    ))
}

fn ensure_batch_blob_namespace(namespace: &str) -> Result<()> {
    if BLOB_SNAPSHOT_NAMESPACES.contains(&namespace) {
        return Ok(());
    }
    Err(Error::config(
        "memory_write_transaction_preflight_failed",
        format!("unsupported blob namespace {namespace}"),
    ))
}

fn memory_write_transaction_preflight_error(error: Error) -> Error {
    if error.stage() == "memory_write_transaction_preflight_failed" {
        error
    } else {
        Error::config(
            "memory_write_transaction_preflight_failed",
            error.to_string(),
        )
    }
}

fn memory_write_transaction_commit_error(error: Error) -> Error {
    match error.stage() {
        "store_budget_exceeded" | "store_event_log" | "store_snapshot_import" => {
            memory_write_transaction_preflight_error(error)
        }
        "memory_write_transaction_precondition_failed"
        | "memory_write_transaction_evidence_source_ref_post_image_invalid"
        | "store_transaction_busy"
        | "memory_write_transaction_repair_required" => error,
        _ => Error::config("memory_write_transaction_commit_failed", error.to_string()),
    }
}

const JSON_SNAPSHOT_NAMESPACES: &[&str] = &[
    "skill_meta",
    "active_work",
    "execution_state",
    "long_term_extraction_state",
    "turn_ledger",
    "self_model",
    "self_authored_core",
    "core_revision_ledger",
    "self_continuity",
    "relationship_constitution",
    "relationship_portfolio",
    "relationship_topology",
    "world_sense",
    "outer_voice",
    "autonomy_strategy",
    "inner_life",
    "felt_significance",
    "temperament_continuity",
    "inner_conflict",
    "mental_privacy",
    "private_doc",
    "conversation_transcript",
    "conversation_transcript_alias",
    "conversation_transcript_attr",
    "conversation_transcript_derived_ref",
    "memory_graph_nodes",
    "memory_graph_edges",
    "memory_graph_backlinks",
    "memory_graph_indexes",
    "memory_graph_revisions",
    "memory_graph_manifests",
    "memory_graph_node_memberships",
    "memory_graph_edge_memberships",
    "memory_graph_backlink_memberships",
    "memory_facet_indexes",
    "memory_facet_postings",
    LONG_TERM_CONTROL_REVISION_NAMESPACE,
    LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
    LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
    LONG_TERM_CONTROL_AUDIT_NAMESPACE,
    CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
    "session_summary",
    "session",
    "long_term",
    GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
    GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE,
    GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
    "continuity_capsule",
    "turn_continuity_evidence",
    "private_garden",
    "remind_at",
    "task",
    "task_run",
    "task_artifact",
    "task_learning",
    CONVERSATION_RECALL_MANIFEST_NAMESPACE,
    ARCHIVE_RECALL_MANIFEST_NAMESPACE,
    RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE,
    CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE,
    ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE,
    TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
    RECALL_OWNER_SCOPE_BINDING_NAMESPACE,
];

const BLOB_SNAPSHOT_NAMESPACES: &[&str] = &["state_fs", "skills", "memory", "daily"];

pub(crate) fn snapshot_namespace_requires_private_export(namespace: &str) -> bool {
    matches!(
        namespace,
        "self_model"
            | "self_authored_core"
            | "core_revision_ledger"
            | "self_continuity"
            | "relationship_constitution"
            | "relationship_portfolio"
            | "relationship_topology"
            | "outer_voice"
            | "inner_life"
            | "felt_significance"
            | "temperament_continuity"
            | "inner_conflict"
            | "mental_privacy"
            | "private_doc"
            | "conversation_transcript"
            | "conversation_transcript_alias"
            | "conversation_transcript_attr"
            | "conversation_transcript_derived_ref"
            | GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
            | GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE
            | GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE
            | "private_garden"
    )
}

pub(crate) fn snapshot_key_requires_private_export(key: &str) -> bool {
    key.contains("private_garden")
        || key.contains("private_doc")
        || key.contains("mental_privacy")
        || key.contains("inner_life")
        || key.contains("self_model")
        || key.contains("self_continuity")
        || key.contains("conversation_transcript")
        || key.contains("conversation_transcript_alias")
        || key.contains("conversation_transcript_derived_ref")
}

pub(crate) fn snapshot_json_requires_private_export(
    namespace: &str,
    value: &serde_json::Value,
) -> bool {
    snapshot_namespace_requires_private_export(namespace)
        || value
            .get("privacy")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|privacy| {
                matches!(
                    privacy,
                    "private_garden" | "soul_private" | "operator_diagnostic"
                )
            })
}

impl StoreEventLog for StorePlatform {
    #[cfg(feature = "nonproduction-replay-harness")]
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        self.append_validated_event(event)
    }

    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        self.engine.read_events()
    }
}

impl RuntimeLifecycleEventSink for StorePlatform {
    fn record_lifecycle_event(&self, event: RuntimeLifecycleEvent) -> Result<()> {
        let event_value = serde_json::to_value(&event)
            .map_err(|error| Error::config("runtime_lifecycle_event", error.to_string()))?;
        let content_hash = stable_hash_json(&event_value)?;
        let kind = match event.kind {
            RuntimeLifecycleEventKind::RuntimeLifecycle => MemoryStoreEventKind::RuntimeLifecycle,
            RuntimeLifecycleEventKind::OperatorAction => MemoryStoreEventKind::OperatorAction,
        };
        let mut store_event = MemoryStoreEvent::new(
            event.event_id,
            kind,
            StoreEventScope::system(event.operation.as_str()),
            event.timestamp_unix_secs,
        )
        .with_plane("runtime_lifecycle")
        .with_record_key(event.operation.as_str())
        .with_content_hash(content_hash);
        store_event = store_event
            .with_payload("operation", event.operation.as_str())
            .with_payload("trigger", event.trigger.as_str())
            .with_payload("disposition", event.disposition.as_str())
            .with_payload("effect", event.effect.as_str())
            .with_payload("profile", event.profile.as_str())
            .with_payload("mode", event.mode.as_str())
            .with_payload(
                "pressure",
                format!("{:?}", event.pressure).to_ascii_lowercase(),
            )
            .with_payload("reason", event.reason)
            .with_payload("result", event.result)
            .with_payload("error_stage", event.error_stage.unwrap_or_default());
        for (key, value) in event.payload {
            store_event = store_event.with_payload(key, value);
        }
        self.append_validated_event(store_event)
    }
}

impl Platform for StorePlatform {
    fn memory_system_kind(&self) -> MemorySystemKind {
        self.config.memory_system_kind
    }

    fn runtime_lifecycle_event_sink(&self) -> Arc<dyn bm_core::runtime::RuntimeLifecycleEventSink> {
        Arc::new(self.clone())
    }

    fn state_fs(&self) -> Arc<dyn StateFs> {
        Arc::new(self.clone())
    }

    fn skill_storage(&self) -> Arc<dyn SkillStorage> {
        Arc::new(self.clone())
    }

    fn skill_meta_store(&self) -> Arc<dyn SkillMetaStore> {
        Arc::new(self.clone())
    }

    fn active_work_store(&self) -> Arc<dyn ActiveWorkStore> {
        Arc::new(self.clone())
    }

    fn memory_store(&self) -> Arc<dyn MemoryStore> {
        Arc::new(self.clone())
    }

    fn session_store(&self) -> Arc<dyn SessionStore> {
        Arc::new(self.clone())
    }

    fn conversation_transcript_store(&self) -> Arc<dyn ConversationTranscriptStore> {
        Arc::new(self.clone())
    }

    fn session_summary_store(&self) -> Arc<dyn SessionSummaryStore> {
        Arc::new(self.clone())
    }

    fn long_term_memory_extraction_state_store(
        &self,
    ) -> Arc<dyn LongTermMemoryExtractionStateStore> {
        Arc::new(self.clone())
    }

    fn continuity_capsule_store(&self) -> Arc<dyn ContinuityCapsuleStore> {
        Arc::new(self.clone())
    }

    fn turn_ledger_store(&self) -> Arc<dyn TurnLedgerStore> {
        Arc::new(self.clone())
    }

    fn self_model_store(&self) -> Arc<dyn SelfModelStore> {
        Arc::new(self.clone())
    }

    fn self_authored_core_store(&self) -> Arc<dyn SelfAuthoredCoreStore> {
        Arc::new(self.clone())
    }

    fn core_revision_ledger_store(&self) -> Arc<dyn CoreRevisionLedgerStore> {
        Arc::new(self.clone())
    }

    fn self_continuity_store(&self) -> Arc<dyn SelfContinuityStore> {
        Arc::new(self.clone())
    }

    fn relationship_constitution_store(&self) -> Arc<dyn RelationshipConstitutionStore> {
        Arc::new(self.clone())
    }

    fn relationship_portfolio_store(&self) -> Arc<dyn RelationshipPortfolioStore> {
        Arc::new(self.clone())
    }

    fn relationship_topology_store(&self) -> Arc<dyn RelationshipTopologyStore> {
        Arc::new(self.clone())
    }

    fn execution_state_store(&self) -> Arc<dyn ExecutionStateStore> {
        Arc::new(self.clone())
    }

    fn world_sense_store(&self) -> Arc<dyn WorldSenseStore> {
        Arc::new(self.clone())
    }

    fn outer_voice_store(&self) -> Arc<dyn OuterVoiceStore> {
        Arc::new(self.clone())
    }

    fn autonomy_strategy_store(&self) -> Arc<dyn AutonomyStrategyStore> {
        Arc::new(self.clone())
    }

    fn inner_life_store(&self) -> Arc<dyn InnerLifeStore> {
        Arc::new(self.clone())
    }

    fn felt_significance_store(&self) -> Arc<dyn FeltSignificanceStore> {
        Arc::new(self.clone())
    }

    fn temperament_continuity_store(&self) -> Arc<dyn TemperamentContinuityStore> {
        Arc::new(self.clone())
    }

    fn inner_conflict_store(&self) -> Arc<dyn InnerConflictStore> {
        Arc::new(self.clone())
    }

    fn mental_privacy_store(&self) -> Arc<dyn MentalPrivacyStore> {
        Arc::new(self.clone())
    }

    fn private_doc_store(&self) -> Arc<dyn PrivateDocStore> {
        Arc::new(self.clone())
    }

    fn private_garden_store(&self) -> Arc<dyn PrivateGardenStore> {
        Arc::new(self.clone())
    }

    fn turn_continuity_evidence_store(&self) -> Arc<dyn TurnContinuityEvidenceStore> {
        Arc::new(self.clone())
    }

    fn remind_at_store(&self) -> Arc<dyn RemindAtStore> {
        Arc::new(self.clone())
    }

    fn task_store(&self) -> Arc<dyn TaskStore> {
        Arc::new(self.clone())
    }

    fn task_run_store(&self) -> Arc<dyn TaskRunStore> {
        Arc::new(self.clone())
    }

    fn task_artifact_store(&self) -> Arc<dyn TaskArtifactStore> {
        Arc::new(self.clone())
    }

    fn task_learning_store(&self) -> Arc<dyn TaskLearningStore> {
        Arc::new(self.clone())
    }
}

impl StateFs for StorePlatform {
    fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
        self.engine.get_blob("state_fs", rel_path)
    }

    fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
        self.blob_put("state_fs", rel_path, data)
    }

    fn remove(&self, rel_path: &str) -> Result<()> {
        self.blob_delete("state_fs", rel_path).map(|_| ())
    }

    fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
        let prefix = rel_path.trim_end_matches('/');
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        let mut out = self
            .engine
            .list_blob_keys("state_fs")?
            .into_iter()
            .filter_map(|key| key.strip_prefix(&prefix).map(ToString::to_string))
            .collect::<Vec<_>>();
        out.sort();
        Ok(out)
    }
}

impl SkillStorage for StorePlatform {
    fn list_names(&self) -> Result<Vec<String>> {
        self.engine.list_blob_keys("skills")
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        Ok(self.engine.get_blob("skills", name)?.unwrap_or_default())
    }

    fn write(&self, name: &str, content: &[u8]) -> Result<()> {
        let _transaction_guard = self.lock_transaction("runtime_skill_recall_index_write")?;
        let updated_at = runtime_skill_owner_updated_at(name, content).ok_or_else(|| {
            Error::config(
                "runtime_skill_recall_manifest",
                "runtime skill content must provide a canonical typed owner timestamp",
            )
        })?;
        let index = self.plan_runtime_skill_index_upsert(name, content, updated_at)?;
        self.commit_recall_indexed_mutations_at(
            "runtime_skill.write",
            self.recall_scope(),
            vec![StoreMutation::PutBlob {
                namespace: "skills".to_string(),
                key: name.to_string(),
                value: content.to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skills".to_string(),
                record_key: name.to_string(),
            }],
            vec![index],
            Some(updated_at),
        )?;
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<()> {
        let _transaction_guard = self.lock_transaction("runtime_skill_recall_index_delete")?;
        let Some(content) = self.engine.get_blob("skills", name)? else {
            return Ok(());
        };
        let updated_at = runtime_skill_owner_updated_at(name, &content).ok_or_else(|| {
            Error::config(
                "runtime_skill_recall_manifest",
                "runtime skill delete requires a canonical typed owner timestamp",
            )
        })?;
        let index = self.plan_runtime_skill_index_remove(name)?;
        self.commit_recall_indexed_mutations_at(
            "runtime_skill.delete",
            self.recall_scope(),
            vec![StoreMutation::DeleteBlob {
                namespace: "skills".to_string(),
                key: name.to_string(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "skills".to_string(),
                record_key: name.to_string(),
            }],
            vec![index],
            Some(updated_at),
        )?;
        Ok(())
    }
}

impl SkillMetaStore for StorePlatform {
    fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
        let order = self
            .json_get::<Vec<String>>("skill_meta", "order")?
            .unwrap_or_default();
        let disabled = self
            .json_get::<Vec<String>>("skill_meta", "disabled")?
            .unwrap_or_default();
        Ok((order, disabled))
    }

    fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()> {
        self.json_put("skill_meta", "order", &order.to_vec())?;
        self.json_put("skill_meta", "disabled", &disabled.to_vec())
    }
}

macro_rules! impl_keyed_json_store {
    ($trait_name:path, $value_ty:path, $namespace:literal) => {
        impl $trait_name for StorePlatform {
            fn get(&self, key: &str) -> Result<Option<$value_ty>> {
                self.json_get($namespace, key)
            }

            fn set(&self, key: &str, value: &$value_ty) -> Result<()> {
                self.json_put($namespace, key, value)
            }

            fn clear(&self, key: &str) -> Result<()> {
                self.json_delete($namespace, key).map(|_| ())
            }
        }
    };
}

impl_keyed_json_store!(ExecutionStateStore, ExecutionState, "execution_state");
impl_keyed_json_store!(
    LongTermMemoryExtractionStateStore,
    LongTermMemoryExtractionState,
    "long_term_extraction_state"
);
impl_keyed_json_store!(SelfModelStore, SelfModel, "self_model");
impl_keyed_json_store!(
    SelfAuthoredCoreStore,
    SelfAuthoredCore,
    "self_authored_core"
);
impl_keyed_json_store!(
    CoreRevisionLedgerStore,
    CoreRevisionLedger,
    "core_revision_ledger"
);
impl_keyed_json_store!(SelfContinuityStore, SelfContinuity, "self_continuity");
impl_keyed_json_store!(
    RelationshipConstitutionStore,
    RelationshipConstitution,
    "relationship_constitution"
);
impl_keyed_json_store!(
    RelationshipPortfolioStore,
    RelationshipPortfolio,
    "relationship_portfolio"
);
impl_keyed_json_store!(
    RelationshipTopologyStore,
    RelationshipTopology,
    "relationship_topology"
);
impl_keyed_json_store!(WorldSenseStore, WorldSense, "world_sense");
impl_keyed_json_store!(OuterVoiceStore, OuterVoice, "outer_voice");
impl_keyed_json_store!(AutonomyStrategyStore, AutonomyStrategy, "autonomy_strategy");
impl_keyed_json_store!(InnerLifeStore, InnerLife, "inner_life");
impl_keyed_json_store!(FeltSignificanceStore, FeltSignificance, "felt_significance");
impl_keyed_json_store!(
    TemperamentContinuityStore,
    TemperamentContinuity,
    "temperament_continuity"
);
impl_keyed_json_store!(InnerConflictStore, InnerConflict, "inner_conflict");
impl_keyed_json_store!(MentalPrivacyStore, MentalPrivacyState, "mental_privacy");
impl_keyed_json_store!(PrivateDocStore, PrivateDocWorkspace, "private_doc");

impl ActiveWorkStore for StorePlatform {
    fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
        self.json_get("active_work", chat_id)
    }

    fn set(&self, chat_id: &str, record: &ActiveWorkRecord) -> Result<()> {
        self.put_archive_json_owner("archive.active_work.write", "active_work", chat_id, record)
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.delete_archive_json_owner("archive.active_work.delete", "active_work", chat_id)
    }
}

impl TurnLedgerStore for StorePlatform {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        self.json_get("turn_ledger", chat_id)
    }

    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
        self.put_archive_json_owner("archive.turn_ledger.write", "turn_ledger", chat_id, ledger)
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.delete_archive_json_owner("archive.turn_ledger.delete", "turn_ledger", chat_id)
    }
}

impl SessionSummaryStore for StorePlatform {
    fn get(&self, chat_id: &str) -> Result<Option<String>> {
        Ok(self
            .json_get::<SessionSummaryRecord>("session_summary", chat_id)?
            .map(|record| record.summary))
    }

    fn set(&self, chat_id: &str, summary: &str) -> Result<()> {
        let count = self
            .json_get::<SessionSummaryRecord>("session_summary", chat_id)?
            .map(|record| record.message_count)
            .unwrap_or(0);
        self.put_archive_json_owner(
            "archive.session_summary.write",
            "session_summary",
            chat_id,
            &SessionSummaryRecord {
                summary: summary.to_string(),
                message_count: count,
            },
        )
    }

    fn set_with_count(&self, chat_id: &str, summary: &str, message_count: usize) -> Result<()> {
        self.put_archive_json_owner(
            "archive.session_summary.write",
            "session_summary",
            chat_id,
            &SessionSummaryRecord {
                summary: summary.to_string(),
                message_count,
            },
        )
    }

    fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
        Ok(self
            .json_get::<SessionSummaryRecord>("session_summary", chat_id)?
            .map(|record| (record.summary, record.message_count)))
    }
}

impl MemoryStore for StorePlatform {
    fn get_memory(&self) -> Result<String> {
        Ok(self
            .engine
            .get_blob("memory", "MEMORY.md")?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default())
    }

    fn set_memory(&self, content: &str) -> Result<()> {
        self.put_archive_blob_owner(
            "archive.memory.write",
            "memory",
            "MEMORY.md",
            content.as_bytes(),
        )
    }

    fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
        let mut names = self.engine.list_blob_keys("daily")?;
        names.sort_by(|a, b| b.cmp(a));
        names.truncate(recent_n);
        Ok(names)
    }

    fn get_daily_note(&self, name: &str) -> Result<String> {
        Ok(self
            .engine
            .get_blob("daily", name)?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default())
    }

    fn write_daily_note(&self, name: &str, content: &str) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_daily_write")?;
        let manifest_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&manifest_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::blob(
            "daily",
            name,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Blob,
                "daily",
                name,
            ),
            current_unix_secs(),
            content.as_bytes(),
        )?;
        let index = self.plan_archive_index_upsert(address)?;
        self.commit_recall_indexed_mutations(
            "archive.daily.write",
            self.recall_scope(),
            vec![StoreMutation::PutBlob {
                namespace: "daily".to_string(),
                key: name.to_string(),
                value: content.as_bytes().to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "daily".to_string(),
                record_key: name.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }
}

impl SessionStore for StorePlatform {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_session_append")?;
        if role.trim().is_empty() {
            return Err(Error::config("session_append", "role must not be empty"));
        }
        if content.len() > MAX_SESSION_MESSAGE_LEN {
            return Err(Error::config(
                "session_append",
                format!(
                    "content length {} exceeds {}",
                    content.len(),
                    MAX_SESSION_MESSAGE_LEN
                ),
            ));
        }
        let mut messages = self
            .json_get::<Vec<SessionMessage>>("session", chat_id)?
            .unwrap_or_default();
        let now_secs = current_unix_secs();
        let (speaker_id, speaker_kind) = default_session_speaker_for_role(role);
        let message_id = stable_hash_id(
            "msg",
            &(
                chat_id,
                role,
                content,
                messages.len(),
                now_secs,
                current_unix_nanos(),
            ),
        );
        messages.push(SessionMessage::new(
            message_id,
            role,
            content,
            now_secs,
            now_secs,
            speaker_id,
            speaker_kind,
        ));
        if messages.len() > MAX_SESSION_ENTRIES {
            let remove_count = messages.len() - MAX_SESSION_ENTRIES;
            messages.drain(0..remove_count);
        }
        let value = serde_json::to_value(&messages)
            .map_err(|error| Error::config("archive_recall_session", error.to_string()))?;
        let manifest_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&manifest_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "session",
            chat_id,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                "session",
                chat_id,
            ),
            now_secs,
            &value,
        )?;
        let index = self.plan_archive_index_upsert(address)?;
        self.commit_recall_indexed_mutations(
            "archive.session.append",
            self.recall_scope(),
            vec![StoreMutation::PutJson {
                namespace: "session".to_string(),
                key: chat_id.to_string(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "session".to_string(),
                record_key: chat_id.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn append_batch(&self, chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
        let _transaction_guard = self.lock_transaction("archive_recall_session_append_batch")?;
        let mut persisted = self
            .json_get::<Vec<SessionMessage>>("session", chat_id)?
            .unwrap_or_default();
        for message in messages {
            if message.message_id.trim().is_empty() {
                return Err(Error::config(
                    "session_append_batch",
                    "message_id must not be empty",
                ));
            }
            if message.role.trim().is_empty() {
                return Err(Error::config(
                    "session_append_batch",
                    "role must not be empty",
                ));
            }
            if message.content.len() > MAX_SESSION_MESSAGE_LEN {
                return Err(Error::config(
                    "session_append_batch",
                    format!(
                        "content length {} exceeds {}",
                        message.content.len(),
                        MAX_SESSION_MESSAGE_LEN
                    ),
                ));
            }
            persisted.push(message.clone());
        }
        if persisted.len() > MAX_SESSION_ENTRIES {
            let remove_count = persisted.len() - MAX_SESSION_ENTRIES;
            persisted.drain(0..remove_count);
        }
        let value = serde_json::to_value(&persisted)
            .map_err(|error| Error::config("archive_recall_session", error.to_string()))?;
        let manifest_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&manifest_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "session",
            chat_id,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                "session",
                chat_id,
            ),
            current_unix_secs(),
            &value,
        )?;
        let index = self.plan_archive_index_upsert(address)?;
        self.commit_recall_indexed_mutations(
            "archive.session.append_batch",
            self.recall_scope(),
            vec![StoreMutation::PutJson {
                namespace: "session".to_string(),
                key: chat_id.to_string(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "session".to_string(),
                record_key: chat_id.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let messages = self
            .json_get::<Vec<SessionMessage>>("session", chat_id)?
            .unwrap_or_default();
        Ok(tail(messages, n))
    }

    fn message_count(&self, chat_id: &str) -> Result<usize> {
        Ok(self
            .json_get::<Vec<SessionMessage>>("session", chat_id)?
            .map(|messages| messages.len())
            .unwrap_or(0))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_session_clear")?;
        if self.engine.get_json_value("session", chat_id)?.is_none() {
            return Ok(());
        }
        let index =
            self.plan_archive_index_remove(RecallIndexAddressKind::Json, "session", chat_id)?;
        self.commit_recall_indexed_mutations(
            "archive.session.clear",
            self.recall_scope(),
            vec![StoreMutation::DeleteJson {
                namespace: "session".to_string(),
                key: chat_id.to_string(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "session".to_string(),
                record_key: chat_id.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        self.engine.list_json_keys("session")
    }
}

impl ConversationTranscriptStore for StorePlatform {
    fn append_turn(&self, record: &TranscriptTurnRecord) -> Result<TranscriptCommitReport> {
        let _transaction_guard = self.lock_transaction("conversation_recall_manifest_append")?;
        let key = transcript_turn_storage_key(&record.key, &record.subject, &record.turn_id);
        let (_, manifest, manifest_before) =
            self.load_conversation_recall_manifest(&record.key, &record.subject)?;
        if let Some(manifest) = manifest.as_ref() {
            self.validate_conversation_manifest_subject(manifest, &record.subject)?;
        }
        let before_count = manifest
            .as_ref()
            .map(|manifest| {
                manifest
                    .entries
                    .iter()
                    .filter(|entry| entry.namespace == "conversation_transcript")
                    .count()
            })
            .unwrap_or(0);
        if self
            .engine
            .get_json_value("conversation_transcript", &key)?
            .is_some()
        {
            let indexed = manifest.as_ref().is_some_and(|manifest| {
                manifest.entries.iter().any(|entry| {
                    entry.kind == RecallIndexAddressKind::Json
                        && entry.namespace == "conversation_transcript"
                        && entry.key == key
                })
            });
            if !indexed {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "transcript owner exists without its required manifest binding",
                ));
            }
            return Ok(TranscriptCommitReport {
                key: record.key.clone(),
                turn_id: record.turn_id.clone(),
                sequence: record.sequence,
                committed: false,
                before_count,
                after_count: before_count,
                skipped_reason: Some("conversation_transcript_turn_already_committed".to_string()),
            });
        }
        let mut record = record.clone();
        if record.sequence == 0 {
            record.sequence = u64::try_from(before_count)
                .unwrap_or(u64::MAX)
                .saturating_add(1);
        }
        let sequence = record.sequence;
        let value = serde_json::to_value(&record)
            .map_err(|error| Error::config("conversation_recall_manifest", error.to_string()))?;
        let previous_entries = manifest
            .as_ref()
            .map(|manifest| manifest.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "conversation_transcript",
            &key,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                "conversation_transcript",
                &key,
            ),
            record.updated_at,
            &value,
        )?;
        let entries = replace_recall_index_address(previous_entries, address);
        let index = self.plan_conversation_index(
            &record.key,
            &record.subject,
            manifest.as_ref(),
            manifest_before,
            entries,
        )?;
        let mut scope = self.recall_scope();
        scope
            .memory_space_id
            .clone_from(&record.key.memory_space_id);
        scope.subject_id.clone_from(&record.subject);
        scope.channel.clone_from(&record.key.channel_id);
        scope.conversation_id = Some(record.key.conversation_id.clone());
        self.commit_recall_indexed_mutations(
            "conversation.transcript.append",
            scope,
            vec![StoreMutation::PutJson {
                namespace: "conversation_transcript".to_string(),
                key: key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "conversation_transcript".to_string(),
                record_key: key,
            }],
            vec![index],
        )?;
        Ok(TranscriptCommitReport {
            key: record.key,
            turn_id: record.turn_id,
            sequence,
            committed: true,
            before_count,
            after_count: before_count.saturating_add(1),
            skipped_reason: None,
        })
    }

    fn remember_conversation_alias(&self, alias: &TranscriptConversationAlias) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_alias_write")?;
        let owner_key = alias.storage_key();
        let value = serde_json::to_value(alias)
            .map_err(|error| Error::config("archive_recall_manifest", error.to_string()))?;
        let manifest_key = ArchiveRecallManifest::build(
            1,
            &alias.memory_space_id,
            &alias.mounted_subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&manifest_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "conversation_transcript_alias",
            &owner_key,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                "conversation_transcript_alias",
                &owner_key,
            ),
            alias.updated_at,
            &value,
        )?;
        let index = self.plan_archive_index_upsert_for_scope(
            &alias.memory_space_id,
            &alias.mounted_subject_id,
            address,
        )?;
        let mut scope = self.recall_scope();
        scope.memory_space_id.clone_from(&alias.memory_space_id);
        scope.subject_id.clone_from(&alias.mounted_subject_id);
        scope.channel.clone_from(&alias.channel_id);
        scope.chat_id.clone_from(&alias.chat_id);
        scope.conversation_id = Some(alias.conversation_id.clone());
        self.commit_recall_indexed_mutations(
            "archive.conversation_alias.write",
            scope,
            vec![StoreMutation::PutJson {
                namespace: "conversation_transcript_alias".to_string(),
                key: owner_key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "conversation_transcript_alias".to_string(),
                record_key: owner_key,
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn resolve_conversation_alias(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel_id: &str,
        chat_id: &str,
    ) -> Result<Option<String>> {
        let key = TranscriptConversationAlias::storage_key_for(
            memory_space_id,
            mounted_subject_id,
            channel_id,
            chat_id,
        );
        Ok(self
            .json_get::<TranscriptConversationAlias>("conversation_transcript_alias", &key)?
            .filter(|alias| {
                alias.memory_space_id == memory_space_id
                    && alias.mounted_subject_id == mounted_subject_id
                    && alias.channel_id == channel_id
                    && alias.chat_id == chat_id
            })
            .map(|alias| alias.conversation_id))
    }

    fn get_turn(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: &str,
    ) -> Result<Option<TranscriptTurnRecord>> {
        let (_, manifest, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let Some(manifest) = manifest else {
            return Ok(None);
        };
        self.validate_conversation_manifest_subject(&manifest, mounted_subject_id)?;
        let owner_key = transcript_turn_storage_key(key, mounted_subject_id, turn_id);
        let indexed = manifest.entries.iter().any(|entry| {
            entry.kind == RecallIndexAddressKind::Json
                && entry.namespace == "conversation_transcript"
                && entry.key == owner_key
        });
        if !indexed {
            return Ok(None);
        }
        let record = self
            .json_get::<TranscriptTurnRecord>("conversation_transcript", &owner_key)?
            .ok_or_else(|| {
                Error::config(
                    "conversation_recall_manifest",
                    "indexed transcript owner is missing",
                )
            })?;
        if record.key != *key || record.subject != mounted_subject_id {
            return Err(Error::config(
                "conversation_recall_manifest",
                "transcript owner scope differs from the requested subject root",
            ));
        }
        Ok(Some(record))
    }

    fn list_turns(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        limit: usize,
    ) -> Result<Vec<TranscriptTurnRecord>> {
        let (_, manifest, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let Some(manifest) = manifest else {
            return Ok(Vec::new());
        };
        self.validate_conversation_manifest_subject(&manifest, mounted_subject_id)?;
        let mut records = Vec::new();
        for entry in &manifest.entries {
            if entry.kind != RecallIndexAddressKind::Json
                || entry.namespace != "conversation_transcript"
            {
                continue;
            }
            if let Some(record) =
                self.json_get::<TranscriptTurnRecord>("conversation_transcript", &entry.key)?
            {
                if record.key != *key
                    || record.subject != mounted_subject_id
                    || transcript_turn_storage_key(key, mounted_subject_id, &record.turn_id)
                        != entry.key
                {
                    return Err(Error::config(
                        "conversation_recall_manifest",
                        "transcript owner scope or storage key differs from its subject root",
                    ));
                }
                records.push(record);
            } else {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "indexed transcript owner is missing",
                ));
            }
        }
        records.sort_by(|left, right| {
            left.sequence
                .cmp(&right.sequence)
                .then_with(|| left.created_at.cmp(&right.created_at))
                .then_with(|| left.turn_id.cmp(&right.turn_id))
        });
        if limit > 0 && records.len() > limit {
            records = records[records.len() - limit..].to_vec();
        }
        Ok(records)
    }

    fn upsert_transcript_attrs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        attrs: &[TranscriptAttrEnvelope],
    ) -> Result<TranscriptAttrWriteReport> {
        let _transaction_guard = self.lock_transaction("conversation_recall_manifest_attrs")?;
        let (_, manifest, manifest_before) =
            self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let manifest = manifest.ok_or_else(|| {
            Error::config(
                "conversation_recall_manifest",
                "transcript attrs require an existing conversation recall manifest",
            )
        })?;
        self.validate_conversation_manifest_subject(&manifest, mounted_subject_id)?;
        let mut accepted_attrs = Vec::new();
        let mut rejected_attrs = Vec::new();
        let mut mutations = Vec::new();
        let mut entries = manifest.entries.clone();
        let mut seen_keys = BTreeSet::new();
        let mut owner_subject_id = None::<String>;
        for attr in attrs {
            if attr.target.key != *key {
                rejected_attrs.push(transcript_attr_rejection(
                    attr,
                    "attr target key does not match conversation key",
                ));
                continue;
            }
            let Some(turn) = self.get_turn(key, mounted_subject_id, &attr.target.turn_id)? else {
                rejected_attrs.push(transcript_attr_rejection(
                    attr,
                    "attr target turn does not exist",
                ));
                continue;
            };
            if let Err(error) = attr.validate_for_record(&turn) {
                rejected_attrs.push(transcript_attr_rejection(attr, error.to_string()));
                continue;
            }
            if owner_subject_id
                .as_ref()
                .is_some_and(|subject_id| subject_id != &turn.subject)
            {
                rejected_attrs.push(transcript_attr_rejection(
                    attr,
                    "attrs from different subject owners cannot share one transaction",
                ));
                continue;
            }
            owner_subject_id.get_or_insert_with(|| turn.subject.clone());
            let owner_key = transcript_attr_storage_key(key, mounted_subject_id, attr);
            if !seen_keys.insert(owner_key.clone()) {
                rejected_attrs.push(transcript_attr_rejection(
                    attr,
                    "duplicate attr storage key in one write",
                ));
                continue;
            }
            let value = serde_json::to_value(attr).map_err(|error| {
                Error::config("conversation_recall_manifest", error.to_string())
            })?;
            let address = RecallIndexAddress::json(
                "conversation_transcript_attr",
                &owner_key,
                next_entry_revision(
                    &entries,
                    RecallIndexAddressKind::Json,
                    "conversation_transcript_attr",
                    &owner_key,
                ),
                attr.created_at,
                &value,
            )?;
            entries = replace_recall_index_address(&entries, address);
            mutations.push(StoreMutation::PutJson {
                namespace: "conversation_transcript_attr".to_string(),
                key: owner_key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "conversation_transcript_attr".to_string(),
                record_key: owner_key,
            });
            accepted_attrs.push(attr.clone());
        }
        if !mutations.is_empty() {
            let index = self.plan_conversation_index(
                key,
                mounted_subject_id,
                Some(&manifest),
                manifest_before,
                entries,
            )?;
            let mut scope = self.recall_scope();
            scope.memory_space_id.clone_from(&key.memory_space_id);
            let owner_subject_id = owner_subject_id.expect("accepted attrs have a subject owner");
            if owner_subject_id != mounted_subject_id {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "transcript attr owner differs from the requested subject root",
                ));
            }
            scope.subject_id = owner_subject_id;
            scope.channel.clone_from(&key.channel_id);
            scope.conversation_id = Some(key.conversation_id.clone());
            self.commit_recall_indexed_mutations(
                "conversation.transcript_attrs.upsert",
                scope,
                mutations,
                vec![index],
            )?;
        }
        Ok(TranscriptAttrWriteReport {
            key: key.clone(),
            accepted_attrs,
            rejected_attrs,
        })
    }

    fn list_transcript_attrs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Vec<TranscriptAttrEnvelope>> {
        let (_, manifest, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let Some(manifest) = manifest else {
            return Ok(Vec::new());
        };
        self.validate_conversation_manifest_subject(&manifest, mounted_subject_id)?;
        let mut attrs = Vec::new();
        for entry in &manifest.entries {
            if entry.kind != RecallIndexAddressKind::Json
                || entry.namespace != "conversation_transcript_attr"
            {
                continue;
            }
            let Some(value) = self
                .engine
                .get_json_value("conversation_transcript_attr", &entry.key)?
            else {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "indexed transcript attr owner is missing",
                ));
            };
            let attr =
                serde_json::from_value::<TranscriptAttrEnvelope>(value).map_err(|error| {
                    Error::config("conversation_recall_manifest", error.to_string())
                })?;
            if attr.target.key != *key
                || transcript_attr_storage_key(key, mounted_subject_id, &attr) != entry.key
            {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "transcript attr owner scope or storage key differs from its subject root",
                ));
            }
            let turn = self
                .get_turn(key, mounted_subject_id, &attr.target.turn_id)?
                .ok_or_else(|| {
                    Error::config(
                        "conversation_recall_manifest",
                        "transcript attr target turn is missing",
                    )
                })?;
            attr.validate_for_record(&turn)?;
            if turn_id
                .map(|turn_id| attr.target.turn_id == turn_id)
                .unwrap_or(true)
            {
                attrs.push(attr);
            }
        }
        attrs.sort_by(|left, right| {
            left.target
                .turn_id
                .cmp(&right.target.turn_id)
                .then_with(|| left.target.message_id.cmp(&right.target.message_id))
                .then_with(|| left.created_at.cmp(&right.created_at))
                .then_with(|| left.attr_id.cmp(&right.attr_id))
        });
        Ok(attrs)
    }

    fn inspect_repair_records(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
    ) -> Result<TranscriptRepairInspection> {
        let (_, manifest, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let Some(manifest) = manifest else {
            return Ok(TranscriptRepairInspection::default());
        };
        if manifest.memory_space_id != key.memory_space_id
            || manifest.mounted_subject_id != mounted_subject_id
            || manifest.channel_id != key.channel_id
            || manifest.conversation_id != key.conversation_id
        {
            return Err(Error::config(
                "conversation_recall_manifest",
                "conversation repair manifest identity differs from the requested subject root",
            ));
        }
        let mut inspection = TranscriptRepairInspection::default();
        for entry in &manifest.entries {
            if entry.kind != RecallIndexAddressKind::Json {
                inspection.issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::CorruptRecord,
                    turn_id: String::new(),
                    message_id: None,
                    derived_ref: None,
                    reason: "conversation_manifest_contains_non_json_owner".to_string(),
                });
                continue;
            }
            let Some(value) = self.engine.get_json_value(&entry.namespace, &entry.key)? else {
                inspection.issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::CorruptRecord,
                    turn_id: String::new(),
                    message_id: None,
                    derived_ref: None,
                    reason: format!(
                        "conversation_manifest_owner_missing:{}:{}",
                        entry.namespace, entry.key
                    ),
                });
                continue;
            };
            match entry.namespace.as_str() {
                "conversation_transcript" => {
                    inspection.checked_turns = inspection.checked_turns.saturating_add(1);
                    match serde_json::from_value::<TranscriptTurnRecord>(value.clone()) {
                        Ok(record)
                            if record.key == *key
                                && record.subject == mounted_subject_id
                                && transcript_turn_storage_key(
                                    key,
                                    mounted_subject_id,
                                    &record.turn_id,
                                ) == entry.key =>
                        {
                            inspection.turns.push(record);
                        }
                        Ok(record) => inspection.issues.push(TranscriptRepairIssue {
                            kind: TranscriptRepairIssueKind::CorruptRecord,
                            turn_id: record.turn_id,
                            message_id: None,
                            derived_ref: None,
                            reason: "transcript_owner_scope_or_storage_key_mismatch".to_string(),
                        }),
                        Err(error) => inspection.issues.push(TranscriptRepairIssue {
                            kind: TranscriptRepairIssueKind::CorruptRecord,
                            turn_id: value
                                .get("turn_id")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            message_id: None,
                            derived_ref: None,
                            reason: format!("transcript_turn_decode_failed:{error}"),
                        }),
                    }
                }
                "conversation_transcript_attr" => {
                    inspection.checked_attrs = inspection.checked_attrs.saturating_add(1);
                    match serde_json::from_value::<TranscriptAttrEnvelope>(value.clone()) {
                        Ok(attr)
                            if attr.target.key == *key
                                && transcript_attr_storage_key(key, mounted_subject_id, &attr)
                                    == entry.key =>
                        {
                            inspection.attrs.push(attr);
                        }
                        Ok(attr) => inspection.issues.push(TranscriptRepairIssue {
                            kind: TranscriptRepairIssueKind::MismatchedAttrSourceKey,
                            turn_id: attr.target.turn_id,
                            message_id: attr.target.message_id,
                            derived_ref: None,
                            reason: "transcript_attr_owner_scope_or_storage_key_mismatch"
                                .to_string(),
                        }),
                        Err(error) => {
                            let (turn_id, message_id) =
                                transcript_attr_repair_target_from_value(&value);
                            inspection.issues.push(TranscriptRepairIssue {
                                kind: TranscriptRepairIssueKind::CorruptTranscriptAttrRecord,
                                turn_id,
                                message_id,
                                derived_ref: None,
                                reason: format!("transcript_attr_decode_failed:{error}"),
                            });
                        }
                    }
                }
                "conversation_transcript_derived_ref" => {
                    inspection.checked_derived_refs =
                        inspection.checked_derived_refs.saturating_add(1);
                    match serde_json::from_value::<DerivedMemoryRef>(value) {
                        Ok(derived)
                            if validate_derived_ref_matches_key(key, &derived).is_ok()
                                && exact_derived_owner_subject(&derived).ok()
                                    == Some(mounted_subject_id)
                                && transcript_derived_ref_storage_key(
                                    key,
                                    mounted_subject_id,
                                    &derived,
                                )
                                .ok()
                                .as_deref()
                                    == Some(entry.key.as_str()) =>
                        {
                            inspection.derived_refs.push(derived);
                        }
                        Ok(derived) => inspection.issues.push(TranscriptRepairIssue {
                            kind: TranscriptRepairIssueKind::MismatchedSourceKey,
                            turn_id: derived.source.turn_id.clone(),
                            message_id: derived.source.message_id.clone(),
                            derived_ref: Some(derived),
                            reason: "derived_memory_ref_owner_scope_or_storage_key_mismatch"
                                .to_string(),
                        }),
                        Err(error) => inspection.issues.push(TranscriptRepairIssue {
                            kind: TranscriptRepairIssueKind::CorruptRecord,
                            turn_id: String::new(),
                            message_id: None,
                            derived_ref: None,
                            reason: format!("derived_memory_ref_decode_failed:{error}"),
                        }),
                    }
                }
                _ => inspection.issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::CorruptRecord,
                    turn_id: String::new(),
                    message_id: None,
                    derived_ref: None,
                    reason: format!(
                        "conversation_manifest_contains_non_conversation_owner:{}",
                        entry.namespace
                    ),
                }),
            }
        }
        Ok(inspection)
    }

    fn append_derived_memory_ref(
        &self,
        key: &ConversationKey,
        derived: &DerivedMemoryRef,
    ) -> Result<()> {
        validate_derived_ref_matches_key(key, derived)?;
        let subject_id = exact_derived_owner_subject(derived)?;
        let record_key = transcript_derived_ref_storage_key(key, subject_id, derived)?;
        let value = serde_json::to_value(derived).map_err(|error| {
            Error::config("conversation_transcript_derived_ref", error.to_string())
        })?;
        let mut scope = self.recall_scope();
        scope.memory_space_id.clone_from(&key.memory_space_id);
        scope.subject_id = subject_id.to_string();
        scope.channel.clone_from(&key.channel_id);
        scope.conversation_id = Some(key.conversation_id.clone());
        self.commit_governed_memory_transaction_authorized(
            StoreMutationBatch {
                transaction_id: format!("conversation-derived-ref:{}", current_unix_nanos()),
                operation: "conversation.derived_ref.append".to_string(),
                scope,
                mutations: vec![StoreMutation::PutJson {
                    namespace: "conversation_transcript_derived_ref".to_string(),
                    key: record_key.clone(),
                    value,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "conversation_transcript_derived_ref".to_string(),
                    record_key,
                }],
            },
            &[],
            None,
            None,
            Some(derived.created_at),
        )?;
        Ok(())
    }

    fn list_derived_memory_refs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Vec<DerivedMemoryRef>> {
        let (_, manifest, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let Some(manifest) = manifest else {
            return Ok(Vec::new());
        };
        self.validate_conversation_manifest_subject(&manifest, mounted_subject_id)?;
        let mut refs = Vec::new();
        for entry in &manifest.entries {
            if entry.kind != RecallIndexAddressKind::Json
                || entry.namespace != "conversation_transcript_derived_ref"
            {
                continue;
            }
            let Some(derived) = self
                .json_get::<DerivedMemoryRef>("conversation_transcript_derived_ref", &entry.key)?
            else {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "indexed transcript derived owner is missing",
                ));
            };
            validate_derived_ref_matches_key(key, &derived)?;
            if exact_derived_owner_subject(&derived)? != mounted_subject_id
                || transcript_derived_ref_storage_key(key, mounted_subject_id, &derived)?
                    != entry.key
            {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "transcript derived owner scope or storage key differs from its subject root",
                ));
            }
            if turn_id
                .map(|turn_id| derived.source.turn_id == turn_id)
                .unwrap_or(true)
            {
                refs.push(derived);
            }
        }
        refs.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.store_key.cmp(&right.store_key))
                .then_with(|| left.source.turn_id.cmp(&right.source.turn_id))
                .then_with(|| left.source.message_id.cmp(&right.source.message_id))
        });
        Ok(refs)
    }

    fn apply_lifecycle_request(
        &self,
        mounted_subject_id: &str,
        request: &TranscriptLifecycleRequest,
    ) -> Result<TranscriptLifecycleReport> {
        let _transaction_guard = self.lock_transaction("conversation_recall_manifest_lifecycle")?;
        let (_, manifest, manifest_before) =
            self.load_conversation_recall_manifest(&request.key, mounted_subject_id)?;
        let manifest = manifest.ok_or_else(|| {
            Error::config(
                "conversation_recall_manifest",
                "transcript lifecycle transition requires its typed recall manifest",
            )
        })?;
        self.validate_conversation_manifest_subject(&manifest, mounted_subject_id)?;
        let mut affected_turns = 0usize;
        let mut affected_turn_ids = Vec::new();
        let mut affected_message_ids = Vec::new();
        let mut affected_host_refs = Vec::new();
        let mut mutations = Vec::new();
        let mut entries = manifest.entries.clone();
        let mut records = self.list_turns(&request.key, mounted_subject_id, usize::MAX)?;
        let mut owner_subject_id = None::<String>;
        for record in &mut records {
            let matches_turn = request
                .turn_id
                .as_ref()
                .map(|turn_id| turn_id == &record.turn_id)
                .unwrap_or(true);
            if matches_turn {
                if owner_subject_id
                    .as_ref()
                    .is_some_and(|subject_id| subject_id != &record.subject)
                {
                    return Err(Error::config(
                        "conversation_recall_manifest",
                        "one conversation manifest cannot contain multiple subject owners",
                    ));
                }
                owner_subject_id.get_or_insert_with(|| record.subject.clone());
                affected_turn_ids.push(record.turn_id.clone());
                for message in &record.input_messages {
                    affected_message_ids.push(message.message_id.clone());
                }
                if let Some(message) = &record.assistant_message {
                    affected_message_ids.push(message.message_id.clone());
                }
                affected_host_refs.extend(record.host_refs.clone());
                record.apply_lifecycle_transition(request.transition, request.requested_at);
                let record_key =
                    transcript_turn_storage_key(&request.key, mounted_subject_id, &record.turn_id);
                let value = serde_json::to_value(&*record).map_err(|error| {
                    Error::config("conversation_recall_manifest", error.to_string())
                })?;
                let address = RecallIndexAddress::json(
                    "conversation_transcript",
                    &record_key,
                    next_entry_revision(
                        &entries,
                        RecallIndexAddressKind::Json,
                        "conversation_transcript",
                        &record_key,
                    ),
                    record.updated_at,
                    &value,
                )?;
                entries = replace_recall_index_address(&entries, address);
                mutations.push(StoreMutation::PutJson {
                    namespace: "conversation_transcript".to_string(),
                    key: record_key.clone(),
                    value,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "conversation_transcript".to_string(),
                    record_key,
                });
                affected_turns = affected_turns.saturating_add(1);
            }
        }
        if !mutations.is_empty() {
            let index = self.plan_conversation_index(
                &request.key,
                mounted_subject_id,
                Some(&manifest),
                manifest_before,
                entries,
            )?;
            let mut scope = self.recall_scope();
            scope
                .memory_space_id
                .clone_from(&request.key.memory_space_id);
            scope.subject_id = owner_subject_id.expect("affected turns have a subject owner");
            scope.channel.clone_from(&request.key.channel_id);
            scope.conversation_id = Some(request.key.conversation_id.clone());
            self.commit_recall_indexed_mutations(
                "conversation.transcript.lifecycle",
                scope,
                mutations,
                vec![index],
            )?;
        }
        let derived_memory_refs = self
            .list_derived_memory_refs(&request.key, mounted_subject_id, None)?
            .into_iter()
            .filter(|derived| affected_turn_ids.contains(&derived.source.turn_id))
            .collect::<Vec<_>>();
        Ok(TranscriptLifecycleReport {
            key: request.key.clone(),
            transition: request.transition,
            affected_turns,
            affected_turn_ids,
            affected_message_ids,
            affected_host_refs,
            redacted_host_refs: 0,
            host_ref_redactions: Vec::new(),
            derived_memory_refs,
            profile_budget_applied: false,
            reason: request.reason.clone(),
            requested_by: request.requested_by.clone(),
            requested_at: request.requested_at,
        })
    }
}

fn validate_derived_ref_matches_key(
    key: &ConversationKey,
    derived: &DerivedMemoryRef,
) -> Result<()> {
    if derived.source.memory_space_id != key.memory_space_id
        || derived.source.channel_id != key.channel_id
        || derived.source.conversation_id != key.conversation_id
    {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref source does not match conversation key",
        ));
    }
    if derived.source.turn_id.trim().is_empty() {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref turn_id must not be empty",
        ));
    }
    if derived.store_key.trim().is_empty() {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref store_key must not be empty",
        ));
    }
    Ok(())
}

pub(crate) fn validate_conversation_recall_owner_for_scope(
    namespace: &str,
    physical_key: &str,
    value: &serde_json::Value,
    memory_space_id: &str,
    mounted_subject_id: &str,
) -> Result<()> {
    let valid = match namespace {
        "conversation_transcript" => {
            let record =
                serde_json::from_value::<TranscriptTurnRecord>(value.clone()).map_err(|error| {
                    Error::config(
                        "typed_recall_manifest_closure",
                        format!("conversation turn decode failed: {error}"),
                    )
                })?;
            record.key.memory_space_id == memory_space_id
                && record.subject == mounted_subject_id
                && transcript_turn_storage_key(&record.key, mounted_subject_id, &record.turn_id)
                    == physical_key
        }
        "conversation_transcript_attr" => {
            let attr = serde_json::from_value::<TranscriptAttrEnvelope>(value.clone()).map_err(
                |error| {
                    Error::config(
                        "typed_recall_manifest_closure",
                        format!("conversation attr decode failed: {error}"),
                    )
                },
            )?;
            attr.validate()?;
            attr.target.key.memory_space_id == memory_space_id
                && transcript_attr_storage_key(&attr.target.key, mounted_subject_id, &attr)
                    == physical_key
        }
        "conversation_transcript_derived_ref" => {
            let derived =
                serde_json::from_value::<DerivedMemoryRef>(value.clone()).map_err(|error| {
                    Error::config(
                        "typed_recall_manifest_closure",
                        format!("conversation derived-ref decode failed: {error}"),
                    )
                })?;
            let key = ConversationKey::new(
                &derived.source.memory_space_id,
                &derived.source.channel_id,
                &derived.source.conversation_id,
            )?;
            validate_derived_ref_matches_key(&key, &derived)?;
            key.memory_space_id == memory_space_id
                && exact_derived_owner_subject(&derived)? == mounted_subject_id
                && transcript_derived_ref_storage_key(&key, mounted_subject_id, &derived)?
                    == physical_key
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Error::config(
            "typed_recall_manifest_closure",
            "conversation recall owner key or exact subject scope is not canonical",
        ))
    }
}

fn exact_derived_owner_subject(derived: &DerivedMemoryRef) -> Result<&str> {
    let subject_id = derived
        .subject_id
        .as_deref()
        .or(derived.source.subject_id.as_deref())
        .ok_or_else(|| {
            Error::config(
                "conversation_transcript_derived_ref",
                "derived memory ref requires an exact subject owner",
            )
        })?;
    if derived
        .subject_id
        .as_deref()
        .zip(derived.source.subject_id.as_deref())
        .is_some_and(|(owner, source_owner)| owner != source_owner)
    {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref owner and source subject differ",
        ));
    }
    Ok(subject_id)
}

fn transcript_owner_storage_key_prefix(key: &ConversationKey, mounted_subject_id: &str) -> String {
    format!(
        "{}__subject__{}__",
        key.storage_key(),
        stable_hash_hex(mounted_subject_id)
    )
}

pub(crate) fn transcript_turn_storage_key(
    key: &ConversationKey,
    mounted_subject_id: &str,
    turn_id: &str,
) -> String {
    format!(
        "{}turn__{}",
        transcript_owner_storage_key_prefix(key, mounted_subject_id),
        stable_hash_hex(turn_id)
    )
}

fn transcript_derived_ref_storage_key_prefix(
    key: &ConversationKey,
    mounted_subject_id: &str,
) -> String {
    format!(
        "{}derived_ref__",
        transcript_owner_storage_key_prefix(key, mounted_subject_id)
    )
}

pub(crate) fn transcript_derived_ref_storage_key(
    key: &ConversationKey,
    mounted_subject_id: &str,
    derived: &DerivedMemoryRef,
) -> Result<String> {
    let payload = serde_json::to_string(derived)
        .map_err(|error| Error::config("conversation_transcript_derived_ref", error.to_string()))?;
    Ok(format!(
        "{}{}",
        transcript_derived_ref_storage_key_prefix(key, mounted_subject_id),
        stable_hash_hex(&payload)
    ))
}

fn transcript_attr_rejection(
    attr: &TranscriptAttrEnvelope,
    reason: impl Into<String>,
) -> TranscriptAttrWriteRejection {
    TranscriptAttrWriteRejection {
        attr_id: attr.attr_id.clone(),
        attr_key: attr.key.clone(),
        reason: reason.into(),
    }
}

fn transcript_attr_storage_key_prefix(key: &ConversationKey, mounted_subject_id: &str) -> String {
    format!(
        "{}attr__",
        transcript_owner_storage_key_prefix(key, mounted_subject_id)
    )
}

fn transcript_attr_storage_key(
    key: &ConversationKey,
    mounted_subject_id: &str,
    attr: &TranscriptAttrEnvelope,
) -> String {
    format!(
        "{}{}",
        transcript_attr_storage_key_prefix(key, mounted_subject_id),
        stable_hash_hex(&attr.attr_id)
    )
}

fn transcript_attr_repair_target_from_value(value: &serde_json::Value) -> (String, Option<String>) {
    let Some(target) = value.get("target") else {
        return ("*".to_string(), None);
    };
    let turn_id = target
        .get("turn_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("*")
        .to_string();
    let message_id = target
        .get("message_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    (turn_id, message_id)
}

impl ScopedLongTermMemoryStore {
    fn physical_key(&self, logical_owner_id: &str) -> Result<String> {
        scoped_long_term_memory_storage_key(&self.memory_space_id, logical_owner_id)
    }
}

impl bm_core::memory::LongTermMemoryReadStore for ScopedLongTermMemoryStore {
    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = bm_core::memory::LongTermMemoryReadStore::list(self, usize::MAX)?;
        let query = query.trim().to_lowercase();
        if !query.is_empty() {
            entries.retain(|entry| {
                entry.topic.to_lowercase().contains(&query)
                    || entry.content.to_lowercase().contains(&query)
                    || entry
                        .keywords
                        .iter()
                        .any(|keyword| keyword.to_lowercase().contains(&query))
            });
        }
        entries.sort_by(|left, right| {
            let left_scope = usize::from(left.source_chat_id.as_deref() == source_chat_id);
            let right_scope = usize::from(right.source_chat_id.as_deref() == source_chat_id);
            right_scope
                .cmp(&left_scope)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
        });
        if limit > 0 {
            entries.truncate(limit);
        }
        Ok(entries)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        self.platform.json_get("long_term", &self.physical_key(id)?)
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = Vec::new();
        for key in self.platform.engine.list_json_keys("long_term")? {
            if !key.starts_with(&self.key_prefix) {
                continue;
            }
            let Some(entry) = self
                .platform
                .json_get::<LongTermMemoryEntry>("long_term", &key)?
            else {
                continue;
            };
            if self.physical_key(&entry.id)? != key {
                return Err(Error::config(
                    "long_term_storage_scope",
                    "scoped long-term owner key does not match logical owner id",
                ));
            }
            entries.push(entry);
        }
        entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        if limit > 0 {
            entries.truncate(limit);
        }
        Ok(entries)
    }

    fn count(&self) -> Result<usize> {
        Ok(self
            .platform
            .engine
            .list_json_keys("long_term")?
            .into_iter()
            .filter(|key| key.starts_with(&self.key_prefix))
            .count())
    }
}

impl ScopedLongTermMemoryControlReadStore {
    fn list_scoped<T>(&self, namespace: &str) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let manifest_key =
            control_plane_scope_manifest_key(&self.memory_space_id, &self.mounted_subject_id)?;
        let mut session = self
            .platform
            .engine
            .open_immutable_read_session(self.platform.capacity)?;
        let manifest_reads = session.read_json_known_keys(&[(
            CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
            manifest_key.clone(),
        )])?;
        let manifest_value = manifest_reads
            .into_iter()
            .next()
            .and_then(|read| read.value);
        let Some(manifest_value) = manifest_value else {
            return Ok(Vec::new());
        };
        let manifest = serde_json::from_value::<ControlPlaneScopeManifest>(manifest_value)
            .map_err(|error| {
                Error::config(
                    "control_plane_scope_manifest",
                    format!("control read manifest decode failed: {error}"),
                )
            })?;
        manifest.validate(self.platform.capacity.kv_max_entries)?;
        if manifest.physical_key != manifest_key
            || manifest.memory_space_id != self.memory_space_id
            || manifest.mounted_subject_id != self.mounted_subject_id
        {
            return Err(Error::config(
                "control_plane_scope_manifest",
                "control read manifest differs from the exact mounted subject scope",
            ));
        }
        let entries = manifest
            .entries
            .iter()
            .filter(|entry| entry.namespace == namespace)
            .collect::<Vec<_>>();
        let addresses = entries
            .iter()
            .map(|entry| (entry.namespace.clone(), entry.key.clone()))
            .collect::<Vec<_>>();
        let reads = session.read_json_known_keys(&addresses)?;
        if reads.len() != entries.len() {
            return Err(Error::config(
                "control_plane_scope_manifest",
                "bounded control read returned the wrong address count",
            ));
        }
        reads
            .into_iter()
            .zip(entries)
            .map(|(read, entry)| {
                if read.namespace != entry.namespace || read.key != entry.key {
                    return Err(Error::config(
                        "control_plane_scope_manifest",
                        "bounded control read returned a wrong address",
                    ));
                }
                let value = read.value.ok_or_else(|| {
                    Error::config(
                        "control_plane_scope_manifest",
                        "manifested control document is missing",
                    )
                })?;
                entry.validate_value(&value)?;
                validate_control_document_for_scope(
                    namespace,
                    &entry.key,
                    &value,
                    &self.memory_space_id,
                    &self.mounted_subject_id,
                )?;
                serde_json::from_value(value).map_err(|error| {
                    Error::config("control_plane_scope_manifest", error.to_string())
                })
            })
            .collect()
    }
}

impl LongTermMemoryControlReadStore for ScopedLongTermMemoryControlReadStore {
    fn list_long_term_control_revisions(
        &self,
        record_id: &str,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlRevision>> {
        let mut revisions = self
            .list_scoped::<LongTermMemoryControlRevision>(LONG_TERM_CONTROL_REVISION_NAMESPACE)?;
        revisions.retain(|revision| revision.record_id == record_id);
        revisions.sort_by(|left, right| {
            right
                .owner_revision
                .cmp(&left.owner_revision)
                .then_with(|| right.created_at.cmp(&left.created_at))
        });
        revisions.truncate(limit);
        Ok(revisions)
    }

    fn get_long_term_control_tombstone(
        &self,
        record_id: &str,
    ) -> Result<Option<LongTermMemoryTombstone>> {
        Ok(self
            .list_scoped::<LongTermMemoryTombstone>(LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE)?
            .into_iter()
            .find(|tombstone| tombstone.record_id == record_id))
    }

    fn list_long_term_control_tombstones(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryTombstone>> {
        let mut tombstones =
            self.list_scoped::<LongTermMemoryTombstone>(LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE)?;
        tombstones.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        tombstones.truncate(limit);
        Ok(tombstones)
    }

    fn list_long_term_governance_policies(
        &self,
        limit: usize,
    ) -> Result<Vec<MemoryLongTermGovernancePolicy>> {
        let mut policies = self
            .list_scoped::<MemoryLongTermGovernancePolicy>(LONG_TERM_GOVERNANCE_POLICY_NAMESPACE)?;
        policies.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        policies.truncate(limit);
        Ok(policies)
    }

    fn list_long_term_control_audit(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlAuditEvent>> {
        let mut events =
            self.list_scoped::<LongTermMemoryControlAuditEvent>(LONG_TERM_CONTROL_AUDIT_NAMESPACE)?;
        events.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        events.truncate(limit);
        Ok(events)
    }
}

impl ContinuityCapsuleStore for StorePlatform {
    fn upsert_many(
        &self,
        drafts: &[ContinuityCapsuleDraft],
        now_secs: u64,
    ) -> Result<ContinuityCapsuleWriteOutcome> {
        let _transaction_guard = self.lock_transaction("continuity_capsule_scope_index_upsert")?;
        let mut upserted = 0usize;
        for draft in drafts {
            let capsule_id = stable_hash_id(
                "cc",
                &(
                    draft.kind.label(),
                    draft.scope_kind.label(),
                    &draft.scope_id,
                    &draft.topic,
                ),
            );
            let capsule = ContinuityCapsule {
                capsule_id: capsule_id.clone(),
                kind: draft.kind,
                scope_kind: draft.scope_kind,
                scope_id: draft.scope_id.clone(),
                source_chat_id: draft.source_chat_id.clone(),
                source_channel: draft.source_channel.clone(),
                run_id: draft.run_id.clone(),
                topic: draft.topic.clone(),
                summary: draft.summary.clone(),
                outcome: draft.outcome.clone(),
                decisions: draft.decisions.clone(),
                next_step: draft.next_step.clone(),
                unresolved: draft.unresolved.clone(),
                artifact_refs: draft.artifact_refs.clone(),
                provenance_refs: draft.provenance_refs.clone(),
                source: draft.source,
                status: draft.status,
                supersedes: Vec::new(),
                observed_at: if draft.observed_at > 0 {
                    draft.observed_at
                } else {
                    now_secs
                },
                updated_at: now_secs,
            };
            if capsule.is_meaningful() {
                let scope_kind = capsule.scope_kind.label();
                let manifest_key = ContinuityCapsuleScopeIndex::build(
                    1,
                    &self.config.event_scope.memory_space_id,
                    scope_kind,
                    &capsule.scope_id,
                    std::iter::empty(),
                )?
                .physical_key;
                let (previous, before) =
                    self.load_typed_recall_index::<ContinuityCapsuleScopeIndex>(&manifest_key)?;
                if self
                    .engine
                    .get_json_value("continuity_capsule", &capsule_id)?
                    .is_some()
                    && previous.is_none()
                {
                    return Err(Error::config(
                        "continuity_capsule_scope_index",
                        "capsule exists without its required scope index",
                    ));
                }
                let previous_entries = previous
                    .as_ref()
                    .map(|index| index.entries.as_slice())
                    .unwrap_or(&[]);
                let value = serde_json::to_value(&capsule).map_err(|error| {
                    Error::config("continuity_capsule_scope_index", error.to_string())
                })?;
                let address = RecallIndexAddress::json(
                    "continuity_capsule",
                    &capsule_id,
                    next_entry_revision(
                        previous_entries,
                        RecallIndexAddressKind::Json,
                        "continuity_capsule",
                        &capsule_id,
                    ),
                    capsule.updated_at,
                    &value,
                )?;
                let next = ContinuityCapsuleScopeIndex::build(
                    previous
                        .as_ref()
                        .map(|index| index.revision.saturating_add(1))
                        .unwrap_or(1),
                    &self.config.event_scope.memory_space_id,
                    scope_kind,
                    &capsule.scope_id,
                    replace_recall_index_address(previous_entries, address),
                )?;
                let index = (
                    ContinuityCapsuleScopeIndex::NAMESPACE,
                    manifest_key,
                    serde_json::to_value(next).map_err(|error| {
                        Error::config("continuity_capsule_scope_index", error.to_string())
                    })?,
                    before,
                );
                let mut event_scope = self.recall_scope();
                event_scope.channel.clone_from(&capsule.source_channel);
                event_scope.chat_id.clone_from(&capsule.source_chat_id);
                self.commit_recall_indexed_mutations(
                    "continuity_capsule.upsert",
                    event_scope,
                    vec![StoreMutation::PutJson {
                        namespace: "continuity_capsule".to_string(),
                        key: capsule_id.clone(),
                        value,
                        event_kind: MemoryStoreEventKind::MemoryWrite,
                        plane: "continuity_capsule".to_string(),
                        record_key: capsule_id.clone(),
                    }],
                    vec![index],
                )?;
                upserted = upserted.saturating_add(1);
            }
        }
        Ok(ContinuityCapsuleWriteOutcome {
            considered: drafts.len(),
            upserted,
            superseded: 0,
            total: ContinuityCapsuleStore::count(self)?,
        })
    }

    fn get(&self, capsule_id: &str) -> Result<Option<ContinuityCapsule>> {
        self.json_get("continuity_capsule", capsule_id)
    }

    fn list(&self, limit: usize) -> Result<Vec<ContinuityCapsule>> {
        let mut items = self.json_list::<ContinuityCapsule>("continuity_capsule", limit)?;
        items.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        items.truncate(limit);
        Ok(items)
    }

    fn count(&self) -> Result<usize> {
        self.engine
            .list_json_keys("continuity_capsule")
            .map(|keys| keys.len())
    }
}

impl TurnContinuityEvidenceStore for StorePlatform {
    fn append(&self, chat_id: &str, evidence: &TurnContinuityEvidence) -> Result<()> {
        let mut items = self
            .json_get::<Vec<TurnContinuityEvidence>>("turn_continuity_evidence", chat_id)?
            .unwrap_or_default();
        items.push(evidence.clone());
        if items.len() > TURN_CONTINUITY_EVIDENCE_HISTORY_MAX_ITEMS {
            let remove_count = items.len() - TURN_CONTINUITY_EVIDENCE_HISTORY_MAX_ITEMS;
            items.drain(0..remove_count);
        }
        self.json_put("turn_continuity_evidence", chat_id, &items)
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.json_delete("turn_continuity_evidence", chat_id)
            .map(|_| ())
    }

    fn list_recent(&self, chat_id: &str, limit: usize) -> Result<Vec<TurnContinuityEvidence>> {
        let mut items = self
            .json_get::<Vec<TurnContinuityEvidence>>("turn_continuity_evidence", chat_id)?
            .unwrap_or_default();
        items.reverse();
        items.truncate(limit);
        Ok(items)
    }
}

impl PrivateGardenStore for StorePlatform {
    fn list(&self, chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
        let prefix = private_garden_key_prefix(chat_id);
        let mut docs = Vec::new();
        for key in self.engine.list_json_keys("private_garden")? {
            if !key.starts_with(&prefix) {
                continue;
            }
            if let Some(doc) = self.json_get::<PrivateGardenDoc>("private_garden", &key)? {
                docs.push(private_garden_record(&doc));
            }
        }
        docs.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        docs.truncate(limit);
        Ok(docs)
    }

    fn read(&self, chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
        let path = normalize_private_garden_doc_path(doc_path)?;
        self.json_get("private_garden", &private_garden_key(chat_id, &path))
    }

    fn write(
        &self,
        chat_id: &str,
        doc_path: &str,
        content: &str,
        now_secs: u64,
    ) -> Result<PrivateGardenDocRecord> {
        if content.len() > PRIVATE_GARDEN_MAX_DOC_BYTES {
            return Err(Error::config(
                "private_garden_write",
                format!(
                    "document size {} exceeds {}",
                    content.len(),
                    PRIVATE_GARDEN_MAX_DOC_BYTES
                ),
            ));
        }
        let path = normalize_private_garden_doc_path(doc_path)?;
        let key = private_garden_key(chat_id, &path);
        let revision = self
            .json_get::<PrivateGardenDoc>("private_garden", &key)?
            .map(|doc| doc.revision.saturating_add(1))
            .unwrap_or(1);
        let doc = PrivateGardenDoc {
            path,
            content: content.to_string(),
            updated_at: now_secs,
            revision,
        };
        let record = private_garden_record(&doc);
        self.json_put("private_garden", &key, &doc)?;
        Ok(record)
    }

    fn move_doc(
        &self,
        chat_id: &str,
        from_path: &str,
        to_path: &str,
        now_secs: u64,
    ) -> Result<Option<PrivateGardenDocRecord>> {
        let from = normalize_private_garden_doc_path(from_path)?;
        let to = normalize_private_garden_doc_path(to_path)?;
        let Some(doc) = PrivateGardenStore::read(self, chat_id, &from)? else {
            return Ok(None);
        };
        PrivateGardenStore::delete(self, chat_id, &from)?;
        Ok(Some(PrivateGardenStore::write(
            self,
            chat_id,
            &to,
            &doc.content,
            now_secs,
        )?))
    }

    fn delete(&self, chat_id: &str, doc_path: &str) -> Result<bool> {
        let path = normalize_private_garden_doc_path(doc_path)?;
        self.json_delete("private_garden", &private_garden_key(chat_id, &path))
    }
}

impl RemindAtStore for StorePlatform {
    fn get(
        &self,
        channel: &str,
        chat_id: &str,
        id: &str,
    ) -> Result<Option<bm_core::reminder::ReminderItem>> {
        self.json_get("remind_at", &triple_key(channel, chat_id, id))
    }

    fn upsert(&self, reminder: &bm_core::reminder::ReminderItem) -> Result<()> {
        self.json_put(
            "remind_at",
            &triple_key(&reminder.channel, &reminder.chat_id, &reminder.id),
            reminder,
        )
    }

    fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool> {
        self.json_delete("remind_at", &triple_key(channel, chat_id, id))
    }

    fn list_due(
        &self,
        now_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<bm_core::reminder::ReminderItem>> {
        let mut items =
            self.json_list::<bm_core::reminder::ReminderItem>("remind_at", usize::MAX)?;
        items.retain(|item| item.at_unix_secs > 0 && item.at_unix_secs <= now_unix_secs);
        items.sort_by(|left, right| left.at_unix_secs.cmp(&right.at_unix_secs));
        items.truncate(limit);
        Ok(items)
    }

    fn delete_due(&self, reminder: &bm_core::reminder::ReminderItem) -> Result<bool> {
        self.json_delete(
            "remind_at",
            &triple_key(&reminder.channel, &reminder.chat_id, &reminder.id),
        )
    }

    fn next_due_at(&self) -> Result<Option<u64>> {
        let mut due_times = self
            .json_list::<bm_core::reminder::ReminderItem>("remind_at", usize::MAX)?
            .into_iter()
            .filter_map(|item| (item.at_unix_secs > 0).then_some(item.at_unix_secs))
            .collect::<Vec<_>>();
        due_times.sort_unstable();
        Ok(due_times.into_iter().next())
    }

    fn list_upcoming(
        &self,
        channel: &str,
        chat_id: &str,
        now_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<bm_core::reminder::ReminderItem>> {
        let mut items =
            self.json_list::<bm_core::reminder::ReminderItem>("remind_at", usize::MAX)?;
        items.retain(|item| {
            item.channel == channel && item.chat_id == chat_id && item.at_unix_secs > now_unix_secs
        });
        items.sort_by(|left, right| left.at_unix_secs.cmp(&right.at_unix_secs));
        items.truncate(limit);
        Ok(items)
    }
}

impl TaskStore for StorePlatform {
    fn list(&self, channel: &str, chat_id: &str, query: TaskQuery) -> Result<Vec<TaskItem>> {
        let mut items = self.json_list::<TaskItem>("task", usize::MAX)?;
        items.retain(|task| {
            task.channel == channel
                && task.chat_id == chat_id
                && query.status.is_none_or(|status| task.status == status)
                && (query.include_completed || !task.status.is_terminal())
                && (query.project.trim().is_empty() || task.project == query.project)
        });
        items.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        items.truncate(query.limit);
        Ok(items)
    }

    fn get(&self, channel: &str, chat_id: &str, id: &str) -> Result<Option<TaskItem>> {
        self.json_get("task", &triple_key(channel, chat_id, id))
    }

    fn upsert(&self, task: &TaskItem) -> Result<()> {
        let task = normalize_task_item(task.clone())?;
        self.json_put(
            "task",
            &triple_key(&task.channel, &task.chat_id, &task.id),
            &task,
        )
    }

    fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool> {
        self.json_delete("task", &triple_key(channel, chat_id, id))
    }

    fn list_due_unnotified(&self, now_unix_secs: u64, limit: usize) -> Result<Vec<TaskItem>> {
        let mut items = self.json_list::<TaskItem>("task", usize::MAX)?;
        items.retain(|task| {
            task.due_at_unix_secs > 0
                && task.due_at_unix_secs <= now_unix_secs
                && task.due_notified_at_unix_secs == 0
                && !task.status.is_terminal()
        });
        items.sort_by(|left, right| left.due_at_unix_secs.cmp(&right.due_at_unix_secs));
        items.truncate(limit);
        Ok(items)
    }

    fn mark_due_notified(&self, task: &TaskItem, notified_at_unix_secs: u64) -> Result<bool> {
        let key = triple_key(&task.channel, &task.chat_id, &task.id);
        let Some(mut latest) = self.json_get::<TaskItem>("task", &key)? else {
            return Ok(false);
        };
        latest.due_notified_at_unix_secs = notified_at_unix_secs;
        self.json_put("task", &key, &latest)?;
        Ok(true)
    }

    fn next_due_at(&self) -> Result<Option<u64>> {
        let mut values = self
            .json_list::<TaskItem>("task", usize::MAX)?
            .into_iter()
            .filter_map(|task| {
                (task.due_at_unix_secs > 0
                    && task.due_notified_at_unix_secs == 0
                    && !task.status.is_terminal())
                .then_some(task.due_at_unix_secs)
            })
            .collect::<Vec<_>>();
        values.sort_unstable();
        Ok(values.into_iter().next())
    }
}

impl TaskRunStore for StorePlatform {
    fn get(&self, run_id: &str) -> Result<Option<TaskRunRecord>> {
        self.json_get("task_run", run_id)
    }

    fn upsert(&self, record: &TaskRunRecord) -> Result<()> {
        let _transaction_guard = self.lock_transaction("active_task_run_by_chat_index_upsert")?;
        let owner_key = record.run.run_id.clone();
        let previous_record = self.json_get::<TaskRunRecord>("task_run", &owner_key)?;
        let value = serde_json::to_value(record)
            .map_err(|error| Error::config("active_task_run_by_chat_index", error.to_string()))?;
        let mut indexes = Vec::new();
        if let Some(previous_record) = previous_record.as_ref().filter(|previous| {
            previous.run.source_channel != record.run.source_channel
                || previous.run.source_chat_id != record.run.source_chat_id
        }) {
            let old_key = ActiveTaskRunByChatIndex::build(
                1,
                &self.config.event_scope.memory_space_id,
                &previous_record.run.source_channel,
                &previous_record.run.source_chat_id,
                std::iter::empty(),
            )?
            .physical_key;
            let (old, before) =
                self.load_typed_recall_index::<ActiveTaskRunByChatIndex>(&old_key)?;
            let old = old.ok_or_else(|| {
                Error::config(
                    "active_task_run_by_chat_index",
                    "task run exists without its prior chat index",
                )
            })?;
            let next = ActiveTaskRunByChatIndex::build(
                old.revision.saturating_add(1),
                &self.config.event_scope.memory_space_id,
                &previous_record.run.source_channel,
                &previous_record.run.source_chat_id,
                remove_recall_index_address(
                    &old.entries,
                    RecallIndexAddressKind::Json,
                    "task_run",
                    &owner_key,
                ),
            )?;
            indexes.push((
                ActiveTaskRunByChatIndex::NAMESPACE,
                old_key,
                serde_json::to_value(next).map_err(|error| {
                    Error::config("active_task_run_by_chat_index", error.to_string())
                })?,
                before,
            ));
        }
        let new_key = ActiveTaskRunByChatIndex::build(
            1,
            &self.config.event_scope.memory_space_id,
            &record.run.source_channel,
            &record.run.source_chat_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (current, before) =
            self.load_typed_recall_index::<ActiveTaskRunByChatIndex>(&new_key)?;
        if previous_record.is_some() && indexes.is_empty() && current.is_none() {
            return Err(Error::config(
                "active_task_run_by_chat_index",
                "task run exists without its required chat index",
            ));
        }
        let current_entries = current
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let mut entries = remove_recall_index_address(
            current_entries,
            RecallIndexAddressKind::Json,
            "task_run",
            &owner_key,
        );
        if record.run.status.is_active() {
            let address = RecallIndexAddress::json(
                "task_run",
                &owner_key,
                next_entry_revision(
                    current_entries,
                    RecallIndexAddressKind::Json,
                    "task_run",
                    &owner_key,
                ),
                record.run.updated_at,
                &value,
            )?;
            entries = replace_recall_index_address(&entries, address);
        }
        let next = ActiveTaskRunByChatIndex::build(
            current
                .as_ref()
                .map(|index| index.revision.saturating_add(1))
                .unwrap_or(1),
            &self.config.event_scope.memory_space_id,
            &record.run.source_channel,
            &record.run.source_chat_id,
            entries,
        )?;
        indexes.push((
            ActiveTaskRunByChatIndex::NAMESPACE,
            new_key,
            serde_json::to_value(next).map_err(|error| {
                Error::config("active_task_run_by_chat_index", error.to_string())
            })?,
            before,
        ));
        let mut scope = self.recall_scope();
        scope.channel.clone_from(&record.run.source_channel);
        scope.chat_id.clone_from(&record.run.source_chat_id);
        self.commit_recall_indexed_mutations(
            "task_run.upsert",
            scope,
            vec![StoreMutation::PutJson {
                namespace: "task_run".to_string(),
                key: owner_key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "task_run".to_string(),
                record_key: owner_key,
            }],
            indexes,
        )?;
        Ok(())
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskRunRecord>> {
        let mut records = self.json_list::<TaskRunRecord>("task_run", usize::MAX)?;
        records.sort_by(|left, right| right.run.updated_at.cmp(&left.run.updated_at));
        records.truncate(limit);
        Ok(records)
    }

    fn list_active_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskRunRecord>> {
        let mut records = self.json_list::<TaskRunRecord>("task_run", usize::MAX)?;
        records.retain(|record| {
            record.run.source_channel == channel
                && record.run.source_chat_id == chat_id
                && record.run.status.is_active()
        });
        records.sort_by(|left, right| right.run.updated_at.cmp(&left.run.updated_at));
        records.truncate(limit);
        Ok(records)
    }
}

impl TaskArtifactStore for StorePlatform {
    fn put(&self, record: &TaskArtifactRecord) -> Result<()> {
        self.json_put(
            "task_artifact",
            &triple_key("", &record.artifact.run_id, &record.artifact.artifact_id),
            record,
        )
    }

    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskArtifactRecord>> {
        let mut records = self.json_list::<TaskArtifactRecord>("task_artifact", usize::MAX)?;
        records.retain(|record| record.artifact.run_id == run_id);
        records.sort_by(|left, right| right.artifact.created_at.cmp(&left.artifact.created_at));
        records.truncate(limit);
        Ok(records)
    }

    fn delete(&self, run_id: &str, artifact_id: &str) -> Result<bool> {
        self.json_delete("task_artifact", &triple_key("", run_id, artifact_id))
    }
}

impl TaskLearningStore for StorePlatform {
    fn get(&self, learning_id: &str) -> Result<Option<TaskLearningRecord>> {
        self.json_get("task_learning", learning_id)
    }

    fn upsert(&self, record: &TaskLearningRecord) -> Result<()> {
        let _transaction_guard = self.lock_transaction("task_learning_by_chat_index_upsert")?;
        let owner_key = record.learning_id.clone();
        let previous_record = self.json_get::<TaskLearningRecord>("task_learning", &owner_key)?;
        let value = serde_json::to_value(record)
            .map_err(|error| Error::config("task_learning_by_chat_index", error.to_string()))?;
        let mut indexes = Vec::new();
        if let Some(previous_record) = previous_record.as_ref().filter(|previous| {
            previous.source_channel != record.source_channel
                || previous.source_chat_id != record.source_chat_id
        }) {
            let old_key = TaskLearningByChatIndex::build(
                1,
                &self.config.event_scope.memory_space_id,
                &previous_record.source_channel,
                &previous_record.source_chat_id,
                std::iter::empty(),
            )?
            .physical_key;
            let (old, before) =
                self.load_typed_recall_index::<TaskLearningByChatIndex>(&old_key)?;
            let old = old.ok_or_else(|| {
                Error::config(
                    "task_learning_by_chat_index",
                    "task learning exists without its prior chat index",
                )
            })?;
            let next = TaskLearningByChatIndex::build(
                old.revision.saturating_add(1),
                &self.config.event_scope.memory_space_id,
                &previous_record.source_channel,
                &previous_record.source_chat_id,
                remove_recall_index_address(
                    &old.entries,
                    RecallIndexAddressKind::Json,
                    "task_learning",
                    &owner_key,
                ),
            )?;
            indexes.push((
                TaskLearningByChatIndex::NAMESPACE,
                old_key,
                serde_json::to_value(next).map_err(|error| {
                    Error::config("task_learning_by_chat_index", error.to_string())
                })?,
                before,
            ));
        }
        let new_key = TaskLearningByChatIndex::build(
            1,
            &self.config.event_scope.memory_space_id,
            &record.source_channel,
            &record.source_chat_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (current, before) =
            self.load_typed_recall_index::<TaskLearningByChatIndex>(&new_key)?;
        if previous_record.is_some() && indexes.is_empty() && current.is_none() {
            return Err(Error::config(
                "task_learning_by_chat_index",
                "task learning exists without its required chat index",
            ));
        }
        let current_entries = current
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "task_learning",
            &owner_key,
            next_entry_revision(
                current_entries,
                RecallIndexAddressKind::Json,
                "task_learning",
                &owner_key,
            ),
            record.observed_at,
            &value,
        )?;
        let next = TaskLearningByChatIndex::build(
            current
                .as_ref()
                .map(|index| index.revision.saturating_add(1))
                .unwrap_or(1),
            &self.config.event_scope.memory_space_id,
            &record.source_channel,
            &record.source_chat_id,
            replace_recall_index_address(current_entries, address),
        )?;
        indexes.push((
            TaskLearningByChatIndex::NAMESPACE,
            new_key,
            serde_json::to_value(next)
                .map_err(|error| Error::config("task_learning_by_chat_index", error.to_string()))?,
            before,
        ));
        let mut scope = self.recall_scope();
        scope.channel.clone_from(&record.source_channel);
        scope.chat_id.clone_from(&record.source_chat_id);
        self.commit_recall_indexed_mutations(
            "task_learning.upsert",
            scope,
            vec![StoreMutation::PutJson {
                namespace: "task_learning".to_string(),
                key: owner_key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "task_learning".to_string(),
                record_key: owner_key,
            }],
            indexes,
        )?;
        Ok(())
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        let mut records = self.json_list::<TaskLearningRecord>("task_learning", usize::MAX)?;
        records.sort_by(|left, right| right.observed_at.cmp(&left.observed_at));
        records.truncate(limit);
        Ok(records)
    }

    fn list_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskLearningRecord>> {
        let mut records = self.json_list::<TaskLearningRecord>("task_learning", usize::MAX)?;
        records
            .retain(|record| record.source_channel == channel && record.source_chat_id == chat_id);
        records.sort_by(|left, right| right.observed_at.cmp(&left.observed_at));
        records.truncate(limit);
        Ok(records)
    }

    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        let mut records = self.json_list::<TaskLearningRecord>("task_learning", usize::MAX)?;
        records.retain(|record| record.run_id == run_id);
        records.sort_by(|left, right| right.observed_at.cmp(&left.observed_at));
        records.truncate(limit);
        Ok(records)
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn canonical_transaction_timestamp(
    batch: &StoreMutationBatch,
    runtime_timestamp_unix_secs: Option<u64>,
) -> Result<u64> {
    let event_timestamp = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::AppendEvent { event } => Some(event.timestamp_unix_secs),
            _ => None,
        })
        .try_fold(None, |observed, timestamp| match observed {
            Some(existing) if existing != timestamp => Err(Error::config(
                "memory_write_transaction_timestamp",
                "transaction events do not share one canonical runtime timestamp",
            )),
            Some(existing) => Ok(Some(existing)),
            None => Ok(Some(timestamp)),
        })?;
    match (runtime_timestamp_unix_secs, event_timestamp) {
        (Some(runtime), Some(event)) if runtime != event => Err(Error::config(
            "memory_write_transaction_timestamp",
            "explicit runtime clock does not match the transaction event timestamp",
        )),
        (Some(runtime), _) => Ok(runtime),
        (None, Some(event)) => Ok(event),
        (None, None)
            if batch.mutations.iter().any(|mutation| {
                matches!(mutation,
                    StoreMutation::PutBlob { namespace, .. }
                    | StoreMutation::DeleteBlob { namespace, .. }
                        if namespace == "skills")
            }) =>
        {
            Err(Error::config(
                "runtime_skill_recall_manifest",
                "skill mutation requires an explicit runtime or typed owner timestamp",
            ))
        }
        (None, None) => Ok(current_unix_secs()),
    }
}

fn current_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(feature = "sqlite-store")]
fn sqlite_engine(
    config: &StoreBackendConfig,
    capacity: StoreCapacityBudget,
    admission_authority: StoreAdmissionAuthority,
) -> Result<(Arc<dyn StoreEngine>, StoreSchemaManifest)> {
    let (engine, manifest) =
        SqliteStoreEngine::open_with_capacity_and_authority(config, capacity, admission_authority)?;
    Ok((Arc::new(engine), manifest))
}

#[cfg(not(feature = "sqlite-store"))]
fn sqlite_engine(
    _config: &StoreBackendConfig,
    _capacity: StoreCapacityBudget,
    _admission_authority: StoreAdmissionAuthority,
) -> Result<(Arc<dyn StoreEngine>, StoreSchemaManifest)> {
    Err(Error::config(
        "store_platform_open",
        "sqlite store backend requires sqlite-store feature",
    ))
}

fn next_event_id() -> String {
    let sequence = EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(
        "evt_{:032x}_{:08x}_{sequence:016x}",
        current_unix_nanos(),
        std::process::id()
    )
}

fn stable_hash_json(value: &serde_json::Value) -> Result<String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| Error::config("store_json_hash", error.to_string()))?;
    Ok(stable_hash_hex(bytes.as_slice()))
}

fn stable_hash_hex<T: Hash + ?Sized>(value: &T) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn stable_hash_id<T: Hash>(prefix: &str, value: &T) -> String {
    format!("{prefix}_{}", stable_hash_hex(value))
}

fn validate_snapshot_import_contract(snapshot: &StoreSnapshot) -> Result<()> {
    if snapshot.schema_id != STORE_SCHEMA_ID {
        return Err(Error::config(
            "store_snapshot_import",
            format!("unsupported schema {}", snapshot.schema_id),
        ));
    }
    if snapshot.schema_manifest.schema_id != STORE_SCHEMA_ID {
        return Err(Error::config(
            "store_snapshot_import",
            format!(
                "unsupported manifest schema {}",
                snapshot.schema_manifest.schema_id
            ),
        ));
    }
    if snapshot.schema_manifest.schema_version != STORE_SCHEMA_VERSION {
        return Err(Error::config(
            "store_snapshot_import",
            format!(
                "unsupported manifest schema version {}",
                snapshot.schema_manifest.schema_version
            ),
        ));
    }

    let json_namespaces = JSON_SNAPSHOT_NAMESPACES
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut json_keys = HashSet::new();
    let mut snapshot_json = BTreeMap::new();
    let mut evidence_documents = BTreeMap::new();
    let mut evidence_source_claims = BTreeMap::new();
    let mut evidence_claim_manifests = BTreeMap::new();
    for doc in &snapshot.json_docs {
        if !json_namespaces.contains(doc.namespace.as_str()) {
            return Err(Error::config(
                "store_snapshot_import",
                format!("unknown json namespace {}", doc.namespace),
            ));
        }
        if !json_keys.insert((doc.namespace.clone(), doc.key.clone())) {
            return Err(Error::config(
                "store_snapshot_import",
                format!("duplicate json doc {}:{}", doc.namespace, doc.key),
            ));
        }
        snapshot_json.insert((doc.namespace.clone(), doc.key.clone()), doc.value.clone());
        match doc.namespace.as_str() {
            CONVERSATION_RECALL_MANIFEST_NAMESPACE => {
                decode_typed_recall_index::<ConversationRecallManifest>(
                    &doc.key,
                    doc.value.clone(),
                )?;
            }
            ARCHIVE_RECALL_MANIFEST_NAMESPACE => {
                decode_typed_recall_index::<ArchiveRecallManifest>(&doc.key, doc.value.clone())?;
            }
            RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE => {
                decode_typed_recall_index::<RuntimeSkillRecallManifest>(
                    &doc.key,
                    doc.value.clone(),
                )?;
            }
            CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE => {
                decode_typed_recall_index::<ContinuityCapsuleScopeIndex>(
                    &doc.key,
                    doc.value.clone(),
                )?;
            }
            ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE => {
                decode_typed_recall_index::<ActiveTaskRunByChatIndex>(&doc.key, doc.value.clone())?;
            }
            TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE => {
                decode_typed_recall_index::<TaskLearningByChatIndex>(&doc.key, doc.value.clone())?;
            }
            RECALL_OWNER_SCOPE_BINDING_NAMESPACE => {
                let binding = serde_json::from_value::<RecallOwnerScopeBinding>(doc.value.clone())
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
                binding.validate()?;
                if binding.physical_key != doc.key {
                    return Err(Error::config(
                        "store_snapshot_import",
                        "recall owner scope binding snapshot key mismatch",
                    ));
                }
            }
            CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE => {
                let manifest =
                    serde_json::from_value::<ControlPlaneScopeManifest>(doc.value.clone())
                        .map_err(|error| {
                            Error::config("store_snapshot_import", error.to_string())
                        })?;
                manifest.validate(snapshot.json_docs.len().max(1))?;
                if manifest.physical_key != doc.key {
                    return Err(Error::config(
                        "store_snapshot_import",
                        "control-plane scope manifest snapshot key mismatch",
                    ));
                }
            }
            _ => {}
        }
        if doc.namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE {
            let document = serde_json::from_value::<GovernedEvidenceDocument>(doc.value.clone())
                .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
            validate_governed_evidence_document(&document)
                .map_err(|error| Error::config("store_snapshot_import", format!("{error:?}")))?;
            if document.physical_key != doc.key {
                return Err(Error::config(
                    "store_snapshot_import",
                    "evidence document snapshot key mismatch",
                ));
            }
            evidence_documents.insert(document.physical_key.clone(), document);
        } else if doc.namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE {
            let source_claim =
                serde_json::from_value::<GovernedEvidenceSourceRef>(doc.value.clone())
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
            if source_claim.physical_key != doc.key {
                return Err(Error::config(
                    "store_snapshot_import",
                    "evidence source claim snapshot key mismatch",
                ));
            }
            evidence_source_claims.insert(source_claim.physical_key.clone(), source_claim);
        } else if doc.namespace == GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE {
            let manifest =
                serde_json::from_value::<GovernedEvidenceSourceClaimManifest>(doc.value.clone())
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
            if manifest.physical_key != doc.key {
                return Err(Error::config(
                    "store_snapshot_import",
                    "evidence source claim manifest snapshot key mismatch",
                ));
            }
            evidence_claim_manifests.insert(
                (
                    manifest.memory_space_id.clone(),
                    manifest.mounted_subject_id.clone(),
                ),
                manifest,
            );
        }
    }
    crate::store_internal::transaction::validate_control_plane_manifest_set(
        &snapshot_json,
        snapshot.json_docs.len().max(1),
    )?;
    for document in evidence_documents.values() {
        let expected_claim = governed_evidence_source_ref_from_document(document)
            .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
        let Some(source_claim) = evidence_source_claims.get(&expected_claim.physical_key) else {
            return Err(Error::config(
                "store_snapshot_import",
                "evidence document snapshot is missing source claim",
            ));
        };
        validate_governed_evidence_source_ref(document, source_claim)
            .map_err(|error| Error::config("store_snapshot_import", format!("{error:?}")))?;
    }
    for source_claim in evidence_source_claims.values() {
        let owner_key = scoped_governed_evidence_document_key(
            &source_claim.memory_space_id,
            &source_claim.owner_ref.owner_id,
        )
        .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
        let Some(document) = evidence_documents.get(&owner_key) else {
            return Err(Error::config(
                "store_snapshot_import",
                "evidence source claim snapshot owner is missing",
            ));
        };
        validate_governed_evidence_source_ref(document, source_claim)
            .map_err(|error| Error::config("store_snapshot_import", format!("{error:?}")))?;
    }
    let mut evidence_scopes = BTreeSet::new();
    evidence_scopes.extend(evidence_documents.values().map(|document| {
        (
            document.memory_space_id.clone(),
            document.mounted_subject_id.clone(),
        )
    }));
    evidence_scopes.extend(evidence_source_claims.values().map(|claim| {
        (
            claim.memory_space_id.clone(),
            claim.mounted_subject_id.clone(),
        )
    }));
    evidence_scopes.extend(evidence_claim_manifests.keys().cloned());
    for (memory_space_id, mounted_subject_id) in evidence_scopes {
        let manifest = evidence_claim_manifests
            .get(&(memory_space_id.clone(), mounted_subject_id.clone()))
            .ok_or_else(|| {
                Error::config(
                    "store_snapshot_import",
                    "evidence source claim scope manifest is missing",
                )
            })?;
        let bindings = evidence_documents
            .values()
            .filter(|document| {
                document.memory_space_id == memory_space_id
                    && document.mounted_subject_id == mounted_subject_id
            })
            .map(|document| {
                let expected = governed_evidence_source_ref_from_document(document)?;
                let claim = evidence_source_claims.get(&expected.physical_key).ok_or_else(|| {
                    Error::config(
                        "store_snapshot_import",
                        "manifest evidence owner is missing its exact claim",
                    )
                })?;
                crate::store_internal::schema::GovernedEvidenceOwnerClaimBinding::from_document_claim(
                    document,
                    claim,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        manifest.validate_exact(
            &memory_space_id,
            &mounted_subject_id,
            bindings,
            snapshot.json_docs.len().max(1),
        )?;
    }

    let blob_namespaces = BLOB_SNAPSHOT_NAMESPACES
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut blob_keys = HashSet::new();
    let mut snapshot_blobs = BTreeMap::new();
    for blob in &snapshot.blobs {
        if !blob_namespaces.contains(blob.namespace.as_str()) {
            return Err(Error::config(
                "store_snapshot_import",
                format!("unknown blob namespace {}", blob.namespace),
            ));
        }
        if !blob_keys.insert((blob.namespace.clone(), blob.key.clone())) {
            return Err(Error::config(
                "store_snapshot_import",
                format!("duplicate blob {}:{}", blob.namespace, blob.key),
            ));
        }
        snapshot_blobs.insert(
            (blob.namespace.clone(), blob.key.clone()),
            blob.value.clone(),
        );
    }
    crate::store_internal::transaction::validate_snapshot_recall_manifest_documents(
        &snapshot_json,
        &snapshot_blobs,
    )?;

    let mut event_ids = HashSet::new();
    for event in &snapshot.events {
        if !event_ids.insert(event.event_id.clone()) {
            return Err(Error::config(
                "store_snapshot_import",
                format!("duplicate event id {}", event.event_id),
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_scoped_projection_governed_closure(
    snapshot: &StoreSnapshot,
    memory_space_id: &str,
    mounted_subject_id: &str,
) -> Result<()> {
    let after = BackendTransactionState {
        json: snapshot
            .json_docs
            .iter()
            .map(|doc| ((doc.namespace.clone(), doc.key.clone()), doc.value.clone()))
            .collect(),
        blobs: BTreeMap::new(),
        events: snapshot.events.clone(),
    };
    let before = after.clone();
    let projection_scope = StoreScopedProjectionScope::new(memory_space_id, mounted_subject_id)?;
    crate::store_internal::transaction::validate_scoped_recall_manifest_documents(
        &after.json,
        &after.blobs,
        &projection_scope,
    )?;
    crate::store_internal::transaction::validate_scoped_control_plane_documents(
        &after.json,
        &projection_scope,
        snapshot.json_docs.len().max(1),
    )?;
    let scope = StoreEventScope::system("memory_space_import_validation")
        .with_memory_space(memory_space_id)
        .with_subject(mounted_subject_id);
    let build_batch = |operation: &str, namespaces: &[&str]| StoreMutationBatch {
        transaction_id: format!("{operation}:{memory_space_id}:{mounted_subject_id}"),
        operation: operation.to_string(),
        scope: scope.clone(),
        mutations: snapshot
            .json_docs
            .iter()
            .filter(|doc| namespaces.contains(&doc.namespace.as_str()))
            .map(|doc| StoreMutation::PutJson {
                namespace: doc.namespace.clone(),
                key: doc.key.clone(),
                value: doc.value.clone(),
                event_kind: MemoryStoreEventKind::MemoryMaintenance,
                plane: doc.namespace.clone(),
                record_key: doc.key.clone(),
            })
            .collect(),
    };

    const FACET_CLOSURE_NAMESPACES: &[&str] =
        &[MEMORY_FACET_INDEX_NAMESPACE, MEMORY_FACET_POSTING_NAMESPACE];
    let governed_owner_present = snapshot.json_docs.iter().any(|doc| {
        doc.namespace == "long_term" || doc.namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
    });
    let facet_batch = build_batch(
        "memory_space_import_facet_validation",
        FACET_CLOSURE_NAMESPACES,
    );
    if governed_owner_present && facet_batch.mutations.is_empty() {
        return Err(Error::config(
            "memory_space_import",
            "governed owner projection is missing its facet closure",
        ));
    }
    if !facet_batch.mutations.is_empty() {
        validate_facet_post_image(&facet_batch, &before, &after)?;
    }

    const GRAPH_CLOSURE_NAMESPACES: &[&str] = &[
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        MEMORY_GRAPH_REVISION_NAMESPACE,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_INDEX_NAMESPACE,
        MEMORY_GRAPH_NODE_NAMESPACE,
        MEMORY_GRAPH_EDGE_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_NAMESPACE,
    ];
    let graph_batch = build_batch(
        "memory_space_import_graph_validation",
        GRAPH_CLOSURE_NAMESPACES,
    );
    if !graph_batch.mutations.is_empty() {
        validate_graph_post_image(&graph_batch, &before, &after, false)?;
    }
    Ok(())
}

#[cfg(feature = "nonproduction-replay-harness")]
fn enforce_snapshot_logical_budget(
    capacity: StoreCapacityBudget,
    snapshot: &StoreSnapshot,
) -> Result<()> {
    for doc in &snapshot.json_docs {
        enforce_logical_key_budget(capacity, &doc.namespace, &doc.key, "store_snapshot_import")?;
    }
    for blob in &snapshot.blobs {
        enforce_logical_key_budget(
            capacity,
            &blob.namespace,
            &blob.key,
            "store_snapshot_import",
        )?;
    }
    for event in &snapshot.events {
        enforce_event_key_budget(capacity, event, "store_snapshot_import")?;
    }
    Ok(())
}

fn tail<T>(mut values: Vec<T>, limit: usize) -> Vec<T> {
    if values.len() <= limit {
        return values;
    }
    let remove_count = values.len() - limit;
    values.drain(0..remove_count);
    values
}

fn private_garden_key_prefix(chat_id: &str) -> String {
    format!("{chat_id}::")
}

fn private_garden_key(chat_id: &str, doc_path: &str) -> String {
    format!("{}{doc_path}", private_garden_key_prefix(chat_id))
}

fn private_garden_record(doc: &PrivateGardenDoc) -> PrivateGardenDocRecord {
    PrivateGardenDocRecord {
        path: doc.path.clone(),
        updated_at: doc.updated_at,
        revision: doc.revision,
        bytes: doc.content.len(),
        preview: doc.content.chars().take(160).collect(),
    }
}

fn triple_key(a: &str, b: &str, c: &str) -> String {
    format!("{a}::{b}::{c}")
}

fn _dedupe_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

#[cfg(test)]
mod transaction_error_contract_tests {
    use super::*;
    use bm_core::feature_gate::ProfileId;
    #[cfg(feature = "nonproduction-replay-harness")]
    use bm_core::orchestrator::PressureLevel;
    #[cfg(feature = "nonproduction-replay-harness")]
    use bm_core::resource::{RuntimeResourceObservation, RuntimeResourceProbe};
    #[cfg(feature = "nonproduction-replay-harness")]
    use std::sync::atomic::{AtomicU64, Ordering};

    fn native_production_profile() -> ProfileId {
        #[cfg(target_os = "macos")]
        return ProfileId::DesktopMacosEmbeddedSdk;
        #[cfg(target_os = "windows")]
        return ProfileId::DesktopWindowsEmbeddedSdk;
        #[cfg(target_os = "linux")]
        return ProfileId::ServerLinuxMemoryGateway;
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        compile_error!("store preparation tests require a supported host target");
    }

    #[test]
    fn backend_conflict_busy_and_repair_stages_survive_the_production_coordinator() {
        for stage in [
            "memory_write_transaction_precondition_failed",
            "store_transaction_busy",
            "memory_write_transaction_repair_required",
        ] {
            let mapped = memory_write_transaction_commit_error(Error::config(stage, "proof"));
            assert_eq!(mapped.stage(), stage);
        }
    }

    #[test]
    fn skill_transaction_timestamp_requires_one_canonical_runtime_authority() {
        let scope = StoreEventScope::new("agent", "owner", "console", "chat")
            .with_memory_space("space")
            .with_subject("subject");
        let skill_batch = StoreMutationBatch {
            transaction_id: "skill-edit".to_string(),
            operation: "runtime_skill.edit".to_string(),
            scope: scope.clone(),
            mutations: vec![StoreMutation::PutBlob {
                namespace: "skills".to_string(),
                key: "runtime_skill__release".to_string(),
                value: b"typed skill".to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skills".to_string(),
                record_key: "runtime_skill__release".to_string(),
            }],
        };
        let missing = canonical_transaction_timestamp(&skill_batch, None)
            .expect_err("skill timestamp must not fall back to wall clock");
        assert_eq!(missing.stage(), "runtime_skill_recall_manifest");
        assert_eq!(
            canonical_transaction_timestamp(&skill_batch, Some(1_800_000_001))
                .expect("explicit runtime clock"),
            1_800_000_001
        );

        let mut mismatched = skill_batch;
        mismatched.mutations.push(StoreMutation::AppendEvent {
            event: MemoryStoreEvent::new(
                "event-skill-edit",
                MemoryStoreEventKind::MemoryWrite,
                scope,
                1_800_000_000,
            ),
        });
        let mismatch = canonical_transaction_timestamp(&mismatched, Some(1_800_000_001))
            .expect_err("two timestamp authorities must not diverge");
        assert_eq!(mismatch.stage(), "memory_write_transaction_timestamp");
    }

    #[test]
    fn prepared_store_consumes_the_exact_fresh_report_identity() {
        let profile = native_production_profile();
        let config = StoreBackendConfig::in_memory(profile).expect("in-memory config");
        let now_secs = current_unix_secs();
        let preparation = StorePlatformPreparation::prepare_at(config, None, now_secs)
            .expect("prepare store admission");
        let expected_report_id = preparation.report_id.clone();
        let capacity = resolve_store_capacity(&preparation.runtime_budget_authority)
            .expect("resolve capacity");

        let (platform, _) = StorePlatform::open_prepared_at(preparation, capacity, now_secs)
            .expect("consume exact prepared admission");

        assert_eq!(
            platform.current_runtime_budget(now_secs).report_id,
            expected_report_id
        );
    }

    #[test]
    fn prepared_store_admission_cannot_be_consumed_twice() {
        let profile = native_production_profile();
        let config = StoreBackendConfig::in_memory(profile).expect("in-memory config");
        let now_secs = current_unix_secs();
        let preparation = StorePlatformPreparation::prepare_at(config, None, now_secs)
            .expect("prepare store admission");
        let replay = preparation.duplicate_for_consumption_contract_test();
        let capacity = resolve_store_capacity(&preparation.runtime_budget_authority)
            .expect("resolve capacity");

        StorePlatform::open_prepared_at(preparation, capacity, now_secs)
            .expect("consume issued admission");
        let error = match StorePlatform::open_prepared_at(replay, capacity, now_secs) {
            Ok(_) => panic!("consumed admission must not open a second engine"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "store_prepared_admission_consumed");
    }

    #[test]
    fn prepared_store_rejects_a_report_that_expired_before_open() {
        let profile = native_production_profile();
        let config = StoreBackendConfig::in_memory(profile).expect("in-memory config");
        let preparation = StorePlatformPreparation::prepare_at(config, None, 10)
            .expect("prepare store admission");
        let capacity = resolve_store_capacity(&preparation.runtime_budget_authority)
            .expect("resolve capacity");

        let error = match StorePlatform::open_prepared_at(preparation, capacity, 40) {
            Ok(_) => panic!("expired prepared identity must not be refreshed in place"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "store_prepared_admission_invalid");
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    struct ControlledResourceProbe {
        ttl_ms: u64,
        memory_available_bytes: AtomicU64,
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    impl ControlledResourceProbe {
        fn new(ttl_ms: u64) -> Self {
            Self {
                ttl_ms,
                memory_available_bytes: AtomicU64::new(512 * 1024 * 1024),
            }
        }

        fn contract_memory_budget(&self) {
            self.memory_available_bytes
                .store(128 * 1024 * 1024, Ordering::Release);
        }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    impl RuntimeResourceProbe for ControlledResourceProbe {
        fn probe(&self, now_secs: u64) -> Result<RuntimeResourceObservation> {
            Ok(RuntimeResourceObservation {
                observed_at_unix_secs: now_secs,
                ttl_ms: self.ttl_ms,
                stale: false,
                pressure: PressureLevel::Normal,
                available_parallelism: Some(4),
                memory_total_bytes: Some(1024 * 1024 * 1024),
                memory_available_bytes: Some(self.memory_available_bytes.load(Ordering::Acquire)),
                internal_heap_free_bytes: Some(512 * 1024),
                internal_heap_minimum_free_bytes: Some(256 * 1024),
                internal_heap_largest_block_bytes: Some(128 * 1024),
                psram_total_bytes: Some(8 * 1024 * 1024),
                psram_free_bytes: Some(4 * 1024 * 1024),
                psram_largest_block_bytes: Some(2 * 1024 * 1024),
                storage_total_bytes: Some(1024 * 1024 * 1024),
                storage_available_bytes: Some(512 * 1024 * 1024),
                unavailable_reason: None,
                unavailable_detail: None,
            })
        }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn admission_test_platforms(
        root: &std::path::Path,
        ttl_ms: u64,
    ) -> Vec<(String, StorePlatform, Arc<ControlledResourceProbe>)> {
        let host = native_production_profile();
        let configs = vec![
            (
                "in_memory".to_string(),
                StoreBackendConfig::in_memory(host).expect("in-memory config"),
            ),
            (
                "embedded".to_string(),
                StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory)
                    .expect("embedded config"),
            ),
            (
                "file".to_string(),
                StoreBackendConfig::file(root.join("file"), host).expect("file config"),
            ),
        ];
        #[cfg(feature = "sqlite-store")]
        let configs = configs
            .into_iter()
            .chain(std::iter::once((
                "sqlite".to_string(),
                StoreBackendConfig::sqlite(root.join("sqlite.db"), host).expect("sqlite config"),
            )))
            .collect::<Vec<_>>();
        configs
            .into_iter()
            .map(|(name, config)| {
                let probe = Arc::new(ControlledResourceProbe::new(ttl_ms));
                let platform = StorePlatform::open_with_nonproduction_probe(
                    config,
                    Arc::clone(&probe) as Arc<dyn RuntimeResourceProbe>,
                )
                .unwrap_or_else(|error| panic!("open {name} admission fixture: {error}"));
                (name, platform, probe)
            })
            .collect()
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn admission_write_request(case_name: &str) -> StoreTransactionRequest {
        StoreTransactionRequest::new(
            format!("admission-write-{case_name}"),
            Vec::new(),
            vec![StoreEngineMutation::PutJson {
                namespace: "admission_contract".to_string(),
                key: case_name.to_string(),
                value: serde_json::json!({"case": case_name}),
            }],
            None,
        )
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn empty_scoped_replace() -> StoreScopedProjectionReplaceRequest {
        StoreScopedProjectionReplaceRequest {
            scope: StoreScopedProjectionScope::new("space:admission", "subject:admission")
                .expect("scope"),
            json_namespaces: Vec::new(),
            json_docs: Vec::new(),
            events: Vec::new(),
        }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn exact_evidence_read_requires_the_active_pinned_runtime_budget_report() {
        let config = StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("in-memory config");
        let platform = StorePlatform::open(config).expect("open exact evidence store");
        let other_platform = StorePlatform::open(
            StoreBackendConfig::embedded(ProfileId::EspEmbeddedSdk)
                .expect("other authority config"),
        )
        .expect("open other exact evidence authority");
        let other_report = other_platform.current_runtime_budget(current_unix_secs());
        let request = StoreGovernedEvidenceExactReadRequest {
            memory_space_id: "space:exact-admission".to_string(),
            mounted_subject_id: "subject:exact-admission".to_string(),
            owner_keys: vec!["owner:absent".to_string()],
            include_all_manifest_bindings: false,
            allow_missing_manifest_for_empty_scope: true,
        };
        let lease =
            crate::RuntimeBudgetLease::issue(Arc::clone(&platform.runtime_budget_authority))
                .expect("issue exact evidence lease");
        let pinned = lease.report().clone();

        let error = platform
            .read_governed_evidence_exact(&pinned, request.clone())
            .expect_err("exact evidence read without an active lease must fail");
        assert_eq!(error.stage(), "governed_evidence_exact_read_admission");

        lease
            .execute(&platform.runtime_budget_authority, || {
                let result = platform.read_governed_evidence_exact(&pinned, request.clone())?;
                assert_eq!(result.entry_count, 1);
                assert!(result.reads[0].owner.is_none());

                let error = platform
                    .read_governed_evidence_exact(&other_report, request)
                    .expect_err("another authority report id must fail under the active lease");
                assert_eq!(error.stage(), "governed_evidence_exact_read_admission");
                Ok(())
            })
            .expect("execute exact evidence lease contract");
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn all_backend_write_and_typed_replace_fences_reject_admission_after_ttl() {
        let root = std::env::temp_dir().join(format!(
            "beetle-memory-admission-ttl-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let cases = admission_test_platforms(&root, 1_000);
        let admissions = cases
            .iter()
            .map(|(name, platform, _)| {
                (
                    name.clone(),
                    platform
                        .current_store_transaction_admission()
                        .expect("fresh admission"),
                )
            })
            .collect::<BTreeMap<_, _>>();
        std::thread::sleep(std::time::Duration::from_millis(1_100));

        for (name, platform, _) in &cases {
            let admission = admissions.get(name).expect("admission");
            let write_error = platform
                .engine
                .commit_transaction_admitted(&admission_write_request(name), admission)
                .expect_err("expired admission must fail inside the backend write fence");
            assert_eq!(
                write_error.stage(),
                "memory_write_transaction_resource_admission",
                "backend={name}: {write_error}"
            );
            let replace_error = platform
                .engine
                .replace_scoped_projection(&empty_scoped_replace(), admission)
                .expect_err("expired admission must fail inside the typed replace fence");
            assert_eq!(
                replace_error.stage(),
                "memory_write_transaction_resource_admission",
                "backend={name}: {replace_error}"
            );
            assert!(platform
                .engine
                .get_json_value("admission_contract", name)
                .expect("read admission fixture")
                .is_none());
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn all_backend_fences_reject_a_fresh_but_superseded_authority_report() {
        let root = std::env::temp_dir().join(format!(
            "beetle-memory-admission-authority-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let cases = admission_test_platforms(&root, 30_000);

        for (name, platform, probe) in &cases {
            let admission = platform
                .current_store_transaction_admission()
                .expect("fresh admission");
            probe.contract_memory_budget();
            platform
                .refresh_runtime_resource_snapshot(current_unix_secs())
                .expect("refresh contracted authority report");
            let error = platform
                .engine
                .commit_transaction_admitted(&admission_write_request(name), &admission)
                .expect_err("superseded report must fail inside the backend write fence");
            assert_eq!(
                error.stage(),
                "memory_write_transaction_resource_admission",
                "backend={name}: {error}"
            );
            assert!(error
                .to_string()
                .contains("current exact runtime authority"));
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trust_boundary_requires_old_root_removal_and_unique_new_root() {
        use bm_core::task_execution::{
            TaskLearningCandidateState, TaskLearningKind, TaskLearningRoute, TaskRunStatus,
        };

        let before = TaskLearningRecord {
            learning_id: "learning".into(),
            source_channel: "old-channel".into(),
            source_chat_id: "old-chat".into(),
            run_id: "run".into(),
            step_id: String::new(),
            kind: TaskLearningKind::DurableFact,
            route: TaskLearningRoute::Pending,
            run_status: TaskRunStatus::Completed,
            topic: "topic".into(),
            summary: "summary".into(),
            content: "content".into(),
            memory_kind: None,
            review_summary: String::new(),
            source_artifact_ids: Vec::new(),
            provenance: String::new(),
            archive_note_name: String::new(),
            route_detail: String::new(),
            candidate_state: Some(TaskLearningCandidateState::Observed),
            candidate_state_updated_at: 1,
            last_failure_reason: String::new(),
            observed_at: 1,
        };
        let mut next = before.clone();
        next.source_channel = "new-channel".into();
        next.source_chat_id = "new-chat".into();
        next.observed_at = 2;
        let next_value = serde_json::to_value(&next).unwrap();
        let owner_digest = RecallIndexAddress::json("task_learning", "learning", 1, 0, &next_value)
            .unwrap()
            .content_sha256;
        let binding = RecallOwnerScopeBinding::build(
            "space",
            "subject",
            "json",
            "task_learning",
            "learning",
            &owner_digest,
        )
        .unwrap();
        let binding_mutation = StoreMutation::PutJson {
            namespace: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.into(),
            key: binding.physical_key.clone(),
            value: serde_json::to_value(binding).unwrap(),
            event_kind: MemoryStoreEventKind::MemoryMaintenance,
            plane: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.into(),
            record_key: "binding".into(),
        };
        let old_root =
            TaskLearningByChatIndex::build(2, "space", "old-channel", "old-chat", []).unwrap();
        let new_root = TaskLearningByChatIndex::build(
            1,
            "space",
            "new-channel",
            "new-chat",
            [RecallIndexAddress::json("task_learning", "learning", 2, 2, &next_value).unwrap()],
        )
        .unwrap();
        let root_mutation = |root: TaskLearningByChatIndex| StoreMutation::PutJson {
            namespace: TaskLearningByChatIndex::NAMESPACE.into(),
            key: root.physical_key.clone(),
            value: serde_json::to_value(root).unwrap(),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: TaskLearningByChatIndex::NAMESPACE.into(),
            record_key: "root".into(),
        };
        let owner_mutation = StoreMutation::PutJson {
            namespace: "task_learning".into(),
            key: "learning".into(),
            value: next_value,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "task_learning".into(),
            record_key: "learning".into(),
        };
        let batch = StoreMutationBatch {
            transaction_id: "scope-transfer".into(),
            operation: "test".into(),
            scope: StoreEventScope::new("agent", "subject", "new-channel", "new-chat")
                .with_memory_space("space"),
            mutations: vec![
                owner_mutation.clone(),
                binding_mutation,
                root_mutation(old_root.clone()),
                root_mutation(new_root.clone()),
            ],
        };
        let read_before = |namespace: &str, key: &str| {
            Ok((namespace == "task_learning" && key == "learning")
                .then(|| serde_json::to_value(&before).unwrap()))
        };
        validate_recall_index_mutation_closure(&batch, read_before, |_, _| Ok(None)).unwrap();

        let mut missing_old = batch.clone();
        missing_old.mutations.retain(|mutation| !matches!(mutation,
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE && key == &old_root.physical_key));
        assert!(
            validate_recall_index_mutation_closure(&missing_old, read_before, |_, _| Ok(None))
                .is_err()
        );

        let wrong_root = TaskLearningByChatIndex::build(
            1,
            "space",
            "wrong-channel",
            "wrong-chat",
            [RecallIndexAddress::json(
                "task_learning",
                "learning",
                2,
                2,
                match &owner_mutation {
                    StoreMutation::PutJson { value, .. } => value,
                    _ => unreachable!(),
                },
            )
            .unwrap()],
        )
        .unwrap();
        let mut wrong = batch;
        wrong.mutations.push(root_mutation(wrong_root));
        assert!(
            validate_recall_index_mutation_closure(&wrong, read_before, |_, _| Ok(None)).is_err()
        );
    }
}
