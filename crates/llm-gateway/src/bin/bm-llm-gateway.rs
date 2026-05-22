use std::net::TcpListener;

use bm_llm_gateway::{
    serve_openai_http_stream, GatewayConfig, GatewayProviderConfig, GatewayRuntime,
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
        if let Err(error) = serve_openai_http_stream(&gateway, &config, &mut upstream, &mut stream)
        {
            eprintln!("bm-llm-gateway request failed: {error}");
        }
    }
    Ok(())
}
