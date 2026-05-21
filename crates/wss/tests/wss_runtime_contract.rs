#![cfg(feature = "server-axum")]

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};
use bm_wss::{WssRuntimeFrame, WssRuntimeSession};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxMemoryGateway,
        identity: EntryIdentity {
            agent_id: "wss-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "wss".to_string(),
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
fn wss_session_dispatches_command_frame_through_entry_runtime() {
    let runtime = runtime();
    let mut session = WssRuntimeSession::new("session-1", bm_wss::WssBudget::server_gateway());
    let response = session
        .handle_frame(
            &runtime,
            WssRuntimeFrame::command("command.recall", r#"{"query":"release","limit":2}"#),
        )
        .expect("wss frame");

    assert_eq!(response.kind, "event.report");
    assert!(response.payload.contains("\"status\""));
    assert!(!response.private_raw_allowed);
}

#[test]
fn wss_subscription_respects_budget_and_never_allows_private_raw() {
    let runtime = runtime();
    let mut session = WssRuntimeSession::new(
        "session-1",
        bm_wss::WssBudget {
            max_frame_bytes: 1024,
            max_subscriptions: 1,
        },
    );

    let first = session
        .handle_frame(&runtime, WssRuntimeFrame::subscribe("subscribe.projection"))
        .expect("first subscription");
    assert_eq!(first.kind, "event.lifecycle");
    assert!(!first.private_raw_allowed);

    let second = session
        .handle_frame(&runtime, WssRuntimeFrame::subscribe("subscribe.inspection"))
        .expect("second subscription");
    assert_eq!(second.kind, "event.error");
    assert!(second.payload.contains("subscription_budget_exceeded"));
    assert!(!second.private_raw_allowed);
}
