#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::Arc;

use bm_sdk::{
    fingerprint_agent_tool_registry, AgentToolDescriptor, AgentToolObservationDigest,
    AgentToolOutcome, AgentToolRegistryScope, AgentToolRegistrySnapshot, AgentToolUsageFeedbackV2,
    AuthorizedGovernanceEnvelope, CanonicalTurnDelta, ConversationScope, GovernanceEgressAuthority,
    GovernanceExecutionOperation, GovernanceExecutionPort, GovernanceExecutionPortFailure,
    ImmutableGovernanceExecutionBinding, MemoryCapabilityPolicy, MemoryClock, MemoryIdentity,
    MemoryLearningCycleOutcome, MemoryLearningCycleRequest, MemoryLearningEngine,
    MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryRecallTemporalOperation, MemoryRuntime, MemoryScope, MemoryStoreHandle,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    NoopMemoryAuditSink, PostTurnLearningInputV1, PressureLevel, ProceduralApplicabilityContextV1,
    ProceduralExecutionOutcomeV1, RuntimeLifecycleModeInput, TranscriptInputMessage,
};

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
) -> Arc<MemoryRuntime> {
    Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
            .subject_id("subject-default")
            .scope(MemoryScope::new("llm.gateway", chat_id).expect("scope"))
            .store(store)
            .clock(Arc::new(FixedClock))
            .capability_policy(MemoryCapabilityPolicy::strict_profile())
            .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
            .audit_sink(Arc::new(NoopMemoryAuditSink))
            .procedural_applicability_context(context)
            .agent_tool_registry(registry)
            .build()
            .expect("runtime"),
    )
}

fn observation(
    registry: &AgentToolRegistrySnapshot,
    task: &str,
    marker: &str,
    suffix: &str,
) -> AgentToolObservationDigest {
    AgentToolObservationDigest {
        observation_id: format!("observation-{task}-{suffix}"),
        registry_id: registry.registry_id.clone(),
        tool_id: "archive.unpack".to_string(),
        schema_fingerprint: "schema-archive-v1".to_string(),
        call_id: Some(format!("call-{task}-{suffix}")),
        task_signature: task.to_string(),
        summary: marker.to_string(),
        outcome: AgentToolOutcome::Succeeded,
        error_code: None,
        external_content: false,
        private_content_used: false,
        permission_tags: Vec::new(),
        risk_tags: Vec::new(),
        started_at: Some(NOW),
        completed_at: Some(NOW + 1),
    }
}

fn finalize_request(
    runtime: &MemoryRuntime,
    turn_id: &str,
    registry: &AgentToolRegistrySnapshot,
    observations: Vec<AgentToolObservationDigest>,
) -> MemoryTurnFinalizeRequest {
    let tool_call_count = observations.len() as u32;
    let tool_observations = observations
        .iter()
        .map(|observation| bm_sdk::ToolObservationDigest {
            observation_id: observation.observation_id.clone(),
            tool_name: observation.tool_id.clone(),
            summary: observation.summary.clone(),
            external_content: observation.external_content,
        })
        .collect();
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
        learning: PostTurnLearningInputV1 {
            tool_call_count,
            selection_receipt: None,
            runtime_skill_feedback: Vec::new(),
            agent_skill_feedback: Vec::new(),
            task_learning_feedback: Vec::new(),
            agent_tool_feedback: vec![AgentToolUsageFeedbackV2 {
                registry_ref: registry.registry_ref(),
                tool_id: "archive.unpack".to_string(),
                schema_fingerprint: "schema-archive-v1".to_string(),
                observations,
                outcome: ProceduralExecutionOutcomeV1::Succeeded,
                user_visible_result_summary: None,
                operator_note: None,
            }],
            authority: bm_sdk::ProceduralFeedbackAuthorityInputV1::HostRuntimeObservation,
        },
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn finish_procedural_learning(runtime: Arc<MemoryRuntime>, expected_job_id: &str) {
    let engine = MemoryLearningEngine::attach(runtime).expect("attach learning engine");
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
    runtime: Arc<MemoryRuntime>,
    registry: &AgentToolRegistrySnapshot,
    turn_id: &str,
    observations: Vec<AgentToolObservationDigest>,
) {
    let report = runtime
        .finalize_turn(finalize_request(&runtime, turn_id, registry, observations))
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
        let observations = ["1", "2"]
            .into_iter()
            .map(|suffix| observation(&registry, task, marker, suffix))
            .collect();
        learn(
            Arc::clone(&runtime),
            &registry,
            &format!("turn-{task}"),
            observations,
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
            ["1", "2"]
                .into_iter()
                .map(|suffix| {
                    observation(
                        &registry,
                        &format!("{label}_task"),
                        &format!("SCOPED_{label}_MARKER authorized scoped execution"),
                        suffix,
                    )
                })
                .collect(),
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
        ["1", "2"]
            .into_iter()
            .map(|suffix| {
                observation(
                    &registry,
                    "fingerprint_task",
                    "FINGERPRINT_SENTINEL verified archive operation",
                    suffix,
                )
            })
            .collect(),
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
