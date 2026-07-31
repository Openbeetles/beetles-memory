use std::collections::BTreeSet;

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    long_term_version_head_key, long_term_version_material_key,
    long_term_version_scope_manifest_key, memory_facet_manifest_key, memory_graph_scope_digest,
    memory_graph_scope_manifest_key, scoped_governed_evidence_document_key,
    scoped_long_term_control_storage_key, scoped_memory_facet_owner_storage_key,
    validate_governed_evidence_document, validate_memory_graph_scope_manifest, ControlEffectRef,
    EvidenceBacklink, GovernedEvidenceDocument, GovernedEvidenceSourceRef,
    LongTermMemoryControlAuditEvent, LongTermMemoryControlRevision, LongTermMemoryHeadManifest,
    LongTermMemoryTombstone, LongTermMemoryVersionMaterial, LongTermMemoryVersionScopeManifest,
    MemoryFacetIndexDoc, MemoryFacetIndexManifest, MemoryFacetPostingDoc,
    MemoryGraphBacklinkMembership, MemoryGraphEdge, MemoryGraphEdgeMembership, MemoryGraphNode,
    MemoryGraphNodeMembership, MemoryGraphRecallIndexDoc, MemoryGraphRevisionDoc,
    MemoryGraphScopeManifest, MemoryLongTermGovernancePolicy,
    GOVERNED_EVIDENCE_DOCUMENT_SCHEMA_VERSION, GOVERNED_EVIDENCE_SOURCE_REF_SCHEMA_VERSION,
    LONG_TERM_CONTROL_AUDIT_NAMESPACE, LONG_TERM_CONTROL_REVISION_NAMESPACE,
    LONG_TERM_CONTROL_SCHEMA_VERSION, LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
    LONG_TERM_GOVERNANCE_POLICY_NAMESPACE, LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
    MEMORY_FACET_INDEX_NAMESPACE, MEMORY_FACET_POSTING_NAMESPACE,
    MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE, MEMORY_GRAPH_BACKLINK_NAMESPACE,
    MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE, MEMORY_GRAPH_EDGE_NAMESPACE,
    MEMORY_GRAPH_INDEX_NAMESPACE, MEMORY_GRAPH_MANIFEST_NAMESPACE,
    MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE, MEMORY_GRAPH_NODE_NAMESPACE,
    MEMORY_GRAPH_REVISION_NAMESPACE, MEMORY_GRAPH_SCHEMA_VERSION,
};
use bm_core::platform::MemorySystemKind;
use bm_core::skills::{
    canonical_runtime_skill_owner_key, runtime_skill_scope_manifest_key, RuntimeSkillOwnerRecord,
    RuntimeSkillOwningScope, RuntimeSkillScopeManifest,
    RUNTIME_SKILL_SCOPE_MANIFEST_SCHEMA_VERSION,
};
use bm_core::{Error, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::StorePhysicalOwningScope;
use sha2::{Digest, Sha256};

use crate::store_internal::config::{profile_memory_system_kind, StoreBackendKind};
use crate::store_internal::recall_index::{
    decode_typed_recall_index, ActiveTaskRunByChatIndex, ArchiveRecallManifest,
    ContinuityCapsuleScopeIndex, ConversationRecallManifest, ConversationTranscriptAuxManifest,
    ConversationTranscriptPageIndex, TaskLearningByChatIndex, TypedRecallIndex,
    ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE, ARCHIVE_RECALL_MANIFEST_NAMESPACE,
    CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE, CONVERSATION_RECALL_MANIFEST_NAMESPACE,
    CONVERSATION_TRANSCRIPT_AUX_MANIFEST_NAMESPACE, CONVERSATION_TRANSCRIPT_PAGE_NAMESPACE,
    TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
};

pub const STORE_SCHEMA_ID: &str = "beetle_memory_store_schema_v6";
pub const STORE_SCHEMA_VERSION: u32 = 6;
pub(crate) const LONG_TERM_VERSION_MATERIAL_NAMESPACE: &str = "long_term_version_materials";
pub(crate) const LONG_TERM_HEAD_MANIFEST_NAMESPACE: &str = "long_term_head_manifests";
pub(crate) const LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE: &str =
    "long_term_version_scope_manifests";
pub(crate) const RUNTIME_SKILL_RECORD_NAMESPACE: &str = "runtime_skill_records";
pub(crate) const RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE: &str = "runtime_skill_scope_manifests";
pub(crate) const LEGACY_LONG_TERM_OWNER_NAMESPACE: &str = "long_term";
pub(crate) const LEGACY_RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE: &str =
    "runtime_skill_recall_manifests";
pub(crate) const GENERIC_SKILL_BLOB_NAMESPACE: &str = "skills";

const LEGACY_RUNTIME_SKILL_KEY_PREFIX: &str = "runtime_skill__";
const LEGACY_RUNTIME_SKILL_CONTENT_MARKER: &str = "<!-- beetle:runtime-skill -->";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreJsonDecoderKind {
    OpaqueExisting,
    GenericSkillMeta,
    MemoryFacetIndex,
    MemoryFacetPosting,
    MemoryGraphNode,
    MemoryGraphEdge,
    MemoryGraphBacklink,
    MemoryGraphRecallIndex,
    MemoryGraphRevision,
    MemoryGraphManifest,
    MemoryGraphNodeMembership,
    MemoryGraphEdgeMembership,
    MemoryGraphBacklinkMembership,
    LongTermControlRevision,
    LongTermControlTombstone,
    LongTermGovernancePolicy,
    LongTermControlAudit,
    ControlPlaneScopeManifest,
    LongTermVersionMaterial,
    LongTermHeadManifest,
    LongTermVersionScopeManifest,
    GovernedEvidenceDocument,
    GovernedEvidenceSourceRef,
    GovernedEvidenceSourceClaimManifest,
    RuntimeSkillOwnerRecord,
    RuntimeSkillScopeManifest,
    ConversationRecallManifest,
    ConversationTranscriptPageIndex,
    ConversationTranscriptAuxManifest,
    ArchiveRecallManifest,
    ContinuityCapsuleScopeIndex,
    ActiveTaskRunByChatIndex,
    TaskLearningByChatIndex,
    RecallOwnerScopeBinding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreBlobDecoderKind {
    OpaqueExisting,
    GenericSkill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreLegacyAddressKind {
    LongTermOwner,
    RuntimeSkillRecallManifest,
    RuntimeSkillBlobKeyPrefix,
    RuntimeSkillBlobMarker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreAddressAdmission<T> {
    Active(T),
    ForbiddenLegacy(StoreLegacyAddressKind),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StoreJsonNamespaceContract {
    pub namespace: &'static str,
    pub decoder_kind: StoreJsonDecoderKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StoreBlobNamespaceContract {
    pub namespace: &'static str,
    pub decoder_kind: StoreBlobDecoderKind,
}

pub(crate) const STORE_V6_JSON_NAMESPACE_REGISTRY: &[StoreJsonNamespaceContract] = &[
    StoreJsonNamespaceContract {
        namespace: "skill_meta",
        decoder_kind: StoreJsonDecoderKind::GenericSkillMeta,
    },
    opaque_json_namespace("active_work"),
    opaque_json_namespace("execution_state"),
    opaque_json_namespace("long_term_extraction_state"),
    opaque_json_namespace("turn_ledger"),
    opaque_json_namespace("self_model"),
    opaque_json_namespace("self_authored_core"),
    opaque_json_namespace("core_revision_ledger"),
    opaque_json_namespace("self_continuity"),
    opaque_json_namespace("relationship_constitution"),
    opaque_json_namespace("relationship_portfolio"),
    opaque_json_namespace("relationship_topology"),
    opaque_json_namespace("world_sense"),
    opaque_json_namespace("outer_voice"),
    opaque_json_namespace("autonomy_strategy"),
    opaque_json_namespace("inner_life"),
    opaque_json_namespace("felt_significance"),
    opaque_json_namespace("temperament_continuity"),
    opaque_json_namespace("inner_conflict"),
    opaque_json_namespace("mental_privacy"),
    opaque_json_namespace("private_doc"),
    opaque_json_namespace("conversation_transcript"),
    opaque_json_namespace("conversation_transcript_alias"),
    opaque_json_namespace("conversation_transcript_attr"),
    opaque_json_namespace("conversation_transcript_derived_ref"),
    typed_json_namespace(
        MEMORY_GRAPH_NODE_NAMESPACE,
        StoreJsonDecoderKind::MemoryGraphNode,
    ),
    typed_json_namespace(
        MEMORY_GRAPH_EDGE_NAMESPACE,
        StoreJsonDecoderKind::MemoryGraphEdge,
    ),
    typed_json_namespace(
        MEMORY_GRAPH_BACKLINK_NAMESPACE,
        StoreJsonDecoderKind::MemoryGraphBacklink,
    ),
    typed_json_namespace(
        MEMORY_GRAPH_INDEX_NAMESPACE,
        StoreJsonDecoderKind::MemoryGraphRecallIndex,
    ),
    typed_json_namespace(
        MEMORY_GRAPH_REVISION_NAMESPACE,
        StoreJsonDecoderKind::MemoryGraphRevision,
    ),
    typed_json_namespace(
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        StoreJsonDecoderKind::MemoryGraphManifest,
    ),
    typed_json_namespace(
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        StoreJsonDecoderKind::MemoryGraphNodeMembership,
    ),
    typed_json_namespace(
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        StoreJsonDecoderKind::MemoryGraphEdgeMembership,
    ),
    typed_json_namespace(
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        StoreJsonDecoderKind::MemoryGraphBacklinkMembership,
    ),
    typed_json_namespace(
        MEMORY_FACET_INDEX_NAMESPACE,
        StoreJsonDecoderKind::MemoryFacetIndex,
    ),
    typed_json_namespace(
        MEMORY_FACET_POSTING_NAMESPACE,
        StoreJsonDecoderKind::MemoryFacetPosting,
    ),
    typed_json_namespace(
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        StoreJsonDecoderKind::LongTermControlRevision,
    ),
    typed_json_namespace(
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        StoreJsonDecoderKind::LongTermControlTombstone,
    ),
    typed_json_namespace(
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        StoreJsonDecoderKind::LongTermGovernancePolicy,
    ),
    typed_json_namespace(
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        StoreJsonDecoderKind::LongTermControlAudit,
    ),
    typed_json_namespace(
        CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
        StoreJsonDecoderKind::ControlPlaneScopeManifest,
    ),
    opaque_json_namespace("session_summary"),
    opaque_json_namespace("session"),
    StoreJsonNamespaceContract {
        namespace: LONG_TERM_VERSION_MATERIAL_NAMESPACE,
        decoder_kind: StoreJsonDecoderKind::LongTermVersionMaterial,
    },
    StoreJsonNamespaceContract {
        namespace: LONG_TERM_HEAD_MANIFEST_NAMESPACE,
        decoder_kind: StoreJsonDecoderKind::LongTermHeadManifest,
    },
    StoreJsonNamespaceContract {
        namespace: LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
        decoder_kind: StoreJsonDecoderKind::LongTermVersionScopeManifest,
    },
    typed_json_namespace(
        "governed_evidence_documents",
        StoreJsonDecoderKind::GovernedEvidenceDocument,
    ),
    typed_json_namespace(
        "governed_evidence_source_refs",
        StoreJsonDecoderKind::GovernedEvidenceSourceRef,
    ),
    typed_json_namespace(
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
        StoreJsonDecoderKind::GovernedEvidenceSourceClaimManifest,
    ),
    opaque_json_namespace("continuity_capsule"),
    opaque_json_namespace("turn_continuity_evidence"),
    opaque_json_namespace("private_garden"),
    opaque_json_namespace("remind_at"),
    opaque_json_namespace("task"),
    opaque_json_namespace("task_run"),
    opaque_json_namespace("task_artifact"),
    opaque_json_namespace("task_learning"),
    StoreJsonNamespaceContract {
        namespace: RUNTIME_SKILL_RECORD_NAMESPACE,
        decoder_kind: StoreJsonDecoderKind::RuntimeSkillOwnerRecord,
    },
    StoreJsonNamespaceContract {
        namespace: RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
        decoder_kind: StoreJsonDecoderKind::RuntimeSkillScopeManifest,
    },
    typed_json_namespace(
        CONVERSATION_RECALL_MANIFEST_NAMESPACE,
        StoreJsonDecoderKind::ConversationRecallManifest,
    ),
    typed_json_namespace(
        CONVERSATION_TRANSCRIPT_PAGE_NAMESPACE,
        StoreJsonDecoderKind::ConversationTranscriptPageIndex,
    ),
    typed_json_namespace(
        CONVERSATION_TRANSCRIPT_AUX_MANIFEST_NAMESPACE,
        StoreJsonDecoderKind::ConversationTranscriptAuxManifest,
    ),
    typed_json_namespace(
        ARCHIVE_RECALL_MANIFEST_NAMESPACE,
        StoreJsonDecoderKind::ArchiveRecallManifest,
    ),
    typed_json_namespace(
        CONTINUITY_CAPSULE_SCOPE_INDEX_NAMESPACE,
        StoreJsonDecoderKind::ContinuityCapsuleScopeIndex,
    ),
    typed_json_namespace(
        ACTIVE_TASK_RUN_BY_CHAT_INDEX_NAMESPACE,
        StoreJsonDecoderKind::ActiveTaskRunByChatIndex,
    ),
    typed_json_namespace(
        TASK_LEARNING_BY_CHAT_INDEX_NAMESPACE,
        StoreJsonDecoderKind::TaskLearningByChatIndex,
    ),
    typed_json_namespace(
        RECALL_OWNER_SCOPE_BINDING_NAMESPACE,
        StoreJsonDecoderKind::RecallOwnerScopeBinding,
    ),
];

pub(crate) const STORE_V6_BLOB_NAMESPACE_REGISTRY: &[StoreBlobNamespaceContract] = &[
    opaque_blob_namespace("state_fs"),
    StoreBlobNamespaceContract {
        namespace: GENERIC_SKILL_BLOB_NAMESPACE,
        decoder_kind: StoreBlobDecoderKind::GenericSkill,
    },
    opaque_blob_namespace("memory"),
    opaque_blob_namespace("daily"),
];

const fn opaque_json_namespace(namespace: &'static str) -> StoreJsonNamespaceContract {
    StoreJsonNamespaceContract {
        namespace,
        decoder_kind: StoreJsonDecoderKind::OpaqueExisting,
    }
}

const fn typed_json_namespace(
    namespace: &'static str,
    decoder_kind: StoreJsonDecoderKind,
) -> StoreJsonNamespaceContract {
    StoreJsonNamespaceContract {
        namespace,
        decoder_kind,
    }
}

const fn opaque_blob_namespace(namespace: &'static str) -> StoreBlobNamespaceContract {
    StoreBlobNamespaceContract {
        namespace,
        decoder_kind: StoreBlobDecoderKind::OpaqueExisting,
    }
}

pub(crate) fn store_v6_json_namespaces() -> impl ExactSizeIterator<Item = &'static str> {
    STORE_V6_JSON_NAMESPACE_REGISTRY
        .iter()
        .map(|contract| contract.namespace)
}

const SUBJECT_GLOBAL_SOUL_JSON_NAMESPACES: &[&str] = &[
    "self_model",
    "self_authored_core",
    "core_revision_ledger",
    "self_continuity",
    "relationship_portfolio",
    "relationship_topology",
    "autonomy_strategy",
    "inner_life",
    "felt_significance",
    "temperament_continuity",
    "inner_conflict",
    "private_doc",
];

pub(crate) fn is_subject_global_soul_json_namespace(namespace: &str) -> bool {
    SUBJECT_GLOBAL_SOUL_JSON_NAMESPACES.contains(&namespace)
}

pub(crate) fn store_memory_space_archive_json_namespaces() -> impl Iterator<Item = &'static str> {
    store_v6_json_namespaces().filter(|namespace| !is_subject_global_soul_json_namespace(namespace))
}

#[cfg(any(test, feature = "nonproduction-replay-harness"))]
pub(crate) fn store_v6_blob_namespaces() -> impl ExactSizeIterator<Item = &'static str> {
    STORE_V6_BLOB_NAMESPACE_REGISTRY
        .iter()
        .map(|contract| contract.namespace)
}

pub(crate) fn classify_store_json_address(
    namespace: &str,
    key: &str,
) -> Result<StoreAddressAdmission<StoreJsonDecoderKind>> {
    require_store_address(namespace, key)?;
    match namespace {
        LEGACY_LONG_TERM_OWNER_NAMESPACE => Ok(StoreAddressAdmission::ForbiddenLegacy(
            StoreLegacyAddressKind::LongTermOwner,
        )),
        LEGACY_RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE => {
            Ok(StoreAddressAdmission::ForbiddenLegacy(
                StoreLegacyAddressKind::RuntimeSkillRecallManifest,
            ))
        }
        _ => Ok(STORE_V6_JSON_NAMESPACE_REGISTRY
            .iter()
            .find(|contract| contract.namespace == namespace)
            .map_or(StoreAddressAdmission::Unknown, |contract| {
                StoreAddressAdmission::Active(contract.decoder_kind)
            })),
    }
}

pub(crate) fn admit_store_json_address(
    namespace: &str,
    key: &str,
    stage: &'static str,
) -> Result<StoreJsonDecoderKind> {
    match classify_store_json_address(namespace, key)? {
        StoreAddressAdmission::Active(decoder) => Ok(decoder),
        StoreAddressAdmission::ForbiddenLegacy(kind) => Err(Error::config(
            stage,
            format!("forbidden legacy json address {namespace}:{key} ({kind:?})"),
        )),
        StoreAddressAdmission::Unknown => Err(Error::config(
            stage,
            format!("unsupported json namespace {namespace}"),
        )),
    }
}

pub(crate) fn admit_store_json_document(
    namespace: &str,
    key: &str,
    value: &serde_json::Value,
    stage: &'static str,
) -> Result<StoreJsonDecoderKind> {
    let decoder = admit_store_json_address(namespace, key, stage)?;
    validate_store_json_value(decoder, key, value, stage)?;
    Ok(decoder)
}

fn validate_store_json_value(
    decoder: StoreJsonDecoderKind,
    key: &str,
    value: &serde_json::Value,
    stage: &'static str,
) -> Result<()> {
    let invalid = |detail: String| Error::config(stage, format!("{key}: {detail}"));
    match decoder {
        StoreJsonDecoderKind::OpaqueExisting => Ok(()),
        StoreJsonDecoderKind::GenericSkillMeta => {
            if !matches!(key, "order" | "disabled") {
                return Err(invalid(
                    "generic skill metadata only admits order or disabled".to_string(),
                ));
            }
            let names = serde_json::from_value::<Vec<String>>(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if names.iter().any(|name| {
                name.trim().is_empty()
                    || name != name.trim()
                    || name.starts_with(LEGACY_RUNTIME_SKILL_KEY_PREFIX)
                    || name.contains(LEGACY_RUNTIME_SKILL_CONTENT_MARKER)
            }) {
                return Err(invalid(
                    "generic skill metadata must not contain runtime skill identity or markers"
                        .to_string(),
                ));
            }
            if names.iter().collect::<BTreeSet<_>>().len() != names.len() {
                return Err(invalid(
                    "generic skill metadata names must be unique".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::MemoryFacetIndex => {
            require_exact_object_fields(
                value,
                &[
                    "owner_ref",
                    "schema_version",
                    "owner_revision",
                    "facet_index_revision",
                    "memory_space_id",
                    "subject_ids",
                    "privacy",
                    "status",
                    "exact_facets",
                    "expanded_facets",
                    "canonical_evidence_refs",
                    "updated_at",
                ],
                invalid,
            )?;
            let facet = decode_json::<MemoryFacetIndexDoc>(value, invalid)?;
            let key_matches_subject = facet.subject_ids.iter().any(|subject_id| {
                scoped_memory_facet_owner_storage_key(
                    &facet.memory_space_id,
                    subject_id,
                    &facet.owner_ref,
                )
                .is_ok_and(|expected| expected == key)
            });
            if facet.schema_version == 0
                || facet.owner_revision == 0
                || facet.facet_index_revision == 0
                || facet.updated_at == 0
                || facet.subject_ids.is_empty()
                || facet.subject_ids.windows(2).any(|pair| pair[0] >= pair[1])
                || !key_matches_subject
            {
                return Err(invalid(
                    "memory facet owner structure or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::MemoryFacetPosting => {
            if value.get("posting_revisions").is_some() {
                require_exact_object_fields(
                    value,
                    &[
                        "schema_version",
                        "memory_space_id",
                        "subject_id",
                        "owner_doc_count",
                        "posting_doc_count",
                        "revision",
                        "owner_versions",
                        "posting_revisions",
                    ],
                    invalid,
                )?;
                let manifest = decode_json::<MemoryFacetIndexManifest>(value, invalid)?;
                if manifest.schema_version == 0
                    || manifest.revision == 0
                    || manifest.owner_doc_count != manifest.owner_versions.len()
                    || manifest.posting_doc_count != manifest.posting_revisions.len()
                    || manifest
                        .owner_versions
                        .windows(2)
                        .any(|pair| pair[0] >= pair[1])
                    || manifest
                        .posting_revisions
                        .windows(2)
                        .any(|pair| pair[0] >= pair[1])
                    || memory_facet_manifest_key(&manifest.memory_space_id, &manifest.subject_id)
                        .ok()
                        .as_deref()
                        != Some(key)
                {
                    return Err(invalid(
                        "memory facet manifest structure or physical key mismatch".to_string(),
                    ));
                }
            } else {
                require_exact_object_fields(
                    value,
                    &[
                        "schema_version",
                        "memory_space_id",
                        "subject_id",
                        "posting_key",
                        "revision",
                        "owner_versions",
                    ],
                    invalid,
                )?;
                let posting = decode_json::<MemoryFacetPostingDoc>(value, invalid)?;
                if posting.schema_version == 0
                    || posting.revision == 0
                    || posting.posting_key != key
                    || posting
                        .owner_versions
                        .windows(2)
                        .any(|pair| pair[0] >= pair[1])
                {
                    return Err(invalid(
                        "memory facet posting structure or physical key mismatch".to_string(),
                    ));
                }
            }
            Ok(())
        }
        StoreJsonDecoderKind::MemoryGraphNode => {
            require_exact_object_fields(
                value,
                &["node_id", "kind", "label", "evidence_refs"],
                invalid,
            )?;
            let node = decode_json::<MemoryGraphNode>(value, invalid)?;
            if !node.validate_contract().accepted {
                return Err(invalid("memory graph node contract is invalid".to_string()));
            }
            Ok(())
        }
        StoreJsonDecoderKind::MemoryGraphEdge => {
            require_exact_object_fields(
                value,
                &[
                    "edge_id",
                    "kind",
                    "from_node_id",
                    "to_node_id",
                    "validity",
                    "evidence_refs",
                ],
                invalid,
            )?;
            let edge = decode_json::<MemoryGraphEdge>(value, invalid)?;
            if !edge.validate_contract().accepted {
                return Err(invalid("memory graph edge contract is invalid".to_string()));
            }
            Ok(())
        }
        StoreJsonDecoderKind::MemoryGraphBacklink => {
            require_exact_object_fields(
                value,
                &["source_kind", "source_id", "fingerprint"],
                invalid,
            )?;
            let backlink = decode_json::<EvidenceBacklink>(value, invalid)?;
            if !canonical_nonempty(&backlink.source_kind)
                || !canonical_nonempty(&backlink.source_id)
                || !canonical_nonempty(&backlink.fingerprint)
            {
                return Err(invalid(
                    "memory graph backlink structure is invalid".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::MemoryGraphRecallIndex => {
            require_exact_object_fields(
                value,
                &[
                    "schema_version",
                    "owner",
                    "index_id",
                    "index_key",
                    "memory_space_id",
                    "mounted_subject_id",
                    "scope_digest",
                    "owner_ref",
                    "owner_candidate_id",
                    "owner_revision",
                    "source_anchor_node_ids",
                    "manifest_generation",
                    "graph_revision",
                    "node_memberships",
                    "edge_memberships",
                    "backlink_memberships",
                    "node_count",
                    "edge_count",
                    "backlink_count",
                    "dependency_digest",
                ],
                invalid,
            )?;
            let index = decode_json::<MemoryGraphRecallIndexDoc>(value, invalid)?;
            if index.schema_version != MEMORY_GRAPH_SCHEMA_VERSION
                || index.index_key != key
                || index.owner_revision == 0
                || index.manifest_generation == 0
                || index.scope_digest
                    != memory_graph_scope_digest(&index.memory_space_id, &index.mounted_subject_id)
                || index.node_count != index.node_memberships.len()
                || index.edge_count != index.edge_memberships.len()
                || index.backlink_count != index.backlink_memberships.len()
            {
                return Err(invalid(
                    "memory graph recall index structure or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::MemoryGraphRevision => {
            require_exact_object_fields(
                value,
                &[
                    "schema_version",
                    "memory_space_id",
                    "mounted_subject_id",
                    "scope_digest",
                    "manifest_generation",
                    "graph_revision",
                    "revision_key",
                    "node_count",
                    "edge_count",
                    "backlink_count",
                    "index_count",
                    "dependency_digest",
                ],
                invalid,
            )?;
            let revision = decode_json::<MemoryGraphRevisionDoc>(value, invalid)?;
            if revision.schema_version != MEMORY_GRAPH_SCHEMA_VERSION
                || revision.revision_key != key
                || revision.manifest_generation == 0
                || revision.scope_digest
                    != memory_graph_scope_digest(
                        &revision.memory_space_id,
                        &revision.mounted_subject_id,
                    )
            {
                return Err(invalid(
                    "memory graph revision structure or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::MemoryGraphManifest => {
            require_exact_object_fields(
                value,
                &[
                    "schema_version",
                    "memory_space_id",
                    "mounted_subject_id",
                    "scope_digest",
                    "manifest_generation",
                    "graph_revision",
                    "node_count",
                    "edge_count",
                    "backlink_count",
                    "index_count",
                    "node_memberships",
                    "edge_memberships",
                    "backlink_memberships",
                    "recall_indexes",
                    "revision",
                    "dependency_digest",
                ],
                invalid,
            )?;
            let manifest = decode_json::<MemoryGraphScopeManifest>(value, invalid)?;
            let validation = validate_memory_graph_scope_manifest(&manifest);
            if !validation.verified
                || memory_graph_scope_manifest_key(
                    &manifest.memory_space_id,
                    &manifest.mounted_subject_id,
                ) != key
            {
                return Err(invalid(format!(
                    "memory graph manifest contract or physical key mismatch: {:?}",
                    validation.failures
                )));
            }
            Ok(())
        }
        StoreJsonDecoderKind::MemoryGraphNodeMembership => {
            validate_memory_graph_membership::<MemoryGraphNodeMembership>(
                value,
                key,
                "membership_key",
                &[
                    "schema_version",
                    "memory_space_id",
                    "mounted_subject_id",
                    "scope_digest",
                    "manifest_generation",
                    "graph_revision",
                    "membership_key",
                    "node_id",
                    "document_key",
                    "document_digest",
                    "owner_ref",
                    "owner_revision",
                    "index_key",
                    "backlink_membership_keys",
                    "dependency_digest",
                ],
                invalid,
            )
        }
        StoreJsonDecoderKind::MemoryGraphEdgeMembership => {
            validate_memory_graph_membership::<MemoryGraphEdgeMembership>(
                value,
                key,
                "membership_key",
                &[
                    "schema_version",
                    "memory_space_id",
                    "mounted_subject_id",
                    "scope_digest",
                    "manifest_generation",
                    "graph_revision",
                    "membership_key",
                    "edge_id",
                    "document_key",
                    "document_digest",
                    "from_node_membership_key",
                    "to_node_membership_key",
                    "backlink_membership_keys",
                    "dependency_digest",
                ],
                invalid,
            )
        }
        StoreJsonDecoderKind::MemoryGraphBacklinkMembership => {
            validate_memory_graph_membership::<MemoryGraphBacklinkMembership>(
                value,
                key,
                "membership_key",
                &[
                    "schema_version",
                    "memory_space_id",
                    "mounted_subject_id",
                    "scope_digest",
                    "manifest_generation",
                    "graph_revision",
                    "membership_key",
                    "backlink_key",
                    "document_key",
                    "document_digest",
                    "node_membership_keys",
                    "edge_membership_keys",
                    "index_keys",
                    "dependency_digest",
                ],
                invalid,
            )
        }
        StoreJsonDecoderKind::LongTermControlRevision
        | StoreJsonDecoderKind::LongTermControlTombstone
        | StoreJsonDecoderKind::LongTermGovernancePolicy
        | StoreJsonDecoderKind::LongTermControlAudit => {
            validate_control_document(decoder, key, value, invalid)
        }
        StoreJsonDecoderKind::ControlPlaneScopeManifest => {
            let manifest = decode_json::<ControlPlaneScopeManifest>(value, invalid)?;
            validate_control_plane_manifest_structure(&manifest, key)
                .map_err(|error| invalid(error.to_string()))
        }
        StoreJsonDecoderKind::LongTermVersionMaterial => {
            let material = serde_json::from_value::<LongTermMemoryVersionMaterial>(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if !material.validate_contract().accepted
                || long_term_version_material_key(
                    &material.memory_space_id,
                    &material.mounted_subject_id,
                    &material.owner_ref,
                    material.owner_revision,
                )
                .ok()
                .as_deref()
                    != Some(key)
            {
                return Err(invalid(
                    "long-term material contract or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::LongTermHeadManifest => {
            let head = serde_json::from_value::<LongTermMemoryHeadManifest>(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if !head.validate_contract().accepted
                || long_term_version_head_key(
                    &head.memory_space_id,
                    &head.mounted_subject_id,
                    &head.owner_ref,
                )
                .ok()
                .as_deref()
                    != Some(key)
            {
                return Err(invalid(
                    "long-term head contract or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::LongTermVersionScopeManifest => {
            let manifest =
                serde_json::from_value::<LongTermMemoryVersionScopeManifest>(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
            let bindings_sorted = manifest
                .head_bindings
                .windows(2)
                .all(|pair| pair[0] < pair[1])
                && manifest
                    .transition_bindings
                    .windows(2)
                    .all(|pair| pair[0] < pair[1]);
            if manifest.schema_version != LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION
                || manifest.manifest_revision == 0
                || manifest.head_count as usize != manifest.head_bindings.len()
                || manifest.transition_count as usize != manifest.transition_bindings.len()
                || !bindings_sorted
                || long_term_version_scope_manifest_key(
                    &manifest.memory_space_id,
                    &manifest.mounted_subject_id,
                )
                .ok()
                .as_deref()
                    != Some(key)
                || manifest.physical_key != key
            {
                return Err(invalid(
                    "long-term scope manifest structure or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::GovernedEvidenceDocument => {
            require_exact_object_fields(
                value,
                &[
                    "schema_version",
                    "physical_key",
                    "memory_space_id",
                    "mounted_subject_id",
                    "document_id",
                    "source_kind",
                    "source_locator",
                    "canonical_evidence_group",
                    "evidence_family_group",
                    "source_revision",
                    "owner_revision",
                    "body",
                    "chunks",
                    "content_digest",
                    "authority",
                    "privacy",
                    "observed_at",
                    "created_at",
                    "updated_at",
                ],
                invalid,
            )?;
            let document = decode_json::<GovernedEvidenceDocument>(value, invalid)?;
            if validate_governed_evidence_document(&document).is_err()
                || document.schema_version != GOVERNED_EVIDENCE_DOCUMENT_SCHEMA_VERSION
                || document.physical_key != key
                || scoped_governed_evidence_document_key(
                    &document.memory_space_id,
                    &document.document_id,
                )
                .ok()
                .as_deref()
                    != Some(key)
            {
                return Err(invalid(
                    "governed evidence document contract or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::GovernedEvidenceSourceRef => {
            let source_ref = decode_json::<GovernedEvidenceSourceRef>(value, invalid)?;
            if source_ref.schema_version != GOVERNED_EVIDENCE_SOURCE_REF_SCHEMA_VERSION
                || source_ref.physical_key != key
                || !source_ref.owner_ref.is_valid()
                || source_ref.owner_revision == 0
                || source_ref.source_revision == 0
                || !canonical_nonempty(&source_ref.memory_space_id)
                || !canonical_nonempty(&source_ref.mounted_subject_id)
                || !canonical_nonempty(&source_ref.source_locator_digest)
                || !canonical_nonempty(&source_ref.canonical_evidence_group)
                || !canonical_nonempty(&source_ref.content_digest)
            {
                return Err(invalid(
                    "governed evidence source ref structure or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::GovernedEvidenceSourceClaimManifest => {
            let manifest = decode_json::<GovernedEvidenceSourceClaimManifest>(value, invalid)?;
            validate_evidence_claim_manifest_structure(&manifest, key)
                .map_err(|error| invalid(error.to_string()))
        }
        StoreJsonDecoderKind::RuntimeSkillOwnerRecord => {
            let record = serde_json::from_value::<RuntimeSkillOwnerRecord>(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if !record.validate_contract().accepted || record.physical_key != key {
                return Err(invalid(
                    "runtime skill owner contract or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::RuntimeSkillScopeManifest => {
            let manifest = serde_json::from_value::<RuntimeSkillScopeManifest>(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if validate_runtime_skill_manifest_structure(&manifest, key).is_err() {
                return Err(invalid(
                    "runtime skill scope manifest structure or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
        StoreJsonDecoderKind::ConversationRecallManifest => {
            decode_recall::<ConversationRecallManifest>(key, value, invalid)
        }
        StoreJsonDecoderKind::ConversationTranscriptPageIndex => {
            decode_recall::<ConversationTranscriptPageIndex>(key, value, invalid)
        }
        StoreJsonDecoderKind::ConversationTranscriptAuxManifest => {
            decode_recall::<ConversationTranscriptAuxManifest>(key, value, invalid)
        }
        StoreJsonDecoderKind::ArchiveRecallManifest => {
            decode_recall::<ArchiveRecallManifest>(key, value, invalid)
        }
        StoreJsonDecoderKind::ContinuityCapsuleScopeIndex => {
            decode_recall::<ContinuityCapsuleScopeIndex>(key, value, invalid)
        }
        StoreJsonDecoderKind::ActiveTaskRunByChatIndex => {
            decode_recall::<ActiveTaskRunByChatIndex>(key, value, invalid)
        }
        StoreJsonDecoderKind::TaskLearningByChatIndex => {
            decode_recall::<TaskLearningByChatIndex>(key, value, invalid)
        }
        StoreJsonDecoderKind::RecallOwnerScopeBinding => {
            let binding = decode_json::<RecallOwnerScopeBinding>(value, invalid)?;
            if binding.physical_key != key || binding.validate().is_err() {
                return Err(invalid(
                    "recall owner scope binding contract or physical key mismatch".to_string(),
                ));
            }
            Ok(())
        }
    }
}

fn decode_json<T: DeserializeOwned>(
    value: &serde_json::Value,
    invalid: impl Fn(String) -> Error,
) -> Result<T> {
    serde_json::from_value::<T>(value.clone()).map_err(|error| invalid(error.to_string()))
}

fn require_exact_object_fields(
    value: &serde_json::Value,
    fields: &[&str],
    invalid: impl Fn(String) -> Error,
) -> Result<()> {
    require_object_fields(value, fields, &[], invalid)
}

fn require_object_fields(
    value: &serde_json::Value,
    required: &[&str],
    optional: &[&str],
    invalid: impl Fn(String) -> Error,
) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("typed document must be a JSON object".to_string()))?;
    let required = required.iter().copied().collect::<BTreeSet<_>>();
    let allowed = required
        .iter()
        .copied()
        .chain(optional.iter().copied())
        .collect::<BTreeSet<_>>();
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if !required.is_subset(&actual) || !actual.is_subset(&allowed) {
        return Err(invalid(format!(
            "typed document fields differ from exact schema; required={required:?}; actual={actual:?}"
        )));
    }
    Ok(())
}

fn canonical_nonempty(value: &str) -> bool {
    !value.trim().is_empty() && value == value.trim()
}

fn validate_memory_graph_membership<T: DeserializeOwned>(
    value: &serde_json::Value,
    key: &str,
    physical_key_field: &str,
    fields: &[&str],
    invalid: impl Fn(String) -> Error + Copy,
) -> Result<()> {
    require_exact_object_fields(value, fields, invalid)?;
    let _: T = decode_json(value, invalid)?;
    let string = |field: &str| {
        value
            .get(field)
            .and_then(serde_json::Value::as_str)
            .filter(|value| canonical_nonempty(value))
    };
    let revision = value
        .get("manifest_generation")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let memory_space_id = string("memory_space_id");
    let mounted_subject_id = string("mounted_subject_id");
    if schema_version != u64::from(MEMORY_GRAPH_SCHEMA_VERSION)
        || revision == 0
        || string(physical_key_field) != Some(key)
        || string("graph_revision").is_none()
        || string("document_key").is_none()
        || string("dependency_digest").is_none()
        || memory_space_id
            .zip(mounted_subject_id)
            .is_none_or(|(space, subject)| {
                string("scope_digest") != Some(memory_graph_scope_digest(space, subject).as_str())
            })
    {
        return Err(invalid(
            "memory graph membership structure or physical key mismatch".to_string(),
        ));
    }
    Ok(())
}

fn validate_control_document(
    decoder: StoreJsonDecoderKind,
    key: &str,
    value: &serde_json::Value,
    invalid: impl Fn(String) -> Error + Copy,
) -> Result<()> {
    let (memory_space_id, logical_key) = match decoder {
        StoreJsonDecoderKind::LongTermControlRevision => {
            let revision = decode_json::<LongTermMemoryControlRevision>(value, invalid)?;
            revision
                .validate_contract()
                .map_err(|error| invalid(error.to_string()))?;
            (revision.memory_space_id, revision.revision_id)
        }
        StoreJsonDecoderKind::LongTermControlTombstone => {
            let tombstone = decode_json::<LongTermMemoryTombstone>(value, invalid)?;
            tombstone
                .validate_contract()
                .map_err(|error| invalid(error.to_string()))?;
            (tombstone.memory_space_id, tombstone.record_id)
        }
        StoreJsonDecoderKind::LongTermGovernancePolicy => {
            require_object_fields(
                value,
                &[
                    "schema_version",
                    "policy_revision",
                    "memory_space_id",
                    "policy_id",
                    "kind",
                    "selector",
                    "reason",
                    "created_at",
                    "updated_at",
                ],
                &["duration", "expires_at"],
                invalid,
            )?;
            let policy = decode_json::<MemoryLongTermGovernancePolicy>(value, invalid)?;
            if policy.schema_version != LONG_TERM_CONTROL_SCHEMA_VERSION
                || policy.policy_revision == 0
                || !canonical_nonempty(&policy.memory_space_id)
                || !canonical_nonempty(&policy.policy_id)
                || !canonical_nonempty(&policy.kind)
                || !canonical_nonempty(&policy.reason)
                || policy.created_at == 0
                || policy.updated_at < policy.created_at
            {
                return Err(invalid(
                    "long-term governance policy structure is invalid".to_string(),
                ));
            }
            (policy.memory_space_id, policy.policy_id)
        }
        StoreJsonDecoderKind::LongTermControlAudit => {
            require_object_fields(
                value,
                &[
                    "schema_version",
                    "event_id",
                    "transaction_id",
                    "operation",
                    "effects",
                    "reason",
                    "owner_subject_id",
                    "created_at",
                ],
                &["actor_subject_id", "memory_space_id"],
                invalid,
            )?;
            let audit = decode_json::<LongTermMemoryControlAuditEvent>(value, invalid)?;
            let memory_space_id = audit.memory_space_id.clone().ok_or_else(|| {
                invalid("long-term control audit is missing memory-space ownership".to_string())
            })?;
            let mut canonical = audit.clone();
            canonical
                .bind_canonical_event_id()
                .map_err(|error| invalid(error.to_string()))?;
            if audit.schema_version != LONG_TERM_CONTROL_SCHEMA_VERSION
                || !canonical_nonempty(&audit.transaction_id)
                || !canonical_nonempty(&audit.reason)
                || !canonical_nonempty(&audit.owner_subject_id)
                || audit.created_at == 0
                || canonical.event_id != audit.event_id
                || audit.effects.is_empty()
                || audit.effects.windows(2).any(|pair| pair[0] >= pair[1])
                || audit.effects.iter().any(|effect| match effect {
                    ControlEffectRef::Revision {
                        mounted_subject_id, ..
                    }
                    | ControlEffectRef::Tombstone {
                        owner_subject_id: mounted_subject_id,
                        ..
                    }
                    | ControlEffectRef::Policy {
                        owner_subject_id: mounted_subject_id,
                        ..
                    } => mounted_subject_id != &audit.owner_subject_id,
                })
            {
                return Err(invalid(
                    "long-term control audit structure is invalid".to_string(),
                ));
            }
            (memory_space_id, audit.event_id)
        }
        _ => {
            return Err(invalid(
                "unsupported control-plane decoder kind".to_string(),
            ));
        }
    };
    let namespace = match decoder {
        StoreJsonDecoderKind::LongTermControlRevision => LONG_TERM_CONTROL_REVISION_NAMESPACE,
        StoreJsonDecoderKind::LongTermControlTombstone => LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        StoreJsonDecoderKind::LongTermGovernancePolicy => LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        StoreJsonDecoderKind::LongTermControlAudit => LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        _ => unreachable!("control decoder filtered above"),
    };
    if scoped_long_term_control_storage_key(&memory_space_id, namespace, &logical_key)
        .ok()
        .as_deref()
        != Some(key)
    {
        return Err(invalid(
            "control-plane physical key is not canonical".to_string(),
        ));
    }
    Ok(())
}

fn validate_control_plane_manifest_structure(
    manifest: &ControlPlaneScopeManifest,
    key: &str,
) -> Result<()> {
    let expected_key =
        control_plane_scope_manifest_key(&manifest.memory_space_id, &manifest.mounted_subject_id)?;
    let canonical_entries = manifest.entries.windows(2).all(|pair| pair[0] < pair[1])
        && manifest.entries.iter().all(|entry| {
            canonical_nonempty(&entry.namespace)
                && canonical_nonempty(&entry.key)
                && is_sha256_digest(&entry.content_sha256)
        });
    if manifest.schema_version != CONTROL_PLANE_SCOPE_MANIFEST_SCHEMA_VERSION
        || manifest.revision == 0
        || manifest.physical_key != key
        || expected_key != key
        || manifest.entry_count != manifest.entries.len()
        || !canonical_entries
        || manifest.entries_digest
            != control_plane_scope_manifest_digest(
                manifest.revision,
                &manifest.memory_space_id,
                &manifest.mounted_subject_id,
                &manifest.entries,
            )?
    {
        return Err(Error::config(
            "control_plane_scope_manifest",
            "control-plane manifest structure or physical key mismatch",
        ));
    }
    Ok(())
}

fn validate_evidence_claim_manifest_structure(
    manifest: &GovernedEvidenceSourceClaimManifest,
    key: &str,
) -> Result<()> {
    let expected_key = governed_evidence_source_claim_manifest_key(
        &manifest.memory_space_id,
        &manifest.mounted_subject_id,
    )?;
    let mut bindings = manifest.owner_claim_bindings.clone();
    for binding in &bindings {
        binding.validate()?;
    }
    bindings.sort_by(|left, right| {
        left.owner_physical_key
            .cmp(&right.owner_physical_key)
            .then_with(|| left.claim_physical_key.cmp(&right.claim_physical_key))
    });
    let owner_keys = bindings
        .iter()
        .map(|binding| binding.owner_physical_key.clone())
        .collect::<Vec<_>>();
    let mut claim_keys = bindings
        .iter()
        .map(|binding| binding.claim_physical_key.clone())
        .collect::<Vec<_>>();
    claim_keys.sort();
    if manifest.schema_version != GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_SCHEMA_VERSION
        || manifest.physical_key != key
        || expected_key != key
        || bindings != manifest.owner_claim_bindings
        || bindings.windows(2).any(|pair| {
            pair[0].owner_physical_key == pair[1].owner_physical_key
                || pair[0].claim_physical_key == pair[1].claim_physical_key
        })
        || manifest.owner_count != owner_keys.len()
        || manifest.claim_count != claim_keys.len()
        || manifest.owner_keys != owner_keys
        || manifest.claim_keys != claim_keys
        || manifest.owner_keys_digest
            != governed_evidence_source_claim_keys_digest(&manifest.owner_keys)
        || manifest.claim_keys_digest
            != governed_evidence_source_claim_keys_digest(&manifest.claim_keys)
        || manifest.closure_digest
            != governed_evidence_source_claim_closure_digest(
                &manifest.memory_space_id,
                &manifest.mounted_subject_id,
                &manifest.owner_claim_bindings,
                &manifest.owner_keys_digest,
                &manifest.claim_keys_digest,
            )
    {
        return Err(Error::config(
            "governed_evidence_source_claim_manifest",
            "evidence claim manifest structure or physical key mismatch",
        ));
    }
    Ok(())
}

fn validate_runtime_skill_manifest_structure(
    manifest: &RuntimeSkillScopeManifest,
    key: &str,
) -> Result<()> {
    let expected_key =
        runtime_skill_scope_manifest_key(&manifest.memory_space_id, &manifest.owning_scope)?;
    let canonical_bindings = manifest
        .owner_bindings
        .windows(2)
        .all(|pair| pair[0].owner_ref < pair[1].owner_ref)
        && manifest.owner_bindings.iter().all(|binding| {
            binding.owner_ref.is_valid()
                && binding.owner_revision > 0
                && is_sha256_digest(&binding.content_digest)
                && canonical_runtime_skill_owner_key(
                    &manifest.memory_space_id,
                    &manifest.owning_scope,
                    &binding.owner_ref.owner_id,
                )
                .is_ok_and(|expected| expected == binding.owner_physical_key)
        });
    if manifest.schema_version != RUNTIME_SKILL_SCOPE_MANIFEST_SCHEMA_VERSION
        || manifest.revision == 0
        || manifest.physical_key != key
        || expected_key != key
        || manifest.owner_count != manifest.owner_bindings.len()
        || !canonical_bindings
        || manifest.bindings_digest != runtime_skill_manifest_bindings_digest(manifest)?
    {
        return Err(Error::config(
            "runtime_skill_scope_manifest",
            "runtime skill manifest structure or physical key mismatch",
        ));
    }
    Ok(())
}

fn runtime_skill_manifest_bindings_digest(manifest: &RuntimeSkillScopeManifest) -> Result<String> {
    let encoded = serde_json::to_vec(&manifest.owner_bindings)
        .map_err(|error| Error::config("runtime_skill_scope_manifest", error.to_string()))?;
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, b"runtime_skill_scope_bindings_digest_v1");
    hash_field(&mut hasher, &manifest.revision.to_be_bytes());
    hash_field(&mut hasher, manifest.memory_space_id.as_bytes());
    match &manifest.owning_scope {
        RuntimeSkillOwningScope::Subject { mounted_subject_id } => {
            hash_field(&mut hasher, b"subject");
            hash_field(&mut hasher, mounted_subject_id.as_bytes());
        }
        RuntimeSkillOwningScope::SharedProgram => hash_field(&mut hasher, b"shared_program"),
    }
    hash_field(&mut hasher, &encoded);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn decode_recall<T: TypedRecallIndex>(
    key: &str,
    value: &serde_json::Value,
    invalid: impl Fn(String) -> Error,
) -> Result<()> {
    decode_typed_recall_index::<T>(key, value.clone())
        .map(|_| ())
        .map_err(|error| invalid(error.to_string()))
}

pub(crate) fn classify_store_blob_address(
    namespace: &str,
    key: &str,
    value: Option<&[u8]>,
) -> Result<StoreAddressAdmission<StoreBlobDecoderKind>> {
    require_store_address(namespace, key)?;
    let Some(contract) = STORE_V6_BLOB_NAMESPACE_REGISTRY
        .iter()
        .find(|contract| contract.namespace == namespace)
    else {
        return Ok(StoreAddressAdmission::Unknown);
    };
    if namespace == GENERIC_SKILL_BLOB_NAMESPACE {
        if key.starts_with(LEGACY_RUNTIME_SKILL_KEY_PREFIX) {
            return Ok(StoreAddressAdmission::ForbiddenLegacy(
                StoreLegacyAddressKind::RuntimeSkillBlobKeyPrefix,
            ));
        }
        if value.is_some_and(legacy_runtime_skill_marker_matches) {
            return Ok(StoreAddressAdmission::ForbiddenLegacy(
                StoreLegacyAddressKind::RuntimeSkillBlobMarker,
            ));
        }
    }
    Ok(StoreAddressAdmission::Active(contract.decoder_kind))
}

pub(crate) fn validate_store_schema_identity(
    schema_id: &str,
    schema_version: u32,
    stage: &'static str,
) -> Result<()> {
    if schema_id != STORE_SCHEMA_ID {
        return Err(Error::config(
            stage,
            format!("unsupported schema {schema_id}"),
        ));
    }
    if schema_version != STORE_SCHEMA_VERSION {
        return Err(Error::config(
            stage,
            format!("unsupported schema version {schema_version}"),
        ));
    }
    Ok(())
}

fn require_store_address(namespace: &str, key: &str) -> Result<()> {
    if namespace.is_empty()
        || namespace != namespace.trim()
        || key.trim().is_empty()
        || namespace.contains('\0')
        || key.contains('\0')
    {
        return Err(Error::config(
            "store_schema_address",
            "namespace and key must be non-empty canonical address components",
        ));
    }
    Ok(())
}

fn legacy_runtime_skill_marker_matches(value: &[u8]) -> bool {
    std::str::from_utf8(value).ok().is_some_and(|content| {
        content
            .trim_start()
            .starts_with(LEGACY_RUNTIME_SKILL_CONTENT_MARKER)
    })
}
pub const GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_SCHEMA_VERSION: u32 = 2;
pub const GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE: &str =
    "governed_evidence_source_claim_manifests";
pub const CONTROL_PLANE_SCOPE_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE: &str = "control_plane_scope_manifests";
pub const RECALL_OWNER_SCOPE_BINDING_SCHEMA_VERSION: u32 = 1;
pub const RECALL_OWNER_SCOPE_BINDING_NAMESPACE: &str = "recall_owner_scope_bindings";

const GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_KEY_DOMAIN: &[u8] =
    b"governed_evidence_source_claim_manifest_key_v1";
const GOVERNED_EVIDENCE_SOURCE_CLAIM_KEYS_DIGEST_DOMAIN: &[u8] =
    b"governed_evidence_source_claim_keys_digest_v2";
const GOVERNED_EVIDENCE_SOURCE_CLAIM_BINDING_DIGEST_DOMAIN: &[u8] =
    b"governed_evidence_source_claim_binding_digest_v2";
const GOVERNED_EVIDENCE_SOURCE_CLAIM_CLOSURE_DIGEST_DOMAIN: &[u8] =
    b"governed_evidence_source_claim_closure_digest_v2";
const CONTROL_PLANE_SCOPE_MANIFEST_KEY_DOMAIN: &[u8] = b"control_plane_scope_manifest_key_v1";
const CONTROL_PLANE_SCOPE_MANIFEST_DIGEST_DOMAIN: &[u8] = b"control_plane_scope_manifest_digest_v1";
const RECALL_OWNER_SCOPE_BINDING_KEY_DOMAIN: &[u8] = b"recall_owner_scope_binding_key_v1";
const RECALL_OWNER_SCOPE_BINDING_DIGEST_DOMAIN: &[u8] = b"recall_owner_scope_binding_digest_v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ControlPlaneScopeEntry {
    pub namespace: String,
    pub key: String,
    pub content_sha256: String,
}

impl ControlPlaneScopeEntry {
    pub fn from_json(namespace: &str, key: &str, value: &serde_json::Value) -> Result<Self> {
        require_scope_component(namespace, "control_plane_scope_manifest", "namespace")?;
        require_scope_component(key, "control_plane_scope_manifest", "key")?;
        Ok(Self {
            namespace: namespace.to_string(),
            key: key.to_string(),
            content_sha256: json_sha256(value, "control_plane_scope_manifest")?,
        })
    }

    pub fn validate_value(&self, value: &serde_json::Value) -> Result<()> {
        let expected = Self::from_json(&self.namespace, &self.key, value)?;
        if self == &expected {
            Ok(())
        } else {
            Err(Error::config(
                "control_plane_scope_manifest",
                "control-plane entry content digest mismatch",
            ))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ControlPlaneScopeManifest {
    pub schema_version: u32,
    pub physical_key: String,
    pub revision: u64,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub entry_count: usize,
    pub entries: Vec<ControlPlaneScopeEntry>,
    pub entries_digest: String,
}

impl ControlPlaneScopeManifest {
    pub fn build(
        revision: u64,
        memory_space_id: &str,
        mounted_subject_id: &str,
        entries: impl IntoIterator<Item = ControlPlaneScopeEntry>,
        max_entries: usize,
    ) -> Result<Self> {
        let memory_space_id = require_scope_component(
            memory_space_id,
            "control_plane_scope_manifest",
            "memory_space_id",
        )?;
        let mounted_subject_id = require_scope_component(
            mounted_subject_id,
            "control_plane_scope_manifest",
            "mounted_subject_id",
        )?;
        if revision == 0 || max_entries == 0 {
            return Err(Error::config(
                "control_plane_scope_manifest",
                "revision and max_entries must be greater than zero",
            ));
        }
        let mut entries = entries.into_iter().collect::<Vec<_>>();
        entries.sort();
        if entries.len() > max_entries
            || entries
                .windows(2)
                .any(|pair| pair[0].namespace == pair[1].namespace && pair[0].key == pair[1].key)
        {
            return Err(Error::config(
                "control_plane_scope_manifest",
                "control-plane entries are duplicate or exceed the pinned limit",
            ));
        }
        let physical_key = control_plane_scope_manifest_key(memory_space_id, mounted_subject_id)?;
        let entries_digest = control_plane_scope_manifest_digest(
            revision,
            memory_space_id,
            mounted_subject_id,
            &entries,
        )?;
        Ok(Self {
            schema_version: CONTROL_PLANE_SCOPE_MANIFEST_SCHEMA_VERSION,
            physical_key,
            revision,
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: mounted_subject_id.to_string(),
            entry_count: entries.len(),
            entries,
            entries_digest,
        })
    }

    pub fn validate(&self, max_entries: usize) -> Result<()> {
        let expected = Self::build(
            self.revision,
            &self.memory_space_id,
            &self.mounted_subject_id,
            self.entries.clone(),
            max_entries,
        )?;
        if self == &expected && self.schema_version == CONTROL_PLANE_SCOPE_MANIFEST_SCHEMA_VERSION {
            Ok(())
        } else {
            Err(Error::config(
                "control_plane_scope_manifest",
                "control-plane scope manifest is not canonical",
            ))
        }
    }
}

pub fn control_plane_scope_manifest_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
) -> Result<String> {
    let memory_space_id = require_scope_component(
        memory_space_id,
        "control_plane_scope_manifest",
        "memory_space_id",
    )?;
    let mounted_subject_id = require_scope_component(
        mounted_subject_id,
        "control_plane_scope_manifest",
        "mounted_subject_id",
    )?;
    Ok(format!(
        "cpsm1:{}",
        digest_fields(
            CONTROL_PLANE_SCOPE_MANIFEST_KEY_DOMAIN,
            &[memory_space_id.as_bytes(), mounted_subject_id.as_bytes()],
        )
    ))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecallOwnerScopeBinding {
    pub schema_version: u32,
    pub physical_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub owner_kind: String,
    pub owner_namespace: String,
    pub owner_key: String,
    pub owner_content_sha256: String,
    pub binding_digest: String,
}

impl RecallOwnerScopeBinding {
    pub fn build(
        memory_space_id: &str,
        mounted_subject_id: &str,
        owner_kind: &str,
        owner_namespace: &str,
        owner_key: &str,
        owner_content_sha256: &str,
    ) -> Result<Self> {
        let memory_space_id = require_scope_component(
            memory_space_id,
            "recall_owner_scope_binding",
            "memory_space_id",
        )?;
        let mounted_subject_id = require_scope_component(
            mounted_subject_id,
            "recall_owner_scope_binding",
            "mounted_subject_id",
        )?;
        let owner_kind =
            require_scope_component(owner_kind, "recall_owner_scope_binding", "owner_kind")?;
        let owner_namespace = require_scope_component(
            owner_namespace,
            "recall_owner_scope_binding",
            "owner_namespace",
        )?;
        let owner_key =
            require_scope_component(owner_key, "recall_owner_scope_binding", "owner_key")?;
        if !is_sha256_digest(owner_content_sha256) {
            return Err(Error::config(
                "recall_owner_scope_binding",
                "owner content digest is not canonical sha256",
            ));
        }
        let physical_key = recall_owner_scope_binding_key(owner_kind, owner_namespace, owner_key)?;
        let binding_digest = format!(
            "sha256:{}",
            digest_fields(
                RECALL_OWNER_SCOPE_BINDING_DIGEST_DOMAIN,
                &[
                    memory_space_id.as_bytes(),
                    mounted_subject_id.as_bytes(),
                    owner_kind.as_bytes(),
                    owner_namespace.as_bytes(),
                    owner_key.as_bytes(),
                    owner_content_sha256.as_bytes(),
                ],
            )
        );
        Ok(Self {
            schema_version: RECALL_OWNER_SCOPE_BINDING_SCHEMA_VERSION,
            physical_key,
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: mounted_subject_id.to_string(),
            owner_kind: owner_kind.to_string(),
            owner_namespace: owner_namespace.to_string(),
            owner_key: owner_key.to_string(),
            owner_content_sha256: owner_content_sha256.to_string(),
            binding_digest,
        })
    }

    pub fn validate(&self) -> Result<()> {
        let expected = Self::build(
            &self.memory_space_id,
            &self.mounted_subject_id,
            &self.owner_kind,
            &self.owner_namespace,
            &self.owner_key,
            &self.owner_content_sha256,
        )?;
        if self == &expected && self.schema_version == RECALL_OWNER_SCOPE_BINDING_SCHEMA_VERSION {
            Ok(())
        } else {
            Err(Error::config(
                "recall_owner_scope_binding",
                "recall owner scope binding is not canonical",
            ))
        }
    }
}

pub fn recall_owner_scope_binding_key(
    owner_kind: &str,
    owner_namespace: &str,
    owner_key: &str,
) -> Result<String> {
    for (field, value) in [
        ("owner_kind", owner_kind),
        ("owner_namespace", owner_namespace),
        ("owner_key", owner_key),
    ] {
        require_scope_component(value, "recall_owner_scope_binding", field)?;
    }
    Ok(format!(
        "rosb1:{}",
        digest_fields(
            RECALL_OWNER_SCOPE_BINDING_KEY_DOMAIN,
            &[
                owner_kind.as_bytes(),
                owner_namespace.as_bytes(),
                owner_key.as_bytes(),
            ],
        )
    ))
}

fn control_plane_scope_manifest_digest(
    revision: u64,
    memory_space_id: &str,
    mounted_subject_id: &str,
    entries: &[ControlPlaneScopeEntry],
) -> Result<String> {
    let encoded = serde_json::to_vec(entries)
        .map_err(|error| Error::config("control_plane_scope_manifest", error.to_string()))?;
    Ok(format!(
        "sha256:{}",
        digest_fields(
            CONTROL_PLANE_SCOPE_MANIFEST_DIGEST_DOMAIN,
            &[
                &revision.to_be_bytes(),
                memory_space_id.as_bytes(),
                mounted_subject_id.as_bytes(),
                &encoded,
            ],
        )
    ))
}

fn json_sha256(value: &serde_json::Value, stage: &'static str) -> Result<String> {
    let encoded =
        serde_json::to_vec(value).map_err(|error| Error::config(stage, error.to_string()))?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn digest_fields(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, domain);
    for field in fields {
        hash_field(&mut hasher, field);
    }
    format!("{:x}", hasher.finalize())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StoreProjectionScope {
    FullStore,
    MemorySpace {
        memory_space_id: String,
        physical_owning_scope: StorePhysicalOwningScope,
        includes_private: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GovernedEvidenceOwnerClaimBinding {
    pub owner_physical_key: String,
    pub claim_physical_key: String,
    pub owner_revision: u64,
    pub source_revision: u64,
    pub content_digest: String,
    pub binding_digest: String,
}

impl GovernedEvidenceOwnerClaimBinding {
    pub fn from_document_claim(
        document: &GovernedEvidenceDocument,
        claim: &GovernedEvidenceSourceRef,
    ) -> Result<Self> {
        if document.memory_space_id != claim.memory_space_id
            || document.mounted_subject_id != claim.mounted_subject_id
            || document.owner_revision != claim.owner_revision
            || document.source_revision != claim.source_revision
            || document.content_digest != claim.content_digest
        {
            return Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence owner and claim cannot form an exact typed binding",
            ));
        }
        Self::new(
            document.physical_key.clone(),
            claim.physical_key.clone(),
            document.owner_revision,
            document.source_revision,
            document.content_digest.clone(),
        )
    }

    pub fn new(
        owner_physical_key: impl Into<String>,
        claim_physical_key: impl Into<String>,
        owner_revision: u64,
        source_revision: u64,
        content_digest: impl Into<String>,
    ) -> Result<Self> {
        let owner_physical_key = owner_physical_key.into();
        let claim_physical_key = claim_physical_key.into();
        let content_digest = content_digest.into();
        if owner_physical_key.trim().is_empty()
            || claim_physical_key.trim().is_empty()
            || content_digest.trim().is_empty()
            || owner_physical_key != owner_physical_key.trim()
            || claim_physical_key != claim_physical_key.trim()
            || content_digest != content_digest.trim()
            || owner_revision == 0
            || source_revision == 0
        {
            return Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence owner-claim binding is not canonical",
            ));
        }
        let binding_digest = governed_evidence_source_claim_binding_digest(
            &owner_physical_key,
            &claim_physical_key,
            owner_revision,
            source_revision,
            &content_digest,
        );
        Ok(Self {
            owner_physical_key,
            claim_physical_key,
            owner_revision,
            source_revision,
            content_digest,
            binding_digest,
        })
    }

    pub fn validate(&self) -> Result<()> {
        let expected = Self::new(
            self.owner_physical_key.clone(),
            self.claim_physical_key.clone(),
            self.owner_revision,
            self.source_revision,
            self.content_digest.clone(),
        )?;
        if self == &expected {
            Ok(())
        } else {
            Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence owner-claim binding digest mismatch",
            ))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GovernedEvidenceSourceClaimManifest {
    pub schema_version: u32,
    pub physical_key: String,
    pub memory_space_id: String,
    pub mounted_subject_id: String,
    pub owner_count: usize,
    pub claim_count: usize,
    pub owner_keys: Vec<String>,
    pub claim_keys: Vec<String>,
    pub owner_keys_digest: String,
    pub claim_keys_digest: String,
    pub owner_claim_bindings: Vec<GovernedEvidenceOwnerClaimBinding>,
    pub closure_digest: String,
}

impl GovernedEvidenceSourceClaimManifest {
    pub fn build(
        memory_space_id: &str,
        mounted_subject_id: &str,
        bindings: impl IntoIterator<Item = GovernedEvidenceOwnerClaimBinding>,
        max_scope_entries: usize,
    ) -> Result<Self> {
        let memory_space_id = require_scope_component(
            memory_space_id,
            "governed_evidence_source_claim_manifest",
            "memory_space_id",
        )?;
        let mounted_subject_id = require_scope_component(
            mounted_subject_id,
            "governed_evidence_source_claim_manifest",
            "mounted_subject_id",
        )?;
        let mut owner_claim_bindings = bindings.into_iter().collect::<Vec<_>>();
        for binding in &owner_claim_bindings {
            binding.validate()?;
        }
        owner_claim_bindings.sort_by(|left, right| {
            left.owner_physical_key
                .cmp(&right.owner_physical_key)
                .then_with(|| left.claim_physical_key.cmp(&right.claim_physical_key))
        });
        if owner_claim_bindings.windows(2).any(|pair| {
            pair[0].owner_physical_key == pair[1].owner_physical_key
                || pair[0].claim_physical_key == pair[1].claim_physical_key
        }) {
            return Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence owner-claim bindings contain duplicate owner or claim keys",
            ));
        }
        let owner_keys = owner_claim_bindings
            .iter()
            .map(|binding| binding.owner_physical_key.clone())
            .collect::<Vec<_>>();
        let mut claim_keys = owner_claim_bindings
            .iter()
            .map(|binding| binding.claim_physical_key.clone())
            .collect::<Vec<_>>();
        claim_keys.sort();
        if max_scope_entries == 0
            || owner_keys.len() > max_scope_entries
            || claim_keys.len() > max_scope_entries
        {
            return Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence source claim scope exceeds the pinned profile entry limit",
            ));
        }
        let owner_keys_digest = governed_evidence_source_claim_keys_digest(&owner_keys);
        let claim_keys_digest = governed_evidence_source_claim_keys_digest(&claim_keys);
        let closure_digest = governed_evidence_source_claim_closure_digest(
            memory_space_id,
            mounted_subject_id,
            &owner_claim_bindings,
            &owner_keys_digest,
            &claim_keys_digest,
        );
        Ok(Self {
            schema_version: GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_SCHEMA_VERSION,
            physical_key: governed_evidence_source_claim_manifest_key(
                memory_space_id,
                mounted_subject_id,
            )?,
            memory_space_id: memory_space_id.to_string(),
            mounted_subject_id: mounted_subject_id.to_string(),
            owner_count: owner_keys.len(),
            claim_count: claim_keys.len(),
            owner_keys: owner_keys.clone(),
            claim_keys: claim_keys.clone(),
            owner_keys_digest,
            claim_keys_digest,
            owner_claim_bindings,
            closure_digest,
        })
    }

    pub fn validate_exact(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        bindings: impl IntoIterator<Item = GovernedEvidenceOwnerClaimBinding>,
        max_scope_entries: usize,
    ) -> Result<()> {
        let expected = Self::build(
            memory_space_id,
            mounted_subject_id,
            bindings,
            max_scope_entries,
        )?;
        if self == &expected {
            Ok(())
        } else {
            Err(Error::config(
                "governed_evidence_source_claim_manifest",
                "evidence source claim manifest does not match exact scope closure",
            ))
        }
    }

    pub fn binding_for_owner(
        &self,
        owner_physical_key: &str,
    ) -> Option<&GovernedEvidenceOwnerClaimBinding> {
        self.owner_claim_bindings
            .binary_search_by(|binding| binding.owner_physical_key.as_str().cmp(owner_physical_key))
            .ok()
            .map(|index| &self.owner_claim_bindings[index])
    }
}

pub fn governed_evidence_source_claim_manifest_key(
    memory_space_id: &str,
    mounted_subject_id: &str,
) -> Result<String> {
    let memory_space_id = require_scope_component(
        memory_space_id,
        "governed_evidence_source_claim_manifest_key",
        "memory_space_id",
    )?;
    let mounted_subject_id = require_scope_component(
        mounted_subject_id,
        "governed_evidence_source_claim_manifest_key",
        "mounted_subject_id",
    )?;
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_KEY_DOMAIN,
    );
    hash_field(&mut hasher, memory_space_id.as_bytes());
    hash_field(&mut hasher, mounted_subject_id.as_bytes());
    Ok(format!(
        "{}:{:x}",
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
        hasher.finalize()
    ))
}

pub fn validate_governed_evidence_source_claim_scope_closure(
    manifest: Option<&GovernedEvidenceSourceClaimManifest>,
    memory_space_id: &str,
    mounted_subject_id: &str,
    bindings: impl IntoIterator<Item = GovernedEvidenceOwnerClaimBinding>,
    max_scope_entries: usize,
) -> Result<()> {
    let manifest = manifest.ok_or_else(|| {
        Error::config(
            "governed_evidence_source_claim_manifest",
            "evidence source claim scope manifest is missing",
        )
    })?;
    manifest.validate_exact(
        memory_space_id,
        mounted_subject_id,
        bindings,
        max_scope_entries,
    )
}

fn governed_evidence_source_claim_keys_digest(keys: &[String]) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_KEYS_DIGEST_DOMAIN,
    );
    for key in keys {
        hash_field(&mut hasher, key.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn governed_evidence_source_claim_binding_digest(
    owner_physical_key: &str,
    claim_physical_key: &str,
    owner_revision: u64,
    source_revision: u64,
    content_digest: &str,
) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_BINDING_DIGEST_DOMAIN,
    );
    hash_field(&mut hasher, owner_physical_key.as_bytes());
    hash_field(&mut hasher, claim_physical_key.as_bytes());
    hash_field(&mut hasher, &owner_revision.to_be_bytes());
    hash_field(&mut hasher, &source_revision.to_be_bytes());
    hash_field(&mut hasher, content_digest.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn governed_evidence_source_claim_closure_digest(
    memory_space_id: &str,
    mounted_subject_id: &str,
    bindings: &[GovernedEvidenceOwnerClaimBinding],
    owner_keys_digest: &str,
    claim_keys_digest: &str,
) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        GOVERNED_EVIDENCE_SOURCE_CLAIM_CLOSURE_DIGEST_DOMAIN,
    );
    hash_field(&mut hasher, memory_space_id.as_bytes());
    hash_field(&mut hasher, mounted_subject_id.as_bytes());
    hash_field(&mut hasher, owner_keys_digest.as_bytes());
    hash_field(&mut hasher, claim_keys_digest.as_bytes());
    for binding in bindings {
        hash_field(&mut hasher, binding.binding_digest.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn require_scope_component<'a>(
    value: &'a str,
    stage: &'static str,
    field: &str,
) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::config(stage, format!("{field} must not be empty")));
    }
    Ok(value)
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoreSchemaManifest {
    pub schema_id: String,
    pub schema_version: u32,
    pub backend: String,
    pub profile: String,
    pub memory_system_kind: String,
    pub projection_scope: StoreProjectionScope,
    pub created_at_unix_secs: u64,
    pub last_opened_at_unix_secs: u64,
}

impl StoreSchemaManifest {
    pub fn new(backend: StoreBackendKind, profile: ProfileId, now_secs: u64) -> Self {
        Self {
            schema_id: STORE_SCHEMA_ID.to_string(),
            schema_version: STORE_SCHEMA_VERSION,
            backend: backend.as_str().to_string(),
            profile: profile.as_str().to_string(),
            memory_system_kind: profile_memory_system_kind(profile).as_str().to_string(),
            projection_scope: StoreProjectionScope::FullStore,
            created_at_unix_secs: now_secs,
            last_opened_at_unix_secs: now_secs,
        }
    }

    pub fn validate_against(
        &self,
        backend: StoreBackendKind,
        profile: ProfileId,
        memory_system_kind: MemorySystemKind,
        stage: &'static str,
    ) -> Result<()> {
        validate_store_schema_identity(&self.schema_id, self.schema_version, stage)?;
        if self.backend != backend.as_str() {
            return Err(Error::config(
                stage,
                format!(
                    "backend mismatch: manifest={}, config={}",
                    self.backend,
                    backend.as_str()
                ),
            ));
        }
        if self.profile != profile.as_str() {
            return Err(Error::config(
                stage,
                format!(
                    "profile mismatch: manifest={}, config={}",
                    self.profile,
                    profile.as_str()
                ),
            ));
        }
        if self.memory_system_kind != memory_system_kind.as_str() {
            return Err(Error::config(
                stage,
                format!(
                    "memory system kind mismatch: manifest={}, config={}",
                    self.memory_system_kind,
                    memory_system_kind.as_str()
                ),
            ));
        }
        if self.projection_scope != StoreProjectionScope::FullStore {
            return Err(Error::config(
                stage,
                "store manifest must use full_store projection scope",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn binding(owner: &str, claim: &str, revision: u64) -> GovernedEvidenceOwnerClaimBinding {
        GovernedEvidenceOwnerClaimBinding::new(
            owner,
            claim,
            revision,
            revision,
            format!("content:{revision}"),
        )
        .unwrap()
    }

    #[test]
    fn source_claim_manifest_is_order_independent_and_rejects_extra_claims() {
        let manifest = GovernedEvidenceSourceClaimManifest::build(
            "space:a",
            "subject:a",
            [
                binding("owner:b", "claim:b", 2),
                binding("owner:a", "claim:a", 1),
            ],
            8,
        )
        .unwrap();
        validate_governed_evidence_source_claim_scope_closure(
            Some(&manifest),
            "space:a",
            "subject:a",
            [
                binding("owner:a", "claim:a", 1),
                binding("owner:b", "claim:b", 2),
            ],
            8,
        )
        .unwrap();
        assert!(validate_governed_evidence_source_claim_scope_closure(
            Some(&manifest),
            "space:a",
            "subject:a",
            [
                binding("owner:a", "claim:a", 1),
                binding("owner:b", "claim:b", 2),
                binding("owner:extra", "claim:extra", 3),
            ],
            8,
        )
        .is_err());
        assert!(validate_governed_evidence_source_claim_scope_closure(
            None,
            "space:a",
            "subject:a",
            [
                binding("owner:a", "claim:a", 1),
                binding("owner:b", "claim:b", 2)
            ],
            8,
        )
        .is_err());
    }

    #[test]
    fn source_claim_manifest_v1_shape_fails_closed() {
        let old = serde_json::json!({
            "schema_version": 1,
            "physical_key": "manifest",
            "memory_space_id": "space:a",
            "mounted_subject_id": "subject:a",
            "owner_count": 1,
            "claim_count": 1,
            "owner_keys": ["owner:a"],
            "claim_keys": ["claim:a"],
            "owner_keys_digest": "old",
            "claim_keys_digest": "old"
        });
        assert!(serde_json::from_value::<GovernedEvidenceSourceClaimManifest>(old).is_err());
    }

    #[test]
    fn source_claim_manifest_key_is_scope_bound() {
        let first = governed_evidence_source_claim_manifest_key("space:a", "subject:a").unwrap();
        let other_space =
            governed_evidence_source_claim_manifest_key("space:b", "subject:a").unwrap();
        let other_subject =
            governed_evidence_source_claim_manifest_key("space:a", "subject:b").unwrap();
        assert_ne!(first, other_space);
        assert_ne!(first, other_subject);
    }

    #[test]
    fn p8_store_schema_identity_is_exactly_v6() {
        assert_eq!(STORE_SCHEMA_ID, "beetle_memory_store_schema_v6");
        assert_eq!(STORE_SCHEMA_VERSION, 6);
        assert!(
            validate_store_schema_identity(STORE_SCHEMA_ID, STORE_SCHEMA_VERSION, "test").is_ok()
        );
        assert!(
            validate_store_schema_identity("beetle_memory_store_schema_v5", 5, "test").is_err()
        );
        assert!(
            validate_store_schema_identity("unknown_memory_store_schema_v999", 999, "test")
                .is_err()
        );
    }

    #[test]
    fn p8_typed_namespaces_have_one_canonical_registry() {
        let active = store_v6_json_namespaces().collect::<BTreeSet<_>>();
        assert_eq!(active.len(), STORE_V6_JSON_NAMESPACE_REGISTRY.len());
        assert_eq!(
            store_v6_blob_namespaces().collect::<BTreeSet<_>>().len(),
            STORE_V6_BLOB_NAMESPACE_REGISTRY.len()
        );
        for namespace in [
            LONG_TERM_VERSION_MATERIAL_NAMESPACE,
            LONG_TERM_HEAD_MANIFEST_NAMESPACE,
            LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
            RUNTIME_SKILL_RECORD_NAMESPACE,
            RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
        ] {
            assert!(active.contains(namespace));
        }
        assert!(!active.contains(LEGACY_LONG_TERM_OWNER_NAMESPACE));
        assert!(!active.contains(LEGACY_RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE));

        assert_eq!(
            classify_store_json_address(LONG_TERM_VERSION_MATERIAL_NAMESPACE, "material:key")
                .unwrap(),
            StoreAddressAdmission::Active(StoreJsonDecoderKind::LongTermVersionMaterial)
        );
        assert_eq!(
            classify_store_json_address(LONG_TERM_HEAD_MANIFEST_NAMESPACE, "head:key").unwrap(),
            StoreAddressAdmission::Active(StoreJsonDecoderKind::LongTermHeadManifest)
        );
        assert_eq!(
            classify_store_json_address(LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE, "scope:key")
                .unwrap(),
            StoreAddressAdmission::Active(StoreJsonDecoderKind::LongTermVersionScopeManifest)
        );
        assert_eq!(
            classify_store_json_address(RUNTIME_SKILL_RECORD_NAMESPACE, "skill:key").unwrap(),
            StoreAddressAdmission::Active(StoreJsonDecoderKind::RuntimeSkillOwnerRecord)
        );
        assert_eq!(
            classify_store_json_address(RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE, "scope:key")
                .unwrap(),
            StoreAddressAdmission::Active(StoreJsonDecoderKind::RuntimeSkillScopeManifest)
        );
    }

    #[test]
    fn p8_legacy_json_owners_are_explicitly_forbidden() {
        assert_eq!(
            classify_store_json_address(LEGACY_LONG_TERM_OWNER_NAMESPACE, "legacy-owner").unwrap(),
            StoreAddressAdmission::ForbiddenLegacy(StoreLegacyAddressKind::LongTermOwner)
        );
        assert_eq!(
            classify_store_json_address(
                LEGACY_RUNTIME_SKILL_RECALL_MANIFEST_NAMESPACE,
                "legacy-manifest"
            )
            .unwrap(),
            StoreAddressAdmission::ForbiddenLegacy(
                StoreLegacyAddressKind::RuntimeSkillRecallManifest
            )
        );
        assert_eq!(
            classify_store_json_address("unknown_namespace", "key").unwrap(),
            StoreAddressAdmission::Unknown
        );
    }

    #[test]
    fn generic_skills_blob_rejects_only_legacy_runtime_skill_addresses() {
        assert_eq!(
            classify_store_blob_address("skills", "agent_skill__pdf", Some(b"# Agent skill"))
                .unwrap(),
            StoreAddressAdmission::Active(StoreBlobDecoderKind::GenericSkill)
        );
        assert_eq!(
            classify_store_blob_address(
                "skills",
                "runtime_skill__release",
                Some(b"not a valid runtime record")
            )
            .unwrap(),
            StoreAddressAdmission::ForbiddenLegacy(
                StoreLegacyAddressKind::RuntimeSkillBlobKeyPrefix
            )
        );
        assert_eq!(
            classify_store_blob_address(
                "skills",
                "renamed_runtime_record",
                Some(b" \n<!-- beetle:runtime-skill -->\n# Release")
            )
            .unwrap(),
            StoreAddressAdmission::ForbiddenLegacy(StoreLegacyAddressKind::RuntimeSkillBlobMarker)
        );
        assert_eq!(
            classify_store_blob_address("unknown_blob", "key", Some(b"value")).unwrap(),
            StoreAddressAdmission::Unknown
        );
    }

    #[test]
    fn store_manifest_rejects_unknown_fields() {
        let manifest =
            StoreSchemaManifest::new(StoreBackendKind::InMemory, ProfileId::EspEmbeddedSdk, 1);
        let mut value = serde_json::to_value(manifest).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("legacy_alias".to_string(), serde_json::Value::Bool(true));
        assert!(serde_json::from_value::<StoreSchemaManifest>(value).is_err());
    }
}
