#![cfg(feature = "server-stdio")]

mod support;

use bm_entry::{
    EntryAuthConfig, EntryBearerPrincipal, EntryIdempotencyConfig, EntryIdentity,
    EntryOperationCapability, EntryRuntime, EntryRuntimeConfig, EntryScope, EntryTransportConfig,
};
use bm_mcp::{McpToolCall, McpToolServer};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig};

fn remote_runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "mcp-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            conversation_id: None,
            channel: "mcp.remote".to_string(),
            chat_id: "chat-remote".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::required_bearer_principal(
            "secret-token",
            EntryBearerPrincipal::new(
                "mcp-remote-principal",
                "owner-default",
                [EntryOperationCapability::Write],
            ),
        ),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[test]
fn mcp_write_rejects_caller_supplied_scope_fields() {
    let runtime = remote_runtime();
    let server = McpToolServer::new("mcp-remote", "mcp-remote-client");

    let mut payload = support::factual_write_body();
    payload["source_chat_id"] = "forged-chat".into();
    let error = server
        .call(
            &runtime,
            McpToolCall::json("memory_write_candidate", payload.to_string()),
        )
        .expect_err("caller scope field must be rejected by canonical decoder");

    assert_eq!(error.stage(), "adapter_json_command");
    assert!(error.to_string().contains("source_chat_id"), "{error}");
}
