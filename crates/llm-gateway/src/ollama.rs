use std::collections::BTreeMap;

use bm_sdk::{
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, MemoryProjectionRequest, Message,
    RuntimeLifecycleModeInput, StopReason, ToolChoicePolicy, ToolSpec,
};
use serde_json::{json, Map, Value};

use crate::maintenance::{
    run_text_maintenance, BoundedText, GatewayMaintenancePlan, GatewayMaintenancePlanInput,
};
use crate::provider::select_provider_for_kind;
use crate::{
    GatewayAuditOutcome, GatewayAuditReport, GatewayAuditStage, GatewayConfig, GatewayError,
    GatewayMaintenanceConfig, GatewayProviderConfig, GatewayProviderKind, GatewayRuntime,
    GatewayScopeRequest, GatewayScopeResolver, OpenAiGatewayServices, Result,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OllamaGatewayMethod {
    Get,
    Post,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OllamaGatewayRequest {
    pub method: OllamaGatewayMethod,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Value>,
    pub scope: GatewayScopeRequest,
    pub provider_name: Option<String>,
    pub client_profile: String,
}

impl OllamaGatewayRequest {
    pub fn get(path: impl Into<String>, scope: GatewayScopeRequest) -> Self {
        Self {
            method: OllamaGatewayMethod::Get,
            path: path.into(),
            headers: BTreeMap::new(),
            body: None,
            scope,
            provider_name: None,
            client_profile: "ollama_native".to_string(),
        }
    }

    pub fn post_json(path: impl Into<String>, scope: GatewayScopeRequest, body: Value) -> Self {
        Self {
            method: OllamaGatewayMethod::Post,
            path: path.into(),
            headers: BTreeMap::new(),
            body: Some(body),
            scope,
            provider_name: None,
            client_profile: "ollama_native".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct OllamaGatewayResponse {
    pub status_code: u16,
    pub body: OllamaGatewayBody,
    pub audit: GatewayAuditReport,
}

impl OllamaGatewayResponse {
    pub fn finish_deferred_maintenance(&mut self, services: &mut OpenAiGatewayServices<'_>) {
        if let Some(outcome) = self.body.finish_deferred_maintenance(services) {
            self.audit
                .record_stage(GatewayAuditStage::Maintenance, outcome);
        }
    }

    fn prepare_post_reply_maintenance(
        &mut self,
        plan: GatewayMaintenancePlan,
        endpoint: OllamaCompletionEndpoint,
        services: &mut OpenAiGatewayServices<'_>,
    ) {
        match &mut self.body {
            OllamaGatewayBody::Json(body) => {
                let outcome = run_ollama_json_maintenance(plan, endpoint, body, services);
                self.audit
                    .record_stage(GatewayAuditStage::Maintenance, outcome.into());
            }
            OllamaGatewayBody::Ndjson(body) => {
                let placeholder = OllamaNdjsonBody::buffered(Vec::new());
                let owned = std::mem::replace(body, placeholder);
                *body = owned.with_deferred_maintenance(plan, endpoint);
            }
        }
    }
}

pub enum OllamaGatewayBody {
    Json(Value),
    Ndjson(OllamaNdjsonBody),
}

impl std::fmt::Debug for OllamaGatewayBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(value) => f.debug_tuple("Json").field(value).finish(),
            Self::Ndjson(_) => f.write_str("Ndjson(<stream>)"),
        }
    }
}

impl OllamaGatewayBody {
    pub const fn is_ndjson(&self) -> bool {
        matches!(self, Self::Ndjson(_))
    }

    pub fn buffered_ndjson_chunks(&self) -> Option<&[String]> {
        match self {
            Self::Json(_) => None,
            Self::Ndjson(body) => body.buffered_chunks(),
        }
    }

    pub fn into_json(self) -> Option<Value> {
        match self {
            Self::Json(value) => Some(value),
            Self::Ndjson(_) => None,
        }
    }

    fn finish_deferred_maintenance(
        &mut self,
        services: &mut OpenAiGatewayServices<'_>,
    ) -> Option<GatewayAuditOutcome> {
        match self {
            Self::Json(_) => None,
            Self::Ndjson(body) => body
                .finish_deferred_maintenance(services)
                .map(GatewayAuditOutcome::from),
        }
    }
}

pub trait OllamaNdjsonStream: Send {
    fn next_chunk(&mut self) -> Result<Option<String>>;
}

pub struct OllamaNdjsonBody {
    source: OllamaNdjsonSource,
    deferred_maintenance: Option<Box<OllamaDeferredMaintenance>>,
}

enum OllamaNdjsonSource {
    Buffered { chunks: Vec<String>, offset: usize },
    Streaming(Box<dyn OllamaNdjsonStream>),
}

impl OllamaNdjsonBody {
    pub fn buffered(chunks: Vec<String>) -> Self {
        Self {
            source: OllamaNdjsonSource::Buffered { chunks, offset: 0 },
            deferred_maintenance: None,
        }
    }

    pub fn streaming(stream: Box<dyn OllamaNdjsonStream>) -> Self {
        Self {
            source: OllamaNdjsonSource::Streaming(stream),
            deferred_maintenance: None,
        }
    }

    fn with_deferred_maintenance(
        mut self,
        plan: GatewayMaintenancePlan,
        endpoint: OllamaCompletionEndpoint,
    ) -> Self {
        self.deferred_maintenance = Some(Box::new(OllamaDeferredMaintenance::new(plan, endpoint)));
        self
    }

    pub fn buffered_chunks(&self) -> Option<&[String]> {
        match &self.source {
            OllamaNdjsonSource::Buffered { chunks, offset } => Some(&chunks[*offset..]),
            OllamaNdjsonSource::Streaming(_) => None,
        }
    }

    pub fn next_chunk(&mut self) -> Result<Option<String>> {
        let chunk = match &mut self.source {
            OllamaNdjsonSource::Buffered { chunks, offset } => {
                if let Some(chunk) = chunks.get(*offset).cloned() {
                    *offset += 1;
                    Some(chunk)
                } else {
                    None
                }
            }
            OllamaNdjsonSource::Streaming(stream) => stream.next_chunk()?,
        };
        if let Some(chunk) = chunk.as_deref() {
            if let Some(maintenance) = &mut self.deferred_maintenance {
                maintenance.observe_ndjson_chunk(chunk);
            }
        }
        Ok(chunk)
    }

    pub fn collect_chunks(mut self) -> Result<Vec<String>> {
        let mut chunks = Vec::new();
        while let Some(chunk) = self.next_chunk()? {
            chunks.push(chunk);
        }
        Ok(chunks)
    }

    fn finish_deferred_maintenance(
        &mut self,
        services: &mut OpenAiGatewayServices<'_>,
    ) -> Option<crate::maintenance::GatewayMaintenanceRunOutcome> {
        self.deferred_maintenance
            .take()
            .map(|maintenance| maintenance.finish(services))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OllamaUpstreamRequest {
    pub endpoint: String,
    pub body: Value,
    pub stream: bool,
    pub model: String,
    pub extracted_user_text: String,
}

pub enum OllamaUpstreamResponse {
    Json {
        status_code: u16,
        body: Value,
    },
    Ndjson {
        status_code: u16,
        body: OllamaNdjsonBody,
    },
}

impl OllamaUpstreamResponse {
    pub fn json(status_code: u16, body: Value) -> Self {
        Self::Json { status_code, body }
    }

    pub fn ndjson(status_code: u16, chunks: Vec<String>) -> Self {
        Self::Ndjson {
            status_code,
            body: OllamaNdjsonBody::buffered(chunks),
        }
    }
}

pub trait OllamaNativeUpstream {
    fn tags(&mut self, provider: &GatewayProviderConfig) -> Result<OllamaUpstreamResponse>;

    fn version(&mut self, provider: &GatewayProviderConfig) -> Result<OllamaUpstreamResponse>;

    fn chat(
        &mut self,
        provider: &GatewayProviderConfig,
        request: OllamaUpstreamRequest,
    ) -> Result<OllamaUpstreamResponse>;

    fn generate(
        &mut self,
        provider: &GatewayProviderConfig,
        request: OllamaUpstreamRequest,
    ) -> Result<OllamaUpstreamResponse>;

    fn embed(
        &mut self,
        provider: &GatewayProviderConfig,
        body: Value,
    ) -> Result<OllamaUpstreamResponse>;

    fn embeddings(
        &mut self,
        provider: &GatewayProviderConfig,
        body: Value,
    ) -> Result<OllamaUpstreamResponse>;

    fn show(
        &mut self,
        provider: &GatewayProviderConfig,
        body: Value,
    ) -> Result<OllamaUpstreamResponse>;
}

pub fn handle_ollama_request(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    request: OllamaGatewayRequest,
    upstream: &mut dyn OllamaNativeUpstream,
) -> Result<OllamaGatewayResponse> {
    let mut services = OpenAiGatewayServices::new();
    handle_ollama_request_with_services(gateway, config, request, upstream, &mut services)
}

pub fn handle_ollama_request_with_services(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    request: OllamaGatewayRequest,
    upstream: &mut dyn OllamaNativeUpstream,
    services: &mut OpenAiGatewayServices<'_>,
) -> Result<OllamaGatewayResponse> {
    let provider = select_provider_for_kind(
        config,
        request.provider_name.as_deref(),
        GatewayProviderKind::OllamaNative,
        "ollama-native",
    )?;

    match (request.method, request.path.as_str()) {
        (OllamaGatewayMethod::Get, "/api/tags") => handle_tags(config, request, provider, upstream),
        (OllamaGatewayMethod::Get, "/api/version") => {
            handle_version(config, request, provider, upstream)
        }
        (OllamaGatewayMethod::Post, "/api/chat") => {
            handle_chat(gateway, config, request, provider, upstream, services)
        }
        (OllamaGatewayMethod::Post, "/api/generate") => {
            handle_generate(gateway, config, request, provider, upstream, services)
        }
        (OllamaGatewayMethod::Post, "/api/embed") => {
            handle_embed(config, request, provider, upstream)
        }
        (OllamaGatewayMethod::Post, "/api/embeddings") => {
            handle_embeddings(config, request, provider, upstream)
        }
        (OllamaGatewayMethod::Post, "/api/show") => {
            handle_show(config, request, provider, upstream)
        }
        _ => Err(GatewayError::invalid_request(
            "unsupported Ollama gateway route",
        )),
    }
}

fn handle_tags(
    config: &GatewayConfig,
    request: OllamaGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OllamaNativeUpstream,
) -> Result<OllamaGatewayResponse> {
    let mut audit = audit_for_passthrough(config, &request, "ollama-tags", "none")?;
    let response = upstream.tags(provider).map_err(|error| {
        audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
        GatewayError::upstream_unavailable(error.to_string())
    })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    Ok(upstream_response_to_gateway(response, audit))
}

fn handle_version(
    config: &GatewayConfig,
    request: OllamaGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OllamaNativeUpstream,
) -> Result<OllamaGatewayResponse> {
    let mut audit = audit_for_passthrough(config, &request, "ollama-version", "none")?;
    let response = upstream.version(provider).map_err(|error| {
        audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
        GatewayError::upstream_unavailable(error.to_string())
    })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    Ok(upstream_response_to_gateway(response, audit))
}

fn handle_embed(
    config: &GatewayConfig,
    mut request: OllamaGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OllamaNativeUpstream,
) -> Result<OllamaGatewayResponse> {
    let body = required_body(&mut request, "embed body is required")?;
    let mut audit = audit_for_passthrough(config, &request, "ollama-embed", model_alias(&body))?;
    let response = upstream.embed(provider, body).map_err(|error| {
        audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
        GatewayError::upstream_unavailable(error.to_string())
    })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    Ok(upstream_response_to_gateway(response, audit))
}

fn handle_embeddings(
    config: &GatewayConfig,
    mut request: OllamaGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OllamaNativeUpstream,
) -> Result<OllamaGatewayResponse> {
    let body = required_body(&mut request, "embeddings body is required")?;
    let mut audit =
        audit_for_passthrough(config, &request, "ollama-embeddings", model_alias(&body))?;
    let response = upstream.embeddings(provider, body).map_err(|error| {
        audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
        GatewayError::upstream_unavailable(error.to_string())
    })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    Ok(upstream_response_to_gateway(response, audit))
}

fn handle_show(
    config: &GatewayConfig,
    mut request: OllamaGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OllamaNativeUpstream,
) -> Result<OllamaGatewayResponse> {
    let body = required_body(&mut request, "show body is required")?;
    let mut audit = audit_for_passthrough(config, &request, "ollama-show", model_alias(&body))?;
    let response = upstream.show(provider, body).map_err(|error| {
        audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
        GatewayError::upstream_unavailable(error.to_string())
    })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    Ok(upstream_response_to_gateway(response, audit))
}

fn handle_chat(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    mut request: OllamaGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OllamaNativeUpstream,
    services: &mut OpenAiGatewayServices<'_>,
) -> Result<OllamaGatewayResponse> {
    let body = required_body(&mut request, "chat body is required")?;
    let body_object = body
        .as_object()
        .ok_or_else(|| GatewayError::invalid_request("chat body must be an object"))?;
    let model_alias = body_object
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| GatewayError::invalid_request("chat model is required"))?;
    if request.scope.model_alias.is_none() {
        request.scope.model_alias = Some(model_alias.to_string());
    }

    let scope = GatewayScopeResolver::new(config.scope.clone()).resolve(&request.scope)?;
    let mut audit = GatewayAuditReport::new(
        "ollama-chat",
        "/api/chat",
        request.client_profile,
        model_alias,
        scope.clone(),
    );
    let runtime = gateway.runtime_for_scope(scope.entry_scope)?;
    let extracted_user_text = extract_chat_messages_text(body_object.get("messages"))?;
    let external_content_used = chat_uses_external_content(body_object.get("messages"))
        || body_object.get("tools").is_some();
    let projection = runtime
        .runtime()
        .project(MemoryProjectionRequest {
            user_query: extracted_user_text.clone(),
            system_max_len: config.projection.system_max_len,
            recent_messages_limit: config.projection.recent_messages_limit,
            pressure: config.projection.pressure,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .map_err(|error| {
            audit.record_stage(GatewayAuditStage::Projection, GatewayAuditOutcome::Failed);
            GatewayError::projection_failed(error.to_string())
        })?;
    audit.record_stage(
        GatewayAuditStage::Projection,
        GatewayAuditOutcome::Succeeded,
    );

    let stream = body_object
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let model = provider_model_name(provider, model_alias);
    let upstream_body = build_upstream_chat_body(&body, &projection.system_memory_block, &model)?;
    let carry = projection.context.into_runtime_carry();
    let maintenance_plan = GatewayMaintenancePlan::new(GatewayMaintenancePlanInput {
        runtime,
        user_content: extracted_user_text.clone(),
        external_content_used,
        runtime_skill_selected_ids: carry.runtime_skill_selected_ids,
        task_learning_selected_ids: carry.task_recall_selected_ids,
        pressure: config.projection.pressure,
        mode_input: RuntimeLifecycleModeInput::default(),
        config: config.maintenance,
    });
    let upstream_request = OllamaUpstreamRequest {
        endpoint: "/chat".to_string(),
        body: upstream_body,
        stream,
        model,
        extracted_user_text,
    };
    let response = upstream.chat(provider, upstream_request).map_err(|error| {
        audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
        GatewayError::upstream_unavailable(error.to_string())
    })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    let mut response = upstream_response_to_gateway(response, audit);
    response.prepare_post_reply_maintenance(
        maintenance_plan,
        OllamaCompletionEndpoint::Chat,
        services,
    );
    Ok(response)
}

fn handle_generate(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    mut request: OllamaGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OllamaNativeUpstream,
    services: &mut OpenAiGatewayServices<'_>,
) -> Result<OllamaGatewayResponse> {
    let body = required_body(&mut request, "generate body is required")?;
    let body_object = body
        .as_object()
        .ok_or_else(|| GatewayError::invalid_request("generate body must be an object"))?;
    let model_alias = body_object
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| GatewayError::invalid_request("generate model is required"))?;
    if request.scope.model_alias.is_none() {
        request.scope.model_alias = Some(model_alias.to_string());
    }

    let scope = GatewayScopeResolver::new(config.scope.clone()).resolve(&request.scope)?;
    let mut audit = GatewayAuditReport::new(
        "ollama-generate",
        "/api/generate",
        request.client_profile,
        model_alias,
        scope.clone(),
    );
    let runtime = gateway.runtime_for_scope(scope.entry_scope)?;
    let extracted_user_text = body_object
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let external_content_used = body_object
        .get("images")
        .and_then(Value::as_array)
        .map(|images| !images.is_empty())
        .unwrap_or(false);
    let projection = runtime
        .runtime()
        .project(MemoryProjectionRequest {
            user_query: extracted_user_text.clone(),
            system_max_len: config.projection.system_max_len,
            recent_messages_limit: config.projection.recent_messages_limit,
            pressure: config.projection.pressure,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .map_err(|error| {
            audit.record_stage(GatewayAuditStage::Projection, GatewayAuditOutcome::Failed);
            GatewayError::projection_failed(error.to_string())
        })?;
    audit.record_stage(
        GatewayAuditStage::Projection,
        GatewayAuditOutcome::Succeeded,
    );

    let stream = body_object
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let model = provider_model_name(provider, model_alias);
    let (upstream_body, used_prompt_prefix_fallback) =
        build_upstream_generate_body(&body, &projection.system_memory_block, &model, provider)?;
    if used_prompt_prefix_fallback {
        audit.record_note("ollama_generate_prompt_prefix_fallback");
    }
    let carry = projection.context.into_runtime_carry();
    let maintenance_plan = GatewayMaintenancePlan::new(GatewayMaintenancePlanInput {
        runtime,
        user_content: extracted_user_text.clone(),
        external_content_used,
        runtime_skill_selected_ids: carry.runtime_skill_selected_ids,
        task_learning_selected_ids: carry.task_recall_selected_ids,
        pressure: config.projection.pressure,
        mode_input: RuntimeLifecycleModeInput::default(),
        config: config.maintenance,
    });
    let upstream_request = OllamaUpstreamRequest {
        endpoint: "/generate".to_string(),
        body: upstream_body,
        stream,
        model,
        extracted_user_text,
    };
    let response = upstream
        .generate(provider, upstream_request)
        .map_err(|error| {
            audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
            GatewayError::upstream_unavailable(error.to_string())
        })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    let mut response = upstream_response_to_gateway(response, audit);
    response.prepare_post_reply_maintenance(
        maintenance_plan,
        OllamaCompletionEndpoint::Generate,
        services,
    );
    Ok(response)
}

fn required_body(request: &mut OllamaGatewayRequest, message: &str) -> Result<Value> {
    request
        .body
        .take()
        .ok_or_else(|| GatewayError::invalid_request(message))
}

fn audit_for_passthrough(
    config: &GatewayConfig,
    request: &OllamaGatewayRequest,
    audit_id: &str,
    model_alias: &str,
) -> Result<GatewayAuditReport> {
    let scope = GatewayScopeResolver::new(config.scope.clone()).resolve(&request.scope)?;
    Ok(GatewayAuditReport::new(
        audit_id,
        request.path.clone(),
        request.client_profile.clone(),
        model_alias,
        scope,
    ))
}

fn model_alias(body: &Value) -> &str {
    body.get("model").and_then(Value::as_str).unwrap_or("none")
}

fn upstream_response_to_gateway(
    response: OllamaUpstreamResponse,
    audit: GatewayAuditReport,
) -> OllamaGatewayResponse {
    match response {
        OllamaUpstreamResponse::Json { status_code, body } => OllamaGatewayResponse {
            status_code,
            body: OllamaGatewayBody::Json(body),
            audit,
        },
        OllamaUpstreamResponse::Ndjson { status_code, body } => OllamaGatewayResponse {
            status_code,
            body: OllamaGatewayBody::Ndjson(body),
            audit,
        },
    }
}

fn provider_model_name(provider: &GatewayProviderConfig, model_alias: &str) -> String {
    provider
        .model_aliases
        .iter()
        .find_map(|(alias, model)| (alias == model_alias).then(|| model.clone()))
        .unwrap_or_else(|| model_alias.to_string())
}

fn build_upstream_chat_body(
    original_body: &Value,
    memory_block: &str,
    model: &str,
) -> Result<Value> {
    let mut object = original_body
        .as_object()
        .cloned()
        .ok_or_else(|| GatewayError::invalid_request("chat body must be an object"))?;
    object.insert("model".to_string(), Value::String(model.to_string()));
    let mut messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| GatewayError::invalid_request("chat messages must be an array"))?
        .clone();
    if !memory_block.trim().is_empty() {
        inject_memory_into_messages(&mut messages, memory_block);
        object.insert("messages".to_string(), Value::Array(messages));
    }
    Ok(Value::Object(object))
}

fn inject_memory_into_messages(messages: &mut Vec<Value>, memory_block: &str) {
    let memory_text = format!("Beetle Memory context:\n{memory_block}");
    for message in messages.iter_mut() {
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        if object.get("role").and_then(Value::as_str) != Some("system") {
            continue;
        }
        let Some(content) = object.get("content").and_then(Value::as_str) else {
            break;
        };
        let content = if content.trim().is_empty() {
            memory_text
        } else {
            format!("{content}\n\n{memory_text}")
        };
        object.insert("content".to_string(), Value::String(content));
        return;
    }

    let mut memory_message = Map::new();
    memory_message.insert("role".to_string(), Value::String("system".to_string()));
    memory_message.insert("content".to_string(), Value::String(memory_text));
    messages.insert(0, Value::Object(memory_message));
}

fn build_upstream_generate_body(
    original_body: &Value,
    memory_block: &str,
    model: &str,
    provider: &GatewayProviderConfig,
) -> Result<(Value, bool)> {
    let mut object = original_body
        .as_object()
        .cloned()
        .ok_or_else(|| GatewayError::invalid_request("generate body must be an object"))?;
    object.insert("model".to_string(), Value::String(model.to_string()));
    if memory_block.trim().is_empty() {
        return Ok((Value::Object(object), false));
    }

    let memory_text = format!("Beetle Memory context:\n{memory_block}");
    if provider.ollama_generate_system_supported {
        let system = object
            .get("system")
            .and_then(Value::as_str)
            .map(|existing| {
                if existing.trim().is_empty() {
                    memory_text.clone()
                } else {
                    format!("{existing}\n\n{memory_text}")
                }
            })
            .unwrap_or_else(|| memory_text.clone());
        object.insert("system".to_string(), Value::String(system));
        return Ok((Value::Object(object), false));
    }

    let existing_system = object
        .remove("system")
        .and_then(|value| match value {
            Value::String(text) => Some(text),
            _ => None,
        })
        .unwrap_or_default();
    let prompt = object
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut prompt_parts = Vec::new();
    if !existing_system.trim().is_empty() {
        prompt_parts.push(existing_system.trim().to_string());
    }
    prompt_parts.push(memory_text);
    if !prompt.trim().is_empty() {
        prompt_parts.push(prompt.trim().to_string());
    }
    object.insert(
        "prompt".to_string(),
        Value::String(prompt_parts.join("\n\n")),
    );
    Ok((Value::Object(object), true))
}

fn extract_chat_messages_text(messages: Option<&Value>) -> Result<String> {
    let messages = messages
        .and_then(Value::as_array)
        .ok_or_else(|| GatewayError::invalid_request("chat messages must be an array"))?;
    let mut parts = Vec::new();
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        if let Some(content) = message.get("content").and_then(Value::as_str) {
            if !content.trim().is_empty() {
                parts.push(content.trim().to_string());
            }
        }
    }
    Ok(parts.join("\n").trim().to_string())
}

fn chat_uses_external_content(messages: Option<&Value>) -> bool {
    let Some(messages) = messages.and_then(Value::as_array) else {
        return false;
    };
    messages.iter().any(|message| {
        matches!(message.get("role").and_then(Value::as_str), Some("tool"))
            || message
                .get("images")
                .and_then(Value::as_array)
                .map(|images| !images.is_empty())
                .unwrap_or(false)
    })
}

#[derive(Clone, Copy)]
enum OllamaCompletionEndpoint {
    Chat,
    Generate,
}

struct OllamaDeferredMaintenance {
    plan: GatewayMaintenancePlan,
    endpoint: OllamaCompletionEndpoint,
    accumulator: OllamaReplyAccumulator,
}

impl OllamaDeferredMaintenance {
    fn new(plan: GatewayMaintenancePlan, endpoint: OllamaCompletionEndpoint) -> Self {
        let config = plan.config();
        Self {
            plan,
            endpoint,
            accumulator: OllamaReplyAccumulator::new(config),
        }
    }

    fn observe_ndjson_chunk(&mut self, chunk: &str) {
        self.accumulator.observe_ndjson_chunk(chunk, self.endpoint);
    }

    fn finish(
        self,
        services: &mut OpenAiGatewayServices<'_>,
    ) -> crate::maintenance::GatewayMaintenanceRunOutcome {
        let (reply_content, tool_calls, reuse_outcome_note, saw_done) =
            self.accumulator.into_parts(self.endpoint);
        if !saw_done {
            return crate::maintenance::GatewayMaintenanceRunOutcome::Skipped;
        }
        run_text_maintenance(
            self.plan,
            reply_content,
            tool_calls,
            reuse_outcome_note,
            services,
        )
    }
}

fn run_ollama_json_maintenance(
    plan: GatewayMaintenancePlan,
    endpoint: OllamaCompletionEndpoint,
    body: &Value,
    services: &mut OpenAiGatewayServices<'_>,
) -> crate::maintenance::GatewayMaintenanceRunOutcome {
    let mut accumulator = OllamaReplyAccumulator::new(plan.config());
    accumulator.observe_json_response(body, endpoint);
    let (reply_content, tool_calls, reuse_outcome_note, saw_done) =
        accumulator.into_parts(endpoint);
    if !saw_done {
        return crate::maintenance::GatewayMaintenanceRunOutcome::Skipped;
    }
    run_text_maintenance(
        plan,
        reply_content,
        tool_calls,
        reuse_outcome_note,
        services,
    )
}

struct OllamaReplyAccumulator {
    reply: BoundedText,
    tool_calls: Vec<OllamaToolCallSummary>,
    ndjson_buffer: String,
    saw_done: bool,
}

impl OllamaReplyAccumulator {
    fn new(config: GatewayMaintenanceConfig) -> Self {
        Self {
            reply: BoundedText::new(config.reply_max_chars, config.reply_max_bytes),
            tool_calls: Vec::new(),
            ndjson_buffer: String::new(),
            saw_done: false,
        }
    }

    fn observe_json_response(&mut self, body: &Value, endpoint: OllamaCompletionEndpoint) {
        if body.get("done").and_then(Value::as_bool).unwrap_or(false) {
            self.saw_done = true;
        }
        match endpoint {
            OllamaCompletionEndpoint::Chat => {
                if let Some(message) = body.get("message") {
                    self.observe_chat_message(message);
                }
            }
            OllamaCompletionEndpoint::Generate => {
                if let Some(response) = body.get("response").and_then(Value::as_str) {
                    self.reply.push_str(response);
                }
            }
        }
    }

    fn observe_ndjson_chunk(&mut self, chunk: &str, endpoint: OllamaCompletionEndpoint) {
        self.ndjson_buffer.push_str(chunk);
        while let Some(line_end) = self.ndjson_buffer.find('\n') {
            let line = self.ndjson_buffer[..line_end]
                .trim_end_matches('\r')
                .to_string();
            self.ndjson_buffer.drain(..=line_end);
            self.observe_ndjson_line(&line, endpoint);
        }
    }

    fn observe_ndjson_line(&mut self, line: &str, endpoint: OllamaCompletionEndpoint) {
        if line.trim().is_empty() {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        self.observe_json_response(&value, endpoint);
    }

    fn observe_chat_message(&mut self, message: &Value) {
        if let Some(content) = message.get("content").and_then(Value::as_str) {
            self.reply.push_str(content);
        }
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                self.observe_tool_call(tool_call);
            }
        }
    }

    fn observe_tool_call(&mut self, tool_call: &Value) {
        let function = tool_call.get("function").and_then(Value::as_object);
        let name = function
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let arguments_bytes = function
            .and_then(|function| function.get("arguments"))
            .and_then(|arguments| serde_json::to_string(arguments).ok())
            .map(|arguments| arguments.len())
            .unwrap_or(0);
        self.tool_calls.push(OllamaToolCallSummary {
            name,
            arguments_bytes,
        });
    }

    fn into_parts(mut self, endpoint: OllamaCompletionEndpoint) -> (String, u32, String, bool) {
        if !self.ndjson_buffer.trim().is_empty() {
            let line = std::mem::take(&mut self.ndjson_buffer);
            self.observe_ndjson_line(&line, endpoint);
        }
        let tool_calls = self.tool_calls.len() as u32;
        let reuse_outcome_note = if tool_calls == 0 {
            String::new()
        } else {
            format!(
                "ollama_tool_calls={tool_calls}; tool_summaries={}",
                self.tool_call_summary()
            )
        };
        (
            self.reply.into_string(),
            tool_calls,
            reuse_outcome_note,
            self.saw_done,
        )
    }

    fn tool_call_summary(&self) -> String {
        self.tool_calls
            .iter()
            .enumerate()
            .map(|(index, summary)| {
                format!(
                    "tool={index}:name={}:arguments_bytes={}",
                    summary.name, summary.arguments_bytes
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

struct OllamaToolCallSummary {
    name: String,
    arguments_bytes: usize,
}

pub struct OllamaMaintenanceLlmClient {
    provider: GatewayProviderConfig,
    model: String,
}

impl OllamaMaintenanceLlmClient {
    pub fn new(provider: GatewayProviderConfig, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }
}

impl LlmClient for OllamaMaintenanceLlmClient {
    fn model_compat(&self) -> LlmModelCompat {
        LlmModelCompat::default()
    }

    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> bm_sdk::Result<LlmResponse> {
        let mut ollama_messages = Vec::new();
        if !system.trim().is_empty() {
            ollama_messages.push(json!({
                "role": "system",
                "content": system,
            }));
        }
        ollama_messages.extend(messages.iter().map(|message| {
            json!({
                "role": message.role.as_ref(),
                "content": message.content,
            })
        }));
        let body = json!({
            "model": self.model,
            "messages": ollama_messages,
            "stream": false,
        })
        .to_string();
        let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
        let bearer;
        if let Some(env_name) = self.provider.secret_env_name() {
            let token = std::env::var(env_name).map_err(|_| {
                bm_sdk::Error::config("ollama_maintenance_llm", "provider api key env is unset")
            })?;
            bearer = format!("Bearer {token}");
            headers.push(("authorization".to_string(), bearer));
        }
        let header_refs = headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        let (status, response) = http.do_post(
            &format!("{}/chat", self.provider.base_url.trim_end_matches('/')),
            &header_refs,
            body.as_bytes(),
        )?;
        if !(200..300).contains(&status) {
            return Err(bm_sdk::Error::http("ollama_maintenance_llm", status));
        }
        let value: Value = serde_json::from_slice(response.as_ref())
            .map_err(|error| bm_sdk::Error::config("ollama_maintenance_llm", error.to_string()))?;
        let message = value.get("message").ok_or_else(|| {
            bm_sdk::Error::config("ollama_maintenance_llm", "missing message in response")
        })?;
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let has_tool_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|tool_calls| !tool_calls.is_empty())
            .unwrap_or(false);
        let stop_reason = if has_tool_calls {
            StopReason::ToolUse
        } else {
            match value.get("done_reason").and_then(Value::as_str) {
                Some("length") => StopReason::MaxTokens,
                Some("stop") | None => StopReason::EndTurn,
                Some(_) => StopReason::Other,
            }
        };
        Ok(LlmResponse {
            content,
            stop_reason,
            tool_calls: None,
        })
    }
}

#[cfg(feature = "client-reqwest")]
pub struct ReqwestOllamaNativeUpstream {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "client-reqwest")]
impl ReqwestOllamaNativeUpstream {
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        Ok(Self { client })
    }
}

#[cfg(feature = "client-reqwest")]
impl OllamaNativeUpstream for ReqwestOllamaNativeUpstream {
    fn tags(&mut self, provider: &GatewayProviderConfig) -> Result<OllamaUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::GET, "/tags")?
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        ollama_response_to_json(response)
    }

    fn version(&mut self, provider: &GatewayProviderConfig) -> Result<OllamaUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::GET, "/version")?
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        ollama_response_to_json(response)
    }

    fn chat(
        &mut self,
        provider: &GatewayProviderConfig,
        request: OllamaUpstreamRequest,
    ) -> Result<OllamaUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::POST, request.endpoint.as_str())?
            .json(&request.body)
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        if request.stream {
            let status_code = response.status().as_u16();
            return Ok(OllamaUpstreamResponse::Ndjson {
                status_code,
                body: OllamaNdjsonBody::streaming(Box::new(ReqwestLineNdjsonStream::new(response))),
            });
        }
        ollama_response_to_json(response)
    }

    fn generate(
        &mut self,
        provider: &GatewayProviderConfig,
        request: OllamaUpstreamRequest,
    ) -> Result<OllamaUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::POST, request.endpoint.as_str())?
            .json(&request.body)
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        if request.stream {
            let status_code = response.status().as_u16();
            return Ok(OllamaUpstreamResponse::Ndjson {
                status_code,
                body: OllamaNdjsonBody::streaming(Box::new(ReqwestLineNdjsonStream::new(response))),
            });
        }
        ollama_response_to_json(response)
    }

    fn embed(
        &mut self,
        provider: &GatewayProviderConfig,
        body: Value,
    ) -> Result<OllamaUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::POST, "/embed")?
            .json(&body)
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        ollama_response_to_json(response)
    }

    fn embeddings(
        &mut self,
        provider: &GatewayProviderConfig,
        body: Value,
    ) -> Result<OllamaUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::POST, "/embeddings")?
            .json(&body)
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        ollama_response_to_json(response)
    }

    fn show(
        &mut self,
        provider: &GatewayProviderConfig,
        body: Value,
    ) -> Result<OllamaUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::POST, "/show")?
            .json(&body)
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        ollama_response_to_json(response)
    }
}

#[cfg(feature = "client-reqwest")]
impl ReqwestOllamaNativeUpstream {
    fn authorized_request(
        &self,
        provider: &GatewayProviderConfig,
        method: reqwest::Method,
        endpoint: &str,
    ) -> Result<reqwest::blocking::RequestBuilder> {
        let url = format!("{}{}", provider.base_url.trim_end_matches('/'), endpoint);
        let mut builder = self.client.request(method, url);
        if let Some(env_name) = provider.secret_env_name() {
            let token = std::env::var(env_name).map_err(|_| {
                GatewayError::invalid_config(format!("provider api key env is unset: {env_name}"))
            })?;
            builder = builder.bearer_auth(token);
        }
        Ok(builder)
    }
}

#[cfg(feature = "client-reqwest")]
fn ollama_response_to_json(
    response: reqwest::blocking::Response,
) -> Result<OllamaUpstreamResponse> {
    let status_code = response.status().as_u16();
    let body = response
        .json::<Value>()
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
    Ok(OllamaUpstreamResponse::Json { status_code, body })
}

#[cfg(feature = "client-reqwest")]
struct ReqwestLineNdjsonStream {
    reader: std::io::BufReader<reqwest::blocking::Response>,
}

#[cfg(feature = "client-reqwest")]
impl ReqwestLineNdjsonStream {
    fn new(response: reqwest::blocking::Response) -> Self {
        Self {
            reader: std::io::BufReader::new(response),
        }
    }
}

#[cfg(feature = "client-reqwest")]
impl OllamaNdjsonStream for ReqwestLineNdjsonStream {
    fn next_chunk(&mut self) -> Result<Option<String>> {
        use std::io::BufRead;

        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        if read == 0 {
            Ok(None)
        } else {
            Ok(Some(line))
        }
    }
}
