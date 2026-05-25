use std::collections::BTreeMap;
use std::io::{Read, Write};

use bm_entry::EntryAuthDecision;
use bm_sdk::RuntimeBudgetReport;
use serde_json::json;

use crate::{
    handle_ollama_request_with_services, handle_openai_request_with_services, GatewayConfig,
    GatewayError, GatewayErrorKey, GatewayRuntime, GatewayScopeRequest, OllamaGatewayBody,
    OllamaGatewayMethod, OllamaGatewayRequest, OllamaNativeUpstream, OpenAiCompatibleUpstream,
    OpenAiGatewayBody, OpenAiGatewayMethod, OpenAiGatewayRequest, OpenAiGatewayServices, Result,
};

pub fn serve_llm_gateway_http_stream<S: Read + Write>(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    openai_upstream: &mut dyn OpenAiCompatibleUpstream,
    ollama_upstream: &mut dyn OllamaNativeUpstream,
    stream: &mut S,
) -> Result<()> {
    let mut services = OpenAiGatewayServices::new();
    serve_llm_gateway_http_stream_with_services(
        gateway,
        config,
        openai_upstream,
        ollama_upstream,
        &mut services,
        stream,
    )
}

pub fn serve_llm_gateway_http_stream_with_services<S: Read + Write>(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    openai_upstream: &mut dyn OpenAiCompatibleUpstream,
    ollama_upstream: &mut dyn OllamaNativeUpstream,
    services: &mut OpenAiGatewayServices<'_>,
    stream: &mut S,
) -> Result<()> {
    let request = match read_http_gateway_request(stream, config) {
        Ok(request) => request,
        Err(error) => return write_error_response(stream, &error),
    };
    if request.path.starts_with("/v1/") {
        let request = match request.into_openai_request() {
            Ok(request) => request,
            Err(error) => return write_error_response(stream, &error),
        };
        return match handle_openai_request_with_services(
            gateway,
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
        return match handle_ollama_request_with_services(
            gateway,
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

pub fn serve_openai_http_stream<S: Read + Write>(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    upstream: &mut dyn OpenAiCompatibleUpstream,
    stream: &mut S,
) -> Result<()> {
    let mut services = OpenAiGatewayServices::new();
    serve_openai_http_stream_with_services(gateway, config, upstream, &mut services, stream)
}

pub fn serve_openai_http_stream_with_services<S: Read + Write>(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    upstream: &mut dyn OpenAiCompatibleUpstream,
    services: &mut OpenAiGatewayServices<'_>,
    stream: &mut S,
) -> Result<()> {
    let request = match read_http_gateway_request(stream, config)
        .and_then(|request| request.into_openai_request())
    {
        Ok(request) => request,
        Err(error) => return write_error_response(stream, &error),
    };
    match handle_openai_request_with_services(gateway, config, request, upstream, services) {
        Ok(response) => write_openai_http_response(stream, response, services),
        Err(error) => write_error_response(stream, &error),
    }
}

pub fn serve_ollama_http_stream<S: Read + Write>(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    upstream: &mut dyn OllamaNativeUpstream,
    stream: &mut S,
) -> Result<()> {
    let mut services = OpenAiGatewayServices::new();
    serve_ollama_http_stream_with_services(gateway, config, upstream, &mut services, stream)
}

pub fn serve_ollama_http_stream_with_services<S: Read + Write>(
    gateway: &GatewayRuntime,
    config: &GatewayConfig,
    upstream: &mut dyn OllamaNativeUpstream,
    services: &mut OpenAiGatewayServices<'_>,
    stream: &mut S,
) -> Result<()> {
    let request = match read_http_gateway_request(stream, config) {
        Ok(request) => request.into_ollama_request(),
        Err(error) => return write_error_response(stream, &error),
    };
    match handle_ollama_request_with_services(gateway, config, request, upstream, services) {
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
    stream: &mut impl Read,
    config: &GatewayConfig,
) -> Result<HttpGatewayRequest> {
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    while !buffer.ends_with(b"\r\n\r\n") {
        let read = stream
            .read(&mut byte)
            .map_err(|error| GatewayError::invalid_request(error.to_string()))?;
        if read == 0 {
            return Err(GatewayError::invalid_request(
                "unexpected EOF while reading HTTP headers",
            ));
        }
        buffer.push(byte[0]);
        if buffer.len() > 64 * 1024 {
            return Err(GatewayError::invalid_request("HTTP headers are too large"));
        }
    }

    let header_text = std::str::from_utf8(&buffer)
        .map_err(|error| GatewayError::invalid_request(error.to_string()))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| GatewayError::invalid_request("missing HTTP request line"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = match request_parts.next() {
        Some("GET") => HttpGatewayMethod::Get,
        Some("POST") => HttpGatewayMethod::Post,
        Some("DELETE") => HttpGatewayMethod::Delete,
        _ => return Err(GatewayError::invalid_request("unsupported HTTP method")),
    };
    let path = request_parts
        .next()
        .ok_or_else(|| GatewayError::invalid_request("missing HTTP path"))?
        .to_string();
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let content_length = headers
        .get("content-length")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|error| GatewayError::invalid_request(error.to_string()))
        })
        .transpose()?
        .unwrap_or(0);
    let body_budget = RuntimeBudgetReport::static_for_profile(config.entry.profile)
        .adapter_budget
        .http_body_max_bytes;
    if content_length > body_budget {
        return Err(GatewayError::invalid_request(
            "HTTP body exceeds runtime adapter budget",
        ));
    }
    let mut body_bytes = vec![0u8; content_length];
    if content_length > 0 {
        stream
            .read_exact(&mut body_bytes)
            .map_err(|error| GatewayError::invalid_request(error.to_string()))?;
    }
    let body = if body_bytes.is_empty() {
        None
    } else {
        Some(
            serde_json::from_slice(&body_bytes)
                .map_err(|error| GatewayError::invalid_request(error.to_string()))?,
        )
    };
    let auth = if config.server.loopback_only {
        EntryAuthDecision::loopback("llm-gateway-loopback")
    } else {
        EntryAuthDecision::remote_bearer(
            &config.entry.auth,
            headers.get("authorization").map(String::as_str),
            headers.get("x-bm-auth-subject").map(String::as_str),
        )
    };
    if !auth.authenticated {
        return Err(GatewayError::invalid_request(format!(
            "gateway auth rejected request: {}",
            auth.rejection_reason
                .as_deref()
                .unwrap_or("unauthenticated")
        )));
    }
    let scope = GatewayScopeRequest {
        auth_subject: auth.auth_subject.clone(),
        headers: headers.clone(),
        workspace_root_digest: headers.get("x-bm-workspace-digest").cloned(),
        client_conversation_hint: headers.get("x-bm-conversation-id").cloned(),
        request_id_hint: headers.get("x-request-id").cloned(),
        body_conversation_hint: body.as_ref().and_then(extract_body_conversation_hint),
        ..GatewayScopeRequest::default()
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
            write!(
                stream,
                "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.status_code,
                reason_phrase(response.status_code),
                body.len(),
                body
            )
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            stream
                .flush()
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
        }
        OpenAiGatewayBody::Sse(body) => {
            write!(
                stream,
                "HTTP/1.1 {} {}\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n",
                response.status_code,
                reason_phrase(response.status_code)
            )
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
            write!(
                stream,
                "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.status_code,
                reason_phrase(response.status_code),
                body.len(),
                body
            )
            .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            stream
                .flush()
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
        }
        OllamaGatewayBody::Ndjson(body) => {
            write!(
                stream,
                "HTTP/1.1 {} {}\r\ncontent-type: application/x-ndjson\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n",
                response.status_code,
                reason_phrase(response.status_code)
            )
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
            "type": format!("{:?}", error.key()),
            "message": error.message(),
        }
    })
    .to_string();
    write!(
        stream,
        "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        status_code,
        reason_phrase(status_code),
        body.len(),
        body
    )
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
        GatewayErrorKey::InvalidRequest => 400,
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
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "OK",
    }
}
