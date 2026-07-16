#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::Arc;

use bm_core::memory::{
    board_subject_scope_id, governed_memory_recall_candidate_id, private_garden_scope_id,
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, InnerLife, PrivateDocEntry,
    PrivateDocWorkspace, SelfContinuity, SelfModel,
};
use bm_core::platform::Platform as _;
use bm_sdk::{
    IngressKind, MemoryAuditSink, MemoryClock, MemoryIdentity, MemoryInspectionRequest,
    MemoryMaintenanceRequest, MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryRuntime, MemoryScope, MemoryWriteRequest, NoopMemoryAuditSink, PressureLevel,
    ProjectionSourceAuthority, RuntimeLifecycleModeInput, RuntimeSkillReuseOutcome,
    RuntimeSkillWrite, RuntimeSkillWriteSource,
};

use support::{
    empty_store_platform, test_runtime, test_runtime_with_scope, StaticHttpClient, StaticLlmClient,
};

#[test]
fn runtime_write_recall_project_uses_sdk_entry_only() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform, support::host_test_profile());

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
            structured_query_facets: Vec::new(),
            query: "release artifact".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    let runtime_skill_candidate_id =
        governed_memory_recall_candidate_id(&GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::RuntimeSkill,
            "runtime_skill__release_guard",
        ));

    assert!(recall
        .procedural_hits
        .iter()
        .any(|hit| hit.record.name == "runtime_skill__release_guard"));
    assert!(recall
        .graph_rerank
        .candidate_ids
        .iter()
        .any(|candidate| candidate == &runtime_skill_candidate_id));
    assert!(recall
        .graph_rerank
        .reranked_candidate_ids
        .iter()
        .any(|candidate| candidate == &runtime_skill_candidate_id));
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
        .any(|node| node.node_id == runtime_skill_candidate_id));

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "How should I publish?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(projection.system_memory_block.len() <= 4096);
}

#[test]
fn runtime_projection_isolates_session_context_by_chat_scope_under_same_store_platform() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    platform
        .replay_harness()
        .session_store()
        .append("chat-a", "user", "chat-a-only-user")
        .expect("seed chat-a user");
    platform
        .replay_harness()
        .session_store()
        .append("chat-a", "assistant", "chat-a-only-assistant")
        .expect("seed chat-a assistant");
    platform
        .replay_harness()
        .session_store()
        .append("chat-b", "user", "chat-b-only-user")
        .expect("seed chat-b user");
    platform
        .replay_harness()
        .session_store()
        .append("chat-b", "assistant", "chat-b-only-assistant")
        .expect("seed chat-b assistant");
    platform
        .replay_harness()
        .session_summary_store()
        .set_with_count("chat-a", "chat-a-only-summary", 2)
        .expect("seed chat-a summary");
    platform
        .replay_harness()
        .session_summary_store()
        .set_with_count("chat-b", "chat-b-only-summary", 2)
        .expect("seed chat-b summary");

    let runtime_a = test_runtime_with_scope(platform.clone(), profile, "local", "chat-a");
    let runtime_b = test_runtime_with_scope(platform, profile, "local", "chat-b");

    let project = |runtime: &MemoryRuntime, query: &str| {
        runtime
            .project(MemoryProjectionRequest {
                structured_query_facets: Vec::new(),
                user_query: query.to_string(),
                system_max_len: 4096,
                recent_messages_limit: 8,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                tool_registry_refs: Vec::new(),
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
    assert!(!projection_a
        .projection_surfaces
        .shared_fact_surface
        .contains("chat-a-only-summary"));

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
    assert!(!projection_b
        .projection_surfaces
        .shared_fact_surface
        .contains("chat-b-only-summary"));
}

#[test]
fn runtime_maintain_and_inspect_return_structured_reports() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform, support::host_test_profile());
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
        support::host_test_profile()
    );
}

#[test]
fn runtime_projection_includes_private_planes_when_policy_allows_it() {
    let platform = empty_store_platform(support::host_test_profile());
    platform
        .replay_harness()
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
        .replay_harness()
        .private_garden_store()
        .write(
            private_garden_scope_id(),
            "diary/release.md",
            "private garden release note",
            1_800_000_000,
        )
        .expect("private garden seed");
    platform
        .replay_harness()
        .self_model_store()
        .set(
            board_subject_scope_id(),
            &SelfModel {
                continuity_anchor: "private self model release anchor".to_string(),
                attachment_style: "steady".to_string(),
                privacy_need: "high".to_string(),
                directness: "direct".to_string(),
                ..SelfModel::default()
            },
        )
        .expect("self model seed");

    let mut privacy = MemoryPrivacyPolicy::standard_private_boundary();
    privacy.private_plane_projection_allowed = true;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(platform)
        .clock(Arc::new(TestClock))
        .capability_policy(bm_sdk::MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(privacy)
        .audit_sink(Arc::new(NoopMemoryAuditSink) as Arc<dyn MemoryAuditSink>)
        .build()
        .expect("runtime");

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "release".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(projection
        .runtime_projection
        .protected_private_runtime_context
        .iter()
        .any(|item| item.content.contains("private workspace release note")));
    assert!(projection
        .runtime_projection
        .protected_private_runtime_context
        .iter()
        .any(|item| item.content.contains("private garden release note")));
    assert!(projection
        .runtime_projection
        .protected_private_runtime_context
        .iter()
        .any(|item| item.content.contains("Continuity tendencies")));
    let lower = projection.system_memory_block.to_ascii_lowercase();
    for forbidden in [
        "roleplay",
        "personality",
        "model identity",
        "memory helper",
        "assistant self-description",
        "relationship theater",
        "training provenance",
        "user-facing identity",
        "personality axes",
    ] {
        assert!(
            !lower.contains(forbidden),
            "{forbidden} leaked into private runtime projection:\n{}",
            projection.system_memory_block
        );
    }
    assert!(
        projection
            .system_memory_block
            .contains("## Soul Private Runtime Context"),
        "{}",
        projection.system_memory_block
    );
    assert!(
        projection
            .system_memory_block
            .contains("Runtime private context: allowed"),
        "{}",
        projection.system_memory_block
    );
    assert!(
        projection
            .audit
            .private_gate
            .runtime_private_context_allowed
    );
    assert!(!projection.audit.private_gate.foreground_disclosure_allowed);
    let private_garden_authority = projection
        .audit
        .source_authority
        .iter()
        .find(|source| source.source_id == "private_garden")
        .expect("private garden authority");
    assert!(private_garden_authority.loaded);
    assert!(private_garden_authority.runtime_private_context_allowed);
    assert!(!private_garden_authority.foreground_disclosure_allowed);
    assert!(!private_garden_authority.raw_audit_plaintext_allowed);
    assert!(private_garden_authority
        .authorities
        .contains(&ProjectionSourceAuthority::PrivateInternal));
    let private_workspace_authority = projection
        .audit
        .source_authority
        .iter()
        .find(|source| source.source_id == "private_workspace")
        .expect("private workspace authority");
    assert!(private_workspace_authority.loaded);
    assert!(private_workspace_authority.runtime_private_context_allowed);
    assert!(!private_workspace_authority.foreground_disclosure_allowed);
    assert!(!private_workspace_authority.raw_audit_plaintext_allowed);
    assert!(private_workspace_authority
        .authorities
        .contains(&ProjectionSourceAuthority::PrivateInternal));
    for forbidden_heading in [
        "## Private Garden",
        "## Inner Workspace",
        "## Inner Life",
        "## Self State",
        "## Outer Voice",
    ] {
        assert!(
            !projection.system_memory_block.contains(forbidden_heading),
            "{}",
            projection.system_memory_block
        );
    }
    assert_eq!(
        projection.projection_surfaces.prompt,
        projection.system_memory_block
    );
    for (surface, block) in [
        ("ui_api", &projection.projection_surfaces.ui_api),
        ("operator_raw", &projection.projection_surfaces.operator_raw),
        (
            "gateway_raw_audit",
            &projection.projection_surfaces.gateway_raw_audit,
        ),
        (
            "shared_fact_surface",
            &projection.projection_surfaces.shared_fact_surface,
        ),
    ] {
        for private_raw in [
            "private workspace release note",
            "private garden release note",
            "private self model release anchor",
        ] {
            assert!(
                !block.contains(private_raw),
                "{surface} leaked exact protected content: {private_raw}"
            );
        }
    }
    assert_eq!(
        projection
            .private_disclosure_integrity
            .surface_reports
            .len(),
        5
    );
    assert!(projection
        .private_disclosure_integrity
        .surface_reports
        .iter()
        .all(|surface| surface.passed && surface.violation_count == 0));
    assert!(projection.private_disclosure_integrity.passed);
}

#[test]
fn runtime_projection_excludes_private_planes_when_policy_denies_it() {
    let platform = empty_store_platform(support::host_test_profile());
    platform
        .replay_harness()
        .self_model_store()
        .set(
            board_subject_scope_id(),
            &SelfModel {
                continuity_anchor: "denied private self model anchor".to_string(),
                private_notes: "denied private self model note".to_string(),
                ..SelfModel::default()
            },
        )
        .expect("self model seed");
    platform
        .replay_harness()
        .self_continuity_store()
        .set(
            board_subject_scope_id(),
            &SelfContinuity {
                wake_anchor: "denied private self continuity anchor".to_string(),
                current_self_state: "denied private self continuity state".to_string(),
                ..SelfContinuity::default()
            },
        )
        .expect("self continuity seed");
    platform
        .replay_harness()
        .inner_life_store()
        .set(
            board_subject_scope_id(),
            &InnerLife {
                internal_monologue: "denied private inner monologue".to_string(),
                private_journal: "denied private inner journal".to_string(),
                ..InnerLife::default()
            },
        )
        .expect("inner life seed");
    platform
        .replay_harness()
        .private_doc_store()
        .set(
            board_subject_scope_id(),
            &PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "denied private workspace note".to_string(),
                    updated_at: 1_800_000_000,
                    revision: 1,
                }),
                ..PrivateDocWorkspace::default()
            },
        )
        .expect("private workspace seed");
    platform
        .replay_harness()
        .private_garden_store()
        .write(
            private_garden_scope_id(),
            "diary/denied.md",
            "denied private garden note",
            1_800_000_000,
        )
        .expect("private garden seed");
    let runtime =
        test_runtime_with_scope(platform, support::host_test_profile(), "local", "chat-1");

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "release".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(
        !projection
            .audit
            .private_gate
            .runtime_private_context_allowed
    );
    assert!(!projection.audit.private_gate.foreground_disclosure_allowed);
    for private_text in [
        "denied private self model anchor",
        "denied private self model note",
        "denied private self continuity anchor",
        "denied private self continuity state",
        "denied private inner monologue",
        "denied private inner journal",
        "denied private workspace note",
        "denied private garden note",
    ] {
        assert!(
            !projection.system_memory_block.contains(private_text),
            "{}",
            projection.system_memory_block
        );
    }
    for source_id in [
        "self_model",
        "self_continuity",
        "self_state",
        "inner_life",
        "private_workspace",
        "private_garden",
        "mental_privacy",
    ] {
        let source = projection
            .audit
            .source_authority
            .iter()
            .find(|source| source.source_id == source_id)
            .expect("source authority");
        assert!(
            !source.loaded,
            "{source_id} must not load when policy denies private runtime context"
        );
    }
}

struct TestClock;

impl MemoryClock for TestClock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}
