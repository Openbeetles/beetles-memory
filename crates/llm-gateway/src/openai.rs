use std::collections::BTreeMap;

use bm_sdk::{MemoryProjectionRequest, RuntimeLifecycleModeInput};
use serde_json::{Map, Value};

use crate::{
    GatewayAuditOutcome, GatewayAuditReport, GatewayAuditStage, GatewayConfig, GatewayError,
    GatewayProviderConfig, GatewayProviderKind, GatewayRuntime, GatewayScopeRequest,
    GatewayScopeResolver, Result,
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
}

pub trait OpenAiSseStream: Send {
    fn next_chunk(&mut self) -> Result<Option<String>>;
}

pub struct OpenAiSseBody {
    source: OpenAiSseSource,
}

enum OpenAiSseSource {
    Buffered { chunks: Vec<String>, offset: usize },
    Streaming(Box<dyn OpenAiSseStream>),
}

impl OpenAiSseBody {
    pub fn buffered(chunks: Vec<String>) -> Self {
        Self {
            source: OpenAiSseSource::Buffered { chunks, offset: 0 },
        }
    }

    pub fn streaming(stream: Box<dyn OpenAiSseStream>) -> Self {
        Self {
            source: OpenAiSseSource::Streaming(stream),
        }
    }

    pub fn buffered_chunks(&self) -> Option<&[String]> {
        match &self.source {
            OpenAiSseSource::Buffered { chunks, offset } => Some(&chunks[*offset..]),
            OpenAiSseSource::Streaming(_) => None,
        }
    }

    pub fn next_chunk(&mut self) -> Result<Option<String>> {
        match &mut self.source {
            OpenAiSseSource::Buffered { chunks, offset } => {
                if let Some(chunk) = chunks.get(*offset).cloned() {
                    *offset += 1;
                    Ok(Some(chunk))
                } else {
                    Ok(None)
                }
            }
            OpenAiSseSource::Streaming(stream) => stream.next_chunk(),
        }
    }

    pub fn collect_chunks(mut self) -> Result<Vec<String>> {
        let mut chunks = Vec::new();
        while let Some(chunk) = self.next_chunk()? {
            chunks.push(chunk);
        }
        Ok(chunks)
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
}

pub fn handle_openai_request(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    request: OpenAiGatewayRequest,
    upstream: &mut dyn OpenAiCompatibleUpstream,
) -> Result<OpenAiGatewayResponse> {
    let provider_name = request
        .provider_name
        .as_deref()
        .unwrap_or(config.default_provider.as_str());
    let provider = config
        .providers
        .get(provider_name)
        .ok_or_else(|| GatewayError::provider_unavailable("provider is not configured"))?;
    if provider.kind != GatewayProviderKind::OpenAiCompatible {
        return Err(GatewayError::provider_unavailable(
            "provider is not openai-compatible",
        ));
    }

    match (request.method, request.path.as_str()) {
        (OpenAiGatewayMethod::Get, "/v1/models") => {
            handle_models(config, request, provider, upstream)
        }
        (OpenAiGatewayMethod::Post, "/v1/chat/completions") => {
            handle_chat_completion(gateway, config, request, provider, upstream)
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
    let extracted_user_text = extract_messages_text(body_object.get("messages"))?;
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
        .unwrap_or(false);
    let model = provider_model_name(provider, model_alias);
    let upstream_body = build_upstream_chat_body(&body, &projection.system_memory_block, &model)?;
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
    Ok(upstream_response_to_gateway(response, audit))
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
    if !memory_block.trim().is_empty() {
        let mut injected = Vec::with_capacity(messages.len() + 1);
        let mut memory_message = Map::new();
        memory_message.insert("role".to_string(), Value::String("system".to_string()));
        memory_message.insert(
            "content".to_string(),
            Value::String(format!("Beetle Memory context:\n{memory_block}")),
        );
        injected.push(Value::Object(memory_message));
        injected.extend(messages);
        object.insert("messages".to_string(), Value::Array(injected));
    }
    Ok(Value::Object(object))
}

fn extract_messages_text(messages: Option<&Value>) -> Result<String> {
    let messages = messages.and_then(Value::as_array).ok_or_else(|| {
        GatewayError::invalid_request("chat completions messages must be an array")
    })?;
    let mut parts = Vec::new();
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        extract_content_text(message.get("content"), &mut parts);
    }
    Ok(parts.join("\n").trim().to_string())
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
        let client = reqwest::blocking::Client::builder()
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
