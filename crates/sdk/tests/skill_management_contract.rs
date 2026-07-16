mod support;
use bm_sdk::{
    MemoryIdentity, MemoryRuntime, MemoryScope, MemoryWriteRequest, RuntimeSkillDeleteRequest,
    RuntimeSkillDetailRequest, RuntimeSkillEditRequest, RuntimeSkillListRequest,
    RuntimeSkillSetEnabledRequest, RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
};

fn test_runtime() -> MemoryRuntime {
    let profile = support::host_test_profile();
    let store =
        support::open_memory_store(StoreBackendConfig::in_memory(profile).expect("store config"))
            .expect("store");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("skill-test-agent", "owner-default").expect("identity"))
        .scope(MemoryScope::new("console", "chat-1").expect("scope"))
        .store(store)
        .build()
        .expect("runtime")
}

fn seed_release_skill(runtime: &MemoryRuntime) {
    let report = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "runtime_skill__release_guard".to_string(),
                title: "Release guard".to_string(),
                topic: "release".to_string(),
                summary: "Check release artifacts before publishing.".to_string(),
                content: "1. run gates\n2. inspect artifacts\n3. dry run publish".to_string(),
                citations: vec!["test".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed procedural skill");
    assert!(report.changed > 0);
}

fn release_skill_edit_request() -> RuntimeSkillEditRequest {
    RuntimeSkillEditRequest {
        name: "runtime_skill__release_guard".to_string(),
        title: "Release guard".to_string(),
        topic: "release".to_string(),
        summary: "Check release artifacts and changelog before publishing.".to_string(),
        procedure: "1. run gates\n2. inspect artifacts\n3. inspect changelog".to_string(),
        citations: vec!["test-edit".to_string()],
        source_chat_id: Some("chat-1".to_string()),
        edit_reason: "sdk_contract_edit".to_string(),
        observed_at: 1_800_000_001,
    }
}

#[test]
fn runtime_lists_runtime_skills_with_summary_counts() {
    let runtime = test_runtime();
    seed_release_skill(&runtime);

    let report = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            query: Some("release".to_string()),
            include_disabled: true,
            include_retired: true,
            limit: 10,
        })
        .expect("list");

    assert_eq!(report.total, 1);
    assert_eq!(report.runtime_skills, 1);
    assert_eq!(report.skills[0].name, "runtime_skill__release_guard");
    assert_eq!(report.skills[0].title, "Release guard");
}

#[test]
fn runtime_gets_runtime_skill_detail_without_executing_it() {
    let runtime = test_runtime();
    seed_release_skill(&runtime);

    let detail = runtime
        .get_runtime_skill(RuntimeSkillDetailRequest {
            name: "runtime_skill__release_guard".to_string(),
        })
        .expect("detail");

    assert_eq!(detail.summary.name, "runtime_skill__release_guard");
    assert!(detail.procedure_text.contains("run gates"));
    assert!(detail.raw_content.contains("procedural_runtime_skill"));
}

#[test]
fn runtime_skill_management_mutations_require_existing_runtime_skill() {
    let runtime = test_runtime();
    assert!(runtime
        .edit_runtime_skill(release_skill_edit_request())
        .is_err());

    seed_release_skill(&runtime);

    let edited = runtime
        .edit_runtime_skill(release_skill_edit_request())
        .expect("edit");
    assert!(edited.accepted);
    assert!(edited.changed);

    let disabled = runtime
        .set_runtime_skill_enabled(RuntimeSkillSetEnabledRequest {
            name: edited.name.clone(),
            enabled: false,
        })
        .expect("disable");
    assert!(disabled.accepted);

    let list = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            query: None,
            include_disabled: false,
            include_retired: true,
            limit: 10,
        })
        .expect("list");
    assert!(list.skills.iter().all(|skill| skill.name != edited.name));

    let deleted = runtime
        .delete_runtime_skill(RuntimeSkillDeleteRequest {
            name: edited.name.clone(),
        })
        .expect("delete");
    assert!(deleted.accepted);

    assert!(runtime
        .get_runtime_skill(RuntimeSkillDetailRequest { name: edited.name })
        .is_err());
}
