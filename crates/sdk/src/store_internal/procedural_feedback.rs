use std::collections::{BTreeMap, BTreeSet};

use bm_core::memory::{
    GovernedMemoryOwnerPlane, MemoryMutationAuditRecord, MemoryMutationEffect,
    MemoryMutationOperationIdentity, MemoryMutationOperationKind, MemoryMutationReceipt,
    PostTurnGovernanceJobRefV2, PostTurnGovernanceJobV3, PostTurnGovernanceScopeIndexV3,
    ProceduralAppliedOwnerBindingV1, ProceduralFeedbackApplicationLedgerV1,
    ProceduralFeedbackErrorClassV1, ProceduralFeedbackJobRefV1, ProceduralFeedbackJobStatusV1,
    ProceduralFeedbackJobV1, ProceduralFeedbackReceiptV1, ProceduralFeedbackReconciliationCursorV1,
    ProceduralFeedbackScopeIndexV1, MAX_PROCEDURAL_FEEDBACK_ACTIVE_JOBS,
    MAX_PROCEDURAL_FEEDBACK_RECENT_TERMINAL_JOBS, PROCEDURAL_FEEDBACK_RECEIPT_SCHEMA_VERSION,
};
use bm_core::skills::{
    AgentToolExperienceOwnerHeadV2, AgentToolExperienceRetainedRevisionDigestV2,
    AgentToolExperienceRevisionMaterialV2,
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
    PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
};
use super::transaction::BackendTransactionState;
use super::{StoreMutationOperationOutcome, StoreMutationOperationPlan, StorePlatform};
use bm_core::memory::{
    MAX_POST_TURN_GOVERNANCE_ACTIVE_JOBS, MAX_POST_TURN_GOVERNANCE_RECENT_TERMINAL_JOBS,
    POST_TURN_GOVERNANCE_JOB_NAMESPACE, POST_TURN_GOVERNANCE_SCOPE_INDEX_NAMESPACE,
};

const PROCEDURAL_RETRY_BASE_SECS: u64 = 5;
const PROCEDURAL_RETRY_MAX_SECS: u64 = 300;

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
    fn procedural(job: &'a ProceduralFeedbackJobV1) -> Result<Self> {
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
            learning_evidence_digest: Some(&job.learning_evidence_digest),
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
        AgentToolExperienceHeadStateV2, AgentToolExperienceOwningScopeV1,
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
        let before: ProceduralFeedbackScopeIndexV1 = decode(document.value, stage)?;
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
                job.identity.memory_space_id.as_str(),
                job.identity.mounted_subject_id.as_str(),
                job.identity.channel_id.as_str(),
                job.identity.conversation_id.as_str(),
                job.identity.turn_id.as_str(),
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
    // scheduling index. Lifecycle must still find old completed derivations.
    let lifecycle_budget = platform.current_runtime_budget(super::platform::current_unix_secs());
    for document in
        platform.read_procedural_application_ledgers_with_runtime_budget(&lifecycle_budget)?
    {
        let ledger: ProceduralFeedbackApplicationLedgerV1 = decode(document.value, stage)?;
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
            || ledger.identity != job.identity
            || ledger.learning_evidence_digest != job.learning_evidence_digest
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
        let head: AgentToolExperienceOwnerHeadV2 = decode(doc.value, stage)?;
        if head.memory_space_id != scope.memory_space_id
            || head.owning_scope != owning_scope
            || *binding != AgentToolExperienceHeadBindingV1::from_head(&head)?
        {
            return Err(Error::config(
                stage,
                "derived experience head differs from exact scope binding",
            ));
        }
        if head.state == AgentToolExperienceHeadStateV2::Tombstoned {
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
            let typed_material: AgentToolExperienceRevisionMaterialV2 =
                decode(material.value.clone(), stage)?;
            if AgentToolExperienceRetainedRevisionDigestV2::from_material(&typed_material)?
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
    procedural_job: Option<&ProceduralFeedbackJobV1>,
    now_secs: u64,
) -> Result<(
    GovernanceIntentEnsureOutcome,
    Option<ProceduralIntentEnsureOutcome>,
)> {
    semantic_job.validate()?;
    if let Some(job) = procedural_job {
        job.validate()?;
        validate_scope(&scope, job, "post_turn_learning_intents")?;
        if job.identity.memory_space_id != semantic_job.identity.memory_space_id
            || job.identity.mounted_subject_id != semantic_job.identity.mounted_subject_id
            || job.identity.channel_id != semantic_job.identity.channel_id
            || job.identity.chat_id != semantic_job.identity.chat_id
            || job.identity.conversation_id != semantic_job.identity.conversation_id
            || job.identity.turn_id != semantic_job.identity.turn_id
            || job.transcript_sequence != semantic_job.transcript_sequence
            || job.transcript_digest != semantic_job.transcript_digest
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
        let before_index = read_scope_index(platform, &job.scope_index_key)?;
        let mut after_index = before_index
            .clone()
            .unwrap_or_else(|| ProceduralFeedbackScopeIndexV1::empty(&job.identity, now_secs));
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
            .push(ProceduralFeedbackJobRefV1::from_job(job));
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
                &job.scope_index_key,
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
                &job.scope_index_key,
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
    pub(crate) deferred_count: u32,
    pub(crate) rejected_count: u32,
    pub(crate) changed_count: u32,
    pub(crate) reason_digest: String,
    pub(crate) completed_at: u64,
}

#[derive(Clone, Debug)]
pub(crate) enum ProceduralFeedbackCompletionOutcome {
    Committed {
        job: ProceduralFeedbackJobV1,
        receipt: ProceduralFeedbackReceiptV1,
    },
    Replayed {
        job: ProceduralFeedbackJobV1,
        receipt: ProceduralFeedbackReceiptV1,
    },
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn reconcile_procedural_feedback_intents(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    identity: &bm_core::memory::ProceduralFeedbackIdentityV1,
    jobs: &[ProceduralFeedbackJobV1],
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
    let mut after_index = before_index
        .clone()
        .unwrap_or_else(|| ProceduralFeedbackScopeIndexV1::empty(identity, now_secs));
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
        if job.scope_index_key != scope_index_key
            || job.identity.conversation_id != identity.conversation_id
            || job.transcript_sequence > cursor_sequence
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
                    .push(ProceduralFeedbackJobRefV1::from_job(job));
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
    now_secs: u64,
    limit: usize,
) -> Result<Vec<ProceduralFeedbackJobV1>> {
    if limit == 0 || limit > MAX_PROCEDURAL_FEEDBACK_ACTIVE_JOBS {
        return Err(Error::invalid_input(
            "procedural_feedback_discover",
            "discovery limit must be positive and bounded",
        ));
    }
    let Some(index) = read_scope_index(platform, scope_index_key)? else {
        return Ok(Vec::new());
    };
    let keys = index
        .active_jobs
        .iter()
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
    let mut jobs = platform
        .read_json_docs_by_keys(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &keys)?
        .into_iter()
        .map(|doc| decode(doc.value, "procedural_feedback_discover"))
        .collect::<Result<Vec<ProceduralFeedbackJobV1>>>()?;
    jobs.retain(|job| match job.status {
        ProceduralFeedbackJobStatusV1::Pending => true,
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
) -> Result<ProceduralFeedbackJobV1> {
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
    if before_job.status.is_terminal()
        || before_job.attempt_count >= before_job.max_attempts
        || (before_job.status == ProceduralFeedbackJobStatusV1::Leased
            && before_job
                .lease_until
                .is_some_and(|deadline| deadline > now_secs))
        || (before_job.status == ProceduralFeedbackJobStatusV1::RetryWaiting
            && before_job
                .next_attempt_at
                .is_some_and(|eligible| eligible > now_secs))
    {
        return Err(Error::conflict(
            "procedural_feedback_claim",
            "procedural feedback job is not claimable",
        ));
    }
    let before_index = required_index(platform, &before_job, "procedural_feedback_claim")?;
    let mut after_job = before_job.clone();
    after_job.status = ProceduralFeedbackJobStatusV1::Leased;
    after_job.state_revision = checked_increment(
        after_job.state_revision,
        "procedural_feedback_claim",
        "job state revision overflow",
    )?;
    after_job.attempt_count = after_job
        .attempt_count
        .checked_add(1)
        .ok_or_else(|| Error::config("procedural_feedback_claim", "job attempt count overflow"))?;
    after_job.lease_epoch = checked_increment(
        after_job.lease_epoch,
        "procedural_feedback_claim",
        "job lease epoch overflow",
    )?;
    after_job.next_attempt_at = None;
    after_job.lease_owner = Some(lease_owner.to_string());
    after_job.lease_until = Some(lease_until);
    after_job.last_error_class = None;
    after_job.updated_at = now_secs;
    after_job.validate()?;
    let after_index = updated_active_index(
        &before_index,
        &before_job,
        &after_job,
        now_secs,
        "procedural_feedback_claim",
    )?;
    if let Err(error) = commit_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.procedural.claim",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        now_secs,
    ) {
        if error.stage() == "memory_write_transaction_precondition_failed" {
            return Err(Error::conflict(
                "procedural_feedback_claim",
                "procedural feedback claim lost its Store compare-and-swap",
            ));
        }
        return Err(error);
    }
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
) -> Result<ProceduralFeedbackJobV1> {
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
    let before_index = required_index(platform, &before_job, "procedural_feedback_retry")?;
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
    if exhausted {
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
    let after_index = if exhausted {
        moved_to_terminal_index(
            &before_index,
            &before_job,
            &after_job,
            now_secs,
            "procedural_feedback_retry",
        )?
    } else {
        updated_active_index(
            &before_index,
            &before_job,
            &after_job,
            now_secs,
            "procedural_feedback_retry",
        )?
    };
    commit_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.procedural.retry",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
        now_secs,
    )?;
    Ok(after_job)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn repair_required_procedural_feedback_job(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    job_id: &str,
    lease_owner: &str,
    lease_epoch: u64,
    error_class: ProceduralFeedbackErrorClassV1,
    now_secs: u64,
) -> Result<ProceduralFeedbackJobV1> {
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
    let before_index =
        required_index(platform, &before_job, "procedural_feedback_repair_required")?;
    let mut after_job = before_job.clone();
    after_job.status = ProceduralFeedbackJobStatusV1::RepairRequired;
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
    let after_index = moved_to_terminal_index(
        &before_index,
        &before_job,
        &after_job,
        now_secs,
        "procedural_feedback_repair_required",
    )?;
    commit_job_and_index(
        platform,
        scope,
        runtime_budget,
        "post_turn.procedural.repair_required",
        &before_job,
        &after_job,
        &before_index,
        &after_index,
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
        .checked_add(input.deferred_count)
        .and_then(|value| value.checked_add(input.rejected_count));
    if accounted != Some(before_job.submitted_count)
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
        input.deferred_count,
        input.rejected_count,
        input.changed_count,
    )?;
    validate_runtime_skill_promotion_sources(
        &before_job,
        &input.experience_mutations,
        &input.experience_preconditions,
    )?;
    let ledger = ProceduralFeedbackApplicationLedgerV1::build(
        &before_job,
        input.applied_owner_bindings,
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
    let mut receipt = ProceduralFeedbackReceiptV1 {
        mutation_receipt_key: identity.storage_key(),
        schema_version: PROCEDURAL_FEEDBACK_RECEIPT_SCHEMA_VERSION,
        job_id: before_job.job_id.clone(),
        operation_id: input.operation_id,
        transaction_id: String::new(),
        transcript_digest: before_job.transcript_digest.clone(),
        learning_evidence_digest: before_job.learning_evidence_digest.clone(),
        plan_digest: plan_digest.clone(),
        post_image_digest,
        submitted_count: before_job.submitted_count,
        accepted_count: input.accepted_count,
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
    if !receipt.validate_contract() {
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
    after_job.receipt = Some(receipt.clone());
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
            &after_job.scope_index_key,
            encode(&after_index, "procedural_feedback_complete")?,
        ),
        put_json(
            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
            &ledger.job_id,
            encode(&ledger, "procedural_feedback_complete")?,
        ),
    ]);
    let mut preconditions = input.experience_preconditions;
    preconditions.extend([
        exact(
            PROCEDURAL_FEEDBACK_JOB_NAMESPACE,
            &before_job.job_id,
            &before_job,
            "procedural_feedback_complete",
        )?,
        exact(
            PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE,
            &before_job.scope_index_key,
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
                receipt: replayed_receipt,
            })
        }
    }
}

pub(crate) fn read_job(
    platform: &StorePlatform,
    job_id: &str,
) -> Result<Option<ProceduralFeedbackJobV1>> {
    let mut docs = platform
        .read_json_docs_by_keys(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &[job_id.to_string()])?;
    docs.pop()
        .map(|doc| decode(doc.value, "procedural_feedback_job_read"))
        .transpose()
}

pub(crate) fn read_scope_index(
    platform: &StorePlatform,
    key: &str,
) -> Result<Option<ProceduralFeedbackScopeIndexV1>> {
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
    current_job: &ProceduralFeedbackJobV1,
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
                    serde_json::from_value::<AgentToolExperienceRevisionMaterialV2>(value.clone())
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
        let expected_constraints = material
            .constraints
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
            || owner.procedural_content.procedure != material.usage_guidance.trim()
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
            let source: ProceduralFeedbackJobV1 = if binding.safe_ref == current_job.job_id {
                current_job.clone()
            } else {
                decode(
                    exact_value(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &binding.safe_ref)?,
                    stage,
                )?
            };
            source.validate()?;
            if source.job_id != binding.safe_ref
                || source.learning_evidence_digest != binding.source_digest
                || source.identity.memory_space_id != current_job.identity.memory_space_id
                || source.identity.mounted_subject_id != current_job.identity.mounted_subject_id
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
                let ledger: ProceduralFeedbackApplicationLedgerV1 = decode(
                    exact_value(
                        PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
                        &source.job_id,
                    )?,
                    stage,
                )?;
                ledger.validate()?;
                if ledger.identity != source.identity
                    || ledger.learning_evidence_digest != source.learning_evidence_digest
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
                    let retained: AgentToolExperienceRevisionMaterialV2 = decode(
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
                || evidence.authority
                    == bm_core::memory::ProceduralFeedbackAuthorityV1::ModelInferred
                || turn.external_content_used
            {
                return Err(Error::conflict(
                    stage,
                    "promotion source authority is denied",
                ));
            }
            let matching_method = evidence.agent_tool_feedback.iter().any(|feedback| {
                feedback.registry_ref.registry_id == material.registry_id
                    && feedback.registry_ref.scope == material.registry_scope
                    && feedback.tool_id == material.tool_id
                    && feedback.schema_fingerprint == material.schema_fingerprint
                    && feedback.observations.iter().any(|observation| {
                        observation.task_signature == material.task_signature
                            && observation.outcome == bm_core::skills::AgentToolOutcome::Succeeded
                            && !observation.private_content_used
                            && !observation.external_content
                            && observation.summary.trim() == owner.procedural_content.procedure
                            && material.evidence_refs.contains(&observation.observation_id)
                            && bm_core::skills::agent_tool_observation_matches_transcript(
                                observation,
                                &turn,
                            )
                    })
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
    ledger: &ProceduralFeedbackApplicationLedgerV1,
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
                    let material: AgentToolExperienceRevisionMaterialV2 =
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
    let mut jobs = BTreeMap::<String, ProceduralFeedbackJobV1>::new();
    let mut indexes = BTreeMap::<String, ProceduralFeedbackScopeIndexV1>::new();
    let mut ledgers = BTreeMap::<String, ProceduralFeedbackApplicationLedgerV1>::new();
    let mut experience_heads = BTreeMap::new();
    let mut experience_materials = BTreeMap::new();
    let mut runtime_owners = Vec::new();
    let mut mutation_receipts = Vec::new();
    let mut mutation_audits = BTreeMap::new();
    for ((namespace, key), value) in &state.json {
        match namespace.as_str() {
            PROCEDURAL_FEEDBACK_JOB_NAMESPACE => {
                let job: ProceduralFeedbackJobV1 = decode(value.clone(), stage)?;
                if job.job_id != *key || job.validate().is_err() {
                    return Err(Error::config(stage, "invalid procedural feedback job"));
                }
                jobs.insert(key.clone(), job);
            }
            PROCEDURAL_FEEDBACK_SCOPE_INDEX_NAMESPACE => {
                let index: ProceduralFeedbackScopeIndexV1 = decode(value.clone(), stage)?;
                if index.scope_index_key != *key || index.validate().is_err() {
                    return Err(Error::config(
                        stage,
                        "invalid procedural feedback scope index",
                    ));
                }
                indexes.insert(key.clone(), index);
            }
            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE => {
                let ledger: ProceduralFeedbackApplicationLedgerV1 = decode(value.clone(), stage)?;
                if ledger.job_id != *key || ledger.validate().is_err() {
                    return Err(Error::config(
                        stage,
                        "invalid procedural feedback application ledger",
                    ));
                }
                ledgers.insert(key.clone(), ledger);
            }
            AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE => {
                let head: AgentToolExperienceOwnerHeadV2 = decode(value.clone(), stage)?;
                experience_heads.insert(key.clone(), head);
            }
            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE => {
                let material: AgentToolExperienceRevisionMaterialV2 = decode(value.clone(), stage)?;
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
    let mut referenced = BTreeSet::new();
    for index in indexes.values() {
        for reference in index
            .active_jobs
            .iter()
            .chain(index.recent_terminal_jobs.iter())
        {
            let job = jobs.get(&reference.job_id).ok_or_else(|| {
                Error::config(stage, "procedural scope index has a dangling job reference")
            })?;
            if !referenced.insert(reference.job_id.clone())
                || job.scope_index_key != index.scope_index_key
                || job.identity.memory_space_id != index.memory_space_id
                || job.identity.mounted_subject_id != index.mounted_subject_id
                || job.identity.channel_id != index.channel_id
                || job.identity.chat_id != index.chat_id
                || ProceduralFeedbackJobRefV1::from_job(job) != *reference
            {
                return Err(Error::config(
                    stage,
                    "procedural job and scope index closure diverged",
                ));
            }
        }
    }
    if referenced.len() != jobs.len() {
        return Err(Error::config(
            stage,
            "procedural feedback store contains an orphan job",
        ));
    }
    for job in jobs.values() {
        let ledger = ledgers.get(&job.job_id);
        match (job.status, ledger) {
            (ProceduralFeedbackJobStatusV1::Succeeded, Some(ledger))
                if ledger.identity == job.identity
                    && ledger.learning_evidence_digest == job.learning_evidence_digest => {}
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
            .filter(|job| job.status == ProceduralFeedbackJobStatusV1::Succeeded)
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
            .expect("succeeded job receipt validated by canonical job contract");
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
        if mutation_audit.identity != mutation_receipt.identity
            || mutation_audit.intent_digest != mutation_receipt.intent_digest
            || mutation_audit.effect_plan_digest != mutation_receipt.effect_plan_digest
            || mutation_audit.transaction_id != mutation_receipt.transaction_id
            || mutation_audit.effect != mutation_receipt.effect
            || mutation_audit.changed_count != mutation_receipt.changed_count
            || mutation_audit.audit_record_id != mutation_receipt.audit_record_id
            || mutation_audit.committed_at_unix_secs != mutation_receipt.committed_at_unix_secs
        {
            return Err(Error::config(
                stage,
                "procedural mutation receipt and audit diverged",
            ));
        }
        let expected_identity = MemoryMutationOperationIdentity::new(
            &procedural_receipt.operation_id,
            &ledger.identity.memory_space_id,
            &ledger.identity.mounted_subject_id,
            mutation_receipt.identity.actor_subject_id(),
            MemoryMutationOperationKind::ProceduralLearning,
        )?;
        if mutation_receipt.intent_digest != procedural_receipt.plan_digest
            || mutation_receipt.transaction_id != procedural_receipt.transaction_id
            || mutation_receipt.identity.storage_key() != procedural_receipt.mutation_receipt_key
            || mutation_receipt.identity != expected_identity
            || mutation_receipt.identity.operation_kind()
                != MemoryMutationOperationKind::ProceduralLearning
            || mutation_receipt.identity.memory_space_id() != ledger.identity.memory_space_id
            || mutation_receipt.identity.mounted_subject_id() != ledger.identity.mounted_subject_id
        {
            return Err(Error::config(
                stage,
                "procedural feedback receipt differs from its authoritative mutation receipt",
            ));
        }
        let expected_post_image = digest_serialized(
            "procedural_feedback_post_image_digest_v1",
            &(
                &ledger.application_digest,
                &procedural_receipt.plan_digest,
                ProceduralFeedbackJobStatusV1::Succeeded,
            ),
            stage,
        )?;
        if procedural_receipt.post_image_digest != expected_post_image {
            return Err(Error::config(
                stage,
                "procedural application differs from its committed post-image proof",
            ));
        }
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
            if head.state == bm_core::skills::AgentToolExperienceHeadStateV2::Tombstoned {
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
        validate_runtime_promotion_store_sources(owner, state, &jobs, &ledgers, stage)?;
    }
    Ok(())
}

fn validate_runtime_promotion_store_sources(
    owner: &bm_core::skills::RuntimeSkillOwnerRecord,
    state: &BackendTransactionState,
    jobs: &BTreeMap<String, ProceduralFeedbackJobV1>,
    ledgers: &BTreeMap<String, ProceduralFeedbackApplicationLedgerV1>,
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
            || job.learning_evidence_digest != source_ref.source_digest
            || job.identity.memory_space_id != owner.memory_space_id
            || job.identity.mounted_subject_id != *mounted_subject_id
            || ledger.identity != job.identity
            || ledger.learning_evidence_digest != job.learning_evidence_digest
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
            job.identity.channel_id.clone(),
            job.identity.conversation_id.clone(),
            job.identity.turn_id.clone(),
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
            || evidence.authority == bm_core::memory::ProceduralFeedbackAuthorityV1::ModelInferred
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
        let material: AgentToolExperienceRevisionMaterialV2 = decode(
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
            feedback.registry_ref.registry_id == material.registry_id
                && feedback.registry_ref.scope == material.registry_scope
                && feedback.tool_id == material.tool_id
                && feedback.schema_fingerprint == material.schema_fingerprint
                && feedback.observations.iter().any(|observation| {
                    observation.task_signature == material.task_signature
                        && observation.outcome == bm_core::skills::AgentToolOutcome::Succeeded
                        && !observation.private_content_used
                        && !observation.external_content
                        && material.evidence_refs.contains(&observation.observation_id)
                        && bm_core::skills::agent_tool_observation_matches_transcript(
                            observation,
                            &turn,
                        )
                        && (owner.owner_revision != 1
                            || observation.summary.trim() == owner.procedural_content.procedure)
                })
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
    job: &ProceduralFeedbackJobV1,
    stage: &'static str,
) -> Result<ProceduralFeedbackScopeIndexV1> {
    read_scope_index(platform, &job.scope_index_key)?.ok_or_else(|| {
        Error::config(
            stage,
            "procedural feedback job is missing its exact scope index",
        )
    })
}

fn updated_active_index(
    before: &ProceduralFeedbackScopeIndexV1,
    before_job: &ProceduralFeedbackJobV1,
    after_job: &ProceduralFeedbackJobV1,
    now_secs: u64,
    stage: &'static str,
) -> Result<ProceduralFeedbackScopeIndexV1> {
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
    *reference = ProceduralFeedbackJobRefV1::from_job(after_job);
    sort_job_refs(&mut after.active_jobs);
    after.index_revision =
        checked_increment(after.index_revision, stage, "scope index revision overflow")?;
    after.updated_at = now_secs;
    after.validate()?;
    Ok(after)
}

fn moved_to_terminal_index(
    before: &ProceduralFeedbackScopeIndexV1,
    before_job: &ProceduralFeedbackJobV1,
    after_job: &ProceduralFeedbackJobV1,
    now_secs: u64,
    stage: &'static str,
) -> Result<ProceduralFeedbackScopeIndexV1> {
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
        .push(ProceduralFeedbackJobRefV1::from_job(after_job));
    sort_job_refs(&mut after.recent_terminal_jobs);
    after.index_revision =
        checked_increment(after.index_revision, stage, "scope index revision overflow")?;
    after.updated_at = now_secs;
    after.validate()?;
    Ok(after)
}

#[allow(clippy::too_many_arguments)]
fn commit_job_and_index(
    platform: &StorePlatform,
    scope: StoreEventScope,
    runtime_budget: &RuntimeBudgetReport,
    operation: &str,
    before_job: &ProceduralFeedbackJobV1,
    after_job: &ProceduralFeedbackJobV1,
    before_index: &ProceduralFeedbackScopeIndexV1,
    after_index: &ProceduralFeedbackScopeIndexV1,
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
            &before_job.scope_index_key,
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
                &after_job.scope_index_key,
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
    job: &ProceduralFeedbackJobV1,
    stage: &'static str,
) -> Result<()> {
    if scope.memory_space_id != job.identity.memory_space_id
        || scope.subject_id != job.identity.mounted_subject_id
        || scope.channel != job.identity.channel_id
        || scope.chat_id != job.identity.chat_id
    {
        return Err(Error::invalid_input(
            stage,
            "Store event scope differs from procedural job identity",
        ));
    }
    Ok(())
}

fn validate_active_lease(
    job: &ProceduralFeedbackJobV1,
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

fn same_intent(left: &ProceduralFeedbackJobV1, right: &ProceduralFeedbackJobV1) -> bool {
    left.identity == right.identity
        && left.transcript_sequence == right.transcript_sequence
        && left.transcript_digest == right.transcript_digest
        && left.learning_evidence_digest == right.learning_evidence_digest
        && left.submitted_count == right.submitted_count
}

fn sort_job_refs(values: &mut [ProceduralFeedbackJobRefV1]) {
    values.sort_by(|left, right| {
        left.transcript_sequence
            .cmp(&right.transcript_sequence)
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
        plane: "procedural_feedback".to_string(),
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

fn digest_serialized<T: Serialize>(domain: &str, value: &T, stage: &'static str) -> Result<String> {
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
    deferred_count: u32,
    rejected_count: u32,
    changed_count: u32,
) -> Result<String> {
    digest_serialized(
        "procedural_feedback_plan_digest_v1",
        &(
            job_id,
            leased_state_revision,
            experience_mutations,
            experience_preconditions,
            applied_owner_bindings,
            accepted_count,
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

    fn source_record(now_secs: u64) -> bm_core::memory::TranscriptTurnRecord {
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
                tool_name: "tool:test".into(),
                summary: "synthetic completed operation".into(),
                external_content: false,
            }],
            external_content_used: false,
            candidate_ids: vec![],
        };
        let mut evidence = PostTurnLearningEvidenceV1 {
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
            agent_tool_feedback: vec![AgentToolUsageFeedbackV2 {
                registry_ref: bm_core::skills::AgentToolRegistryRef {
                    registry_id: "registry:test".into(),
                    fingerprint: "registry-fingerprint:test".into(),
                    scope: bm_core::skills::AgentToolRegistryScope::Global,
                },
                tool_id: "tool:test".into(),
                schema_fingerprint: "schema:test".into(),
                observations: vec![bm_core::skills::AgentToolObservationDigest {
                    observation_id: "observation:test".into(),
                    registry_id: "registry:test".into(),
                    tool_id: "tool:test".into(),
                    schema_fingerprint: "schema:test".into(),
                    call_id: Some("call:test".into()),
                    task_signature: "task:test".into(),
                    summary: "synthetic completed operation".into(),
                    outcome: bm_core::skills::AgentToolOutcome::Succeeded,
                    error_code: None,
                    external_content: false,
                    private_content_used: false,
                    permission_tags: vec![],
                    risk_tags: vec![],
                    started_at: Some(now_secs),
                    completed_at: Some(now_secs),
                }],
                outcome: ProceduralExecutionOutcomeV1::Succeeded,
                user_visible_result_summary: None,
                operator_note: None,
            }],
            authority: ProceduralFeedbackAuthorityV1::HostRuntimeObservation,
            learning_evidence_digest: String::new(),
        };
        evidence.learning_evidence_digest = evidence.canonical_digest().expect("evidence digest");
        TranscriptTurnRecord::from_delta_with_learning_evidence(
            &ConversationKey::from_delta("space:test", &delta).expect("key"),
            1,
            &delta,
            vec![],
            Some(evidence),
            now_secs,
        )
        .expect("source record")
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
        let job = ProceduralFeedbackJobV1::pending(
            identity,
            1,
            format!("sha256:{}", "a".repeat(64)),
            format!("sha256:{}", "b".repeat(64)),
            1,
            3,
            100,
        )
        .unwrap();
        let empty = ProceduralFeedbackApplicationLedgerV1::build(&job, vec![], 101).unwrap();
        assert!(validate_new_applied_post_images(&empty, &[]).is_ok());
        let claimed = ProceduralFeedbackApplicationLedgerV1::build(
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
        let record = source_record(now_secs);
        platform.append_turn(&record).expect("canonical source");
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
        let record = source_record(now_secs);
        platform.append_turn(&record).expect("canonical source");
        let identity = ProceduralFeedbackIdentityV1::new(
            "space:test",
            "subject:test",
            "channel:test",
            "chat:test",
            "conversation:test",
            "turn:test",
        )
        .expect("identity");
        let job = ProceduralFeedbackJobV1::pending(
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
        let record = source_record(now_secs);
        platform.append_turn(&record).expect("canonical source");
        let identity = ProceduralFeedbackIdentityV1::new(
            "space:test",
            "subject:test",
            "channel:test",
            "chat:test",
            "conversation:test",
            "turn:test",
        )
        .expect("identity");
        let job = ProceduralFeedbackJobV1::pending(
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
        let input = ProceduralFeedbackCompletionInput {
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
            accepted_count: 0,
            deferred_count: 1,
            rejected_count: 0,
            changed_count: 0,
            reason_digest: format!("sha256:{}", "c".repeat(64)),
            completed_at: now_secs + 1,
        };
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
