#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    governed_memory_recall_candidate_id, scoped_long_term_memory_storage_key, LongTermMemoryEntry,
    MemoryGraphNodeMembership, MemoryGraphScopeManifest, MEMORY_GRAPH_SCHEMA_VERSION,
};
use bm_sdk::{
    EvidenceBacklink, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, LongTermMemoryDraft,
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvalRecallBenchmarkContext,
    MemoryEvalRecallRequest, MemoryEvidenceAuthority, MemoryGraphEdge, MemoryGraphEdgeKind,
    MemoryGraphIntegrityMaintenanceRequest, MemoryGraphNode, MemoryGraphNodeKind,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermTarget,
    MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest, MemorySemanticJudgmentSource,
    MemoryStoreHandle, MemoryWriteCandidate, MemoryWriteRequest, ParsedLongTermMemoryExtraction,
    PressureLevel, RuntimeLifecycleModeInput, TemporalMemoryGraphNodeOwnerRef,
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

fn tamper_delete_owner(
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
            extraction: ParsedLongTermMemoryExtraction {
                upserts: drafts,
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed governed owners");
    platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
fn recall_eval_and_project_fail_closed_on_owner_drift_without_graph_mutation() {
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
    tamper_delete_owner(&platform, runtime.memory_space_id(), drifted_entry);
    let graph_before_reads = graph_docs(&platform);

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "pure read anchor".to_string(),
            limit: 8,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("recall incident");
    assert!(!recall.graph_index_report.used);
    assert!(!recall.graph_index_report.manifest_contract_verified);
    assert!(!recall.graph_index_report.selected_dependency_chain_verified);
    assert!(!recall.graph_index_report.full_scope_closure_verified);
    assert!(recall.graph_index_report.maintenance_required);
    assert_eq!(recall.graph_index_report.read_path_mutation_delta, 0);
    let incident_token = recall
        .graph_index_report
        .incident_token
        .clone()
        .expect("opaque incident token");
    assert!(incident_token.starts_with("graph_incident:"));
    let safe_incident = format!("{:?}", recall.graph_index_report);
    assert!(!safe_incident.contains(&drifted_id));
    assert!(!safe_incident.contains("evidence:shared"));
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
        .expect("eval incident");
    assert_eq!(eval.graph_index_report.read_path_mutation_delta, 0);
    assert!(eval.graph_index_report.maintenance_required);
    assert_eq!(graph_docs(&platform), graph_before_reads);

    let project = runtime
        .project(MemoryProjectionRequest {
            user_query: "pure read anchor".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project incident");
    assert_eq!(project.graph_index_report.read_path_mutation_delta, 0);
    assert!(project.graph_index_report.maintenance_required);
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
fn explicit_maintenance_rejects_generation_drift_then_removes_ownerless_closure() {
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

    let stale_entry = entries
        .iter()
        .find(|entry| entry.id == stale_id)
        .expect("stale owner");
    tamper_delete_owner(&platform, runtime.memory_space_id(), stale_entry);
    let incident = runtime
        .recall(MemoryRecallRequest {
            query: "maintenance anchor".to_string(),
            limit: 8,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("maintenance incident")
        .graph_index_report
        .incident_token
        .expect("incident token");

    let repaired = runtime
        .run_graph_integrity_maintenance(MemoryGraphIntegrityMaintenanceRequest {
            expected_manifest_generation: 1,
            incident_token: Some(incident),
        })
        .expect("explicit graph maintenance");
    assert!(repaired.accepted, "{:?}", repaired.failures);
    assert!(repaired.committed);
    assert_eq!(repaired.manifest_generation, Some(2));
    assert_eq!(repaired.removed_node_count, 1);
    assert_eq!(repaired.removed_edge_count, 1);
    assert_eq!(repaired.retained_shared_backlink_count, 1);
    assert!(repaired.transaction.is_some());
    let after = graph_docs(&platform);
    assert!(!after.iter().any(|(_, _, value)| value.contains(&stale_id)));
    assert!(after
        .iter()
        .any(|(namespace, _, _)| namespace == "memory_graph_backlinks"));
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
            candidates: vec![candidate("candidate cascade", "Initial candidate owner.")],
        })
        .expect("seed candidate owner");
    let candidate_owner = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
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
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![extraction_update],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("update extraction owner");
    assert!(graph_docs(&platform).is_empty());
}
