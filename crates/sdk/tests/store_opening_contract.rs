#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use bm_sdk::{
    IngressKind, LongTermMemoryDraft, LongTermMemoryKind, MemoryArchiveScope,
    MemoryInspectionRequest, MemoryMaintenanceRequest, MemoryPrivacyClass, MemoryProjectionRequest,
    MemoryRecallRequest, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy, MemoryWriteRequest, ParsedLongTermMemoryExtraction,
    PressureLevel, RuntimeLifecycleModeInput, RuntimeSkillReuseOutcome, StoreBackendConfig,
};

use support::{test_runtime, StaticHttpClient, StaticLlmClient};

#[test]
fn sdk_runtime_accepts_store_platform_without_host_store_traits() {
    let root = std::env::temp_dir().join(format!(
        "bm-sdk-store-opening-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let store = support::open_memory_store(
        StoreBackendConfig::file(&root, support::host_test_profile()).expect("config"),
    )
    .expect("store");

    let runtime = test_runtime(store.clone(), support::host_test_profile());

    let write = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    topic: "store backend".to_string(),
                    content:
                        "SDK hosts open MemoryStoreHandle instead of implementing store traits."
                            .to_string(),
                    keywords: vec!["MemoryStoreHandle".to_string(), "backend".to_string()],
                    source_chat_id: Some("chat-1".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["sdk store opening contract".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    last_confirmed_at: Some(1_800_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("write");

    assert!(write.accepted);
    assert_eq!(write.changed, 1);

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "MemoryStoreHandle backend".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");

    assert_eq!(recall.delivery_report.selected_candidate_ids.len(), 1);

    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "How do I open storage?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");
    assert!(projection.provider_payload().system_memory_block().len() <= 4096);

    let llm = StaticLlmClient::summary_response("Summary: store backend opening");
    let mut http = StaticHttpClient;
    let maintenance = runtime
        .maintain(
            &mut http,
            &llm,
            MemoryMaintenanceRequest {
                ingress: IngressKind::User,
                user_content: "remember the store backend opening path".to_string(),
                reply_content: "Use MemoryStoreHandle and pass it to MemoryRuntime.".to_string(),
                tool_calls: 0,
                external_content_used: false,
                runtime_skill_selected_ids: Vec::new(),
                task_learning_selected_ids: Vec::new(),
                reuse_outcome: RuntimeSkillReuseOutcome::Neutral,
                reuse_outcome_note: String::new(),
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .expect("maintenance");
    let maintenance_report = maintenance.report.expect("maintenance report");
    assert!(maintenance_report.after_count <= maintenance_report.after_count.saturating_add(1));

    let inspection = runtime
        .inspect(MemoryInspectionRequest {
            query: "store backend".to_string(),
            system_max_len: 4096,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("inspection");
    assert_eq!(inspection.working.query, "store backend");

    let runtime_scope =
        MemoryArchiveScope::subject(runtime.memory_space_id(), runtime.subject_id())
            .expect("runtime archive scope");
    let exported = runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: runtime_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .expect("export");
    assert_eq!(exported.projection_scope.scope, runtime_scope);
    assert_eq!(
        exported.projection_scope.private_material_policy,
        MemorySpacePrivateMaterialPolicy::IncludePrivate
    );

    let imported = runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope: runtime_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        })
        .expect("import");
    assert_eq!(imported.imported_scope, runtime_scope);

    let reopened = support::open_memory_store(
        StoreBackendConfig::file(&root, support::host_test_profile()).expect("config"),
    )
    .expect("reopen");
    let reopened_runtime = support::test_runtime(reopened, support::host_test_profile());
    let reopened_recall = reopened_runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "MemoryStoreHandle backend".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("reopened typed recall");
    assert_eq!(
        reopened_recall.delivery_report.selected_candidate_ids.len(),
        1
    );
}
