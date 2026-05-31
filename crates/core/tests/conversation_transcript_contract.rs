use bm_core::memory::{
    ActorAttribution, CanonicalTurnDelta, ConversationKey, HostOpaqueRef, HostRefRelation,
    HostRefVisibility, MemoryEvidenceAuthority, MemoryTurnDeliveryStatus, MemoryTurnProtocol,
    MemoryTurnSource, RedactedTranscriptSlice, ToolObservationDigest, TranscriptInputMessage,
    TranscriptLifecycleState, TranscriptRedactionState, TranscriptReplayView, TranscriptTurnRecord,
};

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_core::memory::IngressKind::User,
        channel: "llm.gateway".to_string(),
        provider: Some("ollama".to_string()),
        protocol: MemoryTurnProtocol::OllamaChat,
        endpoint: Some("/api/chat".to_string()),
        model_alias: Some("qwen".to_string()),
        model_resolved: Some("qwen3".to_string()),
        request_id: Some("req-1".to_string()),
        client_conversation_hint: Some("window-a".to_string()),
    }
}

fn delivered_delta(turn_id: &str) -> CanonicalTurnDelta {
    CanonicalTurnDelta {
        turn_id: turn_id.to_string(),
        conversation: bm_core::memory::ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "legacy-chat-a".to_string(),
            conversation_id: Some("conversation-a".to_string()),
        },
        subject: "subject-qingchuan".to_string(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: turn_source(),
        input_messages: vec![TranscriptInputMessage::user("请把这个事项挂到宿主任务上")
            .with_observed_at(1_800_000_001)
            .with_speaker("owner-human", "human")],
        assistant_message: Some(
            TranscriptInputMessage::assistant("已记录证据，但不会解释宿主任务状态。")
                .with_observed_at(1_800_000_002)
                .with_speaker("assistant-main", "llm_agent"),
        ),
        tool_observations: vec![ToolObservationDigest {
            observation_id: "tool-1".to_string(),
            tool_name: "host.lookup".to_string(),
            summary: "host returned opaque ref".to_string(),
            external_content: true,
        }],
        external_content_used: true,
        candidate_ids: vec!["candidate-1".to_string()],
    }
}

#[test]
fn conversation_key_separates_memory_space_channel_and_conversation() {
    let left = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let right = ConversationKey::new("space-b", "llm.gateway", "conversation-a").unwrap();

    assert_ne!(left, right);
    assert_ne!(left.storage_key(), right.storage_key());
    assert!(left.storage_key().contains("space-a"));
    assert!(left.storage_key().contains("conversation-a"));
}

#[test]
fn conversation_key_storage_key_is_not_ambiguous_when_components_contain_separators() {
    let left = ConversationKey::new("space__a", "channel", "conversation").unwrap();
    let right = ConversationKey::new("space", "a__channel", "conversation").unwrap();

    assert_ne!(left.storage_key(), right.storage_key());
}

#[test]
fn transcript_turn_record_keeps_actor_attribution_and_host_refs_opaque() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let host_ref = HostOpaqueRef {
        host_kind: "beetle-agent".to_string(),
        business_ref_type: "task".to_string(),
        business_ref_id: "task-42".to_string(),
        relation: HostRefRelation::Related,
        visibility: HostRefVisibility::HostUi,
        label: Some("宿主任务引用".to_string()),
    };

    let record =
        TranscriptTurnRecord::from_delta(&key, 7, &delivered_delta("turn-7"), vec![host_ref], 10)
            .unwrap();

    assert_eq!(record.key, key);
    assert_eq!(record.turn_id, "turn-7");
    assert_eq!(record.sequence, 7);
    assert_eq!(record.subject, "subject-qingchuan");
    assert_eq!(
        record.actor.subject_id.as_deref(),
        Some("subject-qingchuan")
    );
    assert_eq!(record.input_messages[0].actor.speaker_id, "owner-human");
    assert_eq!(record.input_messages[0].actor.speaker_kind, "human");
    assert_eq!(
        record.assistant_message.as_ref().unwrap().actor.speaker_id,
        "assistant-main"
    );
    assert_eq!(record.host_refs.len(), 1);
    assert_eq!(record.host_refs[0].business_ref_type, "task");
    assert_eq!(record.host_refs[0].business_ref_id, "task-42");
    assert_eq!(record.lifecycle_state, TranscriptLifecycleState::Active);
    assert_eq!(
        record.redaction_state,
        TranscriptRedactionState::RawAvailable
    );
}

#[test]
fn transcript_turn_record_serialization_roundtrips_contract_fields() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let record =
        TranscriptTurnRecord::from_delta(&key, 3, &delivered_delta("turn-json"), Vec::new(), 10)
            .unwrap();

    let json = serde_json::to_string(&record).unwrap();
    let decoded: TranscriptTurnRecord = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded, record);
    assert_eq!(decoded.key.memory_space_id, "space-a");
    assert_eq!(decoded.turn_id, "turn-json");
    assert_eq!(
        decoded.input_messages[0].authority,
        MemoryEvidenceAuthority::UserAsserted
    );
}

#[test]
fn redacted_replay_keeps_attribution_and_refs_without_raw_deleted_content() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut record =
        TranscriptTurnRecord::from_delta(&key, 1, &delivered_delta("turn-1"), Vec::new(), 10)
            .unwrap();
    record.redaction_state = TranscriptRedactionState::RawDeleted;
    record.lifecycle_state = TranscriptLifecycleState::RawDeleted;
    record.actor = ActorAttribution::for_subject("subject-qingchuan");

    let replay = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::HostUi,
        &[record.clone()],
    );

    assert_eq!(replay.key, key);
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(replay.turns[0].turn_id, "turn-1");
    assert_eq!(replay.turns[0].input_messages[0].content, None);
    assert!(replay.turns[0].input_messages[0].redacted);
    assert_eq!(
        replay.turns[0].input_messages[0]
            .actor
            .subject_id
            .as_deref(),
        Some("subject-qingchuan")
    );
    assert_eq!(replay.audit.redacted_messages, 2);
}

#[test]
fn non_owner_replay_views_redact_private_internal_authority() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut delta = delivered_delta("turn-private");
    delta.input_messages = vec![TranscriptInputMessage::new(
        "assistant",
        "private scratch",
        MemoryEvidenceAuthority::PrivateGardenInternal,
    )];
    delta.assistant_message = None;
    let record = TranscriptTurnRecord::from_delta(&key, 1, &delta, Vec::new(), 10).unwrap();

    let replay =
        RedactedTranscriptSlice::from_records(key, TranscriptReplayView::HostUi, &[record]);

    assert_eq!(replay.turns[0].input_messages[0].content, None);
    assert!(replay.turns[0].input_messages[0].redacted);
    assert_eq!(replay.audit.redacted_messages, 1);
}
