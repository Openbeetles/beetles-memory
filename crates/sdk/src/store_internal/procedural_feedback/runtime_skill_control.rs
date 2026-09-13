//! Ordinary edits to feedback-derived skills invalidate their exact read proof.
//! They join the existing subject reconciliation lane; assets and provenance
//! still belong to their original owners. No operation-name exemption exists.
use super::*;
use crate::store_internal::schema::RUNTIME_SKILL_RECORD_NAMESPACE as SKILL;
use crate::store_internal::{StoreCapacityBudget, StoreMutationOperationPlan};
use bm_core::memory::{
    ProceduralAppliedOwnerBindingV1, ProceduralLearningWorkV1, ProceduralReconciliationTriggerV1,
    ProceduralReconciliationWorkV1, ProceduralRuntimeSkillTransitionV1 as Change,
};
use bm_core::skills::{
    runtime_skill_scope_manifest_key, RuntimeSkillCreationRef, RuntimeSkillOwnerBinding,
    RuntimeSkillOwnerRecord, RuntimeSkillOwningScope, RuntimeSkillScopeManifest,
};

type Json = BTreeMap<(String, String), serde_json::Value>;
const STAGE: &str = "procedural_runtime_skill_control";
fn repair() -> Error {
    Error::config(STAGE, "runtime skill change lacks exact causal closure")
}
fn parse<T: serde::de::DeserializeOwned>(value: &serde_json::Value) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|_| repair())
}
fn derived(record: &RuntimeSkillOwnerRecord) -> bool {
    matches!(
        record.creation_ref,
        RuntimeSkillCreationRef::AgentToolExperiencePromotion { .. }
    ) || !record
        .lifecycle
        .usage_outcome
        .retained_contributions
        .is_empty()
}

fn changes(before: &Json, after: &Json) -> Result<Vec<(ProceduralSubjectScopeV1, Change)>> {
    let keys = before
        .keys()
        .chain(after.keys())
        .filter(|(ns, _)| ns == SKILL)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut result = Vec::new();
    for key in keys {
        if before.get(&key) == after.get(&key) {
            continue;
        }
        let old: Option<RuntimeSkillOwnerRecord> = before.get(&key).map(parse).transpose()?;
        let new: Option<RuntimeSkillOwnerRecord> = after.get(&key).map(parse).transpose()?;
        if !old.as_ref().is_some_and(derived) && !new.as_ref().is_some_and(derived) {
            continue;
        }
        let record = old.as_ref().or(new.as_ref()).ok_or_else(repair)?;
        let RuntimeSkillOwningScope::Subject { mounted_subject_id } = &record.owning_scope else {
            return Err(repair());
        };
        if old.as_ref().zip(new.as_ref()).is_some_and(|(old, new)| {
            old.memory_space_id != new.memory_space_id || old.owning_scope != new.owning_scope
        }) {
            return Err(repair());
        }
        let scope = ProceduralSubjectScopeV1 {
            memory_space_id: record.memory_space_id.clone(),
            mounted_subject_id: mounted_subject_id.clone(),
        };
        let change = Change {
            before: old
                .as_ref()
                .map(RuntimeSkillOwnerBinding::from_record)
                .transpose()?,
            after: new
                .as_ref()
                .map(RuntimeSkillOwnerBinding::from_record)
                .transpose()?,
        };
        if !change.validates_for(&scope) {
            return Err(repair());
        }
        result.push((scope, change));
    }
    Ok(result)
}

/// Only this transaction's successful exact application can own a new binding.
/// The locked path also verifies the real MOR/audit closure; planning does not
/// authorize a write and is repeated against backend-locked actual images.
fn feedback_owns(
    before: &Json,
    after: &Json,
    change: &Change,
    transaction: &str,
    operation: Option<&MemoryMutationOperationIdentity>,
    locked: bool,
) -> Result<bool> {
    let Some(binding) = &change.after else {
        return Ok(false);
    };
    let Some(operation) = operation.filter(|operation| {
        operation.operation_kind() == MemoryMutationOperationKind::ProceduralLearning
    }) else {
        return Ok(false);
    };
    let expected = ProceduralAppliedOwnerBindingV1::RuntimeSkill {
        binding: binding.clone(),
    };
    let mut claims = 0;
    for (key, value) in after
        .iter()
        .filter(|((ns, _), _)| ns == PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE)
    {
        if before.contains_key(key) {
            continue;
        }
        let ledger: ProceduralFeedbackApplicationLedgerV2 = parse(value)?;
        if !ledger.applied_owner_bindings.contains(&expected) {
            continue;
        }
        ledger.validate()?;
        let address = (
            PROCEDURAL_FEEDBACK_JOB_NAMESPACE.to_owned(),
            ledger.job_id.clone(),
        );
        let old: ProceduralFeedbackJobV2 = parse(before.get(&address).ok_or_else(repair)?)?;
        let new: ProceduralFeedbackJobV2 = parse(after.get(&address).ok_or_else(repair)?)?;
        old.validate()?;
        new.validate()?;
        let receipt = new.receipt.as_ref().ok_or_else(repair)?.feedback()?;
        if old.status != ProceduralFeedbackJobStatusV1::Leased
            || new.status != ProceduralFeedbackJobStatusV1::Succeeded
            || old.work != new.work
            || old.lease_epoch != new.lease_epoch
            || old.state_revision.checked_add(1) != Some(new.state_revision)
            || old
                .lease_until
                .is_none_or(|until| until <= receipt.completed_at)
            || receipt.transaction_id != transaction
            || receipt.mutation_receipt_key != operation.storage_key()
            || ledger.identity != new.feedback_source()?.identity
        {
            return Err(repair());
        }
        if locked {
            let key = receipt.mutation_receipt_key.clone();
            let receipt: MemoryMutationReceipt = parse(
                after
                    .get(&(
                        bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE.into(),
                        key.clone(),
                    ))
                    .ok_or_else(repair)?,
            )?;
            let audit: MemoryMutationAuditRecord = parse(
                after
                    .get(&(bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE.into(), key))
                    .ok_or_else(repair)?,
            )?;
            validate_feedback_application_completion(&new, &ledger, &receipt, &audit)?;
        }
        claims += 1;
    }
    if claims > 1 {
        return Err(repair());
    }
    Ok(claims == 1)
}

fn uncontrolled(
    before: &Json,
    after: &Json,
    transaction: &str,
    operation: Option<&MemoryMutationOperationIdentity>,
    authority: Option<&ProceduralMutationAuthorization>,
    locked: bool,
) -> Result<BTreeMap<(String, String), Vec<Change>>> {
    let mut groups = BTreeMap::<_, Vec<Change>>::new();
    for (scope, change) in changes(before, after)? {
        let feedback = feedback_owns(before, after, &change, transaction, operation, locked)?;
        let reconciliation =
            authority.is_some_and(|authority| authority.owns_runtime_change(&change, before));
        if feedback && reconciliation {
            return Err(repair());
        }
        if !feedback && !reconciliation {
            groups
                .entry((scope.memory_space_id, scope.mounted_subject_id))
                .or_default()
                .push(change);
        }
    }
    for group in groups.values_mut() {
        group.sort_by(|a, b| a.owner_ref().cmp(&b.owner_ref()));
    }
    Ok(groups)
}

fn advance_work(
    before: &Json,
    after: &Json,
    scope: &ProceduralSubjectScopeV1,
) -> Result<Option<ProceduralReconciliationWorkV1>> {
    let address = (
        PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE.into(),
        scope.root_key()?,
    );
    let old: ProceduralSubjectValidityRootV1 = parse(before.get(&address).ok_or_else(repair)?)?;
    let next: ProceduralSubjectValidityRootV1 = parse(after.get(&address).ok_or_else(repair)?)?;
    if old == next {
        return Ok(None);
    }
    if old.validity_epoch.checked_add(1) != Some(next.validity_epoch) {
        return Err(repair());
    }
    let reference = next.retained_work.last().ok_or_else(repair)?;
    let address = (
        PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(),
        reference.job_id.clone(),
    );
    let job: ProceduralFeedbackJobV2 = parse(after.get(&address).ok_or_else(repair)?)?;
    let ProceduralLearningWorkV1::Reconcile { source } = &job.work else {
        return Err(repair());
    };
    if source.scope != *scope
        || next != old.begin(source, MAX_PROCEDURAL_SUBJECT_SCOPES)?
        || before.contains_key(&address)
        || job != ProceduralFeedbackJobV2::pending_work(job.work.clone(), 5, job.created_at)?
    {
        return Err(repair());
    }
    Ok(Some(source.clone()))
}

pub(crate) fn append(
    platform: &StorePlatform,
    batch: &mut StoreMutationBatch,
    conditions: &mut Vec<StoreJsonPrecondition>,
    operation: Option<&StoreMutationOperationPlan>,
    now: u64,
    capacity: StoreCapacityBudget,
) -> Result<()> {
    if !batch.mutations.iter().any(|mutation| {
        matches!(mutation, StoreMutation::PutJson { namespace, .. }
        | StoreMutation::DeleteJson { namespace, .. } if namespace == SKILL)
    }) {
        return Ok(());
    }
    let expected = conditions.clone();
    let mut reader =
        super::source_dependents::ReadPlan::for_conditions(platform, &expected, capacity);
    let mut before = Json::new();
    let mut after = Json::new();
    let owning_scope = match &batch.scope.physical_owning_scope {
        crate::StorePhysicalOwningScope::Subject { mounted_subject_id }
            if mounted_subject_id == &batch.scope.subject_id =>
        {
            RuntimeSkillOwningScope::Subject {
                mounted_subject_id: mounted_subject_id.clone(),
            }
        }
        crate::StorePhysicalOwningScope::SharedProgram => RuntimeSkillOwningScope::SharedProgram,
        _ => return Err(repair()),
    };
    let manifest_key =
        runtime_skill_scope_manifest_key(&batch.scope.memory_space_id, &owning_scope)?;
    let manifest: Option<RuntimeSkillScopeManifest> = reader
        .before(
            crate::store_internal::schema::RUNTIME_SKILL_SCOPE_MANIFEST_NAMESPACE,
            &manifest_key,
        )?
        .as_ref()
        .map(parse)
        .transpose()?;
    if let Some(manifest) = &manifest {
        manifest.validate_exact(
            &batch.scope.memory_space_id,
            &owning_scope,
            manifest.owner_bindings.clone(),
            capacity.kv_max_entries,
        )?;
    }
    // A mutation address is not read authority. Existing bodies come only from
    // the actual scope manifest; a creation key is derived from a canonical
    // scoped proposed owner and must really be absent.
    for mutation in &batch.mutations {
        let (namespace, key, value) = match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                value,
                ..
            } => (namespace, key, Some(value.clone())),
            StoreMutation::DeleteJson { namespace, key, .. } => (namespace, key, None),
            _ => continue,
        };
        if namespace != SKILL {
            continue;
        }
        let proposed: Option<RuntimeSkillOwnerRecord> = value.as_ref().map(parse).transpose()?;
        if let Some(record) = &proposed {
            let binding = RuntimeSkillOwnerBinding::from_record(record)?;
            if record.memory_space_id != batch.scope.memory_space_id
                || record.owning_scope != owning_scope
                || binding.owner_physical_key != *key
            {
                return Err(repair());
            }
        }
        let expected = manifest.as_ref().and_then(|manifest| {
            manifest
                .owner_bindings
                .iter()
                .find(|binding| binding.owner_physical_key == *key)
        });
        if expected.is_none()
            && proposed
                .as_ref()
                .is_none_or(|record| record.owner_revision != 1)
        {
            return Err(repair());
        }
        let address = (namespace.clone(), key.clone());
        let old = reader.before(namespace, key)?;
        let actual = old
            .as_ref()
            .map(parse::<RuntimeSkillOwnerRecord>)
            .transpose()?
            .as_ref()
            .map(RuntimeSkillOwnerBinding::from_record)
            .transpose()?;
        if actual.as_ref() != expected {
            return Err(repair());
        }
        if let Some(old) = old {
            before.insert(address.clone(), old);
        }
        if let Some(value) = value {
            after.insert(address, value);
        }
    }
    let changed = changes(&before, &after)?;
    let proposed_value = |namespace: &str, key: &str| -> Result<Option<serde_json::Value>> {
        let values = batch
            .mutations
            .iter()
            .filter_map(|mutation| match mutation {
                StoreMutation::PutJson {
                    namespace: ns,
                    key: k,
                    value,
                    ..
                } if ns == namespace && k == key => Some(value.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if values.len() > 1 {
            return Err(repair());
        }
        Ok(values.into_iter().next())
    };
    // Only an exact new application to one of these changed owners needs its
    // ledger/job preimage. Unrelated caller-supplied rows grant no reads.
    for mutation in &batch.mutations {
        let StoreMutation::PutJson {
            namespace,
            key,
            value,
            ..
        } = mutation
        else {
            continue;
        };
        if namespace != PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE {
            continue;
        }
        let ledger: ProceduralFeedbackApplicationLedgerV2 = parse(value)?;
        if !changed.iter().any(|(_, change)| {
            change.after.as_ref().is_some_and(|binding| {
                ledger.applied_owner_bindings.contains(
                    &ProceduralAppliedOwnerBindingV1::RuntimeSkill {
                        binding: binding.clone(),
                    },
                )
            })
        }) {
            continue;
        }
        ledger.validate()?;
        if ledger.job_id != *key
            || ledger.identity.memory_space_id != batch.scope.memory_space_id
            || ledger.identity.mounted_subject_id != batch.scope.subject_id
        {
            return Err(repair());
        }
        let job_value = proposed_value(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &ledger.job_id)?
            .ok_or_else(repair)?;
        let job: ProceduralFeedbackJobV2 = parse(&job_value)?;
        job.validate()?;
        validate_scope(&batch.scope, &job, STAGE)?;
        if job.job_id != ledger.job_id || job.feedback_source()?.identity != ledger.identity {
            return Err(repair());
        }
        for (namespace, key, value) in [
            (
                PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
                ledger.job_id.clone(),
                value.clone(),
            ),
            (
                PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
                ledger.job_id.clone(),
                job_value,
            ),
        ] {
            let address = (namespace.into(), key.clone());
            if let Some(old) = reader.before(namespace, &key)? {
                before.insert(address.clone(), old);
            }
            after.insert(address, value);
        }
    }
    if !changed.is_empty() {
        if let Some(authority) =
            operation.and_then(StoreMutationOperationPlan::procedural_mutation_authority)
        {
            for (address, expected) in authority.runtime_preimages()? {
                let actual = reader.before(&address.0, &address.1)?.ok_or_else(repair)?;
                if actual != expected {
                    return Err(repair());
                }
                before.insert(address, actual);
            }
        }
    }
    // Root reads are derived from actual typed skill ownership, not caller IDs.
    for (scope, _) in changed {
        if scope.memory_space_id != batch.scope.memory_space_id
            || scope.mounted_subject_id != batch.scope.subject_id
        {
            return Err(repair());
        }
        let address = (
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE.to_owned(),
            scope.root_key()?,
        );
        let value = reader.before(&address.0, &address.1)?.ok_or_else(repair)?;
        before.insert(address.clone(), value.clone());
        let proposed = proposed_value(&address.0, &address.1)?.unwrap_or(value);
        let root: ProceduralSubjectValidityRootV1 = parse(&proposed)?;
        if root.scope != scope || root.physical_key != address.1 {
            return Err(repair());
        }
        if before.get(&address) != Some(&proposed) {
            let old: ProceduralSubjectValidityRootV1 =
                parse(before.get(&address).ok_or_else(repair)?)?;
            if old.validity_epoch != root.validity_epoch {
                let reference = root.retained_work.last().ok_or_else(repair)?;
                let job_value =
                    proposed_value(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &reference.job_id)?
                        .ok_or_else(repair)?;
                let job: ProceduralFeedbackJobV2 = parse(&job_value)?;
                job.validate()?;
                validate_scope(&batch.scope, &job, STAGE)?;
                if job.job_id != reference.job_id {
                    return Err(repair());
                }
                let job_address = (PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(), job.job_id.clone());
                if let Some(old) = reader.before(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &job.job_id)? {
                    before.insert(job_address.clone(), old);
                }
                after.insert(job_address, job_value);
            }
        }
        after.insert(address, proposed);
    }
    let groups = uncontrolled(
        &before,
        &after,
        &batch.transaction_id,
        operation.map(StoreMutationOperationPlan::identity),
        operation.and_then(StoreMutationOperationPlan::procedural_mutation_authority),
        false,
    )?;
    let mut mutations = Vec::new();
    for ((memory_space_id, mounted_subject_id), changes) in groups {
        let scope = ProceduralSubjectScopeV1 {
            memory_space_id,
            mounted_subject_id,
        };
        // A source/Transcript owner may already have created one atomic epoch
        // for this whole subject. Its separate locked validator must prove it.
        if advance_work(&before, &after, &scope)?.is_some() {
            continue;
        }
        let key = scope.root_key()?;
        let old: ProceduralSubjectValidityRootV1 = parse(
            before
                .get(&(PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE.into(), key.clone()))
                .ok_or_else(repair)?,
        )?;
        let trigger = ProceduralReconciliationTriggerV1::RuntimeSkillControl {
            transaction_id: batch.transaction_id.clone(),
            operation: operation.map(|operation| operation.identity().clone()),
            changes,
        };
        let (root, jobs, dependencies) = plan_subject_reconciliation(platform, &old, trigger, now)?;
        mutations.push(put_json(
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
            &key,
            encode(&root, STAGE)?,
        ));
        mutations.extend(jobs);
        super::source_dependents::merge_conditions(conditions, dependencies)?;
    }
    super::source_dependents::merge_conditions(
        conditions,
        reader
            .observed
            .into_iter()
            .map(|((namespace, key), value)| match value {
                Some(value) => StoreJsonPrecondition::Exact {
                    namespace,
                    key,
                    value,
                },
                None => StoreJsonPrecondition::Absent { namespace, key },
            }),
    )?;
    batch.mutations.extend(mutations);
    Ok(())
}

pub(crate) fn validate_transition(
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    transaction: &str,
    conditions: &[StoreJsonPrecondition],
    authority: Option<&ProceduralMutationAuthorization>,
) -> Result<BTreeSet<String>> {
    let operations = after
        .json
        .iter()
        .filter(|((namespace, key), value)| {
            namespace == bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE
                && before.json.get(&(namespace.clone(), key.clone())) != Some(*value)
        })
        .map(|(_, value)| parse::<MemoryMutationReceipt>(value))
        .collect::<Result<Vec<_>>>()?;
    let operations = operations
        .iter()
        .filter(|receipt| receipt.transaction_id == transaction)
        .collect::<Vec<_>>();
    if operations.len() > 1 {
        return Err(repair());
    }
    let operation = operations.first().map(|receipt| &receipt.identity);
    let groups = uncontrolled(
        &before.json,
        &after.json,
        transaction,
        operation,
        authority,
        true,
    )?;
    let mut fences = BTreeSet::new();
    for ((memory_space_id, mounted_subject_id), changes) in groups {
        let scope = ProceduralSubjectScopeV1 {
            memory_space_id,
            mounted_subject_id,
        };
        let work = advance_work(&before.json, &after.json, &scope)?.ok_or_else(repair)?;
        let key = scope.root_key()?;
        if !conditions.iter().any(|condition| {
            matches!(condition, StoreJsonPrecondition::Exact { namespace, key: k, value }
            if namespace == PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE && k == &key
                && Some(value) == before.json.get(&(namespace.clone(), k.clone())))
        }) {
            return Err(repair());
        }
        if let ProceduralReconciliationTriggerV1::RuntimeSkillControl {
            transaction_id,
            operation: expected_operation,
            changes: expected_changes,
        } = &work.trigger
        {
            if transaction_id != transaction
                || expected_operation.as_ref() != operation
                || expected_changes != &changes
            {
                return Err(repair());
            }
            fences.insert(key);
        }
    }
    Ok(fences)
}
