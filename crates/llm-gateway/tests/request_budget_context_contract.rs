use serde_json::json;

use bm_llm_gateway::{
    handle_openai_request, serve_llm_gateway_http_accepted_stream_in_request, GatewayConfig,
    GatewayErrorKey, GatewayHttpRequestBindings, GatewayProviderConfig, GatewayRuntime,
    OllamaNativeUpstream, OllamaUpstreamRequest, OllamaUpstreamResponse, OpenAiCompatibleUpstream,
    OpenAiGatewayRequest, OpenAiUpstreamRequest, OpenAiUpstreamResponse,
};

mod support;

#[derive(Default)]
struct StreamingUpstream {
    bound_report_id: Option<String>,
}

impl OpenAiCompatibleUpstream for StreamingUpstream {
    fn bind_response_budget(
        &mut self,
        budget: bm_llm_gateway::GatewayUpstreamResponseBudget,
    ) -> bm_llm_gateway::Result<()> {
        self.bound_report_id = Some(budget.report_id().to_string());
        Ok(())
    }

    fn models(
        &mut self,
        _provider: &GatewayProviderConfig,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        unreachable!("chat contract does not call models")
    }

    fn chat_completion(
        &mut self,
        _provider: &GatewayProviderConfig,
        request: OpenAiUpstreamRequest,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        if request.stream {
            Ok(OpenAiUpstreamResponse::sse(
                200,
                vec!["data: [DONE]\n\n".to_string()],
            ))
        } else {
            Ok(OpenAiUpstreamResponse::json(
                200,
                json!({
                    "id": "chatcmpl-pinned",
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "ok"},
                        "finish_reason": "stop"
                    }]
                }),
            ))
        }
    }
}

#[test]
fn streaming_response_holds_the_report_derived_concurrency_permit_until_drop() {
    let config = GatewayConfig::default_for_local_dev();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway runtime");
    let limit = gateway
        .runtime_budget()
        .runtime_job_budget
        .max_concurrent_jobs;
    let mut upstream = StreamingUpstream::default();

    let response = handle_openai_request(
        &gateway,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            support::loopback_scope_request("request-budget-test"),
            json!({
                "model": "local",
                "stream": true,
                "messages": [{"role": "user", "content": "hold the request permit"}]
            }),
        ),
        &mut upstream,
    )
    .expect("streaming gateway response");

    let report_id = upstream
        .bound_report_id
        .as_deref()
        .expect("upstream budget was bound");
    assert_eq!(report_id, gateway.runtime_budget().report_id);

    let mut other_requests = Vec::new();
    for _ in 1..limit {
        other_requests.push(gateway.begin_request().expect("permit below exact limit"));
    }
    let error = gateway
        .begin_request()
        .expect_err("limit plus one must be rejected while stream is alive");
    assert_eq!(error.key(), GatewayErrorKey::CapacityExceeded);

    drop(response);
    gateway
        .begin_request()
        .expect("dropping the stream response releases its permit");
}

#[test]
fn http_parser_scope_projection_upstream_and_response_share_one_pinned_report() {
    let config = GatewayConfig::default_for_local_dev();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway runtime");
    let context = gateway.begin_request().expect("request budget context");
    let expected_report_id = context.report_id().to_string();
    let body = json!({
        "model": "local",
        "stream": false,
        "messages": [{"role": "user", "content": "one pinned report"}]
    })
    .to_string();
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let (mut stream, client) = support::accepted_request(request);
    let mut upstream = StreamingUpstream::default();
    let mut ollama = UnusedOllamaUpstream;
    serve_llm_gateway_http_accepted_stream_in_request(
        &gateway,
        &context,
        GatewayHttpRequestBindings::new(&mut upstream, &mut ollama),
        &mut stream,
    )
    .expect("gateway HTTP request");

    assert_eq!(
        upstream.bound_report_id.as_deref(),
        Some(expected_report_id.as_str())
    );
    drop(stream);
    let output = support::finish_request(client);
    assert!(output.contains("HTTP/1.1 200 OK"), "{output}");
}

struct UnusedOllamaUpstream;

impl OllamaNativeUpstream for UnusedOllamaUpstream {
    fn chat(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        unreachable!("OpenAI contract does not call Ollama chat")
    }

    fn generate(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        unreachable!("OpenAI contract does not call Ollama generate")
    }
}
