use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use bm_core::{Error, Result};
use serde_json::Value;

#[cfg(feature = "nonproduction-replay-harness")]
use crate::enforce_event_key_budget;
#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_budget_error;
#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_internal::transaction::{
    read_consistent_from_state, validate_restore_post_image_blob_bytes,
};
use crate::{
    enforce_logical_key_budget, materialize_metric_event_source,
    store_internal::transaction::{
        apply_transaction, read_bounded_known_keys_from_parts, read_scoped_projection_from_parts,
        validate_immutable_read_session_capacity, validate_scoped_projection_post_image,
        BackendTransactionState, StoreAdmissionAuthority, StoreBackendUsage,
        StoreBoundedKnownBlobRead, StoreBoundedKnownJsonRead, StoreBoundedKnownKeyReadResult,
        StoreImmutableReadSession, StoreReadReceipt, StoreReadSessionState,
        StoreTransactionAdmission, StoreTransactionContext,
    },
    MemoryStoreEvent, StoreCapacityBudget, StoreEngine, StoreEventLog, StoreMetricEventSourceRead,
    StoreTransactionReport, StoreTransactionRequest,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{
    StoreConsistentReadRequest, StoreConsistentReadResult, StoreSnapshotBlob, StoreSnapshotJsonDoc,
    StoreSnapshotReplaceReport,
};

pub struct EmbeddedStoreEngine {
    capacity: StoreCapacityBudget,
    admission_authority: StoreAdmissionAuthority,
    state: Mutex<EmbeddedStoreState>,
}

#[derive(Default)]
struct EmbeddedStoreState {
    json: BTreeMap<(String, String), Value>,
    blobs: BTreeMap<(String, String), Vec<u8>>,
    events: Vec<MemoryStoreEvent>,
    event_ids: BTreeSet<String>,
}

struct EmbeddedImmutableReadSession<'a> {
    state: MutexGuard<'a, EmbeddedStoreState>,
    read: StoreReadSessionState,
}

impl StoreImmutableReadSession for EmbeddedImmutableReadSession<'_> {
    fn read_json_known_keys(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreBoundedKnownJsonRead>> {
        addresses
            .iter()
            .map(|(namespace, key)| {
                self.read.record_json(
                    namespace,
                    key,
                    self.state
                        .json
                        .get(&(namespace.clone(), key.clone()))
                        .cloned(),
                )
            })
            .collect()
    }

    fn read_blob_known_keys(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreBoundedKnownBlobRead>> {
        addresses
            .iter()
            .map(|(namespace, key)| {
                self.read.record_blob(
                    namespace,
                    key,
                    self.state
                        .blobs
                        .get(&(namespace.clone(), key.clone()))
                        .cloned(),
                )
            })
            .collect()
    }

    fn receipt(&self) -> Result<StoreReadReceipt> {
        self.read.receipt()
    }
}

impl EmbeddedStoreEngine {
    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn new(capacity: StoreCapacityBudget) -> Self {
        Self::new_with_admission_authority(capacity, StoreAdmissionAuthority::new())
    }

    pub(crate) fn new_with_admission_authority(
        capacity: StoreCapacityBudget,
        admission_authority: StoreAdmissionAuthority,
    ) -> Self {
        Self {
            capacity,
            admission_authority,
            state: Mutex::new(EmbeddedStoreState::default()),
        }
    }
}

fn apply_embedded_transaction_plan(
    state: &mut EmbeddedStoreState,
    mutations: &[crate::StoreEngineMutation],
) -> Result<()> {
    for mutation in mutations {
        match mutation {
            crate::StoreEngineMutation::PutJson {
                namespace,
                key,
                value,
            } => {
                state
                    .json
                    .insert((namespace.clone(), key.clone()), value.clone());
            }
            crate::StoreEngineMutation::DeleteJson { namespace, key } => {
                state.json.remove(&(namespace.clone(), key.clone()));
            }
            crate::StoreEngineMutation::PutBlob {
                namespace,
                key,
                value,
            } => {
                state
                    .blobs
                    .insert((namespace.clone(), key.clone()), value.clone());
            }
            crate::StoreEngineMutation::DeleteBlob { namespace, key } => {
                state.blobs.remove(&(namespace.clone(), key.clone()));
            }
            crate::StoreEngineMutation::AppendEvent { event } => {
                state.event_ids.insert(event.event_id.clone());
                state.events.push((**event).clone());
            }
            crate::StoreEngineMutation::DeleteJsonIfPresent { .. }
            | crate::StoreEngineMutation::DeleteBlobIfPresent { .. } => {
                return Err(Error::config(
                    "memory_write_transaction",
                    "conditional mutation reached embedded primitive execution",
                ));
            }
        }
    }
    Ok(())
}

impl StoreEventLog for EmbeddedStoreEngine {
    #[cfg(feature = "nonproduction-replay-harness")]
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        enforce_event_key_budget(self.capacity, &event, "embedded_store_event")?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.event_ids.insert(event.event_id.clone()) {
            return Err(Error::config(
                "store_event_log",
                format!("duplicate event id {}", event.event_id),
            ));
        }
        state.events.push(event);
        while state.events.len() > self.capacity.event_log_max_items {
            if let Some(removed_id) = state.events.first().map(|event| event.event_id.clone()) {
                state.event_ids.remove(&removed_id);
            }
            state.events.remove(0);
        }
        Ok(())
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .events
            .clone())
    }
}

impl StoreEngine for EmbeddedStoreEngine {
    fn admission_authority(&self) -> &StoreAdmissionAuthority {
        &self.admission_authority
    }

    fn read_metric_events(
        &self,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreMetricEventSourceRead> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        materialize_metric_event_source(&state.events, capacity)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn store_capacity(&self) -> StoreCapacityBudget {
        self.capacity
    }

    fn commit_transaction_admitted(
        &self,
        request: &StoreTransactionRequest,
        admission: &StoreTransactionAdmission,
    ) -> Result<StoreTransactionReport> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        admission.validate_inside_engine_fence(self.capacity, &self.admission_authority)?;
        let mut touched = BackendTransactionState {
            json: request
                .read_set()
                .json
                .iter()
                .filter_map(|address| {
                    state
                        .json
                        .get(address)
                        .cloned()
                        .map(|value| (address.clone(), value))
                })
                .collect(),
            blobs: request
                .read_set()
                .blobs
                .iter()
                .filter_map(|address| {
                    state
                        .blobs
                        .get(address)
                        .cloned()
                        .map(|value| (address.clone(), value))
                })
                .collect(),
            events: Vec::new(),
        };
        for (namespace, prefix) in &request.read_set().json_prefixes {
            touched.json.extend(
                state
                    .json
                    .iter()
                    .filter(|((candidate_namespace, key), _)| {
                        candidate_namespace == namespace && key.starts_with(prefix)
                    })
                    .map(|(address, value)| (address.clone(), value.clone())),
            );
        }
        let existing_event_ids = request
            .mutations
            .iter()
            .filter_map(crate::store_internal::transaction::mutation_event_id)
            .filter(|event_id| state.event_ids.contains(*event_id))
            .map(str::to_string)
            .collect();
        let plan = apply_transaction(
            admission,
            request,
            &StoreTransactionContext {
                touched,
                usage: StoreBackendUsage {
                    kv_entries: state.json.len().saturating_add(state.blobs.len()),
                    blob_bytes: state.blobs.values().map(Vec::len).sum(),
                    event_count: state.events.len(),
                },
                existing_event_ids,
            },
        )?;
        apply_embedded_transaction_plan(&mut state, &plan.effective_request.mutations)?;
        Ok(plan.report)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn read_consistent(
        &self,
        request: &StoreConsistentReadRequest,
    ) -> Result<StoreConsistentReadResult> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        Ok(read_consistent_from_state(
            request,
            &BackendTransactionState {
                json: state.json.clone(),
                blobs: state.blobs.clone(),
                events: state.events.clone(),
            },
        ))
    }

    fn read_consistent_known_keys(
        &self,
        json_keys: &[(String, String)],
        blob_keys: &[(String, String)],
        include_events: bool,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreBoundedKnownKeyReadResult> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        read_bounded_known_keys_from_parts(
            json_keys,
            blob_keys,
            include_events,
            capacity,
            &state.json,
            &state.blobs,
            &state.events,
        )
    }

    fn open_immutable_read_session<'a>(
        &'a self,
        capacity: StoreCapacityBudget,
    ) -> Result<Box<dyn StoreImmutableReadSession + 'a>> {
        validate_immutable_read_session_capacity(self.capacity, capacity)?;
        Ok(Box::new(EmbeddedImmutableReadSession {
            state: self.state.lock().unwrap_or_else(|error| error.into_inner()),
            read: StoreReadSessionState::new(capacity),
        }))
    }

    fn read_scoped_projection(
        &self,
        request: &crate::StoreScopedProjectionRequest,
        capacity: StoreCapacityBudget,
    ) -> Result<crate::StoreScopedProjection> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        read_scoped_projection_from_parts(request, capacity, &state.json, &state.events)
    }

    fn replace_scoped_projection(
        &self,
        request: &crate::StoreScopedProjectionReplaceRequest,
        admission: &StoreTransactionAdmission,
    ) -> Result<crate::StoreScopedProjectionReplaceReport> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        admission.validate_inside_engine_fence(self.capacity, &self.admission_authority)?;
        let deleted_json = crate::store_internal::transaction::scoped_projection_json_addresses(
            &request.json_namespaces,
            &state.json,
            &request.scope,
            admission.operation_capacity(),
        )?;
        let deleted_events = state
            .events
            .iter()
            .filter(|event| {
                crate::store_internal::transaction::event_matches_scoped_projection(
                    event,
                    &request.scope,
                )
            })
            .count();
        let next_entries = state
            .json
            .len()
            .saturating_add(state.blobs.len())
            .saturating_sub(deleted_json.len())
            .saturating_add(request.json_docs.len());
        let next_events = state
            .events
            .len()
            .saturating_sub(deleted_events)
            .saturating_add(request.events.len());
        for doc in &request.json_docs {
            let address = (doc.namespace.clone(), doc.key.clone());
            if state.json.contains_key(&address) && !deleted_json.contains(&address) {
                return Err(Error::config(
                    "store_scoped_projection",
                    format!(
                        "replacement address {}/{} is owned by another projection scope",
                        doc.namespace, doc.key
                    ),
                ));
            }
        }
        let retained_event_ids = state
            .events
            .iter()
            .filter(|event| {
                !crate::store_internal::transaction::event_matches_scoped_projection(
                    event,
                    &request.scope,
                )
            })
            .map(|event| event.event_id.as_str())
            .collect::<BTreeSet<_>>();
        if request
            .events
            .iter()
            .any(|event| retained_event_ids.contains(event.event_id.as_str()))
        {
            return Err(Error::config(
                "store_scoped_projection",
                "replacement would create a duplicate event id",
            ));
        }
        validate_scoped_projection_post_image(
            admission,
            request,
            next_entries,
            state.blobs.values().map(Vec::len),
            std::iter::empty(),
            next_events,
        )?;
        for address in &deleted_json {
            state.json.remove(address);
        }
        state.events.retain(|event| {
            !crate::store_internal::transaction::event_matches_scoped_projection(
                event,
                &request.scope,
            )
        });
        for doc in &request.json_docs {
            state
                .json
                .insert((doc.namespace.clone(), doc.key.clone()), doc.value.clone());
        }
        state.events.extend(request.events.iter().cloned());
        state.event_ids = state
            .events
            .iter()
            .map(|event| event.event_id.clone())
            .collect();
        Ok(crate::StoreScopedProjectionReplaceReport {
            admission_report_id: admission.report_id().to_string(),
            deleted_json: deleted_json.len(),
            inserted_json: request.json_docs.len(),
            deleted_events,
            inserted_events: request.events.len(),
        })
    }

    fn get_json_value(&self, namespace: &str, key: &str) -> Result<Option<Value>> {
        enforce_logical_key_budget(self.capacity, namespace, key, "embedded_store_json_read")?;
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .json
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_json_value(&self, namespace: &str, key: &str, value: Value) -> Result<()> {
        enforce_logical_key_budget(self.capacity, namespace, key, "embedded_store_json_write")?;
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

    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_json_value(&self, namespace: &str, key: &str) -> Result<bool> {
        enforce_logical_key_budget(self.capacity, namespace, key, "embedded_store_json_delete")?;
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .json
            .remove(&(namespace.to_string(), key.to_string()))
            .is_some())
    }

    fn list_json_keys(&self, namespace: &str) -> Result<Vec<String>> {
        enforce_logical_key_budget(self.capacity, namespace, "", "embedded_store_json_list")?;
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
        enforce_logical_key_budget(self.capacity, namespace, key, "embedded_store_blob_read")?;
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blobs
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn put_blob(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        enforce_logical_key_budget(self.capacity, namespace, key, "embedded_store_blob_write")?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let current_bytes = state.blobs.values().map(Vec::len).sum::<usize>();
        let previous = state
            .blobs
            .get(&(namespace.to_string(), key.to_string()))
            .map(Vec::len)
            .unwrap_or(0);
        let next_bytes = current_bytes
            .saturating_sub(previous)
            .saturating_add(value.len());
        if next_bytes > self.capacity.blob_max_bytes {
            return Err(store_budget_error(format!(
                "blob bytes {} exceed {}",
                next_bytes, self.capacity.blob_max_bytes
            )));
        }
        state
            .blobs
            .insert((namespace.to_string(), key.to_string()), value.to_vec());
        Ok(())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn delete_blob(&self, namespace: &str, key: &str) -> Result<bool> {
        enforce_logical_key_budget(self.capacity, namespace, key, "embedded_store_blob_delete")?;
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blobs
            .remove(&(namespace.to_string(), key.to_string()))
            .is_some())
    }

    fn list_blob_keys(&self, namespace: &str) -> Result<Vec<String>> {
        enforce_logical_key_budget(self.capacity, namespace, "", "embedded_store_blob_list")?;
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

    #[cfg(feature = "nonproduction-replay-harness")]
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
                "event lineage items {} exceed {}",
                events.len(),
                self.capacity.event_log_max_items
            )));
        }
        let mut event_ids = BTreeSet::new();
        for event in events {
            enforce_event_key_budget(self.capacity, event, "embedded_store_snapshot_import")?;
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
                    "embedded_store_snapshot_import",
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
                    "embedded_store_snapshot_import",
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
        validate_restore_post_image_blob_bytes(
            self.capacity,
            state
                .blobs
                .iter()
                .filter(|((namespace, _key), _value)| {
                    !blob_namespace_set.contains(namespace.as_str())
                })
                .map(|(_key, value)| value.len()),
            blobs.iter().map(|blob| blob.value.len()),
        )?;

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

#[cfg(feature = "nonproduction-replay-harness")]
fn namespace_set(namespaces: &[&str]) -> BTreeSet<String> {
    namespaces
        .iter()
        .map(|namespace| (*namespace).to_string())
        .collect()
}
