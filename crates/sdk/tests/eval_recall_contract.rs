mod support;

use bm_core::platform::Platform as _;
use bm_sdk::{
    EvidenceBacklink, LongTermMemoryDraft, LongTermMemoryKind, MemoryEvalRecallBenchmarkContext,
    MemoryEvalRecallRequest, MemoryGraphEdge, MemoryGraphEdgeKind, MemoryGraphNode,
    MemoryGraphNodeKind, MemoryRecallRequest, MemoryWriteRequest, ProfileId, RuntimeSkillWrite,
    RuntimeSkillWriteSource, TemporalMemoryGraphWriteRequest, TemporalValidity,
};

use support::{empty_store_platform, test_runtime, test_runtime_with_scope_subject_and_budget};

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
fn eval_recall_reports_facet_stage_for_expanded_miss() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);
    let store = platform.long_term_memory_store();

    store
        .upsert_many(
            &[long_term_draft(
                "release baseline source",
                "release baseline source is visible without facet expansion",
                "external_eval:baseline-source",
            )],
            1_800_000_000,
        )
        .expect("seed long-term memory");

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "release missing facet target".to_string(),
            k: 5,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4_facet".to_string(),
                question_id: "facet-expanded-miss".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec!["external_eval:facet-only-gold".to_string()],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");

    let facet_stage = &report.stage_diagnostics.facet_stage;
    assert!(report.stage_diagnostics.miss_after_expanded);
    assert!(facet_stage.miss_after_expanded);
    assert!(facet_stage
        .expanded_missing_evidence_refs
        .iter()
        .any(|evidence_ref| evidence_ref == "external_eval:facet-only-gold"));
    assert!(report.facet_index_report.report_only);
    assert!(!report.facet_index_report.used);
    assert!(!report.facet_index_report.fallback_full_scan);
    assert!(facet_stage
        .blocked_reasons
        .iter()
        .any(|reason| reason == "memory_facet_index_not_loaded"));

    let required = ["facet_off", "rank_fusion_off", "coverage_selection_off"];
    assert_eq!(report.ablation_report.method, "sdk_eval_recall_off_run_v1");
    for name in required {
        assert!(report
            .ablation_report
            .slices
            .iter()
            .any(|slice| slice.name == name && slice.report_available && !slice.feature_enabled));
    }
    assert_eq!(report.ablation_report.render_growth, 0);
    assert_eq!(
        report.stage_diagnostics.ablation_report,
        report.ablation_report
    );
    assert_eq!(
        facet_stage.rendered_candidate_count,
        report.rendered_candidates.len()
    );
}

#[test]
fn facet_recall_expands_graph_anchor_pool_without_render_growth() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: bm_sdk::ParsedLongTermMemoryExtraction {
                upserts: vec![
                    long_term_draft(
                        "release/facet/baseline",
                        "Release baseline remains visible through ordinary source recall.",
                        "external_eval:baseline-source",
                    ),
                    long_term_draft(
                        "release/facet/rare-target",
                        "Rare target is discoverable through the governed facet hierarchy.",
                        "external_eval:facet-only-gold",
                    ),
                ],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed governed facet index docs");

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "release facet".to_string(),
            k: 1,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4_facet".to_string(),
                question_id: "facet-p3-anchor-pool".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec![
                    "external_eval:baseline-source".to_string(),
                    "external_eval:facet-only-gold".to_string(),
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");

    let facet_owner_ids = report
        .facet_index_report
        .exact_facet_candidate_ids
        .iter()
        .chain(
            report
                .facet_index_report
                .expanded_facet_candidate_ids
                .iter(),
        )
        .cloned()
        .collect::<Vec<_>>();
    assert!(report.facet_index_report.used);
    assert!(!report.facet_index_report.report_only);
    assert!(!report.facet_index_report.fallback_full_scan);
    assert_eq!(report.facet_index_report.failures, Vec::<String>::new());
    assert!(report.facet_index_report.exact_facet_doc_count > 0);
    assert!(report.facet_index_report.expanded_facet_doc_count > 0);
    assert!(facet_owner_ids.iter().any(|candidate_id| report
        .graph_anchor_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == *candidate_id)));
    assert!(facet_owner_ids.iter().any(|candidate_id| report
        .eval_candidate_pool
        .iter()
        .any(|candidate| candidate.candidate_id == *candidate_id)));
    assert!(report.rendered_candidates.iter().all(|candidate| report
        .source_candidates
        .iter()
        .any(|source| source.candidate_id == candidate.candidate_id)));
    assert!(report.rendered_candidates.len() <= report.source_candidates.len());
    assert!(report.facet_index_report.render_growth == 0);
    assert!(report.rank_fusion_report.used);
    assert!(report
        .rank_fusion_report
        .candidate_reports
        .iter()
        .any(|candidate| candidate.facet_rank.is_some()));
    assert!(report.coverage_selection_report.used);
    assert!(report
        .coverage_selection_report
        .selected_candidate_ids
        .iter()
        .all(|candidate_id| report
            .graph_anchor_candidates
            .iter()
            .any(|candidate| candidate.candidate_id == *candidate_id)));
    assert!(report.budget_report.facet_recall_budget.max_query_facets > 0);
    assert!(
        report
            .budget_report
            .facet_recall_budget
            .max_facet_index_docs_read
            > 0
    );
    assert!(
        report
            .budget_report
            .facet_recall_budget
            .max_facet_anchor_candidates
            > 0
    );
    assert!(
        report
            .budget_report
            .facet_recall_budget
            .max_facet_expanded_candidates
            > 0
    );
    assert_eq!(
        report
            .stage_diagnostics
            .facet_stage
            .rendered_candidate_count,
        report.rendered_candidates.len()
    );
}

#[test]
fn facet_recall_respects_privacy_scope_and_profile_budget() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    budget.facet_recall_budget.max_facet_anchor_candidates = 1;
    budget.facet_recall_budget.max_facet_expanded_candidates = 1;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform.clone(),
        profile,
        "llm.gateway",
        "facet-budget",
        "subject-alpha",
        budget.clone(),
    );

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: bm_sdk::ParsedLongTermMemoryExtraction {
                upserts: vec![
                    long_term_draft(
                        "facet/budget/alpha-one",
                        "Alpha one should match facet budget recall.",
                        "external_eval:facet-budget-alpha-one",
                    ),
                    long_term_draft(
                        "facet/budget/alpha-two",
                        "Alpha two should be budget truncated from facet recall.",
                        "external_eval:facet-budget-alpha-two",
                    ),
                    long_term_draft(
                        "facet/budget/alpha-three",
                        "Alpha three should also be budget truncated from facet recall.",
                        "external_eval:facet-budget-alpha-three",
                    ),
                ],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed subject-alpha facet docs");

    let alpha = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "facet budget".to_string(),
            k: 5,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4_facet".to_string(),
                question_id: "facet-p3-budget".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec![
                    "external_eval:facet-budget-alpha-one".to_string(),
                    "external_eval:facet-budget-alpha-two".to_string(),
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("alpha eval recall");

    assert!(alpha.facet_index_report.used);
    assert!(alpha.facet_index_report.exact_facet_candidate_ids.len() <= 1);
    assert!(alpha.facet_index_report.expanded_facet_candidate_ids.len() <= 1);
    assert!(alpha.facet_index_report.failures.iter().any(|failure| {
        failure == "memory_facet_anchor_candidates_budget_truncated"
            || failure == "memory_facet_expanded_candidates_budget_truncated"
    }));
    assert_eq!(alpha.facet_index_report.render_growth, 0);

    let other_runtime = test_runtime_with_scope_subject_and_budget(
        platform,
        profile,
        "llm.gateway",
        "facet-budget",
        "subject-beta",
        budget,
    );
    let beta = other_runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "facet budget".to_string(),
            k: 5,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4_facet".to_string(),
                question_id: "facet-p3-cross-subject".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec![
                    "external_eval:facet-budget-alpha-one".to_string(),
                    "external_eval:facet-budget-alpha-two".to_string(),
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("beta eval recall");

    assert!(beta.facet_index_report.used);
    assert_eq!(
        beta.facet_index_report.exact_facet_candidate_ids,
        Vec::<String>::new()
    );
    assert_eq!(
        beta.facet_index_report.expanded_facet_candidate_ids,
        Vec::<String>::new()
    );
    assert!(!beta.eval_candidate_pool.iter().any(|candidate| candidate
        .evidence_refs
        .iter()
        .any(|evidence| evidence.starts_with("external_eval:facet-budget-alpha"))));
}

#[test]
fn facet_rank_fusion_preserves_pool_provenance() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: bm_sdk::ParsedLongTermMemoryExtraction {
                upserts: vec![
                    long_term_draft(
                        "facet/fusion/source-visible",
                        "Facet fusion source candidate remains visible to source recall.",
                        "external_eval:facet-fusion-source",
                    ),
                    long_term_draft(
                        "facet/fusion/exact-anchor",
                        "Facet fusion exact candidate keeps exact pool provenance.",
                        "external_eval:facet-fusion-exact",
                    ),
                    long_term_draft(
                        "facet/fusion/expanded-anchor",
                        "Facet fusion expanded candidate keeps expanded pool provenance.",
                        "external_eval:facet-fusion-expanded",
                    ),
                ],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed fusion facet docs");

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "facet fusion".to_string(),
            k: 5,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4_facet".to_string(),
                question_id: "facet-p3-rank-fusion".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec![
                    "external_eval:facet-fusion-source".to_string(),
                    "external_eval:facet-fusion-exact".to_string(),
                    "external_eval:facet-fusion-expanded".to_string(),
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");

    let fusion = &report.rank_fusion_report;
    assert!(report.facet_index_report.used);
    assert!(fusion.used);
    assert_eq!(fusion.strategy, "rrf_source_facet_pool_v1");
    assert!(fusion.source_pool_count > 0);
    assert!(fusion.exact_facet_pool_count > 0);
    assert!(fusion.expanded_facet_pool_count > 0);
    assert!(fusion
        .candidate_reports
        .iter()
        .any(|candidate| candidate.source_rank.is_some()));
    assert!(fusion
        .candidate_reports
        .iter()
        .any(|candidate| candidate.exact_facet_rank.is_some()));
    assert!(fusion
        .candidate_reports
        .iter()
        .any(|candidate| candidate.expanded_facet_rank.is_some()));
    assert!(fusion
        .candidate_reports
        .iter()
        .all(|candidate| candidate.fused_score_bps > 0));
    let mut fused_ranks = fusion
        .candidate_reports
        .iter()
        .map(|candidate| candidate.fused_rank)
        .collect::<Vec<_>>();
    fused_ranks.sort_unstable();
    assert_eq!(
        fused_ranks,
        (1..=fusion.candidate_reports.len()).collect::<Vec<_>>()
    );
    assert_eq!(fusion.blocked_reasons, Vec::<String>::new());
}

#[test]
fn facet_coverage_selection_prioritizes_distinct_canonical_evidence_groups() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    budget.graph_expansion_budget.max_seed_candidates = 2;
    budget.facet_recall_budget.max_facet_anchor_candidates = 8;
    budget.facet_recall_budget.max_facet_expanded_candidates = 8;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform,
        profile,
        "llm.gateway",
        "facet-coverage",
        "subject-coverage",
        budget,
    );

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: bm_sdk::ParsedLongTermMemoryExtraction {
                upserts: vec![
                    long_term_draft(
                        "facet/coverage/shared-one",
                        "Coverage shared one should not crowd out distinct evidence.",
                        "external_eval:coverage-shared|turn=1",
                    ),
                    long_term_draft(
                        "facet/coverage/shared-two",
                        "Coverage shared two has the same canonical evidence group.",
                        "external_eval:coverage-shared|turn=2",
                    ),
                    long_term_draft(
                        "facet/coverage/distinct",
                        "Coverage distinct should survive evidence-group selection.",
                        "external_eval:coverage-distinct",
                    ),
                ],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed coverage facet docs");

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "facet coverage".to_string(),
            k: 5,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4_facet".to_string(),
                question_id: "facet-p3-coverage-selection".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec![
                    "external_eval:coverage-shared|turn=1".to_string(),
                    "external_eval:coverage-distinct".to_string(),
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");

    let coverage = &report.coverage_selection_report;
    assert!(coverage.used);
    assert_eq!(coverage.strategy, "evidence_group_coverage_v1");
    assert_eq!(coverage.selected_candidate_ids.len(), 2);
    assert!(coverage
        .covered_evidence_groups
        .iter()
        .any(|group| group == "external_eval:coverage-shared"));
    assert!(coverage
        .covered_evidence_groups
        .iter()
        .any(|group| group == "external_eval:coverage-distinct"));
    assert!(
        !coverage.coverage_dropped_candidate_ids.is_empty()
            || !coverage.budget_truncated_candidate_ids.is_empty()
    );
    assert!(coverage
        .selected_candidate_ids
        .iter()
        .all(|candidate_id| report
            .graph_anchor_candidates
            .iter()
            .any(|candidate| candidate.candidate_id == *candidate_id)));
    assert!(report.rendered_candidates.iter().all(|candidate| report
        .source_candidates
        .iter()
        .any(|source| source.candidate_id == candidate.candidate_id)));
}

#[test]
fn facet_recall_blocks_cross_subject_expanded_metadata_leakage() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let alpha_runtime = test_runtime_with_scope_subject_and_budget(
        platform.clone(),
        profile,
        "llm.gateway",
        "facet-hidden-alpha",
        "subject-alpha",
        bm_sdk::RuntimeBudgetReport::static_for_profile(profile),
    );

    alpha_runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: bm_sdk::ParsedLongTermMemoryExtraction {
                upserts: vec![long_term_draft(
                    "facet/hidden/alpha-only",
                    "Alpha-only hidden metadata must stay out of beta facet recall.",
                    "external_eval:facet-hidden-alpha",
                )],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed subject-alpha hidden facet doc");

    let beta_runtime = test_runtime_with_scope_subject_and_budget(
        platform,
        profile,
        "llm.gateway",
        "facet-hidden-beta",
        "subject-beta",
        bm_sdk::RuntimeBudgetReport::static_for_profile(profile),
    );
    let beta = beta_runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "facet hidden".to_string(),
            k: 5,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4_facet".to_string(),
                question_id: "facet-p3-cross-subject-expanded-metadata".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec!["external_eval:facet-hidden-alpha".to_string()],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("beta eval recall");

    assert!(beta.facet_index_report.used);
    assert!(beta
        .facet_index_report
        .failures
        .iter()
        .any(|failure| failure == "memory_facet_index_no_query_match"));
    assert_eq!(
        beta.facet_index_report.exact_facet_candidate_ids,
        Vec::<String>::new()
    );
    assert_eq!(
        beta.facet_index_report.expanded_facet_candidate_ids,
        Vec::<String>::new()
    );
    assert!(!beta.rank_fusion_report.used);
    assert!(!beta.coverage_selection_report.used);
    assert_eq!(beta.ablation_report.method, "sdk_eval_recall_off_run_v1");
    assert!(!beta.ablation_report.contribution_proven);
    assert!(beta.ablation_report.blocked_reasons.is_empty());
    let facet_off = beta
        .ablation_report
        .slices
        .iter()
        .find(|slice| slice.name == "facet_off")
        .expect("facet_off ablation slice");
    assert!(!facet_off.contribution_proven);
    assert_eq!(facet_off.affected_candidate_count, 0);
    assert!(facet_off.blocked_reasons.is_empty());
}

#[test]
fn facet_graph_propagation_uses_indexed_graph_anchor_without_full_scan() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);
    let anchor_draft = long_term_draft(
        "facet/p4/propagation-anchor",
        "Facet propagation anchor binds to persistent graph neighbors.",
        "external_eval:facet-p4-anchor",
    );
    let anchor_id = anchor_draft.stable_id().expect("stable long-term id");

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: bm_sdk::ParsedLongTermMemoryExtraction {
                upserts: vec![anchor_draft],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed facet graph anchor");
    runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &anchor_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Facet P4 graph anchor",
                    "external_eval:facet-p4-anchor",
                ),
                graph_node(
                    "graph:facet-p4-distinct-alpha",
                    MemoryGraphNodeKind::Task,
                    "Facet P4 distinct alpha evidence",
                    "external_eval:facet-p4-distinct-alpha",
                ),
                graph_node(
                    "graph:facet-p4-distinct-beta",
                    MemoryGraphNodeKind::Task,
                    "Facet P4 distinct beta evidence",
                    "external_eval:facet-p4-distinct-beta",
                ),
            ],
            edges: vec![
                graph_edge(
                    "edge:facet-p4-anchor-alpha",
                    &anchor_id,
                    "graph:facet-p4-distinct-alpha",
                    "external_eval:facet-p4-distinct-alpha",
                ),
                graph_edge(
                    "edge:facet-p4-anchor-beta",
                    &anchor_id,
                    "graph:facet-p4-distinct-beta",
                    "external_eval:facet-p4-distinct-beta",
                ),
            ],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "external_eval".to_string(),
                    source_id: "external_eval:facet-p4-anchor".to_string(),
                    fingerprint: "fp-facet-p4-anchor".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "external_eval".to_string(),
                    source_id: "external_eval:facet-p4-distinct-alpha".to_string(),
                    fingerprint: "fp-facet-p4-alpha".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "external_eval".to_string(),
                    source_id: "external_eval:facet-p4-distinct-beta".to_string(),
                    fingerprint: "fp-facet-p4-beta".to_string(),
                },
            ],
        })
        .expect("seed indexed graph");

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "facet p4".to_string(),
            k: 5,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4_facet".to_string(),
                question_id: "facet-p4-graph-propagation".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec![
                    "external_eval:facet-p4-distinct-alpha".to_string(),
                    "external_eval:facet-p4-distinct-beta".to_string(),
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");

    assert!(report.facet_index_report.used);
    assert!(report.graph_index_report.used);
    assert!(!report.graph_index_report.fallback_full_scan);
    assert!(report
        .graph_index_report
        .source_anchor_ids
        .iter()
        .any(|source| source == &anchor_id));
    assert!(report
        .graph_anchor_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == anchor_id));
    assert!(report
        .expanded_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == "graph:facet-p4-distinct-alpha"));
    assert!(report
        .expanded_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == "graph:facet-p4-distinct-beta"));
    let alpha = report
        .eval_candidate_pool
        .iter()
        .find(|candidate| candidate.candidate_id == "graph:facet-p4-distinct-alpha")
        .expect("alpha expanded candidate");
    assert!(alpha.score_breakdown.facet_exact_score > 0);
    assert!(alpha.score_breakdown.facet_diversity_score > 0);
    assert!(alpha.score_breakdown.facet_temporal_score > 0);
    assert!(alpha.score_breakdown.total_score >= alpha.score_breakdown.facet_exact_score);
    assert!(report.metrics.all_evidence_hit);
    assert_eq!(report.ablation_report.method, "sdk_eval_recall_off_run_v1");
    assert!(report.ablation_report.blocked_reasons.is_empty());
    let facet_off = report
        .ablation_report
        .slices
        .iter()
        .find(|slice| slice.name == "facet_off")
        .expect("facet_off ablation slice");
    assert!(facet_off.report_available);
    assert_eq!(facet_off.render_growth, 0);
    assert!(report.rendered_candidates.iter().all(|candidate| report
        .source_candidates
        .iter()
        .any(|source| source.candidate_id == candidate.candidate_id)));
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
