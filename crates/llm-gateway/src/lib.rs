//! LLM protocol gateway contracts for Beetle Memory.

mod agent_tools;
mod audit;
mod budget_io;
mod config;
mod error;
mod http_front;
mod maintenance;
mod ollama;
mod ollama_passthrough;
mod ollama_privacy;
mod openai;
mod projection;
mod provider;
mod provider_probe;
mod runtime;
mod scope;
mod server;

pub use audit::{
    GatewayAuditOutcome, GatewayAuditReport, GatewayAuditStage, GatewayAuditStageReport,
    GatewayProjectionAuditRecord, GatewayProjectionAuditStatus,
};
pub use budget_io::GatewayUpstreamResponseBudget;
pub use config::{
    llm_gateway_transport_config, GatewayAuditConfig, GatewayConfig, GatewayProjectionConfig,
    GatewayRuntimeCacheConfig, GatewayServerConfig,
};
pub use error::{GatewayError, GatewayErrorKey, Result};
pub use http_front::{GatewayHttpConnectionHandler, GatewayHttpFront, GatewayHttpFrontConfig};
#[cfg(feature = "client-reqwest")]
pub use ollama::ReqwestOllamaNativeUpstream;
pub use ollama::{
    handle_ollama_request, OllamaGatewayBody, OllamaGatewayMethod, OllamaGatewayRequest,
    OllamaGatewayResponse, OllamaNativeUpstream, OllamaNdjsonBody, OllamaNdjsonStream,
    OllamaUpstreamRequest, OllamaUpstreamResponse,
};
pub use ollama_passthrough::{
    classify_ollama_route, OllamaKnownEndpoint, OllamaPassthroughRequest, OllamaRouteAction,
    OllamaRouteDecision,
};
#[cfg(feature = "client-reqwest")]
pub use openai::ReqwestOpenAiCompatibleUpstream;
pub use openai::{
    handle_openai_request, OpenAiCompatibleUpstream, OpenAiGatewayBody, OpenAiGatewayMethod,
    OpenAiGatewayRequest, OpenAiGatewayResponse, OpenAiSseBody, OpenAiSseStream,
    OpenAiUpstreamRequest, OpenAiUpstreamResponse,
};
pub use provider::{GatewayProviderConfig, GatewayProviderKind};
pub use provider_probe::{
    probe_openai_provider_capabilities, GatewayModelCapabilityReport,
    GatewayProviderCapabilityReport,
};
pub use runtime::{GatewayRequestBudgetContext, GatewayRuntime};
pub use scope::{
    GatewayOllamaAppScopeConfig, GatewayScopeRequest, GatewayScopeResolution, GatewayScopeResolver,
    GatewayScopeResolverConfig, GatewayTrustedHeaders,
};
pub use server::{
    serve_llm_gateway_http_accepted_stream, serve_llm_gateway_http_accepted_stream_in_request,
    serve_ollama_http_accepted_stream, serve_openai_http_accepted_stream,
    GatewayHttpRequestBindings,
};
