//! Transcript withdrawal invalidates the subject's single learning lane, including
//! semantic method sources which never created a procedural feedback ledger.
use super::*;
use crate::store_internal::{StoreCapacityBudget, StoreMutationOperationPlan};
use bm_core::memory::{
    ProceduralLearningWorkV1, ProceduralReconciliationTriggerV1 as Trigger,
    ProceduralReconciliationWorkV1, TranscriptLifecycleState, TranscriptLifecycleTransition,
    TranscriptTurnRecord,
};
type Json = BTreeMap<(String, String), serde_json::Value>;
const STAGE: &str = "procedural_transcript_control";
fn repair() -> Error {
    Error::config(
        STAGE,
        "Transcript withdrawal lacks its exact learning invalidation",
    )
}
fn parse<T: serde::de::DeserializeOwned>(value: &serde_json::Value) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|_| repair())
}
fn changed(
    before: &Json,
    after: &Json,
) -> Result<BTreeMap<(String, String), (TranscriptTurnRecord, TranscriptTurnRecord)>> {
    let mut groups = BTreeMap::new();
    for ((namespace, key), value) in before {
        if namespace != "conversation_transcript"
            || after.get(&(namespace.clone(), key.clone())) == Some(value)
        {
            continue;
        }
        let old: TranscriptTurnRecord = parse(value)?;
        if !old.permits_post_turn_learning()
            || old
                .learning_evidence
                .as_ref()
                .and_then(|evidence| evidence.authority.producer_revision())
                .is_none()
        {
            continue;
        }
        let new: TranscriptTurnRecord = parse(
            after
                .get(&(namespace.clone(), key.clone()))
                .ok_or_else(repair)?,
        )?;
        if old.key != new.key
            || old.subject != new.subject
            || old.turn_id != new.turn_id
            || old.sequence != new.sequence
            || super::super::transcript_turn_storage_key(&old.key, &old.subject, &old.turn_id)
                != *key
        {
            return Err(repair());
        }
        if new.permits_post_turn_learning() {
            continue;
        }
        if !matches!(
            new.lifecycle_state,
            TranscriptLifecycleState::Masked | TranscriptLifecycleState::RawDeleted
        ) {
            return Err(repair());
        }
        // Canonical key ordering picks one causal witness. The epoch reconciles
        // every owner of this subject, so it covers all withdrawn turns in batch.
        groups
            .entry((old.key.memory_space_id.clone(), old.subject.clone()))
            .or_insert((old, new));
    }
    Ok(groups)
}
fn trigger(
    old: &TranscriptTurnRecord,
    new: &TranscriptTurnRecord,
    transaction: &str,
    operation: &MemoryMutationOperationIdentity,
) -> Result<Trigger> {
    if operation.operation_kind() != MemoryMutationOperationKind::ProceduralLifecycle
        || operation.memory_space_id() != old.key.memory_space_id
        || operation.mounted_subject_id() != old.subject
    {
        return Err(repair());
    }
    Ok(Trigger::TranscriptLifecycle {
        transaction_id: transaction.into(),
        operation: operation.clone(),
        key: old.key.clone(),
        turn_id: old.turn_id.clone(),
        sequence: old.sequence,
        transition: if new.lifecycle_state == TranscriptLifecycleState::Masked {
            TranscriptLifecycleTransition::Mask
        } else {
            TranscriptLifecycleTransition::DeleteRaw
        },
        before_state: old.lifecycle_state,
        after_state: new.lifecycle_state,
        before_content_digest: bm_core::memory::procedural_transcript_state_digest(old)?,
        after_content_digest: bm_core::memory::procedural_transcript_state_digest(new)?,
    })
}
fn advanced(
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
    let reference = next.retained_work.last().ok_or_else(repair)?;
    let job_address = (
        PROCEDURAL_FEEDBACK_JOB_NAMESPACE.into(),
        reference.job_id.clone(),
    );
    let job: ProceduralFeedbackJobV2 = parse(after.get(&job_address).ok_or_else(repair)?)?;
    let ProceduralLearningWorkV1::Reconcile { source } = &job.work else {
        return Err(repair());
    };
    if source.scope != *scope
        || next != old.begin(source, MAX_PROCEDURAL_SUBJECT_SCOPES)?
        || before.contains_key(&job_address)
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
    if !batch.mutations.iter().any(|m| matches!(m, StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. } if namespace == "conversation_transcript")) { return Ok(()); }
    let expected = conditions.clone();
    let mut reader = source_dependents::ReadPlan::for_conditions(platform, &expected, capacity);
    let mut before = Json::new();
    let mut after = Json::new();
    for mutation in &batch.mutations {
        if let StoreMutation::PutJson {
            namespace,
            key,
            value,
            ..
        } = mutation
        {
            after.insert((namespace.clone(), key.clone()), value.clone());
            if namespace == "conversation_transcript" {
                // Existing lifecycle reads already carry exact ownership/CAS.
                // A new transcript has no preimage and cannot withdraw a source.
                let proposed: TranscriptTurnRecord = parse(value)?;
                if proposed.key.memory_space_id != batch.scope.memory_space_id
                    || proposed.subject != batch.scope.subject_id
                    || super::super::transcript_turn_storage_key(
                        &proposed.key,
                        &proposed.subject,
                        &proposed.turn_id,
                    ) != *key
                {
                    return Err(repair());
                }
                if let Some(old) = reader.before(namespace, key)? {
                    before.insert((namespace.clone(), key.clone()), old);
                }
            }
        }
    }
    let groups = changed(&before, &after)?;
    let mut additions = Vec::new();
    for ((memory_space_id, mounted_subject_id), (old_turn, new_turn)) in groups {
        let scope = ProceduralSubjectScopeV1 {
            memory_space_id,
            mounted_subject_id,
        };
        let key = scope.root_key()?;
        let old = reader
            .before(PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE, &key)?
            .ok_or_else(repair)?;
        let root: ProceduralSubjectValidityRootV1 = parse(&old)?;
        let address = (PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE.into(), key.clone());
        before.insert(address.clone(), old.clone());
        after.entry(address).or_insert(old);
        let causal = trigger(
            &old_turn,
            &new_turn,
            &batch.transaction_id,
            operation.ok_or_else(repair)?.identity(),
        )?;
        if let Some(work) = advanced(&before, &after, &scope)? {
            // SourceChange is separately proved under lock; neither arbitrary
            // work nor a runtime edit can substitute for Transcript authority.
            if !matches!(work.trigger, Trigger::SourceChange { .. }) && work.trigger != causal {
                return Err(repair());
            }
            continue;
        }
        let (next, jobs, dependencies) = plan_subject_reconciliation(platform, &root, causal, now)?;
        additions.push(put_json(
            PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE,
            &key,
            encode(&next, STAGE)?,
        ));
        additions.extend(jobs);
        super::super::transaction::merge_json_preconditions(conditions, dependencies)?;
    }
    super::super::transaction::merge_json_preconditions(
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
    batch.mutations.extend(additions);
    Ok(())
}

pub(crate) fn validate_transition(
    before: &BackendTransactionState,
    after: &BackendTransactionState,
    transaction: &str,
    conditions: &[StoreJsonPrecondition],
) -> Result<BTreeSet<String>> {
    let groups = changed(&before.json, &after.json)?;
    if groups.is_empty() {
        return Ok(BTreeSet::new());
    }
    let receipts = after
        .json
        .iter()
        .filter(|((ns, key), value)| {
            ns == bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE
                && before.json.get(&(ns.clone(), key.clone())) != Some(*value)
        })
        .map(|(_, value)| parse::<MemoryMutationReceipt>(value))
        .collect::<Result<Vec<_>>>()?;
    let receipts = receipts
        .iter()
        .filter(|receipt| receipt.transaction_id == transaction)
        .collect::<Vec<_>>();
    if receipts.len() != 1 {
        return Err(repair());
    }
    let mut fences = BTreeSet::new();
    for ((memory_space_id, mounted_subject_id), (old, new)) in groups {
        let scope = ProceduralSubjectScopeV1 {
            memory_space_id,
            mounted_subject_id,
        };
        let causal = trigger(&old, &new, transaction, &receipts[0].identity)?;
        let transcript_key =
            super::super::transcript_turn_storage_key(&old.key, &old.subject, &old.turn_id);
        if !conditions.iter().any(|condition| matches!(condition, StoreJsonPrecondition::Exact { namespace,key,value }
            if namespace == "conversation_transcript" && key == &transcript_key && Some(value) == before.json.get(&(namespace.clone(),key.clone())))) { return Err(repair()); }
        let work = advanced(&before.json, &after.json, &scope)?.ok_or_else(repair)?;
        let key = scope.root_key()?;
        if !conditions.iter().any(|condition| matches!(condition, StoreJsonPrecondition::Exact { namespace,key:k,value }
            if namespace == PROCEDURAL_SUBJECT_VALIDITY_NAMESPACE && k == &key && Some(value) == before.json.get(&(namespace.clone(),k.clone())))) { return Err(repair()); }
        if work.trigger == causal {
            fences.insert(key);
        } else if !matches!(work.trigger, Trigger::SourceChange { .. }) {
            return Err(repair());
        }
    }
    Ok(fences)
}
