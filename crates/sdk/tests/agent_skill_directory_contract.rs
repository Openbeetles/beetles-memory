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
