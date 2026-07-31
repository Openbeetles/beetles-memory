mod support;
use bm_sdk::{
    ErrorClass, MemoryIdentity, MemoryRuntime, MemoryScope, MemoryWriteRequest,
    RuntimeSkillDetailRequest, RuntimeSkillEditRequest, RuntimeSkillListRequest,
    RuntimeSkillOwnerLocator, RuntimeSkillRetireRequest, RuntimeSkillSetEnabledRequest,
    RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
};

fn test_runtime() -> MemoryRuntime {
    let profile = support::host_test_profile();
    let store =
        support::open_memory_store(StoreBackendConfig::in_memory(profile).expect("store config"))
            .expect("store");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("console", "chat-1").expect("scope"))
        .store(store)
        .build()
        .expect("runtime")
}

fn seed_release_skill(runtime: &MemoryRuntime) -> RuntimeSkillOwnerLocator {
    let report = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                name: "runtime_skill__release_guard".to_string(),
                title: "Release guard".to_string(),
                topic: "release".to_string(),
                summary: "Check release artifacts before publishing.".to_string(),
                content: "1. run gates\n2. inspect artifacts\n3. dry run publish".to_string(),
                citations: vec!["test".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            })],
            owning_scope: support::runtime_skill_subject_scope(),
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed procedural skill");
    assert!(report.changed > 0);
    runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: support::runtime_skill_subject_scope(),
            query: Some("release".to_string()),
            include_disabled: true,
            include_retired: true,
            limit: 1,
        })
        .expect("seeded skill list")
        .skills
        .into_iter()
        .next()
        .expect("seeded skill")
        .locator
}

fn release_skill_write(summary: &str) -> MemoryWriteRequest {
    MemoryWriteRequest::Procedural {
        writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
            name: "runtime_skill__release_guard".to_string(),
            title: "Release guard".to_string(),
            topic: "release".to_string(),
            summary: summary.to_string(),
            content: "1. run gates\n2. inspect artifacts\n3. dry run publish".to_string(),
            citations: vec!["test".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        })],
        owning_scope: support::runtime_skill_subject_scope(),
        source: RuntimeSkillWriteSource::Manual,
    }
}

fn release_skill_edit_request(locator: RuntimeSkillOwnerLocator) -> RuntimeSkillEditRequest {
    RuntimeSkillEditRequest {
        locator,
        title: "Release guard".to_string(),
        topic: "release".to_string(),
        summary: "Check release artifacts and changelog before publishing.".to_string(),
        procedure: "1. run gates\n2. inspect artifacts\n3. inspect changelog".to_string(),
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
            owning_scope: support::runtime_skill_subject_scope(),
            query: Some("release".to_string()),
            include_disabled: true,
            include_retired: true,
            limit: 10,
        })
        .expect("list");

    assert_eq!(report.total, 1);
    assert_eq!(report.runtime_skills, 1);
    assert_eq!(report.skills[0].title, "Release guard");
    assert_eq!(report.skills[0].locator.owner_revision(), 1);
}

#[test]
fn runtime_gets_runtime_skill_detail_without_executing_it() {
    let runtime = test_runtime();
    let locator = seed_release_skill(&runtime);

    let detail = runtime
        .get_runtime_skill(RuntimeSkillDetailRequest { locator })
        .expect("detail");

    assert_eq!(detail.summary.locator.owner_revision(), 1);
    assert!(detail.procedure_text.contains("run gates"));
    assert!(detail.raw_content.contains("\"schema_version\": 1"));
}

#[test]
fn runtime_skill_management_mutations_require_existing_runtime_skill() {
    let runtime = test_runtime();
    let locator = seed_release_skill(&runtime);
    let stale_locator =
        RuntimeSkillOwnerLocator::try_new(locator.owning_scope().clone(), locator.owner_id(), 2)
            .expect("stale locator shape");
    let stale_error = runtime
        .edit_runtime_skill(release_skill_edit_request(stale_locator))
        .expect_err("stale locator");
    assert_eq!(stale_error.class(), Some(ErrorClass::Conflict));
    let unchanged = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: support::runtime_skill_subject_scope(),
            query: None,
            include_disabled: true,
            include_retired: true,
            limit: 10,
        })
        .expect("unchanged list");
    assert_eq!(unchanged.skills[0].locator.owner_revision(), 1);

    let edited = runtime
        .edit_runtime_skill(release_skill_edit_request(locator))
        .expect("edit");
    assert!(edited.accepted);
    assert!(edited.changed);

    let disabled = runtime
        .set_runtime_skill_enabled(RuntimeSkillSetEnabledRequest {
            locator: edited.current_locator.clone(),
            enabled: false,
            observed_at: 1_800_000_002,
        })
        .expect("disable");
    assert!(disabled.accepted);

    let list = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: support::runtime_skill_subject_scope(),
            query: None,
            include_disabled: false,
            include_retired: true,
            limit: 10,
        })
        .expect("list");
    assert!(list.skills.is_empty());

    let retired_mutation = runtime
        .retire_runtime_skill(RuntimeSkillRetireRequest {
            locator: disabled.current_locator.clone(),
            observed_at: 1_800_000_003,
        })
        .expect("retire");
    assert!(retired_mutation.accepted);

    let retired = runtime
        .get_runtime_skill(RuntimeSkillDetailRequest {
            locator: retired_mutation.current_locator,
        })
        .expect("retired detail");
    assert_eq!(retired.summary.status, "retired");
}

#[test]
fn repeated_creation_authority_is_idempotent_or_advances_one_exact_revision() {
    let runtime = test_runtime();
    let first = runtime
        .write(release_skill_write(
            "Check release artifacts before publishing.",
        ))
        .expect("first write");
    assert_eq!(first.changed, 1);

    let duplicate = runtime
        .write(release_skill_write(
            "Check release artifacts before publishing.",
        ))
        .expect("idempotent duplicate");
    assert_eq!(duplicate.changed, 0);

    let revised = runtime
        .write(release_skill_write(
            "Check release artifacts and receipts before publishing.",
        ))
        .expect("revised write");
    assert_eq!(revised.changed, 1);

    let listed = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: support::runtime_skill_subject_scope(),
            query: Some("release".to_string()),
            include_disabled: true,
            include_retired: true,
            limit: 10,
        })
        .expect("list");
    assert_eq!(listed.skills.len(), 1);
    assert_eq!(listed.skills[0].locator.owner_revision(), 2);
}
