use bm_adapter::{
    dispatch_adapter_command, AdapterAuthContext, AdapterCommand, AdapterEnvelope,
    AdapterOperation, AdapterResponse, AdapterSdkReport, AdapterSource, TransportKind,
    TransportMode,
};
use bm_sdk::{
    MemoryIdentity, MemoryRecallRequest, MemoryRuntime, MemoryScope, MemoryWriteRequest, ProfileId,
    RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig, StorePlatform,
};

fn main() -> bm_sdk::Result<()> {
    let profile = ProfileId::ServerLinuxMemoryGateway;
    let store = StorePlatform::open(StoreBackendConfig::in_memory(profile)?)?;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("gateway-agent", "owner-default")?)
        .scope(MemoryScope::new("gateway", "chat-1")?)
        .profile(profile)
        .store_platform(store)
        .build()?;

    runtime.write(MemoryWriteRequest::Procedural {
        writes: vec![RuntimeSkillWrite {
            name: "gateway_dispatch_guard".to_string(),
            topic: "gateway".to_string(),
            title: "Gateway dispatch guard".to_string(),
            summary: "Memory gateways dispatch transport commands into one SDK runtime.".to_string(),
            content: "1. decode transport payload\n2. build adapter envelope\n3. dispatch into the SDK runtime"
                .to_string(),
            citations: vec!["memory gateway example".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        }],
        source: RuntimeSkillWriteSource::Manual,
    })?;

    let envelope = AdapterEnvelope {
        request_id: "req-1".to_string(),
        transport: TransportKind::Http,
        mode: TransportMode::Server,
        operation: AdapterOperation::Recall,
        source: AdapterSource {
            source_id: "generic-host".to_string(),
            source_kind: "external_ai_project".to_string(),
            agent_id: "gateway-agent".to_string(),
            owner_id: "owner-default".to_string(),
            channel: "gateway".to_string(),
            chat_id: "chat-1".to_string(),
        },
        auth: AdapterAuthContext {
            authenticated: true,
            auth_kind: "local-test".to_string(),
            principal: "operator".to_string(),
        },
        idempotency_key: "idem-1".to_string(),
        audit_id: "audit-1".to_string(),
        payload: AdapterCommand::Recall(MemoryRecallRequest {
            query: "gateway dispatch".to_string(),
            limit: 4,
        }),
    };

    match dispatch_adapter_command(&runtime, envelope)? {
        AdapterResponse::Accepted {
            report: AdapterSdkReport::Recall(report),
            ..
        } => assert!(!report.procedural_hits.is_empty()),
        response => panic!("unexpected adapter response: {response:?}"),
    }

    println!("memory-gateway smoke passed");
    Ok(())
}
