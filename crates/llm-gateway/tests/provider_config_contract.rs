use bm_llm_gateway::{GatewayProviderConfig, GatewayProviderKind};

#[test]
fn provider_config_accepts_openai_compatible_and_ollama_native_shapes() {
    let openai =
        GatewayProviderConfig::openai_compatible("http://127.0.0.1:8000/v1", Some("VLLM_API_KEY"));
    let ollama = GatewayProviderConfig::ollama_native("http://127.0.0.1:11434/api");

    assert_eq!(openai.kind, GatewayProviderKind::OpenAiCompatible);
    assert_eq!(ollama.kind, GatewayProviderKind::OllamaNative);
    assert_eq!(openai.base_url, "http://127.0.0.1:8000/v1");
    assert_eq!(ollama.base_url, "http://127.0.0.1:11434/api");
}

#[test]
fn provider_config_keeps_protocol_routes_out_of_provider_shape() {
    let openai = GatewayProviderConfig::openai_compatible("http://127.0.0.1:8000/v1", None);
    let ollama = GatewayProviderConfig::ollama_native("http://127.0.0.1:11434/api");

    let openai_json = serde_json::to_value(openai).expect("openai json");
    let ollama_json = serde_json::to_value(ollama).expect("ollama json");

    assert!(openai_json.get("endpoint").is_none());
    assert!(openai_json.get("route").is_none());
    assert!(ollama_json.get("endpoint").is_none());
    assert!(ollama_json.get("route").is_none());
}
