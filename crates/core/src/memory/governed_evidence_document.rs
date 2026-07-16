//! Governed projection-visible evidence-document schema and revision planner.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use super::governed_memory_owner::{
    GovernedEvidenceBinding, GovernedLexicalContent, GovernedLexicalExcerpt,
    GovernedLexicalSegment, GovernedLexicalSegmentKind, GovernedMemoryOwnerMaterial,
};
use super::{
    canonical_recall_evidence_group, score_recall_delivery_texts,
    CanonicalRecallEvidenceFamilyGroup, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    MemoryEvidenceAuthority, MemoryPrivacyClass, RecallDeliveryText,
};

pub const MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES: usize = 256 * 1024;
pub const MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES: usize = 256 * 1024;
pub const MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES: usize = 32 * 1024;
pub const MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS: usize = 64;
pub const GOVERNED_EVIDENCE_DOCUMENT_SCHEMA_VERSION: u32 = 3;

const EVIDENCE_DOCUMENT_KEY_DOMAIN: &str = "governed_evidence_document_key_v1";
const EVIDENCE_DOCUMENT_CONTENT_DOMAIN: &str = "governed_evidence_document_content_v2";
const EVIDENCE_SOURCE_REF_KEY_DOMAIN: &str = "governed_evidence_source_ref_key_v2";
const EVIDENCE_SOURCE_LOCATOR_DIGEST_DOMAIN: &str = "governed_evidence_source_locator_digest_v1";
pub const GOVERNED_EVIDENCE_SOURCE_REF_SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GovernedEvidenceDocumentSourceKind {
    ConversationTranscript,
    File,
    Url,
    Api,
    Archive,
    ExternalContent,
    StructuredMaterial,
}

impl GovernedEvidenceDocumentSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConversationTranscript => "conversation_transcript",
            Self::File => "file",
            Self::Url => "url",
            Self::Api => "api",
            Self::Archive => "archive",
            Self::ExternalContent => "external_content",
            Self::StructuredMaterial => "structured_material",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct GovernedEvidenceDocumentChunk {
    pub identity: String,
    pub ordinal: u32,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GovernedEvidenceDocumentDraft {
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub document_id: String,
    pub source_kind: GovernedEvidenceDocumentSourceKind,
    pub source_locator: String,
    pub canonical_evidence_group: String,
    pub evidence_family_group: Option<String>,
    pub source_revision: u64,
    pub body: String,
    pub chunks: Vec<GovernedEvidenceDocumentChunk>,
    pub content_digest: String,
    pub authority: MemoryEvidenceAuthority,
    pub privacy: MemoryPrivacyClass,
    pub observed_at: u64,
}

impl GovernedEvidenceDocumentDraft {
    pub const MAX_MEMORY_SPACE_ID_BYTES: usize = 256;
    pub const MAX_MOUNTED_SUBJECT_ID_BYTES: usize = 256;
    pub const MAX_DOCUMENT_ID_BYTES: usize = 1024;
    pub const MAX_SOURCE_LOCATOR_BYTES: usize = 8 * 1024;
    pub const MAX_CANONICAL_EVIDENCE_GROUP_BYTES: usize = 1024;
    pub const MAX_EVIDENCE_FAMILY_GROUP_BYTES: usize = 1024;
    pub const MAX_CHUNK_IDENTITY_BYTES: usize = 512;
    pub const CONTENT_DIGEST_BYTES: usize = 64;

    /// Adds the UTF-8 bytes of every caller-controlled persisted string to `accumulated`.
    pub fn checked_caller_controlled_persisted_bytes(&self, accumulated: usize) -> Option<usize> {
        let top_level_fields = [
            self.memory_space_id.as_str(),
            self.mounted_subject_id.as_str(),
            self.document_id.as_str(),
            self.source_locator.as_str(),
            self.canonical_evidence_group.as_str(),
            self.body.as_str(),
            self.content_digest.as_str(),
        ];
        let total = top_level_fields
            .iter()
            .try_fold(accumulated, |total, field| total.checked_add(field.len()))?;
        let total = self
            .evidence_family_group
            .as_deref()
            .map_or(Some(total), |family| total.checked_add(family.len()))?;
        self.chunks.iter().try_fold(total, |total, chunk| {
            total
                .checked_add(chunk.identity.len())?
                .checked_add(chunk.body.len())
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GovernedEvidenceDocument {
    pub schema_version: u32,
    pub physical_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub document_id: String,
    pub source_kind: GovernedEvidenceDocumentSourceKind,
    pub source_locator: String,
    pub canonical_evidence_group: String,
    pub evidence_family_group: Option<String>,
    pub source_revision: u64,
    pub owner_revision: u64,
    pub body: String,
    pub chunks: Vec<GovernedEvidenceDocumentChunk>,
    pub content_digest: String,
    pub authority: MemoryEvidenceAuthority,
    pub privacy: MemoryPrivacyClass,
    pub observed_at: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GovernedEvidenceSourceRef {
    pub schema_version: u32,
    pub physical_key: String,
    pub owner_ref: GovernedMemoryOwnerRef,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub source_kind: GovernedEvidenceDocumentSourceKind,
    pub source_locator_digest: String,
    pub canonical_evidence_group: String,
    pub evidence_family_group: Option<String>,
    pub source_revision: u64,
    pub owner_revision: u64,
    pub content_digest: String,
    pub observed_at: u64,
}

impl GovernedEvidenceDocument {
    pub const fn shared_fact_surface_allowed(&self) -> bool {
        false
    }

    pub fn owner_material(&self) -> Result<GovernedMemoryOwnerMaterial<'_>> {
        validate_governed_evidence_document(self).map_err(|rejection| {
            Error::config(
                "governed_evidence_owner_material",
                format!("invalid governed evidence owner: {rejection:?}"),
            )
        })?;
        let source_ref = governed_evidence_source_ref_from_document(self)?;
        let owner_ref = source_ref.owner_ref.clone();
        let evidence_binding = GovernedEvidenceBinding::new(
            source_ref.physical_key,
            source_ref.canonical_evidence_group,
            source_ref.evidence_family_group,
        );
        let mut segments = Vec::with_capacity(self.chunks.len().saturating_add(1));
        segments.push(GovernedLexicalSegment::new(
            GovernedLexicalSegmentKind::Body,
            "body",
            0,
            &self.body,
        ));
        segments.extend(self.chunks.iter().map(|chunk| {
            GovernedLexicalSegment::new(
                GovernedLexicalSegmentKind::Chunk,
                &chunk.identity,
                chunk.ordinal.saturating_add(1),
                &chunk.body,
            )
        }));
        Ok(GovernedMemoryOwnerMaterial::new(
            owner_ref,
            vec![evidence_binding],
            GovernedLexicalContent::new(&self.content_digest, segments),
        ))
    }

    pub fn select_lexical_excerpt(
        &self,
        query: &str,
        max_chars: usize,
    ) -> Result<Option<GovernedLexicalExcerpt>> {
        if max_chars == 0 {
            return Ok(None);
        }
        let material = self.owner_material()?;
        let segments = material.lexical_content().segments();
        let documents = segments
            .iter()
            .map(|segment| RecallDeliveryText {
                candidate_id: segment.identity(),
                text: segment.text(),
            })
            .collect::<Vec<_>>();
        let scores = score_recall_delivery_texts(query, &documents);
        let selected_index = scores
            .iter()
            .enumerate()
            .max_by(|(left_index, left), (right_index, right)| {
                left.score
                    .cmp(&right.score)
                    .then_with(|| right_index.cmp(left_index))
            })
            .map(|(index, _)| index)
            .unwrap_or(0);
        let segment = &segments[selected_index];
        let text = segment.text().chars().take(max_chars).collect::<String>();
        Ok(Some(GovernedLexicalExcerpt::new(
            material.lexical_content().content_digest().to_string(),
            segment.kind(),
            segment.identity().to_string(),
            segment.ordinal(),
            text,
        )))
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GovernedEvidenceDocumentRejection {
    InvalidSchemaVersion,
    EmptyMemorySpaceId,
    EmptyMountedSubjectId,
    EmptyDocumentId,
    EmptySourceLocator,
    EmptyCanonicalEvidenceGroup,
    InvalidCanonicalEvidenceGroup,
    InvalidEvidenceFamilyGroup,
    NonCanonicalIdentity,
    MemorySpaceIdTooLarge,
    MountedSubjectIdTooLarge,
    DocumentIdTooLarge,
    SourceLocatorTooLarge,
    CanonicalEvidenceGroupTooLarge,
    EvidenceFamilyGroupTooLarge,
    EmptyBody,
    EmptyChunkIdentity,
    ChunkIdentityTooLarge,
    EmptyChunkBody,
    InvalidChunkOrdinal,
    DuplicateChunkIdentity,
    InvalidSourceRevision,
    InvalidOwnerRevision,
    InvalidAuthority,
    PrivacyNotProjectionVisible,
    InvalidObservedAt,
    InvalidTimestamps,
    BodyTooLarge,
    ChunkTooLarge,
    TooManyChunks,
    DocumentTooLarge,
    InvalidContentDigest,
    DigestMismatch,
    PhysicalKeyMismatch,
    IdentityMismatch,
    MountedSubjectMismatch,
    SourceLineageMismatch,
    OlderSourceRevision,
    SourceRevisionConflict,
    OwnerRevisionOverflow,
    TimestampOverflow,
    InvalidExistingDocument,
    OwnerDocumentMissing,
    OwnerRevisionConflict,
    SourceRefMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GovernedEvidenceDocumentPlan {
    Created(GovernedEvidenceDocument),
    Updated(GovernedEvidenceDocument),
    Noop,
    Rejected(GovernedEvidenceDocumentRejection),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GovernedEvidenceDocumentDeletePlan {
    Deleted,
    Rejected(GovernedEvidenceDocumentRejection),
}

impl GovernedEvidenceDocumentPlan {
    pub fn changed_document(&self) -> Option<&GovernedEvidenceDocument> {
        match self {
            Self::Created(document) | Self::Updated(document) => Some(document),
            Self::Noop | Self::Rejected(_) => None,
        }
    }
}

pub fn scoped_governed_evidence_document_key(
    memory_space_id: &str,
    document_id: &str,
) -> Result<String> {
    let memory_space_id = memory_space_id.trim();
    let document_id = document_id.trim();
    if memory_space_id.is_empty() || document_id.is_empty() {
        return Err(Error::config(
            "governed_evidence_document_scope",
            "memory_space_id and document_id must not be empty",
        ));
    }
    let digest = domain_separated_sha256(
        EVIDENCE_DOCUMENT_KEY_DOMAIN,
        &[memory_space_id, document_id],
    );
    Ok(format!("scope:{digest}:evidence_document"))
}

pub fn governed_evidence_source_locator_digest(source_locator: &str) -> String {
    domain_separated_sha256(EVIDENCE_SOURCE_LOCATOR_DIGEST_DOMAIN, &[source_locator])
}

pub fn scoped_governed_evidence_source_ref_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
    source_kind: GovernedEvidenceDocumentSourceKind,
    source_locator: &str,
    source_revision: u64,
) -> Result<String> {
    let memory_space_id = memory_space_id.trim();
    let mounted_subject_id = mounted_subject_id.trim();
    if memory_space_id.is_empty()
        || mounted_subject_id.is_empty()
        || source_locator.trim().is_empty()
        || source_revision == 0
    {
        return Err(Error::config(
            "governed_evidence_source_ref_scope",
            "evidence source ref requires valid source identity scope",
        ));
    }
    let digest = domain_separated_sha256(
        EVIDENCE_SOURCE_REF_KEY_DOMAIN,
        &[
            memory_space_id,
            mounted_subject_id,
            source_kind.as_str(),
            source_locator,
            &source_revision.to_string(),
        ],
    );
    Ok(format!("scope:{digest}:evidence_source_ref"))
}

pub fn governed_evidence_source_ref_from_document(
    document: &GovernedEvidenceDocument,
) -> Result<GovernedEvidenceSourceRef> {
    let owner_ref = GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::EvidenceDocument,
        document.document_id.clone(),
    );
    let physical_key = scoped_governed_evidence_source_ref_key(
        &document.memory_space_id,
        &document.mounted_subject_id,
        document.source_kind,
        &document.source_locator,
        document.source_revision,
    )?;
    Ok(GovernedEvidenceSourceRef {
        schema_version: GOVERNED_EVIDENCE_SOURCE_REF_SCHEMA_VERSION,
        physical_key,
        owner_ref,
        memory_space_id: document.memory_space_id.clone(),
        mounted_subject_id: document.mounted_subject_id.clone(),
        source_kind: document.source_kind,
        source_locator_digest: governed_evidence_source_locator_digest(&document.source_locator),
        canonical_evidence_group: document.canonical_evidence_group.clone(),
        evidence_family_group: document.evidence_family_group.clone(),
        source_revision: document.source_revision,
        owner_revision: document.owner_revision,
        content_digest: document.content_digest.clone(),
        observed_at: document.observed_at,
    })
}

pub fn validate_governed_evidence_source_ref(
    document: &GovernedEvidenceDocument,
    source_ref: &GovernedEvidenceSourceRef,
) -> std::result::Result<(), GovernedEvidenceDocumentRejection> {
    validate_governed_evidence_document(document)?;
    let expected = governed_evidence_source_ref_from_document(document)
        .map_err(|_| GovernedEvidenceDocumentRejection::SourceRefMismatch)?;
    if source_ref != &expected || source_ref.physical_key.is_empty() {
        return Err(GovernedEvidenceDocumentRejection::SourceRefMismatch);
    }
    Ok(())
}

pub fn governed_evidence_document_content_digest(
    source_locator: &str,
    canonical_evidence_group: &str,
    evidence_family_group: Option<&str>,
    body: &str,
    chunks: &[GovernedEvidenceDocumentChunk],
) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, EVIDENCE_DOCUMENT_CONTENT_DOMAIN.as_bytes());
    hash_field(&mut hasher, source_locator.as_bytes());
    hash_field(&mut hasher, canonical_evidence_group.as_bytes());
    match evidence_family_group {
        Some(family) => {
            hasher.update([1]);
            hash_field(&mut hasher, family.as_bytes());
        }
        None => hasher.update([0]),
    }
    hash_field(&mut hasher, body.as_bytes());
    hasher.update((chunks.len() as u64).to_be_bytes());
    for chunk in chunks {
        hasher.update(chunk.ordinal.to_be_bytes());
        hash_field(&mut hasher, chunk.identity.as_bytes());
        hash_field(&mut hasher, chunk.body.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

pub fn validate_governed_evidence_document_draft(
    draft: &GovernedEvidenceDocumentDraft,
) -> std::result::Result<(), GovernedEvidenceDocumentRejection> {
    validate_content(draft)
}

pub fn validate_governed_evidence_document(
    document: &GovernedEvidenceDocument,
) -> std::result::Result<(), GovernedEvidenceDocumentRejection> {
    if document.schema_version != GOVERNED_EVIDENCE_DOCUMENT_SCHEMA_VERSION {
        return Err(GovernedEvidenceDocumentRejection::InvalidSchemaVersion);
    }
    if document.owner_revision == 0 {
        return Err(GovernedEvidenceDocumentRejection::InvalidOwnerRevision);
    }
    let draft = draft_from_document(document);
    validate_content(&draft)?;
    if document.created_at == 0
        || document.updated_at < document.created_at
        || document.updated_at < document.observed_at
    {
        return Err(GovernedEvidenceDocumentRejection::InvalidTimestamps);
    }
    let expected_key =
        scoped_governed_evidence_document_key(&document.memory_space_id, &document.document_id)
            .map_err(|_| GovernedEvidenceDocumentRejection::PhysicalKeyMismatch)?;
    if document.physical_key != expected_key {
        return Err(GovernedEvidenceDocumentRejection::PhysicalKeyMismatch);
    }
    Ok(())
}

pub fn plan_governed_evidence_document_upsert(
    existing: Option<&GovernedEvidenceDocument>,
    draft: &GovernedEvidenceDocumentDraft,
    now_secs: u64,
) -> GovernedEvidenceDocumentPlan {
    if let Err(rejection) = validate_governed_evidence_document_draft(draft) {
        return GovernedEvidenceDocumentPlan::Rejected(rejection);
    }
    let physical_key =
        scoped_governed_evidence_document_key(&draft.memory_space_id, &draft.document_id)
            .expect("validated evidence document owner identity");

    let Some(existing) = existing else {
        let updated_at = now_secs.max(draft.observed_at);
        return GovernedEvidenceDocumentPlan::Created(document_from_draft(
            draft,
            physical_key,
            1,
            updated_at,
            updated_at,
        ));
    };
    if validate_governed_evidence_document(existing).is_err() {
        return GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::InvalidExistingDocument,
        );
    }
    if existing.physical_key != physical_key {
        return GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::IdentityMismatch,
        );
    }
    if existing.mounted_subject_id != draft.mounted_subject_id {
        return GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::MountedSubjectMismatch,
        );
    }
    if draft.source_revision < existing.source_revision {
        return GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::OlderSourceRevision,
        );
    }
    if draft.source_revision == existing.source_revision {
        return if same_source_payload(existing, draft) {
            GovernedEvidenceDocumentPlan::Noop
        } else {
            GovernedEvidenceDocumentPlan::Rejected(
                GovernedEvidenceDocumentRejection::SourceRevisionConflict,
            )
        };
    }
    if existing.source_kind != draft.source_kind || existing.source_locator != draft.source_locator
    {
        return GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::SourceLineageMismatch,
        );
    }
    let Some(owner_revision) = existing.owner_revision.checked_add(1) else {
        return GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::OwnerRevisionOverflow,
        );
    };
    let Some(owner_successor_time) = existing.updated_at.checked_add(1) else {
        return GovernedEvidenceDocumentPlan::Rejected(
            GovernedEvidenceDocumentRejection::TimestampOverflow,
        );
    };
    let updated_at = now_secs.max(draft.observed_at).max(owner_successor_time);
    GovernedEvidenceDocumentPlan::Updated(document_from_draft(
        draft,
        physical_key,
        owner_revision,
        existing.created_at,
        updated_at,
    ))
}

pub fn plan_governed_evidence_document_delete(
    existing: Option<&GovernedEvidenceDocument>,
    expected_owner_revision: u64,
) -> GovernedEvidenceDocumentDeletePlan {
    if expected_owner_revision == 0 {
        return GovernedEvidenceDocumentDeletePlan::Rejected(
            GovernedEvidenceDocumentRejection::InvalidOwnerRevision,
        );
    }
    let Some(existing) = existing else {
        return GovernedEvidenceDocumentDeletePlan::Rejected(
            GovernedEvidenceDocumentRejection::OwnerDocumentMissing,
        );
    };
    if validate_governed_evidence_document(existing).is_err() {
        return GovernedEvidenceDocumentDeletePlan::Rejected(
            GovernedEvidenceDocumentRejection::InvalidExistingDocument,
        );
    }
    if existing.owner_revision != expected_owner_revision {
        return GovernedEvidenceDocumentDeletePlan::Rejected(
            GovernedEvidenceDocumentRejection::OwnerRevisionConflict,
        );
    }
    GovernedEvidenceDocumentDeletePlan::Deleted
}

pub trait GovernedEvidenceDocumentReadStore: Send + Sync {
    fn get(&self, physical_key: &str) -> Result<Option<GovernedEvidenceDocument>>;
}

fn validate_content(
    draft: &GovernedEvidenceDocumentDraft,
) -> std::result::Result<(), GovernedEvidenceDocumentRejection> {
    if draft.memory_space_id.trim().is_empty() {
        return Err(GovernedEvidenceDocumentRejection::EmptyMemorySpaceId);
    }
    if draft.mounted_subject_id.trim().is_empty() {
        return Err(GovernedEvidenceDocumentRejection::EmptyMountedSubjectId);
    }
    if draft.document_id.trim().is_empty() {
        return Err(GovernedEvidenceDocumentRejection::EmptyDocumentId);
    }
    if draft.memory_space_id.len() > GovernedEvidenceDocumentDraft::MAX_MEMORY_SPACE_ID_BYTES {
        return Err(GovernedEvidenceDocumentRejection::MemorySpaceIdTooLarge);
    }
    if draft.mounted_subject_id.len() > GovernedEvidenceDocumentDraft::MAX_MOUNTED_SUBJECT_ID_BYTES
    {
        return Err(GovernedEvidenceDocumentRejection::MountedSubjectIdTooLarge);
    }
    if draft.document_id.len() > GovernedEvidenceDocumentDraft::MAX_DOCUMENT_ID_BYTES {
        return Err(GovernedEvidenceDocumentRejection::DocumentIdTooLarge);
    }
    if draft.memory_space_id != draft.memory_space_id.trim()
        || draft.mounted_subject_id != draft.mounted_subject_id.trim()
        || draft.document_id != draft.document_id.trim()
    {
        return Err(GovernedEvidenceDocumentRejection::NonCanonicalIdentity);
    }
    if draft.source_locator.trim().is_empty() {
        return Err(GovernedEvidenceDocumentRejection::EmptySourceLocator);
    }
    if draft.source_locator.len() > GovernedEvidenceDocumentDraft::MAX_SOURCE_LOCATOR_BYTES {
        return Err(GovernedEvidenceDocumentRejection::SourceLocatorTooLarge);
    }
    if draft.canonical_evidence_group.trim().is_empty() {
        return Err(GovernedEvidenceDocumentRejection::EmptyCanonicalEvidenceGroup);
    }
    if draft.canonical_evidence_group.len()
        > GovernedEvidenceDocumentDraft::MAX_CANONICAL_EVIDENCE_GROUP_BYTES
    {
        return Err(GovernedEvidenceDocumentRejection::CanonicalEvidenceGroupTooLarge);
    }
    if canonical_recall_evidence_group(&draft.canonical_evidence_group)
        != draft.canonical_evidence_group
    {
        return Err(GovernedEvidenceDocumentRejection::InvalidCanonicalEvidenceGroup);
    }
    if let Some(family) = &draft.evidence_family_group {
        if family.len() > GovernedEvidenceDocumentDraft::MAX_EVIDENCE_FAMILY_GROUP_BYTES {
            return Err(GovernedEvidenceDocumentRejection::EvidenceFamilyGroupTooLarge);
        }
        if CanonicalRecallEvidenceFamilyGroup::from_canonical(family.clone()).is_none() {
            return Err(GovernedEvidenceDocumentRejection::InvalidEvidenceFamilyGroup);
        }
    }
    if draft.body.trim().is_empty() {
        return Err(GovernedEvidenceDocumentRejection::EmptyBody);
    }
    if draft.source_revision == 0 {
        return Err(GovernedEvidenceDocumentRejection::InvalidSourceRevision);
    }
    if draft.observed_at == 0 {
        return Err(GovernedEvidenceDocumentRejection::InvalidObservedAt);
    }
    if !draft.privacy.projection_content_allowed() {
        return Err(GovernedEvidenceDocumentRejection::PrivacyNotProjectionVisible);
    }
    if !authority_allowed(draft.authority) {
        return Err(GovernedEvidenceDocumentRejection::InvalidAuthority);
    }
    if draft.body.len() > MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES {
        return Err(GovernedEvidenceDocumentRejection::BodyTooLarge);
    }
    if draft.chunks.len() > MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS {
        return Err(GovernedEvidenceDocumentRejection::TooManyChunks);
    }
    if draft.content_digest.len() != GovernedEvidenceDocumentDraft::CONTENT_DIGEST_BYTES {
        return Err(GovernedEvidenceDocumentRejection::InvalidContentDigest);
    }
    let mut identities = BTreeSet::new();
    for (expected_ordinal, chunk) in draft.chunks.iter().enumerate() {
        if chunk.ordinal as usize != expected_ordinal {
            return Err(GovernedEvidenceDocumentRejection::InvalidChunkOrdinal);
        }
        if chunk.identity.trim().is_empty() {
            return Err(GovernedEvidenceDocumentRejection::EmptyChunkIdentity);
        }
        if chunk.identity.len() > GovernedEvidenceDocumentDraft::MAX_CHUNK_IDENTITY_BYTES {
            return Err(GovernedEvidenceDocumentRejection::ChunkIdentityTooLarge);
        }
        if !identities.insert(chunk.identity.as_str()) {
            return Err(GovernedEvidenceDocumentRejection::DuplicateChunkIdentity);
        }
        if chunk.body.trim().is_empty() {
            return Err(GovernedEvidenceDocumentRejection::EmptyChunkBody);
        }
        if chunk.body.len() > MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES {
            return Err(GovernedEvidenceDocumentRejection::ChunkTooLarge);
        }
    }
    if draft
        .checked_caller_controlled_persisted_bytes(0)
        .is_none_or(|document_bytes| document_bytes > MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES)
    {
        return Err(GovernedEvidenceDocumentRejection::DocumentTooLarge);
    }
    if draft.content_digest
        != governed_evidence_document_content_digest(
            &draft.source_locator,
            &draft.canonical_evidence_group,
            draft.evidence_family_group.as_deref(),
            &draft.body,
            &draft.chunks,
        )
    {
        return Err(GovernedEvidenceDocumentRejection::DigestMismatch);
    }
    Ok(())
}

fn authority_allowed(authority: MemoryEvidenceAuthority) -> bool {
    matches!(
        authority,
        MemoryEvidenceAuthority::UserAsserted
            | MemoryEvidenceAuthority::RuntimeObservation
            | MemoryEvidenceAuthority::WorldObservation
            | MemoryEvidenceAuthority::ArchiveEvidence
            | MemoryEvidenceAuthority::ExternalContent
            | MemoryEvidenceAuthority::LegacyTranscript
    )
}

fn same_source_payload(
    existing: &GovernedEvidenceDocument,
    draft: &GovernedEvidenceDocumentDraft,
) -> bool {
    existing.memory_space_id == draft.memory_space_id
        && existing.mounted_subject_id == draft.mounted_subject_id
        && existing.document_id == draft.document_id
        && existing.source_kind == draft.source_kind
        && existing.source_locator == draft.source_locator
        && existing.canonical_evidence_group == draft.canonical_evidence_group
        && existing.evidence_family_group == draft.evidence_family_group
        && existing.source_revision == draft.source_revision
        && existing.body == draft.body
        && existing.chunks == draft.chunks
        && existing.content_digest == draft.content_digest
        && existing.authority == draft.authority
        && existing.privacy == draft.privacy
        && existing.observed_at == draft.observed_at
}

fn document_from_draft(
    draft: &GovernedEvidenceDocumentDraft,
    physical_key: String,
    owner_revision: u64,
    created_at: u64,
    updated_at: u64,
) -> GovernedEvidenceDocument {
    GovernedEvidenceDocument {
        schema_version: GOVERNED_EVIDENCE_DOCUMENT_SCHEMA_VERSION,
        physical_key,
        memory_space_id: draft.memory_space_id.clone(),
        mounted_subject_id: draft.mounted_subject_id.clone(),
        document_id: draft.document_id.clone(),
        source_kind: draft.source_kind,
        source_locator: draft.source_locator.clone(),
        canonical_evidence_group: draft.canonical_evidence_group.clone(),
        evidence_family_group: draft.evidence_family_group.clone(),
        source_revision: draft.source_revision,
        owner_revision,
        body: draft.body.clone(),
        chunks: draft.chunks.clone(),
        content_digest: draft.content_digest.clone(),
        authority: draft.authority,
        privacy: draft.privacy,
        observed_at: draft.observed_at,
        created_at,
        updated_at,
    }
}

fn draft_from_document(document: &GovernedEvidenceDocument) -> GovernedEvidenceDocumentDraft {
    GovernedEvidenceDocumentDraft {
        memory_space_id: document.memory_space_id.clone(),
        mounted_subject_id: document.mounted_subject_id.clone(),
        document_id: document.document_id.clone(),
        source_kind: document.source_kind,
        source_locator: document.source_locator.clone(),
        canonical_evidence_group: document.canonical_evidence_group.clone(),
        evidence_family_group: document.evidence_family_group.clone(),
        source_revision: document.source_revision,
        body: document.body.clone(),
        chunks: document.chunks.clone(),
        content_digest: document.content_digest.clone(),
        authority: document.authority,
        privacy: document.privacy,
        observed_at: document.observed_at,
    }
}

fn domain_separated_sha256(domain: &str, fields: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, domain.as_bytes());
    for field in fields {
        hash_field(&mut hasher, field.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}
