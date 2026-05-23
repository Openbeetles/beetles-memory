use std::sync::Mutex;

use bm_core::memory::{
    commit_session_turn, GovernedWriteDecision, MemoryTurnDeliveryStatus, MemoryTurnProtocol,
    MemoryTurnSource, MemoryWriteAuthority, PostTurnPrivateGardenReport,
    PrivateGardenAdmissionDecision, SessionStore, SessionTurnCommitInput,
};
use bm_core::memory::{SessionMessage, SessionMessageRecord};
use bm_core::Result;

#[derive(Default)]
struct InMemorySessionStore {
    messages: Mutex<Vec<SessionMessage>>,
}

impl SessionStore for InMemorySessionStore {
    fn append(&self, _chat_id: &str, role: &str, content: &str) -> Result<()> {
        self.messages.lock().unwrap().push(SessionMessage {
            role: role.to_string(),
            content: content.to_string(),
        });
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
            .enumerate()
            .map(|(idx, message)| SessionMessageRecord {
                message_id: format!("msg-{idx}"),
                role: message.role,
                content: message.content,
            })
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

#[test]
fn delivered_turn_commits_user_and_assistant_messages() {
    let store = InMemorySessionStore::default();

    let report = commit_session_turn(
        &store,
        "chat-a",
        SessionTurnCommitInput {
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            user_content: "叫我青川".to_string(),
            assistant_content: Some("你好，青川。".to_string()),
        },
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
fn incomplete_stream_does_not_commit_partial_assistant() {
    let store = InMemorySessionStore::default();

    let report = commit_session_turn(
        &store,
        "chat-a",
        SessionTurnCommitInput {
            delivery_status: MemoryTurnDeliveryStatus::IncompleteStream,
            source: turn_source(),
            user_content: "叫我青川".to_string(),
            assistant_content: Some("你好，青".to_string()),
        },
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

    let report = commit_session_turn(
        &store,
        "chat-a",
        SessionTurnCommitInput {
            delivery_status: MemoryTurnDeliveryStatus::UserOnly,
            source: turn_source(),
            user_content: "叫我青川".to_string(),
            assistant_content: None,
        },
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
