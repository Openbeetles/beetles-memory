//! Canonical public contracts for post-turn procedural learning.
//!
//! These types intentionally carry no Store or worker behavior. PFI1-A freezes the
//! host-neutral evidence boundary before later substages attach it to Transcript and jobs.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::memory::{GovernedMemoryOwnerPlane, GovernedOwnerRevisionRef};
use crate::skills::{
    AgentSkillScope, AgentSkillTrust, AgentToolObservationDigest, AgentToolRegistryRef,
    AgentToolRegistryScope, RuntimeSkillOwnerBinding, RuntimeSkillOwnerLocator,
    RuntimeSkillOwningScope,
};

pub const PROCEDURAL_APPLICABILITY_CONTEXT_SCHEMA_VERSION: u32 = 1;
pub const PROCEDURAL_SELECTION_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION: u32 = 1;
pub const PROCEDURAL_FEEDBACK_JOB_SCHEMA_VERSION: u32 = 1;
pub const PROCEDURAL_FEEDBACK_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const PROCEDURAL_FEEDBACK_SCOPE_INDEX_SCHEMA_VERSION: u32 = 1;
pub const PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_SCHEMA_VERSION: u32 = 1;
pub const MAX_PROCEDURAL_FEEDBACK_ACTIVE_JOBS: usize = 256;
pub const MAX_PROCEDURAL_FEEDBACK_RECENT_TERMINAL_JOBS: usize = 256;
pub const MAX_PROCEDURAL_FEEDBACK_RECONCILIATION_CURSORS: usize = 256;

const APPLICABILITY_DIGEST_DOMAIN: &str = "procedural_applicability_context_digest_v1";
const SELECTION_DIGEST_DOMAIN: &str = "procedural_selection_digest_v1";
const LEARNING_EVIDENCE_DIGEST_DOMAIN: &str = "post_turn_learning_evidence_digest_v1";
const PROCEDURAL_JOB_ID_DOMAIN: &str = "procedural_feedback_job_id_v1";
const PROCEDURAL_SCOPE_ID_DOMAIN: &str = "procedural_feedback_scope_id_v1";
const RECEIPT_DIGEST_DOMAIN: &str = "procedural_selection_receipt_ref_v1";
const PROCEDURAL_APPLICATION_DIGEST_DOMAIN: &str = "procedural_feedback_application_digest_v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralProjectionBindingV1 {
    Preview,
    Turn { turn_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralFeedbackAuthorityInputV1 {
    HostRuntimeObservation,
    ModelInferred,
    HumanConfirmed { operation_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostTurnLearningInputV1 {
    pub tool_call_count: u32,
    pub selection_receipt: Option<ProceduralSelectionReceiptV1>,
    pub runtime_skill_feedback: Vec<RuntimeSkillUsageFeedbackV1>,
    pub agent_skill_feedback: Vec<AgentSkillUsageFeedbackV1>,
    pub task_learning_feedback: Vec<TaskLearningUsageFeedbackV1>,
    pub agent_tool_feedback: Vec<AgentToolUsageFeedbackV2>,
    pub authority: ProceduralFeedbackAuthorityInputV1,
}

impl PostTurnLearningInputV1 {
    pub fn empty() -> Self {
        Self::with_tool_call_count(0)
    }

    pub fn with_tool_call_count(tool_call_count: u32) -> Self {
        Self {
            tool_call_count,
            selection_receipt: None,
            runtime_skill_feedback: Vec::new(),
            agent_skill_feedback: Vec::new(),
            task_learning_feedback: Vec::new(),
            agent_tool_feedback: Vec::new(),
            authority: ProceduralFeedbackAuthorityInputV1::HostRuntimeObservation,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralApplicabilityContextV1 {
    pub schema_version: u32,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub conversation_id: Option<String>,
    pub context_digest: String,
}

impl ProceduralApplicabilityContextV1 {
    pub fn try_new(
        project_id: Option<String>,
        workspace_id: Option<String>,
        conversation_id: Option<String>,
    ) -> Result<Self> {
        if [&project_id, &workspace_id, &conversation_id]
            .into_iter()
            .flatten()
            .any(|value| !is_canonical(value))
        {
            return Err(Error::config(
                "procedural_applicability_context",
                "applicability identifiers must be canonical",
            ));
        }
        let mut context = Self {
            schema_version: PROCEDURAL_APPLICABILITY_CONTEXT_SCHEMA_VERSION,
            project_id,
            workspace_id,
            conversation_id,
            context_digest: String::new(),
        };
        context.context_digest = context.canonical_digest()?;
        Ok(context)
    }

    pub fn canonical_digest(&self) -> Result<String> {
        let encoded = serde_json::to_vec(&(
            self.schema_version,
            &self.project_id,
            &self.workspace_id,
            &self.conversation_id,
        ))
        .map_err(|error| Error::config("procedural_applicability_digest", error.to_string()))?;
        Ok(domain_digest(APPLICABILITY_DIGEST_DOMAIN, &[&encoded]))
    }

    pub fn validate_contract(&self) -> bool {
        self.schema_version == PROCEDURAL_APPLICABILITY_CONTEXT_SCHEMA_VERSION
            && [&self.project_id, &self.workspace_id, &self.conversation_id]
                .into_iter()
                .flatten()
                .all(|value| is_canonical(value))
            && self
                .canonical_digest()
                .is_ok_and(|value| value == self.context_digest)
    }

    pub fn permits_registry_scope(&self, scope: &AgentToolRegistryScope) -> bool {
        match scope {
            AgentToolRegistryScope::Global | AgentToolRegistryScope::Owner => true,
            AgentToolRegistryScope::Project { project_id } => {
                self.project_id.as_deref() == Some(project_id.as_str())
            }
            AgentToolRegistryScope::Workspace { workspace_id } => {
                self.workspace_id.as_deref() == Some(workspace_id.as_str())
            }
            AgentToolRegistryScope::Conversation { conversation_id } => {
                self.conversation_id.as_deref() == Some(conversation_id.as_str())
            }
        }
    }

    pub fn permits_agent_skill_scope(&self, scope: &crate::skills::AgentSkillScope) -> bool {
        use crate::skills::AgentSkillScope;
        self.validate_contract()
            && match scope {
                AgentSkillScope::Global | AgentSkillScope::Owner => true,
                AgentSkillScope::Project { project_id } => {
                    self.project_id.as_deref() == Some(project_id)
                }
                AgentSkillScope::Workspace { workspace_id } => {
                    self.workspace_id.as_deref() == Some(workspace_id)
                }
                AgentSkillScope::Conversation { conversation_id } => {
                    self.conversation_id.as_deref() == Some(conversation_id)
                }
            }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralProjectionIdentityV1 {
    pub projection_id: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel: String,
    pub chat_id: String,
    pub conversation_id: String,
    pub turn_id: String,
}

impl ProceduralProjectionIdentityV1 {
    pub fn validate_contract(&self) -> bool {
        [
            &self.projection_id,
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.channel,
            &self.chat_id,
            &self.conversation_id,
            &self.turn_id,
        ]
        .into_iter()
        .all(|value| is_canonical(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardAgentSkillSelectionV1 {
    pub package_id: String,
    pub package_fingerprint: String,
    pub package_binding: String,
    pub scope: AgentSkillScope,
    pub trust: AgentSkillTrust,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskLearningSelectionV1 {
    pub learning_id: String,
    pub learning_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillSelectionV1 {
    pub locator: RuntimeSkillOwnerLocator,
    pub content_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolExperienceSelectionV1 {
    pub registry_ref: AgentToolRegistryRef,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub experience_owner_id: String,
    pub experience_revision: u64,
    pub experience_content_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralSelectionReceiptV1 {
    pub schema_version: u32,
    pub receipt_ref: String,
    pub signing_key_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub identity: ProceduralProjectionIdentityV1,
    pub applicability: ProceduralApplicabilityContextV1,
    pub standard_agent_skills: Vec<StandardAgentSkillSelectionV1>,
    pub runtime_skills: Vec<RuntimeSkillSelectionV1>,
    pub task_learnings: Vec<TaskLearningSelectionV1>,
    pub agent_tool_experiences: Vec<AgentToolExperienceSelectionV1>,
    pub selection_digest: String,
    pub delivery_digest: String,
    pub authority_tag: String,
}

impl ProceduralSelectionReceiptV1 {
    pub fn validate_contract(&self) -> bool {
        self.schema_version == PROCEDURAL_SELECTION_RECEIPT_SCHEMA_VERSION
            && is_prefixed_digest(&self.receipt_ref, "procedural_selection_receipt:sha256:")
            && is_canonical(&self.signing_key_id)
            && self.issued_at > 0
            && self.expires_at > self.issued_at
            && self.identity.validate_contract()
            && self.applicability.validate_contract()
            && is_digest(&self.selection_digest)
            && is_digest(&self.delivery_digest)
            && is_digest(&self.authority_tag)
            && self.standard_agent_skills.iter().all(|value| {
                is_canonical(&value.package_id)
                    && is_canonical(&value.package_fingerprint)
                    && is_digest(&value.package_binding)
                    && self.applicability.permits_agent_skill_scope(&value.scope)
            })
            && self.runtime_skills.iter().all(|value| {
                is_digest(&value.content_digest)
                    && value.locator.validate_for(&self.identity.memory_space_id)
                    && match value.locator.owning_scope() {
                        RuntimeSkillOwningScope::SharedProgram => true,
                        RuntimeSkillOwningScope::Subject { mounted_subject_id } => {
                            mounted_subject_id == &self.identity.mounted_subject_id
                        }
                    }
            })
            && self
                .task_learnings
                .iter()
                .all(|value| is_canonical(&value.learning_id) && is_digest(&value.learning_digest))
            && self.agent_tool_experiences.iter().all(|value| {
                is_canonical(&value.registry_ref.registry_id)
                    && is_canonical(&value.registry_ref.fingerprint)
                    && self
                        .applicability
                        .permits_registry_scope(&value.registry_ref.scope)
                    && is_canonical(&value.tool_id)
                    && is_canonical(&value.schema_fingerprint)
                    && is_prefixed_digest(
                        &value.experience_owner_id,
                        "agent_tool_experience:sha256:",
                    )
                    && value.experience_revision > 0
                    && is_digest(&value.experience_content_digest)
            })
            && canonical_order(&self.standard_agent_skills)
            && canonical_order(&self.runtime_skills)
            && canonical_order(&self.task_learnings)
            && canonical_order(&self.agent_tool_experiences)
            && self
                .canonical_selection_digest()
                .is_ok_and(|value| value == self.selection_digest)
            && self
                .canonical_receipt_ref()
                .is_ok_and(|value| value == self.receipt_ref)
    }

    pub fn canonical_selection_digest(&self) -> Result<String> {
        let encoded = serde_json::to_vec(&(
            &self.identity,
            &self.applicability,
            &self.standard_agent_skills,
            &self.runtime_skills,
            &self.task_learnings,
            &self.agent_tool_experiences,
        ))
        .map_err(|error| Error::config("procedural_selection_digest", error.to_string()))?;
        Ok(domain_digest(SELECTION_DIGEST_DOMAIN, &[&encoded]))
    }

    pub fn canonical_receipt_ref(&self) -> Result<String> {
        let encoded = serde_json::to_vec(&(
            self.schema_version,
            &self.signing_key_id,
            self.issued_at,
            self.expires_at,
            &self.identity,
            &self.applicability,
            &self.selection_digest,
            &self.delivery_digest,
        ))
        .map_err(|error| Error::config("procedural_selection_receipt", error.to_string()))?;
        Ok(format!(
            "procedural_selection_receipt:sha256:{}",
            domain_hex(RECEIPT_DIGEST_DOMAIN, &[&encoded])
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralFeedbackAuthorityV1 {
    HostRuntimeObservation,
    ModelInferred,
    HumanConfirmed {
        actor_subject_id: String,
        operation_id: String,
        evidence_digest: String,
    },
}

impl ProceduralFeedbackAuthorityV1 {
    pub fn validate_contract(&self) -> bool {
        match self {
            Self::HostRuntimeObservation | Self::ModelInferred => true,
            Self::HumanConfirmed {
                actor_subject_id,
                operation_id,
                evidence_digest,
            } => {
                is_canonical(actor_subject_id)
                    && is_canonical(operation_id)
                    && is_digest(evidence_digest)
            }
        }
    }

    pub const fn is_human_confirmation(&self) -> bool {
        matches!(self, Self::HumanConfirmed { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralExecutionOutcomeV1 {
    Succeeded,
    Mismatch,
    Failed,
    Partial,
    Cancelled,
    NotExecuted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillUsageFeedbackV1 {
    pub locator: RuntimeSkillOwnerLocator,
    pub selected_content_digest: String,
    pub outcome: ProceduralExecutionOutcomeV1,
    pub observation_ref: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSkillUsageFeedbackV1 {
    pub package_id: String,
    pub package_fingerprint: String,
    pub package_binding: String,
    pub outcome: ProceduralExecutionOutcomeV1,
    pub observation_ref: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskLearningUsageFeedbackV1 {
    pub learning_id: String,
    pub learning_digest: String,
    pub outcome: ProceduralExecutionOutcomeV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolUsageFeedbackV2 {
    pub registry_ref: AgentToolRegistryRef,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub observations: Vec<AgentToolObservationDigest>,
    pub outcome: ProceduralExecutionOutcomeV1,
    pub user_visible_result_summary: Option<String>,
    pub operator_note: Option<String>,
}

impl AgentToolUsageFeedbackV2 {
    pub fn validate_contract(&self) -> bool {
        is_canonical(&self.registry_ref.registry_id)
            && is_canonical(&self.registry_ref.fingerprint)
            && is_canonical(&self.tool_id)
            && is_canonical(&self.schema_fingerprint)
            && !self.observations.is_empty()
            && self.observations.iter().all(|value| {
                is_canonical(&value.observation_id)
                    && value.registry_id == self.registry_ref.registry_id
                    && value.tool_id == self.tool_id
                    && value.schema_fingerprint == self.schema_fingerprint
                    && is_canonical(&value.task_signature)
                    && is_canonical_text(&value.summary)
            })
            && canonical_order(&self.observations)
            && self
                .user_visible_result_summary
                .as_ref()
                .is_none_or(|value| is_canonical_text(value))
            && self
                .operator_note
                .as_ref()
                .is_none_or(|value| is_canonical_text(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostTurnLearningEvidenceV1 {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub conversation_id: String,
    pub turn_id: String,
    pub canonical_turn_digest: String,
    pub tool_call_count: u32,
    pub selection_receipt: Option<ProceduralSelectionReceiptV1>,
    pub runtime_skill_feedback: Vec<RuntimeSkillUsageFeedbackV1>,
    pub agent_skill_feedback: Vec<AgentSkillUsageFeedbackV1>,
    pub task_learning_feedback: Vec<TaskLearningUsageFeedbackV1>,
    pub agent_tool_feedback: Vec<AgentToolUsageFeedbackV2>,
    pub authority: ProceduralFeedbackAuthorityV1,
    pub learning_evidence_digest: String,
}

impl PostTurnLearningEvidenceV1 {
    pub fn canonical_digest(&self) -> Result<String> {
        let encoded = serde_json::to_vec(&(
            self.schema_version,
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.conversation_id,
            &self.turn_id,
            &self.canonical_turn_digest,
            self.tool_call_count,
            &self.selection_receipt,
            &self.runtime_skill_feedback,
            &self.agent_skill_feedback,
            &self.task_learning_feedback,
            &self.agent_tool_feedback,
            &self.authority,
        ))
        .map_err(|error| Error::config("post_turn_learning_evidence_digest", error.to_string()))?;
        Ok(domain_digest(LEARNING_EVIDENCE_DIGEST_DOMAIN, &[&encoded]))
    }

    /// The confirmation binds the canonical fact payload, not its own authority claim.
    pub fn canonical_confirmation_payload_digest(&self) -> Result<String> {
        let encoded = serde_json::to_vec(&(
            self.schema_version,
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.conversation_id,
            &self.turn_id,
            &self.canonical_turn_digest,
            self.tool_call_count,
            &self.selection_receipt,
            &self.runtime_skill_feedback,
            &self.agent_skill_feedback,
            &self.task_learning_feedback,
            &self.agent_tool_feedback,
        ))
        .map_err(|error| Error::config("procedural_confirmation_digest", error.to_string()))?;
        Ok(domain_digest(
            "procedural_confirmation_payload_digest_v1",
            &[&encoded],
        ))
    }

    fn selection_contains_feedback(&self) -> bool {
        let Some(receipt) = &self.selection_receipt else {
            return self.runtime_skill_feedback.is_empty()
                && self.agent_skill_feedback.is_empty()
                && self.task_learning_feedback.is_empty();
        };
        receipt.validate_contract()
            && receipt.identity.memory_space_id == self.memory_space_id
            && receipt.identity.mounted_subject_id == self.mounted_subject_id
            && receipt.identity.conversation_id == self.conversation_id
            && receipt.identity.turn_id == self.turn_id
            && self.runtime_skill_feedback.iter().all(|feedback| {
                receipt.runtime_skills.iter().any(|selected| {
                    selected.locator == feedback.locator
                        && selected.content_digest == feedback.selected_content_digest
                })
            })
            && self.agent_skill_feedback.iter().all(|feedback| {
                receipt.standard_agent_skills.iter().any(|selected| {
                    selected.package_id == feedback.package_id
                        && selected.package_fingerprint == feedback.package_fingerprint
                        && selected.package_binding == feedback.package_binding
                })
            })
            && self.task_learning_feedback.iter().all(|feedback| {
                receipt.task_learnings.iter().any(|selected| {
                    selected.learning_id == feedback.learning_id
                        && selected.learning_digest == feedback.learning_digest
                })
            })
    }

    pub fn validate_contract(&self) -> bool {
        self.schema_version == POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION
            && [
                &self.memory_space_id,
                &self.mounted_subject_id,
                &self.conversation_id,
                &self.turn_id,
            ]
            .into_iter()
            .all(|value| is_canonical(value))
            && is_digest(&self.canonical_turn_digest)
            && self.selection_contains_feedback()
            && self.authority.validate_contract()
            && match &self.authority {
                ProceduralFeedbackAuthorityV1::HumanConfirmed {
                    evidence_digest, ..
                } => self
                    .canonical_confirmation_payload_digest()
                    .is_ok_and(|digest| &digest == evidence_digest),
                _ => true,
            }
            && self.runtime_skill_feedback.iter().all(|value| {
                is_digest(&value.selected_content_digest) && is_canonical(&value.observation_ref)
            })
            && self.agent_skill_feedback.iter().all(|value| {
                is_canonical(&value.package_id)
                    && is_canonical(&value.package_fingerprint)
                    && is_digest(&value.package_binding)
                    && is_canonical(&value.observation_ref)
            })
            && self
                .task_learning_feedback
                .iter()
                .all(|value| is_canonical(&value.learning_id) && is_digest(&value.learning_digest))
            && self
                .agent_tool_feedback
                .iter()
                .all(AgentToolUsageFeedbackV2::validate_contract)
            && canonical_order(&self.runtime_skill_feedback)
            && canonical_order(&self.agent_skill_feedback)
            && canonical_order(&self.task_learning_feedback)
            && canonical_order(&self.agent_tool_feedback)
            && self
                .canonical_digest()
                .is_ok_and(|value| value == self.learning_evidence_digest)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackIdentityV1 {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: String,
    pub chat_id: String,
    pub conversation_id: String,
    pub turn_id: String,
}

impl ProceduralFeedbackIdentityV1 {
    pub fn new(
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel_id: &str,
        chat_id: &str,
        conversation_id: &str,
        turn_id: &str,
    ) -> Result<Self> {
        let identity = Self {
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: mounted_subject_id.to_string(),
            channel_id: channel_id.to_string(),
            chat_id: chat_id.to_string(),
            conversation_id: conversation_id.to_string(),
            turn_id: turn_id.to_string(),
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<()> {
        if [
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.channel_id,
            &self.chat_id,
            &self.conversation_id,
            &self.turn_id,
        ]
        .into_iter()
        .all(|value| is_canonical(value))
        {
            Ok(())
        } else {
            Err(Error::invalid_input(
                "procedural_feedback_identity",
                "procedural feedback identity must be canonical",
            ))
        }
    }

    pub fn scope_id(&self) -> String {
        format!(
            "procedural_feedback_scope:sha256:{}",
            domain_hex(
                PROCEDURAL_SCOPE_ID_DOMAIN,
                &[
                    self.memory_space_id.as_bytes(),
                    self.mounted_subject_id.as_bytes(),
                    self.channel_id.as_bytes(),
                    self.chat_id.as_bytes(),
                ],
            )
        )
    }

    pub fn job_id(&self, learning_evidence_digest: &str) -> Result<String> {
        if !is_digest(learning_evidence_digest) {
            return Err(Error::invalid_input(
                "procedural_feedback_identity",
                "learning evidence digest must be canonical",
            ));
        }
        Ok(format!(
            "procedural_feedback_job:sha256:{}",
            domain_hex(
                PROCEDURAL_JOB_ID_DOMAIN,
                &[
                    self.memory_space_id.as_bytes(),
                    self.mounted_subject_id.as_bytes(),
                    self.channel_id.as_bytes(),
                    self.chat_id.as_bytes(),
                    self.conversation_id.as_bytes(),
                    self.turn_id.as_bytes(),
                    learning_evidence_digest.as_bytes(),
                ],
            )
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralFeedbackJobStatusV1 {
    Pending,
    Leased,
    RetryWaiting,
    Succeeded,
    Cancelled,
    RepairRequired,
    DeadLetter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralFeedbackErrorClassV1 {
    StoreUnavailable,
    CasConflict,
    BudgetExceeded,
    RegistryUnavailable,
    ClosureRejected,
    EvidenceUnavailable,
}

impl ProceduralFeedbackErrorClassV1 {
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::StoreUnavailable
                | Self::CasConflict
                | Self::BudgetExceeded
                | Self::RegistryUnavailable
                | Self::EvidenceUnavailable
        )
    }
}

impl ProceduralFeedbackJobStatusV1 {
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Leased | Self::RetryWaiting)
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Cancelled | Self::RepairRequired | Self::DeadLetter
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackJobV1 {
    pub schema_version: u32,
    pub job_id: String,
    pub idempotency_key: String,
    pub scope_index_key: String,
    pub identity: ProceduralFeedbackIdentityV1,
    pub transcript_sequence: u64,
    pub transcript_digest: String,
    pub learning_evidence_digest: String,
    pub submitted_count: u32,
    pub status: ProceduralFeedbackJobStatusV1,
    pub state_revision: u64,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub next_attempt_at: Option<u64>,
    pub lease_owner: Option<String>,
    pub lease_until: Option<u64>,
    pub lease_epoch: u64,
    pub last_error_class: Option<ProceduralFeedbackErrorClassV1>,
    pub receipt: Option<ProceduralFeedbackReceiptV1>,
    pub created_at: u64,
    pub updated_at: u64,
    pub terminal_at: Option<u64>,
}

impl ProceduralFeedbackJobV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn pending(
        identity: ProceduralFeedbackIdentityV1,
        transcript_sequence: u64,
        transcript_digest: impl Into<String>,
        learning_evidence_digest: impl Into<String>,
        submitted_count: u32,
        max_attempts: u32,
        now_secs: u64,
    ) -> Result<Self> {
        identity.validate()?;
        let transcript_digest = transcript_digest.into();
        let learning_evidence_digest = learning_evidence_digest.into();
        let job_id = identity.job_id(&learning_evidence_digest)?;
        let job = Self {
            schema_version: PROCEDURAL_FEEDBACK_JOB_SCHEMA_VERSION,
            idempotency_key: job_id.clone(),
            scope_index_key: identity.scope_id(),
            job_id,
            identity,
            transcript_sequence,
            transcript_digest,
            learning_evidence_digest,
            submitted_count,
            status: ProceduralFeedbackJobStatusV1::Pending,
            state_revision: 1,
            attempt_count: 0,
            max_attempts,
            next_attempt_at: Some(now_secs),
            lease_owner: None,
            lease_until: None,
            lease_epoch: 0,
            last_error_class: None,
            receipt: None,
            created_at: now_secs,
            updated_at: now_secs,
            terminal_at: None,
        };
        job.validate()?;
        Ok(job)
    }

    pub fn validate(&self) -> Result<()> {
        self.identity.validate()?;
        let lease_fields_present = self.lease_owner.is_some() && self.lease_until.is_some();
        let state_timing_valid = match self.status {
            ProceduralFeedbackJobStatusV1::Pending
            | ProceduralFeedbackJobStatusV1::RetryWaiting => {
                self.next_attempt_at.is_some() && !lease_fields_present
            }
            ProceduralFeedbackJobStatusV1::Leased => {
                self.next_attempt_at.is_none() && lease_fields_present
            }
            status if status.is_terminal() => {
                self.next_attempt_at.is_none() && !lease_fields_present
            }
            _ => false,
        };
        let receipt_valid = match (&self.status, &self.receipt) {
            (ProceduralFeedbackJobStatusV1::Succeeded, Some(receipt)) => {
                receipt.job_id == self.job_id
                    && receipt.transcript_digest == self.transcript_digest
                    && receipt.learning_evidence_digest == self.learning_evidence_digest
                    && receipt.submitted_count == self.submitted_count
                    && receipt.validate_contract()
            }
            (ProceduralFeedbackJobStatusV1::Succeeded, None) => false,
            (_, None) => true,
            (_, Some(_)) => false,
        };
        if self.schema_version != PROCEDURAL_FEEDBACK_JOB_SCHEMA_VERSION
            || self.job_id != self.identity.job_id(&self.learning_evidence_digest)?
            || self.idempotency_key != self.job_id
            || self.scope_index_key != self.identity.scope_id()
            || self.transcript_sequence == 0
            || !is_digest(&self.transcript_digest)
            || !is_digest(&self.learning_evidence_digest)
            || self.submitted_count == 0
            || self.state_revision == 0
            || self.max_attempts == 0
            || self.attempt_count > self.max_attempts
            || self.created_at == 0
            || self.updated_at < self.created_at
            || !state_timing_valid
            || self.status.is_terminal() != self.terminal_at.is_some()
            || !receipt_valid
            || self
                .lease_owner
                .as_deref()
                .is_some_and(|value| !is_canonical(value))
        {
            return Err(Error::invalid_input(
                "procedural_feedback_job",
                "job identity, state, lease, receipt, or timing is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackJobRefV1 {
    pub job_id: String,
    pub status: ProceduralFeedbackJobStatusV1,
    pub state_revision: u64,
    pub transcript_sequence: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

impl ProceduralFeedbackJobRefV1 {
    pub fn from_job(job: &ProceduralFeedbackJobV1) -> Self {
        Self {
            job_id: job.job_id.clone(),
            status: job.status,
            state_revision: job.state_revision,
            transcript_sequence: job.transcript_sequence,
            created_at: job.created_at,
            updated_at: job.updated_at,
        }
    }

    pub fn validate(&self) -> bool {
        self.job_id.starts_with("procedural_feedback_job:sha256:")
            && self.state_revision > 0
            && self.transcript_sequence > 0
            && self.created_at > 0
            && self.updated_at >= self.created_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackReconciliationCursorV1 {
    pub conversation_id: String,
    pub sequence: u64,
    pub turn_id: String,
    pub updated_at: u64,
}

impl ProceduralFeedbackReconciliationCursorV1 {
    pub fn validate(&self) -> bool {
        is_canonical(&self.conversation_id)
            && self.sequence > 0
            && is_canonical(&self.turn_id)
            && self.updated_at > 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackScopeIndexV1 {
    pub schema_version: u32,
    pub scope_index_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: String,
    pub chat_id: String,
    pub index_revision: u64,
    pub active_jobs: Vec<ProceduralFeedbackJobRefV1>,
    pub recent_terminal_jobs: Vec<ProceduralFeedbackJobRefV1>,
    pub reconciliation_cursors: Vec<ProceduralFeedbackReconciliationCursorV1>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl ProceduralFeedbackScopeIndexV1 {
    pub fn empty(identity: &ProceduralFeedbackIdentityV1, now_secs: u64) -> Self {
        Self {
            schema_version: PROCEDURAL_FEEDBACK_SCOPE_INDEX_SCHEMA_VERSION,
            scope_index_key: identity.scope_id(),
            memory_space_id: identity.memory_space_id.clone(),
            mounted_subject_id: identity.mounted_subject_id.clone(),
            channel_id: identity.channel_id.clone(),
            chat_id: identity.chat_id.clone(),
            index_revision: 1,
            active_jobs: Vec::new(),
            recent_terminal_jobs: Vec::new(),
            reconciliation_cursors: Vec::new(),
            created_at: now_secs,
            updated_at: now_secs,
        }
    }

    pub fn reconciliation_cursor(
        &self,
        conversation_id: &str,
    ) -> Option<&ProceduralFeedbackReconciliationCursorV1> {
        self.reconciliation_cursors
            .iter()
            .find(|cursor| cursor.conversation_id == conversation_id)
    }

    pub fn set_reconciliation_cursor(
        &mut self,
        cursor: ProceduralFeedbackReconciliationCursorV1,
    ) -> Result<()> {
        if !cursor.validate() {
            return Err(Error::invalid_input(
                "procedural_feedback_reconcile",
                "reconciliation cursor must be canonical",
            ));
        }
        if let Some(existing) = self
            .reconciliation_cursors
            .iter_mut()
            .find(|existing| existing.conversation_id == cursor.conversation_id)
        {
            if cursor.sequence <= existing.sequence {
                return Err(Error::conflict(
                    "procedural_feedback_reconcile",
                    "reconciliation cursor must advance monotonically",
                ));
            }
            *existing = cursor;
        } else {
            if self.reconciliation_cursors.len() >= MAX_PROCEDURAL_FEEDBACK_RECONCILIATION_CURSORS {
                return Err(Error::config(
                    "procedural_feedback_reconcile",
                    "runtime scope reconciliation cursor budget is exhausted",
                ));
            }
            self.reconciliation_cursors.push(cursor);
            self.reconciliation_cursors
                .sort_by(|left, right| left.conversation_id.cmp(&right.conversation_id));
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let jobs_canonical = |jobs: &[ProceduralFeedbackJobRefV1]| {
            jobs.iter().all(ProceduralFeedbackJobRefV1::validate)
                && jobs.windows(2).all(|pair| {
                    (pair[0].transcript_sequence, &pair[0].job_id)
                        < (pair[1].transcript_sequence, &pair[1].job_id)
                })
        };
        let cursor_ordered = self
            .reconciliation_cursors
            .windows(2)
            .all(|pair| pair[0].conversation_id < pair[1].conversation_id);
        let active_ids = self
            .active_jobs
            .iter()
            .map(|value| value.job_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let terminal_ids = self
            .recent_terminal_jobs
            .iter()
            .map(|value| value.job_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if self.schema_version != PROCEDURAL_FEEDBACK_SCOPE_INDEX_SCHEMA_VERSION
            || !self
                .scope_index_key
                .starts_with("procedural_feedback_scope:sha256:")
            || [
                &self.memory_space_id,
                &self.mounted_subject_id,
                &self.channel_id,
                &self.chat_id,
            ]
            .into_iter()
            .any(|value| !is_canonical(value))
            || self.index_revision == 0
            || self.created_at == 0
            || self.updated_at < self.created_at
            || self.active_jobs.len() > MAX_PROCEDURAL_FEEDBACK_ACTIVE_JOBS
            || self.recent_terminal_jobs.len() > MAX_PROCEDURAL_FEEDBACK_RECENT_TERMINAL_JOBS
            || self.reconciliation_cursors.len() > MAX_PROCEDURAL_FEEDBACK_RECONCILIATION_CURSORS
            || !jobs_canonical(&self.active_jobs)
            || !jobs_canonical(&self.recent_terminal_jobs)
            || self
                .active_jobs
                .iter()
                .any(|value| !value.status.is_active())
            || self
                .recent_terminal_jobs
                .iter()
                .any(|value| !value.status.is_terminal())
            || active_ids.len() != self.active_jobs.len()
            || terminal_ids.len() != self.recent_terminal_jobs.len()
            || !active_ids.is_disjoint(&terminal_ids)
            || !self
                .reconciliation_cursors
                .iter()
                .all(ProceduralFeedbackReconciliationCursorV1::validate)
            || !cursor_ordered
        {
            return Err(Error::invalid_input(
                "procedural_feedback_scope_index",
                "scope index identity, jobs, cursors, capacity, or ordering is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralAppliedOwnerBindingV1 {
    AgentToolExperience {
        owner_revision: GovernedOwnerRevisionRef,
        content_digest: String,
    },
    RuntimeSkill {
        binding: RuntimeSkillOwnerBinding,
    },
}

impl ProceduralAppliedOwnerBindingV1 {
    pub fn owner_revision_ref(&self) -> GovernedOwnerRevisionRef {
        match self {
            Self::AgentToolExperience { owner_revision, .. } => owner_revision.clone(),
            Self::RuntimeSkill { binding } => GovernedOwnerRevisionRef {
                owner_ref: binding.owner_ref.clone(),
                owner_revision: binding.owner_revision,
            },
        }
    }

    pub fn validate_for(&self, identity: &ProceduralFeedbackIdentityV1) -> bool {
        match self {
            Self::AgentToolExperience {
                owner_revision,
                content_digest,
            } => {
                owner_revision.is_valid()
                    && owner_revision.owner_ref.owner_plane
                        == GovernedMemoryOwnerPlane::AgentToolExperience
                    && is_digest(content_digest)
            }
            Self::RuntimeSkill { binding } => binding.validate_for(
                &identity.memory_space_id,
                &RuntimeSkillOwningScope::Subject {
                    mounted_subject_id: identity.mounted_subject_id.clone(),
                },
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackApplicationLedgerV1 {
    pub schema_version: u32,
    pub job_id: String,
    pub identity: ProceduralFeedbackIdentityV1,
    pub learning_evidence_digest: String,
    pub applied_owner_bindings: Vec<ProceduralAppliedOwnerBindingV1>,
    pub application_digest: String,
    pub completed_at: u64,
}

impl ProceduralFeedbackApplicationLedgerV1 {
    pub fn build(
        job: &ProceduralFeedbackJobV1,
        mut applied_owner_bindings: Vec<ProceduralAppliedOwnerBindingV1>,
        completed_at: u64,
    ) -> Result<Self> {
        applied_owner_bindings.sort_by(|left, right| {
            let left = serde_json::to_vec(left).unwrap_or_default();
            let right = serde_json::to_vec(right).unwrap_or_default();
            left.cmp(&right)
        });
        let mut ledger = Self {
            schema_version: PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_SCHEMA_VERSION,
            job_id: job.job_id.clone(),
            identity: job.identity.clone(),
            learning_evidence_digest: job.learning_evidence_digest.clone(),
            applied_owner_bindings,
            application_digest: String::new(),
            completed_at,
        };
        ledger.application_digest = ledger.canonical_digest()?;
        ledger.validate()?;
        Ok(ledger)
    }

    pub fn canonical_digest(&self) -> Result<String> {
        let encoded = serde_json::to_vec(&(
            self.schema_version,
            &self.job_id,
            &self.identity,
            &self.learning_evidence_digest,
            &self.applied_owner_bindings,
            self.completed_at,
        ))
        .map_err(|error| Error::config("procedural_feedback_application", error.to_string()))?;
        Ok(domain_digest(
            PROCEDURAL_APPLICATION_DIGEST_DOMAIN,
            &[&encoded],
        ))
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_SCHEMA_VERSION
            || !self.job_id.starts_with("procedural_feedback_job:sha256:")
            || !is_digest(&self.learning_evidence_digest)
            || self.completed_at == 0
            || !canonical_order(&self.applied_owner_bindings)
            || !self
                .applied_owner_bindings
                .iter()
                .all(|binding| binding.validate_for(&self.identity))
            || !self
                .canonical_digest()
                .is_ok_and(|value| value == self.application_digest)
        {
            return Err(Error::invalid_input(
                "procedural_feedback_application",
                "application ledger is non-canonical or has invalid identity",
            ));
        }
        self.identity.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackReceiptV1 {
    pub schema_version: u32,
    pub job_id: String,
    pub operation_id: String,
    pub transaction_id: String,
    pub mutation_receipt_key: String,
    pub transcript_digest: String,
    pub learning_evidence_digest: String,
    pub plan_digest: String,
    pub post_image_digest: String,
    pub submitted_count: u32,
    pub accepted_count: u32,
    pub deferred_count: u32,
    pub rejected_count: u32,
    pub changed_count: u32,
    pub reason_digest: String,
    pub completed_at: u64,
}

impl ProceduralFeedbackReceiptV1 {
    pub fn validate_contract(&self) -> bool {
        let accounted = self
            .accepted_count
            .checked_add(self.deferred_count)
            .and_then(|value| value.checked_add(self.rejected_count));
        self.schema_version == PROCEDURAL_FEEDBACK_RECEIPT_SCHEMA_VERSION
            && [&self.job_id, &self.operation_id, &self.transaction_id]
                .into_iter()
                .all(|value| is_canonical(value))
            && [
                &self.mutation_receipt_key,
                &self.transcript_digest,
                &self.learning_evidence_digest,
                &self.plan_digest,
                &self.post_image_digest,
                &self.reason_digest,
            ]
            .into_iter()
            .all(|value| is_digest(value))
            && self.submitted_count > 0
            && accounted == Some(self.submitted_count)
            // Group counts and changed owner counts use different units.
            // Store closure verifies changed_count against the exact revision ledger.
            && self.completed_at > 0
    }
}

fn canonical_order<T: Serialize>(values: &[T]) -> bool {
    let encoded = values
        .iter()
        .map(serde_json::to_vec)
        .collect::<std::result::Result<Vec<_>, _>>();
    encoded.is_ok_and(|values| values.windows(2).all(|pair| pair[0] < pair[1]))
}

fn is_canonical(value: &str) -> bool {
    !value.is_empty() && value == value.trim() && !value.chars().any(char::is_control)
}

fn is_canonical_text(value: &str) -> bool {
    crate::util::is_canonical_evidence_text(value)
}

fn is_digest(value: &str) -> bool {
    is_prefixed_digest(value, "sha256:")
}

fn is_prefixed_digest(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn domain_digest(domain: &str, fields: &[&[u8]]) -> String {
    format!("sha256:{}", domain_hex(domain, fields))
}

fn domain_hex(domain: &str, fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, domain.as_bytes());
    for field in fields {
        hash_field(&mut hasher, field);
    }
    format!("{:x}", hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}
