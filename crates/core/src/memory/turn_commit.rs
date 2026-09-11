use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::bus::IngressKind;
use crate::error::Result;

use super::transcript::TranscriptTurnRecordAttachments;
use super::{
    default_session_speaker_for_role, ActorAttribution, CanonicalTurnAppendIntent,
    CanonicalTurnTranscriptCommitReport, ConversationKey, ConversationTranscriptStore,
    HostOpaqueRef, PostTurnLearningEvidenceV1, SessionMessage, SessionMessageRecord, SessionStore,
    SubjectId, TranscriptAppendIntent, TranscriptCommitReport, TranscriptConversationAlias,
    TranscriptTurnRecord, MAX_SESSION_ENTRIES,
};

const CANONICAL_TURN_LEARNING_DIGEST_DOMAIN: &str = "canonical_turn_learning_digest_v1";

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
    /// Inputs belonging to this turn only, never a full conversation history.
    /// Normalize protocol history with `protocol_window_user_delta` before intake.
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalTurnTranscriptCommitOptions {
    pub host_refs: Vec<HostOpaqueRef>,
    pub learning_evidence: Option<PostTurnLearningEvidenceV1>,
    pub conversation_alias: Option<TranscriptConversationAlias>,
    pub now_secs: u64,
}

pub fn canonical_turn_learning_digest(delta: &CanonicalTurnDelta) -> Result<String> {
    let encoded = serde_json::to_vec(delta).map_err(|error| {
        crate::error::Error::config("canonical_turn_learning_digest", error.to_string())
    })?;
    let mut hasher = Sha256::new();
    hasher.update((CANONICAL_TURN_LEARNING_DIGEST_DOMAIN.len() as u64).to_be_bytes());
    hasher.update(CANONICAL_TURN_LEARNING_DIGEST_DOMAIN.as_bytes());
    hasher.update((encoded.len() as u64).to_be_bytes());
    hasher.update(encoded);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub fn commit_canonical_turn_delta_with_transcript(
    session_store: &dyn SessionStore,
    transcript_store: &dyn ConversationTranscriptStore,
    memory_space_id: &str,
    delta: &CanonicalTurnDelta,
    options: CanonicalTurnTranscriptCommitOptions,
) -> Result<CanonicalTurnTranscriptCommitReport> {
    let CanonicalTurnTranscriptCommitOptions {
        host_refs,
        learning_evidence,
        conversation_alias,
        now_secs,
    } = options;
    let key = ConversationKey::from_delta(memory_space_id, delta)?;
    if let Some(evidence) = learning_evidence.as_ref() {
        let conversation_id = delta
            .conversation
            .conversation_id
            .as_deref()
            .unwrap_or(delta.conversation.chat_id.as_str());
        if !evidence.validate_contract()
            || evidence.memory_space_id != memory_space_id
            || evidence.mounted_subject_id != delta.subject
            || evidence.conversation_id != conversation_id
            || evidence.turn_id != delta.turn_id
            || evidence.canonical_turn_digest != canonical_turn_learning_digest(delta)?
        {
            return Err(crate::error::Error::config(
                "post_turn_learning_evidence",
                "learning evidence must bind the exact canonical turn owner and digest",
            ));
        }
    }
    if let Some(alias) = conversation_alias.as_ref() {
        alias.validate_for_transcript_owner(&key, &delta.subject)?;
        if alias.chat_id != delta.conversation.chat_id {
            return Err(crate::error::Error::config(
                "turn_transcript_commit",
                "conversation_alias_chat_id_must_match_turn_delta",
            ));
        }
    }
    let recent_records =
        session_store.load_recent_records(&delta.conversation.chat_id, usize::MAX)?;
    let session_before = recent_records
        .iter()
        .map(SessionMessageRecord::as_message)
        .collect::<Vec<_>>();
    let before_count = session_before.len();
    if let Some(existing) = transcript_store.get_turn(&key, &delta.subject, &delta.turn_id)? {
        existing.validate_canonical_intake()?;
        if existing.canonical_turn_digest != canonical_turn_learning_digest(delta)?
            || existing.host_refs != host_refs
            || existing.learning_evidence != learning_evidence
        {
            return Err(crate::error::Error::conflict(
                "canonical_turn_intake_conflict",
                "same turn cannot be committed with different canonical input, host references, or learning evidence",
            ));
        }
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
    let prepared = prepare_canonical_turn_delta(before_count, delta);
    let committed_inputs = prepared.messages.clone();
    let (session_append, mut session_commit) =
        plan_prepared_session_messages(&key, delta, prepared, now_secs);
    if !session_commit.committed {
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
        TranscriptTurnRecordAttachments {
            host_refs,
            learning_evidence,
        },
        now_secs,
    )?;
    let intent = TranscriptAppendIntent {
        record,
        conversation_alias,
    };
    intent.validate()?;
    let atomic = CanonicalTurnAppendIntent {
        session_chat_id: delta.conversation.chat_id.clone(),
        session_before,
        session_append,
        transcript: intent,
    };
    atomic.validate()?;
    let transcript_commit = transcript_store.append_canonical_turn_intent(&atomic)?;
    if !transcript_commit.committed {
        session_commit.committed = false;
        session_commit.after_count = session_commit.before_count;
        session_commit.committed_messages.clear();
        session_commit.skipped_reason = transcript_commit.skipped_reason.clone();
    }
    Ok(CanonicalTurnTranscriptCommitReport {
        session_commit,
        transcript_commit: Some(transcript_commit),
    })
}

fn prepare_canonical_turn_delta(
    before_count: usize,
    delta: &CanonicalTurnDelta,
) -> PreparedCanonicalTurnCommit {
    let user_delta = delta
        .input_messages
        .iter()
        .filter(|message| message.is_role("user") && !message.content.trim().is_empty())
        .cloned();
    let assistant_message = delta
        .assistant_message
        .as_ref()
        .filter(|message| message.is_role("assistant") && !message.content.trim().is_empty())
        .cloned();
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
    PreparedCanonicalTurnCommit {
        before_count,
        messages,
        skipped_reason,
    }
}

fn plan_prepared_session_messages(
    key: &ConversationKey,
    delta: &CanonicalTurnDelta,
    prepared: PreparedCanonicalTurnCommit,
    now: u64,
) -> (Vec<SessionMessage>, SessionTurnCommitReport) {
    let chat_id = delta.conversation.chat_id.as_str();
    if prepared.messages.is_empty() {
        return (
            Vec::new(),
            SessionTurnCommitReport {
                attempted: true,
                committed: false,
                chat_id: chat_id.to_string(),
                before_count: prepared.before_count,
                after_count: prepared.before_count,
                committed_messages: Vec::new(),
                skipped_reason: prepared.skipped_reason,
            },
        );
    }

    let prepared_messages = prepared
        .messages
        .into_iter()
        .enumerate()
        .map(|(index, message)| {
            let authority = message.authority;
            let mut hasher = Sha256::new();
            for field in [
                "bm.canonical-turn.message.v1",
                key.memory_space_id.as_str(),
                key.channel_id.as_str(),
                key.conversation_id.as_str(),
                delta.subject.as_str(),
                delta.turn_id.as_str(),
            ] {
                hasher.update((field.len() as u64).to_be_bytes());
                hasher.update(field.as_bytes());
            }
            hasher.update((index as u64).to_be_bytes());
            let message_id = format!("msg_{:x}", hasher.finalize());
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
    let after_count = prepared
        .before_count
        .saturating_add(session_messages.len())
        .min(MAX_SESSION_ENTRIES);
    (
        session_messages,
        SessionTurnCommitReport {
            attempted: true,
            committed: true,
            chat_id: chat_id.to_string(),
            before_count: prepared.before_count,
            after_count,
            committed_messages,
            skipped_reason: None,
        },
    )
}

/// Resolve the unanswered user-message group of a protocol history window.
///
/// Assistant entries (including empty tool-call entries) delimit answered history.
/// With no assistant boundary, every user entry belongs to the current group.
/// This is stateless normalization, not retry detection; canonical turn identity
/// and the immutable Transcript exclusively own idempotency.
pub fn protocol_window_user_delta(
    input_messages: &[TranscriptInputMessage],
) -> Vec<TranscriptInputMessage> {
    let start = input_messages
        .iter()
        .rposition(|message| message.is_role("assistant"))
        .map_or(0, |index| index + 1);
    input_messages[start..]
        .iter()
        .filter(|message| message.is_role("user") && !message.content.trim().is_empty())
        .cloned()
        .collect()
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
