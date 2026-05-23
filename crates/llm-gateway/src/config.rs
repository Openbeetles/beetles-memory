use std::collections::BTreeMap;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryRuntimeBaseConfig, EntryStoreConfig,
    EntryTransportConfig,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, PressureLevel, ProfileId, StoreBackendKind,
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
    pub loopback_only: bool,
    pub memory_required: bool,
    pub require_token_for_remote: bool,
}

impl Default for GatewayServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:8787".to_string(),
            loopback_only: true,
            memory_required: true,
            require_token_for_remote: true,
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
    pub record_full_request_body: bool,
    pub record_full_response_body: bool,
}

impl Default for GatewayAuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            record_raw_projection: false,
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
        Self {
            entry: EntryRuntimeBaseConfig {
                profile: ProfileId::ServerLinuxDevFull,
                store: EntryStoreConfig {
                    backend: StoreBackendKind::InMemory,
                    data_path: None,
                    fsync: false,
                },
                transports: EntryTransportConfig::all_enabled(),
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
        if self.server.loopback_only && !is_loopback_bind_addr(&self.server.bind_addr) {
            return Err(GatewayError::invalid_config(
                "server.loopback_only requires a loopback bind address",
            ));
        }
        if !self.server.loopback_only && !self.server.require_token_for_remote {
            return Err(GatewayError::invalid_config(
                "remote gateway requires token enforcement",
            ));
        }
        Ok(())
    }
}

fn is_loopback_bind_addr(bind_addr: &str) -> bool {
    let trimmed = bind_addr.trim();
    let host = if let Some(rest) = trimmed.strip_prefix('[') {
        rest.split_once(']')
            .map(|(host, _)| host)
            .unwrap_or(trimmed)
    } else {
        trimmed
            .rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or(trimmed)
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}
