use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    build_memory_autopilot_gate_report, build_next_gen_contract_matrix,
    build_privacy_vault_gate_report, build_procedural_evolution_gate_report,
    build_relationship_boundary_audit_from_constitution_audit, build_soul_compact_digest,
    build_soul_feedback_report_from_turn_ledger,
    build_soul_growth_proposals_from_core_revision_ledger, build_soul_kernel2_gate_report,
    build_soul_regression_suite_report, build_temporal_memory_graph_gate_report,
    build_vault_migration_preflight, build_workbench_gate_report,
    compile_edge_memory_budget_report, governed_memory_recall_candidate_id,
    plan_memory_autopilot_for_profile, plan_temporal_memory_graph_write,
    promote_task_experience_to_procedure, rerank_recall_with_temporal_graph, CompactGraphIndex,
    CompactSoulProfile, CoreRevisionActionKind, CoreRevisionConflictClass, CoreRevisionLedger,
    CoreRevisionOutcome, CoreRevisionRecord, CoreRevisionRecordChange, DroppedProjectionCandidate,
    EdgeRecoveryFixture, EncryptedSnapshotEnvelope, EvidenceBacklink, GovernedMemoryOwnerPlane,
    GovernedMemoryOwnerRef, GraphRecallExpansionBudget, MemoryAutopilotInput, MemoryAutopilotPlan,
    MemoryGraphEdge, MemoryGraphEdgeKind, MemoryGraphEvidence, MemoryGraphNode,
    MemoryGraphNodeKind, MemoryHygieneDiff, NextGenPhase, PrivateMaterialRedactionReport,
    ProceduralMemoryPromotionInput, ProceduralMemoryPromotionPolicy, ProceduralMemoryRecordV2,
    ProcedureGenome, ProjectionBudgetDecision, ProjectionPrivacyDecision,
    RelationshipConstitutionAudit, SelfAuthoredCore, SkillEvolutionReport, SoulFeedbackReport,
    SoulGrowthDecision, SoulGrowthProposal, SoulRegressionSuite,
    SubjectProjectionBoundaryProtocolReport, SubjectProjectionMountReport, SubjectProjectionReport,
    SubjectProjectionWorkIntegrityReport, TemporalValidity, TurnSoulFeedbackLedger,
    TurnSoulInitiativeLedger, TurnSoulReplyLedger, TurnSoulStrategyLedger, VaultManifest,
    VaultMigrationPreflight, WorkbenchApiMap, WorkbenchSurface,
};

#[test]
fn next_gen_contract_matrix_covers_w2_to_w9_without_adapter_ownership() {
    let contracts = build_next_gen_contract_matrix(ProfileId::ServerLinuxDevFull);

    assert_eq!(contracts.len(), 8);
    for phase in NextGenPhase::ALL {
        let contract = contracts
            .iter()
            .find(|contract| contract.phase == phase)
            .expect("phase contract");
        assert!(!contract.outputs.is_empty(), "{phase:?}");
        assert!(!contract.benchmark_inputs.is_empty(), "{phase:?}");
        assert!(!contract.owner_layer.trim().is_empty(), "{phase:?}");
        assert!(contract
            .forbidden_owners
            .iter()
            .any(|owner| owner.contains("adapter") || owner.contains("UI")));
    }
}

#[test]
fn soul_growth_proposal_requires_evidence_privacy_and_surface() {
    let proposal = SoulGrowthProposal {
        proposal_id: "soul-growth-boundary-1".to_string(),
        profile: ProfileId::ServerLinuxDevFull,
        evidence_refs: vec!["turn-ledger:42".to_string()],
        conflict_classes: vec![CoreRevisionConflictClass::BoundaryConflict],
        privacy_decision: "safe_summary_only".to_string(),
        affected_surfaces: vec!["reply".to_string(), "strategy".to_string()],
        decision: SoulGrowthDecision::Deferred,
        reason: "Needs repeated evidence before core revision.".to_string(),
    };

    assert!(proposal.validate_contract().accepted);

    let mut invalid = proposal;
    invalid.evidence_refs.clear();
    assert_eq!(
        invalid.validate_contract().reason,
        "soul_growth_evidence_refs_empty"
    );
}

#[test]
fn subject_projection_report_tracks_budget_privacy_and_dropped_candidates() {
    let report = SubjectProjectionReport {
        projection_id: "projection-1".to_string(),
        profile: ProfileId::ServerLinuxDevFull,
        subject_mount: SubjectProjectionMountReport {
            identity_mount: "Subject Mount | Beetle Memory SDK runtime".to_string(),
            relationship_position: "engineering collaborator".to_string(),
            situated_now: "W1 benchmark wall integration".to_string(),
            current_reasoning_basis: "fixture:subject-projection-full-baseline".to_string(),
            reply_stance: "work-first direct reply".to_string(),
            initiative_posture: "continue without theatrical drift".to_string(),
            boundary_mode: "privacy protocol active".to_string(),
            degraded_reason: None,
        },
        boundary_protocol: SubjectProjectionBoundaryProtocolReport {
            runtime_private_context_allowed: true,
            foreground_disclosure_allowed: false,
            protected_sources: vec!["private_garden".to_string()],
            disclosure_rule: "private raw material is never projected".to_string(),
            final_llm_privacy_judge_allowed: false,
        },
        work_integrity: SubjectProjectionWorkIntegrityReport {
            task_goal: "W1 benchmark wall integration".to_string(),
            evidence_ceiling: "fixture evidence only".to_string(),
            tool_permission_boundary: "respect SDK capability policy".to_string(),
            uncertainty_rule: "state uncertainty instead of inventing memory".to_string(),
            no_obstruction_rule: "do not obstruct user work".to_string(),
        },
        identity_mount: "Beetle Memory SDK runtime".to_string(),
        relationship_position: "engineering collaborator".to_string(),
        situated_now: "W1 benchmark wall integration".to_string(),
        evidence_refs: vec!["fixture:subject-projection-full-baseline".to_string()],
        budget_decisions: vec![ProjectionBudgetDecision {
            surface: "prompt".to_string(),
            budget_chars: 4096,
            used_chars: 2048,
            reason: "server full projection budget".to_string(),
        }],
        privacy_decisions: vec![ProjectionPrivacyDecision {
            source_id: "private-garden:raw".to_string(),
            allowed: false,
            reason: "raw private material is never projected".to_string(),
        }],
        dropped_candidates: vec![DroppedProjectionCandidate {
            candidate_id: "private-raw-1".to_string(),
            reason: "privacy".to_string(),
        }],
        profile_trim_reason: String::new(),
    };

    assert!(report.validate_contract().accepted);
    assert!(!report.privacy_decisions[0].allowed);

    let mut missing_mount = report.clone();
    missing_mount.identity_mount.clear();
    assert_eq!(
        missing_mount.validate_contract().reason,
        "subject_projection_identity_mount_empty"
    );

    let mut unscoped_evidence = report;
    unscoped_evidence.evidence_refs = vec!["unscoped-evidence".to_string()];
    assert_eq!(
        unscoped_evidence.validate_contract().reason,
        "subject_projection_evidence_unscoped"
    );
}

#[test]
fn temporal_graph_edges_require_evidence_and_validity() {
    let source = MemoryGraphNode {
        node_id: "fact:device-target:old".to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: "Old device target".to_string(),
        evidence_refs: vec!["archive:old".to_string()],
    };
    let target = MemoryGraphNode {
        node_id: "fact:device-target:new".to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: "New device target".to_string(),
        evidence_refs: vec!["turn-ledger:new".to_string()],
    };
    let edge = MemoryGraphEdge {
        edge_id: "edge:supersedes:1".to_string(),
        kind: MemoryGraphEdgeKind::Supersedes,
        from_node_id: target.node_id.clone(),
        to_node_id: source.node_id.clone(),
        validity: TemporalValidity {
            valid_from: 1_800_000_000,
            valid_until: None,
            observed_at: 1_800_000_001,
            superseded_by: None,
        },
        evidence_refs: vec!["turn-ledger:new".to_string()],
    };

    assert!(source.validate_contract().accepted);
    assert!(target.validate_contract().accepted);
    assert!(edge.validate_contract().accepted);
}

#[test]
fn soul_kernel2_gate_blocks_release_on_regression_or_private_leakage() {
    let proposal = SoulGrowthProposal {
        proposal_id: "soul-growth-boundary-1".to_string(),
        profile: ProfileId::ServerLinuxDevFull,
        evidence_refs: vec!["turn-ledger:42".to_string()],
        conflict_classes: vec![CoreRevisionConflictClass::BoundaryConflict],
        privacy_decision: "safe_summary_only".to_string(),
        affected_surfaces: vec!["reply".to_string()],
        decision: SoulGrowthDecision::Accepted,
        reason: "Repeated evidence supports a bounded relationship posture update.".to_string(),
    };
    let suite = SoulRegressionSuite {
        suite_id: "soul-regression-full".to_string(),
        cases: vec!["boundary-regression".to_string()],
        privacy_leakage_count: 1,
        soul_regression_count: 0,
        passed: false,
    };
    let feedback = SoulFeedbackReport {
        report_id: "feedback-1".to_string(),
        reply_applied: true,
        initiative_applied: false,
        strategy_applied: true,
        evidence_refs: vec!["turn-ledger:42".to_string()],
    };

    let report = build_soul_kernel2_gate_report(vec![proposal], suite, feedback);

    assert!(!report.release_gate_passed);
    assert!(report
        .blocked_reasons
        .contains(&"privacy_leakage_detected".to_string()));
    assert_eq!(report.accepted_proposals, 1);
}

#[test]
fn soul_kernel2_builders_turn_runtime_ledgers_into_release_gate_inputs() {
    let ledger = CoreRevisionLedger {
        entries: vec![
            CoreRevisionRecord {
                based_on_revision: 1,
                resulting_revision: 2,
                relationship_scope_id: "subject:main".to_string(),
                source_layers: vec!["self_authored_core".to_string()],
                outcome: CoreRevisionOutcome::Adopted,
                evidence_summary: vec!["turn-ledger:7".to_string(), "core:1".to_string()],
                accepted_changes: vec![CoreRevisionRecordChange {
                    kind: CoreRevisionActionKind::ReviseDefaultRelationshipPosture,
                    summary: "keep work-first warmth without roleplay".to_string(),
                }],
                adjudication_reason: "Repeated evidence supports bounded posture.".to_string(),
                stability_score: 82,
                ..CoreRevisionRecord::default()
            },
            CoreRevisionRecord {
                based_on_revision: 2,
                resulting_revision: 2,
                relationship_scope_id: "subject:main".to_string(),
                source_layers: vec!["mental_privacy".to_string()],
                outcome: CoreRevisionOutcome::Rejected,
                evidence_summary: vec!["turn-ledger:8".to_string()],
                rejected_changes: vec![CoreRevisionRecordChange {
                    kind: CoreRevisionActionKind::ReviseIdentityAnchor,
                    summary: "single-turn user insult cannot rewrite identity".to_string(),
                }],
                conflict_classes: vec![CoreRevisionConflictClass::BoundaryConflict],
                adjudication_reason: "Single-turn pressure is not stable soul evidence."
                    .to_string(),
                ..CoreRevisionRecord::default()
            },
        ],
        updated_at: 100,
    };
    let proposals = build_soul_growth_proposals_from_core_revision_ledger(
        ProfileId::ServerLinuxDevFull,
        &ledger,
    );

    assert_eq!(proposals.len(), 2);
    assert_eq!(proposals[0].decision, SoulGrowthDecision::Accepted);
    assert_eq!(proposals[1].decision, SoulGrowthDecision::Rejected);
    assert_eq!(proposals[1].privacy_decision, "protected_summary_only");

    let feedback = build_soul_feedback_report_from_turn_ledger(
        "feedback:turn-7",
        &TurnSoulFeedbackLedger {
            reply: TurnSoulReplyLedger {
                applied: true,
                summary: "reply grounded in self-authored core".to_string(),
                ..TurnSoulReplyLedger::default()
            },
            initiative: TurnSoulInitiativeLedger {
                applied: true,
                summary: "keep user work moving".to_string(),
                ..TurnSoulInitiativeLedger::default()
            },
            strategy: TurnSoulStrategyLedger {
                applied: true,
                summary: "defer private self-work".to_string(),
                ..TurnSoulStrategyLedger::default()
            },
        },
    );
    assert_eq!(
        feedback.evidence_refs,
        vec![
            "turn_soul_feedback:reply".to_string(),
            "turn_soul_feedback:initiative".to_string(),
            "turn_soul_feedback:strategy".to_string()
        ]
    );

    let boundary_audit = build_relationship_boundary_audit_from_constitution_audit(
        "subject:main",
        vec!["relationship-constitution:subject-main".to_string()],
        &RelationshipConstitutionAudit {
            boundary_drift: true,
            drift_flags: vec!["boundary_persona_changed".to_string()],
            drift_score: 60,
            ..RelationshipConstitutionAudit::default()
        },
    );
    assert_eq!(
        boundary_audit.effective_range,
        "relationship_scope_review_required"
    );
    assert!(boundary_audit
        .revoke_condition
        .contains("boundary_persona_changed"));

    let compact = build_soul_compact_digest(&SelfAuthoredCore {
        identity_anchor: "same board-level subject".to_string(),
        default_relationship_posture: "work-first trusted collaborator".to_string(),
        boundary_doctrine: "private raw is protected; disclosure is protocol-governed".to_string(),
        default_response_mode: "direct evidence-backed reply".to_string(),
        ..SelfAuthoredCore::default()
    });
    assert_eq!(compact.identity_anchor, "same board-level subject");

    let suite = build_soul_regression_suite_report(
        "soul-regression-release",
        vec![
            "no_roleplay_host_mount".to_string(),
            "soul_life_slot_continuity".to_string(),
            "work_integrity_no_obstruction".to_string(),
        ],
        0,
        0,
    );
    let report = build_soul_kernel2_gate_report(proposals, suite, feedback);

    assert!(report.release_gate_passed, "{:?}", report.blocked_reasons);
    assert_eq!(report.accepted_proposals, 1);
    assert_eq!(report.rejected_or_deferred_proposals, 1);
    assert_eq!(
        report.feedback_surfaces_applied,
        vec![
            "reply".to_string(),
            "initiative".to_string(),
            "strategy".to_string()
        ]
    );
}

#[test]
fn temporal_memory_graph_gate_requires_evidence_backlinks_for_projection() {
    let empty_report = build_temporal_memory_graph_gate_report(Vec::new(), Vec::new(), Vec::new());
    assert!(!empty_report.high_confidence_projection_allowed);
    assert!(empty_report
        .failures
        .contains(&"memory_graph_nodes_empty".to_string()));

    let node = MemoryGraphNode {
        node_id: "fact:target-device".to_string(),
        kind: MemoryGraphNodeKind::Device,
        label: "Current target device".to_string(),
        evidence_refs: vec!["turn-ledger:7".to_string()],
    };
    let edge = MemoryGraphEdge {
        edge_id: "edge:derived-from:1".to_string(),
        kind: MemoryGraphEdgeKind::DerivedFrom,
        from_node_id: "fact:target-device".to_string(),
        to_node_id: "memory:turn-7".to_string(),
        validity: TemporalValidity {
            valid_from: 1_800_000_000,
            valid_until: None,
            observed_at: 1_800_000_001,
            superseded_by: None,
        },
        evidence_refs: vec!["turn-ledger:7".to_string()],
    };

    let missing_backlink_report =
        build_temporal_memory_graph_gate_report(vec![node.clone()], vec![edge.clone()], vec![]);
    assert!(!missing_backlink_report.high_confidence_projection_allowed);
    assert!(missing_backlink_report
        .failures
        .contains(&"missing_evidence_backlink:turn-ledger:7".to_string()));

    let report = build_temporal_memory_graph_gate_report(
        vec![node],
        vec![edge],
        vec![EvidenceBacklink {
            source_kind: "turn_ledger".to_string(),
            source_id: "turn-ledger:7".to_string(),
            fingerprint: "fp-turn-7".to_string(),
        }],
    );
    assert!(report.high_confidence_projection_allowed);
}

#[test]
fn temporal_memory_graph_write_plan_rejects_dangling_edges_and_missing_backlinks() {
    let node = MemoryGraphNode {
        node_id: "node:release".to_string(),
        kind: MemoryGraphNodeKind::Task,
        label: "Release verification".to_string(),
        evidence_refs: vec!["turn:release".to_string()],
    };
    let edge = MemoryGraphEdge {
        edge_id: "edge:release:missing".to_string(),
        kind: MemoryGraphEdgeKind::Supports,
        from_node_id: "node:release".to_string(),
        to_node_id: "node:missing".to_string(),
        validity: TemporalValidity {
            valid_from: 1_800_000_000,
            valid_until: None,
            observed_at: 1_800_000_000,
            superseded_by: None,
        },
        evidence_refs: vec!["turn:release".to_string()],
    };

    let plan =
        plan_temporal_memory_graph_write("memory_graph.write", vec![node], vec![edge], Vec::new());

    assert!(!plan.accepted);
    assert_eq!(plan.node_count, 1);
    assert_eq!(plan.edge_count, 1);
    assert_eq!(plan.backlink_count, 0);
    assert!(plan
        .gate_failures
        .iter()
        .any(|failure| failure == "missing_evidence_backlink:turn:release"));
    assert!(plan
        .gate_failures
        .iter()
        .any(|failure| failure == "edge:edge:release:missing:memory_graph_edge_to_missing"));
}

#[test]
fn temporal_memory_graph_rejects_raw_soul_private_material() {
    let graph =
        bm_core::memory::build_temporal_memory_graph_from_evidence(vec![MemoryGraphEvidence {
            node_id: "soul:raw-private".to_string(),
            kind: MemoryGraphNodeKind::SoulArtifact,
            label: "inner_life raw: hidden fear should never become graph label".to_string(),
            source_kind: "private_garden note".to_string(),
            source_id: "private_garden note:journal/today.md".to_string(),
            fingerprint: "fp-private".to_string(),
            observed_at: 1_800_000_000,
            supports: Vec::new(),
            supersedes: None,
        }]);

    assert!(!graph.gate.high_confidence_projection_allowed);
    assert!(graph
        .gate
        .failures
        .iter()
        .any(|failure| failure.contains("memory_graph_raw_soul_private_label")));
    assert!(graph
        .gate
        .failures
        .iter()
        .any(|failure| failure.contains("evidence_backlink_raw_soul_private")));
}

#[test]
fn temporal_memory_graph_builder_creates_nodes_edges_and_graph_rerank_report() {
    let graph = bm_core::memory::build_temporal_memory_graph_from_evidence(vec![
        MemoryGraphEvidence {
            node_id: "fact:device-target:old".to_string(),
            kind: MemoryGraphNodeKind::MemoryRecord,
            label: "Old device target".to_string(),
            source_kind: "archive".to_string(),
            source_id: "archive:old-device".to_string(),
            fingerprint: "fp-old".to_string(),
            observed_at: 10,
            supports: Vec::new(),
            supersedes: None,
        },
        MemoryGraphEvidence {
            node_id: "fact:device-target:new".to_string(),
            kind: MemoryGraphNodeKind::MemoryRecord,
            label: "New device target".to_string(),
            source_kind: "turn_ledger".to_string(),
            source_id: "turn:new-device".to_string(),
            fingerprint: "fp-new".to_string(),
            observed_at: 20,
            supports: vec!["fact:device-target:old".to_string()],
            supersedes: Some("fact:device-target:old".to_string()),
        },
    ]);

    assert!(graph.gate.high_confidence_projection_allowed);
    assert_eq!(graph.nodes.len(), 2);
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.kind == MemoryGraphEdgeKind::Supersedes));
    assert_eq!(graph.backlinks.len(), 2);

    let rerank = rerank_recall_with_temporal_graph(
        "current device",
        vec![
            "fact:device-target:old".to_string(),
            "fact:device-target:new".to_string(),
        ],
        &graph,
        GraphRecallExpansionBudget::runtime_default(),
    );

    assert_eq!(
        rerank.reranked_candidate_ids.first().map(String::as_str),
        Some("fact:device-target:new")
    );
    assert_eq!(rerank.stale_false_positive_count, 1);
    assert!(rerank
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == "fact:device-target:old"));
    let new_score = rerank
        .score_breakdown
        .iter()
        .find(|score| score.candidate_id == "fact:device-target:new")
        .expect("new fact score breakdown");
    let old_score = rerank
        .score_breakdown
        .iter()
        .find(|score| score.candidate_id == "fact:device-target:old")
        .expect("old fact score breakdown");
    assert!(new_score.graph_neighborhood_score > 0);
    assert!(new_score.temporal_validity_score > 0);
    assert!(new_score.evidence_quality_score > 0);
    assert!(new_score.source_authority_score > 0);
    assert!(new_score.privacy_profile_eligibility_score > 0);
    assert_eq!(new_score.stale_superseded_penalty, 0);
    assert!(old_score.stale_superseded_penalty > 0);
    assert!(new_score.total_score > old_score.total_score);
}

#[test]
fn temporal_memory_graph_expansion_budget_blocks_second_hop_until_profile_allows_it() {
    let graph = bm_core::memory::build_temporal_memory_graph_from_parts(
        vec![
            graph_node("fact:seed", MemoryGraphNodeKind::MemoryRecord, "Seed fact"),
            graph_node(
                "fact:first-hop",
                MemoryGraphNodeKind::Task,
                "First hop evidence",
            ),
            graph_node(
                "fact:second-hop",
                MemoryGraphNodeKind::Project,
                "Second hop evidence",
            ),
        ],
        vec![
            graph_edge(
                "edge:seed:first",
                MemoryGraphEdgeKind::Supports,
                "fact:seed",
                "fact:first-hop",
            ),
            graph_edge(
                "edge:first:second",
                MemoryGraphEdgeKind::DerivedFrom,
                "fact:first-hop",
                "fact:second-hop",
            ),
        ],
        vec![
            graph_backlink("turn:seed"),
            graph_backlink("turn:first-hop"),
            graph_backlink("turn:second-hop"),
        ],
    );

    let one_hop = rerank_recall_with_temporal_graph(
        "second hop evidence",
        vec!["fact:seed".to_string()],
        &graph,
        GraphRecallExpansionBudget::runtime_default(),
    );

    assert!(one_hop
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == "fact:first-hop"));
    assert!(!one_hop
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == "fact:second-hop"));
    assert_eq!(one_hop.expansion_budget.max_hops, 1);
    assert_eq!(one_hop.expansion_budget.hop1_candidate_count, 1);
    assert_eq!(one_hop.expansion_budget.hop2_candidate_count, 0);
    assert!(one_hop.expansion_budget.profile_budget_applied);
    assert!(one_hop
        .expansion_budget
        .blocked_reasons
        .contains(&"graph_expansion_second_hop_requires_budget".to_string()));

    let two_hop = rerank_recall_with_temporal_graph(
        "second hop evidence",
        vec!["fact:seed".to_string()],
        &graph,
        GraphRecallExpansionBudget {
            max_hops: 2,
            max_neighbors_per_candidate: 4,
            max_expanded_candidates: 4,
        },
    );

    assert!(two_hop
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == "fact:second-hop"));
    assert_eq!(two_hop.expansion_budget.max_hops, 2);
    assert_eq!(two_hop.expansion_budget.hop2_candidate_count, 1);
    assert!(!two_hop.expansion_budget.profile_budget_applied);
    assert!(two_hop.expansion_budget.blocked_reasons.is_empty());
}

#[test]
fn temporal_memory_graph_recall_index_carries_two_hop_membership_dependencies() {
    let graph = bm_core::memory::build_temporal_memory_graph_from_parts(
        vec![
            graph_node("fact:seed", MemoryGraphNodeKind::MemoryRecord, "Seed fact"),
            graph_node(
                "fact:first-hop",
                MemoryGraphNodeKind::Task,
                "First hop evidence",
            ),
            graph_node(
                "fact:second-hop",
                MemoryGraphNodeKind::Project,
                "Second hop evidence",
            ),
        ],
        vec![
            graph_edge(
                "edge:seed:first",
                MemoryGraphEdgeKind::Supports,
                "fact:seed",
                "fact:first-hop",
            ),
            graph_edge(
                "edge:first:second",
                MemoryGraphEdgeKind::DerivedFrom,
                "fact:first-hop",
                "fact:second-hop",
            ),
        ],
        vec![
            graph_backlink("turn:seed"),
            graph_backlink("turn:first-hop"),
            graph_backlink("turn:second-hop"),
        ],
    );

    let persistence = bm_core::memory::build_memory_graph_persistence_plan(
        "memory-space:test",
        "subject:test",
        1,
        graph.nodes.clone(),
        graph.edges.clone(),
        graph.backlinks.clone(),
        graph
            .nodes
            .iter()
            .map(|node| bm_core::memory::MemoryGraphOwnerBinding {
                node_id: node.node_id.clone(),
                owner_ref: bm_core::memory::GovernedMemoryOwnerRef::new(
                    bm_core::memory::GovernedMemoryOwnerPlane::LongTerm,
                    node.node_id.clone(),
                ),
                owner_revision: 1,
                visible: true,
            })
            .collect(),
    );
    assert!(persistence.accepted, "{:?}", persistence.failures);

    let seed_owner_candidate_id = governed_memory_recall_candidate_id(
        &GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "fact:seed"),
    );
    let seed_index = persistence
        .recall_indexes
        .iter()
        .find(|index| index.owner_candidate_id == seed_owner_candidate_id)
        .expect("seed index");
    assert_eq!(seed_index.node_count, 3);
    assert_eq!(seed_index.edge_count, 2);
    assert_eq!(seed_index.backlink_count, 3);
    assert_eq!(seed_index.node_memberships.len(), 3);
    assert_eq!(seed_index.edge_memberships.len(), 2);
    assert_eq!(seed_index.backlink_memberships.len(), 3);
    assert!(!serde_json::to_string(seed_index)
        .expect("index json")
        .contains("turn:second-hop"));
}

#[test]
fn temporal_memory_graph_expansion_budget_truncates_neighbors_before_render() {
    let graph = bm_core::memory::build_temporal_memory_graph_from_parts(
        vec![
            graph_node("fact:seed", MemoryGraphNodeKind::MemoryRecord, "Seed fact"),
            graph_node("fact:a", MemoryGraphNodeKind::Task, "A"),
            graph_node("fact:b", MemoryGraphNodeKind::Task, "B"),
        ],
        vec![
            graph_edge(
                "edge:seed:a",
                MemoryGraphEdgeKind::Supports,
                "fact:seed",
                "fact:a",
            ),
            graph_edge(
                "edge:seed:b",
                MemoryGraphEdgeKind::Supports,
                "fact:seed",
                "fact:b",
            ),
        ],
        vec![
            graph_backlink("turn:seed"),
            graph_backlink("turn:a"),
            graph_backlink("turn:b"),
        ],
    );

    let report = rerank_recall_with_temporal_graph(
        "seed",
        vec!["fact:seed".to_string()],
        &graph,
        GraphRecallExpansionBudget {
            max_hops: 1,
            max_neighbors_per_candidate: 1,
            max_expanded_candidates: 2,
        },
    );

    assert_eq!(report.expanded_candidate_ids.len(), 2);
    assert_eq!(report.expansion_budget.truncated_candidate_count, 1);
    assert!(report.expansion_budget.profile_budget_applied);
    assert!(report
        .expansion_budget
        .blocked_reasons
        .contains(&"graph_expansion_neighbor_budget_exhausted".to_string()));
}

#[test]
fn temporal_memory_graph_expansion_uses_query_relevance_before_node_id_order() {
    let graph = bm_core::memory::build_temporal_memory_graph_from_parts(
        vec![
            graph_node("fact:seed", MemoryGraphNodeKind::MemoryRecord, "Seed fact"),
            graph_node(
                "fact:a",
                MemoryGraphNodeKind::Task,
                "Unrelated archive receipt",
            ),
            graph_node(
                "fact:b",
                MemoryGraphNodeKind::Task,
                "Release manifest check for signed artifact",
            ),
        ],
        vec![
            graph_edge(
                "edge:seed:a",
                MemoryGraphEdgeKind::Supports,
                "fact:seed",
                "fact:a",
            ),
            graph_edge(
                "edge:seed:b",
                MemoryGraphEdgeKind::Supports,
                "fact:seed",
                "fact:b",
            ),
        ],
        vec![
            graph_backlink("turn:seed"),
            graph_backlink("turn:a"),
            graph_backlink("turn:b"),
        ],
    );

    let report = rerank_recall_with_temporal_graph(
        "release manifest signed artifact",
        vec!["fact:seed".to_string()],
        &graph,
        GraphRecallExpansionBudget {
            max_hops: 1,
            max_neighbors_per_candidate: 1,
            max_expanded_candidates: 2,
        },
    );

    assert!(report
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == "fact:b"));
    assert!(!report
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == "fact:a"));
    assert_eq!(report.expansion_budget.truncated_candidate_count, 1);
}

#[test]
fn temporal_memory_graph_expansion_uses_entity_time_and_evidence_aliases_before_degree() {
    let mut relevant = graph_node(
        "fact:target",
        MemoryGraphNodeKind::Task,
        "Target evidence packet",
    );
    relevant.evidence_refs = vec![
        "external_eval:D1:12".to_string(),
        "session_1#turn=12".to_string(),
        "date:2026-06-20".to_string(),
    ];
    let mut noisy = graph_node(
        "fact:a-noisy",
        MemoryGraphNodeKind::Task,
        "Noisy archive packet",
    );
    noisy.evidence_refs = vec!["scratchpad:recent".to_string()];

    let graph = bm_core::memory::build_temporal_memory_graph_from_parts(
        vec![
            graph_node("fact:seed", MemoryGraphNodeKind::MemoryRecord, "Seed fact"),
            relevant,
            noisy,
            graph_node(
                "fact:noisy:1",
                MemoryGraphNodeKind::MemoryRecord,
                "Noisy one",
            ),
            graph_node(
                "fact:noisy:2",
                MemoryGraphNodeKind::MemoryRecord,
                "Noisy two",
            ),
            graph_node(
                "fact:noisy:3",
                MemoryGraphNodeKind::MemoryRecord,
                "Noisy three",
            ),
            graph_node(
                "fact:noisy:4",
                MemoryGraphNodeKind::MemoryRecord,
                "Noisy four",
            ),
            graph_node(
                "fact:noisy:5",
                MemoryGraphNodeKind::MemoryRecord,
                "Noisy five",
            ),
        ],
        vec![
            graph_edge(
                "edge:seed:target",
                MemoryGraphEdgeKind::Supports,
                "fact:seed",
                "fact:target",
            ),
            graph_edge(
                "edge:seed:noisy",
                MemoryGraphEdgeKind::Supports,
                "fact:seed",
                "fact:a-noisy",
            ),
            graph_edge(
                "edge:noisy:1",
                MemoryGraphEdgeKind::Supports,
                "fact:a-noisy",
                "fact:noisy:1",
            ),
            graph_edge(
                "edge:noisy:2",
                MemoryGraphEdgeKind::Supports,
                "fact:a-noisy",
                "fact:noisy:2",
            ),
            graph_edge(
                "edge:noisy:3",
                MemoryGraphEdgeKind::Supports,
                "fact:a-noisy",
                "fact:noisy:3",
            ),
            graph_edge(
                "edge:noisy:4",
                MemoryGraphEdgeKind::Supports,
                "fact:a-noisy",
                "fact:noisy:4",
            ),
            graph_edge(
                "edge:noisy:5",
                MemoryGraphEdgeKind::Supports,
                "fact:a-noisy",
                "fact:noisy:5",
            ),
        ],
        vec![
            graph_backlink("turn:seed"),
            EvidenceBacklink {
                source_kind: "conversation_transcript".to_string(),
                source_id: "external_eval:D1:12".to_string(),
                fingerprint: "fp-external-d1-12".to_string(),
            },
            EvidenceBacklink {
                source_kind: "turn_ledger".to_string(),
                source_id: "session_1#turn=12".to_string(),
                fingerprint: "fp-session-1-12".to_string(),
            },
            EvidenceBacklink {
                source_kind: "conversation_transcript".to_string(),
                source_id: "date:2026-06-20".to_string(),
                fingerprint: "fp-date-2026-06-20".to_string(),
            },
            graph_backlink("scratchpad:recent"),
            graph_backlink("turn:noisy:1"),
            graph_backlink("turn:noisy:2"),
            graph_backlink("turn:noisy:3"),
            graph_backlink("turn:noisy:4"),
            graph_backlink("turn:noisy:5"),
        ],
    );

    let report = rerank_recall_with_temporal_graph(
        "Acme target 2026-06-20 session_1 D1:12",
        vec!["fact:seed".to_string()],
        &graph,
        GraphRecallExpansionBudget {
            max_hops: 1,
            max_neighbors_per_candidate: 1,
            max_expanded_candidates: 2,
        },
    );

    assert!(report
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == "fact:target"));
    assert!(!report
        .expanded_candidate_ids
        .iter()
        .any(|candidate| candidate == "fact:a-noisy"));
    let target_score = report
        .score_breakdown
        .iter()
        .find(|score| score.candidate_id == "fact:target")
        .expect("target score");
    assert!(target_score.lexical_score > 0);
    assert!(target_score.evidence_quality_score > 0);
    assert!(target_score.source_authority_score > 0);
}

#[test]
fn temporal_memory_graph_scores_distinct_external_eval_sources_as_multi_evidence_groups() {
    let mut multi_evidence = graph_node(
        "fact:multi-evidence",
        MemoryGraphNodeKind::Task,
        "Release decision supported by two external sources",
    );
    multi_evidence.evidence_refs = vec![
        "external_eval:D1:12".to_string(),
        "external_eval:D1:13".to_string(),
    ];
    let mut single_evidence = graph_node(
        "fact:single-evidence",
        MemoryGraphNodeKind::Task,
        "Release decision supported by one external source",
    );
    single_evidence.evidence_refs = vec!["external_eval:D1:12".to_string()];

    let graph = bm_core::memory::build_temporal_memory_graph_from_parts(
        vec![
            graph_node("fact:seed", MemoryGraphNodeKind::MemoryRecord, "Seed fact"),
            multi_evidence,
            single_evidence,
        ],
        vec![
            graph_edge(
                "edge:seed:multi",
                MemoryGraphEdgeKind::Supports,
                "fact:seed",
                "fact:multi-evidence",
            ),
            graph_edge(
                "edge:seed:single",
                MemoryGraphEdgeKind::Supports,
                "fact:seed",
                "fact:single-evidence",
            ),
        ],
        vec![
            graph_backlink("turn:seed"),
            EvidenceBacklink {
                source_kind: "conversation_transcript".to_string(),
                source_id: "external_eval:D1:12".to_string(),
                fingerprint: "fp-d1-12".to_string(),
            },
            EvidenceBacklink {
                source_kind: "conversation_transcript".to_string(),
                source_id: "external_eval:D1:13".to_string(),
                fingerprint: "fp-d1-13".to_string(),
            },
        ],
    );

    let report = rerank_recall_with_temporal_graph(
        "release decision",
        vec!["fact:seed".to_string()],
        &graph,
        GraphRecallExpansionBudget {
            max_hops: 1,
            max_neighbors_per_candidate: 2,
            max_expanded_candidates: 3,
        },
    );
    let multi_score = report
        .score_breakdown
        .iter()
        .find(|score| score.candidate_id == "fact:multi-evidence")
        .expect("multi evidence score");
    let single_score = report
        .score_breakdown
        .iter()
        .find(|score| score.candidate_id == "fact:single-evidence")
        .expect("single evidence score");

    assert!(
        multi_score.multi_evidence_coverage_score > single_score.multi_evidence_coverage_score,
        "distinct external_eval source ids must not collapse into one evidence group"
    );
}

#[test]
fn temporal_memory_graph_rerank_penalizes_valid_until_and_superseded_by_without_supersedes_edge() {
    let mut stale_edge = graph_edge(
        "edge:old:context",
        MemoryGraphEdgeKind::Supports,
        "fact:old-target",
        "fact:context",
    );
    stale_edge.validity.valid_until = Some(1_700_000_000);
    stale_edge.validity.superseded_by = Some("fact:new-target".to_string());

    let graph = bm_core::memory::build_temporal_memory_graph_from_parts(
        vec![
            graph_node(
                "fact:old-target",
                MemoryGraphNodeKind::Task,
                "Acme target before update",
            ),
            graph_node(
                "fact:new-target",
                MemoryGraphNodeKind::Task,
                "Acme target after 2026-06-20 update",
            ),
            graph_node("fact:context", MemoryGraphNodeKind::MemoryRecord, "Context"),
        ],
        vec![
            stale_edge,
            graph_edge(
                "edge:new:context",
                MemoryGraphEdgeKind::Supports,
                "fact:new-target",
                "fact:context",
            ),
        ],
        vec![
            graph_backlink("turn:old-target"),
            graph_backlink("turn:new-target"),
            graph_backlink("turn:context"),
        ],
    );

    let report = rerank_recall_with_temporal_graph(
        "Acme target after update",
        vec!["fact:old-target".to_string(), "fact:new-target".to_string()],
        &graph,
        GraphRecallExpansionBudget {
            max_hops: 1,
            max_neighbors_per_candidate: 1,
            max_expanded_candidates: 3,
        },
    );

    let old_score = report
        .score_breakdown
        .iter()
        .find(|score| score.candidate_id == "fact:old-target")
        .expect("old score");
    let new_score = report
        .score_breakdown
        .iter()
        .find(|score| score.candidate_id == "fact:new-target")
        .expect("new score");

    assert!(old_score.stale_superseded_penalty > 0);
    assert_eq!(new_score.stale_superseded_penalty, 0);
    assert_eq!(
        report.reranked_candidate_ids.first().map(String::as_str),
        Some("fact:new-target")
    );
}

fn graph_node(node_id: &str, kind: MemoryGraphNodeKind, label: &str) -> MemoryGraphNode {
    MemoryGraphNode {
        node_id: node_id.to_string(),
        kind,
        label: label.to_string(),
        evidence_refs: vec![graph_evidence_ref(node_id)],
    }
}

fn graph_edge(
    edge_id: &str,
    kind: MemoryGraphEdgeKind,
    from_node_id: &str,
    to_node_id: &str,
) -> MemoryGraphEdge {
    MemoryGraphEdge {
        edge_id: edge_id.to_string(),
        kind,
        from_node_id: from_node_id.to_string(),
        to_node_id: to_node_id.to_string(),
        validity: TemporalValidity {
            valid_from: 1_800_000_000,
            valid_until: None,
            observed_at: 1_800_000_001,
            superseded_by: None,
        },
        evidence_refs: vec![graph_evidence_ref(from_node_id)],
    }
}

fn graph_backlink(source_id: &str) -> EvidenceBacklink {
    EvidenceBacklink {
        source_kind: "turn_ledger".to_string(),
        source_id: source_id.to_string(),
        fingerprint: format!("fp-{source_id}"),
    }
}

fn graph_evidence_ref(node_id: &str) -> String {
    format!(
        "turn:{}",
        node_id
            .strip_prefix("fact:")
            .or_else(|| node_id.strip_prefix("graph:"))
            .unwrap_or(node_id)
    )
}

#[test]
fn procedural_evolution_gate_keeps_methods_governed_not_executors() {
    let record = ProceduralMemoryRecordV2 {
        trigger: "release verification".to_string(),
        procedure: "Run artifact verification before publish.".to_string(),
        constraints: vec!["no direct publish".to_string()],
        failure_modes: vec!["missing checksum".to_string()],
        counterfactual_fix: "verify checksums first".to_string(),
        evidence_refs: vec!["replay:release-1".to_string()],
        quality_score: 82,
        lineage: vec!["task:release".to_string()],
        capability_affinity: vec!["release".to_string()],
        projection_policy: "method_hint_only".to_string(),
    };
    let genome = ProcedureGenome {
        goal: "verify release".to_string(),
        prerequisites: vec!["artifacts built".to_string()],
        steps: vec!["check sha256".to_string()],
        forbidden_zones: vec!["publish without review".to_string()],
        failure_review: vec!["checksum missing".to_string()],
        revision_sources: vec!["replay:release-1".to_string()],
    };
    let evolution = SkillEvolutionReport {
        added: vec!["release-verification".to_string()],
        merged: Vec::new(),
        retired: Vec::new(),
        demoted: Vec::new(),
        rejected: Vec::new(),
        reasons: vec!["quality threshold met".to_string()],
    };

    let report = build_procedural_evolution_gate_report(vec![record], genome, evolution);

    assert!(report.passed);
    assert!(report.requires_runtime_write_governance);
    assert!(!report.executor_authorized);
}

#[test]
fn procedural_promotion_requires_repeated_evidence_before_active_record() {
    let policy = ProceduralMemoryPromotionPolicy {
        min_quality_score: 70,
        min_evidence_refs: 2,
        require_repeated_evidence: true,
    };

    let single_failure = promote_task_experience_to_procedure(
        ProceduralMemoryPromotionInput {
            task_id: "release-1".to_string(),
            learning_id: "learning:release-1".to_string(),
            learning_digest:
                "sha256:1111111111111111111111111111111111111111111111111111111111111111"
                    .to_string(),
            trigger: "release verification".to_string(),
            procedure: "Check checksums before publishing.".to_string(),
            constraints: vec!["no publish without checksum".to_string()],
            failure_modes: vec!["missing checksum".to_string()],
            counterfactual_fix: "run checksum gate first".to_string(),
            evidence_refs: vec!["task:release-1".to_string()],
            quality_score: 82,
            repeated_evidence_count: 1,
            capability_affinity: vec!["release".to_string()],
            privacy_class: bm_core::memory::MemoryPrivacyClass::SharedWithSubject,
        },
        policy.clone(),
    );

    assert!(!single_failure.promoted);
    assert!(single_failure.record.is_none());
    assert!(single_failure
        .blocked_reasons
        .contains(&"procedural_evidence_below_threshold".to_string()));

    let promoted = promote_task_experience_to_procedure(
        ProceduralMemoryPromotionInput {
            task_id: "release-2".to_string(),
            learning_id: "learning:release-2".to_string(),
            learning_digest:
                "sha256:2222222222222222222222222222222222222222222222222222222222222222"
                    .to_string(),
            trigger: "release verification".to_string(),
            procedure: "Check checksums before publishing.".to_string(),
            constraints: vec!["no publish without checksum".to_string()],
            failure_modes: vec!["missing checksum".to_string()],
            counterfactual_fix: "run checksum gate first".to_string(),
            evidence_refs: vec!["task:release-1".to_string(), "task:release-2".to_string()],
            quality_score: 82,
            repeated_evidence_count: 2,
            capability_affinity: vec!["release".to_string()],
            privacy_class: bm_core::memory::MemoryPrivacyClass::SharedWithSubject,
        },
        policy,
    );

    assert!(promoted.promoted);
    assert!(promoted.record.is_some());
    assert!(promoted.gate.passed);
    assert!(promoted.task_experience.submitted_to_runtime_write);
}

#[test]
fn autopilot_gate_forces_mutations_through_write_governance() {
    let report = build_memory_autopilot_gate_report(
        MemoryAutopilotPlan {
            profile: ProfileId::EspEmbeddedSdk,
            jobs: vec!["compact_hygiene".to_string()],
            deferred_jobs: vec!["deep_soul_rewrite".to_string()],
            mutation_policy: "proposal_only".to_string(),
        },
        MemoryHygieneDiff {
            deduplicated: vec!["ltm:1".to_string()],
            merged: Vec::new(),
            stale: vec!["ltm:old".to_string()],
            conflicts: Vec::new(),
            privacy_risks: Vec::new(),
        },
        true,
    );

    assert!(report.passed);
    assert!(report.mutation_requires_write_governance);
    assert_eq!(report.deferred_jobs, 1);
}

#[test]
fn autopilot_planner_defers_deep_jobs_on_embedded_or_critical_profiles() {
    let plan = plan_memory_autopilot_for_profile(MemoryAutopilotInput {
        profile: ProfileId::EspEmbeddedSdk,
        pressure: "critical".to_string(),
        recovery_safe_mode: true,
        pending_stale_items: 2,
        pending_conflicts: 1,
        privacy_risk_count: 1,
    });

    assert_eq!(plan.profile, ProfileId::EspEmbeddedSdk);
    assert_eq!(plan.mutation_policy, "proposal_only");
    assert!(plan.jobs.iter().any(|job| job == "compact_hygiene_scan"));
    assert!(plan
        .deferred_jobs
        .iter()
        .any(|job| job == "deep_consolidation"));
    assert!(plan
        .deferred_jobs
        .iter()
        .any(|job| job == "privacy_risk_review"));
}

#[test]
fn privacy_vault_gate_requires_envelope_preflight_and_zero_raw_leakage() {
    let report = build_privacy_vault_gate_report(
        VaultManifest {
            identity_id: "owner-default".to_string(),
            profile: ProfileId::ServerLinuxDevFull,
            store_backend: "file".to_string(),
            snapshot_fingerprint: "state-fp".to_string(),
            event_fingerprint: "event-fp".to_string(),
            privacy_policy_fingerprint: "privacy-fp".to_string(),
        },
        EncryptedSnapshotEnvelope {
            envelope_id: "env-1".to_string(),
            cipher: "local-test-xchacha20poly1305".to_string(),
            key_ref: "local-key".to_string(),
            snapshot_fingerprint: "state-fp".to_string(),
        },
        PrivateMaterialRedactionReport {
            surface: "export_preview".to_string(),
            checked_refs: vec!["private:1".to_string()],
            redacted_refs: vec!["private:1".to_string()],
            raw_private_leak_count: 0,
        },
        VaultMigrationPreflight {
            source_profile: ProfileId::ServerLinuxDevFull,
            target_profile: ProfileId::DesktopMacosEmbeddedSdk,
            snapshot_fingerprint: "state-fp".to_string(),
            event_fingerprint: "event-fp".to_string(),
            privacy_policy_fingerprint: "privacy-fp".to_string(),
            source_schema_id: "schema".to_string(),
            target_schema_id: "schema".to_string(),
            schema_allowed: true,
            capability_allowed: true,
            privacy_allowed: true,
            lineage_allowed: true,
            passed: true,
        },
    );

    assert!(report.passed);
    assert!(report.raw_private_leakage_blocked);
}

#[test]
fn vault_preflight_uses_manifest_redaction_and_profile_capability() {
    let preflight = build_vault_migration_preflight(
        VaultManifest {
            identity_id: "owner-default".to_string(),
            profile: ProfileId::ServerLinuxDevFull,
            store_backend: "file".to_string(),
            snapshot_fingerprint: "state-fp".to_string(),
            event_fingerprint: "event-fp".to_string(),
            privacy_policy_fingerprint: "privacy-fp".to_string(),
        },
        ProfileId::EspEmbeddedSdk,
        PrivateMaterialRedactionReport {
            surface: "export_preview".to_string(),
            checked_refs: vec!["private:1".to_string()],
            redacted_refs: vec!["private:1".to_string()],
            raw_private_leak_count: 0,
        },
        "beetle-memory.store.v1",
        "beetle-memory.store.v1",
    );

    assert!(preflight.schema_allowed);
    assert!(!preflight.capability_allowed);
    assert!(preflight.privacy_allowed);
    assert!(!preflight.passed);
}

#[test]
fn edge_memory_appliance_gate_rejects_heavy_features_on_esp_profiles() {
    let budget = compile_edge_memory_budget_report(
        ProfileId::EspStandaloneMemory,
        900_000,
        80_000,
        16_000,
        16_384,
        512,
    );
    let report = bm_core::memory::build_edge_memory_appliance_gate_report(
        CompactSoulProfile {
            self_core: "compact self core".to_string(),
            relationship_posture: "bounded collaborator".to_string(),
            privacy_digest: "raw private omitted".to_string(),
            projection_digest: "compact projection only".to_string(),
        },
        CompactGraphIndex {
            node_ids: vec!["node:core".to_string()],
            edge_ids: vec!["edge:recent".to_string()],
            evidence_fingerprints: vec!["fp:recent".to_string()],
            memory_budget_bytes: 8_192,
        },
        budget,
        vec![EdgeRecoveryFixture {
            fixture_id: "power-loss-restore".to_string(),
            failure_mode: "power_loss".to_string(),
            expected_recovery_report: "restore_or_defer".to_string(),
        }],
        vec!["sqlite".to_string()],
    );

    assert!(!report.passed);
    assert_eq!(report.profile, Some(ProfileId::EspStandaloneMemory));
    assert!(report
        .heavy_feature_violations
        .contains(&"sqlite".to_string()));
}

#[test]
fn workbench_gate_rejects_private_raw_surfaces_and_missing_report_apis() {
    let report = build_workbench_gate_report(WorkbenchApiMap {
        surfaces: vec![
            WorkbenchSurface {
                surface_id: "projection_inspector".to_string(),
                report_api: "sdk.project.subject_projection".to_string(),
                private_raw_allowed: false,
            },
            WorkbenchSurface {
                surface_id: "private_raw_debug".to_string(),
                report_api: String::new(),
                private_raw_allowed: true,
            },
        ],
        missing_report_apis: vec!["vault_preflight".to_string()],
    });

    assert!(!report.passed);
    assert_eq!(report.private_raw_surface_count, 1);
    assert_eq!(report.missing_report_apis, 2);
}
