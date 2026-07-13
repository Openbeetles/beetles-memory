use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::agent::{ActiveWorkRecord, ActiveWorkStore};
use bm_core::memory::*;
use bm_core::platform::{MemorySystemKind, Platform, SkillMetaStore, SkillStorage, StateFs};
use bm_core::resource::{HostRuntimeResourceProbe, RuntimeResourceProbe};
use bm_core::runtime::{
    RuntimeLifecycleEvent, RuntimeLifecycleEventKind, RuntimeLifecycleEventSink,
};
use bm_core::task::{normalize_task_item, TaskItem, TaskQuery, TaskStore};
use bm_core::task_execution::{
    TaskArtifactRecord, TaskArtifactStore, TaskLearningRecord, TaskLearningStore, TaskRunRecord,
    TaskRunStore,
};
use bm_core::{Error, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::store_internal::read_snapshot::ReadOnlySnapshotStoreEngine;
#[cfg(feature = "sqlite-store")]
use crate::store_internal::sqlite::SqliteStoreEngine;
use crate::store_internal::transaction::{BackendTransactionState, GraphRepairAuthority};
use crate::{
    enforce_event_key_budget, enforce_logical_key_budget, store_budget_error,
    store_internal::embedded::EmbeddedStoreEngine, store_internal::file::FileStoreEngine,
    InMemoryStoreEngine, MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig,
    StoreBackendKind, StoreCapacityBudget, StoreConsistentNamespaceReadRequest, StoreEngine,
    StoreEngineMutation, StoreEventLog, StoreEventScope, StoreJsonPrecondition, StoreMutation,
    StoreMutationBatch, StoreMutationBatchReport, StoreOpenReport, StoreReadReceipt,
    StoreRepairReport, StoreSchemaManifest, StoreSnapshot, StoreSnapshotBlob,
    StoreSnapshotExportReport, StoreSnapshotImportReport, StoreSnapshotJsonDoc,
    StoreTransactionRequest, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION,
};

static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct StorePlatform {
    config: StoreBackendConfig,
    engine: Arc<dyn StoreEngine>,
    transaction_mutex: Arc<Mutex<()>>,
    schema_manifest: StoreSchemaManifest,
    open_report: StoreOpenReport,
}

pub struct GovernedRecallSnapshot {
    platform: StorePlatform,
    receipt: StoreReadReceipt,
}

impl GovernedRecallSnapshot {
    pub fn platform(&self) -> &StorePlatform {
        &self.platform
    }

    pub(crate) fn verified(&self) -> bool {
        self.receipt.state_digest.len() == 64
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn receipt(&self) -> &StoreReadReceipt {
        &self.receipt
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
        Self::open_with_report(config).map(|(platform, _report)| platform)
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

    pub fn open_with_report(config: StoreBackendConfig) -> Result<(Self, StoreOpenReport)> {
        let now_secs = current_unix_secs();
        let (engine, repair, schema_manifest): (
            Arc<dyn StoreEngine>,
            StoreRepairReport,
            StoreSchemaManifest,
        ) = match config.backend {
            StoreBackendKind::InMemory => (
                Arc::new(InMemoryStoreEngine::new(config.capacity)),
                StoreRepairReport::clean(),
                StoreSchemaManifest::new(config.backend, config.profile, now_secs),
            ),
            StoreBackendKind::Embedded => (
                Arc::new(EmbeddedStoreEngine::new(config.capacity)),
                StoreRepairReport::clean(),
                StoreSchemaManifest::new(config.backend, config.profile, now_secs),
            ),
            StoreBackendKind::File => {
                let (engine, repair, manifest) = FileStoreEngine::open(&config)?;
                (Arc::new(engine), repair, manifest)
            }
            StoreBackendKind::Sqlite => {
                let (engine, manifest) = sqlite_engine(&config)?;
                (engine, StoreRepairReport::clean(), manifest)
            }
        };
        let report = StoreOpenReport {
            backend: config.backend.as_str().to_string(),
            schema_id: STORE_SCHEMA_ID.to_string(),
            repair,
        };
        let platform = Self {
            config,
            engine,
            transaction_mutex: Arc::new(Mutex::new(())),
            schema_manifest,
            open_report: report.clone(),
        };
        platform.emit_runtime_event("open")?;
        Ok((platform, report))
    }

    pub fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        self.engine.read_events()
    }

    pub fn load_governed_recall_snapshot(&self) -> Result<GovernedRecallSnapshot> {
        let result =
            self.engine
                .read_consistent_namespaces(&StoreConsistentNamespaceReadRequest {
                    json_namespaces: JSON_SNAPSHOT_NAMESPACES
                        .iter()
                        .map(|namespace| (*namespace).to_string())
                        .collect(),
                    blob_namespaces: BLOB_SNAPSHOT_NAMESPACES
                        .iter()
                        .map(|namespace| (*namespace).to_string())
                        .collect(),
                    include_events: false,
                })?;
        let receipt = result.receipt.clone();
        let engine = Arc::new(ReadOnlySnapshotStoreEngine::from_consistent_read(&result));
        Ok(GovernedRecallSnapshot {
            platform: StorePlatform {
                config: self.config.clone(),
                engine,
                transaction_mutex: Arc::new(Mutex::new(())),
                schema_manifest: self.schema_manifest.clone(),
                open_report: self.open_report.clone(),
            },
            receipt,
        })
    }

    fn lock_transaction(&self, stage: &'static str) -> Result<std::sync::MutexGuard<'_, ()>> {
        self.transaction_mutex
            .lock()
            .map_err(|_| Error::config(stage, "transaction mutex poisoned"))
    }

    pub fn read_file_store_events(root: impl AsRef<Path>) -> Result<Vec<MemoryStoreEvent>> {
        crate::store_internal::file::read_events_from_root(root.as_ref())
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

    pub fn commit_governed_memory_transaction_with_preconditions(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(batch, preconditions, None)
    }

    pub(crate) fn commit_governed_graph_repair_transaction(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        authority: GraphRepairAuthority,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(batch, preconditions, Some(authority))
    }

    fn commit_governed_memory_transaction_authorized(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        graph_repair_authority: Option<GraphRepairAuthority>,
    ) -> Result<StoreMutationBatchReport> {
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

        let _transaction_guard = self.lock_transaction("memory_write_transaction_commit_failed")?;

        validate_batch_mutation_namespaces(&batch)?;
        validate_protected_json_mutation_preconditions(&batch, preconditions)?;
        validate_long_term_owner_facet_closure(&batch, preconditions)?;
        validate_graph_manifest_closure(&batch)?;
        validate_control_audit_closure(&batch, preconditions)?;

        let mut json_docs = self
            .snapshot_json_map()
            .map_err(memory_write_transaction_preflight_error)?;
        self.validate_json_preconditions(preconditions, &json_docs)?;
        let mut blob_docs = self
            .snapshot_blob_map()
            .map_err(memory_write_transaction_preflight_error)?;
        let mut events = self
            .engine
            .read_events()
            .map_err(memory_write_transaction_preflight_error)?;

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
                        self.config.capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    let event = self.build_batch_event(
                        &batch,
                        event_kind.clone(),
                        plane,
                        record_key,
                        stable_hash_json(value)
                            .map_err(memory_write_transaction_preflight_error)?,
                    );
                    enforce_event_key_budget(
                        self.config.capacity,
                        &event,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    json_docs.insert((namespace.clone(), key.clone()), value.clone());
                    events.push(event.clone());
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
                        self.config.capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    if json_docs
                        .remove(&(namespace.clone(), key.clone()))
                        .is_some()
                    {
                        let event = self.build_batch_event(
                            &batch,
                            event_kind.clone(),
                            plane,
                            record_key,
                            stable_hash_hex(&("delete", namespace, key)),
                        );
                        enforce_event_key_budget(
                            self.config.capacity,
                            &event,
                            "memory_write_transaction",
                        )
                        .map_err(memory_write_transaction_preflight_error)?;
                        events.push(event.clone());
                        engine_mutations.push(StoreEngineMutation::DeleteJson {
                            namespace: namespace.clone(),
                            key: key.clone(),
                        });
                        engine_mutations.push(StoreEngineMutation::AppendEvent {
                            event: Box::new(event),
                        });
                    } else {
                        engine_mutations.push(StoreEngineMutation::DeleteJson {
                            namespace: namespace.clone(),
                            key: key.clone(),
                        });
                    }
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
                        self.config.capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    let event = self.build_batch_event(
                        &batch,
                        event_kind.clone(),
                        plane,
                        record_key,
                        stable_hash_hex(value),
                    );
                    enforce_event_key_budget(
                        self.config.capacity,
                        &event,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    blob_docs.insert((namespace.clone(), key.clone()), value.clone());
                    events.push(event.clone());
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
                        self.config.capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    if blob_docs
                        .remove(&(namespace.clone(), key.clone()))
                        .is_some()
                    {
                        let event = self.build_batch_event(
                            &batch,
                            event_kind.clone(),
                            plane,
                            record_key,
                            stable_hash_hex(&("delete", namespace, key)),
                        );
                        enforce_event_key_budget(
                            self.config.capacity,
                            &event,
                            "memory_write_transaction",
                        )
                        .map_err(memory_write_transaction_preflight_error)?;
                        events.push(event.clone());
                        engine_mutations.push(StoreEngineMutation::DeleteBlob {
                            namespace: namespace.clone(),
                            key: key.clone(),
                        });
                        engine_mutations.push(StoreEngineMutation::AppendEvent {
                            event: Box::new(event),
                        });
                    } else {
                        engine_mutations.push(StoreEngineMutation::DeleteBlob {
                            namespace: namespace.clone(),
                            key: key.clone(),
                        });
                    }
                }
                StoreMutation::AppendEvent { event } => {
                    enforce_event_key_budget(
                        self.config.capacity,
                        event,
                        "memory_write_transaction",
                    )
                    .map_err(memory_write_transaction_preflight_error)?;
                    events.push(event.clone());
                    engine_mutations.push(StoreEngineMutation::AppendEvent {
                        event: Box::new(event.clone()),
                    });
                }
            }
        }

        validate_mutation_batch_budget(self.config.capacity, &json_docs, &blob_docs, &events)
            .map_err(memory_write_transaction_preflight_error)?;
        validate_unique_event_ids(&events)?;

        let mut request = StoreTransactionRequest::new(
            batch.transaction_id.clone(),
            preconditions.to_vec(),
            engine_mutations,
            Some(Box::new(batch.clone())),
        );
        if let Some(authority) = graph_repair_authority {
            request = request.authorize_graph_repair(authority);
        }
        let engine_report = self
            .engine
            .commit_transaction(&request)
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

    pub fn into_arc(self) -> Arc<dyn Platform> {
        Arc::new(self)
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

    pub fn read_json_docs_by_keys(
        &self,
        namespace: &str,
        keys: &[String],
    ) -> Result<Vec<StoreSnapshotJsonDoc>> {
        ensure_json_snapshot_namespace(namespace, "store_json_namespace_read")?;
        let mut seen = BTreeSet::new();
        let mut docs = Vec::new();
        for key in keys {
            if !seen.insert(key.as_str()) {
                continue;
            }
            if let Some(value) = self.engine.get_json_value(namespace, key)? {
                docs.push(StoreSnapshotJsonDoc {
                    namespace: namespace.to_string(),
                    key: key.clone(),
                    value,
                });
            }
        }
        Ok(docs)
    }

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
        self.enforce_snapshot_budget(&snapshot, self.config.capacity.export_max_bytes, "export")?;
        let report = snapshot.export_report();
        Ok((snapshot, report))
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn import_store_snapshot(&self, snapshot: &StoreSnapshot) -> Result<()> {
        self.import_store_snapshot_with_report(snapshot)
            .map(|_report| ())
    }

    pub fn import_store_snapshot_with_report(
        &self,
        snapshot: &StoreSnapshot,
    ) -> Result<StoreSnapshotImportReport> {
        validate_snapshot_import_contract(snapshot)?;
        enforce_snapshot_logical_budget(self.config.capacity, snapshot)?;
        self.enforce_snapshot_budget(snapshot, self.config.capacity.import_max_bytes, "import")?;
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
        let _transaction_guard = self.lock_transaction("store_json_write")?;
        enforce_logical_key_budget(self.config.capacity, namespace, key, "store_json_write")?;
        if self.engine.get_json_value(namespace, key)?.is_none() {
            let current_entries = self.engine.list_json_keys(namespace)?.len();
            if current_entries >= self.config.capacity.kv_max_entries {
                return Err(Error::config(
                    "store_budget_exceeded",
                    format!(
                        "kv entries {} exceed {}",
                        current_entries.saturating_add(1),
                        self.config.capacity.kv_max_entries
                    ),
                ));
            }
        }
        let value = serde_json::to_value(value)
            .map_err(|error| Error::config("store_json_encode", error.to_string()))?;
        let content_hash = stable_hash_json(&value)?;
        let event = self.build_memory_event(event_kind, namespace, record_key, content_hash);
        self.engine
            .put_json_value_and_event(namespace, key, value, event)
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
        enforce_logical_key_budget(self.config.capacity, namespace, key, "store_json_delete")?;
        let content_hash = stable_hash_hex(&("delete", namespace, key));
        let event = self.build_memory_event(event_kind, namespace, record_key, content_hash);
        self.engine
            .delete_json_value_and_event(namespace, key, event)
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
        let _transaction_guard = self.lock_transaction("store_blob_write")?;
        enforce_logical_key_budget(self.config.capacity, namespace, key, "store_blob_write")?;
        if value.len() > self.config.capacity.blob_max_bytes {
            return Err(Error::config(
                "store_budget_exceeded",
                format!(
                    "blob bytes {} exceed {}",
                    value.len(),
                    self.config.capacity.blob_max_bytes
                ),
            ));
        }
        let content_hash = stable_hash_hex(value);
        let event = self.build_memory_event(
            MemoryStoreEventKind::MemoryWrite,
            namespace,
            key,
            content_hash,
        );
        self.engine.put_blob_and_event(namespace, key, value, event)
    }

    fn blob_delete(&self, namespace: &str, key: &str) -> Result<bool> {
        let _transaction_guard = self.lock_transaction("store_blob_delete")?;
        enforce_logical_key_budget(self.config.capacity, namespace, key, "store_blob_delete")?;
        let content_hash = stable_hash_hex(&("delete", namespace, key));
        let event = self.build_memory_event(
            MemoryStoreEventKind::MemoryDelete,
            namespace,
            key,
            content_hash,
        );
        self.engine.delete_blob_and_event(namespace, key, event)
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

    fn build_batch_event(
        &self,
        batch: &StoreMutationBatch,
        kind: MemoryStoreEventKind,
        plane: &str,
        record_key: &str,
        content_hash: String,
    ) -> MemoryStoreEvent {
        MemoryStoreEvent::new(
            next_event_id(),
            kind,
            batch.scope.clone(),
            current_unix_secs(),
        )
        .with_plane(plane)
        .with_record_key(record_key)
        .with_content_hash(content_hash)
        .with_payload("transaction_id", batch.transaction_id.as_str())
        .with_payload("operation", batch.operation.as_str())
    }

    fn snapshot_json_map(&self) -> Result<BTreeMap<(String, String), serde_json::Value>> {
        let mut json_docs = BTreeMap::new();
        for namespace in JSON_SNAPSHOT_NAMESPACES {
            for key in self.engine.list_json_keys(namespace)? {
                if let Some(value) = self.engine.get_json_value(namespace, &key)? {
                    json_docs.insert(((*namespace).to_string(), key), value);
                }
            }
        }
        Ok(json_docs)
    }

    fn validate_json_preconditions(
        &self,
        preconditions: &[StoreJsonPrecondition],
        json_docs: &BTreeMap<(String, String), serde_json::Value>,
    ) -> Result<()> {
        for precondition in preconditions {
            let (namespace, key) = match precondition {
                StoreJsonPrecondition::Absent { namespace, key }
                | StoreJsonPrecondition::Exact { namespace, key, .. } => (namespace, key),
            };
            ensure_batch_json_namespace(namespace)?;
            enforce_logical_key_budget(
                self.config.capacity,
                namespace,
                key,
                "memory_write_transaction",
            )
            .map_err(memory_write_transaction_preflight_error)?;

            let actual = json_docs.get(&(namespace.clone(), key.clone()));
            let satisfied = match precondition {
                StoreJsonPrecondition::Absent { .. } => actual.is_none(),
                StoreJsonPrecondition::Exact { value, .. } => actual == Some(value),
            };
            if !satisfied {
                let expected = match precondition {
                    StoreJsonPrecondition::Absent { .. } => "absent",
                    StoreJsonPrecondition::Exact { .. } => "an exact JSON value",
                };
                return Err(Error::config(
                    "memory_write_transaction_precondition_failed",
                    format!(
                        "json precondition failed for namespace {namespace}, key {key}: expected {expected}"
                    ),
                ));
            }
        }
        Ok(())
    }

    fn snapshot_blob_map(&self) -> Result<BTreeMap<(String, String), Vec<u8>>> {
        let mut blobs = BTreeMap::new();
        for namespace in BLOB_SNAPSHOT_NAMESPACES {
            for key in self.engine.list_blob_keys(namespace)? {
                if let Some(value) = self.engine.get_blob(namespace, &key)? {
                    blobs.insert(((*namespace).to_string(), key), value);
                }
            }
        }
        Ok(blobs)
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
        enforce_event_key_budget(self.config.capacity, &event, "store_event_log")?;
        self.engine.append_event(event)
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
        MEMORY_FACET_INDEX_NAMESPACE,
        MEMORY_FACET_POSTING_NAMESPACE,
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        "memory_graph_manifests",
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

pub(crate) fn validate_governed_transaction_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    graph_repair_authorized: bool,
) -> Result<()> {
    validate_facet_post_image(batch, before, after)?;
    validate_graph_post_image(batch, before, after, graph_repair_authorized)?;
    validate_control_post_image(batch, before, after)
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

    let mut owner_ids = BTreeSet::new();
    let mut posting_keys = BTreeSet::new();
    for value in [manifest.before.as_ref(), manifest.after.as_ref()]
        .into_iter()
        .flatten()
    {
        owner_ids.extend(
            value
                .owner_versions
                .iter()
                .map(|owner| owner.owner_record_id.clone()),
        );
        posting_keys.extend(
            value
                .posting_revisions
                .iter()
                .map(|posting| posting.posting_key.clone()),
        );
    }

    let mut facet_keys = batch_json_keys(batch, MEMORY_FACET_INDEX_NAMESPACE);
    for owner_id in owner_ids.clone() {
        facet_keys.insert(
            scoped_memory_facet_owner_storage_key(memory_space_id, subject_id, &owner_id).map_err(
                |error| {
                    Error::config(
                        "memory_write_transaction_owner_facet_post_image_invalid",
                        format!("facet owner key: {error:?}"),
                    )
                },
            )?,
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
            owner_ids.insert(doc.owner_record_id.clone());
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
                owner_ids.insert(owner.id);
                owner_ids.insert(record_key.clone());
            }
            StoreMutation::DeleteJson {
                namespace,
                record_key,
                ..
            } if namespace == "long_term" => {
                owner_ids.insert(record_key.clone());
            }
            _ => {}
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
        "memory_write_transaction_owner_facet_post_image_invalid",
        validate_memory_facet_post_image(&MemoryFacetPostImageClosure {
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: subject_id.to_string(),
            owner_records,
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

fn graph_dependency_keys(manifests: [&Option<MemoryGraphScopeManifest>; 2]) -> GraphDependencyKeys {
    let mut keys = GraphDependencyKeys::default();
    for manifest in manifests.into_iter().filter_map(Option::as_ref) {
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
    let scoped_graph_after_present = graph_namespaces
        .iter()
        .any(|namespace| !scoped_graph_state_keys(after, namespace, &scope_digest).is_empty());
    if !graph_touched
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
    } = graph_dependency_keys([&manifest.before, &manifest.after]);
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

    let mut owner_ids = BTreeSet::new();
    let mut node_keys = batch_json_keys(batch, MEMORY_GRAPH_NODE_NAMESPACE);
    node_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_NODE_NAMESPACE,
        &scope_digest,
    ));
    for membership in &node_memberships {
        for value in membership.before.iter().chain(membership.after.iter()) {
            owner_ids.insert(value.owner_record_id.clone());
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
    let owner_records = owner_ids
        .into_iter()
        .map(|owner_id| {
            let key = scoped_long_term_memory_storage_key(memory_space_id, &owner_id)?;
            governed_image::<LongTermMemoryEntry>("long_term", &key, before, after)
        })
        .collect::<Result<Vec<_>>>()?;

    ensure_post_image_validation(
        "memory_write_transaction_graph_post_image_invalid",
        validate_memory_graph_post_image(&MemoryGraphPostImageClosure {
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: subject_id.to_string(),
            allow_missing_before_owners: graph_repair_authorized,
            validate_transition_successors: true,
            owner_records,
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
            actor_subject_id,
            owner_records,
            revisions,
            tombstones,
            policies,
            audits,
        }),
    )
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
    if graph_mutations.is_empty() {
        return Ok(());
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

fn validate_long_term_owner_facet_closure(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    let owner_ids = batch
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
            } if namespace == "long_term" => Some(record_key.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if owner_ids.is_empty() {
        return Ok(());
    }

    let facet_owner_ids = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == MEMORY_FACET_INDEX_NAMESPACE => {
                serde_json::from_value::<MemoryFacetIndexDoc>(value.clone())
                    .ok()
                    .map(|doc| doc.owner_record_id)
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
                                .map(|doc| doc.owner_record_id)
                        }
                        _ => None,
                    })
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let missing = owner_ids
        .difference(&facet_owner_ids)
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(Error::config(
        "memory_write_transaction_owner_facet_closure_missing",
        format!(
            "long-term owner mutations require same-transaction facet owner closure: {}",
            missing.join(",")
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

fn validate_mutation_batch_budget(
    capacity: StoreCapacityBudget,
    json_docs: &BTreeMap<(String, String), serde_json::Value>,
    blobs: &BTreeMap<(String, String), Vec<u8>>,
    events: &[MemoryStoreEvent],
) -> Result<()> {
    if events.len() > capacity.event_log_max_items {
        return Err(store_budget_error(format!(
            "memory_write_transaction events {} exceed {}",
            events.len(),
            capacity.event_log_max_items
        )));
    }
    let kv_entries = json_docs.len() + blobs.len();
    if kv_entries > capacity.kv_max_entries {
        return Err(store_budget_error(format!(
            "memory_write_transaction entries {} exceed {}",
            kv_entries, capacity.kv_max_entries
        )));
    }
    let blob_bytes = blobs.values().map(Vec::len).sum::<usize>();
    if blob_bytes > capacity.blob_max_bytes {
        return Err(store_budget_error(format!(
            "memory_write_transaction blob bytes {} exceed {}",
            blob_bytes, capacity.blob_max_bytes
        )));
    }
    Ok(())
}

fn validate_unique_event_ids(events: &[MemoryStoreEvent]) -> Result<()> {
    let mut event_ids = BTreeSet::new();
    for event in events {
        if !event_ids.insert(event.event_id.as_str()) {
            return Err(Error::config(
                "memory_write_transaction_preflight_failed",
                format!("duplicate event id {}", event.event_id),
            ));
        }
    }
    Ok(())
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
    "session_summary",
    "session",
    "long_term",
    "continuity_capsule",
    "turn_continuity_evidence",
    "private_garden",
    "remind_at",
    "task",
    "task_run",
    "task_artifact",
    "task_learning",
];

const BLOB_SNAPSHOT_NAMESPACES: &[&str] = &["state_fs", "skills", "memory", "daily"];

impl StoreEventLog for StorePlatform {
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

    fn runtime_resource_probe(&self) -> Arc<dyn RuntimeResourceProbe> {
        Arc::new(HostRuntimeResourceProbe)
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
        self.blob_put("skills", name, content)
    }

    fn remove(&self, name: &str) -> Result<()> {
        self.blob_delete("skills", name).map(|_| ())
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

impl_keyed_json_store!(ActiveWorkStore, ActiveWorkRecord, "active_work");
impl_keyed_json_store!(ExecutionStateStore, ExecutionState, "execution_state");
impl_keyed_json_store!(
    LongTermMemoryExtractionStateStore,
    LongTermMemoryExtractionState,
    "long_term_extraction_state"
);
impl_keyed_json_store!(TurnLedgerStore, TurnLedger, "turn_ledger");
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
        self.json_put(
            "session_summary",
            chat_id,
            &SessionSummaryRecord {
                summary: summary.to_string(),
                message_count: count,
            },
        )
    }

    fn set_with_count(&self, chat_id: &str, summary: &str, message_count: usize) -> Result<()> {
        self.json_put(
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
        self.blob_put("memory", "MEMORY.md", content.as_bytes())
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
        self.blob_put("daily", name, content.as_bytes())
    }
}

impl SessionStore for StorePlatform {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
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
        self.json_put("session", chat_id, &messages)
    }

    fn append_batch(&self, chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
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
        self.json_put("session", chat_id, &persisted)
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
        self.json_delete("session", chat_id).map(|_| ())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        self.engine.list_json_keys("session")
    }
}

impl ConversationTranscriptStore for StorePlatform {
    fn append_turn(&self, record: &TranscriptTurnRecord) -> Result<TranscriptCommitReport> {
        let key = record.key.turn_storage_key(&record.turn_id);
        let before_count = self.list_turns(&record.key, usize::MAX)?.len();
        if self
            .json_get::<TranscriptTurnRecord>("conversation_transcript", &key)?
            .is_some()
        {
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
        self.json_put("conversation_transcript", &key, &record)?;
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
        self.json_put("conversation_transcript_alias", &alias.storage_key(), alias)
    }

    fn resolve_conversation_alias(
        &self,
        memory_space_id: &str,
        channel_id: &str,
        chat_id: &str,
    ) -> Result<Option<String>> {
        let key =
            TranscriptConversationAlias::storage_key_for(memory_space_id, channel_id, chat_id);
        Ok(self
            .json_get::<TranscriptConversationAlias>("conversation_transcript_alias", &key)?
            .map(|alias| alias.conversation_id))
    }

    fn get_turn(
        &self,
        key: &ConversationKey,
        turn_id: &str,
    ) -> Result<Option<TranscriptTurnRecord>> {
        self.json_get::<TranscriptTurnRecord>(
            "conversation_transcript",
            &key.turn_storage_key(turn_id),
        )
    }

    fn list_turns(&self, key: &ConversationKey, limit: usize) -> Result<Vec<TranscriptTurnRecord>> {
        let prefix = key.turn_storage_key_prefix();
        let mut records = Vec::new();
        for record_key in self.engine.list_json_keys("conversation_transcript")? {
            if !record_key.starts_with(&prefix) {
                continue;
            }
            if let Some(record) =
                self.json_get::<TranscriptTurnRecord>("conversation_transcript", &record_key)?
            {
                records.push(record);
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
        attrs: &[TranscriptAttrEnvelope],
    ) -> Result<TranscriptAttrWriteReport> {
        let mut accepted_attrs = Vec::new();
        let mut rejected_attrs = Vec::new();
        for attr in attrs {
            if attr.target.key != *key {
                rejected_attrs.push(transcript_attr_rejection(
                    attr,
                    "attr target key does not match conversation key",
                ));
                continue;
            }
            let Some(turn) = self.get_turn(key, &attr.target.turn_id)? else {
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
            self.json_put(
                "conversation_transcript_attr",
                &transcript_attr_storage_key(key, attr),
                attr,
            )?;
            accepted_attrs.push(attr.clone());
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
        turn_id: Option<&str>,
    ) -> Result<Vec<TranscriptAttrEnvelope>> {
        let prefix = transcript_attr_storage_key_prefix(key);
        let mut attrs = Vec::new();
        for record_key in self.engine.list_json_keys("conversation_transcript_attr")? {
            if !record_key.starts_with(&prefix) {
                continue;
            }
            let Some(value) = self
                .engine
                .get_json_value("conversation_transcript_attr", &record_key)?
            else {
                continue;
            };
            let Ok(attr) = serde_json::from_value::<TranscriptAttrEnvelope>(value) else {
                continue;
            };
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

    fn list_transcript_attr_repair_issues(
        &self,
        key: &ConversationKey,
    ) -> Result<Vec<TranscriptRepairIssue>> {
        let prefix = transcript_attr_storage_key_prefix(key);
        let mut issues = Vec::new();
        for record_key in self.engine.list_json_keys("conversation_transcript_attr")? {
            if !record_key.starts_with(&prefix) {
                continue;
            }
            let Some(value) = self
                .engine
                .get_json_value("conversation_transcript_attr", &record_key)?
            else {
                continue;
            };
            if let Err(error) = serde_json::from_value::<TranscriptAttrEnvelope>(value.clone()) {
                let (turn_id, message_id) = transcript_attr_repair_target_from_value(&value);
                issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::CorruptTranscriptAttrRecord,
                    turn_id,
                    message_id,
                    derived_ref: None,
                    reason: format!("transcript_attr_decode_failed:{error}"),
                });
            }
        }
        Ok(issues)
    }

    fn append_derived_memory_ref(
        &self,
        key: &ConversationKey,
        derived: &DerivedMemoryRef,
    ) -> Result<()> {
        validate_derived_ref_matches_key(key, derived)?;
        let record_key = transcript_derived_ref_storage_key(key, derived)?;
        self.json_put("conversation_transcript_derived_ref", &record_key, derived)
    }

    fn list_derived_memory_refs(
        &self,
        key: &ConversationKey,
        turn_id: Option<&str>,
    ) -> Result<Vec<DerivedMemoryRef>> {
        let prefix = transcript_derived_ref_storage_key_prefix(key);
        let mut refs = Vec::new();
        for record_key in self
            .engine
            .list_json_keys("conversation_transcript_derived_ref")?
        {
            if !record_key.starts_with(&prefix) {
                continue;
            }
            let Some(derived) = self
                .json_get::<DerivedMemoryRef>("conversation_transcript_derived_ref", &record_key)?
            else {
                continue;
            };
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
        request: &TranscriptLifecycleRequest,
    ) -> Result<TranscriptLifecycleReport> {
        let mut affected_turns = 0usize;
        let mut affected_turn_ids = Vec::new();
        let mut affected_message_ids = Vec::new();
        let mut affected_host_refs = Vec::new();
        let mut records = self.list_turns(&request.key, usize::MAX)?;
        for record in &mut records {
            let matches_turn = request
                .turn_id
                .as_ref()
                .map(|turn_id| turn_id == &record.turn_id)
                .unwrap_or(true);
            if matches_turn {
                affected_turn_ids.push(record.turn_id.clone());
                for message in &record.input_messages {
                    affected_message_ids.push(message.message_id.clone());
                }
                if let Some(message) = &record.assistant_message {
                    affected_message_ids.push(message.message_id.clone());
                }
                affected_host_refs.extend(record.host_refs.clone());
                record.apply_lifecycle_transition(request.transition, request.requested_at);
                let record_key = request.key.turn_storage_key(&record.turn_id);
                self.json_put("conversation_transcript", &record_key, record)?;
                affected_turns = affected_turns.saturating_add(1);
            }
        }
        let derived_memory_refs = self
            .list_derived_memory_refs(&request.key, None)?
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

fn transcript_derived_ref_storage_key_prefix(key: &ConversationKey) -> String {
    format!("{}__derived_ref__", key.storage_key())
}

fn transcript_derived_ref_storage_key(
    key: &ConversationKey,
    derived: &DerivedMemoryRef,
) -> Result<String> {
    let payload = serde_json::to_string(derived)
        .map_err(|error| Error::config("conversation_transcript_derived_ref", error.to_string()))?;
    Ok(format!(
        "{}{}",
        transcript_derived_ref_storage_key_prefix(key),
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

fn transcript_attr_storage_key_prefix(key: &ConversationKey) -> String {
    format!("{}__attr__", key.storage_key())
}

fn transcript_attr_storage_key(key: &ConversationKey, attr: &TranscriptAttrEnvelope) -> String {
    format!(
        "{}{}",
        transcript_attr_storage_key_prefix(key),
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
    fn physical_key(&self, namespace: &str, logical_key: &str) -> Result<String> {
        scoped_long_term_control_storage_key(&self.memory_space_id, namespace, logical_key)
    }

    fn list_scoped<T>(&self, namespace: &str) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let prefix = scoped_long_term_control_storage_prefix(&self.memory_space_id, namespace)?;
        let mut values = Vec::new();
        for key in self.platform.engine.list_json_keys(namespace)? {
            if key.starts_with(&prefix) {
                if let Some(value) = self.platform.json_get(namespace, &key)? {
                    values.push(value);
                }
            }
        }
        Ok(values)
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
        self.platform.json_get(
            LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
            &self.physical_key(LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE, record_id)?,
        )
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
                self.json_put("continuity_capsule", &capsule_id, &capsule)?;
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
        self.json_put("task_run", &record.run.run_id, record)
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
        self.json_put("task_learning", &record.learning_id, record)
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

fn current_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(feature = "sqlite-store")]
fn sqlite_engine(
    config: &StoreBackendConfig,
) -> Result<(Arc<dyn StoreEngine>, StoreSchemaManifest)> {
    let (engine, manifest) = SqliteStoreEngine::open(config)?;
    Ok((Arc::new(engine), manifest))
}

#[cfg(not(feature = "sqlite-store"))]
fn sqlite_engine(
    _config: &StoreBackendConfig,
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
    }

    let blob_namespaces = BLOB_SNAPSHOT_NAMESPACES
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut blob_keys = HashSet::new();
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
    }

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
}
