mod support;

use bm_sdk::{
    default_agent_subject_id, default_memory_space_id, primary_human_subject_id,
    LongTermMemoryDraft, LongTermMemoryKind, LongTermMemoryQuery, MemoryIdentity,
    MemoryLongTermControlView, MemoryLongTermDetailRequest, MemoryLongTermListRequest,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermTarget,
    MemoryPrivacyClass, MemoryRuntime, MemoryScope, MemoryWriteRequest,
    ParsedLongTermMemoryExtraction, RuntimeLifecycleModeInput, SubjectRegistry,
    SubjectRelationshipGraph, SubjectScopedRuntime,
};

#[test]
fn public_detail_returns_memory_space_scoped_human_tombstone_by_record_id() {
    let profile = support::host_test_profile();
    let platform = support::empty_store_platform(profile);
    let human_subject_id = primary_human_subject_id("owner-default");
    let subject_registry =
        SubjectRegistry::single_agent_default("owner-default", "agent-main").expect("registry");
    let subject_relationship_graph =
        SubjectRelationshipGraph::single_agent_default(&subject_registry)
            .expect("relationship graph");
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "human-control").expect("scope"))
        .store(platform.clone())
        .subject_registry(subject_registry)
        .subject_relationship_graph(subject_relationship_graph)
        .scoped_runtime(SubjectScopedRuntime {
            memory_space_id: default_memory_space_id("owner-default"),
            mounted_subject_id: human_subject_id.clone(),
            actor_subject_id: human_subject_id.clone(),
            agent_id: "agent-main".to_string(),
            relationship_scope: None,
            projection_policy: "subject_aware_default".to_string(),
            write_policy: "subject_candidate_then_space_governance".to_string(),
        })
        .build()
        .expect("human-mounted desktop embedded runtime");

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Profile,
                    topic: "public_human_control".to_string(),
                    content: "The public SDK must retain governed deletion detail.".to_string(),
                    keywords: vec!["public".to_string(), "deletion".to_string()],
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    source_chat_id: Some("human-control".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["turn:human-control".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    last_confirmed_at: Some(1_800_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed subject-scoped long-term memory through the public SDK");
    let record_id = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list subject-scoped long-term memory")
        .records
        .into_iter()
        .next()
        .expect("seeded record")
        .record
        .id;

    let delete = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
            },
            reason: "human deleted memory from the public control surface".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete subject-scoped memory through the public SDK");
    assert!(delete.accepted);

    for view in [
        MemoryLongTermControlView::HostUi,
        MemoryLongTermControlView::Operator,
    ] {
        let detail = runtime
            .get_long_term_memory(MemoryLongTermDetailRequest {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                view,
            })
            .expect("read deleted detail through the public SDK");
        assert!(detail.record.is_none());
        assert!(!detail.revisions.is_empty());
        let tombstone = detail.tombstone.expect("typed deletion tombstone");
        assert_eq!(tombstone.record_id, record_id);
        assert_eq!(
            tombstone.actor_subject_id.as_deref(),
            Some(human_subject_id.as_str())
        );
        assert_eq!(tombstone.factual_owner_id, runtime.memory_space_id());
        assert_eq!(tombstone.memory_space_id, runtime.memory_space_id());
    }

    let other_subject = default_agent_subject_id("agent-main");
    let other_subject_runtime = support::test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "sdk.direct",
        "other-subject-control",
        &other_subject,
    );
    let same_space_other_subject = other_subject_runtime
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(record_id.clone()),
            view: MemoryLongTermControlView::Operator,
        })
        .expect("same MemorySpace subject reads the shared governed tombstone");
    assert!(same_space_other_subject.record.is_none());
    assert!(!same_space_other_subject.revisions.is_empty());
    assert_eq!(
        same_space_other_subject
            .tombstone
            .expect("same-space shared tombstone")
            .record_id,
        record_id
    );

    let other_owner_subject = default_agent_subject_id("agent-other");
    let other_space_runtime = support::test_runtime_with_identity_scope_and_subject(
        platform,
        profile,
        "agent-other",
        "owner-other",
        &other_owner_subject,
        "sdk.direct",
        "other-space-control",
    );
    let cross_space = other_space_runtime
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(record_id),
            view: MemoryLongTermControlView::Operator,
        })
        .expect("cross-space detail remains a safe empty report");
    assert!(cross_space.record.is_none());
    assert!(cross_space.revisions.is_empty());
    assert!(cross_space.tombstone.is_none());
}
