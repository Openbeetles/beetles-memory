use std::net::TcpListener;

use bm_llm_gateway::{
    serve_llm_gateway_http_stream_with_services, GatewayConfig, GatewayError,
    GatewayProviderConfig, GatewayProviderKind, GatewayRuntime, OllamaMaintenanceLlmClient,
    OpenAiGatewayServices, OpenAiMaintenanceLlmClient, ReqwestGatewayLlmHttpClient,
    ReqwestOllamaNativeUpstream, ReqwestOpenAiCompatibleUpstream,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = GatewayConfig::default_for_local_dev();
    if let Ok(bind_addr) = std::env::var("BM_LLM_GATEWAY_BIND") {
        config.server.bind_addr = bind_addr;
    }
    if let Ok(base_url) = std::env::var("BM_LLM_GATEWAY_OPENAI_BASE_URL") {
        let api_key_env = std::env::var("BM_LLM_GATEWAY_OPENAI_API_KEY_ENV").ok();
        config.providers.insert(
            config.default_provider.clone(),
            GatewayProviderConfig::openai_compatible(base_url, api_key_env.as_deref()),
        );
    }
    let ollama_base_url = std::env::var("BM_LLM_GATEWAY_OLLAMA_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:11434/api".to_string());
    config.providers.insert(
        "ollama".to_string(),
        GatewayProviderConfig::ollama_native(ollama_base_url),
    );
    if let Ok(default_provider) = std::env::var("BM_LLM_GATEWAY_DEFAULT_PROVIDER") {
        config.default_provider = default_provider;
    }
    config.validate()?;

    let listener = TcpListener::bind(&config.server.bind_addr)?;
    let gateway = GatewayRuntime::open(config.clone())?;
    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut upstream = ReqwestOpenAiCompatibleUpstream::new()?;
        let mut ollama_upstream = ReqwestOllamaNativeUpstream::new()?;
        let mut maintenance_http = ReqwestGatewayLlmHttpClient::new()?;
        let maintenance_provider_name = std::env::var("BM_LLM_GATEWAY_MAINTENANCE_PROVIDER")
            .unwrap_or_else(|_| config.default_provider.clone());
        let maintenance_provider = config
            .providers
            .get(&maintenance_provider_name)
            .ok_or_else(|| {
                GatewayError::invalid_config(format!(
                    "maintenance provider is not configured: {maintenance_provider_name}"
                ))
            })?
            .clone();
        let maintenance_model = std::env::var("BM_LLM_GATEWAY_MAINTENANCE_MODEL")
            .unwrap_or_else(|_| "local".to_string());
        let result = match maintenance_provider.kind {
            GatewayProviderKind::OpenAiCompatible => {
                let maintenance_llm =
                    OpenAiMaintenanceLlmClient::new(maintenance_provider, maintenance_model);
                let mut services = OpenAiGatewayServices::new()
                    .with_maintenance(&mut maintenance_http, &maintenance_llm);
                serve_llm_gateway_http_stream_with_services(
                    &gateway,
                    &config,
                    &mut upstream,
                    &mut ollama_upstream,
                    &mut services,
                    &mut stream,
                )
            }
            GatewayProviderKind::OllamaNative => {
                let maintenance_llm =
                    OllamaMaintenanceLlmClient::new(maintenance_provider, maintenance_model);
                let mut services = OpenAiGatewayServices::new()
                    .with_maintenance(&mut maintenance_http, &maintenance_llm);
                serve_llm_gateway_http_stream_with_services(
                    &gateway,
                    &config,
                    &mut upstream,
                    &mut ollama_upstream,
                    &mut services,
                    &mut stream,
                )
            }
        };
        if let Err(error) = result {
            eprintln!("bm-llm-gateway request failed: {error}");
        }
    }
    Ok(())
}
