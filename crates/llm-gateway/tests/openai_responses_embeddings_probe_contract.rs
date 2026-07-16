use bm_entry::EntryRuntimeBaseConfig;
use bm_llm_gateway::{
    handle_openai_request, probe_openai_provider_capabilities, GatewayConfig, GatewayErrorKey,
    GatewayProjectionAuditStatus, GatewayRuntime, GatewayScopeRequest, GatewayScopeResolver,
    OpenAiCompatibleUpstream, OpenAiGatewayBody, OpenAiGatewayRequest, OpenAiUpstreamRequest,
    OpenAiUpstreamResponse,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryWriteRequest, RuntimeSkillWrite, RuntimeSkillWriteSource,
};
use serde_json::{json, Value};

mod support;

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
        workspace_root_digest: Some("workspace-digest".to_string()),
        client_conversation_hint: Some("thread-7".to_string()),
        model_alias: Some("local".to_string()),
        ..GatewayScopeRequest::new(support::gateway_bearer_auth("owner-token"))
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
                name: "responses_gateway_style".to_string(),
                topic: "llm_gateway_responses".to_string(),
                title: "Responses gateway style".to_string(),
                summary: "Always mention the responses boundary.".to_string(),
                content: "When answering through Responses, keep the memory boundary explicit."
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
    response_calls: Vec<OpenAiUpstreamRequest>,
    embedding_calls: Vec<OpenAiUpstreamRequest>,
}

impl OpenAiCompatibleUpstream for MockOpenAiUpstream {
    fn models(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        self.model_calls += 1;
        Ok(OpenAiUpstreamResponse::json(
            200,
            json!({ "data": [{ "id": "qwen-local" }, { "id": "embed-local" }] }),
        ))
    }

    fn chat_completion(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        _request: OpenAiUpstreamRequest,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        panic!("chat completion must not be called by this contract")
    }

    fn responses(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        request: OpenAiUpstreamRequest,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        self.response_calls.push(request);
        Ok(OpenAiUpstreamResponse::json(
            200,
            json!({
                "id": "resp-local",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "ok" }]
                }],
                "output_text": "ok"
            }),
        ))
    }

    fn embeddings(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        request: OpenAiUpstreamRequest,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        self.embedding_calls.push(request);
        Ok(OpenAiUpstreamResponse::json(
            200,
            json!({ "data": [{ "embedding": [0.1, 0.2], "index": 0 }] }),
        ))
    }
}

#[test]
fn responses_stateless_injects_memory_into_instructions_and_preserves_payload() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let scope = scope_request();
    seed_runtime_skill(&gateway, &config, &scope);
    let mut upstream = MockOpenAiUpstream::default();

    let response = handle_openai_request(
        &gateway,
        OpenAiGatewayRequest::post_json(
            "/v1/responses",
            scope,
            json!({
                "model": "local",
                "instructions": "Keep the answer short.",
                "input": [{
                    "role": "user",
                    "content": [
                        { "type": "input_text", "text": "How should Responses answer?" },
                        { "type": "input_image", "image_url": "file://diagram.png" }
                    ]
                }],
                "tools": [{ "type": "function", "name": "lookup" }],
                "metadata": { "keep": true }
            }),
        ),
        &mut upstream,
    )
    .expect("responses response");

    assert_eq!(response.status_code, 200);
    assert_eq!(upstream.response_calls.len(), 1);
    let sent = &upstream.response_calls[0];
    assert_eq!(sent.endpoint, "/responses");
    assert_eq!(sent.model, "local");
    assert_eq!(sent.extracted_user_text, "How should Responses answer?");
    assert!(!sent.stream);
    assert_eq!(sent.body["metadata"]["keep"], true);
    assert_eq!(
        sent.body["input"][0]["content"][1]["image_url"],
        "file://diagram.png"
    );
    let instructions = sent.body["instructions"]
        .as_str()
        .expect("instructions string");
    assert!(instructions.contains("Keep the answer short."));
    assert!(instructions.contains("<beetle-memory-projection version=\"1\">"));
    assert_eq!(
        response.audit.projection_record.status,
        GatewayProjectionAuditStatus::NotRecorded
    );
    assert_eq!(
        response.audit.projection_record.reason,
        "raw_projection_recording_disabled"
    );
    assert!(response.audit.projection_record.projection_chars > 0);
    assert!(response.audit.projection_record.block.is_none());
    assert_eq!(response.body.json()["output_text"], "ok");
}

#[test]
fn responses_previous_response_id_is_capability_error_without_upstream_call() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOpenAiUpstream::default();

    let error = handle_openai_request(
        &gateway,
        OpenAiGatewayRequest::post_json(
            "/v1/responses",
            scope_request(),
            json!({
                "model": "local",
                "previous_response_id": "resp_123",
                "input": "continue the stateful response"
            }),
        ),
        &mut upstream,
    )
    .expect_err("stateful responses must be blocked unless provider supports them");

    assert_eq!(error.key(), GatewayErrorKey::ProviderUnavailable);
    assert!(upstream.response_calls.is_empty());
}

#[test]
fn embeddings_passthrough_does_not_project_or_maintain() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOpenAiUpstream::default();

    let response = handle_openai_request(
        &gateway,
        OpenAiGatewayRequest::post_json(
            "/v1/embeddings",
            scope_request(),
            json!({
                "model": "embed-local",
                "input": ["alpha", "beta"],
                "encoding_format": "float"
            }),
        ),
        &mut upstream,
    )
    .expect("embeddings response");

    assert_eq!(upstream.embedding_calls.len(), 1);
    let sent = &upstream.embedding_calls[0];
    assert_eq!(sent.endpoint, "/embeddings");
    assert_eq!(sent.model, "embed-local");
    assert_eq!(sent.body["input"][0], "alpha");
    assert_eq!(response.body.json()["data"][0]["embedding"][1], 0.2);
    assert!(response
        .audit
        .stages
        .iter()
        .all(|stage| stage.stage != bm_llm_gateway::GatewayAuditStage::Projection));
}

#[test]
fn provider_probe_reports_per_model_openai_capabilities() {
    let config = gateway_config();
    let mut upstream = MockOpenAiUpstream::default();

    let report = probe_openai_provider_capabilities(&config, Some("local"), &mut upstream)
        .expect("capability report");

    assert_eq!(upstream.model_calls, 1);
    assert_eq!(report.provider_name, "local");
    assert!(report.chat_completions);
    assert!(report.responses);
    assert!(!report.stateful_responses);
    assert!(report.embeddings);
    assert!(report.streaming);
    assert_eq!(report.models.len(), 2);
    assert_eq!(report.models[0].model, "qwen-local");
    assert!(report.models[0].responses);
    assert!(report.models[1].embeddings);
}

trait OpenAiGatewayBodyAssertions {
    fn json(&self) -> &Value;
}

impl OpenAiGatewayBodyAssertions for OpenAiGatewayBody {
    fn json(&self) -> &Value {
        match self {
            OpenAiGatewayBody::Json(value) => value,
            OpenAiGatewayBody::Sse(_) => panic!("expected json body"),
        }
    }
}
