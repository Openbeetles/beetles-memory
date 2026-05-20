use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use bm_core::{Error, Result};
use serde_json::Value;

use crate::{
    MemoryStoreEvent, StoreEngine, StoreEventLog, StoreSnapshotBlob, StoreSnapshotJsonDoc,
    StoreSnapshotReplaceReport,
};

#[derive(Default)]
pub struct InMemoryStoreEngine {
    state: Mutex<InMemoryStoreEngineState>,
}

#[derive(Default)]
struct InMemoryStoreEngineState {
    json: BTreeMap<(String, String), Value>,
    blobs: BTreeMap<(String, String), Vec<u8>>,
    events: Vec<MemoryStoreEvent>,
    event_ids: BTreeSet<String>,
}

impl InMemoryStoreEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StoreEventLog for InMemoryStoreEngine {
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.event_ids.insert(event.event_id.clone()) {
            return Err(Error::config(
                "store_event_log",
                format!("duplicate event id {}", event.event_id),
            ));
        }
        state.events.push(event);
        Ok(())
    }

    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .events
            .clone())
    }
}

impl StoreEngine for InMemoryStoreEngine {
    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .json
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()> {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .json
            .insert((namespace.to_string(), key.to_string()), value);
        Ok(())
    }

    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .json
            .remove(&(namespace.to_string(), key.to_string()))
            .is_some())
    }

    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>> {
        let mut keys = self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .json
            .keys()
            .filter(|(candidate_namespace, _key)| candidate_namespace == namespace)
            .map(|(_candidate_namespace, key)| key.clone())
            .collect::<Vec<_>>();
        keys.sort();
        Ok(keys)
    }

    fn get_blob(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blobs
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blobs
            .insert((namespace.to_string(), key.to_string()), value.to_vec());
        Ok(())
    }

    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blobs
            .remove(&(namespace.to_string(), key.to_string()))
            .is_some())
    }

    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>> {
        let mut keys = self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blobs
            .keys()
            .filter(|(candidate_namespace, _key)| candidate_namespace == namespace)
            .map(|(_candidate_namespace, key)| key.clone())
            .collect::<Vec<_>>();
        keys.sort();
        Ok(keys)
    }

    fn replace_events(&self, events: &[MemoryStoreEvent]) -> Result<()> {
        let mut event_ids = BTreeSet::new();
        for event in events {
            if !event_ids.insert(event.event_id.clone()) {
                return Err(Error::config(
                    "store_event_log",
                    format!("duplicate event id {}", event.event_id),
                ));
            }
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.events = events.to_vec();
        state.event_ids = event_ids;
        Ok(())
    }

    fn replace_snapshot(
        &self,
        json_namespaces: &[&str],
        blob_namespaces: &[&str],
        json_docs: &[StoreSnapshotJsonDoc],
        blobs: &[StoreSnapshotBlob],
        events: &[MemoryStoreEvent],
    ) -> Result<StoreSnapshotReplaceReport> {
        let mut event_ids = BTreeSet::new();
        for event in events {
            if !event_ids.insert(event.event_id.clone()) {
                return Err(Error::config(
                    "store_event_log",
                    format!("duplicate event id {}", event.event_id),
                ));
            }
        }

        let json_namespace_set = namespace_set(json_namespaces);
        let blob_namespace_set = namespace_set(blob_namespaces);
        let json_snapshot_keys = json_docs
            .iter()
            .map(|doc| (doc.namespace.clone(), doc.key.clone()))
            .collect::<BTreeSet<_>>();
        let blob_snapshot_keys = blobs
            .iter()
            .map(|blob| (blob.namespace.clone(), blob.key.clone()))
            .collect::<BTreeSet<_>>();

        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let json_deleted = state
            .json
            .keys()
            .filter(|(namespace, key)| {
                json_namespace_set.contains(namespace.as_str())
                    && !json_snapshot_keys.contains(&(namespace.clone(), key.clone()))
            })
            .count();
        let blobs_deleted = state
            .blobs
            .keys()
            .filter(|(namespace, key)| {
                blob_namespace_set.contains(namespace.as_str())
                    && !blob_snapshot_keys.contains(&(namespace.clone(), key.clone()))
            })
            .count();

        state
            .json
            .retain(|(namespace, _key), _value| !json_namespace_set.contains(namespace.as_str()));
        state
            .blobs
            .retain(|(namespace, _key), _value| !blob_namespace_set.contains(namespace.as_str()));
        for doc in json_docs {
            state
                .json
                .insert((doc.namespace.clone(), doc.key.clone()), doc.value.clone());
        }
        for blob in blobs {
            state.blobs.insert(
                (blob.namespace.clone(), blob.key.clone()),
                blob.value.clone(),
            );
        }
        state.events = events.to_vec();
        state.event_ids = event_ids;

        Ok(StoreSnapshotReplaceReport {
            json_deleted,
            blobs_deleted,
            events_imported: events.len(),
        })
    }
}

fn namespace_set(namespaces: &[&str]) -> BTreeSet<String> {
    namespaces
        .iter()
        .map(|namespace| (*namespace).to_string())
        .collect()
}
