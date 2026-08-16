#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    canonical_recall_evidence_group, governed_memory_recall_candidate_id,
    long_term_version_head_key, long_term_version_material_key,
    scoped_long_term_memory_storage_key, GovernedOwnerRevisionRef, LongTermInvalidationContract,
    LongTermInvalidationReasonCode, LongTermMemoryEntry, LongTermMemoryHeadManifest,
    LongTermMemoryStaleHint, LongTermMemoryVersionMaterial, MemoryGraphNodeMembership,
    MemoryGraphScopeManifest, MEMORY_GRAPH_SCHEMA_VERSION,
};
use bm_sdk::{
    default_agent_subject_id, governed_evidence_document_content_digest, EvidenceBacklink,
    GovernedEvidenceDocumentChunk, GovernedEvidenceDocumentDraft,
    GovernedEvidenceDocumentSourceKind, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    LongTermMemoryDraft, LongTermMemoryKind, LongTermMemoryQuery, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryEvalRecallBenchmarkContext, MemoryEvalRecallRequest, MemoryEvidenceAuthority,
    MemoryEvidenceDocumentMutation, MemoryGraphEdge, MemoryGraphEdgeKind,
    MemoryGraphIntegrityMaintenanceRequest, MemoryGraphNode, MemoryGraphNodeKind,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermTarget,
    MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallReport, MemoryRecallRequest,
    MemorySemanticJudgmentSource, MemoryStoreHandle, MemorySubjectVisibilityPolicy,
    MemoryWriteCandidate, MemoryWriteRequest, ParsedLongTermMemoryExtraction, PressureLevel,
    RuntimeLifecycleModeInput, SubjectDescriptor, SubjectRegistry, TemporalMemoryGraphNodeOwnerRef,
    TemporalMemoryGraphWriteRequest, TemporalValidity,
};

use support::test_runtime;

fn draft(topic: &str, content: &str, evidence_ref: &str) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind: LongTermMemoryKind::Fact,
        topic: topic.to_string(),
        content: content.to_string(),
        keywords: vec!["graph-v2".to_string(), topic.to_string()],
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: Some("chat-1".to_string()),
        source_type: None,
        source_scope: None,
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: vec![evidence_ref.to_string()],
        canonical_entities: Vec::new(),
        evidence_count: Some(1),
        observed_at: Some(1_800_000_000),
        last_confirmed_at: Some(1_800_000_000),
        source_revision: None,
    }
}

fn tamper_delete_legacy_owner(
    platform: &MemoryStoreHandle,
    memory_space_id: &str,
    entry: &LongTermMemoryEntry,
) {
    let key =
        scoped_long_term_memory_storage_key(memory_space_id, &entry.id).expect("scoped owner key");
    let mut snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot fixture");
    snapshot
        .json_docs
        .retain(|doc| doc.namespace != "long_term" || doc.key != key);
    platform
        .replay_harness()
        .import_store_snapshot(&snapshot)
        .expect("inject corrupted snapshot fixture");
}

fn graph_node(id: &str, evidence_ref: &str) -> MemoryGraphNode {
    MemoryGraphNode {
        node_id: id.to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: format!("governed graph node {id}"),
        evidence_refs: vec![evidence_ref.to_string()],
    }
}

fn graph_edge(id: &str, from: &str, to: &str, evidence_ref: &str) -> MemoryGraphEdge {
    MemoryGraphEdge {
        edge_id: id.to_string(),
        kind: MemoryGraphEdgeKind::Supports,
        from_node_id: from.to_string(),
        to_node_id: to.to_string(),
        validity: TemporalValidity {
            valid_from: 1_800_000_000,
            valid_until: None,
            observed_at: 1_800_000_000,
            superseded_by: None,
        },
        evidence_refs: vec![evidence_ref.to_string()],
    }
}

fn long_term_node_owner(node_id: &str, owner_id: &str) -> TemporalMemoryGraphNodeOwnerRef {
    TemporalMemoryGraphNodeOwnerRef {
        node_id: node_id.to_string(),
        owner_ref: GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner_id),
    }
}

fn graph_node_owner(
    node_id: &str,
    owner_ref: GovernedMemoryOwnerRef,
) -> TemporalMemoryGraphNodeOwnerRef {
    TemporalMemoryGraphNodeOwnerRef {
        node_id: node_id.to_string(),
        owner_ref,
    }
}

fn graph_docs(platform: &MemoryStoreHandle) -> Vec<(String, String, String)> {
    let mut docs = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot")
        .json_docs
        .into_iter()
        .filter(|doc| doc.namespace.starts_with("memory_graph_"))
        .map(|doc| (doc.namespace, doc.key, doc.value.to_string()))
        .collect::<Vec<_>>();
    docs.sort();
    docs
}

fn seed_drafts(
    platform: &MemoryStoreHandle,
    runtime: &bm_sdk::MemoryRuntime,
    drafts: Vec<LongTermMemoryDraft>,
) -> Vec<bm_sdk::LongTermMemoryEntry> {
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: drafts,
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed governed owners");
    platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(usize::MAX)
        .expect("list owners")
}

fn write_shared_graph(
    runtime: &bm_sdk::MemoryRuntime,
    left_id: &str,
    right_id: &str,
) -> bm_sdk::TemporalMemoryGraphMutationReport {
    runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(left_id, "evidence:shared"),
                graph_node(right_id, "evidence:shared"),
            ],
            node_owners: vec![
                long_term_node_owner(left_id, left_id),
                long_term_node_owner(right_id, right_id),
            ],
            edges: vec![graph_edge(
                "edge:left:right",
                left_id,
                right_id,
                "evidence:shared",
            )],
            backlinks: vec![EvidenceBacklink {
                source_kind: "long_term_memory".to_string(),
                source_id: "evidence:shared".to_string(),
                fingerprint: "fp:evidence:shared".to_string(),
            }],
        })
        .expect("write graph v2")
}

fn owner_ids(
    entries: &[bm_sdk::LongTermMemoryEntry],
    left_topic: &str,
    right_topic: &str,
) -> (String, String) {
    let left = entries
        .iter()
        .find(|entry| entry_topic_matches(entry, left_topic))
        .unwrap_or_else(|| panic!("left owner {left_topic} missing from {entries:#?}"))
        .id
        .clone();
    let right = entries
        .iter()
        .find(|entry| entry_topic_matches(entry, right_topic))
        .unwrap_or_else(|| panic!("right owner {right_topic} missing from {entries:#?}"))
        .id
        .clone();
    (left, right)
}

fn entry_topic_matches(entry: &bm_sdk::LongTermMemoryEntry, expected: &str) -> bool {
    entry.topic == expected.replace(' ', "_")
}

fn recall(runtime: &bm_sdk::MemoryRuntime, query: &str) -> MemoryRecallReport {
    runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: query.to_string(),
            limit: 8,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("production recall")
}

fn assert_owner_reaches_every_graph_delivery_stage(
    recall: &MemoryRecallReport,
    owner_ref: &GovernedMemoryOwnerRef,
    content: &str,
) {
    let candidate_id = governed_memory_recall_candidate_id(owner_ref);
    assert!(recall.source_candidate_ids.contains(&candidate_id));
    assert!(recall.graph_anchor_candidate_ids.contains(&candidate_id));
    assert!(
        recall
            .facet_index_report
            .exact_facet_candidate_ids
            .contains(&candidate_id)
            || recall
                .facet_index_report
                .expanded_facet_candidate_ids
                .contains(&candidate_id)
    );
    assert!(recall
        .rank_fusion_report
        .candidate_reports
        .iter()
        .any(|candidate| candidate.candidate_id == candidate_id));
    assert!(recall
        .coverage_selection_report
        .selected_candidate_ids
        .contains(&candidate_id));
    assert!(
        recall.graph_index_report.used,
        "{:#?}",
        recall.graph_index_report
    );
    assert!(recall
        .graph_index_report
        .expanded_node_ids
        .contains(&candidate_id));
    assert!(recall.graph_rerank.candidate_ids.contains(&candidate_id));
    assert!(recall
        .graph_rerank
        .reranked_candidate_ids
        .contains(&candidate_id));
    assert!(recall
        .compact_graph
        .nodes
        .iter()
        .any(|node| node.node_id == candidate_id));
    assert!(recall
        .delivery_report
        .selected_candidate_ids
        .contains(&candidate_id));
    assert!(recall
        .delivery_report
        .selection_decisions
        .iter()
        .any(|decision| decision.candidate_id == candidate_id));
    assert!(recall
        .delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| capsule.candidate_id == candidate_id && capsule.content.contains(content)));
    assert!(recall
        .delivery_report
        .render_decisions
        .iter()
        .any(|decision| decision.candidate_id == candidate_id));
}

fn assert_owner_reaches_graph_as_neighbor(
    recall: &MemoryRecallReport,
    owner_ref: &GovernedMemoryOwnerRef,
) {
    let candidate_id = governed_memory_recall_candidate_id(owner_ref);
    assert!(
        !recall.source_candidate_ids.contains(&candidate_id),
        "the graph-neighbor control must not be a source-query hit"
    );
    assert!(
        recall.graph_index_report.used,
        "{:#?}",
        recall.graph_index_report
    );
    assert!(recall
        .graph_index_report
        .expanded_node_ids
        .contains(&candidate_id));
    assert!(recall
        .graph_rerank
        .expanded_candidate_ids
        .contains(&candidate_id));
    assert!(recall
        .graph_rerank
        .graph_neighbor_ids
        .contains(&candidate_id));
    assert!(recall
        .graph_rerank
        .reranked_candidate_ids
        .contains(&candidate_id));
    assert!(recall
        .compact_graph
        .nodes
        .iter()
        .any(|node| node.node_id == candidate_id));
}

fn assert_owner_is_exact_zero_after_graph_expansion(
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
    for candidates in [
        &recall.source_candidate_ids,
        &recall.graph_anchor_candidate_ids,
        &recall.facet_index_report.exact_facet_candidate_ids,
        &recall.facet_index_report.expanded_facet_candidate_ids,
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
        .rank_fusion_report
        .candidate_reports
        .iter()
        .any(|candidate| candidate.candidate_id == candidate_id));
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
        .any(|edge| edge.from_node_id == candidate_id || edge.to_node_id == candidate_id));
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

#[derive(Clone, Copy, Debug)]
enum GraphExclusionCase {
    Delete,
    Forget,
    Invalidate,
    Privacy,
    Supersede,
}

fn apply_graph_exclusion(
    runtime: &bm_sdk::MemoryRuntime,
    target: &LongTermMemoryEntry,
    case: GraphExclusionCase,
) {
    let operation = match case {
        GraphExclusionCase::Delete => MemoryLongTermMutation::Delete {
            target: MemoryLongTermTarget::RecordId(target.id.clone()),
        },
        GraphExclusionCase::Forget => {
            let selector = bm_sdk::MemoryLongTermSelector {
                query: LongTermMemoryQuery {
                    kind: Some(target.kind.clone()),
                    topic: Some(target.topic.clone()),
                    limit: 8,
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
                    reason: "preview graph exact-zero forget".to_string(),
                    dry_run: true,
                    mode_input: RuntimeLifecycleModeInput::default(),
                })
                .expect("forget preview");
            let confirmation_token = preview
                .policy_decision
                .confirmation_token
                .expect("forget confirmation token");
            MemoryLongTermMutation::ForgetByQuery {
                selector,
                confirmation_token: Some(confirmation_token),
            }
        }
        GraphExclusionCase::Invalidate => MemoryLongTermMutation::Invalidate {
            contract: LongTermInvalidationContract {
                target: MemoryLongTermTarget::RecordId(target.id.clone()),
                reason_code: LongTermInvalidationReasonCode::ContradictedByGovernedEvidence,
                governed_evidence_refs: vec![GovernedOwnerRevisionRef::try_new(
                    GovernedMemoryOwnerRef::new(
                        GovernedMemoryOwnerPlane::EvidenceDocument,
                        "graph-matrix-invalidation-evidence",
                    ),
                    1,
                )
                .expect("invalidation evidence ref")],
                actor_subject_id: runtime.scoped_runtime().actor_subject_id.clone(),
                audit_reason: "graph matrix invalidation".to_string(),
            },
        },
        GraphExclusionCase::Privacy => MemoryLongTermMutation::ChangePrivacy {
            target: MemoryLongTermTarget::RecordId(target.id.clone()),
            privacy: MemoryPrivacyClass::SoulPrivate,
        },
        GraphExclusionCase::Supersede => MemoryLongTermMutation::Supersede {
            target: MemoryLongTermTarget::RecordId(target.id.clone()),
            replacement: draft(
                "graph matrix replacement",
                "A new owner replaces the excluded graph target.",
                "evidence:graph-matrix-replacement",
            ),
        },
    };
    let reason = match case {
        GraphExclusionCase::Invalidate => "graph matrix invalidation".to_string(),
        _ => format!("graph exact-zero matrix {case:?}"),
    };
    let report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation,
            reason,
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("apply graph exclusion");
    assert!(report.accepted, "{case:?}: {report:#?}");
}

#[test]
fn graph_v2_write_binds_governed_owners_and_exactly_replaces_scope_closure() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform.clone(), support::host_test_profile());
    let entries = seed_drafts(
        &platform,
        &runtime,
        vec![
            draft("graph left", "Graph left owner.", "evidence:shared"),
            draft("graph right", "Graph right owner.", "evidence:shared"),
        ],
    );
    let (left_id, right_id) = owner_ids(&entries, "graph left", "graph right");

    let report = write_shared_graph(&runtime, &left_id, &right_id);
    assert!(report.accepted, "{:?}", report.gate_failures);
    assert_eq!(report.manifest_generation, Some(1));
    assert!(report
        .graph_revision
        .as_deref()
        .is_some_and(|value| !value.is_empty()));
    assert!(!report.scope_digest.is_empty());
    let transaction = report.transaction.expect("graph transaction");
    assert_eq!(transaction.operation, "memory_graph.write");
    assert!(!transaction.partial_write);

    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    for namespace in [
        "memory_graph_nodes",
        "memory_graph_edges",
        "memory_graph_backlinks",
        "memory_graph_indexes",
        "memory_graph_revisions",
        "memory_graph_manifests",
        "memory_graph_node_memberships",
        "memory_graph_edge_memberships",
        "memory_graph_backlink_memberships",
    ] {
        assert!(snapshot
            .json_docs
            .iter()
            .any(|doc| doc.namespace == namespace));
    }
    let manifest_doc = snapshot
        .json_docs
        .iter()
        .find(|doc| doc.namespace == "memory_graph_manifests")
        .expect("manifest doc");
    let manifest: MemoryGraphScopeManifest =
        serde_json::from_value(manifest_doc.value.clone()).expect("manifest v2");
    assert_eq!(manifest.schema_version, MEMORY_GRAPH_SCHEMA_VERSION);
    assert_eq!(manifest.manifest_generation, 1);
    assert_eq!(manifest.node_count, 2);

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: "graph left".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("graph v2 recall");
    assert!(
        recall.graph_index_report.used,
        "{:#?}",
        recall.graph_index_report
    );
    assert!(recall.graph_index_report.manifest_contract_verified);
    assert!(recall.graph_index_report.selected_dependency_chain_verified);
    assert!(!recall.graph_index_report.full_scope_closure_verified);

    let replacement = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![graph_node(&left_id, "evidence:left")],
            node_owners: vec![long_term_node_owner(&left_id, &left_id)],
            edges: Vec::new(),
            backlinks: vec![EvidenceBacklink {
                source_kind: "long_term_memory".to_string(),
                source_id: "evidence:left".to_string(),
                fingerprint: "fp:evidence:left".to_string(),
            }],
        })
        .expect("replace graph closure");
    assert!(replacement.accepted, "{:?}", replacement.gate_failures);
    assert_eq!(replacement.manifest_generation, Some(2));
    let replaced = graph_docs(&platform);
    assert!(!replaced
        .iter()
        .any(|(_, _, value)| value.contains(&right_id)));
    assert!(!replaced
        .iter()
        .any(|(_, _, value)| value.contains("edge:left:right")));
    assert_eq!(
        replaced
            .iter()
            .filter(|(namespace, _, _)| namespace == "memory_graph_revisions")
            .count(),
        1
    );
}

#[test]
fn stale_neighbor_reintroduced_after_control_is_exact_zero_before_graph_rerank() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform.clone(), support::host_test_profile());
    let entries = seed_drafts(
        &platform,
        &runtime,
        vec![
            draft(
                "eligible graph anchor",
                "Eligible graph anchor remains available.",
                "evidence:eligible-anchor",
            ),
            draft(
                "stale graph neighbor",
                "Stale graph neighbor must never influence recall.",
                "evidence:stale-neighbor",
            ),
        ],
    );
    let (anchor_id, stale_id) =
        owner_ids(&entries, "eligible graph anchor", "stale graph neighbor");
    runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::MarkStale {
                target: MemoryLongTermTarget::RecordId(stale_id.clone()),
                stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
            },
            reason: "graph exact-zero regression fixture".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("mark neighbor stale");

    let graph_write = write_shared_graph(&runtime, &anchor_id, &stale_id);
    assert!(graph_write.accepted, "{:?}", graph_write.gate_failures);

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: "eligible graph anchor".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("recall through persistent graph");
    let anchor_candidate = governed_memory_recall_candidate_id(&GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::LongTerm,
        anchor_id,
    ));
    let stale_candidate = governed_memory_recall_candidate_id(&GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::LongTerm,
        stale_id,
    ));

    assert!(recall.source_candidate_ids.contains(&anchor_candidate));
    assert!(
        recall.graph_index_report.used,
        "{:#?}",
        recall.graph_index_report
    );
    assert!(recall
        .graph_rerank
        .reranked_candidate_ids
        .contains(&anchor_candidate));
    assert!(!recall
        .graph_index_report
        .expanded_node_ids
        .contains(&stale_candidate));
    assert!(!recall.graph_rerank.candidate_ids.contains(&stale_candidate));
    assert!(!recall
        .graph_rerank
        .expanded_candidate_ids
        .contains(&stale_candidate));
    assert!(!recall
        .graph_rerank
        .graph_neighbor_ids
        .contains(&stale_candidate));
    assert!(!recall
        .graph_rerank
        .reranked_candidate_ids
        .contains(&stale_candidate));
    assert!(!recall
        .graph_rerank
        .score_breakdown
        .iter()
        .any(|score| score.candidate_id == stale_candidate));
    assert!(!recall
        .graph_candidate_evidence_ref_index
        .iter()
        .any(|entry| entry.candidate_id == stale_candidate));
    assert!(!recall
        .compact_graph
        .nodes
        .iter()
        .any(|node| node.node_id == stale_candidate));
    assert!(!recall.compact_graph.edges.iter().any(|edge| {
        edge.from_node_id == stale_candidate || edge.to_node_id == stale_candidate
    }));
}

#[test]
fn subject_restricted_neighbor_is_exact_zero_before_graph_expansion() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let subject_a = default_agent_subject_id("agent-a");
    let subject_b = default_agent_subject_id("agent-b");
    let mut registry =
        SubjectRegistry::single_agent_default("owner-default", "agent-a").expect("registry");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(&subject_b, "Agent B"))
        .expect("agent-b subject");
    let runtime_a = support::test_runtime_with_subject_registry(
        platform.clone(),
        "agent-a",
        &subject_a,
        "chat-graph-visibility",
        registry.clone(),
    );
    let runtime_b = support::test_runtime_with_subject_registry(
        platform.clone(),
        "agent-b",
        &subject_b,
        "chat-graph-visibility",
        registry,
    );
    let entries = seed_drafts(
        &platform,
        &runtime_a,
        vec![
            draft(
                "visible graph anchor",
                "Visible graph anchor remains available.",
                "evidence:visible-anchor",
            ),
            draft(
                "restricted graph neighbor",
                "PSV1_GRAPH_RESTRICTED_SENTINEL must never influence agent-main recall.",
                "evidence:restricted-neighbor-private",
            ),
        ],
    );
    let (anchor_id, restricted_id) = owner_ids(
        &entries,
        "visible graph anchor",
        "restricted graph neighbor",
    );
    let restricted_source_scope = entries
        .iter()
        .find(|entry| entry.id == restricted_id)
        .expect("restricted owner")
        .source_scope;
    for runtime in [&runtime_a, &runtime_b] {
        let graph_write = write_shared_graph(runtime, &anchor_id, &restricted_id);
        assert!(graph_write.accepted, "{:?}", graph_write.gate_failures);
    }
    let before_memberships = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("pre-visibility graph snapshot")
        .json_docs
        .into_iter()
        .filter(|doc| doc.namespace == "memory_graph_node_memberships")
        .map(|doc| {
            serde_json::from_value::<MemoryGraphNodeMembership>(doc.value)
                .expect("typed graph membership")
        })
        .filter(|membership| membership.owner_ref.owner_id == restricted_id)
        .map(|membership| membership.mounted_subject_id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        before_memberships,
        std::collections::BTreeSet::from([subject_a.clone(), subject_b.clone()])
    );
    runtime_a
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(restricted_id.clone()),
                source_scope: restricted_source_scope,
                subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![
                    subject_a.clone()
                ]),
            },
            reason: "graph neighbor visibility must fail closed".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("restrict graph neighbor");

    let post_memberships = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("post-visibility graph snapshot")
        .json_docs
        .into_iter()
        .filter(|doc| doc.namespace == "memory_graph_node_memberships")
        .map(|doc| {
            serde_json::from_value::<MemoryGraphNodeMembership>(doc.value)
                .expect("typed graph membership")
        })
        .collect::<Vec<_>>();
    assert!(
        post_memberships
            .iter()
            .all(|membership| membership.owner_ref.owner_id != restricted_id),
        "one visibility transaction must atomically remove every registered-subject predecessor membership"
    );
    let retained_anchor_subjects = post_memberships
        .iter()
        .filter(|membership| membership.owner_ref.owner_id == anchor_id)
        .map(|membership| membership.mounted_subject_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        retained_anchor_subjects,
        std::collections::BTreeSet::from([subject_a, subject_b.clone()]),
        "both subject graph closures must retain their visible anchor"
    );

    let recall = recall(&runtime_b, "visible graph anchor");
    let anchor_ref = GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, anchor_id);
    let restricted_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, restricted_id);
    assert!(recall
        .source_candidate_ids
        .contains(&governed_memory_recall_candidate_id(&anchor_ref)));
    assert_owner_is_exact_zero_after_graph_expansion(
        &recall,
        &restricted_ref,
        "PSV1_GRAPH_RESTRICTED_SENTINEL",
    );
}

#[test]
fn cross_subject_long_term_cascade_preserves_other_subject_private_evidence_owner() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let subject_a = default_agent_subject_id("agent-a");
    let subject_b = default_agent_subject_id("agent-b");
    let mut registry =
        SubjectRegistry::single_agent_default("owner-default", "agent-a").expect("registry");
    registry
        .upsert_subject(SubjectDescriptor::agent_persona(&subject_b, "Agent B"))
        .expect("agent-b subject");
    let runtime_a = support::test_runtime_with_subject_registry(
        platform.clone(),
        "agent-a",
        &subject_a,
        "chat-graph-evidence-visibility",
        registry.clone(),
    );
    let runtime_b = support::test_runtime_with_subject_registry(
        platform.clone(),
        "agent-b",
        &subject_b,
        "chat-graph-evidence-visibility",
        registry,
    );
    let evidence_id = "evidence:agent-b-private-graph-owner";
    let source_locator = "opaque://psv1/agent-b-private-graph-owner";
    let evidence_group = canonical_recall_evidence_group("psv1:agent-b-private-graph-owner");
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "section:private".to_string(),
        ordinal: 0,
        body: "PSV1_AGENT_B_PRIVATE_EVIDENCE_SENTINEL".to_string(),
    }];
    runtime_b
        .write(MemoryWriteRequest::GovernedEvidenceDocuments {
            mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                draft: Box::new(GovernedEvidenceDocumentDraft {
                    memory_space_id: "space:owner-default".to_string(),
                    mounted_subject_id: subject_b.clone(),
                    document_id: evidence_id.to_string(),
                    source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
                    source_locator: source_locator.to_string(),
                    canonical_evidence_group: evidence_group.clone(),
                    evidence_family_group: None,
                    source_revision: 1,
                    body: "PSV1_AGENT_B_PRIVATE_EVIDENCE_SENTINEL".to_string(),
                    content_digest: governed_evidence_document_content_digest(
                        source_locator,
                        &evidence_group,
                        None,
                        "PSV1_AGENT_B_PRIVATE_EVIDENCE_SENTINEL",
                        &chunks,
                    ),
                    chunks,
                    authority: MemoryEvidenceAuthority::WorldObservation,
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    observed_at: 1_800_000_000,
                }),
            }],
        })
        .expect("seed agent-b evidence owner");

    let entries = seed_drafts(
        &platform,
        &runtime_a,
        vec![
            draft(
                "cross graph anchor",
                "Cross-subject graph anchor remains available.",
                "evidence:cross-subject-anchor",
            ),
            draft(
                "cross graph target",
                "Cross-subject graph target becomes restricted.",
                "evidence:cross-subject-target",
            ),
        ],
    );
    let (anchor_id, target_id) = owner_ids(&entries, "cross graph anchor", "cross graph target");
    let target_scope = entries
        .iter()
        .find(|entry| entry.id == target_id)
        .expect("target owner")
        .source_scope;
    let evidence_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::EvidenceDocument, evidence_id);
    let evidence_node_id = governed_memory_recall_candidate_id(&evidence_ref);
    runtime_b
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                MemoryGraphNode {
                    node_id: evidence_node_id.clone(),
                    kind: MemoryGraphNodeKind::MemoryRecord,
                    label: "agent-b governed evidence".to_string(),
                    evidence_refs: vec![evidence_group.clone()],
                },
                graph_node(&anchor_id, "evidence:cross-subject-shared"),
                graph_node(&target_id, "evidence:cross-subject-shared"),
            ],
            node_owners: vec![
                graph_node_owner(&evidence_node_id, evidence_ref.clone()),
                long_term_node_owner(&anchor_id, &anchor_id),
                long_term_node_owner(&target_id, &target_id),
            ],
            edges: vec![graph_edge(
                "edge:cross-subject-anchor-target",
                &anchor_id,
                &target_id,
                "evidence:cross-subject-shared",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "governed_evidence_document".to_string(),
                    source_id: evidence_group,
                    fingerprint: "fp:agent-b-private-evidence".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "long_term_memory".to_string(),
                    source_id: "evidence:cross-subject-shared".to_string(),
                    fingerprint: "fp:cross-subject-shared".to_string(),
                },
            ],
        })
        .expect("write agent-b mixed-owner graph");

    runtime_a
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangeScope {
                target: MemoryLongTermTarget::RecordId(target_id.clone()),
                source_scope: target_scope,
                subject_visibility: MemorySubjectVisibilityPolicy::OnlySubjects(vec![subject_a]),
            },
            reason: "restrict shared target without deleting agent-b evidence".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("restrict target from agent-a runtime");

    let retained = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("post-cascade snapshot")
        .json_docs
        .into_iter()
        .filter(|doc| doc.namespace == "memory_graph_node_memberships")
        .map(|doc| {
            serde_json::from_value::<MemoryGraphNodeMembership>(doc.value)
                .expect("typed graph membership")
        })
        .collect::<Vec<_>>();
    assert!(retained.iter().any(|membership| {
        membership.mounted_subject_id == subject_b
            && membership.owner_ref == evidence_ref
            && membership.owner_revision == 1
    }));
    assert!(!retained.iter().any(|membership| {
        membership.mounted_subject_id == subject_b && membership.owner_ref.owner_id == target_id
    }));
    assert!(retained.iter().any(|membership| {
        membership.mounted_subject_id == subject_b && membership.owner_ref.owner_id == anchor_id
    }));
}

#[test]
fn terminal_privacy_and_supersede_graph_matrix_has_non_vacuous_positive_controls() {
    for case in [
        GraphExclusionCase::Delete,
        GraphExclusionCase::Forget,
        GraphExclusionCase::Invalidate,
        GraphExclusionCase::Privacy,
        GraphExclusionCase::Supersede,
    ] {
        let platform = support::empty_store_platform(support::host_test_profile());
        let runtime = test_runtime(platform.clone(), support::host_test_profile());
        let case_label = format!("{case:?}").to_lowercase();
        let anchor_topic = "surviving lighthouse".to_string();
        let anchor_content = "Stable lighthouse remains available.".to_string();
        let target_topic = "retired compass".to_string();
        let target_content = "Obsolete compass disappears from delivery.".to_string();
        let entries = seed_drafts(
            &platform,
            &runtime,
            vec![
                draft(
                    &anchor_topic,
                    &anchor_content,
                    &format!("evidence:graph-matrix-anchor-{case_label}"),
                ),
                draft(
                    &target_topic,
                    &target_content,
                    &format!("evidence:graph-matrix-target-{case_label}"),
                ),
            ],
        );
        let (anchor_id, target_id) = owner_ids(&entries, &anchor_topic, &target_topic);
        let anchor = entries
            .iter()
            .find(|entry| entry.id == anchor_id)
            .expect("matrix anchor");
        let target = entries
            .iter()
            .find(|entry| entry.id == target_id)
            .expect("matrix target");
        assert!(
            write_shared_graph(&runtime, &anchor_id, &target_id).accepted,
            "{case:?}"
        );

        let positive_control = recall(&runtime, &target_topic);
        assert_owner_reaches_every_graph_delivery_stage(
            &positive_control,
            &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, target.id.clone()),
            &target.content,
        );
        let graph_neighbor_control = recall(&runtime, &anchor_topic);
        assert_owner_reaches_every_graph_delivery_stage(
            &graph_neighbor_control,
            &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, anchor.id.clone()),
            &anchor.content,
        );
        assert_owner_reaches_graph_as_neighbor(
            &graph_neighbor_control,
            &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, target.id.clone()),
        );

        apply_graph_exclusion(&runtime, target, case);

        let after_target_query = recall(&runtime, &target_topic);
        assert_owner_is_exact_zero_after_graph_expansion(
            &after_target_query,
            &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, target.id.clone()),
            &target.content,
        );

        let after_anchor_query = recall(&runtime, &anchor_topic);
        assert_owner_reaches_every_graph_delivery_stage(
            &after_anchor_query,
            &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, anchor.id.clone()),
            &anchor.content,
        );
        assert_owner_is_exact_zero_after_graph_expansion(
            &after_anchor_query,
            &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, target.id.clone()),
            &target.content,
        );
    }
}

#[test]
fn typed_head_and_material_revision_drift_fail_closed_in_recall_eval_and_projection() {
    for drift in ["head", "material"] {
        let platform = support::empty_store_platform(support::host_test_profile());
        let runtime = test_runtime(platform.clone(), support::host_test_profile());
        let target_topic = format!("typed {drift} drift target");
        let target_content = format!("Typed {drift} drift must fail every production read.");
        let anchor_topic = format!("typed {drift} drift anchor");
        let entries = seed_drafts(
            &platform,
            &runtime,
            vec![
                draft(
                    &anchor_topic,
                    &format!("Typed {drift} drift anchor remains valid."),
                    &format!("evidence:typed-{drift}-drift-anchor"),
                ),
                draft(
                    &target_topic,
                    &target_content,
                    &format!("evidence:typed-{drift}-drift-target"),
                ),
            ],
        );
        let (anchor_id, target_id) = owner_ids(&entries, &anchor_topic, &target_topic);
        let target = entries
            .iter()
            .find(|entry| entry.id == target_id)
            .expect("typed drift target");
        assert!(write_shared_graph(&runtime, &anchor_id, &target_id).accepted);

        let baseline = recall(&runtime, &target_topic);
        assert_owner_reaches_every_graph_delivery_stage(
            &baseline,
            &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, target.id.clone()),
            &target.content,
        );
        let baseline_eval = runtime
            .eval_recall(MemoryEvalRecallRequest {
                query: target_topic.clone(),
                k: 8,
                include_expanded_candidates: true,
                include_graph_neighbors: true,
                include_score_breakdown: true,
                include_missing_evidence: false,
                benchmark_context: None,
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
            })
            .expect("typed drift eval positive control");
        assert!(baseline_eval.graph_index_report.used);
        let baseline_projection = runtime
            .project(MemoryProjectionRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                user_query: target_topic.clone(),
                system_max_len: 4096,
                recent_messages_limit: 4,
                pressure: PressureLevel::Normal,
                mode_input: RuntimeLifecycleModeInput::default(),
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
            })
            .expect("typed drift projection positive control");
        assert!(baseline_projection.report().audit().graph_used);
        assert!(baseline_projection
            .provider_payload()
            .system_memory_block()
            .contains(&target_content));

        let owner_ref =
            GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, target.id.clone());
        match drift {
            "head" => {
                let key = long_term_version_head_key(
                    runtime.memory_space_id(),
                    runtime.memory_space_id(),
                    &owner_ref,
                )
                .expect("head key");
                let mut head = platform
                    .replay_harness()
                    .read_json_namespace_unchecked_for_nonproduction_harness(
                        "long_term_head_manifests",
                    )
                    .expect("head docs")
                    .into_iter()
                    .find(|doc| doc.key == key)
                    .map(|doc| {
                        serde_json::from_value::<LongTermMemoryHeadManifest>(doc.value)
                            .expect("typed head")
                    })
                    .expect("target head");
                head.current_revision = head.current_revision.saturating_add(1);
                platform
                    .replay_harness()
                    .tamper_json_document_for_nonproduction_harness(
                        "long_term_head_manifests",
                        &key,
                        serde_json::to_value(head).expect("tampered head"),
                    )
                    .expect("inject head revision drift");
            }
            "material" => {
                let key = long_term_version_material_key(
                    runtime.memory_space_id(),
                    runtime.memory_space_id(),
                    &owner_ref,
                    target.owner_revision,
                )
                .expect("material key");
                let mut material = platform
                    .replay_harness()
                    .read_json_namespace_unchecked_for_nonproduction_harness(
                        "long_term_version_materials",
                    )
                    .expect("material docs")
                    .into_iter()
                    .find(|doc| doc.key == key)
                    .map(|doc| {
                        serde_json::from_value::<LongTermMemoryVersionMaterial>(doc.value)
                            .expect("typed material")
                    })
                    .expect("target material");
                material.owner_revision = material.owner_revision.saturating_add(1);
                platform
                    .replay_harness()
                    .tamper_json_document_for_nonproduction_harness(
                        "long_term_version_materials",
                        &key,
                        serde_json::to_value(material).expect("tampered material"),
                    )
                    .expect("inject material revision drift");
            }
            _ => unreachable!("fixed drift matrix"),
        }
        let expected_stage = match drift {
            "head" => "long_term_version_head_binding",
            "material" => "recall_long_term_owner_closure",
            _ => unreachable!("fixed drift matrix"),
        };

        let recall_error = runtime
            .recall(MemoryRecallRequest {
                temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                query: target_topic.clone(),
                limit: 8,
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
            })
            .expect_err("typed revision drift must fail recall");
        assert_eq!(recall_error.stage(), expected_stage);
        let eval_error = runtime
            .eval_recall(MemoryEvalRecallRequest {
                query: target_topic.clone(),
                k: 8,
                include_expanded_candidates: true,
                include_graph_neighbors: true,
                include_score_breakdown: true,
                include_missing_evidence: false,
                benchmark_context: None,
                structured_query_facets: Vec::new(),
                tool_registry_refs: Vec::new(),
            })
            .expect_err("typed revision drift must fail eval");
        assert_eq!(eval_error.stage(), expected_stage);
        let projection_error = match runtime.project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            user_query: target_topic,
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        }) {
            Ok(_) => panic!("typed revision drift must fail projection"),
            Err(error) => error,
        };
        assert_eq!(projection_error.stage(), expected_stage);
    }
}

#[test]
fn graph_v2_write_rejects_ownerless_and_private_nodes_without_partial_state() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform.clone(), support::host_test_profile());
    let mut private = draft(
        "private graph owner",
        "Private owner must never become persistent graph material.",
        "private://graph-owner",
    );
    private.privacy = MemoryPrivacyClass::SoulPrivate;
    let entries = seed_drafts(
        &platform,
        &runtime,
        vec![
            draft("visible graph owner", "Visible owner.", "evidence:visible"),
            private,
        ],
    );
    let visible_id = entries
        .iter()
        .find(|entry| entry_topic_matches(entry, "visible graph owner"))
        .expect("visible owner")
        .id
        .clone();
    let private_id = entries
        .iter()
        .find(|entry| entry_topic_matches(entry, "private graph owner"))
        .expect("private owner")
        .id
        .clone();

    for rejected_id in ["owner:missing", private_id.as_str()] {
        let report = runtime
            .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
                operation: "memory_graph.write".to_string(),
                nodes: vec![
                    graph_node(&visible_id, "evidence:visible"),
                    graph_node(rejected_id, "evidence:rejected"),
                ],
                node_owners: vec![
                    long_term_node_owner(&visible_id, &visible_id),
                    long_term_node_owner(rejected_id, rejected_id),
                ],
                edges: vec![graph_edge(
                    "edge:visible:rejected",
                    &visible_id,
                    rejected_id,
                    "evidence:rejected",
                )],
                backlinks: vec![
                    EvidenceBacklink {
                        source_kind: "long_term_memory".to_string(),
                        source_id: "evidence:visible".to_string(),
                        fingerprint: "fp:visible".to_string(),
                    },
                    EvidenceBacklink {
                        source_kind: "long_term_memory".to_string(),
                        source_id: "evidence:rejected".to_string(),
                        fingerprint: "fp:rejected".to_string(),
                    },
                ],
            })
            .expect("rejected graph write report");
        assert!(!report.accepted);
        assert!(report.transaction.is_none());
        assert!(report.gate_failures.iter().any(|failure| {
            failure == "memory_graph_persistent_node_owner_missing"
                || failure == "memory_graph_persistent_node_owner_not_visible"
        }));
        assert!(graph_docs(&platform).is_empty());
    }
}

#[test]
fn graph_node_identity_is_independent_from_its_typed_governed_owner() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform.clone(), support::host_test_profile());
    let owner = seed_drafts(
        &platform,
        &runtime,
        vec![draft(
            "independent graph owner",
            "A governed owner can back a graph node with an unrelated identity.",
            "evidence:independent-node",
        )],
    )
    .into_iter()
    .find(|entry| entry_topic_matches(entry, "independent graph owner"))
    .expect("independent graph owner");
    let node_id = "graph-node:unrelated-to-owner";
    assert_ne!(node_id, owner.id);

    let report = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![graph_node(node_id, "evidence:independent-node")],
            node_owners: vec![long_term_node_owner(node_id, &owner.id)],
            edges: Vec::new(),
            backlinks: vec![EvidenceBacklink {
                source_kind: "long_term_memory".to_string(),
                source_id: "evidence:independent-node".to_string(),
                fingerprint: "fp:independent-node".to_string(),
            }],
        })
        .expect("write graph node with independent owner identity");
    assert!(report.accepted, "{:?}", report.gate_failures);

    let membership_doc = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot")
        .json_docs
        .into_iter()
        .find(|doc| {
            doc.namespace == "memory_graph_node_memberships" && doc.value["node_id"] == node_id
        })
        .expect("independent node membership");
    let membership: MemoryGraphNodeMembership =
        serde_json::from_value(membership_doc.value).expect("node membership");
    assert_eq!(membership.node_id, node_id);
    assert_eq!(
        membership.owner_ref,
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner.id)
    );
    assert_eq!(membership.owner_revision, owner.owner_revision);

    let owner_candidate_id = governed_memory_recall_candidate_id(&membership.owner_ref);
    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "independent graph owner".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall through typed owner index");
    assert!(
        recall.graph_index_report.used,
        "{:#?}",
        recall.graph_index_report
    );
    assert!(recall
        .graph_index_report
        .source_anchor_ids
        .contains(&owner_candidate_id));
    assert!(recall
        .graph_rerank
        .reranked_candidate_ids
        .contains(&owner_candidate_id));
    assert!(!recall
        .graph_rerank
        .reranked_candidate_ids
        .iter()
        .any(|candidate_id| candidate_id == node_id));
}

#[test]
fn owner_projection_preserves_edge_only_evidence_between_same_owner_anchors() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform.clone(), support::host_test_profile());
    let owner = seed_drafts(
        &platform,
        &runtime,
        vec![draft(
            "multi anchor projection owner",
            "One governed owner may back multiple graph anchors.",
            "evidence:multi-anchor-owner",
        )],
    )
    .into_iter()
    .find(|entry| entry_topic_matches(entry, "multi anchor projection owner"))
    .expect("multi-anchor owner");
    let first_node_id = "graph-node:multi-anchor:first";
    let second_node_id = "graph-node:multi-anchor:second";
    let edge_evidence_ref = "evidence:multi-anchor-edge-only";

    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(first_node_id, "evidence:multi-anchor-owner"),
                graph_node(second_node_id, "evidence:multi-anchor-owner"),
            ],
            node_owners: vec![
                long_term_node_owner(first_node_id, &owner.id),
                long_term_node_owner(second_node_id, &owner.id),
            ],
            edges: vec![graph_edge(
                "edge:multi-anchor:self-projection",
                first_node_id,
                second_node_id,
                edge_evidence_ref,
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "long_term_memory".to_string(),
                    source_id: "evidence:multi-anchor-owner".to_string(),
                    fingerprint: "fp:multi-anchor-owner".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: edge_evidence_ref.to_string(),
                    fingerprint: "fp:multi-anchor-edge-only".to_string(),
                },
            ],
        })
        .expect("write multi-anchor graph");
    assert!(graph_write.accepted, "{:?}", graph_write.gate_failures);
    assert_eq!(graph_write.index_count, 1);

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "multi anchor projection owner".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall multi-anchor owner");
    let owner_candidate_id = governed_memory_recall_candidate_id(&GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::LongTerm,
        owner.id,
    ));
    let edge_evidence_group = bm_core::memory::canonical_recall_evidence_group(edge_evidence_ref);
    assert!(
        recall.graph_index_report.used,
        "{:#?}",
        recall.graph_index_report
    );
    assert!(recall
        .graph_candidate_evidence_ref_index
        .iter()
        .any(|entry| entry.candidate_id == owner_candidate_id
            && entry.evidence_refs.contains(&edge_evidence_group)));
}

#[test]
fn raw_legacy_owner_loss_does_not_override_typed_owner_or_mutate_graph() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform.clone(), support::host_test_profile());
    let entries = seed_drafts(
        &platform,
        &runtime,
        vec![
            draft(
                "pure read anchor",
                "Pure read anchor remains governed.",
                "evidence:shared",
            ),
            draft(
                "drifted neighbor",
                "Drifted neighbor will lose its owner.",
                "evidence:shared",
            ),
        ],
    );
    let (anchor_id, drifted_id) = owner_ids(&entries, "pure read anchor", "drifted neighbor");
    assert!(write_shared_graph(&runtime, &anchor_id, &drifted_id).accepted);

    let drifted_entry = entries
        .iter()
        .find(|entry| entry.id == drifted_id)
        .expect("drifted owner");
    tamper_delete_legacy_owner(&platform, runtime.memory_space_id(), drifted_entry);
    let graph_before_reads = graph_docs(&platform);

    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: "pure read anchor".to_string(),
            limit: 8,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("recall from typed owner");
    assert!(recall.graph_index_report.used);
    assert!(recall.graph_index_report.manifest_contract_verified);
    assert!(recall.graph_index_report.selected_dependency_chain_verified);
    assert!(!recall.graph_index_report.full_scope_closure_verified);
    assert!(!recall.graph_index_report.maintenance_required);
    assert_eq!(recall.graph_index_report.read_path_mutation_delta, 0);
    assert!(recall.graph_index_report.incident_token.is_none());
    assert_eq!(graph_docs(&platform), graph_before_reads);

    let eval = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "pure read anchor".to_string(),
            k: 8,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: false,
            benchmark_context: None,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval from typed owner");
    assert_eq!(eval.graph_index_report.read_path_mutation_delta, 0);
    assert!(eval.graph_index_report.used);
    assert!(!eval.graph_index_report.maintenance_required);
    assert_eq!(graph_docs(&platform), graph_before_reads);

    let project = runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            user_query: "pure read anchor".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project from typed owner");
    assert_eq!(project.report().audit().graph_read_path_mutation_delta, 0);
    assert!(project.report().audit().graph_used);
    assert!(!project.report().audit().graph_maintenance_required);
    assert_eq!(graph_docs(&platform), graph_before_reads);
}

#[test]
fn eval_report_canonicalizes_benchmark_locators_before_they_reach_the_public_report() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform.clone(), support::host_test_profile());
    seed_drafts(
        &platform,
        &runtime,
        vec![draft(
            "safe eval owner",
            "Safe eval owner is available without disclosing the raw benchmark locator.",
            "archive:/private/eval.md#turn=1",
        )],
    );

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "safe eval owner".to_string(),
            k: 4,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit-private-locator".to_string(),
                question_id: "private-locator-q".to_string(),
                question_type: "single_gold".to_string(),
                expected_evidence_refs: vec!["archive:/private/eval.md#turn=1".to_string()],
            }),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");
    let public_report = format!("{report:?}");
    assert!(!public_report.contains("archive:/private/eval.md#turn=1"));
    assert!(report
        .benchmark_context
        .as_ref()
        .is_some_and(|context| context
            .expected_evidence_refs
            .iter()
            .all(|reference| reference.starts_with("opaque:recall-group:sha256:"))));
}

#[test]
fn explicit_maintenance_rejects_generation_drift_without_reviving_legacy_owner_authority() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform.clone(), support::host_test_profile());
    let entries = seed_drafts(
        &platform,
        &runtime,
        vec![
            draft(
                "maintenance anchor",
                "Maintenance anchor remains governed while the stale neighbor is repaired.",
                "evidence:shared",
            ),
            draft(
                "maintenance stale",
                "Maintenance removes this stale node.",
                "evidence:shared",
            ),
        ],
    );
    let (anchor_id, stale_id) = owner_ids(&entries, "maintenance anchor", "maintenance stale");
    assert!(write_shared_graph(&runtime, &anchor_id, &stale_id).accepted);
    let before_generation_drift = graph_docs(&platform);

    let drift = runtime
        .run_graph_integrity_maintenance(MemoryGraphIntegrityMaintenanceRequest {
            expected_manifest_generation: 0,
            incident_token: None,
        })
        .expect("maintenance generation drift report");
    assert!(!drift.accepted);
    assert!(drift.transaction.is_none());
    assert!(drift
        .failures
        .contains(&"memory_graph_manifest_generation_drift".to_string()));
    assert_eq!(graph_docs(&platform), before_generation_drift);

    let legacy_entry = entries
        .iter()
        .find(|entry| entry.id == stale_id)
        .expect("legacy owner");
    tamper_delete_legacy_owner(&platform, runtime.memory_space_id(), legacy_entry);
    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            query: "maintenance anchor".to_string(),
            limit: 8,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("typed-owner recall");
    assert!(recall.graph_index_report.used);
    assert!(!recall.graph_index_report.maintenance_required);
    assert!(recall.graph_index_report.incident_token.is_none());
    assert_eq!(graph_docs(&platform), before_generation_drift);
}

#[test]
fn owner_mutation_cascade_keeps_shared_backlink_until_last_reverse_reference_is_removed() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform.clone(), support::host_test_profile());
    let left = draft("cascade left", "Cascade left owner.", "evidence:shared");
    let right = draft("cascade right", "Cascade right owner.", "evidence:shared");
    let entries = seed_drafts(&platform, &runtime, vec![left.clone(), right.clone()]);
    let (left_id, right_id) = owner_ids(&entries, "cascade left", "cascade right");
    assert!(write_shared_graph(&runtime, &left_id, &right_id).accepted);

    let mut corrected_left = left;
    corrected_left.content = "Corrected left owner invalidates its graph closure.".to_string();
    let left_report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(left_id.clone()),
                replacement: corrected_left,
            },
            reason: "graph_owner_revision_cascade".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("correct left owner");
    assert!(left_report.accepted);
    let after_left = graph_docs(&platform);
    assert!(!after_left
        .iter()
        .any(|(_, _, value)| value.contains(&left_id)));
    assert!(!after_left
        .iter()
        .any(|(_, _, value)| value.contains("edge:left:right")));
    assert!(after_left
        .iter()
        .any(|(namespace, _, _)| namespace == "memory_graph_backlinks"));
    let backlink_membership = after_left
        .iter()
        .find(|(namespace, _, _)| namespace == "memory_graph_backlink_memberships")
        .expect("retained backlink membership");
    let backlink_membership: serde_json::Value =
        serde_json::from_str(&backlink_membership.2).expect("backlink membership json");
    assert_eq!(
        backlink_membership["node_membership_keys"]
            .as_array()
            .expect("reverse node memberships")
            .len(),
        1
    );

    let mut corrected_right = right;
    corrected_right.content =
        "Corrected right owner removes the final graph reference.".to_string();
    let right_report = runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::Correct {
                target: MemoryLongTermTarget::RecordId(right_id),
                replacement: corrected_right,
            },
            reason: "graph_last_owner_revision_cascade".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("correct right owner");
    assert!(right_report.accepted);
    assert!(graph_docs(&platform).is_empty());
}

fn candidate(topic: &str, body: &str) -> MemoryWriteCandidate {
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Profile,
        topic: topic.to_string(),
    };
    MemoryWriteCandidate {
        candidate_id: format!("candidate:{topic}"),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: target.clone(),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: topic.to_string(),
            body: body.to_string(),
            keywords: vec!["graph-v2".to_string()],
        },
        evidence_refs: vec!["evidence:candidate".to_string()],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::LlmGovernance,
            decision: MemoryCandidateSemanticDecision::Accept,
            governed_target: Some(target),
            reason: "graph_v2_contract".to_string(),
        }),
    }
}

#[test]
fn candidate_and_extraction_owner_updates_cascade_graph_in_the_same_transaction() {
    let platform = support::empty_store_platform(support::host_test_profile());
    let runtime = test_runtime(platform.clone(), support::host_test_profile());
    runtime
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: None,
            candidates: vec![candidate("candidate cascade", "Initial candidate owner.")],
        })
        .expect("seed candidate owner");
    let candidate_owner = platform
        .replay_harness()
        .memory_space_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(usize::MAX)
        .expect("owners")
        .into_iter()
        .find(|entry| entry_topic_matches(entry, "candidate cascade"))
        .expect("candidate owner");
    let candidate_graph = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![graph_node(&candidate_owner.id, "evidence:candidate")],
            node_owners: vec![long_term_node_owner(
                &candidate_owner.id,
                &candidate_owner.id,
            )],
            edges: Vec::new(),
            backlinks: vec![EvidenceBacklink {
                source_kind: "long_term_memory".to_string(),
                source_id: "evidence:candidate".to_string(),
                fingerprint: "fp:candidate".to_string(),
            }],
        })
        .expect("candidate graph");
    assert!(candidate_graph.accepted);

    let candidate_update = runtime
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: None,
            candidates: vec![candidate(
                "candidate cascade",
                "Updated candidate owner removes stale graph material.",
            )],
        })
        .expect("update candidate owner");
    assert!(candidate_update.accepted);
    assert!(graph_docs(&platform).is_empty());

    let extraction = draft(
        "extraction cascade",
        "Initial extraction owner.",
        "evidence:extraction",
    );
    let entries = seed_drafts(&platform, &runtime, vec![extraction.clone()]);
    let extraction_owner = entries
        .iter()
        .find(|entry| entry_topic_matches(entry, "extraction cascade"))
        .expect("extraction owner");
    let extraction_graph = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![graph_node(&extraction_owner.id, "evidence:extraction")],
            node_owners: vec![long_term_node_owner(
                &extraction_owner.id,
                &extraction_owner.id,
            )],
            edges: Vec::new(),
            backlinks: vec![EvidenceBacklink {
                source_kind: "long_term_memory".to_string(),
                source_id: "evidence:extraction".to_string(),
                fingerprint: "fp:extraction".to_string(),
            }],
        })
        .expect("extraction graph");
    assert!(extraction_graph.accepted);

    let mut extraction_update = extraction;
    extraction_update.content =
        "Updated extraction owner removes stale graph material.".to_string();
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_update],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("update extraction owner");
    assert!(graph_docs(&platform).is_empty());
}
