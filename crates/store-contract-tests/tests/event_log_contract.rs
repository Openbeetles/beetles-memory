use bm_core::feature_gate::ProfileId;
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{
    InMemoryStoreEngine, MemoryStoreEvent, MemoryStoreEventKind, StoreBackendConfig, StoreEventLog,
    StoreEventScope, StorePlatform, STORE_SCHEMA_VERSION,
};

#[test]
fn event_log_records_semantic_events_in_append_order() {
    let engine = InMemoryStoreEngine::default();
    let scope = StoreEventScope::new("agent-a", "owner-a", "chat", "chat-a");

    let write = MemoryStoreEvent::new(
        "evt-0001",
        MemoryStoreEventKind::MemoryWrite,
        scope.clone(),
        10,
    )
    .with_plane("long_term")
    .with_record_key("ltm-1")
    .with_content_hash("hash-a");
    let projection = MemoryStoreEvent::new(
        "evt-0002",
        MemoryStoreEventKind::MemoryProjection,
        scope,
        11,
    )
    .with_payload("visible_plane_count", "3")
    .with_payload("redaction_count", "1");

    engine.append_event(write.clone()).unwrap();
    engine.append_event(projection.clone()).unwrap();

    let events = engine.read_events().unwrap();
    assert_eq!(events, vec![write, projection]);
    assert!(events
        .iter()
        .all(|event| event.schema_version == STORE_SCHEMA_VERSION));
}

#[test]
fn duplicate_event_ids_are_rejected() {
    let engine = InMemoryStoreEngine::default();
    let event = MemoryStoreEvent::new(
        "evt-dup",
        MemoryStoreEventKind::RuntimeLifecycle,
        StoreEventScope::system("runtime-open"),
        12,
    );

    engine.append_event(event.clone()).unwrap();
    let err = engine
        .append_event(event)
        .expect_err("duplicate event id must be rejected");

    assert_eq!(err.stage(), "store_event_log");
    assert!(err.to_string().contains("duplicate"));
}

#[test]
fn file_store_can_reopen_without_runtime_event_id_collision() {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-reopen-event-id-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let config = StoreBackendConfig::file(&root, ProfileId::ServerLinuxDevFull).unwrap();
    StorePlatform::open(config.clone()).expect("first open");
    StorePlatform::open(config).expect("second open must not collide on runtime event id");
}

#[test]
fn store_platform_events_use_configured_scope_and_content_hash() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .unwrap()
        .with_event_scope(StoreEventScope::new(
            "agent-a", "owner-a", "local", "chat-a",
        ));
    let platform = StorePlatform::open_in_memory(config).unwrap();

    platform
        .state_fs()
        .write("runtime/state.json", br#"{"value":1}"#)
        .unwrap();
    platform
        .state_fs()
        .write("runtime/state.json", br#"{"value":2}"#)
        .unwrap();

    let writes = platform
        .read_events()
        .unwrap()
        .into_iter()
        .filter(|event| event.kind_name == "memory.write" && event.plane == "state_fs")
        .collect::<Vec<_>>();

    assert_eq!(writes.len(), 2);
    assert!(writes.iter().all(|event| event.scope.agent_id == "agent-a"));
    assert!(writes.iter().all(|event| event.scope.owner_id == "owner-a"));
    assert!(writes.iter().all(|event| event.scope.chat_id == "chat-a"));
    assert_eq!(writes[0].record_key, "runtime/state.json");
    assert_eq!(writes[1].record_key, "runtime/state.json");
    assert_ne!(writes[0].content_hash, writes[1].content_hash);
}
