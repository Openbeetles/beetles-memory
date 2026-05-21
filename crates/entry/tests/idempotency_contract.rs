use bm_adapter::{AdapterCommand, AdapterOperation, AdapterResponse};
use bm_entry::{
    EntryAuthConfig, EntryAuthDecision, EntryIdempotencyConfig, EntryIdentity, EntryRuntime,
    EntryRuntimeConfig, EntryScope, EntryStoreConfig, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryWriteRequest, ProfileId, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendKind,
};

fn config() -> EntryRuntimeConfig {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "agent-main".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "local".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_disabled().with_cli(true),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    }
}

fn context(idempotency_key: &str) -> EntryTransportContext {
    EntryTransportContext {
        request_id: "req-write".to_string(),
        transport: bm_adapter::TransportKind::Cli,
        mode: bm_adapter::TransportMode::InProcess,
        operation: AdapterOperation::Write,
        source_id: "source-1".to_string(),
        source_kind: "local_cli".to_string(),
        idempotency_key: idempotency_key.to_string(),
        audit_id: "audit-write".to_string(),
        auth: EntryAuthDecision::authenticated("local", "operator"),
    }
}

fn write_command() -> AdapterCommand {
    AdapterCommand::Write(MemoryWriteRequest::Procedural {
        writes: vec![RuntimeSkillWrite {
            name: "runtime_skill__entry_runtime".to_string(),
            topic: "entry-runtime".to_string(),
            title: "Entry runtime writes".to_string(),
            summary: "Entry runtime dispatches writes through SDK governance.".to_string(),
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
