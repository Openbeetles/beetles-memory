#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_sdk::{
    default_agent_subject_id, primary_human_subject_id, MemoryIdentity, MemoryPrivacyPolicy,
    MemoryProjectionOutput, MemoryProjectionRequest, MemoryRuntime, MemoryScope, MemoryStoreHandle,
    PressureLevel, RuntimeLifecycleModeInput, SubjectDescriptor, SubjectRegistry,
    SubjectSoulFoundingCharterSeedV1, SubjectSoulProvisionIntentV1, SubjectSoulReadSelectorV1,
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

fn provision(runtime: &MemoryRuntime, label: &str) {
    runtime
        .provision_subject_soul(SubjectSoulProvisionIntentV1::Founding {
            operation_id: format!("projection-no-mutation-{label}"),
            human_actor_subject_id: primary_human_subject_id("owner-shared"),
            charter: Box::new(
                SubjectSoulFoundingCharterSeedV1 {
                    identity_anchor: Some(format!("{label}-SOUL-PROVIDER-ONLY")),
                    character_tendencies: vec![format!("{label}-STABLE-TENDENCY")],
                    priority_constitution: vec!["projection remains read-only".to_string()],
                    non_negotiables: vec!["never expose private raw material".to_string()],
                    default_response_mode: Some(format!("direct work mode for {label}")),
                    default_initiative_posture: None,
                    default_relationship_posture: None,
                    boundary_doctrine: None,
                    truth_seeking_commitment: None,
                    self_preservation_doctrine: None,
                    repair_doctrine: None,
                    change_principle: None,
                }
                .canonicalize()
                .expect("canonical no-mutation Soul seed"),
            ),
            source_asserted_at: Some(1_700_000_000),
        })
        .expect("provision typed Soul");
}

fn project(runtime: &MemoryRuntime) -> MemoryProjectionOutput {
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
        .expect("project")
}

#[test]
fn projection_composer_does_not_mutate_either_subject_soul_or_private_surfaces() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let registry = two_agent_registry();
    let runtime_a = runtime_for_subject(platform.clone(), registry.clone(), "agent-a");
    let runtime_b = runtime_for_subject(platform.clone(), registry, "agent-b");
    provision(&runtime_a, "agent-a");
    provision(&runtime_b, "agent-b");
    let before_a = runtime_a
        .export_subject_soul_operator_safe(SubjectSoulReadSelectorV1::Current)
        .expect("agent-a verified Soul before projection");
    let before_b = runtime_b
        .export_subject_soul_operator_safe(SubjectSoulReadSelectorV1::Current)
        .expect("agent-b verified Soul before projection");
    let before_store = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("canonical store before projection");
    let protected_namespaces = [
        "self_model",
        "self_authored_core",
        "core_revision_ledger",
        "self_continuity",
        "relationship_portfolio",
        "relationship_topology",
        "autonomy_strategy",
        "inner_life",
        "felt_significance",
        "temperament_continuity",
        "inner_conflict",
        "mental_privacy",
        "private_doc",
        "private_garden",
        "outer_voice",
        "subject_soul_lifecycle_heads",
        "subject_soul_revision_materials",
        "subject_soul_scope_manifests",
        "subject_soul_generation_tombstones",
        "subject_soul_relationship_projections",
        "subject_soul_operation_results",
        "memory_mutation_receipts",
        "memory_mutation_audits",
    ];
    let before_protected_docs = before_store
        .json_docs
        .iter()
        .filter(|document| protected_namespaces.contains(&document.namespace.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let before_events = before_store
        .events
        .iter()
        .filter(|event| protected_namespaces.contains(&event.plane.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    let projection_a = project(&runtime_a);
    let projection_b = project(&runtime_b);
    let prompt_a = projection_a.provider_payload().system_memory_block();
    let prompt_b = projection_b.provider_payload().system_memory_block();
    assert!(prompt_a.contains("agent-a-SOUL-PROVIDER-ONLY"));
    assert!(!prompt_a.contains("agent-b-SOUL-PROVIDER-ONLY"));
    assert!(prompt_b.contains("agent-b-SOUL-PROVIDER-ONLY"));
    assert!(!prompt_b.contains("agent-a-SOUL-PROVIDER-ONLY"));
    for projection in [&projection_a, &projection_b] {
        assert!(projection
            .provider_payload()
            .system_memory_block()
            .contains("## Soul Private Runtime Context"));
        assert!(projection.report().audit().runtime_private_context_allowed);
        assert_eq!(projection.report().audit().raw_private_violation_count, 0);
    }
    assert!(!projection_a
        .report()
        .ui_api_projection()
        .contains("agent-b-SOUL-PROVIDER-ONLY"));
    assert!(!projection_b
        .report()
        .ui_api_projection()
        .contains("agent-a-SOUL-PROVIDER-ONLY"));
    assert!(!projection_a
        .report()
        .gateway_audit()
        .block
        .contains("agent-b-SOUL-PROVIDER-ONLY"));
    assert!(!projection_b
        .report()
        .gateway_audit()
        .block
        .contains("agent-a-SOUL-PROVIDER-ONLY"));

    assert_eq!(
        runtime_a
            .export_subject_soul_operator_safe(SubjectSoulReadSelectorV1::Current)
            .expect("agent-a verified Soul after projection"),
        before_a
    );
    assert_eq!(
        runtime_b
            .export_subject_soul_operator_safe(SubjectSoulReadSelectorV1::Current)
            .expect("agent-b verified Soul after projection"),
        before_b
    );
    let after_store = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("canonical store after projection");
    assert_eq!(
        after_store
            .json_docs
            .iter()
            .filter(|document| protected_namespaces.contains(&document.namespace.as_str()))
            .cloned()
            .collect::<Vec<_>>(),
        before_protected_docs,
        "projection must not mutate any typed Soul, private layer, receipt, or audit"
    );
    assert_eq!(
        after_store
            .events
            .iter()
            .filter(|event| protected_namespaces.contains(&event.plane.as_str()))
            .cloned()
            .collect::<Vec<_>>(),
        before_events,
        "projection must not append a Soul or mutation event"
    );
}
