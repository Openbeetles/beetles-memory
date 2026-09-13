mod support;
use std::sync::Arc;

use bm_sdk::*;

const TOOL_DESCRIPTION: &str = "Registry-only PDF extraction capability description";

struct TestRuntime {
    memory: Arc<MemoryRuntime>,
    capability: MemoryProceduralSubmissionCapability,
}
impl std::ops::Deref for TestRuntime {
    type Target = MemoryRuntime;
    fn deref(&self) -> &MemoryRuntime {
        &self.memory
    }
}
fn authorized_runtime(memory: MemoryRuntime, store: MemoryStoreHandle) -> Arc<TestRuntime> {
    let governor = support::procedural::governor(&memory, store);
    let spec = support::procedural::runtime_observer_spec(&memory, "registry-executor");
    let capability =
        support::procedural::register_and_issue(&governor, spec, "register-registry-executor");
    Arc::new(TestRuntime {
        memory: Arc::new(memory),
        capability,
    })
}

#[derive(Default)]
struct NoProviderPort;

impl GovernanceExecutionPort for NoProviderPort {
    fn execute(
        &mut self,
        _envelope: &AuthorizedGovernanceEnvelope,
        _binding: &ImmutableGovernanceExecutionBinding,
        _egress: &GovernanceEgressAuthority,
        _operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        Err(GovernanceExecutionPortFailure::Other(
            bm_sdk::Error::config(
                "agent_tool_registry_contract",
                "procedural feedback must remain local",
            ),
        ))
    }
}

fn registry() -> AgentToolRegistrySnapshot {
    let mut tool = AgentToolDescriptor::compact("pdf.extract", TOOL_DESCRIPTION, "schema-pdf-v1");
    tool.permission_tags = vec!["filesystem.read".to_string()];
    tool.risk_tags = vec!["external_content".to_string()];
    AgentToolRegistrySnapshot::compact("host-tools", "host", vec![tool], 1_800_000_000)
}

fn applicability_for_registry(
    registry: &AgentToolRegistrySnapshot,
    conversation_id: &str,
) -> ProceduralApplicabilityContextV1 {
    let project_id = match &registry.scope {
        AgentToolRegistryScope::Project { project_id } => Some(project_id.clone()),
        _ => None,
    };
    ProceduralApplicabilityContextV1::try_new(project_id, None, Some(conversation_id.to_string()))
        .expect("applicability")
}

fn runtime_with_registry(registry: AgentToolRegistrySnapshot) -> Arc<TestRuntime> {
    let profile = support::host_test_profile();
    let store =
        support::open_memory_store(StoreBackendConfig::in_memory(profile).expect("store config"))
            .expect("store");
    authorized_runtime(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("agent-tool-test", "owner-default").expect("identity"))
            .scope(MemoryScope::new("sdk.direct", "chat-1").expect("scope"))
            .store(store.clone())
            .clock(support::fixed_clock(1_800_000_100))
            .procedural_applicability_context(applicability_for_registry(&registry, "chat-1"))
            .agent_tool_registry(registry)
            .build()
            .expect("runtime"),
        store,
    )
}

fn two_agent_registry() -> SubjectRegistry {
    let mut registry =
        SubjectRegistry::single_agent_default("owner-shared", "agent-a").expect("registry");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "Agent B",
        ))
        .expect("agent-b subject");
    registry
}

fn runtime_for_subject(
    store: MemoryStoreHandle,
    subject_registry: SubjectRegistry,
    registry: AgentToolRegistrySnapshot,
    agent_id: &str,
) -> Arc<TestRuntime> {
    let applicability = applicability_for_registry(&registry, "shared-chat");
    authorized_runtime(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new(agent_id, "owner-shared").expect("identity"))
            .scope(MemoryScope::new("sdk.direct", "shared-chat").expect("scope"))
            .store(store.clone())
            .clock(support::fixed_clock(1_800_000_100))
            .subject_registry(subject_registry)
            .procedural_applicability_context(applicability)
            .agent_tool_registry(registry)
            .build()
            .expect("subject runtime"),
        store,
    )
}

fn observation(observation_id: &str) -> ToolExecutionFactV1 {
    ToolExecutionFactV1 {
        observation_id: observation_id.into(),
        call_id: format!("call-{observation_id}"),
        outcome: ToolExecutionOutcome::Succeeded,
        source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
        started_at: Some(1_800_000_010),
        completed_at: Some(1_800_000_011),
    }
}

fn feedback(
    registry: &AgentToolRegistrySnapshot,
    facts: Vec<ToolExecutionFactV1>,
) -> AgentToolUsageFeedbackV3 {
    AgentToolUsageFeedbackV3 { registry_ref: registry.registry_ref(), tool_id: "pdf.extract".into(),
        schema_fingerprint: "schema-pdf-v1".into(), method_evidence: vec![ToolMethodEvidenceV1 {
            method_id: format!("method-{}", facts[0].observation_id), task_signature: "extract_pdf_text_for_release_notes".into(),
            body: "1. Extract PDF text from the provided document\n2. Validate extracted text against the expected release sections\n3. Return verified release note text".into(),
            execution_refs: facts.iter().map(|fact| fact.observation_id.clone()).collect(),
            source_sensitivity: ProceduralSourceSensitivity::NonPrivate, external_content: false,
        }], execution_facts: facts }
}

fn submit_feedback(
    runtime: Arc<TestRuntime>,
    feedback: AgentToolUsageFeedbackV3,
) -> ProceduralFeedbackReceiptV2 {
    let turn_id = format!("turn-{}", feedback.execution_facts[0].observation_id);
    let tool_call_count = feedback.execution_facts.len() as u32;
    let tool_observations =
        support::procedural::canonical_observations("pdf.extract", &feedback.execution_facts);
    let finalize = runtime
        .finalize_turn_with_procedural_evidence(
            &runtime.capability,
            MemoryTurnFinalizeRequest {
                turn: CanonicalTurnDelta {
                    turn_id: turn_id.clone(),
                    conversation: ConversationScope {
                        channel: runtime.scope().channel.clone(),
                        chat_id: runtime.scope().chat_id.clone(),
                        conversation_id: Some(
                            runtime.scope().conversation_id_or_chat_id().to_string(),
                        ),
                    },
                    subject: runtime.subject_id().to_string(),
                    delivery_status: MemoryTurnDeliveryStatus::Delivered,
                    source: MemoryTurnSource {
                        ingress: bm_sdk::IngressKind::User,
                        channel: runtime.scope().channel.clone(),
                        provider: None,
                        protocol: MemoryTurnProtocol::Native,
                        endpoint: None,
                        model_alias: None,
                        model_resolved: None,
                        request_id: Some(format!("request-{turn_id}")),
                        client_conversation_hint: None,
                    },
                    actor: None,
                    input_messages: vec![TranscriptInputMessage::user("synthetic tool execution")],
                    assistant_message: Some(TranscriptInputMessage::assistant("synthetic result")),
                    tool_observations,
                    external_content_used: false,
                    candidate_ids: Vec::new(),
                },
                learning: PostTurnLearningInputV2 {
                    tool_call_count,
                    selection_receipt: None,
                    runtime_skill_feedback: Vec::new(),
                    agent_skill_feedback: Vec::new(),
                    task_learning_feedback: Vec::new(),
                    agent_tool_feedback: vec![feedback],
                    human_confirmation_operation_id: None,
                },
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .expect("finalize tool feedback");
    let job_id = finalize
        .procedural_learning
        .job_id
        .expect("procedural feedback job");
    let engine = MemoryLearningEngine::attach(runtime.memory.clone()).expect("learning engine");
    let mut port = NoProviderPort;
    for attempt in 0..4 {
        match engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: format!("agent-tool-contract-{attempt}"),
                    lease_duration_secs: 60,
                },
                &mut port,
            )
            .expect("learning cycle")
        {
            MemoryLearningCycleOutcome::ProceduralCompleted(report) => {
                assert_eq!(report.job.job_id, job_id);
                return report.receipt;
            }
            MemoryLearningCycleOutcome::Idle { .. } => continue,
            other => panic!("unexpected learning outcome: {other:?}"),
        }
    }
    panic!("procedural feedback job did not complete")
}

#[test]
fn sdk_agent_tool_registry_never_cold_starts_from_tool_descriptions() {
    let registry = registry();
    let runtime = runtime_with_registry(registry.clone());

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "extract text from this PDF".to_string(),
            limit: 5,
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .expect("recall");

    assert!(recall.agent_tool_hints.is_empty());
    assert_eq!(
        recall.tool_experience_status.reason,
        AGENT_TOOL_NO_EXPERIENCE_REASON
    );
    assert!(!recall.tool_experience_status.cold_start_selection_used);

    let inspection = runtime
        .inspect(MemoryInspectionRequest {
            query: "pdf extract".to_string(),
            system_max_len: 4096,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("inspect");
    assert_eq!(inspection.agent_tool_registry.registries, 1);
    assert_eq!(inspection.agent_tool_registry.tools, 1);
    assert_eq!(inspection.agent_tool_registry.governed_experiences, Some(0));
}

#[test]
fn sdk_agent_tool_feedback_requires_governed_experience_before_projection() {
    let registry = registry();
    let runtime = runtime_with_registry(registry.clone());

    let deferred = submit_feedback(
        runtime.clone(),
        feedback(&registry, vec![observation("obs-1")]),
    );
    assert_eq!(deferred.deferred_count, 0);
    assert_eq!(deferred.accepted_count, 1);
    assert!(
        deferred.changed_count > 0,
        "execution facts and candidate method are real durable owners"
    );

    let no_hint = runtime
        .project(MemoryProjectionRequest {
            binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "extract text from this PDF".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .expect("project without governed experience");
    assert!(no_hint.provider_payload().agent_tool_hints().is_empty());
    assert_eq!(no_hint.report().audit().agent_tool_selected_count, 0);

    let accepted = submit_feedback(
        runtime.clone(),
        feedback(&registry, vec![observation("obs-2"), observation("obs-3")]),
    );
    assert_eq!(accepted.accepted_count, 1);
    assert!(accepted.changed_count > 0);

    let projected = runtime
        .project(MemoryProjectionRequest {
            binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "extract text from this PDF".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .expect("project with governed experience");
    assert_eq!(projected.provider_payload().agent_tool_hints().len(), 1);
    assert_eq!(
        projected.provider_payload().agent_tool_hints()[0].tool_id,
        "pdf.extract"
    );
    assert!(projected.provider_payload().agent_tool_hints()[0].host_execution_required);
    assert_eq!(projected.report().audit().agent_tool_selected_count, 1);
    assert!(
        !projected
            .report()
            .audit()
            .agent_tool_cold_start_selection_used
    );
    assert!(projected
        .provider_payload()
        .system_memory_block()
        .contains("Agent Tool Experience Hints"));
    assert!(!projected
        .provider_payload()
        .system_memory_block()
        .contains(TOOL_DESCRIPTION));
}

#[test]
fn sdk_agent_tool_projection_rejects_registry_fingerprint_drift() {
    let registry = registry();
    let runtime = runtime_with_registry(registry.clone());
    let accepted = submit_feedback(
        runtime.clone(),
        feedback(&registry, vec![observation("obs-a"), observation("obs-b")]),
    );
    assert!(accepted.changed_count > 0);

    let mut stale_ref = registry.registry_ref();
    stale_ref.fingerprint = "stale-fingerprint".to_string();
    let projected = runtime
        .project(MemoryProjectionRequest {
            binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "extract text from this PDF".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: vec![stale_ref],
        })
        .expect("project stale ref");

    assert!(projected.provider_payload().agent_tool_hints().is_empty());
    assert_eq!(projected.report().audit().agent_tool_selected_count, 0);
    assert!(projected.report().audit().agent_tool_rejection_count > 0);
}

#[test]
fn agent_tool_experience_is_exactly_isolated_by_mounted_subject_in_a_shared_store() {
    let profile = support::host_test_profile();
    let store =
        support::open_memory_store(StoreBackendConfig::in_memory(profile).expect("store config"))
            .expect("shared store");
    let registry = registry();
    let subjects = two_agent_registry();
    let agent_a = runtime_for_subject(store.clone(), subjects.clone(), registry.clone(), "agent-a");
    let agent_b = runtime_for_subject(store, subjects, registry.clone(), "agent-b");

    let accepted = submit_feedback(
        agent_a.clone(),
        feedback(
            &registry,
            vec![observation("obs-a-1"), observation("obs-a-2")],
        ),
    );
    assert!(accepted.changed_count > 0);

    let project = |runtime: &MemoryRuntime| {
        runtime
            .project(MemoryProjectionRequest {
                binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: "extract text from this PDF".to_string(),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: vec![registry.registry_ref()],
            })
            .expect("project")
    };

    assert_eq!(
        project(&agent_a)
            .provider_payload()
            .agent_tool_hints()
            .len(),
        1,
        "positive control: the owning subject must see its governed experience"
    );
    let denied = project(&agent_b);
    assert!(
        denied.provider_payload().agent_tool_hints().is_empty(),
        "a different mounted subject must not receive another subject's experience"
    );
    assert!(!denied
        .provider_payload()
        .system_memory_block()
        .contains("Agent Tool Experience Hints"));
    assert_eq!(denied.report().audit().agent_tool_selected_count, 0);
}

#[test]
fn free_text_operator_note_never_counts_as_human_confirmation() {
    let registry = registry();
    let runtime = runtime_with_registry(registry.clone());
    let unconfirmed = feedback(&registry, vec![observation("obs-note-only")]);
    let mut wire = serde_json::to_value(&unconfirmed).unwrap();
    wire["operator_note"] = "looks good to me".into();
    assert!(serde_json::from_value::<AgentToolUsageFeedbackV3>(wire).is_err());
    let report = submit_feedback(runtime.clone(), unconfirmed);
    assert_eq!(
        report.accepted_count, 1,
        "ordinary execution is accepted without pretending to be confirmed"
    );
    let recalled = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![],
            query: "extract PDF release notes".into(),
            limit: 8,
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .unwrap();
    assert!(
        recalled.agent_tool_hints.is_empty(),
        "one unconfirmed execution cannot activate a method"
    );
}

#[test]
fn historical_recall_does_not_apply_current_agent_tool_experience() {
    let registry = registry();
    let runtime = runtime_with_registry(registry.clone());
    let accepted = submit_feedback(
        runtime.clone(),
        feedback(
            &registry,
            vec![observation("obs-history-1"), observation("obs-history-2")],
        ),
    );
    assert!(accepted.changed_count > 0);

    let project = |temporal_operation| {
        runtime
            .project(MemoryProjectionRequest {
                binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
                temporal_operation,
                structured_query_facets: Vec::new(),
                user_query: "extract text from this PDF".to_string(),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: vec![registry.registry_ref()],
            })
            .expect("projection")
    };
    let current = project(bm_sdk::MemoryRecallTemporalOperation::Current);
    assert_eq!(
        current.provider_payload().agent_tool_hints().len(),
        1,
        "positive control"
    );

    let historical =
        project(bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time: 1 });
    assert!(
        historical.provider_payload().agent_tool_hints().is_empty(),
        "current experience must not leak into a historical snapshot"
    );
}

#[test]
fn agent_tool_registry_scope_mismatch_is_rejected_before_hint_selection() {
    let mut scoped = registry();
    scoped.scope = AgentToolRegistryScope::Project {
        project_id: "project-a".to_string(),
    };
    scoped.fingerprint = fingerprint_agent_tool_registry(&scoped);
    let runtime = runtime_with_registry(scoped.clone());
    let accepted = submit_feedback(
        runtime.clone(),
        feedback(
            &scoped,
            vec![observation("obs-scope-1"), observation("obs-scope-2")],
        ),
    );
    assert!(accepted.changed_count > 0);

    let project = |registry_ref| {
        runtime
            .project(MemoryProjectionRequest {
                binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                structured_query_facets: Vec::new(),
                user_query: "extract text from this PDF".to_string(),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: vec![registry_ref],
            })
            .expect("projection")
    };
    let positive = project(scoped.registry_ref());
    assert_eq!(
        positive.provider_payload().agent_tool_hints().len(),
        1,
        "positive control"
    );

    let mut mismatched_ref = scoped.registry_ref();
    mismatched_ref.scope = AgentToolRegistryScope::Project {
        project_id: "project-b".to_string(),
    };
    let denied = project(mismatched_ref);
    assert!(denied.provider_payload().agent_tool_hints().is_empty());
    assert_eq!(denied.report().audit().agent_tool_selected_count, 0);
    assert!(denied.report().audit().agent_tool_rejection_count > 0);
}
