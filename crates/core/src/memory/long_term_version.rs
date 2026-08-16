//! Immutable long-term owner version and head-manifest contracts.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

use super::scoped_long_term_control_storage_key;
use super::{
    build_memory_update_lineage_report, CanonicalEntityRef, GovernedContractFailure,
    GovernedContractValidation, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    GovernedOwnerRevisionRef, GovernedOwnerTermination, GovernedOwnerValidity,
    GovernedUpdateLineageItem, LongTermControlOperation, LongTermMemoryConfidence,
    LongTermMemoryControlRevision, LongTermMemoryEntry, LongTermMemoryFreshness,
    LongTermMemoryKind, LongTermMemorySourceScope, LongTermMemorySourceType,
    LongTermMemoryStaleHint, MemoryPrivacyClass, MemorySpaceId, MemorySubjectVisibilityPolicy,
    MemoryUpdateLineageReport, LONG_TERM_CONTROL_REVISION_NAMESPACE,
};

pub const LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION: u32 = 3;
const LONG_TERM_VERSION_MATERIAL_KEY_DOMAIN: &str = "long_term_version_material_key_v3";
const LONG_TERM_VERSION_HEAD_KEY_DOMAIN: &str = "long_term_version_head_key_v3";
const LONG_TERM_VERSION_SCOPE_MANIFEST_KEY_DOMAIN: &str = "long_term_version_scope_manifest_key_v3";
const LONG_TERM_VERSION_CONTENT_DIGEST_DOMAIN: &str = "long_term_version_content_digest_v3";
const LONG_TERM_VERSION_HEAD_CONTENT_DIGEST_DOMAIN: &str =
    "long_term_version_head_content_digest_v3";
const LONG_TERM_VERSION_SCOPE_CLOSURE_DIGEST_DOMAIN: &str =
    "long_term_version_scope_closure_digest_v3";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LongTermVersionRetentionLease {
    max_retained_revisions_per_owner: usize,
}

impl LongTermVersionRetentionLease {
    pub fn try_new(max_retained_revisions_per_owner: usize) -> Result<Self> {
        if max_retained_revisions_per_owner == 0 {
            return Err(Error::config(
                "long_term_version_retention_lease",
                "request-pinned retention limit must be positive",
            ));
        }
        Ok(Self {
            max_retained_revisions_per_owner,
        })
    }

    pub fn max_retained_revisions_per_owner(self) -> usize {
        self.max_retained_revisions_per_owner
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryGovernedContent {
    pub kind: LongTermMemoryKind,
    pub topic: String,
    pub content: String,
    pub keywords: Vec<String>,
    pub source_chat_id: Option<String>,
    pub source_type: LongTermMemorySourceType,
    pub source_scope: LongTermMemorySourceScope,
    pub confidence: LongTermMemoryConfidence,
    pub freshness: LongTermMemoryFreshness,
    pub stale_hint: LongTermMemoryStaleHint,
    pub supporting_citations: Vec<String>,
    pub canonical_entities: Vec<CanonicalEntityRef>,
    pub evidence_count: u32,
    pub created_at: u64,
    pub updated_at: u64,
    pub observed_at: u64,
    pub last_confirmed_at: u64,
    pub source_revision: Option<u64>,
    pub last_used_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryVersionOrigin {
    pub valid_from: u64,
    pub observed_at: u64,
    pub predecessor: Option<GovernedOwnerRevisionRef>,
}

impl LongTermMemoryVersionOrigin {
    fn validate_for(&self, current: &GovernedOwnerRevisionRef) -> GovernedContractValidation {
        let mut failures = Vec::new();
        if !current.is_valid()
            || self
                .predecessor
                .as_ref()
                .is_some_and(|predecessor| !predecessor.is_valid())
        {
            failures.push(GovernedContractFailure::OwnerRevisionRefInvalid);
        }
        if self.predecessor.as_ref() == Some(current) {
            failures.push(GovernedContractFailure::ValiditySelfLoop);
        }

        match (current.owner_revision, self.predecessor.as_ref()) {
            (1, None) => {}
            (1, Some(predecessor)) => {
                if predecessor.owner_ref.owner_plane != current.owner_ref.owner_plane {
                    failures.push(GovernedContractFailure::LineageScopeMismatch);
                }
                if predecessor.owner_ref == current.owner_ref {
                    failures.push(GovernedContractFailure::LineageGap);
                }
            }
            (_, Some(predecessor))
                if predecessor.owner_ref == current.owner_ref
                    && predecessor
                        .owner_revision
                        .checked_add(1)
                        .is_some_and(|revision| revision == current.owner_revision) => {}
            _ => failures.push(GovernedContractFailure::LineageGap),
        }

        contract_validation(failures)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryVersionMaterial {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub factual_owner_id: MemorySpaceId,
    pub owner_ref: GovernedMemoryOwnerRef,
    pub owner_revision: u64,
    pub governed_content: LongTermMemoryGovernedContent,
    pub governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
    pub origin: LongTermMemoryVersionOrigin,
    pub privacy_class: MemoryPrivacyClass,
    pub subject_visibility: MemorySubjectVisibilityPolicy,
    pub content_digest: String,
}

impl LongTermMemoryVersionMaterial {
    pub fn from_current_projection(
        memory_space_id: &str,
        factual_owner_id: &str,
        entry: &LongTermMemoryEntry,
        valid_from: u64,
        predecessor: Option<GovernedOwnerRevisionRef>,
        mut governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
    ) -> Result<Self> {
        governed_evidence_refs.sort();
        governed_evidence_refs.dedup();
        let owner_ref =
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, entry.id.clone());
        let mut material = Self {
            schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
            memory_space_id: memory_space_id.to_string(),
            factual_owner_id: factual_owner_id.to_string(),
            owner_ref,
            owner_revision: entry.owner_revision,
            governed_content: LongTermMemoryGovernedContent {
                kind: entry.kind.clone(),
                topic: entry.topic.clone(),
                content: entry.content.clone(),
                keywords: entry.keywords.clone(),
                source_chat_id: entry.source_chat_id.clone(),
                source_type: entry.source_type,
                source_scope: entry.source_scope,
                confidence: entry.confidence,
                freshness: entry.freshness,
                stale_hint: entry.stale_hint,
                supporting_citations: entry.supporting_citations.clone(),
                canonical_entities: entry.canonical_entities.clone(),
                evidence_count: entry.evidence_count,
                created_at: entry.created_at,
                updated_at: entry.updated_at,
                observed_at: entry.observed_at,
                last_confirmed_at: entry.last_confirmed_at,
                source_revision: entry.source_revision,
                last_used_at: entry.last_used_at,
            },
            governed_evidence_refs,
            origin: LongTermMemoryVersionOrigin {
                valid_from,
                observed_at: entry.observed_at,
                predecessor,
            },
            privacy_class: entry.privacy,
            subject_visibility: entry.subject_visibility.clone(),
            content_digest: String::new(),
        };
        material.content_digest = material.canonical_content_digest()?;
        if material.validate_contract().accepted {
            Ok(material)
        } else {
            Err(Error::config(
                "long_term_version_material",
                "current projection cannot form a canonical immutable material",
            ))
        }
    }

    pub fn to_current_projection(&self) -> Result<LongTermMemoryEntry> {
        if !self.validate_contract().accepted {
            return Err(Error::config(
                "long_term_version_material",
                "invalid material cannot produce a current projection",
            ));
        }
        Ok(LongTermMemoryEntry {
            id: self.owner_ref.owner_id.clone(),
            kind: self.governed_content.kind.clone(),
            topic: self.governed_content.topic.clone(),
            content: self.governed_content.content.clone(),
            keywords: self.governed_content.keywords.clone(),
            privacy: self.privacy_class,
            source_chat_id: self.governed_content.source_chat_id.clone(),
            source_type: self.governed_content.source_type,
            source_scope: self.governed_content.source_scope,
            subject_visibility: self.subject_visibility.clone(),
            confidence: self.governed_content.confidence,
            freshness: self.governed_content.freshness,
            stale_hint: self.governed_content.stale_hint,
            supporting_citations: self.governed_content.supporting_citations.clone(),
            canonical_entities: self.governed_content.canonical_entities.clone(),
            evidence_count: self.governed_content.evidence_count,
            created_at: self.governed_content.created_at,
            updated_at: self.governed_content.updated_at,
            observed_at: self.governed_content.observed_at,
            last_confirmed_at: self.governed_content.last_confirmed_at,
            source_revision: self.governed_content.source_revision,
            owner_revision: self.owner_revision,
            last_used_at: self.governed_content.last_used_at,
        })
    }

    pub fn owner_revision_ref(&self) -> GovernedOwnerRevisionRef {
        GovernedOwnerRevisionRef {
            owner_ref: self.owner_ref.clone(),
            owner_revision: self.owner_revision,
        }
    }

    pub fn canonical_content_digest(&self) -> Result<String> {
        #[derive(Serialize)]
        struct DigestInput<'a> {
            schema_version: u32,
            memory_space_id: &'a str,
            factual_owner_id: &'a str,
            owner_ref: &'a GovernedMemoryOwnerRef,
            owner_revision: u64,
            governed_content: &'a LongTermMemoryGovernedContent,
            governed_evidence_refs: &'a [GovernedOwnerRevisionRef],
            origin: &'a LongTermMemoryVersionOrigin,
            privacy_class: MemoryPrivacyClass,
            subject_visibility: &'a MemorySubjectVisibilityPolicy,
        }

        let bytes = serde_json::to_vec(&DigestInput {
            schema_version: self.schema_version,
            memory_space_id: &self.memory_space_id,
            factual_owner_id: &self.factual_owner_id,
            owner_ref: &self.owner_ref,
            owner_revision: self.owner_revision,
            governed_content: &self.governed_content,
            governed_evidence_refs: &self.governed_evidence_refs,
            origin: &self.origin,
            privacy_class: self.privacy_class,
            subject_visibility: &self.subject_visibility,
        })
        .map_err(|error| Error::config("long_term_version_digest", error.to_string()))?;
        Ok(domain_separated_sha256(
            LONG_TERM_VERSION_CONTENT_DIGEST_DOMAIN,
            &[&bytes],
        ))
    }

    pub fn validate_contract(&self) -> GovernedContractValidation {
        let mut failures = Vec::new();
        let revision_ref = self.owner_revision_ref();
        if self.schema_version != LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION
            || self.memory_space_id.trim().is_empty()
            || self.memory_space_id != self.memory_space_id.trim()
            || self.factual_owner_id.trim().is_empty()
            || self.factual_owner_id != self.factual_owner_id.trim()
            || self.factual_owner_id != self.memory_space_id
            || self.owner_ref.owner_plane != GovernedMemoryOwnerPlane::LongTerm
            || !revision_ref.is_valid()
        {
            failures.push(GovernedContractFailure::OwnerRevisionRefInvalid);
        }
        failures.extend(self.origin.validate_for(&revision_ref).failures);
        if self.subject_visibility.validate_canonical().is_err() {
            failures.push(GovernedContractFailure::ContentDigestMismatch);
        }
        let evidence_refs = self
            .governed_evidence_refs
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if self.governed_evidence_refs.iter().any(|evidence| {
            !evidence.is_valid()
                || evidence.owner_ref.owner_plane != GovernedMemoryOwnerPlane::EvidenceDocument
        }) || self
            .governed_evidence_refs
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            failures.push(GovernedContractFailure::OwnerRevisionRefInvalid);
        }
        if evidence_refs.len() != self.governed_evidence_refs.len() {
            failures.push(GovernedContractFailure::LineageDuplicateRevision);
        }
        if self.governed_content.content.trim().is_empty()
            || self.governed_content.observed_at != self.origin.observed_at
            || self.canonical_content_digest().ok().as_deref() != Some(self.content_digest.as_str())
        {
            failures.push(GovernedContractFailure::ContentDigestMismatch);
        }
        failures.sort();
        failures.dedup();
        GovernedContractValidation {
            accepted: failures.is_empty(),
            failures,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermMemoryVersionCreateIntent {
    pub memory_space_id: String,
    pub factual_owner_id: MemorySpaceId,
    pub projection: LongTermMemoryEntry,
    pub governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
    pub requested_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundLongTermVersionCreation {
    pub effective_at: u64,
    pub material: LongTermMemoryVersionMaterial,
    pub head: LongTermMemoryHeadManifest,
}

pub fn bind_long_term_version_creation(
    intent: LongTermMemoryVersionCreateIntent,
    lease: LongTermVersionRetentionLease,
) -> Result<BoundLongTermVersionCreation> {
    if intent.memory_space_id.trim().is_empty()
        || intent.memory_space_id != intent.memory_space_id.trim()
        || intent.factual_owner_id.trim().is_empty()
        || intent.factual_owner_id != intent.factual_owner_id.trim()
        || intent.factual_owner_id != intent.memory_space_id
        || intent.projection.owner_revision != 1
        || lease.max_retained_revisions_per_owner() < 1
    {
        return Err(Error::config(
            "long_term_version_creation",
            "canonical scope, revision-one owner and request-pinned retention capacity are required",
        ));
    }
    let effective_at = intent.requested_at.max(intent.projection.updated_at).max(1);
    let material = LongTermMemoryVersionMaterial::from_current_projection(
        &intent.memory_space_id,
        &intent.factual_owner_id,
        &intent.projection,
        effective_at,
        None,
        intent.governed_evidence_refs,
    )?;
    let head = LongTermMemoryHeadManifest {
        schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
        memory_space_id: intent.memory_space_id,
        factual_owner_id: intent.factual_owner_id,
        owner_ref: material.owner_ref.clone(),
        current_revision: material.owner_revision,
        retained_revision_digests: vec![LongTermMemoryRetainedRevisionDigest {
            owner_revision: material.owner_revision,
            content_digest: material.content_digest.clone(),
        }],
        terminal_transition_ref: None,
        manifest_revision: 1,
    };
    if !head.validate_contract().accepted {
        return Err(Error::config(
            "long_term_version_creation",
            "bound creation head contract is invalid",
        ));
    }
    Ok(BoundLongTermVersionCreation {
        effective_at,
        material,
        head,
    })
}

/// Current-owner before/after image for immutable version materials.
///
/// A mutable owner document has one physical key across a transaction. Immutable version
/// materials do not: an update closes the previous material and creates a new physical key.
/// Keeping both exact keys here prevents post-image validators from inventing a legacy mutable
/// owner address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermMemoryVersionMaterialImage {
    pub before_physical_key: Option<String>,
    pub before: Option<LongTermMemoryVersionMaterial>,
    pub after_physical_key: Option<String>,
    pub after: Option<LongTermMemoryVersionMaterial>,
}

impl LongTermMemoryVersionMaterialImage {
    pub fn created(
        after_physical_key: impl Into<String>,
        after: LongTermMemoryVersionMaterial,
    ) -> Self {
        Self {
            before_physical_key: None,
            before: None,
            after_physical_key: Some(after_physical_key.into()),
            after: Some(after),
        }
    }

    pub fn updated(
        before_physical_key: impl Into<String>,
        before: LongTermMemoryVersionMaterial,
        after_physical_key: impl Into<String>,
        after: LongTermMemoryVersionMaterial,
    ) -> Self {
        Self {
            before_physical_key: Some(before_physical_key.into()),
            before: Some(before),
            after_physical_key: Some(after_physical_key.into()),
            after: Some(after),
        }
    }

    pub fn deleted(
        before_physical_key: impl Into<String>,
        before: LongTermMemoryVersionMaterial,
    ) -> Self {
        Self {
            before_physical_key: Some(before_physical_key.into()),
            before: Some(before),
            after_physical_key: None,
            after: None,
        }
    }

    pub fn observed_owner_ref(&self) -> Option<&GovernedMemoryOwnerRef> {
        self.after
            .as_ref()
            .or(self.before.as_ref())
            .map(|material| &material.owner_ref)
    }

    pub fn has_exact_physical_closure(
        &self,
        memory_space_id: &str,
        factual_owner_id: &str,
    ) -> bool {
        let sides = [
            (self.before_physical_key.as_deref(), self.before.as_ref()),
            (self.after_physical_key.as_deref(), self.after.as_ref()),
        ];
        if sides
            .iter()
            .any(|(key, material)| key.is_some() != material.is_some())
        {
            return false;
        }
        if self.before.is_none() && self.after.is_none() {
            return false;
        }
        sides.into_iter().all(|(key, material)| {
            let Some(material) = material else {
                return true;
            };
            material.memory_space_id == memory_space_id
                && material.factual_owner_id == factual_owner_id
                && material.validate_contract().accepted
                && long_term_version_material_key(
                    memory_space_id,
                    factual_owner_id,
                    &material.owner_ref,
                    material.owner_revision,
                )
                .ok()
                .as_deref()
                    == key
        }) && match (&self.before, &self.after) {
            (Some(before), Some(after)) => before.owner_ref == after.owner_ref,
            _ => true,
        }
    }
}

/// Typed lifecycle payload persisted by the existing control owner.
///
/// This value has no storage identity of its own. Its predecessor revision is the exact control
/// effect target and therefore also the closure key used by head and scope manifests.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedOwnerTransition {
    pub predecessor: GovernedOwnerRevisionRef,
    pub terminated_at: u64,
    pub termination: GovernedOwnerTermination,
    pub successor: Option<GovernedOwnerRevisionRef>,
}

impl GovernedOwnerTransition {
    pub fn validate_contract(
        &self,
        predecessor_material: &LongTermMemoryVersionMaterial,
        successor_material: Option<&LongTermMemoryVersionMaterial>,
    ) -> GovernedContractValidation {
        let mut failures = predecessor_material.validate_contract().failures;
        let predecessor_ref = predecessor_material.owner_revision_ref();
        if !self.predecessor.is_valid() || self.predecessor != predecessor_ref {
            failures.push(GovernedContractFailure::OwnerRevisionRefInvalid);
        }
        if self.terminated_at <= predecessor_material.origin.valid_from {
            failures.push(GovernedContractFailure::ValidityIntervalInvalid);
        }
        if self
            .successor
            .as_ref()
            .is_some_and(|successor| !successor.is_valid())
        {
            failures.push(GovernedContractFailure::OwnerRevisionRefInvalid);
        }

        match (self.successor.as_ref(), successor_material) {
            (Some(successor_ref), Some(successor)) => {
                failures.extend(successor.validate_contract().failures);
                if successor.owner_revision_ref() != *successor_ref
                    || successor.memory_space_id != predecessor_material.memory_space_id
                    || successor.factual_owner_id != predecessor_material.factual_owner_id
                    || successor.origin.predecessor.as_ref() != Some(&self.predecessor)
                    || successor.origin.valid_from != self.terminated_at
                {
                    failures.push(GovernedContractFailure::ValiditySuccessorMismatch);
                }
            }
            (None, None) => {}
            _ => failures.push(GovernedContractFailure::ValiditySuccessorMismatch),
        }

        match self.termination {
            GovernedOwnerTermination::Revised | GovernedOwnerTermination::Corrected => {
                let exact_successor = self.successor.as_ref().is_some_and(|successor| {
                    successor.owner_ref == self.predecessor.owner_ref
                        && self
                            .predecessor
                            .owner_revision
                            .checked_add(1)
                            .is_some_and(|revision| successor.owner_revision == revision)
                });
                if !exact_successor {
                    failures.push(GovernedContractFailure::ValiditySuccessorMismatch);
                }
            }
            GovernedOwnerTermination::Superseded => {
                let cross_owner_revision_one = self.successor.as_ref().is_some_and(|successor| {
                    successor.owner_ref.owner_plane == self.predecessor.owner_ref.owner_plane
                        && successor.owner_ref.owner_id != self.predecessor.owner_ref.owner_id
                        && successor.owner_revision == 1
                });
                if !cross_owner_revision_one {
                    failures.push(GovernedContractFailure::ValiditySuccessorMismatch);
                }
            }
            termination
                if termination.is_terminal_without_successor()
                    && (self.successor.is_some() || successor_material.is_some()) =>
            {
                failures.push(GovernedContractFailure::ValiditySuccessorMismatch);
            }
            _ => {}
        }

        contract_validation(failures)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryRetainedRevisionDigest {
    pub owner_revision: u64,
    pub content_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryHeadManifest {
    pub schema_version: u32,
    pub memory_space_id: String,
    pub factual_owner_id: MemorySpaceId,
    pub owner_ref: GovernedMemoryOwnerRef,
    pub current_revision: u64,
    pub retained_revision_digests: Vec<LongTermMemoryRetainedRevisionDigest>,
    pub terminal_transition_ref: Option<GovernedOwnerRevisionRef>,
    pub manifest_revision: u64,
}

impl LongTermMemoryHeadManifest {
    pub fn canonical_content_digest(&self) -> Result<String> {
        let bytes = serde_json::to_vec(self)
            .map_err(|error| Error::config("long_term_version_head_digest", error.to_string()))?;
        Ok(domain_separated_sha256(
            LONG_TERM_VERSION_HEAD_CONTENT_DIGEST_DOMAIN,
            &[&bytes],
        ))
    }

    pub fn validate_contract(&self) -> GovernedContractValidation {
        let mut failures = Vec::new();
        if self.schema_version != LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION
            || self.memory_space_id.trim().is_empty()
            || self.memory_space_id != self.memory_space_id.trim()
            || self.factual_owner_id.trim().is_empty()
            || self.factual_owner_id != self.factual_owner_id.trim()
            || self.factual_owner_id != self.memory_space_id
            || self.owner_ref.owner_plane != GovernedMemoryOwnerPlane::LongTerm
            || !self.owner_ref.is_valid()
            || self.current_revision == 0
            || self.manifest_revision == 0
        {
            failures.push(GovernedContractFailure::OwnerRevisionRefInvalid);
        }
        let revisions = self
            .retained_revision_digests
            .iter()
            .map(|entry| entry.owner_revision)
            .collect::<BTreeSet<_>>();
        if revisions.len() != self.retained_revision_digests.len()
            || self.retained_revision_digests.iter().any(|entry| {
                entry.owner_revision == 0 || !is_lowercase_sha256(&entry.content_digest)
            })
            || !revisions.contains(&self.current_revision)
        {
            failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
        }
        if self
            .retained_revision_digests
            .last()
            .map(|entry| entry.owner_revision)
            != Some(self.current_revision)
            || self
                .retained_revision_digests
                .windows(2)
                .any(|pair| pair[0].owner_revision >= pair[1].owner_revision)
            || self
                .terminal_transition_ref
                .as_ref()
                .is_some_and(|reference| {
                    !reference.is_valid()
                        || reference.owner_ref != self.owner_ref
                        || reference.owner_revision != self.current_revision
                })
        {
            failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
        }
        contract_validation(failures)
    }
}

pub fn validate_long_term_version_head_closure(
    manifest: &LongTermMemoryHeadManifest,
    materials: &[LongTermMemoryVersionMaterial],
    transitions: &[GovernedOwnerTransition],
    max_retained_revisions_per_owner: usize,
) -> GovernedContractValidation {
    validate_scope_closure(
        &manifest.memory_space_id,
        &manifest.factual_owner_id,
        std::slice::from_ref(manifest),
        materials,
        transitions,
        max_retained_revisions_per_owner,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermMemoryVersionReadProjection {
    pub material: LongTermMemoryVersionMaterial,
    pub validity: GovernedOwnerValidity,
}

pub fn project_long_term_owner_validity(
    material: &LongTermMemoryVersionMaterial,
    transition: Option<&GovernedOwnerTransition>,
    successor_material: Option<&LongTermMemoryVersionMaterial>,
) -> Result<GovernedOwnerValidity> {
    if !material.validate_contract().accepted {
        return Err(Error::config(
            "long_term_owner_validity",
            "owner material is invalid",
        ));
    }
    if let Some(transition) = transition {
        if !transition
            .validate_contract(material, successor_material)
            .accepted
        {
            return Err(Error::config(
                "long_term_owner_validity",
                "owner transition differs from the immutable material closure",
            ));
        }
    } else if successor_material.is_some() {
        return Err(Error::config(
            "long_term_owner_validity",
            "successor material requires an owner transition",
        ));
    }
    let validity = GovernedOwnerValidity {
        valid_from: material.origin.valid_from,
        valid_until: transition.map(|transition| transition.terminated_at),
        observed_at: material.origin.observed_at,
        predecessor: material.origin.predecessor.clone(),
        successor: transition.and_then(|transition| transition.successor.clone()),
        termination: transition.map(|transition| transition.termination),
    };
    if !validity
        .validate_for(&material.owner_revision_ref())
        .accepted
    {
        return Err(Error::config(
            "long_term_owner_validity",
            "projected owner validity is invalid",
        ));
    }
    Ok(validity)
}

pub fn project_current_long_term_recall_lifecycle_facts(
    authority: &LongTermCurrentRecallAuthority,
) -> Result<super::GovernedRecallLifecycleFacts> {
    let projection = &authority.projection;
    let owner_materials = &authority.owner_materials;
    let control_revisions = &authority.control_revisions;
    if !projection.material.validate_contract().accepted
        || !projection
            .validity
            .validate_for(&projection.material.owner_revision_ref())
            .accepted
    {
        return Err(Error::config(
            "long_term_recall_lifecycle_facts",
            "current projection is not canonical",
        ));
    }
    if owner_materials.iter().any(|material| {
        !material.validate_contract().accepted
            || material.memory_space_id != projection.material.memory_space_id
            || material.factual_owner_id != projection.material.factual_owner_id
            || material.owner_ref != projection.material.owner_ref
    }) {
        return Err(Error::config(
            "long_term_recall_lifecycle_facts",
            "owner materials are not canonical for the projected owner",
        ));
    }
    if control_revisions.iter().any(|revision| {
        revision.validate_contract().is_err()
            || revision.memory_space_id != projection.material.memory_space_id
            || revision.factual_owner_id != projection.material.factual_owner_id
            || revision.transition.predecessor.owner_ref != projection.material.owner_ref
    }) {
        return Err(Error::config(
            "long_term_recall_lifecycle_facts",
            "control revisions are not canonical for the projected owner",
        ));
    }
    let explicitly_marked_stale =
        explicitly_marked_stale_for_projection(projection, owner_materials, control_revisions)?;
    super::GovernedRecallLifecycleFacts::new(
        projection.material.owner_revision_ref(),
        projection.validity.clone(),
        explicitly_marked_stale,
        false,
        false,
        false,
    )
}

fn explicitly_marked_stale_for_projection(
    projection: &LongTermMemoryVersionReadProjection,
    owner_materials: &[LongTermMemoryVersionMaterial],
    control_revisions: &[LongTermMemoryControlRevision],
) -> Result<bool> {
    let latest_stale_effect = control_revisions
        .iter()
        .filter(|revision| {
            matches!(
                revision.operation,
                LongTermControlOperation::MarkStale
                    | LongTermControlOperation::Correct
                    | LongTermControlOperation::Refresh
            )
        })
        .filter_map(|revision| {
            revision
                .transition
                .successor
                .as_ref()
                .filter(|successor| {
                    successor.owner_ref == projection.material.owner_ref
                        && successor.owner_revision <= projection.material.owner_revision
                })
                .map(|successor| (successor.owner_revision, revision))
        })
        .max_by_key(|(successor_revision, _)| *successor_revision);
    Ok(match latest_stale_effect {
        Some((_, revision)) if revision.operation == LongTermControlOperation::MarkStale => {
            let successor = revision
                .transition
                .successor
                .as_ref()
                .expect("validated MarkStale control has a successor");
            owner_materials
                .iter()
                .find(|material| material.owner_revision_ref() == *successor)
                .map(|material| {
                    material.governed_content.stale_hint != LongTermMemoryStaleHint::None
                })
                .ok_or_else(|| {
                    Error::config(
                        "long_term_recall_lifecycle_facts",
                        "latest explicit stale successor material is missing",
                    )
                })?
        }
        _ => false,
    })
}

#[derive(Clone, Debug)]
pub struct LongTermCurrentRecallAuthority {
    projection: LongTermMemoryVersionReadProjection,
    owner_materials: Vec<LongTermMemoryVersionMaterial>,
    control_revisions: Vec<LongTermMemoryControlRevision>,
}

impl LongTermCurrentRecallAuthority {
    pub fn projection(&self) -> &LongTermMemoryVersionReadProjection {
        &self.projection
    }
}

#[derive(Clone, Debug)]
pub struct LongTermHistoricalRecallAuthority {
    projection: LongTermMemoryVersionReadProjection,
    lineage_report: MemoryUpdateLineageReport,
    connected_materials: Vec<LongTermMemoryVersionMaterial>,
    connected_control_revisions: Vec<LongTermMemoryControlRevision>,
}

impl LongTermHistoricalRecallAuthority {
    pub fn projection(&self) -> &LongTermMemoryVersionReadProjection {
        &self.projection
    }

    pub fn lineage_report(&self) -> &MemoryUpdateLineageReport {
        &self.lineage_report
    }
}

pub fn select_long_term_historical_recall_query_time<'a>(
    authorities: impl IntoIterator<Item = &'a LongTermHistoricalRecallAuthority>,
    operation_time: u64,
) -> Result<u64> {
    if operation_time == 0 {
        return Err(Error::config(
            "long_term_historical_recall_query_time",
            "operation time must be positive",
        ));
    }
    authorities
        .into_iter()
        .try_fold(operation_time, |logical_time, authority| {
            let projection = authority.projection();
            if !projection.material.validate_contract().accepted
                || !projection
                    .validity
                    .validate_for(&projection.material.owner_revision_ref())
                    .accepted
                || !authority.lineage_report.complete
                || !authority.lineage_report.validate_contract().accepted
            {
                return Err(Error::config(
                    "long_term_historical_recall_query_time",
                    "historical authority projection or lineage is invalid",
                ));
            }
            Ok(logical_time
                .max(projection.validity.valid_from)
                .max(projection.validity.valid_until.unwrap_or_default()))
        })
}

#[allow(clippy::too_many_arguments)]
pub fn build_long_term_historical_recall_authority(
    scope_manifest: &LongTermMemoryVersionScopeManifest,
    heads: &[LongTermMemoryHeadManifest],
    materials: &[LongTermMemoryVersionMaterial],
    control_revisions: &[LongTermMemoryControlRevision],
    selected_owner_ref: &GovernedMemoryOwnerRef,
    as_of_time: u64,
    max_retained_revisions_per_owner: usize,
    max_lineage_depth: usize,
) -> Result<Option<LongTermHistoricalRecallAuthority>> {
    if selected_owner_ref.owner_plane != GovernedMemoryOwnerPlane::LongTerm
        || !selected_owner_ref.is_valid()
        || max_lineage_depth == 0
    {
        return Err(Error::config(
            "long_term_historical_recall_authority",
            "canonical long-term owner and positive lineage depth are required",
        ));
    }

    let materials_by_ref = materials
        .iter()
        .map(|material| (material.owner_revision_ref(), material))
        .collect::<BTreeMap<_, _>>();
    if materials_by_ref.len() != materials.len() {
        return Err(Error::config(
            "long_term_historical_recall_authority",
            "scope contains duplicate material revisions",
        ));
    }

    let mut transitions = Vec::with_capacity(control_revisions.len());
    let mut transition_bindings = Vec::with_capacity(control_revisions.len());
    let mut controls_by_predecessor = BTreeMap::new();
    for revision in control_revisions {
        revision.validate_contract()?;
        let predecessor = materials_by_ref
            .get(&revision.transition.predecessor)
            .copied()
            .ok_or_else(|| {
                Error::config(
                    "long_term_historical_recall_authority",
                    "control predecessor material is missing",
                )
            })?;
        let successor = revision
            .transition
            .successor
            .as_ref()
            .map(|successor| {
                materials_by_ref.get(successor).copied().ok_or_else(|| {
                    Error::config(
                        "long_term_historical_recall_authority",
                        "control successor material is missing",
                    )
                })
            })
            .transpose()?;
        if revision.predecessor_material_digest != predecessor.content_digest
            || revision.successor_material_digest.as_deref()
                != successor.map(|material| material.content_digest.as_str())
            || !revision
                .transition
                .validate_contract(predecessor, successor)
                .accepted
            || controls_by_predecessor
                .insert(revision.transition.predecessor.clone(), revision)
                .is_some()
        {
            return Err(Error::config(
                "long_term_historical_recall_authority",
                "control revision differs from the exact material transition",
            ));
        }
        transition_bindings.push(LongTermMemoryVersionTransitionBinding::new(
            revision.transition.predecessor.clone(),
            scoped_long_term_control_storage_key(
                &revision.memory_space_id,
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                &revision.revision_id,
            )?,
            revision.content_digest.clone(),
        )?);
        transitions.push(revision.transition.clone());
    }

    if !scope_manifest
        .validate_exact(
            heads,
            materials,
            &transitions,
            &transition_bindings,
            max_retained_revisions_per_owner,
        )
        .accepted
    {
        return Err(Error::config(
            "long_term_historical_recall_authority",
            "scope manifest does not bind the exact historical closure",
        ));
    }

    let Some(head) = heads
        .iter()
        .find(|head| &head.owner_ref == selected_owner_ref)
    else {
        return Ok(None);
    };
    let Some(projection) = select_long_term_version_as_of(
        head,
        materials,
        &transitions,
        as_of_time,
        max_retained_revisions_per_owner,
    )?
    else {
        return Ok(None);
    };

    let selected_ref = projection.material.owner_revision_ref();
    let mut connected_refs = BTreeSet::new();
    let mut frontier = BTreeSet::from([selected_ref]);
    while let Some(current) = frontier.iter().next().cloned() {
        frontier.remove(&current);
        if !connected_refs.insert(current.clone()) {
            continue;
        }
        let material = materials_by_ref.get(&current).copied().ok_or_else(|| {
            Error::config(
                "long_term_historical_recall_authority",
                "connected lineage material is missing",
            )
        })?;
        if let Some(predecessor) = material.origin.predecessor.as_ref() {
            frontier.insert(predecessor.clone());
        }
        if let Some(successor) = controls_by_predecessor
            .get(&current)
            .and_then(|revision| revision.transition.successor.as_ref())
        {
            frontier.insert(successor.clone());
        }
    }

    let connected_materials = connected_refs
        .iter()
        .map(|reference| {
            materials_by_ref
                .get(reference)
                .copied()
                .cloned()
                .ok_or_else(|| {
                    Error::config(
                        "long_term_historical_recall_authority",
                        "connected lineage material is missing",
                    )
                })
        })
        .collect::<Result<Vec<_>>>()?;
    let connected_control_revisions = connected_refs
        .iter()
        .filter_map(|reference| controls_by_predecessor.get(reference).copied().cloned())
        .collect::<Vec<_>>();
    let items = connected_materials
        .iter()
        .map(|material| GovernedUpdateLineageItem {
            owner_revision_ref: material.owner_revision_ref(),
            predecessor: material.origin.predecessor.clone(),
            successor: controls_by_predecessor
                .get(&material.owner_revision_ref())
                .and_then(|revision| revision.transition.successor.clone()),
            scope_digest: scope_manifest.closure_digest.clone(),
            privacy_class: material.privacy_class,
            content_digest: material.content_digest.clone(),
        })
        .collect::<Vec<_>>();
    let lineage_report = build_memory_update_lineage_report(
        items,
        scope_manifest.manifest_revision,
        max_lineage_depth,
    )?;
    if !lineage_report.complete {
        return Err(Error::config(
            "long_term_historical_recall_authority",
            "historical lineage is incomplete or crosses a governed boundary",
        ));
    }

    Ok(Some(LongTermHistoricalRecallAuthority {
        projection,
        lineage_report,
        connected_materials,
        connected_control_revisions,
    }))
}

pub fn build_long_term_current_recall_authority(
    scope_manifest: &LongTermMemoryVersionScopeManifest,
    head: &LongTermMemoryHeadManifest,
    owner_materials: &[LongTermMemoryVersionMaterial],
    dependency_heads: &[LongTermMemoryHeadManifest],
    dependency_materials: &[LongTermMemoryVersionMaterial],
    control_revisions: &[LongTermMemoryControlRevision],
    max_retained_revisions_per_owner: usize,
) -> Result<LongTermCurrentRecallAuthority> {
    validate_long_term_recall_scope_binding(
        scope_manifest,
        head,
        owner_materials,
        dependency_heads,
        dependency_materials,
        control_revisions,
        max_retained_revisions_per_owner,
    )?;
    let materials = owner_materials
        .iter()
        .chain(dependency_materials)
        .cloned()
        .collect::<Vec<_>>();
    let transitions = control_revisions
        .iter()
        .map(|revision| revision.transition.clone())
        .collect::<Vec<_>>();
    for revision in control_revisions {
        revision.validate_contract()?;
        let predecessor = materials
            .iter()
            .find(|material| material.owner_revision_ref() == revision.transition.predecessor)
            .ok_or_else(|| {
                Error::config(
                    "long_term_current_recall_authority",
                    "control predecessor material is missing",
                )
            })?;
        let successor = revision
            .transition
            .successor
            .as_ref()
            .map(|successor| {
                materials
                    .iter()
                    .find(|material| material.owner_revision_ref() == *successor)
                    .ok_or_else(|| {
                        Error::config(
                            "long_term_current_recall_authority",
                            "control successor material is missing",
                        )
                    })
            })
            .transpose()?;
        if revision.predecessor_material_digest != predecessor.content_digest
            || revision.successor_material_digest.as_deref()
                != successor.map(|material| material.content_digest.as_str())
            || !revision
                .transition
                .validate_contract(predecessor, successor)
                .accepted
        {
            return Err(Error::config(
                "long_term_current_recall_authority",
                "control revision is not bound to the exact material closure",
            ));
        }
    }
    let projection = select_long_term_version_current(
        head,
        &materials,
        &transitions,
        max_retained_revisions_per_owner,
    )?;
    Ok(LongTermCurrentRecallAuthority {
        projection,
        owner_materials: owner_materials.to_vec(),
        control_revisions: control_revisions.to_vec(),
    })
}

pub fn project_historical_long_term_recall_lifecycle_facts(
    authority: &LongTermHistoricalRecallAuthority,
) -> Result<super::GovernedRecallLifecycleFacts> {
    let projection = &authority.projection;
    if !projection.material.validate_contract().accepted
        || !projection
            .validity
            .validate_for(&projection.material.owner_revision_ref())
            .accepted
        || !authority.lineage_report.complete
        || !authority.lineage_report.validate_contract().accepted
    {
        return Err(Error::config(
            "long_term_historical_recall_lifecycle_facts",
            "historical projection and lineage are not canonical",
        ));
    }
    let explicitly_marked_stale = explicitly_marked_stale_for_projection(
        projection,
        &authority.connected_materials,
        &authority.connected_control_revisions,
    )?;
    let historical_model_allowed = matches!(
        projection.validity.termination,
        None | Some(GovernedOwnerTermination::Revised | GovernedOwnerTermination::Superseded)
    );
    super::GovernedRecallLifecycleFacts::new(
        projection.material.owner_revision_ref(),
        projection.validity.clone(),
        explicitly_marked_stale,
        false,
        false,
        historical_model_allowed,
    )
}

fn validate_long_term_recall_scope_binding(
    scope_manifest: &LongTermMemoryVersionScopeManifest,
    head: &LongTermMemoryHeadManifest,
    owner_materials: &[LongTermMemoryVersionMaterial],
    dependency_heads: &[LongTermMemoryHeadManifest],
    dependency_materials: &[LongTermMemoryVersionMaterial],
    control_revisions: &[LongTermMemoryControlRevision],
    max_retained_revisions_per_owner: usize,
) -> Result<()> {
    let canonical_scope_key =
        long_term_version_scope_manifest_key(&head.memory_space_id, &head.factual_owner_id)?;
    if scope_manifest.schema_version != LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION
        || scope_manifest.physical_key != canonical_scope_key
        || scope_manifest.memory_space_id != head.memory_space_id
        || scope_manifest.factual_owner_id != head.factual_owner_id
        || scope_manifest.manifest_revision == 0
        || scope_manifest.head_count != scope_manifest.head_bindings.len() as u64
        || scope_manifest.transition_count != scope_manifest.transition_bindings.len() as u64
        || scope_manifest.material_count < scope_manifest.head_count
        || !is_lowercase_sha256(&scope_manifest.closure_digest)
        || scope_manifest
            .head_bindings
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || scope_manifest
            .transition_bindings
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || scope_manifest
            .head_bindings
            .iter()
            .filter(|binding| binding.owner_ref == head.owner_ref)
            .count()
            != 1
        || !scope_manifest
            .head_bindings
            .contains(&LongTermMemoryVersionHeadBinding::from_head(head)?)
    {
        return Err(Error::config(
            "long_term_current_recall_authority",
            "scope root does not bind the exact owner head",
        ));
    }

    let expected_predecessors = owner_materials
        .iter()
        .filter(|material| {
            material.owner_revision != head.current_revision
                || head.terminal_transition_ref.as_ref() == Some(&material.owner_revision_ref())
        })
        .map(LongTermMemoryVersionMaterial::owner_revision_ref)
        .collect::<BTreeSet<_>>();
    let revisions_by_predecessor = control_revisions
        .iter()
        .map(|revision| (revision.transition.predecessor.clone(), revision))
        .collect::<BTreeMap<_, _>>();
    if revisions_by_predecessor.len() != control_revisions.len()
        || revisions_by_predecessor
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != expected_predecessors
    {
        return Err(Error::config(
            "long_term_current_recall_authority",
            "control revisions differ from the exact owner transition closure",
        ));
    }

    for (predecessor, revision) in &revisions_by_predecessor {
        revision.validate_contract()?;
        let canonical_control_key = scoped_long_term_control_storage_key(
            &head.memory_space_id,
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
            &revision.revision_id,
        )?;
        let exact_binding = scope_manifest
            .transition_bindings
            .iter()
            .filter(|binding| &binding.predecessor == predecessor)
            .collect::<Vec<_>>();
        if revision.memory_space_id != head.memory_space_id
            || revision.factual_owner_id != head.factual_owner_id
            || exact_binding.len() != 1
            || exact_binding[0].control_revision_physical_key != canonical_control_key
            || exact_binding[0].control_revision_content_digest != revision.content_digest
        {
            return Err(Error::config(
                "long_term_current_recall_authority",
                "scope root does not bind the exact control revision",
            ));
        }
    }

    let expected_dependencies = control_revisions
        .iter()
        .filter_map(|revision| revision.transition.successor.as_ref())
        .filter(|successor| successor.owner_ref != head.owner_ref)
        .cloned()
        .collect::<BTreeSet<_>>();
    let actual_dependencies = dependency_materials
        .iter()
        .map(LongTermMemoryVersionMaterial::owner_revision_ref)
        .collect::<BTreeSet<_>>();
    let expected_dependency_owners = expected_dependencies
        .iter()
        .map(|dependency| dependency.owner_ref.clone())
        .collect::<BTreeSet<_>>();
    let dependency_heads_by_owner = dependency_heads
        .iter()
        .map(|dependency_head| (dependency_head.owner_ref.clone(), dependency_head))
        .collect::<BTreeMap<_, _>>();
    if actual_dependencies.len() != dependency_materials.len()
        || actual_dependencies != expected_dependencies
        || dependency_heads_by_owner.len() != dependency_heads.len()
        || dependency_heads_by_owner
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != expected_dependency_owners
        || dependency_materials.iter().any(|material| {
            material.memory_space_id != head.memory_space_id
                || material.factual_owner_id != head.factual_owner_id
                || material.owner_ref == head.owner_ref
        })
    {
        return Err(Error::config(
            "long_term_current_recall_authority",
            "cross-owner dependency materials differ from exact transition successors",
        ));
    }
    for material in dependency_materials {
        let dependency_head = dependency_heads_by_owner
            .get(&material.owner_ref)
            .copied()
            .ok_or_else(|| {
                Error::config(
                    "long_term_current_recall_authority",
                    "cross-owner dependency head is missing",
                )
            })?;
        let exact_binding = scope_manifest
            .head_bindings
            .iter()
            .filter(|binding| binding.owner_ref == material.owner_ref)
            .collect::<Vec<_>>();
        if dependency_head.memory_space_id != head.memory_space_id
            || dependency_head.factual_owner_id != head.factual_owner_id
            || dependency_head.retained_revision_digests.len() > max_retained_revisions_per_owner
            || exact_binding.len() != 1
            || exact_binding[0] != &LongTermMemoryVersionHeadBinding::from_head(dependency_head)?
            || !dependency_head
                .retained_revision_digests
                .iter()
                .any(|retained| {
                    retained.owner_revision == material.owner_revision
                        && retained.content_digest == material.content_digest
                })
        {
            return Err(Error::config(
                "long_term_current_recall_authority",
                "scope root does not bind the exact cross-owner dependency head and material",
            ));
        }
    }
    Ok(())
}

pub fn select_long_term_current_recall_query_time<'a>(
    authorities: impl IntoIterator<Item = &'a LongTermCurrentRecallAuthority>,
    operation_time: u64,
) -> Result<u64> {
    if operation_time == 0 {
        return Err(Error::config(
            "long_term_current_recall_query_time",
            "operation time must be positive",
        ));
    }
    Ok(authorities
        .into_iter()
        .fold(operation_time, |logical_time, authority| {
            logical_time
                .max(authority.projection.validity.valid_from)
                .max(
                    authority
                        .projection
                        .validity
                        .valid_until
                        .unwrap_or_default(),
                )
        }))
}

pub fn select_long_term_version_current(
    head: &LongTermMemoryHeadManifest,
    materials: &[LongTermMemoryVersionMaterial],
    transitions: &[GovernedOwnerTransition],
    max_retained_revisions_per_owner: usize,
) -> Result<LongTermMemoryVersionReadProjection> {
    if !head.validate_contract().accepted || max_retained_revisions_per_owner == 0 {
        return Err(Error::config(
            "long_term_version_current_read",
            "head contract or request-pinned retention limit is invalid",
        ));
    }
    let material = materials
        .iter()
        .find(|material| {
            material.owner_ref == head.owner_ref && material.owner_revision == head.current_revision
        })
        .ok_or_else(|| {
            Error::config(
                "long_term_version_current_read",
                "current material is missing from the exact head closure",
            )
        })?;
    let projection = select_long_term_version_as_of(
        head,
        materials,
        transitions,
        material.origin.valid_from,
        max_retained_revisions_per_owner,
    )?
    .ok_or_else(|| {
        Error::config(
            "long_term_version_current_read",
            "current material validity does not contain its own valid-from time",
        )
    })?;
    if projection.material.owner_revision_ref() != material.owner_revision_ref() {
        return Err(Error::config(
            "long_term_version_current_read",
            "as-of closure selected a different current material",
        ));
    }
    Ok(projection)
}

pub fn select_long_term_version_as_of(
    head: &LongTermMemoryHeadManifest,
    materials: &[LongTermMemoryVersionMaterial],
    transitions: &[GovernedOwnerTransition],
    as_of_time: u64,
    max_retained_revisions_per_owner: usize,
) -> Result<Option<LongTermMemoryVersionReadProjection>> {
    if !head.validate_contract().accepted || max_retained_revisions_per_owner == 0 {
        return Err(Error::config(
            "long_term_version_as_of_read",
            "head contract or request-pinned retention limit is invalid",
        ));
    }
    let owner_materials = materials
        .iter()
        .filter(|material| material.owner_ref == head.owner_ref)
        .collect::<Vec<_>>();
    let retained_digests = owner_materials
        .iter()
        .map(|material| LongTermMemoryRetainedRevisionDigest {
            owner_revision: material.owner_revision,
            content_digest: material.content_digest.clone(),
        })
        .collect::<BTreeSet<_>>();
    if owner_materials.len() > max_retained_revisions_per_owner
        || retained_digests
            != head
                .retained_revision_digests
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
    {
        return Err(Error::config(
            "long_term_version_as_of_read",
            "retained material set differs from the exact head closure",
        ));
    }
    let materials_by_ref = materials
        .iter()
        .map(|material| (material.owner_revision_ref(), material))
        .collect::<BTreeMap<_, _>>();
    if materials_by_ref.len() != materials.len() {
        return Err(Error::config(
            "long_term_version_as_of_read",
            "duplicate material revision",
        ));
    }
    let transitions_by_predecessor = transitions
        .iter()
        .map(|transition| (transition.predecessor.clone(), transition))
        .collect::<BTreeMap<_, _>>();
    if transitions_by_predecessor.len() != transitions.len() {
        return Err(Error::config(
            "long_term_version_as_of_read",
            "duplicate predecessor transition",
        ));
    }

    let mut selected = None;
    for material in owner_materials {
        let owner_revision_ref = material.owner_revision_ref();
        let transition = transitions_by_predecessor.get(&owner_revision_ref).copied();
        let transition_required = material.owner_revision != head.current_revision
            || head.terminal_transition_ref.as_ref() == Some(&owner_revision_ref);
        if transition.is_some() != transition_required {
            return Err(Error::config(
                "long_term_version_as_of_read",
                "material transition presence differs from the pinned head",
            ));
        }
        let successor_material = transition.and_then(|transition| {
            transition
                .successor
                .as_ref()
                .and_then(|successor| materials_by_ref.get(successor).copied())
        });
        let validity = project_long_term_owner_validity(material, transition, successor_material)?;
        let contains_as_of = as_of_time >= validity.valid_from
            && validity
                .valid_until
                .is_none_or(|valid_until| as_of_time < valid_until);
        if contains_as_of {
            if selected.is_some() {
                return Err(Error::config(
                    "long_term_version_as_of_read",
                    "multiple retained materials overlap the as-of time",
                ));
            }
            selected = Some(LongTermMemoryVersionReadProjection {
                material: material.clone(),
                validity,
            });
        }
    }
    Ok(selected)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryVersionHeadBinding {
    pub owner_ref: GovernedMemoryOwnerRef,
    pub head_physical_key: String,
    pub head_content_digest: String,
    pub head_manifest_revision: u64,
}

impl LongTermMemoryVersionHeadBinding {
    pub fn from_head(head: &LongTermMemoryHeadManifest) -> Result<Self> {
        if !head.validate_contract().accepted {
            return Err(Error::config(
                "long_term_version_head_binding",
                "head contract is invalid",
            ));
        }
        Ok(Self {
            owner_ref: head.owner_ref.clone(),
            head_physical_key: long_term_version_head_key(
                &head.memory_space_id,
                &head.factual_owner_id,
                &head.owner_ref,
            )?,
            head_content_digest: head.canonical_content_digest()?,
            head_manifest_revision: head.manifest_revision,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryVersionTransitionBinding {
    pub predecessor: GovernedOwnerRevisionRef,
    pub control_revision_physical_key: String,
    pub control_revision_content_digest: String,
}

impl LongTermMemoryVersionTransitionBinding {
    pub fn new(
        predecessor: GovernedOwnerRevisionRef,
        control_revision_physical_key: impl Into<String>,
        control_revision_content_digest: impl Into<String>,
    ) -> Result<Self> {
        let control_revision_physical_key = control_revision_physical_key.into();
        let control_revision_content_digest = control_revision_content_digest.into();
        if !predecessor.is_valid()
            || predecessor.owner_ref.owner_plane != GovernedMemoryOwnerPlane::LongTerm
            || control_revision_physical_key.trim().is_empty()
            || control_revision_physical_key != control_revision_physical_key.trim()
            || !is_lowercase_sha256(&control_revision_content_digest)
        {
            return Err(Error::config(
                "long_term_version_transition_binding",
                "exact predecessor, canonical control key and lowercase sha256 digest are required",
            ));
        }
        Ok(Self {
            predecessor,
            control_revision_physical_key,
            control_revision_content_digest,
        })
    }

    fn validate_for(&self, transition: &GovernedOwnerTransition) -> Result<()> {
        let canonical = Self::new(
            self.predecessor.clone(),
            self.control_revision_physical_key.clone(),
            self.control_revision_content_digest.clone(),
        )?;
        if self == &canonical && self.predecessor == transition.predecessor {
            Ok(())
        } else {
            Err(Error::config(
                "long_term_version_transition_binding",
                "control binding does not identify the exact transition predecessor",
            ))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongTermMemoryVersionScopeManifest {
    pub schema_version: u32,
    pub physical_key: String,
    pub memory_space_id: String,
    pub factual_owner_id: MemorySpaceId,
    pub manifest_revision: u64,
    pub head_bindings: Vec<LongTermMemoryVersionHeadBinding>,
    pub transition_bindings: Vec<LongTermMemoryVersionTransitionBinding>,
    pub head_count: u64,
    pub material_count: u64,
    pub transition_count: u64,
    pub closure_digest: String,
}

impl LongTermMemoryVersionScopeManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        memory_space_id: &str,
        factual_owner_id: &str,
        manifest_revision: u64,
        heads: &[LongTermMemoryHeadManifest],
        materials: &[LongTermMemoryVersionMaterial],
        transitions: &[GovernedOwnerTransition],
        transition_bindings: &[LongTermMemoryVersionTransitionBinding],
        max_retained_revisions_per_owner: usize,
    ) -> Result<Self> {
        let validation = validate_scope_closure(
            memory_space_id,
            factual_owner_id,
            heads,
            materials,
            transitions,
            max_retained_revisions_per_owner,
        );
        if manifest_revision == 0 || !validation.accepted {
            return Err(Error::config(
                "long_term_version_scope_manifest",
                format!("scope closure rejected: {:?}", validation.failures),
            ));
        }

        let mut head_bindings = heads
            .iter()
            .map(LongTermMemoryVersionHeadBinding::from_head)
            .collect::<Result<Vec<_>>>()?;
        head_bindings.sort();
        if head_bindings
            .windows(2)
            .any(|pair| pair[0].owner_ref == pair[1].owner_ref)
        {
            return Err(Error::config(
                "long_term_version_scope_manifest",
                "scope contains duplicate head bindings",
            ));
        }

        let mut transition_bindings = transition_bindings.to_vec();
        transition_bindings.sort();
        if transition_bindings.len() != transitions.len()
            || transition_bindings.windows(2).any(|pair| {
                pair[0].predecessor == pair[1].predecessor
                    || pair[0].control_revision_physical_key
                        == pair[1].control_revision_physical_key
            })
        {
            return Err(Error::config(
                "long_term_version_scope_manifest",
                "transition bindings must be an exact unique set",
            ));
        }
        let transitions_by_predecessor = transitions
            .iter()
            .map(|transition| (transition.predecessor.clone(), transition))
            .collect::<BTreeMap<_, _>>();
        if transitions_by_predecessor.len() != transitions.len()
            || transition_bindings.iter().any(|binding| {
                transitions_by_predecessor
                    .get(&binding.predecessor)
                    .is_none_or(|transition| binding.validate_for(transition).is_err())
            })
        {
            return Err(Error::config(
                "long_term_version_scope_manifest",
                "transition bindings differ from the exact transition closure",
            ));
        }

        Ok(Self {
            schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
            physical_key: long_term_version_scope_manifest_key(memory_space_id, factual_owner_id)?,
            memory_space_id: memory_space_id.to_owned(),
            factual_owner_id: factual_owner_id.to_owned(),
            manifest_revision,
            head_bindings: head_bindings.clone(),
            transition_bindings: transition_bindings.clone(),
            head_count: bounded_count("head_count", heads.len())?,
            material_count: bounded_count("material_count", materials.len())?,
            transition_count: bounded_count("transition_count", transitions.len())?,
            closure_digest: scope_closure_digest(
                memory_space_id,
                factual_owner_id,
                manifest_revision,
                heads,
                materials,
                transitions,
                &head_bindings,
                &transition_bindings,
            )?,
        })
    }

    pub fn validate_exact(
        &self,
        heads: &[LongTermMemoryHeadManifest],
        materials: &[LongTermMemoryVersionMaterial],
        transitions: &[GovernedOwnerTransition],
        transition_bindings: &[LongTermMemoryVersionTransitionBinding],
        max_retained_revisions_per_owner: usize,
    ) -> GovernedContractValidation {
        let mut failures = validate_scope_closure(
            &self.memory_space_id,
            &self.factual_owner_id,
            heads,
            materials,
            transitions,
            max_retained_revisions_per_owner,
        )
        .failures;
        let expected = Self::build(
            &self.memory_space_id,
            &self.factual_owner_id,
            self.manifest_revision,
            heads,
            materials,
            transitions,
            transition_bindings,
            max_retained_revisions_per_owner,
        );
        if expected.as_ref().is_err() || expected.as_ref().is_ok_and(|expected| expected != self) {
            failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
        }
        contract_validation(failures)
    }
}

fn validate_scope_closure(
    memory_space_id: &str,
    factual_owner_id: &str,
    heads: &[LongTermMemoryHeadManifest],
    materials: &[LongTermMemoryVersionMaterial],
    transitions: &[GovernedOwnerTransition],
    max_retained_revisions_per_owner: usize,
) -> GovernedContractValidation {
    let mut failures = Vec::new();
    if memory_space_id.trim().is_empty()
        || memory_space_id != memory_space_id.trim()
        || factual_owner_id.trim().is_empty()
        || factual_owner_id != factual_owner_id.trim()
        || factual_owner_id != memory_space_id
        || max_retained_revisions_per_owner == 0
    {
        failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
    }

    let mut heads_by_owner = BTreeMap::new();
    for head in heads {
        failures.extend(head.validate_contract().failures);
        if head.memory_space_id != memory_space_id
            || head.factual_owner_id != factual_owner_id
            || head.retained_revision_digests.len() > max_retained_revisions_per_owner
            || heads_by_owner
                .insert(head.owner_ref.clone(), head)
                .is_some()
        {
            failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
        }
    }

    if heads
        .len()
        .checked_mul(max_retained_revisions_per_owner)
        .is_none_or(|maximum_materials| materials.len() > maximum_materials)
    {
        failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
    }

    let mut materials_by_ref = BTreeMap::new();
    let mut material_digests_by_owner =
        BTreeMap::<GovernedMemoryOwnerRef, BTreeSet<LongTermMemoryRetainedRevisionDigest>>::new();
    for material in materials {
        failures.extend(material.validate_contract().failures);
        let material_ref = material.owner_revision_ref();
        if material.memory_space_id != memory_space_id
            || material.factual_owner_id != factual_owner_id
            || materials_by_ref.insert(material_ref, material).is_some()
        {
            failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
        }
        material_digests_by_owner
            .entry(material.owner_ref.clone())
            .or_default()
            .insert(LongTermMemoryRetainedRevisionDigest {
                owner_revision: material.owner_revision,
                content_digest: material.content_digest.clone(),
            });
    }

    for head in heads {
        let expected = head
            .retained_revision_digests
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let actual = material_digests_by_owner
            .remove(&head.owner_ref)
            .unwrap_or_default();
        if expected != actual {
            failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
        }
    }
    if !material_digests_by_owner.is_empty() {
        failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
    }

    let mut transitions_by_predecessor = BTreeMap::new();
    for transition in transitions {
        if !transition.predecessor.is_valid()
            || transitions_by_predecessor
                .insert(transition.predecessor.clone(), transition)
                .is_some()
        {
            failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
        }
    }

    let mut expected_transition_predecessors = BTreeSet::new();
    for head in heads {
        let mut retained_materials = materials
            .iter()
            .filter(|material| material.owner_ref == head.owner_ref)
            .collect::<Vec<_>>();
        retained_materials.sort_by_key(|material| material.owner_revision);
        for pair in retained_materials.windows(2) {
            if pair[0]
                .owner_revision
                .checked_add(1)
                .is_none_or(|revision| revision != pair[1].owner_revision)
            {
                failures.push(GovernedContractFailure::LineageGap);
            }
            expected_transition_predecessors.insert(pair[0].owner_revision_ref());
        }
        if let Some(terminal_transition_ref) = &head.terminal_transition_ref {
            expected_transition_predecessors.insert(terminal_transition_ref.clone());
        }
    }

    let actual_transition_predecessors = transitions_by_predecessor
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if expected_transition_predecessors != actual_transition_predecessors {
        failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
    }

    for predecessor_ref in expected_transition_predecessors {
        let Some(predecessor_material) = materials_by_ref.get(&predecessor_ref).copied() else {
            failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
            continue;
        };
        let Some(transition) = transitions_by_predecessor.get(&predecessor_ref).copied() else {
            failures.push(GovernedContractFailure::HeadManifestClosureMismatch);
            continue;
        };
        let successor_material = transition
            .successor
            .as_ref()
            .and_then(|successor| materials_by_ref.get(successor).copied());
        failures.extend(
            transition
                .validate_contract(predecessor_material, successor_material)
                .failures,
        );
    }

    contract_validation(failures)
}

#[allow(clippy::too_many_arguments)]
fn scope_closure_digest(
    memory_space_id: &str,
    factual_owner_id: &str,
    manifest_revision: u64,
    heads: &[LongTermMemoryHeadManifest],
    materials: &[LongTermMemoryVersionMaterial],
    transitions: &[GovernedOwnerTransition],
    head_bindings: &[LongTermMemoryVersionHeadBinding],
    transition_bindings: &[LongTermMemoryVersionTransitionBinding],
) -> Result<String> {
    let mut canonical_heads = heads.to_vec();
    canonical_heads.sort_by(|left, right| left.owner_ref.cmp(&right.owner_ref));
    let mut canonical_materials = materials
        .iter()
        .map(|material| {
            (
                &material.owner_ref,
                material.owner_revision,
                material.content_digest.as_str(),
            )
        })
        .collect::<Vec<_>>();
    canonical_materials.sort();
    let mut canonical_transitions = transitions.to_vec();
    canonical_transitions.sort();
    let mut canonical_head_bindings = head_bindings.to_vec();
    canonical_head_bindings.sort();
    let mut canonical_transition_bindings = transition_bindings.to_vec();
    canonical_transition_bindings.sort();
    let bytes = serde_json::to_vec(&(
        memory_space_id,
        factual_owner_id,
        manifest_revision,
        canonical_heads,
        canonical_materials,
        canonical_transitions,
        canonical_head_bindings,
        canonical_transition_bindings,
    ))
    .map_err(|error| Error::config("long_term_version_scope_digest", error.to_string()))?;
    Ok(domain_separated_sha256(
        LONG_TERM_VERSION_SCOPE_CLOSURE_DIGEST_DOMAIN,
        &[&bytes],
    ))
}

fn bounded_count(field: &str, count: usize) -> Result<u64> {
    u64::try_from(count).map_err(|_| {
        Error::config(
            "long_term_version_scope_manifest",
            format!("{field} exceeds the manifest count representation"),
        )
    })
}

fn contract_validation(mut failures: Vec<GovernedContractFailure>) -> GovernedContractValidation {
    failures.sort();
    failures.dedup();
    GovernedContractValidation {
        accepted: failures.is_empty(),
        failures,
    }
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn long_term_version_material_key(
    memory_space_id: &str,
    factual_owner_id: &str,
    owner_ref: &GovernedMemoryOwnerRef,
    owner_revision: u64,
) -> Result<String> {
    validate_key_input(memory_space_id, factual_owner_id, owner_ref, owner_revision)?;
    Ok(format!(
        "p8-material:{}",
        domain_separated_sha256(
            LONG_TERM_VERSION_MATERIAL_KEY_DOMAIN,
            &[
                memory_space_id.as_bytes(),
                factual_owner_id.as_bytes(),
                owner_ref.owner_plane.as_str().as_bytes(),
                owner_ref.owner_id.as_bytes(),
                &owner_revision.to_be_bytes(),
            ],
        )
    ))
}

pub fn long_term_version_head_key(
    memory_space_id: &str,
    factual_owner_id: &str,
    owner_ref: &GovernedMemoryOwnerRef,
) -> Result<String> {
    validate_key_input(memory_space_id, factual_owner_id, owner_ref, 1)?;
    Ok(format!(
        "p8-head:{}",
        domain_separated_sha256(
            LONG_TERM_VERSION_HEAD_KEY_DOMAIN,
            &[
                memory_space_id.as_bytes(),
                factual_owner_id.as_bytes(),
                owner_ref.owner_plane.as_str().as_bytes(),
                owner_ref.owner_id.as_bytes(),
            ],
        )
    ))
}

pub fn long_term_version_scope_manifest_key(
    memory_space_id: &str,
    factual_owner_id: &str,
) -> Result<String> {
    validate_scope_key_input(memory_space_id, factual_owner_id)?;
    Ok(format!(
        "long-term-version-scope:{}",
        domain_separated_sha256(
            LONG_TERM_VERSION_SCOPE_MANIFEST_KEY_DOMAIN,
            &[memory_space_id.as_bytes(), factual_owner_id.as_bytes(),],
        )
    ))
}

fn validate_key_input(
    memory_space_id: &str,
    factual_owner_id: &str,
    owner_ref: &GovernedMemoryOwnerRef,
    owner_revision: u64,
) -> Result<()> {
    validate_scope_key_input(memory_space_id, factual_owner_id)?;
    if owner_ref.owner_plane != GovernedMemoryOwnerPlane::LongTerm
        || !owner_ref.is_valid()
        || owner_revision == 0
    {
        return Err(Error::config(
            "long_term_version_key",
            "scope, long-term owner and positive revision are required",
        ));
    }
    Ok(())
}

fn validate_scope_key_input(memory_space_id: &str, factual_owner_id: &str) -> Result<()> {
    if memory_space_id.trim().is_empty()
        || memory_space_id != memory_space_id.trim()
        || factual_owner_id.trim().is_empty()
        || factual_owner_id != factual_owner_id.trim()
    {
        return Err(Error::config(
            "long_term_version_key",
            "canonical memory space and factual owner are required",
        ));
    }
    Ok(())
}

fn domain_separated_sha256(domain: &str, fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update((domain.len() as u64).to_be_bytes());
    hasher.update(domain.as_bytes());
    for field in fields {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field);
    }
    format!("{:x}", hasher.finalize())
}
