//! Store verification of method facts at a reconciliation checkpoint. Registry
//! policy remains in SDK. A conservative SDK denial may remove positive support,
//! but cannot erase a still-valid negative task and then authorize Active.
use super::*;
use bm_core::memory::{
    MethodTaskContributionV1, MethodTaskOutcomeV1,
    ProceduralReconciliationCheckpointV1 as Checkpoint,
    ProceduralReconciliationDependencyPinV1 as Pin,
    ProceduralReconciliationReadDispositionV1 as Disposition,
};
use bm_core::skills::{AgentToolExperienceBodyV1 as Body, AgentToolExperienceStatus as Status};

const STAGE: &str = "method_reconciliation_proof";
fn invalid(message: &str) -> Error {
    Error::conflict(STAGE, message)
}

pub(super) fn validate_checkpoint(
    scope: &ProceduralSubjectScopeV1,
    checkpoint: &Checkpoint,
    mut read: impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<()> {
    for proof in &checkpoint.verified_owners {
        let ProceduralAppliedOwnerBindingV1::AgentToolExperience {
            owner_revision,
            content_digest,
        } = &proof.binding
        else {
            continue;
        };
        if proof.disposition != Disposition::Visible && proof.withdrawn_task_refs.is_empty() {
            continue;
        }
        let owning_scope = bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
            mounted_subject_id: scope.mounted_subject_id.clone(),
        };
        let key = bm_core::skills::agent_tool_experience_material_key(
            &scope.memory_space_id,
            &owning_scope,
            &owner_revision.owner_ref,
            owner_revision.owner_revision,
        )?;
        let material: AgentToolExperienceRevisionMaterialV4 =
            decode(read(AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE, &key)?, STAGE)?;
        if !material.validate_contract().accepted
            || material.physical_key != key
            || material.memory_space_id != scope.memory_space_id
            || material.owning_scope != owning_scope
            || material.owner_revision_ref() != *owner_revision
            || material.content_digest != *content_digest
        {
            return Err(invalid("checkpoint does not own its exact method material"));
        }
        if !matches!(material.body, Body::Method { .. }) {
            if !proof.withdrawn_task_refs.is_empty() {
                return Err(invalid("task withdrawals require an exact method material"));
            }
            continue;
        }
        if !proof.withdrawn_task_refs.is_empty() {
            let actual = withdrawn_task_refs(scope, &material, checkpoint, &mut read)?;
            if proof
                .withdrawn_task_refs
                .iter()
                .any(|reference| !actual.contains(reference))
            {
                return Err(invalid(
                    "checkpoint erases a task whose factual source remains valid",
                ));
            }
        }
        if proof.disposition != Disposition::Visible {
            continue;
        }
        // These states cannot be used for trial, normal recall or promotion.
        // Keeping an unavailable method conservative is always safe.
        if matches!(material.status, Status::Stale | Status::Rejected) {
            continue;
        }
        if material.status == Status::Active
            && material.body.method_usage_contributions().is_empty()
        {
            return Err(invalid("Active requires real task application witnesses"));
        }
        let head_key = bm_core::skills::agent_tool_experience_head_key(
            &scope.memory_space_id,
            &owning_scope,
            &material.owner_ref,
        )?;
        let head: AgentToolExperienceOwnerHeadV3 = decode(
            read(AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE, &head_key)?,
            STAGE,
        )?;
        if !head.validate_contract().accepted
            || head.owner_ref != material.owner_ref
            || head.memory_space_id != scope.memory_space_id
            || head.owning_scope != owning_scope
            || head.state != bm_core::skills::AgentToolExperienceHeadStateV3::Active
        {
            return Err(invalid("visible method lacks an active exact owner head"));
        }
        let mut refs = BTreeMap::new();
        let digest = material
            .body
            .method_content_digest()?
            .ok_or_else(|| invalid("method identity missing"))?;
        let mut previous_status = None;
        for retained in &head.retained_revisions {
            if retained.owner_revision > material.owner_revision {
                continue;
            }
            let historic: AgentToolExperienceRevisionMaterialV4 =
                if retained.material_key == material.physical_key {
                    material.clone()
                } else {
                    decode(
                        read(
                            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                            &retained.material_key,
                        )?,
                        STAGE,
                    )?
                };
            if AgentToolExperienceRetainedRevisionDigestV3::from_material(&historic)? != *retained
                || historic.owner_ref != material.owner_ref
                || historic.memory_space_id != material.memory_space_id
                || historic.owning_scope != material.owning_scope
            {
                return Err(invalid("method history differs from retained commitment"));
            }
            if retained.owner_revision.checked_add(1) == Some(material.owner_revision) {
                previous_status = Some(historic.status);
            }
            // The current recommendation must account for all retained facts.
            // Historical projections are checked against their own exact refs.
            if (head.current_revision == material.owner_revision
                || historic.owner_revision == material.owner_revision)
                && historic.body.method_content_digest()?.as_deref() == Some(digest.as_str())
            {
                for reference in historic.body.method_usage_contributions() {
                    if refs
                        .insert(reference.contribution_id.clone(), reference.clone())
                        .is_some_and(|old| old != *reference)
                    {
                        return Err(invalid("retained task identity conflicts"));
                    }
                }
            }
        }
        let mut retained_refs = Vec::new();
        let mut tasks = Vec::new();
        let mut executions = BTreeMap::new();
        for reference in refs.values() {
            let (ledger, job, task) = read_task_witness(scope, &material, reference, &mut read)?;
            if !task_is_retained(&task, &ledger, &job, &checkpoint.dependencies, &mut read)? {
                continue;
            }
            retained_refs.push(reference.clone());
            tasks.push(task.clone());
            for execution_ref in &task.execution_refs {
                let execution = ledger
                    .execution_contributions
                    .iter()
                    .find(|fact| fact.contribution_id == execution_ref.contribution_id)
                    .ok_or_else(|| invalid("execution witness missing"))?;
                if execution
                    .reference()
                    .map_err(|_| invalid("execution witness invalid"))?
                    != *execution_ref
                {
                    return Err(invalid("execution witness differs"));
                }
                executions.insert(execution.contribution_id.clone(), execution.clone());
            }
        }
        let mut expected_body = material.body.clone();
        if let Body::Method {
            usage_contributions,
            ..
        } = &mut expected_body
        {
            *usage_contributions = retained_refs;
        }
        let expected_status = if let Some(first) = tasks.first() {
            let tool = bm_core::memory::ProceduralProducerToolV1 {
                registry_ref: first.adoption.method.registry_ref.clone(),
                tool_id: material.tool_id.clone(),
                schema_fingerprint: material.schema_fingerprint.clone(),
            };
            let executions = executions.into_values().collect::<Vec<_>>();
            let (body, status) = bm_core::memory::reduce_agent_tool_method_task_contributions(
                scope,
                &tool,
                &expected_body,
                &tasks,
                &executions,
                previous_status,
                expected_body
                    .contribution_count()
                    .max(tasks.len())
                    .max(executions.len()),
            )
            .map_err(|_| invalid("actual task reduction failed"))?;
            expected_body = body;
            status
        } else {
            match previous_status {
                Some(status @ (Status::Stale | Status::Rejected)) => status,
                _ => Status::Candidate,
            }
        };
        if expected_body != material.body || expected_status != material.status {
            return Err(invalid(
                "checkpoint discarded valid evidence or overstated method status",
            ));
        }
    }
    Ok(())
}

fn read_task_witness(
    scope: &ProceduralSubjectScopeV1,
    material: &AgentToolExperienceRevisionMaterialV4,
    reference: &bm_core::memory::MethodTaskContributionRefV1,
    read: &mut impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<(
    ProceduralFeedbackApplicationLedgerV3,
    ProceduralFeedbackJobV2,
    MethodTaskContributionV1,
)> {
    let ledger: ProceduralFeedbackApplicationLedgerV3 = decode(
        read(
            PROCEDURAL_FEEDBACK_APPLICATION_LEDGER_NAMESPACE,
            &reference.source_job_id,
        )?,
        STAGE,
    )?;
    let job: ProceduralFeedbackJobV2 = decode(
        read(PROCEDURAL_FEEDBACK_JOB_NAMESPACE, &reference.source_job_id)?,
        STAGE,
    )?;
    let receipt_key = job
        .receipt
        .as_ref()
        .ok_or_else(|| invalid("task source receipt missing"))?
        .mutation_receipt_key();
    let receipt: MemoryMutationReceipt = decode(
        read(
            bm_core::memory::MEMORY_MUTATION_RECEIPT_NAMESPACE,
            receipt_key,
        )?,
        STAGE,
    )?;
    let audit: MemoryMutationAuditRecord = decode(
        read(
            bm_core::memory::MEMORY_MUTATION_AUDIT_NAMESPACE,
            receipt_key,
        )?,
        STAGE,
    )?;
    validate_feedback_application_completion(&job, &ledger, &receipt, &audit)?;
    if ledger.identity.memory_space_id != scope.memory_space_id
        || ledger.identity.mounted_subject_id != scope.mounted_subject_id
    {
        return Err(invalid("task source crosses subject scope"));
    }
    let task = ledger
        .method_task_contributions
        .iter()
        .find(|task| task.contribution_id == reference.contribution_id)
        .ok_or_else(|| invalid("retained task witness missing"))?
        .clone();
    if task.reference()? != *reference
        || task.adoption.method.owner_revision.owner_ref != material.owner_ref
        || Some(task.method_content_digest.as_str())
            != material.body.method_content_digest()?.as_deref()
    {
        return Err(invalid("task witness differs from method"));
    }
    validate_method_ledger_witnesses(material, &ledger, &job, read)?;
    Ok((ledger, job, task))
}

fn withdrawn_task_refs(
    scope: &ProceduralSubjectScopeV1,
    material: &AgentToolExperienceRevisionMaterialV4,
    checkpoint: &Checkpoint,
    read: &mut impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<Vec<bm_core::memory::MethodTaskContributionRefV1>> {
    if material.memory_space_id != scope.memory_space_id
        || material.owning_scope.mounted_subject_id() != scope.mounted_subject_id
        || !material.validate_contract().accepted
    {
        return Err(invalid(
            "withdrawal material crosses scope or is noncanonical",
        ));
    }
    let retained = retained_task_refs(
        scope,
        material,
        material.body.method_usage_contributions(),
        &checkpoint.dependencies,
        read,
    )?;
    Ok(material
        .body
        .method_usage_contributions()
        .iter()
        .filter(|reference| !retained.contains(reference))
        .cloned()
        .collect())
}

pub(crate) fn reconciliation_withdrawn_task_refs(
    platform: &StorePlatform,
    scope: &ProceduralSubjectScopeV1,
    material: &AgentToolExperienceRevisionMaterialV4,
    checkpoint: &Checkpoint,
    existing: &[StoreJsonPrecondition],
    budget: &RuntimeBudgetReport,
) -> Result<(
    Vec<bm_core::memory::MethodTaskContributionRefV1>,
    Vec<StoreJsonPrecondition>,
)> {
    let mut plan = source_dependents::ReadPlan::for_conditions(
        platform,
        existing,
        super::super::StoreCapacityBudget::from_runtime_budget(budget.store_budget),
    );
    let original = plan.before(
        AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
        &material.physical_key,
    )?;
    if original
        != Some(
            serde_json::to_value(material)
                .map_err(|_| invalid("withdrawal material encoding failed"))?,
        )
    {
        return Err(invalid(
            "withdrawal material differs from the exact read set",
        ));
    }
    let references = withdrawn_task_refs(scope, material, checkpoint, &mut |namespace, key| {
        plan.before(namespace, key)?
            .ok_or_else(|| invalid(&format!("withdrawal dependency missing: {namespace}/{key}")))
    })?;
    let conditions = plan
        .observed
        .into_iter()
        .map(|((namespace, key), value)| match value {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace,
                key,
                value,
            },
            None => StoreJsonPrecondition::Absent { namespace, key },
        })
        .collect();
    Ok((references, conditions))
}

fn retained_task_refs(
    scope: &ProceduralSubjectScopeV1,
    material: &AgentToolExperienceRevisionMaterialV4,
    references: &[bm_core::memory::MethodTaskContributionRefV1],
    dependencies: &[Pin],
    read: &mut impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<Vec<bm_core::memory::MethodTaskContributionRefV1>> {
    let mut retained = Vec::new();
    for reference in references {
        let (ledger, job, task) = read_task_witness(scope, material, reference, read)?;
        if task_is_retained(&task, &ledger, &job, dependencies, read)? {
            retained.push(reference.clone());
        }
    }
    Ok(retained)
}

/// The SDK planner and locked checkpoint reducer share the same task authority
/// decision. A historical union must originate from actual retained materials,
/// not merely from a task present somewhere in an intake ledger.
pub(crate) fn reconciliation_retained_task_refs(
    platform: &StorePlatform,
    material: &AgentToolExperienceRevisionMaterialV4,
    applied_body: &Body,
    dependencies: &[Pin],
    existing: &[StoreJsonPrecondition],
    budget: &RuntimeBudgetReport,
) -> Result<(
    Vec<bm_core::memory::MethodTaskContributionRefV1>,
    Vec<StoreJsonPrecondition>,
)> {
    if applied_body.method_usage_contributions().is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    if !material.validate_contract().accepted
        || material.body.method_content_digest()? != applied_body.method_content_digest()?
    {
        return Err(invalid("retained task basis differs from its method"));
    }
    let scope = ProceduralSubjectScopeV1 {
        memory_space_id: material.memory_space_id.clone(),
        mounted_subject_id: material.owning_scope.mounted_subject_id().into(),
    };
    let mut plan = source_dependents::ReadPlan::for_conditions(
        platform,
        existing,
        super::super::StoreCapacityBudget::from_runtime_budget(budget.store_budget),
    );
    let mut read = |namespace: &str, key: &str| {
        plan.before(namespace, key)?.ok_or_else(|| {
            invalid(&format!(
                "retained task dependency missing: {namespace}/{key}"
            ))
        })
    };
    let actual: AgentToolExperienceRevisionMaterialV4 = decode(
        read(
            AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
            &material.physical_key,
        )?,
        STAGE,
    )?;
    if actual != *material {
        return Err(invalid("retained task material differs from exact source"));
    }
    let head: AgentToolExperienceOwnerHeadV3 = decode(
        read(
            AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE,
            &bm_core::skills::agent_tool_experience_head_key(
                &scope.memory_space_id,
                &material.owning_scope,
                &material.owner_ref,
            )?,
        )?,
        STAGE,
    )?;
    if !head.validate_contract().accepted
        || head.memory_space_id != material.memory_space_id
        || head.owning_scope != material.owning_scope
        || head.owner_ref != material.owner_ref
    {
        return Err(invalid("retained task head differs from exact source"));
    }
    let mut known = BTreeMap::new();
    for retained in &head.retained_revisions {
        if retained.owner_revision > material.owner_revision {
            continue;
        }
        let historical: AgentToolExperienceRevisionMaterialV4 = decode(
            read(
                AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE,
                &retained.material_key,
            )?,
            STAGE,
        )?;
        if AgentToolExperienceRetainedRevisionDigestV3::from_material(&historical)? != *retained
            || historical.owner_ref != material.owner_ref
            || historical.memory_space_id != material.memory_space_id
            || historical.owning_scope != material.owning_scope
        {
            return Err(invalid(
                "retained task history differs from immutable commitment",
            ));
        }
        if historical.body.method_content_digest()? != material.body.method_content_digest()? {
            continue;
        }
        for reference in historical.body.method_usage_contributions() {
            if known
                .insert(reference.contribution_id.clone(), reference.clone())
                .is_some_and(|old| old != *reference)
            {
                return Err(invalid("historical task identity conflicts"));
            }
        }
    }
    if applied_body
        .method_usage_contributions()
        .iter()
        .any(|reference| known.get(&reference.contribution_id) != Some(reference))
    {
        return Err(invalid(
            "task basis invents an unapplied historical contribution",
        ));
    }
    let retained = retained_task_refs(
        &scope,
        material,
        applied_body.method_usage_contributions(),
        dependencies,
        &mut read,
    )?;
    let conditions = plan
        .observed
        .into_iter()
        .map(|((namespace, key), value)| match value {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace,
                key,
                value,
            },
            None => StoreJsonPrecondition::Absent { namespace, key },
        })
        .collect();
    Ok((retained, conditions))
}

fn task_is_retained(
    task: &MethodTaskContributionV1,
    ledger: &ProceduralFeedbackApplicationLedgerV3,
    job: &ProceduralFeedbackJobV2,
    dependencies: &[Pin],
    read: &mut impl FnMut(&str, &str) -> Result<serde_json::Value>,
) -> Result<bool> {
    let fence = TranscriptLearningSource::procedural(job)?;
    let value = read("conversation_transcript", &fence.storage_key())?;
    let turn: bm_core::memory::TranscriptTurnRecord = decode(value.clone(), STAGE)?;
    let transcript_pin = dependencies
        .iter()
        .find(|pin| matches!(pin, Pin::Transcript { identity, .. } if identity == &ledger.identity))
        .ok_or_else(|| invalid("task source has no exact transcript pin"))?;
    if !matches!(transcript_pin, Pin::Transcript { sequence, content_digest, lifecycle, .. }
        if *sequence == turn.sequence && *lifecycle == turn.lifecycle_state && *content_digest == bm_core::memory::procedural_transcript_state_digest(&turn)?)
    {
        return Err(invalid("task transcript pin changed"));
    }
    if !turn.permits_post_turn_learning() {
        return Ok(false);
    }
    fence.validate_record(&value)?;
    let evidence = turn
        .learning_evidence
        .as_ref()
        .ok_or_else(|| invalid("task source evidence missing"))?;
    let producer = read_verified_procedural_producer_with(&task.producer, &mut *read)?;
    let producer_pin = dependencies.iter().find(|pin| matches!(pin, Pin::Producer { original_revision, .. } if original_revision == &task.producer))
        .ok_or_else(|| invalid("task source has no exact producer pin"))?;
    let permitted = match producer_pin {
        Pin::Producer {
            current_revision,
            permitted,
            ..
        } if *current_revision == producer.head.current => *permitted,
        _ => return Err(invalid("task producer pin changed")),
    };
    match producer
        .historical
        .authorize_task_with_current(&producer.current, evidence, task, job)
    {
        Ok(()) => {}
        Err(
            bm_core::memory::ProceduralProducerErrorV1::ClaimForbidden
            | bm_core::memory::ProceduralProducerErrorV1::ToolForbidden
            | bm_core::memory::ProceduralProducerErrorV1::SourceClassificationForbidden
            | bm_core::memory::ProceduralProducerErrorV1::Revoked,
        ) => return Ok(false),
        Err(_) => {
            return Err(invalid(
                "task authority proof is invalid, not a lawful withdrawal",
            ))
        }
    }
    if let bm_core::memory::ProceduralProducerSourceAuthorityV1::GovernedSource {
        source_revision,
    } = &producer.historical.spec.source_authority
    {
        let root: bm_core::memory::ProceduralSourceDependentsRootV1 = decode(
            read(
                super::super::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE,
                &bm_core::memory::ProceduralSourceDependentsRootV1::key(
                    &ledger.identity.memory_space_id,
                    &source_revision.owner_ref,
                )?,
            )?,
            STAGE,
        )?;
        root.validate()?;
        if !dependencies.iter().any(|pin| matches!(pin, Pin::Source { owner_ref, post_image, owner_state_digest }
            if *owner_ref == root.owner_ref && *post_image == root.current && *owner_state_digest == root.content_digest)) {
            return Err(invalid("governed task source has no exact current pin"));
        }
        if !matches!(
            root.current,
            bm_core::memory::ProceduralSourcePostImageV1::Present { .. }
        ) {
            return Ok(false);
        }
    }
    // A configuration-only denial never supplies evidence that a real failed
    // task stopped happening. Keep its negative effect until its factual source
    // or actual producer/evaluator authority has been withdrawn.
    Ok(permitted
        || matches!(
            task.evaluation.outcome,
            MethodTaskOutcomeV1::Failed | MethodTaskOutcomeV1::Partial
        ))
}

pub(crate) fn preconditions(
    platform: &StorePlatform,
    scope: &ProceduralSubjectScopeV1,
    checkpoint: &Checkpoint,
    mutations: &[StoreMutation],
    existing: &[StoreJsonPrecondition],
    budget: &RuntimeBudgetReport,
) -> Result<Vec<StoreJsonPrecondition>> {
    let mut plan = source_dependents::ReadPlan::for_conditions(
        platform,
        existing,
        super::super::StoreCapacityBudget::from_runtime_budget(budget.store_budget),
    );
    validate_checkpoint(scope, checkpoint, |namespace, key| {
        if let Some(value) = mutations.iter().find_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace: ns,
                key: k,
                value,
                ..
            } if ns == namespace && k == key => Some(value.clone()),
            _ => None,
        }) {
            return Ok(value);
        }
        plan.before(namespace, key)?
            .ok_or_else(|| invalid(&format!("checkpoint dependency missing: {namespace}/{key}")))
    })?;
    Ok(plan
        .observed
        .into_iter()
        .map(|((namespace, key), value)| match value {
            Some(value) => StoreJsonPrecondition::Exact {
                namespace,
                key,
                value,
            },
            None => StoreJsonPrecondition::Absent { namespace, key },
        })
        .collect())
}

pub(super) fn dependencies(job: &ProceduralFeedbackJobV2) -> Result<Vec<(String, String)>> {
    let Some(checkpoint) = &job.checkpoint else {
        return Ok(Vec::new());
    };
    let scope = job.work.subject_scope();
    let owning = bm_core::skills::AgentToolExperienceOwningScopeV1::Subject {
        mounted_subject_id: scope.mounted_subject_id.clone(),
    };
    let mut addresses = Vec::new();
    for proof in &checkpoint.verified_owners {
        if let ProceduralAppliedOwnerBindingV1::AgentToolExperience { owner_revision, .. } =
            &proof.binding
        {
            addresses.push((
                AGENT_TOOL_EXPERIENCE_HEAD_NAMESPACE.into(),
                bm_core::skills::agent_tool_experience_head_key(
                    &scope.memory_space_id,
                    &owning,
                    &owner_revision.owner_ref,
                )?,
            ));
            if proof.disposition == Disposition::Visible || !proof.withdrawn_task_refs.is_empty() {
                addresses.push((
                    AGENT_TOOL_EXPERIENCE_MATERIAL_NAMESPACE.into(),
                    bm_core::skills::agent_tool_experience_material_key(
                        &scope.memory_space_id,
                        &owning,
                        &owner_revision.owner_ref,
                        owner_revision.owner_revision,
                    )?,
                ));
            }
        }
    }
    for pin in &checkpoint.dependencies {
        match pin {
            Pin::Producer {
                original_revision, ..
            } => {
                addresses.push((
                    super::super::schema::PROCEDURAL_PRODUCER_HEAD_NAMESPACE.into(),
                    original_revision.binding_key.clone(),
                ));
                addresses.push((
                    super::super::schema::PROCEDURAL_PRODUCER_BINDING_NAMESPACE.into(),
                    original_revision.material_key(),
                ));
            }
            Pin::Source { owner_ref, .. } => addresses.push((
                super::super::schema::PROCEDURAL_SOURCE_DEPENDENTS_NAMESPACE.into(),
                bm_core::memory::ProceduralSourceDependentsRootV1::key(
                    &scope.memory_space_id,
                    owner_ref,
                )?,
            )),
            Pin::Transcript { identity, .. } => {
                let key = bm_core::memory::ConversationKey::new(
                    &identity.memory_space_id,
                    &identity.channel_id,
                    &identity.conversation_id,
                )?;
                addresses.push((
                    "conversation_transcript".into(),
                    super::super::transcript_turn_storage_key(
                        &key,
                        &identity.mounted_subject_id,
                        &identity.turn_id,
                    ),
                ));
            }
            Pin::SemanticTranscript { identity, .. } => {
                let key = bm_core::memory::ConversationKey::new(
                    &identity.memory_space_id,
                    &identity.channel_id,
                    &identity.conversation_id,
                )?;
                addresses.push((
                    "conversation_transcript".into(),
                    super::super::transcript_turn_storage_key(
                        &key,
                        &identity.mounted_subject_id,
                        &identity.turn_id,
                    ),
                ));
            }
        }
    }
    addresses.sort();
    addresses.dedup();
    Ok(addresses)
}
