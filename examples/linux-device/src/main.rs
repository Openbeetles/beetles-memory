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
    let root = unique_temp_dir("beetle-memory-linux-device-entry");
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let runtime = EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::LinuxDeviceStandaloneMemory,
        identity: EntryIdentity {
            agent_id: "device-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "device".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::File,
            data_path: Some(root),
            fsync: false,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })?;

    runtime.handle(
        context(AdapterOperation::Write, "idem-linux-write"),
        AdapterCommand::Write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "runtime_skill__device_entry_guard".to_string(),
                topic: "device-entry".to_string(),
                title: "Device entry guard".to_string(),
                summary: "Linux device deployments keep local entry runtime available.".to_string(),
                content: "1. Open the local file store.\n2. Normalize device source metadata.\n3. Dispatch through EntryRuntime.\n4. Keep server-grade features gated by profile."
                    .to_string(),
                citations: vec!["linux device entry example".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        }),
    )?;
    let recall = runtime.handle(
        context(AdapterOperation::Recall, "idem-linux-recall"),
        AdapterCommand::Recall(MemoryRecallRequest {
            query: "device entry".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        }),
    )?;
    assert_eq!(recall.status.as_str(), "accepted");
    println!("linux-device entry smoke passed");
    Ok(())
}

fn context(operation: AdapterOperation, idempotency_key: &str) -> EntryTransportContext {
    EntryTransportContext {
        request_id: format!("linux-device-{operation:?}"),
        transport: bm_adapter::TransportKind::Cli,
        mode: bm_adapter::TransportMode::InProcess,
        operation,
        source_id: "linux-device".to_string(),
        source_kind: "local_cli".to_string(),
        idempotency_key: idempotency_key.to_string(),
        audit_id: format!("audit-linux-device-{operation:?}"),
        auth: EntryAuthDecision::authenticated("local", "operator"),
    }
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}
