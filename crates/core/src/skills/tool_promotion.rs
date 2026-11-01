//! Pure admission of observed tool methods into the existing RuntimeSkill owner.

use std::collections::BTreeMap;

use crate::memory::{
    post_turn_governance_transcript_digest, MemoryPrivacyClass, MethodTaskContributionV1,
    ProceduralFeedbackJobStatusV1, ProceduralFeedbackJobV2, ProceduralProducerToolV1,
    ToolExecutionFactV1, TranscriptTurnRecord,
};
use crate::{Error, Result};

use super::*;

pub struct RuntimeSkillToolPromotionSource<'a> {
    pub job: &'a ProceduralFeedbackJobV2,
    pub transcript: &'a TranscriptTurnRecord,
    /// Exact immutable revisions actually adopted in this source turn.
    pub adopted_materials: &'a [AgentToolExperienceRevisionMaterialV4],
}

pub struct RuntimeSkillToolPromotionInput<'a> {
    pub experience: &'a AgentToolExperienceRevisionMaterialV4,
    pub registry: &'a AgentToolRegistrySnapshot,
    pub sources: &'a [RuntimeSkillToolPromotionSource<'a>],
    pub existing_owner: Option<&'a RuntimeSkillOwnerRecord>,
    pub observed_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSkillToolPromotionReason {
    WeakProcedure,
    InsufficientIndependentTasks,
    ContradictoryTaskEvidence,
    SourceUnavailable,
    SourceMismatch,
    AuthorityDenied,
    PrivacyDenied,
    RawPayload,
    SourceTimeInvalid,
}

/// The learning payload must refer to an observation in the canonical turn,
/// rather than vouch for its own execution by repeating an arbitrary identifier.
pub fn agent_tool_execution_matches_transcript(
    observation: &ToolExecutionFactV1,
    tool_id: &str,
    transcript: &TranscriptTurnRecord,
) -> bool {
    observation.validates_canonical_call(
        tool_id,
        &transcript.tool_observations,
        transcript.created_at,
    )
}

/// Rebuild task witnesses from canonical source records and exact adopted
/// revisions. The caller authenticates current producer/evaluator authority and
/// source visibility. Authorship declarations and tool success are not usage.
pub fn agent_tool_method_task_contributions(
    material: &AgentToolExperienceRevisionMaterialV4,
    job: &ProceduralFeedbackJobV2,
    turn: &TranscriptTurnRecord,
    adopted_materials: &[AgentToolExperienceRevisionMaterialV4],
) -> std::result::Result<Vec<MethodTaskContributionV1>, RuntimeSkillToolPromotionReason> {
    use RuntimeSkillToolPromotionReason as Reason;
    let references = material.body.method_usage_contributions();
    let relevant = references
        .iter()
        .filter(|reference| reference.source_job_id == job.job_id)
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        return Ok(Vec::new());
    }
    let evidence = turn
        .learning_evidence
        .as_ref()
        .ok_or(Reason::SourceMismatch)?;
    if evidence.authority.is_model_inferred() {
        return Err(Reason::AuthorityDenied);
    }
    if turn.external_content_used {
        return Err(Reason::PrivacyDenied);
    }
    let mut rebuilt = Vec::new();
    for (evaluation_ordinal, evaluation) in evidence.method_task_evaluations.iter().enumerate() {
        if !relevant
            .iter()
            .any(|reference| reference.method == evaluation.method)
        {
            continue;
        }
        let adoptions = evidence
            .method_adoptions
            .iter()
            .enumerate()
            .filter(|(_, adoption)| {
                adoption.method == evaluation.method && adoption.task_id == evaluation.task_id
            })
            .collect::<Vec<_>>();
        let materials = adopted_materials
            .iter()
            .filter(|candidate| {
                candidate.owner_revision_ref() == evaluation.method.owner_revision
                    && candidate.content_digest == evaluation.method.content_digest
                    && candidate.memory_space_id == evaluation.method.memory_space_id
                    && candidate.owning_scope.mounted_subject_id()
                        == evaluation.method.mounted_subject_id
            })
            .collect::<Vec<_>>();
        if adoptions.len() != 1 || materials.len() != 1 {
            return Err(Reason::SourceMismatch);
        }
        rebuilt.push(
            MethodTaskContributionV1::build(
                job,
                turn,
                adoptions[0].0 as u32,
                evaluation_ordinal as u32,
                materials[0],
            )
            .map_err(|_| Reason::SourceMismatch)?,
        );
    }
    let mut result = Vec::new();
    for reference in relevant {
        let contribution = rebuilt
            .iter()
            .find(|contribution| contribution.contribution_id == reference.contribution_id)
            .ok_or(Reason::SourceMismatch)?;
        if contribution
            .reference()
            .map_err(|_| Reason::SourceMismatch)?
            != *reference
            || contribution.observed_at > material.updated_at
            || contribution.adoption.method.owner_revision.owner_ref != material.owner_ref
            || contribution.adoption.method.owner_revision.owner_revision >= material.owner_revision
            || Some(contribution.method_content_digest.as_str())
                != material
                    .body
                    .method_content_digest()
                    .map_err(|_| Reason::SourceMismatch)?
                    .as_deref()
        {
            return Err(Reason::SourceMismatch);
        }
        result.push(contribution.clone());
    }
    Ok(result)
}

pub fn runtime_skill_applicability_for_tool_scope(
    scope: &AgentToolRegistryScope,
) -> Result<RuntimeSkillApplicability> {
    Ok(match scope {
        AgentToolRegistryScope::Global | AgentToolRegistryScope::Owner => {
            RuntimeSkillApplicability::Global
        }
        AgentToolRegistryScope::Project { project_id } => {
            RuntimeSkillApplicability::try_all_of(vec![RuntimeSkillApplicabilityTarget::Project {
                project_id: project_id.clone(),
            }])?
        }
        AgentToolRegistryScope::Workspace { workspace_id } => {
            RuntimeSkillApplicability::try_all_of(vec![
                RuntimeSkillApplicabilityTarget::Workspace {
                    workspace_id: workspace_id.clone(),
                },
            ])?
        }
        AgentToolRegistryScope::Conversation { conversation_id } => {
            RuntimeSkillApplicability::try_all_of(vec![
                RuntimeSkillApplicabilityTarget::Conversation {
                    conversation_id: conversation_id.clone(),
                },
            ])?
        }
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeSkillToolPromotionDecision {
    Promote {
        record: Box<RuntimeSkillOwnerRecord>,
        source_job_ids: Vec<String>,
    },
    Observe {
        reason: RuntimeSkillToolPromotionReason,
    },
    Reject {
        reason: RuntimeSkillToolPromotionReason,
    },
    AlreadyPromoted {
        binding: RuntimeSkillOwnerBinding,
    },
}

pub fn plan_runtime_skill_promotion_from_tool_evidence(
    input: &RuntimeSkillToolPromotionInput<'_>,
) -> Result<RuntimeSkillToolPromotionDecision> {
    use RuntimeSkillToolPromotionReason as Reason;
    let reject = |reason| Ok(RuntimeSkillToolPromotionDecision::Reject { reason });
    let observe = |reason| Ok(RuntimeSkillToolPromotionDecision::Observe { reason });
    let material = input.experience;
    let AgentToolExperienceBodyV1::Method {
        task_signature,
        procedure,
        constraints,
        ..
    } = &material.body
    else {
        return observe(Reason::WeakProcedure);
    };
    if input.observed_at < material.updated_at || input.observed_at == 0 {
        return reject(Reason::SourceTimeInvalid);
    }
    if !material.validate_contract().accepted
        || material.status != AgentToolExperienceStatus::Active
        || material.registry_id != input.registry.registry_id
        || material.registry_scope != input.registry.scope
        || input.registry.fingerprint != fingerprint_agent_tool_registry(input.registry)
        || !input.registry.tools.iter().any(|tool| {
            tool.tool_id == material.tool_id
                && tool.schema_fingerprint == material.schema_fingerprint
                && !tool.disabled
        })
    {
        return reject(Reason::SourceMismatch);
    }
    if !material.privacy_class.projection_content_allowed() {
        return reject(Reason::PrivacyDenied);
    }
    let AgentToolExperienceOwningScopeV1::Subject { mounted_subject_id } = &material.owning_scope;
    let scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: mounted_subject_id.clone(),
    };
    let creation_ref = RuntimeSkillCreationRef::AgentToolExperiencePromotion {
        experience_owner_ref: material.owner_ref.clone(),
    };
    let owner_id =
        canonical_runtime_skill_owner_id(&material.memory_space_id, &scope, &creation_ref)?;
    if let Some(existing) = input.existing_owner {
        if !existing.validate_contract().accepted
            || existing.memory_space_id != material.memory_space_id
            || existing.owning_scope != scope
            || existing.creation_ref != creation_ref
            || existing.owner_ref.owner_id != owner_id
        {
            return reject(Reason::SourceMismatch);
        }
        return Ok(RuntimeSkillToolPromotionDecision::AlreadyPromoted {
            binding: RuntimeSkillOwnerBinding::from_record(existing)?,
        });
    }
    let method = procedure.as_str();
    let title = method
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim();
    let title = crate::util::truncate_content_to_max(title, 160)
        .trim()
        .to_string();
    let write = RuntimeSkillWrite {
        name: runtime_skill_name_for_topic(task_signature),
        topic: task_signature.clone(),
        title: title.clone(),
        summary: title.clone(),
        content: method.to_string(),
        citations: vec![],
        source_chat_id: None,
        observed_at: input.observed_at,
    };
    let shape =
        govern_runtime_skill_write_shapes(&[write], RuntimeSkillWriteSource::ProgrammableReasoning);
    if shape.accepted != 1 {
        return if shape
            .reports
            .iter()
            .any(|item| item.reason == RuntimeSkillWriteReason::RawPayloadOrLog)
        {
            reject(Reason::RawPayload)
        } else {
            observe(Reason::WeakProcedure)
        };
    }
    let mut tasks = BTreeMap::new();
    let mut execution = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for source in input.sources {
        let job = source.job;
        let turn = source.transcript;
        job.validate()?;
        turn.validate_canonical_intake()?;
        if input.observed_at < job.updated_at || input.observed_at < turn.updated_at {
            return reject(Reason::SourceTimeInvalid);
        }
        if !matches!(
            job.status,
            ProceduralFeedbackJobStatusV1::Leased | ProceduralFeedbackJobStatusV1::Succeeded
        ) || !turn.permits_post_turn_learning()
        {
            return reject(Reason::SourceUnavailable);
        }
        let Some(evidence) = turn.learning_evidence.as_ref() else {
            return reject(Reason::SourceMismatch);
        };
        if matches!(&material.registry_scope, AgentToolRegistryScope::Conversation { conversation_id } if conversation_id != &turn.key.conversation_id)
            || evidence.selection_receipt.as_ref().is_some_and(|receipt| {
                !receipt
                    .applicability
                    .permits_registry_scope(&material.registry_scope)
            })
        {
            return reject(Reason::SourceMismatch);
        }
        if !evidence.validate_contract()
            || job.feedback_source()?.identity.memory_space_id != material.memory_space_id
            || job.feedback_source()?.identity.mounted_subject_id != *mounted_subject_id
            || turn.key.memory_space_id != job.feedback_source()?.identity.memory_space_id
            || turn.key.channel_id != job.feedback_source()?.identity.channel_id
            || turn.key.conversation_id != job.feedback_source()?.identity.conversation_id
            || turn.subject != job.feedback_source()?.identity.mounted_subject_id
            || turn.turn_id != job.feedback_source()?.identity.turn_id
            || turn.sequence != job.feedback_source()?.transcript_sequence
            || post_turn_governance_transcript_digest(turn)?
                != job.feedback_source()?.transcript_digest
            || evidence.learning_evidence_digest != job.feedback_source()?.learning_evidence_digest
            || evidence.memory_space_id != material.memory_space_id
            || evidence.mounted_subject_id != *mounted_subject_id
            || evidence.turn_id != turn.turn_id
            || evidence.conversation_id != turn.key.conversation_id
        {
            return reject(Reason::SourceMismatch);
        }
        if evidence.authority.is_model_inferred() {
            if evidence
                .agent_tool_feedback
                .iter()
                .any(|feedback| !feedback.execution_facts.is_empty())
            {
                return reject(Reason::AuthorityDenied);
            }
            // An authenticated proposal is retained as authorship, not counted
            // as execution and not a veto over independent later witnesses.
            continue;
        }
        if turn.external_content_used {
            return reject(Reason::PrivacyDenied);
        }
        let contributions = match agent_tool_method_task_contributions(
            material,
            job,
            turn,
            source.adopted_materials,
        ) {
            Ok(contributions) => contributions,
            Err(reason) => return reject(reason),
        };
        if contributions.is_empty() {
            continue;
        }
        for contribution in contributions {
            if tasks
                .insert(contribution.contribution_id.clone(), contribution.clone())
                .is_some_and(|old| old != contribution)
            {
                return reject(Reason::SourceMismatch);
            }
        }
        for fact in evidence.execution_contributions_for_job(job)? {
            if fact.tool.registry_ref != input.registry.registry_ref()
                || fact.tool.tool_id != material.tool_id
                || fact.tool.schema_fingerprint != material.schema_fingerprint
            {
                continue;
            }
            if execution
                .insert(fact.contribution_id.clone(), fact.clone())
                .is_some_and(|old| old != fact)
            {
                return reject(Reason::SourceMismatch);
            }
        }
        sources.insert(
            job.job_id.clone(),
            evidence.learning_evidence_digest.clone(),
        );
    }
    let tasks = tasks.into_values().collect::<Vec<_>>();
    let execution = execution.into_values().collect::<Vec<_>>();
    let scope_basis = crate::memory::ProceduralSubjectScopeV1 {
        memory_space_id: material.memory_space_id.clone(),
        mounted_subject_id: mounted_subject_id.clone(),
    };
    let tool = ProceduralProducerToolV1 {
        registry_ref: input.registry.registry_ref(),
        tool_id: material.tool_id.clone(),
        schema_fingerprint: material.schema_fingerprint.clone(),
    };
    // Reuse the method owner's exact positive/negative and independence policy.
    // Profile capacity admission happens before this pure promotion planner.
    let max_references = material
        .body
        .contribution_count()
        .max(tasks.len())
        .max(execution.len());
    let (reduced, status) = match crate::memory::reduce_agent_tool_method_task_contributions(
        &scope_basis,
        &tool,
        &material.body,
        &tasks,
        &execution,
        None,
        max_references,
    ) {
        Ok(result) => result,
        Err(_) => return reject(Reason::SourceMismatch),
    };
    if reduced != material.body {
        return reject(Reason::SourceUnavailable);
    }
    if status == AgentToolExperienceStatus::Stale {
        return reject(Reason::ContradictoryTaskEvidence);
    }
    if status != AgentToolExperienceStatus::Active {
        return observe(Reason::InsufficientIndependentTasks);
    }
    let applicability = runtime_skill_applicability_for_tool_scope(&material.registry_scope)?;
    let evidence_bindings = sources
        .iter()
        .map(|(job_id, digest)| RuntimeSkillEvidenceBinding {
            kind: RuntimeSkillEvidenceKind::ProceduralFeedbackSource,
            safe_ref: job_id.clone(),
            source_digest: digest.clone(),
        })
        .collect();
    let privacy_class = material.privacy_class;
    let record = RuntimeSkillOwnerRecord::build(
        &material.memory_space_id,
        scope,
        creation_ref,
        1,
        RuntimeSkillIntrinsicContract {
            schema_version: RUNTIME_SKILL_GOVERNED_CONTRACT_SCHEMA_VERSION,
            applicability,
            triggers: vec![RuntimeSkillTrigger {
                kind: RuntimeSkillTriggerKind::TaskRequirement,
                canonical_ref: task_signature.clone(),
            }],
            constraints: constraints
                .iter()
                .map(|tag| RuntimeSkillConstraint {
                    kind: RuntimeSkillConstraintKind::ObservedToolBoundary,
                    policy_safe_ref: tag.clone(),
                })
                .collect(),
            premises: vec![],
            failure_modes: vec![
                RuntimeSkillFailureMode::ExecutionFailed,
                RuntimeSkillFailureMode::OutputRejected,
            ],
            evidence_bindings,
            projection_policy: RuntimeSkillProjectionPolicy {
                privacy_class,
                model_projection_allowed: true,
                require_all_mandatory_premises: true,
            },
            capability_affinities: vec![RuntimeSkillCapabilityAffinity::ProceduralRecall],
        },
        RuntimeSkillProceduralContent {
            title: title.clone(),
            topic: task_signature.clone(),
            summary: title,
            procedure: method.to_string(),
        },
        RuntimeSkillLifecycle::created(input.observed_at)?,
        privacy_class,
    )?;
    if record.privacy_class != MemoryPrivacyClass::SharedWithSubject {
        return Err(Error::config(
            "runtime_skill_tool_promotion",
            "unsupported source privacy for subject promotion",
        ));
    }
    Ok(RuntimeSkillToolPromotionDecision::Promote {
        record: Box::new(record),
        source_job_ids: sources.into_keys().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::*;

    const METHOD: &str = "1. Inspect the archive before extraction.\n2. Extract into an empty directory.\n3. Verify the extracted manifest.";

    struct Fixture {
        registry: AgentToolRegistrySnapshot,
        material: AgentToolExperienceRevisionMaterialV4,
        turns: Vec<TranscriptTurnRecord>,
        jobs: Vec<ProceduralFeedbackJobV2>,
        adopted_materials: Vec<AgentToolExperienceRevisionMaterialV4>,
    }

    impl Fixture {
        fn new(method: &str, scope: AgentToolRegistryScope) -> Self {
            Self::with_proposal(method, scope, false)
        }

        fn with_proposal(
            method: &str,
            scope: AgentToolRegistryScope,
            include_model_proposal: bool,
        ) -> Self {
            Self::legacy(method, scope, include_model_proposal).with_task_witnesses()
        }

        fn legacy(
            method: &str,
            scope: AgentToolRegistryScope,
            include_model_proposal: bool,
        ) -> Self {
            let mut registry = AgentToolRegistrySnapshot::compact(
                "tools",
                "tools",
                vec![AgentToolDescriptor::compact(
                    "extract", "Extract", "schema-a",
                )],
                10,
            );
            registry.scope = scope;
            registry.fingerprint = fingerprint_agent_tool_registry(&registry);
            let producer = ProceduralProducerBindingV1::build(
                ProceduralProducerSpecV1 {
                    method_evaluators: vec![MethodTaskEvaluatorV1 {
                        id: "archive-verifier".into(),
                        revision: 1,
                    }],
                    binding_id: "executor".into(),
                    scope: ProceduralProducerScopeV1 {
                        memory_space_id: "space-a".into(),
                        mounted_subject_id: "agent-a".into(),
                        channel_id: "sdk.direct".into(),
                        chat_id: "chat-a".into(),
                    },
                    principal: ProceduralProducerPrincipalV1::LocalCapability {
                        capability_id: "executor".into(),
                    },
                    source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
                    claims: ProceduralProducerClaimsV1 {
                        method_learning_materials: false,
                        method_adoptions: true,
                        method_task_evaluations: true,
                        execution_facts: true,
                        method_declarations: true,
                        usage_feedback: false,
                        source_classifications: vec![ProceduralSourceSensitivity::NonPrivate],
                    },
                    tools: vec![ProceduralProducerToolV1 {
                        registry_ref: registry.registry_ref(),
                        tool_id: "extract".into(),
                        schema_fingerprint: "schema-a".into(),
                    }],
                    source_config_ref: "synthetic-executor-config".into(),
                },
                ProceduralProducerStateV1::Active,
                crate::memory::MemoryMutationOperationIdentity::new(
                    "grant-executor",
                    "space-a",
                    "agent-a",
                    "governor-a",
                    crate::memory::MemoryMutationOperationKind::ProceduralProducerControl,
                )
                .unwrap(),
                10,
                None,
            )
            .unwrap();
            let mut model_spec = producer.spec.clone();
            model_spec.binding_id = "model-proposer".into();
            model_spec.source_authority = ProceduralProducerSourceAuthorityV1::ModelInferred {
                subject_id: "agent-a".into(),
            };
            model_spec.claims.execution_facts = false;
            model_spec.claims.method_adoptions = false;
            model_spec.claims.method_task_evaluations = false;
            model_spec.method_evaluators.clear();
            let model_producer = ProceduralProducerBindingV1::build(
                model_spec,
                ProceduralProducerStateV1::Active,
                crate::memory::MemoryMutationOperationIdentity::new(
                    "grant-proposer",
                    "space-a",
                    "agent-a",
                    "governor-a",
                    crate::memory::MemoryMutationOperationKind::ProceduralProducerControl,
                )
                .unwrap(),
                10,
                None,
            )
            .unwrap();
            let mut turns = Vec::new();
            for index in 1..=if include_model_proposal { 3 } else { 2 } {
                let proposal = include_model_proposal && index == 1;
                let source_producer = if proposal { &model_producer } else { &producer };
                let turn_id = format!("turn-{index}");
                let observation_id = format!("observation-{index}");
                let mut delta = CanonicalTurnDelta {
                    turn_id: turn_id.clone(),
                    conversation: ConversationScope {
                        channel: "sdk.direct".into(),
                        chat_id: "chat-a".into(),
                        conversation_id: Some("conversation-a".into()),
                    },
                    subject: "agent-a".into(),
                    delivery_status: MemoryTurnDeliveryStatus::Delivered,
                    source: MemoryTurnSource {
                        ingress: crate::bus::IngressKind::User,
                        channel: "sdk.direct".into(),
                        provider: None,
                        protocol: MemoryTurnProtocol::Native,
                        endpoint: None,
                        model_alias: None,
                        model_resolved: None,
                        request_id: Some(turn_id.clone()),
                        client_conversation_hint: None,
                    },
                    actor: None,
                    input_messages: vec![TranscriptInputMessage::user("Extract the archive")],
                    assistant_message: Some(TranscriptInputMessage::assistant("Done")),
                    tool_observations: vec![ToolObservationDigest {
                        observation_id: observation_id.clone(),
                        call_id: format!("call-{index}"),
                        tool_name: "extract".into(),
                        summary: "Extraction finished".into(),
                        external_content: false,
                    }],
                    external_content_used: false,
                    candidate_ids: vec![],
                };
                if proposal {
                    delta.tool_observations.clear();
                }
                let mut evidence = PostTurnLearningEvidenceV3 {
                    method_learning_inputs: Vec::new(),
                    method_adoptions: Vec::new(),
                    method_task_evaluations: Vec::new(),
                    submitted_input_commitment: Some(format!("sha256:{}", "a".repeat(64))),
                    schema_version: POST_TURN_LEARNING_EVIDENCE_SCHEMA_VERSION,
                    memory_space_id: "space-a".into(),
                    mounted_subject_id: "agent-a".into(),
                    conversation_id: "conversation-a".into(),
                    turn_id,
                    canonical_turn_digest: canonical_turn_learning_digest(&delta).unwrap(),
                    tool_call_count: if proposal { 0 } else { 1 },
                    selection_receipt: None,
                    runtime_skill_feedback: vec![],
                    agent_skill_feedback: vec![],
                    task_learning_feedback: vec![],
                    agent_tool_feedback: vec![AgentToolUsageFeedbackV3 {
                        registry_ref: registry.registry_ref(),
                        tool_id: "extract".into(),
                        schema_fingerprint: "schema-a".into(),
                        execution_facts: if proposal {
                            vec![]
                        } else {
                            vec![ToolExecutionFactV1 {
                                observation_id,
                                call_id: format!("call-{index}"),
                                outcome: ToolExecutionOutcome::Succeeded,
                                source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
                                started_at: Some(10),
                                completed_at: Some(20),
                            }]
                        },
                        method_evidence: vec![ToolMethodEvidenceV1 {
                            method_id: "method-a".into(),
                            task_signature: "archive-task".into(),
                            body: method.into(),
                            execution_refs: if proposal {
                                vec![]
                            } else {
                                vec![format!("observation-{index}")]
                            },
                            source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
                            external_content: false,
                        }],
                    }
                    .admit_content()
                    .unwrap()],
                    authority: ProceduralFeedbackAuthorityV2::Producer {
                        producer_revision: source_producer.revision_ref().unwrap(),
                        source_authority: source_producer.spec.source_authority.clone(),
                        confirmation: None,
                    },
                    learning_evidence_digest: String::new(),
                };
                evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
                turns.push(
                    TranscriptTurnRecord::from_delta_with_learning_evidence(
                        &ConversationKey::new("space-a", "sdk.direct", "conversation-a").unwrap(),
                        index,
                        &delta,
                        vec![],
                        Some(evidence),
                        20,
                    )
                    .unwrap(),
                );
            }
            let jobs: Vec<_> = turns.iter().map(Self::job).collect();
            let mut body = AgentToolExperienceBodyV1::Method {
                usage_contributions: Vec::new(),
                task_signature: "archive-task".into(),
                procedure: method.into(),
                constraints: vec!["workspace-write".into()],
                sources: vec![],
            };
            let method_digest = body.method_content_digest().unwrap().unwrap();
            let AgentToolExperienceBodyV1::Method { sources, .. } = &mut body else {
                unreachable!()
            };
            for (turn, job) in turns.iter().zip(&jobs) {
                let evidence = turn.learning_evidence.as_ref().unwrap();
                let feedback = &evidence.agent_tool_feedback[0];
                let mut execution_refs = feedback
                    .execution_facts
                    .iter()
                    .map(|fact| {
                        ToolExecutionContributionV1::build(
                            job.feedback_source().unwrap().identity.clone(),
                            evidence.learning_evidence_digest.clone(),
                            producer.revision_ref().unwrap(),
                            producer.spec.tools[0].clone(),
                            fact.clone(),
                        )
                        .unwrap()
                        .reference()
                        .unwrap()
                    })
                    .collect::<Vec<_>>();
                execution_refs.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
                sources.push(AgentToolMethodSourceRefV2::ProducerDeclaration {
                    source_job_id: job.job_id.clone(),
                    method_id: "method-a".into(),
                    method_digest: method_digest.clone(),
                    execution_refs,
                });
            }
            AgentToolMethodSourceRefV2::canonicalize(sources).unwrap();
            let material = AgentToolExperienceRevisionMaterialV4::build(
                "space-a",
                AgentToolExperienceOwningScopeV1::Subject {
                    mounted_subject_id: "agent-a".into(),
                },
                "tools",
                registry.scope.clone(),
                "extract",
                "schema-a",
                1,
                body,
                AgentToolExperienceStatus::Active,
                MemoryPrivacyClass::SharedWithSubject,
                20,
                20,
                None,
            )
            .unwrap();
            Self {
                registry,
                material,
                turns,
                jobs,
                adopted_materials: Vec::new(),
            }
        }

        fn with_task_witnesses(mut self) -> Self {
            let initial = &self.material;
            let candidate = AgentToolExperienceRevisionMaterialV4::build(
                &initial.memory_space_id,
                initial.owning_scope.clone(),
                &initial.registry_id,
                initial.registry_scope.clone(),
                &initial.tool_id,
                &initial.schema_fingerprint,
                1,
                initial.body.clone(),
                AgentToolExperienceStatus::Candidate,
                initial.privacy_class,
                20,
                20,
                None,
            )
            .unwrap();
            let reference = MethodContentRefV1 {
                memory_space_id: candidate.memory_space_id.clone(),
                mounted_subject_id: "agent-a".into(),
                registry_ref: self.registry.registry_ref(),
                tool_id: candidate.tool_id.clone(),
                schema_fingerprint: candidate.schema_fingerprint.clone(),
                owner_revision: candidate.owner_revision_ref(),
                content_digest: candidate.content_digest.clone(),
            };
            let originals = self.turns.clone();
            for original in originals {
                let original_evidence = original.learning_evidence.as_ref().unwrap();
                if original_evidence.authority.is_model_inferred() {
                    continue;
                }
                let turn_id = format!("adoption-{}", original.turn_id);
                let observations = original.tool_observations.clone();
                let delta = CanonicalTurnDelta {
                    turn_id: turn_id.clone(),
                    conversation: ConversationScope {
                        channel: "sdk.direct".into(),
                        chat_id: "chat-a".into(),
                        conversation_id: Some("conversation-a".into()),
                    },
                    subject: "agent-a".into(),
                    delivery_status: MemoryTurnDeliveryStatus::Delivered,
                    source: MemoryTurnSource {
                        ingress: crate::bus::IngressKind::User,
                        channel: "sdk.direct".into(),
                        provider: None,
                        protocol: MemoryTurnProtocol::Native,
                        endpoint: None,
                        model_alias: None,
                        model_resolved: None,
                        request_id: Some(turn_id.clone()),
                        client_conversation_hint: None,
                    },
                    actor: None,
                    input_messages: vec![TranscriptInputMessage::user(
                        "Apply the method and verify extraction",
                    )],
                    assistant_message: Some(TranscriptInputMessage::assistant(
                        "Verification passed",
                    )),
                    tool_observations: observations.clone(),
                    external_content_used: false,
                    candidate_ids: vec![],
                };
                let mut evidence = original_evidence.clone();
                evidence.turn_id = turn_id.clone();
                evidence.canonical_turn_digest = canonical_turn_learning_digest(&delta).unwrap();
                evidence.agent_tool_feedback[0].method_evidence.clear();
                evidence.agent_tool_feedback[0].submitted_method_count = 0;
                let observation_refs = observations
                    .iter()
                    .map(|item| item.observation_id.clone())
                    .collect::<Vec<_>>();
                evidence.method_adoptions = vec![MethodAdoptionEvidenceV1 {
                    method: reference.clone(),
                    task_id: turn_id.clone(),
                    adoption: MethodAdoptionStatusV1::Adopted,
                    observation_refs: observation_refs.clone(),
                }];
                evidence.method_task_evaluations = vec![MethodTaskEvaluationV1 {
                    method: reference.clone(),
                    task_id: turn_id.clone(),
                    task_root_id: format!("root-{turn_id}"),
                    independence_group: format!("scenario-{turn_id}"),
                    evaluator_id: "archive-verifier".into(),
                    evaluator_revision: 1,
                    outcome: MethodTaskOutcomeV1::Passed,
                    observation_refs,
                }];
                evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
                let turn = TranscriptTurnRecord::from_delta_with_learning_evidence(
                    &original.key,
                    original.sequence + 100,
                    &delta,
                    vec![],
                    Some(evidence),
                    30,
                )
                .unwrap();
                self.jobs.push(Self::job(&turn));
                self.turns.push(turn);
            }
            self.adopted_materials = vec![candidate];
            self.rebuild_usage();
            self
        }

        fn rebuild_usage(&mut self) {
            let candidate = &self.adopted_materials[0];
            let mut body = candidate.body.clone();
            let AgentToolExperienceBodyV1::Method {
                usage_contributions,
                ..
            } = &mut body
            else {
                unreachable!()
            };
            for (turn, job) in self.turns.iter().zip(&self.jobs) {
                let evidence = turn.learning_evidence.as_ref().unwrap();
                for contribution in evidence
                    .method_task_contributions_for_job(job, turn, |_| Ok(candidate.clone()))
                    .unwrap()
                {
                    usage_contributions.push(contribution.reference().unwrap());
                }
            }
            usage_contributions.sort_by(|a, b| a.contribution_id.cmp(&b.contribution_id));
            usage_contributions.dedup();
            self.material = AgentToolExperienceRevisionMaterialV4::build(
                &candidate.memory_space_id,
                candidate.owning_scope.clone(),
                &candidate.registry_id,
                candidate.registry_scope.clone(),
                &candidate.tool_id,
                &candidate.schema_fingerprint,
                2,
                body,
                AgentToolExperienceStatus::Active,
                candidate.privacy_class,
                20,
                40,
                Some(
                    AgentToolExperienceRetainedRevisionDigestV3::from_material(candidate).unwrap(),
                ),
            )
            .unwrap();
        }

        fn job(turn: &TranscriptTurnRecord) -> ProceduralFeedbackJobV2 {
            let mut job = ProceduralFeedbackJobV2::pending(
                ProceduralFeedbackIdentityV1::new(
                    "space-a",
                    "agent-a",
                    "sdk.direct",
                    "chat-a",
                    "conversation-a",
                    &turn.turn_id,
                )
                .unwrap(),
                turn.sequence,
                post_turn_governance_transcript_digest(turn).unwrap(),
                &turn
                    .learning_evidence
                    .as_ref()
                    .unwrap()
                    .learning_evidence_digest,
                1,
                3,
                20,
            )
            .unwrap();
            job.status = ProceduralFeedbackJobStatusV1::Leased;
            job.next_attempt_at = None;
            job.lease_owner = Some("official-worker".into());
            job.lease_until = Some(200);
            job.lease_epoch = 1;
            job.validate().unwrap();
            job
        }

        fn refresh(&mut self, index: usize) {
            let evidence = self.turns[index].learning_evidence.as_mut().unwrap();
            evidence.learning_evidence_digest = evidence.canonical_digest().unwrap();
            self.jobs[index] = Self::job(&self.turns[index]);
        }

        fn plan(
            &self,
            existing: Option<&RuntimeSkillOwnerRecord>,
            observed_at: u64,
        ) -> RuntimeSkillToolPromotionDecision {
            let sources = self
                .jobs
                .iter()
                .zip(&self.turns)
                .map(|(job, transcript)| RuntimeSkillToolPromotionSource {
                    job,
                    transcript,
                    adopted_materials: &self.adopted_materials,
                })
                .collect::<Vec<_>>();
            plan_runtime_skill_promotion_from_tool_evidence(&RuntimeSkillToolPromotionInput {
                experience: &self.material,
                registry: &self.registry,
                sources: &sources,
                existing_owner: existing,
                observed_at,
            })
            .unwrap()
        }
    }

    #[test]
    fn successful_tool_calls_without_task_adoption_cannot_promote() {
        let fixture = Fixture::legacy(METHOD, AgentToolRegistryScope::Global, false);
        assert!(matches!(
            fixture.plan(None, 100),
            RuntimeSkillToolPromotionDecision::Observe { .. }
        ), "different successful tool calls do not prove method adoption or independent task success");
    }

    #[test]
    fn model_proposal_is_not_execution_and_cannot_poison_later_independent_witnesses() {
        let mut fixture = Fixture::with_proposal(METHOD, AgentToolRegistryScope::Global, true);
        let proposal_id = fixture.jobs[0].job_id.clone();
        assert!(fixture.turns[0]
            .learning_evidence
            .as_ref()
            .unwrap()
            .authority
            .is_model_inferred());
        assert!(fixture.turns[0]
            .learning_evidence
            .as_ref()
            .unwrap()
            .agent_tool_feedback[0]
            .execution_facts
            .is_empty());
        let RuntimeSkillToolPromotionDecision::Promote { source_job_ids, .. } =
            fixture.plan(None, 100)
        else {
            panic!("a proposal is neither execution proof nor a veto on later real evidence");
        };
        assert_eq!(source_job_ids.len(), 2);
        assert!(!source_job_ids.contains(&proposal_id));
        fixture.turns.pop();
        fixture.jobs.pop();
        fixture.rebuild_usage();
        assert!(matches!(
            fixture.plan(None, 100),
            RuntimeSkillToolPromotionDecision::Observe {
                reason: RuntimeSkillToolPromotionReason::InsufficientIndependentTasks
            }
        ));
    }

    #[test]
    fn tool_promotion_retains_method_sources_boundaries_and_stable_identity() {
        let fixture = Fixture::new(METHOD, AgentToolRegistryScope::Global);
        let RuntimeSkillToolPromotionDecision::Promote {
            record,
            source_job_ids,
        } = fixture.plan(None, 100)
        else {
            panic!("two distinct canonical sources must promote")
        };
        assert_eq!(record.procedural_content.procedure, METHOD);
        assert_eq!(record.owner_revision, 1);
        assert_eq!(source_job_ids.len(), 2);
        assert_eq!(
            record.intrinsic_contract.constraints,
            vec![RuntimeSkillConstraint {
                kind: RuntimeSkillConstraintKind::ObservedToolBoundary,
                policy_safe_ref: "workspace-write".into()
            }]
        );
        assert!(record
            .intrinsic_contract
            .evidence_bindings
            .iter()
            .all(|binding| binding.kind == RuntimeSkillEvidenceKind::ProceduralFeedbackSource));
        let retired = record.retire(101).unwrap();
        assert!(matches!(
            fixture.plan(Some(&retired), 102),
            RuntimeSkillToolPromotionDecision::AlreadyPromoted { .. }
        ));
        assert!(matches!(
            fixture.plan(None, 19),
            RuntimeSkillToolPromotionDecision::Reject {
                reason: RuntimeSkillToolPromotionReason::SourceTimeInvalid
            }
        ));
    }

    #[test]
    fn tool_promotion_does_not_invent_methods_or_count_one_task_twice() {
        assert!(validate_runtime_skill_method_shape(
            "archive-task",
            "Archive extraction completed"
        )
        .is_err());
        let mut fixture = Fixture::new(METHOD, AgentToolRegistryScope::Global);
        let last = fixture.turns.len() - 1;
        fixture.turns[last] = fixture.turns[last - 1].clone();
        fixture.jobs[last] = fixture.jobs[last - 1].clone();
        fixture.rebuild_usage();
        assert!(matches!(
            fixture.plan(None, 100),
            RuntimeSkillToolPromotionDecision::Observe {
                reason: RuntimeSkillToolPromotionReason::InsufficientIndependentTasks,
            }
        ));
    }

    #[test]
    fn task_root_group_and_outcome_are_rechecked_instead_of_trusting_active_status() {
        for mutation in 0..5 {
            let mut fixture = Fixture::new(METHOD, AgentToolRegistryScope::Global);
            let last = fixture.turns.len() - 1;
            let first = fixture.turns[last - 1]
                .learning_evidence
                .as_ref()
                .unwrap()
                .method_task_evaluations[0]
                .clone();
            let evidence = fixture.turns[last].learning_evidence.as_mut().unwrap();
            match mutation {
                0 => evidence.method_task_evaluations[0].task_root_id = first.task_root_id,
                1 => {
                    evidence.method_task_evaluations[0].independence_group =
                        first.independence_group
                }
                2 => evidence.method_task_evaluations[0].outcome = MethodTaskOutcomeV1::Failed,
                3 => evidence.method_task_evaluations[0].outcome = MethodTaskOutcomeV1::Unknown,
                _ => evidence.method_adoptions[0].adoption = MethodAdoptionStatusV1::NotAdopted,
            }
            fixture.refresh(last);
            fixture.rebuild_usage();
            assert!(
                !matches!(
                    fixture.plan(None, 100),
                    RuntimeSkillToolPromotionDecision::Promote { .. }
                ),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn missing_or_different_adopted_revision_fails_closed() {
        for mutation in 0..3 {
            let mut fixture = Fixture::new(METHOD, AgentToolRegistryScope::Global);
            match mutation {
                0 => fixture.adopted_materials.clear(),
                1 => fixture
                    .adopted_materials
                    .push(fixture.adopted_materials[0].clone()),
                _ => {
                    fixture.adopted_materials[0].content_digest =
                        format!("sha256:{}", "a".repeat(64))
                }
            }
            assert!(matches!(
                fixture.plan(None, 100),
                RuntimeSkillToolPromotionDecision::Reject { .. }
            ));
        }
    }

    #[test]
    fn tool_promotion_rejects_injected_source_authority_privacy_and_canonical_observation_drift() {
        for mutation in 0..7 {
            let mut fixture = Fixture::new(METHOD, AgentToolRegistryScope::Global);
            match mutation {
                0 => {
                    fixture.turns[2]
                        .learning_evidence
                        .as_mut()
                        .unwrap()
                        .authority = ProceduralFeedbackAuthorityV2::Producer {
                        producer_revision: fixture.turns[1]
                            .learning_evidence
                            .as_ref()
                            .unwrap()
                            .authority
                            .producer_revision()
                            .unwrap()
                            .clone(),
                        source_authority: ProceduralProducerSourceAuthorityV1::ModelInferred {
                            subject_id: "agent-a".into(),
                        },
                        confirmation: None,
                    }
                }
                1 => {
                    fixture.turns[2]
                        .learning_evidence
                        .as_mut()
                        .unwrap()
                        .agent_tool_feedback[0]
                        .execution_facts[0]
                        .source_sensitivity = ProceduralSourceSensitivity::Private
                }
                2 => fixture.turns[2].tool_observations[0].tool_name = "other-tool".into(),
                3 => {
                    fixture.turns[2].tool_observations[0].observation_id =
                        "other-observation".into()
                }
                4 => fixture.turns[2].tool_observations[0].call_id = "other-call".into(),
                5 => fixture.turns[2].tool_observations[0].external_content = true,
                _ => fixture.turns[2].subject = "agent-b".into(),
            }
            fixture.refresh(2);
            assert!(
                matches!(
                    fixture.plan(None, 100),
                    RuntimeSkillToolPromotionDecision::Reject { .. }
                ),
                "mutation {mutation}"
            );
        }
        let mut fixture = Fixture::new(METHOD, AgentToolRegistryScope::Global);
        fixture.turns[2].apply_lifecycle_transition(TranscriptLifecycleTransition::Mask, 50);
        assert!(matches!(
            fixture.plan(None, 100),
            RuntimeSkillToolPromotionDecision::Reject {
                reason: RuntimeSkillToolPromotionReason::SourceUnavailable
            }
        ));
    }

    #[test]
    fn tool_promotion_preserves_workspace_and_conversation_applicability() {
        for (scope, target) in [
            (
                AgentToolRegistryScope::Workspace {
                    workspace_id: "workspace-a".into(),
                },
                RuntimeSkillApplicabilityTarget::Workspace {
                    workspace_id: "workspace-a".into(),
                },
            ),
            (
                AgentToolRegistryScope::Conversation {
                    conversation_id: "conversation-a".into(),
                },
                RuntimeSkillApplicabilityTarget::Conversation {
                    conversation_id: "conversation-a".into(),
                },
            ),
        ] {
            let fixture = Fixture::new(METHOD, scope);
            let RuntimeSkillToolPromotionDecision::Promote { record, .. } = fixture.plan(None, 100)
            else {
                panic!("scope positive")
            };
            assert_eq!(
                record.intrinsic_contract.applicability,
                RuntimeSkillApplicability::try_all_of(vec![target]).unwrap()
            );
            assert_eq!(
                record.owning_scope,
                RuntimeSkillOwningScope::Subject {
                    mounted_subject_id: "agent-a".into()
                }
            );
        }
    }
}
