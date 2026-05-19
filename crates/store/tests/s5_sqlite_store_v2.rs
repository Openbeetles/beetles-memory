#![cfg(feature = "sqlite")]

use bm_core::{Confidence, MemoryPlane, MemoryRecordMeta, NewMemoryRecord};
use bm_store::{MemoryStore, SqliteStore, StoreResult};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn sqlite_store_persists_metadata_replace_and_delete() -> StoreResult<()> {
    let root = TempStoreRoot::new("sqlite_store_persists_metadata_replace_and_delete");
    let db = root.path().join("store.sqlite");
    let mut store = SqliteStore::open(&db)?;
    let mut inserted = store.insert(record("initial"))?;
    inserted.content = "replaced".to_owned();
    inserted.meta.confidence = Confidence::High;
    store.replace(inserted.clone())?;
    drop(store);

    let mut reopened = SqliteStore::open(&db)?;
    let records = reopened.records()?;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].content, "replaced");
    assert_eq!(records[0].meta.confidence, Confidence::High);
    assert!(reopened.delete(&inserted.id)?);
    assert!(reopened.records()?.is_empty());
    Ok(())
}

#[test]
fn sqlite_store_migrates_v1_records_to_metadata_json() -> StoreResult<()> {
    let root = TempStoreRoot::new("sqlite_store_migrates_v1_records_to_metadata_json");
    let db = root.path().join("store.sqlite");
    seed_v1_sqlite(&db);

    let store = SqliteStore::open(&db)?;
    let records = store.records()?;

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].meta.confidence, Confidence::Medium);
    assert!(records[0].meta.canonical);
    Ok(())
}

fn record(content: &str) -> NewMemoryRecord {
    NewMemoryRecord {
        identity: "agent:s5".to_owned(),
        scope: "task:s5".to_owned(),
        content: content.to_owned(),
        source: "sqlite-s5".to_owned(),
        domain: MemoryPlane::SharedFactual.domain(),
        plane: MemoryPlane::SharedFactual,
        meta: MemoryRecordMeta::default_for_plane(MemoryPlane::SharedFactual),
    }
}

fn seed_v1_sqlite(path: &Path) {
    let conn = Connection::open(path).expect("open seed sqlite");
    conn.execute_batch(
        "
        CREATE TABLE memory_records (
            id TEXT PRIMARY KEY,
            identity TEXT NOT NULL,
            scope TEXT NOT NULL,
            content TEXT NOT NULL,
            source TEXT NOT NULL,
            domain TEXT NOT NULL,
            plane TEXT NOT NULL
        );
        CREATE TABLE store_events (
            seq INTEGER PRIMARY KEY,
            event_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            record_id TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );
        CREATE TABLE store_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        INSERT INTO store_meta (key, value) VALUES ('schema_version', '1');
        INSERT INTO store_meta (key, value) VALUES ('next_id', '2');
        INSERT INTO store_meta (key, value) VALUES ('last_event_seq', '0');
        INSERT INTO store_meta (key, value) VALUES ('snapshot_event_seq', '0');
        INSERT INTO memory_records (id, identity, scope, content, source, domain, plane)
        VALUES ('mem-1', 'agent:s5', 'task:s5', 'legacy fact', 'seed', 'Program', 'SharedFactual');
        ",
    )
    .expect("seed v1 sqlite");
}

struct TempStoreRoot {
    path: PathBuf,
}

impl TempStoreRoot {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("bm-s5-{name}-{nanos}"));
        fs::create_dir_all(&path).expect("create temp store root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempStoreRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
