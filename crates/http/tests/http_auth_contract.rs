#![cfg(feature = "server-std")]

mod support;

use bm_entry::{
    EntryAuthConfig, EntryBearerPrincipal, EntryIdempotencyConfig, EntryIdentity,
    EntryOperationCapability, EntryRuntime, EntryRuntimeConfig, EntryScope, EntryTransportConfig,
};
use bm_http::{handle_http_in_process_request, validate_http_bind_security, HttpRuntimeRequest};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig};

fn remote_runtime() -> (EntryRuntime, EntryAuthConfig) {
    let auth = EntryAuthConfig::required_bearer_principal(
        "secret-token",
        EntryBearerPrincipal::new(
            "http-principal",
            "owner-default",
            [
                EntryOperationCapability::Recall,
                EntryOperationCapability::Write,
            ],
        ),
    );
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let runtime = EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "http-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "http.remote".to_string(),
            chat_id: "chat-remote".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
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
    let request = HttpRuntimeRequest::post_json("/memory/recall", r#"{"query":"release"}"#);

    let response = handle_http_in_process_request(&runtime, request).expect("http response");

    assert_eq!(response.status_code, 401);
    assert!(response.body.contains("unauthorized"), "{}", response.body);
    assert!(
        response.body.contains("missing_bearer_token"),
        "{}",
        response.body
    );
}

#[test]
fn http_authenticated_principal_without_capability_is_forbidden() {
    let (runtime, _auth) = remote_runtime();
    let request = HttpRuntimeRequest::post_json(
        "/memory/project",
        r#"{"query":"release","system_max_len":1024,"recent_messages_limit":2}"#,
    )
    .with_bearer_token("secret-token");

    let response = handle_http_in_process_request(&runtime, request).expect("http response");

    assert_eq!(response.status_code, 403);
    assert!(response.body.contains("forbidden"), "{}", response.body);
    assert!(response.body.contains("project"), "{}", response.body);
}

#[test]
fn http_remote_write_without_explicit_source_scope_does_not_fall_back_to_chat_1() {
    let (runtime, _auth) = remote_runtime();
    let request = HttpRuntimeRequest::post_json(
        "/memory/write",
        r#"{"name":"runtime_skill__remote_write","topic":"scope","title":"Remote write","summary":"Remote write","content":"must declare scope"}"#,
    )
    .with_bearer_token("secret-token");

    let error = handle_http_in_process_request(&runtime, request)
        .expect_err("remote write without source_chat_id must be rejected before dispatch");

    assert_eq!(error.stage(), "adapter_json_command");
    assert!(error.to_string().contains("source_chat_id"), "{error}");
}

#[test]
fn http_local_profile_uses_explicit_local_default_scope_policy() {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let runtime = EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "http-agent".to_string(),
            owner_id: "owner-local".to_string(),
        },
        scope: EntryScope {
            channel: "http.local".to_string(),
            chat_id: "chat-local".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
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
    let response = handle_http_in_process_request(&runtime, request).expect("http response");

    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("\"status\""), "{}", response.body);
}

#[test]
fn non_loopback_http_bind_without_bearer_verifier_fails_before_accept() {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let runtime = EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "http-agent".to_string(),
            owner_id: "owner-local".to_string(),
        },
        scope: EntryScope {
            channel: "http.local".to_string(),
            chat_id: "chat-local".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime");
    let error = validate_http_bind_security(
        &runtime,
        std::net::SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, 8718)),
    )
    .expect_err("wildcard bind without verifier must fail closed");

    assert_eq!(error.stage(), "http_listener_auth");
}
