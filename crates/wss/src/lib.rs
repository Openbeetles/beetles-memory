//! WSS adapter contracts for Beetle Memory.
//!
//! Production callers cannot inject a WSS budget.
//!
//! ```compile_fail
//! use bm_wss::WssBudget;
//!
//! let _ = WssBudget {
//!     max_frame_bytes: 1,
//!     max_subscriptions: 1,
//! };
//! ```

#[cfg(all(
    feature = "server-std",
    any(
        feature = "profile-esp-standalone-memory",
        feature = "profile-esp-embedded-sdk"
    )
))]
compile_error!("bm-wss server-std is forbidden for ESP profiles; use client-compact only when the ESP profile explicitly permits a lightweight WSS client.");

use bm_adapter::AdapterOperation;

#[cfg(any(feature = "server-std", feature = "client-compact"))]
use bm_adapter::{
    decode_json_adapter_command, AdapterJsonCommandOptions, AdapterRequestIdentityOwner,
    AdapterResponse, AdapterSdkReport, TransportKind, TransportMode,
};
#[cfg(feature = "server-std")]
use bm_entry::EntryAcceptedTcpStream;
#[cfg(any(feature = "server-std", feature = "client-compact"))]
use bm_entry::{
    EntryAuthDecision, EntryLocalTransport, EntryOperationCapability, EntryRuntime,
    EntryRuntimeBudgetLease, EntryTransportContext,
};
#[cfg(any(feature = "server-std", feature = "client-compact"))]
use bm_sdk::RuntimeBudgetReport;
#[cfg(any(feature = "server-std", feature = "client-compact"))]
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "server-std", feature = "client-compact"))]
use serde_json::json;
#[cfg(feature = "server-std")]
use std::io::{Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WssMessageSpec {
    pub name: &'static str,
    pub inbound_operation: Option<AdapterOperation>,
    pub private_raw_allowed: bool,
}

const MESSAGE_SPECS: &[WssMessageSpec] = &[
    inbound("command.write", AdapterOperation::Write),
    inbound("command.finalize_turn", AdapterOperation::FinalizeTurn),
    inbound("command.recall", AdapterOperation::Recall),
    inbound("command.project", AdapterOperation::Project),
    inbound("command.inspect", AdapterOperation::Inspect),
    inbound("command.replay", AdapterOperation::Replay),
    inbound("command.long_term.list", AdapterOperation::LongTermList),
    inbound("command.long_term.detail", AdapterOperation::LongTermDetail),
    inbound("command.long_term.mutate", AdapterOperation::LongTermMutate),
    inbound("command.long_term.policy", AdapterOperation::LongTermPolicy),
    inbound(
        "command.transcript.attrs",
        AdapterOperation::TranscriptAttrWrite,
    ),
    inbound("command.capabilities", AdapterOperation::Capabilities),
    stream("subscribe.projection"),
    stream("subscribe.inspection"),
    stream("subscribe.replay"),
    stream("subscribe.capability"),
    stream("event.report"),
    stream("event.lifecycle"),
    stream("event.error"),
];

const fn inbound(name: &'static str, operation: AdapterOperation) -> WssMessageSpec {
    WssMessageSpec {
        name,
        inbound_operation: Some(operation),
        private_raw_allowed: false,
    }
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct WssRuntimeFrame {
    pub kind: String,
    pub payload: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
impl WssRuntimeFrame {
    pub fn command(kind: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            payload: payload.into(),
            idempotency_key: None,
        }
    }

    pub fn with_idempotency_key(mut self, idempotency_key: impl Into<String>) -> Self {
        self.idempotency_key = Some(idempotency_key.into());
        self
    }

    pub fn subscribe(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            payload: String::new(),
            idempotency_key: None,
        }
    }
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct WssRuntimeEvent {
    pub kind: String,
    pub payload: String,
    pub private_raw_allowed: bool,
    pub budget_report_id: String,
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WssLocalSessionIdentity {
    principal: String,
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
impl WssLocalSessionIdentity {
    pub fn in_process(principal: impl Into<String>) -> Self {
        Self {
            principal: principal.into(),
        }
    }
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
pub struct WssRuntimeSession<'runtime> {
    runtime: &'runtime EntryRuntime,
    session_id: String,
    auth: EntryAuthDecision,
    request_identity_owner: AdapterRequestIdentityOwner,
    subscriptions: Vec<String>,
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
impl<'runtime> WssRuntimeSession<'runtime> {
    fn with_auth(
        runtime: &'runtime EntryRuntime,
        session_id: impl Into<String>,
        auth: EntryAuthDecision,
    ) -> Self {
        let session_id = session_id.into();
        Self {
            runtime,
            request_identity_owner: AdapterRequestIdentityOwner::new(
                TransportKind::Wss,
                session_id.clone(),
                auth.principal_id(),
            ),
            auth,
            session_id,
            subscriptions: Vec::new(),
        }
    }

    pub fn new(
        runtime: &'runtime EntryRuntime,
        session_id: impl Into<String>,
        identity: WssLocalSessionIdentity,
    ) -> Self {
        Self::for_local_in_process(runtime, session_id, &identity.principal)
    }

    pub fn for_local_in_process(
        runtime: &'runtime EntryRuntime,
        session_id: impl Into<String>,
        principal: &str,
    ) -> Self {
        let auth = runtime.authenticate_local_transport(EntryLocalTransport::InProcess, principal);
        Self::with_auth(runtime, session_id, auth)
    }

    pub fn handle_frame(&mut self, frame: WssRuntimeFrame) -> bm_sdk::Result<WssRuntimeEvent> {
        let runtime = self.runtime;
        let lease = acquire_runtime_budget_lease(runtime)?;
        runtime.execute_with_budget_lease(&lease, || {
            self.handle_frame_with_budget_lease(frame, &lease)
        })
    }

    fn handle_frame_with_budget_lease(
        &mut self,
        frame: WssRuntimeFrame,
        lease: &EntryRuntimeBudgetLease,
    ) -> bm_sdk::Result<WssRuntimeEvent> {
        let budget_report = lease.report();
        let frame_material_bytes = frame
            .payload
            .len()
            .saturating_add(frame.idempotency_key.as_ref().map_or(0, |key| key.len()));
        if frame_material_bytes > budget_report.adapter_budget.wss_frame_max_bytes {
            return Ok(error_event("frame_budget_exceeded", budget_report));
        }
        if !self.auth.is_authenticated() || self.auth.principal_id().is_empty() {
            return Ok(error_event("authentication_required", budget_report));
        }
        if frame.kind.starts_with("subscribe.") {
            if !self.auth.allows(EntryOperationCapability::Subscribe) {
                return Ok(error_event("subscription_not_authorized", budget_report));
            }
            return Ok(self.subscribe(frame.kind, budget_report));
        }
        let operation = message_specs()
            .iter()
            .find(|spec| spec.name == frame.kind)
            .and_then(|spec| spec.inbound_operation)
            .ok_or_else(|| bm_sdk::Error::config("wss_runtime", "unsupported frame kind"))?;
        let request_identity = self
            .request_identity_owner
            .issue(frame.idempotency_key.as_deref())
            .map_err(|error| bm_sdk::Error::config("wss_request_identity", error.to_string()))?;
        reject_missing_remote_source_scope(self.runtime, operation, &frame.payload)?;
        let command = decode_json_adapter_command(
            operation,
            &frame.payload,
            &wss_command_options(self.runtime),
        )?;
        let response = self.runtime.handle_with_budget_lease(
            EntryTransportContext::new(
                request_identity.request_id,
                TransportKind::Wss,
                TransportMode::Bidirectional,
                operation,
                self.session_id.clone(),
                "wss_peer",
                request_identity.mutation_operation_id.unwrap_or_default(),
                request_identity.audit_id,
                self.auth.clone(),
            ),
            command,
            lease,
        )?;
        if response.budget_report != *budget_report {
            return Err(bm_sdk::Error::config(
                "wss_runtime_budget",
                "entry_response_budget_lease_identity_mismatch",
            ));
        }
        Ok(bind_wss_event_budget(
            WssRuntimeEvent {
                kind: "event.report".to_string(),
                payload: render_response(response.adapter),
                private_raw_allowed: false,
                budget_report_id: String::new(),
            },
            budget_report,
        ))
    }

    fn subscribe(&mut self, kind: String, budget_report: &RuntimeBudgetReport) -> WssRuntimeEvent {
        if self.subscriptions.len() >= budget_report.adapter_budget.wss_max_subscriptions {
            return error_event("subscription_budget_exceeded", budget_report);
        }
        let allowed = message_specs()
            .iter()
            .any(|spec| spec.name == kind && spec.inbound_operation.is_none());
        if !allowed {
            return error_event("unsupported_subscription", budget_report);
        }
        self.subscriptions.push(kind.clone());
        bind_wss_event_budget(
            WssRuntimeEvent {
                kind: "event.lifecycle".to_string(),
                payload: json!({
                    "status": "subscribed",
                    "subscription": kind,
                    "private_raw_allowed": false,
                })
                .to_string(),
                private_raw_allowed: false,
                budget_report_id: String::new(),
            },
            budget_report,
        )
    }
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
fn wss_command_options(runtime: &EntryRuntime) -> AdapterJsonCommandOptions {
    let options = AdapterJsonCommandOptions::new("bm-wss");
    if runtime.uses_local_default_scope_policy() {
        options.with_default_source_chat_id(runtime.runtime().scope().chat_id.clone())
    } else {
        options
    }
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
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
    if value
        .get("source_chat_id")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(());
    }
    Err(bm_sdk::Error::config(
        "adapter_json_command",
        "remote adapter write payload missing source_chat_id; refusing implicit chat-1 scope",
    ))
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
fn acquire_runtime_budget_lease(runtime: &EntryRuntime) -> bm_sdk::Result<EntryRuntimeBudgetLease> {
    runtime.acquire_budget_lease()
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
fn bind_wss_event_budget(
    mut event: WssRuntimeEvent,
    report: &RuntimeBudgetReport,
) -> WssRuntimeEvent {
    event.budget_report_id.clone_from(&report.report_id);
    if let Ok(mut payload) = serde_json::from_str::<serde_json::Value>(&event.payload) {
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "runtime_budget_report_id".to_string(),
                json!(report.report_id),
            );
            event.payload = payload.to_string();
        }
    }
    event
}

#[cfg(feature = "server-std")]
pub fn serve_wss_accepted_stream(
    runtime: &EntryRuntime,
    stream: &mut EntryAcceptedTcpStream,
    session_id: impl Into<String>,
) -> bm_sdk::Result<()> {
    let lease = acquire_runtime_budget_lease(runtime)?;
    let budget_report = lease.report();
    let adapter_budget = &budget_report.adapter_budget;
    let handshake = read_websocket_handshake(stream, adapter_budget.http_header_max_bytes)?;
    if !wss_origin_allowed(handshake.origin.as_deref(), &handshake.host) {
        return Err(bm_sdk::Error::config(
            "wss_handshake_origin",
            "websocket Origin is not an exact local authority",
        ));
    }
    let auth = runtime.authenticate_accepted_tcp_stream(
        stream,
        handshake.authorization.as_deref(),
        "wss-loopback",
    );
    if !auth.is_authenticated() {
        return Err(bm_sdk::Error::config(
            "wss_handshake_auth",
            auth.rejection_reason().unwrap_or("authentication failed"),
        ));
    }
    let accept = websocket_accept_key(&handshake.key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\nx-bm-runtime-budget-report-id: {}\r\n\r\n",
        budget_report.report_id,
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|err| bm_sdk::Error::config("wss_handshake_write", err.to_string()))?;
    stream
        .flush()
        .map_err(|err| bm_sdk::Error::config("wss_handshake_flush", err.to_string()))?;

    let mut session = WssRuntimeSession::with_auth(runtime, session_id, auth);
    runtime.execute_with_budget_lease(&lease, || {
        for _ in 0..adapter_budget.wss_session_max_frames {
            match read_client_frame(stream, adapter_budget.wss_frame_max_bytes)? {
                WssClientFrame::Text(text) => {
                    let frame: WssRuntimeFrame = serde_json::from_str(&text)
                        .map_err(|err| bm_sdk::Error::config("wss_frame_json", err.to_string()))?;
                    let event = session.handle_frame_with_budget_lease(frame, &lease)?;
                    let event = serde_json::to_string(&event).map_err(|error| {
                        bm_sdk::Error::config("wss_event_json", error.to_string())
                    })?;
                    write_server_text_frame(stream, &event, adapter_budget.wss_frame_max_bytes)?;
                }
                WssClientFrame::Ping(payload) => {
                    write_server_control_frame(stream, 0xA, &payload)?;
                }
                WssClientFrame::Pong => {}
                WssClientFrame::Close(payload) => {
                    write_server_control_frame(stream, 0x8, &payload)?;
                    return Ok(());
                }
            }
        }
        write_server_control_frame(stream, 0x8, &[0x03, 0xF0])
    })
}

#[cfg(feature = "server-std")]
#[derive(Debug)]
struct WssHandshake {
    key: String,
    host: String,
    origin: Option<String>,
    authorization: Option<String>,
}

#[cfg(feature = "server-std")]
fn read_websocket_handshake(
    stream: &mut impl Read,
    max_header_bytes: usize,
) -> bm_sdk::Result<WssHandshake> {
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    while !buffer.ends_with(b"\r\n\r\n") {
        stream
            .read_exact(&mut byte)
            .map_err(|err| bm_sdk::Error::config("wss_handshake_read", err.to_string()))?;
        if buffer.len() == max_header_bytes {
            return Err(bm_sdk::Error::config(
                "wss_handshake_read",
                "websocket handshake exceeds budget",
            ));
        }
        buffer.push(byte[0]);
    }
    let request = String::from_utf8(buffer)
        .map_err(|err| bm_sdk::Error::config("wss_handshake_utf8", err.to_string()))?;
    let mut lines = request.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| bm_sdk::Error::config("wss_handshake", "missing request line"))?;
    let mut request_parts = request_line.split(' ');
    if request_parts.next() != Some("GET")
        || request_parts.next() != Some("/memory/ws")
        || request_parts.next() != Some("HTTP/1.1")
        || request_parts.next().is_some()
    {
        return Err(bm_sdk::Error::config(
            "wss_handshake",
            "unsupported websocket request line",
        ));
    }
    let mut headers = std::collections::BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        if line.starts_with([' ', '\t']) {
            return Err(bm_sdk::Error::config(
                "wss_handshake",
                "folded websocket headers are forbidden",
            ));
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| bm_sdk::Error::config("wss_handshake", "malformed websocket header"))?;
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
                "wss_handshake",
                "invalid websocket header name",
            ));
        }
        let name = name.to_ascii_lowercase();
        if headers.insert(name, value.trim().to_string()).is_some() {
            return Err(bm_sdk::Error::config(
                "wss_handshake",
                "duplicate websocket header",
            ));
        }
    }
    if headers.contains_key("transfer-encoding") || headers.contains_key("content-length") {
        return Err(bm_sdk::Error::config(
            "wss_handshake",
            "websocket upgrade request bodies are forbidden",
        ));
    }
    if headers.get("host").is_none_or(|value| value.is_empty()) {
        return Err(bm_sdk::Error::config(
            "wss_handshake",
            "missing websocket host",
        ));
    }
    if headers
        .get("upgrade")
        .is_none_or(|value| !value.eq_ignore_ascii_case("websocket"))
        || headers.get("connection").is_none_or(|value| {
            !value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        })
    {
        return Err(bm_sdk::Error::config(
            "wss_handshake",
            "missing websocket upgrade",
        ));
    }
    if headers.get("sec-websocket-version").map(String::as_str) != Some("13") {
        return Err(bm_sdk::Error::config(
            "wss_handshake",
            "unsupported websocket version",
        ));
    }
    let websocket_key = headers
        .get("sec-websocket-key")
        .ok_or_else(|| bm_sdk::Error::config("wss_handshake", "missing websocket key"))?;
    validate_websocket_key(websocket_key)?;
    Ok(WssHandshake {
        key: websocket_key.clone(),
        host: headers["host"].clone(),
        origin: headers.get("origin").cloned(),
        authorization: headers.get("authorization").cloned(),
    })
}

#[cfg(feature = "server-std")]
fn wss_origin_allowed(origin: Option<&str>, host: &str) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    if !is_local_authority(host) {
        return false;
    }
    origin == format!("http://{host}") || origin == format!("https://{host}")
}

#[cfg(feature = "server-std")]
fn is_local_authority(authority: &str) -> bool {
    if authority.is_empty()
        || authority
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'?' | b'#' | b'@'))
    {
        return false;
    }
    let host = if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return false;
        };
        if !suffix.is_empty() && !valid_authority_port(suffix.strip_prefix(':')) {
            return false;
        }
        host
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if !valid_authority_port(Some(port)) {
            return false;
        }
        host
    } else {
        authority
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

#[cfg(feature = "server-std")]
fn valid_authority_port(port: Option<&str>) -> bool {
    port.is_some_and(|port| {
        !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|port| port != 0)
    })
}

#[cfg(feature = "server-std")]
enum WssClientFrame {
    Text(String),
    Ping(Vec<u8>),
    Pong,
    Close(Vec<u8>),
}

#[cfg(feature = "server-std")]
fn read_client_frame(
    stream: &mut impl Read,
    max_frame_bytes: usize,
) -> bm_sdk::Result<WssClientFrame> {
    let mut header = [0_u8; 2];
    stream
        .read_exact(&mut header)
        .map_err(|err| bm_sdk::Error::config("wss_frame_header", err.to_string()))?;
    if header[0] & 0x80 == 0 || header[0] & 0x70 != 0 {
        return Err(bm_sdk::Error::config(
            "wss_frame",
            "fragmented or RSV websocket frames are forbidden",
        ));
    }
    let opcode = header[0] & 0x0f;
    if !matches!(opcode, 0x01 | 0x08 | 0x09 | 0x0A) {
        return Err(bm_sdk::Error::config(
            "wss_frame",
            "unsupported websocket opcode",
        ));
    }
    let masked = (header[1] & 0x80) != 0;
    if !masked {
        return Err(bm_sdk::Error::config(
            "wss_frame",
            "client frames must be masked",
        ));
    }
    let mut len = (header[1] & 0x7f) as usize;
    if len == 126 {
        let mut extended = [0_u8; 2];
        stream
            .read_exact(&mut extended)
            .map_err(|err| bm_sdk::Error::config("wss_frame_len", err.to_string()))?;
        len = u16::from_be_bytes(extended) as usize;
        if len < 126 {
            return Err(bm_sdk::Error::config(
                "wss_frame",
                "non-canonical websocket frame length",
            ));
        }
    } else if len == 127 {
        let mut extended = [0_u8; 8];
        stream
            .read_exact(&mut extended)
            .map_err(|err| bm_sdk::Error::config("wss_frame_len", err.to_string()))?;
        let encoded = u64::from_be_bytes(extended);
        if encoded <= u16::MAX as u64 || encoded & (1_u64 << 63) != 0 {
            return Err(bm_sdk::Error::config(
                "wss_frame",
                "non-canonical websocket frame length",
            ));
        }
        len = usize::try_from(encoded).map_err(|_| {
            bm_sdk::Error::config("wss_frame", "websocket frame length exceeds address space")
        })?;
    }
    if len > max_frame_bytes {
        return Err(bm_sdk::Error::config(
            "wss_frame",
            "websocket frame exceeds budget",
        ));
    }
    if matches!(opcode, 0x08..=0x0A) && len > 125 {
        return Err(bm_sdk::Error::config(
            "wss_frame",
            "websocket control frame exceeds 125 bytes",
        ));
    }
    let mut mask = [0_u8; 4];
    stream
        .read_exact(&mut mask)
        .map_err(|err| bm_sdk::Error::config("wss_frame_mask", err.to_string()))?;
    let mut payload = vec![0_u8; len];
    stream
        .read_exact(&mut payload)
        .map_err(|err| bm_sdk::Error::config("wss_frame_payload", err.to_string()))?;
    for (idx, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[idx % 4];
    }
    match opcode {
        0x01 => String::from_utf8(payload)
            .map(WssClientFrame::Text)
            .map_err(|err| bm_sdk::Error::config("wss_frame_utf8", err.to_string())),
        0x08 => {
            if payload.len() == 1 {
                return Err(bm_sdk::Error::config(
                    "wss_frame",
                    "websocket close payload must be empty or contain a status code",
                ));
            }
            if payload.len() > 2 {
                std::str::from_utf8(&payload[2..])
                    .map_err(|err| bm_sdk::Error::config("wss_frame_utf8", err.to_string()))?;
            }
            Ok(WssClientFrame::Close(payload))
        }
        0x09 => Ok(WssClientFrame::Ping(payload)),
        0x0A => Ok(WssClientFrame::Pong),
        _ => unreachable!("validated websocket opcode"),
    }
}

#[cfg(all(feature = "server-std", test))]
fn read_client_text_frame(
    stream: &mut impl Read,
    max_frame_bytes: usize,
) -> bm_sdk::Result<String> {
    match read_client_frame(stream, max_frame_bytes)? {
        WssClientFrame::Text(text) => Ok(text),
        _ => Err(bm_sdk::Error::config(
            "wss_frame",
            "expected websocket text frame",
        )),
    }
}

#[cfg(feature = "server-std")]
fn write_server_text_frame(
    stream: &mut impl Write,
    text: &str,
    max_frame_bytes: usize,
) -> bm_sdk::Result<()> {
    let payload = text.as_bytes();
    if payload.len() > max_frame_bytes {
        return Err(bm_sdk::Error::config(
            "wss_frame_write",
            "server websocket frame exceeds runtime budget",
        ));
    }
    let mut frame = vec![0x81];
    if payload.len() < 126 {
        frame.push(payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        frame.push(126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        return Err(bm_sdk::Error::config(
            "wss_frame_write",
            "server frame exceeds supported length",
        ));
    }
    frame.extend_from_slice(payload);
    stream
        .write_all(&frame)
        .and_then(|_| stream.flush())
        .map_err(|err| bm_sdk::Error::config("wss_frame_write", err.to_string()))
}

#[cfg(feature = "server-std")]
fn write_server_control_frame(
    stream: &mut impl Write,
    opcode: u8,
    payload: &[u8],
) -> bm_sdk::Result<()> {
    if !matches!(opcode, 0x08..=0x0A) || payload.len() > 125 {
        return Err(bm_sdk::Error::config(
            "wss_frame_write",
            "invalid websocket control frame",
        ));
    }
    let mut frame = Vec::with_capacity(payload.len() + 2);
    frame.push(0x80 | opcode);
    frame.push(payload.len() as u8);
    frame.extend_from_slice(payload);
    stream
        .write_all(&frame)
        .and_then(|_| stream.flush())
        .map_err(|err| bm_sdk::Error::config("wss_frame_write", err.to_string()))
}

#[cfg(feature = "server-std")]
fn websocket_accept_key(key: &str) -> String {
    let mut input = Vec::with_capacity(key.len() + 36);
    input.extend_from_slice(key.as_bytes());
    input.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64_encode(&sha1_digest(&input))
}

#[cfg(feature = "server-std")]
fn validate_websocket_key(key: &str) -> bm_sdk::Result<()> {
    let bytes = key.as_bytes();
    let valid = bytes.len() == 24
        && bytes[22..] == *b"=="
        && bytes[..22]
            .iter()
            .copied()
            .all(|byte| base64_value(byte).is_some())
        && base64_value(bytes[21]).is_some_and(|value| value & 0x0f == 0);
    if !valid {
        return Err(bm_sdk::Error::config(
            "wss_handshake",
            "websocket key must be canonical base64 for 16 bytes",
        ));
    }
    Ok(())
}

#[cfg(feature = "server-std")]
const fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(feature = "server-std")]
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut idx = 0;
    while idx < bytes.len() {
        let b0 = bytes[idx];
        let b1 = bytes.get(idx + 1).copied().unwrap_or(0);
        let b2 = bytes.get(idx + 2).copied().unwrap_or(0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if idx + 1 < bytes.len() {
            out.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if idx + 2 < bytes.len() {
            out.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        idx += 3;
    }
    out
}

#[cfg(feature = "server-std")]
fn sha1_digest(bytes: &[u8]) -> [u8; 20] {
    let mut h0 = 0x6745_2301_u32;
    let mut h1 = 0xefcd_ab89_u32;
    let mut h2 = 0x98ba_dcfe_u32;
    let mut h3 = 0x1032_5476_u32;
    let mut h4 = 0xc3d2_e1f0_u32;

    let bit_len = (bytes.len() as u64) * 8;
    let mut message = bytes.to_vec();
    message.push(0x80);
    while (message.len() % 64) != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks(64) {
        let mut w = [0_u32; 80];
        for (idx, word) in w.iter_mut().take(16).enumerate() {
            let offset = idx * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for idx in 16..80 {
            w[idx] = (w[idx - 3] ^ w[idx - 8] ^ w[idx - 14] ^ w[idx - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (idx, word) in w.iter().enumerate() {
            let (f, k) = match idx {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0_u8; 20];
    out[..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
fn render_response(response: AdapterResponse<AdapterSdkReport>) -> String {
    match response {
        AdapterResponse::Accepted {
            report, receipt, ..
        } => {
            if let Some(governed) = report.governed_safe_report() {
                return omit_null_receipt(
                    json!({"status":"accepted","result":governed,"receipt":receipt}).to_string(),
                );
            }
            omit_null_receipt(match report {
                AdapterSdkReport::Recall(_) | AdapterSdkReport::Project(_) => {
                    unreachable!("governed DTO handled above")
                }
                AdapterSdkReport::Capabilities(report) => json!({
                    "status": "accepted",
                    "profile": report.profile.as_str(),
                    "capabilities": report.capabilities,
                    "sdk_mutation_inventory": report.sdk_mutation_inventory,
                })
                .to_string(),
                AdapterSdkReport::FinalizeTurn(report) => json!({
                    "status": "accepted",
                    "operation": "finalize_turn",
                    "result": report,
                    "receipt": receipt,
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
                    "receipt": receipt,
                })
                .to_string(),
                other => json!({
                    "status": "accepted",
                    "report_kind": other.public_kind(),
                    "receipt": receipt,
                })
                .to_string(),
            })
        }
        AdapterResponse::Rejected { reason, .. } => {
            json!({"status":"rejected","reason":reason}).to_string()
        }
        AdapterResponse::Queued { queue, .. } => {
            json!({"status":"queued","queue":queue}).to_string()
        }
        AdapterResponse::Replayed {
            mutation_operation_id,
            receipt,
            ..
        } => json!({"status":"replayed","mutation_operation_id":mutation_operation_id,"receipt":receipt}).to_string(),
    }
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
fn omit_null_receipt(rendered: String) -> String {
    let mut value: serde_json::Value =
        serde_json::from_str(&rendered).expect("adapter response renderer emits valid JSON");
    if value.get("receipt").is_some_and(serde_json::Value::is_null) {
        value
            .as_object_mut()
            .expect("adapter response renderer emits an object")
            .remove("receipt");
    }
    value.to_string()
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
fn error_event(reason: &str, report: &RuntimeBudgetReport) -> WssRuntimeEvent {
    bind_wss_event_budget(
        WssRuntimeEvent {
            kind: "event.error".to_string(),
            payload: json!({
                "status": "rejected",
                "reason": reason,
            })
            .to_string(),
            private_raw_allowed: false,
            budget_report_id: String::new(),
        },
        report,
    )
}

const fn stream(name: &'static str) -> WssMessageSpec {
    WssMessageSpec {
        name,
        inbound_operation: None,
        private_raw_allowed: false,
    }
}

pub const fn message_specs() -> &'static [WssMessageSpec] {
    MESSAGE_SPECS
}

#[cfg(all(test, feature = "server-std"))]
mod tests {
    use std::io::Cursor;

    use super::{
        read_client_text_frame, read_websocket_handshake, validate_websocket_key,
        write_server_text_frame, wss_origin_allowed,
    };

    #[test]
    fn server_writer_rejects_payload_over_runtime_frame_budget_before_writing() {
        let mut stream = Vec::new();

        let error = write_server_text_frame(&mut stream, "12345", 4)
            .expect_err("outbound frame must respect runtime budget");

        assert_eq!(error.stage(), "wss_frame_write");
        assert!(error.to_string().contains("budget"), "{error}");
        assert!(stream.is_empty());
    }

    #[test]
    fn handshake_rejects_ambiguous_framing_and_duplicate_security_headers() {
        for request in [
            "GET /memory/ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nTransfer-Encoding: chunked\r\n\r\n",
            "GET /memory/ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        ] {
            let mut stream = Cursor::new(request.as_bytes());
            let error = read_websocket_handshake(&mut stream, 4096)
                .expect_err("ambiguous websocket handshake must fail closed");
            assert_eq!(error.stage(), "wss_handshake");
        }
    }

    #[test]
    fn websocket_key_requires_canonical_base64_for_exactly_sixteen_bytes() {
        validate_websocket_key("dGhlIHNhbXBsZSBub25jZQ==").expect("canonical 16-byte nonce");

        for invalid in [
            "dGhlIHNhbXBsZSBub25jZR==",
            "dGhlIHNhbXBsZSBub25jZQ=",
            "dGhlIHNhbXBsZSBub25jZQAA",
        ] {
            let error = validate_websocket_key(invalid)
                .expect_err("non-canonical or wrong-length nonce must fail closed");
            assert_eq!(error.stage(), "wss_handshake");
        }
    }

    #[test]
    fn websocket_origin_requires_an_exact_local_handshake_authority() {
        assert!(wss_origin_allowed(None, "localhost:8787"));
        assert!(wss_origin_allowed(
            Some("http://localhost:8787"),
            "localhost:8787"
        ));
        for (origin, host) in [
            ("http://localhost.evil:8787", "localhost:8787"),
            ("http://user@localhost:8787", "localhost:8787"),
            ("http://localhost:8787/path", "localhost:8787"),
            ("http://localhost:8787", "example.test:8787"),
        ] {
            assert!(!wss_origin_allowed(Some(origin), host));
        }
    }

    #[test]
    fn client_frame_budget_accepts_exact_and_rejects_plus_one_before_payload_read() {
        let mut exact = vec![0x81, 0x80 | 4, 1, 2, 3, 4];
        exact.extend([b't' ^ 1, b'e' ^ 2, b's' ^ 3, b't' ^ 4]);
        assert_eq!(
            read_client_text_frame(&mut Cursor::new(exact), 4).expect("exact frame"),
            "test"
        );

        let mut over = Cursor::new(vec![0x81, 0x80 | 5]);
        let error = read_client_text_frame(&mut over, 4)
            .expect_err("plus-one frame must fail before mask or payload read");
        assert_eq!(error.stage(), "wss_frame");
        assert_eq!(over.position(), 2);
    }

    #[test]
    fn client_frame_rejects_fragmented_rsv_and_noncanonical_lengths() {
        for bytes in [
            vec![0x01, 0x80],
            vec![0xc1, 0x80],
            vec![0x81, 0x80 | 126, 0, 125],
        ] {
            let error = read_client_text_frame(&mut Cursor::new(bytes), 1024)
                .expect_err("ambiguous frame must fail closed");
            assert_eq!(error.stage(), "wss_frame");
        }
    }
}
