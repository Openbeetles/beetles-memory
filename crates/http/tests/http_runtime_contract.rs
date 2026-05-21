#![cfg(feature = "server-axum")]

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{handle_http_request, HttpRuntimeRequest};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "http-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "http".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_disabled().with_cli(true),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[test]
fn http_runtime_dispatches_capabilities_and_recall_through_entry_runtime() {
    let runtime = runtime();

    let caps = handle_http_request(
        &runtime,
        HttpRuntimeRequest::get("/memory/profile/capabilities"),
    )
    .expect("capabilities");
    assert_eq!(caps.status_code, 200);
    assert!(caps.body.contains("\"profile\""));
    assert!(caps.body.contains("\"entry\""));

    let recall = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/recall", r#"{"query":"release","limit":2}"#),
    )
    .expect("recall");
    assert_eq!(recall.status_code, 200);
    assert!(recall.body.contains("\"status\""));
}

#[test]
fn webhook_write_candidate_uses_same_entry_runtime_dispatch() {
    let runtime = runtime();
    let response = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/webhook/write-candidate",
            r#"{
              "name":"runtime_skill__http_webhook_entry",
              "topic":"http-webhook",
              "title":"HTTP webhook entry",
              "summary":"Webhook writes procedural memory through the common entry runtime.",
              "content":"1. Verify webhook auth and payload budget.\n2. Normalize source metadata.\n3. Dispatch the write candidate through EntryRuntime.\n4. Return only the adapter report."
            }"#,
        ),
    )
    .expect("webhook write");

    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("\"operation\""));
    assert!(response.body.contains("write.procedural"));
}
