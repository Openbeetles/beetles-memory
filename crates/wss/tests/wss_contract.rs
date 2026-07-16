use bm_adapter::AdapterOperation;
use bm_sdk::{
    resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryStoreHandle,
    ProfileId, StoreBackendConfig,
};
use bm_wss::message_specs;

#[test]
fn inbound_messages_map_to_adapter_commands() {
    assert_eq!(
        message_specs()
            .iter()
            .map(|message| message.name)
            .collect::<Vec<_>>(),
        vec![
            "command.write",
            "command.recall",
            "command.project",
            "command.inspect",
            "command.replay",
            "command.long_term.list",
            "command.long_term.detail",
            "command.long_term.mutate",
            "command.long_term.policy",
            "command.transcript.attrs",
            "command.capabilities",
            "subscribe.projection",
            "subscribe.inspection",
            "subscribe.replay",
            "subscribe.capability",
            "event.report",
            "event.lifecycle",
            "event.error",
        ]
    );
    let operations: Vec<_> = message_specs()
        .iter()
        .filter_map(|message| message.inbound_operation)
        .collect();
    assert!(operations.contains(&AdapterOperation::Write));
    assert!(operations.contains(&AdapterOperation::Recall));
    assert!(operations.contains(&AdapterOperation::Project));
    assert!(operations.contains(&AdapterOperation::Inspect));
    assert!(operations.contains(&AdapterOperation::Replay));
    assert!(operations.contains(&AdapterOperation::LongTermList));
    assert!(operations.contains(&AdapterOperation::LongTermDetail));
    assert!(operations.contains(&AdapterOperation::LongTermMutate));
    assert!(operations.contains(&AdapterOperation::LongTermPolicy));
    assert!(operations.contains(&AdapterOperation::TranscriptAttrWrite));
    assert!(operations.contains(&AdapterOperation::Capabilities));
}

#[test]
fn transcript_attr_wss_message_is_declared_as_thin_adapter_operation() {
    let message = message_specs()
        .iter()
        .find(|message| message.name == "command.transcript.attrs")
        .expect("transcript attr WSS message");

    assert_eq!(
        message.inbound_operation,
        Some(AdapterOperation::TranscriptAttrWrite)
    );
    assert!(!message.private_raw_allowed);
}

#[test]
fn esp_standalone_wss_is_summary_only_with_bounded_frames() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    policy.adapter.wss_enabled = true;
    let catalog = resolve_memory_capabilities(
        ProfileId::EspStandaloneMemory,
        &policy,
        &MemoryPrivacyPolicy::standard_private_boundary(),
    )
    .expect("catalog");

    assert!(catalog.adapter.wss.client_allowed);
    assert!(!catalog.adapter.wss.server_allowed);
    assert!(!catalog.adapter.wss.private_data_allowed);
    assert!(catalog.adapter.wss.visible);

    let report = MemoryStoreHandle::open(
        StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory).expect("store config"),
    )
    .expect("store")
    .runtime_budget();
    assert!(report.adapter_budget.wss_frame_max_bytes <= 8 * 1024);
    assert!(report.adapter_budget.wss_max_subscriptions <= 4);
}

#[test]
fn production_wss_api_exposes_no_budget_injection_surface() {
    let source = include_str!("../src/lib.rs");

    assert!(!source.contains("pub struct WssBudget"));
    assert!(!source.contains("budget: WssBudget"));
    assert!(!source.contains("WssBudget::from_runtime_budget"));
    assert!(!source.contains("buffer.len() > 8192"));
    assert!(source.contains("adapter_budget.http_header_max_bytes"));
    assert!(source.contains("adapter_budget.wss_frame_max_bytes"));
}
