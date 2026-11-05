//! Source-bound production persona evidence. Interpretation is not a Soul
//! revision or a host-editable persona ledger; lifecycle authority stays with
//! the existing Subject Soul generation and canonical Transcript owners.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    AssistantMessageSettlement, MemoryEvidenceAuthority, MemoryTurnDeliveryStatus,
    PostTurnGovernanceIdentityV2, SubjectSoulExpectedStateV1, SubjectSoulLifecycleStateV1,
    SubjectSoulOwnerV1, TranscriptTurnRecord, TurnContinuityEvidence, TurnLedgerStatus,
    TurnPersonaLedger, TurnPersonaPriorityLedger,
};
use crate::{Error, Result};

pub const MAX_PERSONA_OBSERVATION_REFS: usize = 8;
pub const MAX_PERSONA_OBSERVATION_QUOTE_BYTES: usize = 512;
pub const MAX_PERSONA_INTERPRETATION_BYTES: usize = 8192;

/// Ordinary growth and evidence collected during a source review are distinct
/// authorities; a new head cannot relabel a pending turn after the fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "purpose", rename_all = "snake_case", deny_unknown_fields)]
pub enum SoulTurnSourcePurposeV1 {
    Growth,
    SourceReview {
        basis: super::SubjectSoulSourceReviewBasisV1,
    },
}

/// Captured before execution in a Store-authenticated turn projection receipt.
/// Serialization alone grants no authority; the SDK verifies its outer receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoulTurnSourceAdmissionV1 {
    pub purpose: SoulTurnSourcePurposeV1,
    pub owner: SubjectSoulOwnerV1,
    pub basis: SubjectSoulExpectedStateV1,
}

impl SoulTurnSourceAdmissionV1 {
    pub fn generation(&self) -> Result<u64> {
        self.owner.validate_contract().map_err(|_| invalid())?;
        self.basis.validate_contract().map_err(|_| invalid())?;
        if let SoulTurnSourcePurposeV1::SourceReview { basis } = &self.purpose {
            super::SubjectSoulRevisionEvidenceV1 {
                source_lineage: Vec::new(),
                source_review: Some(basis.clone()),
                lawful_founding: None,
            }
            .validate(&self.owner)
            .map_err(|_| invalid())?;
            if matches!(
                self.basis,
                SubjectSoulExpectedStateV1::PristineAbsent { .. }
            ) {
                return Err(invalid());
            }
        }
        match self.basis {
            SubjectSoulExpectedStateV1::PristineAbsent { .. } => Ok(1),
            SubjectSoulExpectedStateV1::Exact {
                generation,
                lifecycle_state:
                    SubjectSoulLifecycleStateV1::Unseeded | SubjectSoulLifecycleStateV1::Active,
                ..
            } => Ok(generation),
            _ => Err(invalid()),
        }
    }

    /// Ordinary same-generation revisions do not revoke a pending source.
    /// Callers must CAS the newly verified current head at final commit.
    pub fn permits_current(&self, current: &Self) -> bool {
        self.owner == current.owner
            && self.purpose == current.purpose
            && self.generation().is_ok_and(|generation| {
                current
                    .generation()
                    .is_ok_and(|current_generation| current_generation == generation)
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPersonaInterpretationScopeV1 {
    Subject,
    ActiveRelationship,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaObservationRefV1 {
    pub message_id: String,
    pub quote: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum TurnPersonaInterpretationV1 {
    NoChange,
    OperationalOnly,
    Growth {
        scope: TurnPersonaInterpretationScopeV1,
        priority_order: Vec<String>,
        /// Subject meaning-frame evidence; never a governance priority domain.
        value_orientation: String,
        relationship_posture: String,
        supporting_observations: Vec<PersonaObservationRefV1>,
        counter_observations: Vec<PersonaObservationRefV1>,
    },
}

impl TurnPersonaInterpretationV1 {
    pub fn parse(raw: &str) -> Result<Self> {
        if raw.len() > MAX_PERSONA_INTERPRETATION_BYTES {
            return Err(invalid());
        }
        // No salvage of prose, partial JSON, defaults or authority fields.
        serde_json::from_str(raw).map_err(|_| invalid())
    }

    pub fn validate_against(
        &self,
        transcript: &TranscriptTurnRecord,
        relationship: Option<&str>,
    ) -> Result<()> {
        let Self::Growth {
            scope,
            priority_order,
            value_orientation,
            relationship_posture,
            supporting_observations,
            counter_observations,
        } = self
        else {
            return Ok(());
        };
        if !transcript.permits_post_turn_learning()
            || transcript.delivery_status != MemoryTurnDeliveryStatus::Delivered
            || transcript.external_content_used
            || transcript.assistant_messages.is_empty()
            || transcript.assistant_messages.iter().any(|entry| {
                entry.settlement != AssistantMessageSettlement::Complete
                    || entry.message.content.trim().is_empty()
            })
            || priority_order.len() > 8
            || priority_order.iter().any(|value| {
                super::persona_priority::canonical_priority_token(value).as_deref()
                    != Some(value.as_str())
            })
            || priority_order
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != priority_order.len()
            || (!relationship_posture.is_empty() && !text(relationship_posture, 256))
            || (!value_orientation.is_empty() && !text(value_orientation, 120))
            || (priority_order.is_empty()
                && value_orientation.is_empty()
                && relationship_posture.is_empty())
            || (*scope == TurnPersonaInterpretationScopeV1::ActiveRelationship
                && !value_orientation.is_empty())
            || (*scope == TurnPersonaInterpretationScopeV1::Subject
                && !relationship_posture.is_empty())
            || (*scope == TurnPersonaInterpretationScopeV1::ActiveRelationship
                && relationship.is_none_or(|id| !identifier(id)))
            || supporting_observations.is_empty()
            || supporting_observations
                .len()
                .saturating_add(counter_observations.len())
                > MAX_PERSONA_OBSERVATION_REFS
        {
            return Err(invalid());
        }
        let mut observed = std::collections::BTreeSet::new();
        for observation in supporting_observations.iter().chain(counter_observations) {
            if !identifier(&observation.message_id)
                || !text(&observation.quote, MAX_PERSONA_OBSERVATION_QUOTE_BYTES)
                || !observed.insert((&observation.message_id, &observation.quote))
                || !transcript.input_messages.iter().any(|message| {
                    message.message_id == observation.message_id
                        && matches!(
                            message.authority,
                            MemoryEvidenceAuthority::UserAsserted
                                | MemoryEvidenceAuthority::RuntimeObservation
                        )
                        && message.content.contains(&observation.quote)
                })
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

/// Protected generation-owned evidence. Never expose this body in operator
/// reports, Transcript UI replay/export, or public Soul metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedTurnPersonaEvidenceV1 {
    pub schema_version: u32,
    pub evidence_id: String,
    pub owner: SubjectSoulOwnerV1,
    pub generation: u64,
    pub source: PostTurnGovernanceIdentityV2,
    pub source_purpose: SoulTurnSourcePurposeV1,
    pub source_sequence: u64,
    pub source_transcript_digest: String,
    pub canonical_turn_digest: String,
    pub relationship_id: Option<String>,
    pub interpretation: TurnPersonaInterpretationV1,
    pub observed_at: u64,
    pub content_digest: String,
}

impl GovernedTurnPersonaEvidenceV1 {
    pub fn validate_contract(&self) -> Result<()> {
        self.owner.validate_contract().map_err(|_| invalid())?;
        self.source.validate()?;
        if let SoulTurnSourcePurposeV1::SourceReview { basis } = &self.source_purpose {
            super::SubjectSoulRevisionEvidenceV1 {
                source_lineage: Vec::new(),
                source_review: Some(basis.clone()),
                lawful_founding: None,
            }
            .validate(&self.owner)
            .map_err(|_| invalid())?;
        }
        if self.schema_version != 1
            || self.generation == 0
            || self.source_sequence == 0
            || self.observed_at == 0
            || self.source.memory_space_id != self.owner.memory_space_id
            || self.source.mounted_subject_id != self.owner.subject_id
            || !self.source_transcript_digest.starts_with("sha256:")
            || self.evidence_id != self.expected_id()?
            || self.content_digest != self.expected_digest()?
        {
            return Err(invalid());
        }
        let TurnPersonaInterpretationV1::Growth {
            scope,
            priority_order,
            value_orientation,
            relationship_posture,
            supporting_observations,
            counter_observations,
        } = &self.interpretation
        else {
            return Err(invalid());
        };
        if supporting_observations.is_empty()
            || supporting_observations
                .len()
                .saturating_add(counter_observations.len())
                > MAX_PERSONA_OBSERVATION_REFS
            || priority_order.len() > 8
            || priority_order.iter().any(|value| {
                super::persona_priority::canonical_priority_token(value).as_deref()
                    != Some(value.as_str())
            })
            || priority_order
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != priority_order.len()
            || (!value_orientation.is_empty() && !text(value_orientation, 120))
            || (priority_order.is_empty()
                && value_orientation.is_empty()
                && relationship_posture.is_empty())
            || (*scope == TurnPersonaInterpretationScopeV1::ActiveRelationship
                && !value_orientation.is_empty())
            || (!relationship_posture.is_empty() && !text(relationship_posture, 256))
            || supporting_observations
                .iter()
                .chain(counter_observations)
                .any(|observation| {
                    !identifier(&observation.message_id)
                        || !text(&observation.quote, MAX_PERSONA_OBSERVATION_QUOTE_BYTES)
                })
            || match scope {
                TurnPersonaInterpretationScopeV1::Subject => {
                    self.relationship_id.is_some() || !relationship_posture.is_empty()
                }
                TurnPersonaInterpretationScopeV1::ActiveRelationship => self
                    .relationship_id
                    .as_deref()
                    .is_none_or(|id| !identifier(id)),
            }
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn bind(
        admission: &SoulTurnSourceAdmissionV1,
        source: PostTurnGovernanceIdentityV2,
        transcript: &TranscriptTurnRecord,
        relationship: Option<String>,
        interpretation: TurnPersonaInterpretationV1,
    ) -> Result<Option<Self>> {
        let generation = admission.generation()?;
        source.validate()?;
        if admission.owner.memory_space_id != transcript.key.memory_space_id
            || admission.owner.subject_id != transcript.subject
            || source.memory_space_id != transcript.key.memory_space_id
            || source.mounted_subject_id != transcript.subject
            || source.channel_id != transcript.key.channel_id
            || source.conversation_id != transcript.key.conversation_id
            || source.turn_id != transcript.turn_id
            || !transcript.permits_post_turn_learning()
            || transcript.sequence == 0
            || transcript.created_at == 0
        {
            return Err(invalid());
        }
        interpretation.validate_against(transcript, relationship.as_deref())?;
        if !matches!(interpretation, TurnPersonaInterpretationV1::Growth { .. }) {
            return Ok(None);
        }
        let relationship_id = match &interpretation {
            TurnPersonaInterpretationV1::Growth {
                scope: TurnPersonaInterpretationScopeV1::ActiveRelationship,
                ..
            } => relationship,
            _ => None,
        };
        let mut evidence = Self {
            schema_version: 1,
            evidence_id: String::new(),
            owner: admission.owner.clone(),
            generation,
            source,
            source_purpose: admission.purpose.clone(),
            source_sequence: transcript.sequence,
            source_transcript_digest: super::SubjectSoulSourceRefV1::from_transcript(transcript)
                .map_err(|_| invalid())?
                .transcript_content_digest,
            canonical_turn_digest: transcript.canonical_turn_digest.clone(),
            relationship_id,
            interpretation,
            observed_at: transcript.created_at,
            content_digest: String::new(),
        };
        // Neither model output, lease epoch, attempt nor model identity changes
        // an observation's key. A retry cannot become another independent turn.
        evidence.evidence_id = evidence.expected_id()?;
        evidence.content_digest = evidence.expected_digest()?;
        evidence.validate_contract()?;
        Ok(Some(evidence))
    }

    fn expected_id(&self) -> Result<String> {
        digest(
            "soul_turn_evidence_identity_v1",
            &(&self.owner, self.generation, &self.source),
        )
    }

    fn expected_digest(&self) -> Result<String> {
        let mut value = self.clone();
        value.content_digest.clear();
        digest("soul_turn_evidence_content_v1", &value)
    }

    pub fn validate_source(
        &self,
        admission: &SoulTurnSourceAdmissionV1,
        transcript: &TranscriptTurnRecord,
    ) -> Result<()> {
        let rebound = Self::bind(
            admission,
            self.source.clone(),
            transcript,
            self.relationship_id.clone(),
            self.interpretation.clone(),
        )?
        .ok_or_else(invalid)?;
        if self != &rebound {
            return Err(invalid());
        }
        Ok(())
    }

    /// Produces the existing continuity consumer's view only after the caller
    /// has checked the current source and generation; never persists a second ledger.
    pub fn continuity_view(
        &self,
        admission: &SoulTurnSourceAdmissionV1,
        transcript: &TranscriptTurnRecord,
        scope: super::PersonalityRelationshipScope<'_>,
    ) -> Result<Option<TurnContinuityEvidence>> {
        self.validate_source(admission, transcript)?;
        let TurnPersonaInterpretationV1::Growth {
            priority_order,
            value_orientation,
            relationship_posture,
            counter_observations,
            scope: evidence_scope,
            ..
        } = &self.interpretation
        else {
            return Err(invalid());
        };
        match (scope, evidence_scope) {
            (
                super::PersonalityRelationshipScope::SubjectOnly,
                TurnPersonaInterpretationScopeV1::Subject,
            ) => {}
            (
                super::PersonalityRelationshipScope::Active(id),
                TurnPersonaInterpretationScopeV1::ActiveRelationship,
            ) if self.relationship_id.as_deref() == Some(id) => {}
            (super::PersonalityRelationshipScope::Unverified, _) => return Err(invalid()),
            _ => return Ok(None),
        }
        Ok(Some(TurnContinuityEvidence {
            growth_counterevidence: !counter_observations.is_empty(),
            subject_value_orientation: value_orientation.clone(),
            ingress: transcript.source.ingress,
            status: TurnLedgerStatus::Answered,
            final_reply_delivered: true,
            canonical_reply_source: transcript
                .assistant_messages
                .last()
                .ok_or_else(invalid)?
                .message
                .message_id
                .clone(),
            observed_at_ms: self.observed_at.checked_mul(1000).ok_or_else(invalid)?,
            persona: Some(TurnPersonaLedger {
                priority: Some(TurnPersonaPriorityLedger {
                    priority_order: priority_order.clone(),
                    relationship_posture: relationship_posture.clone(),
                    ..Default::default()
                }),
                reply_delivered: true,
                ..Default::default()
            }),
        }))
    }
}

fn digest<T: Serialize>(domain: &str, value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    let mut hash = Sha256::new();
    hash.update((domain.len() as u64).to_be_bytes());
    hash.update(domain.as_bytes());
    hash.update(bytes);
    Ok(format!("sha256:{:x}", hash.finalize()))
}
fn identifier(value: &str) -> bool {
    text(value, 256) && !value.chars().any(char::is_control)
}
fn text(value: &str, limit: usize) -> bool {
    value.len() <= limit && crate::util::is_canonical_evidence_text(value)
}
fn invalid() -> Error {
    Error::invalid_input(
        "turn_persona_learning",
        "persona source, interpretation or generation admission is invalid",
    )
}

#[cfg(test)]
#[path = "turn_persona_learning_tests.rs"]
mod tests;
