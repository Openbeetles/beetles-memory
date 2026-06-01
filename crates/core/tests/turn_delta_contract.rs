use bm_core::error::Result;
use bm_core::memory::{
    commit_canonical_turn_delta, CanonicalTurnDelta, ConversationScope, MemoryEvidenceAuthority,
    MemoryTurnDeliveryStatus, MemoryTurnProtocol, MemoryTurnSource, SessionMessage, SessionStore,
    ToolObservationDigest, TranscriptInputMessage,
};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Default)]
struct SessionStoreStub {
    messages: Mutex<BTreeMap<String, Vec<SessionMessage>>>,
}

impl SessionStore for SessionStoreStub {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        self.messages
            .lock()
            .unwrap()
            .entry(chat_id.to_string())
            .or_default()
            .push(SessionMessage::synthetic(role, content));
        Ok(())
    }

    fn append_batch(&self, chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
        self.messages
            .lock()
            .unwrap()
            .entry(chat_id.to_string())
            .or_default()
            .extend_from_slice(messages);
        Ok(())
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let mut messages = self
            .messages
            .lock()
            .unwrap()
            .get(chat_id)
            .cloned()
            .unwrap_or_default();
        if messages.len() > n {
            messages = messages[messages.len() - n..].to_vec();
        }
        Ok(messages)
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.messages.lock().unwrap().remove(chat_id);
        Ok(())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        Ok(self.messages.lock().unwrap().keys().cloned().collect())
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
        model_resolved: Some("qwen3".to_string()),
        request_id: Some("req-1".to_string()),
        client_conversation_hint: Some("window-a".to_string()),
    }
}

#[test]
fn canonical_turn_delta_is_idempotent_and_does_not_recommit_full_history() {
    let store = SessionStoreStub::default();
    store.append("chat-a", "user", "你好").unwrap();
    store.append("chat-a", "assistant", "你好，我在。").unwrap();

    let delta = CanonicalTurnDelta {
        turn_id: "turn-0002".to_string(),
        conversation: ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "chat-a".to_string(),
            conversation_id: Some("ollama-window-a".to_string()),
        },
        subject: "subject-qingchuan".to_string(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: turn_source(),
        actor: None,
        input_messages: vec![
            TranscriptInputMessage::user("你好"),
            TranscriptInputMessage::assistant("你好，我在。"),
            TranscriptInputMessage::user("叫我青川"),
        ],
        assistant_message: Some(TranscriptInputMessage::new(
            "assistant",
            "你好，青川。",
            MemoryEvidenceAuthority::AssistantUtterance,
        )),
        tool_observations: vec![ToolObservationDigest {
            observation_id: "tool-1".to_string(),
            tool_name: "web_fetch".to_string(),
            summary: "external page was consulted".to_string(),
            external_content: true,
        }],
        external_content_used: true,
        candidate_ids: vec!["candidate-1".to_string()],
    };

    assert_eq!(delta.subject, "subject-qingchuan");
    assert!(delta.external_content_used);
    assert!(delta.tool_observations[0].external_content);
    assert_eq!(delta.candidate_ids, vec!["candidate-1"]);

    let first = commit_canonical_turn_delta(&store, &delta).unwrap();
    assert!(first.committed);
    assert_eq!(first.after_count, 4);
    assert_eq!(first.committed_messages.len(), 2);
    assert_eq!(first.committed_messages[0].role, "user");
    assert_eq!(first.committed_messages[1].role, "assistant");

    let second = commit_canonical_turn_delta(&store, &delta).unwrap();
    assert!(!second.committed);
    assert_eq!(second.after_count, 4);
    assert_eq!(
        second.skipped_reason.as_deref(),
        Some("canonical_turn_delta_already_committed")
    );
}

#[test]
fn canonical_turn_delta_persists_message_identity_time_and_speaker_metadata() {
    let store = SessionStoreStub::default();
    let delta = CanonicalTurnDelta {
        turn_id: "turn-speaker".to_string(),
        conversation: ConversationScope {
            channel: "llm.gateway".to_string(),
            chat_id: "chat-speaker".to_string(),
            conversation_id: Some("ollama-window-speaker".to_string()),
        },
        subject: "subject-qingchuan".to_string(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: turn_source(),
        actor: None,
        input_messages: vec![TranscriptInputMessage::user("Human asks")
            .with_observed_at(1_800_000_001)
            .with_speaker("owner-human", "human")],
        assistant_message: Some(
            TranscriptInputMessage::assistant("Specialist answers")
                .with_observed_at(1_800_000_002)
                .with_speaker("planner-agent", "llm_agent"),
        ),
        tool_observations: vec![],
        external_content_used: false,
        candidate_ids: vec![],
    };

    let report = commit_canonical_turn_delta(&store, &delta).unwrap();
    assert!(report.committed);

    let messages = store.load_recent("chat-speaker", 10).expect("load recent");
    assert_eq!(messages.len(), 2);
    assert!(messages[0].message_id.starts_with("msg_"));
    assert_ne!(messages[0].message_id, messages[1].message_id);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[0].observed_at, 1_800_000_001);
    assert!(messages[0].created_at >= messages[0].observed_at);
    assert_eq!(messages[0].speaker_id, "owner-human");
    assert_eq!(messages[0].speaker_kind, "human");
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[1].observed_at, 1_800_000_002);
    assert!(messages[1].created_at >= messages[1].observed_at);
    assert_eq!(messages[1].speaker_id, "planner-agent");
    assert_eq!(messages[1].speaker_kind, "llm_agent");
}

#[test]
fn session_message_rejects_old_role_content_only_shape() {
    let legacy_json = r#"{"role":"user","content":"legacy"}"#;
    let parsed = serde_json::from_str::<SessionMessage>(legacy_json);
    assert!(parsed.is_err());
}
