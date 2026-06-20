mod support;

use bm_core::platform::Platform as _;
use bm_sdk::{
    EvidenceBacklink, LongTermMemoryDraft, LongTermMemoryKind, MemoryEvalRecallBenchmarkContext,
    MemoryEvalRecallRequest, MemoryGraphEdge, MemoryGraphEdgeKind, MemoryGraphNode,
    MemoryGraphNodeKind, MemoryRecallRequest, MemoryWriteRequest, ProfileId, RuntimeSkillWrite,
    RuntimeSkillWriteSource, TemporalMemoryGraphWriteRequest, TemporalValidity,
};

use support::{empty_store_platform, test_runtime};

fn graph_node(
    node_id: &str,
    kind: MemoryGraphNodeKind,
    label: &str,
    evidence_ref: &str,
) -> MemoryGraphNode {
    MemoryGraphNode {
        node_id: node_id.to_string(),
        kind,
        label: label.to_string(),
        evidence_refs: vec![evidence_ref.to_string()],
    }
}

fn graph_edge(edge_id: &str, from: &str, to: &str, evidence_ref: &str) -> MemoryGraphEdge {
    MemoryGraphEdge {
        edge_id: edge_id.to_string(),
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

fn long_term_draft(topic: &str, content: &str, citation: &str) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind: LongTermMemoryKind::Fact,
        topic: topic.to_string(),
        content: content.to_string(),
        keywords: vec!["release".to_string()],
        source_chat_id: Some("chat-a".to_string()),
        source_type: None,
        source_scope: None,
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: vec![citation.to_string()],
        evidence_count: Some(1),
        observed_at: None,
        last_confirmed_at: None,
        source_revision: None,
    }
}

#[test]
fn eval_recall_report_separates_source_expanded_selected_and_rendered_layers() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed procedural memory");

    let baseline = runtime
        .recall(MemoryRecallRequest {
            query: "release artifact".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("baseline recall");
    assert!(baseline
        .graph_gate
        .failures
        .iter()
        .any(|failure| failure == "runtime_recall_graph_preview_not_persistent"));

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "release artifact".to_string(),
            k: 20,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4".to_string(),
                question_id: "release-q1".to_string(),
                question_type: "temporal_update".to_string(),
                expected_evidence_refs: vec![
                    "runtime_skill:runtime_skill__release_guard".to_string(),
                    "external_eval:missing-source".to_string(),
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");

    assert!(report
        .source_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == "runtime_skill__release_guard"));
    assert!(report
        .expanded_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == "runtime_skill__release_guard"));
    assert!(report
        .reranked_candidates
        .iter()
        .any(
            |candidate| candidate.candidate_id == "runtime_skill__release_guard"
                && candidate.score_breakdown.evidence_quality_score > 0
                && candidate.score_breakdown.source_authority_score > 0
        ));
    assert!(report
        .selected_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == "runtime_skill__release_guard"));
    assert!(report
        .rendered_block_preview
        .contains("runtime_skill__release_guard"));
    assert_eq!(
        report.missing_evidence_refs,
        vec!["external_eval:missing-source".to_string()]
    );
    assert_eq!(
        report
            .metrics
            .recall_at_k
            .iter()
            .map(|item| item.k)
            .collect::<Vec<_>>(),
        vec![5, 10, 20, 50]
    );
    assert!(report.metrics.any_evidence_hit);
    assert!(!report.metrics.all_evidence_hit);
    assert!(report.metrics.mrr_bps > 0);
    assert!(report.privacy_report.passed);
    assert!(report
        .graph_gate
        .failures
        .iter()
        .any(|failure| failure == "runtime_recall_graph_preview_not_persistent"));
}

#[test]
fn persistent_graph_write_expands_default_and_eval_recall_without_render_growth() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed procedural memory");

    let preview = runtime
        .recall(MemoryRecallRequest {
            query: "release artifact".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("preview recall");
    assert!(preview
        .graph_gate
        .failures
        .iter()
        .any(|failure| failure == "runtime_recall_graph_preview_not_persistent"));
    assert!(!preview
        .graph_rerank
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == "graph:release_manifest_check"));

    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    "runtime_skill__release_guard",
                    MemoryGraphNodeKind::Procedure,
                    "Release artifact guard",
                    "runtime_skill:runtime_skill__release_guard",
                ),
                graph_node(
                    "graph:release_manifest_check",
                    MemoryGraphNodeKind::Task,
                    "Release manifest check",
                    "turn:release-manifest",
                ),
            ],
            edges: vec![graph_edge(
                "edge:release_guard:manifest_check",
                "runtime_skill__release_guard",
                "graph:release_manifest_check",
                "turn:release-manifest",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "procedural_memory".to_string(),
                    source_id: "runtime_skill:runtime_skill__release_guard".to_string(),
                    fingerprint: "fp-runtime-skill-release-guard".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: "turn:release-manifest".to_string(),
                    fingerprint: "fp-release-manifest".to_string(),
                },
            ],
        })
        .expect("graph write report");
    assert!(graph_write.accepted);
    assert!(graph_write.gate_failures.is_empty());

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "release artifact".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("persistent graph recall");
    assert!(recall.graph_gate.high_confidence_projection_allowed);
    assert!(!recall
        .graph_gate
        .failures
        .iter()
        .any(|failure| failure == "runtime_recall_graph_preview_not_persistent"));
    assert!(recall
        .graph_rerank
        .candidate_ids
        .iter()
        .any(|candidate| candidate == "runtime_skill__release_guard"));
    assert!(recall
        .graph_rerank
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == "graph:release_manifest_check"));
    assert!(recall
        .graph_rerank
        .graph_neighbor_ids
        .iter()
        .any(|candidate| candidate == "graph:release_manifest_check"));
    assert!(recall.compact_graph.nodes.iter().any(|node| node.node_id
        == "graph:release_manifest_check"
        && node
            .evidence_refs
            .iter()
            .any(|evidence_ref| evidence_ref == "turn:release-manifest")));

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "release artifact".to_string(),
            k: 20,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4".to_string(),
                question_id: "release-q2".to_string(),
                question_type: "temporal_update".to_string(),
                expected_evidence_refs: vec!["turn:release-manifest".to_string()],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");

    assert!(report
        .expanded_candidates
        .iter()
        .any(
            |candidate| candidate.candidate_id == "graph:release_manifest_check"
                && candidate
                    .graph_neighbor_ids
                    .iter()
                    .any(|neighbor| neighbor == "runtime_skill__release_guard")
        ));
    assert!(report
        .reranked_candidates
        .iter()
        .any(
            |candidate| candidate.candidate_id == "graph:release_manifest_check"
                && candidate
                    .evidence_refs
                    .iter()
                    .any(|evidence_ref| evidence_ref == "turn:release-manifest")
        ));
    assert!(report.metrics.any_evidence_hit);
    assert!(report.metrics.all_evidence_hit);
    assert!(report.missing_evidence_refs.is_empty());
    assert!(!report
        .source_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == "graph:release_manifest_check"));
    assert!(!report
        .rendered_block_preview
        .contains("Release manifest check"));
}

#[test]
fn eval_recall_reports_w41_diagnostics_without_expanding_prompt_pool() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed procedural memory");

    runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    "runtime_skill__release_guard",
                    MemoryGraphNodeKind::Procedure,
                    "Release artifact guard",
                    "runtime_skill:runtime_skill__release_guard",
                ),
                graph_node(
                    "graph:release_manifest_check",
                    MemoryGraphNodeKind::Task,
                    "Release manifest check",
                    "turn:release-manifest",
                ),
            ],
            edges: vec![graph_edge(
                "edge:release_guard:manifest_check",
                "runtime_skill__release_guard",
                "graph:release_manifest_check",
                "turn:release-manifest",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "procedural_memory".to_string(),
                    source_id: "runtime_skill:runtime_skill__release_guard".to_string(),
                    fingerprint: "fp-runtime-skill-release-guard".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: "turn:release-manifest".to_string(),
                    fingerprint: "fp-release-manifest".to_string(),
                },
            ],
        })
        .expect("graph write");

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "release artifact".to_string(),
            k: 1,
            include_expanded_candidates: true,
            include_graph_neighbors: false,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w41".to_string(),
                question_id: "release-w41-q".to_string(),
                question_type: "temporal_update".to_string(),
                expected_evidence_refs: vec!["turn:release-manifest".to_string()],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");

    assert!(report
        .graph_anchor_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == "runtime_skill__release_guard"));
    assert!(report
        .eval_candidate_pool
        .iter()
        .any(|candidate| candidate.candidate_id == "graph:release_manifest_check"));
    assert!(report
        .evidence_ref_index
        .iter()
        .any(|entry| entry.candidate_id == "graph:release_manifest_check"
            && entry
                .evidence_refs
                .iter()
                .any(|evidence_ref| evidence_ref == "turn:release-manifest")));

    let rendered_candidate_ids = report
        .rendered_candidates
        .iter()
        .map(|candidate| candidate.candidate_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(rendered_candidate_ids.len(), 1);
    assert!(report.eval_candidate_pool.len() > report.rendered_candidates.len());
    assert!(report.eval_candidate_pool.iter().any(|candidate| {
        !rendered_candidate_ids
            .iter()
            .any(|rendered| rendered == &candidate.candidate_id)
    }));
    assert!(!report
        .rendered_block_preview
        .contains("Release manifest check"));

    let diagnostics = &report.stage_diagnostics;
    assert_eq!(diagnostics.suite, "unit_w41");
    assert_eq!(diagnostics.question_id, "release-w41-q");
    assert_eq!(diagnostics.question_type, "temporal_update");
    assert_eq!(diagnostics.evidence_count, 1);
    assert_eq!(
        diagnostics.gold_evidence_refs,
        vec!["turn:release-manifest".to_string()]
    );
    assert_eq!(diagnostics.first_any_hit_stage.as_deref(), Some("expanded"));
    assert_eq!(diagnostics.first_all_hit_stage.as_deref(), Some("expanded"));
    assert!(!diagnostics.miss_after_expanded);
    assert!(diagnostics
        .matched_gold_by_stage
        .iter()
        .any(|stage| stage.stage == "expanded"
            && stage
                .evidence_refs
                .iter()
                .any(|evidence_ref| evidence_ref == "turn:release-manifest")));
    assert!(diagnostics
        .missing_gold_by_stage
        .iter()
        .any(|stage| stage.stage == "source"
            && stage
                .evidence_refs
                .iter()
                .any(|evidence_ref| evidence_ref == "turn:release-manifest")));
    assert!(diagnostics
        .gold_rank_by_stage
        .iter()
        .any(|rank| rank.stage == "expanded"
            && rank.evidence_ref == "turn:release-manifest"
            && rank.rank == Some(2)));
    assert!(diagnostics
        .graph_distance_to_gold
        .iter()
        .any(
            |distance| distance.candidate_id == "graph:release_manifest_check"
                && distance.evidence_ref == "turn:release-manifest"
                && distance.distance == Some(1)
        ));
    assert!(diagnostics
        .source_anchor_ids
        .iter()
        .any(|candidate| candidate == "runtime_skill__release_guard"));
    assert!(diagnostics
        .graph_anchor_candidate_ids
        .iter()
        .any(|candidate| candidate == "runtime_skill__release_guard"));
    assert!(diagnostics
        .expanded_node_ids
        .iter()
        .any(|candidate| candidate == "graph:release_manifest_check"));
    assert!(diagnostics
        .graph_neighbor_ids
        .iter()
        .any(|candidate| candidate == "graph:release_manifest_check"));
    assert_eq!(diagnostics.truncated_count, 0);
    assert!(diagnostics.blocked_reasons.is_empty());
    assert_eq!(diagnostics.selected_candidate_ids, rendered_candidate_ids);
    assert_eq!(diagnostics.rendered_candidate_ids, rendered_candidate_ids);
    assert_eq!(
        diagnostics.rendered_evidence_refs,
        report
            .rendered_candidates
            .iter()
            .flat_map(|candidate| candidate.evidence_refs.iter().cloned())
            .collect::<Vec<_>>()
    );
}

#[test]
fn eval_recall_uses_wider_hybrid_graph_anchor_pool_without_render_growth() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);
    let store = platform.long_term_memory_store();

    let topics = [
        (
            "release target acme manifest exception",
            "release target acme manifest exception is only useful as a graph anchor",
            "external_eval:target-anchor",
        ),
        (
            "release canary checklist",
            "release canary checklist stays in the prompt source set",
            "external_eval:canary",
        ),
        (
            "release package checksum",
            "release package checksum stays in the prompt source set",
            "external_eval:checksum",
        ),
        (
            "release customer notice",
            "release customer notice stays in the prompt source set",
            "external_eval:notice",
        ),
        (
            "release owner handoff",
            "release owner handoff stays in the prompt source set",
            "external_eval:handoff",
        ),
        (
            "release rollback drill",
            "release rollback drill stays in the prompt source set",
            "external_eval:rollback",
        ),
        (
            "release audit packet",
            "release audit packet stays in the prompt source set",
            "external_eval:audit",
        ),
    ];
    for (offset, (topic, content, citation)) in topics.iter().enumerate() {
        store
            .upsert_many(
                &[long_term_draft(topic, content, citation)],
                1_800_000_000 + offset as u64,
            )
            .expect("seed long-term memory");
    }
    let target_anchor_id = store
        .list(20)
        .expect("list long-term")
        .into_iter()
        .find(|entry| entry.topic == "release_target_acme_manifest_exception")
        .expect("target anchor")
        .id;

    runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &target_anchor_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release target acme manifest exception",
                    "external_eval:target-anchor",
                ),
                graph_node(
                    "graph:release_target_gold_evidence",
                    MemoryGraphNodeKind::Task,
                    "Release target gold evidence",
                    "external_eval:target-gold",
                ),
            ],
            edges: vec![graph_edge(
                "edge:release-target-anchor:gold",
                &target_anchor_id,
                "graph:release_target_gold_evidence",
                "external_eval:target-gold",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "accepted_long_term_revision".to_string(),
                    source_id: "external_eval:target-anchor".to_string(),
                    fingerprint: "fp-target-anchor".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: "external_eval:target-gold".to_string(),
                    fingerprint: "fp-target-gold".to_string(),
                },
            ],
        })
        .expect("graph write");

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "release".to_string(),
            k: 5,
            include_expanded_candidates: true,
            include_graph_neighbors: false,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w41".to_string(),
                question_id: "release-anchor-pool-q".to_string(),
                question_type: "temporal_update".to_string(),
                expected_evidence_refs: vec!["external_eval:target-gold".to_string()],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");

    let source_ids = report
        .source_candidates
        .iter()
        .map(|candidate| candidate.candidate_id.clone())
        .collect::<Vec<_>>();
    assert!(
        !source_ids
            .iter()
            .any(|candidate| candidate == &target_anchor_id),
        "target anchor should stay outside prompt source pool: {source_ids:?}"
    );
    assert!(report
        .graph_anchor_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == target_anchor_id));
    assert!(report
        .eval_candidate_pool
        .iter()
        .any(
            |candidate| candidate.candidate_id == "graph:release_target_gold_evidence"
                && candidate
                    .evidence_refs
                    .iter()
                    .any(|evidence_ref| evidence_ref == "external_eval:target-gold")
        ));
    assert_eq!(
        report.stage_diagnostics.first_any_hit_stage.as_deref(),
        Some("expanded")
    );
    assert!(!report
        .rendered_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == target_anchor_id
            || candidate.candidate_id == "graph:release_target_gold_evidence"));
    assert!(report.graph_index_report.used);
    assert!(!report.graph_index_report.fallback_full_scan);
}

#[test]
fn persistent_graph_recall_uses_sdk_owned_production_index_report() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);

    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed procedural memory");

    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    "runtime_skill__release_guard",
                    MemoryGraphNodeKind::Procedure,
                    "Release artifact guard",
                    "runtime_skill:runtime_skill__release_guard",
                ),
                graph_node(
                    "graph:release_manifest_check",
                    MemoryGraphNodeKind::Task,
                    "Release manifest check",
                    "turn:release-manifest",
                ),
            ],
            edges: vec![graph_edge(
                "edge:release_guard:manifest_check",
                "runtime_skill__release_guard",
                "graph:release_manifest_check",
                "turn:release-manifest",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "procedural_memory".to_string(),
                    source_id: "runtime_skill:runtime_skill__release_guard".to_string(),
                    fingerprint: "fp-runtime-skill-release-guard".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: "turn:release-manifest".to_string(),
                    fingerprint: "fp-release-manifest".to_string(),
                },
            ],
        })
        .expect("graph write report");

    assert!(graph_write.accepted);
    assert_eq!(graph_write.index_count, 2);
    assert!(graph_write.index_revision.is_some());

    let snapshot = platform.export_store_snapshot().expect("snapshot");
    assert!(snapshot.json_docs.iter().any(|doc| {
        doc.namespace == "memory_graph_indexes"
            && doc.value["owner"] == "bm-sdk::MemoryRuntime"
            && doc.value["source_anchor_id"] == "runtime_skill__release_guard"
            && doc.value["neighbor_node_ids"]
                .as_array()
                .is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item == "graph:release_manifest_check")
                })
    }));

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "release artifact".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("indexed recall");

    assert!(recall.graph_index_report.used);
    assert_eq!(recall.graph_index_report.owner, "bm-sdk::MemoryRuntime");
    assert!(!recall.graph_index_report.fallback_full_scan);
    assert!(recall
        .graph_index_report
        .source_anchor_ids
        .iter()
        .any(|source| source == "runtime_skill__release_guard"));
    assert!(recall
        .graph_index_report
        .expanded_node_ids
        .iter()
        .any(|node| node == "graph:release_manifest_check"));

    let eval = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "release artifact".to_string(),
            k: 20,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4".to_string(),
                question_id: "release-index-q".to_string(),
                question_type: "temporal_update".to_string(),
                expected_evidence_refs: vec!["turn:release-manifest".to_string()],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("indexed eval recall");

    assert!(eval.graph_index_report.used);
    assert_eq!(
        eval.graph_index_report.source_anchor_ids,
        recall.graph_index_report.source_anchor_ids
    );
    assert_eq!(
        eval.graph_index_report.expanded_node_ids,
        recall.graph_index_report.expanded_node_ids
    );
    assert!(eval.metrics.any_evidence_hit);
}

#[test]
fn large_persistent_graph_index_report_explains_anchor_and_expansion_coverage() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed procedural memories");

    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    "runtime_skill__release_guard",
                    MemoryGraphNodeKind::Procedure,
                    "Release artifact guard",
                    "runtime_skill:runtime_skill__release_guard",
                ),
                graph_node(
                    "graph:release_manifest_check",
                    MemoryGraphNodeKind::Task,
                    "Release manifest check",
                    "turn:release-manifest",
                ),
                graph_node(
                    "graph:release_policy_check",
                    MemoryGraphNodeKind::Task,
                    "Release policy check",
                    "turn:release-policy",
                ),
                graph_node(
                    "graph:unrelated_audit",
                    MemoryGraphNodeKind::Task,
                    "Unrelated audit",
                    "turn:unrelated-audit",
                ),
                graph_node(
                    "graph:unrelated_receipt",
                    MemoryGraphNodeKind::Task,
                    "Unrelated receipt",
                    "turn:unrelated-receipt",
                ),
            ],
            edges: vec![
                graph_edge(
                    "edge:release_guard:manifest_check",
                    "runtime_skill__release_guard",
                    "graph:release_manifest_check",
                    "turn:release-manifest",
                ),
                graph_edge(
                    "edge:release_guard:policy_check",
                    "runtime_skill__release_guard",
                    "graph:release_policy_check",
                    "turn:release-policy",
                ),
                graph_edge(
                    "edge:unrelated:audit_receipt",
                    "graph:unrelated_audit",
                    "graph:unrelated_receipt",
                    "turn:unrelated-audit",
                ),
            ],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "procedural_memory".to_string(),
                    source_id: "runtime_skill:runtime_skill__release_guard".to_string(),
                    fingerprint: "fp-runtime-skill-release-guard".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: "turn:release-manifest".to_string(),
                    fingerprint: "fp-release-manifest".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: "turn:release-policy".to_string(),
                    fingerprint: "fp-release-policy".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: "turn:unrelated-audit".to_string(),
                    fingerprint: "fp-unrelated-audit".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: "turn:unrelated-receipt".to_string(),
                    fingerprint: "fp-unrelated-receipt".to_string(),
                },
            ],
        })
        .expect("graph write report");
    assert!(graph_write.accepted);

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "release artifact".to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .expect("indexed recall");

    let index = &recall.graph_index_report;
    assert_eq!(index.owner, "bm-sdk::MemoryRuntime");
    assert!(index.used, "{index:#?}");
    assert!(!index.fallback_full_scan);
    assert_eq!(index.source_candidate_count, 1);
    assert_eq!(index.matched_source_anchor_count, 1);
    assert!(index
        .source_anchor_ids
        .iter()
        .any(|source| source == "runtime_skill__release_guard"));
    assert!(index.unmatched_source_anchor_ids.is_empty());
    assert_eq!(index.indexed_neighbor_count, 2);
    assert_eq!(index.filtered_node_count, 3);
    assert_eq!(index.filtered_edge_count, 2);
    assert_eq!(index.filtered_backlink_count, 3);

    let eval = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "release artifact".to_string(),
            k: 20,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4".to_string(),
                question_id: "release-large-index-q".to_string(),
                question_type: "temporal_update".to_string(),
                expected_evidence_refs: vec!["turn:release-policy".to_string()],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("indexed eval recall");

    assert_eq!(
        eval.graph_index_report.source_candidate_count,
        index.source_candidate_count
    );
    assert_eq!(
        eval.graph_index_report.unmatched_source_anchor_ids,
        index.unmatched_source_anchor_ids
    );
    assert_eq!(
        eval.graph_index_report.filtered_node_count,
        index.filtered_node_count
    );
}

#[test]
fn persistent_graph_recall_fails_closed_when_loaded_graph_exceeds_profile_budget() {
    let platform = empty_store_platform(ProfileId::EspEmbeddedSdk);
    let runtime = test_runtime(platform, ProfileId::EspEmbeddedSdk);

    runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["operator accepted".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("seed procedural memory");

    let mut nodes = vec![graph_node(
        "runtime_skill__release_guard",
        MemoryGraphNodeKind::Procedure,
        "Release artifact guard",
        "runtime_skill:runtime_skill__release_guard",
    )];
    let mut edges = Vec::new();
    let mut backlinks = vec![EvidenceBacklink {
        source_kind: "procedural_memory".to_string(),
        source_id: "runtime_skill:runtime_skill__release_guard".to_string(),
        fingerprint: "fp-runtime-skill-release-guard".to_string(),
    }];
    for index in 0..32 {
        let node_id = format!("graph:release_budget_overflow_{index}");
        let evidence_ref = format!("turn:release-budget-overflow-{index}");
        nodes.push(graph_node(
            &node_id,
            MemoryGraphNodeKind::Task,
            &format!("Release budget overflow {index}"),
            &evidence_ref,
        ));
        edges.push(graph_edge(
            &format!("edge:release_guard:budget_overflow_{index}"),
            "runtime_skill__release_guard",
            &node_id,
            &evidence_ref,
        ));
        backlinks.push(EvidenceBacklink {
            source_kind: "conversation_transcript".to_string(),
            source_id: evidence_ref,
            fingerprint: format!("fp-release-budget-overflow-{index}"),
        });
    }

    runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes,
            edges,
            backlinks,
        })
        .expect("graph write report");

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "release artifact".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("bounded graph recall");

    assert!(!recall.graph_gate.high_confidence_projection_allowed);
    assert!(recall
        .graph_gate
        .failures
        .iter()
        .any(|failure| failure == "memory_graph_nodes_loaded_budget_exceeded"));
    assert!(recall
        .graph_index_report
        .failures
        .iter()
        .any(|failure| failure == "memory_graph_nodes_loaded_budget_exceeded"));
    assert!(recall.graph_index_report.filtered_node_count <= 32);
}
