use bm_adapter::{AdapterCommand, AdapterOperation};
use bm_entry::{
    EntryAuthConfig, EntryAuthDecision, EntryIdentity, EntryIdempotencyConfig, EntryRuntime,
    EntryRuntimeConfig, EntryScope, EntryStoreConfig, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryRecallRequest, MemoryWriteRequest,
    ProfileId, RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendKind,
};

fn main() -> bm_sdk::Result<()> {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let runtime = EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::EspStandaloneMemory,
        identity: EntryIdentity {
            agent_id: "esp-memory-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "device".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::Embedded,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 16 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })?;

    assert!(runtime.capability().wss_client.visible);
    assert!(!runtime.capability().http_server.visible);

    runtime.handle(
        context(AdapterOperation::Write, "idem-esp-write"),
        AdapterCommand::Write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "runtime_skill__compact_entry_guard".to_string(),
                topic: "compact-entry".to_string(),
                title: "Compact entry guard".to_string(),
                summary: "ESP standalone memory can run compact EntryRuntime locally.".to_string(),
                content: "1. Use embedded store.\n2. Keep sqlite disabled.\n3. Dispatch compact memory commands through EntryRuntime.\n4. Keep server listeners hidden."
                    .to_string(),
                citations: vec!["esp standalone entry example".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        }),
    )?;
    let recall = runtime.handle(
        context(AdapterOperation::Recall, "idem-esp-recall"),
        AdapterCommand::Recall(MemoryRecallRequest {
            query: "compact entry".to_string(),
            limit: 2,
        }),
    )?;
    assert_eq!(recall.status.as_str(), "accepted");
    println!("esp-standalone-memory entry smoke passed");
    Ok(())
}

fn context(operation: AdapterOperation, idempotency_key: &str) -> EntryTransportContext {
    EntryTransportContext {
        request_id: format!("esp-standalone-{operation:?}"),
        transport: bm_adapter::TransportKind::Cli,
        mode: bm_adapter::TransportMode::InProcess,
        operation,
        source_id: "esp-standalone".to_string(),
        source_kind: "local_cli".to_string(),
        idempotency_key: idempotency_key.to_string(),
        audit_id: format!("audit-esp-standalone-{operation:?}"),
        auth: EntryAuthDecision::authenticated("local", "operator"),
    }
}
