use bm_entry::{
    EntryAcceptedTcpStream, EntryAuthConfig, EntryBearerPrincipal, EntryOperationCapability,
};
use bm_llm_gateway::{
    serve_llm_gateway_http_accepted_stream, GatewayConfig, GatewayProviderConfig, GatewayRuntime,
    OllamaNativeUpstream, OllamaUpstreamRequest, OllamaUpstreamResponse, OpenAiCompatibleUpstream,
    OpenAiUpstreamRequest, OpenAiUpstreamResponse,
};

mod support;

#[test]
fn public_server_entrypoint_consumes_only_runtime_owned_config() {
    let _serve: fn(
        &GatewayRuntime,
        &mut dyn OpenAiCompatibleUpstream,
        &mut dyn OllamaNativeUpstream,
        &mut EntryAcceptedTcpStream,
    ) -> bm_llm_gateway::Result<()> = serve_llm_gateway_http_accepted_stream;
}

#[test]
fn authentication_failure_is_unauthorized_not_forbidden() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.server.bind_addr = "0.0.0.0:8787".to_string();
    config.entry.auth = EntryAuthConfig::required_bearer_principal(
        "gateway-token",
        EntryBearerPrincipal::new(
            "service-principal",
            "memory-owner",
            EntryOperationCapability::all().iter().copied(),
        ),
    );
    let runtime = GatewayRuntime::open(config).expect("gateway runtime");
    let request = "GET /v1/models HTTP/1.1\r\nhost: gateway\r\n\r\n";
    let (mut stream, client) = support::accepted_request(request);

    serve_llm_gateway_http_accepted_stream(
        &runtime,
        &mut MockOpenAiUpstream,
        &mut MockOllamaUpstream,
        &mut stream,
    )
    .expect("gateway writes authentication error");

    drop(stream);
    let output = support::finish_request(client);
    assert!(output.contains("401 Unauthorized"), "{output}");
    assert!(output.contains(r#""type":"unauthorized""#), "{output}");
    assert!(!output.contains("403 Forbidden"), "{output}");
}

#[test]
fn runtime_owned_config_a_cannot_be_replaced_by_external_config_b() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.server.bind_addr = "0.0.0.0:8787".to_string();
    config.entry.auth = EntryAuthConfig::required_bearer_principal(
        "gateway-token",
        EntryBearerPrincipal::new(
            "service-principal",
            "memory-owner",
            [
                EntryOperationCapability::LlmGatewayProtocol,
                EntryOperationCapability::Project,
            ],
        ),
    );
    let runtime = GatewayRuntime::open(config.clone()).expect("gateway runtime");
    config.entry.auth = EntryAuthConfig::disabled_for_local();
    let body = r#"{"model":"local-model","messages":[{"role":"user","content":"hello"}]}"#;
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nhost: gateway\r\ncontent-type: application/json\r\nauthorization: Bearer gateway-token\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let (mut stream, client) = support::accepted_request(request);

    serve_llm_gateway_http_accepted_stream(
        &runtime,
        &mut MockOpenAiUpstream,
        &mut MockOllamaUpstream,
        &mut stream,
    )
    .expect("gateway writes structured error response");

    drop(stream);
    let output = support::finish_request(client);
    assert!(output.contains("403 Forbidden"), "{output}");
    assert!(output.contains("required capability: maintain"), "{output}");
}

#[test]
fn route_capabilities_are_rejected_before_declared_body_is_read() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.server.bind_addr = "0.0.0.0:8787".to_string();
    config.entry.auth = EntryAuthConfig::required_bearer_principal(
        "gateway-token",
        EntryBearerPrincipal::new(
            "service-principal",
            "memory-owner",
            [
                EntryOperationCapability::LlmGatewayProtocol,
                EntryOperationCapability::Project,
            ],
        ),
    );
    let runtime = GatewayRuntime::open(config.clone()).expect("gateway runtime");
    let request = "POST /v1/chat/completions HTTP/1.1\r\nhost: gateway\r\ncontent-type: application/json\r\nauthorization: Bearer gateway-token\r\ncontent-length: 4096\r\n\r\n";
    let (mut stream, client) = support::accepted_request(request);

    serve_llm_gateway_http_accepted_stream(
        &runtime,
        &mut MockOpenAiUpstream,
        &mut MockOllamaUpstream,
        &mut stream,
    )
    .expect("gateway writes pre-body authorization error");

    drop(stream);
    let output = support::finish_request(client);
    assert!(output.contains("403 Forbidden"), "{output}");
    assert!(output.contains("required capability: maintain"), "{output}");
    assert!(!output.contains("truncated HTTP body"), "{output}");
}

#[test]
fn ollama_route_capabilities_are_rejected_before_declared_body_is_read() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.server.bind_addr = "0.0.0.0:8787".to_string();
    config.entry.auth = EntryAuthConfig::required_bearer_principal(
        "gateway-token",
        EntryBearerPrincipal::new(
            "service-principal",
            "memory-owner",
            [
                EntryOperationCapability::LlmGatewayProtocol,
                EntryOperationCapability::Project,
            ],
        ),
    );
    let runtime = GatewayRuntime::open(config.clone()).expect("gateway runtime");
    let request = "POST /api/chat HTTP/1.1\r\nhost: gateway\r\ncontent-type: application/json\r\nauthorization: Bearer gateway-token\r\ncontent-length: 4096\r\n\r\n";
    let (mut stream, client) = support::accepted_request(request);

    serve_llm_gateway_http_accepted_stream(
        &runtime,
        &mut MockOpenAiUpstream,
        &mut MockOllamaUpstream,
        &mut stream,
    )
    .expect("gateway writes pre-body authorization error");

    drop(stream);
    let output = support::finish_request(client);
    assert!(output.contains("403 Forbidden"), "{output}");
    assert!(output.contains("required capability: maintain"), "{output}");
    assert!(!output.contains("truncated HTTP body"), "{output}");
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
