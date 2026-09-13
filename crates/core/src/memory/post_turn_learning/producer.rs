//! Durable procedural evidence authority, not an account database. The SDK
//! proves the caller and SubjectRegistry membership; Store proves the exact
//! immutable revision/current-head closure. Credentials never enter this owner.

use serde::{Deserialize, Serialize};

use super::evidence::identifier;
use super::{
    canonical_order, domain_digest, is_digest, AgentToolUsageFeedbackV3,
    ProceduralSourceSensitivity,
};
use crate::skills::AgentToolRegistryRef;

pub const PROCEDURAL_PRODUCER_BINDING_SCHEMA_VERSION: u32 = 1;
pub const MAX_PROCEDURAL_PRODUCER_TOOLS: usize = 256;
pub const MAX_PROCEDURAL_PRODUCERS_PER_SCOPE: usize = 256;
pub const MAX_PROCEDURAL_PRODUCER_REVISIONS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralProducerScopeV1 {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub channel_id: String,
    pub chat_id: String,
}

impl ProceduralProducerScopeV1 {
    pub fn validate_contract(&self) -> bool {
        [
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.channel_id,
            &self.chat_id,
        ]
        .into_iter()
        .all(|value| identifier(value))
    }

    pub fn scope_index_key(&self) -> crate::Result<String> {
        if !self.validate_contract() {
            return Err(crate::Error::invalid_input(
                "procedural_producer_scope",
                "producer scope must be canonical",
            ));
        }
        Ok(super::procedural_scope_id(self))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralProducerPrincipalV1 {
    EntryPrincipal {
        principal_id: String,
        owner_id: String,
    },
    LocalCapability {
        capability_id: String,
    },
    OfficialGovernance {
        service_id: String,
    },
}

impl ProceduralProducerPrincipalV1 {
    pub fn validate_contract(&self) -> bool {
        match self {
            Self::EntryPrincipal {
                principal_id,
                owner_id,
            } => identifier(principal_id) && identifier(owner_id),
            Self::LocalCapability { capability_id } => identifier(capability_id),
            Self::OfficialGovernance { service_id } => identifier(service_id),
        }
    }
}

/// Authorship of a declaration, never permission to admit its method. A Human
/// declaration is not by itself execution evidence or a method-quality verdict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralProducerSourceAuthorityV1 {
    RuntimeObservation,
    HumanUser {
        subject_id: String,
    },
    ModelInferred {
        subject_id: String,
    },
    GovernedSource {
        source_revision: crate::memory::GovernedOwnerRevisionRef,
    },
}

impl ProceduralProducerSourceAuthorityV1 {
    /// This is declaration provenance, never execution attestation or method
    /// activation. Private, external, diagnostic and ambiguous origins cannot
    /// be laundered through an otherwise projection-visible owner.
    pub fn permits_governed_method_source(
        authority: crate::memory::MemoryEvidenceAuthority,
        privacy: crate::memory::MemoryPrivacyClass,
        external_content: bool,
    ) -> bool {
        use crate::memory::MemoryEvidenceAuthority as Authority;
        !external_content
            && privacy.projection_content_allowed()
            && matches!(
                authority,
                Authority::UserAsserted
                    | Authority::ModelInferred
                    | Authority::RuntimeObservation
                    | Authority::WorldObservation
                    | Authority::ProgramMemoryCanonical
            )
    }
    pub fn can_witness_execution(&self) -> bool {
        matches!(self, Self::RuntimeObservation | Self::HumanUser { .. })
    }
    pub fn validate_contract(&self) -> bool {
        match self {
            Self::RuntimeObservation => true,
            Self::HumanUser { subject_id } | Self::ModelInferred { subject_id } => {
                identifier(subject_id)
            }
            Self::GovernedSource { source_revision } => {
                source_revision.is_valid()
                    && identifier(&source_revision.owner_ref.owner_id)
                    && matches!(
                        source_revision.owner_ref.owner_plane,
                        crate::memory::GovernedMemoryOwnerPlane::LongTerm
                            | crate::memory::GovernedMemoryOwnerPlane::EvidenceDocument
                    )
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralProducerClaimsV1 {
    pub execution_facts: bool,
    pub method_declarations: bool,
    pub usage_feedback: bool,
    pub source_classifications: Vec<ProceduralSourceSensitivity>,
}

impl ProceduralProducerClaimsV1 {
    pub fn validate_contract(&self) -> bool {
        (self.execution_facts || self.method_declarations || self.usage_feedback)
            && self.source_classifications.len() <= 3
            && self
                .source_classifications
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            && (self.source_classifications.is_empty()
                != (self.execution_facts || self.method_declarations))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralProducerToolV1 {
    pub registry_ref: AgentToolRegistryRef,
    pub tool_id: String,
    pub schema_fingerprint: String,
}

impl ProceduralProducerToolV1 {
    pub fn validate_contract(&self) -> bool {
        identifier(&self.registry_ref.registry_id)
            && identifier(&self.registry_ref.fingerprint)
            && self.registry_ref.scope.validate_contract()
            && identifier(&self.tool_id)
            && identifier(&self.schema_fingerprint)
    }

    pub fn matches_feedback(&self, feedback: &AgentToolUsageFeedbackV3) -> bool {
        self.registry_ref == feedback.registry_ref
            && self.tool_id == feedback.tool_id
            && self.schema_fingerprint == feedback.schema_fingerprint
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralProducerSpecV1 {
    pub binding_id: String,
    pub scope: ProceduralProducerScopeV1,
    pub principal: ProceduralProducerPrincipalV1,
    pub source_authority: ProceduralProducerSourceAuthorityV1,
    pub claims: ProceduralProducerClaimsV1,
    pub tools: Vec<ProceduralProducerToolV1>,
    pub source_config_ref: String,
}

impl ProceduralProducerSpecV1 {
    pub fn validate_contract(&self) -> bool {
        identifier(&self.binding_id)
            && self.scope.validate_contract()
            && self.principal.validate_contract()
            && self.source_authority.validate_contract()
            && self.claims.validate_contract()
            && identifier(&self.source_config_ref)
            && self.tools.len() <= MAX_PROCEDURAL_PRODUCER_TOOLS
            && self
                .tools
                .iter()
                .all(ProceduralProducerToolV1::validate_contract)
            && canonical_order(&self.tools)
            && (self.tools.is_empty()
                != (self.claims.execution_facts || self.claims.method_declarations))
            && (!(self.claims.execution_facts || self.claims.usage_feedback)
                || self.source_authority.can_witness_execution())
    }

    pub fn binding_key(&self) -> crate::Result<String> {
        let encoded = serde_json::to_vec(&(&self.scope, &self.binding_id)).map_err(|_| {
            crate::Error::config("procedural_producer", "binding key encoding failed")
        })?;
        Ok(domain_digest(
            "procedural_producer_binding_key_v1",
            &[&encoded],
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralProducerStateV1 {
    Active,
    Revoked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralProducerErrorV1 {
    NonCanonical,
    AuthorityRequired,
    AuthorityMismatch,
    ScopeMismatch,
    ClaimForbidden,
    ToolForbidden,
    SourceClassificationForbidden,
    RevisionConflict,
    Revoked,
    RepairRequired,
    CapacityBlocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralProducerRevisionRefV1 {
    pub binding_key: String,
    pub revision: u64,
    pub content_digest: String,
}

impl ProceduralProducerRevisionRefV1 {
    pub fn validate_contract(&self) -> bool {
        is_digest(&self.binding_key) && self.revision > 0 && is_digest(&self.content_digest)
    }

    pub fn material_key(&self) -> String {
        format!("{}:{}", self.binding_key, self.revision)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralProducerBindingV1 {
    pub schema_version: u32,
    pub spec: ProceduralProducerSpecV1,
    pub revision: u64,
    pub state: ProceduralProducerStateV1,
    pub operation_identity: crate::memory::MemoryMutationOperationIdentity,
    pub created_at: u64,
    pub updated_at: u64,
    pub predecessor: Option<ProceduralProducerRevisionRefV1>,
    pub content_digest: String,
}

impl ProceduralProducerBindingV1 {
    /// A historical declaration never grants current use. Store supplies the
    /// exact retained material and current head material from one fenced image.
    pub fn authorize_evidence_with_current(
        &self,
        current: &Self,
        scope: &ProceduralProducerScopeV1,
        evidence: &super::PostTurnLearningEvidenceV2,
    ) -> std::result::Result<(), ProceduralProducerErrorV1> {
        use ProceduralProducerErrorV1 as E;
        if !self.validate_contract()
            || !current.validate_contract()
            || !evidence.validate_contract()
        {
            return Err(E::RepairRequired);
        }
        let super::ProceduralFeedbackAuthorityV2::Producer {
            producer_revision,
            source_authority,
            ..
        } = &evidence.authority
        else {
            return Err(E::AuthorityRequired);
        };
        if !self
            .revision_ref()
            .is_ok_and(|reference| &reference == producer_revision)
            || source_authority != &self.spec.source_authority
            || !current.spec.binding_key().is_ok_and(|key| {
                self.spec
                    .binding_key()
                    .is_ok_and(|historical| key == historical)
            })
            || current.spec.principal != self.spec.principal
            || current.spec.source_authority != self.spec.source_authority
            || current.revision < self.revision
        {
            return Err(E::AuthorityMismatch);
        }
        if &self.spec.scope != scope
            || &current.spec.scope != scope
            || evidence.memory_space_id != scope.memory_space_id
            || evidence.mounted_subject_id != scope.mounted_subject_id
        {
            return Err(E::ScopeMismatch);
        }
        for binding in [self, current] {
            if binding.state != ProceduralProducerStateV1::Active {
                return Err(E::Revoked);
            }
            if (!evidence.runtime_skill_feedback.is_empty()
                || !evidence.agent_skill_feedback.is_empty()
                || !evidence.task_learning_feedback.is_empty()
                || evidence.selection_receipt.is_some())
                && !binding.spec.claims.usage_feedback
            {
                return Err(E::ClaimForbidden);
            }
            for feedback in &evidence.agent_tool_feedback {
                if feedback.submitted_method_count > 0 && !binding.spec.claims.method_declarations {
                    return Err(E::ClaimForbidden);
                }
                // Reuse the same declaration authority owner after privacy has
                // removed rejected bodies; this view is never persisted.
                binding.authorize_tool_feedback(
                    &self.spec.principal,
                    scope,
                    &AgentToolUsageFeedbackV3 {
                        registry_ref: feedback.registry_ref.clone(),
                        tool_id: feedback.tool_id.clone(),
                        schema_fingerprint: feedback.schema_fingerprint.clone(),
                        execution_facts: feedback.execution_facts.clone(),
                        method_evidence: feedback.method_evidence.clone(),
                    },
                )?;
            }
        }
        Ok(())
    }

    pub fn build(
        spec: ProceduralProducerSpecV1,
        state: ProceduralProducerStateV1,
        operation_identity: crate::memory::MemoryMutationOperationIdentity,
        now_secs: u64,
        previous: Option<&Self>,
    ) -> std::result::Result<Self, ProceduralProducerErrorV1> {
        use ProceduralProducerErrorV1 as E;
        if !spec.validate_contract()
            || operation_identity.validate_contract().is_err()
            || now_secs == 0
            || operation_identity.operation_kind()
                != crate::memory::MemoryMutationOperationKind::ProceduralProducerControl
            || operation_identity.memory_space_id() != spec.scope.memory_space_id
            || operation_identity.mounted_subject_id() != spec.scope.mounted_subject_id
        {
            return Err(E::NonCanonical);
        }
        if let Some(prior) = previous {
            if !prior.validate_contract() {
                return Err(E::RepairRequired);
            }
            if prior.state == ProceduralProducerStateV1::Revoked {
                return Err(E::Revoked);
            }
            if prior.spec.binding_id != spec.binding_id
                || prior.spec.scope != spec.scope
                || prior.spec.principal != spec.principal
                || prior.spec.source_authority != spec.source_authority
                || now_secs < prior.updated_at
                || prior.operation_identity == operation_identity
            {
                return Err(E::RevisionConflict);
            }
        } else if state != ProceduralProducerStateV1::Active {
            return Err(E::RevisionConflict);
        }
        let mut binding = Self {
            schema_version: PROCEDURAL_PRODUCER_BINDING_SCHEMA_VERSION,
            spec,
            revision: match previous {
                Some(p) => p.revision.checked_add(1).ok_or(E::CapacityBlocked)?,
                None => 1,
            },
            state,
            operation_identity,
            created_at: previous.map_or(now_secs, |p| p.created_at),
            updated_at: now_secs,
            predecessor: previous
                .map(Self::revision_ref)
                .transpose()
                .map_err(|_| E::RepairRequired)?,
            content_digest: String::new(),
        };
        binding.content_digest = binding.canonical_digest().map_err(|_| E::NonCanonical)?;
        Ok(binding)
    }

    pub fn canonical_digest(&self) -> crate::Result<String> {
        let encoded = serde_json::to_vec(&(
            self.schema_version,
            &self.spec,
            self.revision,
            self.state,
            &self.operation_identity,
            self.created_at,
            self.updated_at,
            &self.predecessor,
        ))
        .map_err(|_| {
            crate::Error::config("procedural_producer", "binding digest encoding failed")
        })?;
        Ok(domain_digest(
            "procedural_producer_binding_digest_v1",
            &[&encoded],
        ))
    }

    pub fn revision_ref(&self) -> crate::Result<ProceduralProducerRevisionRefV1> {
        Ok(ProceduralProducerRevisionRefV1 {
            binding_key: self.spec.binding_key()?,
            revision: self.revision,
            content_digest: self.content_digest.clone(),
        })
    }

    pub fn control_intent_digest(&self) -> crate::Result<String> {
        procedural_producer_control_intent_digest(&self.spec, self.state, self.predecessor.as_ref())
    }

    pub fn validate_contract(&self) -> bool {
        self.schema_version == PROCEDURAL_PRODUCER_BINDING_SCHEMA_VERSION
            && self.spec.validate_contract()
            && self.operation_identity.validate_contract().is_ok()
            && self.operation_identity.operation_kind()
                == crate::memory::MemoryMutationOperationKind::ProceduralProducerControl
            && self.operation_identity.memory_space_id() == self.spec.scope.memory_space_id
            && self.operation_identity.mounted_subject_id() == self.spec.scope.mounted_subject_id
            && self.revision > 0
            && self.created_at > 0
            && self.updated_at >= self.created_at
            && match &self.predecessor {
                None => self.revision == 1 && self.state == ProceduralProducerStateV1::Active,
                Some(p) => {
                    p.validate_contract()
                        && p.revision.checked_add(1) == Some(self.revision)
                        && self
                            .spec
                            .binding_key()
                            .is_ok_and(|key| key == p.binding_key)
                }
            }
            && self
                .canonical_digest()
                .is_ok_and(|digest| digest == self.content_digest)
    }

    /// Both intake and later consumption use this same declaration check. The
    /// SDK must additionally fence the current revision in the commit read-set.
    pub fn authorize_tool_feedback(
        &self,
        principal: &ProceduralProducerPrincipalV1,
        scope: &ProceduralProducerScopeV1,
        feedback: &AgentToolUsageFeedbackV3,
    ) -> std::result::Result<(), ProceduralProducerErrorV1> {
        use ProceduralProducerErrorV1 as E;
        if !self.validate_contract() {
            return Err(E::RepairRequired);
        }
        if self.state != ProceduralProducerStateV1::Active {
            return Err(E::Revoked);
        }
        if self.spec.principal != *principal {
            return Err(E::AuthorityMismatch);
        }
        if self.spec.scope != *scope {
            return Err(E::ScopeMismatch);
        }
        if (!feedback.execution_facts.is_empty() && !self.spec.claims.execution_facts)
            || (!feedback.method_evidence.is_empty() && !self.spec.claims.method_declarations)
        {
            return Err(E::ClaimForbidden);
        }
        if !self
            .spec
            .tools
            .iter()
            .any(|tool| tool.matches_feedback(feedback))
        {
            return Err(E::ToolForbidden);
        }
        if feedback
            .execution_facts
            .iter()
            .map(|fact| fact.source_sensitivity)
            .chain(
                feedback
                    .method_evidence
                    .iter()
                    .map(|method| method.source_sensitivity),
            )
            .any(|sensitivity| {
                !self
                    .spec
                    .claims
                    .source_classifications
                    .contains(&sensitivity)
            })
        {
            return Err(E::SourceClassificationForbidden);
        }
        Ok(())
    }
}

/// Stable across retries: commit time and the resulting receipt are not intent.
pub fn procedural_producer_control_intent_digest(
    spec: &ProceduralProducerSpecV1,
    state: ProceduralProducerStateV1,
    expected: Option<&ProceduralProducerRevisionRefV1>,
) -> crate::Result<String> {
    if !spec.validate_contract()
        || expected.is_some_and(|value| {
            !value.validate_contract()
                || !spec.binding_key().is_ok_and(|key| key == value.binding_key)
        })
    {
        return Err(crate::Error::invalid_input(
            "procedural_producer_intent",
            "producer control intent is invalid",
        ));
    }
    let encoded = serde_json::to_vec(&(spec, state, expected)).map_err(|_| {
        crate::Error::config("procedural_producer_intent", "intent encoding failed")
    })?;
    Ok(domain_digest(
        "procedural_producer_control_intent_v1",
        &[&encoded],
    ))
}

/// The current root also retains the immutable revision inventory. Referenced
/// history cannot be silently evicted by the unrelated recent-terminal job list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralProducerHeadV1 {
    pub schema_version: u32,
    pub scope: ProceduralProducerScopeV1,
    pub binding_key: String,
    pub current: ProceduralProducerRevisionRefV1,
    pub retained_revisions: Vec<ProceduralProducerRevisionRefV1>,
    pub content_digest: String,
}

impl ProceduralProducerHeadV1 {
    pub fn advance(
        binding: &ProceduralProducerBindingV1,
        previous: Option<&Self>,
    ) -> std::result::Result<Self, ProceduralProducerErrorV1> {
        use ProceduralProducerErrorV1 as E;
        if !binding.validate_contract() {
            return Err(E::NonCanonical);
        }
        let current = binding.revision_ref().map_err(|_| E::NonCanonical)?;
        let mut retained_revisions = match previous {
            Some(head) => {
                if !head.validate_contract() {
                    return Err(E::RepairRequired);
                }
                if head.scope != binding.spec.scope
                    || head.binding_key != current.binding_key
                    || binding.predecessor.as_ref() != Some(&head.current)
                {
                    return Err(E::RevisionConflict);
                }
                head.retained_revisions.clone()
            }
            None if binding.revision == 1 && binding.predecessor.is_none() => Vec::new(),
            None => return Err(E::RepairRequired),
        };
        if retained_revisions.len() >= MAX_PROCEDURAL_PRODUCER_REVISIONS {
            return Err(E::CapacityBlocked);
        }
        retained_revisions.push(current.clone());
        let mut head = Self {
            schema_version: PROCEDURAL_PRODUCER_BINDING_SCHEMA_VERSION,
            scope: binding.spec.scope.clone(),
            binding_key: current.binding_key.clone(),
            current,
            retained_revisions,
            content_digest: String::new(),
        };
        head.content_digest = head.canonical_digest().map_err(|_| E::NonCanonical)?;
        Ok(head)
    }

    pub fn canonical_digest(&self) -> crate::Result<String> {
        let encoded = serde_json::to_vec(&(
            self.schema_version,
            &self.scope,
            &self.binding_key,
            &self.current,
            &self.retained_revisions,
        ))
        .map_err(|_| crate::Error::config("procedural_producer", "head digest encoding failed"))?;
        Ok(domain_digest(
            "procedural_producer_head_digest_v1",
            &[&encoded],
        ))
    }

    pub fn validate_contract(&self) -> bool {
        self.schema_version == PROCEDURAL_PRODUCER_BINDING_SCHEMA_VERSION
            && self.scope.validate_contract()
            && is_digest(&self.binding_key)
            && !self.retained_revisions.is_empty()
            && self.retained_revisions.len() <= MAX_PROCEDURAL_PRODUCER_REVISIONS
            && self
                .retained_revisions
                .iter()
                .enumerate()
                .all(|(index, reference)| {
                    reference.validate_contract()
                        && reference.binding_key == self.binding_key
                        && reference.revision == index as u64 + 1
                })
            && self.retained_revisions.last() == Some(&self.current)
            && self
                .canonical_digest()
                .is_ok_and(|digest| digest == self.content_digest)
    }

    pub fn validates_materials(&self, materials: &[ProceduralProducerBindingV1]) -> bool {
        self.validate_contract()
            && materials
                .iter()
                .map(|material| material.operation_identity.storage_key())
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == materials.len()
            && self.retained_revisions.len() == materials.len()
            && materials
                .iter()
                .zip(&self.retained_revisions)
                .all(|(material, reference)| {
                    material.validate_contract()
                        && material.spec.scope == self.scope
                        && material
                            .revision_ref()
                            .is_ok_and(|actual| actual == *reference)
                })
            && materials.windows(2).all(|pair| {
                ProceduralProducerBindingV1::build(
                    pair[1].spec.clone(),
                    pair[1].state,
                    pair[1].operation_identity.clone(),
                    pair[1].updated_at,
                    Some(&pair[0]),
                )
                .is_ok_and(|expected| expected == pair[1])
            })
    }
}
