//! Typed, source-owned durable perception evidence. Media bytes remain in a
//! separate blob owner and are never serialized into this JSON document.

use bm_core::memory::{
    ConversationKey, PerceptionEpisodeSourceRef, PerceptionModality, PerceptionReplayOwnerV1,
    PerceptionReplyEvidenceV1, PerceptionRetentionCeiling, PerceptionScope,
    PerceptionSourceDependentEdgeV1, PerceptionSourceDependentKindV1,
    PerceptionSourceDependentsRootV1, PerceptionSourceKey, PerceptionSourceKind,
    PerceptionSourceRef, PerceptionTime, TranscriptExternalContentUseV1, TranscriptTurnRecord,
    VisualObservation,
};
use bm_core::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use crate::store_internal::transaction::BackendTransactionState;
use crate::{
    MemoryStoreEventKind, StoreBlobPrecondition, StoreEventScope, StoreJsonPrecondition,
    StoreMutation, StoreMutationBatch, StoreMutationBatchReport, StorePlatform,
};

pub(crate) const PERCEPTION_EPISODE_NAMESPACE: &str = "perception_episodes";
pub(crate) const PERCEPTION_MEDIA_NAMESPACE: &str = "perception_media_evidence";
pub(crate) const PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE: &str = "perception_episode_tombstones";
pub(crate) const PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE: &str = "perception_source_dependents";
pub(crate) const PERCEPTION_TURN_TAINT_NAMESPACE: &str = "perception_turn_taints";
pub(crate) const PERCEPTION_SOURCE_FENCE_NAMESPACE: &str = "perception_source_fences";
pub(crate) const PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE: &str =
    "perception_source_authorizations";
pub(crate) const PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE: &str =
    "perception_historical_authorizations";
pub(crate) const PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE: &str =
    "perception_source_retention_bans";
pub(crate) const PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE: &str =
    "perception_source_episode_counts";
pub(crate) const PERCEPTION_EPISODE_SCHEMA_VERSION: u32 = 1;
const MAX_EPISODE_ID_LEN: usize = 160;
const MAX_OBSERVATION_CHARS: usize = 4096;
const MAX_DURABLE_MEDIA_BYTES: usize = 8 * 1024 * 1024;

/// Exact Store effects for withdrawing one episode. Callers may compose these
/// with Transcript, long-term and Soul withdrawal before a single commit.
pub(crate) struct PerceptionEpisodeDeletionPlan {
    pub(crate) dependents: PerceptionSourceDependentsRootV1,
    pub(crate) mutations: Vec<StoreMutation>,
    pub(crate) preconditions: Vec<StoreJsonPrecondition>,
    pub(crate) blob_preconditions: Vec<StoreBlobPrecondition>,
}

/// Durable, monotone privacy boundary for one mounted sensory source identity.
/// The identity key excludes generation; a reopened Runtime must advance beyond
/// the last withdrawn generation/epoch pair before it can admit new input.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionSourceFenceV1 {
    pub(crate) schema_version: u32,
    pub(crate) memory_space_id: String,
    pub(crate) subject_id: String,
    pub(crate) relationship_id: Option<String>,
    pub(crate) host_id: String,
    pub(crate) device_id: Option<String>,
    pub(crate) source_id: String,
    pub(crate) revoked_source_generation: u64,
    pub(crate) revoked_epoch: u64,
    /// Permission withdrawal blocks new capture and in-flight delivery for
    /// this generation. Lawfully retained episodes remain readable until
    /// their own source is explicitly deleted or banned.
    pub(crate) permission_revoked_through: Option<PerceptionGenerationEpochV1>,
    pub(crate) updated_at_unix_secs: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionGenerationEpochV1 {
    pub(crate) source_generation: u64,
    pub(crate) revoke_epoch: u64,
}

impl PerceptionSourceFenceV1 {
    fn from_scope(
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
        revoke_epoch: u64,
        now_secs: u64,
    ) -> Result<Self> {
        let fence = Self {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: scope.memory_space_id.clone(),
            subject_id: scope.subject_id.clone(),
            relationship_id: scope.relationship_id.clone(),
            host_id: source.host_id.clone(),
            device_id: source.device_id.clone(),
            source_id: source.source_id.clone(),
            revoked_source_generation: source.source_generation,
            revoked_epoch: revoke_epoch,
            permission_revoked_through: Some(PerceptionGenerationEpochV1 {
                source_generation: source.source_generation,
                revoke_epoch,
            }),
            updated_at_unix_secs: now_secs,
        };
        fence.validate_contract()?;
        Ok(fence)
    }

    fn from_episode_source(source: &PerceptionEpisodeSourceRef, now_secs: u64) -> Result<Self> {
        let fence = Self {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: source.memory_space_id.clone(),
            subject_id: source.subject_id.clone(),
            relationship_id: source.relationship_id.clone(),
            host_id: source.source.host_id.clone(),
            device_id: source.source.device_id.clone(),
            source_id: source.source.source_id.clone(),
            revoked_source_generation: source.source.source_generation,
            revoked_epoch: source.revoke_epoch,
            permission_revoked_through: None,
            updated_at_unix_secs: now_secs,
        };
        fence.validate_contract()?;
        Ok(fence)
    }
    pub(crate) fn key(&self) -> Result<String> {
        perception_source_fence_key(
            &self.memory_space_id,
            &self.subject_id,
            self.relationship_id.as_deref(),
            &PerceptionSourceKey {
                host_id: self.host_id.clone(),
                device_id: self.device_id.clone(),
                source_id: self.source_id.clone(),
                source_generation: self.revoked_source_generation,
            },
        )
    }

    pub(crate) fn validate_contract(&self) -> Result<()> {
        if self.schema_version != PERCEPTION_EPISODE_SCHEMA_VERSION
            || self.updated_at_unix_secs == 0
            || self.memory_space_id.trim().is_empty()
            || self.subject_id.trim().is_empty()
            || self
                .relationship_id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty())
            || self
                .permission_tuple()
                .is_some_and(|tuple| tuple > self.revoked_tuple())
        {
            return Err(invalid(
                "perception source fence scope or version is invalid",
            ));
        }
        self.key()?;
        Ok(())
    }

    fn revoked_tuple(&self) -> (u64, u64) {
        (self.revoked_source_generation, self.revoked_epoch)
    }

    fn permission_tuple(&self) -> Option<(u64, u64)> {
        self.permission_revoked_through
            .map(|value| (value.source_generation, value.revoke_epoch))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionSourceAuthorizationV1 {
    pub(crate) schema_version: u32,
    pub(crate) memory_space_id: String,
    pub(crate) subject_id: String,
    pub(crate) relationship_id: Option<String>,
    pub(crate) source: PerceptionSourceKey,
    pub(crate) source_kind: PerceptionSourceKind,
    pub(crate) revoke_epoch: u64,
    pub(crate) authorization_generation: u64,
    pub(crate) retention_ceiling: PerceptionRetentionCeiling,
    pub(crate) user_consent_attested: bool,
    pub(crate) policy_manifest_revision: u64,
    pub(crate) policy_manifest_digest: Option<String>,
    pub(crate) updated_at_unix_secs: u64,
}

impl PerceptionSourceAuthorizationV1 {
    pub(crate) fn key(&self) -> Result<String> {
        perception_source_fence_key(
            &self.memory_space_id,
            &self.subject_id,
            self.relationship_id.as_deref(),
            &self.source,
        )
    }

    pub(crate) fn validate_contract(&self) -> Result<()> {
        if self.schema_version != PERCEPTION_EPISODE_SCHEMA_VERSION
            || self.authorization_generation == 0
            || self.updated_at_unix_secs == 0
            || self.memory_space_id.trim().is_empty()
            || self.subject_id.trim().is_empty()
            || self
                .relationship_id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty())
            || (self.policy_manifest_revision == 0) != self.policy_manifest_digest.is_none()
        {
            return Err(invalid("perception source authorization is invalid"));
        }
        if let Some(digest) = &self.policy_manifest_digest {
            require_sha256(digest, "policy_manifest_digest")?;
        }
        self.key()?;
        Ok(())
    }

    fn tuple(&self) -> (u64, u64) {
        (self.source.source_generation, self.revoke_epoch)
    }
}

/// Historical authority for one already admitted episode. This owner is never
/// a credential for capture, new episode intake, or a higher retention ceiling.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionHistoricalAuthorizationV1 {
    pub(crate) schema_version: u32,
    pub(crate) source: PerceptionEpisodeSourceRef,
    pub(crate) source_kind: PerceptionSourceKind,
    pub(crate) authorization_generation: u64,
    pub(crate) retention_ceiling: PerceptionRetentionCeiling,
    pub(crate) user_consent_attested: bool,
    pub(crate) policy_manifest_revision: u64,
    pub(crate) policy_manifest_digest: Option<String>,
    pub(crate) authorized_at_unix_secs: u64,
}

impl PerceptionHistoricalAuthorizationV1 {
    pub(crate) fn key(&self) -> Result<String> {
        perception_episode_storage_key(
            &self.source.memory_space_id,
            &self.source.subject_id,
            &self.source.source,
            &self.source.episode_id,
        )
    }

    pub(crate) fn validate_contract(&self) -> Result<()> {
        self.source.validate_contract()?;
        if self.schema_version != PERCEPTION_EPISODE_SCHEMA_VERSION
            || self.authorization_generation == 0
            || !self.retention_ceiling.permits_durable_semantic()
            || !self.user_consent_attested
            || self.authorized_at_unix_secs == 0
            || (self.policy_manifest_revision == 0) != self.policy_manifest_digest.is_none()
        {
            return Err(invalid("historical visual authorization is invalid"));
        }
        if let Some(digest) = &self.policy_manifest_digest {
            require_sha256(digest, "historical policy_manifest_digest")?;
        }
        self.key()?;
        Ok(())
    }

    pub(crate) fn validate_episode(&self, episode: &PerceptionEpisodeEvidenceV1) -> Result<()> {
        self.validate_contract()?;
        episode.validate_contract()?;
        if self.source != episode.source_ref()?
            || self.source_kind != episode.source_kind
            || self.authorization_generation != episode.authorization_generation
            || episode.retention_ceiling > self.retention_ceiling
        {
            return Err(invalid("historical authorization differs from episode"));
        }
        Ok(())
    }
}

pub(crate) fn project_perception_historical_authorization(
    episode: &PerceptionEpisodeEvidenceV1,
    active: &PerceptionSourceAuthorizationV1,
) -> Result<PerceptionHistoricalAuthorizationV1> {
    episode.validate_contract()?;
    active.validate_contract()?;
    if active.memory_space_id != episode.memory_space_id
        || active.subject_id != episode.subject_id
        || active.relationship_id != episode.relationship_id
        || active.source != episode.source
        || active.source_kind != episode.source_kind
        || active.revoke_epoch != episode.revoke_epoch
        || active.authorization_generation != episode.authorization_generation
        || episode.retention_ceiling > active.retention_ceiling
        || !active.user_consent_attested
    {
        return Err(invalid(
            "active authorization cannot attest historical episode",
        ));
    }
    let historical = PerceptionHistoricalAuthorizationV1 {
        schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
        source: episode.source_ref()?,
        source_kind: active.source_kind,
        authorization_generation: active.authorization_generation,
        retention_ceiling: active.retention_ceiling,
        user_consent_attested: active.user_consent_attested,
        policy_manifest_revision: active.policy_manifest_revision,
        policy_manifest_digest: active.policy_manifest_digest.clone(),
        authorized_at_unix_secs: active.updated_at_unix_secs,
    };
    historical.validate_episode(episode)?;
    Ok(historical)
}

fn historical_authorization_for_new_episode(
    platform: &StorePlatform,
    episode: &PerceptionEpisodeEvidenceV1,
) -> Result<PerceptionHistoricalAuthorizationV1> {
    let identity = perception_source_fence_key(
        &episode.memory_space_id,
        &episode.subject_id,
        episode.relationship_id.as_deref(),
        &episode.source,
    )?;
    let value = platform
        .read_json_docs_by_keys(PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE, &[identity])?
        .into_iter()
        .next()
        .ok_or_else(|| invalid("new episode lacks current source authorization"))?
        .value;
    let active: PerceptionSourceAuthorizationV1 =
        serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
    project_perception_historical_authorization(episode, &active)
}

/// A default archive carries visual semantics and provenance, never media bytes
/// or a live capture credential. Unrelated namespaces are handled elsewhere.
pub(crate) fn project_perception_semantic_archive_doc(
    namespace: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<Option<serde_json::Value>> {
    let projected = match namespace {
        PERCEPTION_EPISODE_NAMESPACE => {
            let mut episode: PerceptionEpisodeEvidenceV1 = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            episode.validate_contract()?;
            if episode.key()? != key {
                return Err(invalid("archive episode key mismatch"));
            }
            episode.media = None;
            serde_json::to_value(episode).map_err(|error| invalid(error.to_string()))?
        }
        PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE => {
            let root: PerceptionSourceDependentsRootV1 = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            root.validate_contract()?;
            if perception_episode_storage_key(
                &root.source.memory_space_id,
                &root.source.subject_id,
                &root.source.source,
                &root.source.episode_id,
            )? != key
            {
                return Err(invalid("archive source root key mismatch"));
            }
            let edges = root
                .edges
                .into_iter()
                .filter(|edge| edge.kind != PerceptionSourceDependentKindV1::MediaEvidence)
                .collect();
            let projected = PerceptionSourceDependentsRootV1::new(root.source, edges)?;
            serde_json::to_value(projected).map_err(|error| invalid(error.to_string()))?
        }
        PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE => {
            let historical: PerceptionHistoricalAuthorizationV1 =
                serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
            historical.validate_contract()?;
            if historical.key()? != key {
                return Err(invalid("archive historical authorization key mismatch"));
            }
            value.clone()
        }
        PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE => {
            let count: PerceptionSourceEpisodeCountV1 = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            count.validate_contract()?;
            if count.key()? != key {
                return Err(invalid("archive source count key mismatch"));
            }
            value.clone()
        }
        PERCEPTION_TURN_TAINT_NAMESPACE => {
            let taint: PerceptionTurnTaintV1 = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            taint.validate_contract()?;
            if taint.key()? != key {
                return Err(invalid("archive turn taint key mismatch"));
            }
            value.clone()
        }
        _ => return Ok(None),
    };
    Ok(Some(projected))
}

/// Procedural/private archive redaction may change Transcript or long-term
/// JSON digests. Rebuild only the visual reverse edges against the final
/// disclosed owners, then let the full image gate verify the entire graph.
pub(crate) fn rebuild_perception_archive_roots(snapshot: &mut crate::StoreSnapshot) -> Result<()> {
    let mut documents = BTreeMap::new();
    for document in &snapshot.json_docs {
        if documents
            .insert(
                (document.namespace.clone(), document.key.clone()),
                document.value.clone(),
            )
            .is_some()
        {
            return Err(invalid("archive contains duplicate JSON owner"));
        }
    }
    let mut episodes = BTreeMap::<String, PerceptionEpisodeEvidenceV1>::new();
    let carries_soul_owner = documents.keys().any(|(namespace, _)| {
        crate::store_internal::schema::is_subject_soul_protected_json_namespace(namespace)
    });
    for ((namespace, key), value) in &documents {
        if namespace != PERCEPTION_EPISODE_NAMESPACE {
            continue;
        }
        let episode: PerceptionEpisodeEvidenceV1 =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        episode.validate_contract()?;
        if episode.key()? != *key {
            return Err(invalid("archive episode key mismatch"));
        }
        episodes.insert(key.clone(), episode);
    }
    for ((namespace, _), value) in &documents {
        if namespace == "conversation_transcript" {
            let record: TranscriptTurnRecord = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            for usage in &record.perception_source_uses {
                let source = &usage.source;
                let key = perception_episode_storage_key(
                    &source.memory_space_id,
                    &source.subject_id,
                    &source.source,
                    &source.episode_id,
                )?;
                if episodes
                    .get(&key)
                    .is_none_or(|episode| episode.source_ref().ok().as_ref() != Some(source))
                {
                    return Err(invalid(
                        "archive Transcript retains a missing visual source",
                    ));
                }
            }
        } else if namespace == crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE {
            let material: bm_core::memory::LongTermMemoryVersionMaterial =
                serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
            for source in &material.governed_content.perception_sources {
                let key = perception_episode_storage_key(
                    &source.memory_space_id,
                    &source.subject_id,
                    &source.source,
                    &source.episode_id,
                )?;
                if episodes
                    .get(&key)
                    .is_none_or(|episode| episode.source_ref().ok().as_ref() != Some(source))
                {
                    return Err(invalid(
                        "archive long-term owner retains a missing visual source",
                    ));
                }
            }
        }
    }
    let mut rebuilt = BTreeMap::new();
    for (key, episode) in &episodes {
        let source = episode.source_ref()?;
        let root_value = documents
            .get(&(PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), key.clone()))
            .ok_or_else(|| invalid("archive episode lacks source root"))?;
        let old: PerceptionSourceDependentsRootV1 = serde_json::from_value(root_value.clone())
            .map_err(|error| invalid(error.to_string()))?;
        old.validate_contract()?;
        if old.source != source
            || old.edges.iter().any(|edge| {
                match edge.kind {
                    PerceptionSourceDependentKindV1::MediaEvidence
                    | PerceptionSourceDependentKindV1::TranscriptTurn
                    | PerceptionSourceDependentKindV1::LongTermOwnerRevision => false,
                    // Ordinary memory-space archives exclude Soul material.
                    // Its reverse edges must not claim an owner absent here.
                    PerceptionSourceDependentKindV1::SoulRoot
                    | PerceptionSourceDependentKindV1::SoulRevision => carries_soul_owner,
                    _ => true,
                }
            })
        {
            return Err(invalid(
                "archive source root has unsupported or mismatched lineage",
            ));
        }
        let mut edges = Vec::new();
        if let Some(media) = &episode.media {
            edges.push(PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::MediaEvidence,
                memory_space_id: source.memory_space_id.clone(),
                subject_id: source.subject_id.clone(),
                relationship_id: source.relationship_id.clone(),
                owner_key: media.key.clone(),
                owner_revision: None,
                content_digest: media.sha256.clone(),
            });
        }
        for ((namespace, owner_key), value) in &documents {
            if namespace == "conversation_transcript" {
                let record: TranscriptTurnRecord = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                if record
                    .perception_source_uses
                    .iter()
                    .any(|usage| usage.source == source)
                {
                    edges.push(PerceptionSourceDependentEdgeV1 {
                        kind: PerceptionSourceDependentKindV1::TranscriptTurn,
                        memory_space_id: source.memory_space_id.clone(),
                        subject_id: source.subject_id.clone(),
                        relationship_id: source.relationship_id.clone(),
                        owner_key: owner_key.clone(),
                        owner_revision: Some(record.sequence),
                        content_digest: format!(
                            "sha256:{:x}",
                            Sha256::digest(
                                serde_json::to_vec(&record)
                                    .map_err(|error| invalid(error.to_string()))?
                            )
                        ),
                    });
                }
            } else if namespace == crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE {
                let material: bm_core::memory::LongTermMemoryVersionMaterial =
                    serde_json::from_value(value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                if material
                    .governed_content
                    .perception_sources
                    .contains(&source)
                {
                    edges.push(PerceptionSourceDependentEdgeV1 {
                        kind: PerceptionSourceDependentKindV1::LongTermOwnerRevision,
                        memory_space_id: source.memory_space_id.clone(),
                        subject_id: source.subject_id.clone(),
                        relationship_id: source.relationship_id.clone(),
                        owner_key: owner_key.clone(),
                        owner_revision: Some(material.owner_revision),
                        content_digest: format!("sha256:{}", material.content_digest),
                    });
                }
            }
        }
        let root = PerceptionSourceDependentsRootV1::new(source, edges)?;
        rebuilt.insert(
            key.clone(),
            serde_json::to_value(root).map_err(|error| invalid(error.to_string()))?,
        );
    }
    for document in &mut snapshot.json_docs {
        if document.namespace == PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE {
            document.value = rebuilt
                .remove(&document.key)
                .ok_or_else(|| invalid("archive has orphan visual source root"))?;
        }
    }
    if !rebuilt.is_empty() {
        return Err(invalid(
            "archive failed to rebuild every visual source root",
        ));
    }
    Ok(())
}

/// A source-identity retention ceiling survives camera generation changes.
/// Explicit unban is a separate governed operation; registration cannot override it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionSourceRetentionBanV1 {
    pub(crate) schema_version: u32,
    pub(crate) memory_space_id: String,
    pub(crate) subject_id: String,
    pub(crate) relationship_id: Option<String>,
    pub(crate) host_id: String,
    pub(crate) device_id: Option<String>,
    pub(crate) source_id: String,
    pub(crate) policy_revision: u64,
    pub(crate) retention_ceiling: PerceptionRetentionCeiling,
    pub(crate) updated_at_unix_secs: u64,
}

impl PerceptionSourceRetentionBanV1 {
    pub(crate) fn key(&self) -> Result<String> {
        perception_source_fence_key(
            &self.memory_space_id,
            &self.subject_id,
            self.relationship_id.as_deref(),
            &PerceptionSourceKey {
                host_id: self.host_id.clone(),
                device_id: self.device_id.clone(),
                source_id: self.source_id.clone(),
                source_generation: 1,
            },
        )
    }

    pub(crate) fn validate_contract(&self) -> Result<()> {
        if self.schema_version != PERCEPTION_EPISODE_SCHEMA_VERSION
            || self.policy_revision == 0
            || self.updated_at_unix_secs == 0
            || self.retention_ceiling.permits_durable_semantic()
            || self.memory_space_id.trim().is_empty()
            || self.subject_id.trim().is_empty()
            || self
                .relationship_id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty())
        {
            return Err(invalid("perception source retention ban is invalid"));
        }
        self.key()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionSourceEpisodeCountV1 {
    pub(crate) schema_version: u32,
    pub(crate) memory_space_id: String,
    pub(crate) subject_id: String,
    pub(crate) relationship_id: Option<String>,
    pub(crate) host_id: String,
    pub(crate) device_id: Option<String>,
    pub(crate) source_id: String,
    pub(crate) revision: u64,
    pub(crate) active_count: u64,
}

impl PerceptionSourceEpisodeCountV1 {
    pub(crate) fn key(&self) -> Result<String> {
        perception_source_fence_key(
            &self.memory_space_id,
            &self.subject_id,
            self.relationship_id.as_deref(),
            &PerceptionSourceKey {
                host_id: self.host_id.clone(),
                device_id: self.device_id.clone(),
                source_id: self.source_id.clone(),
                source_generation: 1,
            },
        )
    }

    pub(crate) fn validate_contract(&self) -> Result<()> {
        if self.schema_version != PERCEPTION_EPISODE_SCHEMA_VERSION
            || self.revision == 0
            || self.memory_space_id.trim().is_empty()
            || self.subject_id.trim().is_empty()
        {
            return Err(invalid("perception episode count owner is invalid"));
        }
        self.key()?;
        Ok(())
    }

    fn from_episode(episode: &PerceptionEpisodeEvidenceV1, revision: u64, count: u64) -> Self {
        Self {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: episode.memory_space_id.clone(),
            subject_id: episode.subject_id.clone(),
            relationship_id: episode.relationship_id.clone(),
            host_id: episode.source.host_id.clone(),
            device_id: episode.source.device_id.clone(),
            source_id: episode.source.source_id.clone(),
            revision,
            active_count: count,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionMediaOwnerRefV1 {
    pub(crate) key: String,
    pub(crate) sha256: String,
    pub(crate) mime: String,
    pub(crate) byte_len: usize,
}

/// The SDK creates this only after checking the host's exact final request
/// bytes against its sealed packet. This is a host attestation, not a claim
/// about Provider-internal image transforms or model attention.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionDeliveryEvidenceV1 {
    pub(crate) input_id: String,
    pub(crate) packet_id: String,
    pub(crate) turn_id: String,
    pub(crate) final_request_sha256: String,
    pub(crate) sent_media_sha256: String,
    pub(crate) receipt_sha256: String,
}

impl PerceptionDeliveryEvidenceV1 {
    pub(crate) fn validate_contract(&self) -> Result<()> {
        for (name, value) in [
            ("input_id", self.input_id.as_str()),
            ("packet_id", self.packet_id.as_str()),
            ("turn_id", self.turn_id.as_str()),
        ] {
            require_nonempty(value, name)?;
        }
        for (name, digest) in [
            ("final_request_sha256", &self.final_request_sha256),
            ("sent_media_sha256", &self.sent_media_sha256),
            ("receipt_sha256", &self.receipt_sha256),
        ] {
            require_sha256(digest, name)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionEpisodeEvidenceV1 {
    pub(crate) schema_version: u32,
    pub(crate) memory_space_id: String,
    pub(crate) subject_id: String,
    pub(crate) relationship_id: Option<String>,
    pub(crate) source: PerceptionSourceKey,
    pub(crate) revoke_epoch: u64,
    pub(crate) authorization_generation: u64,
    pub(crate) episode_id: String,
    pub(crate) input_id: String,
    pub(crate) modality: PerceptionModality,
    pub(crate) source_kind: PerceptionSourceKind,
    pub(crate) origin_ref: PerceptionSourceRef,
    pub(crate) time: PerceptionTime,
    pub(crate) retention_ceiling: PerceptionRetentionCeiling,
    pub(crate) audio_free_video_sha256: Option<String>,
    pub(crate) observation: Option<VisualObservation>,
    pub(crate) delivery: Option<PerceptionDeliveryEvidenceV1>,
    pub(crate) media: Option<PerceptionMediaOwnerRefV1>,
}

#[derive(Clone, Debug)]
pub(crate) struct PerceptionTurnEpisodeIntakeV1 {
    pub(crate) episode: PerceptionEpisodeEvidenceV1,
    pub(crate) media_bytes: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionEpisodeTombstoneV1 {
    pub(crate) schema_version: u32,
    pub(crate) memory_space_id: String,
    pub(crate) subject_id: String,
    pub(crate) source: PerceptionSourceKey,
    pub(crate) revoke_epoch: u64,
    pub(crate) episode_id: String,
    pub(crate) withdrawn_content_digest: String,
    pub(crate) deleted_at_unix_secs: u64,
}

/// Content-free marker that prevents a previously visual turn from being
/// retried as an ordinary text turn after media TTL or process restart.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerceptionTurnTaintV1 {
    pub(crate) schema_version: u32,
    pub(crate) memory_space_id: String,
    pub(crate) subject_id: String,
    pub(crate) session_id: String,
    pub(crate) turn_id: String,
    pub(crate) source_commitments: Vec<String>,
    pub(crate) retention_ceiling: PerceptionRetentionCeiling,
    pub(crate) first_seen_at_unix_secs: u64,
    pub(crate) updated_at_unix_secs: u64,
}

pub(crate) struct PerceptionTurnTaintInput<'a> {
    pub(crate) source: &'a PerceptionSourceKey,
    pub(crate) input_id: &'a str,
    pub(crate) revoke_epoch: u64,
    pub(crate) retention_ceiling: PerceptionRetentionCeiling,
}

impl PerceptionTurnTaintV1 {
    pub(crate) fn key(&self) -> Result<String> {
        perception_turn_taint_key(
            &self.memory_space_id,
            &self.subject_id,
            &self.session_id,
            &self.turn_id,
        )
    }

    pub(crate) fn validate_contract(&self) -> Result<()> {
        if self.schema_version != PERCEPTION_EPISODE_SCHEMA_VERSION
            || self.first_seen_at_unix_secs == 0
            || self.updated_at_unix_secs < self.first_seen_at_unix_secs
            || self.source_commitments.is_empty()
            || self.source_commitments.len() > 64
            || self
                .source_commitments
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(invalid("perception turn taint shape is invalid"));
        }
        self.key()?;
        for digest in &self.source_commitments {
            require_sha256(digest, "source_commitment")?;
        }
        Ok(())
    }
}

impl PerceptionEpisodeTombstoneV1 {
    pub(crate) fn validate_contract(&self) -> Result<()> {
        if self.schema_version != PERCEPTION_EPISODE_SCHEMA_VERSION
            || self.deleted_at_unix_secs == 0
        {
            return Err(invalid("episode tombstone version or time is invalid"));
        }
        require_sha256(&self.withdrawn_content_digest, "withdrawn_content_digest")?;
        perception_episode_storage_key(
            &self.memory_space_id,
            &self.subject_id,
            &self.source,
            &self.episode_id,
        )?;
        Ok(())
    }

    pub(crate) fn key(&self) -> Result<String> {
        self.validate_contract()?;
        perception_episode_storage_key(
            &self.memory_space_id,
            &self.subject_id,
            &self.source,
            &self.episode_id,
        )
    }
}

impl PerceptionEpisodeEvidenceV1 {
    pub(crate) fn key(&self) -> Result<String> {
        perception_episode_storage_key(
            &self.memory_space_id,
            &self.subject_id,
            &self.source,
            &self.episode_id,
        )
    }

    pub(crate) fn content_digest(&self) -> Result<String> {
        self.validate_contract()?;
        bm_core::memory::perception_episode_semantic_digest(self)
    }

    pub(crate) fn source_ref(&self) -> Result<PerceptionEpisodeSourceRef> {
        self.validate_contract()?;
        Ok(PerceptionEpisodeSourceRef {
            memory_space_id: self.memory_space_id.clone(),
            subject_id: self.subject_id.clone(),
            relationship_id: self.relationship_id.clone(),
            source: self.source.clone(),
            revoke_epoch: self.revoke_epoch,
            episode_id: self.episode_id.clone(),
            content_digest: self.content_digest()?,
        })
    }

    pub(crate) fn validate_contract(&self) -> Result<()> {
        if self.schema_version != PERCEPTION_EPISODE_SCHEMA_VERSION {
            return Err(invalid("unsupported episode schema version"));
        }
        if self.authorization_generation == 0 {
            return Err(invalid("episode authorization generation is zero"));
        }
        self.source.validate_contract()?;
        bm_core::memory::PerceptionSource {
            key: self.source.clone(),
            kind: self.source_kind,
            source_ref: self.origin_ref.clone(),
        }
        .validate_contract()?;
        if let Some(digest) = &self.audio_free_video_sha256 {
            require_sha256(digest, "audio_free_video_sha256")?;
        }
        for (name, value) in [
            ("memory_space_id", self.memory_space_id.as_str()),
            ("subject_id", self.subject_id.as_str()),
            ("episode_id", self.episode_id.as_str()),
            ("input_id", self.input_id.as_str()),
        ] {
            if value.is_empty() || value.len() > MAX_EPISODE_ID_LEN {
                return Err(invalid(format!("{name} is empty or too long")));
            }
        }
        if self
            .relationship_id
            .as_ref()
            .is_some_and(|id| id.is_empty())
        {
            return Err(invalid("relationship_id cannot be empty"));
        }
        if self.modality != PerceptionModality::Visual
            || !matches!(
                self.source_kind,
                PerceptionSourceKind::HostVisualObservation
                    | PerceptionSourceKind::HumanUserSharedMedia
            )
        {
            return Err(invalid("durable episode must be typed visual input"));
        }
        if self.source_kind == PerceptionSourceKind::HostVisualObservation
            && self.source.device_id.is_none()
        {
            return Err(invalid("host visual source requires device_id"));
        }
        if self.retention_ceiling < PerceptionRetentionCeiling::DurableSemantic {
            return Err(invalid("ephemeral input cannot create a durable episode"));
        }
        if self.observation.is_none() && self.delivery.is_none() {
            return Err(invalid(
                "episode needs a visual observation or verified delivery",
            ));
        }
        if let Some(delivery) = &self.delivery {
            delivery.validate_contract()?;
            if delivery.input_id != self.input_id {
                return Err(invalid("delivery input differs from episode"));
            }
        }
        if let Some(observation) = &self.observation {
            observation.validate_contract()?;
            if observation.scope.memory_space_id != self.memory_space_id
                || observation.scope.subject_id != self.subject_id
                || observation.scope.relationship_id != self.relationship_id
                || observation.source_key != self.source
                || observation.revoke_epoch != self.revoke_epoch
                || observation.observed_at_unix_ms != self.time.observed_at_unix_ms
                || observation.input_id != self.input_id
                || self
                    .delivery
                    .as_ref()
                    .is_some_and(|delivery| delivery.input_id != observation.input_id)
            {
                return Err(invalid("observation differs from exact episode owner"));
            }
            if let bm_core::memory::VisualEvidenceOrigin::HostAttestedReply {
                delivery_receipt_digest,
                ..
            } = &observation.evidence_origin
            {
                if self
                    .delivery
                    .as_ref()
                    .is_none_or(|delivery| delivery.receipt_sha256 != *delivery_receipt_digest)
                {
                    return Err(invalid("reply observation lacks matching delivery proof"));
                }
            }
        }
        if self.time.presented_at_unix_ms == 0
            || self
                .time
                .observed_at_unix_ms
                .is_some_and(|at| at > self.time.presented_at_unix_ms)
        {
            return Err(invalid("episode time is invalid"));
        }
        if self
            .observation
            .as_ref()
            .is_some_and(|observation| observation.text.chars().count() > MAX_OBSERVATION_CHARS)
        {
            return Err(invalid("episode observation exceeds durable budget"));
        }
        if let Some(observation) = &self.observation {
            if observation.classification.sensitivity
                != bm_core::memory::VisualSensitivity::Ordinary
                || observation.classification.minor_present
                || observation.classification.sensitive_document_present
                || observation.classification.conflicting_evidence
                || !(matches!(
                    observation.classification.subject_attribution,
                    bm_core::memory::SubjectAttribution::VerifiedCurrentUser
                        | bm_core::memory::SubjectAttribution::ContextualCurrentUser
                ) || self.source_kind == PerceptionSourceKind::HumanUserSharedMedia
                    && observation
                        .classification
                        .ordinary_nonhuman_semantic_allowed())
            {
                return Err(invalid("sensitive or unbound observation cannot persist"));
            }
        }
        if let Some(media) = &self.media {
            if self.retention_ceiling != PerceptionRetentionCeiling::DurableMedia
                || self.observation.as_ref().is_none_or(|observation| {
                    !observation
                        .classification
                        .media_evidence_allowed_for_source(self.source_kind)
                })
                || media.key != perception_media_storage_key(&self.key()?)
                || media.byte_len == 0
                || media.byte_len > MAX_DURABLE_MEDIA_BYTES
            {
                return Err(invalid("media owner exceeds retention or privacy gate"));
            }
            require_sha256(&media.sha256, "media.sha256")?;
            require_nonempty(&media.mime, "media.mime")?;
            if media.mime.starts_with("video/")
                && self.audio_free_video_sha256.as_deref() != Some(media.sha256.as_str())
            {
                return Err(invalid(
                    "durable video lacks exact audio-free host attestation",
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn perception_episode_storage_key(
    memory_space_id: &str,
    subject_id: &str,
    source: &PerceptionSourceKey,
    episode_id: &str,
) -> Result<String> {
    source.validate_contract()?;
    for value in [memory_space_id, subject_id, episode_id] {
        if value.is_empty() || value.len() > MAX_EPISODE_ID_LEN {
            return Err(invalid(
                "episode address contains an empty or overlong component",
            ));
        }
    }
    let bytes = serde_json::to_vec(&(memory_space_id, subject_id, source, episode_id))
        .map_err(|error| Error::invalid_input("perception_episode_key", error.to_string()))?;
    Ok(format!("episode:sha256:{:x}", Sha256::digest(bytes)))
}

pub(crate) fn perception_source_fence_key(
    memory_space_id: &str,
    subject_id: &str,
    relationship_id: Option<&str>,
    source: &PerceptionSourceKey,
) -> Result<String> {
    source.validate_contract()?;
    for value in [
        memory_space_id,
        subject_id,
        source.host_id.as_str(),
        source.source_id.as_str(),
    ] {
        require_nonempty(value, "source fence identity")?;
    }
    if relationship_id.is_some_and(|id| id.trim().is_empty()) {
        return Err(invalid("source fence relationship is empty"));
    }
    let identity = (
        memory_space_id,
        subject_id,
        relationship_id,
        source.host_id.as_str(),
        source.device_id.as_deref(),
        source.source_id.as_str(),
    );
    let bytes = serde_json::to_vec(&identity).map_err(|error| invalid(error.to_string()))?;
    Ok(format!("source-fence:sha256:{:x}", Sha256::digest(bytes)))
}

pub(crate) fn perception_media_storage_key(episode_key: &str) -> String {
    format!("media:{episode_key}")
}

fn policy_snapshot_from_precondition(
    precondition: &StoreJsonPrecondition,
) -> Result<(u64, Option<String>)> {
    match precondition {
        StoreJsonPrecondition::Absent { .. } => Ok((0, None)),
        StoreJsonPrecondition::Exact { value, .. } => {
            let manifest: crate::store_internal::schema::ControlPlaneScopeManifest =
                serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
            Ok((manifest.revision, Some(manifest.entries_digest)))
        }
    }
}

fn policy_snapshot_from_state(
    state: &BackendTransactionState,
    memory_space_id: &str,
) -> Result<(u64, Option<String>)> {
    let key = crate::store_internal::schema::control_plane_scope_manifest_key(
        memory_space_id,
        memory_space_id,
    )?;
    let Some(value) = state.json.get(&(
        crate::store_internal::schema::CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.into(),
        key.clone(),
    )) else {
        return Ok((0, None));
    };
    let manifest: crate::store_internal::schema::ControlPlaneScopeManifest =
        serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
    if manifest.physical_key != key
        || manifest.memory_space_id != memory_space_id
        || manifest.mounted_subject_id != memory_space_id
    {
        return Err(invalid("policy manifest scope mismatch"));
    }
    Ok((manifest.revision, Some(manifest.entries_digest)))
}

fn broad_visual_retention_blocked_by_policy(
    state: &BackendTransactionState,
    memory_space_id: &str,
    subject_id: &str,
    at_unix_secs: u64,
) -> Result<bool> {
    use crate::store_internal::schema::{
        control_plane_scope_manifest_key, ControlPlaneScopeManifest,
        CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
    };
    let key = control_plane_scope_manifest_key(memory_space_id, memory_space_id)?;
    let Some(value) = state
        .json
        .get(&(CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.into(), key))
    else {
        return Ok(false);
    };
    let manifest: ControlPlaneScopeManifest =
        serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
    for entry in manifest
        .entries
        .iter()
        .filter(|entry| entry.namespace == bm_core::memory::LONG_TERM_GOVERNANCE_POLICY_NAMESPACE)
    {
        let value = state
            .json
            .get(&(entry.namespace.clone(), entry.key.clone()))
            .ok_or_else(|| invalid("visual policy manifest has unread policy"))?;
        entry.validate_value(value)?;
        let policy: bm_core::memory::MemoryLongTermGovernancePolicy =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        policy.validate_contract()?;
        if !matches!(policy.kind.as_str(), "pause" | "suppress")
            || !policy.effective_at(at_unix_secs)
            || policy.selector.kind.is_some()
            || policy.selector.topic_pattern.is_some()
            || policy.selector.source_chat_id.is_some()
            || policy.selector.source_scope.is_some()
        {
            continue;
        }
        if policy.matches_candidate(
            Some(memory_space_id),
            Some(subject_id),
            &bm_core::memory::LongTermMemoryKind::Fact,
            "",
            None,
            bm_core::memory::LongTermMemorySourceScope::User,
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn replay_owner_addresses(
    owner: &PerceptionReplayOwnerV1,
    source: &PerceptionEpisodeSourceRef,
) -> Result<Vec<(String, String)>> {
    match owner {
        PerceptionReplayOwnerV1::TranscriptMessage { reference } => {
            let key = ConversationKey::new(
                &reference.memory_space_id,
                &reference.channel_id,
                &reference.conversation_id,
            )?;
            Ok(vec![(
                "conversation_transcript".into(),
                crate::store_internal::platform::transcript_turn_storage_key(
                    &key,
                    &source.subject_id,
                    &reference.turn_id,
                ),
            )])
        }
        PerceptionReplayOwnerV1::LongTermMemory {
            owner_id,
            owner_revision,
        } => {
            let owner_ref = bm_core::memory::GovernedMemoryOwnerRef::new(
                bm_core::memory::GovernedMemoryOwnerPlane::LongTerm,
                owner_id.clone(),
            );
            Ok(vec![
                (
                    crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE.into(),
                    bm_core::memory::long_term_version_head_key(
                        &source.memory_space_id,
                        &source.memory_space_id,
                        &owner_ref,
                    )?,
                ),
                (
                    crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE.into(),
                    bm_core::memory::long_term_version_material_key(
                        &source.memory_space_id,
                        &source.memory_space_id,
                        &owner_ref,
                        *owner_revision,
                    )?,
                ),
            ])
        }
    }
}

/// Root updates are validated against the immutable episode and every retained
/// JSON dependent. The transaction backend reads only declared addresses, so
/// these owners must be added to its read graph before post-image validation.
pub(crate) fn perception_transaction_dependency_addresses(
    namespace: &str,
    value: &serde_json::Value,
) -> Result<Vec<(String, String)>> {
    if namespace == "conversation_transcript" {
        let record: TranscriptTurnRecord =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        let mut addresses = Vec::new();
        for usage in &record.perception_source_uses {
            let source = &usage.source;
            let episode_key = perception_episode_storage_key(
                &source.memory_space_id,
                &source.subject_id,
                &source.source,
                &source.episode_id,
            )?;
            addresses.extend([
                (PERCEPTION_EPISODE_NAMESPACE.into(), episode_key.clone()),
                (
                    PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(),
                    episode_key.clone(),
                ),
                (
                    PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                    episode_key.clone(),
                ),
                (PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), episode_key),
            ]);
            if let PerceptionReplyEvidenceV1::ReplayedOwner { owner } = &usage.delivery {
                addresses.extend(replay_owner_addresses(owner, source)?);
            }
        }
        return Ok(addresses);
    }
    if namespace == crate::store_internal::schema::CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE {
        let manifest: crate::store_internal::schema::ControlPlaneScopeManifest =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        return Ok(manifest
            .entries
            .into_iter()
            .filter(|entry| {
                entry.namespace == bm_core::memory::LONG_TERM_GOVERNANCE_POLICY_NAMESPACE
            })
            .map(|entry| (entry.namespace, entry.key))
            .collect());
    }
    if namespace == PERCEPTION_EPISODE_NAMESPACE {
        let episode: PerceptionEpisodeEvidenceV1 =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        episode.validate_contract()?;
        let identity_key = perception_source_fence_key(
            &episode.memory_space_id,
            &episode.subject_id,
            episode.relationship_id.as_deref(),
            &episode.source,
        )?;
        return Ok(vec![
            (
                PERCEPTION_SOURCE_FENCE_NAMESPACE.into(),
                identity_key.clone(),
            ),
            (
                PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                episode.key()?,
            ),
            (
                PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                identity_key.clone(),
            ),
            (
                PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                identity_key.clone(),
            ),
            (
                PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
                identity_key,
            ),
        ]);
    }
    if namespace == PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE {
        let authorization: PerceptionSourceAuthorizationV1 =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        authorization.validate_contract()?;
        return Ok(vec![
            (
                PERCEPTION_SOURCE_FENCE_NAMESPACE.into(),
                authorization.key()?,
            ),
            (
                PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                authorization.key()?,
            ),
            (
                crate::store_internal::schema::CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.into(),
                crate::store_internal::schema::control_plane_scope_manifest_key(
                    &authorization.memory_space_id,
                    &authorization.memory_space_id,
                )?,
            ),
        ]);
    }
    if namespace == PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE {
        let historical: PerceptionHistoricalAuthorizationV1 =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        historical.validate_contract()?;
        return Ok(vec![
            (PERCEPTION_EPISODE_NAMESPACE.into(), historical.key()?),
            (
                PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(),
                historical.key()?,
            ),
        ]);
    }
    if namespace == PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE {
        let ban: PerceptionSourceRetentionBanV1 =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        ban.validate_contract()?;
        return Ok(vec![(
            PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
            ban.key()?,
        )]);
    }
    if namespace != PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE {
        return Ok(Vec::new());
    }
    let root: PerceptionSourceDependentsRootV1 =
        serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
    root.validate_contract()?;
    let mut addresses = vec![(
        PERCEPTION_EPISODE_NAMESPACE.into(),
        perception_episode_storage_key(
            &root.source.memory_space_id,
            &root.source.subject_id,
            &root.source.source,
            &root.source.episode_id,
        )?,
    )];
    addresses.push((
        PERCEPTION_SOURCE_FENCE_NAMESPACE.into(),
        perception_source_fence_key(
            &root.source.memory_space_id,
            &root.source.subject_id,
            root.source.relationship_id.as_deref(),
            &root.source.source,
        )?,
    ));
    for edge in root.edges {
        let owner_namespace = match edge.kind {
            PerceptionSourceDependentKindV1::TranscriptTurn => "conversation_transcript",
            PerceptionSourceDependentKindV1::LongTermOwnerRevision => {
                crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE
            }
            _ => continue,
        };
        addresses.push((owner_namespace.into(), edge.owner_key));
    }
    Ok(addresses)
}

pub(crate) fn perception_turn_taint_key(
    memory_space_id: &str,
    subject_id: &str,
    session_id: &str,
    turn_id: &str,
) -> Result<String> {
    for value in [memory_space_id, subject_id, session_id, turn_id] {
        require_nonempty(value, "turn taint scope")?;
    }
    let bytes = serde_json::to_vec(&(memory_space_id, subject_id, session_id, turn_id))
        .map_err(|error| invalid(error.to_string()))?;
    Ok(format!("turn:sha256:{:x}", Sha256::digest(bytes)))
}

pub(crate) fn perception_source_commitment(
    source: &PerceptionSourceKey,
    input_id: &str,
    revoke_epoch: u64,
) -> Result<String> {
    source.validate_contract()?;
    require_nonempty(input_id, "input_id")?;
    let bytes = serde_json::to_vec(&(source, input_id, revoke_epoch))
        .map_err(|error| invalid(error.to_string()))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

pub(crate) fn plan_perception_episode_intake(
    platform: &StorePlatform,
    record: &TranscriptTurnRecord,
    episodes: &[PerceptionTurnEpisodeIntakeV1],
) -> Result<(
    Vec<StoreMutation>,
    Vec<StoreJsonPrecondition>,
    Vec<StoreBlobPrecondition>,
)> {
    let fresh_uses = record
        .perception_source_uses
        .iter()
        .filter(|usage| {
            !matches!(
                &usage.delivery,
                PerceptionReplyEvidenceV1::ReplayedOwner { .. }
            )
        })
        .count();
    if episodes.len() != fresh_uses {
        return Err(invalid(
            "Transcript fresh visual sources and episode count differ",
        ));
    }
    let mut mutations = Vec::new();
    let mut preconditions = Vec::new();
    let mut blob_preconditions = Vec::new();
    let mut seen = BTreeSet::new();
    for intake in episodes {
        let episode = &intake.episode;
        episode.validate_contract()?;
        if episode.memory_space_id != record.key.memory_space_id
            || episode.subject_id != record.subject
        {
            return Err(invalid("turn intake episode escapes canonical owner scope"));
        }
        if let PerceptionSourceRef::UserAttachment { message_id, .. } = &episode.origin_ref {
            if !record.input_messages.iter().any(|message| {
                message.message_id == *message_id
                    && message.role == "user"
                    && message.authority == bm_core::memory::MemoryEvidenceAuthority::UserAsserted
            }) {
                return Err(invalid("user-shared visual lacks exact user message owner"));
            }
        }
        if episode
            .delivery
            .as_ref()
            .is_some_and(|delivery| delivery.turn_id != record.turn_id)
            || episode.observation.as_ref().is_some_and(|observation| {
                observation.scope.turn_id.as_deref() != Some(record.turn_id.as_str())
                    || !matches!(
                        observation.evidence_origin,
                        bm_core::memory::VisualEvidenceOrigin::BeetleInference { .. }
                    )
            })
        {
            return Err(invalid(
                "episode observation or delivery belongs to another turn or authority",
            ));
        }
        let source_ref = episode.source_ref()?;
        let matches_delivery = record.perception_source_uses.iter().any(|usage| {
            usage.source == source_ref && episode.delivery.as_ref().is_some_and(|delivery| {
                matches!(&usage.delivery, PerceptionReplyEvidenceV1::HostAttested { receipt_digest }
                        if receipt_digest == &delivery.receipt_sha256)
            })
        });
        let observation_digest = episode
            .observation
            .as_ref()
            .map(|observation| {
                serde_json::to_vec(observation)
                    .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)))
                    .map_err(|error| invalid(error.to_string()))
            })
            .transpose()?;
        let matches_observation = record.perception_source_uses.iter().any(|usage| {
            usage.source == source_ref && observation_digest.as_ref().is_some_and(|digest| {
                matches!(&usage.delivery, PerceptionReplyEvidenceV1::BeetleObservation { observation_digest }
                    if observation_digest == digest)
            })
        });
        if !seen.insert(source_ref.clone()) || !(matches_delivery || matches_observation) {
            return Err(invalid(
                "episode lacks exact Transcript source and receipt binding",
            ));
        }
        let key = episode.key()?;
        let transcript_key = crate::store_internal::platform::transcript_turn_storage_key(
            &record.key,
            &record.subject,
            &record.turn_id,
        );
        let transcript_digest = format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(record).map_err(|error| invalid(error.to_string()))?)
        );
        let mut edges = vec![PerceptionSourceDependentEdgeV1 {
            kind: PerceptionSourceDependentKindV1::TranscriptTurn,
            memory_space_id: record.key.memory_space_id.clone(),
            subject_id: record.subject.clone(),
            relationship_id: episode.relationship_id.clone(),
            owner_key: transcript_key,
            owner_revision: Some(record.sequence),
            content_digest: transcript_digest,
        }];
        match (&episode.media, &intake.media_bytes) {
            (Some(media), Some(bytes)) => {
                if bytes.len() != media.byte_len
                    || format!("sha256:{:x}", Sha256::digest(bytes)) != media.sha256
                {
                    return Err(invalid("turn media bytes differ from typed owner"));
                }
                edges.push(PerceptionSourceDependentEdgeV1 {
                    kind: PerceptionSourceDependentKindV1::MediaEvidence,
                    memory_space_id: episode.memory_space_id.clone(),
                    subject_id: episode.subject_id.clone(),
                    relationship_id: episode.relationship_id.clone(),
                    owner_key: media.key.clone(),
                    owner_revision: None,
                    content_digest: media.sha256.clone(),
                });
                mutations.push(StoreMutation::PutBlob {
                    namespace: PERCEPTION_MEDIA_NAMESPACE.into(),
                    key: media.key.clone(),
                    value: bytes.clone(),
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "perception_evidence".into(),
                    record_key: episode.episode_id.clone(),
                });
                blob_preconditions.push(StoreBlobPrecondition::Absent {
                    namespace: PERCEPTION_MEDIA_NAMESPACE.into(),
                    key: media.key.clone(),
                });
            }
            (None, None) => {}
            _ => return Err(invalid("turn media bytes and typed owner must match")),
        }
        let root = PerceptionSourceDependentsRootV1::new(source_ref, edges)?;
        let historical = historical_authorization_for_new_episode(platform, episode)?;
        mutations.push(StoreMutation::PutJson {
            namespace: PERCEPTION_EPISODE_NAMESPACE.into(),
            key: key.clone(),
            value: serde_json::to_value(episode).map_err(|error| invalid(error.to_string()))?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "perception_evidence".into(),
            record_key: episode.episode_id.clone(),
        });
        mutations.push(StoreMutation::PutJson {
            namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
            key: key.clone(),
            value: serde_json::to_value(root).map_err(|error| invalid(error.to_string()))?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "perception_evidence".into(),
            record_key: episode.episode_id.clone(),
        });
        mutations.push(StoreMutation::PutJson {
            namespace: PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
            key: key.clone(),
            value: serde_json::to_value(historical).map_err(|error| invalid(error.to_string()))?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "perception_evidence".into(),
            record_key: episode.episode_id.clone(),
        });
        for namespace in [
            PERCEPTION_EPISODE_NAMESPACE,
            PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE,
            PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE,
            PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE,
        ] {
            preconditions.push(StoreJsonPrecondition::Absent {
                namespace: namespace.into(),
                key: key.clone(),
            });
        }
    }
    for usage in &record.perception_source_uses {
        let PerceptionReplyEvidenceV1::ReplayedOwner { owner } = &usage.delivery else {
            continue;
        };
        if !seen.insert(usage.source.clone()) {
            return Err(invalid("replayed source duplicates fresh episode"));
        }
        let source = &usage.source;
        let key = perception_episode_storage_key(
            &source.memory_space_id,
            &source.subject_id,
            &source.source,
            &source.episode_id,
        )?;
        let episode_doc = platform
            .read_json_docs_by_keys(PERCEPTION_EPISODE_NAMESPACE, std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("replayed episode is absent"))?;
        let episode: PerceptionEpisodeEvidenceV1 =
            serde_json::from_value(episode_doc.value.clone())
                .map_err(|error| invalid(error.to_string()))?;
        episode.validate_contract()?;
        if episode.source_ref()? != *source {
            return Err(invalid("replayed episode differs from exact source"));
        }
        platform.assert_perception_episode_replay_allowed(source)?;
        let mut owner_preconditions = validate_replay_owner_in_store(
            platform,
            owner,
            source,
            episode.source_kind,
            record.mounted_relationship_id.as_deref(),
        )?;
        preconditions.append(&mut owner_preconditions);
        let root_doc = platform
            .read_json_docs_by_keys(
                PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("replayed episode lacks dependents root"))?;
        let root: PerceptionSourceDependentsRootV1 = serde_json::from_value(root_doc.value.clone())
            .map_err(|error| invalid(error.to_string()))?;
        root.validate_contract()?;
        if root.source != *source {
            return Err(invalid("replayed root differs from exact source"));
        }
        let mut edges = root.edges;
        edges.push(transcript_edge_for_episode(record, source)?);
        let next_root = PerceptionSourceDependentsRootV1::new(source.clone(), edges)?;
        mutations.push(StoreMutation::PutJson {
            namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
            key: key.clone(),
            value: serde_json::to_value(next_root).map_err(|error| invalid(error.to_string()))?,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "perception_source_dependents".into(),
            record_key: key.clone(),
        });
        preconditions.push(StoreJsonPrecondition::Exact {
            namespace: PERCEPTION_EPISODE_NAMESPACE.into(),
            key: key.clone(),
            value: episode_doc.value,
        });
        preconditions.push(StoreJsonPrecondition::Absent {
            namespace: PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(),
            key: key.clone(),
        });
        preconditions.push(StoreJsonPrecondition::Exact {
            namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
            key,
            value: root_doc.value,
        });
    }
    let (count_mutations, count_preconditions) =
        platform.plan_perception_source_episode_count_delta(&mutations)?;
    mutations.extend(count_mutations);
    preconditions.extend(count_preconditions);
    Ok((mutations, preconditions, blob_preconditions))
}

fn transcript_edge_for_episode(
    record: &TranscriptTurnRecord,
    source: &PerceptionEpisodeSourceRef,
) -> Result<PerceptionSourceDependentEdgeV1> {
    let owner_key = crate::store_internal::platform::transcript_turn_storage_key(
        &record.key,
        &record.subject,
        &record.turn_id,
    );
    let content_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(record).map_err(|error| invalid(error.to_string()))?)
    );
    Ok(PerceptionSourceDependentEdgeV1 {
        kind: PerceptionSourceDependentKindV1::TranscriptTurn,
        memory_space_id: source.memory_space_id.clone(),
        subject_id: source.subject_id.clone(),
        relationship_id: source.relationship_id.clone(),
        owner_key,
        owner_revision: Some(record.sequence),
        content_digest,
    })
}

fn replay_relationship_allowed(
    source: &PerceptionEpisodeSourceRef,
    target_relationship_id: Option<&str>,
    long_term_scope: Option<bm_core::memory::LongTermMemorySourceScope>,
) -> bool {
    source.relationship_id.as_deref() == target_relationship_id
        || long_term_scope == Some(bm_core::memory::LongTermMemorySourceScope::User)
}

fn validate_replay_owner_in_store(
    platform: &StorePlatform,
    owner: &PerceptionReplayOwnerV1,
    source: &PerceptionEpisodeSourceRef,
    source_kind: PerceptionSourceKind,
    target_relationship_id: Option<&str>,
) -> Result<Vec<StoreJsonPrecondition>> {
    match owner {
        PerceptionReplayOwnerV1::TranscriptMessage { reference } => {
            let key = ConversationKey::new(
                &reference.memory_space_id,
                &reference.channel_id,
                &reference.conversation_id,
            )?;
            let owner_key = crate::store_internal::platform::transcript_turn_storage_key(
                &key,
                &source.subject_id,
                &reference.turn_id,
            );
            let value = platform
                .read_json_docs_by_keys(
                    "conversation_transcript",
                    std::slice::from_ref(&owner_key),
                )?
                .into_iter()
                .next()
                .ok_or_else(|| invalid("replay Transcript owner is missing"))?
                .value;
            let record: TranscriptTurnRecord = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if !replay_relationship_allowed(source, target_relationship_id, None)
                || record.mounted_relationship_id != source.relationship_id
                || !reference.is_available_in(&record)
            {
                return Err(invalid("replay Transcript message is no longer available"));
            }
            let message_id = reference
                .message_id
                .as_deref()
                .ok_or_else(|| invalid("replay Transcript lacks message id"))?;
            let source_usage = record
                .perception_source_uses
                .iter()
                .find(|usage| usage.source == *source)
                .ok_or_else(|| invalid("replay Transcript lacks exact visual source"))?;
            let assistant_affected = record.assistant_messages.iter().any(|item| {
                item.message.message_id == message_id
                    && item.message.sequence >= source_usage.first_affected_assistant_sequence
            });
            let user_attachment = source_kind == PerceptionSourceKind::HumanUserSharedMedia
                && record
                    .input_messages
                    .iter()
                    .any(|message| message.message_id == message_id);
            if !assistant_affected && !user_attachment {
                return Err(invalid("replay message predates visual source effect"));
            }
            Ok(vec![StoreJsonPrecondition::Exact {
                namespace: "conversation_transcript".into(),
                key: owner_key,
                value,
            }])
        }
        PerceptionReplayOwnerV1::LongTermMemory {
            owner_id,
            owner_revision,
        } => {
            let owner_ref = bm_core::memory::GovernedMemoryOwnerRef::new(
                bm_core::memory::GovernedMemoryOwnerPlane::LongTerm,
                owner_id.clone(),
            );
            let head_key = bm_core::memory::long_term_version_head_key(
                &source.memory_space_id,
                &source.memory_space_id,
                &owner_ref,
            )?;
            let material_key = bm_core::memory::long_term_version_material_key(
                &source.memory_space_id,
                &source.memory_space_id,
                &owner_ref,
                *owner_revision,
            )?;
            let head_value = platform
                .read_json_docs_by_keys(
                    crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
                    std::slice::from_ref(&head_key),
                )?
                .into_iter()
                .next()
                .ok_or_else(|| invalid("replay long-term head is missing"))?
                .value;
            let material_value = platform
                .read_json_docs_by_keys(
                    crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
                    std::slice::from_ref(&material_key),
                )?
                .into_iter()
                .next()
                .ok_or_else(|| invalid("replay long-term material is missing"))?
                .value;
            let head: bm_core::memory::LongTermMemoryHeadManifest =
                serde_json::from_value(head_value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
            let material: bm_core::memory::LongTermMemoryVersionMaterial =
                serde_json::from_value(material_value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
            if !head.validate_contract().accepted
                || !material.validate_contract().accepted
                || head.owner_ref != owner_ref
                || head.current_revision != *owner_revision
                || head.terminal_transition_ref.is_some()
                || material.owner_ref != owner_ref
                || material.owner_revision != *owner_revision
                || !material.governed_content.transcript_sources.is_empty()
                || !material
                    .governed_content
                    .perception_sources
                    .contains(source)
                || !replay_relationship_allowed(
                    source,
                    target_relationship_id,
                    Some(material.governed_content.source_scope),
                )
                || !head.retained_revision_digests.iter().any(|entry| {
                    entry.owner_revision == *owner_revision
                        && entry.content_digest == material.content_digest
                })
            {
                return Err(invalid(
                    "replay long-term owner is not current visual evidence",
                ));
            }
            Ok(vec![
                StoreJsonPrecondition::Exact {
                    namespace: crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE.into(),
                    key: head_key,
                    value: head_value,
                },
                StoreJsonPrecondition::Exact {
                    namespace: crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE.into(),
                    key: material_key,
                    value: material_value,
                },
            ])
        }
    }
}

fn replay_owner_available_in_state(
    state: &BackendTransactionState,
    owner: &PerceptionReplayOwnerV1,
    source: &PerceptionEpisodeSourceRef,
    source_kind: PerceptionSourceKind,
    target_relationship_id: Option<&str>,
) -> Result<bool> {
    let addresses = replay_owner_addresses(owner, source)?;
    match owner {
        PerceptionReplayOwnerV1::TranscriptMessage { reference } => {
            let Some(value) = state.json.get(&addresses[0]) else {
                return Ok(false);
            };
            let record: TranscriptTurnRecord = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            let Some(message_id) = reference.message_id.as_deref() else {
                return Ok(false);
            };
            let Some(usage) = record
                .perception_source_uses
                .iter()
                .find(|usage| usage.source == *source)
            else {
                return Ok(false);
            };
            let assistant_affected = record.assistant_messages.iter().any(|item| {
                item.message.message_id == message_id
                    && item.message.sequence >= usage.first_affected_assistant_sequence
            });
            let user_attachment = source_kind == PerceptionSourceKind::HumanUserSharedMedia
                && record
                    .input_messages
                    .iter()
                    .any(|message| message.message_id == message_id);
            Ok(
                replay_relationship_allowed(source, target_relationship_id, None)
                    && record.mounted_relationship_id == source.relationship_id
                    && reference.is_available_in(&record)
                    && (assistant_affected || user_attachment),
            )
        }
        PerceptionReplayOwnerV1::LongTermMemory {
            owner_id,
            owner_revision,
        } => {
            let (Some(head_value), Some(material_value)) =
                (state.json.get(&addresses[0]), state.json.get(&addresses[1]))
            else {
                return Ok(false);
            };
            let head: bm_core::memory::LongTermMemoryHeadManifest =
                serde_json::from_value(head_value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
            let material: bm_core::memory::LongTermMemoryVersionMaterial =
                serde_json::from_value(material_value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
            let owner_ref = bm_core::memory::GovernedMemoryOwnerRef::new(
                bm_core::memory::GovernedMemoryOwnerPlane::LongTerm,
                owner_id.clone(),
            );
            Ok(head.validate_contract().accepted
                && material.validate_contract().accepted
                && head.owner_ref == owner_ref
                && head.current_revision == *owner_revision
                && head.terminal_transition_ref.is_none()
                && material.owner_ref == owner_ref
                && material.owner_revision == *owner_revision
                && material.governed_content.transcript_sources.is_empty()
                && material
                    .governed_content
                    .perception_sources
                    .contains(source)
                && replay_relationship_allowed(
                    source,
                    target_relationship_id,
                    Some(material.governed_content.source_scope),
                )
                && head.retained_revision_digests.iter().any(|entry| {
                    entry.owner_revision == *owner_revision
                        && entry.content_digest == material.content_digest
                }))
        }
    }
}

fn validate_replayed_source_in_state(
    state: &BackendTransactionState,
    source: &PerceptionEpisodeSourceRef,
    owner: &PerceptionReplayOwnerV1,
    target_relationship_id: Option<&str>,
) -> Result<()> {
    let episode_key = perception_episode_storage_key(
        &source.memory_space_id,
        &source.subject_id,
        &source.source,
        &source.episode_id,
    )?;
    if state.json.contains_key(&(
        PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(),
        episode_key.clone(),
    )) {
        return Err(invalid("replayed episode is withdrawn"));
    }
    let episode_value = state
        .json
        .get(&(PERCEPTION_EPISODE_NAMESPACE.into(), episode_key))
        .ok_or_else(|| invalid("replayed episode is missing"))?;
    let episode: PerceptionEpisodeEvidenceV1 = serde_json::from_value(episode_value.clone())
        .map_err(|error| invalid(error.to_string()))?;
    episode.validate_contract()?;
    if episode.source_ref()? != *source {
        return Err(invalid("replayed episode source digest differs"));
    }
    let identity_key = perception_source_fence_key(
        &source.memory_space_id,
        &source.subject_id,
        source.relationship_id.as_deref(),
        &source.source,
    )?;
    let historical_value = state
        .json
        .get(&(
            PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
            perception_episode_storage_key(
                &source.memory_space_id,
                &source.subject_id,
                &source.source,
                &source.episode_id,
            )?,
        ))
        .ok_or_else(|| invalid("replayed episode lacks historical authorization"))?;
    let historical: PerceptionHistoricalAuthorizationV1 =
        serde_json::from_value(historical_value.clone())
            .map_err(|error| invalid(error.to_string()))?;
    historical.validate_episode(&episode)?;
    if state.json.contains_key(&(
        PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
        identity_key,
    )) || !replay_owner_available_in_state(
        state,
        owner,
        source,
        episode.source_kind,
        target_relationship_id,
    )? {
        return Err(invalid(
            "replayed source or upstream owner is no longer available",
        ));
    }
    Ok(())
}

fn validate_transcript_external_visual_inventory(
    state: &BackendTransactionState,
    record: &TranscriptTurnRecord,
) -> Result<()> {
    let mut actual_host = BTreeSet::new();
    let mut actual_replay = BTreeSet::new();
    for usage in &record.perception_source_uses {
        match &usage.delivery {
            PerceptionReplyEvidenceV1::HostAttested { receipt_digest } => {
                actual_host.insert((usage.source.clone(), receipt_digest.clone()));
            }
            PerceptionReplyEvidenceV1::ReplayedOwner { owner } => {
                actual_replay.insert((usage.source.clone(), owner.clone()));
            }
            PerceptionReplyEvidenceV1::BeetleObservation { .. } => {}
        }
    }
    let Some(manifest) = &record.external_content_manifest else {
        return Ok(());
    };
    manifest.validate_against(record)?;
    let mut declared_host = BTreeSet::new();
    let mut declared_replay = BTreeSet::new();
    for usage in &manifest.uses {
        match usage {
            TranscriptExternalContentUseV1::Visual {
                source,
                input_id,
                packet_id,
                request_sha256,
                receipt_digest,
                ..
            } => {
                let key = perception_episode_storage_key(
                    &source.memory_space_id,
                    &source.subject_id,
                    &source.source,
                    &source.episode_id,
                )?;
                if state
                    .json
                    .contains_key(&(PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(), key.clone()))
                {
                    return Err(invalid("external visual source was withdrawn"));
                }
                let value = state
                    .json
                    .get(&(PERCEPTION_EPISODE_NAMESPACE.into(), key))
                    .ok_or_else(|| invalid("external visual source episode is absent"))?;
                let episode: PerceptionEpisodeEvidenceV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                episode.validate_contract()?;
                let delivery = episode
                    .delivery
                    .as_ref()
                    .ok_or_else(|| invalid("external visual source lacks host delivery"))?;
                if episode.source_ref()? != *source
                    || delivery.input_id != *input_id
                    || delivery.packet_id != *packet_id
                    || delivery.final_request_sha256 != *request_sha256
                    || delivery.receipt_sha256 != *receipt_digest
                    || delivery.turn_id != record.turn_id
                {
                    return Err(invalid(
                        "external visual inventory differs from exact delivery",
                    ));
                }
                if !declared_host.insert((source.clone(), receipt_digest.clone())) {
                    return Err(invalid("external visual delivery is declared twice"));
                }
            }
            TranscriptExternalContentUseV1::ReplayedVisual { source, owner, .. } => {
                validate_replayed_source_in_state(
                    state,
                    source,
                    owner,
                    record.mounted_relationship_id.as_deref(),
                )?;
                if !declared_replay.insert((source.clone(), owner.clone())) {
                    return Err(invalid("external visual replay is declared twice"));
                }
            }
            TranscriptExternalContentUseV1::Other { .. } => {}
        }
    }
    if declared_host != actual_host || declared_replay != actual_replay {
        return Err(invalid(
            "external visual inventory omits or invents a source use",
        ));
    }
    Ok(())
}

fn require_nonempty(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > MAX_EPISODE_ID_LEN {
        return Err(invalid(format!("{name} is empty or over budget")));
    }
    Ok(())
}

fn require_sha256(value: &str, name: &str) -> Result<()> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(invalid(format!("{name} must be a sha256 digest")));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> Error {
    Error::invalid_input("perception_episode", message)
}

fn append_live_long_term_edges(
    after: &BackendTransactionState,
    source: &PerceptionEpisodeSourceRef,
    edges: &mut Vec<PerceptionSourceDependentEdgeV1>,
) -> Result<()> {
    for ((namespace, key), value) in &after.json {
        if namespace != crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE {
            continue;
        }
        let material: bm_core::memory::LongTermMemoryVersionMaterial =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        if !material
            .governed_content
            .perception_sources
            .contains(source)
        {
            continue;
        }
        edges.push(PerceptionSourceDependentEdgeV1 {
            kind: PerceptionSourceDependentKindV1::LongTermOwnerRevision,
            memory_space_id: source.memory_space_id.clone(),
            subject_id: source.subject_id.clone(),
            relationship_id: source.relationship_id.clone(),
            owner_key: key.clone(),
            owner_revision: Some(material.owner_revision),
            content_digest: format!("sha256:{}", material.content_digest),
        });
    }
    Ok(())
}

fn live_soul_edges_for_source(
    json: &BTreeMap<(String, String), serde_json::Value>,
    source: &PerceptionEpisodeSourceRef,
) -> Result<Vec<PerceptionSourceDependentEdgeV1>> {
    use crate::store_internal::schema::{
        SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE, SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE,
        SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
    };
    use bm_core::memory::{
        SubjectSoulLifecycleHeadV1, SubjectSoulLifecycleStateV1, SubjectSoulRevisionMaterialV1,
        SubjectSoulScopeManifestV1, SubjectSoulSourceRefV1,
    };
    let source_lineage = SubjectSoulSourceRefV1::Perception(source.clone());
    let mut edges = Vec::new();
    for ((namespace, manifest_key), value) in json {
        if namespace != SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE {
            continue;
        }
        let manifest: SubjectSoulScopeManifestV1 =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        manifest
            .validate_contract()
            .map_err(|error| invalid(error.to_string()))?;
        if manifest.memory_space_id != source.memory_space_id
            || manifest.subject_id != source.subject_id
        {
            continue;
        }
        let head_value = json
            .get(&(
                SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.into(),
                manifest_key.clone(),
            ))
            .ok_or_else(|| invalid("visual Soul manifest lacks lifecycle head"))?;
        let head: SubjectSoulLifecycleHeadV1 = serde_json::from_value(head_value.clone())
            .map_err(|error| invalid(error.to_string()))?;
        head.validate_contract()
            .map_err(|error| invalid(error.to_string()))?;
        if head.memory_space_id != manifest.memory_space_id
            || head.subject_id != manifest.subject_id
            || head.soul_id != manifest.soul_id
            || head.generation != manifest.generation
            || head.scope_manifest_digest != manifest.closure_digest
        {
            return Err(invalid("visual Soul head and manifest disagree"));
        }
        if head.state == SubjectSoulLifecycleStateV1::Deleted {
            continue;
        }
        let mut has_live_revision = false;
        for entry in &manifest.entries {
            if entry.namespace != SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE
                || !entry.source_lineage.contains(&source_lineage)
            {
                continue;
            }
            let material_value = json
                .get(&(entry.namespace.clone(), entry.physical_key.clone()))
                .ok_or_else(|| invalid("visual Soul revision material is missing"))?;
            let material: SubjectSoulRevisionMaterialV1 =
                serde_json::from_value(material_value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
            material
                .validate_contract()
                .map_err(|error| invalid(error.to_string()))?;
            if material.memory_space_id != manifest.memory_space_id
                || material.subject_id != manifest.subject_id
                || material.soul_id != manifest.soul_id
                || material.generation != manifest.generation
                || material.content_digest != entry.content_digest
                || material.source_lineage != entry.source_lineage
                || entry.revision != Some(material.revision)
            {
                return Err(invalid("visual Soul revision differs from manifest entry"));
            }
            if manifest.source_review.denies_revision(&material) {
                continue;
            }
            has_live_revision = true;
            edges.push(PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::SoulRevision,
                memory_space_id: source.memory_space_id.clone(),
                subject_id: source.subject_id.clone(),
                relationship_id: None,
                owner_key: entry.physical_key.clone(),
                owner_revision: Some(material.revision),
                content_digest: format!("sha256:{}", material.content_digest),
            });
        }
        if has_live_revision {
            edges.push(PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::SoulRoot,
                memory_space_id: source.memory_space_id.clone(),
                subject_id: source.subject_id.clone(),
                relationship_id: None,
                owner_key: manifest_key.clone(),
                owner_revision: Some(manifest.manifest_revision),
                content_digest: format!("sha256:{}", manifest.closure_digest),
            });
        }
    }
    Ok(edges)
}

/// Adds exact reverse edges for Soul material/manifest changes before the
/// enclosing Store intent is signed. Existing root mutations are rewritten in
/// place, so a visual turn that also creates its episode has one root Put.
pub(crate) fn append_perception_soul_source_closure(
    platform: &StorePlatform,
    mutations: &mut Vec<StoreMutation>,
    preconditions: &mut Vec<StoreJsonPrecondition>,
) -> Result<()> {
    use crate::store_internal::schema::{
        SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE, SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
    };
    use bm_core::memory::{
        SubjectSoulRevisionMaterialV1, SubjectSoulScopeManifestV1, SubjectSoulSourceRefV1,
    };
    let soul_mutations = mutations
        .iter()
        .filter(|mutation| match mutation {
            StoreMutation::PutJson { namespace, .. }
            | StoreMutation::DeleteJson { namespace, .. } => {
                namespace == SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE
                    || namespace == SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE
            }
            _ => false,
        })
        .collect::<Vec<_>>();
    if soul_mutations.is_empty() {
        return Ok(());
    }
    let mut scope_keys = BTreeSet::new();
    let mut material_keys = BTreeSet::new();
    let mut manifest_values = Vec::new();
    for mutation in &soul_mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } if namespace == SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE => {
                scope_keys.insert(key.clone());
                manifest_values.push(value.clone());
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE =>
            {
                scope_keys.insert(key.clone());
            }
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } if namespace == SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE => {
                material_keys.insert(key.clone());
                let material: SubjectSoulRevisionMaterialV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                scope_keys.insert(crate::store_internal::schema::subject_soul_scope_key(
                    &material.memory_space_id,
                    &material.subject_id,
                    &material.soul_id,
                )?);
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE =>
            {
                material_keys.insert(key.clone());
                let old = platform
                    .read_json_docs_by_keys(namespace, std::slice::from_ref(key))?
                    .into_iter()
                    .next()
                    .ok_or_else(|| invalid("deleted Soul material is absent"))?;
                let material: SubjectSoulRevisionMaterialV1 = serde_json::from_value(old.value)
                    .map_err(|error| invalid(error.to_string()))?;
                scope_keys.insert(crate::store_internal::schema::subject_soul_scope_key(
                    &material.memory_space_id,
                    &material.subject_id,
                    &material.soul_id,
                )?);
            }
            _ => {}
        }
    }
    let scope_keys = scope_keys.into_iter().collect::<Vec<_>>();
    let mut before = BTreeMap::new();
    for namespace in [
        SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
        crate::store_internal::schema::SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE,
    ] {
        for document in platform.read_json_docs_by_keys(namespace, &scope_keys)? {
            if namespace == SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE {
                manifest_values.push(document.value.clone());
            }
            before.insert((document.namespace, document.key), document.value);
        }
    }
    for value in manifest_values {
        let manifest: SubjectSoulScopeManifestV1 =
            serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
        manifest
            .validate_contract()
            .map_err(|error| invalid(error.to_string()))?;
        for entry in manifest.entries {
            if entry.namespace == SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE {
                material_keys.insert(entry.physical_key);
            }
        }
    }
    let material_keys = material_keys.into_iter().collect::<Vec<_>>();
    for document in
        platform.read_json_docs_by_keys(SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE, &material_keys)?
    {
        before.insert((document.namespace, document.key), document.value);
    }
    let mut after = before.clone();
    let mut affected = BTreeSet::<PerceptionEpisodeSourceRef>::new();
    {
        let mut add_sources = |namespace: &str, value: &serde_json::Value| -> Result<()> {
            let sources = if namespace == SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE {
                let material: SubjectSoulRevisionMaterialV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                material.source_lineage
            } else {
                let manifest: SubjectSoulScopeManifestV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                manifest
                    .entries
                    .into_iter()
                    .flat_map(|entry| entry.source_lineage)
                    .collect()
            };
            for source in sources {
                if let SubjectSoulSourceRefV1::Perception(source) = source {
                    source.validate_contract()?;
                    affected.insert(source);
                }
            }
            Ok(())
        };
        for mutation in &soul_mutations {
            let (namespace, key, next) = match mutation {
                StoreMutation::PutJson {
                    namespace,
                    key,
                    value,
                    ..
                } => (namespace.as_str(), key.as_str(), Some(value)),
                StoreMutation::DeleteJson { namespace, key, .. } => {
                    (namespace.as_str(), key.as_str(), None)
                }
                _ => unreachable!(),
            };
            if let Some(previous) = before.get(&(namespace.into(), key.into())) {
                add_sources(namespace, previous)?;
            }
            if let Some(value) = next {
                add_sources(namespace, value)?;
                after.insert((namespace.into(), key.into()), (*value).clone());
            } else {
                after.remove(&(namespace.into(), key.into()));
            }
        }
    }
    drop(soul_mutations);
    for mutation in mutations.iter() {
        match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } if namespace
                == crate::store_internal::schema::SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE =>
            {
                after.insert((namespace.clone(), key.clone()), value.clone());
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace
                    == crate::store_internal::schema::SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE =>
            {
                after.remove(&(namespace.clone(), key.clone()));
            }
            _ => {}
        }
    }
    for source in affected {
        let key = perception_episode_storage_key(
            &source.memory_space_id,
            &source.subject_id,
            &source.source,
            &source.episode_id,
        )?;
        let old_soul_edges = live_soul_edges_for_source(&before, &source)?;
        let new_soul_edges = live_soul_edges_for_source(&after, &source)?;
        if old_soul_edges.is_empty() && new_soul_edges.is_empty() {
            continue;
        }
        if mutations.iter().any(|mutation| {
            matches!(
                mutation,
                StoreMutation::DeleteJson { namespace, key: deleted_key, .. }
                    if namespace == PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE && deleted_key == &key
            )
        }) {
            if !new_soul_edges.is_empty() {
                return Err(invalid("Soul keeps a source whose episode root is deleted"));
            }
            continue;
        }
        let planned_root = mutations.iter().position(|mutation| {
            matches!(
                mutation,
                StoreMutation::PutJson { namespace, key: put_key, .. }
                    if namespace == PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE && put_key == &key
            )
        });
        let (root_value, current_value) = if let Some(index) = planned_root {
            let StoreMutation::PutJson { value, .. } = &mutations[index] else {
                unreachable!()
            };
            (value.clone(), None)
        } else {
            let document = platform
                .read_json_docs_by_keys(
                    PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE,
                    std::slice::from_ref(&key),
                )?
                .into_iter()
                .next()
                .ok_or_else(|| invalid("Soul visual source lacks an existing episode root"))?;
            (document.value.clone(), Some(document.value))
        };
        let root: PerceptionSourceDependentsRootV1 =
            serde_json::from_value(root_value).map_err(|error| invalid(error.to_string()))?;
        root.validate_contract()?;
        if root.source != source {
            return Err(invalid("Soul visual source root differs from episode"));
        }
        let mut edges = root
            .edges
            .into_iter()
            .filter(|edge| {
                !matches!(
                    edge.kind,
                    PerceptionSourceDependentKindV1::SoulRoot
                        | PerceptionSourceDependentKindV1::SoulRevision
                )
            })
            .collect::<Vec<_>>();
        edges.extend(new_soul_edges);
        let updated = PerceptionSourceDependentsRootV1::new(source.clone(), edges)?;
        let updated_value =
            serde_json::to_value(updated).map_err(|error| invalid(error.to_string()))?;
        if let Some(index) = planned_root {
            let StoreMutation::PutJson { value, .. } = &mut mutations[index] else {
                unreachable!()
            };
            *value = updated_value;
        } else if current_value.as_ref() != Some(&updated_value) {
            mutations.push(StoreMutation::PutJson {
                namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
                key: key.clone(),
                value: updated_value,
                event_kind: MemoryStoreEventKind::MemoryControl,
                plane: "perception_source_dependents".into(),
                record_key: key.clone(),
            });
            crate::store_internal::transaction::merge_json_preconditions(
                preconditions,
                [StoreJsonPrecondition::Exact {
                    namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
                    key,
                    value: current_value
                        .ok_or_else(|| invalid("Soul root CAS lacks a before-image"))?,
                }],
            )?;
        }
    }
    Ok(())
}

impl StorePlatform {
    /// Aggregate every episode Put/Delete in the eventual batch before
    /// signing it. Multiple sibling deletions share one identity counter CAS.
    pub(crate) fn plan_perception_source_episode_count_delta(
        &self,
        mutations: &[StoreMutation],
    ) -> Result<(Vec<StoreMutation>, Vec<StoreJsonPrecondition>)> {
        let mut by_identity = BTreeMap::<String, (PerceptionEpisodeEvidenceV1, i128)>::new();
        for mutation in mutations {
            let (episode, delta) = match mutation {
                StoreMutation::PutJson {
                    namespace, value, ..
                } if namespace == PERCEPTION_EPISODE_NAMESPACE => {
                    let episode: PerceptionEpisodeEvidenceV1 =
                        serde_json::from_value(value.clone())
                            .map_err(|error| invalid(error.to_string()))?;
                    episode.validate_contract()?;
                    (episode, 1_i128)
                }
                StoreMutation::DeleteJson { namespace, key, .. }
                    if namespace == PERCEPTION_EPISODE_NAMESPACE =>
                {
                    let value = self
                        .read_json_docs_by_keys(
                            PERCEPTION_EPISODE_NAMESPACE,
                            std::slice::from_ref(key),
                        )?
                        .into_iter()
                        .next()
                        .ok_or_else(|| invalid("deleted episode count owner is absent"))?
                        .value;
                    let episode: PerceptionEpisodeEvidenceV1 = serde_json::from_value(value)
                        .map_err(|error| invalid(error.to_string()))?;
                    episode.validate_contract()?;
                    if episode.key()? != *key {
                        return Err(invalid("deleted episode count owner key mismatch"));
                    }
                    (episode, -1_i128)
                }
                _ => continue,
            };
            let identity_key = perception_source_fence_key(
                &episode.memory_space_id,
                &episode.subject_id,
                episode.relationship_id.as_deref(),
                &episode.source,
            )?;
            let entry = by_identity
                .entry(identity_key)
                .or_insert_with(|| (episode.clone(), 0));
            entry.1 += delta;
        }
        let mut count_mutations = Vec::new();
        let mut preconditions = Vec::new();
        for (key, (episode, delta)) in by_identity {
            let old_value = self
                .read_json_docs_by_keys(
                    PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE,
                    std::slice::from_ref(&key),
                )?
                .into_iter()
                .next()
                .map(|document| document.value);
            let (old_revision, old_count) = match &old_value {
                Some(value) => {
                    let old: PerceptionSourceEpisodeCountV1 = serde_json::from_value(value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                    old.validate_contract()?;
                    if old.key()? != key {
                        return Err(invalid("episode count identity mismatch"));
                    }
                    (old.revision, old.active_count)
                }
                None => (0, 0),
            };
            let next_count = (i128::from(old_count) + delta)
                .try_into()
                .map_err(|_| invalid("episode count would underflow or overflow"))?;
            let next = PerceptionSourceEpisodeCountV1::from_episode(
                &episode,
                old_revision.saturating_add(1),
                next_count,
            );
            next.validate_contract()?;
            preconditions.push(match old_value {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace: PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
                    key: key.clone(),
                    value,
                },
                None => StoreJsonPrecondition::Absent {
                    namespace: PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
                    key: key.clone(),
                },
            });
            count_mutations.push(StoreMutation::PutJson {
                namespace: PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
                key: key.clone(),
                value: serde_json::to_value(next).map_err(|error| invalid(error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryControl,
                plane: "perception_episode_count".into(),
                record_key: key,
            });
        }
        Ok((count_mutations, preconditions))
    }

    fn broad_visual_retention_blocked_at(
        &self,
        memory_space_id: &str,
        subject_id: &str,
        now_secs: u64,
    ) -> Result<bool> {
        use crate::store_internal::schema::{
            control_plane_scope_manifest_key, ControlPlaneScopeManifest,
            CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
        };
        let key = control_plane_scope_manifest_key(memory_space_id, memory_space_id)?;
        let Some(document) = self
            .read_json_docs_by_keys(
                CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
        else {
            return Ok(false);
        };
        let manifest: ControlPlaneScopeManifest = serde_json::from_value(document.value.clone())
            .map_err(|error| invalid(error.to_string()))?;
        manifest.validate(self.capacity().kv_max_entries)?;
        if manifest.physical_key != key {
            return Err(invalid("visual policy manifest key mismatch"));
        }
        let mut state = BackendTransactionState::default();
        state.json.insert(
            (CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.into(), key),
            document.value,
        );
        for entry in manifest.entries.iter().filter(|entry| {
            entry.namespace == bm_core::memory::LONG_TERM_GOVERNANCE_POLICY_NAMESPACE
        }) {
            let value = self
                .read_json_docs_by_keys(&entry.namespace, std::slice::from_ref(&entry.key))?
                .into_iter()
                .next()
                .ok_or_else(|| invalid("visual policy manifest owner is missing"))?
                .value;
            state
                .json
                .insert((entry.namespace.clone(), entry.key.clone()), value);
        }
        broad_visual_retention_blocked_by_policy(&state, memory_space_id, subject_id, now_secs)
    }

    pub(crate) fn read_perception_source_authorization(
        &self,
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
    ) -> Result<Option<PerceptionSourceAuthorizationV1>> {
        let key = perception_source_fence_key(
            &scope.memory_space_id,
            &scope.subject_id,
            scope.relationship_id.as_deref(),
            source,
        )?;
        let value = self
            .read_json_docs_by_keys(
                PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
            .map(|document| document.value);
        let Some(value) = value else { return Ok(None) };
        let authorization: PerceptionSourceAuthorizationV1 =
            serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
        authorization.validate_contract()?;
        if authorization.key()? != key {
            return Err(invalid("source authorization key differs from owner"));
        }
        Ok(Some(authorization))
    }

    pub(crate) fn read_perception_source_retention_ban(
        &self,
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
    ) -> Result<Option<PerceptionSourceRetentionBanV1>> {
        let key = perception_source_fence_key(
            &scope.memory_space_id,
            &scope.subject_id,
            scope.relationship_id.as_deref(),
            source,
        )?;
        let value = self
            .read_json_docs_by_keys(
                PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
            .map(|document| document.value);
        let Some(value) = value else { return Ok(None) };
        let ban: PerceptionSourceRetentionBanV1 =
            serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
        ban.validate_contract()?;
        if ban.key()? != key {
            return Err(invalid("source retention ban key differs from owner"));
        }
        Ok(Some(ban))
    }

    pub(crate) fn plan_perception_source_retention_ban_for_withdrawal(
        &self,
        sources: &[PerceptionEpisodeSourceRef],
        retention_ceiling: PerceptionRetentionCeiling,
        now_secs: u64,
    ) -> Result<(Vec<StoreMutation>, Vec<StoreJsonPrecondition>)> {
        if retention_ceiling.permits_durable_semantic() || now_secs == 0 {
            return Err(invalid("source ban must prohibit durable memory"));
        }
        let mut identities = BTreeMap::<String, PerceptionEpisodeSourceRef>::new();
        for source in sources {
            source.validate_contract()?;
            let key = perception_source_fence_key(
                &source.memory_space_id,
                &source.subject_id,
                source.relationship_id.as_deref(),
                &source.source,
            )?;
            identities.entry(key).or_insert_with(|| source.clone());
        }
        let mut mutations = Vec::new();
        let mut preconditions = Vec::new();
        for (key, source) in identities {
            let old_value = self
                .read_json_docs_by_keys(
                    PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE,
                    std::slice::from_ref(&key),
                )?
                .into_iter()
                .next()
                .map(|document| document.value);
            let old = old_value
                .as_ref()
                .map(|value| {
                    serde_json::from_value::<PerceptionSourceRetentionBanV1>(value.clone())
                })
                .transpose()
                .map_err(|error| invalid(error.to_string()))?;
            if let Some(old) = &old {
                old.validate_contract()?;
                if old.key()? != key || retention_ceiling > old.retention_ceiling {
                    return Err(invalid("source ban cannot be loosened"));
                }
            }
            let ban = PerceptionSourceRetentionBanV1 {
                schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
                memory_space_id: source.memory_space_id.clone(),
                subject_id: source.subject_id.clone(),
                relationship_id: source.relationship_id.clone(),
                host_id: source.source.host_id.clone(),
                device_id: source.source.device_id.clone(),
                source_id: source.source.source_id.clone(),
                policy_revision: old.as_ref().map_or(1, |old| old.policy_revision + 1),
                retention_ceiling,
                updated_at_unix_secs: old
                    .as_ref()
                    .map_or(now_secs, |old| now_secs.max(old.updated_at_unix_secs)),
            };
            ban.validate_contract()?;
            preconditions.push(match old_value {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace: PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                    key: key.clone(),
                    value,
                },
                None => StoreJsonPrecondition::Absent {
                    namespace: PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                    key: key.clone(),
                },
            });
            mutations.push(StoreMutation::PutJson {
                namespace: PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                key: key.clone(),
                value: serde_json::to_value(ban).map_err(|error| invalid(error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryControl,
                plane: "perception_source_retention_ban".into(),
                record_key: key,
            });
        }
        Ok((mutations, preconditions))
    }

    pub(crate) fn plan_perception_source_authorization_downgrade(
        &self,
        source: &PerceptionEpisodeSourceRef,
        retention_ceiling: PerceptionRetentionCeiling,
        now_secs: u64,
    ) -> Result<(Vec<StoreMutation>, Vec<StoreJsonPrecondition>)> {
        if retention_ceiling.permits_durable_semantic() || now_secs == 0 {
            return Err(invalid("source authorization downgrade is not restrictive"));
        }
        source.validate_contract()?;
        let key = perception_source_fence_key(
            &source.memory_space_id,
            &source.subject_id,
            source.relationship_id.as_deref(),
            &source.source,
        )?;
        let value = self
            .read_json_docs_by_keys(
                PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("source authorization downgrade lacks prior owner"))?
            .value;
        let old: PerceptionSourceAuthorizationV1 =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        old.validate_contract()?;
        if old.key()? != key || retention_ceiling > old.retention_ceiling {
            return Err(invalid(
                "source authorization downgrade cannot loosen ceiling",
            ));
        }
        let mut next = old.clone();
        next.authorization_generation = next
            .authorization_generation
            .checked_add(1)
            .ok_or_else(|| invalid("authorization generation overflow"))?;
        next.retention_ceiling = retention_ceiling;
        // Restrictive withdrawal preserves the authorization's historical
        // policy anchor. A concurrently updated policy cannot make this
        // already-disabled source eligible for new durable intake.
        next.updated_at_unix_secs = now_secs.max(old.updated_at_unix_secs);
        next.validate_contract()?;
        Ok((
            vec![StoreMutation::PutJson {
                namespace: PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                key: key.clone(),
                value: serde_json::to_value(next).map_err(|error| invalid(error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryControl,
                plane: "perception_source_authorization".into(),
                record_key: key.clone(),
            }],
            vec![StoreJsonPrecondition::Exact {
                namespace: PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                key,
                value,
            }],
        ))
    }

    pub(crate) fn assert_perception_source_authorized(
        &self,
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
        source_kind: PerceptionSourceKind,
        revoke_epoch: u64,
        authorization_generation: u64,
        requested_retention: PerceptionRetentionCeiling,
    ) -> Result<()> {
        let authorization = self
            .read_perception_source_authorization(scope, source)?
            .ok_or_else(|| invalid("perception source has no durable authorization"))?;
        if authorization.source != *source
            || authorization.source_kind != source_kind
            || authorization.revoke_epoch != revoke_epoch
            || authorization.authorization_generation != authorization_generation
            || requested_retention > authorization.retention_ceiling
            || (requested_retention.permits_durable_semantic()
                && !authorization.user_consent_attested)
            || (
                authorization.policy_manifest_revision,
                authorization.policy_manifest_digest.clone(),
            ) != self.current_perception_policy_manifest(&scope.memory_space_id)?
            || self
                .read_perception_source_retention_ban(scope, source)?
                .is_some_and(|ban| requested_retention > ban.retention_ceiling)
        {
            return Err(invalid(
                "perception source authorization does not permit use",
            ));
        }
        self.assert_perception_source_live(scope, source, revoke_epoch)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn register_perception_source_authorization(
        &self,
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
        source_kind: PerceptionSourceKind,
        revoke_epoch: u64,
        authorization_generation: u64,
        retention_ceiling: PerceptionRetentionCeiling,
        consent_attested: bool,
        event_scope: StoreEventScope,
        transaction_id: String,
        now_secs: u64,
    ) -> Result<Option<StoreMutationBatchReport>> {
        if event_scope.memory_space_id != scope.memory_space_id
            || event_scope.subject_id != scope.subject_id
        {
            return Err(invalid("source authorization event scope mismatch"));
        }
        self.assert_perception_source_live(scope, source, revoke_epoch)?;
        if self
            .read_perception_source_retention_ban(scope, source)?
            .is_some_and(|ban| retention_ceiling > ban.retention_ceiling)
        {
            return Err(invalid("source registration exceeds durable retention ban"));
        }
        if retention_ceiling.permits_durable_semantic()
            && self.broad_visual_retention_blocked_at(
                &scope.memory_space_id,
                &scope.subject_id,
                now_secs,
            )?
        {
            return Err(invalid(
                "broad memory prohibition denies durable visual retention",
            ));
        }
        let policy_precondition =
            self.perception_policy_manifest_precondition(&scope.memory_space_id)?;
        let (policy_manifest_revision, policy_manifest_digest) =
            policy_snapshot_from_precondition(&policy_precondition)?;
        let next = PerceptionSourceAuthorizationV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: scope.memory_space_id.clone(),
            subject_id: scope.subject_id.clone(),
            relationship_id: scope.relationship_id.clone(),
            source: source.clone(),
            source_kind,
            revoke_epoch,
            authorization_generation,
            retention_ceiling,
            user_consent_attested: consent_attested,
            policy_manifest_revision,
            policy_manifest_digest,
            updated_at_unix_secs: now_secs,
        };
        next.validate_contract()?;
        let key = next.key()?;
        let old_value = self
            .read_json_docs_by_keys(
                PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
            .map(|document| document.value);
        if let Some(value) = &old_value {
            let old: PerceptionSourceAuthorizationV1 = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            old.validate_contract()?;
            if old.key()? != key {
                return Err(invalid("source authorization owner is corrupt"));
            }
            if next.tuple() < old.tuple() || authorization_generation < old.authorization_generation
            {
                return Err(invalid("source authorization generation moved backward"));
            }
            if next.tuple() == old.tuple()
                && authorization_generation == old.authorization_generation
            {
                if next.source_kind != old.source_kind
                    || next.retention_ceiling != old.retention_ceiling
                    || next.user_consent_attested != old.user_consent_attested
                {
                    return Err(invalid(
                        "source authorization changed without new generation",
                    ));
                }
                return Ok(None);
            }
        }
        let precondition = match old_value {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace: PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                key: key.clone(),
                value,
            },
            None => StoreJsonPrecondition::Absent {
                namespace: PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                key: key.clone(),
            },
        };
        let batch = StoreMutationBatch {
            transaction_id,
            operation: "perception_source_authorize".into(),
            scope: event_scope,
            mutations: vec![StoreMutation::PutJson {
                namespace: PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                key: key.clone(),
                value: serde_json::to_value(next).map_err(|error| invalid(error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryControl,
                plane: "perception_source_authorization".into(),
                record_key: key,
            }],
        };
        let budget = self.current_runtime_budget(now_secs);
        self.commit_governed_memory_transaction_with_runtime_budget_at(
            batch,
            &[precondition, policy_precondition],
            &budget,
            now_secs,
        )
        .map(Some)
    }

    /// A single CAS anchor covers the complete set of policies, including a
    /// newly added prohibition that did not exist when the draft was planned.
    pub(crate) fn perception_policy_manifest_precondition(
        &self,
        memory_space_id: &str,
    ) -> Result<StoreJsonPrecondition> {
        use crate::store_internal::schema::{
            control_plane_scope_manifest_key, ControlPlaneScopeManifest,
            CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
        };
        let key = control_plane_scope_manifest_key(memory_space_id, memory_space_id)?;
        let value = self
            .read_json_docs_by_keys(
                CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
            .map(|document| document.value);
        match value {
            Some(value) => {
                let manifest: ControlPlaneScopeManifest = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                manifest.validate(self.capacity().kv_max_entries)?;
                if manifest.physical_key != key
                    || manifest.memory_space_id != memory_space_id
                    || manifest.mounted_subject_id != memory_space_id
                {
                    return Err(invalid("policy manifest differs from visual scope"));
                }
                Ok(StoreJsonPrecondition::Exact {
                    namespace: CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.into(),
                    key,
                    value,
                })
            }
            None => Ok(StoreJsonPrecondition::Absent {
                namespace: CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.into(),
                key,
            }),
        }
    }

    pub(crate) fn current_perception_policy_manifest(
        &self,
        memory_space_id: &str,
    ) -> Result<(u64, Option<String>)> {
        let precondition = self.perception_policy_manifest_precondition(memory_space_id)?;
        policy_snapshot_from_precondition(&precondition)
    }

    pub(crate) fn read_perception_source_fence(
        &self,
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
    ) -> Result<Option<PerceptionSourceFenceV1>> {
        let key = perception_source_fence_key(
            &scope.memory_space_id,
            &scope.subject_id,
            scope.relationship_id.as_deref(),
            source,
        )?;
        let value = self
            .read_json_docs_by_keys(
                PERCEPTION_SOURCE_FENCE_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
            .map(|document| document.value);
        let Some(value) = value else { return Ok(None) };
        let fence: PerceptionSourceFenceV1 =
            serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
        fence.validate_contract()?;
        if fence.key()? != key {
            return Err(invalid("source fence key differs from its owner"));
        }
        Ok(Some(fence))
    }

    pub(crate) fn assert_perception_source_live(
        &self,
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
        revoke_epoch: u64,
    ) -> Result<()> {
        if let Some(fence) = self.read_perception_source_fence(scope, source)? {
            if (source.source_generation, revoke_epoch) <= fence.revoked_tuple() {
                return Err(invalid(
                    "perception source generation was durably withdrawn",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn assert_perception_episode_replay_allowed(
        &self,
        source: &PerceptionEpisodeSourceRef,
    ) -> Result<PerceptionEpisodeEvidenceV1> {
        source.validate_contract()?;
        let key = perception_episode_storage_key(
            &source.memory_space_id,
            &source.subject_id,
            &source.source,
            &source.episode_id,
        )?;
        let value = self
            .read_json_docs_by_keys(PERCEPTION_EPISODE_NAMESPACE, std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("replayed episode is absent"))?
            .value;
        let episode: PerceptionEpisodeEvidenceV1 =
            serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
        episode.validate_contract()?;
        if episode.source_ref()? != *source
            || !self
                .read_json_docs_by_keys(
                    PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE,
                    std::slice::from_ref(&key),
                )?
                .is_empty()
        {
            return Err(invalid("replayed episode is missing or withdrawn"));
        }
        let historical_value = self
            .read_json_docs_by_keys(
                PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("replayed episode lacks historical authorization"))?
            .value;
        let historical: PerceptionHistoricalAuthorizationV1 =
            serde_json::from_value(historical_value).map_err(|error| invalid(error.to_string()))?;
        historical.validate_episode(&episode)?;
        let root_value = self
            .read_json_docs_by_keys(
                PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("replayed episode lacks dependents root"))?
            .value;
        let root: PerceptionSourceDependentsRootV1 =
            serde_json::from_value(root_value).map_err(|error| invalid(error.to_string()))?;
        root.validate_contract()?;
        if root.source != *source {
            return Err(invalid("replayed episode root differs from source"));
        }
        let identity = perception_source_fence_key(
            &source.memory_space_id,
            &source.subject_id,
            source.relationship_id.as_deref(),
            &source.source,
        )?;
        if !self
            .read_json_docs_by_keys(PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE, &[identity])?
            .is_empty()
        {
            return Err(invalid("replayed episode source is durably forbidden"));
        }
        Ok(episode)
    }

    fn plan_perception_source_fence_records(
        &self,
        candidates: impl IntoIterator<Item = PerceptionSourceFenceV1>,
    ) -> Result<(Vec<StoreMutation>, Vec<StoreJsonPrecondition>)> {
        let mut by_key = BTreeMap::<String, PerceptionSourceFenceV1>::new();
        for candidate in candidates {
            candidate.validate_contract()?;
            let key = candidate.key()?;
            match by_key.get_mut(&key) {
                Some(existing) => {
                    if candidate.revoked_tuple() > existing.revoked_tuple() {
                        existing.revoked_source_generation = candidate.revoked_source_generation;
                        existing.revoked_epoch = candidate.revoked_epoch;
                    }
                    existing.permission_revoked_through = existing
                        .permission_revoked_through
                        .max(candidate.permission_revoked_through);
                    existing.updated_at_unix_secs = existing
                        .updated_at_unix_secs
                        .max(candidate.updated_at_unix_secs);
                }
                None => {
                    by_key.insert(key, candidate);
                }
            }
        }
        let mut mutations = Vec::new();
        let mut preconditions = Vec::new();
        for (key, mut candidate) in by_key {
            let prior_value = self
                .read_json_docs_by_keys(
                    PERCEPTION_SOURCE_FENCE_NAMESPACE,
                    std::slice::from_ref(&key),
                )?
                .into_iter()
                .next()
                .map(|document| document.value);
            if let Some(value) = &prior_value {
                let prior: PerceptionSourceFenceV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                prior.validate_contract()?;
                if prior.key()? != key {
                    return Err(invalid("source fence owner is corrupt"));
                }
                if candidate.revoked_tuple() < prior.revoked_tuple() {
                    candidate.revoked_source_generation = prior.revoked_source_generation;
                    candidate.revoked_epoch = prior.revoked_epoch;
                }
                candidate.permission_revoked_through = candidate
                    .permission_revoked_through
                    .max(prior.permission_revoked_through);
                if candidate.revoked_tuple() == prior.revoked_tuple()
                    && candidate.permission_tuple() == prior.permission_tuple()
                {
                    continue;
                }
                candidate.updated_at_unix_secs = candidate
                    .updated_at_unix_secs
                    .max(prior.updated_at_unix_secs);
            }
            preconditions.push(match prior_value {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace: PERCEPTION_SOURCE_FENCE_NAMESPACE.into(),
                    key: key.clone(),
                    value,
                },
                None => StoreJsonPrecondition::Absent {
                    namespace: PERCEPTION_SOURCE_FENCE_NAMESPACE.into(),
                    key: key.clone(),
                },
            });
            mutations.push(StoreMutation::PutJson {
                namespace: PERCEPTION_SOURCE_FENCE_NAMESPACE.into(),
                key: key.clone(),
                value: serde_json::to_value(candidate)
                    .map_err(|error| invalid(error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryControl,
                plane: "perception_source_fence".into(),
                record_key: key,
            });
        }
        Ok((mutations, preconditions))
    }

    pub(crate) fn plan_perception_source_fence_for_withdrawal(
        &self,
        sources: &[PerceptionEpisodeSourceRef],
        now_secs: u64,
    ) -> Result<(Vec<StoreMutation>, Vec<StoreJsonPrecondition>)> {
        let candidates = sources
            .iter()
            .map(|source| PerceptionSourceFenceV1::from_episode_source(source, now_secs))
            .collect::<Result<Vec<_>>>()?;
        self.plan_perception_source_fence_records(candidates)
    }

    /// Persist a host permission/source withdrawal even when no episode was
    /// retained. A subsequent Runtime must use a strictly newer tuple.
    pub(crate) fn advance_perception_source_fence(
        &self,
        scope: &PerceptionScope,
        source: &PerceptionSourceKey,
        revoke_epoch: u64,
        event_scope: StoreEventScope,
        transaction_id: String,
        now_secs: u64,
    ) -> Result<Option<StoreMutationBatchReport>> {
        self.advance_perception_source_fences(
            &[(scope.clone(), source.clone(), revoke_epoch)],
            event_scope,
            transaction_id,
            now_secs,
        )
    }

    pub(crate) fn advance_perception_source_fences(
        &self,
        sources: &[(PerceptionScope, PerceptionSourceKey, u64)],
        event_scope: StoreEventScope,
        transaction_id: String,
        now_secs: u64,
    ) -> Result<Option<StoreMutationBatchReport>> {
        let candidates = sources
            .iter()
            .map(|(scope, source, revoke_epoch)| {
                if event_scope.memory_space_id != scope.memory_space_id
                    || event_scope.subject_id != scope.subject_id
                {
                    return Err(invalid("source fence event scope mismatch"));
                }
                PerceptionSourceFenceV1::from_scope(scope, source, *revoke_epoch, now_secs)
            })
            .collect::<Result<Vec<_>>>()?;
        let (mutations, preconditions) = self.plan_perception_source_fence_records(candidates)?;
        if mutations.is_empty() {
            return Ok(None);
        }
        let batch = StoreMutationBatch {
            transaction_id,
            operation: "perception_source_withdraw".into(),
            scope: event_scope,
            mutations,
        };
        let budget = self.current_runtime_budget(now_secs);
        self.commit_governed_memory_transaction_with_runtime_budget_at(
            batch,
            &preconditions,
            &budget,
            now_secs,
        )
        .map(Some)
    }
    fn load_perception_root_for_update(
        &self,
        changed: &mut BTreeMap<String, (PerceptionSourceDependentsRootV1, serde_json::Value)>,
        source: &PerceptionEpisodeSourceRef,
    ) -> Result<String> {
        let key = perception_episode_storage_key(
            &source.memory_space_id,
            &source.subject_id,
            &source.source,
            &source.episode_id,
        )?;
        if !changed.contains_key(&key) {
            let doc = self
                .read_json_docs_by_keys(
                    PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE,
                    std::slice::from_ref(&key),
                )?
                .into_iter()
                .next()
                .ok_or_else(|| invalid("visual source dependents root is missing"))?;
            let root: PerceptionSourceDependentsRootV1 = serde_json::from_value(doc.value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            root.validate_contract()?;
            if root.source != *source {
                return Err(invalid("visual source dependents root differs from source"));
            }
            changed.insert(key.clone(), (root, doc.value));
        }
        Ok(key)
    }

    pub(crate) fn plan_perception_long_term_root_edges(
        &self,
        mutations: &[StoreMutation],
    ) -> Result<(Vec<StoreMutation>, Vec<StoreJsonPrecondition>)> {
        self.plan_perception_transcript_withdrawal(&[], mutations)
    }

    pub(crate) fn plan_perception_transcript_withdrawal(
        &self,
        changed_records: &[(TranscriptTurnRecord, TranscriptTurnRecord)],
        material_mutations: &[StoreMutation],
    ) -> Result<(Vec<StoreMutation>, Vec<StoreJsonPrecondition>)> {
        let mut changed =
            BTreeMap::<String, (PerceptionSourceDependentsRootV1, serde_json::Value)>::new();
        for (before, after) in changed_records {
            if !after.perception_source_uses.is_empty() {
                continue;
            }
            let owner_key = crate::store_internal::platform::transcript_turn_storage_key(
                &before.key,
                &before.subject,
                &before.turn_id,
            );
            let before_digest = format!(
                "sha256:{:x}",
                Sha256::digest(
                    serde_json::to_vec(before).map_err(|error| invalid(error.to_string()))?
                )
            );
            for use_ref in &before.perception_source_uses {
                let source = &use_ref.source;
                let key = self.load_perception_root_for_update(&mut changed, source)?;
                let (root, _) = changed.get_mut(&key).expect("root inserted above");
                let prior_len = root.edges.len();
                root.edges.retain(|edge| {
                    !(edge.kind == PerceptionSourceDependentKindV1::TranscriptTurn
                        && edge.owner_key == owner_key
                        && edge.owner_revision == Some(before.sequence)
                        && edge.content_digest == before_digest)
                });
                if root.edges.len() + 1 != prior_len {
                    return Err(invalid("exact Transcript edge is missing or duplicated"));
                }
            }
        }
        for mutation in material_mutations {
            let (namespace, owner_key, material, inserting) = match mutation {
                StoreMutation::PutJson {
                    namespace,
                    key,
                    value,
                    ..
                } if namespace == crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE => (
                    namespace,
                    key,
                    serde_json::from_value::<bm_core::memory::LongTermMemoryVersionMaterial>(
                        value.clone(),
                    )
                    .map_err(|error| invalid(error.to_string()))?,
                    true,
                ),
                StoreMutation::DeleteJson { namespace, key, .. }
                    if namespace == crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE =>
                {
                    let value = self
                        .read_json_docs_by_keys(namespace, std::slice::from_ref(key))?
                        .into_iter()
                        .next()
                        .ok_or_else(|| invalid("deleted visual material is missing"))?
                        .value;
                    (
                        namespace,
                        key,
                        serde_json::from_value::<bm_core::memory::LongTermMemoryVersionMaterial>(
                            value,
                        )
                        .map_err(|error| invalid(error.to_string()))?,
                        false,
                    )
                }
                _ => continue,
            };
            let _ = namespace;
            for source in &material.governed_content.perception_sources {
                let key = self.load_perception_root_for_update(&mut changed, source)?;
                let (root, _) = changed.get_mut(&key).expect("root inserted above");
                let edge = PerceptionSourceDependentEdgeV1 {
                    kind: PerceptionSourceDependentKindV1::LongTermOwnerRevision,
                    memory_space_id: source.memory_space_id.clone(),
                    subject_id: source.subject_id.clone(),
                    relationship_id: source.relationship_id.clone(),
                    owner_key: owner_key.clone(),
                    owner_revision: Some(material.owner_revision),
                    content_digest: format!("sha256:{}", material.content_digest),
                };
                edge.validate_for(source)?;
                if inserting {
                    if root.edges.contains(&edge) {
                        return Err(invalid("visual long-term material edge already exists"));
                    }
                    root.edges.push(edge);
                } else {
                    let prior_len = root.edges.len();
                    root.edges.retain(|candidate| candidate != &edge);
                    if root.edges.len() + 1 != prior_len {
                        return Err(invalid(
                            "visual long-term material edge is missing or duplicated",
                        ));
                    }
                }
            }
        }
        let mut mutations = Vec::new();
        let mut preconditions = Vec::new();
        for (key, (root, before)) in changed {
            let next = PerceptionSourceDependentsRootV1::new(root.source, root.edges)?;
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
                key: key.clone(),
                value: before,
            });
            mutations.push(StoreMutation::PutJson {
                namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
                key: key.clone(),
                value: serde_json::to_value(next).map_err(|error| invalid(error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryControl,
                plane: "perception_source_dependents".into(),
                record_key: key,
            });
        }
        Ok((mutations, preconditions))
    }

    /// Fail-closed guard for the legacy free-text maintenance entry point.
    /// The key is intentionally opaque, so every typed taint in this local
    /// namespace is checked before a session is declared free of visual input.
    pub(crate) fn has_perception_taint_in_session(
        &self,
        memory_space_id: &str,
        subject_id: &str,
        session_id: &str,
    ) -> Result<bool> {
        for document in self.read_json_namespace(PERCEPTION_TURN_TAINT_NAMESPACE)? {
            let taint: PerceptionTurnTaintV1 = serde_json::from_value(document.value)
                .map_err(|error| invalid(error.to_string()))?;
            taint.validate_contract()?;
            if taint.key()? != document.key {
                return Err(invalid("perception taint owner key mismatch"));
            }
            if taint.memory_space_id == memory_space_id
                && taint.subject_id == subject_id
                && taint.session_id == session_id
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn has_other_perception_taint_in_session(
        &self,
        memory_space_id: &str,
        subject_id: &str,
        session_id: &str,
        except_turn_id: &str,
    ) -> Result<bool> {
        require_nonempty(except_turn_id, "except_turn_id")?;
        for document in self.read_json_namespace(PERCEPTION_TURN_TAINT_NAMESPACE)? {
            let taint: PerceptionTurnTaintV1 = serde_json::from_value(document.value)
                .map_err(|error| invalid(error.to_string()))?;
            taint.validate_contract()?;
            if taint.key()? != document.key {
                return Err(invalid("perception taint owner key mismatch"));
            }
            if taint.memory_space_id == memory_space_id
                && taint.subject_id == subject_id
                && taint.session_id == session_id
                && taint.turn_id != except_turn_id
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn read_perception_turn_taint(
        &self,
        memory_space_id: &str,
        subject_id: &str,
        session_id: &str,
        turn_id: &str,
    ) -> Result<Option<PerceptionTurnTaintV1>> {
        let key = perception_turn_taint_key(memory_space_id, subject_id, session_id, turn_id)?;
        self.read_json_docs_by_keys(PERCEPTION_TURN_TAINT_NAMESPACE, &[key])?
            .into_iter()
            .next()
            .map(|doc| {
                let taint: PerceptionTurnTaintV1 = serde_json::from_value(doc.value)
                    .map_err(|error| invalid(error.to_string()))?;
                taint.validate_contract()?;
                Ok(taint)
            })
            .transpose()
    }

    pub(crate) fn mark_perception_turn_taint(
        &self,
        scope: &PerceptionScope,
        input: PerceptionTurnTaintInput<'_>,
        event_scope: StoreEventScope,
        now_secs: u64,
    ) -> Result<PerceptionTurnTaintV1> {
        scope.validate_contract()?;
        if event_scope.memory_space_id != scope.memory_space_id
            || event_scope.subject_id != scope.subject_id
            || now_secs == 0
        {
            return Err(invalid("turn taint scope or time differs from Store owner"));
        }
        let turn_id = scope
            .turn_id
            .as_deref()
            .ok_or_else(|| invalid("turn taint requires a bound turn_id"))?;
        let key = perception_turn_taint_key(
            &scope.memory_space_id,
            &scope.subject_id,
            &scope.session_id,
            turn_id,
        )?;
        let commitment =
            perception_source_commitment(input.source, input.input_id, input.revoke_epoch)?;
        let before = self
            .read_json_docs_by_keys(PERCEPTION_TURN_TAINT_NAMESPACE, std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .map(|doc| doc.value);
        let mut taint = if let Some(value) = &before {
            let old: PerceptionTurnTaintV1 = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            old.validate_contract()?;
            if old.memory_space_id != scope.memory_space_id
                || old.subject_id != scope.subject_id
                || old.session_id != scope.session_id
                || old.turn_id != turn_id
            {
                return Err(invalid("turn taint owner differs from current scope"));
            }
            old
        } else {
            PerceptionTurnTaintV1 {
                schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
                memory_space_id: scope.memory_space_id.clone(),
                subject_id: scope.subject_id.clone(),
                session_id: scope.session_id.clone(),
                turn_id: turn_id.to_string(),
                source_commitments: Vec::new(),
                retention_ceiling: input.retention_ceiling,
                first_seen_at_unix_secs: now_secs,
                updated_at_unix_secs: now_secs,
            }
        };
        if !taint.source_commitments.contains(&commitment) {
            taint.source_commitments.push(commitment.clone());
            taint.source_commitments.sort();
        }
        taint.retention_ceiling = taint.retention_ceiling.min(input.retention_ceiling);
        taint.updated_at_unix_secs = now_secs.max(taint.updated_at_unix_secs);
        taint.validate_contract()?;
        let value = serde_json::to_value(&taint).map_err(|error| invalid(error.to_string()))?;
        if before.as_ref() == Some(&value) {
            return Ok(taint);
        }
        let precondition = match before {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace: PERCEPTION_TURN_TAINT_NAMESPACE.into(),
                key: key.clone(),
                value,
            },
            None => StoreJsonPrecondition::Absent {
                namespace: PERCEPTION_TURN_TAINT_NAMESPACE.into(),
                key: key.clone(),
            },
        };
        let transaction_id = format!(
            "perception-taint:{:x}",
            Sha256::digest(
                serde_json::to_vec(&(key.as_str(), commitment.as_str(), now_secs))
                    .map_err(|error| invalid(error.to_string()))?
            )
        );
        let batch = StoreMutationBatch {
            transaction_id,
            operation: "perception_turn_taint".into(),
            scope: event_scope,
            mutations: vec![StoreMutation::PutJson {
                namespace: PERCEPTION_TURN_TAINT_NAMESPACE.into(),
                key: key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryControl,
                plane: "perception_turn_taint".into(),
                record_key: key,
            }],
        };
        let budget = self.current_runtime_budget(now_secs);
        self.commit_governed_memory_transaction_with_runtime_budget_at(
            batch,
            &[precondition],
            &budget,
            now_secs,
        )?;
        Ok(taint)
    }

    pub(crate) fn persist_perception_episode(
        &self,
        episode: PerceptionEpisodeEvidenceV1,
        media_bytes: Option<Vec<u8>>,
        event_scope: StoreEventScope,
        transaction_id: String,
        now_secs: u64,
    ) -> Result<(StoreMutationBatchReport, PerceptionEpisodeSourceRef)> {
        episode.validate_contract()?;
        let source_ref = episode.source_ref()?;
        let key = episode.key()?;
        let value = serde_json::to_value(&episode).map_err(|error| invalid(error.to_string()))?;
        let media_edges = episode
            .media
            .iter()
            .map(|media| PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::MediaEvidence,
                memory_space_id: episode.memory_space_id.clone(),
                subject_id: episode.subject_id.clone(),
                relationship_id: episode.relationship_id.clone(),
                owner_key: media.key.clone(),
                owner_revision: None,
                content_digest: media.sha256.clone(),
            })
            .collect();
        let dependents = PerceptionSourceDependentsRootV1::new(source_ref.clone(), media_edges)?;
        let historical = historical_authorization_for_new_episode(self, &episode)?;
        let mut mutations = vec![
            StoreMutation::PutJson {
                namespace: PERCEPTION_EPISODE_NAMESPACE.into(),
                key: key.clone(),
                value,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "perception_evidence".into(),
                record_key: episode.episode_id.clone(),
            },
            StoreMutation::PutJson {
                namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
                key: key.clone(),
                value: serde_json::to_value(&dependents)
                    .map_err(|error| invalid(error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "perception_evidence".into(),
                record_key: episode.episode_id.clone(),
            },
            StoreMutation::PutJson {
                namespace: PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                key: key.clone(),
                value: serde_json::to_value(historical)
                    .map_err(|error| invalid(error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "perception_evidence".into(),
                record_key: episode.episode_id.clone(),
            },
        ];
        let mut blob_preconditions = Vec::new();
        match (&episode.media, media_bytes) {
            (Some(media), Some(bytes)) => {
                if bytes.len() != media.byte_len
                    || format!("sha256:{:x}", Sha256::digest(&bytes)) != media.sha256
                {
                    return Err(invalid("persisted media differs from owner digest"));
                }
                mutations.push(StoreMutation::PutBlob {
                    namespace: PERCEPTION_MEDIA_NAMESPACE.into(),
                    key: media.key.clone(),
                    value: bytes,
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "perception_evidence".into(),
                    record_key: episode.episode_id.clone(),
                });
                blob_preconditions.push(StoreBlobPrecondition::Absent {
                    namespace: PERCEPTION_MEDIA_NAMESPACE.into(),
                    key: media.key.clone(),
                });
            }
            (None, None) => {}
            _ => {
                return Err(invalid(
                    "media bytes and owner ref must be supplied together",
                ))
            }
        }
        let (count_mutations, count_preconditions) =
            self.plan_perception_source_episode_count_delta(&mutations)?;
        mutations.extend(count_mutations);
        let batch = StoreMutationBatch {
            transaction_id,
            operation: "perception_episode_persist".into(),
            scope: event_scope,
            mutations,
        };
        let mut preconditions = vec![
            StoreJsonPrecondition::Absent {
                namespace: PERCEPTION_EPISODE_NAMESPACE.into(),
                key: key.clone(),
            },
            StoreJsonPrecondition::Absent {
                namespace: PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(),
                key: key.clone(),
            },
            StoreJsonPrecondition::Absent {
                namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
                key: key.clone(),
            },
            StoreJsonPrecondition::Absent {
                namespace: PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                key,
            },
        ];
        preconditions.extend(count_preconditions);
        let budget = self.current_runtime_budget(now_secs);
        let report = self
            .commit_governed_memory_transaction_with_blob_preconditions_and_runtime_budget_at(
                batch,
                &preconditions,
                &blob_preconditions,
                &budget,
                now_secs,
            )?;
        Ok((report, source_ref))
    }

    pub(crate) fn plan_perception_episode_deletion(
        &self,
        source_ref: &PerceptionEpisodeSourceRef,
        now_secs: u64,
    ) -> Result<Option<PerceptionEpisodeDeletionPlan>> {
        source_ref
            .validate_contract()
            .map_err(|error| invalid(error.to_string()))?;
        let key = perception_episode_storage_key(
            &source_ref.memory_space_id,
            &source_ref.subject_id,
            &source_ref.source,
            &source_ref.episode_id,
        )?;
        let active =
            self.read_json_docs_by_keys(PERCEPTION_EPISODE_NAMESPACE, std::slice::from_ref(&key))?;
        let Some(document) = active.into_iter().next() else {
            let tombstones =
                self.read_json_docs_by_keys(PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE, &[key])?;
            if let Some(document) = tombstones.into_iter().next() {
                let tombstone: PerceptionEpisodeTombstoneV1 =
                    serde_json::from_value(document.value)
                        .map_err(|error| invalid(error.to_string()))?;
                if tombstone.withdrawn_content_digest == source_ref.content_digest {
                    return Ok(None);
                }
            }
            return Err(invalid(
                "episode source is absent or differs from tombstone",
            ));
        };
        let episode: PerceptionEpisodeEvidenceV1 = serde_json::from_value(document.value.clone())
            .map_err(|error| invalid(error.to_string()))?;
        if episode.source_ref()? != *source_ref {
            return Err(invalid("episode source differs from requested withdrawal"));
        }
        let roots = self.read_json_docs_by_keys(
            PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE,
            std::slice::from_ref(&key),
        )?;
        let root_doc = roots
            .into_iter()
            .next()
            .ok_or_else(|| invalid("episode dependents root is missing"))?;
        let root: PerceptionSourceDependentsRootV1 = serde_json::from_value(root_doc.value.clone())
            .map_err(|error| invalid(error.to_string()))?;
        root.validate_contract()?;
        if root.source != *source_ref {
            return Err(invalid(
                "episode dependents differ from requested withdrawal",
            ));
        }
        let historical_doc = self
            .read_json_docs_by_keys(
                PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE,
                std::slice::from_ref(&key),
            )?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("episode historical authorization is missing"))?;
        let historical: PerceptionHistoricalAuthorizationV1 =
            serde_json::from_value(historical_doc.value.clone())
                .map_err(|error| invalid(error.to_string()))?;
        historical.validate_episode(&episode)?;
        let tombstone = PerceptionEpisodeTombstoneV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: source_ref.memory_space_id.clone(),
            subject_id: source_ref.subject_id.clone(),
            source: source_ref.source.clone(),
            revoke_epoch: source_ref.revoke_epoch,
            episode_id: source_ref.episode_id.clone(),
            withdrawn_content_digest: source_ref.content_digest.clone(),
            deleted_at_unix_secs: now_secs,
        };
        tombstone.validate_contract()?;
        let mut mutations = vec![
            StoreMutation::DeleteJson {
                namespace: PERCEPTION_EPISODE_NAMESPACE.into(),
                key: key.clone(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "perception_evidence".into(),
                record_key: episode.episode_id.clone(),
            },
            StoreMutation::PutJson {
                namespace: PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(),
                key: key.clone(),
                value: serde_json::to_value(&tombstone)
                    .map_err(|error| invalid(error.to_string()))?,
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "perception_evidence".into(),
                record_key: episode.episode_id.clone(),
            },
            StoreMutation::DeleteJson {
                namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
                key: key.clone(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "perception_evidence".into(),
                record_key: episode.episode_id.clone(),
            },
            StoreMutation::DeleteJson {
                namespace: PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                key: key.clone(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "perception_evidence".into(),
                record_key: episode.episode_id.clone(),
            },
        ];
        let mut blob_preconditions = Vec::new();
        if let Some(media) = &episode.media {
            mutations.push(StoreMutation::DeleteBlob {
                namespace: PERCEPTION_MEDIA_NAMESPACE.into(),
                key: media.key.clone(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "perception_evidence".into(),
                record_key: episode.episode_id.clone(),
            });
            blob_preconditions.push(StoreBlobPrecondition::ExactDigest {
                namespace: PERCEPTION_MEDIA_NAMESPACE.into(),
                key: media.key.clone(),
                content_digest: media.sha256.clone(),
            });
        }
        let preconditions = vec![
            StoreJsonPrecondition::Exact {
                namespace: PERCEPTION_EPISODE_NAMESPACE.into(),
                key: key.clone(),
                value: document.value,
            },
            StoreJsonPrecondition::Absent {
                namespace: PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(),
                key: key.clone(),
            },
            StoreJsonPrecondition::Exact {
                namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
                key: key.clone(),
                value: root_doc.value,
            },
            StoreJsonPrecondition::Exact {
                namespace: PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                key,
                value: historical_doc.value,
            },
        ];
        Ok(Some(PerceptionEpisodeDeletionPlan {
            dependents: root,
            mutations,
            preconditions,
            blob_preconditions,
        }))
    }
}

/// Media and its typed owner must enter or leave the Store in one transaction.
/// Episodes are immutable; revisions use a new episode id and source lineage.
pub(crate) fn validate_perception_transaction_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<()> {
    let mut episode_deltas = BTreeMap::<String, i128>::new();
    for mutation in &batch.mutations {
        let (episode, delta) = match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == PERCEPTION_EPISODE_NAMESPACE => (
                serde_json::from_value::<PerceptionEpisodeEvidenceV1>(value.clone())
                    .map_err(|error| invalid(error.to_string()))?,
                1_i128,
            ),
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == PERCEPTION_EPISODE_NAMESPACE =>
            {
                let value = before
                    .json
                    .get(&(PERCEPTION_EPISODE_NAMESPACE.into(), key.clone()))
                    .ok_or_else(|| invalid("episode count delete lacks before-image"))?;
                (
                    serde_json::from_value::<PerceptionEpisodeEvidenceV1>(value.clone())
                        .map_err(|error| invalid(error.to_string()))?,
                    -1_i128,
                )
            }
            _ => continue,
        };
        episode.validate_contract()?;
        let key = perception_source_fence_key(
            &episode.memory_space_id,
            &episode.subject_id,
            episode.relationship_id.as_deref(),
            &episode.source,
        )?;
        *episode_deltas.entry(key).or_default() += delta;
    }
    let mut count_puts = BTreeSet::new();
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::DeleteJson { namespace, .. }
                if namespace == PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE
                    || namespace == PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE =>
            {
                return Err(invalid("perception source count or ban cannot be deleted"));
            }
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE =>
            {
                if !count_puts.insert(key.clone()) {
                    return Err(invalid("source episode count is mutated twice"));
                }
                let delta = episode_deltas
                    .get(key)
                    .ok_or_else(|| invalid("source episode count changed without episodes"))?;
                let value = after
                    .json
                    .get(&(
                        PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
                        key.clone(),
                    ))
                    .ok_or_else(|| invalid("episode count post-image is absent"))?;
                let count: PerceptionSourceEpisodeCountV1 =
                    serde_json::from_value(value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                count.validate_contract()?;
                let old = before
                    .json
                    .get(&(
                        PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
                        key.clone(),
                    ))
                    .map(|value| {
                        serde_json::from_value::<PerceptionSourceEpisodeCountV1>(value.clone())
                    })
                    .transpose()
                    .map_err(|error| invalid(error.to_string()))?;
                if count.key()? != *key
                    || count.memory_space_id != batch.scope.memory_space_id
                    || count.subject_id != batch.scope.subject_id
                    || count.revision != old.as_ref().map_or(1, |old| old.revision + 1)
                    || i128::from(count.active_count)
                        != old.as_ref().map_or(0, |old| i128::from(old.active_count)) + delta
                {
                    return Err(invalid("source episode count does not match exact delta"));
                }
            }
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE =>
            {
                let value = after
                    .json
                    .get(&(
                        PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                        key.clone(),
                    ))
                    .ok_or_else(|| invalid("source ban post-image is absent"))?;
                let ban: PerceptionSourceRetentionBanV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                ban.validate_contract()?;
                let old = before
                    .json
                    .get(&(
                        PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                        key.clone(),
                    ))
                    .map(|value| {
                        serde_json::from_value::<PerceptionSourceRetentionBanV1>(value.clone())
                    })
                    .transpose()
                    .map_err(|error| invalid(error.to_string()))?;
                let remaining = after
                    .json
                    .get(&(
                        PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
                        key.clone(),
                    ))
                    .map(|value| {
                        serde_json::from_value::<PerceptionSourceEpisodeCountV1>(value.clone())
                    })
                    .transpose()
                    .map_err(|error| invalid(error.to_string()))?;
                if ban.key()? != *key
                    || ban.memory_space_id != batch.scope.memory_space_id
                    || ban.subject_id != batch.scope.subject_id
                    || remaining
                        .as_ref()
                        .is_some_and(|count| count.active_count != 0)
                    || ban.policy_revision != old.as_ref().map_or(1, |old| old.policy_revision + 1)
                    || old.as_ref().is_some_and(|old| {
                        ban.retention_ceiling > old.retention_ceiling
                            || ban.updated_at_unix_secs < old.updated_at_unix_secs
                    })
                {
                    return Err(invalid("source ban lacks closed zero-episode post-image"));
                }
            }
            StoreMutation::DeleteJson { namespace, .. }
                if namespace == PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE =>
            {
                return Err(invalid("source authorization cannot be deleted"));
            }
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE =>
            {
                let value = after
                    .json
                    .get(&(
                        PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                        key.clone(),
                    ))
                    .ok_or_else(|| invalid("source authorization post-image is absent"))?;
                let authorization: PerceptionSourceAuthorizationV1 =
                    serde_json::from_value(value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                authorization.validate_contract()?;
                let old_authorization = before
                    .json
                    .get(&(
                        PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                        key.clone(),
                    ))
                    .map(|value| {
                        serde_json::from_value::<PerceptionSourceAuthorizationV1>(value.clone())
                    })
                    .transpose()
                    .map_err(|error| invalid(error.to_string()))?;
                let is_restrictive_same_source = old_authorization.as_ref().is_some_and(|old| {
                    authorization.tuple() == old.tuple()
                        && authorization.retention_ceiling <= old.retention_ceiling
                        && !authorization.retention_ceiling.permits_durable_semantic()
                });
                let policy_bound = if is_restrictive_same_source {
                    old_authorization.as_ref().is_some_and(|old| {
                        authorization.policy_manifest_revision == old.policy_manifest_revision
                            && authorization.policy_manifest_digest == old.policy_manifest_digest
                    })
                } else {
                    (
                        authorization.policy_manifest_revision,
                        authorization.policy_manifest_digest.clone(),
                    ) == policy_snapshot_from_state(after, &authorization.memory_space_id)?
                };
                if authorization.key()? != *key
                    || authorization.memory_space_id != batch.scope.memory_space_id
                    || authorization.subject_id != batch.scope.subject_id
                    || !policy_bound
                {
                    return Err(invalid("source authorization scope or policy mismatch"));
                }
                if authorization.retention_ceiling.permits_durable_semantic()
                    && broad_visual_retention_blocked_by_policy(
                        after,
                        &authorization.memory_space_id,
                        &authorization.subject_id,
                        authorization.updated_at_unix_secs,
                    )?
                {
                    return Err(invalid("broad memory prohibition denies source retention"));
                }
                if let Some(value) = after.json.get(&(
                    PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                    key.clone(),
                )) {
                    let ban: PerceptionSourceRetentionBanV1 = serde_json::from_value(value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                    ban.validate_contract()?;
                    if ban.key()? != *key || authorization.retention_ceiling > ban.retention_ceiling
                    {
                        return Err(invalid("source authorization exceeds durable ban"));
                    }
                }
                if let Some(value) = before.json.get(&(
                    PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                    key.clone(),
                )) {
                    let old: PerceptionSourceAuthorizationV1 =
                        serde_json::from_value(value.clone())
                            .map_err(|error| invalid(error.to_string()))?;
                    old.validate_contract()?;
                    if old.key()? != *key
                        || authorization.tuple() < old.tuple()
                        || authorization.authorization_generation <= old.authorization_generation
                    {
                        return Err(invalid("source authorization cannot move backward"));
                    }
                }
                if let Some(fence_value) = after
                    .json
                    .get(&(PERCEPTION_SOURCE_FENCE_NAMESPACE.into(), key.clone()))
                {
                    let fence: PerceptionSourceFenceV1 =
                        serde_json::from_value(fence_value.clone())
                            .map_err(|error| invalid(error.to_string()))?;
                    fence.validate_contract()?;
                    if fence.key()? != *key
                        || (authorization.tuple() <= fence.revoked_tuple()
                            && authorization.retention_ceiling.permits_durable_semantic())
                    {
                        return Err(invalid("source authorization is already withdrawn"));
                    }
                }
            }
            StoreMutation::DeleteJson { namespace, .. }
                if namespace == PERCEPTION_SOURCE_FENCE_NAMESPACE =>
            {
                return Err(invalid("perception source fence cannot be deleted"));
            }
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == PERCEPTION_SOURCE_FENCE_NAMESPACE =>
            {
                let value = after
                    .json
                    .get(&(PERCEPTION_SOURCE_FENCE_NAMESPACE.into(), key.clone()))
                    .ok_or_else(|| invalid("source fence post-image is absent"))?;
                let fence: PerceptionSourceFenceV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                fence.validate_contract()?;
                if fence.key()? != *key
                    || fence.memory_space_id != batch.scope.memory_space_id
                    || fence.subject_id != batch.scope.subject_id
                {
                    return Err(invalid("source fence owner or scope mismatch"));
                }
                if let Some(old_value) = before
                    .json
                    .get(&(PERCEPTION_SOURCE_FENCE_NAMESPACE.into(), key.clone()))
                {
                    let old: PerceptionSourceFenceV1 = serde_json::from_value(old_value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                    old.validate_contract()?;
                    if old.key()? != *key
                        || fence.revoked_tuple() < old.revoked_tuple()
                        || fence.permission_tuple() < old.permission_tuple()
                        || (fence.revoked_tuple() == old.revoked_tuple()
                            && fence.permission_tuple() == old.permission_tuple())
                        || fence.updated_at_unix_secs < old.updated_at_unix_secs
                    {
                        return Err(invalid("source fence cannot move backward or be rewritten"));
                    }
                }
            }
            StoreMutation::DeleteJson { namespace, .. }
                if namespace == PERCEPTION_TURN_TAINT_NAMESPACE =>
            {
                return Err(invalid("perception turn taint cannot be deleted"));
            }
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == PERCEPTION_TURN_TAINT_NAMESPACE =>
            {
                let value = after
                    .json
                    .get(&(PERCEPTION_TURN_TAINT_NAMESPACE.into(), key.clone()))
                    .ok_or_else(|| invalid("perception turn taint post-image is absent"))?;
                let taint: PerceptionTurnTaintV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                taint.validate_contract()?;
                if taint.key()? != *key
                    || taint.memory_space_id != batch.scope.memory_space_id
                    || taint.subject_id != batch.scope.subject_id
                {
                    return Err(invalid("perception turn taint scope mismatch"));
                }
                if let Some(old_value) = before
                    .json
                    .get(&(PERCEPTION_TURN_TAINT_NAMESPACE.into(), key.clone()))
                {
                    let old: PerceptionTurnTaintV1 = serde_json::from_value(old_value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                    old.validate_contract()?;
                    if old.first_seen_at_unix_secs != taint.first_seen_at_unix_secs
                        || old.updated_at_unix_secs > taint.updated_at_unix_secs
                        || old.retention_ceiling < taint.retention_ceiling
                        || !old
                            .source_commitments
                            .iter()
                            .all(|source| taint.source_commitments.contains(source))
                    {
                        return Err(invalid(
                            "perception turn taint cannot loosen or lose sources",
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    if count_puts != episode_deltas.keys().cloned().collect() {
        return Err(invalid("episode transaction lacks identity count CAS"));
    }
    let mut episode_puts = BTreeSet::new();
    let mut episode_deletes = BTreeSet::new();
    let mut tombstone_puts = BTreeSet::new();
    let mut root_puts = BTreeSet::new();
    let mut root_deletes = BTreeSet::new();
    let mut historical_puts = BTreeSet::new();
    let mut historical_deletes = BTreeSet::new();
    let mut media_puts = BTreeSet::new();
    let mut media_deletes = BTreeSet::new();
    for mutation in &batch.mutations {
        match mutation {
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == PERCEPTION_EPISODE_NAMESPACE =>
            {
                episode_puts.insert(key.as_str());
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == PERCEPTION_EPISODE_NAMESPACE =>
            {
                episode_deletes.insert(key.as_str());
            }
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE =>
            {
                tombstone_puts.insert(key.as_str());
            }
            StoreMutation::DeleteJson { namespace, .. }
                if namespace == PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE =>
            {
                return Err(invalid("episode tombstone cannot be deleted"));
            }
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE =>
            {
                root_puts.insert(key.as_str());
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE =>
            {
                root_deletes.insert(key.as_str());
            }
            StoreMutation::PutJson { namespace, key, .. }
                if namespace == PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE =>
            {
                historical_puts.insert(key.as_str());
            }
            StoreMutation::DeleteJson { namespace, key, .. }
                if namespace == PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE =>
            {
                historical_deletes.insert(key.as_str());
            }
            StoreMutation::PutBlob { namespace, key, .. }
                if namespace == PERCEPTION_MEDIA_NAMESPACE =>
            {
                media_puts.insert(key.as_str());
            }
            StoreMutation::DeleteBlob { namespace, key, .. }
                if namespace == PERCEPTION_MEDIA_NAMESPACE =>
            {
                media_deletes.insert(key.as_str());
            }
            _ => {}
        }
    }
    for mutation in &batch.mutations {
        let (namespace, key) = match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. } => (namespace, key),
            _ => continue,
        };
        if namespace == crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE {
            let old = before
                .json
                .get(&(namespace.clone(), key.clone()))
                .map(|value| {
                    serde_json::from_value::<bm_core::memory::LongTermMemoryVersionMaterial>(
                        value.clone(),
                    )
                })
                .transpose()
                .map_err(|error| invalid(error.to_string()))?;
            let new = after
                .json
                .get(&(namespace.clone(), key.clone()))
                .map(|value| {
                    serde_json::from_value::<bm_core::memory::LongTermMemoryVersionMaterial>(
                        value.clone(),
                    )
                })
                .transpose()
                .map_err(|error| invalid(error.to_string()))?;
            if old
                .as_ref()
                .map(|material| &material.governed_content.perception_sources)
                != new
                    .as_ref()
                    .map(|material| &material.governed_content.perception_sources)
            {
                let refs = old
                    .into_iter()
                    .chain(new)
                    .flat_map(|material| material.governed_content.perception_sources);
                for source in refs {
                    let episode_key = perception_episode_storage_key(
                        &source.memory_space_id,
                        &source.subject_id,
                        &source.source,
                        &source.episode_id,
                    )?;
                    let source_root_closed = root_puts.contains(episode_key.as_str())
                        || (root_deletes.contains(episode_key.as_str())
                            && episode_deletes.contains(episode_key.as_str())
                            && tombstone_puts.contains(episode_key.as_str()));
                    if !source_root_closed {
                        return Err(invalid(
                            "long-term visual lineage changed without its dependents root",
                        ));
                    }
                }
            }
            continue;
        }
        if namespace != "conversation_transcript" {
            continue;
        }
        let old = before
            .json
            .get(&(namespace.clone(), key.clone()))
            .map(|value| serde_json::from_value::<TranscriptTurnRecord>(value.clone()))
            .transpose()
            .map_err(|error| invalid(error.to_string()))?;
        let new = after
            .json
            .get(&(namespace.clone(), key.clone()))
            .map(|value| serde_json::from_value::<TranscriptTurnRecord>(value.clone()))
            .transpose()
            .map_err(|error| invalid(error.to_string()))?;
        if let Some(record) = &new {
            validate_transcript_external_visual_inventory(after, record)?;
            if let Some(manifest) = &record.external_content_manifest {
                for usage in &manifest.uses {
                    let TranscriptExternalContentUseV1::Visual { source, .. } = usage else {
                        continue;
                    };
                    let previously_attested = old.as_ref().is_some_and(|previous| {
                        previous
                            .external_content_manifest
                            .as_ref()
                            .is_some_and(|prior| prior.uses.contains(usage))
                    });
                    let episode_key = perception_episode_storage_key(
                        &source.memory_space_id,
                        &source.subject_id,
                        &source.source,
                        &source.episode_id,
                    )?;
                    if !previously_attested && !episode_puts.contains(episode_key.as_str()) {
                        return Err(invalid("fresh host visual claim lacks same-batch episode"));
                    }
                }
            }
        }
        let old_uses = old.as_ref().map(|record| &record.perception_source_uses);
        let new_uses = new.as_ref().map(|record| &record.perception_source_uses);
        if old_uses == new_uses {
            continue;
        }
        if let Some(record) = &new {
            for usage in &record.perception_source_uses {
                if let PerceptionReplyEvidenceV1::ReplayedOwner { owner } = &usage.delivery {
                    validate_replayed_source_in_state(
                        after,
                        &usage.source,
                        owner,
                        record.mounted_relationship_id.as_deref(),
                    )?;
                }
            }
        }
        let refs = old
            .into_iter()
            .chain(new)
            .flat_map(|record| record.perception_source_uses);
        for use_ref in refs {
            let source = &use_ref.source;
            let episode_key = perception_episode_storage_key(
                &source.memory_space_id,
                &source.subject_id,
                &source.source,
                &source.episode_id,
            )?;
            let source_root_closed = root_puts.contains(episode_key.as_str())
                || (root_deletes.contains(episode_key.as_str())
                    && episode_deletes.contains(episode_key.as_str())
                    && tombstone_puts.contains(episode_key.as_str()));
            if !source_root_closed {
                return Err(invalid(
                    "Transcript visual lineage changed without its dependents root",
                ));
            }
        }
    }
    if episode_puts.is_empty()
        && episode_deletes.is_empty()
        && tombstone_puts.is_empty()
        && root_puts.is_empty()
        && root_deletes.is_empty()
        && historical_puts.is_empty()
        && historical_deletes.is_empty()
        && media_puts.is_empty()
        && media_deletes.is_empty()
    {
        return Ok(());
    }
    if !episode_puts.is_disjoint(&episode_deletes) {
        return Err(invalid(
            "episode cannot be inserted and deleted in one transaction",
        ));
    }
    if episode_puts != historical_puts || episode_deletes != historical_deletes {
        return Err(invalid(
            "episode and historical authorization mutations differ",
        ));
    }
    for key in episode_puts {
        if before
            .json
            .contains_key(&(PERCEPTION_EPISODE_NAMESPACE.into(), key.into()))
            || after
                .json
                .contains_key(&(PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(), key.into()))
        {
            return Err(invalid("episode owner is immutable or withdrawn"));
        }
        let value = after
            .json
            .get(&(PERCEPTION_EPISODE_NAMESPACE.into(), key.into()))
            .ok_or_else(|| invalid("episode post-image is absent"))?;
        let episode: PerceptionEpisodeEvidenceV1 =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        episode.validate_contract()?;
        if episode.key()? != key
            || episode.memory_space_id != batch.scope.memory_space_id
            || episode.subject_id != batch.scope.subject_id
        {
            return Err(invalid("episode owner differs from transaction scope"));
        }
        let fence_key = perception_source_fence_key(
            &episode.memory_space_id,
            &episode.subject_id,
            episode.relationship_id.as_deref(),
            &episode.source,
        )?;
        if let Some(value) = after
            .json
            .get(&(PERCEPTION_SOURCE_FENCE_NAMESPACE.into(), fence_key.clone()))
        {
            let fence: PerceptionSourceFenceV1 = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            fence.validate_contract()?;
            if fence.key()? != fence_key
                || (episode.source.source_generation, episode.revoke_epoch) <= fence.revoked_tuple()
            {
                return Err(invalid("episode source generation was durably withdrawn"));
            }
        }
        let authorization_value = after
            .json
            .get(&(
                PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                fence_key.clone(),
            ))
            .ok_or_else(|| invalid("episode source lacks durable authorization"))?;
        let authorization: PerceptionSourceAuthorizationV1 =
            serde_json::from_value(authorization_value.clone())
                .map_err(|error| invalid(error.to_string()))?;
        authorization.validate_contract()?;
        if authorization.key()? != fence_key
            || authorization.source != episode.source
            || authorization.source_kind != episode.source_kind
            || authorization.revoke_epoch != episode.revoke_epoch
            || authorization.authorization_generation != episode.authorization_generation
            || episode.retention_ceiling > authorization.retention_ceiling
            || !authorization.user_consent_attested
            || (
                authorization.policy_manifest_revision,
                authorization.policy_manifest_digest.clone(),
            ) != policy_snapshot_from_state(after, &episode.memory_space_id)?
            || broad_visual_retention_blocked_by_policy(
                after,
                &episode.memory_space_id,
                &episode.subject_id,
                authorization.updated_at_unix_secs,
            )?
        {
            return Err(invalid("episode exceeds durable source authorization"));
        }
        let historical_value = after
            .json
            .get(&(
                PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                key.into(),
            ))
            .ok_or_else(|| invalid("new episode lacks historical authorization"))?;
        let historical: PerceptionHistoricalAuthorizationV1 =
            serde_json::from_value(historical_value.clone())
                .map_err(|error| invalid(error.to_string()))?;
        if before.json.contains_key(&(
            PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
            key.into(),
        )) || historical
            != project_perception_historical_authorization(&episode, &authorization)?
        {
            return Err(invalid(
                "new episode historical authorization differs from active",
            ));
        }
        if let Some(value) = after.json.get(&(
            PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
            fence_key.clone(),
        )) {
            let ban: PerceptionSourceRetentionBanV1 = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            ban.validate_contract()?;
            if ban.key()? != fence_key || episode.retention_ceiling > ban.retention_ceiling {
                return Err(invalid("episode exceeds source identity retention ban"));
            }
        }
        if let Some(media) = &episode.media {
            if !media_puts.remove(media.key.as_str()) {
                return Err(invalid("episode media blob is not in the same transaction"));
            }
            let bytes = after
                .blobs
                .get(&(PERCEPTION_MEDIA_NAMESPACE.into(), media.key.clone()))
                .ok_or_else(|| invalid("episode media post-image is absent"))?;
            if bytes.len() != media.byte_len
                || format!("sha256:{:x}", Sha256::digest(bytes)) != media.sha256
            {
                return Err(invalid("episode media bytes differ from owner digest"));
            }
        }
        if !root_puts.remove(key) {
            return Err(invalid("episode insert lacks a dependents root"));
        }
        let root_value = after
            .json
            .get(&(PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), key.into()))
            .ok_or_else(|| invalid("dependents root post-image is absent"))?;
        let root: PerceptionSourceDependentsRootV1 = serde_json::from_value(root_value.clone())
            .map_err(|error| invalid(error.to_string()))?;
        root.validate_contract()?;
        let mut expected_edges = episode
            .media
            .iter()
            .map(|media| PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::MediaEvidence,
                memory_space_id: episode.memory_space_id.clone(),
                subject_id: episode.subject_id.clone(),
                relationship_id: episode.relationship_id.clone(),
                owner_key: media.key.clone(),
                owner_revision: None,
                content_digest: media.sha256.clone(),
            })
            .collect::<Vec<_>>();
        let source_ref = episode.source_ref()?;
        let mut user_attachment_verified = !matches!(
            episode.origin_ref,
            PerceptionSourceRef::UserAttachment { .. }
        );
        for ((namespace, owner_key), value) in &after.json {
            if namespace != "conversation_transcript" {
                continue;
            }
            let record: TranscriptTurnRecord = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if !record
                .perception_source_uses
                .iter()
                .any(|usage| usage.source == source_ref)
            {
                continue;
            }
            if let PerceptionSourceRef::UserAttachment { message_id, .. } = &episode.origin_ref {
                user_attachment_verified |= record.input_messages.iter().any(|message| {
                    message.message_id == *message_id
                        && message.role == "user"
                        && message.authority
                            == bm_core::memory::MemoryEvidenceAuthority::UserAsserted
                });
            }
            let digest = format!(
                "sha256:{:x}",
                Sha256::digest(
                    serde_json::to_vec(&record).map_err(|error| invalid(error.to_string()))?
                )
            );
            expected_edges.push(PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::TranscriptTurn,
                memory_space_id: record.key.memory_space_id.clone(),
                subject_id: record.subject.clone(),
                relationship_id: episode.relationship_id.clone(),
                owner_key: owner_key.clone(),
                owner_revision: Some(record.sequence),
                content_digest: digest,
            });
        }
        if !user_attachment_verified {
            return Err(invalid(
                "user-shared episode has no exact user message owner",
            ));
        }
        append_live_long_term_edges(after, &source_ref, &mut expected_edges)?;
        expected_edges.extend(live_soul_edges_for_source(&after.json, &source_ref)?);
        let expected_root = PerceptionSourceDependentsRootV1::new(source_ref, expected_edges)?;
        if root != expected_root {
            return Err(invalid(
                "new dependents root differs from exact episode owners",
            ));
        }
    }
    for key in std::mem::take(&mut root_puts) {
        let before_episode = before
            .json
            .get(&(PERCEPTION_EPISODE_NAMESPACE.into(), key.into()))
            .ok_or_else(|| invalid("dependents update lacks an existing episode"))?;
        if after
            .json
            .get(&(PERCEPTION_EPISODE_NAMESPACE.into(), key.into()))
            != Some(before_episode)
        {
            return Err(invalid("dependents update cannot change episode owner"));
        }
        let episode: PerceptionEpisodeEvidenceV1 = serde_json::from_value(before_episode.clone())
            .map_err(|error| invalid(error.to_string()))?;
        episode.validate_contract()?;
        if episode.key()? != key {
            return Err(invalid("dependents update episode key mismatch"));
        }
        let before_root = before
            .json
            .get(&(PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), key.into()))
            .ok_or_else(|| invalid("dependents update lacks an existing root"))?;
        let old: PerceptionSourceDependentsRootV1 = serde_json::from_value(before_root.clone())
            .map_err(|error| invalid(error.to_string()))?;
        old.validate_contract()?;
        if old.source != episode.source_ref()? {
            return Err(invalid("dependents update source differs from episode"));
        }
        let after_root = after
            .json
            .get(&(PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), key.into()))
            .ok_or_else(|| invalid("dependents update post-image is absent"))?;
        let new: PerceptionSourceDependentsRootV1 = serde_json::from_value(after_root.clone())
            .map_err(|error| invalid(error.to_string()))?;
        new.validate_contract()?;
        let source_ref = episode.source_ref()?;
        let mut expected_edges = episode
            .media
            .iter()
            .map(|media| PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::MediaEvidence,
                memory_space_id: episode.memory_space_id.clone(),
                subject_id: episode.subject_id.clone(),
                relationship_id: episode.relationship_id.clone(),
                owner_key: media.key.clone(),
                owner_revision: None,
                content_digest: media.sha256.clone(),
            })
            .collect::<Vec<_>>();
        for ((namespace, owner_key), value) in &after.json {
            if namespace != "conversation_transcript" {
                continue;
            }
            let record: TranscriptTurnRecord = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if record
                .perception_source_uses
                .iter()
                .any(|usage| usage.source == source_ref)
            {
                expected_edges.push(PerceptionSourceDependentEdgeV1 {
                    kind: PerceptionSourceDependentKindV1::TranscriptTurn,
                    memory_space_id: record.key.memory_space_id.clone(),
                    subject_id: record.subject.clone(),
                    relationship_id: episode.relationship_id.clone(),
                    owner_key: owner_key.clone(),
                    owner_revision: Some(record.sequence),
                    content_digest: format!(
                        "sha256:{:x}",
                        Sha256::digest(
                            serde_json::to_vec(&record)
                                .map_err(|error| invalid(error.to_string()))?
                        )
                    ),
                });
            }
        }
        append_live_long_term_edges(after, &source_ref, &mut expected_edges)?;
        expected_edges.extend(live_soul_edges_for_source(&after.json, &source_ref)?);
        if new != PerceptionSourceDependentsRootV1::new(source_ref, expected_edges)? {
            return Err(invalid(
                "dependents update does not match exact live owners",
            ));
        }
    }
    for key in episode_deletes {
        let value = before
            .json
            .get(&(PERCEPTION_EPISODE_NAMESPACE.into(), key.into()))
            .ok_or_else(|| invalid("episode delete lacks a typed before-image"))?;
        let episode: PerceptionEpisodeEvidenceV1 =
            serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
        episode.validate_contract()?;
        if episode.key()? != key
            || episode.memory_space_id != batch.scope.memory_space_id
            || episode.subject_id != batch.scope.subject_id
        {
            return Err(invalid("episode delete differs from transaction scope"));
        }
        let fence_key = perception_source_fence_key(
            &episode.memory_space_id,
            &episode.subject_id,
            episode.relationship_id.as_deref(),
            &episode.source,
        )?;
        let fence_value = after
            .json
            .get(&(PERCEPTION_SOURCE_FENCE_NAMESPACE.into(), fence_key.clone()))
            .ok_or_else(|| invalid("episode delete lacks durable source fence"))?;
        let fence: PerceptionSourceFenceV1 = serde_json::from_value(fence_value.clone())
            .map_err(|error| invalid(error.to_string()))?;
        fence.validate_contract()?;
        if fence.key()? != fence_key
            || fence.revoked_tuple() < (episode.source.source_generation, episode.revoke_epoch)
        {
            return Err(invalid("episode delete lacks advanced source fence"));
        }
        if !tombstone_puts.remove(key) {
            return Err(invalid("episode delete lacks a durable tombstone"));
        }
        if !root_deletes.remove(key) {
            return Err(invalid("episode delete lacks dependents root deletion"));
        }
        let historical_value = before
            .json
            .get(&(
                PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                key.into(),
            ))
            .ok_or_else(|| invalid("episode delete lacks historical authorization"))?;
        let historical: PerceptionHistoricalAuthorizationV1 =
            serde_json::from_value(historical_value.clone())
                .map_err(|error| invalid(error.to_string()))?;
        if historical.key()? != key
            || after.json.contains_key(&(
                PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                key.into(),
            ))
        {
            return Err(invalid("episode delete leaves historical authorization"));
        }
        historical.validate_episode(&episode)?;
        let tombstone_value = after
            .json
            .get(&(PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(), key.into()))
            .ok_or_else(|| invalid("episode tombstone post-image is absent"))?;
        let tombstone: PerceptionEpisodeTombstoneV1 =
            serde_json::from_value(tombstone_value.clone())
                .map_err(|error| invalid(error.to_string()))?;
        if tombstone.key()? != key
            || tombstone.withdrawn_content_digest != episode.content_digest()?
            || tombstone.revoke_epoch != episode.revoke_epoch
        {
            return Err(invalid("episode tombstone differs from withdrawn source"));
        }
        let root_value = before
            .json
            .get(&(PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), key.into()))
            .ok_or_else(|| invalid("episode delete lacks dependents before-image"))?;
        let root: PerceptionSourceDependentsRootV1 = serde_json::from_value(root_value.clone())
            .map_err(|error| invalid(error.to_string()))?;
        root.validate_contract()?;
        let source_ref = episode.source_ref()?;
        if root.source != source_ref
            || after
                .json
                .contains_key(&(PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), key.into()))
        {
            return Err(invalid("episode delete has unresolved dependents"));
        }
        if root.edges.iter().any(|edge| {
            !matches!(
                edge.kind,
                PerceptionSourceDependentKindV1::MediaEvidence
                    | PerceptionSourceDependentKindV1::TranscriptTurn
                    | PerceptionSourceDependentKindV1::LongTermOwnerRevision
                    | PerceptionSourceDependentKindV1::SoulRoot
                    | PerceptionSourceDependentKindV1::SoulRevision
            )
        }) {
            return Err(invalid("episode delete has unsupported dependent kind"));
        }
        for ((namespace, _), value) in &after.json {
            if namespace != "conversation_transcript" {
                continue;
            }
            let record: TranscriptTurnRecord = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if record
                .perception_source_uses
                .iter()
                .any(|usage| usage.source == source_ref)
            {
                return Err(invalid("episode delete leaves a live Transcript dependent"));
            }
        }
        let mut live_long_term = Vec::new();
        append_live_long_term_edges(after, &source_ref, &mut live_long_term)?;
        if !live_long_term.is_empty()
            || !live_soul_edges_for_source(&after.json, &source_ref)?.is_empty()
        {
            return Err(invalid("episode delete leaves a live derived dependent"));
        }
        if let Some(media) = &episode.media {
            if !media_deletes.remove(media.key.as_str()) {
                return Err(invalid("episode media is not deleted atomically"));
            }
            if after
                .blobs
                .contains_key(&(PERCEPTION_MEDIA_NAMESPACE.into(), media.key.clone()))
            {
                return Err(invalid("episode media remains after deletion"));
            }
        }
    }
    if !media_puts.is_empty()
        || !media_deletes.is_empty()
        || !tombstone_puts.is_empty()
        || !root_puts.is_empty()
        || !root_deletes.is_empty()
    {
        return Err(invalid("orphan perception media or tombstone mutation"));
    }
    Ok(())
}

/// Full import gate. A snapshot is admitted only when every visual owner and
/// reverse dependency survives as a closed typed graph, including media bytes.
pub(crate) fn validate_perception_store_image(state: &BackendTransactionState) -> Result<()> {
    let mut episodes = BTreeMap::<String, PerceptionEpisodeEvidenceV1>::new();
    let mut roots = BTreeMap::<String, PerceptionSourceDependentsRootV1>::new();
    let mut historical_authorizations =
        BTreeMap::<String, PerceptionHistoricalAuthorizationV1>::new();
    let mut active_counts = BTreeMap::<String, u64>::new();
    for ((namespace, key), value) in &state.json {
        match namespace.as_str() {
            PERCEPTION_EPISODE_NAMESPACE => {
                let episode: PerceptionEpisodeEvidenceV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                episode.validate_contract()?;
                if episode.key()? != *key {
                    return Err(invalid("snapshot episode physical key mismatch"));
                }
                let identity = perception_source_fence_key(
                    &episode.memory_space_id,
                    &episode.subject_id,
                    episode.relationship_id.as_deref(),
                    &episode.source,
                )?;
                *active_counts.entry(identity).or_default() += 1;
                episodes.insert(key.clone(), episode);
            }
            PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE => {
                let root: PerceptionSourceDependentsRootV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                root.validate_contract()?;
                if perception_episode_storage_key(
                    &root.source.memory_space_id,
                    &root.source.subject_id,
                    &root.source.source,
                    &root.source.episode_id,
                )? != *key
                {
                    return Err(invalid("snapshot dependents root physical key mismatch"));
                }
                roots.insert(key.clone(), root);
            }
            PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE => {
                let tombstone: PerceptionEpisodeTombstoneV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                tombstone.validate_contract()?;
                if tombstone.key()? != *key {
                    return Err(invalid("snapshot tombstone physical key mismatch"));
                }
            }
            PERCEPTION_SOURCE_FENCE_NAMESPACE => {
                let fence: PerceptionSourceFenceV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                fence.validate_contract()?;
                if fence.key()? != *key {
                    return Err(invalid("snapshot source fence key mismatch"));
                }
            }
            PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE => {
                let authorization: PerceptionSourceAuthorizationV1 =
                    serde_json::from_value(value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                authorization.validate_contract()?;
                if authorization.key()? != *key {
                    return Err(invalid("snapshot source authorization key mismatch"));
                }
                if let Some(ban_value) = state.json.get(&(
                    PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                    key.clone(),
                )) {
                    let ban: PerceptionSourceRetentionBanV1 =
                        serde_json::from_value(ban_value.clone())
                            .map_err(|error| invalid(error.to_string()))?;
                    ban.validate_contract()?;
                    if ban.key()? != *key || authorization.retention_ceiling > ban.retention_ceiling
                    {
                        return Err(invalid("snapshot live authorization exceeds source ban"));
                    }
                }
            }
            PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE => {
                let historical: PerceptionHistoricalAuthorizationV1 =
                    serde_json::from_value(value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                historical.validate_contract()?;
                if historical.key()? != *key {
                    return Err(invalid("snapshot historical authorization key mismatch"));
                }
                historical_authorizations.insert(key.clone(), historical);
            }
            PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE => {
                let ban: PerceptionSourceRetentionBanV1 = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                ban.validate_contract()?;
                if ban.key()? != *key {
                    return Err(invalid("snapshot source ban key mismatch"));
                }
            }
            PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE => {
                let count: PerceptionSourceEpisodeCountV1 =
                    serde_json::from_value(value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                count.validate_contract()?;
                if count.key()? != *key {
                    return Err(invalid("snapshot source count key mismatch"));
                }
            }
            _ => {}
        }
    }
    for (key, episode) in &episodes {
        if state
            .json
            .contains_key(&(PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(), key.clone()))
        {
            return Err(invalid("snapshot episode has tombstone"));
        }
        let source = episode.source_ref()?;
        let root = roots
            .get(key)
            .ok_or_else(|| invalid("snapshot episode lacks dependents root"))?;
        if root.source != source {
            return Err(invalid("snapshot episode root differs from source"));
        }
        let identity = perception_source_fence_key(
            &episode.memory_space_id,
            &episode.subject_id,
            episode.relationship_id.as_deref(),
            &episode.source,
        )?;
        historical_authorizations
            .get(key)
            .ok_or_else(|| invalid("snapshot episode lacks historical authorization"))?
            .validate_episode(episode)?;
        let count_value = state
            .json
            .get(&(PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(), identity))
            .ok_or_else(|| invalid("snapshot episode lacks source count"))?;
        let count: PerceptionSourceEpisodeCountV1 = serde_json::from_value(count_value.clone())
            .map_err(|error| invalid(error.to_string()))?;
        if count.active_count != active_counts.get(&count.key()?).copied().unwrap_or(0) {
            return Err(invalid("snapshot episode count differs from active owners"));
        }
        if let Some(media) = &episode.media {
            let bytes = state
                .blobs
                .get(&(PERCEPTION_MEDIA_NAMESPACE.into(), media.key.clone()))
                .ok_or_else(|| invalid("snapshot media owner blob is missing"))?;
            if bytes.len() != media.byte_len
                || format!("sha256:{:x}", Sha256::digest(bytes)) != media.sha256
            {
                return Err(invalid("snapshot media blob differs from owner digest"));
            }
        }
        let mut expected = Vec::new();
        let mut user_attachment_verified = !matches!(
            episode.origin_ref,
            PerceptionSourceRef::UserAttachment { .. }
        );
        if let Some(media) = &episode.media {
            expected.push(PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::MediaEvidence,
                memory_space_id: source.memory_space_id.clone(),
                subject_id: source.subject_id.clone(),
                relationship_id: source.relationship_id.clone(),
                owner_key: media.key.clone(),
                owner_revision: None,
                content_digest: media.sha256.clone(),
            });
        }
        for ((namespace, owner_key), value) in &state.json {
            if namespace == "conversation_transcript" {
                let record: TranscriptTurnRecord = serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                if record
                    .perception_source_uses
                    .iter()
                    .any(|usage| usage.source == source)
                {
                    if record.key.memory_space_id != source.memory_space_id
                        || record.subject != source.subject_id
                    {
                        return Err(invalid("snapshot visual Transcript crosses source scope"));
                    }
                    for usage in &record.perception_source_uses {
                        if usage.source == source {
                            if let PerceptionReplyEvidenceV1::ReplayedOwner { owner } =
                                &usage.delivery
                            {
                                validate_replayed_source_in_state(
                                    state,
                                    &usage.source,
                                    owner,
                                    record.mounted_relationship_id.as_deref(),
                                )?;
                            }
                        }
                    }
                    if let PerceptionSourceRef::UserAttachment { message_id, .. } =
                        &episode.origin_ref
                    {
                        user_attachment_verified |= record.input_messages.iter().any(|message| {
                            message.message_id == *message_id
                                && message.role == "user"
                                && message.authority
                                    == bm_core::memory::MemoryEvidenceAuthority::UserAsserted
                        });
                    }
                    expected.push(PerceptionSourceDependentEdgeV1 {
                        kind: PerceptionSourceDependentKindV1::TranscriptTurn,
                        memory_space_id: source.memory_space_id.clone(),
                        subject_id: source.subject_id.clone(),
                        relationship_id: source.relationship_id.clone(),
                        owner_key: owner_key.clone(),
                        owner_revision: Some(record.sequence),
                        content_digest: format!(
                            "sha256:{:x}",
                            Sha256::digest(
                                serde_json::to_vec(&record)
                                    .map_err(|error| invalid(error.to_string()))?
                            )
                        ),
                    });
                }
            } else if namespace == crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE {
                let material: bm_core::memory::LongTermMemoryVersionMaterial =
                    serde_json::from_value(value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                if material
                    .governed_content
                    .perception_sources
                    .contains(&source)
                {
                    if material.memory_space_id != source.memory_space_id {
                        return Err(invalid(
                            "snapshot visual long-term owner crosses source scope",
                        ));
                    }
                    expected.push(PerceptionSourceDependentEdgeV1 {
                        kind: PerceptionSourceDependentKindV1::LongTermOwnerRevision,
                        memory_space_id: source.memory_space_id.clone(),
                        subject_id: source.subject_id.clone(),
                        relationship_id: source.relationship_id.clone(),
                        owner_key: owner_key.clone(),
                        owner_revision: Some(material.owner_revision),
                        content_digest: format!("sha256:{}", material.content_digest),
                    });
                }
            }
        }
        if !user_attachment_verified {
            return Err(invalid(
                "snapshot user-shared visual lacks exact user message",
            ));
        }
        expected.extend(live_soul_edges_for_source(&state.json, &source)?);
        let exact_root = PerceptionSourceDependentsRootV1::new(source, expected)?;
        if *root != exact_root {
            return Err(invalid(
                "snapshot visual reverse source graph is incomplete",
            ));
        }
    }
    for key in roots.keys() {
        if !episodes.contains_key(key) {
            return Err(invalid("snapshot has orphan perception dependents root"));
        }
    }
    for key in historical_authorizations.keys() {
        if !episodes.contains_key(key) {
            return Err(invalid("snapshot has orphan historical authorization"));
        }
    }
    for ((namespace, key), value) in &state.json {
        if namespace == "conversation_transcript" {
            let record: TranscriptTurnRecord = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            validate_transcript_external_visual_inventory(state, &record)?;
            for usage in &record.perception_source_uses {
                let source = &usage.source;
                let episode_key = perception_episode_storage_key(
                    &source.memory_space_id,
                    &source.subject_id,
                    &source.source,
                    &source.episode_id,
                )?;
                if episodes
                    .get(&episode_key)
                    .is_none_or(|episode| episode.source_ref().ok().as_ref() != Some(source))
                {
                    return Err(invalid("snapshot Transcript has dangling visual source"));
                }
            }
        }
        if namespace == crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE {
            let material: bm_core::memory::LongTermMemoryVersionMaterial =
                serde_json::from_value(value.clone())
                    .map_err(|error| invalid(error.to_string()))?;
            for source in &material.governed_content.perception_sources {
                let episode_key = perception_episode_storage_key(
                    &source.memory_space_id,
                    &source.subject_id,
                    &source.source,
                    &source.episode_id,
                )?;
                if episodes
                    .get(&episode_key)
                    .is_none_or(|episode| episode.source_ref().ok().as_ref() != Some(source))
                {
                    return Err(invalid(
                        "snapshot long-term owner has dangling visual source",
                    ));
                }
            }
        }
        if namespace == PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE {
            let count: PerceptionSourceEpisodeCountV1 = serde_json::from_value(value.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if count.active_count != active_counts.get(key).copied().unwrap_or(0) {
                return Err(invalid("snapshot source episode count is not exact"));
            }
        }
        if namespace == PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE
            && active_counts.get(key).copied().unwrap_or(0) != 0
        {
            return Err(invalid("snapshot source ban retains active episodes"));
        }
    }
    for (namespace, key) in state.blobs.keys() {
        if namespace == PERCEPTION_MEDIA_NAMESPACE
            && !episodes.values().any(|episode| {
                episode
                    .media
                    .as_ref()
                    .is_some_and(|media| media.key == *key)
            })
        {
            return Err(invalid("snapshot has orphan perception media blob"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod deletion_tests {
    use super::*;

    fn perception_scope() -> PerceptionScope {
        PerceptionScope {
            memory_space_id: "space".into(),
            subject_id: "soul".into(),
            relationship_id: None,
            session_id: "session".into(),
            turn_id: None,
            step_id: None,
        }
    }

    fn broad_pause_policy_state() -> BackendTransactionState {
        use crate::store_internal::schema::{
            control_plane_scope_manifest_key, ControlPlaneScopeEntry, ControlPlaneScopeManifest,
            CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE,
        };
        let policy = bm_core::memory::MemoryLongTermGovernancePolicy {
            schema_version: bm_core::memory::LONG_TERM_CONTROL_SCHEMA_VERSION,
            policy_revision: 1,
            memory_space_id: "space".into(),
            policy_id: "pause-all".into(),
            kind: "pause".into(),
            selector: bm_core::memory::MemoryGovernanceSelector {
                memory_space_id: Some("space".into()),
                subject_id: None,
                kind: None,
                topic_pattern: None,
                source_chat_id: None,
                source_scope: None,
            },
            duration: None,
            expires_at: None,
            reason: "test".into(),
            created_at: 1,
            updated_at: 1,
        };
        let policy_value = serde_json::to_value(policy).expect("policy JSON");
        let namespace = bm_core::memory::LONG_TERM_GOVERNANCE_POLICY_NAMESPACE;
        let policy_key = "policy:pause-all";
        let entry = ControlPlaneScopeEntry::from_json(namespace, policy_key, &policy_value)
            .expect("policy entry");
        let manifest = ControlPlaneScopeManifest::build(1, "space", "space", [entry], 100)
            .expect("policy manifest");
        let manifest_key =
            control_plane_scope_manifest_key("space", "space").expect("manifest key");
        let mut state = BackendTransactionState::default();
        state
            .json
            .insert((namespace.into(), policy_key.into()), policy_value);
        state.json.insert(
            (CONTROL_PLANE_SCOPE_MANIFEST_NAMESPACE.into(), manifest_key),
            serde_json::to_value(manifest).expect("manifest JSON"),
        );
        state
    }

    fn digest(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn episode() -> PerceptionEpisodeEvidenceV1 {
        PerceptionEpisodeEvidenceV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: "space".into(),
            subject_id: "soul".into(),
            relationship_id: None,
            source: PerceptionSourceKey {
                host_id: "host".into(),
                device_id: Some("device".into()),
                source_id: "camera".into(),
                source_generation: 1,
            },
            revoke_epoch: 1,
            authorization_generation: 1,
            episode_id: "episode".into(),
            input_id: "input".into(),
            modality: PerceptionModality::Visual,
            source_kind: PerceptionSourceKind::HostVisualObservation,
            origin_ref: PerceptionSourceRef::Sensor {
                sensor_id: "camera".into(),
            },
            time: PerceptionTime {
                observed_at_unix_ms: Some(1),
                presented_at_unix_ms: 2,
            },
            retention_ceiling: PerceptionRetentionCeiling::DurableSemantic,
            audio_free_video_sha256: None,
            observation: None,
            delivery: Some(PerceptionDeliveryEvidenceV1 {
                input_id: "input".into(),
                packet_id: "packet".into(),
                turn_id: "turn".into(),
                final_request_sha256: digest('a'),
                sent_media_sha256: digest('b'),
                receipt_sha256: digest('c'),
            }),
            media: None,
        }
    }

    fn closed_snapshot_image() -> BackendTransactionState {
        let episode = episode();
        let source = episode.source_ref().expect("source ref");
        let key = episode.key().expect("episode key");
        let identity = perception_source_fence_key(
            &episode.memory_space_id,
            &episode.subject_id,
            episode.relationship_id.as_deref(),
            &episode.source,
        )
        .expect("source identity");
        let root =
            PerceptionSourceDependentsRootV1::new(source, Vec::new()).expect("empty source root");
        let authorization = PerceptionSourceAuthorizationV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: episode.memory_space_id.clone(),
            subject_id: episode.subject_id.clone(),
            relationship_id: episode.relationship_id.clone(),
            source: episode.source.clone(),
            source_kind: episode.source_kind,
            revoke_epoch: episode.revoke_epoch,
            authorization_generation: episode.authorization_generation,
            retention_ceiling: episode.retention_ceiling,
            user_consent_attested: true,
            policy_manifest_revision: 0,
            policy_manifest_digest: None,
            updated_at_unix_secs: 1,
        };
        let historical = project_perception_historical_authorization(&episode, &authorization)
            .expect("historical authorization");
        let count = PerceptionSourceEpisodeCountV1::from_episode(&episode, 1, 1);
        let mut state = BackendTransactionState::default();
        for (namespace, key, value) in [
            (
                PERCEPTION_EPISODE_NAMESPACE,
                key.clone(),
                serde_json::to_value(episode).expect("episode JSON"),
            ),
            (
                PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE,
                key.clone(),
                serde_json::to_value(root).expect("root JSON"),
            ),
            (
                PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE,
                identity.clone(),
                serde_json::to_value(authorization).expect("auth JSON"),
            ),
            (
                PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE,
                key.clone(),
                serde_json::to_value(historical).expect("historical JSON"),
            ),
            (
                PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE,
                identity,
                serde_json::to_value(count).expect("count JSON"),
            ),
        ] {
            state.json.insert((namespace.into(), key), value);
        }
        state
    }

    fn media_episode() -> PerceptionEpisodeEvidenceV1 {
        let mut episode = episode();
        episode.retention_ceiling = PerceptionRetentionCeiling::DurableMedia;
        episode.observation = Some(VisualObservation {
            input_id: episode.input_id.clone(),
            source_key: episode.source.clone(),
            scope: perception_scope(),
            revoke_epoch: episode.revoke_epoch,
            observed_at_unix_ms: episode.time.observed_at_unix_ms,
            text: "今天穿了红裙子。".into(),
            search_terms: Vec::new(),
            uncertainty: bm_core::memory::VisualObservationUncertainty::Low,
            classification: bm_core::memory::VisualClassification {
                subject_attribution: bm_core::memory::SubjectAttribution::ContextualCurrentUser,
                attribution_confidence: bm_core::memory::AttributionConfidence::High,
                visible_person_count: Some(1),
                third_party_present: false,
                sensitivity: bm_core::memory::VisualSensitivity::Ordinary,
                minor_present: false,
                sensitive_document_present: false,
                attribution_evidence_refs: vec!["interaction".into()],
                conflicting_evidence: false,
            },
            evidence_origin: bm_core::memory::VisualEvidenceOrigin::HostAttestedReply {
                delivery_receipt_digest: digest('c'),
                reply_digest: digest('d'),
            },
        });
        episode.media = Some(PerceptionMediaOwnerRefV1 {
            key: perception_media_storage_key(&episode.key().expect("episode key")),
            sha256: format!("sha256:{:x}", Sha256::digest(b"image")),
            mime: "image/png".into(),
            byte_len: 5,
        });
        episode
    }

    #[test]
    fn optional_media_owner_does_not_change_semantic_episode_source() {
        let episode = media_episode();
        let mut semantic_only = episode.clone();
        semantic_only.media = None;
        let semantic = semantic_only.source_ref().expect("semantic source");
        assert_eq!(episode.source_ref().expect("media source"), semantic);
        let projected = project_perception_semantic_archive_doc(
            PERCEPTION_EPISODE_NAMESPACE,
            &episode.key().expect("episode key"),
            &serde_json::to_value(&episode).expect("episode JSON"),
        )
        .expect("semantic projection")
        .expect("episode is included");
        let projected_episode: PerceptionEpisodeEvidenceV1 =
            serde_json::from_value(projected).expect("projected episode");
        assert!(projected_episode.media.is_none());
        assert_eq!(
            projected_episode.source_ref().expect("projected source"),
            semantic
        );

        let source = episode.source_ref().expect("source");
        let media = episode.media.as_ref().expect("media");
        let root = PerceptionSourceDependentsRootV1::new(
            source.clone(),
            vec![PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::MediaEvidence,
                memory_space_id: source.memory_space_id.clone(),
                subject_id: source.subject_id.clone(),
                relationship_id: source.relationship_id.clone(),
                owner_key: media.key.clone(),
                owner_revision: None,
                content_digest: media.sha256.clone(),
            }],
        )
        .expect("media root");
        let projected_root = project_perception_semantic_archive_doc(
            PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE,
            &episode.key().expect("episode key"),
            &serde_json::to_value(&root).expect("root JSON"),
        )
        .expect("root projection")
        .expect("root is included");
        let projected_root: PerceptionSourceDependentsRootV1 =
            serde_json::from_value(projected_root).expect("projected root");
        projected_root.validate_contract().expect("canonical root");
        assert!(projected_root.edges.is_empty());
        assert_eq!(projected_root.source, semantic);
    }

    #[test]
    fn snapshot_import_requires_closed_visual_source_graph() {
        let valid = closed_snapshot_image();
        validate_perception_store_image(&valid).expect("closed visual snapshot");

        let mut historical_only = valid.clone();
        historical_only
            .json
            .retain(|(namespace, _), _| namespace != PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE);
        validate_perception_store_image(&historical_only)
            .expect("historical evidence remains valid without live capture authority");

        let mut missing_historical = valid.clone();
        missing_historical
            .json
            .retain(|(namespace, _), _| namespace != PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE);
        assert!(validate_perception_store_image(&missing_historical).is_err());

        let mut missing_root = valid.clone();
        missing_root
            .json
            .retain(|(namespace, _), _| namespace != PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE);
        assert!(validate_perception_store_image(&missing_root).is_err());

        let mut wrong_count = valid.clone();
        for ((namespace, _), value) in &mut wrong_count.json {
            if namespace == PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE {
                value["active_count"] = serde_json::json!(0);
            }
        }
        assert!(validate_perception_store_image(&wrong_count).is_err());

        let mut banned = valid;
        let episode = episode();
        let ban = PerceptionSourceRetentionBanV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: episode.memory_space_id.clone(),
            subject_id: episode.subject_id.clone(),
            relationship_id: episode.relationship_id.clone(),
            host_id: episode.source.host_id.clone(),
            device_id: episode.source.device_id.clone(),
            source_id: episode.source.source_id.clone(),
            policy_revision: 1,
            retention_ceiling: PerceptionRetentionCeiling::EphemeralSession,
            updated_at_unix_secs: 1,
        };
        banned.json.insert(
            (
                PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                ban.key().expect("ban key"),
            ),
            serde_json::to_value(ban).expect("ban JSON"),
        );
        assert!(validate_perception_store_image(&banned).is_err());
    }

    #[test]
    fn media_archive_requires_exact_blob_and_semantic_projection_removes_it() {
        let episode = media_episode();
        let media = episode.media.as_ref().expect("media owner");
        let key = episode.key().expect("episode key");
        let source = episode.source_ref().expect("source");
        let root = PerceptionSourceDependentsRootV1::new(
            source.clone(),
            vec![PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::MediaEvidence,
                memory_space_id: source.memory_space_id.clone(),
                subject_id: source.subject_id.clone(),
                relationship_id: source.relationship_id.clone(),
                owner_key: media.key.clone(),
                owner_revision: None,
                content_digest: media.sha256.clone(),
            }],
        )
        .expect("media root");
        let mut image = closed_snapshot_image();
        let identity = perception_source_fence_key(
            &episode.memory_space_id,
            &episode.subject_id,
            episode.relationship_id.as_deref(),
            &episode.source,
        )
        .expect("identity");
        let mut active: PerceptionSourceAuthorizationV1 = serde_json::from_value(
            image.json[&(
                PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                identity.clone(),
            )]
                .clone(),
        )
        .expect("active auth");
        active.retention_ceiling = PerceptionRetentionCeiling::DurableMedia;
        let historical = project_perception_historical_authorization(&episode, &active)
            .expect("historical media auth");
        image.json.insert(
            (PERCEPTION_EPISODE_NAMESPACE.into(), key.clone()),
            serde_json::to_value(&episode).expect("episode JSON"),
        );
        image.json.insert(
            (PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), key.clone()),
            serde_json::to_value(root).expect("root JSON"),
        );
        image.json.insert(
            (PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(), identity),
            serde_json::to_value(active).expect("active JSON"),
        );
        image.json.insert(
            (
                PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                key.clone(),
            ),
            serde_json::to_value(historical).expect("historical JSON"),
        );
        image.blobs.insert(
            (PERCEPTION_MEDIA_NAMESPACE.into(), media.key.clone()),
            b"image".to_vec(),
        );
        validate_perception_store_image(&image).expect("exact media image");

        let mut missing = image.clone();
        missing.blobs.clear();
        assert!(validate_perception_store_image(&missing).is_err());
        let mut wrong_digest = image.clone();
        wrong_digest.blobs.insert(
            (PERCEPTION_MEDIA_NAMESPACE.into(), media.key.clone()),
            b"other".to_vec(),
        );
        assert!(validate_perception_store_image(&wrong_digest).is_err());

        let mut semantic = image;
        for ((namespace, document_key), value) in &mut semantic.json {
            if document_key != &key {
                continue;
            }
            if let Some(projected) =
                project_perception_semantic_archive_doc(namespace, document_key, value)
                    .expect("semantic projection")
            {
                *value = projected;
            }
        }
        semantic.blobs.clear();
        validate_perception_store_image(&semantic)
            .expect("ExcludePrivate semantic image retains exact source without media");

        let mut downgraded = semantic;
        let historical_value = downgraded
            .json
            .get_mut(&(PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(), key))
            .expect("historical owner");
        historical_value["retention_ceiling"] =
            serde_json::to_value(PerceptionRetentionCeiling::EphemeralSession)
                .expect("ceiling JSON");
        assert!(validate_perception_store_image(&downgraded).is_err());
    }

    #[test]
    fn archive_replay_cannot_override_local_tombstone_ban_or_stricter_authority() {
        let episode = episode();
        let key = episode.key().expect("episode key");
        let identity = perception_source_fence_key(
            &episode.memory_space_id,
            &episode.subject_id,
            episode.relationship_id.as_deref(),
            &episode.source,
        )
        .expect("identity");
        let mut image = closed_snapshot_image();
        image
            .json
            .retain(|(namespace, _), _| namespace != PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE);
        let request = crate::StoreScopedProjectionReplaceRequest {
            scope: crate::StoreScopedProjectionScope::subject("space", "soul")
                .expect("subject scope"),
            json_namespaces:
                crate::store_internal::schema::store_memory_space_archive_json_namespaces()
                    .map(str::to_string)
                    .collect(),
            json_docs: image
                .json
                .into_iter()
                .map(|((namespace, key), value)| crate::StoreSnapshotJsonDoc {
                    namespace,
                    key,
                    value,
                })
                .collect(),
            events: Vec::new(),
            preserve_protected_owner_state: true,
            source_closure: None,
        };
        let validate = |local: &BTreeMap<(String, String), serde_json::Value>| {
            crate::store_internal::transaction::validate_visual_archive_import_against_live(
                &request,
                |namespace, key| Ok(local.get(&(namespace.into(), key.into())).cloned()),
            )
        };
        validate(&BTreeMap::new()).expect("historical archive without local revocation");

        let tombstone = PerceptionEpisodeTombstoneV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: episode.memory_space_id.clone(),
            subject_id: episode.subject_id.clone(),
            source: episode.source.clone(),
            revoke_epoch: episode.revoke_epoch,
            episode_id: episode.episode_id.clone(),
            withdrawn_content_digest: episode.content_digest().expect("source digest"),
            deleted_at_unix_secs: 3,
        };
        let mut tombstoned = BTreeMap::new();
        tombstoned.insert(
            (PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(), key),
            serde_json::to_value(tombstone).expect("tombstone JSON"),
        );
        assert!(validate(&tombstoned).is_err());

        let ban = PerceptionSourceRetentionBanV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: episode.memory_space_id.clone(),
            subject_id: episode.subject_id.clone(),
            relationship_id: episode.relationship_id.clone(),
            host_id: episode.source.host_id.clone(),
            device_id: episode.source.device_id.clone(),
            source_id: episode.source.source_id.clone(),
            policy_revision: 1,
            retention_ceiling: PerceptionRetentionCeiling::EphemeralSession,
            updated_at_unix_secs: 1,
        };
        let mut banned = BTreeMap::new();
        banned.insert(
            (
                PERCEPTION_SOURCE_RETENTION_BAN_NAMESPACE.into(),
                identity.clone(),
            ),
            serde_json::to_value(ban).expect("ban JSON"),
        );
        assert!(validate(&banned).is_err());

        let authority = PerceptionSourceAuthorizationV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: episode.memory_space_id.clone(),
            subject_id: episode.subject_id.clone(),
            relationship_id: episode.relationship_id.clone(),
            source: episode.source.clone(),
            source_kind: episode.source_kind,
            revoke_epoch: episode.revoke_epoch,
            authorization_generation: episode.authorization_generation,
            retention_ceiling: PerceptionRetentionCeiling::EphemeralSession,
            user_consent_attested: true,
            policy_manifest_revision: 0,
            policy_manifest_digest: None,
            updated_at_unix_secs: 1,
        };
        let mut downgraded = BTreeMap::new();
        downgraded.insert(
            (PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(), identity),
            serde_json::to_value(authority).expect("authority JSON"),
        );
        assert!(validate(&downgraded).is_err());
    }

    #[test]
    fn empty_subject_archive_preserves_local_visual_authority_and_taint() {
        let episode = episode();
        let episode_key = episode.key().expect("episode key");
        let identity = perception_source_fence_key(
            &episode.memory_space_id,
            &episode.subject_id,
            episode.relationship_id.as_deref(),
            &episode.source,
        )
        .expect("source identity");
        let mut local = closed_snapshot_image().json;
        let authority_key = (PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(), identity);
        let authority = local.get_mut(&authority_key).expect("local authority");
        authority["retention_ceiling"] =
            serde_json::to_value(PerceptionRetentionCeiling::EphemeralSession)
                .expect("ceiling JSON");
        let taint = PerceptionTurnTaintV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: episode.memory_space_id.clone(),
            subject_id: episode.subject_id.clone(),
            session_id: "session".into(),
            turn_id: "turn".into(),
            source_commitments: vec![digest('d')],
            retention_ceiling: PerceptionRetentionCeiling::CurrentTurn,
            first_seen_at_unix_secs: 1,
            updated_at_unix_secs: 1,
        };
        taint.validate_contract().expect("taint contract");
        let taint_key = (
            PERCEPTION_TURN_TAINT_NAMESPACE.into(),
            taint.key().expect("taint key"),
        );
        local.insert(
            taint_key.clone(),
            serde_json::to_value(&taint).expect("taint JSON"),
        );
        let retained_authority = local[&authority_key].clone();
        let retained_taint = local[&taint_key].clone();

        // An empty subject archive replaces memory owners, not the destination's
        // current source authorization or crash-safe visual turn marker.
        let namespaces =
            crate::store_internal::schema::store_memory_space_archive_json_namespaces()
                .filter(|namespace| *namespace != PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE)
                .map(str::to_string)
                .collect::<Vec<_>>();
        assert!(!namespaces.contains(&PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.to_string()));
        assert!(namespaces.contains(&PERCEPTION_TURN_TAINT_NAMESPACE.to_string()));
        let scope =
            crate::StoreScopedProjectionScope::subject("space", "soul").expect("subject scope");
        let empty_archive = crate::StoreScopedProjectionReplaceRequest {
            scope: scope.clone(),
            json_namespaces: namespaces.clone(),
            json_docs: Vec::new(),
            events: Vec::new(),
            preserve_protected_owner_state: true,
            source_closure: None,
        };
        let candidate_addresses =
            crate::store_internal::transaction::scoped_projection_json_addresses(
                &namespaces,
                &local,
                &scope,
                crate::StoreCapacityBudget::full(),
            )
            .expect("empty archive replacement addresses");
        assert!(!candidate_addresses.contains(&authority_key));
        assert!(candidate_addresses.contains(&taint_key));
        let removed = candidate_addresses
            .into_iter()
            .filter(|address| {
                !crate::store_internal::engine::json_document_is_preserved_by_scoped_projection(
                    &address.0,
                    &local[address],
                    &empty_archive,
                )
                .expect("scoped preservation gate")
            })
            .collect::<BTreeSet<_>>();
        assert!(!removed.contains(&taint_key));
        local.retain(|address, _| !removed.contains(address));
        assert_eq!(local.get(&authority_key), Some(&retained_authority));
        assert_eq!(local.get(&taint_key), Some(&retained_taint));

        let mut archived = closed_snapshot_image().json;
        archived.remove(&authority_key);
        assert!(archived.contains_key(&(
            PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
            episode_key
        )));
        let old_archive = crate::StoreScopedProjectionReplaceRequest {
            scope,
            json_namespaces: namespaces,
            json_docs: archived
                .into_iter()
                .map(|((namespace, key), value)| crate::StoreSnapshotJsonDoc {
                    namespace,
                    key,
                    value,
                })
                .collect(),
            events: Vec::new(),
            preserve_protected_owner_state: true,
            source_closure: None,
        };
        assert!(!old_archive
            .json_docs
            .iter()
            .any(|document| { document.namespace == PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE }));
        assert!(
            crate::store_internal::transaction::validate_visual_archive_import_against_live(
                &old_archive,
                |namespace, key| Ok(local.get(&(namespace.into(), key.into())).cloned()),
            )
            .is_err()
        );
    }

    #[test]
    fn archive_root_rebuild_removes_redacted_transcript_edge() {
        let mut state = closed_snapshot_image();
        let episode = episode();
        let key = episode.key().expect("episode key");
        let source = episode.source_ref().expect("source");
        let old = PerceptionSourceDependentsRootV1::new(
            source.clone(),
            vec![PerceptionSourceDependentEdgeV1 {
                kind: PerceptionSourceDependentKindV1::TranscriptTurn,
                memory_space_id: source.memory_space_id.clone(),
                subject_id: source.subject_id.clone(),
                relationship_id: source.relationship_id.clone(),
                owner_key: "redacted-transcript".into(),
                owner_revision: Some(1),
                content_digest: digest('a'),
            }],
        )
        .expect("original root");
        state.json.insert(
            (PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), key.clone()),
            serde_json::to_value(old).expect("root JSON"),
        );
        let mut snapshot = crate::StoreSnapshot::new(
            crate::StoreSchemaManifest::new(
                crate::StoreBackendKind::InMemory,
                crate::ProfileId::DesktopMacosEmbeddedSdk,
                1,
            ),
            state
                .json
                .into_iter()
                .map(|((namespace, key), value)| crate::StoreSnapshotJsonDoc {
                    namespace,
                    key,
                    value,
                })
                .collect(),
            Vec::new(),
            Vec::new(),
        );
        rebuild_perception_archive_roots(&mut snapshot).expect("rebuild disclosed root");
        let rebuilt = snapshot
            .json_docs
            .iter()
            .find(|doc| doc.namespace == PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE && doc.key == key)
            .expect("rebuilt root");
        let rebuilt: PerceptionSourceDependentsRootV1 =
            serde_json::from_value(rebuilt.value.clone()).expect("typed root");
        assert!(rebuilt.edges.is_empty());
        assert_eq!(rebuilt.source, source);
        let final_image = BackendTransactionState {
            json: snapshot
                .json_docs
                .into_iter()
                .map(|doc| ((doc.namespace, doc.key), doc.value))
                .collect(),
            ..BackendTransactionState::default()
        };
        validate_perception_store_image(&final_image).expect("rebuilt archive is closed");
    }

    #[test]
    fn soul_reverse_edges_follow_live_revision_and_durable_denial() {
        use crate::store_internal::schema::{
            subject_soul_revision_material_key, subject_soul_scope_key,
            SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE, SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE,
            SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
        };
        use bm_core::memory::{
            SelfAuthoredCore, SubjectSoulDeniedRevisionV1, SubjectSoulLifecycleHeadV1,
            SubjectSoulLifecycleStateV1, SubjectSoulManifestOwnerRoleV1,
            SubjectSoulRevisionMaterialV1, SubjectSoulRevisionOriginV1,
            SubjectSoulRevisionProvenanceV1, SubjectSoulScopeManifestEntryV1,
            SubjectSoulScopeManifestV1, SubjectSoulSourceAuthorityV1, SubjectSoulSourceRefV1,
            SubjectSoulSourceReviewStateV1,
        };
        let source = episode().source_ref().expect("visual source");
        let source_lineage = SubjectSoulSourceRefV1::Perception(source.clone());
        let mut material = SubjectSoulRevisionMaterialV1 {
            schema_version: bm_core::memory::SUBJECT_SOUL_SCHEMA_VERSION,
            memory_space_id: "space".into(),
            subject_id: "soul".into(),
            soul_id: "soul-id".into(),
            generation: 1,
            revision: 1,
            supersedes_revision: None,
            core: SelfAuthoredCore {
                revision: 1,
                identity_anchor: "我记得我见过。".into(),
                ..SelfAuthoredCore::default()
            },
            provenance: SubjectSoulRevisionProvenanceV1 {
                origin: SubjectSoulRevisionOriginV1::SelfAuthoredBootstrap,
                source_authority: SubjectSoulSourceAuthorityV1::SoulSelfGovernance,
                source_subject_id: "soul".into(),
                source_asserted_at: Some(1),
                recorded_at: 1,
                operation_ref: None,
                proposal_ref: Some("proposal".into()),
                source_refs: Vec::new(),
            },
            source_lineage: vec![source_lineage.clone()],
            source_review_basis: None,
            content_digest: String::new(),
        };
        material.refresh_digest().expect("material digest");
        material.validate_contract().expect("valid material");
        let scope_key = subject_soul_scope_key("space", "soul", "soul-id").expect("scope key");
        let material_key = subject_soul_revision_material_key("space", "soul", "soul-id", 1, 1)
            .expect("material key");
        let entry = SubjectSoulScopeManifestEntryV1 {
            namespace: SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE.into(),
            physical_key: material_key.clone(),
            owner_role: SubjectSoulManifestOwnerRoleV1::GenerationDerived,
            generation: 1,
            revision: Some(1),
            content_digest: material.content_digest.clone(),
            source_lineage: vec![source_lineage.clone()],
        };
        let mut manifest = SubjectSoulScopeManifestV1 {
            source_review: SubjectSoulSourceReviewStateV1::empty(),
            schema_version: bm_core::memory::SUBJECT_SOUL_SCHEMA_VERSION,
            memory_space_id: "space".into(),
            subject_id: "soul".into(),
            soul_id: "soul-id".into(),
            generation: 1,
            manifest_revision: 1,
            entries: vec![entry],
            closure_digest: String::new(),
        };
        manifest.refresh_digest().expect("manifest digest");
        let mut head = SubjectSoulLifecycleHeadV1 {
            schema_version: bm_core::memory::SUBJECT_SOUL_SCHEMA_VERSION,
            memory_space_id: "space".into(),
            subject_id: "soul".into(),
            soul_id: "soul-id".into(),
            generation: 1,
            state: SubjectSoulLifecycleStateV1::Active,
            current_revision: Some(1),
            current_material_digest: Some(material.content_digest.clone()),
            current_ledger_digest: Some("a".repeat(64)),
            scope_manifest_digest: manifest.closure_digest.clone(),
            retained_revision_refs: Vec::new(),
            retained_tombstone_refs: Vec::new(),
            updated_at: 1,
            head_digest: String::new(),
        };
        head.refresh_digest().expect("head digest");
        let mut json = BTreeMap::from([
            (
                (
                    SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.into(),
                    scope_key.clone(),
                ),
                serde_json::to_value(head.clone()).expect("head JSON"),
            ),
            (
                (
                    SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.into(),
                    scope_key.clone(),
                ),
                serde_json::to_value(&manifest).expect("manifest JSON"),
            ),
            (
                (
                    SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE.into(),
                    material_key,
                ),
                serde_json::to_value(&material).expect("material JSON"),
            ),
        ]);
        let live = live_soul_edges_for_source(&json, &source).expect("live edges");
        assert_eq!(live.len(), 2);
        assert!(live
            .iter()
            .any(|edge| edge.kind == PerceptionSourceDependentKindV1::SoulRoot));
        assert!(live
            .iter()
            .any(|edge| edge.kind == PerceptionSourceDependentKindV1::SoulRevision));

        manifest.source_review = SubjectSoulSourceReviewStateV1 {
            epoch: 1,
            pending: None,
            withdrawn_sources: vec![source_lineage],
            denied_revisions: vec![SubjectSoulDeniedRevisionV1 {
                generation: 1,
                revision: 1,
                material_digest: material.content_digest.clone(),
            }],
        };
        manifest.manifest_revision = 2;
        manifest.refresh_digest().expect("denial manifest digest");
        head.scope_manifest_digest = manifest.closure_digest.clone();
        head.refresh_digest().expect("denial head digest");
        json.insert(
            (
                SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.into(),
                scope_key.clone(),
            ),
            serde_json::to_value(head).expect("denial head JSON"),
        );
        json.insert(
            (SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.into(), scope_key),
            serde_json::to_value(&manifest).expect("denial manifest JSON"),
        );
        assert!(live_soul_edges_for_source(&json, &source)
            .expect("denied edges")
            .is_empty());
        assert!(manifest
            .source_review
            .validate_material_read(&material)
            .is_err());
    }

    fn deletion_images(
        dependent_kind: PerceptionSourceDependentKindV1,
    ) -> (
        StoreMutationBatch,
        BackendTransactionState,
        BackendTransactionState,
    ) {
        let episode = episode();
        episode.validate_contract().expect("valid episode");
        let source = episode.source_ref().expect("source ref");
        let key = episode.key().expect("episode key");
        let root = PerceptionSourceDependentsRootV1::new(
            source.clone(),
            vec![PerceptionSourceDependentEdgeV1 {
                kind: dependent_kind,
                memory_space_id: source.memory_space_id.clone(),
                subject_id: source.subject_id.clone(),
                relationship_id: None,
                owner_key: "derived:owner:1".into(),
                owner_revision: Some(1),
                content_digest: digest('d'),
            }],
        )
        .expect("valid dependent root");
        let tombstone = PerceptionEpisodeTombstoneV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: source.memory_space_id.clone(),
            subject_id: source.subject_id.clone(),
            source: source.source.clone(),
            revoke_epoch: source.revoke_epoch,
            episode_id: source.episode_id.clone(),
            withdrawn_content_digest: source.content_digest.clone(),
            deleted_at_unix_secs: 3,
        };
        let mut before = BackendTransactionState::default();
        let authorization = PerceptionSourceAuthorizationV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: episode.memory_space_id.clone(),
            subject_id: episode.subject_id.clone(),
            relationship_id: episode.relationship_id.clone(),
            source: episode.source.clone(),
            source_kind: episode.source_kind,
            revoke_epoch: episode.revoke_epoch,
            authorization_generation: episode.authorization_generation,
            retention_ceiling: episode.retention_ceiling,
            user_consent_attested: true,
            policy_manifest_revision: 0,
            policy_manifest_digest: None,
            updated_at_unix_secs: 1,
        };
        let historical = project_perception_historical_authorization(&episode, &authorization)
            .expect("historical authorization");
        before.json.insert(
            (
                PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                key.clone(),
            ),
            serde_json::to_value(historical).expect("historical JSON"),
        );
        let count_before = PerceptionSourceEpisodeCountV1::from_episode(&episode, 1, 1);
        let count_after = PerceptionSourceEpisodeCountV1::from_episode(&episode, 2, 0);
        let count_key = count_before.key().expect("count key");
        before.json.insert(
            (
                PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
                count_key.clone(),
            ),
            serde_json::to_value(&count_before).expect("count before JSON"),
        );
        before.json.insert(
            (PERCEPTION_EPISODE_NAMESPACE.into(), key.clone()),
            serde_json::to_value(&episode).expect("episode JSON"),
        );
        before.json.insert(
            (PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), key.clone()),
            serde_json::to_value(root).expect("root JSON"),
        );
        let mut after = BackendTransactionState::default();
        after.json.insert(
            (
                PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
                count_key.clone(),
            ),
            serde_json::to_value(&count_after).expect("count after JSON"),
        );
        let fence = PerceptionSourceFenceV1::from_episode_source(&source, 3).expect("source fence");
        let fence_key = fence.key().expect("fence key");
        after.json.insert(
            (PERCEPTION_SOURCE_FENCE_NAMESPACE.into(), fence_key.clone()),
            serde_json::to_value(&fence).expect("fence JSON"),
        );
        after.json.insert(
            (PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(), key.clone()),
            serde_json::to_value(&tombstone).expect("tombstone JSON"),
        );
        let batch = StoreMutationBatch {
            transaction_id: "delete-episode".into(),
            operation: "perception_episode_forget".into(),
            scope: StoreEventScope::new("agent", "space", "local", "chat").with_subject("soul"),
            mutations: vec![
                StoreMutation::DeleteJson {
                    namespace: PERCEPTION_EPISODE_NAMESPACE.into(),
                    key: key.clone(),
                    event_kind: MemoryStoreEventKind::MemoryDelete,
                    plane: "perception_evidence".into(),
                    record_key: "episode".into(),
                },
                StoreMutation::PutJson {
                    namespace: PERCEPTION_EPISODE_TOMBSTONE_NAMESPACE.into(),
                    key: key.clone(),
                    value: serde_json::to_value(tombstone).expect("tombstone JSON"),
                    event_kind: MemoryStoreEventKind::MemoryDelete,
                    plane: "perception_evidence".into(),
                    record_key: "episode".into(),
                },
                StoreMutation::DeleteJson {
                    namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
                    key,
                    event_kind: MemoryStoreEventKind::MemoryDelete,
                    plane: "perception_evidence".into(),
                    record_key: "episode".into(),
                },
                StoreMutation::DeleteJson {
                    namespace: PERCEPTION_HISTORICAL_AUTHORIZATION_NAMESPACE.into(),
                    key: episode.key().expect("episode key"),
                    event_kind: MemoryStoreEventKind::MemoryDelete,
                    plane: "perception_evidence".into(),
                    record_key: "episode".into(),
                },
                StoreMutation::PutJson {
                    namespace: PERCEPTION_SOURCE_FENCE_NAMESPACE.into(),
                    key: fence_key.clone(),
                    value: serde_json::to_value(fence).expect("fence JSON"),
                    event_kind: MemoryStoreEventKind::MemoryControl,
                    plane: "perception_source_fence".into(),
                    record_key: fence_key,
                },
                StoreMutation::PutJson {
                    namespace: PERCEPTION_SOURCE_EPISODE_COUNT_NAMESPACE.into(),
                    key: count_key.clone(),
                    value: serde_json::to_value(count_after).expect("count after JSON"),
                    event_kind: MemoryStoreEventKind::MemoryControl,
                    plane: "perception_episode_count".into(),
                    record_key: count_key,
                },
            ],
        };
        (batch, before, after)
    }

    #[test]
    fn composed_episode_delete_accepts_derived_owner_removed_in_final_image() {
        let (batch, before, after) =
            deletion_images(PerceptionSourceDependentKindV1::LongTermOwnerRevision);
        validate_perception_transaction_post_image(&batch, &before, &after)
            .expect("the final image has no derived owner");
    }

    #[test]
    fn episode_delete_rejects_unhandled_dependent_kind() {
        let (batch, before, after) =
            deletion_images(PerceptionSourceDependentKindV1::DerivedSummary);
        assert!(validate_perception_transaction_post_image(&batch, &before, &after).is_err());
    }

    #[test]
    fn episode_deletion_plan_is_read_only_until_composed_commit() {
        let platform = StorePlatform::open(
            crate::StoreBackendConfig::in_memory(crate::ProfileId::DesktopMacosEmbeddedSdk)
                .unwrap(),
        )
        .expect("in-memory Store");
        let episode = episode();
        let source = episode.source_ref().expect("source ref");
        let scope = StoreEventScope::new("agent", "space", "local", "chat").with_subject("soul");
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        let perception_scope = perception_scope();
        platform
            .register_perception_source_authorization(
                &perception_scope,
                &episode.source,
                episode.source_kind,
                episode.revoke_epoch,
                episode.authorization_generation,
                episode.retention_ceiling,
                true,
                scope.clone(),
                "authorize-source".into(),
                now_secs,
            )
            .expect("authorize source");
        platform
            .persist_perception_episode(
                episode.clone(),
                None,
                scope,
                "episode-insert".into(),
                now_secs,
            )
            .expect("persist episode");
        let plan = platform
            .plan_perception_episode_deletion(&source, now_secs + 1)
            .expect("plan deletion")
            .expect("active source");
        assert_eq!(plan.mutations.len(), 4);
        assert_eq!(plan.preconditions.len(), 4);
        assert!(plan.blob_preconditions.is_empty());
        assert_eq!(plan.dependents.source, source);
        assert_eq!(
            platform
                .read_json_docs_by_keys(PERCEPTION_EPISODE_NAMESPACE, &[episode.key().unwrap()])
                .expect("read episode")
                .len(),
            1,
            "planning must leave the source untouched",
        );
    }

    #[test]
    fn broad_pause_blocks_durable_visual_media() {
        let state = broad_pause_policy_state();
        assert!(
            broad_visual_retention_blocked_by_policy(&state, "space", "soul", 2)
                .expect("evaluate broad policy")
        );
        let episode = episode();
        let auth_key = perception_source_fence_key("space", "soul", None, &episode.source)
            .expect("authorization key");
        let (revision, digest) =
            policy_snapshot_from_state(&state, "space").expect("policy snapshot");
        let auth = PerceptionSourceAuthorizationV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: "space".into(),
            subject_id: "soul".into(),
            relationship_id: None,
            source: episode.source,
            source_kind: episode.source_kind,
            revoke_epoch: episode.revoke_epoch,
            authorization_generation: 1,
            retention_ceiling: PerceptionRetentionCeiling::DurableMedia,
            user_consent_attested: true,
            policy_manifest_revision: revision,
            policy_manifest_digest: digest,
            updated_at_unix_secs: 2,
        };
        let mut after = state;
        after.json.insert(
            (
                PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                auth_key.clone(),
            ),
            serde_json::to_value(&auth).expect("authorization JSON"),
        );
        let batch = StoreMutationBatch {
            transaction_id: "authorize-under-pause".into(),
            operation: "perception_source_authorize".into(),
            scope: StoreEventScope::new("agent", "space", "local", "chat").with_subject("soul"),
            mutations: vec![StoreMutation::PutJson {
                namespace: PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                key: auth_key.clone(),
                value: serde_json::to_value(auth).expect("authorization JSON"),
                event_kind: MemoryStoreEventKind::MemoryControl,
                plane: "perception_source_authorization".into(),
                record_key: auth_key,
            }],
        };
        assert!(validate_perception_transaction_post_image(
            &batch,
            &BackendTransactionState::default(),
            &after,
        )
        .is_err());
    }

    #[test]
    fn restrictive_auth_downgrade_survives_same_batch_policy_change() {
        let episode = episode();
        let key =
            perception_source_fence_key("space", "soul", None, &episode.source).expect("auth key");
        let old = PerceptionSourceAuthorizationV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: "space".into(),
            subject_id: "soul".into(),
            relationship_id: None,
            source: episode.source.clone(),
            source_kind: episode.source_kind,
            revoke_epoch: episode.revoke_epoch,
            authorization_generation: 1,
            retention_ceiling: PerceptionRetentionCeiling::DurableMedia,
            user_consent_attested: true,
            policy_manifest_revision: 0,
            policy_manifest_digest: None,
            updated_at_unix_secs: 1,
        };
        let mut before = BackendTransactionState::default();
        before.json.insert(
            (
                PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                key.clone(),
            ),
            serde_json::to_value(&old).expect("old auth JSON"),
        );
        let mut after = broad_pause_policy_state();
        let mut next = old;
        next.authorization_generation = 2;
        next.retention_ceiling = PerceptionRetentionCeiling::EphemeralSession;
        next.updated_at_unix_secs = 2;
        after.json.insert(
            (
                PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                key.clone(),
            ),
            serde_json::to_value(&next).expect("new auth JSON"),
        );
        let batch = StoreMutationBatch {
            transaction_id: "policy-and-source-withdrawal".into(),
            operation: "perception_source_forbid".into(),
            scope: StoreEventScope::new("agent", "space", "local", "chat").with_subject("soul"),
            mutations: vec![StoreMutation::PutJson {
                namespace: PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(),
                key: key.clone(),
                value: serde_json::to_value(next).expect("new auth JSON"),
                event_kind: MemoryStoreEventKind::MemoryControl,
                plane: "perception_source_authorization".into(),
                record_key: key,
            }],
        };
        validate_perception_transaction_post_image(&batch, &before, &after)
            .expect("restrictive auth retains its historical policy anchor");
    }

    #[test]
    fn policy_revision_changed_before_episode_commit_is_rejected() {
        let episode = episode();
        let key = episode.key().expect("episode key");
        let auth_key = perception_source_fence_key("space", "soul", None, &episode.source)
            .expect("authorization key");
        let auth = PerceptionSourceAuthorizationV1 {
            schema_version: PERCEPTION_EPISODE_SCHEMA_VERSION,
            memory_space_id: "space".into(),
            subject_id: "soul".into(),
            relationship_id: None,
            source: episode.source.clone(),
            source_kind: episode.source_kind,
            revoke_epoch: episode.revoke_epoch,
            authorization_generation: episode.authorization_generation,
            retention_ceiling: PerceptionRetentionCeiling::DurableMedia,
            user_consent_attested: true,
            policy_manifest_revision: 0,
            policy_manifest_digest: None,
            updated_at_unix_secs: 1,
        };
        let mut after = broad_pause_policy_state();
        after.json.insert(
            (PERCEPTION_SOURCE_AUTHORIZATION_NAMESPACE.into(), auth_key),
            serde_json::to_value(auth).expect("authorization JSON"),
        );
        after.json.insert(
            (PERCEPTION_EPISODE_NAMESPACE.into(), key.clone()),
            serde_json::to_value(&episode).expect("episode JSON"),
        );
        let root = PerceptionSourceDependentsRootV1::new(
            episode.source_ref().expect("source ref"),
            Vec::new(),
        )
        .expect("root");
        after.json.insert(
            (PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(), key.clone()),
            serde_json::to_value(&root).expect("root JSON"),
        );
        let batch = StoreMutationBatch {
            transaction_id: "episode-after-policy-change".into(),
            operation: "perception_episode_persist".into(),
            scope: StoreEventScope::new("agent", "space", "local", "chat").with_subject("soul"),
            mutations: vec![
                StoreMutation::PutJson {
                    namespace: PERCEPTION_EPISODE_NAMESPACE.into(),
                    key: key.clone(),
                    value: serde_json::to_value(episode).expect("episode JSON"),
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "perception_evidence".into(),
                    record_key: "episode".into(),
                },
                StoreMutation::PutJson {
                    namespace: PERCEPTION_SOURCE_DEPENDENTS_NAMESPACE.into(),
                    key,
                    value: serde_json::to_value(root).expect("root JSON"),
                    event_kind: MemoryStoreEventKind::MemoryWrite,
                    plane: "perception_evidence".into(),
                    record_key: "episode".into(),
                },
            ],
        };
        assert!(validate_perception_transaction_post_image(
            &batch,
            &BackendTransactionState::default(),
            &after,
        )
        .is_err());
    }

    #[test]
    fn durable_fence_rejects_reopened_old_source_generation() {
        let platform = StorePlatform::open(
            crate::StoreBackendConfig::in_memory(crate::ProfileId::DesktopMacosEmbeddedSdk)
                .expect("config"),
        )
        .expect("Store");
        let episode = episode();
        let scope = perception_scope();
        let event_scope =
            StoreEventScope::new("agent", "space", "local", "chat").with_subject("soul");
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        platform
            .register_perception_source_authorization(
                &scope,
                &episode.source,
                episode.source_kind,
                episode.revoke_epoch,
                1,
                PerceptionRetentionCeiling::DurableMedia,
                true,
                event_scope.clone(),
                "authorize".into(),
                now_secs,
            )
            .expect("authorize");
        platform
            .advance_perception_source_fence(
                &scope,
                &episode.source,
                episode.revoke_epoch,
                event_scope,
                "withdraw".into(),
                now_secs + 1,
            )
            .expect("withdraw");
        assert!(platform
            .assert_perception_source_authorized(
                &scope,
                &episode.source,
                episode.source_kind,
                episode.revoke_epoch,
                1,
                PerceptionRetentionCeiling::DurableSemantic,
            )
            .is_err());
        let fence = platform
            .read_perception_source_fence(&scope, &episode.source)
            .expect("read durable fence")
            .expect("fence remains in Store");
        assert_eq!(fence.revoked_tuple(), (1, 1));
        let mut newer = episode.source.clone();
        newer.source_generation = 2;
        platform
            .assert_perception_source_live(&scope, &newer, 0)
            .expect("strictly newer source generation can be authorized");
    }

    #[test]
    fn deleting_one_episode_preserves_same_generation_sibling() {
        let platform = StorePlatform::open(
            crate::StoreBackendConfig::in_memory(crate::ProfileId::DesktopMacosEmbeddedSdk)
                .expect("config"),
        )
        .expect("Store");
        let first = episode();
        let mut sibling = episode();
        sibling.episode_id = "episode-sibling".into();
        sibling.input_id = "input-sibling".into();
        sibling.delivery.as_mut().expect("delivery").input_id = sibling.input_id.clone();
        let scope = perception_scope();
        let event_scope =
            StoreEventScope::new("agent", "space", "local", "chat").with_subject("soul");
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        platform
            .register_perception_source_authorization(
                &scope,
                &first.source,
                first.source_kind,
                first.revoke_epoch,
                1,
                PerceptionRetentionCeiling::DurableSemantic,
                true,
                event_scope.clone(),
                "authorize-siblings".into(),
                now_secs,
            )
            .expect("authorize");
        let (_, first_source) = platform
            .persist_perception_episode(
                first,
                None,
                event_scope.clone(),
                "persist-first".into(),
                now_secs,
            )
            .expect("persist first");
        let (_, sibling_source) = platform
            .persist_perception_episode(
                sibling,
                None,
                event_scope.clone(),
                "persist-sibling".into(),
                now_secs,
            )
            .expect("persist sibling");
        let deletion = platform
            .plan_perception_episode_deletion(&first_source, now_secs + 1)
            .expect("plan deletion")
            .expect("active episode");
        let (fence_mutations, fence_preconditions) = platform
            .plan_perception_source_fence_for_withdrawal(
                std::slice::from_ref(&first_source),
                now_secs + 1,
            )
            .expect("plan fence");
        let mut mutations = deletion.mutations;
        mutations.extend(fence_mutations);
        let mut preconditions = deletion.preconditions;
        preconditions.extend(fence_preconditions);
        let (count_mutations, count_preconditions) = platform
            .plan_perception_source_episode_count_delta(&mutations)
            .expect("plan count");
        mutations.extend(count_mutations);
        preconditions.extend(count_preconditions);
        platform
            .commit_governed_memory_transaction_with_blob_preconditions_and_runtime_budget_at(
                StoreMutationBatch {
                    transaction_id: "delete-first".into(),
                    operation: "perception_episode_forget".into(),
                    scope: event_scope,
                    mutations,
                },
                &preconditions,
                &deletion.blob_preconditions,
                &platform.current_runtime_budget(now_secs + 1),
                now_secs + 1,
            )
            .expect("delete first");
        assert!(platform
            .assert_perception_episode_replay_allowed(&first_source)
            .is_err());
        platform
            .assert_perception_episode_replay_allowed(&sibling_source)
            .expect("same-generation sibling remains readable");
        platform
            .advance_perception_source_fence(
                &scope,
                &sibling_source.source,
                sibling_source.revoke_epoch,
                StoreEventScope::new("agent", "space", "local", "chat").with_subject("soul"),
                "disable-camera".into(),
                now_secs + 2,
            )
            .expect("disable camera");
        platform
            .assert_perception_episode_replay_allowed(&sibling_source)
            .expect("camera permission withdrawal preserves lawful historical memory");
        assert!(
            platform
                .assert_perception_source_live(
                    &scope,
                    &sibling_source.source,
                    sibling_source.revoke_epoch
                )
                .is_err(),
            "old tuple cannot ingest a new episode after deletion"
        );
    }

    #[test]
    fn source_identity_ban_survives_new_camera_generation() {
        let platform = StorePlatform::open(
            crate::StoreBackendConfig::in_memory(crate::ProfileId::DesktopMacosEmbeddedSdk)
                .expect("config"),
        )
        .expect("Store");
        let episode = episode();
        let source = episode.source_ref().expect("source");
        let scope = perception_scope();
        let event_scope =
            StoreEventScope::new("agent", "space", "local", "chat").with_subject("soul");
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        platform
            .register_perception_source_authorization(
                &scope,
                &episode.source,
                episode.source_kind,
                episode.revoke_epoch,
                1,
                PerceptionRetentionCeiling::DurableSemantic,
                true,
                event_scope.clone(),
                "ban-authorize".into(),
                now_secs,
            )
            .expect("authorize");
        platform
            .persist_perception_episode(
                episode,
                None,
                event_scope.clone(),
                "ban-persist".into(),
                now_secs,
            )
            .expect("persist");
        let deletion = platform
            .plan_perception_episode_deletion(&source, now_secs + 1)
            .expect("plan deletion")
            .expect("active source");
        let mut mutations = deletion.mutations;
        let mut preconditions = deletion.preconditions;
        let (fence_mutations, fence_preconditions) = platform
            .plan_perception_source_fence_for_withdrawal(
                std::slice::from_ref(&source),
                now_secs + 1,
            )
            .expect("plan fence");
        mutations.extend(fence_mutations);
        preconditions.extend(fence_preconditions);
        let (count_mutations, count_preconditions) = platform
            .plan_perception_source_episode_count_delta(&mutations)
            .expect("plan count");
        mutations.extend(count_mutations);
        preconditions.extend(count_preconditions);
        let (ban_mutations, ban_preconditions) = platform
            .plan_perception_source_retention_ban_for_withdrawal(
                std::slice::from_ref(&source),
                PerceptionRetentionCeiling::EphemeralSession,
                now_secs + 1,
            )
            .expect("plan ban");
        mutations.extend(ban_mutations);
        preconditions.extend(ban_preconditions);
        let (auth_mutations, auth_preconditions) = platform
            .plan_perception_source_authorization_downgrade(
                &source,
                PerceptionRetentionCeiling::EphemeralSession,
                now_secs + 1,
            )
            .expect("plan downgrade");
        mutations.extend(auth_mutations);
        preconditions.extend(auth_preconditions);
        platform
            .commit_governed_memory_transaction_with_blob_preconditions_and_runtime_budget_at(
                StoreMutationBatch {
                    transaction_id: "ban-source".into(),
                    operation: "visual_source_forbid".into(),
                    scope: event_scope.clone(),
                    mutations,
                },
                &preconditions,
                &deletion.blob_preconditions,
                &platform.current_runtime_budget(now_secs + 1),
                now_secs + 1,
            )
            .expect("atomic ban and withdrawal");
        let mut next_generation = source.source.clone();
        next_generation.source_generation += 1;
        assert!(platform
            .register_perception_source_authorization(
                &scope,
                &next_generation,
                PerceptionSourceKind::HostVisualObservation,
                0,
                3,
                PerceptionRetentionCeiling::DurableMedia,
                true,
                event_scope,
                "ban-bypass-attempt".into(),
                now_secs + 2,
            )
            .is_err());
        assert_eq!(
            platform
                .read_perception_source_retention_ban(&scope, &next_generation)
                .expect("read ban")
                .expect("ban persists")
                .retention_ceiling,
            PerceptionRetentionCeiling::EphemeralSession,
        );
    }

    #[test]
    fn replayed_transcript_owner_requires_live_affected_message() {
        use bm_core::memory::*;
        let delta = CanonicalTurnDelta {
            turn_id: "turn-a".into(),
            conversation: ConversationScope {
                channel: "desktop".into(),
                chat_id: "chat".into(),
                conversation_id: Some("conversation".into()),
            },
            subject: "soul".into(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: IngressKind::User,
                channel: "desktop".into(),
                provider: None,
                protocol: MemoryTurnProtocol::Native,
                endpoint: None,
                model_alias: None,
                model_resolved: None,
                request_id: None,
                client_conversation_hint: None,
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user("看我")],
            assistant_messages: vec![TranscriptAssistantMessage::complete(
                TranscriptInputMessage::assistant("我看见你了"),
            )],
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: Vec::new(),
        };
        let key = ConversationKey::from_delta("space", &delta).expect("conversation key");
        let base = TranscriptTurnRecord::from_delta(&key, 1, &delta, Vec::new(), 10)
            .expect("Transcript A");
        let source = episode().source_ref().expect("source ref");
        let assistant = base.assistant_messages[0].message.clone();
        let first_affected_assistant_sequence = assistant.sequence;
        let record = base
            .with_perception_source_uses(vec![TranscriptPerceptionSourceUseV1 {
                source: source.clone(),
                first_affected_assistant_sequence,
                delivery: PerceptionReplyEvidenceV1::HostAttested {
                    receipt_digest: digest('c'),
                },
            }])
            .expect("visual Transcript A");
        let reference = TranscriptEvidenceRef::from_message(&key, &record, &assistant);
        let owner = PerceptionReplayOwnerV1::TranscriptMessage { reference };
        let address = replay_owner_addresses(&owner, &source)
            .expect("owner address")
            .remove(0);
        let mut state = BackendTransactionState::default();
        state.json.insert(
            address.clone(),
            serde_json::to_value(&record).expect("record JSON"),
        );
        assert!(replay_owner_available_in_state(
            &state,
            &owner,
            &source,
            PerceptionSourceKind::HostVisualObservation,
            None,
        )
        .expect("valid replay owner"));
        assert!(!replay_owner_available_in_state(
            &state,
            &owner,
            &source,
            PerceptionSourceKind::HostVisualObservation,
            Some("relationship:other"),
        )
        .expect("cross-relationship Transcript replay"));
        let mut redacted = record;
        redacted.apply_lifecycle_transition(TranscriptLifecycleTransition::DeleteRaw, 11);
        state.json.insert(
            address,
            serde_json::to_value(redacted).expect("redacted JSON"),
        );
        assert!(!replay_owner_available_in_state(
            &state,
            &owner,
            &source,
            PerceptionSourceKind::HostVisualObservation,
            None,
        )
        .expect("redacted replay owner"));
    }

    #[test]
    fn cross_relationship_replay_requires_user_scope_long_term_owner() {
        use bm_core::memory::LongTermMemorySourceScope;

        let mut source = episode().source_ref().expect("source ref");
        source.relationship_id = Some("relationship:a".into());
        assert!(replay_relationship_allowed(
            &source,
            Some("relationship:a"),
            None,
        ));
        assert!(!replay_relationship_allowed(
            &source,
            Some("relationship:b"),
            None,
        ));
        assert!(!replay_relationship_allowed(
            &source,
            Some("relationship:b"),
            Some(LongTermMemorySourceScope::Chat),
        ));
        assert!(!replay_relationship_allowed(
            &source,
            Some("relationship:b"),
            Some(LongTermMemorySourceScope::World),
        ));
        assert!(replay_relationship_allowed(
            &source,
            Some("relationship:b"),
            Some(LongTermMemorySourceScope::User),
        ));
    }
}
