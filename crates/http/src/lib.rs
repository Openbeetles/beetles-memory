//! HTTP and Webhook adapter contracts for Beetle Memory.

use bm_adapter::{AdapterErrorKey, AdapterOperation, TransportKind};

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
