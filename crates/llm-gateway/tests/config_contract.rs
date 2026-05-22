use bm_llm_gateway::{
    GatewayConfig, GatewayErrorKey, GatewayMaintenanceConfig, GatewayProjectionConfig,
    GatewayProviderConfig, GatewayProviderKind, GatewayRuntimeCacheConfig,
};
use bm_sdk::PressureLevel;

#[test]
fn gateway_config_defaults_are_loopback_memory_required_and_bounded() {
    let config = GatewayConfig::default_for_local_dev();

    assert_eq!(config.server.bind_addr, "127.0.0.1:8787");
    assert!(config.server.loopback_only);
    assert!(config.server.memory_required);
    assert!(config.server.require_token_for_remote);
    assert_eq!(config.scope.default_channel, "llm.gateway");
    assert_eq!(config.scope.default_chat_id, None);
    assert_eq!(config.runtime_cache.max_runtimes, 256);
    assert_eq!(
        config.projection,
        GatewayProjectionConfig {
            system_max_len: 8192,
            recent_messages_limit: 32,
            pressure: PressureLevel::Normal,
        }
    );
    assert_eq!(
        config.maintenance,
        GatewayMaintenanceConfig {
            enabled: true,
            user_max_chars: 8192,
            user_max_bytes: 16 * 1024,
            reply_max_chars: 8192,
            reply_max_bytes: 16 * 1024,
        }
    );
    assert!(!config.audit.record_raw_projection);
    assert!(!config.audit.record_full_request_body);
    assert!(!config.audit.record_full_response_body);
}

#[test]
fn gateway_config_rejects_zero_runtime_cache_and_missing_default_provider() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.runtime_cache = GatewayRuntimeCacheConfig { max_runtimes: 0 };
    let error = config.validate().expect_err("zero runtime cache must fail");
    assert_eq!(error.key(), GatewayErrorKey::InvalidConfig);

    let mut config = GatewayConfig::default_for_local_dev();
    config.providers.clear();
    let error = config
        .validate()
        .expect_err("default provider must exist in provider map");
    assert_eq!(error.key(), GatewayErrorKey::InvalidConfig);

    let mut config = GatewayConfig::default_for_local_dev();
    config.projection.recent_messages_limit = 0;
    let error = config
        .validate()
        .expect_err("zero projection recent messages limit must fail");
    assert_eq!(error.key(), GatewayErrorKey::InvalidConfig);

    let mut config = GatewayConfig::default_for_local_dev();
    config.maintenance.reply_max_bytes = 0;
    let error = config
        .validate()
        .expect_err("zero maintenance reply byte limit must fail");
    assert_eq!(error.key(), GatewayErrorKey::InvalidConfig);
}

#[test]
fn gateway_config_rejects_non_loopback_bind_when_loopback_only_is_true() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.server.bind_addr = "0.0.0.0:8787".to_string();

    let error = config
        .validate()
        .expect_err("loopback_only requires a loopback bind address");

    assert_eq!(error.key(), GatewayErrorKey::InvalidConfig);
}

#[test]
fn provider_config_uses_secret_env_not_plaintext_api_key() {
    let provider = GatewayProviderConfig {
        kind: GatewayProviderKind::OpenAiCompatible,
        base_url: "http://127.0.0.1:8000/v1".to_string(),
        api_key_env: Some("VLLM_API_KEY".to_string()),
        model_aliases: vec![("local".to_string(), "qwen2.5".to_string())],
        timeout_ms: Some(30_000),
        ollama_generate_system_supported: true,
        openai_responses_supported: true,
        openai_stateful_responses_supported: false,
        openai_embeddings_supported: true,
        openai_tools_supported: true,
        openai_streaming_supported: true,
    };

    assert_eq!(provider.secret_env_name(), Some("VLLM_API_KEY"));
    assert!(!serde_json::to_value(&provider)
        .expect("provider json")
        .as_object()
        .expect("provider object")
        .contains_key("api_key"));
}
