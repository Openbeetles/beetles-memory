mod support;

use bm_core::memory::commit_canonical_turn_delta;
use bm_core::platform::Platform as _;
use bm_sdk::{
    CanonicalTurnDelta, ConversationScope, HostOpaqueRef, HostRefRelation, HostRefVisibility,
    MemorySpaceExportRequest, MemoryTranscriptCommitRequest, MemoryTranscriptExportRequest,
    MemoryTranscriptLifecycleRequest, MemoryTranscriptReplayRequest, MemoryTurnDeliveryStatus,
    MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource, PressureLevel, ProfileId,
    RuntimeLifecycleModeInput, TranscriptInputMessage, TranscriptLifecycleTransition,
    TranscriptReplayView,
};

use support::{empty_store_platform, test_runtime_with_scope_and_subject};

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
    HostOpaqueRef {
        host_kind: "generic-host".to_string(),
        business_ref_type: "ticket".to_string(),
        business_ref_id: "T-42".to_string(),
        relation: HostRefRelation::Related,
        visibility: HostRefVisibility::HostUi,
        label: Some("opaque ticket".to_string()),
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
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();

    assert_eq!(replay.slice.turns.len(), 1);
    assert_eq!(replay.slice.turns[0].subject, "subject-default");
    assert_eq!(
        replay.slice.turns[0].input_messages[0].content.as_deref(),
        Some("记住我是青川")
    );
    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 2);
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
            view: TranscriptReplayView::HostUi,
        })
        .unwrap();

    assert_eq!(replay.slice.turns[0].input_messages[0].content, None);
    assert!(replay.slice.audit.redacted_messages > 0);
    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 2);
}

#[test]
fn memory_space_export_redacts_raw_conversation_transcript_by_default() {
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
        .finalize_turn_and_maintain(None, None, finalize_request("导出前需要保护", "已记录。"))
        .unwrap();

    let redacted = runtime
        .export_memory_space(MemorySpaceExportRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            include_private: false,
        })
        .unwrap();
    assert!(redacted.privacy_redactions > 0);
    assert!(!redacted
        .snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript"));

    let raw = runtime
        .export_memory_space(MemorySpaceExportRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            include_private: true,
        })
        .unwrap();
    assert!(raw
        .snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript"));
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
        })
        .unwrap();

    assert_eq!(export.slice.view, TranscriptReplayView::Export);
    assert_eq!(export.slice.turns.len(), 1);
    assert_eq!(export.slice.turns[0].host_refs[0].business_ref_id, "T-42");
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
    platform.session_store().clear("chat-a").unwrap();

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
    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 0);
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
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
    let session_store = platform.session_store();
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
    assert_eq!(platform.session_store().message_count("chat-a").unwrap(), 2);
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
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
