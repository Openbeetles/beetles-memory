use bm_entry::EntryRuntimeBaseConfig;
use bm_llm_gateway::{
    classify_ollama_route, GatewayAuditStage, GatewayConfig, GatewayProviderConfig, GatewayRuntime,
    GatewayScopeRequest, GatewayScopeResolver, OllamaGatewayMethod, OllamaGatewayRequest,
    OllamaNativeUpstream, OllamaPassthroughRequest, OllamaRouteAction, OllamaUpstreamRequest,
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
        client_conversation_hint: Some("thread-ollama-transparent".to_string()),
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
    let agent_id = resolved.entry_scope.identity.agent_id.clone();
    let runtime = gateway
        .runtime_for_scope(resolved.entry_scope)
        .expect("runtime");
    runtime
        .runtime()
        .write(MemoryWriteRequest::Procedural {
            writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                name: "transparent_ollama_style".to_string(),
                topic: "llm_gateway".to_string(),
                title: "Transparent Ollama style".to_string(),
                summary: "Transparent Ollama requests must still receive memory projection."
                    .to_string(),
                content: "Keep Beetle Memory projection visible for chat and generate.".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("thread-ollama-transparent".to_string()),
                observed_at: 1,
            })],
            owning_scope: support::runtime_skill_subject_scope(&agent_id),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed skill");
}

#[derive(Default)]
struct TransparentMockOllamaUpstream {
    passthrough_calls: Vec<OllamaPassthroughRequest>,
    chat_calls: Vec<OllamaUpstreamRequest>,
    generate_calls: Vec<OllamaUpstreamRequest>,
}

impl OllamaNativeUpstream for TransparentMockOllamaUpstream {
    fn passthrough(
        &mut self,
        _provider: &GatewayProviderConfig,
        request: OllamaPassthroughRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        let method = format!("{:?}", request.method);
        let path = request.path.clone();
        let body = request.body.clone().unwrap_or(Value::Null);
        self.passthrough_calls.push(request);
        Ok(OllamaUpstreamResponse::json(
            200,
            json!({ "method": method, "path": path, "body": body }),
        ))
    }

    fn chat(
        &mut self,
        _provider: &GatewayProviderConfig,
        request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.chat_calls.push(request);
        Ok(OllamaUpstreamResponse::json(
            200,
            json!({
                "model": "qwen2.5:7b",
                "message": { "role": "assistant", "content": "chat ok" },
                "done": true
            }),
        ))
    }

    fn generate(
        &mut self,
        _provider: &GatewayProviderConfig,
        request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.generate_calls.push(request);
        Ok(OllamaUpstreamResponse::json(
            200,
            json!({
                "model": "qwen2.5:7b",
                "response": "generate ok",
                "done": true
            }),
        ))
    }
}

#[test]
fn chat_and_generate_still_enter_projection_instead_of_passthrough() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let scope = scope_request();
    seed_runtime_skill(&gateway, &config, &scope);
    let mut upstream = TransparentMockOllamaUpstream::default();

    bm_llm_gateway::handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/chat",
            scope.clone(),
            json!({
                "model": "local",
                "stream": false,
                "messages": [{ "role": "user", "content": "teach through chat" }]
            }),
        ),
        &mut upstream,
    )
    .expect("chat response");
    bm_llm_gateway::handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/generate",
            scope,
            json!({
                "model": "local",
                "stream": false,
                "prompt": "teach through generate"
            }),
        ),
        &mut upstream,
    )
    .expect("generate response");

    assert!(upstream.passthrough_calls.is_empty());
    assert_eq!(upstream.chat_calls.len(), 1);
    assert_eq!(upstream.generate_calls.len(), 1);
    let chat_system = upstream.chat_calls[0].body["messages"][0]["content"]
        .as_str()
        .expect("chat system");
    let generate_system = upstream.generate_calls[0].body["system"]
        .as_str()
        .expect("generate system");
    assert!(chat_system.contains("<beetle-memory-projection version=\"1\">"));
    assert!(generate_system.contains("<beetle-memory-projection version=\"1\">"));
}

#[test]
fn transparent_app_endpoints_passthrough_without_projection_or_maintenance() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = TransparentMockOllamaUpstream::default();

    let requests = vec![
        OllamaGatewayRequest::get("/api/version", scope_request()),
        OllamaGatewayRequest::get("/api/tags", scope_request()),
        OllamaGatewayRequest::post_json(
            "/api/show",
            scope_request(),
            json!({ "model": "local", "verbose": true }),
        ),
        OllamaGatewayRequest::post_json("/api/me", scope_request(), json!({})),
        OllamaGatewayRequest::get("/api/experimental/model-recommendations", scope_request()),
    ];

    let responses = requests
        .into_iter()
        .map(|request| {
            bm_llm_gateway::handle_ollama_request(&gateway, request, &mut upstream)
                .expect("passthrough response")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        upstream
            .passthrough_calls
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/api/version",
            "/api/tags",
            "/api/show",
            "/api/me",
            "/api/experimental/model-recommendations"
        ]
    );
    for response in responses {
        assert!(response.audit.stages.iter().all(|stage| {
            stage.stage != GatewayAuditStage::Projection
                && stage.stage != GatewayAuditStage::Maintenance
        }));
    }
}

#[test]
fn unknown_api_routes_follow_central_passthrough_policy() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = TransparentMockOllamaUpstream::default();

    let decision = classify_ollama_route(OllamaGatewayMethod::Post, "/api/future/app-endpoint");
    assert_eq!(decision.action, OllamaRouteAction::Passthrough);
    assert!(decision.known_endpoint.is_none());

    let response = bm_llm_gateway::handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/future/app-endpoint",
            scope_request(),
            json!({ "opaque": true }),
        ),
        &mut upstream,
    )
    .expect("unknown api passthrough response");

    assert_eq!(upstream.passthrough_calls.len(), 1);
    assert_eq!(
        upstream.passthrough_calls[0].path,
        "/api/future/app-endpoint"
    );
    assert_eq!(
        response.audit.notes,
        vec!["ollama_passthrough_unknown_api_endpoint".to_string()]
    );

    let rejected = bm_llm_gateway::handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::get("/internal/not-ollama", scope_request()),
        &mut upstream,
    )
    .expect_err("non api route must be rejected by policy");
    assert_eq!(
        rejected.key(),
        bm_llm_gateway::GatewayErrorKey::InvalidRequest
    );
}
