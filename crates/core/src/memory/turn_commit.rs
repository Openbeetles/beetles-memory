use serde::{Deserialize, Serialize};

use crate::bus::IngressKind;
use crate::error::Result;

use super::{SessionMessage, SessionStore};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTurnDeliveryStatus {
    Delivered,
    UserOnly,
    UpstreamFailed,
    Cancelled,
    IncompleteStream,
    RejectedByPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTurnProtocol {
    OpenAiChat,
    OpenAiResponses,
    OllamaChat,
    OllamaGenerate,
    Native,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryTurnSource {
    pub ingress: IngressKind,
    pub channel: String,
    pub provider: Option<String>,
    pub protocol: MemoryTurnProtocol,
    pub endpoint: Option<String>,
    pub model_alias: Option<String>,
    pub model_resolved: Option<String>,
    pub request_id: Option<String>,
    pub client_conversation_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTurnCommitInput {
    pub delivery_status: MemoryTurnDeliveryStatus,
    pub source: MemoryTurnSource,
    pub user_content: String,
    pub assistant_content: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTurnCommitReport {
    pub attempted: bool,
    pub committed: bool,
    pub chat_id: String,
    pub before_count: usize,
    pub after_count: usize,
    pub committed_messages: Vec<CommittedSessionMessage>,
    pub skipped_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedSessionMessage {
    pub role: String,
    pub content_chars: usize,
    pub content_bytes: usize,
}

pub fn commit_session_turn(
    session_store: &dyn SessionStore,
    chat_id: &str,
    input: SessionTurnCommitInput,
) -> Result<SessionTurnCommitReport> {
    let before_count = session_store.message_count(chat_id)?;
    let mut messages = Vec::new();
    match input.delivery_status {
        MemoryTurnDeliveryStatus::Delivered => {
            push_if_not_empty(&mut messages, "user", &input.user_content);
            if let Some(assistant_content) = input.assistant_content.as_deref() {
                push_if_not_empty(&mut messages, "assistant", assistant_content);
            }
        }
        MemoryTurnDeliveryStatus::UserOnly
        | MemoryTurnDeliveryStatus::UpstreamFailed
        | MemoryTurnDeliveryStatus::Cancelled => {
            push_if_not_empty(&mut messages, "user", &input.user_content);
        }
        MemoryTurnDeliveryStatus::IncompleteStream | MemoryTurnDeliveryStatus::RejectedByPolicy => {
        }
    }

    if messages.is_empty() {
        return Ok(SessionTurnCommitReport {
            attempted: true,
            committed: false,
            chat_id: chat_id.to_string(),
            before_count,
            after_count: session_store.message_count(chat_id)?,
            committed_messages: Vec::new(),
            skipped_reason: Some(skipped_reason(input.delivery_status).to_string()),
        });
    }

    let committed_messages = messages
        .iter()
        .map(|message| CommittedSessionMessage {
            role: message.role.clone(),
            content_chars: message.content.chars().count(),
            content_bytes: message.content.len(),
        })
        .collect::<Vec<_>>();
    session_store.append_batch(chat_id, &messages)?;
    let after_count = session_store.message_count(chat_id)?;
    Ok(SessionTurnCommitReport {
        attempted: true,
        committed: true,
        chat_id: chat_id.to_string(),
        before_count,
        after_count,
        committed_messages,
        skipped_reason: None,
    })
}

fn push_if_not_empty(messages: &mut Vec<SessionMessage>, role: &str, content: &str) {
    if content.trim().is_empty() {
        return;
    }
    messages.push(SessionMessage {
        role: role.to_string(),
        content: content.to_string(),
    });
}

fn skipped_reason(status: MemoryTurnDeliveryStatus) -> &'static str {
    match status {
        MemoryTurnDeliveryStatus::Delivered => "empty_delivered_turn",
        MemoryTurnDeliveryStatus::UserOnly => "empty_user_turn",
        MemoryTurnDeliveryStatus::UpstreamFailed => "empty_failed_turn",
        MemoryTurnDeliveryStatus::Cancelled => "empty_cancelled_turn",
        MemoryTurnDeliveryStatus::IncompleteStream => "incomplete_stream",
        MemoryTurnDeliveryStatus::RejectedByPolicy => "rejected_by_policy",
    }
}
