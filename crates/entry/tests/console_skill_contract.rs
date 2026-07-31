use bm_adapter::{AdapterCommand, AdapterOperation, TransportKind, TransportMode};
use bm_entry::{
    EntryAuthConfig, EntryConsoleRuntimeSkillEdit, EntryConsoleSkillSetEnabled,
    EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig, EntryScope,
    EntryTransportConfig, EntryTransportContext,
};
use bm_sdk::{
    ErrorClass, MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryWriteRequest, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig,
};

mod support;

fn config() -> EntryRuntimeConfig {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let profile = support::host_production_profile();
    EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "console-skill-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "console".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(profile)
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_disabled().with_cli(true),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 16 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    }
}

#[test]
fn console_skill_facade_edits_disables_and_retires_runtime_skills_only() {
    let runtime = EntryRuntime::open(config()).expect("entry runtime");
    seed_runtime_skill(&runtime, "runtime_skill__release_guard");

    let listed = runtime
        .console_skills(Some("release".to_string()))
        .expect("list");
    assert_eq!(listed.skills.len(), 1);
    assert_eq!(listed.runtime_learned, 1);
    let locator = listed.skills[0].locator.clone();

    let stale_locator = bm_sdk::RuntimeSkillOwnerLocator::try_new(
        locator.owning_scope().clone(),
        locator.owner_id(),
        locator.owner_revision() + 1,
    )
    .expect("stale locator shape");
    let stale_error = runtime
        .console_edit_runtime_skill(EntryConsoleRuntimeSkillEdit {
            locator: stale_locator,
            title: "Manual".to_string(),
            topic: "manual".to_string(),
            summary: "A stale locator must not create a skill.".to_string(),
            procedure: "forbidden".to_string(),
            edit_reason: None,
        })
        .expect_err("stale locator");
    assert_eq!(stale_error.class(), Some(ErrorClass::Conflict));

    let detail = runtime
        .console_skill_detail(locator.clone())
        .expect("detail");
    assert!(detail.procedure_text.contains("run gates"));

    let edited = runtime
        .console_edit_runtime_skill(EntryConsoleRuntimeSkillEdit {
            locator,
            title: "Release guard".to_string(),
            topic: "release".to_string(),
            summary: "Check release artifacts and changelog before publishing.".to_string(),
            procedure: "1. run gates\n2. inspect artifacts\n3. inspect changelog".to_string(),
            edit_reason: Some("entry_test_edit".to_string()),
        })
        .expect("edit");
    assert!(edited.accepted);

    let disabled = runtime
        .console_set_skill_enabled(EntryConsoleSkillSetEnabled {
            locator: edited.current_locator,
            enabled: false,
        })
        .expect("disable");
    assert!(disabled.accepted);

    let retired_mutation = runtime
        .console_retire_skill(disabled.current_locator)
        .expect("retire");
    assert!(retired_mutation.accepted);
    let retired = runtime
        .console_skill_detail(retired_mutation.current_locator)
        .expect("retired detail");
    assert_eq!(retired.summary.status, "retired");
}

fn seed_runtime_skill(runtime: &EntryRuntime, name: &str) {
    let response = runtime
        .handle(
            EntryTransportContext::new(
                "seed-runtime-skill",
                TransportKind::Cli,
                TransportMode::InProcess,
                AdapterOperation::Write,
                "entry-test",
                "test",
                format!("seed-{name}"),
                "seed-audit",
                support::trusted_local_auth("operator"),
            ),
            AdapterCommand::Write(MemoryWriteRequest::Procedural {
                writes: vec![support::governed_runtime_skill_write(RuntimeSkillWrite {
                    name: name.to_string(),
                    title: "Release guard".to_string(),
                    topic: "release".to_string(),
                    summary: "Check release artifacts before publishing.".to_string(),
                    content: "1. run gates\n2. inspect artifacts\n3. dry run publish".to_string(),
                    citations: vec!["entry-test".to_string()],
                    source_chat_id: Some("chat-1".to_string()),
                    observed_at: 1_700_000_000,
                })],
                owning_scope: support::runtime_skill_subject_scope("console-skill-agent"),
                source: RuntimeSkillWriteSource::Manual,
            }),
        )
        .expect("seed write");
    assert!(matches!(
        response.adapter,
        bm_adapter::AdapterResponse::Accepted { .. }
    ));
}
