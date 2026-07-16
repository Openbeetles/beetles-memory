use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use bm_core::memory::{
    governed_evidence_source_ref_from_document, memory_facet_manifest_key,
    memory_graph_scope_manifest_key, scoped_long_term_memory_storage_key,
    scoped_memory_facet_owner_storage_key, validate_governed_evidence_document,
    validate_governed_evidence_source_ref, GovernedEvidenceDocument, GovernedEvidenceSourceRef,
    GovernedMemoryOwnerPlane, MemoryFacetIndexDoc, MemoryFacetIndexManifest,
    MemoryGraphScopeManifest, RelationshipPortfolio, RelationshipTopology,
};
use bm_core::{Error, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::store_internal::recall_index::{
    ArchiveRecallManifest, ConversationRecallManifest, TypedRecallIndex,
    ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE, ARCHIVE_RECALL_MANIFEST_NAMESPACE,
    CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE, CONVERSATION_RECALL_MANIFEST_NAMESPACE,
    RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE, TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
};
use crate::store_internal::schema::{
    control_plane_scope_manifest_key, governed_evidence_source_claim_manifest_key,
    recall_owner_scope_binding_key, ControlPlaneScopeManifest, GovernedEvidenceOwnerClaimBinding,
    GovernedEvidenceSourceClaimManifest, RecallOwnerScopeBinding,
    CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE, GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
    RECALL_OWNER_SCOPE_BINDING_NAMESPACE,
};
use crate::store_internal::{
    GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE, GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE,
};
use crate::{
    enforce_event_key_budget, enforce_logical_key_budget, store_budget_error, MemoryStoreEvent,
    MemoryStoreEventKind, StoreCapacityBudget, StoreEventScope, StoreJsonPrecondition,
    StoreMutationBatch, StoreMutationBudgetReport, StoreSnapshotJsonDoc,
};

#[derive(Clone)]
pub struct StoreTransactionAdmission {
    authority: StoreAdmissionAuthority,
    runtime_budget_authority: Option<Arc<bm_core::budget::RuntimeBudgetAuthority>>,
    report_id: String,
    operation_capacity: StoreCapacityBudget,
    resource_snapshot: Option<bm_core::resource::RuntimeResourceSnapshot>,
}

#[derive(Clone, Debug)]
pub struct StoreAdmissionAuthority(Arc<()>);

impl StoreAdmissionAuthority {
    pub(crate) fn new() -> Self {
        Self(Arc::new(()))
    }

    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl PartialEq for StoreAdmissionAuthority {
    fn eq(&self, other: &Self) -> bool {
        self.matches(other)
    }
}

impl Eq for StoreAdmissionAuthority {}

impl StoreTransactionAdmission {
    pub(crate) fn from_runtime_budget(
        report: &bm_core::budget::RuntimeBudgetReport,
        runtime_budget_authority: Arc<bm_core::budget::RuntimeBudgetAuthority>,
        authority: &StoreAdmissionAuthority,
    ) -> Result<Self> {
        report.validate_for_admission(current_admission_unix_secs())?;
        let report_id = report.report_id.clone();
        if report_id.trim().is_empty() {
            return Err(Error::config(
                "memory_write_transaction_resource_admission",
                "runtime budget admission report id is required",
            ));
        }
        Ok(Self {
            authority: authority.clone(),
            runtime_budget_authority: Some(runtime_budget_authority),
            report_id,
            operation_capacity: StoreCapacityBudget::from_runtime_budget(report.store_budget),
            resource_snapshot: Some(report.resource_snapshot.clone()),
        })
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    pub(crate) fn for_nonproduction_harness(
        operation_capacity: StoreCapacityBudget,
        authority: &StoreAdmissionAuthority,
    ) -> Self {
        Self {
            authority: authority.clone(),
            runtime_budget_authority: None,
            report_id: "nonproduction-replay-harness".to_string(),
            operation_capacity,
            resource_snapshot: None,
        }
    }

    pub fn report_id(&self) -> &str {
        &self.report_id
    }

    pub const fn operation_capacity(&self) -> StoreCapacityBudget {
        self.operation_capacity
    }

    pub(crate) fn validate_inside_engine_fence(
        &self,
        engine_capacity: StoreCapacityBudget,
        authority: &StoreAdmissionAuthority,
    ) -> Result<()> {
        if !self.authority.matches(authority) {
            return Err(Error::config(
                "memory_write_transaction_resource_admission",
                "transaction admission was issued by a different store authority",
            ));
        }
        let now_secs = current_admission_unix_secs();
        if self
            .resource_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.stale || snapshot.is_expired(now_secs))
        {
            return Err(Error::config(
                "memory_write_transaction_resource_admission",
                "transaction admission resource snapshot is stale or expired",
            ));
        }
        if let Some(runtime_budget_authority) = &self.runtime_budget_authority {
            let current = runtime_budget_authority.current_report(now_secs);
            current.validate_for_admission(now_secs)?;
            if current.report_id != self.report_id
                || StoreCapacityBudget::from_runtime_budget(current.store_budget)
                    != self.operation_capacity
            {
                return Err(Error::config(
                    "memory_write_transaction_resource_admission",
                    "transaction admission is not the current exact runtime authority report",
                ));
            }
        }
        if !engine_capacity.admits_runtime_budget(self.operation_capacity.into_runtime_budget()) {
            return Err(Error::config(
                "memory_write_transaction_resource_admission",
                "transaction admission exceeds the backend open capacity",
            ));
        }
        Ok(())
    }
}

fn current_admission_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StoreTransactionReadSet {
    pub(crate) json: BTreeSet<(String, String)>,
    pub(crate) json_prefixes: BTreeSet<(String, String)>,
    pub(crate) blobs: BTreeSet<(String, String)>,
}

impl StoreTransactionReadSet {
    fn from_request_parts(
        preconditions: &[StoreJsonPrecondition],
        mutations: &[StoreEngineMutation],
    ) -> Self {
        let mut read_set = Self::default();
        for precondition in preconditions {
            let (namespace, key) = match precondition {
                StoreJsonPrecondition::Absent { namespace, key }
                | StoreJsonPrecondition::Exact { namespace, key, .. } => (namespace, key),
            };
            read_set.json.insert((namespace.clone(), key.clone()));
        }
        for mutation in mutations {
            match mutation {
                StoreEngineMutation::PutJson { namespace, key, .. }
                | StoreEngineMutation::DeleteJson { namespace, key }
                | StoreEngineMutation::DeleteJsonIfPresent { namespace, key, .. } => {
                    read_set.json.insert((namespace.clone(), key.clone()));
                }
                StoreEngineMutation::PutBlob { namespace, key, .. }
                | StoreEngineMutation::DeleteBlob { namespace, key }
                | StoreEngineMutation::DeleteBlobIfPresent { namespace, key, .. } => {
                    read_set.blobs.insert((namespace.clone(), key.clone()));
                }
                StoreEngineMutation::AppendEvent { .. } => {}
            }
        }
        read_set
    }
}

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
pub struct ConditionalDeleteEventTemplate {
    event_id: String,
    kind: MemoryStoreEventKind,
    scope: StoreEventScope,
    plane: String,
    record_key: String,
    timestamp_unix_secs: u64,
    payload: BTreeMap<String, String>,
}

impl ConditionalDeleteEventTemplate {
    pub fn new(
        event_id: impl Into<String>,
        kind: MemoryStoreEventKind,
        scope: StoreEventScope,
        timestamp_unix_secs: u64,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            kind,
            scope,
            plane: String::new(),
            record_key: String::new(),
            timestamp_unix_secs,
            payload: BTreeMap::new(),
        }
    }

    pub fn with_plane(mut self, plane: impl Into<String>) -> Self {
        self.plane = plane.into();
        self
    }

    pub fn with_record_key(mut self, record_key: impl Into<String>) -> Self {
        self.record_key = record_key.into();
        self
    }

    pub fn with_payload(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.payload.insert(key.into(), value.into());
        self
    }

    pub(crate) fn event_id(&self) -> &str {
        &self.event_id
    }

    pub(crate) fn materialize_json(&self, before_image: &Value) -> Result<MemoryStoreEvent> {
        let before_image_bytes = serde_json::to_vec(before_image)
            .map_err(|error| Error::config("memory_write_transaction", error.to_string()))?;
        Ok(self.materialize_bytes(&before_image_bytes))
    }

    pub(crate) fn materialize_blob(&self, before_image: &[u8]) -> MemoryStoreEvent {
        self.materialize_bytes(before_image)
    }

    fn materialize_bytes(&self, before_image_bytes: &[u8]) -> MemoryStoreEvent {
        let mut event = MemoryStoreEvent::new(
            self.event_id.clone(),
            self.kind.clone(),
            self.scope.clone(),
            self.timestamp_unix_secs,
        )
        .with_plane(self.plane.clone())
        .with_record_key(self.record_key.clone())
        .with_content_hash(content_hash_from_bytes(before_image_bytes));
        event.payload = self.payload.clone();
        event
    }
}

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
    DeleteJsonIfPresent {
        namespace: String,
        key: String,
        event_template: Box<ConditionalDeleteEventTemplate>,
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
    DeleteBlobIfPresent {
        namespace: String,
        key: String,
        event_template: Box<ConditionalDeleteEventTemplate>,
    },
    AppendEvent {
        event: Box<MemoryStoreEvent>,
    },
}

impl StoreEngineMutation {
    pub fn conditional_delete_event_template(
        event_id: impl Into<String>,
        kind: MemoryStoreEventKind,
        scope: StoreEventScope,
        timestamp_unix_secs: u64,
    ) -> ConditionalDeleteEventTemplate {
        ConditionalDeleteEventTemplate::new(event_id, kind, scope, timestamp_unix_secs)
    }

    pub fn delete_json_if_present(
        namespace: impl Into<String>,
        key: impl Into<String>,
        event_template: ConditionalDeleteEventTemplate,
    ) -> Self {
        Self::DeleteJsonIfPresent {
            namespace: namespace.into(),
            key: key.into(),
            event_template: Box::new(event_template),
        }
    }

    pub fn delete_blob_if_present(
        namespace: impl Into<String>,
        key: impl Into<String>,
        event_template: ConditionalDeleteEventTemplate,
    ) -> Self {
        Self::DeleteBlobIfPresent {
            namespace: namespace.into(),
            key: key.into(),
            event_template: Box::new(event_template),
        }
    }
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
    read_set: StoreTransactionReadSet,
}

impl StoreTransactionRequest {
    pub fn new(
        transaction_id: impl Into<String>,
        preconditions: Vec<StoreJsonPrecondition>,
        mutations: Vec<StoreEngineMutation>,
        governed_batch: Option<Box<StoreMutationBatch>>,
    ) -> Self {
        let read_set = StoreTransactionReadSet::from_request_parts(&preconditions, &mutations);
        Self {
            transaction_id: transaction_id.into(),
            preconditions,
            mutations,
            governed_batch,
            graph_repair_authority: None,
            read_set,
        }
    }

    pub(crate) fn authorize_graph_repair(mut self, authority: GraphRepairAuthority) -> Self {
        self.graph_repair_authority = Some(authority);
        self
    }

    pub(crate) fn include_governed_json_reads(
        mut self,
        addresses: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        self.read_set.json.extend(addresses);
        self
    }

    pub(crate) fn include_governed_json_prefix_reads(
        mut self,
        prefixes: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        self.read_set.json_prefixes.extend(prefixes);
        self
    }

    fn graph_repair_authorized(&self) -> bool {
        self.graph_repair_authority.is_some()
    }

    pub(crate) fn read_set(&self) -> &StoreTransactionReadSet {
        &self.read_set
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

#[derive(Clone, Debug, PartialEq)]
pub struct StoreBoundedKnownJsonRead {
    pub namespace: String,
    pub key: String,
    pub value: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreBoundedKnownBlobRead {
    pub namespace: String,
    pub key: String,
    pub value: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoreBoundedKnownKeyReadResult {
    pub json: Vec<StoreBoundedKnownJsonRead>,
    pub blobs: Vec<StoreBoundedKnownBlobRead>,
    pub events: Vec<MemoryStoreEvent>,
    pub receipt: StoreReadReceipt,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreReadReceipt {
    pub state_digest: String,
    pub json_doc_count: usize,
    pub blob_count: usize,
    pub event_count: usize,
    pub entry_count: usize,
    pub json_bytes: usize,
    pub blob_bytes: usize,
}

pub trait StoreImmutableReadSession {
    fn read_json_known_keys(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreBoundedKnownJsonRead>>;
    fn read_blob_known_keys(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreBoundedKnownBlobRead>>;
    fn receipt(&self) -> Result<StoreReadReceipt>;
}

pub(crate) struct StoreReadSessionState {
    capacity: StoreCapacityBudget,
    json_reads: Vec<StoreBoundedKnownJsonRead>,
    blob_reads: Vec<StoreBoundedKnownBlobRead>,
    seen_json: BTreeSet<(String, String)>,
    seen_blobs: BTreeSet<(String, String)>,
    json_bytes: usize,
    blob_bytes: usize,
}

impl StoreReadSessionState {
    pub(crate) fn new(capacity: StoreCapacityBudget) -> Self {
        Self {
            capacity,
            json_reads: Vec::new(),
            blob_reads: Vec::new(),
            seen_json: BTreeSet::new(),
            seen_blobs: BTreeSet::new(),
            json_bytes: 0,
            blob_bytes: 0,
        }
    }

    pub(crate) fn record_json(
        &mut self,
        namespace: &str,
        key: &str,
        value: Option<Value>,
    ) -> Result<StoreBoundedKnownJsonRead> {
        enforce_logical_key_budget(
            self.capacity,
            namespace,
            key,
            "store_immutable_read_session",
        )?;
        if !self
            .seen_json
            .insert((namespace.to_string(), key.to_string()))
        {
            return Err(Error::config(
                "store_immutable_read_session",
                format!("duplicate JSON address {namespace}/{key} across one read session"),
            ));
        }
        self.enforce_entry_ceiling()?;
        if let Some(value) = value.as_ref() {
            self.json_bytes = self
                .json_bytes
                .checked_add(serialized_json_len(value)?)
                .ok_or_else(|| {
                    Error::config("store_immutable_read_session", "JSON byte count overflow")
                })?;
            if self.json_bytes > self.capacity.snapshot_max_bytes {
                return Err(Error::config(
                    "store_consistent_read_budget_exceeded",
                    format!(
                        "JSON bytes {} exceed {}",
                        self.json_bytes, self.capacity.snapshot_max_bytes
                    ),
                ));
            }
        }
        let read = StoreBoundedKnownJsonRead {
            namespace: namespace.to_string(),
            key: key.to_string(),
            value,
        };
        self.json_reads.push(read.clone());
        Ok(read)
    }

    pub(crate) fn record_blob(
        &mut self,
        namespace: &str,
        key: &str,
        value: Option<Vec<u8>>,
    ) -> Result<StoreBoundedKnownBlobRead> {
        enforce_logical_key_budget(
            self.capacity,
            namespace,
            key,
            "store_immutable_read_session",
        )?;
        if !self
            .seen_blobs
            .insert((namespace.to_string(), key.to_string()))
        {
            return Err(Error::config(
                "store_immutable_read_session",
                format!("duplicate blob address {namespace}/{key} across one read session"),
            ));
        }
        self.enforce_entry_ceiling()?;
        if let Some(value) = value.as_ref() {
            self.blob_bytes = self.blob_bytes.checked_add(value.len()).ok_or_else(|| {
                Error::config("store_immutable_read_session", "blob byte count overflow")
            })?;
            if self.blob_bytes > self.capacity.blob_max_bytes {
                return Err(Error::config(
                    "store_consistent_read_budget_exceeded",
                    format!(
                        "blob bytes {} exceed {}",
                        self.blob_bytes, self.capacity.blob_max_bytes
                    ),
                ));
            }
        }
        let read = StoreBoundedKnownBlobRead {
            namespace: namespace.to_string(),
            key: key.to_string(),
            value,
        };
        self.blob_reads.push(read.clone());
        Ok(read)
    }

    pub(crate) fn receipt(&self) -> Result<StoreReadReceipt> {
        immutable_read_session_receipt(
            &self.json_reads,
            &self.blob_reads,
            self.json_bytes,
            self.blob_bytes,
        )
    }

    pub(crate) fn remaining_json_bytes(&self) -> usize {
        self.capacity
            .snapshot_max_bytes
            .saturating_sub(self.json_bytes)
    }

    pub(crate) fn remaining_blob_bytes(&self) -> usize {
        self.capacity.blob_max_bytes.saturating_sub(self.blob_bytes)
    }

    fn enforce_entry_ceiling(&self) -> Result<()> {
        let entries = self.seen_json.len().saturating_add(self.seen_blobs.len());
        if entries > self.capacity.kv_max_entries {
            return Err(Error::config(
                "store_consistent_read_budget_exceeded",
                format!(
                    "requested entries {entries} exceed {}",
                    self.capacity.kv_max_entries
                ),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreGovernedEvidenceExactReadRequest {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub owner_keys: Vec<String>,
    pub include_all_manifest_bindings: bool,
    pub allow_missing_manifest_for_empty_scope: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreGovernedEvidenceOwnerClaimRead {
    pub owner_key: String,
    pub owner: Option<Value>,
    pub claim_key: Option<String>,
    pub claim: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreGovernedEvidenceScopeIndexReceipt {
    pub owner_count: usize,
    pub claim_count: usize,
    pub owner_keys_digest: String,
    pub claim_keys_digest: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreGovernedEvidenceExactReadResult {
    pub manifest: Option<GovernedEvidenceSourceClaimManifest>,
    pub reads: Vec<StoreGovernedEvidenceOwnerClaimRead>,
    pub scope_index: StoreGovernedEvidenceScopeIndexReceipt,
    pub entry_count: usize,
    pub json_bytes: usize,
    pub blob_bytes: usize,
    pub receipt: StoreReadReceipt,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StoreBackendUsage {
    pub(crate) kv_entries: usize,
    pub(crate) blob_bytes: usize,
    pub(crate) event_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct StoreTransactionContext {
    pub(crate) touched: BackendTransactionState,
    pub(crate) usage: StoreBackendUsage,
    pub(crate) existing_event_ids: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct StoreTransactionPlan {
    pub(crate) effective_request: StoreTransactionRequest,
    pub(crate) next_touched: BackendTransactionState,
    pub(crate) report: StoreTransactionReport,
}

pub(crate) fn apply_transaction(
    admission: &StoreTransactionAdmission,
    request: &StoreTransactionRequest,
    context: &StoreTransactionContext,
) -> Result<StoreTransactionPlan> {
    if request.transaction_id.trim().is_empty() {
        return Err(Error::config(
            "memory_write_transaction_preflight_failed",
            "transaction_id is required",
        ));
    }
    let capacity = admission.operation_capacity();
    let current = &context.touched;
    let current_json_bytes = current.json.values().try_fold(0_usize, |total, value| {
        total
            .checked_add(serialized_json_len(value)?)
            .ok_or_else(|| store_budget_error("transaction JSON read-set byte count overflow"))
    })?;
    if current.json.len() > capacity.kv_max_entries
        || current_json_bytes > capacity.snapshot_max_bytes
    {
        return Err(store_budget_error(
            "transaction JSON read-set exceeds the pinned operation budget",
        ));
    }
    let effective = materialize_conditional_mutations(request, current)?;
    validate_preconditions(capacity, &effective.preconditions, &current.json)?;

    let mut next = current.clone();
    let mut changed_json = BTreeSet::new();
    let mut changed_blobs = BTreeSet::new();
    let mut event_ids = Vec::new();
    let mut known_event_ids = context.existing_event_ids.clone();
    let mut mutated_json = BTreeSet::new();
    let mut mutated_blobs = BTreeSet::new();

    for mutation in &effective.mutations {
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
            StoreEngineMutation::DeleteJsonIfPresent { .. }
            | StoreEngineMutation::DeleteBlobIfPresent { .. } => {
                return Err(Error::config(
                    "memory_write_transaction_preflight_failed",
                    "conditional mutation reached the primitive transaction executor",
                ));
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

    if let Some(batch) = effective.governed_batch.as_deref() {
        crate::store_internal::platform::validate_governed_transaction_post_image(
            batch,
            current,
            &next,
            request.graph_repair_authorized(),
        )?;
    }

    let current_touched_entries = current.json.len().saturating_add(current.blobs.len());
    let next_touched_entries = next.json.len().saturating_add(next.blobs.len());
    let kv_entries = context
        .usage
        .kv_entries
        .saturating_sub(current_touched_entries)
        .saturating_add(next_touched_entries);
    if kv_entries > capacity.kv_max_entries {
        return Err(store_budget_error(format!(
            "kv entries {} exceed {}",
            kv_entries, capacity.kv_max_entries
        )));
    }
    let current_touched_blob_bytes = current.blobs.values().map(Vec::len).sum::<usize>();
    let next_touched_blob_bytes = next.blobs.values().map(Vec::len).sum::<usize>();
    let blob_bytes = context
        .usage
        .blob_bytes
        .saturating_sub(current_touched_blob_bytes)
        .saturating_add(next_touched_blob_bytes);
    if blob_bytes > capacity.blob_max_bytes {
        return Err(store_budget_error(format!(
            "blob bytes {} exceed {}",
            blob_bytes, capacity.blob_max_bytes
        )));
    }
    let next_event_count = context.usage.event_count.saturating_add(event_ids.len());
    if next_event_count > capacity.event_log_max_items {
        return Err(store_budget_error(format!(
            "event log items {} exceed {}",
            next_event_count, capacity.event_log_max_items
        )));
    }

    let budget_report = backend_budget_report(
        admission,
        context.usage,
        kv_entries,
        blob_bytes,
        next_event_count,
    );
    Ok(StoreTransactionPlan {
        effective_request: effective,
        next_touched: next,
        report: StoreTransactionReport {
            transaction_id: request.transaction_id.clone(),
            changed_json: changed_json.len(),
            changed_blobs: changed_blobs.len(),
            appended_events: event_ids.len(),
            event_ids,
            budget_report,
        },
    })
}

#[cfg(feature = "nonproduction-replay-harness")]
pub(crate) fn validate_restore_post_image_blob_bytes(
    capacity: StoreCapacityBudget,
    retained_blob_lengths: impl IntoIterator<Item = usize>,
    replacement_blob_lengths: impl IntoIterator<Item = usize>,
) -> Result<usize> {
    let final_blob_bytes = retained_blob_lengths
        .into_iter()
        .chain(replacement_blob_lengths)
        .try_fold(0_usize, |total, len| {
            total
                .checked_add(len)
                .ok_or_else(|| store_budget_error("restore post-image blob byte count overflow"))
        })?;
    if final_blob_bytes > capacity.blob_max_bytes {
        return Err(store_budget_error(format!(
            "restore post-image blob bytes {} exceed {}",
            final_blob_bytes, capacity.blob_max_bytes
        )));
    }
    Ok(final_blob_bytes)
}

fn materialize_conditional_mutations(
    request: &StoreTransactionRequest,
    current: &BackendTransactionState,
) -> Result<StoreTransactionRequest> {
    let mut effective = request.clone();
    effective.mutations.clear();
    for mutation in &request.mutations {
        match mutation {
            StoreEngineMutation::DeleteJsonIfPresent {
                namespace,
                key,
                event_template,
            } => {
                let Some(before_image) = current.json.get(&(namespace.clone(), key.clone())) else {
                    continue;
                };
                effective.mutations.push(StoreEngineMutation::DeleteJson {
                    namespace: namespace.clone(),
                    key: key.clone(),
                });
                effective.mutations.push(StoreEngineMutation::AppendEvent {
                    event: Box::new(event_template.materialize_json(before_image)?),
                });
            }
            StoreEngineMutation::DeleteBlobIfPresent {
                namespace,
                key,
                event_template,
            } => {
                let Some(before_image) = current.blobs.get(&(namespace.clone(), key.clone()))
                else {
                    continue;
                };
                effective.mutations.push(StoreEngineMutation::DeleteBlob {
                    namespace: namespace.clone(),
                    key: key.clone(),
                });
                effective.mutations.push(StoreEngineMutation::AppendEvent {
                    event: Box::new(event_template.materialize_blob(before_image)),
                });
            }
            _ => effective.mutations.push(mutation.clone()),
        }
    }
    Ok(effective)
}

pub(crate) fn mutation_event_id(mutation: &StoreEngineMutation) -> Option<&str> {
    match mutation {
        StoreEngineMutation::AppendEvent { event } => Some(event.event_id.as_str()),
        StoreEngineMutation::DeleteJsonIfPresent { event_template, .. }
        | StoreEngineMutation::DeleteBlobIfPresent { event_template, .. } => {
            Some(event_template.event_id())
        }
        _ => None,
    }
}

fn content_hash_from_bytes(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn backend_budget_report(
    admission: &StoreTransactionAdmission,
    current: StoreBackendUsage,
    next_kv_entries: usize,
    next_blob_bytes: usize,
    next_event_count: usize,
) -> StoreMutationBudgetReport {
    let capacity = admission.operation_capacity();
    StoreMutationBudgetReport {
        admission_report_id: admission.report_id().to_string(),
        required_events: next_event_count.saturating_sub(current.event_count),
        remaining_events: capacity
            .event_log_max_items
            .saturating_sub(next_event_count),
        required_kv_entries: next_kv_entries.saturating_sub(current.kv_entries),
        remaining_kv_entries: capacity.kv_max_entries.saturating_sub(next_kv_entries),
        required_blob_bytes: next_blob_bytes.saturating_sub(current.blob_bytes),
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

fn scoped_projection_receipt(
    json_docs: &[StoreSnapshotJsonDoc],
    events: &[MemoryStoreEvent],
) -> Result<StoreReadReceipt> {
    let mut hasher = Sha256::new();
    hasher.update(b"beetle_memory_scoped_projection_read_v1");
    for doc in json_docs {
        update_read_digest(&mut hasher, doc.namespace.as_bytes());
        update_read_digest(&mut hasher, doc.key.as_bytes());
        let value = serde_json::to_vec(&doc.value)
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        update_read_digest(&mut hasher, &value);
    }
    for event in events {
        let value = serde_json::to_vec(event)
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        update_read_digest(&mut hasher, &value);
    }
    Ok(StoreReadReceipt {
        state_digest: format!("{:x}", hasher.finalize()),
        json_doc_count: json_docs.len(),
        blob_count: 0,
        event_count: events.len(),
        entry_count: json_docs.len(),
        json_bytes: json_docs
            .iter()
            .map(|doc| serialized_json_len(&doc.value))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum(),
        blob_bytes: 0,
    })
}

pub(crate) fn read_bounded_known_keys_from_parts(
    json_keys: &[(String, String)],
    blob_keys: &[(String, String)],
    include_events: bool,
    capacity: StoreCapacityBudget,
    json: &BTreeMap<(String, String), Value>,
    blobs: &BTreeMap<(String, String), Vec<u8>>,
    all_events: &[MemoryStoreEvent],
) -> Result<StoreBoundedKnownKeyReadResult> {
    let requested_entries = json_keys.len().saturating_add(blob_keys.len());
    if requested_entries > capacity.kv_max_entries {
        return Err(Error::config(
            "store_consistent_read_budget_exceeded",
            format!(
                "requested entries {requested_entries} exceed {}",
                capacity.kv_max_entries
            ),
        ));
    }
    reject_duplicate_known_keys(json_keys, "json")?;
    reject_duplicate_known_keys(blob_keys, "blob")?;

    let mut json_bytes = 0usize;
    let mut json_reads = Vec::with_capacity(json_keys.len());
    for (namespace, key) in json_keys {
        enforce_logical_key_budget(capacity, namespace, key, "store_consistent_read")?;
        let value = json.get(&(namespace.clone(), key.clone()));
        if let Some(value) = value {
            json_bytes = json_bytes.saturating_add(serialized_json_len(value)?);
            if json_bytes > capacity.snapshot_max_bytes {
                return Err(Error::config(
                    "store_consistent_read_budget_exceeded",
                    format!(
                        "json bytes {json_bytes} exceed {}",
                        capacity.snapshot_max_bytes
                    ),
                ));
            }
        }
        json_reads.push(StoreBoundedKnownJsonRead {
            namespace: namespace.clone(),
            key: key.clone(),
            value: value.cloned(),
        });
    }

    let mut blob_bytes = 0usize;
    let mut blob_reads = Vec::with_capacity(blob_keys.len());
    for (namespace, key) in blob_keys {
        enforce_logical_key_budget(capacity, namespace, key, "store_consistent_read")?;
        let value = blobs.get(&(namespace.clone(), key.clone()));
        if let Some(value) = value {
            blob_bytes = blob_bytes.saturating_add(value.len());
            if blob_bytes > capacity.blob_max_bytes {
                return Err(Error::config(
                    "store_consistent_read_budget_exceeded",
                    format!("blob bytes {blob_bytes} exceed {}", capacity.blob_max_bytes),
                ));
            }
        }
        blob_reads.push(StoreBoundedKnownBlobRead {
            namespace: namespace.clone(),
            key: key.clone(),
            value: value.cloned(),
        });
    }

    let events = if include_events {
        if all_events.len() > capacity.event_log_max_items {
            return Err(Error::config(
                "store_consistent_read_budget_exceeded",
                format!(
                    "event items {} exceed {}",
                    all_events.len(),
                    capacity.event_log_max_items
                ),
            ));
        }
        let mut total_json_bytes = json_bytes;
        for event in all_events {
            total_json_bytes = total_json_bytes.saturating_add(serialized_len(event)?);
            if total_json_bytes > capacity.snapshot_max_bytes {
                return Err(Error::config(
                    "store_consistent_read_budget_exceeded",
                    format!(
                        "json and event bytes {total_json_bytes} exceed {}",
                        capacity.snapshot_max_bytes
                    ),
                ));
            }
        }
        all_events.to_vec()
    } else {
        Vec::new()
    };
    let receipt = known_key_read_receipt(&json_reads, &blob_reads, &events)?;
    Ok(StoreBoundedKnownKeyReadResult {
        json: json_reads,
        blobs: blob_reads,
        events,
        receipt,
    })
}

pub fn read_governed_evidence_exact_in_session(
    session: &mut dyn StoreImmutableReadSession,
    request: &StoreGovernedEvidenceExactReadRequest,
) -> Result<StoreGovernedEvidenceExactReadResult> {
    let stage = "governed_evidence_exact_read";
    if request.memory_space_id.trim().is_empty()
        || request.mounted_subject_id.trim().is_empty()
        || request.memory_space_id != request.memory_space_id.trim()
        || request.mounted_subject_id != request.mounted_subject_id.trim()
    {
        return Err(Error::config(stage, "exact read scope is not canonical"));
    }
    let mut requested_owner_keys = request.owner_keys.clone();
    if requested_owner_keys
        .iter()
        .any(|key| key.trim().is_empty() || key != key.trim())
    {
        return Err(Error::config(
            stage,
            "exact read owner key is not canonical",
        ));
    }
    requested_owner_keys.sort();
    if requested_owner_keys
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        return Err(Error::config(
            stage,
            "exact read owner keys contain duplicates",
        ));
    }

    let manifest_key = governed_evidence_source_claim_manifest_key(
        &request.memory_space_id,
        &request.mounted_subject_id,
    )?;
    let manifest_address = (
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
        manifest_key.clone(),
    );
    let manifest_read =
        read_json_known_keys_exact(session, std::slice::from_ref(&manifest_address), stage)?
            .into_iter()
            .next()
            .ok_or_else(|| Error::config(stage, "manifest address was not returned explicitly"))?;
    let manifest = manifest_read
        .value
        .as_ref()
        .map(|value| {
            serde_json::from_value::<GovernedEvidenceSourceClaimManifest>(value.clone())
                .map_err(|error| Error::config(stage, format!("manifest decode failed: {error}")))
        })
        .transpose()?;
    let Some(manifest) = manifest else {
        if request.allow_missing_manifest_for_empty_scope {
            let receipt = session.receipt()?;
            return Ok(StoreGovernedEvidenceExactReadResult {
                manifest: None,
                reads: requested_owner_keys
                    .into_iter()
                    .map(|owner_key| StoreGovernedEvidenceOwnerClaimRead {
                        owner_key,
                        owner: None,
                        claim_key: None,
                        claim: None,
                    })
                    .collect(),
                scope_index: StoreGovernedEvidenceScopeIndexReceipt::default(),
                entry_count: receipt.entry_count,
                json_bytes: receipt.json_bytes,
                blob_bytes: receipt.blob_bytes,
                receipt,
            });
        }
        return Err(Error::config(
            stage,
            "governed evidence manifest is missing",
        ));
    };
    manifest.validate_exact(
        &request.memory_space_id,
        &request.mounted_subject_id,
        manifest.owner_claim_bindings.clone(),
        usize::MAX,
    )?;
    let scope_index = StoreGovernedEvidenceScopeIndexReceipt {
        owner_count: manifest.owner_count,
        claim_count: manifest.claim_count,
        owner_keys_digest: manifest.owner_keys_digest.clone(),
        claim_keys_digest: manifest.claim_keys_digest.clone(),
    };
    if request.include_all_manifest_bindings {
        requested_owner_keys = manifest.owner_keys.clone();
    }

    let manifested_owner_keys = requested_owner_keys
        .iter()
        .filter(|owner_key| manifest.binding_for_owner(owner_key).is_some())
        .cloned()
        .collect::<Vec<_>>();
    let owner_addresses = manifested_owner_keys
        .iter()
        .map(|key| {
            (
                GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                key.clone(),
            )
        })
        .collect::<Vec<_>>();
    let owner_reads = read_json_known_keys_exact(session, &owner_addresses, stage)?;
    let owner_values = owner_reads
        .into_iter()
        .map(|read| (read.key, read.value))
        .collect::<BTreeMap<_, _>>();
    let claim_addresses = manifested_owner_keys
        .iter()
        .filter_map(|owner_key| manifest.binding_for_owner(owner_key))
        .map(|binding| {
            (
                GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                binding.claim_physical_key.clone(),
            )
        })
        .collect::<Vec<_>>();
    let claim_reads = read_json_known_keys_exact(session, &claim_addresses, stage)?;
    let claim_values = claim_reads
        .into_iter()
        .map(|read| (read.key, read.value))
        .collect::<BTreeMap<_, _>>();
    let mut reads = Vec::with_capacity(requested_owner_keys.len());
    for owner_key in requested_owner_keys {
        let Some(binding) = manifest.binding_for_owner(&owner_key) else {
            reads.push(StoreGovernedEvidenceOwnerClaimRead {
                owner_key,
                owner: None,
                claim_key: None,
                claim: None,
            });
            continue;
        };
        let owner_value = owner_values
            .get(&owner_key)
            .and_then(Option::as_ref)
            .ok_or_else(|| Error::config(stage, "manifested evidence owner is missing"))?;
        let claim_value = claim_values
            .get(&binding.claim_physical_key)
            .and_then(Option::as_ref)
            .ok_or_else(|| Error::config(stage, "manifested evidence claim is missing"))?;
        let owner = serde_json::from_value::<GovernedEvidenceDocument>(owner_value.clone())
            .map_err(|error| Error::config(stage, format!("owner decode failed: {error}")))?;
        let claim = serde_json::from_value::<GovernedEvidenceSourceRef>(claim_value.clone())
            .map_err(|error| Error::config(stage, format!("claim decode failed: {error}")))?;
        validate_governed_evidence_document(&owner)
            .map_err(|error| Error::config(stage, format!("owner invalid: {error:?}")))?;
        validate_governed_evidence_source_ref(&owner, &claim)
            .map_err(|error| Error::config(stage, format!("claim invalid: {error:?}")))?;
        let actual_binding =
            GovernedEvidenceOwnerClaimBinding::from_document_claim(&owner, &claim)?;
        if &actual_binding != binding
            || owner.physical_key != owner_key
            || governed_evidence_source_ref_from_document(&owner)?.physical_key
                != binding.claim_physical_key
        {
            return Err(Error::config(
                stage,
                "owner or claim revision/content binding does not match manifest",
            ));
        }
        reads.push(StoreGovernedEvidenceOwnerClaimRead {
            owner_key,
            owner: Some(owner_value.clone()),
            claim_key: Some(binding.claim_physical_key.clone()),
            claim: Some(claim_value.clone()),
        });
    }
    let receipt = session.receipt()?;
    Ok(StoreGovernedEvidenceExactReadResult {
        manifest: Some(manifest),
        reads,
        scope_index,
        entry_count: receipt.entry_count,
        json_bytes: receipt.json_bytes,
        blob_bytes: receipt.blob_bytes,
        receipt,
    })
}

fn read_json_known_keys_exact(
    session: &mut dyn StoreImmutableReadSession,
    addresses: &[(String, String)],
    stage: &'static str,
) -> Result<Vec<StoreBoundedKnownJsonRead>> {
    let reads = session.read_json_known_keys(addresses)?;
    if reads.len() != addresses.len()
        || reads
            .iter()
            .zip(addresses)
            .any(|(read, (namespace, key))| &read.namespace != namespace || &read.key != key)
    {
        return Err(Error::config(
            stage,
            "immutable read session returned an extra, missing, or wrong JSON address",
        ));
    }
    Ok(reads)
}

pub(crate) fn json_document_matches_scoped_projection(
    namespace: &str,
    key: &str,
    value: &Value,
    scope: &crate::StoreScopedProjectionScope,
) -> bool {
    match namespace {
        "long_term" => false,
        "self_model"
        | "self_authored_core"
        | "core_revision_ledger"
        | "self_continuity"
        | "relationship_portfolio"
        | "relationship_topology"
        | "autonomy_strategy"
        | "inner_life"
        | "felt_significance"
        | "temperament_continuity"
        | "inner_conflict"
        | "private_doc" => key == scope.mounted_subject_id,
        "conversation_transcript" => {
            value
                .get("key")
                .and_then(|key| key.get("memory_space_id"))
                .and_then(Value::as_str)
                == Some(scope.memory_space_id.as_str())
                && value.get("subject").and_then(Value::as_str)
                    == Some(scope.mounted_subject_id.as_str())
        }
        "archive_recall_manifests" | "conversation_recall_manifests" => {
            value.get("memory_space_id").and_then(Value::as_str)
                == Some(scope.memory_space_id.as_str())
                && value.get("mounted_subject_id").and_then(Value::as_str)
                    == Some(scope.mounted_subject_id.as_str())
        }
        CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE => {
            value.get("memory_space_id").and_then(Value::as_str)
                == Some(scope.memory_space_id.as_str())
                && value.get("mounted_subject_id").and_then(Value::as_str)
                    == Some(scope.mounted_subject_id.as_str())
                && control_plane_scope_manifest_key(
                    &scope.memory_space_id,
                    &scope.mounted_subject_id,
                )
                .is_ok_and(|expected| expected == key)
        }
        "memory_graph_nodes"
        | "memory_graph_edges"
        | "memory_graph_backlinks"
        | "memory_graph_indexes"
        | "memory_graph_revisions"
        | "memory_graph_manifests"
        | "memory_graph_node_memberships"
        | "memory_graph_edge_memberships"
        | "memory_graph_backlink_memberships"
        | "governed_evidence_documents"
        | "governed_evidence_source_refs"
        | "governed_evidence_source_claim_manifests" => {
            value.get("memory_space_id").and_then(Value::as_str)
                == Some(scope.memory_space_id.as_str())
                && value.get("mounted_subject_id").and_then(Value::as_str)
                    == Some(scope.mounted_subject_id.as_str())
        }
        "memory_facet_indexes" => {
            value.get("memory_space_id").and_then(Value::as_str)
                == Some(scope.memory_space_id.as_str())
                && value
                    .get("subject_ids")
                    .and_then(Value::as_array)
                    .is_some_and(|subjects| {
                        subjects.iter().any(|subject| {
                            subject.as_str() == Some(scope.mounted_subject_id.as_str())
                        })
                    })
        }
        "memory_facet_postings" => {
            value.get("memory_space_id").and_then(Value::as_str)
                == Some(scope.memory_space_id.as_str())
                && value.get("subject_id").and_then(Value::as_str)
                    == Some(scope.mounted_subject_id.as_str())
        }
        "conversation_transcript_alias"
        | "conversation_transcript_attr"
        | "conversation_transcript_derived_ref"
        | "turn_ledger"
        | "continuity_capsule"
        | "turn_continuity_evidence" => {
            value.get("memory_space_id").and_then(Value::as_str)
                == Some(scope.memory_space_id.as_str())
                && (value.get("mounted_subject_id").and_then(Value::as_str)
                    == Some(scope.mounted_subject_id.as_str())
                    || value.get("subject_id").and_then(Value::as_str)
                        == Some(scope.mounted_subject_id.as_str()))
        }
        _ => false,
    }
}

pub(crate) fn event_matches_scoped_projection(
    event: &MemoryStoreEvent,
    scope: &crate::StoreScopedProjectionScope,
) -> bool {
    event.scope.memory_space_id == scope.memory_space_id
        && event.scope.subject_id == scope.mounted_subject_id
}

pub(crate) fn read_scoped_projection_from_parts(
    request: &crate::StoreScopedProjectionRequest,
    capacity: StoreCapacityBudget,
    json: &BTreeMap<(String, String), Value>,
    events: &[MemoryStoreEvent],
) -> Result<crate::StoreScopedProjection> {
    let scoped_addresses =
        scoped_projection_json_addresses(&request.json_namespaces, json, &request.scope, capacity)?;
    let scoped_json = scoped_addresses
        .into_iter()
        .filter_map(|address| json.get(&address).cloned().map(|value| (address, value)))
        .collect::<BTreeMap<_, _>>();
    validate_scoped_recall_manifest_documents(&scoped_json, &BTreeMap::new(), &request.scope)?;
    validate_scoped_control_plane_documents(&scoped_json, &request.scope, capacity.kv_max_entries)?;
    let mut scoped_events = Vec::new();
    if request.include_events {
        for event in events
            .iter()
            .filter(|event| event_matches_scoped_projection(event, &request.scope))
        {
            if scoped_events.len() == capacity.event_log_max_items {
                return Err(Error::config(
                    "store_scoped_projection_budget_exceeded",
                    "scoped projection exceeds the pinned event budget",
                ));
            }
            scoped_events.push(event.clone());
        }
    }
    let json_docs = scoped_json
        .into_iter()
        .map(|((namespace, key), value)| StoreSnapshotJsonDoc {
            namespace,
            key,
            value,
        })
        .collect::<Vec<_>>();
    let json_bytes = json_docs.iter().try_fold(0_usize, |total, doc| {
        let bytes = serialized_json_len(&doc.value)?;
        total
            .checked_add(bytes)
            .ok_or_else(|| store_budget_error("scoped projection JSON byte count overflow"))
    })?;
    if json_docs.len() > capacity.kv_max_entries
        || scoped_events.len() > capacity.event_log_max_items
        || json_bytes > capacity.snapshot_max_bytes
    {
        return Err(Error::config(
            "store_scoped_projection_budget_exceeded",
            "scoped projection exceeds the pinned operation budget",
        ));
    }
    Ok(crate::StoreScopedProjection {
        scope: request.scope.clone(),
        receipt: scoped_projection_receipt(&json_docs, &scoped_events)?,
        json_docs,
        events: scoped_events,
    })
}

pub(crate) fn scoped_projection_json_addresses(
    json_namespaces: &[String],
    json: &BTreeMap<(String, String), Value>,
    scope: &crate::StoreScopedProjectionScope,
    capacity: StoreCapacityBudget,
) -> Result<BTreeSet<(String, String)>> {
    let namespaces = json_namespaces
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut scoped = BTreeMap::new();
    let mut scoped_bytes = 0_usize;
    for (address, value) in json.iter().filter(|((namespace, key), value)| {
        namespaces.contains(namespace.as_str())
            && json_document_matches_scoped_projection(namespace, key, value, scope)
    }) {
        admit_scoped_projection_value(&mut scoped_bytes, scoped.len(), value, capacity)?;
        scoped.insert(address.clone(), value.clone());
    }
    loop {
        let dependencies = scoped_projection_dependency_addresses(&scoped, json_namespaces, scope)?;
        let mut changed = false;
        for address in dependencies {
            if scoped.contains_key(&address) {
                continue;
            }
            if let Some(value) = json.get(&address) {
                admit_scoped_projection_value(&mut scoped_bytes, scoped.len(), value, capacity)?;
                scoped.insert(address, value.clone());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(scoped.into_keys().collect())
}

fn admit_scoped_projection_value(
    scoped_bytes: &mut usize,
    scoped_len: usize,
    value: &Value,
    capacity: StoreCapacityBudget,
) -> Result<()> {
    if scoped_len == capacity.kv_max_entries {
        return Err(Error::config(
            "store_scoped_projection_budget_exceeded",
            "scoped projection exceeds the pinned entry budget",
        ));
    }
    *scoped_bytes = (*scoped_bytes)
        .checked_add(serialized_json_len(value)?)
        .ok_or_else(|| store_budget_error("scoped projection JSON byte count overflow"))?;
    if *scoped_bytes > capacity.snapshot_max_bytes {
        return Err(Error::config(
            "store_scoped_projection_budget_exceeded",
            "scoped projection exceeds the pinned byte budget",
        ));
    }
    Ok(())
}

pub(crate) fn scoped_projection_dependency_addresses(
    scoped_json: &BTreeMap<(String, String), Value>,
    json_namespaces: &[String],
    scope: &crate::StoreScopedProjectionScope,
) -> Result<Vec<(String, String)>> {
    let namespaces = json_namespaces
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut addresses = BTreeSet::new();
    addresses.extend(scoped_manifest_dependency_addresses(
        scoped_json,
        &namespaces,
        scope,
    )?);
    addresses.extend(scoped_relationship_dependency_addresses(
        scoped_json,
        &namespaces,
    )?);
    if namespaces.contains("long_term") {
        addresses.extend(scoped_long_term_addresses_from_facet_docs(
            scoped_json,
            scope,
        )?);
    }
    addresses.extend(scoped_typed_recall_index_addresses(
        scoped_json,
        &namespaces,
        scope,
    )?);
    addresses.extend(scoped_graph_dependency_addresses(scoped_json, &namespaces)?);
    Ok(addresses.into_iter().collect())
}

pub(crate) fn validate_typed_recall_manifest_closure(
    manifest_namespace: &str,
    manifest_key: &str,
    manifest_value: &Value,
    scope: &crate::StoreScopedProjectionScope,
    mut read_json: impl FnMut(&str, &str) -> Result<Option<Value>>,
    mut read_blob: impl FnMut(&str, &str) -> Result<Option<Vec<u8>>>,
) -> Result<()> {
    let entries = typed_recall_manifest_entries_for_scope(
        manifest_namespace,
        manifest_key,
        manifest_value,
        scope,
    )?;

    for entry in entries {
        validate_recall_owner_namespace(manifest_namespace, &entry)?;
        let owner_kind = match entry.kind {
            crate::store_internal::recall_index::RecallIndexAddressKind::Json => "json",
            crate::store_internal::recall_index::RecallIndexAddressKind::Blob => "blob",
        };
        let binding_key = recall_owner_scope_binding_key(owner_kind, &entry.namespace, &entry.key)?;
        let binding_value = read_json(RECALL_OWNER_SCOPE_BINDING_NAMESPACE, &binding_key)?
            .ok_or_else(|| {
                Error::config(
                    "typed_recall_manifest_closure",
                    format!(
                        "recall owner {}/{} is missing its scope binding",
                        entry.namespace, entry.key
                    ),
                )
            })?;
        let binding =
            serde_json::from_value::<RecallOwnerScopeBinding>(binding_value).map_err(|error| {
                Error::config(
                    "typed_recall_manifest_closure",
                    format!("recall owner scope binding decode failed: {error}"),
                )
            })?;
        binding.validate()?;
        if binding.physical_key != binding_key
            || binding.memory_space_id != scope.memory_space_id
            || binding.mounted_subject_id != scope.mounted_subject_id
            || binding.owner_kind != owner_kind
            || binding.owner_namespace != entry.namespace
            || binding.owner_key != entry.key
            || binding.owner_content_sha256 != entry.content_sha256
        {
            return Err(Error::config(
                "typed_recall_manifest_closure",
                "recall owner binding differs from the exact manifest entry and subject scope",
            ));
        }
        let owner_bytes = match entry.kind {
            crate::store_internal::recall_index::RecallIndexAddressKind::Json => {
                let owner = read_json(&entry.namespace, &entry.key)?.ok_or_else(|| {
                    Error::config(
                        "typed_recall_manifest_closure",
                        format!(
                            "manifested JSON owner {}/{} is missing",
                            entry.namespace, entry.key
                        ),
                    )
                })?;
                validate_canonical_recall_json_owner(
                    manifest_namespace,
                    &entry.namespace,
                    &entry.key,
                    &owner,
                    scope,
                )?;
                serde_json::to_vec(&owner).map_err(|error| {
                    Error::config("typed_recall_manifest_closure", error.to_string())
                })?
            }
            crate::store_internal::recall_index::RecallIndexAddressKind::Blob => {
                read_blob(&entry.namespace, &entry.key)?.ok_or_else(|| {
                    Error::config(
                        "typed_recall_manifest_closure",
                        format!(
                            "manifested blob owner {}/{} is missing",
                            entry.namespace, entry.key
                        ),
                    )
                })?
            }
        };
        let actual_digest = format!("sha256:{:x}", Sha256::digest(&owner_bytes));
        if actual_digest != entry.content_sha256 {
            return Err(Error::config(
                "typed_recall_manifest_closure",
                format!(
                    "manifested owner {}/{} content digest mismatch",
                    entry.namespace, entry.key
                ),
            ));
        }
    }
    Ok(())
}

fn typed_recall_manifest_entries_for_scope(
    manifest_namespace: &str,
    manifest_key: &str,
    manifest_value: &Value,
    scope: &crate::StoreScopedProjectionScope,
) -> Result<Vec<crate::store_internal::recall_index::RecallIndexAddress>> {
    let (memory_space_id, mounted_subject_id, entries) =
        decode_typed_recall_manifest_scope(manifest_namespace, manifest_key, manifest_value)?;
    if memory_space_id != scope.memory_space_id
        || mounted_subject_id
            .as_deref()
            .is_some_and(|subject_id| subject_id != scope.mounted_subject_id)
    {
        return Err(Error::config(
            "typed_recall_manifest_closure",
            "typed recall manifest differs from the exact projection scope",
        ));
    }
    Ok(entries)
}

fn decode_typed_recall_manifest_scope(
    manifest_namespace: &str,
    manifest_key: &str,
    manifest_value: &Value,
) -> Result<(
    String,
    Option<String>,
    Vec<crate::store_internal::recall_index::RecallIndexAddress>,
)> {
    match manifest_namespace {
        CONVERSATION_RECALL_MANIFEST_NAMESPACE => {
            let manifest = crate::store_internal::recall_index::decode_typed_recall_index::<
                ConversationRecallManifest,
            >(manifest_key, manifest_value.clone())?;
            Ok((
                manifest.memory_space_id,
                Some(manifest.mounted_subject_id),
                manifest.entries,
            ))
        }
        ARCHIVE_RECALL_MANIFEST_NAMESPACE => {
            let manifest = crate::store_internal::recall_index::decode_typed_recall_index::<
                ArchiveRecallManifest,
            >(manifest_key, manifest_value.clone())?;
            Ok((
                manifest.memory_space_id,
                Some(manifest.mounted_subject_id),
                manifest.entries,
            ))
        }
        RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE => {
            let manifest = crate::store_internal::recall_index::decode_typed_recall_index::<
                crate::store_internal::recall_index::RuntimeSkillRecallManifest,
            >(manifest_key, manifest_value.clone())?;
            Ok((manifest.memory_space_id, None, manifest.entries))
        }
        CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE => {
            let manifest = crate::store_internal::recall_index::decode_typed_recall_index::<
                crate::store_internal::recall_index::ContinuityCapsuleScopeIndex,
            >(manifest_key, manifest_value.clone())?;
            Ok((manifest.memory_space_id, None, manifest.entries))
        }
        ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE => {
            let manifest = crate::store_internal::recall_index::decode_typed_recall_index::<
                crate::store_internal::recall_index::ActiveTaskRunByChatIndex,
            >(manifest_key, manifest_value.clone())?;
            Ok((manifest.memory_space_id, None, manifest.entries))
        }
        TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE => {
            let manifest = crate::store_internal::recall_index::decode_typed_recall_index::<
                crate::store_internal::recall_index::TaskLearningByChatIndex,
            >(manifest_key, manifest_value.clone())?;
            Ok((manifest.memory_space_id, None, manifest.entries))
        }
        _ => Err(Error::config(
            "typed_recall_manifest_closure",
            format!("unsupported typed recall manifest namespace {manifest_namespace}"),
        )),
    }
}

fn validate_recall_owner_namespace(
    manifest_namespace: &str,
    entry: &crate::store_internal::recall_index::RecallIndexAddress,
) -> Result<()> {
    let allowed = match manifest_namespace {
        CONVERSATION_RECALL_MANIFEST_NAMESPACE => matches!(
            entry.namespace.as_str(),
            "conversation_transcript"
                | "conversation_transcript_attr"
                | "conversation_transcript_derived_ref"
        ),
        ARCHIVE_RECALL_MANIFEST_NAMESPACE => matches!(
            entry.namespace.as_str(),
            "session"
                | "session_summary"
                | "active_work"
                | "turn_ledger"
                | "conversation_transcript_alias"
                | "daily"
                | "memory"
        ),
        RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE => entry.namespace == "skills",
        CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE => entry.namespace == "continuity_capsule",
        ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE => entry.namespace == "task_run",
        TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE => entry.namespace == "task_learning",
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(Error::config(
            "typed_recall_manifest_closure",
            format!(
                "typed recall manifest {manifest_namespace} cannot own namespace {}",
                entry.namespace
            ),
        ))
    }
}

fn validate_canonical_recall_json_owner(
    manifest_namespace: &str,
    owner_namespace: &str,
    owner_key: &str,
    owner: &Value,
    scope: &crate::StoreScopedProjectionScope,
) -> Result<()> {
    if manifest_namespace == CONVERSATION_RECALL_MANIFEST_NAMESPACE {
        return crate::store_internal::platform::validate_conversation_recall_owner_for_scope(
            owner_namespace,
            owner_key,
            owner,
            &scope.memory_space_id,
            &scope.mounted_subject_id,
        );
    }
    if owner_namespace == "conversation_transcript_alias" {
        let alias =
            serde_json::from_value::<bm_core::memory::TranscriptConversationAlias>(owner.clone())
                .map_err(|error| {
                Error::config(
                    "typed_recall_manifest_closure",
                    format!("conversation alias decode failed: {error}"),
                )
            })?;
        if alias.memory_space_id != scope.memory_space_id
            || alias.mounted_subject_id != scope.mounted_subject_id
            || alias.storage_key() != owner_key
        {
            return Err(Error::config(
                "typed_recall_manifest_closure",
                "conversation alias key or exact subject scope is not canonical",
            ));
        }
    }
    Ok(())
}

pub(crate) fn scoped_projection_root_addresses(
    json_namespaces: &[String],
    scope: &crate::StoreScopedProjectionScope,
) -> Result<Vec<(String, String)>> {
    let namespaces = json_namespaces
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut addresses = BTreeSet::new();
    for namespace in [
        "self_model",
        "self_authored_core",
        "core_revision_ledger",
        "self_continuity",
        "relationship_portfolio",
        "relationship_topology",
        "autonomy_strategy",
        "inner_life",
        "felt_significance",
        "temperament_continuity",
        "inner_conflict",
        "private_doc",
    ] {
        if namespaces.contains(namespace) {
            addresses.insert((namespace.to_string(), scope.mounted_subject_id.clone()));
        }
    }
    if namespaces.contains("archive_recall_manifests") {
        let manifest = ArchiveRecallManifest::build(
            1,
            &scope.memory_space_id,
            &scope.mounted_subject_id,
            std::iter::empty(),
        )?;
        addresses.insert((
            ArchiveRecallManifest::NAMESPACE.to_string(),
            manifest.physical_key,
        ));
    }
    if namespaces.contains("memory_facet_postings") {
        addresses.insert((
            "memory_facet_postings".to_string(),
            memory_facet_manifest_key(&scope.memory_space_id, &scope.mounted_subject_id)
                .map_err(|error| Error::config("store_scoped_projection", error.to_string()))?,
        ));
    }
    if namespaces.contains("memory_graph_manifests") {
        addresses.insert((
            "memory_graph_manifests".to_string(),
            memory_graph_scope_manifest_key(&scope.memory_space_id, &scope.mounted_subject_id),
        ));
    }
    if namespaces.contains(GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE) {
        addresses.insert((
            GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
            governed_evidence_source_claim_manifest_key(
                &scope.memory_space_id,
                &scope.mounted_subject_id,
            )?,
        ));
    }
    if namespaces.contains(CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE) {
        addresses.insert((
            CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
            control_plane_scope_manifest_key(&scope.memory_space_id, &scope.mounted_subject_id)?,
        ));
    }
    Ok(addresses.into_iter().collect())
}

fn scoped_relationship_dependency_addresses(
    scoped_json: &BTreeMap<(String, String), Value>,
    namespaces: &BTreeSet<&str>,
) -> Result<Vec<(String, String)>> {
    let mut relationship_ids = BTreeSet::new();
    for ((namespace, _), value) in scoped_json {
        match namespace.as_str() {
            "relationship_portfolio" => {
                let portfolio = serde_json::from_value::<RelationshipPortfolio>(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "store_scoped_projection",
                            format!("relationship portfolio: {error}"),
                        )
                    })?;
                relationship_ids.extend(portfolio.entries.into_iter().filter_map(|entry| {
                    let scope_id = entry.scope_id.trim();
                    (!scope_id.is_empty()).then(|| scope_id.to_string())
                }));
            }
            "relationship_topology" => {
                let topology = serde_json::from_value::<RelationshipTopology>(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "store_scoped_projection",
                            format!("relationship topology: {error}"),
                        )
                    })?;
                relationship_ids.extend(topology.entries.into_iter().filter_map(|entry| {
                    let scope_id = entry.scope_id.trim();
                    (!scope_id.is_empty()).then(|| scope_id.to_string())
                }));
            }
            _ => {}
        }
    }
    let mut addresses = BTreeSet::new();
    for namespace in [
        "relationship_constitution",
        "world_sense",
        "outer_voice",
        "mental_privacy",
    ] {
        if namespaces.contains(namespace) {
            addresses.extend(
                relationship_ids
                    .iter()
                    .cloned()
                    .map(|key| (namespace.to_string(), key)),
            );
        }
    }
    Ok(addresses.into_iter().collect())
}

fn scoped_manifest_dependency_addresses(
    scoped_json: &BTreeMap<(String, String), Value>,
    namespaces: &BTreeSet<&str>,
    scope: &crate::StoreScopedProjectionScope,
) -> Result<Vec<(String, String)>> {
    let mut addresses = BTreeSet::new();
    for ((namespace, _), value) in scoped_json {
        match namespace.as_str() {
            "memory_facet_postings" if value.get("posting_revisions").is_some() => {
                let manifest = serde_json::from_value::<MemoryFacetIndexManifest>(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "store_scoped_projection",
                            format!("facet manifest: {error}"),
                        )
                    })?;
                for posting in manifest.posting_revisions {
                    addresses.insert(("memory_facet_postings".to_string(), posting.posting_key));
                }
                if namespaces.contains("memory_facet_indexes") {
                    for owner in manifest.owner_versions {
                        addresses.insert((
                            "memory_facet_indexes".to_string(),
                            scoped_memory_facet_owner_storage_key(
                                &scope.memory_space_id,
                                &scope.mounted_subject_id,
                                &owner.owner_ref,
                            )
                            .map_err(|error| {
                                Error::config("store_scoped_projection", error.to_string())
                            })?,
                        ));
                    }
                }
            }
            "memory_graph_manifests" => {
                let manifest = serde_json::from_value::<MemoryGraphScopeManifest>(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "store_scoped_projection",
                            format!("graph manifest: {error}"),
                        )
                    })?;
                for (dependency_namespace, dependencies) in [
                    ("memory_graph_node_memberships", manifest.node_memberships),
                    ("memory_graph_edge_memberships", manifest.edge_memberships),
                    (
                        "memory_graph_backlink_memberships",
                        manifest.backlink_memberships,
                    ),
                    ("memory_graph_indexes", manifest.recall_indexes),
                ] {
                    if namespaces.contains(dependency_namespace) {
                        addresses.extend(dependencies.into_iter().map(|dependency| {
                            (dependency_namespace.to_string(), dependency.storage_key)
                        }));
                    }
                }
                if namespaces.contains("memory_graph_revisions") {
                    addresses.insert((
                        "memory_graph_revisions".to_string(),
                        manifest.revision.storage_key,
                    ));
                }
            }
            GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE => {
                let manifest =
                    serde_json::from_value::<GovernedEvidenceSourceClaimManifest>(value.clone())
                        .map_err(|error| {
                            Error::config(
                                "store_scoped_projection",
                                format!("evidence manifest: {error}"),
                            )
                        })?;
                if namespaces.contains(GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE) {
                    addresses.extend(
                        manifest
                            .owner_keys
                            .into_iter()
                            .map(|key| (GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(), key)),
                    );
                }
                if namespaces.contains(GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE) {
                    addresses.extend(
                        manifest
                            .claim_keys
                            .into_iter()
                            .map(|key| (GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(), key)),
                    );
                }
            }
            CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE => {
                let manifest = serde_json::from_value::<ControlPlaneScopeManifest>(value.clone())
                    .map_err(|error| {
                    Error::config(
                        "store_scoped_projection",
                        format!("control-plane manifest decode: {error}"),
                    )
                })?;
                manifest.validate(usize::MAX)?;
                if manifest.memory_space_id != scope.memory_space_id
                    || manifest.mounted_subject_id != scope.mounted_subject_id
                {
                    return Err(Error::config(
                        "store_scoped_projection",
                        "control-plane manifest differs from the exact projection scope",
                    ));
                }
                addresses.extend(
                    manifest
                        .entries
                        .into_iter()
                        .filter(|entry| namespaces.contains(entry.namespace.as_str()))
                        .map(|entry| (entry.namespace, entry.key)),
                );
            }
            _ => {}
        }
    }
    Ok(addresses.into_iter().collect())
}

fn scoped_typed_recall_index_addresses(
    scoped_json: &BTreeMap<(String, String), Value>,
    namespaces: &BTreeSet<&str>,
    scope: &crate::StoreScopedProjectionScope,
) -> Result<Vec<(String, String)>> {
    let mut addresses = BTreeSet::new();
    for ((namespace, key), value) in scoped_json {
        if namespace == "conversation_transcript_alias" {
            let alias = serde_json::from_value::<bm_core::memory::TranscriptConversationAlias>(
                value.clone(),
            )
            .map_err(|error| {
                Error::config(
                    "store_scoped_projection",
                    format!("conversation alias decode: {error}"),
                )
            })?;
            if alias.storage_key() != *key {
                return Err(Error::config(
                    "store_scoped_projection",
                    "conversation alias storage key is not canonical",
                ));
            }
            if alias.memory_space_id != scope.memory_space_id
                || alias.mounted_subject_id != scope.mounted_subject_id
            {
                return Err(Error::config(
                    "store_scoped_projection",
                    "conversation alias differs from the exact projection scope",
                ));
            }
            if namespaces.contains(CONVERSATION_RECALL_MANIFEST_NAMESPACE) {
                addresses.insert((
                    CONVERSATION_RECALL_MANIFEST_NAMESPACE.to_string(),
                    ConversationRecallManifest::build(
                        1,
                        &alias.memory_space_id,
                        &alias.mounted_subject_id,
                        &alias.channel_id,
                        &alias.conversation_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                ));
            }
            continue;
        }
        if !matches!(
            namespace.as_str(),
            CONVERSATION_RECALL_MANIFEST_NAMESPACE
                | ARCHIVE_RECALL_MANIFEST_NAMESPACE
                | RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE
                | CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE
                | ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE
                | TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE
        ) {
            continue;
        }
        let entries = typed_recall_manifest_entries_for_scope(namespace, key, value, scope)?;
        for entry in entries {
            validate_recall_owner_namespace(namespace, &entry)?;
            let owner_kind = match entry.kind {
                crate::store_internal::recall_index::RecallIndexAddressKind::Json => "json",
                crate::store_internal::recall_index::RecallIndexAddressKind::Blob => "blob",
            };
            if namespaces.contains(entry.namespace.as_str()) {
                addresses.insert((entry.namespace.clone(), entry.key.clone()));
                addresses.insert((
                    RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                    recall_owner_scope_binding_key(owner_kind, &entry.namespace, &entry.key)?,
                ));
            }
        }
    }
    Ok(addresses.into_iter().collect())
}

pub(crate) fn validate_scoped_recall_manifest_documents(
    json: &BTreeMap<(String, String), Value>,
    blobs: &BTreeMap<(String, String), Vec<u8>>,
    scope: &crate::StoreScopedProjectionScope,
) -> Result<()> {
    for ((namespace, key), value) in json {
        if !matches!(
            namespace.as_str(),
            CONVERSATION_RECALL_MANIFEST_NAMESPACE
                | ARCHIVE_RECALL_MANIFEST_NAMESPACE
                | RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE
                | CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE
                | ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE
                | TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE
        ) {
            continue;
        }
        validate_typed_recall_manifest_closure(
            namespace,
            key,
            value,
            scope,
            |owner_namespace, owner_key| {
                Ok(json
                    .get(&(owner_namespace.to_string(), owner_key.to_string()))
                    .cloned())
            },
            |owner_namespace, owner_key| {
                Ok(blobs
                    .get(&(owner_namespace.to_string(), owner_key.to_string()))
                    .cloned())
            },
        )?;
    }
    Ok(())
}

pub(crate) fn validate_snapshot_recall_manifest_documents(
    json: &BTreeMap<(String, String), Value>,
    blobs: &BTreeMap<(String, String), Vec<u8>>,
) -> Result<()> {
    for ((namespace, key), value) in json {
        if !matches!(
            namespace.as_str(),
            CONVERSATION_RECALL_MANIFEST_NAMESPACE
                | ARCHIVE_RECALL_MANIFEST_NAMESPACE
                | RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE
                | CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE
                | ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE
                | TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE
        ) {
            continue;
        }
        let (memory_space_id, explicit_subject_id, entries) =
            decode_typed_recall_manifest_scope(namespace, key, value)?;
        let mounted_subject_id = match explicit_subject_id {
            Some(subject_id) => subject_id,
            None => {
                let Some(first_entry) = entries.first() else {
                    continue;
                };
                let owner_kind = match first_entry.kind {
                    crate::store_internal::recall_index::RecallIndexAddressKind::Json => "json",
                    crate::store_internal::recall_index::RecallIndexAddressKind::Blob => "blob",
                };
                let binding_key = recall_owner_scope_binding_key(
                    owner_kind,
                    &first_entry.namespace,
                    &first_entry.key,
                )?;
                let binding_value = json
                    .get(&(
                        RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                        binding_key,
                    ))
                    .ok_or_else(|| {
                        Error::config(
                            "typed_recall_manifest_closure",
                            "recall manifest owner is missing its scope binding",
                        )
                    })?;
                let binding =
                    serde_json::from_value::<RecallOwnerScopeBinding>(binding_value.clone())
                        .map_err(|error| {
                            Error::config(
                                "typed_recall_manifest_closure",
                                format!("recall owner scope binding decode failed: {error}"),
                            )
                        })?;
                binding.validate()?;
                binding.mounted_subject_id
            }
        };
        let scope = crate::StoreScopedProjectionScope::new(memory_space_id, mounted_subject_id)?;
        validate_typed_recall_manifest_closure(
            namespace,
            key,
            value,
            &scope,
            |owner_namespace, owner_key| {
                Ok(json
                    .get(&(owner_namespace.to_string(), owner_key.to_string()))
                    .cloned())
            },
            |owner_namespace, owner_key| {
                Ok(blobs
                    .get(&(owner_namespace.to_string(), owner_key.to_string()))
                    .cloned())
            },
        )?;
    }
    Ok(())
}

pub(crate) fn validate_control_plane_manifest_set(
    json: &BTreeMap<(String, String), Value>,
    max_entries: usize,
) -> Result<()> {
    let control_namespaces = BTreeSet::from([
        "long_term_control_revision",
        "long_term_control_tombstone",
        "long_term_governance_policy",
        "long_term_control_audit",
    ]);
    let actual_addresses = json
        .keys()
        .filter(|(namespace, _)| control_namespaces.contains(namespace.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut claimed_addresses = BTreeSet::new();
    let mut manifest_count = 0usize;
    for ((namespace, key), value) in json {
        if namespace != CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE {
            continue;
        }
        manifest_count = manifest_count.saturating_add(1);
        let manifest = serde_json::from_value::<ControlPlaneScopeManifest>(value.clone()).map_err(
            |error| {
                Error::config(
                    "control_plane_scope_manifest",
                    format!("control-plane manifest decode failed: {error}"),
                )
            },
        )?;
        manifest.validate(max_entries)?;
        let canonical_key = control_plane_scope_manifest_key(
            &manifest.memory_space_id,
            &manifest.mounted_subject_id,
        )?;
        if manifest.physical_key != *key || canonical_key != *key {
            return Err(Error::config(
                "control_plane_scope_manifest",
                "control-plane manifest physical key is not canonical",
            ));
        }
        for entry in &manifest.entries {
            let address = (entry.namespace.clone(), entry.key.clone());
            if !claimed_addresses.insert(address.clone()) {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "control-plane document is claimed by multiple scope manifests",
                ));
            }
            let document = json.get(&address).ok_or_else(|| {
                Error::config(
                    "control_plane_scope_manifest",
                    "manifested control-plane document is missing",
                )
            })?;
            entry.validate_value(document)?;
            crate::store_internal::platform::validate_control_document_for_scope(
                &entry.namespace,
                &entry.key,
                document,
                &manifest.memory_space_id,
                &manifest.mounted_subject_id,
            )?;
        }
    }
    if manifest_count == 0 && actual_addresses.is_empty() {
        return Ok(());
    }
    if actual_addresses != claimed_addresses {
        return Err(Error::config(
            "control_plane_scope_manifest",
            "control-plane documents must have one exact typed scope-manifest owner",
        ));
    }
    Ok(())
}

pub(crate) fn validate_scoped_control_plane_documents(
    json: &BTreeMap<(String, String), Value>,
    scope: &crate::StoreScopedProjectionScope,
    max_entries: usize,
) -> Result<()> {
    let manifest_key =
        control_plane_scope_manifest_key(&scope.memory_space_id, &scope.mounted_subject_id)?;
    let manifest_value = json.get(&(
        CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
        manifest_key.clone(),
    ));
    let control_namespaces = BTreeSet::from([
        "long_term_control_revision",
        "long_term_control_tombstone",
        "long_term_governance_policy",
        "long_term_control_audit",
    ]);
    let actual_addresses = json
        .keys()
        .filter(|(namespace, _)| control_namespaces.contains(namespace.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>();
    let Some(manifest_value) = manifest_value else {
        if actual_addresses.is_empty() {
            return Ok(());
        }
        return Err(Error::config(
            "control_plane_scope_manifest",
            "control-plane documents require their exact subject scope manifest",
        ));
    };
    if json
        .keys()
        .filter(|(namespace, _)| namespace == CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE)
        .count()
        != 1
    {
        return Err(Error::config(
            "control_plane_scope_manifest",
            "scoped control-plane projection must contain exactly one scope manifest",
        ));
    }
    validate_control_plane_manifest_set(json, max_entries)?;
    let manifest = serde_json::from_value::<ControlPlaneScopeManifest>(manifest_value.clone())
        .map_err(|error| {
            Error::config(
                "control_plane_scope_manifest",
                format!("control-plane manifest decode failed: {error}"),
            )
        })?;
    manifest.validate(max_entries)?;
    if manifest.physical_key != manifest_key
        || manifest.memory_space_id != scope.memory_space_id
        || manifest.mounted_subject_id != scope.mounted_subject_id
    {
        return Err(Error::config(
            "control_plane_scope_manifest",
            "control-plane manifest differs from the exact projection scope",
        ));
    }
    let expected_addresses = manifest
        .entries
        .iter()
        .map(|entry| (entry.namespace.clone(), entry.key.clone()))
        .collect::<BTreeSet<_>>();
    if actual_addresses != expected_addresses {
        return Err(Error::config(
            "control_plane_scope_manifest",
            "control-plane projection does not exactly match its manifest closure",
        ));
    }
    Ok(())
}

fn scoped_graph_dependency_addresses(
    scoped_json: &BTreeMap<(String, String), Value>,
    namespaces: &BTreeSet<&str>,
) -> Result<Vec<(String, String)>> {
    let mut addresses = BTreeSet::new();
    for ((namespace, _), value) in scoped_json {
        let dependency =
            match namespace.as_str() {
                bm_core::memory::MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE
                    if namespaces.contains(bm_core::memory::MEMORY_GRAPH_NODE_NAMESPACE) =>
                {
                    let membership = serde_json::from_value::<
                        bm_core::memory::MemoryGraphNodeMembership,
                    >(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "store_scoped_projection",
                            format!("graph node membership decode: {error}"),
                        )
                    })?;
                    Some((
                        bm_core::memory::MEMORY_GRAPH_NODE_NAMESPACE.to_string(),
                        membership.document_key,
                    ))
                }
                bm_core::memory::MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE
                    if namespaces.contains(bm_core::memory::MEMORY_GRAPH_EDGE_NAMESPACE) =>
                {
                    let membership = serde_json::from_value::<
                        bm_core::memory::MemoryGraphEdgeMembership,
                    >(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "store_scoped_projection",
                            format!("graph edge membership decode: {error}"),
                        )
                    })?;
                    Some((
                        bm_core::memory::MEMORY_GRAPH_EDGE_NAMESPACE.to_string(),
                        membership.document_key,
                    ))
                }
                bm_core::memory::MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE
                    if namespaces.contains(bm_core::memory::MEMORY_GRAPH_BACKLINK_NAMESPACE) =>
                {
                    let membership = serde_json::from_value::<
                        bm_core::memory::MemoryGraphBacklinkMembership,
                    >(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "store_scoped_projection",
                            format!("graph backlink membership decode: {error}"),
                        )
                    })?;
                    Some((
                        bm_core::memory::MEMORY_GRAPH_BACKLINK_NAMESPACE.to_string(),
                        membership.document_key,
                    ))
                }
                _ => None,
            };
        if let Some(dependency) = dependency {
            addresses.insert(dependency);
        }
    }
    Ok(addresses.into_iter().collect())
}

pub(crate) fn scoped_long_term_addresses_from_facet_docs(
    scoped_json: &BTreeMap<(String, String), Value>,
    scope: &crate::StoreScopedProjectionScope,
) -> Result<Vec<(String, String)>> {
    let mut addresses = BTreeSet::new();
    for ((namespace, key), value) in scoped_json {
        if namespace != "memory_facet_indexes"
            || !json_document_matches_scoped_projection(namespace, key, value, scope)
        {
            continue;
        }
        let facet: MemoryFacetIndexDoc =
            serde_json::from_value(value.clone()).map_err(|error| {
                Error::config(
                    "store_scoped_projection",
                    format!("facet owner decode: {error}"),
                )
            })?;
        let expected_key = bm_core::memory::scoped_memory_facet_owner_storage_key(
            &scope.memory_space_id,
            &scope.mounted_subject_id,
            &facet.owner_ref,
        )
        .map_err(|error| {
            Error::config(
                "store_scoped_projection",
                format!("facet owner key: {error:?}"),
            )
        })?;
        if facet.schema_version != bm_core::memory::MEMORY_FACET_SCHEMA_VERSION
            || facet.memory_space_id != scope.memory_space_id
            || !facet
                .subject_ids
                .iter()
                .any(|subject| subject == &scope.mounted_subject_id)
            || key != &expected_key
        {
            return Err(Error::config(
                "store_scoped_projection",
                "facet-derived owner address is not canonically bound to the projection scope",
            ));
        }
        if facet.owner_ref.owner_plane == GovernedMemoryOwnerPlane::LongTerm {
            addresses.insert((
                "long_term".to_string(),
                scoped_long_term_memory_storage_key(
                    &scope.memory_space_id,
                    &facet.owner_ref.owner_id,
                )?,
            ));
        }
    }
    Ok(addresses.into_iter().collect())
}

pub(crate) fn validate_scoped_projection_post_image<R, P>(
    admission: &StoreTransactionAdmission,
    request: &crate::StoreScopedProjectionReplaceRequest,
    final_kv_entries: usize,
    retained_blob_lengths: R,
    replacement_blob_lengths: P,
    final_event_count: usize,
) -> Result<StoreBackendUsage>
where
    R: IntoIterator<Item = usize>,
    P: IntoIterator<Item = usize>,
{
    let capacity = admission.operation_capacity();
    if request.json_docs.len() > capacity.kv_max_entries
        || request.events.len() > capacity.event_log_max_items
    {
        return Err(store_budget_error(
            "scoped projection replacement exceeds entry capacity",
        ));
    }
    let mut namespaces = BTreeSet::new();
    for namespace in &request.json_namespaces {
        enforce_logical_key_budget(capacity, namespace, "", "store_scoped_projection")?;
        if !namespaces.insert(namespace.as_str()) {
            return Err(Error::config(
                "store_scoped_projection",
                "replacement JSON namespaces contain duplicates",
            ));
        }
    }
    let projection_json = request
        .json_docs
        .iter()
        .map(|doc| ((doc.namespace.clone(), doc.key.clone()), doc.value.clone()))
        .collect::<BTreeMap<_, _>>();
    let owned_addresses = scoped_projection_json_addresses(
        &request.json_namespaces,
        &projection_json,
        &request.scope,
        capacity,
    )?;
    validate_scoped_recall_manifest_documents(&projection_json, &BTreeMap::new(), &request.scope)?;
    validate_scoped_control_plane_documents(
        &projection_json,
        &request.scope,
        capacity.kv_max_entries,
    )?;
    let mut addresses = BTreeSet::new();
    let mut json_event_bytes = 0_usize;
    for doc in &request.json_docs {
        enforce_logical_key_budget(
            capacity,
            &doc.namespace,
            &doc.key,
            "store_scoped_projection",
        )?;
        if !namespaces.contains(doc.namespace.as_str())
            || !addresses.insert((doc.namespace.as_str(), doc.key.as_str()))
            || !owned_addresses.contains(&(doc.namespace.clone(), doc.key.clone()))
        {
            return Err(Error::config(
                "store_scoped_projection",
                "replacement JSON is outside the exact projection scope or duplicated",
            ));
        }
        let remaining = capacity.snapshot_max_bytes.saturating_sub(json_event_bytes);
        json_event_bytes = json_event_bytes
            .checked_add(bounded_serialized_len(
                &doc.value,
                remaining,
                "scoped projection JSON and event bytes",
            )?)
            .ok_or_else(|| {
                store_budget_error("scoped projection JSON and event byte count overflow")
            })?;
    }
    let mut event_ids = BTreeSet::new();
    for event in &request.events {
        enforce_event_key_budget(capacity, event, "store_scoped_projection")?;
        if !event_ids.insert(event.event_id.as_str()) {
            return Err(Error::config(
                "store_scoped_projection",
                "replacement payload contains duplicate event ids",
            ));
        }
        if !event_matches_scoped_projection(event, &request.scope) {
            return Err(Error::config(
                "store_scoped_projection",
                "replacement payload contains an event outside the exact scope",
            ));
        }
        let remaining = capacity.snapshot_max_bytes.saturating_sub(json_event_bytes);
        json_event_bytes = json_event_bytes
            .checked_add(bounded_serialized_len(
                event,
                remaining,
                "scoped projection JSON and event bytes",
            )?)
            .ok_or_else(|| {
                store_budget_error("scoped projection JSON and event byte count overflow")
            })?;
    }
    if json_event_bytes > capacity.snapshot_max_bytes {
        return Err(store_budget_error(format!(
            "scoped projection JSON and event bytes {} exceed {}",
            json_event_bytes, capacity.snapshot_max_bytes
        )));
    }
    bounded_serialized_len(
        request,
        capacity.import_max_bytes,
        "scoped projection import bytes",
    )?;
    let final_blob_bytes = retained_blob_lengths
        .into_iter()
        .chain(replacement_blob_lengths)
        .try_fold(0_usize, |total, len| {
            total.checked_add(len).ok_or_else(|| {
                store_budget_error("scoped projection post-image blob byte count overflow")
            })
        })?;
    if final_kv_entries > capacity.kv_max_entries {
        return Err(store_budget_error(format!(
            "scoped projection post-image kv entries {} exceed {}",
            final_kv_entries, capacity.kv_max_entries
        )));
    }
    if final_blob_bytes > capacity.blob_max_bytes {
        return Err(store_budget_error(format!(
            "scoped projection post-image blob bytes {} exceed {}",
            final_blob_bytes, capacity.blob_max_bytes
        )));
    }
    if final_event_count > capacity.event_log_max_items {
        return Err(store_budget_error(format!(
            "scoped projection post-image event count {} exceeds {}",
            final_event_count, capacity.event_log_max_items
        )));
    }
    Ok(StoreBackendUsage {
        kv_entries: final_kv_entries,
        blob_bytes: final_blob_bytes,
        event_count: final_event_count,
    })
}

fn reject_duplicate_known_keys(keys: &[(String, String)], kind: &str) -> Result<()> {
    let mut unique = BTreeSet::new();
    for (namespace, key) in keys {
        if !unique.insert((namespace, key)) {
            return Err(Error::config(
                "store_consistent_read",
                format!("duplicate {kind} address {namespace}/{key}"),
            ));
        }
    }
    Ok(())
}

fn serialized_json_len(value: &Value) -> Result<usize> {
    serialized_len(value)
}

fn serialized_len(value: &impl serde::Serialize) -> Result<usize> {
    struct CountingWriter(usize);
    impl std::io::Write for CountingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut writer = CountingWriter(0);
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
    Ok(writer.0)
}

fn bounded_serialized_len(
    value: &impl serde::Serialize,
    max_bytes: usize,
    label: &'static str,
) -> Result<usize> {
    struct BoundedCountingWriter {
        bytes: usize,
        max_bytes: usize,
        exceeded: bool,
    }

    impl std::io::Write for BoundedCountingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let Some(next) = self.bytes.checked_add(bytes.len()) else {
                self.exceeded = true;
                return Err(std::io::Error::other("serialized byte count overflow"));
            };
            if next > self.max_bytes {
                self.exceeded = true;
                return Err(std::io::Error::other("serialized byte budget exceeded"));
            }
            self.bytes = next;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut writer = BoundedCountingWriter {
        bytes: 0,
        max_bytes,
        exceeded: false,
    };
    if let Err(error) = serde_json::to_writer(&mut writer, value) {
        if writer.exceeded {
            return Err(store_budget_error(format!("{label} exceed {max_bytes}")));
        }
        return Err(Error::config("store_scoped_projection", error.to_string()));
    }
    Ok(writer.bytes)
}

fn known_key_read_receipt(
    json: &[StoreBoundedKnownJsonRead],
    blobs: &[StoreBoundedKnownBlobRead],
    events: &[MemoryStoreEvent],
) -> Result<StoreReadReceipt> {
    let mut hasher = Sha256::new();
    hasher.update(b"beetle_memory_bounded_known_key_read_v1");
    hasher.update((json.len() as u64).to_be_bytes());
    for read in json {
        hasher.update(b"json");
        update_read_digest(&mut hasher, read.namespace.as_bytes());
        update_read_digest(&mut hasher, read.key.as_bytes());
        match &read.value {
            Some(value) => {
                hasher.update([1]);
                let value = serde_json::to_vec(value)
                    .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
                update_read_digest(&mut hasher, &value);
            }
            None => hasher.update([0]),
        }
    }
    hasher.update((blobs.len() as u64).to_be_bytes());
    for read in blobs {
        hasher.update(b"blob");
        update_read_digest(&mut hasher, read.namespace.as_bytes());
        update_read_digest(&mut hasher, read.key.as_bytes());
        match &read.value {
            Some(value) => {
                hasher.update([1]);
                update_read_digest(&mut hasher, value);
            }
            None => hasher.update([0]),
        }
    }
    hasher.update((events.len() as u64).to_be_bytes());
    for event in events {
        hasher.update(b"event");
        let value = serde_json::to_vec(event)
            .map_err(|error| Error::config("store_consistent_read", error.to_string()))?;
        update_read_digest(&mut hasher, &value);
    }
    Ok(StoreReadReceipt {
        state_digest: format!("{:x}", hasher.finalize()),
        json_doc_count: json.iter().filter(|read| read.value.is_some()).count(),
        blob_count: blobs.iter().filter(|read| read.value.is_some()).count(),
        event_count: events.len(),
        entry_count: json.len().saturating_add(blobs.len()),
        json_bytes: json
            .iter()
            .filter_map(|read| read.value.as_ref())
            .map(serialized_json_len)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum(),
        blob_bytes: blobs
            .iter()
            .filter_map(|read| read.value.as_ref())
            .map(Vec::len)
            .sum(),
    })
}

fn immutable_read_session_receipt(
    json: &[StoreBoundedKnownJsonRead],
    blobs: &[StoreBoundedKnownBlobRead],
    json_bytes: usize,
    blob_bytes: usize,
) -> Result<StoreReadReceipt> {
    let mut receipt = known_key_read_receipt(json, blobs, &[])?;
    let mut hasher = Sha256::new();
    hasher.update(b"beetle_memory_immutable_read_session_v1");
    update_read_digest(&mut hasher, receipt.state_digest.as_bytes());
    receipt.state_digest = format!("{:x}", hasher.finalize());
    receipt.entry_count = json.len().saturating_add(blobs.len());
    receipt.json_bytes = json_bytes;
    receipt.blob_bytes = blob_bytes;
    Ok(receipt)
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

    struct ExactEvidenceTestSession {
        json: BTreeMap<(String, String), Value>,
        read: StoreReadSessionState,
    }

    impl ExactEvidenceTestSession {
        fn new(json: BTreeMap<(String, String), Value>) -> Self {
            Self {
                json,
                read: StoreReadSessionState::new(StoreCapacityBudget::full()),
            }
        }
    }

    impl StoreImmutableReadSession for ExactEvidenceTestSession {
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
                        self.json.get(&(namespace.clone(), key.clone())).cloned(),
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
                .map(|(namespace, key)| self.read.record_blob(namespace, key, None))
                .collect()
        }

        fn receipt(&self) -> Result<StoreReadReceipt> {
            self.read.receipt()
        }
    }

    #[test]
    fn exact_evidence_absence_never_reads_an_owner_outside_the_subject_manifest() {
        let memory_space_id = "space:manifest-first";
        let mounted_subject_id = "subject:a";
        let other_subject_owner_key = "owner:subject-b";
        let missing_owner_key = "owner:missing";
        let manifest = GovernedEvidenceSourceClaimManifest::build(
            memory_space_id,
            mounted_subject_id,
            Vec::<GovernedEvidenceOwnerClaimBinding>::new(),
            8,
        )
        .expect("empty subject manifest");
        let json = BTreeMap::from([
            (
                (
                    GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
                    manifest.physical_key.clone(),
                ),
                serde_json::to_value(&manifest).expect("manifest value"),
            ),
            (
                (
                    GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                    other_subject_owner_key.to_string(),
                ),
                serde_json::json!({"mounted_subject_id": "subject:b"}),
            ),
        ]);
        let read_absence = |owner_key: &str| {
            let mut session = ExactEvidenceTestSession::new(json.clone());
            read_governed_evidence_exact_in_session(
                &mut session,
                &StoreGovernedEvidenceExactReadRequest {
                    memory_space_id: memory_space_id.to_string(),
                    mounted_subject_id: mounted_subject_id.to_string(),
                    owner_keys: vec![owner_key.to_string()],
                    include_all_manifest_bindings: false,
                    allow_missing_manifest_for_empty_scope: false,
                },
            )
            .expect("manifest-first exact absence")
        };

        let other_subject = read_absence(other_subject_owner_key);
        let missing = read_absence(missing_owner_key);

        assert_eq!(&other_subject.manifest, &missing.manifest);
        assert_eq!(&other_subject.scope_index, &missing.scope_index);
        assert_eq!(other_subject.entry_count, 1);
        assert_eq!(other_subject.entry_count, missing.entry_count);
        assert_eq!(other_subject.json_bytes, missing.json_bytes);
        assert_eq!(other_subject.blob_bytes, missing.blob_bytes);
        assert_eq!(&other_subject.receipt, &missing.receipt);
        for result in [&other_subject, &missing] {
            assert_eq!(result.reads.len(), 1);
            assert!(result.reads[0].owner.is_none());
            assert!(result.reads[0].claim_key.is_none());
            assert!(result.reads[0].claim.is_none());
        }
    }

    #[test]
    fn scoped_recall_manifest_rejects_cross_subject_owner_smuggling() {
        let owner_value = serde_json::json!({"content": "subject-b session"});
        let owner = crate::store_internal::recall_index::RecallIndexAddress::json(
            "session",
            "session-b",
            1,
            1,
            &owner_value,
        )
        .expect("owner address");
        let manifest = ArchiveRecallManifest::build(1, "space-a", "subject-a", [owner.clone()])
            .expect("archive manifest");
        let binding = RecallOwnerScopeBinding::build(
            "space-a",
            "subject-b",
            "json",
            &owner.namespace,
            &owner.key,
            &owner.content_sha256,
        )
        .expect("subject-b binding");
        let json = BTreeMap::from([
            ((owner.namespace.clone(), owner.key.clone()), owner_value),
            (
                (
                    RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                    binding.physical_key.clone(),
                ),
                serde_json::to_value(binding).expect("binding value"),
            ),
            (
                (
                    ARCHIVE_RECALL_MANIFEST_NAMESPACE.to_string(),
                    manifest.physical_key.clone(),
                ),
                serde_json::to_value(manifest).expect("manifest value"),
            ),
        ]);

        let error = validate_scoped_recall_manifest_documents(
            &json,
            &BTreeMap::new(),
            &crate::StoreScopedProjectionScope::new("space-a", "subject-a").expect("scope"),
        )
        .expect_err("subject-b owner must not be admitted into subject-a recall manifest");

        assert_eq!(error.stage(), "typed_recall_manifest_closure");
        assert!(error.to_string().contains("subject scope"));
    }

    #[test]
    fn scoped_control_manifest_rejects_cross_subject_control_record_smuggling() {
        let memory_space_id = "space-a";
        let mounted_subject_id = "subject-a";
        let revision = bm_core::memory::LongTermMemoryControlRevision {
            schema_version: bm_core::memory::LONG_TERM_CONTROL_SCHEMA_VERSION,
            revision_id: "revision-b".to_string(),
            record_id: "record-b".to_string(),
            successor_record_id: None,
            operation: "correct".to_string(),
            owner_revision: 2,
            source_revision: Some(1),
            previous_digest: "before".to_string(),
            new_digest: "after".to_string(),
            reason: "cross-subject fixture".to_string(),
            owner_subject_id: "subject-b".to_string(),
            actor_subject_id: Some("governor".to_string()),
            memory_space_id: Some(memory_space_id.to_string()),
            created_at: 1,
        };
        let revision_key = bm_core::memory::scoped_long_term_control_storage_key(
            memory_space_id,
            bm_core::memory::LONG_TERM_CONTROL_REVISION_NAMESPACE,
            &revision.revision_id,
        )
        .expect("revision key");
        let revision_value = serde_json::to_value(revision).expect("revision value");
        let entry = crate::store_internal::schema::ControlPlaneScopeEntry::from_json(
            bm_core::memory::LONG_TERM_CONTROL_REVISION_NAMESPACE,
            &revision_key,
            &revision_value,
        )
        .expect("manifest entry");
        let manifest =
            ControlPlaneScopeManifest::build(1, memory_space_id, mounted_subject_id, [entry], 8)
                .expect("manifest");
        let mut json = BTreeMap::from([(
            (
                bm_core::memory::LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
                revision_key,
            ),
            revision_value,
        )]);
        json.insert(
            (
                CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                manifest.physical_key.clone(),
            ),
            serde_json::to_value(manifest).expect("manifest value"),
        );

        let error = validate_scoped_control_plane_documents(
            &json,
            &crate::StoreScopedProjectionScope::new(memory_space_id, mounted_subject_id)
                .expect("scope"),
            8,
        )
        .expect_err("subject-b record must not be admitted into subject-a manifest");

        assert_eq!(error.stage(), "control_plane_scope_manifest");
        assert!(error.to_string().contains("owner scope"));
    }

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

        let authority = StoreAdmissionAuthority::new();
        let admission = StoreTransactionAdmission::for_nonproduction_harness(capacity, &authority);
        let plan = apply_transaction(
            &admission,
            &request,
            &StoreTransactionContext {
                touched: current,
                usage: StoreBackendUsage {
                    kv_entries: 1,
                    blob_bytes: 0,
                    event_count: 0,
                },
                existing_event_ids: BTreeSet::new(),
            },
        )
        .unwrap();

        assert_eq!(plan.report.budget_report.required_kv_entries, 1);
        assert_eq!(plan.report.budget_report.remaining_kv_entries, 2);
        assert_eq!(plan.report.budget_report.remaining_events, 4);
        assert_eq!(
            plan.report.budget_report.admission_report_id,
            "nonproduction-replay-harness"
        );
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

        let authority = StoreAdmissionAuthority::new();
        let admission = StoreTransactionAdmission::for_nonproduction_harness(capacity, &authority);
        let error = apply_transaction(
            &admission,
            &request,
            &StoreTransactionContext {
                touched: current,
                usage: StoreBackendUsage::default(),
                existing_event_ids: BTreeSet::new(),
            },
        )
        .expect_err("json and blob entries share the kv entry budget");

        assert_eq!(error.stage(), "store_budget_exceeded");
    }

    #[test]
    fn transaction_admission_rejects_a_different_store_authority() {
        let capacity = StoreCapacityBudget::full();
        let issuer = StoreAdmissionAuthority::new();
        let other_store = StoreAdmissionAuthority::new();
        let admission = StoreTransactionAdmission::for_nonproduction_harness(capacity, &issuer);

        let error = admission
            .validate_inside_engine_fence(capacity, &other_store)
            .expect_err("store admission authority must be instance-bound");

        assert_eq!(error.stage(), "memory_write_transaction_resource_admission");
        assert!(error.to_string().contains("different store authority"));
    }
}
