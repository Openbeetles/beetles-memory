#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::memory::{
    default_agent_subject_id, governed_memory_recall_candidate_id, GovernedMemoryOwnerPlane,
    GovernedMemoryOwnerRef, LongTermMemoryProvenance, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryEvidenceAuthority, MemorySemanticJudgmentSource, MemoryWriteCandidate, SubjectDescriptor,
    SubjectRegistry, LONG_TERM_CONTROL_TOMBSTONE_SCHEMA_VERSION,
};
use bm_sdk::{
    LongTermMemoryDraft, LongTermMemoryKind, LongTermMemorySourceScope,
    LongTermMemoryVersionMaterial, MemoryIdentity, MemoryLongTermControlView,
    MemoryLongTermDetailRequest, MemoryLongTermMutation, MemoryLongTermMutationRequest,
    MemoryLongTermTarget, MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryRuntime, MemoryScope, MemoryStoreHandle, MemorySubjectVisibilityPolicy,
    MemoryWriteRequest, ParsedLongTermMemoryExtraction, PressureLevel, QueryFacetInput,
    RuntimeLifecycleModeInput, StoreBackendConfig,
};

const CONTENT: &str = "PSV1_REOPEN_SENTINEL belongs to one MemorySpace owner.";
const PRIVATE_EVIDENCE: &str = "psv1:private-evidence-sentinel";
const HIDDEN_CONTENT: &str = "PSV1_HIDDEN_REOPEN_SENTINEL is hidden from one subject at creation.";
const HIDDEN_PRIVATE_EVIDENCE: &str = "psv2:hidden-private-evidence-sentinel";

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

fn assert_owner_exact_zero(
    recall: &bm_sdk::MemoryRecallReport,
    owner_id: &str,
    forbidden_content: &str,
    forbidden_evidence: &str,
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
    let safe_report = format!("{recall:#?}");
    assert!(!safe_report.contains(forbidden_content), "{safe_report}");
    assert!(!safe_report.contains(forbidden_evidence), "{safe_report}");
    assert!(!safe_report.contains(owner_id), "{safe_report}");
    assert!(!safe_report.contains(&candidate_id), "{safe_report}");
}

fn assert_subject_visibility(open: impl Fn() -> MemoryStoreHandle) {
    let subject_a = default_agent_subject_id("agent-a");
    let subject_b = default_agent_subject_id("agent-b");
    let (owner_id, hidden_owner_id) = {
        let platform = open();
        let runtime_a = runtime(platform.clone(), registry(), "agent-a");
        runtime_a
            .write(MemoryWriteRequest::LongTermExtraction {
                extraction: ParsedLongTermMemoryExtraction {
                    upserts: vec![
                        LongTermMemoryDraft {
                            kind: LongTermMemoryKind::Fact,
                            topic: "psv1_reopen".to_string(),
                            content: CONTENT.to_string(),
                            keywords: vec!["psv1".to_string()],
                            privacy: MemoryPrivacyClass::PublicRuntime,
                            source_chat_id: Some("psv1-reopen".to_string()),
                            source_type: None,
                            source_scope: None,
                            subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![
                                subject_a.clone(),
                            ]),
                            provenance: LongTermMemoryProvenance {
                                source_authority: MemoryEvidenceAuthority::UserAsserted,
                                semantic_judgment_source: Some(
                                    MemorySemanticJudgmentSource::RuntimeGate,
                                ),
                            },
                            confidence: None,
                            freshness: None,
                            stale_hint: None,
                            supporting_citations: vec![PRIVATE_EVIDENCE.to_string()],
                            canonical_entities: Vec::new(),
                            evidence_count: Some(1),
                            observed_at: Some(1_800_000_000),
                            source_revision: Some(1),
                        },
                        LongTermMemoryDraft {
                            kind: LongTermMemoryKind::Fact,
                            topic: "psv2_hidden_reopen".to_string(),
                            content: HIDDEN_CONTENT.to_string(),
                            keywords: vec!["psv1".to_string(), "hidden".to_string()],
                            privacy: MemoryPrivacyClass::PublicRuntime,
                            source_chat_id: Some("psv1-reopen".to_string()),
                            source_type: None,
                            source_scope: None,
                            subject_visibility: MemorySubjectVisibilityPolicy::HiddenFromSubjects(
                                vec![subject_b.clone()],
                            ),
                            provenance: LongTermMemoryProvenance {
                                source_authority: MemoryEvidenceAuthority::UserAsserted,
                                semantic_judgment_source: Some(
                                    MemorySemanticJudgmentSource::RuntimeGate,
                                ),
                            },
                            confidence: None,
                            freshness: None,
                            stale_hint: None,
                            supporting_citations: vec![HIDDEN_PRIVATE_EVIDENCE.to_string()],
                            canonical_entities: Vec::new(),
                            evidence_count: Some(1),
                            observed_at: Some(1_800_000_001),
                            source_revision: Some(1),
                        },
                    ],
                    deletes: Vec::new(),
                    skill_writes: Vec::new(),
                },
                governed_skill_writes: Vec::new(),
                runtime_skill_owning_scope: None,
            })
            .expect("seed owner");
        let materials = platform
            .replay_harness()
            .read_json_namespace("long_term_version_materials")
            .expect("read rev1 materials");
        assert_eq!(
            materials.len(),
            2,
            "both restricted creates must have exactly one revision"
        );
        let materials = materials
            .into_iter()
            .map(|doc| {
                serde_json::from_value::<LongTermMemoryVersionMaterial>(doc.value)
                    .expect("decode rev1 material")
            })
            .collect::<Vec<_>>();
        let only_material = materials
            .iter()
            .find(|material| material.governed_content.topic == "psv1_reopen")
            .expect("OnlySubjects rev1 material");
        assert_eq!(only_material.owner_revision, 1);
        assert_eq!(
            only_material.subject_visibility,
            MemorySubjectVisibilityPolicy::OnlySubjects(vec![subject_a.clone()])
        );
        let hidden_material = materials
            .iter()
            .find(|material| material.governed_content.topic == "psv2_hidden_reopen")
            .expect("HiddenFromSubjects rev1 material");
        assert_eq!(hidden_material.owner_revision, 1);
        assert_eq!(
            hidden_material.subject_visibility,
            MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec![subject_b.clone()])
        );
        let owner_id = only_material.owner_ref.owner_id.clone();
        let hidden_owner_id = hidden_material.owner_ref.owner_id.clone();
        runtime_a
            .mutate_long_term_memory(MemoryLongTermMutationRequest {
                operation: MemoryLongTermMutation::Correct {
                    target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                    replacement: LongTermMemoryDraft {
                        kind: LongTermMemoryKind::Fact,
                        topic: "psv1_reopen".to_string(),
                        content: CONTENT.to_string(),
                        keywords: vec!["psv1".to_string()],
                        privacy: MemoryPrivacyClass::PublicRuntime,
                        source_chat_id: Some("psv1-reopen".to_string()),
                        source_type: None,
                        source_scope: None,
                        subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![
                            subject_a.clone(),
                        ]),
                        provenance: LongTermMemoryProvenance {
                            source_authority: MemoryEvidenceAuthority::UserAsserted,
                            semantic_judgment_source: Some(
                                MemorySemanticJudgmentSource::RuntimeGate,
                            ),
                        },
                        confidence: None,
                        freshness: None,
                        stale_hint: None,
                        supporting_citations: vec![PRIVATE_EVIDENCE.to_string()],
                        canonical_entities: Vec::new(),
                        evidence_count: Some(1),
                        observed_at: Some(1_800_000_002),
                        source_revision: Some(1),
                    },
                },
                reason: "persist typed Correct confirmation across reopen".to_string(),
                dry_run: false,
                mode_input: RuntimeLifecycleModeInput::default(),
            })
            .expect("persist typed correction confirmation");
        let corrected = platform
            .replay_harness()
            .read_json_namespace("long_term_version_materials")
            .expect("read corrected materials")
            .into_iter()
            .map(|doc| {
                serde_json::from_value::<LongTermMemoryVersionMaterial>(doc.value)
                    .expect("decode corrected material")
            })
            .filter(|material| material.owner_ref.owner_id == owner_id)
            .max_by_key(|material| material.owner_revision)
            .expect("corrected owner material");
        let correction = corrected
            .governed_content
            .correction_evidence
            .as_ref()
            .expect("typed neutral correction evidence");
        assert_eq!(correction.memory_space_id, runtime_a.memory_space_id());
        assert_eq!(correction.actor_subject_id, subject_a);
        assert_eq!(correction.successor.owner_revision, 2);
        assert!(corrected.governed_content.confirmation_evidence.is_none());
        (owner_id, hidden_owner_id)
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
        assert!(current_recall(&runtime_a)
            .delivery_report
            .rendered_capsules
            .iter()
            .any(|capsule| capsule.content.contains(HIDDEN_CONTENT)));
        let denied = current_recall(&runtime_b);
        assert_owner_exact_zero(&denied, &owner_id, CONTENT, PRIVATE_EVIDENCE);
        assert_owner_exact_zero(
            &denied,
            &hidden_owner_id,
            HIDDEN_CONTENT,
            HIDDEN_PRIVATE_EVIDENCE,
        );
        let subject_c_recall = current_recall(&runtime_c);
        assert_owner_exact_zero(&subject_c_recall, &owner_id, CONTENT, PRIVATE_EVIDENCE);
        assert!(subject_c_recall
            .delivery_report
            .rendered_capsules
            .iter()
            .any(|capsule| capsule.content.contains(HIDDEN_CONTENT)));
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
    let runtime_c = runtime(platform.clone(), registry(), "agent-c");
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
    assert_owner_exact_zero(&denied, &owner_id, CONTENT, PRIVATE_EVIDENCE);
    assert_owner_exact_zero(
        &denied,
        &hidden_owner_id,
        HIDDEN_CONTENT,
        HIDDEN_PRIVATE_EVIDENCE,
    );
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
    let persisted = platform
        .replay_harness()
        .read_json_namespace("long_term_version_materials")
        .expect("read persisted hidden materials")
        .into_iter()
        .map(|doc| {
            serde_json::from_value::<LongTermMemoryVersionMaterial>(doc.value)
                .expect("decode persisted hidden material")
        })
        .filter(|material| material.owner_ref.owner_id == owner_id)
        .max_by_key(|material| material.owner_revision)
        .expect("persisted hidden owner material");
    assert_eq!(
        persisted.subject_visibility,
        MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec![subject_b.clone()])
    );
    assert!(persisted.governed_content.correction_evidence.is_some());
    assert!(persisted.governed_content.confirmation_evidence.is_none());
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
fn initial_visibility_unknown_subject_rejects_before_any_write() {
    let platform = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).expect("config"),
    )
    .expect("in-memory store");
    let runtime_a = runtime(platform.clone(), registry(), "agent-a");
    let before = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("before snapshot");

    let error = runtime_a
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Fact,
                    topic: "unknown_subject_create".to_string(),
                    content: "UNKNOWN_SUBJECT_CREATE_SENTINEL".to_string(),
                    keywords: vec!["unknown".to_string()],
                    privacy: MemoryPrivacyClass::PublicRuntime,
                    source_chat_id: Some("psv1-reopen".to_string()),
                    source_type: None,
                    source_scope: None,
                    subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![
                        "agent:missing".to_string(),
                    ]),
                    provenance: LongTermMemoryProvenance {
                        source_authority: MemoryEvidenceAuthority::UserAsserted,
                        semantic_judgment_source: Some(MemorySemanticJudgmentSource::RuntimeGate),
                    },
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["psv2:unknown-subject-membership".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_001),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
        })
        .expect_err("unknown visibility subject must fail before planning or commit");

    assert_eq!(error.stage(), "long_term_subject_visibility");
    let after = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("after snapshot");
    assert_eq!(after.json_docs, before.json_docs);
    assert_eq!(after.events, before.events);
}

#[test]
fn candidate_visibility_unknown_subject_rejects_before_any_write() {
    let platform = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).expect("config"),
    )
    .expect("in-memory store");
    let runtime_a = runtime(platform.clone(), registry(), "agent-a");
    let before = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("before snapshot");
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Fact,
        topic: "candidate_unknown_subject".to_string(),
    };

    let error = runtime_a
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: None,
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-unknown-subject".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: target.clone(),
                long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::OnlySubjects(
                    vec!["agent:missing".to_string()],
                )),
                privacy: MemoryPrivacyClass::PublicRuntime,
                content: MemoryCandidateContent::Text {
                    topic: "candidate_unknown_subject".to_string(),
                    body: "CANDIDATE_UNKNOWN_SUBJECT_SENTINEL".to_string(),
                    keywords: vec!["unknown".to_string()],
                },
                evidence_refs: vec!["psv2:candidate-unknown-subject".to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                    source: MemorySemanticJudgmentSource::RuntimeGate,
                    decision: MemoryCandidateSemanticDecision::Accept,
                    governed_target: Some(target),
                    reason: "trusted runtime gate".to_string(),
                }),
            }],
        })
        .expect_err("unknown candidate visibility subject must fail before commit");

    assert_eq!(error.stage(), "long_term_subject_visibility");
    let after = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("after snapshot");
    assert_eq!(after.json_docs, before.json_docs);
    assert_eq!(after.events, before.events);
}

#[test]
fn candidate_initial_only_subjects_is_revision_one_and_plain_update_cannot_change_policy() {
    const CANDIDATE_CONTENT: &str =
        "PSV1_CANDIDATE_ONLY_SENTINEL is private to agent-a at creation.";
    const CANDIDATE_EVIDENCE: &str = "psv2:candidate-only-evidence";
    let platform = support::open_memory_store(
        StoreBackendConfig::in_memory(support::host_test_profile()).expect("config"),
    )
    .expect("in-memory store");
    let runtime_a = runtime(platform.clone(), registry(), "agent-a");
    let subject_a = default_agent_subject_id("agent-a");
    let subject_b = default_agent_subject_id("agent-b");
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Fact,
        topic: "psv1_candidate_only".to_string(),
    };

    runtime_a
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: None,
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-initial-only-subjects".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: target.clone(),
                long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::OnlySubjects(
                    vec![subject_a.clone()],
                )),
                privacy: MemoryPrivacyClass::PublicRuntime,
                content: MemoryCandidateContent::Text {
                    topic: "psv1_candidate_only".to_string(),
                    body: CANDIDATE_CONTENT.to_string(),
                    keywords: vec!["psv1".to_string(), "candidate".to_string()],
                },
                evidence_refs: vec![CANDIDATE_EVIDENCE.to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                    source: MemorySemanticJudgmentSource::RuntimeGate,
                    decision: MemoryCandidateSemanticDecision::Accept,
                    governed_target: Some(target),
                    reason: "trusted runtime gate".to_string(),
                }),
            }],
        })
        .expect("create candidate with initial OnlySubjects");

    let material_docs = platform
        .replay_harness()
        .read_json_namespace("long_term_version_materials")
        .expect("read candidate material");
    assert_eq!(material_docs.len(), 1);
    let material =
        serde_json::from_value::<LongTermMemoryVersionMaterial>(material_docs[0].value.clone())
            .expect("decode candidate material");
    assert_eq!(material.owner_revision, 1);
    assert_eq!(
        material.subject_visibility,
        MemorySubjectVisibilityPolicy::OnlySubjects(vec![subject_a.clone()])
    );
    assert_eq!(
        material.governed_content.provenance,
        LongTermMemoryProvenance {
            source_authority: MemoryEvidenceAuthority::UserAsserted,
            semantic_judgment_source: Some(MemorySemanticJudgmentSource::RuntimeGate),
        }
    );
    let owner_id = material.owner_ref.owner_id.clone();
    assert!(current_recall(&runtime_a)
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| capsule.content.contains(CANDIDATE_CONTENT)));
    let runtime_b = runtime(platform.clone(), registry(), "agent-b");
    assert_owner_exact_zero(
        &current_recall(&runtime_b),
        &owner_id,
        CANDIDATE_CONTENT,
        CANDIDATE_EVIDENCE,
    );

    let before = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot before forbidden transition");
    let error = runtime_a
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Fact,
                    topic: "psv1_candidate_only".to_string(),
                    content: "A plain upsert must not change initial visibility.".to_string(),
                    keywords: vec!["psv1".to_string(), "candidate".to_string()],
                    privacy: MemoryPrivacyClass::PublicRuntime,
                    source_chat_id: Some("psv1-reopen".to_string()),
                    source_type: None,
                    source_scope: None,
                    subject_visibility: MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec![
                        subject_b,
                    ]),
                    provenance: LongTermMemoryProvenance {
                        source_authority: MemoryEvidenceAuthority::UserAsserted,
                        semantic_judgment_source: Some(MemorySemanticJudgmentSource::RuntimeGate),
                    },
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["psv2:forbidden-policy-transition".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_002),
                    source_revision: Some(2),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
        })
        .expect_err("plain upsert visibility transition must require control mutation");

    assert_eq!(error.stage(), "long_term_entry_plan");
    assert!(
        error
            .to_string()
            .contains("SubjectVisibilityTransitionRequiresControl"),
        "{error}"
    );
    let after = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot after forbidden transition");
    assert_eq!(after, before);
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
