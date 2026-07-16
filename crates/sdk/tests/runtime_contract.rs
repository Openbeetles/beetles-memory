mod support;
use std::sync::Arc;

use bm_sdk::{
    MemoryCapabilityPolicy, MemoryClock, MemoryIdentity, MemoryPrivacyPolicy, MemoryRecallRequest,
    MemoryRuntime, MemoryScope, NoopMemoryAuditSink, ProfileId, StoreBackendConfig,
};

struct FixedMemoryClock {
    now_secs: u64,
}

impl FixedMemoryClock {
    fn new(now_secs: u64) -> Self {
        Self { now_secs }
    }
}

impl MemoryClock for FixedMemoryClock {
    fn now_secs(&self) -> u64 {
        self.now_secs
    }
}

#[test]
fn runtime_builder_rejects_empty_identity_and_scope() {
    let err = MemoryIdentity::new("", "owner").expect_err("empty agent id rejected");
    assert_eq!(err.stage(), "memory_identity");

    let err = MemoryScope::new("local", "").expect_err("empty chat id rejected");
    assert_eq!(err.stage(), "memory_scope");
}

#[test]
fn runtime_builder_exposes_capabilities_with_store_platform() {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(ProfileId::EspEmbeddedSdk).unwrap(),
    )
    .unwrap();
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(store)
        .clock(Arc::new(FixedMemoryClock::new(1_800_000_000)))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime");

    assert_eq!(runtime.identity().agent_id, "agent-main");
    assert_eq!(runtime.scope().chat_id, "chat-1");
    assert_eq!(runtime.capabilities().profile, ProfileId::EspEmbeddedSdk);
}

#[test]
fn recall_reports_the_single_immutable_session_read_view_it_consumed() {
    let store = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
    )
    .unwrap();
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").unwrap())
        .scope(MemoryScope::new("local", "chat-1").unwrap())
        .store(store)
        .clock(Arc::new(FixedMemoryClock::new(1_800_000_000)))
        .build()
        .unwrap();

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "snapshot receipt".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();

    assert!(recall.store_snapshot_consistent);
}
