#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::platform::Platform as _;
use bm_sdk::{
    IngressKind, MemoryInspectionRequest, MemoryMaintenanceRequest, MemoryProjectionRequest,
    MemoryRecallRequest, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy, MemorySpaceScope, MemoryWriteRequest, PressureLevel,
    RuntimeLifecycleModeInput, RuntimeSkillReuseOutcome, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig,
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
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "store_contract".to_string(),
                topic: "store backend".to_string(),
                title: "Use MemoryStoreHandle directly".to_string(),
                summary: "SDK hosts open MemoryStoreHandle instead of implementing store traits."
                    .to_string(),
                content:
                    "1. choose backend\n2. open MemoryStoreHandle\n3. pass platform to runtime"
                        .to_string(),
                citations: vec!["sdk store opening contract".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write");

    assert!(write.accepted);
    assert_eq!(write.changed, 1);

    let recall = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "MemoryStoreHandle backend".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");

    assert!(recall
        .procedural_hits
        .iter()
        .any(|hit| hit.record.name == "runtime_skill__store_contract"));

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "How do I open storage?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");
    assert!(projection.system_memory_block.len() <= 4096);

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

    let runtime_scope = MemorySpaceScope {
        memory_space_id: runtime.memory_space_id().to_string(),
        mounted_subject_id: runtime.subject_id().to_string(),
    };
    let exported = runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: runtime_scope.clone(),
            include_private: true,
        })
        .expect("export");
    assert_eq!(exported.projection_scope.scope, runtime_scope);

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
    assert!(reopened
        .replay_harness()
        .skill_storage()
        .list_names()
        .expect("list")
        .contains(&"runtime_skill__store_contract".to_string()));
}
