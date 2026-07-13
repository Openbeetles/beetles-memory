use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "nonproduction-replay-harness")]
use std::io::{Read, Write};
#[cfg(feature = "nonproduction-replay-harness")]
use std::net::TcpStream;

use bm_core::{Error, Result};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension, TransactionBehavior};
use serde_json::Value;

#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_internal::transaction::read_consistent_from_state;
use crate::{
    enforce_event_key_budget, enforce_logical_key_budget, store_budget_error,
    store_internal::transaction::{
        apply_transaction, read_consistent_namespaces_from_state, BackendTransactionState,
        EventOverflowPolicy,
    },
    MemoryStoreEvent, StoreBackendConfig, StoreCapacityBudget, StoreConsistentNamespaceReadRequest,
    StoreConsistentNamespaceReadResult, StoreEngine, StoreEngineMutation, StoreEventLog,
    StoreSchemaManifest, StoreSnapshotBlob, StoreSnapshotJsonDoc, StoreSnapshotReplaceReport,
    StoreTransactionReport, StoreTransactionRequest, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreConsistentReadRequest, StoreConsistentReadResult};

pub struct SqliteStoreEngine {
    capacity: StoreCapacityBudget,
    connection: Mutex<Connection>,
}

impl SqliteStoreEngine {
    pub fn open(config: &StoreBackendConfig) -> Result<(Self, StoreSchemaManifest)> {
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
            capacity: config.capacity,
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
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_open", error.to_string()))?;
        connection
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
        let incompatible_schema: Option<String> = connection
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
        let existing: Option<String> = connection
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
            None => StoreSchemaManifest::new(config.backend, config.profile, now_secs),
        };
        let raw = serde_json::to_string(&manifest)
            .map_err(|error| Error::config("sqlite_store_schema", error.to_string()))?;
        connection
            .execute(
                "INSERT OR REPLACE INTO bm_schema(schema_id, schema_version, manifest_json) VALUES (?1, ?2, ?3)",
                params![STORE_SCHEMA_ID, STORE_SCHEMA_VERSION, raw],
            )
            .map_err(|error| Error::storage("sqlite_store_schema", error))?;
        Ok(manifest)
    }

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

    fn load_transaction_state(connection: &Connection) -> Result<BackendTransactionState> {
        let mut state = BackendTransactionState::default();
        {
            let mut statement = connection
                .prepare("SELECT namespace, key, value_json FROM bm_kv ORDER BY namespace, key")
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            for row in rows {
                let (namespace, key, raw) =
                    row.map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                let value = serde_json::from_str(&raw).map_err(|error| {
                    Error::config("memory_write_transaction", error.to_string())
                })?;
                state.json.insert((namespace, key), value);
            }
        }
        {
            let mut statement = connection
                .prepare("SELECT namespace, key, value_blob FROM bm_blob ORDER BY namespace, key")
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                })
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            for row in rows {
                let (namespace, key, value) =
                    row.map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                state.blobs.insert((namespace, key), value);
            }
        }
        {
            let mut statement = connection
                .prepare("SELECT event_json FROM bm_event_log ORDER BY sequence ASC")
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
            for row in rows {
                let raw =
                    row.map_err(|error| map_transaction_error("memory_write_transaction", error))?;
                state
                    .events
                    .push(serde_json::from_str(&raw).map_err(|error| {
                        Error::config("memory_write_transaction", error.to_string())
                    })?);
            }
        }
        Ok(state)
    }
}

impl StoreEventLog for SqliteStoreEngine {
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
    fn commit_transaction(
        &self,
        request: &StoreTransactionRequest,
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
        let current = Self::load_transaction_state(&tx)?;
        let (_next, report) = apply_transaction(
            self.capacity,
            request,
            &current,
            EventOverflowPolicy::Reject,
        )?;

        for mutation in &request.mutations {
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
            }
        }
        tx.commit()
            .map_err(|error| map_transaction_error("memory_write_transaction", error))?;
        Ok(report)
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

    fn read_consistent_namespaces(
        &self,
        request: &StoreConsistentNamespaceReadRequest,
    ) -> Result<StoreConsistentNamespaceReadResult> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| map_transaction_error("store_consistent_read", error))?;
        let mut state = BackendTransactionState::default();
        for namespace in &request.json_namespaces {
            let mut statement = tx
                .prepare("SELECT key, value_json FROM bm_kv WHERE namespace = ?1 ORDER BY key")
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            let rows = statement
                .query_map(params![namespace], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            for row in rows {
                let (key, raw) =
                    row.map_err(|error| map_transaction_error("store_consistent_read", error))?;
                let value = serde_json::from_str(&raw)
                    .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
                state.json.insert((namespace.clone(), key), value);
            }
        }
        for namespace in &request.blob_namespaces {
            let mut statement = tx
                .prepare("SELECT key, value_blob FROM bm_blob WHERE namespace = ?1 ORDER BY key")
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            let rows = statement
                .query_map(params![namespace], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .map_err(|error| map_transaction_error("store_consistent_read", error))?;
            for row in rows {
                let (key, value) =
                    row.map_err(|error| map_transaction_error("store_consistent_read", error))?;
                state.blobs.insert((namespace.clone(), key), value);
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
        let result = read_consistent_namespaces_from_state(request, &state)?;
        tx.commit()
            .map_err(|error| map_transaction_error("store_consistent_read", error))?;
        Ok(result)
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

    fn put_json_value_and_event(
        &self,
        namespace: &str,
        key: &str,
        value: Value,
        event: MemoryStoreEvent,
    ) -> Result<()> {
        let raw = serde_json::to_string(&value)
            .map_err(|error| Error::config("sqlite_store_json_write", error.to_string()))?;
        let event_raw = serialize_event(&event)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_json_write", error.to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|error| Error::storage("sqlite_store_json_write", error))?;
        Self::ensure_can_insert_event(self.capacity, &tx, &event)?;
        Self::ensure_json_entry_budget(self.capacity, &tx, namespace, key)?;
        tx.execute(
            "INSERT OR REPLACE INTO bm_kv(namespace, key, value_json) VALUES (?1, ?2, ?3)",
            params![namespace, key, raw],
        )
        .map_err(|error| Error::storage("sqlite_store_json_write", error))?;
        tx.execute(
            "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
            params![event.event_id, event_raw],
        )
        .map_err(map_event_insert_error)?;
        tx.commit()
            .map_err(|error| Error::storage("sqlite_store_json_write", error))?;
        Ok(())
    }

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

    fn delete_json_value_and_event(
        &self,
        namespace: &str,
        key: &str,
        event: MemoryStoreEvent,
    ) -> Result<bool> {
        let event_raw = serialize_event(&event)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_json_delete", error.to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|error| Error::storage("sqlite_store_json_delete", error))?;
        enforce_logical_key_budget(self.capacity, namespace, key, "sqlite_store_json_delete")?;
        let rows = tx
            .execute(
                "DELETE FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )
            .map_err(|error| Error::storage("sqlite_store_json_delete", error))?;
        if rows > 0 {
            Self::ensure_can_insert_event(self.capacity, &tx, &event)?;
            tx.execute(
                "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                params![event.event_id, event_raw],
            )
            .map_err(map_event_insert_error)?;
        }
        tx.commit()
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

    fn put_blob_and_event(
        &self,
        namespace: &str,
        key: &str,
        value: &[u8],
        event: MemoryStoreEvent,
    ) -> Result<()> {
        let event_raw = serialize_event(&event)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_blob_write", error.to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|error| Error::storage("sqlite_store_blob_write", error))?;
        Self::ensure_can_insert_event(self.capacity, &tx, &event)?;
        Self::ensure_blob_budget(self.capacity, &tx, namespace, key, value.len())?;
        tx.execute(
            "INSERT OR REPLACE INTO bm_blob(namespace, key, value_blob) VALUES (?1, ?2, ?3)",
            params![namespace, key, value],
        )
        .map_err(|error| Error::storage("sqlite_store_blob_write", error))?;
        tx.execute(
            "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
            params![event.event_id, event_raw],
        )
        .map_err(map_event_insert_error)?;
        tx.commit()
            .map_err(|error| Error::storage("sqlite_store_blob_write", error))?;
        Ok(())
    }

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

    fn delete_blob_and_event(
        &self,
        namespace: &str,
        key: &str,
        event: MemoryStoreEvent,
    ) -> Result<bool> {
        let event_raw = serialize_event(&event)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_blob_delete", error.to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|error| Error::storage("sqlite_store_blob_delete", error))?;
        enforce_logical_key_budget(self.capacity, namespace, key, "sqlite_store_blob_delete")?;
        let rows = tx
            .execute(
                "DELETE FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )
            .map_err(|error| Error::storage("sqlite_store_blob_delete", error))?;
        if rows > 0 {
            Self::ensure_can_insert_event(self.capacity, &tx, &event)?;
            tx.execute(
                "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                params![event.event_id, event_raw],
            )
            .map_err(map_event_insert_error)?;
        }
        tx.commit()
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
        let retained_blob_bytes =
            count_retained_blob_bytes(&tx, "sqlite_store_snapshot_import", blob_namespaces)?;
        let snapshot_blob_bytes = blobs.iter().map(|blob| blob.value.len()).sum::<usize>();
        let final_blob_bytes = retained_blob_bytes.saturating_add(snapshot_blob_bytes);
        if final_blob_bytes > self.capacity.blob_max_bytes {
            return Err(store_budget_error(format!(
                "blob bytes {} exceed {}",
                final_blob_bytes, self.capacity.blob_max_bytes
            )));
        }

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

fn count_retained_blob_bytes(
    connection: &Connection,
    stage: &'static str,
    replaced_namespaces: &[&str],
) -> Result<usize> {
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
    let mut bytes = 0usize;
    for row in rows {
        let (namespace, len) = row.map_err(|error| Error::storage(stage, error))?;
        if !replaced.contains(namespace.as_str()) {
            bytes = bytes.saturating_add(len);
        }
    }
    Ok(bytes)
}

fn map_event_insert_error(error: rusqlite::Error) -> Error {
    if error.to_string().contains("UNIQUE") {
        Error::config("store_event_log", "duplicate event id")
    } else {
        Error::storage("store_event_log", error)
    }
}
