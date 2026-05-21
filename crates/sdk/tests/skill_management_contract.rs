use bm_sdk::{
    MemoryIdentity, MemoryRuntime, MemoryScope, MemorySkillDeleteRequest, MemorySkillDetailRequest,
    MemorySkillListRequest, MemorySkillOrigin, MemorySkillSetEnabledRequest,
    MemorySkillUpsertRequest, ProfileId, StoreBackendConfig, StorePlatform,
};

fn test_runtime() -> MemoryRuntime {
    let profile = ProfileId::ServerLinuxDevFull;
    let store = StorePlatform::open(StoreBackendConfig::in_memory(profile).expect("store config"))
        .expect("store");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("skill-test-agent", "owner-default").expect("identity"))
        .scope(MemoryScope::new("console", "chat-1").expect("scope"))
        .profile(profile)
        .store_platform(store)
        .build()
        .expect("runtime")
}

fn release_skill_request() -> MemorySkillUpsertRequest {
    MemorySkillUpsertRequest {
        name: Some("runtime_skill__release_guard".to_string()),
        title: "Release guard".to_string(),
        topic: "release".to_string(),
        summary: "Check release artifacts before publishing.".to_string(),
        procedure: "1. run gates\n2. inspect artifacts\n3. dry run publish".to_string(),
        citations: vec!["test".to_string()],
        source_chat_id: Some("chat-1".to_string()),
        observed_at: 1_800_000_000,
    }
}

#[test]
fn runtime_lists_user_and_runtime_skills_with_summary_counts() {
    let runtime = test_runtime();
    runtime
        .upsert_skill(release_skill_request())
        .expect("upsert");

    let report = runtime
        .list_skills(MemorySkillListRequest {
            query: Some("release".to_string()),
            include_disabled: true,
            include_retired: true,
            limit: 10,
        })
        .expect("list");

    assert_eq!(report.total, 1);
    assert_eq!(report.user_provided, 1);
    assert_eq!(report.runtime_learned, 0);
    assert_eq!(report.skills[0].origin, MemorySkillOrigin::UserProvided);
    assert_eq!(report.skills[0].name, "runtime_skill__release_guard");
    assert_eq!(report.skills[0].title, "Release guard");
}

#[test]
fn runtime_gets_skill_detail_without_executing_it() {
    let runtime = test_runtime();
    let created = runtime
        .upsert_skill(release_skill_request())
        .expect("upsert");
    let detail = runtime
        .get_skill(MemorySkillDetailRequest { name: created.name })
        .expect("detail");

    assert_eq!(detail.summary.name, "runtime_skill__release_guard");
    assert!(detail.procedure_text.contains("run gates"));
    assert!(detail.raw_content.contains("procedural_runtime_skill"));
}

#[test]
fn runtime_skill_management_mutations_go_through_runtime_reports() {
    let runtime = test_runtime();
    let upsert = runtime
        .upsert_skill(release_skill_request())
        .expect("upsert");
    assert!(upsert.accepted);
    assert!(upsert.changed);

    let disabled = runtime
        .set_skill_enabled(MemorySkillSetEnabledRequest {
            name: upsert.name.clone(),
            enabled: false,
        })
        .expect("disable");
    assert!(disabled.accepted);

    let list = runtime
        .list_skills(MemorySkillListRequest {
            query: None,
            include_disabled: false,
            include_retired: true,
            limit: 10,
        })
        .expect("list");
    assert!(list.skills.iter().all(|skill| skill.name != upsert.name));

    let deleted = runtime
        .delete_skill(MemorySkillDeleteRequest {
            name: upsert.name.clone(),
        })
        .expect("delete");
    assert!(deleted.accepted);

    assert!(runtime
        .get_skill(MemorySkillDetailRequest { name: upsert.name })
        .is_err());
}
