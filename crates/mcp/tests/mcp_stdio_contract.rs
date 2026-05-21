#![cfg(feature = "server-stdio")]

use std::io::Cursor;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_mcp::{serve_mcp_stdio_once, McpToolServer};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

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
    assert!(output.contains(r#""status":"accepted""#), "{output}");
    assert!(output.contains(r#""profile""#), "{output}");
}
