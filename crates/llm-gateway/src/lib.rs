//! LLM protocol gateway contracts for Beetle Memory.

mod audit;
mod config;
mod error;
mod provider;
mod runtime;
mod scope;

pub use audit::{
    GatewayAuditOutcome, GatewayAuditReport, GatewayAuditStage, GatewayAuditStageReport,
};
pub use config::{
    GatewayAuditConfig, GatewayConfig, GatewayRuntimeCacheConfig, GatewayServerConfig,
};
pub use error::{GatewayError, GatewayErrorKey, Result};
pub use provider::{GatewayProviderConfig, GatewayProviderKind};
pub use runtime::GatewayRuntime;
pub use scope::{
    GatewayScopeRequest, GatewayScopeResolution, GatewayScopeResolver, GatewayScopeResolverConfig,
    GatewayTrustedHeaders,
};
