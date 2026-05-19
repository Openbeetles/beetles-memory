use std::{fs, path::Path};

use bm_core::{MemoryDomain, MemoryPlane, MemoryRecord, MemoryRecordMeta, NewMemoryRecord};
use rusqlite::{params, Connection};

use crate::{
    StoreError, StoreErrorKind, StoreHealthReport, StoreOperation, StoreResult,
    StoreSnapshotReport, STORE_SCHEMA_VERSION,
};

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                StoreError::new(
                    StoreErrorKind::Io,
                    StoreOperation::OpenBackend,
                    err.to_string(),
                )
                .path(parent)
            })?;
        }

        let conn =
            Connection::open(path).map_err(|err| sqlite_error(StoreOperation::OpenBackend, err))?;
        let store = Self { conn };
        store.initialize_schema()?;
        store.validate_schema()?;
        Ok(store)
    }

    fn initialize_schema(&self) -> StoreResult<()> {
        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS memory_records (
                    id TEXT PRIMARY KEY,
                    identity TEXT NOT NULL,
                    scope TEXT NOT NULL,
                    content TEXT NOT NULL,
                    source TEXT NOT NULL,
                    domain TEXT NOT NULL,
                    plane TEXT NOT NULL,
                    metadata_json TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS store_events (
                    seq INTEGER PRIMARY KEY,
                    event_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    record_id TEXT NOT NULL,
                    payload_json TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS store_meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                INSERT OR IGNORE INTO store_meta (key, value) VALUES ('schema_version', '2');
                INSERT OR IGNORE INTO store_meta (key, value) VALUES ('next_id', '1');
                INSERT OR IGNORE INTO store_meta (key, value) VALUES ('last_event_seq', '0');
                INSERT OR IGNORE INTO store_meta (key, value) VALUES ('snapshot_event_seq', '0');
                ",
            )
            .map_err(|err| sqlite_error(StoreOperation::OpenBackend, err))?;
        self.migrate_schema_v2()
    }

    fn validate_schema(&self) -> StoreResult<()> {
        let version = self.meta_u64("schema_version", StoreOperation::LoadManifest)? as u32;
        if version == STORE_SCHEMA_VERSION {
            return Ok(());
        }
        Err(StoreError::new(
            StoreErrorKind::UnsupportedSchemaVersion,
            StoreOperation::LoadManifest,
            format!("schema version {version} is not supported"),
        )
        .recoverable(false))
    }

    fn meta_u64(&self, key: &str, operation: StoreOperation) -> StoreResult<u64> {
        let value: String = self
            .conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .map_err(|err| sqlite_error(operation, err))?;
        value.parse::<u64>().map_err(|err| {
            StoreError::new(
                StoreErrorKind::Json,
                operation,
                format!("meta key {key} is not a u64: {err}"),
            )
            .recoverable(false)
        })
    }

    fn migrate_schema_v2(&self) -> StoreResult<()> {
        if !self.has_column("memory_records", "metadata_json")? {
            self.conn
                .execute(
                    "ALTER TABLE memory_records ADD COLUMN metadata_json TEXT NOT NULL DEFAULT '{}'",
                    [],
                )
                .map_err(|err| sqlite_error(StoreOperation::WriteManifest, err))?;
            let mut stmt = self
                .conn
                .prepare("SELECT id, plane FROM memory_records")
                .map_err(|err| sqlite_error(StoreOperation::ReadRecords, err))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|err| sqlite_error(StoreOperation::ReadRecords, err))?;
            for row in rows {
                let (id, plane) =
                    row.map_err(|err| sqlite_error(StoreOperation::ReadRecords, err))?;
                let plane = parse_plane(&plane)?;
                let metadata_json = serde_json::to_string(&MemoryRecordMeta::default_for_plane(
                    plane,
                ))
                .map_err(|err| {
                    StoreError::new(
                        StoreErrorKind::Json,
                        StoreOperation::WriteManifest,
                        err.to_string(),
                    )
                })?;
                self.conn
                    .execute(
                        "UPDATE memory_records SET metadata_json = ?1 WHERE id = ?2",
                        params![metadata_json, id],
                    )
                    .map_err(|err| sqlite_error(StoreOperation::WriteManifest, err))?;
            }
        }
        self.conn
            .execute(
                "UPDATE store_meta SET value = ?1 WHERE key = 'schema_version'",
                params![STORE_SCHEMA_VERSION.to_string()],
            )
            .map_err(|err| sqlite_error(StoreOperation::WriteManifest, err))?;
        Ok(())
    }

    fn has_column(&self, table: &str, column: &str) -> StoreResult<bool> {
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(|err| sqlite_error(StoreOperation::LoadManifest, err))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|err| sqlite_error(StoreOperation::LoadManifest, err))?;
        for row in rows {
            if row.map_err(|err| sqlite_error(StoreOperation::LoadManifest, err))? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn record_count(&self) -> StoreResult<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM memory_records", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| count as usize)
            .map_err(|err| sqlite_error(StoreOperation::ReadRecords, err))
    }
}

impl crate::MemoryStore for SqliteStore {
    fn insert(&mut self, record: NewMemoryRecord) -> StoreResult<MemoryRecord> {
        let next_id = self.meta_u64("next_id", StoreOperation::InsertRecord)?;
        let seq = self.meta_u64("last_event_seq", StoreOperation::InsertRecord)? + 1;
        let stored = MemoryRecord {
            id: format!("mem-{next_id}"),
            identity: record.identity,
            scope: record.scope,
            content: record.content,
            source: record.source,
            domain: record.domain,
            plane: record.plane,
            meta: record.meta,
        };
        let payload_json = serde_json::to_string(&stored).map_err(|err| {
            StoreError::new(
                StoreErrorKind::Json,
                StoreOperation::AppendEvent,
                err.to_string(),
            )
        })?;

        let tx = self
            .conn
            .transaction()
            .map_err(|err| sqlite_error(StoreOperation::InsertRecord, err))?;
        tx.execute(
            "INSERT INTO memory_records (id, identity, scope, content, source, domain, plane, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &stored.id,
                &stored.identity,
                &stored.scope,
                &stored.content,
                &stored.source,
                stored.domain.as_str(),
                stored.plane.as_str(),
                serde_json::to_string(&stored.meta).map_err(|err| {
                    StoreError::new(
                        StoreErrorKind::Json,
                        StoreOperation::InsertRecord,
                        err.to_string(),
                    )
                })?,
            ],
        )
        .map_err(|err| sqlite_error(StoreOperation::InsertRecord, err))?;
        tx.execute(
            "INSERT INTO store_events (seq, event_id, kind, record_id, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                seq as i64,
                format!("evt-{seq}"),
                "record_inserted",
                &stored.id,
                &payload_json,
            ],
        )
        .map_err(|err| sqlite_error(StoreOperation::AppendEvent, err))?;
        tx.execute(
            "UPDATE store_meta SET value = ?1 WHERE key = 'next_id'",
            params![(next_id + 1).to_string()],
        )
        .map_err(|err| sqlite_error(StoreOperation::WriteManifest, err))?;
        tx.execute(
            "UPDATE store_meta SET value = ?1 WHERE key = 'last_event_seq'",
            params![seq.to_string()],
        )
        .map_err(|err| sqlite_error(StoreOperation::WriteManifest, err))?;
        tx.commit()
            .map_err(|err| sqlite_error(StoreOperation::InsertRecord, err))?;
        Ok(stored)
    }

    fn replace(&mut self, record: MemoryRecord) -> StoreResult<MemoryRecord> {
        let seq = self.meta_u64("last_event_seq", StoreOperation::ReplaceRecord)? + 1;
        let payload_json = serde_json::to_string(&record).map_err(|err| {
            StoreError::new(
                StoreErrorKind::Json,
                StoreOperation::AppendEvent,
                err.to_string(),
            )
        })?;
        let metadata_json = serde_json::to_string(&record.meta).map_err(|err| {
            StoreError::new(
                StoreErrorKind::Json,
                StoreOperation::ReplaceRecord,
                err.to_string(),
            )
        })?;
        let tx = self
            .conn
            .transaction()
            .map_err(|err| sqlite_error(StoreOperation::ReplaceRecord, err))?;
        tx.execute(
            "INSERT OR REPLACE INTO memory_records (id, identity, scope, content, source, domain, plane, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &record.id,
                &record.identity,
                &record.scope,
                &record.content,
                &record.source,
                record.domain.as_str(),
                record.plane.as_str(),
                metadata_json,
            ],
        )
        .map_err(|err| sqlite_error(StoreOperation::ReplaceRecord, err))?;
        tx.execute(
            "INSERT INTO store_events (seq, event_id, kind, record_id, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                seq as i64,
                format!("evt-{seq}"),
                "record_replaced",
                &record.id,
                &payload_json,
            ],
        )
        .map_err(|err| sqlite_error(StoreOperation::AppendEvent, err))?;
        tx.execute(
            "UPDATE store_meta SET value = ?1 WHERE key = 'last_event_seq'",
            params![seq.to_string()],
        )
        .map_err(|err| sqlite_error(StoreOperation::WriteManifest, err))?;
        tx.commit()
            .map_err(|err| sqlite_error(StoreOperation::ReplaceRecord, err))?;
        Ok(record)
    }

    fn delete(&mut self, record_id: &str) -> StoreResult<bool> {
        let seq = self.meta_u64("last_event_seq", StoreOperation::DeleteRecord)? + 1;
        let tx = self
            .conn
            .transaction()
            .map_err(|err| sqlite_error(StoreOperation::DeleteRecord, err))?;
        let deleted = tx
            .execute(
                "DELETE FROM memory_records WHERE id = ?1",
                params![record_id],
            )
            .map_err(|err| sqlite_error(StoreOperation::DeleteRecord, err))?
            > 0;
        if deleted {
            tx.execute(
                "INSERT INTO store_events (seq, event_id, kind, record_id, payload_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    seq as i64,
                    format!("evt-{seq}"),
                    "record_deleted",
                    record_id,
                    "{}",
                ],
            )
            .map_err(|err| sqlite_error(StoreOperation::AppendEvent, err))?;
            tx.execute(
                "UPDATE store_meta SET value = ?1 WHERE key = 'last_event_seq'",
                params![seq.to_string()],
            )
            .map_err(|err| sqlite_error(StoreOperation::WriteManifest, err))?;
        }
        tx.commit()
            .map_err(|err| sqlite_error(StoreOperation::DeleteRecord, err))?;
        Ok(deleted)
    }

    fn records(&self) -> StoreResult<Vec<MemoryRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, identity, scope, content, source, domain, plane, metadata_json
                 FROM memory_records
                 ORDER BY id",
            )
            .map_err(|err| sqlite_error(StoreOperation::ReadRecords, err))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(StoredRecordRow {
                    id: row.get(0)?,
                    identity: row.get(1)?,
                    scope: row.get(2)?,
                    content: row.get(3)?,
                    source: row.get(4)?,
                    domain: row.get(5)?,
                    plane: row.get(6)?,
                    metadata_json: row.get(7)?,
                })
            })
            .map_err(|err| sqlite_error(StoreOperation::ReadRecords, err))?;

        let mut records = Vec::new();
        for row in rows {
            let row = row.map_err(|err| sqlite_error(StoreOperation::ReadRecords, err))?;
            let meta = serde_json::from_str(&row.metadata_json).map_err(|err| {
                StoreError::new(
                    StoreErrorKind::Json,
                    StoreOperation::ReadRecords,
                    format!("invalid metadata_json for {}: {err}", row.id),
                )
                .recoverable(false)
            })?;
            records.push(MemoryRecord {
                id: row.id,
                identity: row.identity,
                scope: row.scope,
                content: row.content,
                source: row.source,
                domain: parse_domain(&row.domain)?,
                plane: parse_plane(&row.plane)?,
                meta,
            });
        }
        Ok(records)
    }

    fn snapshot(&mut self) -> StoreResult<StoreSnapshotReport> {
        let last_event_seq = self.meta_u64("last_event_seq", StoreOperation::WriteSnapshot)?;
        self.conn
            .execute(
                "UPDATE store_meta SET value = ?1 WHERE key = 'snapshot_event_seq'",
                params![last_event_seq.to_string()],
            )
            .map_err(|err| sqlite_error(StoreOperation::WriteSnapshot, err))?;

        Ok(StoreSnapshotReport {
            schema_version: STORE_SCHEMA_VERSION,
            snapshot_event_seq: last_event_seq,
            record_count: self.record_count()?,
        })
    }

    fn health(&self) -> StoreHealthReport {
        let record_count = self.record_count().unwrap_or_default();
        let last_event_seq = self
            .meta_u64("last_event_seq", StoreOperation::LoadManifest)
            .unwrap_or_default();
        let snapshot_event_seq = self
            .meta_u64("snapshot_event_seq", StoreOperation::LoadManifest)
            .unwrap_or_default();

        StoreHealthReport {
            backend: "sqlite",
            healthy: true,
            record_count,
            last_event_seq,
            snapshot_event_seq,
        }
    }
}

struct StoredRecordRow {
    id: String,
    identity: String,
    scope: String,
    content: String,
    source: String,
    domain: String,
    plane: String,
    metadata_json: String,
}

fn parse_domain(value: &str) -> StoreResult<MemoryDomain> {
    match value {
        "Program" => Ok(MemoryDomain::Program),
        "Subject" => Ok(MemoryDomain::Subject),
        "Soul" => Ok(MemoryDomain::Soul),
        _ => Err(StoreError::new(
            StoreErrorKind::Json,
            StoreOperation::ReadRecords,
            format!("unknown memory domain {value}"),
        )
        .recoverable(false)),
    }
}

fn parse_plane(value: &str) -> StoreResult<MemoryPlane> {
    match value {
        "SharedFactual" => Ok(MemoryPlane::SharedFactual),
        "Procedural" => Ok(MemoryPlane::Procedural),
        "ContinuityCapsule" => Ok(MemoryPlane::ContinuityCapsule),
        "ArchiveEvidence" => Ok(MemoryPlane::ArchiveEvidence),
        "TaskRecall" => Ok(MemoryPlane::TaskRecall),
        "SubjectProjection" => Ok(MemoryPlane::SubjectProjection),
        "SoulGovernance" => Ok(MemoryPlane::SoulGovernance),
        _ => Err(StoreError::new(
            StoreErrorKind::Json,
            StoreOperation::ReadRecords,
            format!("unknown memory plane {value}"),
        )
        .recoverable(false)),
    }
}

fn sqlite_error(operation: StoreOperation, err: rusqlite::Error) -> StoreError {
    StoreError::new(
        StoreErrorKind::BackendUnavailable,
        operation,
        err.to_string(),
    )
}
