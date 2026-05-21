//! MCP adapter contracts for Beetle Memory.

use bm_adapter::AdapterOperation;

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
