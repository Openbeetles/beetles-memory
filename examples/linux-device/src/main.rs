use bm_adapter::{AdapterCommand, AdapterOperation};
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryLocalTransport, EntryRuntime,
    EntryRuntimeConfig, EntryScope, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryRecallRequest, ProfileId, StoreBackendConfig,
};

fn main() -> bm_sdk::Result<()> {
    let root = unique_temp_dir("beetle-memory-linux-device-entry");
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let runtime = EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "device-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            conversation_id: None,
            channel: "device".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::file(root, ProfileId::LinuxDeviceStandaloneMemory)?
            .with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })?;

    let recall = runtime.handle(
        context(&runtime, AdapterOperation::Recall, "idem-linux-recall"),
        AdapterCommand::Recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
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

fn context(
    runtime: &EntryRuntime,
    operation: AdapterOperation,
    idempotency_key: &str,
) -> EntryTransportContext {
    EntryTransportContext::new(
        format!("linux-device-{operation:?}"),
        bm_adapter::TransportKind::Cli,
        bm_adapter::TransportMode::InProcess,
        operation,
        "linux-device",
        "local_cli",
        idempotency_key,
        format!("audit-linux-device-{operation:?}"),
        runtime.authenticate_local_transport(EntryLocalTransport::InProcess, "operator"),
    )
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}
