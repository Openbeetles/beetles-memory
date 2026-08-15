use bm_entry::EntryRuntimeBaseConfig;
use bm_llm_gateway::{
    handle_ollama_request, GatewayAuditOutcome, GatewayAuditStage, GatewayConfig,
    GatewayProviderConfig, GatewayRuntime, GatewayScopeRequest, OllamaGatewayBody,
    OllamaGatewayRequest, OllamaNativeUpstream, OllamaPassthroughRequest, OllamaUpstreamRequest,
    OllamaUpstreamResponse,
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
    capability.maintenance_enabled = true;
    config.entry = EntryRuntimeBaseConfig {
        capability,
        ..config.entry.clone()
    };
    let mut provider = GatewayProviderConfig::ollama_native("http://127.0.0.1:11435/api");
    provider
        .model_aliases
        .push(("local".to_string(), "qwen2.5:7b".to_string()));
    config.providers.clear();
    config.providers.insert("ollama".to_string(), provider);
    config.default_provider = "ollama".to_string();
    config
}

fn scope_request() -> GatewayScopeRequest {
    GatewayScopeRequest {
        workspace_root_digest: Some("workspace-digest".to_string()),
        client_conversation_hint: Some("thread-ollama-thinking".to_string()),
        model_alias: Some("local".to_string()),
        ..GatewayScopeRequest::new(support::gateway_bearer_auth("owner-token"))
    }
}

fn seed_runtime_skill(gateway: &GatewayRuntime, config: &GatewayConfig) {
    let resolved = bm_llm_gateway::GatewayScopeResolver::new(config.scope.clone())
        .resolve(&scope_request())
        .expect("scope");
    let agent_id = resolved.entry_scope.identity.agent_id.clone();
    let runtime = gateway
        .runtime_for_scope(resolved.entry_scope)
        .expect("runtime");
    runtime
        .runtime()
        .write(MemoryWriteRequest::Procedural {
            writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                name: "thinking_privacy_style".to_string(),
                topic: "llm_gateway".to_string(),
                title: "Thinking privacy style".to_string(),
                summary: "The gateway must not persist or return hidden model thinking."
                    .to_string(),
                content: "Only final assistant content may enter maintenance.".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("thread-ollama-thinking".to_string()),
                observed_at: 1,
            })],
            owning_scope: support::runtime_skill_subject_scope(&agent_id),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed skill");
}

#[derive(Default)]
struct ThinkingMockOllamaUpstream {
    chat_calls: Vec<OllamaUpstreamRequest>,
    generate_calls: Vec<OllamaUpstreamRequest>,
    response: Option<OllamaUpstreamResponse>,
}

impl ThinkingMockOllamaUpstream {
    fn with_response(response: OllamaUpstreamResponse) -> Self {
        Self {
            response: Some(response),
            ..Self::default()
        }
    }
}

impl OllamaNativeUpstream for ThinkingMockOllamaUpstream {
    fn passthrough(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OllamaPassthroughRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        Ok(OllamaUpstreamResponse::json(200, json!({})))
    }

    fn chat(
        &mut self,
        _provider: &GatewayProviderConfig,
        request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.chat_calls.push(request);
        Ok(self.response.take().unwrap_or_else(|| {
            OllamaUpstreamResponse::json(
                200,
                json!({
                    "model": "qwen2.5:7b",
                    "message": {
                        "role": "assistant",
                        "content": "final answer",
                        "thinking": "SECRET_JSON_THINKING"
                    },
                    "thinking": "SECRET_TOP_LEVEL_THINKING",
                    "done": true
                }),
            )
        }))
    }

    fn generate(
        &mut self,
        _provider: &GatewayProviderConfig,
        request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.generate_calls.push(request);
        Ok(self.response.take().unwrap_or_else(|| {
            OllamaUpstreamResponse::json(
                200,
                json!({
                    "model": "qwen2.5:7b",
                    "response": "generated answer",
                    "thinking": "SECRET_GENERATE_THINKING",
                    "done": true
                }),
            )
        }))
    }
}

#[test]
fn chat_forces_think_false_and_strips_thinking_before_response_and_maintenance() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    seed_runtime_skill(&gateway, &config);
    let mut upstream = ThinkingMockOllamaUpstream::default();

    let response = handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/chat",
            scope_request(),
            json!({
                "model": "local",
                "stream": false,
                "think": true,
                "messages": [{ "role": "user", "content": "answer only" }]
            }),
        ),
        &mut upstream,
    )
    .expect("chat response");

    assert_eq!(upstream.chat_calls[0].body["think"], false);
    let body = response.body.json();
    assert_eq!(body["message"]["content"], "final answer");
    assert!(body["message"].get("thinking").is_none());
    assert!(body.get("thinking").is_none());
    assert!(response
        .audit
        .notes
        .contains(&"ollama_thinking_request_forced_false".to_string()));
    assert!(response
        .audit
        .notes
        .contains(&"ollama_thinking_response_stripped".to_string()));
    let audit_text = serde_json::to_string(&response.audit).expect("audit json");
    assert!(!audit_text.contains("SECRET_JSON_THINKING"));
    assert!(!audit_text.contains("SECRET_TOP_LEVEL_THINKING"));
    assert!(response.audit.stages.iter().any(|stage| {
        stage.stage == GatewayAuditStage::Maintenance
            && stage.outcome == GatewayAuditOutcome::Queued
    }));
}

#[test]
fn streaming_chat_strips_thinking_before_chunks_and_deferred_maintenance() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    seed_runtime_skill(&gateway, &config);
    let mut upstream = ThinkingMockOllamaUpstream::with_response(OllamaUpstreamResponse::ndjson(
        200,
        vec![
            "{\"message\":{\"role\":\"assistant\",\"content\":\"hel\",\"thinking\":\"SECRET_STREAM_THINKING\"},\"done\":false}\n".to_string(),
            "{\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"thinking\":\"SECRET_STREAM_TOP\",\"done\":true}\n".to_string(),
        ],
    ));

    let mut response = handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/chat",
            scope_request(),
            json!({
                "model": "local",
                "stream": true,
                "messages": [{ "role": "user", "content": "stream answer only" }]
            }),
        ),
        &mut upstream,
    )
    .expect("stream response");

    assert_eq!(upstream.chat_calls[0].body["think"], false);
    let chunks = match &mut response.body {
        OllamaGatewayBody::Ndjson(body) => {
            let mut chunks = Vec::new();
            while let Some(chunk) = body.next_chunk().expect("chunk") {
                chunks.push(chunk);
            }
            chunks
        }
        OllamaGatewayBody::Json(_) => panic!("expected ndjson body"),
    };
    let returned = chunks.join("");
    assert!(returned.contains("\"content\":\"hel\""));
    assert!(!returned.contains("SECRET_STREAM_THINKING"));
    assert!(!returned.contains("SECRET_STREAM_TOP"));
    assert!(!returned.contains("\"thinking\""));
    response.finish_deferred_maintenance();
    assert!(response.audit.stages.iter().any(|stage| {
        stage.stage == GatewayAuditStage::Maintenance
            && stage.outcome == GatewayAuditOutcome::Queued
    }));
}

trait OllamaGatewayBodyAssertions {
    fn json(&self) -> &Value;
}

impl OllamaGatewayBodyAssertions for OllamaGatewayBody {
    fn json(&self) -> &Value {
        match self {
            OllamaGatewayBody::Json(value) => value,
            OllamaGatewayBody::Ndjson(_) => panic!("expected json body"),
        }
    }
}
