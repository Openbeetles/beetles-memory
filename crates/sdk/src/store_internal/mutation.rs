use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{MemoryStoreEvent, MemoryStoreEventKind, StoreEventScope};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreMutationBatch {
    pub transaction_id: String,
    pub operation: String,
    pub scope: StoreEventScope,
    pub mutations: Vec<StoreMutation>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoreJsonPrecondition {
    Absent {
        namespace: String,
        key: String,
    },
    Exact {
        namespace: String,
        key: String,
        value: Value,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoreMutation {
    PutJson {
        namespace: String,
        key: String,
        value: Value,
        event_kind: MemoryStoreEventKind,
        plane: String,
        record_key: String,
    },
    DeleteJson {
        namespace: String,
        key: String,
        event_kind: MemoryStoreEventKind,
        plane: String,
        record_key: String,
    },
    PutBlob {
        namespace: String,
        key: String,
        value: Vec<u8>,
        event_kind: MemoryStoreEventKind,
        plane: String,
        record_key: String,
    },
    DeleteBlob {
        namespace: String,
        key: String,
        event_kind: MemoryStoreEventKind,
        plane: String,
        record_key: String,
    },
    AppendEvent {
        event: MemoryStoreEvent,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreMutationBudgetReport {
    pub admission_report_id: String,
    pub required_events: usize,
    pub remaining_events: usize,
    pub required_kv_entries: usize,
    pub remaining_kv_entries: usize,
    pub required_blob_bytes: usize,
    pub remaining_blob_bytes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreMutationBatchReport {
    pub transaction_id: String,
    pub admitted: bool,
    pub committed: bool,
    pub mutations: usize,
    pub events: usize,
    pub changed_json: usize,
    pub changed_blobs: usize,
    pub budget_report: StoreMutationBudgetReport,
    pub event_ids: Vec<String>,
}
