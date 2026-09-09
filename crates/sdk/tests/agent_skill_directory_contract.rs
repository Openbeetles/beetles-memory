mod support;
use std::fs;
use std::path::PathBuf;

use bm_sdk::{
    AgentSkillDirConfig, MemoryIdentity, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryRuntime, MemoryScope, PressureLevel, RuntimeLifecycleModeInput, StoreBackendConfig,
};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn runtime_with_agent_skill_dir(root: PathBuf) -> MemoryRuntime {
    let profile = support::host_test_profile();
    let store =
        support::open_memory_store(StoreBackendConfig::in_memory(profile).expect("store config"))
            .expect("store");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-skill-test", "owner-default").expect("identity"))
        .scope(MemoryScope::new("console", "chat-1").expect("scope"))
        .store(store)
        .add_agent_skill_dir(AgentSkillDirConfig::read_only(root, "host"))
        .build()
        .expect("runtime")
}

fn assert_colliding_skill_projection_keeps_exact_package_binding(deny_first: bool) {
    let root = unique_temp_dir("bm-agent-skill-collision");
    let first = root.join("first");
    let second = root.join("second");
    for (dir, marker) in [
        (&first, "FIRST_PACKAGE_BODY"),
        (&second, "SECOND_PACKAGE_BODY"),
    ] {
        fs::create_dir_all(dir).expect("synthetic mount");
        fs::write(dir.join("SKILL.md"), format!(
            "---\nname: release-check\ndescription: Verify release artifacts.\n---\n# Release\n{marker}\n",
        )).expect("synthetic skill");
    }
    let mut first_mount = AgentSkillDirConfig::read_only(&first, "host");
    first_mount.scope = bm_sdk::AgentSkillScope::Conversation {
        conversation_id: if deny_first { "other-chat" } else { "chat-a" }.into(),
    };
    let mut second_mount = AgentSkillDirConfig::read_only(&second, "host");
    second_mount.scope = bm_sdk::AgentSkillScope::Conversation {
        conversation_id: "chat-a".into(),
    };
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-skill-test", "owner-default").expect("identity"))
        .scope(MemoryScope::new("console", "chat-a").expect("scope"))
        .store(support::empty_store_platform(support::host_test_profile()))
        .add_agent_skill_dir(first_mount)
        .add_agent_skill_dir(second_mount)
        .build()
        .expect("runtime");
    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![],
            query: "release artifacts".into(),
            limit: 4,
            tool_registry_refs: vec![],
        })
        .expect("scoped recall");
    assert!(
        !recall.agent_skill_hits.is_empty(),
        "nonempty admitted positive control"
    );
    if !deny_first {
        assert!(
            recall.agent_skill_hits.windows(2).any(|hits| {
                hits[0].package_id == hits[1].package_id
                    && hits[0].fingerprint != hits[1].fingerprint
            }),
            "same logical ID must exercise different actual content"
        );
    }
    let projected = runtime
        .project(MemoryProjectionRequest {
            binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![],
            user_query: "release artifacts".into(),
            system_max_len: 6000,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: vec![],
        })
        .expect("projection");
    let body = projected.provider_payload().system_memory_block();
    assert!(
        body.contains("SECOND_PACKAGE_BODY"),
        "selected second mount body must be projected"
    );
    if deny_first {
        assert!(
            !body.contains("FIRST_PACKAGE_BODY"),
            "denied first mount body must never substitute for an allowed colliding hit"
        );
    } else {
        assert!(
            body.contains("FIRST_PACKAGE_BODY"),
            "both selected same-scope packages remain positive"
        );
    }
    fs::remove_dir_all(root).expect("remove synthetic mounts");
}

#[test]
fn agent_skill_projection_colliding_id_cannot_substitute_denied_mount_body() {
    assert_colliding_skill_projection_keeps_exact_package_binding(true);
}

#[test]
fn agent_skill_projection_colliding_id_keeps_each_same_scope_content() {
    assert_colliding_skill_projection_keeps_exact_package_binding(false);
}

#[test]
fn scoped_agent_skill_is_not_a_cross_conversation_or_historical_candidate() {
    struct FixedClock(u64);
    impl bm_sdk::MemoryClock for FixedClock {
        fn now_secs(&self) -> u64 {
            self.0
        }
    }
    let root = unique_temp_dir("bm-scoped-agent-skill");
    fs::write(root.join("SKILL.md"), "---\nname: release-check\ndescription: Verify release artifacts and checksums.\n---\n# Release\nVerify release artifacts.\n").expect("synthetic skill");
    let mut dir = AgentSkillDirConfig::read_only(&root, "host");
    dir.scope = bm_sdk::AgentSkillScope::Conversation {
        conversation_id: "chat-a".into(),
    };
    let build = |chat: &str| {
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("agent-skill-test", "owner-default").expect("identity"))
            .scope(MemoryScope::new("console", chat).expect("scope"))
            .store(support::empty_store_platform(support::host_test_profile()))
            .clock(std::sync::Arc::new(FixedClock(1_800_000_100)))
            .add_agent_skill_dir(dir.clone())
            .build()
            .expect("runtime")
    };
    let request = |temporal_operation| MemoryRecallRequest {
        temporal_operation,
        structured_query_facets: vec![],
        query: "release artifacts".into(),
        limit: 4,
        tool_registry_refs: vec![],
    };
    let own = build("chat-a");
    assert!(!own
        .recall(request(bm_sdk::MemoryRecallTemporalOperation::Current))
        .expect("own positive")
        .agent_skill_hits
        .is_empty());
    let other = build("chat-b");
    assert!(
        other
            .recall(request(bm_sdk::MemoryRecallTemporalOperation::Current))
            .expect("different context")
            .agent_skill_hits
            .is_empty(),
        "conversation skill must not cross its applicability"
    );
    assert!(own
        .recall(request(
            bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_800_000_000
            }
        ))
        .expect("historical")
        .agent_skill_hits
        .is_empty());
    fs::remove_dir_all(root).expect("remove synthetic skill");
}

#[test]
fn sdk_recalls_and_projects_host_agent_skills_without_managing_them() {
    let root = unique_temp_dir("bm-agent-skill-directory");
    let skill_dir = root.join("release-check");
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: release-check
description: Validate release artifacts, checksums, and changelog before publishing.
---
# Release Check

Use this when a release needs artifact verification and changelog inspection.
"#,
    )
    .expect("skill file");

    let runtime = runtime_with_agent_skill_dir(root);
    let inspection = runtime
        .inspect(bm_sdk::MemoryInspectionRequest {
            query: "release artifacts".to_string(),
            system_max_len: 4096,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("inspect");
    assert_eq!(inspection.agent_skill_directory.active_packages, 1);

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release artifact checksums".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    assert_eq!(recall.agent_skill_hits.len(), 1);

    let projection = runtime
        .project(MemoryProjectionRequest {
            binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "prepare release artifact checks".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");
    assert_eq!(projection.report().audit().agent_skill_selected_count, 1);
    assert!(projection
        .provider_payload()
        .system_memory_block()
        .contains("Agent Skill Hints"));
}
