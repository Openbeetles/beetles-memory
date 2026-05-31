use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

use super::{
    synthesize_session_message_id, CanonicalTurnDelta, CommittedSessionMessage,
    MemoryEvidenceAuthority, MemoryTurnDeliveryStatus, MemoryTurnSource, SessionTurnCommitReport,
    SubjectId, ToolObservationDigest, TranscriptInputMessage,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationKey {
    pub memory_space_id: String,
    pub channel_id: String,
    pub conversation_id: String,
}

impl ConversationKey {
    pub fn new(
        memory_space_id: impl Into<String>,
        channel_id: impl Into<String>,
        conversation_id: impl Into<String>,
    ) -> Result<Self> {
        let key = Self {
            memory_space_id: memory_space_id.into().trim().to_string(),
            channel_id: channel_id.into().trim().to_string(),
            conversation_id: conversation_id.into().trim().to_string(),
        };
        key.validate()?;
        Ok(key)
    }

    pub fn from_delta(
        memory_space_id: impl Into<String>,
        delta: &CanonicalTurnDelta,
    ) -> Result<Self> {
        let conversation_id = delta
            .conversation
            .conversation_id
            .as_deref()
            .unwrap_or(delta.conversation.chat_id.as_str());
        Self::new(
            memory_space_id,
            &delta.conversation.channel,
            conversation_id,
        )
    }

    pub fn storage_key(&self) -> String {
        format!(
            "{}__{}__{}",
            encode_labeled_key_component("ms", &self.memory_space_id),
            encode_labeled_key_component("ch", &self.channel_id),
            encode_labeled_key_component("cv", &self.conversation_id)
        )
    }

    pub fn turn_storage_key(&self, turn_id: &str) -> String {
        format!(
            "{}__{}",
            self.storage_key(),
            encode_labeled_key_component("turn", turn_id)
        )
    }

    pub fn turn_storage_key_prefix(&self) -> String {
        format!("{}__turn", self.storage_key())
    }

    fn validate(&self) -> Result<()> {
        if self.memory_space_id.is_empty() {
            return Err(Error::config(
                "conversation_key",
                "memory_space_id must not be empty",
            ));
        }
        if self.channel_id.is_empty() {
            return Err(Error::config(
                "conversation_key",
                "channel_id must not be empty",
            ));
        }
        if self.conversation_id.is_empty() {
            return Err(Error::config(
                "conversation_key",
                "conversation_id must not be empty",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorAttribution {
    pub speaker_id: String,
    pub speaker_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mounted_subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggered_by: Option<String>,
}

impl ActorAttribution {
    pub fn for_subject(subject_id: impl Into<String>) -> Self {
        let subject_id = subject_id.into();
        Self {
            speaker_id: subject_id.clone(),
            speaker_kind: "subject".to_string(),
            subject_id: Some(subject_id.clone()),
            actor_subject_id: Some(subject_id.clone()),
            mounted_subject_id: Some(subject_id),
            agent_id: None,
            triggered_by: None,
        }
    }

    pub fn from_message(message: &TranscriptInputMessage, subject_id: &str) -> Self {
        Self {
            speaker_id: message.speaker_id.clone(),
            speaker_kind: message.speaker_kind.clone(),
            subject_id: Some(subject_id.to_string()),
            actor_subject_id: Some(subject_id.to_string()),
            mounted_subject_id: Some(subject_id.to_string()),
            agent_id: None,
            triggered_by: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRefRelation {
    Origin,
    EvidenceFor,
    Trigger,
    Related,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRefVisibility {
    Internal,
    HostUi,
    ModelContext,
    OperatorAudit,
    Export,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostOpaqueRef {
    pub host_kind: String,
    pub business_ref_type: String,
    pub business_ref_id: String,
    pub relation: HostRefRelation,
    pub visibility: HostRefVisibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptLifecycleState {
    Active,
    Archived,
    Masked,
    RawDeleted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRedactionState {
    RawAvailable,
    Masked,
    RawDeleted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptReplayView {
    RawOwnerOnly,
    ModelContext,
    HostUi,
    OperatorAudit,
    Export,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptLifecycleTransition {
    Archive,
    Restore,
    Mask,
    DeleteRaw,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptMessageRecord {
    pub message_id: String,
    pub turn_id: String,
    pub sequence: u64,
    pub role: String,
    pub content: String,
    pub authority: MemoryEvidenceAuthority,
    pub observed_at: u64,
    pub created_at: u64,
    pub actor: ActorAttribution,
}

impl TranscriptMessageRecord {
    pub fn from_input(
        key: &ConversationKey,
        turn_id: &str,
        sequence: u64,
        message: &TranscriptInputMessage,
        fallback_observed_at: u64,
        created_at: u64,
        subject_id: &str,
    ) -> Self {
        let occurrence = u32::try_from(sequence).unwrap_or(u32::MAX);
        let observed_at = if message.observed_at == 0 {
            fallback_observed_at
        } else {
            message.observed_at
        };
        Self {
            message_id: synthesize_session_message_id(
                &key.storage_key(),
                &message.role,
                &message.content,
                occurrence,
            ),
            turn_id: turn_id.to_string(),
            sequence,
            role: message.role.clone(),
            content: message.content.clone(),
            authority: message.authority,
            observed_at,
            created_at: created_at.max(observed_at),
            actor: ActorAttribution::from_message(message, subject_id),
        }
    }

    pub fn from_committed_input(
        turn_id: &str,
        sequence: u64,
        message: &TranscriptInputMessage,
        committed: &CommittedSessionMessage,
        subject_id: &str,
    ) -> Self {
        Self {
            message_id: committed.message_id.clone(),
            turn_id: turn_id.to_string(),
            sequence,
            role: committed.role.clone(),
            content: message.content.clone(),
            authority: committed.authority,
            observed_at: committed.observed_at,
            created_at: committed.created_at,
            actor: ActorAttribution {
                speaker_id: committed.speaker_id.clone(),
                speaker_kind: committed.speaker_kind.clone(),
                subject_id: Some(subject_id.to_string()),
                actor_subject_id: Some(subject_id.to_string()),
                mounted_subject_id: Some(subject_id.to_string()),
                agent_id: None,
                triggered_by: None,
            },
        }
    }

    fn redact_raw(&mut self) {
        self.content.clear();
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptTurnRecord {
    pub key: ConversationKey,
    pub turn_id: String,
    pub sequence: u64,
    pub delivery_status: MemoryTurnDeliveryStatus,
    pub source: MemoryTurnSource,
    pub subject: SubjectId,
    pub actor: ActorAttribution,
    pub input_messages: Vec<TranscriptMessageRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_message: Option<TranscriptMessageRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_observations: Vec<ToolObservationDigest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_refs: Vec<HostOpaqueRef>,
    pub external_content_used: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_ids: Vec<String>,
    pub lifecycle_state: TranscriptLifecycleState,
    pub redaction_state: TranscriptRedactionState,
    pub created_at: u64,
    pub updated_at: u64,
}

impl TranscriptTurnRecord {
    pub fn from_delta(
        key: &ConversationKey,
        sequence: u64,
        delta: &CanonicalTurnDelta,
        host_refs: Vec<HostOpaqueRef>,
        now_secs: u64,
    ) -> Result<Self> {
        validate_key_matches_delta(key, delta)?;
        let input_messages = delta
            .input_messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                TranscriptMessageRecord::from_input(
                    key,
                    &delta.turn_id,
                    u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1),
                    message,
                    now_secs,
                    now_secs,
                    &delta.subject,
                )
            })
            .collect::<Vec<_>>();
        let assistant_message = delta.assistant_message.as_ref().map(|message| {
            TranscriptMessageRecord::from_input(
                key,
                &delta.turn_id,
                u64::try_from(input_messages.len())
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
                message,
                now_secs,
                now_secs,
                &delta.subject,
            )
        });
        Ok(Self {
            key: key.clone(),
            turn_id: delta.turn_id.clone(),
            sequence,
            delivery_status: delta.delivery_status,
            source: delta.source.clone(),
            subject: delta.subject.clone(),
            actor: ActorAttribution::for_subject(&delta.subject),
            input_messages,
            assistant_message,
            tool_observations: delta.tool_observations.clone(),
            host_refs,
            external_content_used: delta.external_content_used,
            candidate_ids: delta.candidate_ids.clone(),
            lifecycle_state: TranscriptLifecycleState::Active,
            redaction_state: TranscriptRedactionState::RawAvailable,
            created_at: now_secs,
            updated_at: now_secs,
        })
    }

    pub fn from_committed_messages(
        key: &ConversationKey,
        sequence: u64,
        delta: &CanonicalTurnDelta,
        committed_inputs: &[TranscriptInputMessage],
        committed_messages: &[CommittedSessionMessage],
        host_refs: Vec<HostOpaqueRef>,
        now_secs: u64,
    ) -> Result<Self> {
        validate_key_matches_delta(key, delta)?;
        let mut input_messages = Vec::new();
        let mut assistant_message = None;
        for (index, (message, committed)) in committed_inputs
            .iter()
            .zip(committed_messages.iter())
            .enumerate()
        {
            let record = TranscriptMessageRecord::from_committed_input(
                &delta.turn_id,
                u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1),
                message,
                committed,
                &delta.subject,
            );
            if record.role.eq_ignore_ascii_case("assistant") {
                assistant_message = Some(record);
            } else {
                input_messages.push(record);
            }
        }
        Ok(Self {
            key: key.clone(),
            turn_id: delta.turn_id.clone(),
            sequence,
            delivery_status: delta.delivery_status,
            source: delta.source.clone(),
            subject: delta.subject.clone(),
            actor: ActorAttribution::for_subject(&delta.subject),
            input_messages,
            assistant_message,
            tool_observations: delta.tool_observations.clone(),
            host_refs,
            external_content_used: delta.external_content_used,
            candidate_ids: delta.candidate_ids.clone(),
            lifecycle_state: TranscriptLifecycleState::Active,
            redaction_state: TranscriptRedactionState::RawAvailable,
            created_at: now_secs,
            updated_at: now_secs,
        })
    }

    pub fn apply_lifecycle_transition(
        &mut self,
        transition: TranscriptLifecycleTransition,
        updated_at: u64,
    ) {
        match transition {
            TranscriptLifecycleTransition::Archive => {
                self.lifecycle_state = TranscriptLifecycleState::Archived;
            }
            TranscriptLifecycleTransition::Restore => {
                if self.redaction_state == TranscriptRedactionState::RawAvailable {
                    self.lifecycle_state = TranscriptLifecycleState::Active;
                }
            }
            TranscriptLifecycleTransition::Mask => {
                self.lifecycle_state = TranscriptLifecycleState::Masked;
                self.redaction_state = TranscriptRedactionState::Masked;
            }
            TranscriptLifecycleTransition::DeleteRaw => {
                self.lifecycle_state = TranscriptLifecycleState::RawDeleted;
                self.redaction_state = TranscriptRedactionState::RawDeleted;
                for message in &mut self.input_messages {
                    message.redact_raw();
                }
                if let Some(message) = &mut self.assistant_message {
                    message.redact_raw();
                }
            }
        }
        self.updated_at = updated_at;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptCommitReport {
    pub key: ConversationKey,
    pub turn_id: String,
    pub sequence: u64,
    pub committed: bool,
    pub before_count: usize,
    pub after_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalTurnTranscriptCommitReport {
    pub session_commit: SessionTurnCommitReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_commit: Option<TranscriptCommitReport>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptLifecycleRequest {
    pub key: ConversationKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub transition: TranscriptLifecycleTransition,
    pub reason: String,
    pub requested_by: String,
    pub requested_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptLifecycleReport {
    pub key: ConversationKey,
    pub transition: TranscriptLifecycleTransition,
    pub affected_turns: usize,
    pub reason: String,
    pub requested_by: String,
    pub requested_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedTranscriptMessage {
    pub message_id: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub authority: MemoryEvidenceAuthority,
    pub actor: ActorAttribution,
    pub observed_at: u64,
    pub redacted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedTranscriptTurn {
    pub turn_id: String,
    pub sequence: u64,
    pub subject: SubjectId,
    pub actor: ActorAttribution,
    pub delivery_status: MemoryTurnDeliveryStatus,
    pub input_messages: Vec<RedactedTranscriptMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_message: Option<RedactedTranscriptMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_refs: Vec<HostOpaqueRef>,
    pub lifecycle_state: TranscriptLifecycleState,
    pub redaction_state: TranscriptRedactionState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptReplayAudit {
    pub view: TranscriptReplayView,
    pub source_turns: usize,
    pub returned_turns: usize,
    pub redacted_messages: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedTranscriptSlice {
    pub key: ConversationKey,
    pub view: TranscriptReplayView,
    pub turns: Vec<RedactedTranscriptTurn>,
    pub audit: TranscriptReplayAudit,
}

impl RedactedTranscriptSlice {
    pub fn from_records(
        key: ConversationKey,
        view: TranscriptReplayView,
        records: &[TranscriptTurnRecord],
    ) -> Self {
        let mut redacted_messages = 0usize;
        let turns = records
            .iter()
            .map(|record| {
                let input_messages = record
                    .input_messages
                    .iter()
                    .map(|message| {
                        redact_message_for_view(record, message, view, &mut redacted_messages)
                    })
                    .collect();
                let assistant_message = record.assistant_message.as_ref().map(|message| {
                    redact_message_for_view(record, message, view, &mut redacted_messages)
                });
                RedactedTranscriptTurn {
                    turn_id: record.turn_id.clone(),
                    sequence: record.sequence,
                    subject: record.subject.clone(),
                    actor: record.actor.clone(),
                    delivery_status: record.delivery_status,
                    input_messages,
                    assistant_message,
                    host_refs: record.host_refs.clone(),
                    lifecycle_state: record.lifecycle_state,
                    redaction_state: record.redaction_state,
                }
            })
            .collect::<Vec<_>>();
        Self {
            key,
            view,
            audit: TranscriptReplayAudit {
                view,
                source_turns: records.len(),
                returned_turns: turns.len(),
                redacted_messages,
            },
            turns,
        }
    }
}

pub trait ConversationTranscriptStore: Send + Sync {
    fn append_turn(&self, record: &TranscriptTurnRecord) -> Result<TranscriptCommitReport>;
    fn get_turn(
        &self,
        key: &ConversationKey,
        turn_id: &str,
    ) -> Result<Option<TranscriptTurnRecord>>;
    fn list_turns(&self, key: &ConversationKey, limit: usize) -> Result<Vec<TranscriptTurnRecord>>;
    fn apply_lifecycle_request(
        &self,
        request: &TranscriptLifecycleRequest,
    ) -> Result<TranscriptLifecycleReport>;

    fn redacted_replay(
        &self,
        key: &ConversationKey,
        limit: usize,
        view: TranscriptReplayView,
    ) -> Result<RedactedTranscriptSlice> {
        let records = self.list_turns(key, limit)?;
        Ok(RedactedTranscriptSlice::from_records(
            key.clone(),
            view,
            &records,
        ))
    }
}

fn redact_message_for_view(
    record: &TranscriptTurnRecord,
    message: &TranscriptMessageRecord,
    view: TranscriptReplayView,
    redacted_messages: &mut usize,
) -> RedactedTranscriptMessage {
    let redacted = should_redact(record, message, view);
    if redacted {
        *redacted_messages = redacted_messages.saturating_add(1);
    }
    RedactedTranscriptMessage {
        message_id: message.message_id.clone(),
        role: message.role.clone(),
        content: if redacted {
            None
        } else {
            Some(message.content.clone())
        },
        authority: message.authority,
        actor: message.actor.clone(),
        observed_at: message.observed_at,
        redacted,
    }
}

fn should_redact(
    record: &TranscriptTurnRecord,
    message: &TranscriptMessageRecord,
    view: TranscriptReplayView,
) -> bool {
    if matches!(
        record.redaction_state,
        TranscriptRedactionState::Masked | TranscriptRedactionState::RawDeleted
    ) || matches!(
        record.lifecycle_state,
        TranscriptLifecycleState::Masked | TranscriptLifecycleState::RawDeleted
    ) {
        return true;
    }
    if view == TranscriptReplayView::RawOwnerOnly {
        return false;
    }
    matches!(
        message.authority,
        MemoryEvidenceAuthority::PrivateGardenInternal
            | MemoryEvidenceAuthority::SoulGovernance
            | MemoryEvidenceAuthority::OperatorDiagnostic
    )
}

fn validate_key_matches_delta(key: &ConversationKey, delta: &CanonicalTurnDelta) -> Result<()> {
    if key.channel_id != delta.conversation.channel.trim() {
        return Err(Error::config(
            "transcript_turn_record",
            "conversation key channel does not match delta",
        ));
    }
    let delta_conversation = delta
        .conversation
        .conversation_id
        .as_deref()
        .unwrap_or(delta.conversation.chat_id.as_str())
        .trim();
    if key.conversation_id != delta_conversation {
        return Err(Error::config(
            "transcript_turn_record",
            "conversation key conversation_id does not match delta",
        ));
    }
    Ok(())
}

fn encode_key_component(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "_".to_string();
    }
    let mut out = String::with_capacity(trimmed.len());
    for byte in trimmed.bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

fn encode_labeled_key_component(label: &str, raw: &str) -> String {
    let encoded = encode_key_component(raw);
    format!("{label}{}:{encoded}", encoded.len())
}
