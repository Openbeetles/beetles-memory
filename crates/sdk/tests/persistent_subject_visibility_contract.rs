#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::memory::{
    default_agent_subject_id, governed_memory_recall_candidate_id, GovernedMemoryOwnerPlane,
    GovernedMemoryOwnerRef, LongTermMemoryQuery, SubjectDescriptor, SubjectRegistry,
    LONG_TERM_CONTROL_TOMBSTONE_SCHEMA_VERSION,
};
use bm_sdk::{
    LongTermMemoryDraft, LongTermMemoryKind, LongTermMemorySourceScope, MemoryIdentity,
    MemoryLongTermControlView, MemoryLongTermDetailRequest, MemoryLongTermListRequest,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermTarget,
    MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemoryStoreHandle, MemorySubjectVisibilityPolicy, MemoryWriteRequest,
    ParsedLongTermMemoryExtraction, PressureLevel, QueryFacetInput, RuntimeLifecycleModeInput,
    StoreBackendConfig,
};

const CONTENT: &str = "PSV1_REOPEN_SENTINEL belongs to one MemorySpace owner.";
const PRIVATE_EVIDENCE: &str = "psv1:private-evidence-sentinel";

fn registry() -> SubjectRegistry {
    let mut registry =
        SubjectRegistry::single_agent_default("owner-psv1", "agent-a").expect("registry");
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

fn runtime(
    platform: MemoryStoreHandle,
    registry: SubjectRegistry,
    agent_id: &str,
) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent_id, "owner-psv1").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "psv1-reopen").expect("scope"))
        .store(platform)
        .subject_registry(registry)
        .build()
        .expect("runtime")
}

fn current_recall(runtime: &MemoryRuntime) -> bm_sdk::MemoryRecallReport {
    runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![QueryFacetInput::Keyword("psv1".to_string())],
            query: "PSV1_REOPEN_SENTINEL".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("current recall")
}

fn assert_owner_exact_zero(recall: &bm_sdk::MemoryRecallReport, owner_id: &str) {
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
    let safe_report = format!("{recall:#?}");
    assert!(!safe_report.contains(CONTENT), "{safe_report}");
    assert!(!safe_report.contains(PRIVATE_EVIDENCE), "{safe_report}");
    assert!(!safe_report.contains(owner_id), "{safe_report}");
    assert!(!safe_report.contains(&candidate_id), "{safe_report}");
}

fn assert_subject_visibility(open: impl Fn() -> MemoryStoreHandle) {
    let subject_a = default_agent_subject_id("agent-a");
    let subject_b = default_agent_subject_id("agent-b");
    let owner_id = {
        let platform = open();
        let runtime_a = runtime(platform, registry(), "agent-a");
        runtime_a
            .write(MemoryWriteRequest::LongTermExtraction {
                extraction: ParsedLongTermMemoryExtraction {
                    upserts: vec![LongTermMemoryDraft {
                        kind: LongTermMemoryKind::Fact,
                        topic: "psv1_reopen".to_string(),
                        content: CONTENT.to_string(),
                        keywords: vec!["psv1".to_string()],
                        privacy: MemoryPrivacyClass::PublicRuntime,
                        source_chat_id: Some("psv1-reopen".to_string()),
                        source_type: None,
                        source_scope: None,
                        confidence: None,
                        freshness: None,
                        stale_hint: None,
                        supporting_citations: vec![PRIVATE_EVIDENCE.to_string()],
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
            .expect("seed owner");
        let owner_id = runtime_a
            .list_long_term_memory(MemoryLongTermListRequest {
                query: LongTermMemoryQuery::default(),
                cursor: None,
                limit: 10,
                view: MemoryLongTermControlView::HostUi,
            })
            .expect("list owner")
            .records[0]
            .record
            .id
            .clone();
        runtime_a
            .mutate_long_term_memory(MemoryLongTermMutationRequest {
                operation: MemoryLongTermMutation::ChangeScope {
                    target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                    source_scope: LongTermMemorySourceScope::World,
                    subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![
                        subject_a.clone(),
                    ]),
                },
                reason: "persist OnlySubjects across reopen".to_string(),
                dry_run: false,
                mode_input: RuntimeLifecycleModeInput::default(),
            })
            .expect("persist OnlySubjects");
        owner_id
    };

    {
        let platform = open();
        let runtime_a = runtime(platform.clone(), registry(), "agent-a");
        let runtime_b = runtime(platform.clone(), registry(), "agent-b");
        let runtime_c = runtime(platform, registry(), "agent-c");
        assert!(current_recall(&runtime_a)
            .delivery_report
            .rendered_capsules
            .iter()
            .any(|capsule| capsule.content.contains(CONTENT)));
        let denied = current_recall(&runtime_b);
        assert_owner_exact_zero(&denied, &owner_id);
        assert_owner_exact_zero(&current_recall(&runtime_c), &owner_id);
        runtime_a
            .mutate_long_term_memory(MemoryLongTermMutationRequest {
                operation: MemoryLongTermMutation::ChangeScope {
                    target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                    source_scope: LongTermMemorySourceScope::World,
                    subject_visibility: MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec![
                        subject_b.clone(),
                    ]),
                },
                reason: "persist HiddenFromSubjects across reopen".to_string(),
                dry_run: false,
                mode_input: RuntimeLifecycleModeInput::default(),
            })
            .expect("persist HiddenFromSubjects");
    }

    let platform = open();
    let runtime_a = runtime(platform.clone(), registry(), "agent-a");
    let runtime_b = runtime(platform.clone(), registry(), "agent-b");
    let runtime_c = runtime(platform, registry(), "agent-c");
    assert!(runtime_a
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![QueryFacetInput::Keyword("psv1".to_string())],
            user_query: "PSV1_REOPEN_SENTINEL".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("authorized projection")
        .provider_payload()
        .system_memory_block()
        .contains(CONTENT));
    let denied = current_recall(&runtime_b);
    assert_owner_exact_zero(&denied, &owner_id);
    let denied_projection = runtime_b
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: vec![QueryFacetInput::Keyword("psv1".to_string())],
            user_query: "PSV1_REOPEN_SENTINEL".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("denied projection");
    let denied_provider_block = denied_projection.provider_payload().system_memory_block();
    assert!(!denied_provider_block.contains(CONTENT));
    assert!(!denied_provider_block.contains(PRIVATE_EVIDENCE));
    assert!(current_recall(&runtime_c)
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| capsule.content.contains(CONTENT)));
    let persisted = runtime_a
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list persisted hidden policy")
        .records
        .into_iter()
        .find(|record| record.record.id == owner_id)
        .expect("persisted hidden owner")
        .record;
    assert_eq!(
        persisted.subject_visibility,
        MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec![subject_b.clone()])
    );
    runtime_a
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
            },
            reason: "persist restricted terminal visibility across reopen".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete restricted owner");
    drop(runtime_a);
    drop(runtime_b);
    drop(runtime_c);

    let reopened = open();
    let reopened_a = runtime(reopened.clone(), registry(), "agent-a");
    let reopened_b = runtime(reopened.clone(), registry(), "agent-b");
    let reopened_c = runtime(reopened, registry(), "agent-c");
    for allowed in [&reopened_a, &reopened_c] {
        let detail = allowed
            .get_long_term_memory(MemoryLongTermDetailRequest {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                view: MemoryLongTermControlView::HostUi,
            })
            .expect("authorized terminal detail survives reopen");
        assert!(detail.record.is_none());
        assert!(!detail.revisions.is_empty());
        let tombstone = detail.tombstone.expect("authorized typed tombstone");
        assert_eq!(
            tombstone.schema_version,
            LONG_TERM_CONTROL_TOMBSTONE_SCHEMA_VERSION
        );
        assert_eq!(
            tombstone.subject_visibility,
            MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec![subject_b.clone()])
        );
    }
    let denied_detail = reopened_b
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(owner_id.clone()),
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("denied terminal detail fails closed after reopen");
    assert!(denied_detail.record.is_none());
    assert!(denied_detail.revisions.is_empty());
    assert!(denied_detail.tombstone.is_none());
    assert!(denied_detail.transcript_refs.is_empty());
    let safe_detail = format!("{denied_detail:#?}");
    assert!(!safe_detail.contains(&owner_id), "{safe_detail}");
    assert!(!safe_detail.contains(CONTENT), "{safe_detail}");
    assert!(!safe_detail.contains(PRIVATE_EVIDENCE), "{safe_detail}");
}

fn temp_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "bm-psv1-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

#[test]
fn in_memory_runtime_reopen_preserves_subject_visibility() {
    let platform = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).expect("config"),
    )
    .expect("in-memory store");
    assert_subject_visibility(|| platform.clone());
}

#[test]
fn file_runtime_reopen_preserves_subject_visibility() {
    let root = temp_path("file");
    assert_subject_visibility(|| {
        support::open_memory_store(
            StoreBackendConfig::file(&root, support::host_test_profile()).expect("config"),
        )
        .expect("file store")
    });
}

#[test]
#[cfg(feature = "sqlite-store")]
fn sqlite_runtime_reopen_preserves_subject_visibility() {
    let path = temp_path("sqlite").with_extension("sqlite3");
    assert_subject_visibility(|| {
        support::open_memory_store(
            StoreBackendConfig::sqlite(&path, support::host_test_profile()).expect("config"),
        )
        .expect("sqlite store")
    });
}
