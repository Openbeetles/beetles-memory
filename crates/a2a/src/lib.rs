//! A2A bridge contracts for Beetle Memory.

#[cfg(all(
    feature = "bridge-http",
    any(
        feature = "profile-esp-standalone-memory",
        feature = "profile-esp-embedded-sdk"
    )
))]
compile_error!("bm-a2a bridge-http is forbidden for ESP profiles.");

use bm_adapter::AdapterOperation;
use serde::Serialize;

#[cfg(feature = "bridge-http")]
use bm_adapter::{
    decode_json_adapter_command, AdapterJsonCommandOptions, AdapterRequestIdentityOwner,
    AdapterResponse, AdapterSdkReport, TransportKind, TransportMode,
};
#[cfg(feature = "bridge-http")]
use bm_entry::{
    EntryAcceptedTcpStream, EntryAuthDecision, EntryLocalTransport, EntryRuntime,
    EntryRuntimeBudgetLease, EntryTransportContext,
};
#[cfg(feature = "bridge-http")]
use serde::Deserialize;
#[cfg(feature = "bridge-http")]
use serde_json::{json, Value};
#[cfg(feature = "bridge-http")]
use std::io::{Read, Write};
#[cfg(feature = "bridge-http")]
use std::net::Shutdown;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum A2aPermission {
    MemoryReport,
    Executor,
    Tool,
    Workflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A2aBridgeMessageSpec {
    pub name: &'static str,
    pub operation: Option<AdapterOperation>,
    pub permissions: Vec<A2aPermission>,
}

pub fn merge_peer_visibility(local_visible: bool, peer_visible: bool) -> bool {
    local_visible && peer_visible
}

pub fn bridge_message_specs() -> Vec<A2aBridgeMessageSpec> {
    vec![
        message("peer_capability", None),
        message("memory_write_candidate", Some(AdapterOperation::Write)),
        message(
            "memory_finalize_turn_request",
            Some(AdapterOperation::FinalizeTurn),
        ),
        message("memory_recall_request", Some(AdapterOperation::Recall)),
        message("memory_projection_request", Some(AdapterOperation::Project)),
        message(
            "memory_long_term_list_request",
            Some(AdapterOperation::LongTermList),
        ),
        message(
            "memory_long_term_detail_request",
            Some(AdapterOperation::LongTermDetail),
        ),
        message(
            "memory_long_term_mutation_request",
            Some(AdapterOperation::LongTermMutate),
        ),
        message(
            "memory_long_term_policy_request",
            Some(AdapterOperation::LongTermPolicy),
        ),
        message(
            "memory_transcript_attr_write_request",
            Some(AdapterOperation::TranscriptAttrWrite),
        ),
        message("memory_report", None),
        message("runtime_lifecycle_event", None),
    ]
}

fn message(name: &'static str, operation: Option<AdapterOperation>) -> A2aBridgeMessageSpec {
    A2aBridgeMessageSpec {
        name,
        operation,
        permissions: vec![A2aPermission::MemoryReport],
    }
}

#[cfg(feature = "bridge-http")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A2aPeerCapability {
    pub memory_report_visible: bool,
}

#[cfg(feature = "bridge-http")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A2aRuntimeMessage {
    pub name: String,
    pub payload: String,
    pub idempotency_key: Option<String>,
}

#[cfg(feature = "bridge-http")]
impl A2aRuntimeMessage {
    pub fn json(name: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            payload: payload.into(),
            idempotency_key: None,
        }
    }

    pub fn with_idempotency_key(mut self, idempotency_key: impl Into<String>) -> Self {
        self.idempotency_key = Some(idempotency_key.into());
        self
    }
}

#[cfg(feature = "bridge-http")]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct A2aRuntimeResponse {
    pub kind: String,
    pub payload: String,
    pub permissions: Vec<A2aPermission>,
}

#[cfg(feature = "bridge-http")]
pub struct A2aBridge {
    bridge_id: String,
}

#[cfg(feature = "bridge-http")]
impl A2aBridge {
    pub fn new(bridge_id: impl Into<String>) -> Self {
        Self {
            bridge_id: bridge_id.into(),
        }
    }

    pub fn merge_peer_visibility(&self, peer: A2aPeerCapability) -> bool {
        merge_peer_visibility(true, peer.memory_report_visible)
    }

    pub fn handle_in_process_request(
        &self,
        runtime: &EntryRuntime,
        principal: &str,
        message: A2aRuntimeMessage,
    ) -> bm_sdk::Result<A2aRuntimeResponse> {
        let lease = acquire_runtime_budget_lease(runtime)?;
        let auth = runtime.authenticate_local_transport(EntryLocalTransport::InProcess, principal);
        runtime.execute_with_budget_lease(&lease, || {
            self.handle_authenticated_request(
                runtime,
                message,
                &lease,
                &auth,
                TransportMode::InProcess,
                "a2a_in_process",
            )
        })
    }

    fn handle_authenticated_request(
        &self,
        runtime: &EntryRuntime,
        message: A2aRuntimeMessage,
        lease: &EntryRuntimeBudgetLease,
        auth: &EntryAuthDecision,
        mode: TransportMode,
        source_kind: &str,
    ) -> bm_sdk::Result<A2aRuntimeResponse> {
        let budget_report = lease.report();
        if !auth.is_authenticated() || auth.principal_id().is_empty() {
            return Err(bm_sdk::Error::config(
                "entry_auth",
                auth.rejection_reason()
                    .unwrap_or("A2A request requires an authenticated principal"),
            ));
        }
        let message_material_bytes = message
            .payload
            .len()
            .saturating_add(message.idempotency_key.as_ref().map_or(0, |key| key.len()));
        if message_material_bytes > budget_report.adapter_budget.http_body_max_bytes {
            return Err(bm_sdk::Error::config(
                "a2a_message_budget",
                "A2A message payload and metadata exceed pinned runtime adapter budget",
            ));
        }
        let spec = bridge_message_specs()
            .into_iter()
            .find(|spec| spec.name == message.name)
            .ok_or_else(|| bm_sdk::Error::config("a2a_bridge", "unsupported bridge message"))?;
        let operation = spec.operation.ok_or_else(|| {
            bm_sdk::Error::config("a2a_bridge", "message has no memory operation")
        })?;
        reject_missing_remote_source_scope(runtime, operation, &message.payload)?;
        let command = decode_json_adapter_command(
            operation,
            &message.payload,
            &a2a_command_options(runtime),
        )?;
        let request_identity = AdapterRequestIdentityOwner::new(
            TransportKind::A2a,
            &self.bridge_id,
            auth.principal_id(),
        )
        .issue(message.idempotency_key.as_deref())
        .map_err(|error| bm_sdk::Error::config("a2a_request_identity", error.to_string()))?;
        let response = runtime.handle_with_budget_lease(
            EntryTransportContext::new(
                request_identity.request_id,
                TransportKind::A2a,
                mode,
                operation,
                self.bridge_id.clone(),
                source_kind,
                request_identity.idempotency_key,
                request_identity.audit_id,
                auth.clone(),
            ),
            command,
            lease,
        )?;
        if response.budget_report != *budget_report {
            return Err(bm_sdk::Error::config(
                "a2a_runtime_budget",
                "entry_response_budget_lease_identity_mismatch",
            ));
        }
        Ok(A2aRuntimeResponse {
            kind: "memory_report".to_string(),
            payload: render_response(response.adapter),
            permissions: vec![A2aPermission::MemoryReport],
        })
    }
}

#[cfg(feature = "bridge-http")]
fn a2a_command_options(runtime: &EntryRuntime) -> AdapterJsonCommandOptions {
    let options = AdapterJsonCommandOptions::new("bm-a2a");
    if runtime.uses_local_default_scope_policy() {
        options.with_default_source_chat_id(runtime.runtime().scope().chat_id.clone())
    } else {
        options
    }
}

#[cfg(feature = "bridge-http")]
fn reject_missing_remote_source_scope(
    runtime: &EntryRuntime,
    operation: AdapterOperation,
    body: &str,
) -> bm_sdk::Result<()> {
    if runtime.uses_local_default_scope_policy() || operation != AdapterOperation::Write {
        return Ok(());
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|err| bm_sdk::Error::config("adapter_json_command", err.to_string()))?;
    if value
        .get("source_chat_id")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(());
    }
    Err(bm_sdk::Error::config(
        "adapter_json_command",
        "remote adapter write payload missing source_chat_id; refusing implicit chat-1 scope",
    ))
}

#[cfg(feature = "bridge-http")]
fn acquire_runtime_budget_lease(runtime: &EntryRuntime) -> bm_sdk::Result<EntryRuntimeBudgetLease> {
    runtime.acquire_budget_lease()
}

#[cfg(feature = "bridge-http")]
pub fn serve_a2a_http_accepted_stream(
    runtime: &EntryRuntime,
    bridge: &A2aBridge,
    stream: &mut EntryAcceptedTcpStream,
) -> bm_sdk::Result<()> {
    let lease = acquire_runtime_budget_lease(runtime)?;
    let budget_report = lease.report();
    let request_head = read_a2a_http_request_head(
        stream,
        budget_report.adapter_budget.http_header_max_bytes,
        budget_report.adapter_budget.http_body_max_bytes,
    )?;
    let auth = runtime.authenticate_accepted_tcp_stream(
        stream,
        request_head.authorization.as_deref(),
        "a2a-loopback-peer",
    );
    if !auth.is_authenticated() || auth.principal_id().is_empty() {
        write_a2a_http_auth_error(
            stream,
            auth.rejection_reason()
                .unwrap_or("A2A HTTP authentication failed"),
            &budget_report.report_id,
        )?;
        return stream
            .shutdown(Shutdown::Write)
            .map_err(|err| bm_sdk::Error::config("a2a_http_write", err.to_string()));
    }
    let body = read_a2a_http_body(stream, request_head.content_length)?;
    let request: A2aHttpRequest = serde_json::from_str(&body)
        .map_err(|err| bm_sdk::Error::config("a2a_http_json", err.to_string()))?;
    let mut message = A2aRuntimeMessage::json(request.name, request.payload.to_string());
    if let Some(idempotency_key) = request.idempotency_key {
        message = message.with_idempotency_key(idempotency_key);
    }
    runtime.execute_with_budget_lease(&lease, || {
        let response = bridge.handle_authenticated_request(
            runtime,
            message,
            &lease,
            &auth,
            TransportMode::Bidirectional,
            "a2a_peer",
        )?;
        write_a2a_http_response(
            stream,
            response,
            budget_report.adapter_budget.http_body_max_bytes,
            &budget_report.report_id,
        )
    })
}

#[cfg(feature = "bridge-http")]
struct A2aHttpRequestHead {
    authorization: Option<String>,
    content_length: usize,
}

#[cfg(feature = "bridge-http")]
fn read_a2a_http_request_head(
    stream: &mut impl Read,
    header_max_bytes: usize,
    body_max_bytes: usize,
) -> bm_sdk::Result<A2aHttpRequestHead> {
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    while !buffer.ends_with(b"\r\n\r\n") {
        if buffer.len() == header_max_bytes {
            return Err(bm_sdk::Error::config(
                "a2a_http_read",
                "A2A HTTP headers exceed pinned runtime adapter budget",
            ));
        }
        stream
            .read_exact(&mut byte)
            .map_err(|err| bm_sdk::Error::config("a2a_http_read", err.to_string()))?;
        buffer.push(byte[0]);
    }
    let header_text = std::str::from_utf8(&buffer)
        .map_err(|err| bm_sdk::Error::config("a2a_http_header", err.to_string()))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| bm_sdk::Error::config("a2a_http_header", "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("POST")
        || parts.next() != Some("/a2a/message")
        || parts.next() != Some("HTTP/1.1")
        || parts.next().is_some()
    {
        return Err(bm_sdk::Error::config(
            "a2a_http_route",
            "unsupported A2A HTTP route",
        ));
    }
    let mut content_length = None;
    let mut authorization = None;
    for line in lines.filter(|line| !line.is_empty()) {
        if line.starts_with([' ', '\t']) {
            return Err(bm_sdk::Error::config(
                "a2a_http_header",
                "folded A2A HTTP headers are forbidden",
            ));
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(bm_sdk::Error::config(
                "a2a_http_header",
                "malformed A2A HTTP header",
            ));
        };
        if name.is_empty()
            || name.bytes().any(|byte| {
                !byte.is_ascii_alphanumeric()
                    && !matches!(
                        byte,
                        b'!' | b'#'
                            ..=b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
                    )
            })
        {
            return Err(bm_sdk::Error::config(
                "a2a_http_header",
                "invalid A2A HTTP header name",
            ));
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(bm_sdk::Error::config(
                "a2a_http_header",
                "A2A HTTP transfer-encoding is forbidden",
            ));
        }
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(bm_sdk::Error::config(
                    "a2a_http_header",
                    "duplicate A2A HTTP content-length",
                ));
            }
            let value = value.trim();
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(bm_sdk::Error::config(
                    "a2a_http_header",
                    "invalid A2A HTTP content-length",
                ));
            }
            content_length = Some(value.parse::<usize>().map_err(|_| {
                bm_sdk::Error::config("a2a_http_header", "invalid A2A HTTP content-length")
            })?);
        }
        if name.eq_ignore_ascii_case("authorization") {
            if authorization.is_some() {
                return Err(bm_sdk::Error::config(
                    "a2a_http_header",
                    "duplicate A2A HTTP authorization",
                ));
            }
            authorization = Some(value.trim().to_string());
        }
    }
    let content_length = content_length.ok_or_else(|| {
        bm_sdk::Error::config("a2a_http_header", "missing A2A HTTP content-length")
    })?;
    if content_length > body_max_bytes {
        return Err(bm_sdk::Error::config(
            "a2a_http_read",
            "A2A HTTP body exceeds pinned runtime adapter budget",
        ));
    }
    Ok(A2aHttpRequestHead {
        authorization,
        content_length,
    })
}

#[cfg(feature = "bridge-http")]
fn read_a2a_http_body(stream: &mut impl Read, content_length: usize) -> bm_sdk::Result<String> {
    let mut body = vec![0_u8; content_length];
    stream
        .read_exact(&mut body)
        .map_err(|err| bm_sdk::Error::config("a2a_http_body", err.to_string()))?;
    String::from_utf8(body).map_err(|err| bm_sdk::Error::config("a2a_http_body", err.to_string()))
}

#[cfg(feature = "bridge-http")]
fn write_a2a_http_auth_error(
    stream: &mut impl Write,
    reason: &str,
    budget_report_id: &str,
) -> bm_sdk::Result<()> {
    let body = json!({"error": "unauthorized", "reason": reason}).to_string();
    write!(
        stream,
        "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\nx-bm-runtime-budget-report-id: {budget_report_id}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .and_then(|_| stream.flush())
    .map_err(|err| bm_sdk::Error::config("a2a_http_write", err.to_string()))
}

#[cfg(feature = "bridge-http")]
fn write_a2a_http_response(
    stream: &mut impl Write,
    response: A2aRuntimeResponse,
    body_max_bytes: usize,
    budget_report_id: &str,
) -> bm_sdk::Result<()> {
    let body = encode_a2a_http_response(&response, body_max_bytes)?;
    let head = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nx-bm-runtime-budget-report-id: {budget_report_id}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(&body))
        .and_then(|_| stream.flush())
        .map_err(|err| bm_sdk::Error::config("a2a_http_write", err.to_string()))
}

#[cfg(feature = "bridge-http")]
fn encode_a2a_http_response(
    response: &A2aRuntimeResponse,
    body_max_bytes: usize,
) -> bm_sdk::Result<Vec<u8>> {
    let body = serde_json::to_vec(response)
        .map_err(|error| bm_sdk::Error::config("a2a_http_response", error.to_string()))?;
    if body.len() > body_max_bytes {
        return Err(bm_sdk::Error::config(
            "a2a_http_response",
            "A2A HTTP response exceeds pinned runtime adapter budget",
        ));
    }
    Ok(body)
}

#[cfg(all(test, feature = "bridge-http"))]
mod scope_tests {
    use super::*;
    use bm_entry::{
        EntryAuthConfig, EntryBearerPrincipal, EntryIdempotencyConfig, EntryIdentity,
        EntryOperationCapability, EntryRuntime, EntryRuntimeConfig, EntryScope,
        EntryTransportConfig,
    };
    use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendConfig};

    fn native_runtime_profile() -> ProfileId {
        #[cfg(target_os = "macos")]
        {
            ProfileId::DesktopMacosStandaloneMemory
        }
        #[cfg(target_os = "windows")]
        {
            ProfileId::DesktopWindowsEmbeddedSdk
        }
        #[cfg(target_os = "linux")]
        {
            ProfileId::LinuxDeviceStandaloneMemory
        }
    }

    fn remote_runtime() -> EntryRuntime {
        let mut capability = MemoryCapabilityPolicy::strict_profile();
        capability.communication_adapter_enabled = true;
        EntryRuntime::open(EntryRuntimeConfig {
            identity: EntryIdentity {
                agent_id: "a2a-agent".to_string(),
                owner_id: "owner-default".to_string(),
            },
            scope: EntryScope {
                channel: "a2a.remote".to_string(),
                chat_id: "chat-remote".to_string(),
            },
            store: StoreBackendConfig::in_memory(native_runtime_profile())
                .expect("store config")
                .with_fsync(false),
            transports: EntryTransportConfig::all_enabled(),
            auth: EntryAuthConfig::required_bearer_principal(
                "secret-token",
                EntryBearerPrincipal::new(
                    "remote-peer-principal",
                    "owner-default",
                    EntryOperationCapability::all().iter().copied(),
                ),
            ),
            idempotency: EntryIdempotencyConfig { max_keys: 64 },
            privacy: MemoryPrivacyPolicy::standard_private_boundary(),
            capability,
        })
        .expect("entry runtime")
    }

    #[test]
    fn a2a_response_budget_accepts_exact_and_rejects_plus_one() {
        let response = A2aRuntimeResponse {
            kind: "memory_report".to_string(),
            payload: "{}".to_string(),
            permissions: vec![A2aPermission::MemoryReport],
        };
        let exact = serde_json::to_vec(&response).expect("typed response").len();

        assert_eq!(
            encode_a2a_http_response(&response, exact)
                .expect("exact response budget")
                .len(),
            exact
        );
        let error = encode_a2a_http_response(&response, exact - 1)
            .expect_err("plus-one response must fail before write");
        assert_eq!(error.stage(), "a2a_http_response");
    }

    #[test]
    fn a2a_remote_write_without_explicit_source_scope_is_rejected() {
        let runtime = remote_runtime();
        let bridge = A2aBridge::new("remote-peer");
        let lease = runtime
            .acquire_budget_lease()
            .expect("runtime budget lease");
        let auth = runtime.authenticate_remote_bearer(Some("Bearer secret-token"));
        let error = runtime
            .execute_with_budget_lease(&lease, || {
                bridge.handle_authenticated_request(
                &runtime,
                A2aRuntimeMessage::json(
                    "memory_write_candidate",
                    r#"{"name":"runtime_skill__a2a_remote","topic":"scope","title":"Remote","summary":"Remote","content":"must declare scope"}"#,
                ),
                    &lease,
                    &auth,
                    TransportMode::Bidirectional,
                    "a2a_peer",
                )
            })
            .expect_err("remote A2A write must not silently fall back to chat-1");

        assert_eq!(error.stage(), "adapter_json_command");
        assert!(error.to_string().contains("source_chat_id"), "{error}");
    }
}

#[cfg(feature = "bridge-http")]
#[derive(Deserialize)]
struct A2aHttpRequest {
    name: String,
    payload: Value,
    #[serde(default)]
    idempotency_key: Option<String>,
}

#[cfg(feature = "bridge-http")]
fn render_response(response: AdapterResponse<AdapterSdkReport>) -> String {
    match response {
        AdapterResponse::Accepted { report, .. } => {
            if let Some(governed) = report.governed_safe_report() {
                return json!({"status":"accepted","result":governed}).to_string();
            }
            match report {
                AdapterSdkReport::Recall(_) | AdapterSdkReport::Project(_) => {
                    unreachable!("governed DTO handled above")
                }
                AdapterSdkReport::FinalizeTurn(report) => json!({
                    "status": "accepted",
                    "operation": "finalize_turn",
                    "result": report,
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
            }
        }
        AdapterResponse::Rejected { reason, .. } => {
            json!({"status":"rejected","reason":reason}).to_string()
        }
        AdapterResponse::Queued { queue, .. } => {
            json!({"status":"queued","queue":queue}).to_string()
        }
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => json!({"status":"duplicated","idempotency_key":idempotency_key}).to_string(),
    }
}
