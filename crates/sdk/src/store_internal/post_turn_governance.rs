use bm_core::memory::{
    MemoryMutationEffect, MemoryMutationOperationIdentity, MemoryMutationOperationKind,
    MemoryMutationReceipt, PostTurnGovernanceAttemptAuthorityV3,
    PostTurnGovernanceBindingRevisionIndexV1, PostTurnGovernanceBindingRevisionRefV1,
    PostTurnGovernanceBindingSnapshotV1, PostTurnGovernanceDecisionSummaryV1,
    PostTurnGovernanceErrorClassV2, PostTurnGovernanceExecutionBindingV1,
    PostTurnGovernanceExecutionBlockAuthorityV1, PostTurnGovernanceJobRefV2,
    PostTurnGovernanceJobStatusV2, PostTurnGovernanceJobV3, PostTurnGovernanceReceiptV3,
    PostTurnGovernanceReconciliationCursorV1, PostTurnGovernanceScopeIndexV3,
    MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS, MAX_POST_TURN_GOVERNANCE_BINDING_REVISIONS,
    MAX_POST_TURN_GOVERNANCE_RECENT_TERMINAL_JOBS, MEMORY_MUTATION_RECEIPT_NAMESPACE,
    POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_NAMESPACE,
    POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE, POST_TURN_GOVERNANCE_JOB_NAMESPACE,
    POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
};
use bm_core::{Error, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use crate::{
    MemoryStoreEventKind, RuntimeBudgetReport, StoreBlobPrecondition, StoreEventScope,
    StoreJsonPrecondition, StoreMutation, StoreMutationBatch,
};

use super::schema::{
    is_relationship_source_protected_json_namespace, is_subject_soul_protected_json_namespace,
};
use super::subject_soul::{
    SubjectSoulStoreFailure, SubjectSoulStoreMutationOutcome, SubjectSoulStoreMutationPlan,
};
use super::{StoreMutationOperationOutcome, StoreMutationOperationPlan, StorePlatform};

const GOVERNANCE_RETRY_BASE_SECS: u64 = 5;
const GOVERNANCE_RETRY_MAX_SECS: u64 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GovernanceIntentEnsureOutcome {
    Created,
    AlreadyPresent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GovernanceRecoveryOutcome {
    pub(crate) job: PostTurnGovernanceJobV3,
    pub(crate) replayed: bool,
}

pub(crate) fn governance_recovery_operation_was_committed(
    platform: &StorePlatform,
    job: &PostTurnGovernanceJobV3,
    caller_operation_id: &str,
    actor_subject_id: &str,
    operation_kind: MemoryMutationOperationKind,
    recovery_authority_key: &str,
    new_generation: u64,
) -> Result<bool> {
    let identity = governance_recovery_operation_identity(
        job,
        caller_operation_id,
        actor_subject_id,
        operation_kind,
    )?;
    let mut docs = platform
        .read_json_docs_by_keys(MEMORY_MUTATION_RECEIPT_NAMESPACE, &[identity.storage_key()])?;
    let Some(doc) = docs.pop() else {
        return Ok(false);
    };
    let receipt = serde_json::from_value::<MemoryMutationReceipt>(doc.value).map_err(|error| {
        Error::config("post_turn_governance_recovery_operation", error.to_string())
    })?;
    receipt.classify_replay(
        &identity,
        &governance_recovery_intent_digest(
            &job.job_id,
            operation_kind,
            recovery_authority_key,
            new_generation,
        )?,
    )?;
    Ok(true)
}

pub(crate) fn ensure_governance_intent(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job: &PostTurnGovernanceJobV3,
    now_secs: u64,
) -> Result<GovernanceIntentEnsureOutcome> {
    job.validate()?;
    let existing = read_job(platform, &job.job_id)?;
    if let Some(existing) = existing {
        if existing == *job {
            return Ok(GovernanceIntentEnsureOutcome::AlreadyPresent);
        }
        if existing.identity == job.identity
            && existing.transcript_sequence == job.transcript_sequence
            && existing.transcript_digest == job.transcript_digest
        {
            return Ok(GovernanceIntentEnsureOutcome::AlreadyPresent);
        }
        return Err(Error::conflict(
            "post_turn_governance_intent",
            "deterministic job identity has divergent transcript authority",
        ));
    }

    let before_index = read_scope_index(platform, &job.scope_index_key)?;
    let mut after_index = before_index
        .clone()
        .unwrap_or_else(|| PostTurnGovernanceScopeIndexV3::empty(&job.identity, now_secs));
    if after_index.active_jobs.len() >= MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS {
        return Err(Error::config(
            "post_turn_governance_intent",
            "exact governance scope active-job budget is exhausted",
        ));
    }
    if after_index.recent_terminal_jobs.len() >= MAX_POST_TURN_GOVERNANCE_RECENT_TERMINAL_JOBS {
        return Err(Error::config(
            "post_turn_governance_intent",
            "exact governance scope terminal-receipt retention is exhausted",
        ));
    }
    after_index
        .active_jobs
        .push(PostTurnGovernanceJobRefV2::from_job(job));
    after_index.active_jobs.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.job_id.cmp(&right.job_id))
    });
    if before_index.is_some() {
        after_index.index_revision =
            after_index.index_revision.checked_add(1).ok_or_else(|| {
                Error::config(
                    "post_turn_governance_intent",
                    "governance scope index revision overflow",
                )
            })?;
    }
    after_index.updated_at = now_secs;
    after_index.validate()?;

    let job_value = serde_json::to_value(job)
        .map_err(|error| Error::config("post_turn_governance_intent", error.to_string()))?;
    let index_value = serde_json::to_value(&after_index)
        .map_err(|error| Error::config("post_turn_governance_intent", error.to_string()))?;
    let mut preconditions = vec![
        StoreJsonPrecondition::Absent {
            namespace: POST_TURN_GOVERNANCE_JOB_NAMESPACE.to_string(),
            key: job.job_id.clone(),
        },
        json_precondition(
            POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
            &job.scope_index_key,
            before_index.as_ref(),
        )?,
    ];
    let mut mutations = vec![
        put_json(POST_TURN_GOVERNANCE_JOB_NAMESPACE, &job.job_id, job_value),
        put_json(
            POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
            &job.scope_index_key,
            index_value,
        ),
    ];
    append_binding_reference_mutation(
        platform,
        &job.execution_binding,
        now_secs,
        &mut mutations,
        &mut preconditions,
    )?;
    let batch = StoreMutationBatch {
        transaction_id: format!("post_turn_governance_enqueue_{}", job.job_id),
        operation: "post_turn.governance.enqueue".to_string(),
        scope,
        mutations,
    };
    let commit = platform.commit_governed_memory_transaction_with_runtime_budget_at(
        batch,
        &preconditions,
        runtime_budget,
        now_secs,
    );
    if let Err(error) = commit {
        let concurrent_job = read_job(platform, &job.job_id)?;
        let concurrent_index = read_scope_index(platform, &job.scope_index_key)?;
        let same_intent = concurrent_job.as_ref().is_some_and(|existing| {
            existing.identity == job.identity
                && existing.transcript_sequence == job.transcript_sequence
                && existing.transcript_digest == job.transcript_digest
        });
        let indexed_exactly = concurrent_job.as_ref().is_some_and(|existing| {
            concurrent_index.as_ref().is_some_and(|index| {
                index.active_jobs.iter().any(|reference| {
                    reference.job_id == existing.job_id
                        && reference.status == existing.status
                        && reference.state_revision == existing.state_revision
                })
            })
        });
        if same_intent && indexed_exactly {
            return Ok(GovernanceIntentEnsureOutcome::AlreadyPresent);
        }
        return Err(error);
    }
    Ok(GovernanceIntentEnsureOutcome::Created)
}

pub(crate) fn reconcile_governance_intents(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    jobs: &[PostTurnGovernanceJobV3],
    now_secs: u64,
) -> Result<usize> {
    let first = jobs.first().ok_or_else(|| {
        Error::invalid_input(
            "post_turn_governance_reconcile",
            "a non-empty bounded transcript page is required",
        )
    })?;
    let last = jobs.last().expect("non-empty reconciliation page");
    let identity = &first.identity;
    let cursor_sequence = last.transcript_sequence;
    let cursor_turn_id = &last.identity.turn_id;
    let scope_index_key = identity.scope_id();
    let before_index = read_scope_index(platform, &scope_index_key)?;
    let mut after_index = before_index
        .clone()
        .unwrap_or_else(|| PostTurnGovernanceScopeIndexV3::empty(identity, now_secs));
    if after_index.recent_terminal_jobs.len() >= MAX_POST_TURN_GOVERNANCE_RECENT_TERMINAL_JOBS {
        return Err(Error::config(
            "post_turn_governance_reconcile",
            "exact governance scope terminal-receipt retention is exhausted",
        ));
    }
    if after_index
        .reconciliation_cursor(&identity.conversation_id)
        .is_some_and(|cursor| cursor_sequence <= cursor.sequence)
    {
        return Err(Error::conflict(
            "post_turn_governance_reconcile",
            "reconciliation cursor must advance monotonically",
        ));
    }

    let mut preconditions = vec![json_precondition(
        POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
        &scope_index_key,
        before_index.as_ref(),
    )?];
    let mut mutations = Vec::new();
    let mut referenced_bindings = BTreeSet::new();
    let mut created = 0usize;
    for job in jobs {
        job.validate()?;
        if job.scope_index_key != scope_index_key
            || job.identity.memory_space_id != identity.memory_space_id
            || job.identity.mounted_subject_id != identity.mounted_subject_id
            || job.identity.channel_id != identity.channel_id
            || job.identity.chat_id != identity.chat_id
            || job.identity.conversation_id != identity.conversation_id
        {
            return Err(Error::invalid_input(
                "post_turn_governance_reconcile",
                "reconciliation page crosses its exact governance scope",
            ));
        }
        let existing = read_job(platform, &job.job_id)?;
        match existing.as_ref() {
            Some(existing)
                if existing.identity == job.identity
                    && existing.transcript_sequence == job.transcript_sequence
                    && existing.transcript_digest == job.transcript_digest =>
            {
                preconditions.push(json_precondition(
                    POST_TURN_GOVERNANCE_JOB_NAMESPACE,
                    &job.job_id,
                    Some(existing),
                )?);
                if existing.status.is_active() {
                    match after_index
                        .active_jobs
                        .iter()
                        .find(|reference| reference.job_id == existing.job_id)
                    {
                        Some(reference)
                            if reference.state_revision == existing.state_revision
                                && reference.status == existing.status => {}
                        Some(_) => {
                            return Err(Error::conflict(
                                "post_turn_governance_reconcile",
                                "scope index ref differs from governance job authority",
                            ));
                        }
                        None => after_index
                            .active_jobs
                            .push(PostTurnGovernanceJobRefV2::from_job(existing)),
                    }
                }
            }
            Some(_) => {
                return Err(Error::conflict(
                    "post_turn_governance_reconcile",
                    "deterministic job identity has divergent transcript authority",
                ));
            }
            None => {
                preconditions.push(StoreJsonPrecondition::Absent {
                    namespace: POST_TURN_GOVERNANCE_JOB_NAMESPACE.to_string(),
                    key: job.job_id.clone(),
                });
                mutations.push(put_json(
                    POST_TURN_GOVERNANCE_JOB_NAMESPACE,
                    &job.job_id,
                    serde_json::to_value(job).map_err(|error| {
                        Error::config("post_turn_governance_reconcile", error.to_string())
                    })?,
                ));
                after_index
                    .active_jobs
                    .push(PostTurnGovernanceJobRefV2::from_job(job));
                if let PostTurnGovernanceExecutionBindingV1::Bound {
                    binding_id,
                    binding_revision,
                } = &job.execution_binding
                {
                    referenced_bindings.insert((binding_id.clone(), *binding_revision));
                }
                created = created.saturating_add(1);
            }
        }
        if after_index.active_jobs.len() > MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS {
            return Err(Error::config(
                "post_turn_governance_reconcile",
                "exact governance scope active-job budget is exhausted",
            ));
        }
    }

    after_index.active_jobs.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.job_id.cmp(&right.job_id))
    });
    after_index.set_reconciliation_cursor(PostTurnGovernanceReconciliationCursorV1 {
        conversation_id: identity.conversation_id.clone(),
        sequence: cursor_sequence,
        turn_id: cursor_turn_id.clone(),
    })?;
    if before_index.is_some() {
        after_index.index_revision =
            after_index.index_revision.checked_add(1).ok_or_else(|| {
                Error::config(
                    "post_turn_governance_reconcile",
                    "governance scope index revision overflow",
                )
            })?;
    }
    after_index.updated_at = now_secs;
    after_index.validate()?;
    for (binding_id, binding_revision) in referenced_bindings {
        let binding = PostTurnGovernanceExecutionBindingV1::Bound {
            binding_id,
            binding_revision,
        };
        append_binding_reference_mutation(
            platform,
            &binding,
            now_secs,
            &mut mutations,
            &mut preconditions,
        )?;
    }
    mutations.push(put_json(
        POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
        &scope_index_key,
        serde_json::to_value(&after_index)
            .map_err(|error| Error::config("post_turn_governance_reconcile", error.to_string()))?,
    ));
    platform.commit_governed_memory_transaction_with_runtime_budget_at(
        StoreMutationBatch {
            transaction_id: format!(
                "post_turn_governance_reconcile_{}_{}",
                scope_index_key, cursor_sequence
            ),
            operation: "post_turn.governance.reconcile".to_string(),
            scope,
            mutations,
        },
        &preconditions,
        runtime_budget,
        now_secs,
    )?;
    Ok(created)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn claim_governance_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_until: u64,
    authority: PostTurnGovernanceAttemptAuthorityV3,
    now_secs: u64,
) -> Result<PostTurnGovernanceJobV3> {
    authority.validate()?;
    if lease_owner.trim().is_empty() || lease_until <= now_secs {
        return Err(Error::invalid_input(
            "post_turn_governance_claim",
            "lease owner and future lease deadline are required",
        ));
    }
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found("post_turn_governance_claim", "governance job not found")
    })?;
    if before_job.status == PostTurnGovernanceJobStatusV2::Leased
        && before_job.lease_until.is_some_and(|until| until > now_secs)
    {
        return Err(Error::conflict(
            "post_turn_governance_claim",
            "governance job already has an active lease",
        ));
    }
    if before_job.status.is_terminal() || before_job.attempt_count >= before_job.max_attempts {
        return Err(Error::conflict(
            "post_turn_governance_claim",
            "governance job is not claimable",
        ));
    }
    let unbound_adoption = before_job.status == PostTurnGovernanceJobStatusV2::BlockedConfiguration
        && before_job.execution_binding == PostTurnGovernanceExecutionBindingV1::Unbound
        && before_job.attempt_authority.is_none()
        && before_job
            .execution_block_authority
            .as_ref()
            .is_some_and(|block| {
                block.typed_block_reason
                    == bm_core::memory::PostTurnGovernanceExecutionBlockReasonV1::BindingUnavailable
            });
    if (matches!(
        before_job.status,
        PostTurnGovernanceJobStatusV2::BlockedConfiguration
            | PostTurnGovernanceJobStatusV2::BlockedCapability
            | PostTurnGovernanceJobStatusV2::BlockedPolicy
    ) && !unbound_adoption)
        || before_job
            .next_attempt_at
            .is_some_and(|eligible_at| eligible_at > now_secs)
    {
        return Err(Error::conflict(
            "post_turn_governance_claim",
            "governance job is not yet eligible for claim",
        ));
    }
    if let Some(pinned) = before_job.attempt_authority.as_ref() {
        if pinned != &authority {
            return Err(Error::conflict(
                "post_turn_governance_claim",
                "retry attempt authority differs from the first claim",
            ));
        }
    }
    match &before_job.execution_binding {
        PostTurnGovernanceExecutionBindingV1::Unbound => {}
        PostTurnGovernanceExecutionBindingV1::Bound {
            binding_id,
            binding_revision,
        } if binding_id == &authority.binding_id
            && *binding_revision == authority.binding_revision => {}
        _ => {
            return Err(Error::conflict(
                "post_turn_governance_claim",
                "execution binding differs from the durable job authority",
            ));
        }
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_claim",
                "governance job is missing its exact scope index",
            )
        })?;
    let mut after_job = before_job.clone();
    after_job.status = PostTurnGovernanceJobStatusV2::Leased;
    after_job.state_revision = after_job.state_revision.checked_add(1).ok_or_else(|| {
        Error::config("post_turn_governance_claim", "job state revision overflow")
    })?;
    after_job.attempt_count = after_job
        .attempt_count
        .checked_add(1)
        .ok_or_else(|| Error::config("post_turn_governance_claim", "job attempt count overflow"))?;
    after_job.lease_epoch = after_job
        .lease_epoch
        .checked_add(1)
        .ok_or_else(|| Error::config("post_turn_governance_claim", "job lease epoch overflow"))?;
    after_job.lease_owner = Some(lease_owner.to_string());
    after_job.lease_until = Some(lease_until);
    after_job.next_attempt_at = None;
    after_job.execution_binding = PostTurnGovernanceExecutionBindingV1::Bound {
        binding_id: authority.binding_id.clone(),
        binding_revision: authority.binding_revision,
    };
    after_job.attempt_authority = Some(authority);
    after_job.execution_block_authority = None;
    after_job.blocking_reason = None;
    after_job.updated_at = now_secs;
    after_job.validate()?;

    let mut after_index = before_index.clone();
    replace_scope_index_ref(
        &mut after_index,
        &before_job,
        &after_job,
        "post_turn_governance_claim",
    )?;
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_claim",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;

    let mut extra_mutations = Vec::new();
    let mut extra_preconditions = Vec::new();
    if before_job.execution_binding == PostTurnGovernanceExecutionBindingV1::Unbound {
        append_binding_reference_mutation(
            platform,
            &after_job.execution_binding,
            now_secs,
            &mut extra_mutations,
            &mut extra_preconditions,
        )?;
    }
    commit_job_and_index_with_extra(
        platform,
        scope,
        runtime_budget,
        "post_turn.governance.claim",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        extra_mutations,
        extra_preconditions,
        now_secs,
    )?;
    Ok(after_job)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn renew_governance_job_lease(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_epoch: u64,
    lease_until: u64,
    now_secs: u64,
) -> Result<PostTurnGovernanceJobV3> {
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found("post_turn_governance_renew", "governance job not found")
    })?;
    if before_job.status != PostTurnGovernanceJobStatusV2::Leased
        || before_job.lease_owner.as_deref() != Some(lease_owner)
        || before_job.lease_epoch != lease_epoch
        || before_job.lease_until.is_none_or(|until| until <= now_secs)
    {
        return Err(Error::conflict(
            "post_turn_governance_renew",
            "renew authority differs from the active unexpired lease",
        ));
    }
    if lease_until <= before_job.lease_until.unwrap_or(0) {
        return Err(Error::invalid_input(
            "post_turn_governance_renew",
            "renewed lease deadline must strictly extend the current lease",
        ));
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_renew",
                "governance job is missing its exact scope index",
            )
        })?;
    let mut after_job = before_job.clone();
    after_job.state_revision = after_job.state_revision.checked_add(1).ok_or_else(|| {
        Error::config("post_turn_governance_renew", "job state revision overflow")
    })?;
    after_job.lease_until = Some(lease_until);
    after_job.updated_at = now_secs;
    after_job.validate()?;

    let mut after_index = before_index.clone();
    replace_scope_index_ref(
        &mut after_index,
        &before_job,
        &after_job,
        "post_turn_governance_renew",
    )?;
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_renew",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;
    commit_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.governance.renew",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        now_secs,
    )?;
    Ok(after_job)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn retry_governance_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_epoch: u64,
    error_class: PostTurnGovernanceErrorClassV2,
    now_secs: u64,
) -> Result<PostTurnGovernanceJobV3> {
    if !error_class.is_retryable() {
        return Err(Error::invalid_input(
            "post_turn_governance_retry",
            "non-retryable errors must use a terminal transition",
        ));
    }
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found("post_turn_governance_retry", "governance job not found")
    })?;
    if before_job.status != PostTurnGovernanceJobStatusV2::Leased
        || before_job.lease_owner.as_deref() != Some(lease_owner)
        || before_job.lease_epoch != lease_epoch
        || before_job.lease_until.is_none_or(|until| until <= now_secs)
    {
        return Err(Error::conflict(
            "post_turn_governance_retry",
            "retry authority differs from the active lease",
        ));
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_retry",
                "governance job is missing its exact scope index",
            )
        })?;
    let mut after_job = before_job.clone();
    after_job.state_revision = after_job.state_revision.checked_add(1).ok_or_else(|| {
        Error::config("post_turn_governance_retry", "job state revision overflow")
    })?;
    after_job.lease_owner = None;
    after_job.lease_until = None;
    after_job.execution_block_authority = None;
    after_job.last_error_class = Some(error_class);
    after_job.updated_at = now_secs;
    let exhausted = after_job.attempt_count >= after_job.max_attempts;
    if exhausted {
        after_job.status = PostTurnGovernanceJobStatusV2::DeadLetter;
        after_job.next_attempt_at = None;
        after_job.blocking_reason = Some("retry_attempts_exhausted".to_string());
        after_job.terminal_at = Some(now_secs);
    } else {
        let exponent = after_job.attempt_count.saturating_sub(1).min(31);
        let delay = GOVERNANCE_RETRY_BASE_SECS
            .saturating_mul(1_u64 << exponent)
            .min(GOVERNANCE_RETRY_MAX_SECS);
        after_job.status = PostTurnGovernanceJobStatusV2::RetryWaiting;
        after_job.next_attempt_at = Some(now_secs.saturating_add(delay));
        after_job.blocking_reason = Some("retry_backoff".to_string());
        after_job.terminal_at = None;
    }
    after_job.validate()?;

    let mut after_index = before_index.clone();
    if exhausted {
        let reference = after_index
            .active_jobs
            .iter()
            .find(|reference| reference.job_id == before_job.job_id)
            .ok_or_else(|| {
                Error::config(
                    "post_turn_governance_retry",
                    "scope index is missing the governance job ref",
                )
            })?;
        if reference.state_revision != before_job.state_revision {
            return Err(Error::conflict(
                "post_turn_governance_retry",
                "scope index job revision differs from job authority",
            ));
        }
        after_index
            .active_jobs
            .retain(|reference| reference.job_id != before_job.job_id);
        append_terminal_ref(&mut after_index, &after_job, "post_turn_governance_retry")?;
    } else {
        replace_scope_index_ref(
            &mut after_index,
            &before_job,
            &after_job,
            "post_turn_governance_retry",
        )?;
    }
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_retry",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;
    commit_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.governance.retry",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        now_secs,
    )?;
    Ok(after_job)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn block_governance_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    status: PostTurnGovernanceJobStatusV2,
    reason: &str,
    execution_block_authority: Option<PostTurnGovernanceExecutionBlockAuthorityV1>,
    now_secs: u64,
) -> Result<PostTurnGovernanceJobV3> {
    if !matches!(
        status,
        PostTurnGovernanceJobStatusV2::BlockedConfiguration
            | PostTurnGovernanceJobStatusV2::BlockedCapability
            | PostTurnGovernanceJobStatusV2::BlockedPolicy
    ) || reason.trim().is_empty()
    {
        return Err(Error::invalid_input(
            "post_turn_governance_block",
            "typed blocked status and reason are required",
        ));
    }
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found("post_turn_governance_block", "governance job not found")
    })?;
    if before_job.status == status && before_job.blocking_reason.as_deref() == Some(reason) {
        return Ok(before_job);
    }
    if before_job.status.is_terminal() || before_job.status == PostTurnGovernanceJobStatusV2::Leased
    {
        return Err(Error::conflict(
            "post_turn_governance_block",
            "leased or terminal governance job cannot be blocked",
        ));
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_block",
                "governance job is missing its runtime scope index",
            )
        })?;
    let mut after_job = before_job.clone();
    after_job.status = status;
    after_job.state_revision = after_job.state_revision.checked_add(1).ok_or_else(|| {
        Error::config("post_turn_governance_block", "job state revision overflow")
    })?;
    after_job.next_attempt_at = None;
    after_job.lease_owner = None;
    after_job.lease_until = None;
    after_job.execution_block_authority = execution_block_authority;
    after_job.blocking_reason = Some(reason.to_string());
    after_job.last_error_class = None;
    after_job.updated_at = now_secs;
    after_job.validate()?;

    let mut after_index = before_index.clone();
    replace_scope_index_ref(
        &mut after_index,
        &before_job,
        &after_job,
        "post_turn_governance_block",
    )?;
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_block",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;
    commit_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.governance.block",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        now_secs,
    )?;
    Ok(after_job)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn block_claimed_governance_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_epoch: u64,
    status: PostTurnGovernanceJobStatusV2,
    reason: &str,
    execution_block_authority: Option<PostTurnGovernanceExecutionBlockAuthorityV1>,
    now_secs: u64,
) -> Result<PostTurnGovernanceJobV3> {
    if !matches!(
        status,
        PostTurnGovernanceJobStatusV2::BlockedConfiguration
            | PostTurnGovernanceJobStatusV2::BlockedCapability
            | PostTurnGovernanceJobStatusV2::BlockedPolicy
    ) || reason.trim().is_empty()
    {
        return Err(Error::invalid_input(
            "post_turn_governance_block_claimed",
            "typed blocked status and reason are required",
        ));
    }
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found(
            "post_turn_governance_block_claimed",
            "governance job not found",
        )
    })?;
    if before_job.status != PostTurnGovernanceJobStatusV2::Leased
        || before_job.lease_owner.as_deref() != Some(lease_owner)
        || before_job.lease_epoch != lease_epoch
        || before_job.lease_until.is_none_or(|until| until <= now_secs)
    {
        return Err(Error::conflict(
            "post_turn_governance_block_claimed",
            "blocked authority differs from the active lease",
        ));
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_block_claimed",
                "governance job is missing its runtime scope index",
            )
        })?;
    let mut after_job = before_job.clone();
    after_job.status = status;
    after_job.state_revision = after_job.state_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_block_claimed",
            "job state revision overflow",
        )
    })?;
    after_job.next_attempt_at = None;
    after_job.lease_owner = None;
    after_job.lease_until = None;
    after_job.execution_block_authority = execution_block_authority;
    after_job.blocking_reason = Some(reason.to_string());
    after_job.last_error_class = None;
    after_job.updated_at = now_secs;
    after_job.validate()?;

    let mut after_index = before_index.clone();
    replace_scope_index_ref(
        &mut after_index,
        &before_job,
        &after_job,
        "post_turn_governance_block_claimed",
    )?;
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_block_claimed",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;
    commit_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.governance.block_claimed",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        now_secs,
    )?;
    Ok(after_job)
}

pub(crate) fn resume_governance_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    now_secs: u64,
) -> Result<PostTurnGovernanceJobV3> {
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found("post_turn_governance_resume", "governance job not found")
    })?;
    if !matches!(
        before_job.status,
        PostTurnGovernanceJobStatusV2::BlockedConfiguration
            | PostTurnGovernanceJobStatusV2::BlockedCapability
            | PostTurnGovernanceJobStatusV2::BlockedPolicy
    ) {
        return Err(Error::conflict(
            "post_turn_governance_resume",
            "only a blocked governance job can be resumed",
        ));
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_resume",
                "governance job is missing its runtime scope index",
            )
        })?;
    let mut after_job = before_job.clone();
    after_job.status = PostTurnGovernanceJobStatusV2::Pending;
    after_job.state_revision = after_job.state_revision.checked_add(1).ok_or_else(|| {
        Error::config("post_turn_governance_resume", "job state revision overflow")
    })?;
    after_job.next_attempt_at = Some(now_secs);
    after_job.execution_block_authority = None;
    after_job.blocking_reason = None;
    after_job.last_error_class = None;
    after_job.updated_at = now_secs;
    after_job.validate()?;

    let mut after_index = before_index.clone();
    replace_scope_index_ref(
        &mut after_index,
        &before_job,
        &after_job,
        "post_turn_governance_resume",
    )?;
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_resume",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;
    commit_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.governance.resume",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        now_secs,
    )?;
    Ok(after_job)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resume_governance_job_for_credential(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    credential_ref_safe_id: &str,
    new_generation: u64,
    operation_id: &str,
    actor_subject_id: &str,
    now_secs: u64,
) -> Result<GovernanceRecoveryOutcome> {
    if credential_ref_safe_id.trim().is_empty()
        || new_generation == 0
        || operation_id.trim().is_empty()
        || operation_id.trim() != operation_id
    {
        return Err(Error::invalid_input(
            "post_turn_governance_credential_resume",
            "safe credential identity, generation, and operation id are required",
        ));
    }
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found(
            "post_turn_governance_credential_resume",
            "governance job not found",
        )
    })?;
    let block = before_job
        .execution_block_authority
        .as_ref()
        .ok_or_else(|| {
            Error::conflict(
                "post_turn_governance_credential_resume",
                "governance job has no durable execution block authority",
            )
        })?;
    if before_job.status != PostTurnGovernanceJobStatusV2::BlockedConfiguration
        || !block.typed_block_reason.is_credential_scoped()
        || block.credential_ref_safe_id.as_deref() != Some(credential_ref_safe_id)
        || block
            .credential_generation
            .is_some_and(|generation| new_generation <= generation)
    {
        return Err(Error::conflict(
            "post_turn_governance_credential_resume",
            "credential recovery authority does not advance the exact blocked generation",
        ));
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_credential_resume",
                "governance job is missing its exact scope index",
            )
        })?;
    let mut after_job = before_job.clone();
    after_job.status = PostTurnGovernanceJobStatusV2::Pending;
    after_job.state_revision = after_job.state_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_credential_resume",
            "job state revision overflow",
        )
    })?;
    after_job.next_attempt_at = Some(now_secs);
    after_job.execution_block_authority = None;
    after_job.blocking_reason = None;
    after_job.last_error_class = None;
    after_job.updated_at = now_secs;
    after_job.validate()?;

    let mut after_index = before_index.clone();
    replace_scope_index_ref(
        &mut after_index,
        &before_job,
        &after_job,
        "post_turn_governance_credential_resume",
    )?;
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_credential_resume",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;
    let replayed = commit_recovery_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.governance.credential_resume",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        operation_id,
        actor_subject_id,
        MemoryMutationOperationKind::GovernanceCredentialRecovery,
        credential_ref_safe_id,
        new_generation,
        now_secs,
    )?;
    Ok(GovernanceRecoveryOutcome {
        job: read_job(platform, &after_job.job_id)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_credential_resume",
                "recovered governance job disappeared after operation commit",
            )
        })?,
        replayed,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resume_governance_job_for_provider_permission(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    binding_id: &str,
    binding_revision: u64,
    new_generation: u64,
    operation_id: &str,
    actor_subject_id: &str,
    now_secs: u64,
) -> Result<GovernanceRecoveryOutcome> {
    if binding_id.trim().is_empty()
        || binding_id.trim() != binding_id
        || binding_revision == 0
        || new_generation == 0
        || operation_id.trim().is_empty()
        || operation_id.trim() != operation_id
    {
        return Err(Error::invalid_input(
            "post_turn_governance_permission_resume",
            "exact binding, advanced permission generation, and operation id are required",
        ));
    }
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found(
            "post_turn_governance_permission_resume",
            "governance job not found",
        )
    })?;
    let block = before_job
        .execution_block_authority
        .as_ref()
        .ok_or_else(|| {
            Error::conflict(
                "post_turn_governance_permission_resume",
                "governance job has no durable execution block authority",
            )
        })?;
    if before_job.status != PostTurnGovernanceJobStatusV2::BlockedPolicy
        || block.typed_block_reason
            != bm_core::memory::PostTurnGovernanceExecutionBlockReasonV1::ProviderPermissionDenied
        || block.binding_id.as_deref() != Some(binding_id)
        || block.binding_revision != Some(binding_revision)
        || block
            .provider_permission_generation
            .is_none_or(|generation| new_generation <= generation)
        || before_job
            .last_provider_permission_recovery_generation
            .is_some_and(|generation| new_generation <= generation)
    {
        return Err(Error::conflict(
            "post_turn_governance_permission_resume",
            "permission recovery authority does not advance the exact blocked binding generation",
        ));
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_permission_resume",
                "governance job is missing its exact scope index",
            )
        })?;
    let mut after_job = before_job.clone();
    after_job.status = PostTurnGovernanceJobStatusV2::Pending;
    after_job.state_revision = after_job.state_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_permission_resume",
            "job state revision overflow",
        )
    })?;
    after_job.next_attempt_at = Some(now_secs);
    after_job.execution_block_authority = None;
    after_job.last_provider_permission_recovery_generation = Some(new_generation);
    after_job.blocking_reason = None;
    after_job.last_error_class = None;
    after_job.updated_at = now_secs;
    after_job.validate()?;

    let mut after_index = before_index.clone();
    replace_scope_index_ref(
        &mut after_index,
        &before_job,
        &after_job,
        "post_turn_governance_permission_resume",
    )?;
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_permission_resume",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;
    let replayed = commit_recovery_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.governance.permission_resume",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        operation_id,
        actor_subject_id,
        MemoryMutationOperationKind::GovernanceProviderPermissionRecovery,
        binding_id,
        new_generation,
        now_secs,
    )?;
    Ok(GovernanceRecoveryOutcome {
        job: read_job(platform, &after_job.job_id)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_permission_resume",
                "recovered governance job disappeared after operation commit",
            )
        })?,
        replayed,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dead_letter_governance_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_epoch: u64,
    error_class: PostTurnGovernanceErrorClassV2,
    reason: &str,
    now_secs: u64,
) -> Result<PostTurnGovernanceJobV3> {
    if error_class.is_retryable() || reason.trim().is_empty() {
        return Err(Error::invalid_input(
            "post_turn_governance_fail",
            "non-retryable error class and reason are required",
        ));
    }
    let before_job = read_job(platform, job_id)?
        .ok_or_else(|| Error::not_found("post_turn_governance_fail", "governance job not found"))?;
    if before_job.status != PostTurnGovernanceJobStatusV2::Leased
        || before_job.lease_owner.as_deref() != Some(lease_owner)
        || before_job.lease_epoch != lease_epoch
        || before_job.lease_until.is_none_or(|until| until <= now_secs)
    {
        return Err(Error::conflict(
            "post_turn_governance_fail",
            "failure authority differs from the active lease",
        ));
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_fail",
                "governance job is missing its runtime scope index",
            )
        })?;
    let mut after_job = before_job.clone();
    after_job.status = PostTurnGovernanceJobStatusV2::DeadLetter;
    after_job.state_revision = after_job
        .state_revision
        .checked_add(1)
        .ok_or_else(|| Error::config("post_turn_governance_fail", "job state revision overflow"))?;
    after_job.next_attempt_at = None;
    after_job.lease_owner = None;
    after_job.lease_until = None;
    after_job.execution_block_authority = None;
    after_job.last_error_class = Some(error_class);
    after_job.blocking_reason = Some(reason.to_string());
    after_job.updated_at = now_secs;
    after_job.terminal_at = Some(now_secs);
    after_job.validate()?;

    let mut after_index = before_index.clone();
    let reference = after_index
        .active_jobs
        .iter()
        .find(|reference| reference.job_id == before_job.job_id)
        .ok_or_else(|| {
            Error::config(
                "post_turn_governance_fail",
                "runtime scope index is missing the governance job ref",
            )
        })?;
    if reference.state_revision != before_job.state_revision {
        return Err(Error::conflict(
            "post_turn_governance_fail",
            "runtime scope index job revision differs from job authority",
        ));
    }
    after_index
        .active_jobs
        .retain(|reference| reference.job_id != before_job.job_id);
    append_terminal_ref(&mut after_index, &after_job, "post_turn_governance_fail")?;
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_fail",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;
    commit_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.governance.fail",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        now_secs,
    )?;
    Ok(after_job)
}

pub(crate) fn cancel_governance_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    reason: &str,
    now_secs: u64,
) -> Result<PostTurnGovernanceJobV3> {
    if reason.trim().is_empty() {
        return Err(Error::invalid_input(
            "post_turn_governance_cancel",
            "cancellation reason is required",
        ));
    }
    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found("post_turn_governance_cancel", "governance job not found")
    })?;
    if before_job.status.is_terminal() {
        return Ok(before_job);
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_cancel",
                "governance job is missing its exact scope index",
            )
        })?;
    let mut after_job = before_job.clone();
    after_job.status = PostTurnGovernanceJobStatusV2::Cancelled;
    after_job.state_revision = after_job.state_revision.checked_add(1).ok_or_else(|| {
        Error::config("post_turn_governance_cancel", "job state revision overflow")
    })?;
    after_job.next_attempt_at = None;
    after_job.lease_owner = None;
    after_job.lease_until = None;
    after_job.execution_block_authority = None;
    after_job.blocking_reason = Some(reason.to_string());
    after_job.last_error_class = None;
    after_job.terminal_at = Some(now_secs);
    after_job.updated_at = now_secs;
    after_job.validate()?;

    let mut after_index = before_index.clone();
    let reference = after_index
        .active_jobs
        .iter()
        .find(|reference| reference.job_id == before_job.job_id)
        .ok_or_else(|| {
            Error::config(
                "post_turn_governance_cancel",
                "scope index is missing the governance job ref",
            )
        })?;
    if reference.state_revision != before_job.state_revision {
        return Err(Error::conflict(
            "post_turn_governance_cancel",
            "scope index job revision differs from job authority",
        ));
    }
    after_index
        .active_jobs
        .retain(|reference| reference.job_id != before_job.job_id);
    append_terminal_ref(&mut after_index, &after_job, "post_turn_governance_cancel")?;
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_cancel",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;

    commit_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.governance.cancel",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        now_secs,
    )?;
    Ok(after_job)
}

#[allow(clippy::too_many_arguments)]
fn plan_governance_job_completion(
    platform: &StorePlatform,
    scope: StoreEventScope,
    job_id: &str,
    lease_owner: &str,
    lease_epoch: u64,
    decision_summary: PostTurnGovernanceDecisionSummaryV1,
    mut memory_mutations: Vec<StoreMutation>,
    mut memory_preconditions: Vec<StoreJsonPrecondition>,
    now_secs: u64,
) -> Result<GovernanceCompletionPlan> {
    if job_id.trim().is_empty() || lease_owner.trim().is_empty() || lease_epoch == 0 {
        return Err(Error::invalid_input(
            "post_turn_governance_complete",
            "job id and active lease authority are required",
        ));
    }
    if memory_mutations.iter().any(|mutation| match mutation {
        StoreMutation::PutJson { namespace, .. } | StoreMutation::DeleteJson { namespace, .. } => {
            is_governance_namespace(namespace)
        }
        StoreMutation::PutBlob { .. }
        | StoreMutation::DeleteBlob { .. }
        | StoreMutation::AppendEvent { .. } => false,
    }) || memory_preconditions.iter().any(|precondition| {
        let namespace = match precondition {
            StoreJsonPrecondition::Absent { namespace, .. }
            | StoreJsonPrecondition::Exact { namespace, .. } => namespace,
        };
        is_governance_namespace(namespace)
    }) {
        return Err(Error::invalid_input(
            "post_turn_governance_complete",
            "memory plan must not mutate governance job ownership",
        ));
    }

    let mutation_plan_digest = governance_sha256(
        &serde_json::to_vec(&(&memory_mutations, &memory_preconditions))
            .map_err(|error| Error::config("post_turn_governance_complete", error.to_string()))?,
    );
    let memory_post_image_digest = governance_sha256(
        &serde_json::to_vec(&memory_mutations)
            .map_err(|error| Error::config("post_turn_governance_complete", error.to_string()))?,
    );
    let transaction_id = governance_completion_transaction_id(job_id, lease_epoch)?;
    let expected_receipt = PostTurnGovernanceReceiptV3::new(
        job_id,
        transaction_id.clone(),
        mutation_plan_digest,
        memory_post_image_digest,
        now_secs,
        decision_summary,
    )?;

    let before_job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::not_found("post_turn_governance_complete", "governance job not found")
    })?;
    if before_job.status == PostTurnGovernanceJobStatusV2::Succeeded {
        let job = completed_job_if_same_receipt(before_job, &expected_receipt)?;
        return Ok(GovernanceCompletionPlan::AlreadyCompleted {
            job: Box::new(job),
            transaction_id,
        });
    }
    if before_job.status != PostTurnGovernanceJobStatusV2::Leased
        || before_job.lease_owner.as_deref() != Some(lease_owner)
        || before_job.lease_epoch != lease_epoch
        || before_job.lease_until.is_none_or(|until| until <= now_secs)
    {
        return Err(Error::conflict(
            "post_turn_governance_complete",
            "completion authority differs from the active lease",
        ));
    }
    let before_index =
        read_scope_index(platform, &before_job.scope_index_key)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_complete",
                "governance job is missing its exact scope index",
            )
        })?;
    let reference = before_index
        .active_jobs
        .iter()
        .find(|reference| reference.job_id == before_job.job_id)
        .ok_or_else(|| {
            Error::config(
                "post_turn_governance_complete",
                "scope index is missing the governance job ref",
            )
        })?;
    if reference.state_revision != before_job.state_revision {
        return Err(Error::conflict(
            "post_turn_governance_complete",
            "scope index job revision differs from job authority",
        ));
    }

    let mut after_job = before_job.clone();
    after_job.status = PostTurnGovernanceJobStatusV2::Succeeded;
    after_job.state_revision = after_job.state_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_complete",
            "job state revision overflow",
        )
    })?;
    after_job.next_attempt_at = None;
    after_job.lease_owner = None;
    after_job.lease_until = None;
    after_job.execution_block_authority = None;
    after_job.blocking_reason = None;
    after_job.last_error_class = None;
    after_job.receipt = Some(expected_receipt.clone());
    after_job.updated_at = now_secs;
    after_job.terminal_at = Some(now_secs);
    after_job.validate()?;

    let mut after_index = before_index.clone();
    after_index
        .active_jobs
        .retain(|reference| reference.job_id != before_job.job_id);
    append_terminal_ref(
        &mut after_index,
        &after_job,
        "post_turn_governance_complete",
    )?;
    after_index.index_revision = after_index.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_complete",
            "governance scope index revision overflow",
        )
    })?;
    after_index.updated_at = now_secs;
    after_index.validate()?;

    let before_job_value = serde_json::to_value(&before_job)
        .map_err(|error| Error::config("post_turn_governance_complete", error.to_string()))?;
    let before_index_value = serde_json::to_value(&before_index)
        .map_err(|error| Error::config("post_turn_governance_complete", error.to_string()))?;
    merge_completion_preconditions(
        &mut memory_preconditions,
        [
            StoreJsonPrecondition::Exact {
                namespace: POST_TURN_GOVERNANCE_JOB_NAMESPACE.to_string(),
                key: before_job.job_id.clone(),
                value: before_job_value,
            },
            StoreJsonPrecondition::Exact {
                namespace: POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE.to_string(),
                key: before_job.scope_index_key.clone(),
                value: before_index_value,
            },
        ],
    )?;
    memory_mutations.push(put_json(
        POST_TURN_GOVERNANCE_JOB_NAMESPACE,
        &after_job.job_id,
        serde_json::to_value(&after_job)
            .map_err(|error| Error::config("post_turn_governance_complete", error.to_string()))?,
    ));
    memory_mutations.push(put_json(
        POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
        &after_job.scope_index_key,
        serde_json::to_value(&after_index)
            .map_err(|error| Error::config("post_turn_governance_complete", error.to_string()))?,
    ));

    Ok(GovernanceCompletionPlan::Pending(Box::new(
        GovernanceCompletionPending {
            expected_receipt,
            after_job,
            batch: StoreMutationBatch {
                transaction_id,
                operation: "post_turn.governance.complete".to_string(),
                scope,
                mutations: memory_mutations,
            },
            preconditions: memory_preconditions,
        },
    )))
}

enum GovernanceCompletionPlan {
    AlreadyCompleted {
        job: Box<PostTurnGovernanceJobV3>,
        transaction_id: String,
    },
    Pending(Box<GovernanceCompletionPending>),
}

struct GovernanceCompletionPending {
    expected_receipt: PostTurnGovernanceReceiptV3,
    after_job: PostTurnGovernanceJobV3,
    batch: StoreMutationBatch,
    preconditions: Vec<StoreJsonPrecondition>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn complete_governance_job_with_memory_plan(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_epoch: u64,
    decision_summary: PostTurnGovernanceDecisionSummaryV1,
    memory_mutations: Vec<StoreMutation>,
    memory_preconditions: Vec<StoreJsonPrecondition>,
    memory_blob_preconditions: Vec<StoreBlobPrecondition>,
    now_secs: u64,
) -> Result<PostTurnGovernanceJobV3> {
    let plan = plan_governance_job_completion(
        platform,
        scope,
        job_id,
        lease_owner,
        lease_epoch,
        decision_summary,
        memory_mutations,
        memory_preconditions,
        now_secs,
    )?;
    let (expected_receipt, after_job, batch, preconditions) = match plan {
        GovernanceCompletionPlan::AlreadyCompleted { job, .. } => return Ok(*job),
        GovernanceCompletionPlan::Pending(pending) => {
            let GovernanceCompletionPending {
                expected_receipt,
                after_job,
                batch,
                preconditions,
            } = *pending;
            (expected_receipt, after_job, batch, preconditions)
        }
    };
    let commit = platform
        .commit_governed_memory_transaction_with_blob_preconditions_and_runtime_budget_at(
            batch,
            &preconditions,
            &memory_blob_preconditions,
            runtime_budget,
            now_secs,
        );
    match commit {
        Ok(_) => Ok(after_job),
        Err(error) => {
            if let Some(current) = read_job(platform, job_id)? {
                if current.status == PostTurnGovernanceJobStatusV2::Succeeded {
                    return completed_job_if_same_receipt(current, &expected_receipt);
                }
            }
            Err(error)
        }
    }
}

#[derive(Debug)]
pub(crate) struct SubjectSoulGovernanceCompletionOutcome {
    pub(crate) job: PostTurnGovernanceJobV3,
    pub(crate) soul: SubjectSoulStoreMutationOutcome,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn complete_governance_job_with_subject_soul_plan(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_epoch: u64,
    decision_summary: PostTurnGovernanceDecisionSummaryV1,
    soul_plan: SubjectSoulStoreMutationPlan,
    additional_mutations: Vec<StoreMutation>,
    additional_preconditions: Vec<StoreJsonPrecondition>,
    additional_blob_preconditions: Vec<StoreBlobPrecondition>,
    now_secs: u64,
) -> Result<SubjectSoulGovernanceCompletionOutcome> {
    let additional_touches_protected = additional_mutations.iter().any(|mutation| {
        matches!(mutation,
            StoreMutation::PutJson { namespace, .. }
                | StoreMutation::DeleteJson { namespace, .. }
                if is_subject_soul_protected_json_namespace(namespace)
                    || is_relationship_source_protected_json_namespace(namespace))
    }) || additional_preconditions.iter().any(|precondition| {
        let namespace = match precondition {
            StoreJsonPrecondition::Absent { namespace, .. }
            | StoreJsonPrecondition::Exact { namespace, .. } => namespace,
        };
        is_subject_soul_protected_json_namespace(namespace)
            || is_relationship_source_protected_json_namespace(namespace)
    });
    if additional_touches_protected {
        return Err(Error::invalid_input(
            "post_turn_governance_subject_soul_complete",
            "additional governance memory plan must not contain protected Soul/Relationship addresses",
        ));
    }
    if soul_plan.batch().scope != scope {
        return Err(Error::invalid_input(
            "post_turn_governance_subject_soul_complete",
            "governance completion scope differs from the typed Subject Soul owner",
        ));
    }

    let transaction_id = governance_completion_transaction_id(job_id, lease_epoch)?;
    let mut rebound_batch = soul_plan.batch().clone();
    rebound_batch.transaction_id = transaction_id;
    rebound_batch.operation = "post_turn.governance.complete.subject_soul".to_string();
    let soul_preconditions = soul_plan.preconditions().to_vec();
    let soul_plan = soul_plan
        .bind_governance_completion(rebound_batch, soul_preconditions)
        .map_err(SubjectSoulStoreFailure::into_store_error)?;
    let mut combined_mutations = soul_plan.batch().mutations.clone();
    combined_mutations.extend(additional_mutations);
    let mut combined_preconditions = soul_plan.preconditions().to_vec();
    merge_completion_preconditions(&mut combined_preconditions, additional_preconditions)?;
    let replay_batch = StoreMutationBatch {
        transaction_id: soul_plan.batch().transaction_id.clone(),
        operation: "post_turn.governance.complete.subject_soul".to_string(),
        scope: scope.clone(),
        mutations: combined_mutations.clone(),
    };
    let replay_preconditions = combined_preconditions.clone();
    let completion = plan_governance_job_completion(
        platform,
        scope,
        job_id,
        lease_owner,
        lease_epoch,
        decision_summary,
        combined_mutations,
        combined_preconditions,
        now_secs,
    )?;
    let (expected_receipt, expected_job, final_batch, final_preconditions) = match completion {
        GovernanceCompletionPlan::AlreadyCompleted {
            job,
            transaction_id,
        } => {
            if transaction_id != replay_batch.transaction_id {
                return Err(Error::conflict(
                    "post_turn_governance_subject_soul_complete",
                    "completed governance transaction differs from the typed replay",
                ));
            }
            (
                job.receipt.clone(),
                *job,
                replay_batch,
                replay_preconditions,
            )
        }
        GovernanceCompletionPlan::Pending(pending) => {
            let GovernanceCompletionPending {
                expected_receipt,
                after_job,
                mut batch,
                preconditions,
            } = *pending;
            batch.operation = "post_turn.governance.complete.subject_soul".to_string();
            (Some(expected_receipt), after_job, batch, preconditions)
        }
    };
    let soul_plan = soul_plan
        .bind_governance_completion(final_batch, final_preconditions)
        .map_err(SubjectSoulStoreFailure::into_store_error)?;
    let soul_plan = soul_plan
        .append_governance_blob_preconditions(additional_blob_preconditions)
        .map_err(SubjectSoulStoreFailure::into_store_error)?;
    let soul = platform
        .commit_subject_soul_mutation_with_runtime_budget(soul_plan, runtime_budget)
        .map_err(SubjectSoulStoreFailure::into_store_error)?;
    let job = read_job(platform, job_id)?.ok_or_else(|| {
        Error::config(
            "post_turn_governance_subject_soul_complete",
            "atomic Subject Soul commit is missing its governance job post-image",
        )
    })?;
    let expected_receipt = expected_receipt.ok_or_else(|| {
        Error::config(
            "post_turn_governance_subject_soul_complete",
            "completed governance job is missing its receipt",
        )
    })?;
    let job = completed_job_if_same_receipt(job, &expected_receipt)?;
    if job != expected_job {
        return Err(Error::conflict(
            "post_turn_governance_subject_soul_complete",
            "persisted governance post-image differs from the planned atomic completion",
        ));
    }
    Ok(SubjectSoulGovernanceCompletionOutcome { job, soul })
}

fn completed_job_if_same_receipt(
    job: PostTurnGovernanceJobV3,
    expected: &PostTurnGovernanceReceiptV3,
) -> Result<PostTurnGovernanceJobV3> {
    let Some(actual) = job.receipt.as_ref() else {
        return Err(Error::config(
            "post_turn_governance_complete",
            "succeeded governance job is missing its receipt",
        ));
    };
    if actual.semantic_transaction_id == expected.semantic_transaction_id
        && actual.mutation_plan_digest == expected.mutation_plan_digest
        && actual.memory_post_image_digest == expected.memory_post_image_digest
        && actual.decision_summary == expected.decision_summary
    {
        return Ok(job);
    }
    Err(Error::conflict(
        "post_turn_governance_complete",
        "governance job already completed with a different semantic result",
    ))
}

fn is_governance_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        POST_TURN_GOVERNANCE_JOB_NAMESPACE | POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE
    )
}

fn governance_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

pub(crate) fn governance_completion_transaction_id(
    job_id: &str,
    lease_epoch: u64,
) -> Result<String> {
    if job_id.trim().is_empty() || lease_epoch == 0 {
        return Err(Error::invalid_input(
            "post_turn_governance_complete",
            "job id and lease epoch are required for transaction identity",
        ));
    }
    let digest = governance_sha256(
        &serde_json::to_vec(&(job_id, lease_epoch))
            .map_err(|error| Error::config("post_turn_governance_complete", error.to_string()))?,
    );
    Ok(digest)
}

fn merge_completion_preconditions(
    target: &mut Vec<StoreJsonPrecondition>,
    incoming: impl IntoIterator<Item = StoreJsonPrecondition>,
) -> Result<()> {
    let mut by_address = std::collections::BTreeMap::new();
    for precondition in target.drain(..).chain(incoming) {
        let address = match &precondition {
            StoreJsonPrecondition::Absent { namespace, key }
            | StoreJsonPrecondition::Exact { namespace, key, .. } => {
                (namespace.clone(), key.clone())
            }
        };
        if let Some(existing) = by_address.get(&address) {
            if existing != &precondition {
                return Err(Error::conflict(
                    "post_turn_governance_complete",
                    "memory plan contains conflicting JSON preconditions",
                ));
            }
        } else {
            by_address.insert(address, precondition);
        }
    }
    target.extend(by_address.into_values());
    Ok(())
}

pub(crate) fn ensure_binding_snapshot(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    snapshot: &PostTurnGovernanceBindingSnapshotV1,
    now_secs: u64,
) -> Result<PostTurnGovernanceBindingSnapshotV1> {
    snapshot.validate()?;
    let key = snapshot.storage_key();
    if let Some(existing) =
        read_binding_snapshot(platform, &snapshot.binding_id, snapshot.binding_revision)?
    {
        if binding_snapshots_share_canonical_identity(&existing, snapshot) {
            let index = read_binding_revision_index(platform, &snapshot.binding_id)?;
            if index
                .as_ref()
                .is_some_and(|index| binding_snapshot_has_exact_revision_ref(&existing, index))
            {
                return Ok(existing);
            }
            return Err(Error::config(
                "post_turn_governance_binding_revision_index",
                "binding snapshot lacks its exact revision index reference",
            ));
        }
        return Err(Error::conflict(
            "post_turn_governance_binding_snapshot",
            "immutable binding identity already has a different canonical snapshot",
        ));
    }
    let before_index = read_binding_revision_index(platform, &snapshot.binding_id)?;
    let mut after_index = before_index
        .clone()
        .unwrap_or_else(|| PostTurnGovernanceBindingRevisionIndexV1::empty(snapshot));
    if after_index.binding_id != snapshot.binding_id
        || after_index.source_owner_id != snapshot.source_owner_id
        || after_index.source_config_id != snapshot.source_config_id
    {
        return Err(Error::config(
            "post_turn_governance_binding_revision_index",
            "binding revision index owner differs from the immutable snapshot",
        ));
    }
    let mut pruned = None;
    if after_index.revisions.len() >= MAX_POST_TURN_GOVERNANCE_BINDING_REVISIONS {
        let position = after_index
            .revisions
            .iter()
            .position(|revision| !revision.referenced)
            .ok_or_else(|| {
                Error::config(
                    "post_turn_governance_binding_retention_exhausted",
                    "all retained binding revisions are referenced by durable governance jobs",
                )
            })?;
        pruned = Some(after_index.revisions.remove(position));
    }
    after_index
        .revisions
        .push(PostTurnGovernanceBindingRevisionRefV1::from_snapshot(
            snapshot,
        ));
    after_index
        .revisions
        .sort_by_key(|revision| revision.binding_revision);
    if before_index.is_some() {
        after_index.index_revision =
            after_index.index_revision.checked_add(1).ok_or_else(|| {
                Error::config(
                    "post_turn_governance_binding_revision_index",
                    "binding revision index revision overflow",
                )
            })?;
    }
    after_index.updated_at = now_secs;
    after_index.validate()?;
    let value = serde_json::to_value(snapshot).map_err(|error| {
        Error::config("post_turn_governance_binding_snapshot", error.to_string())
    })?;
    let index_value = serde_json::to_value(&after_index).map_err(|error| {
        Error::config(
            "post_turn_governance_binding_revision_index",
            error.to_string(),
        )
    })?;
    let mut mutations = Vec::with_capacity(3);
    let mut preconditions = vec![
        StoreJsonPrecondition::Absent {
            namespace: POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE.to_string(),
            key: key.clone(),
        },
        json_precondition(
            POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_NAMESPACE,
            &snapshot.binding_id,
            before_index.as_ref(),
        )?,
    ];
    if let Some(pruned) = pruned {
        let pruned_key = format!("{}:{}", snapshot.binding_id, pruned.binding_revision);
        let before_snapshot =
            read_binding_snapshot(platform, &snapshot.binding_id, pruned.binding_revision)?
                .ok_or_else(|| {
                    Error::config(
                        "post_turn_governance_binding_revision_index",
                        "binding revision index references a missing snapshot",
                    )
                })?;
        if before_snapshot.canonical_digest != pruned.canonical_digest {
            return Err(Error::config(
                "post_turn_governance_binding_revision_index",
                "binding revision index digest differs from its snapshot",
            ));
        }
        preconditions.push(json_precondition(
            POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE,
            &pruned_key,
            Some(&before_snapshot),
        )?);
        mutations.push(StoreMutation::DeleteJson {
            namespace: POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE.to_string(),
            key: pruned_key.clone(),
            event_kind: MemoryStoreEventKind::MemoryMaintenance,
            plane: "post_turn_governance".to_string(),
            record_key: pruned_key,
        });
    }
    mutations.push(put_json(
        POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE,
        &key,
        value,
    ));
    mutations.push(put_json(
        POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_NAMESPACE,
        &snapshot.binding_id,
        index_value,
    ));
    let commit = platform.commit_governed_memory_transaction_with_runtime_budget_at(
        StoreMutationBatch {
            transaction_id: format!("post_turn_governance_binding_{}", snapshot.canonical_digest),
            operation: "post_turn.governance.binding.ensure".to_string(),
            scope,
            mutations,
        },
        &preconditions,
        runtime_budget,
        now_secs,
    );
    match commit {
        Ok(_) => Ok(snapshot.clone()),
        Err(error) if error.stage() == "memory_write_transaction_precondition_failed" => {
            let existing =
                read_binding_snapshot(platform, &snapshot.binding_id, snapshot.binding_revision)?;
            let index = read_binding_revision_index(platform, &snapshot.binding_id)?;
            if existing.as_ref().is_some_and(|existing| {
                binding_snapshots_share_canonical_identity(existing, snapshot)
                    && index.as_ref().is_some_and(|index| {
                        binding_snapshot_has_exact_revision_ref(existing, index)
                    })
            }) {
                Ok(existing.expect("exact binding snapshot checked above"))
            } else if existing.is_some() {
                Err(Error::conflict(
                    "post_turn_governance_binding_snapshot",
                    "immutable binding identity already has a different canonical snapshot",
                ))
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

fn binding_snapshots_share_canonical_identity(
    existing: &PostTurnGovernanceBindingSnapshotV1,
    requested: &PostTurnGovernanceBindingSnapshotV1,
) -> bool {
    existing.binding_id == requested.binding_id
        && existing.binding_revision == requested.binding_revision
        && existing.canonical_digest == requested.canonical_digest
}

fn binding_snapshot_has_exact_revision_ref(
    snapshot: &PostTurnGovernanceBindingSnapshotV1,
    index: &PostTurnGovernanceBindingRevisionIndexV1,
) -> bool {
    index.binding_id == snapshot.binding_id
        && index.source_owner_id == snapshot.source_owner_id
        && index.source_config_id == snapshot.source_config_id
        && index.revisions.iter().any(|revision| {
            revision.binding_revision == snapshot.binding_revision
                && revision.canonical_digest == snapshot.canonical_digest
                && revision.created_at == snapshot.created_at
        })
}

pub(crate) fn read_binding_revision_index(
    platform: &StorePlatform,
    binding_id: &str,
) -> Result<Option<PostTurnGovernanceBindingRevisionIndexV1>> {
    if binding_id.trim().is_empty() || binding_id.trim() != binding_id {
        return Err(Error::invalid_input(
            "post_turn_governance_binding_revision_index",
            "canonical binding identity is required",
        ));
    }
    let mut docs = platform.read_json_docs_by_keys(
        POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_NAMESPACE,
        &[binding_id.to_string()],
    )?;
    let Some(doc) = docs.pop() else {
        return Ok(None);
    };
    let index: PostTurnGovernanceBindingRevisionIndexV1 = serde_json::from_value(doc.value)
        .map_err(|error| {
            Error::config(
                "post_turn_governance_binding_revision_index",
                error.to_string(),
            )
        })?;
    index.validate()?;
    Ok(Some(index))
}

pub(crate) fn read_binding_snapshot(
    platform: &StorePlatform,
    binding_id: &str,
    binding_revision: u64,
) -> Result<Option<PostTurnGovernanceBindingSnapshotV1>> {
    if binding_id.trim().is_empty() || binding_revision == 0 {
        return Err(Error::invalid_input(
            "post_turn_governance_binding_snapshot",
            "binding identity and revision are required",
        ));
    }
    let key = format!("{binding_id}:{binding_revision}");
    let mut docs =
        platform.read_json_docs_by_keys(POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE, &[key])?;
    let Some(doc) = docs.pop() else {
        return Ok(None);
    };
    let snapshot: PostTurnGovernanceBindingSnapshotV1 =
        serde_json::from_value(doc.value).map_err(|error| {
            Error::config("post_turn_governance_binding_snapshot", error.to_string())
        })?;
    snapshot.validate()?;
    Ok(Some(snapshot))
}

pub(crate) fn read_job(
    platform: &StorePlatform,
    job_id: &str,
) -> Result<Option<PostTurnGovernanceJobV3>> {
    let mut docs = platform
        .read_json_docs_by_keys(POST_TURN_GOVERNANCE_JOB_NAMESPACE, &[job_id.to_string()])?;
    let Some(doc) = docs.pop() else {
        return Ok(None);
    };
    serde_json::from_value(doc.value)
        .map(Some)
        .map_err(|error| Error::config("post_turn_governance_read", error.to_string()))
}

pub(crate) fn read_scope_index(
    platform: &StorePlatform,
    scope_index_key: &str,
) -> Result<Option<PostTurnGovernanceScopeIndexV3>> {
    let mut docs = platform.read_json_docs_by_keys(
        POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
        &[scope_index_key.to_string()],
    )?;
    let Some(doc) = docs.pop() else {
        return Ok(None);
    };
    serde_json::from_value(doc.value)
        .map(Some)
        .map_err(|error| Error::config("post_turn_governance_read", error.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn commit_job_and_index(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    operation: &str,
    before_job: &PostTurnGovernanceJobV3,
    after_job: &PostTurnGovernanceJobV3,
    before_index: &PostTurnGovernanceScopeIndexV3,
    after_index: &PostTurnGovernanceScopeIndexV3,
    now_secs: u64,
) -> Result<()> {
    commit_job_and_index_with_extra(
        platform,
        scope,
        runtime_budget,
        operation,
        before_job,
        after_job,
        before_index,
        after_index,
        Vec::new(),
        Vec::new(),
        now_secs,
    )
}

#[allow(clippy::too_many_arguments)]
fn commit_job_and_index_with_extra(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    operation: &str,
    before_job: &PostTurnGovernanceJobV3,
    after_job: &PostTurnGovernanceJobV3,
    before_index: &PostTurnGovernanceScopeIndexV3,
    after_index: &PostTurnGovernanceScopeIndexV3,
    mut extra_mutations: Vec<StoreMutation>,
    mut extra_preconditions: Vec<StoreJsonPrecondition>,
    now_secs: u64,
) -> Result<()> {
    let before_job_value = serde_json::to_value(before_job)
        .map_err(|error| Error::config("post_turn_governance_transaction", error.to_string()))?;
    let after_job_value = serde_json::to_value(after_job)
        .map_err(|error| Error::config("post_turn_governance_transaction", error.to_string()))?;
    let before_index_value = serde_json::to_value(before_index)
        .map_err(|error| Error::config("post_turn_governance_transaction", error.to_string()))?;
    let after_index_value = serde_json::to_value(after_index)
        .map_err(|error| Error::config("post_turn_governance_transaction", error.to_string()))?;
    let mut mutations = vec![
        put_json(
            POST_TURN_GOVERNANCE_JOB_NAMESPACE,
            &after_job.job_id,
            after_job_value,
        ),
        put_json(
            POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
            &after_job.scope_index_key,
            after_index_value,
        ),
    ];
    mutations.append(&mut extra_mutations);
    let mut preconditions = vec![
        StoreJsonPrecondition::Exact {
            namespace: POST_TURN_GOVERNANCE_JOB_NAMESPACE.to_string(),
            key: before_job.job_id.clone(),
            value: before_job_value,
        },
        StoreJsonPrecondition::Exact {
            namespace: POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE.to_string(),
            key: before_job.scope_index_key.clone(),
            value: before_index_value,
        },
    ];
    preconditions.append(&mut extra_preconditions);
    platform.commit_governed_memory_transaction_with_runtime_budget_at(
        StoreMutationBatch {
            transaction_id: format!(
                "post_turn_governance_{}_{}",
                after_job.job_id, after_job.state_revision
            ),
            operation: operation.to_string(),
            scope,
            mutations,
        },
        &preconditions,
        runtime_budget,
        now_secs,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn commit_recovery_job_and_index(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    operation: &str,
    before_job: &PostTurnGovernanceJobV3,
    after_job: &PostTurnGovernanceJobV3,
    before_index: &PostTurnGovernanceScopeIndexV3,
    after_index: &PostTurnGovernanceScopeIndexV3,
    caller_operation_id: &str,
    actor_subject_id: &str,
    operation_kind: MemoryMutationOperationKind,
    recovery_authority_key: &str,
    new_generation: u64,
    now_secs: u64,
) -> Result<bool> {
    let identity = governance_recovery_operation_identity(
        before_job,
        caller_operation_id,
        actor_subject_id,
        operation_kind,
    )?;
    let intent_digest = governance_recovery_intent_digest(
        &before_job.job_id,
        operation_kind,
        recovery_authority_key,
        new_generation,
    )?;
    let operation_plan = StoreMutationOperationPlan::new(
        identity.clone(),
        intent_digest,
        MemoryMutationEffect::Changed,
        2,
        actor_subject_id,
        now_secs,
    )?;
    let before_job_value = serde_json::to_value(before_job).map_err(|error| {
        Error::config("post_turn_governance_recovery_operation", error.to_string())
    })?;
    let before_index_value = serde_json::to_value(before_index).map_err(|error| {
        Error::config("post_turn_governance_recovery_operation", error.to_string())
    })?;
    let after_job_value = serde_json::to_value(after_job).map_err(|error| {
        Error::config("post_turn_governance_recovery_operation", error.to_string())
    })?;
    let after_index_value = serde_json::to_value(after_index).map_err(|error| {
        Error::config("post_turn_governance_recovery_operation", error.to_string())
    })?;
    let event = crate::MemoryStoreEvent::new(
        format!("post-turn-governance-recovery:{}", identity.storage_key()),
        MemoryStoreEventKind::MemoryControl,
        scope.clone(),
        now_secs,
    )
    .with_plane("post_turn_governance")
    .with_record_key(&after_job.job_id);
    let outcome = platform.commit_memory_mutation_operation_with_runtime_budget(
        StoreMutationBatch {
            transaction_id: operation_plan.transaction_id().to_string(),
            operation: operation.to_string(),
            scope,
            mutations: vec![
                put_json(
                    POST_TURN_GOVERNANCE_JOB_NAMESPACE,
                    &after_job.job_id,
                    after_job_value,
                ),
                put_json(
                    POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
                    &after_job.scope_index_key,
                    after_index_value,
                ),
                StoreMutation::AppendEvent {
                    event: Box::new(event),
                },
            ],
        },
        &[
            StoreJsonPrecondition::Exact {
                namespace: POST_TURN_GOVERNANCE_JOB_NAMESPACE.to_string(),
                key: before_job.job_id.clone(),
                value: before_job_value,
            },
            StoreJsonPrecondition::Exact {
                namespace: POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE.to_string(),
                key: before_job.scope_index_key.clone(),
                value: before_index_value,
            },
        ],
        operation_plan,
        runtime_budget,
    )?;
    Ok(matches!(
        outcome,
        StoreMutationOperationOutcome::Replayed { .. }
    ))
}

fn governance_recovery_intent_digest(
    job_id: &str,
    operation_kind: MemoryMutationOperationKind,
    recovery_authority_key: &str,
    new_generation: u64,
) -> Result<String> {
    let operation_label = match operation_kind {
        MemoryMutationOperationKind::GovernanceCredentialRecovery => "credential",
        MemoryMutationOperationKind::GovernanceProviderPermissionRecovery => "permission",
        _ => {
            return Err(Error::config(
                "post_turn_governance_recovery_operation",
                "recovery intent requires an exact governance recovery operation kind",
            ));
        }
    };
    if job_id.trim().is_empty()
        || recovery_authority_key.trim().is_empty()
        || recovery_authority_key.trim() != recovery_authority_key
        || new_generation == 0
    {
        return Err(Error::invalid_input(
            "post_turn_governance_recovery_operation",
            "canonical recovery job, authority key, and generation are required",
        ));
    }
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(
            [
                b"post-turn-governance-recovery-intent-v1\0".as_slice(),
                operation_label.as_bytes(),
                b"\0",
                job_id.as_bytes(),
                b"\0",
                recovery_authority_key.as_bytes(),
                b"\0",
                &new_generation.to_be_bytes(),
            ]
            .concat()
        )
    ))
}

fn governance_recovery_operation_identity(
    job: &PostTurnGovernanceJobV3,
    caller_operation_id: &str,
    actor_subject_id: &str,
    operation_kind: MemoryMutationOperationKind,
) -> Result<MemoryMutationOperationIdentity> {
    let effective_operation_id = format!(
        "sha256:{:x}",
        Sha256::digest(
            [
                b"post-turn-governance-recovery-operation-v1\0".as_slice(),
                caller_operation_id.as_bytes(),
                b"\0",
                job.job_id.as_bytes(),
            ]
            .concat()
        )
    );
    MemoryMutationOperationIdentity::new(
        effective_operation_id,
        &job.identity.memory_space_id,
        &job.identity.mounted_subject_id,
        actor_subject_id,
        operation_kind,
    )
}

fn append_binding_reference_mutation(
    platform: &StorePlatform,
    execution_binding: &PostTurnGovernanceExecutionBindingV1,
    now_secs: u64,
    mutations: &mut Vec<StoreMutation>,
    preconditions: &mut Vec<StoreJsonPrecondition>,
) -> Result<()> {
    let PostTurnGovernanceExecutionBindingV1::Bound {
        binding_id,
        binding_revision,
    } = execution_binding
    else {
        return Ok(());
    };
    let snapshot =
        read_binding_snapshot(platform, binding_id, *binding_revision)?.ok_or_else(|| {
            Error::config(
                "post_turn_governance_binding_reference",
                "bound governance job is missing its immutable binding snapshot",
            )
        })?;
    let before = read_binding_revision_index(platform, binding_id)?.ok_or_else(|| {
        Error::config(
            "post_turn_governance_binding_reference",
            "bound governance job is missing its binding revision index",
        )
    })?;
    let mut after = before.clone();
    let revision = after
        .revisions
        .iter_mut()
        .find(|revision| revision.binding_revision == *binding_revision)
        .ok_or_else(|| {
            Error::config(
                "post_turn_governance_binding_reference",
                "binding revision index is missing the bound revision",
            )
        })?;
    if revision.canonical_digest != snapshot.canonical_digest {
        return Err(Error::config(
            "post_turn_governance_binding_reference",
            "binding revision index digest differs from the immutable snapshot",
        ));
    }
    preconditions.push(json_precondition(
        POST_TURN_GOVERNANCE_BINDING_SNAPSHOT_NAMESPACE,
        &snapshot.storage_key(),
        Some(&snapshot),
    )?);
    preconditions.push(json_precondition(
        POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_NAMESPACE,
        binding_id,
        Some(&before),
    )?);
    if revision.referenced {
        return Ok(());
    }
    revision.referenced = true;
    after.index_revision = after.index_revision.checked_add(1).ok_or_else(|| {
        Error::config(
            "post_turn_governance_binding_reference",
            "binding revision index revision overflow",
        )
    })?;
    after.updated_at = now_secs;
    after.validate()?;
    mutations.push(put_json(
        POST_TURN_GOVERNANCE_BINDING_REVISION_INDEX_NAMESPACE,
        binding_id,
        serde_json::to_value(after).map_err(|error| {
            Error::config("post_turn_governance_binding_reference", error.to_string())
        })?,
    ));
    Ok(())
}

fn replace_scope_index_ref(
    index: &mut PostTurnGovernanceScopeIndexV3,
    before_job: &PostTurnGovernanceJobV3,
    after_job: &PostTurnGovernanceJobV3,
    stage: &'static str,
) -> Result<()> {
    let reference = index
        .active_jobs
        .iter_mut()
        .find(|reference| reference.job_id == before_job.job_id)
        .ok_or_else(|| Error::config(stage, "scope index is missing the governance job ref"))?;
    if reference.state_revision != before_job.state_revision {
        return Err(Error::conflict(
            stage,
            "scope index job revision differs from job authority",
        ));
    }
    *reference = PostTurnGovernanceJobRefV2::from_job(after_job);
    Ok(())
}

fn append_terminal_ref(
    index: &mut PostTurnGovernanceScopeIndexV3,
    job: &PostTurnGovernanceJobV3,
    stage: &'static str,
) -> Result<()> {
    if !job.status.is_terminal() {
        return Err(Error::config(
            stage,
            "terminal index ref requires terminal job",
        ));
    }
    if index.recent_terminal_jobs.len() >= MAX_POST_TURN_GOVERNANCE_RECENT_TERMINAL_JOBS {
        return Err(Error::config(
            stage,
            "governance terminal-receipt retention is exhausted",
        ));
    }
    if index
        .recent_terminal_jobs
        .iter()
        .any(|reference| reference.job_id == job.job_id)
    {
        return Err(Error::conflict(
            stage,
            "scope index already contains the terminal governance job ref",
        ));
    }
    index
        .recent_terminal_jobs
        .push(PostTurnGovernanceJobRefV2::from_job(job));
    index.recent_terminal_jobs.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.job_id.cmp(&right.job_id))
    });
    Ok(())
}

fn json_precondition<T: serde::Serialize>(
    namespace: &str,
    key: &str,
    value: Option<&T>,
) -> Result<StoreJsonPrecondition> {
    match value {
        Some(value) => Ok(StoreJsonPrecondition::Exact {
            namespace: namespace.to_string(),
            key: key.to_string(),
            value: serde_json::to_value(value).map_err(|error| {
                Error::config("post_turn_governance_transaction", error.to_string())
            })?,
        }),
        None => Ok(StoreJsonPrecondition::Absent {
            namespace: namespace.to_string(),
            key: key.to_string(),
        }),
    }
}

fn put_json(namespace: &str, key: &str, value: serde_json::Value) -> StoreMutation {
    StoreMutation::PutJson {
        namespace: namespace.to_string(),
        key: key.to_string(),
        value,
        event_kind: MemoryStoreEventKind::MemoryMaintenance,
        plane: "post_turn_governance".to_string(),
        record_key: key.to_string(),
    }
}

#[cfg(all(test, feature = "nonproduction-replay-harness"))]
mod tests {
    use crate::store_internal::subject_soul::tests::unseeded_plan_for_scope;
    use crate::store_internal::subject_soul::SubjectSoulStoreMutationOutcome;
    use crate::store_internal::{StoreBackendConfig, StoreCapacityBudget, StorePlatform};
    use crate::{MemoryStoreEventKind, ProfileId, StoreEventScope, StoreMutation};
    use bm_core::memory::{
        PostTurnGovernanceAttemptAuthorityV3, PostTurnGovernanceBindingSnapshotV1,
        PostTurnGovernanceDecisionSummaryV1, PostTurnGovernanceExecutionBindingV1,
        PostTurnGovernanceIdentityV2, PostTurnGovernanceJobStatusV2, PostTurnGovernanceJobV3,
        PostTurnGovernancePrivacyAuthorityV1, PostTurnGovernanceProviderProtocolV1,
        PostTurnSemanticGovernanceReport, SubjectSoulReadOutcomeV1, SubjectSoulReadRequestV1,
        SubjectSoulReadSelectorV1, SubjectSoulReadViewV1,
        POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
    };

    use super::{
        claim_governance_job, complete_governance_job_with_memory_plan,
        complete_governance_job_with_subject_soul_plan, ensure_binding_snapshot,
        ensure_governance_intent, read_job, read_scope_index,
    };

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_secs()
    }

    fn pending_job(now_secs: u64) -> PostTurnGovernanceJobV3 {
        PostTurnGovernanceJobV3::pending(
            PostTurnGovernanceIdentityV2::new(
                "space:atomic",
                "subject:atomic",
                "llm.gateway",
                "chat:atomic",
                "conversation:atomic",
                "turn:atomic",
            )
            .expect("identity"),
            1,
            "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            "sha256:2222222222222222222222222222222222222222222222222222222222222222",
            PostTurnGovernanceExecutionBindingV1::Unbound,
            PostTurnGovernancePrivacyAuthorityV1 {
                policy_schema_version: 1,
                exact_policy_digest:
                    "sha256:3333333333333333333333333333333333333333333333333333333333333333"
                        .to_string(),
            },
            Vec::new(),
            0,
            3,
            now_secs,
        )
        .expect("pending job")
    }

    fn scope() -> StoreEventScope {
        StoreEventScope::new("agent:atomic", "owner:atomic", "llm.gateway", "chat:atomic")
            .with_memory_space("space:atomic")
            .with_subject("subject:atomic")
            .with_conversation("conversation:atomic")
    }

    fn authority(
        platform: &StorePlatform,
        budget: &crate::RuntimeBudgetReport,
        now_secs: u64,
    ) -> PostTurnGovernanceAttemptAuthorityV3 {
        let snapshot = PostTurnGovernanceBindingSnapshotV1::new(
            "owner:atomic",
            "config:atomic",
            1,
            PostTurnGovernanceProviderProtocolV1::OpenAiCompatible,
            "https://governance.invalid/v1",
            "model:atomic",
            None,
            30_000,
            4_096,
            1_024,
            1,
            now_secs,
        )
        .expect("binding snapshot");
        let snapshot = ensure_binding_snapshot(platform, scope(), budget, &snapshot, now_secs)
            .expect("install binding snapshot");
        PostTurnGovernanceAttemptAuthorityV3 {
            binding_id: snapshot.binding_id,
            binding_revision: snapshot.binding_revision,
            model_id: snapshot.model_id,
            privacy_authority: PostTurnGovernancePrivacyAuthorityV1 {
                policy_schema_version: 1,
                exact_policy_digest:
                    "sha256:3333333333333333333333333333333333333333333333333333333333333333"
                        .to_string(),
            },
            transcript_lifecycle_revision: 1,
            disclosure_authority_digest:
                "sha256:5555555555555555555555555555555555555555555555555555555555555555"
                    .to_string(),
        }
    }

    fn decision_summary() -> PostTurnGovernanceDecisionSummaryV1 {
        PostTurnGovernanceDecisionSummaryV1::from_semantic_report(
            &PostTurnSemanticGovernanceReport::skipped("synthetic no-candidate completion"),
        )
        .expect("decision summary")
    }

    #[test]
    fn completion_commits_memory_terminal_job_and_index_in_one_transaction() {
        let platform = StorePlatform::open(
            StoreBackendConfig::in_memory(ProfileId::DesktopMacosEmbeddedSdk)
                .expect("store config"),
        )
        .expect("store");
        let now_secs = now_secs();
        let budget = platform.current_runtime_budget(now_secs);
        let pending = pending_job(now_secs);
        ensure_governance_intent(&platform, scope(), &budget, &pending, now_secs)
            .expect("ensure intent");
        let leased = claim_governance_job(
            &platform,
            scope(),
            &budget,
            &pending.job_id,
            "worker:atomic",
            now_secs + 60,
            authority(&platform, &budget, now_secs),
            now_secs,
        )
        .expect("claim");
        let memory_event = crate::MemoryStoreEvent::new(
            "governance-memory-event:atomic",
            MemoryStoreEventKind::MemoryWrite,
            scope(),
            now_secs + 1,
        )
        .with_plane("post_turn_memory")
        .with_record_key("atomic-memory-plan");
        let mutations = vec![StoreMutation::AppendEvent {
            event: Box::new(memory_event.clone()),
        }];
        let preconditions = Vec::new();

        let completed = complete_governance_job_with_memory_plan(
            &platform,
            scope(),
            &budget,
            &leased.job_id,
            "worker:atomic",
            leased.lease_epoch,
            decision_summary(),
            mutations.clone(),
            preconditions.clone(),
            Vec::new(),
            now_secs + 1,
        )
        .expect("atomic complete");
        assert_eq!(completed.status, PostTurnGovernanceJobStatusV2::Succeeded);
        assert!(completed.receipt.is_some());
        assert_eq!(
            read_job(&platform, &completed.job_id)
                .expect("read job")
                .expect("job"),
            completed
        );
        let index = read_scope_index(&platform, &completed.scope_index_key)
            .expect("read index")
            .expect("index");
        assert!(index.active_jobs.is_empty());
        assert!(platform
            .read_events()
            .expect("memory events")
            .iter()
            .any(|event| event.event_id == memory_event.event_id));

        let duplicate = complete_governance_job_with_memory_plan(
            &platform,
            scope(),
            &budget,
            &completed.job_id,
            "worker:atomic",
            leased.lease_epoch,
            decision_summary(),
            mutations,
            preconditions,
            Vec::new(),
            now_secs + 2,
        )
        .expect("idempotent complete");
        assert_eq!(duplicate.receipt, completed.receipt);
        assert!(
            platform
                .read_json_docs_by_keys(
                    POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
                    &[completed.scope_index_key]
                )
                .expect("index doc")
                .len()
                == 1
        );
    }

    #[test]
    fn typed_soul_completion_commits_and_replays_one_governance_transaction() {
        let platform = StorePlatform::open(
            StoreBackendConfig::in_memory(ProfileId::DesktopMacosEmbeddedSdk)
                .expect("store config"),
        )
        .expect("store");
        let now_secs = now_secs();
        let budget = platform.current_runtime_budget(now_secs);
        let pending = pending_job(now_secs);
        ensure_governance_intent(&platform, scope(), &budget, &pending, now_secs)
            .expect("ensure intent");
        let leased = claim_governance_job(
            &platform,
            scope(),
            &budget,
            &pending.job_id,
            "worker:atomic-soul",
            now_secs + 60,
            authority(&platform, &budget, now_secs),
            now_secs,
        )
        .expect("claim");
        let soul_plan = unseeded_plan_for_scope(
            &platform,
            "governance-soul-operation",
            "space:atomic",
            "subject:atomic",
            "soul:atomic",
            now_secs + 1,
            scope(),
        );
        let replay_plan = soul_plan.clone();
        let additional_event = crate::MemoryStoreEvent::new(
            "governance-additional-memory:atomic-soul",
            MemoryStoreEventKind::MemoryWrite,
            scope(),
            now_secs + 1,
        )
        .with_plane("post_turn_memory")
        .with_record_key("additional-memory");
        let committed = complete_governance_job_with_subject_soul_plan(
            &platform,
            scope(),
            &budget,
            &leased.job_id,
            "worker:atomic-soul",
            leased.lease_epoch,
            decision_summary(),
            soul_plan,
            vec![StoreMutation::AppendEvent {
                event: Box::new(additional_event.clone()),
            }],
            Vec::new(),
            Vec::new(),
            now_secs + 1,
        )
        .expect("atomic typed Soul governance completion");
        assert_eq!(
            committed.job.status,
            PostTurnGovernanceJobStatusV2::Succeeded
        );
        assert!(matches!(
            committed.soul,
            SubjectSoulStoreMutationOutcome::Committed { .. }
        ));
        let read = platform
            .read_verified_subject_soul(
                "space:atomic",
                "soul:atomic",
                &SubjectSoulReadRequestV1 {
                    target_subject_id: "subject:atomic".to_string(),
                    selector: SubjectSoulReadSelectorV1::Current,
                    view: SubjectSoulReadViewV1::OperatorSafe,
                },
                &budget,
            )
            .expect("read atomic explicit unseeded Soul");
        assert!(matches!(
            read.outcome,
            SubjectSoulReadOutcomeV1::Verified { .. }
        ));
        let before_replay = platform
            .export_store_snapshot()
            .expect("snapshot before response-loss replay");
        let replayed = complete_governance_job_with_subject_soul_plan(
            &platform,
            scope(),
            &budget,
            &leased.job_id,
            "worker:atomic-soul",
            leased.lease_epoch,
            decision_summary(),
            replay_plan,
            vec![StoreMutation::AppendEvent {
                event: Box::new(additional_event),
            }],
            Vec::new(),
            Vec::new(),
            now_secs + 1,
        )
        .expect("response-loss replay");
        assert!(matches!(
            replayed.soul,
            SubjectSoulStoreMutationOutcome::Replayed { .. }
        ));
        assert_eq!(replayed.job, committed.job);
        assert_eq!(
            platform
                .export_store_snapshot()
                .expect("snapshot after response-loss replay"),
            before_replay,
            "replay must not duplicate Soul roots, revisions, receipts, audits, events, or job completion"
        );
    }

    #[test]
    fn typed_soul_completion_budget_failure_changes_nothing() {
        let mut capacity = StoreCapacityBudget::full();
        capacity.event_log_max_items = 8;
        let config = StoreBackendConfig::in_memory(ProfileId::DesktopMacosEmbeddedSdk)
            .expect("store config")
            .try_with_nonproduction_store_budget_limit(capacity.into_runtime_budget())
            .expect("constrained Store budget");
        let platform = StorePlatform::open(config).expect("store");
        let now_secs = now_secs();
        let budget = platform.current_runtime_budget(now_secs);
        let pending = pending_job(now_secs);
        ensure_governance_intent(&platform, scope(), &budget, &pending, now_secs)
            .expect("ensure intent");
        let leased = claim_governance_job(
            &platform,
            scope(),
            &budget,
            &pending.job_id,
            "worker:atomic-soul-budget",
            now_secs + 60,
            authority(&platform, &budget, now_secs),
            now_secs,
        )
        .expect("claim");
        let soul_plan = unseeded_plan_for_scope(
            &platform,
            "governance-soul-budget-operation",
            "space:atomic",
            "subject:atomic",
            "soul:atomic",
            now_secs + 1,
            scope(),
        );
        let before = platform
            .export_store_snapshot()
            .expect("snapshot before composite budget rejection");
        let error = complete_governance_job_with_subject_soul_plan(
            &platform,
            scope(),
            &budget,
            &leased.job_id,
            "worker:atomic-soul-budget",
            leased.lease_epoch,
            decision_summary(),
            soul_plan,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            now_secs + 1,
        )
        .expect_err("composite must exceed constrained event capacity");
        assert_eq!(error.stage(), "subject_soul_store_capacity");
        assert_eq!(
            platform
                .export_store_snapshot()
                .expect("snapshot after composite budget rejection"),
            before,
            "failed composite must leave Soul roots/result/MOR/audit/events and governance job/index unchanged"
        );
    }

    #[test]
    fn typed_soul_completion_rejects_additional_protected_writes_without_changes() {
        let platform = StorePlatform::open(
            StoreBackendConfig::in_memory(ProfileId::DesktopMacosEmbeddedSdk)
                .expect("store config"),
        )
        .expect("store");
        let now_secs = now_secs();
        let budget = platform.current_runtime_budget(now_secs);
        let pending = pending_job(now_secs);
        ensure_governance_intent(&platform, scope(), &budget, &pending, now_secs)
            .expect("ensure intent");
        let leased = claim_governance_job(
            &platform,
            scope(),
            &budget,
            &pending.job_id,
            "worker:atomic-soul-protected",
            now_secs + 60,
            authority(&platform, &budget, now_secs),
            now_secs,
        )
        .expect("claim");
        let soul_plan = unseeded_plan_for_scope(
            &platform,
            "governance-soul-protected-operation",
            "space:atomic",
            "subject:atomic",
            "soul:atomic",
            now_secs + 1,
            scope(),
        );
        let before = platform
            .export_store_snapshot()
            .expect("snapshot before protected additional write rejection");
        let error = complete_governance_job_with_subject_soul_plan(
            &platform,
            scope(),
            &budget,
            &leased.job_id,
            "worker:atomic-soul-protected",
            leased.lease_epoch,
            decision_summary(),
            soul_plan,
            vec![StoreMutation::PutJson {
                namespace: "private_garden".to_string(),
                key: "forbidden-bypass".to_string(),
                value: serde_json::json!({"raw": "must-not-commit"}),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: "private_garden".to_string(),
                record_key: "forbidden-bypass".to_string(),
            }],
            Vec::new(),
            Vec::new(),
            now_secs + 1,
        )
        .expect_err("additional protected writes must not bypass typed Soul closure");
        assert_eq!(error.stage(), "post_turn_governance_subject_soul_complete");
        assert_eq!(
            platform
                .export_store_snapshot()
                .expect("snapshot after protected additional write rejection"),
            before,
            "rejected additional protected write must not mutate Soul, MOR, audit, events, or governance job/index"
        );
    }
}
