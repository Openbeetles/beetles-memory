use bm_a2a::{bridge_message_specs, merge_peer_visibility, A2aPermission};
use bm_adapter::AdapterOperation;

#[test]
fn peer_capability_does_not_upgrade_local_memory_capability() {
    assert!(!merge_peer_visibility(false, true));
    assert!(merge_peer_visibility(true, true));
    assert!(!merge_peer_visibility(true, false));
}

#[test]
fn a2a_bridge_never_carries_executor_or_tool_permissions() {
    for message in bridge_message_specs() {
        assert!(!message.permissions.contains(&A2aPermission::Executor));
        assert!(!message.permissions.contains(&A2aPermission::Tool));
        assert!(!message.permissions.contains(&A2aPermission::Workflow));
    }
}

#[test]
fn bridge_messages_are_memory_report_or_request_only() {
    let operations: Vec<_> = bridge_message_specs()
        .iter()
        .filter_map(|message| message.operation)
        .collect();
    assert!(operations.contains(&AdapterOperation::Write));
    assert!(operations.contains(&AdapterOperation::Recall));
    assert!(operations.contains(&AdapterOperation::Project));
    assert!(operations.contains(&AdapterOperation::LongTermList));
    assert!(operations.contains(&AdapterOperation::LongTermDetail));
    assert!(operations.contains(&AdapterOperation::LongTermMutate));
    assert!(operations.contains(&AdapterOperation::LongTermPolicy));
    assert!(operations.contains(&AdapterOperation::TranscriptAttrWrite));
}

#[test]
fn transcript_attr_a2a_message_is_declared_as_thin_adapter_operation() {
    let message = bridge_message_specs()
        .into_iter()
        .find(|message| message.name == "memory_transcript_attr_write_request")
        .expect("transcript attr A2A message");

    assert_eq!(
        message.operation,
        Some(AdapterOperation::TranscriptAttrWrite)
    );
    assert!(message.permissions.contains(&A2aPermission::MemoryReport));
}
