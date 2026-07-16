use bm_llm_gateway::{
    serve_openai_http_accepted_stream, GatewayConfig, GatewayRuntime, OpenAiCompatibleUpstream,
    OpenAiUpstreamRequest, OpenAiUpstreamResponse,
};
use serde_json::json;

mod support;

fn gateway_config() -> GatewayConfig {
    GatewayConfig::default_for_local_dev()
}

#[derive(Default)]
struct MockOpenAiUpstream {
    chat_calls: usize,
}

impl OpenAiCompatibleUpstream for MockOpenAiUpstream {
    fn models(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        Ok(OpenAiUpstreamResponse::json(
            200,
            json!({ "data": [{ "id": "qwen-local" }] }),
        ))
    }

    fn chat_completion(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        request: OpenAiUpstreamRequest,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        self.chat_calls += 1;
        assert!(request.stream);
        Ok(OpenAiUpstreamResponse::sse(
            200,
            vec![
                "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n".to_string(),
                "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n".to_string(),
                "data: [DONE]\n\n".to_string(),
            ],
        ))
    }
}

#[test]
fn http_chat_route_writes_sse_chunks_without_buffering_as_json() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let body = json!({
        "model": "local",
        "stream": true,
        "messages": [{ "role": "user", "content": "stream this" }]
    })
    .to_string();
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-BM-Conversation-ID: thread-7\r\n\r\n{}",
        body.len(),
        body
    );
    let (mut stream, client) = support::accepted_request(request);
    let mut upstream = MockOpenAiUpstream::default();

    serve_openai_http_accepted_stream(&gateway, &mut upstream, &mut stream)
        .expect("serve openai request");

    drop(stream);
    let response = support::finish_request(client);
    assert_eq!(upstream.chat_calls, 1);
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("content-type: text/event-stream\r\n"));
    assert!(response.contains("cache-control: no-cache\r\n"));
    assert!(!response.contains("content-length:"));
    assert!(response.contains("data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n"));
    assert!(response.contains("data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n"));
    assert!(response.contains("data: [DONE]\n\n"));
}
