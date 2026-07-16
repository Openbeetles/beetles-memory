use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryRuntimeBaseConfig, EntryTransportConfig,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, PressureLevel, ProfileId, StoreBackendConfig,
};

use crate::{GatewayError, GatewayProviderConfig, GatewayScopeResolverConfig};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayConfig {
    pub entry: EntryRuntimeBaseConfig,
    pub server: GatewayServerConfig,
    pub providers: BTreeMap<String, GatewayProviderConfig>,
    pub default_provider: String,
    pub scope: GatewayScopeResolverConfig,
    pub runtime_cache: GatewayRuntimeCacheConfig,
    pub projection: GatewayProjectionConfig,
    pub maintenance: GatewayMaintenanceConfig,
    pub audit: GatewayAuditConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayServerConfig {
    pub bind_addr: String,
    pub memory_required: bool,
    pub allowed_origins: Vec<String>,
}

impl Default for GatewayServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:8787".to_string(),
            memory_required: true,
            allowed_origins: vec![
                "http://127.0.0.1:8787".to_string(),
                "http://localhost:8787".to_string(),
                "http://[::1]:8787".to_string(),
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayRuntimeCacheConfig {
    pub max_runtimes: usize,
}

impl Default for GatewayRuntimeCacheConfig {
    fn default() -> Self {
        Self { max_runtimes: 256 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayProjectionConfig {
    pub pressure: PressureLevel,
}

impl Default for GatewayProjectionConfig {
    fn default() -> Self {
        Self {
            pressure: PressureLevel::Normal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayMaintenanceConfig {
    pub enabled: bool,
}

impl Default for GatewayMaintenanceConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayAuditConfig {
    pub enabled: bool,
    pub record_raw_projection: bool,
    pub raw_projection_diagnostic_path: Option<PathBuf>,
    pub raw_projection_retention_limit: usize,
    pub record_full_request_body: bool,
    pub record_full_response_body: bool,
}

impl Default for GatewayAuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            record_raw_projection: false,
            raw_projection_diagnostic_path: None,
            raw_projection_retention_limit: 32,
            record_full_request_body: false,
            record_full_response_body: false,
        }
    }
}

impl GatewayConfig {
    pub fn default_for_local_dev() -> Self {
        let mut providers = BTreeMap::new();
        providers.insert(
            "local".to_string(),
            GatewayProviderConfig::openai_compatible("http://127.0.0.1:8000/v1", None),
        );
        let mut capability = MemoryCapabilityPolicy::strict_profile();
        capability.communication_adapter_enabled = true;
        let store = StoreBackendConfig::in_memory(host_production_gateway_profile())
            .expect("host production gateway profile must support in-memory store")
            .with_fsync(false);
        Self {
            entry: EntryRuntimeBaseConfig {
                store,
                transports: llm_gateway_transport_config(),
                auth: EntryAuthConfig::disabled_for_local(),
                idempotency: EntryIdempotencyConfig { max_keys: 1024 },
                privacy: MemoryPrivacyPolicy::standard_private_boundary(),
                capability,
            },
            server: GatewayServerConfig::default(),
            providers,
            default_provider: "local".to_string(),
            scope: GatewayScopeResolverConfig::default_for_local_dev(),
            runtime_cache: GatewayRuntimeCacheConfig::default(),
            projection: GatewayProjectionConfig::default(),
            maintenance: GatewayMaintenanceConfig::default(),
            audit: GatewayAuditConfig::default(),
        }
    }

    pub fn validate(&self) -> crate::Result<()> {
        if self.entry.transports != llm_gateway_transport_config() {
            return Err(GatewayError::invalid_config(
                "LLM Gateway entry transports must enable only llm_gateway_server",
            ));
        }
        let capability_view = bm_entry::entry_capability_view(
            self.entry.store.profile(),
            &self.entry.capability,
            &self.entry.privacy,
            &self.entry.transports,
        )
        .map_err(|error| {
            GatewayError::invalid_config(format!(
                "LLM Gateway profile capability resolution failed: {error}"
            ))
        })?;
        if !capability_view.llm_gateway_server.profile_allowed
            || !capability_view.llm_gateway_server.visible
        {
            return Err(GatewayError::invalid_config(format!(
                "profile {} does not authorize the LLM Gateway server entry",
                self.entry.store.profile().as_str()
            )));
        }
        if self.runtime_cache.max_runtimes == 0 {
            return Err(GatewayError::invalid_config(
                "runtime_cache.max_runtimes must be greater than zero",
            ));
        }
        if !self.providers.contains_key(&self.default_provider) {
            return Err(GatewayError::invalid_config(
                "default_provider must exist in providers",
            ));
        }
        if self.server.bind_addr.trim().is_empty() {
            return Err(GatewayError::invalid_config(
                "server.bind_addr must not be empty",
            ));
        }
        let bind_addr = self.server.bind_addr.parse::<SocketAddr>().map_err(|_| {
            GatewayError::invalid_config("server.bind_addr must be a socket address")
        })?;
        if !bind_addr.ip().is_loopback() && !self.entry.auth.has_bearer_verifier() {
            return Err(GatewayError::invalid_config(
                "non-loopback gateway bind requires a configured bearer verifier",
            ));
        }
        if self.server.allowed_origins.is_empty()
            || self
                .server
                .allowed_origins
                .iter()
                .any(|origin| !is_exact_local_origin(origin))
        {
            return Err(GatewayError::invalid_config(
                "server.allowed_origins must contain only exact local http(s) origins",
            ));
        }
        if self.audit.enabled && self.audit.record_raw_projection {
            if self.audit.raw_projection_diagnostic_path.is_none() {
                return Err(GatewayError::invalid_config(
                    "audit.record_raw_projection requires audit.raw_projection_diagnostic_path",
                ));
            }
            if self.audit.raw_projection_retention_limit == 0 {
                return Err(GatewayError::invalid_config(
                    "audit.raw_projection_retention_limit must be greater than zero",
                ));
            }
        }
        Ok(())
    }
}

pub const fn llm_gateway_transport_config() -> EntryTransportConfig {
    EntryTransportConfig {
        cli: false,
        http_server: false,
        wss_client: false,
        wss_server: false,
        mcp_server: false,
        a2a_bridge: false,
        llm_gateway_server: true,
    }
}

#[cfg(target_os = "linux")]
const fn host_production_gateway_profile() -> ProfileId {
    ProfileId::ServerLinuxMemoryGateway
}

#[cfg(target_os = "macos")]
const fn host_production_gateway_profile() -> ProfileId {
    ProfileId::DesktopMacosStandaloneMemory
}

#[cfg(target_os = "windows")]
const fn host_production_gateway_profile() -> ProfileId {
    panic!("bm-llm-gateway has no Windows production profile")
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!("bm-llm-gateway has no production profile for this target OS");

fn is_exact_local_origin(origin: &str) -> bool {
    if origin.trim() != origin || origin.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return false;
    }
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https")
        || authority.is_empty()
        || authority
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'?' | b'#' | b'@'))
    {
        return false;
    }
    let host = if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return false;
        };
        if !suffix.is_empty() && !suffix.strip_prefix(':').is_some_and(valid_origin_port) {
            return false;
        }
        host
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if !valid_origin_port(port) {
            return false;
        }
        host
    } else {
        authority
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn valid_origin_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|port| port != 0)
}

#[cfg(test)]
mod host_profile_tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn production_gateway_profile_is_linux_memory_gateway() {
        assert_eq!(
            host_production_gateway_profile(),
            ProfileId::ServerLinuxMemoryGateway
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn production_gateway_profile_is_macos_standalone_memory() {
        assert_eq!(
            host_production_gateway_profile(),
            ProfileId::DesktopMacosStandaloneMemory
        );
    }
}
