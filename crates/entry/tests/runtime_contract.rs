use bm_adapter::{AdapterCommand, AdapterOperation, AdapterResponse, AdapterSdkReport};
use bm_entry::{
    EntryAuthConfig, EntryAuthDecision, EntryIdempotencyConfig, EntryIdentity, EntryRuntime,
    EntryRuntimeConfig, EntryScope, EntryStoreConfig, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryRecallRequest, ProfileId, StoreBackendKind,
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

fn context(operation: AdapterOperation, idempotency_key: &str) -> EntryTransportContext {
    EntryTransportContext {
        request_id: "req-1".to_string(),
        transport: bm_adapter::TransportKind::Cli,
        mode: bm_adapter::TransportMode::InProcess,
        operation,
        source_id: "source-1".to_string(),
        source_kind: "local_cli".to_string(),
        idempotency_key: idempotency_key.to_string(),
        audit_id: "audit-1".to_string(),
        auth: EntryAuthDecision::authenticated("local", "operator"),
    }
}

#[test]
fn entry_runtime_dispatches_adapter_command_through_sdk_runtime() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let response = runtime
        .handle(
            context(AdapterOperation::Recall, "idem-recall-1"),
            AdapterCommand::Recall(MemoryRecallRequest {
                query: "release".to_string(),
                limit: 2,
            }),
        )
        .expect("entry handle");

    match response.adapter {
        AdapterResponse::Accepted {
            request_id,
            audit_id,
            report: AdapterSdkReport::Recall(report),
        } => {
            assert_eq!(request_id, "req-1");
            assert_eq!(audit_id, "audit-1");
            assert_eq!(report.query, "release");
            assert_eq!(response.status.as_str(), "accepted");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn entry_runtime_rejects_operation_mismatch_before_sdk_runtime_call() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    let response = runtime
        .handle(
            context(AdapterOperation::Write, "idem-mismatch-1"),
            AdapterCommand::Recall(MemoryRecallRequest {
                query: "release".to_string(),
                limit: 2,
            }),
        )
        .expect("entry handle");

    match response.adapter {
        AdapterResponse::Rejected { error_key, .. } => {
            assert_eq!(error_key, bm_adapter::AdapterErrorKey::OperationMismatch);
            assert_eq!(response.status.as_str(), "rejected");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}
