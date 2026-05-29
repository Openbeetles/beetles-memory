use std::collections::BTreeMap;

use bm_sdk::{
    ConversationScope, MemoryProjectionRequest, MemoryTurnProtocol, MemoryTurnSource,
    ProviderModelContextLimit, RuntimeLifecycleModeInput, TranscriptInputMessage,
};
use serde_json::{Map, Value};

use crate::agent_tools::request_scoped_agent_tool_registry;
use crate::projection::render_model_facing_projection;
use crate::provider::select_provider_for_kind;
use crate::{
    maintenance::{
        run_json_maintenance, GatewayInputTranscript, GatewayMaintenancePlan,
        GatewayMaintenancePlanInput, GatewayMaintenanceRunOutcome, OpenAiDeferredMaintenance,
    },
    probe_openai_provider_capabilities, GatewayAuditOutcome, GatewayAuditReport, GatewayAuditStage,
    GatewayConfig, GatewayError, GatewayProviderConfig, GatewayProviderKind, GatewayRuntime,
    GatewayScopeRequest, GatewayScopeResolver, OpenAiGatewayServices, Result,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenAiGatewayMethod {
    Get,
    Post,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenAiGatewayRequest {
    pub method: OpenAiGatewayMethod,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Value>,
    pub scope: GatewayScopeRequest,
    pub provider_name: Option<String>,
    pub client_profile: String,
}

impl OpenAiGatewayRequest {
    pub fn get(path: impl Into<String>, scope: GatewayScopeRequest) -> Self {
        Self {
            method: OpenAiGatewayMethod::Get,
            path: path.into(),
            headers: BTreeMap::new(),
            body: None,
            scope,
            provider_name: None,
            client_profile: "openai_compatible".to_string(),
        }
    }

    pub fn post_json(path: impl Into<String>, scope: GatewayScopeRequest, body: Value) -> Self {
        Self {
            method: OpenAiGatewayMethod::Post,
            path: path.into(),
            headers: BTreeMap::new(),
            body: Some(body),
            scope,
            provider_name: None,
            client_profile: "openai_compatible".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct OpenAiGatewayResponse {
    pub status_code: u16,
    pub body: OpenAiGatewayBody,
    pub audit: GatewayAuditReport,
}

impl OpenAiGatewayResponse {
    pub fn finish_deferred_maintenance(&mut self, services: &mut OpenAiGatewayServices<'_>) {
        if let Some(outcome) = self.body.finish_deferred_maintenance(services) {
            self.audit
                .record_stage(GatewayAuditStage::Maintenance, outcome);
        }
    }

    fn prepare_post_reply_maintenance(
        &mut self,
        plan: GatewayMaintenancePlan,
        services: &mut OpenAiGatewayServices<'_>,
    ) {
        match &mut self.body {
            OpenAiGatewayBody::Json(body) => {
                let outcome = run_json_maintenance(plan, body, services);
                self.audit
                    .record_stage(GatewayAuditStage::Maintenance, outcome.into());
            }
            OpenAiGatewayBody::Sse(body) => {
                let placeholder = OpenAiSseBody::buffered(Vec::new());
                let owned = std::mem::replace(body, placeholder);
                *body = owned.with_deferred_maintenance(plan);
            }
        }
    }
}

pub enum OpenAiGatewayBody {
    Json(Value),
    Sse(OpenAiSseBody),
}

impl std::fmt::Debug for OpenAiGatewayBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(value) => f.debug_tuple("Json").field(value).finish(),
            Self::Sse(_) => f.write_str("Sse(<stream>)"),
        }
    }
}

impl OpenAiGatewayBody {
    pub const fn is_sse(&self) -> bool {
        matches!(self, Self::Sse(_))
    }

    pub fn buffered_sse_chunks(&self) -> Option<&[String]> {
        match self {
            Self::Json(_) => None,
            Self::Sse(body) => body.buffered_chunks(),
        }
    }

    pub fn into_json(self) -> Option<Value> {
        match self {
            Self::Json(value) => Some(value),
            Self::Sse(_) => None,
        }
    }

    fn finish_deferred_maintenance(
        &mut self,
        services: &mut OpenAiGatewayServices<'_>,
    ) -> Option<GatewayAuditOutcome> {
        match self {
            Self::Json(_) => None,
            Self::Sse(body) => body
                .finish_deferred_maintenance(services)
                .map(GatewayAuditOutcome::from),
        }
    }
}

pub trait OpenAiSseStream: Send {
    fn next_chunk(&mut self) -> Result<Option<String>>;
}

pub struct OpenAiSseBody {
    source: OpenAiSseSource,
    deferred_maintenance: Option<Box<OpenAiDeferredMaintenance>>,
}

enum OpenAiSseSource {
    Buffered { chunks: Vec<String>, offset: usize },
    Streaming(Box<dyn OpenAiSseStream>),
}

impl OpenAiSseBody {
    pub fn buffered(chunks: Vec<String>) -> Self {
        Self {
            source: OpenAiSseSource::Buffered { chunks, offset: 0 },
            deferred_maintenance: None,
        }
    }

    pub fn streaming(stream: Box<dyn OpenAiSseStream>) -> Self {
        Self {
            source: OpenAiSseSource::Streaming(stream),
            deferred_maintenance: None,
        }
    }

    pub(crate) fn with_deferred_maintenance(mut self, plan: GatewayMaintenancePlan) -> Self {
        self.deferred_maintenance = Some(Box::new(OpenAiDeferredMaintenance::new(plan)));
        self
    }

    pub fn buffered_chunks(&self) -> Option<&[String]> {
        match &self.source {
            OpenAiSseSource::Buffered { chunks, offset } => Some(&chunks[*offset..]),
            OpenAiSseSource::Streaming(_) => None,
        }
    }

    pub fn next_chunk(&mut self) -> Result<Option<String>> {
        let chunk = match &mut self.source {
            OpenAiSseSource::Buffered { chunks, offset } => {
                if let Some(chunk) = chunks.get(*offset).cloned() {
                    *offset += 1;
                    Some(chunk)
                } else {
                    None
                }
            }
            OpenAiSseSource::Streaming(stream) => stream.next_chunk()?,
        };
        if let Some(chunk) = chunk.as_deref() {
            if let Some(maintenance) = &mut self.deferred_maintenance {
                maintenance.observe_sse_chunk(chunk);
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
    ) -> Option<GatewayMaintenanceRunOutcome> {
        self.deferred_maintenance
            .take()
            .map(|maintenance| maintenance.finish(services))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenAiUpstreamRequest {
    pub endpoint: String,
    pub body: Value,
    pub stream: bool,
    pub model: String,
    pub extracted_user_text: String,
}

pub enum OpenAiUpstreamResponse {
    Json {
        status_code: u16,
        body: Value,
    },
    Sse {
        status_code: u16,
        body: OpenAiSseBody,
    },
}

impl OpenAiUpstreamResponse {
    pub fn json(status_code: u16, body: Value) -> Self {
        Self::Json { status_code, body }
    }

    pub fn sse(status_code: u16, chunks: Vec<String>) -> Self {
        Self::Sse {
            status_code,
            body: OpenAiSseBody::buffered(chunks),
        }
    }
}

pub trait OpenAiCompatibleUpstream {
    fn models(&mut self, provider: &GatewayProviderConfig) -> Result<OpenAiUpstreamResponse>;

    fn chat_completion(
        &mut self,
        provider: &GatewayProviderConfig,
        request: OpenAiUpstreamRequest,
    ) -> Result<OpenAiUpstreamResponse>;

    fn responses(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OpenAiUpstreamRequest,
    ) -> Result<OpenAiUpstreamResponse> {
        Err(GatewayError::provider_unavailable(
            "openai-compatible provider does not support responses",
        ))
    }

    fn embeddings(
        &mut self,
        _provider: &GatewayProviderConfig,
        _request: OpenAiUpstreamRequest,
    ) -> Result<OpenAiUpstreamResponse> {
        Err(GatewayError::provider_unavailable(
            "openai-compatible provider does not support embeddings",
        ))
    }
}

pub fn handle_openai_request(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    request: OpenAiGatewayRequest,
    upstream: &mut dyn OpenAiCompatibleUpstream,
) -> Result<OpenAiGatewayResponse> {
    let mut services = OpenAiGatewayServices::new();
    handle_openai_request_with_services(gateway, config, request, upstream, &mut services)
}

pub fn handle_openai_request_with_services(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    request: OpenAiGatewayRequest,
    upstream: &mut dyn OpenAiCompatibleUpstream,
    services: &mut OpenAiGatewayServices<'_>,
) -> Result<OpenAiGatewayResponse> {
    let provider = select_provider_for_kind(
        config,
        request.provider_name.as_deref(),
        GatewayProviderKind::OpenAiCompatible,
        "openai-compatible",
    )?;

    match (request.method, request.path.as_str()) {
        (OpenAiGatewayMethod::Get, "/v1/models") => {
            handle_models(config, request, provider, upstream)
        }
        (OpenAiGatewayMethod::Post, "/v1/chat/completions") => {
            handle_chat_completion(gateway, config, request, provider, upstream, services)
        }
        (OpenAiGatewayMethod::Post, "/v1/responses") => {
            handle_responses(gateway, config, request, provider, upstream, services)
        }
        (OpenAiGatewayMethod::Post, "/v1/embeddings") => {
            handle_embeddings(config, request, provider, upstream)
        }
        (OpenAiGatewayMethod::Get, "/v1/bm/provider-capabilities") => {
            handle_provider_capabilities(config, request, upstream)
        }
        _ => Err(GatewayError::invalid_request(
            "unsupported OpenAI gateway route",
        )),
    }
}

fn handle_models(
    config: &GatewayConfig,
    request: OpenAiGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OpenAiCompatibleUpstream,
) -> Result<OpenAiGatewayResponse> {
    let scope = GatewayScopeResolver::new(config.scope.clone()).resolve(&request.scope)?;
    let mut audit = GatewayAuditReport::new(
        "openai-models",
        "/v1/models",
        request.client_profile,
        "none",
        scope,
    );
    let response = upstream.models(provider).map_err(|error| {
        audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
        GatewayError::upstream_unavailable(error.to_string())
    })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    Ok(upstream_response_to_gateway(response, audit))
}

fn handle_chat_completion(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    mut request: OpenAiGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OpenAiCompatibleUpstream,
    services: &mut OpenAiGatewayServices<'_>,
) -> Result<OpenAiGatewayResponse> {
    let body = request
        .body
        .take()
        .ok_or_else(|| GatewayError::invalid_request("chat completions body is required"))?;
    let body_object = body
        .as_object()
        .ok_or_else(|| GatewayError::invalid_request("chat completions body must be an object"))?;
    let model_alias = body_object
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| GatewayError::invalid_request("chat completions model is required"))?;
    if request.scope.model_alias.is_none() {
        request.scope.model_alias = Some(model_alias.to_string());
    }

    let scope = GatewayScopeResolver::new(config.scope.clone()).resolve(&request.scope)?;
    let mut audit = GatewayAuditReport::new(
        "openai-chat",
        "/v1/chat/completions",
        request.client_profile,
        model_alias,
        scope.clone(),
    );
    let runtime = gateway.runtime_for_scope(scope.entry_scope.clone())?;
    let input_transcript = extract_chat_input_transcript(body_object.get("messages"))?;
    let extracted_user_text = input_transcript.latest_user_text.clone();
    let external_content_used = request_uses_external_content(body_object.get("messages"))
        || body_object.get("tools").is_some();
    let provider_limit = provider_model_context_limit(provider, model_alias);
    let runtime_budget = runtime.runtime().runtime_budget();
    let tool_registry_refs = if let Some(registry) =
        request_scoped_agent_tool_registry("openai-compatible", body_object.get("tools"))
    {
        audit.record_note("gateway_host_tools_no_cold_route");
        let registry_ref = registry.registry_ref();
        runtime
            .runtime()
            .upsert_agent_tool_registry(registry)
            .map_err(|error| GatewayError::projection_failed(error.to_string()))?;
        vec![registry_ref]
    } else {
        Vec::new()
    };
    let projection = runtime
        .runtime()
        .project(MemoryProjectionRequest {
            user_query: extracted_user_text.clone(),
            system_max_len: runtime_budget
                .projection_render_chars_for_request(usize::MAX, Some(&provider_limit)),
            recent_messages_limit: runtime_budget
                .projection_source_budget
                .recent_messages_limit,
            pressure: config.projection.pressure,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs,
        })
        .map_err(|error| {
            audit.record_stage(GatewayAuditStage::Projection, GatewayAuditOutcome::Failed);
            GatewayError::projection_failed(error.to_string())
        })?;
    audit.record_stage(
        GatewayAuditStage::Projection,
        GatewayAuditOutcome::Succeeded,
    );
    audit.record_projection(&config.audit, &projection)?;

    let stream = body_object
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let model = provider_model_name(provider, model_alias);
    let upstream_body = build_upstream_chat_body(&body, &projection.system_memory_block, &model)?;
    let carry = projection.context.into_runtime_carry();
    let maintenance_plan = GatewayMaintenancePlan::new(GatewayMaintenancePlanInput {
        runtime,
        user_content: extracted_user_text.clone(),
        input_messages: input_transcript.messages.clone(),
        conversation: ConversationScope {
            channel: scope.channel.clone(),
            chat_id: scope.chat_id.clone(),
            conversation_id: request
                .scope
                .client_conversation_hint
                .clone()
                .or_else(|| request.scope.body_conversation_hint.clone()),
        },
        turn_source: MemoryTurnSource {
            ingress: bm_sdk::IngressKind::User,
            channel: scope.channel.clone(),
            provider: Some(format!("{:?}", provider.kind)),
            protocol: MemoryTurnProtocol::OpenAiChat,
            endpoint: Some("/v1/chat/completions".to_string()),
            model_alias: Some(model_alias.to_string()),
            model_resolved: Some(model.clone()),
            request_id: request.scope.request_id_hint.clone(),
            client_conversation_hint: request.scope.client_conversation_hint.clone(),
        },
        external_content_used,
        runtime_skill_selected_ids: carry.runtime_skill_selected_ids,
        task_learning_selected_ids: carry.task_recall_selected_ids,
        pressure: config.projection.pressure,
        mode_input: RuntimeLifecycleModeInput::default(),
        config: config.maintenance,
    });
    let upstream_request = OpenAiUpstreamRequest {
        endpoint: "/chat/completions".to_string(),
        body: upstream_body,
        stream,
        model,
        extracted_user_text,
    };
    let response = upstream
        .chat_completion(provider, upstream_request)
        .map_err(|error| {
            audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
            GatewayError::upstream_unavailable(error.to_string())
        })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    let mut response = upstream_response_to_gateway(response, audit);
    response.prepare_post_reply_maintenance(maintenance_plan, services);
    Ok(response)
}

fn handle_responses(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    mut request: OpenAiGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OpenAiCompatibleUpstream,
    services: &mut OpenAiGatewayServices<'_>,
) -> Result<OpenAiGatewayResponse> {
    if !provider.openai_responses_supported {
        return Err(GatewayError::provider_unavailable(
            "openai-compatible provider does not support responses",
        ));
    }
    let body = request
        .body
        .take()
        .ok_or_else(|| GatewayError::invalid_request("responses body is required"))?;
    let body_object = body
        .as_object()
        .ok_or_else(|| GatewayError::invalid_request("responses body must be an object"))?;
    let model_alias = body_object
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| GatewayError::invalid_request("responses model is required"))?;
    if body_object.get("previous_response_id").is_some()
        && !provider.openai_stateful_responses_supported
    {
        return Err(GatewayError::provider_unavailable(
            "stateful responses are not supported by this provider",
        ));
    }
    if request.scope.model_alias.is_none() {
        request.scope.model_alias = Some(model_alias.to_string());
    }

    let scope = GatewayScopeResolver::new(config.scope.clone()).resolve(&request.scope)?;
    let mut audit = GatewayAuditReport::new(
        "openai-responses",
        "/v1/responses",
        request.client_profile,
        model_alias,
        scope.clone(),
    );
    if body_object.get("previous_response_id").is_some() {
        audit.record_note("openai_responses_stateful_passthrough");
    }
    let runtime = gateway.runtime_for_scope(scope.entry_scope.clone())?;
    let extracted_user_text = extract_response_input_text(body_object.get("input"));
    let input_transcript = input_transcript_from_user_text(&extracted_user_text);
    let external_content_used = response_input_uses_external_content(body_object.get("input"))
        || body_object.get("tools").is_some();
    let provider_limit = provider_model_context_limit(provider, model_alias);
    let runtime_budget = runtime.runtime().runtime_budget();
    let tool_registry_refs = if let Some(registry) =
        request_scoped_agent_tool_registry("openai-compatible", body_object.get("tools"))
    {
        audit.record_note("gateway_host_tools_no_cold_route");
        let registry_ref = registry.registry_ref();
        runtime
            .runtime()
            .upsert_agent_tool_registry(registry)
            .map_err(|error| GatewayError::projection_failed(error.to_string()))?;
        vec![registry_ref]
    } else {
        Vec::new()
    };
    let projection = runtime
        .runtime()
        .project(MemoryProjectionRequest {
            user_query: extracted_user_text.clone(),
            system_max_len: runtime_budget
                .projection_render_chars_for_request(usize::MAX, Some(&provider_limit)),
            recent_messages_limit: runtime_budget
                .projection_source_budget
                .recent_messages_limit,
            pressure: config.projection.pressure,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs,
        })
        .map_err(|error| {
            audit.record_stage(GatewayAuditStage::Projection, GatewayAuditOutcome::Failed);
            GatewayError::projection_failed(error.to_string())
        })?;
    audit.record_stage(
        GatewayAuditStage::Projection,
        GatewayAuditOutcome::Succeeded,
    );
    audit.record_projection(&config.audit, &projection)?;

    let stream = body_object
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let model = provider_model_name(provider, model_alias);
    let upstream_body =
        build_upstream_responses_body(&body, &projection.system_memory_block, &model)?;
    let carry = projection.context.into_runtime_carry();
    let maintenance_plan = GatewayMaintenancePlan::new(GatewayMaintenancePlanInput {
        runtime,
        user_content: extracted_user_text.clone(),
        input_messages: input_transcript.messages,
        conversation: ConversationScope {
            channel: scope.channel.clone(),
            chat_id: scope.chat_id.clone(),
            conversation_id: request
                .scope
                .client_conversation_hint
                .clone()
                .or_else(|| request.scope.body_conversation_hint.clone()),
        },
        turn_source: MemoryTurnSource {
            ingress: bm_sdk::IngressKind::User,
            channel: scope.channel.clone(),
            provider: Some(format!("{:?}", provider.kind)),
            protocol: MemoryTurnProtocol::OpenAiResponses,
            endpoint: Some("/v1/responses".to_string()),
            model_alias: Some(model_alias.to_string()),
            model_resolved: Some(model.clone()),
            request_id: request.scope.request_id_hint.clone(),
            client_conversation_hint: request.scope.client_conversation_hint.clone(),
        },
        external_content_used,
        runtime_skill_selected_ids: carry.runtime_skill_selected_ids,
        task_learning_selected_ids: carry.task_recall_selected_ids,
        pressure: config.projection.pressure,
        mode_input: RuntimeLifecycleModeInput::default(),
        config: config.maintenance,
    });
    let upstream_request = OpenAiUpstreamRequest {
        endpoint: "/responses".to_string(),
        body: upstream_body,
        stream,
        model,
        extracted_user_text,
    };
    let response = upstream
        .responses(provider, upstream_request)
        .map_err(|error| {
            audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
            GatewayError::upstream_unavailable(error.to_string())
        })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    let mut response = upstream_response_to_gateway(response, audit);
    response.prepare_post_reply_maintenance(maintenance_plan, services);
    Ok(response)
}

fn handle_embeddings(
    config: &GatewayConfig,
    mut request: OpenAiGatewayRequest,
    provider: &GatewayProviderConfig,
    upstream: &mut dyn OpenAiCompatibleUpstream,
) -> Result<OpenAiGatewayResponse> {
    if !provider.openai_embeddings_supported {
        return Err(GatewayError::provider_unavailable(
            "openai-compatible provider does not support embeddings",
        ));
    }
    let body = request
        .body
        .take()
        .ok_or_else(|| GatewayError::invalid_request("embeddings body is required"))?;
    let body_object = body
        .as_object()
        .ok_or_else(|| GatewayError::invalid_request("embeddings body must be an object"))?;
    let model_alias = body_object
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| GatewayError::invalid_request("embeddings model is required"))?;
    if request.scope.model_alias.is_none() {
        request.scope.model_alias = Some(model_alias.to_string());
    }
    let scope = GatewayScopeResolver::new(config.scope.clone()).resolve(&request.scope)?;
    let mut audit = GatewayAuditReport::new(
        "openai-embeddings",
        "/v1/embeddings",
        request.client_profile,
        model_alias,
        scope,
    );
    let model = provider_model_name(provider, model_alias);
    let upstream_body = build_upstream_passthrough_body(&body, &model)?;
    let response = upstream
        .embeddings(
            provider,
            OpenAiUpstreamRequest {
                endpoint: "/embeddings".to_string(),
                body: upstream_body,
                stream: false,
                model,
                extracted_user_text: String::new(),
            },
        )
        .map_err(|error| {
            audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
            GatewayError::upstream_unavailable(error.to_string())
        })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    Ok(upstream_response_to_gateway(response, audit))
}

fn handle_provider_capabilities(
    config: &GatewayConfig,
    request: OpenAiGatewayRequest,
    upstream: &mut dyn OpenAiCompatibleUpstream,
) -> Result<OpenAiGatewayResponse> {
    let scope = GatewayScopeResolver::new(config.scope.clone()).resolve(&request.scope)?;
    let mut audit = GatewayAuditReport::new(
        "openai-provider-capabilities",
        "/v1/bm/provider-capabilities",
        request.client_profile,
        request
            .provider_name
            .clone()
            .unwrap_or_else(|| config.default_provider.clone()),
        scope,
    );
    let report =
        probe_openai_provider_capabilities(config, request.provider_name.as_deref(), upstream)
            .inspect_err(|_| {
                audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Failed);
            })?;
    audit.record_stage(GatewayAuditStage::Upstream, GatewayAuditOutcome::Succeeded);
    Ok(OpenAiGatewayResponse {
        status_code: 200,
        body: OpenAiGatewayBody::Json(
            serde_json::to_value(report)
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?,
        ),
        audit,
    })
}

fn upstream_response_to_gateway(
    response: OpenAiUpstreamResponse,
    audit: GatewayAuditReport,
) -> OpenAiGatewayResponse {
    match response {
        OpenAiUpstreamResponse::Json { status_code, body } => OpenAiGatewayResponse {
            status_code,
            body: OpenAiGatewayBody::Json(body),
            audit,
        },
        OpenAiUpstreamResponse::Sse { status_code, body } => OpenAiGatewayResponse {
            status_code,
            body: OpenAiGatewayBody::Sse(body),
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

fn provider_model_context_limit(
    provider: &GatewayProviderConfig,
    model_alias: &str,
) -> ProviderModelContextLimit {
    ProviderModelContextLimit {
        provider: Some(provider.base_url.clone()),
        model: Some(provider_model_name(provider, model_alias)),
        max_context_tokens: None,
        max_prompt_chars: provider.max_prompt_chars,
    }
}

fn build_upstream_chat_body(
    original_body: &Value,
    memory_block: &str,
    model: &str,
) -> Result<Value> {
    let mut object = original_body
        .as_object()
        .cloned()
        .ok_or_else(|| GatewayError::invalid_request("chat completions body must be an object"))?;
    object.insert("model".to_string(), Value::String(model.to_string()));
    let messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| GatewayError::invalid_request("chat completions messages must be an array"))?
        .clone();
    if let Some(memory_text) = render_model_facing_projection(memory_block) {
        object.insert(
            "messages".to_string(),
            Value::Array(inject_memory_projection_into_messages(
                messages,
                memory_text,
            )),
        );
    }
    Ok(Value::Object(object))
}

fn inject_memory_projection_into_messages(
    mut messages: Vec<Value>,
    memory_text: String,
) -> Vec<Value> {
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
        return messages;
    }

    let mut memory_message = Map::new();
    memory_message.insert("role".to_string(), Value::String("system".to_string()));
    memory_message.insert("content".to_string(), Value::String(memory_text));
    messages.insert(0, Value::Object(memory_message));
    messages
}

fn build_upstream_responses_body(
    original_body: &Value,
    memory_block: &str,
    model: &str,
) -> Result<Value> {
    let mut object = original_body
        .as_object()
        .cloned()
        .ok_or_else(|| GatewayError::invalid_request("responses body must be an object"))?;
    object.insert("model".to_string(), Value::String(model.to_string()));
    if let Some(memory) = render_model_facing_projection(memory_block) {
        let instructions = match object.get("instructions") {
            Some(Value::String(existing)) if !existing.trim().is_empty() => {
                format!("{}\n\n{memory}", existing.trim())
            }
            Some(Value::String(_)) | None => memory,
            Some(_) => {
                return Err(GatewayError::invalid_request(
                    "responses instructions must be a string",
                ))
            }
        };
        object.insert("instructions".to_string(), Value::String(instructions));
    }
    Ok(Value::Object(object))
}

fn build_upstream_passthrough_body(original_body: &Value, model: &str) -> Result<Value> {
    let mut object = original_body
        .as_object()
        .cloned()
        .ok_or_else(|| GatewayError::invalid_request("request body must be an object"))?;
    object.insert("model".to_string(), Value::String(model.to_string()));
    Ok(Value::Object(object))
}

fn extract_chat_input_transcript(messages: Option<&Value>) -> Result<GatewayInputTranscript> {
    let messages = messages.and_then(Value::as_array).ok_or_else(|| {
        GatewayError::invalid_request("chat completions messages must be an array")
    })?;
    let mut transcript = GatewayInputTranscript::default();
    for message in messages {
        let Some(role) = message.get("role").and_then(Value::as_str) else {
            continue;
        };
        let mut parts = Vec::new();
        extract_content_text(message.get("content"), &mut parts);
        let content = parts.join("\n").trim().to_string();
        if content.is_empty() {
            continue;
        }
        if role.eq_ignore_ascii_case("user") {
            transcript.latest_user_text = content.clone();
            transcript
                .messages
                .push(transcript_message_with_gateway_speaker(
                    TranscriptInputMessage::user(content),
                    message,
                    role,
                ));
        } else if role.eq_ignore_ascii_case("assistant") {
            transcript
                .messages
                .push(transcript_message_with_gateway_speaker(
                    TranscriptInputMessage::assistant(content),
                    message,
                    role,
                ));
        }
    }
    Ok(transcript)
}

fn transcript_message_with_gateway_speaker(
    message: TranscriptInputMessage,
    raw: &Value,
    role: &str,
) -> TranscriptInputMessage {
    let Some(speaker_id) = raw
        .get("speaker_id")
        .or_else(|| raw.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return message;
    };
    let speaker_kind = raw
        .get("speaker_kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_gateway_speaker_kind(role));
    message.with_speaker(speaker_id, speaker_kind)
}

fn default_gateway_speaker_kind(role: &str) -> &'static str {
    if role.eq_ignore_ascii_case("user") {
        "human"
    } else if role.eq_ignore_ascii_case("assistant") {
        "llm_agent"
    } else {
        "external"
    }
}

fn input_transcript_from_user_text(text: &str) -> GatewayInputTranscript {
    let latest_user_text = text.trim().to_string();
    let messages = if latest_user_text.is_empty() {
        Vec::new()
    } else {
        vec![TranscriptInputMessage::user(latest_user_text.clone())]
    };
    GatewayInputTranscript {
        latest_user_text,
        messages,
    }
}

fn extract_response_input_text(input: Option<&Value>) -> String {
    let mut parts = Vec::new();
    extract_response_value_text(input, &mut parts, true);
    parts.join("\n").trim().to_string()
}

fn extract_response_value_text(value: Option<&Value>, parts: &mut Vec<String>, role_allowed: bool) {
    match value {
        Some(Value::String(text)) if role_allowed && !text.trim().is_empty() => {
            parts.push(text.trim().to_string())
        }
        Some(Value::Array(items)) => {
            for item in items {
                extract_response_value_text(Some(item), parts, role_allowed);
            }
        }
        Some(Value::Object(object)) => {
            let item_role_allowed = match object.get("role").and_then(Value::as_str) {
                Some("user") => true,
                Some(_) => false,
                None => role_allowed,
            };
            if object.get("type").and_then(Value::as_str) == Some("input_text") {
                if let Some(text) = object.get("text").and_then(Value::as_str) {
                    if item_role_allowed && !text.trim().is_empty() {
                        parts.push(text.trim().to_string());
                    }
                }
            } else if let Some(text) = object.get("text").and_then(Value::as_str) {
                if item_role_allowed && !text.trim().is_empty() {
                    parts.push(text.trim().to_string());
                }
            }
            extract_response_value_text(object.get("content"), parts, item_role_allowed);
        }
        _ => {}
    }
}

fn response_input_uses_external_content(input: Option<&Value>) -> bool {
    match input {
        Some(Value::Array(items)) => items.iter().any(response_value_uses_external_content),
        Some(value) => response_value_uses_external_content(value),
        None => false,
    }
}

fn response_value_uses_external_content(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if let Some(kind) = object.get("type").and_then(Value::as_str) {
        if !matches!(kind, "message" | "input_text") {
            return true;
        }
    }
    match object.get("content") {
        Some(Value::Array(items)) => items.iter().any(|item| {
            item.get("type").and_then(Value::as_str) != Some("input_text")
                || response_value_uses_external_content(item)
        }),
        Some(value) => response_value_uses_external_content(value),
        None => false,
    }
}

fn request_uses_external_content(messages: Option<&Value>) -> bool {
    let Some(messages) = messages.and_then(Value::as_array) else {
        return false;
    };
    messages.iter().any(|message| {
        matches!(message.get("role").and_then(Value::as_str), Some("tool"))
            || message
                .get("content")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .any(|item| item.get("type").and_then(Value::as_str) != Some("text"))
                })
                .unwrap_or(false)
    })
}

fn extract_content_text(content: Option<&Value>, parts: &mut Vec<String>) {
    match content {
        Some(Value::String(text)) if !text.trim().is_empty() => parts.push(text.trim().to_string()),
        Some(Value::Array(items)) => {
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        if !text.trim().is_empty() {
                            parts.push(text.trim().to_string());
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(feature = "client-reqwest")]
pub struct ReqwestOpenAiCompatibleUpstream {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "client-reqwest")]
impl ReqwestOpenAiCompatibleUpstream {
    pub fn new() -> Result<Self> {
        Self::new_with_timeout(std::time::Duration::from_secs(600))
    }

    pub fn new_with_timeout(timeout: std::time::Duration) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        Ok(Self { client })
    }
}

#[cfg(feature = "client-reqwest")]
impl OpenAiCompatibleUpstream for ReqwestOpenAiCompatibleUpstream {
    fn models(&mut self, provider: &GatewayProviderConfig) -> Result<OpenAiUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::GET, "/models")?
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        response_to_json(response)
    }

    fn chat_completion(
        &mut self,
        provider: &GatewayProviderConfig,
        request: OpenAiUpstreamRequest,
    ) -> Result<OpenAiUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::POST, request.endpoint.as_str())?
            .json(&request.body)
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        if request.stream {
            let status_code = response.status().as_u16();
            return Ok(OpenAiUpstreamResponse::Sse {
                status_code,
                body: OpenAiSseBody::streaming(Box::new(ReqwestLineSseStream::new(response))),
            });
        }
        response_to_json(response)
    }

    fn responses(
        &mut self,
        provider: &GatewayProviderConfig,
        request: OpenAiUpstreamRequest,
    ) -> Result<OpenAiUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::POST, request.endpoint.as_str())?
            .json(&request.body)
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        if request.stream {
            let status_code = response.status().as_u16();
            return Ok(OpenAiUpstreamResponse::Sse {
                status_code,
                body: OpenAiSseBody::streaming(Box::new(ReqwestLineSseStream::new(response))),
            });
        }
        response_to_json(response)
    }

    fn embeddings(
        &mut self,
        provider: &GatewayProviderConfig,
        request: OpenAiUpstreamRequest,
    ) -> Result<OpenAiUpstreamResponse> {
        let response = self
            .authorized_request(provider, reqwest::Method::POST, request.endpoint.as_str())?
            .json(&request.body)
            .send()
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
        response_to_json(response)
    }
}

#[cfg(feature = "client-reqwest")]
impl ReqwestOpenAiCompatibleUpstream {
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
fn response_to_json(response: reqwest::blocking::Response) -> Result<OpenAiUpstreamResponse> {
    let status_code = response.status().as_u16();
    let body = response
        .json::<Value>()
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
    Ok(OpenAiUpstreamResponse::Json { status_code, body })
}

#[cfg(feature = "client-reqwest")]
struct ReqwestLineSseStream {
    reader: std::io::BufReader<reqwest::blocking::Response>,
}

#[cfg(feature = "client-reqwest")]
impl ReqwestLineSseStream {
    fn new(response: reqwest::blocking::Response) -> Self {
        Self {
            reader: std::io::BufReader::new(response),
        }
    }
}

#[cfg(feature = "client-reqwest")]
impl OpenAiSseStream for ReqwestLineSseStream {
    fn next_chunk(&mut self) -> Result<Option<String>> {
        use std::io::BufRead;

        let mut event = String::new();
        loop {
            let mut line = String::new();
            let read = self
                .reader
                .read_line(&mut line)
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            if read == 0 {
                return if event.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(event))
                };
            }
            event.push_str(&line);
            if line == "\n" || line == "\r\n" {
                return Ok(Some(event));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openai_chat_transcript_maps_name_to_speaker_identity() {
        let body = json!([
            {
                "role": "user",
                "name": "reviewer-agent",
                "content": "remember release guard"
            }
        ]);

        let transcript = extract_chat_input_transcript(Some(&body)).expect("transcript");

        assert_eq!(transcript.messages.len(), 1);
        assert_eq!(transcript.messages[0].role, "user");
        assert_eq!(transcript.messages[0].speaker_id, "reviewer-agent");
        assert_eq!(transcript.messages[0].speaker_kind, "human");
    }
}
