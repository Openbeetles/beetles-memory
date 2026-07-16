use bm_core::Result;
use serde::Serialize;
use serde_json::Value;

use crate::store_internal::transaction::{
    ConditionalDeleteEventTemplate, StoreAdmissionAuthority, StoreBoundedKnownKeyReadResult,
    StoreImmutableReadSession, StoreTransactionAdmission,
};
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
    pub mounted_subject_id: String,
}

impl StoreScopedProjectionScope {
    pub fn new(
        memory_space_id: impl Into<String>,
        mounted_subject_id: impl Into<String>,
    ) -> Result<Self> {
        let scope = Self {
            memory_space_id: memory_space_id.into(),
            mounted_subject_id: mounted_subject_id.into(),
        };
        if scope.memory_space_id.trim().is_empty()
            || scope.mounted_subject_id.trim().is_empty()
            || scope.memory_space_id != scope.memory_space_id.trim()
            || scope.mounted_subject_id != scope.mounted_subject_id.trim()
        {
            return Err(bm_core::Error::config(
                "store_scoped_projection",
                "memory_space_id and mounted_subject_id must be canonical non-empty values",
            ));
        }
        Ok(scope)
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

pub trait StoreEngine: StoreEventLog {
    fn admission_authority(&self) -> &StoreAdmissionAuthority;
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
