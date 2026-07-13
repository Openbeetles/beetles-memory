use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    CanonicalTurnDelta, ConversationKey, DerivedMemoryPlane, DerivedMemoryRef, HostOpaqueRef,
    HostRefRelation, HostRefVisibility, MemoryEvidenceAuthority, MemoryTurnDeliveryStatus,
    MemoryTurnProtocol, MemoryTurnSource, TranscriptAttrEnvelope, TranscriptAttrGovernance,
    TranscriptAttrLink, TranscriptAttrRedactionPolicy, TranscriptAttrScope, TranscriptAttrSource,
    TranscriptAttrSourceKind, TranscriptAttrTarget, TranscriptAttrValueKind,
    TranscriptConversationAlias, TranscriptEvidenceRef, TranscriptInputMessage,
    TranscriptLifecycleRequest, TranscriptLifecycleState, TranscriptLifecycleTransition,
    TranscriptRepairIssueKind, TranscriptReplayView, TranscriptTurnRecord,
};
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{
    StoreBackendConfig, StoreEventScope, StorePlatform, StoreSnapshotJsonDoc,
};
use serde_json::json;

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

fn model_usage_attr(
    key: &ConversationKey,
    turn_id: &str,
    message_id: &str,
) -> TranscriptAttrEnvelope {
    TranscriptAttrEnvelope {
        attr_id: format!("usage-{turn_id}-{message_id}"),
        target: TranscriptAttrTarget {
            key: key.clone(),
            scope: TranscriptAttrScope::Message,
            turn_id: turn_id.to_string(),
            message_id: Some(message_id.to_string()),
        },
        key: "host.beetle_agent.model_usage".to_string(),
        value_kind: TranscriptAttrValueKind::JsonObject,
        schema_ref: Some("beetle-agent.model-usage.v1".to_string()),
        value: json!({
            "status": "measured",
            "input_tokens": 42,
            "output_tokens": 9,
            "usage_source": "provider_reported"
        }),
        visibility: HostRefVisibility::HostUi,
        source: TranscriptAttrSource {
            writer: "beetle-agent".to_string(),
            source_kind: TranscriptAttrSourceKind::ProviderReported,
            written_at: 1_800_000_010,
            audit_reason: "model invocation completed".to_string(),
        },
        governance: TranscriptAttrGovernance {
            max_value_bytes: 4096,
            redaction_policy: TranscriptAttrRedactionPolicy::MetadataSurvivesMask,
            export_allowed: false,
        },
        links: vec![TranscriptAttrLink {
            relation: "model_invocation".to_string(),
            ref_kind: "model_invocation_id".to_string(),
            ref_id: "model-1".to_string(),
        }],
        created_at: 1_800_000_010,
        updated_at: 1_800_000_010,
    }
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
        actor: None,
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
fn store_persists_transcript_attrs_and_replays_visible_message_attrs() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record = transcript_record(&key, "turn-attr", "count this model reply");
    let message_id = record
        .assistant_message
        .as_ref()
        .unwrap()
        .message_id
        .clone();
    store.append_turn(&record).unwrap();

    let attr = model_usage_attr(&key, "turn-attr", &message_id);
    let report = store
        .upsert_transcript_attrs(&key, std::slice::from_ref(&attr))
        .unwrap();

    assert_eq!(report.accepted_attrs, vec![attr.clone()]);
    assert!(report.rejected_attrs.is_empty());
    let replay = store
        .redacted_replay(&key, 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(
        replay.turns[0].assistant_message.as_ref().unwrap().attrs,
        vec![attr]
    );
}

#[test]
fn store_rejects_transcript_attrs_when_target_turn_or_message_is_missing() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record = transcript_record(&key, "turn-attr-target", "target exists");
    let message_id = record
        .assistant_message
        .as_ref()
        .unwrap()
        .message_id
        .clone();
    store.append_turn(&record).unwrap();

    let missing_turn = model_usage_attr(&key, "missing-turn", &message_id);
    let mut missing_message = model_usage_attr(&key, "turn-attr-target", &message_id);
    missing_message.attr_id = "usage-missing-message".to_string();
    missing_message.target.message_id = Some("missing-message".to_string());
    let report = store
        .upsert_transcript_attrs(&key, &[missing_turn, missing_message])
        .unwrap();

    assert!(report.accepted_attrs.is_empty());
    assert_eq!(report.rejected_attrs.len(), 2);
    assert!(store.list_transcript_attrs(&key, None).unwrap().is_empty());
    let repair = store.repair_report(&key).unwrap();
    assert_eq!(repair.checked_attrs, 0);
    assert!(!repair.fail_closed);
}

#[test]
fn store_repair_reports_corrupt_transcript_attr_records_without_failing_replay() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record = transcript_record(&key, "turn-corrupt-attr", "target exists");
    let message_id = record
        .assistant_message
        .as_ref()
        .unwrap()
        .message_id
        .clone();
    store.append_turn(&record).unwrap();

    let mut snapshot = platform.export_store_snapshot().unwrap();
    snapshot.json_docs.push(StoreSnapshotJsonDoc {
        namespace: "conversation_transcript_attr".to_string(),
        key: format!("{}__attr__corrupt", key.storage_key()),
        value: json!({
            "attr_id": "corrupt-attr",
            "target": {
                "key": key.clone(),
                "scope": "message",
                "turn_id": "turn-corrupt-attr",
                "message_id": message_id
            },
            "key": "host.beetle_agent.model_usage",
            "value_kind": "json_object",
            "value": {"status": "measured"},
            "visibility": "not_a_visibility",
            "source": {
                "writer": "beetle-agent",
                "source_kind": "provider_reported",
                "written_at": 1,
                "audit_reason": "corrupt fixture"
            },
            "governance": {
                "max_value_bytes": 4096,
                "redaction_policy": "metadata_survives_mask",
                "export_allowed": false
            },
            "created_at": 1,
            "updated_at": 1
        }),
    });
    let target = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    target.import_store_snapshot(&snapshot).unwrap();
    let target_store = target.conversation_transcript_store();

    let replay = target_store
        .redacted_replay(&key, 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert!(replay.turns[0]
        .assistant_message
        .as_ref()
        .unwrap()
        .attrs
        .is_empty());
    let repair = target_store.repair_report(&key).unwrap();
    assert!(repair.fail_closed);
    assert_eq!(repair.checked_attrs, 1);
    assert!(repair.issues.iter().any(|issue| {
        issue.kind == TranscriptRepairIssueKind::CorruptTranscriptAttrRecord
            && issue.turn_id == "turn-corrupt-attr"
    }));
}

#[test]
fn store_resolves_conversation_transcript_alias_by_legacy_chat_id() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    store
        .remember_conversation_alias(
            &TranscriptConversationAlias::new(
                "space-store",
                "llm.gateway",
                "legacy-chat-a",
                "conversation-store",
                10,
            )
            .unwrap(),
        )
        .unwrap();

    assert_eq!(
        store
            .resolve_conversation_alias("space-store", "llm.gateway", "legacy-chat-a")
            .unwrap()
            .as_deref(),
        Some("conversation-store")
    );
    assert!(store
        .resolve_conversation_alias("space-store", "llm.gateway", "missing-chat")
        .unwrap()
        .is_none());
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
    assert_eq!(report.affected_turn_ids, vec!["turn-1".to_string()]);
    assert_eq!(report.affected_message_ids.len(), 2);
    assert!(report.affected_host_refs.is_empty());
    assert!(report.derived_memory_refs.is_empty());
    let loaded = store.get_turn(&key, "turn-1").unwrap().unwrap();
    assert_eq!(loaded.lifecycle_state, TranscriptLifecycleState::RawDeleted);

    let replay = store
        .redacted_replay(&key, 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(replay.turns[0].input_messages[0].content, None);
    assert_eq!(replay.audit.redacted_messages, 2);
    assert_eq!(replay.redactions.len(), 2);
}

#[test]
fn lifecycle_report_includes_memory_owned_derived_refs_for_affected_turns() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record =
        TranscriptTurnRecord::from_delta(&key, 1, &delta("turn-1", "derive me"), Vec::new(), 10)
            .unwrap();
    let message = record.input_messages[0].clone();
    store.append_turn(&record).unwrap();

    let derived = DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: "long_term:preference:response_style".to_string(),
        subject_id: Some("subject-store".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            turn_id: "turn-1".to_string(),
            message_id: Some(message.message_id),
            subject_id: Some("subject-store".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 11,
    };
    store.append_derived_memory_ref(&key, &derived).unwrap();

    let report = store
        .apply_lifecycle_request(&TranscriptLifecycleRequest {
            key: key.clone(),
            turn_id: Some("turn-1".to_string()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "review_derived_memory".to_string(),
            requested_by: "owner".to_string(),
            requested_at: 20,
        })
        .unwrap();

    assert_eq!(report.affected_turn_ids, vec!["turn-1".to_string()]);
    assert_eq!(report.derived_memory_refs, vec![derived]);
}

#[test]
fn transcript_list_page_returns_bounded_cursor_pages() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    store
        .append_turn(
            &TranscriptTurnRecord::from_delta(&key, 1, &delta("turn-1", "first"), Vec::new(), 10)
                .unwrap(),
        )
        .unwrap();
    store
        .append_turn(
            &TranscriptTurnRecord::from_delta(&key, 2, &delta("turn-2", "second"), Vec::new(), 11)
                .unwrap(),
        )
        .unwrap();

    let first = store.list_turns_page(&key, None, 1).unwrap();
    assert_eq!(first.turns[0].turn_id, "turn-1");
    assert!(first.has_more);
    let second = store
        .list_turns_page(&key, first.next_cursor.as_deref(), 1)
        .unwrap();
    assert_eq!(second.turns[0].turn_id, "turn-2");
    assert!(!second.has_more);
    assert!(second.next_cursor.is_none());
}

#[test]
fn transcript_repair_report_flags_missing_derived_source_turn() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let derived = DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: "long_term:missing-source".to_string(),
        subject_id: Some("subject-store".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            turn_id: "missing-turn".to_string(),
            message_id: Some("missing-message".to_string()),
            subject_id: Some("subject-store".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 11,
    };
    store.append_derived_memory_ref(&key, &derived).unwrap();

    let report = store.repair_report(&key).unwrap();
    assert!(report.fail_closed);
    assert_eq!(report.checked_turns, 0);
    assert_eq!(report.checked_derived_refs, 1);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(
        report.issues[0].kind,
        TranscriptRepairIssueKind::MissingSourceTurn
    );
    assert_eq!(report.issues[0].turn_id, "missing-turn");
}

#[test]
fn transcript_repair_report_flags_missing_derived_source_message() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let store = platform.conversation_transcript_store();
    let key = ConversationKey::new("space-store", "llm.gateway", "conversation-store").unwrap();
    let record = transcript_record(&key, "turn-without-message", "present message");
    store.append_turn(&record).unwrap();
    let derived = DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: "long_term:missing-message".to_string(),
        subject_id: Some("subject-store".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            turn_id: "turn-without-message".to_string(),
            message_id: Some("missing-message".to_string()),
            subject_id: Some("subject-store".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 11,
    };
    store.append_derived_memory_ref(&key, &derived).unwrap();

    let report = store.repair_report(&key).unwrap();

    assert!(report.fail_closed);
    assert_eq!(report.checked_turns, 1);
    assert_eq!(report.checked_derived_refs, 1);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(
        report.issues[0].kind,
        TranscriptRepairIssueKind::MissingSourceMessage
    );
    assert_eq!(
        report.issues[0].message_id.as_deref(),
        Some("missing-message")
    );
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
fn file_store_persists_long_transcript_keys_and_attrs_across_reopen() {
    let root = temp_root("file-long-transcript-key");
    let config = StoreBackendConfig::file(&root, ProfileId::DesktopMacosEmbeddedSdk).unwrap();
    let key = ConversationKey::new(
        "space:local-user-with-default-desktop-memory",
        "llm.gateway",
        format!("work-room:{}", "long-input-segment-".repeat(16)),
    )
    .unwrap();
    let mut turn_delta = delta("turn-long-file-key", "persist long key");
    turn_delta.conversation.channel = key.channel_id.clone();
    turn_delta.conversation.conversation_id = Some(key.conversation_id.clone());
    let record = TranscriptTurnRecord::from_delta(&key, 1, &turn_delta, Vec::new(), 10).unwrap();
    let message_id = record
        .assistant_message
        .as_ref()
        .unwrap()
        .message_id
        .clone();
    let attr = model_usage_attr(&key, "turn-long-file-key", &message_id);

    {
        let platform = StorePlatform::open(config.clone()).unwrap();
        let store = platform.conversation_transcript_store();
        store.append_turn(&record).unwrap();
        store
            .upsert_transcript_attrs(&key, std::slice::from_ref(&attr))
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
        Some("persist long key")
    );
    assert_eq!(
        replay.turns[0].assistant_message.as_ref().unwrap().attrs,
        vec![attr]
    );
}

#[test]
fn snapshot_export_import_carries_conversation_transcript_namespace() {
    let source = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let key = ConversationKey::new("space-snapshot", "llm.gateway", "conversation-store").unwrap();
    let store = source.conversation_transcript_store();
    let record = transcript_record(&key, "turn-snapshot", "snapshot me");
    let message = record.input_messages[0].clone();
    store.append_turn(&record).unwrap();
    let attr = model_usage_attr(&key, "turn-snapshot", &message.message_id);
    store
        .upsert_transcript_attrs(&key, std::slice::from_ref(&attr))
        .unwrap();
    let derived = DerivedMemoryRef {
        plane: DerivedMemoryPlane::ArchiveEvidence,
        store_key: "archive:conversation-store:turn-snapshot".to_string(),
        subject_id: Some("subject-store".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            turn_id: "turn-snapshot".to_string(),
            message_id: Some(message.message_id),
            subject_id: Some("subject-store".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 12,
    };
    store.append_derived_memory_ref(&key, &derived).unwrap();

    let snapshot = source.export_store_snapshot().unwrap();
    assert!(snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript"));
    assert!(snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript_attr"));
    assert!(snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript_derived_ref"));

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
    assert_eq!(replay.turns[0].input_messages[0].attrs, vec![attr]);
    let derived_refs = target
        .conversation_transcript_store()
        .list_derived_memory_refs(&key, Some("turn-snapshot"))
        .unwrap();
    assert_eq!(derived_refs, vec![derived]);
}

#[test]
fn file_snapshot_export_import_preserves_long_transcript_keys_and_attrs() {
    let source_root = temp_root("file-snapshot-source-long-key");
    let target_root = temp_root("file-snapshot-target-long-key");
    let source = StorePlatform::open(
        StoreBackendConfig::file(&source_root, ProfileId::DesktopMacosEmbeddedSdk).unwrap(),
    )
    .unwrap();
    let key = ConversationKey::new(
        "space:local-user-with-default-desktop-memory",
        "llm.gateway",
        format!("work-room:{}", "long-input-segment-".repeat(16)),
    )
    .unwrap();
    let mut turn_delta = delta("turn-long-file-snapshot", "snapshot long key");
    turn_delta.conversation.channel = key.channel_id.clone();
    turn_delta.conversation.conversation_id = Some(key.conversation_id.clone());
    let record = TranscriptTurnRecord::from_delta(&key, 1, &turn_delta, Vec::new(), 10).unwrap();
    let message_id = record
        .assistant_message
        .as_ref()
        .unwrap()
        .message_id
        .clone();
    let attr = model_usage_attr(&key, "turn-long-file-snapshot", &message_id);
    let source_store = source.conversation_transcript_store();
    source_store.append_turn(&record).unwrap();
    source_store
        .upsert_transcript_attrs(&key, std::slice::from_ref(&attr))
        .unwrap();

    let snapshot = source.export_store_snapshot().unwrap();
    assert!(snapshot.json_docs.iter().any(|doc| {
        doc.namespace == "conversation_transcript"
            && doc.key.contains("long-input-segment")
            && doc.key.contains("turn-long-file-snapshot")
    }));
    assert!(snapshot.json_docs.iter().any(|doc| {
        doc.namespace == "conversation_transcript_attr" && doc.key.contains("long-input-segment")
    }));

    let target = StorePlatform::open(
        StoreBackendConfig::file(&target_root, ProfileId::DesktopMacosEmbeddedSdk).unwrap(),
    )
    .unwrap();
    target.import_store_snapshot(&snapshot).unwrap();
    let target_store = target.conversation_transcript_store();
    let turns = target_store.list_turns(&key, 10).unwrap();
    assert_eq!(turns.len(), 1);

    let replay = target_store
        .redacted_replay(&key, 10, TranscriptReplayView::HostUi)
        .unwrap();
    assert_eq!(
        replay.turns[0].input_messages[0].content.as_deref(),
        Some("snapshot long key")
    );
    assert_eq!(
        replay.turns[0].assistant_message.as_ref().unwrap().attrs,
        vec![attr]
    );
}
