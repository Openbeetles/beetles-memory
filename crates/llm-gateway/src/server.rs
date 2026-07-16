use std::collections::BTreeMap;
use std::io::Write;

use bm_entry::{
    read_authorized_http_request, EntryAcceptedTcpStream, EntryHttpAuthorization,
    EntryHttpIngressErrorKind, EntryHttpIngressLimits, EntryOperationCapability,
};
use serde_json::json;

use crate::{
    ollama::{handle_ollama_request_with_services_in_budget_lease, required_ollama_capabilities},
    openai::{handle_openai_request_with_services_in_budget_lease, required_openai_capabilities},
    GatewayConfig, GatewayError, GatewayErrorKey, GatewayRequestBudgetContext, GatewayRuntime,
    GatewayScopeRequest, OllamaGatewayBody, OllamaGatewayMethod, OllamaGatewayRequest,
    OllamaNativeUpstream, OpenAiCompatibleUpstream, OpenAiGatewayBody, OpenAiGatewayMethod,
    OpenAiGatewayRequest, OpenAiGatewayServices, Result,
};

pub struct GatewayHttpRequestBindings<'a, 'services> {
    openai_upstream: &'a mut dyn OpenAiCompatibleUpstream,
    ollama_upstream: &'a mut dyn OllamaNativeUpstream,
    services: &'a mut OpenAiGatewayServices<'services>,
}

impl<'a, 'services> GatewayHttpRequestBindings<'a, 'services> {
    pub fn new(
        openai_upstream: &'a mut dyn OpenAiCompatibleUpstream,
        ollama_upstream: &'a mut dyn OllamaNativeUpstream,
        services: &'a mut OpenAiGatewayServices<'services>,
    ) -> Self {
        Self {
            openai_upstream,
            ollama_upstream,
            services,
        }
    }
}

pub fn serve_llm_gateway_http_accepted_stream(
    gateway: &GatewayRuntime,
    openai_upstream: &mut dyn OpenAiCompatibleUpstream,
    ollama_upstream: &mut dyn OllamaNativeUpstream,
    stream: &mut EntryAcceptedTcpStream,
) -> Result<()> {
    let mut services = OpenAiGatewayServices::new();
    serve_llm_gateway_http_accepted_stream_with_services(
        gateway,
        openai_upstream,
        ollama_upstream,
        &mut services,
        stream,
    )
}

pub fn serve_llm_gateway_http_accepted_stream_with_services(
    gateway: &GatewayRuntime,
    openai_upstream: &mut dyn OpenAiCompatibleUpstream,
    ollama_upstream: &mut dyn OllamaNativeUpstream,
    services: &mut OpenAiGatewayServices<'_>,
    stream: &mut EntryAcceptedTcpStream,
) -> Result<()> {
    let context = match gateway.begin_request() {
        Ok(context) => context,
        Err(error) => return write_error_response(stream, &error),
    };
    match gateway.execute_with_request_context(&context, || {
        let bindings = GatewayHttpRequestBindings::new(openai_upstream, ollama_upstream, services);
        serve_llm_gateway_http_stream_in_budget_lease(gateway, &context, bindings, stream)
    }) {
        Ok(()) => Ok(()),
        Err(error) => write_error_response(stream, &error),
    }
}

pub fn serve_llm_gateway_http_accepted_stream_with_services_in_request(
    gateway: &GatewayRuntime,
    context: &GatewayRequestBudgetContext,
    bindings: GatewayHttpRequestBindings<'_, '_>,
    stream: &mut EntryAcceptedTcpStream,
) -> Result<()> {
    gateway.execute_with_request_context(context, || {
        serve_llm_gateway_http_stream_in_budget_lease(gateway, context, bindings, stream)
    })
}

fn serve_llm_gateway_http_stream_in_budget_lease(
    gateway: &GatewayRuntime,
    context: &GatewayRequestBudgetContext,
    bindings: GatewayHttpRequestBindings<'_, '_>,
    stream: &mut EntryAcceptedTcpStream,
) -> Result<()> {
    let config = gateway.config();
    let GatewayHttpRequestBindings {
        openai_upstream,
        ollama_upstream,
        services,
    } = bindings;
    let request = match read_http_gateway_request(stream, context, config) {
        Ok(request) => request,
        Err(error) => return write_error_response(stream, &error),
    };
    if request.path.starts_with("/v1/") {
        let request = match request.into_openai_request() {
            Ok(request) => request,
            Err(error) => return write_error_response(stream, &error),
        };
        return match handle_openai_request_with_services_in_budget_lease(
            gateway,
            context,
            config,
            request,
            openai_upstream,
            services,
        ) {
            Ok(response) => write_openai_http_response(stream, response, services),
            Err(error) => write_error_response(stream, &error),
        };
    }
    if request.path.starts_with("/api/") {
        let request = request.into_ollama_request();
        return match handle_ollama_request_with_services_in_budget_lease(
            gateway,
            context,
            config,
            request,
            ollama_upstream,
            services,
        ) {
            Ok(response) => write_ollama_http_response(stream, response, services),
            Err(error) => write_error_response(stream, &error),
        };
    }
    write_error_response(
        stream,
        &GatewayError::invalid_request("unsupported LLM gateway route"),
    )
}

pub fn serve_openai_http_accepted_stream(
    gateway: &GatewayRuntime,
    upstream: &mut dyn OpenAiCompatibleUpstream,
    stream: &mut EntryAcceptedTcpStream,
) -> Result<()> {
    let mut services = OpenAiGatewayServices::new();
    serve_openai_http_accepted_stream_with_services(gateway, upstream, &mut services, stream)
}

pub fn serve_openai_http_accepted_stream_with_services(
    gateway: &GatewayRuntime,
    upstream: &mut dyn OpenAiCompatibleUpstream,
    services: &mut OpenAiGatewayServices<'_>,
    stream: &mut EntryAcceptedTcpStream,
) -> Result<()> {
    let context = match gateway.begin_request() {
        Ok(context) => context,
        Err(error) => return write_error_response(stream, &error),
    };
    match gateway.execute_with_request_context(&context, || {
        serve_openai_http_stream_in_budget_lease(gateway, &context, upstream, services, stream)
    }) {
        Ok(()) => Ok(()),
        Err(error) => write_error_response(stream, &error),
    }
}

fn serve_openai_http_stream_in_budget_lease(
    gateway: &GatewayRuntime,
    context: &GatewayRequestBudgetContext,
    upstream: &mut dyn OpenAiCompatibleUpstream,
    services: &mut OpenAiGatewayServices<'_>,
    stream: &mut EntryAcceptedTcpStream,
) -> Result<()> {
    let config = gateway.config();
    let request = match read_http_gateway_request(stream, context, config)
        .and_then(|request| request.into_openai_request())
    {
        Ok(request) => request,
        Err(error) => return write_error_response(stream, &error),
    };
    match handle_openai_request_with_services_in_budget_lease(
        gateway, context, config, request, upstream, services,
    ) {
        Ok(response) => write_openai_http_response(stream, response, services),
        Err(error) => write_error_response(stream, &error),
    }
}

pub fn serve_ollama_http_accepted_stream(
    gateway: &GatewayRuntime,
    upstream: &mut dyn OllamaNativeUpstream,
    stream: &mut EntryAcceptedTcpStream,
) -> Result<()> {
    let mut services = OpenAiGatewayServices::new();
    serve_ollama_http_accepted_stream_with_services(gateway, upstream, &mut services, stream)
}

pub fn serve_ollama_http_accepted_stream_with_services(
    gateway: &GatewayRuntime,
    upstream: &mut dyn OllamaNativeUpstream,
    services: &mut OpenAiGatewayServices<'_>,
    stream: &mut EntryAcceptedTcpStream,
) -> Result<()> {
    let context = match gateway.begin_request() {
        Ok(context) => context,
        Err(error) => return write_error_response(stream, &error),
    };
    match gateway.execute_with_request_context(&context, || {
        serve_ollama_http_stream_in_budget_lease(gateway, &context, upstream, services, stream)
    }) {
        Ok(()) => Ok(()),
        Err(error) => write_error_response(stream, &error),
    }
}

fn serve_ollama_http_stream_in_budget_lease(
    gateway: &GatewayRuntime,
    context: &GatewayRequestBudgetContext,
    upstream: &mut dyn OllamaNativeUpstream,
    services: &mut OpenAiGatewayServices<'_>,
    stream: &mut EntryAcceptedTcpStream,
) -> Result<()> {
    let config = gateway.config();
    let request = match read_http_gateway_request(stream, context, config) {
        Ok(request) => request.into_ollama_request(),
        Err(error) => return write_error_response(stream, &error),
    };
    match handle_ollama_request_with_services_in_budget_lease(
        gateway, context, config, request, upstream, services,
    ) {
        Ok(response) => write_ollama_http_response(stream, response, services),
        Err(error) => write_error_response(stream, &error),
    }
}

#[derive(Clone, Copy)]
enum HttpGatewayMethod {
    Get,
    Post,
    Delete,
}

struct HttpGatewayRequest {
    method: HttpGatewayMethod,
    path: String,
    headers: BTreeMap<String, String>,
    body: Option<serde_json::Value>,
    scope: GatewayScopeRequest,
    provider_name: Option<String>,
}

impl HttpGatewayRequest {
    fn into_openai_request(self) -> Result<OpenAiGatewayRequest> {
        let method = match self.method {
            HttpGatewayMethod::Get => OpenAiGatewayMethod::Get,
            HttpGatewayMethod::Post => OpenAiGatewayMethod::Post,
            HttpGatewayMethod::Delete => {
                return Err(GatewayError::invalid_request(
                    "unsupported OpenAI HTTP method",
                ));
            }
        };
        Ok(OpenAiGatewayRequest {
            method,
            path: self.path,
            headers: self.headers,
            body: self.body,
            scope: self.scope,
            provider_name: self.provider_name,
            client_profile: "openai_http".to_string(),
        })
    }

    fn into_ollama_request(self) -> OllamaGatewayRequest {
        OllamaGatewayRequest {
            method: match self.method {
                HttpGatewayMethod::Get => OllamaGatewayMethod::Get,
                HttpGatewayMethod::Post => OllamaGatewayMethod::Post,
                HttpGatewayMethod::Delete => OllamaGatewayMethod::Delete,
            },
            path: self.path,
            headers: self.headers,
            body: self.body,
            scope: self.scope,
            provider_name: self.provider_name,
            client_profile: "ollama_http".to_string(),
        }
    }
}

fn read_http_gateway_request(
    stream: &mut EntryAcceptedTcpStream,
    context: &GatewayRequestBudgetContext,
    config: &GatewayConfig,
) -> Result<HttpGatewayRequest> {
    let adapter_budget = context.report().adapter_budget;
    let ingress = read_authorized_http_request(
        stream,
        EntryHttpIngressLimits::new(
            adapter_budget.http_header_max_bytes,
            adapter_budget.http_body_max_bytes,
        )
        .map_err(|error| GatewayError::invalid_request(error.to_string()))?,
        |accepted, head| {
            let auth = config.entry.auth.authenticate_accepted_tcp_stream(
                accepted,
                head.header("authorization"),
                "llm-gateway-loopback",
            );
            let mut required = vec![EntryOperationCapability::LlmGatewayProtocol];
            required.extend(required_route_capabilities(head.method(), head.target()));
            let authorization = EntryHttpAuthorization::require_all(auth, required)?;
            if let Some(origin) = head.header("origin") {
                if !config
                    .server
                    .allowed_origins
                    .iter()
                    .any(|allowed| allowed == origin)
                {
                    return Err(bm_entry::EntryHttpIngressError::forbidden(
                        "gateway Origin is not allowed",
                    ));
                }
            }
            Ok(authorization)
        },
    )
    .map_err(|error| {
        if matches!(
            error.kind(),
            EntryHttpIngressErrorKind::Forbidden | EntryHttpIngressErrorKind::Unauthorized
        ) {
            let _ = stream.shutdown(std::net::Shutdown::Read);
        }
        match error.kind() {
            EntryHttpIngressErrorKind::Forbidden => {
                let message = error.required_capability().map_or_else(
                    || error.to_string(),
                    |capability| {
                        format!(
                            "gateway principal lacks required capability: {}",
                            capability.as_str()
                        )
                    },
                );
                GatewayError::forbidden(message)
            }
            EntryHttpIngressErrorKind::Unauthorized => GatewayError::unauthorized(format!(
                "gateway auth rejected request: {}",
                error.message()
            )),
            _ if error.message() == "invalid HTTP method" => {
                GatewayError::invalid_request("unsupported HTTP method")
            }
            _ if error.message() == "missing HTTP request target" => {
                GatewayError::invalid_request("missing HTTP path")
            }
            _ => GatewayError::invalid_request(error.to_string()),
        }
    })?;
    let (head, body_bytes, auth) = ingress.into_parts();
    let method = match head.method() {
        "GET" => HttpGatewayMethod::Get,
        "POST" => HttpGatewayMethod::Post,
        "DELETE" => HttpGatewayMethod::Delete,
        _ => return Err(GatewayError::invalid_request("unsupported HTTP method")),
    };
    let path = head.target().to_string();
    let headers = head.headers().clone();
    let content_length = headers.get("content-length").map(|_| head.content_length());
    match method {
        HttpGatewayMethod::Post => content_length
            .ok_or_else(|| GatewayError::invalid_request("POST requires HTTP content-length"))?,
        HttpGatewayMethod::Get | HttpGatewayMethod::Delete => {
            let content_length = content_length.unwrap_or(0);
            if content_length != 0 {
                return Err(GatewayError::invalid_request(
                    "GET and DELETE require zero HTTP content-length",
                ));
            }
            content_length
        }
    };
    if matches!(method, HttpGatewayMethod::Post)
        && headers.get("content-type").map(String::as_str) != Some("application/json")
    {
        return Err(GatewayError::invalid_request(
            "POST requires content-type application/json",
        ));
    }
    let body = if body_bytes.is_empty() {
        None
    } else {
        Some(
            serde_json::from_slice(&body_bytes)
                .map_err(|error| GatewayError::invalid_request(error.to_string()))?,
        )
    };
    let scope = GatewayScopeRequest {
        auth,
        headers: headers.clone(),
        workspace_root_digest: headers.get("x-bm-workspace-digest").cloned(),
        workspace_root_path: None,
        client_conversation_hint: headers.get("x-bm-conversation-id").cloned(),
        request_id_hint: headers.get("x-request-id").cloned(),
        body_conversation_hint: body.as_ref().and_then(extract_body_conversation_hint),
        model_alias: None,
    };
    let provider_name = headers.get("x-bm-provider").cloned();
    Ok(HttpGatewayRequest {
        method,
        path,
        headers,
        body,
        scope,
        provider_name,
    })
}

fn required_route_capabilities(method: &str, path: &str) -> &'static [EntryOperationCapability] {
    if path.starts_with("/v1/") {
        let method = match method {
            "GET" => OpenAiGatewayMethod::Get,
            "POST" => OpenAiGatewayMethod::Post,
            _ => return &[],
        };
        return required_openai_capabilities(method, path);
    }
    let method = match method {
        "GET" => OllamaGatewayMethod::Get,
        "POST" => OllamaGatewayMethod::Post,
        "DELETE" => OllamaGatewayMethod::Delete,
        _ => return &[],
    };
    required_ollama_capabilities(method, path)
}

fn extract_body_conversation_hint(body: &serde_json::Value) -> Option<String> {
    let object = body.as_object()?;
    [
        "conversation_id",
        "conversationId",
        "chat_id",
        "chatId",
        "session_id",
        "sessionId",
    ]
    .into_iter()
    .find_map(|key| {
        object
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn write_openai_http_response(
    stream: &mut impl Write,
    mut response: crate::OpenAiGatewayResponse,
    services: &mut OpenAiGatewayServices<'_>,
) -> Result<()> {
    match &mut response.body {
        OpenAiGatewayBody::Json(body) => {
            let body = body.to_string();
            let response = format!(
                "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.status_code,
                reason_phrase(response.status_code),
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            stream
                .flush()
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
        }
        OpenAiGatewayBody::Sse(body) => {
            let headers = format!(
                "HTTP/1.1 {} {}\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n",
                response.status_code,
                reason_phrase(response.status_code)
            );
            stream
                .write_all(headers.as_bytes())
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            while let Some(chunk) = body.next_chunk()? {
                stream
                    .write_all(chunk.as_bytes())
                    .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
                stream
                    .flush()
                    .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            }
            response.finish_deferred_maintenance(services);
            Ok(())
        }
    }
}

fn write_ollama_http_response(
    stream: &mut impl Write,
    mut response: crate::OllamaGatewayResponse,
    services: &mut OpenAiGatewayServices<'_>,
) -> Result<()> {
    match &mut response.body {
        OllamaGatewayBody::Json(body) => {
            let body = body.to_string();
            let response = format!(
                "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.status_code,
                reason_phrase(response.status_code),
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            stream
                .flush()
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
        }
        OllamaGatewayBody::Ndjson(body) => {
            let headers = format!(
                "HTTP/1.1 {} {}\r\ncontent-type: application/x-ndjson\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n",
                response.status_code,
                reason_phrase(response.status_code)
            );
            stream
                .write_all(headers.as_bytes())
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            while let Some(chunk) = body.next_chunk()? {
                stream
                    .write_all(chunk.as_bytes())
                    .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
                stream
                    .flush()
                    .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            }
            response.finish_deferred_maintenance(services);
            Ok(())
        }
    }
}

fn write_error_response(stream: &mut impl Write, error: &GatewayError) -> Result<()> {
    let status_code = error_status_code(error);
    let body = json!({
        "error": {
            "type": error.key().as_str(),
            "message": error.message(),
        }
    })
    .to_string();
    let response = format!(
        "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        status_code,
        reason_phrase(status_code),
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
    stream
        .flush()
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
}

fn error_status_code(error: &GatewayError) -> u16 {
    if error.key() == GatewayErrorKey::InvalidRequest
        && error.message().contains("runtime adapter budget")
    {
        return 413;
    }
    match error.key() {
        GatewayErrorKey::InvalidConfig => 500,
        GatewayErrorKey::CapacityExceeded => 503,
        GatewayErrorKey::InvalidRequest => 400,
        GatewayErrorKey::Unauthorized => 401,
        GatewayErrorKey::Forbidden => 403,
        GatewayErrorKey::ProviderUnavailable | GatewayErrorKey::UpstreamUnavailable => 502,
        GatewayErrorKey::ScopeResolutionFailed
        | GatewayErrorKey::ProjectionFailed
        | GatewayErrorKey::RuntimeUnavailable => 422,
    }
}

fn reason_phrase(status_code: u16) -> &'static str {
    match status_code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    }
}
