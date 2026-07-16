#![cfg(feature = "server-stdio")]

mod support;

use std::io::Cursor;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
use bm_mcp::{serve_mcp_stdio_once, McpToolServer};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig};
use serde_json::Value;

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "mcp-stdio-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "mcp-stdio".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[test]
fn stdio_json_rpc_tool_call_dispatches_through_entry_runtime() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-stdio", "mcp-stdio-client");
    let input = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_capabilities","arguments":{}}}
"#;
    let mut reader = Cursor::new(input.as_slice());
    let mut writer = Vec::new();

    serve_mcp_stdio_once(&server, &runtime, &mut reader, &mut writer).expect("serve stdio once");

    let output = String::from_utf8(writer).expect("utf8 output");
    assert!(output.contains(r#""jsonrpc":"2.0""#), "{output}");
    assert!(output.contains(r#""id":1"#), "{output}");
    let body: Value = serde_json::from_str(output.trim()).expect("json rpc response");
    assert!(body
        .pointer("/_meta/runtimeBudgetReportId")
        .and_then(Value::as_str)
        .is_some_and(|value| value.starts_with("rtb-v2-")));
    assert_eq!(
        body.pointer("/result/structuredContent/status")
            .and_then(Value::as_str),
        Some("accepted")
    );
    assert_eq!(
        body.pointer("/result/content/0/type")
            .and_then(Value::as_str),
        Some("text")
    );
    assert!(output.contains(r#""profile""#), "{output}");
}

#[test]
fn stdio_json_rpc_resources_read_returns_safe_content() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-stdio-resource", "mcp-stdio-client");
    let input =
        br#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"memory://scope"}}
"#;
    let mut reader = Cursor::new(input.as_slice());
    let mut writer = Vec::new();

    serve_mcp_stdio_once(&server, &runtime, &mut reader, &mut writer).expect("serve stdio once");

    let output = String::from_utf8(writer).expect("utf8 output");
    assert!(output.contains(r#""jsonrpc":"2.0""#), "{output}");
    assert!(output.contains(r#""id":2"#), "{output}");
    assert!(output.contains(r#""uri":"memory://scope""#), "{output}");
    let body: Value = serde_json::from_str(output.trim()).expect("json rpc response");
    assert_eq!(
        body.pointer("/result/contents/0/_meta/private_raw_allowed")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(!output.contains("store_schema"), "{output}");
}

#[test]
fn stdio_json_rpc_initialize_uses_mcp_lifecycle_shape() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-stdio-init", "mcp-stdio-client");
    let input = br#"{"jsonrpc":"2.0","id":"init-stdio","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"stdio-contract","version":"1.0.0"}}}
"#;
    let mut reader = Cursor::new(input.as_slice());
    let mut writer = Vec::new();

    serve_mcp_stdio_once(&server, &runtime, &mut reader, &mut writer).expect("serve stdio once");

    let output = String::from_utf8(writer).expect("utf8 output");
    let body: Value = serde_json::from_str(output.trim()).expect("json rpc response");
    assert_eq!(
        body.pointer("/result/protocolVersion")
            .and_then(Value::as_str),
        Some("2025-11-25")
    );
    assert!(
        body.pointer("/result/capabilities/tools").is_some(),
        "{body}"
    );
    assert!(
        body.pointer("/result/capabilities/resources").is_some(),
        "{body}"
    );
}

#[test]
fn project_and_inspect_schema_share_the_required_typed_decoder_contract() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-stdio-schema", "mcp-stdio-client");
    let input = br#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}
"#;
    let mut reader = Cursor::new(input.as_slice());
    let mut writer = Vec::new();

    serve_mcp_stdio_once(&server, &runtime, &mut reader, &mut writer).expect("tools/list");
    let body: Value = serde_json::from_slice(&writer).expect("tools/list response");
    for name in ["memory_project", "memory_inspect"] {
        let tool = body
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .and_then(|tools| tools.iter().find(|tool| tool["name"] == name))
            .unwrap_or_else(|| panic!("missing {name}"));
        assert_eq!(
            tool.pointer("/inputSchema/required"),
            Some(&serde_json::json!(["query", "system_max_len"]))
        );
        assert_eq!(
            tool.pointer("/inputSchema/additionalProperties")
                .and_then(Value::as_bool),
            Some(false)
        );
    }

    let missing_bound = McpToolServer::new("mcp-typed-decode", "mcp-stdio-client")
        .call(
            &runtime,
            bm_mcp::McpToolCall::json("memory_inspect", r#"{"query":"release"}"#),
        )
        .expect_err("typed decoder must require system_max_len");
    assert_eq!(missing_bound.stage(), "mcp_tool_arguments");
}

#[test]
fn stdio_json_rpc_initialized_notification_writes_no_stdout() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-stdio-notification", "mcp-stdio-client");
    let input = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}
"#;
    let mut reader = Cursor::new(input.as_slice());
    let mut writer = Vec::new();

    serve_mcp_stdio_once(&server, &runtime, &mut reader, &mut writer).expect("serve stdio once");

    assert!(writer.is_empty(), "notification wrote stdout");
}

#[test]
fn stdio_rejects_stream_without_newline_before_copying_past_budget() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-stdio-bounded", "mcp-stdio-client");
    let max_bytes = runtime.runtime_budget().adapter_budget.http_body_max_bytes;
    let mut reader = Cursor::new(vec![b'x'; max_bytes + 1]);
    let mut writer = Vec::new();

    let error = serve_mcp_stdio_once(&server, &runtime, &mut reader, &mut writer)
        .expect_err("oversized unterminated request must fail closed");

    assert_eq!(error.stage(), "mcp_stdio_read");
    assert!(error.to_string().contains("exceeds pinned"));
    assert!(writer.is_empty());
}

#[test]
fn stdio_accepts_request_at_exact_body_budget() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-stdio-exact-boundary", "mcp-stdio-client");
    let max_bytes = runtime.runtime_budget().adapter_budget.http_body_max_bytes;
    let base = br#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#;
    assert!(base.len() <= max_bytes);
    let mut input = base.to_vec();
    input.resize(max_bytes, b' ');
    input.push(b'\n');
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();

    serve_mcp_stdio_once(&server, &runtime, &mut reader, &mut writer)
        .expect("exact boundary request");

    assert!(!writer.is_empty());
}
