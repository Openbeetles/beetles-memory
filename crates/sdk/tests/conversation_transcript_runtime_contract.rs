#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::commit_canonical_turn_delta;
use bm_core::platform::Platform as _;
use bm_sdk::{
    ActorAttribution, CanonicalTurnDelta, ConversationKey, ConversationScope, DerivedMemoryPlane,
    DerivedMemoryRef, HostOpaqueRef, HostRefRelation, HostRefVisibility, LongTermMemoryDraft,
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryInspectionRequest, MemoryMaintenanceRequest, MemoryPrivacyClass, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryReplayRequest, MemorySemanticJudgmentSource,
    MemorySpaceExportRequest, MemoryTranscriptAttrWriteRequest, MemoryTranscriptCommitRequest,
    MemoryTranscriptExportRequest, MemoryTranscriptLifecycleRequest, MemoryTranscriptReplayRequest,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    MemoryWriteCandidate, MemoryWriteRequest, ParsedLongTermMemoryExtraction, PressureLevel,
    ProfileId, RuntimeLifecycleModeInput, RuntimeSkillReuseOutcome, RuntimeSkillWrite,
    TranscriptAttrEnvelope, TranscriptAttrGovernance, TranscriptAttrLink,
    TranscriptAttrRedactionPolicy, TranscriptAttrScope, TranscriptAttrSource,
    TranscriptAttrSourceKind, TranscriptAttrTarget, TranscriptAttrValueKind, TranscriptEvidenceRef,
    TranscriptInputMessage, TranscriptLifecycleTransition, TranscriptRedactionReason,
    TranscriptReplayView,
};
use serde_json::json;

use support::{
    empty_store_platform, test_runtime_with_scope_and_subject, StaticHttpClient, StaticLlmClient,
};

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_sdk::IngressKind::User,
        channel: "llm.gateway".to_string(),
        provider: Some("ollama".to_string()),
        protocol: MemoryTurnProtocol::OllamaChat,
        endpoint: Some("/api/chat".to_string()),
        model_alias: Some("qwen".to_string()),
        model_resolved: Some("qwen3".to_string()),
        request_id: Some("req-transcript".to_string()),
        client_conversation_hint: Some("window-a".to_string()),
    }
}

fn finalize_request(user: &str, assistant: &str) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: format!("turn-{}", user.len()),
            conversation: ConversationScope {
                channel: "llm.gateway".to_string(),
                chat_id: "chat-a".to_string(),
                conversation_id: Some("conversation-a".to_string()),
            },
            subject: "subject-default".to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            actor: None,
            input_messages: vec![TranscriptInputMessage::user(user)],
            assistant_message: Some(TranscriptInputMessage::assistant(assistant)),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        tool_calls: 0,
        runtime_skill_selected_ids: Vec::new(),
        task_learning_selected_ids: Vec::new(),
        reuse_outcome_note: String::new(),
        tool_usage_feedback: None,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn host_ref() -> HostOpaqueRef {
    host_ref_with_visibility("T-42", HostRefVisibility::Export)
}

fn host_ref_with_visibility(id: &str, visibility: HostRefVisibility) -> HostOpaqueRef {
    HostOpaqueRef {
        host_kind: "generic-host".to_string(),
        business_ref_type: "ticket".to_string(),
        business_ref_id: id.to_string(),
        relation: HostRefRelation::Related,
        visibility,
        label: Some(format!("opaque ticket {id}")),
    }
}

fn model_usage_attr(
    key: ConversationKey,
    turn_id: &str,
    message_id: &str,
    visibility: HostRefVisibility,
) -> TranscriptAttrEnvelope {
    TranscriptAttrEnvelope {
        attr_id: format!("usage-{turn_id}-{message_id}"),
        target: TranscriptAttrTarget {
            key,
            scope: TranscriptAttrScope::Message,
            turn_id: turn_id.to_string(),
            message_id: Some(message_id.to_string()),
        },
        key: "host.beetle_agent.model_usage".to_string(),
        value_kind: TranscriptAttrValueKind::JsonObject,
        schema_ref: Some("beetle-agent.model-usage.v1".to_string()),
        value: json!({
            "status": "measured",
            "input_tokens": 33,
            "output_tokens": 7,
            "usage_source": "provider_reported"
        }),
        visibility,
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

#[test]
fn finalize_turn_commits_conversation_transcript_in_runtime_memory_space() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );

    let report = runtime
        .finalize_turn_and_maintain(None, None, finalize_request("记住我是青川", "好的，青川。"))
        .unwrap();
    assert!(report.session_commit.committed);

    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();

    assert_eq!(replay.slice.turns.len(), 1);
    assert_eq!(replay.slice.turns[0].subject, "subject-default");
    assert_eq!(
        replay.slice.turns[0].input_messages[0].content.as_deref(),
        Some("记住我是青川")
    );
    assert_eq!(
        platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        2
    );
}

#[test]
fn desktop_profiles_can_read_host_ui_transcript_without_debug_replay() {
    for profile in [
        ProfileId::DesktopMacosStandaloneMemory,
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
    ] {
        let platform = empty_store_platform(profile);
        let runtime = test_runtime_with_scope_and_subject(
            platform,
            profile,
            "llm.gateway",
            "chat-a",
            "subject-default",
        );

        runtime
            .commit_transcript(MemoryTranscriptCommitRequest {
                turn: finalize_request("桌面写入聊天", "桌面读回聊天。").turn,
                host_refs: vec![host_ref_with_visibility("UI-1", HostRefVisibility::HostUi)],
            })
            .expect("desktop transcript commit should be allowed");

        let replay = runtime
            .replay_transcript(MemoryTranscriptReplayRequest {
                memory_space_id: runtime.memory_space_id().to_string(),
                channel_id: "llm.gateway".to_string(),
                conversation_id: "conversation-a".to_string(),
                limit: 10,
                cursor: None,
                view: TranscriptReplayView::HostUi,
            })
            .expect("desktop HostUi transcript read should be allowed");

        assert_eq!(replay.slice.view, TranscriptReplayView::HostUi);
        assert_eq!(replay.slice.turns.len(), 1);
        assert_eq!(
            replay.slice.turns[0].input_messages[0].content.as_deref(),
            Some("桌面写入聊天")
        );
        assert_eq!(replay.slice.turns[0].host_refs.len(), 1);
        assert_eq!(replay.slice.turns[0].host_refs[0].business_ref_id, "UI-1");

        assert!(
            runtime
                .replay(MemoryReplayRequest {
                    chat_id: "chat-a".to_string(),
                    limit: 10,
                })
                .is_err(),
            "desktop HostUi transcript read must not enable intelligence replay for {}",
            profile.as_str()
        );
    }
}

#[test]
fn runtime_records_transcript_attrs_and_replays_host_ui_message_usage() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: finalize_request("统计这条模型回复", "已统计。").turn,
            host_refs: Vec::new(),
        })
        .unwrap();
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::RawOwnerOnly,
        })
        .unwrap();
    let turn = &replay.slice.turns[0];
    let message_id = turn.assistant_message.as_ref().unwrap().message_id.clone();
    let attr = model_usage_attr(
        replay.slice.key.clone(),
        &turn.turn_id,
        &message_id,
        HostRefVisibility::HostUi,
    );

    let report = runtime
        .record_transcript_attrs(MemoryTranscriptAttrWriteRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            attrs: vec![attr.clone()],
            idempotency_key: Some("usage-write-1".to_string()),
            dry_run: false,
        })
        .unwrap();
    assert_eq!(report.accepted_attrs, vec![attr.clone()]);
    assert!(report.rejected_attrs.is_empty());
    assert!(report.redactions_preview.is_empty());
    assert!(!report.profile_budget_applied);
    assert!(report.audit_event_id.is_some());

    let host_ui = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();
    assert_eq!(
        host_ui.slice.turns[0]
            .assistant_message
            .as_ref()
            .unwrap()
            .attrs,
        vec![attr]
    );
}

#[test]
fn runtime_transcript_attr_dry_run_reports_without_persisting() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: finalize_request("dry run attr", "not persisted.").turn,
            host_refs: Vec::new(),
        })
        .unwrap();
    let key = ConversationKey::new(
        runtime.memory_space_id().to_string(),
        "llm.gateway",
        "conversation-a",
    )
    .unwrap();
    let attr = model_usage_attr(key, "turn-12", "missing-message", HostRefVisibility::HostUi);

    let report = runtime
        .record_transcript_attrs(MemoryTranscriptAttrWriteRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            attrs: vec![attr],
            idempotency_key: Some("usage-write-dry-run".to_string()),
            dry_run: true,
        })
        .unwrap();

    assert!(report.accepted_attrs.is_empty());
    assert_eq!(report.rejected_attrs.len(), 1);
    assert_eq!(report.redactions_preview.len(), 1);
    assert_eq!(
        report.redactions_preview[0].reason,
        TranscriptRedactionReason::AttrVisibility
    );
    assert!(report.audit_event_id.is_some());
    let host_ui = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();
    assert!(host_ui.slice.turns[0]
        .assistant_message
        .as_ref()
        .unwrap()
        .attrs
        .is_empty());
}

#[test]
fn finalize_turn_preserves_host_provided_actor_attribution() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut request = finalize_request("由宿主事件触发", "已记录。");
    request.turn.actor = Some(ActorAttribution {
        speaker_id: "runtime-dispatcher".to_string(),
        speaker_kind: "runtime".to_string(),
        subject_id: Some("subject-default".to_string()),
        actor_subject_id: Some("subject-human".to_string()),
        mounted_subject_id: Some("subject-agent".to_string()),
        agent_id: Some("agent-alpha".to_string()),
        triggered_by: Some("host:event:42".to_string()),
    });

    runtime
        .finalize_turn_and_maintain(None, None, request)
        .unwrap();

    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::OperatorAudit,
        })
        .unwrap();

    let turn = &replay.slice.turns[0];
    assert_eq!(
        turn.actor.actor_subject_id.as_deref(),
        Some("subject-human")
    );
    assert_eq!(
        turn.actor.mounted_subject_id.as_deref(),
        Some("subject-agent")
    );
    assert_eq!(turn.actor.agent_id.as_deref(), Some("agent-alpha"));
    assert_eq!(
        turn.input_messages[0].actor.triggered_by.as_deref(),
        Some("host:event:42")
    );
}

#[test]
fn projection_uses_transcript_substrate_after_session_shadow_is_cleared() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request(
                "transcript-only user evidence",
                "transcript-only assistant evidence",
            ),
        )
        .unwrap();
    platform
        .replay_harness()
        .session_store()
        .clear("chat-a")
        .unwrap();

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "what evidence exists?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();

    assert_eq!(
        platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        0
    );
    assert!(projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "transcript-only user evidence"));
    assert!(projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "transcript-only assistant evidence"));
}

#[test]
fn projection_does_not_fallback_to_session_shadow_after_transcript_mask() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalize = runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request(
                "masked transcript user evidence",
                "masked transcript assistant evidence",
            ),
        )
        .unwrap();
    let turn_id = finalize.transcript_commit.unwrap().turn_id;
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(turn_id),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "projection_should_fail_closed".to_string(),
        })
        .unwrap();

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "what evidence exists?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();

    assert_eq!(
        platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        2
    );
    assert!(!projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content.contains("masked transcript")));
}

#[test]
fn transcript_backed_projection_honors_recent_message_limit() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request("limit first user", "limit first assistant"),
        )
        .unwrap();
    runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request("limit second user", "limit second assistant"),
        )
        .unwrap();
    platform
        .replay_harness()
        .session_store()
        .clear("chat-a")
        .unwrap();

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "limit evidence".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 1,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();

    assert_eq!(projection.context.recent_messages.len(), 1);
    assert!(!projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "limit first user"));
    assert!(projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "limit second assistant"));
}

#[test]
fn fresh_runtime_does_not_fallback_to_session_shadow_after_transcript_mask() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalize = runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request(
                "fresh runtime masked user evidence",
                "fresh runtime masked assistant evidence",
            ),
        )
        .unwrap();
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(finalize.transcript_commit.unwrap().turn_id),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "fresh_runtime_consumer_should_fail_closed".to_string(),
        })
        .unwrap();

    let fresh_runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let projection = fresh_runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "what evidence exists?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();

    assert_eq!(
        platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        2
    );
    assert!(!projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content.contains("fresh runtime masked")));
}

#[test]
fn fresh_runtime_does_not_fallback_to_session_shadow_after_transcript_raw_delete() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalize = runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request(
                "fresh runtime deleted user evidence",
                "fresh runtime deleted assistant evidence",
            ),
        )
        .unwrap();
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(finalize.transcript_commit.unwrap().turn_id),
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "fresh_runtime_delete_raw_should_fail_closed".to_string(),
        })
        .unwrap();

    let fresh_runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let inspect = fresh_runtime
        .inspect(MemoryInspectionRequest {
            query: "deleted evidence".to_string(),
            system_max_len: 4096,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .unwrap();

    assert_eq!(
        platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        2
    );
    assert!(!format!("{:?}", inspect.working).contains("fresh runtime deleted"));
}

#[test]
fn fresh_runtime_fails_closed_when_transcript_alias_is_corrupt() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request(
                "corrupt alias raw user evidence",
                "corrupt alias raw assistant evidence",
            ),
        )
        .unwrap();
    assert_eq!(
        platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        2
    );

    let mut snapshot = platform.replay_harness().export_store_snapshot().unwrap();
    let alias_doc = snapshot
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == "conversation_transcript_alias")
        .expect("conversation transcript alias doc");
    alias_doc.value = serde_json::json!({
        "memory_space_id": 7,
        "channel_id": "llm.gateway",
        "chat_id": "chat-a",
        "conversation_id": "conversation-a",
        "updated_at": 1_800_000_000_u64,
    });
    let corrupt_platform = empty_store_platform(profile);
    corrupt_platform
        .replay_harness()
        .import_store_snapshot(&snapshot)
        .unwrap();
    assert_eq!(
        corrupt_platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        2
    );

    let fresh_runtime = test_runtime_with_scope_and_subject(
        corrupt_platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let projection = fresh_runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "what evidence exists?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .unwrap();
    assert!(projection.context.recent_messages.is_empty());
    assert!(!projection
        .context
        .recent_messages
        .iter()
        .any(|message| message.content.contains("corrupt alias raw")));
}

#[test]
fn recall_inspect_and_maintenance_do_not_fallback_to_session_shadow_after_transcript_mask() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalize = runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request(
                "forbidden redaction user evidence",
                "forbidden redaction assistant evidence",
            ),
        )
        .unwrap();
    let turn_id = finalize.transcript_commit.unwrap().turn_id;
    runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(turn_id),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "consumer_paths_should_fail_closed".to_string(),
        })
        .unwrap();

    let recall = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "evidence".to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .unwrap();
    assert!(!format!("{:?}", recall.working).contains("forbidden redaction"));

    let inspect = runtime
        .inspect(MemoryInspectionRequest {
            query: "evidence".to_string(),
            system_max_len: 4096,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .unwrap();
    assert!(!format!("{:?}", inspect.working).contains("forbidden redaction"));

    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response("Summary: consumer mask check");
    let maintenance = runtime
        .maintain(
            &mut http,
            &llm,
            MemoryMaintenanceRequest {
                ingress: bm_sdk::IngressKind::User,
                user_content: "new unmasked input".to_string(),
                reply_content: "new unmasked reply".to_string(),
                tool_calls: 0,
                external_content_used: false,
                runtime_skill_selected_ids: Vec::new(),
                task_learning_selected_ids: Vec::new(),
                reuse_outcome: RuntimeSkillReuseOutcome::Neutral,
                reuse_outcome_note: String::new(),
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .unwrap()
        .report
        .expect("maintenance report");
    assert_eq!(maintenance.after_count, 0);
    assert_eq!(
        platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        2
    );
}

#[test]
fn runtime_lifecycle_request_deletes_raw_transcript_without_deleting_session_shadow() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .finalize_turn_and_maintain(None, None, finalize_request("需要脱敏", "已记录。"))
        .unwrap();

    let lifecycle = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: None,
            transition: TranscriptLifecycleTransition::DeleteRaw,
            reason: "privacy_request".to_string(),
        })
        .unwrap();
    assert_eq!(lifecycle.transcript.affected_turns, 1);

    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();

    assert_eq!(replay.slice.turns[0].input_messages[0].content, None);
    assert!(replay.slice.audit.redacted_messages > 0);
    assert_eq!(
        platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        2
    );
}

#[test]
fn lifecycle_request_without_affected_turns_reports_noop() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .finalize_turn_and_maintain(None, None, finalize_request("存在的 turn", "已记录。"))
        .unwrap();

    let lifecycle = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some("missing-turn".to_string()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "no matching turn".to_string(),
        })
        .unwrap();

    assert_eq!(lifecycle.transcript.affected_turns, 0);
    assert!(!lifecycle.lifecycle_report.changed);
}

#[test]
fn candidate_write_records_transcript_derived_ref_for_lifecycle_impact() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalize = runtime
        .finalize_turn_and_maintain(None, None, finalize_request("我偏好简洁回答", "已记录。"))
        .unwrap();
    assert!(finalize
        .transcript_commit
        .as_ref()
        .is_some_and(|commit| commit.committed));

    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::RawOwnerOnly,
        })
        .unwrap();
    let turn = &replay.slice.turns[0];
    let message = &turn.input_messages[0];
    let evidence_ref = TranscriptEvidenceRef {
        memory_space_id: runtime.memory_space_id().to_string(),
        channel_id: "llm.gateway".to_string(),
        conversation_id: "conversation-a".to_string(),
        turn_id: turn.turn_id.clone(),
        message_id: Some(message.message_id.clone()),
        subject_id: Some("subject-default".to_string()),
        authority: Some(MemoryEvidenceAuthority::UserAsserted),
    };

    let write = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-concise-style".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Preference,
                    topic: "response_style".to_string(),
                },
                privacy: MemoryPrivacyClass::SharedWithSubject,
                content: MemoryCandidateContent::Text {
                    topic: "response_style".to_string(),
                    body: "The user prefers concise answers.".to_string(),
                    keywords: vec!["concise".to_string()],
                },
                evidence_refs: vec![evidence_ref.display_citation()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                    source: MemorySemanticJudgmentSource::LlmGovernance,
                    decision: MemoryCandidateSemanticDecision::Accept,
                    governed_target: Some(MemoryCandidateTarget::LongTermMemory {
                        kind: LongTermMemoryKind::Preference,
                        topic: "response_style".to_string(),
                    }),
                    reason: "user_asserted_preference".to_string(),
                }),
            }],
        })
        .unwrap();
    assert_eq!(write.changed, 1);

    let err = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(turn.turn_id.clone()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "review_preference_source".to_string(),
        })
        .expect_err("facet-backed transcript source redaction must fail closed");

    assert_eq!(err.stage(), "transcript_lifecycle_facet_preflight");
}

#[test]
fn candidate_write_records_only_second_stage_accepted_derived_refs() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request("我发了一段不能直接变成长记忆的材料", "我会只保留证据。"),
        )
        .unwrap();

    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::RawOwnerOnly,
        })
        .unwrap();
    let turn = &replay.slice.turns[0];
    let message = &turn.input_messages[0];
    let evidence_ref = TranscriptEvidenceRef {
        memory_space_id: runtime.memory_space_id().to_string(),
        channel_id: "llm.gateway".to_string(),
        conversation_id: "conversation-a".to_string(),
        turn_id: turn.turn_id.clone(),
        message_id: Some(message.message_id.clone()),
        subject_id: Some("subject-default".to_string()),
        authority: Some(MemoryEvidenceAuthority::UserAsserted),
    };

    let write = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![
                MemoryWriteCandidate {
                    candidate_id: "candidate-structured-fact".to_string(),
                    authority: MemoryEvidenceAuthority::UserAsserted,
                    target: MemoryCandidateTarget::LongTermMemory {
                        kind: LongTermMemoryKind::Fact,
                        topic: "release_notes_blob".to_string(),
                    },
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    content: MemoryCandidateContent::Text {
                        topic: "release_notes_blob".to_string(),
                        body: "# Release notes\n- copied item\n- another copied item".to_string(),
                        keywords: vec!["release".to_string()],
                    },
                    evidence_refs: vec![evidence_ref.display_citation()],
                    canonical_entities: Vec::new(),
                    semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                        source: MemorySemanticJudgmentSource::LlmGovernance,
                        decision: MemoryCandidateSemanticDecision::Accept,
                        governed_target: Some(MemoryCandidateTarget::LongTermMemory {
                            kind: LongTermMemoryKind::Fact,
                            topic: "release_notes_blob".to_string(),
                        }),
                        reason: "candidate_requires_second_stage_shared_fact_governance"
                            .to_string(),
                    }),
                },
                MemoryWriteCandidate {
                    candidate_id: "candidate-weak-skill".to_string(),
                    authority: MemoryEvidenceAuthority::UserAsserted,
                    target: MemoryCandidateTarget::ProceduralMemory {
                        name: "runtime_skill__weak_summary".to_string(),
                        topic: "summary_style".to_string(),
                    },
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    content: MemoryCandidateContent::Text {
                        topic: "summary_style".to_string(),
                        body: "Use summaries.".to_string(),
                        keywords: vec!["summary".to_string()],
                    },
                    evidence_refs: vec![evidence_ref.display_citation()],
                    canonical_entities: Vec::new(),
                    semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                        source: MemorySemanticJudgmentSource::LlmGovernance,
                        decision: MemoryCandidateSemanticDecision::Accept,
                        governed_target: Some(MemoryCandidateTarget::ProceduralMemory {
                            name: "runtime_skill__weak_summary".to_string(),
                            topic: "summary_style".to_string(),
                        }),
                        reason: "candidate_requires_second_stage_skill_governance".to_string(),
                    }),
                },
            ],
        })
        .unwrap();
    assert_eq!(write.changed, 0);

    let lifecycle = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(turn.turn_id.clone()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "second_stage_rejected_candidates_are_not_memory_impact".to_string(),
        })
        .unwrap();

    assert!(
        lifecycle.transcript.derived_memory_refs.is_empty(),
        "second-stage rejected candidates must not be reported as accepted memory impact: {:?}",
        lifecycle.transcript.derived_memory_refs
    );
}

#[test]
fn long_term_extraction_records_transcript_derived_ref_for_lifecycle_impact() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .finalize_turn_and_maintain(None, None, finalize_request("我喜欢结构化摘要", "已记录。"))
        .unwrap();
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::RawOwnerOnly,
        })
        .unwrap();
    let turn = &replay.slice.turns[0];
    let message = &turn.input_messages[0];
    let evidence_ref = TranscriptEvidenceRef {
        memory_space_id: runtime.memory_space_id().to_string(),
        channel_id: "llm.gateway".to_string(),
        conversation_id: "conversation-a".to_string(),
        turn_id: turn.turn_id.clone(),
        message_id: Some(message.message_id.clone()),
        subject_id: Some("subject-default".to_string()),
        authority: Some(MemoryEvidenceAuthority::UserAsserted),
    };

    let write = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Preference,
                    privacy: bm_sdk::MemoryPrivacyClass::SharedWithSubject,
                    topic: "summary_style".to_string(),
                    content: "The user likes structured summaries.".to_string(),
                    keywords: vec!["structured".to_string()],
                    source_chat_id: Some("chat-a".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec![evidence_ref.display_citation()],
                    canonical_entities: Vec::new(),
                    evidence_count: None,
                    observed_at: Some(10),
                    last_confirmed_at: None,
                    source_revision: None,
                }],
                deletes: Vec::new(),
                skill_writes: vec![RuntimeSkillWrite {
                    name: "runtime_skill__structured_summary".to_string(),
                    topic: "summary_style".to_string(),
                    title: "Structured summaries".to_string(),
                    summary: "Write concise structured summaries and verify headings.".to_string(),
                    content: "- write a concise structured summary\n- verify headings before final output"
                        .to_string(),
                    citations: vec![evidence_ref.display_citation()],
                    source_chat_id: Some("chat-a".to_string()),
                    observed_at: 10,
                }],
            },
        })
        .unwrap();
    assert_eq!(write.changed, 2);

    let err = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(turn.turn_id.clone()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "review_extraction_source".to_string(),
        })
        .expect_err("facet-backed transcript source redaction must fail closed");

    assert_eq!(err.stage(), "transcript_lifecycle_facet_preflight");
}

#[test]
fn automatic_post_turn_extraction_records_transcript_derived_ref_for_lifecycle_impact() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response(
        r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "fact",
                "source_authority": "user_asserted",
                "topic": "primary_llm",
                "content": "当前主模型是 OpenAI。",
                "keywords": ["OpenAI", "主模型"]
            }
        ]"#,
    );
    let request = finalize_request(
        "当前主模型已经切到 OpenAI",
        "收到，这轮把主模型事实和证据一起写回 shared factual plane。",
    );
    let turn_id = request.turn.turn_id.clone();

    let report = runtime
        .finalize_turn_and_maintain(Some(&mut http), Some(&llm), request)
        .unwrap();
    assert_eq!(report.semantic_governance.accepted_count, 1);
    let entries = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(10)
        .unwrap();
    assert!(
        entries.iter().any(|entry| entry.topic == "primary_llm"
            && entry
                .supporting_citations
                .iter()
                .any(|citation| TranscriptEvidenceRef::parse_display_citation(citation).is_some())),
        "automatic extraction should persist structured transcript citations: {entries:?}"
    );
    let transcript_store = platform.replay_harness().conversation_transcript_store();
    let key = ConversationKey::new(
        runtime.memory_space_id().to_string(),
        "llm.gateway",
        "conversation-a",
    )
    .unwrap();
    let derived_refs = transcript_store
        .list_derived_memory_refs(&key, None)
        .unwrap();
    assert!(
        derived_refs
            .iter()
            .any(|derived| derived.plane == DerivedMemoryPlane::SharedFact
                && derived.source.turn_id == turn_id),
        "automatic extraction should record derived refs: {derived_refs:?}"
    );

    let err = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(turn_id.clone()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "review_automatic_extraction_source".to_string(),
        })
        .expect_err("facet-backed transcript source redaction must fail closed");

    assert_eq!(err.stage(), "transcript_lifecycle_facet_preflight");
}

#[test]
fn soul_candidate_handoff_records_transcript_derived_ref_for_lifecycle_impact() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .finalize_turn_and_maintain(
            None,
            None,
            finalize_request("这段自我理解交给灵魂治理", "我会作为候选交给灵魂治理。"),
        )
        .unwrap();

    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::RawOwnerOnly,
        })
        .unwrap();
    let turn = &replay.slice.turns[0];
    let message = &turn.input_messages[0];
    let evidence_ref = TranscriptEvidenceRef {
        memory_space_id: runtime.memory_space_id().to_string(),
        channel_id: "llm.gateway".to_string(),
        conversation_id: "conversation-a".to_string(),
        turn_id: turn.turn_id.clone(),
        message_id: Some(message.message_id.clone()),
        subject_id: Some("subject-default".to_string()),
        authority: Some(MemoryEvidenceAuthority::UserAsserted),
    };

    let write = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-soul-handoff".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: MemoryCandidateTarget::Soul {
                    surface: "self_understanding".to_string(),
                },
                privacy: MemoryPrivacyClass::SoulPrivate,
                content: MemoryCandidateContent::Text {
                    topic: "self_understanding".to_string(),
                    body: "Soul governance should review this material.".to_string(),
                    keywords: vec!["soul".to_string()],
                },
                evidence_refs: vec![evidence_ref.display_citation()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                    source: MemorySemanticJudgmentSource::LlmGovernance,
                    decision: MemoryCandidateSemanticDecision::HandoffToSoulGovernance,
                    governed_target: Some(MemoryCandidateTarget::Soul {
                        surface: "self_understanding".to_string(),
                    }),
                    reason: "requires_soul_governance".to_string(),
                }),
            }],
        })
        .unwrap();
    assert_eq!(write.changed, 0);

    let lifecycle = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(turn.turn_id.clone()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "review_soul_handoff_source".to_string(),
        })
        .unwrap();

    assert_eq!(lifecycle.transcript.derived_memory_refs.len(), 1);
    assert_eq!(
        lifecycle.transcript.derived_memory_refs[0].plane,
        DerivedMemoryPlane::SoulCandidateHandoff
    );
    assert_eq!(
        lifecycle.transcript.derived_memory_refs[0].source.turn_id,
        turn.turn_id
    );
}

#[test]
fn memory_space_export_redacts_raw_conversation_transcript_by_default() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let finalize = runtime
        .finalize_turn_and_maintain(None, None, finalize_request("导出前需要保护", "已记录。"))
        .unwrap();
    let turn_id = finalize.transcript_commit.unwrap().turn_id;
    let key = ConversationKey::new(
        runtime.memory_space_id().to_string(),
        "llm.gateway".to_string(),
        "conversation-a".to_string(),
    )
    .unwrap();
    platform
        .replay_harness()
        .conversation_transcript_store()
        .append_derived_memory_ref(
            &key,
            &DerivedMemoryRef {
                plane: DerivedMemoryPlane::PrivateGarden,
                store_key: "private_garden:board.self::journal/export-check.md".to_string(),
                subject_id: Some("subject-default".to_string()),
                source: TranscriptEvidenceRef {
                    memory_space_id: runtime.memory_space_id().to_string(),
                    channel_id: "llm.gateway".to_string(),
                    conversation_id: "conversation-a".to_string(),
                    turn_id: turn_id.clone(),
                    message_id: None,
                    subject_id: Some("subject-default".to_string()),
                    authority: Some(MemoryEvidenceAuthority::PrivateGardenInternal),
                },
                created_at: 1_800_000_000,
            },
        )
        .unwrap();
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::RawOwnerOnly,
        })
        .unwrap();
    let assistant_message_id = replay.slice.turns[0]
        .assistant_message
        .as_ref()
        .unwrap()
        .message_id
        .clone();
    runtime
        .record_transcript_attrs(MemoryTranscriptAttrWriteRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            attrs: vec![model_usage_attr(
                key.clone(),
                &turn_id,
                &assistant_message_id,
                HostRefVisibility::HostUi,
            )],
            idempotency_key: Some("export-privacy-attr-write".to_string()),
            dry_run: false,
        })
        .unwrap();

    let redacted = runtime
        .export_memory_space(MemorySpaceExportRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            include_private: false,
        })
        .unwrap();
    assert!(redacted.privacy_redactions > 0);
    assert!(!redacted
        .archive
        .contains_json_namespace("conversation_transcript"));
    assert!(!redacted
        .archive
        .contains_json_namespace("conversation_transcript_alias"));
    assert!(!redacted
        .archive
        .contains_json_namespace("conversation_transcript_derived_ref"));
    assert!(!redacted
        .archive
        .contains_json_namespace("conversation_transcript_attr"));

    let raw = runtime
        .export_memory_space(MemorySpaceExportRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            include_private: true,
        })
        .unwrap();
    assert!(raw
        .archive
        .contains_json_namespace("conversation_transcript"));
    assert!(raw
        .archive
        .contains_json_namespace("conversation_transcript_alias"));
    assert!(raw
        .archive
        .contains_json_namespace("conversation_transcript_derived_ref"));
    assert!(raw
        .archive
        .contains_json_namespace("conversation_transcript_attr"));
}

#[test]
fn private_garden_self_work_records_private_garden_derived_refs_without_raw_content() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response(
        r#"{"writes":[{"path":"journal/qingchuan.md","content":"Keep a private note that the current exchange made the name Qingchuan salient."}]}"#,
    );
    let request = finalize_request("叫我青川，后续称呼要自然一些", "我会记住这个称呼。");
    let turn_id = request.turn.turn_id.clone();

    let report = runtime
        .finalize_turn_and_maintain(Some(&mut http), Some(&llm), request)
        .unwrap();

    assert!(report.private_garden_self_work.executed);
    assert_eq!(report.private_garden_self_work.writes, 1);

    let lifecycle = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(turn_id.clone()),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "review_private_garden_source".to_string(),
        })
        .unwrap();

    let private_ref = lifecycle
        .transcript
        .derived_memory_refs
        .iter()
        .find(|derived| derived.plane == DerivedMemoryPlane::PrivateGarden)
        .expect("private garden self-work should be linked to transcript evidence");
    assert_eq!(
        private_ref.store_key,
        "private_garden:board.self::journal/qingchuan.md"
    );
    assert_eq!(private_ref.source.turn_id, turn_id);
    assert!(private_ref.source.message_id.is_none());
    assert_eq!(
        private_ref.source.authority,
        Some(MemoryEvidenceAuthority::PrivateGardenInternal)
    );
    assert!(!format!("{private_ref:?}").contains("Keep a private note"));
}

#[test]
fn runtime_exposes_manual_transcript_commit_and_export_surface() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let request = finalize_request("只提交 transcript", "已提交。");

    let commit = runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: request.turn,
            host_refs: vec![host_ref()],
        })
        .unwrap();
    assert!(commit.session_commit.committed);
    assert_eq!(commit.key.memory_space_id, runtime.memory_space_id());

    let export = runtime
        .export_transcript(MemoryTranscriptExportRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
        })
        .unwrap();

    assert_eq!(export.slice.view, TranscriptReplayView::Export);
    assert_eq!(export.slice.turns.len(), 1);
    assert_eq!(export.slice.turns[0].host_refs[0].business_ref_id, "T-42");
}

#[test]
fn lifecycle_report_sanitizes_host_refs_for_operator_view() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let request = finalize_request("提交带 host refs 的 transcript", "已提交。");
    let turn_id = request.turn.turn_id.clone();
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: request.turn,
            host_refs: vec![
                host_ref_with_visibility("internal", HostRefVisibility::Internal),
                host_ref_with_visibility("model", HostRefVisibility::ModelContext),
                host_ref_with_visibility("operator", HostRefVisibility::OperatorAudit),
                host_ref_with_visibility("host", HostRefVisibility::HostUi),
                host_ref_with_visibility("export", HostRefVisibility::Export),
            ],
        })
        .unwrap();

    let lifecycle = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: Some(turn_id),
            transition: TranscriptLifecycleTransition::Mask,
            reason: "operator_review_host_refs".to_string(),
        })
        .unwrap();
    let ids = lifecycle
        .transcript
        .affected_host_refs
        .iter()
        .map(|host_ref| host_ref.business_ref_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(ids, vec!["operator", "host", "export"]);
    assert!(lifecycle
        .transcript
        .affected_host_refs
        .iter()
        .all(|host_ref| host_ref.label.is_none()));
    assert_eq!(lifecycle.transcript.redacted_host_refs, 2);
    assert!(lifecycle
        .transcript
        .host_ref_redactions
        .iter()
        .any(|item| item.reason == TranscriptRedactionReason::HostRefVisibility));
    assert!(lifecycle
        .transcript
        .host_ref_redactions
        .iter()
        .any(|item| item.reason == TranscriptRedactionReason::HostRefLabel));
}

#[test]
fn runtime_replay_and_export_transcript_support_cursor_pages() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .finalize_turn_and_maintain(None, None, finalize_request("第一页", "已记录第一页。"))
        .unwrap();
    runtime
        .finalize_turn_and_maintain(None, None, finalize_request("第二页内容", "已记录第二页。"))
        .unwrap();

    let first = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 1,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();
    assert_eq!(first.slice.turns.len(), 1);
    assert!(first.has_more);
    let next_cursor = first.next_cursor.clone().expect("next replay cursor");

    let second = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 1,
            cursor: Some(next_cursor),
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();
    assert_eq!(second.slice.turns.len(), 1);
    assert!(!second.has_more);
    assert!(second.next_cursor.is_none());

    let export = runtime
        .export_transcript(MemoryTranscriptExportRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 1,
            cursor: None,
        })
        .unwrap();
    assert_eq!(export.slice.view, TranscriptReplayView::Export);
    assert!(export.has_more);
    assert!(export.next_cursor.is_some());
}

#[test]
fn manual_transcript_commit_is_idempotent_by_transcript_turn_even_if_session_shadow_was_cleared() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let request = finalize_request("幂等提交", "已提交。");
    let turn = request.turn.clone();
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn: request.turn,
            host_refs: Vec::new(),
        })
        .unwrap();
    platform
        .replay_harness()
        .session_store()
        .clear("chat-a")
        .unwrap();

    let second = runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn,
            host_refs: Vec::new(),
        })
        .unwrap();

    assert!(!second.session_commit.committed);
    assert_eq!(
        second.session_commit.skipped_reason.as_deref(),
        Some("conversation_transcript_turn_already_committed")
    );
    assert_eq!(
        platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        0
    );
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();
    assert_eq!(replay.slice.turns.len(), 1);
}

#[test]
fn manual_transcript_commit_backfills_when_session_shadow_already_has_turn() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let request = finalize_request("旧 session 已有", "回填 transcript。");
    let turn = request.turn.clone();
    let session_store = platform.replay_harness().session_store();
    let legacy_session = commit_canonical_turn_delta(session_store.as_ref(), &turn).unwrap();
    assert!(legacy_session.committed);

    let commit = runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn,
            host_refs: Vec::new(),
        })
        .unwrap();

    assert!(!commit.session_commit.committed);
    assert_eq!(
        commit.session_commit.skipped_reason.as_deref(),
        Some("canonical_turn_delta_already_committed")
    );
    assert!(commit
        .transcript_commit
        .as_ref()
        .is_some_and(|report| report.committed));
    assert_eq!(
        platform
            .replay_harness()
            .session_store()
            .message_count("chat-a")
            .unwrap(),
        2
    );
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();
    assert_eq!(replay.slice.turns.len(), 1);
    assert_eq!(
        replay.slice.turns[0].input_messages[0].content.as_deref(),
        Some("旧 session 已有")
    );
}

#[test]
fn finalize_turn_reports_transcript_backfill_as_committed_when_session_shadow_already_has_turn() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let request = finalize_request("finalize 回填", "回填 transcript。");
    let turn = request.turn.clone();
    let session_store = platform.replay_harness().session_store();
    let legacy_session = commit_canonical_turn_delta(session_store.as_ref(), &turn).unwrap();
    assert!(legacy_session.committed);

    let report = runtime
        .finalize_turn_and_maintain(None, None, request)
        .unwrap();

    assert!(!report.session_commit.committed);
    assert_eq!(
        report.session_commit.skipped_reason.as_deref(),
        Some("canonical_turn_delta_already_committed")
    );
    assert!(report
        .transcript_commit
        .as_ref()
        .is_some_and(|transcript| transcript.committed));
    assert!(report.lifecycle_report.changed);
    assert_eq!(
        report.semantic_governance.skipped_reason.as_deref(),
        Some("maintenance_http_unavailable")
    );

    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();
    assert_eq!(
        replay.slice.turns[0].input_messages[0].content.as_deref(),
        Some("finalize 回填")
    );
}

#[test]
fn runtime_transcript_requests_reject_other_memory_space() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope_and_subject(
        platform,
        profile,
        "llm.gateway",
        "chat-a",
        "subject-default",
    );

    let replay_err = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: "space:other".to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::HostUi,
        })
        .unwrap_err();
    assert!(replay_err.to_string().contains("memory_space_id"));

    let export_err = runtime
        .export_transcript(MemoryTranscriptExportRequest {
            memory_space_id: "space:other".to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
        })
        .unwrap_err();
    assert!(export_err.to_string().contains("memory_space_id"));

    let lifecycle_err = runtime
        .request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
            memory_space_id: "space:other".to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            turn_id: None,
            transition: TranscriptLifecycleTransition::Archive,
            reason: "cross_space_probe".to_string(),
        })
        .unwrap_err();
    assert!(lifecycle_err.to_string().contains("memory_space_id"));
}
