//! HTTP adapter contracts for Beetle Memory.

use bm_adapter::{AdapterErrorKey, AdapterOperation, TransportKind};

#[cfg(feature = "server-std")]
use bm_adapter::{
    decode_json_adapter_command, AdapterJsonCommandOptions, AdapterResponse,
    AdapterRuntimeServices, AdapterSdkReport, TransportMode,
};
#[cfg(feature = "server-std")]
use bm_entry::{
    EntryAuthDecision, EntryConsoleDeviceCreate, EntryConsoleDeviceUpdate,
    EntryConsoleSkillSetEnabled, EntryConsoleSkillUpsert, EntryConsoleTransportUpdate,
    EntryRuntime, EntryTransportContext,
};
#[cfg(feature = "server-std")]
use serde_json::json;
#[cfg(feature = "server-std")]
use std::collections::BTreeMap;
#[cfg(feature = "server-std")]
use std::io::{Read, Write};
#[cfg(feature = "server-std")]
use std::net::TcpListener;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Patch,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteBodyMode {
    None,
    Json { max_bytes: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteAuth {
    TokenOrLoopback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteSpec {
    pub method: HttpMethod,
    pub path: &'static str,
    pub transport: TransportKind,
    pub operation: AdapterOperation,
    pub body: RouteBodyMode,
    pub auth: RouteAuth,
    pub profile_gate_required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleRouteSpec {
    pub method: HttpMethod,
    pub path: &'static str,
    pub auth: RouteAuth,
}

const JSON_BODY_MAX_BYTES: usize = 64 * 1024;

const ROUTES: &[RouteSpec] = &[
    RouteSpec {
        method: HttpMethod::Get,
        path: "/memory/profile/capabilities",
        transport: TransportKind::Http,
        operation: AdapterOperation::Capabilities,
        body: RouteBodyMode::None,
        auth: RouteAuth::TokenOrLoopback,
        profile_gate_required: true,
    },
    memory_post("/memory/write", AdapterOperation::Write),
    memory_post("/memory/recall", AdapterOperation::Recall),
    memory_post("/memory/project", AdapterOperation::Project),
    memory_post("/memory/maintain", AdapterOperation::Maintain),
    memory_post("/memory/inspect", AdapterOperation::Inspect),
    memory_post("/memory/recover", AdapterOperation::Recover),
    memory_post("/memory/replay", AdapterOperation::Replay),
    memory_post("/memory/export", AdapterOperation::Export),
    memory_post("/memory/import", AdapterOperation::Import),
];

const CONSOLE_ROUTES: &[ConsoleRouteSpec] = &[
    console_get("/console/overview"),
    console_get("/console/skills"),
    console_get("/console/skills/{name}"),
    console_post("/console/skills"),
    console_patch("/console/skills/{name}"),
    console_patch("/console/skills/{name}/enabled"),
    console_delete("/console/skills/{name}"),
    console_get("/console/transports"),
    console_patch("/console/transports/{id}"),
    console_get("/console/devices"),
    console_post("/console/devices"),
    console_patch("/console/devices/{id}"),
    console_post("/console/devices/{id}/rotate-key"),
    console_get("/console/session"),
];

const fn memory_post(path: &'static str, operation: AdapterOperation) -> RouteSpec {
    RouteSpec {
        method: HttpMethod::Post,
        path,
        transport: TransportKind::Http,
        operation,
        body: RouteBodyMode::Json {
            max_bytes: JSON_BODY_MAX_BYTES,
        },
        auth: RouteAuth::TokenOrLoopback,
        profile_gate_required: true,
    }
}

pub const fn route_specs() -> &'static [RouteSpec] {
    ROUTES
}

const fn console_get(path: &'static str) -> ConsoleRouteSpec {
    ConsoleRouteSpec {
        method: HttpMethod::Get,
        path,
        auth: RouteAuth::TokenOrLoopback,
    }
}

const fn console_post(path: &'static str) -> ConsoleRouteSpec {
    ConsoleRouteSpec {
        method: HttpMethod::Post,
        path,
        auth: RouteAuth::TokenOrLoopback,
    }
}

const fn console_patch(path: &'static str) -> ConsoleRouteSpec {
    ConsoleRouteSpec {
        method: HttpMethod::Patch,
        path,
        auth: RouteAuth::TokenOrLoopback,
    }
}

const fn console_delete(path: &'static str) -> ConsoleRouteSpec {
    ConsoleRouteSpec {
        method: HttpMethod::Delete,
        path,
        auth: RouteAuth::TokenOrLoopback,
    }
}

pub const fn console_route_specs() -> &'static [ConsoleRouteSpec] {
    CONSOLE_ROUTES
}

pub const fn invalid_json_error() -> AdapterErrorKey {
    AdapterErrorKey::InvalidJson
}

pub const fn unauthorized_error() -> AdapterErrorKey {
    AdapterErrorKey::Unauthorized
}

pub const fn duplicate_idempotency_error() -> AdapterErrorKey {
    AdapterErrorKey::Duplicated
}

pub const fn payload_too_large_error() -> AdapterErrorKey {
    AdapterErrorKey::PayloadTooLarge
}

#[cfg(feature = "server-std")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRuntimeRequest {
    pub method: HttpMethod,
    pub path: String,
    pub body: String,
    pub request_id: String,
    pub idempotency_key: String,
    pub audit_id: String,
    pub authenticated: bool,
}

#[cfg(feature = "server-std")]
impl HttpRuntimeRequest {
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            path: path.into(),
            body: String::new(),
            request_id: "http-req".to_string(),
            idempotency_key: "http-idem".to_string(),
            audit_id: "http-audit".to_string(),
            authenticated: true,
        }
    }

    pub fn post_json(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Post,
            path: path.into(),
            body: body.into(),
            request_id: "http-req".to_string(),
            idempotency_key: format!("http-idem-{}", unique_request_suffix()),
            audit_id: "http-audit".to_string(),
            authenticated: true,
        }
    }

    pub fn patch_json(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Patch,
            path: path.into(),
            body: body.into(),
            request_id: "http-req".to_string(),
            idempotency_key: format!("http-idem-{}", unique_request_suffix()),
            audit_id: "http-audit".to_string(),
            authenticated: true,
        }
    }

    pub fn delete(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Delete,
            path: path.into(),
            body: String::new(),
            request_id: "http-req".to_string(),
            idempotency_key: format!("http-idem-{}", unique_request_suffix()),
            audit_id: "http-audit".to_string(),
            authenticated: true,
        }
    }
}

#[cfg(feature = "server-std")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRuntimeResponse {
    pub status_code: u16,
    pub body: String,
}

#[cfg(feature = "server-std")]
pub fn handle_http_request(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    handle_http_request_with_services(runtime, request, AdapterRuntimeServices::none())
}

#[cfg(feature = "server-std")]
pub fn handle_http_request_with_services(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
    services: AdapterRuntimeServices<'_>,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    if request.path.starts_with("/console/") {
        return handle_console_request(runtime, request);
    }
    let route = route_specs()
        .iter()
        .find(|route| route.method == request.method && route.path == request.path)
        .copied()
        .ok_or_else(|| bm_sdk::Error::config("http_runtime", "unknown route"))?;
    let command = decode_json_adapter_command(
        route.operation,
        &request.body,
        &AdapterJsonCommandOptions::new("bm-http").with_default_source_chat_id("chat-1"),
    )?;
    let response = runtime.handle_with_services(
        EntryTransportContext {
            request_id: request.request_id,
            transport: route.transport,
            mode: TransportMode::Server,
            operation: route.operation,
            source_id: "http-runtime".to_string(),
            source_kind: "http_client".to_string(),
            idempotency_key: request.idempotency_key,
            audit_id: request.audit_id,
            auth: if request.authenticated {
                EntryAuthDecision::authenticated("token_or_loopback", "http-client")
            } else {
                EntryAuthDecision::unauthenticated("token_or_loopback")
            },
        },
        command,
        services,
    )?;
    Ok(render_http_response(response.adapter))
}

#[cfg(feature = "server-std")]
pub fn serve_http_listener_once(
    runtime: &EntryRuntime,
    listener: &TcpListener,
) -> bm_sdk::Result<()> {
    let (mut stream, _) = listener
        .accept()
        .map_err(|err| bm_sdk::Error::config("http_listener_accept", err.to_string()))?;
    serve_http_stream(runtime, &mut stream)
}

#[cfg(feature = "server-std")]
pub fn serve_http_stream<S: Read + Write>(
    runtime: &EntryRuntime,
    stream: &mut S,
) -> bm_sdk::Result<()> {
    let request = read_http_runtime_request(stream)?;
    let response = handle_http_request(runtime, request)?;
    write_http_response(stream, response)
}

#[cfg(feature = "server-std")]
fn read_http_runtime_request<S: Read>(stream: &mut S) -> bm_sdk::Result<HttpRuntimeRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|err| bm_sdk::Error::config("http_read", err.to_string()))?;
        if read == 0 {
            break find_header_end(&buffer)
                .ok_or_else(|| bm_sdk::Error::config("http_read", "missing HTTP headers"))?;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(pos) = find_header_end(&buffer) {
            break pos;
        }
        if buffer.len() > JSON_BODY_MAX_BYTES + 8192 {
            return Err(bm_sdk::Error::config("http_read", "HTTP request too large"));
        }
    };

    let header_bytes = &buffer[..header_end];
    let header_text = std::str::from_utf8(header_bytes)
        .map_err(|err| bm_sdk::Error::config("http_headers", err.to_string()))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| bm_sdk::Error::config("http_headers", "missing request line"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = match request_parts.next() {
        Some("GET") => HttpMethod::Get,
        Some("POST") => HttpMethod::Post,
        Some("PATCH") => HttpMethod::Patch,
        Some("DELETE") => HttpMethod::Delete,
        Some(other) => {
            return Err(bm_sdk::Error::config(
                "http_headers",
                format!("unsupported HTTP method: {other}"),
            ))
        }
        None => return Err(bm_sdk::Error::config("http_headers", "missing method")),
    };
    let path = request_parts
        .next()
        .ok_or_else(|| bm_sdk::Error::config("http_headers", "missing path"))?
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
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > JSON_BODY_MAX_BYTES {
        return Err(bm_sdk::Error::config(
            "http_body",
            "HTTP body exceeds configured budget",
        ));
    }

    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = stream
            .read(&mut chunk)
            .map_err(|err| bm_sdk::Error::config("http_body", err.to_string()))?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    if buffer.len() < body_start + content_length {
        return Err(bm_sdk::Error::config("http_body", "truncated HTTP body"));
    }
    let body = String::from_utf8(buffer[body_start..body_start + content_length].to_vec())
        .map_err(|err| bm_sdk::Error::config("http_body", err.to_string()))?;

    Ok(HttpRuntimeRequest {
        method,
        path,
        body,
        request_id: header_or_default(&headers, "x-request-id", "http-req"),
        idempotency_key: header_or_default(&headers, "x-idempotency-key", "http-idem"),
        audit_id: header_or_default(&headers, "x-audit-id", "http-audit"),
        authenticated: headers.contains_key("authorization")
            || headers
                .get("x-loopback")
                .is_some_and(|value| value == "true" || value == "1"),
    })
}

#[cfg(feature = "server-std")]
fn write_http_response(
    stream: &mut impl Write,
    response: HttpRuntimeResponse,
) -> bm_sdk::Result<()> {
    let reason = match response.status_code {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        409 => "Conflict",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        _ => "OK",
    };
    let body = response.body;
    let head = format!(
        "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        response.status_code,
        reason,
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body.as_bytes()))
        .and_then(|_| stream.flush())
        .map_err(|err| bm_sdk::Error::config("http_write", err.to_string()))
}

#[cfg(feature = "server-std")]
fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

#[cfg(feature = "server-std")]
fn header_or_default(headers: &BTreeMap<String, String>, name: &str, default: &str) -> String {
    headers
        .get(name)
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

#[cfg(feature = "server-std")]
fn handle_console_request(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    if !request.authenticated {
        return Ok(json_response(
            401,
            json!({
                "status": "rejected",
                "errorKey": "Unauthorized",
                "reason": "console auth rejected request",
            }),
        ));
    }

    let (route_path, query_string) = split_query_path(&request.path);
    match (request.method, route_path) {
        (HttpMethod::Get, "/console/overview") => Ok(json_response(
            200,
            json!({
                "status": "accepted",
                "overview": runtime.console_overview(),
            }),
        )),
        (HttpMethod::Get, "/console/transports") => Ok(json_response(
            200,
            json!({
                "status": "accepted",
                "transports": runtime.console_transports(),
            }),
        )),
        (HttpMethod::Get, "/console/devices") => Ok(json_response(
            200,
            json!({
                "status": "accepted",
                "devices": runtime.console_devices(),
            }),
        )),
        (HttpMethod::Get, "/console/session") => Ok(json_response(
            200,
            json!({
                "status": "accepted",
                "session": runtime.console_session(),
            }),
        )),
        (HttpMethod::Get, "/console/skills") => {
            let skills = runtime.console_skills(query_param(query_string, "query"))?;
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "skills": skills,
                }),
            ))
        }
        (HttpMethod::Get, path) if path.starts_with("/console/skills/") => {
            let name = trim_suffix_path(path, "/console/skills/");
            if name.is_empty() {
                return Ok(not_found("console skill not found"));
            }
            match runtime.console_skill_detail(name)? {
                Some(skill) => Ok(json_response(
                    200,
                    json!({
                        "status": "accepted",
                        "skill": skill,
                    }),
                )),
                None => Ok(not_found("console skill not found")),
            }
        }
        (HttpMethod::Post, "/console/skills") => {
            let payload: EntryConsoleSkillUpsert = parse_console_json(&request.body)?;
            let mutation = runtime.console_upsert_skill(payload)?;
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "mutation": mutation,
                }),
            ))
        }
        (HttpMethod::Patch, path)
            if path.starts_with("/console/skills/") && path.ends_with("/enabled") =>
        {
            let name = path
                .strip_prefix("/console/skills/")
                .and_then(|value| value.strip_suffix("/enabled"))
                .map(|value| value.trim_matches('/'))
                .unwrap_or_default();
            if name.is_empty() {
                return Ok(not_found("console skill not found"));
            }
            let payload: EntryConsoleSkillSetEnabled = parse_console_json(&request.body)?;
            match runtime.console_set_skill_enabled(name, payload)? {
                Some(mutation) => Ok(json_response(
                    200,
                    json!({
                        "status": "accepted",
                        "mutation": mutation,
                    }),
                )),
                None => Ok(not_found("console skill not found")),
            }
        }
        (HttpMethod::Patch, path) if path.starts_with("/console/skills/") => {
            let name = trim_suffix_path(path, "/console/skills/");
            if name.is_empty() {
                return Ok(not_found("console skill not found"));
            }
            let mut payload: EntryConsoleSkillUpsert = parse_console_json(&request.body)?;
            payload.name = Some(name.to_string());
            let mutation = runtime.console_upsert_skill(payload)?;
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "mutation": mutation,
                }),
            ))
        }
        (HttpMethod::Delete, path) if path.starts_with("/console/skills/") => {
            let name = trim_suffix_path(path, "/console/skills/");
            if name.is_empty() {
                return Ok(not_found("console skill not found"));
            }
            match runtime.console_delete_skill(name)? {
                Some(mutation) => Ok(json_response(
                    200,
                    json!({
                        "status": "accepted",
                        "mutation": mutation,
                    }),
                )),
                None => Ok(not_found("console skill not found")),
            }
        }
        (HttpMethod::Post, "/console/devices") => {
            let payload: EntryConsoleDeviceCreate = parse_console_json(&request.body)?;
            match runtime.console_add_device(payload) {
                Ok(report) => Ok(json_response(
                    200,
                    json!({
                        "status": "accepted",
                        "device": report.device,
                        "appKeyOnce": report.app_key_once,
                    }),
                )),
                Err(reason) => Ok(json_response(
                    422,
                    json!({
                        "status": "rejected",
                        "errorKey": "RuntimeRejected",
                        "reason": reason,
                    }),
                )),
            }
        }
        (HttpMethod::Patch, path) if path.starts_with("/console/transports/") => {
            let id = trim_suffix_path(path, "/console/transports/");
            let payload: EntryConsoleTransportUpdate = parse_console_json(&request.body)?;
            match runtime.console_update_transport(id, payload) {
                Some(transport) => Ok(json_response(
                    200,
                    json!({
                        "status": "accepted",
                        "transport": transport,
                    }),
                )),
                None => Ok(not_found("console transport not found")),
            }
        }
        (HttpMethod::Patch, path) if path.starts_with("/console/devices/") => {
            let device_id = trim_suffix_path(path, "/console/devices/");
            let payload: EntryConsoleDeviceUpdate = parse_console_json(&request.body)?;
            match runtime.console_update_device(device_id, payload) {
                Some(device) => Ok(json_response(
                    200,
                    json!({
                        "status": "accepted",
                        "device": device,
                    }),
                )),
                None => Ok(not_found("console device not found")),
            }
        }
        (HttpMethod::Post, path)
            if path.starts_with("/console/devices/") && path.ends_with("/rotate-key") =>
        {
            let device_id = path
                .strip_prefix("/console/devices/")
                .and_then(|value| value.strip_suffix("/rotate-key"))
                .map(|value| value.trim_matches('/'))
                .unwrap_or_default();
            match runtime.console_rotate_device_key(device_id) {
                Some(report) => Ok(json_response(
                    200,
                    json!({
                        "status": "accepted",
                        "device": report.device,
                        "appKeyOnce": report.app_key_once,
                    }),
                )),
                None => Ok(not_found("console device not found")),
            }
        }
        _ if is_known_console_path(route_path) => Ok(json_response(
            405,
            json!({
                "status": "rejected",
                "errorKey": "UnsupportedOperation",
                "reason": "console method is not allowed for route",
            }),
        )),
        _ => Ok(not_found("console route not found")),
    }
}

#[cfg(feature = "server-std")]
fn parse_console_json<T: serde::de::DeserializeOwned>(body: &str) -> bm_sdk::Result<T> {
    serde_json::from_str(body)
        .map_err(|error| bm_sdk::Error::config("console_json", error.to_string()))
}

#[cfg(feature = "server-std")]
fn trim_suffix_path<'a>(path: &'a str, prefix: &str) -> &'a str {
    path.strip_prefix(prefix).unwrap_or(path).trim_matches('/')
}

#[cfg(feature = "server-std")]
fn split_query_path(path: &str) -> (&str, Option<&str>) {
    path.split_once('?')
        .map(|(route, query)| (route, Some(query)))
        .unwrap_or((path, None))
}

#[cfg(feature = "server-std")]
fn query_param(query_string: Option<&str>, key: &str) -> Option<String> {
    let query_string = query_string?;
    query_string.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (name == key && !value.trim().is_empty()).then(|| value.replace('+', " "))
    })
}

#[cfg(feature = "server-std")]
fn is_known_console_path(path: &str) -> bool {
    matches!(
        path,
        "/console/overview"
            | "/console/skills"
            | "/console/transports"
            | "/console/devices"
            | "/console/session"
    ) || path.starts_with("/console/skills/")
        || path.starts_with("/console/transports/")
        || path.starts_with("/console/devices/")
}

#[cfg(feature = "server-std")]
fn not_found(reason: &str) -> HttpRuntimeResponse {
    json_response(
        404,
        json!({
            "status": "rejected",
            "errorKey": "NotFound",
            "reason": reason,
        }),
    )
}

#[cfg(feature = "server-std")]
fn json_response(status_code: u16, body: serde_json::Value) -> HttpRuntimeResponse {
    HttpRuntimeResponse {
        status_code,
        body: body.to_string(),
    }
}

#[cfg(feature = "server-std")]
fn render_http_response(response: AdapterResponse<AdapterSdkReport>) -> HttpRuntimeResponse {
    match response {
        AdapterResponse::Accepted { report, .. } => HttpRuntimeResponse {
            status_code: 200,
            body: render_report(report),
        },
        AdapterResponse::Rejected {
            error_key, reason, ..
        } => HttpRuntimeResponse {
            status_code: match error_key {
                AdapterErrorKey::Unauthorized => 401,
                AdapterErrorKey::PayloadTooLarge => 413,
                AdapterErrorKey::InvalidJson => 400,
                AdapterErrorKey::Duplicated => 409,
                AdapterErrorKey::OperationMismatch
                | AdapterErrorKey::UnsupportedOperation
                | AdapterErrorKey::RuntimeRejected => 422,
            },
            body: json!({
                "status": "rejected",
                "error_key": format!("{error_key:?}"),
                "reason": reason,
            })
            .to_string(),
        },
        AdapterResponse::Queued { queue, .. } => HttpRuntimeResponse {
            status_code: 202,
            body: json!({ "status": "queued", "queue": queue }).to_string(),
        },
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => HttpRuntimeResponse {
            status_code: 409,
            body: json!({ "status": "duplicated", "idempotency_key": idempotency_key }).to_string(),
        },
    }
}

#[cfg(feature = "server-std")]
fn render_report(report: AdapterSdkReport) -> String {
    match report {
        AdapterSdkReport::Capabilities(catalog) => json!({
            "status": "accepted",
            "profile": catalog.profile.as_str(),
            "entry": {
                "http_server": catalog.entry.http_server.visible,
            }
        })
        .to_string(),
        AdapterSdkReport::Recall(report) => json!({
            "status": "accepted",
            "query": report.query,
            "procedural_hits": report.procedural_hits.len(),
        })
        .to_string(),
        AdapterSdkReport::Write(report) => json!({
            "status": "accepted",
            "operation": report.operation,
            "accepted": report.accepted,
            "changed": report.changed,
        })
        .to_string(),
        other => json!({
            "status": "accepted",
            "report": format!("{other:?}"),
        })
        .to_string(),
    }
}

#[cfg(feature = "server-std")]
fn unique_request_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}
