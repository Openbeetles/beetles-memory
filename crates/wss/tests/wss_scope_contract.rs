#![cfg(feature = "server-std")]

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};
use bm_wss::{WssBudget, WssRuntimeFrame, WssRuntimeSession};

fn remote_runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "wss-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "wss.remote".to_string(),
            chat_id: "chat-remote".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::required_bearer_token("secret-token"),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[test]
fn wss_remote_write_without_explicit_source_scope_is_rejected() {
    let runtime = remote_runtime();
    let mut session = WssRuntimeSession::new(
        "remote-session",
        WssBudget::from_runtime_budget(runtime.runtime_budget()),
    );

    let error = session
        .handle_frame(
            &runtime,
            WssRuntimeFrame::command(
                "command.write",
                r#"{"name":"runtime_skill__wss_remote","topic":"scope","title":"Remote","summary":"Remote","content":"must declare scope"}"#,
            ),
        )
        .expect_err("remote WSS write must not silently fall back to chat-1");

    assert_eq!(error.stage(), "adapter_json_command");
    assert!(error.to_string().contains("source_chat_id"), "{error}");
}
