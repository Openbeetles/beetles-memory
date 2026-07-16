//! HTTP adapter contracts for Beetle Memory.

#[cfg(all(
    feature = "server-std",
    any(
        feature = "profile-esp-standalone-memory",
        feature = "profile-esp-embedded-sdk"
    )
))]
compile_error!("bm-http server-std is forbidden for ESP profiles; use bm-sdk or a compact client transport instead.");

use bm_adapter::{AdapterErrorKey, AdapterOperation, TransportKind};

#[cfg(feature = "server-std")]
use bm_adapter::{
    decode_json_adapter_command, AdapterJsonCommandOptions, AdapterRequestIdentityOwner,
    AdapterResponse, AdapterRuntimeServices, AdapterSdkReport, TransportMode,
};
#[cfg(feature = "server-std")]
use bm_entry::{
    read_authorized_http_request, EntryAcceptedTcpStream, EntryAuthDecision,
    EntryConsoleDeviceCreate, EntryConsoleDeviceUpdate, EntryConsoleRuntimeSkillEdit,
    EntryConsoleSkillSetEnabled, EntryConsoleTransportUpdate, EntryHttpAuthorization,
    EntryHttpIngressErrorKind, EntryHttpIngressLimits, EntryHttpRequestHead, EntryLocalTransport,
    EntryOperationCapability, EntryRuntime, EntryRuntimeBudgetLease, EntryTransportContext,
};
#[cfg(feature = "server-std")]
use bm_ollama_transparent::{
    DisableOllamaTransparentRequest, EnableOllamaTransparentRequest, OllamaTransparentController,
};
#[cfg(feature = "server-std")]
use bm_sdk::{AgentToolRegistrySnapshot, RuntimeBudgetReport};
#[cfg(feature = "server-std")]
use serde_json::json;
#[cfg(feature = "server-std")]
use std::io::Write;
#[cfg(feature = "server-std")]
use std::net::{SocketAddr, TcpListener};
#[cfg(feature = "server-std")]
use std::path::PathBuf;
#[cfg(feature = "server-std")]
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Put,
    Post,
    Patch,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteBodyMode {
    None,
    Json,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentToolRegistryRouteSpec {
    pub method: HttpMethod,
    pub path: &'static str,
    pub body: RouteBodyMode,
    pub auth: RouteAuth,
    pub profile_gate_required: bool,
}

#[cfg(feature = "server-std")]
const CONSOLE_CAPABILITY_SCHEMA: &str = "beetle-memory.console.capabilities.v1";

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
    memory_post("/memory/long-term/list", AdapterOperation::LongTermList),
    memory_post("/memory/long-term/detail", AdapterOperation::LongTermDetail),
    memory_post("/memory/long-term/mutate", AdapterOperation::LongTermMutate),
    memory_post("/memory/long-term/policy", AdapterOperation::LongTermPolicy),
    memory_post(
        "/memory/transcript/attrs",
        AdapterOperation::TranscriptAttrWrite,
    ),
];

const CONSOLE_ROUTES: &[ConsoleRouteSpec] = &[
    console_get("/console/overview"),
    console_get("/console/capabilities"),
    console_get("/console/workbench/api-map"),
    console_get("/console/workbench/report"),
    console_get("/console/skills"),
    console_get("/console/skills/{name}"),
    console_patch("/console/skills/{name}"),
    console_patch("/console/skills/{name}/enabled"),
    console_delete("/console/skills/{name}"),
    console_get("/console/llm-gateway"),
    console_post("/console/llm-gateway/smoke-checks/{id}/run"),
    console_get("/console/ollama-transparent/status"),
    console_post("/console/ollama-transparent/preflight"),
    console_post("/console/ollama-transparent/enable"),
    console_post("/console/ollama-transparent/disable"),
    console_post("/console/ollama-transparent/open-app"),
    console_get("/console/transports"),
    console_patch("/console/transports/{id}"),
    console_get("/console/devices"),
    console_post("/console/devices"),
    console_patch("/console/devices/{id}"),
    console_post("/console/devices/{id}/rotate-key"),
    console_get("/console/session"),
];

const AGENT_TOOL_REGISTRY_ROUTES: &[AgentToolRegistryRouteSpec] = &[
    agent_tool_registry_get("/agent-tool-registries"),
    agent_tool_registry_get("/agent-tool-registries/{id}"),
    agent_tool_registry_put("/agent-tool-registries/{id}"),
    agent_tool_registry_delete("/agent-tool-registries/{id}"),
];

const fn memory_post(path: &'static str, operation: AdapterOperation) -> RouteSpec {
    RouteSpec {
        method: HttpMethod::Post,
        path,
        transport: TransportKind::Http,
        operation,
        body: RouteBodyMode::Json,
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

const fn agent_tool_registry_get(path: &'static str) -> AgentToolRegistryRouteSpec {
    AgentToolRegistryRouteSpec {
        method: HttpMethod::Get,
        path,
        body: RouteBodyMode::None,
        auth: RouteAuth::TokenOrLoopback,
        profile_gate_required: true,
    }
}

const fn agent_tool_registry_put(path: &'static str) -> AgentToolRegistryRouteSpec {
    AgentToolRegistryRouteSpec {
        method: HttpMethod::Put,
        path,
        body: RouteBodyMode::Json,
        auth: RouteAuth::TokenOrLoopback,
        profile_gate_required: true,
    }
}

const fn agent_tool_registry_delete(path: &'static str) -> AgentToolRegistryRouteSpec {
    AgentToolRegistryRouteSpec {
        method: HttpMethod::Delete,
        path,
        body: RouteBodyMode::None,
        auth: RouteAuth::TokenOrLoopback,
        profile_gate_required: true,
    }
}

pub const fn agent_tool_registry_route_specs() -> &'static [AgentToolRegistryRouteSpec] {
    AGENT_TOOL_REGISTRY_ROUTES
}

pub const fn invalid_json_error() -> AdapterErrorKey {
    AdapterErrorKey::InvalidJson
}

pub const fn unauthorized_error() -> AdapterErrorKey {
    AdapterErrorKey::Unauthorized
}

pub const fn forbidden_error() -> AdapterErrorKey {
    AdapterErrorKey::Forbidden
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
    pub authorization: Option<String>,
}

#[cfg(feature = "server-std")]
impl HttpRuntimeRequest {
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            path: path.into(),
            body: String::new(),
            request_id: String::new(),
            idempotency_key: String::new(),
            audit_id: String::new(),
            authorization: None,
        }
    }

    pub fn post_json(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Post,
            path: path.into(),
            body: body.into(),
            request_id: String::new(),
            idempotency_key: String::new(),
            audit_id: String::new(),
            authorization: None,
        }
    }

    pub fn put_json(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Put,
            path: path.into(),
            body: body.into(),
            request_id: String::new(),
            idempotency_key: String::new(),
            audit_id: String::new(),
            authorization: None,
        }
    }

    pub fn patch_json(path: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Patch,
            path: path.into(),
            body: body.into(),
            request_id: String::new(),
            idempotency_key: String::new(),
            audit_id: String::new(),
            authorization: None,
        }
    }

    pub fn delete(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Delete,
            path: path.into(),
            body: String::new(),
            request_id: String::new(),
            idempotency_key: String::new(),
            audit_id: String::new(),
            authorization: None,
        }
    }

    pub fn with_bearer_token(mut self, token: impl AsRef<str>) -> Self {
        self.authorization = Some(format!("Bearer {}", token.as_ref()));
        self
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = key.into();
        self
    }
}

#[cfg(feature = "server-std")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpConnectionTimeouts {
    pub read: Duration,
    pub write: Duration,
}

#[cfg(feature = "server-std")]
impl HttpConnectionTimeouts {
    pub fn new(read: Duration, write: Duration) -> bm_sdk::Result<Self> {
        if read.is_zero() || write.is_zero() {
            return Err(bm_sdk::Error::config(
                "http_listener_timeout",
                "HTTP read and write timeouts must be greater than zero",
            ));
        }
        Ok(Self { read, write })
    }
}

#[cfg(feature = "server-std")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRuntimeResponse {
    pub status_code: u16,
    pub body: String,
    pub budget_report_id: String,
}

#[cfg(feature = "server-std")]
fn acquire_runtime_budget_lease(runtime: &EntryRuntime) -> bm_sdk::Result<EntryRuntimeBudgetLease> {
    runtime.acquire_budget_lease()
}

#[cfg(feature = "server-std")]
fn bind_http_response_budget(
    mut response: HttpRuntimeResponse,
    report: &RuntimeBudgetReport,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    if response.body.len() > report.adapter_budget.http_body_max_bytes {
        return Err(bm_sdk::Error::config(
            "http_response_body",
            "HTTP response exceeds pinned runtime adapter budget",
        ));
    }
    response.budget_report_id.clone_from(&report.report_id);
    Ok(response)
}

#[cfg(feature = "server-std")]
#[derive(Clone, Copy, Default)]
pub struct HttpConsoleServices<'a> {
    pub ollama_transparent: Option<&'a dyn OllamaTransparentController>,
    pub memory_event_store_paths: &'a [PathBuf],
}

#[cfg(feature = "server-std")]
impl<'a> HttpConsoleServices<'a> {
    pub const fn none() -> Self {
        Self {
            ollama_transparent: None,
            memory_event_store_paths: &[],
        }
    }

    pub const fn with_ollama_transparent(
        ollama_transparent: &'a dyn OllamaTransparentController,
    ) -> Self {
        Self {
            ollama_transparent: Some(ollama_transparent),
            memory_event_store_paths: &[],
        }
    }

    pub const fn with_memory_event_store_paths(
        mut self,
        memory_event_store_paths: &'a [PathBuf],
    ) -> Self {
        self.memory_event_store_paths = memory_event_store_paths;
        self
    }
}

#[cfg(feature = "server-std")]
pub fn handle_http_in_process_request(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    handle_http_in_process_request_with_console_services(
        runtime,
        request,
        AdapterRuntimeServices::none(),
        HttpConsoleServices::none(),
    )
}

#[cfg(feature = "server-std")]
pub fn handle_http_in_process_request_with_services(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
    services: AdapterRuntimeServices<'_>,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    handle_http_in_process_request_with_console_services(
        runtime,
        request,
        services,
        HttpConsoleServices::none(),
    )
}

#[cfg(feature = "server-std")]
pub fn handle_http_in_process_request_with_console(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
    console_services: HttpConsoleServices<'_>,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    handle_http_in_process_request_with_console_services(
        runtime,
        request,
        AdapterRuntimeServices::none(),
        console_services,
    )
}

#[cfg(feature = "server-std")]
pub fn handle_http_in_process_request_with_console_services(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
    services: AdapterRuntimeServices<'_>,
    console_services: HttpConsoleServices<'_>,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    let lease = acquire_runtime_budget_lease(runtime)?;
    let auth = resolve_in_process_http_auth(runtime, &request);
    let response = runtime.execute_with_budget_lease(&lease, || {
        handle_http_in_process_request_with_budget_lease(
            runtime,
            request,
            services,
            console_services,
            &lease,
            auth,
        )
    })?;
    bind_http_response_budget(response, lease.report())
}

#[cfg(feature = "server-std")]
fn handle_http_in_process_request_with_budget_lease(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
    services: AdapterRuntimeServices<'_>,
    console_services: HttpConsoleServices<'_>,
    lease: &EntryRuntimeBudgetLease,
    auth: EntryAuthDecision,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    let budget_report = lease.report();
    if request.body.len() > budget_report.adapter_budget.http_body_max_bytes {
        return Err(bm_sdk::Error::config(
            "http_body",
            "HTTP body exceeds runtime adapter budget",
        ));
    }
    if !auth.is_authenticated() {
        return Ok(unauthorized_http_response(&auth));
    }
    let principal = auth.principal_id();
    if principal.is_empty() {
        return Ok(unauthorized_http_response(&auth));
    }
    let request_identity =
        AdapterRequestIdentityOwner::new(TransportKind::Http, "bm-http-runtime", principal)
            .issue(
                (!request.idempotency_key.trim().is_empty())
                    .then_some(request.idempotency_key.as_str()),
            )
            .map_err(|error| bm_sdk::Error::config("http_request_identity", error.to_string()))?;
    if request.path.starts_with("/console/") {
        let required = if request.method == HttpMethod::Get {
            EntryOperationCapability::ConsoleRead
        } else {
            EntryOperationCapability::ConsoleWrite
        };
        if !auth.allows(required) {
            return Ok(forbidden_capability_response(required));
        }
        return handle_console_request(runtime, request, console_services);
    }
    let (route_path, _) = split_query_path(&request.path);
    if route_path == "/agent-tool-registries" || route_path.starts_with("/agent-tool-registries/") {
        let route = find_agent_tool_registry_route(request.method, route_path)
            .ok_or_else(|| bm_sdk::Error::config("http_runtime", "unknown route"))?;
        let required = if route.method == HttpMethod::Get {
            EntryOperationCapability::ConsoleRead
        } else {
            EntryOperationCapability::ConsoleWrite
        };
        if !auth.allows(required) {
            return Ok(forbidden_capability_response(required));
        }
        let route_path = route_path.to_string();
        return handle_agent_tool_registry_request(runtime, request, route_path);
    }
    let route = route_specs()
        .iter()
        .find(|route| route.method == request.method && route.path == request.path)
        .copied()
        .ok_or_else(|| bm_sdk::Error::config("http_runtime", "unknown route"))?;
    reject_missing_remote_source_scope(runtime, route.operation, &request.body)?;
    let command = decode_json_adapter_command(
        route.operation,
        &request.body,
        &http_command_options(runtime),
    )?;
    let response = runtime.handle_with_budget_lease_and_services(
        EntryTransportContext::new(
            request_identity.request_id,
            route.transport,
            TransportMode::Server,
            route.operation,
            "http-runtime",
            "http_client",
            request_identity.idempotency_key,
            request_identity.audit_id,
            auth,
        ),
        command,
        services,
        lease,
    )?;
    if response.budget_report != *budget_report {
        return Err(bm_sdk::Error::config(
            "http_runtime_budget",
            "entry_response_budget_lease_identity_mismatch",
        ));
    }
    render_http_response(response.adapter)
}

#[cfg(feature = "server-std")]
fn resolve_in_process_http_auth(
    runtime: &EntryRuntime,
    request: &HttpRuntimeRequest,
) -> EntryAuthDecision {
    if runtime.uses_local_default_scope_policy() {
        runtime.authenticate_local_transport(EntryLocalTransport::InProcess, "http-in-process")
    } else {
        runtime.authenticate_remote_bearer(request.authorization.as_deref())
    }
}

#[cfg(feature = "server-std")]
fn unauthorized_http_response(auth: &EntryAuthDecision) -> HttpRuntimeResponse {
    json_response(
        401,
        json!({
            "status": "rejected",
            "error_key": AdapterErrorKey::Unauthorized.as_str(),
            "reason": auth.rejection_reason().unwrap_or("unauthenticated"),
        }),
    )
}

#[cfg(feature = "server-std")]
fn forbidden_capability_response(capability: EntryOperationCapability) -> HttpRuntimeResponse {
    json_response(
        403,
        json!({
            "status": "rejected",
            "error_key": AdapterErrorKey::Forbidden.as_str(),
            "reason": format!("principal lacks required capability: {}", capability.as_str()),
        }),
    )
}

#[cfg(feature = "server-std")]
pub fn serve_http_listener_once(
    runtime: &EntryRuntime,
    listener: &TcpListener,
    timeouts: HttpConnectionTimeouts,
) -> bm_sdk::Result<()> {
    serve_http_listener_once_with_console_services(
        runtime,
        listener,
        timeouts,
        HttpConsoleServices::none(),
    )
}

#[cfg(feature = "server-std")]
pub fn serve_http_listener_once_with_console_services(
    runtime: &EntryRuntime,
    listener: &TcpListener,
    timeouts: HttpConnectionTimeouts,
    console_services: HttpConsoleServices<'_>,
) -> bm_sdk::Result<()> {
    validate_http_listener_security(runtime, listener)?;
    let mut stream = EntryAcceptedTcpStream::accept(listener)
        .map_err(|err| bm_sdk::Error::config("http_listener_accept", err.to_string()))?;
    configure_http_stream_timeouts(&stream, timeouts)?;
    serve_http_accepted_stream(runtime, &mut stream, console_services)
}

#[cfg(feature = "server-std")]
pub fn validate_http_listener_security(
    runtime: &EntryRuntime,
    listener: &TcpListener,
) -> bm_sdk::Result<()> {
    let local_addr = listener
        .local_addr()
        .map_err(|error| bm_sdk::Error::config("http_listener_addr", error.to_string()))?;
    validate_http_bind_security(runtime, local_addr)
}

#[cfg(feature = "server-std")]
pub fn validate_http_bind_security(
    runtime: &EntryRuntime,
    local_addr: SocketAddr,
) -> bm_sdk::Result<()> {
    if !local_addr.ip().is_loopback() && !runtime.has_bearer_verifier() {
        return Err(bm_sdk::Error::config(
            "http_listener_auth",
            "non-loopback HTTP bind requires a configured bearer verifier",
        ));
    }
    Ok(())
}

#[cfg(feature = "server-std")]
fn configure_http_stream_timeouts(
    stream: &EntryAcceptedTcpStream,
    timeouts: HttpConnectionTimeouts,
) -> bm_sdk::Result<()> {
    stream
        .set_read_timeout(Some(timeouts.read))
        .map_err(|error| bm_sdk::Error::config("http_read_timeout", error.to_string()))?;
    stream
        .set_write_timeout(Some(timeouts.write))
        .map_err(|error| bm_sdk::Error::config("http_write_timeout", error.to_string()))
}

#[cfg(feature = "server-std")]
pub fn serve_http_accepted_stream(
    runtime: &EntryRuntime,
    stream: &mut EntryAcceptedTcpStream,
    console_services: HttpConsoleServices<'_>,
) -> bm_sdk::Result<()> {
    let lease = acquire_runtime_budget_lease(runtime)?;
    let budget_report = lease.report();
    let ingress = read_authorized_http_request(
        stream,
        EntryHttpIngressLimits::new(
            budget_report.adapter_budget.http_header_max_bytes,
            budget_report.adapter_budget.http_body_max_bytes,
        )
        .map_err(|error| bm_sdk::Error::config("http_ingress", error.to_string()))?,
        |accepted, head| {
            let required = http_head_required_capability(head)?;
            let auth = runtime.authenticate_accepted_tcp_stream(
                accepted,
                head.header("authorization"),
                "http-loopback",
            );
            EntryHttpAuthorization::require(auth, required)
        },
    );
    let ingress = match ingress {
        Ok(ingress) => ingress,
        Err(error) if error.kind() == EntryHttpIngressErrorKind::Unauthorized => {
            let auth = error.auth_decision().ok_or_else(|| {
                bm_sdk::Error::config("http_auth", "missing ingress auth decision")
            })?;
            let response =
                bind_http_response_budget(unauthorized_http_response(auth), budget_report)?;
            return write_http_rejection_response(stream, response);
        }
        Err(error) if error.kind() == EntryHttpIngressErrorKind::Forbidden => {
            let capability = error.required_capability().ok_or_else(|| {
                bm_sdk::Error::config("http_auth", "missing ingress capability decision")
            })?;
            let response = bind_http_response_budget(
                forbidden_capability_response(capability),
                budget_report,
            )?;
            return write_http_rejection_response(stream, response);
        }
        Err(error) => return Err(map_http_ingress_error(error)),
    };
    let request = http_runtime_request_from_ingress(ingress)?;
    let auth = request.1;
    let request = request.0;
    let response = runtime.execute_with_budget_lease(&lease, || {
        handle_http_in_process_request_with_budget_lease(
            runtime,
            request,
            AdapterRuntimeServices::none(),
            console_services,
            &lease,
            auth,
        )
    })?;
    let response = bind_http_response_budget(response, budget_report)?;
    write_http_response(stream, response)
}

#[cfg(feature = "server-std")]
fn map_http_ingress_error(error: bm_entry::EntryHttpIngressError) -> bm_sdk::Error {
    let stage = if error.kind() == EntryHttpIngressErrorKind::PayloadTooLarge
        || error.message().contains("content-length")
        || error.message().contains("HTTP body")
    {
        "http_body"
    } else {
        "http_headers"
    };
    bm_sdk::Error::config(stage, error.to_string())
}

#[cfg(feature = "server-std")]
fn http_runtime_request_from_ingress(
    ingress: bm_entry::EntryAuthorizedHttpRequest,
) -> bm_sdk::Result<(HttpRuntimeRequest, EntryAuthDecision)> {
    let (head, body_bytes, auth) = ingress.into_parts();
    let method = parse_http_method(head.method())?;
    let headers = head.headers();
    let content_length = head.content_length();
    if matches!(
        method,
        HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch
    ) && !headers.contains_key("content-length")
    {
        return Err(bm_sdk::Error::config(
            "http_body",
            "content-length is required for request bodies",
        ));
    }
    if matches!(method, HttpMethod::Get | HttpMethod::Delete) && content_length != 0 {
        return Err(bm_sdk::Error::config(
            "http_body",
            "GET and DELETE requests must not carry a body",
        ));
    }
    if matches!(
        method,
        HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch
    ) && headers
        .get("content-type")
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        != Some("application/json")
    {
        return Err(bm_sdk::Error::config(
            "http_headers",
            "request body requires application/json content-type",
        ));
    }

    let body = String::from_utf8(body_bytes)
        .map_err(|err| bm_sdk::Error::config("http_body", err.to_string()))?;

    Ok((
        HttpRuntimeRequest {
            method,
            path: head.target().to_string(),
            body,
            request_id: String::new(),
            idempotency_key: headers
                .get("x-idempotency-key")
                .cloned()
                .unwrap_or_default(),
            audit_id: String::new(),
            authorization: headers.get("authorization").cloned(),
        },
        auth,
    ))
}

#[cfg(feature = "server-std")]
fn parse_http_method(method: &str) -> bm_sdk::Result<HttpMethod> {
    match method {
        "GET" => Ok(HttpMethod::Get),
        "PUT" => Ok(HttpMethod::Put),
        "POST" => Ok(HttpMethod::Post),
        "PATCH" => Ok(HttpMethod::Patch),
        "DELETE" => Ok(HttpMethod::Delete),
        other => Err(bm_sdk::Error::config(
            "http_headers",
            format!("unsupported HTTP method: {other}"),
        )),
    }
}

#[cfg(feature = "server-std")]
fn http_head_required_capability(
    head: &EntryHttpRequestHead,
) -> Result<EntryOperationCapability, bm_entry::EntryHttpIngressError> {
    let method = parse_http_method(head.method())
        .map_err(|error| bm_entry::EntryHttpIngressError::invalid_request(error.to_string()))?;
    let (route_path, _) = split_query_path(head.target());
    if route_path.starts_with("/console/") {
        return Ok(if method == HttpMethod::Get {
            EntryOperationCapability::ConsoleRead
        } else {
            EntryOperationCapability::ConsoleWrite
        });
    }
    if route_path == "/agent-tool-registries" || route_path.starts_with("/agent-tool-registries/") {
        let route = find_agent_tool_registry_route(method, route_path).ok_or_else(|| {
            bm_entry::EntryHttpIngressError::invalid_request("unknown HTTP route")
        })?;
        return Ok(if route.method == HttpMethod::Get {
            EntryOperationCapability::ConsoleRead
        } else {
            EntryOperationCapability::ConsoleWrite
        });
    }
    route_specs()
        .iter()
        .find(|route| route.method == method && route.path == head.target())
        .map(|route| EntryOperationCapability::for_adapter_operation(route.operation))
        .ok_or_else(|| bm_entry::EntryHttpIngressError::invalid_request("unknown HTTP route"))
}

#[cfg(feature = "server-std")]
fn http_command_options(runtime: &EntryRuntime) -> AdapterJsonCommandOptions {
    let options = AdapterJsonCommandOptions::new("bm-http");
    if runtime.uses_local_default_scope_policy() {
        options.with_default_source_chat_id(runtime.runtime().scope().chat_id.clone())
    } else {
        options
    }
}

#[cfg(feature = "server-std")]
fn reject_missing_remote_source_scope(
    runtime: &EntryRuntime,
    operation: AdapterOperation,
    body: &str,
) -> bm_sdk::Result<()> {
    if runtime.uses_local_default_scope_policy() || operation != AdapterOperation::Write {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|err| bm_sdk::Error::config("adapter_json_command", err.to_string()))?;
    if has_source_chat_id(&value) {
        return Ok(());
    }
    Err(bm_sdk::Error::config(
        "adapter_json_command",
        "remote adapter write payload missing source_chat_id; refusing implicit chat-1 scope",
    ))
}

#[cfg(feature = "server-std")]
fn has_source_chat_id(value: &serde_json::Value) -> bool {
    if value
        .get("source_chat_id")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return true;
    }
    value
        .get("writes")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|writes| {
            !writes.is_empty()
                && writes.iter().all(|write| {
                    write
                        .get("source_chat_id")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty())
                })
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
        403 => "Forbidden",
        409 => "Conflict",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        _ => "OK",
    };
    let body = response.body;
    let head = format!(
        "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nx-bm-runtime-budget-report-id: {}\r\nconnection: close\r\n\r\n",
        response.status_code,
        reason,
        body.len(),
        response.budget_report_id,
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body.as_bytes()))
        .and_then(|_| stream.flush())
        .map_err(|err| bm_sdk::Error::config("http_write", err.to_string()))
}

#[cfg(feature = "server-std")]
fn write_http_rejection_response(
    stream: &mut EntryAcceptedTcpStream,
    response: HttpRuntimeResponse,
) -> bm_sdk::Result<()> {
    write_http_response(stream, response)?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .and_then(|_| stream.discard_currently_buffered_input())
        .map_err(|error| bm_sdk::Error::config("http_rejection_close", error.to_string()))
}

#[cfg(feature = "server-std")]
fn handle_agent_tool_registry_request(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
    route_path: String,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    match (request.method, route_path.as_str()) {
        (HttpMethod::Get, "/agent-tool-registries") => {
            let registries = runtime.runtime().agent_tool_registries();
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "registries": registries,
                    "report": runtime.runtime().agent_tool_registry_report()?,
                }),
            ))
        }
        (HttpMethod::Get, path) if path.starts_with("/agent-tool-registries/") => {
            let registry_id = trim_suffix_path(path, "/agent-tool-registries/");
            match runtime
                .runtime()
                .agent_tool_registries()
                .into_iter()
                .find(|registry| registry.registry_id == registry_id)
            {
                Some(registry) => Ok(json_response(
                    200,
                    json!({
                        "status": "accepted",
                        "registry": registry,
                        "report": runtime.runtime().agent_tool_registry_report()?,
                    }),
                )),
                None => Ok(not_found("agent tool registry not found")),
            }
        }
        (HttpMethod::Put, path) if path.starts_with("/agent-tool-registries/") => {
            let registry_id = trim_suffix_path(path, "/agent-tool-registries/");
            let registry: AgentToolRegistrySnapshot = parse_console_json(&request.body)?;
            if registry.registry_id != registry_id {
                return Ok(json_response(
                    422,
                    json!({
                        "status": "rejected",
                        "errorKey": "RuntimeRejected",
                        "reason": "agent tool registry path id does not match payload registry_id",
                    }),
                ));
            }
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "registry": registry,
                    "report": runtime.runtime().upsert_agent_tool_registry(registry)?,
                }),
            ))
        }
        (HttpMethod::Delete, path) if path.starts_with("/agent-tool-registries/") => {
            let registry_id = trim_suffix_path(path, "/agent-tool-registries/");
            let existed = runtime
                .runtime()
                .agent_tool_registries()
                .iter()
                .any(|registry| registry.registry_id == registry_id);
            if !existed {
                return Ok(not_found("agent tool registry not found"));
            }
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "deleted": registry_id,
                    "report": runtime.runtime().delete_agent_tool_registry(registry_id)?,
                }),
            ))
        }
        _ => Ok(json_response(
            405,
            json!({
                "status": "rejected",
                "errorKey": "UnsupportedOperation",
                "reason": "agent tool registry method is not allowed for route",
            }),
        )),
    }
}

#[cfg(feature = "server-std")]
fn handle_console_request(
    runtime: &EntryRuntime,
    request: HttpRuntimeRequest,
    services: HttpConsoleServices<'_>,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    let (route_path, query_string) = split_query_path(&request.path);
    match (request.method, route_path) {
        (HttpMethod::Get, "/console/overview") => Ok(json_response(
            200,
            json!({
                "status": "accepted",
                "overview": runtime.console_overview_with_event_store_paths(
                    services.memory_event_store_paths,
                ),
            }),
        )),
        (HttpMethod::Get, "/console/capabilities") => Ok(json_response(
            200,
            json!({
                "status": "accepted",
                "capabilities": console_capabilities(services),
            }),
        )),
        (HttpMethod::Get, "/console/workbench/api-map") => Ok(json_response(
            200,
            json!({
                "status": "accepted",
                "workbench": runtime.console_workbench_api_map(),
            }),
        )),
        (HttpMethod::Get, "/console/workbench/report") => Ok(json_response(
            200,
            json!({
                "status": "accepted",
                "workbenchReport": runtime.console_workbench_report(),
            }),
        )),
        (HttpMethod::Get, "/console/transports") => Ok(json_response(
            200,
            json!({
                "status": "accepted",
                "transports": runtime.console_transports(),
            }),
        )),
        (HttpMethod::Get, "/console/llm-gateway") => Ok(json_response(
            200,
            json!({
                "status": "accepted",
                "llmGateway": runtime.console_llm_gateway(),
            }),
        )),
        (HttpMethod::Get, "/console/ollama-transparent/status") => {
            let controller = require_ollama_transparent_controller(services)?;
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "ollamaTransparent": controller.status().map_err(map_transparent_error)?,
                }),
            ))
        }
        (HttpMethod::Post, "/console/ollama-transparent/preflight") => {
            let controller = require_ollama_transparent_controller(services)?;
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "preflight": controller.preflight().map_err(map_transparent_error)?,
                }),
            ))
        }
        (HttpMethod::Post, "/console/ollama-transparent/enable") => {
            let controller = require_ollama_transparent_controller(services)?;
            let payload: EnableOllamaTransparentRequest = parse_console_json(&request.body)?;
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "transition": controller.enable(payload).map_err(map_transparent_error)?,
                }),
            ))
        }
        (HttpMethod::Post, "/console/ollama-transparent/disable") => {
            let controller = require_ollama_transparent_controller(services)?;
            let payload: DisableOllamaTransparentRequest = parse_console_json(&request.body)?;
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "transition": controller.disable(payload).map_err(map_transparent_error)?,
                }),
            ))
        }
        (HttpMethod::Post, "/console/ollama-transparent/open-app") => {
            let controller = require_ollama_transparent_controller(services)?;
            Ok(json_response(
                200,
                json!({
                    "status": "accepted",
                    "action": controller.open_app().map_err(map_transparent_error)?,
                }),
            ))
        }
        (HttpMethod::Post, path)
            if path.starts_with("/console/llm-gateway/smoke-checks/") && path.ends_with("/run") =>
        {
            let id = trim_suffix_path(path, "/console/llm-gateway/smoke-checks/")
                .strip_suffix("/run")
                .unwrap_or("")
                .trim_matches('/');
            if id.is_empty() {
                return Ok(not_found("console llm gateway smoke check not found"));
            }
            match runtime.console_run_llm_gateway_smoke_check(id) {
                Some(result) => Ok(json_response(
                    200,
                    json!({
                        "status": "accepted",
                        "result": result,
                    }),
                )),
                None => Ok(not_found("console llm gateway smoke check not found")),
            }
        }
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
            let payload: EntryConsoleRuntimeSkillEdit = parse_console_json(&request.body)?;
            let mutation = runtime.console_edit_runtime_skill(name, payload)?;
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
fn console_capabilities(services: HttpConsoleServices<'_>) -> serde_json::Value {
    let ollama_transparent_app_visible = services.ollama_transparent.is_some();
    json!({
        "schema": CONSOLE_CAPABILITY_SCHEMA,
        "features": {
            "ollamaTransparentApp": {
                "id": "ollamaTransparentApp",
                "visible": ollama_transparent_app_visible,
                "available": ollama_transparent_app_visible,
                "owner": if ollama_transparent_app_visible { "desktop-shell" } else { "unsupported-shell" },
                "reason": if ollama_transparent_app_visible { serde_json::Value::Null } else { json!("DesktopShellOnly") },
                "routes": if ollama_transparent_app_visible {
                    json!({
                        "status": "/console/ollama-transparent/status",
                        "enable": "/console/ollama-transparent/enable",
                        "disable": "/console/ollama-transparent/disable",
                        "openApp": "/console/ollama-transparent/open-app"
                    })
                } else {
                    json!({})
                }
            }
        }
    })
}

#[cfg(feature = "server-std")]
fn require_ollama_transparent_controller(
    services: HttpConsoleServices<'_>,
) -> bm_sdk::Result<&dyn OllamaTransparentController> {
    services.ollama_transparent.ok_or_else(|| {
        bm_sdk::Error::config(
            "console_ollama_transparent",
            "ollama transparent controller is not configured",
        )
    })
}

#[cfg(feature = "server-std")]
fn map_transparent_error(error: bm_ollama_transparent::OllamaTransparentError) -> bm_sdk::Error {
    bm_sdk::Error::config(
        "console_ollama_transparent",
        format!("{}: {}", error.key().as_str(), error.message()),
    )
}

#[cfg(feature = "server-std")]
fn find_agent_tool_registry_route(
    method: HttpMethod,
    path: &str,
) -> Option<AgentToolRegistryRouteSpec> {
    agent_tool_registry_route_specs()
        .iter()
        .find(|route| route.method == method && route_pattern_matches(route.path, path))
        .copied()
}

#[cfg(feature = "server-std")]
fn route_pattern_matches(pattern: &str, path: &str) -> bool {
    if pattern == path {
        return true;
    }
    let Some(prefix) = pattern.strip_suffix("/{id}") else {
        return false;
    };
    let Some(id) = path
        .strip_prefix(prefix)
        .and_then(|value| value.strip_prefix('/'))
    else {
        return false;
    };
    !id.trim().is_empty() && !id.contains('/')
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
            | "/console/capabilities"
            | "/console/workbench/api-map"
            | "/console/workbench/report"
            | "/console/skills"
            | "/console/llm-gateway"
            | "/console/ollama-transparent/status"
            | "/console/ollama-transparent/preflight"
            | "/console/ollama-transparent/enable"
            | "/console/ollama-transparent/disable"
            | "/console/ollama-transparent/open-app"
            | "/console/transports"
            | "/console/devices"
            | "/console/session"
    ) || path.starts_with("/console/skills/")
        || path.starts_with("/console/llm-gateway/smoke-checks/")
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
        budget_report_id: String::new(),
    }
}

#[cfg(feature = "server-std")]
fn render_http_response(
    response: AdapterResponse<AdapterSdkReport>,
) -> bm_sdk::Result<HttpRuntimeResponse> {
    Ok(match response {
        AdapterResponse::Accepted { report, .. } => HttpRuntimeResponse {
            status_code: 200,
            body: render_report(report)?,
            budget_report_id: String::new(),
        },
        AdapterResponse::Rejected {
            error_key, reason, ..
        } => HttpRuntimeResponse {
            status_code: match error_key {
                AdapterErrorKey::Unauthorized => 401,
                AdapterErrorKey::Forbidden => 403,
                AdapterErrorKey::PayloadTooLarge => 413,
                AdapterErrorKey::InvalidJson => 400,
                AdapterErrorKey::Duplicated => 409,
                AdapterErrorKey::OperationMismatch
                | AdapterErrorKey::UnsupportedOperation
                | AdapterErrorKey::RuntimeRejected => 422,
            },
            body: json!({
                "status": "rejected",
                "error_key": error_key.as_str(),
                "reason": reason,
            })
            .to_string(),
            budget_report_id: String::new(),
        },
        AdapterResponse::Queued { queue, .. } => HttpRuntimeResponse {
            status_code: 202,
            body: json!({ "status": "queued", "queue": queue }).to_string(),
            budget_report_id: String::new(),
        },
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => HttpRuntimeResponse {
            status_code: 409,
            body: json!({ "status": "duplicated", "idempotency_key": idempotency_key }).to_string(),
            budget_report_id: String::new(),
        },
    })
}

#[cfg(feature = "server-std")]
fn render_report(report: AdapterSdkReport) -> bm_sdk::Result<String> {
    Ok(match report {
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
            "agent_tool_hints": report.agent_tool_hints,
            "tool_experience_status": report.tool_experience_status,
        })
        .to_string(),
        AdapterSdkReport::Write(report) => json!({
            "status": "accepted",
            "operation": report.operation,
            "accepted": report.accepted,
            "changed": report.changed,
            "agent_tool_experience": report.agent_tool_experience,
        })
        .to_string(),
        AdapterSdkReport::Project(report) => json!({
            "status": "accepted",
            "projection_surface": "ui_api",
            "projection_block": report.projection_block,
            "chars": report.chars,
            "agent_tool_hints": report.agent_tool_hints,
            "audit": report.audit,
        })
        .to_string(),
        AdapterSdkReport::LongTermList(report) => json!({
            "status": "accepted",
            "records": report.records,
            "total_visible": report.total_visible,
            "next_cursor": report.next_cursor,
        })
        .to_string(),
        AdapterSdkReport::LongTermDetail(report) => json!({
            "status": "accepted",
            "record": report.record,
            "revisions": report.revisions,
            "tombstone": report.tombstone,
            "transcript_refs": report.transcript_refs,
        })
        .to_string(),
        AdapterSdkReport::LongTermMutate(report) => json!({
            "status": "accepted",
            "accepted": report.accepted,
            "operation": report.operation,
            "affected_records": report.affected_records,
            "tombstones": report.tombstones,
            "transcript_refs": report.transcript_refs,
            "policy_decision": report.policy_decision,
            "lifecycle": report.lifecycle_report.result_summary,
        })
        .to_string(),
        AdapterSdkReport::LongTermPolicy(report) => json!({
            "status": "accepted",
            "accepted": report.accepted,
            "operation": report.operation,
            "policy_id": report.policy_id,
            "affected_future_writes": report.affected_future_writes,
            "policy_decision": report.policy_decision,
            "lifecycle": report.lifecycle_report.result_summary,
        })
        .to_string(),
        AdapterSdkReport::TranscriptAttrWrite(report) => json!({
            "status": "accepted",
            "memory_space_id": report.key.memory_space_id,
            "channel_id": report.key.channel_id,
            "conversation_id": report.key.conversation_id,
            "accepted_attrs": report.accepted_attrs,
            "rejected_attrs": report.rejected_attrs,
            "redactions_preview": report.redactions_preview,
            "profile_budget_applied": report.profile_budget_applied,
            "audit_event_id": report.audit_event_id,
            "dry_run": report.dry_run,
            "lifecycle": report.lifecycle_report.result_summary,
        })
        .to_string(),
        other => json!({
            "status": "accepted",
            "report_kind": other.public_kind(),
        })
        .to_string(),
    })
}
