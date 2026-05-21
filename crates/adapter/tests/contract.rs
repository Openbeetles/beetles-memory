use std::sync::Arc;

use bm_adapter::{
    dispatch_adapter_command, AdapterAuthContext, AdapterCommand, AdapterEnvelope, AdapterErrorKey,
    AdapterOperation, AdapterResponse, AdapterSdkReport, AdapterSource, TransportKind,
    TransportMode,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryClock, MemoryIdentity, MemoryPrivacyPolicy, MemoryRecallRequest,
    MemoryRuntime, MemoryScope, NoopMemoryAuditSink, ProfileId, StoreBackendConfig, StorePlatform,
};

struct FixedClock;

impl MemoryClock for FixedClock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}

fn runtime() -> MemoryRuntime {
    let store = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .profile(ProfileId::ServerLinuxDevFull)
        .store_platform(store)
        .clock(Arc::new(FixedClock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime")
}

fn envelope<T>(operation: AdapterOperation, payload: T) -> AdapterEnvelope<T> {
    AdapterEnvelope {
        request_id: "req-1".to_string(),
        transport: TransportKind::Http,
        mode: TransportMode::Server,
        operation,
        source: AdapterSource {
            source_id: "source-1".to_string(),
            source_kind: "http_client".to_string(),
            agent_id: "agent-main".to_string(),
            owner_id: "owner-default".to_string(),
            channel: "local".to_string(),
            chat_id: "chat-1".to_string(),
        },
        auth: AdapterAuthContext {
            authenticated: true,
            auth_kind: "token".to_string(),
            principal: "operator".to_string(),
        },
        idempotency_key: "idem-1".to_string(),
        audit_id: "audit-1".to_string(),
        payload,
    }
}

#[test]
fn recall_command_dispatches_through_memory_runtime() {
    let runtime = runtime();
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            AdapterOperation::Recall,
            AdapterCommand::Recall(MemoryRecallRequest {
                query: "release".to_string(),
                limit: 2,
            }),
        ),
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Accepted {
            request_id,
            audit_id,
            report: AdapterSdkReport::Recall(report),
        } => {
            assert_eq!(request_id, "req-1");
            assert_eq!(audit_id, "audit-1");
            assert_eq!(report.query, "release");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn operation_mismatch_is_rejected_before_runtime_call() {
    let runtime = runtime();
    let response = dispatch_adapter_command(
        &runtime,
        envelope(
            AdapterOperation::Write,
            AdapterCommand::Recall(MemoryRecallRequest {
                query: "release".to_string(),
                limit: 2,
            }),
        ),
    )
    .expect("dispatch");

    match response {
        AdapterResponse::Rejected { error_key, .. } => {
            assert_eq!(error_key, AdapterErrorKey::OperationMismatch);
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn adapter_crate_manifest_has_no_direct_core_or_store_dependency() {
    let manifest = std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")))
        .expect("manifest");
    let dependencies = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or_default()
        .split('[')
        .next()
        .unwrap_or_default();

    assert!(!dependencies.contains("bm-core"));
    assert!(!dependencies.contains("bm-store"));
    assert!(dependencies.contains("bm-sdk"));
}
