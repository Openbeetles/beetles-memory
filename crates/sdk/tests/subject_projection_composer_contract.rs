#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::sync::{Arc, Mutex};

use bm_core::memory::{
    canonical_evidence_ref_from_source, governed_memory_recall_candidate_id, CanonicalEntityKey,
    CanonicalEntityKind, CanonicalEntityRef, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    LongTermControlOperation, LongTermMemoryControlRevision, SelfAuthoredCore,
    LONG_TERM_CONTROL_REVISION_NAMESPACE,
};
use bm_core::platform::Platform as _;
use bm_sdk::{
    default_agent_subject_id, LongTermMemoryDraft, LongTermMemoryKind, LongTermMemorySourceScope,
    MemoryAuditEvent, MemoryAuditSink, MemoryIdentity, MemoryLongTermControlView,
    MemoryLongTermDetailRequest, MemoryLongTermMutation, MemoryLongTermMutationRequest,
    MemoryLongTermTarget, MemoryPrivacyClass, MemoryPrivacyPolicy, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryRuntime, MemoryScope, MemoryStoreHandle,
    MemorySubjectVisibilityPolicy, MemoryWriteRequest, ParsedLongTermMemoryExtraction,
    PressureLevel, QueryFacetInput, RuntimeLifecycleModeInput, SubjectDescriptor, SubjectRegistry,
};

use support::empty_store_platform;

const SHARED_FACT: &str = "The shared release train is named Copper Finch.";
const FACET_ONLY_SENTINEL: &str = "PSV1_FACET_ONLY_PRIVATE_SENTINEL";
const SUPERSEDE_SENTINEL: &str = "PSV1_RESTRICTED_SUPERSEDE_SENTINEL";

#[derive(Clone, Default)]
struct RecordingAuditSink {
    events: Arc<Mutex<Vec<MemoryAuditEvent>>>,
}

impl MemoryAuditSink for RecordingAuditSink {
    fn record(&self, event: MemoryAuditEvent) {
        self.events.lock().expect("audit lock").push(event);
    }
}

impl RecordingAuditSink {
    fn events(&self) -> Vec<MemoryAuditEvent> {
        self.events.lock().expect("audit lock").clone()
    }
}

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
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-c"),
            "Agent C",
        ))
        .expect("agent-c subject");
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

fn runtime_for_subject_with_audit(
    platform: MemoryStoreHandle,
    registry: SubjectRegistry,
    agent_id: &str,
    audit_sink: Arc<dyn MemoryAuditSink>,
) -> MemoryRuntime {
    let mut privacy = MemoryPrivacyPolicy::standard_private_boundary();
    privacy.private_plane_projection_allowed = true;
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent_id, "owner-shared").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "shared-chat").expect("scope"))
        .store(platform)
        .subject_registry(registry)
        .privacy_policy(privacy)
        .audit_sink(audit_sink)
        .build()
        .expect("subject runtime with audit")
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

fn current_recall(runtime: &MemoryRuntime) -> bm_sdk::MemoryRecallReport {
    runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![QueryFacetInput::Keyword("release".to_string())],
            query: "What is the shared release train called?".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("current shared-fact recall")
}

fn assert_long_term_owner_exact_zero(
    recall: &bm_sdk::MemoryRecallReport,
    owner_id: &str,
    content_sentinel: &str,
    evidence_sentinel: &str,
) {
    let owner_ref = GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner_id);
    let candidate_id = governed_memory_recall_candidate_id(&owner_ref);
    for candidates in [
        &recall.source_candidate_ids,
        &recall.facet_index_report.exact_facet_candidate_ids,
        &recall.facet_index_report.expanded_facet_candidate_ids,
        &recall.coverage_selection_report.selected_candidate_ids,
        &recall.graph_rerank.candidate_ids,
        &recall.graph_rerank.expanded_candidate_ids,
        &recall.graph_rerank.reranked_candidate_ids,
        &recall.delivery_report.selected_candidate_ids,
    ] {
        assert!(!candidates.contains(&candidate_id), "{recall:#?}");
    }
    assert!(!recall
        .rank_fusion_report
        .candidate_reports
        .iter()
        .any(|candidate| candidate.candidate_id == candidate_id));
    assert!(!recall
        .graph_candidate_evidence_ref_index
        .iter()
        .any(|entry| entry.candidate_id == candidate_id));
    assert!(!recall
        .compact_graph
        .nodes
        .iter()
        .any(|node| node.node_id == candidate_id));
    let safe_report = format!("{recall:#?}");
    assert!(!safe_report.contains(content_sentinel), "{safe_report}");
    assert!(!safe_report.contains(evidence_sentinel), "{safe_report}");
    assert!(!safe_report.contains(owner_id), "{safe_report}");
    assert!(!safe_report.contains(&candidate_id), "{safe_report}");
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
    let owner_id = shared_records[0].id.clone();
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

    runtime_a
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                source_scope: LongTermMemorySourceScope::World,
                subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![
                    subject_a.clone()
                ]),
            },
            reason: "psv1 exact subject visibility".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("restrict shared owner to agent-a");

    assert!(project(&runtime_a)
        .report()
        .shared_fact_projection()
        .contains(SHARED_FACT));
    let denied_projection = project(&runtime_b);
    assert!(!denied_projection
        .report()
        .shared_fact_projection()
        .contains(SHARED_FACT));
    assert!(!denied_projection
        .provider_payload()
        .system_memory_block()
        .contains(SHARED_FACT));

    let denied_recall = current_recall(&runtime_b);
    let owner_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner_id.clone());
    let candidate_id = governed_memory_recall_candidate_id(&owner_ref);
    assert!(!denied_recall
        .working
        .long_term_memory_text
        .as_deref()
        .is_some_and(|text| text.contains(SHARED_FACT)));
    assert!(!denied_recall
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| capsule.content.contains(SHARED_FACT)));
    for candidates in [
        &denied_recall.source_candidate_ids,
        &denied_recall.facet_index_report.exact_facet_candidate_ids,
        &denied_recall
            .facet_index_report
            .expanded_facet_candidate_ids,
        &denied_recall.graph_rerank.reranked_candidate_ids,
        &denied_recall.delivery_report.selected_candidate_ids,
    ] {
        assert!(!candidates.contains(&candidate_id));
    }
    assert_long_term_owner_exact_zero(&denied_recall, &owner_id, SHARED_FACT, "shared-chat:turn-1");
    let denied_detail = runtime_b
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(owner_id.clone()),
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("known-id detail must fail closed for denied subject");
    assert!(denied_detail.record.is_none());
    assert!(denied_detail.revisions.is_empty(), "{denied_detail:#?}");
    assert!(denied_detail.tombstone.is_none(), "{denied_detail:#?}");
    assert!(
        denied_detail.transcript_refs.is_empty(),
        "{denied_detail:#?}"
    );
    let safe_detail = format!("{denied_detail:#?}");
    assert!(!safe_detail.contains(&owner_id), "{safe_detail}");
    assert!(!safe_detail.contains("shared-chat:turn-1"), "{safe_detail}");

    let only_transition = platform
        .replay_harness()
        .read_json_namespace(LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .expect("read visibility control revisions")
        .into_iter()
        .map(|document| {
            serde_json::from_value::<LongTermMemoryControlRevision>(document.value)
                .expect("typed visibility control revision")
        })
        .find(|revision| {
            revision.operation == LongTermControlOperation::ChangeScope
                && revision.transition.predecessor.owner_ref == owner_ref
        })
        .expect("OnlySubjects transition");
    let predecessor_as_of = only_transition
        .transition
        .terminated_at
        .checked_sub(1)
        .expect("positive predecessor interval");
    let historical_for_b = runtime_b
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: predecessor_as_of,
            },
            structured_query_facets: vec![QueryFacetInput::Keyword("release".to_string())],
            query: "What is the shared release train called?".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("agent-b historical recall uses revision-1 policy");
    assert!(historical_for_b
        .delivery_report
        .selected_candidate_ids
        .contains(&candidate_id));
    assert!(historical_for_b
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| capsule.content.contains(SHARED_FACT)));

    runtime_a
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                source_scope: LongTermMemorySourceScope::World,
                subject_visibility: MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec![
                    subject_b.clone(),
                ]),
            },
            reason: "psv1 hidden subject visibility".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("hide shared owner from agent-b");
    assert!(project(&runtime_a)
        .report()
        .shared_fact_projection()
        .contains(SHARED_FACT));
    assert!(!project(&runtime_b)
        .report()
        .shared_fact_projection()
        .contains(SHARED_FACT));
    let hidden_denied = current_recall(&runtime_b);
    assert_long_term_owner_exact_zero(&hidden_denied, &owner_id, SHARED_FACT, "shared-chat:turn-1");
    let runtime_c = runtime_for_subject(platform.clone(), two_agent_registry(), "agent-c");
    assert!(
        project(&runtime_c)
            .report()
            .shared_fact_projection()
            .contains(SHARED_FACT),
        "HiddenFromSubjects([b]) must remain visible to distinct subject c"
    );

    let only_successor_transition = platform
        .replay_harness()
        .read_json_namespace(LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .expect("read OnlySubjects successor transition")
        .into_iter()
        .map(|document| {
            serde_json::from_value::<LongTermMemoryControlRevision>(document.value)
                .expect("typed visibility transition")
        })
        .find(|revision| {
            revision.operation == LongTermControlOperation::ChangeScope
                && revision.transition.predecessor.owner_ref == owner_ref
                && revision.transition.predecessor.owner_revision == 2
        })
        .expect("revision-2 OnlySubjects transition");
    let only_as_of = only_successor_transition
        .transition
        .terminated_at
        .checked_sub(1)
        .expect("positive OnlySubjects interval");
    let historical_only_for_b = runtime_b
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: only_as_of,
            },
            structured_query_facets: vec![QueryFacetInput::Keyword("release".to_string())],
            query: "What is the shared release train called?".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("agent-b historical revision-2 recall");
    assert_long_term_owner_exact_zero(
        &historical_only_for_b,
        &owner_id,
        SHARED_FACT,
        "shared-chat:turn-1",
    );

    let registry_without_b =
        SubjectRegistry::single_agent_default("owner-shared", "agent-a").expect("registry-a");
    let runtime_with_unknown_persisted_policy_subject =
        runtime_for_subject(platform.clone(), registry_without_b, "agent-a");
    let unknown_persisted_subject_error = runtime_with_unknown_persisted_policy_subject
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release".to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .expect_err("persisted policy subject absent from reopened registry must fail closed");
    assert_eq!(
        unknown_persisted_subject_error.stage(),
        "long_term_subject_visibility"
    );

    let unknown_subject_error = runtime_a
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(shared_records[0].id.clone()),
                source_scope: LongTermMemorySourceScope::World,
                subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![
                    "agent:missing".to_string(),
                ]),
            },
            reason: "unknown subject must fail closed".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect_err("policy subject must exist in SubjectRegistry");
    assert_eq!(
        unknown_subject_error.stage(),
        "long_term_subject_visibility"
    );
    assert!(!project(&runtime_b)
        .report()
        .shared_fact_projection()
        .contains(SHARED_FACT));

    runtime_a
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                source_scope: LongTermMemorySourceScope::World,
                subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            },
            reason: "restore current visibility without rewriting history".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("restore AllSubjects");
    assert!(project(&runtime_b)
        .report()
        .shared_fact_projection()
        .contains(SHARED_FACT));
    let historical_only_after_current_all = runtime_b
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: only_as_of,
            },
            structured_query_facets: vec![QueryFacetInput::Keyword("release".to_string())],
            query: "What is the shared release train called?".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("revision-2 policy remains exact after current AllSubjects");
    assert_long_term_owner_exact_zero(
        &historical_only_after_current_all,
        &owner_id,
        SHARED_FACT,
        "shared-chat:turn-1",
    );
}

#[test]
fn restricted_supersede_inherits_policy_and_never_opens_the_successor_to_other_subjects() {
    let platform = empty_store_platform(support::host_test_profile());
    let registry = two_agent_registry();
    let subject_a = default_agent_subject_id("agent-a");
    let runtime_a = runtime_for_subject(platform.clone(), registry.clone(), "agent-a");
    let runtime_b = runtime_for_subject(platform.clone(), registry, "agent-b");
    runtime_a
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Fact,
                    topic: "restricted_supersede_predecessor".to_string(),
                    content: "Restricted release predecessor.".to_string(),
                    keywords: vec!["release".to_string()],
                    privacy: MemoryPrivacyClass::PublicRuntime,
                    source_chat_id: Some("shared-chat".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["psv1:supersede-private-evidence".to_string()],
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
        .expect("seed restricted predecessor");
    let predecessor = platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-shared")
        .expect("shared store")
        .list(8)
        .expect("shared owners")
        .into_iter()
        .find(|entry| entry.topic == "restricted_supersede_predecessor")
        .expect("restricted predecessor");
    runtime_a
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(predecessor.id.clone()),
                source_scope: predecessor.source_scope,
                subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![subject_a]),
            },
            reason: "restrict predecessor before supersede".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("restrict predecessor");
    let replacement = LongTermMemoryDraft {
        kind: LongTermMemoryKind::Fact,
        topic: "restricted_supersede_successor".to_string(),
        content: SUPERSEDE_SENTINEL.to_string(),
        keywords: vec!["release".to_string()],
        privacy: MemoryPrivacyClass::PublicRuntime,
        source_chat_id: Some("shared-chat".to_string()),
        source_type: None,
        source_scope: None,
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: vec!["psv1:supersede-private-evidence".to_string()],
        canonical_entities: Vec::new(),
        evidence_count: Some(1),
        observed_at: Some(1_800_000_001),
        last_confirmed_at: Some(1_800_000_001),
        source_revision: Some(2),
    };
    let successor_id = replacement.stable_id().expect("successor id");
    let supersede = runtime_a
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Supersede {
                target: MemoryLongTermTarget::RecordId(predecessor.id.clone()),
                replacement,
            },
            reason: "supersede must retain the exact visibility policy".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("supersede restricted owner");
    assert!(supersede.accepted, "{supersede:#?}");
    assert_eq!(
        supersede.projection_impact.subject_visibility,
        MemorySubjectVisibilityPolicy::OnlySubjects(vec![default_agent_subject_id("agent-a")])
    );
    let successor = platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-shared")
        .expect("shared store")
        .get(&successor_id)
        .expect("successor read")
        .expect("successor owner");
    assert_eq!(successor.owner_revision, 1);
    assert_eq!(
        successor.subject_visibility,
        MemorySubjectVisibilityPolicy::OnlySubjects(vec![default_agent_subject_id("agent-a")])
    );
    assert!(current_recall(&runtime_a)
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| capsule.content.contains(SUPERSEDE_SENTINEL)));
    assert_long_term_owner_exact_zero(
        &current_recall(&runtime_b),
        &successor_id,
        SUPERSEDE_SENTINEL,
        "psv1:supersede-private-evidence",
    );
    let denied_predecessor_detail = runtime_b
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(predecessor.id.clone()),
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("restricted terminal predecessor detail must fail closed");
    assert!(denied_predecessor_detail.record.is_none());
    assert!(denied_predecessor_detail.revisions.is_empty());
    assert!(denied_predecessor_detail.tombstone.is_none());
    assert!(denied_predecessor_detail.transcript_refs.is_empty());
    let safe_detail = format!("{denied_predecessor_detail:#?}");
    assert!(!safe_detail.contains(&predecessor.id), "{safe_detail}");
    assert!(!safe_detail.contains("psv1:supersede-private-evidence"));
    let allowed_predecessor_detail = runtime_a
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(predecessor.id.clone()),
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("authorized subject retains terminal governance detail");
    assert!(allowed_predecessor_detail.record.is_none());
    assert!(!allowed_predecessor_detail.revisions.is_empty());
    assert_eq!(
        allowed_predecessor_detail
            .tombstone
            .as_ref()
            .expect("authorized terminal tombstone")
            .subject_visibility,
        MemorySubjectVisibilityPolicy::OnlySubjects(vec![default_agent_subject_id("agent-a")])
    );
}

#[test]
fn restricted_subject_is_exact_zero_for_facet_only_hit_and_audit_is_safe() {
    let platform = empty_store_platform(support::host_test_profile());
    let registry = two_agent_registry();
    let subject_a = default_agent_subject_id("agent-a");
    let audit_sink = RecordingAuditSink::default();
    let runtime_a = runtime_for_subject(platform.clone(), registry.clone(), "agent-a");
    let runtime_b = runtime_for_subject_with_audit(
        platform.clone(),
        registry,
        "agent-b",
        Arc::new(audit_sink.clone()),
    );
    let source_ref = "turn:psv1-facet-only-private-evidence";
    let entity_key = CanonicalEntityKey {
        kind: CanonicalEntityKind::Repository,
        canonical_id: "psv1-facet-owner".to_string(),
    };
    runtime_a
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Fact,
                    topic: "opaque_navigation_note".to_string(),
                    content: format!("{FACET_ONLY_SENTINEL} remains governed."),
                    keywords: vec!["opaque".to_string(), "navigation".to_string()],
                    privacy: MemoryPrivacyClass::PublicRuntime,
                    source_chat_id: Some("shared-chat".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec![source_ref.to_string()],
                    canonical_entities: vec![CanonicalEntityRef {
                        key: entity_key.clone(),
                        display_label: Some("PSV1 facet owner".to_string()),
                        aliases: Vec::new(),
                        evidence_refs: vec![canonical_evidence_ref_from_source(source_ref)
                            .expect("canonical evidence")],
                    }],
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
        .expect("write facet-only owner");
    let owner = platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-shared")
        .expect("shared store")
        .list(8)
        .expect("shared owners")
        .into_iter()
        .find(|entry| entry.topic == "opaque_navigation_note")
        .expect("facet-only owner");
    let owner_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner.id.clone());
    let candidate_id = governed_memory_recall_candidate_id(&owner_ref);
    let lexical_query = "meteorological disjunction";
    let allowed = runtime_a
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![QueryFacetInput::Entity(entity_key.clone())],
            query: lexical_query.to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .expect("authorized facet-only recall");
    assert_eq!(
        allowed.facet_index_report.exact_facet_candidate_ids,
        vec![candidate_id.clone()]
    );

    runtime_a
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(owner.id.clone()),
                source_scope: owner.source_scope,
                subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![subject_a]),
            },
            reason: "facet-only visibility must fail closed".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("restrict facet-only owner");

    let denied = runtime_b
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![QueryFacetInput::Entity(entity_key)],
            query: lexical_query.to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .expect("denied facet-only recall");
    for candidates in [
        &denied.source_candidate_ids,
        &denied.facet_index_report.exact_facet_candidate_ids,
        &denied.facet_index_report.expanded_facet_candidate_ids,
        &denied.coverage_selection_report.selected_candidate_ids,
        &denied.graph_rerank.candidate_ids,
        &denied.graph_rerank.expanded_candidate_ids,
        &denied.graph_rerank.reranked_candidate_ids,
        &denied.delivery_report.selected_candidate_ids,
    ] {
        assert!(!candidates.contains(&candidate_id), "{denied:#?}");
    }
    assert!(
        denied.facet_index_report.manifest_integrity_verified,
        "visibility denial is not facet corruption: {denied:#?}"
    );
    assert!(!denied
        .facet_index_report
        .failures
        .iter()
        .any(|failure| failure == "memory_facet_owner_record_unavailable"));
    assert!(!denied
        .rank_fusion_report
        .candidate_reports
        .iter()
        .any(|candidate| candidate.candidate_id == candidate_id));
    assert!(!format!("{denied:#?}").contains(FACET_ONLY_SENTINEL));
    assert!(!format!("{denied:#?}").contains(source_ref));

    let visibility_audits = audit_sink
        .events()
        .into_iter()
        .filter(|event| event.operation == "memory_recall.subject_visibility")
        .collect::<Vec<_>>();
    assert!(!visibility_audits.is_empty());
    for event in visibility_audits {
        assert!(!event.allowed);
        assert_eq!(event.reason, "subject_visibility_blocked");
        let safe_event = format!("{event:#?}");
        for secret in [
            FACET_ONLY_SENTINEL,
            source_ref,
            owner.id.as_str(),
            candidate_id.as_str(),
        ] {
            assert!(!safe_event.contains(secret), "unsafe audit: {safe_event}");
        }
    }
}
