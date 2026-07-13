#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    canonical_evidence_ref_from_source, canonical_recall_evidence_group, memory_facet_manifest_key,
    CanonicalEntityKey, CanonicalEntityKind, CanonicalEntityRef, QueryFacetInput,
    RecallDeliverySelectionDropReason, TemporalAnchor, TemporalAnchorKind, TemporalAnchorPrecision,
    MEMORY_FACET_POSTING_NAMESPACE,
};
use bm_sdk::{
    default_agent_subject_id, EvidenceBacklink, LongTermMemoryDraft, LongTermMemoryKind,
    MemoryEvalRecallBenchmarkContext, MemoryEvalRecallRequest, MemoryEvidenceRefVisibility,
    MemoryGraphEdge, MemoryGraphEdgeKind, MemoryGraphNode, MemoryGraphNodeKind, MemoryPrivacyClass,
    MemoryProjectionRequest, MemoryRecallRequest, MemoryWriteRequest, PressureLevel, ProfileId,
    RuntimeLifecycleModeInput, RuntimeSkillWrite, RuntimeSkillWriteSource,
    TemporalMemoryGraphWriteRequest, TemporalValidity,
};

use support::{
    empty_store_platform, test_runtime, test_runtime_with_delegated_actor,
    test_runtime_with_identity_scope_and_subject, test_runtime_with_scope_and_subject,
    test_runtime_with_scope_subject_and_budget,
};

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
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: Some("chat-a".to_string()),
        source_type: None,
        source_scope: None,
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: vec![citation.to_string()],
        canonical_entities: Vec::new(),
        evidence_count: Some(1),
        observed_at: None,
        last_confirmed_at: None,
        source_revision: None,
    }
}

fn release_artifact_source_decoys(prefix: &str) -> Vec<LongTermMemoryDraft> {
    (0..4)
        .map(|index| {
            long_term_draft(
                &format!("{prefix} source decoy {index}"),
                &format!("Release artifact source decoy {index} has no persistent graph index."),
                &format!("external_eval:{prefix}-source-decoy-{index}"),
            )
        })
        .collect()
}

fn seed_governed_long_term(runtime: &bm_sdk::MemoryRuntime, drafts: Vec<LongTermMemoryDraft>) {
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: bm_sdk::ParsedLongTermMemoryExtraction {
                upserts: drafts,
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed governed long-term memory");
}

#[test]
fn production_delivery_rejects_private_owner_records_before_capsule_render() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);
    let mut private = long_term_draft(
        "private delivery sentinel",
        "SOUL_PRIVATE_RAW_SENTINEL must never reach recall delivery.",
        "private://soul/raw-locator-sentinel",
    );
    private.privacy = MemoryPrivacyClass::SoulPrivate;
    let public = long_term_draft(
        "public delivery anchor",
        "Public delivery anchor may load a graph with a restricted neighbor.",
        "external_eval:public-delivery-anchor",
    );
    seed_governed_long_term(&runtime, vec![private, public]);
    let raw_records = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(10)
        .expect("list long-term owner records");
    let private_id = raw_records
        .iter()
        .find(|entry| entry.content.contains("SOUL_PRIVATE_RAW_SENTINEL"))
        .expect("private owner record")
        .id
        .clone();
    let public_id = raw_records
        .iter()
        .find(|entry| entry.content.contains("Public delivery anchor may load"))
        .expect("public owner record")
        .id
        .clone();
    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &public_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Public delivery anchor",
                    "external_eval:public-delivery-anchor",
                ),
                graph_node(
                    &private_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Restricted governed neighbor",
                    "turn:restricted-neighbor",
                ),
            ],
            edges: vec![graph_edge(
                "edge:public-anchor:restricted-neighbor",
                &public_id,
                &private_id,
                "turn:restricted-neighbor",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "long_term_memory".to_string(),
                    source_id: "external_eval:public-delivery-anchor".to_string(),
                    fingerprint: "fp-public-delivery-anchor".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: "turn:restricted-neighbor".to_string(),
                    fingerprint: "fp-restricted-neighbor".to_string(),
                },
            ],
        })
        .expect("write graph containing restricted neighbor");
    assert!(!graph_write.accepted);
    assert!(graph_write
        .gate_failures
        .contains(&"memory_graph_persistent_node_owner_not_visible".to_string()));
    assert!(graph_write.transaction.is_none());

    let report = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "public delivery anchor".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("production recall");

    assert!(!report.graph_index_report.used);
    assert!(!report.graph_index_report.maintenance_required);
    assert_eq!(report.graph_index_report.read_path_mutation_delta, 0);
    assert!(!report.source_candidate_ids.contains(&private_id));
    assert!(!report
        .graph_rerank
        .reranked_candidate_ids
        .contains(&private_id));
    assert!(!report
        .facet_index_report
        .exact_facet_candidate_ids
        .contains(&private_id));
    assert!(!report
        .facet_index_report
        .expanded_facet_candidate_ids
        .contains(&private_id));
    assert!(!report
        .graph_candidate_evidence_ref_index
        .iter()
        .any(|entry| entry.candidate_id == private_id
            || entry
                .evidence_refs
                .iter()
                .any(|reference| reference.contains("private://soul"))));
    assert!(!report
        .facet_index_report
        .failures
        .iter()
        .any(|failure| failure == "memory_facet_privacy_scope_blocked"));
    assert!(!report
        .delivery_report
        .selection_decisions
        .iter()
        .any(|decision| {
            decision.candidate_id == private_id
                || decision
                    .canonical_evidence_groups
                    .iter()
                    .any(|group| group.contains("raw-locator-sentinel"))
        }));
    assert!(report
        .delivery_report
        .rendered_capsules
        .iter()
        .all(|capsule| {
            !capsule.content.contains("SOUL_PRIVATE_RAW_SENTINEL")
                && capsule
                    .visible_evidence_refs
                    .iter()
                    .all(|reference| !reference.contains("raw-locator-sentinel"))
        }));
    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "public delivery anchor".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("private projection");
    assert!(projection.context.long_term_memory_text.is_none());
    assert!(!projection
        .system_memory_block
        .contains("SOUL_PRIVATE_RAW_SENTINEL"));
    assert!(projection
        .runtime_projection
        .governed_memory_evidence
        .iter()
        .all(|block| !block.content.contains("SOUL_PRIVATE_RAW_SENTINEL")));

    assert!(!platform
        .replay_harness()
        .export_store_snapshot()
        .expect("rejected graph snapshot")
        .json_docs
        .iter()
        .any(|doc| doc.namespace.starts_with("memory_graph_")));
}

#[test]
fn persistent_graph_recall_drops_ownerless_private_like_nodes_and_dependents() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);
    seed_governed_long_term(
        &runtime,
        vec![long_term_draft(
            "ownerless graph anchor",
            "Visible governed owner anchors the graph recall.",
            "external_eval:ownerless-visible-anchor",
        )],
    );
    let visible_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(usize::MAX)
        .expect("list visible owner")
        .into_iter()
        .find(|entry| entry.content.contains("Visible governed owner anchors"))
        .expect("visible owner")
        .id;
    let ownerless_id = "graph:ownerless-private-like";
    let ownerless_evidence = "archive:/private/ownerless#turn=9";
    let ownerless_label = "OWNERLESS_PRIVATE_GRAPH_SENTINEL";

    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &visible_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Visible governed owner",
                    "external_eval:ownerless-visible-anchor",
                ),
                graph_node(
                    ownerless_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    ownerless_label,
                    ownerless_evidence,
                ),
            ],
            edges: vec![graph_edge(
                "edge:visible-owner:ownerless-private-like",
                &visible_id,
                ownerless_id,
                ownerless_evidence,
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "long_term_memory".to_string(),
                    source_id: "external_eval:ownerless-visible-anchor".to_string(),
                    fingerprint: "fp-ownerless-visible-anchor".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: ownerless_evidence.to_string(),
                    fingerprint: "fp-ownerless-private-like".to_string(),
                },
            ],
        })
        .expect("write ownerless graph fixture");
    assert!(!graph_write.accepted);
    assert!(graph_write
        .gate_failures
        .contains(&"memory_graph_persistent_node_owner_missing".to_string()));
    assert!(graph_write.transaction.is_none());

    let recall = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "ownerless graph anchor".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall ownerless graph fixture");
    let recall_disclosure = format!(
        "{:?}{:?}{:?}{:?}",
        recall.compact_graph,
        recall.graph_candidate_evidence_ref_index,
        recall.graph_index_report,
        recall.graph_rerank
    );
    assert!(!recall_disclosure.contains(ownerless_id));
    assert!(!recall_disclosure.contains(ownerless_label));
    assert!(!recall_disclosure.contains(ownerless_evidence));
    assert!(!recall.graph_index_report.used);
    assert_eq!(recall.graph_index_report.read_path_mutation_delta, 0);

    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("rejected graph snapshot");
    assert!(!snapshot
        .json_docs
        .iter()
        .any(|doc| { doc.namespace.starts_with("memory_graph_") }));

    let eval = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
            query: "ownerless graph anchor".to_string(),
            k: 4,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: false,
            benchmark_context: None,
            tool_registry_refs: Vec::new(),
        })
        .expect("eval ownerless graph fixture");
    let eval_disclosure = format!("{eval:?}");
    assert!(!eval_disclosure.contains(ownerless_id));
    assert!(!eval_disclosure.contains(ownerless_label));
    assert!(!eval_disclosure.contains(ownerless_evidence));
    assert_eq!(eval.graph_index_report.read_path_mutation_delta, 0);
}

#[test]
fn governed_read_filters_private_records_before_source_recall_limit() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform.clone(), profile);
    let mut public = long_term_draft(
        "visibility displacement sentinel public",
        "The governed public record must survive source recall limiting.",
        "external_eval:visibility-public",
    );
    public.privacy = MemoryPrivacyClass::PublicRuntime;
    let private = (0..17)
        .map(|index| {
            let mut draft = long_term_draft(
                &format!("visibility displacement sentinel private {index}"),
                "A newer private record must not consume a governed source slot.",
                &format!("private://visibility/{index}"),
            );
            draft.privacy = MemoryPrivacyClass::SoulPrivate;
            draft
        })
        .collect::<Vec<_>>();
    let mut drafts = vec![public];
    drafts.extend(private);
    seed_governed_long_term(&runtime, drafts);
    let public_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(96)
        .expect("list records")
        .into_iter()
        .find(|entry| {
            entry
                .supporting_citations
                .iter()
                .any(|citation| citation == "external_eval:visibility-public")
        })
        .expect("public record")
        .id;
    let report = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "visibility displacement sentinel".to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .expect("governed recall");

    assert!(report.source_candidate_ids.contains(&public_id));
    assert!(report
        .source_candidate_ids
        .iter()
        .all(|candidate_id| candidate_id == &public_id));
}

#[test]
fn production_capsule_exposes_non_public_locator_only_as_stable_opaque_ref() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);
    seed_governed_long_term(
        &runtime,
        vec![long_term_draft(
            "opaque locator evidence",
            "Governed shared evidence remains usable without exposing its locator.",
            "archive://private-path/session-7#turn=3",
        )],
    );

    let first = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "opaque locator evidence".to_string(),
            limit: 1,
            tool_registry_refs: Vec::new(),
        })
        .expect("first production recall");
    let second = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "opaque locator evidence".to_string(),
            limit: 1,
            tool_registry_refs: Vec::new(),
        })
        .expect("second production recall");
    let first_view = &first.delivery_report.rendered_capsules[0].source_locator_view;
    let second_view = &second.delivery_report.rendered_capsules[0].source_locator_view;

    assert_eq!(
        first_view.visibility,
        MemoryEvidenceRefVisibility::GovernedOpaque
    );
    assert_eq!(first_view.reference, second_view.reference);
    assert!(first_view
        .reference
        .as_deref()
        .is_some_and(|reference| reference.starts_with("opaque:evidence:")
            && !reference.contains("private-path")));
}

#[test]
fn delivery_and_loss_reports_never_expose_archive_or_transcript_locators() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    budget.recall_delivery_budget.max_selected_candidates = 1;
    budget.recall_delivery_budget.max_rendered_capsules = 1;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform,
        profile,
        "llm.gateway",
        "opaque-report",
        "subject-opaque-report",
        budget,
    );
    let archive_locator = "archive:/private/releases.md#turn=7";
    let transcript_locator = "transcript:private-chat#message=9";
    seed_governed_long_term(
        &runtime,
        vec![
            long_term_draft(
                "opaque report archive evidence",
                "Opaque report evidence alpha remains governed and attributable.",
                archive_locator,
            ),
            long_term_draft(
                "opaque report transcript evidence",
                "Opaque report evidence beta remains governed and attributable.",
                transcript_locator,
            ),
        ],
    );

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
            query: "opaque report evidence".to_string(),
            k: 4,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_p7".to_string(),
                question_id: "opaque-report-locators".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec![
                    archive_locator.to_string(),
                    transcript_locator.to_string(),
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("opaque report eval recall");
    let disclosure = format!(
        "{:?}{:?}",
        report.delivery_report, report.stage_diagnostics.loss_ledger
    );

    assert!(!disclosure.contains(archive_locator));
    assert!(!disclosure.contains(transcript_locator));
    assert!(!disclosure.contains("private/releases.md"));
    assert!(!disclosure.contains("private-chat"));
    assert!(disclosure.contains("opaque:recall-group:sha256:"));
}

#[test]
fn evidence_locator_fragments_never_count_as_typed_exact_facet_matches() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform.clone(), profile);
    seed_governed_long_term(
        &runtime,
        vec![long_term_draft(
            "citation metadata sentinel",
            "The governed content intentionally omits locator namespace words.",
            "external_eval:D1:12|session_1",
        )],
    );
    let owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(10)
        .expect("list long-term records")
        .into_iter()
        .find(|entry| {
            entry
                .supporting_citations
                .iter()
                .any(|citation| citation == "external_eval:D1:12|session_1")
        })
        .expect("owner record")
        .id;

    for weak_locator_fragment in ["external", "eval", "d1", "12", "session_1"] {
        let report = runtime
            .recall(MemoryRecallRequest {
                structured_query_facets: Vec::new(),
                query: weak_locator_fragment.to_string(),
                limit: 4,
                tool_registry_refs: Vec::new(),
            })
            .expect("production recall");
        assert!(!report
            .facet_index_report
            .exact_facet_candidate_ids
            .contains(&owner_id));
        assert!(!report
            .facet_index_report
            .expanded_facet_candidate_ids
            .contains(&owner_id));
    }
}

#[test]
fn facet_recall_fails_closed_when_manifest_counts_are_corrupt() {
    let profile = ProfileId::ServerLinuxDevFull;
    let source_platform = empty_store_platform(profile);
    let source_runtime = test_runtime(source_platform.clone(), profile);
    seed_governed_long_term(
        &source_runtime,
        vec![long_term_draft(
            "manifest integrity sentinel",
            "Manifest integrity must be checked before exact posting recall.",
            "external_eval:manifest-integrity",
        )],
    );
    let manifest_key = memory_facet_manifest_key(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .expect("facet manifest key");
    let mut snapshot = source_platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    let manifest_doc = snapshot
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == MEMORY_FACET_POSTING_NAMESPACE && doc.key == manifest_key)
        .expect("facet manifest");
    manifest_doc.value["owner_doc_count"] = serde_json::json!(0);

    let platform = empty_store_platform(profile);
    platform
        .replay_harness()
        .import_store_snapshot(&snapshot)
        .expect("restore schema-valid snapshot with corrupt facet manifest");
    let runtime = test_runtime(platform, profile);

    let report = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "manifest integrity sentinel".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall remains available without trusting corrupt facet index");

    assert!(!report.facet_index_report.manifest_integrity_verified);
    assert!(report.facet_index_report.report_only);
    assert!(report
        .facet_index_report
        .failures
        .contains(&"memory_facet_manifest_contract_mismatch".to_string()));
}

#[test]
fn facet_recall_fails_closed_for_independent_owner_and_facet_version_drift() {
    let profile = ProfileId::ServerLinuxDevFull;
    let source_platform = empty_store_platform(profile);
    let source_runtime = test_runtime(source_platform.clone(), profile);
    seed_governed_long_term(
        &source_runtime,
        vec![long_term_draft(
            "dual version integrity sentinel",
            "Owner and facet versions must both match through the read chain.",
            "external_eval:dual-version-integrity",
        )],
    );
    let manifest_key = memory_facet_manifest_key(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .expect("facet manifest key");

    for field in ["owner_revision", "facet_index_revision"] {
        let mut snapshot = source_platform
            .replay_harness()
            .export_store_snapshot()
            .expect("snapshot");
        let manifest = snapshot
            .json_docs
            .iter_mut()
            .find(|doc| doc.namespace == MEMORY_FACET_POSTING_NAMESPACE && doc.key == manifest_key)
            .expect("facet manifest");
        let version = manifest.value["owner_versions"][0][field]
            .as_u64()
            .expect("owner version field");
        manifest.value["owner_versions"][0][field] = serde_json::json!(version + 1);

        let platform = empty_store_platform(profile);
        platform
            .replay_harness()
            .import_store_snapshot(&snapshot)
            .expect("restore schema-valid drifted snapshot");
        let runtime = test_runtime(platform, profile);
        let report = runtime
            .recall(MemoryRecallRequest {
                structured_query_facets: Vec::new(),
                query: "dual version integrity sentinel".to_string(),
                limit: 4,
                tool_registry_refs: Vec::new(),
            })
            .expect("recall fails closed without failing request");

        assert!(!report.facet_index_report.manifest_integrity_verified);
        assert!(report.facet_index_report.report_only);
        assert!(
            report
                .facet_index_report
                .failures
                .iter()
                .any(|failure| failure.contains("version_mismatch")),
            "unexpected failures for {field}: {:?}",
            report.facet_index_report.failures
        );
    }
}

#[test]
fn facet_recall_reports_verified_zero_hit_when_query_posting_is_absent_from_manifest() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);
    seed_governed_long_term(
        &runtime,
        vec![long_term_draft(
            "existing posting owner",
            "An existing owner creates a valid scoped facet manifest.",
            "external_eval:existing-posting-owner",
        )],
    );

    let report = runtime
        .recall(MemoryRecallRequest {
            query: "unrelated query".to_string(),
            limit: 4,
            structured_query_facets: vec![QueryFacetInput::Keyword(
                "posting-that-does-not-exist".to_string(),
            )],
            tool_registry_refs: Vec::new(),
        })
        .expect("a governed zero-hit posting lookup must preserve recall");

    assert!(report.facet_index_report.posting_key_lookup_count > 0);
    assert_eq!(report.facet_index_report.manifest_matched_posting_count, 0);
    assert_eq!(report.facet_index_report.posting_doc_read_count, 0);
    assert_eq!(report.facet_index_report.owner_key_lookup_count, 0);
    assert_eq!(report.facet_index_report.owner_doc_read_count, 0);
    assert!(report.facet_index_report.used);
    assert!(!report.facet_index_report.report_only);
    assert!(report.facet_index_report.manifest_integrity_verified);
    assert!(!report
        .facet_index_report
        .failures
        .iter()
        .any(|failure| failure == "memory_facet_posting_not_found"));
}

#[test]
fn facet_recall_fails_closed_when_manifest_posting_document_is_missing() {
    let profile = ProfileId::ServerLinuxDevFull;
    let source_platform = empty_store_platform(profile);
    let source_runtime = test_runtime(source_platform.clone(), profile);
    let mut draft = long_term_draft(
        "missing posting sentinel",
        "A manifest-listed posting must exist before owner reads are allowed.",
        "external_eval:missing-posting-sentinel",
    );
    draft.keywords = vec!["missing-posting-sentinel".to_string()];
    seed_governed_long_term(&source_runtime, vec![draft]);
    let manifest_key = memory_facet_manifest_key(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .expect("facet manifest key");
    let mut snapshot = source_platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    snapshot
        .json_docs
        .retain(|doc| doc.namespace != MEMORY_FACET_POSTING_NAMESPACE || doc.key == manifest_key);

    let platform = empty_store_platform(profile);
    platform
        .replay_harness()
        .import_store_snapshot(&snapshot)
        .expect("restore snapshot with missing posting document");
    let runtime = test_runtime(platform, profile);
    let report = runtime
        .recall(MemoryRecallRequest {
            query: "irrelevant".to_string(),
            limit: 4,
            structured_query_facets: vec![QueryFacetInput::Keyword(
                "missing-posting-sentinel".to_string(),
            )],
            tool_registry_refs: Vec::new(),
        })
        .expect("corrupt facet index must not break baseline recall");

    assert!(!report.facet_index_report.used);
    assert!(report.facet_index_report.report_only);
    assert!(!report.facet_index_report.manifest_integrity_verified);
    assert_eq!(report.facet_index_report.owner_key_lookup_count, 0);
    assert_eq!(report.facet_index_report.owner_doc_read_count, 0);
    assert!(report
        .facet_index_report
        .failures
        .contains(&"memory_facet_posting_manifest_read_contract_mismatch".to_string()));
}

#[test]
fn production_typed_temporal_facet_preempts_text_facets_and_hits_recall_projection_and_eval() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    budget.facet_recall_budget.max_query_facets = 1;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform.clone(),
        profile,
        "llm.gateway",
        "typed-temporal",
        "subject-typed-temporal",
        budget,
    );
    let mut temporal_owner = long_term_draft(
        "typed temporal owner",
        "Only the typed temporal anchor should select this owner.",
        "external_eval:typed-temporal-owner",
    );
    temporal_owner.observed_at = Some(1_700_000_123);
    let mut text_owner = long_term_draft(
        "plain text decoy",
        "The plain text decoy matches the request text.",
        "external_eval:plain-text-decoy",
    );
    text_owner.observed_at = Some(1_700_000_456);
    seed_governed_long_term(&runtime, vec![temporal_owner, text_owner]);
    let records = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(usize::MAX)
        .expect("typed temporal owners");
    let temporal_owner_id = records
        .iter()
        .find(|entry| entry.content.contains("typed temporal anchor"))
        .expect("temporal owner")
        .id
        .clone();
    let text_owner_id = records
        .iter()
        .find(|entry| entry.content.contains("plain text decoy matches"))
        .expect("text owner")
        .id
        .clone();
    let evidence = canonical_evidence_ref_from_source("external_eval:typed-temporal-query")
        .expect("canonical typed temporal query evidence");
    let typed_facet = QueryFacetInput::Temporal(TemporalAnchor {
        anchor_kind: TemporalAnchorKind::ObservedAt,
        epoch_secs: 1_700_000_123,
        precision: TemporalAnchorPrecision::Second,
        evidence_ref: evidence,
    });

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "plain text decoy".to_string(),
            limit: 8,
            structured_query_facets: vec![typed_facet.clone()],
            tool_registry_refs: Vec::new(),
        })
        .expect("typed temporal recall");
    assert_eq!(
        recall.facet_index_report.exact_facet_candidate_ids,
        vec![temporal_owner_id.clone()]
    );
    assert!(!recall
        .facet_index_report
        .exact_facet_candidate_ids
        .contains(&text_owner_id));

    let projection = runtime
        .project(MemoryProjectionRequest {
            user_query: "plain text decoy".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![typed_facet.clone()],
            tool_registry_refs: Vec::new(),
        })
        .expect("typed temporal projection");
    assert!(projection
        .recall_delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| capsule.candidate_id == temporal_owner_id));

    let eval = runtime
        .eval_recall(MemoryEvalRecallRequest {
            query: "plain text decoy".to_string(),
            k: 8,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: false,
            benchmark_context: None,
            structured_query_facets: vec![typed_facet],
            tool_registry_refs: Vec::new(),
        })
        .expect("typed temporal eval recall");
    assert_eq!(
        eval.facet_index_report.exact_facet_candidate_ids,
        vec![temporal_owner_id]
    );
}

#[test]
fn production_typed_entity_facet_hits_a_governed_owner() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform.clone(), profile);
    let citation = "external_eval:typed-entity-owner";
    let key = CanonicalEntityKey {
        kind: CanonicalEntityKind::Person,
        canonical_id: "alice-42".to_string(),
    };
    let mut draft = long_term_draft(
        "typed entity owner",
        "A governed owner can be selected by a canonical entity facet.",
        citation,
    );
    draft.canonical_entities = vec![CanonicalEntityRef {
        key: key.clone(),
        display_label: Some("Alice".to_string()),
        aliases: vec!["Alice Example".to_string()],
        evidence_refs: vec![
            canonical_evidence_ref_from_source(citation).expect("canonical entity evidence")
        ],
    }];
    seed_governed_long_term(&runtime, vec![draft]);
    let owner_id = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(usize::MAX)
        .expect("typed entity owner")
        .into_iter()
        .find(|entry| entry.content.contains("canonical entity facet"))
        .expect("entity owner")
        .id;
    let report = runtime
        .recall(MemoryRecallRequest {
            query: "text query without entity owner terms".to_string(),
            limit: 8,
            structured_query_facets: vec![QueryFacetInput::Entity(key)],
            tool_registry_refs: Vec::new(),
        })
        .expect("typed entity production recall");

    assert!(report.facet_index_report.manifest_integrity_verified);
    assert_eq!(
        report.facet_index_report.exact_facet_candidate_ids,
        vec![owner_id]
    );
}

#[test]
fn invalid_or_unresolved_typed_entity_and_temporal_facets_reject_all_production_entries() {
    let profile = ProfileId::ServerLinuxDevFull;
    let runtime = test_runtime(empty_store_platform(profile), profile);
    let requests = [
        (
            QueryFacetInput::UnresolvedEntity("Alice".to_string()),
            "entity_query_facet_requires_typed_anchor",
        ),
        (
            QueryFacetInput::UnresolvedTemporal("last week".to_string()),
            "temporal_query_facet_requires_typed_anchor",
        ),
    ];

    for (facet, reason) in requests {
        let recall_error = runtime
            .recall(MemoryRecallRequest {
                query: "typed rejection".to_string(),
                limit: 4,
                structured_query_facets: vec![facet.clone()],
                tool_registry_refs: Vec::new(),
            })
            .expect_err("recall must reject unresolved typed facet");
        assert_eq!(recall_error.stage(), "memory_facet_query");
        assert!(recall_error.to_string().contains(reason));

        let projection_error = match runtime.project(MemoryProjectionRequest {
            user_query: "typed rejection".to_string(),
            system_max_len: 1024,
            recent_messages_limit: 2,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: vec![facet.clone()],
            tool_registry_refs: Vec::new(),
        }) {
            Ok(_) => panic!("projection must reject unresolved typed facet"),
            Err(error) => error,
        };
        assert_eq!(projection_error.stage(), "memory_facet_query");
        assert!(projection_error.to_string().contains(reason));

        let eval_error = runtime
            .eval_recall(MemoryEvalRecallRequest {
                query: "typed rejection".to_string(),
                k: 4,
                include_expanded_candidates: true,
                include_graph_neighbors: true,
                include_score_breakdown: true,
                include_missing_evidence: false,
                benchmark_context: None,
                structured_query_facets: vec![facet],
                tool_registry_refs: Vec::new(),
            })
            .expect_err("eval recall must reject unresolved typed facet");
        assert_eq!(eval_error.stage(), "memory_facet_query");
        assert!(eval_error.to_string().contains(reason));
    }
}

#[test]
fn facet_recall_fails_closed_before_owner_reads_when_posting_exceeds_governed_budget() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    budget.facet_recall_budget.max_facet_index_docs_read = 8;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform,
        profile,
        "llm.gateway",
        "facet-owner-budget",
        "subject-facet-owner-budget",
        budget,
    );
    let drafts = (0..9)
        .map(|index| {
            let mut draft = long_term_draft(
                &format!("governed owner {index}"),
                &format!("Governed owner {index} belongs to one oversized posting."),
                &format!("external_eval:governed-owner-{index}"),
            );
            draft.keywords = vec!["oversized-posting".to_string()];
            draft
        })
        .collect::<Vec<_>>();
    seed_governed_long_term(&runtime, drafts);

    let report = runtime
        .recall(MemoryRecallRequest {
            query: "unrelated text".to_string(),
            limit: 8,
            structured_query_facets: vec![QueryFacetInput::Keyword(
                "oversized-posting".to_string(),
            )],
            tool_registry_refs: Vec::new(),
        })
        .expect("oversized governed posting recall");

    assert!(
        !report.facet_index_report.used,
        "{:#?}",
        report.facet_index_report
    );
    assert!(!report.facet_index_report.manifest_integrity_verified);
    assert_eq!(report.facet_index_report.owner_doc_read_count, 0);
    assert!(report
        .facet_index_report
        .failures
        .contains(&"memory_facet_governed_owner_read_budget_exceeded".to_string()));
    assert!(report
        .facet_index_report
        .exact_facet_candidate_ids
        .is_empty());
}

#[test]
fn eval_recall_reports_production_selection_render_loss_ledger() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);
    seed_governed_long_term(
        &runtime,
        vec![
            long_term_draft(
                "release alpha evidence",
                "Release alpha evidence remains available for the governed answer.",
                "external_eval:alpha",
            ),
            long_term_draft(
                "release beta evidence",
                "Release beta evidence remains available for the governed answer.",
                "external_eval:beta",
            ),
        ],
    );
    let entries = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(10)
        .expect("list multi-gold long-term memory");
    let alpha_id = entries
        .iter()
        .find(|entry| {
            entry
                .supporting_citations
                .iter()
                .any(|citation| citation == "external_eval:alpha")
        })
        .expect("alpha entry")
        .id
        .clone();
    let beta_id = entries
        .iter()
        .find(|entry| {
            entry
                .supporting_citations
                .iter()
                .any(|citation| citation == "external_eval:beta")
        })
        .expect("beta entry")
        .id
        .clone();
    runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &alpha_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release alpha evidence",
                    "external_eval:alpha",
                ),
                graph_node(
                    &beta_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release beta evidence",
                    "external_eval:beta",
                ),
            ],
            edges: vec![graph_edge(
                "edge:p7-alpha-beta",
                &alpha_id,
                &beta_id,
                "external_eval:beta",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "external_eval_dataset".to_string(),
                    source_id: "external_eval:alpha".to_string(),
                    fingerprint: "fp-p7-alpha".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "external_eval_dataset".to_string(),
                    source_id: "external_eval:beta".to_string(),
                    fingerprint: "fp-p7-beta".to_string(),
                },
            ],
        })
        .expect("write multi-gold graph");

    let production = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "release evidence".to_string(),
            limit: 1,
            tool_registry_refs: Vec::new(),
        })
        .expect("production recall");
    assert_eq!(production.delivery_report.owner, "bm-sdk::MemoryRuntime");
    assert_eq!(production.delivery_report.selected_candidate_ids.len(), 1);
    assert_eq!(production.delivery_report.rendered_capsules.len(), 1);
    assert!(production.delivery_report.render_growth == 0);

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
            query: "release evidence".to_string(),
            k: 1,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_p7".to_string(),
                question_id: "multi-gold-loss".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec![
                    "external_eval:alpha".to_string(),
                    "external_eval:beta".to_string(),
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");
    let unrelated_gold_report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
            query: "release evidence".to_string(),
            k: 1,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_p7".to_string(),
                question_id: "unrelated-gold-must-not-steer-delivery".to_string(),
                question_type: "single_gold".to_string(),
                expected_evidence_refs: vec!["external_eval:not-present".to_string()],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall with unrelated gold");

    assert_eq!(
        report.delivery_report, unrelated_gold_report.delivery_report,
        "benchmark gold may diagnose delivery but must never steer it"
    );

    assert_eq!(
        report
            .selected_candidates
            .iter()
            .map(|candidate| candidate.candidate_id.clone())
            .collect::<Vec<_>>(),
        production.delivery_report.selected_candidate_ids
    );
    assert_eq!(
        report
            .rendered_candidates
            .iter()
            .map(|candidate| candidate.candidate_id.clone())
            .collect::<Vec<_>>(),
        production
            .delivery_report
            .rendered_capsules
            .iter()
            .map(|capsule| capsule.candidate_id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        report
            .stage_diagnostics
            .loss_ledger
            .expanded_hit_selected_miss
            .len(),
        1
    );
    assert!(report
        .stage_diagnostics
        .loss_ledger
        .expanded_hit_selected_miss
        .iter()
        .all(|entry| entry.selection_drop_reason.is_some()));
    assert!(report
        .stage_diagnostics
        .loss_ledger
        .selected_hit_rendered_miss
        .is_empty());
}

#[test]
fn projection_consumes_production_evidence_capsules_without_render_budget_growth() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);
    seed_governed_long_term(
        &runtime,
        vec![long_term_draft(
            "release capsule evidence",
            "Release capsule evidence must reach governed memory evidence.",
            "external_eval:capsule",
        )],
    );

    let eval = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
            query: "release capsule evidence".to_string(),
            k: 1,
            include_expanded_candidates: true,
            include_graph_neighbors: false,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_p7".to_string(),
                question_id: "production-capsule-citation".to_string(),
                question_type: "single_gold".to_string(),
                expected_evidence_refs: vec!["external_eval:capsule".to_string()],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval capsule delivery");
    assert!(eval.facet_index_report.manifest_owner_doc_count > 0);
    assert!(eval.facet_index_report.manifest_posting_doc_count > 0);
    assert!(eval.facet_index_report.manifest_integrity_verified);
    assert!(eval.rendered_candidates.iter().any(|candidate| {
        candidate
            .evidence_refs
            .iter()
            .any(|reference| reference == &canonical_recall_evidence_group("external_eval:capsule"))
    }));

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "release capsule evidence".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");

    assert!(!projection
        .recall_delivery_report
        .rendered_capsules
        .is_empty());
    assert!(
        projection
            .runtime_projection
            .governed_memory_evidence
            .iter()
            .any(|block| block.role == "recall_evidence_capsule"
                && block
                    .evidence_refs
                    .iter()
                    .any(|reference| reference.starts_with("opaque:evidence:"))),
        "governed evidence: {:?}; delivery: {:?}",
        projection.runtime_projection.governed_memory_evidence,
        projection.recall_delivery_report.rendered_capsules
    );
    assert!(projection.system_memory_block.chars().count() <= 4096);
    assert!(!projection
        .system_memory_block
        .contains("external_eval:capsule"));
    assert!(projection
        .recall_delivery_report
        .rendered_capsules
        .iter()
        .any(|capsule| capsule
            .canonical_evidence_groups
            .iter()
            .any(|group| group == &canonical_recall_evidence_group("external_eval:capsule"))));
    assert!(projection
        .system_memory_block
        .contains("## Boundary And Disclosure Protocol"));
    assert!(projection
        .system_memory_block
        .contains("## Work Integrity Covenant"));
    assert_eq!(projection.recall_delivery_report.render_growth, 0);

    let constrained = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "release capsule evidence".to_string(),
            system_max_len: 512,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("constrained projection");
    assert!(constrained
        .recall_delivery_report
        .rendered_capsules
        .is_empty());
    assert!(constrained
        .runtime_projection
        .governed_memory_evidence
        .iter()
        .all(|block| block.role != "recall_evidence_capsule"));
    assert!(!constrained
        .system_memory_block
        .contains("external_eval:capsule"));
    assert!(constrained.system_memory_block.chars().count() <= 512);
}

#[test]
fn projection_keeps_unicode_capsules_consistent_with_the_character_budget() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);
    let repeated_evidence = "记忆证据".repeat(80);
    let drafts = (0..16)
        .map(|index| {
            long_term_draft(
                &format!("unicode delivery evidence {index}"),
                &format!("unicode delivery evidence {index}: {repeated_evidence}"),
                &format!("external_eval:unicode-{index}"),
            )
        })
        .collect();
    seed_governed_long_term(&runtime, drafts);

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "unicode delivery evidence".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("unicode projection");

    assert!(projection.recall_delivery_report.rendered_capsules.len() > 1);
    assert!(projection.system_memory_block.chars().count() <= 4096);
    assert!(projection.system_memory_block.len() > 4096);
    assert_eq!(projection.delivery_digest_manifest.schema_version, 2);
    assert!(projection.delivery_digest_manifest.exact_render_match);
    assert!(projection
        .delivery_digest_manifest
        .integrity_failures
        .is_empty());
    assert_eq!(
        projection.delivery_digest_manifest.candidate_receipts.len(),
        projection.recall_delivery_report.rendered_capsules.len()
    );
    assert_eq!(
        projection
            .delivery_digest_manifest
            .system_memory_block_sha256
            .len(),
        64
    );
    for capsule in &projection.recall_delivery_report.rendered_capsules {
        let block = projection
            .runtime_projection
            .governed_memory_evidence
            .iter()
            .find(|block| block.source_id == capsule.candidate_id)
            .expect("rendered capsule must own a governed projection block");
        assert_eq!(block.role, "recall_evidence_capsule");
        assert_eq!(block.content, capsule.content);
        let receipt = projection
            .delivery_digest_manifest
            .candidate_receipts
            .iter()
            .find(|receipt| receipt.candidate_id == capsule.candidate_id)
            .expect("renderer-owned candidate receipt");
        assert_eq!(receipt.source_block_sha256.len(), 64);
    }
}

#[test]
fn projection_digest_proves_duplicate_content_by_candidate_source_id() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);
    let duplicate_content =
        "Duplicate projection content must remain attributable to its exact candidate source id.";
    seed_governed_long_term(
        &runtime,
        vec![
            long_term_draft(
                "duplicate receipt alpha",
                duplicate_content,
                "external_eval:duplicate-receipt-alpha",
            ),
            long_term_draft(
                "duplicate receipt beta",
                duplicate_content,
                "external_eval:duplicate-receipt-beta",
            ),
        ],
    );

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "duplicate receipt".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("duplicate-content projection");
    let receipts = &projection.delivery_digest_manifest.candidate_receipts;
    let capsule_entries = &projection.delivery_digest_manifest.capsule_entries;
    let governed_entries = &projection.delivery_digest_manifest.governed_block_entries;
    let prompt_entries = &projection.delivery_digest_manifest.prompt_visible_entries;

    assert!(projection.delivery_digest_manifest.exact_render_match);
    assert_eq!(capsule_entries, governed_entries);
    assert_eq!(capsule_entries, prompt_entries);
    assert_eq!(capsule_entries.len(), 2);
    assert_ne!(
        capsule_entries[0].candidate_id,
        capsule_entries[1].candidate_id
    );
    assert_eq!(
        capsule_entries[0].content_sha256,
        capsule_entries[1].content_sha256
    );
    assert_eq!(receipts.len(), 2);
    assert_ne!(receipts[0].candidate_id, receipts[1].candidate_id);
    assert_ne!(
        receipts[0].source_block_sha256,
        receipts[1].source_block_sha256
    );
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
            structured_query_facets: Vec::new(),
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
            structured_query_facets: Vec::new(),
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
    assert!(!report
        .selected_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == "runtime_skill__release_guard"));
    assert!(report.rendered_candidates.is_empty());
    assert!(!report
        .delivery_report
        .selection_decisions
        .iter()
        .any(|decision| decision.candidate_id == "runtime_skill__release_guard"));
    assert!(report
        .delivery_report
        .delivery_drop_reasons
        .iter()
        .any(|reason| reason == "owner_record_unavailable"));
    assert_eq!(
        report.missing_evidence_refs,
        vec![canonical_recall_evidence_group(
            "external_eval:missing-source"
        )]
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
    let anchor = long_term_draft(
        "release artifact anchor",
        "Release artifact anchor owns the persistent graph seed.",
        "external_eval:release-artifact-anchor",
    );
    let anchor_id = anchor.stable_id().expect("anchor owner id");
    let mut manifest = long_term_draft(
        "manifest graph neighbor",
        "Manifest checksum evidence is available through graph expansion.",
        "turn:release-manifest",
    );
    manifest.keywords = vec!["manifest-checksum".to_string()];
    let manifest_id = manifest.stable_id().expect("manifest owner id");
    let mut owners = vec![anchor, manifest];
    owners.extend(release_artifact_source_decoys("persistent-graph"));
    seed_governed_long_term(&runtime, owners);

    let preview = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
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
    assert!(!preview.graph_index_report.used);

    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &anchor_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release artifact guard",
                    "external_eval:release-artifact-anchor",
                ),
                graph_node(
                    &manifest_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release manifest check",
                    "turn:release-manifest",
                ),
            ],
            edges: vec![graph_edge(
                "edge:release_guard:manifest_check",
                &anchor_id,
                &manifest_id,
                "turn:release-manifest",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "long_term_memory".to_string(),
                    source_id: "external_eval:release-artifact-anchor".to_string(),
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
            structured_query_facets: Vec::new(),
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
        .any(|candidate| candidate == &anchor_id));
    assert!(recall
        .graph_rerank
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == &manifest_id));
    assert!(
        recall
            .graph_rerank
            .graph_neighbor_ids
            .iter()
            .any(|candidate| candidate == &manifest_id),
        "{:#?}",
        recall.graph_rerank
    );
    let manifest_evidence_group = canonical_recall_evidence_group("turn:release-manifest");
    assert!(recall
        .compact_graph
        .nodes
        .iter()
        .any(|node| node.node_id == manifest_id
            && node
                .evidence_refs
                .iter()
                .any(|evidence_ref| evidence_ref == &manifest_evidence_group)));
    assert!(recall
        .compact_graph
        .nodes
        .iter()
        .flat_map(|node| node.evidence_refs.iter())
        .chain(
            recall
                .compact_graph
                .edges
                .iter()
                .flat_map(|edge| edge.evidence_refs.iter()),
        )
        .all(|reference| reference.starts_with("opaque:recall-group:sha256:")));
    assert!(recall
        .graph_candidate_evidence_ref_index
        .iter()
        .flat_map(|entry| entry.evidence_refs.iter())
        .all(|reference| reference.starts_with("opaque:recall-group:sha256:")));
    assert!(!format!("{recall:?}").contains("turn:release-manifest"));

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
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
        .any(|candidate| candidate.candidate_id == manifest_id
            && candidate
                .graph_neighbor_ids
                .iter()
                .any(|neighbor| neighbor == &anchor_id)));
    assert!(report
        .reranked_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == manifest_id
            && candidate
                .evidence_refs
                .iter()
                .any(|evidence_ref| evidence_ref == &manifest_evidence_group)));
    assert!(report
        .eval_candidate_pool
        .iter()
        .flat_map(|candidate| candidate.evidence_refs.iter())
        .chain(
            report
                .evidence_ref_index
                .iter()
                .flat_map(|entry| entry.evidence_refs.iter()),
        )
        .chain(
            report
                .compact_graph
                .nodes
                .iter()
                .flat_map(|node| node.evidence_refs.iter()),
        )
        .chain(
            report
                .compact_graph
                .edges
                .iter()
                .flat_map(|edge| edge.evidence_refs.iter()),
        )
        .all(|reference| reference.starts_with("opaque:recall-group:sha256:")));
    assert!(report.metrics.any_evidence_hit);
    assert!(report.metrics.all_evidence_hit);
    assert!(report.missing_evidence_refs.is_empty());
    assert!(!report
        .source_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == manifest_id));
    assert!(!report
        .rendered_block_preview
        .contains("Release manifest check"));
}

#[test]
fn persistent_graph_storage_is_isolated_by_memory_space_and_subject() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_identity_scope_and_subject(
        platform.clone(),
        profile,
        "agent-a",
        "owner-a",
        "subject-a",
        "local",
        "chat-a",
    );
    let runtime_b = test_runtime_with_identity_scope_and_subject(
        platform.clone(),
        profile,
        "agent-b",
        "owner-a",
        "subject-b",
        "local",
        "chat-b",
    );
    let runtime_c = test_runtime_with_identity_scope_and_subject(
        platform.clone(),
        profile,
        "agent-c",
        "owner-c",
        "subject-a",
        "local",
        "chat-c",
    );
    assert_eq!(runtime_a.memory_space_id(), runtime_b.memory_space_id());
    assert_ne!(runtime_a.subject_id(), runtime_b.subject_id());
    assert_ne!(runtime_a.memory_space_id(), runtime_c.memory_space_id());
    assert_eq!(runtime_a.subject_id(), runtime_c.subject_id());
    seed_governed_long_term(
        &runtime_a,
        vec![
            long_term_draft(
                "space alpha anchor",
                "Alpha runtime owns the alpha graph anchor.",
                "external_eval:space-alpha",
            ),
            long_term_draft(
                "alpha neighbor owner",
                "ALPHA_GRAPH_NEIGHBOR_SENTINEL has a governed long-term owner.",
                "turn:space-alpha-neighbor",
            ),
        ],
    );
    seed_governed_long_term(
        &runtime_b,
        vec![
            long_term_draft(
                "space beta anchor",
                "Beta runtime owns the beta graph anchor.",
                "external_eval:space-beta",
            ),
            long_term_draft(
                "beta neighbor owner",
                "BETA_GRAPH_NEIGHBOR_SENTINEL has a governed long-term owner.",
                "turn:space-beta-neighbor",
            ),
        ],
    );
    seed_governed_long_term(
        &runtime_c,
        vec![
            long_term_draft(
                "space gamma anchor",
                "Gamma runtime owns the gamma graph anchor.",
                "external_eval:space-gamma",
            ),
            long_term_draft(
                "gamma neighbor owner",
                "GAMMA_GRAPH_NEIGHBOR_SENTINEL has a governed long-term owner.",
                "turn:space-gamma-neighbor",
            ),
        ],
    );
    let owner_a_records = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-a")
        .expect("scoped long-term read store")
        .list(10)
        .expect("owner-a graph owner records");
    let owner_c_records = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-c")
        .expect("scoped long-term read store")
        .list(10)
        .expect("owner-c graph owner records");
    let alpha_id = owner_a_records
        .iter()
        .find(|entry| entry.content.contains("Alpha runtime owns"))
        .expect("alpha owner")
        .id
        .clone();
    let beta_id = owner_a_records
        .iter()
        .find(|entry| entry.content.contains("Beta runtime owns"))
        .expect("beta owner")
        .id
        .clone();
    let gamma_id = owner_c_records
        .iter()
        .find(|entry| entry.content.contains("Gamma runtime owns"))
        .expect("gamma owner")
        .id
        .clone();
    let alpha_neighbor_id = owner_a_records
        .iter()
        .find(|entry| entry.content.contains("ALPHA_GRAPH_NEIGHBOR_SENTINEL"))
        .expect("alpha neighbor owner")
        .id
        .clone();
    let beta_neighbor_id = owner_a_records
        .iter()
        .find(|entry| entry.content.contains("BETA_GRAPH_NEIGHBOR_SENTINEL"))
        .expect("beta neighbor owner")
        .id
        .clone();
    let gamma_neighbor_id = owner_c_records
        .iter()
        .find(|entry| entry.content.contains("GAMMA_GRAPH_NEIGHBOR_SENTINEL"))
        .expect("gamma neighbor owner")
        .id
        .clone();

    let write_graph = |runtime: &bm_sdk::MemoryRuntime,
                       anchor_id: &str,
                       anchor_ref: &str,
                       neighbor_id: &str,
                       neighbor_label: &str,
                       neighbor_ref: &str| {
        runtime
            .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
                operation: "memory_graph.write".to_string(),
                nodes: vec![
                    graph_node(
                        anchor_id,
                        MemoryGraphNodeKind::MemoryRecord,
                        anchor_id,
                        anchor_ref,
                    ),
                    graph_node(
                        neighbor_id,
                        MemoryGraphNodeKind::MemoryRecord,
                        neighbor_label,
                        neighbor_ref,
                    ),
                ],
                edges: vec![graph_edge(
                    &format!("edge:{anchor_id}:shared-neighbor"),
                    anchor_id,
                    neighbor_id,
                    neighbor_ref,
                )],
                backlinks: vec![
                    EvidenceBacklink {
                        source_kind: "long_term_memory".to_string(),
                        source_id: anchor_ref.to_string(),
                        fingerprint: format!("fp:{anchor_id}"),
                    },
                    EvidenceBacklink {
                        source_kind: "conversation_transcript".to_string(),
                        source_id: neighbor_ref.to_string(),
                        fingerprint: format!("fp:{neighbor_ref}"),
                    },
                ],
            })
            .expect("scoped graph write")
    };
    assert!(
        write_graph(
            &runtime_a,
            &alpha_id,
            "external_eval:space-alpha",
            &alpha_neighbor_id,
            "ALPHA_GRAPH_NEIGHBOR_SENTINEL",
            "turn:space-alpha-neighbor",
        )
        .accepted
    );
    assert!(
        write_graph(
            &runtime_c,
            &gamma_id,
            "external_eval:space-gamma",
            &gamma_neighbor_id,
            "GAMMA_GRAPH_NEIGHBOR_SENTINEL",
            "turn:space-gamma-neighbor",
        )
        .accepted
    );
    assert!(
        write_graph(
            &runtime_b,
            &beta_id,
            "external_eval:space-beta",
            &beta_neighbor_id,
            "BETA_GRAPH_NEIGHBOR_SENTINEL",
            "turn:space-beta-neighbor",
        )
        .accepted
    );

    let alpha_recall = runtime_a
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "space alpha anchor".to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .expect("alpha scoped graph recall");
    assert!(alpha_recall
        .compact_graph
        .nodes
        .iter()
        .any(|node| node.label == "ALPHA_GRAPH_NEIGHBOR_SENTINEL"));
    assert!(!alpha_recall
        .compact_graph
        .nodes
        .iter()
        .any(|node| node.label == "BETA_GRAPH_NEIGHBOR_SENTINEL"));
    assert!(!alpha_recall
        .compact_graph
        .nodes
        .iter()
        .any(|node| node.label == "GAMMA_GRAPH_NEIGHBOR_SENTINEL"));
    assert!(alpha_recall
        .graph_index_report
        .failures
        .iter()
        .all(|failure| !failure.contains("scope")));
}

#[test]
fn eval_recall_reports_w41_diagnostics_without_expanding_prompt_pool() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);
    let anchor = long_term_draft(
        "release artifact guard",
        "Release artifact guard is the visible governed graph anchor.",
        "external_eval:release-guard-owner",
    );
    let mut manifest = long_term_draft(
        "manifest gold neighbor",
        "Gold checksum evidence is available only through graph expansion.",
        "turn:release-manifest",
    );
    manifest.keywords = vec!["manifest-checksum".to_string()];
    let mut owners = vec![anchor, manifest];
    owners.extend(release_artifact_source_decoys("w41"));
    seed_governed_long_term(&runtime, owners);
    let records = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store")
        .list(usize::MAX)
        .expect("list graph owners");
    let anchor_id = records
        .iter()
        .find(|entry| entry.content.contains("visible governed graph anchor"))
        .expect("anchor owner")
        .id
        .clone();
    let manifest_id = records
        .iter()
        .find(|entry| entry.content.contains("Gold checksum evidence"))
        .expect("manifest owner")
        .id
        .clone();

    runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &anchor_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release artifact guard",
                    "external_eval:release-guard-owner",
                ),
                graph_node(
                    &manifest_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release manifest check",
                    "turn:release-manifest",
                ),
            ],
            edges: vec![graph_edge(
                "edge:release_guard:manifest_check",
                &anchor_id,
                &manifest_id,
                "turn:release-manifest",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "long_term_memory".to_string(),
                    source_id: "external_eval:release-guard-owner".to_string(),
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
            structured_query_facets: Vec::new(),
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
        .any(|candidate| candidate.candidate_id == anchor_id));
    assert!(report
        .eval_candidate_pool
        .iter()
        .any(|candidate| candidate.candidate_id == manifest_id));
    let manifest_evidence_group = canonical_recall_evidence_group("turn:release-manifest");
    assert!(report
        .evidence_ref_index
        .iter()
        .any(|entry| entry.candidate_id == manifest_id
            && entry
                .evidence_refs
                .iter()
                .any(|evidence_ref| evidence_ref == &manifest_evidence_group)));

    let rendered_candidate_ids = report
        .rendered_candidates
        .iter()
        .map(|candidate| candidate.candidate_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(rendered_candidate_ids, vec![anchor_id.clone()]);
    assert!(report.eval_candidate_pool.len() > report.rendered_candidates.len());
    assert!(report.eval_candidate_pool.iter().any(|candidate| {
        !rendered_candidate_ids
            .iter()
            .any(|rendered| rendered == &candidate.candidate_id)
    }));
    assert!(!report
        .rendered_block_preview
        .contains("Release manifest check"));
    assert_eq!(
        report
            .stage_diagnostics
            .loss_ledger
            .expanded_hit_selected_miss
            .len(),
        1
    );

    let diagnostics = &report.stage_diagnostics;
    assert_eq!(diagnostics.suite, "unit_w41");
    assert_eq!(diagnostics.question_id, "release-w41-q");
    assert_eq!(diagnostics.question_type, "temporal_update");
    assert_eq!(diagnostics.evidence_count, 1);
    assert_eq!(
        diagnostics.gold_evidence_refs,
        vec![canonical_recall_evidence_group("turn:release-manifest")]
    );
    assert_eq!(diagnostics.first_any_hit_stage.as_deref(), Some("expanded"));
    assert_eq!(diagnostics.first_all_hit_stage.as_deref(), Some("expanded"));
    assert!(!diagnostics.miss_after_expanded);
    assert!(diagnostics
        .matched_gold_by_stage
        .iter()
        .any(|stage| stage.stage == "source" && stage.evidence_refs.is_empty()));
    assert!(diagnostics
        .missing_gold_by_stage
        .iter()
        .any(|stage| stage.stage == "source"
            && stage.evidence_refs.iter().any(|evidence_ref| evidence_ref
                == &canonical_recall_evidence_group("turn:release-manifest"))));
    assert!(diagnostics
        .gold_rank_by_stage
        .iter()
        .any(|rank| rank.stage == "expanded"
            && rank.evidence_ref == canonical_recall_evidence_group("turn:release-manifest")
            && rank.rank.is_some()));
    assert!(
        diagnostics
            .graph_distance_to_gold
            .iter()
            .any(|distance| distance.candidate_id == manifest_id
                && distance.evidence_ref
                    == canonical_recall_evidence_group("turn:release-manifest")
                && distance.distance == Some(1)),
        "{diagnostics:#?}"
    );
    assert!(diagnostics
        .source_anchor_ids
        .iter()
        .any(|candidate| candidate == &anchor_id));
    assert!(diagnostics
        .graph_anchor_candidate_ids
        .iter()
        .any(|candidate| candidate == &anchor_id));
    assert!(diagnostics
        .expanded_node_ids
        .iter()
        .any(|candidate| candidate == &manifest_id));
    assert!(diagnostics
        .graph_neighbor_ids
        .iter()
        .any(|candidate| candidate == &manifest_id));
    assert_eq!(diagnostics.truncated_count, 0);
    assert!(diagnostics.blocked_reasons.is_empty());
    assert_eq!(
        diagnostics.selected_candidate_ids,
        report
            .selected_candidates
            .iter()
            .map(|candidate| candidate.candidate_id.clone())
            .collect::<Vec<_>>()
    );
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
    let store = platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:owner-default")
        .expect("scoped long-term read store");

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
    let drafts = topics
        .iter()
        .map(|(topic, content, citation)| {
            let mut draft = long_term_draft(topic, content, citation);
            draft.privacy = MemoryPrivacyClass::PublicRuntime;
            draft
        })
        .collect();
    seed_governed_long_term(&runtime, drafts);
    let initial_source_ids = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "release".to_string(),
            limit: 5,
            tool_registry_refs: Vec::new(),
        })
        .expect("initial source recall")
        .source_candidate_ids;
    let target_anchor = store
        .list(20)
        .expect("list long-term")
        .into_iter()
        .find(|entry| !initial_source_ids.contains(&entry.id))
        .expect("target anchor outside prompt source pool");
    let target_anchor_id = target_anchor.id;
    let target_anchor_ref = target_anchor
        .supporting_citations
        .first()
        .expect("target anchor citation")
        .clone();
    let target_gold = long_term_draft(
        "gold evidence neighbor",
        "Target gold evidence is reachable only from its governed graph owner.",
        "external_eval:target-gold",
    );
    let target_gold_id = target_gold.stable_id().expect("target gold owner id");
    seed_governed_long_term(&runtime, vec![target_gold]);

    runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &target_anchor_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release target acme manifest exception",
                    &target_anchor_ref,
                ),
                graph_node(
                    &target_gold_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release target gold evidence",
                    "external_eval:target-gold",
                ),
            ],
            edges: vec![graph_edge(
                "edge:release-target-anchor:gold",
                &target_anchor_id,
                &target_gold_id,
                "external_eval:target-gold",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "accepted_long_term_revision".to_string(),
                    source_id: target_anchor_ref,
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
            structured_query_facets: Vec::new(),
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
    let target_gold_evidence_group = canonical_recall_evidence_group("external_eval:target-gold");
    assert!(report
        .eval_candidate_pool
        .iter()
        .any(|candidate| candidate.candidate_id == target_gold_id
            && candidate
                .evidence_refs
                .iter()
                .any(|evidence_ref| evidence_ref == &target_gold_evidence_group)));
    assert_eq!(
        report.stage_diagnostics.first_any_hit_stage.as_deref(),
        Some("expanded")
    );
    assert!(report
        .rendered_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == target_anchor_id));
    assert!(report
        .rendered_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == target_gold_id));
    assert_eq!(report.delivery_report.render_growth, 0);
    assert!(report.graph_index_report.used);
    assert!(!report.graph_index_report.fallback_full_scan);
}

#[test]
fn eval_recall_reports_facet_stage_for_expanded_miss() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform.clone(), ProfileId::ServerLinuxDevFull);
    seed_governed_long_term(
        &runtime,
        vec![long_term_draft(
            "release baseline source",
            "release baseline source is visible without facet expansion",
            "external_eval:baseline-source",
        )],
    );

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
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
        .any(|evidence_ref| evidence_ref
            == &canonical_recall_evidence_group("external_eval:facet-only-gold")));
    assert!(report.facet_index_report.used);
    assert!(!report.facet_index_report.report_only);
    assert!(!report.facet_index_report.fallback_full_scan);
    assert!(report.facet_index_report.manifest_integrity_verified);
    assert!(!facet_stage
        .blocked_reasons
        .iter()
        .any(|reason| reason == "memory_facet_index_not_loaded"));

    let required = [
        "facet_off",
        "rank_fusion_off",
        "coverage_selection_off",
        "delivery_relevance_fusion_off",
        "evidence_family_rotation_off",
        "render_capsule_off",
        "capsule_dedupe_off",
    ];
    assert_eq!(report.ablation_report.method, "sdk_eval_recall_off_run_v1");
    for name in required {
        assert!(report
            .ablation_report
            .slices
            .iter()
            .any(|slice| slice.name == name && slice.report_available && !slice.feature_enabled));
    }
    assert_eq!(report.ablation_report.render_growth, 0);
    let baseline_selected_candidate_ids = report
        .selected_candidates
        .iter()
        .map(|candidate| candidate.candidate_id.clone())
        .collect::<Vec<_>>();
    let baseline_rendered_candidate_ids = report
        .rendered_candidates
        .iter()
        .map(|candidate| candidate.candidate_id.clone())
        .collect::<Vec<_>>();
    for slice in &report.ablation_report.slices {
        assert_eq!(slice.render_growth, 0);
        assert_eq!(
            slice.baseline_selected_candidate_ids,
            baseline_selected_candidate_ids
        );
        assert_eq!(
            slice.baseline_rendered_candidate_ids,
            baseline_rendered_candidate_ids
        );
        assert_eq!(
            slice.baseline_selected_candidate_ids.len(),
            slice.baseline_selected_candidate_count
        );
        assert_eq!(
            slice.off_run_selected_candidate_ids.len(),
            slice.off_run_selected_candidate_count
        );
        assert_eq!(
            slice.baseline_rendered_candidate_ids.len(),
            slice.baseline_rendered_candidate_count
        );
        assert_eq!(
            slice.off_run_rendered_candidate_ids.len(),
            slice.off_run_rendered_candidate_count
        );
        let _selected_hit_delta: i64 = slice.selected_evidence_hit_delta;
        let _rendered_hit_delta: i64 = slice.rendered_evidence_hit_delta;
        let _selected_all_hit_lost: bool = slice.selected_all_hit_lost;
        let _rendered_all_hit_lost: bool = slice.rendered_all_hit_lost;
        let _expanded_candidate_delta: i64 = slice.expanded_candidate_delta;
        let _selected_candidate_delta: i64 = slice.selected_candidate_delta;
        let _rendered_candidate_delta: i64 = slice.rendered_candidate_delta;
        let _rendered_char_delta: i64 = slice.rendered_char_delta;
    }
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
            structured_query_facets: Vec::new(),
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
    assert!(report.facet_index_report.exact_facet_match_count > 0);
    assert!(report.facet_index_report.expanded_facet_match_count > 0);
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
            structured_query_facets: Vec::new(),
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
            structured_query_facets: Vec::new(),
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

    assert!(!beta.facet_index_report.used);
    assert!(beta
        .facet_index_report
        .failures
        .iter()
        .any(|failure| failure == "memory_facet_index_not_loaded"));
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
            structured_query_facets: Vec::new(),
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
    budget.recall_delivery_budget.max_selected_candidates = 2;
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
            structured_query_facets: Vec::new(),
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
    let shared_group = canonical_recall_evidence_group("external_eval:coverage-shared|turn=1");
    let distinct_group = canonical_recall_evidence_group("external_eval:coverage-distinct");
    assert!(coverage.used);
    assert_eq!(coverage.strategy, "evidence_group_coverage_v1");
    assert_eq!(coverage.selected_candidate_ids.len(), 2);
    assert!(coverage
        .covered_evidence_groups
        .iter()
        .any(|group| group == &shared_group));
    assert!(coverage
        .covered_evidence_groups
        .iter()
        .any(|group| group == &distinct_group));
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
    assert_eq!(
        report.delivery_report.selection_strategy,
        "profile_bounded_exact_evidence_coverage_with_relevance_fusion_v2"
    );
    assert_eq!(report.delivery_report.selected_candidate_ids.len(), 2);
    let delivered_groups = report
        .delivery_report
        .selection_decisions
        .iter()
        .filter(|decision| decision.selected)
        .flat_map(|decision| decision.canonical_evidence_groups.iter())
        .collect::<Vec<_>>();
    assert!(delivered_groups
        .iter()
        .any(|group| group.as_str() == shared_group));
    assert!(delivered_groups
        .iter()
        .any(|group| group.as_str() == distinct_group));
}

#[test]
fn facet_graph_anchor_selection_never_backfills_duplicate_evidence_group() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    budget.graph_expansion_budget.max_seed_candidates = 8;
    budget.facet_recall_budget.max_facet_anchor_candidates = 8;
    budget.facet_recall_budget.max_facet_expanded_candidates = 8;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform,
        profile,
        "llm.gateway",
        "facet-no-backfill",
        "subject-no-backfill",
        budget,
    );
    seed_governed_long_term(
        &runtime,
        vec![
            long_term_draft(
                "facet/no-backfill/primary",
                "Primary duplicate-group evidence.",
                "external_eval:no-backfill-shared|session_1",
            ),
            long_term_draft(
                "facet/no-backfill/secondary",
                "Secondary duplicate-group evidence.",
                "external_eval:no-backfill-shared|session_2",
            ),
        ],
    );

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
            query: "facet no backfill".to_string(),
            k: 8,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_w4_facet".to_string(),
                question_id: "facet-no-backfill".to_string(),
                question_type: "single_gold".to_string(),
                expected_evidence_refs: vec![
                    "external_eval:no-backfill-shared|session_1".to_string()
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("eval recall");

    assert_eq!(
        report
            .coverage_selection_report
            .selected_candidate_ids
            .len(),
        1
    );
    assert_eq!(
        report
            .coverage_selection_report
            .coverage_dropped_candidate_ids
            .len(),
        1
    );
}

#[test]
fn production_delivery_enforces_profile_capsule_character_ceiling() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    budget.recall_delivery_budget.max_selected_candidates = 2;
    budget.recall_delivery_budget.max_rendered_capsules = 2;
    budget.recall_delivery_budget.max_capsule_chars = 64;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform,
        profile,
        "llm.gateway",
        "capsule-budget",
        "subject-capsule-budget",
        budget,
    );
    seed_governed_long_term(
        &runtime,
        vec![
            long_term_draft(
                "capsule budget alpha",
                "Capsule budget alpha has deliberately long governed evidence content that must be truncated by the profile-owned delivery ceiling.",
                "external_eval:capsule-budget-alpha",
            ),
            long_term_draft(
                "capsule budget beta",
                "Capsule budget beta has separate deliberately long governed evidence content that must share the same fixed delivery ceiling.",
                "external_eval:capsule-budget-beta",
            ),
        ],
    );

    let report = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "capsule budget governed evidence".to_string(),
            limit: 2,
            tool_registry_refs: Vec::new(),
        })
        .expect("production recall with capsule budget");

    assert_eq!(report.delivery_report.render_budget_chars, 64);
    assert!(report.delivery_report.rendered_chars <= 64);
    assert_eq!(report.delivery_report.render_growth, 0);
    assert!(report.delivery_report.integrity_failures.is_empty());
    assert!(report
        .delivery_report
        .delivery_drop_reasons
        .iter()
        .all(|reason| !reason.contains("read_failed") && !reason.contains("decode_failed")));
    assert_eq!(
        report.delivery_report.rendered_chars,
        report
            .delivery_report
            .rendered_capsules
            .iter()
            .map(|capsule| capsule.content.chars().count())
            .sum::<usize>()
    );
    assert!(report
        .delivery_report
        .rendered_capsules
        .iter()
        .all(|capsule| capsule.rendered_chars == capsule.content.chars().count()));

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "capsule budget governed evidence".to_string(),
            system_max_len: 512,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection with tighter final render ceiling");
    let selected_ids = projection
        .recall_delivery_report
        .selection_decisions
        .iter()
        .filter(|decision| decision.selected)
        .map(|decision| decision.candidate_id.clone())
        .collect::<Vec<_>>();
    let shared_factual_audit = projection
        .audit
        .sources
        .iter()
        .find(|source| source.plane == "shared_factual")
        .expect("shared factual projection audit");
    assert_eq!(shared_factual_audit.selected_ids, selected_ids);
    assert_eq!(shared_factual_audit.selected_count, selected_ids.len());
    assert!(selected_ids.len() >= projection.recall_delivery_report.rendered_capsules.len());
}

#[test]
fn capsule_dedupe_assigns_each_exact_evidence_group_to_one_primary_capsule() {
    let platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);
    let mut first = long_term_draft(
        "overlap evidence alpha",
        "Overlap evidence alpha carries the first and shared proof.",
        "external_eval:D1:1",
    );
    first
        .supporting_citations
        .push("external_eval:D1:2".to_string());
    first
        .supporting_citations
        .push("external_eval:D1:1|session_1".to_string());
    let mut second = long_term_draft(
        "overlap evidence beta",
        "Overlap evidence beta carries the shared and third proof.",
        "external_eval:D1:2",
    );
    second
        .supporting_citations
        .push("external_eval:D1:3".to_string());
    seed_governed_long_term(&runtime, vec![first, second]);

    let report = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
            query: "overlap evidence proof".to_string(),
            k: 2,
            include_expanded_candidates: true,
            include_graph_neighbors: true,
            include_score_breakdown: true,
            include_missing_evidence: true,
            benchmark_context: Some(MemoryEvalRecallBenchmarkContext {
                suite: "unit_p7".to_string(),
                question_id: "partial-overlap-dedupe".to_string(),
                question_type: "multi_gold".to_string(),
                expected_evidence_refs: vec![
                    "external_eval:D1:1".to_string(),
                    "external_eval:D1:2".to_string(),
                    "external_eval:D1:3".to_string(),
                ],
            }),
            tool_registry_refs: Vec::new(),
        })
        .expect("overlap capsule delivery");

    let delivered_groups = report
        .delivery_report
        .rendered_capsules
        .iter()
        .flat_map(|capsule| capsule.canonical_evidence_groups.iter().cloned())
        .collect::<Vec<_>>();
    assert!(report
        .delivery_report
        .selection_decisions
        .iter()
        .all(|decision| {
            decision.canonical_evidence_groups.len()
                == decision
                    .canonical_evidence_groups
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
        }));
    assert_eq!(report.delivery_report.selected_candidate_ids.len(), 1);
    assert!(report
        .delivery_report
        .selection_decisions
        .iter()
        .any(|decision| {
            !decision.selected
                && decision.drop_reason
                    == Some(RecallDeliverySelectionDropReason::DuplicateEvidenceGroup)
        }));
    assert_eq!(delivered_groups.len(), 2);
    assert_eq!(
        delivered_groups
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        report
            .stage_diagnostics
            .loss_ledger
            .canonical_evidence_group_coverage
            .duplicate_rendered_group_count,
        0
    );
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
            structured_query_facets: Vec::new(),
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

    assert!(!beta.facet_index_report.used);
    assert!(beta
        .facet_index_report
        .failures
        .iter()
        .any(|failure| failure == "memory_facet_index_not_loaded"));
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
    assert!(
        !beta.ablation_report.delivery_contribution_proven,
        "{:#?}",
        beta.ablation_report
    );
    assert!(beta.ablation_report.blocked_reasons.is_empty());
    let facet_off = beta
        .ablation_report
        .slices
        .iter()
        .find(|slice| slice.name == "facet_off")
        .expect("facet_off ablation slice");
    assert!(!facet_off.delivery_contribution_proven);
    assert_eq!(facet_off.delivery_affected_candidate_count, 0);
    assert!(facet_off.blocked_reasons.is_empty());
}

#[test]
fn delegated_actor_is_not_added_to_shared_memory_owner_subjects() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let actor_subject_id = default_agent_subject_id("agent-actor");
    let mounted_subject_id = default_agent_subject_id("agent-delegated");
    let delegated_runtime = test_runtime_with_delegated_actor(
        platform.clone(),
        profile,
        "agent-delegated",
        &actor_subject_id,
        "delegated-write",
    );
    seed_governed_long_term(
        &delegated_runtime,
        vec![long_term_draft(
            "delegated owner evidence",
            "Delegated writes remain owned by the mounted subject, not the acting subject.",
            "external_eval:delegated-owner",
        )],
    );

    let mounted_runtime = test_runtime_with_scope_and_subject(
        platform.clone(),
        profile,
        "llm.gateway",
        "mounted-read",
        &mounted_subject_id,
    );
    let actor_runtime = test_runtime_with_identity_scope_and_subject(
        platform,
        profile,
        "agent-actor",
        "owner-default",
        &actor_subject_id,
        "llm.gateway",
        "actor-read",
    );

    let mounted = mounted_runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "delegated owner evidence".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("mounted subject recall");
    let actor = actor_runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "delegated owner evidence".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("actor subject recall");

    assert!(!mounted.delivery_report.rendered_capsules.is_empty());
    assert!(actor.delivery_report.rendered_capsules.is_empty());
    assert!(actor
        .working
        .long_term_memory_text
        .as_deref()
        .is_none_or(|text| !text.contains("Delegated writes remain owned")));
}

#[test]
fn subject_local_capsule_never_enters_cross_subject_shared_fact_surface() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime(platform, profile);
    let content = "Subject-local release evidence must stay out of the shared fact surface.";
    seed_governed_long_term(
        &runtime,
        vec![long_term_draft(
            "subject local surface evidence",
            content,
            "external_eval:subject-local-surface",
        )],
    );

    let projection = runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "subject local surface evidence".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project subject-local evidence");

    assert!(projection.system_memory_block.contains(content));
    assert!(!projection
        .projection_surfaces
        .shared_fact_surface
        .contains(content));
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
    let alpha_draft = long_term_draft(
        "distinct alpha graph owner",
        "Alpha governed evidence is reachable through the persistent graph.",
        "external_eval:facet-p4-distinct-alpha",
    );
    let alpha_id = alpha_draft.stable_id().expect("alpha owner id");
    let beta_draft = long_term_draft(
        "distinct beta graph owner",
        "Beta governed evidence is reachable through the persistent graph.",
        "external_eval:facet-p4-distinct-beta",
    );
    let beta_id = beta_draft.stable_id().expect("beta owner id");

    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: bm_sdk::ParsedLongTermMemoryExtraction {
                upserts: vec![anchor_draft, alpha_draft, beta_draft],
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
                    &alpha_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Facet P4 distinct alpha evidence",
                    "external_eval:facet-p4-distinct-alpha",
                ),
                graph_node(
                    &beta_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Facet P4 distinct beta evidence",
                    "external_eval:facet-p4-distinct-beta",
                ),
            ],
            edges: vec![
                graph_edge(
                    "edge:facet-p4-anchor-alpha",
                    &anchor_id,
                    &alpha_id,
                    "external_eval:facet-p4-distinct-alpha",
                ),
                graph_edge(
                    "edge:facet-p4-anchor-beta",
                    &anchor_id,
                    &beta_id,
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
            structured_query_facets: Vec::new(),
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
        .any(|candidate| candidate.candidate_id == alpha_id));
    assert!(report
        .expanded_candidates
        .iter()
        .any(|candidate| candidate.candidate_id == beta_id));
    let alpha = report
        .eval_candidate_pool
        .iter()
        .find(|candidate| candidate.candidate_id == alpha_id)
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
    let anchor = long_term_draft(
        "release artifact indexed anchor",
        "Release artifact indexed anchor owns the production graph index.",
        "external_eval:release-index-anchor",
    );
    let anchor_id = anchor.stable_id().expect("index anchor id");
    let manifest = long_term_draft(
        "indexed manifest neighbor",
        "Manifest evidence has a visible long-term owner.",
        "turn:release-manifest",
    );
    let manifest_id = manifest.stable_id().expect("manifest owner id");
    seed_governed_long_term(&runtime, vec![anchor, manifest]);

    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &anchor_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release artifact guard",
                    "external_eval:release-index-anchor",
                ),
                graph_node(
                    &manifest_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release manifest check",
                    "turn:release-manifest",
                ),
            ],
            edges: vec![graph_edge(
                "edge:release_guard:manifest_check",
                &anchor_id,
                &manifest_id,
                "turn:release-manifest",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "long_term_memory".to_string(),
                    source_id: "external_eval:release-index-anchor".to_string(),
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
    assert_eq!(graph_write.manifest_generation, Some(1));
    assert!(graph_write.graph_revision.is_some());

    let snapshot = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot");
    assert!(snapshot.json_docs.iter().any(|doc| {
        doc.namespace == "memory_graph_indexes"
            && doc.value["owner"] == "bm-sdk::MemoryRuntime"
            && doc.value["source_anchor_id"] == anchor_id
            && doc.value["schema_version"] == 2
            && doc.value["owner_revision"] == 1
            && doc.value["manifest_generation"] == 1
            && doc.value["node_memberships"]
                .as_array()
                .is_some_and(|items| items.len() == 2)
            && doc.value.get("neighbor_node_ids").is_none()
    }));

    let recall = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
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
        .any(|source| source == &anchor_id));
    assert!(recall
        .graph_index_report
        .expanded_node_ids
        .iter()
        .any(|node| node == &manifest_id));

    let eval = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
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
    let anchor = long_term_draft(
        "release artifact large index anchor",
        "Release artifact large index anchor owns the graph seed.",
        "external_eval:release-large-index-anchor",
    );
    let anchor_id = anchor.stable_id().expect("large index anchor id");
    let manifest = long_term_draft(
        "large index manifest owner",
        "Manifest graph evidence has a governed owner.",
        "turn:release-manifest",
    );
    let manifest_id = manifest.stable_id().expect("manifest owner id");
    let policy = long_term_draft(
        "large index policy owner",
        "Policy graph evidence has a governed owner.",
        "turn:release-policy",
    );
    let policy_id = policy.stable_id().expect("policy owner id");
    let unrelated_audit = long_term_draft(
        "unrelated audit owner",
        "Unrelated audit graph evidence remains separately governed.",
        "turn:unrelated-audit",
    );
    let unrelated_audit_id = unrelated_audit.stable_id().expect("audit owner id");
    let unrelated_receipt = long_term_draft(
        "unrelated receipt owner",
        "Unrelated receipt graph evidence remains separately governed.",
        "turn:unrelated-receipt",
    );
    let unrelated_receipt_id = unrelated_receipt.stable_id().expect("receipt owner id");
    seed_governed_long_term(
        &runtime,
        vec![anchor, manifest, policy, unrelated_audit, unrelated_receipt],
    );

    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &anchor_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release artifact guard",
                    "external_eval:release-large-index-anchor",
                ),
                graph_node(
                    &manifest_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release manifest check",
                    "turn:release-manifest",
                ),
                graph_node(
                    &policy_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release policy check",
                    "turn:release-policy",
                ),
                graph_node(
                    &unrelated_audit_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Unrelated audit",
                    "turn:unrelated-audit",
                ),
                graph_node(
                    &unrelated_receipt_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Unrelated receipt",
                    "turn:unrelated-receipt",
                ),
            ],
            edges: vec![
                graph_edge(
                    "edge:release_guard:manifest_check",
                    &anchor_id,
                    &manifest_id,
                    "turn:release-manifest",
                ),
                graph_edge(
                    "edge:release_guard:policy_check",
                    &anchor_id,
                    &policy_id,
                    "turn:release-policy",
                ),
                graph_edge(
                    "edge:unrelated:audit_receipt",
                    &unrelated_audit_id,
                    &unrelated_receipt_id,
                    "turn:unrelated-audit",
                ),
            ],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "long_term_memory".to_string(),
                    source_id: "external_eval:release-large-index-anchor".to_string(),
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
            structured_query_facets: Vec::new(),
            query: "release artifact".to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .expect("indexed recall");

    let index = &recall.graph_index_report;
    assert_eq!(index.owner, "bm-sdk::MemoryRuntime");
    assert!(index.used, "{index:#?}");
    assert!(!index.fallback_full_scan);
    assert_eq!(index.source_candidate_count, 5);
    assert_eq!(index.matched_source_anchor_count, 5);
    assert!(index
        .source_anchor_ids
        .iter()
        .any(|source| source == &anchor_id));
    assert!(index.unmatched_source_anchor_ids.is_empty());
    assert_eq!(index.indexed_neighbor_count, 5);
    assert_eq!(index.filtered_node_count, 5);
    assert_eq!(index.filtered_edge_count, 3);
    assert_eq!(index.filtered_backlink_count, 5);

    let eval = runtime
        .eval_recall(MemoryEvalRecallRequest {
            structured_query_facets: Vec::new(),
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
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let mut budget = bm_sdk::RuntimeBudgetReport::static_for_profile(profile);
    budget.graph_expansion_budget.max_graph_nodes_loaded = 1;
    let runtime = test_runtime_with_scope_subject_and_budget(
        platform,
        profile,
        "llm.gateway",
        "graph-budget",
        &default_agent_subject_id("agent-main"),
        budget,
    );
    let anchor = long_term_draft(
        "release budget anchor",
        "Release budget anchor remains the governed source candidate.",
        "external_eval:release-budget-anchor",
    );
    let anchor_id = anchor.stable_id().expect("anchor id");
    let neighbor = long_term_draft(
        "release budget neighbor",
        "Release budget neighbor exceeds the bounded graph node load ceiling.",
        "turn:release-budget-neighbor",
    );
    let neighbor_id = neighbor.stable_id().expect("neighbor id");
    seed_governed_long_term(&runtime, vec![anchor, neighbor]);

    let graph_write = runtime
        .write_temporal_memory_graph(TemporalMemoryGraphWriteRequest {
            operation: "memory_graph.write".to_string(),
            nodes: vec![
                graph_node(
                    &anchor_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release budget anchor",
                    "external_eval:release-budget-anchor",
                ),
                graph_node(
                    &neighbor_id,
                    MemoryGraphNodeKind::MemoryRecord,
                    "Release budget neighbor",
                    "turn:release-budget-neighbor",
                ),
            ],
            edges: vec![graph_edge(
                "edge:release_budget:neighbor",
                &anchor_id,
                &neighbor_id,
                "turn:release-budget-neighbor",
            )],
            backlinks: vec![
                EvidenceBacklink {
                    source_kind: "long_term_memory".to_string(),
                    source_id: "external_eval:release-budget-anchor".to_string(),
                    fingerprint: "fp-release-budget-anchor".to_string(),
                },
                EvidenceBacklink {
                    source_kind: "conversation_transcript".to_string(),
                    source_id: "turn:release-budget-neighbor".to_string(),
                    fingerprint: "fp-release-budget-neighbor".to_string(),
                },
            ],
        })
        .expect("graph write report");
    assert!(graph_write.accepted, "{:?}", graph_write.gate_failures);

    let recall = runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "release budget anchor".to_string(),
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
    assert_eq!(recall.graph_index_report.filtered_node_count, 0);
    assert!(recall.graph_index_report.maintenance_required);
    assert_eq!(recall.graph_index_report.read_path_mutation_delta, 0);
}
