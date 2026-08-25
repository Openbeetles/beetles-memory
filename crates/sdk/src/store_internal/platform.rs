use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::agent::{ActiveWorkRecord, ActiveWorkStore};
#[cfg(feature = "nonproduction-replay-harness")]
use bm_core::budget::{BenchmarkStoreCapacityExtension, StoreRuntimeBudget};
use bm_core::budget::{GovernedStateRuntimeBudget, RuntimeBudgetAuthority, RuntimeBudgetReport};
use bm_core::memory::*;
use bm_core::platform::{MemorySystemKind, Platform, SkillMetaStore, SkillStorage, StateFs};
use bm_core::resource::RuntimeResourceProbe;
use bm_core::runtime::{
    RuntimeLifecycleEffect, RuntimeLifecycleEvent, RuntimeLifecycleEventKind,
    RuntimeLifecycleEventSink, RuntimeLifecycleOperation, RuntimeLifecycleTrigger,
};
use bm_core::skills::{
    runtime_skill_scope_manifest_key, RuntimeSkillOwnerBinding, RuntimeSkillOwnerRecord,
    RuntimeSkillOwningScope, RuntimeSkillScopeManifest,
};
use bm_core::task::{normalize_task_item, TaskItem, TaskQuery, TaskStore};
use bm_core::task_execution::{
    TaskArtifactRecord, TaskArtifactStore, TaskLearningRecord, TaskLearningStore, TaskRunRecord,
    TaskRunStore,
};
use bm_core::{Error, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::store_internal::config::{open_runtime_budget_authority, resolve_store_capacity};
use crate::store_internal::recall_index::{
    decode_typed_recall_index, next_entry_revision, remove_recall_index_address,
    replace_recall_index_address, ActiveTaskRunByChatIndex, ArchiveRecallManifest,
    ContinuityCapsuleScopeIndex, ConversationRecallManifest, ConversationTranscriptAuxManifest,
    ConversationTranscriptPageIndex, RecallIndexAddress, RecallIndexAddressKind,
    TaskLearningByChatIndex, TypedRecallIndex, ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE,
    ARCHIVE_RECALL_MANIFEST_NAMESPACE, CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE,
    CONVERSATION_RECALL_MANIFEST_NAMESPACE, CONVERSATION_TRANSCRIPT_AUX_MANIFEST_NAMESPACE,
    CONVERSATION_TRANSCRIPT_PAGE_NAMESPACE, CONVERSATION_TRANSCRIPT_PAGE_SIZE,
    TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
};
use crate::store_internal::recall_read::{
    RecallImmutableReadContext, RecallReadSetClosureEvidence,
};
use crate::store_internal::schema::{
    admit_store_json_address, admit_store_json_document, classify_store_blob_address,
    control_plane_scope_manifest_key, governed_evidence_source_claim_manifest_key,
    recall_owner_scope_binding_key, store_memory_space_archive_json_namespaces,
    ControlPlaneScopeEntry, ControlPlaneScopeManifest, GovernedEvidenceSourceClaimManifest,
    RecallOwnerScopeBinding, StoreAddressAdmission, StoreBlobDecoderKind, StoreJsonDecoderKind,
    CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE, GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
    RECALL_OWNER_SCOPE_BINDING_NAMESPACE,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_internal::schema::{store_blob_namespaces, store_json_namespaces};
#[cfg(feature = "sqlite-store")]
use crate::store_internal::sqlite::SqliteStoreEngine;
use crate::store_internal::subject_soul::{
    build_subject_soul_open_certificate, validate_subject_soul_open_snapshot,
    StoreOpenClosureCertificate, SubjectSoulStoreMutationAuthority,
};
use crate::store_internal::transaction::{
    read_governed_evidence_exact_in_session, BackendTransactionState,
    ConditionalDeleteEventTemplate, GraphRepairAuthority, StoreAdmissionAuthority,
    StoreGovernedEvidenceExactReadRequest, StoreGovernedEvidenceExactReadResult,
};
use crate::store_internal::transcript_query::{
    catalog_page_key, catalog_root_key, decode_cursor, encode_cursor, keyring_key,
    message_locators, search_message_manifest_key, search_posting_key, search_root_key,
    term_digest, term_set_digest, time_posting_key, time_root_key,
    transcript_query_namespace_is_derived, TranscriptCatalogPageV1, TranscriptCatalogRootV1,
    TranscriptMessageSearchManifestV1, TranscriptPostingLocatorV1, TranscriptQueryCursorClaimsV1,
    TranscriptQueryKeyringV1, TranscriptSearchPostingPageV1, TranscriptSearchPostingRootV1,
    TranscriptTimePostingPageV1, TranscriptTimePostingRootV1, TRANSCRIPT_CATALOG_PAGE_NAMESPACE,
    TRANSCRIPT_CATALOG_ROOT_NAMESPACE, TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
    TRANSCRIPT_QUERY_KEYRING_NAMESPACE, TRANSCRIPT_QUERY_PAGE_CAPACITY,
    TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE, TRANSCRIPT_SEARCH_POSTING_NAMESPACE,
    TRANSCRIPT_SEARCH_ROOT_NAMESPACE, TRANSCRIPT_TIME_POSTING_NAMESPACE,
    TRANSCRIPT_TIME_ROOT_NAMESPACE,
};

pub(crate) struct RecallImmutableReadSessionOutcome<T> {
    pub(crate) output: T,
    pub(crate) receipt: StoreReadReceipt,
    pub(crate) read_set: RecallReadSetClosureEvidence,
    pub(crate) session_open_count: u64,
    pub(crate) receipt_count: u64,
}
use crate::{
    enforce_event_key_budget, enforce_logical_key_budget, store_budget_error,
    store_internal::embedded::EmbeddedStoreEngine, store_internal::file::FileStoreEngine,
    InMemoryStoreEngine, MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig,
    StoreBackendKind, StoreBlobPrecondition, StoreCapacityBudget, StoreEngine, StoreEngineMutation,
    StoreEventLog, StoreEventScope, StoreJsonPrecondition, StoreMetricEventSourceRead,
    StoreMutation, StoreMutationBatch, StoreMutationBatchReport, StoreOpenReport, StoreReadReceipt,
    StoreRepairReport, StoreSchemaManifest, StoreScopedProjectionReplaceReport,
    StoreScopedProjectionReplaceRequest, StoreScopedProjectionRequest, StoreScopedProjectionScope,
    StoreSnapshot, StoreSnapshotJsonDoc, StoreTransactionAdmission, StoreTransactionRequest,
    STORE_SCHEMA_ID, STORE_SCHEMA_VERSION,
};
#[cfg(feature = "nonproduction-replay-harness")]
use crate::{StoreSnapshotBlob, StoreSnapshotExportReport, StoreSnapshotImportReport};

static EVENT_SEQUENCE: Mutex<u64> = Mutex::new(1);

type RecallIndexMutationPlan = (
    &'static str,
    String,
    serde_json::Value,
    Option<serde_json::Value>,
);
type TranscriptCatalogPageSnapshot = (String, TranscriptCatalogPageV1, serde_json::Value);
type TranscriptCatalogLoad = (
    String,
    Option<TranscriptCatalogRootV1>,
    Option<serde_json::Value>,
    Vec<TranscriptCatalogPageSnapshot>,
);

/// SDK-private physical owner plan assembled before one governed Store commit.
///
/// Core self-runtime effects are deliberately not accepted here: SDK runtime owns
/// the effect-to-owner routing, while Store owns physical addresses, recall-index
/// post-images, and optimistic preconditions.
#[derive(Clone, Debug, Default)]
pub(crate) struct StoreOwnerMutationPlan {
    pub(crate) mutations: Vec<StoreMutation>,
    pub(crate) preconditions: Vec<StoreJsonPrecondition>,
    pub(crate) blob_preconditions: Vec<StoreBlobPrecondition>,
}

impl StoreOwnerMutationPlan {
    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<StoreMutation>,
        Vec<StoreJsonPrecondition>,
        Vec<StoreBlobPrecondition>,
    ) {
        (self.mutations, self.preconditions, self.blob_preconditions)
    }
}

pub(crate) fn canonical_subject_soul_full_intent_digest(
    core_intent_digest: &str,
    additional_mutations: &[StoreMutation],
    additional_preconditions: &[StoreJsonPrecondition],
    additional_blob_preconditions: &[StoreBlobPrecondition],
) -> Result<String> {
    let canonical_core = core_intent_digest.len() == 64
        && core_intent_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !canonical_core {
        return Err(Error::invalid_input(
            "subject_soul_full_intent_digest",
            "Core intent digest must be canonical lowercase hex without a prefix",
        ));
    }
    if additional_mutations.is_empty()
        && additional_preconditions.is_empty()
        && additional_blob_preconditions.is_empty()
    {
        return Ok(core_intent_digest.to_string());
    }
    fn canonical_parts<T: Serialize>(values: &[T]) -> Result<Vec<Vec<u8>>> {
        let mut parts = values
            .iter()
            .map(|value| {
                serde_json::to_vec(value).map_err(|error| {
                    Error::config("subject_soul_full_intent_digest", error.to_string())
                })
            })
            .collect::<Result<Vec<_>>>()?;
        parts.sort();
        Ok(parts)
    }
    let mut hasher = Sha256::new();
    for part in [
        b"beetle_memory_subject_soul_full_intent_v1".as_slice(),
        core_intent_digest.as_bytes(),
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    for group in [
        canonical_parts(additional_mutations)?,
        canonical_parts(additional_preconditions)?,
        canonical_parts(additional_blob_preconditions)?,
    ] {
        hasher.update((group.len() as u64).to_be_bytes());
        for part in group {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part);
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn push_exact_json_precondition(
    preconditions: &mut Vec<StoreJsonPrecondition>,
    namespace: &str,
    key: &str,
    before: Option<serde_json::Value>,
) -> Result<()> {
    let next = match before {
        Some(value) => StoreJsonPrecondition::Exact {
            namespace: namespace.to_string(),
            key: key.to_string(),
            value,
        },
        None => StoreJsonPrecondition::Absent {
            namespace: namespace.to_string(),
            key: key.to_string(),
        },
    };
    if let Some(existing) = preconditions
        .iter()
        .find(|precondition| match precondition {
            StoreJsonPrecondition::Absent {
                namespace: candidate_namespace,
                key: candidate_key,
            }
            | StoreJsonPrecondition::Exact {
                namespace: candidate_namespace,
                key: candidate_key,
                ..
            } => candidate_namespace == namespace && candidate_key == key,
        })
    {
        if existing != &next {
            return Err(Error::conflict(
                "store_owner_mutation_plan",
                format!("conflicting owner preconditions for {namespace}/{key}"),
            ));
        }
        return Ok(());
    }
    preconditions.push(next);
    Ok(())
}

fn blob_precondition(namespace: &str, key: &str, before: Option<&[u8]>) -> StoreBlobPrecondition {
    match before {
        Some(value) => StoreBlobPrecondition::ExactDigest {
            namespace: namespace.to_string(),
            key: key.to_string(),
            content_digest: format!("sha256:{:x}", Sha256::digest(value)),
        },
        None => StoreBlobPrecondition::Absent {
            namespace: namespace.to_string(),
            key: key.to_string(),
        },
    }
}

pub(crate) const GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE: &str = "governed_evidence_documents";
pub(crate) const GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE: &str = "governed_evidence_source_refs";

#[derive(Clone, Debug)]
pub(crate) struct StoreMutationOperationPlan {
    identity: MemoryMutationOperationIdentity,
    intent_digest: String,
    effect: MemoryMutationEffect,
    changed_count: u64,
    actor_subject_id: String,
    committed_at_unix_secs: u64,
    transaction_id: String,
    subject_soul_authorized: bool,
}

impl StoreMutationOperationPlan {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        identity: MemoryMutationOperationIdentity,
        intent_digest: impl Into<String>,
        effect: MemoryMutationEffect,
        changed_count: usize,
        actor_subject_id: impl Into<String>,
        committed_at_unix_secs: u64,
    ) -> Result<Self> {
        let intent_digest = intent_digest.into();
        let actor_subject_id = actor_subject_id.into();
        let changed_count = u64::try_from(changed_count).map_err(|_| {
            Error::invalid_input(
                "memory_mutation_operation_plan",
                "changed_count exceeds the durable receipt width",
            )
        })?;
        let transaction_id = mutation_operation_transaction_id(&identity, &intent_digest);
        MemoryMutationAuditRecord::new(
            identity.clone(),
            &intent_digest,
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            &transaction_id,
            effect,
            usize::try_from(changed_count).map_err(|_| {
                Error::invalid_input(
                    "memory_mutation_operation_plan",
                    "changed_count cannot be represented by this process",
                )
            })?,
            &actor_subject_id,
            committed_at_unix_secs,
        )?;
        Ok(Self {
            identity,
            intent_digest,
            effect,
            changed_count,
            actor_subject_id,
            committed_at_unix_secs,
            transaction_id,
            subject_soul_authorized: false,
        })
    }

    pub(crate) fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    pub(crate) fn identity(&self) -> &MemoryMutationOperationIdentity {
        &self.identity
    }

    pub(crate) fn intent_digest(&self) -> &str {
        &self.intent_digest
    }

    pub(crate) fn authorize_subject_soul(
        mut self,
        _authority: SubjectSoulStoreMutationAuthority,
    ) -> Self {
        self.subject_soul_authorized = true;
        self
    }

    pub(super) fn bind_subject_soul_transaction(
        mut self,
        transaction_id: impl Into<String>,
        _authority: SubjectSoulStoreMutationAuthority,
    ) -> Self {
        self.transaction_id = transaction_id.into();
        self.subject_soul_authorized = true;
        self
    }

    fn subject_soul_authorized(&self) -> bool {
        self.subject_soul_authorized
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StoreMutationOperationPreflight {
    pub(crate) identity: MemoryMutationOperationIdentity,
    pub(crate) intent_digest: String,
}

impl StoreMutationOperationPreflight {
    pub(crate) fn new(
        identity: MemoryMutationOperationIdentity,
        intent_digest: impl Into<String>,
    ) -> Result<Self> {
        let intent_digest = intent_digest.into();
        identity.validate_contract()?;
        if intent_digest.trim().is_empty() {
            return Err(Error::invalid_input(
                "memory_mutation_operation_intent",
                "intent_digest must not be empty",
            ));
        }
        Ok(Self {
            identity,
            intent_digest,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) enum StoreMutationOperationOutcome {
    Committed {
        receipt: MemoryMutationReceipt,
        report: StoreMutationBatchReport,
    },
    Replayed {
        receipt: MemoryMutationReceipt,
    },
}

pub(crate) struct StoreOpenPreflight {
    capacity: StoreCapacityBudget,
    governed_state_budget: GovernedStateRuntimeBudget,
    required_open_event: MemoryStoreEvent,
}

impl StoreOpenPreflight {
    fn new(
        capacity: StoreCapacityBudget,
        governed_state_budget: &GovernedStateRuntimeBudget,
        required_open_event: MemoryStoreEvent,
    ) -> Self {
        Self {
            capacity,
            governed_state_budget: *governed_state_budget,
            required_open_event,
        }
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    pub(crate) fn for_nonproduction_harness(
        config: &StoreBackendConfig,
        capacity: StoreCapacityBudget,
    ) -> Result<Self> {
        let preparation = StorePlatformPreparation::prepare(config.clone(), None)?;
        Ok(Self::new(
            capacity,
            &preparation.runtime_budget.governed_state_budget,
            build_runtime_event(config, "open", current_unix_secs()),
        ))
    }

    pub(crate) fn admit_snapshot(
        &self,
        snapshot: &StoreSnapshot,
        stage: &'static str,
    ) -> Result<()> {
        if snapshot.events.len() >= self.capacity.event_log_max_items {
            return Err(Error::config(
                stage,
                format!(
                    "existing store event count {} leaves no capacity for the required open event",
                    snapshot.events.len()
                ),
            ));
        }
        let mut post_open = snapshot.clone();
        post_open.events.push(self.required_open_event.clone());
        validate_snapshot_import_contract(snapshot, &self.governed_state_budget, self.capacity)
            .and_then(|_| validate_subject_soul_open_snapshot(snapshot).map(|_| ()))
            .and_then(|_| enforce_snapshot_logical_budget(self.capacity, &post_open))
            .map_err(|error| Error::config(stage, error.to_string()))
    }
}

#[derive(Clone)]
pub struct StorePlatform {
    config: StoreBackendConfig,
    capacity: StoreCapacityBudget,
    engine: Arc<dyn StoreEngine>,
    transaction_mutex: Arc<Mutex<()>>,
    schema_manifest: StoreSchemaManifest,
    open_report: StoreOpenReport,
    runtime_budget_authority: Arc<RuntimeBudgetAuthority>,
    subject_soul_open_closure_certificate: Arc<StoreOpenClosureCertificate>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StoreMemorySpaceProjectionReport {
    pub omitted_private_entries: usize,
    pub operation_capacity: StoreCapacityBudget,
    pub max_retained_long_term_revisions_per_owner: usize,
}

struct StoreBatchEventContext<'a> {
    batch: &'a StoreMutationBatch,
    event_scope: StoreEventScope,
    transaction_timestamp: u64,
    kind: MemoryStoreEventKind,
    plane: &'a str,
    record_key: &'a str,
}

#[derive(Clone, Copy)]
struct StoreCommitPreconditions<'a> {
    json: &'a [StoreJsonPrecondition],
    blobs: &'a [StoreBlobPrecondition],
}

impl<'a> StoreCommitPreconditions<'a> {
    const fn new(json: &'a [StoreJsonPrecondition], blobs: &'a [StoreBlobPrecondition]) -> Self {
        Self { json, blobs }
    }
}

pub(crate) struct StorePlatformPreparation {
    config: StoreBackendConfig,
    runtime_budget_authority: Arc<RuntimeBudgetAuthority>,
    runtime_budget: RuntimeBudgetReport,
    report_id: String,
    probe_source: bm_core::resource::RuntimeResourceProbeSource,
    store_medium: bm_core::budget::RuntimeStoreMedium,
    consumption: Arc<AtomicU8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreMigrationReport {
    pub backend: StoreBackendKind,
    pub from_schema_id: String,
    pub from_schema_version: u32,
    pub to_schema_id: String,
    pub to_schema_version: u32,
    pub migrated: bool,
    pub migration_event_id: String,
}

impl StorePlatformPreparation {
    fn prepare(
        config: StoreBackendConfig,
        firmware_probe: Option<Arc<dyn RuntimeResourceProbe>>,
    ) -> Result<Self> {
        Self::prepare_at(config, firmware_probe, current_unix_secs())
    }

    fn prepare_at(
        config: StoreBackendConfig,
        firmware_probe: Option<Arc<dyn RuntimeResourceProbe>>,
        now_secs: u64,
    ) -> Result<Self> {
        let runtime_budget_authority = Arc::new(open_runtime_budget_authority(
            &config,
            firmware_probe,
            now_secs,
        )?);
        let runtime_budget = runtime_budget_authority.current_report(now_secs);
        if runtime_budget.resource_snapshot.stale
            || runtime_budget.resource_snapshot.is_expired(now_secs)
        {
            return Err(Error::config(
                "store_prepare_admission",
                "prepared store requires a fresh runtime budget report",
            ));
        }
        Ok(Self {
            config,
            runtime_budget_authority,
            report_id: runtime_budget.report_id.clone(),
            probe_source: runtime_budget.resource_snapshot.source,
            store_medium: runtime_budget.store_medium,
            runtime_budget,
            consumption: Arc::new(AtomicU8::new(0)),
        })
    }

    fn consume(&self) -> Result<()> {
        self.consumption
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| {
                Error::config(
                    "store_prepared_admission_consumed",
                    "prepared store admission is a one-shot capability and was already consumed",
                )
            })
    }

    #[cfg(test)]
    fn duplicate_for_consumption_contract_test(&self) -> Self {
        Self {
            config: self.config.clone(),
            runtime_budget_authority: Arc::clone(&self.runtime_budget_authority),
            runtime_budget: self.runtime_budget.clone(),
            report_id: self.report_id.clone(),
            probe_source: self.probe_source,
            store_medium: self.store_medium,
            consumption: Arc::clone(&self.consumption),
        }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn runtime_budget(&self) -> &RuntimeBudgetReport {
        &self.runtime_budget
    }

    fn open(self) -> Result<(StorePlatform, StoreOpenReport)> {
        let capacity = resolve_store_capacity(&self.runtime_budget_authority)?;
        StorePlatform::open_prepared_at(self, capacity, current_unix_secs())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn open_with_benchmark_store_capacity(
        self,
        capacity: StoreRuntimeBudget,
    ) -> Result<StorePlatform> {
        let extension = BenchmarkStoreCapacityExtension::try_new(&self.runtime_budget, capacity)?;
        StorePlatform::open_prepared_at(
            self,
            StoreCapacityBudget::from_runtime_budget(extension.capacity()),
            current_unix_secs(),
        )
        .map(|(platform, _report)| platform)
    }
}

#[derive(Clone)]
pub(crate) struct ScopedLongTermMemoryStore {
    platform: StorePlatform,
    memory_space_id: String,
    factual_owner_id: String,
}

#[derive(Clone)]
pub(crate) struct ScopedLongTermMemoryControlReadStore {
    platform: StorePlatform,
    memory_space_id: String,
    control_owner_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct SessionSummaryRecord {
    summary: String,
    message_count: usize,
}

fn push_continuity_snapshot_json_write<T: Serialize>(
    mutations: &mut Vec<StoreMutation>,
    preconditions: &mut Vec<StoreJsonPrecondition>,
    namespace: &str,
    key: &str,
    observed: Option<&T>,
    next: &T,
) -> Result<()> {
    let observed = observed
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| Error::config("continuity_snapshot_import_plan", error.to_string()))?;
    preconditions.push(match observed {
        Some(value) => StoreJsonPrecondition::Exact {
            namespace: namespace.to_string(),
            key: key.to_string(),
            value,
        },
        None => StoreJsonPrecondition::Absent {
            namespace: namespace.to_string(),
            key: key.to_string(),
        },
    });
    mutations.push(StoreMutation::PutJson {
        namespace: namespace.to_string(),
        key: key.to_string(),
        value: serde_json::to_value(next)
            .map_err(|error| Error::config("continuity_snapshot_import_plan", error.to_string()))?,
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: namespace.to_string(),
        record_key: key.to_string(),
    });
    Ok(())
}

impl StorePlatform {
    pub fn open(config: StoreBackendConfig) -> Result<Self> {
        StorePlatformPreparation::prepare(config, None)?
            .open()
            .map(|(platform, _report)| platform)
    }

    pub(crate) fn migrate_v10_to_v11(config: StoreBackendConfig) -> Result<StoreMigrationReport> {
        let preparation = StorePlatformPreparation::prepare(config.clone(), None)?;
        preparation.consume()?;
        let capacity = resolve_store_capacity(&preparation.runtime_budget_authority)?;
        let migration_event =
            build_runtime_event(&config, "store.migration.v10_to_v11", current_unix_secs())
                .with_plane("store_schema")
                .with_record_key(STORE_SCHEMA_ID)
                .with_payload("from_schema_id", "beetle_memory_store_schema_v10")
                .with_payload("from_schema_version", "10")
                .with_payload("to_schema_id", STORE_SCHEMA_ID)
                .with_payload("to_schema_version", STORE_SCHEMA_VERSION.to_string());
        let migration_event_id = migration_event.event_id.clone();
        let open_preflight = StoreOpenPreflight::new(
            capacity,
            &preparation.runtime_budget.governed_state_budget,
            build_runtime_event(&config, "open", current_unix_secs()),
        );
        match config.backend {
            StoreBackendKind::File => {
                crate::store_internal::file::FileStoreEngine::migrate_v10_to_v11_explicit(
                    &config,
                    capacity,
                    StoreAdmissionAuthority::new(),
                    &open_preflight,
                    migration_event,
                )?
            }
            #[cfg(feature = "sqlite-store")]
            StoreBackendKind::Sqlite => {
                crate::store_internal::sqlite::migrate_sqlite_v10_to_v11_explicit(
                    &config,
                    capacity,
                    &open_preflight,
                    migration_event,
                )?;
            }
            #[cfg(not(feature = "sqlite-store"))]
            StoreBackendKind::Sqlite => {
                return Err(Error::config(
                    "store_migration_not_supported",
                    "SQLite migration requires the sqlite-store feature",
                ));
            }
            StoreBackendKind::InMemory | StoreBackendKind::Embedded => {
                return Err(Error::config(
                    "store_migration_not_supported",
                    "v10 to v11 migration requires a persistent File or SQLite backend",
                ));
            }
        }
        Ok(StoreMigrationReport {
            backend: config.backend,
            from_schema_id: "beetle_memory_store_schema_v10".to_string(),
            from_schema_version: 10,
            to_schema_id: STORE_SCHEMA_ID.to_string(),
            to_schema_version: STORE_SCHEMA_VERSION,
            migrated: true,
            migration_event_id,
        })
    }

    pub fn open_with_firmware_resource_probe(
        config: StoreBackendConfig,
        probe: Arc<dyn RuntimeResourceProbe>,
    ) -> Result<Self> {
        StorePlatformPreparation::prepare(config, Some(probe))?
            .open()
            .map(|(platform, _report)| platform)
    }

    #[cfg(all(test, feature = "nonproduction-replay-harness"))]
    pub(crate) fn engine_for_test(&self) -> Arc<dyn StoreEngine> {
        self.engine.clone()
    }

    #[cfg(all(test, feature = "nonproduction-replay-harness"))]
    pub(crate) fn with_engine_for_test(&self, engine: Arc<dyn StoreEngine>) -> Self {
        let mut platform = self.clone();
        platform.engine = engine;
        platform
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn tamper_json_document_for_nonproduction_harness(
        &self,
        namespace: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<()> {
        self.engine.put_json_value(namespace, key, value)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn delete_json_document_for_nonproduction_harness(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<()> {
        self.engine.delete_json_value(namespace, key).map(|_| ())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn read_json_namespace_unchecked_for_nonproduction_harness(
        &self,
        namespace: &str,
    ) -> Result<Vec<StoreSnapshotJsonDoc>> {
        let mut docs = Vec::new();
        for key in self.engine.list_json_keys(namespace)? {
            if let Some(value) = self.engine.get_json_value(namespace, &key)? {
                docs.push(StoreSnapshotJsonDoc {
                    namespace: namespace.to_string(),
                    key,
                    value,
                });
            }
        }
        Ok(docs)
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    #[allow(dead_code)] // SDK unit/replay internals may supply an attested firmware observation.
    pub(crate) fn open_with_nonproduction_probe(
        config: StoreBackendConfig,
        probe: Arc<dyn RuntimeResourceProbe>,
    ) -> Result<Self> {
        StorePlatformPreparation::prepare(config, Some(probe))?
            .open()
            .map(|(platform, _report)| platform)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn prepare_for_nonproduction_harness(
        config: StoreBackendConfig,
    ) -> Result<StorePlatformPreparation> {
        StorePlatformPreparation::prepare(config, None)
    }

    pub fn memory_space_long_term_memory_read_store(
        &self,
        memory_space_id: &str,
    ) -> Result<Arc<dyn bm_core::memory::LongTermMemoryReadStore>> {
        let memory_space_id = memory_space_id.trim().to_string();
        scoped_long_term_memory_storage_prefix(&memory_space_id)?;
        long_term_version_scope_manifest_key(&memory_space_id, &memory_space_id)?;
        Ok(Arc::new(ScopedLongTermMemoryStore {
            platform: self.clone(),
            factual_owner_id: memory_space_id.clone(),
            memory_space_id,
        }))
    }

    pub fn memory_space_long_term_memory_control_read_store(
        &self,
        memory_space_id: &str,
    ) -> Result<Arc<dyn LongTermMemoryControlReadStore>> {
        scoped_long_term_control_storage_prefix(
            memory_space_id,
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
        )?;
        Ok(Arc::new(ScopedLongTermMemoryControlReadStore {
            platform: self.clone(),
            memory_space_id: memory_space_id.trim().to_string(),
            control_owner_id: memory_space_id.trim().to_string(),
        }))
    }

    pub fn plan_continuity_snapshot_import_mutations(
        &self,
        plan: &ContinuitySnapshotImportPlan,
    ) -> Result<(Vec<StoreMutation>, Vec<StoreJsonPrecondition>)> {
        let mut mutations = Vec::new();
        let mut preconditions = Vec::new();
        if let Some(write) = plan.writes.summary.as_ref() {
            let observed =
                write
                    .observed
                    .as_ref()
                    .map(|(summary, message_count)| SessionSummaryRecord {
                        summary: summary.clone(),
                        message_count: *message_count,
                    });
            push_continuity_snapshot_json_write(
                &mut mutations,
                &mut preconditions,
                "session_summary",
                &write.chat_id,
                observed.as_ref(),
                &SessionSummaryRecord {
                    summary: write.summary.clone(),
                    message_count: write.message_count,
                },
            )?;
        }
        macro_rules! push_write {
            ($field:ident, $namespace:literal) => {
                if let Some(write) = plan.writes.$field.as_ref() {
                    push_continuity_snapshot_json_write(
                        &mut mutations,
                        &mut preconditions,
                        $namespace,
                        &write.key,
                        write.observed.as_ref(),
                        &write.next,
                    )?;
                }
            };
        }
        push_write!(self_model, "self_model");
        push_write!(self_authored_core, "self_authored_core");
        push_write!(core_revision_ledger, "core_revision_ledger");
        push_write!(self_continuity, "self_continuity");
        push_write!(relationship_constitution, "relationship_constitution");
        push_write!(relationship_portfolio, "relationship_portfolio");
        push_write!(execution_state, "execution_state");
        Ok((mutations, preconditions))
    }

    fn open_prepared_at(
        preparation: StorePlatformPreparation,
        capacity: StoreCapacityBudget,
        now_secs: u64,
    ) -> Result<(Self, StoreOpenReport)> {
        preparation.consume()?;
        let current_report = preparation
            .runtime_budget_authority
            .current_report(now_secs);
        if current_report.resource_snapshot.stale
            || current_report.resource_snapshot.is_expired(now_secs)
            || current_report.report_id != preparation.report_id
            || current_report.report_id != preparation.runtime_budget.report_id
            || current_report.resource_snapshot.source != preparation.probe_source
            || current_report.store_medium != preparation.store_medium
        {
            return Err(Error::config(
                "store_prepared_admission_invalid",
                "prepared store report identity, freshness, probe source, or medium changed before open",
            ));
        }
        let StorePlatformPreparation {
            config,
            runtime_budget_authority,
            runtime_budget: _,
            report_id: _,
            probe_source: _,
            store_medium: _,
            consumption: _,
        } = preparation;
        let open_event = build_runtime_event(&config, "open", now_secs);
        let (engine, repair, schema_manifest): (
            Arc<dyn StoreEngine>,
            StoreRepairReport,
            StoreSchemaManifest,
        ) = {
            let admission_authority = StoreAdmissionAuthority::new();
            let open_preflight = StoreOpenPreflight::new(
                capacity,
                &current_report.governed_state_budget,
                open_event.clone(),
            );
            match config.backend {
                StoreBackendKind::InMemory => (
                    Arc::new(InMemoryStoreEngine::new_with_admission_authority(
                        capacity,
                        admission_authority.clone(),
                    )),
                    StoreRepairReport::clean(),
                    StoreSchemaManifest::new(config.backend, config.profile, now_secs),
                ),
                StoreBackendKind::Embedded => (
                    Arc::new(EmbeddedStoreEngine::new_with_admission_authority(
                        capacity,
                        admission_authority.clone(),
                    )),
                    StoreRepairReport::clean(),
                    StoreSchemaManifest::new(config.backend, config.profile, now_secs),
                ),
                StoreBackendKind::File => {
                    let (engine, repair, manifest) =
                        FileStoreEngine::open_with_capacity_and_authority(
                            &config,
                            capacity,
                            admission_authority.clone(),
                            &open_preflight,
                        )?;
                    (Arc::new(engine), repair, manifest)
                }
                StoreBackendKind::Sqlite => {
                    let (engine, manifest) = sqlite_engine(
                        &config,
                        capacity,
                        admission_authority.clone(),
                        &open_preflight,
                    )?;
                    (engine, StoreRepairReport::clean(), manifest)
                }
            }
        };
        let report = StoreOpenReport {
            backend: config.backend.as_str().to_string(),
            schema_id: STORE_SCHEMA_ID.to_string(),
            repair,
        };
        let subject_soul_open_closure_certificate = Arc::new(
            build_subject_soul_open_certificate(engine.as_ref(), capacity).map_err(|error| {
                Error::config("store_subject_soul_open_closure", error.to_string())
            })?,
        );
        let platform = Self {
            config,
            capacity,
            engine,
            transaction_mutex: Arc::new(Mutex::new(())),
            schema_manifest,
            open_report: report.clone(),
            runtime_budget_authority,
            subject_soul_open_closure_certificate,
        };
        validate_transcript_query_engine_open_closure(platform.engine.as_ref()).map_err(
            |error| Error::config("store_transcript_query_open_closure", error.to_string()),
        )?;
        platform.append_validated_event(open_event)?;
        Ok((platform, report))
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    pub fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        self.engine.read_events()
    }

    pub(crate) fn read_metric_events(
        &self,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreMetricEventSourceRead> {
        self.engine.read_metric_events(capacity)
    }

    fn lock_transaction(&self, stage: &'static str) -> Result<std::sync::MutexGuard<'_, ()>> {
        self.transaction_mutex
            .lock()
            .map_err(|_| Error::config(stage, "transaction mutex poisoned"))
    }

    pub(crate) fn read_file_metric_events(
        root: impl AsRef<Path>,
        capacity: StoreCapacityBudget,
    ) -> Result<StoreMetricEventSourceRead> {
        crate::store_internal::file::read_metric_events_from_root(root.as_ref(), capacity)
    }

    pub(crate) fn current_runtime_budget(&self, now_secs: u64) -> RuntimeBudgetReport {
        self.runtime_budget_authority.current_report(now_secs)
    }

    #[allow(
        dead_code,
        reason = "foundation API consumed by the production recall integration"
    )]
    pub(crate) fn with_recall_immutable_read_session<T>(
        &self,
        runtime_budget: &RuntimeBudgetReport,
        read: impl FnOnce(&mut RecallImmutableReadContext<'_>) -> Result<T>,
    ) -> Result<RecallImmutableReadSessionOutcome<T>> {
        let now_secs = current_unix_secs();
        runtime_budget.validate_for_admission(now_secs)?;
        let active_report = crate::RuntimeBudgetLease::active_report(
            &self.runtime_budget_authority,
        )
        .ok_or_else(|| {
            Error::config(
                "recall_immutable_read_session_admission",
                "recall requires an active request-scoped runtime budget lease",
            )
        })?;
        if active_report.report_id != runtime_budget.report_id {
            return Err(Error::config(
                "recall_immutable_read_session_admission",
                "recall store session budget identity differs from the active request lease",
            ));
        }
        let capacity = StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        let mut session_open_count = 0u64;
        let session = self.engine.open_immutable_read_session(capacity)?;
        session_open_count = session_open_count.checked_add(1).ok_or_else(|| {
            Error::config(
                "recall_immutable_read_session",
                "immutable session-open count overflow",
            )
        })?;
        let mut context = RecallImmutableReadContext::new(session)
            .with_subject_soul_open_closure_certificate(
                self.subject_soul_open_closure_certificate().clone(),
            );
        let output = read(&mut context)?;
        let mut receipt_count = 0u64;
        let (receipt, read_set) = context.finish()?;
        receipt_count = receipt_count.checked_add(1).ok_or_else(|| {
            Error::config(
                "recall_immutable_read_session",
                "immutable receipt count overflow",
            )
        })?;
        Ok(RecallImmutableReadSessionOutcome {
            output,
            receipt,
            read_set,
            session_open_count,
            receipt_count,
        })
    }

    pub(crate) fn refresh_runtime_resource_snapshot(
        &self,
        now_secs: u64,
    ) -> Result<RuntimeBudgetReport> {
        self.runtime_budget_authority.refresh(now_secs)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn refresh_runtime_resource_snapshot_for_nonproduction_harness(
        &self,
    ) -> Result<RuntimeBudgetReport> {
        self.refresh_runtime_resource_snapshot(current_unix_secs())
    }

    pub(crate) fn refresh_runtime_resource_snapshot_if_stale(
        &self,
        now_secs: u64,
    ) -> Result<RuntimeBudgetReport> {
        let current = self.current_runtime_budget(now_secs);
        if current.resource_snapshot.stale {
            return self.refresh_runtime_resource_snapshot(now_secs);
        }
        Ok(current)
    }

    pub fn open_in_memory(config: StoreBackendConfig) -> Result<Self> {
        if config.backend != StoreBackendKind::InMemory {
            return Err(Error::config(
                "store_backend_config",
                "open_in_memory requires in-memory backend config",
            ));
        }
        Self::open(config)
    }

    pub fn config(&self) -> &StoreBackendConfig {
        &self.config
    }

    pub const fn capacity(&self) -> StoreCapacityBudget {
        self.capacity
    }

    pub(crate) fn runtime_budget_authority(&self) -> Arc<RuntimeBudgetAuthority> {
        self.runtime_budget_authority.clone()
    }

    pub(crate) fn engine_for_subject_soul(&self) -> &dyn StoreEngine {
        self.engine.as_ref()
    }

    pub(crate) fn subject_soul_open_closure_certificate(&self) -> &StoreOpenClosureCertificate {
        self.subject_soul_open_closure_certificate.as_ref()
    }

    pub fn open_report(&self) -> &StoreOpenReport {
        &self.open_report
    }

    pub(crate) fn exact_runtime_skill_manifest_materializer_available(&self) -> bool {
        self.schema_manifest.schema_id == STORE_SCHEMA_ID
            && self.schema_manifest.schema_version == STORE_SCHEMA_VERSION
            && self.schema_manifest.backend == self.config.backend.as_str()
            && self.schema_manifest.profile == self.config.profile.as_str()
            && self.schema_manifest.memory_system_kind == self.config.memory_system_kind.as_str()
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn commit_governed_memory_transaction(
        &self,
        batch: StoreMutationBatch,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_with_preconditions(batch, &[])
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn commit_governed_memory_transaction_with_preconditions(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(
            batch,
            StoreCommitPreconditions::new(preconditions, &[]),
            None,
            None,
            None,
            None,
        )
    }

    pub(crate) fn commit_governed_memory_transaction_with_blob_preconditions_and_runtime_budget(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        blob_preconditions: &[StoreBlobPrecondition],
        runtime_budget: &RuntimeBudgetReport,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(
            batch,
            StoreCommitPreconditions::new(preconditions, blob_preconditions),
            None,
            Some(runtime_budget),
            None,
            None,
        )
    }

    pub(crate) fn commit_governed_memory_transaction_with_runtime_budget_at(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        runtime_budget: &RuntimeBudgetReport,
        runtime_timestamp_unix_secs: u64,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(
            batch,
            StoreCommitPreconditions::new(preconditions, &[]),
            None,
            Some(runtime_budget),
            Some(runtime_timestamp_unix_secs),
            None,
        )
    }

    pub(crate) fn commit_governed_memory_transaction_with_blob_preconditions_and_runtime_budget_at(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        blob_preconditions: &[StoreBlobPrecondition],
        runtime_budget: &RuntimeBudgetReport,
        runtime_timestamp_unix_secs: u64,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(
            batch,
            StoreCommitPreconditions::new(preconditions, blob_preconditions),
            None,
            Some(runtime_budget),
            Some(runtime_timestamp_unix_secs),
            None,
        )
    }

    pub(crate) fn commit_governed_graph_repair_transaction_with_runtime_budget(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        authority: GraphRepairAuthority,
        runtime_budget: &RuntimeBudgetReport,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_governed_memory_transaction_authorized(
            batch,
            StoreCommitPreconditions::new(preconditions, &[]),
            Some(authority),
            Some(runtime_budget),
            None,
            None,
        )
    }

    pub(crate) fn commit_memory_mutation_operation_with_runtime_budget(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        operation: StoreMutationOperationPlan,
        runtime_budget: &RuntimeBudgetReport,
    ) -> Result<StoreMutationOperationOutcome> {
        self.commit_memory_mutation_operation_with_blob_preconditions_and_runtime_budget(
            batch,
            preconditions,
            &[],
            operation,
            runtime_budget,
        )
    }

    pub(crate) fn commit_memory_mutation_operation_with_blob_preconditions_and_runtime_budget(
        &self,
        batch: StoreMutationBatch,
        preconditions: &[StoreJsonPrecondition],
        blob_preconditions: &[StoreBlobPrecondition],
        operation: StoreMutationOperationPlan,
        runtime_budget: &RuntimeBudgetReport,
    ) -> Result<StoreMutationOperationOutcome> {
        operation.identity.validate_contract()?;
        if batch.scope.memory_space_id != operation.identity.memory_space_id()
            || batch.scope.subject_id != operation.identity.mounted_subject_id()
            || batch.transaction_id != operation.transaction_id
        {
            return Err(Error::invalid_input(
                "memory_mutation_operation_scope",
                "operation identity or transaction id differs from the governed batch scope",
            ));
        }
        if let Some(receipt) = self.load_committed_mutation_operation(&operation)? {
            return Ok(StoreMutationOperationOutcome::Replayed { receipt });
        }
        let commit = self.commit_governed_memory_transaction_authorized(
            batch,
            StoreCommitPreconditions::new(preconditions, blob_preconditions),
            None,
            Some(runtime_budget),
            Some(operation.committed_at_unix_secs),
            Some(&operation),
        );
        match commit {
            Ok(report) => {
                let receipt = self
                    .load_committed_mutation_operation(&operation)?
                    .ok_or_else(|| {
                        Error::config(
                            "memory_mutation_operation_commit",
                            "committed operation receipt is missing after store commit",
                        )
                    })?;
                Ok(StoreMutationOperationOutcome::Committed { receipt, report })
            }
            Err(error) if error.stage() == "memory_write_transaction_precondition_failed" => {
                match self.load_committed_mutation_operation(&operation)? {
                    Some(receipt) => Ok(StoreMutationOperationOutcome::Replayed { receipt }),
                    None => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn preflight_memory_mutation_operation(
        &self,
        operation: &StoreMutationOperationPreflight,
    ) -> Result<Option<MemoryMutationReceipt>> {
        operation.identity.validate_contract()?;
        self.load_committed_mutation_operation_parts(&operation.identity, &operation.intent_digest)
    }

    fn load_committed_mutation_operation(
        &self,
        operation: &StoreMutationOperationPlan,
    ) -> Result<Option<MemoryMutationReceipt>> {
        self.load_committed_mutation_operation_parts(&operation.identity, &operation.intent_digest)
    }

    fn load_committed_mutation_operation_parts(
        &self,
        identity: &MemoryMutationOperationIdentity,
        intent_digest: &str,
    ) -> Result<Option<MemoryMutationReceipt>> {
        let key = identity.storage_key();
        let addresses = [
            (MEMORY_MUTATION_RECEIPT_NAMESPACE.to_string(), key.clone()),
            (MEMORY_MUTATION_AUDIT_NAMESPACE.to_string(), key.clone()),
        ];
        for (namespace, key) in &addresses {
            admit_store_json_address(namespace, key, "memory_mutation_operation_pair_read")?;
        }
        let pair = self
            .engine
            .read_consistent_known_keys(&addresses, &[], false, self.capacity)?;
        let receipt = pair
            .json
            .iter()
            .find(|document| document.namespace == MEMORY_MUTATION_RECEIPT_NAMESPACE)
            .and_then(|document| document.value.clone())
            .map(serde_json::from_value::<MemoryMutationReceipt>)
            .transpose()
            .map_err(|error| Error::config("memory_mutation_receipt_read", error.to_string()))?;
        let audit = pair
            .json
            .iter()
            .find(|document| document.namespace == MEMORY_MUTATION_AUDIT_NAMESPACE)
            .and_then(|document| document.value.clone())
            .map(serde_json::from_value::<MemoryMutationAuditRecord>)
            .transpose()
            .map_err(|error| Error::config("memory_mutation_audit_read", error.to_string()))?;
        match (receipt, audit) {
            (None, None) => Ok(None),
            (Some(receipt), Some(audit)) => {
                receipt
                    .classify_replay(identity, intent_digest)
                    .map_err(|error| {
                        if error.class() == Some(bm_core::ErrorClass::Conflict) {
                            error
                        } else {
                            Error::config(
                                "memory_write_transaction_repair_required",
                                format!("invalid durable mutation receipt: {error}"),
                            )
                        }
                    })?;
                audit.validate_contract().map_err(|error| {
                    Error::config(
                        "memory_write_transaction_repair_required",
                        format!("invalid authoritative mutation audit: {error}"),
                    )
                })?;
                if audit.audit_record_id != receipt.audit_record_id
                    || audit.identity != receipt.identity
                    || audit.intent_digest != receipt.intent_digest
                    || audit.effect_plan_digest != receipt.effect_plan_digest
                    || audit.transaction_id != receipt.transaction_id
                    || audit.effect != receipt.effect
                    || audit.changed_count != receipt.changed_count
                    || audit.committed_at_unix_secs != receipt.committed_at_unix_secs
                {
                    return Err(Error::config(
                        "memory_write_transaction_repair_required",
                        "mutation receipt and authoritative audit binding differ",
                    ));
                }
                Ok(Some(receipt))
            }
            _ => Err(Error::config(
                "memory_write_transaction_repair_required",
                "mutation receipt and authoritative audit must exist together",
            )),
        }
    }

    fn commit_governed_memory_transaction_authorized(
        &self,
        mut batch: StoreMutationBatch,
        commit_preconditions: StoreCommitPreconditions<'_>,
        graph_repair_authority: Option<GraphRepairAuthority>,
        pinned_runtime_budget: Option<&RuntimeBudgetReport>,
        runtime_timestamp_unix_secs: Option<u64>,
        mutation_operation: Option<&StoreMutationOperationPlan>,
    ) -> Result<StoreMutationBatchReport> {
        let mut preconditions = commit_preconditions.json.to_vec();
        let blob_preconditions = commit_preconditions.blobs;
        let transaction_timestamp =
            canonical_transaction_timestamp(&batch, runtime_timestamp_unix_secs)?;
        self.append_conversation_derived_ref_recall_index_closure(
            &mut batch,
            &mut preconditions,
            transaction_timestamp,
        )?;
        self.append_recall_owner_scope_binding_closure(&mut batch, &mut preconditions)?;
        self.append_control_plane_scope_manifest_closure(&mut batch, &mut preconditions)?;
        if batch.transaction_id.trim().is_empty() {
            return Err(Error::config(
                "memory_write_transaction_preflight_failed",
                "transaction_id is required",
            ));
        }
        if batch.operation.trim().is_empty() {
            return Err(Error::config(
                "memory_write_transaction_preflight_failed",
                "operation is required",
            ));
        }
        append_mutation_operation_records(
            &mut batch,
            &mut preconditions,
            mutation_operation,
            transaction_timestamp,
        )?;

        let owned_runtime_budget;
        let runtime_budget = if let Some(runtime_budget) = pinned_runtime_budget {
            runtime_budget
        } else {
            owned_runtime_budget = self.current_runtime_budget(current_unix_secs());
            &owned_runtime_budget
        };
        if runtime_budget.resource_snapshot.stale {
            return Err(Error::config(
                "memory_write_transaction_resource_admission",
                "store transaction requires a fresh runtime budget report",
            ));
        }
        let operation_capacity =
            StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);

        validate_batch_mutation_namespaces(&batch, &preconditions, |namespace, key| {
            self.engine.get_blob(namespace, key)
        })?;
        let subject_soul_authorized =
            mutation_operation.is_some_and(StoreMutationOperationPlan::subject_soul_authorized);
        validate_protected_json_mutation_preconditions(
            &batch,
            &preconditions,
            subject_soul_authorized,
        )?;
        validate_mutation_operation_closure(&batch, &preconditions, mutation_operation)?;
        validate_recall_index_mutation_closure(
            &batch,
            |namespace, key| self.engine.get_json_value(namespace, key),
            |namespace, key| self.engine.get_blob(namespace, key),
        )?;
        validate_evidence_effect_address_closure(&batch, &preconditions)?;
        validate_governed_owner_facet_closure(&batch, &preconditions)?;
        validate_graph_manifest_closure(&batch, &preconditions)?;
        validate_control_audit_closure(&batch, &preconditions)?;
        validate_evidence_lifecycle_closure(&batch)?;

        let graph_scopes = if batch.mutations.iter().any(|mutation| {
            matches!(
                mutation,
                StoreMutation::PutJson { namespace, .. }
                    | StoreMutation::DeleteJson { namespace, .. }
                    if is_graph_namespace(namespace)
            )
        }) {
            graph_transaction_scopes(&batch, &preconditions)?
        } else {
            Vec::new()
        };

        let mut engine_mutations = Vec::new();

        for mutation in &batch.mutations {
            match mutation {
                StoreMutation::PutJson {
                    namespace,
                    key,
                    value,
                    event_kind,
                    plane,
                    record_key,
                } => {
                    ensure_batch_json_address(namespace, key)?;
                    enforce_logical_key_budget(
                        operation_capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(|error| {
                        memory_write_transaction_preflight_error_for_authority(
                            error,
                            subject_soul_authorized,
                        )
                    })?;
                    engine_mutations.push(StoreEngineMutation::PutJson {
                        namespace: namespace.clone(),
                        key: key.clone(),
                        value: value.clone(),
                    });
                    // CTQ postings/catalog documents are a derived closure of the
                    // authoritative transcript/head mutation in this same batch.
                    // Emitting one event per term posting causes unbounded audit-log
                    // amplification while adding no independent owner fact.
                    if !transcript_query_namespace_is_derived(namespace) {
                        let event_scope =
                            graph_effect_event_scope(&batch.scope, namespace, key, &graph_scopes);
                        let event = self.build_batch_event_in_scope(
                            StoreBatchEventContext {
                                batch: &batch,
                                event_scope,
                                transaction_timestamp,
                                kind: event_kind.clone(),
                                plane,
                                record_key,
                            },
                            stable_hash_json(value).map_err(|error| {
                                memory_write_transaction_preflight_error_for_authority(
                                    error,
                                    subject_soul_authorized,
                                )
                            })?,
                        );
                        enforce_event_key_budget(
                            operation_capacity,
                            &event,
                            "memory_write_transaction",
                        )
                        .map_err(|error| {
                            memory_write_transaction_preflight_error_for_authority(
                                error,
                                subject_soul_authorized,
                            )
                        })?;
                        engine_mutations.push(StoreEngineMutation::AppendEvent {
                            event: Box::new(event),
                        });
                    }
                }
                StoreMutation::DeleteJson {
                    namespace,
                    key,
                    event_kind,
                    plane,
                    record_key,
                } => {
                    ensure_batch_json_address(namespace, key)?;
                    enforce_logical_key_budget(
                        operation_capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(|error| {
                        memory_write_transaction_preflight_error_for_authority(
                            error,
                            subject_soul_authorized,
                        )
                    })?;
                    let event_scope =
                        graph_effect_event_scope(&batch.scope, namespace, key, &graph_scopes);
                    let event_template =
                        self.build_batch_event_template_in_scope(StoreBatchEventContext {
                            batch: &batch,
                            event_scope,
                            transaction_timestamp,
                            kind: event_kind.clone(),
                            plane,
                            record_key,
                        });
                    engine_mutations.push(StoreEngineMutation::DeleteJsonIfPresent {
                        namespace: namespace.clone(),
                        key: key.clone(),
                        event_template: Box::new(event_template),
                    });
                }
                StoreMutation::PutBlob {
                    namespace,
                    key,
                    value,
                    event_kind,
                    plane,
                    record_key,
                } => {
                    ensure_batch_blob_address(namespace, key, Some(value))?;
                    enforce_logical_key_budget(
                        operation_capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(|error| {
                        memory_write_transaction_preflight_error_for_authority(
                            error,
                            subject_soul_authorized,
                        )
                    })?;
                    let event = self.build_batch_event(
                        &batch,
                        transaction_timestamp,
                        event_kind.clone(),
                        plane,
                        record_key,
                        stable_hash_hex(value),
                    );
                    enforce_event_key_budget(
                        operation_capacity,
                        &event,
                        "memory_write_transaction",
                    )
                    .map_err(|error| {
                        memory_write_transaction_preflight_error_for_authority(
                            error,
                            subject_soul_authorized,
                        )
                    })?;
                    engine_mutations.push(StoreEngineMutation::PutBlob {
                        namespace: namespace.clone(),
                        key: key.clone(),
                        value: value.clone(),
                    });
                    engine_mutations.push(StoreEngineMutation::AppendEvent {
                        event: Box::new(event),
                    });
                }
                StoreMutation::DeleteBlob {
                    namespace,
                    key,
                    event_kind,
                    plane,
                    record_key,
                } => {
                    ensure_batch_blob_address(
                        namespace,
                        key,
                        self.engine.get_blob(namespace, key)?.as_deref(),
                    )?;
                    enforce_logical_key_budget(
                        operation_capacity,
                        namespace,
                        key,
                        "memory_write_transaction",
                    )
                    .map_err(|error| {
                        memory_write_transaction_preflight_error_for_authority(
                            error,
                            subject_soul_authorized,
                        )
                    })?;
                    let event_template = self.build_batch_event_template(
                        &batch,
                        transaction_timestamp,
                        event_kind.clone(),
                        plane,
                        record_key,
                    );
                    engine_mutations.push(StoreEngineMutation::DeleteBlobIfPresent {
                        namespace: namespace.clone(),
                        key: key.clone(),
                        event_template: Box::new(event_template),
                    });
                }
                StoreMutation::AppendEvent { event } => {
                    enforce_event_key_budget(operation_capacity, event, "memory_write_transaction")
                        .map_err(|error| {
                            memory_write_transaction_preflight_error_for_authority(
                                error,
                                subject_soul_authorized,
                            )
                        })?;
                    engine_mutations.push(StoreEngineMutation::AppendEvent {
                        event: event.clone(),
                    });
                }
            }
        }

        let governed_json_reads =
            governed_transaction_dependency_json_reads(&batch, &preconditions)?;
        let mut request = StoreTransactionRequest::new(
            batch.transaction_id.clone(),
            preconditions,
            engine_mutations,
            Some(Box::new(batch.clone())),
        )
        .with_blob_preconditions(blob_preconditions.to_vec())
        .with_governed_long_term_retention_limit(
            runtime_budget
                .governed_state_budget
                .max_retained_long_term_revisions_per_owner,
        )
        .with_governed_runtime_skill_owner_limit(
            runtime_budget
                .governed_state_budget
                .max_retained_runtime_skill_owners_per_scope,
        )
        .include_governed_json_reads(governed_json_reads);
        if !graph_scopes.is_empty() {
            for (_, _, scope_digest) in &graph_scopes {
                let prefix = format!("scope:{scope_digest}:doc:");
                request = request.include_governed_json_prefix_reads(
                    [
                        MEMORY_GRAPH_MANIFEST_NAMESPACE,
                        MEMORY_GRAPH_REVISION_NAMESPACE,
                        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
                        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
                        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
                        MEMORY_GRAPH_INDEX_NAMESPACE,
                        MEMORY_GRAPH_NODE_NAMESPACE,
                        MEMORY_GRAPH_EDGE_NAMESPACE,
                        MEMORY_GRAPH_BACKLINK_NAMESPACE,
                    ]
                    .into_iter()
                    .map(|namespace| (namespace.to_string(), prefix.clone())),
                );
            }
        }
        if let Some(authority) = graph_repair_authority {
            request = request.authorize_graph_repair(authority);
        }
        let admission = self.store_transaction_admission_for_report(runtime_budget)?;
        let engine_report = self
            .engine
            .commit_transaction_admitted(&request, &admission)
            .map_err(|error| {
                memory_write_transaction_commit_error(error, subject_soul_authorized)
            })?;

        Ok(StoreMutationBatchReport {
            transaction_id: batch.transaction_id,
            admitted: true,
            committed: true,
            mutations: batch.mutations.len(),
            events: engine_report.appended_events,
            changed_json: engine_report.changed_json,
            changed_blobs: engine_report.changed_blobs,
            budget_report: engine_report.budget_report,
            event_ids: engine_report.event_ids,
        })
    }

    fn append_recall_owner_scope_binding_closure(
        &self,
        batch: &mut StoreMutationBatch,
        preconditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<()> {
        let mut plans = Vec::new();
        let mut seen = BTreeSet::new();
        for mutation in &batch.mutations {
            let plan = match mutation {
                StoreMutation::PutJson {
                    namespace,
                    key,
                    value,
                    ..
                } if recall_index_namespace_for_json_owner(namespace).is_some() => {
                    let digest =
                        RecallIndexAddress::json(namespace, key, 1, 0, value)?.content_sha256;
                    Some(("json", namespace.as_str(), key.as_str(), Some(digest)))
                }
                StoreMutation::DeleteJson { namespace, key, .. }
                    if recall_index_namespace_for_json_owner(namespace).is_some() =>
                {
                    Some(("json", namespace.as_str(), key.as_str(), None))
                }
                StoreMutation::PutBlob {
                    namespace,
                    key,
                    value,
                    ..
                } if recall_index_namespace_for_blob_owner(namespace).is_some() => {
                    let digest =
                        RecallIndexAddress::blob(namespace, key, 1, 0, value)?.content_sha256;
                    Some(("blob", namespace.as_str(), key.as_str(), Some(digest)))
                }
                StoreMutation::DeleteBlob { namespace, key, .. }
                    if recall_index_namespace_for_blob_owner(namespace).is_some() =>
                {
                    Some(("blob", namespace.as_str(), key.as_str(), None))
                }
                _ => None,
            };
            let Some((owner_kind, owner_namespace, owner_key, content_digest)) = plan else {
                continue;
            };
            let identity = (
                owner_kind.to_string(),
                owner_namespace.to_string(),
                owner_key.to_string(),
            );
            if !seen.insert(identity.clone()) {
                return Err(Error::config(
                    "recall_owner_scope_binding",
                    "one transaction cannot mutate the same recall owner twice",
                ));
            }
            plans.push((identity.0, identity.1, identity.2, content_digest));
        }
        for (owner_kind, owner_namespace, owner_key, content_digest) in plans {
            let binding_key =
                recall_owner_scope_binding_key(&owner_kind, &owner_namespace, &owner_key)?;
            let previous = self
                .engine
                .get_json_value(RECALL_OWNER_SCOPE_BINDING_NAMESPACE, &binding_key)?;
            preconditions.push(match previous.clone() {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                    key: binding_key.clone(),
                    value,
                },
                None => StoreJsonPrecondition::Absent {
                    namespace: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                    key: binding_key.clone(),
                },
            });
            match content_digest {
                Some(content_digest) => {
                    if let Some(previous) = previous {
                        let previous = serde_json::from_value::<RecallOwnerScopeBinding>(previous)
                            .map_err(|error| {
                                Error::config(
                                    "recall_owner_scope_binding",
                                    format!("existing binding decode failed: {error}"),
                                )
                            })?;
                        previous.validate()?;
                        if previous.memory_space_id != batch.scope.memory_space_id
                            || previous.mounted_subject_id != batch.scope.subject_id
                        {
                            return Err(Error::config(
                                "recall_owner_scope_binding",
                                "recall owner is already bound to another exact subject scope",
                            ));
                        }
                    }
                    let binding = RecallOwnerScopeBinding::build(
                        &batch.scope.memory_space_id,
                        &batch.scope.subject_id,
                        &owner_kind,
                        &owner_namespace,
                        &owner_key,
                        &content_digest,
                    )?;
                    batch.mutations.push(StoreMutation::PutJson {
                        namespace: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                        key: binding_key.clone(),
                        value: serde_json::to_value(binding).map_err(|error| {
                            Error::config("recall_owner_scope_binding", error.to_string())
                        })?,
                        event_kind: MemoryStoreEventKind::MemoryMaintenance,
                        plane: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                        record_key: binding_key,
                    });
                }
                None => {
                    let previous = previous.ok_or_else(|| {
                        Error::config(
                            "recall_owner_scope_binding",
                            "recall owner delete requires an existing exact scope binding",
                        )
                    })?;
                    let previous = serde_json::from_value::<RecallOwnerScopeBinding>(previous)
                        .map_err(|error| {
                            Error::config(
                                "recall_owner_scope_binding",
                                format!("existing binding decode failed: {error}"),
                            )
                        })?;
                    previous.validate()?;
                    if previous.memory_space_id != batch.scope.memory_space_id
                        || previous.mounted_subject_id != batch.scope.subject_id
                    {
                        return Err(Error::config(
                            "recall_owner_scope_binding",
                            "recall owner delete scope differs from its exact binding",
                        ));
                    }
                    batch.mutations.push(StoreMutation::DeleteJson {
                        namespace: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                        key: binding_key.clone(),
                        event_kind: MemoryStoreEventKind::MemoryMaintenance,
                        plane: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.to_string(),
                        record_key: binding_key,
                    });
                }
            }
        }
        Ok(())
    }

    fn append_control_plane_scope_manifest_closure(
        &self,
        batch: &mut StoreMutationBatch,
        preconditions: &mut Vec<StoreJsonPrecondition>,
    ) -> Result<()> {
        const CONTROL_NAMESPACES: &[&str] = &[
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
            LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
            LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
            LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        ];
        let control_mutations = batch
            .mutations
            .iter()
            .filter(|mutation| match mutation {
                StoreMutation::PutJson { namespace, .. }
                | StoreMutation::DeleteJson { namespace, .. } => {
                    CONTROL_NAMESPACES.contains(&namespace.as_str())
                }
                _ => false,
            })
            .cloned()
            .collect::<Vec<_>>();
        if control_mutations.is_empty() {
            return Ok(());
        }
        let control_owner_id = batch.scope.memory_space_id.as_str();
        let manifest_key =
            control_plane_scope_manifest_key(&batch.scope.memory_space_id, control_owner_id)?;
        let previous_value = self
            .engine
            .get_json_value(CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE, &manifest_key)?;
        let previous = previous_value
            .clone()
            .map(|value| {
                serde_json::from_value::<ControlPlaneScopeManifest>(value).map_err(|error| {
                    Error::config(
                        "control_plane_scope_manifest",
                        format!("existing manifest decode failed: {error}"),
                    )
                })
            })
            .transpose()?;
        if let Some(previous) = previous.as_ref() {
            previous.validate(self.capacity.kv_max_entries)?;
            if previous.physical_key != manifest_key
                || previous.memory_space_id != batch.scope.memory_space_id
                || previous.mounted_subject_id != control_owner_id
            {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "existing control-plane manifest differs from the transaction scope",
                ));
            }
            for entry in &previous.entries {
                let value = self
                    .engine
                    .get_json_value(&entry.namespace, &entry.key)?
                    .ok_or_else(|| {
                        Error::config(
                            "control_plane_scope_manifest",
                            "manifested control-plane document is missing",
                        )
                    })?;
                entry.validate_value(&value)?;
                validate_control_document_for_scope(
                    &entry.namespace,
                    &entry.key,
                    &value,
                    &batch.scope.memory_space_id,
                    control_owner_id,
                )?;
            }
        }
        let mut entries = previous
            .as_ref()
            .map(|manifest| {
                manifest
                    .entries
                    .iter()
                    .cloned()
                    .map(|entry| ((entry.namespace.clone(), entry.key.clone()), entry))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        for mutation in control_mutations {
            match mutation {
                StoreMutation::PutJson {
                    namespace,
                    key,
                    value,
                    ..
                } => {
                    validate_control_document_for_scope(
                        &namespace,
                        &key,
                        &value,
                        &batch.scope.memory_space_id,
                        control_owner_id,
                    )?;
                    entries.insert(
                        (namespace.clone(), key.clone()),
                        ControlPlaneScopeEntry::from_json(&namespace, &key, &value)?,
                    );
                }
                StoreMutation::DeleteJson { namespace, key, .. } => {
                    if entries.remove(&(namespace, key)).is_none() {
                        return Err(Error::config(
                            "control_plane_scope_manifest",
                            "control-plane delete is outside the exact subject manifest",
                        ));
                    }
                }
                _ => unreachable!("filtered control-plane JSON mutation"),
            }
        }
        preconditions.push(match previous_value {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace: CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                key: manifest_key.clone(),
                value,
            },
            None => StoreJsonPrecondition::Absent {
                namespace: CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                key: manifest_key.clone(),
            },
        });
        let next = ControlPlaneScopeManifest::build(
            previous
                .as_ref()
                .map(|manifest| manifest.revision.saturating_add(1))
                .unwrap_or(1),
            &batch.scope.memory_space_id,
            control_owner_id,
            entries.into_values(),
            self.capacity.kv_max_entries,
        )?;
        batch.mutations.push(StoreMutation::PutJson {
            namespace: CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
            key: manifest_key.clone(),
            value: serde_json::to_value(next).map_err(|error| {
                Error::config("control_plane_scope_manifest", error.to_string())
            })?,
            event_kind: MemoryStoreEventKind::MemoryMaintenance,
            plane: CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
            record_key: manifest_key,
        });
        Ok(())
    }

    pub fn into_arc(self) -> Arc<dyn Platform> {
        Arc::new(self)
    }

    pub(crate) fn with_runtime_event_scope(mut self, event_scope: StoreEventScope) -> Self {
        self.config.event_scope = event_scope;
        self
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn export_store_snapshot(&self) -> Result<StoreSnapshot> {
        self.export_store_snapshot_with_report()
            .map(|(snapshot, _report)| snapshot)
    }

    pub fn read_json_namespace(&self, namespace: &str) -> Result<Vec<StoreSnapshotJsonDoc>> {
        ensure_json_snapshot_namespace(namespace, "store_json_namespace_read")?;
        let mut docs = Vec::new();
        for key in self.engine.list_json_keys(namespace)? {
            if let Some(value) = self.engine.get_json_value(namespace, &key)? {
                admit_store_json_document(namespace, &key, &value, "store_json_namespace_read")?;
                docs.push(StoreSnapshotJsonDoc {
                    namespace: namespace.to_string(),
                    key,
                    value,
                });
            }
        }
        Ok(docs)
    }

    pub(crate) fn read_governed_evidence_exact(
        &self,
        runtime_budget: &RuntimeBudgetReport,
        request: StoreGovernedEvidenceExactReadRequest,
    ) -> Result<StoreGovernedEvidenceExactReadResult> {
        let stage = "governed_evidence_exact_read_admission";
        runtime_budget.validate_for_admission(current_unix_secs())?;
        let active_report = crate::RuntimeBudgetLease::active_report(
            &self.runtime_budget_authority,
        )
        .ok_or_else(|| {
            Error::config(
                stage,
                "exact evidence read requires an active request-scoped runtime budget lease",
            )
        })?;
        if active_report.report_id != runtime_budget.report_id {
            return Err(Error::config(
                stage,
                "exact evidence read budget identity differs from the active request lease",
            ));
        }
        let capacity = StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        let mut session = self.engine.open_immutable_read_session(capacity)?;
        read_governed_evidence_exact_in_session(session.as_mut(), &request)
    }

    pub fn read_json_docs_by_keys(
        &self,
        namespace: &str,
        keys: &[String],
    ) -> Result<Vec<StoreSnapshotJsonDoc>> {
        ensure_json_snapshot_namespace(namespace, "store_json_namespace_read")?;
        let addresses = keys
            .iter()
            .map(|key| (namespace.to_string(), key.clone()))
            .collect::<Vec<_>>();
        self.read_json_docs_by_addresses(&addresses)
    }

    pub fn read_json_docs_by_addresses(
        &self,
        addresses: &[(String, String)],
    ) -> Result<Vec<StoreSnapshotJsonDoc>> {
        let mut seen = BTreeSet::new();
        let addresses = addresses
            .iter()
            .filter(|(namespace, key)| seen.insert((namespace.as_str(), key.as_str())))
            .map(|(namespace, key)| {
                admit_store_json_address(namespace, key, "store_json_known_key_read")?;
                Ok((namespace.clone(), key.clone()))
            })
            .collect::<Result<Vec<_>>>()?;
        let runtime_budget = self.current_runtime_budget(current_unix_secs());
        if runtime_budget.resource_snapshot.stale {
            return Err(Error::config(
                "store_json_known_key_read",
                "known-key read requires a fresh runtime budget report",
            ));
        }
        let result = self.engine.read_consistent_known_keys(
            &addresses,
            &[],
            false,
            StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget),
        )?;
        let mut docs = Vec::new();
        for read in result.json {
            if let Some(value) = read.value {
                admit_store_json_document(
                    &read.namespace,
                    &read.key,
                    &value,
                    "store_json_known_key_read",
                )?;
                docs.push(StoreSnapshotJsonDoc {
                    namespace: read.namespace,
                    key: read.key,
                    value,
                });
            }
        }
        Ok(docs)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn p8_read_json_addresses_with_receipt(
        &self,
        addresses: &[(String, String)],
    ) -> Result<(Vec<StoreSnapshotJsonDoc>, StoreReadReceipt)> {
        let mut seen = BTreeSet::new();
        let addresses = addresses
            .iter()
            .filter(|(namespace, key)| seen.insert((namespace.as_str(), key.as_str())))
            .map(|(namespace, key)| {
                admit_store_json_address(namespace, key, "p8_forget_post_image_read")?;
                Ok((namespace.clone(), key.clone()))
            })
            .collect::<Result<Vec<_>>>()?;
        let runtime_budget = self.current_runtime_budget(current_unix_secs());
        if runtime_budget.resource_snapshot.stale {
            return Err(Error::config(
                "p8_forget_post_image_read",
                "post-Forget exact-zero read requires a fresh runtime budget report",
            ));
        }
        let result = self.engine.read_consistent_known_keys(
            &addresses,
            &[],
            false,
            StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget),
        )?;
        let mut docs = Vec::new();
        for read in result.json {
            if let Some(value) = read.value {
                admit_store_json_document(
                    &read.namespace,
                    &read.key,
                    &value,
                    "p8_forget_post_image_read",
                )?;
                docs.push(StoreSnapshotJsonDoc {
                    namespace: read.namespace,
                    key: read.key,
                    value,
                });
            }
        }
        Ok((docs, result.receipt))
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn export_store_snapshot_with_report(
        &self,
    ) -> Result<(StoreSnapshot, StoreSnapshotExportReport)> {
        let runtime_budget = self.current_runtime_budget(current_unix_secs());
        let mut json_docs = Vec::new();
        for namespace in store_json_namespaces() {
            for key in self.engine.list_json_keys(namespace)? {
                if let Some(value) = self.engine.get_json_value(namespace, &key)? {
                    admit_store_json_document(namespace, &key, &value, "store_snapshot_export")?;
                    json_docs.push(StoreSnapshotJsonDoc {
                        namespace: namespace.to_string(),
                        key,
                        value,
                    });
                }
            }
        }
        let mut blobs = Vec::new();
        for namespace in store_blob_namespaces() {
            for key in self.engine.list_blob_keys(namespace)? {
                if let Some(value) = self.engine.get_blob(namespace, &key)? {
                    blobs.push(StoreSnapshotBlob {
                        namespace: namespace.to_string(),
                        key,
                        value,
                    });
                }
            }
        }
        let snapshot = StoreSnapshot::new(
            self.schema_manifest.clone(),
            json_docs,
            blobs,
            self.read_events()?,
        );
        validate_snapshot_import_contract(
            &snapshot,
            &runtime_budget.governed_state_budget,
            self.capacity,
        )
        .map_err(|error| Error::config("store_snapshot_export", error.to_string()))?;
        enforce_snapshot_logical_budget(self.capacity, &snapshot)
            .map_err(|error| Error::config("store_snapshot_export", error.to_string()))?;
        self.enforce_snapshot_budget(&snapshot, self.capacity.export_max_bytes, "export")?;
        let report = snapshot.export_report();
        Ok((snapshot, report))
    }

    pub(crate) fn export_memory_space_projection_with_report(
        &self,
        scope: &StoreScopedProjectionScope,
        pinned_runtime_budget: Option<&RuntimeBudgetReport>,
    ) -> Result<(StoreSnapshot, StoreMemorySpaceProjectionReport)> {
        let owned_runtime_budget;
        let runtime_budget = if let Some(runtime_budget) = pinned_runtime_budget {
            runtime_budget
        } else {
            owned_runtime_budget = self.current_runtime_budget(current_unix_secs());
            &owned_runtime_budget
        };
        let operation_capacity =
            StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        let projection = self.engine.read_scoped_projection(
            &StoreScopedProjectionRequest {
                scope: scope.clone(),
                json_namespaces: store_memory_space_archive_json_namespaces()
                    .map(str::to_string)
                    .collect(),
                include_events: true,
            },
            operation_capacity,
        )?;
        let mut omitted_private_entries = 0usize;
        let json_docs = projection
            .json_docs
            .into_iter()
            .filter_map(|document| {
                if document.namespace == TRANSCRIPT_QUERY_KEYRING_NAMESPACE {
                    omitted_private_entries = omitted_private_entries.saturating_add(1);
                    return None;
                }
                crate::store_internal::engine::json_document_is_protected_owner(
                    &document.namespace,
                    &document.value,
                )
                .map(|protected| (!protected).then_some(document))
                .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        let events = projection
            .events
            .into_iter()
            .filter(|event| !crate::store_internal::engine::event_is_protected_owner(event))
            .collect();
        let snapshot =
            StoreSnapshot::new(self.schema_manifest.clone(), json_docs, Vec::new(), events);
        self.enforce_snapshot_budget(
            &snapshot,
            self.capacity.snapshot_max_bytes,
            "memory_space_export",
        )?;
        Ok((
            snapshot,
            StoreMemorySpaceProjectionReport {
                omitted_private_entries,
                operation_capacity,
                max_retained_long_term_revisions_per_owner: runtime_budget
                    .governed_state_budget
                    .max_retained_long_term_revisions_per_owner,
            },
        ))
    }

    pub(crate) fn replace_memory_space_projection_with_report(
        &self,
        scope: &StoreScopedProjectionScope,
        snapshot: &StoreSnapshot,
        pinned_runtime_budget: Option<&RuntimeBudgetReport>,
    ) -> Result<StoreScopedProjectionReplaceReport> {
        let owned_runtime_budget;
        let runtime_budget = if let Some(runtime_budget) = pinned_runtime_budget {
            runtime_budget
        } else {
            owned_runtime_budget = self.current_runtime_budget(current_unix_secs());
            &owned_runtime_budget
        };
        let operation_capacity =
            StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        if snapshot
            .json_docs
            .iter()
            .any(|document| document.namespace == TRANSCRIPT_QUERY_KEYRING_NAMESPACE)
        {
            return Err(Error::config(
                "memory_space_import",
                "typed memory-space archive must not carry transcript cursor authority",
            ));
        }
        if !snapshot.blobs.is_empty() {
            return Err(Error::config(
                "memory_space_import",
                "typed memory-space archive must not contain unowned blobs",
            ));
        }
        for document in &snapshot.json_docs {
            if crate::store_internal::engine::json_document_is_protected_owner(
                &document.namespace,
                &document.value,
            )? {
                return Err(Error::config(
                    "memory_space_import",
                    "typed memory-space archive must not contain protected Soul/Relationship state",
                ));
            }
        }
        if snapshot
            .events
            .iter()
            .any(crate::store_internal::engine::event_is_protected_owner)
        {
            return Err(Error::config(
                "memory_space_import",
                "typed memory-space archive must not contain protected Soul/Relationship events",
            ));
        }
        let mut admitted_snapshot = snapshot.clone();
        if admitted_snapshot.json_docs.iter().any(|document| {
            document.namespace == CONVERSATION_RECALL_MANIFEST_NAMESPACE
                || transcript_query_namespace_is_derived(&document.namespace)
        }) {
            let keyring = Self::fresh_query_keyring(&scope.memory_space_id, current_unix_secs())?;
            admitted_snapshot.json_docs.push(StoreSnapshotJsonDoc {
                namespace: TRANSCRIPT_QUERY_KEYRING_NAMESPACE.to_string(),
                key: keyring_key(&scope.memory_space_id),
                value: serde_json::to_value(keyring)
                    .map_err(|error| Error::config("memory_space_import", error.to_string()))?,
            });
        }
        validate_snapshot_import_contract(
            &admitted_snapshot,
            &runtime_budget.governed_state_budget,
            operation_capacity,
        )?;
        validate_scoped_projection_governed_closure(&admitted_snapshot, scope)?;
        let admission = self.store_transaction_admission_for_report(runtime_budget)?;
        let replace = self.engine.replace_scoped_projection(
            &StoreScopedProjectionReplaceRequest {
                scope: scope.clone(),
                json_namespaces: store_memory_space_archive_json_namespaces()
                    .map(str::to_string)
                    .collect(),
                json_docs: admitted_snapshot.json_docs,
                events: admitted_snapshot.events,
                preserve_protected_owner_state: true,
            },
            &admission,
        )?;
        Ok(replace)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn import_store_snapshot(&self, snapshot: &StoreSnapshot) -> Result<()> {
        self.import_store_snapshot_with_report(snapshot)
            .map(|_report| ())
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn import_store_snapshot_with_report(
        &self,
        snapshot: &StoreSnapshot,
    ) -> Result<StoreSnapshotImportReport> {
        let runtime_budget = self.current_runtime_budget(current_unix_secs());
        validate_snapshot_import_contract(
            snapshot,
            &runtime_budget.governed_state_budget,
            self.capacity,
        )?;
        enforce_snapshot_logical_budget(self.capacity, snapshot)?;
        self.enforce_snapshot_budget(snapshot, self.capacity.import_max_bytes, "import")?;
        let _transaction_guard = self.lock_transaction("store_snapshot_import")?;
        let json_namespaces = store_json_namespaces().collect::<Vec<_>>();
        let blob_namespaces = store_blob_namespaces().collect::<Vec<_>>();
        let replace_report = self.engine.replace_snapshot(
            &json_namespaces,
            &blob_namespaces,
            &snapshot.json_docs,
            &snapshot.blobs,
            &snapshot.events,
        )?;
        Ok(StoreSnapshotImportReport {
            schema_id: snapshot.schema_id.clone(),
            json_docs: snapshot.json_docs.len(),
            blobs: snapshot.blobs.len(),
            json_deleted: replace_report.json_deleted,
            blobs_deleted: replace_report.blobs_deleted,
            events_imported: replace_report.events_imported,
            events_skipped: 0,
            state_fingerprint: snapshot.state_fingerprint(),
            event_fingerprint: snapshot.event_fingerprint(),
        })
    }

    fn json_get<T>(&self, namespace: &str, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.engine.get_json_value(namespace, key)? else {
            admit_store_json_address(namespace, key, "store_json_decode")?;
            return Ok(None);
        };
        admit_store_json_document(namespace, key, &value, "store_json_decode")?;
        serde_json::from_value(value)
            .map(Some)
            .map_err(|error| Error::config("store_json_decode", error.to_string()))
    }

    fn load_typed_recall_index<T: TypedRecallIndex>(
        &self,
        physical_key: &str,
    ) -> Result<(Option<T>, Option<serde_json::Value>)> {
        let value = self.engine.get_json_value(T::NAMESPACE, physical_key)?;
        let index = value
            .clone()
            .map(|value| decode_typed_recall_index::<T>(physical_key, value))
            .transpose()?;
        Ok((index, value))
    }

    fn commit_recall_indexed_mutations(
        &self,
        operation: &str,
        scope: StoreEventScope,
        owner_mutations: Vec<StoreMutation>,
        indexes: Vec<(
            &'static str,
            String,
            serde_json::Value,
            Option<serde_json::Value>,
        )>,
    ) -> Result<StoreMutationBatchReport> {
        self.commit_recall_indexed_mutations_at(operation, scope, owner_mutations, indexes, None)
    }

    fn commit_recall_indexed_mutations_at(
        &self,
        operation: &str,
        scope: StoreEventScope,
        mut owner_mutations: Vec<StoreMutation>,
        indexes: Vec<RecallIndexMutationPlan>,
        runtime_timestamp_unix_secs: Option<u64>,
    ) -> Result<StoreMutationBatchReport> {
        let mut preconditions = Vec::with_capacity(indexes.len());
        for (namespace, key, value, before) in indexes {
            preconditions.push(match before {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace: namespace.to_string(),
                    key: key.clone(),
                    value,
                },
                None => StoreJsonPrecondition::Absent {
                    namespace: namespace.to_string(),
                    key: key.clone(),
                },
            });
            owner_mutations.push(StoreMutation::PutJson {
                namespace: namespace.to_string(),
                key: key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: namespace.to_string(),
                record_key: key,
            });
        }
        self.commit_governed_memory_transaction_authorized(
            StoreMutationBatch {
                transaction_id: format!("recall-index:{operation}:{}", current_unix_nanos()),
                operation: operation.to_string(),
                scope,
                mutations: owner_mutations,
            },
            StoreCommitPreconditions::new(&preconditions, &[]),
            None,
            None,
            runtime_timestamp_unix_secs,
            None,
        )
    }

    pub(crate) fn plan_world_sense_set(
        &self,
        scope_id: &str,
        value: &WorldSense,
    ) -> Result<StoreOwnerMutationPlan> {
        self.plan_typed_json_owner_put("world_sense", scope_id, value)
    }

    pub(crate) fn plan_world_sense_clear(&self, scope_id: &str) -> Result<StoreOwnerMutationPlan> {
        self.plan_json_owner_delete("world_sense", scope_id)
    }

    pub(crate) fn plan_continuity_capsule_upserts(
        &self,
        drafts: &[ContinuityCapsuleDraft],
        now_secs: u64,
    ) -> Result<StoreOwnerMutationPlan> {
        let mut plan = StoreOwnerMutationPlan::default();
        let mut staged_indexes = BTreeMap::<
            String,
            (
                Option<serde_json::Value>,
                Option<ContinuityCapsuleScopeIndex>,
            ),
        >::new();
        for draft in drafts {
            let capsule_id = stable_hash_id(
                "cc",
                &(
                    draft.kind.label(),
                    draft.scope_kind.label(),
                    &draft.scope_id,
                    &draft.topic,
                ),
            );
            let capsule = ContinuityCapsule {
                capsule_id: capsule_id.clone(),
                kind: draft.kind,
                scope_kind: draft.scope_kind,
                scope_id: draft.scope_id.clone(),
                source_chat_id: draft.source_chat_id.clone(),
                source_channel: draft.source_channel.clone(),
                run_id: draft.run_id.clone(),
                topic: draft.topic.clone(),
                summary: draft.summary.clone(),
                outcome: draft.outcome.clone(),
                decisions: draft.decisions.clone(),
                next_step: draft.next_step.clone(),
                unresolved: draft.unresolved.clone(),
                artifact_refs: draft.artifact_refs.clone(),
                provenance_refs: draft.provenance_refs.clone(),
                source: draft.source,
                status: draft.status,
                supersedes: Vec::new(),
                observed_at: if draft.observed_at > 0 {
                    draft.observed_at
                } else {
                    now_secs
                },
                updated_at: now_secs,
            };
            if !capsule.is_meaningful() {
                continue;
            }
            let owner_before = self
                .engine
                .get_json_value("continuity_capsule", &capsule_id)?;
            push_exact_json_precondition(
                &mut plan.preconditions,
                "continuity_capsule",
                &capsule_id,
                owner_before.clone(),
            )?;
            let scope_kind = capsule.scope_kind.label();
            let manifest_key = ContinuityCapsuleScopeIndex::build(
                1,
                &self.config.event_scope.memory_space_id,
                scope_kind,
                &capsule.scope_id,
                std::iter::empty(),
            )?
            .physical_key;
            if !staged_indexes.contains_key(&manifest_key) {
                let before = self
                    .engine
                    .get_json_value(ContinuityCapsuleScopeIndex::NAMESPACE, &manifest_key)?;
                let current = before
                    .clone()
                    .map(|value| {
                        decode_typed_recall_index::<ContinuityCapsuleScopeIndex>(
                            &manifest_key,
                            value,
                        )
                    })
                    .transpose()?;
                if owner_before.is_some() && current.is_none() {
                    return Err(Error::config(
                        "continuity_capsule_scope_index",
                        "capsule exists without its required scope index",
                    ));
                }
                staged_indexes.insert(manifest_key.clone(), (before, current));
            }
            let (_, current) = staged_indexes
                .get_mut(&manifest_key)
                .expect("staged continuity index exists");
            let previous_entries = current
                .as_ref()
                .map(|index| index.entries.as_slice())
                .unwrap_or(&[]);
            let value = serde_json::to_value(&capsule).map_err(|error| {
                Error::config("continuity_capsule_scope_index", error.to_string())
            })?;
            let address = RecallIndexAddress::json(
                "continuity_capsule",
                &capsule_id,
                next_entry_revision(
                    previous_entries,
                    RecallIndexAddressKind::Json,
                    "continuity_capsule",
                    &capsule_id,
                ),
                capsule.updated_at,
                &value,
            )?;
            *current = Some(ContinuityCapsuleScopeIndex::build(
                current
                    .as_ref()
                    .map(|index| index.revision.saturating_add(1))
                    .unwrap_or(1),
                &self.config.event_scope.memory_space_id,
                scope_kind,
                &capsule.scope_id,
                replace_recall_index_address(previous_entries, address),
            )?);
            plan.mutations.push(StoreMutation::PutJson {
                namespace: "continuity_capsule".to_string(),
                key: capsule_id.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "continuity_capsule".to_string(),
                record_key: capsule_id,
            });
        }
        for (key, (before, current)) in staged_indexes {
            push_exact_json_precondition(
                &mut plan.preconditions,
                ContinuityCapsuleScopeIndex::NAMESPACE,
                &key,
                before,
            )?;
            let value = serde_json::to_value(current.expect("staged index has a post-image"))
                .map_err(|error| {
                    Error::config("continuity_capsule_scope_index", error.to_string())
                })?;
            plan.mutations.push(StoreMutation::PutJson {
                namespace: ContinuityCapsuleScopeIndex::NAMESPACE.to_string(),
                key: key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: ContinuityCapsuleScopeIndex::NAMESPACE.to_string(),
                record_key: key,
            });
        }
        Ok(plan)
    }

    pub(crate) fn plan_task_learning_upsert(
        &self,
        record: &TaskLearningRecord,
    ) -> Result<StoreOwnerMutationPlan> {
        let owner_key = record.learning_id.clone();
        let previous_value = self.engine.get_json_value("task_learning", &owner_key)?;
        let previous_record = previous_value
            .clone()
            .map(serde_json::from_value::<TaskLearningRecord>)
            .transpose()
            .map_err(|error| Error::config("task_learning_by_chat_index", error.to_string()))?;
        let value = serde_json::to_value(record)
            .map_err(|error| Error::config("task_learning_by_chat_index", error.to_string()))?;
        let mut plan = StoreOwnerMutationPlan::default();
        push_exact_json_precondition(
            &mut plan.preconditions,
            "task_learning",
            &owner_key,
            previous_value,
        )?;
        let mut indexes =
            BTreeMap::<String, (Option<serde_json::Value>, Option<TaskLearningByChatIndex>)>::new();
        if let Some(previous) = previous_record.as_ref().filter(|previous| {
            previous.source_channel != record.source_channel
                || previous.source_chat_id != record.source_chat_id
        }) {
            let key = TaskLearningByChatIndex::build(
                1,
                &self.config.event_scope.memory_space_id,
                &previous.source_channel,
                &previous.source_chat_id,
                std::iter::empty(),
            )?
            .physical_key;
            let before = self
                .engine
                .get_json_value(TaskLearningByChatIndex::NAMESPACE, &key)?;
            let current = before
                .clone()
                .map(|value| decode_typed_recall_index::<TaskLearningByChatIndex>(&key, value))
                .transpose()?
                .ok_or_else(|| {
                    Error::config(
                        "task_learning_by_chat_index",
                        "task learning exists without its prior chat index",
                    )
                })?;
            let next = TaskLearningByChatIndex::build(
                current.revision.saturating_add(1),
                &self.config.event_scope.memory_space_id,
                &previous.source_channel,
                &previous.source_chat_id,
                remove_recall_index_address(
                    &current.entries,
                    RecallIndexAddressKind::Json,
                    "task_learning",
                    &owner_key,
                ),
            )?;
            indexes.insert(key, (before, Some(next)));
        }
        let new_key = TaskLearningByChatIndex::build(
            1,
            &self.config.event_scope.memory_space_id,
            &record.source_channel,
            &record.source_chat_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (before, current) = if let Some(staged) = indexes.remove(&new_key) {
            staged
        } else {
            let before = self
                .engine
                .get_json_value(TaskLearningByChatIndex::NAMESPACE, &new_key)?;
            let current = before
                .clone()
                .map(|value| decode_typed_recall_index::<TaskLearningByChatIndex>(&new_key, value))
                .transpose()?;
            (before, current)
        };
        if previous_record.is_some() && current.is_none() {
            return Err(Error::config(
                "task_learning_by_chat_index",
                "task learning exists without its required chat index",
            ));
        }
        let previous_entries = current
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "task_learning",
            &owner_key,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                "task_learning",
                &owner_key,
            ),
            record.observed_at,
            &value,
        )?;
        let next = TaskLearningByChatIndex::build(
            current
                .as_ref()
                .map(|index| index.revision.saturating_add(1))
                .unwrap_or(1),
            &self.config.event_scope.memory_space_id,
            &record.source_channel,
            &record.source_chat_id,
            replace_recall_index_address(previous_entries, address),
        )?;
        indexes.insert(new_key, (before, Some(next)));
        for (key, (before, next)) in indexes {
            push_exact_json_precondition(
                &mut plan.preconditions,
                TaskLearningByChatIndex::NAMESPACE,
                &key,
                before,
            )?;
            plan.mutations.push(StoreMutation::PutJson {
                namespace: TaskLearningByChatIndex::NAMESPACE.to_string(),
                key: key.clone(),
                value: serde_json::to_value(next.expect("staged task-learning index exists"))
                    .map_err(|error| {
                        Error::config("task_learning_by_chat_index", error.to_string())
                    })?,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: TaskLearningByChatIndex::NAMESPACE.to_string(),
                record_key: key,
            });
        }
        plan.mutations.push(StoreMutation::PutJson {
            namespace: "task_learning".to_string(),
            key: owner_key.clone(),
            value,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "task_learning".to_string(),
            record_key: owner_key,
        });
        Ok(plan)
    }

    pub(crate) fn plan_task_artifact_put(
        &self,
        record: &TaskArtifactRecord,
    ) -> Result<StoreOwnerMutationPlan> {
        self.plan_typed_json_owner_put(
            "task_artifact",
            &triple_key("", &record.artifact.run_id, &record.artifact.artifact_id),
            record,
        )
    }

    pub(crate) fn plan_task_artifact_delete(
        &self,
        run_id: &str,
        artifact_id: &str,
    ) -> Result<StoreOwnerMutationPlan> {
        self.plan_json_owner_delete("task_artifact", &triple_key("", run_id, artifact_id))
    }

    pub(crate) fn plan_runtime_skill_write(
        &self,
        name: &str,
        content: &[u8],
    ) -> Result<StoreOwnerMutationPlan> {
        let before = self.engine.get_blob("skills", name)?;
        Ok(StoreOwnerMutationPlan {
            mutations: vec![StoreMutation::PutBlob {
                namespace: "skills".to_string(),
                key: name.to_string(),
                value: content.to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skills".to_string(),
                record_key: name.to_string(),
            }],
            preconditions: Vec::new(),
            blob_preconditions: vec![blob_precondition("skills", name, before.as_deref())],
        })
    }

    pub(crate) fn plan_runtime_skill_remove(&self, name: &str) -> Result<StoreOwnerMutationPlan> {
        let before = self.engine.get_blob("skills", name)?;
        Ok(StoreOwnerMutationPlan {
            mutations: before
                .as_ref()
                .map(|_| StoreMutation::DeleteBlob {
                    namespace: "skills".to_string(),
                    key: name.to_string(),
                    event_kind: MemoryStoreEventKind::MemoryDelete,
                    plane: "skills".to_string(),
                    record_key: name.to_string(),
                })
                .into_iter()
                .collect(),
            preconditions: Vec::new(),
            blob_preconditions: vec![blob_precondition("skills", name, before.as_deref())],
        })
    }

    pub(crate) fn plan_legacy_memory_set(
        &self,
        content: &str,
        now_secs: u64,
    ) -> Result<StoreOwnerMutationPlan> {
        self.plan_archive_blob_put("memory", "MEMORY.md", content.as_bytes(), now_secs)
    }

    pub(crate) fn plan_daily_note_write(
        &self,
        name: &str,
        content: &str,
        now_secs: u64,
    ) -> Result<StoreOwnerMutationPlan> {
        self.plan_archive_blob_put("daily", name, content.as_bytes(), now_secs)
    }

    fn plan_typed_json_owner_put<T: Serialize>(
        &self,
        namespace: &str,
        key: &str,
        value: &T,
    ) -> Result<StoreOwnerMutationPlan> {
        let value = serde_json::to_value(value)
            .map_err(|error| Error::config("store_owner_mutation_plan", error.to_string()))?;
        let before = self.engine.get_json_value(namespace, key)?;
        let mut preconditions = Vec::new();
        push_exact_json_precondition(&mut preconditions, namespace, key, before)?;
        Ok(StoreOwnerMutationPlan {
            mutations: vec![StoreMutation::PutJson {
                namespace: namespace.to_string(),
                key: key.to_string(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: namespace.to_string(),
                record_key: key.to_string(),
            }],
            preconditions,
            blob_preconditions: Vec::new(),
        })
    }

    fn plan_json_owner_delete(&self, namespace: &str, key: &str) -> Result<StoreOwnerMutationPlan> {
        let before = self.engine.get_json_value(namespace, key)?;
        let mut preconditions = Vec::new();
        push_exact_json_precondition(&mut preconditions, namespace, key, before.clone())?;
        Ok(StoreOwnerMutationPlan {
            mutations: before
                .map(|_| StoreMutation::DeleteJson {
                    namespace: namespace.to_string(),
                    key: key.to_string(),
                    event_kind: MemoryStoreEventKind::MemoryDelete,
                    plane: namespace.to_string(),
                    record_key: key.to_string(),
                })
                .into_iter()
                .collect(),
            preconditions,
            blob_preconditions: Vec::new(),
        })
    }

    fn plan_archive_blob_put(
        &self,
        namespace: &str,
        key: &str,
        value: &[u8],
        now_secs: u64,
    ) -> Result<StoreOwnerMutationPlan> {
        let manifest_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, before) =
            self.load_typed_recall_index::<ArchiveRecallManifest>(&manifest_key)?;
        let before_blob = self.engine.get_blob(namespace, key)?;
        if before_blob.is_some() && previous.is_none() {
            return Err(Error::config(
                "archive_recall_manifest",
                "archive blob exists without its required recall manifest",
            ));
        }
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::blob(
            namespace,
            key,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Blob,
                namespace,
                key,
            ),
            now_secs,
            value,
        )?;
        let next = ArchiveRecallManifest::build(
            previous
                .as_ref()
                .map(|index| index.revision.saturating_add(1))
                .unwrap_or(1),
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            replace_recall_index_address(previous_entries, address),
        )?;
        let mut preconditions = Vec::new();
        push_exact_json_precondition(
            &mut preconditions,
            ArchiveRecallManifest::NAMESPACE,
            &manifest_key,
            before,
        )?;
        Ok(StoreOwnerMutationPlan {
            mutations: vec![
                StoreMutation::PutBlob {
                    namespace: namespace.to_string(),
                    key: key.to_string(),
                    value: value.to_vec(),
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: namespace.to_string(),
                    record_key: key.to_string(),
                },
                StoreMutation::PutJson {
                    namespace: ArchiveRecallManifest::NAMESPACE.to_string(),
                    key: manifest_key.clone(),
                    value: serde_json::to_value(next).map_err(|error| {
                        Error::config("archive_recall_manifest", error.to_string())
                    })?,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: ArchiveRecallManifest::NAMESPACE.to_string(),
                    record_key: manifest_key,
                },
            ],
            preconditions,
            blob_preconditions: vec![blob_precondition(namespace, key, before_blob.as_deref())],
        })
    }

    fn recall_scope(&self) -> StoreEventScope {
        self.config.event_scope.clone()
    }

    fn plan_archive_index_upsert(
        &self,
        address: RecallIndexAddress,
    ) -> Result<RecallIndexMutationPlan> {
        self.plan_archive_index_upsert_for_scope(
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            address,
        )
    }

    fn plan_archive_index_upsert_for_scope(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        address: RecallIndexAddress,
    ) -> Result<RecallIndexMutationPlan> {
        let key = ArchiveRecallManifest::build(
            1,
            memory_space_id,
            mounted_subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, before) = self.load_typed_recall_index::<ArchiveRecallManifest>(&key)?;
        let revision = previous
            .as_ref()
            .map(|index| index.revision.saturating_add(1))
            .unwrap_or(1);
        let entries = replace_recall_index_address(
            previous
                .as_ref()
                .map(|index| index.entries.as_slice())
                .unwrap_or(&[]),
            address,
        );
        let next =
            ArchiveRecallManifest::build(revision, memory_space_id, mounted_subject_id, entries)?;
        Ok((
            ArchiveRecallManifest::NAMESPACE,
            key,
            serde_json::to_value(next)
                .map_err(|error| Error::config("archive_recall_manifest", error.to_string()))?,
            before,
        ))
    }

    fn plan_archive_index_remove(
        &self,
        kind: RecallIndexAddressKind,
        namespace: &str,
        owner_key: &str,
    ) -> Result<RecallIndexMutationPlan> {
        let scope = &self.config.event_scope;
        let key = ArchiveRecallManifest::build(
            1,
            &scope.memory_space_id,
            &scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, before) = self.load_typed_recall_index::<ArchiveRecallManifest>(&key)?;
        let previous = previous.ok_or_else(|| {
            Error::config(
                "archive_recall_manifest",
                "owner exists without its required archive recall manifest",
            )
        })?;
        let entries = remove_recall_index_address(&previous.entries, kind, namespace, owner_key);
        let next = ArchiveRecallManifest::build(
            previous.revision.saturating_add(1),
            &scope.memory_space_id,
            &scope.subject_id,
            entries,
        )?;
        Ok((
            ArchiveRecallManifest::NAMESPACE,
            key,
            serde_json::to_value(next)
                .map_err(|error| Error::config("archive_recall_manifest", error.to_string()))?,
            before,
        ))
    }

    fn put_archive_json_owner<T: Serialize>(
        &self,
        operation: &str,
        namespace: &str,
        key: &str,
        value: &T,
    ) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_json_owner_write")?;
        let value = serde_json::to_value(value)
            .map_err(|error| Error::config("archive_recall_manifest", error.to_string()))?;
        let root_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&root_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            namespace,
            key,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                namespace,
                key,
            ),
            current_unix_secs(),
            &value,
        )?;
        let index = self.plan_archive_index_upsert(address)?;
        self.commit_recall_indexed_mutations(
            operation,
            self.recall_scope(),
            vec![StoreMutation::PutJson {
                namespace: namespace.to_string(),
                key: key.to_string(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: namespace.to_string(),
                record_key: key.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn delete_archive_json_owner(&self, operation: &str, namespace: &str, key: &str) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_json_owner_delete")?;
        if self.engine.get_json_value(namespace, key)?.is_none() {
            return Ok(());
        }
        let index = self.plan_archive_index_remove(RecallIndexAddressKind::Json, namespace, key)?;
        self.commit_recall_indexed_mutations(
            operation,
            self.recall_scope(),
            vec![StoreMutation::DeleteJson {
                namespace: namespace.to_string(),
                key: key.to_string(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: namespace.to_string(),
                record_key: key.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn put_archive_blob_owner(
        &self,
        operation: &str,
        namespace: &str,
        key: &str,
        value: &[u8],
    ) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_blob_owner_write")?;
        let root_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&root_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::blob(
            namespace,
            key,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Blob,
                namespace,
                key,
            ),
            current_unix_secs(),
            value,
        )?;
        let index = self.plan_archive_index_upsert(address)?;
        self.commit_recall_indexed_mutations(
            operation,
            self.recall_scope(),
            vec![StoreMutation::PutBlob {
                namespace: namespace.to_string(),
                key: key.to_string(),
                value: value.to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: namespace.to_string(),
                record_key: key.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn append_conversation_derived_ref_recall_index_closure(
        &self,
        batch: &mut StoreMutationBatch,
        preconditions: &mut Vec<StoreJsonPrecondition>,
        canonical_updated_at: u64,
    ) -> Result<()> {
        let mut groups =
            BTreeMap::<(String, String, String, String), Vec<(String, serde_json::Value)>>::new();
        for mutation in &batch.mutations {
            let StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } = mutation
            else {
                continue;
            };
            if namespace != "conversation_transcript_derived_ref" {
                continue;
            }
            let derived =
                serde_json::from_value::<DerivedMemoryRef>(value.clone()).map_err(|error| {
                    Error::config("conversation_recall_manifest", error.to_string())
                })?;
            let subject_id = derived
                .subject_id
                .as_deref()
                .or(derived.source.subject_id.as_deref())
                .ok_or_else(|| {
                    Error::config(
                        "conversation_recall_manifest",
                        "derived memory ref requires an exact subject owner",
                    )
                })?;
            if derived
                .subject_id
                .as_deref()
                .zip(derived.source.subject_id.as_deref())
                .is_some_and(|(owner, source_owner)| owner != source_owner)
                || derived.source.memory_space_id != batch.scope.memory_space_id
                || subject_id != batch.scope.subject_id
            {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "derived memory ref owner does not match the transaction scope",
                ));
            }
            groups
                .entry((
                    derived.source.memory_space_id,
                    derived.source.channel_id,
                    derived.source.conversation_id,
                    derived.source.turn_id,
                ))
                .or_default()
                .push((key.clone(), value.clone()));
        }

        for ((memory_space_id, channel_id, conversation_id, turn_id), owners) in groups {
            let conversation_key =
                ConversationKey::new(memory_space_id, channel_id, conversation_id)?;
            let manifest_key = ConversationTranscriptAuxManifest::build(
                1,
                &conversation_key.memory_space_id,
                &batch.scope.subject_id,
                &conversation_key.channel_id,
                &conversation_key.conversation_id,
                &turn_id,
                std::iter::empty(),
            )?
            .physical_key;
            let existing_position = batch.mutations.iter().position(|mutation| {
                matches!(mutation,
                    StoreMutation::PutJson { namespace, key, .. }
                        if namespace == ConversationTranscriptAuxManifest::NAMESPACE
                            && key == &manifest_key)
            });

            let (before, revision, mut entries) = if let Some(position) = existing_position {
                let StoreMutation::PutJson { value, .. } = &batch.mutations[position] else {
                    unreachable!("matched conversation recall manifest put")
                };
                let pending = decode_typed_recall_index::<ConversationTranscriptAuxManifest>(
                    &manifest_key,
                    value.clone(),
                )?;
                (None, pending.revision, pending.entries)
            } else {
                let (previous, before) = self
                    .load_typed_recall_index::<ConversationTranscriptAuxManifest>(&manifest_key)?;
                let revision = previous
                    .as_ref()
                    .map(|index| index.revision.saturating_add(1))
                    .unwrap_or(1);
                let entries = previous
                    .as_ref()
                    .map(|index| index.entries.clone())
                    .unwrap_or_default();
                (before, revision, entries)
            };

            for (owner_key, value) in owners {
                let address = RecallIndexAddress::json(
                    "conversation_transcript_derived_ref",
                    &owner_key,
                    next_entry_revision(
                        &entries,
                        RecallIndexAddressKind::Json,
                        "conversation_transcript_derived_ref",
                        &owner_key,
                    ),
                    canonical_updated_at,
                    &value,
                )?;
                entries = replace_recall_index_address(&entries, address);
            }
            let next = ConversationTranscriptAuxManifest::build(
                revision,
                &conversation_key.memory_space_id,
                &batch.scope.subject_id,
                &conversation_key.channel_id,
                &conversation_key.conversation_id,
                &turn_id,
                entries,
            )?;
            let next_value = serde_json::to_value(next)
                .map_err(|error| Error::config("conversation_transcript_aux", error.to_string()))?;
            if let Some(position) = existing_position {
                let StoreMutation::PutJson { value, .. } = &mut batch.mutations[position] else {
                    unreachable!("matched conversation recall manifest put")
                };
                *value = next_value;
            } else {
                preconditions.push(match before {
                    Some(value) => StoreJsonPrecondition::Exact {
                        namespace: ConversationTranscriptAuxManifest::NAMESPACE.to_string(),
                        key: manifest_key.clone(),
                        value,
                    },
                    None => StoreJsonPrecondition::Absent {
                        namespace: ConversationTranscriptAuxManifest::NAMESPACE.to_string(),
                        key: manifest_key.clone(),
                    },
                });
                batch.mutations.push(StoreMutation::PutJson {
                    namespace: ConversationTranscriptAuxManifest::NAMESPACE.to_string(),
                    key: manifest_key.clone(),
                    value: next_value,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: ConversationTranscriptAuxManifest::NAMESPACE.to_string(),
                    record_key: manifest_key,
                });
            }
        }
        Ok(())
    }

    fn load_conversation_recall_manifest(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
    ) -> Result<(
        String,
        Option<ConversationRecallManifest>,
        Option<serde_json::Value>,
    )> {
        let physical_key = Self::conversation_head_physical_key(key, mounted_subject_id)?;
        let value = self
            .engine
            .get_json_value(ConversationRecallManifest::NAMESPACE, &physical_key)?;
        let manifest = value
            .clone()
            .map(|value| {
                decode_typed_recall_index::<ConversationRecallManifest>(&physical_key, value)
            })
            .transpose()?;
        Ok((physical_key, manifest, value))
    }

    fn conversation_head_physical_key(
        key: &ConversationKey,
        mounted_subject_id: &str,
    ) -> Result<String> {
        Ok(ConversationRecallManifest::build(
            1,
            &key.memory_space_id,
            mounted_subject_id,
            &key.channel_id,
            &key.conversation_id,
            0,
            0,
        )?
        .physical_key)
    }

    fn validate_conversation_manifest_subject(
        &self,
        manifest: &ConversationRecallManifest,
        mounted_subject_id: &str,
    ) -> Result<()> {
        if manifest.mounted_subject_id != mounted_subject_id {
            return Err(Error::config(
                "conversation_recall_manifest",
                "conversation manifest subject differs from the requested owner",
            ));
        }
        Ok(())
    }

    fn plan_conversation_head(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        previous: Option<&ConversationRecallManifest>,
        before: Option<serde_json::Value>,
        turn_count: u64,
        last_sequence: u64,
    ) -> Result<RecallIndexMutationPlan> {
        let next = ConversationRecallManifest::build(
            previous
                .map(|manifest| manifest.revision.saturating_add(1))
                .unwrap_or(1),
            &key.memory_space_id,
            mounted_subject_id,
            &key.channel_id,
            &key.conversation_id,
            turn_count,
            last_sequence,
        )?;
        let physical_key = next.physical_key.clone();
        Ok((
            ConversationRecallManifest::NAMESPACE,
            physical_key,
            serde_json::to_value(next).map_err(|error| {
                Error::config("conversation_recall_manifest", error.to_string())
            })?,
            before,
        ))
    }

    fn conversation_page_id(page_id: u64) -> String {
        format!("{page_id:020}")
    }

    fn load_conversation_transcript_page(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        page_id: u64,
    ) -> Result<(
        String,
        Option<ConversationTranscriptPageIndex>,
        Option<serde_json::Value>,
    )> {
        let page_id = Self::conversation_page_id(page_id);
        let physical_key = ConversationTranscriptPageIndex::build(
            1,
            &key.memory_space_id,
            mounted_subject_id,
            &key.channel_id,
            &key.conversation_id,
            &page_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (page, value) =
            self.load_typed_recall_index::<ConversationTranscriptPageIndex>(&physical_key)?;
        Ok((physical_key, page, value))
    }

    fn plan_conversation_transcript_page(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        page_id: u64,
        previous: Option<&ConversationTranscriptPageIndex>,
        before: Option<serde_json::Value>,
        entries: Vec<RecallIndexAddress>,
    ) -> Result<RecallIndexMutationPlan> {
        let page_id = Self::conversation_page_id(page_id);
        let next = ConversationTranscriptPageIndex::build(
            previous
                .map(|page| page.revision.saturating_add(1))
                .unwrap_or(1),
            &key.memory_space_id,
            mounted_subject_id,
            &key.channel_id,
            &key.conversation_id,
            &page_id,
            entries,
        )?;
        Ok((
            ConversationTranscriptPageIndex::NAMESPACE,
            next.physical_key.clone(),
            serde_json::to_value(next).map_err(|error| {
                Error::config("conversation_transcript_page", error.to_string())
            })?,
            before,
        ))
    }

    fn load_conversation_transcript_aux(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: &str,
    ) -> Result<(
        String,
        Option<ConversationTranscriptAuxManifest>,
        Option<serde_json::Value>,
    )> {
        let physical_key = ConversationTranscriptAuxManifest::build(
            1,
            &key.memory_space_id,
            mounted_subject_id,
            &key.channel_id,
            &key.conversation_id,
            turn_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (manifest, value) =
            self.load_typed_recall_index::<ConversationTranscriptAuxManifest>(&physical_key)?;
        Ok((physical_key, manifest, value))
    }

    fn plan_conversation_transcript_aux(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: &str,
        previous: Option<&ConversationTranscriptAuxManifest>,
        before: Option<serde_json::Value>,
        entries: Vec<RecallIndexAddress>,
    ) -> Result<RecallIndexMutationPlan> {
        let next = ConversationTranscriptAuxManifest::build(
            previous
                .map(|manifest| manifest.revision.saturating_add(1))
                .unwrap_or(1),
            &key.memory_space_id,
            mounted_subject_id,
            &key.channel_id,
            &key.conversation_id,
            turn_id,
            entries,
        )?;
        Ok((
            ConversationTranscriptAuxManifest::NAMESPACE,
            next.physical_key.clone(),
            serde_json::to_value(next)
                .map_err(|error| Error::config("conversation_transcript_aux", error.to_string()))?,
            before,
        ))
    }

    fn conversation_page_for_sequence(sequence: u64) -> Result<u64> {
        if sequence == 0 {
            return Err(Error::config(
                "conversation_transcript_page",
                "turn sequence must be greater than zero",
            ));
        }
        let page_size = u64::try_from(CONVERSATION_TRANSCRIPT_PAGE_SIZE).map_err(|_| {
            Error::config(
                "conversation_transcript_page",
                "page size does not fit the sequence domain",
            )
        })?;
        Ok(sequence.saturating_sub(1) / page_size)
    }

    fn load_validated_conversation_page_records(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        head: &ConversationRecallManifest,
        page_id: u64,
    ) -> Result<Vec<TranscriptTurnRecord>> {
        if page_id >= head.page_count {
            return Err(Error::config(
                "conversation_transcript_page",
                "requested page is outside the conversation head",
            ));
        }
        let (_, page, _) =
            self.load_conversation_transcript_page(key, mounted_subject_id, page_id)?;
        let page = page.ok_or_else(|| {
            Error::config(
                "conversation_transcript_page",
                "conversation head references a missing page",
            )
        })?;
        let expected_page_id = Self::conversation_page_id(page_id);
        if page.memory_space_id != key.memory_space_id
            || page.mounted_subject_id != mounted_subject_id
            || page.channel_id != key.channel_id
            || page.conversation_id != key.conversation_id
            || page.page_id != expected_page_id
        {
            return Err(Error::config(
                "conversation_transcript_page",
                "page scope differs from the conversation head",
            ));
        }
        let page_size = u64::try_from(CONVERSATION_TRANSCRIPT_PAGE_SIZE).map_err(|_| {
            Error::config(
                "conversation_transcript_page",
                "page size does not fit the sequence domain",
            )
        })?;
        let first_sequence = page_id.saturating_mul(page_size).saturating_add(1);
        let expected_count = if page_id == head.active_page_id {
            head.active_page_entry_count
        } else {
            CONVERSATION_TRANSCRIPT_PAGE_SIZE
        };
        if page.entries.len() != expected_count {
            return Err(Error::config(
                "conversation_transcript_page",
                "page entry count differs from the conversation head",
            ));
        }
        let mut records = Vec::with_capacity(page.entries.len());
        for entry in &page.entries {
            if entry.kind != RecallIndexAddressKind::Json
                || entry.namespace != "conversation_transcript"
            {
                return Err(Error::config(
                    "conversation_transcript_page",
                    "page contains a non-transcript owner",
                ));
            }
            let value = self
                .engine
                .get_json_value("conversation_transcript", &entry.key)?
                .ok_or_else(|| {
                    Error::config(
                        "conversation_transcript_page",
                        "indexed transcript owner is missing",
                    )
                })?;
            let record =
                serde_json::from_value::<TranscriptTurnRecord>(value.clone()).map_err(|error| {
                    Error::config("conversation_transcript_page", error.to_string())
                })?;
            let expected_address = RecallIndexAddress::json(
                "conversation_transcript",
                &entry.key,
                entry.revision,
                entry.updated_at,
                &value,
            )?;
            if expected_address.content_sha256 != entry.content_sha256
                || record.key != *key
                || record.subject != mounted_subject_id
                || transcript_turn_storage_key(key, mounted_subject_id, &record.turn_id)
                    != entry.key
                || Self::conversation_page_for_sequence(record.sequence)? != page_id
            {
                return Err(Error::config(
                    "conversation_transcript_page",
                    "transcript owner binding differs from its page",
                ));
            }
            records.push(record);
        }
        records.sort_by(|left, right| {
            left.sequence
                .cmp(&right.sequence)
                .then_with(|| left.turn_id.cmp(&right.turn_id))
        });
        for (offset, record) in records.iter().enumerate() {
            let expected_sequence =
                first_sequence.saturating_add(u64::try_from(offset).unwrap_or(u64::MAX));
            if record.sequence != expected_sequence {
                return Err(Error::config(
                    "conversation_transcript_page",
                    "page contains a sequence gap or duplicate",
                ));
            }
        }
        Ok(records)
    }

    fn json_put<T>(&self, namespace: &str, key: &str, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        self.json_put_with_event_kind(namespace, key, value, MemoryStoreEventKind::MemoryWrite)
    }

    fn json_put_with_event_kind<T>(
        &self,
        namespace: &str,
        key: &str,
        value: &T,
        event_kind: MemoryStoreEventKind,
    ) -> Result<()>
    where
        T: Serialize,
    {
        self.json_put_with_event_kind_and_record_key(namespace, key, key, value, event_kind)
    }

    fn json_put_with_event_kind_and_record_key<T>(
        &self,
        namespace: &str,
        key: &str,
        record_key: &str,
        value: &T,
        event_kind: MemoryStoreEventKind,
    ) -> Result<()>
    where
        T: Serialize,
    {
        let value = serde_json::to_value(value)
            .map_err(|error| Error::config("store_json_encode", error.to_string()))?;
        let content_hash = stable_hash_json(&value)?;
        let event = self.build_memory_event(event_kind, namespace, record_key, content_hash);
        let admission = self.current_store_transaction_admission()?;
        self.engine
            .put_json_value_and_event(namespace, key, value, event, &admission)
    }

    fn json_delete(&self, namespace: &str, key: &str) -> Result<bool> {
        self.json_delete_with_event_kind(namespace, key, MemoryStoreEventKind::MemoryDelete)
    }

    fn json_delete_with_event_kind(
        &self,
        namespace: &str,
        key: &str,
        event_kind: MemoryStoreEventKind,
    ) -> Result<bool> {
        self.json_delete_with_event_kind_and_record_key(namespace, key, key, event_kind)
    }

    fn json_delete_with_event_kind_and_record_key(
        &self,
        namespace: &str,
        key: &str,
        record_key: &str,
        event_kind: MemoryStoreEventKind,
    ) -> Result<bool> {
        let _transaction_guard = self.lock_transaction("store_json_delete")?;
        enforce_logical_key_budget(self.capacity, namespace, key, "store_json_delete")?;
        let event_template = self.build_memory_event_template(event_kind, namespace, record_key);
        let admission = self.current_store_transaction_admission()?;
        self.engine.delete_json_value_and_materialize_event(
            namespace,
            key,
            event_template,
            &admission,
        )
    }

    fn json_list<T>(&self, namespace: &str, limit: usize) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let mut values = Vec::new();
        for key in self.engine.list_json_keys(namespace)? {
            if limit > 0 && values.len() >= limit {
                break;
            }
            if let Some(value) = self.json_get(namespace, &key)? {
                values.push(value);
            }
        }
        Ok(values)
    }

    fn blob_put(&self, namespace: &str, key: &str, value: &[u8]) -> Result<()> {
        let content_hash = stable_hash_hex(value);
        let event = self.build_memory_event(
            MemoryStoreEventKind::MemoryWrite,
            namespace,
            key,
            content_hash,
        );
        let admission = self.current_store_transaction_admission()?;
        self.engine
            .put_blob_and_event(namespace, key, value, event, &admission)
    }

    fn blob_delete(&self, namespace: &str, key: &str) -> Result<bool> {
        let _transaction_guard = self.lock_transaction("store_blob_delete")?;
        enforce_logical_key_budget(self.capacity, namespace, key, "store_blob_delete")?;
        let event_template =
            self.build_memory_event_template(MemoryStoreEventKind::MemoryDelete, namespace, key);
        let admission = self.current_store_transaction_admission()?;
        self.engine
            .delete_blob_and_materialize_event(namespace, key, event_template, &admission)
    }

    fn build_memory_event(
        &self,
        kind: MemoryStoreEventKind,
        plane: &str,
        record_key: &str,
        content_hash: String,
    ) -> MemoryStoreEvent {
        MemoryStoreEvent::new(
            next_event_id(),
            kind,
            self.config.event_scope.clone(),
            current_unix_secs(),
        )
        .with_plane(plane)
        .with_record_key(record_key)
        .with_content_hash(content_hash)
    }

    fn build_memory_event_template(
        &self,
        kind: MemoryStoreEventKind,
        plane: &str,
        record_key: &str,
    ) -> ConditionalDeleteEventTemplate {
        StoreEngineMutation::conditional_delete_event_template(
            next_event_id(),
            kind,
            self.config.event_scope.clone(),
            current_unix_secs(),
        )
        .with_plane(plane)
        .with_record_key(record_key)
    }

    fn current_store_transaction_admission(&self) -> Result<StoreTransactionAdmission> {
        let runtime_budget = self.current_runtime_budget(current_unix_secs());
        if runtime_budget.resource_snapshot.stale {
            return Err(Error::config(
                "memory_write_transaction_resource_admission",
                "store transaction requires a fresh runtime budget report",
            ));
        }
        self.store_transaction_admission_for_report(&runtime_budget)
    }

    fn store_transaction_admission_for_report(
        &self,
        runtime_budget: &RuntimeBudgetReport,
    ) -> Result<StoreTransactionAdmission> {
        let now_secs = current_unix_secs();
        runtime_budget.validate_for_admission(now_secs)?;
        let owner_report = crate::RuntimeBudgetLease::active_report(&self.runtime_budget_authority)
            .unwrap_or_else(|| self.runtime_budget_authority.current_report(now_secs));
        if owner_report.report_id != runtime_budget.report_id {
            return Err(Error::config(
                "memory_write_transaction_resource_admission",
                "runtime budget report was not issued by the current store authority boundary",
            ));
        }
        StoreTransactionAdmission::from_runtime_budget(
            runtime_budget,
            Arc::clone(&self.runtime_budget_authority),
            self.engine.admission_authority(),
        )
    }

    fn build_batch_event(
        &self,
        batch: &StoreMutationBatch,
        transaction_timestamp: u64,
        kind: MemoryStoreEventKind,
        plane: &str,
        record_key: &str,
        content_hash: String,
    ) -> MemoryStoreEvent {
        self.build_batch_event_in_scope(
            StoreBatchEventContext {
                batch,
                event_scope: batch.scope.clone(),
                transaction_timestamp,
                kind,
                plane,
                record_key,
            },
            content_hash,
        )
    }

    fn build_batch_event_in_scope(
        &self,
        context: StoreBatchEventContext<'_>,
        content_hash: String,
    ) -> MemoryStoreEvent {
        MemoryStoreEvent::new(
            next_event_id(),
            context.kind,
            context.event_scope,
            context.transaction_timestamp,
        )
        .with_plane(context.plane)
        .with_record_key(context.record_key)
        .with_content_hash(content_hash)
        .with_payload("transaction_id", context.batch.transaction_id.as_str())
        .with_payload("operation", context.batch.operation.as_str())
    }

    fn build_batch_event_template(
        &self,
        batch: &StoreMutationBatch,
        transaction_timestamp: u64,
        kind: MemoryStoreEventKind,
        plane: &str,
        record_key: &str,
    ) -> ConditionalDeleteEventTemplate {
        self.build_batch_event_template_in_scope(StoreBatchEventContext {
            batch,
            event_scope: batch.scope.clone(),
            transaction_timestamp,
            kind,
            plane,
            record_key,
        })
    }

    fn build_batch_event_template_in_scope(
        &self,
        context: StoreBatchEventContext<'_>,
    ) -> ConditionalDeleteEventTemplate {
        StoreEngineMutation::conditional_delete_event_template(
            next_event_id(),
            context.kind,
            context.event_scope,
            context.transaction_timestamp,
        )
        .with_plane(context.plane)
        .with_record_key(context.record_key)
        .with_payload("transaction_id", context.batch.transaction_id.as_str())
        .with_payload("operation", context.batch.operation.as_str())
    }

    fn append_validated_event(&self, event: MemoryStoreEvent) -> Result<()> {
        let _transaction_guard = self.lock_transaction("store_event_log")?;
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&self.runtime_budget_authority))?;
        lease.execute(&self.runtime_budget_authority, || {
            let admission = self.store_transaction_admission_for_report(lease.report())?;
            let request = StoreTransactionRequest::new(
                format!("{}:event-only", event.event_id),
                Vec::new(),
                vec![StoreEngineMutation::AppendEvent {
                    event: Box::new(event),
                }],
                None,
            );
            self.engine
                .commit_transaction_admitted(&request, &admission)
                .map(|_| ())
        })
    }

    fn enforce_snapshot_budget(
        &self,
        snapshot: &StoreSnapshot,
        max_bytes: usize,
        operation: &'static str,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(snapshot)
            .map_err(|error| Error::config("store_snapshot_budget", error.to_string()))?;
        if bytes.len() > max_bytes {
            return Err(store_budget_error(format!(
                "{operation} snapshot bytes {} exceed {}",
                bytes.len(),
                max_bytes
            )));
        }
        Ok(())
    }
}

fn mutation_operation_transaction_id(
    identity: &MemoryMutationOperationIdentity,
    intent_digest: &str,
) -> String {
    canonical_mutation_operation_digest(
        b"memory_mutation_transaction_id_v1",
        &[identity.storage_key().as_bytes(), intent_digest.as_bytes()],
    )
}

fn canonical_mutation_operation_digest(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain.len().to_be_bytes());
    hasher.update(domain);
    for field in fields {
        hasher.update(field.len().to_be_bytes());
        hasher.update(field);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn append_mutation_operation_records(
    batch: &mut StoreMutationBatch,
    preconditions: &mut Vec<StoreJsonPrecondition>,
    operation: Option<&StoreMutationOperationPlan>,
    transaction_timestamp: u64,
) -> Result<()> {
    let protected_mutation_present = batch.mutations.iter().any(|mutation| {
        matches!(mutation,
            StoreMutation::PutJson { namespace, .. }
                | StoreMutation::DeleteJson { namespace, .. }
                if matches!(namespace.as_str(), MEMORY_MUTATION_RECEIPT_NAMESPACE | MEMORY_MUTATION_AUDIT_NAMESPACE))
    });
    let Some(operation) = operation else {
        if protected_mutation_present {
            return Err(Error::config(
                "memory_mutation_operation_authority",
                "only the Store operation-aware commit may create mutation receipt records",
            ));
        }
        return Ok(());
    };
    if protected_mutation_present {
        return Err(Error::config(
            "memory_mutation_operation_authority",
            "callers must not provide mutation receipt or audit records",
        ));
    }
    if operation.committed_at_unix_secs != transaction_timestamp
        || batch.transaction_id != operation.transaction_id
        || batch.scope.memory_space_id != operation.identity.memory_space_id()
        || batch.scope.subject_id != operation.identity.mounted_subject_id()
    {
        return Err(Error::invalid_input(
            "memory_mutation_operation_scope",
            "operation plan does not match the final governed batch authority",
        ));
    }
    let encoded_effect_plan = serde_json::to_vec(&(
        &batch.transaction_id,
        &batch.operation,
        &batch.scope,
        &batch.mutations,
    ))
    .map_err(|error| Error::config("memory_mutation_effect_plan_digest", error.to_string()))?;
    let effect_plan_digest = canonical_mutation_operation_digest(
        b"memory_mutation_effect_plan_v1",
        &[&encoded_effect_plan],
    );
    let key = operation.identity.storage_key();
    let audit = MemoryMutationAuditRecord::new(
        operation.identity.clone(),
        &operation.intent_digest,
        &effect_plan_digest,
        &operation.transaction_id,
        operation.effect,
        usize::try_from(operation.changed_count).map_err(|_| {
            Error::invalid_input(
                "memory_mutation_operation_plan",
                "changed_count cannot be represented by this process",
            )
        })?,
        &operation.actor_subject_id,
        operation.committed_at_unix_secs,
    )?;
    let receipt = MemoryMutationReceipt::new(
        operation.identity.clone(),
        &operation.intent_digest,
        &effect_plan_digest,
        &operation.transaction_id,
        operation.effect,
        usize::try_from(operation.changed_count).map_err(|_| {
            Error::invalid_input(
                "memory_mutation_operation_plan",
                "changed_count cannot be represented by this process",
            )
        })?,
        operation.committed_at_unix_secs,
    )?;
    preconditions.push(StoreJsonPrecondition::Absent {
        namespace: MEMORY_MUTATION_AUDIT_NAMESPACE.to_string(),
        key: key.clone(),
    });
    preconditions.push(StoreJsonPrecondition::Absent {
        namespace: MEMORY_MUTATION_RECEIPT_NAMESPACE.to_string(),
        key: key.clone(),
    });
    batch.mutations.push(StoreMutation::PutJson {
        namespace: MEMORY_MUTATION_AUDIT_NAMESPACE.to_string(),
        key: key.clone(),
        value: serde_json::to_value(audit)
            .map_err(|error| Error::config("memory_mutation_audit_encode", error.to_string()))?,
        event_kind: MemoryStoreEventKind::MemoryControl,
        plane: MEMORY_MUTATION_AUDIT_NAMESPACE.to_string(),
        record_key: key.clone(),
    });
    batch.mutations.push(StoreMutation::PutJson {
        namespace: MEMORY_MUTATION_RECEIPT_NAMESPACE.to_string(),
        key: key.clone(),
        value: serde_json::to_value(receipt)
            .map_err(|error| Error::config("memory_mutation_receipt_encode", error.to_string()))?,
        event_kind: MemoryStoreEventKind::MemoryControl,
        plane: MEMORY_MUTATION_RECEIPT_NAMESPACE.to_string(),
        record_key: key,
    });
    Ok(())
}

fn validate_mutation_operation_closure(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
    operation: Option<&StoreMutationOperationPlan>,
) -> Result<()> {
    let receipt_values = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == MEMORY_MUTATION_RECEIPT_NAMESPACE => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    let audit_values = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == MEMORY_MUTATION_AUDIT_NAMESPACE => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    let Some(operation) = operation else {
        if receipt_values.is_empty() && audit_values.is_empty() {
            return Ok(());
        }
        return Err(Error::config(
            "memory_mutation_operation_authority",
            "mutation receipt records require Store operation authority",
        ));
    };
    if receipt_values.len() != 1 || audit_values.len() != 1 {
        return Err(Error::config(
            "memory_mutation_operation_closure",
            "one operation transaction requires exactly one receipt and one audit",
        ));
    }
    let receipt = serde_json::from_value::<MemoryMutationReceipt>((*receipt_values[0]).clone())
        .map_err(|error| Error::config("memory_mutation_operation_closure", error.to_string()))?;
    let audit = serde_json::from_value::<MemoryMutationAuditRecord>((*audit_values[0]).clone())
        .map_err(|error| Error::config("memory_mutation_operation_closure", error.to_string()))?;
    let key = operation.identity.storage_key();
    let absent = |namespace: &str| {
        preconditions.iter().any(|precondition| {
            matches!(precondition,
                StoreJsonPrecondition::Absent { namespace: actual_namespace, key: actual_key }
                    if actual_namespace == namespace && actual_key == &key)
        })
    };
    if !absent(MEMORY_MUTATION_RECEIPT_NAMESPACE)
        || !absent(MEMORY_MUTATION_AUDIT_NAMESPACE)
        || receipt.identity != operation.identity
        || receipt.intent_digest != operation.intent_digest
        || receipt.transaction_id != operation.transaction_id
        || receipt.effect != operation.effect
        || receipt.changed_count != operation.changed_count
        || receipt.audit_record_id != key
        || audit.audit_record_id != receipt.audit_record_id
        || audit.identity != receipt.identity
        || audit.intent_digest != receipt.intent_digest
        || audit.effect_plan_digest != receipt.effect_plan_digest
        || audit.transaction_id != receipt.transaction_id
        || audit.effect != receipt.effect
        || audit.changed_count != receipt.changed_count
        || audit.actor_subject_id != operation.actor_subject_id
        || audit.committed_at_unix_secs != receipt.committed_at_unix_secs
    {
        return Err(Error::config(
            "memory_mutation_operation_closure",
            "mutation receipt and authoritative audit are not an exact operation binding",
        ));
    }
    Ok(())
}

fn validate_protected_json_mutation_preconditions(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
    operation_authorized: bool,
) -> Result<()> {
    const PROTECTED_NAMESPACES: &[&str] = &[
        crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
        crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
        crate::store_internal::schema::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
        crate::store_internal::schema::RUNTIME_SKILL_RECORD_NAMESPACE,
        crate::store_internal::schema::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
        GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
        GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
        MEMORY_FACET_INDEX_NAMESPACE,
        MEMORY_FACET_POSTING_NAMESPACE,
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
        "memory_graph_manifests",
        CONVERSATION_RECALL_MANIFEST_NAMESPACE,
        ARCHIVE_RECALL_MANIFEST_NAMESPACE,
        CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE,
        ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE,
        TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
        RECALL_OWNER_SCOPE_BINDING_NAMESPACE,
        POST_TURN_GOVERNANCE_JOB_NAMESPACE,
        POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
        MEMORY_MUTATION_RECEIPT_NAMESPACE,
        MEMORY_MUTATION_AUDIT_NAMESPACE,
        crate::store_internal::schema::SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE,
        crate::store_internal::schema::SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE,
        crate::store_internal::schema::SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
        crate::store_internal::schema::SUBJECT_SOUL_GENERATION_TOMBSTONE_NAMESPACE,
        crate::store_internal::schema::RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE,
        crate::store_internal::schema::RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE,
        crate::store_internal::schema::RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE,
        crate::store_internal::schema::SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE,
        crate::store_internal::schema::SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE,
    ];
    let mut by_address = BTreeMap::<(String, String), &StoreJsonPrecondition>::new();
    for precondition in preconditions {
        let address = match precondition {
            StoreJsonPrecondition::Absent { namespace, key }
            | StoreJsonPrecondition::Exact { namespace, key, .. } => {
                (namespace.clone(), key.clone())
            }
        };
        if let Some(existing) = by_address.insert(address.clone(), precondition) {
            if existing != precondition {
                return Err(Error::config(
                    "memory_write_transaction_precondition_conflict",
                    format!(
                        "conflicting JSON preconditions for namespace {}, key {}",
                        address.0, address.1
                    ),
                ));
            }
        }
    }
    for mutation in &batch.mutations {
        let (namespace, key) = match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. } => (namespace, key),
            _ => continue,
        };
        if PROTECTED_NAMESPACES.contains(&namespace.as_str())
            && !by_address.contains_key(&(namespace.clone(), key.clone()))
        {
            return Err(Error::config(
                "memory_write_transaction_precondition_missing",
                format!(
                    "protected JSON mutation requires a read-set precondition for namespace {namespace}, key {key}"
                ),
            ));
        }
    }
    validate_subject_soul_mutation_root_preconditions(batch, preconditions, operation_authorized)
}

fn validate_subject_soul_mutation_root_preconditions(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
    operation_authorized: bool,
) -> Result<()> {
    use crate::store_internal::schema::{
        is_relationship_source_protected_json_namespace, is_subject_soul_protected_json_namespace,
        RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE, RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE,
        SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE, SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
    };

    let json_mutations = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. } => {
                Some((namespace.as_str(), key.as_str()))
            }
            _ => None,
        });
    let addresses = json_mutations.collect::<Vec<_>>();
    let soul_mutated = addresses
        .iter()
        .any(|(namespace, _)| is_subject_soul_protected_json_namespace(namespace));
    if soul_mutated {
        if !operation_authorized {
            return Err(Error::config(
                "memory_write_transaction_subject_soul_authority_missing",
                "subject Soul protected namespace requires typed lifecycle closure authority",
            ));
        }
        let head_keys = addresses
            .iter()
            .filter_map(|(namespace, key)| {
                (*namespace == SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE).then_some(*key)
            })
            .collect::<BTreeSet<_>>();
        let manifest_keys = addresses
            .iter()
            .filter_map(|(namespace, key)| {
                (*namespace == SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE).then_some(*key)
            })
            .collect::<BTreeSet<_>>();
        if head_keys.len() != 1 || head_keys != manifest_keys {
            return Err(Error::config(
                "memory_write_transaction_subject_soul_closure_missing",
                "subject Soul protected namespace requires one exact lifecycle head and matching scope manifest mutation",
            ));
        }
    }

    let relationship_mutated = addresses
        .iter()
        .any(|(namespace, _)| is_relationship_source_protected_json_namespace(namespace));
    if relationship_mutated {
        if !operation_authorized {
            return Err(Error::config(
                "memory_write_transaction_relationship_source_authority_missing",
                "relationship source protected namespace requires typed operation authority",
            ));
        }
        let source_mutated = addresses
            .iter()
            .any(|(namespace, _)| *namespace == RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE);
        let manifest_mutated = addresses
            .iter()
            .any(|(namespace, _)| *namespace == RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE);
        if !source_mutated || !manifest_mutated {
            return Err(Error::config(
                "memory_write_transaction_relationship_source_closure_missing",
                "relationship source mutation requires its exact protected root and scope manifest",
            ));
        }
    }

    let root_mode = |namespace: &str, key: &str| {
        preconditions
            .iter()
            .find_map(|precondition| match precondition {
                StoreJsonPrecondition::Absent {
                    namespace: candidate_namespace,
                    key: candidate_key,
                } if candidate_namespace == namespace && candidate_key == key => Some(false),
                StoreJsonPrecondition::Exact {
                    namespace: candidate_namespace,
                    key: candidate_key,
                    ..
                } if candidate_namespace == namespace && candidate_key == key => Some(true),
                _ => None,
            })
    };
    for key in addresses.iter().filter_map(|(namespace, key)| {
        (*namespace == SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE).then_some(*key)
    }) {
        if root_mode(SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE, key)
            != root_mode(SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE, key)
        {
            return Err(Error::config(
                "memory_write_transaction_subject_soul_root_cas_mismatch",
                "lifecycle head and scope manifest require paired Absent or Exact CAS",
            ));
        }
    }

    for mutation in &batch.mutations {
        let (projection_key, projection_value) = match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } if namespace
                == crate::store_internal::schema::SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE =>
            {
                (key, value)
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace
                    == crate::store_internal::schema::SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE =>
            {
                let Some(StoreJsonPrecondition::Exact { value, .. }) = preconditions.iter().find(
                    |precondition| match precondition {
                        StoreJsonPrecondition::Exact {
                            namespace: candidate_namespace,
                            key: candidate_key,
                            ..
                        } => candidate_namespace == namespace && candidate_key == key,
                        _ => false,
                    },
                ) else {
                    return Err(Error::config(
                        "memory_write_transaction_relationship_projection_four_cas_missing",
                        "relationship projection delete requires its exact prior value",
                    ));
                };
                (key, value)
            }
            _ => continue,
        };
        let projection =
            serde_json::from_value::<SubjectSoulRelationshipProjectionV1>(projection_value.clone())
                .map_err(|error| {
                    Error::config(
                        "memory_write_transaction_relationship_projection_four_cas_invalid",
                        error.to_string(),
                    )
                })?;
        let soul_root_key = crate::store_internal::schema::subject_soul_scope_key(
            &projection.memory_space_id,
            &projection.subject_id,
            &projection.soul_id,
        )?;
        let relationship_manifest_key =
            crate::store_internal::schema::relationship_source_scope_key(
                &projection.memory_space_id,
                &projection.relationship_id,
            )?;
        let find_cas = |namespace: &str, key: &str| {
            preconditions
                .iter()
                .find(|precondition| match precondition {
                    StoreJsonPrecondition::Absent {
                        namespace: candidate_namespace,
                        key: candidate_key,
                    }
                    | StoreJsonPrecondition::Exact {
                        namespace: candidate_namespace,
                        key: candidate_key,
                        ..
                    } => candidate_namespace == namespace && candidate_key == key,
                })
        };
        let soul_is_exact = [
            SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE,
            SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
        ]
        .iter()
        .all(|namespace| {
            matches!(
                find_cas(namespace, &soul_root_key),
                Some(StoreJsonPrecondition::Exact { .. })
            )
        });
        let relationship_is_cas_bound = match find_cas(
            RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE,
            &relationship_manifest_key,
        ) {
            Some(StoreJsonPrecondition::Exact { value, .. }) => {
                let prior_manifest = serde_json::from_value::<
                    bm_core::memory::RelationshipSourceScopeManifestV1,
                >(value.clone())
                .map_err(|error| {
                    Error::config(
                        "memory_write_transaction_relationship_projection_four_cas_invalid",
                        error.to_string(),
                    )
                })?;
                let prior_source_key =
                    crate::store_internal::schema::relationship_source_revision_key(
                        &projection.memory_space_id,
                        &projection.relationship_id,
                        prior_manifest.current_revision,
                    )?;
                matches!(
                    find_cas(
                        RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE,
                        &prior_source_key
                    ),
                    Some(StoreJsonPrecondition::Exact { .. })
                )
            }
            Some(StoreJsonPrecondition::Absent { .. }) => {
                let post_source_key =
                    crate::store_internal::schema::relationship_source_revision_key(
                        &projection.memory_space_id,
                        &projection.relationship_id,
                        projection.relationship_source_revision,
                    )?;
                matches!(
                    find_cas(RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE, &post_source_key),
                    Some(StoreJsonPrecondition::Absent { .. })
                )
            }
            None => false,
        };
        if !soul_is_exact || !relationship_is_cas_bound {
            return Err(Error::config(
                "memory_write_transaction_relationship_projection_four_cas_missing",
                format!(
                    "relationship projection {projection_key} requires exact Soul head/manifest plus exact-current or pristine-absent relationship source/manifest four-CAS"
                ),
            ));
        }
    }

    let precondition_addresses = preconditions
        .iter()
        .map(|precondition| match precondition {
            StoreJsonPrecondition::Absent { namespace, key }
            | StoreJsonPrecondition::Exact { namespace, key, .. } => {
                (namespace.as_str(), key.as_str())
            }
        })
        .collect::<BTreeSet<_>>();
    for (namespace, key) in addresses {
        if (is_subject_soul_protected_json_namespace(namespace)
            || is_relationship_source_protected_json_namespace(namespace))
            && !precondition_addresses.contains(&(namespace, key))
        {
            return Err(Error::config(
                "memory_write_transaction_typed_precondition_missing",
                format!("protected owner mutation requires Absent or Exact precondition for {namespace}:{key}"),
            ));
        }
    }
    Ok(())
}

fn validate_recall_index_mutation_closure(
    batch: &StoreMutationBatch,
    read_before_json: impl Fn(&str, &str) -> Result<Option<serde_json::Value>>,
    read_before_blob: impl Fn(&str, &str) -> Result<Option<Vec<u8>>>,
) -> Result<()> {
    let index_namespaces = [
        CONVERSATION_RECALL_MANIFEST_NAMESPACE,
        CONVERSATION_TRANSCRIPT_PAGE_NAMESPACE,
        CONVERSATION_TRANSCRIPT_AUX_MANIFEST_NAMESPACE,
        ARCHIVE_RECALL_MANIFEST_NAMESPACE,
        CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE,
        ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE,
        TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
    ];
    let mut indexes = BTreeMap::<(String, String), Vec<RecallIndexAddress>>::new();
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } if index_namespaces.contains(&namespace.as_str()) => {
                if indexes.contains_key(&(namespace.clone(), key.clone())) {
                    return Err(Error::config(
                        "recall_index_mutation_closure",
                        format!("duplicate typed recall index mutation for {namespace}"),
                    ));
                }
                let entries = match namespace.as_str() {
                    CONVERSATION_RECALL_MANIFEST_NAMESPACE => {
                        let index = decode_typed_recall_index::<ConversationRecallManifest>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id
                            || index.mounted_subject_id != batch.scope.subject_id
                            || index.channel_id != batch.scope.channel
                            || batch.scope.conversation_id.as_deref()
                                != Some(index.conversation_id.as_str())
                        {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "conversation index scope differs from transaction scope",
                            ));
                        }
                        Vec::new()
                    }
                    CONVERSATION_TRANSCRIPT_PAGE_NAMESPACE => {
                        let index = decode_typed_recall_index::<ConversationTranscriptPageIndex>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id
                            || index.mounted_subject_id != batch.scope.subject_id
                            || index.channel_id != batch.scope.channel
                            || batch.scope.conversation_id.as_deref()
                                != Some(index.conversation_id.as_str())
                        {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "conversation page scope differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    CONVERSATION_TRANSCRIPT_AUX_MANIFEST_NAMESPACE => {
                        let index = decode_typed_recall_index::<ConversationTranscriptAuxManifest>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id
                            || index.mounted_subject_id != batch.scope.subject_id
                            || index.channel_id != batch.scope.channel
                            || batch.scope.conversation_id.as_deref()
                                != Some(index.conversation_id.as_str())
                        {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "conversation aux scope differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    ARCHIVE_RECALL_MANIFEST_NAMESPACE => {
                        let index =
                            decode_typed_recall_index::<ArchiveRecallManifest>(key, value.clone())?;
                        if index.memory_space_id != batch.scope.memory_space_id
                            || index.mounted_subject_id != batch.scope.subject_id
                        {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "archive index scope differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE => {
                        let index = decode_typed_recall_index::<ContinuityCapsuleScopeIndex>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "continuity index memory space differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE => {
                        let index = decode_typed_recall_index::<ActiveTaskRunByChatIndex>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "task-run index memory space differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE => {
                        let index = decode_typed_recall_index::<TaskLearningByChatIndex>(
                            key,
                            value.clone(),
                        )?;
                        if index.memory_space_id != batch.scope.memory_space_id {
                            return Err(Error::config(
                                "recall_index_mutation_closure",
                                "task-learning index memory space differs from transaction scope",
                            ));
                        }
                        index.entries
                    }
                    _ => unreachable!("guarded recall index namespace"),
                };
                indexes.insert((namespace.clone(), key.clone()), entries);
            }
            StoreMutation::DeleteJson { namespace, .. }
                if index_namespaces.contains(&namespace.as_str()) =>
            {
                return Err(Error::config(
                    "recall_index_mutation_closure",
                    "typed recall index roots must remain explicit when empty",
                ));
            }
            _ => {}
        }
    }

    for mutation in &batch.mutations {
        let (
            owner_kind,
            owner_namespace,
            owner_key,
            expected_index,
            expected_root,
            expected_digest,
            must_exist,
            previous_root,
        ) = match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } => {
                let Some(expected_index) = recall_index_namespace_for_json_owner(namespace) else {
                    continue;
                };
                let (expected_root, must_exist, previous_root) = expected_json_owner_recall_roots(
                    batch,
                    namespace,
                    key,
                    Some(value),
                    read_before_json(namespace, key)?.as_ref(),
                )?;
                let digest = RecallIndexAddress::json(namespace, key, 1, 0, value)?.content_sha256;
                (
                    RecallIndexAddressKind::Json,
                    namespace.as_str(),
                    key.as_str(),
                    expected_index,
                    expected_root,
                    Some(digest),
                    must_exist,
                    previous_root,
                )
            }
            StoreMutation::DeleteJson { namespace, key, .. } => {
                let Some(expected_index) = recall_index_namespace_for_json_owner(namespace) else {
                    continue;
                };
                let before = read_before_json(namespace, key)?;
                let (expected_root, must_exist, previous_root) =
                    expected_json_owner_recall_roots(batch, namespace, key, None, before.as_ref())?;
                (
                    RecallIndexAddressKind::Json,
                    namespace.as_str(),
                    key.as_str(),
                    expected_index,
                    expected_root,
                    None,
                    must_exist,
                    previous_root,
                )
            }
            StoreMutation::PutBlob {
                namespace,
                key,
                value,
                ..
            } => {
                let Some(expected_index) = recall_index_namespace_for_blob_owner(namespace) else {
                    continue;
                };
                let digest = RecallIndexAddress::blob(namespace, key, 1, 0, value)?.content_sha256;
                let expected_root = expected_blob_owner_recall_root(batch, namespace)?;
                (
                    RecallIndexAddressKind::Blob,
                    namespace.as_str(),
                    key.as_str(),
                    expected_index,
                    expected_root,
                    Some(digest),
                    true,
                    None,
                )
            }
            StoreMutation::DeleteBlob { namespace, key, .. } => {
                let Some(expected_index) = recall_index_namespace_for_blob_owner(namespace) else {
                    continue;
                };
                let expected_root = expected_blob_owner_recall_root(batch, namespace)?;
                let previous_root = expected_root.clone();
                (
                    RecallIndexAddressKind::Blob,
                    namespace.as_str(),
                    key.as_str(),
                    expected_index,
                    expected_root,
                    None,
                    false,
                    Some(previous_root),
                )
            }
            StoreMutation::AppendEvent { .. } => continue,
        };
        let entries = indexes
            .get(&(expected_index.to_string(), expected_root.clone()))
            .ok_or_else(|| {
            Error::config(
                "recall_index_mutation_closure",
                format!("owner mutation {owner_namespace}/{owner_key} lacks exact root {expected_index}/{expected_root}"),
            )
        })?;
        let matches = entries
            .iter()
            .filter(|entry| {
                entry.kind == owner_kind
                    && entry.namespace == owner_namespace
                    && entry.key == owner_key
            })
            .collect::<Vec<_>>();
        if must_exist {
            if matches.len() != 1
                || expected_digest
                    .as_ref()
                    .is_none_or(|digest| matches[0].content_sha256 != *digest)
            {
                return Err(Error::config(
                    "recall_index_mutation_closure",
                    format!("owner mutation {owner_namespace}/{owner_key} is not exactly bound"),
                ));
            }
        } else if !matches.is_empty() {
            return Err(Error::config(
                "recall_index_mutation_closure",
                format!("removed/inactive owner {owner_namespace}/{owner_key} remains indexed"),
            ));
        }
        if let Some(previous_root) = previous_root.filter(|root| root != &expected_root) {
            let old_entries = indexes
                .get(&(expected_index.to_string(), previous_root.clone()))
                .ok_or_else(|| {
                    Error::config(
                        "recall_index_mutation_closure",
                        format!("scope transfer for {owner_namespace}/{owner_key} omits old root {previous_root}"),
                    )
                })?;
            if old_entries.iter().any(|entry| {
                entry.kind == owner_kind
                    && entry.namespace == owner_namespace
                    && entry.key == owner_key
            }) {
                return Err(Error::config(
                    "recall_index_mutation_closure",
                    format!("scope transfer for {owner_namespace}/{owner_key} remains in old root"),
                ));
            }
        }
        let containing_roots = indexes
            .iter()
            .filter(|((namespace, _), entries)| {
                namespace == expected_index
                    && entries.iter().any(|entry| {
                        entry.kind == owner_kind
                            && entry.namespace == owner_namespace
                            && entry.key == owner_key
                    })
            })
            .map(|((_, root), _)| root)
            .collect::<Vec<_>>();
        if must_exist {
            if containing_roots != vec![&expected_root] {
                return Err(Error::config(
                    "recall_index_mutation_closure",
                    format!("owner {owner_namespace}/{owner_key} is bound to a wrong or duplicate typed root"),
                ));
            }
        } else if !containing_roots.is_empty() {
            return Err(Error::config(
                "recall_index_mutation_closure",
                format!(
                    "removed/inactive owner {owner_namespace}/{owner_key} remains in a typed root"
                ),
            ));
        }
    }
    let scope =
        StoreScopedProjectionScope::subject(&batch.scope.memory_space_id, &batch.scope.subject_id)?;
    for mutation in &batch.mutations {
        let StoreMutation::PutJson {
            namespace,
            key,
            value,
            ..
        } = mutation
        else {
            continue;
        };
        if !index_namespaces.contains(&namespace.as_str()) {
            continue;
        }
        crate::store_internal::transaction::validate_typed_recall_manifest_closure(
            namespace,
            key,
            value,
            &scope,
            |owner_namespace, owner_key| {
                transaction_post_json_value(batch, owner_namespace, owner_key, &read_before_json)
            },
            |owner_namespace, owner_key| {
                transaction_post_blob_value(batch, owner_namespace, owner_key, &read_before_blob)
            },
        )?;
    }
    Ok(())
}

fn transaction_post_json_value(
    batch: &StoreMutationBatch,
    namespace: &str,
    key: &str,
    read_before: &impl Fn(&str, &str) -> Result<Option<serde_json::Value>>,
) -> Result<Option<serde_json::Value>> {
    for mutation in batch.mutations.iter().rev() {
        match mutation {
            StoreMutation::PutJson {
                namespace: candidate_namespace,
                key: candidate_key,
                value,
                ..
            } if candidate_namespace == namespace && candidate_key == key => {
                return Ok(Some(value.clone()));
            }
            StoreMutation::DeleteJson {
                namespace: candidate_namespace,
                key: candidate_key,
                ..
            } if candidate_namespace == namespace && candidate_key == key => return Ok(None),
            _ => {}
        }
    }
    read_before(namespace, key)
}

fn transaction_post_blob_value(
    batch: &StoreMutationBatch,
    namespace: &str,
    key: &str,
    read_before: &impl Fn(&str, &str) -> Result<Option<Vec<u8>>>,
) -> Result<Option<Vec<u8>>> {
    for mutation in batch.mutations.iter().rev() {
        match mutation {
            StoreMutation::PutBlob {
                namespace: candidate_namespace,
                key: candidate_key,
                value,
                ..
            } if candidate_namespace == namespace && candidate_key == key => {
                return Ok(Some(value.clone()));
            }
            StoreMutation::DeleteBlob {
                namespace: candidate_namespace,
                key: candidate_key,
                ..
            } if candidate_namespace == namespace && candidate_key == key => return Ok(None),
            _ => {}
        }
    }
    read_before(namespace, key)
}

fn expected_json_owner_recall_roots(
    batch: &StoreMutationBatch,
    namespace: &str,
    key: &str,
    next: Option<&serde_json::Value>,
    before: Option<&serde_json::Value>,
) -> Result<(String, bool, Option<String>)> {
    let root_for = |value: &serde_json::Value| -> Result<(String, bool)> {
        match namespace {
            "conversation_transcript" => {
                let record = serde_json::from_value::<TranscriptTurnRecord>(value.clone())
                    .map_err(|error| {
                        Error::config("recall_index_mutation_closure", error.to_string())
                    })?;
                Ok((
                    ConversationTranscriptPageIndex::build(
                        1,
                        &record.key.memory_space_id,
                        &record.subject,
                        &record.key.channel_id,
                        &record.key.conversation_id,
                        &StorePlatform::conversation_page_id(
                            StorePlatform::conversation_page_for_sequence(record.sequence)?,
                        ),
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            "conversation_transcript_attr" => {
                let attr = serde_json::from_value::<TranscriptAttrEnvelope>(value.clone())
                    .map_err(|error| {
                        Error::config("recall_index_mutation_closure", error.to_string())
                    })?;
                Ok((
                    ConversationTranscriptAuxManifest::build(
                        1,
                        &attr.target.key.memory_space_id,
                        &batch.scope.subject_id,
                        &attr.target.key.channel_id,
                        &attr.target.key.conversation_id,
                        &attr.target.turn_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            "conversation_transcript_derived_ref" => {
                let derived =
                    serde_json::from_value::<DerivedMemoryRef>(value.clone()).map_err(|error| {
                        Error::config("recall_index_mutation_closure", error.to_string())
                    })?;
                let subject_id = exact_derived_owner_subject(&derived)?;
                Ok((
                    ConversationTranscriptAuxManifest::build(
                        1,
                        &derived.source.memory_space_id,
                        subject_id,
                        &derived.source.channel_id,
                        &derived.source.conversation_id,
                        &derived.source.turn_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            "conversation_transcript_alias" => {
                let alias = serde_json::from_value::<TranscriptConversationAlias>(value.clone())
                    .map_err(|error| {
                        Error::config("recall_index_mutation_closure", error.to_string())
                    })?;
                Ok((
                    ArchiveRecallManifest::build(
                        1,
                        &alias.memory_space_id,
                        &alias.mounted_subject_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            "session" | "session_summary" | "active_work" | "turn_ledger" => Ok((
                ArchiveRecallManifest::build(
                    1,
                    &batch.scope.memory_space_id,
                    &batch.scope.subject_id,
                    std::iter::empty(),
                )?
                .physical_key,
                true,
            )),
            "continuity_capsule" => {
                let capsule = serde_json::from_value::<ContinuityCapsule>(value.clone()).map_err(
                    |error| Error::config("recall_index_mutation_closure", error.to_string()),
                )?;
                Ok((
                    ContinuityCapsuleScopeIndex::build(
                        1,
                        &batch.scope.memory_space_id,
                        capsule.scope_kind.label(),
                        &capsule.scope_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            "task_run" => {
                let record =
                    serde_json::from_value::<TaskRunRecord>(value.clone()).map_err(|error| {
                        Error::config("recall_index_mutation_closure", error.to_string())
                    })?;
                Ok((
                    ActiveTaskRunByChatIndex::build(
                        1,
                        &batch.scope.memory_space_id,
                        &record.run.source_channel,
                        &record.run.source_chat_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    record.run.status.is_active(),
                ))
            }
            "task_learning" => {
                let record = serde_json::from_value::<TaskLearningRecord>(value.clone()).map_err(
                    |error| Error::config("recall_index_mutation_closure", error.to_string()),
                )?;
                Ok((
                    TaskLearningByChatIndex::build(
                        1,
                        &batch.scope.memory_space_id,
                        &record.source_channel,
                        &record.source_chat_id,
                        std::iter::empty(),
                    )?
                    .physical_key,
                    true,
                ))
            }
            _ => Err(Error::config(
                "recall_index_mutation_closure",
                format!("unsupported indexed JSON owner {namespace}/{key}"),
            )),
        }
    };
    let next_root = next.map(root_for).transpose()?;
    let before_root = before.map(root_for).transpose()?.map(|(root, _)| root);
    match next_root {
        Some((root, must_exist)) => Ok((root, must_exist, before_root)),
        None => before_root
            .clone()
            .map(|root| (root, false, before_root))
            .ok_or_else(|| {
                Error::config(
                    "recall_index_mutation_closure",
                    format!("delete owner {namespace}/{key} has no typed before-image"),
                )
            }),
    }
}

fn expected_blob_owner_recall_root(batch: &StoreMutationBatch, namespace: &str) -> Result<String> {
    match namespace {
        "daily" | "memory" => Ok(ArchiveRecallManifest::build(
            1,
            &batch.scope.memory_space_id,
            &batch.scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key),
        _ => Err(Error::config(
            "recall_index_mutation_closure",
            format!("unsupported indexed blob owner {namespace}"),
        )),
    }
}

fn recall_index_namespace_for_json_owner(namespace: &str) -> Option<&'static str> {
    match namespace {
        "conversation_transcript" => Some(CONVERSATION_TRANSCRIPT_PAGE_NAMESPACE),
        "conversation_transcript_attr" | "conversation_transcript_derived_ref" => {
            Some(CONVERSATION_TRANSCRIPT_AUX_MANIFEST_NAMESPACE)
        }
        "session"
        | "session_summary"
        | "active_work"
        | "turn_ledger"
        | "conversation_transcript_alias" => Some(ARCHIVE_RECALL_MANIFEST_NAMESPACE),
        "continuity_capsule" => Some(CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE),
        "task_run" => Some(ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE),
        "task_learning" => Some(TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE),
        _ => None,
    }
}

fn recall_index_namespace_for_blob_owner(namespace: &str) -> Option<&'static str> {
    match namespace {
        "daily" | "memory" => Some(ARCHIVE_RECALL_MANIFEST_NAMESPACE),
        _ => None,
    }
}

fn validate_evidence_effect_address_closure(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    let mut actual_addresses = BTreeSet::new();
    let mut expected_addresses = BTreeSet::new();
    let mut owner_ids = BTreeSet::new();

    for mutation in &batch.mutations {
        let (namespace, key) = match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. }
                if matches!(
                    namespace.as_str(),
                    GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
                        | GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE
                        | GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE
                ) =>
            {
                (namespace, key)
            }
            _ => continue,
        };
        if !actual_addresses.insert((namespace.clone(), key.clone())) {
            return Err(Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                format!("duplicate evidence mutation effect address {namespace}/{key}"),
            ));
        }

        if namespace != GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE {
            continue;
        }
        let document = evidence_owner_document_for_mutation(mutation, preconditions)?;
        validate_governed_evidence_document(&document).map_err(|error| {
            Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                format!("invalid evidence owner mutation: {error:?}"),
            )
        })?;
        if document.memory_space_id != batch.scope.memory_space_id
            || document.mounted_subject_id != batch.scope.subject_id
        {
            return Err(Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                "evidence owner mutation scope does not match transaction scope",
            ));
        }
        let (mutation_key, plane, record_key) = match mutation {
            StoreMutation::PutJson {
                key,
                plane,
                record_key,
                ..
            }
            | StoreMutation::DeleteJson {
                key,
                plane,
                record_key,
                ..
            } => (key, plane, record_key),
            _ => unreachable!("evidence owner closure only accepts JSON mutations"),
        };
        let owner_ref = GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::EvidenceDocument,
            document.document_id.clone(),
        );
        if plane != GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
            || record_key != &document.document_id
            || mutation_key != &document.physical_key
            || !owner_ids.insert(document.document_id.clone())
        {
            return Err(Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                "evidence owner mutation metadata or logical identity is not exact",
            ));
        }

        let owner_key = scoped_governed_evidence_document_key(
            &batch.scope.memory_space_id,
            &document.document_id,
        )?;
        expected_addresses.insert((GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(), owner_key));
        let source_ref_key = governed_evidence_source_ref_from_document(&document)?.physical_key;
        expected_addresses.insert((
            GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
            source_ref_key,
        ));
        if matches!(mutation, StoreMutation::PutJson { .. }) {
            if let Some(before_document) = evidence_owner_before_document_for_scope(
                &document.physical_key,
                &batch.scope.memory_space_id,
                &batch.scope.subject_id,
                &owner_ref,
                preconditions,
            )? {
                let before_source_ref_key =
                    governed_evidence_source_ref_from_document(&before_document)?.physical_key;
                expected_addresses.insert((
                    GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                    before_source_ref_key,
                ));
            }
        }
    }

    if !owner_ids.is_empty() {
        expected_addresses.insert((
            GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
            governed_evidence_source_claim_manifest_key(
                &batch.scope.memory_space_id,
                &batch.scope.subject_id,
            )?,
        ));
    }

    if actual_addresses == expected_addresses {
        return Ok(());
    }
    let missing = expected_addresses
        .difference(&actual_addresses)
        .map(|(namespace, key)| format!("{namespace}/{key}"))
        .collect::<Vec<_>>();
    let extra = actual_addresses
        .difference(&expected_addresses)
        .map(|(namespace, key)| format!("{namespace}/{key}"))
        .collect::<Vec<_>>();
    Err(Error::config(
        "memory_write_transaction_evidence_source_ref_closure_invalid",
        format!(
            "evidence mutation effect address closure drift: missing [{}], extra [{}]",
            missing.join(","),
            extra.join(",")
        ),
    ))
}

fn evidence_owner_before_document_for_scope(
    physical_key: &str,
    memory_space_id: &str,
    mounted_subject_id: &str,
    owner_ref: &GovernedMemoryOwnerRef,
    preconditions: &[StoreJsonPrecondition],
) -> Result<Option<GovernedEvidenceDocument>> {
    for precondition in preconditions {
        let StoreJsonPrecondition::Exact {
            namespace,
            key,
            value,
        } = precondition
        else {
            continue;
        };
        if namespace != GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE || key != physical_key {
            continue;
        }
        let document =
            serde_json::from_value::<GovernedEvidenceDocument>(value.clone()).map_err(|error| {
                Error::config(
                    "memory_write_transaction_evidence_source_ref_closure_invalid",
                    format!("invalid evidence owner precondition: {error}"),
                )
            })?;
        validate_governed_evidence_document(&document).map_err(|error| {
            Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                format!("invalid evidence owner precondition: {error:?}"),
            )
        })?;
        let precondition_owner_ref = GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::EvidenceDocument,
            document.document_id.clone(),
        );
        if document.physical_key != *physical_key
            || document.memory_space_id != memory_space_id
            || document.mounted_subject_id != mounted_subject_id
            || precondition_owner_ref != *owner_ref
        {
            return Err(Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                "evidence owner precondition does not match canonical typed owner scope",
            ));
        }
        return Ok(Some(document));
    }
    Ok(None)
}

fn evidence_owner_document_for_mutation(
    mutation: &StoreMutation,
    preconditions: &[StoreJsonPrecondition],
) -> Result<GovernedEvidenceDocument> {
    let value = match mutation {
        StoreMutation::PutJson { value, .. } => value,
        StoreMutation::DeleteJson { namespace, key, .. } => preconditions
            .iter()
            .find_map(|precondition| match precondition {
                StoreJsonPrecondition::Exact {
                    namespace: expected_namespace,
                    key: expected_key,
                    value,
                } if expected_namespace == namespace && expected_key == key => Some(value),
                _ => None,
            })
            .ok_or_else(|| {
                Error::config(
                    "memory_write_transaction_evidence_source_ref_closure_invalid",
                    "evidence owner delete requires an exact typed precondition",
                )
            })?,
        _ => {
            return Err(Error::config(
                "memory_write_transaction_evidence_source_ref_closure_invalid",
                "evidence owner closure requires a JSON mutation",
            ));
        }
    };
    serde_json::from_value(value.clone()).map_err(|error| {
        Error::config(
            "memory_write_transaction_evidence_source_ref_closure_invalid",
            format!("invalid evidence owner document: {error}"),
        )
    })
}

fn validate_evidence_lifecycle_closure(batch: &StoreMutationBatch) -> Result<()> {
    let owner_mutated = batch.mutations.iter().any(|mutation| {
        matches!(
            mutation,
            StoreMutation::PutJson { namespace, .. }
                | StoreMutation::DeleteJson { namespace, .. }
                if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
        )
    });
    if !owner_mutated {
        return Ok(());
    }

    let lifecycle_events = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::AppendEvent { event } => Some(event),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [event] = lifecycle_events.as_slice() else {
        return Err(Error::config(
            "memory_write_transaction_evidence_lifecycle_closure_invalid",
            "evidence owner mutation requires exactly one lifecycle AppendEvent",
        ));
    };

    let expected_operation = RuntimeLifecycleOperation::Maintain.as_str();
    let expected_effect = RuntimeLifecycleEffect::RunMaintenance.as_str();
    let expected_trigger = RuntimeLifecycleTrigger::SdkCall.as_str();
    let exact_binding = event.kind == MemoryStoreEventKind::RuntimeLifecycle
        && event.kind_name == MemoryStoreEventKind::RuntimeLifecycle.as_str()
        && event.plane == "runtime_lifecycle"
        && event.record_key == expected_operation
        && event.scope == StoreEventScope::system(expected_operation)
        && event.payload.get("runtime_operation").map(String::as_str) == Some(expected_operation)
        && event.payload.get("operation").map(String::as_str) == Some(batch.operation.as_str())
        && event.payload.get("transaction_id").map(String::as_str)
            == Some(batch.transaction_id.as_str())
        && event.payload.get("effect").map(String::as_str) == Some(expected_effect)
        && event.payload.get("trigger").map(String::as_str) == Some(expected_trigger)
        && !event.event_id.trim().is_empty()
        && !event.content_hash.trim().is_empty();
    if exact_binding {
        Ok(())
    } else {
        Err(Error::config(
            "memory_write_transaction_evidence_lifecycle_closure_invalid",
            "evidence owner lifecycle AppendEvent is not exactly bound to the transaction",
        ))
    }
}

pub(crate) fn validate_governed_transaction_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    graph_repair_authorized: bool,
    governed_long_term_retention_limit: Option<usize>,
    governed_runtime_skill_owner_limit: Option<usize>,
    operation_capacity: StoreCapacityBudget,
) -> Result<()> {
    validate_confirmation_evidence_post_image(batch, before, after)?;
    validate_evidence_source_ref_post_image(
        batch,
        before,
        after,
        operation_capacity.kv_max_entries,
    )?;
    validate_long_term_version_root_post_image(batch, after, governed_long_term_retention_limit)?;
    validate_runtime_skill_owner_post_image(
        batch,
        before,
        after,
        governed_runtime_skill_owner_limit,
    )?;
    crate::store_internal::subject_soul::validate_subject_soul_transaction_post_image(
        batch,
        before,
        after,
        operation_capacity,
    )?;
    validate_facet_post_image(batch, before, after)?;
    validate_graph_post_image(batch, before, after, graph_repair_authorized)?;
    validate_control_post_image(batch, before, after)
}

fn validate_confirmation_evidence_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<()> {
    let material_namespace = crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE;
    if !batch_mutates_namespace(batch, material_namespace) {
        return Ok(());
    }
    let materials = batch_json_keys(batch, material_namespace)
        .into_iter()
        .map(|key| {
            governed_image::<LongTermMemoryVersionMaterial>(material_namespace, &key, before, after)
        })
        .collect::<Result<Vec<_>>>()?;
    let revisions = batch_json_keys(batch, LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .into_iter()
        .map(|key| {
            governed_image::<LongTermMemoryControlRevision>(
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    for material in materials.iter().filter_map(|image| image.after.as_ref()) {
        let Some(evidence) = material.governed_content.confirmation_evidence.as_ref() else {
            continue;
        };
        let exact_correction = revisions
            .iter()
            .filter_map(|image| image.after.as_ref())
            .filter(|revision| {
                let correction = &evidence.correction;
                revision.operation == bm_core::memory::LongTermControlOperation::Correct
                    && revision.memory_space_id == correction.memory_space_id
                    && revision.actor_subject_id.as_deref()
                        == Some(correction.actor_subject_id.as_str())
                    && revision.transition.predecessor == correction.predecessor
                    && revision.transition.successor.as_ref() == Some(&correction.successor)
                    && revision.created_at == evidence.confirmed_at
                    && revision.revision_id == correction.control_revision_id
                    && revision.successor_material_digest.as_deref()
                        == Some(material.content_digest.as_str())
            })
            .count()
            == 1;
        let carried_by_non_correction_control = revisions
            .iter()
            .filter_map(|image| image.after.as_ref())
            .any(|revision| {
                let correction = &evidence.correction;
                revision.operation != bm_core::memory::LongTermControlOperation::Correct
                    && revision.transition.successor.as_ref()
                        == Some(&material.owner_revision_ref())
                    && revision.successor_material_digest.as_deref()
                        == Some(material.content_digest.as_str())
                    && correction.successor.owner_ref == revision.transition.predecessor.owner_ref
                    && correction.successor.owner_revision
                        <= revision.transition.predecessor.owner_revision
            });
        if !exact_correction && !carried_by_non_correction_control {
            return Err(Error::config(
                "memory_write_transaction_confirmation_evidence_invalid",
                "confirmation evidence must be carried unchanged or bound to one same-batch Correct revision",
            ));
        }
    }
    Ok(())
}

fn validate_long_term_version_root_post_image(
    batch: &StoreMutationBatch,
    after: &BackendTransactionState,
    governed_long_term_retention_limit: Option<usize>,
) -> Result<()> {
    let material_namespace = crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE;
    let head_namespace = crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE;
    let root_namespace = crate::store_internal::schema::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE;
    if !batch_mutates_namespace(batch, material_namespace)
        && !batch_mutates_namespace(batch, head_namespace)
        && !batch_mutates_namespace(batch, root_namespace)
        && !batch_mutates_namespace(batch, LONG_TERM_CONTROL_REVISION_NAMESPACE)
    {
        return Ok(());
    }
    let max_retained = governed_long_term_retention_limit
        .filter(|limit| *limit > 0)
        .ok_or_else(|| {
            Error::config(
                "memory_write_transaction_long_term_root_post_image_invalid",
                "typed long-term mutations require a positive request-pinned retention limit",
            )
        })?;
    let memory_space_id = batch.scope.memory_space_id.as_str();
    let factual_owner_id = memory_space_id;
    validate_long_term_version_scope_image(
        after,
        memory_space_id,
        factual_owner_id,
        max_retained,
        "memory_write_transaction_long_term_root_post_image_invalid",
    )
}

fn validate_long_term_version_scope_image(
    state: &BackendTransactionState,
    memory_space_id: &str,
    factual_owner_id: &str,
    max_retained: usize,
    stage: &'static str,
) -> Result<()> {
    let material_namespace = crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE;
    let head_namespace = crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE;
    let root_namespace = crate::store_internal::schema::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE;
    let root_key = long_term_version_scope_manifest_key(memory_space_id, factual_owner_id)?;
    let root_value = state
        .json
        .get(&(root_namespace.to_string(), root_key.clone()))
        .ok_or_else(|| {
            Error::config(
                stage,
                "typed long-term owners require their exact scope root",
            )
        })?;
    let root = serde_json::from_value::<LongTermMemoryVersionScopeManifest>(root_value.clone())
        .map_err(|error| {
            Error::config(
                stage,
                format!("long-term scope root decode failed: {error}"),
            )
        })?;
    if root.physical_key != root_key
        || root.memory_space_id != memory_space_id
        || root.factual_owner_id != factual_owner_id
    {
        return Err(Error::config(
            stage,
            "long-term root differs from the requested physical scope",
        ));
    }
    let heads = state
        .json
        .iter()
        .filter(|((namespace, _), _)| namespace == head_namespace)
        .map(|(_, value)| {
            serde_json::from_value::<LongTermMemoryHeadManifest>(value.clone()).map_err(|error| {
                Error::config(stage, format!("long-term head decode failed: {error}"))
            })
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|head| {
            head.memory_space_id == memory_space_id && head.factual_owner_id == factual_owner_id
        })
        .collect::<Vec<_>>();
    let materials = state
        .json
        .iter()
        .filter(|((namespace, _), _)| namespace == material_namespace)
        .map(|(_, value)| {
            serde_json::from_value::<LongTermMemoryVersionMaterial>(value.clone()).map_err(
                |error| Error::config(stage, format!("long-term material decode failed: {error}")),
            )
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|material| {
            material.memory_space_id == memory_space_id
                && material.factual_owner_id == factual_owner_id
        })
        .collect::<Vec<_>>();
    let scoped_control_revisions = state
        .json
        .iter()
        .filter(|((namespace, _), _)| namespace == LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .map(|((_, physical_key), value)| {
            let revision = serde_json::from_value::<LongTermMemoryControlRevision>(value.clone())
                .map_err(|error| {
                Error::config(
                    stage,
                    format!("long-term control revision decode failed: {error}"),
                )
            })?;
            Ok((physical_key.clone(), revision))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|(_, revision)| {
            revision.memory_space_id == memory_space_id
                && revision.factual_owner_id == factual_owner_id
        })
        .collect::<Vec<_>>();
    let purged_owner_refs = scoped_control_revisions
        .iter()
        .filter(|(_, revision)| {
            matches!(
                revision.transition.termination,
                GovernedOwnerTermination::Deleted | GovernedOwnerTermination::Forgotten
            )
        })
        .map(|(_, revision)| revision.transition.predecessor.owner_ref.clone())
        .collect::<BTreeSet<_>>();
    let required_control_keys = scoped_control_revisions
        .iter()
        .filter(|(_, revision)| {
            !purged_owner_refs.contains(&revision.transition.predecessor.owner_ref)
        })
        .map(|(physical_key, _)| physical_key.clone())
        .collect::<BTreeSet<_>>();
    let root_control_keys = root
        .transition_bindings
        .iter()
        .map(|binding| binding.control_revision_physical_key.clone())
        .collect::<BTreeSet<_>>();
    if required_control_keys != root_control_keys {
        return Err(Error::config(
            stage,
            "long-term version root and bind-required control revisions differ",
        ));
    }
    let mut transitions = Vec::new();
    for binding in &root.transition_bindings {
        let value = state
            .json
            .get(&(
                LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
                binding.control_revision_physical_key.clone(),
            ))
            .ok_or_else(|| {
                Error::config(
                    stage,
                    "long-term transition binding is missing its control revision",
                )
            })?;
        let revision = serde_json::from_value::<LongTermMemoryControlRevision>(value.clone())
            .map_err(|error| {
                Error::config(
                    stage,
                    format!("long-term control revision decode failed: {error}"),
                )
            })?;
        if revision.validate_contract().is_err()
            || revision.transition.predecessor != binding.predecessor
            || revision.content_digest != binding.control_revision_content_digest
        {
            return Err(Error::config(
                stage,
                "long-term transition control binding digest or predecessor drift",
            ));
        }
        transitions.push(revision.transition);
    }
    let validation = root.validate_exact(
        &heads,
        &materials,
        &transitions,
        &root.transition_bindings,
        max_retained,
    );
    if validation.accepted {
        Ok(())
    } else {
        let rebuilt = LongTermMemoryVersionScopeManifest::build(
            memory_space_id,
            factual_owner_id,
            root.manifest_revision,
            &heads,
            &materials,
            &transitions,
            &root.transition_bindings,
            max_retained,
        );
        Err(Error::config(
            stage,
            format!(
                "long-term root exact closure rejected: {:?}; observed=({},{},{},{}); rebuilt={:?}",
                validation.failures,
                root.head_count,
                root.material_count,
                root.transition_count,
                root.closure_digest,
                rebuilt.as_ref().map(|manifest| (
                    manifest.head_count,
                    manifest.material_count,
                    manifest.transition_count,
                    manifest.closure_digest.as_str(),
                ))
            ),
        ))
    }
}

fn validate_long_term_version_store_image(
    state: &BackendTransactionState,
    max_retained: usize,
    stage: &'static str,
) -> Result<()> {
    if max_retained == 0 {
        return Err(Error::config(
            stage,
            "long-term footprint validation requires a positive pinned retention limit",
        ));
    }
    let material_namespace = crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE;
    let head_namespace = crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE;
    let root_namespace = crate::store_internal::schema::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE;
    let mut scopes = BTreeSet::new();
    for ((namespace, _), value) in &state.json {
        match namespace.as_str() {
            namespace if namespace == material_namespace => {
                let material =
                    serde_json::from_value::<LongTermMemoryVersionMaterial>(value.clone())
                        .map_err(|error| Error::config(stage, error.to_string()))?;
                scopes.insert((material.memory_space_id, material.factual_owner_id));
            }
            namespace if namespace == head_namespace => {
                let head = serde_json::from_value::<LongTermMemoryHeadManifest>(value.clone())
                    .map_err(|error| Error::config(stage, error.to_string()))?;
                scopes.insert((head.memory_space_id, head.factual_owner_id));
            }
            namespace if namespace == root_namespace => {
                let root =
                    serde_json::from_value::<LongTermMemoryVersionScopeManifest>(value.clone())
                        .map_err(|error| Error::config(stage, error.to_string()))?;
                scopes.insert((root.memory_space_id, root.factual_owner_id));
            }
            namespace if namespace == LONG_TERM_CONTROL_REVISION_NAMESPACE => {
                let revision =
                    serde_json::from_value::<LongTermMemoryControlRevision>(value.clone())
                        .map_err(|error| Error::config(stage, error.to_string()))?;
                if !matches!(
                    revision.transition.termination,
                    GovernedOwnerTermination::Deleted | GovernedOwnerTermination::Forgotten
                ) {
                    scopes.insert((revision.memory_space_id, revision.factual_owner_id));
                }
            }
            _ => {}
        }
    }
    for (memory_space_id, factual_owner_id) in scopes {
        validate_long_term_version_scope_image(
            state,
            &memory_space_id,
            &factual_owner_id,
            max_retained,
            stage,
        )?;
    }
    Ok(())
}

fn validate_runtime_skill_owner_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    governed_runtime_skill_owner_limit: Option<usize>,
) -> Result<()> {
    let owner_namespace = crate::store_internal::schema::RUNTIME_SKILL_RECORD_NAMESPACE;
    let manifest_namespace = crate::store_internal::schema::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE;
    if !batch_mutates_namespace(batch, owner_namespace)
        && !batch_mutates_namespace(batch, manifest_namespace)
    {
        return Ok(());
    }
    let max_owners = governed_runtime_skill_owner_limit
        .filter(|limit| *limit > 0)
        .ok_or_else(|| {
            Error::config(
                "memory_write_transaction_runtime_skill_post_image_invalid",
                "typed runtime skill mutations require a positive request-pinned owner limit",
            )
        })?;

    let mut scopes = BTreeSet::<(String, RuntimeSkillOwningScope)>::new();
    for state in [before, after] {
        for ((namespace, _), value) in &state.json {
            if namespace == owner_namespace {
                if let Ok(record) = serde_json::from_value::<RuntimeSkillOwnerRecord>(value.clone())
                {
                    scopes.insert((record.memory_space_id, record.owning_scope));
                }
            } else if namespace == manifest_namespace {
                if let Ok(manifest) =
                    serde_json::from_value::<RuntimeSkillScopeManifest>(value.clone())
                {
                    scopes.insert((manifest.memory_space_id, manifest.owning_scope));
                }
            }
        }
    }
    if scopes.is_empty() {
        return Err(Error::config(
            "memory_write_transaction_runtime_skill_post_image_invalid",
            "runtime skill mutation has no decodable physical owning scope",
        ));
    }

    for (memory_space_id, owning_scope) in scopes {
        let batch_owning_scope = match &batch.scope.physical_owning_scope {
            crate::store_internal::StorePhysicalOwningScope::Subject { mounted_subject_id } => {
                RuntimeSkillOwningScope::Subject {
                    mounted_subject_id: mounted_subject_id.clone(),
                }
            }
            crate::store_internal::StorePhysicalOwningScope::SharedProgram => {
                RuntimeSkillOwningScope::SharedProgram
            }
        };
        if memory_space_id != batch.scope.memory_space_id || owning_scope != batch_owning_scope {
            return Err(Error::config(
                "memory_write_transaction_runtime_skill_post_image_invalid",
                "runtime skill owner scope differs from the transaction physical authority",
            ));
        }
        validate_runtime_skill_scope_image(
            after,
            &memory_space_id,
            &owning_scope,
            max_owners,
            "memory_write_transaction_runtime_skill_post_image_invalid",
        )?;
    }
    Ok(())
}

fn validate_runtime_skill_scope_image(
    state: &BackendTransactionState,
    memory_space_id: &str,
    owning_scope: &RuntimeSkillOwningScope,
    max_owners: usize,
    stage: &'static str,
) -> Result<()> {
    let owner_namespace = crate::store_internal::schema::RUNTIME_SKILL_RECORD_NAMESPACE;
    let manifest_namespace = crate::store_internal::schema::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE;
    let records = state
        .json
        .iter()
        .filter(|((namespace, _), _)| namespace == owner_namespace)
        .map(|(_, value)| {
            serde_json::from_value::<RuntimeSkillOwnerRecord>(value.clone()).map_err(|error| {
                Error::config(stage, format!("runtime skill owner decode failed: {error}"))
            })
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|record| {
            record.memory_space_id == memory_space_id && record.owning_scope == *owning_scope
        })
        .collect::<Vec<_>>();
    let bindings = records
        .iter()
        .map(RuntimeSkillOwnerBinding::from_record)
        .collect::<Result<Vec<_>>>()?;
    let manifest_key = runtime_skill_scope_manifest_key(memory_space_id, owning_scope)?;
    let manifest_value = state
        .json
        .get(&(manifest_namespace.to_string(), manifest_key));
    if bindings.is_empty() {
        if manifest_value.is_some() {
            return Err(Error::config(
                stage,
                "empty runtime skill scope must not retain a manifest",
            ));
        }
        return Ok(());
    }
    let manifest_value = manifest_value.ok_or_else(|| {
        Error::config(
            stage,
            "runtime skill owners require their exact scope manifest",
        )
    })?;
    let manifest = serde_json::from_value::<RuntimeSkillScopeManifest>(manifest_value.clone())
        .map_err(|error| {
            Error::config(
                stage,
                format!("runtime skill scope manifest decode failed: {error}"),
            )
        })?;
    manifest
        .validate_exact(memory_space_id, owning_scope, bindings, max_owners)
        .map_err(|error| Error::config(stage, error.to_string()))
}

fn validate_runtime_skill_store_image(
    state: &BackendTransactionState,
    max_owners: usize,
    stage: &'static str,
) -> Result<()> {
    if max_owners == 0 {
        return Err(Error::config(
            stage,
            "runtime skill footprint validation requires a positive pinned owner limit",
        ));
    }
    let owner_namespace = crate::store_internal::schema::RUNTIME_SKILL_RECORD_NAMESPACE;
    let manifest_namespace = crate::store_internal::schema::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE;
    let mut scopes = BTreeSet::new();
    for ((namespace, _), value) in &state.json {
        if namespace == owner_namespace {
            let record = serde_json::from_value::<RuntimeSkillOwnerRecord>(value.clone())
                .map_err(|error| Error::config(stage, error.to_string()))?;
            scopes.insert((record.memory_space_id, record.owning_scope));
        } else if namespace == manifest_namespace {
            let manifest = serde_json::from_value::<RuntimeSkillScopeManifest>(value.clone())
                .map_err(|error| Error::config(stage, error.to_string()))?;
            scopes.insert((manifest.memory_space_id, manifest.owning_scope));
        }
    }
    for (memory_space_id, owning_scope) in scopes {
        validate_runtime_skill_scope_image(
            state,
            &memory_space_id,
            &owning_scope,
            max_owners,
            stage,
        )?;
    }
    Ok(())
}

fn governed_transaction_dependency_json_reads(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<BTreeSet<(String, String)>> {
    let mut documents = Vec::new();
    for mutation in &batch.mutations {
        if let StoreMutation::PutJson {
            namespace,
            key,
            value,
            ..
        } = mutation
        {
            documents.push((namespace.as_str(), key.as_str(), value));
        }
    }
    for precondition in preconditions {
        if let StoreJsonPrecondition::Exact {
            namespace,
            key,
            value,
        } = precondition
        {
            documents.push((namespace.as_str(), key.as_str(), value));
        }
    }

    let mut reads = BTreeSet::new();
    for (namespace, _key, value) in documents {
        match namespace {
            GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE => {
                let manifest = decode_transaction_dependency::<GovernedEvidenceSourceClaimManifest>(
                    value,
                    "evidence source claim manifest",
                )?;
                reads.extend(
                    manifest
                        .owner_keys
                        .into_iter()
                        .map(|key| (GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(), key)),
                );
                reads.extend(
                    manifest
                        .claim_keys
                        .into_iter()
                        .map(|key| (GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(), key)),
                );
            }
            GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE => {
                let owner = decode_transaction_dependency::<GovernedEvidenceDocument>(
                    value,
                    "evidence owner",
                )?;
                let claim = governed_evidence_source_ref_from_document(&owner)?;
                reads.insert((
                    GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                    claim.physical_key,
                ));
            }
            GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE => {
                let claim = decode_transaction_dependency::<GovernedEvidenceSourceRef>(
                    value,
                    "evidence source claim",
                )?;
                reads.extend(governed_owner_storage_addresses(
                    &batch.scope.memory_space_id,
                    &batch.scope.subject_id,
                    &claim.owner_ref,
                    claim.owner_revision,
                )?);
            }
            MEMORY_FACET_POSTING_NAMESPACE => {
                if let Ok(manifest) =
                    serde_json::from_value::<MemoryFacetIndexManifest>(value.clone())
                {
                    for owner in manifest.owner_versions {
                        reads.insert((
                            MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                            scoped_memory_facet_owner_storage_key(
                                &batch.scope.memory_space_id,
                                &batch.scope.subject_id,
                                &owner.owner_ref,
                            )
                            .map_err(|error| {
                                Error::config(
                                    "memory_write_transaction_dependency_read_set",
                                    format!("facet owner key: {error:?}"),
                                )
                            })?,
                        ));
                        reads.extend(governed_owner_storage_addresses(
                            &batch.scope.memory_space_id,
                            &batch.scope.subject_id,
                            &owner.owner_ref,
                            owner.owner_revision,
                        )?);
                    }
                    reads.extend(manifest.posting_revisions.into_iter().map(|posting| {
                        (
                            MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                            posting.posting_key,
                        )
                    }));
                }
            }
            MEMORY_FACET_INDEX_NAMESPACE => {
                let facet = decode_transaction_dependency::<MemoryFacetIndexDoc>(
                    value,
                    "memory facet owner",
                )?;
                reads.extend(governed_owner_storage_addresses(
                    &batch.scope.memory_space_id,
                    &batch.scope.subject_id,
                    &facet.owner_ref,
                    facet.owner_revision,
                )?);
            }
            MEMORY_GRAPH_MANIFEST_NAMESPACE => {
                let manifest = decode_transaction_dependency::<MemoryGraphScopeManifest>(
                    value,
                    "memory graph manifest",
                )?;
                reads.insert((
                    MEMORY_GRAPH_REVISION_NAMESPACE.to_string(),
                    manifest.revision.storage_key,
                ));
                reads.extend(manifest.node_memberships.into_iter().map(|dependency| {
                    (
                        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE.to_string(),
                        dependency.storage_key,
                    )
                }));
                reads.extend(manifest.edge_memberships.into_iter().map(|dependency| {
                    (
                        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE.to_string(),
                        dependency.storage_key,
                    )
                }));
                reads.extend(manifest.backlink_memberships.into_iter().map(|dependency| {
                    (
                        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE.to_string(),
                        dependency.storage_key,
                    )
                }));
                reads.extend(manifest.recall_indexes.into_iter().map(|dependency| {
                    (
                        MEMORY_GRAPH_INDEX_NAMESPACE.to_string(),
                        dependency.storage_key,
                    )
                }));
            }
            MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE => {
                let membership = decode_transaction_dependency::<MemoryGraphNodeMembership>(
                    value,
                    "memory graph node membership",
                )?;
                reads.insert((
                    MEMORY_GRAPH_NODE_NAMESPACE.to_string(),
                    membership.document_key,
                ));
                reads.insert((
                    MEMORY_GRAPH_INDEX_NAMESPACE.to_string(),
                    membership.index_key,
                ));
                reads.extend(
                    membership
                        .backlink_membership_keys
                        .into_iter()
                        .map(|key| (MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE.to_string(), key)),
                );
                reads.extend(governed_owner_storage_addresses(
                    &membership.memory_space_id,
                    &membership.mounted_subject_id,
                    &membership.owner_ref,
                    membership.owner_revision,
                )?);
            }
            MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE => {
                let membership = decode_transaction_dependency::<MemoryGraphEdgeMembership>(
                    value,
                    "memory graph edge membership",
                )?;
                reads.insert((
                    MEMORY_GRAPH_EDGE_NAMESPACE.to_string(),
                    membership.document_key,
                ));
                reads.extend(
                    membership
                        .backlink_membership_keys
                        .into_iter()
                        .map(|key| (MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE.to_string(), key)),
                );
            }
            MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE => {
                let membership = decode_transaction_dependency::<MemoryGraphBacklinkMembership>(
                    value,
                    "memory graph backlink membership",
                )?;
                reads.insert((
                    MEMORY_GRAPH_BACKLINK_NAMESPACE.to_string(),
                    membership.document_key,
                ));
            }
            LONG_TERM_CONTROL_REVISION_NAMESPACE => {
                let revision = decode_transaction_dependency::<LongTermMemoryControlRevision>(
                    value,
                    "long-term control revision",
                )?;
                reads.extend(governed_owner_storage_addresses(
                    &batch.scope.memory_space_id,
                    &batch.scope.subject_id,
                    &revision.transition.predecessor.owner_ref,
                    revision.transition.predecessor.owner_revision,
                )?);
                if let Some(successor) = revision.transition.successor {
                    reads.extend(governed_owner_storage_addresses(
                        &batch.scope.memory_space_id,
                        &batch.scope.subject_id,
                        &successor.owner_ref,
                        successor.owner_revision,
                    )?);
                }
            }
            LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE => {
                let tombstone = decode_transaction_dependency::<LongTermMemoryTombstone>(
                    value,
                    "long-term control tombstone",
                )?;
                reads.extend(governed_owner_storage_addresses(
                    &batch.scope.memory_space_id,
                    &batch.scope.subject_id,
                    &GovernedMemoryOwnerRef::new(
                        GovernedMemoryOwnerPlane::LongTerm,
                        tombstone.record_id,
                    ),
                    tombstone.last_owner_revision,
                )?);
            }
            crate::store_internal::schema::SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE => {
                let manifest = decode_transaction_dependency::<SubjectSoulScopeManifestV1>(
                    value,
                    "Subject Soul scope manifest",
                )?;
                reads.extend(
                    manifest
                        .entries
                        .into_iter()
                        .map(|entry| (entry.namespace, entry.physical_key)),
                );
            }
            crate::store_internal::schema::SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE => {
                let head = decode_transaction_dependency::<SubjectSoulLifecycleHeadV1>(
                    value,
                    "Subject Soul lifecycle head",
                )?;
                reads.extend(head.retained_revision_refs.into_iter().map(|key| {
                    (
                        crate::store_internal::schema::SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE
                            .to_string(),
                        key,
                    )
                }));
                reads.extend(head.retained_tombstone_refs.into_iter().map(|key| {
                    (
                        crate::store_internal::schema::SUBJECT_SOUL_GENERATION_TOMBSTONE_NAMESPACE
                            .to_string(),
                        key,
                    )
                }));
            }
            crate::store_internal::schema::RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE => {
                let manifest = decode_transaction_dependency::<RelationshipSourceScopeManifestV1>(
                    value,
                    "Relationship Source scope manifest",
                )?;
                reads.insert((
                    crate::store_internal::schema::RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE
                        .to_string(),
                    crate::store_internal::schema::relationship_source_revision_key(
                        &manifest.memory_space_id,
                        &manifest.relationship_id,
                        manifest.current_revision,
                    )?,
                ));
                reads.extend(manifest.retained_revision_refs.into_iter().map(|key| {
                    (
                        crate::store_internal::schema::RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE
                            .to_string(),
                        key,
                    )
                }));
            }
            crate::store_internal::schema::SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE => {
                let projection = decode_transaction_dependency::<
                    SubjectSoulRelationshipProjectionV1,
                >(value, "Subject Soul relationship projection")?;
                let soul_scope_key = crate::store_internal::schema::subject_soul_scope_key(
                    &projection.memory_space_id,
                    &projection.subject_id,
                    &projection.soul_id,
                )?;
                reads.extend([
                    (
                        crate::store_internal::schema::SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE
                            .to_string(),
                        soul_scope_key.clone(),
                    ),
                    (
                        crate::store_internal::schema::SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE
                            .to_string(),
                        soul_scope_key,
                    ),
                    (
                        crate::store_internal::schema::SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE
                            .to_string(),
                        crate::store_internal::schema::subject_soul_revision_material_key(
                            &projection.memory_space_id,
                            &projection.subject_id,
                            &projection.soul_id,
                            projection.generation,
                            projection.soul_revision,
                        )?,
                    ),
                    (
                        crate::store_internal::schema::RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE
                            .to_string(),
                        crate::store_internal::schema::relationship_source_revision_key(
                            &projection.memory_space_id,
                            &projection.relationship_id,
                            projection.relationship_source_revision,
                        )?,
                    ),
                    (
                        crate::store_internal::schema::RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE
                            .to_string(),
                        crate::store_internal::schema::relationship_source_scope_key(
                            &projection.memory_space_id,
                            &projection.relationship_id,
                        )?,
                    ),
                ]);
            }
            _ => {}
        }
    }
    Ok(reads)
}

fn governed_owner_storage_addresses(
    memory_space_id: &str,
    _mounted_subject_id: &str,
    owner_ref: &GovernedMemoryOwnerRef,
    owner_revision: u64,
) -> Result<Vec<(String, String)>> {
    match owner_ref.owner_plane {
        GovernedMemoryOwnerPlane::LongTerm => Ok(vec![
            (
                crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE.to_string(),
                long_term_version_head_key(memory_space_id, memory_space_id, owner_ref)?,
            ),
            (
                crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE.to_string(),
                long_term_version_material_key(
                    memory_space_id,
                    memory_space_id,
                    owner_ref,
                    owner_revision,
                )?,
            ),
        ]),
        GovernedMemoryOwnerPlane::EvidenceDocument => Ok(vec![(
            GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
            scoped_governed_evidence_document_key(memory_space_id, &owner_ref.owner_id)?,
        )]),
        _ => Err(Error::config(
            "memory_write_transaction_dependency_read_set",
            format!(
                "unsupported governed owner plane {}",
                owner_ref.owner_plane.as_str()
            ),
        )),
    }
}

fn decode_transaction_dependency<T: DeserializeOwned>(
    value: &serde_json::Value,
    label: &str,
) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|error| {
        Error::config(
            "memory_write_transaction_dependency_read_set",
            format!("invalid {label}: {error}"),
        )
    })
}

fn validate_evidence_source_ref_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    max_scope_entries: usize,
) -> Result<()> {
    let stage = "memory_write_transaction_evidence_source_ref_post_image_invalid";
    let mut owner_keys = BTreeSet::new();
    let mut source_ref_keys = BTreeSet::new();
    let mut manifest_touched = false;
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE =>
            {
                owner_keys.insert(key.clone());
            }
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE =>
            {
                source_ref_keys.insert(key.clone());
            }
            StoreMutation::PutJson { namespace, .. }
            | StoreMutation::DeleteJson { namespace, .. }
                if namespace == GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE =>
            {
                manifest_touched = true;
            }
            _ => {}
        }
    }
    if owner_keys.is_empty() && source_ref_keys.is_empty() && !manifest_touched {
        return Ok(());
    }

    for source_ref_key in &source_ref_keys {
        let source_ref_address = (
            GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
            source_ref_key.clone(),
        );
        for image in [before, after] {
            let Some(source_ref) = decode_optional_governed_doc::<GovernedEvidenceSourceRef>(
                image.json.get(&source_ref_address),
                stage,
            )?
            else {
                continue;
            };
            owner_keys.insert(validate_evidence_source_ref_image(
                &source_ref,
                source_ref_key,
                batch,
                image,
                stage,
            )?);
        }
    }

    for owner_key in owner_keys {
        let owner_address = (GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(), owner_key);
        let before_owner = decode_optional_governed_doc::<GovernedEvidenceDocument>(
            before.json.get(&owner_address),
            stage,
        )?;
        let after_owner = decode_optional_governed_doc::<GovernedEvidenceDocument>(
            after.json.get(&owner_address),
            stage,
        )?;

        if let Some(before_owner) = before_owner.as_ref() {
            validate_evidence_owner_source_ref_image(
                before_owner,
                &owner_address.1,
                batch,
                before,
                stage,
            )?;
        }
        if let Some(after_owner) = after_owner.as_ref() {
            validate_evidence_owner_source_ref_image(
                after_owner,
                &owner_address.1,
                batch,
                after,
                stage,
            )?;
        }
    }
    validate_evidence_source_claim_manifest_image(before, batch, false, max_scope_entries, stage)?;
    validate_evidence_source_claim_manifest_image(after, batch, true, max_scope_entries, stage)?;
    Ok(())
}

fn validate_evidence_source_claim_manifest_image(
    image: &BackendTransactionState,
    batch: &StoreMutationBatch,
    required: bool,
    max_scope_entries: usize,
    stage: &'static str,
) -> Result<()> {
    let manifest_key = governed_evidence_source_claim_manifest_key(
        &batch.scope.memory_space_id,
        &batch.scope.subject_id,
    )?;
    let manifest = decode_optional_governed_doc::<GovernedEvidenceSourceClaimManifest>(
        image.json.get(&(
            GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
            manifest_key,
        )),
        stage,
    )?;
    let Some(manifest) = manifest else {
        return if required {
            Err(Error::config(
                stage,
                "evidence source claim scope manifest is missing",
            ))
        } else {
            Ok(())
        };
    };
    manifest.validate_exact(
        &batch.scope.memory_space_id,
        &batch.scope.subject_id,
        manifest.owner_claim_bindings.clone(),
        max_scope_entries,
    )?;
    for owner_key in &manifest.owner_keys {
        let owner = decode_optional_governed_doc::<GovernedEvidenceDocument>(
            image.json.get(&(
                GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                owner_key.clone(),
            )),
            stage,
        )?
        .ok_or_else(|| Error::config(stage, "manifest evidence owner is missing"))?;
        validate_evidence_owner_source_ref_image(&owner, owner_key, batch, image, stage)?;
    }
    for claim_key in &manifest.claim_keys {
        let claim = decode_optional_governed_doc::<GovernedEvidenceSourceRef>(
            image.json.get(&(
                GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                claim_key.clone(),
            )),
            stage,
        )?
        .ok_or_else(|| Error::config(stage, "manifest evidence source claim is missing"))?;
        validate_evidence_source_ref_image(&claim, claim_key, batch, image, stage)?;
    }
    Ok(())
}

fn evidence_source_ref_owner_key(
    source_ref: &GovernedEvidenceSourceRef,
    source_ref_key: &str,
    batch: &StoreMutationBatch,
    stage: &'static str,
) -> Result<String> {
    if source_ref.physical_key != source_ref_key
        || source_ref.memory_space_id != batch.scope.memory_space_id
        || source_ref.mounted_subject_id != batch.scope.subject_id
        || source_ref.owner_ref.owner_plane != GovernedMemoryOwnerPlane::EvidenceDocument
    {
        return Err(Error::config(
            stage,
            "touched evidence source claim does not match canonical typed owner scope",
        ));
    }
    scoped_governed_evidence_document_key(
        &source_ref.memory_space_id,
        &source_ref.owner_ref.owner_id,
    )
    .map_err(|error| Error::config(stage, format!("evidence source claim owner key: {error:?}")))
}

fn validate_evidence_source_ref_image(
    source_ref: &GovernedEvidenceSourceRef,
    source_ref_key: &str,
    batch: &StoreMutationBatch,
    image: &BackendTransactionState,
    stage: &'static str,
) -> Result<String> {
    let owner_key = evidence_source_ref_owner_key(source_ref, source_ref_key, batch, stage)?;
    let owner_address = (
        GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
        owner_key.clone(),
    );
    let owner = decode_optional_governed_doc::<GovernedEvidenceDocument>(
        image.json.get(&owner_address),
        stage,
    )?
    .ok_or_else(|| Error::config(stage, "evidence source claim image owner is missing"))?;
    validate_evidence_owner_source_ref_image(&owner, &owner_key, batch, image, stage)?;
    validate_governed_evidence_source_ref(&owner, source_ref)
        .map_err(|error| Error::config(stage, format!("{error:?}")))?;
    Ok(owner_key)
}

fn validate_evidence_owner_source_ref_image(
    owner: &GovernedEvidenceDocument,
    owner_key: &str,
    batch: &StoreMutationBatch,
    image: &BackendTransactionState,
    stage: &'static str,
) -> Result<()> {
    validate_governed_evidence_document(owner).map_err(|error| {
        Error::config(stage, format!("invalid evidence owner image: {error:?}"))
    })?;
    let owner_ref = GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::EvidenceDocument,
        owner.document_id.clone(),
    );
    let expected_owner_key =
        scoped_governed_evidence_document_key(&owner.memory_space_id, &owner_ref.owner_id)
            .map_err(|error| Error::config(stage, format!("evidence owner key: {error:?}")))?;
    if owner.physical_key != owner_key
        || expected_owner_key != owner_key
        || owner.memory_space_id != batch.scope.memory_space_id
        || owner.mounted_subject_id != batch.scope.subject_id
    {
        return Err(Error::config(
            stage,
            "evidence owner image does not match canonical typed owner scope",
        ));
    }
    let expected_source_ref = governed_evidence_source_ref_from_document(owner)?;
    let source_ref_address = (
        GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
        expected_source_ref.physical_key.clone(),
    );
    let source_ref = decode_optional_governed_doc::<GovernedEvidenceSourceRef>(
        image.json.get(&source_ref_address),
        stage,
    )?
    .ok_or_else(|| {
        Error::config(
            stage,
            "evidence owner image is missing its typed source claim",
        )
    })?;
    if source_ref.owner_ref != owner_ref {
        return Err(Error::config(
            stage,
            "evidence source claim owner ref does not close back to its owner",
        ));
    }
    validate_governed_evidence_source_ref(owner, &source_ref)
        .map_err(|error| Error::config(stage, format!("{error:?}")))
}

fn decode_optional_governed_doc<T: DeserializeOwned>(
    value: Option<&serde_json::Value>,
    stage: &'static str,
) -> Result<Option<T>> {
    value
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| Error::config(stage, error.to_string()))
}

fn governed_image<T: DeserializeOwned>(
    namespace: &str,
    key: &str,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<GovernedDocumentImage<T>> {
    let decode = |value: Option<&serde_json::Value>| {
        value
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| {
                Error::config(
                    "memory_write_transaction_post_image_decode_failed",
                    format!("{namespace}/{key}: {error}"),
                )
            })
    };
    Ok(GovernedDocumentImage {
        physical_key: key.to_string(),
        before: decode(before.json.get(&(namespace.to_string(), key.to_string())))?,
        after: decode(after.json.get(&(namespace.to_string(), key.to_string())))?,
    })
}

fn long_term_current_material_in_state(
    state: &BackendTransactionState,
    memory_space_id: &str,
    factual_owner_id: &str,
    owner_ref: &GovernedMemoryOwnerRef,
) -> Result<Option<(String, LongTermMemoryVersionMaterial)>> {
    let head_key = long_term_version_head_key(memory_space_id, factual_owner_id, owner_ref)?;
    let Some(head_value) = state.json.get(&(
        crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE.to_string(),
        head_key.clone(),
    )) else {
        return Ok(None);
    };
    let head = serde_json::from_value::<LongTermMemoryHeadManifest>(head_value.clone()).map_err(
        |error| {
            Error::config(
                "memory_write_transaction_typed_owner_resolver_failed",
                format!("invalid long-term head {head_key}: {error}"),
            )
        },
    )?;
    if !head.validate_contract().accepted
        || head.memory_space_id != memory_space_id
        || head.factual_owner_id != factual_owner_id
        || head.owner_ref != *owner_ref
    {
        return Err(Error::config(
            "memory_write_transaction_typed_owner_resolver_failed",
            format!("long-term head {head_key} has invalid scope or owner binding"),
        ));
    }
    if head.terminal_transition_ref.is_some() {
        return Ok(None);
    }
    let retained = head
        .retained_revision_digests
        .iter()
        .find(|retained| retained.owner_revision == head.current_revision)
        .ok_or_else(|| {
            Error::config(
                "memory_write_transaction_typed_owner_resolver_failed",
                format!("long-term head {head_key} has no current retained material"),
            )
        })?;
    let material_key = long_term_version_material_key(
        memory_space_id,
        factual_owner_id,
        owner_ref,
        head.current_revision,
    )?;
    let material_value = state
        .json
        .get(&(
            crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE.to_string(),
            material_key.clone(),
        ))
        .ok_or_else(|| {
            Error::config(
                "memory_write_transaction_typed_owner_resolver_failed",
                format!("long-term current material {material_key} is missing"),
            )
        })?;
    let material = serde_json::from_value::<LongTermMemoryVersionMaterial>(material_value.clone())
        .map_err(|error| {
            Error::config(
                "memory_write_transaction_typed_owner_resolver_failed",
                format!("invalid long-term material {material_key}: {error}"),
            )
        })?;
    if !material.validate_contract().accepted
        || material.memory_space_id != memory_space_id
        || material.factual_owner_id != factual_owner_id
        || material.owner_ref != *owner_ref
        || material.owner_revision != head.current_revision
        || material.content_digest != retained.content_digest
    {
        return Err(Error::config(
            "memory_write_transaction_typed_owner_resolver_failed",
            format!("long-term material {material_key} differs from its exact head binding"),
        ));
    }
    Ok(Some((material_key, material)))
}

fn governed_long_term_material_image(
    memory_space_id: &str,
    factual_owner_id: &str,
    owner_ref: &GovernedMemoryOwnerRef,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<LongTermMemoryVersionMaterialImage> {
    let before_material =
        long_term_current_material_in_state(before, memory_space_id, factual_owner_id, owner_ref)?;
    let after_material =
        long_term_current_material_in_state(after, memory_space_id, factual_owner_id, owner_ref)?;
    Ok(LongTermMemoryVersionMaterialImage {
        before_physical_key: before_material.as_ref().map(|(key, _)| key.clone()),
        before: before_material.map(|(_, material)| material),
        after_physical_key: after_material.as_ref().map(|(key, _)| key.clone()),
        after: after_material.map(|(_, material)| material),
    })
}

fn ensure_post_image_validation(
    stage: &'static str,
    validation: GovernedPostImageValidation,
) -> Result<()> {
    if validation.accepted {
        return Ok(());
    }
    Err(Error::config(stage, validation.failures.join(",")))
}

fn batch_mutates_namespace(batch: &StoreMutationBatch, namespace: &str) -> bool {
    batch.mutations.iter().any(|mutation| match mutation {
        StoreMutation::PutJson {
            namespace: mutated, ..
        }
        | StoreMutation::DeleteJson {
            namespace: mutated, ..
        } => mutated == namespace,
        _ => false,
    })
}

fn is_graph_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        MEMORY_GRAPH_MANIFEST_NAMESPACE
            | MEMORY_GRAPH_REVISION_NAMESPACE
            | MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE
            | MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE
            | MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE
            | MEMORY_GRAPH_INDEX_NAMESPACE
            | MEMORY_GRAPH_NODE_NAMESPACE
            | MEMORY_GRAPH_EDGE_NAMESPACE
            | MEMORY_GRAPH_BACKLINK_NAMESPACE
    )
}

fn graph_effect_event_scope(
    batch_scope: &StoreEventScope,
    namespace: &str,
    key: &str,
    graph_scopes: &[(String, String, String)],
) -> StoreEventScope {
    if !is_graph_namespace(namespace) {
        return batch_scope.clone();
    }
    graph_scopes
        .iter()
        .find(|(_, _, scope_digest)| key.starts_with(&format!("scope:{scope_digest}:doc:")))
        .map(|(_, mounted_subject_id, _)| {
            batch_scope.clone().with_subject(mounted_subject_id.clone())
        })
        .unwrap_or_else(|| batch_scope.clone())
}

fn batch_json_keys(batch: &StoreMutationBatch, namespace: &str) -> BTreeSet<String> {
    batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace: mutated,
                key,
                ..
            }
            | StoreMutation::DeleteJson {
                namespace: mutated,
                key,
                ..
            } if mutated == namespace => Some(key.clone()),
            _ => None,
        })
        .collect()
}

fn batch_graph_scope_json_keys(
    batch: &StoreMutationBatch,
    namespace: &str,
    scope_digest: &str,
) -> BTreeSet<String> {
    let prefix = format!("scope:{scope_digest}:doc:");
    batch_json_keys(batch, namespace)
        .into_iter()
        .filter(|key| key.starts_with(&prefix))
        .collect()
}

fn scoped_graph_state_keys(
    state: &BackendTransactionState,
    namespace: &str,
    scope_digest: &str,
) -> BTreeSet<String> {
    let prefix = format!("scope:{scope_digest}:doc:");
    state
        .json
        .keys()
        .filter(|(candidate_namespace, key)| {
            candidate_namespace == namespace && key.starts_with(&prefix)
        })
        .map(|(_, key)| key.clone())
        .collect()
}

fn validate_facet_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<()> {
    let facet_touched = batch_mutates_namespace(
        batch,
        crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
    ) || batch_mutates_namespace(
        batch,
        crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
    ) || batch_mutates_namespace(batch, GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE)
        || batch_mutates_namespace(batch, MEMORY_FACET_INDEX_NAMESPACE)
        || batch_mutates_namespace(batch, MEMORY_FACET_POSTING_NAMESPACE);
    if !facet_touched {
        return Ok(());
    }
    let memory_space_id = batch.scope.memory_space_id.as_str();
    let facet_owner_keys = batch_json_keys(batch, MEMORY_FACET_INDEX_NAMESPACE);
    let facet_posting_keys = batch_json_keys(batch, MEMORY_FACET_POSTING_NAMESPACE);
    let long_term_facet_transaction = batch_mutates_namespace(
        batch,
        crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
    ) || batch_mutates_namespace(
        batch,
        crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
    ) || facet_owner_keys.iter().any(|key| {
        [after, before].into_iter().any(|state| {
            state
                .json
                .get(&(MEMORY_FACET_INDEX_NAMESPACE.to_string(), key.clone()))
                .and_then(|value| serde_json::from_value::<MemoryFacetIndexDoc>(value.clone()).ok())
                .is_some_and(|doc| doc.owner_ref.owner_plane == GovernedMemoryOwnerPlane::LongTerm)
        })
    }) || facet_posting_keys.iter().any(|key| {
        [after, before].into_iter().any(|state| {
            let value = state
                .json
                .get(&(MEMORY_FACET_POSTING_NAMESPACE.to_string(), key.clone()));
            value.is_some_and(|value| {
                serde_json::from_value::<MemoryFacetIndexManifest>(value.clone())
                    .is_ok_and(|manifest| manifest.subject_id == memory_space_id)
                    || serde_json::from_value::<MemoryFacetPostingDoc>(value.clone())
                        .is_ok_and(|posting| posting.subject_id == memory_space_id)
            })
        })
    });
    let subject_id = if long_term_facet_transaction {
        memory_space_id
    } else {
        batch.scope.subject_id.as_str()
    };
    let manifest_key = memory_facet_manifest_key(memory_space_id, subject_id).map_err(|error| {
        Error::config(
            "memory_write_transaction_owner_facet_post_image_invalid",
            format!("facet manifest key: {error:?}"),
        )
    })?;
    let manifest = governed_image::<MemoryFacetIndexManifest>(
        MEMORY_FACET_POSTING_NAMESPACE,
        &manifest_key,
        before,
        after,
    )?;

    let mut owner_refs = BTreeSet::new();
    let mut posting_keys = BTreeSet::new();
    for value in [manifest.before.as_ref(), manifest.after.as_ref()]
        .into_iter()
        .flatten()
    {
        owner_refs.extend(
            value
                .owner_versions
                .iter()
                .map(|owner| owner.owner_ref.clone()),
        );
        posting_keys.extend(
            value
                .posting_revisions
                .iter()
                .map(|posting| posting.posting_key.clone()),
        );
    }

    let mut facet_keys = batch_json_keys(batch, MEMORY_FACET_INDEX_NAMESPACE);
    for owner_ref in owner_refs.clone() {
        facet_keys.insert(
            scoped_memory_facet_owner_storage_key(memory_space_id, subject_id, &owner_ref)
                .map_err(|error| {
                    Error::config(
                        "memory_write_transaction_owner_facet_post_image_invalid",
                        format!("facet owner key: {error:?}"),
                    )
                })?,
        );
    }
    let mut facet_owners = Vec::new();
    for key in facet_keys {
        let image = governed_image::<MemoryFacetIndexDoc>(
            MEMORY_FACET_INDEX_NAMESPACE,
            &key,
            before,
            after,
        )?;
        if let Some(doc) = image.after.as_ref().or(image.before.as_ref()) {
            owner_refs.insert(doc.owner_ref.clone());
        }
        facet_owners.push(image);
    }

    posting_keys.extend(batch_json_keys(batch, MEMORY_FACET_POSTING_NAMESPACE));
    posting_keys.remove(&manifest_key);
    let postings = posting_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryFacetPostingDoc>(
                MEMORY_FACET_POSTING_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace
                == crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE =>
            {
                let material =
                    serde_json::from_value::<LongTermMemoryVersionMaterial>(value.clone())
                        .map_err(|error| {
                            Error::config(
                                "memory_write_transaction_post_image_decode_failed",
                                error.to_string(),
                            )
                        })?;
                owner_refs.insert(material.owner_ref);
            }
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE => {
                let head = serde_json::from_value::<LongTermMemoryHeadManifest>(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "memory_write_transaction_post_image_decode_failed",
                            error.to_string(),
                        )
                    })?;
                owner_refs.insert(head.owner_ref);
            }
            StoreMutation::PutJson {
                namespace,
                value,
                record_key,
                ..
            } if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE => {
                let owner = serde_json::from_value::<GovernedEvidenceDocument>(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "memory_write_transaction_post_image_decode_failed",
                            error.to_string(),
                        )
                    })?;
                owner_refs.insert(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    owner.document_id,
                ));
                owner_refs.insert(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    record_key.clone(),
                ));
            }
            StoreMutation::DeleteJson {
                namespace,
                record_key,
                ..
            } if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE => {
                owner_refs.insert(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    record_key.clone(),
                ));
            }
            _ => {}
        }
    }
    let mut long_term_owners = Vec::new();
    let mut evidence_document_owners = Vec::new();
    for owner_ref in owner_refs {
        match owner_ref.owner_plane {
            GovernedMemoryOwnerPlane::LongTerm => {
                long_term_owners.push(governed_long_term_material_image(
                    memory_space_id,
                    memory_space_id,
                    &owner_ref,
                    before,
                    after,
                )?);
            }
            GovernedMemoryOwnerPlane::EvidenceDocument => {
                let key =
                    scoped_governed_evidence_document_key(memory_space_id, &owner_ref.owner_id)?;
                evidence_document_owners.push(governed_image::<GovernedEvidenceDocument>(
                    GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
                    &key,
                    before,
                    after,
                )?);
            }
            _ => {
                return Err(Error::config(
                    "memory_write_transaction_owner_facet_post_image_invalid",
                    format!(
                        "unsupported governed facet owner plane {}",
                        owner_ref.owner_plane.as_str()
                    ),
                ));
            }
        }
    }

    ensure_post_image_validation(
        "memory_write_transaction_owner_facet_post_image_invalid",
        validate_memory_facet_post_image(&MemoryFacetPostImageClosure {
            memory_space_id: memory_space_id.to_string(),
            long_term_owner_id: memory_space_id.to_string(),
            mounted_subject_id: subject_id.to_string(),
            long_term_owners,
            evidence_document_owners,
            facet_owners,
            postings,
            manifest,
        }),
    )
}

#[derive(Default)]
struct GraphDependencyKeys {
    revisions: BTreeSet<String>,
    node_memberships: BTreeSet<String>,
    edge_memberships: BTreeSet<String>,
    backlink_memberships: BTreeSet<String>,
    indexes: BTreeSet<String>,
}

fn graph_dependency_keys(manifests: &[&Option<MemoryGraphScopeManifest>]) -> GraphDependencyKeys {
    let mut keys = GraphDependencyKeys::default();
    for manifest in manifests.iter().filter_map(|manifest| (*manifest).as_ref()) {
        keys.revisions.insert(manifest.revision.storage_key.clone());
        keys.node_memberships.extend(
            manifest
                .node_memberships
                .iter()
                .map(|item| item.storage_key.clone()),
        );
        keys.edge_memberships.extend(
            manifest
                .edge_memberships
                .iter()
                .map(|item| item.storage_key.clone()),
        );
        keys.backlink_memberships.extend(
            manifest
                .backlink_memberships
                .iter()
                .map(|item| item.storage_key.clone()),
        );
        keys.indexes.extend(
            manifest
                .recall_indexes
                .iter()
                .map(|item| item.storage_key.clone()),
        );
    }
    keys
}

fn validate_graph_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    graph_repair_authorized: bool,
) -> Result<()> {
    let graph_namespaces = [
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        MEMORY_GRAPH_REVISION_NAMESPACE,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_INDEX_NAMESPACE,
        MEMORY_GRAPH_NODE_NAMESPACE,
        MEMORY_GRAPH_EDGE_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_NAMESPACE,
    ];
    let graph_touched = graph_namespaces
        .iter()
        .any(|namespace| batch_mutates_namespace(batch, namespace));
    let evidence_owner_touched =
        batch_mutates_namespace(batch, GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE);
    if !graph_touched && !evidence_owner_touched {
        return Ok(());
    }
    let manifest_keys = batch_json_keys(batch, MEMORY_GRAPH_MANIFEST_NAMESPACE);
    if manifest_keys.is_empty() {
        return Err(Error::config(
            "memory_write_transaction_graph_post_image_invalid",
            "governed graph effects require a typed graph manifest mutation",
        ));
    }
    let mut scopes = BTreeSet::new();
    for manifest_key in manifest_keys {
        let manifest = governed_image::<MemoryGraphScopeManifest>(
            MEMORY_GRAPH_MANIFEST_NAMESPACE,
            &manifest_key,
            before,
            after,
        )?;
        let Some(scope_manifest) = manifest.after.as_ref().or(manifest.before.as_ref()) else {
            return Err(Error::config(
                "memory_write_transaction_graph_post_image_invalid",
                "graph manifest mutation has no typed before or after image",
            ));
        };
        if scope_manifest.memory_space_id != batch.scope.memory_space_id
            || manifest_key
                != memory_graph_scope_manifest_key(
                    &scope_manifest.memory_space_id,
                    &scope_manifest.mounted_subject_id,
                )
        {
            return Err(Error::config(
                "memory_write_transaction_graph_post_image_invalid",
                "graph manifest scope is non-canonical or crosses MemorySpace ownership",
            ));
        }
        scopes.insert((
            scope_manifest.memory_space_id.clone(),
            scope_manifest.mounted_subject_id.clone(),
        ));
    }
    for (memory_space_id, subject_id) in scopes {
        validate_graph_scope_post_image(
            batch,
            before,
            after,
            graph_repair_authorized,
            &memory_space_id,
            &subject_id,
        )?;
    }
    Ok(())
}

fn validate_graph_scope_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    graph_repair_authorized: bool,
    memory_space_id: &str,
    subject_id: &str,
) -> Result<()> {
    let graph_namespaces = [
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        MEMORY_GRAPH_REVISION_NAMESPACE,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_INDEX_NAMESPACE,
        MEMORY_GRAPH_NODE_NAMESPACE,
        MEMORY_GRAPH_EDGE_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_NAMESPACE,
    ];
    let scope_digest = memory_graph_scope_digest(memory_space_id, subject_id);
    let graph_touched = graph_namespaces
        .iter()
        .any(|namespace| !batch_graph_scope_json_keys(batch, namespace, &scope_digest).is_empty());
    let evidence_owner_touched =
        batch_mutates_namespace(batch, GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE);
    let manifest_key = memory_graph_scope_manifest_key(memory_space_id, subject_id);
    let manifest = governed_image::<MemoryGraphScopeManifest>(
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        &manifest_key,
        before,
        after,
    )?;
    let scoped_after_manifest_keys =
        scoped_graph_state_keys(after, MEMORY_GRAPH_MANIFEST_NAMESPACE, &scope_digest);
    let expected_after_manifest_keys = manifest
        .after
        .as_ref()
        .map(|_| BTreeSet::from([manifest_key.clone()]))
        .unwrap_or_default();
    if scoped_after_manifest_keys != expected_after_manifest_keys {
        return Err(Error::config(
            "memory_write_transaction_graph_post_image_invalid",
            "graph after-state manifest keys must exactly match the transaction scope closure",
        ));
    }
    let GraphDependencyKeys {
        revisions: expected_after_revision_keys,
        node_memberships: expected_after_node_membership_keys,
        edge_memberships: expected_after_edge_membership_keys,
        backlink_memberships: expected_after_backlink_membership_keys,
        indexes: expected_after_index_keys,
    } = graph_dependency_keys(&[&manifest.after]);
    for (namespace, expected) in [
        (
            MEMORY_GRAPH_REVISION_NAMESPACE,
            expected_after_revision_keys,
        ),
        (
            MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
            expected_after_node_membership_keys,
        ),
        (
            MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
            expected_after_edge_membership_keys,
        ),
        (
            MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
            expected_after_backlink_membership_keys,
        ),
        (MEMORY_GRAPH_INDEX_NAMESPACE, expected_after_index_keys),
    ] {
        if scoped_graph_state_keys(after, namespace, &scope_digest) != expected {
            return Err(Error::config(
                "memory_write_transaction_graph_post_image_invalid",
                format!("graph after-state {namespace} keys do not exactly match the manifest"),
            ));
        }
    }
    let scoped_graph_after_present = graph_namespaces
        .iter()
        .any(|namespace| !scoped_graph_state_keys(after, namespace, &scope_digest).is_empty());
    if !graph_touched
        && !evidence_owner_touched
        && manifest.before.is_none()
        && manifest.after.is_none()
        && !scoped_graph_after_present
    {
        return Ok(());
    }
    if graph_touched
        && batch_graph_scope_json_keys(batch, MEMORY_GRAPH_MANIFEST_NAMESPACE, &scope_digest)
            != BTreeSet::from([manifest_key.clone()])
    {
        return Err(Error::config(
            "memory_write_transaction_graph_post_image_invalid",
            "graph manifest mutations must exactly match the transaction scope manifest",
        ));
    }

    let GraphDependencyKeys {
        revisions: mut revision_keys,
        node_memberships: mut node_membership_keys,
        edge_memberships: mut edge_membership_keys,
        backlink_memberships: mut backlink_membership_keys,
        indexes: mut index_keys,
    } = graph_dependency_keys(&[&manifest.before, &manifest.after]);
    revision_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_REVISION_NAMESPACE,
        &scope_digest,
    ));
    node_membership_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        &scope_digest,
    ));
    edge_membership_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        &scope_digest,
    ));
    backlink_membership_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        &scope_digest,
    ));
    index_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_INDEX_NAMESPACE,
        &scope_digest,
    ));
    if graph_touched
        && batch_graph_scope_json_keys(batch, MEMORY_GRAPH_REVISION_NAMESPACE, &scope_digest)
            != revision_keys
    {
        return Err(Error::config(
            "memory_write_transaction_graph_post_image_invalid",
            "graph revision mutations must exactly match the scope manifest closure",
        ));
    }
    node_membership_keys.extend(batch_graph_scope_json_keys(
        batch,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        &scope_digest,
    ));
    edge_membership_keys.extend(batch_graph_scope_json_keys(
        batch,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        &scope_digest,
    ));
    backlink_membership_keys.extend(batch_graph_scope_json_keys(
        batch,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        &scope_digest,
    ));
    index_keys.extend(batch_graph_scope_json_keys(
        batch,
        MEMORY_GRAPH_INDEX_NAMESPACE,
        &scope_digest,
    ));

    if revision_keys.len() != 1 {
        return Err(Error::config(
            "memory_write_transaction_graph_post_image_invalid",
            "graph closure must resolve to exactly one revision key",
        ));
    }
    let revision_key = revision_keys
        .into_iter()
        .next()
        .expect("revision key cardinality checked");
    let revision = governed_image::<MemoryGraphRevisionDoc>(
        MEMORY_GRAPH_REVISION_NAMESPACE,
        &revision_key,
        before,
        after,
    )?;
    let node_memberships = node_membership_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphNodeMembership>(
                MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let edge_memberships = edge_membership_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphEdgeMembership>(
                MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let backlink_memberships = backlink_membership_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphBacklinkMembership>(
                MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let indexes = index_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphRecallIndexDoc>(
                MEMORY_GRAPH_INDEX_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    let expected_after_node_keys = node_memberships
        .iter()
        .filter_map(|image| image.after.as_ref().map(|value| value.document_key.clone()))
        .collect::<BTreeSet<_>>();
    let expected_after_edge_keys = edge_memberships
        .iter()
        .filter_map(|image| image.after.as_ref().map(|value| value.document_key.clone()))
        .collect::<BTreeSet<_>>();
    let expected_after_backlink_keys = backlink_memberships
        .iter()
        .filter_map(|image| image.after.as_ref().map(|value| value.document_key.clone()))
        .collect::<BTreeSet<_>>();
    for (namespace, expected) in [
        (MEMORY_GRAPH_NODE_NAMESPACE, expected_after_node_keys),
        (MEMORY_GRAPH_EDGE_NAMESPACE, expected_after_edge_keys),
        (
            MEMORY_GRAPH_BACKLINK_NAMESPACE,
            expected_after_backlink_keys,
        ),
    ] {
        if scoped_graph_state_keys(after, namespace, &scope_digest) != expected {
            return Err(Error::config(
                "memory_write_transaction_graph_post_image_invalid",
                format!("graph after-state {namespace} keys do not exactly match memberships"),
            ));
        }
    }

    let mut owner_refs = BTreeSet::new();
    for namespace in [
        crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
        crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
    ] {
        for key in batch_json_keys(batch, namespace) {
            let owner_ref = match namespace {
                crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE => {
                    let image = governed_image::<LongTermMemoryVersionMaterial>(
                        namespace, &key, before, after,
                    )?;
                    image
                        .after
                        .as_ref()
                        .or(image.before.as_ref())
                        .map(|material| material.owner_ref.clone())
                }
                crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE => {
                    let image = governed_image::<LongTermMemoryHeadManifest>(
                        namespace, &key, before, after,
                    )?;
                    image
                        .after
                        .as_ref()
                        .or(image.before.as_ref())
                        .map(|head| head.owner_ref.clone())
                }
                _ => None,
            };
            if let Some(owner_ref) = owner_ref {
                owner_refs.insert(owner_ref);
            }
        }
    }
    for key in batch_json_keys(batch, GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE) {
        let image = governed_image::<GovernedEvidenceDocument>(
            GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
            &key,
            before,
            after,
        )?;
        if let Some(owner) = image.after.as_ref().or(image.before.as_ref()) {
            owner_refs.insert(GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::EvidenceDocument,
                owner.document_id.clone(),
            ));
        }
    }
    let mut node_keys =
        batch_graph_scope_json_keys(batch, MEMORY_GRAPH_NODE_NAMESPACE, &scope_digest);
    node_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_NODE_NAMESPACE,
        &scope_digest,
    ));
    for membership in &node_memberships {
        for value in membership.before.iter().chain(membership.after.iter()) {
            owner_refs.insert(value.owner_ref.clone());
        }
        if let Some(value) = membership.after.as_ref().or(membership.before.as_ref()) {
            node_keys.insert(value.document_key.clone());
        }
    }
    let mut edge_keys =
        batch_graph_scope_json_keys(batch, MEMORY_GRAPH_EDGE_NAMESPACE, &scope_digest);
    edge_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_EDGE_NAMESPACE,
        &scope_digest,
    ));
    for membership in &edge_memberships {
        if let Some(value) = membership.after.as_ref().or(membership.before.as_ref()) {
            edge_keys.insert(value.document_key.clone());
        }
    }
    let mut backlink_keys =
        batch_graph_scope_json_keys(batch, MEMORY_GRAPH_BACKLINK_NAMESPACE, &scope_digest);
    backlink_keys.extend(scoped_graph_state_keys(
        after,
        MEMORY_GRAPH_BACKLINK_NAMESPACE,
        &scope_digest,
    ));
    for membership in &backlink_memberships {
        if let Some(value) = membership.after.as_ref().or(membership.before.as_ref()) {
            backlink_keys.insert(value.document_key.clone());
        }
    }
    let nodes = node_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphNode>(MEMORY_GRAPH_NODE_NAMESPACE, &key, before, after)
        })
        .collect::<Result<Vec<_>>>()?;
    let edges = edge_keys
        .into_iter()
        .map(|key| {
            governed_image::<MemoryGraphEdge>(MEMORY_GRAPH_EDGE_NAMESPACE, &key, before, after)
        })
        .collect::<Result<Vec<_>>>()?;
    let backlinks = backlink_keys
        .into_iter()
        .map(|key| {
            governed_image::<EvidenceBacklink>(MEMORY_GRAPH_BACKLINK_NAMESPACE, &key, before, after)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut long_term_owners = Vec::new();
    let mut evidence_document_owners = Vec::new();
    for owner_ref in owner_refs {
        match owner_ref.owner_plane {
            GovernedMemoryOwnerPlane::LongTerm => {
                long_term_owners.push(governed_long_term_material_image(
                    memory_space_id,
                    memory_space_id,
                    &owner_ref,
                    before,
                    after,
                )?);
            }
            GovernedMemoryOwnerPlane::EvidenceDocument => {
                let key =
                    scoped_governed_evidence_document_key(memory_space_id, &owner_ref.owner_id)?;
                evidence_document_owners.push(governed_image::<GovernedEvidenceDocument>(
                    GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
                    &key,
                    before,
                    after,
                )?);
            }
            _ => {
                return Err(Error::config(
                    "memory_write_transaction_graph_post_image_invalid",
                    format!(
                        "unsupported governed graph owner plane {}",
                        owner_ref.owner_plane.as_str()
                    ),
                ));
            }
        }
    }

    ensure_post_image_validation(
        "memory_write_transaction_graph_post_image_invalid",
        validate_memory_graph_post_image(&MemoryGraphPostImageClosure {
            memory_space_id: memory_space_id.to_string(),
            long_term_owner_id: memory_space_id.to_string(),
            mounted_subject_id: subject_id.to_string(),
            allow_missing_before_owners: graph_repair_authorized,
            validate_transition_successors: true,
            long_term_owners,
            evidence_document_owners,
            manifest,
            revision,
            node_memberships,
            edge_memberships,
            backlink_memberships,
            indexes,
            nodes,
            edges,
            backlinks,
        }),
    )
}

fn validate_control_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<()> {
    let control_namespaces = [
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
    ];
    if !control_namespaces
        .iter()
        .any(|namespace| batch_mutates_namespace(batch, namespace))
    {
        return Ok(());
    }
    let memory_space_id = batch.scope.memory_space_id.as_str();
    let revisions = batch_json_keys(batch, LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .into_iter()
        .map(|key| {
            governed_image::<LongTermMemoryControlRevision>(
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let tombstones = batch_json_keys(batch, LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE)
        .into_iter()
        .map(|key| {
            governed_image::<LongTermMemoryTombstone>(
                LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let policies = batch_json_keys(batch, LONG_TERM_GOVERNANCE_POLICY_NAMESPACE)
        .into_iter()
        .map(|key| {
            governed_image::<MemoryLongTermGovernancePolicy>(
                LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let audits = batch_json_keys(batch, LONG_TERM_CONTROL_AUDIT_NAMESPACE)
        .into_iter()
        .map(|key| {
            governed_image::<LongTermMemoryControlAuditEvent>(
                LONG_TERM_CONTROL_AUDIT_NAMESPACE,
                &key,
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let operation = audits
        .iter()
        .find_map(|image| image.after.as_ref().map(|audit| audit.operation))
        .ok_or_else(|| {
            Error::config(
                "memory_write_transaction_control_post_image_invalid",
                "control transaction has no typed audit operation",
            )
        })?;
    let actor_subject_id = audits
        .iter()
        .find_map(|image| image.after.as_ref())
        .and_then(|audit| audit.actor_subject_id.clone());
    let mut owner_ids = BTreeSet::new();
    for image in &revisions {
        if let Some(value) = image.after.as_ref().or(image.before.as_ref()) {
            owner_ids.insert(value.transition.predecessor.owner_ref.owner_id.clone());
            if let Some(successor) = value.transition.successor.as_ref() {
                owner_ids.insert(successor.owner_ref.owner_id.clone());
            }
        }
    }
    for image in &tombstones {
        if let Some(value) = image.after.as_ref().or(image.before.as_ref()) {
            owner_ids.insert(value.record_id.clone());
        }
    }
    let owner_records = owner_ids
        .into_iter()
        .map(|owner_id| {
            governed_long_term_material_image(
                memory_space_id,
                memory_space_id,
                &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner_id),
                before,
                after,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    ensure_post_image_validation(
        "memory_write_transaction_control_post_image_invalid",
        validate_long_term_control_post_image(&LongTermControlPostImageClosure {
            transaction_id: batch.transaction_id.clone(),
            operation,
            memory_space_id: memory_space_id.to_string(),
            factual_owner_id: memory_space_id.to_string(),
            actor_subject_id,
            owner_records,
            revisions,
            tombstones,
            policies,
            audits,
        }),
    )
}

pub(crate) fn validate_control_document_for_scope(
    namespace: &str,
    physical_key: &str,
    value: &serde_json::Value,
    memory_space_id: &str,
    factual_owner_id: &str,
) -> Result<()> {
    let logical_key = match namespace {
        LONG_TERM_CONTROL_REVISION_NAMESPACE => {
            let revision = serde_json::from_value::<LongTermMemoryControlRevision>(value.clone())
                .map_err(|error| {
                Error::config(
                    "control_plane_scope_manifest",
                    format!("control revision decode failed: {error}"),
                )
            })?;
            if revision.memory_space_id != memory_space_id
                || revision.factual_owner_id != factual_owner_id
            {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "control revision owner scope differs from the manifest scope",
                ));
            }
            revision.revision_id
        }
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE => {
            let tombstone = serde_json::from_value::<LongTermMemoryTombstone>(value.clone())
                .map_err(|error| {
                    Error::config(
                        "control_plane_scope_manifest",
                        format!("control tombstone decode failed: {error}"),
                    )
                })?;
            tombstone.validate_contract().map_err(|error| {
                Error::config(
                    "control_plane_scope_manifest",
                    format!("control tombstone contract failed: {error}"),
                )
            })?;
            if tombstone.memory_space_id != memory_space_id
                || tombstone.factual_owner_id != factual_owner_id
            {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "control tombstone owner scope differs from the manifest scope",
                ));
            }
            tombstone.record_id
        }
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE => {
            let policy = serde_json::from_value::<MemoryLongTermGovernancePolicy>(value.clone())
                .map_err(|error| {
                    Error::config(
                        "control_plane_scope_manifest",
                        format!("governance policy decode failed: {error}"),
                    )
                })?;
            if policy.memory_space_id != memory_space_id
                || policy.selector.memory_space_id.as_deref() != Some(memory_space_id)
            {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "governance policy must declare the exact memory-space closure",
                ));
            }
            policy.policy_id
        }
        LONG_TERM_CONTROL_AUDIT_NAMESPACE => {
            let audit = serde_json::from_value::<LongTermMemoryControlAuditEvent>(value.clone())
                .map_err(|error| {
                    Error::config(
                        "control_plane_scope_manifest",
                        format!("control audit decode failed: {error}"),
                    )
                })?;
            if audit.memory_space_id.as_deref() != Some(memory_space_id)
                || audit.factual_owner_id != factual_owner_id
                || audit.effects.iter().any(|effect| match effect {
                    ControlEffectRef::Revision {
                        factual_owner_id: effect_owner_id,
                        ..
                    }
                    | ControlEffectRef::Tombstone {
                        factual_owner_id: effect_owner_id,
                        ..
                    }
                    | ControlEffectRef::Policy {
                        factual_owner_id: effect_owner_id,
                        ..
                    } => effect_owner_id != factual_owner_id,
                })
            {
                return Err(Error::config(
                    "control_plane_scope_manifest",
                    "control audit owner closure differs from the manifest scope",
                ));
            }
            audit.event_id
        }
        _ => {
            return Err(Error::config(
                "control_plane_scope_manifest",
                format!("unsupported control-plane namespace {namespace}"),
            ));
        }
    };
    let expected = scoped_long_term_control_storage_key(memory_space_id, namespace, &logical_key)?;
    if expected != physical_key {
        return Err(Error::config(
            "control_plane_scope_manifest",
            "control-plane document physical key is not canonical",
        ));
    }
    Ok(())
}

fn graph_transaction_scopes(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<Vec<(String, String, String)>> {
    let mut manifests = Vec::new();
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } if namespace == MEMORY_GRAPH_MANIFEST_NAMESPACE => {
                manifests.push((key.clone(), value.clone()));
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == MEMORY_GRAPH_MANIFEST_NAMESPACE =>
            {
                let value = preconditions
                    .iter()
                    .find_map(|precondition| match precondition {
                        StoreJsonPrecondition::Exact {
                            namespace,
                            key: precondition_key,
                            value,
                        } if namespace == MEMORY_GRAPH_MANIFEST_NAMESPACE
                            && precondition_key == key =>
                        {
                            Some(value.clone())
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        Error::config(
                            "memory_write_transaction_graph_manifest_closure_missing",
                            "graph manifest deletion requires its exact typed precondition",
                        )
                    })?;
                manifests.push((key.clone(), value));
            }
            _ => {}
        }
    }

    let mut scopes = BTreeMap::new();
    for (key, value) in manifests {
        let manifest =
            serde_json::from_value::<MemoryGraphScopeManifest>(value).map_err(|error| {
                Error::config(
                    "memory_write_transaction_graph_manifest_closure_missing",
                    format!("invalid typed graph manifest: {error}"),
                )
            })?;
        let memory_space_id = manifest.memory_space_id.trim();
        let mounted_subject_id = manifest.mounted_subject_id.trim();
        let expected_scope_digest = memory_graph_scope_digest(memory_space_id, mounted_subject_id);
        let expected_key = memory_graph_scope_manifest_key(memory_space_id, mounted_subject_id);
        if memory_space_id.is_empty()
            || mounted_subject_id.is_empty()
            || memory_space_id != manifest.memory_space_id
            || mounted_subject_id != manifest.mounted_subject_id
            || memory_space_id != batch.scope.memory_space_id
            || manifest.scope_digest != expected_scope_digest
            || key != expected_key
        {
            return Err(Error::config(
                "memory_write_transaction_graph_scope_mismatch",
                "typed graph manifest is not canonical or belongs to another MemorySpace",
            ));
        }
        let scope = (
            manifest.memory_space_id,
            manifest.mounted_subject_id,
            expected_scope_digest.clone(),
        );
        if scopes
            .insert(expected_scope_digest, scope.clone())
            .is_some_and(|existing| existing != scope)
        {
            return Err(Error::config(
                "memory_write_transaction_graph_scope_mismatch",
                "typed graph scope digest maps to conflicting scope identities",
            ));
        }
    }
    Ok(scopes.into_values().collect())
}

fn validate_graph_manifest_closure(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    const GRAPH_NAMESPACES: &[&str] = &[
        "memory_graph_nodes",
        "memory_graph_edges",
        "memory_graph_backlinks",
        "memory_graph_indexes",
        "memory_graph_revisions",
        "memory_graph_manifests",
        "memory_graph_node_memberships",
        "memory_graph_edge_memberships",
        "memory_graph_backlink_memberships",
    ];
    let graph_mutations = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. }
                if GRAPH_NAMESPACES.contains(&namespace.as_str()) =>
            {
                Some((namespace.as_str(), key.as_str()))
            }
            _ => None,
        });
    let graph_mutations = graph_mutations.collect::<Vec<_>>();
    let evidence_owner_mutated = batch.mutations.iter().any(|mutation| {
        matches!(
            mutation,
            StoreMutation::PutJson { namespace, .. }
                | StoreMutation::DeleteJson { namespace, .. }
                if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
        )
    });
    if graph_mutations.is_empty() && !evidence_owner_mutated {
        return Ok(());
    }
    if graph_mutations.is_empty() {
        return Err(Error::config(
            "memory_write_transaction_graph_manifest_closure_missing",
            "evidence owner mutations require an explicit same-transaction graph closure",
        ));
    }

    let graph_scopes = graph_transaction_scopes(batch, preconditions)?;
    if graph_scopes.is_empty() {
        return Err(Error::config(
            "memory_write_transaction_graph_manifest_closure_missing",
            "graph mutations require at least one typed graph scope manifest",
        ));
    }
    let expected_prefixes = graph_scopes
        .iter()
        .map(|(_, _, scope_digest)| format!("scope:{scope_digest}:doc:"))
        .collect::<Vec<_>>();
    for (namespace, key) in &graph_mutations {
        let Some(document_digest) = expected_prefixes
            .iter()
            .find_map(|prefix| key.strip_prefix(prefix))
        else {
            return Err(Error::config(
                "memory_write_transaction_graph_scope_mismatch",
                format!("graph mutation {namespace}/{key} is outside every typed graph scope"),
            ));
        };
        let Some(document_digest) = document_digest.strip_prefix("sha256:") else {
            return Err(Error::config(
                "memory_write_transaction_graph_physical_key_invalid",
                format!("graph mutation {namespace}/{key} has an invalid physical key"),
            ));
        };
        if document_digest.len() != 64
            || !document_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::config(
                "memory_write_transaction_graph_physical_key_invalid",
                format!("graph mutation {namespace}/{key} has an invalid physical key"),
            ));
        }
    }

    let expected_manifest_keys = graph_scopes
        .iter()
        .map(|(memory_space_id, mounted_subject_id, _)| {
            memory_graph_scope_manifest_key(memory_space_id, mounted_subject_id)
        })
        .collect::<BTreeSet<_>>();
    let manifest_keys = graph_mutations
        .iter()
        .filter_map(|(namespace, key)| {
            (*namespace == MEMORY_GRAPH_MANIFEST_NAMESPACE).then_some(*key)
        })
        .collect::<BTreeSet<_>>();
    if manifest_keys == expected_manifest_keys.iter().map(String::as_str).collect() {
        Ok(())
    } else {
        Err(Error::config(
            "memory_write_transaction_graph_manifest_closure_missing",
            "graph mutations require exactly one manifest mutation for every typed graph scope",
        ))
    }
}

fn validate_control_audit_closure(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    let control_mutated = batch.mutations.iter().any(|mutation| match mutation {
        StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. } => {
            matches!(
                namespace.as_str(),
                LONG_TERM_CONTROL_REVISION_NAMESPACE
                    | LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE
                    | LONG_TERM_GOVERNANCE_POLICY_NAMESPACE
            )
        }
        _ => false,
    });
    let audit_mutated = batch.mutations.iter().any(|mutation| match mutation {
        StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. } => {
            namespace == LONG_TERM_CONTROL_AUDIT_NAMESPACE
        }
        _ => false,
    });
    if control_mutated != audit_mutated {
        return Err(Error::config(
            "memory_write_transaction_control_audit_closure_missing",
            "control metadata and its audit event must mutate in the same transaction",
        ));
    }
    if !control_mutated {
        return Ok(());
    }

    let mut required_record_ids = BTreeSet::new();
    let mut required_policy_ids = BTreeSet::new();
    let mut audited_record_ids = BTreeSet::new();
    let mut audited_policy_ids = BTreeSet::new();

    for mutation in &batch.mutations {
        let (namespace, record_key) = match mutation {
            StoreMutation::PutJson {
                namespace,
                record_key,
                ..
            }
            | StoreMutation::DeleteJson {
                namespace,
                record_key,
                ..
            } => (namespace.as_str(), record_key.as_str()),
            _ => continue,
        };
        match namespace {
            LONG_TERM_CONTROL_REVISION_NAMESPACE => {
                let revision = serde_json::from_value::<LongTermMemoryControlRevision>(
                    control_mutation_value(mutation, preconditions)?,
                )
                .map_err(|error| {
                    Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        format!("invalid control revision: {error}"),
                    )
                })?;
                if revision.revision_id != record_key {
                    return Err(Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        "control revision record_key does not match revision_id",
                    ));
                }
                required_record_ids
                    .insert(revision.transition.predecessor.owner_ref.owner_id.clone());
            }
            LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE => {
                let tombstone = serde_json::from_value::<LongTermMemoryTombstone>(
                    control_mutation_value(mutation, preconditions)?,
                )
                .map_err(|error| {
                    Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        format!("invalid control tombstone: {error}"),
                    )
                })?;
                if tombstone.record_id != record_key {
                    return Err(Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        "control tombstone record_key does not match record_id",
                    ));
                }
                required_record_ids.insert(tombstone.record_id);
            }
            LONG_TERM_GOVERNANCE_POLICY_NAMESPACE => {
                let policy = serde_json::from_value::<MemoryLongTermGovernancePolicy>(
                    control_mutation_value(mutation, preconditions)?,
                )
                .map_err(|error| {
                    Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        format!("invalid governance policy: {error}"),
                    )
                })?;
                if policy.policy_id != record_key {
                    return Err(Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        "governance policy record_key does not match policy_id",
                    ));
                }
                required_policy_ids.insert(policy.policy_id);
            }
            LONG_TERM_CONTROL_AUDIT_NAMESPACE => {
                let StoreMutation::PutJson { value, .. } = mutation else {
                    return Err(Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        "control audit history is append-only",
                    ));
                };
                let audit =
                    serde_json::from_value::<LongTermMemoryControlAuditEvent>(value.clone())
                        .map_err(|error| {
                            Error::config(
                                "memory_write_transaction_control_audit_binding_invalid",
                                format!("invalid control audit event: {error}"),
                            )
                        })?;
                if audit.event_id != record_key {
                    return Err(Error::config(
                        "memory_write_transaction_control_audit_binding_invalid",
                        "control audit record_key does not match event_id",
                    ));
                }
                for effect in audit.effects {
                    match effect {
                        ControlEffectRef::Revision { transition, .. } => {
                            audited_record_ids.insert(transition.predecessor.owner_ref.owner_id);
                            if let Some(successor) = transition.successor {
                                audited_record_ids.insert(successor.owner_ref.owner_id);
                            }
                        }
                        ControlEffectRef::Tombstone { record_id, .. } => {
                            audited_record_ids.insert(record_id);
                        }
                        ControlEffectRef::Policy { policy_id, .. } => {
                            audited_policy_ids.insert(policy_id);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let missing_records = required_record_ids
        .difference(&audited_record_ids)
        .cloned()
        .collect::<Vec<_>>();
    let missing_policies = required_policy_ids
        .difference(&audited_policy_ids)
        .cloned()
        .collect::<Vec<_>>();
    if missing_records.is_empty() && missing_policies.is_empty() {
        return Ok(());
    }
    Err(Error::config(
        "memory_write_transaction_control_audit_binding_invalid",
        format!(
            "control audit does not cover records [{}] and policies [{}]",
            missing_records.join(","),
            missing_policies.join(",")
        ),
    ))
}

fn control_mutation_value(
    mutation: &StoreMutation,
    preconditions: &[StoreJsonPrecondition],
) -> Result<serde_json::Value> {
    match mutation {
        StoreMutation::PutJson { value, .. } => Ok(value.clone()),
        StoreMutation::DeleteJson { namespace, key, .. } => preconditions
            .iter()
            .find_map(|precondition| match precondition {
                StoreJsonPrecondition::Exact {
                    namespace: expected_namespace,
                    key: expected_key,
                    value,
                } if expected_namespace == namespace && expected_key == key => Some(value.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                Error::config(
                    "memory_write_transaction_control_audit_binding_invalid",
                    "control delete requires an exact precondition for audit binding",
                )
            }),
        _ => Err(Error::config(
            "memory_write_transaction_control_audit_binding_invalid",
            "control audit binding requires a JSON mutation",
        )),
    }
}

fn validate_batch_mutation_namespaces(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
    mut get_blob: impl FnMut(&str, &str) -> Result<Option<Vec<u8>>>,
) -> Result<()> {
    for precondition in preconditions {
        let (namespace, key) = match precondition {
            StoreJsonPrecondition::Absent { namespace, key }
            | StoreJsonPrecondition::Exact { namespace, key, .. } => (namespace, key),
        };
        ensure_batch_json_address(namespace, key)?;
    }
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } => {
                let decoder = ensure_batch_json_address(namespace, key)?;
                validate_typed_json_mutation(
                    decoder,
                    namespace,
                    key,
                    Some(value),
                    preconditions,
                    false,
                )?;
            }
            StoreMutation::DeleteJson { namespace, key, .. } => {
                let decoder = ensure_batch_json_address(namespace, key)?;
                validate_typed_json_mutation(decoder, namespace, key, None, preconditions, true)?;
            }
            StoreMutation::PutBlob {
                namespace,
                key,
                value,
                ..
            } => {
                ensure_batch_blob_address(namespace, key, Some(value))?;
            }
            StoreMutation::DeleteBlob { namespace, key, .. } => {
                let existing = get_blob(namespace, key)?;
                ensure_batch_blob_address(namespace, key, existing.as_deref())?;
            }
            StoreMutation::AppendEvent { event } => {
                event.validate_current_schema("memory_write_transaction_preflight_failed")?;
            }
        }
    }
    Ok(())
}

fn validate_typed_json_mutation(
    decoder: StoreJsonDecoderKind,
    namespace: &str,
    key: &str,
    put_value: Option<&serde_json::Value>,
    preconditions: &[StoreJsonPrecondition],
    deleting: bool,
) -> Result<()> {
    if !matches!(
        decoder,
        StoreJsonDecoderKind::LongTermVersionMaterial
            | StoreJsonDecoderKind::LongTermHeadManifest
            | StoreJsonDecoderKind::LongTermVersionScopeManifest
            | StoreJsonDecoderKind::LongTermControlTombstone
            | StoreJsonDecoderKind::RuntimeSkillOwnerRecord
            | StoreJsonDecoderKind::RuntimeSkillScopeManifest
            | StoreJsonDecoderKind::MemoryMutationReceipt
            | StoreJsonDecoderKind::MemoryMutationAudit
            | StoreJsonDecoderKind::SubjectSoulLifecycleHead
            | StoreJsonDecoderKind::SubjectSoulRevisionMaterial
            | StoreJsonDecoderKind::SubjectSoulScopeManifest
            | StoreJsonDecoderKind::SubjectSoulGenerationTombstone
            | StoreJsonDecoderKind::RelationshipSourceConstitution
            | StoreJsonDecoderKind::RelationshipSourceScopeManifest
            | StoreJsonDecoderKind::SubjectSoulRelationshipProjection
            | StoreJsonDecoderKind::SubjectSoulOperationResult
            | StoreJsonDecoderKind::RelationshipSourceOperationResult
    ) {
        return Ok(());
    }
    let precondition = preconditions
        .iter()
        .find(|precondition| match precondition {
            StoreJsonPrecondition::Absent {
                namespace: expected_namespace,
                key: expected_key,
            }
            | StoreJsonPrecondition::Exact {
                namespace: expected_namespace,
                key: expected_key,
                ..
            } => expected_namespace == namespace && expected_key == key,
        });
    let prior_value = match precondition {
        Some(StoreJsonPrecondition::Exact { value, .. }) => Some(value),
        _ => None,
    };
    if matches!(
        decoder,
        StoreJsonDecoderKind::MemoryMutationReceipt | StoreJsonDecoderKind::MemoryMutationAudit
    ) && (deleting || !matches!(precondition, Some(StoreJsonPrecondition::Absent { .. })))
    {
        return Err(Error::config(
            "memory_write_transaction_typed_precondition_invalid",
            "mutation receipt and audit records are append-only and require Absent preconditions",
        ));
    }
    if matches!(
        decoder,
        StoreJsonDecoderKind::SubjectSoulOperationResult
            | StoreJsonDecoderKind::RelationshipSourceOperationResult
    ) && (deleting || !matches!(precondition, Some(StoreJsonPrecondition::Absent { .. })))
    {
        return Err(Error::config(
            "memory_write_transaction_typed_precondition_invalid",
            "Subject Soul durable operation results are append-only and require Absent preconditions",
        ));
    }
    if deleting && prior_value.is_none() {
        return Err(Error::config(
            "memory_write_transaction_typed_precondition_missing",
            format!("typed delete requires Exact precondition for {namespace}:{key}"),
        ));
    }
    if !deleting && precondition.is_none() {
        return Err(Error::config(
            "memory_write_transaction_typed_precondition_missing",
            format!("typed put requires Absent or Exact precondition for {namespace}:{key}"),
        ));
    }
    if decoder == StoreJsonDecoderKind::LongTermVersionMaterial
        && put_value.is_some()
        && !matches!(precondition, Some(StoreJsonPrecondition::Absent { .. }))
    {
        return Err(Error::config(
            "memory_write_transaction_typed_precondition_invalid",
            "immutable long-term material put requires Absent precondition",
        ));
    }
    if matches!(
        decoder,
        StoreJsonDecoderKind::SubjectSoulRevisionMaterial
            | StoreJsonDecoderKind::SubjectSoulGenerationTombstone
            | StoreJsonDecoderKind::RelationshipSourceConstitution
    ) && put_value.is_some()
        && !matches!(precondition, Some(StoreJsonPrecondition::Absent { .. }))
    {
        return Err(Error::config(
            "memory_write_transaction_typed_precondition_invalid",
            "immutable Soul/relationship material put requires Absent precondition",
        ));
    }
    if let Some(value) = prior_value {
        admit_store_json_document(
            namespace,
            key,
            value,
            "memory_write_transaction_typed_document_invalid",
        )?;
    }
    if let Some(value) = put_value {
        admit_store_json_document(
            namespace,
            key,
            value,
            "memory_write_transaction_typed_document_invalid",
        )?;
    }
    Ok(())
}

fn validate_governed_owner_facet_closure(
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    let mut owner_refs = BTreeSet::new();
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace
                == crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE =>
            {
                let material =
                    serde_json::from_value::<LongTermMemoryVersionMaterial>(value.clone())
                        .map_err(|error| {
                            Error::config(
                                "memory_write_transaction_owner_facet_closure_invalid",
                                format!("invalid long-term material: {error}"),
                            )
                        })?;
                owner_refs.insert(material.owner_ref);
            }
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE => {
                let head = serde_json::from_value::<LongTermMemoryHeadManifest>(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "memory_write_transaction_owner_facet_closure_invalid",
                            format!("invalid long-term head: {error}"),
                        )
                    })?;
                owner_refs.insert(head.owner_ref);
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace
                    == crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE =>
            {
                let value = preconditions
                    .iter()
                    .find_map(|precondition| match precondition {
                        StoreJsonPrecondition::Exact {
                            namespace: expected_namespace,
                            key: expected_key,
                            value,
                        } if expected_namespace == namespace && expected_key == key => Some(value),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        Error::config(
                            "memory_write_transaction_owner_facet_closure_invalid",
                            "long-term head delete requires an exact typed precondition",
                        )
                    })?;
                let head = serde_json::from_value::<LongTermMemoryHeadManifest>(value.clone())
                    .map_err(|error| {
                        Error::config(
                            "memory_write_transaction_owner_facet_closure_invalid",
                            format!("invalid prior long-term head: {error}"),
                        )
                    })?;
                owner_refs.insert(head.owner_ref);
            }
            StoreMutation::PutJson {
                namespace,
                record_key,
                ..
            }
            | StoreMutation::DeleteJson {
                namespace,
                record_key,
                ..
            } if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE => {
                owner_refs.insert(GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    record_key.clone(),
                ));
            }
            _ => {}
        }
    }
    if owner_refs.is_empty() {
        return Ok(());
    }

    let facet_owner_refs = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == MEMORY_FACET_INDEX_NAMESPACE => {
                serde_json::from_value::<MemoryFacetIndexDoc>(value.clone())
                    .ok()
                    .map(|doc| doc.owner_ref)
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == MEMORY_FACET_INDEX_NAMESPACE =>
            {
                preconditions
                    .iter()
                    .find_map(|precondition| match precondition {
                        StoreJsonPrecondition::Exact {
                            namespace,
                            key: precondition_key,
                            value,
                        } if namespace == MEMORY_FACET_INDEX_NAMESPACE
                            && precondition_key == key =>
                        {
                            serde_json::from_value::<MemoryFacetIndexDoc>(value.clone())
                                .ok()
                                .map(|doc| doc.owner_ref)
                        }
                        _ => None,
                    })
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let missing = owner_refs
        .difference(&facet_owner_refs)
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(Error::config(
        "memory_write_transaction_owner_facet_closure_missing",
        format!(
            "governed owner mutations require same-transaction facet owner closure: {}",
            missing
                .iter()
                .map(|owner_ref| format!(
                    "{}:{}",
                    owner_ref.owner_plane.as_str(),
                    owner_ref.owner_id
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
    ))
}

fn ensure_batch_json_address(namespace: &str, key: &str) -> Result<StoreJsonDecoderKind> {
    admit_store_json_address(namespace, key, "memory_write_transaction_preflight_failed")
}

fn ensure_json_snapshot_namespace(namespace: &str, stage: &'static str) -> Result<()> {
    admit_store_json_address(namespace, "_namespace_probe", stage).map(|_| ())
}

fn ensure_batch_blob_address(
    namespace: &str,
    key: &str,
    value: Option<&[u8]>,
) -> Result<StoreBlobDecoderKind> {
    match classify_store_blob_address(namespace, key, value)? {
        StoreAddressAdmission::Active(kind) => Ok(kind),
        StoreAddressAdmission::ForbiddenLegacy(kind) => Err(Error::config(
            "memory_write_transaction_preflight_failed",
            format!("forbidden legacy blob address {namespace}:{key} ({kind:?})"),
        )),
        StoreAddressAdmission::Unknown => Err(Error::config(
            "memory_write_transaction_preflight_failed",
            format!("unsupported blob namespace {namespace}"),
        )),
    }
}

fn ensure_snapshot_blob_address(
    namespace: &str,
    key: &str,
    value: Option<&[u8]>,
) -> Result<StoreBlobDecoderKind> {
    match classify_store_blob_address(namespace, key, value)? {
        StoreAddressAdmission::Active(kind) => Ok(kind),
        StoreAddressAdmission::ForbiddenLegacy(kind) => Err(Error::config(
            "store_snapshot_import",
            format!("forbidden legacy blob address {namespace}:{key} ({kind:?})"),
        )),
        StoreAddressAdmission::Unknown => Err(Error::config(
            "store_snapshot_import",
            format!("unknown blob namespace {namespace}"),
        )),
    }
}

fn memory_write_transaction_preflight_error(error: Error) -> Error {
    if error.stage() == "memory_write_transaction_preflight_failed" {
        error
    } else {
        Error::config(
            "memory_write_transaction_preflight_failed",
            error.to_string(),
        )
    }
}

fn memory_write_transaction_preflight_error_for_authority(
    error: Error,
    subject_soul_authorized: bool,
) -> Error {
    if subject_soul_authorized
        && (error.stage().contains("budget") || error.stage().contains("capacity"))
    {
        Error::config("subject_soul_store_capacity", error.to_string())
    } else {
        memory_write_transaction_preflight_error(error)
    }
}

fn memory_write_transaction_commit_error(error: Error, subject_soul_authorized: bool) -> Error {
    if subject_soul_authorized
        && matches!(error.stage(), "store_budget_exceeded" | "store_event_log")
    {
        return Error::config("subject_soul_store_capacity", error.to_string());
    }
    match error.stage() {
        "store_budget_exceeded" | "store_event_log" | "store_snapshot_import" => {
            memory_write_transaction_preflight_error(error)
        }
        "memory_write_transaction_precondition_failed"
        | "memory_write_transaction_evidence_source_ref_post_image_invalid"
        | "store_transaction_busy"
        | "memory_write_transaction_repair_required" => error,
        _ => Error::config("memory_write_transaction_commit_failed", error.to_string()),
    }
}

pub(crate) fn snapshot_namespace_requires_private_export(namespace: &str) -> bool {
    matches!(
        namespace,
        "self_model"
            | "self_authored_core"
            | "core_revision_ledger"
            | "self_continuity"
            | "relationship_constitution"
            | "relationship_portfolio"
            | "relationship_topology"
            | "outer_voice"
            | "inner_life"
            | "felt_significance"
            | "temperament_continuity"
            | "inner_conflict"
            | "mental_privacy"
            | "private_doc"
            | "conversation_transcript"
            | "conversation_transcript_alias"
            | "conversation_transcript_attr"
            | "conversation_transcript_derived_ref"
            | crate::store_internal::transcript_query::TRANSCRIPT_CATALOG_PAGE_NAMESPACE
            | crate::store_internal::transcript_query::TRANSCRIPT_CATALOG_ROOT_NAMESPACE
            | crate::store_internal::transcript_query::TRANSCRIPT_TIME_POSTING_NAMESPACE
            | crate::store_internal::transcript_query::TRANSCRIPT_TIME_ROOT_NAMESPACE
            | crate::store_internal::transcript_query::TRANSCRIPT_SEARCH_POSTING_NAMESPACE
            | crate::store_internal::transcript_query::TRANSCRIPT_SEARCH_ROOT_NAMESPACE
            | crate::store_internal::transcript_query::TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE
            | crate::store_internal::transcript_query::TRANSCRIPT_QUERY_KEYRING_NAMESPACE
            | GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
            | GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE
            | GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE
            | "private_garden"
    )
}

pub(crate) fn snapshot_key_requires_private_export(key: &str) -> bool {
    key.contains("private_garden")
        || key.contains("private_doc")
        || key.contains("mental_privacy")
        || key.contains("inner_life")
        || key.contains("self_model")
        || key.contains("self_continuity")
        || key.contains("conversation_transcript")
        || key.contains("conversation_transcript_alias")
        || key.contains("conversation_transcript_derived_ref")
        || key.contains("conversation_transcript_query")
}

pub(crate) fn snapshot_json_requires_private_export(
    namespace: &str,
    value: &serde_json::Value,
) -> bool {
    snapshot_namespace_requires_private_export(namespace)
        || value
            .get("privacy")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|privacy| {
                matches!(
                    privacy,
                    "private_garden" | "soul_private" | "operator_diagnostic"
                )
            })
}

impl StoreEventLog for StorePlatform {
    #[cfg(feature = "nonproduction-replay-harness")]
    fn append_event(&self, event: MemoryStoreEvent) -> Result<()> {
        self.append_validated_event(event)
    }

    #[cfg(any(test, feature = "nonproduction-replay-harness"))]
    fn read_events(&self) -> Result<Vec<MemoryStoreEvent>> {
        self.engine.read_events()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeLifecycleStoreBinding<'a> {
    Direct,
    Transaction {
        operation: &'a str,
        transaction_id: &'a str,
    },
}

pub(crate) fn materialize_runtime_lifecycle_store_event(
    event: &RuntimeLifecycleEvent,
    binding: RuntimeLifecycleStoreBinding<'_>,
) -> Result<MemoryStoreEvent> {
    event.validate_for_recording()?;
    let event_value = serde_json::to_value(event)
        .map_err(|error| Error::config("runtime_lifecycle_event", error.to_string()))?;
    let content_hash = stable_hash_json(&event_value)?;
    let kind = match event.kind {
        RuntimeLifecycleEventKind::RuntimeLifecycle => MemoryStoreEventKind::RuntimeLifecycle,
        RuntimeLifecycleEventKind::OperatorAction => MemoryStoreEventKind::OperatorAction,
    };
    let operation = match binding {
        RuntimeLifecycleStoreBinding::Direct => event.operation.as_str(),
        RuntimeLifecycleStoreBinding::Transaction { operation, .. } => operation,
    };
    let mut store_event = MemoryStoreEvent::new(
        event.event_id.clone(),
        kind,
        StoreEventScope::system(event.operation.as_str()),
        event.timestamp_unix_secs,
    )
    .with_plane("runtime_lifecycle")
    .with_record_key(event.operation.as_str())
    .with_content_hash(content_hash)
    .with_payload("runtime_operation", event.operation.as_str())
    .with_payload("operation", operation)
    .with_payload("trigger", event.trigger.as_str())
    .with_payload("disposition", event.disposition.as_str())
    .with_payload("effect", event.effect.as_str())
    .with_payload("profile", event.profile.as_str())
    .with_payload("mode", event.mode.as_str())
    .with_payload(
        "pressure",
        format!("{:?}", event.pressure).to_ascii_lowercase(),
    )
    .with_payload("reason", event.reason.clone())
    .with_payload("success", event.success().to_string())
    .with_payload("result", event.result())
    .with_payload("result_summary", event.result_summary())
    .with_payload("error_stage", event.error_stage.clone().unwrap_or_default());
    if let RuntimeLifecycleStoreBinding::Transaction { transaction_id, .. } = binding {
        store_event = store_event.with_payload("transaction_id", transaction_id);
    }
    for (key, value) in event.payload() {
        store_event = store_event.with_payload(key.clone(), value.clone());
    }
    Ok(store_event)
}

impl RuntimeLifecycleEventSink for StorePlatform {
    fn record_lifecycle_event(&self, event: RuntimeLifecycleEvent) -> Result<()> {
        self.append_validated_event(materialize_runtime_lifecycle_store_event(
            &event,
            RuntimeLifecycleStoreBinding::Direct,
        )?)
    }
}

impl Platform for StorePlatform {
    fn memory_system_kind(&self) -> MemorySystemKind {
        self.config.memory_system_kind
    }

    fn runtime_lifecycle_event_sink(&self) -> Arc<dyn bm_core::runtime::RuntimeLifecycleEventSink> {
        Arc::new(self.clone())
    }

    fn state_fs(&self) -> Arc<dyn StateFs> {
        Arc::new(self.clone())
    }

    fn skill_storage(&self) -> Arc<dyn SkillStorage> {
        Arc::new(self.clone())
    }

    fn skill_meta_store(&self) -> Arc<dyn SkillMetaStore> {
        Arc::new(self.clone())
    }

    fn active_work_store(&self) -> Arc<dyn ActiveWorkStore> {
        Arc::new(self.clone())
    }

    fn memory_store(&self) -> Arc<dyn MemoryStore> {
        Arc::new(self.clone())
    }

    fn session_store(&self) -> Arc<dyn SessionStore> {
        Arc::new(self.clone())
    }

    fn conversation_transcript_store(&self) -> Arc<dyn ConversationTranscriptStore> {
        Arc::new(self.clone())
    }

    fn session_summary_store(&self) -> Arc<dyn SessionSummaryStore> {
        Arc::new(self.clone())
    }

    fn long_term_memory_extraction_state_store(
        &self,
    ) -> Arc<dyn LongTermMemoryExtractionStateStore> {
        Arc::new(self.clone())
    }

    fn continuity_capsule_store(&self) -> Arc<dyn ContinuityCapsuleStore> {
        Arc::new(self.clone())
    }

    fn turn_ledger_store(&self) -> Arc<dyn TurnLedgerStore> {
        Arc::new(self.clone())
    }

    fn self_model_store(&self) -> Arc<dyn SelfModelStore> {
        Arc::new(self.clone())
    }

    fn self_authored_core_store(&self) -> Arc<dyn SelfAuthoredCoreStore> {
        Arc::new(self.clone())
    }

    fn core_revision_ledger_store(&self) -> Arc<dyn CoreRevisionLedgerStore> {
        Arc::new(self.clone())
    }

    fn self_continuity_store(&self) -> Arc<dyn SelfContinuityStore> {
        Arc::new(self.clone())
    }

    fn relationship_constitution_store(&self) -> Arc<dyn RelationshipConstitutionStore> {
        Arc::new(self.clone())
    }

    fn relationship_portfolio_store(&self) -> Arc<dyn RelationshipPortfolioStore> {
        Arc::new(self.clone())
    }

    fn relationship_topology_store(&self) -> Arc<dyn RelationshipTopologyStore> {
        Arc::new(self.clone())
    }

    fn execution_state_store(&self) -> Arc<dyn ExecutionStateStore> {
        Arc::new(self.clone())
    }

    fn world_sense_store(&self) -> Arc<dyn WorldSenseStore> {
        Arc::new(self.clone())
    }

    fn outer_voice_store(&self) -> Arc<dyn OuterVoiceStore> {
        Arc::new(self.clone())
    }

    fn autonomy_strategy_store(&self) -> Arc<dyn AutonomyStrategyStore> {
        Arc::new(self.clone())
    }

    fn inner_life_store(&self) -> Arc<dyn InnerLifeStore> {
        Arc::new(self.clone())
    }

    fn felt_significance_store(&self) -> Arc<dyn FeltSignificanceStore> {
        Arc::new(self.clone())
    }

    fn temperament_continuity_store(&self) -> Arc<dyn TemperamentContinuityStore> {
        Arc::new(self.clone())
    }

    fn inner_conflict_store(&self) -> Arc<dyn InnerConflictStore> {
        Arc::new(self.clone())
    }

    fn mental_privacy_store(&self) -> Arc<dyn MentalPrivacyStore> {
        Arc::new(self.clone())
    }

    fn private_doc_store(&self) -> Arc<dyn PrivateDocStore> {
        Arc::new(self.clone())
    }

    fn private_garden_store(&self) -> Arc<dyn PrivateGardenStore> {
        Arc::new(self.clone())
    }

    fn turn_continuity_evidence_store(&self) -> Arc<dyn TurnContinuityEvidenceStore> {
        Arc::new(self.clone())
    }

    fn remind_at_store(&self) -> Arc<dyn RemindAtStore> {
        Arc::new(self.clone())
    }

    fn task_store(&self) -> Arc<dyn TaskStore> {
        Arc::new(self.clone())
    }

    fn task_run_store(&self) -> Arc<dyn TaskRunStore> {
        Arc::new(self.clone())
    }

    fn task_artifact_store(&self) -> Arc<dyn TaskArtifactStore> {
        Arc::new(self.clone())
    }

    fn task_learning_store(&self) -> Arc<dyn TaskLearningStore> {
        Arc::new(self.clone())
    }
}

impl StateFs for StorePlatform {
    fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
        self.engine.get_blob("state_fs", rel_path)
    }

    fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
        self.blob_put("state_fs", rel_path, data)
    }

    fn remove(&self, rel_path: &str) -> Result<()> {
        self.blob_delete("state_fs", rel_path).map(|_| ())
    }

    fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
        let prefix = rel_path.trim_end_matches('/');
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        let mut out = self
            .engine
            .list_blob_keys("state_fs")?
            .into_iter()
            .filter_map(|key| key.strip_prefix(&prefix).map(ToString::to_string))
            .collect::<Vec<_>>();
        out.sort();
        Ok(out)
    }
}

impl SkillStorage for StorePlatform {
    fn list_names(&self) -> Result<Vec<String>> {
        self.engine.list_blob_keys("skills")
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        Ok(self.engine.get_blob("skills", name)?.unwrap_or_default())
    }

    fn write(&self, name: &str, content: &[u8]) -> Result<()> {
        self.commit_recall_indexed_mutations(
            "skill.write",
            self.recall_scope(),
            vec![StoreMutation::PutBlob {
                namespace: "skills".to_string(),
                key: name.to_string(),
                value: content.to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skills".to_string(),
                record_key: name.to_string(),
            }],
            Vec::new(),
        )?;
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<()> {
        if self.engine.get_blob("skills", name)?.is_none() {
            return Ok(());
        }
        self.commit_recall_indexed_mutations(
            "skill.delete",
            self.recall_scope(),
            vec![StoreMutation::DeleteBlob {
                namespace: "skills".to_string(),
                key: name.to_string(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "skills".to_string(),
                record_key: name.to_string(),
            }],
            Vec::new(),
        )?;
        Ok(())
    }
}

impl SkillMetaStore for StorePlatform {
    fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
        let order = self
            .json_get::<Vec<String>>("skill_meta", "order")?
            .unwrap_or_default();
        let disabled = self
            .json_get::<Vec<String>>("skill_meta", "disabled")?
            .unwrap_or_default();
        Ok((order, disabled))
    }

    fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()> {
        self.json_put("skill_meta", "order", &order.to_vec())?;
        self.json_put("skill_meta", "disabled", &disabled.to_vec())
    }
}

macro_rules! impl_keyed_json_store {
    ($trait_name:path, $value_ty:path, $namespace:literal) => {
        impl $trait_name for StorePlatform {
            fn get(&self, key: &str) -> Result<Option<$value_ty>> {
                self.json_get($namespace, key)
            }

            fn set(&self, key: &str, value: &$value_ty) -> Result<()> {
                self.json_put($namespace, key, value)
            }

            fn clear(&self, key: &str) -> Result<()> {
                self.json_delete($namespace, key).map(|_| ())
            }
        }
    };
}

impl_keyed_json_store!(ExecutionStateStore, ExecutionState, "execution_state");
impl_keyed_json_store!(
    LongTermMemoryExtractionStateStore,
    LongTermMemoryExtractionState,
    "long_term_extraction_state"
);
impl_keyed_json_store!(SelfModelStore, SelfModel, "self_model");
impl_keyed_json_store!(
    SelfAuthoredCoreStore,
    SelfAuthoredCore,
    "self_authored_core"
);
impl_keyed_json_store!(
    CoreRevisionLedgerStore,
    CoreRevisionLedger,
    "core_revision_ledger"
);
impl_keyed_json_store!(SelfContinuityStore, SelfContinuity, "self_continuity");
impl_keyed_json_store!(
    RelationshipConstitutionStore,
    RelationshipConstitution,
    "relationship_constitution"
);
impl_keyed_json_store!(
    RelationshipPortfolioStore,
    RelationshipPortfolio,
    "relationship_portfolio"
);
impl_keyed_json_store!(
    RelationshipTopologyStore,
    RelationshipTopology,
    "relationship_topology"
);
impl_keyed_json_store!(WorldSenseStore, WorldSense, "world_sense");
impl_keyed_json_store!(OuterVoiceStore, OuterVoice, "outer_voice");
impl_keyed_json_store!(AutonomyStrategyStore, AutonomyStrategy, "autonomy_strategy");
impl_keyed_json_store!(InnerLifeStore, InnerLife, "inner_life");
impl_keyed_json_store!(FeltSignificanceStore, FeltSignificance, "felt_significance");
impl_keyed_json_store!(
    TemperamentContinuityStore,
    TemperamentContinuity,
    "temperament_continuity"
);
impl_keyed_json_store!(InnerConflictStore, InnerConflict, "inner_conflict");
impl_keyed_json_store!(MentalPrivacyStore, MentalPrivacyState, "mental_privacy");
impl_keyed_json_store!(PrivateDocStore, PrivateDocWorkspace, "private_doc");

impl ActiveWorkStore for StorePlatform {
    fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
        self.json_get("active_work", chat_id)
    }

    fn set(&self, chat_id: &str, record: &ActiveWorkRecord) -> Result<()> {
        self.put_archive_json_owner("archive.active_work.write", "active_work", chat_id, record)
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.delete_archive_json_owner("archive.active_work.delete", "active_work", chat_id)
    }
}

impl TurnLedgerStore for StorePlatform {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        self.json_get("turn_ledger", chat_id)
    }

    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
        self.put_archive_json_owner("archive.turn_ledger.write", "turn_ledger", chat_id, ledger)
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.delete_archive_json_owner("archive.turn_ledger.delete", "turn_ledger", chat_id)
    }
}

impl SessionSummaryStore for StorePlatform {
    fn get(&self, chat_id: &str) -> Result<Option<String>> {
        Ok(self
            .json_get::<SessionSummaryRecord>("session_summary", chat_id)?
            .map(|record| record.summary))
    }

    fn set(&self, chat_id: &str, summary: &str) -> Result<()> {
        let count = self
            .json_get::<SessionSummaryRecord>("session_summary", chat_id)?
            .map(|record| record.message_count)
            .unwrap_or(0);
        self.put_archive_json_owner(
            "archive.session_summary.write",
            "session_summary",
            chat_id,
            &SessionSummaryRecord {
                summary: summary.to_string(),
                message_count: count,
            },
        )
    }

    fn set_with_count(&self, chat_id: &str, summary: &str, message_count: usize) -> Result<()> {
        self.put_archive_json_owner(
            "archive.session_summary.write",
            "session_summary",
            chat_id,
            &SessionSummaryRecord {
                summary: summary.to_string(),
                message_count,
            },
        )
    }

    fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
        Ok(self
            .json_get::<SessionSummaryRecord>("session_summary", chat_id)?
            .map(|record| (record.summary, record.message_count)))
    }
}

impl MemoryStore for StorePlatform {
    fn get_memory(&self) -> Result<String> {
        Ok(self
            .engine
            .get_blob("memory", "MEMORY.md")?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default())
    }

    fn set_memory(&self, content: &str) -> Result<()> {
        self.put_archive_blob_owner(
            "archive.memory.write",
            "memory",
            "MEMORY.md",
            content.as_bytes(),
        )
    }

    fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
        let mut names = self.engine.list_blob_keys("daily")?;
        names.sort_by(|a, b| b.cmp(a));
        names.truncate(recent_n);
        Ok(names)
    }

    fn get_daily_note(&self, name: &str) -> Result<String> {
        Ok(self
            .engine
            .get_blob("daily", name)?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default())
    }

    fn write_daily_note(&self, name: &str, content: &str) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_daily_write")?;
        let manifest_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&manifest_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::blob(
            "daily",
            name,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Blob,
                "daily",
                name,
            ),
            current_unix_secs(),
            content.as_bytes(),
        )?;
        let index = self.plan_archive_index_upsert(address)?;
        self.commit_recall_indexed_mutations(
            "archive.daily.write",
            self.recall_scope(),
            vec![StoreMutation::PutBlob {
                namespace: "daily".to_string(),
                key: name.to_string(),
                value: content.as_bytes().to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "daily".to_string(),
                record_key: name.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }
}

impl SessionStore for StorePlatform {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_session_append")?;
        if role.trim().is_empty() {
            return Err(Error::config("session_append", "role must not be empty"));
        }
        if content.len() > MAX_SESSION_MESSAGE_LEN {
            return Err(Error::config(
                "session_append",
                format!(
                    "content length {} exceeds {}",
                    content.len(),
                    MAX_SESSION_MESSAGE_LEN
                ),
            ));
        }
        let mut messages = self
            .json_get::<Vec<SessionMessage>>("session", chat_id)?
            .unwrap_or_default();
        let now_secs = current_unix_secs();
        let (speaker_id, speaker_kind) = default_session_speaker_for_role(role);
        let message_id = stable_hash_id(
            "msg",
            &(
                chat_id,
                role,
                content,
                messages.len(),
                now_secs,
                current_unix_nanos(),
            ),
        );
        messages.push(SessionMessage::new(
            message_id,
            role,
            content,
            now_secs,
            now_secs,
            speaker_id,
            speaker_kind,
        ));
        if messages.len() > MAX_SESSION_ENTRIES {
            let remove_count = messages.len() - MAX_SESSION_ENTRIES;
            messages.drain(0..remove_count);
        }
        let value = serde_json::to_value(&messages)
            .map_err(|error| Error::config("archive_recall_session", error.to_string()))?;
        let manifest_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&manifest_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "session",
            chat_id,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                "session",
                chat_id,
            ),
            now_secs,
            &value,
        )?;
        let index = self.plan_archive_index_upsert(address)?;
        self.commit_recall_indexed_mutations(
            "archive.session.append",
            self.recall_scope(),
            vec![StoreMutation::PutJson {
                namespace: "session".to_string(),
                key: chat_id.to_string(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "session".to_string(),
                record_key: chat_id.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn append_batch(&self, chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
        let _transaction_guard = self.lock_transaction("archive_recall_session_append_batch")?;
        let mut persisted = self
            .json_get::<Vec<SessionMessage>>("session", chat_id)?
            .unwrap_or_default();
        for message in messages {
            if message.message_id.trim().is_empty() {
                return Err(Error::config(
                    "session_append_batch",
                    "message_id must not be empty",
                ));
            }
            if message.role.trim().is_empty() {
                return Err(Error::config(
                    "session_append_batch",
                    "role must not be empty",
                ));
            }
            if message.content.len() > MAX_SESSION_MESSAGE_LEN {
                return Err(Error::config(
                    "session_append_batch",
                    format!(
                        "content length {} exceeds {}",
                        message.content.len(),
                        MAX_SESSION_MESSAGE_LEN
                    ),
                ));
            }
            persisted.push(message.clone());
        }
        if persisted.len() > MAX_SESSION_ENTRIES {
            let remove_count = persisted.len() - MAX_SESSION_ENTRIES;
            persisted.drain(0..remove_count);
        }
        let value = serde_json::to_value(&persisted)
            .map_err(|error| Error::config("archive_recall_session", error.to_string()))?;
        let manifest_key = ArchiveRecallManifest::build(
            1,
            &self.config.event_scope.memory_space_id,
            &self.config.event_scope.subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&manifest_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "session",
            chat_id,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                "session",
                chat_id,
            ),
            current_unix_secs(),
            &value,
        )?;
        let index = self.plan_archive_index_upsert(address)?;
        self.commit_recall_indexed_mutations(
            "archive.session.append_batch",
            self.recall_scope(),
            vec![StoreMutation::PutJson {
                namespace: "session".to_string(),
                key: chat_id.to_string(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "session".to_string(),
                record_key: chat_id.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let messages = self
            .json_get::<Vec<SessionMessage>>("session", chat_id)?
            .unwrap_or_default();
        Ok(tail(messages, n))
    }

    fn message_count(&self, chat_id: &str) -> Result<usize> {
        Ok(self
            .json_get::<Vec<SessionMessage>>("session", chat_id)?
            .map(|messages| messages.len())
            .unwrap_or(0))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_session_clear")?;
        if self.engine.get_json_value("session", chat_id)?.is_none() {
            return Ok(());
        }
        let index =
            self.plan_archive_index_remove(RecallIndexAddressKind::Json, "session", chat_id)?;
        self.commit_recall_indexed_mutations(
            "archive.session.clear",
            self.recall_scope(),
            vec![StoreMutation::DeleteJson {
                namespace: "session".to_string(),
                key: chat_id.to_string(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "session".to_string(),
                record_key: chat_id.to_string(),
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        self.engine.list_json_keys("session")
    }
}

impl StorePlatform {
    fn load_catalog_head_exact(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
    ) -> Result<Option<ConversationCatalogHead>> {
        let (_, root, _, pages) = self.load_catalog_pages(
            &key.memory_space_id,
            mounted_subject_id,
            Some(&key.channel_id),
        )?;
        if root.is_none() {
            return Ok(None);
        }
        Ok(pages
            .into_iter()
            .flat_map(|(_, page, _)| page.heads)
            .find(|head| head.key == *key))
    }

    fn load_query_keyring(&self, memory_space_id: &str) -> Result<TranscriptQueryKeyringV1> {
        let key = keyring_key(memory_space_id);
        let value = self
            .engine
            .get_json_value(TRANSCRIPT_QUERY_KEYRING_NAMESPACE, &key)?
            .ok_or_else(|| {
                Error::config(
                    "conversation_transcript_query_cursor",
                    "query keyring is missing",
                )
            })?;
        let keyring =
            serde_json::from_value::<TranscriptQueryKeyringV1>(value).map_err(|error| {
                Error::config("conversation_transcript_query_cursor", error.to_string())
            })?;
        keyring.validate_for_memory_space(memory_space_id)?;
        Ok(keyring)
    }

    fn fresh_query_keyring(memory_space_id: &str, now: u64) -> Result<TranscriptQueryKeyringV1> {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|error| {
            Error::config(
                "conversation_transcript_query_cursor_entropy",
                error.to_string(),
            )
        })?;
        let digest = secret
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(TranscriptQueryKeyringV1 {
            schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
            memory_space_id: memory_space_id.to_string(),
            incarnation: format!(
                "sha256:{:x}",
                Sha256::digest(format!("incarnation:{digest}").as_bytes())
            ),
            current: crate::store_internal::transcript_query::TranscriptQuerySigningKeyV1 {
                key_id: format!(
                    "sha256:{:x}",
                    Sha256::digest(format!("key-id:{digest}").as_bytes())
                ),
                key_hex: digest,
                created_at: now,
                expires_at: now.saturating_add(7_776_000),
            },
            previous: None,
        })
    }

    fn plan_query_keyring(&self, memory_space_id: &str) -> Result<Option<RecallIndexMutationPlan>> {
        let key = keyring_key(memory_space_id);
        let before = self
            .engine
            .get_json_value(TRANSCRIPT_QUERY_KEYRING_NAMESPACE, &key)?;
        let now = current_unix_secs();
        let previous = before
            .as_ref()
            .map(|value| serde_json::from_value::<TranscriptQueryKeyringV1>(value.clone()))
            .transpose()
            .map_err(|error| {
                Error::config("conversation_transcript_query_cursor", error.to_string())
            })?;
        if let Some(existing) = previous.as_ref() {
            existing.validate_for_memory_space(memory_space_id)?;
            if existing.current.expires_at > now.saturating_add(604_800) {
                if existing
                    .previous
                    .as_ref()
                    .is_some_and(|key| key.expires_at <= now)
                {
                    let mut pruned = existing.clone();
                    pruned.previous = None;
                    return Ok(Some((
                        TRANSCRIPT_QUERY_KEYRING_NAMESPACE,
                        key,
                        serde_json::to_value(pruned).map_err(|error| {
                            Error::config("conversation_transcript_query_cursor", error.to_string())
                        })?,
                        before,
                    )));
                }
                return Ok(None);
            }
        }
        let mut keyring = Self::fresh_query_keyring(memory_space_id, now)?;
        if let Some(previous) = previous {
            keyring.incarnation = previous.incarnation;
            keyring.previous = Some(previous.current).filter(|key| key.expires_at > now);
        }
        Ok(Some((
            TRANSCRIPT_QUERY_KEYRING_NAMESPACE,
            key,
            serde_json::to_value(keyring).map_err(|error| {
                Error::config("conversation_transcript_query_cursor", error.to_string())
            })?,
            before,
        )))
    }

    fn ensure_query_keyring(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel_id: Option<&str>,
    ) -> Result<TranscriptQueryKeyringV1> {
        let _transaction_guard = self.lock_transaction("conversation_transcript_query_keyring")?;
        if let Some(plan) = self.plan_query_keyring(memory_space_id)? {
            let mut scope = self.recall_scope();
            scope.memory_space_id = memory_space_id.to_string();
            scope.subject_id = mounted_subject_id.to_string();
            if let Some(channel_id) = channel_id {
                scope.channel = channel_id.to_string();
            }
            self.commit_recall_indexed_mutations(
                "conversation.transcript.query_keyring.rotate",
                scope,
                Vec::new(),
                vec![plan],
            )?;
        }
        self.load_query_keyring(memory_space_id)
    }

    fn validate_transcript_query_record_closure(
        &self,
        record: &TranscriptTurnRecord,
        owner_head: &ConversationRecallManifest,
    ) -> Result<()> {
        let catalog_head = self
            .load_catalog_head_exact(&record.key, &record.subject)?
            .ok_or_else(|| {
                Error::config(
                    "conversation_transcript_query_repair_required",
                    "transcript owner is missing its catalog head",
                )
            })?;
        if catalog_head.revision != owner_head.revision
            || catalog_head.head_digest != owner_head.head_digest
            || catalog_head.content_generation != owner_head.revision
            || catalog_head.index_generation != owner_head.revision
        {
            return Err(Error::config(
                "conversation_transcript_query_repair_required",
                "transcript catalog head identity differs from its owner head",
            ));
        }
        self.load_query_keyring(&record.key.memory_space_id)?;
        for (locator, content) in message_locators(record) {
            let manifest_key = search_message_manifest_key(&locator);
            let manifest_value = self
                .engine
                .get_json_value(TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE, &manifest_key)?
                .ok_or_else(|| {
                    Error::config(
                        "conversation_transcript_query_repair_required",
                        "transcript message is missing its search manifest",
                    )
                })?;
            let manifest =
                serde_json::from_value::<TranscriptMessageSearchManifestV1>(manifest_value)
                    .map_err(|error| {
                        Error::config(
                            "conversation_transcript_query_repair_required",
                            error.to_string(),
                        )
                    })?;
            let expected_terms = TranscriptSearchNormalizerV1::index_terms(
                content,
                MAX_TRANSCRIPT_INDEX_TERMS_PER_MESSAGE,
            )?
            .iter()
            .map(|term| term_digest(term))
            .collect::<Vec<_>>();
            if manifest.locator != locator
                || manifest.term_set_digest != term_set_digest(&expected_terms)
            {
                return Err(Error::config(
                    "conversation_transcript_query_repair_required",
                    "transcript message search manifest differs from canonical content",
                ));
            }
            let day = locator.observed_at / 86_400;
            let time_root_key = time_root_key(&record.key, &record.subject, day);
            let time_root = self
                .engine
                .get_json_value(TRANSCRIPT_TIME_ROOT_NAMESPACE, &time_root_key)?
                .map(serde_json::from_value::<TranscriptTimePostingRootV1>)
                .transpose()
                .map_err(|error| {
                    Error::config(
                        "conversation_transcript_query_repair_required",
                        error.to_string(),
                    )
                })?
                .ok_or_else(|| {
                    Error::config(
                        "conversation_transcript_query_repair_required",
                        "transcript message is missing its time root",
                    )
                })?;
            let mut time_bound = false;
            for page_id in 0..time_root.page_count {
                let page_key = time_posting_key(&record.key, &record.subject, day, page_id);
                let page = self
                    .engine
                    .get_json_value(TRANSCRIPT_TIME_POSTING_NAMESPACE, &page_key)?
                    .map(serde_json::from_value::<TranscriptTimePostingPageV1>)
                    .transpose()
                    .map_err(|error| {
                        Error::config(
                            "conversation_transcript_query_repair_required",
                            error.to_string(),
                        )
                    })?
                    .ok_or_else(|| {
                        Error::config(
                            "conversation_transcript_query_repair_required",
                            "time root references a missing page",
                        )
                    })?;
                time_bound |= page.locators.iter().any(|entry| entry.locator == locator);
            }
            if !time_bound {
                return Err(Error::config(
                    "conversation_transcript_query_repair_required",
                    "transcript message is absent from its time posting",
                ));
            }
            for digest in &expected_terms {
                let root_key =
                    search_root_key(&record.key.memory_space_id, &record.subject, digest);
                let root = self
                    .engine
                    .get_json_value(TRANSCRIPT_SEARCH_ROOT_NAMESPACE, &root_key)?
                    .map(serde_json::from_value::<TranscriptSearchPostingRootV1>)
                    .transpose()
                    .map_err(|error| {
                        Error::config(
                            "conversation_transcript_query_repair_required",
                            error.to_string(),
                        )
                    })?
                    .ok_or_else(|| {
                        Error::config(
                            "conversation_transcript_query_repair_required",
                            "transcript message search term root is missing",
                        )
                    })?;
                let mut search_bound = false;
                for page_id in 0..root.page_count {
                    let page_key = search_posting_key(
                        &record.key.memory_space_id,
                        &record.subject,
                        digest,
                        page_id,
                    );
                    let page = self
                        .engine
                        .get_json_value(TRANSCRIPT_SEARCH_POSTING_NAMESPACE, &page_key)?
                        .map(serde_json::from_value::<TranscriptSearchPostingPageV1>)
                        .transpose()
                        .map_err(|error| {
                            Error::config(
                                "conversation_transcript_query_repair_required",
                                error.to_string(),
                            )
                        })?
                        .ok_or_else(|| {
                            Error::config(
                                "conversation_transcript_query_repair_required",
                                "search root references a missing page",
                            )
                        })?;
                    search_bound |= page.locators.iter().any(|entry| entry.locator == locator);
                }
                if !search_bound {
                    return Err(Error::config(
                        "conversation_transcript_query_repair_required",
                        "transcript message is absent from its search posting",
                    ));
                }
            }
        }
        Ok(())
    }

    fn transcript_catalog_head_after_append(
        previous: Option<&ConversationCatalogHead>,
        manifest: &ConversationRecallManifest,
        record: &TranscriptTurnRecord,
    ) -> Result<ConversationCatalogHead> {
        if let Some(previous) = previous {
            if previous.revision.saturating_add(1) != manifest.revision
                || previous.head_digest == manifest.head_digest
                || previous.key != record.key
                || previous.mounted_subject_id != record.subject
            {
                return Err(Error::config(
                    "conversation_transcript_catalog_index",
                    "catalog head is not the exact predecessor of the transcript head",
                ));
            }
        }
        let mut lifecycle = previous.map(|head| head.lifecycle).unwrap_or_default();
        let stats = match record.lifecycle_state {
            TranscriptLifecycleState::Active => &mut lifecycle.active,
            TranscriptLifecycleState::Archived => &mut lifecycle.archived,
            TranscriptLifecycleState::Masked => &mut lifecycle.masked,
            TranscriptLifecycleState::RawDeleted => &mut lifecycle.raw_deleted,
        };
        let messages = record
            .input_messages
            .iter()
            .chain(record.assistant_message.iter())
            .collect::<Vec<_>>();
        stats.turn_count = stats.turn_count.saturating_add(1);
        stats.message_count = stats
            .message_count
            .saturating_add(u64::try_from(messages.len()).unwrap_or(u64::MAX));
        for message in messages {
            stats.first_observed_at = Some(
                stats
                    .first_observed_at
                    .map(|value| value.min(message.observed_at))
                    .unwrap_or(message.observed_at),
            );
            stats.last_observed_at = Some(
                stats
                    .last_observed_at
                    .map(|value| value.max(message.observed_at))
                    .unwrap_or(message.observed_at),
            );
        }
        let head = ConversationCatalogHead {
            key: record.key.clone(),
            mounted_subject_id: record.subject.clone(),
            revision: manifest.revision,
            head_digest: manifest.head_digest.clone(),
            turn_count: manifest.turn_count,
            message_count: previous
                .map(|head| head.message_count)
                .unwrap_or(0)
                .saturating_add(
                    u64::try_from(
                        record.input_messages.len()
                            + usize::from(record.assistant_message.is_some()),
                    )
                    .unwrap_or(u64::MAX),
                ),
            lifecycle,
            first_sequence: previous
                .and_then(|head| head.first_sequence)
                .or(Some(record.sequence)),
            last_sequence: Some(record.sequence),
            // Append extends the immutable transcript tail but does not invalidate a
            // cursor whose authenticated snapshot upper bound precedes the append.
            // Lifecycle/repair rebuilds intentionally advance both generations.
            content_generation: previous.map(|head| head.content_generation).unwrap_or(1),
            index_generation: previous.map(|head| head.index_generation).unwrap_or(1),
            updated_at: record.updated_at,
        };
        head.validate()?;
        Ok(head)
    }

    fn transcript_catalog_head_from_records(
        manifest: &ConversationRecallManifest,
        records: &[TranscriptTurnRecord],
    ) -> Result<ConversationCatalogHead> {
        if records.len() != usize::try_from(manifest.turn_count).unwrap_or(usize::MAX) {
            return Err(Error::config(
                "conversation_transcript_catalog_index",
                "head and transcript page closure differ",
            ));
        }
        let mut message_count = 0u64;
        let mut lifecycle = TranscriptLifecycleAggregate::default();
        for record in records {
            let count = u64::try_from(
                record.input_messages.len() + usize::from(record.assistant_message.is_some()),
            )
            .unwrap_or(u64::MAX);
            message_count = message_count.saturating_add(count);
            let stats = match record.lifecycle_state {
                TranscriptLifecycleState::Active => &mut lifecycle.active,
                TranscriptLifecycleState::Archived => &mut lifecycle.archived,
                TranscriptLifecycleState::Masked => &mut lifecycle.masked,
                TranscriptLifecycleState::RawDeleted => &mut lifecycle.raw_deleted,
            };
            stats.turn_count = stats.turn_count.saturating_add(1);
            stats.message_count = stats.message_count.saturating_add(count);
            for message in record
                .input_messages
                .iter()
                .chain(record.assistant_message.iter())
            {
                stats.first_observed_at = Some(
                    stats
                        .first_observed_at
                        .map(|value| value.min(message.observed_at))
                        .unwrap_or(message.observed_at),
                );
                stats.last_observed_at = Some(
                    stats
                        .last_observed_at
                        .map(|value| value.max(message.observed_at))
                        .unwrap_or(message.observed_at),
                );
            }
        }
        let head = ConversationCatalogHead {
            key: ConversationKey::new(
                manifest.memory_space_id.clone(),
                manifest.channel_id.clone(),
                manifest.conversation_id.clone(),
            )?,
            mounted_subject_id: manifest.mounted_subject_id.clone(),
            revision: manifest.revision,
            head_digest: manifest.head_digest.clone(),
            turn_count: manifest.turn_count,
            message_count,
            lifecycle,
            first_sequence: records.first().map(|record| record.sequence),
            last_sequence: records.last().map(|record| record.sequence),
            content_generation: manifest.revision,
            index_generation: manifest.revision,
            updated_at: records
                .iter()
                .map(|record| record.updated_at)
                .max()
                .unwrap_or(1),
        };
        head.validate()?;
        Ok(head)
    }

    fn load_catalog_pages(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel_id: Option<&str>,
    ) -> Result<TranscriptCatalogLoad> {
        let root_key = catalog_root_key(memory_space_id, mounted_subject_id, channel_id);
        let root_before = self
            .engine
            .get_json_value(TRANSCRIPT_CATALOG_ROOT_NAMESPACE, &root_key)?;
        let root = root_before
            .clone()
            .map(serde_json::from_value::<TranscriptCatalogRootV1>)
            .transpose()
            .map_err(|error| {
                Error::config("conversation_transcript_catalog_index", error.to_string())
            })?;
        let mut pages = Vec::new();
        if let Some(root) = root.as_ref() {
            for page_id in 0..root.page_count {
                let key =
                    catalog_page_key(memory_space_id, mounted_subject_id, channel_id, page_id);
                let value = self
                    .engine
                    .get_json_value(TRANSCRIPT_CATALOG_PAGE_NAMESPACE, &key)?
                    .ok_or_else(|| {
                        Error::config(
                            "conversation_transcript_catalog_index",
                            "catalog root references a missing page",
                        )
                    })?;
                let page = serde_json::from_value::<TranscriptCatalogPageV1>(value.clone())
                    .map_err(|error| {
                        Error::config("conversation_transcript_catalog_index", error.to_string())
                    })?;
                page.validate()?;
                if page.page_id != page_id {
                    return Err(Error::config(
                        "conversation_transcript_catalog_index",
                        "catalog page ordinal differs from root",
                    ));
                }
                pages.push((key, page, value));
            }
        }
        Ok((root_key, root, root_before, pages))
    }

    fn plan_catalog_page_upsert(
        &self,
        head: &ConversationCatalogHead,
        channel_id: Option<&str>,
    ) -> Result<Vec<RecallIndexMutationPlan>> {
        let (root_key, previous_root, root_before, previous_pages) = self.load_catalog_pages(
            &head.key.memory_space_id,
            &head.mounted_subject_id,
            channel_id,
        )?;
        let mut heads = previous_pages
            .iter()
            .flat_map(|(_, page, _)| page.heads.clone())
            .collect::<Vec<_>>();
        heads.retain(|candidate| candidate.key != head.key);
        heads.push(head.clone());
        heads.sort_by(|left, right| {
            (right.updated_at, &right.key.conversation_id)
                .cmp(&(left.updated_at, &left.key.conversation_id))
        });
        let revision = previous_root
            .as_ref()
            .map(|root| root.revision.saturating_add(1))
            .unwrap_or(1);
        let page_count =
            u64::try_from(heads.len().div_ceil(TRANSCRIPT_QUERY_PAGE_CAPACITY)).unwrap_or(u64::MAX);
        let root = TranscriptCatalogRootV1 {
            schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
            memory_space_id: head.key.memory_space_id.clone(),
            mounted_subject_id: head.mounted_subject_id.clone(),
            channel_id: channel_id.map(str::to_string),
            revision,
            page_count,
            entry_count: u64::try_from(heads.len()).unwrap_or(u64::MAX),
        };
        let mut plans = vec![(
            TRANSCRIPT_CATALOG_ROOT_NAMESPACE,
            root_key,
            serde_json::to_value(root).map_err(|error| {
                Error::config("conversation_transcript_catalog_index", error.to_string())
            })?,
            root_before,
        )];
        for (page_id, chunk) in heads.chunks(TRANSCRIPT_QUERY_PAGE_CAPACITY).enumerate() {
            let page_id = u64::try_from(page_id).unwrap_or(u64::MAX);
            let page = TranscriptCatalogPageV1 {
                schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                memory_space_id: head.key.memory_space_id.clone(),
                mounted_subject_id: head.mounted_subject_id.clone(),
                channel_id: channel_id.map(str::to_string),
                page_id,
                revision,
                heads: chunk.to_vec(),
            };
            page.validate()?;
            let key = catalog_page_key(
                &head.key.memory_space_id,
                &head.mounted_subject_id,
                channel_id,
                page_id,
            );
            let before = previous_pages
                .iter()
                .find(|(candidate, _, _)| candidate == &key)
                .map(|(_, _, value)| value.clone());
            plans.push((
                TRANSCRIPT_CATALOG_PAGE_NAMESPACE,
                key,
                serde_json::to_value(page).map_err(|error| {
                    Error::config("conversation_transcript_catalog_index", error.to_string())
                })?,
                before,
            ));
        }
        Ok(plans)
    }

    fn plan_transcript_query_indexes(
        &self,
        record: &TranscriptTurnRecord,
        head: &ConversationCatalogHead,
    ) -> Result<Vec<RecallIndexMutationPlan>> {
        let mut plans = self.plan_catalog_page_upsert(head, None)?;
        plans.extend(self.plan_catalog_page_upsert(head, Some(&record.key.channel_id))?);
        if let Some(keyring) = self.plan_query_keyring(&record.key.memory_space_id)? {
            plans.push(keyring);
        }
        let mut by_day = BTreeMap::<u64, Vec<TranscriptPostingLocatorV1>>::new();
        let mut by_term = BTreeMap::<String, Vec<TranscriptPostingLocatorV1>>::new();
        let mut manifests = Vec::<(TranscriptLocator, Vec<String>)>::new();
        for (locator, content) in message_locators(record) {
            let posting_locator = TranscriptPostingLocatorV1 {
                locator: locator.clone(),
                lifecycle_state: record.lifecycle_state,
                redaction_state: record.redaction_state,
            };
            let utc_day = locator.observed_at / 86_400;
            by_day
                .entry(utc_day)
                .or_default()
                .push(posting_locator.clone());
            let mut term_digests = TranscriptSearchNormalizerV1::index_terms(
                content,
                MAX_TRANSCRIPT_INDEX_TERMS_PER_MESSAGE,
            )?
            .iter()
            .map(|term| term_digest(term))
            .collect::<Vec<_>>();
            term_digests.sort();
            term_digests.dedup();
            for digest in &term_digests {
                by_term
                    .entry(digest.clone())
                    .or_default()
                    .push(posting_locator.clone());
            }
            manifests.push((locator, term_digests));
        }
        for (utc_day, additions) in by_day {
            plans.extend(self.plan_time_posting_upsert(record, utc_day, additions)?);
        }
        for (digest, additions) in by_term {
            plans.extend(self.plan_search_posting_upsert(record, &digest, additions)?);
        }
        for (locator, term_digests) in manifests {
            let manifest_key = search_message_manifest_key(&locator);
            let manifest_before = self
                .engine
                .get_json_value(TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE, &manifest_key)?;
            plans.push((
                TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE,
                manifest_key,
                serde_json::to_value(TranscriptMessageSearchManifestV1 {
                    locator,
                    term_set_digest: term_set_digest(&term_digests),
                })
                .map_err(|error| {
                    Error::config("conversation_transcript_search_manifest", error.to_string())
                })?,
                manifest_before,
            ));
        }
        Ok(plans)
    }

    fn plan_time_posting_upsert(
        &self,
        record: &TranscriptTurnRecord,
        utc_day: u64,
        additions: Vec<TranscriptPostingLocatorV1>,
    ) -> Result<Vec<RecallIndexMutationPlan>> {
        let root_key = time_root_key(&record.key, &record.subject, utc_day);
        let root_before = self
            .engine
            .get_json_value(TRANSCRIPT_TIME_ROOT_NAMESPACE, &root_key)?;
        let previous_root = root_before
            .clone()
            .map(serde_json::from_value::<TranscriptTimePostingRootV1>)
            .transpose()
            .map_err(|error| {
                Error::config("conversation_transcript_time_index", error.to_string())
            })?;
        if let Some(root) = previous_root.as_ref() {
            root.validate()?;
            let last_page_id = root.page_count.saturating_sub(1);
            let last_key = time_posting_key(&record.key, &record.subject, utc_day, last_page_id);
            let last_before = self
                .engine
                .get_json_value(TRANSCRIPT_TIME_POSTING_NAMESPACE, &last_key)?
                .ok_or_else(|| {
                    Error::config(
                        "conversation_transcript_time_index",
                        "time root references a missing tail page",
                    )
                })?;
            let last_page =
                serde_json::from_value::<TranscriptTimePostingPageV1>(last_before.clone())
                    .map_err(|error| {
                        Error::config("conversation_transcript_time_index", error.to_string())
                    })?;
            last_page.validate_for_root(root)?;
            let order = |entry: &TranscriptPostingLocatorV1| {
                (
                    entry.locator.observed_at,
                    entry.locator.turn_sequence,
                    entry.locator.message_id.clone(),
                )
            };
            let mut additions = additions.clone();
            additions.sort_by_key(&order);
            if last_page.locators.last().is_some_and(|last| {
                additions
                    .first()
                    .is_some_and(|first| order(first) > order(last))
            }) {
                let revision = root.revision.saturating_add(1);
                let mut tail = last_page.locators.clone();
                tail.extend(additions);
                let added_pages = tail.len().div_ceil(TRANSCRIPT_QUERY_PAGE_CAPACITY);
                let page_count =
                    last_page_id.saturating_add(u64::try_from(added_pages).unwrap_or(u64::MAX));
                let next_root = TranscriptTimePostingRootV1 {
                    schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                    key: record.key.clone(),
                    mounted_subject_id: record.subject.clone(),
                    utc_day,
                    revision,
                    page_count,
                    entry_count: root.entry_count.saturating_add(
                        u64::try_from(tail.len() - last_page.locators.len()).unwrap_or(u64::MAX),
                    ),
                };
                let mut plans = vec![(
                    TRANSCRIPT_TIME_ROOT_NAMESPACE,
                    root_key,
                    serde_json::to_value(next_root).map_err(|error| {
                        Error::config("conversation_transcript_time_index", error.to_string())
                    })?,
                    root_before,
                )];
                for (offset, chunk) in tail.chunks(TRANSCRIPT_QUERY_PAGE_CAPACITY).enumerate() {
                    let page_id =
                        last_page_id.saturating_add(u64::try_from(offset).unwrap_or(u64::MAX));
                    let key = time_posting_key(&record.key, &record.subject, utc_day, page_id);
                    plans.push((
                        TRANSCRIPT_TIME_POSTING_NAMESPACE,
                        key,
                        serde_json::to_value(TranscriptTimePostingPageV1 {
                            schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                            key: record.key.clone(),
                            mounted_subject_id: record.subject.clone(),
                            utc_day,
                            page_id,
                            revision: if page_id == last_page_id {
                                last_page.revision.saturating_add(1)
                            } else {
                                1
                            },
                            locators: chunk.to_vec(),
                        })
                        .map_err(|error| {
                            Error::config("conversation_transcript_time_index", error.to_string())
                        })?,
                        (page_id == last_page_id).then(|| last_before.clone()),
                    ));
                }
                return Ok(plans);
            }
        }
        let mut previous_pages = Vec::new();
        let mut locators = Vec::new();
        for page_id in 0..previous_root
            .as_ref()
            .map(|root| root.page_count)
            .unwrap_or(0)
        {
            let key = time_posting_key(&record.key, &record.subject, utc_day, page_id);
            let value = self
                .engine
                .get_json_value(TRANSCRIPT_TIME_POSTING_NAMESPACE, &key)?
                .ok_or_else(|| {
                    Error::config(
                        "conversation_transcript_time_index",
                        "time root references a missing page",
                    )
                })?;
            let page = serde_json::from_value::<TranscriptTimePostingPageV1>(value.clone())
                .map_err(|error| {
                    Error::config("conversation_transcript_time_index", error.to_string())
                })?;
            locators.extend(page.locators);
            previous_pages.push((key, value));
        }
        for addition in additions {
            locators.retain(|candidate| candidate.locator != addition.locator);
            locators.push(addition);
        }
        locators.sort_by_key(|candidate| {
            (
                candidate.locator.observed_at,
                candidate.locator.turn_sequence,
                candidate.locator.message_id.clone(),
            )
        });
        let revision = previous_root
            .as_ref()
            .map(|root| root.revision.saturating_add(1))
            .unwrap_or(1);
        let page_count = u64::try_from(locators.len().div_ceil(TRANSCRIPT_QUERY_PAGE_CAPACITY))
            .unwrap_or(u64::MAX);
        let mut plans = vec![(
            TRANSCRIPT_TIME_ROOT_NAMESPACE,
            root_key,
            serde_json::to_value(TranscriptTimePostingRootV1 {
                schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                key: record.key.clone(),
                mounted_subject_id: record.subject.clone(),
                utc_day,
                revision,
                page_count,
                entry_count: u64::try_from(locators.len()).unwrap_or(u64::MAX),
            })
            .map_err(|error| {
                Error::config("conversation_transcript_time_index", error.to_string())
            })?,
            root_before,
        )];
        for (page_id, chunk) in locators.chunks(TRANSCRIPT_QUERY_PAGE_CAPACITY).enumerate() {
            let page_id = u64::try_from(page_id).unwrap_or(u64::MAX);
            let key = time_posting_key(&record.key, &record.subject, utc_day, page_id);
            plans.push((
                TRANSCRIPT_TIME_POSTING_NAMESPACE,
                key.clone(),
                serde_json::to_value(TranscriptTimePostingPageV1 {
                    schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                    key: record.key.clone(),
                    mounted_subject_id: record.subject.clone(),
                    utc_day,
                    page_id,
                    revision,
                    locators: chunk.to_vec(),
                })
                .map_err(|error| {
                    Error::config("conversation_transcript_time_index", error.to_string())
                })?,
                previous_pages
                    .iter()
                    .find(|(candidate, _)| candidate == &key)
                    .map(|(_, value)| value.clone()),
            ));
        }
        Ok(plans)
    }

    fn plan_search_posting_upsert(
        &self,
        record: &TranscriptTurnRecord,
        digest: &str,
        additions: Vec<TranscriptPostingLocatorV1>,
    ) -> Result<Vec<RecallIndexMutationPlan>> {
        let root_key = search_root_key(&record.key.memory_space_id, &record.subject, digest);
        let root_before = self
            .engine
            .get_json_value(TRANSCRIPT_SEARCH_ROOT_NAMESPACE, &root_key)?;
        let previous_root = root_before
            .clone()
            .map(serde_json::from_value::<TranscriptSearchPostingRootV1>)
            .transpose()
            .map_err(|error| {
                Error::config("conversation_transcript_search_index", error.to_string())
            })?;
        if let Some(root) = previous_root.as_ref() {
            root.validate()?;
            let last_page_id = root.page_count.saturating_sub(1);
            let last_key = search_posting_key(
                &record.key.memory_space_id,
                &record.subject,
                digest,
                last_page_id,
            );
            let last_before = self
                .engine
                .get_json_value(TRANSCRIPT_SEARCH_POSTING_NAMESPACE, &last_key)?
                .ok_or_else(|| {
                    Error::config(
                        "conversation_transcript_search_index",
                        "search root references a missing tail page",
                    )
                })?;
            let last_page =
                serde_json::from_value::<TranscriptSearchPostingPageV1>(last_before.clone())
                    .map_err(|error| {
                        Error::config("conversation_transcript_search_index", error.to_string())
                    })?;
            last_page.validate_for_root(root)?;
            let order = |entry: &TranscriptPostingLocatorV1| {
                (
                    entry.locator.observed_at,
                    entry.locator.turn_sequence,
                    entry.locator.message_id.clone(),
                )
            };
            let mut additions = additions.clone();
            additions.sort_by_key(&order);
            if last_page.locators.last().is_some_and(|last| {
                additions
                    .first()
                    .is_some_and(|first| order(first) > order(last))
            }) {
                let revision = root.revision.saturating_add(1);
                let mut tail = last_page.locators.clone();
                tail.extend(additions);
                let added_pages = tail.len().div_ceil(TRANSCRIPT_QUERY_PAGE_CAPACITY);
                let page_count =
                    last_page_id.saturating_add(u64::try_from(added_pages).unwrap_or(u64::MAX));
                let next_root = TranscriptSearchPostingRootV1 {
                    schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                    memory_space_id: record.key.memory_space_id.clone(),
                    mounted_subject_id: record.subject.clone(),
                    term_digest: digest.to_string(),
                    revision,
                    page_count,
                    entry_count: root.entry_count.saturating_add(
                        u64::try_from(tail.len() - last_page.locators.len()).unwrap_or(u64::MAX),
                    ),
                };
                let mut plans = vec![(
                    TRANSCRIPT_SEARCH_ROOT_NAMESPACE,
                    root_key,
                    serde_json::to_value(next_root).map_err(|error| {
                        Error::config("conversation_transcript_search_index", error.to_string())
                    })?,
                    root_before,
                )];
                for (offset, chunk) in tail.chunks(TRANSCRIPT_QUERY_PAGE_CAPACITY).enumerate() {
                    let page_id =
                        last_page_id.saturating_add(u64::try_from(offset).unwrap_or(u64::MAX));
                    let key = search_posting_key(
                        &record.key.memory_space_id,
                        &record.subject,
                        digest,
                        page_id,
                    );
                    plans.push((
                        TRANSCRIPT_SEARCH_POSTING_NAMESPACE,
                        key,
                        serde_json::to_value(TranscriptSearchPostingPageV1 {
                            schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                            memory_space_id: record.key.memory_space_id.clone(),
                            mounted_subject_id: record.subject.clone(),
                            term_digest: digest.to_string(),
                            page_id,
                            revision: if page_id == last_page_id {
                                last_page.revision.saturating_add(1)
                            } else {
                                1
                            },
                            locators: chunk.to_vec(),
                        })
                        .map_err(|error| {
                            Error::config("conversation_transcript_search_index", error.to_string())
                        })?,
                        (page_id == last_page_id).then(|| last_before.clone()),
                    ));
                }
                return Ok(plans);
            }
        }
        let mut previous_pages = Vec::new();
        let mut locators = Vec::new();
        for page_id in 0..previous_root
            .as_ref()
            .map(|root| root.page_count)
            .unwrap_or(0)
        {
            let key = search_posting_key(
                &record.key.memory_space_id,
                &record.subject,
                digest,
                page_id,
            );
            let value = self
                .engine
                .get_json_value(TRANSCRIPT_SEARCH_POSTING_NAMESPACE, &key)?
                .ok_or_else(|| {
                    Error::config(
                        "conversation_transcript_search_index",
                        "search root references a missing page",
                    )
                })?;
            let page = serde_json::from_value::<TranscriptSearchPostingPageV1>(value.clone())
                .map_err(|error| {
                    Error::config("conversation_transcript_search_index", error.to_string())
                })?;
            locators.extend(page.locators);
            previous_pages.push((key, value));
        }
        for addition in additions {
            locators.retain(|candidate| candidate.locator != addition.locator);
            locators.push(addition);
        }
        locators.sort_by_key(|candidate| {
            (
                candidate.locator.observed_at,
                candidate.locator.turn_sequence,
                candidate.locator.message_id.clone(),
            )
        });
        let revision = previous_root
            .as_ref()
            .map(|root| root.revision.saturating_add(1))
            .unwrap_or(1);
        let page_count = u64::try_from(locators.len().div_ceil(TRANSCRIPT_QUERY_PAGE_CAPACITY))
            .unwrap_or(u64::MAX);
        let mut plans = vec![(
            TRANSCRIPT_SEARCH_ROOT_NAMESPACE,
            root_key,
            serde_json::to_value(TranscriptSearchPostingRootV1 {
                schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                memory_space_id: record.key.memory_space_id.clone(),
                mounted_subject_id: record.subject.clone(),
                term_digest: digest.to_string(),
                revision,
                page_count,
                entry_count: u64::try_from(locators.len()).unwrap_or(u64::MAX),
            })
            .map_err(|error| {
                Error::config("conversation_transcript_search_index", error.to_string())
            })?,
            root_before,
        )];
        for (page_id, chunk) in locators.chunks(TRANSCRIPT_QUERY_PAGE_CAPACITY).enumerate() {
            let page_id = u64::try_from(page_id).unwrap_or(u64::MAX);
            let key = search_posting_key(
                &record.key.memory_space_id,
                &record.subject,
                digest,
                page_id,
            );
            plans.push((
                TRANSCRIPT_SEARCH_POSTING_NAMESPACE,
                key.clone(),
                serde_json::to_value(TranscriptSearchPostingPageV1 {
                    schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                    memory_space_id: record.key.memory_space_id.clone(),
                    mounted_subject_id: record.subject.clone(),
                    term_digest: digest.to_string(),
                    page_id,
                    revision,
                    locators: chunk.to_vec(),
                })
                .map_err(|error| {
                    Error::config("conversation_transcript_search_index", error.to_string())
                })?,
                previous_pages
                    .iter()
                    .find(|(candidate, _)| candidate == &key)
                    .map(|(_, value)| value.clone()),
            ));
        }
        Ok(plans)
    }

    fn plan_transcript_query_lifecycle_indexes(
        &self,
        changed: &[(TranscriptTurnRecord, TranscriptTurnRecord)],
        head: &ConversationCatalogHead,
    ) -> Result<Vec<RecallIndexMutationPlan>> {
        let mut plans = self.plan_catalog_page_upsert(head, None)?;
        plans.extend(self.plan_catalog_page_upsert(head, Some(&head.key.channel_id))?);
        let mut by_day = BTreeMap::<u64, Vec<TranscriptPostingLocatorV1>>::new();
        let mut by_term = BTreeMap::<String, Vec<TranscriptPostingLocatorV1>>::new();
        let mut owner = None::<TranscriptTurnRecord>;
        for (before_record, after_record) in changed {
            owner.get_or_insert_with(|| after_record.clone());
            for (locator, content) in message_locators(before_record) {
                let replacement = TranscriptPostingLocatorV1 {
                    locator: locator.clone(),
                    lifecycle_state: after_record.lifecycle_state,
                    redaction_state: after_record.redaction_state,
                };
                let utc_day = locator.observed_at / 86_400;
                by_day.entry(utc_day).or_default().push(replacement.clone());

                let manifest_key = search_message_manifest_key(&locator);
                let manifest_value = self
                    .engine
                    .get_json_value(TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE, &manifest_key)?
                    .ok_or_else(|| {
                        Error::config(
                            "conversation_transcript_search_manifest",
                            "lifecycle target search manifest is missing",
                        )
                    })?;
                let manifest =
                    serde_json::from_value::<TranscriptMessageSearchManifestV1>(manifest_value)
                        .map_err(|error| {
                            Error::config(
                                "conversation_transcript_search_manifest",
                                error.to_string(),
                            )
                        })?;
                let mut term_digests = TranscriptSearchNormalizerV1::index_terms(
                    content,
                    MAX_TRANSCRIPT_INDEX_TERMS_PER_MESSAGE,
                )?
                .iter()
                .map(|term| term_digest(term))
                .collect::<Vec<_>>();
                term_digests.sort();
                term_digests.dedup();
                if manifest.locator != locator
                    || manifest.term_set_digest != term_set_digest(&term_digests)
                {
                    return Err(Error::config(
                        "conversation_transcript_search_manifest",
                        "lifecycle target search manifest differs from canonical content",
                    ));
                }
                for digest in term_digests {
                    by_term.entry(digest).or_default().push(replacement.clone());
                }
            }
        }
        if let Some(owner) = owner.as_ref() {
            for (utc_day, additions) in by_day {
                plans.extend(self.plan_time_posting_upsert(owner, utc_day, additions)?);
            }
            for (digest, additions) in by_term {
                plans.extend(self.plan_search_posting_upsert(owner, &digest, additions)?);
            }
        }
        Ok(plans)
    }

    fn load_visible_timeline_backward(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        manifest: &ConversationRecallManifest,
        end_sequence: u64,
        limit: usize,
    ) -> Result<Vec<TranscriptTurnRecord>> {
        if end_sequence == 0 || limit == 0 {
            return Ok(Vec::new());
        }
        let mut page_id =
            Self::conversation_page_for_sequence(end_sequence.min(manifest.last_sequence))?;
        let mut output = Vec::new();
        loop {
            let mut page = self.load_validated_conversation_page_records(
                key,
                mounted_subject_id,
                manifest,
                page_id,
            )?;
            page.reverse();
            for record in page {
                if record.sequence <= end_sequence && record.is_searchable_for_presentation() {
                    output.push(record);
                    if output.len() == limit {
                        output.reverse();
                        return Ok(output);
                    }
                }
            }
            if page_id == 0 {
                break;
            }
            page_id -= 1;
        }
        output.reverse();
        Ok(output)
    }

    fn load_visible_timeline_forward(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        manifest: &ConversationRecallManifest,
        start_sequence: u64,
        limit: usize,
    ) -> Result<Vec<TranscriptTurnRecord>> {
        if start_sequence == 0 || start_sequence > manifest.last_sequence || limit == 0 {
            return Ok(Vec::new());
        }
        let first_page = Self::conversation_page_for_sequence(start_sequence)?;
        let mut output = Vec::new();
        for page_id in first_page..manifest.page_count {
            for record in self.load_validated_conversation_page_records(
                key,
                mounted_subject_id,
                manifest,
                page_id,
            )? {
                if record.sequence >= start_sequence && record.is_searchable_for_presentation() {
                    output.push(record);
                    if output.len() == limit {
                        return Ok(output);
                    }
                }
            }
        }
        Ok(output)
    }

    fn has_visible_timeline_before(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        manifest: &ConversationRecallManifest,
        sequence: u64,
    ) -> Result<bool> {
        if sequence <= 1 {
            return Ok(false);
        }
        self.load_visible_timeline_backward(
            key,
            mounted_subject_id,
            manifest,
            sequence.saturating_sub(1),
            1,
        )
        .map(|turns| !turns.is_empty())
    }

    fn has_visible_timeline_after_until(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        manifest: &ConversationRecallManifest,
        sequence: u64,
        snapshot_upper_bound: u64,
    ) -> Result<bool> {
        if sequence >= snapshot_upper_bound {
            return Ok(false);
        }
        let first_page = Self::conversation_page_for_sequence(sequence.saturating_add(1))?;
        let last_page = Self::conversation_page_for_sequence(snapshot_upper_bound)?;
        for page_id in first_page..=last_page.min(manifest.page_count.saturating_sub(1)) {
            if self
                .load_validated_conversation_page_records(
                    key,
                    mounted_subject_id,
                    manifest,
                    page_id,
                )?
                .into_iter()
                .any(|record| {
                    record.sequence > sequence
                        && record.sequence <= snapshot_upper_bound
                        && record.is_searchable_for_presentation()
                })
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn first_visible_sequence_in_range(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        range: &TranscriptUtcRange,
    ) -> Result<Option<u64>> {
        let mut first = None::<u64>;
        for day in range.start_inclusive / 86_400..=range.end_exclusive.saturating_sub(1) / 86_400 {
            let root_key = time_root_key(key, mounted_subject_id, day);
            let Some(root_value) = self
                .engine
                .get_json_value(TRANSCRIPT_TIME_ROOT_NAMESPACE, &root_key)?
            else {
                continue;
            };
            let root = serde_json::from_value::<TranscriptTimePostingRootV1>(root_value).map_err(
                |error| Error::config("conversation_transcript_time_index", error.to_string()),
            )?;
            for page_id in 0..root.page_count {
                let page_key = time_posting_key(key, mounted_subject_id, day, page_id);
                let value = self
                    .engine
                    .get_json_value(TRANSCRIPT_TIME_POSTING_NAMESPACE, &page_key)?
                    .ok_or_else(|| {
                        Error::config(
                            "conversation_transcript_time_index",
                            "time root references a missing page",
                        )
                    })?;
                let page = serde_json::from_value::<TranscriptTimePostingPageV1>(value).map_err(
                    |error| Error::config("conversation_transcript_time_index", error.to_string()),
                )?;
                for locator in page.locators.into_iter().filter(|locator| {
                    locator.visible(true) && range.contains(locator.locator.observed_at)
                }) {
                    first = Some(
                        first
                            .map(|value| value.min(locator.locator.turn_sequence))
                            .unwrap_or(locator.locator.turn_sequence),
                    );
                }
            }
        }
        Ok(first)
    }
}

impl ConversationTranscriptStore for StorePlatform {
    fn append_turn_intent(
        &self,
        intent: &TranscriptAppendIntent,
    ) -> Result<TranscriptCommitReport> {
        intent.validate()?;
        let record = &intent.record;
        let _transaction_guard = self.lock_transaction("conversation_recall_manifest_append")?;
        let key = transcript_turn_storage_key(&record.key, &record.subject, &record.turn_id);
        let (_, head, head_before) =
            self.load_conversation_recall_manifest(&record.key, &record.subject)?;
        if let Some(head) = head.as_ref() {
            self.validate_conversation_manifest_subject(head, &record.subject)?;
        }
        let before_count = head
            .as_ref()
            .map(|head| usize::try_from(head.turn_count).unwrap_or(usize::MAX))
            .unwrap_or(0);
        if let Some(value) = self
            .engine
            .get_json_value("conversation_transcript", &key)?
        {
            let existing =
                serde_json::from_value::<TranscriptTurnRecord>(value.clone()).map_err(|error| {
                    Error::config("conversation_transcript_page", error.to_string())
                })?;
            let head = head.as_ref().ok_or_else(|| {
                Error::config(
                    "conversation_transcript_head",
                    "transcript owner exists without its required head",
                )
            })?;
            let page_id = Self::conversation_page_for_sequence(existing.sequence)?;
            let records = self.load_validated_conversation_page_records(
                &record.key,
                &record.subject,
                head,
                page_id,
            )?;
            if !records.iter().any(|candidate| candidate == &existing) {
                return Err(Error::config(
                    "conversation_transcript_page",
                    "transcript owner exists without its required page binding",
                ));
            }
            let mut requested = record.clone();
            if requested.sequence == 0 {
                requested.sequence = existing.sequence;
            }
            if requested != existing {
                return Err(Error::config(
                    "conversation_transcript_append",
                    "turn id already exists with divergent payload",
                ));
            }
            self.validate_transcript_query_record_closure(&existing, head)?;
            if let Some(alias) = intent.conversation_alias.as_ref() {
                let alias_key = alias.storage_key();
                let stored = self
                    .engine
                    .get_json_value("conversation_transcript_alias", &alias_key)?
                    .map(serde_json::from_value::<TranscriptConversationAlias>)
                    .transpose()
                    .map_err(|error| {
                        Error::config("conversation_transcript_alias", error.to_string())
                    })?;
                if stored.as_ref() != Some(alias) {
                    return Err(Error::config(
                        "conversation_transcript_query_repair_required",
                        "idempotent transcript retry is missing its exact conversation alias",
                    ));
                }
            }
            return Ok(TranscriptCommitReport {
                key: existing.key,
                turn_id: existing.turn_id,
                sequence: existing.sequence,
                committed: false,
                before_count,
                after_count: before_count,
                skipped_reason: Some("conversation_transcript_turn_already_committed".to_string()),
            });
        }
        let mut record = record.clone();
        let next_sequence = head
            .as_ref()
            .map(|head| head.last_sequence.saturating_add(1))
            .unwrap_or(1);
        if record.sequence == 0 {
            record.sequence = next_sequence;
        } else if record.sequence != next_sequence {
            return Err(Error::config(
                "conversation_transcript_append",
                "turn sequence must be the exact next conversation sequence",
            ));
        }
        let sequence = record.sequence;
        let value = serde_json::to_value(&record)
            .map_err(|error| Error::config("conversation_transcript_page", error.to_string()))?;
        let page_id = Self::conversation_page_for_sequence(sequence)?;
        let (_, page, page_before) =
            self.load_conversation_transcript_page(&record.key, &record.subject, page_id)?;
        let previous_entries = page
            .as_ref()
            .map(|page| page.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "conversation_transcript",
            &key,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                "conversation_transcript",
                &key,
            ),
            record.updated_at,
            &value,
        )?;
        let entries = replace_recall_index_address(previous_entries, address);
        let page_plan = self.plan_conversation_transcript_page(
            &record.key,
            &record.subject,
            page_id,
            page.as_ref(),
            page_before,
            entries,
        )?;
        let head_plan = self.plan_conversation_head(
            &record.key,
            &record.subject,
            head.as_ref(),
            head_before,
            sequence,
            sequence,
        )?;
        let next_manifest = serde_json::from_value::<ConversationRecallManifest>(
            head_plan.2.clone(),
        )
        .map_err(|error| Error::config("conversation_transcript_head", error.to_string()))?;
        let previous_catalog_head = self.load_catalog_head_exact(&record.key, &record.subject)?;
        if head.is_some() && previous_catalog_head.is_none() {
            return Err(Error::config(
                "conversation_transcript_query_migration_required",
                "existing transcript head has no v11 catalog closure",
            ));
        }
        let catalog_head = Self::transcript_catalog_head_after_append(
            previous_catalog_head.as_ref(),
            &next_manifest,
            &record,
        )?;
        let mut index_plans = vec![head_plan, page_plan];
        index_plans.extend(self.plan_transcript_query_indexes(&record, &catalog_head)?);

        let mut owner_mutations = vec![StoreMutation::PutJson {
            namespace: "conversation_transcript".to_string(),
            key: key.clone(),
            value,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "conversation_transcript".to_string(),
            record_key: key,
        }];
        if let Some(alias) = intent.conversation_alias.as_ref() {
            let alias_key = alias.storage_key();
            let alias_value = serde_json::to_value(alias).map_err(|error| {
                Error::config("conversation_transcript_alias", error.to_string())
            })?;
            let archive_plan = self.plan_archive_index_upsert_for_scope(
                &alias.memory_space_id,
                &alias.mounted_subject_id,
                RecallIndexAddress::json(
                    "conversation_transcript_alias",
                    &alias_key,
                    1,
                    alias.updated_at,
                    &alias_value,
                )?,
            )?;
            owner_mutations.push(StoreMutation::PutJson {
                namespace: "conversation_transcript_alias".to_string(),
                key: alias_key.clone(),
                value: alias_value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "conversation_transcript_alias".to_string(),
                record_key: alias_key,
            });
            index_plans.push(archive_plan);
        }

        let mut scope = self.recall_scope();
        scope
            .memory_space_id
            .clone_from(&record.key.memory_space_id);
        scope.subject_id.clone_from(&record.subject);
        scope.channel.clone_from(&record.key.channel_id);
        scope.conversation_id = Some(record.key.conversation_id.clone());
        self.commit_recall_indexed_mutations(
            "conversation.transcript.append",
            scope,
            owner_mutations,
            index_plans,
        )?;
        Ok(TranscriptCommitReport {
            key: record.key,
            turn_id: record.turn_id,
            sequence,
            committed: true,
            before_count,
            after_count: before_count.saturating_add(1),
            skipped_reason: None,
        })
    }

    fn remember_conversation_alias(&self, alias: &TranscriptConversationAlias) -> Result<()> {
        let _transaction_guard = self.lock_transaction("archive_recall_alias_write")?;
        let owner_key = alias.storage_key();
        let value = serde_json::to_value(alias)
            .map_err(|error| Error::config("archive_recall_manifest", error.to_string()))?;
        let manifest_key = ArchiveRecallManifest::build(
            1,
            &alias.memory_space_id,
            &alias.mounted_subject_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (previous, _) = self.load_typed_recall_index::<ArchiveRecallManifest>(&manifest_key)?;
        let previous_entries = previous
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "conversation_transcript_alias",
            &owner_key,
            next_entry_revision(
                previous_entries,
                RecallIndexAddressKind::Json,
                "conversation_transcript_alias",
                &owner_key,
            ),
            alias.updated_at,
            &value,
        )?;
        let index = self.plan_archive_index_upsert_for_scope(
            &alias.memory_space_id,
            &alias.mounted_subject_id,
            address,
        )?;
        let mut scope = self.recall_scope();
        scope.memory_space_id.clone_from(&alias.memory_space_id);
        scope.subject_id.clone_from(&alias.mounted_subject_id);
        scope.channel.clone_from(&alias.channel_id);
        scope.chat_id.clone_from(&alias.chat_id);
        scope.conversation_id = Some(alias.conversation_id.clone());
        self.commit_recall_indexed_mutations(
            "archive.conversation_alias.write",
            scope,
            vec![StoreMutation::PutJson {
                namespace: "conversation_transcript_alias".to_string(),
                key: owner_key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "conversation_transcript_alias".to_string(),
                record_key: owner_key,
            }],
            vec![index],
        )?;
        Ok(())
    }

    fn resolve_conversation_alias(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel_id: &str,
        chat_id: &str,
    ) -> Result<Option<String>> {
        let key = TranscriptConversationAlias::storage_key_for(
            memory_space_id,
            mounted_subject_id,
            channel_id,
            chat_id,
        );
        Ok(self
            .json_get::<TranscriptConversationAlias>("conversation_transcript_alias", &key)?
            .filter(|alias| {
                alias.memory_space_id == memory_space_id
                    && alias.mounted_subject_id == mounted_subject_id
                    && alias.channel_id == channel_id
                    && alias.chat_id == chat_id
            })
            .map(|alias| alias.conversation_id))
    }

    fn get_turn(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: &str,
    ) -> Result<Option<TranscriptTurnRecord>> {
        let owner_key = transcript_turn_storage_key(key, mounted_subject_id, turn_id);
        let Some(value) = self
            .engine
            .get_json_value("conversation_transcript", &owner_key)?
        else {
            return Ok(None);
        };
        let record = serde_json::from_value::<TranscriptTurnRecord>(value)
            .map_err(|error| Error::config("conversation_transcript_page", error.to_string()))?;
        if record.key != *key || record.subject != mounted_subject_id {
            return Err(Error::config(
                "conversation_recall_manifest",
                "transcript owner scope differs from the requested subject root",
            ));
        }
        let (_, head, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let head = head.ok_or_else(|| {
            Error::config(
                "conversation_transcript_head",
                "transcript owner exists without its required head",
            )
        })?;
        self.validate_conversation_manifest_subject(&head, mounted_subject_id)?;
        let page_id = Self::conversation_page_for_sequence(record.sequence)?;
        let records =
            self.load_validated_conversation_page_records(key, mounted_subject_id, &head, page_id)?;
        if !records.iter().any(|candidate| candidate == &record) {
            return Err(Error::config(
                "conversation_transcript_page",
                "transcript owner is not bound by its sequence page",
            ));
        }
        Ok(Some(record))
    }

    fn list_turns(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        limit: usize,
    ) -> Result<Vec<TranscriptTurnRecord>> {
        let (_, head, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let Some(head) = head else {
            return Ok(Vec::new());
        };
        self.validate_conversation_manifest_subject(&head, mounted_subject_id)?;
        let requested = if limit == 0 {
            head.turn_count
        } else {
            u64::try_from(limit)
                .unwrap_or(u64::MAX)
                .min(head.turn_count)
        };
        if requested == 0 {
            return Ok(Vec::new());
        }
        let first_sequence = head
            .last_sequence
            .saturating_sub(requested)
            .saturating_add(1);
        let first_page = Self::conversation_page_for_sequence(first_sequence)?;
        let mut records = Vec::new();
        for page_id in first_page..head.page_count {
            records.extend(self.load_validated_conversation_page_records(
                key,
                mounted_subject_id,
                &head,
                page_id,
            )?);
        }
        records.retain(|record| record.sequence >= first_sequence);
        Ok(records)
    }

    fn list_conversation_catalog(
        &self,
        mounted_subject_id: &str,
        query: &TranscriptCatalogQuery,
    ) -> Result<ConversationCatalogCandidatePage> {
        query.validate()?;
        let (_, root, _, pages) = self.load_catalog_pages(
            &query.memory_space_id,
            mounted_subject_id,
            query.channel_id.as_deref(),
        )?;
        let Some(root) = root else {
            return Ok(ConversationCatalogCandidatePage {
                heads: Vec::new(),
                next_cursor: None,
                has_more: false,
            });
        };
        let keyring = self.ensure_query_keyring(
            &query.memory_space_id,
            mounted_subject_id,
            query.channel_id.as_deref(),
        )?;
        let root_bytes = serde_json::to_vec(&root).map_err(|error| {
            Error::config("conversation_transcript_catalog_index", error.to_string())
        })?;
        let root_digest = format!("sha256:{:x}", Sha256::digest(root_bytes));
        let kind = match query.lifecycle {
            TranscriptCatalogLifecycle::ActiveOnly => "catalog_active",
            TranscriptCatalogLifecycle::ActiveAndArchived => "catalog_active_archived",
        };
        let query_digest = format!(
            "sha256:{:x}",
            Sha256::digest(
                format!(
                    "catalog:{}:{}:{}:{:?}:{}",
                    query.memory_space_id,
                    query.governance_context_digest,
                    query.channel_id.as_deref().unwrap_or("*"),
                    query.lifecycle,
                    query.limit,
                )
                .as_bytes()
            )
        );
        let start = if let Some(cursor) = query.cursor.as_ref() {
            let claims = decode_cursor(&keyring, cursor)?;
            if claims.kind != kind
                || claims.memory_space_id != query.memory_space_id
                || claims.mounted_subject_id != mounted_subject_id
                || claims.channel_id != query.channel_id.as_deref().unwrap_or("*")
                || claims.head_revision != root.revision
                || claims.head_digest != root_digest
                || claims.query_digest != query_digest
                || claims.limit != query.limit
                || claims.lifecycle != format!("{:?}", query.lifecycle)
                || claims.direction != "forward"
                || claims.view_context != "candidate"
                || claims.snapshot_upper_bound != root.entry_count
                || claims.content_generation != root.revision
                || claims.index_generation != root.revision
                || claims.incarnation != keyring.incarnation
                || claims.expires_at < current_unix_secs()
            {
                return Err(Error::config(
                    "conversation_transcript_query_cursor_stale",
                    "catalog cursor is stale or outside scope",
                ));
            }
            usize::try_from(claims.position).unwrap_or(usize::MAX)
        } else {
            0
        };
        let mut heads = pages
            .into_iter()
            .flat_map(|(_, page, _)| page.heads)
            .filter(|head| match query.lifecycle {
                TranscriptCatalogLifecycle::ActiveOnly => head.lifecycle.active.turn_count > 0,
                TranscriptCatalogLifecycle::ActiveAndArchived => {
                    head.lifecycle.active.turn_count > 0 || head.lifecycle.archived.turn_count > 0
                }
            })
            .collect::<Vec<_>>();
        if start > heads.len() {
            return Err(Error::config(
                "conversation_transcript_query_cursor_stale",
                "catalog cursor position is outside snapshot",
            ));
        }
        heads = heads.into_iter().skip(start).collect();
        let has_more = heads.len() > query.limit;
        heads.truncate(query.limit);
        let next_position = start.saturating_add(heads.len());
        let now = current_unix_secs();
        let next_cursor = if has_more {
            Some(encode_cursor(
                &keyring,
                &TranscriptQueryCursorClaimsV1 {
                    schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                    key_id: keyring.current.key_id.clone(),
                    kind: kind.to_string(),
                    direction: "forward".to_string(),
                    query_digest: query_digest.clone(),
                    lifecycle: format!("{:?}", query.lifecycle),
                    view_context: "candidate".to_string(),
                    limit: query.limit,
                    memory_space_id: query.memory_space_id.clone(),
                    mounted_subject_id: mounted_subject_id.to_string(),
                    channel_id: query.channel_id.clone().unwrap_or_else(|| "*".to_string()),
                    conversation_id: "*".to_string(),
                    head_revision: root.revision,
                    head_digest: root_digest,
                    snapshot_upper_bound: root.entry_count,
                    content_generation: root.revision,
                    index_generation: root.revision,
                    position: u64::try_from(next_position).unwrap_or(u64::MAX),
                    incarnation: keyring.incarnation.clone(),
                    issued_at: now,
                    expires_at: now.saturating_add(604_800).min(keyring.current.expires_at),
                },
            )?)
        } else {
            None
        };
        Ok(ConversationCatalogCandidatePage {
            heads,
            next_cursor,
            has_more,
        })
    }

    fn query_transcript_timeline(
        &self,
        mounted_subject_id: &str,
        query: &TranscriptTimelineQuery,
    ) -> Result<TranscriptTimelineCandidatePage> {
        query.validate()?;
        let (_, manifest, _) =
            self.load_conversation_recall_manifest(&query.key, mounted_subject_id)?;
        let manifest = manifest.ok_or_else(|| {
            Error::config(
                "conversation_transcript_timeline",
                "conversation head not found",
            )
        })?;
        let head = self
            .load_catalog_head_exact(&query.key, mounted_subject_id)?
            .ok_or_else(|| {
                Error::config(
                    "conversation_transcript_timeline",
                    "conversation catalog head not found",
                )
            })?;
        if head.revision != manifest.revision || head.head_digest != manifest.head_digest {
            return Err(Error::config(
                "conversation_transcript_timeline",
                "catalog and transcript head identity differ",
            ));
        }
        let keyring = self.ensure_query_keyring(
            &query.key.memory_space_id,
            mounted_subject_id,
            Some(&query.key.channel_id),
        )?;
        let anchor_bytes = serde_json::to_vec(&query.anchor).map_err(|error| {
            Error::config("conversation_transcript_query_cursor", error.to_string())
        })?;
        let query_digest = format!(
            "sha256:{:x}",
            Sha256::digest(
                [
                    b"timeline:".as_slice(),
                    query.key.memory_space_id.as_bytes(),
                    query.key.channel_id.as_bytes(),
                    query.key.conversation_id.as_bytes(),
                    mounted_subject_id.as_bytes(),
                    query.governance_context_digest.as_bytes(),
                    &anchor_bytes,
                    query.limit.to_string().as_bytes(),
                ]
                .concat()
            )
        );
        let cursor_claims = query
            .cursor
            .as_ref()
            .map(|cursor| decode_cursor(&keyring, cursor))
            .transpose()?;
        if let Some(claims) = cursor_claims.as_ref() {
            let direction_is_valid = matches!(
                (claims.kind.as_str(), claims.direction.as_str()),
                ("timeline_older", "backward") | ("timeline_newer", "forward")
            );
            if !direction_is_valid
                || claims.memory_space_id != query.key.memory_space_id
                || claims.mounted_subject_id != mounted_subject_id
                || claims.channel_id != query.key.channel_id
                || claims.conversation_id != query.key.conversation_id
                || claims.incarnation != keyring.incarnation
                || claims.expires_at < current_unix_secs()
                || claims.head_revision > head.revision
                || (claims.head_revision == head.revision && claims.head_digest != head.head_digest)
                || claims.query_digest != query_digest
                || claims.limit != query.limit
                || claims.lifecycle != "active_and_archived"
                || claims.view_context != "candidate"
                || claims.snapshot_upper_bound > manifest.last_sequence
                || claims.content_generation != head.content_generation
                || claims.index_generation != head.index_generation
            {
                return Err(Error::config(
                    "conversation_transcript_query_cursor_stale",
                    "cursor scope, snapshot, incarnation, or expiry is stale",
                ));
            }
        }
        let snapshot_upper_bound = cursor_claims
            .as_ref()
            .map(|claims| claims.snapshot_upper_bound)
            .unwrap_or(manifest.last_sequence);
        let effective_anchor = cursor_claims.as_ref().map(|claims| claims.position);
        let (turns, _, _) = if let Some(position) = effective_anchor {
            if cursor_claims
                .as_ref()
                .is_some_and(|claims| claims.kind == "timeline_newer")
            {
                let turns = self.load_visible_timeline_forward(
                    &query.key,
                    mounted_subject_id,
                    &manifest,
                    position.saturating_add(1),
                    query.limit,
                )?;
                (turns, true, false)
            } else {
                let turns = self.load_visible_timeline_backward(
                    &query.key,
                    mounted_subject_id,
                    &manifest,
                    position.saturating_sub(1),
                    query.limit,
                )?;
                (turns, false, true)
            }
        } else {
            match &query.anchor {
                TranscriptTimelineAnchor::Latest => {
                    let turns = self.load_visible_timeline_backward(
                        &query.key,
                        mounted_subject_id,
                        &manifest,
                        manifest.last_sequence,
                        query.limit,
                    )?;
                    let has_older = turns.first().is_some_and(|turn| turn.sequence > 1);
                    (turns, has_older, false)
                }
                TranscriptTimelineAnchor::Before(anchor) => {
                    let end = anchor.locator.turn_sequence.saturating_sub(1);
                    let turns = self.load_visible_timeline_backward(
                        &query.key,
                        mounted_subject_id,
                        &manifest,
                        end,
                        query.limit,
                    )?;
                    let has_older = turns.first().is_some_and(|turn| turn.sequence > 1);
                    (turns, has_older, end < manifest.last_sequence)
                }
                TranscriptTimelineAnchor::After(anchor) => {
                    let start = anchor.locator.turn_sequence.saturating_add(1);
                    let turns = self.load_visible_timeline_forward(
                        &query.key,
                        mounted_subject_id,
                        &manifest,
                        start,
                        query.limit,
                    )?;
                    let has_newer = turns
                        .last()
                        .is_some_and(|turn| turn.sequence < manifest.last_sequence);
                    (turns, start > 1, has_newer)
                }
                TranscriptTimelineAnchor::Around(anchor) => {
                    let center = anchor.locator.turn_sequence;
                    let older_limit = query.limit / 2;
                    let mut turns = self.load_visible_timeline_backward(
                        &query.key,
                        mounted_subject_id,
                        &manifest,
                        center,
                        older_limit.saturating_add(1),
                    )?;
                    let next = turns
                        .last()
                        .map(|turn| turn.sequence.saturating_add(1))
                        .unwrap_or(center);
                    let remaining = query.limit.saturating_sub(turns.len());
                    turns.extend(self.load_visible_timeline_forward(
                        &query.key,
                        mounted_subject_id,
                        &manifest,
                        next,
                        remaining,
                    )?);
                    let has_older = turns.first().is_some_and(|turn| turn.sequence > 1);
                    let has_newer = turns
                        .last()
                        .is_some_and(|turn| turn.sequence < manifest.last_sequence);
                    (turns, has_older, has_newer)
                }
                TranscriptTimelineAnchor::AroundSequence(sequence) => {
                    let older_limit = query.limit / 2;
                    let mut turns = self.load_visible_timeline_backward(
                        &query.key,
                        mounted_subject_id,
                        &manifest,
                        *sequence,
                        older_limit.saturating_add(1),
                    )?;
                    let next = turns
                        .last()
                        .map(|turn| turn.sequence.saturating_add(1))
                        .unwrap_or(*sequence);
                    let remaining = query.limit.saturating_sub(turns.len());
                    turns.extend(self.load_visible_timeline_forward(
                        &query.key,
                        mounted_subject_id,
                        &manifest,
                        next,
                        remaining,
                    )?);
                    let has_older = turns.first().is_some_and(|turn| turn.sequence > 1);
                    let has_newer = turns
                        .last()
                        .is_some_and(|turn| turn.sequence < manifest.last_sequence);
                    (turns, has_older, has_newer)
                }
                TranscriptTimelineAnchor::FirstVisibleInRange(range) => {
                    let Some(sequence) = self.first_visible_sequence_in_range(
                        &query.key,
                        mounted_subject_id,
                        range,
                    )?
                    else {
                        return Ok(TranscriptTimelineCandidatePage {
                            head,
                            turns: Vec::new(),
                            older_cursor: None,
                            newer_cursor: None,
                            has_older: false,
                            has_newer: false,
                        });
                    };
                    let turns = self.load_visible_timeline_forward(
                        &query.key,
                        mounted_subject_id,
                        &manifest,
                        sequence,
                        query.limit,
                    )?;
                    let has_newer = turns
                        .last()
                        .is_some_and(|turn| turn.sequence < manifest.last_sequence);
                    (turns, sequence > 1, has_newer)
                }
            }
        };
        let has_older = match turns.first() {
            Some(first) => self.has_visible_timeline_before(
                &query.key,
                mounted_subject_id,
                &manifest,
                first.sequence,
            )?,
            None => false,
        };
        let has_newer = match turns.last() {
            Some(last) => self.has_visible_timeline_after_until(
                &query.key,
                mounted_subject_id,
                &manifest,
                last.sequence,
                snapshot_upper_bound,
            )?,
            None => false,
        };
        let now = current_unix_secs();
        let encode_timeline_cursor =
            |kind: &str, direction: &str, position: u64| -> Result<TranscriptQueryCursor> {
                encode_cursor(
                    &keyring,
                    &TranscriptQueryCursorClaimsV1 {
                        schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                        key_id: keyring.current.key_id.clone(),
                        kind: kind.to_string(),
                        direction: direction.to_string(),
                        query_digest: query_digest.clone(),
                        lifecycle: "active_and_archived".to_string(),
                        view_context: "candidate".to_string(),
                        limit: query.limit,
                        memory_space_id: query.key.memory_space_id.clone(),
                        mounted_subject_id: mounted_subject_id.to_string(),
                        channel_id: query.key.channel_id.clone(),
                        conversation_id: query.key.conversation_id.clone(),
                        head_revision: cursor_claims
                            .as_ref()
                            .map(|claims| claims.head_revision)
                            .unwrap_or(head.revision),
                        head_digest: cursor_claims
                            .as_ref()
                            .map(|claims| claims.head_digest.clone())
                            .unwrap_or_else(|| head.head_digest.clone()),
                        snapshot_upper_bound,
                        content_generation: cursor_claims
                            .as_ref()
                            .map(|claims| claims.content_generation)
                            .unwrap_or(head.content_generation),
                        index_generation: cursor_claims
                            .as_ref()
                            .map(|claims| claims.index_generation)
                            .unwrap_or(head.index_generation),
                        position,
                        incarnation: keyring.incarnation.clone(),
                        issued_at: now,
                        expires_at: now.saturating_add(604_800).min(keyring.current.expires_at),
                    },
                )
            };
        let older_cursor = if has_older {
            turns
                .first()
                .map(|turn| encode_timeline_cursor("timeline_older", "backward", turn.sequence))
                .transpose()?
        } else {
            None
        };
        let newer_cursor = if has_newer {
            turns
                .last()
                .map(|turn| encode_timeline_cursor("timeline_newer", "forward", turn.sequence))
                .transpose()?
        } else {
            None
        };
        Ok(TranscriptTimelineCandidatePage {
            head,
            turns,
            older_cursor,
            newer_cursor,
            has_older,
            has_newer,
        })
    }

    fn search_transcript(
        &self,
        mounted_subject_id: &str,
        query: &TranscriptSearchQuery,
    ) -> Result<TranscriptSearchCandidatePage> {
        query.validate()?;
        let memory_space_id = match &query.scope {
            TranscriptSearchScope::ExactConversation { key } => key.memory_space_id.as_str(),
            TranscriptSearchScope::MountedSubject {
                memory_space_id, ..
            } => memory_space_id,
        };
        let include_archived = query.lifecycle == TranscriptSearchLifecycle::ActiveAndArchived;
        let query_channel = match &query.scope {
            TranscriptSearchScope::ExactConversation { key } => Some(key.channel_id.as_str()),
            TranscriptSearchScope::MountedSubject { channel_id, .. } => channel_id.as_deref(),
        };
        let keyring =
            self.ensure_query_keyring(memory_space_id, mounted_subject_id, query_channel)?;
        let query_identity = serde_json::to_vec(&serde_json::json!({
            "scope": &query.scope,
            "query": &query.query,
            "governance_context_digest": &query.governance_context_digest,
            "sort": query.sort,
            "lifecycle": query.lifecycle,
            "limit": query.limit,
            "view": "candidate",
        }))
        .map_err(|error| {
            Error::config("conversation_transcript_query_cursor", error.to_string())
        })?;
        let query_digest = format!("sha256:{:x}", Sha256::digest(query_identity));
        let mut snapshot_hasher = Sha256::new();
        let mut snapshot_revision = 0u64;
        let mut snapshot_entries = 0u64;
        let mut scores = BTreeMap::<
            (String, String, String, String, String),
            (u32, TranscriptPostingLocatorV1),
        >::new();
        for term in &query.query.terms {
            let digest = term_digest(term);
            let root_key = search_root_key(memory_space_id, mounted_subject_id, &digest);
            let Some(root_value) = self
                .engine
                .get_json_value(TRANSCRIPT_SEARCH_ROOT_NAMESPACE, &root_key)?
            else {
                continue;
            };
            let root = serde_json::from_value::<TranscriptSearchPostingRootV1>(root_value)
                .map_err(|error| {
                    Error::config("conversation_transcript_search_index", error.to_string())
                })?;
            snapshot_hasher.update(serde_json::to_vec(&root).map_err(|error| {
                Error::config("conversation_transcript_search_index", error.to_string())
            })?);
            snapshot_revision = snapshot_revision.max(root.revision);
            snapshot_entries = snapshot_entries.saturating_add(root.entry_count);
            let mut postings = Vec::new();
            for page_id in 0..root.page_count {
                let key = search_posting_key(memory_space_id, mounted_subject_id, &digest, page_id);
                let value = self
                    .engine
                    .get_json_value(TRANSCRIPT_SEARCH_POSTING_NAMESPACE, &key)?
                    .ok_or_else(|| {
                        Error::config(
                            "conversation_transcript_search_index",
                            "search root references a missing page",
                        )
                    })?;
                let page = serde_json::from_value::<TranscriptSearchPostingPageV1>(value).map_err(
                    |error| {
                        Error::config("conversation_transcript_search_index", error.to_string())
                    },
                )?;
                postings.extend(page.locators);
            }
            for locator in postings
                .into_iter()
                .filter(|locator| locator.visible(include_archived))
            {
                let in_scope = match &query.scope {
                    TranscriptSearchScope::ExactConversation { key } => locator.locator.key == *key,
                    TranscriptSearchScope::MountedSubject { channel_id, .. } => channel_id
                        .as_ref()
                        .is_none_or(|channel| locator.locator.key.channel_id == *channel),
                };
                if !in_scope {
                    continue;
                }
                let identity = (
                    locator.locator.key.memory_space_id.clone(),
                    locator.locator.key.channel_id.clone(),
                    locator.locator.key.conversation_id.clone(),
                    locator.locator.turn_id.clone(),
                    locator.locator.message_id.clone().unwrap_or_default(),
                );
                let entry = scores.entry(identity).or_insert((0, locator));
                entry.0 = entry.0.saturating_add(1);
            }
        }
        let mut candidates = scores
            .into_values()
            .map(|(score, locator)| {
                let observed_at = locator.locator.observed_at;
                let record = self
                    .get_turn(
                        &locator.locator.key,
                        mounted_subject_id,
                        &locator.locator.turn_id,
                    )?
                    .ok_or_else(|| {
                        Error::config(
                            "conversation_transcript_search_index",
                            "posting references a missing turn",
                        )
                    })?;
                let owner_head = self
                    .load_conversation_recall_manifest(&locator.locator.key, mounted_subject_id)?
                    .1
                    .ok_or_else(|| {
                        Error::config(
                            "conversation_transcript_search_index",
                            "posting owner head is missing",
                        )
                    })?;
                Ok((
                    TranscriptSearchCandidate {
                        record,
                        message_id: locator.locator.message_id.unwrap_or_default(),
                        score,
                        head_revision: owner_head.revision,
                        head_digest: owner_head.head_digest,
                    },
                    observed_at,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        candidates.sort_by(|(left, left_observed_at), (right, right_observed_at)| {
            match query.sort {
                TranscriptSearchSort::RelevanceThenObservedAt => right
                    .score
                    .cmp(&left.score)
                    .then_with(|| right_observed_at.cmp(left_observed_at)),
                TranscriptSearchSort::ObservedAtDescending => {
                    right_observed_at.cmp(left_observed_at)
                }
            }
            .then_with(|| right.record.sequence.cmp(&left.record.sequence))
            .then_with(|| right.message_id.cmp(&left.message_id))
        });
        let snapshot_digest = format!("sha256:{:x}", snapshot_hasher.finalize());
        let start = if let Some(cursor) = query.cursor.as_ref() {
            let claims = decode_cursor(&keyring, cursor)?;
            if claims.kind != "search"
                || claims.direction != "forward"
                || claims.query_digest != query_digest
                || claims.lifecycle != format!("{:?}", query.lifecycle)
                || claims.view_context != "candidate"
                || claims.limit != query.limit
                || claims.memory_space_id != memory_space_id
                || claims.mounted_subject_id != mounted_subject_id
                || claims.head_revision != snapshot_revision
                || claims.head_digest != snapshot_digest
                || claims.snapshot_upper_bound != snapshot_entries
                || claims.incarnation != keyring.incarnation
                || claims.expires_at < current_unix_secs()
            {
                return Err(Error::config(
                    "conversation_transcript_query_cursor_stale",
                    "search cursor is stale or outside query scope",
                ));
            }
            usize::try_from(claims.position).unwrap_or(usize::MAX)
        } else {
            0
        };
        if start > candidates.len() {
            return Err(Error::config(
                "conversation_transcript_query_cursor_stale",
                "search cursor position is outside snapshot",
            ));
        }
        candidates = candidates.into_iter().skip(start).collect();
        let has_more = candidates.len() > query.limit;
        candidates.truncate(query.limit);
        let now = current_unix_secs();
        let next_position = start.saturating_add(candidates.len());
        let next_cursor = if has_more {
            Some(encode_cursor(
                &keyring,
                &TranscriptQueryCursorClaimsV1 {
                    schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                    key_id: keyring.current.key_id.clone(),
                    kind: "search".to_string(),
                    direction: "forward".to_string(),
                    query_digest,
                    lifecycle: format!("{:?}", query.lifecycle),
                    view_context: "candidate".to_string(),
                    limit: query.limit,
                    memory_space_id: memory_space_id.to_string(),
                    mounted_subject_id: mounted_subject_id.to_string(),
                    channel_id: "*".to_string(),
                    conversation_id: "*".to_string(),
                    head_revision: snapshot_revision,
                    head_digest: snapshot_digest,
                    snapshot_upper_bound: snapshot_entries,
                    content_generation: snapshot_revision,
                    index_generation: snapshot_revision,
                    position: u64::try_from(next_position).unwrap_or(u64::MAX),
                    incarnation: keyring.incarnation.clone(),
                    issued_at: now,
                    expires_at: now.saturating_add(604_800).min(keyring.current.expires_at),
                },
            )?)
        } else {
            None
        };
        Ok(TranscriptSearchCandidatePage {
            candidates: candidates
                .into_iter()
                .map(|(candidate, _)| candidate)
                .collect(),
            next_cursor,
            has_more,
            budget_applied: false,
        })
    }

    fn query_transcript_activity(
        &self,
        mounted_subject_id: &str,
        query: &TranscriptActivityQuery,
    ) -> Result<TranscriptActivityCandidateReport> {
        query.validate()?;
        let head = self
            .load_catalog_head_exact(&query.key, mounted_subject_id)?
            .ok_or_else(|| {
                Error::config(
                    "conversation_transcript_activity",
                    "conversation catalog head not found",
                )
            })?;
        let mut buckets = Vec::with_capacity(query.ranges.len());
        for range in &query.ranges {
            let first_day = range.start_inclusive / 86_400;
            let last_day = range.end_exclusive.saturating_sub(1) / 86_400;
            let mut candidates = Vec::new();
            for day in first_day..=last_day {
                let root_key = time_root_key(&query.key, mounted_subject_id, day);
                let Some(root_value) = self
                    .engine
                    .get_json_value(TRANSCRIPT_TIME_ROOT_NAMESPACE, &root_key)?
                else {
                    continue;
                };
                let root = serde_json::from_value::<TranscriptTimePostingRootV1>(root_value)
                    .map_err(|error| {
                        Error::config("conversation_transcript_time_index", error.to_string())
                    })?;
                let mut postings = Vec::new();
                for page_id in 0..root.page_count {
                    let key = time_posting_key(&query.key, mounted_subject_id, day, page_id);
                    let value = self
                        .engine
                        .get_json_value(TRANSCRIPT_TIME_POSTING_NAMESPACE, &key)?
                        .ok_or_else(|| {
                            Error::config(
                                "conversation_transcript_time_index",
                                "time root references a missing page",
                            )
                        })?;
                    let page = serde_json::from_value::<TranscriptTimePostingPageV1>(value)
                        .map_err(|error| {
                            Error::config("conversation_transcript_time_index", error.to_string())
                        })?;
                    postings.extend(page.locators);
                }
                for locator in postings.into_iter().filter(|locator| {
                    locator.visible(query.lifecycle == TranscriptSearchLifecycle::ActiveAndArchived)
                        && range.contains(locator.locator.observed_at)
                }) {
                    let record = self
                        .get_turn(&query.key, mounted_subject_id, &locator.locator.turn_id)?
                        .ok_or_else(|| {
                            Error::config(
                                "conversation_transcript_time_index",
                                "posting references a missing turn",
                            )
                        })?;
                    candidates.push(TranscriptActivityCandidate {
                        record,
                        message_id: locator.locator.message_id.unwrap_or_default(),
                    });
                }
            }
            buckets.push(TranscriptActivityCandidateBucket {
                range: *range,
                candidates,
            });
        }
        Ok(TranscriptActivityCandidateReport {
            head,
            buckets,
            budget_applied: false,
        })
    }

    fn turn_count(&self, key: &ConversationKey, mounted_subject_id: &str) -> Result<usize> {
        let (_, head, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let Some(head) = head else {
            return Ok(0);
        };
        self.validate_conversation_manifest_subject(&head, mounted_subject_id)?;
        usize::try_from(head.turn_count).map_err(|_| {
            Error::config(
                "conversation_transcript_head",
                "turn count does not fit the platform",
            )
        })
    }

    fn list_turns_page(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<TranscriptTurnPage> {
        let decoded_cursor = cursor
            .map(str::trim)
            .filter(|cursor| !cursor.is_empty())
            .map(|encoded| TranscriptTurnCursor::decode_for_scope(encoded, key, mounted_subject_id))
            .transpose()?;
        let (_, head, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let Some(head) = head else {
            if decoded_cursor.is_some() {
                return Err(Error::config(
                    "conversation_transcript_page",
                    "cursor_not_found",
                ));
            }
            return Ok(TranscriptTurnPage {
                key: key.clone(),
                turns: Vec::new(),
                next_cursor: None,
                has_more: false,
            });
        };
        self.validate_conversation_manifest_subject(&head, mounted_subject_id)?;
        let start_sequence = match decoded_cursor {
            Some(cursor) => {
                let turn = self
                    .get_turn(key, mounted_subject_id, &cursor.turn_id)?
                    .ok_or_else(|| {
                        Error::config("conversation_transcript_page", "cursor_not_found")
                    })?;
                if turn.sequence != cursor.sequence {
                    return Err(Error::config(
                        "conversation_transcript_page",
                        "cursor_sequence_mismatch",
                    ));
                }
                cursor.sequence.saturating_add(1)
            }
            None => 1,
        };
        if start_sequence > head.last_sequence {
            return Ok(TranscriptTurnPage {
                key: key.clone(),
                turns: Vec::new(),
                next_cursor: None,
                has_more: false,
            });
        }
        let limit = limit.max(1);
        let first_page = Self::conversation_page_for_sequence(start_sequence)?;
        let mut turns = Vec::with_capacity(limit.saturating_add(1));
        for page_id in first_page..head.page_count {
            for turn in self.load_validated_conversation_page_records(
                key,
                mounted_subject_id,
                &head,
                page_id,
            )? {
                if turn.sequence >= start_sequence {
                    turns.push(turn);
                }
                if turns.len() > limit {
                    break;
                }
            }
            if turns.len() > limit {
                break;
            }
        }
        let has_more = turns.len() > limit;
        if has_more {
            turns.truncate(limit);
        }
        let next_cursor = if has_more {
            turns
                .last()
                .map(TranscriptTurnCursor::for_record)
                .map(|cursor| cursor.encode())
                .transpose()?
        } else {
            None
        };
        Ok(TranscriptTurnPage {
            key: key.clone(),
            turns,
            next_cursor,
            has_more,
        })
    }

    fn upsert_transcript_attrs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        attrs: &[TranscriptAttrEnvelope],
    ) -> Result<TranscriptAttrWriteReport> {
        let _transaction_guard = self.lock_transaction("conversation_recall_manifest_attrs")?;
        let (_, head, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let head = head.ok_or_else(|| {
            Error::config(
                "conversation_recall_manifest",
                "transcript attrs require an existing conversation recall manifest",
            )
        })?;
        self.validate_conversation_manifest_subject(&head, mounted_subject_id)?;
        let mut accepted_attrs = Vec::new();
        let mut rejected_attrs = Vec::new();
        let mut mutations = Vec::new();
        let mut aux_by_turn = BTreeMap::<
            String,
            (
                Option<ConversationTranscriptAuxManifest>,
                Option<serde_json::Value>,
                Vec<RecallIndexAddress>,
            ),
        >::new();
        let mut seen_keys = BTreeSet::new();
        let mut owner_subject_id = None::<String>;
        for attr in attrs {
            if attr.target.key != *key {
                rejected_attrs.push(transcript_attr_rejection(
                    attr,
                    "attr target key does not match conversation key",
                ));
                continue;
            }
            let Some(turn) = self.get_turn(key, mounted_subject_id, &attr.target.turn_id)? else {
                rejected_attrs.push(transcript_attr_rejection(
                    attr,
                    "attr target turn does not exist",
                ));
                continue;
            };
            if let Err(error) = attr.validate_for_record(&turn) {
                rejected_attrs.push(transcript_attr_rejection(attr, error.to_string()));
                continue;
            }
            if owner_subject_id
                .as_ref()
                .is_some_and(|subject_id| subject_id != &turn.subject)
            {
                rejected_attrs.push(transcript_attr_rejection(
                    attr,
                    "attrs from different subject owners cannot share one transaction",
                ));
                continue;
            }
            owner_subject_id.get_or_insert_with(|| turn.subject.clone());
            let owner_key = transcript_attr_storage_key(key, mounted_subject_id, attr);
            if !seen_keys.insert(owner_key.clone()) {
                rejected_attrs.push(transcript_attr_rejection(
                    attr,
                    "duplicate attr storage key in one write",
                ));
                continue;
            }
            if !aux_by_turn.contains_key(&attr.target.turn_id) {
                let (_, manifest, before) = self.load_conversation_transcript_aux(
                    key,
                    mounted_subject_id,
                    &attr.target.turn_id,
                )?;
                let entries = manifest
                    .as_ref()
                    .map(|manifest| manifest.entries.clone())
                    .unwrap_or_default();
                aux_by_turn.insert(attr.target.turn_id.clone(), (manifest, before, entries));
            }
            let (_, _, entries) = aux_by_turn
                .get_mut(&attr.target.turn_id)
                .expect("turn aux initialized");
            let value = serde_json::to_value(attr)
                .map_err(|error| Error::config("conversation_transcript_aux", error.to_string()))?;
            let address = RecallIndexAddress::json(
                "conversation_transcript_attr",
                &owner_key,
                next_entry_revision(
                    entries,
                    RecallIndexAddressKind::Json,
                    "conversation_transcript_attr",
                    &owner_key,
                ),
                attr.created_at,
                &value,
            )?;
            *entries = replace_recall_index_address(entries, address);
            mutations.push(StoreMutation::PutJson {
                namespace: "conversation_transcript_attr".to_string(),
                key: owner_key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "conversation_transcript_attr".to_string(),
                record_key: owner_key,
            });
            accepted_attrs.push(attr.clone());
        }
        if !mutations.is_empty() {
            let mut indexes = Vec::with_capacity(aux_by_turn.len());
            for (turn_id, (manifest, before, entries)) in aux_by_turn {
                indexes.push(self.plan_conversation_transcript_aux(
                    key,
                    mounted_subject_id,
                    &turn_id,
                    manifest.as_ref(),
                    before,
                    entries,
                )?);
            }
            let mut scope = self.recall_scope();
            scope.memory_space_id.clone_from(&key.memory_space_id);
            let owner_subject_id = owner_subject_id.expect("accepted attrs have a subject owner");
            if owner_subject_id != mounted_subject_id {
                return Err(Error::config(
                    "conversation_recall_manifest",
                    "transcript attr owner differs from the requested subject root",
                ));
            }
            scope.subject_id = owner_subject_id;
            scope.channel.clone_from(&key.channel_id);
            scope.conversation_id = Some(key.conversation_id.clone());
            self.commit_recall_indexed_mutations(
                "conversation.transcript_attrs.upsert",
                scope,
                mutations,
                indexes,
            )?;
        }
        Ok(TranscriptAttrWriteReport {
            key: key.clone(),
            accepted_attrs,
            rejected_attrs,
        })
    }

    fn list_transcript_attrs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Vec<TranscriptAttrEnvelope>> {
        let turns = match turn_id {
            Some(turn_id) => self
                .get_turn(key, mounted_subject_id, turn_id)?
                .into_iter()
                .collect::<Vec<_>>(),
            None => self.list_turns(key, mounted_subject_id, usize::MAX)?,
        };
        let mut attrs = Vec::new();
        for turn in turns {
            let (_, manifest, _) =
                self.load_conversation_transcript_aux(key, mounted_subject_id, &turn.turn_id)?;
            let Some(manifest) = manifest else {
                continue;
            };
            if manifest.memory_space_id != key.memory_space_id
                || manifest.mounted_subject_id != mounted_subject_id
                || manifest.channel_id != key.channel_id
                || manifest.conversation_id != key.conversation_id
                || manifest.turn_id != turn.turn_id
            {
                return Err(Error::config(
                    "conversation_transcript_aux",
                    "transcript aux scope differs from its target turn",
                ));
            }
            for entry in &manifest.entries {
                if entry.kind != RecallIndexAddressKind::Json
                    || entry.namespace != "conversation_transcript_attr"
                {
                    continue;
                }
                let value = self
                    .engine
                    .get_json_value("conversation_transcript_attr", &entry.key)?
                    .ok_or_else(|| {
                        Error::config(
                            "conversation_transcript_aux",
                            "indexed transcript attr owner is missing",
                        )
                    })?;
                let attr = serde_json::from_value::<TranscriptAttrEnvelope>(value.clone())
                    .map_err(|error| {
                        Error::config("conversation_transcript_aux", error.to_string())
                    })?;
                let expected_address = RecallIndexAddress::json(
                    "conversation_transcript_attr",
                    &entry.key,
                    entry.revision,
                    entry.updated_at,
                    &value,
                )?;
                if expected_address.content_sha256 != entry.content_sha256
                    || attr.target.key != *key
                    || attr.target.turn_id != turn.turn_id
                    || transcript_attr_storage_key(key, mounted_subject_id, &attr) != entry.key
                {
                    return Err(Error::config(
                        "conversation_transcript_aux",
                        "transcript attr owner binding differs from its turn aux",
                    ));
                }
                attr.validate_for_record(&turn)?;
                attrs.push(attr);
            }
        }
        attrs.sort_by(|left, right| {
            left.target
                .turn_id
                .cmp(&right.target.turn_id)
                .then_with(|| left.target.message_id.cmp(&right.target.message_id))
                .then_with(|| left.created_at.cmp(&right.created_at))
                .then_with(|| left.attr_id.cmp(&right.attr_id))
        });
        Ok(attrs)
    }

    fn inspect_repair_records(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
    ) -> Result<TranscriptRepairInspection> {
        let (_, head, _) = self.load_conversation_recall_manifest(key, mounted_subject_id)?;
        let mut inspection = TranscriptRepairInspection::default();
        if let Some(head) = head {
            self.validate_conversation_manifest_subject(&head, mounted_subject_id)?;
            for page_id in 0..head.page_count {
                inspection
                    .turns
                    .extend(self.load_validated_conversation_page_records(
                        key,
                        mounted_subject_id,
                        &head,
                        page_id,
                    )?);
            }
        }
        inspection.checked_turns = inspection.turns.len();
        for aux_key in self
            .engine
            .list_json_keys(CONVERSATION_TRANSCRIPT_AUX_MANIFEST_NAMESPACE)?
        {
            let Some(aux_value) = self
                .engine
                .get_json_value(CONVERSATION_TRANSCRIPT_AUX_MANIFEST_NAMESPACE, &aux_key)?
            else {
                continue;
            };
            let aux = decode_typed_recall_index::<ConversationTranscriptAuxManifest>(
                &aux_key, aux_value,
            )?;
            if aux.memory_space_id != key.memory_space_id
                || aux.mounted_subject_id != mounted_subject_id
                || aux.channel_id != key.channel_id
                || aux.conversation_id != key.conversation_id
            {
                continue;
            }
            for entry in &aux.entries {
                let Some(value) = self.engine.get_json_value(&entry.namespace, &entry.key)? else {
                    inspection.issues.push(TranscriptRepairIssue {
                        kind: TranscriptRepairIssueKind::CorruptRecord,
                        turn_id: aux.turn_id.clone(),
                        message_id: None,
                        derived_ref: None,
                        reason: format!(
                            "conversation_aux_owner_missing:{}:{}",
                            entry.namespace, entry.key
                        ),
                    });
                    continue;
                };
                match entry.namespace.as_str() {
                    "conversation_transcript_attr" => {
                        inspection.checked_attrs = inspection.checked_attrs.saturating_add(1);
                        match serde_json::from_value::<TranscriptAttrEnvelope>(value.clone()) {
                            Ok(attr)
                                if attr.target.key == *key
                                    && attr.target.turn_id == aux.turn_id
                                    && transcript_attr_storage_key(
                                        key,
                                        mounted_subject_id,
                                        &attr,
                                    ) == entry.key =>
                            {
                                inspection.attrs.push(attr);
                            }
                            Ok(attr) => inspection.issues.push(TranscriptRepairIssue {
                                kind: TranscriptRepairIssueKind::MismatchedAttrSourceKey,
                                turn_id: attr.target.turn_id,
                                message_id: attr.target.message_id,
                                derived_ref: None,
                                reason: "transcript_attr_owner_scope_or_storage_key_mismatch"
                                    .to_string(),
                            }),
                            Err(error) => {
                                let (turn_id, message_id) =
                                    transcript_attr_repair_target_from_value(&value);
                                inspection.issues.push(TranscriptRepairIssue {
                                    kind: TranscriptRepairIssueKind::CorruptTranscriptAttrRecord,
                                    turn_id,
                                    message_id,
                                    derived_ref: None,
                                    reason: format!("transcript_attr_decode_failed:{error}"),
                                });
                            }
                        }
                    }
                    "conversation_transcript_derived_ref" => {
                        inspection.checked_derived_refs =
                            inspection.checked_derived_refs.saturating_add(1);
                        match serde_json::from_value::<DerivedMemoryRef>(value) {
                            Ok(derived)
                                if validate_derived_ref_matches_key(key, &derived).is_ok()
                                    && derived.source.turn_id == aux.turn_id
                                    && exact_derived_owner_subject(&derived).ok()
                                        == Some(mounted_subject_id)
                                    && transcript_derived_ref_storage_key(
                                        key,
                                        mounted_subject_id,
                                        &derived,
                                    )
                                    .ok()
                                    .as_deref()
                                        == Some(entry.key.as_str()) =>
                            {
                                inspection.derived_refs.push(derived);
                            }
                            Ok(derived) => inspection.issues.push(TranscriptRepairIssue {
                                kind: TranscriptRepairIssueKind::MismatchedSourceKey,
                                turn_id: derived.source.turn_id.clone(),
                                message_id: derived.source.message_id.clone(),
                                derived_ref: Some(derived),
                                reason: "derived_memory_ref_owner_scope_or_storage_key_mismatch"
                                    .to_string(),
                            }),
                            Err(error) => inspection.issues.push(TranscriptRepairIssue {
                                kind: TranscriptRepairIssueKind::CorruptRecord,
                                turn_id: aux.turn_id.clone(),
                                message_id: None,
                                derived_ref: None,
                                reason: format!("derived_memory_ref_decode_failed:{error}"),
                            }),
                        }
                    }
                    _ => inspection.issues.push(TranscriptRepairIssue {
                        kind: TranscriptRepairIssueKind::CorruptRecord,
                        turn_id: aux.turn_id.clone(),
                        message_id: None,
                        derived_ref: None,
                        reason: format!(
                            "conversation_aux_contains_non_aux_owner:{}",
                            entry.namespace
                        ),
                    }),
                }
            }
        }
        Ok(inspection)
    }

    fn append_derived_memory_ref(
        &self,
        key: &ConversationKey,
        derived: &DerivedMemoryRef,
    ) -> Result<()> {
        validate_derived_ref_matches_key(key, derived)?;
        let subject_id = exact_derived_owner_subject(derived)?;
        let record_key = transcript_derived_ref_storage_key(key, subject_id, derived)?;
        let value = serde_json::to_value(derived).map_err(|error| {
            Error::config("conversation_transcript_derived_ref", error.to_string())
        })?;
        let mut scope = self.recall_scope();
        scope.memory_space_id.clone_from(&key.memory_space_id);
        scope.subject_id = subject_id.to_string();
        scope.channel.clone_from(&key.channel_id);
        scope.conversation_id = Some(key.conversation_id.clone());
        self.commit_governed_memory_transaction_authorized(
            StoreMutationBatch {
                transaction_id: format!("conversation-derived-ref:{}", current_unix_nanos()),
                operation: "conversation.derived_ref.append".to_string(),
                scope,
                mutations: vec![StoreMutation::PutJson {
                    namespace: "conversation_transcript_derived_ref".to_string(),
                    key: record_key.clone(),
                    value,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "conversation_transcript_derived_ref".to_string(),
                    record_key,
                }],
            },
            StoreCommitPreconditions::new(&[], &[]),
            None,
            None,
            Some(derived.created_at),
            None,
        )?;
        Ok(())
    }

    fn list_derived_memory_refs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Vec<DerivedMemoryRef>> {
        let turns = match turn_id {
            Some(turn_id) => self
                .get_turn(key, mounted_subject_id, turn_id)?
                .into_iter()
                .collect::<Vec<_>>(),
            None => self.list_turns(key, mounted_subject_id, usize::MAX)?,
        };
        let mut refs = Vec::new();
        for turn in turns {
            let (_, manifest, _) =
                self.load_conversation_transcript_aux(key, mounted_subject_id, &turn.turn_id)?;
            let Some(manifest) = manifest else {
                continue;
            };
            if manifest.memory_space_id != key.memory_space_id
                || manifest.mounted_subject_id != mounted_subject_id
                || manifest.channel_id != key.channel_id
                || manifest.conversation_id != key.conversation_id
                || manifest.turn_id != turn.turn_id
            {
                return Err(Error::config(
                    "conversation_transcript_aux",
                    "transcript aux scope differs from its target turn",
                ));
            }
            for entry in &manifest.entries {
                if entry.kind != RecallIndexAddressKind::Json
                    || entry.namespace != "conversation_transcript_derived_ref"
                {
                    continue;
                }
                let value = self
                    .engine
                    .get_json_value("conversation_transcript_derived_ref", &entry.key)?
                    .ok_or_else(|| {
                        Error::config(
                            "conversation_transcript_aux",
                            "indexed transcript derived owner is missing",
                        )
                    })?;
                let derived =
                    serde_json::from_value::<DerivedMemoryRef>(value.clone()).map_err(|error| {
                        Error::config("conversation_transcript_aux", error.to_string())
                    })?;
                let expected_address = RecallIndexAddress::json(
                    "conversation_transcript_derived_ref",
                    &entry.key,
                    entry.revision,
                    entry.updated_at,
                    &value,
                )?;
                validate_derived_ref_matches_key(key, &derived)?;
                if expected_address.content_sha256 != entry.content_sha256
                    || derived.source.turn_id != turn.turn_id
                    || exact_derived_owner_subject(&derived)? != mounted_subject_id
                    || transcript_derived_ref_storage_key(key, mounted_subject_id, &derived)?
                        != entry.key
                {
                    return Err(Error::config(
                        "conversation_transcript_aux",
                        "transcript derived owner binding differs from its turn aux",
                    ));
                }
                refs.push(derived);
            }
        }
        refs.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.store_key.cmp(&right.store_key))
                .then_with(|| left.source.turn_id.cmp(&right.source.turn_id))
                .then_with(|| left.source.message_id.cmp(&right.source.message_id))
        });
        Ok(refs)
    }

    fn apply_lifecycle_request(
        &self,
        mounted_subject_id: &str,
        request: &TranscriptLifecycleRequest,
    ) -> Result<TranscriptLifecycleReport> {
        let _transaction_guard = self.lock_transaction("conversation_recall_manifest_lifecycle")?;
        let (_, head, _) =
            self.load_conversation_recall_manifest(&request.key, mounted_subject_id)?;
        let head = head.ok_or_else(|| {
            Error::config(
                "conversation_recall_manifest",
                "transcript lifecycle transition requires its typed recall manifest",
            )
        })?;
        self.validate_conversation_manifest_subject(&head, mounted_subject_id)?;
        let mut affected_turns = 0usize;
        let mut affected_turn_ids = Vec::new();
        let mut affected_message_ids = Vec::new();
        let mut affected_host_refs = Vec::new();
        let mut mutations = Vec::new();
        let mut pages = BTreeMap::<
            u64,
            (
                Option<ConversationTranscriptPageIndex>,
                Option<serde_json::Value>,
                Vec<RecallIndexAddress>,
            ),
        >::new();
        let mut records = self.list_turns(&request.key, mounted_subject_id, usize::MAX)?;
        let mut changed_records = Vec::<(TranscriptTurnRecord, TranscriptTurnRecord)>::new();
        let mut owner_subject_id = None::<String>;
        for record in &mut records {
            let matches_turn = request
                .turn_id
                .as_ref()
                .map(|turn_id| turn_id == &record.turn_id)
                .unwrap_or(true);
            if matches_turn {
                if owner_subject_id
                    .as_ref()
                    .is_some_and(|subject_id| subject_id != &record.subject)
                {
                    return Err(Error::config(
                        "conversation_recall_manifest",
                        "one conversation manifest cannot contain multiple subject owners",
                    ));
                }
                owner_subject_id.get_or_insert_with(|| record.subject.clone());
                let before_record = record.clone();
                affected_turn_ids.push(record.turn_id.clone());
                for message in &record.input_messages {
                    affected_message_ids.push(message.message_id.clone());
                }
                if let Some(message) = &record.assistant_message {
                    affected_message_ids.push(message.message_id.clone());
                }
                affected_host_refs.extend(record.host_refs.clone());
                record.apply_lifecycle_transition(request.transition, request.requested_at);
                changed_records.push((before_record, record.clone()));
                let page_id = Self::conversation_page_for_sequence(record.sequence)?;
                if let std::collections::btree_map::Entry::Vacant(entry) = pages.entry(page_id) {
                    let (_, page, before) = self.load_conversation_transcript_page(
                        &request.key,
                        mounted_subject_id,
                        page_id,
                    )?;
                    let entries =
                        page.as_ref()
                            .map(|page| page.entries.clone())
                            .ok_or_else(|| {
                                Error::config(
                                    "conversation_transcript_page",
                                    "lifecycle target page is missing",
                                )
                            })?;
                    entry.insert((page, before, entries));
                }
                let record_key =
                    transcript_turn_storage_key(&request.key, mounted_subject_id, &record.turn_id);
                let value = serde_json::to_value(&*record).map_err(|error| {
                    Error::config("conversation_recall_manifest", error.to_string())
                })?;
                let address = RecallIndexAddress::json(
                    "conversation_transcript",
                    &record_key,
                    next_entry_revision(
                        &pages
                            .get(&page_id)
                            .ok_or_else(|| {
                                Error::config(
                                    "conversation_transcript_page",
                                    "lifecycle page state is missing",
                                )
                            })?
                            .2,
                        RecallIndexAddressKind::Json,
                        "conversation_transcript",
                        &record_key,
                    ),
                    record.updated_at,
                    &value,
                )?;
                let page_state = pages.get_mut(&page_id).ok_or_else(|| {
                    Error::config(
                        "conversation_transcript_page",
                        "lifecycle page state is missing",
                    )
                })?;
                page_state.2 = replace_recall_index_address(&page_state.2, address);
                mutations.push(StoreMutation::PutJson {
                    namespace: "conversation_transcript".to_string(),
                    key: record_key.clone(),
                    value,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "conversation_transcript".to_string(),
                    record_key,
                });
                affected_turns = affected_turns.saturating_add(1);
            }
        }
        if !mutations.is_empty() {
            let mut indexes = Vec::with_capacity(pages.len().saturating_add(8));
            for (page_id, (page, before, entries)) in pages {
                indexes.push(self.plan_conversation_transcript_page(
                    &request.key,
                    mounted_subject_id,
                    page_id,
                    page.as_ref(),
                    before,
                    entries,
                )?);
            }
            let head_plan = self.plan_conversation_head(
                &request.key,
                mounted_subject_id,
                Some(&head),
                self.engine
                    .get_json_value(ConversationRecallManifest::NAMESPACE, &head.physical_key)?,
                head.turn_count,
                head.last_sequence,
            )?;
            let next_manifest = serde_json::from_value::<ConversationRecallManifest>(
                head_plan.2.clone(),
            )
            .map_err(|error| Error::config("conversation_transcript_head", error.to_string()))?;
            let catalog_head =
                Self::transcript_catalog_head_from_records(&next_manifest, &records)?;
            indexes.push(head_plan);
            indexes.extend(
                self.plan_transcript_query_lifecycle_indexes(&changed_records, &catalog_head)?,
            );
            let mut scope = self.recall_scope();
            scope
                .memory_space_id
                .clone_from(&request.key.memory_space_id);
            scope.subject_id = owner_subject_id.expect("affected turns have a subject owner");
            scope.channel.clone_from(&request.key.channel_id);
            scope.conversation_id = Some(request.key.conversation_id.clone());
            self.commit_recall_indexed_mutations(
                "conversation.transcript.lifecycle",
                scope,
                mutations,
                indexes,
            )?;
        }
        let mut derived_memory_refs = Vec::new();
        for turn_id in &affected_turn_ids {
            derived_memory_refs.extend(self.list_derived_memory_refs(
                &request.key,
                mounted_subject_id,
                Some(turn_id),
            )?);
        }
        Ok(TranscriptLifecycleReport {
            key: request.key.clone(),
            transition: request.transition,
            affected_turns,
            affected_turn_ids,
            affected_message_ids,
            affected_host_refs,
            redacted_host_refs: 0,
            host_ref_redactions: Vec::new(),
            derived_memory_refs,
            profile_budget_applied: false,
            reason: request.reason.clone(),
            requested_by: request.requested_by.clone(),
            requested_at: request.requested_at,
        })
    }
}

fn validate_derived_ref_matches_key(
    key: &ConversationKey,
    derived: &DerivedMemoryRef,
) -> Result<()> {
    if derived.source.memory_space_id != key.memory_space_id
        || derived.source.channel_id != key.channel_id
        || derived.source.conversation_id != key.conversation_id
    {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref source does not match conversation key",
        ));
    }
    if derived.source.turn_id.trim().is_empty() {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref turn_id must not be empty",
        ));
    }
    if derived.store_key.trim().is_empty() {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref store_key must not be empty",
        ));
    }
    Ok(())
}

pub(crate) fn validate_conversation_recall_owner_for_scope(
    namespace: &str,
    physical_key: &str,
    value: &serde_json::Value,
    memory_space_id: &str,
    mounted_subject_id: &str,
) -> Result<()> {
    let valid = match namespace {
        "conversation_transcript" => {
            let record =
                serde_json::from_value::<TranscriptTurnRecord>(value.clone()).map_err(|error| {
                    Error::config(
                        "typed_recall_manifest_closure",
                        format!("conversation turn decode failed: {error}"),
                    )
                })?;
            record.key.memory_space_id == memory_space_id
                && record.subject == mounted_subject_id
                && transcript_turn_storage_key(&record.key, mounted_subject_id, &record.turn_id)
                    == physical_key
        }
        "conversation_transcript_attr" => {
            let attr = serde_json::from_value::<TranscriptAttrEnvelope>(value.clone()).map_err(
                |error| {
                    Error::config(
                        "typed_recall_manifest_closure",
                        format!("conversation attr decode failed: {error}"),
                    )
                },
            )?;
            attr.validate()?;
            attr.target.key.memory_space_id == memory_space_id
                && transcript_attr_storage_key(&attr.target.key, mounted_subject_id, &attr)
                    == physical_key
        }
        "conversation_transcript_derived_ref" => {
            let derived =
                serde_json::from_value::<DerivedMemoryRef>(value.clone()).map_err(|error| {
                    Error::config(
                        "typed_recall_manifest_closure",
                        format!("conversation derived-ref decode failed: {error}"),
                    )
                })?;
            let key = ConversationKey::new(
                &derived.source.memory_space_id,
                &derived.source.channel_id,
                &derived.source.conversation_id,
            )?;
            validate_derived_ref_matches_key(&key, &derived)?;
            key.memory_space_id == memory_space_id
                && exact_derived_owner_subject(&derived)? == mounted_subject_id
                && transcript_derived_ref_storage_key(&key, mounted_subject_id, &derived)?
                    == physical_key
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Error::config(
            "typed_recall_manifest_closure",
            "conversation recall owner key or exact subject scope is not canonical",
        ))
    }
}

fn exact_derived_owner_subject(derived: &DerivedMemoryRef) -> Result<&str> {
    let subject_id = derived
        .subject_id
        .as_deref()
        .or(derived.source.subject_id.as_deref())
        .ok_or_else(|| {
            Error::config(
                "conversation_transcript_derived_ref",
                "derived memory ref requires an exact subject owner",
            )
        })?;
    if derived
        .subject_id
        .as_deref()
        .zip(derived.source.subject_id.as_deref())
        .is_some_and(|(owner, source_owner)| owner != source_owner)
    {
        return Err(Error::config(
            "conversation_transcript_derived_ref",
            "derived memory ref owner and source subject differ",
        ));
    }
    Ok(subject_id)
}

fn transcript_owner_storage_key_prefix(key: &ConversationKey, mounted_subject_id: &str) -> String {
    format!(
        "{}__subject__{}__",
        key.storage_key(),
        stable_hash_hex(mounted_subject_id)
    )
}

pub(crate) fn transcript_turn_storage_key(
    key: &ConversationKey,
    mounted_subject_id: &str,
    turn_id: &str,
) -> String {
    format!(
        "{}turn__{}",
        transcript_owner_storage_key_prefix(key, mounted_subject_id),
        stable_hash_hex(turn_id)
    )
}

fn transcript_derived_ref_storage_key_prefix(
    key: &ConversationKey,
    mounted_subject_id: &str,
) -> String {
    format!(
        "{}derived_ref__",
        transcript_owner_storage_key_prefix(key, mounted_subject_id)
    )
}

pub(crate) fn transcript_derived_ref_storage_key(
    key: &ConversationKey,
    mounted_subject_id: &str,
    derived: &DerivedMemoryRef,
) -> Result<String> {
    let payload = serde_json::to_string(derived)
        .map_err(|error| Error::config("conversation_transcript_derived_ref", error.to_string()))?;
    Ok(format!(
        "{}{}",
        transcript_derived_ref_storage_key_prefix(key, mounted_subject_id),
        stable_hash_hex(&payload)
    ))
}

fn transcript_attr_rejection(
    attr: &TranscriptAttrEnvelope,
    reason: impl Into<String>,
) -> TranscriptAttrWriteRejection {
    TranscriptAttrWriteRejection {
        attr_id: attr.attr_id.clone(),
        attr_key: attr.key.clone(),
        reason: reason.into(),
    }
}

fn transcript_attr_storage_key_prefix(key: &ConversationKey, mounted_subject_id: &str) -> String {
    format!(
        "{}attr__",
        transcript_owner_storage_key_prefix(key, mounted_subject_id)
    )
}

fn transcript_attr_storage_key(
    key: &ConversationKey,
    mounted_subject_id: &str,
    attr: &TranscriptAttrEnvelope,
) -> String {
    format!(
        "{}{}",
        transcript_attr_storage_key_prefix(key, mounted_subject_id),
        stable_hash_hex(&attr.attr_id)
    )
}

fn transcript_attr_repair_target_from_value(value: &serde_json::Value) -> (String, Option<String>) {
    let Some(target) = value.get("target") else {
        return ("*".to_string(), None);
    };
    let turn_id = target
        .get("turn_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("*")
        .to_string();
    let message_id = target
        .get("message_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    (turn_id, message_id)
}

impl ScopedLongTermMemoryStore {
    fn scope_manifest(&self) -> Result<Option<LongTermMemoryVersionScopeManifest>> {
        let key =
            long_term_version_scope_manifest_key(&self.memory_space_id, &self.factual_owner_id)?;
        let manifest = self
            .platform
            .json_get::<LongTermMemoryVersionScopeManifest>(
                crate::store_internal::schema::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
                &key,
            )?;
        if manifest
            .as_ref()
            .is_some_and(|manifest| manifest.physical_key != key)
        {
            return Err(Error::config(
                "long_term_storage_scope",
                "long-term scope manifest physical key drift",
            ));
        }
        Ok(manifest)
    }

    fn current_projection_for_head(
        &self,
        head: &LongTermMemoryHeadManifest,
    ) -> Result<LongTermMemoryEntry> {
        let material_key = long_term_version_material_key(
            &self.memory_space_id,
            &self.factual_owner_id,
            &head.owner_ref,
            head.current_revision,
        )?;
        let material = self
            .platform
            .json_get::<LongTermMemoryVersionMaterial>(
                crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
                &material_key,
            )?
            .ok_or_else(|| {
                Error::config(
                    "long_term_storage_scope",
                    "head current material is missing",
                )
            })?;
        let expected_digest = head
            .retained_revision_digests
            .iter()
            .find(|entry| entry.owner_revision == head.current_revision)
            .map(|entry| entry.content_digest.as_str());
        if material.owner_ref != head.owner_ref
            || material.owner_revision != head.current_revision
            || expected_digest != Some(material.content_digest.as_str())
        {
            return Err(Error::config(
                "long_term_storage_scope",
                "head current material identity or digest drift",
            ));
        }
        material.to_current_projection()
    }
}

impl bm_core::memory::LongTermMemoryReadStore for ScopedLongTermMemoryStore {
    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = bm_core::memory::LongTermMemoryReadStore::list(self, usize::MAX)?;
        let query = query.trim().to_lowercase();
        if !query.is_empty() {
            entries.retain(|entry| {
                entry.topic.to_lowercase().contains(&query)
                    || entry.content.to_lowercase().contains(&query)
                    || entry
                        .keywords
                        .iter()
                        .any(|keyword| keyword.to_lowercase().contains(&query))
            });
        }
        entries.sort_by(|left, right| {
            let left_scope = usize::from(left.source_chat_id.as_deref() == source_chat_id);
            let right_scope = usize::from(right.source_chat_id.as_deref() == source_chat_id);
            right_scope
                .cmp(&left_scope)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
        });
        if limit > 0 {
            entries.truncate(limit);
        }
        Ok(entries)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        let owner_ref =
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, id.to_string());
        let key =
            long_term_version_head_key(&self.memory_space_id, &self.factual_owner_id, &owner_ref)?;
        let Some(head) = self.platform.json_get::<LongTermMemoryHeadManifest>(
            crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
            &key,
        )?
        else {
            return Ok(None);
        };
        if head.owner_ref != owner_ref || !head.validate_contract().accepted {
            return Err(Error::config(
                "long_term_storage_scope",
                "long-term head identity or contract drift",
            ));
        }
        if head.terminal_transition_ref.is_some() {
            return Ok(None);
        }
        self.current_projection_for_head(&head).map(Some)
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let Some(manifest) = self.scope_manifest()? else {
            return Ok(Vec::new());
        };
        let mut entries = Vec::new();
        for binding in &manifest.head_bindings {
            let head = self
                .platform
                .json_get::<LongTermMemoryHeadManifest>(
                    crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
                    &binding.head_physical_key,
                )?
                .ok_or_else(|| Error::config("long_term_storage_scope", "scope head is missing"))?;
            if LongTermMemoryVersionHeadBinding::from_head(&head)? != *binding {
                return Err(Error::config(
                    "long_term_storage_scope",
                    "scope head binding drift",
                ));
            }
            if head.terminal_transition_ref.is_some() {
                continue;
            }
            entries.push(self.current_projection_for_head(&head)?);
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
        if limit > 0 {
            entries.truncate(limit);
        }
        Ok(entries)
    }

    fn count(&self) -> Result<usize> {
        Ok(bm_core::memory::LongTermMemoryReadStore::list(self, usize::MAX)?.len())
    }
}

impl ScopedLongTermMemoryControlReadStore {
    fn list_scoped<T>(&self, namespace: &str) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let manifest_key =
            control_plane_scope_manifest_key(&self.memory_space_id, &self.control_owner_id)?;
        let mut session = self
            .platform
            .engine
            .open_immutable_read_session(self.platform.capacity)?;
        let manifest_reads = session.read_json_known_keys(&[(
            CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.to_string(),
            manifest_key.clone(),
        )])?;
        let manifest_value = manifest_reads
            .into_iter()
            .next()
            .and_then(|read| read.value);
        let Some(manifest_value) = manifest_value else {
            return Ok(Vec::new());
        };
        let manifest = serde_json::from_value::<ControlPlaneScopeManifest>(manifest_value)
            .map_err(|error| {
                Error::config(
                    "control_plane_scope_manifest",
                    format!("control read manifest decode failed: {error}"),
                )
            })?;
        manifest.validate(self.platform.capacity.kv_max_entries)?;
        if manifest.physical_key != manifest_key
            || manifest.memory_space_id != self.memory_space_id
            || manifest.mounted_subject_id != self.control_owner_id
        {
            return Err(Error::config(
                "control_plane_scope_manifest",
                "control read manifest differs from the exact mounted subject scope",
            ));
        }
        let entries = manifest
            .entries
            .iter()
            .filter(|entry| entry.namespace == namespace)
            .collect::<Vec<_>>();
        let addresses = entries
            .iter()
            .map(|entry| (entry.namespace.clone(), entry.key.clone()))
            .collect::<Vec<_>>();
        let reads = session.read_json_known_keys(&addresses)?;
        if reads.len() != entries.len() {
            return Err(Error::config(
                "control_plane_scope_manifest",
                "bounded control read returned the wrong address count",
            ));
        }
        reads
            .into_iter()
            .zip(entries)
            .map(|(read, entry)| {
                if read.namespace != entry.namespace || read.key != entry.key {
                    return Err(Error::config(
                        "control_plane_scope_manifest",
                        "bounded control read returned a wrong address",
                    ));
                }
                let value = read.value.ok_or_else(|| {
                    Error::config(
                        "control_plane_scope_manifest",
                        "manifested control document is missing",
                    )
                })?;
                entry.validate_value(&value)?;
                validate_control_document_for_scope(
                    namespace,
                    &entry.key,
                    &value,
                    &self.memory_space_id,
                    &self.control_owner_id,
                )?;
                serde_json::from_value(value).map_err(|error| {
                    Error::config("control_plane_scope_manifest", error.to_string())
                })
            })
            .collect()
    }
}

impl LongTermMemoryControlReadStore for ScopedLongTermMemoryControlReadStore {
    fn list_long_term_control_revisions(
        &self,
        record_id: &str,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlRevision>> {
        let mut revisions = self
            .list_scoped::<LongTermMemoryControlRevision>(LONG_TERM_CONTROL_REVISION_NAMESPACE)?;
        revisions
            .retain(|revision| revision.transition.predecessor.owner_ref.owner_id == record_id);
        revisions.sort_by_key(|revision| std::cmp::Reverse(revision.created_at));
        revisions.truncate(limit);
        Ok(revisions)
    }

    fn get_long_term_control_tombstone(
        &self,
        record_id: &str,
    ) -> Result<Option<LongTermMemoryTombstone>> {
        Ok(self
            .list_scoped::<LongTermMemoryTombstone>(LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE)?
            .into_iter()
            .find(|tombstone| tombstone.record_id == record_id))
    }

    fn list_long_term_control_tombstones(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryTombstone>> {
        let mut tombstones =
            self.list_scoped::<LongTermMemoryTombstone>(LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE)?;
        tombstones.sort_by_key(|tombstone| std::cmp::Reverse(tombstone.created_at));
        tombstones.truncate(limit);
        Ok(tombstones)
    }

    fn list_long_term_governance_policies(
        &self,
        limit: usize,
    ) -> Result<Vec<MemoryLongTermGovernancePolicy>> {
        let mut policies = self
            .list_scoped::<MemoryLongTermGovernancePolicy>(LONG_TERM_GOVERNANCE_POLICY_NAMESPACE)?;
        policies.sort_by_key(|policy| std::cmp::Reverse(policy.updated_at));
        policies.truncate(limit);
        Ok(policies)
    }

    fn list_long_term_control_audit(
        &self,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryControlAuditEvent>> {
        let mut events =
            self.list_scoped::<LongTermMemoryControlAuditEvent>(LONG_TERM_CONTROL_AUDIT_NAMESPACE)?;
        events.sort_by_key(|event| std::cmp::Reverse(event.created_at));
        events.truncate(limit);
        Ok(events)
    }
}

impl ContinuityCapsuleStore for StorePlatform {
    fn upsert_many(
        &self,
        drafts: &[ContinuityCapsuleDraft],
        now_secs: u64,
    ) -> Result<ContinuityCapsuleWriteOutcome> {
        let _transaction_guard = self.lock_transaction("continuity_capsule_scope_index_upsert")?;
        let mut upserted = 0usize;
        for draft in drafts {
            let capsule_id = stable_hash_id(
                "cc",
                &(
                    draft.kind.label(),
                    draft.scope_kind.label(),
                    &draft.scope_id,
                    &draft.topic,
                ),
            );
            let capsule = ContinuityCapsule {
                capsule_id: capsule_id.clone(),
                kind: draft.kind,
                scope_kind: draft.scope_kind,
                scope_id: draft.scope_id.clone(),
                source_chat_id: draft.source_chat_id.clone(),
                source_channel: draft.source_channel.clone(),
                run_id: draft.run_id.clone(),
                topic: draft.topic.clone(),
                summary: draft.summary.clone(),
                outcome: draft.outcome.clone(),
                decisions: draft.decisions.clone(),
                next_step: draft.next_step.clone(),
                unresolved: draft.unresolved.clone(),
                artifact_refs: draft.artifact_refs.clone(),
                provenance_refs: draft.provenance_refs.clone(),
                source: draft.source,
                status: draft.status,
                supersedes: Vec::new(),
                observed_at: if draft.observed_at > 0 {
                    draft.observed_at
                } else {
                    now_secs
                },
                updated_at: now_secs,
            };
            if capsule.is_meaningful() {
                let scope_kind = capsule.scope_kind.label();
                let manifest_key = ContinuityCapsuleScopeIndex::build(
                    1,
                    &self.config.event_scope.memory_space_id,
                    scope_kind,
                    &capsule.scope_id,
                    std::iter::empty(),
                )?
                .physical_key;
                let (previous, before) =
                    self.load_typed_recall_index::<ContinuityCapsuleScopeIndex>(&manifest_key)?;
                if self
                    .engine
                    .get_json_value("continuity_capsule", &capsule_id)?
                    .is_some()
                    && previous.is_none()
                {
                    return Err(Error::config(
                        "continuity_capsule_scope_index",
                        "capsule exists without its required scope index",
                    ));
                }
                let previous_entries = previous
                    .as_ref()
                    .map(|index| index.entries.as_slice())
                    .unwrap_or(&[]);
                let value = serde_json::to_value(&capsule).map_err(|error| {
                    Error::config("continuity_capsule_scope_index", error.to_string())
                })?;
                let address = RecallIndexAddress::json(
                    "continuity_capsule",
                    &capsule_id,
                    next_entry_revision(
                        previous_entries,
                        RecallIndexAddressKind::Json,
                        "continuity_capsule",
                        &capsule_id,
                    ),
                    capsule.updated_at,
                    &value,
                )?;
                let next = ContinuityCapsuleScopeIndex::build(
                    previous
                        .as_ref()
                        .map(|index| index.revision.saturating_add(1))
                        .unwrap_or(1),
                    &self.config.event_scope.memory_space_id,
                    scope_kind,
                    &capsule.scope_id,
                    replace_recall_index_address(previous_entries, address),
                )?;
                let index = (
                    ContinuityCapsuleScopeIndex::NAMESPACE,
                    manifest_key,
                    serde_json::to_value(next).map_err(|error| {
                        Error::config("continuity_capsule_scope_index", error.to_string())
                    })?,
                    before,
                );
                let mut event_scope = self.recall_scope();
                event_scope.channel.clone_from(&capsule.source_channel);
                event_scope.chat_id.clone_from(&capsule.source_chat_id);
                self.commit_recall_indexed_mutations(
                    "continuity_capsule.upsert",
                    event_scope,
                    vec![StoreMutation::PutJson {
                        namespace: "continuity_capsule".to_string(),
                        key: capsule_id.clone(),
                        value,
                        event_kind: MemoryStoreEventKind::MemoryWrite,
                        plane: "continuity_capsule".to_string(),
                        record_key: capsule_id.clone(),
                    }],
                    vec![index],
                )?;
                upserted = upserted.saturating_add(1);
            }
        }
        Ok(ContinuityCapsuleWriteOutcome {
            considered: drafts.len(),
            upserted,
            superseded: 0,
            total: ContinuityCapsuleStore::count(self)?,
        })
    }

    fn get(&self, capsule_id: &str) -> Result<Option<ContinuityCapsule>> {
        self.json_get("continuity_capsule", capsule_id)
    }

    fn list(&self, limit: usize) -> Result<Vec<ContinuityCapsule>> {
        let mut items = self.json_list::<ContinuityCapsule>("continuity_capsule", limit)?;
        items.sort_by_key(|item| std::cmp::Reverse(item.updated_at));
        items.truncate(limit);
        Ok(items)
    }

    fn count(&self) -> Result<usize> {
        self.engine
            .list_json_keys("continuity_capsule")
            .map(|keys| keys.len())
    }
}

impl TurnContinuityEvidenceStore for StorePlatform {
    fn append(&self, chat_id: &str, evidence: &TurnContinuityEvidence) -> Result<()> {
        let mut items = self
            .json_get::<Vec<TurnContinuityEvidence>>("turn_continuity_evidence", chat_id)?
            .unwrap_or_default();
        items.push(evidence.clone());
        if items.len() > TURN_CONTINUITY_EVIDENCE_HISTORY_MAX_ITEMS {
            let remove_count = items.len() - TURN_CONTINUITY_EVIDENCE_HISTORY_MAX_ITEMS;
            items.drain(0..remove_count);
        }
        self.json_put("turn_continuity_evidence", chat_id, &items)
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.json_delete("turn_continuity_evidence", chat_id)
            .map(|_| ())
    }

    fn list_recent(&self, chat_id: &str, limit: usize) -> Result<Vec<TurnContinuityEvidence>> {
        let mut items = self
            .json_get::<Vec<TurnContinuityEvidence>>("turn_continuity_evidence", chat_id)?
            .unwrap_or_default();
        items.reverse();
        items.truncate(limit);
        Ok(items)
    }
}

impl PrivateGardenStore for StorePlatform {
    fn list(&self, mounted_subject_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
        let prefix = private_garden_key_prefix(mounted_subject_id);
        let mut docs = Vec::new();
        for key in self.engine.list_json_keys("private_garden")? {
            if !key.starts_with(&prefix) {
                continue;
            }
            if let Some(doc) = self.json_get::<PrivateGardenDoc>("private_garden", &key)? {
                docs.push(private_garden_record(&doc));
            }
        }
        docs.sort_by_key(|doc| std::cmp::Reverse(doc.updated_at));
        docs.truncate(limit);
        Ok(docs)
    }

    fn read(&self, mounted_subject_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
        let path = normalize_private_garden_doc_path(doc_path)?;
        self.json_get(
            "private_garden",
            &private_garden_key(mounted_subject_id, &path),
        )
    }

    fn write(
        &self,
        mounted_subject_id: &str,
        doc_path: &str,
        content: &str,
        now_secs: u64,
    ) -> Result<PrivateGardenDocRecord> {
        if content.len() > PRIVATE_GARDEN_MAX_DOC_BYTES {
            return Err(Error::config(
                "private_garden_write",
                format!(
                    "document size {} exceeds {}",
                    content.len(),
                    PRIVATE_GARDEN_MAX_DOC_BYTES
                ),
            ));
        }
        let path = normalize_private_garden_doc_path(doc_path)?;
        let key = private_garden_key(mounted_subject_id, &path);
        let revision = self
            .json_get::<PrivateGardenDoc>("private_garden", &key)?
            .map(|doc| doc.revision.saturating_add(1))
            .unwrap_or(1);
        let doc = PrivateGardenDoc {
            path,
            content: content.to_string(),
            updated_at: now_secs,
            revision,
        };
        let record = private_garden_record(&doc);
        self.json_put("private_garden", &key, &doc)?;
        Ok(record)
    }

    fn move_doc(
        &self,
        mounted_subject_id: &str,
        from_path: &str,
        to_path: &str,
        now_secs: u64,
    ) -> Result<Option<PrivateGardenDocRecord>> {
        let from = normalize_private_garden_doc_path(from_path)?;
        let to = normalize_private_garden_doc_path(to_path)?;
        let Some(doc) = PrivateGardenStore::read(self, mounted_subject_id, &from)? else {
            return Ok(None);
        };
        PrivateGardenStore::delete(self, mounted_subject_id, &from)?;
        Ok(Some(PrivateGardenStore::write(
            self,
            mounted_subject_id,
            &to,
            &doc.content,
            now_secs,
        )?))
    }

    fn delete(&self, mounted_subject_id: &str, doc_path: &str) -> Result<bool> {
        let path = normalize_private_garden_doc_path(doc_path)?;
        self.json_delete(
            "private_garden",
            &private_garden_key(mounted_subject_id, &path),
        )
    }
}

impl RemindAtStore for StorePlatform {
    fn get(
        &self,
        channel: &str,
        chat_id: &str,
        id: &str,
    ) -> Result<Option<bm_core::reminder::ReminderItem>> {
        self.json_get("remind_at", &triple_key(channel, chat_id, id))
    }

    fn upsert(&self, reminder: &bm_core::reminder::ReminderItem) -> Result<()> {
        self.json_put(
            "remind_at",
            &triple_key(&reminder.channel, &reminder.chat_id, &reminder.id),
            reminder,
        )
    }

    fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool> {
        self.json_delete("remind_at", &triple_key(channel, chat_id, id))
    }

    fn list_due(
        &self,
        now_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<bm_core::reminder::ReminderItem>> {
        let mut items =
            self.json_list::<bm_core::reminder::ReminderItem>("remind_at", usize::MAX)?;
        items.retain(|item| item.at_unix_secs > 0 && item.at_unix_secs <= now_unix_secs);
        items.sort_by_key(|item| item.at_unix_secs);
        items.truncate(limit);
        Ok(items)
    }

    fn delete_due(&self, reminder: &bm_core::reminder::ReminderItem) -> Result<bool> {
        self.json_delete(
            "remind_at",
            &triple_key(&reminder.channel, &reminder.chat_id, &reminder.id),
        )
    }

    fn next_due_at(&self) -> Result<Option<u64>> {
        let mut due_times = self
            .json_list::<bm_core::reminder::ReminderItem>("remind_at", usize::MAX)?
            .into_iter()
            .filter_map(|item| (item.at_unix_secs > 0).then_some(item.at_unix_secs))
            .collect::<Vec<_>>();
        due_times.sort_unstable();
        Ok(due_times.into_iter().next())
    }

    fn list_upcoming(
        &self,
        channel: &str,
        chat_id: &str,
        now_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<bm_core::reminder::ReminderItem>> {
        let mut items =
            self.json_list::<bm_core::reminder::ReminderItem>("remind_at", usize::MAX)?;
        items.retain(|item| {
            item.channel == channel && item.chat_id == chat_id && item.at_unix_secs > now_unix_secs
        });
        items.sort_by_key(|item| item.at_unix_secs);
        items.truncate(limit);
        Ok(items)
    }
}

impl TaskStore for StorePlatform {
    fn list(&self, channel: &str, chat_id: &str, query: TaskQuery) -> Result<Vec<TaskItem>> {
        let mut items = self.json_list::<TaskItem>("task", usize::MAX)?;
        items.retain(|task| {
            task.channel == channel
                && task.chat_id == chat_id
                && query.status.is_none_or(|status| task.status == status)
                && (query.include_completed || !task.status.is_terminal())
                && (query.project.trim().is_empty() || task.project == query.project)
        });
        items.sort_by_key(|item| std::cmp::Reverse(item.updated_at));
        items.truncate(query.limit);
        Ok(items)
    }

    fn get(&self, channel: &str, chat_id: &str, id: &str) -> Result<Option<TaskItem>> {
        self.json_get("task", &triple_key(channel, chat_id, id))
    }

    fn upsert(&self, task: &TaskItem) -> Result<()> {
        let task = normalize_task_item(task.clone())?;
        self.json_put(
            "task",
            &triple_key(&task.channel, &task.chat_id, &task.id),
            &task,
        )
    }

    fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool> {
        self.json_delete("task", &triple_key(channel, chat_id, id))
    }

    fn list_due_unnotified(&self, now_unix_secs: u64, limit: usize) -> Result<Vec<TaskItem>> {
        let mut items = self.json_list::<TaskItem>("task", usize::MAX)?;
        items.retain(|task| {
            task.due_at_unix_secs > 0
                && task.due_at_unix_secs <= now_unix_secs
                && task.due_notified_at_unix_secs == 0
                && !task.status.is_terminal()
        });
        items.sort_by_key(|item| item.due_at_unix_secs);
        items.truncate(limit);
        Ok(items)
    }

    fn mark_due_notified(&self, task: &TaskItem, notified_at_unix_secs: u64) -> Result<bool> {
        let key = triple_key(&task.channel, &task.chat_id, &task.id);
        let Some(mut latest) = self.json_get::<TaskItem>("task", &key)? else {
            return Ok(false);
        };
        latest.due_notified_at_unix_secs = notified_at_unix_secs;
        self.json_put("task", &key, &latest)?;
        Ok(true)
    }

    fn next_due_at(&self) -> Result<Option<u64>> {
        let mut values = self
            .json_list::<TaskItem>("task", usize::MAX)?
            .into_iter()
            .filter_map(|task| {
                (task.due_at_unix_secs > 0
                    && task.due_notified_at_unix_secs == 0
                    && !task.status.is_terminal())
                .then_some(task.due_at_unix_secs)
            })
            .collect::<Vec<_>>();
        values.sort_unstable();
        Ok(values.into_iter().next())
    }
}

impl TaskRunStore for StorePlatform {
    fn get(&self, run_id: &str) -> Result<Option<TaskRunRecord>> {
        self.json_get("task_run", run_id)
    }

    fn upsert(&self, record: &TaskRunRecord) -> Result<()> {
        let _transaction_guard = self.lock_transaction("active_task_run_by_chat_index_upsert")?;
        let owner_key = record.run.run_id.clone();
        let previous_record = self.json_get::<TaskRunRecord>("task_run", &owner_key)?;
        let value = serde_json::to_value(record)
            .map_err(|error| Error::config("active_task_run_by_chat_index", error.to_string()))?;
        let mut indexes = Vec::new();
        if let Some(previous_record) = previous_record.as_ref().filter(|previous| {
            previous.run.source_channel != record.run.source_channel
                || previous.run.source_chat_id != record.run.source_chat_id
        }) {
            let old_key = ActiveTaskRunByChatIndex::build(
                1,
                &self.config.event_scope.memory_space_id,
                &previous_record.run.source_channel,
                &previous_record.run.source_chat_id,
                std::iter::empty(),
            )?
            .physical_key;
            let (old, before) =
                self.load_typed_recall_index::<ActiveTaskRunByChatIndex>(&old_key)?;
            let old = old.ok_or_else(|| {
                Error::config(
                    "active_task_run_by_chat_index",
                    "task run exists without its prior chat index",
                )
            })?;
            let next = ActiveTaskRunByChatIndex::build(
                old.revision.saturating_add(1),
                &self.config.event_scope.memory_space_id,
                &previous_record.run.source_channel,
                &previous_record.run.source_chat_id,
                remove_recall_index_address(
                    &old.entries,
                    RecallIndexAddressKind::Json,
                    "task_run",
                    &owner_key,
                ),
            )?;
            indexes.push((
                ActiveTaskRunByChatIndex::NAMESPACE,
                old_key,
                serde_json::to_value(next).map_err(|error| {
                    Error::config("active_task_run_by_chat_index", error.to_string())
                })?,
                before,
            ));
        }
        let new_key = ActiveTaskRunByChatIndex::build(
            1,
            &self.config.event_scope.memory_space_id,
            &record.run.source_channel,
            &record.run.source_chat_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (current, before) =
            self.load_typed_recall_index::<ActiveTaskRunByChatIndex>(&new_key)?;
        if previous_record.is_some() && indexes.is_empty() && current.is_none() {
            return Err(Error::config(
                "active_task_run_by_chat_index",
                "task run exists without its required chat index",
            ));
        }
        let current_entries = current
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let mut entries = remove_recall_index_address(
            current_entries,
            RecallIndexAddressKind::Json,
            "task_run",
            &owner_key,
        );
        if record.run.status.is_active() {
            let address = RecallIndexAddress::json(
                "task_run",
                &owner_key,
                next_entry_revision(
                    current_entries,
                    RecallIndexAddressKind::Json,
                    "task_run",
                    &owner_key,
                ),
                record.run.updated_at,
                &value,
            )?;
            entries = replace_recall_index_address(&entries, address);
        }
        let next = ActiveTaskRunByChatIndex::build(
            current
                .as_ref()
                .map(|index| index.revision.saturating_add(1))
                .unwrap_or(1),
            &self.config.event_scope.memory_space_id,
            &record.run.source_channel,
            &record.run.source_chat_id,
            entries,
        )?;
        indexes.push((
            ActiveTaskRunByChatIndex::NAMESPACE,
            new_key,
            serde_json::to_value(next).map_err(|error| {
                Error::config("active_task_run_by_chat_index", error.to_string())
            })?,
            before,
        ));
        let mut scope = self.recall_scope();
        scope.channel.clone_from(&record.run.source_channel);
        scope.chat_id.clone_from(&record.run.source_chat_id);
        self.commit_recall_indexed_mutations(
            "task_run.upsert",
            scope,
            vec![StoreMutation::PutJson {
                namespace: "task_run".to_string(),
                key: owner_key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "task_run".to_string(),
                record_key: owner_key,
            }],
            indexes,
        )?;
        Ok(())
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskRunRecord>> {
        let mut records = self.json_list::<TaskRunRecord>("task_run", usize::MAX)?;
        records.sort_by_key(|record| std::cmp::Reverse(record.run.updated_at));
        records.truncate(limit);
        Ok(records)
    }

    fn list_active_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskRunRecord>> {
        let mut records = self.json_list::<TaskRunRecord>("task_run", usize::MAX)?;
        records.retain(|record| {
            record.run.source_channel == channel
                && record.run.source_chat_id == chat_id
                && record.run.status.is_active()
        });
        records.sort_by_key(|record| std::cmp::Reverse(record.run.updated_at));
        records.truncate(limit);
        Ok(records)
    }
}

impl TaskArtifactStore for StorePlatform {
    fn put(&self, record: &TaskArtifactRecord) -> Result<()> {
        self.json_put(
            "task_artifact",
            &triple_key("", &record.artifact.run_id, &record.artifact.artifact_id),
            record,
        )
    }

    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskArtifactRecord>> {
        let mut records = self.json_list::<TaskArtifactRecord>("task_artifact", usize::MAX)?;
        records.retain(|record| record.artifact.run_id == run_id);
        records.sort_by_key(|record| std::cmp::Reverse(record.artifact.created_at));
        records.truncate(limit);
        Ok(records)
    }

    fn delete(&self, run_id: &str, artifact_id: &str) -> Result<bool> {
        self.json_delete("task_artifact", &triple_key("", run_id, artifact_id))
    }
}

impl TaskLearningStore for StorePlatform {
    fn get(&self, learning_id: &str) -> Result<Option<TaskLearningRecord>> {
        self.json_get("task_learning", learning_id)
    }

    fn upsert(&self, record: &TaskLearningRecord) -> Result<()> {
        let _transaction_guard = self.lock_transaction("task_learning_by_chat_index_upsert")?;
        let owner_key = record.learning_id.clone();
        let previous_record = self.json_get::<TaskLearningRecord>("task_learning", &owner_key)?;
        let value = serde_json::to_value(record)
            .map_err(|error| Error::config("task_learning_by_chat_index", error.to_string()))?;
        let mut indexes = Vec::new();
        if let Some(previous_record) = previous_record.as_ref().filter(|previous| {
            previous.source_channel != record.source_channel
                || previous.source_chat_id != record.source_chat_id
        }) {
            let old_key = TaskLearningByChatIndex::build(
                1,
                &self.config.event_scope.memory_space_id,
                &previous_record.source_channel,
                &previous_record.source_chat_id,
                std::iter::empty(),
            )?
            .physical_key;
            let (old, before) =
                self.load_typed_recall_index::<TaskLearningByChatIndex>(&old_key)?;
            let old = old.ok_or_else(|| {
                Error::config(
                    "task_learning_by_chat_index",
                    "task learning exists without its prior chat index",
                )
            })?;
            let next = TaskLearningByChatIndex::build(
                old.revision.saturating_add(1),
                &self.config.event_scope.memory_space_id,
                &previous_record.source_channel,
                &previous_record.source_chat_id,
                remove_recall_index_address(
                    &old.entries,
                    RecallIndexAddressKind::Json,
                    "task_learning",
                    &owner_key,
                ),
            )?;
            indexes.push((
                TaskLearningByChatIndex::NAMESPACE,
                old_key,
                serde_json::to_value(next).map_err(|error| {
                    Error::config("task_learning_by_chat_index", error.to_string())
                })?,
                before,
            ));
        }
        let new_key = TaskLearningByChatIndex::build(
            1,
            &self.config.event_scope.memory_space_id,
            &record.source_channel,
            &record.source_chat_id,
            std::iter::empty(),
        )?
        .physical_key;
        let (current, before) =
            self.load_typed_recall_index::<TaskLearningByChatIndex>(&new_key)?;
        if previous_record.is_some() && indexes.is_empty() && current.is_none() {
            return Err(Error::config(
                "task_learning_by_chat_index",
                "task learning exists without its required chat index",
            ));
        }
        let current_entries = current
            .as_ref()
            .map(|index| index.entries.as_slice())
            .unwrap_or(&[]);
        let address = RecallIndexAddress::json(
            "task_learning",
            &owner_key,
            next_entry_revision(
                current_entries,
                RecallIndexAddressKind::Json,
                "task_learning",
                &owner_key,
            ),
            record.observed_at,
            &value,
        )?;
        let next = TaskLearningByChatIndex::build(
            current
                .as_ref()
                .map(|index| index.revision.saturating_add(1))
                .unwrap_or(1),
            &self.config.event_scope.memory_space_id,
            &record.source_channel,
            &record.source_chat_id,
            replace_recall_index_address(current_entries, address),
        )?;
        indexes.push((
            TaskLearningByChatIndex::NAMESPACE,
            new_key,
            serde_json::to_value(next)
                .map_err(|error| Error::config("task_learning_by_chat_index", error.to_string()))?,
            before,
        ));
        let mut scope = self.recall_scope();
        scope.channel.clone_from(&record.source_channel);
        scope.chat_id.clone_from(&record.source_chat_id);
        self.commit_recall_indexed_mutations(
            "task_learning.upsert",
            scope,
            vec![StoreMutation::PutJson {
                namespace: "task_learning".to_string(),
                key: owner_key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "task_learning".to_string(),
                record_key: owner_key,
            }],
            indexes,
        )?;
        Ok(())
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        let mut records = self.json_list::<TaskLearningRecord>("task_learning", usize::MAX)?;
        records.sort_by_key(|record| std::cmp::Reverse(record.observed_at));
        records.truncate(limit);
        Ok(records)
    }

    fn list_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskLearningRecord>> {
        let mut records = self.json_list::<TaskLearningRecord>("task_learning", usize::MAX)?;
        records
            .retain(|record| record.source_channel == channel && record.source_chat_id == chat_id);
        records.sort_by_key(|record| std::cmp::Reverse(record.observed_at));
        records.truncate(limit);
        Ok(records)
    }

    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        let mut records = self.json_list::<TaskLearningRecord>("task_learning", usize::MAX)?;
        records.retain(|record| record.run_id == run_id);
        records.sort_by_key(|record| std::cmp::Reverse(record.observed_at));
        records.truncate(limit);
        Ok(records)
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn canonical_transaction_timestamp(
    batch: &StoreMutationBatch,
    runtime_timestamp_unix_secs: Option<u64>,
) -> Result<u64> {
    let event_timestamp = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::AppendEvent { event } => Some(event.timestamp_unix_secs),
            _ => None,
        })
        .try_fold(None, |observed, timestamp| match observed {
            Some(existing) if existing != timestamp => Err(Error::config(
                "memory_write_transaction_timestamp",
                "transaction events do not share one canonical runtime timestamp",
            )),
            Some(existing) => Ok(Some(existing)),
            None => Ok(Some(timestamp)),
        })?;
    match (runtime_timestamp_unix_secs, event_timestamp) {
        (Some(runtime), Some(event)) if runtime != event => Err(Error::config(
            "memory_write_transaction_timestamp",
            "explicit runtime clock does not match the transaction event timestamp",
        )),
        (Some(runtime), _) => Ok(runtime),
        (None, Some(event)) => Ok(event),
        (None, None) => Ok(current_unix_secs()),
    }
}

fn current_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(feature = "sqlite-store")]
fn sqlite_engine(
    config: &StoreBackendConfig,
    capacity: StoreCapacityBudget,
    admission_authority: StoreAdmissionAuthority,
    open_preflight: &StoreOpenPreflight,
) -> Result<(Arc<dyn StoreEngine>, StoreSchemaManifest)> {
    let (engine, manifest) = SqliteStoreEngine::open_with_capacity_and_authority(
        config,
        capacity,
        admission_authority,
        open_preflight,
    )?;
    Ok((Arc::new(engine), manifest))
}

#[cfg(not(feature = "sqlite-store"))]
fn sqlite_engine(
    _config: &StoreBackendConfig,
    _capacity: StoreCapacityBudget,
    _admission_authority: StoreAdmissionAuthority,
    _open_preflight: &StoreOpenPreflight,
) -> Result<(Arc<dyn StoreEngine>, StoreSchemaManifest)> {
    Err(Error::config(
        "store_platform_open",
        "sqlite store backend requires sqlite-store feature",
    ))
}

fn next_event_id() -> String {
    let mut counter = EVENT_SEQUENCE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let sequence = *counter;
    *counter = counter.wrapping_add(1);
    drop(counter);
    format!(
        "evt_{:032x}_{:08x}_{sequence:016x}",
        current_unix_nanos(),
        std::process::id()
    )
}

fn stable_hash_json(value: &serde_json::Value) -> Result<String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| Error::config("store_json_hash", error.to_string()))?;
    Ok(stable_hash_hex(bytes.as_slice()))
}

fn stable_hash_hex<T: Hash + ?Sized>(value: &T) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn stable_hash_id<T: Hash>(prefix: &str, value: &T) -> String {
    format!("{prefix}_{}", stable_hash_hex(value))
}

fn validate_transcript_query_engine_open_closure(engine: &dyn StoreEngine) -> Result<()> {
    let namespaces = [
        "conversation_transcript",
        CONVERSATION_RECALL_MANIFEST_NAMESPACE,
        TRANSCRIPT_CATALOG_ROOT_NAMESPACE,
        TRANSCRIPT_CATALOG_PAGE_NAMESPACE,
        TRANSCRIPT_TIME_ROOT_NAMESPACE,
        TRANSCRIPT_TIME_POSTING_NAMESPACE,
        TRANSCRIPT_SEARCH_ROOT_NAMESPACE,
        TRANSCRIPT_SEARCH_POSTING_NAMESPACE,
        TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE,
        TRANSCRIPT_QUERY_KEYRING_NAMESPACE,
    ];
    let mut json = BTreeMap::new();
    for namespace in namespaces {
        for key in engine.list_json_keys(namespace)? {
            let value = engine.get_json_value(namespace, &key)?.ok_or_else(|| {
                Error::config(
                    "conversation_transcript_query_open_closure",
                    "listed query owner disappeared during open closure read",
                )
            })?;
            json.insert((namespace.to_string(), key), value);
        }
    }
    validate_transcript_query_snapshot_closure(&json)
}

pub(crate) fn migrate_v10_snapshot_to_v11(
    snapshot: &StoreSnapshot,
    now_secs: u64,
) -> Result<StoreSnapshot> {
    const V10_SCHEMA_ID: &str = "beetle_memory_store_schema_v10";
    if snapshot.schema_id != V10_SCHEMA_ID
        || snapshot.schema_manifest.schema_id != V10_SCHEMA_ID
        || snapshot.schema_manifest.schema_version != 10
    {
        return Err(Error::config(
            "conversation_transcript_query_migration",
            "only an exact Store v10 snapshot may migrate to v11",
        ));
    }
    let query_namespaces = [
        TRANSCRIPT_CATALOG_ROOT_NAMESPACE,
        TRANSCRIPT_CATALOG_PAGE_NAMESPACE,
        TRANSCRIPT_TIME_ROOT_NAMESPACE,
        TRANSCRIPT_TIME_POSTING_NAMESPACE,
        TRANSCRIPT_SEARCH_ROOT_NAMESPACE,
        TRANSCRIPT_SEARCH_POSTING_NAMESPACE,
        TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE,
        TRANSCRIPT_QUERY_KEYRING_NAMESPACE,
    ];
    if snapshot
        .json_docs
        .iter()
        .any(|doc| query_namespaces.contains(&doc.namespace.as_str()))
    {
        return Err(Error::config(
            "conversation_transcript_query_migration",
            "v10 migration source contains partial or foreign v11 query state",
        ));
    }
    let mut manifests =
        BTreeMap::<(String, String, String, String), ConversationRecallManifest>::new();
    let mut records =
        BTreeMap::<(String, String, String, String), Vec<TranscriptTurnRecord>>::new();
    for doc in &snapshot.json_docs {
        if doc.namespace == CONVERSATION_RECALL_MANIFEST_NAMESPACE {
            let manifest = serde_json::from_value::<ConversationRecallManifest>(doc.value.clone())
                .map_err(|error| {
                    Error::config("conversation_transcript_query_migration", error.to_string())
                })?;
            manifests.insert(
                (
                    manifest.memory_space_id.clone(),
                    manifest.mounted_subject_id.clone(),
                    manifest.channel_id.clone(),
                    manifest.conversation_id.clone(),
                ),
                manifest,
            );
        } else if doc.namespace == "conversation_transcript" {
            let record = serde_json::from_value::<TranscriptTurnRecord>(doc.value.clone())
                .map_err(|error| {
                    Error::config("conversation_transcript_query_migration", error.to_string())
                })?;
            records
                .entry((
                    record.key.memory_space_id.clone(),
                    record.subject.clone(),
                    record.key.channel_id.clone(),
                    record.key.conversation_id.clone(),
                ))
                .or_default()
                .push(record);
        }
    }
    if manifests.len() != records.len() {
        return Err(Error::config(
            "conversation_transcript_query_migration",
            "v10 transcript heads and owner groups are not a closed set",
        ));
    }

    let mut catalog_groups =
        BTreeMap::<(String, String, Option<String>), Vec<ConversationCatalogHead>>::new();
    let mut time_groups =
        BTreeMap::<(String, String, String, String, u64), Vec<TranscriptPostingLocatorV1>>::new();
    let mut search_groups =
        BTreeMap::<(String, String, String), Vec<TranscriptPostingLocatorV1>>::new();
    let mut message_manifests = Vec::<(String, TranscriptMessageSearchManifestV1)>::new();
    let mut memory_spaces = BTreeSet::new();
    for (identity, owner_records) in &mut records {
        owner_records.sort_by_key(|record| record.sequence);
        let manifest = manifests.get(identity).ok_or_else(|| {
            Error::config(
                "conversation_transcript_query_migration",
                "v10 transcript owner group has no exact head",
            )
        })?;
        let head = StorePlatform::transcript_catalog_head_from_records(manifest, owner_records)?;
        memory_spaces.insert(head.key.memory_space_id.clone());
        catalog_groups
            .entry((
                head.key.memory_space_id.clone(),
                head.mounted_subject_id.clone(),
                None,
            ))
            .or_default()
            .push(head.clone());
        catalog_groups
            .entry((
                head.key.memory_space_id.clone(),
                head.mounted_subject_id.clone(),
                Some(head.key.channel_id.clone()),
            ))
            .or_default()
            .push(head);
        for record in owner_records {
            for (locator, content) in message_locators(record) {
                let posting = TranscriptPostingLocatorV1 {
                    locator: locator.clone(),
                    lifecycle_state: record.lifecycle_state,
                    redaction_state: record.redaction_state,
                };
                time_groups
                    .entry((
                        record.key.memory_space_id.clone(),
                        record.subject.clone(),
                        record.key.channel_id.clone(),
                        record.key.conversation_id.clone(),
                        locator.observed_at / 86_400,
                    ))
                    .or_default()
                    .push(posting.clone());
                let mut digests = TranscriptSearchNormalizerV1::index_terms(
                    content,
                    MAX_TRANSCRIPT_INDEX_TERMS_PER_MESSAGE,
                )?
                .iter()
                .map(|term| term_digest(term))
                .collect::<Vec<_>>();
                digests.sort();
                digests.dedup();
                for digest in &digests {
                    search_groups
                        .entry((
                            record.key.memory_space_id.clone(),
                            record.subject.clone(),
                            digest.clone(),
                        ))
                        .or_default()
                        .push(posting.clone());
                }
                message_manifests.push((
                    search_message_manifest_key(&locator),
                    TranscriptMessageSearchManifestV1 {
                        locator,
                        term_set_digest: term_set_digest(&digests),
                    },
                ));
            }
        }
    }

    let mut docs = snapshot.json_docs.clone();
    let mut push_doc = |namespace: &str, key: String, value: serde_json::Value| {
        docs.push(StoreSnapshotJsonDoc {
            namespace: namespace.to_string(),
            key,
            value,
        });
    };
    for ((space, subject, channel), heads) in &mut catalog_groups {
        heads.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.key.conversation_id.cmp(&left.key.conversation_id))
        });
        let page_count =
            u64::try_from(heads.len().div_ceil(TRANSCRIPT_QUERY_PAGE_CAPACITY)).unwrap_or(u64::MAX);
        push_doc(
            TRANSCRIPT_CATALOG_ROOT_NAMESPACE,
            catalog_root_key(space, subject, channel.as_deref()),
            serde_json::to_value(TranscriptCatalogRootV1 {
                schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                memory_space_id: space.clone(),
                mounted_subject_id: subject.clone(),
                channel_id: channel.clone(),
                revision: 1,
                page_count,
                entry_count: u64::try_from(heads.len()).unwrap_or(u64::MAX),
            })
            .map_err(|error| {
                Error::config("conversation_transcript_query_migration", error.to_string())
            })?,
        );
        for (page_id, chunk) in heads.chunks(TRANSCRIPT_QUERY_PAGE_CAPACITY).enumerate() {
            let page_id = u64::try_from(page_id).unwrap_or(u64::MAX);
            push_doc(
                TRANSCRIPT_CATALOG_PAGE_NAMESPACE,
                catalog_page_key(space, subject, channel.as_deref(), page_id),
                serde_json::to_value(TranscriptCatalogPageV1 {
                    schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                    memory_space_id: space.clone(),
                    mounted_subject_id: subject.clone(),
                    channel_id: channel.clone(),
                    page_id,
                    revision: 1,
                    heads: chunk.to_vec(),
                })
                .map_err(|error| {
                    Error::config("conversation_transcript_query_migration", error.to_string())
                })?,
            );
        }
    }
    for ((space, subject, channel, conversation, day), locators) in &mut time_groups {
        locators.sort_by_key(|entry| {
            (
                entry.locator.observed_at,
                entry.locator.turn_sequence,
                entry.locator.message_id.clone(),
            )
        });
        let key = ConversationKey::new(space.clone(), channel.clone(), conversation.clone())?;
        let page_count = u64::try_from(locators.len().div_ceil(TRANSCRIPT_QUERY_PAGE_CAPACITY))
            .unwrap_or(u64::MAX);
        push_doc(
            TRANSCRIPT_TIME_ROOT_NAMESPACE,
            time_root_key(&key, subject, *day),
            serde_json::to_value(TranscriptTimePostingRootV1 {
                schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                key: key.clone(),
                mounted_subject_id: subject.clone(),
                utc_day: *day,
                revision: 1,
                page_count,
                entry_count: u64::try_from(locators.len()).unwrap_or(u64::MAX),
            })
            .map_err(|error| {
                Error::config("conversation_transcript_query_migration", error.to_string())
            })?,
        );
        for (page_id, chunk) in locators.chunks(TRANSCRIPT_QUERY_PAGE_CAPACITY).enumerate() {
            let page_id = u64::try_from(page_id).unwrap_or(u64::MAX);
            push_doc(
                TRANSCRIPT_TIME_POSTING_NAMESPACE,
                time_posting_key(&key, subject, *day, page_id),
                serde_json::to_value(TranscriptTimePostingPageV1 {
                    schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                    key: key.clone(),
                    mounted_subject_id: subject.clone(),
                    utc_day: *day,
                    page_id,
                    revision: 1,
                    locators: chunk.to_vec(),
                })
                .map_err(|error| {
                    Error::config("conversation_transcript_query_migration", error.to_string())
                })?,
            );
        }
    }
    for ((space, subject, digest), locators) in &mut search_groups {
        locators.sort_by_key(|entry| {
            (
                entry.locator.observed_at,
                entry.locator.turn_sequence,
                entry.locator.message_id.clone(),
            )
        });
        let page_count = u64::try_from(locators.len().div_ceil(TRANSCRIPT_QUERY_PAGE_CAPACITY))
            .unwrap_or(u64::MAX);
        push_doc(
            TRANSCRIPT_SEARCH_ROOT_NAMESPACE,
            search_root_key(space, subject, digest),
            serde_json::to_value(TranscriptSearchPostingRootV1 {
                schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                memory_space_id: space.clone(),
                mounted_subject_id: subject.clone(),
                term_digest: digest.clone(),
                revision: 1,
                page_count,
                entry_count: u64::try_from(locators.len()).unwrap_or(u64::MAX),
            })
            .map_err(|error| {
                Error::config("conversation_transcript_query_migration", error.to_string())
            })?,
        );
        for (page_id, chunk) in locators.chunks(TRANSCRIPT_QUERY_PAGE_CAPACITY).enumerate() {
            let page_id = u64::try_from(page_id).unwrap_or(u64::MAX);
            push_doc(
                TRANSCRIPT_SEARCH_POSTING_NAMESPACE,
                search_posting_key(space, subject, digest, page_id),
                serde_json::to_value(TranscriptSearchPostingPageV1 {
                    schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                    memory_space_id: space.clone(),
                    mounted_subject_id: subject.clone(),
                    term_digest: digest.clone(),
                    page_id,
                    revision: 1,
                    locators: chunk.to_vec(),
                })
                .map_err(|error| {
                    Error::config("conversation_transcript_query_migration", error.to_string())
                })?,
            );
        }
    }
    for (key, manifest) in message_manifests {
        push_doc(
            TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE,
            key,
            serde_json::to_value(manifest).map_err(|error| {
                Error::config("conversation_transcript_query_migration", error.to_string())
            })?,
        );
    }
    for memory_space_id in memory_spaces {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|error| {
            Error::config(
                "conversation_transcript_query_migration_entropy",
                error.to_string(),
            )
        })?;
        let key_hex = secret
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let key_id = format!(
            "sha256:{:x}",
            Sha256::digest(format!("key-id:{key_hex}").as_bytes())
        );
        push_doc(
            TRANSCRIPT_QUERY_KEYRING_NAMESPACE,
            keyring_key(&memory_space_id),
            serde_json::to_value(TranscriptQueryKeyringV1 {
                schema_version: TRANSCRIPT_QUERY_INDEX_SCHEMA_VERSION,
                memory_space_id: memory_space_id.clone(),
                incarnation: format!(
                    "sha256:{:x}",
                    Sha256::digest(format!("incarnation:{key_hex}").as_bytes())
                ),
                current: crate::store_internal::transcript_query::TranscriptQuerySigningKeyV1 {
                    key_id,
                    key_hex,
                    created_at: now_secs.max(1),
                    expires_at: now_secs.max(1).saturating_add(7_776_000),
                },
                previous: None,
            })
            .map_err(|error| {
                Error::config("conversation_transcript_query_migration", error.to_string())
            })?,
        );
    }
    let mut schema_manifest = snapshot.schema_manifest.clone();
    schema_manifest.schema_id = STORE_SCHEMA_ID.to_string();
    schema_manifest.schema_version = STORE_SCHEMA_VERSION;
    schema_manifest.last_opened_at_unix_secs = now_secs;
    let migrated = StoreSnapshot::new(
        schema_manifest,
        docs,
        snapshot.blobs.clone(),
        snapshot.events.clone(),
    );
    let json = migrated
        .json_docs
        .iter()
        .map(|doc| ((doc.namespace.clone(), doc.key.clone()), doc.value.clone()))
        .collect::<BTreeMap<_, _>>();
    validate_transcript_query_snapshot_closure(&json)?;
    Ok(migrated)
}

pub(crate) fn validate_transcript_query_snapshot_closure(
    json: &BTreeMap<(String, String), serde_json::Value>,
) -> Result<()> {
    validate_transcript_query_snapshot_closure_with_keyring(json, true)
}

fn validate_transcript_query_snapshot_closure_with_keyring(
    json: &BTreeMap<(String, String), serde_json::Value>,
    require_keyring: bool,
) -> Result<()> {
    let stage = "conversation_transcript_query_closure";
    let mut expected_query_message_keys = HashSet::new();
    for ((namespace, _), value) in json {
        if namespace == TRANSCRIPT_QUERY_KEYRING_NAMESPACE {
            let keyring = serde_json::from_value::<TranscriptQueryKeyringV1>(value.clone())
                .map_err(|error| Error::config(stage, error.to_string()))?;
            keyring
                .validate_for_memory_space(&keyring.memory_space_id)
                .map_err(|error| Error::config(stage, error.to_string()))?;
            if !json.contains_key(&(
                TRANSCRIPT_QUERY_KEYRING_NAMESPACE.to_string(),
                keyring_key(&keyring.memory_space_id),
            )) {
                return Err(Error::config(
                    stage,
                    "query keyring is stored under a non-canonical key",
                ));
            }
        }
        if namespace != "conversation_transcript" {
            continue;
        }
        let record = serde_json::from_value::<TranscriptTurnRecord>(value.clone())
            .map_err(|error| Error::config(stage, error.to_string()))?;
        expected_query_message_keys.extend(
            message_locators(&record)
                .into_iter()
                .map(|(locator, _)| search_message_manifest_key(&locator)),
        );
    }
    let mut catalog_heads =
        BTreeMap::<(String, String, String, String), ConversationCatalogHead>::new();
    for ((namespace, _), value) in json {
        if namespace == TRANSCRIPT_CATALOG_PAGE_NAMESPACE {
            let page = serde_json::from_value::<TranscriptCatalogPageV1>(value.clone())
                .map_err(|error| Error::config(stage, error.to_string()))?;
            page.validate()?;
            for head in page.heads {
                catalog_heads.insert(
                    (
                        head.key.memory_space_id.clone(),
                        head.mounted_subject_id.clone(),
                        head.key.channel_id.clone(),
                        head.key.conversation_id.clone(),
                    ),
                    head,
                );
            }
        }
    }
    for ((namespace, _), value) in json {
        if namespace == "conversation_transcript" {
            let record = serde_json::from_value::<TranscriptTurnRecord>(value.clone())
                .map_err(|error| Error::config(stage, error.to_string()))?;
            for (locator, content) in message_locators(&record) {
                let manifest_key = search_message_manifest_key(&locator);
                let manifest_value = json
                    .get(&(
                        TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE.to_string(),
                        manifest_key,
                    ))
                    .ok_or_else(|| {
                        Error::config(stage, "transcript message has no search manifest")
                    })?;
                let search_manifest = serde_json::from_value::<TranscriptMessageSearchManifestV1>(
                    manifest_value.clone(),
                )
                .map_err(|error| Error::config(stage, error.to_string()))?;
                search_manifest.validate()?;
                if search_manifest.locator != locator {
                    return Err(Error::config(
                        stage,
                        "message search manifest locator differs from transcript owner",
                    ));
                }
                let mut terms = TranscriptSearchNormalizerV1::index_terms(
                    content,
                    MAX_TRANSCRIPT_INDEX_TERMS_PER_MESSAGE,
                )?
                .iter()
                .map(|term| term_digest(term))
                .collect::<Vec<_>>();
                terms.sort();
                terms.dedup();
                if record.redaction_state == TranscriptRedactionState::RawAvailable
                    && search_manifest.term_set_digest != term_set_digest(&terms)
                {
                    return Err(Error::config(
                        stage,
                        "message search manifest digest differs from transcript content",
                    ));
                }
                let day = locator.observed_at / 86_400;
                let time_root_value = json
                    .get(&(
                        TRANSCRIPT_TIME_ROOT_NAMESPACE.to_string(),
                        time_root_key(&record.key, &record.subject, day),
                    ))
                    .ok_or_else(|| Error::config(stage, "message has no time root"))?;
                let time_root =
                    serde_json::from_value::<TranscriptTimePostingRootV1>(time_root_value.clone())
                        .map_err(|error| Error::config(stage, error.to_string()))?;
                let mut time_bound = false;
                for page_id in 0..time_root.page_count {
                    let page_value = json
                        .get(&(
                            TRANSCRIPT_TIME_POSTING_NAMESPACE.to_string(),
                            time_posting_key(&record.key, &record.subject, day, page_id),
                        ))
                        .ok_or_else(|| Error::config(stage, "time root page is missing"))?;
                    let page =
                        serde_json::from_value::<TranscriptTimePostingPageV1>(page_value.clone())
                            .map_err(|error| Error::config(stage, error.to_string()))?;
                    time_bound |= page.locators.iter().any(|entry| {
                        entry.locator == locator
                            && entry.lifecycle_state == record.lifecycle_state
                            && entry.redaction_state == record.redaction_state
                    });
                }
                if !time_bound {
                    return Err(Error::config(
                        stage,
                        "message is absent from its exact time posting",
                    ));
                }
                if record.redaction_state == TranscriptRedactionState::RawAvailable {
                    for digest in terms {
                        let root_value = json
                            .get(&(
                                TRANSCRIPT_SEARCH_ROOT_NAMESPACE.to_string(),
                                search_root_key(
                                    &record.key.memory_space_id,
                                    &record.subject,
                                    &digest,
                                ),
                            ))
                            .ok_or_else(|| {
                                Error::config(stage, "message search root is missing")
                            })?;
                        let root = serde_json::from_value::<TranscriptSearchPostingRootV1>(
                            root_value.clone(),
                        )
                        .map_err(|error| Error::config(stage, error.to_string()))?;
                        let mut search_bound = false;
                        for page_id in 0..root.page_count {
                            let page_value = json
                                .get(&(
                                    TRANSCRIPT_SEARCH_POSTING_NAMESPACE.to_string(),
                                    search_posting_key(
                                        &record.key.memory_space_id,
                                        &record.subject,
                                        &digest,
                                        page_id,
                                    ),
                                ))
                                .ok_or_else(|| {
                                    Error::config(stage, "search root page is missing")
                                })?;
                            let page = serde_json::from_value::<TranscriptSearchPostingPageV1>(
                                page_value.clone(),
                            )
                            .map_err(|error| Error::config(stage, error.to_string()))?;
                            search_bound |= page.locators.iter().any(|entry| {
                                entry.locator == locator
                                    && entry.lifecycle_state == record.lifecycle_state
                                    && entry.redaction_state == record.redaction_state
                            });
                        }
                        if !search_bound {
                            return Err(Error::config(
                                stage,
                                "message is absent from its exact search posting",
                            ));
                        }
                    }
                }
            }
        }
        if namespace == CONVERSATION_RECALL_MANIFEST_NAMESPACE {
            let head = serde_json::from_value::<ConversationRecallManifest>(value.clone())
                .map_err(|error| Error::config(stage, error.to_string()))?;
            let identity = (
                head.memory_space_id.clone(),
                head.mounted_subject_id.clone(),
                head.channel_id.clone(),
                head.conversation_id.clone(),
            );
            let catalog = catalog_heads.get(&identity).ok_or_else(|| {
                Error::config(
                    "conversation_transcript_query_migration_required",
                    "transcript head has no v11 catalog closure",
                )
            })?;
            if catalog.revision != head.revision || catalog.head_digest != head.head_digest {
                return Err(Error::config(
                    stage,
                    "catalog and transcript head identity differ",
                ));
            }
            if require_keyring
                && !json.contains_key(&(
                    TRANSCRIPT_QUERY_KEYRING_NAMESPACE.to_string(),
                    keyring_key(&head.memory_space_id),
                ))
            {
                return Err(Error::config(
                    stage,
                    "transcript memory space has no private query keyring",
                ));
            }
        }
        if namespace == TRANSCRIPT_CATALOG_ROOT_NAMESPACE {
            let root = serde_json::from_value::<TranscriptCatalogRootV1>(value.clone())
                .map_err(|error| Error::config(stage, error.to_string()))?;
            let mut entries = 0u64;
            for page_id in 0..root.page_count {
                let key = catalog_page_key(
                    &root.memory_space_id,
                    &root.mounted_subject_id,
                    root.channel_id.as_deref(),
                    page_id,
                );
                let page = json
                    .get(&(TRANSCRIPT_CATALOG_PAGE_NAMESPACE.to_string(), key))
                    .ok_or_else(|| {
                        Error::config(stage, "catalog root references a missing page")
                    })?;
                let page = serde_json::from_value::<TranscriptCatalogPageV1>(page.clone())
                    .map_err(|error| Error::config(stage, error.to_string()))?;
                entries =
                    entries.saturating_add(u64::try_from(page.heads.len()).unwrap_or(u64::MAX));
            }
            if entries != root.entry_count {
                return Err(Error::config(
                    stage,
                    "catalog root entry count differs from pages",
                ));
            }
        }
        if namespace == TRANSCRIPT_TIME_ROOT_NAMESPACE {
            let root = serde_json::from_value::<TranscriptTimePostingRootV1>(value.clone())
                .map_err(|error| Error::config(stage, error.to_string()))?;
            root.validate()?;
            let mut entries = 0u64;
            for page_id in 0..root.page_count {
                let key =
                    time_posting_key(&root.key, &root.mounted_subject_id, root.utc_day, page_id);
                let page = json
                    .get(&(TRANSCRIPT_TIME_POSTING_NAMESPACE.to_string(), key))
                    .ok_or_else(|| Error::config(stage, "time root references a missing page"))?;
                let page = serde_json::from_value::<TranscriptTimePostingPageV1>(page.clone())
                    .map_err(|error| Error::config(stage, error.to_string()))?;
                page.validate_for_root(&root)?;
                if page.locators.iter().any(|entry| {
                    !expected_query_message_keys
                        .contains(&search_message_manifest_key(&entry.locator))
                }) {
                    return Err(Error::config(
                        stage,
                        "time posting contains a non-indexable transcript message",
                    ));
                }
                entries =
                    entries.saturating_add(u64::try_from(page.locators.len()).unwrap_or(u64::MAX));
            }
            if entries != root.entry_count {
                return Err(Error::config(
                    stage,
                    "time root entry count differs from pages",
                ));
            }
        }
        if namespace == TRANSCRIPT_SEARCH_ROOT_NAMESPACE {
            let root = serde_json::from_value::<TranscriptSearchPostingRootV1>(value.clone())
                .map_err(|error| Error::config(stage, error.to_string()))?;
            root.validate()?;
            let mut entries = 0u64;
            for page_id in 0..root.page_count {
                let key = search_posting_key(
                    &root.memory_space_id,
                    &root.mounted_subject_id,
                    &root.term_digest,
                    page_id,
                );
                let page = json
                    .get(&(TRANSCRIPT_SEARCH_POSTING_NAMESPACE.to_string(), key))
                    .ok_or_else(|| Error::config(stage, "search root references a missing page"))?;
                let page = serde_json::from_value::<TranscriptSearchPostingPageV1>(page.clone())
                    .map_err(|error| Error::config(stage, error.to_string()))?;
                page.validate_for_root(&root)?;
                if page.locators.iter().any(|entry| {
                    !expected_query_message_keys
                        .contains(&search_message_manifest_key(&entry.locator))
                }) {
                    return Err(Error::config(
                        stage,
                        "search posting contains a non-indexable transcript message",
                    ));
                }
                entries =
                    entries.saturating_add(u64::try_from(page.locators.len()).unwrap_or(u64::MAX));
            }
            if entries != root.entry_count {
                return Err(Error::config(
                    stage,
                    "search root entry count differs from pages",
                ));
            }
        }
        if namespace == TRANSCRIPT_SEARCH_MESSAGE_MANIFEST_NAMESPACE {
            let manifest =
                serde_json::from_value::<TranscriptMessageSearchManifestV1>(value.clone())
                    .map_err(|error| Error::config(stage, error.to_string()))?;
            manifest.validate()?;
            if !expected_query_message_keys
                .contains(&search_message_manifest_key(&manifest.locator))
            {
                return Err(Error::config(
                    stage,
                    "search manifest belongs to a non-indexable transcript message",
                ));
            }
            let day = manifest.locator.observed_at / 86_400;
            if !json.contains_key(&(
                TRANSCRIPT_TIME_ROOT_NAMESPACE.to_string(),
                time_root_key(
                    &manifest.locator.key,
                    &manifest.locator.mounted_subject_id,
                    day,
                ),
            )) {
                return Err(Error::config(
                    stage,
                    "message search manifest has no time root",
                ));
            }
        }
    }
    for ((memory_space_id, subject, channel, conversation), catalog) in catalog_heads {
        let key = ConversationKey::new(memory_space_id, channel, conversation)?;
        let physical_key = StorePlatform::conversation_head_physical_key(&key, &subject)?;
        let value = json
            .get(&(
                CONVERSATION_RECALL_MANIFEST_NAMESPACE.to_string(),
                physical_key,
            ))
            .ok_or_else(|| Error::config(stage, "catalog head has no transcript owner head"))?;
        let head = serde_json::from_value::<ConversationRecallManifest>(value.clone())
            .map_err(|error| Error::config(stage, error.to_string()))?;
        if catalog.revision != head.revision || catalog.head_digest != head.head_digest {
            return Err(Error::config(
                stage,
                "catalog reverse closure identity differs",
            ));
        }
    }
    Ok(())
}

fn validate_snapshot_import_contract(
    snapshot: &StoreSnapshot,
    governed_state_budget: &GovernedStateRuntimeBudget,
    capacity: StoreCapacityBudget,
) -> Result<()> {
    if snapshot.schema_id != STORE_SCHEMA_ID {
        return Err(Error::config(
            "store_snapshot_import",
            format!("unsupported schema {}", snapshot.schema_id),
        ));
    }
    if snapshot.schema_manifest.schema_id != STORE_SCHEMA_ID {
        return Err(Error::config(
            "store_snapshot_import",
            format!(
                "unsupported manifest schema {}",
                snapshot.schema_manifest.schema_id
            ),
        ));
    }
    if snapshot.schema_manifest.schema_version != STORE_SCHEMA_VERSION {
        return Err(Error::config(
            "store_snapshot_import",
            format!(
                "unsupported manifest schema version {}",
                snapshot.schema_manifest.schema_version
            ),
        ));
    }

    let mut json_keys = HashSet::new();
    let mut snapshot_json = BTreeMap::new();
    let mut evidence_documents = BTreeMap::new();
    let mut evidence_source_claims = BTreeMap::new();
    let mut evidence_claim_manifests = BTreeMap::new();
    for doc in &snapshot.json_docs {
        admit_store_json_document(
            &doc.namespace,
            &doc.key,
            &doc.value,
            "store_snapshot_import",
        )?;
        if !json_keys.insert((doc.namespace.clone(), doc.key.clone())) {
            return Err(Error::config(
                "store_snapshot_import",
                format!("duplicate json doc {}:{}", doc.namespace, doc.key),
            ));
        }
        snapshot_json.insert((doc.namespace.clone(), doc.key.clone()), doc.value.clone());
        match doc.namespace.as_str() {
            CONVERSATION_RECALL_MANIFEST_NAMESPACE => {
                decode_typed_recall_index::<ConversationRecallManifest>(
                    &doc.key,
                    doc.value.clone(),
                )?;
            }
            ARCHIVE_RECALL_MANIFEST_NAMESPACE => {
                decode_typed_recall_index::<ArchiveRecallManifest>(&doc.key, doc.value.clone())?;
            }
            CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE => {
                decode_typed_recall_index::<ContinuityCapsuleScopeIndex>(
                    &doc.key,
                    doc.value.clone(),
                )?;
            }
            ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE => {
                decode_typed_recall_index::<ActiveTaskRunByChatIndex>(&doc.key, doc.value.clone())?;
            }
            TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE => {
                decode_typed_recall_index::<TaskLearningByChatIndex>(&doc.key, doc.value.clone())?;
            }
            RECALL_OWNER_SCOPE_BINDING_NAMESPACE => {
                let binding = serde_json::from_value::<RecallOwnerScopeBinding>(doc.value.clone())
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
                binding.validate()?;
                if binding.physical_key != doc.key {
                    return Err(Error::config(
                        "store_snapshot_import",
                        "recall owner scope binding snapshot key mismatch",
                    ));
                }
            }
            CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE => {
                let manifest =
                    serde_json::from_value::<ControlPlaneScopeManifest>(doc.value.clone())
                        .map_err(|error| {
                            Error::config("store_snapshot_import", error.to_string())
                        })?;
                manifest.validate(capacity.kv_max_entries)?;
                if manifest.physical_key != doc.key {
                    return Err(Error::config(
                        "store_snapshot_import",
                        "control-plane scope manifest snapshot key mismatch",
                    ));
                }
            }
            _ => {}
        }
        if doc.namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE {
            let document = serde_json::from_value::<GovernedEvidenceDocument>(doc.value.clone())
                .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
            validate_governed_evidence_document(&document)
                .map_err(|error| Error::config("store_snapshot_import", format!("{error:?}")))?;
            if document.physical_key != doc.key {
                return Err(Error::config(
                    "store_snapshot_import",
                    "evidence document snapshot key mismatch",
                ));
            }
            evidence_documents.insert(document.physical_key.clone(), document);
        } else if doc.namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE {
            let source_claim =
                serde_json::from_value::<GovernedEvidenceSourceRef>(doc.value.clone())
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
            if source_claim.physical_key != doc.key {
                return Err(Error::config(
                    "store_snapshot_import",
                    "evidence source claim snapshot key mismatch",
                ));
            }
            evidence_source_claims.insert(source_claim.physical_key.clone(), source_claim);
        } else if doc.namespace == GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE {
            let manifest =
                serde_json::from_value::<GovernedEvidenceSourceClaimManifest>(doc.value.clone())
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
            if manifest.physical_key != doc.key {
                return Err(Error::config(
                    "store_snapshot_import",
                    "evidence source claim manifest snapshot key mismatch",
                ));
            }
            evidence_claim_manifests.insert(
                (
                    manifest.memory_space_id.clone(),
                    manifest.mounted_subject_id.clone(),
                ),
                manifest,
            );
        }
    }
    validate_transcript_query_snapshot_closure(&snapshot_json)?;
    let typed_state = BackendTransactionState {
        json: snapshot_json.clone(),
        blobs: BTreeMap::new(),
        events: Vec::new(),
    };
    validate_long_term_version_store_image(
        &typed_state,
        governed_state_budget.max_retained_long_term_revisions_per_owner,
        "store_snapshot_import",
    )?;
    validate_runtime_skill_store_image(
        &typed_state,
        governed_state_budget.max_retained_runtime_skill_owners_per_scope,
        "store_snapshot_import",
    )?;
    crate::store_internal::transaction::validate_control_plane_manifest_set(
        &snapshot_json,
        capacity.kv_max_entries,
    )?;
    for document in evidence_documents.values() {
        let expected_claim = governed_evidence_source_ref_from_document(document)
            .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
        let Some(source_claim) = evidence_source_claims.get(&expected_claim.physical_key) else {
            return Err(Error::config(
                "store_snapshot_import",
                "evidence document snapshot is missing source claim",
            ));
        };
        validate_governed_evidence_source_ref(document, source_claim)
            .map_err(|error| Error::config("store_snapshot_import", format!("{error:?}")))?;
    }
    for source_claim in evidence_source_claims.values() {
        let owner_key = scoped_governed_evidence_document_key(
            &source_claim.memory_space_id,
            &source_claim.owner_ref.owner_id,
        )
        .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?;
        let Some(document) = evidence_documents.get(&owner_key) else {
            return Err(Error::config(
                "store_snapshot_import",
                "evidence source claim snapshot owner is missing",
            ));
        };
        validate_governed_evidence_source_ref(document, source_claim)
            .map_err(|error| Error::config("store_snapshot_import", format!("{error:?}")))?;
    }
    let mut evidence_scopes = BTreeSet::new();
    evidence_scopes.extend(evidence_documents.values().map(|document| {
        (
            document.memory_space_id.clone(),
            document.mounted_subject_id.clone(),
        )
    }));
    evidence_scopes.extend(evidence_source_claims.values().map(|claim| {
        (
            claim.memory_space_id.clone(),
            claim.mounted_subject_id.clone(),
        )
    }));
    evidence_scopes.extend(evidence_claim_manifests.keys().cloned());
    for (memory_space_id, mounted_subject_id) in evidence_scopes {
        let manifest = evidence_claim_manifests
            .get(&(memory_space_id.clone(), mounted_subject_id.clone()))
            .ok_or_else(|| {
                Error::config(
                    "store_snapshot_import",
                    "evidence source claim scope manifest is missing",
                )
            })?;
        let bindings = evidence_documents
            .values()
            .filter(|document| {
                document.memory_space_id == memory_space_id
                    && document.mounted_subject_id == mounted_subject_id
            })
            .map(|document| {
                let expected = governed_evidence_source_ref_from_document(document)?;
                let claim = evidence_source_claims.get(&expected.physical_key).ok_or_else(|| {
                    Error::config(
                        "store_snapshot_import",
                        "manifest evidence owner is missing its exact claim",
                    )
                })?;
                crate::store_internal::schema::GovernedEvidenceOwnerClaimBinding::from_document_claim(
                    document,
                    claim,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        manifest.validate_exact(
            &memory_space_id,
            &mounted_subject_id,
            bindings,
            capacity.kv_max_entries,
        )?;
    }

    validate_complete_snapshot_facet_graph_closure(&snapshot_json)?;

    let mut blob_keys = HashSet::new();
    let mut snapshot_blobs = BTreeMap::new();
    for blob in &snapshot.blobs {
        ensure_snapshot_blob_address(&blob.namespace, &blob.key, Some(&blob.value))?;
        if !blob_keys.insert((blob.namespace.clone(), blob.key.clone())) {
            return Err(Error::config(
                "store_snapshot_import",
                format!("duplicate blob {}:{}", blob.namespace, blob.key),
            ));
        }
        snapshot_blobs.insert(
            (blob.namespace.clone(), blob.key.clone()),
            blob.value.clone(),
        );
    }
    crate::store_internal::transaction::validate_snapshot_recall_manifest_documents(
        &snapshot_json,
        &snapshot_blobs,
    )?;

    let mut event_ids = HashSet::new();
    for event in &snapshot.events {
        event.validate_current_schema("store_snapshot_import")?;
        if !event_ids.insert(event.event_id.clone()) {
            return Err(Error::config(
                "store_snapshot_import",
                format!("duplicate event id {}", event.event_id),
            ));
        }
    }
    Ok(())
}

fn validate_complete_snapshot_facet_graph_closure(
    snapshot_json: &BTreeMap<(String, String), serde_json::Value>,
) -> Result<()> {
    const SCOPE_BEARING_NAMESPACES: &[&str] = &[
        crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
        crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
        crate::store_internal::schema::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
        GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
        GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
        CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
        MEMORY_FACET_INDEX_NAMESPACE,
        MEMORY_FACET_POSTING_NAMESPACE,
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        MEMORY_GRAPH_REVISION_NAMESPACE,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_INDEX_NAMESPACE,
    ];
    const FACET_INPUT_NAMESPACES: &[&str] = &[
        crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
        crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
        GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
        MEMORY_FACET_INDEX_NAMESPACE,
        MEMORY_FACET_POSTING_NAMESPACE,
    ];
    const GRAPH_INPUT_NAMESPACES: &[&str] = &[
        crate::store_internal::schema::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
        crate::store_internal::schema::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
        GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        MEMORY_GRAPH_REVISION_NAMESPACE,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_INDEX_NAMESPACE,
    ];

    fn exact_graph_document_membership_closure(
        snapshot_json: &BTreeMap<(String, String), serde_json::Value>,
    ) -> Result<()> {
        let actual = |namespace: &str| {
            snapshot_json
                .keys()
                .filter_map(|(candidate_namespace, key)| {
                    (candidate_namespace == namespace).then_some(key.clone())
                })
                .collect::<BTreeSet<_>>()
        };
        let expected_nodes = snapshot_json
            .iter()
            .filter(|((namespace, _), _)| namespace == MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE)
            .map(|(_, value)| {
                serde_json::from_value::<MemoryGraphNodeMembership>(value.clone())
                    .map(|membership| membership.document_key)
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))
            })
            .collect::<Result<BTreeSet<_>>>()?;
        let expected_edges = snapshot_json
            .iter()
            .filter(|((namespace, _), _)| namespace == MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE)
            .map(|(_, value)| {
                serde_json::from_value::<MemoryGraphEdgeMembership>(value.clone())
                    .map(|membership| membership.document_key)
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))
            })
            .collect::<Result<BTreeSet<_>>>()?;
        let expected_backlinks = snapshot_json
            .iter()
            .filter(|((namespace, _), _)| namespace == MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE)
            .map(|(_, value)| {
                serde_json::from_value::<MemoryGraphBacklinkMembership>(value.clone())
                    .map(|membership| membership.document_key)
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))
            })
            .collect::<Result<BTreeSet<_>>>()?;
        for (namespace, expected) in [
            (MEMORY_GRAPH_NODE_NAMESPACE, expected_nodes),
            (MEMORY_GRAPH_EDGE_NAMESPACE, expected_edges),
            (MEMORY_GRAPH_BACKLINK_NAMESPACE, expected_backlinks),
        ] {
            if actual(namespace) != expected {
                return Err(Error::config(
                    "store_snapshot_import",
                    format!(
                        "complete snapshot {namespace} documents must exactly equal membership-owned keys"
                    ),
                ));
            }
        }
        Ok(())
    }

    fn scope_subjects(namespace: &str, value: &serde_json::Value) -> Result<Vec<(String, String)>> {
        let memory_space_id = value
            .get("memory_space_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                Error::config(
                    "store_snapshot_import",
                    format!("{namespace} is missing its exact memory-space scope"),
                )
            })?;
        let mut subjects = Vec::new();
        for field in ["factual_owner_id", "mounted_subject_id", "subject_id"] {
            if let Some(subject_id) = value
                .get(field)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                subjects.push(subject_id.to_string());
            }
        }
        if let Some(values) = value.get("subject_ids") {
            let values = values.as_array().ok_or_else(|| {
                Error::config(
                    "store_snapshot_import",
                    format!("{namespace} subject_ids must be an exact array"),
                )
            })?;
            for subject_id in values {
                let subject_id = subject_id
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        Error::config(
                            "store_snapshot_import",
                            format!("{namespace} contains an invalid subject scope"),
                        )
                    })?;
                subjects.push(subject_id.to_string());
            }
        }
        subjects.sort();
        subjects.dedup();
        if subjects.is_empty() {
            return Err(Error::config(
                "store_snapshot_import",
                format!("{namespace} is missing its exact owner scope"),
            ));
        }
        Ok(subjects
            .into_iter()
            .map(|subject_id| (memory_space_id.to_string(), subject_id))
            .collect())
    }

    fn matches_scope(
        namespace: &str,
        value: &serde_json::Value,
        memory_space_id: &str,
        mounted_subject_id: &str,
    ) -> Result<bool> {
        if !SCOPE_BEARING_NAMESPACES.contains(&namespace) {
            return Ok(false);
        }
        Ok(scope_subjects(namespace, value)?
            .iter()
            .any(|(space, subject)| space == memory_space_id && subject == mounted_subject_id))
    }

    fn batch_for_scope(
        snapshot_json: &BTreeMap<(String, String), serde_json::Value>,
        namespaces: &[&str],
        memory_space_id: &str,
        mounted_subject_id: &str,
        operation: &str,
    ) -> Result<StoreMutationBatch> {
        let mutations = snapshot_json
            .iter()
            .filter_map(|((namespace, key), value)| {
                if !namespaces.contains(&namespace.as_str()) {
                    return None;
                }
                Some(
                    matches_scope(namespace, value, memory_space_id, mounted_subject_id).map(
                        |matches| {
                            matches.then(|| StoreMutation::PutJson {
                                namespace: namespace.clone(),
                                key: key.clone(),
                                value: value.clone(),
                                event_kind: MemoryStoreEventKind::MemoryMaintenance,
                                plane: namespace.clone(),
                                record_key: if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE {
                                    value
                                        .get("document_id")
                                        .and_then(serde_json::Value::as_str)
                                        .unwrap_or(key)
                                        .to_string()
                                } else {
                                    key.clone()
                                },
                            })
                        },
                    ),
                )
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(StoreMutationBatch {
            transaction_id: format!("{operation}:{memory_space_id}:{mounted_subject_id}"),
            operation: operation.to_string(),
            scope: StoreEventScope::system(operation)
                .with_memory_space(memory_space_id)
                .with_subject(mounted_subject_id),
            mutations,
        })
    }

    exact_graph_document_membership_closure(snapshot_json)?;

    let mut scopes = BTreeSet::new();
    for ((namespace, _), value) in snapshot_json {
        if SCOPE_BEARING_NAMESPACES.contains(&namespace.as_str()) {
            scopes.extend(scope_subjects(namespace, value)?);
        }
    }
    if scopes.is_empty() {
        return Ok(());
    }

    let state = BackendTransactionState {
        json: snapshot_json.clone(),
        blobs: BTreeMap::new(),
        events: Vec::new(),
    };
    for (memory_space_id, mounted_subject_id) in scopes {
        let facet_batch = batch_for_scope(
            snapshot_json,
            FACET_INPUT_NAMESPACES,
            &memory_space_id,
            &mounted_subject_id,
            "store_snapshot_facet_validation",
        )?;
        if !facet_batch.mutations.is_empty() {
            validate_facet_post_image(&facet_batch, &state, &state)?;
        }

        let graph_batch = batch_for_scope(
            snapshot_json,
            GRAPH_INPUT_NAMESPACES,
            &memory_space_id,
            &mounted_subject_id,
            "store_snapshot_graph_validation",
        )?;
        let graph_closure_present = graph_batch.mutations.iter().any(|mutation| {
            matches!(
                mutation,
                StoreMutation::PutJson { namespace, .. }
                    if namespace.starts_with("memory_graph_")
            )
        });
        if graph_closure_present {
            validate_graph_post_image(&graph_batch, &state, &state, false)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_scoped_projection_governed_closure(
    snapshot: &StoreSnapshot,
    projection_scope: &StoreScopedProjectionScope,
) -> Result<()> {
    let after = BackendTransactionState {
        json: snapshot
            .json_docs
            .iter()
            .map(|doc| ((doc.namespace.clone(), doc.key.clone()), doc.value.clone()))
            .collect(),
        blobs: BTreeMap::new(),
        events: snapshot.events.clone(),
    };
    crate::store_internal::transaction::validate_scoped_recall_manifest_documents(
        &after.json,
        &after.blobs,
        projection_scope,
    )?;
    crate::store_internal::transaction::validate_scoped_control_plane_documents(
        &after.json,
        projection_scope,
        snapshot.json_docs.len().max(1),
    )?;
    validate_complete_snapshot_facet_graph_closure(&after.json)?;
    if after.json.keys().any(|(namespace, _)| {
        namespace == CONVERSATION_RECALL_MANIFEST_NAMESPACE
            || transcript_query_namespace_is_derived(namespace)
    }) {
        let has_keyring = after
            .json
            .keys()
            .any(|(namespace, _)| namespace == TRANSCRIPT_QUERY_KEYRING_NAMESPACE);
        validate_transcript_query_snapshot_closure_with_keyring(&after.json, has_keyring)?;
    }
    Ok(())
}

fn enforce_snapshot_logical_budget(
    capacity: StoreCapacityBudget,
    snapshot: &StoreSnapshot,
) -> Result<()> {
    let entry_count = snapshot
        .json_docs
        .len()
        .checked_add(snapshot.blobs.len())
        .ok_or_else(|| store_budget_error("snapshot entry count overflow"))?;
    if entry_count > capacity.kv_max_entries {
        return Err(store_budget_error(format!(
            "snapshot entries {entry_count} exceed {}",
            capacity.kv_max_entries
        )));
    }
    if snapshot.events.len() > capacity.event_log_max_items {
        return Err(store_budget_error(format!(
            "snapshot event lineage items {} exceed {}",
            snapshot.events.len(),
            capacity.event_log_max_items
        )));
    }

    let mut json_event_bytes = 0_usize;
    for doc in &snapshot.json_docs {
        enforce_logical_key_budget(capacity, &doc.namespace, &doc.key, "store_snapshot_import")?;
        json_event_bytes = json_event_bytes
            .checked_add(
                serde_json::to_vec(&doc.value)
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?
                    .len(),
            )
            .ok_or_else(|| store_budget_error("snapshot JSON byte count overflow"))?;
    }
    let mut blob_bytes = 0_usize;
    for blob in &snapshot.blobs {
        enforce_logical_key_budget(
            capacity,
            &blob.namespace,
            &blob.key,
            "store_snapshot_import",
        )?;
        blob_bytes = blob_bytes
            .checked_add(blob.value.len())
            .ok_or_else(|| store_budget_error("snapshot blob byte count overflow"))?;
    }
    for event in &snapshot.events {
        enforce_event_key_budget(capacity, event, "store_snapshot_import")?;
        json_event_bytes = json_event_bytes
            .checked_add(
                serde_json::to_vec(event)
                    .map_err(|error| Error::config("store_snapshot_import", error.to_string()))?
                    .len()
                    .saturating_add(1),
            )
            .ok_or_else(|| store_budget_error("snapshot event byte count overflow"))?;
    }
    if json_event_bytes > capacity.snapshot_max_bytes {
        return Err(store_budget_error(format!(
            "snapshot JSON and event bytes {json_event_bytes} exceed {}",
            capacity.snapshot_max_bytes
        )));
    }
    if blob_bytes > capacity.blob_max_bytes {
        return Err(store_budget_error(format!(
            "snapshot blob bytes {blob_bytes} exceed {}",
            capacity.blob_max_bytes
        )));
    }
    Ok(())
}

fn build_runtime_event(
    config: &StoreBackendConfig,
    operation: &str,
    timestamp_unix_secs: u64,
) -> MemoryStoreEvent {
    MemoryStoreEvent::new(
        next_event_id(),
        MemoryStoreEventKind::RuntimeLifecycle,
        StoreEventScope::system(operation),
        timestamp_unix_secs,
    )
    .with_payload("backend", config.backend.as_str())
    .with_payload("profile", config.profile.as_str())
    .with_payload("success", "true")
    .with_payload("result", "ok")
}

fn tail<T>(mut values: Vec<T>, limit: usize) -> Vec<T> {
    if values.len() <= limit {
        return values;
    }
    let remove_count = values.len() - limit;
    values.drain(0..remove_count);
    values
}

fn private_garden_key_prefix(mounted_subject_id: &str) -> String {
    format!("{mounted_subject_id}::")
}

fn private_garden_key(mounted_subject_id: &str, doc_path: &str) -> String {
    format!(
        "{}{doc_path}",
        private_garden_key_prefix(mounted_subject_id)
    )
}

fn private_garden_record(doc: &PrivateGardenDoc) -> PrivateGardenDocRecord {
    PrivateGardenDocRecord {
        path: doc.path.clone(),
        updated_at: doc.updated_at,
        revision: doc.revision,
        bytes: doc.content.len(),
        preview: doc.content.chars().take(160).collect(),
    }
}

fn triple_key(a: &str, b: &str, c: &str) -> String {
    format!("{a}::{b}::{c}")
}

fn _dedupe_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

#[cfg(test)]
mod transaction_error_contract_tests {
    use super::*;
    use bm_core::feature_gate::ProfileId;
    #[cfg(feature = "nonproduction-replay-harness")]
    use bm_core::orchestrator::PressureLevel;
    #[cfg(feature = "nonproduction-replay-harness")]
    use bm_core::resource::{RuntimeResourceObservation, RuntimeResourceProbe};
    #[cfg(feature = "nonproduction-replay-harness")]
    use std::sync::atomic::{AtomicU64, Ordering};

    struct SyntheticStoreDir(std::path::PathBuf);

    impl SyntheticStoreDir {
        fn create(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "bm-mutation-operation-{label}-{}-{}",
                std::process::id(),
                current_unix_nanos()
            ));
            std::fs::create_dir_all(&path).expect("create synthetic store directory");
            Self(path)
        }
    }

    impl Drop for SyntheticStoreDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn native_production_profile() -> ProfileId {
        #[cfg(target_os = "macos")]
        return ProfileId::DesktopMacosEmbeddedSdk;
        #[cfg(target_os = "windows")]
        return ProfileId::DesktopWindowsEmbeddedSdk;
        #[cfg(target_os = "linux")]
        return ProfileId::DesktopLinuxEmbeddedSdk;
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        compile_error!("store preparation tests require a supported host target");
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn runtime_skill_owner_plan_uses_blob_cas_and_stale_commit_changes_nothing() {
        let platform = StorePlatform::open(
            StoreBackendConfig::in_memory(native_production_profile()).expect("store config"),
        )
        .expect("store platform");
        let stale = platform
            .plan_runtime_skill_write("owner.md", b"stale")
            .expect("stale owner plan");
        let concurrent = platform
            .plan_runtime_skill_write("owner.md", b"concurrent")
            .expect("concurrent owner plan");
        let authority = platform.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).expect("budget lease");
        let (mutations, preconditions, blob_preconditions) = concurrent.into_parts();
        lease
            .execute(&authority, || {
                platform
                    .commit_governed_memory_transaction_with_blob_preconditions_and_runtime_budget(
                        StoreMutationBatch {
                            transaction_id: "skill-cas-concurrent".to_string(),
                            operation: "skill.write".to_string(),
                            scope: platform.recall_scope(),
                            mutations,
                        },
                        &preconditions,
                        &blob_preconditions,
                        lease.report(),
                    )
            })
            .expect("concurrent owner commit");
        let before_events = platform.read_events().expect("events before stale");
        let (mutations, preconditions, blob_preconditions) = stale.into_parts();
        let stale_lease =
            crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).expect("stale budget lease");
        let error = stale_lease
            .execute(&authority, || {
                platform
                    .commit_governed_memory_transaction_with_blob_preconditions_and_runtime_budget(
                        StoreMutationBatch {
                            transaction_id: "skill-cas-stale".to_string(),
                            operation: "skill.write".to_string(),
                            scope: platform.recall_scope(),
                            mutations,
                        },
                        &preconditions,
                        &blob_preconditions,
                        stale_lease.report(),
                    )
            })
            .expect_err("stale owner plan must fail");

        assert_eq!(
            error.stage(),
            "memory_write_transaction_precondition_failed"
        );
        assert_eq!(
            platform
                .engine
                .get_blob("skills", "owner.md")
                .expect("skill"),
            Some(b"concurrent".to_vec())
        );
        assert_eq!(
            platform.read_events().expect("events after stale"),
            before_events
        );
    }

    #[test]
    fn subject_soul_full_intent_digest_binds_non_soul_owner_effects() {
        let core = "a".repeat(64);
        let first = canonical_subject_soul_full_intent_digest(
            &core,
            &[StoreMutation::PutBlob {
                namespace: "skills".to_string(),
                key: "owner.md".to_string(),
                value: b"first".to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skills".to_string(),
                record_key: "owner.md".to_string(),
            }],
            &[],
            &[StoreBlobPrecondition::Absent {
                namespace: "skills".to_string(),
                key: "owner.md".to_string(),
            }],
        )
        .expect("first full intent");
        let second = canonical_subject_soul_full_intent_digest(
            &core,
            &[StoreMutation::PutBlob {
                namespace: "skills".to_string(),
                key: "owner.md".to_string(),
                value: b"second".to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skills".to_string(),
                record_key: "owner.md".to_string(),
            }],
            &[],
            &[StoreBlobPrecondition::Absent {
                namespace: "skills".to_string(),
                key: "owner.md".to_string(),
            }],
        )
        .expect("second full intent");

        assert_ne!(first, second);
        assert_eq!(
            canonical_subject_soul_full_intent_digest(&core, &[], &[], &[])
                .expect("Soul-only intent"),
            core
        );
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn direct_and_transaction_lifecycle_materialization_share_typed_completion_fields() {
        let profile = ProfileId::native_dev_full().expect("native dev-full profile");
        let admission = bm_core::runtime::RuntimeLifecycleEngine.admit(
            bm_core::runtime::RuntimeLifecycleOperation::Maintain,
            bm_core::runtime::RuntimeLifecycleTrigger::SdkCall,
            bm_core::runtime::RuntimeLifecycleModeInput {
                profile,
                pressure: bm_core::orchestrator::PressureLevel::Normal,
                ..bm_core::runtime::RuntimeLifecycleModeInput::default()
            },
        );
        let report = bm_core::runtime::RuntimeLifecycleReport::from_admission(admission, 10)
            .finish_success(11, true, "maintenance_completed");
        let event = bm_core::runtime::RuntimeLifecycleEvent::from_report(
            bm_core::runtime::RuntimeLifecycleEventKind::RuntimeLifecycle,
            bm_core::runtime::RuntimeLifecycleEffect::RunMaintenance,
            &report,
            11,
        )
        .with_payload("changed", "true");

        let direct =
            materialize_runtime_lifecycle_store_event(&event, RuntimeLifecycleStoreBinding::Direct)
                .expect("direct lifecycle event");
        let transaction = materialize_runtime_lifecycle_store_event(
            &event,
            RuntimeLifecycleStoreBinding::Transaction {
                operation: "write.candidates",
                transaction_id: "transaction-1",
            },
        )
        .expect("transaction lifecycle event");

        assert_eq!(
            direct.payload.get("operation").map(String::as_str),
            Some("maintain")
        );
        assert_eq!(
            transaction.payload.get("operation").map(String::as_str),
            Some("write.candidates")
        );
        assert_eq!(
            direct.payload.get("runtime_operation").map(String::as_str),
            Some("maintain")
        );
        assert_eq!(
            transaction
                .payload
                .get("runtime_operation")
                .map(String::as_str),
            Some("maintain")
        );
        for key in [
            "runtime_operation",
            "trigger",
            "disposition",
            "effect",
            "profile",
            "mode",
            "pressure",
            "reason",
            "success",
            "result",
            "result_summary",
            "error_stage",
            "changed",
        ] {
            assert_eq!(
                direct.payload.get(key),
                transaction.payload.get(key),
                "{key}"
            );
        }
        assert!(!direct.payload.contains_key("transaction_id"));
        assert_eq!(
            transaction
                .payload
                .get("transaction_id")
                .map(String::as_str),
            Some("transaction-1")
        );
    }

    #[test]
    fn backend_conflict_busy_and_repair_stages_survive_the_production_coordinator() {
        for stage in [
            "memory_write_transaction_precondition_failed",
            "store_transaction_busy",
            "memory_write_transaction_repair_required",
        ] {
            let mapped =
                memory_write_transaction_commit_error(Error::config(stage, "proof"), false);
            assert_eq!(mapped.stage(), stage);
        }
    }

    #[test]
    fn subject_soul_capacity_stage_survives_the_production_coordinator() {
        for stage in ["store_budget_exceeded", "store_event_log"] {
            let mapped = memory_write_transaction_commit_error(Error::config(stage, "proof"), true);
            assert_eq!(mapped.stage(), "subject_soul_store_capacity");
            let generic =
                memory_write_transaction_commit_error(Error::config(stage, "proof"), false);
            assert_eq!(generic.stage(), "memory_write_transaction_preflight_failed");
        }
    }

    #[test]
    fn pristine_subject_soul_read_reuses_the_recall_immutable_session_and_receipt() {
        let platform = StorePlatform::open(
            StoreBackendConfig::in_memory(ProfileId::DesktopMacosEmbeddedSdk)
                .expect("store config"),
        )
        .expect("store");
        let authority = platform.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).expect("lease");
        let budget = lease.report().clone();
        let outcome = lease
            .execute(&authority, || {
                platform.with_recall_immutable_read_session(&budget, |context| {
                    context
                        .read_verified_subject_soul(
                            "space:recall-soul",
                            "soul:recall-soul",
                            &bm_core::memory::SubjectSoulReadRequestV1 {
                                target_subject_id: "subject:recall-soul".to_string(),
                                selector: bm_core::memory::SubjectSoulReadSelectorV1::Current,
                                view: bm_core::memory::SubjectSoulReadViewV1::RuntimePrivate,
                            },
                            &budget,
                        )
                        .map_err(
                            crate::store_internal::subject_soul::SubjectSoulStoreFailure::into_store_error,
                        )
                })
            })
            .expect("one immutable recall session");
        assert_eq!(outcome.session_open_count, 1);
        assert_eq!(outcome.receipt_count, 1);
        assert_eq!(outcome.receipt, outcome.output.receipt);
        assert_eq!(outcome.receipt.entry_count, 2);
        assert_eq!(outcome.receipt.json_doc_count, 0);
        assert!(matches!(
            outcome.output.outcome,
            bm_core::memory::SubjectSoulReadOutcomeV1::ImplicitUnseeded { .. }
        ));
    }

    #[test]
    fn mutation_operation_commits_effect_receipt_and_audit_in_one_store_transaction() {
        let profile = native_production_profile();
        let config = StoreBackendConfig::in_memory(profile)
            .expect("in-memory config")
            .with_fsync(false);
        let platform = StorePlatform::open(config).expect("store platform");
        let identity = MemoryMutationOperationIdentity::new(
            "operation-store-atomic",
            "memory-space-main",
            "subject-main",
            "actor-main",
            MemoryMutationOperationKind::Write,
        )
        .unwrap();
        let operation = StoreMutationOperationPlan::new(
            identity.clone(),
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            MemoryMutationEffect::Changed,
            1,
            "actor-main",
            1_800_000_000,
        )
        .unwrap();
        let effect_key = "order";
        let batch = StoreMutationBatch {
            transaction_id: operation.transaction_id().to_string(),
            operation: "write.procedural".to_string(),
            scope: StoreEventScope::new("agent", "owner", "local", "chat")
                .with_memory_space(identity.memory_space_id())
                .with_subject(identity.mounted_subject_id()),
            mutations: vec![StoreMutation::PutJson {
                namespace: "skill_meta".to_string(),
                key: effect_key.to_string(),
                value: serde_json::json!(["operation-effect"]),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skill_meta".to_string(),
                record_key: effect_key.to_string(),
            }],
        };
        let authority = platform.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).unwrap();
        let replay_batch = batch.clone();
        let replay_operation = operation.clone();
        let first = lease
            .execute(&authority, || {
                platform.commit_memory_mutation_operation_with_runtime_budget(
                    batch,
                    &[],
                    operation,
                    lease.report(),
                )
            })
            .expect("atomic mutation operation");
        let StoreMutationOperationOutcome::Committed { receipt, report } = first else {
            panic!("first operation must commit")
        };

        assert!(report.committed);
        assert_eq!(receipt.transaction_id, report.transaction_id);
        assert_eq!(
            platform
                .json_get::<serde_json::Value>("skill_meta", effect_key)
                .unwrap(),
            Some(serde_json::json!(["operation-effect"]))
        );
        assert_eq!(
            platform
                .json_get::<MemoryMutationReceipt>(
                    MEMORY_MUTATION_RECEIPT_NAMESPACE,
                    &identity.storage_key(),
                )
                .unwrap(),
            Some(receipt.clone())
        );
        let audit = platform
            .json_get::<MemoryMutationAuditRecord>(
                MEMORY_MUTATION_AUDIT_NAMESPACE,
                &receipt.audit_record_id,
            )
            .unwrap()
            .expect("authoritative audit");
        assert_eq!(audit.effect_plan_digest, receipt.effect_plan_digest);
        assert_eq!(audit.transaction_id, receipt.transaction_id);
        let committed_events = platform.read_events().expect("operation events");
        assert_eq!(
            committed_events
                .iter()
                .filter(|event| event.kind == MemoryStoreEventKind::MemoryWrite)
                .count(),
            1,
            "only the business effect is a metric-bearing memory write"
        );
        assert_eq!(
            committed_events
                .iter()
                .filter(|event| event.kind == MemoryStoreEventKind::MemoryControl)
                .count(),
            2,
            "receipt and authoritative audit are governance closure events"
        );

        let replay_authority = platform.runtime_budget_authority();
        let replay_lease = crate::RuntimeBudgetLease::issue(Arc::clone(&replay_authority)).unwrap();
        let replay = replay_lease
            .execute(&replay_authority, || {
                platform.commit_memory_mutation_operation_with_runtime_budget(
                    replay_batch,
                    &[],
                    replay_operation,
                    replay_lease.report(),
                )
            })
            .expect("same operation replay");
        let StoreMutationOperationOutcome::Replayed {
            receipt: replay_receipt,
        } = replay
        else {
            panic!("same operation must replay")
        };
        assert_eq!(replay_receipt, receipt);

        let conflicting_operation = StoreMutationOperationPlan::new(
            identity.clone(),
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            MemoryMutationEffect::Changed,
            1,
            "actor-main",
            1_800_000_001,
        )
        .unwrap();
        let conflicting_batch = StoreMutationBatch {
            transaction_id: conflicting_operation.transaction_id().to_string(),
            operation: "write.procedural".to_string(),
            scope: StoreEventScope::new("agent", "owner", "local", "chat")
                .with_memory_space(identity.memory_space_id())
                .with_subject(identity.mounted_subject_id()),
            mutations: Vec::new(),
        };
        let conflict_authority = platform.runtime_budget_authority();
        let conflict_lease =
            crate::RuntimeBudgetLease::issue(Arc::clone(&conflict_authority)).unwrap();
        let conflict = conflict_lease
            .execute(&conflict_authority, || {
                platform.commit_memory_mutation_operation_with_runtime_budget(
                    conflicting_batch,
                    &[],
                    conflicting_operation,
                    conflict_lease.report(),
                )
            })
            .expect_err("same scoped identity with a different intent must conflict");
        assert_eq!(conflict.class(), Some(bm_core::ErrorClass::Conflict));
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn mutation_operation_pair_read_rejects_a_tampered_authoritative_audit() {
        let profile = native_production_profile();
        let platform = StorePlatform::open(
            StoreBackendConfig::in_memory(profile)
                .expect("in-memory config")
                .with_fsync(false),
        )
        .expect("store platform");
        let identity = MemoryMutationOperationIdentity::new(
            "operation-tampered-audit",
            "memory-space-main",
            "subject-main",
            "actor-main",
            MemoryMutationOperationKind::Write,
        )
        .expect("identity");
        let operation = StoreMutationOperationPlan::new(
            identity.clone(),
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            MemoryMutationEffect::Changed,
            1,
            "actor-main",
            1_800_000_000,
        )
        .expect("operation");
        let batch = StoreMutationBatch {
            transaction_id: operation.transaction_id().to_string(),
            operation: "write.procedural".to_string(),
            scope: StoreEventScope::new("agent", "owner", "local", "chat")
                .with_memory_space(identity.memory_space_id())
                .with_subject(identity.mounted_subject_id()),
            mutations: vec![StoreMutation::PutJson {
                namespace: "skill_meta".to_string(),
                key: "tampered-audit-effect".to_string(),
                value: serde_json::json!(["committed"]),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skill_meta".to_string(),
                record_key: "tampered-audit-effect".to_string(),
            }],
        };
        let authority = platform.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).expect("lease");
        lease
            .execute(&authority, || {
                platform.commit_memory_mutation_operation_with_runtime_budget(
                    batch,
                    &[],
                    operation.clone(),
                    lease.report(),
                )
            })
            .expect("commit operation");

        let key = identity.storage_key();
        let mut audit = platform
            .json_get::<serde_json::Value>(MEMORY_MUTATION_AUDIT_NAMESPACE, &key)
            .expect("read audit")
            .expect("audit document");
        audit["actor_subject_id"] = serde_json::json!("actor-tampered");
        platform
            .tamper_json_document_for_nonproduction_harness(
                MEMORY_MUTATION_AUDIT_NAMESPACE,
                &key,
                audit,
            )
            .expect("tamper authoritative audit");

        let error = platform
            .load_committed_mutation_operation(&operation)
            .expect_err("tampered authoritative audit must fail closed");
        assert_eq!(error.stage(), "memory_write_transaction_repair_required");
        assert!(error
            .to_string()
            .contains("invalid authoritative mutation audit"));
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn mutation_receipts_are_pinned_and_capacity_exhaustion_fails_closed_without_eviction() {
        let profile = native_production_profile();
        let mut budget = StorePlatform::open(
            StoreBackendConfig::in_memory(profile).expect("base in-memory config"),
        )
        .expect("base in-memory store")
        .capacity()
        .into_runtime_budget();
        budget.event_log_max_items = 4;
        let config = StoreBackendConfig::in_memory(profile)
            .expect("in-memory config")
            .with_fsync(false)
            .try_with_nonproduction_store_budget_limit(budget)
            .expect("exact operation receipt capacity contraction");
        let platform = StorePlatform::open(config).expect("capacity-limited store platform");

        let commit = |operation_id: &str, effect_key: &str, intent_byte: char| {
            let identity = MemoryMutationOperationIdentity::new(
                operation_id,
                "memory-space-main",
                "subject-main",
                "actor-main",
                MemoryMutationOperationKind::Write,
            )?;
            let intent_digest = format!("sha256:{}", intent_byte.to_string().repeat(64));
            let operation = StoreMutationOperationPlan::new(
                identity,
                intent_digest,
                MemoryMutationEffect::Changed,
                1,
                "actor-main",
                1_800_000_000,
            )?;
            let batch = StoreMutationBatch {
                transaction_id: operation.transaction_id().to_string(),
                operation: "write.procedural".to_string(),
                scope: StoreEventScope::new("agent", "owner", "local", "chat")
                    .with_memory_space("memory-space-main")
                    .with_subject("subject-main"),
                mutations: vec![StoreMutation::PutJson {
                    namespace: "skill_meta".to_string(),
                    key: effect_key.to_string(),
                    value: serde_json::json!([effect_key]),
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "skill_meta".to_string(),
                    record_key: effect_key.to_string(),
                }],
            };
            let authority = platform.runtime_budget_authority();
            let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority))?;
            lease.execute(&authority, || {
                platform.commit_memory_mutation_operation_with_runtime_budget(
                    batch,
                    &[],
                    operation,
                    lease.report(),
                )
            })
        };

        let first = commit("operation-capacity-first", "order", 'a')
            .expect("first operation exactly fills durable receipt capacity");
        let StoreMutationOperationOutcome::Committed { receipt, .. } = first else {
            panic!("first capacity operation must commit")
        };
        let before = platform
            .export_store_snapshot()
            .expect("snapshot before overflow");

        let error = commit("operation-capacity-second", "disabled", 'b')
            .expect_err("second operation must fail before evicting the first receipt");
        assert_eq!(error.stage(), "memory_write_transaction_preflight_failed");
        assert!(
            error.to_string().contains("store_budget_exceeded"),
            "{error}"
        );
        let after = platform
            .export_store_snapshot()
            .expect("snapshot after overflow");
        assert_eq!(after.state_fingerprint(), before.state_fingerprint());
        assert_eq!(after.event_fingerprint(), before.event_fingerprint());
        assert_eq!(
            platform
                .json_get::<MemoryMutationReceipt>(
                    MEMORY_MUTATION_RECEIPT_NAMESPACE,
                    &receipt.identity.storage_key(),
                )
                .expect("read pinned receipt"),
            Some(receipt)
        );
        assert_eq!(
            platform
                .json_get::<serde_json::Value>("skill_meta", "disabled")
                .expect("read rejected effect"),
            None
        );
    }

    #[test]
    fn file_store_reopen_replays_persisted_mutation_operation_receipt() {
        let store_dir = SyntheticStoreDir::create("file-reopen");
        let config = StoreBackendConfig::file(&store_dir.0, native_production_profile())
            .unwrap()
            .with_fsync(false);
        let identity = MemoryMutationOperationIdentity::new(
            "operation-file-reopen",
            "memory-space-main",
            "subject-main",
            "actor-main",
            MemoryMutationOperationKind::Write,
        )
        .unwrap();
        let intent_digest =
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let now = current_unix_secs();

        let committed_receipt = {
            let platform = StorePlatform::open(config.clone()).expect("first file store open");
            let operation = StoreMutationOperationPlan::new(
                identity.clone(),
                intent_digest,
                MemoryMutationEffect::Changed,
                1,
                "actor-main",
                now,
            )
            .unwrap();
            let batch = StoreMutationBatch {
                transaction_id: operation.transaction_id().to_string(),
                operation: "write.procedural".to_string(),
                scope: StoreEventScope::new("agent", "owner", "local", "chat")
                    .with_memory_space(identity.memory_space_id())
                    .with_subject(identity.mounted_subject_id()),
                mutations: vec![StoreMutation::PutJson {
                    namespace: "skill_meta".to_string(),
                    key: "order".to_string(),
                    value: serde_json::json!(["persisted-operation-effect"]),
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "skill_meta".to_string(),
                    record_key: "order".to_string(),
                }],
            };
            let authority = platform.runtime_budget_authority();
            let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).unwrap();
            let outcome = lease
                .execute(&authority, || {
                    platform.commit_memory_mutation_operation_with_runtime_budget(
                        batch,
                        &[],
                        operation,
                        lease.report(),
                    )
                })
                .expect("commit file operation");
            let StoreMutationOperationOutcome::Committed { receipt, .. } = outcome else {
                panic!("first file operation must commit")
            };
            receipt
        };

        let reopened = StorePlatform::open(config).expect("reopen file store");
        let replay_operation = StoreMutationOperationPlan::new(
            identity.clone(),
            intent_digest,
            MemoryMutationEffect::Changed,
            1,
            "actor-main",
            now.saturating_add(1),
        )
        .unwrap();
        let replay_batch = StoreMutationBatch {
            transaction_id: replay_operation.transaction_id().to_string(),
            operation: "write.procedural".to_string(),
            scope: StoreEventScope::new("agent", "owner", "local", "chat")
                .with_memory_space(identity.memory_space_id())
                .with_subject(identity.mounted_subject_id()),
            mutations: Vec::new(),
        };
        let authority = reopened.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).unwrap();
        let replay = lease
            .execute(&authority, || {
                reopened.commit_memory_mutation_operation_with_runtime_budget(
                    replay_batch,
                    &[],
                    replay_operation,
                    lease.report(),
                )
            })
            .expect("replay reopened file operation");
        let StoreMutationOperationOutcome::Replayed { receipt } = replay else {
            panic!("reopened file operation must replay")
        };
        assert_eq!(receipt, committed_receipt);
        let persisted_operation_documents = [
            MEMORY_MUTATION_RECEIPT_NAMESPACE,
            MEMORY_MUTATION_AUDIT_NAMESPACE,
        ]
        .into_iter()
        .flat_map(|namespace| {
            reopened
                .read_json_namespace(namespace)
                .expect("read persisted operation namespace")
        })
        .collect::<Vec<_>>();
        assert!(
            !serde_json::to_string(&persisted_operation_documents)
                .expect("encode persisted operation documents")
                .contains("operation-file-reopen"),
            "durable operation records must not persist the raw caller operation id"
        );
        assert_eq!(
            reopened
                .json_get::<serde_json::Value>("skill_meta", "order")
                .unwrap(),
            Some(serde_json::json!(["persisted-operation-effect"]))
        );
    }

    #[cfg(feature = "sqlite-store")]
    #[test]
    fn sqlite_store_reopen_replays_persisted_mutation_operation_receipt() {
        let store_dir = SyntheticStoreDir::create("sqlite-reopen");
        let config = StoreBackendConfig::sqlite(
            store_dir.0.join("memory.sqlite"),
            native_production_profile(),
        )
        .unwrap()
        .with_fsync(false);
        let identity = MemoryMutationOperationIdentity::new(
            "operation-sqlite-reopen",
            "memory-space-main",
            "subject-main",
            "actor-main",
            MemoryMutationOperationKind::Write,
        )
        .unwrap();
        let intent_digest =
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let now = current_unix_secs();

        let committed_receipt = {
            let platform = StorePlatform::open(config.clone()).expect("first sqlite store open");
            let operation = StoreMutationOperationPlan::new(
                identity.clone(),
                intent_digest,
                MemoryMutationEffect::Changed,
                1,
                "actor-main",
                now,
            )
            .unwrap();
            let batch = StoreMutationBatch {
                transaction_id: operation.transaction_id().to_string(),
                operation: "write.procedural".to_string(),
                scope: StoreEventScope::new("agent", "owner", "local", "chat")
                    .with_memory_space(identity.memory_space_id())
                    .with_subject(identity.mounted_subject_id()),
                mutations: vec![StoreMutation::PutJson {
                    namespace: "skill_meta".to_string(),
                    key: "disabled".to_string(),
                    value: serde_json::json!(["persisted-sqlite-operation-effect"]),
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "skill_meta".to_string(),
                    record_key: "disabled".to_string(),
                }],
            };
            let authority = platform.runtime_budget_authority();
            let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).unwrap();
            let outcome = lease
                .execute(&authority, || {
                    platform.commit_memory_mutation_operation_with_runtime_budget(
                        batch,
                        &[],
                        operation,
                        lease.report(),
                    )
                })
                .expect("commit sqlite operation");
            let StoreMutationOperationOutcome::Committed { receipt, .. } = outcome else {
                panic!("first sqlite operation must commit")
            };
            receipt
        };

        let reopened = StorePlatform::open(config).expect("reopen sqlite store");
        let replay_operation = StoreMutationOperationPlan::new(
            identity.clone(),
            intent_digest,
            MemoryMutationEffect::Changed,
            1,
            "actor-main",
            now.saturating_add(1),
        )
        .unwrap();
        let replay_batch = StoreMutationBatch {
            transaction_id: replay_operation.transaction_id().to_string(),
            operation: "write.procedural".to_string(),
            scope: StoreEventScope::new("agent", "owner", "local", "chat")
                .with_memory_space(identity.memory_space_id())
                .with_subject(identity.mounted_subject_id()),
            mutations: Vec::new(),
        };
        let authority = reopened.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).unwrap();
        let replay = lease
            .execute(&authority, || {
                reopened.commit_memory_mutation_operation_with_runtime_budget(
                    replay_batch,
                    &[],
                    replay_operation,
                    lease.report(),
                )
            })
            .expect("replay reopened sqlite operation");
        let StoreMutationOperationOutcome::Replayed { receipt } = replay else {
            panic!("reopened sqlite operation must replay")
        };
        assert_eq!(receipt, committed_receipt);
        let persisted_operation_documents = [
            MEMORY_MUTATION_RECEIPT_NAMESPACE,
            MEMORY_MUTATION_AUDIT_NAMESPACE,
        ]
        .into_iter()
        .flat_map(|namespace| {
            reopened
                .read_json_namespace(namespace)
                .expect("read persisted operation namespace")
        })
        .collect::<Vec<_>>();
        assert!(
            !serde_json::to_string(&persisted_operation_documents)
                .expect("encode persisted operation documents")
                .contains("operation-sqlite-reopen"),
            "durable operation records must not persist the raw caller operation id"
        );
        assert_eq!(
            reopened
                .json_get::<serde_json::Value>("skill_meta", "disabled")
                .unwrap(),
            Some(serde_json::json!(["persisted-sqlite-operation-effect"]))
        );
    }

    #[test]
    fn long_term_post_image_rejects_missing_request_pinned_retention_limit() {
        let batch = StoreMutationBatch {
            transaction_id: "long-term-pinned-retention".to_string(),
            operation: "long_term.correct".to_string(),
            scope: StoreEventScope::new("agent", "owner", "runtime", "test")
                .with_memory_space("space")
                .with_subject("subject"),
            mutations: vec![StoreMutation::PutJson {
                namespace:
                    crate::store_internal::schema::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE
                        .to_string(),
                key: "root".to_string(),
                value: serde_json::json!({}),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: crate::store_internal::schema::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE
                    .to_string(),
                record_key: "root".to_string(),
            }],
        };

        let error = validate_long_term_version_root_post_image(
            &batch,
            &BackendTransactionState::default(),
            None,
        )
        .expect_err("locked post-image validation must not derive its own retention cap");

        assert_eq!(
            error.stage(),
            "memory_write_transaction_long_term_root_post_image_invalid"
        );
        assert!(error.to_string().contains("request-pinned retention limit"));
    }

    #[test]
    fn generic_skill_transaction_uses_the_normal_transaction_clock_contract() {
        let scope = StoreEventScope::new("agent", "owner", "console", "chat")
            .with_memory_space("space")
            .with_subject("subject");
        let skill_batch = StoreMutationBatch {
            transaction_id: "skill-edit".to_string(),
            operation: "runtime_skill.edit".to_string(),
            scope: scope.clone(),
            mutations: vec![StoreMutation::PutBlob {
                namespace: "skills".to_string(),
                key: "runtime_skill__release".to_string(),
                value: b"typed skill".to_vec(),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "skills".to_string(),
                record_key: "runtime_skill__release".to_string(),
            }],
        };
        assert!(
            canonical_transaction_timestamp(&skill_batch, None)
                .expect("generic skills use the normal transaction clock")
                > 0
        );
        assert_eq!(
            canonical_transaction_timestamp(&skill_batch, Some(1_800_000_001))
                .expect("explicit runtime clock"),
            1_800_000_001
        );

        let mut mismatched = skill_batch;
        mismatched.mutations.push(StoreMutation::AppendEvent {
            event: Box::new(MemoryStoreEvent::new(
                "event-skill-edit",
                MemoryStoreEventKind::MemoryWrite,
                scope,
                1_800_000_000,
            )),
        });
        let mismatch = canonical_transaction_timestamp(&mismatched, Some(1_800_000_001))
            .expect_err("two timestamp authorities must not diverge");
        assert_eq!(mismatch.stage(), "memory_write_transaction_timestamp");
    }

    #[test]
    fn prepared_store_consumes_the_exact_fresh_report_identity() {
        let profile = native_production_profile();
        let config = StoreBackendConfig::in_memory(profile).expect("in-memory config");
        let now_secs = current_unix_secs();
        let preparation = StorePlatformPreparation::prepare_at(config, None, now_secs)
            .expect("prepare store admission");
        let expected_report_id = preparation.report_id.clone();
        let capacity = resolve_store_capacity(&preparation.runtime_budget_authority)
            .expect("resolve capacity");

        let (platform, _) = StorePlatform::open_prepared_at(preparation, capacity, now_secs)
            .expect("consume exact prepared admission");

        assert_eq!(
            platform.current_runtime_budget(now_secs).report_id,
            expected_report_id
        );
    }

    #[test]
    fn prepared_store_admission_cannot_be_consumed_twice() {
        let profile = native_production_profile();
        let config = StoreBackendConfig::in_memory(profile).expect("in-memory config");
        let now_secs = current_unix_secs();
        let preparation = StorePlatformPreparation::prepare_at(config, None, now_secs)
            .expect("prepare store admission");
        let replay = preparation.duplicate_for_consumption_contract_test();
        let capacity = resolve_store_capacity(&preparation.runtime_budget_authority)
            .expect("resolve capacity");

        StorePlatform::open_prepared_at(preparation, capacity, now_secs)
            .expect("consume issued admission");
        let error = match StorePlatform::open_prepared_at(replay, capacity, now_secs) {
            Ok(_) => panic!("consumed admission must not open a second engine"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "store_prepared_admission_consumed");
    }

    #[test]
    fn prepared_store_rejects_a_report_that_expired_before_open() {
        let profile = native_production_profile();
        let config = StoreBackendConfig::in_memory(profile).expect("in-memory config");
        let preparation = StorePlatformPreparation::prepare_at(config, None, 10)
            .expect("prepare store admission");
        let capacity = resolve_store_capacity(&preparation.runtime_budget_authority)
            .expect("resolve capacity");

        let error = match StorePlatform::open_prepared_at(preparation, capacity, 40) {
            Ok(_) => panic!("expired prepared identity must not be refreshed in place"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "store_prepared_admission_invalid");
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    struct ControlledResourceProbe {
        ttl_ms: u64,
        memory_available_bytes: AtomicU64,
        firmware_memory: bool,
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    impl ControlledResourceProbe {
        fn new(ttl_ms: u64, firmware_memory: bool) -> Self {
            Self {
                ttl_ms,
                memory_available_bytes: AtomicU64::new(if firmware_memory {
                    512 * 1024
                } else {
                    512 * 1024 * 1024
                }),
                firmware_memory,
            }
        }

        fn contract_memory_budget(&self) {
            self.memory_available_bytes.store(
                if self.firmware_memory {
                    128 * 1024
                } else {
                    128 * 1024 * 1024
                },
                Ordering::Release,
            );
        }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    impl RuntimeResourceProbe for ControlledResourceProbe {
        fn probe(&self, now_secs: u64) -> Result<RuntimeResourceObservation> {
            Ok(RuntimeResourceObservation {
                observed_at_unix_secs: now_secs,
                ttl_ms: self.ttl_ms,
                stale: false,
                pressure: PressureLevel::Normal,
                available_parallelism: Some(4),
                memory_total_bytes: (!self.firmware_memory).then_some(1024 * 1024 * 1024),
                memory_available_bytes: (!self.firmware_memory)
                    .then(|| self.memory_available_bytes.load(Ordering::Acquire)),
                internal_heap_free_bytes: self
                    .firmware_memory
                    .then(|| self.memory_available_bytes.load(Ordering::Acquire)),
                internal_heap_minimum_free_bytes: self.firmware_memory.then_some(64 * 1024),
                internal_heap_largest_block_bytes: self.firmware_memory.then_some(64 * 1024),
                psram_total_bytes: self.firmware_memory.then_some(8 * 1024 * 1024),
                psram_free_bytes: self.firmware_memory.then_some(4 * 1024 * 1024),
                psram_largest_block_bytes: self.firmware_memory.then_some(2 * 1024 * 1024),
                storage_total_bytes: Some(1024 * 1024 * 1024),
                storage_available_bytes: Some(512 * 1024 * 1024),
                unavailable_reason: None,
                unavailable_detail: None,
            })
        }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn admission_test_platforms(
        root: &std::path::Path,
        ttl_ms: u64,
    ) -> Vec<(String, StorePlatform, Arc<ControlledResourceProbe>)> {
        let host = native_production_profile();
        let configs = vec![
            (
                "in_memory".to_string(),
                StoreBackendConfig::in_memory(host).expect("in-memory config"),
            ),
            (
                "embedded".to_string(),
                StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory)
                    .expect("embedded config"),
            ),
            (
                "file".to_string(),
                StoreBackendConfig::file(root.join("file"), host).expect("file config"),
            ),
        ];
        #[cfg(feature = "sqlite-store")]
        let configs = configs
            .into_iter()
            .chain(std::iter::once((
                "sqlite".to_string(),
                StoreBackendConfig::sqlite(root.join("sqlite.db"), host).expect("sqlite config"),
            )))
            .collect::<Vec<_>>();
        configs
            .into_iter()
            .map(|(name, config)| {
                let firmware_memory = name == "embedded";
                let probe = Arc::new(ControlledResourceProbe::new(ttl_ms, firmware_memory));
                let platform = StorePlatform::open_with_nonproduction_probe(
                    config,
                    Arc::clone(&probe) as Arc<dyn RuntimeResourceProbe>,
                )
                .unwrap_or_else(|error| panic!("open {name} admission fixture: {error}"));
                (name, platform, probe)
            })
            .collect()
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn admission_write_request(case_name: &str) -> StoreTransactionRequest {
        StoreTransactionRequest::new(
            format!("admission-write-{case_name}"),
            Vec::new(),
            vec![StoreEngineMutation::PutJson {
                namespace: "admission_contract".to_string(),
                key: case_name.to_string(),
                value: serde_json::json!({"case": case_name}),
            }],
            None,
        )
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    fn empty_scoped_replace() -> StoreScopedProjectionReplaceRequest {
        StoreScopedProjectionReplaceRequest {
            scope: StoreScopedProjectionScope::subject("space:admission", "subject:admission")
                .expect("scope"),
            json_namespaces: Vec::new(),
            json_docs: Vec::new(),
            events: Vec::new(),
            preserve_protected_owner_state: false,
        }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn exact_evidence_read_requires_the_active_pinned_runtime_budget_report() {
        let config = StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("in-memory config");
        let platform = StorePlatform::open(config).expect("open exact evidence store");
        let other_platform = StorePlatform::open(
            StoreBackendConfig::embedded(ProfileId::EspEmbeddedSdk)
                .expect("other authority config"),
        )
        .expect("open other exact evidence authority");
        let other_report = other_platform.current_runtime_budget(current_unix_secs());
        let request = StoreGovernedEvidenceExactReadRequest {
            memory_space_id: "space:exact-admission".to_string(),
            mounted_subject_id: "subject:exact-admission".to_string(),
            owner_keys: vec!["owner:absent".to_string()],
            include_all_manifest_bindings: false,
            allow_missing_manifest_for_empty_scope: true,
        };
        let lease =
            crate::RuntimeBudgetLease::issue(Arc::clone(&platform.runtime_budget_authority))
                .expect("issue exact evidence lease");
        let pinned = lease.report().clone();

        let error = platform
            .read_governed_evidence_exact(&pinned, request.clone())
            .expect_err("exact evidence read without an active lease must fail");
        assert_eq!(error.stage(), "governed_evidence_exact_read_admission");

        lease
            .execute(&platform.runtime_budget_authority, || {
                let result = platform.read_governed_evidence_exact(&pinned, request.clone())?;
                assert_eq!(result.entry_count, 1);
                assert!(result.reads[0].owner.is_none());

                let error = platform
                    .read_governed_evidence_exact(&other_report, request)
                    .expect_err("another authority report id must fail under the active lease");
                assert_eq!(error.stage(), "governed_evidence_exact_read_admission");
                Ok(())
            })
            .expect("execute exact evidence lease contract");
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn all_backend_write_and_typed_replace_fences_reject_admission_after_ttl() {
        let root = std::env::temp_dir().join(format!(
            "beetle-memory-admission-ttl-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let cases = admission_test_platforms(&root, 1_000);
        let admissions = cases
            .iter()
            .map(|(name, platform, _)| {
                (
                    name.clone(),
                    platform
                        .current_store_transaction_admission()
                        .expect("fresh admission"),
                )
            })
            .collect::<BTreeMap<_, _>>();
        std::thread::sleep(std::time::Duration::from_millis(1_100));

        for (name, platform, _) in &cases {
            let admission = admissions.get(name).expect("admission");
            let write_error = platform
                .engine
                .commit_transaction_admitted(&admission_write_request(name), admission)
                .expect_err("expired admission must fail inside the backend write fence");
            assert_eq!(
                write_error.stage(),
                "memory_write_transaction_resource_admission",
                "backend={name}: {write_error}"
            );
            let replace_error = platform
                .engine
                .replace_scoped_projection(&empty_scoped_replace(), admission)
                .expect_err("expired admission must fail inside the typed replace fence");
            assert_eq!(
                replace_error.stage(),
                "memory_write_transaction_resource_admission",
                "backend={name}: {replace_error}"
            );
            assert!(platform
                .engine
                .get_json_value("admission_contract", name)
                .expect("read admission fixture")
                .is_none());
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    #[test]
    fn all_backend_fences_reject_a_fresh_but_superseded_authority_report() {
        let root = std::env::temp_dir().join(format!(
            "beetle-memory-admission-authority-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let cases = admission_test_platforms(&root, 30_000);

        for (name, platform, probe) in &cases {
            let admission = platform
                .current_store_transaction_admission()
                .expect("fresh admission");
            probe.contract_memory_budget();
            platform
                .refresh_runtime_resource_snapshot(current_unix_secs())
                .expect("refresh contracted authority report");
            let error = platform
                .engine
                .commit_transaction_admitted(&admission_write_request(name), &admission)
                .expect_err("superseded report must fail inside the backend write fence");
            assert_eq!(
                error.stage(),
                "memory_write_transaction_resource_admission",
                "backend={name}: {error}"
            );
            assert!(error
                .to_string()
                .contains("current exact runtime authority"));
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trust_boundary_requires_old_root_removal_and_unique_new_root() {
        use bm_core::task_execution::{
            TaskLearningCandidateState, TaskLearningKind, TaskLearningRoute, TaskRunStatus,
        };

        let before = TaskLearningRecord {
            learning_id: "learning".into(),
            source_channel: "old-channel".into(),
            source_chat_id: "old-chat".into(),
            run_id: "run".into(),
            step_id: String::new(),
            kind: TaskLearningKind::DurableFact,
            route: TaskLearningRoute::Pending,
            run_status: TaskRunStatus::Completed,
            topic: "topic".into(),
            summary: "summary".into(),
            content: "content".into(),
            memory_kind: None,
            review_summary: String::new(),
            source_artifact_ids: Vec::new(),
            provenance: String::new(),
            archive_note_name: String::new(),
            route_detail: String::new(),
            candidate_state: Some(TaskLearningCandidateState::Observed),
            candidate_state_updated_at: 1,
            last_failure_reason: String::new(),
            observed_at: 1,
        };
        let mut next = before.clone();
        next.source_channel = "new-channel".into();
        next.source_chat_id = "new-chat".into();
        next.observed_at = 2;
        let next_value = serde_json::to_value(&next).unwrap();
        let owner_digest = RecallIndexAddress::json("task_learning", "learning", 1, 0, &next_value)
            .unwrap()
            .content_sha256;
        let binding = RecallOwnerScopeBinding::build(
            "space",
            "subject",
            "json",
            "task_learning",
            "learning",
            &owner_digest,
        )
        .unwrap();
        let binding_mutation = StoreMutation::PutJson {
            namespace: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.into(),
            key: binding.physical_key.clone(),
            value: serde_json::to_value(binding).unwrap(),
            event_kind: MemoryStoreEventKind::MemoryMaintenance,
            plane: RECALL_OWNER_SCOPE_BINDING_NAMESPACE.into(),
            record_key: "binding".into(),
        };
        let old_root =
            TaskLearningByChatIndex::build(2, "space", "old-channel", "old-chat", []).unwrap();
        let new_root = TaskLearningByChatIndex::build(
            1,
            "space",
            "new-channel",
            "new-chat",
            [RecallIndexAddress::json("task_learning", "learning", 2, 2, &next_value).unwrap()],
        )
        .unwrap();
        let root_mutation = |root: TaskLearningByChatIndex| StoreMutation::PutJson {
            namespace: TaskLearningByChatIndex::NAMESPACE.into(),
            key: root.physical_key.clone(),
            value: serde_json::to_value(root).unwrap(),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: TaskLearningByChatIndex::NAMESPACE.into(),
            record_key: "root".into(),
        };
        let owner_mutation = StoreMutation::PutJson {
            namespace: "task_learning".into(),
            key: "learning".into(),
            value: next_value,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "task_learning".into(),
            record_key: "learning".into(),
        };
        let batch = StoreMutationBatch {
            transaction_id: "scope-transfer".into(),
            operation: "test".into(),
            scope: StoreEventScope::new("agent", "subject", "new-channel", "new-chat")
                .with_memory_space("space"),
            mutations: vec![
                owner_mutation.clone(),
                binding_mutation,
                root_mutation(old_root.clone()),
                root_mutation(new_root.clone()),
            ],
        };
        let read_before = |namespace: &str, key: &str| {
            Ok((namespace == "task_learning" && key == "learning")
                .then(|| serde_json::to_value(&before).unwrap()))
        };
        validate_recall_index_mutation_closure(&batch, read_before, |_, _| Ok(None)).unwrap();

        let mut missing_old = batch.clone();
        missing_old.mutations.retain(|mutation| !matches!(mutation,
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE && key == &old_root.physical_key));
        assert!(
            validate_recall_index_mutation_closure(&missing_old, read_before, |_, _| Ok(None))
                .is_err()
        );

        let wrong_root = TaskLearningByChatIndex::build(
            1,
            "space",
            "wrong-channel",
            "wrong-chat",
            [RecallIndexAddress::json(
                "task_learning",
                "learning",
                2,
                2,
                match &owner_mutation {
                    StoreMutation::PutJson { value, .. } => value,
                    _ => unreachable!(),
                },
            )
            .unwrap()],
        )
        .unwrap();
        let mut wrong = batch;
        wrong.mutations.push(root_mutation(wrong_root));
        assert!(
            validate_recall_index_mutation_closure(&wrong, read_before, |_, _| Ok(None)).is_err()
        );
    }
}
