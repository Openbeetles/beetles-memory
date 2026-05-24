use serde::{Deserialize, Serialize};

use crate::bus::IngressKind;
use crate::error::Result;

use super::{SessionMessage, SessionMessageRecord, SessionStore, MAX_SESSION_ENTRIES};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEvidenceAuthority {
    UserAsserted,
    AssistantUtterance,
    AssistantSelfClaim,
    RuntimeObservation,
    WorldObservation,
    ProgramMemoryCanonical,
    ArchiveEvidence,
    SubjectProjection,
    SoulGovernance,
    PrivateGardenInternal,
    OperatorDiagnostic,
    ExternalContent,
    LegacyTranscript,
}

impl MemoryEvidenceAuthority {
    pub fn for_role(role: &str) -> Self {
        if role.eq_ignore_ascii_case("user") {
            Self::UserAsserted
        } else if role.eq_ignore_ascii_case("assistant") {
            Self::AssistantUtterance
        } else {
            Self::LegacyTranscript
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::UserAsserted => "user_asserted",
            Self::AssistantUtterance => "assistant_utterance",
            Self::AssistantSelfClaim => "assistant_self_claim",
            Self::RuntimeObservation => "runtime_observation",
            Self::WorldObservation => "world_observation",
            Self::ProgramMemoryCanonical => "program_memory_canonical",
            Self::ArchiveEvidence => "archive_evidence",
            Self::SubjectProjection => "subject_projection",
            Self::SoulGovernance => "soul_governance",
            Self::PrivateGardenInternal => "private_garden_internal",
            Self::OperatorDiagnostic => "operator_diagnostic",
            Self::ExternalContent => "external_content",
            Self::LegacyTranscript => "legacy_transcript",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptInputMessage {
    pub role: String,
    pub content: String,
    pub authority: MemoryEvidenceAuthority,
}

impl TranscriptInputMessage {
    pub fn new(
        role: impl Into<String>,
        content: impl Into<String>,
        authority: MemoryEvidenceAuthority,
    ) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            authority,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content, MemoryEvidenceAuthority::UserAsserted)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(
            "assistant",
            content,
            MemoryEvidenceAuthority::AssistantUtterance,
        )
    }

    fn is_role(&self, role: &str) -> bool {
        self.role.eq_ignore_ascii_case(role)
    }

    fn into_session_message(self) -> SessionMessage {
        SessionMessage {
            role: self.role,
            content: self.content,
        }
    }
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
    pub input_messages: Vec<TranscriptInputMessage>,
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
    pub authority: MemoryEvidenceAuthority,
    pub content_chars: usize,
    pub content_bytes: usize,
}

pub fn commit_session_turn(
    session_store: &dyn SessionStore,
    chat_id: &str,
    input: SessionTurnCommitInput,
) -> Result<SessionTurnCommitReport> {
    let before_count = session_store.message_count(chat_id)?;
    let recent_records = session_store.load_recent_records(chat_id, MAX_SESSION_ENTRIES)?;
    let mut messages = Vec::new();
    match input.delivery_status {
        MemoryTurnDeliveryStatus::Delivered => {
            push_user_delta(&mut messages, &recent_records, &input);
            if let Some(assistant_content) = input.assistant_content.as_deref() {
                push_if_not_empty(
                    &mut messages,
                    "assistant",
                    assistant_content,
                    MemoryEvidenceAuthority::AssistantUtterance,
                );
            }
        }
        MemoryTurnDeliveryStatus::UserOnly
        | MemoryTurnDeliveryStatus::UpstreamFailed
        | MemoryTurnDeliveryStatus::Cancelled => {
            push_user_delta(&mut messages, &recent_records, &input);
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
            authority: message.authority,
            content_chars: message.content.chars().count(),
            content_bytes: message.content.len(),
        })
        .collect::<Vec<_>>();
    let session_messages = messages
        .into_iter()
        .map(TranscriptInputMessage::into_session_message)
        .collect::<Vec<_>>();
    session_store.append_batch(chat_id, &session_messages)?;
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

fn push_user_delta(
    messages: &mut Vec<TranscriptInputMessage>,
    recent_records: &[SessionMessageRecord],
    input: &SessionTurnCommitInput,
) {
    let deltas = if input.input_messages.is_empty() {
        if input.user_content.trim().is_empty() {
            Vec::new()
        } else {
            vec![TranscriptInputMessage::user(input.user_content.clone())]
        }
    } else {
        canonical_user_delta(recent_records, &input.input_messages)
    };
    for message in deltas {
        if !message.content.trim().is_empty() {
            messages.push(message);
        }
    }
}

pub fn canonical_user_delta(
    recent_records: &[SessionMessageRecord],
    input_messages: &[TranscriptInputMessage],
) -> Vec<TranscriptInputMessage> {
    let user_messages = input_messages
        .iter()
        .filter(|message| message.is_role("user"))
        .filter(|message| !message.content.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if user_messages.is_empty() {
        return Vec::new();
    }

    let existing_users = recent_records
        .iter()
        .filter(|record| record.role.eq_ignore_ascii_case("user"))
        .map(|record| record.content.trim())
        .collect::<Vec<_>>();
    let max_overlap = existing_users.len().min(user_messages.len());
    let mut overlap = 0;
    for size in 1..=max_overlap {
        let input_prefix_matches_existing_tail = user_messages[..size]
            .iter()
            .map(|message| message.content.trim())
            .eq(existing_users[existing_users.len() - size..]
                .iter()
                .copied());
        if input_prefix_matches_existing_tail {
            overlap = size;
        }
    }

    user_messages[overlap..].to_vec()
}

fn push_if_not_empty(
    messages: &mut Vec<TranscriptInputMessage>,
    role: &str,
    content: &str,
    authority: MemoryEvidenceAuthority,
) {
    if content.trim().is_empty() {
        return;
    }
    messages.push(TranscriptInputMessage::new(role, content, authority));
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
