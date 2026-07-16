use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "nonproduction-replay-harness")]
use std::io::{Read, Write};
#[cfg(feature = "nonproduction-replay-harness")]
use std::net::TcpStream;

use bm_core::{Error, Result};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension, TransactionBehavior};
use serde_json::Value;

#[cfg(feature = "nonproduction-replay-harness")]
use crate::enforce_event_key_budget;
#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_internal::transaction::{
    read_consistent_from_state, validate_restore_post_image_blob_bytes,
};
use crate::{
    enforce_logical_key_budget, store_budget_error,
    store_internal::transaction::{
        apply_transaction, read_bounded_known_keys_from_parts,
        scoped_projection_dependency_addresses, scoped_projection_root_addresses,
        validate_scoped_projection_post_image, BackendTransactionState, StoreAdmissionAuthority,
        StoreBackendUsage, StoreBoundedKnownBlobRead, StoreBoundedKnownJsonRead,
        StoreBoundedKnownKeyReadResult, StoreImmutableReadSession, StoreReadReceipt,
        StoreReadSessionState, StoreTransactionAdmission, StoreTransactionContext,
    },
    MemoryStoreEvent, StoreBackendConfig, StoreCapacityBudget, StoreEngine, StoreEngineMutation,
    StoreEventLog, StoreSchemaManifest, StoreTransactionReport, StoreTransactionRequest,
    STORE_SCHEMA_ID, STORE_SCHEMA_VERSION,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreConsistentReadRequest, StoreConsistentReadResult};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreSnapshotBlob, StoreSnapshotJsonDoc, StoreSnapshotReplaceReport};

pub struct SqliteStoreEngine {
    capacity: StoreCapacityBudget,
    admission_authority: StoreAdmissionAuthority,
    connection: Mutex<Connection>,
}

struct ScopedJsonRead {
    documents: BTreeMap<(String, String), Value>,
    logical_bytes: usize,
}

fn read_scoped_json_exact(
    connection: &Connection,
    request: &crate::StoreScopedProjectionRequest,
    capacity: StoreCapacityBudget,
) -> Result<ScopedJsonRead> {
    let mut documents = BTreeMap::new();
    let mut logical_bytes = 0_usize;
    let mut observed = BTreeSet::new();
    let mut pending = scoped_projection_root_addresses(&request.json_namespaces, &request.scope)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    while let Some((namespace, key)) = pending.pop_first() {
        if !observed.insert((namespace.clone(), key.clone())) {
            continue;
        }
        if observed.len() > capacity.kv_max_entries {
            return Err(Error::config(
                "store_scoped_projection_budget_exceeded",
                "scoped projection exact-key reads exceed the pinned operation entry budget",
            ));
        }
        let raw = connection
            .query_row(
                "SELECT value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![&namespace, &key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        let Some(raw) = raw else { continue };
        let value = serde_json::from_str::<Value>(&raw)
            .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?;
        let value_bytes = serde_json::to_vec(&value)
            .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?
            .len();
        logical_bytes = logical_bytes
            .checked_add(value_bytes)
            .ok_or_else(|| store_budget_error("scoped projection JSON byte overflow"))?;
        if logical_bytes > capacity.snapshot_max_bytes {
            return Err(Error::config(
                "store_scoped_projection_budget_exceeded",
                "scoped projection exceeds the pinned operation byte budget",
            ));
        }
        documents.insert((namespace, key), value);
        for dependency in scoped_projection_dependency_addresses(
            &documents,
            &request.json_namespaces,
            &request.scope,
        )? {
            if !observed.contains(&dependency) {
                pending.insert(dependency);
            }
        }
    }
    crate::store_internal::transaction::validate_scoped_recall_manifest_documents(
        &documents,
        &BTreeMap::new(),
        &request.scope,
    )?;
    crate::store_internal::transaction::validate_scoped_control_plane_documents(
        &documents,
        &request.scope,
        capacity.kv_max_entries,
    )?;
    Ok(ScopedJsonRead {
        documents,
        logical_bytes,
    })
}

struct SqliteImmutableReadSession<'a> {
    connection: MutexGuard<'a, Connection>,
    read: StoreReadSessionState,
}

impl Drop for SqliteImmutableReadSession<'_> {
    fn drop(&mut self) {
        let _ = self.connection.execute_batch("ROLLBACK");
    }
}

impl StoreImmutableReadSession for SqliteImmutableReadSession<'_> {
    fn read_json_known_keys(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreBoundedKnownJsonRead>> {
        let mut reads = Vec::with_capacity(addresses.len());
        for (namespace, key) in addresses {
            let raw: Option<String> = self
                .connection
                .query_row(
                    "SELECT value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_immutable_read_session", error))?;
            let value = raw
                .map(|raw| {
                    if raw.len() > self.read.remaining_json_bytes() {
                        return Err(Error::config(
                            "store_consistent_read_budget_exceeded",
                            "SQLite JSON value exceeds remaining immutable session ceiling",
                        ));
                    }
                    serde_json::from_str(&raw).map_err(|error| {
                        Error::config("store_immutable_read_session", error.to_string())
                    })
                })
                .transpose()?;
            reads.push(self.read.record_json(namespace, key, value)?);
        }
        Ok(reads)
    }

    fn read_blob_known_keys(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreBoundedKnownBlobRead>> {
        let mut reads = Vec::with_capacity(addresses.len());
        for (namespace, key) in addresses {
            let value: Option<Vec<u8>> = self
                .connection
                .query_row(
                    "SELECT value_blob FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_immutable_read_session", error))?;
            if value
                .as_ref()
                .is_some_and(|value| value.len() > self.read.remaining_blob_bytes())
            {
                return Err(Error::config(
                    "store_consistent_read_budget_exceeded",
                    "SQLite blob exceeds remaining immutable session ceiling",
                ));
            }
            reads.push(self.read.record_blob(namespace, key, value)?);
        }
        Ok(reads)
    }

    fn receipt(&self) -> Result<StoreReadReceipt> {
        self.read.receipt()
    }
}

impl SqliteStoreEngine {
    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn open_with_capacity(
        config: &StoreBackendConfig,
        capacity: StoreCapacityBudget,
    ) -> Result<(Self, StoreSchemaManifest)> {
        Self::open_with_capacity_and_authority(config, capacity, StoreAdmissionAuthority::new())
    }

    pub(crate) fn open_with_capacity_and_authority(
        config: &StoreBackendConfig,
        capacity: StoreCapacityBudget,
        admission_authority: StoreAdmissionAuthority,
    ) -> Result<(Self, StoreSchemaManifest)> {
        let path = config
            .data_path
            .clone()
            .ok_or_else(|| Error::config("sqlite_store_open", "sqlite path is required"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::io("sqlite_store_open", error))?;
        }
        let connection =
            Connection::open(&path).map_err(|error| Error::storage("sqlite_store_open", error))?;
        connection
            .busy_timeout(config.lock_timeout)
            .map_err(|error| Error::storage("sqlite_store_open", error))?;
        let engine = Self {
            capacity,
            admission_authority,
            connection: Mutex::new(connection),
        };
        let manifest = engine.init_schema(config, path)?;
        Ok((engine, manifest))
    }

    fn init_schema(
        &self,
        config: &StoreBackendConfig,
        path: PathBuf,
    ) -> Result<StoreSchemaManifest> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_open", error.to_string()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        transaction
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS bm_schema (
                    schema_id TEXT PRIMARY KEY,
                    schema_version INTEGER NOT NULL,
                    manifest_json TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS bm_event_log (
                    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_id TEXT NOT NULL UNIQUE,
                    event_json TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS bm_kv (
                    namespace TEXT NOT NULL,
                    key TEXT NOT NULL,
                    value_json TEXT NOT NULL,
                    PRIMARY KEY(namespace, key)
                );
                CREATE TABLE IF NOT EXISTS bm_blob (
                    namespace TEXT NOT NULL,
                    key TEXT NOT NULL,
                    value_blob BLOB NOT NULL,
                    PRIMARY KEY(namespace, key)
                );
                CREATE TABLE IF NOT EXISTS bm_snapshot_manifest (
                    snapshot_id TEXT PRIMARY KEY,
                    manifest_json TEXT NOT NULL
                );
                "#,
            )
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        let incompatible_schema: Option<String> = transaction
            .query_row(
                "SELECT schema_id FROM bm_schema WHERE schema_id <> ?1 LIMIT 1",
                params![STORE_SCHEMA_ID],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        if let Some(schema_id) = incompatible_schema {
            return Err(Error::config(
                "sqlite_store_schema",
                format!("unsupported schema {} in {}", schema_id, path.display()),
            ));
        }
        let existing: Option<String> = transaction
            .query_row(
                "SELECT manifest_json FROM bm_schema WHERE schema_id = ?1",
                params![STORE_SCHEMA_ID],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        let now_secs = current_unix_secs();
        let manifest = match existing {
            Some(raw) => {
                let mut manifest: StoreSchemaManifest = serde_json::from_str(&raw)
                    .map_err(|error| Error::config("sqlite_store_schema", error.to_string()))?;
                if manifest.schema_id != STORE_SCHEMA_ID {
                    return Err(Error::config(
                        "sqlite_store_schema",
                        format!(
                            "unsupported schema {} in {}",
                            manifest.schema_id,
                            path.display()
                        ),
                    ));
                }
                manifest.validate_against(
                    config.backend,
                    config.profile,
                    config.memory_system_kind,
                    "sqlite_store_schema",
                )?;
                manifest.touch_opened(now_secs);
                manifest
            }
            None => {
                let persistent_state_exists: bool = transaction
                    .query_row(
                        r#"
                        SELECT
                            EXISTS(SELECT 1 FROM bm_event_log) OR
                            EXISTS(SELECT 1 FROM bm_kv) OR
                            EXISTS(SELECT 1 FROM bm_blob) OR
                            EXISTS(SELECT 1 FROM bm_snapshot_manifest)
                        "#,
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|error| Error::storage("sqlite_store_schema", error))?;
                if persistent_state_exists {
                    return Err(Error::config(
                        "sqlite_store_schema",
                        format!("schema is missing for non-empty store {}", path.display()),
                    ));
                }
                StoreSchemaManifest::new(config.backend, config.profile, now_secs)
            }
        };
        let raw = serde_json::to_string(&manifest)
            .map_err(|error| Error::config("sqlite_store_schema", error.to_string()))?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO bm_schema(schema_id, schema_version, manifest_json) VALUES (?1, ?2, ?3)",
                params![STORE_SCHEMA_ID, STORE_SCHEMA_VERSION, raw],
            )
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        transaction
            .commit()
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        Ok(manifest)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn ensure_can_insert_event(
        capacity: StoreCapacityBudget,
        connection: &Connection,
        event: &MemoryStoreEvent,
    ) -> Result<()> {
        enforce_event_key_budget(capacity, event, "store_event_log")?;
        let duplicate: Option<String> = connection
            .query_row(
                "SELECT event_id FROM bm_event_log WHERE event_id = ?1",
                params![&event.event_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("store_event_log", error))?;
        if duplicate.is_some() {
            return Err(Error::config("store_event_log", "duplicate event id"));
        }
        let count: usize = connection
            .query_row("SELECT COUNT(*) FROM bm_event_log", [], |row| row.get(0))
            .map_err(|error| Error::storage("store_event_log", error))?;
        if count >= capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                count.saturating_add(1),
                capacity.event_log_max_items
            )));
        }
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn ensure_json_entry_budget(
        capacity: StoreCapacityBudget,
        connection: &Connection,
        namespace: &str,
        key: &str,
    ) -> Result<()> {
        enforce_logical_key_budget(capacity, namespace, key, "sqlite_store_json_write")?;
        let exists: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("sqlite_store_json_write", error))?;
        if exists.is_some() {
            return Ok(());
        }
        let count: usize = connection
            .query_row("SELECT COUNT(*) FROM bm_kv", [], |row| row.get(0))
            .map_err(|error| Error::storage("sqlite_store_json_write", error))?;
        if count >= capacity.kv_max_entries {
            return Err(store_budget_error(format!(
                "kv entries {} exceed {}",
                count.saturating_add(1),
                capacity.kv_max_entries
            )));
        }
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn ensure_blob_budget(
        capacity: StoreCapacityBudget,
        connection: &Connection,
        namespace: &str,
        key: &str,
        value_len: usize,
    ) -> Result<()> {
        enforce_logical_key_budget(capacity, namespace, key, "sqlite_store_blob_write")?;
        let previous: Option<usize> = connection
            .query_row(
                "SELECT length(value_blob) FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("sqlite_store_blob_write", error))?;
        let current: usize = connection
            .query_row(
                "SELECT COALESCE(SUM(length(value_blob)), 0) FROM bm_blob",
                [],
                |row| row.get(0),
            )
            .map_err(|error| Error::storage("sqlite_store_blob_write", error))?;
        let next = current
            .saturating_sub(previous.unwrap_or(0))
            .saturating_add(value_len);
        if next > capacity.blob_max_bytes {
            return Err(store_budget_error(format!(
                "blob bytes {} exceed {}",
                next, capacity.blob_max_bytes
            )));
        }
        Ok(())
    }

    fn load_transaction_context(
        connection: &Connection,
        request: &StoreTransactionRequest,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreTransactionContext> {
        let mut touched = BackendTransactionState::default();
        let mut json_bytes = 0_usize;
        for (namespace, key) in &request.read_set().json {
            enforce_logical_key_budget(capacity, namespace, key, "memory_write_transaction")?;
            let row: Option<(usize, String)> = connection
                .query_row(
                    "SELECT length(value_json), value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            if let Some((bytes, raw)) = row {
                json_bytes = json_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("transaction touched JSON byte count overflow")
                })?;
                if json_bytes > capacity.snapshot_max_bytes {
                    return Err(store_budget_error(format!(
                        "transaction touched JSON bytes {json_bytes} exceed {}",
                        capacity.snapshot_max_bytes
                    )));
                }
                let value = serde_json::from_str(&raw).map_err(|error| {
                    Error::config("memory_write_transaction", error.to_string())
                })?;
                touched.json.insert((namespace.clone(), key.clone()), value);
            }
        }
        for (namespace, prefix) in &request.read_set().json_prefixes {
            enforce_logical_key_budget(capacity, namespace, prefix, "memory_write_transaction")?;
            let mut statement = connection
                .prepare(
                    "SELECT key, length(value_json), value_json FROM bm_kv \
                     WHERE namespace = ?1 AND substr(key, 1, length(?2)) = ?2 ORDER BY key",
                )
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            let mut rows = statement
                .query(params![namespace, prefix])
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            while let Some(row) = rows
                .next()
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?
            {
                let key: String = row
                    .get(0)
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                if touched.json.contains_key(&(namespace.clone(), key.clone())) {
                    continue;
                }
                let bytes: usize = row
                    .get(1)
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                let raw: String = row
                    .get(2)
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                json_bytes = json_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("transaction touched JSON byte count overflow")
                })?;
                if json_bytes > capacity.snapshot_max_bytes {
                    return Err(store_budget_error(format!(
                        "transaction touched JSON bytes {json_bytes} exceed {}",
                        capacity.snapshot_max_bytes
                    )));
                }
                let value = serde_json::from_str(&raw).map_err(|error| {
                    Error::config("memory_write_transaction", error.to_string())
                })?;
                touched.json.insert((namespace.clone(), key), value);
            }
        }
        let mut touched_blob_bytes = 0_usize;
        for (namespace, key) in &request.read_set().blobs {
            enforce_logical_key_budget(capacity, namespace, key, "memory_write_transaction")?;
            let row: Option<(usize, Vec<u8>)> = connection
                .query_row(
                    "SELECT length(value_blob), value_blob FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            if let Some((bytes, value)) = row {
                touched_blob_bytes = touched_blob_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("transaction touched blob byte count overflow")
                })?;
                if touched_blob_bytes > capacity.blob_max_bytes {
                    return Err(store_budget_error(format!(
                        "transaction touched blob bytes {touched_blob_bytes} exceed {}",
                        capacity.blob_max_bytes
                    )));
                }
                touched
                    .blobs
                    .insert((namespace.clone(), key.clone()), value);
            }
        }
        let event_ids = request
            .mutations
            .iter()
            .filter_map(crate::store_internal::transaction::mutation_event_id)
            .collect::<BTreeSet<_>>();
        let mut existing_event_ids = BTreeSet::new();
        for event_id in event_ids {
            let exists: Option<String> = connection
                .query_row(
                    "SELECT event_id FROM bm_event_log WHERE event_id = ?1",
                    params![event_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            existing_event_ids.extend(exists);
        }
        let usage = StoreBackendUsage {
            kv_entries: connection
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM bm_kv) + (SELECT COUNT(*) FROM bm_blob)",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?,
            blob_bytes: connection
                .query_row(
                    "SELECT COALESCE(SUM(length(value_blob)), 0) FROM bm_blob",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?,
            event_count: connection
                .query_row("SELECT COUNT(*) FROM bm_event_log", [], |row| row.get(0))
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?,
        };
        Ok(StoreTransactionContext {
            touched,
            usage,
            existing_event_ids,
        })
    }
}

impl StoreEventLog for SqliteStoreEngine {
    #[cfg(feature = "nonproduction-replay-harness")]
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        let raw = serde_json::to_string(&event)
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        Self::ensure_can_insert_event(self.capacity, &connection, &event)?;
        connection
            .execute(
                "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                params![event.event_id, raw],
            )
            .map_err(|error| {
                if error.to_string().contains("UNIQUE") {
                    Error::config("store_event_log", "duplicate event id")
                } else {
                    Error::storage("store_event_log", error)
                }
            })?;
        Ok(())
    }

    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        let count: usize = connection
            .query_row("SELECT COUNT(*) FROM bm_event_log", [], |row| row.get(0))
            .map_err(|error| Error::storage("store_event_log", error))?;
        if count > self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                count, self.capacity.event_log_max_items
            )));
        }
        let mut statement = connection
            .prepare("SELECT event_json FROM bm_event_log ORDER BY sequence ASC")
            .map_err(|error| Error::storage("store_event_log", error))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| Error::storage("store_event_log", error))?;
        let mut events = Vec::new();
        for row in rows {
            let raw = row.map_err(|error| Error::storage("store_event_log", error))?;
            events.push(
                serde_json::from_str(&raw)
                    .map_err(|error| Error::config("store_event_log", error.to_string()))?,
            );
        }
        Ok(events)
    }
}

impl StoreEngine for SqliteStoreEngine {
    fn admission_authority(&self) -> &StoreAdmissionAuthority {
        &self.admission_authority
    }
    #[cfg(feature = "nonproduction-replay-harness")]
    fn store_capacity(&self) -> StoreCapacityBudget {
        self.capacity
    }

    fn commit_transaction_admitted(
        &self,
        request: &StoreTransactionRequest,
        admission: &StoreTransactionAdmission,
    ) -> Result<StoreTransactionReport> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("memory_write_transaction", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
        #[cfg(feature = "nonproduction-replay-harness")]
        replay_harness_pause_after_begin_immediate()?;
        admission.validate_inside_engine_fence(self.capacity, &self.admission_authority)?;
        let context = Self::load_transaction_context(&tx, request, admission.operation_capacity())?;
        let plan = apply_transaction(admission, request, &context)?;

        for mutation in &plan.effective_request.mutations {
            match mutation {
                StoreEngineMutation::PutJson {
                    namespace,
                    key,
                    value,
                } => {
                    let raw = serde_json::to_string(value).map_err(|error| {
                        Error::config("memory_write_transaction", error.to_string())
                    })?;
                    tx.execute(
                        "INSERT OR REPLACE INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, ?3)",
                        params![namespace, key, raw],
                    )
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                }
                StoreEngineMutation::DeleteJson { namespace, key } => {
                    tx.execute(
                        "DELETE FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                        params![namespace, key],
                    )
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                }
                StoreEngineMutation::PutBlob {
                    namespace,
                    key,
                    value,
                } => {
                    tx.execute(
                        "INSERT OR REPLACE INTO bm_blob(namespace, key, value_blob) VALUES (?1, ?2, ?3)",
                        params![namespace, key, value],
                    )
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                }
                StoreEngineMutation::DeleteBlob { namespace, key } => {
                    tx.execute(
                        "DELETE FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                        params![namespace, key],
                    )
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                }
                StoreEngineMutation::AppendEvent { event } => {
                    let raw = serialize_event(event)?;
                    tx.execute(
                        "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                        params![&event.event_id, raw],
                    )
                    .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                }
                StoreEngineMutation::DeleteJsonIfPresent { .. }
                | StoreEngineMutation::DeleteBlobIfPresent { .. } => {
                    return Err(Error::config(
                        "memory_write_transaction",
                        "conditional mutation reached SQLite primitive execution",
                    ));
                }
            }
        }
        tx.commit()
            .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
        Ok(plan.report)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn read_consistent(
        &self,
        request: &StoreConsistentReadRequest,
    ) -> Result<StoreConsistentReadResult> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| map_transaction_error("store_consistent_read", error))?;
        let mut state = BackendTransactionState::default();
        for address in &request.json {
            let raw: Option<String> = tx
                .query_row(
                    "SELECT value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                    params![&address.namespace, &address.key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            if let Some(raw) = raw {
                let value = serde_json::from_str(&raw)
                    .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
                state
                    .json
                    .insert((address.namespace.clone(), address.key.clone()), value);
            }
        }
        for address in &request.blobs {
            let value: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT value_blob FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                    params![&address.namespace, &address.key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            if let Some(value) = value {
                state
                    .blobs
                    .insert((address.namespace.clone(), address.key.clone()), value);
            }
        }
        if request.include_events {
            let mut statement = tx
                .prepare("SELECT event_json FROM bm_event_log ORDER BY sequence ASC")
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            for row in rows {
                let raw =
                    row.map_err(|error| map_transaction_error("store_consistent_read", error))?;
                state.events.push(
                    serde_json::from_str(&raw).map_err(|error| {
                        Error::config("store_consistent_read", error.to_string())
                    })?,
                );
            }
        }
        let result = read_consistent_from_state(request, &state);
        tx.commit()
            .map_err(|error| map_transaction_error("store_consistent_read", error))?;
        Ok(result)
    }

    fn read_consistent_known_keys(
        &self,
        json_keys: &[(String, String)],
        blob_keys: &[(String, String)],
        include_events: bool,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreBoundedKnownKeyReadResult> {
        let requested_entries = json_keys.len().saturating_add(blob_keys.len());
        if requested_entries > capacity.kv_max_entries {
            return Err(Error::config(
                "store_consistent_read_budget_exceeded",
                format!(
                    "requested entries {requested_entries} exceed {}",
                    capacity.kv_max_entries
                ),
            ));
        }
        let mut addresses = BTreeSet::new();
        for (kind, keys) in [("json", json_keys), ("blob", blob_keys)] {
            for (namespace, key) in keys {
                enforce_logical_key_budget(capacity, namespace, key, "store_consistent_read")?;
                if !addresses.insert((kind, namespace.as_str(), key.as_str())) {
                    return Err(Error::config(
                        "store_consistent_read",
                        format!("duplicate {kind} known-key address {namespace}/{key}"),
                    ));
                }
            }
        }

        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| map_transaction_error("store_consistent_read", error))?;
        let mut json = std::collections::BTreeMap::new();
        let mut json_bytes = 0_usize;
        for (namespace, key) in json_keys {
            let bytes: Option<usize> = tx
                .query_row(
                    "SELECT length(value_json) FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            if let Some(bytes) = bytes {
                json_bytes = json_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("consistent known-key JSON byte count overflow")
                })?;
                if json_bytes > capacity.snapshot_max_bytes {
                    return Err(Error::config(
                        "store_consistent_read_budget_exceeded",
                        format!(
                            "JSON bytes {json_bytes} exceed {}",
                            capacity.snapshot_max_bytes
                        ),
                    ));
                }
                let raw: String = tx
                    .query_row(
                        "SELECT value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                        params![namespace, key],
                        |row| row.get(0),
                    )
                    .map_err(|error| map_transaction_error("store_consistent_read", error))?;
                let value = serde_json::from_str(&raw)
                    .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
                json.insert((namespace.clone(), key.clone()), value);
            }
        }
        let mut blobs = std::collections::BTreeMap::new();
        let mut blob_bytes = 0_usize;
        for (namespace, key) in blob_keys {
            let bytes: Option<usize> = tx
                .query_row(
                    "SELECT length(value_blob) FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                    params![namespace, key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            if let Some(bytes) = bytes {
                blob_bytes = blob_bytes.checked_add(bytes).ok_or_else(|| {
                    store_budget_error("consistent known-key blob byte count overflow")
                })?;
                if blob_bytes > capacity.blob_max_bytes {
                    return Err(Error::config(
                        "store_consistent_read_budget_exceeded",
                        format!("blob bytes {blob_bytes} exceed {}", capacity.blob_max_bytes),
                    ));
                }
                let value: Vec<u8> = tx
                    .query_row(
                        "SELECT value_blob FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                        params![namespace, key],
                        |row| row.get(0),
                    )
                    .map_err(|error| map_transaction_error("store_consistent_read", error))?;
                blobs.insert((namespace.clone(), key.clone()), value);
            }
        }
        let events =
            if include_events {
                let (event_count, event_bytes): (usize, usize) = tx
                    .query_row(
                        "SELECT COUNT(*), COALESCE(SUM(length(event_json)), 0) FROM bm_event_log",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .map_err(|error| map_transaction_error("store_consistent_read", error))?;
                if event_count > capacity.event_log_max_items
                    || json_bytes.saturating_add(event_bytes) > capacity.snapshot_max_bytes
                {
                    return Err(Error::config(
                        "store_consistent_read_budget_exceeded",
                        "event log exceeds the consistent known-key read budget",
                    ));
                }
                let mut statement = tx
                    .prepare("SELECT event_json FROM bm_event_log ORDER BY sequence ASC")
                    .map_err(|error| map_transaction_error("store_consistent_read", error))?;
                let rows = statement
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|error| map_transaction_error("store_consistent_read", error))?;
                let mut events = Vec::with_capacity(event_count);
                for row in rows {
                    let raw =
                        row.map_err(|error| map_transaction_error("store_consistent_read", error))?;
                    events.push(serde_json::from_str(&raw).map_err(|error| {
                        Error::config("store_consistent_read", error.to_string())
                    })?);
                }
                events
            } else {
                Vec::new()
            };
        let result = read_bounded_known_keys_from_parts(
            json_keys,
            blob_keys,
            include_events,
            capacity,
            &json,
            &blobs,
            &events,
        )?;
        tx.commit()
            .map_err(|error| map_transaction_error("store_consistent_read", error))?;
        Ok(result)
    }

    fn open_immutable_read_session<'a>(
        &'a self,
        capacity: StoreCapacityBudget,
    ) -> Result<Box<dyn StoreImmutableReadSession + 'a>> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_immutable_read_session", error.to_string()))?;
        connection
            .execute_batch("BEGIN DEFERRED")
            .map_err(|error| map_transaction_error("store_immutable_read_session", error))?;
        Ok(Box::new(SqliteImmutableReadSession {
            connection,
            read: StoreReadSessionState::new(capacity),
        }))
    }

    fn read_scoped_projection(
        &self,
        request: &crate::StoreScopedProjectionRequest,
        capacity: StoreCapacityBudget,
    ) -> Result<crate::StoreScopedProjection> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        let scoped_json = read_scoped_json_exact(&tx, request, capacity)?;
        let events = if request.include_events {
            let (count, bytes): (usize, usize) = tx
                .query_row(
                    r#"SELECT COUNT(*), COALESCE(SUM(length(event_json)), 0)
                       FROM bm_event_log
                       WHERE json_extract(event_json, '$.scope.memory_space_id') = ?1
                         AND json_extract(event_json, '$.scope.subject_id') = ?2"#,
                    params![
                        &request.scope.memory_space_id,
                        &request.scope.mounted_subject_id
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
            if count > capacity.event_log_max_items
                || scoped_json.logical_bytes.saturating_add(bytes) > capacity.snapshot_max_bytes
            {
                return Err(Error::config(
                    "store_scoped_projection_budget_exceeded",
                    "scoped projection events exceed the pinned operation budget",
                ));
            }
            let mut statement = tx
                .prepare(
                    r#"SELECT event_json FROM bm_event_log
                       WHERE json_extract(event_json, '$.scope.memory_space_id') = ?1
                         AND json_extract(event_json, '$.scope.subject_id') = ?2
                       ORDER BY sequence"#,
                )
                .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
            let rows = statement
                .query_map(
                    params![
                        &request.scope.memory_space_id,
                        &request.scope.mounted_subject_id
                    ],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
            let mut events = Vec::with_capacity(count);
            for row in rows {
                let raw =
                    row.map_err(|error| map_transaction_error("store_scoped_projection", error))?;
                events.push(serde_json::from_str(&raw).map_err(|error| {
                    Error::config("store_scoped_projection", error.to_string())
                })?);
            }
            events
        } else {
            Vec::new()
        };
        let projection = crate::store_internal::transaction::read_scoped_projection_from_parts(
            request,
            capacity,
            &scoped_json.documents,
            &events,
        )?;
        tx.commit()
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        Ok(projection)
    }

    fn replace_scoped_projection(
        &self,
        request: &crate::StoreScopedProjectionReplaceRequest,
        admission: &StoreTransactionAdmission,
    ) -> Result<crate::StoreScopedProjectionReplaceReport> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        admission.validate_inside_engine_fence(self.capacity, &self.admission_authority)?;
        let projection_request = crate::StoreScopedProjectionRequest {
            scope: request.scope.clone(),
            json_namespaces: request.json_namespaces.clone(),
            include_events: false,
        };
        let scoped_json =
            read_scoped_json_exact(&tx, &projection_request, admission.operation_capacity())?;
        let deleted_addresses = scoped_json
            .documents
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        for doc in &request.json_docs {
            let address = (doc.namespace.clone(), doc.key.clone());
            let exists = tx
                .query_row(
                    "SELECT 1 FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                    params![&doc.namespace, &doc.key],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|error| map_transaction_error("store_scoped_projection", error))?
                .is_some();
            if exists && !deleted_addresses.contains(&address) {
                return Err(Error::config(
                    "store_scoped_projection",
                    format!(
                        "replacement address {}/{} is owned by another projection scope",
                        doc.namespace, doc.key
                    ),
                ));
            }
        }
        for (namespace, key) in &deleted_addresses {
            tx.execute(
                "DELETE FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        }
        let deleted_json = deleted_addresses.len();
        for doc in &request.json_docs {
            let raw = serde_json::to_string(&doc.value)
                .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?;
            tx.execute(
                "INSERT INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, ?3)",
                params![&doc.namespace, &doc.key, raw],
            )
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        }
        let deleted_events = tx
            .execute(
                r#"DELETE FROM bm_event_log
                   WHERE json_extract(event_json, '$.scope.memory_space_id') = ?1
                     AND json_extract(event_json, '$.scope.subject_id') = ?2"#,
                params![
                    &request.scope.memory_space_id,
                    &request.scope.mounted_subject_id
                ],
            )
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        for event in &request.events {
            let raw = serialize_event(event)?;
            tx.execute(
                "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                params![&event.event_id, raw],
            )
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        }
        let (entries, blob_bytes, event_count): (usize, usize, usize) = tx
            .query_row(
                "SELECT (SELECT COUNT(*) FROM bm_kv) + (SELECT COUNT(*) FROM bm_blob), COALESCE((SELECT SUM(length(value_blob)) FROM bm_blob), 0), (SELECT COUNT(*) FROM bm_event_log)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        validate_scoped_projection_post_image(
            admission,
            request,
            entries,
            std::iter::once(blob_bytes),
            std::iter::empty(),
            event_count,
        )?;
        tx.commit()
            .map_err(|error| map_transaction_error("store_scoped_projection", error))?;
        Ok(crate::StoreScopedProjectionReplaceReport {
            admission_report_id: admission.report_id().to_string(),
            deleted_json,
            inserted_json: request.json_docs.len(),
            deleted_events,
            inserted_events: request.events.len(),
        })
    }

    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
        enforce_logical_key_budget(self.capacity, namespace, key, "sqlite_store_json_read")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_json_read", error.to_string()))?;
        let raw: Option<String> = connection
            .query_row(
                "SELECT value_json FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("sqlite_store_json_read", error))?;
        raw.map(|value| {
            serde_json::from_str(&value)
                .map_err(|error| Error::config("sqlite_store_json_read", error.to_string()))
        })
        .transpose()
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()> {
        let raw = serde_json::to_string(&value)
            .map_err(|error| Error::config("sqlite_store_json_write", error.to_string()))?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_json_write", error.to_string()))?;
        Self::ensure_json_entry_budget(self.capacity, &connection, namespace, key)?;
        connection
            .execute(
                "INSERT OR REPLACE INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, ?3)",
                params![namespace, key, raw],
            )
            .map_err(|error| Error::storage("sqlite_store_json_write", error))?;
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool> {
        enforce_logical_key_budget(self.capacity, namespace, key, "sqlite_store_json_delete")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_json_delete", error.to_string()))?;
        let rows = connection
            .execute(
                "DELETE FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )
            .map_err(|error| Error::storage("sqlite_store_json_delete", error))?;
        Ok(rows > 0)
    }

    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>> {
        enforce_logical_key_budget(self.capacity, namespace, "", "sqlite_store_json_list")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_json_list", error.to_string()))?;
        let mut statement = connection
            .prepare("SELECT key FROM bm_kv WHERE namespace = ?1 ORDER BY key ASC")
            .map_err(|error| Error::storage("sqlite_store_json_list", error))?;
        let rows = statement
            .query_map(params![namespace], |row| row.get::<_, String>(0))
            .map_err(|error| Error::storage("sqlite_store_json_list", error))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|error| Error::storage("sqlite_store_json_list", error))?);
        }
        Ok(out)
    }

    fn get_blob(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        enforce_logical_key_budget(self.capacity, namespace, key, "sqlite_store_blob_read")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_blob_read", error.to_string()))?;
        connection
            .query_row(
                "SELECT value_blob FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| Error::storage("sqlite_store_blob_read", error))
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_blob_write", error.to_string()))?;
        Self::ensure_blob_budget(self.capacity, &connection, namespace, key, value.len())?;
        connection
            .execute(
                "INSERT OR REPLACE INTO bm_blob(namespace, key, value_blob) VALUES (?1, ?2, ?3)",
                params![namespace, key, value],
            )
            .map_err(|error| Error::storage("sqlite_store_blob_write", error))?;
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool> {
        enforce_logical_key_budget(self.capacity, namespace, key, "sqlite_store_blob_delete")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_blob_delete", error.to_string()))?;
        let rows = connection
            .execute(
                "DELETE FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )
            .map_err(|error| Error::storage("sqlite_store_blob_delete", error))?;
        Ok(rows > 0)
    }

    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>> {
        enforce_logical_key_budget(self.capacity, namespace, "", "sqlite_store_blob_list")?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_blob_list", error.to_string()))?;
        let mut statement = connection
            .prepare("SELECT key FROM bm_blob WHERE namespace = ?1 ORDER BY key ASC")
            .map_err(|error| Error::storage("sqlite_store_blob_list", error))?;
        let rows = statement
            .query_map(params![namespace], |row| row.get::<_, String>(0))
            .map_err(|error| Error::storage("sqlite_store_blob_list", error))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|error| Error::storage("sqlite_store_blob_list", error))?);
        }
        Ok(out)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn replace_snapshot(
        &self,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
        json_docs: &[StoreSnapshotJsonDoc],
        blobs: &[StoreSnapshotBlob],
        events: &[MemoryStoreEvent],
    ) -> Result<StoreSnapshotReplaceReport> {
        if events.len() > self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                events.len(),
                self.capacity.event_log_max_items
            )));
        }
        let mut event_ids = std::collections::BTreeSet::new();
        let mut encoded_events = Vec::with_capacity(events.len());
        for event in events {
            enforce_event_key_budget(self.capacity, event, "sqlite_store_snapshot_import")?;
            if !event_ids.insert(event.event_id.clone()) {
                return Err(Error::config(
                    "store_event_log",
                    format!("duplicate event id {}", event.event_id),
                ));
            }
            encoded_events.push((event.event_id.clone(), serialize_event(event)?));
        }
        let encoded_docs = json_docs
            .iter()
            .map(|doc| {
                enforce_logical_key_budget(
                    self.capacity,
                    &doc.namespace,
                    &doc.key,
                    "sqlite_store_snapshot_import",
                )?;
                serde_json::to_string(&doc.value)
                    .map(|raw| (doc.namespace.clone(), doc.key.clone(), raw))
                    .map_err(|error| {
                        Error::config("sqlite_store_snapshot_import", error.to_string())
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        let json_snapshot_keys = json_docs
            .iter()
            .map(|doc| (doc.namespace.clone(), doc.key.clone()))
            .collect::<std::collections::BTreeSet<_>>();
        let blob_snapshot_keys = blobs
            .iter()
            .map(|blob| {
                enforce_logical_key_budget(
                    self.capacity,
                    &blob.namespace,
                    &blob.key,
                    "sqlite_store_snapshot_import",
                )?;
                Ok((blob.namespace.clone(), blob.key.clone()))
            })
            .collect::<Result<std::collections::BTreeSet<_>>>()?;

        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_snapshot_import", error.to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        let retained_json_entries = count_retained_rows(
            &tx,
            "bm_kv",
            "sqlite_store_snapshot_import",
            json_namespaces,
        )?;
        let final_json_entries = retained_json_entries.saturating_add(json_docs.len());
        if final_json_entries > self.capacity.kv_max_entries {
            return Err(store_budget_error(format!(
                "kv entries {} exceed {}",
                final_json_entries, self.capacity.kv_max_entries
            )));
        }
        let retained_blob_lengths =
            retained_blob_lengths(&tx, "sqlite_store_snapshot_import", blob_namespaces)?;
        validate_restore_post_image_blob_bytes(
            self.capacity,
            retained_blob_lengths,
            blobs.iter().map(|blob| blob.value.len()),
        )?;

        let mut json_deleted = 0usize;
        for namespace in json_namespaces {
            let mut statement = tx
                .prepare("SELECT key FROM bm_kv WHERE namespace = ?1 ORDER BY key ASC")
                .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
            let rows = statement
                .query_map(params![namespace], |row| row.get::<_, String>(0))
                .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
            for row in rows {
                let key =
                    row.map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
                if !json_snapshot_keys.contains(&((*namespace).to_string(), key)) {
                    json_deleted = json_deleted.saturating_add(1);
                }
            }
            drop(statement);
            tx.execute("DELETE FROM bm_kv WHERE namespace = ?1", params![namespace])
                .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        }
        for (namespace, key, raw) in encoded_docs {
            tx.execute(
                "INSERT OR REPLACE INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, ?3)",
                params![namespace, key, raw],
            )
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        }

        let mut blobs_deleted = 0usize;
        for namespace in blob_namespaces {
            let mut statement = tx
                .prepare("SELECT key FROM bm_blob WHERE namespace = ?1 ORDER BY key ASC")
                .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
            let rows = statement
                .query_map(params![namespace], |row| row.get::<_, String>(0))
                .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
            for row in rows {
                let key =
                    row.map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
                if !blob_snapshot_keys.contains(&((*namespace).to_string(), key)) {
                    blobs_deleted = blobs_deleted.saturating_add(1);
                }
            }
            drop(statement);
            tx.execute(
                "DELETE FROM bm_blob WHERE namespace = ?1",
                params![namespace],
            )
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        }
        for blob in blobs {
            tx.execute(
                "INSERT OR REPLACE INTO bm_blob(namespace, key, value_blob) VALUES (?1, ?2, ?3)",
                params![&blob.namespace, &blob.key, &blob.value],
            )
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        }

        tx.execute("DELETE FROM bm_event_log", [])
            .map_err(|error| Error::storage("store_event_log", error))?;
        for (event_id, raw) in encoded_events {
            tx.execute(
                "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                params![event_id, raw],
            )
            .map_err(map_event_insert_error)?;
        }
        tx.commit()
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;
        Ok(StoreSnapshotReplaceReport {
            json_deleted,
            blobs_deleted,
            events_imported: events.len(),
        })
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(feature = "nonproduction-replay-harness")]
fn replay_harness_pause_after_begin_immediate() -> Result<()> {
    const FAILPOINT: &str = "after_begin_immediate_before_load_transaction_state";
    const HANDSHAKE: u8 = b'P';
    const RELEASE: u8 = b'C';

    if std::env::var_os("BM_SQLITE_TRANSACTION_FAILPOINT").as_deref()
        != Some(std::ffi::OsStr::new(FAILPOINT))
    {
        return Ok(());
    }

    let address = std::env::var("BM_SQLITE_TRANSACTION_FAILPOINT_ADDR").map_err(|error| {
        Error::config(
            "memory_write_transaction",
            format!("sqlite replay failpoint address is required: {error}"),
        )
    })?;
    let mut stream = TcpStream::connect(&address).map_err(|error| {
        Error::config(
            "memory_write_transaction",
            format!("connect sqlite replay failpoint {address}: {error}"),
        )
    })?;
    stream.write_all(&[HANDSHAKE]).map_err(|error| {
        Error::config(
            "memory_write_transaction",
            format!("announce sqlite replay failpoint: {error}"),
        )
    })?;
    stream.flush().map_err(|error| {
        Error::config(
            "memory_write_transaction",
            format!("flush sqlite replay failpoint: {error}"),
        )
    })?;
    let mut release = [0];
    stream.read_exact(&mut release).map_err(|error| {
        Error::config(
            "memory_write_transaction",
            format!("wait for sqlite replay failpoint release: {error}"),
        )
    })?;
    if release != [RELEASE] {
        return Err(Error::config(
            "memory_write_transaction",
            "unexpected sqlite replay failpoint release",
        ));
    }
    Ok(())
}

fn map_transaction_error(stage: &'static str, error: rusqlite::Error) -> Error {
    match &error {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            Error::config("store_transaction_busy", error.to_string())
        }
        _ => Error::storage(stage, error),
    }
}

fn serialize_event(event: &MemoryStoreEvent) -> Result<String> {
    serde_json::to_string(event)
        .map_err(|error| Error::config("store_event_log", error.to_string()))
}

#[cfg(feature = "nonproduction-replay-harness")]
fn count_retained_rows(
    connection: &Connection,
    table: &'static str,
    stage: &'static str,
    replaced_namespaces: &[&str],
) -> Result<usize> {
    let replaced = replaced_namespaces
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut statement = connection
        .prepare(&format!("SELECT namespace FROM {table}"))
        .map_err(|error| Error::storage(stage, error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| Error::storage(stage, error))?;
    let mut count = 0usize;
    for row in rows {
        let namespace = row.map_err(|error| Error::storage(stage, error))?;
        if !replaced.contains(namespace.as_str()) {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

#[cfg(feature = "nonproduction-replay-harness")]
fn retained_blob_lengths(
    connection: &Connection,
    stage: &'static str,
    replaced_namespaces: &[&str],
) -> Result<Vec<usize>> {
    let replaced = replaced_namespaces
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut statement = connection
        .prepare("SELECT namespace, length(value_blob) FROM bm_blob")
        .map_err(|error| Error::storage(stage, error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
        })
        .map_err(|error| Error::storage(stage, error))?;
    let mut lengths = Vec::new();
    for row in rows {
        let (namespace, len) = row.map_err(|error| Error::storage(stage, error))?;
        if !replaced.contains(namespace.as_str()) {
            lengths.push(len);
        }
    }
    Ok(lengths)
}

#[cfg(feature = "nonproduction-replay-harness")]
fn map_event_insert_error(error: rusqlite::Error) -> Error {
    if error.to_string().contains("UNIQUE") {
        Error::config("store_event_log", "duplicate event id")
    } else {
        Error::storage("store_event_log", error)
    }
}
