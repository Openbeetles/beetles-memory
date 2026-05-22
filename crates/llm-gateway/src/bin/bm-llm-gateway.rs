use std::net::TcpListener;

use bm_llm_gateway::{
    serve_openai_http_stream_with_services, GatewayConfig, GatewayProviderConfig, GatewayRuntime,
    OpenAiGatewayServices, OpenAiMaintenanceLlmClient, ReqwestGatewayLlmHttpClient,
    ReqwestOpenAiCompatibleUpstream,
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
    config.validate()?;

    let listener = TcpListener::bind(&config.server.bind_addr)?;
    let gateway = GatewayRuntime::open(config.clone())?;
    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut upstream = ReqwestOpenAiCompatibleUpstream::new()?;
        let mut maintenance_http = ReqwestGatewayLlmHttpClient::new()?;
        let maintenance_provider = config
            .providers
            .get(&config.default_provider)
            .expect("validated default provider")
            .clone();
        let maintenance_model = std::env::var("BM_LLM_GATEWAY_MAINTENANCE_MODEL")
            .unwrap_or_else(|_| "local".to_string());
        let maintenance_llm =
            OpenAiMaintenanceLlmClient::new(maintenance_provider, maintenance_model);
        let mut services =
            OpenAiGatewayServices::new().with_maintenance(&mut maintenance_http, &maintenance_llm);
        if let Err(error) = serve_openai_http_stream_with_services(
            &gateway,
            &config,
            &mut upstream,
            &mut services,
            &mut stream,
        ) {
            eprintln!("bm-llm-gateway request failed: {error}");
        }
    }
    Ok(())
}
