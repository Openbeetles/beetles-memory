#![cfg(feature = "bridge-http")]

use bm_a2a::{A2aBridge, A2aPeerCapability, A2aRuntimeMessage};
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxMemoryGateway,
        identity: EntryIdentity {
            agent_id: "a2a-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "a2a".to_string(),
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
fn a2a_bridge_peer_capability_only_narrows_local_visibility() {
    let bridge = A2aBridge::new("bridge-1");
    assert!(bridge.merge_peer_visibility(A2aPeerCapability {
        memory_report_visible: true,
    }));
    assert!(!bridge.merge_peer_visibility(A2aPeerCapability {
        memory_report_visible: false,
    }));
}

#[test]
fn a2a_bridge_dispatches_memory_request_without_executor_permissions() {
    let runtime = runtime();
    let bridge = A2aBridge::new("bridge-1");
    let response = bridge
        .handle(
            &runtime,
            A2aRuntimeMessage::json("memory_recall_request", r#"{"query":"release","limit":2}"#),
        )
        .expect("a2a request");

    assert_eq!(response.kind, "memory_report");
    assert!(!response.permissions.iter().any(|permission| {
        matches!(
            permission,
            bm_a2a::A2aPermission::Executor
                | bm_a2a::A2aPermission::Tool
                | bm_a2a::A2aPermission::Workflow
        )
    }));
    assert!(response.payload.contains("\"status\""));
}
