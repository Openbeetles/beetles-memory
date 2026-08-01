#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    governed_memory_recall_candidate_id, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    GovernedOwnerRevisionRef, LongTermControlOperation, LongTermInvalidationContract,
    LongTermInvalidationReasonCode, LongTermMemoryControlRevision, LongTermMemoryHeadManifest,
    LongTermMemoryStaleHint, LongTermMemoryVersionMaterial, LONG_TERM_CONTROL_AUDIT_NAMESPACE,
    LONG_TERM_CONTROL_REVISION_NAMESPACE, LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
    LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
};
use bm_core::platform::Platform as _;
use bm_sdk::{
    CanonicalTurnDelta, ConversationKey, ConversationScope, DerivedMemoryPlane, DerivedMemoryRef,
    LongTermMemoryKind, LongTermMemoryQuery, MemoryArchiveScope, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryEvidenceAuthority, MemoryGovernancePolicyMutation, MemoryGovernanceSelector,
    MemoryGovernanceSuppressionDuration, MemoryLongTermControlView, MemoryLongTermDetailRequest,
    MemoryLongTermListRequest, MemoryLongTermMutation, MemoryLongTermMutationRequest,
    MemoryLongTermPolicyRequest, MemoryLongTermTarget, MemoryPrivacyClass, MemoryProjectionRequest,
    MemoryRecallReport, MemoryRecallRequest, MemoryRecallTemporalOperation,
    MemorySemanticJudgmentSource, MemorySpaceExportRequest, MemorySpacePrivateMaterialPolicy,
    MemorySubjectVisibilityPolicy, MemoryTranscriptReplayRequest, MemoryTurnDeliveryStatus,
    MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource, MemoryWriteCandidate,
    MemoryWriteRequest, ParsedLongTermMemoryExtraction, PressureLevel, ProfileId,
    RuntimeLifecycleModeInput, RuntimeLifecycleOperation, TranscriptEvidenceRef,
    TranscriptInputMessage, TranscriptReplayView,
};

use support::{StaticHttpClient, StaticLlmClient};

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_sdk::IngressKind::User,
        channel: "llm.gateway".to_string(),
        provider: Some("test-provider".to_string()),
        protocol: MemoryTurnProtocol::Native,
        endpoint: None,
        model_alias: Some("test-model".to_string()),
        model_resolved: Some("test-model".to_string()),
        request_id: Some("req-long-term-control".to_string()),
        client_conversation_hint: Some("window-long-term-control".to_string()),
    }
}

fn finalize_request(user: &str, assistant: &str) -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: format!("turn-ltm-control-{}", user.len()),
            conversation: ConversationScope {
                channel: "llm.gateway".to_string(),
                chat_id: "chat-a".to_string(),
                conversation_id: Some("conversation-a".to_string()),
            },
            subject: "subject-default".to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            actor: None,
            input_messages: vec![TranscriptInputMessage::user(user)],
            assistant_message: Some(TranscriptInputMessage::assistant(assistant)),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        tool_calls: 0,
        runtime_skill_selected_ids: Vec::new(),
        task_learning_selected_ids: Vec::new(),
        reuse_outcome_note: String::new(),
        tool_usage_feedback: None,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }
}

fn assert_long_term_owner_exact_zero(
    recall: &MemoryRecallReport,
    owner_ref: &GovernedMemoryOwnerRef,
    content: &str,
) {
    let candidate_id = governed_memory_recall_candidate_id(owner_ref);
    assert!(!recall
        .working
        .long_term_memory_text
        .as_deref()
        .is_some_and(|text| text.contains(content)));
    for report in [
        Some(&recall.working.shared_factual_report),
        Some(&recall.working.continuity_capsule_report),
        Some(&recall.working.archive_recall_report),
        Some(&recall.working.runtime_skill_report),
        recall.working.task_recall_report.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        assert!(!report.candidates.iter().any(|candidate| {
            candidate.candidate_id == candidate_id
                || candidate.owner_ref.as_ref() == Some(owner_ref)
                || candidate.excerpt.contains(content)
        }));
        assert!(!report.selected_ids.contains(&candidate_id));
    }
    assert!(!recall.source_candidate_ids.contains(&candidate_id));
    assert!(!recall.graph_anchor_candidate_ids.contains(&candidate_id));
    assert!(!recall
        .facet_index_report
        .exact_facet_candidate_ids
        .contains(&candidate_id));
    assert!(!recall
        .facet_index_report
        .expanded_facet_candidate_ids
        .contains(&candidate_id));
    assert!(!recall
        .rank_fusion_report
        .candidate_reports
        .iter()
        .any(|candidate| candidate.candidate_id == candidate_id));
    for candidates in [
        &recall.coverage_selection_report.selected_candidate_ids,
        &recall
            .coverage_selection_report
            .coverage_dropped_candidate_ids,
        &recall
            .coverage_selection_report
            .fusion_dropped_candidate_ids,
        &recall
            .coverage_selection_report
            .budget_truncated_candidate_ids,
        &recall.graph_index_report.source_anchor_ids,
        &recall.graph_index_report.unmatched_source_anchor_ids,
        &recall.graph_index_report.expanded_node_ids,
        &recall.graph_rerank.candidate_ids,
        &recall.graph_rerank.expanded_candidate_ids,
        &recall.graph_rerank.graph_neighbor_ids,
        &recall.graph_rerank.reranked_candidate_ids,
        &recall.delivery_report.selected_candidate_ids,
    ] {
        assert!(!candidates.contains(&candidate_id));
    }
    assert!(!recall
        .graph_rerank
        .score_breakdown
        .iter()
        .any(|score| score.candidate_id == candidate_id));
    assert!(!recall
        .graph_candidate_evidence_ref_index
        .iter()
        .any(|entry| entry.candidate_id == candidate_id));
    assert!(!recall
        .compact_graph
        .nodes
        .iter()
        .any(|node| node.node_id == candidate_id));
    assert!(!recall
        .compact_graph
        .edges
        .iter()
        .any(|edge| { edge.from_node_id == candidate_id || edge.to_node_id == candidate_id }));
    assert!(!recall
        .delivery_report
        .selection_decisions
        .iter()
        .any(|decision| {
            decision.candidate_id == candidate_id || decision.owner_ref.as_ref() == Some(owner_ref)
        }));
    assert!(!recall
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| {
            capsule.candidate_id == candidate_id
                || &capsule.owner_ref == owner_ref
                || capsule.content.contains(content)
        }));
    assert!(!recall
        .delivery_report
        .render_decisions
        .iter()
        .any(|decision| decision.candidate_id == candidate_id));
}

fn assert_projection_exact_zero(runtime: &bm_sdk::MemoryRuntime, query: &str, content: &str) {
    let projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: query.to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection after governed owner exclusion");
    assert!(!projection
        .provider_payload()
        .system_memory_block()
        .contains(content));
}

fn historical_as_of_time_before_termination(
    platform: &bm_sdk::MemoryStoreHandle,
    owner_id: &str,
    operation: LongTermControlOperation,
) -> u64 {
    let control = platform
        .replay_harness()
        .read_json_namespace(LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .expect("historical control revision")
        .into_iter()
        .map(|entry| {
            serde_json::from_value::<LongTermMemoryControlRevision>(entry.value)
                .expect("typed control revision")
        })
        .find(|revision| {
            revision.transition.predecessor.owner_ref.owner_id == owner_id
                && revision.operation == operation
        })
        .expect("exact terminating control revision");
    control
        .transition
        .terminated_at
        .checked_sub(1)
        .expect("positive historical interval")
}

fn recall_historical(
    runtime: &bm_sdk::MemoryRuntime,
    query: &str,
    as_of_time: u64,
) -> MemoryRecallReport {
    runtime
        .recall(MemoryRecallRequest {
            temporal_operation: MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time },
            structured_query_facets: Vec::new(),
            query: query.to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("typed historical recall")
}

#[test]
fn runtime_lists_details_and_deletes_accepted_long_term_memory_with_audit() {
    let platform = support::seeded_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        ProfileId::DesktopMacosStandaloneMemory,
        "local",
        "chat-1",
    );

    let list = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Project),
                limit: 10,
                ..LongTermMemoryQuery::default()
            },
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list long-term");
    assert_eq!(list.records.len(), 1);
    let record_id = list.records[0].record.id.clone();

    let detail = runtime
        .get_long_term_memory(bm_sdk::MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(record_id.clone()),
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("detail");
    assert_eq!(detail.record.as_ref().unwrap().id, record_id);

    let delete = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
            },
            reason: "user_deleted_seeded_project_memory".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete");
    assert!(delete.accepted);
    assert_eq!(
        delete.lifecycle_report.operation,
        RuntimeLifecycleOperation::OperatorAction
    );
    assert!(delete.audit_event_id.is_some());
    assert_eq!(delete.tombstones.len(), 1);
    assert!(runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .unwrap()
        .records
        .is_empty());
    let deleted_detail = runtime
        .get_long_term_memory(bm_sdk::MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(record_id.clone()),
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("deleted detail");
    assert!(deleted_detail.record.is_none());
    assert!(!deleted_detail.revisions.is_empty());
    let deleted_tombstone = deleted_detail
        .tombstone
        .as_ref()
        .expect("public detail keeps the scoped typed tombstone");
    assert_eq!(deleted_tombstone.record_id, record_id);
    assert_eq!(
        deleted_tombstone.actor_subject_id.as_deref(),
        Some(runtime.scoped_runtime().actor_subject_id.as_str())
    );
    assert!(deleted_detail.transcript_refs.is_empty());

    let control_store = runtime
        .replay_harness()
        .scoped_long_term_memory_control_read_store(runtime.memory_space_id())
        .expect("scoped long-term control store");
    let tombstone = control_store
        .get_long_term_control_tombstone(&record_id)
        .unwrap()
        .expect("durable typed tombstone");
    assert_eq!(delete.tombstones[0].tombstone_id, tombstone.tombstone_id);
    assert_eq!(delete.tombstones[0].operation, tombstone.operation);
    assert_eq!(
        delete.affected_records[0].previous_digest,
        tombstone.previous_digest
    );
    let audits = control_store
        .list_long_term_control_audit(10)
        .expect("durable canonical audit");
    assert_eq!(audits.len(), 1);
    assert_eq!(
        delete.audit_event_id.as_deref(),
        Some(audits[0].event_id.as_str())
    );
    assert!(runtime
        .replay_harness()
        .read_events()
        .unwrap()
        .iter()
        .any(|event| event.kind_name == "operator.action"
            && event.payload.get("action").map(String::as_str)
                == Some("long_term_memory_control")));
    assert!(runtime
        .replay_harness()
        .read_events()
        .unwrap()
        .iter()
        .filter(|event| {
            [
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
                LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
                LONG_TERM_CONTROL_AUDIT_NAMESPACE,
            ]
            .contains(&event.plane.as_str())
        })
        .all(|event| !event.record_key.starts_with("scope:")));
}

#[test]
fn runtime_invalidate_retains_operator_only_version_closure_without_tombstone() {
    let profile = ProfileId::DesktopMacosStandaloneMemory;
    let platform = support::seeded_store_platform(profile);
    let runtime =
        support::test_runtime_with_scope(platform.clone(), profile, "local", "chat-invalidate");
    let record = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::Operator,
        })
        .expect("list long-term")
        .records[0]
        .record
        .clone();
    let record_id = record.id.clone();
    let before_materials = platform
        .replay_harness()
        .read_json_namespace("long_term_version_materials")
        .expect("before materials");

    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Invalidate {
                contract: LongTermInvalidationContract {
                    target: MemoryLongTermTarget::RecordId(record_id.clone()),
                    reason_code: LongTermInvalidationReasonCode::ContradictedByGovernedEvidence,
                    governed_evidence_refs: vec![GovernedOwnerRevisionRef::try_new(
                        GovernedMemoryOwnerRef::new(
                            GovernedMemoryOwnerPlane::EvidenceDocument,
                            "evidence-invalidation-1",
                        ),
                        1,
                    )
                    .expect("evidence ref")],
                    actor_subject_id: runtime.scoped_runtime().actor_subject_id.clone(),
                    audit_reason: "governed evidence contradicted the current owner".into(),
                },
            },
            reason: "governed evidence contradicted the current owner".into(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("invalidate");

    assert!(report.accepted);
    assert!(report.tombstones.is_empty());
    assert!(runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::Operator,
        })
        .expect("post-invalidation current read")
        .records
        .is_empty());
    let after_materials = platform
        .replay_harness()
        .read_json_namespace("long_term_version_materials")
        .expect("after materials");
    assert_eq!(after_materials, before_materials);
    let heads = platform
        .replay_harness()
        .read_json_namespace("long_term_head_manifests")
        .expect("terminal head");
    assert_eq!(heads.len(), 1);
    let head =
        serde_json::from_value::<LongTermMemoryHeadManifest>(heads[0].value.clone()).expect("head");
    assert!(head.terminal_transition_ref.is_some());
    let retained =
        serde_json::from_value::<LongTermMemoryVersionMaterial>(after_materials[0].value.clone())
            .expect("retained operator material");
    assert_eq!(retained.owner_ref.owner_id, record_id);
    let control_store = runtime
        .replay_harness()
        .scoped_long_term_memory_control_read_store(runtime.memory_space_id())
        .expect("control store");
    assert!(control_store
        .get_long_term_control_tombstone(&record_id)
        .expect("tombstone read")
        .is_none());
    let revisions = control_store
        .list_long_term_control_revisions(&record_id, 10)
        .expect("invalidation revision");
    assert_eq!(revisions.len(), 1);
    assert_eq!(
        revisions[0].invalidation_reason_code,
        Some(LongTermInvalidationReasonCode::ContradictedByGovernedEvidence)
    );
    assert_eq!(
        report.audit_event_id.as_deref(),
        control_store
            .list_long_term_control_audit(10)
            .expect("invalidation audit")
            .first()
            .map(|audit| audit.event_id.as_str())
    );
    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release safety".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall after invalidation");
    assert_long_term_owner_exact_zero(
        &recall,
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, record_id.clone()),
        &record.content,
    );
    let as_of_time = historical_as_of_time_before_termination(
        &platform,
        &record_id,
        LongTermControlOperation::Invalidate,
    );
    let historical = recall_historical(&runtime, &record.content, as_of_time);
    assert_long_term_owner_exact_zero(
        &historical,
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, record_id),
        &record.content,
    );
    assert_projection_exact_zero(&runtime, "release safety", &record.content);
}

#[test]
fn retained_revision_budget_rejects_owner_advance_without_any_store_change() {
    let profile = support::host_test_profile();
    let platform = support::empty_store_platform(profile);
    let runtime =
        support::test_runtime_with_scope(platform.clone(), profile, "local", "chat-retention");
    let draft_for_revision = |revision: usize| bm_sdk::LongTermMemoryDraft {
        kind: LongTermMemoryKind::Fact,
        topic: "bounded immutable history".to_string(),
        content: format!("Governed immutable material revision {revision}."),
        keywords: vec!["retention".to_string(), format!("revision-{revision}")],
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: Some("chat-retention".to_string()),
        source_type: None,
        source_scope: None,
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: vec!["external_eval:retention-budget".to_string()],
        canonical_entities: Vec::new(),
        evidence_count: Some(1),
        observed_at: Some(1_800_000_000 + revision as u64),
        last_confirmed_at: Some(1_800_000_000 + revision as u64),
        source_revision: Some(1),
    };

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![draft_for_revision(1)],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed first immutable revision");
    let owner_id = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list seeded owner")
        .records
        .into_iter()
        .next()
        .expect("seeded owner")
        .record
        .id;
    let cap = runtime
        .runtime_budget()
        .governed_state_budget
        .max_retained_long_term_revisions_per_owner;
    assert!(cap > 1);

    for revision in 2..=cap {
        runtime
            .mutate_long_term_memory(MemoryLongTermMutationRequest {
                operation: MemoryLongTermMutation::Correct {
                    target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                    replacement: draft_for_revision(revision),
                },
                reason: format!("advance_to_revision_{revision}"),
                dry_run: false,
                mode_input: RuntimeLifecycleModeInput::default(),
            })
            .unwrap_or_else(|error| {
                panic!("revision {revision} must fit the retained cap: {error}")
            });
    }

    let before = platform
        .export_replay_snapshot()
        .expect("snapshot at retained cap");
    let error = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(owner_id),
                replacement: draft_for_revision(cap + 1),
            },
            reason: "advance_beyond_retained_revision_budget".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect_err("advance beyond retained cap must fail closed");
    assert_eq!(error.stage(), "long_term_version_retention");
    assert!(error.to_string().contains("request-pinned retention limit"));
    assert_eq!(
        platform
            .export_replay_snapshot()
            .expect("snapshot after rejected advance"),
        before,
        "budget rejection must leave every store plane and event unchanged"
    );
}

#[test]
fn long_term_control_list_and_detail_use_the_governed_runtime_view() {
    let profile = support::host_test_profile();
    let platform = support::empty_store_platform(profile);
    let runtime = support::test_runtime_with_identity_scope_and_subject(
        platform.clone(),
        profile,
        "agent-a",
        "owner-a",
        "subject-a",
        "local",
        "chat-a",
    );
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![
                    bm_sdk::LongTermMemoryDraft {
                        kind: LongTermMemoryKind::Fact,
                        topic: "governed visible record".to_string(),
                        content: "Only the owning runtime may list this governed record."
                            .to_string(),
                        keywords: vec!["governed".to_string()],
                        privacy: MemoryPrivacyClass::SharedWithSubject,
                        source_chat_id: Some("chat-a".to_string()),
                        source_type: None,
                        source_scope: None,
                        confidence: None,
                        freshness: None,
                        stale_hint: None,
                        supporting_citations: vec!["external_eval:governed-visible".to_string()],
                        canonical_entities: Vec::new(),
                        evidence_count: Some(1),
                        observed_at: Some(1_800_000_000),
                        last_confirmed_at: Some(1_800_000_000),
                        source_revision: Some(1),
                    },
                    bm_sdk::LongTermMemoryDraft {
                        kind: LongTermMemoryKind::Fact,
                        topic: "governed private record".to_string(),
                        content: "PRIVATE_CONTROL_SURFACE_SENTINEL".to_string(),
                        keywords: vec!["private".to_string()],
                        privacy: MemoryPrivacyClass::SoulPrivate,
                        source_chat_id: Some("chat-a".to_string()),
                        source_type: None,
                        source_scope: None,
                        confidence: None,
                        freshness: None,
                        stale_hint: None,
                        supporting_citations: vec!["private://control-sentinel".to_string()],
                        canonical_entities: Vec::new(),
                        evidence_count: Some(1),
                        observed_at: Some(1_800_000_000),
                        last_confirmed_at: Some(1_800_000_000),
                        source_revision: Some(1),
                    },
                ],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed governed and private records");
    let raw_records = runtime
        .replay_harness()
        .scoped_long_term_memory_read_store(runtime.memory_space_id(), runtime.subject_id())
        .expect("scoped long-term read store")
        .list(10)
        .expect("raw owner records");
    let visible_id = raw_records
        .iter()
        .find(|entry| entry.content.contains("Only the owning runtime"))
        .expect("visible owner record")
        .id
        .clone();
    let private_id = raw_records
        .iter()
        .find(|entry| entry.content.contains("PRIVATE_CONTROL_SURFACE_SENTINEL"))
        .expect("private owner record")
        .id
        .clone();

    let owner_list = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::Operator,
        })
        .expect("owner governed list");
    assert_eq!(owner_list.records.len(), 1);
    assert_eq!(owner_list.records[0].record.id, visible_id);
    let visible_revision = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(visible_id.clone()),
                replacement: bm_sdk::LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Fact,
                    topic: "governed visible record".to_string(),
                    content: "Only the owning runtime may list this governed record.".to_string(),
                    keywords: vec!["governed".to_string(), "corrected".to_string()],
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    source_chat_id: Some("chat-a".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["external_eval:governed-visible".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    last_confirmed_at: Some(1_800_000_000),
                    source_revision: Some(2),
                },
            },
            reason: "known visible metadata must remain scope governed".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("create governed visible revision");
    assert!(visible_revision.accepted);

    let private_mutation = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(private_id.clone()),
            },
            reason: "private owner must reject public control mutation".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("reject private control mutation through governed report");
    assert!(!private_mutation.accepted);

    let private_detail = runtime
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(private_id.clone()),
            view: MemoryLongTermControlView::Operator,
        })
        .expect("private governed detail");
    assert!(private_detail.record.is_none());
    assert!(private_detail.revisions.is_empty());
    assert!(private_detail.tombstone.is_none());
    assert!(private_detail.transcript_refs.is_empty());

    let other_runtime = support::test_runtime_with_identity_scope_and_subject(
        platform,
        profile,
        "agent-b",
        "owner-b",
        "subject-b",
        "local",
        "chat-b",
    );
    assert!(other_runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::Operator,
        })
        .expect("cross-space governed list")
        .records
        .is_empty());
    let cross_subject_detail = other_runtime
        .get_long_term_memory(MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(visible_id),
            view: MemoryLongTermControlView::Operator,
        })
        .expect("cross-space governed detail");
    assert!(cross_subject_detail.record.is_none());
    assert!(cross_subject_detail.revisions.is_empty());
    assert!(cross_subject_detail.tombstone.is_none());
    assert!(cross_subject_detail.transcript_refs.is_empty());
}

#[test]
fn long_term_control_mutation_reports_affected_facet_docs_for_operator_review() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = support::test_runtime_with_scope(
        platform,
        support::host_test_profile(),
        "local",
        "facet-control-chat",
    );

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![bm_sdk::LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    privacy: bm_sdk::MemoryPrivacyClass::SharedWithSubject,
                    topic: "facet control review".to_string(),
                    content: "Workbench control deletes must expose affected facet docs."
                        .to_string(),
                    keywords: vec!["facet".to_string(), "control".to_string()],
                    source_chat_id: Some("facet-control-chat".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["external_eval:facet-control-review".to_string()],
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
        .expect("seed long-term facet docs");
    let record_id = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::Operator,
        })
        .expect("list long-term")
        .records[0]
        .record
        .id
        .clone();

    let delete = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
            },
            reason: "operator_deleted_record_and_needs_facet_impact".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete long-term record");

    assert!(delete.accepted);
    assert!(delete
        .affected_facet_docs
        .iter()
        .any(|doc| doc.action == "delete"
            && doc.owner_token.starts_with("facet-owner-token:")
            && !doc.owner_token.contains(&record_id)
            && doc.report_view.redacted_sensitive_metadata
            && doc.report_view.owner_ref.is_none()));
    assert!(delete
        .affected_facet_docs
        .iter()
        .all(|doc| !doc.owner_token.trim().is_empty()));
}

#[test]
fn runtime_dry_run_does_not_mutate_long_term_store() {
    let platform = support::seeded_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        ProfileId::DesktopMacosStandaloneMemory,
        "local",
        "chat-1",
    );
    let record_id = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .unwrap()
        .records[0]
        .record
        .id
        .clone();

    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(record_id.clone()),
                source_scope: bm_sdk::LongTermMemorySourceScope::User,
                subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![
                    "subject-human".to_string(),
                ]),
            },
            reason: "preview_scope_change".to_string(),
            dry_run: true,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("dry run");

    assert!(report.dry_run);
    assert!(!report.accepted);
    assert!(runtime
        .get_long_term_memory(bm_sdk::MemoryLongTermDetailRequest {
            target: MemoryLongTermTarget::RecordId(record_id),
            view: MemoryLongTermControlView::HostUi,
        })
        .unwrap()
        .revisions
        .is_empty());
}

#[test]
fn runtime_policy_mutation_persists_suppression_policy() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime =
        support::test_runtime_with_scope(platform, support::host_test_profile(), "local", "chat-1");

    let report = runtime
        .mutate_memory_governance_policy(MemoryLongTermPolicyRequest {
            operation: MemoryGovernancePolicyMutation::Suppress {
                selector: MemoryGovernanceSelector {
                    memory_space_id: Some(runtime.memory_space_id().to_string()),
                    subject_id: Some(runtime.subject_id().to_string()),
                    kind: Some(LongTermMemoryKind::Preference),
                    topic_pattern: Some("temporary-*".to_string()),
                    source_chat_id: None,
                    source_scope: None,
                },
                duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
            },
            reason: "user_declined_temporary_preference_memory".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("policy mutation");

    assert!(report.accepted);
    let policies = runtime
        .replay_harness()
        .scoped_long_term_memory_control_read_store(runtime.memory_space_id())
        .expect("scoped long-term control store")
        .list_long_term_governance_policies(10)
        .unwrap();
    assert_eq!(policies.len(), 1);
    assert_eq!(report.policy_id, Some(policies[0].policy_id.clone()));
}

#[test]
fn runtime_rejects_space_wide_policy_before_persistence_and_scoped_export() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "local",
        "chat-1",
    );

    let error = runtime
        .mutate_memory_governance_policy(MemoryLongTermPolicyRequest {
            operation: MemoryGovernancePolicyMutation::Suppress {
                selector: MemoryGovernanceSelector {
                    memory_space_id: Some(runtime.memory_space_id().to_string()),
                    subject_id: None,
                    kind: Some(LongTermMemoryKind::Preference),
                    topic_pattern: None,
                    source_chat_id: None,
                    source_scope: None,
                },
                duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
            },
            reason: "space-wide policy is outside scoped runtime storage".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect_err("space-wide policy must fail before persistence");
    assert_eq!(error.stage(), "long_term_control_policy_subject_required");

    let control_store = platform
        .replay_harness()
        .scoped_long_term_memory_control_read_store(runtime.memory_space_id())
        .expect("scoped control store");
    assert!(control_store
        .list_long_term_governance_policies(10)
        .expect("policies")
        .is_empty());
    assert!(control_store
        .list_long_term_control_audit(10)
        .expect("audits")
        .is_empty());

    let archive = runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: MemoryArchiveScope::subject(runtime.memory_space_id(), runtime.subject_id())
                .expect("runtime archive scope"),
            private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate,
        })
        .expect("scoped export")
        .archive;
    assert!(!archive.contains_json_namespace(LONG_TERM_GOVERNANCE_POLICY_NAMESPACE));
    assert!(!archive.contains_json_namespace(LONG_TERM_CONTROL_AUDIT_NAMESPACE));
}

#[test]
fn runtime_tombstone_hides_record_from_recall_and_projection_context() {
    let platform = support::seeded_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        ProfileId::DesktopMacosStandaloneMemory,
        "local",
        "chat-1",
    );
    let record = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .unwrap()
        .records[0]
        .record
        .clone();
    let owner_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, record.id.clone());

    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(record.id.clone()),
            },
            reason: "user_deleted_seeded_project_memory".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete");

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release safety".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    assert_long_term_owner_exact_zero(&recall, &owner_ref, &record.content);
    let as_of_time = historical_as_of_time_before_termination(
        &platform,
        &record.id,
        LongTermControlOperation::Delete,
    );
    let historical = recall_historical(&runtime, &record.content, as_of_time);
    assert_long_term_owner_exact_zero(&historical, &owner_ref, &record.content);

    assert_projection_exact_zero(&runtime, "How should release safety work?", &record.content);
}

#[test]
fn runtime_forget_by_query_excludes_owner_from_every_recall_stage_and_projection() {
    let profile = ProfileId::DesktopMacosStandaloneMemory;
    let platform = support::seeded_store_platform(profile);
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        profile,
        "local",
        "chat-forget-by-query",
    );
    let record = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list seeded owner")
        .records[0]
        .record
        .clone();
    let selector = bm_sdk::MemoryLongTermSelector {
        query: LongTermMemoryQuery {
            kind: Some(record.kind.clone()),
            topic: Some(record.topic.clone()),
            limit: 10,
            ..LongTermMemoryQuery::default()
        },
        evidence_ref: None,
    };
    let preview = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ForgetByQuery {
                selector: selector.clone(),
                confirmation_token: None,
            },
            reason: "preview governed exact-query forgetting".to_string(),
            dry_run: true,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("forget preview");
    let confirmation_token = preview
        .policy_decision
        .confirmation_token
        .expect("canonical confirmation token");
    let applied = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ForgetByQuery {
                selector,
                confirmation_token: Some(confirmation_token),
            },
            reason: "apply governed exact-query forgetting".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("forget apply");
    assert!(applied.accepted);

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release safety".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall after forget");
    assert_long_term_owner_exact_zero(
        &recall,
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, record.id.clone()),
        &record.content,
    );
    let as_of_time = historical_as_of_time_before_termination(
        &platform,
        &record.id,
        LongTermControlOperation::ForgetByQuery,
    );
    let historical = recall_historical(&runtime, &record.content, as_of_time);
    assert_long_term_owner_exact_zero(
        &historical,
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, record.id.clone()),
        &record.content,
    );
    assert_projection_exact_zero(&runtime, "release safety", &record.content);
}

#[test]
fn runtime_soul_private_owner_is_exact_zero_across_recall_and_projection() {
    let profile = ProfileId::DesktopMacosStandaloneMemory;
    let platform = support::seeded_store_platform(profile);
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        profile,
        "local",
        "chat-private-exact-zero",
    );
    let record = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list seeded owner")
        .records[0]
        .record
        .clone();
    let changed = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangePrivacy {
                target: MemoryLongTermTarget::RecordId(record.id.clone()),
                privacy: MemoryPrivacyClass::SoulPrivate,
            },
            reason: "move owner behind soul-private disclosure gate".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("change privacy");
    assert!(changed.accepted);

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release safety".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall after privacy transition");
    assert_long_term_owner_exact_zero(
        &recall,
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, record.id.clone()),
        &record.content,
    );
    let as_of_time = historical_as_of_time_before_termination(
        &platform,
        &record.id,
        LongTermControlOperation::ChangePrivacy,
    );
    let historical_error = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time },
            structured_query_facets: Vec::new(),
            query: record.content.clone(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect_err("cross-privacy historical lineage must fail closed");
    assert_eq!(
        historical_error.stage(),
        "long_term_historical_recall_authority"
    );
    assert_projection_exact_zero(&runtime, "release safety", &record.content);
}

#[test]
fn runtime_mark_stale_excludes_owner_before_rank_selection_and_render() {
    let platform = support::seeded_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let runtime = support::test_runtime_with_scope(
        platform,
        ProfileId::DesktopMacosStandaloneMemory,
        "local",
        "chat-1",
    );
    let record = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list seeded owner")
        .records
        .into_iter()
        .next()
        .expect("seeded owner")
        .record;
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::MarkStale {
                target: MemoryLongTermTarget::RecordId(record.id.clone()),
                stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
            },
            reason: "current owner requires governed refresh".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("mark stale");

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release safety".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall after mark stale");
    let candidate_id = governed_memory_recall_candidate_id(&GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::LongTerm,
        record.id.clone(),
    ));
    assert_long_term_owner_exact_zero(
        &recall,
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, record.id.clone()),
        &record.content,
    );
    assert_projection_exact_zero(&runtime, "release safety", &record.content);

    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(record.id.clone()),
                source_scope: bm_sdk::LongTermMemorySourceScope::User,
                subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            },
            reason: "scope change must preserve explicit stale state".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("change scope after mark stale");
    let after_scope_change = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release safety".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall after stale-preserving scope change");
    assert!(!after_scope_change
        .source_candidate_ids
        .contains(&candidate_id));
    assert_long_term_owner_exact_zero(
        &after_scope_change,
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, record.id.clone()),
        &record.content,
    );
    assert!(!after_scope_change
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| capsule.owner_ref.owner_id == record.id));

    let corrected_draft = |content: &str, source_revision: u64| bm_sdk::LongTermMemoryDraft {
        kind: record.kind.clone(),
        topic: record.topic.clone(),
        content: content.to_string(),
        keywords: record.keywords.clone(),
        privacy: record.privacy,
        source_chat_id: record.source_chat_id.clone(),
        source_type: Some(record.source_type),
        source_scope: Some(bm_sdk::LongTermMemorySourceScope::User),
        confidence: Some(record.confidence),
        freshness: Some(record.freshness),
        stale_hint: None,
        supporting_citations: vec!["external_eval:stale-correction".to_string()],
        canonical_entities: record.canonical_entities.clone(),
        evidence_count: Some(record.evidence_count),
        observed_at: Some(record.observed_at),
        last_confirmed_at: Some(record.last_confirmed_at),
        source_revision: Some(source_revision),
    };
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(record.id.clone()),
                replacement: corrected_draft(
                    "Corrected release safety instruction after governed verification.",
                    record.source_revision.unwrap_or_default() + 1,
                ),
            },
            reason: "semantic correction clears the prior explicit stale state".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("correct stale owner");
    let after_correction = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release safety".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall after semantic correction");
    assert!(after_correction
        .source_candidate_ids
        .contains(&candidate_id));

    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::MarkStale {
                target: MemoryLongTermTarget::RecordId(record.id.clone()),
                stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
            },
            reason: "second explicit stale state".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("mark corrected owner stale");
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::MarkStale {
                target: MemoryLongTermTarget::RecordId(record.id.clone()),
                stale_hint: LongTermMemoryStaleHint::None,
            },
            reason: "governed refresh completed".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("clear stale state");
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(record.id.clone()),
                replacement: corrected_draft(
                    "Second corrected release safety instruction.",
                    record.source_revision.unwrap_or_default() + 2,
                ),
            },
            reason: "automatic dynamic hint must not recreate explicit stale state".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("correct after explicit stale clear");
    let after_clear = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release safety".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall after clearing stale state");
    assert!(after_clear.source_candidate_ids.contains(&candidate_id));
}

#[test]
fn runtime_supersede_excludes_predecessor_and_admits_exact_cross_owner_successor() {
    let platform = support::seeded_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        ProfileId::DesktopMacosStandaloneMemory,
        "local",
        "chat-1",
    );
    let predecessor = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list seeded owner")
        .records
        .into_iter()
        .next()
        .expect("seeded owner")
        .record;
    let successor_content = "Use the signed release manifest as the replacement safety authority.";
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Supersede {
                target: MemoryLongTermTarget::RecordId(predecessor.id.clone()),
                replacement: bm_sdk::LongTermMemoryDraft {
                    kind: predecessor.kind.clone(),
                    topic: "signed release replacement".to_string(),
                    content: successor_content.to_string(),
                    keywords: vec!["signed".to_string(), "manifest".to_string()],
                    privacy: predecessor.privacy,
                    source_chat_id: predecessor.source_chat_id.clone(),
                    source_type: Some(predecessor.source_type),
                    source_scope: Some(predecessor.source_scope),
                    confidence: Some(predecessor.confidence),
                    freshness: Some(predecessor.freshness),
                    stale_hint: None,
                    supporting_citations: vec!["external_eval:supersede-successor".to_string()],
                    canonical_entities: predecessor.canonical_entities.clone(),
                    evidence_count: Some(predecessor.evidence_count),
                    observed_at: Some(predecessor.observed_at.saturating_add(1)),
                    last_confirmed_at: Some(predecessor.last_confirmed_at.saturating_add(1)),
                    source_revision: Some(
                        predecessor
                            .source_revision
                            .unwrap_or_default()
                            .saturating_add(1),
                    ),
                },
            },
            reason: "replace the prior owner with a new governed authority".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("supersede owner");

    let successor = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery {
                topic: Some("signed release replacement".to_string()),
                ..LongTermMemoryQuery::default()
            },
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list successor")
        .records
        .into_iter()
        .next()
        .expect("successor owner")
        .record;
    assert_ne!(successor.id, predecessor.id);
    assert_eq!(successor.owner_revision, 1);

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "signed release manifest safety".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall across exact supersede dependency");
    let predecessor_candidate = governed_memory_recall_candidate_id(&GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::LongTerm,
        predecessor.id.clone(),
    ));
    let successor_candidate = governed_memory_recall_candidate_id(&GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::LongTerm,
        successor.id.clone(),
    ));
    assert!(!recall.source_candidate_ids.contains(&predecessor_candidate));
    assert!(recall.source_candidate_ids.contains(&successor_candidate));
    assert_long_term_owner_exact_zero(
        &recall,
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, predecessor.id.clone()),
        &predecessor.content,
    );

    let predecessor_material = platform
        .replay_harness()
        .read_json_namespace("long_term_version_materials")
        .expect("retained supersede materials")
        .into_iter()
        .map(|entry| {
            serde_json::from_value::<LongTermMemoryVersionMaterial>(entry.value)
                .expect("typed retained material")
        })
        .find(|material| material.owner_ref.owner_id == predecessor.id)
        .expect("retained predecessor material");
    let supersede_control = platform
        .replay_harness()
        .read_json_namespace(LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .expect("supersede control")
        .into_iter()
        .map(|entry| {
            serde_json::from_value::<LongTermMemoryControlRevision>(entry.value)
                .expect("typed control revision")
        })
        .find(|revision| {
            revision.transition.predecessor.owner_ref.owner_id == predecessor.id
                && revision.operation == LongTermControlOperation::Supersede
        })
        .expect("exact supersede transition");
    let as_of_time = supersede_control
        .transition
        .terminated_at
        .checked_sub(1)
        .expect("positive historical interval");
    assert!(as_of_time >= predecessor_material.origin.valid_from);

    let current_old_query = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: predecessor.content.clone(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("current recall must not fall back to the predecessor");
    assert_long_term_owner_exact_zero(
        &current_old_query,
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, predecessor.id.clone()),
        &predecessor.content,
    );

    let historical = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time },
            structured_query_facets: Vec::new(),
            query: predecessor.content.clone(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("explicit typed as-of recall");
    assert_eq!(
        historical.temporal_operation,
        MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time }
    );
    assert!(historical
        .source_candidate_ids
        .contains(&predecessor_candidate));
    assert!(!historical
        .source_candidate_ids
        .contains(&successor_candidate));
    assert!(historical
        .working
        .long_term_memory_text
        .as_deref()
        .is_some_and(|text| text.contains(&predecessor.content)));
    assert!(historical
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| {
            capsule.candidate_id == predecessor_candidate
                && capsule.content.contains(&predecessor.content)
        }));

    let historical_projection = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time },
            structured_query_facets: Vec::new(),
            user_query: predecessor.content.clone(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("explicit typed as-of projection");
    assert_eq!(
        historical_projection.report().temporal_operation(),
        MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time }
    );
    assert!(historical_projection
        .provider_payload()
        .system_memory_block()
        .contains(&predecessor.content));
    assert!(!historical_projection
        .provider_payload()
        .system_memory_block()
        .contains(&successor.content));
    assert!(historical_projection
        .report()
        .procedural_delivery_reports()
        .is_empty());
    assert!(
        historical_projection
            .report()
            .audit()
            .delivery_digest_verified
    );
}

#[test]
fn runtime_revised_predecessor_uses_the_exact_half_open_historical_interval() {
    let profile = ProfileId::DesktopMacosStandaloneMemory;
    let platform = support::seeded_store_platform(profile);
    let runtime =
        support::test_runtime_with_scope(platform.clone(), profile, "local", "chat-revised");
    let record = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list seeded owner")
        .records[0]
        .record
        .clone();
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(record.id.clone()),
                source_scope: bm_sdk::LongTermMemorySourceScope::World,
                subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
            },
            reason: "advance a non-semantic revised owner".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("change source scope");

    let predecessor_as_of = historical_as_of_time_before_termination(
        &platform,
        &record.id,
        LongTermControlOperation::ChangeScope,
    );
    let predecessor = recall_historical(&runtime, &record.content, predecessor_as_of);
    let predecessor_capsule = predecessor
        .delivery_report
        .rendered_capsules
        .iter()
        .find(|capsule| capsule.owner_ref.owner_id == record.id)
        .expect("revised predecessor capsule");
    assert_eq!(
        predecessor_capsule.valid_until,
        Some(predecessor_as_of.saturating_add(1))
    );

    let successor_as_of = predecessor_as_of.saturating_add(1);
    let successor = recall_historical(&runtime, &record.content, successor_as_of);
    let successor_capsule = successor
        .delivery_report
        .rendered_capsules
        .iter()
        .find(|capsule| capsule.owner_ref.owner_id == record.id)
        .expect("revised successor capsule at half-open boundary");
    assert_eq!(successor_capsule.valid_from, Some(successor_as_of));
    assert_eq!(successor_capsule.valid_until, None);
}

#[test]
fn runtime_correct_recall_uses_only_the_exact_current_revision_and_validity() {
    let platform = support::seeded_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        ProfileId::DesktopMacosStandaloneMemory,
        "local",
        "chat-1",
    );
    let previous = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list seeded owner")
        .records
        .into_iter()
        .next()
        .expect("seeded owner")
        .record;
    let corrected_content = "Verify the signed manifest before publishing release artifacts.";
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(previous.id.clone()),
                replacement: bm_sdk::LongTermMemoryDraft {
                    kind: previous.kind.clone(),
                    topic: previous.topic.clone(),
                    content: corrected_content.to_string(),
                    keywords: vec!["signed".to_string(), "manifest".to_string()],
                    privacy: previous.privacy,
                    source_chat_id: previous.source_chat_id.clone(),
                    source_type: Some(previous.source_type),
                    source_scope: Some(previous.source_scope),
                    confidence: Some(previous.confidence),
                    freshness: Some(previous.freshness),
                    stale_hint: Some(LongTermMemoryStaleHint::None),
                    supporting_citations: vec![
                        "external_eval:current-revision-contract".to_string()
                    ],
                    canonical_entities: previous.canonical_entities.clone(),
                    evidence_count: Some(previous.evidence_count),
                    observed_at: Some(previous.observed_at),
                    last_confirmed_at: Some(previous.last_confirmed_at),
                    source_revision: previous.source_revision.map(|revision| revision + 1),
                },
            },
            reason: "replace the obsolete release instruction".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("correct owner");

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "signed manifest release".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall corrected owner");
    let candidate_id = governed_memory_recall_candidate_id(&GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::LongTerm,
        previous.id.clone(),
    ));
    assert_eq!(
        recall
            .source_candidate_ids
            .iter()
            .filter(|candidate| *candidate == &candidate_id)
            .count(),
        1
    );
    let capsule = recall
        .delivery_report
        .rendered_capsules
        .iter()
        .find(|capsule| capsule.owner_ref.owner_id == previous.id)
        .expect("current revision capsule");
    assert!(capsule.content.contains(corrected_content));
    assert!(!capsule.content.contains(&previous.content));
    assert!(capsule.valid_from.is_some());
    assert_eq!(capsule.valid_until, None);
    let as_of_time = historical_as_of_time_before_termination(
        &platform,
        &previous.id,
        LongTermControlOperation::Correct,
    );
    let historical = recall_historical(&runtime, &previous.content, as_of_time);
    assert_long_term_owner_exact_zero(
        &historical,
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, previous.id.clone()),
        &previous.content,
    );
}

#[test]
fn runtime_suppression_policy_blocks_future_candidate_long_term_writes() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "local",
        "chat-1",
    );
    runtime
        .mutate_memory_governance_policy(MemoryLongTermPolicyRequest {
            operation: MemoryGovernancePolicyMutation::Suppress {
                selector: MemoryGovernanceSelector {
                    memory_space_id: Some(runtime.memory_space_id().to_string()),
                    subject_id: Some(runtime.subject_id().to_string()),
                    kind: Some(LongTermMemoryKind::Preference),
                    topic_pattern: Some("temporary-*".to_string()),
                    source_chat_id: None,
                    source_scope: None,
                },
                duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
            },
            reason: "user_declined_temporary_preference_memory".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("policy mutation");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: None,
            candidates: vec![bm_core::memory::MemoryWriteCandidate {
                candidate_id: "candidate-temporary-tone".to_string(),
                authority: bm_core::memory::MemoryEvidenceAuthority::UserAsserted,
                target: bm_core::memory::MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Preference,
                    topic: "temporary-tone".to_string(),
                },
                privacy: bm_core::memory::MemoryPrivacyClass::SharedWithSubject,
                content: bm_core::memory::MemoryCandidateContent::Text {
                    topic: "temporary-tone".to_string(),
                    body: "User briefly wants a pirate tone for this one chat.".to_string(),
                    keywords: vec!["temporary".to_string(), "tone".to_string()],
                },
                evidence_refs: vec!["turn:candidate-temporary-tone".to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(bm_core::memory::MemoryCandidateSemanticJudgment {
                    source: bm_core::memory::MemorySemanticJudgmentSource::LlmGovernance,
                    decision: bm_core::memory::MemoryCandidateSemanticDecision::Accept,
                    governed_target: Some(bm_core::memory::MemoryCandidateTarget::LongTermMemory {
                        kind: LongTermMemoryKind::Preference,
                        topic: "temporary-tone".to_string(),
                    }),
                    reason: "llm_accepts_candidate".to_string(),
                }),
            }],
        })
        .expect("candidate write");

    assert_eq!(report.changed, 0);
    assert!(
        report.reason.contains("suppressed_by_long_term_policy"),
        "write report should make policy blocking visible"
    );
    assert_eq!(
        platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:owner-default", "agent:agent-main")
            .expect("scoped long-term read store")
            .count()
            .unwrap(),
        0
    );
}

#[test]
fn runtime_suppression_policy_blocks_long_term_extraction_writes() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "local",
        "chat-1",
    );
    runtime
        .mutate_memory_governance_policy(MemoryLongTermPolicyRequest {
            operation: MemoryGovernancePolicyMutation::Suppress {
                selector: MemoryGovernanceSelector {
                    memory_space_id: Some(runtime.memory_space_id().to_string()),
                    subject_id: Some(runtime.subject_id().to_string()),
                    kind: Some(LongTermMemoryKind::Preference),
                    topic_pattern: Some("temporary-*".to_string()),
                    source_chat_id: None,
                    source_scope: None,
                },
                duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
            },
            reason: "user_declined_temporary_preference_memory".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("policy mutation");

    let report = runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![bm_sdk::LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Preference,
                    privacy: bm_sdk::MemoryPrivacyClass::SharedWithSubject,
                    topic: "temporary-tone".to_string(),
                    content: "User briefly wants pirate tone in this one chat.".to_string(),
                    keywords: vec!["temporary".to_string(), "tone".to_string()],
                    source_chat_id: Some("chat-1".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["transcript:chat-1#message=1".to_string()],
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
        .expect("extraction write");

    assert_eq!(report.changed, 0);
    assert!(report.reason.contains("suppressed_by_long_term_policy"));
    assert_eq!(
        platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:owner-default", "agent:agent-main")
            .expect("scoped long-term read store")
            .count()
            .unwrap(),
        0
    );
}

#[test]
fn runtime_suppression_policy_blocks_automatic_post_turn_long_term_refresh() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = support::test_runtime_with_scope_and_subject(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .mutate_memory_governance_policy(MemoryLongTermPolicyRequest {
            operation: MemoryGovernancePolicyMutation::Suppress {
                selector: MemoryGovernanceSelector {
                    memory_space_id: Some(runtime.memory_space_id().to_string()),
                    subject_id: Some(runtime.subject_id().to_string()),
                    kind: Some(LongTermMemoryKind::Preference),
                    topic_pattern: Some("temporary-*".to_string()),
                    source_chat_id: None,
                    source_scope: None,
                },
                duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
            },
            reason: "user_declined_temporary_preference_memory".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("policy mutation");
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response(
        r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "preference",
                "source_authority": "user_asserted",
                "topic": "temporary-tone",
                "content": "User briefly wants pirate tone in this one chat.",
                "keywords": ["temporary", "tone"]
            }
        ]"#,
    );

    let report = runtime
        .finalize_turn_and_maintain(
            Some(&mut http),
            Some(&llm),
            finalize_request(
                "这次临时用海盗语气回答",
                "收到，只在这次对话里保持这个语气。",
            ),
        )
        .expect("finalize");

    assert_eq!(
        platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:owner-default", "agent:agent-main")
            .expect("scoped long-term read store")
            .count()
            .unwrap(),
        0
    );
    assert_eq!(report.semantic_governance.accepted_count, 0);
    assert!(report
        .semantic_governance
        .plane_reports
        .iter()
        .any(|plane| plane.plane == "long_term_memory"
            && plane.reason == "long_term_extraction_noop"));
}

#[test]
fn runtime_mutates_long_term_memory_from_transcript_derived_ref_target() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = support::test_runtime_with_scope_and_subject(
        platform,
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    runtime
        .finalize_turn_and_maintain(None, None, finalize_request("我偏好简洁回答", "已记录。"))
        .unwrap();
    let replay = runtime
        .replay_transcript(MemoryTranscriptReplayRequest {
            memory_space_id: runtime.memory_space_id().to_string(),
            channel_id: "llm.gateway".to_string(),
            conversation_id: "conversation-a".to_string(),
            limit: 10,
            cursor: None,
            view: TranscriptReplayView::RawOwnerOnly,
        })
        .unwrap();
    let turn = &replay.slice.turns[0];
    let message = &turn.input_messages[0];
    let evidence_ref = TranscriptEvidenceRef {
        memory_space_id: runtime.memory_space_id().to_string(),
        channel_id: "llm.gateway".to_string(),
        conversation_id: "conversation-a".to_string(),
        turn_id: turn.turn_id.clone(),
        message_id: Some(message.message_id.clone()),
        subject_id: Some("subject-default".to_string()),
        authority: Some(MemoryEvidenceAuthority::UserAsserted),
    };

    runtime
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: None,
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-concise-style".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Preference,
                    topic: "response_style".to_string(),
                },
                privacy: MemoryPrivacyClass::SharedWithSubject,
                content: MemoryCandidateContent::Text {
                    topic: "response_style".to_string(),
                    body: "The user prefers concise answers.".to_string(),
                    keywords: vec!["concise".to_string()],
                },
                evidence_refs: vec![evidence_ref.display_citation()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                    source: MemorySemanticJudgmentSource::LlmGovernance,
                    decision: MemoryCandidateSemanticDecision::Accept,
                    governed_target: Some(MemoryCandidateTarget::LongTermMemory {
                        kind: LongTermMemoryKind::Preference,
                        topic: "response_style".to_string(),
                    }),
                    reason: "user_asserted_preference".to_string(),
                }),
            }],
        })
        .unwrap();
    let record_id = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::Operator,
        })
        .unwrap()
        .records
        .iter()
        .find(|record| record.record.topic == "response_style")
        .expect("long-term record")
        .record
        .id
        .clone();
    let derived_ref = DerivedMemoryRef {
        plane: DerivedMemoryPlane::LongTerm,
        store_key: record_id,
        subject_id: Some("subject-default".to_string()),
        source: evidence_ref,
        created_at: 10,
    };

    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::TranscriptDerivedRef(derived_ref),
            },
            reason: "delete_from_transcript_lifecycle_impact".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete from derived ref");

    assert!(report.accepted);
    assert_eq!(report.target_report.resolved_count, 1);
    assert!(!report.transcript_refs.is_empty());
    assert!(runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .unwrap()
        .records
        .is_empty());
}

#[test]
fn runtime_mutates_shared_fact_memory_from_transcript_derived_ref_target() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = support::test_runtime_with_scope_and_subject(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
        "subject-default",
    );
    let mut http = StaticHttpClient;
    let llm = StaticLlmClient::summary_response(
        r#"[
            {
                "plane": "factual",
                "op": "upsert",
                "kind": "fact",
                "source_authority": "user_asserted",
                "topic": "primary_llm",
                "content": "当前主模型是 OpenAI。",
                "keywords": ["OpenAI", "主模型"]
            }
        ]"#,
    );
    let request = finalize_request(
        "当前主模型已经切到 OpenAI",
        "收到，这轮把主模型事实和证据一起写回 shared factual plane。",
    );
    runtime
        .finalize_turn_and_maintain(Some(&mut http), Some(&llm), request)
        .unwrap();
    let key = ConversationKey::new(
        runtime.memory_space_id().to_string(),
        "llm.gateway",
        "conversation-a",
    )
    .unwrap();
    let derived_ref = platform
        .replay_harness()
        .conversation_transcript_store()
        .list_derived_memory_refs(&key, runtime.subject_id(), None)
        .unwrap()
        .into_iter()
        .find(|derived| derived.plane == DerivedMemoryPlane::SharedFact)
        .expect("shared fact derived ref");

    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::TranscriptDerivedRef(derived_ref),
            },
            reason: "delete_shared_fact_from_transcript_lifecycle_impact".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete shared fact from derived ref");

    assert!(report.accepted);
    assert_eq!(report.target_report.resolved_count, 1);
    assert_eq!(
        platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:owner-default", "agent:agent-main")
            .expect("scoped long-term read store")
            .count()
            .unwrap(),
        0
    );
}

#[test]
fn runtime_rejects_bulk_forget_when_profile_hides_capability() {
    let platform = support::seeded_store_platform(ProfileId::EspStandaloneMemory);
    let runtime = support::test_runtime_with_scope(
        platform,
        ProfileId::EspStandaloneMemory,
        "local",
        "chat-1",
    );

    let err = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ForgetByQuery {
                selector: bm_sdk::MemoryLongTermSelector {
                    query: LongTermMemoryQuery {
                        kind: Some(LongTermMemoryKind::Project),
                        limit: 10,
                        ..LongTermMemoryQuery::default()
                    },
                    evidence_ref: None,
                },
                confirmation_token: Some("irrelevant".to_string()),
            },
            reason: "bulk_forget_not_allowed_on_esp".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect_err("bulk forget hidden");

    assert_eq!(err.stage(), "memory_runtime_operation");
}
