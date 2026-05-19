#![cfg(feature = "sqlite")]

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bm_core::{MemoryPlane, MemoryRecordMeta, NewMemoryRecord};
use bm_store::{MemoryStore, SqliteStore, StoreResult};
use rusqlite::Connection;

// 这些测试要求生产代码提供的 S2 接口：
// - Cargo feature `sqlite` 启用可选 SQLite backend 和依赖。
// - `bm_store::SqliteStore::open(path)` 初始化或打开本地数据库。
// - `SqliteStore` 实现与其他 store 共用的可失败 `MemoryStore` trait。
// - `insert` 写入 `memory_records`，并追加一条 `record_inserted` event。
// - `records` 重新读出 `MemoryRecord`，且不能丢失 domain 或 plane。

#[test]
fn sqlite_store_initializes_s2_schema() -> StoreResult<()> {
    let db = TempSqlitePath::new("schema-init");

    let _store = SqliteStore::open(db.path())?;

    assert_eq!(
        table_names(db.path()),
        BTreeSet::from([
            "memory_records".to_owned(),
            "store_events".to_owned(),
            "store_meta".to_owned(),
        ])
    );
    assert_eq!(
        table_columns(db.path(), "memory_records"),
        vec![
            column("id", "TEXT", false, 1),
            column("identity", "TEXT", true, 0),
            column("scope", "TEXT", true, 0),
            column("content", "TEXT", true, 0),
            column("source", "TEXT", true, 0),
            column("domain", "TEXT", true, 0),
            column("plane", "TEXT", true, 0),
            column("metadata_json", "TEXT", true, 0),
        ]
    );
    assert_eq!(
        table_columns(db.path(), "store_events"),
        vec![
            column("seq", "INTEGER", false, 1),
            column("event_id", "TEXT", true, 0),
            column("kind", "TEXT", true, 0),
            column("record_id", "TEXT", true, 0),
            column("payload_json", "TEXT", true, 0),
        ]
    );
    assert_eq!(
        table_columns(db.path(), "store_meta"),
        vec![
            column("key", "TEXT", false, 1),
            column("value", "TEXT", true, 0),
        ]
    );

    Ok(())
}

#[test]
fn sqlite_store_insert_persists_record_and_records_returns_it() -> StoreResult<()> {
    let db = TempSqlitePath::new("insert-records");

    let inserted = {
        let mut store = SqliteStore::open(db.path())?;
        store.insert(new_record(
            "sqlite insert must persist governed memory",
            MemoryPlane::SharedFactual,
        ))?
    };

    assert_eq!(inserted.id, "mem-1");

    let store = SqliteStore::open(db.path())?;
    let records = store.records()?;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, inserted.id);
    assert_eq!(records[0].identity, "agent:sqlite");
    assert_eq!(records[0].scope, "task:s2-sqlite");
    assert_eq!(
        records[0].content,
        "sqlite insert must persist governed memory"
    );
    assert_eq!(records[0].source, "sqlite-store-test");
    assert_eq!(records[0].domain, MemoryPlane::SharedFactual.domain());
    assert_eq!(records[0].plane, MemoryPlane::SharedFactual);

    assert_eq!(
        stored_record_rows(db.path()),
        vec![StoredRecordRow {
            id: "mem-1".to_owned(),
            identity: "agent:sqlite".to_owned(),
            scope: "task:s2-sqlite".to_owned(),
            content: "sqlite insert must persist governed memory".to_owned(),
            source: "sqlite-store-test".to_owned(),
            domain: "Program".to_owned(),
            plane: "SharedFactual".to_owned(),
        }]
    );

    Ok(())
}

#[test]
fn sqlite_store_appends_record_inserted_events_with_monotonic_seq() -> StoreResult<()> {
    let db = TempSqlitePath::new("event-append");
    let mut store = SqliteStore::open(db.path())?;

    let first = store.insert(new_record(
        "sqlite event append first",
        MemoryPlane::SharedFactual,
    ))?;
    let second = store.insert(new_record(
        "sqlite event append second",
        MemoryPlane::ArchiveEvidence,
    ))?;

    let events = stored_events(db.path());
    assert_eq!(events.len(), 2);

    assert_eq!(events[0].seq, 1);
    assert!(!events[0].event_id.trim().is_empty());
    assert_eq!(events[0].kind, "record_inserted");
    assert_eq!(events[0].record_id, first.id);
    assert!(events[0].payload_json.contains(&first.id));
    assert!(events[0].payload_json.contains("sqlite event append first"));

    assert_eq!(events[1].seq, 2);
    assert!(!events[1].event_id.trim().is_empty());
    assert_ne!(events[0].event_id, events[1].event_id);
    assert_eq!(events[1].kind, "record_inserted");
    assert_eq!(events[1].record_id, second.id);
    assert!(events[1].payload_json.contains(&second.id));
    assert!(events[1].payload_json.contains("ArchiveEvidence"));

    Ok(())
}

fn new_record(content: &str, plane: MemoryPlane) -> NewMemoryRecord {
    NewMemoryRecord {
        identity: "agent:sqlite".to_owned(),
        scope: "task:s2-sqlite".to_owned(),
        content: content.to_owned(),
        source: "sqlite-store-test".to_owned(),
        domain: plane.domain(),
        plane,
        meta: MemoryRecordMeta::default_for_plane(plane),
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Column {
    name: String,
    sql_type: String,
    not_null: bool,
    primary_key_index: i64,
}

fn column(name: &str, sql_type: &str, not_null: bool, primary_key_index: i64) -> Column {
    Column {
        name: name.to_owned(),
        sql_type: sql_type.to_owned(),
        not_null,
        primary_key_index,
    }
}

fn table_names(path: &Path) -> BTreeSet<String> {
    let conn = Connection::open(path).expect("sqlite schema database should open");
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' \
             AND name IN ('memory_records', 'store_events', 'store_meta')",
        )
        .expect("schema table query should prepare");

    stmt.query_map([], |row| row.get::<_, String>(0))
        .expect("schema table query should run")
        .map(|row| row.expect("schema table row should decode"))
        .collect()
}

fn table_columns(path: &Path, table: &str) -> Vec<Column> {
    let conn = Connection::open(path).expect("sqlite schema database should open");
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table_info query should prepare");

    stmt.query_map([], |row| {
        Ok(Column {
            name: row.get(1)?,
            sql_type: row.get(2)?,
            not_null: row.get::<_, i64>(3)? == 1,
            primary_key_index: row.get(5)?,
        })
    })
    .expect("table_info query should run")
    .map(|row| row.expect("table_info row should decode"))
    .collect()
}

#[derive(Debug, Eq, PartialEq)]
struct StoredRecordRow {
    id: String,
    identity: String,
    scope: String,
    content: String,
    source: String,
    domain: String,
    plane: String,
}

fn stored_record_rows(path: &Path) -> Vec<StoredRecordRow> {
    let conn = Connection::open(path).expect("sqlite records database should open");
    let mut stmt = conn
        .prepare(
            "SELECT id, identity, scope, content, source, domain, plane \
             FROM memory_records \
             ORDER BY id",
        )
        .expect("record row query should prepare");

    stmt.query_map([], |row| {
        Ok(StoredRecordRow {
            id: row.get(0)?,
            identity: row.get(1)?,
            scope: row.get(2)?,
            content: row.get(3)?,
            source: row.get(4)?,
            domain: row.get(5)?,
            plane: row.get(6)?,
        })
    })
    .expect("record row query should run")
    .map(|row| row.expect("record row should decode"))
    .collect()
}

#[derive(Debug, Eq, PartialEq)]
struct StoredEvent {
    seq: i64,
    event_id: String,
    kind: String,
    record_id: String,
    payload_json: String,
}

fn stored_events(path: &Path) -> Vec<StoredEvent> {
    let conn = Connection::open(path).expect("sqlite events database should open");
    let mut stmt = conn
        .prepare(
            "SELECT seq, event_id, kind, record_id, payload_json \
             FROM store_events \
             ORDER BY seq",
        )
        .expect("event row query should prepare");

    stmt.query_map([], |row| {
        Ok(StoredEvent {
            seq: row.get(0)?,
            event_id: row.get(1)?,
            kind: row.get(2)?,
            record_id: row.get(3)?,
            payload_json: row.get(4)?,
        })
    })
    .expect("event row query should run")
    .map(|row| row.expect("event row should decode"))
    .collect()
}

struct TempSqlitePath {
    dir: PathBuf,
    db: PathBuf,
}

impl TempSqlitePath {
    fn new(test_name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "bm-store-{test_name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temporary sqlite test directory should be created");

        Self {
            db: dir.join("memory.sqlite3"),
            dir,
        }
    }

    fn path(&self) -> &Path {
        &self.db
    }
}

impl Drop for TempSqlitePath {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
