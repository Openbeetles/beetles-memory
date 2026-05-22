#![cfg(feature = "server-stdio")]

use std::io::Cursor;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_mcp::{serve_mcp_stdio_once, McpToolServer};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};
use serde_json::Value;

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxMemoryGateway,
        identity: EntryIdentity {
            agent_id: "mcp-stdio-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "mcp-stdio".to_string(),
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
fn stdio_json_rpc_tool_call_dispatches_through_entry_runtime() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-stdio");
    let input = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_capabilities","arguments":{}}}
"#;
    let mut reader = Cursor::new(input.as_slice());
    let mut writer = Vec::new();

    serve_mcp_stdio_once(&server, &runtime, &mut reader, &mut writer).expect("serve stdio once");

    let output = String::from_utf8(writer).expect("utf8 output");
    assert!(output.contains(r#""jsonrpc":"2.0""#), "{output}");
    assert!(output.contains(r#""id":1"#), "{output}");
    let body: Value = serde_json::from_str(output.trim()).expect("json rpc response");
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
    let server = McpToolServer::new("mcp-stdio-resource");
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
    let server = McpToolServer::new("mcp-stdio-init");
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
fn stdio_json_rpc_initialized_notification_writes_no_stdout() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-stdio-notification");
    let input = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}
"#;
    let mut reader = Cursor::new(input.as_slice());
    let mut writer = Vec::new();

    serve_mcp_stdio_once(&server, &runtime, &mut reader, &mut writer).expect("serve stdio once");

    assert!(writer.is_empty(), "notification wrote stdout");
}
