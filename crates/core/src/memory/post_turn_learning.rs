//! Canonical public contracts for post-turn procedural learning.
//!
//! These types intentionally carry no Store or worker behavior. PFI1-A freezes the
//! host-neutral evidence boundary before later substages attach it to Transcript and jobs.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod evidence;
pub use evidence::*;
mod producer;
pub use producer::*;
mod contribution;
pub use contribution::*;
mod reconciliation;
pub use reconciliation::*;
mod source_dependents;
pub use source_dependents::*;

use crate::error::{Error, Result};
use crate::memory::{GovernedMemoryOwnerPlane, GovernedOwnerRevisionRef};
use crate::skills::{
    AgentSkillScope, AgentSkillTrust, AgentToolRegistryRef, AgentToolRegistryScope,
    RuntimeSkillOwnerBinding, RuntimeSkillOwnerLocator, RuntimeSkillOwningScope,
};

pub const PROCEDURAL_APPLICABILITY_CONTEXT_SCHEMA_VERSION: u32 = 1;
pub const PROCEDURAL_SELECTION_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION: u32 = 2;
pub const PROCEDURAL_FEEDBACK_JOB_SCHEMA_VERSION: u32 = 2;
pub const PROCEDURAL_FEEDBACK_RECEIPT_SCHEMA_VERSION: u32 = 2;
pub const PROCEDURAL_FEEDBACK_SCOPE_INDEX_SCHEMA_VERSION: u32 = 2;
pub const PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_SCHEMA_VERSION: u32 = 2;
pub const MAX_PROCEDURAL_FEEDBACK_ACTIVE_JOBS: usize = 256;
pub const MAX_PROCEDURAL_FEEDBACK_RECENT_TERMINAL_JOBS: usize = 256;
pub const MAX_PROCEDURAL_FEEDBACK_RECONCILIATION_CURSORS: usize = 256;

const APPLICABILITY_DIGEST_DOMAIN: &str = "procedural_applicability_context_digest_v1";
const SELECTION_DIGEST_DOMAIN: &str = "procedural_selection_digest_v1";
const LEARNING_EVIDENCE_DIGEST_DOMAIN: &str = "post_turn_learning_evidence_digest_v2";
const PROCEDURAL_JOB_ID_DOMAIN: &str = "procedural_feedback_job_id_v1";
const PROCEDURAL_SCOPE_ID_DOMAIN: &str = "procedural_feedback_scope_id_v1";
const RECEIPT_DIGEST_DOMAIN: &str = "procedural_selection_receipt_ref_v1";
const PROCEDURAL_APPLICATION_DIGEST_DOMAIN: &str = "procedural_feedback_application_digest_v2";

fn procedural_scope_id(scope: &ProceduralProducerScopeV1) -> String {
    format!(
        "procedural_feedback_scope:sha256:{}",
        domain_hex(
            PROCEDURAL_SCOPE_ID_DOMAIN,
            &[
                scope.memory_space_id.as_bytes(),
                scope.mounted_subject_id.as_bytes(),
                scope.channel_id.as_bytes(),
                scope.chat_id.as_bytes(),
            ]
        )
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralProjectionBindingV1 {
    Preview,
    Turn { turn_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostTurnLearningInputV2 {
    pub tool_call_count: u32,
    pub selection_receipt: Option<ProceduralSelectionReceiptV1>,
    pub runtime_skill_feedback: Vec<RuntimeSkillUsageFeedbackV1>,
    pub agent_skill_feedback: Vec<AgentSkillUsageFeedbackV1>,
    pub task_learning_feedback: Vec<TaskLearningUsageFeedbackV1>,
    pub agent_tool_feedback: Vec<AgentToolUsageFeedbackV3>,
    pub human_confirmation_operation_id: Option<String>,
}

impl PostTurnLearningInputV2 {
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
            human_confirmation_operation_id: None,
        }
    }

    pub fn has_feedback(&self) -> bool {
        !self.runtime_skill_feedback.is_empty()
            || !self.agent_skill_feedback.is_empty()
            || !self.task_learning_feedback.is_empty()
            || !self.agent_tool_feedback.is_empty()
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
pub enum ProceduralFeedbackAuthorityV2 {
    Empty,
    Producer {
        producer_revision: ProceduralProducerRevisionRefV1,
        source_authority: ProceduralProducerSourceAuthorityV1,
        confirmation: Option<ProceduralHumanConfirmationV1>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralHumanConfirmationV1 {
    pub actor_subject_id: String,
    pub operation_id: String,
    pub evidence_digest: String,
}

impl ProceduralFeedbackAuthorityV2 {
    pub fn validate_contract(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Producer { producer_revision, source_authority, confirmation } => {
                producer_revision.validate_contract() && source_authority.validate_contract()
                    && confirmation.as_ref().is_none_or(|confirmation| {
                        is_canonical(&confirmation.actor_subject_id)
                            && is_canonical(&confirmation.operation_id)
                            && is_digest(&confirmation.evidence_digest)
                            && matches!(source_authority, ProceduralProducerSourceAuthorityV1::HumanUser { subject_id }
                                if subject_id == &confirmation.actor_subject_id)
                    })
            }
        }
    }

    pub const fn is_human_confirmation(&self) -> bool {
        matches!(
            self,
            Self::Producer {
                confirmation: Some(_),
                ..
            }
        )
    }

    pub fn producer_revision(&self) -> Option<&ProceduralProducerRevisionRefV1> {
        match self {
            Self::Empty => None,
            Self::Producer {
                producer_revision, ..
            } => Some(producer_revision),
        }
    }

    pub const fn is_model_inferred(&self) -> bool {
        matches!(
            self,
            Self::Producer {
                source_authority: ProceduralProducerSourceAuthorityV1::ModelInferred { .. },
                ..
            }
        )
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
pub struct PostTurnLearningEvidenceV2 {
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
    pub agent_tool_feedback: Vec<AdmittedAgentToolEvidenceV1>,
    pub authority: ProceduralFeedbackAuthorityV2,
    pub learning_evidence_digest: String,
}

impl PostTurnLearningEvidenceV2 {
    fn validate_job_source(&self, job: &ProceduralFeedbackJobV2) -> Result<()> {
        job.validate()?;
        if !self.validate_contract()
            || self.learning_evidence_digest != job.feedback_source()?.learning_evidence_digest
            || self.memory_space_id != job.feedback_source()?.identity.memory_space_id
            || self.mounted_subject_id != job.feedback_source()?.identity.mounted_subject_id
            || self.conversation_id != job.feedback_source()?.identity.conversation_id
            || self.turn_id != job.feedback_source()?.identity.turn_id
        {
            return Err(Error::invalid_input(
                "procedural_execution_contributions",
                "source evidence and job identity differ",
            ));
        }
        Ok(())
    }

    pub fn runtime_skill_contributions_for_job(
        &self,
        job: &ProceduralFeedbackJobV2,
        observed_at: u64,
    ) -> Result<Vec<RuntimeSkillUsageContributionV1>> {
        self.validate_job_source(job)?;
        // Declaration provenance, including a governed document, never attests
        // execution. Use the same witness rule as producer claim admission.
        if !matches!(&self.authority, ProceduralFeedbackAuthorityV2::Producer {
            source_authority, ..
        } if source_authority.can_witness_execution())
        {
            return Ok(Vec::new());
        }
        let mut contributions = Vec::new();
        for (ordinal, feedback) in self.runtime_skill_feedback.iter().enumerate() {
            if feedback.locator.owning_scope() == &RuntimeSkillOwningScope::SharedProgram {
                continue;
            }
            let producer = self.authority.producer_revision().ok_or_else(|| {
                Error::invalid_input(
                    "procedural_usage_contributions",
                    "usage source authority is missing",
                )
            })?;
            contributions.push(
                RuntimeSkillUsageContributionV1::build(
                    job.feedback_source()?.identity.clone(),
                    self.learning_evidence_digest.clone(),
                    producer.clone(),
                    feedback,
                    ordinal as u32,
                    observed_at,
                )
                .map_err(|_| {
                    Error::invalid_input(
                        "procedural_usage_contributions",
                        "usage source contribution is invalid",
                    )
                })?,
            );
        }
        contributions.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
        Ok(contributions)
    }

    pub fn execution_contributions_for_job(
        &self,
        job: &ProceduralFeedbackJobV2,
    ) -> Result<Vec<ToolExecutionContributionV1>> {
        self.validate_job_source(job)?;
        let mut contributions = Vec::new();
        for feedback in &self.agent_tool_feedback {
            for fact in &feedback.execution_facts {
                let producer = self.authority.producer_revision().ok_or_else(|| {
                    Error::invalid_input(
                        "procedural_execution_contributions",
                        "execution authority is missing",
                    )
                })?;
                contributions.push(
                    ToolExecutionContributionV1::build(
                        job.feedback_source()?.identity.clone(),
                        self.learning_evidence_digest.clone(),
                        producer.clone(),
                        ProceduralProducerToolV1 {
                            registry_ref: feedback.registry_ref.clone(),
                            tool_id: feedback.tool_id.clone(),
                            schema_fingerprint: feedback.schema_fingerprint.clone(),
                        },
                        fact.clone(),
                    )
                    .map_err(|_| {
                        Error::invalid_input(
                            "procedural_execution_contributions",
                            "execution contribution is invalid",
                        )
                    })?,
                );
            }
        }
        contributions.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
        Ok(contributions)
    }

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
        let producer_context = match &self.authority {
            ProceduralFeedbackAuthorityV2::Empty => None,
            ProceduralFeedbackAuthorityV2::Producer {
                producer_revision,
                source_authority,
                confirmation,
            } => Some((
                producer_revision,
                source_authority,
                confirmation.as_ref().map(|value| &value.operation_id),
            )),
        };
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
            &producer_context,
        ))
        .map_err(|error| Error::config("procedural_confirmation_digest", error.to_string()))?;
        Ok(domain_digest(
            "procedural_confirmation_payload_digest_v2",
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
        let mut observation_ids = std::collections::BTreeSet::new();
        let mut call_ids = std::collections::BTreeSet::new();
        let facts_are_unique = self
            .agent_tool_feedback
            .iter()
            .flat_map(|feedback| &feedback.execution_facts)
            .all(|fact| {
                observation_ids.insert(&fact.observation_id) && call_ids.insert(&fact.call_id)
            });
        self.schema_version == POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION
            && facts_are_unique && observation_ids.len() <= MAX_PROCEDURAL_EVIDENCE_ITEMS
            && (observation_ids.is_empty() || matches!(&self.authority,
                ProceduralFeedbackAuthorityV2::Producer { source_authority, .. }
                    if source_authority.can_witness_execution()))
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
                ProceduralFeedbackAuthorityV2::Producer {
                    confirmation: Some(confirmation), ..
                } => self
                    .canonical_confirmation_payload_digest()
                    .is_ok_and(|digest| digest == confirmation.evidence_digest),
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
                .all(AdmittedAgentToolEvidenceV1::validate_contract)
            && evidence::admitted_method_owners_are_consistent(&self.agent_tool_feedback)
            && (matches!(self.authority, ProceduralFeedbackAuthorityV2::Empty)
                == (self.runtime_skill_feedback.is_empty() && self.agent_skill_feedback.is_empty()
                    && self.task_learning_feedback.is_empty() && self.agent_tool_feedback.is_empty()))
            && canonical_order(&self.runtime_skill_feedback)
            && canonical_order(&self.agent_skill_feedback)
            && canonical_order(&self.task_learning_feedback)
            // Every group survives soft filtering, including rejected-only
            // groups. Its Vec position is the stable public receipt ordinal;
            // sorting the filtered body would change both identity and meaning.
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
        procedural_scope_id(&self.producer_scope())
    }

    pub fn producer_scope(&self) -> ProceduralProducerScopeV1 {
        ProceduralProducerScopeV1 {
            memory_space_id: self.memory_space_id.clone(),
            mounted_subject_id: self.mounted_subject_id.clone(),
            channel_id: self.channel_id.clone(),
            chat_id: self.chat_id.clone(),
        }
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
    ReadyToContinue,
    Leased,
    RetryWaiting,
    BlockedCapacity,
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
    AuthorityDenied,
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
        matches!(
            self,
            Self::Pending | Self::ReadyToContinue | Self::Leased | Self::RetryWaiting
        )
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
pub struct ProceduralFeedbackSourceV1 {
    pub identity: ProceduralFeedbackIdentityV1,
    pub transcript_sequence: u64,
    pub transcript_digest: String,
    pub learning_evidence_digest: String,
    pub submitted_count: u32,
}

impl ProceduralFeedbackSourceV1 {
    pub fn validate(&self) -> Result<()> {
        self.identity.validate()?;
        if self.transcript_sequence == 0
            || self.submitted_count == 0
            || !is_digest(&self.transcript_digest)
            || !is_digest(&self.learning_evidence_digest)
        {
            return Err(Error::invalid_input(
                "procedural_feedback_source",
                "invalid canonical feedback source",
            ));
        }
        Ok(())
    }
}

/// Immutable work identity. Reconciliation is caused by a real mutation, not a
/// synthetic conversation. Both variants use the same durable scheduling lane.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralLearningWorkV1 {
    Feedback {
        source: ProceduralFeedbackSourceV1,
    },
    Reconcile {
        source: ProceduralReconciliationWorkV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralLearningReceiptV1 {
    Feedback {
        receipt: ProceduralFeedbackReceiptV2,
    },
    Reconcile {
        receipt: ProceduralReconciliationReceiptV1,
    },
}

impl ProceduralLearningReceiptV1 {
    pub fn feedback(&self) -> Result<&ProceduralFeedbackReceiptV2> {
        match self {
            Self::Feedback { receipt } => Ok(receipt),
            Self::Reconcile { .. } => Err(Error::invalid_input(
                "procedural_learning_work_kind",
                "reconciliation has no feedback receipt",
            )),
        }
    }

    pub fn mutation_receipt_key(&self) -> &str {
        match self {
            Self::Feedback { receipt } => &receipt.mutation_receipt_key,
            Self::Reconcile { receipt } => &receipt.mutation_receipt_key,
        }
    }

    pub fn validates_job(&self, job: &ProceduralFeedbackJobV2) -> bool {
        match (self, &job.work) {
            (Self::Feedback { receipt }, ProceduralLearningWorkV1::Feedback { source }) => {
                receipt.validate_contract()
                    && receipt.job_id == job.job_id
                    && receipt.transcript_digest == source.transcript_digest
                    && receipt.learning_evidence_digest == source.learning_evidence_digest
                    && receipt.submitted_count == source.submitted_count
                    && receipt.completed_at == job.updated_at
            }
            (Self::Reconcile { receipt }, ProceduralLearningWorkV1::Reconcile { source }) => {
                receipt.validate_contract()
                    && source.reference().is_ok_and(|work| work == receipt.work)
                    && job.checkpoint.as_ref().is_some_and(|checkpoint| {
                        matches!(
                            checkpoint.cursor,
                            ProceduralReconciliationCursorV1::Complete
                        ) && checkpoint.content_digest == receipt.checkpoint_digest
                    })
                    && receipt.completed_at == job.updated_at
            }
            _ => false,
        }
    }
}

impl ProceduralLearningWorkV1 {
    pub fn identity(&self) -> Result<(String, String)> {
        match self {
            Self::Feedback { source } => {
                source.validate()?;
                Ok((
                    source.identity.job_id(&source.learning_evidence_digest)?,
                    source.identity.scope_id(),
                ))
            }
            Self::Reconcile { source } => {
                let reference = source.reference()?;
                Ok((reference.job_id, reference.subject_root_key))
            }
        }
    }

    pub fn subject_scope(&self) -> ProceduralSubjectScopeV1 {
        match self {
            Self::Feedback { source } => ProceduralSubjectScopeV1 {
                memory_space_id: source.identity.memory_space_id.clone(),
                mounted_subject_id: source.identity.mounted_subject_id.clone(),
            },
            Self::Reconcile { source } => source.scope.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackJobV2 {
    pub schema_version: u32,
    pub job_id: String,
    pub idempotency_key: String,
    pub discovery_root_key: String,
    pub work: ProceduralLearningWorkV1,
    pub checkpoint: Option<ProceduralReconciliationCheckpointV1>,
    pub checkpoint_authority: Option<ProceduralReconciliationPageAuthorityV1>,
    pub status: ProceduralFeedbackJobStatusV1,
    pub state_revision: u64,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub next_attempt_at: Option<u64>,
    pub lease_owner: Option<String>,
    pub lease_until: Option<u64>,
    pub lease_epoch: u64,
    pub last_error_class: Option<ProceduralFeedbackErrorClassV1>,
    pub receipt: Option<ProceduralLearningReceiptV1>,
    pub created_at: u64,
    pub updated_at: u64,
    pub terminal_at: Option<u64>,
}

impl ProceduralFeedbackJobV2 {
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
        Self::pending_work(
            ProceduralLearningWorkV1::Feedback {
                source: ProceduralFeedbackSourceV1 {
                    identity,
                    transcript_sequence,
                    transcript_digest: transcript_digest.into(),
                    learning_evidence_digest: learning_evidence_digest.into(),
                    submitted_count,
                },
            },
            max_attempts,
            now_secs,
        )
    }

    pub fn pending_work(
        work: ProceduralLearningWorkV1,
        max_attempts: u32,
        now_secs: u64,
    ) -> Result<Self> {
        let (job_id, discovery_root_key) = work.identity()?;
        let job = Self {
            schema_version: PROCEDURAL_FEEDBACK_JOB_SCHEMA_VERSION,
            idempotency_key: job_id.clone(),
            discovery_root_key,
            job_id,
            work,
            checkpoint: None,
            checkpoint_authority: None,
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

    pub fn feedback_source(&self) -> Result<&ProceduralFeedbackSourceV1> {
        match &self.work {
            ProceduralLearningWorkV1::Feedback { source } => Ok(source),
            ProceduralLearningWorkV1::Reconcile { .. } => Err(Error::invalid_input(
                "procedural_learning_work_kind",
                "reconciliation cannot be consumed as turn feedback",
            )),
        }
    }

    /// A successful page keeps its attempt; an expired lease or failed retry
    /// consumes another attempt. Lease epochs always advance, including pages.
    pub fn claim(&self, lease_owner: &str, lease_until: u64, now: u64) -> Result<Self> {
        self.validate()?;
        let continuation = self.status == ProceduralFeedbackJobStatusV1::ReadyToContinue;
        if !is_canonical(lease_owner)
            || lease_until <= now
            || now < self.updated_at
            || !self.status.is_active()
            || (!continuation && self.attempt_count >= self.max_attempts)
            || self.lease_until.is_some_and(|until| until > now)
            || self.next_attempt_at.is_some_and(|eligible| eligible > now)
        {
            return Err(Error::conflict(
                "procedural_feedback_claim",
                "durable work is not claimable",
            ));
        }
        let mut after = self.clone();
        after.status = ProceduralFeedbackJobStatusV1::Leased;
        after.state_revision = self
            .state_revision
            .checked_add(1)
            .ok_or_else(|| Error::config("procedural_feedback_claim", "state revision overflow"))?;
        after.lease_epoch = self
            .lease_epoch
            .checked_add(1)
            .ok_or_else(|| Error::config("procedural_feedback_claim", "lease epoch overflow"))?;
        if !continuation {
            after.attempt_count = self
                .attempt_count
                .checked_add(1)
                .ok_or_else(|| Error::config("procedural_feedback_claim", "attempt overflow"))?;
        }
        after.next_attempt_at = None;
        after.lease_owner = Some(lease_owner.into());
        after.lease_until = Some(lease_until);
        after.last_error_class = None;
        after.updated_at = now;
        after.validate()?;
        Ok(after)
    }

    pub fn continue_reconciliation(
        &self,
        checkpoint: ProceduralReconciliationCheckpointV1,
        authority: ProceduralReconciliationPageAuthorityV1,
        lease_owner: &str,
        lease_epoch: u64,
        now: u64,
    ) -> Result<Self> {
        self.validate()?;
        let ProceduralLearningWorkV1::Reconcile { source } = &self.work else {
            return Err(Error::invalid_input(
                "procedural_reconciliation_page",
                "feedback is not paged reconciliation",
            ));
        };
        checkpoint.validate_for(source)?;
        if authority
            != ProceduralReconciliationPageAuthorityV1::for_claim(
                self,
                authority.operation.actor_subject_id(),
            )?
        {
            return Err(Error::conflict(
                "procedural_reconciliation_page",
                "page authority differs from claimed preimage",
            ));
        }
        if self.status != ProceduralFeedbackJobStatusV1::Leased
            || self.lease_owner.as_deref() != Some(lease_owner)
            || self.lease_epoch != lease_epoch
            || self.lease_until.is_none_or(|until| until <= now)
            || now < self.updated_at
            || checkpoint.updated_at != now
            || !checkpoint.is_successor_of(self.checkpoint.as_ref())
        {
            return Err(Error::conflict(
                "procedural_reconciliation_page",
                "page or lease is not an exact successor",
            ));
        }
        let mut after = self.clone();
        after.status = ProceduralFeedbackJobStatusV1::ReadyToContinue;
        after.state_revision = self.state_revision.checked_add(1).ok_or_else(|| {
            Error::config("procedural_reconciliation_page", "state revision overflow")
        })?;
        after.checkpoint = Some(checkpoint);
        after.checkpoint_authority = Some(authority);
        after.lease_owner = None;
        after.lease_until = None;
        after.next_attempt_at = Some(now);
        after.last_error_class = None;
        after.updated_at = now;
        after.validate()?;
        Ok(after)
    }

    /// Replace a stale scan snapshot without changing the causal work or
    /// resetting failure history. Store must authenticate the new root and
    /// both manifest pins in the same transaction as this transition.
    pub fn restart_reconciliation_snapshot(
        &self,
        checkpoint: ProceduralReconciliationCheckpointV1,
        lease_owner: &str,
        lease_epoch: u64,
        now: u64,
    ) -> Result<Self> {
        self.validate()?;
        let ProceduralLearningWorkV1::Reconcile { source } = &self.work else {
            return Err(Error::invalid_input(
                "procedural_reconciliation_restart",
                "feedback cannot restart a scan",
            ));
        };
        checkpoint.validate_for(source)?;
        if self.status != ProceduralFeedbackJobStatusV1::Leased
            || self.lease_owner.as_deref() != Some(lease_owner)
            || self.lease_epoch != lease_epoch
            || self.lease_until.is_none_or(|until| until <= now)
            || now < self.updated_at
            || checkpoint.updated_at != now
            || checkpoint.page_number != 0
        {
            return Err(Error::conflict(
                "procedural_reconciliation_restart",
                "restart requires the exact lease and empty new snapshot",
            ));
        }
        let mut after = self.clone();
        after.state_revision = self.state_revision.checked_add(1).ok_or_else(|| {
            Error::config(
                "procedural_reconciliation_restart",
                "state revision overflow",
            )
        })?;
        after.status = ProceduralFeedbackJobStatusV1::ReadyToContinue;
        after.checkpoint = Some(checkpoint);
        after.checkpoint_authority = None;
        after.lease_owner = None;
        after.lease_until = None;
        after.next_attempt_at = Some(now);
        after.last_error_class = None;
        after.updated_at = now;
        after.validate()?;
        Ok(after)
    }

    pub fn resume_reconciliation_capacity(
        &self,
        checkpoint: ProceduralReconciliationCheckpointV1,
        now: u64,
    ) -> Result<Self> {
        self.validate()?;
        let ProceduralLearningWorkV1::Reconcile { source } = &self.work else {
            return Err(Error::invalid_input(
                "procedural_reconciliation_resume",
                "only reconciliation has capacity recovery",
            ));
        };
        checkpoint.validate_for(source)?;
        if self.status != ProceduralFeedbackJobStatusV1::BlockedCapacity
            || checkpoint.page_number != 0
            || checkpoint.updated_at != now
            || now < self.updated_at
        {
            return Err(Error::conflict(
                "procedural_reconciliation_resume",
                "resume requires a capacity block and exact empty snapshot",
            ));
        }
        let mut after = self.clone();
        after.state_revision = self.state_revision.checked_add(1).ok_or_else(|| {
            Error::config(
                "procedural_reconciliation_resume",
                "state revision overflow",
            )
        })?;
        after.status = ProceduralFeedbackJobStatusV1::ReadyToContinue;
        after.checkpoint = Some(checkpoint);
        after.checkpoint_authority = None;
        after.next_attempt_at = Some(now);
        after.last_error_class = None;
        after.updated_at = now;
        after.validate()?;
        Ok(after)
    }

    pub fn validate(&self) -> Result<()> {
        let (job_id, discovery_root_key) = self.work.identity()?;
        let lease_fields_present = self.lease_owner.is_some() && self.lease_until.is_some();
        let lease_fields_absent = self.lease_owner.is_none() && self.lease_until.is_none();
        let state_timing_valid = match self.status {
            ProceduralFeedbackJobStatusV1::Pending
            | ProceduralFeedbackJobStatusV1::ReadyToContinue
            | ProceduralFeedbackJobStatusV1::RetryWaiting => {
                self.next_attempt_at.is_some() && lease_fields_absent
            }
            ProceduralFeedbackJobStatusV1::Leased => {
                self.next_attempt_at.is_none() && lease_fields_present
            }
            ProceduralFeedbackJobStatusV1::BlockedCapacity => {
                self.next_attempt_at.is_none()
                    && lease_fields_absent
                    && self.attempt_count > 0
                    && self.last_error_class == Some(ProceduralFeedbackErrorClassV1::BudgetExceeded)
                    && matches!(self.work, ProceduralLearningWorkV1::Reconcile { .. })
            }
            status if status.is_terminal() => self.next_attempt_at.is_none() && lease_fields_absent,
            _ => false,
        };
        let receipt_valid = match (&self.status, &self.receipt) {
            (ProceduralFeedbackJobStatusV1::Succeeded, Some(receipt)) => {
                receipt.validates_job(self)
            }
            (ProceduralFeedbackJobStatusV1::Succeeded, None) => false,
            (_, None) => true,
            (_, Some(_)) => false,
        };
        if self.schema_version != PROCEDURAL_FEEDBACK_JOB_SCHEMA_VERSION
            || self.job_id != job_id
            || self.idempotency_key != self.job_id
            || self.discovery_root_key != discovery_root_key
            || self.state_revision == 0
            || self.max_attempts == 0
            || self.attempt_count > self.max_attempts
            || self.created_at == 0
            || self.updated_at < self.created_at
            || !state_timing_valid
            || self.status.is_terminal() != self.terminal_at.is_some()
            || !receipt_valid
            || match (&self.checkpoint, &self.checkpoint_authority) {
                (None, None) => false,
                (Some(checkpoint), None) => checkpoint.page_number != 0,
                (Some(checkpoint), Some(authority)) => {
                    checkpoint.page_number == 0
                        || authority.validate_for(&self.work.subject_scope()).is_err()
                }
                (None, Some(_)) => true,
            }
            || match (&self.work, &self.checkpoint) {
                (ProceduralLearningWorkV1::Feedback { .. }, Some(_)) => true,
                (ProceduralLearningWorkV1::Reconcile { source }, Some(checkpoint)) => {
                    checkpoint.validate_for(source).is_err()
                        || checkpoint.updated_at > self.updated_at
                }
                (_, None) => self.status == ProceduralFeedbackJobStatusV1::ReadyToContinue,
            }
            || (self.status == ProceduralFeedbackJobStatusV1::ReadyToContinue
                && self.last_error_class.is_some())
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
pub struct ProceduralFeedbackJobRefV2 {
    pub job_id: String,
    pub status: ProceduralFeedbackJobStatusV1,
    pub state_revision: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

impl ProceduralFeedbackJobRefV2 {
    pub fn from_job(job: &ProceduralFeedbackJobV2) -> Self {
        Self {
            job_id: job.job_id.clone(),
            status: job.status,
            state_revision: job.state_revision,
            created_at: job.created_at,
            updated_at: job.updated_at,
        }
    }

    pub fn validate(&self) -> bool {
        self.job_id.starts_with("procedural_feedback_job:sha256:")
            && self.state_revision > 0
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
pub struct ProceduralFeedbackScopeIndexV2 {
    pub schema_version: u32,
    pub scope_index_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: String,
    pub chat_id: String,
    pub index_revision: u64,
    pub producer_heads: Vec<ProceduralProducerRevisionRefV1>,
    pub active_jobs: Vec<ProceduralFeedbackJobRefV2>,
    pub recent_terminal_jobs: Vec<ProceduralFeedbackJobRefV2>,
    pub reconciliation_cursors: Vec<ProceduralFeedbackReconciliationCursorV1>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl ProceduralFeedbackScopeIndexV2 {
    pub fn empty(scope: &ProceduralProducerScopeV1, now_secs: u64) -> Result<Self> {
        let index = Self {
            schema_version: PROCEDURAL_FEEDBACK_SCOPE_INDEX_SCHEMA_VERSION,
            scope_index_key: scope.scope_index_key()?,
            memory_space_id: scope.memory_space_id.clone(),
            mounted_subject_id: scope.mounted_subject_id.clone(),
            channel_id: scope.channel_id.clone(),
            chat_id: scope.chat_id.clone(),
            index_revision: 1,
            producer_heads: Vec::new(),
            active_jobs: Vec::new(),
            recent_terminal_jobs: Vec::new(),
            reconciliation_cursors: Vec::new(),
            created_at: now_secs,
            updated_at: now_secs,
        };
        index.validate()?;
        Ok(index)
    }

    pub fn reconciliation_cursor(
        &self,
        conversation_id: &str,
    ) -> Option<&ProceduralFeedbackReconciliationCursorV1> {
        self.reconciliation_cursors
            .iter()
            .find(|cursor| cursor.conversation_id == conversation_id)
    }

    pub fn producer_scope(&self) -> ProceduralProducerScopeV1 {
        ProceduralProducerScopeV1 {
            memory_space_id: self.memory_space_id.clone(),
            mounted_subject_id: self.mounted_subject_id.clone(),
            channel_id: self.channel_id.clone(),
            chat_id: self.chat_id.clone(),
        }
    }

    /// Pure root update. Store must CAS the original scope and head alongside
    /// the immutable material; creating a producer needs no Transcript alias.
    pub fn bind_producer_head(&mut self, head: &ProceduralProducerHeadV1, now: u64) -> Result<()> {
        self.validate()?;
        if !head.validate_contract() || head.scope != self.producer_scope() || now < self.updated_at
        {
            return Err(Error::invalid_input(
                "procedural_producer_scope",
                "producer scope or time differs from the canonical root",
            ));
        }
        let mut after = self.clone();
        if let Some(reference) = after
            .producer_heads
            .iter_mut()
            .find(|reference| reference.binding_key == head.binding_key)
        {
            if *reference == head.current {
                return Ok(());
            }
            if reference.revision.checked_add(1) != Some(head.current.revision)
                || head
                    .retained_revisions
                    .get(head.retained_revisions.len().saturating_sub(2))
                    != Some(reference)
            {
                return Err(Error::conflict(
                    "procedural_producer_scope",
                    "producer root cannot skip or replace a revision",
                ));
            }
            *reference = head.current.clone();
        } else {
            if head.current.revision != 1
                || after.producer_heads.len() >= MAX_PROCEDURAL_PRODUCERS_PER_SCOPE
            {
                return Err(Error::config(
                    "procedural_producer_scope",
                    "producer root is missing or at capacity",
                ));
            }
            after.producer_heads.push(head.current.clone());
            after
                .producer_heads
                .sort_by(|a, b| a.binding_key.cmp(&b.binding_key));
        }
        after.index_revision = after.index_revision.checked_add(1).ok_or_else(|| {
            Error::config("procedural_producer_scope", "scope revision exhausted")
        })?;
        after.updated_at = now;
        after.validate()?;
        *self = after;
        Ok(())
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
        let jobs_canonical = |jobs: &[ProceduralFeedbackJobRefV2]| {
            jobs.iter().all(ProceduralFeedbackJobRefV2::validate)
                && jobs.windows(2).all(|pair| {
                    (pair[0].created_at, &pair[0].job_id) < (pair[1].created_at, &pair[1].job_id)
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
                .producer_scope()
                .scope_index_key()
                .is_ok_and(|key| key == self.scope_index_key)
            || self.producer_heads.len() > MAX_PROCEDURAL_PRODUCERS_PER_SCOPE
            || !self
                .producer_heads
                .iter()
                .all(ProceduralProducerRevisionRefV1::validate_contract)
            || !self
                .producer_heads
                .windows(2)
                .all(|pair| pair[0].binding_key < pair[1].binding_key)
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
        self.validate_for_subject(&ProceduralSubjectScopeV1 {
            memory_space_id: identity.memory_space_id.clone(),
            mounted_subject_id: identity.mounted_subject_id.clone(),
        })
    }

    pub fn validate_for_subject(&self, scope: &ProceduralSubjectScopeV1) -> bool {
        if !scope.validate_contract() {
            return false;
        }
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
                &scope.memory_space_id,
                &RuntimeSkillOwningScope::Subject {
                    mounted_subject_id: scope.mounted_subject_id.clone(),
                },
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackApplicationLedgerV2 {
    pub schema_version: u32,
    pub job_id: String,
    pub identity: ProceduralFeedbackIdentityV1,
    pub learning_evidence_digest: String,
    pub validity_epoch: u64,
    pub applied_owner_bindings: Vec<ProceduralAppliedOwnerBindingV1>,
    pub execution_contributions: Vec<ToolExecutionContributionV1>,
    pub runtime_skill_contributions: Vec<RuntimeSkillUsageContributionV1>,
    pub application_digest: String,
    pub completed_at: u64,
}

impl ProceduralFeedbackApplicationLedgerV2 {
    pub fn build(
        job: &ProceduralFeedbackJobV2,
        mut applied_owner_bindings: Vec<ProceduralAppliedOwnerBindingV1>,
        mut execution_contributions: Vec<ToolExecutionContributionV1>,
        mut runtime_skill_contributions: Vec<RuntimeSkillUsageContributionV1>,
        validity_epoch: u64,
        completed_at: u64,
    ) -> Result<Self> {
        applied_owner_bindings.sort_by(|left, right| {
            let left = serde_json::to_vec(left).unwrap_or_default();
            let right = serde_json::to_vec(right).unwrap_or_default();
            left.cmp(&right)
        });
        job.validate()?;
        if completed_at < job.created_at {
            return Err(Error::invalid_input(
                "procedural_feedback_application",
                "completion precedes source creation",
            ));
        }
        execution_contributions.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
        runtime_skill_contributions.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
        let mut ledger = Self {
            schema_version: PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_SCHEMA_VERSION,
            job_id: job.job_id.clone(),
            identity: job.feedback_source()?.identity.clone(),
            learning_evidence_digest: job.feedback_source()?.learning_evidence_digest.clone(),
            validity_epoch,
            applied_owner_bindings,
            execution_contributions,
            runtime_skill_contributions,
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
            self.validity_epoch,
            &self.applied_owner_bindings,
            &self.execution_contributions,
            &self.runtime_skill_contributions,
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
            || self.validity_epoch == 0
            || !self
                .identity
                .job_id(&self.learning_evidence_digest)
                .is_ok_and(|key| key == self.job_id)
            || !self.job_id.starts_with("procedural_feedback_job:sha256:")
            || !is_digest(&self.learning_evidence_digest)
            || self.completed_at == 0
            || !canonical_order(&self.applied_owner_bindings)
            || self.execution_contributions.len() > MAX_PROCEDURAL_EVIDENCE_ITEMS
            || self.runtime_skill_contributions.len() > MAX_PROCEDURAL_EVIDENCE_ITEMS
            || !self
                .runtime_skill_contributions
                .windows(2)
                .all(|pair| pair[0].contribution_id < pair[1].contribution_id)
            || !self.runtime_skill_contributions.iter().all(|contribution| {
                contribution.validate_contract()
                    && contribution.source == self.identity
                    && contribution.learning_evidence_digest == self.learning_evidence_digest
                    && contribution.observed_at <= self.completed_at
            })
            || !self
                .execution_contributions
                .windows(2)
                .all(|pair| pair[0].contribution_id < pair[1].contribution_id)
            || !self.execution_contributions.iter().all(|contribution| {
                contribution.validate_contract()
                    && contribution.source == self.identity
                    && contribution.learning_evidence_digest == self.learning_evidence_digest
                    && contribution
                        .fact
                        .completed_at
                        .is_none_or(|time| time <= self.completed_at)
                    && contribution
                        .fact
                        .started_at
                        .is_none_or(|time| time <= self.completed_at)
            })
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralMethodDispositionPhaseV1 {
    Intake,
    Application,
}

/// Safe application correlation has no slot for an arbitrary identifier or text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackMethodDispositionV1 {
    pub feedback_group_ordinal: u32,
    pub method_ordinal: u32,
    pub phase: ProceduralMethodDispositionPhaseV1,
    pub rejection: ProceduralEvidenceRejection,
}

impl ProceduralFeedbackMethodDispositionV1 {
    pub fn validate_contract(&self) -> bool {
        use ProceduralEvidenceRejection as R;
        match self.phase {
            ProceduralMethodDispositionPhaseV1::Intake => matches!(
                self.rejection,
                R::PrivateMethod
                    | R::UnknownMethodSource
                    | R::ExternalMethodSource
                    | R::WeakMethod
                    | R::RawMethodPayload
                    | R::MethodOwnerContentConflict
            ),
            ProceduralMethodDispositionPhaseV1::Application => {
                self.rejection == R::MethodOwnerContentConflict
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralFeedbackReceiptV2 {
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
    pub partially_accepted_count: u32,
    pub method_dispositions: Vec<ProceduralFeedbackMethodDispositionV1>,
    pub deferred_count: u32,
    pub rejected_count: u32,
    pub changed_count: u32,
    pub reason_digest: String,
    pub completed_at: u64,
}

impl ProceduralFeedbackReceiptV2 {
    /// Counts and method ordinals are checked against the exact admitted source,
    /// not inferred from an aggregate or a mutable current method. Agent Tool
    /// groups occupy the first ordinal range; the other feedback planes follow.
    pub fn validates_evidence(&self, evidence: &PostTurnLearningEvidenceV2) -> bool {
        if !self.validate_contract()
            || !evidence.validate_contract()
            || self.learning_evidence_digest != evidence.learning_evidence_digest
            || self.submitted_count as usize
                != evidence.agent_tool_feedback.len()
                    + evidence.runtime_skill_feedback.len()
                    + evidence.agent_skill_feedback.len()
                    + evidence.task_learning_feedback.len()
        {
            return false;
        }
        for item in &self.method_dispositions {
            let Some(group) = evidence
                .agent_tool_feedback
                .get(item.feedback_group_ordinal as usize)
            else {
                return false;
            };
            if item.method_ordinal >= group.submitted_method_count {
                return false;
            }
            let intake = group
                .rejected_methods
                .iter()
                .find(|value| value.input_index == item.method_ordinal);
            match item.phase {
                ProceduralMethodDispositionPhaseV1::Intake => {
                    if !intake.is_some_and(|value| value.rejection == item.rejection) {
                        return false;
                    }
                }
                ProceduralMethodDispositionPhaseV1::Application => {
                    if intake.is_some() {
                        return false;
                    }
                }
            }
        }
        let mut accepted = 0_u32;
        let mut partial = 0_u32;
        let mut rejected = 0_u32;
        for (ordinal, group) in evidence.agent_tool_feedback.iter().enumerate() {
            let items = self
                .method_dispositions
                .iter()
                .filter(|item| item.feedback_group_ordinal as usize == ordinal)
                .collect::<Vec<_>>();
            if !group.rejected_methods.iter().all(|intake| {
                items.iter().any(|item| {
                    item.phase == ProceduralMethodDispositionPhaseV1::Intake
                        && item.method_ordinal == intake.input_index
                        && item.rejection == intake.rejection
                })
            }) {
                return false;
            }
            let has_accepted = !group.execution_facts.is_empty()
                || items.len() < group.submitted_method_count as usize;
            match (has_accepted, items.is_empty()) {
                (true, true) => accepted += 1,
                (true, false) => partial += 1,
                (false, _) => rejected += 1,
            }
        }
        self.partially_accepted_count == partial
            && self.accepted_count >= accepted
            && self.rejected_count >= rejected
    }

    pub fn validate_contract(&self) -> bool {
        let accounted = self
            .accepted_count
            .checked_add(self.partially_accepted_count)
            .and_then(|value| value.checked_add(self.deferred_count))
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
            && self.method_dispositions.iter().all(|item|
                item.validate_contract() && item.feedback_group_ordinal < self.submitted_count
                    && item.method_ordinal < MAX_PROCEDURAL_EVIDENCE_ITEMS as u32)
            && self.method_dispositions.windows(2).all(|pair|
                (pair[0].feedback_group_ordinal, pair[0].method_ordinal)
                    < (pair[1].feedback_group_ordinal, pair[1].method_ordinal))
            && self.partially_accepted_count as usize <= self.method_dispositions.iter()
                .map(|item| item.feedback_group_ordinal).collect::<std::collections::BTreeSet<_>>().len()
            && self.method_dispositions.iter().map(|item| item.feedback_group_ordinal)
                .collect::<std::collections::BTreeSet<_>>().len()
                <= self.partially_accepted_count.saturating_add(self.rejected_count) as usize
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
