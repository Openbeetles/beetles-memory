use std::io::{self, Write};

use bm_core::Result;
use serde::Serialize;
use serde_json::Value;

use crate::store_internal::transaction::{
    ConditionalDeleteEventTemplate, StoreAdmissionAuthority, StoreBoundedKnownKeyReadResult,
    StoreImmutableReadSession, StoreTransactionAdmission,
};
use crate::StorePhysicalOwningScope;
#[cfg(feature = "nonproduction-replay-harness")]
use crate::StoreSnapshotBlob;
use crate::{MemoryStoreEvent, StoreEventLog, StoreSnapshotJsonDoc};
use crate::{
    StoreCapacityBudget, StoreEngineMutation, StoreTransactionReport, StoreTransactionRequest,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreConsistentReadRequest, StoreConsistentReadResult};

#[cfg(feature = "nonproduction-replay-harness")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreSnapshotReplaceReport {
    pub json_deleted: usize,
    pub blobs_deleted: usize,
    pub events_imported: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StoreScopedProjectionScope {
    pub memory_space_id: String,
    pub physical_owning_scope: StorePhysicalOwningScope,
}

impl StoreScopedProjectionScope {
    pub fn subject(
        memory_space_id: impl Into<String>,
        mounted_subject_id: impl Into<String>,
    ) -> Result<Self> {
        let mounted_subject_id = mounted_subject_id.into();
        let scope = Self {
            memory_space_id: memory_space_id.into(),
            physical_owning_scope: StorePhysicalOwningScope::Subject { mounted_subject_id },
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn shared_program(memory_space_id: impl Into<String>) -> Result<Self> {
        let scope = Self {
            memory_space_id: memory_space_id.into(),
            physical_owning_scope: StorePhysicalOwningScope::SharedProgram,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn mounted_subject_id(&self) -> Option<&str> {
        match &self.physical_owning_scope {
            StorePhysicalOwningScope::Subject { mounted_subject_id } => Some(mounted_subject_id),
            StorePhysicalOwningScope::SharedProgram => None,
        }
    }

    pub fn runtime_skill_owning_scope(&self) -> bm_core::skills::RuntimeSkillOwningScope {
        match &self.physical_owning_scope {
            StorePhysicalOwningScope::Subject { mounted_subject_id } => {
                bm_core::skills::RuntimeSkillOwningScope::Subject {
                    mounted_subject_id: mounted_subject_id.clone(),
                }
            }
            StorePhysicalOwningScope::SharedProgram => {
                bm_core::skills::RuntimeSkillOwningScope::SharedProgram
            }
        }
    }

    fn validate(&self) -> Result<()> {
        let canonical_memory_space = !self.memory_space_id.trim().is_empty()
            && self.memory_space_id == self.memory_space_id.trim();
        let canonical_physical_owner = self
            .mounted_subject_id()
            .is_none_or(|subject| !subject.trim().is_empty() && subject == subject.trim());
        if !canonical_memory_space || !canonical_physical_owner {
            return Err(bm_core::Error::config(
                "store_scoped_projection",
                "memory_space_id and physical owning scope must be canonical",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreScopedProjectionRequest {
    pub scope: StoreScopedProjectionScope,
    pub json_namespaces: Vec<String>,
    pub include_events: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreScopedProjection {
    pub scope: StoreScopedProjectionScope,
    pub json_docs: Vec<StoreSnapshotJsonDoc>,
    pub events: Vec<MemoryStoreEvent>,
    pub receipt: crate::StoreReadReceipt,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StoreScopedProjectionReplaceRequest {
    pub scope: StoreScopedProjectionScope,
    pub json_namespaces: Vec<String>,
    pub json_docs: Vec<StoreSnapshotJsonDoc>,
    pub events: Vec<MemoryStoreEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreScopedProjectionReplaceReport {
    pub admission_report_id: String,
    pub deleted_json: usize,
    pub inserted_json: usize,
    pub deleted_events: usize,
    pub inserted_events: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreMetricEventSourceRead {
    pub events: Vec<MemoryStoreEvent>,
    pub accounted_snapshot_bytes: usize,
}

struct MetricEventByteCounter {
    total: usize,
    limit: usize,
}

impl MetricEventByteCounter {
    fn new(limit: usize) -> Self {
        Self { total: 0, limit }
    }
}

impl Write for MetricEventByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self.total.checked_add(bytes.len()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "runtime metric event byte count overflow",
            )
        })?;
        if next > self.limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "runtime metric event source exceeds the active byte budget",
            ));
        }
        self.total = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn materialize_metric_event_source(
    events: &[MemoryStoreEvent],
    capacity: StoreCapacityBudget,
) -> Result<StoreMetricEventSourceRead> {
    if events.len() > capacity.event_log_max_items {
        return Err(bm_core::Error::config(
            "runtime_metrics_event_capacity",
            "runtime metric event source exceeds the active item budget",
        ));
    }
    let mut counter = MetricEventByteCounter::new(capacity.snapshot_max_bytes);
    for event in events {
        serde_json::to_writer(&mut counter, event).map_err(|error| {
            bm_core::Error::config("runtime_metrics_event_bytes", error.to_string())
        })?;
    }
    Ok(StoreMetricEventSourceRead {
        events: events.to_vec(),
        accounted_snapshot_bytes: counter.total,
    })
}

pub trait StoreEngine: StoreEventLog {
    fn admission_authority(&self) -> &StoreAdmissionAuthority;
    fn read_metric_events(
        &self,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreMetricEventSourceRead>;
    #[cfg(feature = "nonproduction-replay-harness")]
    fn store_capacity(&self) -> StoreCapacityBudget;
    #[cfg(feature = "nonproduction-replay-harness")]
    fn commit_transaction(
        &self,
        request: &StoreTransactionRequest,
    ) -> Result<StoreTransactionReport> {
        let admission = StoreTransactionAdmission::for_nonproduction_harness(
            self.store_capacity(),
            self.admission_authority(),
        );
        self.commit_transaction_admitted(request, &admission)
    }
    fn commit_transaction_admitted(
        &self,
        request: &StoreTransactionRequest,
        admission: &StoreTransactionAdmission,
    ) -> Result<StoreTransactionReport>;
    #[cfg(feature = "nonproduction-replay-harness")]
    fn read_consistent(
        &self,
        request: &StoreConsistentReadRequest,
    ) -> Result<StoreConsistentReadResult>;
    fn read_consistent_known_keys(
        &self,
        json_keys: &[(String, String)],
        blob_keys: &[(String, String)],
        include_events: bool,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreBoundedKnownKeyReadResult>;
    fn open_immutable_read_session<'a>(
        &'a self,
        capacity: StoreCapacityBudget,
    ) -> Result<Box<dyn StoreImmutableReadSession + 'a>>;
    fn read_scoped_projection(
        &self,
        request: &StoreScopedProjectionRequest,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreScopedProjection>;
    fn replace_scoped_projection(
        &self,
        request: &StoreScopedProjectionReplaceRequest,
        admission: &StoreTransactionAdmission,
    ) -> Result<StoreScopedProjectionReplaceReport>;
    #[cfg(feature = "nonproduction-replay-harness")]
    fn replace_scoped_projection_with_capacity(
        &self,
        request: &StoreScopedProjectionReplaceRequest,
        operation_capacity: StoreCapacityBudget,
    ) -> Result<StoreScopedProjectionReplaceReport> {
        let admission = StoreTransactionAdmission::for_nonproduction_harness(
            operation_capacity,
            self.admission_authority(),
        );
        self.replace_scoped_projection(request, &admission)
    }
    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>>;
    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()>;
    fn put_json_value_and_event(
        &self,
        namespace: &str,
        key: &str,
        value: Value,
        event: MemoryStoreEvent,
        admission: &StoreTransactionAdmission,
    ) -> Result<()> {
        let request = StoreTransactionRequest::new(
            format!("{}:put-json", event.event_id),
            Vec::new(),
            vec![
                StoreEngineMutation::PutJson {
                    namespace: namespace.to_string(),
                    key: key.to_string(),
                    value,
                },
                StoreEngineMutation::AppendEvent {
                    event: Box::new(event),
                },
            ],
            None,
        );
        self.commit_transaction_admitted(&request, admission)
            .map(|_| ())
    }
    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool>;
    fn delete_json_value_and_materialize_event(
        &self,
        namespace: &str,
        key: &str,
        event_template: ConditionalDeleteEventTemplate,
        admission: &StoreTransactionAdmission,
    ) -> Result<bool> {
        let transaction_id = format!("{}:delete-json", event_template.event_id());
        let request = StoreTransactionRequest::new(
            transaction_id,
            Vec::new(),
            vec![StoreEngineMutation::delete_json_if_present(
                namespace,
                key,
                event_template,
            )],
            None,
        );
        self.commit_transaction_admitted(&request, admission)
            .map(|report| report.changed_json == 1)
    }
    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>>;
    fn get_blob(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>>;
    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()>;
    fn put_blob_and_event(
        &self,
        namespace: &str,
        key: &str,
        value: &[u8],
        event: MemoryStoreEvent,
        admission: &StoreTransactionAdmission,
    ) -> Result<()> {
        let request = StoreTransactionRequest::new(
            format!("{}:put-blob", event.event_id),
            Vec::new(),
            vec![
                StoreEngineMutation::PutBlob {
                    namespace: namespace.to_string(),
                    key: key.to_string(),
                    value: value.to_vec(),
                },
                StoreEngineMutation::AppendEvent {
                    event: Box::new(event),
                },
            ],
            None,
        );
        self.commit_transaction_admitted(&request, admission)
            .map(|_| ())
    }
    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool>;
    fn delete_blob_and_materialize_event(
        &self,
        namespace: &str,
        key: &str,
        event_template: ConditionalDeleteEventTemplate,
        admission: &StoreTransactionAdmission,
    ) -> Result<bool> {
        let transaction_id = format!("{}:delete-blob", event_template.event_id());
        let request = StoreTransactionRequest::new(
            transaction_id,
            Vec::new(),
            vec![StoreEngineMutation::delete_blob_if_present(
                namespace,
                key,
                event_template,
            )],
            None,
        );
        self.commit_transaction_admitted(&request, admission)
            .map(|report| report.changed_blobs == 1)
    }
    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>>;
    #[cfg(feature = "nonproduction-replay-harness")]
    fn replace_snapshot(
        &self,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
        json_docs: &[StoreSnapshotJsonDoc],
        blobs: &[StoreSnapshotBlob],
        events: &[MemoryStoreEvent],
    ) -> Result<StoreSnapshotReplaceReport>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store_internal::{MemoryStoreEventKind, StoreEventScope};

    fn metric_event() -> MemoryStoreEvent {
        MemoryStoreEvent::new(
            "metric-event",
            MemoryStoreEventKind::RuntimeLifecycle,
            StoreEventScope::system("metrics"),
            10,
        )
        .with_payload("operation", "recall")
        .with_payload("success", "true")
        .with_payload("result", "ok")
    }

    #[test]
    fn metric_event_materialization_admits_exact_bytes_and_rejects_n_plus_one() {
        let event = metric_event();
        let exact_bytes = serde_json::to_vec(&event).expect("event bytes").len();
        let mut exact_capacity = StoreCapacityBudget::full();
        exact_capacity.event_log_max_items = 1;
        exact_capacity.snapshot_max_bytes = exact_bytes;

        let admitted =
            materialize_metric_event_source(std::slice::from_ref(&event), exact_capacity)
                .expect("exact metric bytes");
        assert_eq!(admitted.accounted_snapshot_bytes, exact_bytes);
        assert_eq!(admitted.events, vec![event.clone()]);

        let mut below_capacity = exact_capacity;
        below_capacity.snapshot_max_bytes = exact_bytes - 1;
        let error = materialize_metric_event_source(&[event], below_capacity)
            .expect_err("N+1 bytes must fail before the event vector is cloned");
        assert_eq!(error.stage(), "runtime_metrics_event_bytes");
    }
}
