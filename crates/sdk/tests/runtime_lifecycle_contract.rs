#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    memory_facet_manifest_key, ContinuitySnapshot, ContinuitySnapshotManifest,
    ContinuitySnapshotMode, SelfAuthoredCore, MEMORY_FACET_POSTING_NAMESPACE,
};
use bm_core::platform::Platform as _;
use bm_core::runtime::continuity_flush::{
    ContinuitySnapshotBundle, REL_PATH_REBOOT_CONTINUITY_BUNDLE,
};
use bm_sdk::{
    default_agent_subject_id, default_memory_space_id, primary_human_subject_id,
    system_governor_subject_id, IngressKind, LongTermMemoryProvenance, MemoryCloseRequest,
    MemoryEvidenceAuthority, MemoryIdentity, MemoryInspectionRequest, MemoryMaintenanceRequest,
    MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest, MemoryRecoverRequest,
    MemoryRuntime, MemoryScope, MemoryStoreHandle, MemorySubjectVisibilityPolicy,
    MemoryWriteRequest, ParsedLongTermMemoryExtraction, PressureLevel, ProfileId,
    RuntimeLifecycleDisposition, RuntimeLifecycleModeInput, RuntimeLifecycleOperation,
    RuntimeLifecycleTrigger, RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteSource,
    StoreBackendConfig, StoreRuntimeBudget, SubjectRegistry, SubjectRelationshipGraph,
    SubjectScopedRuntime, SubjectSoulFoundingCharterSeedV1, SubjectSoulProvisionIntentV1,
    SubjectSoulReadSelectorV1,
};

use support::{
    empty_store_platform, seeded_store_platform, test_runtime, test_runtime_with_identity_scope,
    StaticHttpClient, StaticLlmClient,
};

fn runtime_mounted_to_subject(store: MemoryStoreHandle, mounted_subject_id: &str) -> MemoryRuntime {
    let owner_id = "owner-non-soul-mount";
    let agent_id = "agent-main";
    let registry = SubjectRegistry::single_agent_default(owner_id, agent_id).expect("registry");
    let relationship_graph =
        SubjectRelationshipGraph::single_agent_default(&registry).expect("relationship graph");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent_id, owner_id).expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(store)
        .subject_registry(registry)
        .subject_relationship_graph(relationship_graph)
        .scoped_runtime(SubjectScopedRuntime {
            memory_space_id: default_memory_space_id(owner_id),
            mounted_subject_id: mounted_subject_id.to_string(),
            actor_subject_id: mounted_subject_id.to_string(),
            agent_id: agent_id.to_string(),
            relationship_scope: None,
            projection_policy: "subject_aware_default".to_string(),
            write_policy: "subject_candidate_then_space_governance".to_string(),
        })
        .build()
        .expect("runtime with non-Soul mounted subject")
}

fn recovery_snapshot(
    source: &MemoryStoreHandle,
    memory_space_id: &str,
    subject_id: &str,
    chat_id: &str,
) -> ContinuitySnapshot {
    let long_term_memory = source
        .replay_harness()
        .memory_space_long_term_memory_read_store(memory_space_id)
        .expect("recovery source store")
        .list(usize::MAX)
        .expect("recovery source entries");
    ContinuitySnapshot {
        version: 6,
        exported_at: 1_800_000_000,
        mode: ContinuitySnapshotMode::FullRestore,
        memory_space_id: memory_space_id.to_string(),
        chat_id: chat_id.to_string(),
        subject_id: subject_id.to_string(),
        manifest: ContinuitySnapshotManifest::default(),
        summary_text: None,
        summary_message_count: None,
        long_term_memory,
        self_model: None,
        self_authored_core: None,
        core_revision_ledger: None,
        self_continuity: None,
        relationship_portfolio: None,
        relationship_constitution: None,
        execution_state: None,
    }
}

#[test]
fn runtime_lifecycle_reports_wrap_sdk_operations() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform, support::host_test_profile());

    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                name: "lifecycle_contract".to_string(),
                topic: "runtime lifecycle".to_string(),
                title: "Runtime lifecycle contract".to_string(),
                summary: "Every SDK operation carries a lifecycle report.".to_string(),
                content: "Call MemoryRuntime and consume structured reports.".to_string(),
                citations: vec!["runtime lifecycle contract test".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            })],
            owning_scope: support::runtime_skill_subject_scope(),
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
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
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
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
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
        projection.report().lifecycle_report().operation,
        RuntimeLifecycleOperation::Project
    );
    assert!(projection.report().lifecycle_report().success);
}

#[test]
fn runtime_lifecycle_events_record_memory_hit_telemetry_for_recall_and_projection() {
    let platform = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).expect("config"),
    )
    .expect("store");
    let event_reader = platform.clone();
    let runtime = test_runtime(platform, support::host_test_profile());

    let write = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![bm_sdk::LongTermMemoryDraft {
                    kind: bm_sdk::LongTermMemoryKind::Fact,
                    topic: "ollama transparent metrics".to_string(),
                    content: "When projection injects remembered context, record memory_hit=true."
                        .to_string(),
                    keywords: vec!["ollama".to_string(), "metrics".to_string()],
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    source_chat_id: Some("chat-1".to_string()),
                    source_type: None,
                    source_scope: None,
                    subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
                    provenance: LongTermMemoryProvenance::new(
                        MemoryEvidenceAuthority::ProgramMemoryCanonical,
                    ),
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["runtime lifecycle telemetry contract".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("write");
    assert!(
        write.accepted,
        "telemetry fixture must seed recallable memory"
    );

    runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "ollama transparent metrics".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
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
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform, support::host_test_profile());
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
    let platform = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).expect("config"),
    )
    .expect("store");
    let event_reader = platform.clone();
    let runtime = test_runtime(platform, support::host_test_profile());

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
fn runtime_recover_rejects_human_and_system_governor_mounts_before_store_access() {
    let profile = support::host_test_profile();
    let owner_id = "owner-non-soul-mount";
    for mounted_subject_id in [
        primary_human_subject_id(owner_id),
        system_governor_subject_id(owner_id),
    ] {
        let store = empty_store_platform(profile);
        let event_reader = store.clone();
        let runtime = runtime_mounted_to_subject(store, &mounted_subject_id);
        let events_before = event_reader
            .replay_harness()
            .read_events()
            .expect("events before rejected recovery");

        let error = match runtime.recover(MemoryRecoverRequest {
            trigger: RuntimeLifecycleTrigger::BootRecovery,
            mode_input: RuntimeLifecycleModeInput::default(),
        }) {
            Ok(_) => panic!("a non-AgentPersona mount must not inspect or recover Soul state"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "soul_kernel_subject_binding");
        assert_eq!(
            event_reader
                .replay_harness()
                .read_events()
                .expect("events after rejected recovery"),
            events_before,
            "Soul ownership rejection must happen before lifecycle/store effects for {mounted_subject_id}"
        );
    }
}

#[test]
fn runtime_builder_rejects_missing_or_unbound_soul_owners_before_opening_runtime() {
    let profile = support::host_test_profile();
    let owner_id = "owner-invalid-soul-mount";
    let agent_id = "agent-main";
    let agent_subject_id = default_agent_subject_id(agent_id);

    let missing_store = empty_store_platform(profile);
    let missing_events = missing_store.clone();
    let missing_events_before = missing_events
        .replay_harness()
        .read_events()
        .expect("events before missing subject rejection");
    let missing_registry =
        SubjectRegistry::single_agent_default(owner_id, agent_id).expect("registry");
    let missing_graph = SubjectRelationshipGraph::single_agent_default(&missing_registry)
        .expect("relationship graph");
    let missing_error = match MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent_id, owner_id).expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(missing_store)
        .subject_registry(missing_registry)
        .subject_relationship_graph(missing_graph)
        .scoped_runtime(SubjectScopedRuntime {
            memory_space_id: default_memory_space_id(owner_id),
            mounted_subject_id: "subject:missing".to_string(),
            actor_subject_id: agent_subject_id.clone(),
            agent_id: agent_id.to_string(),
            relationship_scope: None,
            projection_policy: "subject_aware_default".to_string(),
            write_policy: "subject_candidate_then_space_governance".to_string(),
        })
        .build()
    {
        Ok(_) => panic!("missing mounted subject must fail runtime construction"),
        Err(error) => error,
    };
    assert_eq!(missing_error.stage(), "memory_runtime_config");
    assert_eq!(
        missing_events
            .replay_harness()
            .read_events()
            .expect("events after missing subject rejection"),
        missing_events_before
    );

    let unbound_store = empty_store_platform(profile);
    let unbound_events = unbound_store.clone();
    let unbound_events_before = unbound_events
        .replay_harness()
        .read_events()
        .expect("events before unbound subject rejection");
    let mut unbound_registry =
        SubjectRegistry::single_agent_default(owner_id, agent_id).expect("registry");
    unbound_registry
        .subjects
        .iter_mut()
        .find(|subject| subject.subject_id == agent_subject_id)
        .expect("agent subject")
        .soul_binding = None;
    let unbound_error = match MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent_id, owner_id).expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(unbound_store)
        .subject_registry(unbound_registry)
        .build()
    {
        Ok(_) => panic!("unbound AgentPersona must fail runtime construction"),
        Err(error) => error,
    };
    assert_eq!(unbound_error.stage(), "memory_runtime_config");
    assert_eq!(
        unbound_events
            .replay_harness()
            .read_events()
            .expect("events after unbound subject rejection"),
        unbound_events_before
    );
}

#[test]
fn runtime_recover_commits_bundle_owner_facet_and_lifecycle_atomically() {
    let source_platform = seeded_store_platform(support::host_test_profile());
    let snapshot = recovery_snapshot(
        &source_platform,
        "space:owner-default",
        &default_agent_subject_id("agent-main"),
        "chat-target",
    );

    let target_platform = empty_store_platform(support::host_test_profile());
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
        support::host_test_profile(),
        "agent-main",
        "owner-default",
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
            .memory_space_long_term_memory_read_store("space:owner-default")
            .expect("target owner store")
            .count()
            .expect("target owner count"),
        1
    );
    let manifest_key =
        memory_facet_manifest_key(runtime.memory_space_id(), runtime.memory_space_id())
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
fn runtime_recover_never_restores_soul_owned_core_from_generic_continuity_bundle() {
    let source_platform = seeded_store_platform(support::host_test_profile());
    let subject_id = default_agent_subject_id("agent-main");
    let mut snapshot = recovery_snapshot(
        &source_platform,
        "space:owner-default",
        &subject_id,
        "chat-target",
    );
    snapshot.self_authored_core = Some(SelfAuthoredCore {
        identity_anchor: "GENERIC_RECOVERY_MUST_NOT_RESTORE_SOUL".to_string(),
        ..SelfAuthoredCore::default()
    });

    let target_platform = empty_store_platform(support::host_test_profile());
    target_platform
        .replay_harness()
        .session_store()
        .append("chat-target", "user", "recover non-Soul continuity only")
        .expect("seed degraded session");
    target_platform
        .replay_harness()
        .state_fs()
        .write(
            REL_PATH_REBOOT_CONTINUITY_BUNDLE,
            &serde_json::to_vec(&ContinuitySnapshotBundle {
                version: 1,
                reason: "spv1_generic_recovery_boundary".to_string(),
                flushed_at: 1_800_000_000,
                primary_chat_id: Some("chat-target".to_string()),
                snapshots: vec![snapshot],
            })
            .expect("serialize recovery bundle"),
        )
        .expect("write recovery bundle");
    let runtime = test_runtime_with_identity_scope(
        target_platform.clone(),
        support::host_test_profile(),
        "agent-main",
        "owner-default",
        "llm.gateway",
        "chat-target",
    );
    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: "recovery-existing-soul".to_string(),
            human_actor_subject_id: primary_human_subject_id("owner-default"),
            charter: Box::new(
                SubjectSoulFoundingCharterSeedV1 {
                    identity_anchor: Some("EXISTING_SOUL_MUST_SURVIVE_RECOVERY".to_string()),
                    character_tendencies: vec!["persistent across recovery".to_string()],
                    priority_constitution: vec!["preserve governed ownership".to_string()],
                    non_negotiables: vec!["never import Soul through generic recovery".to_string()],
                    default_response_mode: None,
                    default_initiative_posture: None,
                    default_relationship_posture: None,
                    boundary_doctrine: None,
                    truth_seeking_commitment: None,
                    self_preservation_doctrine: None,
                    repair_doctrine: None,
                    change_principle: None,
                }
                .canonicalize()
                .expect("canonical recovery guard seed"),
            ),
            source_asserted_at: Some(1_700_000_000),
        })
        .expect("seed existing Soul through lifecycle owner");
    let soul_before = runtime
        .export_subject_soul_operator_safe(SubjectSoulReadSelectorV1::Current)
        .expect("read Soul root before generic recovery");

    let report = runtime
        .recover(MemoryRecoverRequest {
            trigger: RuntimeLifecycleTrigger::BootRecovery,
            mode_input: RuntimeLifecycleModeInput {
                recovery_safe_mode_active: true,
                ..RuntimeLifecycleModeInput::default()
            },
        })
        .expect("recover non-Soul continuity");

    assert!(
        !report
            .report
            .restored_layers
            .iter()
            .any(|layer| layer == "self_authored_core"),
        "generic recovery must not claim a Soul-owned layer"
    );
    assert_eq!(
        runtime
            .export_subject_soul_operator_safe(SubjectSoulReadSelectorV1::Current)
            .expect("read Soul root after generic recovery"),
        soul_before,
        "generic recovery must leave the existing protected Soul root exactly unchanged"
    );
}

#[test]
fn runtime_recover_budget_failure_leaves_owner_facet_and_events_unchanged() {
    let source_platform = seeded_store_platform(support::host_test_profile());
    let snapshot = recovery_snapshot(
        &source_platform,
        "space:owner-default",
        &default_agent_subject_id("agent-main"),
        "chat-target",
    );

    let event_log_max_items = 6;
    let budget = StoreRuntimeBudget {
        metric_source_max_items: 1,
        event_log_max_items,
        kv_max_entries: 129,
        blob_max_bytes: 4096,
        snapshot_max_bytes: 131_072,
        logical_namespace_max_bytes: 128,
        logical_key_max_bytes: 1024,
        event_record_key_max_bytes: 1024,
        export_max_bytes: 131_072,
        import_max_bytes: 131_072,
    };
    let config = StoreBackendConfig::in_memory(support::host_test_profile())
        .expect("store config")
        .try_with_nonproduction_store_budget_limit(budget)
        .expect("valid store budget limit");
    let target_platform = support::open_memory_store(config).expect("target store");
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
        support::host_test_profile(),
        "agent-main",
        "owner-default",
        "llm.gateway",
        "chat-target",
    );
    let events_before = target_platform
        .replay_harness()
        .read_events()
        .expect("events before");
    assert_eq!(
        events_before.len(),
        event_log_max_items,
        "fixture must saturate the event budget before recovery"
    );

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
            .memory_space_long_term_memory_read_store("space:owner-default")
            .expect("target owner store")
            .count()
            .expect("target owner count"),
        0
    );
    let manifest_key =
        memory_facet_manifest_key(runtime.memory_space_id(), runtime.memory_space_id())
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
