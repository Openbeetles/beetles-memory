use bm_adapter::{AdapterCommand, AdapterOperation, AdapterResponse};
use bm_entry::{
    EntryAuthConfig, EntryAuthDecision, EntryBearerPrincipal, EntryIdempotencyConfig,
    EntryIdentity, EntryOperationCapability, EntryRuntime, EntryRuntimeConfig, EntryScope,
    EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryRecallRequest, MemoryWriteRequest,
    RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
};

mod support;

fn bearer_config(
    owner_id: &str,
    capabilities: impl IntoIterator<Item = EntryOperationCapability>,
) -> EntryAuthConfig {
    EntryAuthConfig::required_bearer_principal(
        "secret-token",
        EntryBearerPrincipal::new("principal-remote", owner_id, capabilities),
    )
}

fn runtime_with_auth(auth: EntryAuthConfig) -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "agent-main".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "remote".to_string(),
            chat_id: "chat-remote".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::host_production_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth,
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

fn context(auth: EntryAuthDecision, operation: AdapterOperation) -> EntryTransportContext {
    EntryTransportContext::new(
        "auth-req",
        bm_adapter::TransportKind::Http,
        bm_adapter::TransportMode::Server,
        operation,
        "remote-client",
        "http_client",
        "auth-idem",
        "auth-audit",
        auth,
    )
}

fn recall_command() -> AdapterCommand {
    AdapterCommand::Recall(MemoryRecallRequest {
        temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
        structured_query_facets: Vec::new(),
        query: "release".to_string(),
        limit: 2,
        tool_registry_refs: Vec::new(),
    })
}

fn write_command() -> AdapterCommand {
    AdapterCommand::Write(MemoryWriteRequest::Procedural {
        writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
            name: "runtime_skill__auth_capability".to_string(),
            topic: "auth".to_string(),
            title: "Capability gate".to_string(),
            summary: "Capability denial must precede idempotency".to_string(),
            content: "Reject the operation before reserving the caller key.".to_string(),
            citations: Vec::new(),
            source_chat_id: Some("chat-remote".to_string()),
            observed_at: 1_800_000_000,
        })],
        owning_scope: support::runtime_skill_subject_scope("agent-main"),
        source: RuntimeSkillWriteSource::Manual,
    })
}

#[test]
fn bearer_verifier_returns_configured_typed_principal() {
    let config = bearer_config("owner-default", [EntryOperationCapability::Recall]);
    let decision = config.verify_bearer(Some("Bearer secret-token"));

    assert!(decision.is_authenticated());
    assert_eq!(decision.auth_kind(), "bearer_token");
    assert_eq!(decision.principal_id(), "principal-remote");
    let principal = decision.bearer_principal().expect("typed principal");
    assert_eq!(principal.owner_id(), "owner-default");
    assert!(principal.allows(EntryOperationCapability::Recall));
    assert!(!principal.allows(EntryOperationCapability::Write));
    assert_eq!(decision.permissions(), ["recall"]);
    assert_ne!(decision.token_fingerprint(), Some("secret-token"));
    assert!(decision
        .token_fingerprint()
        .is_some_and(|value| value.starts_with("tok_sha256_")));
}

#[test]
fn bearer_secret_is_redacted_from_recursive_debug_output() {
    let config = bearer_config("owner-default", [EntryOperationCapability::Recall]);
    let debug = format!("{config:?}");
    assert!(debug.contains("<redacted>"), "{debug}");
    assert!(!debug.contains("secret-token"), "{debug}");
}

#[test]
fn bearer_missing_mismatch_and_unconfigured_verifier_fail_closed() {
    let config = bearer_config("owner-default", [EntryOperationCapability::Recall]);
    let missing = config.verify_bearer(None);
    assert_eq!(missing.rejection_reason(), Some("missing_bearer_token"));

    let mismatch = config.verify_bearer(Some("Bearer wrong-token"));
    assert_eq!(mismatch.rejection_reason(), Some("token_mismatch"));

    let invalid = EntryAuthConfig::required_bearer_principal(
        "secret-token",
        EntryBearerPrincipal::new("principal-remote", "", [EntryOperationCapability::Recall]),
    );
    let rejected = invalid.verify_bearer(Some("Bearer secret-token"));
    assert_eq!(
        rejected.rejection_reason(),
        Some("invalid_bearer_verifier_config")
    );
}

#[test]
fn auth_required_runtime_rejects_trusted_bool_loopback_and_wrong_owner_binding() {
    let runtime = runtime_with_auth(bearer_config(
        "owner-default",
        [EntryOperationCapability::Recall],
    ));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let client = std::net::TcpStream::connect(listener.local_addr().expect("listener address"))
        .expect("loopback client");
    let accepted = bm_entry::EntryAcceptedTcpStream::accept(&listener).expect("accepted peer");
    drop(client);
    let local_decision = EntryAuthConfig::disabled_for_local()
        .authenticate_local_transport(bm_entry::EntryLocalTransport::InProcess, "http-client");
    let loopback_decision = EntryAuthConfig::disabled_for_local().authenticate_accepted_tcp_stream(
        &accepted,
        None,
        "http-client",
    );
    for decision in [local_decision, loopback_decision] {
        let response = runtime
            .handle(
                context(decision, AdapterOperation::Recall),
                recall_command(),
            )
            .expect("entry response");
        assert!(matches!(
            response.adapter,
            AdapterResponse::Rejected {
                error_key: bm_adapter::AdapterErrorKey::Unauthorized,
                ..
            }
        ));
    }

    let wrong_owner_config = bearer_config("other-owner", [EntryOperationCapability::Recall]);
    let runtime = runtime_with_auth(wrong_owner_config);
    let decision = runtime.authenticate_remote_bearer(Some("Bearer secret-token"));
    assert!(!decision.is_authenticated());
    assert_eq!(
        decision.rejection_reason(),
        Some("bearer_owner_binding_mismatch")
    );
    let response = runtime
        .handle(
            context(decision, AdapterOperation::Recall),
            recall_command(),
        )
        .expect("entry response");
    match response.adapter {
        AdapterResponse::Rejected { reason, .. } => {
            assert!(reason.contains("bearer_owner_binding_mismatch"))
        }
        other => panic!("unexpected owner mismatch response: {other:?}"),
    }
}

#[test]
fn operation_capability_rejection_happens_before_idempotency_reservation() {
    let config = bearer_config("owner-default", [EntryOperationCapability::Recall]);
    let runtime = runtime_with_auth(config.clone());

    for _ in 0..2 {
        let decision = config.verify_bearer(Some("Bearer secret-token"));
        let response = runtime
            .handle(context(decision, AdapterOperation::Write), write_command())
            .expect("entry response");
        match response.adapter {
            AdapterResponse::Rejected {
                error_key, reason, ..
            } => {
                assert_eq!(error_key, bm_adapter::AdapterErrorKey::Forbidden);
                assert!(reason.contains("write"), "{reason}");
            }
            other => panic!("capability denial was not stable: {other:?}"),
        }
    }
}

#[test]
fn operation_capability_public_labels_are_stable_snake_case() {
    let labels = EntryOperationCapability::all()
        .iter()
        .map(|capability| capability.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        labels,
        vec![
            "write",
            "finalize_turn",
            "recall",
            "project",
            "maintain",
            "inspect",
            "recover",
            "replay",
            "long_term_list",
            "long_term_detail",
            "long_term_mutate",
            "long_term_policy",
            "transcript_attr_write",
            "capabilities",
            "subscribe",
            "close",
            "console_read",
            "console_write",
            "mcp_protocol",
            "llm_gateway_protocol",
        ]
    );
}
