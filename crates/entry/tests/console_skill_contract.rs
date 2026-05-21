use bm_entry::{
    EntryAuthConfig, EntryConsoleSkillSetEnabled, EntryConsoleSkillUpsert, EntryIdempotencyConfig,
    EntryIdentity, EntryRuntime, EntryRuntimeConfig, EntryScope, EntryStoreConfig,
    EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn config() -> EntryRuntimeConfig {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "console-skill-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "console".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_disabled().with_cli(true),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 16 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    }
}

#[test]
fn console_skill_facade_lists_imports_edits_disables_and_deletes() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");

    let created = runtime
        .console_upsert_skill(EntryConsoleSkillUpsert {
            name: None,
            title: "Release guard".to_string(),
            topic: "release".to_string(),
            summary: "Check release artifacts before publishing.".to_string(),
            procedure: "1. run gates\n2. inspect artifacts\n3. dry run publish".to_string(),
            citations: vec!["entry-test".to_string()],
            source_chat_id: None,
        })
        .expect("create");
    assert!(created.accepted);

    let listed = runtime
        .console_skills(Some("release".to_string()))
        .expect("list");
    assert_eq!(listed.skills.len(), 1);
    assert_eq!(listed.skills[0].origin, "user_provided");

    let detail = runtime
        .console_skill_detail(&created.name)
        .expect("detail")
        .expect("skill exists");
    assert!(detail.procedure_text.contains("run gates"));

    let edited = runtime
        .console_upsert_skill(EntryConsoleSkillUpsert {
            name: Some(created.name.clone()),
            title: "Release guard".to_string(),
            topic: "release".to_string(),
            summary: "Check release artifacts and changelog before publishing.".to_string(),
            procedure: "1. run gates\n2. inspect artifacts\n3. inspect changelog".to_string(),
            citations: vec!["entry-test-edit".to_string()],
            source_chat_id: None,
        })
        .expect("edit");
    assert!(edited.accepted);

    let disabled = runtime
        .console_set_skill_enabled(
            &created.name,
            EntryConsoleSkillSetEnabled { enabled: false },
        )
        .expect("disable")
        .expect("skill exists");
    assert!(disabled.accepted);

    let deleted = runtime
        .console_delete_skill(&created.name)
        .expect("delete")
        .expect("skill exists");
    assert!(deleted.accepted);
    assert!(runtime
        .console_skill_detail(&created.name)
        .expect("deleted detail")
        .is_none());
}
