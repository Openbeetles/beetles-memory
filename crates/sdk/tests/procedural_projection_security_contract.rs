#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::Arc;

use bm_sdk::*;

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
    let spec = support::procedural::runtime_observer_spec(&memory, "security-executor");
    let capability = support::procedural::register_and_issue(
        &governor,
        spec,
        &format!("register-security-executor-{}", memory.scope().chat_id),
    );
    Arc::new(TestRuntime {
        memory: Arc::new(memory),
        capability,
    })
}

const NOW: u64 = 1_800_000_000;

struct FixedClock;

impl MemoryClock for FixedClock {
    fn now_secs(&self) -> u64 {
        NOW
    }
}

#[derive(Default)]
struct NoProviderPort {
    calls: usize,
}

impl GovernanceExecutionPort for NoProviderPort {
    fn execute(
        &mut self,
        _envelope: &AuthorizedGovernanceEnvelope,
        _binding: &ImmutableGovernanceExecutionBinding,
        _egress: &GovernanceEgressAuthority,
        _operation: &mut dyn GovernanceExecutionOperation,
    ) -> std::result::Result<(), GovernanceExecutionPortFailure> {
        self.calls = self.calls.saturating_add(1);
        Err(GovernanceExecutionPortFailure::Other(
            bm_sdk::Error::config(
                "procedural_projection_security_contract",
                "local procedural learning must not invoke a Provider",
            ),
        ))
    }
}

fn applicability(
    project_id: Option<&str>,
    workspace_id: Option<&str>,
) -> ProceduralApplicabilityContextV1 {
    ProceduralApplicabilityContextV1::try_new(
        project_id.map(str::to_string),
        workspace_id.map(str::to_string),
        None,
    )
    .expect("canonical applicability")
}

fn registry(registry_id: &str, scope: AgentToolRegistryScope) -> AgentToolRegistrySnapshot {
    let mut registry = AgentToolRegistrySnapshot::compact(
        registry_id,
        "synthetic-host",
        vec![AgentToolDescriptor::compact(
            "archive.unpack",
            "Archive Unpack",
            "schema-archive-v1",
        )],
        NOW,
    );
    registry.scope = scope;
    registry.fingerprint = fingerprint_agent_tool_registry(&registry);
    registry
}

fn runtime(
    store: MemoryStoreHandle,
    chat_id: &str,
    registry: AgentToolRegistrySnapshot,
    context: ProceduralApplicabilityContextV1,
) -> Arc<TestRuntime> {
    authorized_runtime(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
            .subject_id("subject-default")
            .scope(MemoryScope::new("llm.gateway", chat_id).expect("scope"))
            .store(store.clone())
            .clock(Arc::new(FixedClock))
            .capability_policy(MemoryCapabilityPolicy::strict_profile())
            .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
            .audit_sink(Arc::new(NoopMemoryAuditSink))
            .procedural_applicability_context(context)
            .agent_tool_registry(registry)
            .build()
            .expect("runtime"),
        store,
    )
}

fn feedback(
    registry: &AgentToolRegistrySnapshot,
    task: &str,
    marker: &str,
) -> AgentToolUsageFeedbackV3 {
    let facts = ["1", "2"]
        .into_iter()
        .map(|suffix| ToolExecutionFactV1 {
            observation_id: format!("observation-{task}-{suffix}"),
            call_id: format!("call-{task}-{suffix}"),
            outcome: ToolExecutionOutcome::Succeeded,
            source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
            started_at: Some(NOW),
            completed_at: Some(NOW),
        })
        .collect::<Vec<_>>();
    AgentToolUsageFeedbackV3 { registry_ref: registry.registry_ref(), tool_id: "archive.unpack".into(),
        schema_fingerprint: "schema-archive-v1".into(), method_evidence: vec![ToolMethodEvidenceV1 {
            method_id: format!("method-{task}"), task_signature: task.into(),
            body: format!("1. Inspect and validate the input for {marker}\n2. Execute the scoped operation\n3. Verify the resulting output"),
            execution_refs: facts.iter().map(|fact| fact.observation_id.clone()).collect(),
            source_sensitivity: ProceduralSourceSensitivity::NonPrivate, external_content: false,
        }], execution_facts: facts }
}

fn finalize_request(
    runtime: &MemoryRuntime,
    turn_id: &str,
    registry: &AgentToolRegistrySnapshot,
    feedback: AgentToolUsageFeedbackV3,
) -> MemoryTurnFinalizeRequest {
    let tool_call_count = feedback.execution_facts.len() as u32;
    let tool_observations =
        support::procedural::canonical_observations(&feedback.tool_id, &feedback.execution_facts);
    assert_eq!(feedback.registry_ref, registry.registry_ref());
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn_id.to_string(),
            conversation: ConversationScope {
                channel: runtime.scope().channel.clone(),
                chat_id: runtime.scope().chat_id.clone(),
                conversation_id: Some(runtime.scope().conversation_id_or_chat_id().to_string()),
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
            input_messages: vec![TranscriptInputMessage::user("synthetic tool-use turn")],
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
    }
}

fn finish_procedural_learning(runtime: Arc<TestRuntime>, expected_job_id: &str) {
    let engine =
        MemoryLearningEngine::attach(runtime.memory.clone()).expect("attach learning engine");
    let mut port = NoProviderPort::default();
    for attempt in 0..4 {
        let outcome = engine
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: format!("procedural-security-worker-{attempt}"),
                    lease_duration_secs: 60,
                },
                &mut port,
            )
            .expect("learning cycle");
        if let MemoryLearningCycleOutcome::ProceduralCompleted(report) = outcome {
            assert_eq!(report.job.job_id, expected_job_id);
            assert_eq!(port.calls, 0, "procedural lane is local and deterministic");
            return;
        }
    }
    panic!("procedural job did not complete within bounded cycles");
}

fn learn(
    runtime: Arc<TestRuntime>,
    registry: &AgentToolRegistrySnapshot,
    turn_id: &str,
    feedback: AgentToolUsageFeedbackV3,
) {
    let report = runtime
        .finalize_turn_with_procedural_evidence(
            &runtime.capability,
            finalize_request(&runtime, turn_id, registry, feedback),
        )
        .expect("production finalize");
    let job_id = report
        .procedural_learning
        .job_id
        .expect("durable procedural job");
    finish_procedural_learning(runtime, &job_id);
}

fn projection(
    runtime: &MemoryRuntime,
    query: &str,
    registry_ref: bm_sdk::AgentToolRegistryRef,
) -> bm_sdk::MemoryProjectionOutput {
    runtime
        .project(MemoryProjectionRequest {
            binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
            temporal_operation: MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: query.to_string(),
            system_max_len: 8192,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: vec![registry_ref],
        })
        .expect("projection")
}

#[test]
fn production_tool_experience_selection_is_task_relevant() {
    let store = support::empty_store_platform(support::host_test_profile());
    let registry = registry("task-relevance-tools", AgentToolRegistryScope::Global);
    let runtime = runtime(store, "chat-a", registry.clone(), applicability(None, None));
    for (task, marker) in [
        (
            "invoice_extract",
            "INVOICE_RELEVANCE_MARKER invoice extraction completed",
        ),
        (
            "database_rotation",
            "DATABASE_ROTATION_MARKER database snapshot rotation completed",
        ),
    ] {
        learn(
            Arc::clone(&runtime),
            &registry,
            &format!("turn-{task}"),
            feedback(&registry, task, marker),
        );
    }

    let related = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "invoice extraction".to_string(),
            limit: 8,
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .expect("related recall");
    assert_eq!(
        related.agent_tool_hints.len(),
        1,
        "nonempty positive control"
    );
    assert!(related.agent_tool_hints[0]
        .reason
        .contains("INVOICE_RELEVANCE_MARKER"));
    assert!(!related.agent_tool_hints[0]
        .reason
        .contains("DATABASE_ROTATION_MARKER"));

    let unrelated = projection(
        &runtime,
        "weather umbrella forecast",
        registry.registry_ref(),
    );
    assert!(unrelated.provider_payload().agent_tool_hints().is_empty());
    assert!(unrelated.report().agent_tool_hints().is_empty());
    assert!(!unrelated
        .provider_payload()
        .system_memory_block()
        .contains("INVOICE_RELEVANCE_MARKER"));
    assert!(!unrelated
        .provider_payload()
        .system_memory_block()
        .contains("DATABASE_ROTATION_MARKER"));
    assert!(unrelated.report().audit().agent_tool_rejection_count >= 2);
}

#[test]
fn scoped_tool_experience_is_exactly_denied_outside_each_applicability() {
    let cases = [
        (
            "project",
            AgentToolRegistryScope::Project {
                project_id: "project-a".to_string(),
            },
            applicability(Some("project-a"), None),
            applicability(Some("project-b"), None),
            "chat-a",
            "chat-a",
        ),
        (
            "workspace",
            AgentToolRegistryScope::Workspace {
                workspace_id: "workspace-a".to_string(),
            },
            applicability(None, Some("workspace-a")),
            applicability(None, Some("workspace-b")),
            "chat-a",
            "chat-a",
        ),
        (
            "conversation",
            AgentToolRegistryScope::Conversation {
                conversation_id: "chat-a".to_string(),
            },
            applicability(None, None),
            applicability(None, None),
            "chat-a",
            "chat-b",
        ),
    ];

    for (label, scope, admitted_context, denied_context, admitted_chat, denied_chat) in cases {
        let store = support::empty_store_platform(support::host_test_profile());
        let registry = registry(&format!("scoped-{label}-tools"), scope);
        let admitted = runtime(
            store.clone(),
            admitted_chat,
            registry.clone(),
            admitted_context,
        );
        learn(
            Arc::clone(&admitted),
            &registry,
            &format!("turn-{label}"),
            feedback(
                &registry,
                &format!("{label}_task"),
                &format!("SCOPED_{label}_MARKER authorized scoped execution"),
            ),
        );
        let positive = projection(
            &admitted,
            "authorized scoped execution",
            registry.registry_ref(),
        );
        assert_eq!(
            positive.provider_payload().agent_tool_hints().len(),
            1,
            "{label} positive control"
        );

        let denied = runtime(store, denied_chat, registry.clone(), denied_context);
        let output = projection(
            &denied,
            "authorized scoped execution",
            registry.registry_ref(),
        );
        assert!(
            output.provider_payload().agent_tool_hints().is_empty(),
            "{label}"
        );
        assert!(output.report().agent_tool_hints().is_empty(), "{label}");
        assert!(
            !output
                .provider_payload()
                .system_memory_block()
                .contains(&format!("SCOPED_{label}_MARKER")),
            "{label} body exact-zero"
        );
        assert!(
            output.report().audit().agent_tool_rejection_count > 0,
            "{label} must retain only a safe rejection count"
        );
    }
}

#[test]
fn unknown_registry_fingerprint_is_denied_with_safe_audit_only() {
    let store = support::empty_store_platform(support::host_test_profile());
    let registry = registry("fingerprint-tools", AgentToolRegistryScope::Global);
    let runtime = runtime(store, "chat-a", registry.clone(), applicability(None, None));
    learn(
        Arc::clone(&runtime),
        &registry,
        "turn-fingerprint",
        feedback(
            &registry,
            "fingerprint_task",
            "FINGERPRINT_SENTINEL verified archive operation",
        ),
    );
    let positive = projection(
        &runtime,
        "verified archive operation",
        registry.registry_ref(),
    );
    assert_eq!(positive.provider_payload().agent_tool_hints().len(), 1);

    let mut unknown = registry.registry_ref();
    unknown.fingerprint = "unknown-registry-fingerprint".to_string();
    let denied = projection(&runtime, "verified archive operation", unknown);
    assert!(denied.provider_payload().agent_tool_hints().is_empty());
    assert!(denied.report().agent_tool_hints().is_empty());
    assert!(!denied
        .provider_payload()
        .system_memory_block()
        .contains("FINGERPRINT_SENTINEL"));
    assert!(denied.report().audit().agent_tool_rejection_count > 0);
}

#[test]
fn noncanonical_registry_is_rejected_before_builder_or_upsert_mutates_state() {
    let store = support::empty_store_platform(support::host_test_profile());
    let mut noncanonical = registry(
        "noncanonical-tools",
        AgentToolRegistryScope::Project {
            project_id: " project-a".to_string(),
        },
    );
    noncanonical.fingerprint = fingerprint_agent_tool_registry(&noncanonical);
    let before = store.export_replay_snapshot().expect("before snapshot");
    let build = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .subject_id("subject-default")
        .scope(MemoryScope::new("llm.gateway", "chat-a").expect("scope"))
        .store(store.clone())
        .clock(Arc::new(FixedClock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .procedural_applicability_context(applicability(Some("project-a"), None))
        .agent_tool_registry(noncanonical.clone())
        .build();
    let build_error = build
        .err()
        .expect("builder must reject noncanonical registry scope");
    assert!(matches!(
        build_error,
        bm_sdk::Error::Config {
            stage: "agent_tool_registry",
            ref message,
        } if message.contains("agent_tool_registry_scope_noncanonical")
    ));
    let after_build = store
        .export_replay_snapshot()
        .expect("after build snapshot");
    assert_eq!(
        before, after_build,
        "failed builder must make zero mutations"
    );

    let valid_registry = registry("valid-tools", AgentToolRegistryScope::Global);
    let valid = runtime(
        store.clone(),
        "chat-a",
        valid_registry,
        applicability(Some("project-a"), None),
    );
    let before_upsert = store
        .export_replay_snapshot()
        .expect("before upsert snapshot");
    let upsert_error = valid
        .upsert_agent_tool_registry(noncanonical)
        .expect_err("upsert must reject noncanonical registry scope");
    assert!(matches!(
        upsert_error,
        bm_sdk::Error::Config {
            stage: "agent_tool_registry",
            ref message,
        } if message.contains("agent_tool_registry_scope_noncanonical")
    ));
    let after_upsert = store
        .export_replay_snapshot()
        .expect("after upsert snapshot");
    assert_eq!(
        before_upsert, after_upsert,
        "failed upsert must make zero Store mutations"
    );
}
