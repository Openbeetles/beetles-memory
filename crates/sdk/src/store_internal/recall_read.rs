use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use bm_core::agent::{ActiveWorkRecord, ActiveWorkStore};
use bm_core::memory::{
    build_long_term_current_recall_authority, build_long_term_historical_recall_authority,
    long_term_version_head_key, long_term_version_material_key,
    long_term_version_scope_manifest_key, scoped_governed_evidence_document_key,
    scoped_long_term_control_storage_key, select_long_term_version_as_of,
    select_long_term_version_current, validate_governed_evidence_document, ContinuityCapsule,
    ContinuityCapsuleDraft, ContinuityCapsuleScopeKind, ContinuityCapsuleStore,
    ContinuityCapsuleWriteOutcome, ConversationKey, ConversationTranscriptStore, DerivedMemoryRef,
    GovernedEvidenceDocument, GovernedMemoryOwnerRef, GovernedOwnerRevisionRef,
    LongTermCurrentRecallAuthority, LongTermHistoricalRecallAuthority,
    LongTermMemoryControlRevision, LongTermMemoryEntry, LongTermMemoryHeadManifest,
    LongTermMemoryQuery, LongTermMemoryReadStore, LongTermMemoryVersionHeadBinding,
    LongTermMemoryVersionMaterial, LongTermMemoryVersionReadProjection,
    LongTermMemoryVersionScopeManifest, MemoryStore, PremiseTypedSource, RedactedTranscriptSlice,
    SessionMessage, SessionMessageRecord, SessionStore, SessionSummaryStore,
    TranscriptAttrEnvelope, TranscriptAttrWriteReport, TranscriptCommitReport,
    TranscriptConversationAlias, TranscriptLifecycleReport, TranscriptLifecycleRequest,
    TranscriptReplayView, TranscriptTurnRecord, TurnLedger, TurnLedgerStore,
    LONG_TERM_CONTROL_REVISION_NAMESPACE, LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
};
use bm_core::platform::SkillStorage;
use bm_core::skills::{
    runtime_skill_scope_manifest_key, RuntimeSkillOwnerBinding, RuntimeSkillOwnerRecord,
    RuntimeSkillOwningScope, RuntimeSkillPremise, RuntimeSkillScopeManifest,
};
use bm_core::task_execution::{TaskLearningRecord, TaskLearningStore, TaskRunRecord, TaskRunStore};
use bm_core::{Error, Result};
use serde::{de::DeserializeOwned, Deserialize};
use sha2::{Digest, Sha256};

use super::recall_index::{decode_typed_recall_index, TypedRecallIndex};
use super::transaction::StoreImmutableReadSession;
use crate::StoreReadReceipt;

type RecallJsonRead = ((String, String), Option<serde_json::Value>);

#[derive(Clone, Debug)]
pub(crate) struct MaterializedLongTermOwnerClosure {
    scope_manifest: LongTermMemoryVersionScopeManifest,
    head: LongTermMemoryHeadManifest,
    owner_materials: Vec<LongTermMemoryVersionMaterial>,
    dependency_heads: Vec<LongTermMemoryHeadManifest>,
    dependency_materials: Vec<LongTermMemoryVersionMaterial>,
    control_revisions: Vec<LongTermMemoryControlRevision>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MaterializedRuntimeSkillScopeClosure {
    memory_space_id: String,
    owning_scope: RuntimeSkillOwningScope,
    manifest: Option<RuntimeSkillScopeManifest>,
    records: Vec<RuntimeSkillOwnerRecord>,
}

#[allow(
    dead_code,
    reason = "typed RuntimeSkill read substrate consumed by the next production runtime step"
)]
impl MaterializedRuntimeSkillScopeClosure {
    pub(crate) fn memory_space_id(&self) -> &str {
        &self.memory_space_id
    }

    pub(crate) fn owning_scope(&self) -> &RuntimeSkillOwningScope {
        &self.owning_scope
    }

    pub(crate) fn manifest(&self) -> Option<&RuntimeSkillScopeManifest> {
        self.manifest.as_ref()
    }

    pub(crate) fn records(&self) -> &[RuntimeSkillOwnerRecord] {
        &self.records
    }
}

#[allow(
    dead_code,
    reason = "foundation type consumed by the production recall integration"
)]
pub(crate) struct RecallImmutableReadContext<'a> {
    session: Box<dyn StoreImmutableReadSession + 'a>,
    json_cache: BTreeMap<(String, String), Option<serde_json::Value>>,
    blob_cache: BTreeMap<(String, String), Option<Vec<u8>>>,
    json_observations: BTreeMap<(String, String), Option<serde_json::Value>>,
    blob_observations: BTreeMap<(String, String), Option<Vec<u8>>>,
    long_term_owner_closures: BTreeMap<GovernedMemoryOwnerRef, MaterializedLongTermOwnerClosure>,
    runtime_skill_scope_closures:
        BTreeMap<(String, RuntimeSkillOwningScope), MaterializedRuntimeSkillScopeClosure>,
    runtime_skill_premise_evidence: BTreeMap<GovernedOwnerRevisionRef, bool>,
    runtime_skill_task_evidence: BTreeMap<(PremiseTypedSource, String), (String, String, String)>,
}

pub(crate) struct RecallReadSetClosureEvidence {
    pub(crate) read_set_exact: bool,
}

#[allow(
    dead_code,
    reason = "foundation API consumed by the production recall integration"
)]
impl<'a> RecallImmutableReadContext<'a> {
    pub(crate) fn new(session: Box<dyn StoreImmutableReadSession + 'a>) -> Self {
        Self {
            session,
            json_cache: BTreeMap::new(),
            blob_cache: BTreeMap::new(),
            json_observations: BTreeMap::new(),
            blob_observations: BTreeMap::new(),
            long_term_owner_closures: BTreeMap::new(),
            runtime_skill_scope_closures: BTreeMap::new(),
            runtime_skill_premise_evidence: BTreeMap::new(),
            runtime_skill_task_evidence: BTreeMap::new(),
        }
    }

    fn ensure_long_term_owner_join_budget(
        &self,
        owner_ref: &GovernedMemoryOwnerRef,
        max_distinct_owners: usize,
    ) -> Result<()> {
        if max_distinct_owners == 0
            || self.long_term_owner_closures.len() > max_distinct_owners
            || (!self.long_term_owner_closures.contains_key(owner_ref)
                && self.long_term_owner_closures.len() >= max_distinct_owners)
        {
            return Err(Error::config(
                "governed_current_recall",
                "request-pinned validity join budget would be exceeded before owner IO",
            ));
        }
        Ok(())
    }

    fn record_json_observation(
        &mut self,
        address: (String, String),
        value: Option<serde_json::Value>,
    ) -> Result<()> {
        if self.json_observations.insert(address, value).is_some() {
            return Err(Error::config(
                "recall_immutable_read_session",
                "backend returned a duplicate JSON observation",
            ));
        }
        Ok(())
    }

    fn record_blob_observation(
        &mut self,
        address: (String, String),
        value: Option<Vec<u8>>,
    ) -> Result<()> {
        if self.blob_observations.insert(address, value).is_some() {
            return Err(Error::config(
                "recall_immutable_read_session",
                "backend returned a duplicate blob observation",
            ));
        }
        Ok(())
    }

    pub(crate) fn read_json_value(
        &mut self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>> {
        let address = (namespace.to_string(), key.to_string());
        if let Some(value) = self.json_cache.get(&address) {
            return Ok(value.clone());
        }
        let reads = self
            .session
            .read_json_known_keys(std::slice::from_ref(&address))?;
        let [read] = reads.as_slice() else {
            return Err(Error::config(
                "recall_immutable_read_session",
                "backend did not return exactly one JSON address",
            ));
        };
        if read.namespace != namespace || read.key != key {
            return Err(Error::config(
                "recall_immutable_read_session",
                "backend returned a different JSON address",
            ));
        }
        self.record_json_observation(address.clone(), read.value.clone())?;
        self.json_cache.insert(address, read.value.clone());
        Ok(read.value.clone())
    }

    pub(crate) fn read_json_values(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<RecallJsonRead>> {
        let mut unique = BTreeSet::new();
        let addresses = addresses
            .iter()
            .filter(|address| unique.insert((*address).clone()))
            .cloned()
            .collect::<Vec<_>>();
        let missing = addresses
            .iter()
            .filter(|address| !self.json_cache.contains_key(*address))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            let reads = self.session.read_json_known_keys(&missing)?;
            if reads.len() != missing.len() {
                return Err(Error::config(
                    "recall_immutable_read_session",
                    "backend did not return every requested JSON address",
                ));
            }
            let expected = missing.iter().cloned().collect::<BTreeSet<_>>();
            let mut observed = BTreeSet::new();
            for read in reads {
                let address = (read.namespace, read.key);
                if !expected.contains(&address) || !observed.insert(address.clone()) {
                    return Err(Error::config(
                        "recall_immutable_read_session",
                        "backend returned a different or duplicate JSON address",
                    ));
                }
                self.record_json_observation(address.clone(), read.value.clone())?;
                self.json_cache.insert(address, read.value);
            }
        }
        Ok(addresses
            .into_iter()
            .map(|address| {
                let value = self.json_cache.get(&address).cloned().unwrap_or(None);
                (address, value)
            })
            .collect())
    }

    pub(crate) fn read_json_values_transient(
        &mut self,
        addresses: &[(String, String)],
    ) -> Result<Vec<RecallJsonRead>> {
        let mut unique = BTreeSet::new();
        let addresses = addresses
            .iter()
            .filter(|address| unique.insert((*address).clone()))
            .cloned()
            .collect::<Vec<_>>();
        let missing = addresses
            .iter()
            .filter(|address| !self.json_cache.contains_key(*address))
            .cloned()
            .collect::<Vec<_>>();
        let reads = if missing.is_empty() {
            BTreeMap::new()
        } else {
            let reads = self.session.read_json_known_keys(&missing)?;
            if reads.len() != missing.len() {
                return Err(Error::config(
                    "recall_immutable_read_session",
                    "backend did not return every transient JSON address",
                ));
            }
            let expected = missing.iter().cloned().collect::<BTreeSet<_>>();
            let mut observed = BTreeMap::new();
            for read in reads {
                let address = (read.namespace, read.key);
                if !expected.contains(&address)
                    || observed
                        .insert(address.clone(), read.value.clone())
                        .is_some()
                {
                    return Err(Error::config(
                        "recall_immutable_read_session",
                        "backend returned a different or duplicate transient JSON address",
                    ));
                }
                self.record_json_observation(address, read.value)?;
            }
            observed
        };
        Ok(addresses
            .into_iter()
            .map(|address| {
                let value = self
                    .json_cache
                    .get(&address)
                    .cloned()
                    .or_else(|| reads.get(&address).cloned())
                    .unwrap_or(None);
                (address, value)
            })
            .collect())
    }

    pub(crate) fn read_json<T: DeserializeOwned>(
        &mut self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<T>> {
        self.read_json_value(namespace, key)?
            .map(|value| {
                serde_json::from_value(value).map_err(|error| {
                    Error::config(
                        "recall_immutable_read_session",
                        format!("invalid typed JSON at {namespace}/{key}: {error}"),
                    )
                })
            })
            .transpose()
    }

    pub(crate) fn read_typed_index<T: TypedRecallIndex>(
        &mut self,
        physical_key: &str,
    ) -> Result<Option<T>> {
        self.read_json_value(T::NAMESPACE, physical_key)?
            .map(|value| decode_typed_recall_index::<T>(physical_key, value))
            .transpose()
    }

    pub(crate) fn materialize_long_term_owner_closure(
        &mut self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        owner_ref: &GovernedMemoryOwnerRef,
        max_retained_revisions_per_owner: usize,
        max_distinct_owners: usize,
    ) -> Result<Option<MaterializedLongTermOwnerClosure>> {
        if max_retained_revisions_per_owner == 0 {
            return Err(Error::config(
                "recall_long_term_owner_closure",
                "request-pinned retention limit must be positive",
            ));
        }
        self.ensure_long_term_owner_join_budget(owner_ref, max_distinct_owners)?;
        if let Some(closure) = self.long_term_owner_closures.get(owner_ref) {
            return Ok(Some(closure.clone()));
        }
        let root_key = long_term_version_scope_manifest_key(memory_space_id, mounted_subject_id)?;
        let Some(root) = self.read_json::<LongTermMemoryVersionScopeManifest>(
            crate::store_internal::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
            &root_key,
        )?
        else {
            return Ok(None);
        };
        validate_long_term_scope_root_shape(&root, &root_key, memory_space_id, mounted_subject_id)?;
        let Some(head_binding) = root
            .head_bindings
            .iter()
            .find(|binding| &binding.owner_ref == owner_ref)
        else {
            return Ok(None);
        };
        let expected_head_key =
            long_term_version_head_key(memory_space_id, mounted_subject_id, owner_ref)?;
        if head_binding.head_physical_key != expected_head_key {
            return Err(Error::config(
                "recall_long_term_owner_closure",
                "scope root head binding address drift",
            ));
        }
        let head = self
            .read_json::<LongTermMemoryHeadManifest>(
                crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
                &head_binding.head_physical_key,
            )?
            .ok_or_else(|| {
                Error::config(
                    "recall_long_term_owner_closure",
                    "scope root head binding is missing its exact head",
                )
            })?;
        if LongTermMemoryVersionHeadBinding::from_head(&head)? != *head_binding
            || head.owner_ref != *owner_ref
            || head.memory_space_id != memory_space_id
            || head.mounted_subject_id != mounted_subject_id
            || head.retained_revision_digests.len() > max_retained_revisions_per_owner
        {
            return Err(Error::config(
                "recall_long_term_owner_closure",
                "long-term head identity, digest, scope, or retention bound drift",
            ));
        }

        let material_addresses = head
            .retained_revision_digests
            .iter()
            .map(|retained| {
                Ok((
                    crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE.to_string(),
                    long_term_version_material_key(
                        memory_space_id,
                        mounted_subject_id,
                        owner_ref,
                        retained.owner_revision,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let owner_materials = self
            .read_json_values(&material_addresses)?
            .into_iter()
            .map(|((_, key), value)| {
                value
                    .ok_or_else(|| {
                        Error::config(
                            "recall_long_term_owner_closure",
                            format!("retained long-term material is missing at {key}"),
                        )
                    })
                    .and_then(|value| {
                        serde_json::from_value::<LongTermMemoryVersionMaterial>(value).map_err(
                            |error| {
                                Error::config(
                                    "recall_long_term_owner_closure",
                                    format!("invalid retained material at {key}: {error}"),
                                )
                            },
                        )
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let retained_by_revision = head
            .retained_revision_digests
            .iter()
            .map(|retained| (retained.owner_revision, retained.content_digest.as_str()))
            .collect::<BTreeMap<_, _>>();
        if owner_materials.iter().any(|material| {
            !material.validate_contract().accepted
                || material.memory_space_id != memory_space_id
                || material.mounted_subject_id != mounted_subject_id
                || material.owner_ref != *owner_ref
                || retained_by_revision.get(&material.owner_revision).copied()
                    != Some(material.content_digest.as_str())
        }) {
            return Err(Error::config(
                "recall_long_term_owner_closure",
                "retained material identity, scope, or digest differs from the pinned head",
            ));
        }

        let expected_transition_refs = owner_materials
            .iter()
            .filter(|material| {
                material.owner_revision != head.current_revision
                    || head.terminal_transition_ref.as_ref() == Some(&material.owner_revision_ref())
            })
            .map(LongTermMemoryVersionMaterial::owner_revision_ref)
            .collect::<BTreeSet<_>>();
        let transition_bindings = root
            .transition_bindings
            .iter()
            .filter(|binding| expected_transition_refs.contains(&binding.predecessor))
            .collect::<Vec<_>>();
        if transition_bindings.len() != expected_transition_refs.len() {
            return Err(Error::config(
                "recall_long_term_owner_closure",
                "retained owner transition closure is incomplete",
            ));
        }
        let control_addresses = transition_bindings
            .iter()
            .map(|binding| {
                (
                    LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
                    binding.control_revision_physical_key.clone(),
                )
            })
            .collect::<Vec<_>>();
        let control_revisions = self
            .read_json_values(&control_addresses)?
            .into_iter()
            .map(|((_, key), value)| {
                value
                    .ok_or_else(|| {
                        Error::config(
                            "recall_long_term_owner_closure",
                            format!("bound long-term control revision is missing at {key}"),
                        )
                    })
                    .and_then(|value| {
                        serde_json::from_value::<LongTermMemoryControlRevision>(value).map_err(
                            |error| {
                                Error::config(
                                    "recall_long_term_owner_closure",
                                    format!("invalid long-term control revision at {key}: {error}"),
                                )
                            },
                        )
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut dependency_heads = BTreeMap::new();
        let mut dependency_materials = Vec::new();
        for (binding, revision) in transition_bindings.iter().zip(&control_revisions) {
            revision.validate_contract()?;
            let canonical_control_key = scoped_long_term_control_storage_key(
                memory_space_id,
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                &revision.revision_id,
            )?;
            if binding.control_revision_physical_key != canonical_control_key
                || revision.memory_space_id != memory_space_id
                || revision.mounted_subject_id != mounted_subject_id
                || revision.transition.predecessor != binding.predecessor
                || revision.content_digest != binding.control_revision_content_digest
            {
                return Err(Error::config(
                    "recall_long_term_owner_closure",
                    "control transition scope, predecessor, or digest drift",
                ));
            }
            if let Some(successor) = revision.transition.successor.as_ref() {
                if owner_materials
                    .iter()
                    .all(|material| material.owner_revision_ref() != *successor)
                {
                    let successor_key = long_term_version_material_key(
                        memory_space_id,
                        mounted_subject_id,
                        &successor.owner_ref,
                        successor.owner_revision,
                    )?;
                    let successor_material = self
                        .read_json::<LongTermMemoryVersionMaterial>(
                            crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE,
                            &successor_key,
                        )?
                        .ok_or_else(|| {
                            Error::config(
                                "recall_long_term_owner_closure",
                                "cross-owner successor material is missing",
                            )
                        })?;
                    let successor_head_binding = root
                        .head_bindings
                        .iter()
                        .find(|binding| binding.owner_ref == successor.owner_ref)
                        .ok_or_else(|| {
                            Error::config(
                                "recall_long_term_owner_closure",
                                "cross-owner successor head binding is missing",
                            )
                        })?;
                    let successor_head_key = long_term_version_head_key(
                        memory_space_id,
                        mounted_subject_id,
                        &successor.owner_ref,
                    )?;
                    if successor_head_binding.head_physical_key != successor_head_key {
                        return Err(Error::config(
                            "recall_long_term_owner_closure",
                            "cross-owner successor head address drift",
                        ));
                    }
                    let successor_head = self
                        .read_json::<LongTermMemoryHeadManifest>(
                            crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE,
                            &successor_head_key,
                        )?
                        .ok_or_else(|| {
                            Error::config(
                                "recall_long_term_owner_closure",
                                "cross-owner successor head is missing",
                            )
                        })?;
                    dependency_heads.insert(successor.owner_ref.clone(), successor_head);
                    dependency_materials.push(successor_material);
                }
            }
        }
        let all_materials = owner_materials
            .iter()
            .chain(&dependency_materials)
            .collect::<Vec<_>>();
        for revision in &control_revisions {
            let predecessor = all_materials
                .iter()
                .find(|material| material.owner_revision_ref() == revision.transition.predecessor)
                .copied()
                .ok_or_else(|| {
                    Error::config(
                        "recall_long_term_owner_closure",
                        "control predecessor material is missing",
                    )
                })?;
            let successor = revision
                .transition
                .successor
                .as_ref()
                .and_then(|successor| {
                    all_materials
                        .iter()
                        .find(|material| material.owner_revision_ref() == *successor)
                        .copied()
                });
            if revision.predecessor_material_digest != predecessor.content_digest
                || revision.successor_material_digest.as_deref()
                    != successor.map(|material| material.content_digest.as_str())
                || !revision
                    .transition
                    .validate_contract(predecessor, successor)
                    .accepted
            {
                return Err(Error::config(
                    "recall_long_term_owner_closure",
                    "control transition differs from its immutable material closure",
                ));
            }
        }
        let closure = MaterializedLongTermOwnerClosure {
            scope_manifest: root,
            head,
            owner_materials,
            dependency_heads: dependency_heads.into_values().collect(),
            dependency_materials,
            control_revisions,
        };
        self.long_term_owner_closures
            .insert(owner_ref.clone(), closure.clone());
        Ok(Some(closure))
    }

    pub(crate) fn materialize_long_term_historical_scope(
        &mut self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        max_retained_revisions_per_owner: usize,
        max_distinct_owners: usize,
        max_as_of_candidates: usize,
    ) -> Result<bool> {
        if max_retained_revisions_per_owner == 0
            || max_distinct_owners == 0
            || max_as_of_candidates == 0
        {
            return Err(Error::config(
                "governed_historical_recall",
                "request-pinned retention, owner join, and as-of candidate budgets must be positive",
            ));
        }
        let root_key = long_term_version_scope_manifest_key(memory_space_id, mounted_subject_id)?;
        let Some(root) = self.read_json::<LongTermMemoryVersionScopeManifest>(
            crate::store_internal::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
            &root_key,
        )?
        else {
            return Ok(false);
        };
        validate_long_term_scope_root_shape(&root, &root_key, memory_space_id, mounted_subject_id)?;

        let owner_count = root.head_bindings.len();
        if owner_count > max_as_of_candidates || owner_count > max_distinct_owners {
            return Err(Error::config(
                "governed_historical_recall",
                "pinned scope root exceeds the request-pinned owner or as-of candidate budget",
            ));
        }
        let root_owner_refs = root
            .head_bindings
            .iter()
            .map(|binding| binding.owner_ref.clone())
            .collect::<BTreeSet<_>>();
        if root_owner_refs.len() != owner_count
            || self
                .long_term_owner_closures
                .iter()
                .any(|(owner_ref, closure)| {
                    !root_owner_refs.contains(owner_ref) || closure.scope_manifest != root
                })
            || self
                .long_term_owner_closures
                .keys()
                .chain(root_owner_refs.iter())
                .cloned()
                .collect::<BTreeSet<_>>()
                .len()
                > max_distinct_owners
        {
            return Err(Error::config(
                "governed_historical_recall",
                "cached owner closures differ from the exact pinned historical scope root",
            ));
        }

        let mut owner_refs = root_owner_refs.into_iter().collect::<Vec<_>>();
        owner_refs.sort();
        for owner_ref in owner_refs {
            if self
                .materialize_long_term_owner_closure(
                    memory_space_id,
                    mounted_subject_id,
                    &owner_ref,
                    max_retained_revisions_per_owner,
                    max_distinct_owners,
                )?
                .is_none()
            {
                return Err(Error::config(
                    "governed_historical_recall",
                    "pinned historical scope root omitted a bound owner closure",
                ));
            }
        }
        Ok(true)
    }

    pub(crate) fn select_long_term_owner_as_of(
        &mut self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        owner_ref: &GovernedMemoryOwnerRef,
        as_of_time: u64,
        max_retained_revisions_per_owner: usize,
        max_distinct_owners: usize,
    ) -> Result<Option<LongTermMemoryVersionReadProjection>> {
        let Some(closure) = self.materialize_long_term_owner_closure(
            memory_space_id,
            mounted_subject_id,
            owner_ref,
            max_retained_revisions_per_owner,
            max_distinct_owners,
        )?
        else {
            return Ok(None);
        };
        let materials = closure
            .owner_materials
            .iter()
            .chain(&closure.dependency_materials)
            .cloned()
            .collect::<Vec<_>>();
        select_long_term_version_as_of(
            &closure.head,
            &materials,
            &closure
                .control_revisions
                .iter()
                .map(|revision| revision.transition.clone())
                .collect::<Vec<_>>(),
            as_of_time,
            max_retained_revisions_per_owner,
        )
    }

    pub(crate) fn materialize_runtime_skill_scope(
        &mut self,
        memory_space_id: &str,
        owning_scope: &RuntimeSkillOwningScope,
        max_owners_per_scope: usize,
    ) -> Result<Option<Vec<RuntimeSkillOwnerRecord>>> {
        if max_owners_per_scope == 0 {
            return Err(Error::config(
                "recall_runtime_skill_scope",
                "request-pinned runtime skill owner bound must be positive",
            ));
        }
        let scope_key = (memory_space_id.to_string(), owning_scope.clone());
        if let Some(closure) = self.runtime_skill_scope_closures.get(&scope_key) {
            if closure.records.len() > max_owners_per_scope {
                return Err(Error::config(
                    "recall_runtime_skill_scope",
                    "materialized runtime skill scope exceeds the current request-pinned bound",
                ));
            }
            return Ok(closure.manifest.is_some().then(|| closure.records.clone()));
        }
        let manifest_key = runtime_skill_scope_manifest_key(memory_space_id, owning_scope)?;
        let Some(manifest) = self.read_json::<RuntimeSkillScopeManifest>(
            crate::store_internal::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
            &manifest_key,
        )?
        else {
            self.runtime_skill_scope_closures.insert(
                scope_key,
                MaterializedRuntimeSkillScopeClosure {
                    memory_space_id: memory_space_id.to_string(),
                    owning_scope: owning_scope.clone(),
                    manifest: None,
                    records: Vec::new(),
                },
            );
            return Ok(None);
        };
        if manifest.physical_key != manifest_key
            || manifest.memory_space_id != memory_space_id
            || &manifest.owning_scope != owning_scope
            || manifest.owner_count != manifest.owner_bindings.len()
            || manifest.owner_count > max_owners_per_scope
            || manifest
                .owner_bindings
                .iter()
                .map(|binding| &binding.owner_ref)
                .collect::<BTreeSet<_>>()
                .len()
                != manifest.owner_bindings.len()
            || manifest
                .owner_bindings
                .iter()
                .map(|binding| &binding.owner_physical_key)
                .collect::<BTreeSet<_>>()
                .len()
                != manifest.owner_bindings.len()
        {
            return Err(Error::config(
                "recall_runtime_skill_scope",
                "runtime skill scope manifest identity, scope, or bound drift",
            ));
        }
        let owner_addresses = manifest
            .owner_bindings
            .iter()
            .map(|binding| {
                (
                    crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_string(),
                    binding.owner_physical_key.clone(),
                )
            })
            .collect::<Vec<_>>();
        let records = self
            .read_json_values(&owner_addresses)?
            .into_iter()
            .map(|((_, key), value)| {
                value
                    .ok_or_else(|| {
                        Error::config(
                            "recall_runtime_skill_scope",
                            format!("bound runtime skill owner is missing at {key}"),
                        )
                    })
                    .and_then(|value| {
                        serde_json::from_value::<RuntimeSkillOwnerRecord>(value).map_err(|error| {
                            Error::config(
                                "recall_runtime_skill_scope",
                                format!("invalid runtime skill owner at {key}: {error}"),
                            )
                        })
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let bindings = records
            .iter()
            .map(RuntimeSkillOwnerBinding::from_record)
            .collect::<Result<Vec<_>>>()?;
        manifest.validate_exact(
            memory_space_id,
            owning_scope,
            bindings,
            max_owners_per_scope,
        )?;
        self.runtime_skill_scope_closures.insert(
            scope_key,
            MaterializedRuntimeSkillScopeClosure {
                memory_space_id: memory_space_id.to_string(),
                owning_scope: owning_scope.clone(),
                manifest: Some(manifest),
                records: records.clone(),
            },
        );
        Ok(Some(records))
    }

    pub(crate) fn materialize_runtime_skill_scopes(
        &mut self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        max_owners_per_scope: usize,
    ) -> Result<(usize, usize)> {
        self.materialize_runtime_skill_scope(
            memory_space_id,
            &RuntimeSkillOwningScope::Subject {
                mounted_subject_id: mounted_subject_id.to_string(),
            },
            max_owners_per_scope,
        )?;
        self.materialize_runtime_skill_scope(
            memory_space_id,
            &RuntimeSkillOwningScope::SharedProgram,
            max_owners_per_scope,
        )?;
        let closures = [
            RuntimeSkillOwningScope::Subject {
                mounted_subject_id: mounted_subject_id.to_string(),
            },
            RuntimeSkillOwningScope::SharedProgram,
        ]
        .into_iter()
        .map(|scope| {
            self.runtime_skill_scope_closures
                .get(&(memory_space_id.to_string(), scope))
                .ok_or_else(|| {
                    Error::config(
                        "recall_runtime_skill_scope",
                        "materializer did not retain both exact scope observations",
                    )
                })
        })
        .collect::<Result<Vec<_>>>()?;
        let owner_key_count = closures.iter().try_fold(0usize, |total, closure| {
            total.checked_add(closure.records.len()).ok_or_else(|| {
                Error::config(
                    "recall_runtime_skill_scope",
                    "runtime skill owner-key count overflow",
                )
            })
        })?;
        Ok((closures.len(), owner_key_count))
    }

    pub(crate) fn materialize_runtime_skill_premise_evidence(
        &mut self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        max_evidence_reads_per_owner: usize,
    ) -> Result<()> {
        if max_evidence_reads_per_owner == 0 {
            return Err(Error::config(
                "recall_runtime_skill_premise_evidence",
                "premise evidence read budget must be positive",
            ));
        }
        let mut evidence_refs = BTreeSet::new();
        for closure in self.runtime_skill_scope_closures.values() {
            for owner in &closure.records {
                let mut owner_refs = BTreeSet::new();
                for requirement in &owner.intrinsic_contract.premises {
                    if let RuntimeSkillPremise::GovernedEnvironmentEvidence {
                        evidence_revision_ref,
                    } = &requirement.premise
                    {
                        owner_refs.insert(evidence_revision_ref.clone());
                    }
                    owner_refs.extend(requirement.governed_evidence_refs.iter().cloned());
                }
                if owner_refs.len() > max_evidence_reads_per_owner {
                    return Err(Error::config(
                        "recall_runtime_skill_premise_evidence",
                        "runtime skill premise evidence read budget exceeded before owner IO",
                    ));
                }
                evidence_refs.extend(owner_refs);
            }
        }
        let addresses = evidence_refs
            .iter()
            .map(|evidence_ref| {
                Ok((
                    super::GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                    scoped_governed_evidence_document_key(
                        memory_space_id,
                        &evidence_ref.owner_ref.owner_id,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        self.read_json_values(&addresses)?;
        for (evidence_ref, (_, key)) in evidence_refs.into_iter().zip(addresses) {
            let present = match self.cached_json::<GovernedEvidenceDocument>(
                super::GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE,
                &key,
            )? {
                Some(document) => {
                    validate_governed_evidence_document(&document).map_err(|rejection| {
                        Error::config(
                            "recall_runtime_skill_premise_evidence",
                            format!("invalid governed evidence owner: {rejection:?}"),
                        )
                    })?;
                    document.memory_space_id == memory_space_id
                        && document.mounted_subject_id == mounted_subject_id
                        && document.document_id == evidence_ref.owner_ref.owner_id
                        && document.owner_revision == evidence_ref.owner_revision
                }
                None => false,
            };
            self.runtime_skill_premise_evidence
                .insert(evidence_ref, present);
        }
        Ok(())
    }

    pub(crate) fn bind_runtime_skill_task_run_evidence(
        &mut self,
        memory_space_id: &str,
        channel_id: &str,
        chat_id: &str,
        record: &TaskRunRecord,
    ) -> Result<()> {
        if record.run.source_channel != channel_id
            || record.run.source_chat_id != chat_id
            || record.run.run_id.trim().is_empty()
        {
            return Err(Error::config(
                "recall_runtime_skill_task_evidence",
                "TaskRun evidence scope or identity drift",
            ));
        }
        self.bind_runtime_skill_task_evidence(
            PremiseTypedSource::TaskRun,
            &record.run.run_id,
            memory_space_id,
            channel_id,
            chat_id,
        )
    }

    pub(crate) fn bind_runtime_skill_task_learning_evidence(
        &mut self,
        memory_space_id: &str,
        channel_id: &str,
        chat_id: &str,
        record: &TaskLearningRecord,
    ) -> Result<()> {
        if record.source_channel != channel_id
            || record.source_chat_id != chat_id
            || record.learning_id.trim().is_empty()
        {
            return Err(Error::config(
                "recall_runtime_skill_task_evidence",
                "TaskLearning evidence scope or identity drift",
            ));
        }
        self.bind_runtime_skill_task_evidence(
            PremiseTypedSource::TaskLearning,
            &record.learning_id,
            memory_space_id,
            channel_id,
            chat_id,
        )
    }

    fn bind_runtime_skill_task_evidence(
        &mut self,
        source: PremiseTypedSource,
        safe_ref: &str,
        memory_space_id: &str,
        channel_id: &str,
        chat_id: &str,
    ) -> Result<()> {
        let key = (source, safe_ref.to_string());
        let binding = (
            memory_space_id.to_string(),
            channel_id.to_string(),
            chat_id.to_string(),
        );
        if self
            .runtime_skill_task_evidence
            .get(&key)
            .is_some_and(|existing| existing != &binding)
        {
            return Err(Error::config(
                "recall_runtime_skill_task_evidence",
                "typed task evidence has conflicting session scope",
            ));
        }
        self.runtime_skill_task_evidence.insert(key, binding);
        Ok(())
    }

    pub(crate) fn materialize_typed_index<T: TypedRecallIndex>(
        &mut self,
        physical_key: &str,
    ) -> Result<Option<T>> {
        let Some(index) = self.read_typed_index::<T>(physical_key)? else {
            return Ok(None);
        };
        let json_addresses = index
            .entries()
            .iter()
            .filter(|entry| entry.kind == super::recall_index::RecallIndexAddressKind::Json)
            .map(|entry| (entry.namespace.clone(), entry.key.clone()))
            .collect::<Vec<_>>();
        self.read_json_values(&json_addresses)?;
        for entry in index.entries() {
            match entry.kind {
                super::recall_index::RecallIndexAddressKind::Json => {
                    let value = self
                        .json_cache
                        .get(&(entry.namespace.clone(), entry.key.clone()))
                        .and_then(Option::as_ref)
                        .ok_or_else(|| {
                            Error::config(
                                "typed_recall_index_entry",
                                format!(
                                    "{} declared JSON owner is missing at {}/{}",
                                    T::KIND,
                                    entry.namespace,
                                    entry.key
                                ),
                            )
                        })?;
                    let bytes = serde_json::to_vec(value).map_err(|error| {
                        Error::config("typed_recall_index_entry", error.to_string())
                    })?;
                    verify_entry_digest::<T>(entry, &bytes)?;
                }
                super::recall_index::RecallIndexAddressKind::Blob => {
                    let value = self
                        .read_blob(&entry.namespace, &entry.key)?
                        .ok_or_else(|| {
                            Error::config(
                                "typed_recall_index_entry",
                                format!(
                                    "{} declared blob owner is missing at {}/{}",
                                    T::KIND,
                                    entry.namespace,
                                    entry.key
                                ),
                            )
                        })?;
                    verify_entry_digest::<T>(entry, &value)?;
                }
            }
        }
        Ok(Some(index))
    }

    pub(crate) fn read_blob(&mut self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let address = (namespace.to_string(), key.to_string());
        if let Some(value) = self.blob_cache.get(&address) {
            return Ok(value.clone());
        }
        let reads = self
            .session
            .read_blob_known_keys(std::slice::from_ref(&address))?;
        let [read] = reads.as_slice() else {
            return Err(Error::config(
                "recall_immutable_read_session",
                "backend did not return exactly one blob address",
            ));
        };
        if read.namespace != namespace || read.key != key {
            return Err(Error::config(
                "recall_immutable_read_session",
                "backend returned a different blob address",
            ));
        }
        self.record_blob_observation(address.clone(), read.value.clone())?;
        self.blob_cache.insert(address, read.value.clone());
        Ok(read.value.clone())
    }

    pub(crate) fn finish(self) -> Result<(StoreReadReceipt, RecallReadSetClosureEvidence)> {
        let receipt = self.session.receipt()?;
        let json_doc_count = self
            .json_observations
            .values()
            .filter(|value| value.is_some())
            .count();
        let blob_count = self
            .blob_observations
            .values()
            .filter(|value| value.is_some())
            .count();
        let entry_count = self
            .json_observations
            .len()
            .checked_add(self.blob_observations.len())
            .ok_or_else(|| {
                Error::config(
                    "recall_immutable_read_session",
                    "materialized read-set count overflow",
                )
            })?;
        let json_bytes = self
            .json_observations
            .values()
            .filter_map(Option::as_ref)
            .try_fold(0usize, |total, value| {
                let bytes = serde_json::to_vec(value).map_err(|error| {
                    Error::config("recall_immutable_read_session", error.to_string())
                })?;
                total.checked_add(bytes.len()).ok_or_else(|| {
                    Error::config(
                        "recall_immutable_read_session",
                        "materialized JSON byte count overflow",
                    )
                })
            })?;
        let blob_bytes = self
            .blob_observations
            .values()
            .filter_map(Option::as_ref)
            .try_fold(0usize, |total, value| {
                total.checked_add(value.len()).ok_or_else(|| {
                    Error::config(
                        "recall_immutable_read_session",
                        "materialized blob byte count overflow",
                    )
                })
            })?;
        let read_set_exact = receipt.json_doc_count == json_doc_count
            && receipt.blob_count == blob_count
            && receipt.event_count == 0
            && receipt.entry_count == entry_count
            && receipt.json_bytes == json_bytes
            && receipt.blob_bytes == blob_bytes;
        if !read_set_exact {
            return Err(Error::config(
                "recall_immutable_read_session",
                "immutable receipt differs from the materialized exact read set",
            ));
        }
        Ok((receipt, RecallReadSetClosureEvidence { read_set_exact }))
    }

    pub(crate) fn cached_json<T: DeserializeOwned>(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<T>> {
        self.json_cache
            .get(&(namespace.to_string(), key.to_string()))
            .and_then(Option::as_ref)
            .cloned()
            .map(|value| {
                serde_json::from_value(value).map_err(|error| {
                    Error::config(
                        "recall_immutable_read_session",
                        format!("invalid cached typed JSON at {namespace}/{key}: {error}"),
                    )
                })
            })
            .transpose()
    }

    pub(crate) fn cached_json_docs<T: DeserializeOwned>(&self, namespace: &str) -> Result<Vec<T>> {
        self.json_cache
            .iter()
            .filter_map(|((observed_namespace, _), value)| {
                (observed_namespace == namespace)
                    .then_some(value.as_ref())
                    .flatten()
            })
            .cloned()
            .map(|value| {
                serde_json::from_value(value).map_err(|error| {
                    Error::config("recall_immutable_read_session", error.to_string())
                })
            })
            .collect()
    }

    pub(crate) fn take_materialized_view(&mut self) -> RecallReadView {
        RecallReadView {
            json: std::mem::take(&mut self.json_cache),
            blobs: std::mem::take(&mut self.blob_cache),
            long_term_owner_closures: std::mem::take(&mut self.long_term_owner_closures),
            runtime_skill_scope_closures: std::mem::take(&mut self.runtime_skill_scope_closures),
            runtime_skill_premise_evidence: std::mem::take(
                &mut self.runtime_skill_premise_evidence,
            ),
            runtime_skill_task_evidence: std::mem::take(&mut self.runtime_skill_task_evidence),
        }
    }

    #[cfg(test)]
    pub(crate) fn cached_address_counts(&self) -> (usize, usize) {
        (self.json_cache.len(), self.blob_cache.len())
    }
}

fn validate_long_term_scope_root_shape(
    root: &LongTermMemoryVersionScopeManifest,
    expected_key: &str,
    memory_space_id: &str,
    mounted_subject_id: &str,
) -> Result<()> {
    if root.schema_version != LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION
        || root.physical_key != expected_key
        || root.memory_space_id != memory_space_id
        || root.mounted_subject_id != mounted_subject_id
        || root.manifest_revision == 0
        || root.head_count != root.head_bindings.len() as u64
        || root.transition_count != root.transition_bindings.len() as u64
        || root.material_count < root.head_count
        || root.closure_digest.len() != 64
        || !root
            .closure_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || root.head_bindings.windows(2).any(|pair| pair[0] >= pair[1])
        || root
            .transition_bindings
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(Error::config(
            "recall_long_term_owner_closure",
            "long-term scope root shape, counts, order, or digest is invalid",
        ));
    }
    Ok(())
}

fn verify_entry_digest<T: TypedRecallIndex>(
    entry: &super::recall_index::RecallIndexAddress,
    bytes: &[u8],
) -> Result<()> {
    let observed = format!("sha256:{:x}", Sha256::digest(bytes));
    if observed != entry.content_sha256 {
        return Err(Error::config(
            "typed_recall_index_entry",
            format!("{} declared owner content digest drift", T::KIND),
        ));
    }
    Ok(())
}

#[derive(Clone, Default)]
pub(crate) struct RecallReadView {
    json: BTreeMap<(String, String), Option<serde_json::Value>>,
    blobs: BTreeMap<(String, String), Option<Vec<u8>>>,
    long_term_owner_closures: BTreeMap<GovernedMemoryOwnerRef, MaterializedLongTermOwnerClosure>,
    runtime_skill_scope_closures:
        BTreeMap<(String, RuntimeSkillOwningScope), MaterializedRuntimeSkillScopeClosure>,
    runtime_skill_premise_evidence: BTreeMap<GovernedOwnerRevisionRef, bool>,
    runtime_skill_task_evidence: BTreeMap<(PremiseTypedSource, String), (String, String, String)>,
}

pub(crate) struct RecallReadJsonDoc {
    pub(crate) key: String,
    pub(crate) value: serde_json::Value,
}

impl RecallReadView {
    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn retained_long_term_counterfactual_inputs(
        &self,
    ) -> Result<
        Vec<(
            LongTermMemoryVersionMaterial,
            Option<LongTermMemoryControlRevision>,
        )>,
    > {
        let mut materials =
            BTreeMap::<GovernedOwnerRevisionRef, LongTermMemoryVersionMaterial>::new();
        let mut controls =
            BTreeMap::<GovernedOwnerRevisionRef, LongTermMemoryControlRevision>::new();
        for closure in self.long_term_owner_closures.values() {
            for material in closure
                .owner_materials
                .iter()
                .chain(&closure.dependency_materials)
            {
                insert_exact(
                    &mut materials,
                    material.owner_revision_ref(),
                    material.clone(),
                    "P8 retained material",
                )?;
            }
            for control in &closure.control_revisions {
                insert_exact(
                    &mut controls,
                    control.transition.predecessor.clone(),
                    control.clone(),
                    "P8 retained control revision",
                )?;
            }
        }
        Ok(materials
            .into_iter()
            .map(|(revision_ref, material)| {
                let control = controls.remove(&revision_ref);
                (material, control)
            })
            .collect())
    }

    #[allow(
        dead_code,
        reason = "typed RuntimeSkill read substrate consumed by the next production runtime step"
    )]
    pub(crate) fn runtime_skill_scope(
        &self,
        memory_space_id: &str,
        owning_scope: &RuntimeSkillOwningScope,
    ) -> Option<&MaterializedRuntimeSkillScopeClosure> {
        self.runtime_skill_scope_closures
            .get(&(memory_space_id.to_string(), owning_scope.clone()))
    }

    pub(crate) fn runtime_skill_premise_evidence(
        &self,
        evidence_ref: &GovernedOwnerRevisionRef,
    ) -> Option<bool> {
        self.runtime_skill_premise_evidence
            .get(evidence_ref)
            .copied()
    }

    pub(crate) fn runtime_skill_task_evidence_present(
        &self,
        source: PremiseTypedSource,
        safe_ref: &str,
        memory_space_id: &str,
        channel_id: &str,
        chat_id: &str,
    ) -> Result<bool> {
        if !matches!(
            source,
            PremiseTypedSource::TaskRun | PremiseTypedSource::TaskLearning
        ) {
            return Err(Error::config(
                "recall_runtime_skill_task_evidence",
                "unsupported task evidence source",
            ));
        }
        Ok(self
            .runtime_skill_task_evidence
            .get(&(source, safe_ref.to_string()))
            .is_some_and(|binding| {
                binding
                    == &(
                        memory_space_id.to_string(),
                        channel_id.to_string(),
                        chat_id.to_string(),
                    )
            }))
    }

    pub(crate) fn current_long_term_projections(
        &self,
        max_retained_revisions_per_owner: usize,
    ) -> Result<BTreeMap<GovernedMemoryOwnerRef, LongTermCurrentRecallAuthority>> {
        self.long_term_owner_closures
            .iter()
            .map(|(owner_ref, closure)| {
                let authority = build_long_term_current_recall_authority(
                    &closure.scope_manifest,
                    &closure.head,
                    &closure.owner_materials,
                    &closure.dependency_heads,
                    &closure.dependency_materials,
                    &closure.control_revisions,
                    max_retained_revisions_per_owner,
                )?;
                Ok((owner_ref.clone(), authority))
            })
            .collect()
    }

    pub(crate) fn historical_long_term_authorities(
        &self,
        as_of_time: u64,
        max_retained_revisions_per_owner: usize,
        max_lineage_depth: usize,
        max_as_of_candidates: usize,
    ) -> Result<BTreeMap<GovernedMemoryOwnerRef, LongTermHistoricalRecallAuthority>> {
        if as_of_time == 0
            || max_retained_revisions_per_owner == 0
            || max_lineage_depth == 0
            || max_as_of_candidates == 0
        {
            return Err(Error::config(
                "governed_historical_recall",
                "as-of time and request-pinned historical budgets must be positive",
            ));
        }
        let Some(root) = self
            .long_term_owner_closures
            .values()
            .next()
            .map(|closure| closure.scope_manifest.clone())
        else {
            return Ok(BTreeMap::new());
        };
        let expected_owner_refs = root
            .head_bindings
            .iter()
            .map(|binding| binding.owner_ref.clone())
            .collect::<BTreeSet<_>>();
        if expected_owner_refs.len() != root.head_bindings.len()
            || expected_owner_refs.len() > max_as_of_candidates
            || self.long_term_owner_closures.len() != expected_owner_refs.len()
            || self
                .long_term_owner_closures
                .iter()
                .any(|(owner_ref, closure)| {
                    !expected_owner_refs.contains(owner_ref) || closure.scope_manifest != root
                })
        {
            return Err(Error::config(
                "governed_historical_recall",
                "materialized owners differ from the exact pinned historical scope root",
            ));
        }

        let mut heads = BTreeMap::<GovernedMemoryOwnerRef, LongTermMemoryHeadManifest>::new();
        let mut materials =
            BTreeMap::<GovernedOwnerRevisionRef, LongTermMemoryVersionMaterial>::new();
        let mut control_revisions =
            BTreeMap::<GovernedOwnerRevisionRef, LongTermMemoryControlRevision>::new();
        for closure in self.long_term_owner_closures.values() {
            for head in std::iter::once(&closure.head).chain(&closure.dependency_heads) {
                insert_exact(
                    &mut heads,
                    head.owner_ref.clone(),
                    head.clone(),
                    "historical head",
                )?;
            }
            for material in closure
                .owner_materials
                .iter()
                .chain(&closure.dependency_materials)
            {
                insert_exact(
                    &mut materials,
                    material.owner_revision_ref(),
                    material.clone(),
                    "historical material",
                )?;
            }
            for revision in &closure.control_revisions {
                insert_exact(
                    &mut control_revisions,
                    revision.transition.predecessor.clone(),
                    revision.clone(),
                    "historical control revision",
                )?;
            }
        }
        if heads.keys().cloned().collect::<BTreeSet<_>>() != expected_owner_refs {
            return Err(Error::config(
                "governed_historical_recall",
                "historical head closure differs from the exact pinned scope root",
            ));
        }
        let heads = heads.into_values().collect::<Vec<_>>();
        let materials = materials.into_values().collect::<Vec<_>>();
        let control_revisions = control_revisions.into_values().collect::<Vec<_>>();
        let mut authorities = BTreeMap::new();
        for owner_ref in expected_owner_refs {
            if let Some(authority) = build_long_term_historical_recall_authority(
                &root,
                &heads,
                &materials,
                &control_revisions,
                &owner_ref,
                as_of_time,
                max_retained_revisions_per_owner,
                max_lineage_depth,
            )? {
                authorities.insert(owner_ref, authority);
            }
        }
        if authorities.len() > max_as_of_candidates {
            return Err(Error::config(
                "governed_historical_recall",
                "historical authority count exceeds the request-pinned as-of candidate budget",
            ));
        }
        Ok(authorities)
    }

    pub(crate) fn json_value(&self, namespace: &str, key: &str) -> Option<&serde_json::Value> {
        self.json
            .get(&(namespace.to_string(), key.to_string()))
            .and_then(Option::as_ref)
    }

    pub(crate) fn json<T: DeserializeOwned>(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<T>> {
        self.json_value(namespace, key)
            .cloned()
            .map(|value| {
                serde_json::from_value(value).map_err(|error| {
                    Error::config(
                        "recall_read_view",
                        format!("invalid typed JSON at {namespace}/{key}: {error}"),
                    )
                })
            })
            .transpose()
    }

    pub(crate) fn json_docs<T: DeserializeOwned>(&self, namespace: &str) -> Result<Vec<T>> {
        self.json
            .iter()
            .filter_map(|((observed_namespace, _), value)| {
                (observed_namespace == namespace)
                    .then_some(value.as_ref())
                    .flatten()
            })
            .cloned()
            .map(|value| {
                serde_json::from_value(value)
                    .map_err(|error| Error::config("recall_read_view", error.to_string()))
            })
            .collect()
    }

    pub(crate) fn json_docs_by_keys(
        &self,
        namespace: &str,
        keys: &[String],
    ) -> Result<Vec<RecallReadJsonDoc>> {
        let mut seen = BTreeSet::new();
        Ok(keys
            .iter()
            .filter(|key| seen.insert((*key).clone()))
            .filter_map(|key| {
                self.json_value(namespace, key)
                    .cloned()
                    .map(|value| RecallReadJsonDoc {
                        key: key.clone(),
                        value,
                    })
            })
            .collect())
    }

    pub(crate) fn blob(&self, namespace: &str, key: &str) -> Option<&[u8]> {
        self.blobs
            .get(&(namespace.to_string(), key.to_string()))
            .and_then(Option::as_deref)
    }

    fn current_long_term_entries(&self) -> Result<Vec<LongTermMemoryEntry>> {
        self.long_term_owner_closures
            .values()
            .filter(|closure| closure.head.terminal_transition_ref.is_none())
            .map(|closure| {
                let materials = closure
                    .owner_materials
                    .iter()
                    .chain(&closure.dependency_materials)
                    .cloned()
                    .collect::<Vec<_>>();
                let transitions = closure
                    .control_revisions
                    .iter()
                    .map(|revision| revision.transition.clone())
                    .collect::<Vec<_>>();
                select_long_term_version_current(
                    &closure.head,
                    &materials,
                    &transitions,
                    closure.owner_materials.len().max(1),
                )?
                .material
                .to_current_projection()
            })
            .collect()
    }

    fn reject_write<T>(&self) -> Result<T> {
        Err(Error::config(
            "recall_read_view",
            "materialized recall view is immutable",
        ))
    }
}

fn insert_exact<K, V>(
    values: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    kind: &'static str,
) -> Result<()>
where
    K: Ord,
    V: PartialEq,
{
    if values.get(&key).is_some_and(|existing| existing != &value) {
        return Err(Error::config(
            "governed_historical_recall",
            format!("duplicate {kind} differs across the pinned closure"),
        ));
    }
    values.entry(key).or_insert(value);
    Ok(())
}

#[derive(Deserialize)]
struct MaterializedSessionSummary {
    summary: String,
    message_count: usize,
}

impl SessionSummaryStore for RecallReadView {
    fn get(&self, chat_id: &str) -> Result<Option<String>> {
        Ok(self
            .json::<MaterializedSessionSummary>("session_summary", chat_id)?
            .map(|record| record.summary))
    }

    fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
        self.reject_write()
    }

    fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
        Ok(self
            .json::<MaterializedSessionSummary>("session_summary", chat_id)?
            .map(|record| (record.summary, record.message_count)))
    }
}

impl SessionStore for RecallReadView {
    fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
        self.reject_write()
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let mut messages = self
            .json::<Vec<SessionMessage>>("session", chat_id)?
            .unwrap_or_default();
        if messages.len() > n {
            messages = messages.split_off(messages.len() - n);
        }
        Ok(messages)
    }

    fn load_recent_records(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessageRecord>> {
        Ok(self
            .load_recent(chat_id, n)?
            .into_iter()
            .map(SessionMessageRecord::from)
            .collect())
    }

    fn message_count(&self, chat_id: &str) -> Result<usize> {
        Ok(self
            .json::<Vec<SessionMessage>>("session", chat_id)?
            .map_or(0, |messages| messages.len()))
    }

    fn clear(&self, _chat_id: &str) -> Result<()> {
        self.reject_write()
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .json
            .iter()
            .filter_map(|((namespace, key), value)| {
                (namespace == "session" && value.is_some()).then_some(key.clone())
            })
            .collect())
    }
}

impl MemoryStore for RecallReadView {
    fn get_memory(&self) -> Result<String> {
        Ok(self
            .blob("memory", "MEMORY.md")
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default())
    }

    fn set_memory(&self, _content: &str) -> Result<()> {
        self.reject_write()
    }

    fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
        let mut names = self
            .blobs
            .iter()
            .filter_map(|((namespace, key), value)| {
                (namespace == "daily" && value.is_some()).then_some(key.clone())
            })
            .collect::<Vec<_>>();
        names.sort_by(|left, right| right.cmp(left));
        names.truncate(recent_n);
        Ok(names)
    }

    fn get_daily_note(&self, name: &str) -> Result<String> {
        Ok(self
            .blob("daily", name)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default())
    }

    fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
        self.reject_write()
    }
}

impl ActiveWorkStore for RecallReadView {
    fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
        self.json("active_work", chat_id)
    }

    fn set(&self, _chat_id: &str, _record: &ActiveWorkRecord) -> Result<()> {
        self.reject_write()
    }
}

impl TurnLedgerStore for RecallReadView {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        self.json("turn_ledger", chat_id)
    }

    fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
        self.reject_write()
    }

    fn clear(&self, _chat_id: &str) -> Result<()> {
        self.reject_write()
    }
}

impl SkillStorage for RecallReadView {
    fn list_names(&self) -> Result<Vec<String>> {
        Ok(self
            .blobs
            .iter()
            .filter_map(|((namespace, key), value)| {
                (namespace == "skills" && value.is_some()).then_some(key.clone())
            })
            .collect())
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.blob("skills", name)
            .map(ToOwned::to_owned)
            .ok_or_else(|| Error::config("recall_read_view", "runtime skill is not materialized"))
    }

    fn write(&self, _name: &str, _content: &[u8]) -> Result<()> {
        self.reject_write()
    }

    fn remove(&self, _name: &str) -> Result<()> {
        self.reject_write()
    }
}

impl ContinuityCapsuleStore for RecallReadView {
    fn upsert_many(
        &self,
        _drafts: &[ContinuityCapsuleDraft],
        _now_secs: u64,
    ) -> Result<ContinuityCapsuleWriteOutcome> {
        self.reject_write()
    }

    fn get(&self, capsule_id: &str) -> Result<Option<ContinuityCapsule>> {
        self.json("continuity_capsule", capsule_id)
    }

    fn list(&self, limit: usize) -> Result<Vec<ContinuityCapsule>> {
        let mut values = self.json_docs::<ContinuityCapsule>("continuity_capsule")?;
        values.sort_by_key(|capsule| Reverse(capsule.updated_at));
        values.truncate(limit);
        Ok(values)
    }

    fn count(&self) -> Result<usize> {
        Ok(self
            .json_docs::<ContinuityCapsule>("continuity_capsule")?
            .len())
    }

    fn list_for_scope(
        &self,
        scope_kind: ContinuityCapsuleScopeKind,
        scope_id: &str,
        limit: usize,
    ) -> Result<Vec<ContinuityCapsule>> {
        let mut values = ContinuityCapsuleStore::list(self, usize::MAX)?;
        values.retain(|capsule| capsule.scope_kind == scope_kind && capsule.scope_id == scope_id);
        values.truncate(limit.max(1));
        Ok(values)
    }
}

impl TaskRunStore for RecallReadView {
    fn get(&self, run_id: &str) -> Result<Option<TaskRunRecord>> {
        self.json("task_run", run_id)
    }

    fn upsert(&self, _record: &TaskRunRecord) -> Result<()> {
        self.reject_write()
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskRunRecord>> {
        let mut values = self.json_docs::<TaskRunRecord>("task_run")?;
        values.sort_by_key(|record| Reverse(record.run.updated_at));
        values.truncate(limit);
        Ok(values)
    }

    fn list_active_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskRunRecord>> {
        let mut values = TaskRunStore::list_recent(self, usize::MAX)?;
        values.retain(|record| {
            record.run.source_channel == channel
                && record.run.source_chat_id == chat_id
                && record.run.status.is_active()
        });
        values.truncate(limit);
        Ok(values)
    }
}

impl TaskLearningStore for RecallReadView {
    fn get(&self, learning_id: &str) -> Result<Option<TaskLearningRecord>> {
        self.json("task_learning", learning_id)
    }

    fn upsert(&self, _record: &TaskLearningRecord) -> Result<()> {
        self.reject_write()
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        let mut values = self.json_docs::<TaskLearningRecord>("task_learning")?;
        values.sort_by_key(|record| Reverse(record.observed_at));
        values.truncate(limit);
        Ok(values)
    }

    fn list_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskLearningRecord>> {
        let mut values = TaskLearningStore::list_recent(self, usize::MAX)?;
        values
            .retain(|record| record.source_channel == channel && record.source_chat_id == chat_id);
        values.truncate(limit);
        Ok(values)
    }

    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        let mut values = TaskLearningStore::list_recent(self, usize::MAX)?;
        values.retain(|record| record.run_id == run_id);
        values.truncate(limit);
        Ok(values)
    }
}

impl LongTermMemoryReadStore for RecallReadView {
    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = LongTermMemoryReadStore::list(self, usize::MAX)?;
        let query = query.trim().to_lowercase();
        if !query.is_empty() {
            entries.retain(|entry| {
                entry.topic.to_lowercase().contains(&query)
                    || entry.content.to_lowercase().contains(&query)
                    || entry
                        .keywords
                        .iter()
                        .any(|keyword| keyword.to_lowercase().contains(&query))
            });
        }
        entries.sort_by(|left, right| {
            let left_scope = usize::from(left.source_chat_id.as_deref() == source_chat_id);
            let right_scope = usize::from(right.source_chat_id.as_deref() == source_chat_id);
            right_scope
                .cmp(&left_scope)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
        });
        entries.truncate(limit);
        Ok(entries)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        Ok(self
            .current_long_term_entries()?
            .into_iter()
            .find(|entry| entry.id == id))
    }

    fn query(&self, query: &LongTermMemoryQuery) -> Result<Vec<LongTermMemoryEntry>> {
        Ok(query.filter_sort_entries(
            LongTermMemoryReadStore::list(self, usize::MAX)?,
            bm_core::util::current_unix_secs(),
        ))
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = self.current_long_term_entries()?;
        entries.sort_by_key(|entry| Reverse(entry.updated_at));
        entries.truncate(limit);
        Ok(entries)
    }

    fn count(&self) -> Result<usize> {
        Ok(self.current_long_term_entries()?.len())
    }
}

impl ConversationTranscriptStore for RecallReadView {
    fn append_turn(&self, _record: &TranscriptTurnRecord) -> Result<TranscriptCommitReport> {
        self.reject_write()
    }

    fn remember_conversation_alias(&self, _alias: &TranscriptConversationAlias) -> Result<()> {
        self.reject_write()
    }

    fn resolve_conversation_alias(
        &self,
        memory_space_id: &str,
        mounted_subject_id: &str,
        channel_id: &str,
        chat_id: &str,
    ) -> Result<Option<String>> {
        let key = TranscriptConversationAlias::storage_key_for(
            memory_space_id,
            mounted_subject_id,
            channel_id,
            chat_id,
        );
        Ok(self
            .json::<TranscriptConversationAlias>("conversation_transcript_alias", &key)?
            .map(|alias| alias.conversation_id))
    }

    fn get_turn(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: &str,
    ) -> Result<Option<TranscriptTurnRecord>> {
        Ok(self
            .list_turns(key, mounted_subject_id, usize::MAX)?
            .into_iter()
            .find(|record| record.turn_id == turn_id))
    }

    fn list_turns(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        limit: usize,
    ) -> Result<Vec<TranscriptTurnRecord>> {
        let mut records = self.json_docs::<TranscriptTurnRecord>("conversation_transcript")?;
        records.retain(|record| record.key == *key);
        if records
            .iter()
            .any(|record| record.subject != mounted_subject_id)
        {
            return Err(Error::config(
                "recall_read_view",
                "conversation transcript owner differs from requested subject",
            ));
        }
        records.sort_by_key(|record| record.sequence);
        if records.len() > limit {
            records = records.split_off(records.len() - limit);
        }
        Ok(records)
    }

    fn upsert_transcript_attrs(
        &self,
        _key: &ConversationKey,
        _mounted_subject_id: &str,
        _attrs: &[TranscriptAttrEnvelope],
    ) -> Result<TranscriptAttrWriteReport> {
        self.reject_write()
    }

    fn list_transcript_attrs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Vec<TranscriptAttrEnvelope>> {
        let mut attrs = self.json_docs::<TranscriptAttrEnvelope>("conversation_transcript_attr")?;
        attrs.retain(|attr| {
            attr.target.key == *key && turn_id.is_none_or(|turn_id| attr.target.turn_id == turn_id)
        });
        for attr in &attrs {
            let turn = self
                .get_turn(key, mounted_subject_id, &attr.target.turn_id)?
                .ok_or_else(|| {
                    Error::config(
                        "recall_read_view",
                        "transcript attr target owner is missing",
                    )
                })?;
            attr.validate_for_record(&turn)?;
        }
        attrs.sort_by(|left, right| left.attr_id.cmp(&right.attr_id));
        Ok(attrs)
    }

    fn append_derived_memory_ref(
        &self,
        _key: &ConversationKey,
        _derived: &DerivedMemoryRef,
    ) -> Result<()> {
        self.reject_write()
    }

    fn list_derived_memory_refs(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Vec<DerivedMemoryRef>> {
        let mut refs = self.json_docs::<DerivedMemoryRef>("conversation_transcript_derived_ref")?;
        refs.retain(|derived| {
            derived.source.memory_space_id == key.memory_space_id
                && derived.source.channel_id == key.channel_id
                && derived.source.conversation_id == key.conversation_id
                && turn_id.is_none_or(|turn_id| derived.source.turn_id == turn_id)
        });
        for derived in &refs {
            let owner = derived
                .subject_id
                .as_deref()
                .or(derived.source.subject_id.as_deref());
            if owner != Some(mounted_subject_id)
                || derived
                    .subject_id
                    .as_deref()
                    .zip(derived.source.subject_id.as_deref())
                    .is_some_and(|(owner, source_owner)| owner != source_owner)
            {
                return Err(Error::config(
                    "recall_read_view",
                    "transcript derived owner differs from requested subject",
                ));
            }
        }
        Ok(refs)
    }

    fn apply_lifecycle_request(
        &self,
        _mounted_subject_id: &str,
        _request: &TranscriptLifecycleRequest,
    ) -> Result<TranscriptLifecycleReport> {
        self.reject_write()
    }

    fn redacted_replay(
        &self,
        key: &ConversationKey,
        mounted_subject_id: &str,
        limit: usize,
        view: TranscriptReplayView,
    ) -> Result<RedactedTranscriptSlice> {
        let records = self.list_turns(key, mounted_subject_id, limit)?;
        let attrs = self.list_transcript_attrs(key, mounted_subject_id, None)?;
        Ok(RedactedTranscriptSlice::from_records_with_attrs(
            key.clone(),
            view,
            &records,
            &attrs,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use bm_core::memory::{
        governed_evidence_document_content_digest, plan_governed_evidence_document_upsert,
        GovernedEvidenceDocumentDraft, GovernedEvidenceDocumentPlan,
        GovernedEvidenceDocumentSourceKind, GovernedMemoryOwnerPlane, GovernedOwnerRevisionRef,
        GovernedOwnerTermination, LongTermControlOperation, LongTermMemoryConfidence,
        LongTermMemoryFreshness, LongTermMemoryGovernedContent, LongTermMemoryKind,
        LongTermMemoryRetainedRevisionDigest, LongTermMemorySourceScope, LongTermMemorySourceType,
        LongTermMemoryStaleHint, LongTermMemoryVersionOrigin,
        LongTermMemoryVersionTransitionBinding, MemoryEvidenceAuthority, MemoryPrivacyClass,
        LONG_TERM_CONTROL_SCHEMA_VERSION,
    };
    use bm_core::skills::{
        RuntimeSkillApplicability, RuntimeSkillCapabilityAffinity, RuntimeSkillCreationRef,
        RuntimeSkillEvidenceBinding, RuntimeSkillEvidenceKind, RuntimeSkillFailureMode,
        RuntimeSkillIntrinsicContract, RuntimeSkillLifecycle, RuntimeSkillPremise,
        RuntimeSkillPremiseRequirement, RuntimeSkillProceduralContent,
        RuntimeSkillProjectionPolicy, RUNTIME_SKILL_GOVERNED_CONTRACT_SCHEMA_VERSION,
    };

    use super::*;
    use crate::store_internal::transaction::{
        StoreBoundedKnownBlobRead, StoreBoundedKnownJsonRead, StoreImmutableReadSession,
    };

    struct CountingSession {
        json_calls: Arc<Mutex<usize>>,
        blob_calls: Arc<Mutex<usize>>,
    }

    struct JsonMapSession {
        json: BTreeMap<(String, String), serde_json::Value>,
    }

    impl StoreImmutableReadSession for JsonMapSession {
        fn read_json_known_keys(
            &mut self,
            addresses: &[(String, String)],
        ) -> Result<Vec<StoreBoundedKnownJsonRead>> {
            Ok(addresses
                .iter()
                .map(|(namespace, key)| StoreBoundedKnownJsonRead {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: self.json.get(&(namespace.clone(), key.clone())).cloned(),
                })
                .collect())
        }

        fn read_blob_known_keys(
            &mut self,
            addresses: &[(String, String)],
        ) -> Result<Vec<StoreBoundedKnownBlobRead>> {
            Ok(addresses
                .iter()
                .map(|(namespace, key)| StoreBoundedKnownBlobRead {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: None,
                })
                .collect())
        }

        fn receipt(&self) -> Result<StoreReadReceipt> {
            Ok(StoreReadReceipt::default())
        }
    }

    fn runtime_skill_digest(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn runtime_skill_owner_with_learning(
        owning_scope: RuntimeSkillOwningScope,
        learning_id: &str,
        digest_byte: char,
    ) -> RuntimeSkillOwnerRecord {
        let learning_digest = runtime_skill_digest(digest_byte);
        RuntimeSkillOwnerRecord::build(
            "space-1",
            owning_scope,
            RuntimeSkillCreationRef::TaskLearningPromotion {
                learning_id: learning_id.into(),
                learning_digest: learning_digest.clone(),
            },
            1,
            RuntimeSkillIntrinsicContract {
                schema_version: RUNTIME_SKILL_GOVERNED_CONTRACT_SCHEMA_VERSION,
                applicability: RuntimeSkillApplicability::Global,
                triggers: Vec::new(),
                constraints: Vec::new(),
                premises: Vec::new(),
                failure_modes: vec![RuntimeSkillFailureMode::RequiredPremiseUnsatisfied],
                evidence_bindings: vec![RuntimeSkillEvidenceBinding {
                    kind: RuntimeSkillEvidenceKind::TaskLearning,
                    safe_ref: learning_id.into(),
                    source_digest: learning_digest,
                }],
                projection_policy: RuntimeSkillProjectionPolicy {
                    privacy_class: MemoryPrivacyClass::SharedWithSubject,
                    model_projection_allowed: true,
                    require_all_mandatory_premises: true,
                },
                capability_affinities: vec![RuntimeSkillCapabilityAffinity::ProceduralRecall],
            },
            RuntimeSkillProceduralContent {
                title: "Deploy safely".into(),
                topic: "deployment".into(),
                summary: "Use a governed release sequence.".into(),
                procedure: "Verify evidence, deploy, then inspect the receipt.".into(),
            },
            RuntimeSkillLifecycle::created(100).expect("created lifecycle"),
            MemoryPrivacyClass::SharedWithSubject,
        )
        .expect("runtime skill owner")
    }

    fn runtime_skill_owner(owning_scope: RuntimeSkillOwningScope) -> RuntimeSkillOwnerRecord {
        runtime_skill_owner_with_learning(owning_scope, "learning-1", 'b')
    }

    fn governed_environment_document(document_id: &str) -> GovernedEvidenceDocument {
        let source_locator = format!("opaque://environment/{document_id}");
        let canonical_evidence_group =
            bm_core::memory::canonical_recall_evidence_group("environment:runtime-skill");
        let body = "governed environment premise".to_string();
        let content_digest = governed_evidence_document_content_digest(
            &source_locator,
            &canonical_evidence_group,
            None,
            &body,
            &[],
        );
        let draft = GovernedEvidenceDocumentDraft {
            memory_space_id: "space-1".into(),
            mounted_subject_id: "subject-1".into(),
            document_id: document_id.into(),
            source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
            source_locator,
            canonical_evidence_group,
            evidence_family_group: None,
            source_revision: 1,
            body,
            chunks: Vec::new(),
            content_digest,
            authority: MemoryEvidenceAuthority::RuntimeObservation,
            privacy: MemoryPrivacyClass::SharedWithSubject,
            observed_at: 90,
        };
        let GovernedEvidenceDocumentPlan::Created(document) =
            plan_governed_evidence_document_upsert(None, &draft, 100)
        else {
            panic!("governed environment document");
        };
        document
    }

    fn runtime_skill_owner_with_governed_evidence(
        owning_scope: RuntimeSkillOwningScope,
        evidence_refs: Vec<GovernedOwnerRevisionRef>,
    ) -> RuntimeSkillOwnerRecord {
        let mut owner = runtime_skill_owner_with_learning(owning_scope.clone(), "learning-1", 'b');
        let evidence_ref = evidence_refs
            .first()
            .cloned()
            .expect("at least one governed evidence ref");
        owner = RuntimeSkillOwnerRecord::build(
            "space-1",
            owning_scope,
            owner.creation_ref,
            1,
            RuntimeSkillIntrinsicContract {
                schema_version: RUNTIME_SKILL_GOVERNED_CONTRACT_SCHEMA_VERSION,
                applicability: RuntimeSkillApplicability::Global,
                triggers: Vec::new(),
                constraints: Vec::new(),
                premises: vec![RuntimeSkillPremiseRequirement {
                    premise: RuntimeSkillPremise::GovernedEnvironmentEvidence {
                        evidence_revision_ref: evidence_ref,
                    },
                    required: true,
                    valid_from: 1,
                    valid_until: None,
                    privacy_class: MemoryPrivacyClass::SharedWithSubject,
                    governed_evidence_refs: evidence_refs,
                }],
                failure_modes: vec![RuntimeSkillFailureMode::RequiredPremiseUnsatisfied],
                evidence_bindings: owner.intrinsic_contract.evidence_bindings,
                projection_policy: owner.intrinsic_contract.projection_policy,
                capability_affinities: vec![RuntimeSkillCapabilityAffinity::EnvironmentPremise],
            },
            owner.procedural_content,
            owner.lifecycle,
            owner.privacy_class,
        )
        .expect("runtime skill owner with governed evidence");
        owner
    }

    fn runtime_skill_scope_documents(
        owning_scope: RuntimeSkillOwningScope,
    ) -> (
        BTreeMap<(String, String), serde_json::Value>,
        RuntimeSkillOwnerRecord,
    ) {
        let owner = runtime_skill_owner(owning_scope.clone());
        let manifest = RuntimeSkillScopeManifest::build(
            1,
            "space-1",
            owning_scope,
            [RuntimeSkillOwnerBinding::from_record(&owner).expect("owner binding")],
            4,
        )
        .expect("scope manifest");
        let documents = BTreeMap::from([
            (
                (
                    crate::store_internal::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                    manifest.physical_key.clone(),
                ),
                serde_json::to_value(manifest).expect("manifest JSON"),
            ),
            (
                (
                    crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_string(),
                    owner.physical_key.clone(),
                ),
                serde_json::to_value(&owner).expect("owner JSON"),
            ),
        ]);
        (documents, owner)
    }

    fn runtime_skill_scope_documents_with_two_owners(
        owning_scope: RuntimeSkillOwningScope,
    ) -> (
        BTreeMap<(String, String), serde_json::Value>,
        Vec<RuntimeSkillOwnerRecord>,
    ) {
        let owners = vec![
            runtime_skill_owner_with_learning(owning_scope.clone(), "learning-1", 'b'),
            runtime_skill_owner_with_learning(owning_scope.clone(), "learning-2", 'c'),
        ];
        let manifest = RuntimeSkillScopeManifest::build(
            1,
            "space-1",
            owning_scope,
            owners
                .iter()
                .map(RuntimeSkillOwnerBinding::from_record)
                .collect::<Result<Vec<_>>>()
                .expect("owner bindings"),
            2,
        )
        .expect("two-owner scope manifest");
        let mut documents = BTreeMap::from([(
            (
                crate::store_internal::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                manifest.physical_key.clone(),
            ),
            serde_json::to_value(manifest).expect("manifest JSON"),
        )]);
        documents.extend(owners.iter().map(|owner| {
            (
                (
                    crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_string(),
                    owner.physical_key.clone(),
                ),
                serde_json::to_value(owner).expect("owner JSON"),
            )
        }));
        (documents, owners)
    }

    fn long_term_owner_ref() -> GovernedMemoryOwnerRef {
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "state-1")
    }

    fn long_term_material(
        owner_revision: u64,
        valid_from: u64,
        predecessor: Option<GovernedOwnerRevisionRef>,
    ) -> LongTermMemoryVersionMaterial {
        let mut material = LongTermMemoryVersionMaterial {
            schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
            memory_space_id: "space-1".into(),
            mounted_subject_id: "subject-1".into(),
            owner_ref: long_term_owner_ref(),
            owner_revision,
            governed_content: LongTermMemoryGovernedContent {
                kind: LongTermMemoryKind::Fact,
                topic: "deployment".into(),
                content: format!("active generation {owner_revision}"),
                keywords: vec!["generation".into()],
                source_chat_id: None,
                source_type: LongTermMemorySourceType::SystemRuntime,
                source_scope: LongTermMemorySourceScope::World,
                confidence: LongTermMemoryConfidence::High,
                freshness: LongTermMemoryFreshness::Dynamic,
                stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
                supporting_citations: Vec::new(),
                canonical_entities: Vec::new(),
                evidence_count: 0,
                created_at: 10,
                updated_at: valid_from,
                observed_at: valid_from,
                last_confirmed_at: valid_from,
                source_revision: Some(owner_revision),
                last_used_at: 0,
            },
            governed_evidence_refs: Vec::new(),
            origin: LongTermMemoryVersionOrigin {
                valid_from,
                observed_at: valid_from,
                predecessor,
            },
            privacy_class: MemoryPrivacyClass::PublicRuntime,
            content_digest: String::new(),
        };
        material.content_digest = material
            .canonical_content_digest()
            .expect("material digest");
        material
    }

    fn long_term_owner_documents() -> (BTreeMap<(String, String), serde_json::Value>, String) {
        let first = long_term_material(1, 10, None);
        let second = long_term_material(2, 20, Some(first.owner_revision_ref()));
        let transition = bm_core::memory::GovernedOwnerTransition {
            predecessor: first.owner_revision_ref(),
            terminated_at: 20,
            termination: GovernedOwnerTermination::Corrected,
            successor: Some(second.owner_revision_ref()),
        };
        let mut revision = LongTermMemoryControlRevision {
            schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
            revision_id: "correct-state-1-r1".into(),
            memory_space_id: "space-1".into(),
            mounted_subject_id: "subject-1".into(),
            operation: LongTermControlOperation::Correct,
            invalidation_reason_code: None,
            transition: transition.clone(),
            predecessor_material_digest: first.content_digest.clone(),
            successor_material_digest: Some(second.content_digest.clone()),
            governed_evidence_refs: Vec::new(),
            reason: "replace stale state".into(),
            actor_subject_id: None,
            created_at: 20,
            content_digest: String::new(),
        };
        revision.content_digest = revision.canonical_content_digest().expect("control digest");
        revision.validate_contract().expect("control contract");
        let control_key = scoped_long_term_control_storage_key(
            "space-1",
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
            &revision.revision_id,
        )
        .expect("control key");
        let transition_binding = LongTermMemoryVersionTransitionBinding::new(
            transition.predecessor.clone(),
            control_key.clone(),
            revision.content_digest.clone(),
        )
        .expect("transition binding");
        let materials = vec![first.clone(), second.clone()];
        let head = LongTermMemoryHeadManifest {
            schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
            memory_space_id: "space-1".into(),
            mounted_subject_id: "subject-1".into(),
            owner_ref: long_term_owner_ref(),
            current_revision: 2,
            retained_revision_digests: materials
                .iter()
                .map(|material| LongTermMemoryRetainedRevisionDigest {
                    owner_revision: material.owner_revision,
                    content_digest: material.content_digest.clone(),
                })
                .collect(),
            terminal_transition_ref: None,
            manifest_revision: 2,
        };
        let root = LongTermMemoryVersionScopeManifest::build(
            "space-1",
            "subject-1",
            2,
            std::slice::from_ref(&head),
            &materials,
            std::slice::from_ref(&transition),
            std::slice::from_ref(&transition_binding),
            4,
        )
        .expect("long-term scope manifest");
        let documents = BTreeMap::from([
            (
                (
                    crate::store_internal::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE.to_string(),
                    root.physical_key.clone(),
                ),
                serde_json::to_value(root).expect("root JSON"),
            ),
            (
                (
                    crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE.to_string(),
                    long_term_version_head_key("space-1", "subject-1", &head.owner_ref)
                        .expect("head key"),
                ),
                serde_json::to_value(head).expect("head JSON"),
            ),
            (
                (
                    crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE.to_string(),
                    long_term_version_material_key(
                        "space-1",
                        "subject-1",
                        &first.owner_ref,
                        first.owner_revision,
                    )
                    .expect("first key"),
                ),
                serde_json::to_value(first).expect("first JSON"),
            ),
            (
                (
                    crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE.to_string(),
                    long_term_version_material_key(
                        "space-1",
                        "subject-1",
                        &second.owner_ref,
                        second.owner_revision,
                    )
                    .expect("second key"),
                ),
                serde_json::to_value(second).expect("second JSON"),
            ),
            (
                (
                    LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
                    control_key.clone(),
                ),
                serde_json::to_value(revision).expect("control JSON"),
            ),
        ]);
        (documents, control_key)
    }

    fn two_owner_long_term_documents() -> BTreeMap<(String, String), serde_json::Value> {
        let (mut documents, _) = long_term_owner_documents();
        let root_key =
            long_term_version_scope_manifest_key("space-1", "subject-1").expect("root key");
        let original_root = documents
            .get(&(
                crate::store_internal::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE.to_string(),
                root_key.clone(),
            ))
            .cloned()
            .and_then(|value| {
                serde_json::from_value::<LongTermMemoryVersionScopeManifest>(value).ok()
            })
            .expect("original root");
        let first_head_key =
            long_term_version_head_key("space-1", "subject-1", &long_term_owner_ref())
                .expect("first head key");
        let first_head = documents
            .get(&(
                crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE.to_string(),
                first_head_key,
            ))
            .cloned()
            .and_then(|value| serde_json::from_value::<LongTermMemoryHeadManifest>(value).ok())
            .expect("first head");
        let mut materials = first_head
            .retained_revision_digests
            .iter()
            .map(|retained| {
                let key = long_term_version_material_key(
                    "space-1",
                    "subject-1",
                    &first_head.owner_ref,
                    retained.owner_revision,
                )
                .expect("first owner material key");
                documents
                    .get(&(
                        crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE.to_string(),
                        key,
                    ))
                    .cloned()
                    .and_then(|value| {
                        serde_json::from_value::<LongTermMemoryVersionMaterial>(value).ok()
                    })
                    .expect("first owner material")
            })
            .collect::<Vec<_>>();
        let transition = original_root
            .transition_bindings
            .first()
            .and_then(|binding| {
                documents
                    .get(&(
                        LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
                        binding.control_revision_physical_key.clone(),
                    ))
                    .cloned()
            })
            .and_then(|value| serde_json::from_value::<LongTermMemoryControlRevision>(value).ok())
            .expect("first owner control revision")
            .transition;

        let second_owner =
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "state-2");
        let mut second_material = long_term_material(1, 15, None);
        second_material.owner_ref = second_owner.clone();
        second_material.governed_content.topic = "secondary deployment".into();
        second_material.governed_content.content = "independent generation".into();
        second_material.content_digest = second_material
            .canonical_content_digest()
            .expect("second material digest");
        let second_head = LongTermMemoryHeadManifest {
            schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
            memory_space_id: "space-1".into(),
            mounted_subject_id: "subject-1".into(),
            owner_ref: second_owner,
            current_revision: 1,
            retained_revision_digests: vec![LongTermMemoryRetainedRevisionDigest {
                owner_revision: 1,
                content_digest: second_material.content_digest.clone(),
            }],
            terminal_transition_ref: None,
            manifest_revision: 1,
        };
        materials.push(second_material.clone());
        let heads = vec![first_head, second_head.clone()];
        let root = LongTermMemoryVersionScopeManifest::build(
            "space-1",
            "subject-1",
            3,
            &heads,
            &materials,
            std::slice::from_ref(&transition),
            &original_root.transition_bindings,
            4,
        )
        .expect("two-owner root");
        documents.insert(
            (
                crate::store_internal::LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE.to_string(),
                root_key,
            ),
            serde_json::to_value(root).expect("two-owner root JSON"),
        );
        documents.insert(
            (
                crate::store_internal::LONG_TERM_HEAD_MANIFEST_NAMESPACE.to_string(),
                long_term_version_head_key("space-1", "subject-1", &second_head.owner_ref)
                    .expect("second head key"),
            ),
            serde_json::to_value(second_head).expect("second head JSON"),
        );
        documents.insert(
            (
                crate::store_internal::LONG_TERM_VERSION_MATERIAL_NAMESPACE.to_string(),
                long_term_version_material_key(
                    "space-1",
                    "subject-1",
                    &second_material.owner_ref,
                    second_material.owner_revision,
                )
                .expect("second material key"),
            ),
            serde_json::to_value(second_material).expect("second material JSON"),
        );
        documents
    }

    impl StoreImmutableReadSession for CountingSession {
        fn read_json_known_keys(
            &mut self,
            addresses: &[(String, String)],
        ) -> Result<Vec<StoreBoundedKnownJsonRead>> {
            *self.json_calls.lock().unwrap() += 1;
            Ok(addresses
                .iter()
                .map(|(namespace, key)| StoreBoundedKnownJsonRead {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: (key == "present").then(|| serde_json::json!({"ok": true})),
                })
                .collect())
        }

        fn read_blob_known_keys(
            &mut self,
            addresses: &[(String, String)],
        ) -> Result<Vec<StoreBoundedKnownBlobRead>> {
            *self.blob_calls.lock().unwrap() += 1;
            Ok(addresses
                .iter()
                .map(|(namespace, key)| StoreBoundedKnownBlobRead {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value: (key == "present").then(|| b"ok".to_vec()),
                })
                .collect())
        }

        fn receipt(&self) -> Result<StoreReadReceipt> {
            Ok(StoreReadReceipt {
                state_digest: "0".repeat(64),
                json_doc_count: 1,
                blob_count: 1,
                event_count: 0,
                entry_count: 4,
                json_bytes: 11,
                blob_bytes: 2,
            })
        }
    }

    #[test]
    fn cached_context_reads_each_present_and_absent_address_once() {
        let json_calls = Arc::new(Mutex::new(0));
        let blob_calls = Arc::new(Mutex::new(0));
        let session = CountingSession {
            json_calls: json_calls.clone(),
            blob_calls: blob_calls.clone(),
        };
        let mut context = RecallImmutableReadContext::new(Box::new(session));

        assert!(context.read_json_value("ns", "present").unwrap().is_some());
        assert!(context.read_json_value("ns", "present").unwrap().is_some());
        assert!(context.read_json_value("ns", "absent").unwrap().is_none());
        assert!(context.read_json_value("ns", "absent").unwrap().is_none());
        assert!(context.read_blob("blob", "present").unwrap().is_some());
        assert!(context.read_blob("blob", "present").unwrap().is_some());
        assert!(context.read_blob("blob", "absent").unwrap().is_none());
        assert!(context.read_blob("blob", "absent").unwrap().is_none());

        assert_eq!(*json_calls.lock().unwrap(), 2);
        assert_eq!(*blob_calls.lock().unwrap(), 2);
        assert_eq!(context.cached_address_counts(), (2, 2));
        let (receipt, evidence) = context.finish().expect("exact context finish");
        assert_eq!(receipt.entry_count, 4);
        assert!(evidence.read_set_exact);
    }

    #[test]
    fn materialized_view_takes_the_cached_read_set_once_without_copying_it() {
        let mut context = RecallImmutableReadContext::new(Box::new(CountingSession {
            json_calls: Arc::new(Mutex::new(0)),
            blob_calls: Arc::new(Mutex::new(0)),
        }));
        context.read_json_value("ns", "present").unwrap();
        context.read_json_value("ns", "absent").unwrap();
        context.read_blob("blob", "present").unwrap();
        context.read_blob("blob", "absent").unwrap();

        let view = context.take_materialized_view();

        assert_eq!(context.cached_address_counts(), (0, 0));
        assert!(view.json_value("ns", "present").is_some());
        assert_eq!(view.blob("blob", "present"), Some(b"ok".as_slice()));
        let (receipt, evidence) = context.finish().expect("exact context finish after view");
        assert_eq!(receipt.entry_count, 4);
        assert!(evidence.read_set_exact);
    }

    #[test]
    fn runtime_skill_subject_and_shared_program_closures_remain_scope_separated_in_the_view() {
        let subject_scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-1".into(),
        };
        let shared_scope = RuntimeSkillOwningScope::SharedProgram;
        let (mut documents, subject_owner) = runtime_skill_scope_documents(subject_scope.clone());
        let (shared_documents, shared_owner) = runtime_skill_scope_documents(shared_scope.clone());
        documents.extend(shared_documents);
        let mut context = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: documents.clone(),
        }));

        context
            .materialize_runtime_skill_scopes("space-1", "subject-1", 1)
            .expect("both exact runtime skill scopes");
        let view = context.take_materialized_view();

        let subject = view
            .runtime_skill_scope("space-1", &subject_scope)
            .expect("materialized subject scope");
        assert_eq!(
            subject.manifest().expect("subject manifest").owning_scope,
            subject_scope
        );
        assert_eq!(subject.records(), [subject_owner]);

        let shared = view
            .runtime_skill_scope("space-1", &shared_scope)
            .expect("materialized shared-program scope");
        assert_eq!(
            shared.manifest().expect("shared manifest").owning_scope,
            shared_scope
        );
        assert_eq!(shared.records(), [shared_owner]);
    }

    #[test]
    fn runtime_skill_absent_manifest_is_an_explicit_empty_scope() {
        let subject_scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-1".into(),
        };
        let shared_scope = RuntimeSkillOwningScope::SharedProgram;
        let (documents, _) = runtime_skill_scope_documents(subject_scope);
        let mut context = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: documents.clone(),
        }));

        context
            .materialize_runtime_skill_scopes("space-1", "subject-1", 1)
            .expect("missing shared manifest is an empty scope");
        assert_eq!(context.cached_address_counts(), (3, 0));
        let view = context.take_materialized_view();
        let shared = view
            .runtime_skill_scope("space-1", &shared_scope)
            .expect("shared scope was materialized");
        assert_eq!(shared.memory_space_id(), "space-1");
        assert_eq!(shared.owning_scope(), &shared_scope);
        assert!(shared.manifest().is_none());
        assert!(shared.records().is_empty());
    }

    #[test]
    fn runtime_skill_governed_premise_evidence_is_an_exact_session_read() {
        let subject_scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-1".into(),
        };
        let evidence_ref = GovernedOwnerRevisionRef {
            owner_ref: GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::EvidenceDocument,
                "environment-1",
            ),
            owner_revision: 1,
        };
        let owner = runtime_skill_owner_with_governed_evidence(
            subject_scope.clone(),
            vec![evidence_ref.clone()],
        );
        let manifest = RuntimeSkillScopeManifest::build(
            1,
            "space-1",
            subject_scope,
            [RuntimeSkillOwnerBinding::from_record(&owner).expect("owner binding")],
            1,
        )
        .expect("scope manifest");
        let documents = BTreeMap::from([
            (
                (
                    crate::store_internal::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                    manifest.physical_key.clone(),
                ),
                serde_json::to_value(manifest).expect("manifest JSON"),
            ),
            (
                (
                    crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_string(),
                    owner.physical_key.clone(),
                ),
                serde_json::to_value(owner).expect("owner JSON"),
            ),
        ]);
        let mut context = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: documents.clone(),
        }));
        context
            .materialize_runtime_skill_scopes("space-1", "subject-1", 1)
            .expect("runtime skill closure");
        context
            .materialize_runtime_skill_premise_evidence("space-1", "subject-1", 1)
            .expect("exact missing evidence read");
        assert_eq!(
            context.cached_address_counts(),
            (4, 0),
            "two manifests, one owner, and exactly one evidence owner address"
        );
        let view = context.take_materialized_view();
        assert_eq!(
            view.runtime_skill_premise_evidence(&evidence_ref),
            Some(false)
        );

        let evidence_document = governed_environment_document("environment-1");
        let mut documents_with_evidence = documents;
        documents_with_evidence.insert(
            (
                crate::store_internal::GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                evidence_document.physical_key.clone(),
            ),
            serde_json::to_value(evidence_document).expect("evidence JSON"),
        );
        let mut present_context = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: documents_with_evidence,
        }));
        present_context
            .materialize_runtime_skill_scopes("space-1", "subject-1", 1)
            .expect("runtime skill closure");
        present_context
            .materialize_runtime_skill_premise_evidence("space-1", "subject-1", 1)
            .expect("exact present evidence read");
        let present_view = present_context.take_materialized_view();
        assert_eq!(
            present_view.runtime_skill_premise_evidence(&evidence_ref),
            Some(true)
        );
    }

    #[test]
    fn runtime_skill_premise_evidence_budget_rejects_n_plus_one_before_evidence_io() {
        let subject_scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-1".into(),
        };
        let evidence_refs = ["environment-1", "environment-2"]
            .into_iter()
            .map(|owner_id| GovernedOwnerRevisionRef {
                owner_ref: GovernedMemoryOwnerRef::new(
                    GovernedMemoryOwnerPlane::EvidenceDocument,
                    owner_id,
                ),
                owner_revision: 1,
            })
            .collect::<Vec<_>>();
        let owner =
            runtime_skill_owner_with_governed_evidence(subject_scope.clone(), evidence_refs);
        let manifest = RuntimeSkillScopeManifest::build(
            1,
            "space-1",
            subject_scope,
            [RuntimeSkillOwnerBinding::from_record(&owner).expect("owner binding")],
            1,
        )
        .expect("scope manifest");
        let documents = BTreeMap::from([
            (
                (
                    crate::store_internal::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                    manifest.physical_key.clone(),
                ),
                serde_json::to_value(manifest).expect("manifest JSON"),
            ),
            (
                (
                    crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_string(),
                    owner.physical_key.clone(),
                ),
                serde_json::to_value(owner).expect("owner JSON"),
            ),
        ]);
        let mut context =
            RecallImmutableReadContext::new(Box::new(JsonMapSession { json: documents }));
        context
            .materialize_runtime_skill_scopes("space-1", "subject-1", 1)
            .expect("runtime skill closure");
        let error = context
            .materialize_runtime_skill_premise_evidence("space-1", "subject-1", 1)
            .expect_err("two evidence refs cannot fit one read slot");
        assert_eq!(error.stage(), "recall_runtime_skill_premise_evidence");
        assert_eq!(
            context.cached_address_counts(),
            (3, 0),
            "evidence owners are not read after the pre-IO budget rejection"
        );
    }

    #[test]
    fn runtime_skill_task_evidence_is_bound_to_typed_source_and_session_scope() {
        let mut context = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: BTreeMap::new(),
        }));
        context
            .bind_runtime_skill_task_evidence(
                PremiseTypedSource::TaskRun,
                "run-1",
                "space-1",
                "channel-1",
                "chat-1",
            )
            .expect("session-bound TaskRun evidence");
        context
            .bind_runtime_skill_task_evidence(
                PremiseTypedSource::TaskLearning,
                "learning-1",
                "space-1",
                "channel-1",
                "chat-1",
            )
            .expect("session-bound TaskLearning evidence");
        let view = context.take_materialized_view();

        assert!(view
            .runtime_skill_task_evidence_present(
                PremiseTypedSource::TaskRun,
                "run-1",
                "space-1",
                "channel-1",
                "chat-1",
            )
            .expect("exact TaskRun presence"));
        assert!(view
            .runtime_skill_task_evidence_present(
                PremiseTypedSource::TaskLearning,
                "learning-1",
                "space-1",
                "channel-1",
                "chat-1",
            )
            .expect("exact TaskLearning presence"));
        for (source, safe_ref, memory_space_id, channel_id, chat_id) in [
            (
                PremiseTypedSource::TaskLearning,
                "run-1",
                "space-1",
                "channel-1",
                "chat-1",
            ),
            (
                PremiseTypedSource::TaskRun,
                "run-1",
                "other-space",
                "channel-1",
                "chat-1",
            ),
            (
                PremiseTypedSource::TaskRun,
                "run-1",
                "space-1",
                "other-channel",
                "chat-1",
            ),
            (
                PremiseTypedSource::TaskRun,
                "run-1",
                "space-1",
                "channel-1",
                "other-chat",
            ),
        ] {
            assert!(!view
                .runtime_skill_task_evidence_present(
                    source,
                    safe_ref,
                    memory_space_id,
                    channel_id,
                    chat_id,
                )
                .expect("mismatched evidence is absent"));
        }
        assert!(view
            .runtime_skill_task_evidence_present(
                PremiseTypedSource::TaskArtifact,
                "artifact-1",
                "space-1",
                "channel-1",
                "chat-1",
            )
            .is_err());
    }

    #[test]
    fn runtime_skill_scope_budget_is_exact_and_rejects_n_plus_one_before_owner_io() {
        let subject_scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-1".into(),
        };
        let (documents, owners) =
            runtime_skill_scope_documents_with_two_owners(subject_scope.clone());
        let mut exact = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: documents.clone(),
        }));
        let records = exact
            .materialize_runtime_skill_scope("space-1", &subject_scope, 2)
            .expect("exact owner budget")
            .expect("subject manifest");
        assert_eq!(records, owners);
        assert_eq!(exact.cached_address_counts(), (3, 0));
        let error = exact
            .materialize_runtime_skill_scope("space-1", &subject_scope, 1)
            .expect_err("a cached scope cannot bypass a tighter request bound");
        assert_eq!(error.stage(), "recall_runtime_skill_scope");

        let mut n_plus_one =
            RecallImmutableReadContext::new(Box::new(JsonMapSession { json: documents }));
        let error = n_plus_one
            .materialize_runtime_skill_scope("space-1", &subject_scope, 1)
            .expect_err("two bound owners cannot fit one slot");
        assert_eq!(error.stage(), "recall_runtime_skill_scope");
        assert_eq!(
            n_plus_one.cached_address_counts(),
            (1, 0),
            "only the exact manifest may be read before owner budget rejection"
        );

        let (zero_documents, _) = runtime_skill_scope_documents(subject_scope.clone());
        let mut zero = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: zero_documents,
        }));
        let error = zero
            .materialize_runtime_skill_scope("space-1", &subject_scope, 0)
            .expect_err("zero owner budget");
        assert_eq!(error.stage(), "recall_runtime_skill_scope");
        assert_eq!(zero.cached_address_counts(), (0, 0));
    }

    #[test]
    fn runtime_skill_manifest_binding_drift_fails_closed() {
        let subject_scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-1".into(),
        };
        let manifest_key =
            runtime_skill_scope_manifest_key("space-1", &subject_scope).expect("manifest key");
        let manifest_address = (
            crate::store_internal::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE.to_string(),
            manifest_key,
        );

        let (mut missing_binding, _) = runtime_skill_scope_documents(subject_scope.clone());
        let mut manifest = missing_binding
            .get(&manifest_address)
            .cloned()
            .and_then(|value| serde_json::from_value::<RuntimeSkillScopeManifest>(value).ok())
            .expect("manifest");
        manifest.owner_bindings.clear();
        manifest.owner_count = 0;
        missing_binding.insert(
            manifest_address.clone(),
            serde_json::to_value(manifest).expect("manifest JSON"),
        );
        RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: missing_binding,
        }))
        .materialize_runtime_skill_scope("space-1", &subject_scope, 2)
        .expect_err("a missing binding must invalidate the manifest");

        let (mut duplicate_binding, _) = runtime_skill_scope_documents(subject_scope.clone());
        let mut manifest = duplicate_binding
            .get(&manifest_address)
            .cloned()
            .and_then(|value| serde_json::from_value::<RuntimeSkillScopeManifest>(value).ok())
            .expect("manifest");
        manifest
            .owner_bindings
            .push(manifest.owner_bindings[0].clone());
        manifest.owner_count = 2;
        duplicate_binding.insert(
            manifest_address.clone(),
            serde_json::to_value(manifest).expect("manifest JSON"),
        );
        RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: duplicate_binding,
        }))
        .materialize_runtime_skill_scope("space-1", &subject_scope, 2)
        .expect_err("a duplicate binding must invalidate the manifest");

        let (mut extra_binding, owners) =
            runtime_skill_scope_documents_with_two_owners(subject_scope.clone());
        extra_binding.remove(&(
            crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_string(),
            owners[1].physical_key.clone(),
        ));
        RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: extra_binding,
        }))
        .materialize_runtime_skill_scope("space-1", &subject_scope, 2)
        .expect_err("an extra binding without its owner must fail closed");

        let (mut cross_scope, subject_owner) = runtime_skill_scope_documents(subject_scope.clone());
        let shared_owner = runtime_skill_owner(RuntimeSkillOwningScope::SharedProgram);
        cross_scope.insert(
            (
                crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_string(),
                subject_owner.physical_key,
            ),
            serde_json::to_value(shared_owner).expect("shared owner JSON"),
        );
        RecallImmutableReadContext::new(Box::new(JsonMapSession { json: cross_scope }))
            .materialize_runtime_skill_scope("space-1", &subject_scope, 2)
            .expect_err("a cross-scope owner must fail closed");
    }

    #[test]
    fn runtime_skill_scope_materialization_is_manifest_first_exact_and_scope_bound() {
        let subject_scope = RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-1".into(),
        };
        let (documents, owner) = runtime_skill_scope_documents(subject_scope.clone());
        let mut context =
            RecallImmutableReadContext::new(Box::new(JsonMapSession { json: documents }));
        let records = context
            .materialize_runtime_skill_scope("space-1", &subject_scope, 4)
            .expect("exact subject closure")
            .expect("subject manifest");
        assert_eq!(records, vec![owner.clone()]);
        assert_eq!(context.cached_address_counts(), (2, 0));

        let (mut missing_documents, _) = runtime_skill_scope_documents(subject_scope.clone());
        missing_documents.remove(&(
            crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_string(),
            owner.physical_key.clone(),
        ));
        let error = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: missing_documents,
        }))
        .materialize_runtime_skill_scope("space-1", &subject_scope, 4)
        .expect_err("a bound owner cannot be absent");
        assert_eq!(error.stage(), "recall_runtime_skill_scope");

        let (mut drifted_documents, _) = runtime_skill_scope_documents(subject_scope.clone());
        drifted_documents
            .get_mut(&(
                crate::store_internal::RUNTIME_SKILL_RECORD_NAMESPACE.to_string(),
                owner.physical_key.clone(),
            ))
            .and_then(serde_json::Value::as_object_mut)
            .expect("owner object")
            .insert(
                "content_digest".into(),
                serde_json::json!(runtime_skill_digest('f')),
            );
        let error = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: drifted_documents,
        }))
        .materialize_runtime_skill_scope("space-1", &subject_scope, 4)
        .expect_err("manifest digest drift must fail closed");
        assert_eq!(error.stage(), "runtime_skill_owner_binding");

        let shared_scope = RuntimeSkillOwningScope::SharedProgram;
        let (shared_documents, _) = runtime_skill_scope_documents(shared_scope);
        let error = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: shared_documents,
        }))
        .materialize_runtime_skill_scope("space-1", &subject_scope, 4)
        .expect("subject root is absent rather than scanned");
        assert!(error.is_none());
    }

    #[test]
    fn long_term_owner_closure_reads_retained_materials_and_control_in_one_session() {
        let (documents, control_key) = long_term_owner_documents();
        let mut context =
            RecallImmutableReadContext::new(Box::new(JsonMapSession { json: documents }));
        let before = context
            .select_long_term_owner_as_of("space-1", "subject-1", &long_term_owner_ref(), 19, 4, 1)
            .expect("as-of predecessor")
            .expect("predecessor projection");
        assert_eq!(before.material.owner_revision, 1);
        assert_eq!(before.validity.valid_until, Some(20));
        let after = context
            .select_long_term_owner_as_of("space-1", "subject-1", &long_term_owner_ref(), 20, 4, 1)
            .expect("cached as-of successor")
            .expect("successor projection");
        assert_eq!(after.material.owner_revision, 2);
        assert_eq!(context.cached_address_counts(), (5, 0));

        let (mut missing_control, _) = long_term_owner_documents();
        missing_control.remove(&(
            LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
            control_key,
        ));
        let error = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: missing_control,
        }))
        .materialize_long_term_owner_closure("space-1", "subject-1", &long_term_owner_ref(), 4, 1)
        .expect_err("a bound control transition cannot be absent");
        assert_eq!(error.stage(), "recall_long_term_owner_closure");
    }

    #[test]
    fn long_term_owner_join_budget_rejects_a_new_owner_before_materialization() {
        let (documents, _) = long_term_owner_documents();
        let mut context =
            RecallImmutableReadContext::new(Box::new(JsonMapSession { json: documents }));
        context
            .materialize_long_term_owner_closure(
                "space-1",
                "subject-1",
                &long_term_owner_ref(),
                4,
                1,
            )
            .expect("first owner closure");
        let error = context
            .materialize_long_term_owner_closure(
                "space-1",
                "subject-1",
                &long_term_owner_ref(),
                4,
                0,
            )
            .expect_err("a cached owner cannot bypass a zero request ceiling");
        assert_eq!(error.stage(), "governed_current_recall");
        let second = GovernedMemoryOwnerRef::new(
            bm_core::memory::GovernedMemoryOwnerPlane::LongTerm,
            "ltm-2",
        );
        let error = context
            .materialize_long_term_owner_closure("space-1", "subject-1", &second, 4, 1)
            .expect_err("second distinct owner must be rejected before IO");
        assert_eq!(error.stage(), "governed_current_recall");
    }

    #[test]
    fn historical_scope_materialization_is_exact_bounded_and_builds_canonical_authorities() {
        let (documents, _) = long_term_owner_documents();
        let mut context =
            RecallImmutableReadContext::new(Box::new(JsonMapSession { json: documents }));
        assert!(context
            .materialize_long_term_historical_scope("space-1", "subject-1", 4, 1, 1)
            .expect("bounded historical scope"));
        assert_eq!(context.cached_address_counts(), (5, 0));

        let view = context.take_materialized_view();
        let authorities = view
            .historical_long_term_authorities(19, 4, 4, 1)
            .expect("historical authorities");
        let authority = authorities
            .get(&long_term_owner_ref())
            .expect("historical owner");
        assert_eq!(authority.projection().material.owner_revision, 1);
        assert_eq!(authority.projection().validity.valid_until, Some(20));
        assert!(authority.lineage_report().complete);
    }

    #[test]
    fn historical_scope_budget_rejects_zero_and_n_plus_one_before_owner_io() {
        let (documents, _) = long_term_owner_documents();
        let mut zero_context =
            RecallImmutableReadContext::new(Box::new(JsonMapSession { json: documents }));
        let error = zero_context
            .materialize_long_term_historical_scope("space-1", "subject-1", 4, 1, 0)
            .expect_err("zero as-of candidate budget");
        assert_eq!(error.stage(), "governed_historical_recall");
        assert_eq!(zero_context.cached_address_counts(), (0, 0));

        let mut n_plus_one_context = RecallImmutableReadContext::new(Box::new(JsonMapSession {
            json: two_owner_long_term_documents(),
        }));
        let error = n_plus_one_context
            .materialize_long_term_historical_scope("space-1", "subject-1", 4, 2, 1)
            .expect_err("two bound owners cannot fit one as-of candidate slot");
        assert_eq!(error.stage(), "governed_historical_recall");
        assert_eq!(
            n_plus_one_context.cached_address_counts(),
            (1, 0),
            "only the exact scope root may be read before owner budget rejection"
        );
    }
}
