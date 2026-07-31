use bm_entry::EntryRuntimeBaseConfig;
use bm_llm_gateway::{
    handle_ollama_request, handle_ollama_request_with_services,
    serve_llm_gateway_http_accepted_stream_with_services, GatewayAuditOutcome, GatewayAuditStage,
    GatewayConfig, GatewayProjectionAuditStatus, GatewayProviderConfig, GatewayRuntime,
    GatewayScopeRequest, GatewayScopeResolver, OllamaGatewayBody, OllamaGatewayRequest,
    OllamaMaintenanceLlmClient, OllamaNativeUpstream, OllamaUpstreamRequest,
    OllamaUpstreamResponse, OpenAiCompatibleUpstream, OpenAiGatewayServices, OpenAiUpstreamRequest,
    OpenAiUpstreamResponse,
};
use bm_sdk::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, MemoryCapabilityPolicy,
    MemoryProjectionRequest, MemoryRuntime, MemoryTranscriptReplayReport,
    MemoryTranscriptReplayRequest, MemoryWriteRequest, Message, PressureLevel, ResponseBody,
    RuntimeLifecycleModeInput, RuntimeSkillWrite, RuntimeSkillWriteSource, StopReason,
    ToolChoicePolicy, ToolSpec, TranscriptReplayView,
};
use serde_json::{json, Value};
use std::borrow::Cow;

mod support;

const FIXTURE_CONVERSATION_ID: &str = "thread-ollama";

fn replay_model_context(runtime: &MemoryRuntime) -> MemoryTranscriptReplayReport {
    let scope = runtime.scope();
    runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: scope.channel.clone(),
            conversation_id: FIXTURE_CONVERSATION_ID.to_string(),
            limit: 32,
            cursor: None,
            view: TranscriptReplayView::ModelContext,
        })
        .expect("model-context transcript replay")
}

fn gateway_config() -> GatewayConfig {
    let mut config = GatewayConfig::default_for_local_dev();
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    config.entry = EntryRuntimeBaseConfig {
        capability,
        ..config.entry.clone()
    };
    let mut provider = GatewayProviderConfig::ollama_native("http://127.0.0.1:11434/api");
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
        client_conversation_hint: Some(FIXTURE_CONVERSATION_ID.to_string()),
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
                name: "ollama_gateway_style".to_string(),
                topic: "llm_gateway".to_string(),
                title: "Ollama gateway reply style".to_string(),
                summary: "Always keep the Ollama native boundary explicit.".to_string(),
                content: "When answering through Ollama native, keep protocol fields intact."
                    .to_string(),
                citations: Vec::new(),
                source_chat_id: Some("thread-ollama".to_string()),
                observed_at: 1,
            })],
            owning_scope: support::runtime_skill_subject_scope(&agent_id),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed skill");
}

#[derive(Default)]
struct MockOllamaUpstream {
    tags_calls: usize,
    version_calls: usize,
    chat_calls: Vec<OllamaUpstreamRequest>,
    generate_calls: Vec<OllamaUpstreamRequest>,
    embed_calls: Vec<Value>,
    embeddings_calls: Vec<Value>,
    show_calls: Vec<Value>,
    response: Option<OllamaUpstreamResponse>,
}

impl MockOllamaUpstream {
    fn with_response(response: OllamaUpstreamResponse) -> Self {
        Self {
            response: Some(response),
            ..Self::default()
        }
    }
}

impl OllamaNativeUpstream for MockOllamaUpstream {
    fn tags(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.tags_calls += 1;
        Ok(OllamaUpstreamResponse::json(
            200,
            json!({ "models": [{ "name": "qwen2.5:7b", "model": "qwen2.5:7b" }] }),
        ))
    }

    fn version(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.version_calls += 1;
        Ok(OllamaUpstreamResponse::json(
            200,
            json!({ "version": "0.12.6" }),
        ))
    }

    fn chat(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.chat_calls.push(request);
        Ok(self.response.take().unwrap_or_else(|| {
            OllamaUpstreamResponse::json(
                200,
                json!({
                    "model": "qwen2.5:7b",
                    "message": { "role": "assistant", "content": "ok" },
                    "done": true
                }),
            )
        }))
    }

    fn generate(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        request: OllamaUpstreamRequest,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.generate_calls.push(request);
        Ok(self.response.take().unwrap_or_else(|| {
            OllamaUpstreamResponse::json(
                200,
                json!({
                    "model": "qwen2.5:7b",
                    "response": "generated",
                    "done": true
                }),
            )
        }))
    }

    fn embed(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        body: Value,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.embed_calls.push(body);
        Ok(OllamaUpstreamResponse::json(
            200,
            json!({ "model": "embeddinggemma", "embeddings": [[0.1, 0.2]] }),
        ))
    }

    fn embeddings(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        body: Value,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.embeddings_calls.push(body);
        Ok(OllamaUpstreamResponse::json(
            200,
            json!({ "embedding": [0.1, 0.2] }),
        ))
    }

    fn show(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        body: Value,
    ) -> bm_llm_gateway::Result<OllamaUpstreamResponse> {
        self.show_calls.push(body);
        Ok(OllamaUpstreamResponse::json(
            200,
            json!({ "capabilities": ["completion"], "details": { "family": "qwen2" } }),
        ))
    }
}

#[derive(Default)]
struct MockOpenAiUpstream;

impl OpenAiCompatibleUpstream for MockOpenAiUpstream {
    fn models(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        Ok(OpenAiUpstreamResponse::json(200, json!({ "data": [] })))
    }

    fn chat_completion(
        &mut self,
        _provider: &bm_llm_gateway::GatewayProviderConfig,
        _request: OpenAiUpstreamRequest,
    ) -> bm_llm_gateway::Result<OpenAiUpstreamResponse> {
        Ok(OpenAiUpstreamResponse::json(
            200,
            json!({ "choices": [{ "message": { "content": "unused" } }] }),
        ))
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
            content: "Summary: ollama maintenance".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

struct LongTermExtractionLlmClient;

impl LlmClient for LongTermExtractionLlmClient {
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
            content: r#"[
                {
                    "plane": "factual",
                    "op": "upsert",
                    "kind": "profile",
                    "source_authority": "user_asserted",
                    "topic": "preferred_name",
                    "content": "The user asked to be called Qingchuan.",
                    "keywords": ["Qingchuan", "preferred name"]
                }
            ]"#
            .to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

#[test]
fn tags_and_version_proxy_without_projection_or_maintenance() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOllamaUpstream::default();

    let tags = handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::get("/api/tags", scope_request()),
        &mut upstream,
    )
    .expect("tags response");
    let version = handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::get("/api/version", scope_request()),
        &mut upstream,
    )
    .expect("version response");

    assert_eq!(upstream.tags_calls, 1);
    assert_eq!(upstream.version_calls, 1);
    assert_eq!(tags.body.json()["models"][0]["model"], "qwen2.5:7b");
    assert_eq!(version.body.json()["version"], "0.12.6");
    assert!(tags
        .audit
        .stages
        .iter()
        .all(|stage| stage.stage != GatewayAuditStage::Projection
            && stage.stage != GatewayAuditStage::Maintenance));
    assert!(version
        .audit
        .stages
        .iter()
        .all(|stage| stage.stage != GatewayAuditStage::Projection
            && stage.stage != GatewayAuditStage::Maintenance));
}

#[test]
fn chat_non_streaming_injects_memory_into_existing_system_and_preserves_native_shape() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let scope = scope_request();
    seed_runtime_skill(&gateway, &config, &scope);
    let mut upstream = MockOllamaUpstream::default();

    let response = handle_ollama_request(
        &gateway,
                OllamaGatewayRequest::post_json(
            "/api/chat",
            scope,
            json!({
                "model": "local",
                "stream": false,
                "messages": [
                    { "role": "system", "content": "Base system." },
                    { "role": "user", "content": "How should native Ollama answer?", "images": ["base64-image"] }
                ],
                "tools": [{ "type": "function", "function": { "name": "lookup" } }],
                "options": { "temperature": 0.2 },
                "format": "json",
                "keep_alive": "5m",
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
    assert_eq!(sent.endpoint, "/chat");
    assert_eq!(sent.model, "qwen2.5:7b");
    assert_eq!(sent.extracted_user_text, "How should native Ollama answer?");
    assert_eq!(sent.body["model"], "qwen2.5:7b");
    assert_eq!(sent.body["options"]["temperature"], 0.2);
    assert_eq!(sent.body["format"], "json");
    assert_eq!(sent.body["keep_alive"], "5m");
    assert_eq!(sent.body["vendor_option"]["keep"], true);
    assert_eq!(sent.body["messages"][1]["images"][0], "base64-image");
    assert_eq!(sent.body["messages"][0]["role"], "system");
    let system = sent.body["messages"][0]["content"]
        .as_str()
        .expect("system content");
    assert!(system.contains("Base system."));
    assert!(system.contains("<beetle-memory-projection version=\"1\">"));
    assert_eq!(sent.body["messages"].as_array().expect("messages").len(), 2);
    assert_eq!(response.body.json()["message"]["content"], "ok");
    assert!(response
        .audit
        .notes
        .contains(&"gateway_host_tools_no_cold_route".to_string()));
}

#[test]
fn chat_non_streaming_finalizes_turn_into_session_store_after_done_true() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOllamaUpstream::default();
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    let response = handle_ollama_request_with_services(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/chat",
            scope_request(),
            json!({
                "model": "local",
                "stream": false,
                "messages": [{
                    "role": "user",
                    "content": "call me Qingchuan",
                    "speaker_id": "owner-human",
                    "speaker_kind": "human"
                }]
            }),
        ),
        &mut upstream,
        &mut services,
    )
    .expect("chat response");

    assert_eq!(response.status_code, 200);
    assert_eq!(response.body.json()["message"]["content"], "ok");
    assert!(response.audit.stages.iter().any(|stage| {
        stage.stage == GatewayAuditStage::Maintenance
            && stage.outcome == GatewayAuditOutcome::Succeeded
    }));

    let resolved = GatewayScopeResolver::new(config.scope.clone())
        .resolve(&scope_request())
        .expect("scope");
    let runtime = gateway
        .runtime_for_scope(resolved.entry_scope)
        .expect("scoped runtime");
    let replay = replay_model_context(runtime.runtime());
    let user_message = replay
        .slice
        .turns
        .iter()
        .flat_map(|turn| turn.input_messages.iter())
        .find(|message| message.content.as_deref() == Some("call me Qingchuan"))
        .expect("user message persisted");
    assert!(user_message.message_id.starts_with("msg_"));
    assert!(user_message.observed_at > 0);
    assert_eq!(user_message.role, "user");
    assert_eq!(user_message.actor.speaker_id, "owner-human");
    assert_eq!(user_message.actor.speaker_kind, "human");
    let assistant_message = replay
        .slice
        .turns
        .iter()
        .filter_map(|turn| turn.assistant_message.as_ref())
        .find(|message| message.content.as_deref() == Some("ok"))
        .expect("assistant message persisted");
    assert!(assistant_message.message_id.starts_with("msg_"));
    assert_eq!(assistant_message.role, "assistant");
    assert_eq!(assistant_message.actor.speaker_id, "assistant");
    assert_eq!(assistant_message.actor.speaker_kind, "llm_agent");
}

#[test]
fn chat_full_history_finalizes_only_new_user_delta_for_same_ollama_thread() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOllamaUpstream::default();
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);
    let scope = scope_request();

    handle_ollama_request_with_services(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/chat",
            scope.clone(),
            json!({
                "model": "local",
                "stream": false,
                "messages": [{ "role": "user", "content": "call me Qingchuan" }]
            }),
        ),
        &mut upstream,
        &mut services,
    )
    .expect("first chat response");

    handle_ollama_request_with_services(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/chat",
            scope.clone(),
            json!({
                "model": "local",
                "stream": false,
                "messages": [
                    { "role": "user", "content": "call me Qingchuan" },
                    { "role": "assistant", "content": "ok" },
                    { "role": "user", "content": "I like cold brew" }
                ]
            }),
        ),
        &mut upstream,
        &mut services,
    )
    .expect("second chat response");

    assert_eq!(upstream.chat_calls.len(), 2);
    assert_eq!(
        upstream.chat_calls[1].extracted_user_text,
        "I like cold brew"
    );

    let resolved = GatewayScopeResolver::new(config.scope.clone())
        .resolve(&scope)
        .expect("scope");
    let runtime = gateway
        .runtime_for_scope(resolved.entry_scope)
        .expect("scoped runtime");
    let replay = replay_model_context(runtime.runtime());
    let recent_contents = replay
        .slice
        .turns
        .iter()
        .flat_map(|turn| {
            turn.input_messages
                .iter()
                .chain(turn.assistant_message.iter())
        })
        .filter_map(|message| message.content.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        recent_contents,
        vec!["call me Qingchuan", "ok", "I like cold brew", "ok"]
    );
    assert!(!recent_contents.contains(&"call me Qingchuan\nI like cold brew"));
}

#[test]
fn chat_non_streaming_applies_long_term_memory_for_new_ollama_chat_projection() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOllamaUpstream::default();
    let mut http = StaticHttpClient;
    let llm = LongTermExtractionLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    let response = handle_ollama_request_with_services(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/chat",
            scope_request(),
            json!({
                "model": "local",
                "stream": false,
                "messages": [{ "role": "user", "content": "以后叫我青川" }]
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

    let mut next_chat_scope = scope_request();
    next_chat_scope.client_conversation_hint = Some("thread-ollama-new".to_string());
    let resolved = GatewayScopeResolver::new(config.scope.clone())
        .resolve(&next_chat_scope)
        .expect("scope");
    let runtime = gateway
        .runtime_for_scope(resolved.entry_scope)
        .expect("scoped runtime");
    let projection = runtime
        .runtime()
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "我叫什么？".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(projection.report().recall_delivery().rendered_count > 0);
    assert!(projection
        .provider_payload()
        .system_memory_block()
        .contains("Qingchuan"));
}

#[test]
fn generate_injects_system_field_without_prompt_prefix_when_supported() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let scope = scope_request();
    seed_runtime_skill(&gateway, &config, &scope);
    let mut upstream = MockOllamaUpstream::default();

    let response = handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/generate",
            scope,
            json!({
                "model": "local",
                "prompt": "Draft the hardware boundary.",
                "system": "Base generate system.",
                "stream": false,
                "options": { "num_ctx": 4096 },
                "format": { "type": "object" },
                "keep_alive": "10m"
            }),
        ),
        &mut upstream,
    )
    .expect("generate response");

    assert_eq!(response.status_code, 200);
    let sent = &upstream.generate_calls[0];
    assert_eq!(sent.endpoint, "/generate");
    assert_eq!(sent.body["model"], "qwen2.5:7b");
    assert_eq!(sent.body["prompt"], "Draft the hardware boundary.");
    assert_eq!(sent.body["options"]["num_ctx"], 4096);
    assert_eq!(sent.body["format"]["type"], "object");
    assert_eq!(sent.body["keep_alive"], "10m");
    let system = sent.body["system"].as_str().expect("system");
    assert!(system.contains("Base generate system."));
    assert!(system.contains("<beetle-memory-projection version=\"1\">"));
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
    assert!(!response
        .audit
        .notes
        .contains(&"ollama_generate_prompt_prefix_fallback".to_string()));
}

#[test]
fn generate_prompt_prefix_fallback_is_explicitly_audited_when_system_is_unsupported() {
    let mut config = gateway_config();
    config
        .providers
        .get_mut("ollama")
        .expect("ollama provider")
        .ollama_generate_system_supported = false;
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let scope = scope_request();
    seed_runtime_skill(&gateway, &config, &scope);
    let mut upstream = MockOllamaUpstream::default();

    let response = handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/generate",
            scope,
            json!({
                "model": "local",
                "prompt": "Use fallback.",
                "stream": false
            }),
        ),
        &mut upstream,
    )
    .expect("generate response");

    let sent = &upstream.generate_calls[0];
    assert!(sent.body.get("system").is_none());
    let prompt = sent.body["prompt"].as_str().expect("prompt");
    assert!(prompt.starts_with("<beetle-memory-projection version=\"1\">\n"));
    assert!(prompt.ends_with("Use fallback."));
    assert!(response
        .audit
        .notes
        .contains(&"ollama_generate_prompt_prefix_fallback".to_string()));
}

#[test]
fn chat_streaming_passes_ndjson_lines_and_runs_maintenance_after_drain() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOllamaUpstream::with_response(OllamaUpstreamResponse::ndjson(
        200,
        vec![
            "{\"message\":{\"role\":\"assistant\",\"content\":\"hel\"},\"done\":false}\n"
                .to_string(),
            "{\"message\":{\"role\":\"assistant\",\"content\":\"lo\",\"tool_calls\":[{\"function\":{\"name\":\"lookup\",\"arguments\":{\"query\":\"secret\"}}}]},\"done\":false}\n"
                .to_string(),
            "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n".to_string(),
        ],
    ));
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    let mut response = handle_ollama_request_with_services(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/chat",
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
    assert_eq!(
        response.body.ndjson_lines(),
        &[
            "{\"message\":{\"role\":\"assistant\",\"content\":\"hel\"},\"done\":false}\n"
                .to_string(),
            "{\"message\":{\"role\":\"assistant\",\"content\":\"lo\",\"tool_calls\":[{\"function\":{\"name\":\"lookup\",\"arguments\":{\"query\":\"secret\"}}}]},\"done\":false}\n"
                .to_string(),
            "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n".to_string()
        ]
    );
    match &mut response.body {
        OllamaGatewayBody::Ndjson(body) => while body.next_chunk().expect("ndjson").is_some() {},
        OllamaGatewayBody::Json(_) => panic!("expected ndjson body"),
    }
    response.finish_deferred_maintenance(&mut services);
    assert!(response.audit.stages.iter().any(|stage| {
        stage.stage == GatewayAuditStage::Maintenance
            && stage.outcome == GatewayAuditOutcome::Succeeded
    }));
}

#[test]
fn chat_non_streaming_without_done_true_skips_maintenance() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOllamaUpstream::with_response(OllamaUpstreamResponse::json(
        200,
        json!({
            "model": "qwen2.5:7b",
            "message": { "role": "assistant", "content": "partial" },
            "done": false
        }),
    ));
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    let response = handle_ollama_request_with_services(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/chat",
            scope_request(),
            json!({
                "model": "local",
                "stream": false,
                "messages": [{ "role": "user", "content": "do not maintain partial" }]
            }),
        ),
        &mut upstream,
        &mut services,
    )
    .expect("chat response");

    assert_eq!(response.body.json()["message"]["content"], "partial");
    assert!(response.audit.stages.iter().any(|stage| {
        stage.stage == GatewayAuditStage::Maintenance
            && stage.outcome == GatewayAuditOutcome::Skipped
    }));
    assert!(response.audit.stages.iter().all(|stage| {
        stage.stage != GatewayAuditStage::Maintenance
            || stage.outcome != GatewayAuditOutcome::Succeeded
    }));
}

#[test]
fn embed_embeddings_and_show_are_passthrough_without_projection() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOllamaUpstream::default();

    let embed = handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/embed",
            scope_request(),
            json!({ "model": "embeddinggemma", "input": ["a", "b"], "truncate": false }),
        ),
        &mut upstream,
    )
    .expect("embed response");
    let embeddings = handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/embeddings",
            scope_request(),
            json!({ "model": "legacy", "prompt": "a" }),
        ),
        &mut upstream,
    )
    .expect("embeddings response");
    let show = handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::post_json(
            "/api/show",
            scope_request(),
            json!({ "model": "qwen2.5:7b", "verbose": true }),
        ),
        &mut upstream,
    )
    .expect("show response");

    assert_eq!(upstream.embed_calls[0]["input"][0], "a");
    assert_eq!(upstream.embed_calls[0]["truncate"], false);
    assert_eq!(upstream.embeddings_calls[0]["prompt"], "a");
    assert_eq!(upstream.show_calls[0]["verbose"], true);
    assert_eq!(embed.body.json()["embeddings"][0][0], 0.1);
    assert_eq!(embeddings.body.json()["embedding"][1], 0.2);
    assert_eq!(show.body.json()["capabilities"][0], "completion");
    for audit in [&embed.audit, &embeddings.audit, &show.audit] {
        assert!(audit
            .stages
            .iter()
            .all(|stage| stage.stage != GatewayAuditStage::Projection
                && stage.stage != GatewayAuditStage::Maintenance));
    }
}

#[test]
fn ollama_handler_rejects_openai_provider_to_prevent_protocol_crossing() {
    let config = GatewayConfig::default_for_local_dev();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let mut upstream = MockOllamaUpstream::default();

    let error = handle_ollama_request(
        &gateway,
        OllamaGatewayRequest::get("/api/tags", scope_request()),
        &mut upstream,
    )
    .expect_err("provider kind must be enforced");

    assert_eq!(
        error.key(),
        bm_llm_gateway::GatewayErrorKey::ProviderUnavailable
    );
    assert_eq!(upstream.tags_calls, 0);
}

#[test]
fn llm_gateway_http_dispatch_writes_ollama_ndjson_without_sse_wrapping() {
    let config = gateway_config();
    let gateway = GatewayRuntime::open(config.clone()).expect("gateway");
    let body = json!({
        "model": "local",
        "stream": true,
        "messages": [{ "role": "user", "content": "stream this" }]
    })
    .to_string();
    let request = format!(
        "POST /api/chat HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-BM-Conversation-ID: thread-ollama\r\n\r\n{}",
        body.len(),
        body
    );
    let (mut stream, client) = support::accepted_request(request);
    let mut openai = MockOpenAiUpstream;
    let mut ollama = MockOllamaUpstream::with_response(OllamaUpstreamResponse::ndjson(
        200,
        vec![
            "{\"message\":{\"role\":\"assistant\",\"content\":\"hel\"},\"done\":false}\n"
                .to_string(),
            "{\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"done\":false}\n"
                .to_string(),
            "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n".to_string(),
        ],
    ));
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient;
    let mut services = OpenAiGatewayServices::new().with_maintenance(&mut http, &llm);

    serve_llm_gateway_http_accepted_stream_with_services(
        &gateway,
        &mut openai,
        &mut ollama,
        &mut services,
        &mut stream,
    )
    .expect("serve ollama request");

    drop(stream);
    let response = support::finish_request(client);
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("content-type: application/x-ndjson\r\n"));
    assert!(!response.contains("text/event-stream"));
    assert!(!response.contains("content-length:"));
    assert!(response
        .contains("{\"message\":{\"role\":\"assistant\",\"content\":\"hel\"},\"done\":false}\n"));
    assert!(response
        .contains("{\"message\":{\"role\":\"assistant\",\"content\":\"lo\"},\"done\":false}\n"));
    assert!(response
        .contains("{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n"));
}

struct CapturingHttpClient {
    url: Option<String>,
    body: Option<Vec<u8>>,
}

impl CapturingHttpClient {
    fn new() -> Self {
        Self {
            url: None,
            body: None,
        }
    }

    fn body_json(&self) -> Value {
        serde_json::from_slice(self.body.as_deref().expect("captured body")).expect("captured json")
    }
}

impl LlmHttpClient for CapturingHttpClient {
    fn do_post(
        &mut self,
        url: &str,
        _headers: &[(&str, &str)],
        body: &[u8],
    ) -> bm_sdk::Result<(u16, ResponseBody)> {
        self.url = Some(url.to_string());
        self.body = Some(body.to_vec());
        Ok((
            200,
            ResponseBody::Heap(
                json!({
                    "message": { "role": "assistant", "content": "maintenance summary" },
                    "done": true,
                    "done_reason": "stop"
                })
                .to_string()
                .into_bytes(),
            ),
        ))
    }
}

#[test]
fn ollama_maintenance_llm_client_uses_native_chat_endpoint() {
    let provider = GatewayProviderConfig::ollama_native("http://127.0.0.1:11434/api");
    let llm = OllamaMaintenanceLlmClient::new(provider, "qwen2.5:7b");
    let mut http = CapturingHttpClient::new();

    let response = llm
        .chat(
            &mut http,
            "Summarize safely.",
            &[Message {
                role: Cow::Borrowed("user"),
                content: "remember ollama boundary".to_string(),
            }],
            None,
            ToolChoicePolicy::Auto,
        )
        .expect("ollama maintenance response");

    assert_eq!(http.url.as_deref(), Some("http://127.0.0.1:11434/api/chat"));
    let body = http.body_json();
    assert_eq!(body["model"], "qwen2.5:7b");
    assert_eq!(body["stream"], false);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(response.content, "maintenance summary");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
}

trait OllamaGatewayBodyAssertions {
    fn json(&self) -> &Value;
    fn ndjson_lines(&self) -> &[String];
}

impl OllamaGatewayBodyAssertions for OllamaGatewayBody {
    fn json(&self) -> &Value {
        match self {
            OllamaGatewayBody::Json(value) => value,
            OllamaGatewayBody::Ndjson(_) => panic!("expected json body"),
        }
    }

    fn ndjson_lines(&self) -> &[String] {
        self.buffered_ndjson_chunks()
            .expect("expected buffered ndjson body")
    }
}
