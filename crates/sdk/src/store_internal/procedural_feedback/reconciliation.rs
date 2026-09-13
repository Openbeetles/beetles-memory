//! Atomic pages in the existing procedural lane. Registry/source governance is
//! resolved by SDK; Store owns exact post-images, CAS, MOR and durable progress.
use super::*;
use crate::store_internal::schema::{
    AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE, RUNTIME_SKILL_RECORD_NAMESPACE,
    RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
};
use bm_core::memory::{
    ProceduralLearningReceiptV1, ProceduralLearningWorkV1, ProceduralReconciliationCheckpointV1,
    ProceduralReconciliationCursorV1, ProceduralReconciliationManifestPinV1 as Pin,
    ProceduralReconciliationManifestPinsV1 as Pins, ProceduralReconciliationReceiptV1,
};
use bm_core::skills::{
    AgentToolExperienceHeadBindingV1, AgentToolExperienceOwningScopeV1,
    AgentToolExperienceScopeManifestV1, RuntimeSkillOwnerBinding, RuntimeSkillOwnerRecord,
    RuntimeSkillOwningScope, RuntimeSkillScopeManifest,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProceduralReconciliationAuthorization {
    before_job: ProceduralFeedbackJobV2,
    after_job: ProceduralFeedbackJobV2,
    before_root: ProceduralSubjectValidityRootV1,
    after_root: Option<ProceduralSubjectValidityRootV1>,
    runtime_changes: Vec<bm_core::memory::ProceduralRuntimeSkillTransitionV1>,
}

impl ProceduralReconciliationAuthorization {
    pub(super) fn runtime_preimages(&self) -> Result<Vec<((String, String), serde_json::Value)>> {
        Ok(vec![
            (
                (
                    PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(),
                    self.before_job.job_id.clone(),
                ),
                encode(&self.before_job, "procedural_runtime_skill_control")?,
            ),
            (
                (
                    PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE.into(),
                    self.before_root.physical_key.clone(),
                ),
                encode(&self.before_root, "procedural_runtime_skill_control")?,
            ),
        ])
    }

    pub(super) fn owns_runtime_change(
        &self,
        change: &bm_core::memory::ProceduralRuntimeSkillTransitionV1,
        current: &BTreeMap<(String, String), serde_json::Value>,
    ) -> bool {
        self.runtime_changes.contains(change)
            && serde_json::to_value(&self.after_job).is_ok_and(|value| {
                self.permits_mutation(
                    &crate::store_internal::StoreEngineMutation::PutJson {
                        namespace: PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(),
                        key: self.after_job.job_id.clone(),
                        value,
                    },
                    current,
                )
            })
    }

    pub(super) fn permits_mutation(
        &self,
        mutation: &crate::store_internal::StoreEngineMutation,
        current: &BTreeMap<(String, String), serde_json::Value>,
    ) -> bool {
        let crate::store_internal::StoreEngineMutation::PutJson {
            namespace,
            key,
            value,
        } = mutation
        else {
            return false;
        };
        let same = |namespace: &str, key: &str, expected: serde_json::Value| {
            current.get(&(namespace.to_owned(), key.to_owned())) == Some(&expected)
        };
        if !serde_json::to_value(&self.before_job).is_ok_and(|value| {
            same(
                PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                &self.before_job.job_id,
                value,
            )
        }) || !serde_json::to_value(&self.before_root).is_ok_and(|value| {
            same(
                PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
                &self.before_root.physical_key,
                value,
            )
        }) {
            return false;
        }
        match namespace.as_str() {
            PROCEDURAL_FEEDBACK_JOB_NAMESPACE => {
                *key == self.after_job.job_id
                    && serde_json::to_value(&self.after_job)
                        .is_ok_and(|expected| expected == *value)
            }
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE => self.after_root.as_ref().is_some_and(|root| {
                root.physical_key == *key
                    && serde_json::to_value(root).is_ok_and(|expected| expected == *value)
            }),
            _ => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProceduralReconciliationPageAction {
    Advance,
    Publish,
    RestartSnapshot,
}

pub(crate) struct ProceduralReconciliationPageInput {
    pub claimed_job: ProceduralFeedbackJobV2,
    pub lease_owner: String,
    pub actor_subject_id: String,
    pub checkpoint: ProceduralReconciliationCheckpointV1,
    pub mutations: Vec<StoreMutation>,
    pub preconditions: Vec<StoreJsonPrecondition>,
    pub action: ProceduralReconciliationPageAction,
    pub now: u64,
}

pub(crate) struct ProceduralReconciliationCommit {
    pub job: ProceduralFeedbackJobV2,
    pub receipt: MemoryMutationReceipt,
    /// Whether this operation published completion, not whether the current
    /// durable job has subsequently completed after an earlier page replay.
    pub completed: bool,
    pub replayed: bool,
}

fn page_identity(
    scope: &StoreEventScope,
    before: &ProceduralFeedbackJobV2,
    lease_owner: &str,
    actor_subject_id: &str,
) -> Result<bm_core::memory::ProceduralReconciliationPageAuthorityV1> {
    let stage = "procedural_reconciliation_page";
    before.validate()?;
    validate_scope(scope, before, stage)?;
    if !matches!(before.work, ProceduralLearningWorkV1::Reconcile { .. })
        || before.status != ProceduralFeedbackJobStatusV1::Leased
        || before.lease_owner.as_deref() != Some(lease_owner)
    {
        return Err(Error::conflict(
            stage,
            "page requires its exact claimed reconciliation lease",
        ));
    }
    bm_core::memory::ProceduralReconciliationPageAuthorityV1::for_claim(before, actor_subject_id)
}

fn read_page_replay(
    platform: &StorePlatform,
    before: &ProceduralFeedbackJobV2,
    receipt: MemoryMutationReceipt,
) -> Result<ProceduralReconciliationCommit> {
    let stage = "procedural_reconciliation_page_replay";
    let job = read_job(platform, &before.job_id)?
        .ok_or_else(|| Error::config(stage, "committed page has no durable job"))?;
    job.validate()?;
    let root = required_subject_root(platform, &before.discovery_root_key, stage)?;
    let ProceduralLearningWorkV1::Reconcile { source } = &before.work else {
        return Err(Error::config(stage, "committed page has incompatible work"));
    };
    if job.work != before.work
        || job.state_revision <= before.state_revision
        || !root.retained_work.contains(&source.reference()?)
        || receipt.effect != MemoryMutationEffect::Changed
    {
        return Err(Error::config(
            stage,
            "committed page differs from durable progress",
        ));
    }
    let completed = matches!(&job.receipt, Some(ProceduralLearningReceiptV1::Reconcile { receipt: proof })
        if proof.mutation_receipt_key == receipt.identity.storage_key()
            && proof.transaction_id == receipt.transaction_id);
    Ok(ProceduralReconciliationCommit {
        job,
        receipt,
        completed,
        replayed: true,
    })
}

/// A replay is a query of an already committed operation. Check it before
/// live manifests, clocks or root epochs can invalidate an old claim.
pub(crate) fn replay_reconciliation_page(
    platform: &StorePlatform,
    scope: &StoreEventScope,
    before: &ProceduralFeedbackJobV2,
    lease_owner: &str,
    actor_subject_id: &str,
) -> Result<Option<ProceduralReconciliationCommit>> {
    let authority = page_identity(scope, before, lease_owner, actor_subject_id)?;
    validate_persisted_reconciliation_checkpoint(platform, before)?;
    if let Some(receipt) = platform.read_reconciliation_page_operation(&authority)? {
        return read_page_replay(platform, before, receipt).map(Some);
    }
    // A missing old-page receipt is never permission to re-plan an old claim.
    if read_job(platform, &before.job_id)?.as_ref() != Some(before) {
        return Err(Error::conflict(
            "procedural_reconciliation_page_replay",
            "uncommitted page requires the exact current claim",
        ));
    }
    Ok(None)
}

pub(crate) fn commit_reconciliation_page(
    platform: &StorePlatform,
    scope: StoreEventScope,
    budget: &RuntimeBudgetReport,
    input: ProceduralReconciliationPageInput,
) -> Result<ProceduralReconciliationCommit> {
    let stage = "procedural_reconciliation_page";
    let before = &input.claimed_job;
    let finish = input.action == ProceduralReconciliationPageAction::Publish;
    let restart = input.action == ProceduralReconciliationPageAction::RestartSnapshot;
    let authority = page_identity(&scope, before, &input.lease_owner, &input.actor_subject_id)?;
    if let Some(replay) = replay_reconciliation_page(
        platform,
        &scope,
        before,
        &input.lease_owner,
        &input.actor_subject_id,
    )? {
        return Ok(replay);
    }
    let identity = authority.operation.clone();
    let intent = authority.intent_digest(&input.checkpoint)?;
    let ProceduralLearningWorkV1::Reconcile { source } = &before.work else {
        return Err(Error::invalid_input(
            stage,
            "page requires causal reconciliation work",
        ));
    };
    let root = required_subject_root(platform, &before.discovery_root_key, stage)?;
    input.checkpoint.validate_for(source)?;
    if root.retained_work.last() != Some(&source.reference()?)
        || input.checkpoint.root_revision != root.revision
        || input.checkpoint.root_digest != root.content_digest
        || before.status != ProceduralFeedbackJobStatusV1::Leased
        || before.lease_owner.as_deref() != Some(input.lease_owner.as_str())
        || before.lease_until.is_none_or(|until| until <= input.now)
        || input.now < before.updated_at
    {
        return Err(Error::conflict(
            stage,
            "reconciliation lease or validity root changed",
        ));
    }
    let max_outputs = budget
        .governed_state_budget
        .max_agent_tool_experience_owners_per_subject
        .checked_mul(
            budget
                .governed_state_budget
                .max_agent_tool_experience_revisions_per_owner,
        )
        .and_then(|value| {
            value.checked_add(
                budget
                    .governed_state_budget
                    .max_retained_runtime_skill_owners_per_scope,
            )
        })
        .ok_or_else(|| {
            crate::store_internal::store_budget_error("reconciliation proof bound overflow")
        })?;
    if input.checkpoint.verified_owners.len() > max_outputs
        || input.checkpoint.dependencies.len() > budget.store_budget.kv_max_entries
    {
        return Err(crate::store_internal::store_budget_error(
            "reconciliation proof capacity exhausted",
        ));
    }
    let mut preconditions = input.preconditions;
    let observed = manifest_pins(platform, &source.scope, &[], budget, &mut preconditions)?;
    let expected_before = before
        .checkpoint
        .as_ref()
        .map(|checkpoint| &checkpoint.current_manifests)
        .unwrap_or(&input.checkpoint.original_manifests);
    if restart {
        if !input.mutations.is_empty()
            || input.checkpoint
                != ProceduralReconciliationCheckpointV1::begin(
                    source,
                    &root,
                    observed.clone(),
                    input.now,
                )?
        {
            return Err(Error::config(
                stage,
                "restart must bind an empty exact current snapshot",
            ));
        }
    } else if observed != *expected_before {
        return Err(Error::conflict(
            stage,
            "reconciliation manifest changed outside this work",
        ));
    }
    let post = manifest_pins(
        platform,
        &source.scope,
        &input.mutations,
        budget,
        &mut Vec::new(),
    )?;
    if post != input.checkpoint.current_manifests {
        return Err(Error::config(
            stage,
            "checkpoint does not bind exact manifest post-images",
        ));
    }
    validate_output_inventory(
        platform,
        &source.scope,
        &input.checkpoint,
        &input.mutations,
        budget,
        finish,
    )?;
    let operation_id = authority.operation_id();
    let operation = StoreMutationOperationPlan::new(
        identity.clone(),
        intent.clone(),
        MemoryMutationEffect::Changed,
        input.mutations.len() + if finish { 2 } else { 1 },
        &input.actor_subject_id,
        input.now,
    )?;
    let mut after_root = None;
    let after = if finish {
        if before.checkpoint.as_ref() != Some(&input.checkpoint)
            || !matches!(
                input.checkpoint.cursor,
                ProceduralReconciliationCursorV1::Complete
            )
            || !input.mutations.is_empty()
        {
            return Err(Error::conflict(
                stage,
                "final publication requires the exact completed checkpoint",
            ));
        }
        let receipt = ProceduralReconciliationReceiptV1 {
            schema_version: 1,
            work: source.reference()?,
            operation_id,
            transaction_id: operation.transaction_id().to_owned(),
            mutation_receipt_key: identity.storage_key(),
            plan_digest: intent,
            post_image_digest: digest_serialized(
                "procedural_reconciliation_post_image_v1",
                &(
                    &input.checkpoint.content_digest,
                    &input.checkpoint.current_manifests,
                ),
                stage,
            )?,
            checkpoint_digest: input.checkpoint.content_digest.clone(),
            completed_at: input.now,
        };
        let mut after = before.clone();
        after.state_revision =
            checked_increment(before.state_revision, stage, "job revision overflow")?;
        after.status = ProceduralFeedbackJobStatusV1::Succeeded;
        after.lease_owner = None;
        after.lease_until = None;
        after.next_attempt_at = None;
        after.last_error_class = None;
        after.updated_at = input.now;
        after.terminal_at = Some(input.now);
        after.receipt = Some(ProceduralLearningReceiptV1::Reconcile {
            receipt: receipt.clone(),
        });
        after.validate()?;
        after_root = Some(root.complete(
            source,
            receipt.canonical_digest()?,
            MAX_PROCEDURAL_SUBJECT_SCOPES,
        )?);
        after
    } else if restart {
        before.restart_reconciliation_snapshot(
            input.checkpoint,
            &input.lease_owner,
            before.lease_epoch,
            input.now,
        )?
    } else {
        before.continue_reconciliation(
            input.checkpoint,
            authority,
            &input.lease_owner,
            before.lease_epoch,
            input.now,
        )?
    };
    preconditions.push(exact(
        PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
        &before.job_id,
        before,
        stage,
    )?);
    preconditions.push(exact(
        PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
        &root.physical_key,
        &root,
        stage,
    )?);
    // The page and asset planners pin shared roots independently. Merge exact
    // observations without weakening a divergent CAS or duplicating its key.
    super::super::post_turn_governance::merge_completion_preconditions(&mut preconditions, [])?;
    let mut runtime_changes = Vec::new();
    for mutation in &input.mutations {
        let StoreMutation::PutJson {
            namespace,
            key,
            value,
            ..
        } = mutation
        else {
            continue;
        };
        if namespace != RUNTIME_SKILL_RECORD_NAMESPACE {
            continue;
        }
        let record: RuntimeSkillOwnerRecord = decode(value.clone(), stage)?;
        let binding = RuntimeSkillOwnerBinding::from_record(&record)?;
        let proof = bm_core::memory::ProceduralAppliedOwnerBindingV1::RuntimeSkill {
            binding: binding.clone(),
        };
        if !after.checkpoint.as_ref().is_some_and(|checkpoint| {
            checkpoint
                .verified_owners
                .iter()
                .any(|output| output.binding == proof)
        }) || before.checkpoint.as_ref().is_some_and(|checkpoint| {
            checkpoint
                .verified_owners
                .iter()
                .any(|output| output.binding == proof)
        }) {
            return Err(Error::config(
                stage,
                "runtime mutation requires this page's new exact output",
            ));
        }
        let previous = preconditions
            .iter()
            .find_map(|condition| match condition {
                StoreJsonPrecondition::Exact {
                    namespace: ns,
                    key: k,
                    value,
                } if ns == namespace && k == key => Some(value.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                Error::config(
                    stage,
                    "runtime page output requires its exact owner pre-image",
                )
            })?;
        let previous: RuntimeSkillOwnerRecord = decode(previous, stage)?;
        runtime_changes.push(bm_core::memory::ProceduralRuntimeSkillTransitionV1 {
            before: Some(RuntimeSkillOwnerBinding::from_record(&previous)?),
            after: Some(binding),
        });
    }
    let authorization = ProceduralReconciliationAuthorization {
        before_job: before.clone(),
        after_job: after.clone(),
        before_root: root,
        after_root: after_root.clone(),
        runtime_changes,
    };
    let mut mutations = input.mutations;
    mutations.push(put_json(
        PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
        &after.job_id,
        encode(&after, stage)?,
    ));
    if let Some(root) = after_root {
        mutations.push(put_json(
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
            &root.physical_key,
            encode(&root, stage)?,
        ));
    }
    let operation = operation.authorize_procedural_reconciliation(authorization);
    let batch = StoreMutationBatch {
        transaction_id: operation.transaction_id().to_owned(),
        operation: "post_turn.procedural.reconcile".into(),
        scope,
        mutations,
    };
    let receipt = match platform.commit_memory_mutation_operation_with_runtime_budget(
        batch,
        &preconditions,
        operation,
        budget,
    )? {
        StoreMutationOperationOutcome::Committed { receipt, .. } => receipt,
        StoreMutationOperationOutcome::Replayed { receipt } => {
            return read_page_replay(platform, before, receipt)
        }
    };
    Ok(ProceduralReconciliationCommit {
        job: after,
        receipt,
        completed: finish,
        replayed: false,
    })
}

fn post_value(
    platform: &StorePlatform,
    namespace: &str,
    key: &str,
    mutations: &[StoreMutation],
) -> Result<Option<serde_json::Value>> {
    for mutation in mutations {
        match mutation {
            StoreMutation::PutJson {
                namespace: ns,
                key: candidate,
                value,
                ..
            } if ns == namespace && candidate == key => return Ok(Some(value.clone())),
            StoreMutation::DeleteJson {
                namespace: ns,
                key: candidate,
                ..
            } if ns == namespace && candidate == key => return Ok(None),
            _ => {}
        }
    }
    Ok(platform
        .read_json_docs_by_keys(namespace, &[key.to_owned()])?
        .pop()
        .map(|document| document.value))
}

pub(crate) fn resume_reconciliation_capacity(
    platform: &StorePlatform,
    scope: StoreEventScope,
    budget: &RuntimeBudgetReport,
    identity: MemoryMutationOperationIdentity,
    request: crate::MemoryProceduralReconciliationResumeRequest,
    now: u64,
) -> Result<crate::MemoryProceduralReconciliationResumeReport> {
    let stage = "procedural_reconciliation_resume";
    if request.job_id.trim().is_empty()
        || request.job_id.trim() != request.job_id
        || request.expected_state_revision == 0
    {
        return Err(Error::invalid_input(
            stage,
            "exact job identity and expected state revision are required",
        ));
    }
    let intent = digest_serialized(
        "procedural_reconciliation_resume_intent_v1",
        &(&request.job_id, request.expected_state_revision),
        stage,
    )?;
    let read_committed = |receipt: MemoryMutationReceipt| -> Result<crate::MemoryProceduralReconciliationResumeReport> {
        let job = read_job(platform, &request.job_id)?.ok_or_else(|| Error::config(stage, "resumed work is missing"))?;
        job.validate()?;
        validate_scope(&scope, &job, stage)?;
        let ProceduralLearningWorkV1::Reconcile { source } = &job.work else {
            return Err(Error::config(stage, "resume receipt cannot refer to feedback"));
        };
        let root = required_subject_root(platform, &job.discovery_root_key, stage)?;
        if job.state_revision <= request.expected_state_revision || !root.retained_work.contains(&source.reference()?) {
            return Err(Error::config(stage, "resume receipt differs from durable progress"));
        }
        Ok(crate::MemoryProceduralReconciliationResumeReport { job, receipt, replayed: true })
    };
    if let Some(receipt) = platform.preflight_memory_mutation_operation(
        &super::super::StoreMutationOperationPreflight::new(identity.clone(), intent.clone())?,
    )? {
        return read_committed(receipt);
    }
    let before = read_job(platform, &request.job_id)?
        .ok_or_else(|| Error::not_found(stage, "reconciliation job not found"))?;
    before.validate()?;
    validate_scope(&scope, &before, stage)?;
    let ProceduralLearningWorkV1::Reconcile { source } = &before.work else {
        return Err(Error::invalid_input(
            stage,
            "capacity resume cannot change feedback",
        ));
    };
    let root = required_subject_root(platform, &before.discovery_root_key, stage)?;
    if before.state_revision != request.expected_state_revision
        || before.status != ProceduralFeedbackJobStatusV1::BlockedCapacity
        || !matches!(&root.state, bm_core::memory::ProceduralSubjectValidityStateV1::Blocked { work, reason }
            if *work == source.reference()? && *reason == bm_core::memory::ProceduralReconciliationBlockV1::Capacity)
    {
        return Err(Error::conflict(
            stage,
            "capacity resume requires the exact current blocked work",
        ));
    }
    let resumed_root = root.resume(source, MAX_PROCEDURAL_SUBJECT_SCOPES)?;
    let mut conditions = vec![
        exact(
            PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
            &before.job_id,
            &before,
            stage,
        )?,
        exact(
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
            &root.physical_key,
            &root,
            stage,
        )?,
    ];
    let pins = manifest_pins(platform, &source.scope, &[], budget, &mut conditions)?;
    let checkpoint = ProceduralReconciliationCheckpointV1::begin(source, &resumed_root, pins, now)?;
    let after = before.resume_reconciliation_capacity(checkpoint, now)?;
    let authorization = ProceduralReconciliationAuthorization {
        before_job: before.clone(),
        after_job: after.clone(),
        before_root: root,
        after_root: Some(resumed_root.clone()),
        runtime_changes: Vec::new(),
    };
    let operation = StoreMutationOperationPlan::new(
        identity.clone(),
        intent,
        MemoryMutationEffect::Changed,
        2,
        identity.actor_subject_id(),
        now,
    )?
    .authorize_procedural_reconciliation(authorization);
    let batch = StoreMutationBatch {
        transaction_id: operation.transaction_id().into(),
        operation: "post_turn.procedural.resume_capacity".into(),
        scope: scope.clone(),
        mutations: vec![
            put_json(
                PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                &after.job_id,
                encode(&after, stage)?,
            ),
            put_json(
                PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
                &resumed_root.physical_key,
                encode(&resumed_root, stage)?,
            ),
        ],
    };
    match platform.commit_memory_mutation_operation_with_runtime_budget(
        batch,
        &conditions,
        operation,
        budget,
    )? {
        StoreMutationOperationOutcome::Replayed { receipt } => read_committed(receipt),
        StoreMutationOperationOutcome::Committed { receipt, .. } => {
            Ok(crate::MemoryProceduralReconciliationResumeReport {
                job: after,
                receipt,
                replayed: false,
            })
        }
    }
}

fn manifest_pins(
    platform: &StorePlatform,
    scope: &ProceduralSubjectScopeV1,
    mutations: &[StoreMutation],
    budget: &RuntimeBudgetReport,
    conditions: &mut Vec<StoreJsonPrecondition>,
) -> Result<Pins> {
    let tool_scope = AgentToolExperienceOwningScopeV1::Subject {
        mounted_subject_id: scope.mounted_subject_id.clone(),
    };
    let skill_scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: scope.mounted_subject_id.clone(),
    };
    let tool_key = bm_core::skills::agent_tool_experience_scope_manifest_key(
        &scope.memory_space_id,
        &tool_scope,
    )?;
    let skill_key =
        bm_core::skills::runtime_skill_scope_manifest_key(&scope.memory_space_id, &skill_scope)?;
    let mut pins = Vec::new();
    for (namespace, key) in [
        (AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE, tool_key),
        (RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE, skill_key),
    ] {
        let value = post_value(platform, namespace, &key, mutations)?;
        let pin = match value.as_ref() {
            None => Pin::Absent,
            Some(value) if namespace == AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE => {
                let manifest: AgentToolExperienceScopeManifestV1 =
                    decode(value.clone(), "procedural_reconciliation_manifest")?;
                manifest.validate_exact(
                    manifest.bindings.clone(),
                    budget
                        .governed_state_budget
                        .max_agent_tool_experience_owners_per_subject,
                )?;
                if manifest.memory_space_id != scope.memory_space_id
                    || manifest.owning_scope != tool_scope
                    || manifest.physical_key != key
                {
                    return Err(Error::config(
                        "procedural_reconciliation_manifest",
                        "tool manifest crosses exact subject scope",
                    ));
                }
                Pin::Present {
                    revision: manifest.revision,
                    bindings_digest: manifest.bindings_digest,
                }
            }
            Some(value) => {
                let manifest: RuntimeSkillScopeManifest =
                    decode(value.clone(), "procedural_reconciliation_manifest")?;
                manifest.validate_exact(
                    &scope.memory_space_id,
                    &skill_scope,
                    manifest.owner_bindings.clone(),
                    budget
                        .governed_state_budget
                        .max_retained_runtime_skill_owners_per_scope,
                )?;
                Pin::Present {
                    revision: manifest.revision,
                    bindings_digest: manifest.bindings_digest,
                }
            }
        };
        conditions.push(match value {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace: namespace.into(),
                key,
                value,
            },
            None => StoreJsonPrecondition::Absent {
                namespace: namespace.into(),
                key,
            },
        });
        pins.push(pin);
    }
    Ok(Pins {
        agent_tool: pins.remove(0),
        runtime_skill: pins.remove(0),
    })
}

fn validate_output_inventory(
    platform: &StorePlatform,
    scope: &ProceduralSubjectScopeV1,
    checkpoint: &ProceduralReconciliationCheckpointV1,
    mutations: &[StoreMutation],
    budget: &RuntimeBudgetReport,
    require_complete: bool,
) -> Result<()> {
    let stage = "procedural_reconciliation_inventory";
    let tool_scope = AgentToolExperienceOwningScopeV1::Subject {
        mounted_subject_id: scope.mounted_subject_id.clone(),
    };
    let skill_scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: scope.mounted_subject_id.clone(),
    };
    let mut inventory = Vec::new();
    let tool_key = bm_core::skills::agent_tool_experience_scope_manifest_key(
        &scope.memory_space_id,
        &tool_scope,
    )?;
    if let Some(value) = post_value(
        platform,
        AGENT_TOOL_EXPERIENCE_SCOPE_MANIFEST_NAMESPACE,
        &tool_key,
        mutations,
    )? {
        let manifest: AgentToolExperienceScopeManifestV1 = decode(value, stage)?;
        for binding in manifest.bindings {
            let head: AgentToolExperienceOwnerHeadV3 = decode(
                post_value(
                    platform,
                    AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
                    &binding.head_key,
                    mutations,
                )?
                .ok_or_else(|| Error::config(stage, "tool inventory head is missing"))?,
                stage,
            )?;
            if AgentToolExperienceHeadBindingV1::from_head(&head)? != binding
                || head.retained_revisions.len()
                    > budget
                        .governed_state_budget
                        .max_agent_tool_experience_revisions_per_owner
            {
                return Err(Error::config(stage, "tool inventory head binding differs"));
            }
            for material in head.retained_revisions {
                inventory.push(ProceduralAppliedOwnerBindingV1::AgentToolExperience {
                    owner_revision: bm_core::memory::GovernedOwnerRevisionRef::try_new(
                        head.owner_ref.clone(),
                        material.owner_revision,
                    )?,
                    content_digest: material.content_digest,
                });
            }
        }
    }
    let skill_key =
        bm_core::skills::runtime_skill_scope_manifest_key(&scope.memory_space_id, &skill_scope)?;
    if let Some(value) = post_value(
        platform,
        RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
        &skill_key,
        mutations,
    )? {
        let manifest: RuntimeSkillScopeManifest = decode(value, stage)?;
        for binding in manifest.owner_bindings {
            let owner: RuntimeSkillOwnerRecord = decode(
                post_value(
                    platform,
                    RUNTIME_SKILL_RECORD_NAMESPACE,
                    &binding.owner_physical_key,
                    mutations,
                )?
                .ok_or_else(|| Error::config(stage, "runtime inventory owner is missing"))?,
                stage,
            )?;
            if RuntimeSkillOwnerBinding::from_record(&owner)? != binding {
                return Err(Error::config(stage, "runtime inventory binding differs"));
            }
            inventory.push(ProceduralAppliedOwnerBindingV1::RuntimeSkill { binding });
        }
    }
    if checkpoint
        .verified_owners
        .iter()
        .any(|proof| !inventory.contains(&proof.binding))
        || (require_complete
            && inventory.iter().any(|binding| {
                !checkpoint
                    .verified_owners
                    .iter()
                    .any(|proof| &proof.binding == binding)
            }))
    {
        return Err(Error::config(
            stage,
            "checkpoint output is not the exact retained inventory",
        ));
    }
    Ok(())
}
