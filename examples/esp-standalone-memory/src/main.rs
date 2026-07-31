use bm_adapter::{AdapterCommand, AdapterOperation};
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryLocalTransport, EntryRuntime,
    EntryRuntimeConfig, EntryScope, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    GovernedRuntimeSkillWriteInput, MemoryCapabilityPolicy, MemoryPrivacyClass, MemoryPrivacyPolicy,
    MemoryRecallRequest, MemoryWriteRequest, ProfileId, RuntimeSkillCreationRef,
    RuntimeSkillOwningScope, RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
};

fn main() -> bm_sdk::Result<()> {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let runtime = EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "esp-memory-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "device".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory)?.with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 16 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })?;

    assert!(runtime.capability().wss_client.visible);
    assert!(!runtime.capability().http_server.visible);

    runtime.handle(
        context(&runtime, AdapterOperation::Write, "idem-esp-write"),
        AdapterCommand::Write(MemoryWriteRequest::Procedural {
            writes: vec![GovernedRuntimeSkillWriteInput {
                write: RuntimeSkillWrite {
                    name: "compact_entry_guard".to_string(),
                    topic: "compact-entry".to_string(),
                    title: "Compact entry guard".to_string(),
                    summary: "ESP standalone memory can run compact EntryRuntime locally."
                        .to_string(),
                    content: "1. Use embedded store.\n2. Keep sqlite disabled.\n3. Dispatch compact memory commands through EntryRuntime.\n4. Keep server listeners hidden."
                        .to_string(),
                    citations: vec!["esp standalone entry example".to_string()],
                    source_chat_id: Some("chat-1".to_string()),
                    observed_at: 1_700_000_000,
                },
                creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
                    candidate_ref: "example:esp-standalone-entry-guard".to_string(),
                    verification_receipt_digest:
                        "sha256:2222222222222222222222222222222222222222222222222222222222222222"
                            .to_string(),
                },
                privacy_class: MemoryPrivacyClass::PublicRuntime,
            }],
            owning_scope: RuntimeSkillOwningScope::SharedProgram,
            source: RuntimeSkillWriteSource::Manual,
        }),
    )?;
    let recall = runtime.handle(
        context(&runtime, AdapterOperation::Recall, "idem-esp-recall"),
        AdapterCommand::Recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: "compact entry".to_string(),
            limit: 2,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        }),
    )?;
    assert_eq!(recall.status.as_str(), "accepted");
    println!("esp-standalone-memory entry smoke passed");
    Ok(())
}

fn context(
    runtime: &EntryRuntime,
    operation: AdapterOperation,
    idempotency_key: &str,
) -> EntryTransportContext {
    EntryTransportContext::new(
        format!("esp-standalone-{operation:?}"),
        bm_adapter::TransportKind::Cli,
        bm_adapter::TransportMode::InProcess,
        operation,
        "esp-standalone",
        "local_cli",
        idempotency_key,
        format!("audit-esp-standalone-{operation:?}"),
        runtime.authenticate_local_transport(EntryLocalTransport::InProcess, "operator"),
    )
}
