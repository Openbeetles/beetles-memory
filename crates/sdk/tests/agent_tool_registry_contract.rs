use bm_sdk::{
    AgentToolDescriptor, AgentToolExperienceGovernanceDecision, AgentToolObservationDigest,
    AgentToolOutcome, AgentToolRegistrySnapshot, AgentToolUsageFeedback, MemoryIdentity,
    MemoryInspectionRequest, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime,
    MemoryScope, MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput,
    RuntimeSkillReuseOutcome, StoreBackendConfig, StorePlatform, AGENT_TOOL_NO_EXPERIENCE_REASON,
    AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH,
};

fn registry() -> AgentToolRegistrySnapshot {
    let mut tool = AgentToolDescriptor::compact("pdf.extract", "Extract PDF text", "schema-pdf-v1");
    tool.permission_tags = vec!["filesystem.read".to_string()];
    tool.risk_tags = vec!["external_content".to_string()];
    AgentToolRegistrySnapshot::compact("host-tools", "host", vec![tool], 1_800_000_000)
}

fn runtime_with_registry(registry: AgentToolRegistrySnapshot) -> MemoryRuntime {
    let profile = ProfileId::ServerLinuxDevFull;
    let store = StorePlatform::open(StoreBackendConfig::in_memory(profile).expect("store config"))
        .expect("store");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-tool-test", "owner-default").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "chat-1").expect("scope"))
        .profile(profile)
        .store_platform(store)
        .agent_tool_registry(registry)
        .build()
        .expect("runtime")
}

fn observation(observation_id: &str) -> AgentToolObservationDigest {
    AgentToolObservationDigest {
        observation_id: observation_id.to_string(),
        registry_id: "host-tools".to_string(),
        tool_id: "pdf.extract".to_string(),
        schema_fingerprint: "schema-pdf-v1".to_string(),
        call_id: Some(format!("call-{observation_id}")),
        task_signature: "extract_pdf_text_for_release_notes".to_string(),
        summary: "PDF extraction produced usable release note text.".to_string(),
        outcome: AgentToolOutcome::Succeeded,
        error_code: None,
        external_content: true,
        private_content_used: false,
        permission_tags: vec!["filesystem.read".to_string()],
        risk_tags: vec!["external_content".to_string()],
        started_at: Some(1_800_000_010),
        completed_at: Some(1_800_000_011),
    }
}

fn feedback(
    registry: &AgentToolRegistrySnapshot,
    observations: Vec<AgentToolObservationDigest>,
) -> AgentToolUsageFeedback {
    AgentToolUsageFeedback {
        registry_ref: registry.registry_ref(),
        observations,
        user_visible_result_summary: Some(
            "PDF extraction helped produce release notes from a local artifact.".to_string(),
        ),
        reuse_outcome: RuntimeSkillReuseOutcome::Succeeded,
        operator_note: None,
    }
}

#[test]
fn sdk_agent_tool_registry_never_cold_starts_from_tool_descriptions() {
    let registry = registry();
    let runtime = runtime_with_registry(registry.clone());

    let recall = runtime
        .recall(MemoryRecallRequest {
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
    assert_eq!(inspection.agent_tool_registry.governed_experiences, 0);
}

#[test]
fn sdk_agent_tool_feedback_requires_governed_experience_before_projection() {
    let registry = registry();
    let runtime = runtime_with_registry(registry.clone());

    let deferred = runtime
        .write(MemoryWriteRequest::AgentToolUsageFeedback {
            feedback: feedback(&registry, vec![observation("obs-1")]),
        })
        .expect("single feedback");
    let deferred_report = deferred.agent_tool_experience.expect("governance report");
    assert_eq!(
        deferred_report.decision,
        AgentToolExperienceGovernanceDecision::DeferredUntilRepeated
    );
    assert_eq!(deferred.changed, 0);

    let no_hint = runtime
        .project(MemoryProjectionRequest {
            user_query: "extract text from this PDF".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .expect("project without governed experience");
    assert!(no_hint.runtime_projection.agent_tool_hints.is_empty());
    assert!(no_hint.audit.agent_tools.selected.is_empty());

    let accepted = runtime
        .write(MemoryWriteRequest::AgentToolUsageFeedback {
            feedback: feedback(&registry, vec![observation("obs-2"), observation("obs-3")]),
        })
        .expect("repeated feedback");
    let accepted_report = accepted.agent_tool_experience.expect("governance report");
    assert!(accepted_report.accepted);
    assert_eq!(
        accepted_report.decision,
        AgentToolExperienceGovernanceDecision::AcceptedAsEvidence
    );
    assert_eq!(accepted.changed, 1);

    let projected = runtime
        .project(MemoryProjectionRequest {
            user_query: "extract text from this PDF".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: vec![registry.registry_ref()],
        })
        .expect("project with governed experience");
    assert_eq!(projected.runtime_projection.agent_tool_hints.len(), 1);
    assert_eq!(
        projected.runtime_projection.agent_tool_hints[0].tool_id,
        "pdf.extract"
    );
    assert!(projected.runtime_projection.agent_tool_hints[0].host_execution_required);
    assert_eq!(projected.audit.agent_tools.selected.len(), 1);
    assert!(!projected.audit.agent_tools.cold_start_selection_used);
    assert!(projected
        .system_memory_block
        .contains("Agent Tool Experience Hints"));
    assert!(!projected.system_memory_block.contains("Extract PDF text"));
}

#[test]
fn sdk_agent_tool_projection_rejects_registry_fingerprint_drift() {
    let registry = registry();
    let runtime = runtime_with_registry(registry.clone());
    runtime
        .write(MemoryWriteRequest::AgentToolUsageFeedback {
            feedback: feedback(&registry, vec![observation("obs-a"), observation("obs-b")]),
        })
        .expect("feedback");

    let mut stale_ref = registry.registry_ref();
    stale_ref.fingerprint = "stale-fingerprint".to_string();
    let projected = runtime
        .project(MemoryProjectionRequest {
            user_query: "extract text from this PDF".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: vec![stale_ref],
        })
        .expect("project stale ref");

    assert!(projected.runtime_projection.agent_tool_hints.is_empty());
    assert!(projected
        .audit
        .agent_tools
        .rejected
        .iter()
        .any(|rejection| rejection.reason == AGENT_TOOL_REGISTRY_FINGERPRINT_MISMATCH));
}
