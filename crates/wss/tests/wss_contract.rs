use bm_adapter::AdapterOperation;
use bm_sdk::{
    resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId,
    RuntimeBudgetReport,
};
use bm_wss::{message_specs, WssBudget};

#[test]
fn inbound_messages_map_to_adapter_commands() {
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
    assert!(operations.contains(&AdapterOperation::Capabilities));
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

    let budget = WssBudget::from_runtime_budget(&RuntimeBudgetReport::static_for_profile(
        ProfileId::EspStandaloneMemory,
    ));
    assert!(budget.max_frame_bytes <= 8 * 1024);
    assert!(budget.max_subscriptions <= 4);
}
