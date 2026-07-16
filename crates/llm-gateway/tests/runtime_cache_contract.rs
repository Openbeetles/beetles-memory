use bm_entry::{EntryIdentity, EntryRuntimeScope, EntryScope};
use bm_llm_gateway::{
    GatewayConfig, GatewayProviderConfig, GatewayRuntime, GatewayRuntimeCacheConfig,
};

fn config() -> GatewayConfig {
    let mut config = GatewayConfig::default_for_local_dev();
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
    assert_eq!(gateway.max_cached_runtimes(), 1);
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

#[test]
fn gateway_runtime_cache_request_is_clamped_by_the_opened_runtime_report() {
    let mut config = config();
    config.runtime_cache = GatewayRuntimeCacheConfig {
        max_runtimes: usize::MAX,
    };

    let gateway = GatewayRuntime::open(config).expect("gateway runtime");
    let compiled_limit = gateway
        .runtime_budget()
        .llm_gateway_budget
        .runtime_cache_max_runtimes;

    assert_eq!(gateway.max_cached_runtimes(), compiled_limit);
}
