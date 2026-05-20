use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::{
    MemoryStoreEvent, StoreBackendConfig, StoreEngine, StoreEventLog, StoreSchemaManifest,
    StoreSnapshotBlob, StoreSnapshotJsonDoc, StoreSnapshotReplaceReport, STORE_SCHEMA_ID,
    STORE_SCHEMA_VERSION,
};

pub struct SqliteStoreEngine {
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
        let engine = Self {
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
}

impl StoreEventLog for SqliteStoreEngine {
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        let raw = serde_json::to_string(&event)
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        let connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
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
    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
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
        let rows = tx
            .execute(
                "DELETE FROM bm_kv WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )
            .map_err(|error| Error::storage("sqlite_store_json_delete", error))?;
        if rows > 0 {
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
        let rows = tx
            .execute(
                "DELETE FROM bm_blob WHERE namespace = ?1 AND key = ?2",
                params![namespace, key],
            )
            .map_err(|error| Error::storage("sqlite_store_blob_delete", error))?;
        if rows > 0 {
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

    fn replace_events(&self, events: &[MemoryStoreEvent]) -> Result<()> {
        let mut event_ids = std::collections::BTreeSet::new();
        let mut encoded = Vec::with_capacity(events.len());
        for event in events {
            if !event_ids.insert(event.event_id.clone()) {
                return Err(Error::config(
                    "store_event_log",
                    format!("duplicate event id {}", event.event_id),
                ));
            }
            encoded.push((event.event_id.clone(), serialize_event(event)?));
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("store_event_log", error.to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|error| Error::storage("store_event_log", error))?;
        tx.execute("DELETE FROM bm_event_log", [])
            .map_err(|error| Error::storage("store_event_log", error))?;
        for (event_id, raw) in encoded {
            tx.execute(
                "INSERT INTO bm_event_log(event_id, event_json) VALUES (?1, ?2)",
                params![event_id, raw],
            )
            .map_err(map_event_insert_error)?;
        }
        tx.commit()
            .map_err(|error| Error::storage("store_event_log", error))?;
        Ok(())
    }

    fn replace_snapshot(
        &self,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
        json_docs: &[StoreSnapshotJsonDoc],
        blobs: &[StoreSnapshotBlob],
        events: &[MemoryStoreEvent],
    ) -> Result<StoreSnapshotReplaceReport> {
        let mut event_ids = std::collections::BTreeSet::new();
        let mut encoded_events = Vec::with_capacity(events.len());
        for event in events {
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
            .map(|blob| (blob.namespace.clone(), blob.key.clone()))
            .collect::<std::collections::BTreeSet<_>>();

        let mut connection = self
            .connection
            .lock()
            .map_err(|error| Error::config("sqlite_store_snapshot_import", error.to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|error| Error::storage("sqlite_store_snapshot_import", error))?;

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

fn serialize_event(event: &MemoryStoreEvent) -> Result<String> {
    serde_json::to_string(event)
        .map_err(|error| Error::config("store_event_log", error.to_string()))
}

fn map_event_insert_error(error: rusqlite::Error) -> Error {
    if error.to_string().contains("UNIQUE") {
        Error::config("store_event_log", "duplicate event id")
    } else {
        Error::storage("store_event_log", error)
    }
}
