//! Process-level entry runtime for Beetle Memory.

mod accepted_tcp;
mod auth;
mod config;
mod console;
mod error;
mod governance_coordinator;
mod governance_model;
mod governance_model_client;
mod http_ingress;
mod idempotency;
mod network_front;
mod response;
mod runtime;
mod source;

pub use accepted_tcp::EntryAcceptedTcpStream;
pub use auth::{
    EntryAuthConfig, EntryAuthDecision, EntryBearerPrincipal, EntryLocalTransport,
    EntryOperationCapability,
};
pub use config::{
    EntryCapabilityItem, EntryCapabilityView, EntryIdempotencyConfig, EntryIdentity, EntryScope,
    EntryTransportConfig,
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
    EntryConsoleWorkbenchArchiveRestore, EntryConsoleWorkbenchBenchmarkWall,
    EntryConsoleWorkbenchFacetInspector, EntryConsoleWorkbenchProceduralEvolution,
    EntryConsoleWorkbenchProjectionInspector, EntryConsoleWorkbenchRecallInspector,
    EntryConsoleWorkbenchReport, EntryConsoleWorkbenchSkillRef, EntryConsoleWorkbenchSoulHealth,
    EntryConsoleWorkbenchStatus,
};
pub use error::EntryErrorKey;
pub use governance_coordinator::{
    EntryGovernanceCoordinatorReport, EntryGovernanceCoordinatorState,
};
pub use governance_model::{
    EntryGovernanceModelAuthMode, EntryGovernanceModelConfigUpdate, EntryGovernanceModelConfigView,
    EntryGovernanceModelExecutionBinding, EntryGovernanceModelProbePlan,
    EntryGovernanceModelProtocol,
};
pub use governance_model_client::{
    ConfiguredGovernanceLlmClient, GovernanceModelConnectionProbe, GovernanceModelConnectionReport,
};
#[cfg(feature = "governance-model-client-std")]
pub use governance_model_client::{
    ReqwestGovernanceLlmHttpClient, ReqwestGovernanceModelConnectionProbe,
};
pub use http_ingress::{
    read_authorized_http_request, EntryAuthorizedHttpRequest, EntryHttpAuthorization,
    EntryHttpIngressError, EntryHttpIngressErrorKind, EntryHttpIngressLimits, EntryHttpRequestHead,
};
pub use idempotency::EntryIdempotencyCache;
pub use network_front::{
    EntryTcpDispatchOutcome, EntryTcpNetworkFront, EntryTcpNetworkFrontConfig,
};
pub use response::{EntryResponse, EntryResponseStatus};
pub use runtime::{
    entry_capability_view, EntryRuntime, EntryRuntimeBaseConfig, EntryRuntimeBudgetLease,
    EntryRuntimeConfig, EntryRuntimeFactory, EntryRuntimeManager, EntryRuntimeScope,
};
pub use source::EntryTransportContext;

pub const fn entry_governance_model_client_compiled() -> bool {
    cfg!(feature = "governance-model-client-std")
}
