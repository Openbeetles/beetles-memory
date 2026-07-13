//! Governed projection-visible evidence-document schema and revision planner.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use super::{MemoryEvidenceAuthority, MemoryPrivacyClass};

pub const MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES: usize = 256 * 1024;
pub const MAX_GOVERNED_EVIDENCE_DOCUMENT_BODY_BYTES: usize = 256 * 1024;
pub const MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNK_BYTES: usize = 32 * 1024;
pub const MAX_GOVERNED_EVIDENCE_DOCUMENT_CHUNKS: usize = 64;

const EVIDENCE_DOCUMENT_KEY_DOMAIN: &str = "governed_evidence_document_key_v1";
const EVIDENCE_DOCUMENT_CONTENT_DOMAIN: &str = "governed_evidence_document_content_v1";

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
    pub source_revision: u64,
    pub body: String,
    pub chunks: Vec<GovernedEvidenceDocumentChunk>,
    pub content_digest: String,
    pub authority: MemoryEvidenceAuthority,
    pub privacy: MemoryPrivacyClass,
    pub observed_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GovernedEvidenceDocument {
    pub physical_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub document_id: String,
    pub source_kind: GovernedEvidenceDocumentSourceKind,
    pub source_locator: String,
    pub canonical_evidence_group: String,
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

impl GovernedEvidenceDocument {
    pub const fn shared_fact_surface_allowed(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GovernedEvidenceDocumentRejection {
    EmptyMemorySpaceId,
    EmptyMountedSubjectId,
    EmptyDocumentId,
    EmptySourceLocator,
    EmptyCanonicalEvidenceGroup,
    NonCanonicalIdentity,
    EmptyBody,
    EmptyChunkIdentity,
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
    DigestMismatch,
    PhysicalKeyMismatch,
    IdentityMismatch,
    SourceLineageMismatch,
    OlderSourceRevision,
    SourceRevisionConflict,
    OwnerRevisionOverflow,
    TimestampOverflow,
    InvalidExistingDocument,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GovernedEvidenceDocumentPlan {
    Created(GovernedEvidenceDocument),
    Updated(GovernedEvidenceDocument),
    Noop,
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

pub fn governed_evidence_document_content_digest(
    source_locator: &str,
    canonical_evidence_group: &str,
    body: &str,
    chunks: &[GovernedEvidenceDocumentChunk],
) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, EVIDENCE_DOCUMENT_CONTENT_DOMAIN.as_bytes());
    hash_field(&mut hasher, source_locator.as_bytes());
    hash_field(&mut hasher, canonical_evidence_group.as_bytes());
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
    if draft.memory_space_id != draft.memory_space_id.trim()
        || draft.mounted_subject_id != draft.mounted_subject_id.trim()
        || draft.document_id != draft.document_id.trim()
    {
        return Err(GovernedEvidenceDocumentRejection::NonCanonicalIdentity);
    }
    if draft.source_locator.trim().is_empty() {
        return Err(GovernedEvidenceDocumentRejection::EmptySourceLocator);
    }
    if draft.canonical_evidence_group.trim().is_empty() {
        return Err(GovernedEvidenceDocumentRejection::EmptyCanonicalEvidenceGroup);
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
    let mut identities = BTreeSet::new();
    let mut document_bytes = draft.body.len();
    for (expected_ordinal, chunk) in draft.chunks.iter().enumerate() {
        if chunk.ordinal as usize != expected_ordinal {
            return Err(GovernedEvidenceDocumentRejection::InvalidChunkOrdinal);
        }
        if chunk.identity.trim().is_empty() {
            return Err(GovernedEvidenceDocumentRejection::EmptyChunkIdentity);
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
        document_bytes = document_bytes
            .saturating_add(chunk.identity.len())
            .saturating_add(chunk.body.len());
    }
    if document_bytes > MAX_GOVERNED_EVIDENCE_DOCUMENT_BYTES {
        return Err(GovernedEvidenceDocumentRejection::DocumentTooLarge);
    }
    if draft.content_digest
        != governed_evidence_document_content_digest(
            &draft.source_locator,
            &draft.canonical_evidence_group,
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
        physical_key,
        memory_space_id: draft.memory_space_id.clone(),
        mounted_subject_id: draft.mounted_subject_id.clone(),
        document_id: draft.document_id.clone(),
        source_kind: draft.source_kind,
        source_locator: draft.source_locator.clone(),
        canonical_evidence_group: draft.canonical_evidence_group.clone(),
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
