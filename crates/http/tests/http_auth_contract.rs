#![cfg(feature = "server-std")]

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{handle_http_request, HttpRuntimeRequest};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn remote_runtime() -> (EntryRuntime, EntryAuthConfig) {
    let auth = EntryAuthConfig::required_bearer_token("secret-token");
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let runtime = EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "http-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "http.remote".to_string(),
            chat_id: "chat-remote".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth: auth.clone(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime");
    (runtime, auth)
}

#[test]
fn http_remote_missing_token_is_structured_unauthorized_response() {
    let (runtime, _auth) = remote_runtime();
    let mut request = HttpRuntimeRequest::post_json("/memory/recall", r#"{"query":"release"}"#);
    request.authenticated = false;

    let response = handle_http_request(&runtime, request).expect("http response");

    assert_eq!(response.status_code, 401);
    assert!(response.body.contains("Unauthorized"), "{}", response.body);
    assert!(
        response.body.contains("unauthenticated"),
        "{}",
        response.body
    );
}

#[test]
fn http_remote_write_without_explicit_source_scope_does_not_fall_back_to_chat_1() {
    let (runtime, _auth) = remote_runtime();
    let request = HttpRuntimeRequest::post_json(
        "/memory/write",
        r#"{"name":"runtime_skill__remote_write","topic":"scope","title":"Remote write","summary":"Remote write","content":"must declare scope"}"#,
    );

    let error = handle_http_request(&runtime, request)
        .expect_err("remote write without source_chat_id must be rejected before dispatch");

    assert_eq!(error.stage(), "adapter_json_command");
    assert!(error.to_string().contains("source_chat_id"), "{error}");
}

#[test]
fn http_local_profile_uses_explicit_local_default_scope_policy() {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let runtime = EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::DesktopMacosStandaloneMemory,
        identity: EntryIdentity {
            agent_id: "http-agent".to_string(),
            owner_id: "owner-local".to_string(),
        },
        scope: EntryScope {
            channel: "http.local".to_string(),
            chat_id: "chat-local".to_string(),
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
    .expect("entry runtime");

    let request = HttpRuntimeRequest::post_json(
        "/memory/write",
        r#"{"name":"runtime_skill__local_write","topic":"scope","title":"Local write","summary":"Local write","content":"local policy may fill scope"}"#,
    );
    let response = handle_http_request(&runtime, request).expect("http response");

    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("\"status\""), "{}", response.body);
}
