use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use bm_core::{Error, Result};
use serde_json::Value;

use crate::{
    enforce_event_key_budget, enforce_logical_key_budget, store_budget_error, MemoryStoreEvent,
    StoreCapacityBudget, StoreEngine, StoreEventLog, StoreSnapshotBlob, StoreSnapshotJsonDoc,
    StoreSnapshotReplaceReport,
};

pub struct InMemoryStoreEngine {
    capacity: StoreCapacityBudget,
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
    pub fn new(capacity: StoreCapacityBudget) -> Self {
        Self {
            capacity,
            state: Mutex::new(InMemoryStoreEngineState::default()),
        }
    }
}

impl Default for InMemoryStoreEngine {
    fn default() -> Self {
        Self::new(StoreCapacityBudget::full())
    }
}

impl StoreEventLog for InMemoryStoreEngine {
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        enforce_event_key_budget(self.capacity, &event, "in_memory_store_event")?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.event_ids.insert(event.event_id.clone()) {
            return Err(Error::config(
                "store_event_log",
                format!("duplicate event id {}", event.event_id),
            ));
        }
        if state.events.len() >= self.capacity.event_log_max_items {
            state.event_ids.remove(&event.event_id);
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                state.events.len().saturating_add(1),
                self.capacity.event_log_max_items
            )));
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
        enforce_logical_key_budget(self.capacity, namespace, key, "in_memory_json_read")?;
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .json
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()> {
        enforce_logical_key_budget(self.capacity, namespace, key, "in_memory_json_write")?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let storage_key = (namespace.to_string(), key.to_string());
        if !state.json.contains_key(&storage_key)
            && state.json.len() >= self.capacity.kv_max_entries
        {
            return Err(store_budget_error(format!(
                "kv entries {} exceed {}",
                state.json.len().saturating_add(1),
                self.capacity.kv_max_entries
            )));
        }
        state.json.insert(storage_key, value);
        Ok(())
    }

    fn put_json_value_and_event(
        &self,
        namespace: &str,
        key: &str,
        value: Value,
        event: MemoryStoreEvent,
    ) -> Result<()> {
        enforce_event_key_budget(self.capacity, &event, "in_memory_json_write")?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.event_ids.contains(&event.event_id) {
            return Err(Error::config(
                "store_event_log",
                format!("duplicate event id {}", event.event_id),
            ));
        }
        if state.events.len() >= self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                state.events.len().saturating_add(1),
                self.capacity.event_log_max_items
            )));
        }
        enforce_logical_key_budget(self.capacity, namespace, key, "in_memory_json_write")?;
        let storage_key = (namespace.to_string(), key.to_string());
        if !state.json.contains_key(&storage_key)
            && state.json.len() >= self.capacity.kv_max_entries
        {
            return Err(store_budget_error(format!(
                "kv entries {} exceed {}",
                state.json.len().saturating_add(1),
                self.capacity.kv_max_entries
            )));
        }
        state.json.insert(storage_key, value);
        state.event_ids.insert(event.event_id.clone());
        state.events.push(event);
        Ok(())
    }

    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool> {
        enforce_logical_key_budget(self.capacity, namespace, key, "in_memory_json_delete")?;
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .json
            .remove(&(namespace.to_string(), key.to_string()))
            .is_some())
    }

    fn delete_json_value_and_event(
        &self,
        namespace: &str,
        key: &str,
        event: MemoryStoreEvent,
    ) -> Result<bool> {
        enforce_logical_key_budget(self.capacity, namespace, key, "in_memory_json_delete")?;
        enforce_event_key_budget(self.capacity, &event, "in_memory_json_delete")?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state
            .json
            .contains_key(&(namespace.to_string(), key.to_string()))
        {
            return Ok(false);
        }
        if state.event_ids.contains(&event.event_id) {
            return Err(Error::config(
                "store_event_log",
                format!("duplicate event id {}", event.event_id),
            ));
        }
        if state.events.len() >= self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                state.events.len().saturating_add(1),
                self.capacity.event_log_max_items
            )));
        }
        state.json.remove(&(namespace.to_string(), key.to_string()));
        state.event_ids.insert(event.event_id.clone());
        state.events.push(event);
        Ok(true)
    }

    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>> {
        enforce_logical_key_budget(self.capacity, namespace, "", "in_memory_json_list")?;
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
        enforce_logical_key_budget(self.capacity, namespace, key, "in_memory_blob_read")?;
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blobs
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        enforce_logical_key_budget(self.capacity, namespace, key, "in_memory_blob_write")?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let storage_key = (namespace.to_string(), key.to_string());
        let current_bytes = state.blobs.values().map(Vec::len).sum::<usize>();
        let previous = state.blobs.get(&storage_key).map(Vec::len).unwrap_or(0);
        let next_bytes = current_bytes
            .saturating_sub(previous)
            .saturating_add(value.len());
        if next_bytes > self.capacity.blob_max_bytes {
            return Err(store_budget_error(format!(
                "blob bytes {} exceed {}",
                next_bytes, self.capacity.blob_max_bytes
            )));
        }
        state.blobs.insert(storage_key, value.to_vec());
        Ok(())
    }

    fn put_blob_and_event(
        &self,
        namespace: &str,
        key: &str,
        value: &[u8],
        event: MemoryStoreEvent,
    ) -> Result<()> {
        enforce_event_key_budget(self.capacity, &event, "in_memory_blob_write")?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.event_ids.contains(&event.event_id) {
            return Err(Error::config(
                "store_event_log",
                format!("duplicate event id {}", event.event_id),
            ));
        }
        if state.events.len() >= self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                state.events.len().saturating_add(1),
                self.capacity.event_log_max_items
            )));
        }
        enforce_logical_key_budget(self.capacity, namespace, key, "in_memory_blob_write")?;
        let storage_key = (namespace.to_string(), key.to_string());
        let current_bytes = state.blobs.values().map(Vec::len).sum::<usize>();
        let previous = state.blobs.get(&storage_key).map(Vec::len).unwrap_or(0);
        let next_bytes = current_bytes
            .saturating_sub(previous)
            .saturating_add(value.len());
        if next_bytes > self.capacity.blob_max_bytes {
            return Err(store_budget_error(format!(
                "blob bytes {} exceed {}",
                next_bytes, self.capacity.blob_max_bytes
            )));
        }
        state.blobs.insert(storage_key, value.to_vec());
        state.event_ids.insert(event.event_id.clone());
        state.events.push(event);
        Ok(())
    }

    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool> {
        enforce_logical_key_budget(self.capacity, namespace, key, "in_memory_blob_delete")?;
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blobs
            .remove(&(namespace.to_string(), key.to_string()))
            .is_some())
    }

    fn delete_blob_and_event(
        &self,
        namespace: &str,
        key: &str,
        event: MemoryStoreEvent,
    ) -> Result<bool> {
        enforce_logical_key_budget(self.capacity, namespace, key, "in_memory_blob_delete")?;
        enforce_event_key_budget(self.capacity, &event, "in_memory_blob_delete")?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state
            .blobs
            .contains_key(&(namespace.to_string(), key.to_string()))
        {
            return Ok(false);
        }
        if state.event_ids.contains(&event.event_id) {
            return Err(Error::config(
                "store_event_log",
                format!("duplicate event id {}", event.event_id),
            ));
        }
        if state.events.len() >= self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                state.events.len().saturating_add(1),
                self.capacity.event_log_max_items
            )));
        }
        state
            .blobs
            .remove(&(namespace.to_string(), key.to_string()));
        state.event_ids.insert(event.event_id.clone());
        state.events.push(event);
        Ok(true)
    }

    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>> {
        enforce_logical_key_budget(self.capacity, namespace, "", "in_memory_blob_list")?;
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
        if events.len() > self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                events.len(),
                self.capacity.event_log_max_items
            )));
        }
        let mut event_ids = BTreeSet::new();
        for event in events {
            enforce_event_key_budget(self.capacity, event, "in_memory_event_replace")?;
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
        if events.len() > self.capacity.event_log_max_items {
            return Err(store_budget_error(format!(
                "event log items {} exceed {}",
                events.len(),
                self.capacity.event_log_max_items
            )));
        }
        let mut event_ids = BTreeSet::new();
        for event in events {
            enforce_event_key_budget(self.capacity, event, "in_memory_snapshot_import")?;
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
            .map(|doc| {
                enforce_logical_key_budget(
                    self.capacity,
                    &doc.namespace,
                    &doc.key,
                    "in_memory_snapshot_import",
                )?;
                Ok((doc.namespace.clone(), doc.key.clone()))
            })
            .collect::<Result<BTreeSet<_>>>()?;
        let blob_snapshot_keys = blobs
            .iter()
            .map(|blob| {
                enforce_logical_key_budget(
                    self.capacity,
                    &blob.namespace,
                    &blob.key,
                    "in_memory_snapshot_import",
                )?;
                Ok((blob.namespace.clone(), blob.key.clone()))
            })
            .collect::<Result<BTreeSet<_>>>()?;

        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let retained_json_entries = state
            .json
            .keys()
            .filter(|(namespace, _key)| !json_namespace_set.contains(namespace.as_str()))
            .count();
        let final_json_entries = retained_json_entries.saturating_add(json_docs.len());
        if final_json_entries > self.capacity.kv_max_entries {
            return Err(store_budget_error(format!(
                "kv entries {} exceed {}",
                final_json_entries, self.capacity.kv_max_entries
            )));
        }
        let retained_blob_bytes = state
            .blobs
            .iter()
            .filter(|((namespace, _key), _value)| !blob_namespace_set.contains(namespace.as_str()))
            .map(|(_key, value)| value.len())
            .sum::<usize>();
        let snapshot_blob_bytes = blobs.iter().map(|blob| blob.value.len()).sum::<usize>();
        let final_blob_bytes = retained_blob_bytes.saturating_add(snapshot_blob_bytes);
        if final_blob_bytes > self.capacity.blob_max_bytes {
            return Err(store_budget_error(format!(
                "blob bytes {} exceed {}",
                final_blob_bytes, self.capacity.blob_max_bytes
            )));
        }
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
