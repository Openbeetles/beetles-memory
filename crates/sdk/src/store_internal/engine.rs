use bm_core::Result;
use serde_json::Value;

use crate::{MemoryStoreEvent, StoreEventLog, StoreSnapshotBlob, StoreSnapshotJsonDoc};
use crate::{
    StoreConsistentNamespaceReadRequest, StoreConsistentNamespaceReadResult,
    StoreTransactionReport, StoreTransactionRequest,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreConsistentReadRequest, StoreConsistentReadResult};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreSnapshotReplaceReport {
    pub json_deleted: usize,
    pub blobs_deleted: usize,
    pub events_imported: usize,
}

pub trait StoreEngine: StoreEventLog {
    fn commit_transaction(
        &self,
        request: &StoreTransactionRequest,
    ) -> Result<StoreTransactionReport>;
    #[cfg(feature = "nonproduction-replay-harness")]
    fn read_consistent(
        &self,
        request: &StoreConsistentReadRequest,
    ) -> Result<StoreConsistentReadResult>;
    fn read_consistent_namespaces(
        &self,
        request: &StoreConsistentNamespaceReadRequest,
    ) -> Result<StoreConsistentNamespaceReadResult>;
    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>>;
    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()>;
    fn put_json_value_and_event(
        &self,
        namespace: &str,
        key: &str,
        value: Value,
        event: MemoryStoreEvent,
    ) -> Result<()> {
        self.put_json_value(namespace, key, value)?;
        self.append_event(event)
    }
    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool>;
    fn delete_json_value_and_event(
        &self,
        namespace: &str,
        key: &str,
        event: MemoryStoreEvent,
    ) -> Result<bool> {
        let deleted = self.delete_json_value(namespace, key)?;
        if deleted {
            self.append_event(event)?;
        }
        Ok(deleted)
    }
    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>>;
    fn get_blob(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>>;
    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()>;
    fn put_blob_and_event(
        &self,
        namespace: &str,
        key: &str,
        value: &[u8],
        event: MemoryStoreEvent,
    ) -> Result<()> {
        self.put_blob(namespace, key, value)?;
        self.append_event(event)
    }
    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool>;
    fn delete_blob_and_event(
        &self,
        namespace: &str,
        key: &str,
        event: MemoryStoreEvent,
    ) -> Result<bool> {
        let deleted = self.delete_blob(namespace, key)?;
        if deleted {
            self.append_event(event)?;
        }
        Ok(deleted)
    }
    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>>;
    fn replace_snapshot(
        &self,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
        json_docs: &[StoreSnapshotJsonDoc],
        blobs: &[StoreSnapshotBlob],
        events: &[MemoryStoreEvent],
    ) -> Result<StoreSnapshotReplaceReport>;
}
