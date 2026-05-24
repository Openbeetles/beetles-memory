use bm_core::error::Result;
use bm_core::memory::{
    commit_canonical_turn_delta, CanonicalTurnDelta, ConversationScope, MemoryEvidenceAuthority,
    MemoryTurnDeliveryStatus, MemoryTurnProtocol, MemoryTurnSource, SessionMessage, SessionStore,
    TranscriptInputMessage,
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
            .push(SessionMessage {
                role: role.to_string(),
                content: content.to_string(),
            });
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
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: turn_source(),
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
    };

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
