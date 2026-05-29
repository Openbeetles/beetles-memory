use bm_adapter::{AdapterCommand, AdapterOperation, AdapterResponse};
use bm_entry::{
    EntryAuthConfig, EntryAuthDecision, EntryIdempotencyConfig, EntryIdentity, EntryRuntime,
    EntryRuntimeConfig, EntryScope, EntryStoreConfig, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryRecallRequest, ProfileId, StoreBackendKind,
};

fn runtime_with_auth(auth: EntryAuthConfig) -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "agent-main".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "remote".to_string(),
            chat_id: "chat-remote".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth,
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

fn context(auth: EntryAuthDecision) -> EntryTransportContext {
    EntryTransportContext {
        request_id: "auth-req".to_string(),
        transport: bm_adapter::TransportKind::Http,
        mode: bm_adapter::TransportMode::Server,
        operation: AdapterOperation::Recall,
        source_id: "remote-client".to_string(),
        source_kind: "http_client".to_string(),
        idempotency_key: "auth-idem".to_string(),
        audit_id: "auth-audit".to_string(),
        auth,
    }
}

#[test]
fn remote_bearer_token_carries_subject_and_does_not_expose_raw_token() {
    let config = EntryAuthConfig::required_bearer_token("secret-token");
    let decision = EntryAuthDecision::remote_bearer(
        &config,
        Some("Bearer secret-token"),
        Some("owner-remote"),
    );

    assert!(decision.authenticated);
    assert_eq!(decision.auth_kind, "bearer_token");
    assert_eq!(decision.auth_subject.as_deref(), Some("owner-remote"));
    assert_eq!(decision.principal, "owner-remote");
    assert_ne!(decision.token_fingerprint.as_deref(), Some("secret-token"));
    assert!(decision
        .token_fingerprint
        .as_deref()
        .unwrap_or("")
        .starts_with("tok_"));
}

#[test]
fn remote_bearer_token_missing_or_mismatch_is_structured_rejection() {
    let config = EntryAuthConfig::required_bearer_token("secret-token");

    let missing = EntryAuthDecision::remote_bearer(&config, None, Some("owner-remote"));
    assert!(!missing.authenticated);
    assert_eq!(missing.auth_kind, "bearer_token");
    assert_eq!(
        missing.rejection_reason.as_deref(),
        Some("missing_bearer_token")
    );

    let mismatch =
        EntryAuthDecision::remote_bearer(&config, Some("Bearer wrong-token"), Some("owner-remote"));
    assert!(!mismatch.authenticated);
    assert_eq!(mismatch.rejection_reason.as_deref(), Some("token_mismatch"));
}

#[test]
fn auth_required_runtime_rejects_arbitrary_authenticated_bool_and_loopback() {
    let auth_config = EntryAuthConfig::required_bearer_token("secret-token");
    let runtime = runtime_with_auth(auth_config);

    let arbitrary = runtime
        .handle(
            context(EntryAuthDecision::authenticated(
                "token_or_loopback",
                "http-client",
            )),
            AdapterCommand::Recall(MemoryRecallRequest {
                query: "release".to_string(),
                limit: 2,
                tool_registry_refs: Vec::new(),
            }),
        )
        .expect("entry response");
    match arbitrary.adapter {
        AdapterResponse::Rejected {
            error_key, reason, ..
        } => {
            assert_eq!(error_key, bm_adapter::AdapterErrorKey::Unauthorized);
            assert!(reason.contains("token_fingerprint"), "{reason}");
        }
        other => panic!("unexpected arbitrary auth response: {other:?}"),
    }

    let loopback = runtime
        .handle(
            context(EntryAuthDecision::loopback("http-client")),
            AdapterCommand::Recall(MemoryRecallRequest {
                query: "release".to_string(),
                limit: 2,
                tool_registry_refs: Vec::new(),
            }),
        )
        .expect("entry response");
    match loopback.adapter {
        AdapterResponse::Rejected {
            error_key, reason, ..
        } => {
            assert_eq!(error_key, bm_adapter::AdapterErrorKey::Unauthorized);
            assert!(reason.contains("loopback"), "{reason}");
        }
        other => panic!("unexpected loopback response: {other:?}"),
    }
}
