use bm_adapter::{AdapterCommand, AdapterOperation, AdapterResponse};
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryWriteRequest, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig,
};

mod support;

fn config() -> EntryRuntimeConfig {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let profile = support::host_production_profile();
    EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "agent-main".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "local".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(profile)
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_disabled().with_cli(true),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    }
}

fn context(idempotency_key: &str) -> EntryTransportContext {
    context_for_transport(idempotency_key, bm_adapter::TransportKind::Cli)
}

fn context_for_transport(
    idempotency_key: &str,
    transport: bm_adapter::TransportKind,
) -> EntryTransportContext {
    EntryTransportContext::new(
        "req-write",
        transport,
        bm_adapter::TransportMode::InProcess,
        AdapterOperation::Write,
        "source-1",
        "local_cli",
        idempotency_key,
        "audit-write",
        support::trusted_local_auth("operator"),
    )
}

#[test]
fn explicit_idempotency_replays_and_conflicts_across_transport_boundaries() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let first = runtime
        .handle(
            context_for_transport("cross-transport-key", bm_adapter::TransportKind::Mcp),
            write_command(),
        )
        .expect("first transport write");
    let replay = runtime
        .handle(
            context_for_transport("cross-transport-key", bm_adapter::TransportKind::Wss),
            write_command(),
        )
        .expect("cross transport replay");
    let conflict = runtime
        .handle(
            context_for_transport("cross-transport-key", bm_adapter::TransportKind::Http),
            write_command_with_summary("cross transport conflict"),
        )
        .expect("cross transport conflict");

    assert!(matches!(first.adapter, AdapterResponse::Accepted { .. }));
    assert!(matches!(replay.adapter, AdapterResponse::Duplicated { .. }));
    assert!(matches!(
        conflict.adapter,
        AdapterResponse::Rejected {
            error_key: bm_adapter::AdapterErrorKey::Duplicated,
            ..
        }
    ));
}

fn write_command() -> AdapterCommand {
    write_command_with_summary("Entry runtime dispatches writes through SDK governance.")
}

fn write_command_with_summary(summary: &str) -> AdapterCommand {
    AdapterCommand::Write(MemoryWriteRequest::Procedural {
        writes: vec![RuntimeSkillWrite {
            name: "runtime_skill__entry_runtime".to_string(),
            topic: "entry-runtime".to_string(),
            title: "Entry runtime writes".to_string(),
            summary: summary.to_string(),
            content: "Use EntryRuntime to normalize source/auth/idempotency before SDK dispatch."
                .to_string(),
            citations: vec![],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        }],
        source: RuntimeSkillWriteSource::Manual,
    })
}

#[test]
fn idempotency_key_reuse_with_a_different_mutation_payload_is_rejected() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let first = runtime
        .handle(context("idem-conflict-1"), write_command())
        .expect("first write");
    let conflicting = runtime
        .handle(
            context("idem-conflict-1"),
            write_command_with_summary("A different mutation payload must not deduplicate."),
        )
        .expect("conflicting write response");

    assert!(matches!(first.adapter, AdapterResponse::Accepted { .. }));
    match conflicting.adapter {
        AdapterResponse::Rejected {
            error_key, reason, ..
        } => {
            assert_eq!(error_key, bm_adapter::AdapterErrorKey::Duplicated);
            assert!(reason.contains("different payload"));
        }
        other => panic!("unexpected conflict response: {other:?}"),
    }
}

#[test]
fn mutation_command_with_same_idempotency_key_is_not_dispatched_twice() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let first = runtime
        .handle(context("idem-write-1"), write_command())
        .expect("first write");
    let second = runtime
        .handle(context("idem-write-1"), write_command())
        .expect("second write");

    assert!(matches!(first.adapter, AdapterResponse::Accepted { .. }));
    match second.adapter {
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => {
            assert_eq!(idempotency_key, "idem-write-1");
            assert_eq!(second.status.as_str(), "duplicated");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn foreign_budget_lease_is_rejected_before_idempotency_state_changes() {
    let lease_owner = EntryRuntime::open(config()).expect("lease owner runtime");
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let foreign_lease = lease_owner.acquire_budget_lease().expect("foreign lease");

    let error = match runtime.handle_with_budget_lease(
        context("idem-foreign-budget-lease"),
        write_command(),
        &foreign_lease,
    ) {
        Ok(_) => panic!("foreign authority lease must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.stage(), "runtime_budget_lease");

    let accepted = runtime
        .handle(context("idem-foreign-budget-lease"), write_command())
        .expect("idempotency key must remain unused after lease rejection");
    assert!(matches!(accepted.adapter, AdapterResponse::Accepted { .. }));
}
