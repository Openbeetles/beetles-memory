use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use crate::budget::{
    MAX_TRANSCRIPT_ACTIVITY_BUCKETS, MAX_TRANSCRIPT_QUERY_CURSOR_BYTES,
    MAX_TRANSCRIPT_SEARCH_EXCERPT_CHARS, MAX_TRANSCRIPT_SEARCH_QUERY_BYTES,
    MAX_TRANSCRIPT_SEARCH_QUERY_CHARS, MAX_TRANSCRIPT_SEARCH_TERMS,
};
use crate::error::{Error, Result};

use super::{
    synthesize_session_message_id, CanonicalTurnDelta, CommittedSessionMessage,
    MemoryEvidenceAuthority, MemoryTurnDeliveryStatus, MemoryTurnSource, SessionTurnCommitReport,
    SubjectId, ToolObservationDigest, TranscriptInputMessage, MAX_SESSION_MESSAGE_LEN,
};
use crate::util::{collect_retrieval_terms, is_cjk, normalize_retrieval_text};

/// Indexing allows a full admitted session message. For the 2/3-gram algorithm,
/// the number of emitted unique terms is bounded by twice the UTF-8 byte budget.
pub const MAX_TRANSCRIPT_INDEX_TERMS_PER_MESSAGE: usize = 2 * MAX_SESSION_MESSAGE_LEN;

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
    pub mounted_subject_id: String,
    pub channel_id: String,
    pub chat_id: String,
    pub conversation_id: String,
    pub updated_at: u64,
}

impl TranscriptConversationAlias {
    pub fn new(
        memory_space_id: impl Into<String>,
        mounted_subject_id: impl Into<String>,
        channel_id: impl Into<String>,
        chat_id: impl Into<String>,
        conversation_id: impl Into<String>,
        updated_at: u64,
    ) -> Result<Self> {
        let alias = Self {
            memory_space_id: memory_space_id.into().trim().to_string(),
            mounted_subject_id: mounted_subject_id.into().trim().to_string(),
            channel_id: channel_id.into().trim().to_string(),
            chat_id: chat_id.into().trim().to_string(),
            conversation_id: conversation_id.into().trim().to_string(),
            updated_at,
        };
        alias.validate()?;
        Ok(alias)
    }

    pub fn storage_key(&self) -> String {
        Self::storage_key_for(
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.channel_id,
            &self.chat_id,
        )
    }

    pub fn storage_key_for(
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel_id: &str,
        chat_id: &str,
    ) -> String {
        format!(
            "{}__{}__{}__{}",
            encode_labeled_key_component("ms", memory_space_id),
            encode_labeled_key_component("subject", mounted_subject_id),
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
        if self.mounted_subject_id.trim().is_empty() {
            return Err(Error::config(
                "conversation_transcript_alias",
                "mounted_subject_id must not be empty",
            ));
        }
        if self.chat_id.trim().is_empty() {
            return Err(Error::config(
                "conversation_transcript_alias",
                "chat_id must not be empty",
            ));
        }
        Ok(())
    }

    pub fn validate_for_transcript_owner(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
    ) -> Result<()> {
        self.validate()?;
        key.validate()?;
        require_canonical_transcript_query_component(mounted_subject_id, "mounted_subject_id")?;
        if self.memory_space_id != key.memory_space_id
            || self.mounted_subject_id != mounted_subject_id
            || self.channel_id != key.channel_id
            || self.conversation_id != key.conversation_id
        {
            return Err(Error::config(
                "conversation_transcript_append",
                "conversation_alias_must_match_transcript_owner",
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
    AttrVisibility,
    AttrValueBudget,
    AttrLifecyclePolicy,
    ProfileBudget,
    OperatorOnly,
    ModelContextPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptAttrScope {
    Turn,
    Message,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptAttrValueKind {
    JsonObject,
    JsonArray,
    String,
    Number,
    Boolean,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptAttrSourceKind {
    HostReported,
    ProviderReported,
    GatewayCounted,
    HostEstimated,
    MemoryComputed,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptAttrRedactionPolicy {
    FollowTranscript,
    MetadataSurvivesMask,
    OperatorAuditOnlyAfterMask,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptAttrTarget {
    pub key: ConversationKey,
    pub scope: TranscriptAttrScope,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptAttrSource {
    pub writer: String,
    pub source_kind: TranscriptAttrSourceKind,
    pub written_at: u64,
    pub audit_reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptAttrGovernance {
    pub max_value_bytes: u32,
    pub redaction_policy: TranscriptAttrRedactionPolicy,
    pub export_allowed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptAttrLink {
    pub relation: String,
    pub ref_kind: String,
    pub ref_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptAttrEnvelope {
    pub attr_id: String,
    pub target: TranscriptAttrTarget,
    pub key: String,
    pub value_kind: TranscriptAttrValueKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_ref: Option<String>,
    pub value: Value,
    pub visibility: HostRefVisibility,
    pub source: TranscriptAttrSource,
    pub governance: TranscriptAttrGovernance,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<TranscriptAttrLink>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Eq for TranscriptAttrEnvelope {}

impl TranscriptAttrEnvelope {
    pub fn validate(&self) -> Result<()> {
        if self.attr_id.trim().is_empty() {
            return Err(Error::config(
                "transcript_attr",
                "attr_id must not be empty",
            ));
        }
        validate_transcript_attr_key(&self.key)?;
        if self.target.turn_id.trim().is_empty() {
            return Err(Error::config(
                "transcript_attr",
                "target turn_id must not be empty",
            ));
        }
        match self.target.scope {
            TranscriptAttrScope::Turn => {
                if self.target.message_id.is_some() {
                    return Err(Error::config(
                        "transcript_attr",
                        "turn scoped attr must not include message_id",
                    ));
                }
            }
            TranscriptAttrScope::Message => {
                if self
                    .target
                    .message_id
                    .as_deref()
                    .is_none_or(|message_id| message_id.trim().is_empty())
                {
                    return Err(Error::config(
                        "transcript_attr",
                        "message scoped attr requires message_id",
                    ));
                }
            }
        }
        validate_transcript_attr_value_kind(self.value_kind, &self.value)?;
        if self.source.writer.trim().is_empty() {
            return Err(Error::config(
                "transcript_attr",
                "source writer must not be empty",
            ));
        }
        if self.source.audit_reason.trim().is_empty() {
            return Err(Error::config(
                "transcript_attr",
                "source audit_reason must not be empty",
            ));
        }
        if self.governance.max_value_bytes == 0 {
            return Err(Error::config(
                "transcript_attr",
                "max_value_bytes must be greater than zero",
            ));
        }
        let value_bytes = serde_json::to_vec(&self.value)
            .map_err(|err| Error::config("transcript_attr", err.to_string()))?
            .len();
        if value_bytes > self.governance.max_value_bytes as usize {
            return Err(Error::config(
                "transcript_attr",
                "value exceeds max_value_bytes",
            ));
        }
        if self.updated_at < self.created_at {
            return Err(Error::config(
                "transcript_attr",
                "updated_at must be greater than or equal to created_at",
            ));
        }
        for link in &self.links {
            if link.relation.trim().is_empty()
                || link.ref_kind.trim().is_empty()
                || link.ref_id.trim().is_empty()
            {
                return Err(Error::config(
                    "transcript_attr",
                    "attr links must include relation, ref_kind, and ref_id",
                ));
            }
        }
        Ok(())
    }

    pub fn validate_for_record(&self, record: &TranscriptTurnRecord) -> Result<()> {
        self.validate()?;
        if self.target.key != record.key {
            return Err(Error::config(
                "transcript_attr",
                "attr target key does not match transcript turn key",
            ));
        }
        if self.target.turn_id != record.turn_id {
            return Err(Error::config(
                "transcript_attr",
                "attr target turn_id does not match transcript turn",
            ));
        }
        if let TranscriptAttrScope::Message = self.target.scope {
            let message_id = self.target.message_id.as_deref().unwrap_or_default();
            if !record_has_message(record, message_id) {
                return Err(Error::config(
                    "transcript_attr",
                    "attr target message_id does not exist in transcript turn",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptAttrWriteRejection {
    pub attr_id: String,
    pub attr_key: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptAttrWriteReport {
    pub key: ConversationKey,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepted_attrs: Vec<TranscriptAttrEnvelope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_attrs: Vec<TranscriptAttrWriteRejection>,
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

/// Returns whether a transcript message may contribute to host-presentable
/// search and activity indexes.
///
/// Query indexes are derived presentation material, not an authority boundary.
/// They therefore admit only dialogue roles and must never contain private
/// garden, Soul-governance, or operator-diagnostic content. Runtime disclosure
/// is still applied again after candidate hydration.
pub fn transcript_message_is_query_index_eligible(message: &TranscriptMessageRecord) -> bool {
    (message.role.eq_ignore_ascii_case("user") || message.role.eq_ignore_ascii_case("assistant"))
        && !matches!(
            message.authority,
            MemoryEvidenceAuthority::PrivateGardenInternal
                | MemoryEvidenceAuthority::SoulGovernance
                | MemoryEvidenceAuthority::OperatorDiagnostic
        )
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

    pub fn is_searchable_for_presentation(&self) -> bool {
        matches!(
            self.lifecycle_state,
            TranscriptLifecycleState::Active | TranscriptLifecycleState::Archived
        ) && self.redaction_state == TranscriptRedactionState::RawAvailable
    }

    pub fn contributes_to_presentation_activity(&self) -> bool {
        self.is_searchable_for_presentation()
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attr_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attr_key: Option<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<TranscriptAttrEnvelope>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<TranscriptAttrEnvelope>,
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

const TRANSCRIPT_QUERY_STAGE: &str = "conversation_transcript_query";
const TRANSCRIPT_QUERY_CURSOR_PREFIX: &str = "btq1:";

/// Opaque continuation value. Runtime/Store owns claims, authentication and issuance.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TranscriptQueryCursor(String);

impl std::fmt::Debug for TranscriptQueryCursor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TranscriptQueryCursor([REDACTED])")
    }
}

pub const TRANSCRIPT_CURSOR_DISCLOSURE_POLICY_SCHEMA_V1: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptCursorOperationKind {
    Catalog,
    Timeline,
    Search,
}

impl TranscriptCursorOperationKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Timeline => "timeline",
            Self::Search => "search",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptCursorDisclosurePolicyV1 {
    pub schema_version: u32,
    pub capability_context_digest: String,
}

impl TranscriptCursorDisclosurePolicyV1 {
    pub fn new(schema_version: u32, capability_context_digest: impl Into<String>) -> Result<Self> {
        let policy = Self {
            schema_version,
            capability_context_digest: capability_context_digest.into(),
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != TRANSCRIPT_CURSOR_DISCLOSURE_POLICY_SCHEMA_V1
            || !canonical_sha256_digest(&self.capability_context_digest)
        {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "cursor_disclosure_policy_invalid",
            ));
        }
        Ok(())
    }
}

pub fn transcript_cursor_governance_context_digest(
    operation: TranscriptCursorOperationKind,
    view: TranscriptReplayView,
    policy: &TranscriptCursorDisclosurePolicyV1,
) -> Result<String> {
    policy.validate()?;
    let view = match view {
        TranscriptReplayView::RawOwnerOnly => "raw_owner_only",
        TranscriptReplayView::ModelContext => "model_context",
        TranscriptReplayView::HostUi => "host_ui",
        TranscriptReplayView::OperatorAudit => "operator_audit",
        TranscriptReplayView::Export => "export",
    };
    let mut hasher = Sha256::new();
    for component in [
        "beetle.transcript.cursor.governance-context.v1",
        operation.label(),
        view,
        &policy.schema_version.to_string(),
        &policy.capability_context_digest,
    ] {
        hasher.update(component.len().to_be_bytes());
        hasher.update(component.as_bytes());
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

impl Serialize for TranscriptQueryCursor {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TranscriptQueryCursor {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from_encoded(value).map_err(serde::de::Error::custom)
    }
}

impl TranscriptQueryCursor {
    pub fn try_from_encoded(encoded: impl Into<String>) -> Result<Self> {
        let encoded = encoded.into();
        if encoded.len() > MAX_TRANSCRIPT_QUERY_CURSOR_BYTES
            || encoded.len() <= TRANSCRIPT_QUERY_CURSOR_PREFIX.len()
            || !encoded.starts_with(TRANSCRIPT_QUERY_CURSOR_PREFIX)
            || !encoded.is_ascii()
            || encoded
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "cursor_shape_invalid",
            ));
        }
        Ok(Self(encoded))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_encoded(self) -> String {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptUtcRange {
    pub start_inclusive: u64,
    pub end_exclusive: u64,
}

impl TranscriptUtcRange {
    pub fn new(start_inclusive: u64, end_exclusive: u64) -> Result<Self> {
        let range = Self {
            start_inclusive,
            end_exclusive,
        };
        range.validate()?;
        Ok(range)
    }

    pub fn validate(&self) -> Result<()> {
        if self.start_inclusive >= self.end_exclusive {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "utc_range_must_be_nonempty_and_half_open",
            ));
        }
        Ok(())
    }

    pub fn contains(&self, timestamp: u64) -> bool {
        timestamp >= self.start_inclusive && timestamp < self.end_exclusive
    }

    pub fn validate_sorted_non_overlapping(ranges: &[Self]) -> Result<()> {
        if ranges.len() > MAX_TRANSCRIPT_ACTIVITY_BUCKETS {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "activity_bucket_limit_exceeded",
            ));
        }
        let mut previous_end = None;
        for range in ranges {
            range.validate()?;
            if previous_end.is_some_and(|end| range.start_inclusive < end) {
                return Err(Error::config(
                    TRANSCRIPT_QUERY_STAGE,
                    "utc_ranges_must_be_sorted_and_non_overlapping",
                ));
            }
            previous_end = Some(range.end_exclusive);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptLocator {
    pub key: ConversationKey,
    pub mounted_subject_id: SubjectId,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub turn_sequence: u64,
    pub observed_at: u64,
}

impl TranscriptLocator {
    pub fn new(
        key: ConversationKey,
        mounted_subject_id: impl Into<String>,
        turn_id: impl Into<String>,
        message_id: Option<String>,
        turn_sequence: u64,
        observed_at: u64,
    ) -> Result<Self> {
        let locator = Self {
            key,
            mounted_subject_id: mounted_subject_id.into(),
            turn_id: turn_id.into(),
            message_id,
            turn_sequence,
            observed_at,
        };
        locator.validate()?;
        Ok(locator)
    }

    pub fn validate(&self) -> Result<()> {
        self.key.validate()?;
        require_canonical_transcript_query_component(
            &self.mounted_subject_id,
            "mounted_subject_id",
        )?;
        require_canonical_transcript_query_component(&self.turn_id, "turn_id")?;
        if let Some(message_id) = self.message_id.as_deref() {
            require_canonical_transcript_query_component(message_id, "message_id")?;
        }
        if self.turn_sequence == 0 || self.observed_at == 0 {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "locator_sequence_and_observed_at_must_be_positive",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptAnchor {
    pub locator: TranscriptLocator,
    pub head_revision: u64,
    pub head_digest: String,
}

impl TranscriptAnchor {
    pub fn new(
        locator: TranscriptLocator,
        head_revision: u64,
        head_digest: impl Into<String>,
    ) -> Result<Self> {
        let anchor = Self {
            locator,
            head_revision,
            head_digest: head_digest.into(),
        };
        anchor.validate()?;
        Ok(anchor)
    }

    pub fn validate(&self) -> Result<()> {
        self.locator.validate()?;
        if self.head_revision == 0 || !canonical_digest(&self.head_digest) {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "anchor_head_identity_invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationCatalogHead {
    pub key: ConversationKey,
    pub mounted_subject_id: SubjectId,
    pub revision: u64,
    pub head_digest: String,
    pub turn_count: u64,
    pub message_count: u64,
    pub lifecycle: TranscriptLifecycleAggregate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sequence: Option<u64>,
    pub content_generation: u64,
    pub index_generation: u64,
    pub updated_at: u64,
}

impl ConversationCatalogHead {
    pub fn validate(&self) -> Result<()> {
        self.key.validate()?;
        require_canonical_transcript_query_component(
            &self.mounted_subject_id,
            "mounted_subject_id",
        )?;
        if self.revision == 0
            || !canonical_digest(&self.head_digest)
            || self.updated_at == 0
            || self.content_generation == 0
            || self.content_generation != self.index_generation
        {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "catalog_head_identity_invalid",
            ));
        }
        let positions = [self.first_sequence, self.last_sequence];
        self.lifecycle
            .validate(self.turn_count, self.message_count)?;
        if self.turn_count == 0 {
            if self.message_count != 0 {
                return Err(Error::config(
                    TRANSCRIPT_QUERY_STAGE,
                    "empty_catalog_head_must_not_claim_messages",
                ));
            }
            if positions.into_iter().flatten().next().is_some() {
                return Err(Error::config(
                    TRANSCRIPT_QUERY_STAGE,
                    "empty_catalog_head_must_not_claim_sequence_bounds",
                ));
            }
        } else {
            let (Some(first_sequence), Some(last_sequence)) =
                (self.first_sequence, self.last_sequence)
            else {
                return Err(Error::config(
                    TRANSCRIPT_QUERY_STAGE,
                    "nonempty_catalog_head_requires_sequence_bounds",
                ));
            };
            if first_sequence == 0 || first_sequence > last_sequence {
                return Err(Error::config(
                    TRANSCRIPT_QUERY_STAGE,
                    "catalog_head_bounds_invalid",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptLifecycleStats {
    pub turn_count: u64,
    pub message_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_observed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_at: Option<u64>,
}

impl TranscriptLifecycleStats {
    pub fn validate(&self) -> Result<()> {
        if self.turn_count == 0 && self.message_count != 0 {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "empty_lifecycle_stats_must_not_claim_messages",
            ));
        }
        if self.message_count == 0 {
            if self.first_observed_at.is_some() || self.last_observed_at.is_some() {
                return Err(Error::config(
                    TRANSCRIPT_QUERY_STAGE,
                    "message_empty_lifecycle_stats_must_not_claim_time_bounds",
                ));
            }
            return Ok(());
        }
        let (Some(first), Some(last)) = (self.first_observed_at, self.last_observed_at) else {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "nonempty_lifecycle_stats_require_time_bounds",
            ));
        };
        if first == 0 || first > last {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "lifecycle_stats_time_bounds_invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptLifecycleAggregate {
    pub active: TranscriptLifecycleStats,
    pub archived: TranscriptLifecycleStats,
    pub masked: TranscriptLifecycleStats,
    pub raw_deleted: TranscriptLifecycleStats,
}

impl TranscriptLifecycleAggregate {
    pub fn validate(&self, turn_count: u64, message_count: u64) -> Result<()> {
        let stats = [self.active, self.archived, self.masked, self.raw_deleted];
        for item in stats {
            item.validate()?;
        }
        let aggregate_turn_count = stats.iter().try_fold(0_u64, |total, item| {
            total
                .checked_add(item.turn_count)
                .ok_or_else(|| Error::config(TRANSCRIPT_QUERY_STAGE, "catalog_turn_count_overflow"))
        })?;
        let aggregate_message_count = stats.iter().try_fold(0_u64, |total, item| {
            total.checked_add(item.message_count).ok_or_else(|| {
                Error::config(TRANSCRIPT_QUERY_STAGE, "catalog_message_count_overflow")
            })
        })?;
        if aggregate_turn_count != turn_count || aggregate_message_count != message_count {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "catalog_lifecycle_aggregate_mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptCatalogLifecycle {
    ActiveOnly,
    ActiveAndArchived,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptCatalogQuery {
    pub memory_space_id: String,
    pub governance_context_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    pub lifecycle: TranscriptCatalogLifecycle,
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<TranscriptQueryCursor>,
}

impl TranscriptCatalogQuery {
    pub fn validate(&self) -> Result<()> {
        require_canonical_transcript_query_component(&self.memory_space_id, "memory_space_id")?;
        require_canonical_governance_context_digest(&self.governance_context_digest)?;
        if let Some(channel_id) = self.channel_id.as_deref() {
            require_canonical_transcript_query_component(channel_id, "channel_id")?;
        }
        validate_transcript_query_limit(self.limit)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationCatalogPage {
    pub conversations: Vec<ConversationCatalogEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<TranscriptQueryCursor>,
    pub has_more: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationCatalogEntry {
    pub key: ConversationKey,
    pub visible_turn_count: u64,
    pub visible_message_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_visible_observed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_visible_observed_at: Option<u64>,
    pub archived: bool,
    pub head_revision: u64,
    pub head_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationCatalogCandidatePage {
    pub heads: Vec<ConversationCatalogHead>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<TranscriptQueryCursor>,
    pub has_more: bool,
}

impl ConversationCatalogCandidatePage {
    pub fn validate_for_query(
        &self,
        mounted_subject_id: &str,
        query: &TranscriptCatalogQuery,
    ) -> Result<()> {
        query.validate()?;
        require_canonical_transcript_query_component(mounted_subject_id, "mounted_subject_id")?;
        for head in &self.heads {
            head.validate()?;
            if head.mounted_subject_id != mounted_subject_id
                || head.key.memory_space_id != query.memory_space_id
                || query
                    .channel_id
                    .as_ref()
                    .is_some_and(|channel| head.key.channel_id != *channel)
            {
                return Err(Error::config(
                    TRANSCRIPT_QUERY_STAGE,
                    "catalog_candidate_outside_query_scope",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptTimelineAnchor {
    Latest,
    Before(TranscriptAnchor),
    After(TranscriptAnchor),
    Around(TranscriptAnchor),
    AroundSequence(u64),
    FirstVisibleInRange(TranscriptUtcRange),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptTimelineQuery {
    pub key: ConversationKey,
    pub governance_context_digest: String,
    pub anchor: TranscriptTimelineAnchor,
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<TranscriptQueryCursor>,
}

impl TranscriptTimelineQuery {
    pub fn validate(&self) -> Result<()> {
        self.key.validate()?;
        require_canonical_governance_context_digest(&self.governance_context_digest)?;
        validate_transcript_query_limit(self.limit)?;
        match &self.anchor {
            TranscriptTimelineAnchor::Latest => Ok(()),
            TranscriptTimelineAnchor::Before(anchor)
            | TranscriptTimelineAnchor::After(anchor)
            | TranscriptTimelineAnchor::Around(anchor) => {
                anchor.validate()?;
                if anchor.locator.key != self.key {
                    return Err(Error::config(
                        TRANSCRIPT_QUERY_STAGE,
                        "timeline_anchor_conversation_mismatch",
                    ));
                }
                Ok(())
            }
            TranscriptTimelineAnchor::AroundSequence(sequence) => {
                if *sequence == 0 {
                    return Err(Error::config(
                        TRANSCRIPT_QUERY_STAGE,
                        "timeline_sequence_must_be_positive",
                    ));
                }
                Ok(())
            }
            TranscriptTimelineAnchor::FirstVisibleInRange(range) => range.validate(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptTimelineCandidatePage {
    pub head: ConversationCatalogHead,
    pub turns: Vec<TranscriptTurnRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older_cursor: Option<TranscriptQueryCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newer_cursor: Option<TranscriptQueryCursor>,
    pub has_older: bool,
    pub has_newer: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptTimelinePage {
    pub key: ConversationKey,
    pub head_revision: u64,
    pub head_digest: String,
    pub turns: Vec<RedactedTranscriptTurn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older_cursor: Option<TranscriptQueryCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newer_cursor: Option<TranscriptQueryCursor>,
    pub has_older: bool,
    pub has_newer: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedTranscriptSearchQueryV1 {
    pub schema_version: u32,
    pub normalized: String,
    pub terms: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSearchSort {
    RelevanceThenObservedAt,
    ObservedAtDescending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSearchLifecycle {
    ActiveOnly,
    ActiveAndArchived,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSearchScope {
    MountedSubject {
        memory_space_id: String,
        channel_id: Option<String>,
    },
    ExactConversation {
        key: ConversationKey,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSearchQuery {
    pub scope: TranscriptSearchScope,
    pub governance_context_digest: String,
    pub query: NormalizedTranscriptSearchQueryV1,
    pub sort: TranscriptSearchSort,
    pub lifecycle: TranscriptSearchLifecycle,
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<TranscriptQueryCursor>,
}

impl TranscriptSearchQuery {
    pub fn validate(&self) -> Result<()> {
        validate_transcript_query_limit(self.limit)?;
        require_canonical_governance_context_digest(&self.governance_context_digest)?;
        if self.query.schema_version != 1
            || self.query.normalized.is_empty()
            || self.query.terms.is_empty()
            || self.query.normalized.len() > MAX_TRANSCRIPT_SEARCH_QUERY_BYTES
            || self.query.normalized.chars().count() > MAX_TRANSCRIPT_SEARCH_QUERY_CHARS
            || self.query.terms.len() > MAX_TRANSCRIPT_SEARCH_TERMS
            || self
                .query
                .terms
                .iter()
                .any(|term| term.is_empty() || normalize_retrieval_text(term) != *term)
        {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "normalized_search_query_invalid",
            ));
        }
        match &self.scope {
            TranscriptSearchScope::MountedSubject {
                memory_space_id,
                channel_id,
            } => {
                require_canonical_transcript_query_component(memory_space_id, "memory_space_id")?;
                if let Some(channel_id) = channel_id.as_deref() {
                    require_canonical_transcript_query_component(channel_id, "channel_id")?;
                }
            }
            TranscriptSearchScope::ExactConversation { key } => key.validate()?,
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptTextHighlight {
    pub start_char: usize,
    pub end_char: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSearchExcerpt {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<TranscriptTextHighlight>,
    pub truncated_before: bool,
    pub truncated_after: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSearchHit {
    pub locator: TranscriptLocator,
    pub role: String,
    pub actor: ActorAttribution,
    pub excerpt: TranscriptSearchExcerpt,
    pub score: u32,
    pub anchor: TranscriptAnchor,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSearchCandidate {
    pub record: TranscriptTurnRecord,
    pub message_id: String,
    pub score: u32,
    pub head_revision: u64,
    pub head_digest: String,
}

impl TranscriptSearchCandidate {
    pub fn validate(&self) -> Result<()> {
        self.record.key.validate()?;
        require_canonical_transcript_query_component(&self.message_id, "message_id")?;
        if self.record.sequence == 0
            || self.head_revision == 0
            || !canonical_digest(&self.head_digest)
        {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "search_candidate_head_identity_invalid",
            ));
        }
        let matches = self
            .record
            .input_messages
            .iter()
            .chain(self.record.assistant_message.iter())
            .any(|message| message.message_id == self.message_id);
        if !matches {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "search_candidate_message_not_in_turn",
            ));
        }
        Ok(())
    }

    pub fn validate_for_scope(
        &self,
        mounted_subject_id: &str,
        scope: &TranscriptSearchScope,
    ) -> Result<()> {
        self.validate()?;
        require_canonical_transcript_query_component(mounted_subject_id, "mounted_subject_id")?;
        if self.record.subject != mounted_subject_id {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "search_candidate_subject_mismatch",
            ));
        }
        let in_scope = match scope {
            TranscriptSearchScope::MountedSubject {
                memory_space_id,
                channel_id,
            } => {
                self.record.key.memory_space_id == *memory_space_id
                    && channel_id
                        .as_ref()
                        .is_none_or(|channel| self.record.key.channel_id == *channel)
            }
            TranscriptSearchScope::ExactConversation { key } => self.record.key == *key,
        };
        if !in_scope {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "search_candidate_outside_query_scope",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSearchCandidatePage {
    pub candidates: Vec<TranscriptSearchCandidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<TranscriptQueryCursor>,
    pub has_more: bool,
    pub budget_applied: bool,
}

impl TranscriptSearchCandidatePage {
    pub fn validate_for_query(
        &self,
        mounted_subject_id: &str,
        query: &TranscriptSearchQuery,
    ) -> Result<()> {
        query.validate()?;
        for candidate in &self.candidates {
            candidate.validate_for_scope(mounted_subject_id, &query.scope)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSearchPage {
    pub hits: Vec<TranscriptSearchHit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<TranscriptQueryCursor>,
    pub has_more: bool,
    pub budget_applied: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptActivityQuery {
    pub key: ConversationKey,
    pub ranges: Vec<TranscriptUtcRange>,
    pub lifecycle: TranscriptSearchLifecycle,
}

impl TranscriptActivityQuery {
    pub fn validate(&self) -> Result<()> {
        self.key.validate()?;
        if self.ranges.is_empty() {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "activity_ranges_must_not_be_empty",
            ));
        }
        TranscriptUtcRange::validate_sorted_non_overlapping(&self.ranges)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptActivityBucket {
    pub range: TranscriptUtcRange,
    pub visible_message_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_visible_anchor: Option<TranscriptAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_visible_anchor: Option<TranscriptAnchor>,
}

impl TranscriptActivityBucket {
    pub fn validate(&self) -> Result<()> {
        self.range.validate()?;
        if self.visible_message_count == 0 {
            if self.first_visible_anchor.is_some() || self.last_visible_anchor.is_some() {
                return Err(Error::config(
                    TRANSCRIPT_QUERY_STAGE,
                    "empty_activity_bucket_must_not_claim_anchors",
                ));
            }
            return Ok(());
        }
        let (Some(first), Some(last)) = (
            self.first_visible_anchor.as_ref(),
            self.last_visible_anchor.as_ref(),
        ) else {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "nonempty_activity_bucket_requires_anchors",
            ));
        };
        first.validate()?;
        last.validate()?;
        if first.locator.key != last.locator.key
            || first.locator.mounted_subject_id != last.locator.mounted_subject_id
            || !self.range.contains(first.locator.observed_at)
            || !self.range.contains(last.locator.observed_at)
            || (first.locator.observed_at, first.locator.turn_sequence)
                > (last.locator.observed_at, last.locator.turn_sequence)
        {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "activity_bucket_anchor_mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptActivityReport {
    pub key: ConversationKey,
    pub head_revision: u64,
    pub head_digest: String,
    pub buckets: Vec<TranscriptActivityBucket>,
    pub budget_applied: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptActivityCandidate {
    pub record: TranscriptTurnRecord,
    pub message_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptActivityCandidateBucket {
    pub range: TranscriptUtcRange,
    pub candidates: Vec<TranscriptActivityCandidate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptActivityCandidateReport {
    pub head: ConversationCatalogHead,
    pub buckets: Vec<TranscriptActivityCandidateBucket>,
    pub budget_applied: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptAppendIntent {
    pub record: TranscriptTurnRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_alias: Option<TranscriptConversationAlias>,
}

impl TranscriptAppendIntent {
    pub fn validate(&self) -> Result<()> {
        self.record.key.validate()?;
        require_canonical_transcript_query_component(&self.record.subject, "record.subject")?;
        if self
            .record
            .input_messages
            .iter()
            .chain(self.record.assistant_message.iter())
            .any(|message| message.content.len() > MAX_SESSION_MESSAGE_LEN)
        {
            return Err(Error::config(
                "conversation_transcript_append",
                "transcript_message_content_budget_exceeded",
            ));
        }
        if let Some(alias) = self.conversation_alias.as_ref() {
            alias.validate_for_transcript_owner(&self.record.key, &self.record.subject)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TranscriptSearchNormalizerV1;

impl TranscriptSearchNormalizerV1 {
    pub fn normalize(query: &str) -> Result<NormalizedTranscriptSearchQueryV1> {
        if query.len() > MAX_TRANSCRIPT_SEARCH_QUERY_BYTES
            || query.chars().count() > MAX_TRANSCRIPT_SEARCH_QUERY_CHARS
            || query.chars().any(char::is_control)
        {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "search_query_limit_or_control_character_invalid",
            ));
        }
        let normalized = normalize_retrieval_text(query);
        let terms = collect_retrieval_terms(query, 2, MAX_TRANSCRIPT_SEARCH_TERMS, &[2, 3]);
        if normalized.is_empty() || terms.is_empty() {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "search_query_has_no_canonical_terms",
            ));
        }
        Ok(NormalizedTranscriptSearchQueryV1 {
            schema_version: 1,
            normalized,
            terms,
        })
    }

    pub fn index_terms(content: &str, max_terms: usize) -> Result<Vec<String>> {
        if max_terms == 0 || max_terms > MAX_TRANSCRIPT_INDEX_TERMS_PER_MESSAGE {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "transcript_document_index_term_budget_invalid",
            ));
        }
        let detection_limit = max_terms.checked_add(1).ok_or_else(|| {
            Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "transcript_document_index_term_budget_overflow",
            )
        })?;
        let terms = collect_retrieval_terms(content, 2, detection_limit, &[2, 3]);
        if terms.len() > max_terms {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "transcript_document_index_term_limit_exceeded",
            ));
        }
        // A valid transcript message need not contain searchable lexical terms
        // (for example a single CJK character, emoji, or punctuation). Index
        // eligibility must not become transcript admission authority.
        Ok(terms)
    }

    pub fn excerpt(
        content: &str,
        query: &NormalizedTranscriptSearchQueryV1,
        max_chars: usize,
    ) -> Result<TranscriptSearchExcerpt> {
        if query.schema_version != 1
            || query.normalized.is_empty()
            || query.terms.is_empty()
            || max_chars == 0
            || max_chars > MAX_TRANSCRIPT_SEARCH_EXCERPT_CHARS
        {
            return Err(Error::config(
                TRANSCRIPT_QUERY_STAGE,
                "search_excerpt_input_invalid",
            ));
        }
        let content_chars = content.chars().collect::<Vec<_>>();
        let (normalized_chars, normalized_to_original) =
            normalize_transcript_text_with_char_map(content);
        let mut needles = Vec::with_capacity(query.terms.len().saturating_add(1));
        needles.push(query.normalized.chars().collect::<Vec<_>>());
        for term in &query.terms {
            let chars = term.chars().collect::<Vec<_>>();
            if !chars.is_empty() && !needles.iter().any(|existing| existing == &chars) {
                needles.push(chars);
            }
        }
        needles.sort_by_key(|needle| std::cmp::Reverse(needle.len()));
        let mut raw_ranges = Vec::<(usize, usize)>::new();
        for needle in needles {
            if needle.is_empty() || normalized_chars.len() < needle.len() {
                continue;
            }
            for start in 0..=normalized_chars.len().saturating_sub(needle.len()) {
                if normalized_chars[start..start + needle.len()] == needle[..] {
                    let original_start = normalized_to_original[start];
                    let original_end = normalized_to_original[start + needle.len() - 1] + 1;
                    raw_ranges.push((original_start, original_end));
                }
            }
        }
        raw_ranges.sort_unstable();
        raw_ranges.dedup();
        let focus = raw_ranges.first().copied().unwrap_or((0, 0));
        let start = if content_chars.len() <= max_chars {
            0
        } else {
            focus
                .0
                .saturating_sub(max_chars.saturating_sub(focus.1.saturating_sub(focus.0)) / 2)
                .min(content_chars.len().saturating_sub(max_chars))
        };
        let end = start.saturating_add(max_chars).min(content_chars.len());
        let text = content_chars[start..end].iter().collect::<String>();
        let mut highlights = raw_ranges
            .into_iter()
            .filter_map(|(range_start, range_end)| {
                let clipped_start = range_start.max(start);
                let clipped_end = range_end.min(end);
                (clipped_start < clipped_end).then_some(TranscriptTextHighlight {
                    start_char: clipped_start - start,
                    end_char: clipped_end - start,
                })
            })
            .collect::<Vec<_>>();
        highlights.sort_by_key(|range| (range.start_char, range.end_char));
        highlights.dedup();
        Ok(TranscriptSearchExcerpt {
            text,
            highlights,
            truncated_before: start > 0,
            truncated_after: end < content_chars.len(),
        })
    }
}

fn normalize_transcript_text_with_char_map(input: &str) -> (Vec<char>, Vec<usize>) {
    let mut normalized = Vec::with_capacity(input.chars().count());
    let mut mapping = Vec::with_capacity(input.chars().count());
    let mut pending_space_at = None;
    for (original_index, ch) in input.chars().enumerate() {
        if ch.is_alphanumeric() || is_cjk(ch) {
            if !normalized.is_empty() {
                if let Some(space_at) = pending_space_at.take() {
                    normalized.push(' ');
                    mapping.push(space_at);
                }
            } else {
                pending_space_at = None;
            }
            for lower in ch.to_lowercase() {
                normalized.push(lower);
                mapping.push(original_index);
            }
        } else if pending_space_at.is_none() {
            pending_space_at = Some(original_index);
        }
    }
    (normalized, mapping)
}

fn require_canonical_transcript_query_component(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().any(char::is_control)
        || value.len() > 512
    {
        return Err(Error::config(
            TRANSCRIPT_QUERY_STAGE,
            format!("{field}_must_be_canonical"),
        ));
    }
    Ok(())
}

fn canonical_digest(value: &str) -> bool {
    value.trim() == value && value.starts_with("sha256:") && value.len() > "sha256:".len()
}

fn canonical_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn require_canonical_governance_context_digest(value: &str) -> Result<()> {
    if !canonical_sha256_digest(value) {
        return Err(Error::config(
            TRANSCRIPT_QUERY_STAGE,
            "governance_context_digest_must_be_canonical_sha256",
        ));
    }
    Ok(())
}

fn validate_transcript_query_limit(limit: usize) -> Result<()> {
    if limit == 0 {
        return Err(Error::config(
            TRANSCRIPT_QUERY_STAGE,
            "query_limit_must_be_positive",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptTurnCursor {
    pub schema_version: u32,
    pub scope_digest: String,
    pub sequence: u64,
    pub turn_id: String,
}

impl TranscriptTurnCursor {
    const SCHEMA_VERSION: u32 = 1;

    pub fn for_record(record: &TranscriptTurnRecord) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            scope_digest: transcript_cursor_scope_digest(&record.key, &record.subject),
            sequence: record.sequence,
            turn_id: record.turn_id.clone(),
        }
    }

    pub fn encode(&self) -> Result<String> {
        self.validate_shape()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|error| Error::config("conversation_transcript_page", error.to_string()))?;
        Ok(format!("btc1:{}", encode_hex(&bytes)))
    }

    pub fn decode_for_scope(
        encoded: &str,
        key: &ConversationKey,
        mounted_subject_id: &str,
    ) -> Result<Self> {
        let payload = encoded.trim().strip_prefix("btc1:").ok_or_else(|| {
            Error::config("conversation_transcript_page", "cursor_schema_invalid")
        })?;
        let bytes = decode_hex(payload)?;
        let cursor = serde_json::from_slice::<Self>(&bytes)
            .map_err(|_| Error::config("conversation_transcript_page", "cursor_payload_invalid"))?;
        cursor.validate_shape()?;
        if cursor.scope_digest != transcript_cursor_scope_digest(key, mounted_subject_id) {
            return Err(Error::config(
                "conversation_transcript_page",
                "cursor_scope_mismatch",
            ));
        }
        Ok(cursor)
    }

    fn validate_shape(&self) -> Result<()> {
        if self.schema_version != Self::SCHEMA_VERSION
            || self.sequence == 0
            || self.turn_id.trim().is_empty()
            || !self.scope_digest.starts_with("sha256:")
            || self.scope_digest.len() != 71
        {
            return Err(Error::config(
                "conversation_transcript_page",
                "cursor_shape_invalid",
            ));
        }
        Ok(())
    }
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
                .position(|record| {
                    transcript_turn_cursor(record).is_ok_and(|candidate| candidate == cursor)
                })
                .map(|index| index.saturating_add(1))
                .ok_or_else(|| Error::config("conversation_transcript_page", "cursor_not_found"))?,
            None => 0,
        };
        let limit = limit.max(1);
        let end = start.saturating_add(limit).min(records.len());
        let turns = records[start..end].to_vec();
        let has_more = end < records.len();
        let next_cursor = if has_more {
            turns.last().map(transcript_turn_cursor).transpose()?
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
    MissingAttrTargetTurn,
    MissingAttrTargetMessage,
    OrphanDerivedRef,
    MismatchedSourceKey,
    MismatchedAttrSourceKey,
    OversizedAttrValue,
    InvalidAttrKey,
    InvalidAttrVisibility,
    CorruptTranscriptAttrRecord,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TranscriptRepairInspection {
    pub turns: Vec<TranscriptTurnRecord>,
    pub derived_refs: Vec<DerivedMemoryRef>,
    pub attrs: Vec<TranscriptAttrEnvelope>,
    pub issues: Vec<TranscriptRepairIssue>,
    pub checked_turns: usize,
    pub checked_derived_refs: usize,
    pub checked_attrs: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptRepairReport {
    pub key: ConversationKey,
    pub checked_turns: usize,
    pub checked_derived_refs: usize,
    pub checked_attrs: usize,
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
        Self::from_records_with_attrs(key, view, records, &[])
    }

    pub fn from_records_with_attrs(
        key: ConversationKey,
        view: TranscriptReplayView,
        records: &[TranscriptTurnRecord],
        attrs: &[TranscriptAttrEnvelope],
    ) -> Self {
        let mut redacted_messages = 0usize;
        let mut redacted_host_refs = 0usize;
        let mut redactions = Vec::new();
        let turns = records
            .iter()
            .map(|record| {
                let attrs_for_turn =
                    filter_attrs_for_target(record, None, attrs, view, &mut redactions);
                let input_messages = record
                    .input_messages
                    .iter()
                    .map(|message| {
                        let attrs_for_message = filter_attrs_for_target(
                            record,
                            Some(&message.message_id),
                            attrs,
                            view,
                            &mut redactions,
                        );
                        redact_message_for_view(
                            record,
                            message,
                            view,
                            attrs_for_message,
                            &mut redacted_messages,
                            &mut redactions,
                        )
                    })
                    .collect();
                let assistant_message = record.assistant_message.as_ref().map(|message| {
                    let attrs_for_message = filter_attrs_for_target(
                        record,
                        Some(&message.message_id),
                        attrs,
                        view,
                        &mut redactions,
                    );
                    redact_message_for_view(
                        record,
                        message,
                        view,
                        attrs_for_message,
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
                    attrs: attrs_for_turn,
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
    fn append_turn_intent(&self, intent: &TranscriptAppendIntent)
        -> Result<TranscriptCommitReport>;
    fn append_turn(&self, record: &TranscriptTurnRecord) -> Result<TranscriptCommitReport> {
        self.append_turn_intent(&TranscriptAppendIntent {
            record: record.clone(),
            conversation_alias: None,
        })
    }
    fn remember_conversation_alias(&self, alias: &TranscriptConversationAlias) -> Result<()>;
    fn resolve_conversation_alias(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel_id: &str,
        chat_id: &str,
    ) -> Result<Option<String>>;
    fn get_turn(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: &str,
    ) -> Result<Option<TranscriptTurnRecord>>;
    fn list_turns(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        limit: usize,
    ) -> Result<Vec<TranscriptTurnRecord>>;
    fn list_conversation_catalog(
        &self,
        mounted_subject_id: &str,
        query: &TranscriptCatalogQuery,
    ) -> Result<ConversationCatalogCandidatePage>;
    fn query_transcript_timeline(
        &self,
        mounted_subject_id: &str,
        query: &TranscriptTimelineQuery,
    ) -> Result<TranscriptTimelineCandidatePage>;
    fn search_transcript(
        &self,
        mounted_subject_id: &str,
        query: &TranscriptSearchQuery,
    ) -> Result<TranscriptSearchCandidatePage>;
    fn query_transcript_activity(
        &self,
        mounted_subject_id: &str,
        query: &TranscriptActivityQuery,
    ) -> Result<TranscriptActivityCandidateReport>;
    fn turn_count(&self, key: &ConversationKey, mounted_subject_id: &str) -> Result<usize>;
    fn list_turns_page(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<TranscriptTurnPage>;
    fn upsert_transcript_attrs(
        &self,
        _key: &ConversationKey,
        _mounted_subject_id: &str,
        _attrs: &[TranscriptAttrEnvelope],
    ) -> Result<TranscriptAttrWriteReport> {
        Err(Error::config(
            "conversation_transcript_attr",
            "transcript attr persistence is not implemented for this store",
        ))
    }
    fn list_transcript_attrs(
        &self,
        _key: &ConversationKey,
        _mounted_subject_id: &str,
        _turn_id: Option<&str>,
    ) -> Result<Vec<TranscriptAttrEnvelope>> {
        Ok(Vec::new())
    }
    fn append_derived_memory_ref(
        &self,
        key: &ConversationKey,
        derived: &DerivedMemoryRef,
    ) -> Result<()>;
    fn list_derived_memory_refs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Vec<DerivedMemoryRef>>;
    fn apply_lifecycle_request(
        &self,
        mounted_subject_id: &str,
        request: &TranscriptLifecycleRequest,
    ) -> Result<TranscriptLifecycleReport>;

    fn inspect_repair_records(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
    ) -> Result<TranscriptRepairInspection> {
        let turns = self.list_turns(key, mounted_subject_id, usize::MAX)?;
        let attrs = self.list_transcript_attrs(key, mounted_subject_id, None)?;
        let derived_refs = self.list_derived_memory_refs(key, mounted_subject_id, None)?;
        Ok(TranscriptRepairInspection {
            checked_turns: turns.len(),
            checked_derived_refs: derived_refs.len(),
            checked_attrs: attrs.len(),
            turns,
            derived_refs,
            attrs,
            issues: Vec::new(),
        })
    }

    fn redacted_replay(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        limit: usize,
        view: TranscriptReplayView,
    ) -> Result<RedactedTranscriptSlice> {
        let records = self.list_turns(key, mounted_subject_id, limit)?;
        let mut attrs = Vec::new();
        for record in &records {
            attrs.extend(self.list_transcript_attrs(
                key,
                mounted_subject_id,
                Some(&record.turn_id),
            )?);
        }
        Ok(RedactedTranscriptSlice::from_records_with_attrs(
            key.clone(),
            view,
            &records,
            &attrs,
        ))
    }

    fn redacted_replay_page(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        cursor: Option<&str>,
        limit: usize,
        view: TranscriptReplayView,
    ) -> Result<(RedactedTranscriptSlice, Option<String>, bool)> {
        let page = self.list_turns_page(key, mounted_subject_id, cursor, limit)?;
        let mut attrs = Vec::new();
        for record in &page.turns {
            attrs.extend(self.list_transcript_attrs(
                key,
                mounted_subject_id,
                Some(&record.turn_id),
            )?);
        }
        Ok((
            RedactedTranscriptSlice::from_records_with_attrs(
                key.clone(),
                view,
                &page.turns,
                &attrs,
            ),
            page.next_cursor,
            page.has_more,
        ))
    }

    fn repair_report(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
    ) -> Result<TranscriptRepairReport> {
        let inspection = self.inspect_repair_records(key, mounted_subject_id)?;
        let turns = inspection.turns;
        let attrs = inspection.attrs;
        let derived_refs = inspection.derived_refs;
        let mut turn_message_ids = HashMap::<String, HashSet<String>>::new();
        let mut turn_ids = HashSet::<String>::new();
        let mut turn_sequences = HashMap::<u64, String>::new();
        let mut issues = inspection.issues;
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
        for attr in &attrs {
            if attr.target.key != *key {
                issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::MismatchedAttrSourceKey,
                    turn_id: attr.target.turn_id.clone(),
                    message_id: attr.target.message_id.clone(),
                    derived_ref: None,
                    reason: "transcript_attr_target_key_mismatch".to_string(),
                });
                continue;
            }
            if let Err(error) = attr.validate() {
                issues.push(TranscriptRepairIssue {
                    kind: transcript_attr_validation_issue_kind(error.to_string().as_str()),
                    turn_id: attr.target.turn_id.clone(),
                    message_id: attr.target.message_id.clone(),
                    derived_ref: None,
                    reason: error.to_string(),
                });
                continue;
            }
            let Some(message_ids) = turn_message_ids.get(&attr.target.turn_id) else {
                issues.push(TranscriptRepairIssue {
                    kind: TranscriptRepairIssueKind::MissingAttrTargetTurn,
                    turn_id: attr.target.turn_id.clone(),
                    message_id: attr.target.message_id.clone(),
                    derived_ref: None,
                    reason: "transcript_attr_target_turn_missing".to_string(),
                });
                continue;
            };
            if let Some(message_id) = attr.target.message_id.as_deref() {
                if !message_ids.contains(message_id) {
                    issues.push(TranscriptRepairIssue {
                        kind: TranscriptRepairIssueKind::MissingAttrTargetMessage,
                        turn_id: attr.target.turn_id.clone(),
                        message_id: Some(message_id.to_string()),
                        derived_ref: None,
                        reason: "transcript_attr_target_message_missing".to_string(),
                    });
                }
            }
        }
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
            checked_turns: inspection.checked_turns,
            checked_derived_refs: inspection.checked_derived_refs,
            checked_attrs: inspection.checked_attrs,
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

fn validate_transcript_attr_key(key: &str) -> Result<()> {
    let trimmed = key.trim();
    if trimmed != key || trimmed.is_empty() {
        return Err(Error::config(
            "transcript_attr",
            "attr key must not be empty or padded",
        ));
    }
    let mut parts = trimmed.split('.');
    let Some(owner) = parts.next() else {
        return Err(Error::config(
            "transcript_attr",
            "attr key must be namespaced",
        ));
    };
    if owner != "host" && owner != "memory" {
        return Err(Error::config(
            "transcript_attr",
            "attr key must start with host. or memory.",
        ));
    }
    let remaining = parts.collect::<Vec<_>>();
    if remaining.len() < 2 || remaining.iter().any(|part| part.is_empty()) {
        return Err(Error::config(
            "transcript_attr",
            "attr key must include namespace and name",
        ));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(Error::config(
            "transcript_attr",
            "attr key contains unsupported characters",
        ));
    }
    Ok(())
}

fn validate_transcript_attr_value_kind(kind: TranscriptAttrValueKind, value: &Value) -> Result<()> {
    let matches_kind = matches!(
        (kind, value),
        (TranscriptAttrValueKind::JsonObject, Value::Object(_))
            | (TranscriptAttrValueKind::JsonArray, Value::Array(_))
            | (TranscriptAttrValueKind::String, Value::String(_))
            | (TranscriptAttrValueKind::Number, Value::Number(_))
            | (TranscriptAttrValueKind::Boolean, Value::Bool(_))
    );
    if matches_kind {
        Ok(())
    } else {
        Err(Error::config(
            "transcript_attr",
            "attr value_kind does not match value",
        ))
    }
}

fn record_has_message(record: &TranscriptTurnRecord, message_id: &str) -> bool {
    record
        .input_messages
        .iter()
        .any(|message| message.message_id == message_id)
        || record
            .assistant_message
            .as_ref()
            .is_some_and(|message| message.message_id == message_id)
}

fn transcript_attr_validation_issue_kind(reason: &str) -> TranscriptRepairIssueKind {
    if reason.contains("max_value_bytes") || reason.contains("value exceeds") {
        TranscriptRepairIssueKind::OversizedAttrValue
    } else if reason.contains("attr key") {
        TranscriptRepairIssueKind::InvalidAttrKey
    } else if reason.contains("visibility") {
        TranscriptRepairIssueKind::InvalidAttrVisibility
    } else {
        TranscriptRepairIssueKind::CorruptTranscriptAttrRecord
    }
}

fn transcript_turn_cursor(record: &TranscriptTurnRecord) -> Result<String> {
    TranscriptTurnCursor::for_record(record).encode()
}

fn transcript_cursor_scope_digest(key: &ConversationKey, mounted_subject_id: &str) -> String {
    let mut hasher = Sha256::new();
    for field in [
        "beetle_conversation_transcript_cursor_scope_v1",
        key.memory_space_id.as_str(),
        mounted_subject_id,
        key.channel_id.as_str(),
        key.conversation_id.as_str(),
    ] {
        hasher.update(field.len().to_be_bytes());
        hasher.update(field.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn decode_hex(encoded: &str) -> Result<Vec<u8>> {
    if encoded.is_empty() || !encoded.len().is_multiple_of(2) {
        return Err(Error::config(
            "conversation_transcript_page",
            "cursor_encoding_invalid",
        ));
    }
    let mut bytes = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair).map_err(|_| {
            Error::config("conversation_transcript_page", "cursor_encoding_invalid")
        })?;
        bytes.push(u8::from_str_radix(pair, 16).map_err(|_| {
            Error::config("conversation_transcript_page", "cursor_encoding_invalid")
        })?);
    }
    Ok(bytes)
}

fn redact_message_for_view(
    record: &TranscriptTurnRecord,
    message: &TranscriptMessageRecord,
    view: TranscriptReplayView,
    attrs: Vec<TranscriptAttrEnvelope>,
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
            attr_id: None,
            attr_key: None,
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
        attrs,
        authority: message.authority,
        actor: message.actor.clone(),
        observed_at: message.observed_at,
        redacted: redaction_reason.is_some(),
    }
}

fn filter_attrs_for_target(
    record: &TranscriptTurnRecord,
    message_id: Option<&str>,
    attrs: &[TranscriptAttrEnvelope],
    view: TranscriptReplayView,
    redactions: &mut Vec<TranscriptRedactionReportItem>,
) -> Vec<TranscriptAttrEnvelope> {
    let mut visible = Vec::new();
    for attr in attrs {
        if attr.target.key != record.key || attr.target.turn_id != record.turn_id {
            continue;
        }
        let target_matches = match (attr.target.scope, message_id) {
            (TranscriptAttrScope::Turn, None) => attr.target.message_id.is_none(),
            (TranscriptAttrScope::Message, Some(message_id)) => {
                attr.target.message_id.as_deref() == Some(message_id)
            }
            _ => false,
        };
        if !target_matches {
            continue;
        }
        if attr.validate_for_record(record).is_err() {
            redactions.push(TranscriptRedactionReportItem {
                turn_id: record.turn_id.clone(),
                message_id: message_id.map(str::to_string),
                host_ref_index: None,
                attr_id: Some(attr.attr_id.clone()),
                attr_key: Some(attr.key.clone()),
                reason: TranscriptRedactionReason::AttrVisibility,
                source_authority: None,
                view,
            });
            continue;
        }
        if let Some(reason) = transcript_attr_redaction_reason(record, attr, view) {
            redactions.push(TranscriptRedactionReportItem {
                turn_id: record.turn_id.clone(),
                message_id: message_id.map(str::to_string),
                host_ref_index: None,
                attr_id: Some(attr.attr_id.clone()),
                attr_key: Some(attr.key.clone()),
                reason,
                source_authority: None,
                view,
            });
            continue;
        }
        visible.push(transcript_attr_for_lifecycle(record, attr, view));
    }
    visible
}

fn transcript_attr_for_lifecycle(
    record: &TranscriptTurnRecord,
    attr: &TranscriptAttrEnvelope,
    view: TranscriptReplayView,
) -> TranscriptAttrEnvelope {
    let lifecycle_redacted = matches!(
        record.redaction_state,
        TranscriptRedactionState::RawDeleted | TranscriptRedactionState::Masked
    ) || matches!(
        record.lifecycle_state,
        TranscriptLifecycleState::RawDeleted | TranscriptLifecycleState::Masked
    );
    if !lifecycle_redacted
        || attr.governance.redaction_policy
            != TranscriptAttrRedactionPolicy::OperatorAuditOnlyAfterMask
        || (view != TranscriptReplayView::OperatorAudit
            && view != TranscriptReplayView::RawOwnerOnly)
    {
        return attr.clone();
    }

    let mut redacted = attr.clone();
    redacted.value_kind = TranscriptAttrValueKind::JsonObject;
    redacted.schema_ref = Some("memory.transcript.attr.redacted.v1".to_string());
    redacted.value = serde_json::json!({
        "redacted": true,
        "reason": "transcript_lifecycle"
    });
    let value_bytes = serde_json::to_vec(&redacted.value)
        .map(|bytes| bytes.len())
        .unwrap_or(0);
    redacted.governance.max_value_bytes = redacted
        .governance
        .max_value_bytes
        .max(u32::try_from(value_bytes).unwrap_or(u32::MAX));
    redacted
}

fn transcript_attr_redaction_reason(
    record: &TranscriptTurnRecord,
    attr: &TranscriptAttrEnvelope,
    view: TranscriptReplayView,
) -> Option<TranscriptRedactionReason> {
    if !transcript_attr_visible_in_view(attr, view) {
        return Some(TranscriptRedactionReason::AttrVisibility);
    }
    let raw_deleted = matches!(record.redaction_state, TranscriptRedactionState::RawDeleted)
        || matches!(record.lifecycle_state, TranscriptLifecycleState::RawDeleted);
    if raw_deleted {
        match attr.governance.redaction_policy {
            TranscriptAttrRedactionPolicy::FollowTranscript
            | TranscriptAttrRedactionPolicy::MetadataSurvivesMask => {
                return Some(TranscriptRedactionReason::AttrLifecyclePolicy);
            }
            TranscriptAttrRedactionPolicy::OperatorAuditOnlyAfterMask => {
                if view != TranscriptReplayView::OperatorAudit
                    && view != TranscriptReplayView::RawOwnerOnly
                {
                    return Some(TranscriptRedactionReason::AttrLifecyclePolicy);
                }
            }
        }
    }
    if matches!(record.redaction_state, TranscriptRedactionState::Masked)
        || matches!(record.lifecycle_state, TranscriptLifecycleState::Masked)
    {
        match attr.governance.redaction_policy {
            TranscriptAttrRedactionPolicy::FollowTranscript => {
                return Some(TranscriptRedactionReason::AttrLifecyclePolicy);
            }
            TranscriptAttrRedactionPolicy::MetadataSurvivesMask => {}
            TranscriptAttrRedactionPolicy::OperatorAuditOnlyAfterMask => {
                if view != TranscriptReplayView::OperatorAudit
                    && view != TranscriptReplayView::RawOwnerOnly
                {
                    return Some(TranscriptRedactionReason::AttrLifecyclePolicy);
                }
            }
        }
    }
    None
}

fn transcript_attr_visible_in_view(
    attr: &TranscriptAttrEnvelope,
    view: TranscriptReplayView,
) -> bool {
    match view {
        TranscriptReplayView::RawOwnerOnly => true,
        TranscriptReplayView::ModelContext => attr.visibility == HostRefVisibility::ModelContext,
        TranscriptReplayView::HostUi => attr.visibility == HostRefVisibility::HostUi,
        TranscriptReplayView::OperatorAudit => matches!(
            attr.visibility,
            HostRefVisibility::HostUi
                | HostRefVisibility::OperatorAudit
                | HostRefVisibility::Export
        ),
        TranscriptReplayView::Export => {
            attr.visibility == HostRefVisibility::Export && attr.governance.export_allowed
        }
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
                    attr_id: None,
                    attr_key: None,
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
            attr_id: None,
            attr_key: None,
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
                    attr_id: None,
                    attr_key: None,
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
            attr_id: None,
            attr_key: None,
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
