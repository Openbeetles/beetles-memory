use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntimeBaseConfig,
    EntryRuntimeScope, EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_llm_gateway::{
    GatewayConfig, GatewayProviderConfig, GatewayRuntime, GatewayRuntimeCacheConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn config() -> GatewayConfig {
    let mut config = GatewayConfig::default_for_local_dev();
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    config.entry = EntryRuntimeBaseConfig {
        profile: ProfileId::ServerLinuxDevFull,
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    };
    config.runtime_cache = GatewayRuntimeCacheConfig { max_runtimes: 1 };
    config.providers.insert(
        "local".to_string(),
        GatewayProviderConfig::openai_compatible("http://127.0.0.1:8000/v1", None),
    );
    config.default_provider = "local".to_string();
    config
}

fn scope(chat_id: &str) -> EntryRuntimeScope {
    EntryRuntimeScope {
        identity: EntryIdentity {
            agent_id: "agent-main".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "llm.gateway".to_string(),
            chat_id: chat_id.to_string(),
        },
    }
}

#[test]
fn gateway_runtime_uses_entry_runtime_manager_and_preserves_active_scope_cache() {
    let gateway = GatewayRuntime::open(config()).expect("gateway runtime");
    let runtime_a_first = gateway
        .runtime_for_scope(scope("chat-a"))
        .expect("runtime a first");
    let _runtime_b = gateway
        .runtime_for_scope(scope("chat-b"))
        .expect("runtime b");
    let runtime_a_second = gateway
        .runtime_for_scope(scope("chat-a"))
        .expect("runtime a second");

    assert!(std::sync::Arc::ptr_eq(&runtime_a_first, &runtime_a_second));
}
