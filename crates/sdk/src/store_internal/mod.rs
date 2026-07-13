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
mod read_snapshot;
mod schema;
mod snapshot;
#[cfg(feature = "sqlite-store")]
mod sqlite;
mod transaction;

pub(crate) use config::{enforce_event_key_budget, enforce_logical_key_budget, store_budget_error};
pub use config::{
    profile_memory_system_kind, StoreBackendConfig, StoreBackendKind, StoreCapacityBudget,
    StorePathBudget, StoreRepairPolicy,
};
#[cfg(feature = "nonproduction-replay-harness")]
pub use embedded::EmbeddedStoreEngine;
pub use engine::{StoreEngine, StoreSnapshotReplaceReport};
pub use error::{StoreOpenReport, StoreRepairReport};
pub use event::{MemoryStoreEvent, MemoryStoreEventKind, StoreEventLog, StoreEventScope};
#[cfg(feature = "nonproduction-replay-harness")]
pub use file::FileStoreEngine;
pub use in_memory::InMemoryStoreEngine;
pub use mutation::{
    StoreJsonPrecondition, StoreMutation, StoreMutationBatch, StoreMutationBatchReport,
    StoreMutationBudgetReport,
};
#[cfg(feature = "nonproduction-replay-harness")]
pub use platform::GovernedRecallSnapshot;
pub use platform::StorePlatform;
pub use schema::{StoreSchemaManifest, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION};
pub use snapshot::{
    StoreSnapshot, StoreSnapshotBlob, StoreSnapshotExportReport, StoreSnapshotImportReport,
    StoreSnapshotJsonDoc,
};
#[cfg(all(feature = "sqlite-store", feature = "nonproduction-replay-harness"))]
pub use sqlite::SqliteStoreEngine;
pub(crate) use transaction::GraphRepairAuthority;
#[cfg(feature = "nonproduction-replay-harness")]
pub use transaction::{
    StoreBlobAddress, StoreConsistentBlobRead, StoreConsistentJsonRead, StoreConsistentReadRequest,
    StoreConsistentReadResult, StoreJsonAddress,
};
pub use transaction::{
    StoreConsistentNamespaceReadRequest, StoreConsistentNamespaceReadResult, StoreEngineMutation,
    StoreReadReceipt, StoreTransactionReport, StoreTransactionRequest,
};
