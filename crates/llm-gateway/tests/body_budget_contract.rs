use std::io::Cursor;

use bm_llm_gateway::{
    serve_llm_gateway_http_stream, GatewayConfig, GatewayProviderConfig, GatewayRuntime,
    OllamaNativeUpstream, OllamaUpstreamRequest, OllamaUpstreamResponse, OpenAiCompatibleUpstream,
    OpenAiUpstreamRequest, OpenAiUpstreamResponse,
};
use bm_sdk::RuntimeBudgetReport;

#[test]
fn gateway_http_front_rejects_body_over_runtime_adapter_budget_before_json_decode() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.providers.clear();
    config.providers.insert(
        "local".to_string(),
        GatewayProviderConfig::ollama_native("http://127.0.0.1:11435/api"),
    );
    config.default_provider = "local".to_string();
    let runtime = GatewayRuntime::open(config.clone()).expect("gateway runtime");
    let budget = RuntimeBudgetReport::static_for_profile(config.entry.profile)
        .adapter_budget
        .http_body_max_bytes;
    let body = "x".repeat(budget + 1);
    let request = format!(
        "POST /api/chat HTTP/1.1\r\nhost: localhost\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let mut stream = Cursor::new(request.into_bytes());

    serve_llm_gateway_http_stream(
        &runtime,
        &config,
        &mut MockOpenAiUpstream,
        &mut MockOllamaUpstream,
        &mut stream,
    )
    .expect("gateway writes structured error response");

    let output = String::from_utf8(stream.into_inner()).expect("utf8");
    assert!(output.contains("413 Payload Too Large"), "{output}");
    assert!(
        output.contains("HTTP body exceeds runtime adapter budget"),
        "{output}"
    );
}

struct MockOpenAiUpstream;

impl OpenAiCompatibleUpstream for MockOpenAiUpstream {
    fn models(
        &mut self,
        _provider: &GatewayProviderConfig,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        panic!("over-budget request must not reach OpenAI upstream");
    }

    fn chat_completion(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OpenAiUpstreamRequest,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        panic!("over-budget request must not reach OpenAI upstream");
    }
}

struct MockOllamaUpstream;

impl OllamaNativeUpstream for MockOllamaUpstream {
    fn chat(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        panic!("over-budget request must not reach Ollama upstream");
    }

    fn generate(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        panic!("over-budget request must not reach Ollama upstream");
    }
}
