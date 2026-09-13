//! Pure admission of observed tool methods into the existing RuntimeSkill owner.

use std::collections::BTreeMap;

use crate::memory::{
    post_turn_governance_transcript_digest, MemoryPrivacyClass, ProceduralFeedbackJobStatusV1,
    ProceduralFeedbackJobV2, ProceduralProducerToolV1, ProceduralSourceSensitivity,
    ToolExecutionContributionV1, ToolExecutionFactV1, ToolExecutionOutcome, TranscriptTurnRecord,
};
use crate::{Error, Result};

use super::*;

pub struct RuntimeSkillToolPromotionSource<'a> {
    pub job: &'a ProceduralFeedbackJobV2,
    pub transcript: &'a TranscriptTurnRecord,
}

pub struct RuntimeSkillToolPromotionInput<'a> {
    pub experience: &'a AgentToolExperienceRevisionMaterialV3,
    pub registry: &'a AgentToolRegistrySnapshot,
    pub sources: &'a [RuntimeSkillToolPromotionSource<'a>],
    pub existing_owner: Option<&'a RuntimeSkillOwnerRecord>,
    pub observed_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSkillToolPromotionReason {
    WeakProcedure,
    InsufficientDistinctTurns,
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

/// Shared method-to-execution proof for the planner and Store post-image
/// closure. The caller separately authenticates the exact job/turn roots and
/// current producer authority; this function grants neither permission.
pub fn agent_tool_method_execution_matches_source(
    material: &AgentToolExperienceRevisionMaterialV3,
    job: &ProceduralFeedbackJobV2,
    turn: &TranscriptTurnRecord,
    feedback: &crate::memory::AdmittedAgentToolEvidenceV1,
) -> std::result::Result<bool, RuntimeSkillToolPromotionReason> {
    use RuntimeSkillToolPromotionReason as Reason;
    let AgentToolExperienceBodyV1::Method {
        task_signature,
        procedure,
        sources,
        ..
    } = &material.body
    else {
        return Ok(false);
    };
    if feedback.registry_ref.registry_id != material.registry_id
        || feedback.registry_ref.scope != material.registry_scope
        || feedback.tool_id != material.tool_id
        || feedback.schema_fingerprint != material.schema_fingerprint
    {
        return Ok(false);
    }
    let evidence = turn
        .learning_evidence
        .as_ref()
        .ok_or(Reason::SourceMismatch)?;
    if evidence.authority.is_model_inferred() {
        return if feedback.execution_facts.is_empty() {
            Ok(false)
        } else {
            Err(Reason::AuthorityDenied)
        };
    }
    if turn.external_content_used {
        return Err(Reason::PrivacyDenied);
    }
    let producer = evidence
        .authority
        .producer_revision()
        .ok_or(Reason::AuthorityDenied)?;
    let mut matched = false;
    for method in &feedback.method_evidence {
        if method.task_signature != *task_signature || method.body != *procedure {
            continue;
        }
        if method.source_sensitivity != ProceduralSourceSensitivity::NonPrivate
            || method.external_content
        {
            return Err(Reason::PrivacyDenied);
        }
        let source = sources
            .iter()
            .find(|source| {
                source.source_job_id == job.job_id && source.method_id == method.method_id
            })
            .ok_or(Reason::SourceMismatch)?;
        for reference in &method.execution_refs {
            let fact = feedback
                .execution_facts
                .iter()
                .find(|fact| &fact.observation_id == reference)
                .ok_or(Reason::SourceMismatch)?;
            if fact.outcome != ToolExecutionOutcome::Succeeded {
                continue;
            }
            if fact.source_sensitivity != ProceduralSourceSensitivity::NonPrivate
                || turn.tool_observations.iter().any(|canonical| {
                    canonical.observation_id == fact.observation_id && canonical.external_content
                })
            {
                return Err(Reason::PrivacyDenied);
            }
            if !agent_tool_execution_matches_transcript(fact, &feedback.tool_id, turn) {
                return Err(Reason::SourceMismatch);
            }
            let contribution = ToolExecutionContributionV1::build(
                job.feedback_source()
                    .map_err(|_| Reason::SourceMismatch)?
                    .identity
                    .clone(),
                evidence.learning_evidence_digest.clone(),
                producer.clone(),
                ProceduralProducerToolV1 {
                    registry_ref: feedback.registry_ref.clone(),
                    tool_id: feedback.tool_id.clone(),
                    schema_fingerprint: feedback.schema_fingerprint.clone(),
                },
                fact.clone(),
            )
            .map_err(|_| Reason::SourceMismatch)?;
            if !source.execution_refs.contains(
                &contribution
                    .reference()
                    .map_err(|_| Reason::SourceMismatch)?,
            ) {
                return Err(Reason::SourceMismatch);
            }
            matched = true;
        }
    }
    Ok(matched)
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
    let mut runs = BTreeMap::new();
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
        let mut matched = false;
        for feedback in &evidence.agent_tool_feedback {
            if feedback.registry_ref != input.registry.registry_ref()
                || feedback.tool_id != material.tool_id
                || feedback.schema_fingerprint != material.schema_fingerprint
            {
                continue;
            }
            match agent_tool_method_execution_matches_source(material, job, turn, feedback) {
                Ok(value) => matched |= value,
                Err(reason) => return reject(reason),
            }
        }
        if matched {
            let run_identity = (
                turn.key.storage_key(),
                turn.subject.clone(),
                turn.turn_id.clone(),
            );
            if runs
                .insert(run_identity, turn.canonical_turn_digest.clone())
                .is_some_and(|previous| previous != turn.canonical_turn_digest)
            {
                return reject(Reason::SourceMismatch);
            }
            sources.insert(
                job.job_id.clone(),
                job.feedback_source()?.learning_evidence_digest.clone(),
            );
        }
    }
    if !crate::reasoning::has_repeated_procedural_runs(runs.len()) {
        return observe(Reason::InsufficientDistinctTurns);
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
        material: AgentToolExperienceRevisionMaterialV3,
        turns: Vec<TranscriptTurnRecord>,
        jobs: Vec<ProceduralFeedbackJobV2>,
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
                let mut evidence = PostTurnLearningEvidenceV2 {
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
                sources.push(AgentToolMethodSourceRefV1 {
                    source_job_id: job.job_id.clone(),
                    method_id: "method-a".into(),
                    method_digest: method_digest.clone(),
                    execution_refs,
                });
            }
            sources.sort_by(|a, b| a.source_job_id.cmp(&b.source_job_id));
            let material = AgentToolExperienceRevisionMaterialV3::build(
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
            }
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
                .map(|(job, transcript)| RuntimeSkillToolPromotionSource { job, transcript })
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
        assert!(matches!(
            fixture.plan(None, 100),
            RuntimeSkillToolPromotionDecision::Observe {
                reason: RuntimeSkillToolPromotionReason::InsufficientDistinctTurns
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
    fn tool_promotion_does_not_invent_methods_or_count_one_turn_twice() {
        assert!(validate_runtime_skill_method_shape(
            "archive-task",
            "Archive extraction completed"
        )
        .is_err());
        let mut fixture = Fixture::new(METHOD, AgentToolRegistryScope::Global);
        fixture.turns[1] = fixture.turns[0].clone();
        fixture.jobs[1] = fixture.jobs[0].clone();
        assert!(matches!(
            fixture.plan(None, 100),
            RuntimeSkillToolPromotionDecision::Observe {
                reason: RuntimeSkillToolPromotionReason::InsufficientDistinctTurns
            }
        ));
        let mut fixture = Fixture::new(METHOD, AgentToolRegistryScope::Global);
        fixture.turns[1]
            .learning_evidence
            .as_mut()
            .unwrap()
            .agent_tool_feedback[0]
            .method_evidence[0]
            .body = "1. Inspect the different format.\n2. Extract its alternate manifest.".into();
        fixture.refresh(1);
        assert!(matches!(
            fixture.plan(None, 100),
            RuntimeSkillToolPromotionDecision::Observe { .. }
        ));
    }

    #[test]
    fn tool_promotion_rejects_injected_source_authority_privacy_and_canonical_observation_drift() {
        for mutation in 0..7 {
            let mut fixture = Fixture::new(METHOD, AgentToolRegistryScope::Global);
            match mutation {
                0 => {
                    fixture.turns[0]
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
                    fixture.turns[0]
                        .learning_evidence
                        .as_mut()
                        .unwrap()
                        .agent_tool_feedback[0]
                        .execution_facts[0]
                        .source_sensitivity = ProceduralSourceSensitivity::Private
                }
                2 => fixture.turns[0].tool_observations[0].tool_name = "other-tool".into(),
                3 => {
                    fixture.turns[0].tool_observations[0].observation_id =
                        "other-observation".into()
                }
                4 => fixture.turns[0].tool_observations[0].call_id = "other-call".into(),
                5 => fixture.turns[0].tool_observations[0].external_content = true,
                _ => fixture.turns[0].subject = "agent-b".into(),
            }
            fixture.refresh(0);
            assert!(
                matches!(
                    fixture.plan(None, 100),
                    RuntimeSkillToolPromotionDecision::Reject { .. }
                ),
                "mutation {mutation}"
            );
        }
        let mut fixture = Fixture::new(METHOD, AgentToolRegistryScope::Global);
        fixture.turns[0].apply_lifecycle_transition(TranscriptLifecycleTransition::Mask, 50);
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
