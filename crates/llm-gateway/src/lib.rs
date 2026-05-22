//! LLM protocol gateway contracts for Beetle Memory.

mod audit;
mod config;
mod error;
mod maintenance;
mod openai;
mod provider;
mod runtime;
mod scope;
mod server;

pub use audit::{
    GatewayAuditOutcome, GatewayAuditReport, GatewayAuditStage, GatewayAuditStageReport,
};
pub use config::{
    GatewayAuditConfig, GatewayConfig, GatewayMaintenanceConfig, GatewayProjectionConfig,
    GatewayRuntimeCacheConfig, GatewayServerConfig,
};
pub use error::{GatewayError, GatewayErrorKey, Result};
#[cfg(feature = "client-reqwest")]
pub use maintenance::ReqwestGatewayLlmHttpClient;
pub use maintenance::{OpenAiGatewayServices, OpenAiMaintenanceLlmClient};
#[cfg(feature = "client-reqwest")]
pub use openai::ReqwestOpenAiCompatibleUpstream;
pub use openai::{
    handle_openai_request, handle_openai_request_with_services, OpenAiCompatibleUpstream,
    OpenAiGatewayBody, OpenAiGatewayMethod, OpenAiGatewayRequest, OpenAiGatewayResponse,
    OpenAiSseBody, OpenAiSseStream, OpenAiUpstreamRequest, OpenAiUpstreamResponse,
};
pub use provider::{GatewayProviderConfig, GatewayProviderKind};
pub use runtime::GatewayRuntime;
pub use scope::{
    GatewayScopeRequest, GatewayScopeResolution, GatewayScopeResolver, GatewayScopeResolverConfig,
    GatewayTrustedHeaders,
};
pub use server::{serve_openai_http_stream, serve_openai_http_stream_with_services};
