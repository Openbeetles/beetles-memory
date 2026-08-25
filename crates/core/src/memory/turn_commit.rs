use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::bus::IngressKind;
use crate::error::Result;

use super::{
    default_session_speaker_for_role, synthesize_session_message_id, ActorAttribution,
    CanonicalTurnTranscriptCommitReport, ConversationKey, ConversationTranscriptStore,
    HostOpaqueRef, SessionMessage, SessionMessageRecord, SessionStore, SubjectId,
    TranscriptAppendIntent, TranscriptCommitReport, TranscriptConversationAlias,
    TranscriptTurnRecord, MAX_SESSION_ENTRIES,
};

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
    ModelInferred,
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
            Self::ModelInferred => "model_inferred",
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
    pub observed_at: u64,
    pub speaker_id: String,
    pub speaker_kind: String,
}

impl TranscriptInputMessage {
    pub fn new(
        role: impl Into<String>,
        content: impl Into<String>,
        authority: MemoryEvidenceAuthority,
    ) -> Self {
        let role = role.into();
        let (speaker_id, speaker_kind) = default_session_speaker_for_role(&role);
        Self {
            role,
            content: content.into(),
            authority,
            observed_at: 0,
            speaker_id,
            speaker_kind,
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

    pub fn with_observed_at(mut self, observed_at: u64) -> Self {
        self.observed_at = observed_at;
        self
    }

    pub fn with_speaker(
        mut self,
        speaker_id: impl Into<String>,
        speaker_kind: impl Into<String>,
    ) -> Self {
        self.speaker_id = speaker_id.into();
        self.speaker_kind = speaker_kind.into();
        self
    }

    fn into_session_message(
        self,
        message_id: String,
        fallback_observed_at: u64,
        created_at: u64,
    ) -> SessionMessage {
        let observed_at = if self.observed_at == 0 {
            fallback_observed_at
        } else {
            self.observed_at
        };
        SessionMessage::new(
            message_id,
            self.role,
            self.content,
            observed_at,
            created_at.max(observed_at),
            self.speaker_id,
            self.speaker_kind,
        )
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
pub struct ConversationScope {
    pub channel: String,
    pub chat_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolObservationDigest {
    pub observation_id: String,
    pub tool_name: String,
    pub summary: String,
    #[serde(default)]
    pub external_content: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalTurnDelta {
    pub turn_id: String,
    pub conversation: ConversationScope,
    pub subject: SubjectId,
    pub delivery_status: MemoryTurnDeliveryStatus,
    pub source: MemoryTurnSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<ActorAttribution>,
    #[serde(default)]
    pub input_messages: Vec<TranscriptInputMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_message: Option<TranscriptInputMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_observations: Vec<ToolObservationDigest>,
    #[serde(default)]
    pub external_content_used: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreparedCanonicalTurnCommit {
    before_count: usize,
    messages: Vec<TranscriptInputMessage>,
    skipped_reason: Option<String>,
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
    pub message_id: String,
    pub role: String,
    pub authority: MemoryEvidenceAuthority,
    pub content_chars: usize,
    pub content_bytes: usize,
    pub observed_at: u64,
    pub created_at: u64,
    pub speaker_id: String,
    pub speaker_kind: String,
}

pub fn commit_canonical_turn_delta(
    session_store: &dyn SessionStore,
    delta: &CanonicalTurnDelta,
) -> Result<SessionTurnCommitReport> {
    let chat_id = delta.conversation.chat_id.as_str();
    let prepared = prepare_canonical_turn_delta_commit(session_store, delta)?;
    if prepared.messages.is_empty() {
        return Ok(SessionTurnCommitReport {
            attempted: true,
            committed: false,
            chat_id: chat_id.to_string(),
            before_count: prepared.before_count,
            after_count: prepared.before_count,
            committed_messages: Vec::new(),
            skipped_reason: prepared.skipped_reason,
        });
    }
    commit_prepared_session_messages(session_store, chat_id, prepared)
}

pub fn commit_canonical_turn_delta_with_transcript(
    session_store: &dyn SessionStore,
    transcript_store: &dyn ConversationTranscriptStore,
    memory_space_id: &str,
    delta: &CanonicalTurnDelta,
    host_refs: Vec<HostOpaqueRef>,
    conversation_alias: Option<TranscriptConversationAlias>,
    now_secs: u64,
) -> Result<CanonicalTurnTranscriptCommitReport> {
    let key = ConversationKey::from_delta(memory_space_id, delta)?;
    if let Some(alias) = conversation_alias.as_ref() {
        alias.validate_for_transcript_owner(&key, &delta.subject)?;
        if alias.chat_id != delta.conversation.chat_id {
            return Err(crate::error::Error::config(
                "turn_transcript_commit",
                "conversation_alias_chat_id_must_match_turn_delta",
            ));
        }
    }
    let before_count = session_store.message_count(&delta.conversation.chat_id)?;
    if let Some(existing) = transcript_store.get_turn(&key, &delta.subject, &delta.turn_id)? {
        let session_commit = SessionTurnCommitReport {
            attempted: true,
            committed: false,
            chat_id: delta.conversation.chat_id.clone(),
            before_count,
            after_count: before_count,
            committed_messages: Vec::new(),
            skipped_reason: Some("conversation_transcript_turn_already_committed".to_string()),
        };
        let transcript_count = transcript_store.turn_count(&key, &delta.subject)?;
        return Ok(CanonicalTurnTranscriptCommitReport {
            session_commit,
            transcript_commit: Some(TranscriptCommitReport {
                key,
                turn_id: delta.turn_id.clone(),
                sequence: existing.sequence,
                committed: false,
                before_count: transcript_count,
                after_count: transcript_count,
                skipped_reason: Some("conversation_transcript_turn_already_committed".to_string()),
            }),
        });
    }
    let prepared = prepare_canonical_turn_delta_commit(session_store, delta)?;
    let committed_inputs = prepared.messages.clone();
    let session_commit = if prepared.messages.is_empty() {
        SessionTurnCommitReport {
            attempted: true,
            committed: false,
            chat_id: delta.conversation.chat_id.clone(),
            before_count: prepared.before_count,
            after_count: prepared.before_count,
            committed_messages: Vec::new(),
            skipped_reason: prepared.skipped_reason,
        }
    } else {
        commit_prepared_session_messages(session_store, &delta.conversation.chat_id, prepared)?
    };
    if !session_commit.committed {
        if session_commit
            .skipped_reason
            .as_deref()
            .is_some_and(|reason| reason == "canonical_turn_delta_already_committed")
        {
            let backfill = committed_transcript_backfill_from_session_shadow(session_store, delta)?;
            if let Some((backfill_inputs, backfill_committed)) = backfill {
                let record = TranscriptTurnRecord::from_committed_messages(
                    &key,
                    0,
                    delta,
                    &backfill_inputs,
                    &backfill_committed,
                    host_refs,
                    now_secs,
                )?;
                let intent = TranscriptAppendIntent {
                    record,
                    conversation_alias: conversation_alias.clone(),
                };
                intent.validate()?;
                let transcript_commit = transcript_store.append_turn_intent(&intent)?;
                return Ok(CanonicalTurnTranscriptCommitReport {
                    session_commit,
                    transcript_commit: Some(transcript_commit),
                });
            }
        }
        return Ok(CanonicalTurnTranscriptCommitReport {
            session_commit,
            transcript_commit: None,
        });
    }
    if committed_inputs.len() != session_commit.committed_messages.len() {
        return Err(crate::error::Error::config(
            "turn_transcript_commit",
            "session commit and transcript commit message count diverged",
        ));
    }
    let record = TranscriptTurnRecord::from_committed_messages(
        &key,
        0,
        delta,
        &committed_inputs,
        &session_commit.committed_messages,
        host_refs,
        now_secs,
    )?;
    let intent = TranscriptAppendIntent {
        record,
        conversation_alias,
    };
    intent.validate()?;
    let transcript_commit = transcript_store.append_turn_intent(&intent)?;
    Ok(CanonicalTurnTranscriptCommitReport {
        session_commit,
        transcript_commit: Some(transcript_commit),
    })
}

fn prepare_canonical_turn_delta_commit(
    session_store: &dyn SessionStore,
    delta: &CanonicalTurnDelta,
) -> Result<PreparedCanonicalTurnCommit> {
    let chat_id = delta.conversation.chat_id.as_str();
    let before_count = session_store.message_count(chat_id)?;
    let recent_records = session_store.load_recent_records(chat_id, MAX_SESSION_ENTRIES)?;
    let user_delta = canonical_user_delta(&recent_records, &delta.input_messages);
    let assistant_message = delta
        .assistant_message
        .as_ref()
        .filter(|message| message.is_role("assistant"))
        .filter(|message| !message.content.trim().is_empty())
        .cloned();
    let assistant_already_committed = assistant_message.as_ref().is_some_and(|message| {
        recent_records
            .iter()
            .rev()
            .find(|record| record.role.eq_ignore_ascii_case("assistant"))
            .is_some_and(|record| record.content.trim() == message.content.trim())
    });
    if user_delta.is_empty() && assistant_already_committed {
        return Ok(PreparedCanonicalTurnCommit {
            before_count,
            messages: Vec::new(),
            skipped_reason: Some("canonical_turn_delta_already_committed".to_string()),
        });
    }
    let mut messages = Vec::new();
    match delta.delivery_status {
        MemoryTurnDeliveryStatus::Delivered => {
            messages.extend(user_delta);
            if let Some(message) = assistant_message {
                messages.push(message);
            }
        }
        MemoryTurnDeliveryStatus::UserOnly
        | MemoryTurnDeliveryStatus::UpstreamFailed
        | MemoryTurnDeliveryStatus::Cancelled => {
            messages.extend(user_delta);
        }
        MemoryTurnDeliveryStatus::IncompleteStream | MemoryTurnDeliveryStatus::RejectedByPolicy => {
        }
    }
    let skipped_reason = if messages.is_empty() {
        Some(skipped_reason(delta.delivery_status).to_string())
    } else {
        None
    };
    Ok(PreparedCanonicalTurnCommit {
        before_count,
        messages,
        skipped_reason,
    })
}

fn committed_transcript_backfill_from_session_shadow(
    session_store: &dyn SessionStore,
    delta: &CanonicalTurnDelta,
) -> Result<Option<(Vec<TranscriptInputMessage>, Vec<CommittedSessionMessage>)>> {
    let recent_records =
        session_store.load_recent_records(&delta.conversation.chat_id, MAX_SESSION_ENTRIES)?;
    let mut inputs = Vec::new();
    let mut committed = Vec::new();

    if matches!(
        delta.delivery_status,
        MemoryTurnDeliveryStatus::Delivered
            | MemoryTurnDeliveryStatus::UserOnly
            | MemoryTurnDeliveryStatus::UpstreamFailed
            | MemoryTurnDeliveryStatus::Cancelled
    ) {
        if let Some(user_message) = delta
            .input_messages
            .iter()
            .rev()
            .find(|message| message.is_role("user") && !message.content.trim().is_empty())
            .cloned()
        {
            if let Some(record) = matching_recent_session_record(&recent_records, &user_message) {
                let committed_message =
                    committed_message_from_session_record(record, &user_message);
                inputs.push(user_message);
                committed.push(committed_message);
            }
        }
    }

    if delta.delivery_status == MemoryTurnDeliveryStatus::Delivered {
        if let Some(assistant_message) = delta
            .assistant_message
            .as_ref()
            .filter(|message| message.is_role("assistant"))
            .filter(|message| !message.content.trim().is_empty())
            .cloned()
        {
            if let Some(record) =
                matching_recent_session_record(&recent_records, &assistant_message)
            {
                let committed_message =
                    committed_message_from_session_record(record, &assistant_message);
                inputs.push(assistant_message);
                committed.push(committed_message);
            }
        }
    }

    if inputs.is_empty() {
        Ok(None)
    } else {
        Ok(Some((inputs, committed)))
    }
}

fn matching_recent_session_record<'a>(
    recent_records: &'a [SessionMessageRecord],
    message: &TranscriptInputMessage,
) -> Option<&'a SessionMessageRecord> {
    recent_records.iter().rev().find(|record| {
        record.role.eq_ignore_ascii_case(&message.role)
            && record.content.trim() == message.content.trim()
    })
}

fn committed_message_from_session_record(
    record: &SessionMessageRecord,
    message: &TranscriptInputMessage,
) -> CommittedSessionMessage {
    CommittedSessionMessage {
        message_id: record.message_id.clone(),
        role: record.role.clone(),
        authority: message.authority,
        content_chars: record.content.chars().count(),
        content_bytes: record.content.len(),
        observed_at: record.observed_at,
        created_at: record.created_at,
        speaker_id: record.speaker_id.clone(),
        speaker_kind: record.speaker_kind.clone(),
    }
}

fn commit_prepared_session_messages(
    session_store: &dyn SessionStore,
    chat_id: &str,
    prepared: PreparedCanonicalTurnCommit,
) -> Result<SessionTurnCommitReport> {
    if prepared.messages.is_empty() {
        return Ok(SessionTurnCommitReport {
            attempted: true,
            committed: false,
            chat_id: chat_id.to_string(),
            before_count: prepared.before_count,
            after_count: session_store.message_count(chat_id)?,
            committed_messages: Vec::new(),
            skipped_reason: prepared.skipped_reason,
        });
    }

    let now = current_unix_secs();
    let prepared_messages = prepared
        .messages
        .into_iter()
        .enumerate()
        .map(|(index, message)| {
            let authority = message.authority;
            let occurrence = u32::try_from(
                prepared
                    .before_count
                    .saturating_add(index)
                    .saturating_add(1),
            )
            .unwrap_or(u32::MAX);
            let message_id = synthesize_session_message_id(
                chat_id,
                message.role.as_str(),
                message.content.as_str(),
                occurrence,
            );
            let session_message = message.into_session_message(message_id, now, now);
            (session_message, authority)
        })
        .collect::<Vec<_>>();
    let committed_messages = prepared_messages
        .iter()
        .map(|(message, authority)| CommittedSessionMessage {
            message_id: message.message_id.clone(),
            role: message.role.clone(),
            authority: *authority,
            content_chars: message.content.chars().count(),
            content_bytes: message.content.len(),
            observed_at: message.observed_at,
            created_at: message.created_at,
            speaker_id: message.speaker_id.clone(),
            speaker_kind: message.speaker_kind.clone(),
        })
        .collect::<Vec<_>>();
    let session_messages = prepared_messages
        .into_iter()
        .map(|(message, _)| message)
        .collect::<Vec<_>>();
    session_store.append_batch(chat_id, &session_messages)?;
    let after_count = session_store.message_count(chat_id)?;
    Ok(SessionTurnCommitReport {
        attempted: true,
        committed: true,
        chat_id: chat_id.to_string(),
        before_count: prepared.before_count,
        after_count,
        committed_messages,
        skipped_reason: None,
    })
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

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
