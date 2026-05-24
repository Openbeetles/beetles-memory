use bm_entry::EntryRuntimeBaseConfig;
use bm_llm_gateway::{
    handle_openai_request, GatewayConfig, GatewayErrorKey, GatewayRuntime, GatewayScopeRequest,
    GatewayScopeResolver, OpenAiCompatibleUpstream, OpenAiGatewayBody, OpenAiGatewayRequest,
    OpenAiUpstreamRequest, OpenAiUpstreamResponse,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryWriteRequest, RuntimeSkillWrite, RuntimeSkillWriteSource,
};
use serde_json::{json, Value};

fn gateway_config() -> GatewayConfig {
    let mut config = GatewayConfig::default_for_local_dev();
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    config.entry = EntryRuntimeBaseConfig {
        capability,
        ..config.entry.clone()
    };
    config
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

fn seed_runtime_skill(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    scope: &GatewayScopeRequest,
) {
    let resolved = GatewayScopeResolver::new(config.scope.clone())
        .resolve(scope)
        .expect("scope");
    let runtime = gateway
        .runtime_for_scope(resolved.entry_scope)
        .expect("runtime");
    runtime
        .runtime()
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "gateway_style".to_string(),
                topic: "llm_gateway".to_string(),
                title: "Gateway reply style".to_string(),
                summary: "Always mention the hardware gateway boundary.".to_string(),
                content: "When answering gateway questions, keep the hardware boundary explicit."
                    .to_string(),
                citations: Vec::new(),
                source_chat_id: Some("thread-7".to_string()),
                observed_at: 1,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed skill");
}

#[derive(Default)]
struct MockOpenAiUpstream {
    model_calls: usize,
    chat_calls: Vec<OpenAiUpstreamRequest>,
    response: Option<OpenAiUpstreamResponse>,
}

impl MockOpenAiUpstream {
    fn with_response(response: OpenAiUpstreamResponse) -> Self {
        Self {
            response: Some(response),
            ..Self::default()
        }
    }
}

impl OpenAiCompatibleUpstream for MockOpenAiUpstream {
    fn models(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        self.model_calls += 1;
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
        self.chat_calls.push(request);
        Ok(self.response.take().unwrap_or_else(|| {
            OpenAiUpstreamResponse::json(
                200,
                json!({
                    "id": "chatcmpl-local",
                    "choices": [{
                        "message": { "role": "assistant", "content": "ok" },
                        "finish_reason": "stop"
                    }]
                }),
            )
        }))
    }
}

#[test]
fn models_endpoint_proxies_openai_provider_models_without_projection() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOpenAiUpstream::default();

    let response = handle_openai_request(
        &gateway,
        &config,
        OpenAiGatewayRequest::get("/v1/models", scope_request()),
        &mut upstream,
    )
    .expect("models response");

    assert_eq!(upstream.model_calls, 1);
    assert_eq!(response.status_code, 200);
    assert_eq!(response.body.json()["data"][0]["id"], "qwen-local");
    assert!(response
        .audit
        .stages
        .iter()
        .all(|stage| stage.stage != bm_llm_gateway::GatewayAuditStage::Projection));
}

#[test]
fn chat_non_streaming_injects_memory_and_preserves_openai_payload_shape() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let scope = scope_request();
    seed_runtime_skill(&gateway, &config, &scope);
    let mut upstream = MockOpenAiUpstream::default();

    let response = handle_openai_request(
        &gateway,
        &config,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            scope,
            json!({
                "model": "local",
                "messages": [{
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "How should the gateway answer?" },
                        { "type": "image_url", "image_url": { "url": "file://diagram.png" } }
                    ]
                }],
                "tools": [{ "type": "function", "function": { "name": "lookup" } }],
                "tool_choice": "auto",
                "response_format": { "type": "json_object" },
                "vendor_option": { "keep": true }
            }),
        ),
        &mut upstream,
    )
    .expect("chat response");

    assert_eq!(response.status_code, 200);
    assert_eq!(upstream.chat_calls.len(), 1);
    let sent = &upstream.chat_calls[0];
    assert!(!sent.stream);
    assert_eq!(sent.extracted_user_text, "How should the gateway answer?");
    assert!(sent.body["tools"].is_array());
    assert_eq!(sent.body["tool_choice"], "auto");
    assert_eq!(sent.body["response_format"]["type"], "json_object");
    assert_eq!(sent.body["vendor_option"]["keep"], true);
    assert_eq!(
        sent.body["messages"][1]["content"][1]["image_url"]["url"],
        "file://diagram.png"
    );
    assert_eq!(sent.body["messages"][0]["role"], "system");
    assert!(sent.body["messages"][0]["content"]
        .as_str()
        .expect("memory system content")
        .contains("<beetle-memory-projection version=\"1\">"));
    assert_eq!(
        response.body.json()["choices"][0]["message"]["content"],
        "ok"
    );
}

#[test]
fn chat_full_history_uses_latest_user_message_as_projection_query() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let scope = scope_request();
    let mut upstream = MockOpenAiUpstream::default();

    handle_openai_request(
        &gateway,
        &config,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            scope,
            json!({
                "model": "local",
                "messages": [
                    { "role": "user", "content": "call me Qingchuan" },
                    { "role": "assistant", "content": "ok" },
                    { "role": "user", "content": "I like cold brew" }
                ]
            }),
        ),
        &mut upstream,
    )
    .expect("chat response");

    assert_eq!(upstream.chat_calls.len(), 1);
    assert_eq!(
        upstream.chat_calls[0].extracted_user_text,
        "I like cold brew"
    );
}

#[test]
fn chat_streaming_passes_through_sse_chunks_without_json_buffering() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let scope = scope_request();
    let mut upstream = MockOpenAiUpstream::with_response(OpenAiUpstreamResponse::sse(
        200,
        vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n".to_string(),
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ],
    ));

    let response = handle_openai_request(
        &gateway,
        &config,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            scope,
            json!({
                "model": "local",
                "stream": true,
                "messages": [{ "role": "user", "content": "stream this" }]
            }),
        ),
        &mut upstream,
    )
    .expect("stream response");

    assert!(upstream.chat_calls[0].stream);
    assert!(response.body.is_sse());
    assert_eq!(
        response.body.sse_chunks(),
        &[
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n".to_string(),
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n".to_string(),
            "data: [DONE]\n\n".to_string()
        ]
    );
}

#[test]
fn projection_failure_is_fail_closed_and_does_not_call_upstream() {
    let mut config = gateway_config();
    config.entry.capability.projection_enabled = false;
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOpenAiUpstream::default();

    let error = handle_openai_request(
        &gateway,
        &config,
        OpenAiGatewayRequest::post_json(
            "/v1/chat/completions",
            scope_request(),
            json!({
                "model": "local",
                "messages": [{ "role": "user", "content": "do not call upstream" }]
            }),
        ),
        &mut upstream,
    )
    .expect_err("projection failure must stop request");

    assert_eq!(error.key(), GatewayErrorKey::ProjectionFailed);
    assert!(upstream.chat_calls.is_empty());
}

trait OpenAiGatewayBodyAssertions {
    fn json(&self) -> &Value;
    fn sse_chunks(&self) -> &[String];
}

impl OpenAiGatewayBodyAssertions for OpenAiGatewayBody {
    fn json(&self) -> &Value {
        match self {
            OpenAiGatewayBody::Json(value) => value,
            OpenAiGatewayBody::Sse(_) => panic!("expected json body"),
        }
    }

    fn sse_chunks(&self) -> &[String] {
        self.buffered_sse_chunks()
            .expect("expected buffered sse body")
    }
}
