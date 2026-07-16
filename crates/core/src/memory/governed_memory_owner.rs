//! Shared typed owner identity for governed memory projections.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

use super::{
    canonical_evidence_ref_from_source, canonical_recall_evidence_group,
    CanonicalRecallEvidenceFamilyGroup, LongTermMemoryEntry,
};

const GOVERNED_MEMORY_RECALL_CANDIDATE_DOMAIN: &str = "governed_memory_recall_candidate_id_v1";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GovernedMemoryOwnerPlane {
    LongTerm,
    EvidenceDocument,
    ConversationTranscript,
    MemoryGraph,
    RuntimeSkill,
}

impl GovernedMemoryOwnerPlane {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LongTerm => "long_term",
            Self::EvidenceDocument => "evidence_document",
            Self::ConversationTranscript => "conversation_transcript",
            Self::MemoryGraph => "memory_graph",
            Self::RuntimeSkill => "runtime_skill",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GovernedMemoryOwnerRef {
    pub owner_plane: GovernedMemoryOwnerPlane,
    pub owner_id: String,
}

/// Immutable evidence identity closed by the governed owner loader.
///
/// Consumers receive opaque safe refs and canonical identities only. Raw locators are never
/// reparsed by selection or rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedEvidenceBinding {
    safe_evidence_ref: String,
    canonical_evidence_group: String,
    evidence_family_group: Option<String>,
}

impl GovernedEvidenceBinding {
    pub(crate) fn new(
        safe_evidence_ref: String,
        canonical_evidence_group: String,
        evidence_family_group: Option<String>,
    ) -> Self {
        Self {
            safe_evidence_ref,
            canonical_evidence_group,
            evidence_family_group,
        }
    }

    pub fn try_new(
        safe_evidence_ref: impl Into<String>,
        canonical_evidence_group: impl Into<String>,
        evidence_family_group: Option<String>,
    ) -> Result<Self> {
        let safe_evidence_ref = safe_evidence_ref.into();
        let canonical_evidence_group = canonical_evidence_group.into();
        let valid = !safe_evidence_ref.trim().is_empty()
            && safe_evidence_ref == safe_evidence_ref.trim()
            && canonical_recall_evidence_group(&safe_evidence_ref) == canonical_evidence_group
            && evidence_family_group.as_ref().is_none_or(|family| {
                family == family.trim()
                    && CanonicalRecallEvidenceFamilyGroup::from_canonical(family.clone()).is_some()
            });
        if !valid {
            return Err(Error::config(
                "governed_evidence_binding",
                "evidence binding is not canonical",
            ));
        }
        Ok(Self::new(
            safe_evidence_ref,
            canonical_evidence_group,
            evidence_family_group,
        ))
    }

    pub fn safe_evidence_ref(&self) -> &str {
        &self.safe_evidence_ref
    }

    pub fn canonical_evidence_group(&self) -> &str {
        &self.canonical_evidence_group
    }

    pub fn evidence_family_group(&self) -> Option<&str> {
        self.evidence_family_group.as_deref()
    }

    pub fn effective_evidence_family_group(&self) -> &str {
        self.evidence_family_group
            .as_deref()
            .unwrap_or(&self.canonical_evidence_group)
    }
}

/// Closes long-term evidence identity from the persisted owner, never from a facet projection.
pub fn governed_long_term_owner_evidence_bindings(
    entry: &LongTermMemoryEntry,
) -> Result<Vec<GovernedEvidenceBinding>> {
    let mut bindings_by_safe_ref = BTreeMap::new();
    let mut explicit_families_by_group = BTreeMap::<String, BTreeSet<String>>::new();
    for citation in &entry.supporting_citations {
        let evidence = canonical_evidence_ref_from_source(citation).ok_or_else(|| {
            Error::config(
                "governed_long_term_owner_evidence_binding",
                "long-term owner contains an invalid supporting citation",
            )
        })?;
        bindings_by_safe_ref.insert(
            evidence.source_ref.clone(),
            GovernedEvidenceBinding::new(
                evidence.source_ref,
                evidence.canonical_evidence_group,
                None,
            ),
        );
    }
    for evidence in entry
        .canonical_entities
        .iter()
        .flat_map(|entity| entity.evidence_refs.iter())
    {
        let safe_ref = evidence.source_ref.trim();
        let canonical_group = evidence.canonical_evidence_group.trim();
        let canonical_group_valid = !safe_ref.is_empty()
            && canonical_group == evidence.canonical_evidence_group
            && canonical_recall_evidence_group(safe_ref) == canonical_group;
        let family_valid = evidence
            .evidence_family_group
            .as_ref()
            .is_none_or(|family| {
                family == family.trim()
                    && CanonicalRecallEvidenceFamilyGroup::from_canonical(family.clone()).is_some()
            });
        if !canonical_group_valid || !family_valid {
            return Err(Error::config(
                "governed_long_term_owner_evidence_binding",
                "long-term owner contains an invalid canonical evidence binding",
            ));
        }
        if let Some(family) = evidence.evidence_family_group.as_ref() {
            explicit_families_by_group
                .entry(canonical_group.to_string())
                .or_default()
                .insert(family.clone());
        }
        bindings_by_safe_ref.insert(
            safe_ref.to_string(),
            GovernedEvidenceBinding::new(
                safe_ref.to_string(),
                canonical_group.to_string(),
                evidence.evidence_family_group.clone(),
            ),
        );
    }
    if explicit_families_by_group
        .values()
        .any(|families| families.len() > 1)
    {
        return Err(Error::config(
            "governed_long_term_owner_evidence_binding",
            "one canonical evidence group maps to multiple evidence families",
        ));
    }
    for binding in bindings_by_safe_ref.values_mut() {
        if binding.evidence_family_group.is_none() {
            let family = explicit_families_by_group
                .get(&binding.canonical_evidence_group)
                .and_then(|families| families.iter().next())
                .cloned();
            binding.evidence_family_group = family;
        }
    }
    Ok(bindings_by_safe_ref.into_values().collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GovernedLexicalSegmentKind {
    Body,
    Chunk,
}

/// One digest-bound segment of the lexical content used by facet, scoring and rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedLexicalSegment<'a> {
    kind: GovernedLexicalSegmentKind,
    identity: &'a str,
    ordinal: u32,
    text: &'a str,
}

impl<'a> GovernedLexicalSegment<'a> {
    pub(crate) fn new(
        kind: GovernedLexicalSegmentKind,
        identity: &'a str,
        ordinal: u32,
        text: &'a str,
    ) -> Self {
        Self {
            kind,
            identity,
            ordinal,
            text,
        }
    }

    pub const fn kind(&self) -> GovernedLexicalSegmentKind {
        self.kind
    }

    pub fn identity(&self) -> &str {
        self.identity
    }

    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    pub fn text(&self) -> &str {
        self.text
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedLexicalContent<'a> {
    content_digest: &'a str,
    segments: Vec<GovernedLexicalSegment<'a>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedLexicalExcerpt {
    content_digest: String,
    segment_kind: GovernedLexicalSegmentKind,
    segment_identity: String,
    segment_ordinal: u32,
    text: String,
}

impl GovernedLexicalExcerpt {
    pub(crate) fn new(
        content_digest: String,
        segment_kind: GovernedLexicalSegmentKind,
        segment_identity: String,
        segment_ordinal: u32,
        text: String,
    ) -> Self {
        Self {
            content_digest,
            segment_kind,
            segment_identity,
            segment_ordinal,
            text,
        }
    }

    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }

    pub const fn segment_kind(&self) -> GovernedLexicalSegmentKind {
        self.segment_kind
    }

    pub fn segment_identity(&self) -> &str {
        &self.segment_identity
    }

    pub const fn segment_ordinal(&self) -> u32 {
        self.segment_ordinal
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl<'a> GovernedLexicalContent<'a> {
    pub(crate) fn new(content_digest: &'a str, segments: Vec<GovernedLexicalSegment<'a>>) -> Self {
        Self {
            content_digest,
            segments,
        }
    }

    pub fn content_digest(&self) -> &str {
        self.content_digest
    }

    pub fn segments(&self) -> &[GovernedLexicalSegment<'a>] {
        &self.segments
    }
}

/// Single immutable material consumed by recall stages for one governed owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GovernedMemoryOwnerMaterial<'a> {
    owner_ref: GovernedMemoryOwnerRef,
    evidence_bindings: Vec<GovernedEvidenceBinding>,
    lexical_content: GovernedLexicalContent<'a>,
}

impl<'a> GovernedMemoryOwnerMaterial<'a> {
    pub(crate) fn new(
        owner_ref: GovernedMemoryOwnerRef,
        evidence_bindings: Vec<GovernedEvidenceBinding>,
        lexical_content: GovernedLexicalContent<'a>,
    ) -> Self {
        Self {
            owner_ref,
            evidence_bindings,
            lexical_content,
        }
    }

    pub fn owner_ref(&self) -> &GovernedMemoryOwnerRef {
        &self.owner_ref
    }

    pub fn evidence_bindings(&self) -> &[GovernedEvidenceBinding] {
        &self.evidence_bindings
    }

    pub fn lexical_content(&self) -> &GovernedLexicalContent<'a> {
        &self.lexical_content
    }
}

impl Default for GovernedMemoryOwnerRef {
    fn default() -> Self {
        Self {
            owner_plane: GovernedMemoryOwnerPlane::LongTerm,
            owner_id: String::new(),
        }
    }
}

impl GovernedMemoryOwnerRef {
    pub fn new(owner_plane: GovernedMemoryOwnerPlane, owner_id: impl Into<String>) -> Self {
        Self {
            owner_plane,
            owner_id: owner_id.into(),
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.owner_id.is_empty() && self.owner_id == self.owner_id.trim()
    }
}

pub fn governed_memory_recall_candidate_id(owner_ref: &GovernedMemoryOwnerRef) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        GOVERNED_MEMORY_RECALL_CANDIDATE_DOMAIN.as_bytes(),
    );
    hash_field(&mut hasher, owner_ref.owner_plane.as_str().as_bytes());
    hash_field(&mut hasher, owner_ref.owner_id.as_bytes());
    format!(
        "owner:{}:{:x}",
        owner_ref.owner_plane.as_str(),
        hasher.finalize()
    )
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}
