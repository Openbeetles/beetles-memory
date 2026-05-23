mod support;

use bm_sdk::{
    IngressKind, MemoryCloseRequest, MemoryInspectionRequest, MemoryMaintenanceRequest,
    MemoryProjectionRequest, MemoryRecallRequest, MemoryRecoverRequest, MemoryWriteRequest,
    PressureLevel, ProfileId, RuntimeLifecycleDisposition, RuntimeLifecycleModeInput,
    RuntimeLifecycleOperation, RuntimeLifecycleTrigger, RuntimeSkillReuseOutcome,
    RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig, StorePlatform,
};

use support::{empty_store_platform, test_runtime, StaticHttpClient, StaticLlmClient};

#[test]
fn runtime_lifecycle_reports_wrap_sdk_operations() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "lifecycle_contract".to_string(),
                topic: "runtime lifecycle".to_string(),
                title: "Runtime lifecycle contract".to_string(),
                summary: "Every SDK operation carries a lifecycle report.".to_string(),
                content: "Call MemoryRuntime and consume structured reports.".to_string(),
                citations: vec!["runtime lifecycle contract test".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write");
    assert_eq!(
        write.lifecycle_report.operation,
        RuntimeLifecycleOperation::Maintain
    );
    assert_eq!(
        write.lifecycle_report.trigger,
        RuntimeLifecycleTrigger::SdkCall
    );
    assert!(write.lifecycle_report.success);

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "runtime lifecycle".to_string(),
            limit: 4,
        })
        .expect("recall");
    assert_eq!(
        recall.lifecycle_report.operation,
        RuntimeLifecycleOperation::Inspect
    );
    assert!(recall.lifecycle_report.success);

    let projection = runtime
        .project(MemoryProjectionRequest {
            user_query: "How should SDK hosts use lifecycle?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("project");
    assert_eq!(
        projection.lifecycle_report.operation,
        RuntimeLifecycleOperation::Project
    );
    assert!(projection.lifecycle_report.success);
}

#[test]
fn runtime_lifecycle_events_record_memory_hit_telemetry_for_recall_and_projection() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");
    let event_reader = platform.clone();
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "ollama_memory_projection_hit".to_string(),
                topic: "ollama transparent metrics".to_string(),
                title: "Ollama transparent metrics".to_string(),
                summary: "Ollama transparent projection must be counted as a memory hit."
                    .to_string(),
                content: "When projection injects remembered context, the runtime event records memory_hit=true."
                    .to_string(),
                citations: vec!["runtime lifecycle telemetry contract".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write");

    runtime
        .recall(MemoryRecallRequest {
            query: "ollama transparent metrics".to_string(),
            limit: 4,
        })
        .expect("recall");
    runtime
        .project(MemoryProjectionRequest {
            user_query: "How should Ollama transparent metrics be counted?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("project");

    let events = event_reader.read_events().expect("events");
    let recall_event = events
        .iter()
        .find(|event| {
            event.payload.get("result_summary").map(String::as_str) == Some("recall_completed")
        })
        .expect("recall lifecycle event");
    assert_eq!(
        recall_event.payload.get("memory_hit").map(String::as_str),
        Some("true")
    );
    assert!(recall_event
        .payload
        .get("hit_count")
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|count| count > 0));

    let project_event = events
        .iter()
        .find(|event| {
            event.payload.get("result_summary").map(String::as_str) == Some("projection_completed")
        })
        .expect("projection lifecycle event");
    assert_eq!(
        project_event.payload.get("memory_hit").map(String::as_str),
        Some("true")
    );
    assert!(project_event
        .payload
        .get("hit_count")
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|count| count > 0));
    assert!(project_event
        .payload
        .get("system_memory_chars")
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|chars| chars > 0));
}

#[test]
fn runtime_lifecycle_maintenance_defer_does_not_run_core_passes() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);
    let llm = StaticLlmClient::summary_response("Summary: should not be used");
    let mut http = StaticHttpClient;

    let maintenance = runtime
        .maintain(
            &mut http,
            &llm,
            MemoryMaintenanceRequest {
                ingress: IngressKind::User,
                user_content: "remember lifecycle pressure".to_string(),
                reply_content: "maintenance should defer".to_string(),
                tool_calls: 0,
                external_content_used: false,
                runtime_skill_selected_ids: Vec::new(),
                task_learning_selected_ids: Vec::new(),
                reuse_outcome: RuntimeSkillReuseOutcome::Neutral,
                reuse_outcome_note: String::new(),
                pressure: PressureLevel::Critical,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .expect("deferred maintenance report");

    assert_eq!(
        maintenance.lifecycle_report.admission.disposition,
        RuntimeLifecycleDisposition::Defer
    );
    assert_eq!(
        maintenance.lifecycle_report.admission.reason,
        "critical_pressure"
    );
    assert!(maintenance.report.is_none());
}

#[test]
fn runtime_lifecycle_inspect_recover_and_close_are_sdk_level_operations() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");
    let event_reader = platform.clone();
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    let inspection = runtime
        .inspect(MemoryInspectionRequest {
            query: "lifecycle".to_string(),
            system_max_len: 4096,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("inspection");
    assert_eq!(
        inspection.lifecycle_report.operation,
        RuntimeLifecycleOperation::Inspect
    );
    assert!(inspection.operator_action_report.accepted);
    assert!(inspection
        .operator_action_report
        .diagnosis
        .safe_actions_available
        .iter()
        .any(|action| action == "inspect_memory_status"));

    let recover = runtime
        .recover(MemoryRecoverRequest {
            trigger: RuntimeLifecycleTrigger::BootRecovery,
            mode_input: RuntimeLifecycleModeInput {
                recovery_safe_mode_active: true,
                ..RuntimeLifecycleModeInput::default()
            },
        })
        .expect("recover");
    assert_eq!(
        recover.lifecycle_report.operation,
        RuntimeLifecycleOperation::Recover
    );
    assert!(recover.lifecycle_report.success);

    let close = runtime
        .close(MemoryCloseRequest {
            reason: "test close".to_string(),
        })
        .expect("close");
    assert_eq!(
        close.lifecycle_report.operation,
        RuntimeLifecycleOperation::Close
    );
    assert!(close.lifecycle_report.success);

    let events = event_reader.read_events().expect("events");
    assert!(events
        .iter()
        .any(|event| event.kind_name == "runtime.lifecycle"
            && event
                .payload
                .get("operation")
                .is_some_and(|value| value == "close")));
    assert!(events
        .iter()
        .any(|event| event.kind_name == "operator.action"
            && event
                .payload
                .get("operation")
                .is_some_and(|value| value == "inspect")));
}

#[test]
fn runtime_lifecycle_recover_respects_profile_capability() {
    let platform = empty_store_platform(ProfileId::EspEmbeddedSdk);
    let runtime = test_runtime(platform, ProfileId::EspEmbeddedSdk);

    let error = match runtime.recover(MemoryRecoverRequest {
        trigger: RuntimeLifecycleTrigger::BootRecovery,
        mode_input: RuntimeLifecycleModeInput::default(),
    }) {
        Ok(_) => panic!("embedded SDK profile does not expose recover"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "memory_runtime_operation");
    assert!(!runtime.capabilities().lifecycle.recover.visible);
}
