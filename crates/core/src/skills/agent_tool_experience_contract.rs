//! Canonical, backend-neutral owner contract for subject-scoped Agent Tool experience.
//!
//! This module defines identity and immutable closure only. Store admission and runtime
//! selection are deliberately owned by later PFI1 substages.

use std::collections::{BTreeMap, BTreeSet};

use serde::{ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::memory::{
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, GovernedOwnerRevisionRef, MemoryPrivacyClass,
};

use super::{AgentToolExperienceStatus, AgentToolRegistryScope};

pub const AGENT_TOOL_EXPERIENCE_MATERIAL_SCHEMA_VERSION: u32 = 3;
pub const AGENT_TOOL_EXPERIENCE_HEAD_SCHEMA_VERSION: u32 = 3;
pub const AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Non-wire projection of one fully verified history under current authority.
/// Denied histories leave only a fixed reason here, never their private bodies.
#[derive(Clone, Debug)]
pub struct AgentToolExperienceReadProjectionV1 {
    memory_space_id: String,
    owning_scope: AgentToolExperienceOwningScopeV1,
    as_of_time: Option<u64>,
    selected: Option<AgentToolExperienceRevisionMaterialV3>,
    rejection: Option<&'static str>,
}

impl AgentToolExperienceReadProjectionV1 {
    pub fn try_new(
        head: &AgentToolExperienceOwnerHeadV3,
        history: &[AgentToolExperienceRevisionMaterialV3],
        authority: &crate::memory::ProceduralReadAuthorityV1<'_>,
        as_of_time: Option<u64>,
    ) -> Result<Self> {
        let invalid = || {
            Error::config(
                "agent_tool_experience_read_projection",
                "exact owner history or read authority is invalid",
            )
        };
        if !head.validate_contract().accepted
            || head.memory_space_id != authority.scope().memory_space_id
            || head.owning_scope.mounted_subject_id() != authority.scope().mounted_subject_id
        {
            return Err(invalid());
        }
        let mut projection = Self {
            memory_space_id: head.memory_space_id.clone(),
            owning_scope: head.owning_scope.clone(),
            as_of_time,
            selected: None,
            rejection: None,
        };
        if head.state == AgentToolExperienceHeadStateV3::Tombstoned {
            if !history.is_empty() {
                return Err(invalid());
            }
            projection.rejection = Some("agent_tool_experience_source_withdrawn");
            return Ok(projection);
        }
        validate_agent_tool_experience_owner_history(history)?;
        let retained = history
            .iter()
            .map(AgentToolExperienceRetainedRevisionDigestV3::from_material)
            .collect::<Result<Vec<_>>>()?;
        if retained != head.retained_revisions
            || history.last().is_none_or(|material| {
                material.owner_revision != head.current_revision
                    || material.memory_space_id != head.memory_space_id
                    || material.owning_scope != head.owning_scope
            })
        {
            return Err(invalid());
        }
        let current = history.last().ok_or_else(invalid)?;
        let binding = |material: &AgentToolExperienceRevisionMaterialV3| {
            crate::memory::ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                owner_revision: material.owner_revision_ref(),
                content_digest: material.content_digest.clone(),
            }
        };
        if !authority.permits_exact(&binding(current))? {
            projection.rejection = Some("agent_tool_experience_authority_withdrawn");
        } else if current.status != AgentToolExperienceStatus::Active {
            projection.rejection = Some("agent_tool_experience_status_ineligible");
        } else if let Some(selected) = history
            .iter()
            .rev()
            .find(|material| as_of_time.is_none_or(|time| material.updated_at <= time))
        {
            // Select first, authorize second. Never fall back to older allowed
            // content when the exact revision at this anchor has been withdrawn.
            if authority.permits_exact(&binding(selected))? {
                projection.selected = Some(selected.clone());
            } else {
                projection.rejection = Some("agent_tool_experience_authority_withdrawn");
            }
        } else {
            projection.rejection = Some("agent_tool_experience_no_revision_at_anchor");
        }
        Ok(projection)
    }

    pub fn memory_space_id(&self) -> &str {
        &self.memory_space_id
    }
    pub fn owning_scope(&self) -> &AgentToolExperienceOwningScopeV1 {
        &self.owning_scope
    }
    pub fn as_of_time(&self) -> Option<u64> {
        self.as_of_time
    }
    pub fn material(&self) -> Option<&AgentToolExperienceRevisionMaterialV3> {
        self.selected.as_ref()
    }
    pub fn rejection(&self) -> Option<&'static str> {
        self.rejection
    }
}

const OWNER_ID_DOMAIN: &str = "agent_tool_experience_owner_id_v3";
const MATERIAL_KEY_DOMAIN: &str = "agent_tool_experience_material_key_v3";
const HEAD_KEY_DOMAIN: &str = "agent_tool_experience_head_key_v3";
const MANIFEST_KEY_DOMAIN: &str = "agent_tool_experience_scope_manifest_key_v1";
const MATERIAL_DIGEST_DOMAIN: &str = "agent_tool_experience_material_digest_v3";
const HEAD_DIGEST_DOMAIN: &str = "agent_tool_experience_head_digest_v3";
const MANIFEST_DIGEST_DOMAIN: &str = "agent_tool_experience_manifest_digest_v1";

/// A view derived from the single material body, never a second persisted tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentToolExperienceFocusV1<'a> {
    Execution,
    Method { task_signature: &'a str },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolMethodSourceRefV1 {
    pub source_job_id: String,
    pub method_id: String,
    pub method_digest: String,
    pub execution_refs: Vec<crate::memory::ProceduralContributionRefV1>,
}

impl AgentToolMethodSourceRefV1 {
    pub fn validate_contract(&self) -> bool {
        bounded_identifier(&self.source_job_id)
            && bounded_identifier(&self.method_id)
            && is_digest(&self.method_digest)
            && canonical_contribution_refs(&self.execution_refs)
            && self
                .execution_refs
                .iter()
                .all(|reference| reference.source_job_id == self.source_job_id)
    }
}

/// Execution metadata has no path to become procedure text. Method confidence
/// and execution success rate are not duplicated into each other's branch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentToolExperienceBodyV1 {
    Execution {
        counts: crate::memory::ToolExecutionCountsV1,
        contributions: Vec<crate::memory::ProceduralContributionRefV1>,
    },
    Method {
        task_signature: String,
        procedure: String,
        constraints: Vec<String>,
        sources: Vec<AgentToolMethodSourceRefV1>,
    },
}

impl AgentToolExperienceBodyV1 {
    pub fn focus(&self) -> AgentToolExperienceFocusV1<'_> {
        match self {
            Self::Execution { .. } => AgentToolExperienceFocusV1::Execution,
            Self::Method { task_signature, .. } => {
                AgentToolExperienceFocusV1::Method { task_signature }
            }
        }
    }

    pub fn method_content_digest(&self) -> Result<Option<String>> {
        match self {
            Self::Execution { .. } => Ok(None),
            Self::Method {
                task_signature,
                procedure,
                ..
            } => {
                let encoded = serde_json::to_vec(&(task_signature, procedure)).map_err(|_| {
                    Error::config("agent_tool_method_digest", "method digest encoding failed")
                })?;
                Ok(Some(domain_digest(
                    "agent_tool_method_content_digest_v1",
                    &[&encoded],
                )))
            }
        }
    }

    pub fn contribution_count(&self) -> usize {
        match self {
            Self::Execution { contributions, .. } => contributions.len(),
            Self::Method { sources, .. } => sources
                .iter()
                .map(|source| source.execution_refs.len().saturating_add(1))
                .fold(0, usize::saturating_add),
        }
    }

    pub fn validate_contract(&self) -> bool {
        match self {
            Self::Execution {
                counts,
                contributions,
            } => {
                canonical_contribution_refs(contributions)
                    && counts.total_count() == u64::try_from(contributions.len()).ok()
            }
            Self::Method {
                task_signature,
                procedure,
                constraints,
                sources,
            } => {
                bounded_identifier(task_signature)
                    && procedure.len() <= crate::memory::MAX_PROCEDURAL_METHOD_BYTES
                    && is_canonical_text(procedure)
                    && super::validate_runtime_skill_method_shape(task_signature, procedure).is_ok()
                    && constraints.iter().all(|value| bounded_identifier(value))
                    && canonical_unique(constraints)
                    && !sources.is_empty()
                    && sources
                        .iter()
                        .all(AgentToolMethodSourceRefV1::validate_contract)
                    && sources.windows(2).all(|pair| {
                        (&pair[0].source_job_id, &pair[0].method_id)
                            < (&pair[1].source_job_id, &pair[1].method_id)
                    })
                    && self.method_content_digest().is_ok_and(|digest| {
                        sources
                            .iter()
                            .all(|source| Some(&source.method_digest) == digest.as_ref())
                    })
            }
        }
    }
}

fn bounded_identifier(value: &str) -> bool {
    value.len() <= 256 && is_canonical(value)
}

fn canonical_contribution_refs(references: &[crate::memory::ProceduralContributionRefV1]) -> bool {
    references
        .iter()
        .all(crate::memory::ProceduralContributionRefV1::validate_contract)
        && references
            .windows(2)
            .all(|pair| pair[0].contribution_id < pair[1].contribution_id)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentToolExperienceOwningScopeV1 {
    Subject { mounted_subject_id: String },
}

impl AgentToolExperienceOwningScopeV1 {
    pub fn mounted_subject_id(&self) -> &str {
        match self {
            Self::Subject { mounted_subject_id } => mounted_subject_id,
        }
    }

    pub fn validate_contract(&self) -> bool {
        is_canonical(self.mounted_subject_id())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentToolExperienceOwnerLocatorV3 {
    memory_space_id: String,
    owning_scope: AgentToolExperienceOwningScopeV1,
    owner_revision_ref: GovernedOwnerRevisionRef,
}

impl AgentToolExperienceOwnerLocatorV3 {
    pub fn try_new(
        memory_space_id: impl Into<String>,
        owning_scope: AgentToolExperienceOwningScopeV1,
        owner_id: impl Into<String>,
        owner_revision: u64,
    ) -> Result<Self> {
        let memory_space_id = memory_space_id.into();
        let owner_id = owner_id.into();
        if !is_canonical(&memory_space_id)
            || !owning_scope.validate_contract()
            || !is_owner_id(&owner_id)
        {
            return Err(Error::config(
                "agent_tool_experience_owner_locator",
                "memory space, subject scope, and owner id must be canonical",
            ));
        }
        Ok(Self {
            memory_space_id,
            owning_scope,
            owner_revision_ref: GovernedOwnerRevisionRef::try_new(
                GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::AgentToolExperience,
                    owner_id,
                ),
                owner_revision,
            )?,
        })
    }

    pub fn from_material(material: &AgentToolExperienceRevisionMaterialV3) -> Self {
        Self {
            memory_space_id: material.memory_space_id.clone(),
            owning_scope: material.owning_scope.clone(),
            owner_revision_ref: material.owner_revision_ref(),
        }
    }

    pub fn memory_space_id(&self) -> &str {
        &self.memory_space_id
    }

    pub fn owning_scope(&self) -> &AgentToolExperienceOwningScopeV1 {
        &self.owning_scope
    }

    pub fn owner_id(&self) -> &str {
        &self.owner_revision_ref.owner_ref.owner_id
    }

    pub fn owner_revision(&self) -> u64 {
        self.owner_revision_ref.owner_revision
    }
}

impl Serialize for AgentToolExperienceOwnerLocatorV3 {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("AgentToolExperienceOwnerLocatorV3", 4)?;
        state.serialize_field("memory_space_id", self.memory_space_id())?;
        state.serialize_field("owning_scope", self.owning_scope())?;
        state.serialize_field("owner_id", self.owner_id())?;
        state.serialize_field("owner_revision", &self.owner_revision())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for AgentToolExperienceOwnerLocatorV3 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            memory_space_id: String,
            owning_scope: AgentToolExperienceOwningScopeV1,
            owner_id: String,
            owner_revision: u64,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::try_new(
            wire.memory_space_id,
            wire.owning_scope,
            wire.owner_id,
            wire.owner_revision,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolExperienceRetainedRevisionDigestV3 {
    pub owner_ref: GovernedMemoryOwnerRef,
    pub owner_revision: u64,
    pub material_key: String,
    pub content_digest: String,
}

impl AgentToolExperienceRetainedRevisionDigestV3 {
    pub fn from_material(material: &AgentToolExperienceRevisionMaterialV3) -> Result<Self> {
        if !material.validate_contract().accepted {
            return Err(Error::config(
                "agent_tool_experience_retained_revision",
                "material must be canonical before it can be retained",
            ));
        }
        Ok(Self {
            owner_ref: material.owner_ref.clone(),
            owner_revision: material.owner_revision,
            material_key: material.physical_key.clone(),
            content_digest: material.content_digest.clone(),
        })
    }

    fn validate_for(
        &self,
        memory_space_id: &str,
        owning_scope: &AgentToolExperienceOwningScopeV1,
        owner_ref: &GovernedMemoryOwnerRef,
    ) -> bool {
        self.owner_ref == *owner_ref
            && self.owner_revision > 0
            && is_digest(&self.content_digest)
            && agent_tool_experience_material_key(
                memory_space_id,
                owning_scope,
                owner_ref,
                self.owner_revision,
            )
            .is_ok_and(|key| key == self.material_key)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolExperienceRevisionMaterialV3 {
    pub schema_version: u32,
    pub physical_key: String,
    pub memory_space_id: String,
    pub owning_scope: AgentToolExperienceOwningScopeV1,
    pub owner_ref: GovernedMemoryOwnerRef,
    pub owner_revision: u64,
    pub registry_id: String,
    pub registry_scope: AgentToolRegistryScope,
    pub tool_id: String,
    pub schema_fingerprint: String,
    pub body: AgentToolExperienceBodyV1,
    pub status: AgentToolExperienceStatus,
    pub privacy_class: MemoryPrivacyClass,
    pub created_at: u64,
    pub updated_at: u64,
    pub predecessor: Option<AgentToolExperienceRetainedRevisionDigestV3>,
    pub content_digest: String,
}

impl AgentToolExperienceRevisionMaterialV3 {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        memory_space_id: &str,
        owning_scope: AgentToolExperienceOwningScopeV1,
        registry_id: &str,
        registry_scope: AgentToolRegistryScope,
        tool_id: &str,
        schema_fingerprint: &str,
        owner_revision: u64,
        body: AgentToolExperienceBodyV1,
        status: AgentToolExperienceStatus,
        privacy_class: MemoryPrivacyClass,
        created_at: u64,
        updated_at: u64,
        predecessor: Option<AgentToolExperienceRetainedRevisionDigestV3>,
    ) -> Result<Self> {
        validate_identity_fields(
            memory_space_id,
            &owning_scope,
            registry_id,
            &registry_scope,
            tool_id,
            schema_fingerprint,
            &body.focus(),
        )?;
        if owner_revision == 0
            || created_at == 0
            || updated_at < created_at
            || !body.validate_contract()
            || predecessor
                .as_ref()
                .is_some_and(|value| value.owner_revision.checked_add(1) != Some(owner_revision))
            || (owner_revision == 1) != predecessor.is_none()
        {
            return Err(Error::config(
                "agent_tool_experience_material",
                "revision material is non-canonical or has invalid lineage",
            ));
        }
        let owner_id = canonical_agent_tool_experience_owner_id(
            memory_space_id,
            &owning_scope,
            registry_id,
            &registry_scope,
            tool_id,
            schema_fingerprint,
            &body.focus(),
        )?;
        let owner_ref =
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::AgentToolExperience, owner_id);
        if predecessor
            .as_ref()
            .is_some_and(|value| !value.validate_for(memory_space_id, &owning_scope, &owner_ref))
        {
            return Err(Error::config(
                "agent_tool_experience_material",
                "predecessor does not bind the exact owner and prior revision",
            ));
        }
        let physical_key = agent_tool_experience_material_key(
            memory_space_id,
            &owning_scope,
            &owner_ref,
            owner_revision,
        )?;
        let mut material = Self {
            schema_version: AGENT_TOOL_EXPERIENCE_MATERIAL_SCHEMA_VERSION,
            physical_key,
            memory_space_id: memory_space_id.to_string(),
            owning_scope,
            owner_ref,
            owner_revision,
            registry_id: registry_id.to_string(),
            registry_scope,
            tool_id: tool_id.to_string(),
            schema_fingerprint: schema_fingerprint.to_string(),
            body,
            status,
            privacy_class,
            created_at,
            updated_at,
            predecessor,
            content_digest: String::new(),
        };
        material.content_digest = material.canonical_content_digest()?;
        Ok(material)
    }

    pub fn owner_revision_ref(&self) -> GovernedOwnerRevisionRef {
        GovernedOwnerRevisionRef {
            owner_ref: self.owner_ref.clone(),
            owner_revision: self.owner_revision,
        }
    }

    pub fn canonical_content_digest(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.content_digest.clear();
        let encoded = serde_json::to_vec(&canonical).map_err(|_| {
            Error::config(
                "agent_tool_experience_material_digest",
                "material digest encoding failed",
            )
        })?;
        Ok(domain_digest(MATERIAL_DIGEST_DOMAIN, &[&encoded]))
    }

    pub fn validate_contract(&self) -> AgentToolExperienceContractValidation {
        let mut failures = Vec::new();
        if self.schema_version != AGENT_TOOL_EXPERIENCE_MATERIAL_SCHEMA_VERSION {
            failures.push(AgentToolExperienceContractFailure::SchemaMismatch);
        }
        if validate_identity_fields(
            &self.memory_space_id,
            &self.owning_scope,
            &self.registry_id,
            &self.registry_scope,
            &self.tool_id,
            &self.schema_fingerprint,
            &self.body.focus(),
        )
        .is_err()
        {
            failures.push(AgentToolExperienceContractFailure::ScopeInvalid);
        }
        let expected_owner = canonical_agent_tool_experience_owner_id(
            &self.memory_space_id,
            &self.owning_scope,
            &self.registry_id,
            &self.registry_scope,
            &self.tool_id,
            &self.schema_fingerprint,
            &self.body.focus(),
        );
        if self.owner_ref.owner_plane != GovernedMemoryOwnerPlane::AgentToolExperience
            || !expected_owner.is_ok_and(|value| value == self.owner_ref.owner_id)
        {
            failures.push(AgentToolExperienceContractFailure::OwnerIdentityInvalid);
        }
        if self.owner_revision == 0
            || (self.owner_revision == 1) != self.predecessor.is_none()
            || self.predecessor.as_ref().is_some_and(|value| {
                value.owner_revision.checked_add(1) != Some(self.owner_revision)
                    || !value.validate_for(
                        &self.memory_space_id,
                        &self.owning_scope,
                        &self.owner_ref,
                    )
            })
        {
            failures.push(AgentToolExperienceContractFailure::RevisionInvalid);
        }
        if !agent_tool_experience_material_key(
            &self.memory_space_id,
            &self.owning_scope,
            &self.owner_ref,
            self.owner_revision,
        )
        .is_ok_and(|value| value == self.physical_key)
        {
            failures.push(AgentToolExperienceContractFailure::PhysicalKeyMismatch);
        }
        if self.created_at == 0
            || self.updated_at < self.created_at
            || !self.body.validate_contract()
        {
            failures.push(AgentToolExperienceContractFailure::ContentInvalid);
        }
        if !self
            .canonical_content_digest()
            .is_ok_and(|value| value == self.content_digest)
        {
            failures.push(AgentToolExperienceContractFailure::DigestMismatch);
        }
        validation(failures)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolExperienceHeadStateV3 {
    Active,
    Tombstoned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolExperienceOwnerHeadV3 {
    pub schema_version: u32,
    pub physical_key: String,
    pub memory_space_id: String,
    pub owning_scope: AgentToolExperienceOwningScopeV1,
    pub owner_ref: GovernedMemoryOwnerRef,
    pub current_revision: u64,
    pub retained_revisions: Vec<AgentToolExperienceRetainedRevisionDigestV3>,
    pub state: AgentToolExperienceHeadStateV3,
    pub content_digest: String,
}

impl AgentToolExperienceOwnerHeadV3 {
    /// Retains only historical commitment digests; materials must be removed atomically.
    pub fn tombstone(&self) -> Result<Self> {
        if !self.validate_contract().accepted {
            return Err(Error::config(
                "agent_tool_experience_tombstone",
                "invalid owner head",
            ));
        }
        let mut head = self.clone();
        head.state = AgentToolExperienceHeadStateV3::Tombstoned;
        head.content_digest = head.canonical_content_digest()?;
        Ok(head)
    }

    pub fn build(
        memory_space_id: &str,
        owning_scope: AgentToolExperienceOwningScopeV1,
        owner_ref: GovernedMemoryOwnerRef,
        current_revision: u64,
        mut retained_revisions: Vec<AgentToolExperienceRetainedRevisionDigestV3>,
    ) -> Result<Self> {
        validate_scope(memory_space_id, &owning_scope)?;
        retained_revisions.sort_by_key(|value| value.owner_revision);
        if owner_ref.owner_plane != GovernedMemoryOwnerPlane::AgentToolExperience
            || !is_owner_id(&owner_ref.owner_id)
            || current_revision == 0
            || retained_revisions.is_empty()
            || retained_revisions.first().map(|value| value.owner_revision) != Some(1)
            || u64::try_from(retained_revisions.len()).ok() != Some(current_revision)
            || retained_revisions.last().map(|value| value.owner_revision) != Some(current_revision)
            || retained_revisions
                .windows(2)
                .any(|pair| pair[0].owner_revision.checked_add(1) != Some(pair[1].owner_revision))
            || retained_revisions
                .iter()
                .any(|value| !value.validate_for(memory_space_id, &owning_scope, &owner_ref))
        {
            return Err(Error::config(
                "agent_tool_experience_head",
                "head must retain one exact, contiguous owner history",
            ));
        }
        let physical_key =
            agent_tool_experience_head_key(memory_space_id, &owning_scope, &owner_ref)?;
        let mut head = Self {
            schema_version: AGENT_TOOL_EXPERIENCE_HEAD_SCHEMA_VERSION,
            physical_key,
            memory_space_id: memory_space_id.to_string(),
            owning_scope,
            owner_ref,
            current_revision,
            retained_revisions,
            state: AgentToolExperienceHeadStateV3::Active,
            content_digest: String::new(),
        };
        head.content_digest = head.canonical_content_digest()?;
        Ok(head)
    }

    pub fn canonical_content_digest(&self) -> Result<String> {
        let mut clone = self.clone();
        clone.content_digest.clear();
        let encoded = serde_json::to_vec(&clone).map_err(|error| {
            Error::config("agent_tool_experience_head_digest", error.to_string())
        })?;
        Ok(domain_digest(HEAD_DIGEST_DOMAIN, &[&encoded]))
    }

    pub fn validate_contract(&self) -> AgentToolExperienceContractValidation {
        let rebuilt = Self::build(
            &self.memory_space_id,
            self.owning_scope.clone(),
            self.owner_ref.clone(),
            self.current_revision,
            self.retained_revisions.clone(),
        );
        if rebuilt.is_ok_and(|mut value| {
            value.state = self.state;
            value.content_digest = value.canonical_content_digest().unwrap_or_default();
            value == *self
        }) {
            validation(Vec::new())
        } else {
            validation(vec![AgentToolExperienceContractFailure::HeadInvalid])
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolExperienceHeadBindingV1 {
    pub owner_ref: GovernedMemoryOwnerRef,
    pub current_revision: u64,
    pub head_key: String,
    pub head_digest: String,
    pub material_key: String,
    pub material_digest: String,
}

impl AgentToolExperienceHeadBindingV1 {
    pub fn from_head(head: &AgentToolExperienceOwnerHeadV3) -> Result<Self> {
        if !head.validate_contract().accepted {
            return Err(Error::config(
                "agent_tool_experience_head_binding",
                "invalid owner head",
            ));
        }
        let current = head.retained_revisions.last().ok_or_else(|| {
            Error::config(
                "agent_tool_experience_head_binding",
                "current commitment is missing",
            )
        })?;
        Ok(Self {
            owner_ref: head.owner_ref.clone(),
            current_revision: head.current_revision,
            head_key: head.physical_key.clone(),
            head_digest: head.content_digest.clone(),
            material_key: current.material_key.clone(),
            material_digest: current.content_digest.clone(),
        })
    }

    pub fn from_head_and_material(
        head: &AgentToolExperienceOwnerHeadV3,
        material: &AgentToolExperienceRevisionMaterialV3,
    ) -> Result<Self> {
        if !head.validate_contract().accepted
            || head.state != AgentToolExperienceHeadStateV3::Active
            || !material.validate_contract().accepted
            || head.owner_ref != material.owner_ref
            || head.current_revision != material.owner_revision
            || head.memory_space_id != material.memory_space_id
            || head.owning_scope != material.owning_scope
        {
            return Err(Error::config(
                "agent_tool_experience_head_binding",
                "head and current material must close the same exact owner revision",
            ));
        }
        Ok(Self {
            owner_ref: head.owner_ref.clone(),
            current_revision: head.current_revision,
            head_key: head.physical_key.clone(),
            head_digest: head.content_digest.clone(),
            material_key: material.physical_key.clone(),
            material_digest: material.content_digest.clone(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolExperienceScopeManifestV1 {
    pub schema_version: u32,
    pub physical_key: String,
    pub revision: u64,
    pub memory_space_id: String,
    pub owning_scope: AgentToolExperienceOwningScopeV1,
    pub owner_count: usize,
    pub bindings: Vec<AgentToolExperienceHeadBindingV1>,
    pub bindings_digest: String,
}

impl AgentToolExperienceScopeManifestV1 {
    pub fn build(
        revision: u64,
        memory_space_id: &str,
        owning_scope: AgentToolExperienceOwningScopeV1,
        mut bindings: Vec<AgentToolExperienceHeadBindingV1>,
        max_entries: usize,
    ) -> Result<Self> {
        validate_scope(memory_space_id, &owning_scope)?;
        bindings.sort();
        if revision == 0
            || max_entries == 0
            || bindings.len() > max_entries
            || bindings.iter().any(|value| {
                value.owner_ref.owner_plane != GovernedMemoryOwnerPlane::AgentToolExperience
                    || !is_owner_id(&value.owner_ref.owner_id)
                    || value.current_revision == 0
                    || !is_digest(&value.head_digest)
                    || !is_digest(&value.material_digest)
            })
            || bindings.windows(2).any(|pair| {
                pair[0].owner_ref == pair[1].owner_ref
                    || pair[0].head_key == pair[1].head_key
                    || pair[0].material_key == pair[1].material_key
            })
        {
            return Err(Error::config(
                "agent_tool_experience_scope_manifest",
                "manifest bindings are invalid, duplicate, or exceed capacity",
            ));
        }
        let physical_key =
            agent_tool_experience_scope_manifest_key(memory_space_id, &owning_scope)?;
        let encoded = serde_json::to_vec(&(revision, memory_space_id, &owning_scope, &bindings))
            .map_err(|error| {
                Error::config("agent_tool_experience_manifest_digest", error.to_string())
            })?;
        Ok(Self {
            schema_version: AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_SCHEMA_VERSION,
            physical_key,
            revision,
            memory_space_id: memory_space_id.to_string(),
            owning_scope,
            owner_count: bindings.len(),
            bindings,
            bindings_digest: domain_digest(MANIFEST_DIGEST_DOMAIN, &[&encoded]),
        })
    }

    pub fn validate_exact(
        &self,
        bindings: Vec<AgentToolExperienceHeadBindingV1>,
        max_entries: usize,
    ) -> Result<()> {
        let expected = Self::build(
            self.revision,
            &self.memory_space_id,
            self.owning_scope.clone(),
            bindings,
            max_entries,
        )?;
        if *self == expected && self.owner_count == self.bindings.len() {
            Ok(())
        } else {
            Err(Error::config(
                "agent_tool_experience_scope_manifest",
                "manifest differs from the exact owner closure",
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolExperienceContractFailure {
    SchemaMismatch,
    ScopeInvalid,
    OwnerIdentityInvalid,
    RevisionInvalid,
    PhysicalKeyMismatch,
    ContentInvalid,
    DigestMismatch,
    HeadInvalid,
    HistoryInvalid,
    ManifestInvalid,
    ClosureIncomplete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolExperienceContractValidation {
    pub accepted: bool,
    pub failures: Vec<AgentToolExperienceContractFailure>,
}

pub fn canonical_agent_tool_experience_owner_id(
    memory_space_id: &str,
    owning_scope: &AgentToolExperienceOwningScopeV1,
    registry_id: &str,
    registry_scope: &AgentToolRegistryScope,
    tool_id: &str,
    schema_fingerprint: &str,
    focus: &AgentToolExperienceFocusV1<'_>,
) -> Result<String> {
    validate_identity_fields(
        memory_space_id,
        owning_scope,
        registry_id,
        registry_scope,
        tool_id,
        schema_fingerprint,
        focus,
    )?;
    let registry_scope = serde_json::to_vec(registry_scope)
        .map_err(|error| Error::config("agent_tool_experience_owner_id", error.to_string()))?;
    let focus = serde_json::to_vec(focus)
        .map_err(|_| Error::config("agent_tool_experience_owner_id", "focus encoding failed"))?;
    Ok(format!(
        "agent_tool_experience:sha256:{}",
        domain_hex(
            OWNER_ID_DOMAIN,
            &[
                memory_space_id.as_bytes(),
                owning_scope.mounted_subject_id().as_bytes(),
                registry_id.as_bytes(),
                &registry_scope,
                tool_id.as_bytes(),
                schema_fingerprint.as_bytes(),
                &focus,
            ],
        )
    ))
}

pub fn agent_tool_experience_material_key(
    memory_space_id: &str,
    owning_scope: &AgentToolExperienceOwningScopeV1,
    owner_ref: &GovernedMemoryOwnerRef,
    revision: u64,
) -> Result<String> {
    validate_owner_key_input(memory_space_id, owning_scope, owner_ref, revision)?;
    Ok(format!(
        "agent_tool_experience_material:sha256:{}",
        domain_hex(
            MATERIAL_KEY_DOMAIN,
            &[
                memory_space_id.as_bytes(),
                owning_scope.mounted_subject_id().as_bytes(),
                owner_ref.owner_id.as_bytes(),
                &revision.to_be_bytes(),
            ],
        )
    ))
}

pub fn agent_tool_experience_head_key(
    memory_space_id: &str,
    owning_scope: &AgentToolExperienceOwningScopeV1,
    owner_ref: &GovernedMemoryOwnerRef,
) -> Result<String> {
    validate_owner_key_input(memory_space_id, owning_scope, owner_ref, 1)?;
    Ok(format!(
        "agent_tool_experience_head:sha256:{}",
        domain_hex(
            HEAD_KEY_DOMAIN,
            &[
                memory_space_id.as_bytes(),
                owning_scope.mounted_subject_id().as_bytes(),
                owner_ref.owner_id.as_bytes(),
            ],
        )
    ))
}

pub fn agent_tool_experience_scope_manifest_key(
    memory_space_id: &str,
    owning_scope: &AgentToolExperienceOwningScopeV1,
) -> Result<String> {
    validate_scope(memory_space_id, owning_scope)?;
    Ok(format!(
        "agent_tool_experience_scope:sha256:{}",
        domain_hex(
            MANIFEST_KEY_DOMAIN,
            &[
                memory_space_id.as_bytes(),
                owning_scope.mounted_subject_id().as_bytes(),
            ],
        )
    ))
}

pub fn validate_agent_tool_experience_owner_history(
    materials: &[AgentToolExperienceRevisionMaterialV3],
) -> Result<()> {
    let Some(first) = materials.first() else {
        return Err(Error::config(
            "agent_tool_experience_owner_history",
            "owner history must not be empty",
        ));
    };
    if materials
        .iter()
        .any(|value| !value.validate_contract().accepted)
        || materials.iter().enumerate().any(|(index, value)| {
            value.owner_ref != first.owner_ref
                || value.memory_space_id != first.memory_space_id
                || value.owning_scope != first.owning_scope
                || value.owner_revision != index as u64 + 1
                || value.created_at != first.created_at
                || (index > 0 && value.updated_at < materials[index - 1].updated_at)
                || (index == 0 && value.predecessor.is_some())
                || (index > 0
                    && value.predecessor.as_ref()
                        != AgentToolExperienceRetainedRevisionDigestV3::from_material(
                            &materials[index - 1],
                        )
                        .ok()
                        .as_ref())
        })
    {
        return Err(Error::config(
            "agent_tool_experience_owner_history",
            "owner history is non-canonical, non-contiguous, or crosses owner scope",
        ));
    }
    Ok(())
}

pub fn validate_agent_tool_experience_scope_closure(
    manifest: &AgentToolExperienceScopeManifestV1,
    heads: &[AgentToolExperienceOwnerHeadV3],
    materials: &[AgentToolExperienceRevisionMaterialV3],
    max_entries: usize,
) -> Result<()> {
    let head_by_owner = heads
        .iter()
        .map(|head| (head.owner_ref.clone(), head))
        .collect::<BTreeMap<_, _>>();
    let material_by_owner_revision = materials
        .iter()
        .map(|material| {
            (
                (material.owner_ref.clone(), material.owner_revision),
                material,
            )
        })
        .collect::<BTreeMap<_, _>>();
    if head_by_owner.len() != heads.len()
        || material_by_owner_revision.len() != materials.len()
        || heads.iter().any(|head| {
            !head.validate_contract().accepted
                || head.memory_space_id != manifest.memory_space_id
                || head.owning_scope != manifest.owning_scope
        })
        || materials.iter().any(|material| {
            !material.validate_contract().accepted
                || material.memory_space_id != manifest.memory_space_id
                || material.owning_scope != manifest.owning_scope
        })
    {
        return Err(Error::config(
            "agent_tool_experience_scope_closure",
            "closure contains duplicate, invalid, or cross-scope owners",
        ));
    }
    let bindings = heads
        .iter()
        .map(|head| {
            if head.state == AgentToolExperienceHeadStateV3::Tombstoned {
                if materials
                    .iter()
                    .any(|material| material.owner_ref == head.owner_ref)
                {
                    return Err(Error::config(
                        "agent_tool_experience_scope_closure",
                        "tombstoned owner must not retain raw revision material",
                    ));
                }
                return AgentToolExperienceHeadBindingV1::from_head(head);
            }
            let material = material_by_owner_revision
                .get(&(head.owner_ref.clone(), head.current_revision))
                .ok_or_else(|| {
                    Error::config(
                        "agent_tool_experience_scope_closure",
                        "head current material is missing",
                    )
                })?;
            if head.retained_revisions.iter().any(|retained| {
                material_by_owner_revision
                    .get(&(head.owner_ref.clone(), retained.owner_revision))
                    .is_none_or(|material| {
                        material.physical_key != retained.material_key
                            || material.content_digest != retained.content_digest
                    })
            }) {
                return Err(Error::config(
                    "agent_tool_experience_scope_closure",
                    "retained revision material is missing or digest-mismatched",
                ));
            }
            AgentToolExperienceHeadBindingV1::from_head_and_material(head, material)
        })
        .collect::<Result<Vec<_>>>()?;
    let bound_owners = bindings
        .iter()
        .map(|value| value.owner_ref.clone())
        .collect::<BTreeSet<_>>();
    if bound_owners.len() != heads.len()
        || materials
            .iter()
            .any(|value| !bound_owners.contains(&value.owner_ref))
    {
        return Err(Error::config(
            "agent_tool_experience_scope_closure",
            "closure contains an orphan owner or material",
        ));
    }
    manifest.validate_exact(bindings, max_entries)
}

fn validate_identity_fields(
    memory_space_id: &str,
    owning_scope: &AgentToolExperienceOwningScopeV1,
    registry_id: &str,
    registry_scope: &AgentToolRegistryScope,
    tool_id: &str,
    schema_fingerprint: &str,
    focus: &AgentToolExperienceFocusV1<'_>,
) -> Result<()> {
    validate_scope(memory_space_id, owning_scope)?;
    if !registry_scope.validate_contract()
        || !is_canonical(registry_id)
        || !is_canonical(tool_id)
        || !is_canonical(schema_fingerprint)
        || matches!(focus, AgentToolExperienceFocusV1::Method { task_signature } if !bounded_identifier(task_signature))
    {
        return Err(Error::config(
            "agent_tool_experience_identity",
            "registry, tool, schema, task, or registry scope is non-canonical",
        ));
    }
    Ok(())
}

fn validate_scope(
    memory_space_id: &str,
    owning_scope: &AgentToolExperienceOwningScopeV1,
) -> Result<()> {
    if !is_canonical(memory_space_id) || !owning_scope.validate_contract() {
        return Err(Error::config(
            "agent_tool_experience_scope",
            "memory space and mounted subject must be canonical",
        ));
    }
    Ok(())
}

fn validate_owner_key_input(
    memory_space_id: &str,
    owning_scope: &AgentToolExperienceOwningScopeV1,
    owner_ref: &GovernedMemoryOwnerRef,
    revision: u64,
) -> Result<()> {
    validate_scope(memory_space_id, owning_scope)?;
    if owner_ref.owner_plane != GovernedMemoryOwnerPlane::AgentToolExperience
        || !is_owner_id(&owner_ref.owner_id)
        || revision == 0
    {
        return Err(Error::config(
            "agent_tool_experience_owner_key",
            "owner plane, owner id, and revision must be canonical",
        ));
    }
    Ok(())
}

fn validation(
    mut failures: Vec<AgentToolExperienceContractFailure>,
) -> AgentToolExperienceContractValidation {
    failures.sort();
    failures.dedup();
    AgentToolExperienceContractValidation {
        accepted: failures.is_empty(),
        failures,
    }
}

fn is_canonical(value: &str) -> bool {
    !value.is_empty() && value == value.trim() && !value.chars().any(char::is_control)
}

fn is_canonical_text(value: &str) -> bool {
    crate::util::is_canonical_evidence_text(value)
}

fn canonical_unique(values: &[String]) -> bool {
    values.iter().all(|value| is_canonical(value))
        && values.windows(2).all(|pair| pair[0] < pair[1])
}

fn is_owner_id(value: &str) -> bool {
    prefixed_hex(value, "agent_tool_experience:sha256:")
}

fn is_digest(value: &str) -> bool {
    prefixed_hex(value, "sha256:")
}

fn prefixed_hex(value: &str, prefix: &str) -> bool {
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn material() -> AgentToolExperienceRevisionMaterialV3 {
        material_with_guidance("1. Read the report.\n2. Verify the governed result.")
    }

    fn material_with_guidance(guidance: &str) -> AgentToolExperienceRevisionMaterialV3 {
        let mut body = AgentToolExperienceBodyV1::Method {
            task_signature: "read-report".into(),
            procedure: guidance.into(),
            constraints: vec![],
            sources: vec![],
        };
        let method_digest = body.method_content_digest().unwrap().unwrap();
        let AgentToolExperienceBodyV1::Method { sources, .. } = &mut body else {
            unreachable!()
        };
        sources.push(AgentToolMethodSourceRefV1 {
            source_job_id: "synthetic-source".into(),
            method_id: "method-a".into(),
            method_digest,
            execution_refs: vec![],
        });
        AgentToolExperienceRevisionMaterialV3::build(
            "space-a",
            AgentToolExperienceOwningScopeV1::Subject {
                mounted_subject_id: "agent-a".to_string(),
            },
            "tools",
            AgentToolRegistryScope::Global,
            "read",
            "schema-v1",
            1,
            body,
            AgentToolExperienceStatus::Active,
            MemoryPrivacyClass::SharedWithSubject,
            100,
            101,
            None,
        )
        .expect("material")
    }

    #[test]
    fn canonical_multiline_method_material_builds_and_retains_exact_digest() {
        let method = "1. Inspect archive inputs.\n2. Extract into an empty directory.\n3. Verify the manifest.";
        let multiline = material_with_guidance(method);
        assert!(
            matches!(&multiline.body, AgentToolExperienceBodyV1::Method { procedure, .. } if procedure == method)
        );
        assert!(multiline.validate_contract().accepted);
        assert_eq!(
            multiline.content_digest,
            multiline
                .canonical_content_digest()
                .expect("canonical digest")
        );
        let mut flattened = multiline.clone();
        let AgentToolExperienceBodyV1::Method { procedure, .. } = &mut flattened.body else {
            unreachable!()
        };
        *procedure = method.replace('\n', " ");
        assert_ne!(
            multiline.content_digest,
            flattened.canonical_content_digest().unwrap()
        );
        assert!(!flattened.validate_contract().accepted);
    }

    #[test]
    fn tombstone_preserves_commitments_but_requires_raw_material_exact_zero() {
        let material = material();
        let head = AgentToolExperienceOwnerHeadV3::build(
            &material.memory_space_id,
            material.owning_scope.clone(),
            material.owner_ref.clone(),
            1,
            vec![
                AgentToolExperienceRetainedRevisionDigestV3::from_material(&material)
                    .expect("retained"),
            ],
        )
        .expect("head");
        let active_manifest = AgentToolExperienceScopeManifestV1::build(
            1,
            &head.memory_space_id,
            head.owning_scope.clone(),
            vec![AgentToolExperienceHeadBindingV1::from_head(&head).expect("binding")],
            8,
        )
        .expect("manifest");
        validate_agent_tool_experience_scope_closure(
            &active_manifest,
            std::slice::from_ref(&head),
            std::slice::from_ref(&material),
            8,
        )
        .expect("positive active closure");
        assert!(validate_agent_tool_experience_scope_closure(
            &active_manifest,
            std::slice::from_ref(&head),
            &[],
            8
        )
        .is_err());
        let terminal = head.tombstone().expect("terminal");
        assert_eq!(terminal.retained_revisions, head.retained_revisions);
        assert_eq!(terminal.tombstone().expect("idempotent"), terminal);
        let terminal_manifest = AgentToolExperienceScopeManifestV1::build(
            2,
            &head.memory_space_id,
            head.owning_scope.clone(),
            vec![AgentToolExperienceHeadBindingV1::from_head(&terminal).expect("binding")],
            8,
        )
        .expect("terminal manifest");
        validate_agent_tool_experience_scope_closure(
            &terminal_manifest,
            std::slice::from_ref(&terminal),
            &[],
            8,
        )
        .expect("terminal commitment closure");
        assert!(validate_agent_tool_experience_scope_closure(
            &terminal_manifest,
            &[terminal],
            &[material],
            8
        )
        .is_err());
    }

    #[test]
    fn revision_history_rejects_time_reversal_and_changed_creation_time() {
        let first = material();
        let mut second = first.clone();
        second.owner_revision = 2;
        second.physical_key = agent_tool_experience_material_key(
            &first.memory_space_id,
            &first.owning_scope,
            &first.owner_ref,
            2,
        )
        .expect("key");
        second.predecessor = Some(
            AgentToolExperienceRetainedRevisionDigestV3::from_material(&first)
                .expect("predecessor"),
        );
        second.updated_at = 102;
        second.content_digest = second.canonical_content_digest().expect("digest");
        validate_agent_tool_experience_owner_history(&[first.clone(), second.clone()])
            .expect("positive monotonic history");
        second.updated_at = first.updated_at;
        second.content_digest = second.canonical_content_digest().expect("digest");
        validate_agent_tool_experience_owner_history(&[first.clone(), second.clone()])
            .expect("same-second revisions preserve revision ordering");
        second.updated_at = 100;
        second.content_digest = second.canonical_content_digest().expect("digest");
        assert!(
            validate_agent_tool_experience_owner_history(&[first.clone(), second.clone()]).is_err()
        );
        second.updated_at = 102;
        second.created_at = 99;
        second.content_digest = second.canonical_content_digest().expect("digest");
        assert!(validate_agent_tool_experience_owner_history(&[first, second]).is_err());
    }
}
