use std::collections::BTreeMap;
use std::io::{Read, Write};

use serde_json::json;

use crate::{
    handle_openai_request_with_services, GatewayConfig, GatewayError, GatewayErrorKey,
    GatewayRuntime, GatewayScopeRequest, OpenAiCompatibleUpstream, OpenAiGatewayBody,
    OpenAiGatewayMethod, OpenAiGatewayRequest, OpenAiGatewayServices, Result,
};

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
    let request = read_openai_http_request(stream)?;
    match handle_openai_request_with_services(gateway, config, request, upstream, services) {
        Ok(response) => write_openai_http_response(stream, response, services),
        Err(error) => write_openai_error_response(stream, &error),
    }
}

fn read_openai_http_request(stream: &mut impl Read) -> Result<OpenAiGatewayRequest> {
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
        Some("GET") => OpenAiGatewayMethod::Get,
        Some("POST") => OpenAiGatewayMethod::Post,
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
    let scope = GatewayScopeRequest {
        headers: headers.clone(),
        workspace_root_digest: headers.get("x-bm-workspace-digest").cloned(),
        client_conversation_hint: headers
            .get("x-bm-conversation-id")
            .or_else(|| headers.get("x-request-id"))
            .cloned(),
        ..GatewayScopeRequest::default()
    };
    Ok(OpenAiGatewayRequest {
        method,
        path,
        headers,
        body,
        scope,
        provider_name: None,
        client_profile: "openai_http".to_string(),
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
                "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
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
                "HTTP/1.1 {} {}\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: keep-alive\r\n\r\n",
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

fn write_openai_error_response(stream: &mut impl Write, error: &GatewayError) -> Result<()> {
    let status_code = error_status_code(error.key());
    let body = json!({
        "error": {
            "type": format!("{:?}", error.key()),
            "message": error.message(),
        }
    })
    .to_string();
    write!(
        stream,
        "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
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

fn error_status_code(key: GatewayErrorKey) -> u16 {
    match key {
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
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "OK",
    }
}
