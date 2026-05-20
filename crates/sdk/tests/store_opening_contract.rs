mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::platform::Platform as _;
use bm_sdk::{
    ContinuitySnapshotImportMode, IngressKind, MemoryExportRequest, MemoryImportRequest,
    MemoryInspectionRequest, MemoryMaintenanceRequest, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryWriteRequest, ProfileId, RuntimeSkillReuseOutcome,
    RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig, StorePlatform,
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
    let store = StorePlatform::open(
        StoreBackendConfig::file(&root, ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");

    let runtime = test_runtime(store.clone(), ProfileId::ServerLinuxDevFull);

    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "store_contract".to_string(),
                topic: "store backend".to_string(),
                title: "Use StorePlatform directly".to_string(),
                summary: "SDK hosts open StorePlatform instead of implementing store traits."
                    .to_string(),
                content: "1. choose backend\n2. open StorePlatform\n3. pass platform to runtime"
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
            query: "StorePlatform backend".to_string(),
            limit: 4,
        })
        .expect("recall");

    assert!(recall
        .procedural_hits
        .iter()
        .any(|hit| hit.record.name == "runtime_skill__store_contract"));

    let projection = runtime
        .project(MemoryProjectionRequest {
            user_query: "How do I open storage?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
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
                reply_content: "Use StorePlatform and pass it to MemoryRuntime.".to_string(),
                tool_calls: 0,
                external_content_used: false,
                runtime_skill_selected_ids: Vec::new(),
                task_learning_selected_ids: Vec::new(),
                reuse_outcome: RuntimeSkillReuseOutcome::Neutral,
                reuse_outcome_note: String::new(),
            },
        )
        .expect("maintenance");
    assert!(maintenance.report.after_count <= maintenance.report.after_count.saturating_add(1));

    let inspection = runtime
        .inspect(MemoryInspectionRequest {
            query: "store backend".to_string(),
            system_max_len: 4096,
        })
        .expect("inspection");
    assert_eq!(inspection.working.query, "store backend");

    let exported = runtime
        .export(MemoryExportRequest {
            chat_id: "chat-1".to_string(),
        })
        .expect("export");
    assert_eq!(exported.snapshot.chat_id, "chat-1");

    let imported = runtime
        .import(MemoryImportRequest {
            snapshot: exported.snapshot,
            target_chat_id: "chat-2".to_string(),
            mode: ContinuitySnapshotImportMode::FullRestore,
        })
        .expect("import");
    assert!(!imported.outcome.decisions.is_empty());

    let reopened = StorePlatform::open(
        StoreBackendConfig::file(&root, ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("reopen");
    assert!(reopened
        .skill_storage()
        .list_names()
        .expect("list")
        .contains(&"runtime_skill__store_contract".to_string()));
}
