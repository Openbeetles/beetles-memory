use bm_core::{MemoryPlane, MemoryRecord, NewMemoryRecord};
use bm_store::{FileStore, MemoryStore, StoreErrorKind, StoreOperation};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn file_store_persists_records_across_reopen() {
    let root = TempStoreRoot::new("file_store_persists_records_across_reopen");

    {
        let mut store = FileStore::open(root.path()).expect("open empty file store");
        let inserted = store
            .insert(new_record(
                MemoryPlane::SharedFactual,
                "项目名称是 Beetle Memory",
                "operator",
            ))
            .expect("insert governed factual memory");

        assert_eq!(inserted.id, "mem-1");
        assert_eq!(inserted.plane, MemoryPlane::SharedFactual);

        let snapshot = store.snapshot().expect("write snapshot");
        assert_eq!(snapshot.schema_version, 1);
        assert_eq!(snapshot.snapshot_event_seq, 1);
        assert_eq!(snapshot.record_count, 1);
    }

    let reopened = FileStore::open(root.path()).expect("reopen persisted file store");
    let records = reopened.records().expect("read persisted records");

    assert_eq!(records.len(), 1);
    assert_record(
        &records[0],
        "mem-1",
        MemoryPlane::SharedFactual,
        "项目名称是 Beetle Memory",
        "operator",
    );
}

#[test]
fn file_store_reloads_archive_evidence_and_subject_projection_planes() {
    let root =
        TempStoreRoot::new("file_store_reloads_archive_evidence_and_subject_projection_planes");

    {
        let mut store = FileStore::open(root.path()).expect("open empty file store");
        store
            .insert(new_record(
                MemoryPlane::ArchiveEvidence,
                "archive hit remains evidence, not canonical factual memory",
                "archive:hit-s2",
            ))
            .expect("insert archive evidence");
        store
            .insert(new_record(
                MemoryPlane::SubjectProjection,
                "当前回合使用 compact 主体挂载帧；私域原文已过滤。",
                "host:subject-state",
            ))
            .expect("insert subject projection");
    }

    let reopened = FileStore::open(root.path()).expect("reopen file store from event log");
    let records = reopened.records().expect("read reloaded records");

    assert_eq!(records.len(), 2);
    assert_record(
        find_record(&records, MemoryPlane::ArchiveEvidence),
        "mem-1",
        MemoryPlane::ArchiveEvidence,
        "archive hit remains evidence, not canonical factual memory",
        "archive:hit-s2",
    );
    assert_record(
        find_record(&records, MemoryPlane::SubjectProjection),
        "mem-2",
        MemoryPlane::SubjectProjection,
        "当前回合使用 compact 主体挂载帧；私域原文已过滤。",
        "host:subject-state",
    );
}

#[test]
fn bad_snapshot_json_returns_structured_error_without_clearing_store() {
    let root =
        TempStoreRoot::new("bad_snapshot_json_returns_structured_error_without_clearing_store");
    let snapshot_path = root.path().join("snapshot.json");

    write_manifest(root.path(), 0, 0);
    fs::write(&snapshot_path, "{ this is not valid json\n").expect("write bad snapshot");
    fs::write(root.path().join("events.jsonl"), "").expect("write empty event log");

    let result = FileStore::open(root.path());
    assert!(
        result.is_err(),
        "bad snapshot must not be treated as empty store"
    );
    let err = result.expect_err("structured snapshot error");
    let snapshot_path = snapshot_path.to_str().expect("utf8 snapshot path");

    assert_eq!(err.kind, StoreErrorKind::Json);
    assert_eq!(err.operation, StoreOperation::LoadSnapshot);
    assert_eq!(err.path.as_deref(), Some(snapshot_path));
    assert!(!err.recoverable);
    assert!(err.message.contains("snapshot.json"));
    assert_eq!(
        fs::read_to_string(snapshot_path).expect("snapshot still exists"),
        "{ this is not valid json\n"
    );
}

#[test]
fn non_empty_store_without_manifest_is_not_treated_as_empty() {
    let root = TempStoreRoot::new("non_empty_store_without_manifest_is_not_treated_as_empty");
    let events_path = root.path().join("events.jsonl");
    fs::write(&events_path, "").expect("write orphan event log");

    let result = FileStore::open(root.path());
    let err = result.expect_err("non-empty store without manifest should fail");

    assert_eq!(err.kind, StoreErrorKind::UnsupportedSchemaVersion);
    assert_eq!(err.operation, StoreOperation::LoadManifest);
    assert!(!err.recoverable);
    assert!(err.message.contains("manifest.json"));
}

#[test]
fn bad_events_jsonl_returns_corrupt_event_log_with_line_context() {
    let root = TempStoreRoot::new("bad_events_jsonl_returns_corrupt_event_log_with_line_context");
    let events_path = root.path().join("events.jsonl");

    write_manifest(root.path(), 1, 0);
    write_empty_snapshot(root.path());
    fs::write(
        &events_path,
        concat!(
            "{\"seq\":1,\"event_id\":\"evt-1\",\"kind\":\"record_inserted\",\"record_id\":\"mem-1\",\"record\":{\"id\":\"mem-1\",\"identity\":\"agent:s2\",\"scope\":\"task:s2:file-store\",\"content\":\"valid first event\",\"source\":\"unit-test\",\"domain\":\"Program\",\"plane\":\"SharedFactual\"}}\n",
            "{ this is not valid json\n"
        ),
    )
    .expect("write bad event log");

    let result = FileStore::open(root.path());
    assert!(result.is_err(), "bad events.jsonl line must fail open");
    let err = result.expect_err("structured event log error");
    let events_path = events_path.to_str().expect("utf8 events path");

    assert_eq!(err.kind, StoreErrorKind::CorruptEventLog);
    assert_eq!(err.path.as_deref(), Some(events_path));
    assert!(!err.recoverable);
    assert!(err.message.contains("events.jsonl"));
    assert!(err.message.contains("line 2"));
}

fn new_record(plane: MemoryPlane, content: &str, source: &str) -> NewMemoryRecord {
    NewMemoryRecord {
        identity: "agent:s2".to_owned(),
        scope: "task:s2:file-store".to_owned(),
        content: content.to_owned(),
        source: source.to_owned(),
        domain: plane.domain(),
        plane,
    }
}

fn assert_record(record: &MemoryRecord, id: &str, plane: MemoryPlane, content: &str, source: &str) {
    assert_eq!(record.id, id);
    assert_eq!(record.identity, "agent:s2");
    assert_eq!(record.scope, "task:s2:file-store");
    assert_eq!(record.domain, plane.domain());
    assert_eq!(record.plane, plane);
    assert_eq!(record.content, content);
    assert_eq!(record.source, source);
}

fn find_record(records: &[MemoryRecord], plane: MemoryPlane) -> &MemoryRecord {
    records
        .iter()
        .find(|record| record.plane == plane)
        .unwrap_or_else(|| panic!("record with plane {plane:?}"))
}

fn write_manifest(root: &Path, last_event_seq: u64, snapshot_event_seq: u64) {
    fs::write(
        root.join("manifest.json"),
        format!(
            concat!(
                "{{\n",
                "  \"schema_version\": 1,\n",
                "  \"backend\": \"file\",\n",
                "  \"last_event_seq\": {},\n",
                "  \"snapshot_event_seq\": {}\n",
                "}}\n"
            ),
            last_event_seq, snapshot_event_seq
        ),
    )
    .expect("write manifest");
}

fn write_empty_snapshot(root: &Path) {
    fs::write(
        root.join("snapshot.json"),
        concat!(
            "{\n",
            "  \"schema_version\": 1,\n",
            "  \"next_id\": 1,\n",
            "  \"records\": []\n",
            "}\n"
        ),
    )
    .expect("write empty snapshot");
}

struct TempStoreRoot {
    path: PathBuf,
}

impl TempStoreRoot {
    fn new(test_name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bm-s2-{test_name}-{}-{unique}", std::process::id()));
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
