//! Process-level entry runtime for Beetle Memory.

mod auth;
mod config;
mod console;
mod error;
mod idempotency;
mod response;
mod runtime;
mod source;

pub use auth::{EntryAuthConfig, EntryAuthDecision};
pub use config::{
    EntryCapabilityItem, EntryCapabilityView, EntryIdempotencyConfig, EntryIdentity, EntryScope,
    EntryStoreConfig, EntryTransportConfig,
};
pub use console::{
    EntryConsoleCapabilityRow, EntryConsoleDevice, EntryConsoleDeviceCreate,
    EntryConsoleDeviceKeyReport, EntryConsoleDeviceUpdate, EntryConsoleEvent, EntryConsoleKv,
    EntryConsoleLlmGateway, EntryConsoleLlmGatewayProtocol, EntryConsoleLlmGatewayRuleExport,
    EntryConsoleLlmGatewaySmokeCheck, EntryConsoleLlmGatewaySmokeRunReport,
    EntryConsoleMemoryBenchmarkBaseline, EntryConsoleMemoryBenchmarkClassCoverage,
    EntryConsoleMemoryBenchmarkFailure, EntryConsoleMemoryBenchmarkMissingClass,
    EntryConsoleMemoryBenchmarkReport, EntryConsoleMetric, EntryConsoleOverview,
    EntryConsoleRuntimeShape, EntryConsoleRuntimeSkillEdit, EntryConsoleSession,
    EntryConsoleSkillDetail, EntryConsoleSkillList, EntryConsoleSkillMutation,
    EntryConsoleSkillSetEnabled, EntryConsoleSkillSummary, EntryConsoleState,
    EntryConsoleSystemInfo, EntryConsoleTransport, EntryConsoleTransportUpdate,
    EntryConsoleWorkbenchBenchmarkWall, EntryConsoleWorkbenchProceduralEvolution,
    EntryConsoleWorkbenchProjectionInspector, EntryConsoleWorkbenchRecallInspector,
    EntryConsoleWorkbenchReport, EntryConsoleWorkbenchSkillRef, EntryConsoleWorkbenchSoulHealth,
    EntryConsoleWorkbenchStatus, EntryConsoleWorkbenchVaultMigration,
};
pub use error::EntryErrorKey;
pub use idempotency::EntryIdempotencyCache;
pub use response::{EntryResponse, EntryResponseStatus};
pub use runtime::{
    entry_capability_view, EntryRuntime, EntryRuntimeBaseConfig, EntryRuntimeConfig,
    EntryRuntimeFactory, EntryRuntimeManager, EntryRuntimeScope,
};
pub use source::EntryTransportContext;
