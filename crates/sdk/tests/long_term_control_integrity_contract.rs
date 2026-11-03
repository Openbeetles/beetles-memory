mod support;
use bm_sdk::*;
use std::collections::BTreeSet;

fn policy(topic: &str) -> MemoryLongTermPolicyRequest {
    MemoryLongTermPolicyRequest {
        operation: MemoryGovernancePolicyMutation::Suppress {
            selector: MemoryGovernanceSelector {
                memory_space_id: None,
                subject_id: None,
                kind: Some(LongTermMemoryKind::Preference),
                topic_pattern: Some(topic.into()),
                source_chat_id: None,
                source_scope: None,
            },
            duration: MemoryGovernanceSuppressionDuration::UntilManualResume,
        },
        reason: "synthetic policy management".into(),
        dry_run: false,
        mode_input: Default::default(),
    }
}
fn remove(target: &MemoryLongTermGovernancePolicy) -> MemoryLongTermPolicyRequest {
    MemoryLongTermPolicyRequest {
        operation: MemoryGovernancePolicyMutation::RemoveSuppression {
            target: MemoryGovernancePolicyRef {
                policy_id: target.policy_id.clone(),
                policy_revision: target.policy_revision,
            },
        },
        reason: "synthetic exact resume".into(),
        dry_run: false,
        mode_input: Default::default(),
    }
}
fn policies(runtime: &MemoryRuntime) -> Vec<MemoryLongTermGovernancePolicy> {
    runtime
        .list_memory_governance_policies(MemoryLongTermPolicyListRequest {
            cursor: None,
            limit: 50,
        })
        .unwrap()
        .policies
        .into_iter()
        .map(|view| view.policy)
        .collect()
}
fn draft(topic: &str) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind: LongTermMemoryKind::Preference,
        topic: topic.into(),
        content: "用户喜欢温水。".into(),
        keywords: vec!["water".into()],
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: None,
        source_type: None,
        source_scope: None,
        subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
        provenance: LongTermMemoryProvenance::new(MemoryEvidenceAuthority::UserAsserted),
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: Vec::new(),
        canonical_entities: Vec::new(),
        evidence_count: None,
        observed_at: Some(1_800_000_000),
        source_revision: Some(1),
    }
}
fn write(runtime: &MemoryRuntime, topic: &str) -> MemoryWriteReport {
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![draft(topic)],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .unwrap()
}
fn lifecycle(config: StoreBackendConfig, reopen: bool) {
    let store = support::open_memory_store(config.clone()).unwrap();
    let runtime = support::test_runtime(store.clone(), support::host_test_profile());
    assert!(policies(&runtime).is_empty());
    assert!(matches!(
        runtime
            .mutate_memory_governance_policy("create-first", policy("drink"))
            .unwrap(),
        MemoryMutationExecution::Committed { .. }
    ));
    let first = policies(&runtime).pop().unwrap();
    assert_eq!(write(&runtime, "drink").changed, 0);
    assert_eq!(
        write(&runtime, "another-topic").changed,
        1,
        "structural selector is not a semantic content ban"
    );
    drop(runtime);
    let store = if reopen {
        drop(store);
        support::open_memory_store(config).unwrap()
    } else {
        store
    };
    let runtime = support::test_runtime(store, support::host_test_profile());
    assert_eq!(policies(&runtime), vec![first.clone()]);
    assert!(matches!(
        runtime
            .mutate_memory_governance_policy("create-first", policy("drink"))
            .unwrap(),
        MemoryMutationExecution::Replayed { .. }
    ));
    assert!(runtime
        .mutate_memory_governance_policy("create-first", policy("different-intent"))
        .is_err());
    assert_eq!(
        runtime
            .get_memory_governance_policy(MemoryLongTermPolicyDetailRequest {
                policy_id: first.policy_id.clone()
            })
            .unwrap()
            .policy
            .unwrap()
            .policy,
        first
    );
    assert!(matches!(
        runtime
            .mutate_memory_governance_policy("remove-first", remove(&first))
            .unwrap(),
        MemoryMutationExecution::Committed { .. }
    ));
    assert!(policies(&runtime).is_empty());
    assert_eq!(write(&runtime, "drink").changed, 1);
    runtime
        .mutate_memory_governance_policy("create-second", policy("drink"))
        .unwrap();
    let second = policies(&runtime).pop().unwrap();
    assert_ne!(first.policy_id, second.policy_id);
    assert!(runtime
        .mutate_memory_governance_policy("stale-unsubmitted-remove", remove(&first))
        .is_err());
    assert!(matches!(
        runtime
            .mutate_memory_governance_policy("remove-first", remove(&first))
            .unwrap(),
        MemoryMutationExecution::Replayed { .. }
    ));
    assert!(matches!(
        runtime
            .mutate_memory_governance_policy("create-first", policy("drink"))
            .unwrap(),
        MemoryMutationExecution::Replayed { .. }
    ));
    assert_eq!(policies(&runtime), vec![second]);
}
#[test]
fn in_memory_policy_management_reconstructs_runtime() {
    lifecycle(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
        false,
    );
}
#[test]
fn file_policy_management_reopens() {
    let root = std::env::temp_dir().join(format!("bm-lci-policy-file-{}", std::process::id()));
    lifecycle(
        StoreBackendConfig::file(&root, support::host_test_profile()).unwrap(),
        true,
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_policy_management_reopens() {
    let root = std::env::temp_dir().join(format!("bm-lci-policy-sqlite-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    lifecycle(
        StoreBackendConfig::sqlite(root.join("memory.sqlite"), support::host_test_profile())
            .unwrap(),
        true,
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn default_query_pages_visit_all_records_and_reject_stale_cursor() {
    let runtime = support::test_runtime(
        support::empty_store_platform(support::host_test_profile()),
        support::host_test_profile(),
    );
    for index in 0..9 {
        assert_eq!(write(&runtime, &format!("water-{index}")).changed, 1);
    }
    let mut cursor = None;
    let mut ids = BTreeSet::new();
    let mut first_cursor = None;
    loop {
        let page = runtime
            .list_long_term_memory(MemoryLongTermListRequest {
                query: Default::default(),
                cursor,
                limit: 2,
                view: MemoryLongTermControlView::Operator,
            })
            .unwrap();
        assert_eq!(page.total_visible, 9);
        for row in page.records {
            assert!(ids.insert(row.record.id));
        }
        if first_cursor.is_none() {
            first_cursor = page.next_cursor.clone();
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 9);
    write(&runtime, "water-new");
    assert!(runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: Default::default(),
            cursor: first_cursor,
            limit: 2,
            view: MemoryLongTermControlView::Operator
        })
        .is_err());
}

#[test]
fn same_human_policies_are_mounted_subject_scoped_and_resume_independently() {
    let store = support::empty_store_platform(support::host_test_profile());
    let human = primary_human_subject_id("owner-default");
    let mut registry = SubjectRegistry::single_agent_default("owner-default", "agent-a").unwrap();
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "B",
        ))
        .unwrap();
    let a = support::test_runtime_with_subject_registry(
        store.clone(),
        "agent-a",
        &human,
        "chat-policy",
        registry.clone(),
    );
    let b = support::test_runtime_with_subject_registry(
        store,
        "agent-b",
        &human,
        "chat-policy",
        registry,
    );
    a.mutate_memory_governance_policy("suppress-a", policy("drink"))
        .unwrap();
    let pa = policies(&a).pop().unwrap();
    assert!(policies(&b).is_empty());
    assert!(b
        .get_memory_governance_policy(MemoryLongTermPolicyDetailRequest {
            policy_id: pa.policy_id.clone()
        })
        .unwrap()
        .policy
        .is_none());
    assert_eq!(write(&a, "drink").changed, 0);
    assert_eq!(
        write(&b, "drink").changed,
        1,
        "A's policy cannot block B's independent intake"
    );
    b.mutate_memory_governance_policy("suppress-b", policy("drink"))
        .unwrap();
    let pb = policies(&b).pop().unwrap();
    assert!(a
        .mutate_memory_governance_policy("cannot-remove-b", remove(&pb))
        .is_err());
    a.mutate_memory_governance_policy("resume-a", remove(&pa))
        .unwrap();
    assert!(policies(&a).is_empty());
    assert_eq!(policies(&b), vec![pb.clone()]);
    let mut new_draft = draft("drink");
    new_draft.content = "用户偏好热茶。".into();
    new_draft.source_revision = Some(2);
    let blocked = b
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![new_draft.clone()],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .unwrap();
    assert_eq!(
        blocked.changed, 0,
        "B's independently active policy must block a real content change"
    );
    let accepted = a
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![new_draft],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .unwrap();
    assert_eq!(accepted.changed, 1);
    b.mutate_memory_governance_policy("resume-b", remove(&pb))
        .unwrap();
    assert!(policies(&b).is_empty());
}

#[cfg(feature = "nonproduction-replay-harness")]
fn commit_source(runtime: &MemoryRuntime, subject: &str, chat: &str) -> TranscriptEvidenceRef {
    let turn = CanonicalTurnDelta {
        turn_id: "identical-turn".into(),
        conversation: ConversationScope {
            channel: "llm.gateway".into(),
            chat_id: chat.into(),
            conversation_id: Some("identical-conversation".into()),
        },
        subject: subject.into(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: MemoryTurnSource {
            ingress: IngressKind::User,
            channel: "llm.gateway".into(),
            provider: None,
            protocol: MemoryTurnProtocol::OllamaChat,
            endpoint: None,
            model_alias: None,
            model_resolved: None,
            request_id: None,
            client_conversation_hint: None,
        },
        actor: None,
        input_messages: vec![TranscriptInputMessage::user("独立的合成来源")],
        assistant_message: Some(TranscriptInputMessage::assistant("收到。")),
        tool_observations: Vec::new(),
        external_content_used: false,
        candidate_ids: Vec::new(),
    };
    runtime
        .commit_transcript(MemoryTranscriptCommitRequest {
            turn,
            host_refs: Vec::new(),
        })
        .unwrap();
    TranscriptEvidenceRef {
        memory_space_id: runtime.memory_space_id().into(),
        channel_id: "llm.gateway".into(),
        conversation_id: "identical-conversation".into(),
        turn_id: "identical-turn".into(),
        message_id: None,
        subject_id: Some(subject.into()),
        authority: None,
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
fn graph_request(owner: &str, node: &str, evidence: &str) -> TemporalMemoryGraphWriteRequest {
    TemporalMemoryGraphWriteRequest {
        operation: "memory_graph.write".into(),
        nodes: vec![MemoryGraphNode {
            node_id: node.into(),
            kind: MemoryGraphNodeKind::MemoryRecord,
            label: node.into(),
            evidence_refs: vec![evidence.into()],
        }],
        node_owners: vec![TemporalMemoryGraphNodeOwnerRef {
            node_id: node.into(),
            owner_ref: GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner),
        }],
        edges: Vec::new(),
        backlinks: vec![EvidenceBacklink {
            source_kind: "long_term_memory".into(),
            source_id: evidence.into(),
            fingerprint: format!("fp:{evidence}"),
        }],
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
fn source_withdrawal_integrity(
    config: StoreBackendConfig,
    reopen: bool,
    transition: TranscriptLifecycleTransition,
) {
    let store = support::open_memory_store(config.clone()).unwrap();
    let human = primary_human_subject_id("owner-default");
    let sa = default_agent_subject_id("agent-a");
    let sb = default_agent_subject_id("agent-b");
    let mut registry = SubjectRegistry::single_agent_default("owner-default", "agent-a").unwrap();
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(&sb, "B"))
        .unwrap();
    let a = support::test_runtime_with_subject_registry(
        store.clone(),
        "agent-a",
        &human,
        "chat-source-a",
        registry.clone(),
    );
    let b = support::test_runtime_with_subject_registry(
        store.clone(),
        "agent-b",
        &human,
        "chat-source-b",
        registry.clone(),
    );
    let source_a = commit_source(&a, &sa, "chat-source-a");
    let source_b = commit_source(&b, &sb, "chat-source-b");
    assert_eq!(
        source_a.display_citation(),
        source_b.display_citation(),
        "the collision is intentional; typed subject is the authority"
    );
    let mut da = draft("alpha-tea");
    da.content = "用户偏好Alpha茶。".into();
    da.privacy = MemoryPrivacyClass::PublicRuntime;
    da.supporting_citations = vec![
        source_a.display_citation(),
        "operator:accepted-alpha".into(),
    ];
    da.subject_visibility = MemorySubjectVisibilityPolicy::OnlySubjects(vec![sa.clone()]);
    let mut db = draft("beta-tea");
    db.content = "用户偏好Beta茶。".into();
    db.privacy = MemoryPrivacyClass::PublicRuntime;
    db.supporting_citations = vec![source_b.display_citation()];
    db.subject_visibility = MemorySubjectVisibilityPolicy::OnlySubjects(vec![sb.clone()]);
    let aid = da.stable_id().unwrap();
    let bid = db.stable_id().unwrap();
    for (runtime, draft) in [(&a, da), (&b, db)] {
        assert_eq!(
            runtime
                .write(MemoryWriteRequest::LongTermExtraction {
                    extraction: ParsedLongTermMemoryExtraction {
                        upserts: vec![draft],
                        deletes: Vec::new(),
                        skill_writes: Vec::new(),
                    }
                })
                .unwrap()
                .changed,
            1
        );
    }
    let mut ga = graph_request(&aid, "node-a", &source_a.display_citation());
    let independent = graph_request(&aid, "independent-node", "operator:accepted-alpha");
    ga.nodes.extend(independent.nodes);
    ga.node_owners.extend(independent.node_owners);
    ga.backlinks.extend(independent.backlinks);
    let gb = graph_request(&bid, "node-b", &source_b.display_citation());
    a.write_temporal_memory_graph(ga.clone()).unwrap();
    b.write_temporal_memory_graph(gb).unwrap();
    let detail = |runtime: &MemoryRuntime, id: &str| {
        runtime
            .get_long_term_memory(MemoryLongTermDetailRequest {
                target: MemoryLongTermTarget::RecordId(id.into()),
                view: MemoryLongTermControlView::Operator,
            })
            .unwrap()
    };
    let a_before = detail(&a, &aid).record.unwrap();
    assert!(!a_before.supporting_citations.is_empty());
    let b_before = detail(&b, &bid).record.unwrap();
    let withdrawn_group =
        bm_core::memory::canonical_evidence_ref_from_source(&source_a.display_citation())
            .unwrap()
            .canonical_evidence_group;
    let recall_views = |runtime: &MemoryRuntime, withdrawn: bool| {
        for temporal_operation in [
            MemoryRecallTemporalOperation::Current,
            MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: a_before.updated_at,
            },
        ] {
            let recall = runtime
                .recall(MemoryRecallRequest {
                    query: a_before.content.clone(),
                    limit: 10,
                    structured_query_facets: vec![QueryFacetInput::Keyword("water".into())],
                    tool_registry_refs: Vec::new(),
                    temporal_operation,
                })
                .unwrap();
            assert!(
                recall
                    .delivery_report
                    .rendered_capsules
                    .iter()
                    .any(|capsule| capsule.owner_ref.owner_id == aid
                        && capsule.content.contains("Alpha茶")),
                "accepted fact must remain deliverable: {recall:?}"
            );
            if withdrawn {
                assert!(recall
                    .delivery_report
                    .selection_decisions
                    .iter()
                    .filter(|decision| decision
                        .owner_ref
                        .as_ref()
                        .is_some_and(|owner| owner.owner_id == aid))
                    .all(|decision| !decision
                        .canonical_evidence_groups
                        .contains(&withdrawn_group)
                        && !decision
                            .renderable_evidence_groups
                            .contains(&withdrawn_group)));
                assert!(recall
                    .delivery_report
                    .rendered_capsules
                    .iter()
                    .filter(|capsule| capsule.owner_ref.owner_id == aid)
                    .all(|capsule| !capsule.canonical_evidence_groups.contains(&withdrawn_group)));
                assert!(
                    !format!("{recall:?}").contains(&source_a.display_citation()),
                    "withdrawn evidence must not return through delivery: {recall:?}"
                );
            } else {
                assert!(recall
                    .delivery_report
                    .rendered_capsules
                    .iter()
                    .any(|capsule| capsule.owner_ref.owner_id == aid
                        && capsule.canonical_evidence_groups.contains(&withdrawn_group)));
                assert!(format!("{recall:?}").contains(&source_a.display_citation()));
            }
            let projection = runtime
                .project(MemoryProjectionRequest {
                    binding: ProceduralProjectionBindingV1::Preview,
                    temporal_operation,
                    user_query: a_before.content.clone(),
                    system_max_len: 8192,
                    recent_messages_limit: 8,
                    pressure: PressureLevel::Normal,
                    mode_input: Default::default(),
                    structured_query_facets: Vec::new(),
                    tool_registry_refs: Vec::new(),
                })
                .unwrap();
            let payload = projection.provider_payload();
            assert!(payload.system_memory_block().contains("Alpha茶"));
            if withdrawn {
                assert!(!payload
                    .system_memory_block()
                    .contains(&source_a.display_citation()));
                assert!(!payload.system_memory_block().contains("独立的合成来源"));
            }
        }
    };
    recall_views(&a, false);
    // A valid source transition without its derived effects must be rejected atomically.
    let before_withdrawal = store.replay_harness().export_store_snapshot().unwrap();
    let source_doc = before_withdrawal
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "conversation_transcript" && doc.value["subject"] == sa)
        .unwrap();
    let mut forged_source: TranscriptTurnRecord =
        serde_json::from_value(source_doc.value.clone()).unwrap();
    forged_source.apply_lifecycle_transition(transition, 1_800_000_001);
    use bm_sdk::nonproduction_replay_harness::{
        MemoryStoreEventKind, StoreEventScope, StoreMutation, StoreMutationBatch,
    };
    let incomplete = StoreMutationBatch {
        transaction_id: "synthetic-incomplete-withdrawal".into(),
        operation: "transcript.lifecycle".into(),
        scope: StoreEventScope::system("synthetic-incomplete-withdrawal")
            .with_memory_space(a.memory_space_id())
            .with_subject(&sa),
        mutations: vec![StoreMutation::PutJson {
            namespace: source_doc.namespace.clone(),
            key: source_doc.key.clone(),
            value: serde_json::to_value(forged_source).unwrap(),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "conversation_transcript".into(),
            record_key: source_doc.key.clone(),
        }],
    };
    assert!(store
        .replay_harness()
        .commit_governed_memory_transaction(incomplete)
        .is_err());
    assert_eq!(
        store.replay_harness().export_store_snapshot().unwrap(),
        before_withdrawal
    );
    a.request_transcript_lifecycle(MemoryTranscriptLifecycleRequest {
        memory_space_id: a.memory_space_id().into(),
        channel_id: "llm.gateway".into(),
        conversation_id: "identical-conversation".into(),
        turn_id: Some("identical-turn".into()),
        transition,
        reason: "synthetic factual source withdrawal".into(),
    })
    .unwrap();
    let assert_views = |a: &MemoryRuntime, b: &MemoryRuntime| {
        recall_views(a, true);
        let report = detail(a, &aid);
        assert!(report.transcript_refs.is_empty());
        let after = report.record.unwrap();
        assert_eq!(
            after.content, a_before.content,
            "accepted fact survives source withdrawal"
        );
        assert_eq!(after.owner_revision, a_before.owner_revision);
        assert_eq!(after.supporting_citations, vec!["operator:accepted-alpha"]);
        assert!(after.transcript_sources.is_empty());
        assert_eq!(
            detail(b, &bid).record.unwrap(),
            b_before,
            "another subject's colliding source remains available"
        );
    };
    assert_views(&a, &b);
    let snapshot = store.replay_harness().export_store_snapshot().unwrap();
    assert!(!snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "memory_graph_nodes" && doc.value["node_id"] == "node-a"));
    assert!(snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "memory_graph_nodes" && doc.value["node_id"] == "node-b"));
    assert!(
        snapshot
            .json_docs
            .iter()
            .any(|doc| doc.namespace == "memory_graph_nodes"
                && doc.value["node_id"] == "independent-node"),
        "independent relation survives withdrawal"
    );
    assert!(
        a.write_temporal_memory_graph(ga).is_err(),
        "current CAS must not permit revoked graph evidence to return"
    );
    assert_eq!(
        store.replay_harness().export_store_snapshot().unwrap(),
        snapshot
    );
    let mut orphan = graph_request(&aid, "independent-node", "operator:accepted-alpha");
    orphan.backlinks.push(EvidenceBacklink {
        source_kind: "long_term_memory".into(),
        source_id: source_a.display_citation(),
        fingerprint: format!("fp:{}", source_a.display_citation()),
    });
    let error = a
        .write_temporal_memory_graph(orphan)
        .expect_err("valid graph with orphan withdrawn backlink must reach Store rejection");
    let Error::Storage {
        source: Some(source),
        stage: "memory_write_transaction_commit_failed",
    } = &error
    else {
        panic!("unexpected orphan backlink rejection: {error:?}");
    };
    let cause = source.downcast_ref::<Error>().expect("typed Store cause");
    assert!(
        matches!(cause, Error::Config { stage: "long_term_source_graph", message } if message == "orphan transcript backlink has no governed consumer"),
        "{error:?}"
    );
    assert_eq!(
        store.replay_harness().export_store_snapshot().unwrap(),
        snapshot
    );
    drop(a);
    drop(b);
    let store = if reopen {
        drop(store);
        support::open_memory_store(config).unwrap()
    } else {
        store
    };
    let a = support::test_runtime_with_subject_registry(
        store.clone(),
        "agent-a",
        &human,
        "chat-source-a",
        registry.clone(),
    );
    let b = support::test_runtime_with_subject_registry(
        store,
        "agent-b",
        &human,
        "chat-source-b",
        registry,
    );
    assert_views(&a, &b);
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn source_withdrawal_keeps_colliding_subject_graph_and_fact_in_memory() {
    source_withdrawal_integrity(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
        false,
        TranscriptLifecycleTransition::Mask,
    );
}
#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn source_withdrawal_keeps_colliding_subject_graph_and_fact_after_file_reopen() {
    let root = std::env::temp_dir().join(format!("bm-lci-source-file-{}", std::process::id()));
    source_withdrawal_integrity(
        StoreBackendConfig::file(&root, support::host_test_profile()).unwrap(),
        true,
        TranscriptLifecycleTransition::Mask,
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[cfg(all(feature = "nonproduction-replay-harness", feature = "sqlite-store"))]
#[test]
fn source_withdrawal_keeps_colliding_subject_graph_and_fact_after_sqlite_reopen() {
    let root = std::env::temp_dir().join(format!("bm-lci-source-sqlite-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    source_withdrawal_integrity(
        StoreBackendConfig::sqlite(root.join("store.sqlite"), support::host_test_profile())
            .unwrap(),
        true,
        TranscriptLifecycleTransition::Mask,
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "nonproduction-replay-harness")]
#[test]
fn delete_raw_withdraws_factual_evidence_on_all_backends() {
    source_withdrawal_integrity(
        StoreBackendConfig::in_memory(support::host_test_profile()).unwrap(),
        false,
        TranscriptLifecycleTransition::DeleteRaw,
    );
    let root = std::env::temp_dir().join(format!("bm-lci-delete-{}", std::process::id()));
    source_withdrawal_integrity(
        StoreBackendConfig::file(root.join("file"), support::host_test_profile()).unwrap(),
        true,
        TranscriptLifecycleTransition::DeleteRaw,
    );
    #[cfg(feature = "sqlite-store")]
    source_withdrawal_integrity(
        StoreBackendConfig::sqlite(root.join("delete.sqlite"), support::host_test_profile())
            .unwrap(),
        true,
        TranscriptLifecycleTransition::DeleteRaw,
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn another_mounted_subject_policy_does_not_expand_through_actor() {
    let store = support::empty_store_platform(support::host_test_profile());
    let mut registry = SubjectRegistry::single_agent_default("owner-default", "agent-a").unwrap();
    let actor = default_agent_subject_id("agent-b");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(&actor, "B"))
        .unwrap();
    let actor_runtime = support::test_runtime_with_subject_registry(
        store.clone(),
        "agent-b",
        &actor,
        "actor-policy",
        registry.clone(),
    );
    let agent = support::test_runtime_with_subject_registry(
        store,
        "agent-a",
        &actor,
        "agent-policy",
        registry,
    );
    actor_runtime
        .mutate_memory_governance_policy("actor-only-policy", policy("actor-water"))
        .unwrap();
    assert_eq!(write(&actor_runtime, "actor-water").changed, 0);
    assert!(policies(&agent).is_empty());
    assert_eq!(write(&agent, "actor-water").changed, 1);
    assert_eq!(policies(&actor_runtime).len(), 1);
}

#[test]
fn human_mounted_policy_with_explicit_relationship_graph_does_not_expand_via_actor() {
    let store = support::empty_store_platform(support::host_test_profile());
    let registry = SubjectRegistry::single_agent_default("owner-default", "agent-a").unwrap();
    let human = primary_human_subject_id("owner-default");
    let human_runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-a", "owner-default").unwrap())
        .scope(MemoryScope::new("llm.gateway", "human-policy").unwrap())
        .store(store.clone())
        .subject_registry(registry.clone())
        .subject_relationship_graph(SubjectRelationshipGraph::empty(
            registry.memory_space_id.clone(),
        ))
        .scoped_runtime(SubjectScopedRuntime {
            memory_space_id: registry.memory_space_id.clone(),
            mounted_subject_id: human.clone(),
            actor_subject_id: human.clone(),
            agent_id: "agent-a".into(),
            relationship_scope: None,
            projection_policy: "subject_aware_default".into(),
            write_policy: "subject_candidate_then_space_governance".into(),
        })
        .clock(support::fixed_clock(1_800_000_000))
        .build()
        .unwrap();
    let agent = support::test_runtime_with_subject_registry(
        store,
        "agent-a",
        &human,
        "agent-policy",
        registry,
    );
    human_runtime
        .mutate_memory_governance_policy("human-only-policy", policy("human-water"))
        .unwrap();
    assert_eq!(write(&human_runtime, "human-water").changed, 0);
    assert!(policies(&agent).is_empty());
    assert_eq!(write(&agent, "human-water").changed, 1);
    assert_eq!(policies(&human_runtime).len(), 1);
}

#[test]
fn policy_directory_paginates_exactly_and_rejects_foreign_and_changed_snapshot_cursors() {
    let store = support::empty_store_platform(support::host_test_profile());
    let human = primary_human_subject_id("owner-default");
    let mut registry = SubjectRegistry::single_agent_default("owner-default", "agent-a").unwrap();
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(
            default_agent_subject_id("agent-b"),
            "B",
        ))
        .unwrap();
    let a = support::test_runtime_with_subject_registry(
        store.clone(),
        "agent-a",
        &human,
        "policy-pages",
        registry.clone(),
    );
    let b = support::test_runtime_with_subject_registry(
        store,
        "agent-b",
        &human,
        "policy-pages-b",
        registry,
    );
    for index in 0..4 {
        a.mutate_memory_governance_policy(
            format!("page-{index}"),
            policy(&format!("page-{index}")),
        )
        .unwrap();
    }
    let first = a
        .list_memory_governance_policies(MemoryLongTermPolicyListRequest {
            cursor: None,
            limit: 1,
        })
        .unwrap();
    let saved = first.next_cursor.clone().unwrap();
    let mut ids = BTreeSet::from([first.policies[0].policy.policy_id.clone()]);
    let mut cursor = first.next_cursor;
    while let Some(next) = cursor {
        let page = a
            .list_memory_governance_policies(MemoryLongTermPolicyListRequest {
                cursor: Some(next),
                limit: 1,
            })
            .unwrap();
        for row in page.policies {
            assert!(ids.insert(row.policy.policy_id));
        }
        cursor = page.next_cursor;
    }
    assert_eq!(
        ids,
        policies(&a)
            .into_iter()
            .map(|policy| policy.policy_id)
            .collect()
    );
    assert!(b
        .list_memory_governance_policies(MemoryLongTermPolicyListRequest {
            cursor: Some(saved.clone()),
            limit: 1
        })
        .is_err());
    a.mutate_memory_governance_policy("page-new", policy("page-new"))
        .unwrap();
    assert!(a
        .list_memory_governance_policies(MemoryLongTermPolicyListRequest {
            cursor: Some(saved),
            limit: 1
        })
        .is_err());
}

#[test]
fn suppression_expires_at_the_exact_boundary_without_deleting_the_rule() {
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };
    struct Clock(AtomicU64);
    impl MemoryClock for Clock {
        fn now_secs(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }
    let clock = Arc::new(Clock(AtomicU64::new(1_800_000_000)));
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").unwrap())
        .scope(MemoryScope::new("local", "expiry").unwrap())
        .store(support::empty_store_platform(support::host_test_profile()))
        .clock(clock.clone())
        .build()
        .unwrap();
    let mut request = policy("expiring-water");
    if let MemoryGovernancePolicyMutation::Suppress { duration, .. } = &mut request.operation {
        *duration = MemoryGovernanceSuppressionDuration::UntilUnixSecs(1_800_000_010);
    }
    runtime
        .mutate_memory_governance_policy("expires-exactly", request)
        .unwrap();
    clock.0.store(1_800_000_009, Ordering::SeqCst);
    assert_eq!(write(&runtime, "expiring-water").changed, 0);
    clock.0.store(1_800_000_010, Ordering::SeqCst);
    assert_eq!(write(&runtime, "expiring-water").changed, 1);
    assert_eq!(policies(&runtime).len(), 1);
}
