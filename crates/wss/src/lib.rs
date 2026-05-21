//! WSS adapter contracts for Beetle Memory.

use bm_adapter::AdapterOperation;

#[cfg(any(feature = "server-axum", feature = "client-compact"))]
use bm_adapter::{AdapterCommand, AdapterResponse, AdapterSdkReport, TransportKind, TransportMode};
#[cfg(any(feature = "server-axum", feature = "client-compact"))]
use bm_entry::{EntryAuthDecision, EntryRuntime, EntryTransportContext};
#[cfg(any(feature = "server-axum", feature = "client-compact"))]
use bm_sdk::MemoryRecallRequest;
#[cfg(any(feature = "server-axum", feature = "client-compact"))]
use serde::Deserialize;
#[cfg(any(feature = "server-axum", feature = "client-compact"))]
use serde_json::json;

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

#[cfg(any(feature = "server-axum", feature = "client-compact"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WssRuntimeFrame {
    pub kind: String,
    pub payload: String,
}

#[cfg(any(feature = "server-axum", feature = "client-compact"))]
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

#[cfg(any(feature = "server-axum", feature = "client-compact"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WssRuntimeEvent {
    pub kind: String,
    pub payload: String,
    pub private_raw_allowed: bool,
}

#[cfg(any(feature = "server-axum", feature = "client-compact"))]
pub struct WssRuntimeSession {
    session_id: String,
    budget: WssBudget,
    subscriptions: Vec<String>,
}

#[cfg(any(feature = "server-axum", feature = "client-compact"))]
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
        let command = decode_command(operation, &frame.payload)?;
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

#[cfg(any(feature = "server-axum", feature = "client-compact"))]
fn decode_command(operation: AdapterOperation, payload: &str) -> bm_sdk::Result<AdapterCommand> {
    match operation {
        AdapterOperation::Capabilities => Ok(AdapterCommand::Capabilities),
        AdapterOperation::Recall => {
            let payload: RecallPayload = serde_json::from_str(payload)
                .map_err(|err| bm_sdk::Error::config("wss_runtime_json", err.to_string()))?;
            Ok(AdapterCommand::Recall(MemoryRecallRequest {
                query: payload.query,
                limit: payload.limit.unwrap_or(8),
            }))
        }
        other => Err(bm_sdk::Error::config(
            "wss_runtime",
            format!("unsupported WSS runtime operation: {other:?}"),
        )),
    }
}

#[cfg(any(feature = "server-axum", feature = "client-compact"))]
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

#[cfg(any(feature = "server-axum", feature = "client-compact"))]
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

#[cfg(any(feature = "server-axum", feature = "client-compact"))]
#[derive(Deserialize)]
struct RecallPayload {
    query: String,
    limit: Option<usize>,
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
    pub const fn esp_standalone() -> Self {
        Self {
            max_frame_bytes: 8 * 1024,
            max_subscriptions: 4,
        }
    }

    pub const fn server_gateway() -> Self {
        Self {
            max_frame_bytes: 64 * 1024,
            max_subscriptions: 64,
        }
    }
}
