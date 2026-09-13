//! Subject-wide validity of learned procedural state. This root owns neither
//! assets nor permissions: SDK proves authority, Store closes the real control
//! transaction and the single procedural worker's exact job/receipt.

use serde::{Deserialize, Serialize};

use super::evidence::identifier;
use super::{domain_digest, is_digest, ProceduralProducerRevisionRefV1, ProceduralProducerScopeV1};
use crate::memory::{
    ConversationKey, GovernedOwnerRevisionRef, MemoryMutationOperationIdentity,
    MemoryMutationOperationKind, TranscriptLifecycleState, TranscriptLifecycleTransition,
};
use crate::{Error, Result};

pub const MAX_PROCEDURAL_SUBJECT_SCOPES: usize = 256;

/// Binds a persisted scan page to the existing immutable mutation receipt.
/// The identity commits to the entire claimed preimage; the receipt intent
/// additionally commits to the resulting checkpoint, not its mutable wrapper.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralReconciliationPageAuthorityV1 {
    pub operation: MemoryMutationOperationIdentity,
    pub claimed_job_digest: String,
}

impl ProceduralReconciliationPageAuthorityV1 {
    pub fn for_claim(job: &super::ProceduralFeedbackJobV2, actor: &str) -> Result<Self> {
        job.validate()?;
        if !matches!(job.work, super::ProceduralLearningWorkV1::Reconcile { .. })
            || job.status != super::ProceduralFeedbackJobStatusV1::Leased
        {
            return Err(invalid());
        }
        let bytes = serde_json::to_vec(job).map_err(|_| invalid())?;
        let claimed_job_digest = domain_digest("procedural_reconciliation_claim_v1", &[&bytes]);
        let scope = job.work.subject_scope();
        let operation = MemoryMutationOperationIdentity::new(
            Self::operation_id_for(&claimed_job_digest),
            scope.memory_space_id,
            scope.mounted_subject_id,
            actor,
            MemoryMutationOperationKind::ProceduralLearning,
        )?;
        Ok(Self {
            operation,
            claimed_job_digest,
        })
    }

    fn operation_id_for(digest: &str) -> String {
        format!("reconcile-page-{digest}")
    }

    pub fn operation_id(&self) -> String {
        Self::operation_id_for(&self.claimed_job_digest)
    }

    pub fn validate_for(&self, scope: &ProceduralSubjectScopeV1) -> Result<()> {
        self.operation.validate_contract()?;
        if !is_digest(&self.claimed_job_digest)
            || self.operation
                != MemoryMutationOperationIdentity::new(
                    self.operation_id(),
                    &scope.memory_space_id,
                    &scope.mounted_subject_id,
                    self.operation.actor_subject_id(),
                    MemoryMutationOperationKind::ProceduralLearning,
                )?
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn intent_digest(
        &self,
        checkpoint: &ProceduralReconciliationCheckpointV1,
    ) -> Result<String> {
        if !is_digest(&self.claimed_job_digest)
            || checkpoint.content_digest != checkpoint.canonical_digest()?
        {
            return Err(invalid());
        }
        Ok(domain_digest(
            "procedural_reconciliation_page_intent_v1",
            &[
                self.claimed_job_digest.as_bytes(),
                checkpoint.content_digest.as_bytes(),
            ],
        ))
    }
}

/// Immutable proof of initialization, never current authority or an inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralSubjectInitializationV1 {
    pub schema_version: u32,
    pub physical_key: String,
    pub scope: ProceduralSubjectScopeV1,
    pub first_producer: ProceduralProducerRevisionRefV1,
    pub content_digest: String,
}

impl ProceduralSubjectInitializationV1 {
    pub fn key(scope: &ProceduralSubjectScopeV1) -> Result<String> {
        Ok(format!("{}:initialization", scope.root_key()?))
    }

    pub fn new(
        scope: ProceduralSubjectScopeV1,
        producer: &super::ProceduralProducerBindingV1,
    ) -> Result<Self> {
        let mut material = Self {
            schema_version: 1,
            physical_key: Self::key(&scope)?,
            scope,
            first_producer: producer.revision_ref()?,
            content_digest: String::new(),
        };
        material.content_digest = material.digest()?;
        material.validate_producer(producer)?;
        Ok(material)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.physical_key != Self::key(&self.scope)?
            || !self.first_producer.validate_contract()
            || self.first_producer.revision != 1
            || self.content_digest != self.digest()?
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn validate_producer(&self, producer: &super::ProceduralProducerBindingV1) -> Result<()> {
        self.validate()?;
        if !producer.validate_contract()
            || !self.scope.contains(&producer.spec.scope)
            || producer.revision_ref()? != self.first_producer
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn digest(&self) -> Result<String> {
        let bytes = serde_json::to_vec(&(
            self.schema_version,
            &self.physical_key,
            &self.scope,
            &self.first_producer,
        ))
        .map_err(|_| invalid())?;
        Ok(domain_digest(
            "procedural_subject_initialization_v1",
            &[&bytes],
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralSubjectScopeV1 {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
}

impl ProceduralSubjectScopeV1 {
    pub fn validate_contract(&self) -> bool {
        identifier(&self.memory_space_id) && identifier(&self.mounted_subject_id)
    }

    pub fn root_key(&self) -> Result<String> {
        if !self.validate_contract() {
            return Err(invalid());
        }
        Ok(format!(
            "procedural_subject_validity:{}",
            domain_digest(
                "procedural_subject_validity_key_v1",
                &[
                    self.memory_space_id.as_bytes(),
                    self.mounted_subject_id.as_bytes()
                ]
            )
        ))
    }

    fn contains(&self, scope: &ProceduralProducerScopeV1) -> bool {
        scope.validate_contract()
            && scope.memory_space_id == self.memory_space_id
            && scope.mounted_subject_id == self.mounted_subject_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralSourcePostImageV1 {
    Present {
        revision: GovernedOwnerRevisionRef,
        content_digest: String,
    },
    Terminated {
        revision: GovernedOwnerRevisionRef,
        content_digest: String,
        termination: crate::memory::GovernedOwnerTermination,
        control: crate::memory::LongTermMemoryVersionTransitionBinding,
    },
    Deleted,
}

impl ProceduralSourcePostImageV1 {
    pub fn material(&self) -> Option<(&GovernedOwnerRevisionRef, &str)> {
        match self {
            Self::Present {
                revision,
                content_digest,
            }
            | Self::Terminated {
                revision,
                content_digest,
                ..
            } => Some((revision, content_digest)),
            Self::Deleted => None,
        }
    }

    pub fn validates_owner(&self, owner: &crate::memory::GovernedMemoryOwnerRef) -> bool {
        if !owner.is_valid()
            || !matches!(
                owner.owner_plane,
                crate::memory::GovernedMemoryOwnerPlane::LongTerm
                    | crate::memory::GovernedMemoryOwnerPlane::EvidenceDocument
            )
        {
            return false;
        }
        match self {
            Self::Deleted => true,
            Self::Present {
                revision,
                content_digest,
            } => revision.is_valid() && revision.owner_ref == *owner && is_digest(content_digest),
            Self::Terminated {
                revision,
                content_digest,
                termination,
                control,
            } => {
                revision.is_valid()
                    && revision.owner_ref == *owner
                    && is_digest(content_digest)
                    && matches!(
                        termination,
                        crate::memory::GovernedOwnerTermination::Invalidated
                            | crate::memory::GovernedOwnerTermination::Superseded
                    )
                    && crate::memory::LongTermMemoryVersionTransitionBinding::new(
                        revision.clone(),
                        control.control_revision_physical_key.clone(),
                        control.control_revision_content_digest.clone(),
                    )
                    .is_ok_and(|canonical| canonical == *control)
            }
        }
    }
}

/// Causal evidence, not a caller's authorization. An ordinary source write has
/// its real transaction identity, not a fabricated user operation or turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralReconciliationTriggerV1 {
    ProducerControl {
        operation: MemoryMutationOperationIdentity,
        before: ProceduralProducerRevisionRefV1,
        after: ProceduralProducerRevisionRefV1,
    },
    SourceChange {
        transaction_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation: Option<MemoryMutationOperationIdentity>,
        changes: Vec<super::ProceduralSourceTransitionV1>,
    },
    RuntimeSkillControl {
        transaction_id: String,
        operation: Option<MemoryMutationOperationIdentity>,
        changes: Vec<ProceduralRuntimeSkillTransitionV1>,
    },
    TranscriptLifecycle {
        transaction_id: String,
        operation: MemoryMutationOperationIdentity,
        key: ConversationKey,
        turn_id: String,
        sequence: u64,
        transition: TranscriptLifecycleTransition,
        before_state: TranscriptLifecycleState,
        after_state: TranscriptLifecycleState,
        before_content_digest: String,
        after_content_digest: String,
    },
}

impl ProceduralReconciliationTriggerV1 {
    fn validates_for(&self, scope: &ProceduralSubjectScopeV1) -> bool {
        match self {
            Self::RuntimeSkillControl {
                transaction_id,
                operation,
                changes,
            } => {
                identifier(transaction_id)
                    && !changes.is_empty()
                    && changes.len() <= super::MAX_PROCEDURAL_EVIDENCE_ITEMS
                    && changes.iter().all(|change| change.validates_for(scope))
                    && changes
                        .windows(2)
                        .all(|pair| pair[0].owner_ref() < pair[1].owner_ref())
                    && operation.as_ref().is_none_or(|operation| {
                        operation.validate_contract().is_ok()
                            && operation.memory_space_id() == scope.memory_space_id
                            && operation.mounted_subject_id() == scope.mounted_subject_id
                    })
            }
            Self::ProducerControl {
                operation,
                before,
                after,
            } => {
                operation.validate_contract().is_ok()
                    && operation.operation_kind()
                        == MemoryMutationOperationKind::ProceduralProducerControl
                    && operation.memory_space_id() == scope.memory_space_id
                    && operation.mounted_subject_id() == scope.mounted_subject_id
                    && before.validate_contract()
                    && after.validate_contract()
                    && before.binding_key == after.binding_key
                    && before.revision.checked_add(1) == Some(after.revision)
            }
            Self::SourceChange {
                transaction_id,
                operation,
                changes,
            } => {
                identifier(transaction_id)
                    && !changes.is_empty()
                    && changes.len() <= super::MAX_PROCEDURAL_SOURCE_DEPENDENTS
                    && changes.iter().all(|change| change.validate().is_ok())
                    && changes
                        .windows(2)
                        .all(|pair| pair[0].owner_ref < pair[1].owner_ref)
                    && operation.as_ref().is_none_or(|identity| {
                        identity.validate_contract().is_ok()
                            && identity.memory_space_id() == scope.memory_space_id
                            && match identity.operation_kind() {
                                MemoryMutationOperationKind::Write => true,
                                MemoryMutationOperationKind::LongTermControl { .. } => {
                                    changes.iter().all(|change| {
                                        change.owner_ref.owner_plane
                                            == crate::memory::GovernedMemoryOwnerPlane::LongTerm
                                    })
                                }
                                _ => false,
                            }
                    })
            }
            Self::TranscriptLifecycle {
                transaction_id,
                operation,
                key,
                turn_id,
                sequence,
                transition,
                before_state,
                after_state,
                before_content_digest,
                after_content_digest,
            } => {
                identifier(transaction_id)
                    && operation.validate_contract().is_ok()
                    && operation.operation_kind()
                        == MemoryMutationOperationKind::ProceduralLifecycle
                    && operation.memory_space_id() == scope.memory_space_id
                    && operation.mounted_subject_id() == scope.mounted_subject_id
                    && key.memory_space_id == scope.memory_space_id
                    && identifier(&key.channel_id)
                    && identifier(&key.conversation_id)
                    && identifier(turn_id)
                    && *sequence > 0
                    && is_digest(before_content_digest)
                    && is_digest(after_content_digest)
                    && before_content_digest != after_content_digest
                    && match transition {
                        TranscriptLifecycleTransition::Mask => {
                            matches!(
                                before_state,
                                TranscriptLifecycleState::Active
                                    | TranscriptLifecycleState::Archived
                            ) && *after_state == TranscriptLifecycleState::Masked
                        }
                        TranscriptLifecycleTransition::DeleteRaw => {
                            *before_state != TranscriptLifecycleState::RawDeleted
                                && *after_state == TranscriptLifecycleState::RawDeleted
                        }
                        TranscriptLifecycleTransition::Archive
                        | TranscriptLifecycleTransition::Restore => false,
                    }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralRuntimeSkillTransitionV1 {
    pub before: Option<crate::skills::RuntimeSkillOwnerBinding>,
    pub after: Option<crate::skills::RuntimeSkillOwnerBinding>,
}

impl ProceduralRuntimeSkillTransitionV1 {
    pub fn owner_ref(&self) -> Option<&crate::memory::GovernedMemoryOwnerRef> {
        self.before
            .as_ref()
            .or(self.after.as_ref())
            .map(|binding| &binding.owner_ref)
    }

    pub fn validates_for(&self, scope: &ProceduralSubjectScopeV1) -> bool {
        let owns = |binding: &crate::skills::RuntimeSkillOwnerBinding| {
            binding.validate_for(
                &scope.memory_space_id,
                &crate::skills::RuntimeSkillOwningScope::Subject {
                    mounted_subject_id: scope.mounted_subject_id.clone(),
                },
            )
        };
        match (&self.before, &self.after) {
            (None, None) => false,
            (None, Some(after)) => owns(after) && after.owner_revision == 1,
            (Some(before), None) => owns(before),
            (Some(before), Some(after)) => {
                owns(before)
                    && owns(after)
                    && before.owner_ref == after.owner_ref
                    && before.owner_revision.checked_add(1) == Some(after.owner_revision)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralReconciliationWorkV1 {
    pub scope: ProceduralSubjectScopeV1,
    pub target_epoch: u64,
    pub trigger: ProceduralReconciliationTriggerV1,
}

impl ProceduralReconciliationWorkV1 {
    pub fn reference(&self) -> Result<ProceduralReconciliationWorkRefV1> {
        if !self.scope.validate_contract()
            || self.target_epoch <= 1
            || !self.trigger.validates_for(&self.scope)
        {
            return Err(invalid());
        }
        let encoded = serde_json::to_vec(self).map_err(|_| invalid())?;
        let content_digest = domain_digest("procedural_reconciliation_work_v1", &[&encoded]);
        Ok(ProceduralReconciliationWorkRefV1 {
            subject_root_key: self.scope.root_key()?,
            target_epoch: self.target_epoch,
            job_id: format!("procedural_feedback_job:{content_digest}"),
            content_digest,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralReconciliationWorkRefV1 {
    pub subject_root_key: String,
    pub target_epoch: u64,
    pub job_id: String,
    pub content_digest: String,
}

impl ProceduralReconciliationWorkRefV1 {
    fn validates_for(&self, root_key: &str, epoch: u64) -> bool {
        self.subject_root_key == root_key
            && self.target_epoch == epoch
            && epoch > 1
            && is_digest(&self.content_digest)
            && self.job_id == format!("procedural_feedback_job:{}", self.content_digest)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralReconciliationBlockV1 {
    Capacity,
    SourceUnavailable,
    RepairRequired,
}

/// The physical address is derived from the job's subject and the asset owner.
/// Absence is a pinned observation, never a substitute for an unreadable root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralReconciliationManifestPinV1 {
    Absent,
    Present {
        revision: u64,
        bindings_digest: String,
    },
}

impl ProceduralReconciliationManifestPinV1 {
    fn validate(&self) -> bool {
        match self {
            Self::Absent => true,
            Self::Present {
                revision,
                bindings_digest,
            } => *revision > 0 && is_digest(bindings_digest),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralReconciliationManifestPinsV1 {
    pub agent_tool: ProceduralReconciliationManifestPinV1,
    pub runtime_skill: ProceduralReconciliationManifestPinV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralReconciliationCursorV1 {
    AgentTool {
        after: Option<GovernedOwnerRevisionRef>,
    },
    RuntimeSkill {
        after: Option<GovernedOwnerRevisionRef>,
    },
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralReconciliationReadDispositionV1 {
    Visible,
    Withdrawn,
}

/// A decision applies to one exact material, including unchanged and historical
/// material. A valid current head never grants all its history permission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralReconciliationOwnerProofV1 {
    pub binding: super::ProceduralAppliedOwnerBindingV1,
    pub disposition: ProceduralReconciliationReadDispositionV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralReconciliationDependencyPinV1 {
    Producer {
        original_revision: ProceduralProducerRevisionRefV1,
        current_revision: ProceduralProducerRevisionRefV1,
        permitted: bool,
    },
    Source {
        owner_ref: crate::memory::GovernedMemoryOwnerRef,
        post_image: ProceduralSourcePostImageV1,
        owner_state_digest: String,
    },
    Transcript {
        identity: super::ProceduralFeedbackIdentityV1,
        sequence: u64,
        content_digest: String,
        lifecycle: TranscriptLifecycleState,
    },
}

impl ProceduralReconciliationDependencyPinV1 {
    fn validate_for(&self, scope: &ProceduralSubjectScopeV1) -> bool {
        match self {
            Self::Producer {
                original_revision,
                current_revision,
                ..
            } => {
                original_revision.validate_contract()
                    && current_revision.validate_contract()
                    && original_revision.binding_key == current_revision.binding_key
                    && original_revision.revision <= current_revision.revision
                    && (original_revision.revision != current_revision.revision
                        || original_revision == current_revision)
            }
            Self::Source {
                owner_ref,
                post_image,
                owner_state_digest,
            } => {
                owner_ref.is_valid()
                    && is_digest(owner_state_digest)
                    && matches!(
                        owner_ref.owner_plane,
                        crate::memory::GovernedMemoryOwnerPlane::LongTerm
                            | crate::memory::GovernedMemoryOwnerPlane::EvidenceDocument
                    )
                    && post_image.validates_owner(owner_ref)
            }
            Self::Transcript {
                identity,
                sequence,
                content_digest,
                ..
            } => {
                identity.validate().is_ok()
                    && identity.memory_space_id == scope.memory_space_id
                    && identity.mounted_subject_id == scope.mounted_subject_id
                    && *sequence > 0
                    && is_digest(content_digest)
            }
        }
    }
}

/// Includes lifecycle and erasure post-images. The immutable intake digest is
/// deliberately insufficient for fencing a later mask or raw deletion.
pub fn procedural_transcript_state_digest(
    record: &crate::memory::TranscriptTurnRecord,
) -> Result<String> {
    Ok(domain_digest(
        "procedural_transcript_state_v1",
        &[&serde_json::to_vec(record).map_err(|_| invalid())?],
    ))
}

/// Durable progress in the existing job, not a second asset/permission owner.
/// Store additionally checks exact inventory coverage, source images and the
/// runtime's cumulative count/byte bounds before accepting any page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralReconciliationCheckpointV1 {
    pub schema_version: u32,
    pub work_digest: String,
    pub root_revision: u64,
    pub root_digest: String,
    pub original_manifests: ProceduralReconciliationManifestPinsV1,
    pub current_manifests: ProceduralReconciliationManifestPinsV1,
    pub cursor: ProceduralReconciliationCursorV1,
    pub verified_owners: Vec<ProceduralReconciliationOwnerProofV1>,
    pub dependencies: Vec<ProceduralReconciliationDependencyPinV1>,
    pub page_number: u64,
    pub updated_at: u64,
    pub content_digest: String,
}

impl ProceduralReconciliationCheckpointV1 {
    pub fn begin(
        work: &ProceduralReconciliationWorkV1,
        root: &ProceduralSubjectValidityRootV1,
        manifests: ProceduralReconciliationManifestPinsV1,
        now: u64,
    ) -> Result<Self> {
        root.require_active_work(work, MAX_PROCEDURAL_SUBJECT_SCOPES)?;
        let mut checkpoint = Self {
            schema_version: 1,
            work_digest: work.reference()?.content_digest,
            root_revision: root.revision,
            root_digest: root.content_digest.clone(),
            original_manifests: manifests.clone(),
            current_manifests: manifests,
            cursor: ProceduralReconciliationCursorV1::AgentTool { after: None },
            verified_owners: Vec::new(),
            dependencies: Vec::new(),
            page_number: 0,
            updated_at: now,
            content_digest: String::new(),
        };
        checkpoint.content_digest = checkpoint.canonical_digest()?;
        checkpoint.validate_for(work)?;
        Ok(checkpoint)
    }

    pub fn canonical_digest(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.content_digest.clear();
        Ok(domain_digest(
            "procedural_reconciliation_checkpoint_v1",
            &[&serde_json::to_vec(&canonical).map_err(|_| invalid())?],
        ))
    }

    pub fn validate_for(&self, work: &ProceduralReconciliationWorkV1) -> Result<()> {
        // JSON order alone cannot reject contradictory observations of one
        // logical source. Keep a single head pin across all retained originals.
        let mut identities = std::collections::BTreeSet::new();
        let mut producer_heads = std::collections::BTreeMap::new();
        for dependency in &self.dependencies {
            let identity = match dependency {
                ProceduralReconciliationDependencyPinV1::Producer {
                    original_revision,
                    current_revision,
                    ..
                } => {
                    if producer_heads
                        .insert(&original_revision.binding_key, current_revision)
                        .is_some_and(|previous| previous != current_revision)
                    {
                        return Err(invalid());
                    }
                    serde_json::to_vec(&(
                        "producer",
                        &original_revision.binding_key,
                        original_revision.revision,
                    ))
                }
                ProceduralReconciliationDependencyPinV1::Source { owner_ref, .. } => {
                    serde_json::to_vec(&("source", owner_ref))
                }
                ProceduralReconciliationDependencyPinV1::Transcript { identity, .. } => {
                    serde_json::to_vec(&("transcript", identity))
                }
            }
            .map_err(|_| invalid())?;
            if !identities.insert(identity) {
                return Err(invalid());
            }
        }
        let cursor_valid = match &self.cursor {
            ProceduralReconciliationCursorV1::AgentTool { after } => {
                after.as_ref().is_none_or(|owner| {
                    owner.is_valid()
                        && owner.owner_ref.owner_plane
                            == crate::memory::GovernedMemoryOwnerPlane::AgentToolExperience
                })
            }
            ProceduralReconciliationCursorV1::RuntimeSkill { after } => {
                after.as_ref().is_none_or(|owner| {
                    owner.is_valid()
                        && owner.owner_ref.owner_plane
                            == crate::memory::GovernedMemoryOwnerPlane::RuntimeSkill
                })
            }
            ProceduralReconciliationCursorV1::Complete => self.page_number > 0,
        };
        if self.schema_version != 1
            || self.work_digest != work.reference()?.content_digest
            || self.root_revision < work.target_epoch
            || !is_digest(&self.root_digest)
            || self.updated_at == 0
            || !cursor_valid
            || [
                &self.original_manifests.agent_tool,
                &self.original_manifests.runtime_skill,
                &self.current_manifests.agent_tool,
                &self.current_manifests.runtime_skill,
            ]
            .into_iter()
            .any(|pin| !pin.validate())
            || self
                .verified_owners
                .iter()
                .any(|proof| !proof.binding.validate_for_subject(&work.scope))
            || !self.verified_owners.windows(2).all(|pair| {
                pair[0].binding.owner_revision_ref() < pair[1].binding.owner_revision_ref()
            })
            || self
                .dependencies
                .iter()
                .any(|pin| !pin.validate_for(&work.scope))
            || !super::canonical_order(&self.dependencies)
            || (self.page_number == 0
                && (!self.verified_owners.is_empty()
                    || !self.dependencies.is_empty()
                    || self.original_manifests != self.current_manifests
                    || self.cursor
                        != (ProceduralReconciliationCursorV1::AgentTool { after: None })))
            || self.content_digest != self.canonical_digest()?
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn permits_exact_read(
        &self,
        work: &ProceduralReconciliationWorkV1,
        binding: &super::ProceduralAppliedOwnerBindingV1,
    ) -> Result<bool> {
        self.validate_for(work)?;
        Ok(
            matches!(self.cursor, ProceduralReconciliationCursorV1::Complete)
                && self.verified_owners.iter().any(|proof| {
                    &proof.binding == binding
                        && proof.disposition == ProceduralReconciliationReadDispositionV1::Visible
                }),
        )
    }

    pub(super) fn is_successor_of(&self, before: Option<&Self>) -> bool {
        let Some(before) = before else {
            return self.page_number == 1;
        };
        let cursor_follows = match (&before.cursor, &self.cursor) {
            (
                ProceduralReconciliationCursorV1::AgentTool { after: previous },
                ProceduralReconciliationCursorV1::AgentTool { after: next },
            )
            | (
                ProceduralReconciliationCursorV1::RuntimeSkill { after: previous },
                ProceduralReconciliationCursorV1::RuntimeSkill { after: next },
            ) => next > previous,
            (
                ProceduralReconciliationCursorV1::AgentTool { .. },
                ProceduralReconciliationCursorV1::RuntimeSkill { .. },
            )
            | (
                ProceduralReconciliationCursorV1::RuntimeSkill { .. },
                ProceduralReconciliationCursorV1::Complete,
            ) => true,
            _ => false,
        };
        before.page_number.checked_add(1) == Some(self.page_number)
            && self.work_digest == before.work_digest
            && self.root_revision == before.root_revision
            && self.root_digest == before.root_digest
            && self.original_manifests == before.original_manifests
            && self.updated_at >= before.updated_at
            && cursor_follows
            && before
                .verified_owners
                .iter()
                .all(|proof| self.verified_owners.contains(proof))
            && before
                .dependencies
                .iter()
                .all(|pin| self.dependencies.contains(pin))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralReconciliationCompletionV1 {
    pub work: ProceduralReconciliationWorkRefV1,
    pub receipt_digest: String,
}

/// Proof produced by the same operation-aware transaction owner as feedback.
/// No feedback counts or invented Transcript commitments belong to this result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralReconciliationReceiptV1 {
    pub schema_version: u32,
    pub work: ProceduralReconciliationWorkRefV1,
    pub operation_id: String,
    pub transaction_id: String,
    pub mutation_receipt_key: String,
    pub plan_digest: String,
    pub post_image_digest: String,
    pub checkpoint_digest: String,
    pub completed_at: u64,
}

impl ProceduralReconciliationReceiptV1 {
    pub fn validate_contract(&self) -> bool {
        self.schema_version == 1
            && self
                .work
                .validates_for(&self.work.subject_root_key, self.work.target_epoch)
            && identifier(&self.operation_id)
            && identifier(&self.transaction_id)
            && self.completed_at > 0
            && [
                &self.mutation_receipt_key,
                &self.plan_digest,
                &self.post_image_digest,
                &self.checkpoint_digest,
            ]
            .into_iter()
            .all(|value| is_digest(value))
    }

    pub fn canonical_digest(&self) -> Result<String> {
        if !self.validate_contract() {
            return Err(invalid());
        }
        Ok(domain_digest(
            "procedural_reconciliation_receipt_v1",
            &[&serde_json::to_vec(self).map_err(|_| invalid())?],
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralSubjectValidityStateV1 {
    Ready {
        completion: Option<ProceduralReconciliationCompletionV1>,
    },
    Reconciling {
        work: ProceduralReconciliationWorkRefV1,
    },
    Blocked {
        work: ProceduralReconciliationWorkRefV1,
        reason: ProceduralReconciliationBlockV1,
    },
}

/// Safe request-local availability, never a grant or persisted state copy.
/// Counts derived from a scope are unavailable unless its read is Ready.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralLearningReadAvailabilityV1 {
    Ready,
    Reconciling,
    Blocked,
    SubjectUnavailable,
    ProfileUnavailable,
    #[default]
    NotMaterialized,
}

impl ProceduralLearningReadAvailabilityV1 {
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    pub const fn reason(self) -> &'static str {
        match self {
            Self::Ready => "procedural_learning_ready",
            Self::Reconciling => "procedural_learning_reconciling",
            Self::Blocked => "procedural_learning_blocked",
            Self::SubjectUnavailable => "procedural_learning_subject_unavailable",
            Self::ProfileUnavailable => "procedural_learning_profile_unavailable",
            Self::NotMaterialized => "procedural_learning_not_materialized",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralSubjectValidityRootV1 {
    pub schema_version: u32,
    pub physical_key: String,
    pub scope: ProceduralSubjectScopeV1,
    pub revision: u64,
    pub validity_epoch: u64,
    pub state: ProceduralSubjectValidityStateV1,
    /// Immutable discovery/retention references; mutable lease status lives only
    /// in the jobs. Advancing an epoch must never orphan previous work.
    pub retained_work: Vec<ProceduralReconciliationWorkRefV1>,
    pub producer_scopes: Vec<ProceduralProducerScopeV1>,
    pub content_digest: String,
}

/// Request-local interpretation of existing durable proofs. This is not a
/// serializable grant or another persisted allow-list. Store validates the
/// authoritative receipt/audit closure before supplying these exact records.
pub struct ProceduralReadAuthorityV1<'a> {
    root: &'a ProceduralSubjectValidityRootV1,
    completion: Option<&'a super::ProceduralFeedbackJobV2>,
    applications: &'a [super::ProceduralFeedbackApplicationLedgerV2],
}

impl<'a> ProceduralReadAuthorityV1<'a> {
    pub fn try_new(
        root: &'a ProceduralSubjectValidityRootV1,
        completion: Option<&'a super::ProceduralFeedbackJobV2>,
        applications: &'a [super::ProceduralFeedbackApplicationLedgerV2],
    ) -> Result<Self> {
        root.validate(MAX_PROCEDURAL_SUBJECT_SCOPES)?;
        match (&root.state, completion) {
            (
                ProceduralSubjectValidityStateV1::Ready {
                    completion: Some(expected),
                },
                Some(job),
            ) => {
                job.validate()?;
                let (
                    super::ProceduralLearningWorkV1::Reconcile { source },
                    Some(super::ProceduralLearningReceiptV1::Reconcile { receipt }),
                ) = (&job.work, &job.receipt)
                else {
                    return Err(invalid());
                };
                if job.status != super::ProceduralFeedbackJobStatusV1::Succeeded
                    || source.reference()? != expected.work
                    || source.scope != root.scope
                    || source.target_epoch != root.validity_epoch
                    || receipt.canonical_digest()? != expected.receipt_digest
                {
                    return Err(invalid());
                }
            }
            (
                ProceduralSubjectValidityStateV1::Ready {
                    completion: Some(_),
                },
                None,
            )
            | (_, Some(_)) => return Err(invalid()),
            (_, None) => {}
        }
        for application in applications {
            application.validate()?;
            if application.identity.memory_space_id != root.scope.memory_space_id
                || application.identity.mounted_subject_id != root.scope.mounted_subject_id
                || application.validity_epoch > root.validity_epoch
                || !root
                    .producer_scopes
                    .contains(&application.identity.producer_scope())
            {
                return Err(invalid());
            }
        }
        Ok(Self {
            root,
            completion,
            applications,
        })
    }

    pub fn scope(&self) -> &ProceduralSubjectScopeV1 {
        &self.root.scope
    }

    pub fn permits_exact(&self, binding: &super::ProceduralAppliedOwnerBindingV1) -> Result<bool> {
        if !binding.validate_for_subject(&self.root.scope) {
            return Err(invalid());
        }
        if !self
            .root
            .permits_learning_reads(MAX_PROCEDURAL_SUBJECT_SCOPES)?
        {
            return Ok(false);
        }
        if let Some(job) = self.completion {
            let checkpoint = job.checkpoint.as_ref().ok_or_else(invalid)?;
            if let Some(proof) = checkpoint
                .verified_owners
                .iter()
                .find(|proof| &proof.binding == binding)
            {
                return Ok(proof.disposition == ProceduralReconciliationReadDispositionV1::Visible);
            }
        }
        Ok(self.applications.iter().any(|application| {
            application.validity_epoch == self.root.validity_epoch
                && application.applied_owner_bindings.contains(binding)
        }))
    }
}

impl ProceduralSubjectValidityRootV1 {
    pub fn initialize(
        scope: ProceduralSubjectScopeV1,
        producer_scope: ProceduralProducerScopeV1,
        max_scopes: usize,
    ) -> Result<Self> {
        let mut root = Self {
            schema_version: 1,
            physical_key: scope.root_key()?,
            scope,
            revision: 1,
            validity_epoch: 1,
            state: ProceduralSubjectValidityStateV1::Ready { completion: None },
            retained_work: Vec::new(),
            producer_scopes: vec![producer_scope],
            content_digest: String::new(),
        };
        root.seal(max_scopes)?;
        Ok(root)
    }

    pub fn validate(&self, max_scopes: usize) -> Result<()> {
        let state_valid = match &self.state {
            ProceduralSubjectValidityStateV1::Ready { completion: None } => {
                self.validity_epoch == 1
            }
            ProceduralSubjectValidityStateV1::Ready {
                completion: Some(completion),
            } => {
                completion
                    .work
                    .validates_for(&self.physical_key, self.validity_epoch)
                    && is_digest(&completion.receipt_digest)
            }
            ProceduralSubjectValidityStateV1::Reconciling { work }
            | ProceduralSubjectValidityStateV1::Blocked { work, .. } => {
                work.validates_for(&self.physical_key, self.validity_epoch)
            }
        };
        let keys = self
            .producer_scopes
            .iter()
            .map(ProceduralProducerScopeV1::scope_index_key)
            .collect::<Result<Vec<_>>>()?;
        if self.schema_version != 1
            || self.physical_key != self.scope.root_key()?
            || self.revision == 0
            || self.validity_epoch == 0
            || self.validity_epoch > self.revision
            || !state_valid
            || self.retained_work.len() > super::MAX_PROCEDURAL_FEEDBACK_RECENT_TERMINAL_JOBS
            || self.retained_work.len() as u64 != self.validity_epoch - 1
            || self
                .retained_work
                .iter()
                .enumerate()
                .any(|(index, work)| !work.validates_for(&self.physical_key, index as u64 + 2))
            || match &self.state {
                ProceduralSubjectValidityStateV1::Ready { completion: None } => {
                    !self.retained_work.is_empty()
                }
                ProceduralSubjectValidityStateV1::Ready {
                    completion: Some(completion),
                } => self.retained_work.last() != Some(&completion.work),
                ProceduralSubjectValidityStateV1::Reconciling { work }
                | ProceduralSubjectValidityStateV1::Blocked { work, .. } => {
                    self.retained_work.last() != Some(work)
                }
            }
            || self.producer_scopes.is_empty()
            || self.producer_scopes.len() > max_scopes
            || self
                .producer_scopes
                .iter()
                .any(|scope| !self.scope.contains(scope))
            || !keys.windows(2).all(|pair| pair[0] < pair[1])
            || self.content_digest != self.digest()?
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn permits_learning_reads(&self, max_scopes: usize) -> Result<bool> {
        self.validate(max_scopes)?;
        Ok(matches!(
            self.state,
            ProceduralSubjectValidityStateV1::Ready { .. }
        ))
    }

    pub fn bind_scope(&self, scope: ProceduralProducerScopeV1, max_scopes: usize) -> Result<Self> {
        self.validate(max_scopes)?;
        if !self.scope.contains(&scope) {
            return Err(invalid());
        }
        if self.producer_scopes.contains(&scope) {
            return Ok(self.clone());
        }
        let mut after = self.clone();
        after.producer_scopes.push(scope);
        let mut keyed = after
            .producer_scopes
            .into_iter()
            .map(|scope| Ok((scope.scope_index_key()?, scope)))
            .collect::<Result<Vec<_>>>()?;
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        after.producer_scopes = keyed.into_iter().map(|(_, scope)| scope).collect();
        after.advance(max_scopes)
    }

    pub fn begin(&self, work: &ProceduralReconciliationWorkV1, max_scopes: usize) -> Result<Self> {
        self.validate(max_scopes)?;
        if work.scope != self.scope || self.validity_epoch.checked_add(1) != Some(work.target_epoch)
        {
            return Err(invalid());
        }
        let mut after = self.clone();
        after.validity_epoch = work.target_epoch;
        let reference = work.reference()?;
        after.retained_work.push(reference.clone());
        after.state = ProceduralSubjectValidityStateV1::Reconciling { work: reference };
        after.advance(max_scopes)
    }

    pub fn complete(
        &self,
        work: &ProceduralReconciliationWorkV1,
        receipt_digest: String,
        max_scopes: usize,
    ) -> Result<Self> {
        self.require_active_work(work, max_scopes)?;
        if !is_digest(&receipt_digest) {
            return Err(invalid());
        }
        let mut after = self.clone();
        after.state = ProceduralSubjectValidityStateV1::Ready {
            completion: Some(ProceduralReconciliationCompletionV1 {
                work: work.reference()?,
                receipt_digest,
            }),
        };
        after.advance(max_scopes)
    }

    pub fn block(
        &self,
        work: &ProceduralReconciliationWorkV1,
        reason: ProceduralReconciliationBlockV1,
        max_scopes: usize,
    ) -> Result<Self> {
        self.require_active_work(work, max_scopes)?;
        let mut after = self.clone();
        after.state = ProceduralSubjectValidityStateV1::Blocked {
            work: work.reference()?,
            reason,
        };
        after.advance(max_scopes)
    }

    pub fn resume(&self, work: &ProceduralReconciliationWorkV1, max_scopes: usize) -> Result<Self> {
        self.validate(max_scopes)?;
        let reference = work.reference()?;
        if !matches!(&self.state, ProceduralSubjectValidityStateV1::Blocked { work, .. } if *work == reference)
        {
            return Err(invalid());
        }
        let mut after = self.clone();
        after.state = ProceduralSubjectValidityStateV1::Reconciling { work: reference };
        after.advance(max_scopes)
    }

    fn require_active_work(
        &self,
        work: &ProceduralReconciliationWorkV1,
        max_scopes: usize,
    ) -> Result<()> {
        self.validate(max_scopes)?;
        let reference = work.reference()?;
        if matches!(&self.state, ProceduralSubjectValidityStateV1::Reconciling { work } if *work == reference)
        {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    fn advance(mut self, max_scopes: usize) -> Result<Self> {
        self.revision = self.revision.checked_add(1).ok_or_else(invalid)?;
        self.seal(max_scopes)?;
        Ok(self)
    }

    fn seal(&mut self, max_scopes: usize) -> Result<()> {
        self.content_digest = self.digest()?;
        self.validate(max_scopes)
    }

    fn digest(&self) -> Result<String> {
        let encoded = serde_json::to_vec(&(
            self.schema_version,
            &self.physical_key,
            &self.scope,
            self.revision,
            self.validity_epoch,
            &self.state,
            &self.retained_work,
            &self.producer_scopes,
        ))
        .map_err(|_| invalid())?;
        Ok(domain_digest(
            "procedural_subject_validity_root_v1",
            &[&encoded],
        ))
    }
}

fn invalid() -> Error {
    Error::invalid_input(
        "procedural_reconciliation",
        "invalid subject validity scope, causal work, epoch, closure or bound",
    )
}
