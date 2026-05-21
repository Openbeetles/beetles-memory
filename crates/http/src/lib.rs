//! HTTP and Webhook adapter contracts for Beetle Memory.

use bm_adapter::{AdapterErrorKey, AdapterOperation, TransportKind};

#[cfg(feature = "server-axum")]
use bm_adapter::{AdapterCommand, AdapterResponse, AdapterSdkReport, TransportMode};
#[cfg(feature = "server-axum")]
use bm_entry::{EntryAuthDecision, EntryRuntime, EntryTransportContext};
#[cfg(feature = "server-axum")]
use bm_sdk::{MemoryRecallRequest, MemoryWriteRequest, RuntimeSkillWrite, RuntimeSkillWriteSource};
#[cfg(feature = "server-axum")]
use serde::Deserialize;
#[cfg(feature = "server-axum")]
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteBodyMode {
    None,
    Json { max_bytes: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteAuth {
    TokenOrLoopback,
    WebhookSignature,
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
    webhook_post("/webhook/write-candidate", AdapterOperation::Write),
    webhook_post("/webhook/report", AdapterOperation::Inspect),
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

const fn webhook_post(path: &'static str, operation: AdapterOperation) -> RouteSpec {
    RouteSpec {
        method: HttpMethod::Post,
        path,
        transport: TransportKind::Webhook,
        operation,
        body: RouteBodyMode::Json {
            max_bytes: JSON_BODY_MAX_BYTES,
        },
        auth: RouteAuth::WebhookSignature,
        profile_gate_required: true,
    }
}

pub const fn route_specs() -> &'static [RouteSpec] {
    ROUTES
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

#[cfg(feature = "server-axum")]
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

#[cfg(feature = "server-axum")]
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
}

#[cfg(feature = "server-axum")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRuntimeResponse {
    pub status_code: u16,
    pub body: String,
}

#[cfg(feature = "server-axum")]
pub fn handle_http_request(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    let route = route_specs()
        .iter()
        .find(|route| route.method == request.method && route.path == request.path)
        .copied()
        .ok_or_else(|| bm_sdk::Error::config("http_runtime", "unknown route"))?;
    let command = decode_command(route.operation, &request.body)?;
    let response = runtime.handle(
        EntryTransportContext {
            request_id: request.request_id,
            transport: route.transport,
            mode: TransportMode::Server,
            operation: route.operation,
            source_id: "http-runtime".to_string(),
            source_kind: match route.transport {
                TransportKind::Webhook => "webhook_inbound",
                _ => "http_client",
            }
            .to_string(),
            idempotency_key: request.idempotency_key,
            audit_id: request.audit_id,
            auth: if request.authenticated {
                EntryAuthDecision::authenticated("token_or_loopback", "http-client")
            } else {
                EntryAuthDecision::unauthenticated("token_or_loopback")
            },
        },
        command,
    )?;
    Ok(render_http_response(response.adapter))
}

#[cfg(feature = "server-axum")]
fn decode_command(operation: AdapterOperation, body: &str) -> bm_sdk::Result<AdapterCommand> {
    match operation {
        AdapterOperation::Capabilities => Ok(AdapterCommand::Capabilities),
        AdapterOperation::Recall => {
            let payload: RecallPayload = parse_json(body)?;
            Ok(AdapterCommand::Recall(MemoryRecallRequest {
                query: payload.query,
                limit: payload.limit.unwrap_or(8),
            }))
        }
        AdapterOperation::Write => {
            let payload: ProceduralWritePayload = parse_json(body)?;
            Ok(AdapterCommand::Write(MemoryWriteRequest::Procedural {
                writes: vec![RuntimeSkillWrite {
                    name: payload.name,
                    topic: payload.topic,
                    title: payload.title,
                    summary: payload.summary,
                    content: payload.content,
                    citations: vec!["bm-http".to_string()],
                    source_chat_id: Some("chat-1".to_string()),
                    observed_at: 1_800_000_000,
                }],
                source: RuntimeSkillWriteSource::Manual,
            }))
        }
        other => Err(bm_sdk::Error::config(
            "http_runtime",
            format!("unsupported HTTP runtime operation: {other:?}"),
        )),
    }
}

#[cfg(feature = "server-axum")]
fn parse_json<T: for<'de> Deserialize<'de>>(body: &str) -> bm_sdk::Result<T> {
    serde_json::from_str(body)
        .map_err(|err| bm_sdk::Error::config("http_runtime_json", err.to_string()))
}

#[cfg(feature = "server-axum")]
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

#[cfg(feature = "server-axum")]
fn render_report(report: AdapterSdkReport) -> String {
    match report {
        AdapterSdkReport::Capabilities(catalog) => json!({
            "status": "accepted",
            "profile": catalog.profile.as_str(),
            "entry": {
                "http_server": catalog.entry.http_server.visible,
                "webhook_receiver": catalog.entry.webhook_receiver.visible,
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

#[cfg(feature = "server-axum")]
#[derive(Deserialize)]
struct RecallPayload {
    query: String,
    limit: Option<usize>,
}

#[cfg(feature = "server-axum")]
#[derive(Deserialize)]
struct ProceduralWritePayload {
    name: String,
    topic: String,
    title: String,
    summary: String,
    content: String,
}

#[cfg(feature = "server-axum")]
fn unique_request_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}
