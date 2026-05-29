use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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

#[cfg(feature = "sqlite-store")]
use crate::sqlite::SqliteStoreEngine;
use crate::{
    embedded::EmbeddedStoreEngine, file::FileStoreEngine, InMemoryStoreEngine, MemoryStoreEvent,
    MemoryStoreEventKind, StoreBackendConfig, StoreBackendKind, StoreEngine, StoreEventLog,
    StoreEventScope, StoreOpenReport, StoreRepairReport, StoreSchemaManifest, StoreSnapshot,
    StoreSnapshotBlob, StoreSnapshotExportReport, StoreSnapshotImportReport, StoreSnapshotJsonDoc,
    STORE_SCHEMA_ID, STORE_SCHEMA_VERSION,
};

static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct StorePlatform {
    config: StoreBackendConfig,
    engine: Arc<dyn StoreEngine>,
    schema_manifest: StoreSchemaManifest,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct SessionSummaryRecord {
    summary: String,
    message_count: usize,
}

impl StorePlatform {
    pub fn open(config: StoreBackendConfig) -> Result<Self> {
        Self::open_with_report(config).map(|(platform, _report)| platform)
    }

    pub fn open_with_report(config: StoreBackendConfig) -> Result<(Self, StoreOpenReport)> {
        let now_secs = current_unix_secs();
        let (engine, repair, schema_manifest): (
            Arc<dyn StoreEngine>,
            StoreRepairReport,
            StoreSchemaManifest,
        ) = match config.backend {
            StoreBackendKind::InMemory => (
                Arc::new(InMemoryStoreEngine::default()),
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
            schema_manifest,
        };
        platform.emit_runtime_event("open")?;
        Ok((platform, report))
    }

    pub fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        self.engine.read_events()
    }

    pub fn read_file_store_events(root: impl AsRef<Path>) -> Result<Vec<MemoryStoreEvent>> {
        crate::file::read_events_from_root(root.as_ref())
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

    pub fn into_arc(self) -> Arc<dyn Platform> {
        Arc::new(self)
    }

    pub fn export_store_snapshot(&self) -> Result<StoreSnapshot> {
        self.export_store_snapshot_with_report()
            .map(|(snapshot, _report)| snapshot)
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
        self.enforce_snapshot_budget(&snapshot)?;
        let report = snapshot.export_report();
        Ok((snapshot, report))
    }

    pub fn import_store_snapshot(&self, snapshot: &StoreSnapshot) -> Result<()> {
        self.import_store_snapshot_with_report(snapshot)
            .map(|_report| ())
    }

    pub fn import_store_snapshot_with_report(
        &self,
        snapshot: &StoreSnapshot,
    ) -> Result<StoreSnapshotImportReport> {
        validate_snapshot_import_contract(snapshot)?;
        self.enforce_snapshot_budget(snapshot)?;
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
        let event = self.build_memory_event(
            MemoryStoreEventKind::MemoryWrite,
            namespace,
            key,
            content_hash,
        );
        self.engine
            .put_json_value_and_event(namespace, key, value, event)
    }

    fn json_delete(&self, namespace: &str, key: &str) -> Result<bool> {
        let content_hash = stable_hash_hex(&("delete", namespace, key));
        let event = self.build_memory_event(
            MemoryStoreEventKind::MemoryDelete,
            namespace,
            key,
            content_hash,
        );
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
        self.engine.append_event(event)
    }

    fn enforce_snapshot_budget(&self, snapshot: &StoreSnapshot) -> Result<()> {
        let bytes = serde_json::to_vec(snapshot)
            .map_err(|error| Error::config("store_snapshot_budget", error.to_string()))?;
        if bytes.len() > self.config.capacity.snapshot_max_bytes {
            return Err(Error::config(
                "store_budget_exceeded",
                format!(
                    "snapshot bytes {} exceed {}",
                    bytes.len(),
                    self.config.capacity.snapshot_max_bytes
                ),
            ));
        }
        Ok(())
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
        self.engine.append_event(event)
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
        self.engine.append_event(store_event)
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

    fn session_summary_store(&self) -> Arc<dyn SessionSummaryStore> {
        Arc::new(self.clone())
    }

    fn long_term_memory_store(&self) -> Arc<dyn LongTermMemoryStore> {
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

impl LongTermMemoryStore for StorePlatform {
    fn upsert_many(&self, drafts: &[LongTermMemoryDraft], now_secs: u64) -> Result<usize> {
        let mut changed = 0usize;
        for draft in drafts {
            let Some(normalized) = draft.normalized() else {
                continue;
            };
            let id = normalized.stable_id().unwrap_or_else(|| {
                stable_hash_id("ltm", &(normalized.kind.label(), &normalized.topic))
            });
            let prior = LongTermMemoryStore::get(self, &id)?;
            let observed_at = normalized.observed_at.unwrap_or(now_secs);
            let last_confirmed_at = normalized
                .last_confirmed_at
                .unwrap_or(observed_at)
                .max(observed_at);
            let entry = LongTermMemoryEntry {
                id: id.clone(),
                kind: normalized.kind,
                topic: normalized.topic,
                content: normalized.content,
                keywords: normalized.keywords,
                source_chat_id: normalized.source_chat_id,
                source_type: normalized.source_type.unwrap_or_default(),
                source_scope: normalized.source_scope.unwrap_or_default(),
                confidence: normalized.confidence.unwrap_or_default(),
                freshness: normalized.freshness.unwrap_or_default(),
                stale_hint: normalized.stale_hint.unwrap_or_default(),
                supporting_citations: normalized.supporting_citations,
                evidence_count: normalized.evidence_count.unwrap_or(0),
                created_at: prior
                    .as_ref()
                    .map(|entry| entry.created_at)
                    .unwrap_or(now_secs),
                updated_at: now_secs,
                observed_at,
                last_confirmed_at,
                source_revision: normalized.source_revision.unwrap_or(0),
                last_used_at: prior.as_ref().map(|entry| entry.last_used_at).unwrap_or(0),
            };
            self.json_put("long_term", &id, &entry)?;
            changed = changed.saturating_add(1);
        }
        Ok(changed)
    }

    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = LongTermMemoryStore::list(self, usize::MAX)?;
        let query = query.trim().to_lowercase();
        if !query.is_empty() {
            entries.retain(|entry| {
                entry.topic.to_lowercase().contains(&query)
                    || entry.content.to_lowercase().contains(&query)
                    || entry
                        .keywords
                        .iter()
                        .any(|keyword: &String| keyword.to_lowercase().contains(&query))
            });
        }
        entries.sort_by(|left, right| {
            let left_scope = usize::from(left.source_chat_id.as_deref() == source_chat_id);
            let right_scope = usize::from(right.source_chat_id.as_deref() == source_chat_id);
            right_scope
                .cmp(&left_scope)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
        });
        entries.truncate(limit);
        Ok(entries)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        self.json_get("long_term", id)
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = self.json_list::<LongTermMemoryEntry>("long_term", limit)?;
        entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(entries)
    }

    fn delete(&self, id: &str) -> Result<bool> {
        self.json_delete("long_term", id)
    }

    fn delete_slot(&self, slot: &LongTermMemorySlot) -> Result<bool> {
        let Some(id) = slot.stable_id() else {
            return Ok(false);
        };
        LongTermMemoryStore::delete(self, &id)
    }

    fn count(&self) -> Result<usize> {
        self.engine
            .list_json_keys("long_term")
            .map(|keys| keys.len())
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
