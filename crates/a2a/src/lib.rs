//! A2A bridge contracts for Beetle Memory.

use bm_adapter::AdapterOperation;

#[cfg(feature = "bridge-http")]
use bm_adapter::{
    decode_json_adapter_command, AdapterJsonCommandOptions, AdapterResponse, AdapterSdkReport,
    TransportKind, TransportMode,
};
#[cfg(feature = "bridge-http")]
use bm_entry::{EntryAuthDecision, EntryRuntime, EntryTransportContext};
#[cfg(feature = "bridge-http")]
use serde::Deserialize;
#[cfg(feature = "bridge-http")]
use serde_json::{json, Value};
#[cfg(feature = "bridge-http")]
use std::io::{Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        message("memory_recall_request", Some(AdapterOperation::Recall)),
        message("memory_projection_request", Some(AdapterOperation::Project)),
        message("memory_report", None),
        message("memory_migration_chunk", Some(AdapterOperation::Import)),
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
}

#[cfg(feature = "bridge-http")]
impl A2aRuntimeMessage {
    pub fn json(name: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            payload: payload.into(),
        }
    }
}

#[cfg(feature = "bridge-http")]
#[derive(Clone, Debug, PartialEq, Eq)]
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

    pub fn handle(
        &self,
        runtime: &EntryRuntime,
        message: A2aRuntimeMessage,
    ) -> bm_sdk::Result<A2aRuntimeResponse> {
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
        let response = runtime.handle(
            EntryTransportContext {
                request_id: format!("a2a-{}-{}", self.bridge_id, spec.name),
                transport: TransportKind::A2a,
                mode: TransportMode::Bidirectional,
                operation,
                source_id: self.bridge_id.clone(),
                source_kind: "a2a_peer".to_string(),
                idempotency_key: format!("a2a-{}-{}", self.bridge_id, spec.name),
                audit_id: format!("audit-a2a-{}-{}", self.bridge_id, spec.name),
                auth: EntryAuthDecision::authenticated("a2a", "peer"),
            },
            command,
        )?;
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
pub fn serve_a2a_http_stream(
    runtime: &EntryRuntime,
    bridge: &A2aBridge,
    stream: &mut (impl Read + Write),
) -> bm_sdk::Result<()> {
    let body = read_a2a_http_body(stream)?;
    let request: A2aHttpRequest = serde_json::from_str(&body)
        .map_err(|err| bm_sdk::Error::config("a2a_http_json", err.to_string()))?;
    let response = bridge.handle(
        runtime,
        A2aRuntimeMessage::json(request.name, request.payload.to_string()),
    )?;
    write_a2a_http_response(stream, response)
}

#[cfg(feature = "bridge-http")]
fn read_a2a_http_body(stream: &mut impl Read) -> bm_sdk::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|err| bm_sdk::Error::config("a2a_http_read", err.to_string()))?;
        if read == 0 {
            break find_header_end(&buffer)
                .ok_or_else(|| bm_sdk::Error::config("a2a_http_read", "missing HTTP headers"))?;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(pos) = find_header_end(&buffer) {
            break pos;
        }
        if buffer.len() > 72 * 1024 {
            return Err(bm_sdk::Error::config(
                "a2a_http_read",
                "HTTP request too large",
            ));
        }
    };
    let header_text = std::str::from_utf8(&buffer[..header_end])
        .map_err(|err| bm_sdk::Error::config("a2a_http_header", err.to_string()))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| bm_sdk::Error::config("a2a_http_header", "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("POST") || parts.next() != Some("/a2a/message") {
        return Err(bm_sdk::Error::config(
            "a2a_http_route",
            "unsupported A2A HTTP route",
        ));
    }
    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = stream
            .read(&mut chunk)
            .map_err(|err| bm_sdk::Error::config("a2a_http_body", err.to_string()))?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    if buffer.len() < body_start + content_length {
        return Err(bm_sdk::Error::config("a2a_http_body", "truncated body"));
    }
    String::from_utf8(buffer[body_start..body_start + content_length].to_vec())
        .map_err(|err| bm_sdk::Error::config("a2a_http_body", err.to_string()))
}

#[cfg(feature = "bridge-http")]
fn write_a2a_http_response(
    stream: &mut impl Write,
    response: A2aRuntimeResponse,
) -> bm_sdk::Result<()> {
    let body = json!({
        "kind": response.kind,
        "payload": response.payload,
        "permissions": response.permissions.into_iter().map(permission_name).collect::<Vec<_>>(),
    })
    .to_string();
    let head = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body.as_bytes()))
        .and_then(|_| stream.flush())
        .map_err(|err| bm_sdk::Error::config("a2a_http_write", err.to_string()))
}

#[cfg(all(test, feature = "bridge-http"))]
mod scope_tests {
    use super::*;
    use bm_entry::{
        EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
        EntryScope, EntryStoreConfig, EntryTransportConfig,
    };
    use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

    fn remote_runtime() -> EntryRuntime {
        let mut capability = MemoryCapabilityPolicy::strict_profile();
        capability.communication_adapter_enabled = true;
        EntryRuntime::open(EntryRuntimeConfig {
            profile: ProfileId::ServerLinuxDevFull,
            identity: EntryIdentity {
                agent_id: "a2a-agent".to_string(),
                owner_id: "owner-default".to_string(),
            },
            scope: EntryScope {
                channel: "a2a.remote".to_string(),
                chat_id: "chat-remote".to_string(),
            },
            store: EntryStoreConfig {
                backend: StoreBackendKind::InMemory,
                data_path: None,
                fsync: false,
            },
            transports: EntryTransportConfig::all_enabled(),
            auth: EntryAuthConfig::required_bearer_token("secret-token"),
            idempotency: EntryIdempotencyConfig { max_keys: 64 },
            privacy: MemoryPrivacyPolicy::standard_private_boundary(),
            capability,
        })
        .expect("entry runtime")
    }

    #[test]
    fn a2a_remote_write_without_explicit_source_scope_is_rejected() {
        let runtime = remote_runtime();
        let bridge = A2aBridge::new("remote-peer");
        let error = bridge
            .handle(
                &runtime,
                A2aRuntimeMessage::json(
                    "memory_write_candidate",
                    r#"{"name":"runtime_skill__a2a_remote","topic":"scope","title":"Remote","summary":"Remote","content":"must declare scope"}"#,
                ),
            )
            .expect_err("remote A2A write must not silently fall back to chat-1");

        assert_eq!(error.stage(), "adapter_json_command");
        assert!(error.to_string().contains("source_chat_id"), "{error}");
    }
}

#[cfg(feature = "bridge-http")]
fn permission_name(permission: A2aPermission) -> &'static str {
    match permission {
        A2aPermission::MemoryReport => "MemoryReport",
        A2aPermission::Executor => "Executor",
        A2aPermission::Tool => "Tool",
        A2aPermission::Workflow => "Workflow",
    }
}

#[cfg(feature = "bridge-http")]
fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

#[cfg(feature = "bridge-http")]
#[derive(Deserialize)]
struct A2aHttpRequest {
    name: String,
    payload: Value,
}

#[cfg(feature = "bridge-http")]
fn render_response(response: AdapterResponse<AdapterSdkReport>) -> String {
    match response {
        AdapterResponse::Accepted { report, .. } => match report {
            AdapterSdkReport::Recall(report) => json!({
                "status": "accepted",
                "query": report.query,
                "procedural_hits": report.procedural_hits.len(),
            })
            .to_string(),
            other => json!({"status":"accepted","report":format!("{other:?}")}).to_string(),
        },
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
