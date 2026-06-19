//! Store backends for Beetle Memory.
//!
//! `bm-store` owns the SDK-facing store implementations, schema manifests,
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
mod schema;
mod snapshot;
#[cfg(feature = "sqlite-store")]
mod sqlite;

pub(crate) use config::{enforce_event_key_budget, enforce_logical_key_budget, store_budget_error};
pub use config::{
    profile_is_esp, profile_memory_system_kind, StoreBackendConfig, StoreBackendKind,
    StoreCapacityBudget, StorePathBudget, StoreRepairPolicy,
};
pub use engine::{StoreEngine, StoreSnapshotReplaceReport};
pub use error::{StoreError, StoreOpenReport, StoreRepairReport};
pub use event::{MemoryStoreEvent, MemoryStoreEventKind, StoreEventLog, StoreEventScope};
pub use in_memory::InMemoryStoreEngine;
pub use mutation::{
    StoreMutation, StoreMutationBatch, StoreMutationBatchReport, StoreMutationBudgetReport,
};
pub use platform::StorePlatform;
pub use schema::{StoreSchemaManifest, STORE_SCHEMA_ID, STORE_SCHEMA_VERSION};
pub use snapshot::{
    StoreSnapshot, StoreSnapshotBlob, StoreSnapshotExportReport, StoreSnapshotImportReport,
    StoreSnapshotJsonDoc,
};
