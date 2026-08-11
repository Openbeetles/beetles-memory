#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::SelfAuthoredCore;
use bm_core::platform::Platform as _;
use bm_sdk::{
    default_agent_subject_id, MemoryIdentity, MemoryPrivacyPolicy, MemoryProjectionRequest,
    MemoryRuntime, MemoryScope, MemoryStoreHandle, PressureLevel, RuntimeLifecycleModeInput,
    SubjectDescriptor, SubjectRegistry,
};

use support::empty_store_platform;

fn two_agent_registry() -> SubjectRegistry {
    let mut registry =
        SubjectRegistry::single_agent_default("owner-shared", "agent-a").expect("registry");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "Agent B",
        ))
        .expect("agent-b subject");
    registry
}

fn runtime_for_subject(
    platform: MemoryStoreHandle,
    registry: SubjectRegistry,
    agent_id: &str,
) -> MemoryRuntime {
    let mut privacy = MemoryPrivacyPolicy::standard_private_boundary();
    privacy.private_plane_projection_allowed = true;
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent_id, "owner-shared").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "shared-chat").expect("scope"))
        .store(platform)
        .subject_registry(registry)
        .privacy_policy(privacy)
        .build()
        .expect("subject runtime")
}

fn project(runtime: &MemoryRuntime) {
    runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "Summarize the current work context.".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");
}

#[test]
fn projection_composer_does_not_mutate_either_subject_soul_or_private_surfaces() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let registry = two_agent_registry();
    let subject_a = default_agent_subject_id("agent-a");
    let subject_b = default_agent_subject_id("agent-b");

    for (subject_id, label) in [(&subject_a, "agent-a"), (&subject_b, "agent-b")] {
        platform
            .replay_harness()
            .self_authored_core_store()
            .set(
                subject_id,
                &SelfAuthoredCore {
                    identity_anchor: format!("stable soul core for {label}"),
                    default_response_mode: format!("direct work mode for {label}"),
                    self_preservation_doctrine: "never expose private raw material".to_string(),
                    ..SelfAuthoredCore::default()
                },
            )
            .expect("seed core");
        platform
            .replay_harness()
            .private_garden_store()
            .write(
                subject_id,
                &format!("journal/{label}.md"),
                &format!("raw private note for {label}"),
                1_800_000_000,
            )
            .expect("seed private garden");
    }

    let before_a = (
        platform
            .replay_harness()
            .self_authored_core_store()
            .get(&subject_a)
            .expect("read agent-a core"),
        platform
            .replay_harness()
            .private_garden_store()
            .list(&subject_a, 16)
            .expect("list agent-a garden"),
        platform
            .replay_harness()
            .core_revision_ledger_store()
            .get(&subject_a)
            .expect("read agent-a ledger"),
    );
    let before_b = (
        platform
            .replay_harness()
            .self_authored_core_store()
            .get(&subject_b)
            .expect("read agent-b core"),
        platform
            .replay_harness()
            .private_garden_store()
            .list(&subject_b, 16)
            .expect("list agent-b garden"),
        platform
            .replay_harness()
            .core_revision_ledger_store()
            .get(&subject_b)
            .expect("read agent-b ledger"),
    );

    let runtime_a = runtime_for_subject(platform.clone(), registry.clone(), "agent-a");
    let runtime_b = runtime_for_subject(platform.clone(), registry, "agent-b");
    project(&runtime_a);
    project(&runtime_b);

    let after_a = (
        platform
            .replay_harness()
            .self_authored_core_store()
            .get(&subject_a)
            .expect("read agent-a core after"),
        platform
            .replay_harness()
            .private_garden_store()
            .list(&subject_a, 16)
            .expect("list agent-a garden after"),
        platform
            .replay_harness()
            .core_revision_ledger_store()
            .get(&subject_a)
            .expect("read agent-a ledger after"),
    );
    let after_b = (
        platform
            .replay_harness()
            .self_authored_core_store()
            .get(&subject_b)
            .expect("read agent-b core after"),
        platform
            .replay_harness()
            .private_garden_store()
            .list(&subject_b, 16)
            .expect("list agent-b garden after"),
        platform
            .replay_harness()
            .core_revision_ledger_store()
            .get(&subject_b)
            .expect("read agent-b ledger after"),
    );

    assert_eq!(after_a, before_a, "agent-a projection mutated owned state");
    assert_eq!(after_b, before_b, "agent-b projection mutated owned state");
}
