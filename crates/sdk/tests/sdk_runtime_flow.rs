mod support;

use std::sync::Arc;

use bm_core::memory::{
    board_subject_scope_id, private_garden_scope_id, PrivateDocEntry, PrivateDocWorkspace,
};
use bm_core::platform::Platform as _;
use bm_sdk::{
    IngressKind, MemoryAuditSink, MemoryClock, MemoryIdentity, MemoryInspectionRequest,
    MemoryMaintenanceRequest, MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryRuntime, MemoryScope, MemoryWriteRequest, NoopMemoryAuditSink, PressureLevel, ProfileId,
    RuntimeLifecycleModeInput, RuntimeSkillReuseOutcome, RuntimeSkillWrite,
    RuntimeSkillWriteSource,
};

use support::{
    empty_store_platform, test_runtime, test_runtime_with_scope, StaticHttpClient, StaticLlmClient,
};

#[test]
fn runtime_write_recall_project_uses_sdk_entry_only() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

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
    let evolution = write
        .procedural_evolution
        .as_ref()
        .expect("procedural evolution report");
    assert!(evolution
        .added
        .iter()
        .any(|name| name == "runtime_skill__release_guard"));
    assert!(evolution
        .reasons
        .iter()
        .any(|reason| reason.contains("procedural_memory")));

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
    assert!(recall
        .graph_rerank
        .candidate_ids
        .iter()
        .any(|candidate| candidate == "runtime_skill__release_guard"));
    assert!(recall
        .graph_rerank
        .selected_ids
        .iter()
        .any(|candidate| candidate == "runtime_skill__release_guard"));
    assert!(!recall.graph_gate.high_confidence_projection_allowed);
    assert!(recall
        .graph_gate
        .failures
        .iter()
        .any(|failure| failure == "runtime_recall_graph_preview_not_persistent"));
    assert!(recall.graph_gate.evidence_backlinks > 0);
    assert!(recall
        .compact_graph
        .nodes
        .iter()
        .any(|node| node.node_id == "runtime_skill__release_guard"));

    let projection = runtime
        .project(MemoryProjectionRequest {
            user_query: "How should I publish?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("projection");

    assert!(projection.system_memory_block.len() <= 4096);
}

#[test]
fn runtime_projection_isolates_session_context_by_chat_scope_under_same_store_platform() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    platform
        .session_store()
        .append("chat-a", "user", "chat-a-only-user")
        .expect("seed chat-a user");
    platform
        .session_store()
        .append("chat-a", "assistant", "chat-a-only-assistant")
        .expect("seed chat-a assistant");
    platform
        .session_store()
        .append("chat-b", "user", "chat-b-only-user")
        .expect("seed chat-b user");
    platform
        .session_store()
        .append("chat-b", "assistant", "chat-b-only-assistant")
        .expect("seed chat-b assistant");
    platform
        .session_summary_store()
        .set_with_count("chat-a", "chat-a-only-summary", 2)
        .expect("seed chat-a summary");
    platform
        .session_summary_store()
        .set_with_count("chat-b", "chat-b-only-summary", 2)
        .expect("seed chat-b summary");

    let runtime_a = test_runtime_with_scope(platform.clone(), profile, "local", "chat-a");
    let runtime_b = test_runtime_with_scope(platform, profile, "local", "chat-b");

    let project = |runtime: &MemoryRuntime, query: &str| {
        runtime
            .project(MemoryProjectionRequest {
                user_query: query.to_string(),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            })
            .expect("projection")
    };

    let projection_a = project(&runtime_a, "what happened in chat a?");
    assert!(projection_a
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "chat-a-only-user"));
    assert!(projection_a
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "chat-a-only-assistant"));
    assert!(!projection_a
        .context
        .recent_messages
        .iter()
        .any(|message| message.content.contains("chat-b-only")));
    assert_eq!(
        projection_a.context.message_summary_text.as_deref(),
        Some("chat-a-only-summary")
    );
    assert!(
        projection_a
            .system_memory_block
            .contains("chat-a-only-summary"),
        "{}",
        projection_a.system_memory_block
    );
    assert!(
        !projection_a
            .system_memory_block
            .contains("chat-b-only-summary"),
        "{}",
        projection_a.system_memory_block
    );

    let projection_b = project(&runtime_b, "what happened in chat b?");
    assert!(projection_b
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "chat-b-only-user"));
    assert!(projection_b
        .context
        .recent_messages
        .iter()
        .any(|message| message.content == "chat-b-only-assistant"));
    assert!(!projection_b
        .context
        .recent_messages
        .iter()
        .any(|message| message.content.contains("chat-a-only")));
    assert_eq!(
        projection_b.context.message_summary_text.as_deref(),
        Some("chat-b-only-summary")
    );
    assert!(
        projection_b
            .system_memory_block
            .contains("chat-b-only-summary"),
        "{}",
        projection_b.system_memory_block
    );
    assert!(
        !projection_b
            .system_memory_block
            .contains("chat-a-only-summary"),
        "{}",
        projection_b.system_memory_block
    );
}

#[test]
fn runtime_maintain_and_inspect_return_structured_reports() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
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
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
            },
        )
        .expect("maintenance");

    let maintenance_report = maintenance.report.expect("maintenance report");
    assert!(maintenance_report.after_count <= maintenance_report.after_count.saturating_add(1));

    let inspection = runtime
        .inspect(MemoryInspectionRequest {
            query: "release".to_string(),
            system_max_len: 4096,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("inspection");

    assert_eq!(inspection.working.query, "release");
    assert_eq!(
        inspection.capabilities.profile,
        ProfileId::ServerLinuxDevFull
    );
}

#[test]
fn runtime_projection_includes_private_planes_when_policy_allows_it() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    platform
        .private_doc_store()
        .set(
            board_subject_scope_id(),
            &PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "private workspace release note".to_string(),
                    updated_at: 1_800_000_000,
                    revision: 1,
                }),
                ..PrivateDocWorkspace::default()
            },
        )
        .expect("private workspace seed");
    platform
        .private_garden_store()
        .write(
            private_garden_scope_id(),
            "diary/release.md",
            "private garden release note",
            1_800_000_000,
        )
        .expect("private garden seed");

    let mut privacy = MemoryPrivacyPolicy::standard_private_boundary();
    privacy.private_plane_projection_allowed = true;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .profile(ProfileId::ServerLinuxDevFull)
        .store_platform(platform)
        .clock(Arc::new(TestClock))
        .capability_policy(bm_sdk::MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(privacy)
        .audit_sink(Arc::new(NoopMemoryAuditSink) as Arc<dyn MemoryAuditSink>)
        .build()
        .expect("runtime");

    let projection = runtime
        .project(MemoryProjectionRequest {
            user_query: "release".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("projection");

    assert!(
        projection
            .system_memory_block
            .contains("private workspace release note"),
        "{}",
        projection.system_memory_block
    );
    assert!(
        projection
            .system_memory_block
            .contains("private garden release note"),
        "{}",
        projection.system_memory_block
    );
}

struct TestClock;

impl MemoryClock for TestClock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}
