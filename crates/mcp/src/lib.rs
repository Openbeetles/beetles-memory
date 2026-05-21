//! MCP adapter contracts for Beetle Memory.

use bm_adapter::AdapterOperation;

#[cfg(feature = "server-stdio")]
use bm_adapter::{AdapterCommand, AdapterResponse, AdapterSdkReport, TransportKind, TransportMode};
#[cfg(feature = "server-stdio")]
use bm_entry::{EntryAuthDecision, EntryRuntime, EntryTransportContext};
#[cfg(feature = "server-stdio")]
use bm_sdk::MemoryRecallRequest;
#[cfg(feature = "server-stdio")]
use serde::Deserialize;
#[cfg(feature = "server-stdio")]
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpToolSpec {
    pub name: &'static str,
    pub operation: AdapterOperation,
    pub schema_fields: Vec<String>,
    pub private_raw_allowed: bool,
}

pub fn tool_specs() -> Vec<McpToolSpec> {
    vec![
        tool("memory_capabilities", AdapterOperation::Capabilities, &[]),
        tool(
            "memory_recall",
            AdapterOperation::Recall,
            &["query", "limit"],
        ),
        tool(
            "memory_project",
            AdapterOperation::Project,
            &["query", "max_len"],
        ),
        tool("memory_inspect", AdapterOperation::Inspect, &["query"]),
        tool(
            "memory_replay",
            AdapterOperation::Replay,
            &["chat_id", "limit"],
        ),
        tool(
            "memory_write_candidate",
            AdapterOperation::Write,
            &["candidate"],
        ),
        tool("memory_export", AdapterOperation::Export, &["chat_id"]),
        tool(
            "memory_import",
            AdapterOperation::Import,
            &["snapshot", "target_chat_id"],
        ),
    ]
}

fn tool(name: &'static str, operation: AdapterOperation, fields: &[&str]) -> McpToolSpec {
    McpToolSpec {
        name,
        operation,
        schema_fields: fields.iter().map(|field| (*field).to_string()).collect(),
        private_raw_allowed: false,
    }
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpToolCall {
    pub name: String,
    pub arguments: String,
}

#[cfg(feature = "server-stdio")]
impl McpToolCall {
    pub fn json(name: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            arguments: arguments.into(),
        }
    }
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpToolResult {
    pub status: String,
    pub content: String,
    pub private_raw_allowed: bool,
}

#[cfg(feature = "server-stdio")]
pub struct McpToolServer {
    server_id: String,
}

#[cfg(feature = "server-stdio")]
impl McpToolServer {
    pub fn new(server_id: impl Into<String>) -> Self {
        Self {
            server_id: server_id.into(),
        }
    }

    pub fn call(&self, runtime: &EntryRuntime, call: McpToolCall) -> bm_sdk::Result<McpToolResult> {
        let spec = tool_specs()
            .into_iter()
            .find(|spec| spec.name == call.name)
            .ok_or_else(|| bm_sdk::Error::config("mcp_runtime", "unsupported tool"))?;
        let command = decode_command(spec.operation, &call.arguments)?;
        let response = runtime.handle(
            EntryTransportContext {
                request_id: format!("mcp-{}-{}", self.server_id, spec.name),
                transport: TransportKind::Mcp,
                mode: TransportMode::Server,
                operation: spec.operation,
                source_id: self.server_id.clone(),
                source_kind: "mcp_tool".to_string(),
                idempotency_key: format!("mcp-{}-{}", self.server_id, spec.name),
                audit_id: format!("audit-mcp-{}-{}", self.server_id, spec.name),
                auth: EntryAuthDecision::authenticated("mcp", "tool-client"),
            },
            command,
        )?;
        Ok(render_tool_result(response.adapter))
    }
}

#[cfg(feature = "server-stdio")]
fn decode_command(operation: AdapterOperation, args: &str) -> bm_sdk::Result<AdapterCommand> {
    match operation {
        AdapterOperation::Capabilities => Ok(AdapterCommand::Capabilities),
        AdapterOperation::Recall => {
            let args: RecallArgs = serde_json::from_str(args)
                .map_err(|err| bm_sdk::Error::config("mcp_runtime_json", err.to_string()))?;
            Ok(AdapterCommand::Recall(MemoryRecallRequest {
                query: args.query,
                limit: args.limit.unwrap_or(8),
            }))
        }
        other => Err(bm_sdk::Error::config(
            "mcp_runtime",
            format!("unsupported MCP runtime operation: {other:?}"),
        )),
    }
}

#[cfg(feature = "server-stdio")]
fn render_tool_result(response: AdapterResponse<AdapterSdkReport>) -> McpToolResult {
    match response {
        AdapterResponse::Accepted { report, .. } => {
            let content = match report {
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
            };
            McpToolResult {
                status: "accepted".to_string(),
                content,
                private_raw_allowed: false,
            }
        }
        AdapterResponse::Rejected { reason, .. } => McpToolResult {
            status: "rejected".to_string(),
            content: json!({"status":"rejected","reason":reason}).to_string(),
            private_raw_allowed: false,
        },
        AdapterResponse::Queued { queue, .. } => McpToolResult {
            status: "queued".to_string(),
            content: json!({"status":"queued","queue":queue}).to_string(),
            private_raw_allowed: false,
        },
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => McpToolResult {
            status: "duplicated".to_string(),
            content: json!({"status":"duplicated","idempotency_key":idempotency_key}).to_string(),
            private_raw_allowed: false,
        },
    }
}

#[cfg(feature = "server-stdio")]
#[derive(Deserialize)]
struct RecallArgs {
    query: String,
    limit: Option<usize>,
}
