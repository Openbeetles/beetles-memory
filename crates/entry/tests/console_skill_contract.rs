use bm_adapter::{AdapterCommand, AdapterOperation, TransportKind, TransportMode};
use bm_entry::{
    EntryAuthConfig, EntryAuthDecision, EntryConsoleRuntimeSkillEdit, EntryConsoleSkillSetEnabled,
    EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig, EntryScope,
    EntryStoreConfig, EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryWriteRequest, ProfileId, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendKind,
};

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
fn console_skill_facade_edits_disables_and_deletes_runtime_skills_only() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    seed_runtime_skill(&runtime, "runtime_skill__release_guard");

    assert!(runtime
        .console_edit_runtime_skill(
            "not_runtime_skill",
            EntryConsoleRuntimeSkillEdit {
                title: "Manual".to_string(),
                topic: "manual".to_string(),
                summary: "Should not create manual skills.".to_string(),
                procedure: "forbidden".to_string(),
                citations: vec!["entry-test".to_string()],
                source_chat_id: None,
                edit_reason: None,
            },
        )
        .is_err());

    let listed = runtime
        .console_skills(Some("release".to_string()))
        .expect("list");
    assert_eq!(listed.skills.len(), 1);
    assert_eq!(listed.runtime_learned, 1);

    let detail = runtime
        .console_skill_detail("runtime_skill__release_guard")
        .expect("detail")
        .expect("skill exists");
    assert!(detail.procedure_text.contains("run gates"));

    let edited = runtime
        .console_edit_runtime_skill(
            "runtime_skill__release_guard",
            EntryConsoleRuntimeSkillEdit {
                title: "Release guard".to_string(),
                topic: "release".to_string(),
                summary: "Check release artifacts and changelog before publishing.".to_string(),
                procedure: "1. run gates\n2. inspect artifacts\n3. inspect changelog".to_string(),
                citations: vec!["entry-test-edit".to_string()],
                source_chat_id: None,
                edit_reason: Some("entry_test_edit".to_string()),
            },
        )
        .expect("edit");
    assert!(edited.accepted);

    let disabled = runtime
        .console_set_skill_enabled(
            "runtime_skill__release_guard",
            EntryConsoleSkillSetEnabled { enabled: false },
        )
        .expect("disable")
        .expect("skill exists");
    assert!(disabled.accepted);

    let deleted = runtime
        .console_delete_skill("runtime_skill__release_guard")
        .expect("delete")
        .expect("skill exists");
    assert!(deleted.accepted);
    assert!(runtime
        .console_skill_detail("runtime_skill__release_guard")
        .expect("deleted detail")
        .is_none());
}

fn seed_runtime_skill(runtime: &EntryRuntime, name: &str) {
    let response = runtime
        .handle(
            EntryTransportContext {
                request_id: "seed-runtime-skill".to_string(),
                transport: TransportKind::Cli,
                mode: TransportMode::InProcess,
                operation: AdapterOperation::Write,
                source_id: "entry-test".to_string(),
                source_kind: "test".to_string(),
                idempotency_key: format!("seed-{name}"),
                audit_id: "seed-audit".to_string(),
                auth: EntryAuthDecision::authenticated("local", "operator"),
            },
            AdapterCommand::Write(MemoryWriteRequest::Procedural {
                writes: vec![RuntimeSkillWrite {
                    name: name.to_string(),
                    title: "Release guard".to_string(),
                    topic: "release".to_string(),
                    summary: "Check release artifacts before publishing.".to_string(),
                    content: "1. run gates\n2. inspect artifacts\n3. dry run publish".to_string(),
                    citations: vec!["entry-test".to_string()],
                    source_chat_id: Some("chat-1".to_string()),
                    observed_at: 1_800_000_000,
                }],
                source: RuntimeSkillWriteSource::Manual,
            }),
        )
        .expect("seed write");
    assert!(matches!(
        response.adapter,
        bm_adapter::AdapterResponse::Accepted { .. }
    ));
}
