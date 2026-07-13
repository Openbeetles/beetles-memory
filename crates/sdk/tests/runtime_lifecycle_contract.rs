#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{memory_facet_manifest_key, MEMORY_FACET_POSTING_NAMESPACE};
use bm_core::platform::Platform as _;
use bm_core::runtime::continuity_flush::{
    ContinuitySnapshotBundle, REL_PATH_REBOOT_CONTINUITY_BUNDLE,
};
use bm_sdk::{
    IngressKind, MemoryCloseRequest, MemoryExportRequest, MemoryInspectionRequest,
    MemoryMaintenanceRequest, MemoryProjectionRequest, MemoryRecallRequest, MemoryRecoverRequest,
    MemoryStoreHandle, MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleDisposition,
    RuntimeLifecycleModeInput, RuntimeLifecycleOperation, RuntimeLifecycleTrigger,
    RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
    StoreRuntimeBudget,
};

use support::{
    empty_store_platform, seeded_store_platform, test_runtime, test_runtime_with_identity_scope,
    StaticHttpClient, StaticLlmClient,
};

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
            structured_query_facets: Vec::new(),
            query: "runtime lifecycle".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    assert_eq!(
        recall.lifecycle_report.operation,
        RuntimeLifecycleOperation::Recall
    );
    assert!(recall.lifecycle_report.success);

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "How should SDK hosts use lifecycle?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
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
    let platform = MemoryStoreHandle::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");
    let event_reader = platform.clone();
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "ollama_memory_projection_hit".to_string(),
                topic: "ollama transparent metrics".to_string(),
                title: "Ollama transparent metrics".to_string(),
                summary: "Ollama transparent projection must be counted as a memory hit."
                    .to_string(),
                content: "- When projection injects remembered context, record memory_hit=true.\n- Use hit_count to back UI metrics instead of fabricating counters."
                    .to_string(),
                citations: vec!["runtime lifecycle telemetry contract".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write");
    assert!(
        write.accepted,
        "telemetry fixture must seed recallable memory"
    );

    runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "ollama transparent metrics".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "How should Ollama transparent metrics be counted?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    let events = event_reader.replay_harness().read_events().expect("events");
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
    let platform = MemoryStoreHandle::open_in_memory(
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

    let events = event_reader.replay_harness().read_events().expect("events");
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
fn runtime_recover_commits_bundle_owner_facet_soul_and_lifecycle_atomically() {
    let source_platform = seeded_store_platform(ProfileId::ServerLinuxDevFull);
    let source_runtime = test_runtime(source_platform, ProfileId::ServerLinuxDevFull);
    let snapshot = source_runtime
        .export(MemoryExportRequest {
            chat_id: "chat-1".to_string(),
        })
        .expect("export recovery snapshot")
        .snapshot;

    let target_platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    target_platform
        .replay_harness()
        .session_store()
        .append("chat-target", "user", "recover this inhabited subject")
        .expect("seed degraded session");
    let bundle = ContinuitySnapshotBundle {
        version: 1,
        reason: "test_recovery".to_string(),
        flushed_at: 1_800_000_000,
        primary_chat_id: Some("chat-target".to_string()),
        snapshots: vec![snapshot],
    };
    target_platform
        .replay_harness()
        .state_fs()
        .write(
            REL_PATH_REBOOT_CONTINUITY_BUNDLE,
            &serde_json::to_vec(&bundle).expect("serialize recovery bundle"),
        )
        .expect("write recovery bundle");
    let runtime = test_runtime_with_identity_scope(
        target_platform.clone(),
        ProfileId::ServerLinuxDevFull,
        "target-agent",
        "target-owner",
        "llm.gateway",
        "chat-target",
    );

    let report = runtime
        .recover(MemoryRecoverRequest {
            trigger: RuntimeLifecycleTrigger::BootRecovery,
            mode_input: RuntimeLifecycleModeInput {
                recovery_safe_mode_active: true,
                ..RuntimeLifecycleModeInput::default()
            },
        })
        .expect("recover from runtime bundle");

    let transaction = report.transaction.expect("recovery transaction proof");
    assert_eq!(transaction.operation, "recover.soul_kernel");
    assert_eq!(
        transaction.planned_mutations,
        transaction.committed_mutations
    );
    assert!(!transaction.partial_write);
    assert_eq!(report.report.restored_snapshots, 1);
    assert!(report
        .report
        .restored_layers
        .iter()
        .any(|layer| layer == "key_memory"));
    assert_eq!(
        target_platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:target-owner")
            .expect("target owner store")
            .count()
            .expect("target owner count"),
        1
    );
    let manifest_key = memory_facet_manifest_key("space:target-owner", runtime.subject_id())
        .expect("target manifest key");
    assert_eq!(
        target_platform
            .replay_harness()
            .read_json_docs_by_keys(
                MEMORY_FACET_POSTING_NAMESPACE,
                std::slice::from_ref(&manifest_key),
            )
            .expect("target facet manifest")
            .len(),
        1
    );
}

#[test]
fn runtime_recover_budget_failure_leaves_owner_facet_and_events_unchanged() {
    let source_platform = seeded_store_platform(ProfileId::ServerLinuxDevFull);
    let source_runtime = test_runtime(source_platform, ProfileId::ServerLinuxDevFull);
    let snapshot = source_runtime
        .export(MemoryExportRequest {
            chat_id: "chat-1".to_string(),
        })
        .expect("export recovery snapshot")
        .snapshot;

    let budget = StoreRuntimeBudget {
        event_log_max_items: 4,
        kv_max_entries: 64,
        blob_max_bytes: 4096,
        snapshot_max_bytes: 131_072,
        logical_namespace_max_bytes: 128,
        logical_key_max_bytes: 1024,
        event_record_key_max_bytes: 1024,
        export_max_bytes: 131_072,
        import_max_bytes: 131_072,
    };
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("store config")
        .with_runtime_store_budget(budget);
    let target_platform = MemoryStoreHandle::open_in_memory(config).expect("target store");
    target_platform
        .replay_harness()
        .session_store()
        .append("chat-target", "user", "recover this inhabited subject")
        .expect("seed degraded session");
    target_platform
        .replay_harness()
        .state_fs()
        .write(
            REL_PATH_REBOOT_CONTINUITY_BUNDLE,
            &serde_json::to_vec(&ContinuitySnapshotBundle {
                version: 1,
                reason: "test_recovery_failure".to_string(),
                flushed_at: 1_800_000_000,
                primary_chat_id: Some("chat-target".to_string()),
                snapshots: vec![snapshot],
            })
            .expect("serialize recovery bundle"),
        )
        .expect("write recovery bundle");
    let runtime = test_runtime_with_identity_scope(
        target_platform.clone(),
        ProfileId::ServerLinuxDevFull,
        "target-agent",
        "target-owner",
        "llm.gateway",
        "chat-target",
    );
    let events_before = target_platform
        .replay_harness()
        .read_events()
        .expect("events before");

    let error = match runtime.recover(MemoryRecoverRequest {
        trigger: RuntimeLifecycleTrigger::BootRecovery,
        mode_input: RuntimeLifecycleModeInput::default(),
    }) {
        Ok(_) => panic!("recovery transaction must fail preflight"),
        Err(error) => error,
    };

    assert_eq!(error.stage(), "memory_write_transaction_preflight_failed");
    assert_eq!(
        target_platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:target-owner")
            .expect("target owner store")
            .count()
            .expect("target owner count"),
        0
    );
    let manifest_key = memory_facet_manifest_key("space:target-owner", runtime.subject_id())
        .expect("target manifest key");
    assert!(target_platform
        .replay_harness()
        .read_json_docs_by_keys(
            MEMORY_FACET_POSTING_NAMESPACE,
            std::slice::from_ref(&manifest_key),
        )
        .expect("target facet manifest")
        .is_empty());
    assert_eq!(
        target_platform
            .replay_harness()
            .read_events()
            .expect("events after"),
        events_before
    );
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
