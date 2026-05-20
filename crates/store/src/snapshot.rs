use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{MemoryStoreEvent, StoreSchemaManifest, STORE_SCHEMA_ID};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StoreSnapshot {
    pub schema_id: String,
    pub schema_manifest: StoreSchemaManifest,
    pub json_docs: Vec<StoreSnapshotJsonDoc>,
    pub blobs: Vec<StoreSnapshotBlob>,
    pub events: Vec<MemoryStoreEvent>,
}

impl StoreSnapshot {
    pub fn new(
        schema_manifest: StoreSchemaManifest,
        json_docs: Vec<StoreSnapshotJsonDoc>,
        blobs: Vec<StoreSnapshotBlob>,
        events: Vec<MemoryStoreEvent>,
    ) -> Self {
        Self {
            schema_id: STORE_SCHEMA_ID.to_string(),
            schema_manifest,
            json_docs,
            blobs,
            events,
        }
    }

    pub fn export_report(&self) -> StoreSnapshotExportReport {
        StoreSnapshotExportReport {
            schema_id: self.schema_id.clone(),
            json_docs: self.json_docs.len(),
            blobs: self.blobs.len(),
            events: self.events.len(),
            state_fingerprint: self.state_fingerprint(),
            event_fingerprint: self.event_fingerprint(),
        }
    }

    pub fn state_fingerprint(&self) -> String {
        let mut json_docs = self.json_docs.clone();
        json_docs.sort_by(|left, right| {
            left.namespace
                .cmp(&right.namespace)
                .then_with(|| left.key.cmp(&right.key))
        });
        let mut blobs = self.blobs.clone();
        blobs.sort_by(|left, right| {
            left.namespace
                .cmp(&right.namespace)
                .then_with(|| left.key.cmp(&right.key))
        });
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for doc in json_docs {
            doc.namespace.hash(&mut hasher);
            doc.key.hash(&mut hasher);
            serde_json::to_string(&doc.value)
                .unwrap_or_default()
                .hash(&mut hasher);
        }
        for blob in blobs {
            blob.namespace.hash(&mut hasher);
            blob.key.hash(&mut hasher);
            blob.value.hash(&mut hasher);
        }
        format!("{:016x}", hasher.finish())
    }

    pub fn event_fingerprint(&self) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for event in &self.events {
            event.event_id.hash(&mut hasher);
            event.kind_name.hash(&mut hasher);
            event.scope.agent_id.hash(&mut hasher);
            event.scope.owner_id.hash(&mut hasher);
            event.scope.channel.hash(&mut hasher);
            event.scope.chat_id.hash(&mut hasher);
            event.plane.hash(&mut hasher);
            event.record_key.hash(&mut hasher);
            event.content_hash.hash(&mut hasher);
            event.schema_version.hash(&mut hasher);
            event.timestamp_unix_secs.hash(&mut hasher);
        }
        format!("{:016x}", hasher.finish())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StoreSnapshotJsonDoc {
    pub namespace: String,
    pub key: String,
    pub value: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreSnapshotBlob {
    pub namespace: String,
    pub key: String,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreSnapshotExportReport {
    pub schema_id: String,
    pub json_docs: usize,
    pub blobs: usize,
    pub events: usize,
    pub state_fingerprint: String,
    pub event_fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreSnapshotImportReport {
    pub schema_id: String,
    pub json_docs: usize,
    pub blobs: usize,
    pub json_deleted: usize,
    pub blobs_deleted: usize,
    pub events_imported: usize,
    pub events_skipped: usize,
    pub state_fingerprint: String,
    pub event_fingerprint: String,
}
