#![cfg(feature = "server-stdio")]

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_mcp::{McpToolCall, McpToolServer};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

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
