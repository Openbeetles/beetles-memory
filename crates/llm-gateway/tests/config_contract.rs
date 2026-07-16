use bm_entry::{EntryAuthConfig, EntryBearerPrincipal, EntryOperationCapability};
use bm_llm_gateway::{
    llm_gateway_transport_config, GatewayConfig, GatewayErrorKey, GatewayMaintenanceConfig,
    GatewayProjectionConfig, GatewayProviderConfig, GatewayProviderKind, GatewayRuntime,
    GatewayRuntimeCacheConfig,
};
use bm_sdk::{PressureLevel, ProfileId, RoleFeature};

#[test]
fn gateway_config_defaults_are_memory_required_origin_bounded_and_loopback_bound() {
    let config = GatewayConfig::default_for_local_dev();

    #[cfg(not(target_os = "windows"))]
    assert_ne!(config.entry.store.profile().role(), RoleFeature::DevFull);
    #[cfg(target_os = "windows")]
    assert_eq!(config.entry.store.profile().role(), RoleFeature::DevFull);
    assert_eq!(config.entry.transports, llm_gateway_transport_config());
    assert_eq!(config.server.bind_addr, "127.0.0.1:8787");
    assert!(config.server.memory_required);
    assert_eq!(
        config.server.allowed_origins,
        [
            "http://127.0.0.1:8787",
            "http://localhost:8787",
            "http://[::1]:8787"
        ]
    );
    assert_eq!(config.scope.default_channel, "llm.gateway");
    assert_eq!(config.scope.default_chat_id, None);
    assert_eq!(config.runtime_cache.max_runtimes, 256);
    assert_eq!(
        config.projection,
        GatewayProjectionConfig {
            pressure: PressureLevel::Normal,
        }
    );
    assert_eq!(
        config.maintenance,
        GatewayMaintenanceConfig { enabled: true }
    );
    assert!(!config.audit.record_raw_projection);
    assert_eq!(config.audit.raw_projection_diagnostic_path, None);
    assert_eq!(config.audit.raw_projection_retention_limit, 32);
    assert!(!config.audit.record_full_request_body);
    assert!(!config.audit.record_full_response_body);
}

#[test]
fn gateway_rejects_transport_capability_expansion() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.entry.transports.http_server = true;
    let error = config
        .validate()
        .expect_err("gateway must reject unrelated transport capability");
    assert!(error.message().contains("only llm_gateway_server"));
}

#[test]
fn gateway_rejects_profiles_that_do_not_own_the_gateway_entry() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.entry.store = bm_sdk::StoreBackendConfig::in_memory(ProfileId::DesktopMacosEmbeddedSdk)
        .expect("embedded store config");
    let error = config
        .validate()
        .expect_err("embedded profile must not bind the gateway entry");
    assert!(error.message().contains("does not authorize"));
}

#[test]
fn gateway_config_debug_recursively_redacts_bearer_secret() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.entry.auth = EntryAuthConfig::required_bearer_principal(
        "gateway-super-secret",
        EntryBearerPrincipal::new(
            "gateway-principal",
            "owner-default",
            [EntryOperationCapability::Project],
        ),
    );

    let debug = format!("{config:?}");
    assert!(debug.contains("<redacted>"), "{debug}");
    assert!(!debug.contains("gateway-super-secret"), "{debug}");
}

#[test]
fn gateway_config_rejects_zero_runtime_cache_missing_provider_and_unbounded_raw_projection_audit() {
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
    config.audit.record_raw_projection = true;
    let error = config
        .validate()
        .expect_err("raw projection audit requires a diagnostic path");
    assert_eq!(error.key(), GatewayErrorKey::InvalidConfig);

    let mut config = GatewayConfig::default_for_local_dev();
    config.audit.record_raw_projection = true;
    config.audit.raw_projection_diagnostic_path =
        Some(std::env::temp_dir().join("bm-llm-gateway-audit-contract"));
    config.audit.raw_projection_retention_limit = 0;
    let error = config
        .validate()
        .expect_err("raw projection audit requires a retention limit");
    assert_eq!(error.key(), GatewayErrorKey::InvalidConfig);

    assert!(GatewayConfig::default_for_local_dev().validate().is_ok());
}

#[test]
fn gateway_config_rejects_non_loopback_bind_without_bearer_verifier() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.server.bind_addr = "0.0.0.0:8787".to_string();

    let error = config
        .validate()
        .expect_err("non-loopback bind requires a bearer verifier");

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
        max_prompt_chars: Some(8192),
    };

    assert_eq!(provider.secret_env_name(), Some("VLLM_API_KEY"));
    assert!(!serde_json::to_value(&provider)
        .expect("provider json")
        .as_object()
        .expect("provider object")
        .contains_key("api_key"));
}

#[test]
fn gateway_runtime_owns_the_only_validated_request_configuration() {
    let mut config = GatewayConfig::default_for_local_dev();
    config
        .providers
        .get_mut("local")
        .expect("default provider")
        .base_url = "http://127.0.0.1:11435/api".to_string();
    let runtime = GatewayRuntime::open(config.clone()).expect("gateway runtime");
    config
        .providers
        .get_mut("local")
        .expect("external default provider")
        .base_url = "http://127.0.0.1:11436/api".to_string();

    assert_eq!(
        runtime
            .provider_config(runtime.default_provider_name())
            .expect("runtime-owned default provider")
            .base_url,
        "http://127.0.0.1:11435/api"
    );
}
