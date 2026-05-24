use bm_llm_gateway::{
    handle_openai_request_with_services, serve_openai_http_stream_with_services,
    GatewayAuditOutcome, GatewayAuditStage, GatewayConfig, GatewayRuntime, GatewayScopeRequest,
    GatewayScopeResolver, OpenAiCompatibleUpstream, OpenAiGatewayBody, OpenAiGatewayRequest,
    OpenAiGatewayServices, OpenAiUpstreamRequest, OpenAiUpstreamResponse,
};
use bm_sdk::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, MemoryProjectionRequest, Message,
    PressureLevel, ResponseBody, RuntimeLifecycleModeInput, StopReason, ToolChoicePolicy, ToolSpec,
};
use serde_json::json;
use std::io::{Cursor, Read, Write};

fn gateway_config() -> GatewayConfig {
    GatewayConfig::default_for_local_dev()
}

fn scope_request() -> GatewayScopeRequest {
    GatewayScopeRequest {
        auth_subject: Some("owner-token".to_string()),
        workspace_root_digest: Some("workspace-digest".to_string()),
        client_conversation_hint: Some("thread-7".to_string()),
        model_alias: Some("local".to_string()),
        ..GatewayScopeRequest::default()
    }
}

#[derive(Default)]
struct StaticHttpClient;

impl LlmHttpClient for StaticHttpClient {
    fn do_post(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> bm_sdk::Result<(u16, ResponseBody)> {
        Ok((200, ResponseBody::Heap(Vec::new())))
    }
}

struct StaticLlmClient;

impl LlmClient for StaticLlmClient {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> bm_sdk::Result<LlmResponse> {
        Ok(LlmResponse {
            content: "Summary: gateway maintenance".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

#[derive(Default)]
struct MockOpenAiUpstream {
    response: Option<OpenAiUpstreamResponse>,
}

impl MockOpenAiUpstream {
    fn with_response(response: OpenAiUpstreamResponse) -> Self {
        Self {
            response: Some(response),
        }
    }
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
        _request: OpenAiUpstreamRequest,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        Ok(self.response.take().unwrap_or_else(|| {
            OpenAiUpstreamResponse::json(
                200,
                json!({
                    "id": "chatcmpl-local",
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": "I will verify artifacts first.",
                            "tool_calls": [{
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "lookup",
                                    "arguments": "{\"query\":\"release\"}"
                                }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }]
                }),
            )
        }))
    }
}

#[test]
fn non_streaming_response_runs_post_reply_maintenance_when_services_are_injected() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOpenAiUpstream::default();
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    let response = handle_openai_request_with_services(
        &gateway,
        &config,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            scope_request(),
            json!({
                "model": "local",
                "messages": [{ "role": "user", "content": "remember release guard" }]
            }),
        ),
        &mut upstream,
        &mut services,
    )
    .expect("chat response");

    assert_eq!(response.status_code, 200);
    assert!(response.audit.stages.iter().any(|stage| {
        stage.stage == GatewayAuditStage::Maintenance
            && stage.outcome == GatewayAuditOutcome::Succeeded
    }));
}

#[test]
fn non_streaming_response_finalizes_turn_into_session_store() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOpenAiUpstream::default();
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    let response = handle_openai_request_with_services(
        &gateway,
        &config,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            scope_request(),
            json!({
                "model": "local",
                "messages": [{ "role": "user", "content": "remember release guard" }]
            }),
        ),
        &mut upstream,
        &mut services,
    )
    .expect("chat response");

    assert_eq!(response.status_code, 200);

    let resolved = GatewayScopeResolver::new(config.scope.clone())
        .resolve(&scope_request())
        .expect("scope");
    let runtime = gateway
        .runtime_for_scope(resolved.entry_scope)
        .expect("scoped runtime");
    let projection = runtime
        .runtime()
        .project(MemoryProjectionRequest {
            user_query: "what did I ask?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("projection");

    assert!(projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "remember release guard"));
    assert!(projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "I will verify artifacts first."));
}

#[test]
fn missing_maintenance_services_skip_without_polluting_successful_response() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOpenAiUpstream::default();
    let mut services = OpenAiGatewayServices::new();

    let response = handle_openai_request_with_services(
        &gateway,
        &config,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            scope_request(),
            json!({
                "model": "local",
                "messages": [{ "role": "user", "content": "remember release guard" }]
            }),
        ),
        &mut upstream,
        &mut services,
    )
    .expect("chat response");

    assert_eq!(response.status_code, 200);
    assert_eq!(
        response.body.into_json().expect("json body")["choices"][0]["message"]["content"],
        "I will verify artifacts first."
    );
    assert!(response.audit.stages.iter().any(|stage| {
        stage.stage == GatewayAuditStage::Maintenance
            && stage.outcome == GatewayAuditOutcome::NotExecuted
    }));

    let resolved = GatewayScopeResolver::new(config.scope.clone())
        .resolve(&scope_request())
        .expect("scope");
    let runtime = gateway
        .runtime_for_scope(resolved.entry_scope)
        .expect("scoped runtime");
    let projection = runtime
        .runtime()
        .project(MemoryProjectionRequest {
            user_query: "what did I ask?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("projection");
    assert!(projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "remember release guard"));
    assert!(projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "I will verify artifacts first."));
}

#[test]
fn maintenance_hidden_records_skipped_without_blocking_turn_commit() {
    let mut config = gateway_config();
    config.entry.capability.maintenance_enabled = false;
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOpenAiUpstream::default();
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    let response = handle_openai_request_with_services(
        &gateway,
        &config,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            scope_request(),
            json!({
                "model": "local",
                "messages": [{ "role": "user", "content": "remember release guard" }]
            }),
        ),
        &mut upstream,
        &mut services,
    )
    .expect("chat response");

    assert_eq!(response.status_code, 200);
    assert_eq!(
        response.body.into_json().expect("json body")["choices"][0]["message"]["content"],
        "I will verify artifacts first."
    );
    assert!(response.audit.stages.iter().any(|stage| {
        stage.stage == GatewayAuditStage::Maintenance
            && stage.outcome == GatewayAuditOutcome::Skipped
    }));

    let resolved = GatewayScopeResolver::new(config.scope.clone())
        .resolve(&scope_request())
        .expect("scope");
    let runtime = gateway
        .runtime_for_scope(resolved.entry_scope)
        .expect("scoped runtime");
    let projection = runtime
        .runtime()
        .project(MemoryProjectionRequest {
            user_query: "what did I ask?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("projection");
    assert!(projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "remember release guard"));
}

#[test]
fn streaming_response_records_maintenance_after_body_is_drained() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOpenAiUpstream::with_response(OpenAiUpstreamResponse::sse(
        200,
        vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n".to_string(),
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ],
    ));
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    let mut response = handle_openai_request_with_services(
        &gateway,
        &config,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            scope_request(),
            json!({
                "model": "local",
                "stream": true,
                "messages": [{ "role": "user", "content": "stream this" }]
            }),
        ),
        &mut upstream,
        &mut services,
    )
    .expect("stream response");

    assert!(response
        .audit
        .stages
        .iter()
        .all(|stage| stage.stage != GatewayAuditStage::Maintenance));
    match &mut response.body {
        OpenAiGatewayBody::Sse(body) => while body.next_chunk().expect("sse chunk").is_some() {},
        OpenAiGatewayBody::Json(_) => panic!("expected sse body"),
    }
    response.finish_deferred_maintenance(&mut services);

    assert!(response.audit.stages.iter().any(|stage| {
        stage.stage == GatewayAuditStage::Maintenance
            && stage.outcome == GatewayAuditOutcome::Succeeded
    }));
}

#[test]
fn streaming_response_without_done_does_not_commit_partial_assistant() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOpenAiUpstream::with_response(OpenAiUpstreamResponse::sse(
        200,
        vec!["data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n".to_string()],
    ));
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    let mut response = handle_openai_request_with_services(
        &gateway,
        &config,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            scope_request(),
            json!({
                "model": "local",
                "stream": true,
                "messages": [{ "role": "user", "content": "stream this" }]
            }),
        ),
        &mut upstream,
        &mut services,
    )
    .expect("stream response");

    match &mut response.body {
        OpenAiGatewayBody::Sse(body) => while body.next_chunk().expect("sse chunk").is_some() {},
        OpenAiGatewayBody::Json(_) => panic!("expected sse body"),
    }
    response.finish_deferred_maintenance(&mut services);

    assert!(response.audit.stages.iter().any(|stage| {
        stage.stage == GatewayAuditStage::Maintenance
            && stage.outcome == GatewayAuditOutcome::Skipped
    }));

    let resolved = GatewayScopeResolver::new(config.scope.clone())
        .resolve(&scope_request())
        .expect("scope");
    let runtime = gateway
        .runtime_for_scope(resolved.entry_scope)
        .expect("scoped runtime");
    let projection = runtime
        .runtime()
        .project(MemoryProjectionRequest {
            user_query: "what was streamed?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("projection");
    assert!(projection.context.recent_messages.is_empty());
}

struct MemoryStream {
    read: Cursor<Vec<u8>>,
    written: Vec<u8>,
}

impl MemoryStream {
    fn new(input: String) -> Self {
        Self {
            read: Cursor::new(input.into_bytes()),
            written: Vec::new(),
        }
    }

    fn written_string(&self) -> String {
        String::from_utf8(self.written.clone()).expect("utf8 response")
    }
}

impl Read for MemoryStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.read.read(buf)
    }
}

impl Write for MemoryStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.written.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn streaming_response_finishes_maintenance_after_sse_done_without_rewriting_chunks() {
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
    let mut stream = MemoryStream::new(request);
    let mut upstream = MockOpenAiUpstream::with_response(OpenAiUpstreamResponse::sse(
        200,
        vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n".to_string(),
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ],
    ));
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    serve_openai_http_stream_with_services(
        &gateway,
        &config,
        &mut upstream,
        &mut services,
        &mut stream,
    )
    .expect("serve openai request");

    let response = stream.written_string();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("content-type: text/event-stream\r\n"));
    assert!(!response.contains("content-length:"));
    assert!(response.contains("data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n"));
    assert!(response.contains("data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n"));
    assert!(response.contains("data: [DONE]\n\n"));
}
