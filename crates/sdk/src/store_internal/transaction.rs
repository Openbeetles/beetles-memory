use std::collections::{BTreeMap, BTreeSet};

use bm_core::{Error, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    enforce_event_key_budget, enforce_logical_key_budget, store_budget_error, MemoryStoreEvent,
    StoreCapacityBudget, StoreJsonPrecondition, StoreMutationBatch, StoreMutationBudgetReport,
    StoreSnapshotBlob, StoreSnapshotJsonDoc,
};

#[cfg(feature = "nonproduction-replay-harness")]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StoreJsonAddress {
    pub namespace: String,
    pub key: String,
}

#[cfg(feature = "nonproduction-replay-harness")]
impl StoreJsonAddress {
    pub fn new(namespace: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            key: key.into(),
        }
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
pub type StoreBlobAddress = StoreJsonAddress;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreEngineMutation {
    PutJson {
        namespace: String,
        key: String,
        value: Value,
    },
    DeleteJson {
        namespace: String,
        key: String,
    },
    PutBlob {
        namespace: String,
        key: String,
        value: Vec<u8>,
    },
    DeleteBlob {
        namespace: String,
        key: String,
    },
    AppendEvent {
        event: Box<MemoryStoreEvent>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GraphRepairAuthority(());

impl GraphRepairAuthority {
    pub(crate) fn issue_for_integrity_maintenance() -> Self {
        Self(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreTransactionRequest {
    pub transaction_id: String,
    pub preconditions: Vec<StoreJsonPrecondition>,
    pub mutations: Vec<StoreEngineMutation>,
    pub governed_batch: Option<Box<StoreMutationBatch>>,
    graph_repair_authority: Option<GraphRepairAuthority>,
}

impl StoreTransactionRequest {
    pub fn new(
        transaction_id: impl Into<String>,
        preconditions: Vec<StoreJsonPrecondition>,
        mutations: Vec<StoreEngineMutation>,
        governed_batch: Option<Box<StoreMutationBatch>>,
    ) -> Self {
        Self {
            transaction_id: transaction_id.into(),
            preconditions,
            mutations,
            governed_batch,
            graph_repair_authority: None,
        }
    }

    pub(crate) fn authorize_graph_repair(mut self, authority: GraphRepairAuthority) -> Self {
        self.graph_repair_authority = Some(authority);
        self
    }

    fn graph_repair_authorized(&self) -> bool {
        self.graph_repair_authority.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreTransactionReport {
    pub transaction_id: String,
    pub changed_json: usize,
    pub changed_blobs: usize,
    pub appended_events: usize,
    pub event_ids: Vec<String>,
    pub budget_report: StoreMutationBudgetReport,
}

#[cfg(feature = "nonproduction-replay-harness")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreConsistentReadRequest {
    pub json: Vec<StoreJsonAddress>,
    pub blobs: Vec<StoreBlobAddress>,
    pub include_events: bool,
}

#[cfg(feature = "nonproduction-replay-harness")]
impl StoreConsistentReadRequest {
    pub fn json(keys: impl IntoIterator<Item = StoreJsonAddress>) -> Self {
        Self {
            json: keys.into_iter().collect(),
            blobs: Vec::new(),
            include_events: false,
        }
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreConsistentReadResult {
    pub json: Vec<StoreConsistentJsonRead>,
    pub blobs: Vec<StoreConsistentBlobRead>,
    pub events: Vec<MemoryStoreEvent>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreConsistentNamespaceReadRequest {
    pub json_namespaces: Vec<String>,
    pub blob_namespaces: Vec<String>,
    pub include_events: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoreConsistentNamespaceReadResult {
    pub json_docs: Vec<StoreSnapshotJsonDoc>,
    pub blobs: Vec<StoreSnapshotBlob>,
    pub events: Vec<MemoryStoreEvent>,
    pub receipt: StoreReadReceipt,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreReadReceipt {
    pub state_digest: String,
    pub json_doc_count: usize,
    pub blob_count: usize,
    pub event_count: usize,
}

#[cfg(feature = "nonproduction-replay-harness")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreConsistentJsonRead {
    pub address: StoreJsonAddress,
    pub value: Option<Value>,
}

#[cfg(feature = "nonproduction-replay-harness")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreConsistentBlobRead {
    pub address: StoreBlobAddress,
    pub value: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BackendTransactionState {
    pub(crate) json: BTreeMap<(String, String), Value>,
    pub(crate) blobs: BTreeMap<(String, String), Vec<u8>>,
    pub(crate) events: Vec<MemoryStoreEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventOverflowPolicy {
    Reject,
    DropOldest,
}

pub(crate) fn apply_transaction(
    capacity: StoreCapacityBudget,
    request: &StoreTransactionRequest,
    current: &BackendTransactionState,
    event_overflow: EventOverflowPolicy,
) -> Result<(BackendTransactionState, StoreTransactionReport)> {
    if request.transaction_id.trim().is_empty() {
        return Err(Error::config(
            "memory_write_transaction_preflight_failed",
            "transaction_id is required",
        ));
    }
    validate_preconditions(capacity, &request.preconditions, &current.json)?;

    let mut next = current.clone();
    let mut changed_json = BTreeSet::new();
    let mut changed_blobs = BTreeSet::new();
    let mut event_ids = Vec::new();
    let mut known_event_ids = next
        .events
        .iter()
        .map(|event| event.event_id.clone())
        .collect::<BTreeSet<_>>();
    let mut mutated_json = BTreeSet::new();
    let mut mutated_blobs = BTreeSet::new();

    for mutation in &request.mutations {
        match mutation {
            StoreEngineMutation::PutJson {
                namespace,
                key,
                value,
            } => {
                enforce_logical_key_budget(capacity, namespace, key, "memory_write_transaction")?;
                reject_duplicate_mutation(&mut mutated_json, namespace, key)?;
                next.json
                    .insert((namespace.clone(), key.clone()), value.clone());
                changed_json.insert((namespace.clone(), key.clone()));
            }
            StoreEngineMutation::DeleteJson { namespace, key } => {
                enforce_logical_key_budget(capacity, namespace, key, "memory_write_transaction")?;
                reject_duplicate_mutation(&mut mutated_json, namespace, key)?;
                if next
                    .json
                    .remove(&(namespace.clone(), key.clone()))
                    .is_some()
                {
                    changed_json.insert((namespace.clone(), key.clone()));
                }
            }
            StoreEngineMutation::PutBlob {
                namespace,
                key,
                value,
            } => {
                enforce_logical_key_budget(capacity, namespace, key, "memory_write_transaction")?;
                reject_duplicate_mutation(&mut mutated_blobs, namespace, key)?;
                next.blobs
                    .insert((namespace.clone(), key.clone()), value.clone());
                changed_blobs.insert((namespace.clone(), key.clone()));
            }
            StoreEngineMutation::DeleteBlob { namespace, key } => {
                enforce_logical_key_budget(capacity, namespace, key, "memory_write_transaction")?;
                reject_duplicate_mutation(&mut mutated_blobs, namespace, key)?;
                if next
                    .blobs
                    .remove(&(namespace.clone(), key.clone()))
                    .is_some()
                {
                    changed_blobs.insert((namespace.clone(), key.clone()));
                }
            }
            StoreEngineMutation::AppendEvent { event } => {
                enforce_event_key_budget(capacity, event, "memory_write_transaction")?;
                if !known_event_ids.insert(event.event_id.clone()) {
                    return Err(Error::config(
                        "store_event_log",
                        format!("duplicate event id {}", event.event_id),
                    ));
                }
                event_ids.push(event.event_id.clone());
                next.events.push((**event).clone());
            }
        }
    }

    if let Some(batch) = request.governed_batch.as_deref() {
        crate::store_internal::platform::validate_governed_transaction_post_image(
            batch,
            current,
            &next,
            request.graph_repair_authorized(),
        )?;
    }

    let kv_entries = next.json.len().saturating_add(next.blobs.len());
    if kv_entries > capacity.kv_max_entries {
        return Err(store_budget_error(format!(
            "kv entries {} exceed {}",
            kv_entries, capacity.kv_max_entries
        )));
    }
    let blob_bytes = next.blobs.values().map(Vec::len).sum::<usize>();
    if blob_bytes > capacity.blob_max_bytes {
        return Err(store_budget_error(format!(
            "blob bytes {} exceed {}",
            blob_bytes, capacity.blob_max_bytes
        )));
    }
    if next.events.len() > capacity.event_log_max_items {
        match event_overflow {
            EventOverflowPolicy::Reject => {
                return Err(store_budget_error(format!(
                    "event log items {} exceed {}",
                    next.events.len(),
                    capacity.event_log_max_items
                )));
            }
            EventOverflowPolicy::DropOldest => {
                let overflow = next
                    .events
                    .len()
                    .saturating_sub(capacity.event_log_max_items);
                next.events.drain(..overflow);
            }
        }
    }

    let budget_report = backend_budget_report(capacity, current, &next);
    Ok((
        next,
        StoreTransactionReport {
            transaction_id: request.transaction_id.clone(),
            changed_json: changed_json.len(),
            changed_blobs: changed_blobs.len(),
            appended_events: event_ids.len(),
            event_ids,
            budget_report,
        },
    ))
}

fn backend_budget_report(
    capacity: StoreCapacityBudget,
    current: &BackendTransactionState,
    next: &BackendTransactionState,
) -> StoreMutationBudgetReport {
    let current_kv_entries = current.json.len().saturating_add(current.blobs.len());
    let next_kv_entries = next.json.len().saturating_add(next.blobs.len());
    let current_blob_bytes = current.blobs.values().map(Vec::len).sum::<usize>();
    let next_blob_bytes = next.blobs.values().map(Vec::len).sum::<usize>();
    StoreMutationBudgetReport {
        required_events: next.events.len().saturating_sub(current.events.len()),
        remaining_events: capacity
            .event_log_max_items
            .saturating_sub(next.events.len()),
        required_kv_entries: next_kv_entries.saturating_sub(current_kv_entries),
        remaining_kv_entries: capacity.kv_max_entries.saturating_sub(next_kv_entries),
        required_blob_bytes: next_blob_bytes.saturating_sub(current_blob_bytes),
        remaining_blob_bytes: capacity.blob_max_bytes.saturating_sub(next_blob_bytes),
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
pub(crate) fn read_consistent_from_state(
    request: &StoreConsistentReadRequest,
    state: &BackendTransactionState,
) -> StoreConsistentReadResult {
    StoreConsistentReadResult {
        json: request
            .json
            .iter()
            .map(|address| StoreConsistentJsonRead {
                address: address.clone(),
                value: state
                    .json
                    .get(&(address.namespace.clone(), address.key.clone()))
                    .cloned(),
            })
            .collect(),
        blobs: request
            .blobs
            .iter()
            .map(|address| StoreConsistentBlobRead {
                address: address.clone(),
                value: state
                    .blobs
                    .get(&(address.namespace.clone(), address.key.clone()))
                    .cloned(),
            })
            .collect(),
        events: if request.include_events {
            state.events.clone()
        } else {
            Vec::new()
        },
    }
}

pub(crate) fn read_consistent_namespaces_from_state(
    request: &StoreConsistentNamespaceReadRequest,
    state: &BackendTransactionState,
) -> Result<StoreConsistentNamespaceReadResult> {
    let json_namespaces = request
        .json_namespaces
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let blob_namespaces = request
        .blob_namespaces
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let json_docs = state
        .json
        .iter()
        .filter(|((namespace, _), _)| json_namespaces.contains(namespace.as_str()))
        .map(|((namespace, key), value)| StoreSnapshotJsonDoc {
            namespace: namespace.clone(),
            key: key.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    let blobs = state
        .blobs
        .iter()
        .filter(|((namespace, _), _)| blob_namespaces.contains(namespace.as_str()))
        .map(|((namespace, key), value)| StoreSnapshotBlob {
            namespace: namespace.clone(),
            key: key.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    let events = if request.include_events {
        state.events.clone()
    } else {
        Vec::new()
    };
    let mut hasher = Sha256::new();
    hasher.update(b"beetle_memory_consistent_namespace_read_v1");
    for doc in &json_docs {
        update_read_digest(&mut hasher, doc.namespace.as_bytes());
        update_read_digest(&mut hasher, doc.key.as_bytes());
        let value = serde_json::to_vec(&doc.value)
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        update_read_digest(&mut hasher, &value);
    }
    for blob in &blobs {
        update_read_digest(&mut hasher, blob.namespace.as_bytes());
        update_read_digest(&mut hasher, blob.key.as_bytes());
        update_read_digest(&mut hasher, &blob.value);
    }
    for event in &events {
        let value = serde_json::to_vec(event)
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        update_read_digest(&mut hasher, &value);
    }
    let receipt = StoreReadReceipt {
        state_digest: format!("{:x}", hasher.finalize()),
        json_doc_count: json_docs.len(),
        blob_count: blobs.len(),
        event_count: events.len(),
    };
    Ok(StoreConsistentNamespaceReadResult {
        json_docs,
        blobs,
        events,
        receipt,
    })
}

fn update_read_digest(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn validate_preconditions(
    capacity: StoreCapacityBudget,
    preconditions: &[StoreJsonPrecondition],
    json: &BTreeMap<(String, String), Value>,
) -> Result<()> {
    let mut addresses = BTreeSet::new();
    for precondition in preconditions {
        let (namespace, key, expected) = match precondition {
            StoreJsonPrecondition::Absent { namespace, key } => (namespace, key, None),
            StoreJsonPrecondition::Exact {
                namespace,
                key,
                value,
            } => (namespace, key, Some(value)),
        };
        enforce_logical_key_budget(capacity, namespace, key, "memory_write_transaction")?;
        if !addresses.insert((namespace.as_str(), key.as_str())) {
            return Err(Error::config(
                "memory_write_transaction_preflight_failed",
                format!("duplicate precondition for {namespace}/{key}"),
            ));
        }
        let observed = json.get(&(namespace.clone(), key.clone()));
        if observed != expected {
            return Err(Error::config(
                "memory_write_transaction_precondition_failed",
                format!("json precondition failed for {namespace}/{key}"),
            ));
        }
    }
    Ok(())
}

fn reject_duplicate_mutation(
    addresses: &mut BTreeSet<(String, String)>,
    namespace: &str,
    key: &str,
) -> Result<()> {
    if addresses.insert((namespace.to_string(), key.to_string())) {
        Ok(())
    } else {
        Err(Error::config(
            "memory_write_transaction_preflight_failed",
            format!("duplicate mutation for {namespace}/{key}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_report_budget_comes_from_the_backend_locked_post_image() {
        let mut capacity = StoreCapacityBudget::full();
        capacity.kv_max_entries = 4;
        capacity.event_log_max_items = 4;
        let mut current = BackendTransactionState::default();
        current.json.insert(
            ("session".to_string(), "existing".to_string()),
            serde_json::json!({"revision": 1}),
        );
        let request = StoreTransactionRequest::new(
            "backend-budget",
            Vec::new(),
            vec![StoreEngineMutation::PutJson {
                namespace: "session".to_string(),
                key: "next".to_string(),
                value: serde_json::json!({"revision": 2}),
            }],
            None,
        );

        let (_, report) =
            apply_transaction(capacity, &request, &current, EventOverflowPolicy::Reject).unwrap();

        assert_eq!(report.budget_report.required_kv_entries, 1);
        assert_eq!(report.budget_report.remaining_kv_entries, 2);
        assert_eq!(report.budget_report.remaining_events, 4);
    }

    #[test]
    fn backend_budget_counts_json_and_blob_entries_together() {
        let mut capacity = StoreCapacityBudget::full();
        capacity.kv_max_entries = 1;
        let current = BackendTransactionState::default();
        let request = StoreTransactionRequest::new(
            "backend-kv-budget",
            Vec::new(),
            vec![
                StoreEngineMutation::PutJson {
                    namespace: "session".to_string(),
                    key: "json".to_string(),
                    value: serde_json::json!({"revision": 1}),
                },
                StoreEngineMutation::PutBlob {
                    namespace: "memory".to_string(),
                    key: "blob".to_string(),
                    value: vec![1],
                },
            ],
            None,
        );

        let error = apply_transaction(capacity, &request, &current, EventOverflowPolicy::Reject)
            .expect_err("json and blob entries share the kv entry budget");

        assert_eq!(error.stage(), "store_budget_exceeded");
    }
}
