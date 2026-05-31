use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    CanonicalTurnDelta, ConversationKey, HostOpaqueRef, HostRefRelation, HostRefVisibility,
    MemoryTurnDeliveryStatus, MemoryTurnProtocol, MemoryTurnSource, TranscriptInputMessage,
    TranscriptLifecycleRequest, TranscriptLifecycleState, TranscriptLifecycleTransition,
    TranscriptReplayView, TranscriptTurnRecord,
};
use bm_core::platform::Platform;
use bm_store::{StoreBackendConfig, StoreEventScope, StorePlatform};

fn temp_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-transcript-store-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_core::memory::IngressKind::User,
        channel: "llm.gateway".to_string(),
        provider: Some("ollama".to_string()),
        protocol: MemoryTurnProtocol::OllamaChat,
        endpoint: Some("/api/chat".to_string()),
        model_alias: Some("qwen".to_string()),
        model_resolved: Some("qwen3".to_string()),
        request_id: Some("req-store".to_string()),
        client_conversation_hint: Some("window-store".to_string()),
    }
}

fn transcript_record(key: &ConversationKey, turn_id: &str, user: &str) -> TranscriptTurnRecord {
    TranscriptTurnRecord::from_delta(key, 1, &delta(turn_id, user), Vec::new(), 10).unwrap()
}

fn delta(turn_id: &str, user: &str) -> CanonicalTurnDelta {
    CanonicalTurnDelta {
        turn_id: turn_id.to_string(),
        conversation: bm_core::memory::ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "legacy-chat-a".to_string(),
            conversation_id: Some("conversation-store".to_string()),
        },
        subject: "subject-store".to_string(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: turn_source(),
        input_messages: vec![TranscriptInputMessage::user(user).with_speaker("owner", "human")],
        assistant_message: Some(TranscriptInputMessage::assistant("noted")),
        tool_observations: Vec::new(),
        external_content_used: false,
        candidate_ids: Vec::new(),
    }
}

#[test]
fn store_persists_transcript_by_memory_space_channel_and_conversation() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let host_ref = HostOpaqueRef {
        host_kind: "host".to_string(),
        business_ref_type: "ticket".to_string(),
        business_ref_id: "T-1".to_string(),
        relation: HostRefRelation::EvidenceFor,
        visibility: HostRefVisibility::OperatorAudit,
        label: None,
    };
    let record =
        TranscriptTurnRecord::from_delta(&key, 1, &delta("turn-1", "hello"), vec![host_ref], 10)
            .unwrap();

    let report = store.append_turn(&record).unwrap();
    assert!(report.committed);
    assert_eq!(report.sequence, 1);

    let loaded = store.get_turn(&key, "turn-1").unwrap().unwrap();
    assert_eq!(loaded.key, key);
    assert_eq!(loaded.turn_id, "turn-1");
    assert_eq!(loaded.host_refs[0].business_ref_id, "T-1");

    let turns = store.list_turns(&key, 10).unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].input_messages[0].actor.speaker_id, "owner");

    let replay = store
        .redacted_replay(&key, 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(
        replay.turns[0].input_messages[0].content.as_deref(),
        Some("hello")
    );
}

#[test]
fn lifecycle_request_masks_raw_transcript_content_but_keeps_audit_key() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record =
        TranscriptTurnRecord::from_delta(&key, 1, &delta("turn-1", "mask me"), Vec::new(), 10)
            .unwrap();
    store.append_turn(&record).unwrap();

    let report = store
        .apply_lifecycle_request(&TranscriptLifecycleRequest {
            key: key.clone(),
            turn_id: Some("turn-1".to_string()),
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "user_redaction_request".to_string(),
            requested_by: "owner".to_string(),
            requested_at: 20,
        })
        .unwrap();

    assert_eq!(report.affected_turns, 1);
    let loaded = store.get_turn(&key, "turn-1").unwrap().unwrap();
    assert_eq!(loaded.lifecycle_state, TranscriptLifecycleState::RawDeleted);

    let replay = store
        .redacted_replay(&key, 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns[0].input_messages[0].content, None);
    assert_eq!(replay.audit.redacted_messages, 2);
}

#[test]
fn transcript_write_events_preserve_memory_space_scope_and_record_key() {
    let scope = StoreEventScope::new("agent-a", "owner-a", "llm.gateway", "legacy-chat-a")
        .with_memory_space("space-store")
        .with_subject("subject-store")
        .with_conversation("conversation-store");
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .unwrap()
        .with_event_scope(scope);
    let platform = StorePlatform::open_in_memory(config).unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record =
        TranscriptTurnRecord::from_delta(&key, 1, &delta("turn-1", "event"), Vec::new(), 10)
            .unwrap();

    store.append_turn(&record).unwrap();
    let events = platform.read_events().unwrap();
    let event = events
        .iter()
        .find(|event| event.kind_name == "memory.write" && event.plane == "conversation_transcript")
        .expect("transcript write event");

    assert_eq!(event.scope.memory_space_id, "space-store");
    assert_eq!(event.scope.subject_id, "subject-store");
    assert_eq!(
        event.scope.conversation_id.as_deref(),
        Some("conversation-store")
    );
    assert!(event.record_key.contains("space-store"));
    assert!(event.record_key.contains("conversation-store"));
    assert!(event.record_key.contains("turn-1"));
}

#[test]
fn file_store_persists_transcript_across_reopen() {
    let root = temp_root("file-reopen");
    let config = StoreBackendConfig::file(&root, ProfileId::DesktopMacosEmbeddedSdk).unwrap();
    let key = ConversationKey::new("space-file", "llm.gateway", "conversation-store").unwrap();

    {
        let platform = StorePlatform::open(config.clone()).unwrap();
        platform
            .conversation_transcript_store()
            .append_turn(&transcript_record(&key, "turn-file", "persist me"))
            .unwrap();
    }

    let reopened = StorePlatform::open(config).unwrap();
    let replay = reopened
        .conversation_transcript_store()
        .redacted_replay(&key, 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(
        replay.turns[0].input_messages[0].content.as_deref(),
        Some("persist me")
    );
}

#[test]
fn snapshot_export_import_carries_conversation_transcript_namespace() {
    let source = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let key = ConversationKey::new("space-snapshot", "llm.gateway", "conversation-store").unwrap();
    source
        .conversation_transcript_store()
        .append_turn(&transcript_record(&key, "turn-snapshot", "snapshot me"))
        .unwrap();

    let snapshot = source.export_store_snapshot().unwrap();
    assert!(snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript"));

    let target = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    target.import_store_snapshot(&snapshot).unwrap();
    let replay = target
        .conversation_transcript_store()
        .redacted_replay(&key, 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(
        replay.turns[0].input_messages[0].content.as_deref(),
        Some("snapshot me")
    );
}
