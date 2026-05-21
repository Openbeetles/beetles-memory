//! A2A bridge contracts for Beetle Memory.

use bm_adapter::AdapterOperation;

#[cfg(feature = "bridge-http")]
use bm_adapter::{AdapterCommand, AdapterResponse, AdapterSdkReport, TransportKind, TransportMode};
#[cfg(feature = "bridge-http")]
use bm_entry::{EntryAuthDecision, EntryRuntime, EntryTransportContext};
#[cfg(feature = "bridge-http")]
use bm_sdk::MemoryRecallRequest;
#[cfg(feature = "bridge-http")]
use serde::Deserialize;
#[cfg(feature = "bridge-http")]
use serde_json::json;

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
        let command = decode_command(operation, &message.payload)?;
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
fn decode_command(operation: AdapterOperation, payload: &str) -> bm_sdk::Result<AdapterCommand> {
    match operation {
        AdapterOperation::Recall => {
            let payload: RecallPayload = serde_json::from_str(payload)
                .map_err(|err| bm_sdk::Error::config("a2a_bridge_json", err.to_string()))?;
            Ok(AdapterCommand::Recall(MemoryRecallRequest {
                query: payload.query,
                limit: payload.limit.unwrap_or(8),
            }))
        }
        other => Err(bm_sdk::Error::config(
            "a2a_bridge",
            format!("unsupported A2A bridge operation: {other:?}"),
        )),
    }
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

#[cfg(feature = "bridge-http")]
#[derive(Deserialize)]
struct RecallPayload {
    query: String,
    limit: Option<usize>,
}
