use std::collections::{BTreeMap, BTreeSet};

use bm_core::budget::RuntimeBudgetReport;
use bm_core::memory::{
    validate_relationship_source_post_image, CoreRevisionLedger, MemoryMutationAuditRecord,
    MemoryMutationReceipt, RelationshipSourceConstitutionV1, RelationshipSourceControlOutcomeV1,
    RelationshipSourceControlPlanV1, RelationshipSourceControlReportV1,
    RelationshipSourceExpectedStateV1, RelationshipSourceScopeManifestV1, SelfAuthoredCore,
    SubjectSoulContractError, SubjectSoulExpectedStateV1, SubjectSoulGenerationTombstoneV1,
    SubjectSoulLifecycleErrorKey, SubjectSoulLifecycleHeadV1, SubjectSoulLifecycleStateV1,
    SubjectSoulMutationOutcomeV1, SubjectSoulMutationReportV1, SubjectSoulOwnedDocumentV1,
    SubjectSoulReadOutcomeV1, SubjectSoulReadRequestV1, SubjectSoulReadSelectorV1,
    SubjectSoulReadViewV1, SubjectSoulRelationshipProjectionPlanV1,
    SubjectSoulRelationshipProjectionV1, SubjectSoulRelationshipRuntimeInputV1,
    SubjectSoulRevisionMaterialV1, SubjectSoulScopeManifestV1, SubjectSoulTerminatedGenerationV1,
    SubjectSoulVerifiedSnapshotV1, VerifiedSubjectSoulReadViewV1,
};
use bm_core::{Error, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::schema::{
    admit_store_json_document, canonical_mor_intent_digest_from_core_digest,
    is_relationship_source_protected_json_namespace, is_subject_global_soul_json_namespace,
    is_subject_soul_protected_json_namespace, relationship_source_revision_key,
    subject_soul_generation_tombstone_key, subject_soul_protected_json_namespaces,
    subject_soul_relationship_projection_key, subject_soul_revision_material_key,
    subject_soul_scope_key, RelationshipSourceDurableOperationResultV1,
    SubjectSoulDurableOperationResultV1, RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE,
    RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE, RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE,
    SUBJECT_SOUL_GENERATION_TOMBSTONE_NAMESPACE, SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE,
    SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE, SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE,
    SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE, SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
};
use super::transaction::{BackendTransactionState, StoreImmutableReadSession};
use super::{
    canonical_subject_soul_full_intent_digest, StoreBlobPrecondition, StoreCapacityBudget,
    StoreEngine, StoreJsonPrecondition, StoreMutation, StoreMutationBatch,
    StoreMutationOperationOutcome, StoreMutationOperationPlan, StorePlatform, StoreReadReceipt,
    StoreSnapshot,
};

#[derive(Clone, Debug)]
pub(crate) struct SubjectSoulStoreMutationAuthority(());

impl SubjectSoulStoreMutationAuthority {
    fn issue() -> Self {
        Self(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubjectSoulStoreFailureStage {
    Contract,
    ExpectedState,
    RepairRequired,
    Capacity,
    Commit,
}

impl SubjectSoulStoreFailureStage {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Contract => "subject_soul_store_contract",
            Self::ExpectedState => "subject_soul_store_expected_state",
            Self::RepairRequired => "subject_soul_store_repair_required",
            Self::Capacity => "subject_soul_store_capacity",
            Self::Commit => "subject_soul_store_commit",
        }
    }
}

impl std::fmt::Display for SubjectSoulStoreFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.stage.as_str(), self.detail)
    }
}

#[derive(Debug)]
pub(crate) struct SubjectSoulStoreFailure {
    stage: SubjectSoulStoreFailureStage,
    lifecycle_error_key: SubjectSoulLifecycleErrorKey,
    detail: String,
}

impl SubjectSoulStoreFailure {
    fn contract(error: SubjectSoulContractError) -> Self {
        Self {
            stage: SubjectSoulStoreFailureStage::Contract,
            lifecycle_error_key: error.key,
            detail: error.reason,
        }
    }

    fn repair(detail: impl Into<String>) -> Self {
        Self {
            stage: SubjectSoulStoreFailureStage::RepairRequired,
            lifecycle_error_key: SubjectSoulLifecycleErrorKey::RepairRequired,
            detail: detail.into(),
        }
    }

    pub(crate) fn from_store(error: Error) -> Self {
        let source_stage = error.stage();
        let (stage, lifecycle_error_key) = if error.class() == Some(bm_core::ErrorClass::Conflict)
            && !source_stage.contains("precondition")
        {
            (
                SubjectSoulStoreFailureStage::ExpectedState,
                SubjectSoulLifecycleErrorKey::OperationConflict,
            )
        } else if source_stage.contains("precondition") {
            (
                SubjectSoulStoreFailureStage::ExpectedState,
                SubjectSoulLifecycleErrorKey::GenerationConflict,
            )
        } else if source_stage.contains("budget") || source_stage.contains("capacity") {
            (
                SubjectSoulStoreFailureStage::Capacity,
                SubjectSoulLifecycleErrorKey::CapacityExceeded,
            )
        } else if source_stage.contains("repair") || source_stage.contains("closure") {
            (
                SubjectSoulStoreFailureStage::RepairRequired,
                SubjectSoulLifecycleErrorKey::RepairRequired,
            )
        } else {
            (
                SubjectSoulStoreFailureStage::Commit,
                SubjectSoulLifecycleErrorKey::RepairRequired,
            )
        };
        Self {
            stage,
            lifecycle_error_key,
            detail: error.to_string(),
        }
    }

    pub(crate) const fn stage(&self) -> SubjectSoulStoreFailureStage {
        self.stage
    }

    pub(crate) const fn lifecycle_error_key(&self) -> SubjectSoulLifecycleErrorKey {
        self.lifecycle_error_key
    }

    pub(crate) fn into_store_error(self) -> Error {
        Error::config(self.stage.as_str(), self.detail)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SubjectSoulStorePostImage {
    pub(crate) head: SubjectSoulLifecycleHeadV1,
    pub(crate) manifest: SubjectSoulScopeManifestV1,
    pub(crate) current_material: Option<SubjectSoulRevisionMaterialV1>,
    pub(crate) current_core: Option<SelfAuthoredCore>,
    pub(crate) current_core_document: Option<SubjectSoulOwnedDocumentV1>,
    pub(crate) current_ledger: Option<CoreRevisionLedger>,
    pub(crate) current_ledger_document: Option<SubjectSoulOwnedDocumentV1>,
}

impl SubjectSoulStorePostImage {
    fn validate(&self) -> std::result::Result<(), SubjectSoulStoreFailure> {
        SubjectSoulVerifiedSnapshotV1 {
            head: self.head.clone(),
            manifest: self.manifest.clone(),
            current_material: self.current_material.clone(),
            current_core: self.current_core.clone(),
            current_core_document: self.current_core_document.clone(),
            current_revision_ledger: self.current_ledger.clone(),
            current_revision_ledger_document: self.current_ledger_document.clone(),
        }
        .validate_contract()
        .map_err(SubjectSoulStoreFailure::contract)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SubjectSoulStoreMutationPlan {
    expected_state: SubjectSoulExpectedStateV1,
    post_image: SubjectSoulStorePostImage,
    batch: StoreMutationBatch,
    preconditions: Vec<StoreJsonPrecondition>,
    blob_preconditions: Vec<StoreBlobPrecondition>,
    operation: StoreMutationOperationPlan,
    durable_result: SubjectSoulDurableOperationResultV1,
    core_intent_digest: String,
    full_intent_digest: String,
    composite_binding_verified: bool,
}

impl SubjectSoulStoreMutationPlan {
    pub(crate) fn new(
        core_intent_digest: &str,
        full_intent_digest: &str,
        expected_state: SubjectSoulExpectedStateV1,
        post_image: SubjectSoulStorePostImage,
        mut batch: StoreMutationBatch,
        mut preconditions: Vec<StoreJsonPrecondition>,
        operation: StoreMutationOperationPlan,
    ) -> std::result::Result<Self, SubjectSoulStoreFailure> {
        let operation =
            operation.authorize_subject_soul(SubjectSoulStoreMutationAuthority::issue());
        let mor_intent_digest = canonical_mor_intent_digest_from_core_digest(full_intent_digest)
            .map_err(SubjectSoulStoreFailure::from_store)?;
        expected_state
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::contract)?;
        post_image.validate()?;
        let state_before = match &expected_state {
            SubjectSoulExpectedStateV1::PristineAbsent { .. } => {
                SubjectSoulLifecycleStateV1::Unseeded
            }
            SubjectSoulExpectedStateV1::Exact {
                lifecycle_state, ..
            } => *lifecycle_state,
        };
        let safe_event_refs = batch
            .mutations
            .iter()
            .filter_map(|mutation| match mutation {
                StoreMutation::AppendEvent { event } => Some(event.event_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let [safe_event_ref] = safe_event_refs.as_slice() else {
            return Err(SubjectSoulStoreFailure::repair(
                "Subject Soul mutation requires exactly one safe lifecycle event",
            ));
        };
        let receipt_key = operation.identity().storage_key();
        let committed_report = SubjectSoulMutationReportV1 {
            outcome: SubjectSoulMutationOutcomeV1::Committed,
            state_before,
            state_after: post_image.head.state,
            generation: post_image.head.generation,
            revision: post_image.head.current_revision,
            head_digest: Some(post_image.head.head_digest.clone()),
            transaction_id: Some(operation.transaction_id().to_string()),
            durable_receipt_ref: Some(receipt_key.clone()),
            replayed: false,
            safe_event_ref: Some(safe_event_ref.clone()),
        };
        let durable_result = SubjectSoulDurableOperationResultV1::new(
            operation.identity().clone(),
            post_image.head.soul_id.clone(),
            committed_report,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        preconditions.push(StoreJsonPrecondition::Absent {
            namespace: SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE.to_string(),
            key: receipt_key.clone(),
        });
        batch.mutations.push(StoreMutation::PutJson {
            namespace: SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE.to_string(),
            key: receipt_key.clone(),
            value: serde_json::to_value(&durable_result).map_err(|error| {
                SubjectSoulStoreFailure::repair(format!(
                    "cannot encode durable Subject Soul result: {error}"
                ))
            })?,
            event_kind: super::MemoryStoreEventKind::MemoryControl,
            plane: SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE.to_string(),
            record_key: receipt_key,
        });
        let plan = Self {
            expected_state,
            post_image,
            batch,
            preconditions,
            blob_preconditions: Vec::new(),
            operation,
            durable_result,
            core_intent_digest: core_intent_digest.to_string(),
            full_intent_digest: full_intent_digest.to_string(),
            composite_binding_verified: core_intent_digest == full_intent_digest,
        };
        if plan.operation.intent_digest() != mor_intent_digest {
            return Err(SubjectSoulStoreFailure::repair(
                "Subject Soul operation MOR intent is not bound to the full owner plan digest",
            ));
        }
        plan.validate_binding()?;
        Ok(plan)
    }

    fn validate_binding(&self) -> std::result::Result<(), SubjectSoulStoreFailure> {
        let head_key = subject_soul_scope_key(
            &self.post_image.head.memory_space_id,
            &self.post_image.head.subject_id,
            &self.post_image.head.soul_id,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        if self.batch.scope.memory_space_id != self.post_image.head.memory_space_id
            || self.batch.scope.subject_id != self.post_image.head.subject_id
            || self.operation.identity().memory_space_id() != self.post_image.head.memory_space_id
            || self.operation.identity().mounted_subject_id() != self.post_image.head.subject_id
        {
            return Err(SubjectSoulStoreFailure::repair(
                "batch scope is not the exact Subject Soul post-image owner",
            ));
        }
        let head_value = mutation_put_value(
            &self.batch,
            SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE,
            &head_key,
        )?;
        let manifest_value = mutation_put_value(
            &self.batch,
            SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
            &head_key,
        )?;
        if head_value
            != serde_json::to_value(&self.post_image.head).map_err(|error| {
                SubjectSoulStoreFailure::repair(format!("cannot encode typed head: {error}"))
            })?
            || manifest_value
                != serde_json::to_value(&self.post_image.manifest).map_err(|error| {
                    SubjectSoulStoreFailure::repair(format!(
                        "cannot encode typed manifest: {error}"
                    ))
                })?
        {
            return Err(SubjectSoulStoreFailure::repair(
                "typed plan head/manifest differs from the batch post-image",
            ));
        }
        validate_expected_preconditions(&self.expected_state, &head_key, &self.preconditions)
    }

    pub(super) fn batch(&self) -> &StoreMutationBatch {
        &self.batch
    }

    pub(super) fn preconditions(&self) -> &[StoreJsonPrecondition] {
        &self.preconditions
    }

    pub(crate) fn bind_additional_owner_plan(
        mut self,
        mutations: &[StoreMutation],
        preconditions: &[StoreJsonPrecondition],
        blob_preconditions: &[StoreBlobPrecondition],
    ) -> std::result::Result<Self, SubjectSoulStoreFailure> {
        let protected = mutations.iter().any(|mutation| {
            matches!(mutation,
                StoreMutation::PutJson { namespace, .. }
                    | StoreMutation::DeleteJson { namespace, .. }
                    if is_subject_soul_protected_json_namespace(namespace)
                        || is_relationship_source_protected_json_namespace(namespace))
        }) || preconditions.iter().any(|precondition| {
            let namespace = match precondition {
                StoreJsonPrecondition::Absent { namespace, .. }
                | StoreJsonPrecondition::Exact { namespace, .. } => namespace,
            };
            is_subject_soul_protected_json_namespace(namespace)
                || is_relationship_source_protected_json_namespace(namespace)
        });
        if protected {
            return Err(SubjectSoulStoreFailure::repair(
                "additional owner plan must not contain protected Soul/Relationship addresses",
            ));
        }
        let expected = canonical_subject_soul_full_intent_digest(
            &self.core_intent_digest,
            mutations,
            preconditions,
            blob_preconditions,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        if expected != self.full_intent_digest {
            return Err(SubjectSoulStoreFailure::repair(
                "Subject Soul operation full intent does not bind every additional owner effect",
            ));
        }
        self.blob_preconditions = blob_preconditions.to_vec();
        self.composite_binding_verified = true;
        Ok(self)
    }

    pub(super) fn append_governance_blob_preconditions(
        mut self,
        incoming: Vec<StoreBlobPrecondition>,
    ) -> std::result::Result<Self, SubjectSoulStoreFailure> {
        for precondition in incoming {
            let (namespace, key) = match &precondition {
                StoreBlobPrecondition::Absent { namespace, key }
                | StoreBlobPrecondition::ExactDigest { namespace, key, .. } => (namespace, key),
            };
            if let Some(existing) = self.blob_preconditions.iter().find(|candidate| {
                let (candidate_namespace, candidate_key) = match candidate {
                    StoreBlobPrecondition::Absent { namespace, key }
                    | StoreBlobPrecondition::ExactDigest { namespace, key, .. } => (namespace, key),
                };
                candidate_namespace == namespace && candidate_key == key
            }) {
                if existing != &precondition {
                    return Err(SubjectSoulStoreFailure::repair(
                        "governance completion carries a conflicting blob precondition",
                    ));
                }
            } else {
                self.blob_preconditions.push(precondition);
            }
        }
        Ok(self)
    }

    pub(super) fn bind_governance_completion(
        mut self,
        batch: StoreMutationBatch,
        preconditions: Vec<StoreJsonPrecondition>,
    ) -> std::result::Result<Self, SubjectSoulStoreFailure> {
        if batch.scope != self.batch.scope || batch.transaction_id.trim().is_empty() {
            return Err(SubjectSoulStoreFailure::repair(
                "governance completion scope/transaction differs from the typed Soul owner",
            ));
        }
        for original in &self.batch.mutations {
            let is_result = matches!(original,
                StoreMutation::PutJson { namespace, .. }
                    if namespace == SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE);
            if !is_result && !batch.mutations.contains(original) {
                return Err(SubjectSoulStoreFailure::repair(
                    "governance completion dropped or rewrote a typed Soul mutation",
                ));
            }
        }
        for original in &self.preconditions {
            let is_result = matches!(original,
                StoreJsonPrecondition::Absent { namespace, .. }
                    if namespace == SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE);
            if !is_result && !preconditions.contains(original) {
                return Err(SubjectSoulStoreFailure::repair(
                    "governance completion dropped or rewrote a typed Soul precondition",
                ));
            }
        }

        let transaction_id = batch.transaction_id.clone();
        self.operation = self.operation.bind_subject_soul_transaction(
            transaction_id.clone(),
            SubjectSoulStoreMutationAuthority::issue(),
        );
        self.durable_result.committed_report.transaction_id = Some(transaction_id);
        self.durable_result = SubjectSoulDurableOperationResultV1::new(
            self.durable_result.identity.clone(),
            self.durable_result.soul_id.clone(),
            self.durable_result.committed_report.clone(),
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        let result_key = self.durable_result.identity.storage_key();
        let mut batch = batch;
        let mut result_values = batch
            .mutations
            .iter_mut()
            .filter_map(|mutation| match mutation {
                StoreMutation::PutJson {
                    namespace,
                    key,
                    value,
                    ..
                } if namespace == SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE && key == &result_key => {
                    Some(value)
                }
                _ => None,
            });
        let Some(result_value) = result_values.next() else {
            return Err(SubjectSoulStoreFailure::repair(
                "governance completion requires one exact durable Soul result mutation",
            ));
        };
        if result_values.next().is_some() {
            return Err(SubjectSoulStoreFailure::repair(
                "governance completion contains duplicate durable Soul result mutations",
            ));
        }
        *result_value = serde_json::to_value(&self.durable_result).map_err(|error| {
            SubjectSoulStoreFailure::repair(format!(
                "cannot encode governance-bound durable Soul result: {error}"
            ))
        })?;
        self.batch = batch;
        self.preconditions = preconditions;
        self.validate_binding()?;
        Ok(self)
    }
}

fn mutation_put_value(
    batch: &StoreMutationBatch,
    expected_namespace: &str,
    expected_key: &str,
) -> std::result::Result<Value, SubjectSoulStoreFailure> {
    let values = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } if namespace == expected_namespace && key == expected_key => Some(value.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    match values.as_slice() {
        [value] => Ok(value.clone()),
        _ => Err(SubjectSoulStoreFailure::repair(format!(
            "typed plan requires one exact {expected_namespace}/{expected_key} PutJson"
        ))),
    }
}

fn validate_expected_preconditions(
    expected: &SubjectSoulExpectedStateV1,
    scope_key: &str,
    preconditions: &[StoreJsonPrecondition],
) -> std::result::Result<(), SubjectSoulStoreFailure> {
    let head = find_precondition(
        preconditions,
        SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE,
        scope_key,
    );
    let manifest = find_precondition(
        preconditions,
        SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
        scope_key,
    );
    match expected {
        SubjectSoulExpectedStateV1::PristineAbsent { .. } => {
            if !matches!(head, Some(StoreJsonPrecondition::Absent { .. }))
                || !matches!(manifest, Some(StoreJsonPrecondition::Absent { .. }))
            {
                return Err(SubjectSoulStoreFailure {
                    stage: SubjectSoulStoreFailureStage::ExpectedState,
                    lifecycle_error_key: SubjectSoulLifecycleErrorKey::GenerationConflict,
                    detail: "pristine Subject Soul requires Absent head and manifest CAS"
                        .to_string(),
                });
            }
        }
        SubjectSoulExpectedStateV1::Exact {
            generation,
            revision,
            lifecycle_state,
            head_digest,
            manifest_digest,
        } => {
            let Some(StoreJsonPrecondition::Exact {
                value: head_value, ..
            }) = head
            else {
                return Err(expected_state_failure(
                    "exact Subject Soul head CAS is missing",
                ));
            };
            let Some(StoreJsonPrecondition::Exact {
                value: manifest_value,
                ..
            }) = manifest
            else {
                return Err(expected_state_failure(
                    "exact Subject Soul manifest CAS is missing",
                ));
            };
            let observed_head: SubjectSoulLifecycleHeadV1 =
                decode_value(head_value).map_err(|e| {
                    SubjectSoulStoreFailure::repair(format!("invalid expected head: {e}"))
                })?;
            let observed_manifest: SubjectSoulScopeManifestV1 = decode_value(manifest_value)
                .map_err(|e| {
                    SubjectSoulStoreFailure::repair(format!("invalid expected manifest: {e}"))
                })?;
            if &observed_head.generation != generation
                || &observed_head.current_revision != revision
                || &observed_head.state != lifecycle_state
                || &observed_head.head_digest != head_digest
                || &observed_manifest.closure_digest != manifest_digest
            {
                return Err(expected_state_failure(
                    "typed expected state differs from exact head/manifest CAS",
                ));
            }
        }
    }
    Ok(())
}

fn expected_state_failure(detail: impl Into<String>) -> SubjectSoulStoreFailure {
    SubjectSoulStoreFailure {
        stage: SubjectSoulStoreFailureStage::ExpectedState,
        lifecycle_error_key: SubjectSoulLifecycleErrorKey::GenerationConflict,
        detail: detail.into(),
    }
}

fn find_precondition<'a>(
    preconditions: &'a [StoreJsonPrecondition],
    namespace: &str,
    key: &str,
) -> Option<&'a StoreJsonPrecondition> {
    preconditions
        .iter()
        .find(|precondition| match precondition {
            StoreJsonPrecondition::Absent {
                namespace: candidate_namespace,
                key: candidate_key,
            }
            | StoreJsonPrecondition::Exact {
                namespace: candidate_namespace,
                key: candidate_key,
                ..
            } => candidate_namespace == namespace && candidate_key == key,
        })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoreOpenClosureCertificate {
    digest: String,
    owners_with_artifacts: BTreeSet<(String, String, String)>,
    relationships_with_artifacts: BTreeSet<(String, String)>,
}

impl StoreOpenClosureCertificate {
    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }

    pub(crate) fn proves_zero_artifacts(
        &self,
        memory_space_id: &str,
        subject_id: &str,
        soul_id: &str,
    ) -> bool {
        !self.owners_with_artifacts.contains(&(
            memory_space_id.to_string(),
            subject_id.to_string(),
            soul_id.to_string(),
        ))
    }

    pub(crate) fn proves_zero_relationship_artifacts(
        &self,
        memory_space_id: &str,
        relationship_id: &str,
    ) -> bool {
        !self
            .relationships_with_artifacts
            .contains(&(memory_space_id.to_string(), relationship_id.to_string()))
    }
}

pub(crate) fn validate_subject_soul_open_snapshot(
    snapshot: &StoreSnapshot,
) -> Result<StoreOpenClosureCertificate> {
    let json = snapshot
        .json_docs
        .iter()
        .map(|document| {
            (
                (document.namespace.clone(), document.key.clone()),
                document.value.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    validate_subject_soul_closure_map(&json)
}

pub(crate) fn build_subject_soul_open_certificate(
    engine: &dyn StoreEngine,
    capacity: StoreCapacityBudget,
) -> Result<StoreOpenClosureCertificate> {
    let mut json = BTreeMap::new();
    for namespace in subject_soul_protected_json_namespaces() {
        for key in engine.list_json_keys(namespace)? {
            if json.len() >= capacity.kv_max_entries {
                return Err(Error::config(
                    "store_subject_soul_open_closure",
                    "protected Subject Soul closure exceeds bounded store capacity",
                ));
            }
            let value = engine.get_json_value(namespace, &key)?.ok_or_else(|| {
                Error::config(
                    "store_subject_soul_open_closure",
                    format!("listed protected address disappeared: {namespace}/{key}"),
                )
            })?;
            json.insert((namespace.to_string(), key), value);
        }
    }
    let result_keys = json
        .keys()
        .filter_map(|(namespace, key)| {
            matches!(
                namespace.as_str(),
                SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE
                    | RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE
            )
            .then_some(key.clone())
        })
        .collect::<Vec<_>>();
    for key in result_keys {
        for namespace in [
            bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE,
            bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE,
        ] {
            let address = (namespace.to_string(), key.clone());
            if !json.contains_key(&address) && json.len() >= capacity.kv_max_entries {
                return Err(Error::config(
                    "store_subject_soul_open_closure",
                    "protected Subject Soul closure plus receipt/audit lineage exceeds bounded store capacity",
                ));
            }
            let value = engine.get_json_value(namespace, &key)?.ok_or_else(|| {
                closure_error("durable Soul result is missing receipt/audit lineage")
            })?;
            json.insert(address, value);
        }
    }
    validate_subject_soul_closure_map(&json)
}

pub(crate) fn validate_subject_soul_transaction_post_image(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    operation_capacity: StoreCapacityBudget,
) -> Result<()> {
    let touches_subject_soul = batch.mutations.iter().any(|mutation| match mutation {
        StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. } => {
            is_subject_soul_protected_json_namespace(namespace)
                || is_relationship_source_protected_json_namespace(namespace)
        }
        _ => false,
    });
    if !touches_subject_soul {
        return Ok(());
    }
    if after.json.len() > operation_capacity.kv_max_entries {
        return Err(Error::config(
            "subject_soul_store_capacity",
            "Subject Soul post-image read set exceeds operation capacity",
        ));
    }
    validate_destructive_known_key_closure(batch, before, after)?;
    validate_relationship_projection_refresh_closure(batch, before, after)?;
    validate_subject_soul_closure_map(&after.json).map(|_| ())
}

fn validate_relationship_projection_refresh_closure(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<()> {
    let changed_relationship_manifests = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE => {
                Some(decode_value::<RelationshipSourceScopeManifestV1>(value))
            }
            _ => None,
        })
        .collect::<Result<Vec<_>>>()?;
    for manifest in changed_relationship_manifests {
        let prior_manifest_key = super::schema::relationship_source_scope_key(
            &manifest.memory_space_id,
            &manifest.relationship_id,
        )?;
        let prior_manifest = before
            .json
            .get(&(
                RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                prior_manifest_key,
            ))
            .map(decode_value::<RelationshipSourceScopeManifestV1>)
            .transpose()?;
        if prior_manifest.as_ref().is_some_and(|prior| {
            prior.current_revision == manifest.current_revision
                && prior.current_digest == manifest.current_digest
                && prior.closure_digest == manifest.closure_digest
        }) {
            continue;
        }
        let source_key = relationship_source_revision_key(
            &manifest.memory_space_id,
            &manifest.relationship_id,
            manifest.current_revision,
        )?;
        let source: RelationshipSourceConstitutionV1 = decode_value(
            after
                .json
                .get(&(
                    RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                    source_key,
                ))
                .ok_or_else(|| {
                    closure_error("changed relationship manifest has no exact current source")
                })?,
        )?;
        let active_heads = after
            .json
            .iter()
            .filter(|((namespace, _), _)| {
                namespace.as_str() == SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE
            })
            .map(|(_, value)| decode_value::<SubjectSoulLifecycleHeadV1>(value))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|head| {
                head.memory_space_id == source.memory_space_id
                    && head.subject_id == source.mounted_subject_id
                    && head.state == SubjectSoulLifecycleStateV1::Active
            })
            .collect::<Vec<_>>();
        let projections = after
            .json
            .iter()
            .filter(|((namespace, _), _)| {
                namespace.as_str() == SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE
            })
            .map(|((_, key), value)| {
                decode_value::<SubjectSoulRelationshipProjectionV1>(value)
                    .map(|projection| (key.clone(), projection))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|(_, projection)| {
                projection.memory_space_id == source.memory_space_id
                    && projection.relationship_id == source.relationship_id
            })
            .collect::<Vec<_>>();

        if source.state == bm_core::memory::RelationshipSourceStateV1::Active {
            for head in active_heads {
                let matches = projections
                    .iter()
                    .filter(|(_, projection)| {
                        projection.subject_id == head.subject_id
                            && projection.soul_id == head.soul_id
                            && projection.generation == head.generation
                            && projection.soul_revision == head.current_revision.unwrap_or_default()
                            && projection.relationship_source_revision == source.revision
                            && projection.relationship_source_digest == source.content_digest
                    })
                    .count();
                if matches != 1 {
                    return Err(closure_error(
                        "active Soul and changed relationship source require one exact refreshed projection",
                    ));
                }
            }
        } else if !projections.is_empty() {
            return Err(closure_error(
                "archived/terminated relationship source cannot retain an active Soul projection",
            ));
        }
    }
    Ok(())
}

fn validate_destructive_known_key_closure(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
) -> Result<()> {
    let deleted = batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::DeleteJson { namespace, key, .. }
                if is_subject_soul_protected_json_namespace(namespace)
                    || is_relationship_source_protected_json_namespace(namespace) =>
            {
                Some((namespace.clone(), key.clone()))
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if deleted.is_empty() {
        return validate_destructive_raw_purge(batch, before, after, &deleted);
    }
    if deleted.iter().any(|(namespace, _)| {
        matches!(
            namespace.as_str(),
            SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE
                | SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE
                | SUBJECT_SOUL_GENERATION_TOMBSTONE_NAMESPACE
                | SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE
                | RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE
                | RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE
                | RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE
        )
    }) {
        return Err(Error::config(
            "subject_soul_destructive_known_key_closure",
            "destructive lifecycle must retain lifecycle roots, generation tombstones, durable operation results, and Relationship Source roots",
        ));
    }
    let mut known = BTreeSet::new();
    for ((namespace, _), value) in &before.json {
        if namespace == SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE {
            let manifest: SubjectSoulScopeManifestV1 = decode_value(value)?;
            known.extend(
                manifest
                    .entries
                    .into_iter()
                    .map(|entry| (entry.namespace, entry.physical_key)),
            );
        } else if namespace == SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE {
            let head: SubjectSoulLifecycleHeadV1 = decode_value(value)?;
            known.extend(
                head.retained_revision_refs
                    .into_iter()
                    .map(|key| (SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE.to_string(), key)),
            );
            known.extend(
                head.retained_tombstone_refs
                    .into_iter()
                    .map(|key| (SUBJECT_SOUL_GENERATION_TOMBSTONE_NAMESPACE.to_string(), key)),
            );
        }
    }
    if !deleted.is_subset(&known) {
        return Err(Error::config(
            "subject_soul_destructive_known_key_closure",
            "destructive mutation contains an address outside the exact prior manifest closure",
        ));
    }
    validate_destructive_raw_purge(batch, before, after, &deleted)
}

fn validate_destructive_raw_purge(
    batch: &StoreMutationBatch,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    deleted: &BTreeSet<(String, String)>,
) -> Result<()> {
    for (scope_key, post_head) in batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } if namespace == SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE => {
                Some((key, decode_value::<SubjectSoulLifecycleHeadV1>(value)))
            }
            _ => None,
        })
    {
        let post_head = post_head?;
        let Some(prior_head_value) = before.json.get(&(
            SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.to_string(),
            scope_key.clone(),
        )) else {
            continue;
        };
        let prior_head: SubjectSoulLifecycleHeadV1 = decode_value(prior_head_value)?;
        let is_terminal_generation_change = (post_head.generation
            == prior_head.generation.saturating_add(1)
            && matches!(
                post_head.state,
                SubjectSoulLifecycleStateV1::Unseeded | SubjectSoulLifecycleStateV1::Active
            ))
            || (post_head.generation == prior_head.generation
                && post_head.state == SubjectSoulLifecycleStateV1::Deleted);
        if !is_terminal_generation_change {
            continue;
        }
        if !post_head.retained_revision_refs.is_empty() {
            return Err(closure_error(
                "reset/reseed/delete cannot retain raw revision material references",
            ));
        }
        let prior_manifest: SubjectSoulScopeManifestV1 = decode_value(
            before
                .json
                .get(&(
                    SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                    scope_key.clone(),
                ))
                .ok_or_else(|| closure_error("destructive purge is missing prior Soul manifest"))?,
        )?;
        let mut required_deleted = prior_manifest
            .entries
            .iter()
            .filter(|entry| entry.namespace != SUBJECT_SOUL_GENERATION_TOMBSTONE_NAMESPACE)
            .map(|entry| (entry.namespace.clone(), entry.physical_key.clone()))
            .collect::<BTreeSet<_>>();
        required_deleted.extend(prior_head.retained_revision_refs.iter().map(|key| {
            (
                SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE.to_string(),
                key.clone(),
            )
        }));
        let replaced = batch
            .mutations
            .iter()
            .filter_map(|mutation| match mutation {
                StoreMutation::PutJson { namespace, key, .. } => {
                    Some((namespace.clone(), key.clone()))
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let post_manifest: SubjectSoulScopeManifestV1 = decode_value(
            after
                .json
                .get(&(
                    SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                    scope_key.clone(),
                ))
                .ok_or_else(|| closure_error("destructive purge is missing post Soul manifest"))?,
        )?;
        let complete_purge = required_deleted.iter().all(|address| {
            if deleted.contains(address) {
                return !after.json.contains_key(address);
            }
            replaced.contains(address)
                && after.json.contains_key(address)
                && post_manifest
                    .entries
                    .iter()
                    .any(|entry| entry.namespace == address.0 && entry.physical_key == address.1)
        });
        if !complete_purge {
            return Err(closure_error(
                "reset/reseed/delete must delete or exact-CAS replace every known raw material, private, derived, and projection address from the terminated generation",
            ));
        }
    }
    Ok(())
}

fn validate_subject_soul_closure_map(
    json: &BTreeMap<(String, String), Value>,
) -> Result<StoreOpenClosureCertificate> {
    let protected = json
        .iter()
        .filter(|((namespace, _), _)| {
            is_subject_soul_protected_json_namespace(namespace)
                || is_relationship_source_protected_json_namespace(namespace)
        })
        .collect::<Vec<_>>();
    for ((namespace, key), value) in &protected {
        admit_store_json_document(namespace, key, value, "store_subject_soul_open_closure")?;
    }

    let mut heads = BTreeMap::<String, SubjectSoulLifecycleHeadV1>::new();
    let mut manifests = BTreeMap::<String, SubjectSoulScopeManifestV1>::new();
    let mut relationship_manifests = BTreeMap::<String, RelationshipSourceScopeManifestV1>::new();
    for ((namespace, key), value) in &protected {
        match namespace.as_str() {
            SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE => {
                heads.insert(key.clone(), decode_value(value)?);
            }
            SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE => {
                manifests.insert(key.clone(), decode_value(value)?);
            }
            RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE => {
                relationship_manifests.insert(key.clone(), decode_value(value)?);
            }
            _ => {}
        }
    }
    if heads.keys().collect::<Vec<_>>() != manifests.keys().collect::<Vec<_>>() {
        return Err(closure_error(
            "lifecycle head and scope manifest roots are not exactly paired",
        ));
    }

    let mut address_owner = BTreeMap::<(String, String), String>::new();
    let mut owners_with_artifacts = BTreeSet::new();
    let mut relationships_with_artifacts = BTreeSet::new();
    for ((namespace, key), value) in &protected {
        if *namespace != SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE {
            continue;
        }
        let result: SubjectSoulDurableOperationResultV1 = decode_value(value)?;
        let receipt: MemoryMutationReceipt = decode_value(
            json.get(&(
                bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.to_string(),
                key.to_string(),
            ))
            .ok_or_else(|| closure_error("Soul result receipt lineage is missing"))?,
        )?;
        let audit: MemoryMutationAuditRecord = decode_value(
            json.get(&(
                bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.to_string(),
                key.to_string(),
            ))
            .ok_or_else(|| closure_error("Soul result audit lineage is missing"))?,
        )?;
        if receipt.identity != result.identity
            || audit.identity != result.identity
            || receipt.transaction_id
                != result
                    .committed_report
                    .transaction_id
                    .clone()
                    .unwrap_or_default()
            || audit.transaction_id != receipt.transaction_id
            || audit.audit_record_id != receipt.audit_record_id
        {
            return Err(closure_error(
                "Soul durable result does not exactly bind receipt/audit lineage",
            ));
        }
        let owner = format!(
            "{}/{}/{}",
            result.identity.memory_space_id(),
            result.identity.mounted_subject_id(),
            result.soul_id
        );
        owners_with_artifacts.insert((
            result.identity.memory_space_id().to_string(),
            result.identity.mounted_subject_id().to_string(),
            result.soul_id.clone(),
        ));
        bind_address_owner(
            &mut address_owner,
            (namespace.to_string(), key.to_string()),
            &owner,
        )?;
    }
    for ((namespace, key), value) in &protected {
        if *namespace != RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE {
            continue;
        }
        let result: RelationshipSourceDurableOperationResultV1 = decode_value(value)?;
        let receipt: MemoryMutationReceipt = decode_value(
            json.get(&(
                bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.to_string(),
                key.to_string(),
            ))
            .ok_or_else(|| closure_error("relationship result receipt lineage is missing"))?,
        )?;
        let audit: MemoryMutationAuditRecord = decode_value(
            json.get(&(
                bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.to_string(),
                key.to_string(),
            ))
            .ok_or_else(|| closure_error("relationship result audit lineage is missing"))?,
        )?;
        let mor_intent_digest =
            canonical_mor_intent_digest_from_core_digest(&result.committed_report.intent_digest)?;
        if receipt.identity != result.identity
            || audit.identity != result.identity
            || receipt.intent_digest != mor_intent_digest
            || audit.intent_digest != receipt.intent_digest
            || receipt.transaction_id != result.committed_report.transaction_id
            || audit.transaction_id != receipt.transaction_id
            || audit.audit_record_id != receipt.audit_record_id
        {
            return Err(closure_error(
                "relationship durable result does not bind exact receipt/audit lineage",
            ));
        }
        bind_address_owner(
            &mut address_owner,
            (namespace.to_string(), key.to_string()),
            &format!(
                "relationship/{}/{}",
                result.identity.memory_space_id(),
                result.relationship_id
            ),
        )?;
    }
    for (scope_key, head) in &heads {
        let manifest = manifests.get(scope_key).expect("paired roots");
        let owner = format!(
            "{}/{}/{}",
            head.memory_space_id, head.subject_id, head.soul_id
        );
        owners_with_artifacts.insert((
            head.memory_space_id.clone(),
            head.subject_id.clone(),
            head.soul_id.clone(),
        ));
        bind_address_owner(
            &mut address_owner,
            (
                SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.to_string(),
                scope_key.clone(),
            ),
            &owner,
        )?;
        bind_address_owner(
            &mut address_owner,
            (
                SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                scope_key.clone(),
            ),
            &owner,
        )?;
        let mut current_material = None;
        let mut current_core = None;
        let mut current_core_document = None;
        let mut current_ledger = None;
        let mut current_ledger_document = None;
        for entry in &manifest.entries {
            let address = (entry.namespace.clone(), entry.physical_key.clone());
            let value = json.get(&address).ok_or_else(|| {
                closure_error(format!(
                    "manifest entry is missing: {}/{}",
                    entry.namespace, entry.physical_key
                ))
            })?;
            bind_address_owner(&mut address_owner, address.clone(), &owner)?;
            if entry.namespace == SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE {
                let material: SubjectSoulRevisionMaterialV1 = decode_value(value)?;
                if material.content_digest != entry.content_digest {
                    return Err(closure_error("material manifest digest mismatch"));
                }
                if Some(material.revision) == head.current_revision {
                    current_material = Some(material);
                }
            } else if is_subject_global_soul_json_namespace(&entry.namespace) {
                let envelope: SubjectSoulOwnedDocumentV1 = decode_value(value)?;
                if envelope.memory_space_id != head.memory_space_id
                    || envelope.subject_id != head.subject_id
                    || envelope.soul_id != head.soul_id
                    || envelope.generation != head.generation
                    || envelope.content_digest != entry.content_digest
                {
                    return Err(closure_error("Subject Soul envelope owner/digest mismatch"));
                }
                if entry.namespace == "self_authored_core"
                    && entry.revision == head.current_revision
                {
                    current_core = Some(decode_value::<SelfAuthoredCore>(&envelope.body)?);
                    current_core_document = Some(envelope.clone());
                }
                if entry.namespace == "core_revision_ledger"
                    && entry.revision == head.current_revision
                {
                    current_ledger = Some(decode_value::<CoreRevisionLedger>(&envelope.body)?);
                    current_ledger_document = Some(envelope);
                }
            } else if entry.namespace == SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE {
                let projection: SubjectSoulRelationshipProjectionV1 = decode_value(value)?;
                if head.state != SubjectSoulLifecycleStateV1::Active {
                    return Err(closure_error(
                        "non-active Subject Soul cannot retain a relationship projection",
                    ));
                }
                if projection.content_digest != entry.content_digest {
                    return Err(closure_error(
                        "relationship projection manifest digest mismatch",
                    ));
                }
            } else if entry.namespace == SUBJECT_SOUL_GENERATION_TOMBSTONE_NAMESPACE {
                let tombstone: SubjectSoulGenerationTombstoneV1 = decode_value(value)?;
                if tombstone.tombstone_digest != entry.content_digest {
                    return Err(closure_error(
                        "generation tombstone manifest digest mismatch",
                    ));
                }
            } else {
                return Err(closure_error(format!(
                    "manifest references unsupported protected namespace {}",
                    entry.namespace
                )));
            }
        }
        for key in &head.retained_revision_refs {
            let address = (
                SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE.to_string(),
                key.clone(),
            );
            let material: SubjectSoulRevisionMaterialV1 =
                decode_value(json.get(&address).ok_or_else(|| {
                    closure_error("retained Soul revision reference is missing")
                })?)?;
            let canonical_key = subject_soul_revision_material_key(
                &material.memory_space_id,
                &material.subject_id,
                &material.soul_id,
                material.generation,
                material.revision,
            )?;
            if material.memory_space_id != head.memory_space_id
                || material.subject_id != head.subject_id
                || material.soul_id != head.soul_id
                || canonical_key != *key
            {
                return Err(closure_error(
                    "retained Soul revision material owner/address mismatch",
                ));
            }
            bind_address_owner(&mut address_owner, address, &owner)?;
        }
        for key in &head.retained_tombstone_refs {
            let address = (
                SUBJECT_SOUL_GENERATION_TOMBSTONE_NAMESPACE.to_string(),
                key.clone(),
            );
            let tombstone: SubjectSoulGenerationTombstoneV1 =
                decode_value(json.get(&address).ok_or_else(|| {
                    closure_error("retained Soul tombstone reference is missing")
                })?)?;
            let canonical_key = subject_soul_generation_tombstone_key(
                &tombstone.memory_space_id,
                &tombstone.subject_id,
                &tombstone.soul_id,
                tombstone.generation,
            )?;
            if tombstone.memory_space_id != head.memory_space_id
                || tombstone.subject_id != head.subject_id
                || tombstone.soul_id != head.soul_id
                || canonical_key != *key
            {
                return Err(closure_error(
                    "retained Soul tombstone owner/address mismatch",
                ));
            }
            bind_address_owner(&mut address_owner, address, &owner)?;
        }
        SubjectSoulVerifiedSnapshotV1 {
            head: head.clone(),
            manifest: manifest.clone(),
            current_material,
            current_core,
            current_core_document,
            current_revision_ledger: current_ledger,
            current_revision_ledger_document: current_ledger_document,
        }
        .validate_contract()
        .map_err(|error| closure_error(error.to_string()))?;
    }

    for (manifest_key, manifest) in &relationship_manifests {
        relationships_with_artifacts.insert((
            manifest.memory_space_id.clone(),
            manifest.relationship_id.clone(),
        ));
        let owner = format!(
            "relationship/{}/{}",
            manifest.memory_space_id, manifest.relationship_id
        );
        bind_address_owner(
            &mut address_owner,
            (
                RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                manifest_key.clone(),
            ),
            &owner,
        )?;
        let current_key = relationship_source_revision_key(
            &manifest.memory_space_id,
            &manifest.relationship_id,
            manifest.current_revision,
        )?;
        let current_address = (
            RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
            current_key,
        );
        let source: RelationshipSourceConstitutionV1 = decode_value(
            json.get(&current_address)
                .ok_or_else(|| closure_error("relationship current source is missing"))?,
        )?;
        if source.content_digest != manifest.current_digest {
            return Err(closure_error(
                "relationship source/manifest digest mismatch",
            ));
        }
        validate_relationship_source_post_image(&source, manifest)
            .map_err(|error| closure_error(error.to_string()))?;
        bind_address_owner(&mut address_owner, current_address, &owner)?;
        for key in &manifest.retained_revision_refs {
            let address = (
                RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                key.clone(),
            );
            let retained: RelationshipSourceConstitutionV1 = decode_value(
                json.get(&address)
                    .ok_or_else(|| closure_error("retained relationship source is missing"))?,
            )?;
            let canonical_key = relationship_source_revision_key(
                &retained.memory_space_id,
                &retained.relationship_id,
                retained.revision,
            )?;
            if retained.memory_space_id != manifest.memory_space_id
                || retained.relationship_id != manifest.relationship_id
                || canonical_key != *key
            {
                return Err(closure_error(
                    "retained relationship source owner/address mismatch",
                ));
            }
            bind_address_owner(&mut address_owner, address, &owner)?;
        }
    }

    for ((namespace, key), value) in &protected {
        if *namespace == SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE {
            let projection: SubjectSoulRelationshipProjectionV1 = decode_value(value)?;
            let material_key = subject_soul_revision_material_key(
                &projection.memory_space_id,
                &projection.subject_id,
                &projection.soul_id,
                projection.generation,
                projection.soul_revision,
            )?;
            let material: SubjectSoulRevisionMaterialV1 = decode_value(
                json.get(&(
                    SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE.to_string(),
                    material_key,
                ))
                .ok_or_else(|| closure_error("projection Soul material root is missing"))?,
            )?;
            let source_key = relationship_source_revision_key(
                &projection.memory_space_id,
                &projection.relationship_id,
                projection.relationship_source_revision,
            )?;
            let source: RelationshipSourceConstitutionV1 = decode_value(
                json.get(&(
                    RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                    source_key,
                ))
                .ok_or_else(|| closure_error("projection relationship source root is missing"))?,
            )?;
            projection
                .validate_contract(&source, &material)
                .map_err(|error| closure_error(error.to_string()))?;
            if !address_owner.contains_key(&(namespace.to_string(), key.to_string())) {
                return Err(closure_error(
                    "relationship projection is not Soul-manifest owned",
                ));
            }
        }
    }

    for ((namespace, key), _) in &protected {
        if !address_owner.contains_key(&(namespace.to_string(), key.to_string())) {
            return Err(closure_error(format!(
                "protected Subject Soul address is orphaned: {namespace}/{key}"
            )));
        }
    }

    let mut hasher = Sha256::new();
    hasher.update(b"store_open_subject_soul_closure_v1\0");
    let durable_result_keys = protected
        .iter()
        .filter_map(|((namespace, key), _)| {
            matches!(
                namespace.as_str(),
                SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE
                    | RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE
            )
            .then_some(key.as_str())
        })
        .collect::<BTreeSet<_>>();
    let certificate_documents = json.iter().filter(|((namespace, key), _)| {
        is_subject_soul_protected_json_namespace(namespace)
            || is_relationship_source_protected_json_namespace(namespace)
            || (durable_result_keys.contains(key.as_str())
                && matches!(
                    namespace.as_str(),
                    bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE
                        | bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE
                ))
    });
    for ((namespace, key), value) in certificate_documents {
        hasher.update((namespace.len() as u64).to_be_bytes());
        hasher.update(namespace.as_bytes());
        hasher.update((key.len() as u64).to_be_bytes());
        hasher.update(key.as_bytes());
        let bytes = serde_json::to_vec(value)
            .map_err(|error| closure_error(format!("cannot hash protected document: {error}")))?;
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    Ok(StoreOpenClosureCertificate {
        digest: format!("{:x}", hasher.finalize()),
        owners_with_artifacts,
        relationships_with_artifacts,
    })
}

fn bind_address_owner(
    owners: &mut BTreeMap<(String, String), String>,
    address: (String, String),
    owner: &str,
) -> Result<()> {
    if let Some(existing) = owners.insert(address.clone(), owner.to_string()) {
        if existing != owner {
            return Err(closure_error(format!(
                "protected address {}/{} has multiple owners",
                address.0, address.1
            )));
        }
    }
    Ok(())
}

fn closure_error(detail: impl Into<String>) -> Error {
    Error::config("store_subject_soul_open_closure", detail.into())
}

fn decode_value<T: DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| closure_error(format!("typed protected document is invalid: {error}")))
}

struct VerifiedRelationshipRoots {
    sources: BTreeMap<String, RelationshipSourceConstitutionV1>,
    manifests: BTreeMap<String, RelationshipSourceScopeManifestV1>,
}

fn relationship_roots_from_closure(
    documents: &[((String, String), Value)],
) -> std::result::Result<VerifiedRelationshipRoots, SubjectSoulStoreFailure> {
    let mut manifests = BTreeMap::new();
    for ((namespace, _), value) in documents {
        if namespace != RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE {
            continue;
        }
        let manifest: RelationshipSourceScopeManifestV1 = serde_json::from_value(value.clone())
            .map_err(|error| {
                SubjectSoulStoreFailure::repair(format!(
                    "invalid relationship manifest in verified Soul closure: {error}"
                ))
            })?;
        manifest
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::contract)?;
        if manifests
            .insert(manifest.relationship_id.clone(), manifest)
            .is_some()
        {
            return Err(SubjectSoulStoreFailure::repair(
                "verified Soul closure contains multiple manifests for one relationship",
            ));
        }
    }
    let mut sources = BTreeMap::new();
    for (relationship_id, manifest) in &manifests {
        let source_key = relationship_source_revision_key(
            &manifest.memory_space_id,
            relationship_id,
            manifest.current_revision,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        let value = documents
            .iter()
            .find(|((namespace, key), _)| {
                namespace == RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE && key == &source_key
            })
            .map(|(_, value)| value)
            .ok_or_else(|| {
                SubjectSoulStoreFailure::repair(
                    "verified Soul closure is missing the current relationship source",
                )
            })?;
        let source: RelationshipSourceConstitutionV1 = serde_json::from_value(value.clone())
            .map_err(|error| {
                SubjectSoulStoreFailure::repair(format!(
                    "invalid current relationship source in verified Soul closure: {error}"
                ))
            })?;
        source
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::contract)?;
        if source.memory_space_id != manifest.memory_space_id
            || source.relationship_id != *relationship_id
            || source.revision != manifest.current_revision
            || source.content_digest != manifest.current_digest
        {
            return Err(SubjectSoulStoreFailure::repair(
                "current relationship source differs from its exact manifest root",
            ));
        }
        sources.insert(relationship_id.clone(), source);
    }
    Ok(VerifiedRelationshipRoots { sources, manifests })
}

pub(crate) struct SubjectSoulVerifiedStoreRead {
    pub(crate) outcome: SubjectSoulReadOutcomeV1,
    pub(crate) head: Option<SubjectSoulLifecycleHeadV1>,
    pub(crate) manifest: Option<SubjectSoulScopeManifestV1>,
    pub(crate) selected_material: Option<SubjectSoulRevisionMaterialV1>,
    pub(crate) current_material: Option<SubjectSoulRevisionMaterialV1>,
    pub(crate) current_core: Option<SelfAuthoredCore>,
    pub(crate) current_core_document: Option<SubjectSoulOwnedDocumentV1>,
    pub(crate) current_ledger: Option<CoreRevisionLedger>,
    pub(crate) current_ledger_document: Option<SubjectSoulOwnedDocumentV1>,
    pub(crate) closure_documents: Vec<((String, String), Value)>,
    relationship_sources: BTreeMap<String, RelationshipSourceConstitutionV1>,
    relationship_source_manifests: BTreeMap<String, RelationshipSourceScopeManifestV1>,
    pub(crate) receipt: StoreReadReceipt,
}

impl SubjectSoulVerifiedStoreRead {
    pub(crate) fn relationship_source(
        &self,
        relationship_id: &str,
    ) -> std::result::Result<Option<RelationshipSourceConstitutionV1>, SubjectSoulStoreFailure>
    {
        let source = self.relationship_sources.get(relationship_id).cloned();
        if let Some(source) = &source {
            source
                .validate_contract()
                .map_err(SubjectSoulStoreFailure::contract)?;
        }
        Ok(source)
    }

    fn relationship_projection_dependency_preconditions_for_id(
        &self,
        relationship_id: &str,
    ) -> std::result::Result<Vec<StoreJsonPrecondition>, SubjectSoulStoreFailure> {
        let source = self
            .relationship_sources
            .get(relationship_id)
            .ok_or_else(|| {
                SubjectSoulStoreFailure::repair(
                    "relationship projection dependency has no verified current source",
                )
            })?;
        let manifest = self
            .relationship_source_manifests
            .get(relationship_id)
            .ok_or_else(|| {
                SubjectSoulStoreFailure::repair(
                    "relationship projection dependency has no verified current manifest",
                )
            })?;
        validate_relationship_source_post_image(source, manifest)
            .map_err(SubjectSoulStoreFailure::contract)?;
        let source_key = relationship_source_revision_key(
            &source.memory_space_id,
            relationship_id,
            source.revision,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        let manifest_key = super::schema::relationship_source_scope_key(
            &manifest.memory_space_id,
            relationship_id,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        Ok(vec![
            StoreJsonPrecondition::Exact {
                namespace: RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                key: source_key,
                value: serde_json::to_value(source)
                    .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?,
            },
            StoreJsonPrecondition::Exact {
                namespace: RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                key: manifest_key,
                value: serde_json::to_value(manifest)
                    .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?,
            },
        ])
    }

    pub(crate) fn relationship_projection_purge_preconditions(
        &self,
        purge_addresses: &[bm_core::memory::SubjectSoulManifestAddressV1],
    ) -> std::result::Result<Vec<StoreJsonPrecondition>, SubjectSoulStoreFailure> {
        let head = self.head.as_ref().ok_or_else(|| {
            SubjectSoulStoreFailure::repair(
                "relationship projection purge requires a verified Subject Soul root",
            )
        })?;
        let mut relationship_projection_digests = BTreeMap::new();
        for address in purge_addresses
            .iter()
            .filter(|address| address.namespace == SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE)
        {
            let value = self
                .closure_documents
                .iter()
                .find(|((namespace, key), _)| {
                    namespace == &address.namespace && key == &address.physical_key
                })
                .map(|(_, value)| value)
                .ok_or_else(|| {
                    SubjectSoulStoreFailure::repair(
                        "relationship projection purge address is outside the verified closure",
                    )
                })?;
            let projection: SubjectSoulRelationshipProjectionV1 =
                serde_json::from_value(value.clone()).map_err(|error| {
                    SubjectSoulStoreFailure::repair(format!(
                        "invalid relationship projection purge document: {error}"
                    ))
                })?;
            let canonical_key = subject_soul_relationship_projection_key(
                &projection.memory_space_id,
                &projection.subject_id,
                &projection.soul_id,
                &projection.relationship_id,
                projection.generation,
            )
            .map_err(SubjectSoulStoreFailure::from_store)?;
            if projection.memory_space_id != head.memory_space_id
                || projection.subject_id != head.subject_id
                || projection.soul_id != head.soul_id
                || projection.generation != head.generation
                || canonical_key != address.physical_key
            {
                return Err(SubjectSoulStoreFailure::repair(
                    "relationship projection purge owner/address differs from the verified Soul",
                ));
            }
            match relationship_projection_digests.insert(
                projection.relationship_id.clone(),
                projection.content_digest.clone(),
            ) {
                Some(existing) if existing != projection.content_digest => {
                    return Err(SubjectSoulStoreFailure::repair(
                        "verified Soul contains conflicting projections for one relationship",
                    ));
                }
                _ => {}
            }
        }

        let mut exact = BTreeMap::new();
        for relationship_id in relationship_projection_digests.keys() {
            for dependency in
                self.relationship_projection_dependency_preconditions_for_id(relationship_id)?
            {
                let StoreJsonPrecondition::Exact {
                    namespace,
                    key,
                    value,
                } = dependency
                else {
                    return Err(SubjectSoulStoreFailure::repair(
                        "relationship projection dependency is not Exact CAS",
                    ));
                };
                match exact.insert((namespace, key), value.clone()) {
                    Some(existing) if existing != value => {
                        return Err(SubjectSoulStoreFailure::repair(
                            "relationship projection dependencies conflict on one Store address",
                        ));
                    }
                    _ => {}
                }
            }
        }
        Ok(exact
            .into_iter()
            .map(|((namespace, key), value)| StoreJsonPrecondition::Exact {
                namespace,
                key,
                value,
            })
            .collect())
    }

    pub(crate) fn relationship_runtime_input(
        &self,
        relationship_id: &str,
    ) -> std::result::Result<Option<SubjectSoulRelationshipRuntimeInputV1>, SubjectSoulStoreFailure>
    {
        let Some(source) = self.relationship_source(relationship_id)? else {
            return Ok(None);
        };
        let mut stored_projection = None;
        for ((namespace, _), value) in &self.closure_documents {
            if namespace != SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE {
                continue;
            }
            let projection: SubjectSoulRelationshipProjectionV1 =
                serde_json::from_value(value.clone()).map_err(|error| {
                    SubjectSoulStoreFailure::repair(format!(
                        "invalid relationship projection in verified closure: {error}"
                    ))
                })?;
            if projection.relationship_id != relationship_id {
                continue;
            }
            let exact_owner = self.head.as_ref().is_some_and(|head| {
                projection.memory_space_id == head.memory_space_id
                    && projection.subject_id == head.subject_id
                    && projection.soul_id == head.soul_id
            });
            if !exact_owner || stored_projection.replace(projection).is_some() {
                return Err(SubjectSoulStoreFailure::repair(
                    "relationship projection owner/cardinality is invalid",
                ));
            }
        }
        Ok(Some(SubjectSoulRelationshipRuntimeInputV1 {
            source,
            current_material: self.current_material.clone(),
            stored_projection,
        }))
    }

    pub(crate) fn relationship_projection(
        &self,
        relationship_id: &str,
    ) -> std::result::Result<
        Option<(
            SubjectSoulRelationshipProjectionV1,
            RelationshipSourceConstitutionV1,
        )>,
        SubjectSoulStoreFailure,
    > {
        let projection = self
            .closure_documents
            .iter()
            .find_map(|((namespace, _), value)| {
                if namespace != SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE {
                    return None;
                }
                serde_json::from_value::<SubjectSoulRelationshipProjectionV1>(value.clone())
                    .ok()
                    .filter(|projection| projection.relationship_id == relationship_id)
            });
        let Some(projection) = projection else {
            return Ok(None);
        };
        let source = self.relationship_source(relationship_id)?.filter(|source| {
            source.revision == projection.relationship_source_revision
                && source.content_digest == projection.relationship_source_digest
        });
        let Some(source) = source else {
            return Err(SubjectSoulStoreFailure::repair(
                "verified projection has no exact relationship source root",
            ));
        };
        let material = self
            .closure_documents
            .iter()
            .find_map(|((namespace, _), value)| {
                if namespace != SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE {
                    return None;
                }
                serde_json::from_value::<SubjectSoulRevisionMaterialV1>(value.clone())
                    .ok()
                    .filter(|material| {
                        material.generation == projection.generation
                            && material.revision == projection.soul_revision
                            && material.content_digest == projection.soul_material_digest
                    })
            })
            .ok_or_else(|| {
                SubjectSoulStoreFailure::repair(
                    "verified projection has no exact Subject Soul material root",
                )
            })?;
        projection
            .validate_contract(&source, &material)
            .map_err(SubjectSoulStoreFailure::contract)?;
        Ok(Some((projection, source)))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RelationshipSourceStoreMutationPlan {
    expected_state: RelationshipSourceExpectedStateV1,
    source: RelationshipSourceConstitutionV1,
    batch: StoreMutationBatch,
    preconditions: Vec<StoreJsonPrecondition>,
    operation: StoreMutationOperationPlan,
    durable_result: RelationshipSourceDurableOperationResultV1,
}

fn validate_relationship_projection_plan_binding(
    projection_plan: Option<&SubjectSoulRelationshipProjectionPlanV1>,
    relationship_source: &RelationshipSourceConstitutionV1,
    batch: &StoreMutationBatch,
    preconditions: &[StoreJsonPrecondition],
) -> std::result::Result<(), SubjectSoulStoreFailure> {
    let has_soul_projection_mutation = batch.mutations.iter().any(|mutation| match mutation {
        StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. } => {
            matches!(
                namespace.as_str(),
                SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE
                    | SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE
                    | SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE
            )
        }
        _ => false,
    });
    let (post_head, post_manifest, projection) = match projection_plan {
        None | Some(SubjectSoulRelationshipProjectionPlanV1::NoEffect) => {
            if has_soul_projection_mutation {
                return Err(SubjectSoulStoreFailure::repair(
                    "relationship projection mutations require the exact Core projection plan",
                ));
            }
            return Ok(());
        }
        Some(SubjectSoulRelationshipProjectionPlanV1::Upsert {
            projection,
            post_head,
            post_manifest,
        }) => (
            post_head.as_ref(),
            post_manifest.as_ref(),
            Some(projection.as_ref()),
        ),
        Some(SubjectSoulRelationshipProjectionPlanV1::Delete {
            post_head,
            post_manifest,
        }) => (post_head.as_ref(), post_manifest.as_ref(), None),
    };
    if post_head.memory_space_id != relationship_source.memory_space_id
        || post_head.subject_id != relationship_source.mounted_subject_id
        || post_manifest.memory_space_id != post_head.memory_space_id
        || post_manifest.subject_id != post_head.subject_id
        || post_manifest.soul_id != post_head.soul_id
    {
        return Err(SubjectSoulStoreFailure::repair(
            "relationship projection plan does not belong to the exact Soul/source owner",
        ));
    }
    let soul_root_key = subject_soul_scope_key(
        &post_head.memory_space_id,
        &post_head.subject_id,
        &post_head.soul_id,
    )
    .map_err(SubjectSoulStoreFailure::from_store)?;
    if mutation_put_value(batch, SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE, &soul_root_key)?
        != serde_json::to_value(post_head)
            .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?
        || mutation_put_value(batch, SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE, &soul_root_key)?
            != serde_json::to_value(post_manifest)
                .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?
        || !matches!(
            find_precondition(
                preconditions,
                SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE,
                &soul_root_key
            ),
            Some(StoreJsonPrecondition::Exact { .. })
        )
        || !matches!(
            find_precondition(
                preconditions,
                SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE,
                &soul_root_key
            ),
            Some(StoreJsonPrecondition::Exact { .. })
        )
    {
        return Err(SubjectSoulStoreFailure::repair(
            "relationship projection plan requires exact prior Soul roots and exact post roots",
        ));
    }

    if let Some(projection) = projection {
        if projection.memory_space_id != relationship_source.memory_space_id
            || projection.subject_id != relationship_source.mounted_subject_id
            || projection.relationship_id != relationship_source.relationship_id
            || projection.relationship_source_revision != relationship_source.revision
            || projection.relationship_source_digest != relationship_source.content_digest
        {
            return Err(SubjectSoulStoreFailure::repair(
                "relationship projection is not bound to the exact post source",
            ));
        }
        let projection_key = subject_soul_relationship_projection_key(
            &projection.memory_space_id,
            &projection.subject_id,
            &projection.soul_id,
            &projection.relationship_id,
            projection.generation,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        if mutation_put_value(
            batch,
            SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE,
            &projection_key,
        )? != serde_json::to_value(projection)
            .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?
            || !post_manifest.entries.iter().any(|entry| {
                entry.namespace == SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE
                    && entry.physical_key == projection_key
                    && entry.content_digest == projection.content_digest
            })
        {
            return Err(SubjectSoulStoreFailure::repair(
                "relationship projection upsert differs from its Core post-image/manifest",
            ));
        }
    } else {
        let deletes = batch
            .mutations
            .iter()
            .filter_map(|mutation| match mutation {
                StoreMutation::DeleteJson { namespace, key, .. }
                    if namespace == SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE =>
                {
                    Some(key)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let [projection_key] = deletes.as_slice() else {
            return Err(SubjectSoulStoreFailure::repair(
                "relationship projection delete plan requires one exact projection deletion",
            ));
        };
        let Some(StoreJsonPrecondition::Exact { value, .. }) = find_precondition(
            preconditions,
            SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE,
            projection_key,
        ) else {
            return Err(SubjectSoulStoreFailure::repair(
                "relationship projection delete requires exact prior projection CAS",
            ));
        };
        let prior: SubjectSoulRelationshipProjectionV1 = serde_json::from_value(value.clone())
            .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
        if prior.memory_space_id != relationship_source.memory_space_id
            || prior.subject_id != relationship_source.mounted_subject_id
            || prior.relationship_id != relationship_source.relationship_id
            || post_manifest.entries.iter().any(|entry| {
                entry.namespace == SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE
                    && entry.physical_key == **projection_key
            })
        {
            return Err(SubjectSoulStoreFailure::repair(
                "relationship projection delete is not the exact Core owner closure",
            ));
        }
    }
    Ok(())
}

impl RelationshipSourceStoreMutationPlan {
    pub(crate) fn new(
        control_plan: RelationshipSourceControlPlanV1,
        projection_plan: Option<SubjectSoulRelationshipProjectionPlanV1>,
        mut batch: StoreMutationBatch,
        mut preconditions: Vec<StoreJsonPrecondition>,
        operation: StoreMutationOperationPlan,
    ) -> std::result::Result<Self, SubjectSoulStoreFailure> {
        let operation =
            operation.authorize_subject_soul(SubjectSoulStoreMutationAuthority::issue());
        control_plan
            .validate_contract()
            .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
        let RelationshipSourceControlPlanV1 {
            action,
            expected_state,
            actor_subject_id,
            intent_digest,
            post_source: source,
            post_manifest: manifest,
        } = control_plan;
        let mor_intent_digest = canonical_mor_intent_digest_from_core_digest(&intent_digest)
            .map_err(SubjectSoulStoreFailure::from_store)?;
        expected_state
            .validate_contract()
            .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
        validate_relationship_source_post_image(&source, &manifest)
            .map_err(SubjectSoulStoreFailure::contract)?;
        validate_relationship_projection_plan_binding(
            projection_plan.as_ref(),
            &source,
            &batch,
            &preconditions,
        )?;
        if batch.scope.memory_space_id != source.memory_space_id
            || batch.scope.subject_id != source.mounted_subject_id
            || operation.identity().memory_space_id() != source.memory_space_id
            || operation.identity().mounted_subject_id() != source.mounted_subject_id
            || operation.identity().actor_subject_id() != actor_subject_id
            || operation.identity().operation_kind()
                != bm_core::memory::MemoryMutationOperationKind::RelationshipControl
            || operation.intent_digest() != mor_intent_digest
        {
            return Err(SubjectSoulStoreFailure::repair(
                "relationship batch/operation scope or MOR intent is not the exact typed control owner",
            ));
        }
        let source_key = relationship_source_revision_key(
            &source.memory_space_id,
            &source.relationship_id,
            source.revision,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        let manifest_key = super::schema::relationship_source_scope_key(
            &source.memory_space_id,
            &source.relationship_id,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        if mutation_put_value(
            &batch,
            RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE,
            &source_key,
        )? != serde_json::to_value(&source)
            .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?
            || mutation_put_value(
                &batch,
                RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE,
                &manifest_key,
            )? != serde_json::to_value(&manifest)
                .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?
        {
            return Err(SubjectSoulStoreFailure::repair(
                "relationship typed plan differs from its batch post-image",
            ));
        }
        if !matches!(
            find_precondition(
                &preconditions,
                RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE,
                &source_key
            ),
            Some(StoreJsonPrecondition::Absent { .. })
        ) {
            return Err(expected_state_failure(
                "immutable relationship revision requires Absent CAS",
            ));
        }
        let manifest_precondition = find_precondition(
            &preconditions,
            RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE,
            &manifest_key,
        );
        match &expected_state {
            RelationshipSourceExpectedStateV1::PristineAbsent { .. } => {
                if source.revision != 1
                    || !matches!(
                        manifest_precondition,
                        Some(StoreJsonPrecondition::Absent { .. })
                    )
                {
                    return Err(expected_state_failure(
                        "relationship create requires revision 1 and Absent manifest CAS",
                    ));
                }
            }
            RelationshipSourceExpectedStateV1::Exact {
                revision,
                state,
                source_digest,
                manifest_digest,
            } => {
                let Some(StoreJsonPrecondition::Exact { value, .. }) = manifest_precondition else {
                    return Err(expected_state_failure(
                        "relationship successor requires Exact manifest CAS",
                    ));
                };
                let prior_manifest: RelationshipSourceScopeManifestV1 =
                    serde_json::from_value(value.clone()).map_err(|error| {
                        SubjectSoulStoreFailure::repair(format!(
                            "invalid prior relationship manifest: {error}"
                        ))
                    })?;
                let prior_key = relationship_source_revision_key(
                    &source.memory_space_id,
                    &source.relationship_id,
                    *revision,
                )
                .map_err(SubjectSoulStoreFailure::from_store)?;
                let Some(StoreJsonPrecondition::Exact {
                    value: prior_value, ..
                }) = find_precondition(
                    &preconditions,
                    RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE,
                    &prior_key,
                )
                else {
                    return Err(expected_state_failure(
                        "relationship successor requires Exact prior source CAS",
                    ));
                };
                let prior_source: RelationshipSourceConstitutionV1 =
                    serde_json::from_value(prior_value.clone()).map_err(|error| {
                        SubjectSoulStoreFailure::repair(format!(
                            "invalid prior relationship source: {error}"
                        ))
                    })?;
                if prior_source.revision != *revision
                    || prior_source.state != *state
                    || prior_source.content_digest != *source_digest
                    || prior_manifest.closure_digest != *manifest_digest
                {
                    return Err(expected_state_failure(
                        "relationship expected state differs from exact source/manifest CAS",
                    ));
                }
            }
        }
        let safe_event_refs = batch
            .mutations
            .iter()
            .filter_map(|mutation| match mutation {
                StoreMutation::AppendEvent { event } => Some(event.event_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let [safe_event_ref] = safe_event_refs.as_slice() else {
            return Err(SubjectSoulStoreFailure::repair(
                "relationship source mutation requires exactly one safe lifecycle event",
            ));
        };
        let receipt_key = operation.identity().storage_key();
        let committed_report = RelationshipSourceControlReportV1 {
            outcome: RelationshipSourceControlOutcomeV1::Committed,
            action,
            revision: source.revision,
            state: source.state,
            source_digest: source.content_digest.clone(),
            manifest_digest: manifest.closure_digest.clone(),
            intent_digest,
            transaction_id: operation.transaction_id().to_string(),
            durable_receipt_ref: receipt_key.clone(),
            safe_event_ref: safe_event_ref.clone(),
            replayed: false,
        };
        let durable_result = RelationshipSourceDurableOperationResultV1::new(
            operation.identity().clone(),
            source.relationship_id.clone(),
            committed_report,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        preconditions.push(StoreJsonPrecondition::Absent {
            namespace: RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE.to_string(),
            key: receipt_key.clone(),
        });
        batch.mutations.push(StoreMutation::PutJson {
            namespace: RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE.to_string(),
            key: receipt_key.clone(),
            value: serde_json::to_value(&durable_result)
                .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?,
            event_kind: super::MemoryStoreEventKind::MemoryControl,
            plane: RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE.to_string(),
            record_key: receipt_key,
        });
        Ok(Self {
            expected_state,
            source,
            batch,
            preconditions,
            operation,
            durable_result,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RelationshipSourceVerifiedStoreRead {
    pub(crate) source: RelationshipSourceConstitutionV1,
    pub(crate) manifest: RelationshipSourceScopeManifestV1,
    pub(crate) receipt: StoreReadReceipt,
}

#[derive(Clone, Debug)]
pub(crate) enum RelationshipSourceStoreMutationOutcome {
    Committed {
        result: RelationshipSourceControlReportV1,
    },
    Replayed {
        result: RelationshipSourceControlReportV1,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum SubjectSoulStoreMutationOutcome {
    Committed { result: SubjectSoulMutationReportV1 },
    Replayed { result: SubjectSoulMutationReportV1 },
}

impl StorePlatform {
    pub(crate) fn preflight_subject_soul_operation(
        &self,
        identity: &bm_core::memory::MemoryMutationOperationIdentity,
        intent_digest: &str,
        runtime_budget: &RuntimeBudgetReport,
    ) -> std::result::Result<Option<SubjectSoulMutationReportV1>, SubjectSoulStoreFailure> {
        let mor_intent_digest = canonical_mor_intent_digest_from_core_digest(intent_digest)
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let result = self.preflight_typed_operation_result::<SubjectSoulDurableOperationResultV1>(
            identity,
            &mor_intent_digest,
            SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE,
            runtime_budget,
        )?;
        result
            .map(|result| {
                if result.identity != *identity {
                    return Err(SubjectSoulStoreFailure::repair(
                        "Soul preflight result identity mismatch",
                    ));
                }
                let mut report = result.committed_report;
                report.outcome = SubjectSoulMutationOutcomeV1::Replayed;
                report.replayed = true;
                report
                    .validate_contract()
                    .map_err(SubjectSoulStoreFailure::contract)?;
                Ok(report)
            })
            .transpose()
    }

    pub(crate) fn preflight_relationship_source_operation(
        &self,
        identity: &bm_core::memory::MemoryMutationOperationIdentity,
        intent_digest: &str,
        runtime_budget: &RuntimeBudgetReport,
    ) -> std::result::Result<Option<RelationshipSourceControlReportV1>, SubjectSoulStoreFailure>
    {
        let mor_intent_digest = canonical_mor_intent_digest_from_core_digest(intent_digest)
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let result = self
            .preflight_typed_operation_result::<RelationshipSourceDurableOperationResultV1>(
                identity,
                &mor_intent_digest,
                RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE,
                runtime_budget,
            )?;
        result
            .map(|result| {
                if result.identity != *identity
                    || result.committed_report.intent_digest != intent_digest
                {
                    return Err(SubjectSoulStoreFailure::repair(
                        "relationship preflight result identity/intent mismatch",
                    ));
                }
                let mut report = result.committed_report;
                report.outcome = RelationshipSourceControlOutcomeV1::Replayed;
                report.replayed = true;
                report
                    .validate_contract()
                    .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
                Ok(report)
            })
            .transpose()
    }

    fn preflight_typed_operation_result<T: DeserializeOwned>(
        &self,
        identity: &bm_core::memory::MemoryMutationOperationIdentity,
        intent_digest: &str,
        result_namespace: &str,
        runtime_budget: &RuntimeBudgetReport,
    ) -> std::result::Result<Option<T>, SubjectSoulStoreFailure> {
        identity
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::from_store)?;
        runtime_budget
            .validate_for_admission(current_unix_secs())
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let capacity = StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        let key = identity.storage_key();
        let mut session = self
            .engine_for_subject_soul()
            .open_immutable_read_session(capacity)
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let reads = session
            .read_json_known_keys(&[
                (
                    bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.to_string(),
                    key.clone(),
                ),
                (
                    bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.to_string(),
                    key.clone(),
                ),
                (result_namespace.to_string(), key.clone()),
            ])
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let value = |namespace: &str| {
            reads
                .iter()
                .find(|read| read.namespace == namespace && read.key == key)
                .and_then(|read| read.value.as_ref())
        };
        let receipt = value(bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE);
        let audit = value(bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE);
        let result = value(result_namespace);
        match (receipt, audit, result) {
            (None, None, None) => Ok(None),
            (Some(receipt), Some(audit), Some(result)) => {
                let receipt: MemoryMutationReceipt = serde_json::from_value(receipt.clone())
                    .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
                let audit: MemoryMutationAuditRecord = serde_json::from_value(audit.clone())
                    .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
                receipt
                    .classify_replay(identity, intent_digest)
                    .map_err(SubjectSoulStoreFailure::from_store)?;
                audit
                    .validate_contract()
                    .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
                if audit.identity != receipt.identity
                    || audit.intent_digest != receipt.intent_digest
                    || audit.transaction_id != receipt.transaction_id
                    || audit.audit_record_id != receipt.audit_record_id
                {
                    return Err(SubjectSoulStoreFailure::repair(
                        "typed operation preflight receipt/audit closure mismatch",
                    ));
                }
                serde_json::from_value(result.clone())
                    .map(Some)
                    .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))
            }
            _ => Err(SubjectSoulStoreFailure::repair(
                "typed operation receipt/audit/result must exist together",
            )),
        }
    }

    pub(crate) fn commit_relationship_source_mutation_with_runtime_budget(
        &self,
        plan: RelationshipSourceStoreMutationPlan,
        runtime_budget: &RuntimeBudgetReport,
    ) -> std::result::Result<RelationshipSourceStoreMutationOutcome, SubjectSoulStoreFailure> {
        if let RelationshipSourceExpectedStateV1::PristineAbsent {
            closure_certificate_digest,
        } = &plan.expected_state
        {
            if closure_certificate_digest != self.subject_soul_open_closure_certificate().digest()
                || !self
                    .subject_soul_open_closure_certificate()
                    .proves_zero_relationship_artifacts(
                        &plan.source.memory_space_id,
                        &plan.source.relationship_id,
                    )
            {
                return Err(expected_state_failure(
                    "pristine relationship source is not proven by the pinned Store-open closure certificate",
                ));
            }
        }
        let expected_result = plan.durable_result.clone();
        let outcome = self
            .commit_memory_mutation_operation_with_runtime_budget(
                plan.batch,
                &plan.preconditions,
                plan.operation,
                runtime_budget,
            )
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let stored_value = self
            .engine_for_subject_soul()
            .read_consistent_known_keys(
                &[(
                    RELATIONSHIP_SOURCE_OPERATION_RESULT_NAMESPACE.to_string(),
                    expected_result.identity.storage_key(),
                )],
                &[],
                false,
                StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget),
            )
            .map_err(SubjectSoulStoreFailure::from_store)?
            .json
            .into_iter()
            .next()
            .and_then(|read| read.value)
            .ok_or_else(|| {
                SubjectSoulStoreFailure::repair(
                    "relationship operation is missing its durable safe result",
                )
            })?;
        let stored: RelationshipSourceDurableOperationResultV1 =
            serde_json::from_value(stored_value).map_err(|error| {
                SubjectSoulStoreFailure::repair(format!(
                    "durable relationship operation result is invalid: {error}"
                ))
            })?;
        stored
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::from_store)?;
        if stored.identity != expected_result.identity
            || stored.committed_report.transaction_id
                != expected_result.committed_report.transaction_id
            || stored.committed_report.intent_digest
                != expected_result.committed_report.intent_digest
        {
            return Err(SubjectSoulStoreFailure::repair(
                "durable relationship result conflicts with the typed operation identity",
            ));
        }
        match outcome {
            StoreMutationOperationOutcome::Committed { .. } => {
                Ok(RelationshipSourceStoreMutationOutcome::Committed {
                    result: stored.committed_report,
                })
            }
            StoreMutationOperationOutcome::Replayed { .. } => {
                let mut result = stored.committed_report;
                result.outcome = RelationshipSourceControlOutcomeV1::Replayed;
                result.replayed = true;
                result
                    .validate_contract()
                    .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
                Ok(RelationshipSourceStoreMutationOutcome::Replayed { result })
            }
        }
    }

    pub(crate) fn read_verified_relationship_source(
        &self,
        memory_space_id: &str,
        relationship_id: &str,
        revision: Option<u64>,
        runtime_budget: &RuntimeBudgetReport,
    ) -> std::result::Result<RelationshipSourceVerifiedStoreRead, SubjectSoulStoreFailure> {
        runtime_budget
            .validate_for_admission(current_unix_secs())
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let capacity = StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        let mut session = self
            .engine_for_subject_soul()
            .open_immutable_read_session(capacity)
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let manifest_key =
            super::schema::relationship_source_scope_key(memory_space_id, relationship_id)
                .map_err(SubjectSoulStoreFailure::from_store)?;
        let manifest_reads = session
            .read_json_known_keys(&[(
                RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                manifest_key.clone(),
            )])
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let manifest: RelationshipSourceScopeManifestV1 = decode_required_read(
            &manifest_reads,
            RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE,
            &manifest_key,
        )?;
        manifest
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::contract)?;
        if manifest.memory_space_id != memory_space_id
            || manifest.relationship_id != relationship_id
        {
            return Err(SubjectSoulStoreFailure::repair(
                "relationship manifest is bound to a different owner",
            ));
        }
        let revision = revision.unwrap_or(manifest.current_revision);
        let source_key =
            relationship_source_revision_key(memory_space_id, relationship_id, revision)
                .map_err(SubjectSoulStoreFailure::from_store)?;
        if revision != manifest.current_revision
            && !manifest.retained_revision_refs.contains(&source_key)
        {
            return Err(SubjectSoulStoreFailure::repair(
                "exact relationship revision is outside the retained source closure",
            ));
        }
        let source_reads = session
            .read_json_known_keys(&[(
                RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                source_key.clone(),
            )])
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let source: RelationshipSourceConstitutionV1 = decode_required_read(
            &source_reads,
            RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE,
            &source_key,
        )?;
        source
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::contract)?;
        if source.memory_space_id != memory_space_id
            || source.relationship_id != relationship_id
            || source.revision != revision
            || (revision == manifest.current_revision
                && source.content_digest != manifest.current_digest)
        {
            return Err(SubjectSoulStoreFailure::repair(
                "relationship source is stale or bound to a different manifest root",
            ));
        }
        let receipt = session
            .receipt()
            .map_err(SubjectSoulStoreFailure::from_store)?;
        Ok(RelationshipSourceVerifiedStoreRead {
            source,
            manifest,
            receipt,
        })
    }

    pub(crate) fn commit_subject_soul_mutation_with_runtime_budget(
        &self,
        plan: SubjectSoulStoreMutationPlan,
        runtime_budget: &RuntimeBudgetReport,
    ) -> std::result::Result<SubjectSoulStoreMutationOutcome, SubjectSoulStoreFailure> {
        if !plan.composite_binding_verified {
            return Err(SubjectSoulStoreFailure::repair(
                "Subject Soul operation has an unverified full owner-plan intent",
            ));
        }
        if let SubjectSoulExpectedStateV1::PristineAbsent {
            closure_certificate_digest,
        } = &plan.expected_state
        {
            if closure_certificate_digest != self.subject_soul_open_closure_certificate().digest()
                || !self
                    .subject_soul_open_closure_certificate()
                    .proves_zero_artifacts(
                        &plan.post_image.head.memory_space_id,
                        &plan.post_image.head.subject_id,
                        &plan.post_image.head.soul_id,
                    )
            {
                return Err(expected_state_failure(
                    "pristine Subject Soul is not proven by the pinned Store-open closure certificate",
                ));
            }
        }
        let expected_result = plan.durable_result.clone();
        let outcome = self
            .commit_memory_mutation_operation_with_blob_preconditions_and_runtime_budget(
                plan.batch,
                &plan.preconditions,
                &plan.blob_preconditions,
                plan.operation,
                runtime_budget,
            )
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let stored_result = self
            .engine_for_subject_soul()
            .read_consistent_known_keys(
                &[(
                    SUBJECT_SOUL_OPERATION_RESULT_NAMESPACE.to_string(),
                    expected_result.identity.storage_key(),
                )],
                &[],
                false,
                StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget),
            )
            .map_err(SubjectSoulStoreFailure::from_store)?
            .json
            .into_iter()
            .next()
            .and_then(|read| read.value)
            .ok_or_else(|| {
                SubjectSoulStoreFailure::repair(
                    "committed/replayed Soul operation is missing its durable safe result",
                )
            })?;
        let stored_result: SubjectSoulDurableOperationResultV1 =
            serde_json::from_value(stored_result).map_err(|error| {
                SubjectSoulStoreFailure::repair(format!(
                    "durable Soul operation result is invalid: {error}"
                ))
            })?;
        stored_result
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::from_store)?;
        if stored_result.identity != expected_result.identity
            || stored_result.soul_id != expected_result.soul_id
            || stored_result.committed_report.transaction_id
                != expected_result.committed_report.transaction_id
        {
            return Err(SubjectSoulStoreFailure::repair(
                "durable Soul operation result conflicts with the typed operation identity",
            ));
        }
        match outcome {
            StoreMutationOperationOutcome::Committed { .. } => {
                Ok(SubjectSoulStoreMutationOutcome::Committed {
                    result: stored_result.committed_report,
                })
            }
            StoreMutationOperationOutcome::Replayed { .. } => {
                let mut result = stored_result.committed_report;
                result.outcome = SubjectSoulMutationOutcomeV1::Replayed;
                result.replayed = true;
                result
                    .validate_contract()
                    .map_err(SubjectSoulStoreFailure::contract)?;
                Ok(SubjectSoulStoreMutationOutcome::Replayed { result })
            }
        }
    }

    pub(crate) fn read_verified_subject_soul(
        &self,
        memory_space_id: &str,
        soul_id: &str,
        request: &SubjectSoulReadRequestV1,
        runtime_budget: &RuntimeBudgetReport,
    ) -> std::result::Result<SubjectSoulVerifiedStoreRead, SubjectSoulStoreFailure> {
        request
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::contract)?;
        runtime_budget
            .validate_for_admission(current_unix_secs())
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let capacity = StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        let mut session = self
            .engine_for_subject_soul()
            .open_immutable_read_session(capacity)
            .map_err(SubjectSoulStoreFailure::from_store)?;
        read_verified_subject_soul_in_session(
            session.as_mut(),
            memory_space_id,
            &request.target_subject_id,
            soul_id,
            request,
            self.subject_soul_open_closure_certificate(),
        )
    }

    pub(crate) fn read_verified_subject_soul_for_autonomous_cycle(
        &self,
        memory_space_id: &str,
        soul_id: &str,
        request: &SubjectSoulReadRequestV1,
        relationship_id: Option<&str>,
        runtime_budget: &RuntimeBudgetReport,
    ) -> std::result::Result<SubjectSoulVerifiedStoreRead, SubjectSoulStoreFailure> {
        request
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::contract)?;
        runtime_budget
            .validate_for_admission(current_unix_secs())
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let capacity = StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
        let mut session = self
            .engine_for_subject_soul()
            .open_immutable_read_session(capacity)
            .map_err(SubjectSoulStoreFailure::from_store)?;
        read_verified_subject_soul_with_relationship_in_session(
            session.as_mut(),
            memory_space_id,
            &request.target_subject_id,
            soul_id,
            request,
            relationship_id,
            self.subject_soul_open_closure_certificate(),
        )
    }
}

fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn subject_soul_session_receipt(
    session: &dyn StoreImmutableReadSession,
    issue_receipt: bool,
) -> std::result::Result<StoreReadReceipt, SubjectSoulStoreFailure> {
    if issue_receipt {
        session
            .receipt()
            .map_err(SubjectSoulStoreFailure::from_store)
    } else {
        Ok(StoreReadReceipt::default())
    }
}

pub(crate) fn read_verified_subject_soul_in_session(
    session: &mut dyn StoreImmutableReadSession,
    memory_space_id: &str,
    subject_id: &str,
    soul_id: &str,
    request: &SubjectSoulReadRequestV1,
    certificate: &StoreOpenClosureCertificate,
) -> std::result::Result<SubjectSoulVerifiedStoreRead, SubjectSoulStoreFailure> {
    read_verified_subject_soul_in_session_with_receipt(
        session,
        memory_space_id,
        subject_id,
        soul_id,
        request,
        certificate,
        true,
    )
}

pub(crate) fn read_verified_subject_soul_with_relationship_in_session(
    session: &mut dyn StoreImmutableReadSession,
    memory_space_id: &str,
    subject_id: &str,
    soul_id: &str,
    request: &SubjectSoulReadRequestV1,
    relationship_id: Option<&str>,
    certificate: &StoreOpenClosureCertificate,
) -> std::result::Result<SubjectSoulVerifiedStoreRead, SubjectSoulStoreFailure> {
    let mut soul = read_verified_subject_soul_in_session_with_receipt(
        session,
        memory_space_id,
        subject_id,
        soul_id,
        request,
        certificate,
        false,
    )?;
    if let Some(relationship_id) = relationship_id {
        if soul.relationship_source(relationship_id)?.is_none() {
            if let Some((source, manifest)) = read_optional_current_relationship_source_in_session(
                session,
                memory_space_id,
                relationship_id,
            )? {
                if source.mounted_subject_id != subject_id {
                    return Err(SubjectSoulStoreFailure::repair(
                        "relationship source is bound to a different mounted Subject Soul",
                    ));
                }
                let source_key = relationship_source_revision_key(
                    memory_space_id,
                    relationship_id,
                    source.revision,
                )
                .map_err(SubjectSoulStoreFailure::from_store)?;
                let manifest_key =
                    super::schema::relationship_source_scope_key(memory_space_id, relationship_id)
                        .map_err(SubjectSoulStoreFailure::from_store)?;
                soul.closure_documents.extend([
                    (
                        (
                            RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                            source_key,
                        ),
                        serde_json::to_value(&source)
                            .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?,
                    ),
                    (
                        (
                            RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                            manifest_key,
                        ),
                        serde_json::to_value(&manifest)
                            .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?,
                    ),
                ]);
                soul.relationship_sources
                    .insert(relationship_id.to_string(), source);
                soul.relationship_source_manifests
                    .insert(relationship_id.to_string(), manifest);
            }
        }
    }
    soul.receipt = session
        .receipt()
        .map_err(SubjectSoulStoreFailure::from_store)?;
    Ok(soul)
}

fn read_verified_subject_soul_in_session_with_receipt(
    session: &mut dyn StoreImmutableReadSession,
    memory_space_id: &str,
    subject_id: &str,
    soul_id: &str,
    request: &SubjectSoulReadRequestV1,
    certificate: &StoreOpenClosureCertificate,
    issue_receipt: bool,
) -> std::result::Result<SubjectSoulVerifiedStoreRead, SubjectSoulStoreFailure> {
    let scope_key = subject_soul_scope_key(memory_space_id, subject_id, soul_id)
        .map_err(SubjectSoulStoreFailure::from_store)?;
    let roots = session
        .read_json_known_keys(&[
            (
                SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.to_string(),
                scope_key.clone(),
            ),
            (
                SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                scope_key.clone(),
            ),
        ])
        .map_err(SubjectSoulStoreFailure::from_store)?;
    let root_value = |namespace: &str| {
        roots
            .iter()
            .find(|read| read.namespace == namespace && read.key == scope_key)
            .and_then(|read| read.value.as_ref())
    };
    let (head_value, manifest_value) = (
        root_value(SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE),
        root_value(SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE),
    );
    if head_value.is_none() && manifest_value.is_none() {
        if !matches!(request.selector, SubjectSoulReadSelectorV1::Current)
            || !certificate.proves_zero_artifacts(memory_space_id, subject_id, soul_id)
        {
            return Err(SubjectSoulStoreFailure::repair(
                "pristine Subject Soul exact read is unavailable without persisted roots",
            ));
        }
        let outcome = SubjectSoulReadOutcomeV1::ImplicitUnseeded {
            memory_space_id: memory_space_id.to_string(),
            subject_id: subject_id.to_string(),
            soul_id: soul_id.to_string(),
            generation: 1,
            closure_certificate_digest: certificate.digest().to_string(),
        };
        outcome
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::contract)?;
        let receipt = subject_soul_session_receipt(session, issue_receipt)?;
        return Ok(SubjectSoulVerifiedStoreRead {
            outcome,
            head: None,
            manifest: None,
            selected_material: None,
            current_material: None,
            current_core: None,
            current_core_document: None,
            current_ledger: None,
            current_ledger_document: None,
            closure_documents: Vec::new(),
            relationship_sources: BTreeMap::new(),
            relationship_source_manifests: BTreeMap::new(),
            receipt,
        });
    }
    let (Some(head_value), Some(manifest_value)) = (head_value, manifest_value) else {
        return Err(SubjectSoulStoreFailure::repair(
            "Subject Soul lifecycle head/manifest root is partially missing",
        ));
    };
    let head: SubjectSoulLifecycleHeadV1 = serde_json::from_value(head_value.clone())
        .map_err(|error| SubjectSoulStoreFailure::repair(format!("invalid Soul head: {error}")))?;
    let manifest: SubjectSoulScopeManifestV1 = serde_json::from_value(manifest_value.clone())
        .map_err(|error| {
            SubjectSoulStoreFailure::repair(format!("invalid Soul manifest: {error}"))
        })?;
    head.validate_contract()
        .map_err(SubjectSoulStoreFailure::contract)?;
    manifest
        .validate_contract()
        .map_err(SubjectSoulStoreFailure::contract)?;
    if head.memory_space_id != memory_space_id
        || head.subject_id != subject_id
        || head.soul_id != soul_id
        || manifest.memory_space_id != memory_space_id
        || manifest.subject_id != subject_id
        || manifest.soul_id != soul_id
        || head.scope_manifest_digest != manifest.closure_digest
    {
        return Err(SubjectSoulStoreFailure::repair(
            "verified read roots are bound to a different Subject Soul owner",
        ));
    }
    let selected_material_ref = match &request.selector {
        SubjectSoulReadSelectorV1::Current => head.current_revision.map(|revision| {
            (
                head.generation,
                revision,
                head.current_material_digest.clone().unwrap_or_default(),
            )
        }),
        SubjectSoulReadSelectorV1::Exact {
            generation,
            revision,
            material_digest,
        } => Some((*generation, *revision, material_digest.clone())),
    };
    let selected_material_address = selected_material_ref
        .as_ref()
        .map(|(generation, revision, _)| {
            subject_soul_revision_material_key(
                memory_space_id,
                subject_id,
                soul_id,
                *generation,
                *revision,
            )
        })
        .transpose()
        .map_err(SubjectSoulStoreFailure::from_store)?;
    if let (Some((generation, revision, digest)), Some(key)) = (
        selected_material_ref.as_ref(),
        selected_material_address.as_ref(),
    ) {
        let selected_raw_is_owned = manifest.entries.iter().any(|entry| {
            entry.namespace == SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE
                && entry.physical_key == *key
                && entry.content_digest == *digest
        }) || head.retained_revision_refs.contains(key);
        if !selected_raw_is_owned {
            if matches!(request.selector, SubjectSoulReadSelectorV1::Exact { .. }) {
                return read_terminated_subject_soul_in_session(
                    session,
                    &head,
                    &manifest,
                    &scope_key,
                    TerminatedSubjectSoulSelector {
                        generation: *generation,
                        revision: *revision,
                        material_digest: digest,
                    },
                    issue_receipt,
                );
            }
            return Err(SubjectSoulStoreFailure::repair(
                "current Soul material is outside the exact owner closure",
            ));
        }
    }
    let mut closure_addresses = manifest
        .entries
        .iter()
        .map(|entry| (entry.namespace.clone(), entry.physical_key.clone()))
        .collect::<BTreeSet<_>>();
    if let Some(key) = selected_material_address.as_ref() {
        closure_addresses.insert((
            SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE.to_string(),
            key.clone(),
        ));
    }
    for key in &head.retained_revision_refs {
        closure_addresses.insert((
            SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE.to_string(),
            key.clone(),
        ));
    }
    for key in &head.retained_tombstone_refs {
        closure_addresses.insert((
            SUBJECT_SOUL_GENERATION_TOMBSTONE_NAMESPACE.to_string(),
            key.clone(),
        ));
    }
    let closure_reads = session
        .read_json_known_keys(&closure_addresses.into_iter().collect::<Vec<_>>())
        .map_err(SubjectSoulStoreFailure::from_store)?;
    let mut closure_map = BTreeMap::from([
        (
            (
                SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.to_string(),
                scope_key.clone(),
            ),
            serde_json::to_value(&head).map_err(|error| {
                SubjectSoulStoreFailure::repair(format!("cannot encode verified head: {error}"))
            })?,
        ),
        (
            (
                SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                scope_key.clone(),
            ),
            serde_json::to_value(&manifest).map_err(|error| {
                SubjectSoulStoreFailure::repair(format!("cannot encode verified manifest: {error}"))
            })?,
        ),
    ]);
    for read in &closure_reads {
        let value = read.value.clone().ok_or_else(|| {
            SubjectSoulStoreFailure::repair(format!(
                "manifest-owned document is missing: {}/{}",
                read.namespace, read.key
            ))
        })?;
        closure_map.insert((read.namespace.clone(), read.key.clone()), value);
    }
    let relationship_addresses = closure_reads
        .iter()
        .filter(|read| read.namespace == SUBJECT_SOUL_RELATIONSHIP_PROJECTION_NAMESPACE)
        .map(|read| {
            let projection: SubjectSoulRelationshipProjectionV1 = read
                .value
                .as_ref()
                .ok_or_else(|| SubjectSoulStoreFailure::repair("projection disappeared"))
                .and_then(|value| {
                    serde_json::from_value(value.clone()).map_err(|error| {
                        SubjectSoulStoreFailure::repair(format!(
                            "invalid relationship projection: {error}"
                        ))
                    })
                })?;
            Ok::<_, SubjectSoulStoreFailure>([
                (
                    RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                    relationship_source_revision_key(
                        &projection.memory_space_id,
                        &projection.relationship_id,
                        projection.relationship_source_revision,
                    )
                    .map_err(SubjectSoulStoreFailure::from_store)?,
                ),
                (
                    RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                    super::schema::relationship_source_scope_key(
                        &projection.memory_space_id,
                        &projection.relationship_id,
                    )
                    .map_err(SubjectSoulStoreFailure::from_store)?,
                ),
            ])
        })
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    if !relationship_addresses.is_empty() {
        let relationship_reads = session
            .read_json_known_keys(&relationship_addresses.into_iter().collect::<Vec<_>>())
            .map_err(SubjectSoulStoreFailure::from_store)?;
        let mut retained_addresses = BTreeSet::new();
        for read in relationship_reads {
            let value = read.value.ok_or_else(|| {
                SubjectSoulStoreFailure::repair(format!(
                    "relationship root is missing: {}/{}",
                    read.namespace, read.key
                ))
            })?;
            if read.namespace == RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE {
                let relationship_manifest: RelationshipSourceScopeManifestV1 =
                    serde_json::from_value(value.clone()).map_err(|error| {
                        SubjectSoulStoreFailure::repair(format!(
                            "invalid relationship manifest: {error}"
                        ))
                    })?;
                let current_address = (
                    RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                    relationship_source_revision_key(
                        &relationship_manifest.memory_space_id,
                        &relationship_manifest.relationship_id,
                        relationship_manifest.current_revision,
                    )
                    .map_err(SubjectSoulStoreFailure::from_store)?,
                );
                if !closure_map.contains_key(&current_address) {
                    retained_addresses.insert(current_address);
                }
                for key in &relationship_manifest.retained_revision_refs {
                    let address = (
                        RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                        key.clone(),
                    );
                    if !closure_map.contains_key(&address) {
                        retained_addresses.insert(address);
                    }
                }
            }
            closure_map.insert((read.namespace, read.key), value);
        }
        if !retained_addresses.is_empty() {
            for read in session
                .read_json_known_keys(&retained_addresses.into_iter().collect::<Vec<_>>())
                .map_err(SubjectSoulStoreFailure::from_store)?
            {
                let value = read.value.ok_or_else(|| {
                    SubjectSoulStoreFailure::repair(format!(
                        "retained relationship root is missing: {}/{}",
                        read.namespace, read.key
                    ))
                })?;
                closure_map.insert((read.namespace, read.key), value);
            }
        }
    }
    validate_subject_soul_closure_map(&closure_map).map_err(SubjectSoulStoreFailure::from_store)?;
    let selected_material = if let (Some((_, _, digest)), Some(key)) = (
        selected_material_ref.as_ref(),
        selected_material_address.as_ref(),
    ) {
        let value = closure_map
            .get(&(
                SUBJECT_SOUL_REVISION_MATERIAL_NAMESPACE.to_string(),
                key.clone(),
            ))
            .ok_or_else(|| SubjectSoulStoreFailure::repair("selected material is missing"))?;
        let material: SubjectSoulRevisionMaterialV1 = serde_json::from_value(value.clone())
            .map_err(|error| {
                SubjectSoulStoreFailure::repair(format!("invalid selected material: {error}"))
            })?;
        if &material.content_digest != digest {
            return Err(SubjectSoulStoreFailure::repair(
                "exact Soul material digest differs from the selector/root",
            ));
        }
        Some(material)
    } else {
        None
    };
    let mut current_core = None;
    let mut current_core_document = None;
    let mut current_ledger = None;
    let mut current_ledger_document = None;
    for entry in &manifest.entries {
        if entry.revision != head.current_revision {
            continue;
        }
        let Some(value) = closure_map.get(&(entry.namespace.clone(), entry.physical_key.clone()))
        else {
            continue;
        };
        if entry.namespace == "self_authored_core" {
            let envelope: SubjectSoulOwnedDocumentV1 = serde_json::from_value(value.clone())
                .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
            current_core = Some(serde_json::from_value(envelope.body.clone()).map_err(
                |error| SubjectSoulStoreFailure::repair(format!("invalid current Core: {error}")),
            )?);
            current_core_document = Some(envelope);
        } else if entry.namespace == "core_revision_ledger" {
            let envelope: SubjectSoulOwnedDocumentV1 = serde_json::from_value(value.clone())
                .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
            current_ledger = Some(
                serde_json::from_value::<CoreRevisionLedger>(envelope.body.clone()).map_err(
                    |error| {
                        SubjectSoulStoreFailure::repair(format!(
                            "invalid current Core ledger: {error}"
                        ))
                    },
                )?,
            );
            current_ledger_document = Some(envelope);
        }
    }
    let current_material = head.current_revision.and_then(|revision| {
        closure_map.values().find_map(|value| {
            serde_json::from_value::<SubjectSoulRevisionMaterialV1>(value.clone())
                .ok()
                .filter(|material| {
                    material.generation == head.generation && material.revision == revision
                })
        })
    });
    SubjectSoulVerifiedSnapshotV1 {
        head: head.clone(),
        manifest: manifest.clone(),
        current_material: current_material.clone(),
        current_core: current_core.clone(),
        current_core_document: current_core_document.clone(),
        current_revision_ledger: current_ledger.clone(),
        current_revision_ledger_document: current_ledger_document.clone(),
    }
    .validate_contract()
    .map_err(SubjectSoulStoreFailure::contract)?;
    let historical_exact = matches!(request.selector, SubjectSoulReadSelectorV1::Exact { .. });
    let view_state = if historical_exact && selected_material.is_some() {
        SubjectSoulLifecycleStateV1::Active
    } else {
        head.state
    };
    let view = VerifiedSubjectSoulReadViewV1 {
        memory_space_id: head.memory_space_id.clone(),
        subject_id: head.subject_id.clone(),
        soul_id: head.soul_id.clone(),
        state: view_state,
        generation: selected_material
            .as_ref()
            .map_or(head.generation, |value| value.generation),
        revision: selected_material.as_ref().map(|value| value.revision),
        material_digest: selected_material
            .as_ref()
            .map(|value| value.content_digest.clone()),
        origin: selected_material
            .as_ref()
            .map(|value| value.provenance.origin),
        requested_view: request.view,
        runtime_private_core: matches!(request.view, SubjectSoulReadViewV1::RuntimePrivate)
            .then(|| selected_material.as_ref().map(|value| value.core.clone()))
            .flatten(),
        governed_disclosure: None,
        head_digest: head.head_digest.clone(),
        manifest_digest: manifest.closure_digest.clone(),
    };
    view.validate_contract()
        .map_err(SubjectSoulStoreFailure::contract)?;
    let receipt = subject_soul_session_receipt(session, issue_receipt)?;
    let closure_documents = closure_map.into_iter().collect::<Vec<_>>();
    let VerifiedRelationshipRoots {
        sources: relationship_sources,
        manifests: relationship_source_manifests,
    } = relationship_roots_from_closure(&closure_documents)?;
    Ok(SubjectSoulVerifiedStoreRead {
        outcome: SubjectSoulReadOutcomeV1::Verified {
            view: Box::new(view),
        },
        head: Some(head),
        manifest: Some(manifest),
        selected_material,
        current_material,
        current_core,
        current_core_document,
        current_ledger,
        current_ledger_document,
        closure_documents,
        relationship_sources,
        relationship_source_manifests,
        receipt,
    })
}

struct TerminatedSubjectSoulSelector<'a> {
    generation: u64,
    revision: u64,
    material_digest: &'a str,
}

fn read_terminated_subject_soul_in_session(
    session: &mut dyn StoreImmutableReadSession,
    head: &SubjectSoulLifecycleHeadV1,
    manifest: &SubjectSoulScopeManifestV1,
    scope_key: &str,
    selector: TerminatedSubjectSoulSelector<'_>,
    issue_receipt: bool,
) -> std::result::Result<SubjectSoulVerifiedStoreRead, SubjectSoulStoreFailure> {
    let addresses = head
        .retained_tombstone_refs
        .iter()
        .map(|key| {
            (
                SUBJECT_SOUL_GENERATION_TOMBSTONE_NAMESPACE.to_string(),
                key.clone(),
            )
        })
        .collect::<Vec<_>>();
    let reads = session
        .read_json_known_keys(&addresses)
        .map_err(SubjectSoulStoreFailure::from_store)?;
    let mut matched = None;
    let mut observed_generation = false;
    let mut closure_documents = vec![
        (
            (
                SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.to_string(),
                scope_key.to_string(),
            ),
            serde_json::to_value(head)
                .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?,
        ),
        (
            (
                SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                scope_key.to_string(),
            ),
            serde_json::to_value(manifest)
                .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?,
        ),
    ];
    for read in reads {
        let value = read
            .value
            .ok_or_else(|| SubjectSoulStoreFailure::repair("retained Soul tombstone is missing"))?;
        let tombstone: SubjectSoulGenerationTombstoneV1 = serde_json::from_value(value.clone())
            .map_err(|error| SubjectSoulStoreFailure::repair(error.to_string()))?;
        tombstone
            .validate_contract()
            .map_err(SubjectSoulStoreFailure::contract)?;
        let canonical_key = subject_soul_generation_tombstone_key(
            &tombstone.memory_space_id,
            &tombstone.subject_id,
            &tombstone.soul_id,
            tombstone.generation,
        )
        .map_err(SubjectSoulStoreFailure::from_store)?;
        if canonical_key != read.key
            || tombstone.memory_space_id != head.memory_space_id
            || tombstone.subject_id != head.subject_id
            || tombstone.soul_id != head.soul_id
        {
            return Err(SubjectSoulStoreFailure::repair(
                "retained Soul tombstone owner/address mismatch",
            ));
        }
        if tombstone.generation == selector.generation {
            observed_generation = true;
            if tombstone.terminal_revision == Some(selector.revision)
                && tombstone.terminal_material_digest.as_deref() == Some(selector.material_digest)
            {
                matched = Some(tombstone.clone());
            }
        }
        closure_documents.push(((read.namespace, read.key), value));
    }
    let Some(tombstone) = matched else {
        return Err(expected_state_failure(if observed_generation {
            "terminated Soul selector revision/material digest conflicts with its tombstone"
        } else {
            "exact Soul selector is outside current and terminated generation metadata"
        }));
    };
    let terminal = SubjectSoulTerminatedGenerationV1 {
        generation: tombstone.generation,
        terminal_revision: tombstone.terminal_revision,
        terminal_material_digest: tombstone.terminal_material_digest,
        terminal_action: tombstone.terminal_action,
        tombstone_digest: tombstone.tombstone_digest,
        terminated_at: tombstone.terminated_at,
        current_generation: head.generation,
        current_state: head.state,
    };
    terminal
        .validate_contract()
        .map_err(SubjectSoulStoreFailure::contract)?;
    let outcome = SubjectSoulReadOutcomeV1::TerminatedGeneration {
        memory_space_id: head.memory_space_id.clone(),
        subject_id: head.subject_id.clone(),
        soul_id: head.soul_id.clone(),
        terminal: Box::new(terminal),
    };
    outcome
        .validate_contract()
        .map_err(SubjectSoulStoreFailure::contract)?;
    let receipt = subject_soul_session_receipt(session, issue_receipt)?;
    let VerifiedRelationshipRoots {
        sources: relationship_sources,
        manifests: relationship_source_manifests,
    } = relationship_roots_from_closure(&closure_documents)?;
    Ok(SubjectSoulVerifiedStoreRead {
        outcome,
        head: Some(head.clone()),
        manifest: Some(manifest.clone()),
        selected_material: None,
        current_material: None,
        current_core: None,
        current_core_document: None,
        current_ledger: None,
        current_ledger_document: None,
        closure_documents,
        relationship_sources,
        relationship_source_manifests,
        receipt,
    })
}

fn read_optional_current_relationship_source_in_session(
    session: &mut dyn StoreImmutableReadSession,
    memory_space_id: &str,
    relationship_id: &str,
) -> std::result::Result<
    Option<(
        RelationshipSourceConstitutionV1,
        RelationshipSourceScopeManifestV1,
    )>,
    SubjectSoulStoreFailure,
> {
    let manifest_key =
        super::schema::relationship_source_scope_key(memory_space_id, relationship_id)
            .map_err(SubjectSoulStoreFailure::from_store)?;
    let manifest_reads = session
        .read_json_known_keys(&[(
            RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE.to_string(),
            manifest_key.clone(),
        )])
        .map_err(SubjectSoulStoreFailure::from_store)?;
    let manifest_value = manifest_reads
        .iter()
        .find(|read| {
            read.namespace == RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE
                && read.key == manifest_key
        })
        .and_then(|read| read.value.as_ref());
    let Some(manifest_value) = manifest_value else {
        return Ok(None);
    };
    let manifest: RelationshipSourceScopeManifestV1 =
        serde_json::from_value(manifest_value.clone()).map_err(|error| {
            SubjectSoulStoreFailure::repair(format!(
                "invalid relationship source manifest: {error}"
            ))
        })?;
    manifest
        .validate_contract()
        .map_err(SubjectSoulStoreFailure::contract)?;
    if manifest.memory_space_id != memory_space_id || manifest.relationship_id != relationship_id {
        return Err(SubjectSoulStoreFailure::repair(
            "relationship source manifest is bound to a different owner",
        ));
    }
    let source_key = relationship_source_revision_key(
        memory_space_id,
        relationship_id,
        manifest.current_revision,
    )
    .map_err(SubjectSoulStoreFailure::from_store)?;
    let source_reads = session
        .read_json_known_keys(&[(
            RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
            source_key.clone(),
        )])
        .map_err(SubjectSoulStoreFailure::from_store)?;
    let source: RelationshipSourceConstitutionV1 = decode_required_read(
        &source_reads,
        RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE,
        &source_key,
    )?;
    source
        .validate_contract()
        .map_err(SubjectSoulStoreFailure::contract)?;
    if source.memory_space_id != memory_space_id
        || source.relationship_id != relationship_id
        || source.revision != manifest.current_revision
        || source.content_digest != manifest.current_digest
    {
        return Err(SubjectSoulStoreFailure::repair(
            "relationship source current post-image differs from its manifest root",
        ));
    }
    Ok(Some((source, manifest)))
}

fn decode_required_read<T: DeserializeOwned>(
    reads: &[super::transaction::StoreBoundedKnownJsonRead],
    namespace: &str,
    key: &str,
) -> std::result::Result<T, SubjectSoulStoreFailure> {
    let value = reads
        .iter()
        .find(|read| read.namespace == namespace && read.key == key)
        .and_then(|read| read.value.as_ref())
        .ok_or_else(|| SubjectSoulStoreFailure::repair(format!("missing {namespace}/{key}")))?;
    serde_json::from_value(value.clone()).map_err(|error| {
        SubjectSoulStoreFailure::repair(format!("invalid {namespace}/{key}: {error}"))
    })
}

#[cfg(all(test, feature = "nonproduction-replay-harness"))]
pub(super) mod tests {
    use super::*;
    use bm_core::feature_gate::ProfileId;
    use bm_core::memory::{
        plan_relationship_source_control, MemoryMutationEffect, MemoryMutationOperationIdentity,
        MemoryMutationOperationKind, RelationshipAccessConstraintV1,
        RelationshipDisclosureCeilingV1, RelationshipSourceClausesV1,
        RelationshipSourceControlAuthorityV1, RelationshipSourceControlIntentActionV1,
        RelationshipSourceControlIntentV1, SubjectSoulLifecycleStateV1,
        SubjectSoulManifestAddressV1, SubjectSoulOwnerV1, SubjectSoulReadViewV1,
    };
    use std::sync::Arc;

    struct KnownKeySession {
        documents: BTreeMap<(String, String), Value>,
        read_state: super::super::transaction::StoreReadSessionState,
    }

    impl KnownKeySession {
        fn new(documents: BTreeMap<(String, String), Value>) -> Self {
            Self {
                documents,
                read_state: super::super::transaction::StoreReadSessionState::new(
                    StoreCapacityBudget::full(),
                ),
            }
        }
    }

    impl StoreImmutableReadSession for KnownKeySession {
        fn read_json_known_keys(
            &mut self,
            addresses: &[(String, String)],
        ) -> Result<Vec<super::super::transaction::StoreBoundedKnownJsonRead>> {
            addresses
                .iter()
                .map(|(namespace, key)| {
                    self.read_state.record_json(
                        namespace,
                        key,
                        self.documents
                            .get(&(namespace.clone(), key.clone()))
                            .cloned(),
                    )
                })
                .collect()
        }

        fn read_blob_known_keys(
            &mut self,
            addresses: &[(String, String)],
        ) -> Result<Vec<super::super::transaction::StoreBoundedKnownBlobRead>> {
            addresses
                .iter()
                .map(|(namespace, key)| self.read_state.record_blob(namespace, key, None))
                .collect()
        }

        fn receipt(&self) -> Result<StoreReadReceipt> {
            self.read_state.receipt()
        }
    }

    fn relationship_source_fixture() -> (
        RelationshipSourceConstitutionV1,
        RelationshipSourceScopeManifestV1,
    ) {
        let intent = RelationshipSourceControlIntentV1 {
            operation_id: "relationship-source-known-key-read".to_string(),
            memory_space_id: "space:relationship-read".to_string(),
            relationship_id: "relationship:known-key".to_string(),
            mounted_subject_id: "subject:agent".to_string(),
            counterparty_subject_ids: vec!["subject:human".to_string()],
            expected_state: RelationshipSourceExpectedStateV1::PristineAbsent {
                closure_certificate_digest: "a".repeat(64),
            },
            authority: RelationshipSourceControlAuthorityV1::HumanUser {
                actor_subject_id: "subject:human".to_string(),
            },
            action: RelationshipSourceControlIntentActionV1::Create {
                clauses: RelationshipSourceClausesV1 {
                    disclosure_ceiling: RelationshipDisclosureCeilingV1::GovernedSummary,
                    access_constraints: vec![
                        RelationshipAccessConstraintV1::NoPrivateRaw,
                        RelationshipAccessConstraintV1::GovernedDisclosureOnly,
                    ],
                    truth_commitments: vec!["state uncertainty".to_string()],
                    mutual_boundary_commitments: vec!["respect refusal".to_string()],
                    repair_commitments: vec!["repair before escalation".to_string()],
                },
                source_asserted_at: Some(7),
                evidence_digest: "b".repeat(64),
            },
        };
        let plan = plan_relationship_source_control(&intent, None, None, 11)
            .expect("relationship fixture post-image");
        (plan.post_source, plan.post_manifest)
    }

    fn native_profile() -> ProfileId {
        ProfileId::native_dev_full().expect("native test profile")
    }

    fn persistent_profile() -> ProfileId {
        #[cfg(target_os = "macos")]
        return ProfileId::DesktopMacosEmbeddedSdk;
        #[cfg(target_os = "windows")]
        return ProfileId::DesktopWindowsEmbeddedSdk;
        #[cfg(target_os = "linux")]
        return ProfileId::DesktopLinuxEmbeddedSdk;
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        compile_error!("SPV1 Store contracts require a supported host target");
    }

    pub(crate) fn unseeded_plan_for_scope(
        platform: &StorePlatform,
        operation_id: &str,
        memory_space_id: &str,
        subject_id: &str,
        soul_id: &str,
        now: u64,
        scope: super::super::StoreEventScope,
    ) -> SubjectSoulStoreMutationPlan {
        let mut manifest = SubjectSoulScopeManifestV1 {
            schema_version: bm_core::memory::SUBJECT_SOUL_SCHEMA_VERSION,
            memory_space_id: memory_space_id.to_string(),
            subject_id: subject_id.to_string(),
            soul_id: soul_id.to_string(),
            generation: 1,
            manifest_revision: 1,
            entries: Vec::new(),
            closure_digest: String::new(),
        };
        manifest.refresh_digest().expect("manifest digest");
        let mut head = SubjectSoulLifecycleHeadV1 {
            schema_version: bm_core::memory::SUBJECT_SOUL_SCHEMA_VERSION,
            memory_space_id: memory_space_id.to_string(),
            subject_id: subject_id.to_string(),
            soul_id: soul_id.to_string(),
            generation: 1,
            state: SubjectSoulLifecycleStateV1::Unseeded,
            current_revision: None,
            current_material_digest: None,
            current_ledger_digest: None,
            scope_manifest_digest: manifest.closure_digest.clone(),
            retained_revision_refs: Vec::new(),
            retained_tombstone_refs: Vec::new(),
            updated_at: now,
            head_digest: String::new(),
        };
        head.refresh_digest().expect("head digest");
        let identity = MemoryMutationOperationIdentity::new(
            operation_id,
            memory_space_id,
            subject_id,
            subject_id,
            MemoryMutationOperationKind::SoulEvidence,
        )
        .expect("operation identity");
        let operation = StoreMutationOperationPlan::new(
            identity,
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            MemoryMutationEffect::Changed,
            2,
            subject_id,
            now,
        )
        .expect("operation plan");
        let scope_key =
            subject_soul_scope_key(memory_space_id, subject_id, soul_id).expect("scope key");
        let event = super::super::MemoryStoreEvent::new(
            format!("{operation_id}-event"),
            super::super::MemoryStoreEventKind::MemoryControl,
            scope.clone(),
            now,
        )
        .with_payload("operation", "soul_evidence")
        .with_payload("result", "explicit_unseeded_created");
        let batch = StoreMutationBatch {
            transaction_id: operation.transaction_id().to_string(),
            operation: "soul.evidence".to_string(),
            scope,
            mutations: vec![
                StoreMutation::PutJson {
                    namespace: SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.to_string(),
                    key: scope_key.clone(),
                    value: serde_json::to_value(&head).expect("head json"),
                    event_kind: super::super::MemoryStoreEventKind::MemoryControl,
                    plane: SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.to_string(),
                    record_key: scope_key.clone(),
                },
                StoreMutation::PutJson {
                    namespace: SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                    key: scope_key.clone(),
                    value: serde_json::to_value(&manifest).expect("manifest json"),
                    event_kind: super::super::MemoryStoreEventKind::MemoryControl,
                    plane: SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                    record_key: scope_key.clone(),
                },
                StoreMutation::AppendEvent {
                    event: Box::new(event),
                },
            ],
        };
        let preconditions = vec![
            StoreJsonPrecondition::Absent {
                namespace: SUBJECT_SOUL_LIFECYCLE_HEAD_NAMESPACE.to_string(),
                key: scope_key.clone(),
            },
            StoreJsonPrecondition::Absent {
                namespace: SUBJECT_SOUL_SCOPE_MANIFEST_NAMESPACE.to_string(),
                key: scope_key,
            },
        ];
        SubjectSoulStoreMutationPlan::new(
            &"a".repeat(64),
            &"a".repeat(64),
            SubjectSoulExpectedStateV1::PristineAbsent {
                closure_certificate_digest: platform
                    .subject_soul_open_closure_certificate()
                    .digest()
                    .to_string(),
            },
            SubjectSoulStorePostImage {
                head,
                manifest,
                current_material: None,
                current_core: None,
                current_core_document: None,
                current_ledger: None,
                current_ledger_document: None,
            },
            batch,
            preconditions,
            operation,
        )
        .expect("typed Subject Soul plan")
    }

    fn unseeded_plan(platform: &StorePlatform, operation_id: &str) -> SubjectSoulStoreMutationPlan {
        unseeded_plan_for_scope(
            platform,
            operation_id,
            "space-spv1",
            "agent-spv1",
            "soul-spv1",
            current_unix_secs(),
            super::super::StoreEventScope::new("agent", "owner", "local", "chat")
                .with_memory_space("space-spv1")
                .with_subject("agent-spv1"),
        )
    }

    fn commit_and_read_unseeded(platform: &StorePlatform) {
        let authority = platform.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).expect("budget lease");
        let outcome = lease
            .execute(&authority, || {
                platform
                    .commit_subject_soul_mutation_with_runtime_budget(
                        unseeded_plan(platform, "spv1-unseeded-create"),
                        lease.report(),
                    )
                    .map_err(SubjectSoulStoreFailure::into_store_error)
            })
            .expect("commit explicit unseeded");
        assert!(matches!(
            outcome,
            SubjectSoulStoreMutationOutcome::Committed { .. }
        ));
        let read = platform
            .read_verified_subject_soul(
                "space-spv1",
                "soul-spv1",
                &SubjectSoulReadRequestV1 {
                    target_subject_id: "agent-spv1".to_string(),
                    selector: SubjectSoulReadSelectorV1::Current,
                    view: SubjectSoulReadViewV1::OperatorSafe,
                },
                lease.report(),
            )
            .expect("verified persisted unseeded read");
        assert!(matches!(
            read.outcome,
            SubjectSoulReadOutcomeV1::Verified { .. }
        ));
        assert_eq!(read.head.as_ref().map(|head| head.generation), Some(1));
        assert!(read.selected_material.is_none());

        let replay_lease =
            crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).expect("replay budget lease");
        let replay = replay_lease
            .execute(&authority, || {
                platform
                    .commit_subject_soul_mutation_with_runtime_budget(
                        unseeded_plan(platform, "spv1-unseeded-create"),
                        replay_lease.report(),
                    )
                    .map_err(SubjectSoulStoreFailure::into_store_error)
            })
            .expect("replay explicit unseeded");
        let SubjectSoulStoreMutationOutcome::Replayed { result, .. } = replay else {
            panic!("same operation must replay")
        };
        assert_eq!(result.generation, 1);
        assert_eq!(result.state_after, SubjectSoulLifecycleStateV1::Unseeded);
        assert!(result.replayed);
    }

    #[test]
    fn pristine_read_is_typed_implicit_unseeded_without_fake_roots() {
        let platform = StorePlatform::open(
            super::super::StoreBackendConfig::in_memory(native_profile())
                .expect("in-memory config"),
        )
        .expect("open store");
        let authority = platform.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).expect("budget lease");
        let read = platform
            .read_verified_subject_soul(
                "space-pristine",
                "soul-pristine",
                &SubjectSoulReadRequestV1 {
                    target_subject_id: "agent-pristine".to_string(),
                    selector: SubjectSoulReadSelectorV1::Current,
                    view: SubjectSoulReadViewV1::OperatorSafe,
                },
                lease.report(),
            )
            .expect("implicit unseeded read");
        assert!(matches!(
            read.outcome,
            SubjectSoulReadOutcomeV1::ImplicitUnseeded { generation: 1, .. }
        ));
        assert!(read.head.is_none());
        assert!(read.manifest.is_none());
    }

    #[test]
    fn relationship_source_known_key_read_is_bounded_receipted_and_fail_closed() {
        let (source, manifest) = relationship_source_fixture();
        let manifest_key = super::super::schema::relationship_source_scope_key(
            &source.memory_space_id,
            &source.relationship_id,
        )
        .expect("relationship manifest key");
        let source_key = relationship_source_revision_key(
            &source.memory_space_id,
            &source.relationship_id,
            source.revision,
        )
        .expect("relationship source key");

        let mut absent = KnownKeySession::new(BTreeMap::new());
        assert!(read_optional_current_relationship_source_in_session(
            &mut absent,
            &source.memory_space_id,
            &source.relationship_id,
        )
        .expect("an absent manifest is a legal absent relationship root")
        .is_none());
        let absent_receipt = absent.receipt().expect("absent receipt");
        assert_eq!(absent_receipt.entry_count, 1);
        assert_eq!(absent_receipt.json_doc_count, 0);

        let mut valid_documents = BTreeMap::from([
            (
                (
                    RELATIONSHIP_SOURCE_SCOPE_MANIFEST_NAMESPACE.to_string(),
                    manifest_key.clone(),
                ),
                serde_json::to_value(&manifest).expect("manifest JSON"),
            ),
            (
                (
                    RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                    source_key.clone(),
                ),
                serde_json::to_value(&source).expect("source JSON"),
            ),
        ]);
        let mut valid = KnownKeySession::new(valid_documents.clone());
        let (read_source, read_manifest) = read_optional_current_relationship_source_in_session(
            &mut valid,
            &source.memory_space_id,
            &source.relationship_id,
        )
        .expect("valid exact relationship root")
        .expect("relationship root must be present");
        assert_eq!(read_source, source);
        assert_eq!(read_manifest, manifest);
        let valid_receipt = valid.receipt().expect("valid receipt");
        assert_eq!(valid_receipt.entry_count, 2);
        assert_eq!(valid_receipt.json_doc_count, 2);

        valid_documents.remove(&(
            RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
            source_key.clone(),
        ));
        let mut partial = KnownKeySession::new(valid_documents.clone());
        assert!(read_optional_current_relationship_source_in_session(
            &mut partial,
            &source.memory_space_id,
            &source.relationship_id,
        )
        .is_err());
        assert_eq!(
            partial.receipt().expect("partial receipt").entry_count,
            2,
            "the attempted missing current source must remain in the exact read transcript"
        );

        let corruptions = [
            ("owner", {
                let mut value = source.clone();
                value.memory_space_id = "space:other".to_string();
                value.refresh_digest().expect("owner-corrupt digest");
                value
            }),
            ("revision", {
                let mut value = source.clone();
                value.revision = 2;
                value.supersedes_revision = Some(1);
                value.refresh_digest().expect("revision-corrupt digest");
                value
            }),
            ("digest", {
                let mut value = source.clone();
                value.content_digest = "f".repeat(64);
                value
            }),
        ];
        for (label, corrupted_source) in corruptions {
            let mut documents = valid_documents.clone();
            documents.insert(
                (
                    RELATIONSHIP_SOURCE_CONSTITUTION_NAMESPACE.to_string(),
                    source_key.clone(),
                ),
                serde_json::to_value(corrupted_source).expect("corrupted source JSON"),
            );
            let mut session = KnownKeySession::new(documents);
            assert!(
                read_optional_current_relationship_source_in_session(
                    &mut session,
                    &source.memory_space_id,
                    &source.relationship_id,
                )
                .is_err(),
                "{label} corruption must fail closed"
            );
            assert_eq!(
                session.receipt().expect("corrupt receipt").entry_count,
                2,
                "{label} corruption must be decided from the same bounded read session"
            );
        }
    }

    #[test]
    fn canonical_envelope_cannot_bypass_typed_lifecycle_authority() {
        let platform = StorePlatform::open(
            super::super::StoreBackendConfig::in_memory(native_profile())
                .expect("in-memory config"),
        )
        .expect("open store");
        let key = "canonical-envelope-without-lifecycle";
        let envelope = SubjectSoulOwnedDocumentV1::new(
            &SubjectSoulOwnerV1 {
                memory_space_id: "space-envelope".to_string(),
                subject_id: "agent-envelope".to_string(),
                soul_id: "soul-envelope".to_string(),
            },
            1,
            None,
            &SubjectSoulManifestAddressV1 {
                namespace: "self_model".to_string(),
                physical_key: key.to_string(),
            },
            &serde_json::json!({"observations": []}),
        )
        .expect("canonical owner envelope");
        let error = platform
            .commit_governed_memory_transaction_with_preconditions(
                StoreMutationBatch {
                    transaction_id: "forged-canonical-envelope".to_string(),
                    operation: "forged.soul".to_string(),
                    scope: super::super::StoreEventScope::system("forged.soul"),
                    mutations: vec![StoreMutation::PutJson {
                        namespace: "self_model".to_string(),
                        key: key.to_string(),
                        value: serde_json::to_value(envelope).expect("envelope json"),
                        event_kind: super::super::MemoryStoreEventKind::MemoryControl,
                        plane: "self_model".to_string(),
                        record_key: key.to_string(),
                    }],
                },
                &[StoreJsonPrecondition::Absent {
                    namespace: "self_model".to_string(),
                    key: key.to_string(),
                }],
            )
            .expect_err("primitive canonical envelope must fail closed");
        assert_eq!(
            error.stage(),
            "memory_write_transaction_subject_soul_authority_missing"
        );
    }

    #[test]
    fn generic_operation_cannot_reuse_a_valid_soul_batch_without_typed_authority() {
        let platform = StorePlatform::open(
            super::super::StoreBackendConfig::in_memory(native_profile())
                .expect("in-memory config"),
        )
        .expect("open store");
        let typed = unseeded_plan(&platform, "spv1-sealed-operation-authority");
        let unauthorized_operation = StoreMutationOperationPlan::new(
            typed.operation.identity().clone(),
            typed.operation.intent_digest().to_string(),
            MemoryMutationEffect::Changed,
            2,
            "agent-spv1",
            current_unix_secs(),
        )
        .expect("generic MOR operation");
        assert_eq!(
            unauthorized_operation.transaction_id(),
            typed.batch.transaction_id
        );
        let before = platform
            .export_store_snapshot()
            .expect("snapshot before unauthorized operation");
        let authority = platform.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).expect("budget lease");
        let error = lease
            .execute(&authority, || {
                platform.commit_memory_mutation_operation_with_runtime_budget(
                    typed.batch,
                    &typed.preconditions,
                    unauthorized_operation,
                    lease.report(),
                )
            })
            .expect_err("generic MOR operation must not inherit typed Soul authority");
        assert_eq!(
            error.stage(),
            "memory_write_transaction_subject_soul_authority_missing"
        );
        assert_eq!(
            platform
                .export_store_snapshot()
                .expect("snapshot after unauthorized operation"),
            before,
            "rejected generic MOR operation must not create Soul roots, receipt, audit, or events"
        );
    }

    #[test]
    fn in_memory_typed_unseeded_commit_and_replay_are_exact() {
        let platform = StorePlatform::open(
            super::super::StoreBackendConfig::in_memory(native_profile())
                .expect("in-memory config"),
        )
        .expect("open store");
        commit_and_read_unseeded(&platform);
    }

    #[test]
    fn file_reopen_preserves_typed_unseeded_closure_and_durable_result() {
        let root = std::env::temp_dir().join(format!(
            "bm-spv1-file-reopen-{}-{}",
            std::process::id(),
            current_unix_secs()
        ));
        let config = super::super::StoreBackendConfig::file(&root, persistent_profile())
            .expect("file config");
        {
            let platform = StorePlatform::open(config.clone()).expect("open file store");
            commit_and_read_unseeded(&platform);
        }
        let reopened = StorePlatform::open(config).expect("reopen file store");
        let authority = reopened.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).expect("budget lease");
        let read = reopened
            .read_verified_subject_soul(
                "space-spv1",
                "soul-spv1",
                &SubjectSoulReadRequestV1 {
                    target_subject_id: "agent-spv1".to_string(),
                    selector: SubjectSoulReadSelectorV1::Current,
                    view: SubjectSoulReadViewV1::OperatorSafe,
                },
                lease.report(),
            )
            .expect("reopen verified read");
        assert!(matches!(
            read.outcome,
            SubjectSoulReadOutcomeV1::Verified { .. }
        ));
        drop(reopened);
        std::fs::remove_dir_all(root).expect("remove file test store");
    }

    #[cfg(feature = "sqlite-store")]
    #[test]
    fn sqlite_reopen_preserves_typed_unseeded_closure_and_durable_result() {
        let path = std::env::temp_dir().join(format!(
            "bm-spv1-sqlite-reopen-{}-{}.sqlite3",
            std::process::id(),
            current_unix_secs()
        ));
        let config = super::super::StoreBackendConfig::sqlite(&path, persistent_profile())
            .expect("sqlite config");
        {
            let platform = StorePlatform::open(config.clone()).expect("open sqlite store");
            commit_and_read_unseeded(&platform);
        }
        let reopened = StorePlatform::open(config).expect("reopen sqlite store");
        let authority = reopened.runtime_budget_authority();
        let lease = crate::RuntimeBudgetLease::issue(Arc::clone(&authority)).expect("budget lease");
        let read = reopened
            .read_verified_subject_soul(
                "space-spv1",
                "soul-spv1",
                &SubjectSoulReadRequestV1 {
                    target_subject_id: "agent-spv1".to_string(),
                    selector: SubjectSoulReadSelectorV1::Current,
                    view: SubjectSoulReadViewV1::OperatorSafe,
                },
                lease.report(),
            )
            .expect("reopen verified read");
        assert!(matches!(
            read.outcome,
            SubjectSoulReadOutcomeV1::Verified { .. }
        ));
        drop(reopened);
        std::fs::remove_file(path).expect("remove sqlite test store");
    }
}
