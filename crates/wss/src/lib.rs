//! WSS adapter contracts for Beetle Memory.

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
    decode_json_adapter_command, AdapterJsonCommandOptions, AdapterResponse, AdapterSdkReport,
    TransportKind, TransportMode,
};
#[cfg(any(feature = "server-std", feature = "client-compact"))]
use bm_entry::{EntryAuthDecision, EntryRuntime, EntryTransportContext};
#[cfg(any(feature = "server-std", feature = "client-compact"))]
use serde::Deserialize;
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
    inbound("command.recall", AdapterOperation::Recall),
    inbound("command.project", AdapterOperation::Project),
    inbound("command.inspect", AdapterOperation::Inspect),
    inbound("command.replay", AdapterOperation::Replay),
    inbound("command.long_term.list", AdapterOperation::LongTermList),
    inbound("command.long_term.detail", AdapterOperation::LongTermDetail),
    inbound("command.long_term.mutate", AdapterOperation::LongTermMutate),
    inbound("command.long_term.policy", AdapterOperation::LongTermPolicy),
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
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
impl WssRuntimeFrame {
    pub fn command(kind: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            payload: payload.into(),
        }
    }

    pub fn subscribe(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            payload: String::new(),
        }
    }
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WssRuntimeEvent {
    pub kind: String,
    pub payload: String,
    pub private_raw_allowed: bool,
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
pub struct WssRuntimeSession {
    session_id: String,
    budget: WssBudget,
    subscriptions: Vec<String>,
}

#[cfg(any(feature = "server-std", feature = "client-compact"))]
impl WssRuntimeSession {
    pub fn new(session_id: impl Into<String>, budget: WssBudget) -> Self {
        Self {
            session_id: session_id.into(),
            budget,
            subscriptions: Vec::new(),
        }
    }

    pub fn handle_frame(
        &mut self,
        runtime: &EntryRuntime,
        frame: WssRuntimeFrame,
    ) -> bm_sdk::Result<WssRuntimeEvent> {
        if frame.payload.len() > self.budget.max_frame_bytes {
            return Ok(error_event("frame_budget_exceeded"));
        }
        if frame.kind.starts_with("subscribe.") {
            return Ok(self.subscribe(frame.kind));
        }
        let operation = message_specs()
            .iter()
            .find(|spec| spec.name == frame.kind)
            .and_then(|spec| spec.inbound_operation)
            .ok_or_else(|| bm_sdk::Error::config("wss_runtime", "unsupported frame kind"))?;
        reject_missing_remote_source_scope(runtime, operation, &frame.payload)?;
        let command =
            decode_json_adapter_command(operation, &frame.payload, &wss_command_options(runtime))?;
        let response = runtime.handle(
            EntryTransportContext {
                request_id: format!("wss-{}-{operation:?}", self.session_id),
                transport: TransportKind::Wss,
                mode: TransportMode::Bidirectional,
                operation,
                source_id: self.session_id.clone(),
                source_kind: "wss_peer".to_string(),
                idempotency_key: format!("wss-{}-{operation:?}", self.session_id),
                audit_id: format!("audit-wss-{}-{operation:?}", self.session_id),
                auth: EntryAuthDecision::authenticated("wss", "peer"),
            },
            command,
        )?;
        Ok(WssRuntimeEvent {
            kind: "event.report".to_string(),
            payload: render_response(response.adapter),
            private_raw_allowed: false,
        })
    }

    fn subscribe(&mut self, kind: String) -> WssRuntimeEvent {
        if self.subscriptions.len() >= self.budget.max_subscriptions {
            return error_event("subscription_budget_exceeded");
        }
        let allowed = message_specs()
            .iter()
            .any(|spec| spec.name == kind && spec.inbound_operation.is_none());
        if !allowed {
            return error_event("unsupported_subscription");
        }
        self.subscriptions.push(kind.clone());
        WssRuntimeEvent {
            kind: "event.lifecycle".to_string(),
            payload: json!({
                "status": "subscribed",
                "subscription": kind,
                "private_raw_allowed": false,
            })
            .to_string(),
            private_raw_allowed: false,
        }
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

#[cfg(feature = "server-std")]
pub fn serve_wss_stream(
    runtime: &EntryRuntime,
    stream: &mut (impl Read + Write),
    session_id: impl Into<String>,
    budget: WssBudget,
) -> bm_sdk::Result<()> {
    let key = read_websocket_key(stream)?;
    let accept = websocket_accept_key(&key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|err| bm_sdk::Error::config("wss_handshake_write", err.to_string()))?;
    stream
        .flush()
        .map_err(|err| bm_sdk::Error::config("wss_handshake_flush", err.to_string()))?;

    let text = read_client_text_frame(stream, budget.max_frame_bytes)?;
    let frame: WssRuntimeFrame = serde_json::from_str(&text)
        .map_err(|err| bm_sdk::Error::config("wss_frame_json", err.to_string()))?;
    let mut session = WssRuntimeSession::new(session_id, budget);
    let event = session.handle_frame(runtime, frame)?;
    write_server_text_frame(stream, &event.payload)
}

#[cfg(feature = "server-std")]
fn read_websocket_key(stream: &mut impl Read) -> bm_sdk::Result<String> {
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    while !buffer.ends_with(b"\r\n\r\n") {
        stream
            .read_exact(&mut byte)
            .map_err(|err| bm_sdk::Error::config("wss_handshake_read", err.to_string()))?;
        buffer.push(byte[0]);
        if buffer.len() > 8192 {
            return Err(bm_sdk::Error::config(
                "wss_handshake_read",
                "websocket handshake exceeds budget",
            ));
        }
    }
    let request = String::from_utf8(buffer)
        .map_err(|err| bm_sdk::Error::config("wss_handshake_utf8", err.to_string()))?;
    let mut has_upgrade = false;
    let mut websocket_key = None;
    for line in request.split("\r\n") {
        if line.to_ascii_lowercase().starts_with("upgrade:")
            && line.to_ascii_lowercase().contains("websocket")
        {
            has_upgrade = true;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("sec-websocket-key") {
                websocket_key = Some(value.trim().to_string());
            }
        }
    }
    if !has_upgrade {
        return Err(bm_sdk::Error::config(
            "wss_handshake",
            "missing websocket upgrade",
        ));
    }
    websocket_key.ok_or_else(|| bm_sdk::Error::config("wss_handshake", "missing websocket key"))
}

#[cfg(feature = "server-std")]
fn read_client_text_frame(
    stream: &mut impl Read,
    max_frame_bytes: usize,
) -> bm_sdk::Result<String> {
    let mut header = [0_u8; 2];
    stream
        .read_exact(&mut header)
        .map_err(|err| bm_sdk::Error::config("wss_frame_header", err.to_string()))?;
    let opcode = header[0] & 0x0f;
    if opcode == 0x08 {
        return Err(bm_sdk::Error::config(
            "wss_frame",
            "client closed websocket",
        ));
    }
    if opcode != 0x01 {
        return Err(bm_sdk::Error::config(
            "wss_frame",
            "only websocket text frames are supported",
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
    } else if len == 127 {
        return Err(bm_sdk::Error::config(
            "wss_frame",
            "64-bit websocket frame length is not supported",
        ));
    }
    if len > max_frame_bytes {
        return Err(bm_sdk::Error::config(
            "wss_frame",
            "websocket frame exceeds budget",
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
    String::from_utf8(payload)
        .map_err(|err| bm_sdk::Error::config("wss_frame_utf8", err.to_string()))
}

#[cfg(feature = "server-std")]
fn write_server_text_frame(stream: &mut impl Write, text: &str) -> bm_sdk::Result<()> {
    let payload = text.as_bytes();
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
fn websocket_accept_key(key: &str) -> String {
    let mut input = Vec::with_capacity(key.len() + 36);
    input.extend_from_slice(key.as_bytes());
    input.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64_encode(&sha1_digest(&input))
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
        AdapterResponse::Accepted { report, .. } => match report {
            AdapterSdkReport::Recall(report) => json!({
                "status": "accepted",
                "query": report.query,
                "procedural_hits": report.procedural_hits.len(),
            })
            .to_string(),
            AdapterSdkReport::Capabilities(catalog) => json!({
                "status": "accepted",
                "profile": catalog.profile.as_str(),
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

#[cfg(any(feature = "server-std", feature = "client-compact"))]
fn error_event(reason: &str) -> WssRuntimeEvent {
    WssRuntimeEvent {
        kind: "event.error".to_string(),
        payload: json!({
            "status": "rejected",
            "reason": reason,
        })
        .to_string(),
        private_raw_allowed: false,
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WssBudget {
    pub max_frame_bytes: usize,
    pub max_subscriptions: usize,
}

impl WssBudget {
    pub fn from_runtime_budget(report: &bm_sdk::RuntimeBudgetReport) -> Self {
        Self {
            max_frame_bytes: report.adapter_budget.wss_frame_max_bytes,
            max_subscriptions: report.adapter_budget.wss_max_subscriptions,
        }
    }
}

impl From<&bm_sdk::RuntimeBudgetReport> for WssBudget {
    fn from(value: &bm_sdk::RuntimeBudgetReport) -> Self {
        Self::from_runtime_budget(value)
    }
}
