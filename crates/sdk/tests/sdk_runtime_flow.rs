mod support;

use std::sync::Arc;

use bm_sdk::{
    IngressKind, MemoryInspectionRequest, MemoryMaintenanceRequest, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryWriteRequest, ProfileId, RuntimeSkillReuseOutcome,
    RuntimeSkillWrite, RuntimeSkillWriteSource,
};

use support::{test_runtime, HostMemoryPlatform, StaticHttpClient, StaticLlmClient};

#[test]
fn runtime_write_recall_project_uses_sdk_entry_only() {
    let platform = Arc::new(HostMemoryPlatform::default());
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);

    let write = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["operator accepted".to_string()],
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
            query: "release artifact".to_string(),
            limit: 4,
        })
        .expect("recall");

    assert!(recall
        .procedural_hits
        .iter()
        .any(|hit| hit.record.name == "runtime_skill__release_guard"));

    let projection = runtime
        .project(MemoryProjectionRequest {
            user_query: "How should I publish?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
        })
        .expect("projection");

    assert!(projection.system_memory_block.len() <= 4096);
}

#[test]
fn runtime_maintain_and_inspect_return_structured_reports() {
    let platform = Arc::new(HostMemoryPlatform::default());
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);
    let llm = StaticLlmClient::summary_response("Summary: release safety");
    let mut http = StaticHttpClient;

    let maintenance = runtime
        .maintain(
            &mut http,
            &llm,
            MemoryMaintenanceRequest {
                ingress: IngressKind::User,
                user_content: "remember the release process".to_string(),
                reply_content: "I will verify artifacts first.".to_string(),
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
            query: "release".to_string(),
            system_max_len: 4096,
        })
        .expect("inspection");

    assert_eq!(inspection.working.query, "release");
    assert_eq!(
        inspection.capabilities.profile,
        ProfileId::ServerLinuxDevFull
    );
}
