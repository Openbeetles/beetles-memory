mod support;

use bm_sdk::{
    default_agent_subject_id, default_memory_space_id, primary_human_subject_id,
    system_governor_subject_id, LongTermMemoryDraft, LongTermMemoryKind, LongTermMemoryProvenance,
    LongTermMemoryQuery, MemoryEvidenceAuthority, MemoryIdentity, MemoryLongTermControlView,
    MemoryLongTermDetailRequest, MemoryLongTermListRequest, MemoryLongTermMutation,
    MemoryLongTermMutationRequest, MemoryLongTermTarget, MemoryPrivacyClass, MemoryRuntime,
    MemoryScope, MemorySubjectVisibilityPolicy, MemoryWriteRequest, ParsedLongTermMemoryExtraction,
    RuntimeLifecycleModeInput, SubjectDescriptor, SubjectKind, SubjectLifecycleState,
    SubjectRegistry, SubjectRelationshipGraph, SubjectScopedRuntime, SubjectVisibility,
};

fn runtime_with_actor(
    platform: bm_sdk::MemoryStoreHandle,
    subject_registry: SubjectRegistry,
    actor_subject_id: String,
    chat_id: &str,
) -> MemoryRuntime {
    let mounted_subject_id = default_agent_subject_id("agent-main");
    let subject_relationship_graph =
        SubjectRelationshipGraph::single_agent_default(&subject_registry)
            .expect("relationship graph");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", chat_id).expect("scope"))
        .store(platform)
        .subject_registry(subject_registry)
        .subject_relationship_graph(subject_relationship_graph)
        .scoped_runtime(SubjectScopedRuntime {
            memory_space_id: default_memory_space_id("owner-default"),
            mounted_subject_id,
            actor_subject_id,
            agent_id: "agent-main".to_string(),
            relationship_scope: None,
            projection_policy: "subject_aware_default".to_string(),
            write_policy: "subject_candidate_then_space_governance".to_string(),
        })
        .build()
        .expect("actor-scoped runtime")
}

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
                    subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
                    provenance: LongTermMemoryProvenance::new(
                        MemoryEvidenceAuthority::ModelInferred,
                    ),
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["turn:human-control".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed subject-scoped long-term memory through the public SDK");
    let record = runtime
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
        .record;
    let record_id = record.id.clone();

    let correction = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                replacement: LongTermMemoryDraft {
                    kind: record.kind.clone(),
                    topic: record.topic.clone(),
                    content: "The active HumanUser confirmed the corrected governed detail."
                        .to_string(),
                    keywords: record.keywords.clone(),
                    privacy: record.privacy,
                    source_chat_id: record.source_chat_id.clone(),
                    source_type: Some(record.source_type),
                    source_scope: Some(record.source_scope),
                    subject_visibility: record.subject_visibility.clone(),
                    provenance: LongTermMemoryProvenance::new(
                        MemoryEvidenceAuthority::ModelInferred,
                    ),
                    confidence: Some(record.confidence),
                    freshness: Some(record.freshness),
                    stale_hint: Some(record.stale_hint),
                    supporting_citations: record.supporting_citations.clone(),
                    canonical_entities: record.canonical_entities.clone(),
                    evidence_count: Some(record.evidence_count),
                    observed_at: Some(record.observed_at.saturating_add(1)),
                    source_revision: record.source_revision.map(|value| value.saturating_add(1)),
                },
            },
            reason: "active HumanUser confirms a corrected model-inferred fact".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("correct through active HumanUser authority");
    assert!(correction.accepted);
    let confirmed = runtime
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(record_id.clone()),
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("read human-confirmed correction")
        .record
        .expect("active record after correction");
    assert_eq!(
        confirmed.provenance.source_authority,
        MemoryEvidenceAuthority::ModelInferred,
        "human confirmation must not rewrite the original source provenance"
    );
    assert!(confirmed.last_confirmed_at.is_some());

    let mut suspended_registry =
        SubjectRegistry::single_agent_default("owner-default", "agent-main").expect("registry");
    let mut suspended_human = SubjectDescriptor::new(
        human_subject_id.clone(),
        SubjectKind::HumanUser,
        "Suspended human",
        SubjectVisibility::Visible,
    );
    suspended_human.lifecycle_state = SubjectLifecycleState::Suspended;
    suspended_registry
        .upsert_subject(suspended_human)
        .expect("replace human lifecycle");
    let non_confirming_actors = [
        (
            runtime_with_actor(
                platform.clone(),
                suspended_registry,
                human_subject_id.clone(),
                "suspended-human-correction",
            ),
            "Suspended HumanUser",
        ),
        (
            runtime_with_actor(
                platform.clone(),
                SubjectRegistry::single_agent_default("owner-default", "agent-main")
                    .expect("registry"),
                system_governor_subject_id("owner-default"),
                "system-correction",
            ),
            "SystemGovernor",
        ),
    ];
    for (actor_runtime, actor_label) in non_confirming_actors {
        let current = actor_runtime
            .get_long_term_memory(MemoryLongTermDetailRequest {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                view: MemoryLongTermControlView::HostUi,
            })
            .expect("read current record for neutral correction")
            .record
            .expect("active current record");
        actor_runtime
            .mutate_long_term_memory(MemoryLongTermMutationRequest {
                operation: MemoryLongTermMutation::Correct {
                    target: MemoryLongTermTarget::RecordId(record_id.clone()),
                    replacement: LongTermMemoryDraft {
                        kind: current.kind.clone(),
                        topic: current.topic.clone(),
                        content: format!(
                            "{actor_label} corrected content without human confirmation."
                        ),
                        keywords: current.keywords.clone(),
                        privacy: current.privacy,
                        source_chat_id: current.source_chat_id.clone(),
                        source_type: Some(current.source_type),
                        source_scope: Some(current.source_scope),
                        subject_visibility: current.subject_visibility.clone(),
                        provenance: current.provenance,
                        confidence: Some(current.confidence),
                        freshness: Some(current.freshness),
                        stale_hint: Some(current.stale_hint),
                        supporting_citations: current.supporting_citations.clone(),
                        canonical_entities: current.canonical_entities.clone(),
                        evidence_count: Some(current.evidence_count),
                        observed_at: Some(current.observed_at.saturating_add(1)),
                        source_revision: current
                            .source_revision
                            .map(|value| value.saturating_add(1)),
                    },
                },
                reason: format!("{actor_label} performs a neutral correction"),
                dry_run: false,
                mode_input: RuntimeLifecycleModeInput::default(),
            })
            .expect("neutral correction");
        let corrected = actor_runtime
            .get_long_term_memory(MemoryLongTermDetailRequest {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                view: MemoryLongTermControlView::HostUi,
            })
            .expect("read neutral correction")
            .record
            .expect("corrected active record");
        assert_eq!(corrected.last_confirmed_at, None, "{actor_label}");
    }

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
