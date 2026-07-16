#![cfg(feature = "server-std")]

mod support;

use bm_entry::{
    EntryAuthConfig, EntryBearerPrincipal, EntryIdempotencyConfig, EntryIdentity,
    EntryOperationCapability, EntryRuntime, EntryRuntimeConfig, EntryScope, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig};

fn remote_runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let profile = support::native_runtime_profile();
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "wss-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "wss.remote".to_string(),
            chat_id: "chat-remote".to_string(),
        },
        store: StoreBackendConfig::in_memory(profile)
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::required_bearer_principal(
            "secret-token",
            EntryBearerPrincipal::new(
                "remote-peer",
                "owner-default",
                EntryOperationCapability::all().iter().copied(),
            ),
        ),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[test]
fn wss_remote_write_without_explicit_source_scope_is_rejected() {
    let runtime = remote_runtime();
    let (result, response) = support::serve_network_frame(
        runtime,
        Some("Bearer secret-token"),
        r#"{"kind":"command.write","payload":"{\"name\":\"runtime_skill__wss_remote\",\"topic\":\"scope\",\"title\":\"Remote\",\"summary\":\"Remote\",\"content\":\"must declare scope\"}"}"#,
    );
    let error = result.expect_err("remote WSS write must not silently fall back to chat-1");

    assert_eq!(error.stage(), "adapter_json_command");
    assert!(error.to_string().contains("source_chat_id"), "{error}");
    assert!(response.starts_with("HTTP/1.1 101 Switching Protocols"));
}
