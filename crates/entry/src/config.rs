use std::path::PathBuf;

use bm_sdk::{
    AdapterTransportVisibility, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId,
    StoreBackendKind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryIdentity {
    pub agent_id: String,
    pub owner_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryScope {
    pub channel: String,
    pub chat_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryStoreConfig {
    pub backend: StoreBackendKind,
    pub data_path: Option<PathBuf>,
    pub fsync: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryTransportConfig {
    pub cli: bool,
    pub http_server: bool,
    pub webhook_receiver: bool,
    pub webhook_sender: bool,
    pub wss_client: bool,
    pub wss_server: bool,
    pub mqtt_client: bool,
    pub mqtt_bridge: bool,
    pub mcp_server: bool,
    pub a2a_bridge: bool,
}

impl EntryTransportConfig {
    pub const fn all_disabled() -> Self {
        Self {
            cli: false,
            http_server: false,
            webhook_receiver: false,
            webhook_sender: false,
            wss_client: false,
            wss_server: false,
            mqtt_client: false,
            mqtt_bridge: false,
            mcp_server: false,
            a2a_bridge: false,
        }
    }

    pub const fn all_enabled() -> Self {
        Self {
            cli: true,
            http_server: true,
            webhook_receiver: true,
            webhook_sender: true,
            wss_client: true,
            wss_server: true,
            mqtt_client: true,
            mqtt_bridge: true,
            mcp_server: true,
            a2a_bridge: true,
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
    pub webhook_receiver: EntryCapabilityItem,
    pub webhook_sender: EntryCapabilityItem,
    pub wss_client: EntryCapabilityItem,
    pub wss_server: EntryCapabilityItem,
    pub mqtt_client: EntryCapabilityItem,
    pub mqtt_bridge: EntryCapabilityItem,
    pub mcp_server: EntryCapabilityItem,
    pub a2a_bridge: EntryCapabilityItem,
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
            webhook_receiver: EntryCapabilityItem::from_adapter(
                catalog.adapter.webhook,
                transports.webhook_receiver,
                true,
            ),
            webhook_sender: EntryCapabilityItem::from_adapter(
                catalog.adapter.webhook,
                transports.webhook_sender,
                false,
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
            mqtt_client: EntryCapabilityItem::from_adapter(
                catalog.adapter.mqtt,
                transports.mqtt_client,
                false,
            ),
            mqtt_bridge: EntryCapabilityItem::from_adapter(
                catalog.adapter.mqtt,
                transports.mqtt_bridge,
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
