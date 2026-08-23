//! SDK-private store backends for Beetle Memory.
//!
//! This module owns the SDK persistence implementations, schema manifests,
//! event log contract, snapshots, and repair reports. Integrators configure a
//! backend and capacity profile; they do not define Beetle Memory persistence
//! semantics.

mod config;
mod embedded;
mod engine;
mod error;
mod event;
mod file;
mod in_memory;
mod mutation;
mod platform;
pub(crate) mod post_turn_governance;
pub(crate) mod recall_index;
pub(crate) mod recall_read;
pub(crate) mod schema;
mod snapshot;
#[cfg(feature = "sqlite-store")]
mod sqlite;
pub(crate) mod subject_soul;
mod transaction;

pub(crate) use config::{enforce_event_key_budget, enforce_logical_key_budget, store_budget_error};
pub use config::{
    profile_memory_system_kind, StoreBackendConfig, StoreBackendKind, StoreCapacityBudget,
    StorePathBudget, StoreRepairPolicy,
};
#[cfg(feature = "nonproduction-replay-harness")]
pub use embedded::EmbeddedStoreEngine;
#[cfg(feature = "nonproduction-replay-harness")]
pub use engine::StoreSnapshotReplaceReport;
pub(crate) use engine::{materialize_metric_event_source, StoreMetricEventSourceRead};
pub use engine::{
    StoreEngine, StoreScopedProjection, StoreScopedProjectionReplaceReport,
    StoreScopedProjectionReplaceRequest, StoreScopedProjectionRequest, StoreScopedProjectionScope,
};
pub use error::{StoreOpenReport, StoreRepairReport};
pub use event::{
    MemoryStoreEvent, MemoryStoreEventKind, StoreEventLog, StoreEventScope,
    StorePhysicalOwningScope,
};
#[cfg(feature = "nonproduction-replay-harness")]
pub use file::FileStoreEngine;
pub use in_memory::InMemoryStoreEngine;
pub use mutation::{
    StoreBlobPrecondition, StoreJsonPrecondition, StoreMutation, StoreMutationBatch,
    StoreMutationBatchReport, StoreMutationBudgetReport,
};
pub use platform::StorePlatform;
#[cfg(feature = "nonproduction-replay-harness")]
pub(crate) use platform::StorePlatformPreparation;
pub(crate) use platform::{
    canonical_subject_soul_full_intent_digest, materialize_runtime_lifecycle_store_event,
    snapshot_json_requires_private_export, snapshot_key_requires_private_export,
    snapshot_namespace_requires_private_export, transcript_derived_ref_storage_key,
    transcript_turn_storage_key, validate_scoped_projection_governed_closure,
    RuntimeLifecycleStoreBinding, StoreMutationOperationOutcome, StoreMutationOperationPlan,
    StoreMutationOperationPreflight, StoreOwnerMutationPlan, GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
    GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE,
};
pub(crate) use schema::{
    governed_evidence_source_claim_manifest_key,
    validate_governed_evidence_source_claim_scope_closure, GovernedEvidenceOwnerClaimBinding,
    GovernedEvidenceSourceClaimManifest, GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
    LONG_TERM_HEAD_MANIFEST_NAMESPACE, LONG_TERM_VERSION_MATERIAL_NAMESPACE,
    LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE, RUNTIME_SKILL_RECORD_NAMESPACE,
    RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
};
pub use schema::{StoreSchemaManifest, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION};
#[cfg(any(test, feature = "nonproduction-replay-harness"))]
pub use snapshot::StoreSnapshotBlob;
pub use snapshot::{StoreSnapshot, StoreSnapshotJsonDoc};
#[cfg(feature = "nonproduction-replay-harness")]
pub use snapshot::{StoreSnapshotExportReport, StoreSnapshotImportReport};
#[cfg(all(feature = "sqlite-store", feature = "nonproduction-replay-harness"))]
pub use sqlite::SqliteStoreEngine;
pub(crate) use transaction::StoreReadReceipt;
pub(crate) use transaction::{
    scoped_projection_json_addresses, GraphRepairAuthority, StoreGovernedEvidenceExactReadRequest,
    StoreTransactionAdmission,
};
#[cfg(all(test, feature = "nonproduction-replay-harness"))]
pub(crate) use transaction::{
    StoreAdmissionAuthority, StoreBoundedKnownBlobRead, StoreBoundedKnownJsonRead,
    StoreBoundedKnownKeyReadResult, StoreImmutableReadSession, StoreReadSessionState,
};
#[cfg(feature = "nonproduction-replay-harness")]
pub use transaction::{
    StoreBlobAddress, StoreConsistentBlobRead, StoreConsistentJsonRead, StoreConsistentReadRequest,
    StoreConsistentReadResult, StoreJsonAddress,
};
pub use transaction::{StoreEngineMutation, StoreTransactionReport, StoreTransactionRequest};
