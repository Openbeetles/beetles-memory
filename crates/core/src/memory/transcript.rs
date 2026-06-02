use std::collections::{HashMap, HashSet};

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
pub struct TranscriptConversationAlias {
    pub memory_space_id: String,
    pub channel_id: String,
    pub chat_id: String,
    pub conversation_id: String,
    pub updated_at: u64,
}

impl TranscriptConversationAlias {
    pub fn new(
        memory_space_id: impl Into<String>,
        channel_id: impl Into<String>,
        chat_id: impl Into<String>,
        conversation_id: impl Into<String>,
        updated_at: u64,
    ) -> Result<Self> {
        let alias = Self {
            memory_space_id: memory_space_id.into().trim().to_string(),
            channel_id: channel_id.into().trim().to_string(),
            chat_id: chat_id.into().trim().to_string(),
            conversation_id: conversation_id.into().trim().to_string(),
            updated_at,
        };
        alias.validate()?;
        Ok(alias)
    }

    pub fn storage_key(&self) -> String {
        Self::storage_key_for(&self.memory_space_id, &self.channel_id, &self.chat_id)
    }

    pub fn storage_key_for(memory_space_id: &str, channel_id: &str, chat_id: &str) -> String {
        format!(
            "{}__{}__{}",
            encode_labeled_key_component("ms", memory_space_id),
            encode_labeled_key_component("ch", channel_id),
            encode_labeled_key_component("chat", chat_id)
        )
    }

    fn validate(&self) -> Result<()> {
        ConversationKey::new(
            self.memory_space_id.clone(),
            self.channel_id.clone(),
            self.conversation_id.clone(),
        )?;
        if self.chat_id.trim().is_empty() {
            return Err(Error::config(
                "conversation_transcript_alias",
                "chat_id must not be empty",
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

    pub fn normalized_for_subject(mut self, subject_id: &str) -> Self {
        if self.speaker_id.trim().is_empty() {
            self.speaker_id = subject_id.to_string();
        }
        if self.speaker_kind.trim().is_empty() {
            self.speaker_kind = "subject".to_string();
        }
        if self.subject_id.is_none() {
            self.subject_id = Some(subject_id.to_string());
        }
        if self.actor_subject_id.is_none() {
            self.actor_subject_id = Some(subject_id.to_string());
        }
        if self.mounted_subject_id.is_none() {
            self.mounted_subject_id = Some(subject_id.to_string());
        }
        self
    }

    pub fn from_message(message: &TranscriptInputMessage, subject_id: &str) -> Self {
        Self::from_message_with_turn_actor(message, subject_id, None)
    }

    pub fn from_message_with_turn_actor(
        message: &TranscriptInputMessage,
        subject_id: &str,
        turn_actor: Option<&ActorAttribution>,
    ) -> Self {
        let inherited = turn_actor
            .cloned()
            .unwrap_or_else(|| Self::for_subject(subject_id))
            .normalized_for_subject(subject_id);
        Self {
            speaker_id: message.speaker_id.clone(),
            speaker_kind: message.speaker_kind.clone(),
            subject_id: inherited.subject_id,
            actor_subject_id: inherited.actor_subject_id,
            mounted_subject_id: inherited.mounted_subject_id,
            agent_id: inherited.agent_id,
            triggered_by: inherited.triggered_by,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRedactionReason {
    LifecycleMasked,
    RawDeleted,
    PrivateAuthority,
    HostRefVisibility,
    HostRefLabel,
    ProfileBudget,
    OperatorOnly,
    ModelContextPolicy,
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

struct TranscriptMessageInput<'a> {
    key: &'a ConversationKey,
    turn_id: &'a str,
    sequence: u64,
    message: &'a TranscriptInputMessage,
    fallback_observed_at: u64,
    created_at: u64,
    subject_id: &'a str,
    turn_actor: Option<&'a ActorAttribution>,
}

impl TranscriptMessageRecord {
    fn from_input(input: TranscriptMessageInput<'_>) -> Self {
        let occurrence = u32::try_from(input.sequence).unwrap_or(u32::MAX);
        let observed_at = if input.message.observed_at == 0 {
            input.fallback_observed_at
        } else {
            input.message.observed_at
        };
        Self {
            message_id: synthesize_session_message_id(
                &input.key.storage_key(),
                &input.message.role,
                &input.message.content,
                occurrence,
            ),
            turn_id: input.turn_id.to_string(),
            sequence: input.sequence,
            role: input.message.role.clone(),
            content: input.message.content.clone(),
            authority: input.message.authority,
            observed_at,
            created_at: input.created_at.max(observed_at),
            actor: ActorAttribution::from_message_with_turn_actor(
                input.message,
                input.subject_id,
                input.turn_actor,
            ),
        }
    }

    pub fn from_committed_input(
        turn_id: &str,
        sequence: u64,
        message: &TranscriptInputMessage,
        committed: &CommittedSessionMessage,
        subject_id: &str,
        turn_actor: Option<&ActorAttribution>,
    ) -> Self {
        let inherited = turn_actor
            .cloned()
            .unwrap_or_else(|| ActorAttribution::for_subject(subject_id))
            .normalized_for_subject(subject_id);
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
                subject_id: inherited.subject_id,
                actor_subject_id: inherited.actor_subject_id,
                mounted_subject_id: inherited.mounted_subject_id,
                agent_id: inherited.agent_id,
                triggered_by: inherited.triggered_by,
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
        let actor = delta
            .actor
            .clone()
            .unwrap_or_else(|| ActorAttribution::for_subject(&delta.subject))
            .normalized_for_subject(&delta.subject);
        let input_messages = delta
            .input_messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                TranscriptMessageRecord::from_input(TranscriptMessageInput {
                    key,
                    turn_id: &delta.turn_id,
                    sequence: u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1),
                    message,
                    fallback_observed_at: now_secs,
                    created_at: now_secs,
                    subject_id: &delta.subject,
                    turn_actor: Some(&actor),
                })
            })
            .collect::<Vec<_>>();
        let assistant_message = if delta.delivery_status == MemoryTurnDeliveryStatus::Delivered {
            delta.assistant_message.as_ref().map(|message| {
                TranscriptMessageRecord::from_input(TranscriptMessageInput {
                    key,
                    turn_id: &delta.turn_id,
                    sequence: u64::try_from(input_messages.len())
                        .unwrap_or(u64::MAX)
                        .saturating_add(1),
                    message,
                    fallback_observed_at: now_secs,
                    created_at: now_secs,
                    subject_id: &delta.subject,
                    turn_actor: Some(&actor),
                })
            })
        } else {
            None
        };
        Ok(Self {
            key: key.clone(),
            turn_id: delta.turn_id.clone(),
            sequence,
            delivery_status: delta.delivery_status,
            source: delta.source.clone(),
            subject: delta.subject.clone(),
            actor,
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
        let actor = delta
            .actor
            .clone()
            .unwrap_or_else(|| ActorAttribution::for_subject(&delta.subject))
            .normalized_for_subject(&delta.subject);
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
                Some(&actor),
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
            actor,
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
pub struct TranscriptEvidenceRef {
    pub memory_space_id: String,
    pub channel_id: String,
    pub conversation_id: String,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<SubjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<MemoryEvidenceAuthority>,
}

impl TranscriptEvidenceRef {
    pub fn from_message(
        key: &ConversationKey,
        record: &TranscriptTurnRecord,
        message: &TranscriptMessageRecord,
    ) -> Self {
        Self {
            memory_space_id: key.memory_space_id.clone(),
            channel_id: key.channel_id.clone(),
            conversation_id: key.conversation_id.clone(),
            turn_id: record.turn_id.clone(),
            message_id: Some(message.message_id.clone()),
            subject_id: Some(record.subject.clone()),
            authority: Some(message.authority),
        }
    }

    pub fn display_citation(&self) -> String {
        match self.message_id.as_deref() {
            Some(message_id) => format!(
                "transcript:{}/{}/{}#turn={}#message={}",
                self.memory_space_id,
                self.channel_id,
                self.conversation_id,
                self.turn_id,
                message_id
            ),
            None => format!(
                "transcript:{}/{}/{}#turn={}",
                self.memory_space_id, self.channel_id, self.conversation_id, self.turn_id
            ),
        }
    }

    pub fn parse_display_citation(value: &str) -> Option<Self> {
        let value = value.trim();
        let rest = value.strip_prefix("transcript:")?;
        let (path, turn_part) = rest.split_once("#turn=")?;
        let mut path_parts = path.splitn(3, '/');
        let memory_space_id = path_parts.next()?.trim();
        let channel_id = path_parts.next()?.trim();
        let conversation_id = path_parts.next()?.trim();
        let (turn_id, message_id) = turn_part
            .split_once("#message=")
            .map(|(turn_id, message_id)| (turn_id.trim(), Some(message_id.trim())))
            .unwrap_or((turn_part.trim(), None));
        if memory_space_id.is_empty()
            || channel_id.is_empty()
            || conversation_id.is_empty()
            || turn_id.is_empty()
        {
            return None;
        }
        Some(Self {
            memory_space_id: memory_space_id.to_string(),
            channel_id: channel_id.to_string(),
            conversation_id: conversation_id.to_string(),
            turn_id: turn_id.to_string(),
            message_id: message_id
                .filter(|message_id| !message_id.is_empty())
                .map(str::to_string),
            subject_id: None,
            authority: None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedMemoryPlane {
    LongTerm,
    SharedFact,
    ProceduralSkill,
    TaskLearning,
    ArchiveEvidence,
    PrivateGarden,
    SoulCandidateHandoff,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedMemoryRef {
    pub plane: DerivedMemoryPlane,
    pub store_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<SubjectId>,
    pub source: TranscriptEvidenceRef,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptLifecycleReport {
    pub key: ConversationKey,
    pub transition: TranscriptLifecycleTransition,
    pub affected_turns: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_turn_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_message_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_host_refs: Vec<HostOpaqueRef>,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub redacted_host_refs: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_ref_redactions: Vec<TranscriptRedactionReportItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_memory_refs: Vec<DerivedMemoryRef>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub profile_budget_applied: bool,
    pub reason: String,
    pub requested_by: String,
    pub requested_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptRedactionReportItem {
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_ref_index: Option<usize>,
    pub reason: TranscriptRedactionReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_authority: Option<MemoryEvidenceAuthority>,
    pub view: TranscriptReplayView,
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
    pub redacted_host_refs: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redaction_reasons: Vec<TranscriptRedactionReason>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedTranscriptSlice {
    pub key: ConversationKey,
    pub view: TranscriptReplayView,
    pub turns: Vec<RedactedTranscriptTurn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redactions: Vec<TranscriptRedactionReportItem>,
    pub audit: TranscriptReplayAudit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptTurnPage {
    pub key: ConversationKey,
    pub turns: Vec<TranscriptTurnRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

impl TranscriptTurnPage {
    pub fn from_records(
        key: ConversationKey,
        records: &[TranscriptTurnRecord],
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Self> {
        let start = match cursor.map(str::trim).filter(|value| !value.is_empty()) {
            Some(cursor) => records
                .iter()
                .position(|record| transcript_turn_cursor(record) == cursor)
                .map(|index| index.saturating_add(1))
                .ok_or_else(|| Error::config("conversation_transcript_page", "cursor_not_found"))?,
            None => 0,
        };
        let limit = limit.max(1);
        let end = start.saturating_add(limit).min(records.len());
        let turns = records[start..end].to_vec();
        let has_more = end < records.len();
        let next_cursor = if has_more {
            turns.last().map(transcript_turn_cursor)
        } else {
            None
        };
        Ok(Self {
            key,
            turns,
            next_cursor,
            has_more,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRepairIssueKind {
    MissingSourceTurn,
    MissingSourceMessage,
    OrphanDerivedRef,
    MismatchedSourceKey,
    DuplicateTurnCursor,
    CorruptRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptRepairIssue {
    pub kind: TranscriptRepairIssueKind,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_ref: Option<DerivedMemoryRef>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptRepairReport {
    pub key: ConversationKey,
    pub checked_turns: usize,
    pub checked_derived_refs: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<TranscriptRepairIssue>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub profile_budget_applied: bool,
    pub fail_closed: bool,
}

impl RedactedTranscriptSlice {
    pub fn from_records(
        key: ConversationKey,
        view: TranscriptReplayView,
        records: &[TranscriptTurnRecord],
    ) -> Self {
        let mut redacted_messages = 0usize;
        let mut redacted_host_refs = 0usize;
        let mut redactions = Vec::new();
        let turns = records
            .iter()
            .map(|record| {
                let input_messages = record
                    .input_messages
                    .iter()
                    .map(|message| {
                        redact_message_for_view(
                            record,
                            message,
                            view,
                            &mut redacted_messages,
                            &mut redactions,
                        )
                    })
                    .collect();
                let assistant_message = record.assistant_message.as_ref().map(|message| {
                    redact_message_for_view(
                        record,
                        message,
                        view,
                        &mut redacted_messages,
                        &mut redactions,
                    )
                });
                let host_refs = filter_host_refs_for_view(
                    record,
                    view,
                    &mut redacted_host_refs,
                    &mut redactions,
                );
                RedactedTranscriptTurn {
                    turn_id: record.turn_id.clone(),
                    sequence: record.sequence,
                    subject: record.subject.clone(),
                    actor: record.actor.clone(),
                    delivery_status: record.delivery_status,
                    input_messages,
                    assistant_message,
                    host_refs,
                    lifecycle_state: record.lifecycle_state,
                    redaction_state: record.redaction_state,
                }
            })
            .collect::<Vec<_>>();
        let redaction_reasons = collect_redaction_reasons(&redactions);
        Self {
            key,
            view,
            audit: TranscriptReplayAudit {
                view,
                source_turns: records.len(),
                returned_turns: turns.len(),
                redacted_messages,
                redacted_host_refs,
                redaction_reasons,
            },
            turns,
            redactions,
        }
    }
}

pub trait ConversationTranscriptStore: Send + Sync {
    fn append_turn(&self, record: &TranscriptTurnRecord) -> Result<TranscriptCommitReport>;
    fn remember_conversation_alias(&self, alias: &TranscriptConversationAlias) -> Result<()>;
    fn resolve_conversation_alias(
        &self,
        memory_space_id: &str,
        channel_id: &str,
        chat_id: &str,
    ) -> Result<Option<String>>;
    fn get_turn(
        &self,
        key: &ConversationKey,
        turn_id: &str,
    ) -> Result<Option<TranscriptTurnRecord>>;
    fn list_turns(&self, key: &ConversationKey, limit: usize) -> Result<Vec<TranscriptTurnRecord>>;
    fn list_turns_page(
        &self,
        key: &ConversationKey,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<TranscriptTurnPage> {
        let records = self.list_turns(key, usize::MAX)?;
        TranscriptTurnPage::from_records(key.clone(), &records, cursor, limit)
    }
    fn append_derived_memory_ref(
        &self,
        key: &ConversationKey,
        derived: &DerivedMemoryRef,
    ) -> Result<()>;
    fn list_derived_memory_refs(
        &self,
        key: &ConversationKey,
        turn_id: Option<&str>,
    ) -> Result<Vec<DerivedMemoryRef>>;
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

    fn redacted_replay_page(
        &self,
        key: &ConversationKey,
        cursor: Option<&str>,
        limit: usize,
        view: TranscriptReplayView,
    ) -> Result<(RedactedTranscriptSlice, Option<String>, bool)> {
        let page = self.list_turns_page(key, cursor, limit)?;
        Ok((
            RedactedTranscriptSlice::from_records(key.clone(), view, &page.turns),
            page.next_cursor,
            page.has_more,
        ))
    }

    fn repair_report(&self, key: &ConversationKey) -> Result<TranscriptRepairReport> {
        let turns = self.list_turns(key, usize::MAX)?;
        let mut turn_message_ids = HashMap::<String, HashSet<String>>::new();
        let mut turn_ids = HashSet::<String>::new();
        let mut turn_sequences = HashMap::<u64, String>::new();
        let mut issues = Vec::new();
        for turn in &turns {
            if turn.turn_id.trim().is_empty() {
                issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::CorruptRecord,
                    turn_id: turn.turn_id.clone(),
                    message_id: None,
                    derived_ref: None,
                    reason: "transcript_turn_id_empty".to_string(),
                });
            }
            if turn.key != *key {
                issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::CorruptRecord,
                    turn_id: turn.turn_id.clone(),
                    message_id: None,
                    derived_ref: None,
                    reason: "transcript_turn_key_mismatch".to_string(),
                });
            }
            if !turn_ids.insert(turn.turn_id.clone()) {
                issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::DuplicateTurnCursor,
                    turn_id: turn.turn_id.clone(),
                    message_id: None,
                    derived_ref: None,
                    reason: "transcript_turn_id_duplicate".to_string(),
                });
            }
            if turn.sequence > 0 {
                if let Some(previous_turn_id) =
                    turn_sequences.insert(turn.sequence, turn.turn_id.clone())
                {
                    if previous_turn_id != turn.turn_id {
                        issues.push(TranscriptRepairIssue {
                            kind: TranscriptRepairIssueKind::DuplicateTurnCursor,
                            turn_id: turn.turn_id.clone(),
                            message_id: None,
                            derived_ref: None,
                            reason: format!(
                                "transcript_turn_sequence_duplicate:{previous_turn_id}"
                            ),
                        });
                    }
                }
            }
            let mut message_ids = turn
                .input_messages
                .iter()
                .map(|message| message.message_id.clone())
                .collect::<HashSet<_>>();
            if let Some(message) = turn.assistant_message.as_ref() {
                message_ids.insert(message.message_id.clone());
            }
            turn_message_ids.insert(turn.turn_id.clone(), message_ids);
        }
        let derived_refs = self.list_derived_memory_refs(key, None)?;
        for derived in &derived_refs {
            if derived.store_key.trim().is_empty() {
                issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::OrphanDerivedRef,
                    turn_id: derived.source.turn_id.clone(),
                    message_id: derived.source.message_id.clone(),
                    derived_ref: Some(derived.clone()),
                    reason: "derived_memory_ref_store_key_empty".to_string(),
                });
            }
            if derived.source.memory_space_id != key.memory_space_id
                || derived.source.channel_id != key.channel_id
                || derived.source.conversation_id != key.conversation_id
            {
                issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::MismatchedSourceKey,
                    turn_id: derived.source.turn_id.clone(),
                    message_id: derived.source.message_id.clone(),
                    derived_ref: Some(derived.clone()),
                    reason: "derived_memory_ref_source_key_mismatch".to_string(),
                });
                continue;
            }
            let Some(message_ids) = turn_message_ids.get(&derived.source.turn_id) else {
                issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::MissingSourceTurn,
                    turn_id: derived.source.turn_id.clone(),
                    message_id: derived.source.message_id.clone(),
                    derived_ref: Some(derived.clone()),
                    reason: "derived_memory_ref_source_turn_missing".to_string(),
                });
                continue;
            };
            if let Some(message_id) = derived.source.message_id.as_deref() {
                if !message_ids.contains(message_id) {
                    issues.push(TranscriptRepairIssue {
                        kind: TranscriptRepairIssueKind::MissingSourceMessage,
                        turn_id: derived.source.turn_id.clone(),
                        message_id: Some(message_id.to_string()),
                        derived_ref: Some(derived.clone()),
                        reason: "derived_memory_ref_source_message_missing".to_string(),
                    });
                }
            }
        }
        Ok(TranscriptRepairReport {
            key: key.clone(),
            checked_turns: turns.len(),
            checked_derived_refs: derived_refs.len(),
            profile_budget_applied: false,
            fail_closed: !issues.is_empty(),
            issues,
        })
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

fn transcript_turn_cursor(record: &TranscriptTurnRecord) -> String {
    format!("{}:{}", record.sequence, record.turn_id)
}

fn redact_message_for_view(
    record: &TranscriptTurnRecord,
    message: &TranscriptMessageRecord,
    view: TranscriptReplayView,
    redacted_messages: &mut usize,
    redactions: &mut Vec<TranscriptRedactionReportItem>,
) -> RedactedTranscriptMessage {
    let redaction_reason = message_redaction_reason(record, message, view);
    if let Some(reason) = redaction_reason {
        *redacted_messages = redacted_messages.saturating_add(1);
        redactions.push(TranscriptRedactionReportItem {
            turn_id: record.turn_id.clone(),
            message_id: Some(message.message_id.clone()),
            host_ref_index: None,
            reason,
            source_authority: Some(message.authority),
            view,
        });
    }
    RedactedTranscriptMessage {
        message_id: message.message_id.clone(),
        role: message.role.clone(),
        content: if redaction_reason.is_some() {
            None
        } else {
            Some(message.content.clone())
        },
        authority: message.authority,
        actor: message.actor.clone(),
        observed_at: message.observed_at,
        redacted: redaction_reason.is_some(),
    }
}

fn message_redaction_reason(
    record: &TranscriptTurnRecord,
    message: &TranscriptMessageRecord,
    view: TranscriptReplayView,
) -> Option<TranscriptRedactionReason> {
    if matches!(record.redaction_state, TranscriptRedactionState::RawDeleted)
        || matches!(record.lifecycle_state, TranscriptLifecycleState::RawDeleted)
    {
        return Some(TranscriptRedactionReason::RawDeleted);
    }
    if matches!(record.redaction_state, TranscriptRedactionState::Masked)
        || matches!(record.lifecycle_state, TranscriptLifecycleState::Masked)
    {
        return Some(TranscriptRedactionReason::LifecycleMasked);
    }
    if view == TranscriptReplayView::RawOwnerOnly {
        return None;
    }
    match message.authority {
        MemoryEvidenceAuthority::PrivateGardenInternal
        | MemoryEvidenceAuthority::SoulGovernance => {
            Some(TranscriptRedactionReason::PrivateAuthority)
        }
        MemoryEvidenceAuthority::OperatorDiagnostic => {
            Some(TranscriptRedactionReason::OperatorOnly)
        }
        _ => None,
    }
}

fn filter_host_refs_for_view(
    record: &TranscriptTurnRecord,
    view: TranscriptReplayView,
    redacted_host_refs: &mut usize,
    redactions: &mut Vec<TranscriptRedactionReportItem>,
) -> Vec<HostOpaqueRef> {
    let mut visible = Vec::with_capacity(record.host_refs.len());
    for (index, host_ref) in record.host_refs.iter().enumerate() {
        if host_ref_visible_in_view(host_ref.visibility, view) {
            let sanitized = sanitize_host_ref_for_view(host_ref, view);
            if host_ref.label.is_some() && sanitized.label.is_none() {
                redactions.push(TranscriptRedactionReportItem {
                    turn_id: record.turn_id.clone(),
                    message_id: None,
                    host_ref_index: Some(index),
                    reason: TranscriptRedactionReason::HostRefLabel,
                    source_authority: None,
                    view,
                });
            }
            visible.push(sanitized);
            continue;
        }
        *redacted_host_refs = redacted_host_refs.saturating_add(1);
        redactions.push(TranscriptRedactionReportItem {
            turn_id: record.turn_id.clone(),
            message_id: None,
            host_ref_index: Some(index),
            reason: TranscriptRedactionReason::HostRefVisibility,
            source_authority: None,
            view,
        });
    }
    visible
}

pub fn filter_host_refs_for_transcript_view(
    turn_id: &str,
    host_refs: &[HostOpaqueRef],
    view: TranscriptReplayView,
) -> (
    Vec<HostOpaqueRef>,
    Vec<TranscriptRedactionReportItem>,
    usize,
) {
    let mut redacted_host_refs = 0usize;
    let mut redactions = Vec::new();
    let mut visible = Vec::with_capacity(host_refs.len());
    for (index, host_ref) in host_refs.iter().enumerate() {
        if host_ref_visible_in_view(host_ref.visibility, view) {
            let sanitized = sanitize_host_ref_for_view(host_ref, view);
            if host_ref.label.is_some() && sanitized.label.is_none() {
                redactions.push(TranscriptRedactionReportItem {
                    turn_id: turn_id.to_string(),
                    message_id: None,
                    host_ref_index: Some(index),
                    reason: TranscriptRedactionReason::HostRefLabel,
                    source_authority: None,
                    view,
                });
            }
            visible.push(sanitized);
            continue;
        }
        redacted_host_refs = redacted_host_refs.saturating_add(1);
        redactions.push(TranscriptRedactionReportItem {
            turn_id: turn_id.to_string(),
            message_id: None,
            host_ref_index: Some(index),
            reason: TranscriptRedactionReason::HostRefVisibility,
            source_authority: None,
            view,
        });
    }
    (visible, redactions, redacted_host_refs)
}

fn sanitize_host_ref_for_view(
    host_ref: &HostOpaqueRef,
    view: TranscriptReplayView,
) -> HostOpaqueRef {
    let mut host_ref = host_ref.clone();
    if !host_ref_label_visible_in_view(host_ref.visibility, view) {
        host_ref.label = None;
    }
    host_ref
}

fn host_ref_label_visible_in_view(
    visibility: HostRefVisibility,
    view: TranscriptReplayView,
) -> bool {
    matches!(view, TranscriptReplayView::RawOwnerOnly)
        || matches!(
            (visibility, view),
            (HostRefVisibility::HostUi, TranscriptReplayView::HostUi)
        )
}

fn host_ref_visible_in_view(visibility: HostRefVisibility, view: TranscriptReplayView) -> bool {
    match view {
        TranscriptReplayView::RawOwnerOnly => true,
        TranscriptReplayView::ModelContext => matches!(visibility, HostRefVisibility::ModelContext),
        TranscriptReplayView::HostUi => {
            matches!(
                visibility,
                HostRefVisibility::HostUi | HostRefVisibility::Export
            )
        }
        TranscriptReplayView::OperatorAudit => matches!(
            visibility,
            HostRefVisibility::HostUi
                | HostRefVisibility::OperatorAudit
                | HostRefVisibility::Export
        ),
        TranscriptReplayView::Export => matches!(visibility, HostRefVisibility::Export),
    }
}

fn collect_redaction_reasons(
    redactions: &[TranscriptRedactionReportItem],
) -> Vec<TranscriptRedactionReason> {
    let mut reasons = Vec::new();
    for redaction in redactions {
        if !reasons.contains(&redaction.reason) {
            reasons.push(redaction.reason);
        }
    }
    reasons
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
