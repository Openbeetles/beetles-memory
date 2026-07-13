#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    scoped_long_term_control_storage_key, LongTermMemoryControlRevision, LongTermMemoryTombstone,
    LONG_TERM_CONTROL_AUDIT_NAMESPACE, LONG_TERM_CONTROL_REVISION_NAMESPACE,
    LONG_TERM_CONTROL_SCHEMA_VERSION, LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
    LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
};
use bm_core::platform::Platform as _;
use bm_sdk::nonproduction_replay_harness::StoreSnapshotJsonDoc;
use bm_sdk::{
    CanonicalTurnDelta, ConversationKey, ConversationScope, DerivedMemoryPlane, DerivedMemoryRef,
    LongTermMemoryKind, LongTermMemoryQuery, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryEvidenceAuthority, MemoryFacetOwnerPlane, MemoryGovernancePolicyMutation,
    MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration, MemoryLongTermControlView,
    MemoryLongTermDetailRequest, MemoryLongTermListRequest, MemoryLongTermMutation,
    MemoryLongTermMutationRequest, MemoryLongTermPolicyRequest, MemoryLongTermTarget,
    MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest, MemorySemanticJudgmentSource,
    MemorySubjectVisibilityPolicy, MemoryTranscriptReplayRequest, MemoryTurnDeliveryStatus,
    MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource, MemoryWriteCandidate,
    MemoryWriteRequest, ParsedLongTermMemoryExtraction, PressureLevel, ProfileId,
    RuntimeLifecycleModeInput, RuntimeLifecycleOperation, TranscriptEvidenceRef,
    TranscriptInputMessage, TranscriptReplayView,
};

use support::{StaticHttpClient, StaticLlmClient};

fn inject_scoped_control_metadata_at_store_trust_boundary(
    platform: &bm_sdk::MemoryStoreHandle,
    memory_space_id: &str,
    revision: &LongTermMemoryControlRevision,
    tombstone: &LongTermMemoryTombstone,
) {
    let revision_key = scoped_long_term_control_storage_key(
        memory_space_id,
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        &revision.revision_id,
    )
    .expect("scoped revision key");
    let tombstone_key = scoped_long_term_control_storage_key(
        memory_space_id,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        &tombstone.record_id,
    )
    .expect("scoped tombstone key");
    let mut snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("export control metadata fixture");
    snapshot.json_docs.push(StoreSnapshotJsonDoc {
        namespace: LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
        key: revision_key,
        value: serde_json::to_value(revision).expect("serialize revision"),
    });
    snapshot.json_docs.push(StoreSnapshotJsonDoc {
        namespace: LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE.to_string(),
        key: tombstone_key,
        value: serde_json::to_value(tombstone).expect("serialize tombstone"),
    });
    platform
        .replay_harness()
        .import_store_snapshot(&snapshot)
        .expect("inject scoped control metadata snapshot");
}

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

#[test]
fn runtime_lists_details_and_deletes_accepted_long_term_memory_with_audit() {
    let platform = support::seeded_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let event_reader = platform.clone();
    let runtime = support::test_runtime_with_scope(
        platform,
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
    assert!(deleted_detail.revisions.is_empty());
    assert!(deleted_detail.tombstone.is_none());
    assert!(deleted_detail.transcript_refs.is_empty());

    let tombstone = event_reader
        .replay_harness()
        .scoped_long_term_memory_control_read_store(runtime.memory_space_id())
        .expect("scoped long-term control store")
        .get_long_term_control_tombstone(&record_id)
        .unwrap();
    assert!(tombstone.is_some());
    assert!(event_reader
        .replay_harness()
        .read_events()
        .unwrap()
        .iter()
        .any(|event| event.kind_name == "operator.action"
            && event.payload.get("action").map(String::as_str)
                == Some("long_term_memory_control")));
    assert!(event_reader
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
fn long_term_control_list_and_detail_use_the_governed_runtime_view() {
    let profile = ProfileId::ServerLinuxDevFull;
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
    let raw_records = platform
        .replay_harness()
        .scoped_long_term_memory_read_store(runtime.memory_space_id())
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
    for record_id in [&private_id, &visible_id] {
        inject_scoped_control_metadata_at_store_trust_boundary(
            &platform,
            runtime.memory_space_id(),
            &LongTermMemoryControlRevision {
                schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
                revision_id: format!("known-id-revision:{record_id}"),
                record_id: record_id.clone(),
                successor_record_id: None,
                operation: "correct".to_string(),
                owner_revision: 2,
                source_revision: Some(1),
                previous_digest: "previous-digest".to_string(),
                new_digest: "new-digest".to_string(),
                reason: "known id metadata must remain governed".to_string(),
                actor_subject_id: Some("subject-a".to_string()),
                memory_space_id: Some(runtime.memory_space_id().to_string()),
                created_at: 1_800_000_001,
            },
            &LongTermMemoryTombstone {
                schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
                tombstone_id: format!("known-id-tombstone:{record_id}"),
                record_id: record_id.clone(),
                operation: "delete".to_string(),
                last_owner_revision: 2,
                last_source_revision: Some(1),
                previous_digest: "previous-digest".to_string(),
                reason: "known id tombstone must remain governed".to_string(),
                actor_subject_id: Some("subject-a".to_string()),
                memory_space_id: Some(runtime.memory_space_id().to_string()),
                created_at: 1_800_000_002,
            },
        );
    }

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
    let platform = support::empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = support::test_runtime_with_scope(
        platform,
        ProfileId::ServerLinuxDevFull,
        "local",
        "facet-control-chat",
    );

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
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
        .any(|doc| doc.owner_record_id == record_id
            && doc.action == "delete"
            && doc.facet_doc_id.starts_with("facet-owner:")
            && doc.report_view.redacted_sensitive_metadata));
    assert!(delete
        .affected_facet_docs
        .iter()
        .all(|doc| !doc.facet_doc_id.trim().is_empty()
            && doc.owner_plane == MemoryFacetOwnerPlane::LongTerm));
}

#[test]
fn runtime_dry_run_does_not_mutate_long_term_store() {
    let platform = support::seeded_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let runtime = support::test_runtime_with_scope(
        platform,
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
    let platform = support::empty_store_platform(ProfileId::ServerLinuxDevFull);
    let event_reader = platform.clone();
    let runtime = support::test_runtime_with_scope(
        platform,
        ProfileId::ServerLinuxDevFull,
        "local",
        "chat-1",
    );

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
    let policies = event_reader
        .replay_harness()
        .scoped_long_term_memory_control_read_store(runtime.memory_space_id())
        .expect("scoped long-term control store")
        .list_long_term_governance_policies(10)
        .unwrap();
    assert_eq!(policies.len(), 1);
    assert_eq!(report.policy_id, Some(policies[0].policy_id.clone()));
}

#[test]
fn runtime_tombstone_hides_record_from_recall_and_projection_context() {
    let platform = support::seeded_store_platform(ProfileId::DesktopMacosStandaloneMemory);
    let runtime = support::test_runtime_with_scope(
        platform,
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

    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Delete {
                target: MemoryLongTermTarget::RecordId(record_id),
            },
            reason: "user_deleted_seeded_project_memory".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("delete");

    let recall = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "release safety".to_string(),
            limit: 10,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    assert!(
        !format!("{:?}", recall.working).contains("Verify release artifacts before publishing."),
        "deleted long-term memory must not remain in governed recall"
    );

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "How should release safety work?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");
    assert!(
        !projection
            .system_memory_block
            .contains("Verify release artifacts before publishing."),
        "deleted long-term memory must not remain in governed projection"
    );
}

#[test]
fn runtime_suppression_policy_blocks_future_candidate_long_term_writes() {
    let platform = support::empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
            .scoped_long_term_memory_read_store("space:owner-default")
            .expect("scoped long-term read store")
            .count()
            .unwrap(),
        0
    );
}

#[test]
fn runtime_suppression_policy_blocks_long_term_extraction_writes() {
    let platform = support::empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = support::test_runtime_with_scope(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
            .scoped_long_term_memory_read_store("space:owner-default")
            .expect("scoped long-term read store")
            .count()
            .unwrap(),
        0
    );
}

#[test]
fn runtime_suppression_policy_blocks_automatic_post_turn_long_term_refresh() {
    let platform = support::empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = support::test_runtime_with_scope_and_subject(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
            .scoped_long_term_memory_read_store("space:owner-default")
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
    let platform = support::empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = support::test_runtime_with_scope_and_subject(
        platform,
        ProfileId::ServerLinuxDevFull,
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
    let platform = support::empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = support::test_runtime_with_scope_and_subject(
        platform.clone(),
        ProfileId::ServerLinuxDevFull,
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
        .list_derived_memory_refs(&key, None)
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
            .scoped_long_term_memory_read_store("space:owner-default")
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
