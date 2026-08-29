use std::sync::Mutex;

use bm_core::memory::{
    commit_canonical_turn_delta, CanonicalTurnDelta, ConversationScope, GovernedWriteDecision,
    MemoryEvidenceAuthority, MemoryTurnDeliveryStatus, MemoryTurnProtocol, MemoryTurnSource,
    MemoryWriteAuthority, PostTurnGovernanceExecutionBindingV1,
    PostTurnGovernanceExecutionBlockReasonV1, PostTurnGovernanceIdentityV2,
    PostTurnGovernanceJobRefV2, PostTurnGovernanceJobStatusV2, PostTurnGovernanceJobV3,
    PostTurnGovernancePrivacyAuthorityV1, PostTurnGovernanceReconciliationCursorV1,
    PostTurnGovernanceScopeIndexV3, PostTurnPrivateGardenReport, PrivateGardenAdmissionDecision,
    SessionStore, TranscriptInputMessage, MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS,
};
use bm_core::memory::{SessionMessage, SessionMessageRecord};
use bm_core::Result;

#[derive(Default)]
struct InMemorySessionStore {
    messages: Mutex<Vec<SessionMessage>>,
}

impl SessionStore for InMemorySessionStore {
    fn append(&self, _chat_id: &str, role: &str, content: &str) -> Result<()> {
        self.messages
            .lock()
            .unwrap()
            .push(SessionMessage::synthetic(role, content));
        Ok(())
    }

    fn append_batch(&self, _chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
        self.messages
            .lock()
            .unwrap()
            .extend(messages.iter().cloned());
        Ok(())
    }

    fn load_recent(&self, _chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let messages = self.messages.lock().unwrap();
        Ok(messages
            .iter()
            .rev()
            .take(n)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect())
    }

    fn load_recent_records(&self, _chat_id: &str, n: usize) -> Result<Vec<SessionMessageRecord>> {
        Ok(self
            .load_recent(_chat_id, n)?
            .into_iter()
            .map(SessionMessageRecord::from)
            .collect())
    }

    fn message_count(&self, _chat_id: &str) -> Result<usize> {
        Ok(self.messages.lock().unwrap().len())
    }

    fn clear(&self, _chat_id: &str) -> Result<()> {
        self.messages.lock().unwrap().clear();
        Ok(())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        Ok(vec!["chat-a".to_string()])
    }
}

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_core::memory::IngressKind::User,
        channel: "llm.gateway".to_string(),
        provider: Some("ollama".to_string()),
        protocol: MemoryTurnProtocol::OllamaChat,
        endpoint: Some("/api/chat".to_string()),
        model_alias: Some("qwen".to_string()),
        model_resolved: Some("qwen3.5:0.8b".to_string()),
        request_id: Some("req-1".to_string()),
        client_conversation_hint: Some("ollama-window".to_string()),
    }
}

fn turn_delta(
    turn_id: &str,
    delivery_status: MemoryTurnDeliveryStatus,
    input_messages: Vec<TranscriptInputMessage>,
    assistant_message: Option<&str>,
) -> CanonicalTurnDelta {
    CanonicalTurnDelta {
        turn_id: turn_id.to_string(),
        conversation: ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "chat-a".to_string(),
            conversation_id: Some("ollama-window".to_string()),
        },
        subject: "subject-default".to_string(),
        delivery_status,
        source: turn_source(),
        actor: None,
        input_messages,
        assistant_message: assistant_message.map(TranscriptInputMessage::assistant),
        tool_observations: Vec::new(),
        external_content_used: false,
        candidate_ids: Vec::new(),
    }
}

#[test]
fn delivered_turn_commits_user_and_assistant_messages() {
    let store = InMemorySessionStore::default();

    let report = commit_canonical_turn_delta(
        &store,
        &turn_delta(
            "turn-1",
            MemoryTurnDeliveryStatus::Delivered,
            vec![TranscriptInputMessage::user("叫我青川")],
            Some("你好，青川。"),
        ),
    )
    .expect("commit succeeds");

    assert!(report.attempted);
    assert!(report.committed);
    assert_eq!(report.before_count, 0);
    assert_eq!(report.after_count, 2);
    assert_eq!(report.committed_messages.len(), 2);
    assert_eq!(report.committed_messages[0].role, "user");
    assert_eq!(report.committed_messages[1].role, "assistant");
    assert_eq!(report.committed_messages[0].content_chars, 4);
    assert_eq!(store.message_count("chat-a").unwrap(), 2);
}

#[test]
fn full_history_turn_commits_only_new_user_delta_without_losing_latest_message() {
    let store = InMemorySessionStore::default();
    commit_canonical_turn_delta(
        &store,
        &turn_delta(
            "turn-1",
            MemoryTurnDeliveryStatus::Delivered,
            vec![TranscriptInputMessage::user("我叫银二")],
            Some("我记住了。"),
        ),
    )
    .expect("first commit succeeds");

    let report = commit_canonical_turn_delta(
        &store,
        &turn_delta(
            "turn-2",
            MemoryTurnDeliveryStatus::Delivered,
            vec![
                TranscriptInputMessage::user("我叫银二"),
                TranscriptInputMessage::assistant("我记住了。"),
                TranscriptInputMessage::user("我喜欢冷萃"),
            ],
            Some("冷萃也记下了。"),
        ),
    )
    .expect("second commit succeeds");

    let recent = store.load_recent("chat-a", 10).expect("recent");
    assert_eq!(report.before_count, 2);
    assert_eq!(report.after_count, 4);
    assert_eq!(report.committed_messages.len(), 2);
    assert_eq!(report.committed_messages[0].role, "user");
    assert_eq!(
        report.committed_messages[0].authority,
        MemoryEvidenceAuthority::UserAsserted
    );
    assert_eq!(report.committed_messages[1].role, "assistant");
    assert_eq!(
        report.committed_messages[1].authority,
        MemoryEvidenceAuthority::AssistantUtterance
    );
    assert_eq!(recent[2].content, "我喜欢冷萃");
    assert!(!recent
        .iter()
        .any(|message| { message.content == "我叫银二\n我喜欢冷萃" }));
}

#[test]
fn assistant_self_description_is_committed_as_low_authority_evidence_not_identity_truth() {
    let store = InMemorySessionStore::default();

    let report = commit_canonical_turn_delta(
        &store,
        &turn_delta(
            "turn-1",
            MemoryTurnDeliveryStatus::Delivered,
            vec![TranscriptInputMessage::user("你叫什么？")],
            Some("我是 Beetle Memory 的记忆助手。"),
        ),
    )
    .expect("commit succeeds");

    assert_eq!(report.committed_messages.len(), 2);
    assert_eq!(
        report.committed_messages[0].authority,
        MemoryEvidenceAuthority::UserAsserted
    );
    assert_eq!(
        report.committed_messages[1].authority,
        MemoryEvidenceAuthority::AssistantUtterance
    );
    assert_ne!(
        report.committed_messages[1].authority,
        MemoryEvidenceAuthority::SoulGovernance
    );
}

#[test]
fn incomplete_stream_does_not_commit_partial_assistant() {
    let store = InMemorySessionStore::default();

    let report = commit_canonical_turn_delta(
        &store,
        &turn_delta(
            "turn-1",
            MemoryTurnDeliveryStatus::IncompleteStream,
            vec![TranscriptInputMessage::user("叫我青川")],
            Some("你好，青"),
        ),
    )
    .expect("commit succeeds");

    assert!(report.attempted);
    assert!(!report.committed);
    assert_eq!(report.after_count, 0);
    assert_eq!(report.committed_messages.len(), 0);
    assert_eq!(store.message_count("chat-a").unwrap(), 0);
}

#[test]
fn user_only_turn_commits_user_without_assistant() {
    let store = InMemorySessionStore::default();

    let report = commit_canonical_turn_delta(
        &store,
        &turn_delta(
            "turn-1",
            MemoryTurnDeliveryStatus::UserOnly,
            vec![TranscriptInputMessage::user("叫我青川")],
            None,
        ),
    )
    .expect("commit succeeds");

    assert!(report.committed);
    assert_eq!(report.after_count, 1);
    assert_eq!(report.committed_messages.len(), 1);
    assert_eq!(report.committed_messages[0].role, "user");
}

#[test]
fn private_garden_freeform_report_is_not_governed_write_decision() {
    let report = PostTurnPrivateGardenReport {
        attempted: true,
        executed: true,
        authority: MemoryWriteAuthority::LlmPrivateGardenFreeform,
        admission: PrivateGardenAdmissionDecision::Applied,
        writes: 1,
        moves: 0,
        deletes: 0,
        manifest: Vec::new(),
        skipped_reason: None,
    };

    assert_eq!(
        report.authority,
        MemoryWriteAuthority::LlmPrivateGardenFreeform
    );
    assert_eq!(report.admission, PrivateGardenAdmissionDecision::Applied);
    assert_ne!(
        format!("{:?}", report.admission),
        format!("{:?}", GovernedWriteDecision::Accepted)
    );
}

fn governance_job(conversation_id: &str) -> PostTurnGovernanceJobV3 {
    PostTurnGovernanceJobV3::pending(
        PostTurnGovernanceIdentityV2::new(
            "space:owner-default",
            "agent:governance",
            "llm.gateway",
            "chat-a",
            conversation_id,
            "turn-1",
        )
        .expect("identity"),
        1,
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "sha256:2222222222222222222222222222222222222222222222222222222222222222",
        PostTurnGovernanceExecutionBindingV1::Unbound,
        PostTurnGovernancePrivacyAuthorityV1 {
            policy_schema_version: 1,
            exact_policy_digest:
                "sha256:3333333333333333333333333333333333333333333333333333333333333333"
                    .to_string(),
        },
        vec!["candidate-1".to_string()],
        0,
        5,
        1_800_000_000,
    )
    .expect("pending job")
}

#[test]
fn unbound_job_is_durably_blocked_without_inventing_a_binding_revision() {
    let job = governance_job("conversation-unbound");

    assert_eq!(
        job.execution_binding,
        PostTurnGovernanceExecutionBindingV1::Unbound
    );
    assert_eq!(
        job.status,
        PostTurnGovernanceJobStatusV2::BlockedConfiguration
    );
    let block = job
        .execution_block_authority
        .as_ref()
        .expect("unbound job must carry durable block authority");
    assert_eq!(
        block.typed_block_reason,
        PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable
    );
    assert_eq!(block.binding_id, None);
    assert_eq!(block.binding_revision, None);
    assert_eq!(block.credential_ref_safe_id, None);
    assert_eq!(job.next_attempt_at, None);
    job.validate().expect("unbound blocked job is canonical");
}

#[test]
fn governance_job_identity_includes_conversation_and_contains_no_raw_turn() {
    let first = governance_job("conversation-a");
    let second = governance_job("conversation-b");
    assert_ne!(first.job_id, second.job_id);
    assert_eq!(
        first.scope_index_key, second.scope_index_key,
        "one mounted Entry runtime needs one deterministic due index across conversations"
    );
    assert!(first.job_id.starts_with("ptgj2:"));
    assert_eq!(first.job_id.len(), 62);

    let encoded = serde_json::to_string(&first).expect("job json");
    assert!(!encoded.contains("叫我青川"));
    assert!(!encoded.contains("inputMessages"));
    assert!(!encoded.contains("assistantMessage"));
    assert!(!encoded.contains("turn\":"));
}

#[test]
fn governance_job_validator_rejects_lease_and_terminal_state_drift() {
    let mut leased_without_authority = governance_job("conversation-a");
    leased_without_authority.status = PostTurnGovernanceJobStatusV2::Leased;
    leased_without_authority.lease_owner = Some("worker-a".to_string());
    leased_without_authority.lease_until = Some(1_800_000_030);
    assert!(leased_without_authority.validate().is_err());

    let mut success_without_receipt = governance_job("conversation-a");
    success_without_receipt.status = PostTurnGovernanceJobStatusV2::Succeeded;
    success_without_receipt.next_attempt_at = None;
    success_without_receipt.terminal_at = Some(1_800_000_010);
    assert!(success_without_receipt.validate().is_err());
}

#[test]
fn governance_scope_index_is_exact_bounded_and_active_only() {
    let job = governance_job("conversation-a");
    let mut index = PostTurnGovernanceScopeIndexV3::empty(&job.identity, 1_800_000_000);
    index
        .active_jobs
        .push(PostTurnGovernanceJobRefV2::from_job(&job));
    assert!(index.validate().is_ok());
    index
        .set_reconciliation_cursor(PostTurnGovernanceReconciliationCursorV1 {
            conversation_id: "conversation-a".to_string(),
            sequence: 1,
            turn_id: "turn-a".to_string(),
        })
        .expect("conversation a cursor");
    index
        .set_reconciliation_cursor(PostTurnGovernanceReconciliationCursorV1 {
            conversation_id: "conversation-b".to_string(),
            sequence: 1,
            turn_id: "turn-b".to_string(),
        })
        .expect("conversation b cursor");
    assert_eq!(index.reconciliation_cursors.len(), 2);

    index
        .active_jobs
        .push(PostTurnGovernanceJobRefV2::from_job(&job));
    assert!(index.validate().is_err());

    index.active_jobs.clear();
    for sequence in 0..=MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS {
        let mut reference = PostTurnGovernanceJobRefV2::from_job(&job);
        reference.job_id = format!("ptgj2:{sequence:056x}");
        reference.job_key.clone_from(&reference.job_id);
        index.active_jobs.push(reference);
    }
    assert!(index.validate().is_err());
}
