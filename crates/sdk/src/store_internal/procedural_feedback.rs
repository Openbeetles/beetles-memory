use std::collections::{BTreeMap, BTreeSet};

pub(crate) mod source_dependents;

pub(crate) const EVENT_PLANE: &str = "procedural_feedback";

use bm_core::memory::{
    GovernedMemoryOwnerPlane, MemoryMutationAuditRecord, MemoryMutationEffect,
    MemoryMutationOperationIdentity, MemoryMutationOperationKind, MemoryMutationReceipt,
    PostTurnGovernanceJobRefV2, PostTurnGovernanceJobV3, PostTurnGovernanceScopeIndexV3,
    ProceduralAppliedOwnerBindingV1, ProceduralFeedbackApplicationLedgerV2,
    ProceduralFeedbackErrorClassV1, ProceduralFeedbackJobRefV2, ProceduralFeedbackJobStatusV1,
    ProceduralFeedbackJobV2, ProceduralFeedbackReceiptV2, ProceduralFeedbackReconciliationCursorV1,
    ProceduralFeedbackScopeIndexV2, ProceduralSubjectInitializationV1, ProceduralSubjectScopeV1,
    ProceduralSubjectValidityRootV1, MAX_PROCEDURAL_FEEDBACK_ACTIVE_JOBS,
    MAX_PROCEDURAL_FEEDBACK_RECENT_TERMINAL_JOBS, MAX_PROCEDURAL_SUBJECT_SCOPES,
    PROCEDURAL_FEEDBACK_RECEIPT_SCHEMA_VERSION,
};
use bm_core::skills::{
    AgentToolExperienceOwnerHeadV3, AgentToolExperienceRetainedRevisionDigestV3,
    AgentToolExperienceRevisionMaterialV3,
};
use bm_core::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    MemoryStoreEventKind, RuntimeBudgetReport, StoreEventScope, StoreJsonPrecondition,
    StoreMutation, StoreMutationBatch,
};

use super::post_turn_governance::{
    append_binding_reference_mutation, GovernanceIntentEnsureOutcome,
};
use super::schema::{
    AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE, AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
    PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE, PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
    PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE, PROCEDURAL_SUBJECT_INITIALIZATION_NAMESPACE,
    PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
};
use super::transaction::BackendTransactionState;
use super::{StoreMutationOperationOutcome, StoreMutationOperationPlan, StorePlatform};
use bm_core::memory::{
    MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS, MAX_POST_TURN_GOVERNANCE_RECENT_TERMINAL_JOBS,
    POST_TURN_GOVERNANCE_JOB_NAMESPACE, POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
};

const PROCEDURAL_RETRY_BASE_SECS: u64 = 5;
const PROCEDURAL_RETRY_MAX_SECS: u64 = 300;

mod reconciliation;
pub(crate) mod runtime_skill_control;
pub(crate) use reconciliation::{
    commit_reconciliation_page, replay_reconciliation_page, resume_reconciliation_capacity,
    ProceduralReconciliationAuthorization, ProceduralReconciliationCommit,
    ProceduralReconciliationPageAction, ProceduralReconciliationPageInput,
};

/// Ephemeral proof carried only by the existing governed transaction. Neither
/// variant is deserializable or a second durable permission registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProceduralMutationAuthorization {
    ProducerControl(Box<ProceduralProducerControlAuthorization>),
    Reconciliation(Box<reconciliation::ProceduralReconciliationAuthorization>),
}

impl ProceduralMutationAuthorization {
    pub(super) fn runtime_preimages(&self) -> Result<Vec<((String, String), serde_json::Value)>> {
        match self {
            Self::Reconciliation(authority) => authority.runtime_preimages(),
            Self::ProducerControl(_) => Ok(Vec::new()),
        }
    }

    pub(super) fn owns_runtime_change(
        &self,
        change: &bm_core::memory::ProceduralRuntimeSkillTransitionV1,
        current: &BTreeMap<(String, String), serde_json::Value>,
    ) -> bool {
        matches!(self, Self::Reconciliation(authority) if authority.owns_runtime_change(change, current))
    }

    pub(crate) fn permits_mutation(
        &self,
        mutation: &super::StoreEngineMutation,
        current: &BTreeMap<(String, String), serde_json::Value>,
    ) -> bool {
        match self {
            Self::ProducerControl(authority) => authority.permits_mutation(mutation, current),
            Self::Reconciliation(authority) => authority.permits_mutation(mutation, current),
        }
    }
}

pub(crate) struct VerifiedProceduralProducer {
    pub(crate) head: bm_core::memory::ProceduralProducerHeadV1,
    pub(crate) historical: bm_core::memory::ProceduralProducerBindingV1,
    pub(crate) current: bm_core::memory::ProceduralProducerBindingV1,
}

/// Bounded, known-key authority read. The resulting head must still be fenced
/// at transaction admission; a successful read is not a grant to write later.
pub(crate) fn read_verified_procedural_producer(
    platform: &StorePlatform,
    reference: &bm_core::memory::ProceduralProducerRevisionRefV1,
) -> Result<VerifiedProceduralProducer> {
    use super::schema::{
        PROCEDURAL_PRODUCER_BINDING_NAMESPACE as MATERIAL,
        PROCEDURAL_PRODUCER_HEAD_NAMESPACE as HEAD,
    };
    let stage = "procedural_producer_read";
    if !reference.validate_contract() {
        return Err(Error::invalid_input(stage, "invalid producer reference"));
    }
    let read = |namespace: &str, key: &str| -> Result<serde_json::Value> {
        platform
            .read_json_docs_by_keys(namespace, &[key.to_owned()])?
            .pop()
            .map(|doc| doc.value)
            .ok_or_else(|| Error::config(stage, "required producer authority root is missing"))
    };
    let head: bm_core::memory::ProceduralProducerHeadV1 =
        decode(read(HEAD, &reference.binding_key)?, stage)?;
    if !head.validate_contract() || !head.retained_revisions.contains(reference) {
        return Err(Error::config(
            stage,
            "producer reference is not retained by its current head",
        ));
    }
    let materials = head
        .retained_revisions
        .iter()
        .map(|revision| {
            decode::<bm_core::memory::ProceduralProducerBindingV1>(
                read(MATERIAL, &revision.material_key())?,
                stage,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    if !head.validates_materials(&materials) {
        return Err(Error::config(stage, "producer history does not close"));
    }
    let root: ProceduralFeedbackScopeIndexV2 = decode(
        read(
            PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
            &head.scope.scope_index_key()?,
        )?,
        stage,
    )?;
    if root.validate().is_err() || !root.producer_heads.contains(&head.current) {
        return Err(Error::config(
            stage,
            "producer scope root does not bind its current head",
        ));
    }
    let subject_key = ProceduralSubjectScopeV1 {
        memory_space_id: head.scope.memory_space_id.clone(),
        mounted_subject_id: head.scope.mounted_subject_id.clone(),
    }
    .root_key()?;
    let subject_root: ProceduralSubjectValidityRootV1 = decode(
        read(PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE, &subject_key)?,
        stage,
    )?;
    subject_root.validate(MAX_PROCEDURAL_SUBJECT_SCOPES)?;
    if subject_root.physical_key != subject_key
        || !subject_root.producer_scopes.contains(&head.scope)
    {
        return Err(Error::config(
            stage,
            "producer subject root does not bind its exact scope",
        ));
    }
    let initialization: ProceduralSubjectInitializationV1 = decode(
        read(
            PROCEDURAL_SUBJECT_INITIALIZATION_NAMESPACE,
            &ProceduralSubjectInitializationV1::key(&subject_root.scope)?,
        )?,
        stage,
    )?;
    if initialization.scope != subject_root.scope {
        return Err(Error::config(stage, "subject initialization scope differs"));
    }
    let first: bm_core::memory::ProceduralProducerBindingV1 = decode(
        read(MATERIAL, &initialization.first_producer.material_key())?,
        stage,
    )?;
    initialization.validate_producer(&first)?;
    if !subject_root.producer_scopes.contains(&first.spec.scope) {
        return Err(Error::config(
            stage,
            "subject root lost its initialization scope",
        ));
    }
    let first_receipt_key = first.operation_identity.storage_key();
    validate_producer_operation_proof(
        &first,
        &decode(
            read(
                bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE,
                &first_receipt_key,
            )?,
            stage,
        )?,
        &decode(
            read(
                bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE,
                &first_receipt_key,
            )?,
            stage,
        )?,
    )?;
    for binding in &materials {
        let key = binding.operation_identity.storage_key();
        let receipt: MemoryMutationReceipt = decode(
            read(bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE, &key)?,
            stage,
        )?;
        let audit: MemoryMutationAuditRecord = decode(
            read(bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE, &key)?,
            stage,
        )?;
        validate_producer_operation_proof(binding, &receipt, &audit)?;
    }
    let historical = materials
        .iter()
        .find(|binding| binding.revision == reference.revision)
        .cloned()
        .ok_or_else(|| Error::config(stage, "producer historical material is missing"))?;
    let current = materials
        .last()
        .cloned()
        .ok_or_else(|| Error::config(stage, "producer current material is missing"))?;
    Ok(VerifiedProceduralProducer {
        head,
        historical,
        current,
    })
}

fn validate_producer_operation_proof(
    binding: &bm_core::memory::ProceduralProducerBindingV1,
    receipt: &MemoryMutationReceipt,
    audit: &MemoryMutationAuditRecord,
) -> Result<()> {
    validate_learning_mutation_proof(receipt, audit, "procedural_producer_operation")?;
    if receipt.identity != binding.operation_identity
        || receipt.intent_digest != binding.control_intent_digest()?
        || receipt.effect != MemoryMutationEffect::Changed
        || receipt.committed_at_unix_secs != binding.updated_at
    {
        return Err(Error::config(
            "procedural_producer_operation",
            "producer revision and authoritative operation proof diverged",
        ));
    }
    Ok(())
}

pub(crate) fn validate_learning_mutation_proof(
    receipt: &MemoryMutationReceipt,
    audit: &MemoryMutationAuditRecord,
    stage: &'static str,
) -> Result<()> {
    receipt.validate_contract()?;
    audit.validate_contract()?;
    if audit.identity != receipt.identity
        || audit.intent_digest != receipt.intent_digest
        || audit.effect_plan_digest != receipt.effect_plan_digest
        || audit.effect != receipt.effect
        || audit.transaction_id != receipt.transaction_id
        || audit.changed_count != receipt.changed_count
        || audit.audit_record_id != receipt.audit_record_id
        || audit.actor_subject_id != receipt.identity.actor_subject_id()
        || audit.committed_at_unix_secs != receipt.committed_at_unix_secs
    {
        return Err(Error::config(
            stage,
            "learning mutation receipt and authoritative audit diverged",
        ));
    }
    Ok(())
}

fn reconciliation_block_reason(
    job: &ProceduralFeedbackJobV2,
) -> bm_core::memory::ProceduralReconciliationBlockV1 {
    use bm_core::memory::ProceduralReconciliationBlockV1 as Block;
    match job.last_error_class {
        Some(ProceduralFeedbackErrorClassV1::BudgetExceeded) => Block::Capacity,
        Some(
            ProceduralFeedbackErrorClassV1::EvidenceUnavailable
            | ProceduralFeedbackErrorClassV1::RegistryUnavailable,
        ) => Block::SourceUnavailable,
        _ => Block::RepairRequired,
    }
}

/// One exact current root/job closure for Store admission and immutable status
/// reads. Discovering a job never grants permission to resume it.
pub(crate) fn validate_subject_reconciliation_job(
    root: &bm_core::memory::ProceduralSubjectValidityRootV1,
    job: &ProceduralFeedbackJobV2,
) -> Result<()> {
    use bm_core::memory::{ProceduralLearningWorkV1, ProceduralSubjectValidityStateV1 as State};
    let stage = "procedural_reconciliation_status";
    let invalid = || Error::config(stage, "subject validity and exact durable job differ");
    job.validate()?;
    let ProceduralLearningWorkV1::Reconcile { source } = &job.work else {
        return Err(invalid());
    };
    let reference = source.reference()?;
    if source.scope != root.scope
        || job.discovery_root_key != root.physical_key
        || reference.target_epoch != root.validity_epoch
        || !root.retained_work.contains(&reference)
    {
        return Err(invalid());
    }
    let closes = match (&root.state, &job.receipt) {
        (
            State::Ready {
                completion: Some(completion),
            },
            Some(bm_core::memory::ProceduralLearningReceiptV1::Reconcile { receipt }),
        ) => {
            job.status == ProceduralFeedbackJobStatusV1::Succeeded
                && completion.work == reference
                && completion.receipt_digest == receipt.canonical_digest()?
        }
        (State::Reconciling { work }, None) => work == &reference && job.status.is_active(),
        (State::Blocked { work, reason }, None) => {
            work == &reference
                && matches!(
                    job.status,
                    ProceduralFeedbackJobStatusV1::RepairRequired
                        | ProceduralFeedbackJobStatusV1::DeadLetter
                        | ProceduralFeedbackJobStatusV1::BlockedCapacity
                )
                && *reason == reconciliation_block_reason(job)
        }
        _ => false,
    };
    if !closes {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn validate_reconciliation_checkpoint_commitment(
    job: &ProceduralFeedbackJobV2,
    receipt: &MemoryMutationReceipt,
) -> Result<()> {
    let stage = "procedural_reconciliation_checkpoint_proof";
    job.validate()?;
    let (Some(checkpoint), Some(authority)) = (&job.checkpoint, &job.checkpoint_authority) else {
        return Err(Error::config(
            stage,
            "persisted page has no exact authority",
        ));
    };
    receipt.validate_contract()?;
    if receipt.identity != authority.operation
        || receipt.intent_digest != authority.intent_digest(checkpoint)?
        || receipt.effect != MemoryMutationEffect::Changed
        || receipt.committed_at_unix_secs != checkpoint.updated_at
    {
        return Err(Error::config(
            stage,
            "checkpoint differs from its immutable page commitment",
        ));
    }
    Ok(())
}

pub(crate) fn validate_reconciliation_checkpoint_in_state(
    job: &ProceduralFeedbackJobV2,
    state: &BackendTransactionState,
) -> Result<()> {
    let stage = "procedural_reconciliation_checkpoint_proof";
    job.validate()?;
    let Some(authority) = &job.checkpoint_authority else {
        return Ok(());
    };
    let key = authority.operation.storage_key();
    let receipt: MemoryMutationReceipt = state
        .json
        .get(&(
            bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.into(),
            key.clone(),
        ))
        .ok_or_else(|| Error::config(stage, "checkpoint mutation receipt is missing"))
        .and_then(|value| decode(value.clone(), stage))?;
    let audit: MemoryMutationAuditRecord = state
        .json
        .get(&(bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.into(), key))
        .ok_or_else(|| Error::config(stage, "checkpoint authoritative audit is missing"))
        .and_then(|value| decode(value.clone(), stage))?;
    validate_learning_mutation_proof(&receipt, &audit, stage)?;
    validate_reconciliation_checkpoint_commitment(job, &receipt)
}

/// Validate retained completion, not only the root's most recent epoch. The
/// immutable checkpoint describes its own commit; later live manifests may grow.
pub(crate) fn validate_reconciliation_completion(
    job: &ProceduralFeedbackJobV2,
    mutation: &MemoryMutationReceipt,
    audit: &MemoryMutationAuditRecord,
) -> Result<()> {
    use bm_core::memory::{ProceduralLearningReceiptV1, ProceduralLearningWorkV1};
    let stage = "procedural_reconciliation_completion";
    job.validate()?;
    validate_learning_mutation_proof(mutation, audit, stage)?;
    let (
        ProceduralLearningWorkV1::Reconcile { source },
        Some(ProceduralLearningReceiptV1::Reconcile { receipt }),
        Some(checkpoint),
    ) = (&job.work, &job.receipt, &job.checkpoint)
    else {
        return Err(Error::config(
            stage,
            "reconciliation completion lacks exact durable proof",
        ));
    };
    let identity = MemoryMutationOperationIdentity::new(
        &receipt.operation_id,
        &source.scope.memory_space_id,
        &source.scope.mounted_subject_id,
        mutation.identity.actor_subject_id(),
        MemoryMutationOperationKind::ProceduralLearning,
    )?;
    if job.status != ProceduralFeedbackJobStatusV1::Succeeded
        || mutation.identity != identity
        || mutation.identity.storage_key() != receipt.mutation_receipt_key
        || mutation.intent_digest != receipt.plan_digest
        || mutation.transaction_id != receipt.transaction_id
        || mutation.effect != MemoryMutationEffect::Changed
        || mutation.committed_at_unix_secs != receipt.completed_at
        || job.terminal_at != Some(receipt.completed_at)
        || checkpoint.content_digest != receipt.checkpoint_digest
        || checkpoint.canonical_digest()? != receipt.checkpoint_digest
        || receipt.work != source.reference()?
        || receipt.post_image_digest
            != digest_serialized(
                "procedural_reconciliation_post_image_v1",
                &(&checkpoint.content_digest, &checkpoint.current_manifests),
                stage,
            )?
    {
        return Err(Error::config(
            stage,
            "reconciliation checkpoint differs from its authoritative mutation",
        ));
    }
    Ok(())
}

pub(crate) fn validate_feedback_application_completion(
    job: &ProceduralFeedbackJobV2,
    ledger: &ProceduralFeedbackApplicationLedgerV2,
    mutation: &MemoryMutationReceipt,
    audit: &MemoryMutationAuditRecord,
) -> Result<()> {
    let stage = "procedural_feedback_application_completion";
    job.validate()?;
    ledger.validate()?;
    validate_learning_mutation_proof(mutation, audit, stage)?;
    let source = job.feedback_source()?;
    let receipt = job
        .receipt
        .as_ref()
        .ok_or_else(|| Error::config(stage, "feedback receipt is missing"))?
        .feedback()?;
    let identity = MemoryMutationOperationIdentity::new(
        &receipt.operation_id,
        &ledger.identity.memory_space_id,
        &ledger.identity.mounted_subject_id,
        mutation.identity.actor_subject_id(),
        MemoryMutationOperationKind::ProceduralLearning,
    )?;
    if job.status != ProceduralFeedbackJobStatusV1::Succeeded
        || ledger.job_id != job.job_id
        || ledger.identity != source.identity
        || ledger.learning_evidence_digest != source.learning_evidence_digest
        || mutation.identity != identity
        || mutation.identity.storage_key() != receipt.mutation_receipt_key
        || mutation.intent_digest != receipt.plan_digest
        || mutation.transaction_id != receipt.transaction_id
        || mutation.committed_at_unix_secs != receipt.completed_at
        || ledger.completed_at != receipt.completed_at
        || usize::try_from(receipt.changed_count).ok() != Some(ledger.applied_owner_bindings.len())
        || receipt.post_image_digest
            != digest_serialized(
                "procedural_feedback_post_image_digest_v1",
                &(
                    &ledger.application_digest,
                    &receipt.plan_digest,
                    ProceduralFeedbackJobStatusV1::Succeeded,
                ),
                stage,
            )?
    {
        return Err(Error::config(
            stage,
            "feedback application differs from its authoritative mutation",
        ));
    }
    Ok(())
}

pub(crate) fn read_authorized_procedural_evidence(
    platform: &StorePlatform,
    evidence: &bm_core::memory::PostTurnLearningEvidenceV2,
    scope: &bm_core::memory::ProceduralProducerScopeV1,
) -> Result<VerifiedProceduralProducer> {
    let bm_core::memory::ProceduralFeedbackAuthorityV2::Producer {
        producer_revision, ..
    } = &evidence.authority
    else {
        return Err(Error::invalid_input(
            "procedural_producer_authority",
            "procedural application requires producer authority",
        ));
    };
    let verified = read_verified_procedural_producer(platform, producer_revision)?;
    verified
        .historical
        .authorize_evidence_with_current(&verified.current, scope, evidence)
        .map_err(|_| {
            Error::conflict(
                "procedural_producer_authority",
                "current producer authority does not permit this evidence",
            )
        })?;
    Ok(verified)
}

/// Not a wire authority. Only this owner can construct the exact revision proof
/// after preparing an operation-aware producer-control transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProceduralProducerControlAuthorization {
    binding: bm_core::memory::ProceduralProducerBindingV1,
    subject_root: ProceduralSubjectValidityRootV1,
    initialization: Option<ProceduralSubjectInitializationV1>,
}

impl ProceduralProducerControlAuthorization {
    pub(crate) fn permits_mutation(
        &self,
        mutation: &super::StoreEngineMutation,
        current: &BTreeMap<(String, String), serde_json::Value>,
    ) -> bool {
        use super::schema::{
            PROCEDURAL_PRODUCER_BINDING_NAMESPACE as MATERIAL,
            PROCEDURAL_PRODUCER_HEAD_NAMESPACE as HEAD,
        };
        let super::StoreEngineMutation::PutJson {
            namespace,
            key,
            value,
        } = mutation
        else {
            return false;
        };
        let Ok(reference) = self.binding.revision_ref() else {
            return false;
        };
        match namespace.as_str() {
            MATERIAL => {
                key == &reference.material_key()
                    && !current.contains_key(&(namespace.clone(), key.clone()))
                    && serde_json::from_value::<bm_core::memory::ProceduralProducerBindingV1>(
                        value.clone(),
                    )
                    .is_ok_and(|binding| binding == self.binding)
            }
            HEAD => {
                key == &reference.binding_key
                    && serde_json::from_value::<bm_core::memory::ProceduralProducerHeadV1>(
                        value.clone(),
                    )
                    .is_ok_and(|head| {
                        head.validate_contract()
                            && head.current == reference
                            && head.scope == self.binding.spec.scope
                    })
            }
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE => {
                key == &self.subject_root.physical_key
                    && serde_json::from_value::<ProceduralSubjectValidityRootV1>(value.clone())
                        .is_ok_and(|root| root == self.subject_root)
            }
            PROCEDURAL_SUBJECT_INITIALIZATION_NAMESPACE => {
                !current.contains_key(&(namespace.clone(), key.clone()))
                    && self.initialization.as_ref().is_some_and(|material| {
                        key == &material.physical_key
                            && serde_json::from_value::<ProceduralSubjectInitializationV1>(
                                value.clone(),
                            )
                            .is_ok_and(|value| value == *material)
                    })
            }
            _ => false,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn control_procedural_producer(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    operation_identity: MemoryMutationOperationIdentity,
    spec: bm_core::memory::ProceduralProducerSpecV1,
    state: bm_core::memory::ProceduralProducerStateV1,
    expected: Option<bm_core::memory::ProceduralProducerRevisionRefV1>,
    now: u64,
    source_preconditions: &[StoreJsonPrecondition],
) -> Result<(
    bm_core::memory::ProceduralProducerBindingV1,
    MemoryMutationReceipt,
)> {
    use super::schema::{
        PROCEDURAL_PRODUCER_BINDING_NAMESPACE as MATERIAL,
        PROCEDURAL_PRODUCER_HEAD_NAMESPACE as HEAD,
    };
    use bm_core::memory::{
        ProceduralProducerBindingV1 as Binding, ProceduralProducerHeadV1 as Head,
    };
    let stage = "procedural_producer_control";
    if scope.memory_space_id != spec.scope.memory_space_id
        || scope.subject_id != spec.scope.mounted_subject_id
        || scope.channel != spec.scope.channel_id
        || scope.chat_id != spec.scope.chat_id
    {
        return Err(Error::invalid_input(
            stage,
            "producer control scope differs from the mounted scope",
        ));
    }
    let intent = bm_core::memory::procedural_producer_control_intent_digest(
        &spec,
        state,
        expected.as_ref(),
    )?;
    let binding_key = spec.binding_key()?;
    let revision = expected
        .as_ref()
        .map_or(Some(1), |reference| reference.revision.checked_add(1))
        .ok_or_else(|| Error::invalid_input(stage, "producer revision capacity is exhausted"))?;
    let material_key = format!("{binding_key}:{revision}");
    let read_committed =
        |receipt: MemoryMutationReceipt| -> Result<(Binding, MemoryMutationReceipt)> {
            let doc = platform
                .read_json_docs_by_keys(MATERIAL, std::slice::from_ref(&material_key))?
                .pop()
                .ok_or_else(|| {
                    Error::config(stage, "producer receipt is missing its exact material")
                })?;
            let binding: Binding = decode(doc.value, stage)?;
            if binding.operation_identity != operation_identity
                || binding.control_intent_digest()? != intent
                || binding.updated_at != receipt.committed_at_unix_secs
            {
                return Err(Error::config(
                    stage,
                    "producer operation replay differs from its retained material",
                ));
            }
            read_verified_procedural_producer(platform, &binding.revision_ref()?)?;
            Ok((binding, receipt))
        };
    if let Some(receipt) = platform.preflight_memory_mutation_operation(
        &super::StoreMutationOperationPreflight::new(operation_identity.clone(), intent.clone())?,
    )? {
        return read_committed(receipt);
    }
    let previous_head: Option<Head> = platform
        .read_json_docs_by_keys(HEAD, std::slice::from_ref(&binding_key))?
        .pop()
        .map(|doc| decode(doc.value, stage))
        .transpose()?;
    if previous_head.as_ref().map(|head| &head.current) != expected.as_ref() {
        return Err(Error::conflict(
            stage,
            "producer expected revision is not current",
        ));
    }
    let previous: Option<Binding> = expected
        .as_ref()
        .map(|reference| {
            platform
                .read_json_docs_by_keys(MATERIAL, &[reference.material_key()])?
                .pop()
                .ok_or_else(|| {
                    Error::config(stage, "producer head is missing its current material")
                })
                .and_then(|doc| decode(doc.value, stage))
        })
        .transpose()?;
    let binding = Binding::build(
        spec,
        state,
        operation_identity.clone(),
        now,
        previous.as_ref(),
    )
    .map_err(|reason| {
        Error::invalid_input(stage, format!("producer transition rejected: {reason:?}"))
    })?;
    let head = Head::advance(&binding, previous_head.as_ref()).map_err(|reason| {
        Error::invalid_input(stage, format!("producer head rejected: {reason:?}"))
    })?;
    let scope_key = binding.spec.scope.scope_index_key()?;
    let previous_index = read_scope_index(platform, &scope_key)?;
    if previous_head.is_some() && previous_index.is_none() {
        return Err(Error::config(stage, "producer scope root is missing"));
    }
    let mut index = match previous_index.clone() {
        Some(index) => index,
        None => ProceduralFeedbackScopeIndexV2::empty(&binding.spec.scope, now)?,
    };
    index.bind_producer_head(&head, now)?;
    let subject_scope = ProceduralSubjectScopeV1 {
        memory_space_id: binding.spec.scope.memory_space_id.clone(),
        mounted_subject_id: binding.spec.scope.mounted_subject_id.clone(),
    };
    let subject_key = subject_scope.root_key()?;
    let previous_root: Option<ProceduralSubjectValidityRootV1> = platform
        .read_json_docs_by_keys(
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
            std::slice::from_ref(&subject_key),
        )?
        .pop()
        .map(|doc| decode(doc.value, stage))
        .transpose()?;
    let initialization_key = ProceduralSubjectInitializationV1::key(&subject_scope)?;
    let previous_initialization: Option<ProceduralSubjectInitializationV1> = platform
        .read_json_docs_by_keys(
            PROCEDURAL_SUBJECT_INITIALIZATION_NAMESPACE,
            std::slice::from_ref(&initialization_key),
        )?
        .pop()
        .map(|doc| decode(doc.value, stage))
        .transpose()?;
    if previous_root.is_some() != previous_initialization.is_some() {
        return Err(Error::config(
            stage,
            "subject root and initialization proof must both exist or both be absent",
        ));
    }
    if previous_index
        .as_ref()
        .is_some_and(|index| !index.producer_heads.is_empty())
        && previous_root.is_none()
    {
        return Err(Error::config(
            stage,
            "producer subject validity root is missing",
        ));
    }
    let mut subject_root = match &previous_root {
        Some(root) => root.bind_scope(binding.spec.scope.clone(), MAX_PROCEDURAL_SUBJECT_SCOPES)?,
        None => ProceduralSubjectValidityRootV1::initialize(
            subject_scope,
            binding.spec.scope.clone(),
            MAX_PROCEDURAL_SUBJECT_SCOPES,
        )?,
    };
    let initialization = if previous_initialization.is_none() {
        Some(ProceduralSubjectInitializationV1::new(
            subject_root.scope.clone(),
            &binding,
        )?)
    } else {
        None
    };
    let mut mutations = vec![
        put_json(MATERIAL, &material_key, encode(&binding, stage)?),
        put_json(HEAD, &binding_key, encode(&head, stage)?),
        put_json(
            PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
            &scope_key,
            encode(&index, stage)?,
        ),
    ];
    let mut reconciliation_preconditions = Vec::new();
    if let Some(previous) = &previous {
        let (root, jobs, dependencies) = plan_subject_reconciliation(
            platform,
            &subject_root,
            bm_core::memory::ProceduralReconciliationTriggerV1::ProducerControl {
                operation: operation_identity.clone(),
                before: previous.revision_ref()?,
                after: binding.revision_ref()?,
            },
            now,
        )?;
        subject_root = root;
        mutations.extend(jobs);
        reconciliation_preconditions = dependencies;
    }
    mutations.push(put_json(
        PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
        &subject_key,
        encode(&subject_root, stage)?,
    ));
    if let Some(material) = &initialization {
        mutations.push(put_json(
            PROCEDURAL_SUBJECT_INITIALIZATION_NAMESPACE,
            &initialization_key,
            encode(material, stage)?,
        ));
    }
    let mut preconditions = vec![StoreJsonPrecondition::Absent {
        namespace: MATERIAL.into(),
        key: material_key.clone(),
    }];
    preconditions.extend(reconciliation_preconditions);
    preconditions.extend_from_slice(source_preconditions);
    for (namespace, key, value) in [
        (
            HEAD,
            &binding_key,
            previous_head
                .as_ref()
                .map(|head| encode(head, stage))
                .transpose()?,
        ),
        (
            PROCEDURAL_SUBJECT_INITIALIZATION_NAMESPACE,
            &initialization_key,
            previous_initialization
                .as_ref()
                .map(|material| encode(material, stage))
                .transpose()?,
        ),
        (
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
            &subject_key,
            previous_root
                .as_ref()
                .map(|root| encode(root, stage))
                .transpose()?,
        ),
        (
            PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
            &scope_key,
            previous_index
                .as_ref()
                .map(|index| encode(index, stage))
                .transpose()?,
        ),
    ] {
        preconditions.push(match value {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace: namespace.into(),
                key: key.clone(),
                value,
            },
            None => StoreJsonPrecondition::Absent {
                namespace: namespace.into(),
                key: key.clone(),
            },
        });
    }
    let operation = StoreMutationOperationPlan::new(
        operation_identity.clone(),
        intent.clone(),
        MemoryMutationEffect::Changed,
        mutations.len(),
        operation_identity.actor_subject_id(),
        now,
    )?
    .authorize_procedural_producer(ProceduralProducerControlAuthorization {
        binding: binding.clone(),
        subject_root,
        initialization,
    });
    let batch = StoreMutationBatch {
        transaction_id: operation.transaction_id().to_owned(),
        operation: "post_turn.producer.control".into(),
        scope,
        mutations,
    };
    match platform.commit_memory_mutation_operation_with_runtime_budget(
        batch,
        &preconditions,
        operation,
        runtime_budget,
    )? {
        StoreMutationOperationOutcome::Committed { receipt, .. } => Ok((binding, receipt)),
        StoreMutationOperationOutcome::Replayed { receipt } => read_committed(receipt),
    }
}

/// Plans the single lane's new causal work and supersedes its previous lease.
/// The caller MUST include the returned root with its own source mutation and
/// exact root CAS; this function performs no writes or asynchronous side effect.
pub(super) fn plan_subject_reconciliation(
    platform: &StorePlatform,
    before: &ProceduralSubjectValidityRootV1,
    trigger: bm_core::memory::ProceduralReconciliationTriggerV1,
    now: u64,
) -> Result<(
    ProceduralSubjectValidityRootV1,
    Vec<StoreMutation>,
    Vec<StoreJsonPrecondition>,
)> {
    use bm_core::memory::{ProceduralLearningWorkV1, ProceduralReconciliationWorkV1};
    let stage = "procedural_reconciliation_intent";
    before.validate(MAX_PROCEDURAL_SUBJECT_SCOPES)?;
    if before.retained_work.len() >= MAX_PROCEDURAL_FEEDBACK_RECENT_TERMINAL_JOBS {
        return Err(super::store_budget_error(
            "procedural reconciliation proof retention is exhausted",
        ));
    }
    let work = ProceduralReconciliationWorkV1 {
        scope: before.scope.clone(),
        target_epoch: checked_increment(before.validity_epoch, stage, "validity epoch overflow")?,
        trigger,
    };
    let job = ProceduralFeedbackJobV2::pending_work(
        ProceduralLearningWorkV1::Reconcile {
            source: work.clone(),
        },
        5,
        now,
    )?;
    let after = before.begin(&work, MAX_PROCEDURAL_SUBJECT_SCOPES)?;
    let mut mutations = vec![put_json(
        PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
        &job.job_id,
        encode(&job, stage)?,
    )];
    let mut preconditions = vec![StoreJsonPrecondition::Absent {
        namespace: PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(),
        key: job.job_id.clone(),
    }];
    if let Some(previous) = before.retained_work.last() {
        let previous_job = read_job(platform, &previous.job_id)?
            .ok_or_else(|| Error::config(stage, "previous reconciliation job is missing"))?;
        if previous_job.work.identity()? != (previous.job_id.clone(), before.physical_key.clone()) {
            return Err(Error::config(
                stage,
                "previous reconciliation job differs from its retained identity",
            ));
        }
        preconditions.push(exact(
            PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
            &previous.job_id,
            &previous_job,
            stage,
        )?);
        if !previous_job.status.is_terminal() {
            let mut cancelled = previous_job.clone();
            cancelled.status = ProceduralFeedbackJobStatusV1::Cancelled;
            cancelled.state_revision =
                checked_increment(cancelled.state_revision, stage, "job revision overflow")?;
            cancelled.next_attempt_at = None;
            cancelled.lease_owner = None;
            cancelled.lease_until = None;
            cancelled.last_error_class = None;
            cancelled.updated_at = now;
            cancelled.terminal_at = Some(now);
            cancelled.validate()?;
            mutations.push(put_json(
                PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                &cancelled.job_id,
                encode(&cancelled, stage)?,
            ));
        }
    }
    Ok((after, mutations, preconditions))
}

/// One known-key dependency owner for mutation admission and scoped reads.
pub(crate) struct ProceduralDependencyNode {
    pub(crate) subject_scope: Option<ProceduralSubjectScopeV1>,
    pub(crate) addresses: Vec<(String, String)>,
}

/// Pure typed edges shared by actual-source discovery and transaction planning.
/// This routine performs no reads and cannot treat a proposed image as authority.
pub(crate) fn procedural_dependency_node(
    namespace: &str,
    value: &serde_json::Value,
    expand_source_dependents: bool,
) -> Result<ProceduralDependencyNode> {
    use super::schema::{
        AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE, RUNTIME_SKILL_RECORD_NAMESPACE,
    };
    use bm_core::skills::{
        agent_tool_experience_head_key, agent_tool_experience_scope_manifest_key,
        AgentToolExperienceOwningScopeV1, AgentToolExperienceScopeManifestV1,
    };
    let stage = "procedural_transaction_dependency_read_set";
    let mut addresses = producer_dependency_addresses(namespace, value)?;
    addresses.extend(source_dependents::dependencies(
        namespace,
        value,
        expand_source_dependents,
    )?);
    let mut subject_scope = None;
    match namespace {
        PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE => {
            let root: ProceduralSubjectValidityRootV1 = decode(value.clone(), stage)?;
            subject_scope = Some(root.scope);
        }
        PROCEDURAL_SUBJECT_INITIALIZATION_NAMESPACE => {
            let initialization: ProceduralSubjectInitializationV1 = decode(value.clone(), stage)?;
            subject_scope = Some(initialization.scope);
        }
        super::schema::PROCEDURAL_PRODUCER_HEAD_NAMESPACE => {
            let head: bm_core::memory::ProceduralProducerHeadV1 = decode(value.clone(), stage)?;
            subject_scope = Some(ProceduralSubjectScopeV1 {
                memory_space_id: head.scope.memory_space_id,
                mounted_subject_id: head.scope.mounted_subject_id,
            });
        }
        super::schema::PROCEDURAL_PRODUCER_BINDING_NAMESPACE => {
            let producer: bm_core::memory::ProceduralProducerBindingV1 =
                decode(value.clone(), stage)?;
            subject_scope = Some(ProceduralSubjectScopeV1 {
                memory_space_id: producer.spec.scope.memory_space_id,
                mounted_subject_id: producer.spec.scope.mounted_subject_id,
            });
        }
        RUNTIME_SKILL_RECORD_NAMESPACE => {
            let owner: bm_core::skills::RuntimeSkillOwnerRecord = decode(value.clone(), stage)?;
            if let bm_core::skills::RuntimeSkillOwningScope::Subject { mounted_subject_id } =
                &owner.owning_scope
            {
                subject_scope = Some(ProceduralSubjectScopeV1 {
                    memory_space_id: owner.memory_space_id.clone(),
                    mounted_subject_id: mounted_subject_id.clone(),
                });
            }
            if matches!(
                owner.creation_ref,
                bm_core::skills::RuntimeSkillCreationRef::AgentToolExperiencePromotion { .. }
            ) {
                for source in &owner.intrinsic_contract.evidence_bindings {
                    if source.kind
                        == bm_core::skills::RuntimeSkillEvidenceKind::ProceduralFeedbackSource
                    {
                        addresses.push((
                            PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(),
                            source.safe_ref.clone(),
                        ));
                        addresses.push((
                            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE.into(),
                            source.safe_ref.clone(),
                        ));
                    }
                }
            }
        }
        "conversation_transcript" => {
            let record: bm_core::memory::TranscriptTurnRecord = decode(value.clone(), stage)?;
            subject_scope = Some(ProceduralSubjectScopeV1 {
                memory_space_id: record.key.memory_space_id.clone(),
                mounted_subject_id: record.subject.clone(),
            });
            if record
                .learning_evidence
                .as_ref()
                .and_then(|evidence| evidence.selection_receipt.as_ref())
                .is_some()
            {
                addresses.push((
                    super::procedural_selection::NAMESPACE.into(),
                    super::procedural_selection::authority_key(&record.key.memory_space_id),
                ));
            }
            addresses.extend(super::procedural_selection::intake_dependencies(&record)?);
        }
        PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE => {
            let index: ProceduralFeedbackScopeIndexV2 = decode(value.clone(), stage)?;
            subject_scope = Some(ProceduralSubjectScopeV1 {
                memory_space_id: index.memory_space_id.clone(),
                mounted_subject_id: index.mounted_subject_id.clone(),
            });
            addresses.extend(
                index
                    .active_jobs
                    .iter()
                    .chain(&index.recent_terminal_jobs)
                    .map(|job| (PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(), job.job_id.clone())),
            );
        }
        PROCEDURAL_FEEDBACK_JOB_NAMESPACE => {
            let job: ProceduralFeedbackJobV2 = decode(value.clone(), stage)?;
            if let Some(authority) = &job.checkpoint_authority {
                let key = authority.operation.storage_key();
                addresses.push((
                    bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.into(),
                    key.clone(),
                ));
                addresses.push((bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.into(), key));
            }
            subject_scope = Some(job.work.subject_scope());
            let anchor_namespace = match &job.work {
                bm_core::memory::ProceduralLearningWorkV1::Feedback { source } => {
                    let key = bm_core::memory::ConversationKey::new(
                        &source.identity.memory_space_id,
                        &source.identity.channel_id,
                        &source.identity.conversation_id,
                    )?;
                    addresses.push((
                        "conversation_transcript".into(),
                        super::transcript_turn_storage_key(
                            &key,
                            &source.identity.mounted_subject_id,
                            &source.identity.turn_id,
                        ),
                    ));
                    PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE
                }
                bm_core::memory::ProceduralLearningWorkV1::Reconcile { .. } => {
                    PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE
                }
            };
            addresses.push((anchor_namespace.into(), job.discovery_root_key));
            if let Some(receipt) = job.receipt {
                if matches!(
                    receipt,
                    bm_core::memory::ProceduralLearningReceiptV1::Feedback { .. }
                ) {
                    addresses.push((
                        PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE.into(),
                        job.job_id,
                    ));
                }
                addresses.push((
                    bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.into(),
                    receipt.mutation_receipt_key().into(),
                ));
                addresses.push((
                    bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.into(),
                    receipt.mutation_receipt_key().into(),
                ));
            }
        }
        PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE => {
            let ledger: ProceduralFeedbackApplicationLedgerV2 = decode(value.clone(), stage)?;
            subject_scope = Some(ProceduralSubjectScopeV1 {
                memory_space_id: ledger.identity.memory_space_id.clone(),
                mounted_subject_id: ledger.identity.mounted_subject_id.clone(),
            });
            let scope = AgentToolExperienceOwningScopeV1::Subject {
                mounted_subject_id: ledger.identity.mounted_subject_id.clone(),
            };
            addresses.push((PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(), ledger.job_id));
            for applied in ledger.applied_owner_bindings {
                match applied {
                    ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                        owner_revision, ..
                    } => addresses.push((
                        AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE.into(),
                        agent_tool_experience_head_key(
                            &ledger.identity.memory_space_id,
                            &scope,
                            &owner_revision.owner_ref,
                        )?,
                    )),
                    ProceduralAppliedOwnerBindingV1::RuntimeSkill { binding } => addresses.push((
                        RUNTIME_SKILL_RECORD_NAMESPACE.into(),
                        binding.owner_physical_key,
                    )),
                }
            }
        }
        AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE => {
            let manifest: AgentToolExperienceScopeManifestV1 = decode(value.clone(), stage)?;
            let AgentToolExperienceOwningScopeV1::Subject { mounted_subject_id } =
                &manifest.owning_scope;
            subject_scope = Some(ProceduralSubjectScopeV1 {
                memory_space_id: manifest.memory_space_id.clone(),
                mounted_subject_id: mounted_subject_id.clone(),
            });
            for binding in &manifest.bindings {
                let key = agent_tool_experience_head_key(
                    &manifest.memory_space_id,
                    &manifest.owning_scope,
                    &binding.owner_ref,
                )?;
                if key != binding.head_key {
                    return Err(Error::config(
                        stage,
                        "experience binding redirects canonical head address",
                    ));
                }
                addresses.push((AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE.into(), key));
            }
        }
        AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE => {
            let head: AgentToolExperienceOwnerHeadV3 = decode(value.clone(), stage)?;
            let AgentToolExperienceOwningScopeV1::Subject { mounted_subject_id } =
                &head.owning_scope;
            subject_scope = Some(ProceduralSubjectScopeV1 {
                memory_space_id: head.memory_space_id.clone(),
                mounted_subject_id: mounted_subject_id.clone(),
            });
            addresses.push((
                AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE.into(),
                agent_tool_experience_scope_manifest_key(
                    &head.memory_space_id,
                    &head.owning_scope,
                )?,
            ));
            if head.state == bm_core::skills::AgentToolExperienceHeadStateV3::Active {
                addresses.extend(head.retained_revisions.into_iter().map(|revision| {
                    (
                        AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.into(),
                        revision.material_key,
                    )
                }));
            }
        }
        AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => {
            let material: AgentToolExperienceRevisionMaterialV3 = decode(value.clone(), stage)?;
            let AgentToolExperienceOwningScopeV1::Subject { mounted_subject_id } =
                &material.owning_scope;
            subject_scope = Some(ProceduralSubjectScopeV1 {
                memory_space_id: material.memory_space_id.clone(),
                mounted_subject_id: mounted_subject_id.clone(),
            });
            addresses.push((
                AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE.into(),
                agent_tool_experience_head_key(
                    &material.memory_space_id,
                    &material.owning_scope,
                    &material.owner_ref,
                )?,
            ));
        }
        bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE => {
            let receipt: MemoryMutationReceipt = decode(value.clone(), stage)?;
            subject_scope = Some(ProceduralSubjectScopeV1 {
                memory_space_id: receipt.identity.memory_space_id().into(),
                mounted_subject_id: receipt.identity.mounted_subject_id().into(),
            });
        }
        bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE => {
            let audit: MemoryMutationAuditRecord = decode(value.clone(), stage)?;
            subject_scope = Some(ProceduralSubjectScopeV1 {
                memory_space_id: audit.identity.memory_space_id().into(),
                mounted_subject_id: audit.identity.mounted_subject_id().into(),
            });
        }
        _ => {}
    }
    Ok(ProceduralDependencyNode {
        subject_scope,
        addresses,
    })
}

/// Producer/root edges also serve the immutable, bounded recall owner.
pub(crate) fn producer_dependency_addresses(
    namespace: &str,
    value: &serde_json::Value,
) -> Result<Vec<(String, String)>> {
    use super::schema::{
        PROCEDURAL_PRODUCER_BINDING_NAMESPACE as MATERIAL,
        PROCEDURAL_PRODUCER_HEAD_NAMESPACE as HEAD,
    };
    let stage = "procedural_producer_dependencies";
    let mut addresses = Vec::new();
    match namespace {
        super::schema::RUNTIME_SKILL_RECORD_NAMESPACE => {
            let owner: bm_core::skills::RuntimeSkillOwnerRecord = decode(value.clone(), stage)?;
            if !owner.validate_contract().accepted {
                return Err(Error::config(stage, "invalid runtime usage owner"));
            }
            for reference in &owner.lifecycle.usage_outcome.retained_contributions {
                addresses.push((
                    PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE.to_owned(),
                    reference.source_job_id.clone(),
                ));
                addresses.push((
                    PROCEDURAL_FEEDBACK_JOB_NAMESPACE.to_owned(),
                    reference.source_job_id.clone(),
                ));
            }
        }
        PROCEDURAL_SUBJECT_INITIALIZATION_NAMESPACE => {
            let material: ProceduralSubjectInitializationV1 = decode(value.clone(), stage)?;
            material.validate()?;
            addresses.push((
                PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE.to_owned(),
                material.scope.root_key()?,
            ));
            addresses.push((MATERIAL.to_owned(), material.first_producer.material_key()));
        }
        PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE => {
            let root: ProceduralSubjectValidityRootV1 = decode(value.clone(), stage)?;
            root.validate(MAX_PROCEDURAL_SUBJECT_SCOPES)?;
            addresses.push((
                PROCEDURAL_SUBJECT_INITIALIZATION_NAMESPACE.to_owned(),
                ProceduralSubjectInitializationV1::key(&root.scope)?,
            ));
            addresses.extend(root.retained_work.iter().map(|work| {
                (
                    PROCEDURAL_FEEDBACK_JOB_NAMESPACE.to_owned(),
                    work.job_id.clone(),
                )
            }));
            addresses.extend(
                root.producer_scopes
                    .iter()
                    .map(|scope| {
                        Ok((
                            PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE.to_owned(),
                            scope.scope_index_key()?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE => {
            let index: ProceduralFeedbackScopeIndexV2 = decode(value.clone(), stage)?;
            index.validate()?;
            if !index.producer_heads.is_empty() {
                addresses.push((
                    PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE.to_owned(),
                    ProceduralSubjectScopeV1 {
                        memory_space_id: index.memory_space_id.clone(),
                        mounted_subject_id: index.mounted_subject_id.clone(),
                    }
                    .root_key()?,
                ));
            }
            addresses.extend(
                index
                    .producer_heads
                    .iter()
                    .map(|head| (HEAD.to_owned(), head.binding_key.clone())),
            );
        }
        HEAD => {
            let head: bm_core::memory::ProceduralProducerHeadV1 = decode(value.clone(), stage)?;
            if !head.validate_contract() {
                return Err(Error::config(stage, "invalid producer head"));
            }
            addresses.push((
                PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE.to_owned(),
                head.scope.scope_index_key()?,
            ));
            addresses.extend(
                head.retained_revisions
                    .iter()
                    .map(|reference| (MATERIAL.to_owned(), reference.material_key())),
            );
        }
        MATERIAL => {
            let binding: bm_core::memory::ProceduralProducerBindingV1 =
                decode(value.clone(), stage)?;
            if !binding.validate_contract() {
                return Err(Error::config(stage, "invalid producer material"));
            }
            addresses.push((HEAD.to_owned(), binding.spec.binding_key()?));
            addresses.push((
                PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE.to_owned(),
                binding.spec.scope.scope_index_key()?,
            ));
            let receipt_key = binding.operation_identity.storage_key();
            addresses.push((
                bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.to_owned(),
                receipt_key.clone(),
            ));
            addresses.push((
                bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.to_owned(),
                receipt_key,
            ));
        }
        _ => {}
    }
    Ok(addresses)
}

struct TranscriptLearningSource<'a> {
    key: bm_core::memory::ConversationKey,
    subject_id: &'a str,
    turn_id: &'a str,
    sequence: u64,
    transcript_digest: &'a str,
    learning_evidence_digest: Option<&'a str>,
}

pub(crate) fn semantic_learning_transcript_precondition(
    platform: &StorePlatform,
    job: &PostTurnGovernanceJobV3,
) -> Result<StoreJsonPrecondition> {
    TranscriptLearningSource::semantic(job)?.read_precondition(platform)
}

impl<'a> TranscriptLearningSource<'a> {
    fn procedural(job: &'a ProceduralFeedbackJobV2) -> Result<Self> {
        Ok(Self {
            key: bm_core::memory::ConversationKey::new(
                &job.feedback_source()?.identity.memory_space_id,
                &job.feedback_source()?.identity.channel_id,
                &job.feedback_source()?.identity.conversation_id,
            )?,
            subject_id: &job.feedback_source()?.identity.mounted_subject_id,
            turn_id: &job.feedback_source()?.identity.turn_id,
            sequence: job.feedback_source()?.transcript_sequence,
            transcript_digest: &job.feedback_source()?.transcript_digest,
            learning_evidence_digest: Some(&job.feedback_source()?.learning_evidence_digest),
        })
    }

    fn semantic(job: &'a PostTurnGovernanceJobV3) -> Result<Self> {
        Ok(Self {
            key: bm_core::memory::ConversationKey::new(
                &job.identity.memory_space_id,
                &job.identity.channel_id,
                &job.identity.conversation_id,
            )?,
            subject_id: &job.identity.mounted_subject_id,
            turn_id: &job.identity.turn_id,
            sequence: job.transcript_sequence,
            transcript_digest: &job.transcript_digest,
            learning_evidence_digest: None,
        })
    }

    fn storage_key(&self) -> String {
        super::platform::transcript_turn_storage_key(&self.key, self.subject_id, self.turn_id)
    }

    fn validate_record(&self, value: &serde_json::Value) -> Result<()> {
        let stage = "post_turn_learning_source_fence";
        let record: bm_core::memory::TranscriptTurnRecord = decode(value.clone(), stage)?;
        if record.key != self.key
            || record.subject != self.subject_id
            || record.turn_id != self.turn_id
            || record.sequence != self.sequence
            || !record.permits_post_turn_learning()
            || bm_core::memory::post_turn_governance_transcript_digest(&record)?
                != self.transcript_digest
            || self.learning_evidence_digest.is_some_and(|expected| {
                record.learning_evidence.as_ref().is_none_or(|evidence| {
                    evidence.learning_evidence_digest != expected
                        || evidence.memory_space_id != self.key.memory_space_id
                        || evidence.mounted_subject_id != self.subject_id
                        || evidence.conversation_id != self.key.conversation_id
                        || evidence.turn_id != self.turn_id
                })
            })
        {
            return Err(Error::conflict(
                stage,
                "canonical source no longer authorizes the exact learning intent",
            ));
        }
        Ok(())
    }

    fn read_precondition(&self, platform: &StorePlatform) -> Result<StoreJsonPrecondition> {
        let key = self.storage_key();
        let document = platform
            .read_json_docs_by_keys("conversation_transcript", std::slice::from_ref(&key))?
            .into_iter()
            .next()
            .ok_or_else(|| {
                Error::conflict(
                    "post_turn_learning_source_fence",
                    "canonical learning source is missing",
                )
            })?;
        self.validate_record(&document.value)?;
        Ok(StoreJsonPrecondition::Exact {
            namespace: "conversation_transcript".to_string(),
            key,
            value: document.value,
        })
    }

    fn require_precondition(&self, preconditions: &[StoreJsonPrecondition]) -> Result<()> {
        let key = self.storage_key();
        let value = preconditions
            .iter()
            .find_map(|condition| match condition {
                StoreJsonPrecondition::Exact {
                    namespace,
                    key: candidate,
                    value,
                } if namespace == "conversation_transcript" && candidate == &key => Some(value),
                _ => None,
            })
            .ok_or_else(|| {
                Error::conflict(
                    "post_turn_learning_source_fence",
                    "learning completion requires exact source CAS",
                )
            })?;
        self.validate_record(value)
    }
}

/// Builds cancellation and derived-owner erasure while the transcript lifecycle
/// caller holds the Store transaction lock. No mutation is committed here.
pub(crate) fn plan_transcript_learning_lifecycle(
    platform: &StorePlatform,
    scope: &StoreEventScope,
    records: &[(
        bm_core::memory::TranscriptTurnRecord,
        bm_core::memory::TranscriptTurnRecord,
    )],
    transition: bm_core::memory::TranscriptLifecycleTransition,
    now_secs: u64,
) -> Result<super::platform::StoreOwnerMutationPlan> {
    use super::schema::AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE;
    use bm_core::memory::{PostTurnGovernanceJobStatusV2, TranscriptLifecycleTransition};
    use bm_core::skills::{
        agent_tool_experience_scope_manifest_key, AgentToolExperienceHeadBindingV1,
        AgentToolExperienceHeadStateV3, AgentToolExperienceOwningScopeV1,
        AgentToolExperienceScopeManifestV1,
    };

    let mut plan = super::platform::StoreOwnerMutationPlan::default();
    let reason = match transition {
        TranscriptLifecycleTransition::Mask => "transcript_masked",
        TranscriptLifecycleTransition::DeleteRaw => "transcript_raw_deleted",
        TranscriptLifecycleTransition::Archive | TranscriptLifecycleTransition::Restore => {
            return Ok(plan)
        }
    };
    let targets = records
        .iter()
        .map(|(record, _)| {
            (
                record.key.memory_space_id.as_str(),
                record.subject.as_str(),
                record.key.channel_id.as_str(),
                record.key.conversation_id.as_str(),
                record.turn_id.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    let stage = "post_turn_learning_lifecycle";
    for document in platform.read_json_namespace(POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE)? {
        let before: PostTurnGovernanceScopeIndexV3 = decode(document.value, stage)?;
        if before.memory_space_id != scope.memory_space_id
            || before.mounted_subject_id != scope.subject_id
        {
            continue;
        }
        let mut after = before.clone();
        for reference in &before.active_jobs {
            let job = super::post_turn_governance::read_job(platform, &reference.job_id)?
                .ok_or_else(|| Error::config(stage, "semantic scope references a missing job"))?;
            if !targets.contains(&(
                job.identity.memory_space_id.as_str(),
                job.identity.mounted_subject_id.as_str(),
                job.identity.channel_id.as_str(),
                job.identity.conversation_id.as_str(),
                job.identity.turn_id.as_str(),
            )) {
                continue;
            }
            let mut cancelled = job.clone();
            cancelled.status = PostTurnGovernanceJobStatusV2::Cancelled;
            cancelled.state_revision =
                checked_increment(job.state_revision, stage, "job revision overflow")?;
            cancelled.next_attempt_at = None;
            cancelled.lease_owner = None;
            cancelled.lease_until = None;
            cancelled.execution_block_authority = None;
            cancelled.blocking_reason = Some(reason.to_string());
            cancelled.last_error_class = None;
            cancelled.terminal_at = Some(now_secs);
            cancelled.updated_at = now_secs;
            cancelled.validate()?;
            if after.recent_terminal_jobs.len() >= MAX_POST_TURN_GOVERNANCE_RECENT_TERMINAL_JOBS {
                return Err(Error::config(
                    stage,
                    "semantic terminal retention is exhausted",
                ));
            }
            after
                .active_jobs
                .retain(|reference| reference.job_id != job.job_id);
            after
                .recent_terminal_jobs
                .push(PostTurnGovernanceJobRefV2::from_job(&cancelled));
            plan.preconditions.push(exact(
                POST_TURN_GOVERNANCE_JOB_NAMESPACE,
                &job.job_id,
                &job,
                stage,
            )?);
            plan.mutations.push(put_json(
                POST_TURN_GOVERNANCE_JOB_NAMESPACE,
                &job.job_id,
                encode(&cancelled, stage)?,
            ));
        }
        if after != before {
            after.recent_terminal_jobs.sort_by(|left, right| {
                right
                    .updated_at
                    .cmp(&left.updated_at)
                    .then_with(|| left.job_id.cmp(&right.job_id))
            });
            after.index_revision =
                checked_increment(before.index_revision, stage, "index revision overflow")?;
            after.updated_at = now_secs;
            after.validate()?;
            plan.preconditions.push(exact(
                POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
                &before.scope_index_key,
                &before,
                stage,
            )?);
            plan.mutations.push(put_json(
                POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
                &before.scope_index_key,
                encode(&after, stage)?,
            ));
        }
    }

    let mut affected_owners = BTreeSet::new();
    let mut withdrawn_sources = BTreeMap::new();
    for document in platform.read_json_namespace(PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE)? {
        let before: ProceduralFeedbackScopeIndexV2 = decode(document.value, stage)?;
        if before.memory_space_id != scope.memory_space_id
            || before.mounted_subject_id != scope.subject_id
        {
            continue;
        }
        let mut after = before.clone();
        for reference in before
            .active_jobs
            .iter()
            .chain(before.recent_terminal_jobs.iter())
        {
            let job = read_job(platform, &reference.job_id)?
                .ok_or_else(|| Error::config(stage, "procedural scope references a missing job"))?;
            if !targets.contains(&(
                job.feedback_source()?.identity.memory_space_id.as_str(),
                job.feedback_source()?.identity.mounted_subject_id.as_str(),
                job.feedback_source()?.identity.channel_id.as_str(),
                job.feedback_source()?.identity.conversation_id.as_str(),
                job.feedback_source()?.identity.turn_id.as_str(),
            )) {
                continue;
            }
            if job.status.is_active() {
                let mut cancelled = job.clone();
                cancelled.status = ProceduralFeedbackJobStatusV1::Cancelled;
                cancelled.state_revision =
                    checked_increment(job.state_revision, stage, "job revision overflow")?;
                cancelled.next_attempt_at = None;
                cancelled.lease_owner = None;
                cancelled.lease_until = None;
                cancelled.last_error_class = None;
                cancelled.terminal_at = Some(now_secs);
                cancelled.updated_at = now_secs;
                cancelled.validate()?;
                after = moved_to_terminal_index(&after, &job, &cancelled, now_secs, stage)?;
                plan.preconditions.push(exact(
                    PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                    &job.job_id,
                    &job,
                    stage,
                )?);
                plan.mutations.push(put_json(
                    PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                    &job.job_id,
                    encode(&cancelled, stage)?,
                ));
            }
        }
        if after != before {
            plan.preconditions.push(exact(
                PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
                &before.scope_index_key,
                &before,
                stage,
            )?);
            plan.mutations.push(put_json(
                PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
                &before.scope_index_key,
                encode(&after, stage)?,
            ));
        }
    }
    // Historical application authority is the durable ledger, not the bounded
    // scheduling index. Derive exact identities before the lifecycle clears
    // evidence; never read other subjects' ledgers and filter them afterwards.
    let mut source_jobs = BTreeSet::new();
    for (record, _) in records {
        let Some(evidence) = &record.learning_evidence else {
            continue;
        };
        let bm_core::memory::ProceduralFeedbackAuthorityV2::Producer {
            producer_revision, ..
        } = &evidence.authority
        else {
            continue;
        };
        let producer = read_verified_procedural_producer(platform, producer_revision)?;
        let producer_scope = &producer.historical.spec.scope;
        if producer_scope.memory_space_id != record.key.memory_space_id
            || producer_scope.mounted_subject_id != record.subject
            || producer_scope.channel_id != record.key.channel_id
            || producer_scope.memory_space_id != scope.memory_space_id
            || producer_scope.mounted_subject_id != scope.subject_id
        {
            return Err(Error::config(stage, "transcript producer scope differs"));
        }
        let identity = bm_core::memory::ProceduralFeedbackIdentityV1::new(
            &record.key.memory_space_id,
            &record.subject,
            &record.key.channel_id,
            &producer_scope.chat_id,
            &record.key.conversation_id,
            &record.turn_id,
        )?;
        let job_id = identity.job_id(&evidence.learning_evidence_digest)?;
        if let Some(job) = read_job(platform, &job_id)? {
            if job.feedback_source()?.identity != identity {
                return Err(Error::config(stage, "exact source job identity differs"));
            }
            if job.status == ProceduralFeedbackJobStatusV1::Succeeded {
                source_jobs.insert(job_id);
            }
        }
    }
    let lifecycle_budget = platform.current_runtime_budget(super::platform::current_unix_secs());
    let source_jobs = source_jobs.into_iter().collect::<Vec<_>>();
    let source_ledgers = platform
        .read_procedural_application_ledgers_with_runtime_budget(&lifecycle_budget, &source_jobs)?;
    if source_ledgers.len() != source_jobs.len() {
        return Err(Error::config(
            stage,
            "succeeded source application ledger is missing",
        ));
    }
    for document in source_ledgers {
        let ledger: ProceduralFeedbackApplicationLedgerV2 = decode(document.value, stage)?;
        if !targets.contains(&(
            ledger.identity.memory_space_id.as_str(),
            ledger.identity.mounted_subject_id.as_str(),
            ledger.identity.channel_id.as_str(),
            ledger.identity.conversation_id.as_str(),
            ledger.identity.turn_id.as_str(),
        )) {
            continue;
        }
        ledger.validate()?;
        let job = read_job(platform, &ledger.job_id)?
            .ok_or_else(|| Error::config(stage, "application ledger job is missing"))?;
        if job.status != ProceduralFeedbackJobStatusV1::Succeeded
            || ledger.identity != job.feedback_source()?.identity
            || ledger.learning_evidence_digest != job.feedback_source()?.learning_evidence_digest
        {
            return Err(Error::config(
                stage,
                "application ledger identity differs from succeeded job",
            ));
        }
        plan.preconditions.push(exact(
            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
            &ledger.job_id,
            &ledger,
            stage,
        )?);
        withdrawn_sources.insert(
            ledger.job_id.clone(),
            ledger.learning_evidence_digest.clone(),
        );
        affected_owners.extend(
            ledger
                .applied_owner_bindings
                .iter()
                .map(|binding| binding.owner_revision_ref().owner_ref),
        );
    }
    let runtime_owners = affected_owners
        .iter()
        .filter(|owner| owner.owner_plane == GovernedMemoryOwnerPlane::RuntimeSkill)
        .cloned()
        .collect::<BTreeSet<_>>();
    plan_runtime_skill_source_withdrawal(
        platform,
        scope,
        &runtime_owners,
        &withdrawn_sources,
        now_secs,
        &mut plan,
    )?;
    affected_owners
        .retain(|owner| owner.owner_plane == GovernedMemoryOwnerPlane::AgentToolExperience);
    if affected_owners.is_empty() {
        return Ok(plan);
    }
    let owning_scope = AgentToolExperienceOwningScopeV1::Subject {
        mounted_subject_id: scope.subject_id.clone(),
    };
    let manifest_key =
        agent_tool_experience_scope_manifest_key(&scope.memory_space_id, &owning_scope)?;
    let manifest = platform
        .read_json_docs_by_keys(
            AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
            std::slice::from_ref(&manifest_key),
        )?
        .into_iter()
        .next()
        .ok_or_else(|| Error::config(stage, "derived experience scope manifest is missing"))?;
    let before_manifest: AgentToolExperienceScopeManifestV1 = decode(manifest.value, stage)?;
    let mut bindings = before_manifest.bindings.clone();
    let mut changed = false;
    for owner in affected_owners {
        let binding = bindings
            .iter_mut()
            .find(|binding| binding.owner_ref == owner)
            .ok_or_else(|| {
                Error::config(stage, "derived experience is absent from scope manifest")
            })?;
        let doc = platform
            .read_json_docs_by_keys(
                AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
                std::slice::from_ref(&binding.head_key),
            )?
            .into_iter()
            .next()
            .ok_or_else(|| Error::config(stage, "derived experience head is missing"))?;
        let head: AgentToolExperienceOwnerHeadV3 = decode(doc.value, stage)?;
        if head.memory_space_id != scope.memory_space_id
            || head.owning_scope != owning_scope
            || *binding != AgentToolExperienceHeadBindingV1::from_head(&head)?
        {
            return Err(Error::config(
                stage,
                "derived experience head differs from exact scope binding",
            ));
        }
        if head.state == AgentToolExperienceHeadStateV3::Tombstoned {
            continue;
        }
        let tombstone = head.tombstone()?;
        plan.preconditions.push(exact(
            AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
            &head.physical_key,
            &head,
            stage,
        )?);
        plan.mutations.push(put_json(
            AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
            &head.physical_key,
            encode(&tombstone, stage)?,
        ));
        for retained in &head.retained_revisions {
            let material = platform
                .read_json_docs_by_keys(
                    AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                    std::slice::from_ref(&retained.material_key),
                )?
                .into_iter()
                .next()
                .ok_or_else(|| Error::config(stage, "retained experience material is missing"))?;
            let typed_material: AgentToolExperienceRevisionMaterialV3 =
                decode(material.value.clone(), stage)?;
            if AgentToolExperienceRetainedRevisionDigestV3::from_material(&typed_material)?
                != *retained
                || typed_material.memory_space_id != scope.memory_space_id
                || typed_material.owning_scope != owning_scope
            {
                return Err(Error::config(
                    stage,
                    "retained material differs from historical owner commitment",
                ));
            }
            plan.preconditions.push(StoreJsonPrecondition::Exact {
                namespace: material.namespace.clone(),
                key: material.key.clone(),
                value: material.value,
            });
            plan.mutations.push(StoreMutation::DeleteJson {
                namespace: material.namespace,
                key: material.key.clone(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: "agent_tool_experience".to_string(),
                record_key: material.key,
            });
        }
        *binding = AgentToolExperienceHeadBindingV1::from_head(&tombstone)?;
        changed = true;
    }
    if changed {
        let after_manifest = AgentToolExperienceScopeManifestV1::build(
            checked_increment(
                before_manifest.revision,
                stage,
                "experience manifest revision overflow",
            )?,
            &scope.memory_space_id,
            owning_scope,
            bindings,
            platform
                .current_runtime_budget(super::platform::current_unix_secs())
                .governed_state_budget
                .max_agent_tool_experience_owners_per_subject,
        )?;
        plan.preconditions.push(exact(
            AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
            &manifest_key,
            &before_manifest,
            stage,
        )?);
        plan.mutations.push(put_json(
            AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
            &manifest_key,
            encode(&after_manifest, stage)?,
        ));
    }
    Ok(plan)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProceduralIntentEnsureOutcome {
    Created,
    AlreadyPresent,
}

pub(crate) fn ensure_post_turn_learning_intents(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    semantic_job: &PostTurnGovernanceJobV3,
    procedural_job: Option<&ProceduralFeedbackJobV2>,
    now_secs: u64,
) -> Result<(
    GovernanceIntentEnsureOutcome,
    Option<ProceduralIntentEnsureOutcome>,
)> {
    semantic_job.validate()?;
    if let Some(job) = procedural_job {
        job.validate()?;
        validate_scope(&scope, job, "post_turn_learning_intents")?;
        if job.feedback_source()?.identity.memory_space_id != semantic_job.identity.memory_space_id
            || job.feedback_source()?.identity.mounted_subject_id
                != semantic_job.identity.mounted_subject_id
            || job.feedback_source()?.identity.channel_id != semantic_job.identity.channel_id
            || job.feedback_source()?.identity.chat_id != semantic_job.identity.chat_id
            || job.feedback_source()?.identity.conversation_id
                != semantic_job.identity.conversation_id
            || job.feedback_source()?.identity.turn_id != semantic_job.identity.turn_id
            || job.feedback_source()?.transcript_sequence != semantic_job.transcript_sequence
            || job.feedback_source()?.transcript_digest != semantic_job.transcript_digest
        {
            return Err(Error::invalid_input(
                "post_turn_learning_intents",
                "semantic and procedural intents must bind the same canonical transcript",
            ));
        }
    }

    let semantic_existing = super::post_turn_governance::read_job(platform, &semantic_job.job_id)?;
    if semantic_existing.as_ref().is_some_and(|existing| {
        existing.identity != semantic_job.identity
            || existing.transcript_sequence != semantic_job.transcript_sequence
            || existing.transcript_digest != semantic_job.transcript_digest
    }) {
        return Err(Error::conflict(
            "post_turn_learning_intents",
            "semantic job identity has divergent transcript authority",
        ));
    }
    let procedural_existing = procedural_job
        .map(|job| read_job(platform, &job.job_id))
        .transpose()?
        .flatten();
    if let (Some(existing), Some(job)) = (procedural_existing.as_ref(), procedural_job) {
        if !same_intent(existing, job) {
            return Err(Error::conflict(
                "post_turn_learning_intents",
                "procedural job identity has divergent learning evidence",
            ));
        }
    }
    let semantic_outcome = if semantic_existing.is_some() {
        GovernanceIntentEnsureOutcome::AlreadyPresent
    } else {
        GovernanceIntentEnsureOutcome::Created
    };
    let procedural_outcome = procedural_job.map(|_| {
        if procedural_existing.is_some() {
            ProceduralIntentEnsureOutcome::AlreadyPresent
        } else {
            ProceduralIntentEnsureOutcome::Created
        }
    });
    if semantic_existing.is_some() && (procedural_job.is_none() || procedural_existing.is_some()) {
        return Ok((semantic_outcome, procedural_outcome));
    }

    let mut mutations = Vec::new();
    let mut preconditions =
        vec![TranscriptLearningSource::semantic(semantic_job)?.read_precondition(platform)?];
    if let Some(job) = procedural_job {
        TranscriptLearningSource::procedural(job)?.require_precondition(&preconditions)?;
    }
    if semantic_existing.is_none() {
        let before_index =
            super::post_turn_governance::read_scope_index(platform, &semantic_job.scope_index_key)?;
        let mut after_index = before_index.clone().unwrap_or_else(|| {
            PostTurnGovernanceScopeIndexV3::empty(&semantic_job.identity, now_secs)
        });
        if after_index.active_jobs.len() >= MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS
            || after_index.recent_terminal_jobs.len()
                >= MAX_POST_TURN_GOVERNANCE_RECENT_TERMINAL_JOBS
        {
            return Err(Error::config(
                "post_turn_learning_intents",
                "semantic job scope capacity or terminal retention is exhausted",
            ));
        }
        after_index
            .active_jobs
            .push(PostTurnGovernanceJobRefV2::from_job(semantic_job));
        after_index.active_jobs.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.job_id.cmp(&right.job_id))
        });
        if before_index.is_some() {
            after_index.index_revision = checked_increment(
                after_index.index_revision,
                "post_turn_learning_intents",
                "semantic scope index revision overflow",
            )?;
        }
        after_index.updated_at = now_secs;
        after_index.validate()?;
        preconditions.extend([
            StoreJsonPrecondition::Absent {
                namespace: POST_TURN_GOVERNANCE_JOB_NAMESPACE.to_string(),
                key: semantic_job.job_id.clone(),
            },
            exact_or_absent(
                POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
                &semantic_job.scope_index_key,
                before_index.as_ref(),
                "post_turn_learning_intents",
            )?,
        ]);
        mutations.extend([
            put_json(
                POST_TURN_GOVERNANCE_JOB_NAMESPACE,
                &semantic_job.job_id,
                encode(semantic_job, "post_turn_learning_intents")?,
            ),
            put_json(
                POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
                &semantic_job.scope_index_key,
                encode(&after_index, "post_turn_learning_intents")?,
            ),
        ]);
        append_binding_reference_mutation(
            platform,
            &semantic_job.execution_binding,
            now_secs,
            &mut mutations,
            &mut preconditions,
        )?;
    }
    if let Some(job) = procedural_job.filter(|_| procedural_existing.is_none()) {
        let before_index = read_scope_index(platform, &job.discovery_root_key)?;
        let mut after_index = match before_index.clone() {
            Some(index) => index,
            None => ProceduralFeedbackScopeIndexV2::empty(
                &job.feedback_source()?.identity.producer_scope(),
                now_secs,
            )?,
        };
        if after_index.active_jobs.len() >= MAX_PROCEDURAL_FEEDBACK_ACTIVE_JOBS
            || after_index.recent_terminal_jobs.len()
                >= MAX_PROCEDURAL_FEEDBACK_RECENT_TERMINAL_JOBS
        {
            return Err(Error::config(
                "post_turn_learning_intents",
                "procedural job scope capacity or terminal retention is exhausted",
            ));
        }
        after_index
            .active_jobs
            .push(ProceduralFeedbackJobRefV2::from_job(job));
        sort_job_refs(&mut after_index.active_jobs);
        if before_index.is_some() {
            after_index.index_revision = checked_increment(
                after_index.index_revision,
                "post_turn_learning_intents",
                "procedural scope index revision overflow",
            )?;
        }
        after_index.updated_at = now_secs;
        after_index.validate()?;
        preconditions.extend([
            StoreJsonPrecondition::Absent {
                namespace: PROCEDURAL_FEEDBACK_JOB_NAMESPACE.to_string(),
                key: job.job_id.clone(),
            },
            exact_or_absent(
                PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
                &job.discovery_root_key,
                before_index.as_ref(),
                "post_turn_learning_intents",
            )?,
        ]);
        mutations.extend([
            put_json(
                PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                &job.job_id,
                encode(job, "post_turn_learning_intents")?,
            ),
            put_json(
                PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
                &job.discovery_root_key,
                encode(&after_index, "post_turn_learning_intents")?,
            ),
        ]);
    }
    let batch = StoreMutationBatch {
        transaction_id: format!(
            "post_turn_learning_intents_{}",
            semantic_job.identity.turn_id
        ),
        operation: "post_turn.learning.enqueue".to_string(),
        scope,
        mutations,
    };
    if let Err(error) = platform.commit_governed_memory_transaction_with_runtime_budget_at(
        batch,
        &preconditions,
        runtime_budget,
        now_secs,
    ) {
        let semantic_now = super::post_turn_governance::read_job(platform, &semantic_job.job_id)?;
        let procedural_now = procedural_job
            .map(|job| read_job(platform, &job.job_id))
            .transpose()?
            .flatten();
        if semantic_now.as_ref().is_some_and(|existing| {
            existing.identity == semantic_job.identity
                && existing.transcript_sequence == semantic_job.transcript_sequence
                && existing.transcript_digest == semantic_job.transcript_digest
        }) && procedural_job.is_none_or(|job| {
            procedural_now
                .as_ref()
                .is_some_and(|existing| same_intent(existing, job))
        }) {
            return Ok((
                GovernanceIntentEnsureOutcome::AlreadyPresent,
                procedural_job.map(|_| ProceduralIntentEnsureOutcome::AlreadyPresent),
            ));
        }
        return Err(error);
    }
    Ok((semantic_outcome, procedural_outcome))
}

#[derive(Clone, Debug)]
pub(crate) struct ProceduralFeedbackCompletionInput {
    pub(crate) job_id: String,
    pub(crate) lease_owner: String,
    pub(crate) lease_epoch: u64,
    pub(crate) operation_id: String,
    pub(crate) actor_subject_id: String,
    pub(crate) experience_mutations: Vec<StoreMutation>,
    pub(crate) experience_preconditions: Vec<StoreJsonPrecondition>,
    pub(crate) applied_owner_bindings: Vec<ProceduralAppliedOwnerBindingV1>,
    pub(crate) accepted_count: u32,
    pub(crate) partially_accepted_count: u32,
    pub(crate) method_dispositions: Vec<bm_core::memory::ProceduralFeedbackMethodDispositionV1>,
    pub(crate) deferred_count: u32,
    pub(crate) rejected_count: u32,
    pub(crate) changed_count: u32,
    pub(crate) reason_digest: String,
    pub(crate) completed_at: u64,
}

#[derive(Clone, Debug)]
pub(crate) enum ProceduralFeedbackCompletionOutcome {
    Committed {
        job: ProceduralFeedbackJobV2,
        receipt: ProceduralFeedbackReceiptV2,
    },
    Replayed {
        job: ProceduralFeedbackJobV2,
        receipt: ProceduralFeedbackReceiptV2,
    },
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn reconcile_procedural_feedback_intents(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    identity: &bm_core::memory::ProceduralFeedbackIdentityV1,
    jobs: &[ProceduralFeedbackJobV2],
    cursor_sequence: u64,
    cursor_turn_id: &str,
    now_secs: u64,
) -> Result<usize> {
    identity.validate()?;
    if scope.memory_space_id != identity.memory_space_id
        || scope.subject_id != identity.mounted_subject_id
        || scope.channel != identity.channel_id
        || scope.chat_id != identity.chat_id
        || cursor_sequence == 0
        || cursor_turn_id.trim().is_empty()
        || cursor_turn_id.trim() != cursor_turn_id
    {
        return Err(Error::invalid_input(
            "procedural_feedback_reconcile",
            "scope and transcript cursor must bind one canonical procedural scope",
        ));
    }
    let scope_index_key = identity.scope_id();
    let before_index = read_scope_index(platform, &scope_index_key)?;
    let mut after_index = match before_index.clone() {
        Some(index) => index,
        None => ProceduralFeedbackScopeIndexV2::empty(&identity.producer_scope(), now_secs)?,
    };
    if after_index
        .reconciliation_cursor(&identity.conversation_id)
        .is_some_and(|cursor| cursor_sequence <= cursor.sequence)
    {
        return Err(Error::conflict(
            "procedural_feedback_reconcile",
            "reconciliation cursor must advance monotonically",
        ));
    }
    let mut preconditions = vec![exact_or_absent(
        PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
        &scope_index_key,
        before_index.as_ref(),
        "procedural_feedback_reconcile",
    )?];
    let mut mutations = Vec::new();
    let mut created = 0usize;
    for job in jobs {
        job.validate()?;
        validate_scope(&scope, job, "procedural_feedback_reconcile")?;
        if job.discovery_root_key != scope_index_key
            || job.feedback_source()?.identity.conversation_id != identity.conversation_id
            || job.feedback_source()?.transcript_sequence > cursor_sequence
        {
            return Err(Error::invalid_input(
                "procedural_feedback_reconcile",
                "reconciliation jobs cross their exact scope or cursor page",
            ));
        }
        match read_job(platform, &job.job_id)? {
            Some(existing) if same_intent(&existing, job) => {
                preconditions.push(exact(
                    PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                    &job.job_id,
                    &existing,
                    "procedural_feedback_reconcile",
                )?);
            }
            Some(_) => {
                return Err(Error::conflict(
                    "procedural_feedback_reconcile",
                    "deterministic procedural job identity has divergent evidence authority",
                ));
            }
            None => {
                preconditions
                    .push(TranscriptLearningSource::procedural(job)?.read_precondition(platform)?);
                if after_index.active_jobs.len() >= MAX_PROCEDURAL_FEEDBACK_ACTIVE_JOBS {
                    return Err(Error::config(
                        "procedural_feedback_reconcile",
                        "exact procedural scope active-job capacity is exhausted",
                    ));
                }
                preconditions.push(StoreJsonPrecondition::Absent {
                    namespace: PROCEDURAL_FEEDBACK_JOB_NAMESPACE.to_string(),
                    key: job.job_id.clone(),
                });
                mutations.push(put_json(
                    PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                    &job.job_id,
                    encode(job, "procedural_feedback_reconcile")?,
                ));
                after_index
                    .active_jobs
                    .push(ProceduralFeedbackJobRefV2::from_job(job));
                created = created.saturating_add(1);
            }
        }
    }
    sort_job_refs(&mut after_index.active_jobs);
    after_index.set_reconciliation_cursor(ProceduralFeedbackReconciliationCursorV1 {
        conversation_id: identity.conversation_id.clone(),
        sequence: cursor_sequence,
        turn_id: cursor_turn_id.to_string(),
        updated_at: now_secs,
    })?;
    if before_index.is_some() {
        after_index.index_revision = checked_increment(
            after_index.index_revision,
            "procedural_feedback_reconcile",
            "scope index revision overflow",
        )?;
    }
    after_index.updated_at = now_secs;
    after_index.validate()?;
    mutations.push(put_json(
        PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
        &scope_index_key,
        encode(&after_index, "procedural_feedback_reconcile")?,
    ));
    platform.commit_governed_memory_transaction_with_runtime_budget_at(
        StoreMutationBatch {
            transaction_id: format!(
                "procedural_feedback_reconcile_{}_{}",
                scope_index_key, cursor_sequence
            ),
            operation: "post_turn.procedural.reconcile".to_string(),
            scope,
            mutations,
        },
        &preconditions,
        runtime_budget,
        now_secs,
    )?;
    Ok(created)
}

pub(crate) fn list_due_procedural_feedback_jobs(
    platform: &StorePlatform,
    scope_index_key: &str,
    subject: &ProceduralSubjectScopeV1,
    now_secs: u64,
    limit: usize,
) -> Result<Vec<ProceduralFeedbackJobV2>> {
    if limit == 0 || limit > MAX_PROCEDURAL_FEEDBACK_ACTIVE_JOBS {
        return Err(Error::invalid_input(
            "procedural_feedback_discover",
            "discovery limit must be positive and bounded",
        ));
    }
    let index = read_scope_index(platform, scope_index_key)?;
    let mut keys = index
        .iter()
        .flat_map(|index| &index.active_jobs)
        .filter(|reference| {
            matches!(
                reference.status,
                ProceduralFeedbackJobStatusV1::Pending
                    | ProceduralFeedbackJobStatusV1::RetryWaiting
                    | ProceduralFeedbackJobStatusV1::Leased
            )
        })
        .map(|reference| reference.job_id.clone())
        .collect::<Vec<_>>();
    let subject_key = subject.root_key()?;
    let root_doc = platform
        .read_json_docs_by_keys(
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
            std::slice::from_ref(&subject_key),
        )?
        .pop();
    if let Some(doc) = root_doc {
        let root: ProceduralSubjectValidityRootV1 =
            decode(doc.value, "procedural_feedback_discover")?;
        root.validate(MAX_PROCEDURAL_SUBJECT_SCOPES)?;
        if root.scope != *subject {
            return Err(Error::config(
                "procedural_feedback_discover",
                "subject reconciliation scope differs",
            ));
        }
        if let bm_core::memory::ProceduralSubjectValidityStateV1::Reconciling { work } = root.state
        {
            keys.push(work.job_id);
        }
    }
    let mut jobs = platform
        .read_json_docs_by_keys(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &keys)?
        .into_iter()
        .map(|doc| decode(doc.value, "procedural_feedback_discover"))
        .collect::<Result<Vec<ProceduralFeedbackJobV2>>>()?;
    jobs.retain(|job| match job.status {
        ProceduralFeedbackJobStatusV1::Pending | ProceduralFeedbackJobStatusV1::ReadyToContinue => {
            job.next_attempt_at
                .is_some_and(|eligible| eligible <= now_secs)
        }
        ProceduralFeedbackJobStatusV1::RetryWaiting => job
            .next_attempt_at
            .is_some_and(|eligible| eligible <= now_secs),
        ProceduralFeedbackJobStatusV1::Leased => {
            job.lease_until.is_some_and(|deadline| deadline <= now_secs)
        }
        _ => false,
    });
    jobs.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.job_id.cmp(&right.job_id))
    });
    jobs.truncate(limit);
    Ok(jobs)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn claim_procedural_feedback_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_until: u64,
    now_secs: u64,
) -> Result<ProceduralFeedbackJobV2> {
    if lease_owner.trim().is_empty() || lease_owner.trim() != lease_owner || lease_until <= now_secs
    {
        return Err(Error::invalid_input(
            "procedural_feedback_claim",
            "canonical lease owner and future deadline are required",
        ));
    }
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found(
            "procedural_feedback_claim",
            "procedural feedback job not found",
        )
    })?;
    validate_scope(&scope, &before_job, "procedural_feedback_claim")?;
    let after_job = before_job.claim(lease_owner, lease_until, now_secs)?;
    commit_job_state_transition(
        platform,
        scope,
        runtime_budget,
        "post_turn.procedural.claim",
        &before_job,
        &after_job,
        now_secs,
    )?;
    Ok(after_job)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn retry_procedural_feedback_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_epoch: u64,
    error_class: ProceduralFeedbackErrorClassV1,
    now_secs: u64,
) -> Result<ProceduralFeedbackJobV2> {
    if !error_class.retryable() {
        return Err(Error::invalid_input(
            "procedural_feedback_retry",
            "non-retryable error must use repair-required transition",
        ));
    }
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found(
            "procedural_feedback_retry",
            "procedural feedback job not found",
        )
    })?;
    validate_active_lease(
        &before_job,
        lease_owner,
        lease_epoch,
        now_secs,
        "procedural_feedback_retry",
    )?;
    validate_scope(&scope, &before_job, "procedural_feedback_retry")?;
    let mut after_job = before_job.clone();
    after_job.state_revision = checked_increment(
        after_job.state_revision,
        "procedural_feedback_retry",
        "job state revision overflow",
    )?;
    after_job.lease_owner = None;
    after_job.lease_until = None;
    after_job.last_error_class = Some(error_class);
    after_job.updated_at = now_secs;
    let exhausted = after_job.attempt_count >= after_job.max_attempts;
    if error_class == ProceduralFeedbackErrorClassV1::BudgetExceeded
        && matches!(
            before_job.work,
            bm_core::memory::ProceduralLearningWorkV1::Reconcile { .. }
        )
    {
        after_job.status = ProceduralFeedbackJobStatusV1::BlockedCapacity;
        after_job.next_attempt_at = None;
        after_job.terminal_at = None;
    } else if exhausted {
        after_job.status = ProceduralFeedbackJobStatusV1::DeadLetter;
        after_job.next_attempt_at = None;
        after_job.terminal_at = Some(now_secs);
    } else {
        let exponent = after_job.attempt_count.saturating_sub(1).min(31);
        let delay = PROCEDURAL_RETRY_BASE_SECS
            .saturating_mul(1_u64 << exponent)
            .min(PROCEDURAL_RETRY_MAX_SECS);
        after_job.status = ProceduralFeedbackJobStatusV1::RetryWaiting;
        after_job.next_attempt_at = Some(now_secs.saturating_add(delay));
        after_job.terminal_at = None;
    }
    after_job.validate()?;
    commit_job_state_transition(
        platform,
        scope,
        runtime_budget,
        "post_turn.procedural.retry",
        &before_job,
        &after_job,
        now_secs,
    )?;
    Ok(after_job)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn terminate_procedural_feedback_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_epoch: u64,
    error_class: ProceduralFeedbackErrorClassV1,
    now_secs: u64,
) -> Result<ProceduralFeedbackJobV2> {
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found(
            "procedural_feedback_repair_required",
            "procedural feedback job not found",
        )
    })?;
    validate_active_lease(
        &before_job,
        lease_owner,
        lease_epoch,
        now_secs,
        "procedural_feedback_repair_required",
    )?;
    validate_scope(&scope, &before_job, "procedural_feedback_repair_required")?;
    let mut after_job = before_job.clone();
    after_job.status = if error_class == ProceduralFeedbackErrorClassV1::AuthorityDenied
        && matches!(
            before_job.work,
            bm_core::memory::ProceduralLearningWorkV1::Feedback { .. }
        ) {
        ProceduralFeedbackJobStatusV1::Cancelled
    } else {
        ProceduralFeedbackJobStatusV1::RepairRequired
    };
    after_job.state_revision = checked_increment(
        after_job.state_revision,
        "procedural_feedback_repair_required",
        "job state revision overflow",
    )?;
    after_job.next_attempt_at = None;
    after_job.lease_owner = None;
    after_job.lease_until = None;
    after_job.last_error_class = Some(error_class);
    after_job.updated_at = now_secs;
    after_job.terminal_at = Some(now_secs);
    after_job.validate()?;
    commit_job_state_transition(
        platform,
        scope,
        runtime_budget,
        if after_job.status == ProceduralFeedbackJobStatusV1::Cancelled {
            "post_turn.procedural.authority_denied"
        } else {
            "post_turn.procedural.repair_required"
        },
        &before_job,
        &after_job,
        now_secs,
    )?;
    Ok(after_job)
}

pub(crate) fn complete_procedural_feedback_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    input: ProceduralFeedbackCompletionInput,
) -> Result<ProceduralFeedbackCompletionOutcome> {
    let before_job = read_job(platform, &input.job_id)?.ok_or_else(|| {
        Error::not_found(
            "procedural_feedback_complete",
            "procedural feedback job not found",
        )
    })?;
    validate_scope(&scope, &before_job, "procedural_feedback_complete")?;
    TranscriptLearningSource::procedural(&before_job)?
        .require_precondition(&input.experience_preconditions)?;
    let identity = MemoryMutationOperationIdentity::new(
        &input.operation_id,
        &scope.memory_space_id,
        &scope.subject_id,
        &input.actor_subject_id,
        MemoryMutationOperationKind::ProceduralLearning,
    )?;
    if before_job.status == ProceduralFeedbackJobStatusV1::Succeeded {
        let receipt = before_job.receipt.clone().ok_or_else(|| {
            Error::config(
                "procedural_feedback_complete",
                "succeeded procedural job is missing its receipt",
            )
        })?;
        let receipt = receipt.feedback()?.clone();
        let completed_lease_revision =
            before_job.state_revision.checked_sub(1).ok_or_else(|| {
                Error::config(
                    "procedural_feedback_complete",
                    "succeeded procedural job has no leased predecessor revision",
                )
            })?;
        if receipt.operation_id != input.operation_id {
            return Err(Error::conflict(
                "procedural_feedback_complete",
                "response-loss replay uses a different operation identity",
            ));
        }
        let replay_plan_digest = procedural_feedback_plan_digest(
            &before_job.job_id,
            completed_lease_revision,
            &input.experience_mutations,
            &input.experience_preconditions,
            &input.applied_owner_bindings,
            input.accepted_count,
            input.partially_accepted_count,
            &input.method_dispositions,
            input.deferred_count,
            input.rejected_count,
            input.changed_count,
        )?;
        let mut generic_docs = platform.read_json_docs_by_keys(
            bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE,
            &[identity.storage_key()],
        )?;
        let generic_receipt = generic_docs.pop().ok_or_else(|| {
            Error::conflict(
                "procedural_feedback_complete",
                "response-loss replay does not match the committed actor identity",
            )
        })?;
        let generic_receipt: MemoryMutationReceipt =
            decode(generic_receipt.value, "procedural_feedback_complete")?;
        generic_receipt.classify_replay(&identity, &replay_plan_digest)?;
        if receipt.operation_id != input.operation_id
            || receipt.mutation_receipt_key != identity.storage_key()
            || receipt.transaction_id != generic_receipt.transaction_id
            || receipt.plan_digest != replay_plan_digest
            || receipt.accepted_count != input.accepted_count
            || receipt.partially_accepted_count != input.partially_accepted_count
            || receipt.method_dispositions != input.method_dispositions
            || receipt.deferred_count != input.deferred_count
            || receipt.rejected_count != input.rejected_count
            || receipt.changed_count != input.changed_count
            || receipt.reason_digest != input.reason_digest
            || receipt.completed_at != input.completed_at
        {
            return Err(Error::conflict(
                "procedural_feedback_complete",
                "response-loss replay differs from the committed procedural operation",
            ));
        }
        return Ok(ProceduralFeedbackCompletionOutcome::Replayed {
            job: before_job,
            receipt,
        });
    }
    validate_active_lease(
        &before_job,
        &input.lease_owner,
        input.lease_epoch,
        input.completed_at,
        "procedural_feedback_complete",
    )?;
    let accounted = input
        .accepted_count
        .checked_add(input.partially_accepted_count)
        .and_then(|value| value.checked_add(input.deferred_count))
        .and_then(|value| value.checked_add(input.rejected_count));
    if accounted != Some(before_job.feedback_source()?.submitted_count)
        || usize::try_from(input.changed_count).ok() != Some(input.applied_owner_bindings.len())
        || input
            .applied_owner_bindings
            .iter()
            .map(|binding| binding.owner_revision_ref().owner_ref)
            .collect::<BTreeSet<_>>()
            .len()
            != input.applied_owner_bindings.len()
        || !is_sha256_digest(&input.reason_digest)
    {
        return Err(Error::invalid_input(
            "procedural_feedback_complete",
            "completion counts or safe reason digest are invalid",
        ));
    }
    let before_index = required_index(platform, &before_job, "procedural_feedback_complete")?;
    let plan_digest = procedural_feedback_plan_digest(
        &before_job.job_id,
        before_job.state_revision,
        &input.experience_mutations,
        &input.experience_preconditions,
        &input.applied_owner_bindings,
        input.accepted_count,
        input.partially_accepted_count,
        &input.method_dispositions,
        input.deferred_count,
        input.rejected_count,
        input.changed_count,
    )?;
    validate_runtime_skill_promotion_sources(
        &before_job,
        &input.experience_mutations,
        &input.experience_preconditions,
    )?;
    // Derive durable contributions from the exact canonical source already in
    // the commit read-set, never from caller-supplied aggregate counters.
    let source_key = TranscriptLearningSource::procedural(&before_job)?.storage_key();
    let source_value = input
        .experience_preconditions
        .iter()
        .find_map(|condition| match condition {
            StoreJsonPrecondition::Exact {
                namespace,
                key,
                value,
            } if namespace == "conversation_transcript" && key == &source_key => Some(value),
            _ => None,
        })
        .ok_or_else(|| {
            Error::conflict(
                "procedural_feedback_complete",
                "exact source precondition is missing",
            )
        })?;
    let source: bm_core::memory::TranscriptTurnRecord =
        decode(source_value.clone(), "procedural_feedback_complete")?;
    let evidence = source.learning_evidence.as_ref().ok_or_else(|| {
        Error::conflict("procedural_feedback_complete", "source evidence is missing")
    })?;
    let producer = read_authorized_procedural_evidence(
        platform,
        evidence,
        &bm_core::memory::ProceduralProducerScopeV1 {
            memory_space_id: before_job
                .feedback_source()?
                .identity
                .memory_space_id
                .clone(),
            mounted_subject_id: before_job
                .feedback_source()?
                .identity
                .mounted_subject_id
                .clone(),
            channel_id: before_job.feedback_source()?.identity.channel_id.clone(),
            chat_id: before_job.feedback_source()?.identity.chat_id.clone(),
        },
    )?;
    let head_value = encode(&producer.head, "procedural_feedback_complete")?;
    if !input.experience_preconditions.iter().any(|condition| matches!(condition,
        StoreJsonPrecondition::Exact { namespace, key, value } if namespace == super::schema::PROCEDURAL_PRODUCER_HEAD_NAMESPACE
            && key == &producer.head.binding_key && value == &head_value)) {
        return Err(Error::conflict("procedural_feedback_complete", "current producer head requires an exact commit fence"));
    }
    let contributions = evidence.execution_contributions_for_job(&before_job)?;
    let validity = required_subject_root(
        platform,
        &before_job.work.subject_scope().root_key()?,
        "procedural_feedback_complete",
    )?;
    if !validity.permits_learning_reads(MAX_PROCEDURAL_SUBJECT_SCOPES)? {
        return Err(Error::conflict(
            "procedural_feedback_complete",
            "subject learning reconciliation must finish before feedback publication",
        ));
    }
    let ledger = ProceduralFeedbackApplicationLedgerV2::build(
        &before_job,
        input.applied_owner_bindings,
        contributions,
        evidence.runtime_skill_contributions_for_job(&before_job, source.created_at)?,
        validity.validity_epoch,
        input.completed_at,
    )?;
    validate_new_applied_post_images(&ledger, &input.experience_mutations)?;
    let post_image_digest = digest_serialized(
        "procedural_feedback_post_image_digest_v1",
        &(
            &ledger.application_digest,
            &plan_digest,
            ProceduralFeedbackJobStatusV1::Succeeded,
        ),
        "procedural_feedback_complete",
    )?;
    let mut receipt = ProceduralFeedbackReceiptV2 {
        mutation_receipt_key: identity.storage_key(),
        schema_version: PROCEDURAL_FEEDBACK_RECEIPT_SCHEMA_VERSION,
        job_id: before_job.job_id.clone(),
        operation_id: input.operation_id,
        transaction_id: String::new(),
        transcript_digest: before_job.feedback_source()?.transcript_digest.clone(),
        learning_evidence_digest: before_job
            .feedback_source()?
            .learning_evidence_digest
            .clone(),
        plan_digest: plan_digest.clone(),
        post_image_digest,
        submitted_count: before_job.feedback_source()?.submitted_count,
        accepted_count: input.accepted_count,
        partially_accepted_count: input.partially_accepted_count,
        method_dispositions: input.method_dispositions,
        deferred_count: input.deferred_count,
        rejected_count: input.rejected_count,
        changed_count: input.changed_count,
        reason_digest: input.reason_digest,
        completed_at: input.completed_at,
    };
    let operation = StoreMutationOperationPlan::new(
        identity,
        plan_digest,
        MemoryMutationEffect::Changed,
        input.experience_mutations.len().saturating_add(3),
        &input.actor_subject_id,
        input.completed_at,
    )?;
    receipt.transaction_id = operation.transaction_id().to_string();
    if !source
        .learning_evidence
        .as_ref()
        .is_some_and(|evidence| receipt.validates_evidence(evidence))
    {
        return Err(Error::config(
            "procedural_feedback_complete",
            "procedural feedback receipt failed canonical validation",
        ));
    }
    let mut after_job = before_job.clone();
    after_job.status = ProceduralFeedbackJobStatusV1::Succeeded;
    after_job.state_revision = checked_increment(
        after_job.state_revision,
        "procedural_feedback_complete",
        "job state revision overflow",
    )?;
    after_job.next_attempt_at = None;
    after_job.lease_owner = None;
    after_job.lease_until = None;
    after_job.last_error_class = None;
    after_job.receipt = Some(bm_core::memory::ProceduralLearningReceiptV1::Feedback {
        receipt: receipt.clone(),
    });
    after_job.updated_at = input.completed_at;
    after_job.terminal_at = Some(input.completed_at);
    after_job.validate()?;
    let after_index = moved_to_terminal_index(
        &before_index,
        &before_job,
        &after_job,
        input.completed_at,
        "procedural_feedback_complete",
    )?;
    let mut mutations = input.experience_mutations;
    mutations.extend([
        put_json(
            PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
            &after_job.job_id,
            encode(&after_job, "procedural_feedback_complete")?,
        ),
        put_json(
            PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
            &after_job.discovery_root_key,
            encode(&after_index, "procedural_feedback_complete")?,
        ),
        put_json(
            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
            &ledger.job_id,
            encode(&ledger, "procedural_feedback_complete")?,
        ),
    ]);
    let mut preconditions = input.experience_preconditions;
    preconditions.push(exact(
        PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
        &validity.physical_key,
        &validity,
        "procedural_feedback_complete",
    )?);
    preconditions.extend([
        exact(
            PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
            &before_job.job_id,
            &before_job,
            "procedural_feedback_complete",
        )?,
        exact(
            PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
            &before_job.discovery_root_key,
            &before_index,
            "procedural_feedback_complete",
        )?,
        StoreJsonPrecondition::Absent {
            namespace: PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE.to_string(),
            key: ledger.job_id.clone(),
        },
    ]);
    let batch = StoreMutationBatch {
        transaction_id: operation.transaction_id().to_string(),
        operation: "post_turn.procedural.complete".to_string(),
        scope,
        mutations,
    };
    match platform.commit_memory_mutation_operation_with_runtime_budget(
        batch,
        &preconditions,
        operation,
        runtime_budget,
    )? {
        StoreMutationOperationOutcome::Committed { .. } => {
            Ok(ProceduralFeedbackCompletionOutcome::Committed {
                job: after_job,
                receipt,
            })
        }
        StoreMutationOperationOutcome::Replayed { .. } => {
            let replayed = read_job(platform, &before_job.job_id)?.ok_or_else(|| {
                Error::config(
                    "procedural_feedback_complete",
                    "replayed completion is missing its procedural job",
                )
            })?;
            let replayed_receipt = replayed.receipt.clone().ok_or_else(|| {
                Error::config(
                    "procedural_feedback_complete",
                    "replayed completion is missing its procedural receipt",
                )
            })?;
            Ok(ProceduralFeedbackCompletionOutcome::Replayed {
                job: replayed,
                receipt: replayed_receipt.feedback()?.clone(),
            })
        }
    }
}

pub(crate) fn read_job(
    platform: &StorePlatform,
    job_id: &str,
) -> Result<Option<ProceduralFeedbackJobV2>> {
    let mut docs = platform
        .read_json_docs_by_keys(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &[job_id.to_string()])?;
    docs.pop()
        .map(|doc| {
            let job: ProceduralFeedbackJobV2 = decode(doc.value, "procedural_feedback_job_read")?;
            validate_persisted_reconciliation_checkpoint(platform, &job)?;
            Ok(job)
        })
        .transpose()
}

pub(crate) fn validate_persisted_reconciliation_checkpoint(
    platform: &StorePlatform,
    job: &ProceduralFeedbackJobV2,
) -> Result<()> {
    job.validate()?;
    if let Some(authority) = &job.checkpoint_authority {
        let receipt = platform
            .read_reconciliation_page_operation(authority)?
            .ok_or_else(|| {
                Error::config(
                    "procedural_reconciliation_checkpoint_proof",
                    "checkpoint mutation proof is missing",
                )
            })?;
        validate_reconciliation_checkpoint_commitment(job, &receipt)?;
    }
    Ok(())
}

pub(crate) fn read_scope_index(
    platform: &StorePlatform,
    key: &str,
) -> Result<Option<ProceduralFeedbackScopeIndexV2>> {
    let mut docs = platform.read_json_docs_by_keys(
        PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
        &[key.to_string()],
    )?;
    docs.pop()
        .map(|doc| decode(doc.value, "procedural_feedback_scope_index_read"))
        .transpose()
}

fn plan_runtime_skill_source_withdrawal(
    platform: &StorePlatform,
    scope: &StoreEventScope,
    owners: &BTreeSet<bm_core::memory::GovernedMemoryOwnerRef>,
    withdrawn_sources: &BTreeMap<String, String>,
    now_secs: u64,
    plan: &mut super::platform::StoreOwnerMutationPlan,
) -> Result<()> {
    use super::schema::{RUNTIME_SKILL_RECORD_NAMESPACE, RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE};
    use bm_core::skills::{
        runtime_skill_scope_manifest_key, RuntimeSkillLifecycleState, RuntimeSkillOwnerBinding,
        RuntimeSkillOwnerRecord, RuntimeSkillOwningScope, RuntimeSkillScopeManifest,
    };
    if owners.is_empty() && withdrawn_sources.is_empty() {
        return Ok(());
    }
    let stage = "procedural_runtime_skill_source_withdrawal";
    let owning_scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: scope.subject_id.clone(),
    };
    let key = runtime_skill_scope_manifest_key(&scope.memory_space_id, &owning_scope)?;
    let document = platform
        .read_json_docs_by_keys(
            RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
            std::slice::from_ref(&key),
        )?
        .into_iter()
        .next();
    let Some(document) = document else {
        if owners.is_empty() {
            return Ok(());
        }
        return Err(Error::config(
            stage,
            "derived runtime skill manifest is missing",
        ));
    };
    let before: RuntimeSkillScopeManifest = decode(document.value, stage)?;
    let limit = platform
        .current_runtime_budget(super::platform::current_unix_secs())
        .governed_state_budget
        .max_retained_runtime_skill_owners_per_scope;
    before.validate_exact(
        &scope.memory_space_id,
        &owning_scope,
        before.owner_bindings.clone(),
        limit,
    )?;
    let mut bindings = before.owner_bindings.clone();
    let mut changed = false;
    if owners
        .iter()
        .any(|owner| !bindings.iter().any(|binding| &binding.owner_ref == owner))
    {
        return Err(Error::config(
            stage,
            "derived runtime skill is absent from scope manifest",
        ));
    }
    for binding in &mut bindings {
        let document = platform
            .read_json_docs_by_keys(
                RUNTIME_SKILL_RECORD_NAMESPACE,
                std::slice::from_ref(&binding.owner_physical_key),
            )?
            .into_iter()
            .next()
            .ok_or_else(|| Error::config(stage, "derived runtime skill owner is missing"))?;
        let owner: RuntimeSkillOwnerRecord = decode(document.value, stage)?;
        if owner.memory_space_id != scope.memory_space_id
            || owner.owning_scope != owning_scope
            || RuntimeSkillOwnerBinding::from_record(&owner)? != *binding
        {
            return Err(Error::config(
                stage,
                "derived runtime skill differs from exact scope binding",
            ));
        }
        let withdrawn_dependency =
            owner
                .intrinsic_contract
                .evidence_bindings
                .iter()
                .any(|evidence| {
                    evidence.kind
                        == bm_core::skills::RuntimeSkillEvidenceKind::ProceduralFeedbackSource
                        && withdrawn_sources.get(&evidence.safe_ref)
                            == Some(&evidence.source_digest)
                });
        if !owners.contains(&owner.owner_ref) && !withdrawn_dependency {
            continue;
        }
        if matches!(
            owner.lifecycle.state,
            RuntimeSkillLifecycleState::Retired | RuntimeSkillLifecycleState::Superseded
        ) {
            continue;
        }
        let retired = owner.retire(now_secs)?;
        plan.preconditions.push(exact(
            RUNTIME_SKILL_RECORD_NAMESPACE,
            &owner.physical_key,
            &owner,
            stage,
        )?);
        plan.mutations.push(put_json(
            RUNTIME_SKILL_RECORD_NAMESPACE,
            &owner.physical_key,
            encode(&retired, stage)?,
        ));
        *binding = RuntimeSkillOwnerBinding::from_record(&retired)?;
        changed = true;
    }
    if changed {
        let after = RuntimeSkillScopeManifest::build(
            checked_increment(before.revision, stage, "manifest revision exhausted")?,
            &scope.memory_space_id,
            owning_scope,
            bindings,
            limit,
        )?;
        plan.preconditions.push(exact(
            RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
            &key,
            &before,
            stage,
        )?);
        plan.mutations.push(put_json(
            RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
            &key,
            encode(&after, stage)?,
        ));
    }
    Ok(())
}

fn validate_runtime_skill_promotion_sources(
    current_job: &ProceduralFeedbackJobV2,
    mutations: &[StoreMutation],
    preconditions: &[StoreJsonPrecondition],
) -> Result<()> {
    use bm_core::skills::{
        RuntimeSkillCreationRef, RuntimeSkillEvidenceKind, RuntimeSkillOwnerRecord,
    };
    let stage = "procedural_runtime_skill_promotion_sources";
    let exact_value = |namespace: &str, key: &str| {
        preconditions
            .iter()
            .find_map(|condition| match condition {
                StoreJsonPrecondition::Exact {
                    namespace: candidate_namespace,
                    key: candidate_key,
                    value,
                } if candidate_namespace == namespace && candidate_key == key => {
                    Some(value.clone())
                }
                _ => None,
            })
            .ok_or_else(|| {
                Error::conflict(stage, "promotion requires every exact source precondition")
            })
    };
    for mutation in mutations {
        let StoreMutation::PutJson {
            namespace, value, ..
        } = mutation
        else {
            continue;
        };
        if namespace != super::schema::RUNTIME_SKILL_RECORD_NAMESPACE {
            continue;
        }
        let owner: RuntimeSkillOwnerRecord = decode(value.clone(), stage)?;
        let RuntimeSkillCreationRef::AgentToolExperiencePromotion {
            experience_owner_ref,
        } = &owner.creation_ref
        else {
            continue;
        };
        if owner.owner_revision != 1 {
            continue;
        }
        if !preconditions.iter().any(|condition| matches!(condition,
            StoreJsonPrecondition::Absent { namespace, key }
            if namespace == super::schema::RUNTIME_SKILL_RECORD_NAMESPACE && key == &owner.physical_key)) {
            return Err(Error::conflict(stage, "promotion requires absent owner CAS"));
        }
        let material = mutations
            .iter()
            .find_map(|mutation| match mutation {
                StoreMutation::PutJson {
                    namespace, value, ..
                } if namespace == AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => {
                    serde_json::from_value::<AgentToolExperienceRevisionMaterialV3>(value.clone())
                        .ok()
                        .filter(|material| &material.owner_ref == experience_owner_ref)
                }
                _ => None,
            })
            .ok_or_else(|| {
                Error::conflict(
                    stage,
                    "promotion requires its exact accepted experience post-image",
                )
            })?;
        let bm_core::skills::AgentToolExperienceBodyV1::Method {
            procedure,
            constraints,
            ..
        } = &material.body
        else {
            return Err(Error::conflict(
                stage,
                "execution statistics cannot create a RuntimeSkill",
            ));
        };
        let expected_constraints = constraints
            .iter()
            .map(|tag| bm_core::skills::RuntimeSkillConstraint {
                kind: bm_core::skills::RuntimeSkillConstraintKind::ObservedToolBoundary,
                policy_safe_ref: tag.clone(),
            })
            .collect::<Vec<_>>();
        if owner.intrinsic_contract.constraints != expected_constraints
            || owner.intrinsic_contract.applicability
                != bm_core::skills::runtime_skill_applicability_for_tool_scope(
                    &material.registry_scope,
                )?
            || owner.lifecycle.observed_at < material.updated_at
            || owner.procedural_content.procedure != *procedure
            || owner.privacy_class != material.privacy_class
        {
            return Err(Error::conflict(
                stage,
                "promotion must preserve source content, time, privacy and boundary constraints",
            ));
        }
        let mut distinct_turns = BTreeSet::new();
        if owner.intrinsic_contract.evidence_bindings.len() < 2 {
            return Err(Error::conflict(
                stage,
                "promotion requires repeated canonical sources",
            ));
        }
        for binding in &owner.intrinsic_contract.evidence_bindings {
            if binding.kind != RuntimeSkillEvidenceKind::ProceduralFeedbackSource {
                return Err(Error::conflict(
                    stage,
                    "promotion source kind is not authoritative",
                ));
            }
            let source: ProceduralFeedbackJobV2 = if binding.safe_ref == current_job.job_id {
                current_job.clone()
            } else {
                decode(
                    exact_value(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &binding.safe_ref)?,
                    stage,
                )?
            };
            source.validate()?;
            if source.job_id != binding.safe_ref
                || source.feedback_source()?.learning_evidence_digest != binding.source_digest
                || source.feedback_source()?.identity.memory_space_id
                    != current_job.feedback_source()?.identity.memory_space_id
                || source.feedback_source()?.identity.mounted_subject_id
                    != current_job.feedback_source()?.identity.mounted_subject_id
            {
                return Err(Error::conflict(stage, "promotion source identity differs"));
            }
            if source.job_id != current_job.job_id {
                if source.status != ProceduralFeedbackJobStatusV1::Succeeded {
                    return Err(Error::conflict(
                        stage,
                        "historical promotion source was not accepted",
                    ));
                }
                let ledger: ProceduralFeedbackApplicationLedgerV2 = decode(
                    exact_value(
                        PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
                        &source.job_id,
                    )?,
                    stage,
                )?;
                ledger.validate()?;
                if ledger.identity != source.feedback_source()?.identity
                    || ledger.learning_evidence_digest
                        != source.feedback_source()?.learning_evidence_digest
                {
                    return Err(Error::conflict(
                        stage,
                        "historical application source differs",
                    ));
                }
                let mut retained_proof = false;
                for applied in &ledger.applied_owner_bindings {
                    let ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                        owner_revision,
                        content_digest,
                    } = applied
                    else {
                        continue;
                    };
                    if &owner_revision.owner_ref != experience_owner_ref {
                        continue;
                    }
                    let key = bm_core::skills::agent_tool_experience_material_key(
                        &material.memory_space_id,
                        &material.owning_scope,
                        experience_owner_ref,
                        owner_revision.owner_revision,
                    )?;
                    let retained: AgentToolExperienceRevisionMaterialV3 = decode(
                        exact_value(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE, &key)?,
                        stage,
                    )?;
                    retained_proof |= retained.validate_contract().accepted
                        && retained.content_digest == *content_digest
                        && retained.owner_ref == *experience_owner_ref
                        && retained.owner_revision == owner_revision.owner_revision;
                }
                if !retained_proof {
                    return Err(Error::conflict(
                        stage,
                        "historical source lacks accepted material proof",
                    ));
                }
            }
            let fence = TranscriptLearningSource::procedural(&source)?;
            fence.require_precondition(preconditions)?;
            let turn: bm_core::memory::TranscriptTurnRecord = decode(
                exact_value("conversation_transcript", &fence.storage_key())?,
                stage,
            )?;
            let evidence = turn
                .learning_evidence
                .as_ref()
                .ok_or_else(|| Error::conflict(stage, "source evidence is missing"))?;
            if matches!(&material.registry_scope, bm_core::skills::AgentToolRegistryScope::Conversation { conversation_id } if conversation_id != &turn.key.conversation_id)
                || evidence.selection_receipt.as_ref().is_some_and(|receipt| {
                    !receipt
                        .applicability
                        .permits_registry_scope(&material.registry_scope)
                })
            {
                return Err(Error::conflict(stage, "promotion source context differs"));
            }
            if owner.lifecycle.observed_at < source.updated_at
                || owner.lifecycle.observed_at < turn.updated_at
            {
                return Err(Error::conflict(
                    stage,
                    "promotion cannot predate its canonical sources",
                ));
            }
            if !evidence.validate_contract()
                || evidence.authority.is_model_inferred()
                || turn.external_content_used
            {
                return Err(Error::conflict(
                    stage,
                    "promotion source authority is denied",
                ));
            }
            let matching_method = evidence.agent_tool_feedback.iter().any(|feedback| {
                bm_core::skills::agent_tool_method_execution_matches_source(
                    &material, &source, &turn, feedback,
                ) == Ok(true)
            });
            if !matching_method {
                return Err(Error::conflict(
                    stage,
                    "promotion method is not present in accepted source",
                ));
            }
            distinct_turns.insert((
                turn.key.storage_key(),
                turn.subject,
                turn.turn_id,
                turn.canonical_turn_digest,
            ));
        }
        if distinct_turns.len() < 2 {
            return Err(Error::conflict(
                stage,
                "promotion sources are not distinct canonical turns",
            ));
        }
    }
    Ok(())
}

fn validate_new_applied_post_images(
    ledger: &ProceduralFeedbackApplicationLedgerV2,
    mutations: &[StoreMutation],
) -> Result<()> {
    let stage = "procedural_feedback_applied_post_image";
    let owner_post_images = mutations.iter().filter(|mutation| matches!(mutation,
        StoreMutation::PutJson { namespace, .. } if namespace == AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE
            || namespace == super::schema::RUNTIME_SKILL_RECORD_NAMESPACE)).count();
    if owner_post_images != ledger.applied_owner_bindings.len() {
        return Err(Error::invalid_input(
            stage,
            "owner mutations and applied bindings must form an exact bijection",
        ));
    }
    for binding in &ledger.applied_owner_bindings {
        let mut matches = 0;
        for mutation in mutations {
            let StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } = mutation
            else {
                continue;
            };
            match binding {
                ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                    owner_revision,
                    content_digest,
                } if namespace == AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => {
                    let material: AgentToolExperienceRevisionMaterialV3 =
                        decode(value.clone(), stage)?;
                    if material.owner_ref == owner_revision.owner_ref
                        && material.owner_revision == owner_revision.owner_revision
                        && material.content_digest == *content_digest
                        && material.physical_key == *key
                        && material.memory_space_id == ledger.identity.memory_space_id
                        && material.owning_scope
                            == (bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
                                mounted_subject_id: ledger.identity.mounted_subject_id.clone(),
                            })
                    {
                        matches += 1;
                    }
                }
                ProceduralAppliedOwnerBindingV1::RuntimeSkill { binding }
                    if namespace == super::schema::RUNTIME_SKILL_RECORD_NAMESPACE =>
                {
                    let owner: bm_core::skills::RuntimeSkillOwnerRecord =
                        decode(value.clone(), stage)?;
                    if owner.memory_space_id == ledger.identity.memory_space_id
                        && owner.owning_scope
                            == (bm_core::skills::RuntimeSkillOwningScope::Subject {
                                mounted_subject_id: ledger.identity.mounted_subject_id.clone(),
                            })
                        && owner.physical_key == *key
                        && bm_core::skills::RuntimeSkillOwnerBinding::from_record(&owner)?
                            == *binding
                    {
                        matches += 1;
                    }
                }
                _ => {}
            }
        }
        if matches != 1 {
            return Err(Error::invalid_input(
                stage,
                "applied binding requires one exact owner mutation post-image",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_procedural_feedback_store_image(
    state: &BackendTransactionState,
    stage: &'static str,
) -> Result<()> {
    let mut jobs = BTreeMap::<String, ProceduralFeedbackJobV2>::new();
    let mut indexes = BTreeMap::<String, ProceduralFeedbackScopeIndexV2>::new();
    let mut subject_roots = BTreeMap::<String, ProceduralSubjectValidityRootV1>::new();
    let mut initializations = BTreeMap::<String, ProceduralSubjectInitializationV1>::new();
    let mut producer_heads = BTreeMap::<String, bm_core::memory::ProceduralProducerHeadV1>::new();
    let mut producer_materials =
        BTreeMap::<String, bm_core::memory::ProceduralProducerBindingV1>::new();
    let mut ledgers = BTreeMap::<String, ProceduralFeedbackApplicationLedgerV2>::new();
    let mut experience_heads = BTreeMap::new();
    let mut experience_materials = BTreeMap::new();
    let mut runtime_owners = Vec::new();
    let mut mutation_receipts = Vec::new();
    let mut mutation_audits = BTreeMap::new();
    for ((namespace, key), value) in &state.json {
        match namespace.as_str() {
            PROCEDURAL_SUBJECT_INITIALIZATION_NAMESPACE => {
                let material: ProceduralSubjectInitializationV1 = decode(value.clone(), stage)?;
                material.validate()?;
                if material.physical_key != *key {
                    return Err(Error::config(
                        stage,
                        "subject initialization physical key differs",
                    ));
                }
                initializations.insert(key.clone(), material);
            }
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE => {
                let root: ProceduralSubjectValidityRootV1 = decode(value.clone(), stage)?;
                root.validate(MAX_PROCEDURAL_SUBJECT_SCOPES)?;
                if root.physical_key != *key {
                    return Err(Error::config(
                        stage,
                        "subject validity physical key differs",
                    ));
                }
                subject_roots.insert(key.clone(), root);
            }
            super::schema::PROCEDURAL_PRODUCER_BINDING_NAMESPACE => {
                let binding: bm_core::memory::ProceduralProducerBindingV1 =
                    decode(value.clone(), stage)?;
                if !binding.validate_contract() || binding.revision_ref()?.material_key() != *key {
                    return Err(Error::config(stage, "invalid producer revision material"));
                }
                producer_materials.insert(key.clone(), binding);
            }
            super::schema::PROCEDURAL_PRODUCER_HEAD_NAMESPACE => {
                let head: bm_core::memory::ProceduralProducerHeadV1 = decode(value.clone(), stage)?;
                if !head.validate_contract() || head.binding_key != *key {
                    return Err(Error::config(stage, "invalid producer head"));
                }
                producer_heads.insert(key.clone(), head);
            }
            PROCEDURAL_FEEDBACK_JOB_NAMESPACE => {
                let job: ProceduralFeedbackJobV2 = decode(value.clone(), stage)?;
                if job.job_id != *key || job.validate().is_err() {
                    return Err(Error::config(stage, "invalid procedural feedback job"));
                }
                jobs.insert(key.clone(), job);
            }
            PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE => {
                let index: ProceduralFeedbackScopeIndexV2 = decode(value.clone(), stage)?;
                if index.scope_index_key != *key || index.validate().is_err() {
                    return Err(Error::config(
                        stage,
                        "invalid procedural feedback scope index",
                    ));
                }
                indexes.insert(key.clone(), index);
            }
            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE => {
                let ledger: ProceduralFeedbackApplicationLedgerV2 = decode(value.clone(), stage)?;
                if ledger.job_id != *key || ledger.validate().is_err() {
                    return Err(Error::config(
                        stage,
                        "invalid procedural feedback application ledger",
                    ));
                }
                ledgers.insert(key.clone(), ledger);
            }
            AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE => {
                let head: AgentToolExperienceOwnerHeadV3 = decode(value.clone(), stage)?;
                experience_heads.insert(key.clone(), head);
            }
            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => {
                let material: AgentToolExperienceRevisionMaterialV3 = decode(value.clone(), stage)?;
                experience_materials.insert(key.clone(), material);
            }
            super::schema::RUNTIME_SKILL_RECORD_NAMESPACE => {
                let owner: bm_core::skills::RuntimeSkillOwnerRecord = decode(value.clone(), stage)?;
                if owner.physical_key != *key || !owner.validate_contract().accepted {
                    return Err(Error::config(
                        stage,
                        "invalid runtime owner in procedural source closure",
                    ));
                }
                runtime_owners.push(owner);
            }
            bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE => {
                let receipt: MemoryMutationReceipt = decode(value.clone(), stage)?;
                receipt.validate_contract()?;
                mutation_receipts.push((key.clone(), receipt));
            }
            bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE => {
                let audit: MemoryMutationAuditRecord = decode(value.clone(), stage)?;
                audit.validate_contract()?;
                mutation_audits.insert(key.clone(), audit);
            }
            _ => {}
        }
    }
    for binding in producer_materials.values() {
        let receipt_key = binding.operation_identity.storage_key();
        let receipt = mutation_receipts
            .iter()
            .find(|(key, _)| key == &receipt_key)
            .map(|(_, receipt)| receipt)
            .ok_or_else(|| Error::config(stage, "producer operation receipt is missing"))?;
        let audit = mutation_audits
            .get(&receipt_key)
            .ok_or_else(|| Error::config(stage, "producer authoritative audit is missing"))?;
        validate_producer_operation_proof(binding, receipt, audit)?;
    }
    let mut referenced = BTreeSet::new();
    let mut referenced_producers = BTreeSet::new();
    let mut referenced_producer_materials = BTreeSet::new();
    for root in subject_roots.values() {
        let initialization = initializations
            .get(&ProceduralSubjectInitializationV1::key(&root.scope)?)
            .ok_or_else(|| {
                Error::config(
                    stage,
                    "subject root is missing its immutable initialization proof",
                )
            })?;
        let first = producer_materials
            .get(&initialization.first_producer.material_key())
            .ok_or_else(|| {
                Error::config(stage, "subject initialization producer material is missing")
            })?;
        initialization.validate_producer(first)?;
        if root.scope != initialization.scope || !root.producer_scopes.contains(&first.spec.scope) {
            return Err(Error::config(
                stage,
                "subject initialization does not bind its original producer scope",
            ));
        }
        for scope in &root.producer_scopes {
            let index = indexes.get(&scope.scope_index_key()?).ok_or_else(|| {
                Error::config(stage, "subject validity root has a dangling producer scope")
            })?;
            if index.producer_scope() != *scope || index.producer_heads.is_empty() {
                return Err(Error::config(
                    stage,
                    "subject validity root cannot bind an unrelated or producerless scope",
                ));
            }
        }
        for reference in &root.retained_work {
            let job = jobs.get(&reference.job_id).ok_or_else(|| {
                Error::config(
                    stage,
                    "subject validity root has a dangling reconciliation job",
                )
            })?;
            let bm_core::memory::ProceduralLearningWorkV1::Reconcile { source } = &job.work else {
                return Err(Error::config(
                    stage,
                    "subject reconciliation reference points to turn feedback",
                ));
            };
            if !referenced.insert(job.job_id.clone())
                || source.reference()? != *reference
                || job.discovery_root_key != root.physical_key
            {
                return Err(Error::config(
                    stage,
                    "reconciliation job and subject retention root differ",
                ));
            }
            if reference.target_epoch < root.validity_epoch {
                if !job.status.is_terminal() {
                    return Err(Error::config(
                        stage,
                        "superseded reconciliation retains an active lease or schedule",
                    ));
                }
                continue;
            }
            validate_subject_reconciliation_job(root, job)?;
        }
    }
    for material in initializations.values() {
        if !subject_roots.contains_key(&material.scope.root_key()?) {
            return Err(Error::config(
                stage,
                "subject initialization proof has a missing root",
            ));
        }
    }
    for index in indexes.values() {
        if !index.producer_heads.is_empty() {
            let key = ProceduralSubjectScopeV1 {
                memory_space_id: index.memory_space_id.clone(),
                mounted_subject_id: index.mounted_subject_id.clone(),
            }
            .root_key()?;
            if !subject_roots
                .get(&key)
                .is_some_and(|root| root.producer_scopes.contains(&index.producer_scope()))
            {
                return Err(Error::config(
                    stage,
                    "producer scope is missing its exact subject validity root",
                ));
            }
        }
        for reference in &index.producer_heads {
            let head = producer_heads
                .get(&reference.binding_key)
                .ok_or_else(|| Error::config(stage, "producer scope root has a dangling head"))?;
            if !referenced_producers.insert(head.binding_key.clone())
                || head.scope != index.producer_scope()
                || head.current != *reference
            {
                return Err(Error::config(
                    stage,
                    "producer scope and current head diverged",
                ));
            }
            let materials = head
                .retained_revisions
                .iter()
                .map(|revision| {
                    let key = revision.material_key();
                    if !referenced_producer_materials.insert(key.clone()) {
                        return Err(Error::config(stage, "producer revision has multiple roots"));
                    }
                    producer_materials
                        .get(&key)
                        .cloned()
                        .ok_or_else(|| Error::config(stage, "producer revision is missing"))
                })
                .collect::<Result<Vec<_>>>()?;
            if !head.validates_materials(&materials) {
                return Err(Error::config(
                    stage,
                    "producer immutable history does not close",
                ));
            }
        }
        for reference in index
            .active_jobs
            .iter()
            .chain(index.recent_terminal_jobs.iter())
        {
            let job = jobs.get(&reference.job_id).ok_or_else(|| {
                Error::config(stage, "procedural scope index has a dangling job reference")
            })?;
            if !referenced.insert(reference.job_id.clone())
                || job.discovery_root_key != index.scope_index_key
                || job.feedback_source()?.identity.memory_space_id != index.memory_space_id
                || job.feedback_source()?.identity.mounted_subject_id != index.mounted_subject_id
                || job.feedback_source()?.identity.channel_id != index.channel_id
                || job.feedback_source()?.identity.chat_id != index.chat_id
                || ProceduralFeedbackJobRefV2::from_job(job) != *reference
            {
                return Err(Error::config(
                    stage,
                    "procedural job and scope index closure diverged",
                ));
            }
        }
    }
    if referenced_producers.len() != producer_heads.len()
        || referenced_producer_materials.len() != producer_materials.len()
    {
        return Err(Error::config(
            stage,
            "procedural producer contains an orphan head or material",
        ));
    }
    if referenced.len() != jobs.len() {
        return Err(Error::config(
            stage,
            "procedural feedback store contains an orphan job",
        ));
    }
    for job in jobs.values() {
        validate_reconciliation_checkpoint_in_state(job, state)?;
        let ledger = ledgers.get(&job.job_id);
        if matches!(
            job.work,
            bm_core::memory::ProceduralLearningWorkV1::Reconcile { .. }
        ) {
            if ledger.is_some() {
                return Err(Error::config(
                    stage,
                    "reconciliation cannot fabricate a feedback application ledger",
                ));
            }
            if job.status == ProceduralFeedbackJobStatusV1::Succeeded {
                let key = job
                    .receipt
                    .as_ref()
                    .ok_or_else(|| Error::config(stage, "completion receipt is missing"))?
                    .mutation_receipt_key();
                let receipt = mutation_receipts
                    .iter()
                    .find(|(candidate, _)| candidate == key)
                    .map(|(_, value)| value)
                    .ok_or_else(|| {
                        Error::config(stage, "reconciliation mutation receipt is missing")
                    })?;
                let audit = mutation_audits.get(key).ok_or_else(|| {
                    Error::config(stage, "reconciliation authoritative audit is missing")
                })?;
                validate_reconciliation_completion(job, receipt, audit)?;
            }
            continue;
        }
        match (job.status, ledger) {
            (ProceduralFeedbackJobStatusV1::Succeeded, Some(ledger))
                if ledger.identity == job.feedback_source()?.identity
                    && ledger.learning_evidence_digest
                        == job.feedback_source()?.learning_evidence_digest => {}
            (ProceduralFeedbackJobStatusV1::Succeeded, _) => {
                return Err(Error::config(
                    stage,
                    "succeeded procedural job is missing its exact application ledger",
                ));
            }
            (_, None) => {}
            (_, Some(_)) => {
                return Err(Error::config(
                    stage,
                    "non-succeeded procedural job cannot own an application ledger",
                ));
            }
        }
    }
    if ledgers.len()
        != jobs
            .values()
            .filter(|job| {
                job.status == ProceduralFeedbackJobStatusV1::Succeeded
                    && matches!(
                        job.work,
                        bm_core::memory::ProceduralLearningWorkV1::Feedback { .. }
                    )
            })
            .count()
    {
        return Err(Error::config(
            stage,
            "procedural feedback store contains an orphan application ledger",
        ));
    }
    for ledger in ledgers.values() {
        let job = jobs
            .get(&ledger.job_id)
            .expect("ledger job closure checked");
        let procedural_receipt = job
            .receipt
            .as_ref()
            .expect("succeeded job receipt validated by canonical job contract")
            .feedback()?;
        if usize::try_from(procedural_receipt.changed_count).ok()
            != Some(ledger.applied_owner_bindings.len())
            || ledger
                .applied_owner_bindings
                .iter()
                .map(|binding| binding.owner_revision_ref().owner_ref)
                .collect::<BTreeSet<_>>()
                .len()
                != ledger.applied_owner_bindings.len()
        {
            return Err(Error::config(
                stage,
                "procedural changed owner count differs from exact application ledger",
            ));
        }
        let matching_receipts = mutation_receipts
            .iter()
            .filter(|(key, _)| key == &procedural_receipt.mutation_receipt_key)
            .collect::<Vec<_>>();
        if matching_receipts.len() != 1 {
            return Err(Error::config(
                stage,
                "procedural feedback completion lacks one exact mutation receipt",
            ));
        }
        let mutation_receipt = &matching_receipts[0].1;
        let mutation_audit = mutation_audits
            .get(&procedural_receipt.mutation_receipt_key)
            .ok_or_else(|| {
                Error::config(
                    stage,
                    "procedural completion lacks its exact mutation audit",
                )
            })?;
        validate_feedback_application_completion(job, ledger, mutation_receipt, mutation_audit)?;
        for applied_binding in &ledger.applied_owner_bindings {
            let ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                owner_revision: applied,
                content_digest,
            } = applied_binding
            else {
                // RuntimeSkill's immutable application is proved by this ledger and the
                // exact MOR receipt, not by comparing a newer mutable current head.
                continue;
            };
            let owning_scope = bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
                mounted_subject_id: ledger.identity.mounted_subject_id.clone(),
            };
            let head_key = bm_core::skills::agent_tool_experience_head_key(
                &ledger.identity.memory_space_id,
                &owning_scope,
                &applied.owner_ref,
            )?;
            let head = experience_heads.get(&head_key).ok_or_else(|| {
                Error::config(
                    stage,
                    "procedural feedback ledger references a missing experience head",
                )
            })?;
            let retained = head
                .retained_revisions
                .iter()
                .find(|retained| retained.owner_revision == applied.owner_revision)
                .ok_or_else(|| {
                    Error::config(
                        stage,
                        "procedural feedback ledger revision is not retained by its owner head",
                    )
                })?;
            if &retained.content_digest != content_digest {
                return Err(Error::config(
                    stage,
                    "applied experience digest differs from retained revision",
                ));
            }
            if head.state == bm_core::skills::AgentToolExperienceHeadStateV3::Tombstoned {
                if experience_materials.values().any(|material| {
                    material.memory_space_id == head.memory_space_id
                        && material.owning_scope == head.owning_scope
                        && material.owner_ref == head.owner_ref
                }) {
                    return Err(Error::config(
                        stage,
                        "tombstoned ledger owner retains raw material",
                    ));
                }
                continue;
            }
            let material = experience_materials
                .get(&retained.material_key)
                .ok_or_else(|| {
                    Error::config(
                        stage,
                        "procedural feedback ledger references a missing experience material",
                    )
                })?;
            if material.physical_key != retained.material_key
                || material.content_digest != retained.content_digest
            {
                return Err(Error::config(
                    stage,
                    "procedural feedback ledger material differs from retained owner authority",
                ));
            }
        }
    }
    for owner in &runtime_owners {
        validate_runtime_usage_store_sources(owner, &jobs, &ledgers, stage)?;
        validate_runtime_promotion_store_sources(owner, state, &jobs, &ledgers, stage)?;
    }
    Ok(())
}

/// Persisted summary is a projection of exact immutable usage contributions,
/// not an independently writable counter. This applies to every RuntimeSkill
/// creation source, including task skills that later receive usage feedback.
fn validate_runtime_usage_store_sources(
    owner: &bm_core::skills::RuntimeSkillOwnerRecord,
    jobs: &BTreeMap<String, ProceduralFeedbackJobV2>,
    ledgers: &BTreeMap<String, ProceduralFeedbackApplicationLedgerV2>,
    stage: &'static str,
) -> Result<()> {
    let summary = &owner.lifecycle.usage_outcome;
    if !owner.validate_contract().accepted {
        return Err(Error::config(stage, "invalid runtime usage owner"));
    }
    let bm_core::skills::RuntimeSkillOwningScope::Subject { mounted_subject_id } =
        &owner.owning_scope
    else {
        return if *summary == bm_core::skills::RuntimeSkillUsageOutcomeSummary::default() {
            Ok(())
        } else {
            Err(Error::config(
                stage,
                "subject feedback cannot grant shared-program usage authority",
            ))
        };
    };
    let mut contributions = Vec::with_capacity(summary.contributions.len());
    for reference in &summary.retained_contributions {
        let ledger = ledgers
            .get(&reference.source_job_id)
            .ok_or_else(|| Error::config(stage, "runtime usage application proof is missing"))?;
        let job = jobs
            .get(&reference.source_job_id)
            .ok_or_else(|| Error::config(stage, "runtime usage source job is missing"))?;
        let contribution = ledger
            .runtime_skill_contributions
            .iter()
            .find(|value| {
                value
                    .reference()
                    .is_ok_and(|candidate| candidate == *reference)
            })
            .ok_or_else(|| {
                Error::config(
                    stage,
                    "runtime usage commitment differs from its application",
                )
            })?;
        if job.status != ProceduralFeedbackJobStatusV1::Succeeded
            || ledger.identity != job.feedback_source()?.identity
            || ledger.learning_evidence_digest != job.feedback_source()?.learning_evidence_digest
            || contribution.source != ledger.identity
            || contribution.locator.owner_revision() >= owner.owner_revision
            || contribution.observed_at < owner.lifecycle.observed_at
            || contribution.observed_at > owner.lifecycle.updated_at
        {
            return Err(Error::config(
                stage,
                "runtime usage source revision, time or authority differs",
            ));
        }
        if !ledger.applied_owner_bindings.iter().any(|binding| matches!(binding,
            ProceduralAppliedOwnerBindingV1::RuntimeSkill { binding }
                if binding.owner_ref == owner.owner_ref && binding.owner_revision <= owner.owner_revision)) {
            return Err(Error::config(stage, "runtime usage was not applied to this exact owner"));
        }
        if summary.contributions.contains(reference) {
            contributions.push(contribution.clone());
        }
    }
    // The reverse edge prevents a resealed owner from erasing its source
    // directory. Store-open has the full image; mutation closure also reads the
    // exact retained sources from the owner pre-image, not just its proposal.
    for ledger in ledgers.values().filter(|ledger| ledger.identity.memory_space_id == owner.memory_space_id
        && ledger.identity.mounted_subject_id == *mounted_subject_id
        && ledger.applied_owner_bindings.iter().any(|binding| matches!(binding,
            ProceduralAppliedOwnerBindingV1::RuntimeSkill { binding }
                if binding.owner_ref == owner.owner_ref && binding.owner_revision <= owner.owner_revision))) {
        for contribution in ledger.runtime_skill_contributions.iter().filter(|contribution|
            contribution.locator.owner_id() == owner.owner_ref.owner_id
                && contribution.outcome != bm_core::memory::ProceduralExecutionOutcomeV1::NotExecuted) {
            let reference = contribution.reference().map_err(|_| Error::config(stage, "invalid applied usage reference"))?;
            if !summary.retained_contributions.contains(&reference) {
                return Err(Error::config(stage, "runtime usage lost an applied source reference"));
            }
        }
    }
    let mut expected = bm_core::memory::reduce_runtime_skill_usage_contributions(
        &owner.memory_space_id,
        mounted_subject_id,
        &owner.owner_ref.owner_id,
        &contributions,
        summary.contributions.len(),
    )
    .map_err(|_| Error::config(stage, "runtime usage contribution closure is invalid"))?;
    expected.retained_contributions = summary.retained_contributions.clone();
    if expected != *summary {
        return Err(Error::config(
            stage,
            "runtime usage summary differs from immutable contributions",
        ));
    }
    Ok(())
}

fn validate_runtime_promotion_store_sources(
    owner: &bm_core::skills::RuntimeSkillOwnerRecord,
    state: &BackendTransactionState,
    jobs: &BTreeMap<String, ProceduralFeedbackJobV2>,
    ledgers: &BTreeMap<String, ProceduralFeedbackApplicationLedgerV2>,
    stage: &'static str,
) -> Result<()> {
    use bm_core::skills::{
        RuntimeSkillCreationRef, RuntimeSkillEvidenceKind, RuntimeSkillLifecycleState,
        RuntimeSkillOwningScope,
    };
    let RuntimeSkillCreationRef::AgentToolExperiencePromotion {
        experience_owner_ref,
    } = &owner.creation_ref
    else {
        return Ok(());
    };
    let RuntimeSkillOwningScope::Subject { mounted_subject_id } = &owner.owning_scope else {
        return Err(Error::config(
            stage,
            "tool promotion cannot acquire shared-program authority",
        ));
    };
    let needs_live_source = !matches!(
        owner.lifecycle.state,
        RuntimeSkillLifecycleState::Retired | RuntimeSkillLifecycleState::Superseded
    );
    let mut source_turns = BTreeSet::new();
    let mut creation_proved = false;
    if owner.intrinsic_contract.evidence_bindings.len() < 2 {
        return Err(Error::config(
            stage,
            "runtime promotion lacks repeated source commitments",
        ));
    }
    for source_ref in &owner.intrinsic_contract.evidence_bindings {
        if source_ref.kind != RuntimeSkillEvidenceKind::ProceduralFeedbackSource {
            return Err(Error::config(
                stage,
                "runtime promotion source kind is not authoritative",
            ));
        }
        let job = jobs.get(&source_ref.safe_ref).ok_or_else(|| {
            Error::config(stage, "runtime promotion references a missing source job")
        })?;
        let ledger = ledgers.get(&source_ref.safe_ref).ok_or_else(|| {
            Error::config(
                stage,
                "runtime promotion references a missing source application",
            )
        })?;
        if job.status != ProceduralFeedbackJobStatusV1::Succeeded
            || job.feedback_source()?.learning_evidence_digest != source_ref.source_digest
            || job.feedback_source()?.identity.memory_space_id != owner.memory_space_id
            || job.feedback_source()?.identity.mounted_subject_id != *mounted_subject_id
            || ledger.identity != job.feedback_source()?.identity
            || ledger.learning_evidence_digest != job.feedback_source()?.learning_evidence_digest
        {
            return Err(Error::config(
                stage,
                "runtime promotion source scope or commitment differs",
            ));
        }
        let accepted = ledger
            .applied_owner_bindings
            .iter()
            .find_map(|binding| match binding {
                ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                    owner_revision,
                    content_digest,
                } if &owner_revision.owner_ref == experience_owner_ref => {
                    Some((owner_revision, content_digest))
                }
                _ => None,
            })
            .ok_or_else(|| {
                Error::config(
                    stage,
                    "runtime promotion source did not accept its experience owner",
                )
            })?;
        source_turns.insert((
            job.feedback_source()?.identity.channel_id.clone(),
            job.feedback_source()?.identity.conversation_id.clone(),
            job.feedback_source()?.identity.turn_id.clone(),
        ));
        for applied in &ledger.applied_owner_bindings {
            if let ProceduralAppliedOwnerBindingV1::RuntimeSkill { binding } = applied {
                if binding.owner_ref == owner.owner_ref && binding.owner_revision == 1 {
                    if owner.owner_revision == 1
                        && *binding
                            != bm_core::skills::RuntimeSkillOwnerBinding::from_record(owner)?
                    {
                        return Err(Error::config(
                            stage,
                            "initial runtime owner differs from its exact application proof",
                        ));
                    }
                    creation_proved = true;
                }
            }
        }
        if !needs_live_source {
            continue;
        }
        let source = TranscriptLearningSource::procedural(job)?;
        let value = state
            .json
            .get(&("conversation_transcript".to_string(), source.storage_key()))
            .ok_or_else(|| {
                Error::config(
                    stage,
                    "active runtime promotion source transcript is missing",
                )
            })?;
        source.validate_record(value)?;
        let turn: bm_core::memory::TranscriptTurnRecord = decode(value.clone(), stage)?;
        let evidence = turn
            .learning_evidence
            .as_ref()
            .ok_or_else(|| Error::config(stage, "runtime source evidence is missing"))?;
        if !evidence.validate_contract()
            || evidence.authority.is_model_inferred()
            || turn.external_content_used
        {
            return Err(Error::config(
                stage,
                "runtime promotion source authority is denied",
            ));
        }
        let scope = bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
            mounted_subject_id: mounted_subject_id.clone(),
        };
        let material_key = bm_core::skills::agent_tool_experience_material_key(
            &owner.memory_space_id,
            &scope,
            experience_owner_ref,
            accepted.0.owner_revision,
        )?;
        let material: AgentToolExperienceRevisionMaterialV3 = decode(
            state
                .json
                .get(&(
                    AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.to_string(),
                    material_key,
                ))
                .ok_or_else(|| {
                    Error::config(stage, "active runtime promotion source material is missing")
                })?
                .clone(),
            stage,
        )?;
        if material.content_digest != *accepted.1 || material.owner_ref != *experience_owner_ref {
            return Err(Error::config(
                stage,
                "runtime promotion accepted source material differs",
            ));
        }
        let actual_execution = evidence.agent_tool_feedback.iter().any(|feedback| {
            bm_core::skills::agent_tool_method_execution_matches_source(
                &material, job, &turn, feedback,
            ) == Ok(true)
                && (owner.owner_revision != 1
                    || matches!(&material.body,
                    bm_core::skills::AgentToolExperienceBodyV1::Method { procedure, .. }
                        if procedure == &owner.procedural_content.procedure))
        });
        if !actual_execution {
            return Err(Error::config(
                stage,
                "runtime promotion source lacks canonical execution evidence",
            ));
        }
    }
    if source_turns.len() < 2 || !creation_proved {
        return Err(Error::config(
            stage,
            "runtime promotion lacks distinct sources or immutable creation proof",
        ));
    }
    Ok(())
}

fn required_index(
    platform: &StorePlatform,
    job: &ProceduralFeedbackJobV2,
    stage: &'static str,
) -> Result<ProceduralFeedbackScopeIndexV2> {
    read_scope_index(platform, &job.discovery_root_key)?.ok_or_else(|| {
        Error::config(
            stage,
            "procedural feedback job is missing its exact scope index",
        )
    })
}

fn updated_active_index(
    before: &ProceduralFeedbackScopeIndexV2,
    before_job: &ProceduralFeedbackJobV2,
    after_job: &ProceduralFeedbackJobV2,
    now_secs: u64,
    stage: &'static str,
) -> Result<ProceduralFeedbackScopeIndexV2> {
    let mut after = before.clone();
    let reference = after
        .active_jobs
        .iter_mut()
        .find(|reference| reference.job_id == before_job.job_id)
        .ok_or_else(|| Error::config(stage, "scope index is missing the active job ref"))?;
    if reference.state_revision != before_job.state_revision {
        return Err(Error::conflict(
            stage,
            "scope index job revision differs from job authority",
        ));
    }
    *reference = ProceduralFeedbackJobRefV2::from_job(after_job);
    sort_job_refs(&mut after.active_jobs);
    after.index_revision =
        checked_increment(after.index_revision, stage, "scope index revision overflow")?;
    after.updated_at = now_secs;
    after.validate()?;
    Ok(after)
}

fn moved_to_terminal_index(
    before: &ProceduralFeedbackScopeIndexV2,
    before_job: &ProceduralFeedbackJobV2,
    after_job: &ProceduralFeedbackJobV2,
    now_secs: u64,
    stage: &'static str,
) -> Result<ProceduralFeedbackScopeIndexV2> {
    if before.recent_terminal_jobs.len() >= MAX_PROCEDURAL_FEEDBACK_RECENT_TERMINAL_JOBS {
        return Err(Error::config(
            stage,
            "procedural terminal receipt retention is exhausted",
        ));
    }
    let mut after = before.clone();
    let reference = after
        .active_jobs
        .iter()
        .find(|reference| reference.job_id == before_job.job_id)
        .ok_or_else(|| Error::config(stage, "scope index is missing the active job ref"))?;
    if reference.state_revision != before_job.state_revision {
        return Err(Error::conflict(
            stage,
            "scope index job revision differs from job authority",
        ));
    }
    after
        .active_jobs
        .retain(|reference| reference.job_id != before_job.job_id);
    after
        .recent_terminal_jobs
        .push(ProceduralFeedbackJobRefV2::from_job(after_job));
    sort_job_refs(&mut after.recent_terminal_jobs);
    after.index_revision =
        checked_increment(after.index_revision, stage, "scope index revision overflow")?;
    after.updated_at = now_secs;
    after.validate()?;
    Ok(after)
}

#[allow(clippy::too_many_arguments)]
fn commit_job_state_transition(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    operation: &str,
    before_job: &ProceduralFeedbackJobV2,
    after_job: &ProceduralFeedbackJobV2,
    now_secs: u64,
) -> Result<()> {
    use bm_core::memory::ProceduralLearningWorkV1;
    let stage = "procedural_feedback_transition";
    match &before_job.work {
        ProceduralLearningWorkV1::Feedback { .. } => {
            let before_index = required_index(platform, before_job, stage)?;
            let after_index = if after_job.status.is_terminal() {
                moved_to_terminal_index(&before_index, before_job, after_job, now_secs, stage)?
            } else {
                updated_active_index(&before_index, before_job, after_job, now_secs, stage)?
            };
            commit_job_and_index(
                platform,
                scope,
                runtime_budget,
                operation,
                before_job,
                after_job,
                &before_index,
                &after_index,
                now_secs,
            )
        }
        ProceduralLearningWorkV1::Reconcile { source } => {
            let before_root =
                required_subject_root(platform, &before_job.discovery_root_key, stage)?;
            if before_root.retained_work.last() != Some(&source.reference()?) {
                return Err(Error::conflict(
                    stage,
                    "reconciliation was superseded by a newer epoch",
                ));
            }
            let mut mutations = vec![put_json(
                PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                &after_job.job_id,
                encode(after_job, stage)?,
            )];
            if after_job.status.is_terminal()
                || after_job.status == ProceduralFeedbackJobStatusV1::BlockedCapacity
            {
                let reason = reconciliation_block_reason(after_job);
                let after_root =
                    before_root.block(source, reason, MAX_PROCEDURAL_SUBJECT_SCOPES)?;
                mutations.push(put_json(
                    PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
                    &after_root.physical_key,
                    encode(&after_root, stage)?,
                ));
            }
            let conditions = vec![
                exact(
                    PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                    &before_job.job_id,
                    before_job,
                    stage,
                )?,
                exact(
                    PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
                    &before_root.physical_key,
                    &before_root,
                    stage,
                )?,
            ];
            platform.commit_governed_memory_transaction_with_runtime_budget_at(
                StoreMutationBatch {
                    transaction_id: format!(
                        "{}_{}_{}",
                        operation.replace('.', "_"),
                        after_job.job_id,
                        after_job.state_revision
                    ),
                    operation: operation.into(),
                    scope,
                    mutations,
                },
                &conditions,
                runtime_budget,
                now_secs,
            )?;
            Ok(())
        }
    }
}

fn required_subject_root(
    platform: &StorePlatform,
    key: &str,
    stage: &'static str,
) -> Result<ProceduralSubjectValidityRootV1> {
    let doc = platform
        .read_json_docs_by_keys(PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE, &[key.to_string()])?
        .pop()
        .ok_or_else(|| Error::config(stage, "procedural subject validity root is missing"))?;
    let root: ProceduralSubjectValidityRootV1 = decode(doc.value, stage)?;
    root.validate(MAX_PROCEDURAL_SUBJECT_SCOPES)?;
    if root.physical_key != key {
        return Err(Error::config(
            stage,
            "procedural subject validity physical key differs",
        ));
    }
    Ok(root)
}

/// Store-authoritative state transition, not a caller-provided capability. Only
/// the exact current reconciliation job may block or complete the same epoch.
pub(crate) fn permits_reconciliation_root_transition(
    mutation: &super::StoreEngineMutation,
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    preconditions: &[StoreJsonPrecondition],
) -> Result<bool> {
    use bm_core::memory::{ProceduralLearningReceiptV1, ProceduralLearningWorkV1};
    let stage = "procedural_reconciliation_transition";
    let super::StoreEngineMutation::PutJson {
        namespace,
        key,
        value,
    } = mutation
    else {
        return Ok(false);
    };
    if namespace != PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE {
        return Ok(false);
    }
    let address = (namespace.clone(), key.clone());
    let Some(before_value) = before.json.get(&address) else {
        return Ok(false);
    };
    let before_root: ProceduralSubjectValidityRootV1 = decode(before_value.clone(), stage)?;
    let after_root: ProceduralSubjectValidityRootV1 = decode(value.clone(), stage)?;
    let Some(reference) = before_root.retained_work.last() else {
        return Ok(false);
    };
    let job_address = (
        PROCEDURAL_FEEDBACK_JOB_NAMESPACE.to_owned(),
        reference.job_id.clone(),
    );
    let Some(before_job_value) = before.json.get(&job_address) else {
        return Ok(false);
    };
    let Some(after_job_value) = after.json.get(&job_address) else {
        return Ok(false);
    };
    let before_job: ProceduralFeedbackJobV2 = decode(before_job_value.clone(), stage)?;
    let after_job: ProceduralFeedbackJobV2 = decode(after_job_value.clone(), stage)?;
    let ProceduralLearningWorkV1::Reconcile { source } = &after_job.work else {
        return Ok(false);
    };
    if before_job.work != after_job.work || source.reference()? != *reference
        || before_job.state_revision.checked_add(1) != Some(after_job.state_revision)
        || ![(&address, before_value), (&job_address, before_job_value)].into_iter().all(|(address, value)|
            preconditions.iter().any(|condition| matches!(condition, StoreJsonPrecondition::Exact { namespace, key, value: expected }
                if namespace == &address.0 && key == &address.1 && expected == value))) {
        return Ok(false);
    }
    after_job.validate()?;
    let expected = match (&before_job.status, &after_job.status, &after_job.receipt) {
        (
            ProceduralFeedbackJobStatusV1::Leased,
            ProceduralFeedbackJobStatusV1::Succeeded,
            Some(ProceduralLearningReceiptV1::Reconcile { receipt }),
        ) => before_root.complete(
            source,
            receipt.canonical_digest()?,
            MAX_PROCEDURAL_SUBJECT_SCOPES,
        )?,
        (
            ProceduralFeedbackJobStatusV1::Leased,
            ProceduralFeedbackJobStatusV1::DeadLetter
            | ProceduralFeedbackJobStatusV1::RepairRequired
            | ProceduralFeedbackJobStatusV1::BlockedCapacity,
            None,
        ) => {
            let reason = reconciliation_block_reason(&after_job);
            before_root.block(source, reason, MAX_PROCEDURAL_SUBJECT_SCOPES)?
        }
        _ => return Ok(false),
    };
    Ok(expected == after_root)
}

#[allow(clippy::too_many_arguments)]
fn commit_job_and_index(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    operation: &str,
    before_job: &ProceduralFeedbackJobV2,
    after_job: &ProceduralFeedbackJobV2,
    before_index: &ProceduralFeedbackScopeIndexV2,
    after_index: &ProceduralFeedbackScopeIndexV2,
    now_secs: u64,
) -> Result<()> {
    let preconditions = vec![
        exact(
            PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
            &before_job.job_id,
            before_job,
            "procedural_feedback_transition",
        )?,
        exact(
            PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
            &before_job.discovery_root_key,
            before_index,
            "procedural_feedback_transition",
        )?,
    ];
    let batch = StoreMutationBatch {
        transaction_id: format!(
            "{}_{}_{}",
            operation.replace('.', "_"),
            after_job.job_id,
            after_job.state_revision
        ),
        operation: operation.to_string(),
        scope,
        mutations: vec![
            put_json(
                PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                &after_job.job_id,
                encode(after_job, "procedural_feedback_transition")?,
            ),
            put_json(
                PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
                &after_job.discovery_root_key,
                encode(after_index, "procedural_feedback_transition")?,
            ),
        ],
    };
    platform.commit_governed_memory_transaction_with_runtime_budget_at(
        batch,
        &preconditions,
        runtime_budget,
        now_secs,
    )?;
    Ok(())
}

fn validate_scope(
    scope: &StoreEventScope,
    job: &ProceduralFeedbackJobV2,
    stage: &'static str,
) -> Result<()> {
    let subject = job.work.subject_scope();
    if scope.memory_space_id != subject.memory_space_id
        || scope.subject_id != subject.mounted_subject_id
        || matches!(&job.work, bm_core::memory::ProceduralLearningWorkV1::Feedback { source }
            if scope.channel != source.identity.channel_id || scope.chat_id != source.identity.chat_id)
    {
        return Err(Error::invalid_input(
            stage,
            "Store event scope differs from procedural job identity",
        ));
    }
    Ok(())
}

fn validate_active_lease(
    job: &ProceduralFeedbackJobV2,
    lease_owner: &str,
    lease_epoch: u64,
    now_secs: u64,
    stage: &'static str,
) -> Result<()> {
    if job.status != ProceduralFeedbackJobStatusV1::Leased
        || job.lease_owner.as_deref() != Some(lease_owner)
        || job.lease_epoch != lease_epoch
        || job.lease_until.is_none_or(|deadline| deadline <= now_secs)
    {
        return Err(Error::conflict(
            stage,
            "operation authority differs from the active unexpired procedural lease",
        ));
    }
    Ok(())
}

fn same_intent(left: &ProceduralFeedbackJobV2, right: &ProceduralFeedbackJobV2) -> bool {
    left.work == right.work
}

fn sort_job_refs(values: &mut [ProceduralFeedbackJobRefV2]) {
    values.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.job_id.cmp(&right.job_id))
    });
}

fn checked_increment(value: u64, stage: &'static str, message: &'static str) -> Result<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| Error::config(stage, message))
}

fn exact_or_absent<T: Serialize>(
    namespace: &str,
    key: &str,
    before: Option<&T>,
    stage: &'static str,
) -> Result<StoreJsonPrecondition> {
    match before {
        Some(value) => exact(namespace, key, value, stage),
        None => Ok(StoreJsonPrecondition::Absent {
            namespace: namespace.to_string(),
            key: key.to_string(),
        }),
    }
}

fn exact<T: Serialize>(
    namespace: &str,
    key: &str,
    value: &T,
    stage: &'static str,
) -> Result<StoreJsonPrecondition> {
    Ok(StoreJsonPrecondition::Exact {
        namespace: namespace.to_string(),
        key: key.to_string(),
        value: encode(value, stage)?,
    })
}

fn put_json(namespace: &str, key: &str, value: serde_json::Value) -> StoreMutation {
    StoreMutation::PutJson {
        namespace: namespace.to_string(),
        key: key.to_string(),
        value,
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: EVENT_PLANE.to_string(),
        record_key: key.to_string(),
    }
}

fn encode<T: Serialize>(value: &T, stage: &'static str) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|error| Error::config(stage, error.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    stage: &'static str,
) -> Result<T> {
    serde_json::from_value(value).map_err(|error| Error::config(stage, error.to_string()))
}

pub(crate) fn digest_serialized<T: Serialize>(
    domain: &str,
    value: &T,
    stage: &'static str,
) -> Result<String> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| Error::config(stage, error.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update((domain.len() as u64).to_be_bytes());
    hasher.update(domain.as_bytes());
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[allow(clippy::too_many_arguments)]
fn procedural_feedback_plan_digest(
    job_id: &str,
    leased_state_revision: u64,
    experience_mutations: &[StoreMutation],
    experience_preconditions: &[StoreJsonPrecondition],
    applied_owner_bindings: &[ProceduralAppliedOwnerBindingV1],
    accepted_count: u32,
    partially_accepted_count: u32,
    method_dispositions: &[bm_core::memory::ProceduralFeedbackMethodDispositionV1],
    deferred_count: u32,
    rejected_count: u32,
    changed_count: u32,
) -> Result<String> {
    digest_serialized(
        "procedural_feedback_plan_digest_v2",
        &(
            job_id,
            leased_state_revision,
            experience_mutations,
            experience_preconditions,
            applied_owner_bindings,
            accepted_count,
            partially_accepted_count,
            method_dispositions,
            deferred_count,
            rejected_count,
            changed_count,
        ),
        "procedural_feedback_complete",
    )
}

fn is_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(all(test, feature = "nonproduction-replay-harness"))]
mod tests {
    use super::*;
    use crate::ProfileId;
    use bm_core::memory::ConversationTranscriptStore;
    use bm_core::memory::ProceduralFeedbackIdentityV1;
    use bm_core::Platform;

    fn source_record(
        platform: &StorePlatform,
        now_secs: u64,
    ) -> bm_core::memory::TranscriptTurnRecord {
        use bm_core::memory::*;
        let delta = CanonicalTurnDelta {
            turn_id: "turn:test".to_string(),
            conversation: ConversationScope {
                channel: "channel:test".to_string(),
                chat_id: "chat:test".to_string(),
                conversation_id: Some("conversation:test".to_string()),
            },
            subject: "subject:test".to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: MemoryTurnSource {
                ingress: IngressKind::User,
                channel: "channel:test".to_string(),
                provider: None,
                protocol: MemoryTurnProtocol::Native,
                endpoint: None,
                model_alias: None,
                model_resolved: None,
                request_id: None,
                client_conversation_hint: None,
            },
            actor: None,
            input_messages: vec![TranscriptInputMessage::user("synthetic source")],
            assistant_message: Some(TranscriptInputMessage::assistant("synthetic result")),
            tool_observations: vec![ToolObservationDigest {
                observation_id: "observation:test".into(),
                call_id: "call:test".into(),
                tool_name: "tool:test".into(),
                summary: "synthetic completed operation".into(),
                external_content: false,
            }],
            external_content_used: false,
            candidate_ids: vec![],
        };
        let producer_scope = ProceduralProducerScopeV1 {
            memory_space_id: "space:test".into(),
            mounted_subject_id: "subject:test".into(),
            channel_id: "channel:test".into(),
            chat_id: "chat:test".into(),
        };
        let tool = ProceduralProducerToolV1 {
            registry_ref: bm_core::skills::AgentToolRegistryRef {
                registry_id: "registry:test".into(),
                fingerprint: "registry-fingerprint:test".into(),
                scope: bm_core::skills::AgentToolRegistryScope::Global,
            },
            tool_id: "tool:test".into(),
            schema_fingerprint: "schema:test".into(),
        };
        let budget = platform.current_runtime_budget(now_secs);
        platform
            .prepare_procedural_selection_authority("space:test", now_secs, &budget)
            .unwrap();
        let (binding, _) = control_procedural_producer(
            platform,
            scope(),
            &budget,
            MemoryMutationOperationIdentity::new(
                "register:test",
                "space:test",
                "subject:test",
                "governor:test",
                MemoryMutationOperationKind::ProceduralProducerControl,
            )
            .unwrap(),
            ProceduralProducerSpecV1 {
                binding_id: "source:test".into(),
                scope: producer_scope,
                principal: ProceduralProducerPrincipalV1::LocalCapability {
                    capability_id: "executor:test".into(),
                },
                source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
                claims: ProceduralProducerClaimsV1 {
                    execution_facts: true,
                    method_declarations: false,
                    usage_feedback: false,
                    source_classifications: vec![ProceduralSourceSensitivity::NonPrivate],
                },
                tools: vec![tool.clone()],
                source_config_ref: "synthetic:test".into(),
            },
            ProceduralProducerStateV1::Active,
            None,
            now_secs,
            &[],
        )
        .unwrap();
        let mut evidence = PostTurnLearningEvidenceV2 {
            schema_version: POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION,
            memory_space_id: "space:test".to_string(),
            mounted_subject_id: delta.subject.clone(),
            conversation_id: "conversation:test".to_string(),
            turn_id: delta.turn_id.clone(),
            canonical_turn_digest: canonical_turn_learning_digest(&delta).expect("turn digest"),
            tool_call_count: 1,
            selection_receipt: None,
            runtime_skill_feedback: vec![],
            agent_skill_feedback: vec![],
            task_learning_feedback: vec![],
            agent_tool_feedback: vec![AdmittedAgentToolEvidenceV1 {
                registry_ref: tool.registry_ref,
                tool_id: "tool:test".into(),
                schema_fingerprint: "schema:test".into(),
                execution_facts: vec![ToolExecutionFactV1 {
                    observation_id: "observation:test".into(),
                    call_id: "call:test".into(),
                    outcome: ToolExecutionOutcome::Succeeded,
                    source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
                    started_at: Some(now_secs),
                    completed_at: Some(now_secs),
                }],
                method_evidence: vec![],
                submitted_method_count: 0,
                rejected_methods: vec![],
            }],
            authority: ProceduralFeedbackAuthorityV2::Producer {
                producer_revision: binding.revision_ref().unwrap(),
                source_authority: binding.spec.source_authority.clone(),
                confirmation: None,
            },
            learning_evidence_digest: String::new(),
        };
        evidence.learning_evidence_digest = evidence.canonical_digest().expect("evidence digest");
        let head = read_verified_procedural_producer(platform, &binding.revision_ref().unwrap())
            .unwrap()
            .head;
        let proof = crate::learning::ProceduralIntakeAuthorization {
            store_authority_digest: platform.learning_store_authority_digest(),
            current_head: head,
            evidence: evidence.clone(),
            store_incarnation: platform.procedural_store_incarnation("space:test").unwrap(),
            source_preconditions: vec![],
        };
        plan_canonical_turn_delta_with_transcript(
            platform.session_store().as_ref(),
            platform,
            "space:test",
            &delta,
            CanonicalTurnTranscriptCommitOptions {
                host_refs: vec![],
                learning_evidence: Some(evidence),
                conversation_alias: None,
                now_secs,
            },
        )
        .unwrap()
        .commit_with(|intent| platform.append_authorized_canonical_turn(intent, &proof))
        .unwrap();
        platform
            .get_turn(
                &ConversationKey::from_delta("space:test", &delta).unwrap(),
                "subject:test",
                &delta.turn_id,
            )
            .unwrap()
            .unwrap()
    }

    fn scope() -> StoreEventScope {
        StoreEventScope::new("agent:test", "owner:test", "channel:test", "chat:test")
            .with_memory_space("space:test")
            .with_subject("subject:test")
            .with_conversation("conversation:test")
    }

    #[test]
    fn application_ledger_cannot_claim_a_missing_or_unlisted_owner_post_image() {
        let identity = ProceduralFeedbackIdentityV1::new(
            "space:test",
            "subject:test",
            "channel:test",
            "chat:test",
            "conversation:test",
            "turn:test",
        )
        .unwrap();
        let job = ProceduralFeedbackJobV2::pending(
            identity,
            1,
            format!("sha256:{}", "a".repeat(64)),
            format!("sha256:{}", "b".repeat(64)),
            1,
            3,
            100,
        )
        .unwrap();
        let empty =
            ProceduralFeedbackApplicationLedgerV2::build(&job, vec![], vec![], vec![], 1, 101)
                .unwrap();
        assert!(validate_new_applied_post_images(&empty, &[]).is_ok());
        let claimed = ProceduralFeedbackApplicationLedgerV2::build(
            &job,
            vec![ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                owner_revision: bm_core::memory::GovernedOwnerRevisionRef::try_new(
                    bm_core::memory::GovernedMemoryOwnerRef::new(
                        GovernedMemoryOwnerPlane::AgentToolExperience,
                        format!("agent_tool_experience:sha256:{}", "c".repeat(64)),
                    ),
                    1,
                )
                .unwrap(),
                content_digest: format!("sha256:{}", "d".repeat(64)),
            }],
            vec![],
            vec![],
            1,
            101,
        )
        .unwrap();
        assert!(validate_new_applied_post_images(&claimed, &[]).is_err());
        assert!(validate_new_applied_post_images(
            &empty,
            &[put_json(
                AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                "unlisted",
                serde_json::json!({})
            )]
        )
        .is_err());
    }

    #[test]
    fn duplicate_source_lifecycle_is_a_persistent_noop() {
        use bm_core::memory::{TranscriptLifecycleRequest, TranscriptLifecycleTransition};
        let platform = StorePlatform::open(
            super::super::StoreBackendConfig::in_memory(ProfileId::DesktopMacosEmbeddedSdk)
                .expect("config"),
        )
        .expect("Store");
        let now_secs = super::super::platform::current_unix_secs();
        let record = source_record(&platform, now_secs);
        for (offset, transition) in [
            (1, TranscriptLifecycleTransition::Mask),
            (3, TranscriptLifecycleTransition::DeleteRaw),
        ] {
            let mut request = TranscriptLifecycleRequest {
                key: record.key.clone(),
                turn_id: Some(record.turn_id.clone()),
                transition,
                reason: "synthetic privacy withdrawal".to_string(),
                requested_by: "subject:test".to_string(),
                requested_at: now_secs + offset,
            };
            assert_eq!(
                platform
                    .apply_lifecycle_request("subject:test", &request)
                    .expect("first lifecycle")
                    .affected_turns,
                1
            );
            let before_retry = platform.export_store_snapshot().expect("snapshot");
            request.requested_at += 1;
            assert_eq!(
                platform
                    .apply_lifecycle_request("subject:test", &request)
                    .expect("retry lifecycle")
                    .affected_turns,
                0
            );
            assert_eq!(
                platform.export_store_snapshot().expect("retry snapshot"),
                before_retry
            );
        }
    }

    #[test]
    fn revoked_canonical_source_cannot_reconstruct_a_procedural_intent() {
        use bm_core::memory::{TranscriptLifecycleRequest, TranscriptLifecycleTransition};
        let platform = StorePlatform::open(
            super::super::StoreBackendConfig::in_memory(ProfileId::DesktopMacosEmbeddedSdk)
                .expect("config"),
        )
        .expect("Store");
        let now_secs = super::super::platform::current_unix_secs();
        let budget = platform.current_runtime_budget(now_secs);
        let record = source_record(&platform, now_secs);
        let identity = ProceduralFeedbackIdentityV1::new(
            "space:test",
            "subject:test",
            "channel:test",
            "chat:test",
            "conversation:test",
            "turn:test",
        )
        .expect("identity");
        let job = ProceduralFeedbackJobV2::pending(
            identity.clone(),
            record.sequence,
            bm_core::memory::post_turn_governance_transcript_digest(&record).expect("digest"),
            record
                .learning_evidence
                .as_ref()
                .expect("evidence")
                .learning_evidence_digest
                .clone(),
            1,
            3,
            now_secs,
        )
        .expect("job");
        TranscriptLearningSource::procedural(&job)
            .expect("source")
            .read_precondition(&platform)
            .expect("positive pre-mask source");
        platform
            .apply_lifecycle_request(
                "subject:test",
                &TranscriptLifecycleRequest {
                    key: record.key,
                    turn_id: Some(record.turn_id),
                    transition: TranscriptLifecycleTransition::Mask,
                    reason: "withdraw".to_string(),
                    requested_by: "subject:test".to_string(),
                    requested_at: now_secs + 1,
                },
            )
            .expect("mask");
        let before = platform
            .export_store_snapshot()
            .expect("before reconstruction");
        let error = reconcile_procedural_feedback_intents(
            &platform,
            scope(),
            &budget,
            &identity,
            &[job],
            1,
            "turn:test",
            now_secs + 2,
        )
        .expect_err("revoked source cannot requeue");
        assert_eq!(error.stage(), "post_turn_learning_source_fence");
        assert_eq!(
            platform
                .export_store_snapshot()
                .expect("after reconstruction"),
            before
        );
    }

    #[test]
    fn response_loss_replays_only_the_exact_committed_procedural_operation() {
        let platform = StorePlatform::open(
            super::super::StoreBackendConfig::in_memory(ProfileId::DesktopMacosEmbeddedSdk)
                .expect("store config"),
        )
        .expect("Store");
        let now_secs = super::super::platform::current_unix_secs();
        let budget = platform.current_runtime_budget(now_secs);
        let record = source_record(&platform, now_secs);
        let identity = ProceduralFeedbackIdentityV1::new(
            "space:test",
            "subject:test",
            "channel:test",
            "chat:test",
            "conversation:test",
            "turn:test",
        )
        .expect("identity");
        let job = ProceduralFeedbackJobV2::pending(
            identity.clone(),
            1,
            bm_core::memory::post_turn_governance_transcript_digest(&record)
                .expect("source digest"),
            record
                .learning_evidence
                .as_ref()
                .expect("evidence")
                .learning_evidence_digest
                .clone(),
            1,
            3,
            now_secs,
        )
        .expect("pending job");
        reconcile_procedural_feedback_intents(
            &platform,
            scope(),
            &budget,
            &identity,
            std::slice::from_ref(&job),
            1,
            "turn:test",
            now_secs,
        )
        .expect("enqueue through reconciliation owner");
        let claimed = claim_procedural_feedback_job(
            &platform,
            scope(),
            &budget,
            &job.job_id,
            "worker:test",
            now_secs + 60,
            now_secs,
        )
        .expect("claim");
        let mut input = ProceduralFeedbackCompletionInput {
            job_id: job.job_id,
            lease_owner: "worker:test".to_string(),
            lease_epoch: claimed.lease_epoch,
            operation_id: "procedural-operation:test".to_string(),
            actor_subject_id: "subject:test".to_string(),
            experience_mutations: Vec::new(),
            experience_preconditions: vec![TranscriptLearningSource::procedural(&claimed)
                .expect("source")
                .read_precondition(&platform)
                .expect("source fence")],
            applied_owner_bindings: Vec::new(),
            // The canonical execution fact is accepted in the durable ledger;
            // it does not invent an applicable method or an owner mutation.
            accepted_count: 1,
            partially_accepted_count: 0,
            method_dispositions: Vec::new(),
            deferred_count: 0,
            rejected_count: 0,
            changed_count: 0,
            reason_digest: format!("sha256:{}", "c".repeat(64)),
            completed_at: now_secs + 1,
        };
        let bm_core::memory::ProceduralFeedbackAuthorityV2::Producer {
            producer_revision, ..
        } = &record.learning_evidence.as_ref().unwrap().authority
        else {
            unreachable!()
        };
        let producer = read_verified_procedural_producer(&platform, producer_revision).unwrap();
        input.experience_preconditions.push(
            exact(
                super::super::schema::PROCEDURAL_PRODUCER_HEAD_NAMESPACE,
                &producer.head.binding_key,
                &producer.head,
                "synthetic_completion",
            )
            .unwrap(),
        );
        let mut missing_fence = input.clone();
        missing_fence.experience_preconditions.clear();
        let before_missing_fence = platform
            .export_store_snapshot()
            .expect("pre-completion snapshot");
        assert_eq!(
            complete_procedural_feedback_job(&platform, scope(), &budget, missing_fence)
                .expect_err("completion cannot omit source fence")
                .stage(),
            "post_turn_learning_source_fence"
        );
        assert_eq!(
            platform
                .export_store_snapshot()
                .expect("after rejected completion"),
            before_missing_fence
        );
        let committed =
            complete_procedural_feedback_job(&platform, scope(), &budget, input.clone())
                .expect("complete");
        assert!(matches!(
            committed,
            ProceduralFeedbackCompletionOutcome::Committed { .. }
        ));
        let before_replay = platform
            .export_store_snapshot()
            .expect("snapshot before replay");
        let replayed = complete_procedural_feedback_job(&platform, scope(), &budget, input.clone())
            .expect("exact replay");
        assert!(matches!(
            replayed,
            ProceduralFeedbackCompletionOutcome::Replayed { .. }
        ));
        assert_eq!(
            platform
                .export_store_snapshot()
                .expect("snapshot after replay"),
            before_replay
        );

        let mut divergent = input;
        divergent.operation_id = "procedural-operation:forged".to_string();
        let error = complete_procedural_feedback_job(&platform, scope(), &budget, divergent)
            .expect_err("different operation identity must not replay");
        assert_eq!(error.class(), Some(bm_core::ErrorClass::Conflict));
        assert_eq!(
            platform
                .export_store_snapshot()
                .expect("snapshot after rejected replay"),
            before_replay
        );
    }
}
