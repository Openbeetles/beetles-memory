use bm_sdk::{AdapterTransportVisibility, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntryIdentity {
    pub agent_id: String,
    pub owner_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntryScope {
    pub channel: String,
    pub chat_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryTransportConfig {
    pub cli: bool,
    pub http_server: bool,
    pub wss_client: bool,
    pub wss_server: bool,
    pub mcp_server: bool,
    pub a2a_bridge: bool,
    pub llm_gateway_server: bool,
}

impl EntryTransportConfig {
    pub const fn all_disabled() -> Self {
        Self {
            cli: false,
            http_server: false,
            wss_client: false,
            wss_server: false,
            mcp_server: false,
            a2a_bridge: false,
            llm_gateway_server: false,
        }
    }

    pub const fn all_enabled() -> Self {
        Self {
            cli: true,
            http_server: true,
            wss_client: true,
            wss_server: true,
            mcp_server: true,
            a2a_bridge: true,
            llm_gateway_server: true,
        }
    }

    pub const fn with_cli(mut self, enabled: bool) -> Self {
        self.cli = enabled;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryCapabilityItem {
    pub profile_allowed: bool,
    pub compiled: bool,
    pub config_enabled: bool,
    pub permission_allowed: bool,
    pub privacy_allowed: bool,
    pub visible: bool,
}

impl EntryCapabilityItem {
    pub const fn hidden() -> Self {
        Self {
            profile_allowed: false,
            compiled: false,
            config_enabled: false,
            permission_allowed: false,
            privacy_allowed: false,
            visible: false,
        }
    }

    pub(crate) fn from_adapter(
        adapter: AdapterTransportVisibility,
        config_enabled: bool,
        server_mode: bool,
    ) -> Self {
        let client_or_local_allowed = adapter.client_allowed || !adapter.server_allowed;
        let mode_allowed = if server_mode {
            adapter.server_allowed
        } else {
            client_or_local_allowed
        };
        Self {
            profile_allowed: adapter.profile_allowed && mode_allowed,
            compiled: adapter.compiled,
            config_enabled,
            permission_allowed: adapter.permission_allowed,
            privacy_allowed: adapter.privacy_allowed,
            visible: adapter.visible && mode_allowed && config_enabled,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryCapabilityView {
    pub profile: ProfileId,
    pub cli: EntryCapabilityItem,
    pub http_server: EntryCapabilityItem,
    pub wss_client: EntryCapabilityItem,
    pub wss_server: EntryCapabilityItem,
    pub mcp_server: EntryCapabilityItem,
    pub a2a_bridge: EntryCapabilityItem,
    pub llm_gateway_server: EntryCapabilityItem,
}

impl EntryCapabilityView {
    pub(crate) fn from_catalog(
        profile: ProfileId,
        catalog: &bm_sdk::MemoryCapabilityCatalog,
        transports: &EntryTransportConfig,
    ) -> Self {
        Self {
            profile,
            cli: EntryCapabilityItem::from_adapter(catalog.adapter.cli, transports.cli, false),
            http_server: EntryCapabilityItem::from_adapter(
                catalog.adapter.http,
                transports.http_server,
                true,
            ),
            wss_client: EntryCapabilityItem::from_adapter(
                catalog.adapter.wss,
                transports.wss_client,
                false,
            ),
            wss_server: EntryCapabilityItem::from_adapter(
                catalog.adapter.wss,
                transports.wss_server,
                true,
            ),
            mcp_server: EntryCapabilityItem::from_adapter(
                catalog.adapter.mcp,
                transports.mcp_server,
                true,
            ),
            a2a_bridge: EntryCapabilityItem::from_adapter(
                catalog.adapter.a2a,
                transports.a2a_bridge,
                true,
            ),
            llm_gateway_server: EntryCapabilityItem::from_adapter(
                catalog.entry.llm_gateway_server,
                transports.llm_gateway_server,
                true,
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryIdempotencyConfig {
    pub max_keys: usize,
}

impl EntryIdempotencyConfig {
    pub const fn disabled() -> Self {
        Self { max_keys: 0 }
    }
}

pub(crate) fn enabled_capability_policy(
    mut policy: MemoryCapabilityPolicy,
) -> MemoryCapabilityPolicy {
    if policy.communication_adapter_enabled {
        return policy;
    }
    policy.communication_adapter_enabled = true;
    policy
}

pub(crate) fn privacy_policy(policy: MemoryPrivacyPolicy) -> MemoryPrivacyPolicy {
    policy
}
