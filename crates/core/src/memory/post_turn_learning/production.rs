//! Production learning inputs and untrusted model proposals. These are not
//! authority capabilities: the SDK must authenticate the producer, resolve each
//! source at its exact revision, and revalidate disclosure before use/commit.

use serde::{Deserialize, Serialize};

use super::{evidence::identifier, is_canonical_text, is_digest, ProceduralSourceSensitivity};
use crate::memory::{GovernedMemoryOwnerPlane, GovernedOwnerRevisionRef, MemoryTurnDeliveryStatus};
use crate::skills::AgentToolRegistryRef;
use crate::{Error, Result};

pub const MAX_METHOD_LEARNING_MATERIALS: usize = 8;
pub const MAX_METHOD_LEARNING_MATERIAL_BYTES: usize = 2048;
pub const MAX_METHOD_LEARNING_PROPOSALS: usize = 4;
pub const MAX_METHOD_LEARNING_PROCEDURE_BYTES: usize = 4096;
pub const MAX_METHOD_LEARNING_SOURCE_REFS: usize = 16;

/// Exact admitted material reference. No raw text is duplicated into the method.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodLearningMaterialRefV1 {
    pub input_group_ordinal: u32,
    pub material_id: String,
    pub source: MethodLearningMaterialSourceV1,
    pub content_digest: String,
}

impl MethodLearningMaterialRefV1 {
    pub fn from_material(
        input_group_ordinal: u32,
        material: &MethodLearningMaterialV1,
    ) -> Result<Self> {
        if !material.permits_method_extraction()
            || input_group_ordinal as usize >= MAX_METHOD_LEARNING_MATERIALS
        {
            return Err(invalid());
        }
        Ok(Self {
            input_group_ordinal,
            material_id: material.material_id.clone(),
            source: material.source.clone(),
            content_digest: production_digest("method_learning_material_ref_v1", material)?,
        })
    }

    pub fn validate_contract(&self) -> bool {
        (self.input_group_ordinal as usize) < MAX_METHOD_LEARNING_MATERIALS
            && identifier(&self.material_id)
            && self.source.validate_contract()
            && is_digest(&self.content_digest)
    }
}

/// Immutable semantic-job provenance. The producing receipt points at the new
/// method post-image; this source deliberately never points back at that receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernanceMethodExtractionSourceV1 {
    pub identity: crate::memory::PostTurnGovernanceIdentityV2,
    pub source_job_id: String,
    pub source_transcript_sequence: u64,
    pub source_transcript_digest: String,
    pub learning_evidence_digest: String,
    pub runtime_binding_digest: String,
    pub execution_binding: crate::memory::PostTurnGovernanceExecutionBindingV1,
    pub execution_binding_digest: String,
    pub governance_contract_version: u32,
    pub proposal_ordinal: u32,
    pub tool: super::ProceduralProducerToolV1,
    pub materials: Vec<MethodLearningMaterialRefV1>,
    pub method_digest: String,
}

impl GovernanceMethodExtractionSourceV1 {
    pub const fn source_authority(&self) -> crate::memory::MemoryEvidenceAuthority {
        crate::memory::MemoryEvidenceAuthority::ModelInferred
    }

    pub fn validate_contract(&self) -> bool {
        self.identity.validate().is_ok()
            && self.source_job_id == self.identity.job_id()
            && self.source_transcript_sequence > 0
            && is_digest(&self.source_transcript_digest)
            && is_digest(&self.learning_evidence_digest)
            && is_digest(&self.runtime_binding_digest)
            && is_digest(&self.execution_binding_digest)
            && self.execution_binding.validate().is_ok()
            && matches!(
                self.execution_binding,
                crate::memory::PostTurnGovernanceExecutionBindingV1::Bound { .. }
            )
            && self.governance_contract_version > 0
            && (self.proposal_ordinal as usize) < MAX_METHOD_LEARNING_PROPOSALS
            && self.tool.validate_contract()
            && is_digest(&self.method_digest)
            && !self.materials.is_empty()
            && self.materials.len() <= MAX_METHOD_LEARNING_MATERIALS
            && self
                .materials
                .iter()
                .all(MethodLearningMaterialRefV1::validate_contract)
            && self.materials.windows(2).all(|pair| {
                (pair[0].input_group_ordinal, &pair[0].material_id)
                    < (pair[1].input_group_ordinal, &pair[1].material_id)
            })
    }

    pub fn build(
        job: &crate::memory::PostTurnGovernanceJob,
        binding: &crate::memory::PostTurnGovernanceBindingSnapshotV1,
        transcript: &crate::memory::TranscriptTurnRecord,
        input_group_ordinal: u32,
        proposal_ordinal: u32,
        proposal: &MethodLearningProposalV1,
    ) -> Result<Self> {
        if job.status != crate::memory::PostTurnGovernanceJobStatusV2::Leased {
            return Err(invalid());
        }
        let evidence = transcript.learning_evidence.as_ref().ok_or_else(invalid)?;
        let group = evidence
            .method_learning_inputs
            .get(input_group_ordinal as usize)
            .ok_or_else(invalid)?;
        proposal.validate_against(&group.materials)?;
        let materials = proposal
            .source_material_ids
            .iter()
            .map(|id| {
                MethodLearningMaterialRefV1::from_material(
                    input_group_ordinal,
                    group
                        .materials
                        .iter()
                        .find(|material| &material.material_id == id)
                        .ok_or_else(invalid)?,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let value = Self {
            identity: job.identity.clone(),
            source_job_id: job.job_id.clone(),
            source_transcript_sequence: job.transcript_sequence,
            source_transcript_digest: job.transcript_digest.clone(),
            learning_evidence_digest: evidence.learning_evidence_digest.clone(),
            runtime_binding_digest: job.runtime_binding_digest.clone(),
            execution_binding: job.execution_binding.clone(),
            execution_binding_digest: binding.canonical_digest.clone(),
            governance_contract_version: job.governance_contract_version,
            proposal_ordinal,
            tool: super::ProceduralProducerToolV1 {
                registry_ref: group.registry_ref.clone(),
                tool_id: group.tool_id.clone(),
                schema_fingerprint: group.schema_fingerprint.clone(),
            },
            materials,
            method_digest: proposal.content_digest()?,
        };
        value.validate_against(job, binding, transcript)?;
        Ok(value)
    }

    pub fn validate_against(
        &self,
        job: &crate::memory::PostTurnGovernanceJob,
        binding: &crate::memory::PostTurnGovernanceBindingSnapshotV1,
        transcript: &crate::memory::TranscriptTurnRecord,
    ) -> Result<()> {
        job.validate()?;
        binding.validate()?;
        let evidence = transcript.learning_evidence.as_ref().ok_or_else(invalid)?;
        if !self.validate_contract()
            || !evidence.validate_contract()
            || !transcript.permits_post_turn_learning()
            || evidence.memory_space_id != self.identity.memory_space_id
            || evidence.mounted_subject_id != self.identity.mounted_subject_id
            || evidence.conversation_id != self.identity.conversation_id
            || evidence.turn_id != self.identity.turn_id
            || self.identity != job.identity
            || self.source_job_id != job.job_id
            || self.source_transcript_sequence != job.transcript_sequence
            || transcript.sequence != self.source_transcript_sequence
            || transcript.key.memory_space_id != self.identity.memory_space_id
            || transcript.subject != self.identity.mounted_subject_id
            || transcript.key.channel_id != self.identity.channel_id
            || transcript.key.conversation_id != self.identity.conversation_id
            || transcript.turn_id != self.identity.turn_id
            || self.source_transcript_digest != job.transcript_digest
            || crate::memory::post_turn_governance_transcript_digest(transcript)?
                != self.source_transcript_digest
            || self.learning_evidence_digest != evidence.learning_evidence_digest
            || self.runtime_binding_digest != job.runtime_binding_digest
            || self.governance_contract_version != job.governance_contract_version
            || self.execution_binding != job.execution_binding
            || self.execution_binding
                != (crate::memory::PostTurnGovernanceExecutionBindingV1::Bound {
                    binding_id: binding.binding_id.clone(),
                    binding_revision: binding.binding_revision,
                })
            || self.execution_binding_digest != binding.canonical_digest
        {
            return Err(invalid());
        }
        for reference in &self.materials {
            let group = evidence
                .method_learning_inputs
                .get(reference.input_group_ordinal as usize)
                .ok_or_else(invalid)?;
            let material = group
                .materials
                .iter()
                .find(|material| material.material_id == reference.material_id)
                .ok_or_else(invalid)?;
            if reference
                != &MethodLearningMaterialRefV1::from_material(
                    reference.input_group_ordinal,
                    material,
                )?
                || group.registry_ref != self.tool.registry_ref
                || group.tool_id != self.tool.tool_id
                || group.schema_fingerprint != self.tool.schema_fingerprint
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

/// Registered with the producer by the governing authority. A submitted name
/// is not permission to invent a new evaluator or change its implementation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodTaskEvaluatorV1 {
    pub id: String,
    pub revision: u64,
}

impl MethodTaskEvaluatorV1 {
    pub fn validate_contract(&self) -> bool {
        identifier(&self.id) && self.revision > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodLearningMaterialRejectionV1 {
    Private,
    Unknown,
    External,
}

/// No rejected source identifier, text, or unkeyed body digest survives admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodLearningMaterialDispositionV1 {
    pub input_index: u32,
    pub rejection: MethodLearningMaterialRejectionV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmittedMethodLearningInputV1 {
    pub registry_ref: AgentToolRegistryRef,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub materials: Vec<MethodLearningMaterialV1>,
    pub submitted_material_count: u32,
    pub submitted_classifications: Vec<ProceduralSourceSensitivity>,
    pub rejected_materials: Vec<MethodLearningMaterialDispositionV1>,
}

impl AdmittedMethodLearningInputV1 {
    /// SDK resolves exact sources first and can only tighten classification.
    pub fn admit(input: &MethodLearningInputV1) -> Result<Self> {
        if !input.validate_contract() {
            return Err(invalid());
        }
        let mut value = Self {
            registry_ref: input.registry_ref.clone(),
            tool_id: input.tool_id.clone(),
            schema_fingerprint: input.schema_fingerprint.clone(),
            materials: Vec::new(),
            submitted_material_count: input.materials.len() as u32,
            submitted_classifications: input
                .materials
                .iter()
                .map(|item| item.sensitivity)
                .collect(),
            rejected_materials: Vec::new(),
        };
        value.submitted_classifications.sort();
        value.submitted_classifications.dedup();
        for (index, material) in input.materials.iter().enumerate() {
            let rejection = match material.sensitivity {
                ProceduralSourceSensitivity::Private => {
                    Some(MethodLearningMaterialRejectionV1::Private)
                }
                ProceduralSourceSensitivity::Unknown => {
                    Some(MethodLearningMaterialRejectionV1::Unknown)
                }
                ProceduralSourceSensitivity::NonPrivate if material.external_content => {
                    Some(MethodLearningMaterialRejectionV1::External)
                }
                ProceduralSourceSensitivity::NonPrivate => None,
            };
            if let Some(rejection) = rejection {
                value
                    .rejected_materials
                    .push(MethodLearningMaterialDispositionV1 {
                        input_index: index as u32,
                        rejection,
                    });
            } else {
                value.materials.push(material.clone());
            }
        }
        Ok(value)
    }

    pub fn validate_contract(&self) -> bool {
        self.registry_ref.validate_contract()
            && identifier(&self.tool_id)
            && identifier(&self.schema_fingerprint)
            && self.submitted_material_count > 0
            && self.submitted_material_count as usize <= MAX_METHOD_LEARNING_MATERIALS
            && self.materials.len() + self.rejected_materials.len()
                == self.submitted_material_count as usize
            && self
                .materials
                .iter()
                .all(MethodLearningMaterialV1::permits_method_extraction)
            && self
                .materials
                .windows(2)
                .all(|pair| pair[0].material_id < pair[1].material_id)
            && !self.submitted_classifications.is_empty()
            && self.submitted_classifications.len() <= 3
            && self
                .submitted_classifications
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            && self
                .materials
                .iter()
                .all(|item| self.submitted_classifications.contains(&item.sensitivity))
            && self
                .rejected_materials
                .iter()
                .all(|item| item.input_index < self.submitted_material_count)
            && self
                .rejected_materials
                .windows(2)
                .all(|pair| pair[0].input_index < pair[1].input_index)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodLearningMaterialPurposeV1 {
    TaskConstraint,
    ExecutedStrategy,
    VerificationObservation,
}

/// A message belongs to the enclosing canonical turn, never a caller-selected
/// conversation. Governed references resolve against the current read authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MethodLearningMaterialSourceV1 {
    CanonicalInputMessage {
        input_index: u32,
    },
    CanonicalAssistantMessage {
        assistant_index: u32,
    },
    /// Bounded strategy observation supplied by the authenticated executor;
    /// never an instruction to fetch raw arguments or tool output.
    ToolObservation {
        observation_id: String,
    },
    GovernedRevision {
        revision: GovernedOwnerRevisionRef,
        content_digest: String,
    },
}

impl MethodLearningMaterialSourceV1 {
    pub fn validate_contract(&self) -> bool {
        match self {
            Self::CanonicalInputMessage { .. } | Self::CanonicalAssistantMessage { .. } => true,
            Self::ToolObservation { observation_id } => identifier(observation_id),
            Self::GovernedRevision {
                revision,
                content_digest,
            } => {
                revision.is_valid()
                    && matches!(
                        revision.owner_ref.owner_plane,
                        GovernedMemoryOwnerPlane::LongTerm
                            | GovernedMemoryOwnerPlane::EvidenceDocument
                    )
                    && content_digest.len() == 64
                    && content_digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodLearningMaterialV1 {
    pub material_id: String,
    pub purpose: MethodLearningMaterialPurposeV1,
    pub source: MethodLearningMaterialSourceV1,
    pub text: String,
    pub sensitivity: ProceduralSourceSensitivity,
    pub external_content: bool,
}

impl MethodLearningMaterialV1 {
    pub fn validate_contract(&self) -> bool {
        identifier(&self.material_id)
            && self.source.validate_contract()
            && self.text.len() <= MAX_METHOD_LEARNING_MATERIAL_BYTES
            && is_canonical_text(&self.text)
    }

    /// A necessary content restriction, not a substitute for source permission.
    pub fn permits_method_extraction(&self) -> bool {
        self.validate_contract()
            && self.sensitivity == ProceduralSourceSensitivity::NonPrivate
            && !self.external_content
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodLearningInputV1 {
    pub registry_ref: AgentToolRegistryRef,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub materials: Vec<MethodLearningMaterialV1>,
}

impl MethodLearningInputV1 {
    pub fn validate_contract(&self) -> bool {
        self.registry_ref.validate_contract()
            && identifier(&self.tool_id)
            && identifier(&self.schema_fingerprint)
            && !self.materials.is_empty()
            && self.materials.len() <= MAX_METHOD_LEARNING_MATERIALS
            && self
                .materials
                .iter()
                .all(MethodLearningMaterialV1::validate_contract)
            && self
                .materials
                .windows(2)
                .all(|pair| pair[0].material_id < pair[1].material_id)
    }
}

/// The model cannot supply authorship, owner/revision, approval, execution, or
/// success. Provenance is bound by the SDK after validating this narrow output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodLearningProposalV1 {
    pub task_signature: String,
    pub procedure: String,
    pub constraints: Vec<String>,
    pub source_material_ids: Vec<String>,
}

impl MethodLearningProposalV1 {
    pub fn validate_against(&self, materials: &[MethodLearningMaterialV1]) -> Result<()> {
        if !identifier(&self.task_signature)
            || self.procedure.len() > MAX_METHOD_LEARNING_PROCEDURE_BYTES
            || !is_canonical_text(&self.procedure)
            || self.constraints.len() > 8
            || self.constraints.iter().any(|text| !identifier(text))
            || !self.constraints.windows(2).all(|pair| pair[0] < pair[1])
            || self.source_material_ids.is_empty()
            || self.source_material_ids.len() > MAX_METHOD_LEARNING_SOURCE_REFS
            || !self
                .source_material_ids
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            || materials.is_empty()
            || materials.len() > MAX_METHOD_LEARNING_MATERIALS
            || !materials
                .iter()
                .all(MethodLearningMaterialV1::validate_contract)
            || !materials
                .windows(2)
                .all(|pair| pair[0].material_id < pair[1].material_id)
            || self.source_material_ids.iter().any(|id| {
                !identifier(id)
                    || !materials.iter().any(|source| {
                        &source.material_id == id && source.permits_method_extraction()
                    })
            })
        {
            return Err(invalid());
        }
        crate::skills::validate_runtime_skill_method_shape(&self.task_signature, &self.procedure)
            .map_err(|_| invalid())
    }

    pub fn content_digest(&self) -> Result<String> {
        crate::skills::canonical_agent_tool_method_content_digest(
            &self.task_signature,
            &self.procedure,
            &self.constraints,
        )
    }
}

/// An exact version, never a fuzzy task signature or a selection receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodContentRefV1 {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub registry_ref: AgentToolRegistryRef,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub owner_revision: GovernedOwnerRevisionRef,
    pub content_digest: String,
}

impl MethodContentRefV1 {
    /// Identify an actually delivered method without exposing raw Store owner
    /// constructors to SDK consumers. This is a reference, not an adoption or
    /// authority grant; intake still verifies the receipt signature and sources.
    pub fn from_selection(
        receipt: &super::MemoryTurnProjectionReceiptV2,
        selection: &super::AgentToolExperienceSelectionV1,
    ) -> Result<Self> {
        if !receipt.validate_contract() || !receipt.agent_tool_experiences.contains(selection) {
            return Err(invalid());
        }
        let reference = Self {
            memory_space_id: receipt.identity.memory_space_id.clone(),
            mounted_subject_id: receipt.identity.mounted_subject_id.clone(),
            registry_ref: selection.registry_ref.clone(),
            tool_id: selection.tool_id.clone(),
            schema_fingerprint: selection.schema_fingerprint.clone(),
            owner_revision: GovernedOwnerRevisionRef {
                owner_ref: crate::memory::GovernedMemoryOwnerRef {
                    owner_plane: GovernedMemoryOwnerPlane::AgentToolExperience,
                    owner_id: selection.experience_owner_id.clone(),
                },
                owner_revision: selection.experience_revision,
            },
            content_digest: selection.experience_content_digest.clone(),
        };
        if !reference.validate_contract() {
            return Err(invalid());
        }
        Ok(reference)
    }

    pub fn validate_contract(&self) -> bool {
        identifier(&self.memory_space_id)
            && identifier(&self.mounted_subject_id)
            && self.registry_ref.validate_contract()
            && identifier(&self.tool_id)
            && identifier(&self.schema_fingerprint)
            && self.owner_revision.is_valid()
            && self.owner_revision.owner_ref.owner_plane
                == GovernedMemoryOwnerPlane::AgentToolExperience
            && is_digest(&self.content_digest)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodAdoptionStatusV1 {
    Adopted,
    NotAdopted,
    Unknown,
}

impl MethodAdoptionStatusV1 {
    pub fn is_adopted(self) -> bool {
        self == Self::Adopted
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodTaskOutcomeV1 {
    Passed,
    Failed,
    Partial,
    Cancelled,
    Unknown,
}

impl MethodTaskOutcomeV1 {
    pub fn supports_positive_contribution(self) -> bool {
        self == Self::Passed
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodAdoptionEvidenceV1 {
    pub method: MethodContentRefV1,
    pub task_id: String,
    pub adoption: MethodAdoptionStatusV1,
    pub observation_refs: Vec<String>,
}

impl MethodAdoptionEvidenceV1 {
    pub fn validate_contract(&self) -> bool {
        self.method.validate_contract()
            && identifier(&self.task_id)
            && self.observation_refs.len() <= MAX_METHOD_LEARNING_SOURCE_REFS
            && self.observation_refs.iter().all(|id| identifier(id))
            && self
                .observation_refs
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            && (!self.adoption.is_adopted() || !self.observation_refs.is_empty())
    }
}

/// Independence is a trusted evaluator claim validated against its producer
/// binding. It is not inferred from a new turn ID, call count or model prose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodTaskEvaluationV1 {
    pub method: MethodContentRefV1,
    pub task_id: String,
    pub task_root_id: String,
    pub independence_group: String,
    pub evaluator_id: String,
    pub evaluator_revision: u64,
    pub outcome: MethodTaskOutcomeV1,
    pub observation_refs: Vec<String>,
}

impl MethodTaskEvaluationV1 {
    /// Two fresh task IDs do not establish independence. The trusted evaluator
    /// must supply both a different root and a different scenario group.
    pub fn independent_of(&self, other: &Self) -> bool {
        self.validate_contract()
            && other.validate_contract()
            && self.task_id != other.task_id
            && self.task_root_id != other.task_root_id
            && self.independence_group != other.independence_group
    }

    pub fn validate_contract(&self) -> bool {
        self.method.validate_contract()
            && [
                &self.task_id,
                &self.task_root_id,
                &self.independence_group,
                &self.evaluator_id,
            ]
            .into_iter()
            .all(|id| identifier(id))
            && self.evaluator_revision > 0
            && !self.observation_refs.is_empty()
            && self.observation_refs.len() <= MAX_METHOD_LEARNING_SOURCE_REFS
            && self.observation_refs.iter().all(|id| identifier(id))
            && self
                .observation_refs
                .windows(2)
                .all(|pair| pair[0] < pair[1])
    }

    /// Correlation only. SDK/Store must additionally verify independent producer
    /// authority, canonical observations, source validity and admission time.
    pub fn supports_adoption(
        &self,
        adoption: &MethodAdoptionEvidenceV1,
        delivery: MemoryTurnDeliveryStatus,
    ) -> bool {
        self.validate_contract()
            && adoption.validate_contract()
            && self.method == adoption.method
            && self.task_id == adoption.task_id
            && delivery == MemoryTurnDeliveryStatus::Delivered
            && adoption.adoption.is_adopted()
            && self.outcome.supports_positive_contribution()
    }
}

/// A reference to adoption of a prior immutable method, not the material that
/// will contain this reference. This makes the post-image digest acyclic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodTaskContributionRefV1 {
    pub contribution_id: String,
    pub content_digest: String,
    pub source_job_id: String,
    pub method: MethodContentRefV1,
    pub method_content_digest: String,
}

impl MethodTaskContributionRefV1 {
    pub fn validate_contract(&self) -> bool {
        is_digest(&self.contribution_id)
            && is_digest(&self.content_digest)
            && identifier(&self.source_job_id)
            && self.method.validate_contract()
            && is_digest(&self.method_content_digest)
    }
}

/// Retained in the existing procedural application ledger. It records the
/// evaluator outcome even when negative; no success count is synthesized.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodTaskContributionV1 {
    pub source: super::ProceduralFeedbackIdentityV1,
    pub source_job_id: String,
    pub source_transcript_digest: String,
    pub learning_evidence_digest: String,
    pub producer: super::ProceduralProducerRevisionRefV1,
    pub adoption_ordinal: u32,
    pub evaluation_ordinal: u32,
    pub adoption: MethodAdoptionEvidenceV1,
    pub evaluation: MethodTaskEvaluationV1,
    pub method_content_digest: String,
    pub execution_refs: Vec<super::ProceduralContributionRefV1>,
    pub delivery_status: MemoryTurnDeliveryStatus,
    pub observed_at: u64,
    pub contribution_id: String,
    pub content_digest: String,
}

impl MethodTaskContributionV1 {
    pub fn build(
        job: &super::ProceduralFeedbackJobV2,
        transcript: &crate::memory::TranscriptTurnRecord,
        adoption_ordinal: u32,
        evaluation_ordinal: u32,
        adopted_material: &crate::skills::AgentToolExperienceRevisionMaterialV4,
    ) -> Result<Self> {
        let evidence = transcript.learning_evidence.as_ref().ok_or_else(invalid)?;
        evidence.validate_job_source(job)?;
        let source = job.feedback_source()?;
        if !transcript.permits_post_turn_learning()
            || source.identity.memory_space_id != transcript.key.memory_space_id
            || source.identity.mounted_subject_id != transcript.subject
            || source.identity.channel_id != transcript.key.channel_id
            || source.identity.conversation_id != transcript.key.conversation_id
            || source.identity.turn_id != transcript.turn_id
            || source.transcript_sequence != transcript.sequence
            || source.transcript_digest
                != crate::memory::post_turn_governance_transcript_digest(transcript)?
            || !matches!(&evidence.authority, super::ProceduralFeedbackAuthorityV2::Producer { source_authority, .. } if source_authority.can_witness_execution())
        {
            return Err(invalid());
        }
        let adoption = evidence
            .method_adoptions
            .get(adoption_ordinal as usize)
            .ok_or_else(invalid)?
            .clone();
        let evaluation = evidence
            .method_task_evaluations
            .get(evaluation_ordinal as usize)
            .ok_or_else(invalid)?
            .clone();
        if adoption.method != evaluation.method
            || adoption.task_id != evaluation.task_id
            || !adopted_material.validate_contract().accepted
            || adopted_material.updated_at > transcript.created_at
            || adopted_material.owner_revision_ref() != adoption.method.owner_revision
            || adopted_material.content_digest != adoption.method.content_digest
            || adopted_material.memory_space_id != adoption.method.memory_space_id
            || adopted_material.owning_scope.mounted_subject_id()
                != adoption.method.mounted_subject_id
            || adopted_material.registry_id != adoption.method.registry_ref.registry_id
            || adopted_material.registry_scope != adoption.method.registry_ref.scope
            || adopted_material.tool_id != adoption.method.tool_id
            || adopted_material.schema_fingerprint != adoption.method.schema_fingerprint
        {
            return Err(invalid());
        }
        let method_content_digest = adopted_material
            .body
            .method_content_digest()?
            .ok_or_else(invalid)?;
        let execution = evidence.execution_contributions_for_job(job)?;
        let mut execution_refs = Vec::new();
        for observation in &adoption.observation_refs {
            let fact = execution
                .iter()
                .find(|fact| {
                    fact.fact.observation_id == *observation
                        && fact.tool.registry_ref == adoption.method.registry_ref
                        && fact.tool.tool_id == adoption.method.tool_id
                        && fact.tool.schema_fingerprint == adoption.method.schema_fingerprint
                })
                .ok_or_else(invalid)?;
            if !fact.fact.validates_canonical_call(
                &fact.tool.tool_id,
                &transcript.tool_observations,
                transcript.created_at,
            ) {
                return Err(invalid());
            }
            execution_refs.push(fact.reference().map_err(|_| invalid())?);
        }
        if evaluation.observation_refs.iter().any(|id| {
            !transcript
                .tool_observations
                .iter()
                .any(|observation| &observation.observation_id == id)
        }) {
            return Err(invalid());
        }
        execution_refs.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
        let mut value = Self {
            source: source.identity.clone(),
            source_job_id: job.job_id.clone(),
            source_transcript_digest: source.transcript_digest.clone(),
            learning_evidence_digest: evidence.learning_evidence_digest.clone(),
            producer: evidence
                .authority
                .producer_revision()
                .ok_or_else(invalid)?
                .clone(),
            adoption_ordinal,
            evaluation_ordinal,
            adoption,
            evaluation,
            method_content_digest,
            execution_refs,
            delivery_status: transcript.delivery_status,
            observed_at: transcript.created_at,
            contribution_id: String::new(),
            content_digest: String::new(),
        };
        value.contribution_id = value.identity_digest()?;
        value.content_digest = value.canonical_digest()?;
        if !value.validate_contract() {
            return Err(invalid());
        }
        Ok(value)
    }

    pub fn identity_digest(&self) -> Result<String> {
        production_digest(
            "method_task_contribution_identity_v1",
            &(
                &self.source,
                &self.producer.binding_key,
                &self.adoption.method,
                &self.adoption.task_id,
                &self.evaluation.evaluator_id,
                self.evaluation.evaluator_revision,
            ),
        )
    }

    pub fn canonical_digest(&self) -> Result<String> {
        production_digest(
            "method_task_contribution_digest_v1",
            &(
                &self.source,
                &self.source_job_id,
                &self.source_transcript_digest,
                &self.learning_evidence_digest,
                &self.producer,
                self.adoption_ordinal,
                self.evaluation_ordinal,
                &self.adoption,
                &self.evaluation,
                &self.method_content_digest,
                &self.execution_refs,
                self.delivery_status,
                self.observed_at,
            ),
        )
    }

    pub fn validate_contract(&self) -> bool {
        self.source.validate().is_ok()
            && self.producer.validate_contract()
            && is_digest(&self.source_transcript_digest)
            && is_digest(&self.learning_evidence_digest)
            && self
                .source
                .job_id(&self.learning_evidence_digest)
                .is_ok_and(|id| id == self.source_job_id)
            && (self.adoption_ordinal as usize) < MAX_METHOD_LEARNING_SOURCE_REFS
            && (self.evaluation_ordinal as usize) < MAX_METHOD_LEARNING_SOURCE_REFS
            && self.adoption.validate_contract()
            && self.evaluation.validate_contract()
            && self.adoption.method == self.evaluation.method
            && self.adoption.task_id == self.evaluation.task_id
            && self.adoption.method.memory_space_id == self.source.memory_space_id
            && self.adoption.method.mounted_subject_id == self.source.mounted_subject_id
            && is_digest(&self.method_content_digest)
            && self.observed_at > 0
            && self.execution_refs.len() == self.adoption.observation_refs.len()
            && self.execution_refs.iter().all(|reference| {
                reference.validate_contract() && reference.source_job_id == self.source_job_id
            })
            && self
                .execution_refs
                .windows(2)
                .all(|pair| pair[0].contribution_id < pair[1].contribution_id)
            && self
                .identity_digest()
                .is_ok_and(|id| id == self.contribution_id)
            && self
                .canonical_digest()
                .is_ok_and(|digest| digest == self.content_digest)
    }

    pub fn reference(&self) -> Result<MethodTaskContributionRefV1> {
        if !self.validate_contract() {
            return Err(invalid());
        }
        Ok(MethodTaskContributionRefV1 {
            contribution_id: self.contribution_id.clone(),
            content_digest: self.content_digest.clone(),
            source_job_id: self.source_job_id.clone(),
            method: self.adoption.method.clone(),
            method_content_digest: self.method_content_digest.clone(),
        })
    }

    pub fn validate_against(
        &self,
        job: &super::ProceduralFeedbackJobV2,
        transcript: &crate::memory::TranscriptTurnRecord,
        adopted_material: &crate::skills::AgentToolExperienceRevisionMaterialV4,
    ) -> Result<()> {
        if &Self::build(
            job,
            transcript,
            self.adoption_ordinal,
            self.evaluation_ordinal,
            adopted_material,
        )? != self
        {
            return Err(invalid());
        }
        Ok(())
    }
}

/// The caller supplies only currently authorized sources from verified ledger
/// closures. Missing withdrawn references are removed, not counted forever.
pub fn reduce_agent_tool_method_task_contributions(
    scope: &super::ProceduralSubjectScopeV1,
    tool: &super::ProceduralProducerToolV1,
    body: &crate::skills::AgentToolExperienceBodyV1,
    contributions: &[MethodTaskContributionV1],
    execution: &[super::ToolExecutionContributionV1],
    previous_status: Option<crate::skills::AgentToolExperienceStatus>,
    max_references: usize,
) -> std::result::Result<
    (
        crate::skills::AgentToolExperienceBodyV1,
        crate::skills::AgentToolExperienceStatus,
    ),
    super::ProceduralContributionErrorV1,
> {
    use super::ProceduralContributionErrorV1 as E;
    use crate::skills::{AgentToolExperienceBodyV1 as Body, AgentToolExperienceStatus as Status};
    use std::collections::BTreeMap;
    if !scope.validate_contract() || !tool.validate_contract() || !body.validate_contract() {
        return Err(E::NonCanonical);
    }
    if body.contribution_count() > max_references
        || contributions.len() > max_references
        || execution.len() > max_references
    {
        return Err(E::CapacityBlocked);
    }
    let mut facts = BTreeMap::new();
    for fact in execution {
        if !fact.validate_contract() {
            return Err(E::NonCanonical);
        }
        if fact.source.memory_space_id != scope.memory_space_id
            || fact.source.mounted_subject_id != scope.mounted_subject_id
            || fact.tool != *tool
        {
            return Err(E::ScopeMismatch);
        }
        if facts
            .insert(&fact.contribution_id, fact)
            .is_some_and(|old| old != fact)
        {
            return Err(E::IdentityConflict);
        }
    }
    let mut exact = BTreeMap::new();
    for contribution in contributions {
        if !contribution.validate_contract() {
            return Err(E::NonCanonical);
        }
        let method = &contribution.adoption.method;
        if method.memory_space_id != scope.memory_space_id
            || method.mounted_subject_id != scope.mounted_subject_id
            || method.registry_ref != tool.registry_ref
            || method.tool_id != tool.tool_id
            || method.schema_fingerprint != tool.schema_fingerprint
        {
            return Err(E::ScopeMismatch);
        }
        if exact
            .insert(&contribution.contribution_id, contribution)
            .is_some_and(|old| old != contribution)
        {
            return Err(E::IdentityConflict);
        }
    }
    let mut reduced = body.clone();
    let Body::Method {
        usage_contributions,
        ..
    } = &mut reduced
    else {
        return Err(E::NonCanonical);
    };
    let mut passed_evaluations: Vec<&MethodTaskEvaluationV1> = Vec::new();
    let mut failed = false;
    let mut retained = Vec::new();
    for reference in usage_contributions.iter() {
        let Some(contribution) = exact.get(&reference.contribution_id) else {
            continue;
        };
        if contribution.reference().map_err(|_| E::NonCanonical)? != *reference {
            return Err(E::IdentityConflict);
        }
        let mut actual_execution = !contribution.execution_refs.is_empty();
        for execution_ref in &contribution.execution_refs {
            let Some(fact) = facts.get(&execution_ref.contribution_id) else {
                actual_execution = false;
                continue;
            };
            if fact.reference()? != *execution_ref {
                return Err(E::IdentityConflict);
            }
            actual_execution &= fact.producer == contribution.producer
                && fact.source == contribution.source
                && contribution
                    .adoption
                    .observation_refs
                    .contains(&fact.fact.observation_id)
                && fact.fact.source_sensitivity == ProceduralSourceSensitivity::NonPrivate
                && matches!(
                    fact.fact.outcome,
                    super::ToolExecutionOutcome::Succeeded
                        | super::ToolExecutionOutcome::Failed
                        | super::ToolExecutionOutcome::Partial
                );
        }
        if !actual_execution {
            continue;
        }
        retained.push(reference.clone());
        if contribution.delivery_status != MemoryTurnDeliveryStatus::Delivered
            || !contribution.adoption.adoption.is_adopted()
        {
            continue;
        }
        let evaluation = &contribution.evaluation;
        failed |= matches!(
            evaluation.outcome,
            MethodTaskOutcomeV1::Failed | MethodTaskOutcomeV1::Partial
        );
        if evaluation.outcome == MethodTaskOutcomeV1::Passed {
            passed_evaluations.push(evaluation);
        }
    }
    *usage_contributions = retained;
    let status = match previous_status {
        Some(previous @ (Status::Stale | Status::Rejected)) => previous,
        _ if failed => Status::Stale,
        _ if passed_evaluations.iter().enumerate().any(|(index, first)| {
            passed_evaluations[index + 1..]
                .iter()
                .any(|second| first.independent_of(second))
        }) =>
        {
            Status::Active
        }
        _ => Status::Candidate,
    };
    if !reduced.validate_contract() {
        return Err(E::NonCanonical);
    }
    Ok((reduced, status))
}

impl super::PostTurnLearningInputV3 {
    pub fn validate_production_contract(&self) -> bool {
        let count = self
            .method_learning_inputs
            .iter()
            .try_fold(0usize, |count, input| {
                count.checked_add(input.materials.len())
            });
        count.is_some_and(|count| count <= MAX_METHOD_LEARNING_MATERIALS)
            && self
                .method_learning_inputs
                .iter()
                .all(MethodLearningInputV1::validate_contract)
            && super::canonical_order(&self.method_learning_inputs)
            && self.method_adoptions.len() + self.method_task_evaluations.len()
                <= MAX_METHOD_LEARNING_SOURCE_REFS
            && self
                .method_adoptions
                .iter()
                .all(MethodAdoptionEvidenceV1::validate_contract)
            && self
                .method_task_evaluations
                .iter()
                .all(MethodTaskEvaluationV1::validate_contract)
            && super::canonical_order(&self.method_adoptions)
            && super::canonical_order(&self.method_task_evaluations)
    }
}

impl super::PostTurnLearningEvidenceV3 {
    /// Reconstruct usage from the admitted canonical turn and exact prior
    /// materials. Producer/evaluator authority is authenticated by the caller;
    /// neither a model proposal nor an aggregate success count is accepted here.
    pub fn method_task_contributions_for_job(
        &self,
        job: &super::ProceduralFeedbackJobV2,
        transcript: &crate::memory::TranscriptTurnRecord,
        mut resolve: impl FnMut(
            &MethodContentRefV1,
        ) -> Result<crate::skills::AgentToolExperienceRevisionMaterialV4>,
    ) -> Result<Vec<MethodTaskContributionV1>> {
        if transcript.learning_evidence.as_ref() != Some(self) {
            return Err(invalid());
        }
        let mut contributions = Vec::new();
        for (evaluation_ordinal, evaluation) in self.method_task_evaluations.iter().enumerate() {
            let mut matching = self
                .method_adoptions
                .iter()
                .enumerate()
                .filter(|(_, adoption)| {
                    adoption.method == evaluation.method && adoption.task_id == evaluation.task_id
                });
            let Some((adoption_ordinal, adoption)) = matching.next() else {
                return Err(invalid());
            };
            if matching.next().is_some() {
                return Err(invalid());
            }
            let material = resolve(&adoption.method)?;
            contributions.push(MethodTaskContributionV1::build(
                job,
                transcript,
                adoption_ordinal as u32,
                evaluation_ordinal as u32,
                &material,
            )?);
        }
        contributions.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
        if !contributions
            .windows(2)
            .all(|pair| pair[0].contribution_id < pair[1].contribution_id)
        {
            return Err(invalid());
        }
        Ok(contributions)
    }
    /// Input groups, not tool calls, materials, or changed owners. Material-only
    /// semantic work must never create an empty procedural feedback job.
    pub fn procedural_submission_count(&self) -> Option<u32> {
        [
            self.runtime_skill_feedback.len(),
            self.agent_skill_feedback.len(),
            self.task_learning_feedback.len(),
            self.agent_tool_feedback.len(),
            self.method_adoptions.len(),
            self.method_task_evaluations.len(),
        ]
        .into_iter()
        .try_fold(0u32, |count, length| {
            count.checked_add(u32::try_from(length).ok()?)
        })
    }

    pub fn has_authorized_content(&self) -> bool {
        self.has_procedural_work() || !self.method_learning_inputs.is_empty()
    }

    pub fn has_procedural_work(&self) -> bool {
        !self.runtime_skill_feedback.is_empty()
            || !self.agent_skill_feedback.is_empty()
            || !self.task_learning_feedback.is_empty()
            || !self.agent_tool_feedback.is_empty()
            || !self.method_adoptions.is_empty()
            || !self.method_task_evaluations.is_empty()
    }

    pub(super) fn production_payload_is_valid(&self) -> bool {
        let count = self
            .method_learning_inputs
            .iter()
            .try_fold(0usize, |count, input| {
                count.checked_add(input.submitted_material_count as usize)
            });
        let matching_scope = |method: &MethodContentRefV1| {
            method.memory_space_id == self.memory_space_id
                && method.mounted_subject_id == self.mounted_subject_id
        };
        count.is_some_and(|count| count <= MAX_METHOD_LEARNING_MATERIALS)
            && self
                .method_learning_inputs
                .iter()
                .all(AdmittedMethodLearningInputV1::validate_contract)
            && self.method_adoptions.len() + self.method_task_evaluations.len()
                <= MAX_METHOD_LEARNING_SOURCE_REFS
            && self
                .method_adoptions
                .iter()
                .all(|item| item.validate_contract() && matching_scope(&item.method))
            && self
                .method_task_evaluations
                .iter()
                .all(|item| item.validate_contract() && matching_scope(&item.method))
            && super::canonical_order(&self.method_adoptions)
            && super::canonical_order(&self.method_task_evaluations)
    }
}

fn production_digest<T: Serialize>(domain: &str, value: &T) -> Result<String> {
    let encoded = serde_json::to_vec(value).map_err(|_| invalid())?;
    Ok(super::domain_digest(domain, &[&encoded]))
}

fn invalid() -> Error {
    Error::invalid_input(
        "production_method_learning",
        "method learning contract or source admission is invalid",
    )
}

#[cfg(test)]
#[path = "production_tests.rs"]
mod tests;
