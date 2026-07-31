//! Backend-neutral governed RuntimeSkill contracts frozen by P8.1.

use std::collections::BTreeSet;

use serde::{ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::memory::{
    build_runtime_skill_premise_evaluation_report, GovernedMemoryOwnerPlane,
    GovernedMemoryOwnerRef, GovernedOwnerRevisionRef, MemoryPrivacyClass, PremiseEvaluationReport,
    PremiseTypedSource,
};

pub const RUNTIME_SKILL_GOVERNED_CONTRACT_SCHEMA_VERSION: u32 = 1;
pub const RUNTIME_SKILL_OWNER_RECORD_SCHEMA_VERSION: u32 = 1;
pub const RUNTIME_SKILL_SCOPE_MANIFEST_SCHEMA_VERSION: u32 = 1;
const RUNTIME_SKILL_OWNER_ID_DOMAIN: &str = "runtime_skill_owner_id_v1";
const RUNTIME_SKILL_OWNER_KEY_DOMAIN: &str = "runtime_skill_physical_owner_key_v1";
const RUNTIME_SKILL_CONTENT_DIGEST_DOMAIN: &str = "runtime_skill_content_digest_v1";
const RUNTIME_SKILL_SCOPE_MANIFEST_KEY_DOMAIN: &str = "runtime_skill_scope_manifest_key_v1";
const RUNTIME_SKILL_SCOPE_BINDINGS_DIGEST_DOMAIN: &str = "runtime_skill_scope_bindings_digest_v1";
const RUNTIME_SKILL_MATERIALIZED_VIEW_REF_DOMAIN: &str = "runtime_skill_materialized_view_ref_v1";
const RUNTIME_SKILL_PROJECTION_CANDIDATE_REF_DOMAIN: &str =
    "runtime_skill_projection_candidate_ref_v1";
const MAX_RUNTIME_SKILL_QUERY_TERMS: usize = 32;
const MAX_RUNTIME_SKILL_QUERY_TERM_BYTES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeSkillOwningScope {
    Subject { mounted_subject_id: String },
    SharedProgram,
}

impl RuntimeSkillOwningScope {
    fn canonical_subject_id(&self) -> Option<&str> {
        match self {
            Self::Subject { mounted_subject_id } => Some(mounted_subject_id),
            Self::SharedProgram => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillFeedbackKind {
    AgentSkillUsage,
    AgentToolUsage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeSkillCreationRef {
    GovernedCandidate {
        candidate_id: String,
        candidate_digest: String,
    },
    TaskLearningPromotion {
        learning_id: String,
        learning_digest: String,
    },
    ReplayPromotion {
        candidate_ref: String,
        verification_receipt_digest: String,
    },
    GovernedUsageFeedback {
        feedback_kind: RuntimeSkillFeedbackKind,
        feedback_ref: String,
        observation_digest: String,
    },
}

impl RuntimeSkillCreationRef {
    pub fn validate_contract(&self) -> bool {
        match self {
            Self::GovernedCandidate {
                candidate_id,
                candidate_digest,
            } => is_canonical(candidate_id) && is_sha256_digest(candidate_digest),
            Self::TaskLearningPromotion {
                learning_id,
                learning_digest,
            } => is_canonical(learning_id) && is_sha256_digest(learning_digest),
            Self::ReplayPromotion {
                candidate_ref,
                verification_receipt_digest,
            } => is_canonical(candidate_ref) && is_sha256_digest(verification_receipt_digest),
            Self::GovernedUsageFeedback {
                feedback_ref,
                observation_digest,
                ..
            } => is_canonical(feedback_ref) && is_sha256_digest(observation_digest),
        }
    }

    fn hash_fields(&self, hasher: &mut Sha256) {
        match self {
            Self::GovernedCandidate {
                candidate_id,
                candidate_digest,
            } => {
                hash_field(hasher, b"governed_candidate");
                hash_field(hasher, candidate_id.as_bytes());
                hash_field(hasher, candidate_digest.as_bytes());
            }
            Self::TaskLearningPromotion {
                learning_id,
                learning_digest,
            } => {
                hash_field(hasher, b"task_learning_promotion");
                hash_field(hasher, learning_id.as_bytes());
                hash_field(hasher, learning_digest.as_bytes());
            }
            Self::ReplayPromotion {
                candidate_ref,
                verification_receipt_digest,
            } => {
                hash_field(hasher, b"replay_promotion");
                hash_field(hasher, candidate_ref.as_bytes());
                hash_field(hasher, verification_receipt_digest.as_bytes());
            }
            Self::GovernedUsageFeedback {
                feedback_kind,
                feedback_ref,
                observation_digest,
            } => {
                hash_field(hasher, b"governed_usage_feedback");
                hash_field(
                    hasher,
                    match feedback_kind {
                        RuntimeSkillFeedbackKind::AgentSkillUsage => {
                            b"agent_skill_usage".as_slice()
                        }
                        RuntimeSkillFeedbackKind::AgentToolUsage => b"agent_tool_usage".as_slice(),
                    },
                );
                hash_field(hasher, feedback_ref.as_bytes());
                hash_field(hasher, observation_digest.as_bytes());
            }
        }
    }
}

pub fn canonical_runtime_skill_owner_id(
    memory_space_id: &str,
    owning_scope: &RuntimeSkillOwningScope,
    creation_ref: &RuntimeSkillCreationRef,
) -> crate::error::Result<String> {
    if !is_canonical(memory_space_id)
        || owning_scope
            .canonical_subject_id()
            .is_some_and(|subject_id| !is_canonical(subject_id))
        || !creation_ref.validate_contract()
    {
        return Err(crate::error::Error::config(
            "runtime_skill_owner_id",
            "memory space, physical owning scope, and creation ref must be canonical",
        ));
    }

    let mut hasher = Sha256::new();
    hash_field(&mut hasher, RUNTIME_SKILL_OWNER_ID_DOMAIN.as_bytes());
    hash_field(&mut hasher, memory_space_id.as_bytes());
    match owning_scope {
        RuntimeSkillOwningScope::Subject { mounted_subject_id } => {
            hash_field(&mut hasher, b"subject");
            hash_field(&mut hasher, mounted_subject_id.as_bytes());
        }
        RuntimeSkillOwningScope::SharedProgram => {
            hash_field(&mut hasher, b"shared_program");
        }
    }
    creation_ref.hash_fields(&mut hasher);
    Ok(format!("runtime_skill:sha256:{:x}", hasher.finalize()))
}

pub fn canonical_runtime_skill_owner_key(
    memory_space_id: &str,
    owning_scope: &RuntimeSkillOwningScope,
    canonical_owner_id: &str,
) -> crate::error::Result<String> {
    validate_physical_scope(memory_space_id, owning_scope, "runtime_skill_owner_key")?;
    if !is_runtime_skill_owner_id(canonical_owner_id) {
        return Err(crate::error::Error::config(
            "runtime_skill_owner_key",
            "runtime skill owner id must be canonical",
        ));
    }
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, RUNTIME_SKILL_OWNER_KEY_DOMAIN.as_bytes());
    hash_field(&mut hasher, memory_space_id.as_bytes());
    hash_owning_scope(&mut hasher, owning_scope);
    hash_field(&mut hasher, canonical_owner_id.as_bytes());
    Ok(format!(
        "runtime_skill_owner:sha256:{:x}",
        hasher.finalize()
    ))
}

pub fn runtime_skill_scope_manifest_key(
    memory_space_id: &str,
    owning_scope: &RuntimeSkillOwningScope,
) -> crate::error::Result<String> {
    validate_physical_scope(
        memory_space_id,
        owning_scope,
        "runtime_skill_scope_manifest_key",
    )?;
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        RUNTIME_SKILL_SCOPE_MANIFEST_KEY_DOMAIN.as_bytes(),
    );
    hash_field(&mut hasher, memory_space_id.as_bytes());
    hash_owning_scope(&mut hasher, owning_scope);
    Ok(format!(
        "runtime_skill_scope:sha256:{:x}",
        hasher.finalize()
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeSkillApplicabilityTarget {
    Project { project_id: String },
    User { user_ref: String },
    Organization { organization_id: String },
    Device { device_id: String },
}

impl RuntimeSkillApplicabilityTarget {
    fn validate_contract(&self) -> bool {
        match self {
            Self::Project { project_id } => is_canonical(project_id),
            Self::User { user_ref } => is_canonical(user_ref),
            Self::Organization { organization_id } => is_canonical(organization_id),
            Self::Device { device_id } => is_canonical(device_id),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeSkillApplicability {
    Global,
    AllOf {
        required: Vec<RuntimeSkillApplicabilityTarget>,
    },
}

impl RuntimeSkillApplicability {
    pub fn try_all_of(
        mut required: Vec<RuntimeSkillApplicabilityTarget>,
    ) -> crate::error::Result<Self> {
        if required.is_empty() || required.iter().any(|target| !target.validate_contract()) {
            return Err(crate::error::Error::config(
                "runtime_skill_applicability",
                "all-of applicability requires canonical non-empty targets",
            ));
        }
        required.sort();
        if required.windows(2).any(|window| window[0] == window[1]) {
            return Err(crate::error::Error::config(
                "runtime_skill_applicability",
                "all-of applicability targets must be unique",
            ));
        }
        Ok(Self::AllOf { required })
    }

    pub fn required_targets(&self) -> &[RuntimeSkillApplicabilityTarget] {
        match self {
            Self::Global => &[],
            Self::AllOf { required } => required,
        }
    }

    fn validate_contract(&self) -> bool {
        match self {
            Self::Global => true,
            Self::AllOf { required } => {
                !required.is_empty()
                    && required.iter().all(|target| target.validate_contract())
                    && required.windows(2).all(|window| window[0] < window[1])
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillTriggerKind {
    QueryIntent,
    TaskRequirement,
    CapabilityRequirement,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillTrigger {
    pub kind: RuntimeSkillTriggerKind,
    pub canonical_ref: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillConstraintKind {
    Profile,
    Privacy,
    ResourceBudget,
    EnvironmentPremise,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillConstraint {
    pub kind: RuntimeSkillConstraintKind,
    pub policy_safe_ref: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeSkillPremise {
    RegisteredCapability {
        capability_id: String,
        version_constraint: RuntimeSkillVersionConstraint,
    },
    GovernedEnvironmentEvidence {
        evidence_revision_ref: GovernedOwnerRevisionRef,
    },
    OpaquePresenceAttestation {
        handle_ref: String,
    },
    TaskEvidence {
        source: PremiseTypedSource,
        evidence_kind: RuntimeSkillEvidenceKind,
        safe_ref: String,
    },
}

impl RuntimeSkillPremise {
    pub(crate) const fn typed_source(&self) -> PremiseTypedSource {
        match self {
            Self::RegisteredCapability { .. } => PremiseTypedSource::RegisteredCapability,
            Self::GovernedEnvironmentEvidence { .. } => {
                PremiseTypedSource::GovernedEnvironmentEvidence
            }
            Self::OpaquePresenceAttestation { .. } => PremiseTypedSource::OpaquePresenceAttestation,
            Self::TaskEvidence { source, .. } => *source,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillVersionConstraint {
    pub min_inclusive: Option<u64>,
    pub max_exclusive: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillPremiseRequirement {
    pub premise: RuntimeSkillPremise,
    pub required: bool,
    pub valid_from: u64,
    pub valid_until: Option<u64>,
    pub privacy_class: MemoryPrivacyClass,
    pub governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillFailureMode {
    RequiredPremiseUnsatisfied,
    GovernedEvidenceInsufficient,
    ExecutionFailed,
    OutputRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillEvidenceKind {
    GovernedEvidence,
    TaskLearning,
    TaskRun,
    TaskArtifact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillEvidenceBinding {
    pub kind: RuntimeSkillEvidenceKind,
    pub safe_ref: String,
    pub source_digest: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillCapabilityAffinity {
    DynamicState,
    HistoricalAsOf,
    ProceduralRecall,
    EnvironmentPremise,
    UpdateLineage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillProjectionPolicy {
    pub privacy_class: MemoryPrivacyClass,
    pub model_projection_allowed: bool,
    pub require_all_mandatory_premises: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillIntrinsicContract {
    pub schema_version: u32,
    pub applicability: RuntimeSkillApplicability,
    pub triggers: Vec<RuntimeSkillTrigger>,
    pub constraints: Vec<RuntimeSkillConstraint>,
    pub premises: Vec<RuntimeSkillPremiseRequirement>,
    pub failure_modes: Vec<RuntimeSkillFailureMode>,
    pub evidence_bindings: Vec<RuntimeSkillEvidenceBinding>,
    pub projection_policy: RuntimeSkillProjectionPolicy,
    pub capability_affinities: Vec<RuntimeSkillCapabilityAffinity>,
}

impl RuntimeSkillIntrinsicContract {
    pub fn validate_contract(&self) -> RuntimeSkillRecallPlanValidation {
        validation_from_failures(validate_intrinsic_contract_fields(
            self.schema_version,
            Some(&self.applicability),
            &self.triggers,
            &self.constraints,
            &self.premises,
            &self.evidence_bindings,
            &self.capability_affinities,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillProceduralContent {
    pub title: String,
    pub topic: String,
    pub summary: String,
    pub procedure: String,
}

impl RuntimeSkillProceduralContent {
    fn validate_contract(&self) -> bool {
        [
            self.title.as_str(),
            self.topic.as_str(),
            self.summary.as_str(),
            self.procedure.as_str(),
        ]
        .into_iter()
        .all(is_canonical)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillAvailability {
    Enabled,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillLifecycleState {
    Active,
    Stale,
    LowValue,
    Retired,
    Superseded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillUsageOutcome {
    Neutral,
    Succeeded,
    Mismatch,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillUsageOutcomeSummary {
    pub observation_count: u32,
    pub succeeded_count: u32,
    pub mismatch_count: u32,
    pub last_outcome: Option<RuntimeSkillUsageOutcome>,
    pub last_outcome_at: Option<u64>,
}

impl RuntimeSkillUsageOutcomeSummary {
    fn validate_between(&self, observed_at: u64, updated_at: u64) -> bool {
        let Some(classified_count) = self.succeeded_count.checked_add(self.mismatch_count) else {
            return false;
        };
        if classified_count > self.observation_count {
            return false;
        }
        if self.observation_count == 0 {
            return self.succeeded_count == 0
                && self.mismatch_count == 0
                && self.last_outcome.is_none()
                && self.last_outcome_at.is_none();
        }
        let Some(last_outcome) = self.last_outcome else {
            return false;
        };
        let Some(last_outcome_at) = self.last_outcome_at else {
            return false;
        };
        if last_outcome_at < observed_at || last_outcome_at > updated_at {
            return false;
        }
        match last_outcome {
            RuntimeSkillUsageOutcome::Neutral => classified_count < self.observation_count,
            RuntimeSkillUsageOutcome::Succeeded => self.succeeded_count > 0,
            RuntimeSkillUsageOutcome::Mismatch => self.mismatch_count > 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillLifecycleLineage {
    pub predecessor: Option<RuntimeSkillOwnerBinding>,
    pub successor: Option<RuntimeSkillOwnerBinding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillLifecycleContractFailure {
    ObservedAtInvalid,
    UpdatedAtInvalid,
    InitialStateInvalid,
    AvailabilityStateInvalid,
    PredecessorInvalid,
    SuccessorInvalid,
    UsageOutcomeInvalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillLifecycleContractValidation {
    pub accepted: bool,
    pub failures: Vec<RuntimeSkillLifecycleContractFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillLifecycle {
    pub availability: RuntimeSkillAvailability,
    pub state: RuntimeSkillLifecycleState,
    pub lineage: RuntimeSkillLifecycleLineage,
    pub observed_at: u64,
    pub updated_at: u64,
    pub usage_outcome: RuntimeSkillUsageOutcomeSummary,
}

impl RuntimeSkillLifecycle {
    pub fn created(observed_at: u64) -> crate::error::Result<Self> {
        if observed_at == 0 {
            return Err(crate::error::Error::config(
                "runtime_skill_lifecycle",
                "created lifecycle requires a positive observation time",
            ));
        }
        Ok(Self {
            availability: RuntimeSkillAvailability::Enabled,
            state: RuntimeSkillLifecycleState::Active,
            lineage: RuntimeSkillLifecycleLineage::default(),
            observed_at,
            updated_at: observed_at,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        })
    }

    pub fn validate_for(
        &self,
        memory_space_id: &str,
        owning_scope: &RuntimeSkillOwningScope,
        owner_ref: &GovernedMemoryOwnerRef,
        owner_revision: u64,
    ) -> RuntimeSkillLifecycleContractValidation {
        let mut failures = Vec::new();
        if self.observed_at == 0 {
            failures.push(RuntimeSkillLifecycleContractFailure::ObservedAtInvalid);
        }
        if self.updated_at < self.observed_at {
            failures.push(RuntimeSkillLifecycleContractFailure::UpdatedAtInvalid);
        }
        if owner_revision == 1 && self.state != RuntimeSkillLifecycleState::Active {
            failures.push(RuntimeSkillLifecycleContractFailure::InitialStateInvalid);
        }
        if matches!(
            self.state,
            RuntimeSkillLifecycleState::Retired | RuntimeSkillLifecycleState::Superseded
        ) && self.availability != RuntimeSkillAvailability::Disabled
        {
            failures.push(RuntimeSkillLifecycleContractFailure::AvailabilityStateInvalid);
        }

        let predecessor_valid = match (owner_revision, &self.lineage.predecessor) {
            (1, None) => true,
            (revision, Some(predecessor)) if revision > 1 => {
                predecessor.validate_for(memory_space_id, owning_scope)
                    && predecessor.owner_ref == *owner_ref
                    && predecessor.owner_revision == revision - 1
            }
            _ => false,
        };
        if !predecessor_valid {
            failures.push(RuntimeSkillLifecycleContractFailure::PredecessorInvalid);
        }

        let successor_valid = match (self.state, &self.lineage.successor) {
            (RuntimeSkillLifecycleState::Superseded, Some(successor)) => {
                successor.validate_for(memory_space_id, owning_scope)
                    && successor.owner_ref.owner_plane == GovernedMemoryOwnerPlane::RuntimeSkill
                    && successor.owner_ref != *owner_ref
                    && successor.owner_revision == 1
            }
            (RuntimeSkillLifecycleState::Superseded, None) => false,
            (_, None) => true,
            (_, Some(_)) => false,
        };
        if !successor_valid {
            failures.push(RuntimeSkillLifecycleContractFailure::SuccessorInvalid);
        }

        if !self
            .usage_outcome
            .validate_between(self.observed_at, self.updated_at)
        {
            failures.push(RuntimeSkillLifecycleContractFailure::UsageOutcomeInvalid);
        }
        if owner_revision == 1 && self.updated_at != self.observed_at {
            failures.push(RuntimeSkillLifecycleContractFailure::UpdatedAtInvalid);
        }
        failures.sort();
        failures.dedup();
        RuntimeSkillLifecycleContractValidation {
            accepted: failures.is_empty(),
            failures,
        }
    }

    fn lineage_privacy_matches(&self, privacy_class: MemoryPrivacyClass) -> bool {
        self.lineage
            .predecessor
            .as_ref()
            .is_none_or(|binding| binding.privacy_class == privacy_class)
            && self
                .lineage
                .successor
                .as_ref()
                .is_none_or(|binding| binding.privacy_class == privacy_class)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillOwnerContractFailure {
    SchemaMismatch,
    ScopeInvalid,
    OwnerIdentityInvalid,
    OwnerRevisionInvalid,
    PhysicalKeyMismatch,
    IntrinsicContractInvalid,
    ProceduralContentInvalid,
    LifecycleInvalid,
    PrivacyMismatch,
    ContentDigestMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillOwnerContractValidation {
    pub accepted: bool,
    pub failures: Vec<RuntimeSkillOwnerContractFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillOwnerRecord {
    pub schema_version: u32,
    pub physical_key: String,
    pub memory_space_id: String,
    pub owning_scope: RuntimeSkillOwningScope,
    pub creation_ref: RuntimeSkillCreationRef,
    pub owner_ref: GovernedMemoryOwnerRef,
    pub owner_revision: u64,
    pub intrinsic_contract: RuntimeSkillIntrinsicContract,
    pub procedural_content: RuntimeSkillProceduralContent,
    pub lifecycle: RuntimeSkillLifecycle,
    pub privacy_class: MemoryPrivacyClass,
    pub content_digest: String,
}

impl RuntimeSkillOwnerRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        memory_space_id: &str,
        owning_scope: RuntimeSkillOwningScope,
        creation_ref: RuntimeSkillCreationRef,
        owner_revision: u64,
        mut intrinsic_contract: RuntimeSkillIntrinsicContract,
        procedural_content: RuntimeSkillProceduralContent,
        lifecycle: RuntimeSkillLifecycle,
        privacy_class: MemoryPrivacyClass,
    ) -> crate::error::Result<Self> {
        validate_physical_scope(memory_space_id, &owning_scope, "runtime_skill_owner_record")?;
        if owner_revision == 0
            || !creation_ref.validate_contract()
            || !procedural_content.validate_contract()
        {
            return Err(crate::error::Error::config(
                "runtime_skill_owner_record",
                "revision, creation ref, or procedural content is invalid",
            ));
        }
        canonicalize_intrinsic_contract(&mut intrinsic_contract)?;
        if !intrinsic_contract.validate_contract().accepted
            || intrinsic_contract.projection_policy.privacy_class != privacy_class
        {
            return Err(crate::error::Error::config(
                "runtime_skill_owner_record",
                "intrinsic contract or privacy binding is invalid",
            ));
        }
        let owner_id =
            canonical_runtime_skill_owner_id(memory_space_id, &owning_scope, &creation_ref)?;
        let owner_ref =
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::RuntimeSkill, owner_id);
        if !lifecycle
            .validate_for(memory_space_id, &owning_scope, &owner_ref, owner_revision)
            .accepted
            || !lifecycle.lineage_privacy_matches(privacy_class)
        {
            return Err(crate::error::Error::config(
                "runtime_skill_owner_record",
                "runtime skill lifecycle or lineage privacy is invalid for the owner revision",
            ));
        }
        let physical_key =
            canonical_runtime_skill_owner_key(memory_space_id, &owning_scope, &owner_ref.owner_id)?;
        let mut record = Self {
            schema_version: RUNTIME_SKILL_OWNER_RECORD_SCHEMA_VERSION,
            physical_key,
            memory_space_id: memory_space_id.to_string(),
            owning_scope,
            creation_ref,
            owner_ref,
            owner_revision,
            intrinsic_contract,
            procedural_content,
            lifecycle,
            privacy_class,
            content_digest: String::new(),
        };
        record.content_digest = record.canonical_content_digest()?;
        Ok(record)
    }

    pub fn owner_revision_ref(&self) -> GovernedOwnerRevisionRef {
        GovernedOwnerRevisionRef {
            owner_ref: self.owner_ref.clone(),
            owner_revision: self.owner_revision,
        }
    }

    pub fn revise_procedural_content(
        &self,
        procedural_content: RuntimeSkillProceduralContent,
        updated_at: u64,
    ) -> crate::error::Result<Self> {
        self.advance(
            procedural_content,
            self.lifecycle.availability,
            self.lifecycle.state,
            updated_at,
        )
    }

    pub fn revise_availability(
        &self,
        availability: RuntimeSkillAvailability,
        updated_at: u64,
    ) -> crate::error::Result<Self> {
        self.advance(
            self.procedural_content.clone(),
            availability,
            self.lifecycle.state,
            updated_at,
        )
    }

    pub fn retire(&self, updated_at: u64) -> crate::error::Result<Self> {
        self.advance(
            self.procedural_content.clone(),
            RuntimeSkillAvailability::Disabled,
            RuntimeSkillLifecycleState::Retired,
            updated_at,
        )
    }

    fn advance(
        &self,
        procedural_content: RuntimeSkillProceduralContent,
        availability: RuntimeSkillAvailability,
        state: RuntimeSkillLifecycleState,
        updated_at: u64,
    ) -> crate::error::Result<Self> {
        if !self.validate_contract().accepted
            || matches!(
                self.lifecycle.state,
                RuntimeSkillLifecycleState::Retired | RuntimeSkillLifecycleState::Superseded
            )
            || updated_at == 0
            || matches!(
                state,
                RuntimeSkillLifecycleState::Retired | RuntimeSkillLifecycleState::Superseded
            ) && availability != RuntimeSkillAvailability::Disabled
        {
            return Err(crate::error::Error::config(
                "runtime_skill_owner_advance",
                "runtime skill transition is terminal, non-monotonic, or invalid",
            ));
        }
        let next_updated_at = self.lifecycle.updated_at.checked_add(1).ok_or_else(|| {
            crate::error::Error::config(
                "runtime_skill_owner_advance",
                "runtime skill lifecycle timestamp exhausted",
            )
        })?;
        let next_owner_revision = self.owner_revision.checked_add(1).ok_or_else(|| {
            crate::error::Error::config(
                "runtime_skill_owner_advance",
                "runtime skill owner revision exhausted",
            )
        })?;
        let effective_updated_at = updated_at.max(next_updated_at);
        let mut lifecycle = self.lifecycle.clone();
        lifecycle.availability = availability;
        lifecycle.state = state;
        lifecycle.updated_at = effective_updated_at;
        lifecycle.lineage = RuntimeSkillLifecycleLineage {
            predecessor: Some(RuntimeSkillOwnerBinding::from_record(self)?),
            successor: None,
        };
        Self::build(
            &self.memory_space_id,
            self.owning_scope.clone(),
            self.creation_ref.clone(),
            next_owner_revision,
            self.intrinsic_contract.clone(),
            procedural_content,
            lifecycle,
            self.privacy_class,
        )
    }

    pub fn canonical_content_digest(&self) -> crate::error::Result<String> {
        #[derive(Serialize)]
        struct DigestInput<'a> {
            schema_version: u32,
            memory_space_id: &'a str,
            owning_scope: &'a RuntimeSkillOwningScope,
            creation_ref: &'a RuntimeSkillCreationRef,
            owner_ref: &'a GovernedMemoryOwnerRef,
            owner_revision: u64,
            intrinsic_contract: &'a RuntimeSkillIntrinsicContract,
            procedural_content: &'a RuntimeSkillProceduralContent,
            lifecycle: &'a RuntimeSkillLifecycle,
            privacy_class: MemoryPrivacyClass,
        }

        let encoded = serde_json::to_vec(&DigestInput {
            schema_version: self.schema_version,
            memory_space_id: &self.memory_space_id,
            owning_scope: &self.owning_scope,
            creation_ref: &self.creation_ref,
            owner_ref: &self.owner_ref,
            owner_revision: self.owner_revision,
            intrinsic_contract: &self.intrinsic_contract,
            procedural_content: &self.procedural_content,
            lifecycle: &self.lifecycle,
            privacy_class: self.privacy_class,
        })
        .map_err(|error| {
            crate::error::Error::config("runtime_skill_content_digest", error.to_string())
        })?;
        Ok(domain_separated_sha256(
            RUNTIME_SKILL_CONTENT_DIGEST_DOMAIN,
            &[&encoded],
        ))
    }

    pub fn validate_contract(&self) -> RuntimeSkillOwnerContractValidation {
        let mut failures = Vec::new();
        if self.schema_version != RUNTIME_SKILL_OWNER_RECORD_SCHEMA_VERSION {
            failures.push(RuntimeSkillOwnerContractFailure::SchemaMismatch);
        }
        if validate_physical_scope(
            &self.memory_space_id,
            &self.owning_scope,
            "runtime_skill_owner_record",
        )
        .is_err()
        {
            failures.push(RuntimeSkillOwnerContractFailure::ScopeInvalid);
        }
        let expected_owner_id = canonical_runtime_skill_owner_id(
            &self.memory_space_id,
            &self.owning_scope,
            &self.creation_ref,
        );
        if self.owner_ref.owner_plane != GovernedMemoryOwnerPlane::RuntimeSkill
            || !self.owner_ref.is_valid()
            || !expected_owner_id.is_ok_and(|owner_id| owner_id == self.owner_ref.owner_id)
        {
            failures.push(RuntimeSkillOwnerContractFailure::OwnerIdentityInvalid);
        }
        if self.owner_revision == 0 {
            failures.push(RuntimeSkillOwnerContractFailure::OwnerRevisionInvalid);
        }
        if !canonical_runtime_skill_owner_key(
            &self.memory_space_id,
            &self.owning_scope,
            &self.owner_ref.owner_id,
        )
        .is_ok_and(|physical_key| physical_key == self.physical_key)
        {
            failures.push(RuntimeSkillOwnerContractFailure::PhysicalKeyMismatch);
        }
        let mut canonical_intrinsic = self.intrinsic_contract.clone();
        if canonicalize_intrinsic_contract(&mut canonical_intrinsic).is_err()
            || canonical_intrinsic != self.intrinsic_contract
            || !self.intrinsic_contract.validate_contract().accepted
        {
            failures.push(RuntimeSkillOwnerContractFailure::IntrinsicContractInvalid);
        }
        if !self.procedural_content.validate_contract() {
            failures.push(RuntimeSkillOwnerContractFailure::ProceduralContentInvalid);
        }
        if !self
            .lifecycle
            .validate_for(
                &self.memory_space_id,
                &self.owning_scope,
                &self.owner_ref,
                self.owner_revision,
            )
            .accepted
        {
            failures.push(RuntimeSkillOwnerContractFailure::LifecycleInvalid);
        }
        if self.intrinsic_contract.projection_policy.privacy_class != self.privacy_class
            || !self.lifecycle.lineage_privacy_matches(self.privacy_class)
        {
            failures.push(RuntimeSkillOwnerContractFailure::PrivacyMismatch);
        }
        if !self
            .canonical_content_digest()
            .is_ok_and(|digest| digest == self.content_digest)
        {
            failures.push(RuntimeSkillOwnerContractFailure::ContentDigestMismatch);
        }
        failures.sort();
        failures.dedup();
        RuntimeSkillOwnerContractValidation {
            accepted: failures.is_empty(),
            failures,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillOwnerLocator {
    owning_scope: RuntimeSkillOwningScope,
    owner_revision_ref: GovernedOwnerRevisionRef,
}

impl Serialize for RuntimeSkillOwnerLocator {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("RuntimeSkillOwnerLocator", 3)?;
        state.serialize_field("owning_scope", &self.owning_scope)?;
        state.serialize_field("owner_id", self.owner_id())?;
        state.serialize_field("owner_revision", &self.owner_revision())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for RuntimeSkillOwnerLocator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RuntimeSkillOwnerLocatorWire {
            owning_scope: RuntimeSkillOwningScope,
            owner_id: String,
            owner_revision: u64,
        }

        let wire = RuntimeSkillOwnerLocatorWire::deserialize(deserializer)?;
        Self::try_new(wire.owning_scope, wire.owner_id, wire.owner_revision)
            .map_err(serde::de::Error::custom)
    }
}

impl RuntimeSkillOwnerLocator {
    pub fn try_new(
        owning_scope: RuntimeSkillOwningScope,
        owner_id: impl Into<String>,
        owner_revision: u64,
    ) -> crate::error::Result<Self> {
        Ok(Self {
            owning_scope,
            owner_revision_ref: GovernedOwnerRevisionRef::try_new(
                GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::RuntimeSkill,
                    owner_id.into(),
                ),
                owner_revision,
            )?,
        })
    }

    pub fn from_record(record: &RuntimeSkillOwnerRecord) -> Self {
        Self {
            owning_scope: record.owning_scope.clone(),
            owner_revision_ref: record.owner_revision_ref(),
        }
    }

    pub fn owning_scope(&self) -> &RuntimeSkillOwningScope {
        &self.owning_scope
    }

    pub fn owner_id(&self) -> &str {
        &self.owner_revision_ref.owner_ref.owner_id
    }

    pub const fn owner_revision(&self) -> u64 {
        self.owner_revision_ref.owner_revision
    }

    pub fn validate_for(&self, memory_space_id: &str) -> bool {
        self.owner_revision_ref.owner_ref.owner_plane == GovernedMemoryOwnerPlane::RuntimeSkill
            && self.owner_revision_ref.owner_ref.is_valid()
            && self.owner_revision_ref.owner_revision > 0
            && validate_physical_scope(
                memory_space_id,
                &self.owning_scope,
                "runtime_skill_owner_locator",
            )
            .is_ok()
            && canonical_runtime_skill_owner_key(
                memory_space_id,
                &self.owning_scope,
                &self.owner_revision_ref.owner_ref.owner_id,
            )
            .is_ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillOwnerBinding {
    pub owner_ref: GovernedMemoryOwnerRef,
    pub owner_revision: u64,
    pub owner_physical_key: String,
    pub privacy_class: MemoryPrivacyClass,
    pub content_digest: String,
}

impl RuntimeSkillOwnerBinding {
    pub fn from_record(record: &RuntimeSkillOwnerRecord) -> crate::error::Result<Self> {
        if !record.validate_contract().accepted {
            return Err(crate::error::Error::config(
                "runtime_skill_owner_binding",
                "runtime skill owner record is invalid",
            ));
        }
        Ok(Self {
            owner_ref: record.owner_ref.clone(),
            owner_revision: record.owner_revision,
            owner_physical_key: record.physical_key.clone(),
            privacy_class: record.privacy_class,
            content_digest: record.content_digest.clone(),
        })
    }

    fn validate_for(&self, memory_space_id: &str, owning_scope: &RuntimeSkillOwningScope) -> bool {
        self.owner_ref.owner_plane == GovernedMemoryOwnerPlane::RuntimeSkill
            && self.owner_ref.is_valid()
            && self.owner_revision > 0
            && is_sha256_digest(&self.content_digest)
            && canonical_runtime_skill_owner_key(
                memory_space_id,
                owning_scope,
                &self.owner_ref.owner_id,
            )
            .is_ok_and(|physical_key| physical_key == self.owner_physical_key)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillScopeManifest {
    pub schema_version: u32,
    pub physical_key: String,
    pub revision: u64,
    pub memory_space_id: String,
    pub owning_scope: RuntimeSkillOwningScope,
    pub owner_count: usize,
    pub owner_bindings: Vec<RuntimeSkillOwnerBinding>,
    pub bindings_digest: String,
}

impl RuntimeSkillScopeManifest {
    pub fn build(
        revision: u64,
        memory_space_id: &str,
        owning_scope: RuntimeSkillOwningScope,
        owner_bindings: impl IntoIterator<Item = RuntimeSkillOwnerBinding>,
        max_entries: usize,
    ) -> crate::error::Result<Self> {
        validate_physical_scope(
            memory_space_id,
            &owning_scope,
            "runtime_skill_scope_manifest",
        )?;
        if revision == 0 || max_entries == 0 {
            return Err(crate::error::Error::config(
                "runtime_skill_scope_manifest",
                "revision and max_entries must be greater than zero",
            ));
        }
        let mut owner_bindings = owner_bindings.into_iter().collect::<Vec<_>>();
        if owner_bindings
            .iter()
            .any(|binding| !binding.validate_for(memory_space_id, &owning_scope))
        {
            return Err(crate::error::Error::config(
                "runtime_skill_scope_manifest",
                "owner binding identity, scope, revision, key, or digest is invalid",
            ));
        }
        owner_bindings.sort_by(|left, right| {
            left.owner_ref
                .cmp(&right.owner_ref)
                .then_with(|| left.owner_revision.cmp(&right.owner_revision))
                .then_with(|| left.owner_physical_key.cmp(&right.owner_physical_key))
        });
        if owner_bindings.len() > max_entries
            || owner_bindings.windows(2).any(|pair| {
                pair[0].owner_ref == pair[1].owner_ref
                    || pair[0].owner_physical_key == pair[1].owner_physical_key
            })
        {
            return Err(crate::error::Error::config(
                "runtime_skill_scope_manifest",
                "owner bindings are duplicate or exceed the pinned bound",
            ));
        }
        let physical_key = runtime_skill_scope_manifest_key(memory_space_id, &owning_scope)?;
        let bindings_digest = runtime_skill_scope_bindings_digest(
            revision,
            memory_space_id,
            &owning_scope,
            &owner_bindings,
        )?;
        Ok(Self {
            schema_version: RUNTIME_SKILL_SCOPE_MANIFEST_SCHEMA_VERSION,
            physical_key,
            revision,
            memory_space_id: memory_space_id.to_string(),
            owning_scope,
            owner_count: owner_bindings.len(),
            owner_bindings,
            bindings_digest,
        })
    }

    pub fn validate_exact(
        &self,
        memory_space_id: &str,
        owning_scope: &RuntimeSkillOwningScope,
        owner_bindings: impl IntoIterator<Item = RuntimeSkillOwnerBinding>,
        max_entries: usize,
    ) -> crate::error::Result<()> {
        let expected = Self::build(
            self.revision,
            memory_space_id,
            owning_scope.clone(),
            owner_bindings,
            max_entries,
        )?;
        if self == &expected
            && self.schema_version == RUNTIME_SKILL_SCOPE_MANIFEST_SCHEMA_VERSION
            && self.owner_count == self.owner_bindings.len()
        {
            Ok(())
        } else {
            Err(crate::error::Error::config(
                "runtime_skill_scope_manifest",
                "scope manifest differs from the exact canonical owner closure",
            ))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillOperationAuthorityRef {
    value: String,
}

impl RuntimeSkillOperationAuthorityRef {
    pub fn try_new(value: impl Into<String>) -> crate::error::Result<Self> {
        let value = value.into();
        if !is_prefixed_sha256(&value, "runtime_skill_operation:sha256:") {
            return Err(crate::error::Error::config(
                "runtime_skill_operation_authority_ref",
                "operation authority ref must be a canonical opaque digest",
            ));
        }
        Ok(Self { value })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct RuntimeSkillMaterializedViewRef {
    value: String,
}

impl RuntimeSkillMaterializedViewRef {
    pub(crate) fn try_new(value: impl Into<String>) -> crate::error::Result<Self> {
        let value = value.into();
        if !is_prefixed_sha256(&value, "runtime_skill_view:sha256:") {
            return Err(crate::error::Error::config(
                "runtime_skill_materialized_view_ref",
                "materialized view ref must be a canonical opaque digest",
            ));
        }
        Ok(Self { value })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillRecallQuery {
    pub terms: Vec<String>,
}

impl RuntimeSkillRecallQuery {
    pub fn try_from_text(text: &str) -> crate::error::Result<Self> {
        let terms = canonical_runtime_skill_terms(text);
        let query = Self { terms };
        if !query.validate_contract() {
            return Err(crate::error::Error::config(
                "runtime_skill_recall_query",
                "query must contain bounded canonical terms",
            ));
        }
        Ok(query)
    }

    fn validate_contract(&self) -> bool {
        !self.terms.is_empty()
            && self.terms.len() <= MAX_RUNTIME_SKILL_QUERY_TERMS
            && self.terms.iter().all(|term| {
                !term.is_empty()
                    && term.len() <= MAX_RUNTIME_SKILL_QUERY_TERM_BYTES
                    && term == &term.to_lowercase()
            })
            && self.terms.windows(2).all(|pair| pair[0] < pair[1])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillApplicabilityContext {
    pub targets: Vec<RuntimeSkillApplicabilityTarget>,
}

impl RuntimeSkillApplicabilityContext {
    pub fn try_new(
        mut targets: Vec<RuntimeSkillApplicabilityTarget>,
    ) -> crate::error::Result<Self> {
        if targets.iter().any(|target| !target.validate_contract()) {
            return Err(crate::error::Error::config(
                "runtime_skill_applicability_context",
                "applicability targets must be canonical",
            ));
        }
        targets.sort();
        if targets.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(crate::error::Error::config(
                "runtime_skill_applicability_context",
                "applicability targets must be unique",
            ));
        }
        Ok(Self { targets })
    }

    fn validate_contract(&self) -> bool {
        self.targets
            .iter()
            .all(RuntimeSkillApplicabilityTarget::validate_contract)
            && self.targets.windows(2).all(|pair| pair[0] < pair[1])
    }

    fn satisfies(&self, applicability: &RuntimeSkillApplicability) -> bool {
        applicability
            .required_targets()
            .iter()
            .all(|required| self.targets.binary_search(required).is_ok())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeSkillPremiseObservation {
    RegisteredCapability {
        capability_id: String,
        version: u64,
    },
    GovernedEnvironmentEvidence {
        evidence_revision_ref: GovernedOwnerRevisionRef,
        present: bool,
    },
    OpaquePresenceAttestation {
        handle_ref: String,
        present: bool,
    },
    TaskEvidence {
        source: PremiseTypedSource,
        evidence_kind: RuntimeSkillEvidenceKind,
        safe_ref: String,
        present: bool,
    },
}

impl RuntimeSkillPremiseObservation {
    pub(crate) fn validate_contract(&self) -> bool {
        match self {
            Self::RegisteredCapability {
                capability_id,
                version,
            } => is_canonical(capability_id) && *version > 0,
            Self::GovernedEnvironmentEvidence {
                evidence_revision_ref,
                ..
            } => {
                evidence_revision_ref.is_valid()
                    && evidence_revision_ref.owner_ref.owner_plane
                        == GovernedMemoryOwnerPlane::EvidenceDocument
            }
            Self::OpaquePresenceAttestation { handle_ref, .. } => is_canonical(handle_ref),
            Self::TaskEvidence {
                source,
                evidence_kind,
                safe_ref,
                ..
            } => task_evidence_source_matches(*source, *evidence_kind) && is_canonical(safe_ref),
        }
    }

    pub(crate) fn canonical_identity(&self) -> Vec<u8> {
        match self {
            Self::RegisteredCapability { capability_id, .. } => {
                serde_json::to_vec(&("registered_capability", capability_id))
            }
            Self::GovernedEnvironmentEvidence {
                evidence_revision_ref,
                ..
            } => serde_json::to_vec(&("governed_environment_evidence", evidence_revision_ref)),
            Self::OpaquePresenceAttestation { handle_ref, .. } => {
                serde_json::to_vec(&("opaque_presence_attestation", handle_ref))
            }
            Self::TaskEvidence {
                source,
                evidence_kind,
                safe_ref,
                ..
            } => serde_json::to_vec(&("task_evidence", source, evidence_kind, safe_ref)),
        }
        .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillRecallAuthority {
    procedural_recall_allowed: bool,
    governed_environment_evidence_allowed: bool,
    task_evidence_allowed: bool,
    mounted_subject_id: Option<String>,
}

impl RuntimeSkillRecallAuthority {
    pub fn try_new(
        procedural_recall_allowed: bool,
        governed_environment_evidence_allowed: bool,
        task_evidence_allowed: bool,
        mounted_subject_id: Option<String>,
    ) -> crate::error::Result<Self> {
        if mounted_subject_id
            .as_deref()
            .is_some_and(|subject_id| !is_canonical(subject_id))
        {
            return Err(crate::error::Error::config(
                "runtime_skill_recall_authority",
                "mounted subject id must be canonical",
            ));
        }
        Ok(Self {
            procedural_recall_allowed,
            governed_environment_evidence_allowed,
            task_evidence_allowed,
            mounted_subject_id,
        })
    }

    pub(crate) const fn governed_environment_evidence_allowed(&self) -> bool {
        self.governed_environment_evidence_allowed
    }

    pub(crate) const fn task_evidence_allowed(&self) -> bool {
        self.task_evidence_allowed
    }

    pub(crate) fn privacy_allows(
        &self,
        owning_scope: &RuntimeSkillOwningScope,
        privacy_class: MemoryPrivacyClass,
    ) -> bool {
        if let RuntimeSkillOwningScope::Subject { mounted_subject_id } = owning_scope {
            if self.mounted_subject_id.as_deref() != Some(mounted_subject_id) {
                return false;
            }
        }
        match privacy_class {
            MemoryPrivacyClass::PublicRuntime => true,
            MemoryPrivacyClass::SharedWithSubject => {
                matches!(owning_scope, RuntimeSkillOwningScope::Subject { .. })
            }
            MemoryPrivacyClass::PrivateGarden
            | MemoryPrivacyClass::SoulPrivate
            | MemoryPrivacyClass::OperatorDiagnostic => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeSkillRecallBudgetAuthority {
    max_retained_runtime_skill_owners_per_scope: usize,
    max_runtime_skill_lineage_depth: usize,
    max_procedural_candidates: usize,
    max_premises_per_skill: usize,
    max_premise_evidence_reads: usize,
}

impl RuntimeSkillRecallBudgetAuthority {
    pub fn try_new(
        max_retained_runtime_skill_owners_per_scope: usize,
        max_runtime_skill_lineage_depth: usize,
        max_procedural_candidates: usize,
        max_premises_per_skill: usize,
        max_premise_evidence_reads: usize,
    ) -> crate::error::Result<Self> {
        let authority = Self {
            max_retained_runtime_skill_owners_per_scope,
            max_runtime_skill_lineage_depth,
            max_procedural_candidates,
            max_premises_per_skill,
            max_premise_evidence_reads,
        };
        if !authority.validate_contract() {
            return Err(crate::error::Error::config(
                "runtime_skill_recall_budget_authority",
                "all RuntimeSkill recall budget dimensions must be positive",
            ));
        }
        Ok(authority)
    }

    pub const fn max_procedural_candidates(self) -> usize {
        self.max_procedural_candidates
    }

    fn validate_contract(&self) -> bool {
        self.max_retained_runtime_skill_owners_per_scope > 0
            && self.max_runtime_skill_lineage_depth > 0
            && self.max_procedural_candidates > 0
            && self.max_premises_per_skill > 0
            && self.max_premise_evidence_reads > 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillSafeEvidenceRef {
    pub kind: RuntimeSkillEvidenceKind,
    pub safe_ref: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillDeliveryDropReason {
    QueryUnmatched,
    LifecycleBlocked,
    ApplicabilityBlocked,
    ProfileBlocked,
    PrivacyBlocked,
    RequiredPremiseBlocked,
    ProjectionBlocked,
    RenderBudgetExceeded,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeSkillProjectionMaterial {
    candidate_ref: String,
    provider_content: RuntimeSkillProceduralContent,
    content_digest: String,
    owner_binding: RuntimeSkillOwnerBinding,
    materialized_view_ref: RuntimeSkillMaterializedViewRef,
}

impl RuntimeSkillProjectionMaterial {
    pub fn candidate_ref(&self) -> &str {
        &self.candidate_ref
    }

    pub fn provider_content(&self) -> &RuntimeSkillProceduralContent {
        &self.provider_content
    }

    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }

    pub(crate) fn validates_plan(&self, plan: &RuntimeSkillRecallPlan) -> bool {
        plan.selected
            && self.owner_binding == plan.owner_binding
            && self.materialized_view_ref == plan.materialized_view_ref
            && self.content_digest == plan.owner_binding.content_digest
            && derive_runtime_skill_projection_candidate_ref(
                plan,
                &self.owner_binding,
                &self.content_digest,
            )
            .is_ok_and(|candidate_ref| candidate_ref == self.candidate_ref)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSkillProjectionRenderOutcome {
    NotRequested,
    Rendered,
    DroppedBudget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillProjectionRenderReceipt {
    outcome: RuntimeSkillProjectionRenderOutcome,
    candidate_ref: Option<String>,
    content_digest: Option<String>,
}

impl RuntimeSkillProjectionRenderReceipt {
    pub const fn not_requested() -> Self {
        Self {
            outcome: RuntimeSkillProjectionRenderOutcome::NotRequested,
            candidate_ref: None,
            content_digest: None,
        }
    }

    pub fn try_rendered(
        candidate_ref: impl Into<String>,
        content_digest: impl Into<String>,
    ) -> crate::error::Result<Self> {
        Self::try_for_material(
            RuntimeSkillProjectionRenderOutcome::Rendered,
            candidate_ref.into(),
            content_digest.into(),
        )
    }

    pub fn try_dropped_budget(
        candidate_ref: impl Into<String>,
        content_digest: impl Into<String>,
    ) -> crate::error::Result<Self> {
        Self::try_for_material(
            RuntimeSkillProjectionRenderOutcome::DroppedBudget,
            candidate_ref.into(),
            content_digest.into(),
        )
    }

    pub const fn outcome(&self) -> RuntimeSkillProjectionRenderOutcome {
        self.outcome
    }

    pub fn candidate_ref(&self) -> Option<&str> {
        self.candidate_ref.as_deref()
    }

    pub fn content_digest(&self) -> Option<&str> {
        self.content_digest.as_deref()
    }

    pub(crate) fn matches_material(&self, material: &RuntimeSkillProjectionMaterial) -> bool {
        self.candidate_ref() == Some(material.candidate_ref())
            && self.content_digest() == Some(material.content_digest())
    }

    fn try_for_material(
        outcome: RuntimeSkillProjectionRenderOutcome,
        candidate_ref: String,
        content_digest: String,
    ) -> crate::error::Result<Self> {
        if outcome == RuntimeSkillProjectionRenderOutcome::NotRequested
            || !is_runtime_skill_projection_candidate_ref(&candidate_ref)
            || !is_sha256_digest(&content_digest)
        {
            return Err(crate::error::Error::config(
                "runtime_skill_projection_render_receipt",
                "render receipt must bind a canonical projection candidate and content digest",
            ));
        }
        Ok(Self {
            outcome,
            candidate_ref: Some(candidate_ref),
            content_digest: Some(content_digest),
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeSkillRecallPlan {
    schema_version: u32,
    memory_space_id: String,
    owning_scope: RuntimeSkillOwningScope,
    manifest_revision: u64,
    owner_binding: RuntimeSkillOwnerBinding,
    operation_authority_ref: RuntimeSkillOperationAuthorityRef,
    materialized_view_ref: RuntimeSkillMaterializedViewRef,
    query_time: u64,
    query: RuntimeSkillRecallQuery,
    applicability_context: RuntimeSkillApplicabilityContext,
    premise_observations: Vec<RuntimeSkillPremiseObservation>,
    authority: RuntimeSkillRecallAuthority,
    budget: RuntimeSkillRecallBudgetAuthority,
    materialized_lineage_depth: usize,
    projection_policy: RuntimeSkillProjectionPolicy,
    evidence_bindings: Vec<RuntimeSkillEvidenceBinding>,
    premise_report: PremiseEvaluationReport,
    matched: bool,
    selected: bool,
    drop_reasons: Vec<RuntimeSkillDeliveryDropReason>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSkillRecallPlanFailure {
    SchemaMismatch,
    ApplicabilityInvalid,
    ScopeInvalid,
    ExactOwnerMismatch,
    MaterializedViewMismatch,
    QueryInvalid,
    BudgetExceeded,
    PremiseBudgetExceeded,
    PremiseValidityInvalid,
    PremiseReportMismatch,
    SafeReferenceInvalid,
    CapabilityAffinityDuplicate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSkillRecallPlanValidation {
    pub accepted: bool,
    pub failures: Vec<RuntimeSkillRecallPlanFailure>,
}

impl RuntimeSkillRecallPlan {
    pub fn premise_report(&self) -> &PremiseEvaluationReport {
        &self.premise_report
    }

    pub const fn matched(&self) -> bool {
        self.matched
    }

    pub const fn selected(&self) -> bool {
        self.selected
    }

    pub fn drop_reasons(&self) -> &[RuntimeSkillDeliveryDropReason] {
        &self.drop_reasons
    }

    pub(crate) fn memory_space_id(&self) -> &str {
        &self.memory_space_id
    }

    pub(crate) fn owning_scope(&self) -> &RuntimeSkillOwningScope {
        &self.owning_scope
    }

    pub(crate) const fn manifest_revision(&self) -> u64 {
        self.manifest_revision
    }

    pub(crate) fn owner_binding(&self) -> &RuntimeSkillOwnerBinding {
        &self.owner_binding
    }

    pub(crate) const fn query_time(&self) -> u64 {
        self.query_time
    }

    pub(crate) fn projection_policy(&self) -> &RuntimeSkillProjectionPolicy {
        &self.projection_policy
    }

    pub(crate) fn evidence_bindings(&self) -> &[RuntimeSkillEvidenceBinding] {
        &self.evidence_bindings
    }

    pub fn validate_for(
        &self,
        owner: &RuntimeSkillOwnerRecord,
        manifest: &RuntimeSkillScopeManifest,
    ) -> RuntimeSkillRecallPlanValidation {
        let expected = build_runtime_skill_recall_plan(
            owner,
            manifest,
            self.operation_authority_ref.clone(),
            self.query_time,
            self.query.clone(),
            self.applicability_context.clone(),
            self.premise_observations.clone(),
            self.authority.clone(),
            self.budget,
        );
        match expected {
            Ok(expected) if self == &expected => validation_from_failures(Vec::new()),
            Ok(expected) => {
                let mut failures = Vec::new();
                if self.owner_binding != expected.owner_binding
                    || self.memory_space_id != expected.memory_space_id
                    || self.owning_scope != expected.owning_scope
                    || self.manifest_revision != expected.manifest_revision
                {
                    failures.push(RuntimeSkillRecallPlanFailure::ExactOwnerMismatch);
                }
                if self.materialized_view_ref != expected.materialized_view_ref {
                    failures.push(RuntimeSkillRecallPlanFailure::MaterializedViewMismatch);
                }
                if self.premise_report != expected.premise_report {
                    failures.push(RuntimeSkillRecallPlanFailure::PremiseReportMismatch);
                }
                if failures.is_empty() {
                    failures.push(RuntimeSkillRecallPlanFailure::ExactOwnerMismatch);
                }
                validation_from_failures(failures)
            }
            Err(_) => {
                validation_from_failures(vec![RuntimeSkillRecallPlanFailure::ExactOwnerMismatch])
            }
        }
    }
}

pub fn build_runtime_skill_projection_material(
    owner: &RuntimeSkillOwnerRecord,
    manifest: &RuntimeSkillScopeManifest,
    plan: &RuntimeSkillRecallPlan,
) -> crate::error::Result<Option<RuntimeSkillProjectionMaterial>> {
    if !plan.validate_for(owner, manifest).accepted {
        return Err(crate::error::Error::config(
            "runtime_skill_projection_material",
            "projection material requires the exact canonical owner, manifest, and recall plan",
        ));
    }
    if !plan.selected() {
        return Ok(None);
    }
    let owner_binding = RuntimeSkillOwnerBinding::from_record(owner)?;
    let content_digest = owner.content_digest.clone();
    let candidate_ref =
        derive_runtime_skill_projection_candidate_ref(plan, &owner_binding, &content_digest)?;
    Ok(Some(RuntimeSkillProjectionMaterial {
        candidate_ref,
        provider_content: owner.procedural_content.clone(),
        content_digest,
        owner_binding,
        materialized_view_ref: plan.materialized_view_ref.clone(),
    }))
}

pub fn runtime_skill_projection_candidate_ref(
    plan: &RuntimeSkillRecallPlan,
) -> crate::error::Result<String> {
    derive_runtime_skill_projection_candidate_ref(
        plan,
        &plan.owner_binding,
        &plan.owner_binding.content_digest,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_runtime_skill_recall_plan(
    owner: &RuntimeSkillOwnerRecord,
    manifest: &RuntimeSkillScopeManifest,
    operation_authority_ref: RuntimeSkillOperationAuthorityRef,
    query_time: u64,
    query: RuntimeSkillRecallQuery,
    applicability_context: RuntimeSkillApplicabilityContext,
    mut premise_observations: Vec<RuntimeSkillPremiseObservation>,
    authority: RuntimeSkillRecallAuthority,
    budget: RuntimeSkillRecallBudgetAuthority,
) -> crate::error::Result<RuntimeSkillRecallPlan> {
    let materialized_lineage_depth = usize::try_from(owner.owner_revision).map_err(|_| {
        crate::error::Error::config(
            "runtime_skill_recall_plan",
            "owner revision exceeds the host lineage range",
        )
    })?;
    if !owner.validate_contract().accepted
        || !budget.validate_contract()
        || query_time == 0
        || !query.validate_contract()
        || !applicability_context.validate_contract()
        || materialized_lineage_depth == 0
        || materialized_lineage_depth > budget.max_runtime_skill_lineage_depth
        || owner.intrinsic_contract.premises.len() > budget.max_premises_per_skill
        || manifest.owner_count > budget.max_procedural_candidates
        || manifest
            .validate_exact(
                &owner.memory_space_id,
                &owner.owning_scope,
                manifest.owner_bindings.clone(),
                budget.max_retained_runtime_skill_owners_per_scope,
            )
            .is_err()
    {
        return Err(crate::error::Error::config(
            "runtime_skill_recall_plan",
            "owner closure, query, applicability, lineage, or budget authority is invalid",
        ));
    }
    let owner_binding = RuntimeSkillOwnerBinding::from_record(owner)?;
    if manifest.memory_space_id != owner.memory_space_id
        || manifest.owning_scope != owner.owning_scope
        || manifest
            .owner_bindings
            .iter()
            .filter(|binding| *binding == &owner_binding)
            .count()
            != 1
    {
        return Err(crate::error::Error::config(
            "runtime_skill_recall_plan",
            "manifest does not contain the exact owner binding",
        ));
    }

    premise_observations.sort_by_key(RuntimeSkillPremiseObservation::canonical_identity);
    if premise_observations
        .windows(2)
        .any(|pair| pair[0].canonical_identity() == pair[1].canonical_identity())
        || premise_observations.len() > budget.max_premise_evidence_reads
        || !authority.governed_environment_evidence_allowed()
            && premise_observations.iter().any(|observation| {
                matches!(
                    observation,
                    RuntimeSkillPremiseObservation::GovernedEnvironmentEvidence { .. }
                )
            })
        || !authority.task_evidence_allowed()
            && premise_observations.iter().any(|observation| {
                matches!(
                    observation,
                    RuntimeSkillPremiseObservation::TaskEvidence { .. }
                )
            })
    {
        return Err(crate::error::Error::config(
            "runtime_skill_recall_plan",
            "premise observations are duplicate, forbidden, or over budget",
        ));
    }

    let premise_report = build_runtime_skill_premise_evaluation_report(
        &owner.intrinsic_contract.premises,
        &premise_observations,
        &owner.owning_scope,
        &authority,
        query_time,
        budget.max_premise_evidence_reads,
    )?;
    let matched = runtime_skill_query_matches(owner, &query);
    let applicability_matches =
        applicability_context.satisfies(&owner.intrinsic_contract.applicability);
    let lifecycle_allows = owner.lifecycle.availability == RuntimeSkillAvailability::Enabled
        && owner.lifecycle.state == RuntimeSkillLifecycleState::Active;
    let privacy_allows = authority.privacy_allows(&owner.owning_scope, owner.privacy_class);
    let projection_allows = owner
        .intrinsic_contract
        .projection_policy
        .model_projection_allowed
        && (!owner
            .intrinsic_contract
            .premises
            .iter()
            .any(|premise| premise.required)
            || owner
                .intrinsic_contract
                .projection_policy
                .require_all_mandatory_premises);

    let mut drop_reasons = Vec::new();
    if !matched {
        drop_reasons.push(RuntimeSkillDeliveryDropReason::QueryUnmatched);
    }
    if !lifecycle_allows {
        drop_reasons.push(RuntimeSkillDeliveryDropReason::LifecycleBlocked);
    }
    if !applicability_matches {
        drop_reasons.push(RuntimeSkillDeliveryDropReason::ApplicabilityBlocked);
    }
    if !authority.procedural_recall_allowed {
        drop_reasons.push(RuntimeSkillDeliveryDropReason::ProfileBlocked);
    }
    if !privacy_allows {
        drop_reasons.push(RuntimeSkillDeliveryDropReason::PrivacyBlocked);
    }
    if premise_report.required_failure_count > 0 {
        drop_reasons.push(RuntimeSkillDeliveryDropReason::RequiredPremiseBlocked);
    }
    if !projection_allows {
        drop_reasons.push(RuntimeSkillDeliveryDropReason::ProjectionBlocked);
    }
    drop_reasons.sort();
    drop_reasons.dedup();

    let materialized_view_ref = derive_runtime_skill_materialized_view_ref(
        &operation_authority_ref,
        manifest,
        &owner_binding,
        query_time,
        &query,
    )?;
    let selected = drop_reasons.is_empty();
    Ok(RuntimeSkillRecallPlan {
        schema_version: RUNTIME_SKILL_GOVERNED_CONTRACT_SCHEMA_VERSION,
        memory_space_id: owner.memory_space_id.clone(),
        owning_scope: owner.owning_scope.clone(),
        manifest_revision: manifest.revision,
        owner_binding,
        operation_authority_ref,
        materialized_view_ref,
        query_time,
        query,
        applicability_context,
        premise_observations,
        authority,
        budget,
        materialized_lineage_depth,
        projection_policy: owner.intrinsic_contract.projection_policy.clone(),
        evidence_bindings: owner.intrinsic_contract.evidence_bindings.clone(),
        premise_report,
        matched,
        selected,
        drop_reasons,
    })
}

fn validate_intrinsic_contract_fields(
    schema_version: u32,
    applicability: Option<&RuntimeSkillApplicability>,
    triggers: &[RuntimeSkillTrigger],
    constraints: &[RuntimeSkillConstraint],
    premises: &[RuntimeSkillPremiseRequirement],
    evidence_bindings: &[RuntimeSkillEvidenceBinding],
    capability_affinities: &[RuntimeSkillCapabilityAffinity],
) -> Vec<RuntimeSkillRecallPlanFailure> {
    let mut failures = Vec::new();
    if schema_version != RUNTIME_SKILL_GOVERNED_CONTRACT_SCHEMA_VERSION {
        failures.push(RuntimeSkillRecallPlanFailure::SchemaMismatch);
    }
    if applicability.is_some_and(|value| !value.validate_contract()) {
        failures.push(RuntimeSkillRecallPlanFailure::ApplicabilityInvalid);
    }
    if premises.iter().any(|requirement| {
        requirement
            .valid_until
            .is_some_and(|valid_until| valid_until <= requirement.valid_from)
    }) {
        failures.push(RuntimeSkillRecallPlanFailure::PremiseValidityInvalid);
    }
    if triggers
        .iter()
        .any(|value| !is_canonical(&value.canonical_ref))
        || constraints
            .iter()
            .any(|value| !is_canonical(&value.policy_safe_ref))
        || evidence_bindings
            .iter()
            .any(|value| !is_canonical(&value.safe_ref) || !is_sha256_digest(&value.source_digest))
        || premises.iter().any(|requirement| {
            requirement.governed_evidence_refs.iter().any(|evidence| {
                !evidence.is_valid()
                    || evidence.owner_ref.owner_plane != GovernedMemoryOwnerPlane::EvidenceDocument
            }) || match &requirement.premise {
                RuntimeSkillPremise::RegisteredCapability {
                    capability_id,
                    version_constraint,
                } => {
                    !is_canonical(capability_id)
                        || matches!(
                            (version_constraint.min_inclusive, version_constraint.max_exclusive),
                            (Some(min), Some(max)) if max <= min
                        )
                }
                RuntimeSkillPremise::GovernedEnvironmentEvidence {
                    evidence_revision_ref,
                } => {
                    !evidence_revision_ref.is_valid()
                        || evidence_revision_ref.owner_ref.owner_plane
                            != GovernedMemoryOwnerPlane::EvidenceDocument
                }
                RuntimeSkillPremise::OpaquePresenceAttestation { handle_ref } => {
                    !is_canonical(handle_ref)
                }
                RuntimeSkillPremise::TaskEvidence {
                    source,
                    evidence_kind,
                    safe_ref,
                } => {
                    !task_evidence_source_matches(*source, *evidence_kind)
                        || !is_canonical(safe_ref)
                }
            }
        })
    {
        failures.push(RuntimeSkillRecallPlanFailure::SafeReferenceInvalid);
    }
    let affinities = capability_affinities.iter().collect::<BTreeSet<_>>();
    if affinities.len() != capability_affinities.len() {
        failures.push(RuntimeSkillRecallPlanFailure::CapabilityAffinityDuplicate);
    }
    failures
}

fn validation_from_failures(
    mut failures: Vec<RuntimeSkillRecallPlanFailure>,
) -> RuntimeSkillRecallPlanValidation {
    failures.sort();
    failures.dedup();
    RuntimeSkillRecallPlanValidation {
        accepted: failures.is_empty(),
        failures,
    }
}

fn canonicalize_intrinsic_contract(
    contract: &mut RuntimeSkillIntrinsicContract,
) -> crate::error::Result<()> {
    contract.triggers.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.canonical_ref.cmp(&right.canonical_ref))
    });
    if contract.triggers.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(crate::error::Error::config(
            "runtime_skill_intrinsic_contract",
            "triggers must be unique",
        ));
    }

    contract.constraints.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.policy_safe_ref.cmp(&right.policy_safe_ref))
    });
    if contract
        .constraints
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        return Err(crate::error::Error::config(
            "runtime_skill_intrinsic_contract",
            "constraints must be unique",
        ));
    }

    let mut premise_keys = contract
        .premises
        .drain(..)
        .map(|premise| {
            serde_json::to_vec(&premise)
                .map(|key| (key, premise))
                .map_err(|error| {
                    crate::error::Error::config(
                        "runtime_skill_intrinsic_contract",
                        error.to_string(),
                    )
                })
        })
        .collect::<crate::error::Result<Vec<_>>>()?;
    premise_keys.sort_by(|left, right| left.0.cmp(&right.0));
    if premise_keys.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(crate::error::Error::config(
            "runtime_skill_intrinsic_contract",
            "premises must be unique",
        ));
    }
    contract.premises = premise_keys
        .into_iter()
        .map(|(_, premise)| premise)
        .collect();

    contract.failure_modes.sort();
    if contract
        .failure_modes
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        return Err(crate::error::Error::config(
            "runtime_skill_intrinsic_contract",
            "failure modes must be unique",
        ));
    }
    contract.evidence_bindings.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.safe_ref.cmp(&right.safe_ref))
            .then_with(|| left.source_digest.cmp(&right.source_digest))
    });
    if contract
        .evidence_bindings
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        return Err(crate::error::Error::config(
            "runtime_skill_intrinsic_contract",
            "evidence bindings must be unique",
        ));
    }
    contract.capability_affinities.sort();
    if contract
        .capability_affinities
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        return Err(crate::error::Error::config(
            "runtime_skill_intrinsic_contract",
            "capability affinities must be unique",
        ));
    }
    Ok(())
}

fn runtime_skill_scope_bindings_digest(
    revision: u64,
    memory_space_id: &str,
    owning_scope: &RuntimeSkillOwningScope,
    bindings: &[RuntimeSkillOwnerBinding],
) -> crate::error::Result<String> {
    let encoded = serde_json::to_vec(bindings).map_err(|error| {
        crate::error::Error::config("runtime_skill_scope_manifest", error.to_string())
    })?;
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        RUNTIME_SKILL_SCOPE_BINDINGS_DIGEST_DOMAIN.as_bytes(),
    );
    hash_field(&mut hasher, &revision.to_be_bytes());
    hash_field(&mut hasher, memory_space_id.as_bytes());
    hash_owning_scope(&mut hasher, owning_scope);
    hash_field(&mut hasher, &encoded);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn derive_runtime_skill_projection_candidate_ref(
    plan: &RuntimeSkillRecallPlan,
    owner_binding: &RuntimeSkillOwnerBinding,
    content_digest: &str,
) -> crate::error::Result<String> {
    if owner_binding != &plan.owner_binding
        || content_digest != plan.owner_binding.content_digest
        || !is_sha256_digest(content_digest)
    {
        return Err(crate::error::Error::config(
            "runtime_skill_projection_candidate_ref",
            "projection candidate must bind the canonical plan and owner content",
        ));
    }
    let encoded_owner_binding = serde_json::to_vec(owner_binding).map_err(|error| {
        crate::error::Error::config("runtime_skill_projection_candidate_ref", error.to_string())
    })?;
    let digest = domain_separated_sha256(
        RUNTIME_SKILL_PROJECTION_CANDIDATE_REF_DOMAIN,
        &[
            plan.operation_authority_ref.value.as_bytes(),
            plan.materialized_view_ref.value.as_bytes(),
            &encoded_owner_binding,
            content_digest.as_bytes(),
        ],
    );
    Ok(format!("runtime_skill_projection_candidate:{digest}"))
}

fn validate_physical_scope(
    memory_space_id: &str,
    owning_scope: &RuntimeSkillOwningScope,
    stage: &'static str,
) -> crate::error::Result<()> {
    if !is_canonical(memory_space_id)
        || owning_scope
            .canonical_subject_id()
            .is_some_and(|subject_id| !is_canonical(subject_id))
    {
        return Err(crate::error::Error::config(
            stage,
            "memory space and physical owning scope must be canonical",
        ));
    }
    Ok(())
}

fn hash_owning_scope(hasher: &mut Sha256, owning_scope: &RuntimeSkillOwningScope) {
    match owning_scope {
        RuntimeSkillOwningScope::Subject { mounted_subject_id } => {
            hash_field(hasher, b"subject");
            hash_field(hasher, mounted_subject_id.as_bytes());
        }
        RuntimeSkillOwningScope::SharedProgram => hash_field(hasher, b"shared_program"),
    }
}

fn domain_separated_sha256(domain: &str, fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, domain.as_bytes());
    for field in fields {
        hash_field(&mut hasher, field);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn derive_runtime_skill_materialized_view_ref(
    operation_authority_ref: &RuntimeSkillOperationAuthorityRef,
    manifest: &RuntimeSkillScopeManifest,
    owner_binding: &RuntimeSkillOwnerBinding,
    query_time: u64,
    query: &RuntimeSkillRecallQuery,
) -> crate::error::Result<RuntimeSkillMaterializedViewRef> {
    let encoded = serde_json::to_vec(&(
        operation_authority_ref,
        manifest,
        owner_binding,
        query_time,
        query,
    ))
    .map_err(|error| {
        crate::error::Error::config("runtime_skill_materialized_view_ref", error.to_string())
    })?;
    let digest = domain_separated_sha256(RUNTIME_SKILL_MATERIALIZED_VIEW_REF_DOMAIN, &[&encoded]);
    RuntimeSkillMaterializedViewRef::try_new(format!("runtime_skill_view:{digest}"))
}

fn runtime_skill_query_matches(
    owner: &RuntimeSkillOwnerRecord,
    query: &RuntimeSkillRecallQuery,
) -> bool {
    let mut searchable = canonical_runtime_skill_terms(&format!(
        "{} {} {} {}",
        owner.procedural_content.title,
        owner.procedural_content.topic,
        owner.procedural_content.summary,
        owner.procedural_content.procedure
    ));
    searchable.extend(
        owner
            .intrinsic_contract
            .triggers
            .iter()
            .flat_map(|trigger| canonical_runtime_skill_terms(&trigger.canonical_ref)),
    );
    searchable.sort();
    searchable.dedup();
    query
        .terms
        .iter()
        .any(|term| searchable.binary_search(term).is_ok())
}

fn canonical_runtime_skill_terms(text: &str) -> Vec<String> {
    let mut terms = text
        .split(|character: char| !character.is_alphanumeric())
        .filter_map(|term| {
            let normalized = term.to_lowercase();
            (!normalized.is_empty() && normalized.len() <= MAX_RUNTIME_SKILL_QUERY_TERM_BYTES)
                .then_some(normalized)
        })
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms.truncate(MAX_RUNTIME_SKILL_QUERY_TERMS);
    terms
}

const fn task_evidence_source_matches(
    source: PremiseTypedSource,
    evidence_kind: RuntimeSkillEvidenceKind,
) -> bool {
    matches!(
        (source, evidence_kind),
        (
            PremiseTypedSource::TaskLearning,
            RuntimeSkillEvidenceKind::TaskLearning
        ) | (
            PremiseTypedSource::TaskRun,
            RuntimeSkillEvidenceKind::TaskRun
        ) | (
            PremiseTypedSource::TaskArtifact,
            RuntimeSkillEvidenceKind::TaskArtifact
        )
    )
}

fn is_prefixed_sha256(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(is_lower_hex_sha256)
}

fn is_canonical(value: &str) -> bool {
    !value.trim().is_empty() && value == value.trim()
}

fn is_runtime_skill_owner_id(value: &str) -> bool {
    value
        .strip_prefix("runtime_skill:sha256:")
        .is_some_and(is_lower_hex_sha256)
}

fn is_runtime_skill_projection_candidate_ref(value: &str) -> bool {
    value
        .strip_prefix("runtime_skill_projection_candidate:sha256:")
        .is_some_and(is_lower_hex_sha256)
}

fn is_sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(is_lower_hex_sha256)
}

fn is_lower_hex_sha256(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}
