use bm_llm_gateway::{
    serve_llm_gateway_http_accepted_stream, GatewayConfig, GatewayProviderConfig, GatewayRuntime,
    OllamaNativeUpstream, OllamaUpstreamRequest, OllamaUpstreamResponse, OpenAiCompatibleUpstream,
    OpenAiUpstreamRequest, OpenAiUpstreamResponse,
};

mod support;

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
    let budget = runtime.runtime_budget().adapter_budget.http_body_max_bytes;
    let body = "x".repeat(budget + 1);
    let request = format!(
        "POST /api/chat HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let output = serve_raw(&runtime, request.into_bytes());
    assert!(output.contains("413 Payload Too Large"), "{output}");
    assert!(
        output.contains("HTTP body exceeds runtime adapter budget"),
        "{output}"
    );
}

#[test]
fn gateway_http_front_rejects_headers_over_the_same_runtime_adapter_budget() {
    let mut config = GatewayConfig::default_for_local_dev();
    config.providers.clear();
    config.providers.insert(
        "local".to_string(),
        GatewayProviderConfig::ollama_native("http://127.0.0.1:11435/api"),
    );
    config.default_provider = "local".to_string();
    let runtime = GatewayRuntime::open(config.clone()).expect("gateway runtime");
    let budget = runtime
        .runtime_budget()
        .adapter_budget
        .http_header_max_bytes;
    let oversized_header = "x".repeat(budget);
    let request = format!(
        "POST /api/chat HTTP/1.1\r\nhost: localhost\r\nx-oversized: {oversized_header}\r\ncontent-length: 0\r\n\r\n"
    );
    let output = serve_raw(&runtime, request.into_bytes());
    assert!(output.contains("413 Payload Too Large"), "{output}");
    assert!(
        output.contains("HTTP headers exceed runtime adapter budget"),
        "{output}"
    );
}

#[test]
fn gateway_http_front_rejects_ambiguous_transfer_framing_before_body_read() {
    let runtime = gateway();
    let request = concat!(
        "POST /api/chat HTTP/1.1\r\n",
        "host: localhost\r\n",
        "transfer-encoding: chunked\r\n",
        "content-length: 1\r\n\r\n"
    );
    let output = serve_raw(&runtime, request.as_bytes().to_vec());
    assert!(output.contains("400 Bad Request"), "{output}");
    assert!(
        output.contains("transfer-encoding is forbidden"),
        "{output}"
    );
}

#[test]
fn gateway_http_front_rejects_duplicate_or_noncanonical_content_length() {
    let runtime = gateway();
    for request in [
        concat!(
            "POST /api/chat HTTP/1.1\r\n",
            "host: localhost\r\n",
            "content-length: 0\r\n",
            "content-length: 0\r\n\r\n"
        ),
        concat!(
            "POST /api/chat HTTP/1.1\r\n",
            "host: localhost\r\n",
            "content-length: +1\r\n\r\n"
        ),
    ] {
        let output = serve_raw(&runtime, request.as_bytes().to_vec());
        assert!(output.contains("400 Bad Request"), "{output}");
    }
}

#[test]
fn gateway_http_front_accepts_exact_body_budget_before_route_validation() {
    let runtime = gateway();
    let budget = runtime.runtime_budget().adapter_budget.http_body_max_bytes;
    assert!(budget >= 2);
    let body = format!("\"{}\"", "x".repeat(budget - 2));
    let request = format!(
        "POST /api/chat HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );

    let output = serve_raw(&runtime, request.into_bytes());

    assert!(output.contains("400 Bad Request"), "{output}");
    assert!(output.contains("chat body must be an object"), "{output}");
    assert!(!output.contains("Payload Too Large"), "{output}");
}

#[test]
fn gateway_http_front_requires_host_and_method_specific_content_length() {
    let runtime = gateway();
    let cases = [
        (
            "POST /api/chat HTTP/1.1\r\ncontent-length: 0\r\n\r\n",
            "Host header is required",
        ),
        (
            "POST /api/chat HTTP/1.1\r\nhost: localhost\r\n\r\n",
            "POST requires HTTP content-length",
        ),
        (
            "GET /api/tags HTTP/1.1\r\nhost: localhost\r\ncontent-length: 1\r\n\r\nx",
            "GET and DELETE require zero HTTP content-length",
        ),
        (
            "DELETE /api/delete HTTP/1.1\r\nhost: localhost\r\ncontent-length: 1\r\n\r\nx",
            "GET and DELETE require zero HTTP content-length",
        ),
    ];

    for (request, reason) in cases {
        let output = serve_raw(&runtime, request.as_bytes().to_vec());
        assert!(output.contains("400 Bad Request"), "{output}");
        assert!(output.contains(reason), "{output}");
    }
}

#[test]
fn gateway_http_front_requires_exact_json_content_type_and_allowed_origin() {
    let runtime = gateway();
    let cases = [
        (
            "POST /api/chat HTTP/1.1\r\nhost: localhost\r\ncontent-length: 2\r\n\r\n{}",
            "POST requires content-type application/json",
        ),
        (
            "POST /api/chat HTTP/1.1\r\nhost: localhost\r\ncontent-type: text/plain\r\ncontent-length: 2\r\n\r\n{}",
            "POST requires content-type application/json",
        ),
        (
            "POST /api/chat HTTP/1.1\r\nhost: localhost\r\norigin: http://localhost.evil:8787\r\ncontent-type: application/json\r\ncontent-length: 2\r\n\r\n{}",
            "gateway Origin is not allowed",
        ),
    ];

    for (request, reason) in cases {
        let output = serve_raw(&runtime, request.as_bytes().to_vec());
        assert!(output.contains(reason), "{output}");
    }
}

#[test]
fn gateway_http_front_rejects_invalid_header_name_and_noncanonical_request_line() {
    let runtime = gateway();
    let cases = [
        (
            "POST /api/chat HTTP/1.1\r\nhost: localhost\r\nbad header: value\r\ncontent-length: 0\r\n\r\n",
            "invalid HTTP header name",
        ),
        (
            "POST\t/api/chat HTTP/1.1\r\nhost: localhost\r\ncontent-length: 0\r\n\r\n",
            "unsupported HTTP method",
        ),
        (
            "POST  /api/chat HTTP/1.1\r\nhost: localhost\r\ncontent-length: 0\r\n\r\n",
            "missing HTTP path",
        ),
    ];

    for (request, reason) in cases {
        let output = serve_raw(&runtime, request.as_bytes().to_vec());
        assert!(output.contains("400 Bad Request"), "{output}");
        assert!(output.contains(reason), "{output}");
    }
}

fn serve_raw(runtime: &GatewayRuntime, request: Vec<u8>) -> String {
    let (mut stream, client) = support::accepted_request(request);
    serve_llm_gateway_http_accepted_stream(
        runtime,
        &mut MockOpenAiUpstream,
        &mut MockOllamaUpstream,
        &mut stream,
    )
    .expect("gateway writes structured framing response");
    drop(stream);
    support::finish_request(client)
}

fn gateway() -> GatewayRuntime {
    let mut config = GatewayConfig::default_for_local_dev();
    config.providers.clear();
    config.providers.insert(
        "local".to_string(),
        GatewayProviderConfig::ollama_native("http://127.0.0.1:11435/api"),
    );
    config.default_provider = "local".to_string();
    GatewayRuntime::open(config).expect("gateway runtime")
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
