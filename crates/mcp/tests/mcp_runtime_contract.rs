#![cfg(feature = "server-stdio")]

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_mcp::{handle_mcp_streamable_http_request, McpResourceRead, McpToolCall, McpToolServer};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};
use serde_json::Value;

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "mcp-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "mcp".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[test]
fn mcp_tool_call_dispatches_through_entry_runtime_without_private_raw() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-server-1");
    let result = server
        .call(
            &runtime,
            McpToolCall::json("memory_recall", r#"{"query":"release","limit":2}"#),
        )
        .expect("tool call");

    assert_eq!(result.status, "accepted");
    assert!(!result.private_raw_allowed);
    assert!(result.content.contains("\"query\""));
}

#[test]
fn mcp_tool_server_decodes_declared_memory_tools() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-server-ops");
    let calls = [
        ("memory_capabilities", r#"{}"#),
        ("memory_project", r#"{"query":"release","max_len":1024}"#),
        ("memory_inspect", r#"{"query":"release"}"#),
        ("memory_replay", r#"{"chat_id":"chat-1","limit":2}"#),
        ("memory_long_term_list", r#"{"query":{},"limit":2}"#),
        (
            "memory_write_candidate",
            r#"{"name":"runtime_skill__mcp_write","topic":"mcp","title":"MCP write","summary":"MCP write summary","content":"1. Decode MCP tool payload.\n2. Dispatch through EntryRuntime."}"#,
        ),
        ("memory_export", r#"{"chat_id":"chat-1"}"#),
        (
            "memory_import",
            r#"{"target_chat_id":"chat-1","snapshot":{"version":5,"exported_at":1800000000,"mode":"full_restore","chat_id":"chat-1"}}"#,
        ),
    ];

    for (name, args) in calls {
        let result = server
            .call(&runtime, McpToolCall::json(name, args))
            .unwrap_or_else(|err| panic!("{name} failed: {err}"));
        assert_eq!(result.status, "accepted", "{name}: {}", result.content);
        assert!(!result.private_raw_allowed);
    }
}

#[test]
fn mcp_resource_read_uses_entry_runtime_safe_reports_without_private_raw() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-resource");

    for uri in [
        "memory://profile",
        "memory://scope",
        "memory://projection-preview",
    ] {
        let resource = server
            .read_resource(&runtime, McpResourceRead::new(uri))
            .unwrap_or_else(|err| panic!("{uri} failed: {err}"));
        assert_eq!(resource.uri, uri);
        assert_eq!(resource.mime_type, "application/json");
        assert!(!resource.private_raw_allowed);
        assert!(!resource.content.contains("\"private_raw\":true"), "{uri}");
        assert!(!resource.content.contains("raw_content"), "{uri}");
        assert!(!resource.content.contains("store_schema"), "{uri}");
    }
}

#[test]
fn streamable_http_handles_single_json_rpc_resource_request() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http");
    let response = handle_mcp_streamable_http_request(
        &server,
        &runtime,
        r#"{"jsonrpc":"2.0","id":"r1","method":"resources/list"}"#,
    )
    .expect("streamable http response");

    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "application/json");
    assert!(
        response.body.contains(r#""jsonrpc":"2.0""#),
        "{}",
        response.body
    );
    assert!(response.body.contains(r#""id":"r1""#), "{}", response.body);
    assert!(
        response.body.contains("memory://profile"),
        "{}",
        response.body
    );
    assert!(
        !response.body.contains("private_raw\":true"),
        "{}",
        response.body
    );
}

#[test]
fn json_rpc_initialize_negotiates_mcp_capabilities() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http");
    let response = handle_mcp_streamable_http_request(
        &server,
        &runtime,
        r#"{"jsonrpc":"2.0","id":"init-1","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"contract","version":"1.0.0"}}}"#,
    )
    .expect("initialize response");
    let body: Value = serde_json::from_str(&response.body).expect("json response");
    let result = body.get("result").expect("initialize result");

    assert_eq!(
        result.get("protocolVersion").and_then(Value::as_str),
        Some("2025-11-25")
    );
    assert!(result.pointer("/capabilities/tools").is_some(), "{body}");
    assert!(
        result.pointer("/capabilities/resources").is_some(),
        "{body}"
    );
    assert_eq!(
        result.pointer("/serverInfo/name").and_then(Value::as_str),
        Some("bm-mcp-server")
    );
}

#[test]
fn json_rpc_tools_list_uses_mcp_input_schema_shape() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http");
    let response = handle_mcp_streamable_http_request(
        &server,
        &runtime,
        r#"{"jsonrpc":"2.0","id":"tools-1","method":"tools/list"}"#,
    )
    .expect("tools list response");
    let body: Value = serde_json::from_str(&response.body).expect("json response");
    let tools = body
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .expect("tools array");
    let recall = tools
        .iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some("memory_recall"))
        .expect("memory_recall tool");

    assert!(recall.get("inputSchema").is_some(), "{recall}");
    assert!(
        recall.pointer("/inputSchema/properties/query").is_some(),
        "{recall}"
    );
    assert!(
        recall.pointer("/inputSchema/properties/limit").is_some(),
        "{recall}"
    );
    assert!(recall.get("schema_fields").is_none(), "{recall}");
}

#[test]
fn json_rpc_tools_call_uses_mcp_content_and_structured_content_shape() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http");
    let response = handle_mcp_streamable_http_request(
        &server,
        &runtime,
        r#"{"jsonrpc":"2.0","id":"call-1","method":"tools/call","params":{"name":"memory_capabilities","arguments":{}}}"#,
    )
    .expect("tools call response");
    let body: Value = serde_json::from_str(&response.body).expect("json response");
    let result = body.get("result").expect("tool result");
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .expect("content array");

    assert_eq!(content.len(), 1);
    assert_eq!(content[0].get("type").and_then(Value::as_str), Some("text"));
    assert!(content[0].get("text").and_then(Value::as_str).is_some());
    assert_eq!(
        result
            .pointer("/structuredContent/status")
            .and_then(Value::as_str),
        Some("accepted")
    );
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(false));
    assert!(result.get("status").is_none(), "{result}");
}

#[test]
fn json_rpc_resource_read_returns_text_resource_contents() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http");
    let response = handle_mcp_streamable_http_request(
        &server,
        &runtime,
        r#"{"jsonrpc":"2.0","id":"res-1","method":"resources/read","params":{"uri":"memory://scope"}}"#,
    )
    .expect("resource read response");
    let body: Value = serde_json::from_str(&response.body).expect("json response");
    let content = body
        .pointer("/result/contents/0")
        .expect("first resource content");

    assert_eq!(
        content.get("uri").and_then(Value::as_str),
        Some("memory://scope")
    );
    assert_eq!(
        content.get("mimeType").and_then(Value::as_str),
        Some("application/json")
    );
    assert!(
        content.get("text").and_then(Value::as_str).is_some(),
        "{content}"
    );
    assert_eq!(
        content
            .pointer("/_meta/private_raw_allowed")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(
        body.pointer("/result/private_raw_allowed").is_none(),
        "{body}"
    );
}
