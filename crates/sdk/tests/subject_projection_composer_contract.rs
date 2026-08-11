#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::SelfAuthoredCore;
use bm_core::platform::Platform as _;
use bm_sdk::{
    default_agent_subject_id, LongTermMemoryDraft, LongTermMemoryKind, MemoryIdentity,
    MemoryPrivacyClass, MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRuntime, MemoryScope,
    MemoryStoreHandle, MemoryWriteRequest, ParsedLongTermMemoryExtraction, PressureLevel,
    QueryFacetInput, RuntimeLifecycleModeInput, SubjectDescriptor, SubjectRegistry,
};

use support::empty_store_platform;

const SHARED_FACT: &str = "The shared release train is named Copper Finch.";

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

fn project(runtime: &MemoryRuntime) -> bm_sdk::MemoryProjectionOutput {
    runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![QueryFacetInput::Keyword("release".to_string())],
            user_query: "What is the shared release train called?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection")
}

#[test]
fn one_shared_fact_can_produce_distinct_subject_projections() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let registry = two_agent_registry();
    let subject_a = default_agent_subject_id("agent-a");
    let subject_b = default_agent_subject_id("agent-b");
    for (subject_id, identity_anchor) in [
        (&subject_a, "AGENT-A-DIRECT-ENGINEERING-PERSONA"),
        (&subject_b, "AGENT-B-CAUTIOUS-REVIEW-PERSONA"),
    ] {
        platform
            .replay_harness()
            .self_authored_core_store()
            .set(
                subject_id,
                &SelfAuthoredCore {
                    identity_anchor: identity_anchor.to_string(),
                    default_response_mode: identity_anchor.to_string(),
                    self_preservation_doctrine: "preserve subject ownership".to_string(),
                    ..SelfAuthoredCore::default()
                },
            )
            .expect("seed subject soul");
    }

    let runtime_a = runtime_for_subject(platform.clone(), registry.clone(), "agent-a");
    let runtime_b = runtime_for_subject(platform.clone(), registry, "agent-b");
    let write = runtime_a
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Fact,
                    topic: "shared_release_train".to_string(),
                    content: SHARED_FACT.to_string(),
                    keywords: vec!["release".to_string(), "copper".to_string()],
                    privacy: MemoryPrivacyClass::PublicRuntime,
                    source_chat_id: Some("shared-chat".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["shared-chat:turn-1".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    last_confirmed_at: Some(1_800_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
        })
        .expect("write shared fact");
    assert!(write.accepted, "{write:#?}");
    assert_eq!(write.changed, 1, "{write:#?}");
    let shared_records = platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-shared")
        .expect("shared store")
        .list(8)
        .expect("shared records");
    assert_eq!(shared_records.len(), 1, "{shared_records:#?}");
    let projection_a = project(&runtime_a);
    let projection_b = project(&runtime_b);
    assert!(
        projection_a
            .report()
            .shared_fact_projection()
            .contains(SHARED_FACT),
        "agent-a did not receive canonical shared fact: {}",
        projection_a.report().shared_fact_projection()
    );
    assert!(
        projection_b
            .report()
            .shared_fact_projection()
            .contains(SHARED_FACT),
        "agent-b did not receive canonical shared fact: {}",
        projection_b.report().shared_fact_projection()
    );
    assert_eq!(
        projection_a.report().shared_fact_projection(),
        projection_b.report().shared_fact_projection(),
        "both subjects must project the same canonical shared owner"
    );

    let prompt_a = projection_a.provider_payload().system_memory_block();
    let prompt_b = projection_b.provider_payload().system_memory_block();
    assert!(
        prompt_a.contains("AGENT-A-DIRECT-ENGINEERING-PERSONA"),
        "{prompt_a}"
    );
    assert!(
        !prompt_a.contains("AGENT-B-CAUTIOUS-REVIEW-PERSONA"),
        "{prompt_a}"
    );
    assert!(
        prompt_b.contains("AGENT-B-CAUTIOUS-REVIEW-PERSONA"),
        "{prompt_b}"
    );
    assert!(
        !prompt_b.contains("AGENT-A-DIRECT-ENGINEERING-PERSONA"),
        "{prompt_b}"
    );
    assert_ne!(
        prompt_a, prompt_b,
        "subject projections must remain persona-specific"
    );
}
