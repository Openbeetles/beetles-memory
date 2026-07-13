use std::collections::BTreeSet;

use bm_core::{Error, Result};
use serde_json::Value;

#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_internal::transaction::read_consistent_from_state;
use crate::store_internal::transaction::{
    read_consistent_namespaces_from_state, BackendTransactionState,
};
use crate::{
    MemoryStoreEvent, StoreConsistentNamespaceReadRequest, StoreConsistentNamespaceReadResult,
    StoreEngine, StoreEventLog, StoreSnapshotBlob, StoreSnapshotJsonDoc,
    StoreSnapshotReplaceReport, StoreTransactionReport, StoreTransactionRequest,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreConsistentReadRequest, StoreConsistentReadResult};

pub(crate) struct ReadOnlySnapshotStoreEngine {
    state: BackendTransactionState,
}

impl ReadOnlySnapshotStoreEngine {
    pub(crate) fn from_consistent_read(result: &StoreConsistentNamespaceReadResult) -> Self {
        Self {
            state: BackendTransactionState {
                json: result
                    .json_docs
                    .iter()
                    .map(|doc| ((doc.namespace.clone(), doc.key.clone()), doc.value.clone()))
                    .collect(),
                blobs: result
                    .blobs
                    .iter()
                    .map(|blob| {
                        (
                            (blob.namespace.clone(), blob.key.clone()),
                            blob.value.clone(),
                        )
                    })
                    .collect(),
                events: result.events.clone(),
            },
        }
    }

    fn reject_write<T>(&self) -> Result<T> {
        Err(Error::config(
            "governed_recall_snapshot_write_forbidden",
            "governed recall snapshots are immutable",
        ))
    }
}

impl StoreEventLog for ReadOnlySnapshotStoreEngine {
    fn append_event(&self, _event: MemoryStoreEvent) -> Result<()> {
        self.reject_write()
    }

    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        Ok(self.state.events.clone())
    }
}

impl StoreEngine for ReadOnlySnapshotStoreEngine {
    fn commit_transaction(
        &self,
        _request: &StoreTransactionRequest,
    ) -> Result<StoreTransactionReport> {
        self.reject_write()
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn read_consistent(
        &self,
        request: &StoreConsistentReadRequest,
    ) -> Result<StoreConsistentReadResult> {
        Ok(read_consistent_from_state(request, &self.state))
    }

    fn read_consistent_namespaces(
        &self,
        request: &StoreConsistentNamespaceReadRequest,
    ) -> Result<StoreConsistentNamespaceReadResult> {
        read_consistent_namespaces_from_state(request, &self.state)
    }

    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
        Ok(self
            .state
            .json
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    fn put_json_value(&self, _namespace: &str, _key: &str, _value: Value) -> Result<()> {
        self.reject_write()
    }

    fn delete_json_value(&self, _namespace: &str, _key: &str) -> Result<bool> {
        self.reject_write()
    }

    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>> {
        Ok(self
            .state
            .json
            .keys()
            .filter(|(candidate, _)| candidate == namespace)
            .map(|(_, key)| key.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    fn get_blob(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .state
            .blobs
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    fn put_blob(&self, _namespace: &str, _key: &str, _value: &[u8]) -> Result<()> {
        self.reject_write()
    }

    fn delete_blob(&self, _namespace: &str, _key: &str) -> Result<bool> {
        self.reject_write()
    }

    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>> {
        Ok(self
            .state
            .blobs
            .keys()
            .filter(|(candidate, _)| candidate == namespace)
            .map(|(_, key)| key.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    fn replace_snapshot(
        &self,
        _json_namespaces: &[&str],
        _blob_namespaces: &[&str],
        _json_docs: &[StoreSnapshotJsonDoc],
        _blobs: &[StoreSnapshotBlob],
        _events: &[MemoryStoreEvent],
    ) -> Result<StoreSnapshotReplaceReport> {
        self.reject_write()
    }
}
