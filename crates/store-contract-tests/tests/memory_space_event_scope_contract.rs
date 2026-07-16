mod support;
use bm_core::feature_gate::ProfileId;
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{StoreBackendConfig, StoreEventScope};

#[test]
fn event_scope_carries_memory_space_subject_and_conversation() {
    let scope = StoreEventScope::new("agent-a", "owner-a", "llm.gateway", "chat-a")
        .with_memory_space("space-main")
        .with_subject("subject-qingchuan")
        .with_conversation("ollama-window-a");
    let json = serde_json::to_string(&scope).expect("serialize");
    let decoded: StoreEventScope = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(decoded.memory_space_id, "space-main");
    assert_eq!(decoded.subject_id, "subject-qingchuan");
    assert_eq!(decoded.conversation_id.as_deref(), Some("ollama-window-a"));
}

#[test]
fn store_platform_events_preserve_memory_space_scope() {
    let scope = StoreEventScope::new("agent-a", "owner-a", "llm.gateway", "chat-a")
        .with_memory_space("space-main")
        .with_subject("subject-qingchuan")
        .with_conversation("ollama-window-a");
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .with_event_scope(scope);
    let platform = support::open_store_in_memory(config).expect("platform");

    platform
        .state_fs()
        .write("runtime/state.json", br#"{"value":1}"#)
        .expect("write");
    let events = platform.read_events().expect("events");
    let write = events
        .iter()
        .find(|event| event.kind_name == "memory.write" && event.plane == "state_fs")
        .expect("state write event");

    assert_eq!(write.scope.memory_space_id, "space-main");
    assert_eq!(write.scope.subject_id, "subject-qingchuan");
    assert_eq!(
        write.scope.conversation_id.as_deref(),
        Some("ollama-window-a")
    );
}
