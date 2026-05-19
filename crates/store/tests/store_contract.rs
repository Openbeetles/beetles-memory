use bm_core::{MemoryPlane, NewMemoryRecord};
use bm_store::{
    InMemoryStore, MemoryStore, StoreError, StoreErrorKind, StoreOperation, StoreResult,
};

#[test]
fn in_memory_store_uses_store_result_contract() -> StoreResult<()> {
    let mut store = InMemoryStore::default();
    let inserted = store.insert(NewMemoryRecord {
        identity: "agent:test".to_owned(),
        scope: "task:s2".to_owned(),
        content: "store contract fact".to_owned(),
        source: "unit-test".to_owned(),
        domain: MemoryPlane::SharedFactual.domain(),
        plane: MemoryPlane::SharedFactual,
    })?;

    assert_eq!(inserted.id, "mem-1");
    assert_eq!(store.records()?.len(), 1);
    assert_eq!(store.health().record_count, 1);

    let snapshot = store.snapshot()?;
    assert_eq!(snapshot.schema_version, 1);
    assert_eq!(snapshot.record_count, 1);
    assert_eq!(snapshot.snapshot_event_seq, 1);
    Ok(())
}

#[test]
fn store_error_keeps_operation_path_and_recoverable_flag() {
    let err = StoreError::new(
        StoreErrorKind::CorruptEventLog,
        StoreOperation::AppendEvent,
        "bad event",
    )
    .path("/tmp/events.jsonl")
    .recoverable(false);

    assert_eq!(err.kind, StoreErrorKind::CorruptEventLog);
    assert_eq!(err.operation, StoreOperation::AppendEvent);
    assert_eq!(err.path.as_deref(), Some("/tmp/events.jsonl"));
    assert_eq!(err.message, "bad event");
    assert!(!err.recoverable);
}

#[test]
fn snapshot_report_exposes_schema_and_event_seq() -> StoreResult<()> {
    let mut store = InMemoryStore::default();
    store.insert(NewMemoryRecord {
        identity: "agent:test".to_owned(),
        scope: "task:s2".to_owned(),
        content: "snapshot fact".to_owned(),
        source: "unit-test".to_owned(),
        domain: MemoryPlane::SharedFactual.domain(),
        plane: MemoryPlane::SharedFactual,
    })?;

    let snapshot = store.snapshot()?;

    assert_eq!(snapshot.schema_version, 1);
    assert_eq!(snapshot.snapshot_event_seq, 1);
    assert_eq!(snapshot.record_count, 1);
    Ok(())
}
