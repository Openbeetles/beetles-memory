use bm_core::memory::{
    ActorAttribution, CanonicalTurnDelta, ConversationKey, ConversationTranscriptStore,
    DerivedMemoryPlane, DerivedMemoryRef, HostOpaqueRef, HostRefRelation, HostRefVisibility,
    MemoryEvidenceAuthority, MemoryTurnDeliveryStatus, MemoryTurnProtocol, MemoryTurnSource,
    RedactedTranscriptSlice, ToolObservationDigest, TranscriptCommitReport,
    TranscriptConversationAlias, TranscriptEvidenceRef, TranscriptInputMessage,
    TranscriptLifecycleReport, TranscriptLifecycleRequest, TranscriptLifecycleState,
    TranscriptRedactionReason, TranscriptRedactionState, TranscriptRepairIssueKind,
    TranscriptReplayView, TranscriptTurnRecord,
};
use bm_core::Result;

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
        actor: None,
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

fn host_ref(id: &str, visibility: HostRefVisibility) -> HostOpaqueRef {
    HostOpaqueRef {
        host_kind: "generic-host".to_string(),
        business_ref_type: "ticket".to_string(),
        business_ref_id: id.to_string(),
        relation: HostRefRelation::Related,
        visibility,
        label: Some(format!("opaque {id}")),
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
fn transcript_turn_record_uses_host_provided_actor_attribution() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut delta = delivered_delta("turn-host-actor");
    delta.actor = Some(ActorAttribution {
        speaker_id: "runtime-dispatcher".to_string(),
        speaker_kind: "runtime".to_string(),
        subject_id: Some("subject-qingchuan".to_string()),
        actor_subject_id: Some("subject-human-owner".to_string()),
        mounted_subject_id: Some("subject-agent-mounted".to_string()),
        agent_id: Some("agent-alpha".to_string()),
        triggered_by: Some("host:event:abc".to_string()),
    });

    let record = TranscriptTurnRecord::from_delta(&key, 2, &delta, Vec::new(), 10).unwrap();

    assert_eq!(record.actor.speaker_id, "runtime-dispatcher");
    assert_eq!(
        record.actor.actor_subject_id.as_deref(),
        Some("subject-human-owner")
    );
    assert_eq!(
        record.actor.mounted_subject_id.as_deref(),
        Some("subject-agent-mounted")
    );
    assert_eq!(record.actor.agent_id.as_deref(), Some("agent-alpha"));
    assert_eq!(record.actor.triggered_by.as_deref(), Some("host:event:abc"));
    assert_eq!(
        record.input_messages[0].actor.actor_subject_id.as_deref(),
        Some("subject-human-owner")
    );
    assert_eq!(
        record.input_messages[0].actor.mounted_subject_id.as_deref(),
        Some("subject-agent-mounted")
    );
    assert_eq!(
        record
            .assistant_message
            .as_ref()
            .unwrap()
            .actor
            .agent_id
            .as_deref(),
        Some("agent-alpha")
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
fn transcript_turn_record_from_delta_does_not_record_non_delivered_assistant_message() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut delta = delivered_delta("turn-incomplete");
    delta.delivery_status = MemoryTurnDeliveryStatus::IncompleteStream;

    let record = TranscriptTurnRecord::from_delta(&key, 3, &delta, Vec::new(), 10).unwrap();

    assert_eq!(
        record.delivery_status,
        MemoryTurnDeliveryStatus::IncompleteStream
    );
    assert_eq!(record.input_messages.len(), 1);
    assert!(record.assistant_message.is_none());
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

#[test]
fn host_ref_visibility_is_enforced_and_reported_per_replay_view() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let record = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-visibility"),
        vec![
            host_ref("internal", HostRefVisibility::Internal),
            host_ref("host", HostRefVisibility::HostUi),
            host_ref("model", HostRefVisibility::ModelContext),
            host_ref("operator", HostRefVisibility::OperatorAudit),
            host_ref("export", HostRefVisibility::Export),
        ],
        10,
    )
    .unwrap();

    let host_ui = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::HostUi,
        std::slice::from_ref(&record),
    );
    assert_eq!(
        host_ui.turns[0]
            .host_refs
            .iter()
            .map(|host_ref| host_ref.business_ref_id.as_str())
            .collect::<Vec<_>>(),
        vec!["host", "export"]
    );
    assert_eq!(host_ui.audit.redacted_host_refs, 3);
    assert!(host_ui.redactions.iter().any(|item| {
        item.reason == TranscriptRedactionReason::HostRefVisibility
            && item.host_ref_index == Some(0)
            && item.turn_id == "turn-visibility"
    }));

    let model = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::ModelContext,
        std::slice::from_ref(&record),
    );
    assert_eq!(model.turns[0].host_refs.len(), 1);
    assert_eq!(model.turns[0].host_refs[0].business_ref_id, "model");

    let export =
        RedactedTranscriptSlice::from_records(key, TranscriptReplayView::Export, &[record]);
    assert_eq!(export.turns[0].host_refs.len(), 1);
    assert_eq!(export.turns[0].host_refs[0].business_ref_id, "export");
}

#[test]
fn host_ref_label_is_redacted_for_non_owner_views() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let record = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-host-ref-label"),
        vec![
            host_ref("host", HostRefVisibility::HostUi),
            host_ref("model", HostRefVisibility::ModelContext),
            host_ref("operator", HostRefVisibility::OperatorAudit),
            host_ref("export", HostRefVisibility::Export),
        ],
        10,
    )
    .unwrap();

    let owner = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::RawOwnerOnly,
        std::slice::from_ref(&record),
    );
    assert!(owner.turns[0]
        .host_refs
        .iter()
        .all(|host_ref| host_ref.label.is_some()));

    let host_ui = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::HostUi,
        std::slice::from_ref(&record),
    );
    assert_eq!(host_ui.turns[0].host_refs[0].business_ref_id, "host");
    assert_eq!(
        host_ui.turns[0].host_refs[0].label.as_deref(),
        Some("opaque host")
    );
    assert_eq!(host_ui.turns[0].host_refs[1].business_ref_id, "export");
    assert!(host_ui.turns[0].host_refs[1].label.is_none());

    let model = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::ModelContext,
        std::slice::from_ref(&record),
    );
    assert_eq!(model.turns[0].host_refs[0].business_ref_id, "model");
    assert!(model.turns[0].host_refs[0].label.is_none());

    let operator = RedactedTranscriptSlice::from_records(
        key.clone(),
        TranscriptReplayView::OperatorAudit,
        std::slice::from_ref(&record),
    );
    assert!(operator.turns[0]
        .host_refs
        .iter()
        .all(|host_ref| host_ref.label.is_none()));

    let export =
        RedactedTranscriptSlice::from_records(key, TranscriptReplayView::Export, &[record]);
    assert_eq!(export.turns[0].host_refs[0].business_ref_id, "export");
    assert!(export.turns[0].host_refs[0].label.is_none());
}

#[test]
fn redaction_report_records_message_reasons() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut delta = delivered_delta("turn-private-report");
    delta.input_messages = vec![TranscriptInputMessage::new(
        "assistant",
        "private scratch",
        MemoryEvidenceAuthority::PrivateGardenInternal,
    )];
    delta.assistant_message = None;
    let record = TranscriptTurnRecord::from_delta(&key, 1, &delta, Vec::new(), 10).unwrap();

    let replay =
        RedactedTranscriptSlice::from_records(key, TranscriptReplayView::HostUi, &[record]);

    assert_eq!(replay.audit.redacted_messages, 1);
    assert!(replay.redactions.iter().any(|item| {
        item.reason == TranscriptRedactionReason::PrivateAuthority
            && item.message_id.is_some()
            && item.source_authority == Some(MemoryEvidenceAuthority::PrivateGardenInternal)
    }));
}

struct RepairFixtureStore {
    turns: Vec<TranscriptTurnRecord>,
    derived_refs: Vec<DerivedMemoryRef>,
}

impl ConversationTranscriptStore for RepairFixtureStore {
    fn append_turn(&self, _record: &TranscriptTurnRecord) -> Result<TranscriptCommitReport> {
        unimplemented!("repair fixture is read-only")
    }

    fn remember_conversation_alias(&self, _alias: &TranscriptConversationAlias) -> Result<()> {
        unimplemented!("repair fixture is read-only")
    }

    fn resolve_conversation_alias(
        &self,
        _memory_space_id: &str,
        _channel_id: &str,
        _chat_id: &str,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    fn get_turn(
        &self,
        _key: &ConversationKey,
        _turn_id: &str,
    ) -> Result<Option<TranscriptTurnRecord>> {
        unimplemented!("repair fixture is read-only")
    }

    fn list_turns(
        &self,
        _key: &ConversationKey,
        _limit: usize,
    ) -> Result<Vec<TranscriptTurnRecord>> {
        Ok(self.turns.clone())
    }

    fn append_derived_memory_ref(
        &self,
        _key: &ConversationKey,
        _derived: &DerivedMemoryRef,
    ) -> Result<()> {
        unimplemented!("repair fixture is read-only")
    }

    fn list_derived_memory_refs(
        &self,
        _key: &ConversationKey,
        _turn_id: Option<&str>,
    ) -> Result<Vec<DerivedMemoryRef>> {
        Ok(self.derived_refs.clone())
    }

    fn apply_lifecycle_request(
        &self,
        _request: &TranscriptLifecycleRequest,
    ) -> Result<TranscriptLifecycleReport> {
        unimplemented!("repair fixture is read-only")
    }
}

#[test]
fn transcript_repair_report_flags_mismatched_orphan_duplicate_and_corrupt_records() {
    let key = ConversationKey::new("space-a", "llm.gateway", "conversation-a").unwrap();
    let mut first = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-repair-a"),
        Vec::new(),
        10,
    )
    .unwrap();
    let mut duplicate_sequence = TranscriptTurnRecord::from_delta(
        &key,
        1,
        &delivered_delta("turn-repair-b"),
        Vec::new(),
        11,
    )
    .unwrap();
    duplicate_sequence.sequence = first.sequence;
    first.key = ConversationKey::new("other-space", "llm.gateway", "conversation-a").unwrap();
    let derived = DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: String::new(),
        subject_id: Some("subject-default".to_string()),
        source: TranscriptEvidenceRef {
            memory_space_id: "other-space".to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: "turn-repair-a".to_string(),
            message_id: None,
            subject_id: Some("subject-default".to_string()),
            authority: Some(MemoryEvidenceAuthority::UserAsserted),
        },
        created_at: 12,
    };
    let store = RepairFixtureStore {
        turns: vec![first, duplicate_sequence],
        derived_refs: vec![derived],
    };

    let report = store.repair_report(&key).unwrap();
    let kinds = report
        .issues
        .iter()
        .map(|issue| issue.kind)
        .collect::<Vec<_>>();

    assert!(report.fail_closed);
    assert!(kinds.contains(&TranscriptRepairIssueKind::CorruptRecord));
    assert!(kinds.contains(&TranscriptRepairIssueKind::DuplicateTurnCursor));
    assert!(kinds.contains(&TranscriptRepairIssueKind::OrphanDerivedRef));
    assert!(kinds.contains(&TranscriptRepairIssueKind::MismatchedSourceKey));
}
