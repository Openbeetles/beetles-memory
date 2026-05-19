use bm_core::{
    MemoryRecord, NewMemoryRecord, RecallQuery, RuntimeProfile, WriteCandidate, WriteDecision,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::{
    MemoryStore, StoreError, StoreErrorKind, StoreHealthReport, StoreOperation, StoreResult,
    StoreSnapshotReport,
};

#[derive(Clone, Debug)]
struct FailingStore {
    fail_insert: bool,
    fail_records: bool,
}

impl MemoryStore for FailingStore {
    fn insert(&mut self, _record: NewMemoryRecord) -> StoreResult<MemoryRecord> {
        if self.fail_insert {
            Err(StoreError::new(
                StoreErrorKind::BackendUnavailable,
                StoreOperation::InsertRecord,
                "insert backend unavailable",
            ))
        } else {
            unreachable!("test only exercises failure")
        }
    }

    fn replace(&mut self, _record: MemoryRecord) -> StoreResult<MemoryRecord> {
        Err(StoreError::new(
            StoreErrorKind::BackendUnavailable,
            StoreOperation::ReplaceRecord,
            "replace backend unavailable",
        ))
    }

    fn delete(&mut self, _record_id: &str) -> StoreResult<bool> {
        Err(StoreError::new(
            StoreErrorKind::BackendUnavailable,
            StoreOperation::DeleteRecord,
            "delete backend unavailable",
        ))
    }

    fn records(&self) -> StoreResult<Vec<MemoryRecord>> {
        if self.fail_records {
            Err(StoreError::new(
                StoreErrorKind::BackendUnavailable,
                StoreOperation::ReadRecords,
                "records backend unavailable",
            ))
        } else {
            Ok(Vec::new())
        }
    }

    fn snapshot(&mut self) -> StoreResult<StoreSnapshotReport> {
        unreachable!("not used by sdk failure tests")
    }

    fn health(&self) -> StoreHealthReport {
        StoreHealthReport {
            backend: "failing",
            healthy: false,
            record_count: 0,
            last_event_seq: 0,
            snapshot_event_seq: 0,
        }
    }
}

#[test]
fn insert_failure_becomes_deferred_write_report() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(FailingStore {
            fail_insert: true,
            fail_records: false,
        })
        .build();

    let report = runtime.write(
        WriteCandidate::new("agent:test", "task:s2", "store unavailable fact").source("unit-test"),
    );

    assert_eq!(report.decision, WriteDecision::Deferred);
    assert_eq!(report.governance.reason, "store_unavailable");
    assert!(report
        .governance
        .detail
        .as_deref()
        .unwrap_or_default()
        .contains("insert backend unavailable"));
}

#[test]
fn records_failure_becomes_recall_warning() {
    let runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(FailingStore {
            fail_insert: false,
            fail_records: true,
        })
        .build();

    let report = runtime.recall(RecallQuery::new("task:s2"));

    assert!(report.selected.is_empty());
    assert!(report
        .warnings
        .iter()
        .any(|warning| format!("{warning:?}").contains("records backend unavailable")));
}
