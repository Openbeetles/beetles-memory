use std::io::Cursor;

use bm_entry::EntryAuthConfig;
use bm_llm_gateway::{
    serve_llm_gateway_http_stream, GatewayConfig, GatewayProviderConfig, GatewayRuntime,
    OllamaNativeUpstream, OllamaUpstreamRequest, OllamaUpstreamResponse, OpenAiCompatibleUpstream,
    OpenAiUpstreamRequest, OpenAiUpstreamResponse,
};

#[test]
fn remote_gateway_missing_token_is_structured_rejection_before_upstream() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.server.loopback_only = false;
    config.server.bind_addr = "0.0.0.0:8787".to_string();
    config.entry.auth = EntryAuthConfig::disabled_for_local();
    config.providers.clear();
    config.providers.insert(
        "local".to_string(),
        GatewayProviderConfig::ollama_native("http://127.0.0.1:11435/api"),
    );
    config.default_provider = "local".to_string();
    let runtime = GatewayRuntime::open(config.clone()).expect("gateway runtime");
    let request = "GET /api/tags HTTP/1.1\r\nhost: gateway\r\n\r\n";
    let mut stream = Cursor::new(request.as_bytes().to_vec());

    serve_llm_gateway_http_stream(
        &runtime,
        &config,
        &mut MockOpenAiUpstream,
        &mut MockOllamaUpstream,
        &mut stream,
    )
    .expect("gateway writes structured error response");

    let output = String::from_utf8(stream.into_inner()).expect("utf8");
    assert!(output.contains("400 Bad Request"), "{output}");
    assert!(output.contains("gateway auth rejected request"), "{output}");
    assert!(output.contains("token_not_configured"), "{output}");
}

struct MockOpenAiUpstream;

impl OpenAiCompatibleUpstream for MockOpenAiUpstream {
    fn models(
        &mut self,
        _provider: &GatewayProviderConfig,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        panic!("unauthorized request must not reach OpenAI upstream");
    }

    fn chat_completion(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OpenAiUpstreamRequest,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        panic!("unauthorized request must not reach OpenAI upstream");
    }
}

struct MockOllamaUpstream;

impl OllamaNativeUpstream for MockOllamaUpstream {
    fn chat(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        panic!("unauthorized request must not reach Ollama upstream");
    }

    fn generate(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        panic!("unauthorized request must not reach Ollama upstream");
    }
}
