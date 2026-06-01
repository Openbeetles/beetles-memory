mod support;

use bm_sdk::{
    default_agent_subject_id, CanonicalTurnDelta, ConversationScope, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol,
    MemoryTurnSource, MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput,
    RuntimeSkillWrite, RuntimeSkillWriteSource, TranscriptInputMessage,
};

use support::{empty_store_platform, test_runtime_with_scope};

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_sdk::IngressKind::User,
        channel: "sdk.direct".to_string(),
        provider: None,
        protocol: MemoryTurnProtocol::Native,
        endpoint: None,
        model_alias: None,
        model_resolved: None,
        request_id: Some("req-metrics-1".to_string()),
        client_conversation_hint: Some("conversation-a".to_string()),
    }
}

fn finalize_request(turn_id: &str) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: turn_id.to_string(),
            conversation: ConversationScope {
                channel: "sdk.direct".to_string(),
                chat_id: "chat-a".to_string(),
                conversation_id: Some("conversation-a".to_string()),
            },
            subject: default_agent_subject_id("agent-main"),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            actor: None,
            input_messages: vec![TranscriptInputMessage::user(
                "记住 runtime metrics 必须来自 SDK report",
            )],
            assistant_message: Some(TranscriptInputMessage::assistant("已记录。")),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        tool_calls: 0,
        runtime_skill_selected_ids: Vec::new(),
        task_learning_selected_ids: Vec::new(),
        reuse_outcome_note: String::new(),
        tool_usage_feedback: None,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

#[test]
fn runtime_metrics_report_counts_write_recall_project_finalize_and_deferred_from_events() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform.clone(), profile, "sdk.direct", "chat-a");

    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "runtime_metrics_contract".to_string(),
                topic: "runtime metrics".to_string(),
                title: "Runtime metrics contract".to_string(),
                summary: "Runtime metrics must come from MemoryRuntime reports.".to_string(),
                content: "Operator surfaces display the SDK/core metrics report.".to_string(),
                citations: vec!["runtime metrics contract".to_string()],
                source_chat_id: Some("chat-a".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write");
    runtime
        .recall(MemoryRecallRequest {
            query: "runtime metrics".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    runtime
        .project(MemoryProjectionRequest {
            user_query: "How should metrics be reported?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");
    runtime
        .finalize_turn_and_maintain(None, None, finalize_request("turn-metrics-1"))
        .expect("finalize");

    let events = platform.read_events().expect("events");
    let report = runtime.runtime_metrics_report_from_events(&events);

    assert_eq!(report.source, "core.runtime_events");
    assert_eq!(report.counters.write_count, 1);
    assert_eq!(report.counters.recall_requests, 1);
    assert_eq!(report.counters.recall_hits, 0);
    assert_eq!(report.counters.projection_requests, 1);
    assert_eq!(report.counters.projection_injections, 1);
    assert_eq!(report.counters.finalize_requests, 1);
    assert_eq!(report.counters.finalize_committed, 1);
    assert_eq!(report.counters.deferred_governance_jobs, 1);
    assert_eq!(report.budget_report_id, runtime.runtime_budget().report_id);
}

#[test]
fn operator_readiness_report_is_sdk_owned_not_ui_inferred() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "sdk.direct", "chat-a");

    let readiness = runtime.operator_readiness_report();

    assert_eq!(readiness.memory_owner, "sdk");
    assert!(readiness.write_candidate_ready);
    assert!(readiness.semantic_governance_ready);
    assert!(readiness.subject_scope_ready);
    assert!(readiness.migration_ready);
    assert!(readiness.adapter_semantics_clean);
    assert!(!readiness.host_direct_write_detected);
    assert_eq!(readiness.metrics_source, "core.runtime_events");
}
