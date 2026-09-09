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
            conversation_id: None,
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
fn wss_remote_write_binds_entry_scope_and_rejects_payload_identity_override() {
    let runtime = remote_runtime();
    let payload = support::factual_write_body();
    let frame = |payload: &serde_json::Value| {
        serde_json::json!({"kind": "command.write", "payload": payload.to_string(), "idempotency_key": "remote-factual-write"}).to_string()
    };
    let (result, response) =
        support::serve_network_frame(&runtime, Some("Bearer secret-token"), &frame(&payload));
    result.expect("authenticated remote factual write");
    assert!(response.starts_with("HTTP/1.1 101 Switching Protocols"));
    let records = runtime
        .runtime()
        .list_long_term_memory(bm_sdk::MemoryLongTermListRequest {
            query: Default::default(),
            cursor: None,
            limit: 8,
            view: bm_sdk::MemoryLongTermControlView::RawOwner,
        })
        .unwrap()
        .records;
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].record.source_chat_id.as_deref(),
        Some("chat-remote")
    );
    assert_eq!(
        records[0].record.content,
        "The remote project uses the authenticated Entry scope."
    );
    #[cfg(feature = "nonproduction-replay-harness")]
    let before = runtime
        .runtime()
        .replay_harness()
        .export_store_snapshot()
        .unwrap();
    for field in ["source_chat_id", "owner_id", "subject_id"] {
        let mut forged = payload.clone();
        forged[field] = "forged-scope".into();
        let (result, _) =
            support::serve_network_frame(&runtime, Some("Bearer secret-token"), &frame(&forged));
        assert_eq!(
            result.expect_err("scope override rejected").stage(),
            "adapter_json_command"
        );
    }
    #[cfg(feature = "nonproduction-replay-harness")]
    assert!(
        runtime
            .runtime()
            .replay_harness()
            .export_store_snapshot()
            .unwrap()
            == before,
        "forged scope must be zero-write"
    );
}
