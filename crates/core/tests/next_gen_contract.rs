use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    build_memory_autopilot_gate_report, build_next_gen_contract_matrix,
    build_privacy_vault_gate_report, build_procedural_evolution_gate_report,
    build_soul_kernel2_gate_report, build_temporal_memory_graph_gate_report,
    build_vault_migration_preflight, build_workbench_gate_report,
    compile_edge_memory_budget_report, plan_memory_autopilot_for_profile,
    promote_task_experience_to_procedure, rerank_recall_with_temporal_graph, CompactGraphIndex,
    CompactSoulProfile, CoreRevisionConflictClass, DroppedProjectionCandidate, EdgeRecoveryFixture,
    EncryptedSnapshotEnvelope, EvidenceBacklink, MemoryAutopilotInput, MemoryAutopilotPlan,
    MemoryGraphEdge, MemoryGraphEdgeKind, MemoryGraphEvidence, MemoryGraphNode,
    MemoryGraphNodeKind, MemoryHygieneDiff, NextGenPhase, PrivateMaterialRedactionReport,
    ProceduralMemoryPromotionInput, ProceduralMemoryPromotionPolicy, ProceduralMemoryRecordV2,
    ProcedureGenome, ProjectionBudgetDecision, ProjectionPrivacyDecision, SkillEvolutionReport,
    SoulFeedbackReport, SoulGrowthDecision, SoulGrowthProposal, SoulRegressionSuite,
    SubjectProjectionReport, TemporalValidity, VaultManifest, VaultMigrationPreflight,
    WorkbenchApiMap, WorkbenchSurface,
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
    assert_eq!(report.privacy_decisions[0].allowed, false);
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
    );

    assert_eq!(
        rerank.selected_ids.first().map(String::as_str),
        Some("fact:device-target:new")
    );
    assert_eq!(rerank.stale_false_positive_count, 1);
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
            trigger: "release verification".to_string(),
            procedure: "Check checksums before publishing.".to_string(),
            constraints: vec!["no publish without checksum".to_string()],
            failure_modes: vec!["missing checksum".to_string()],
            counterfactual_fix: "run checksum gate first".to_string(),
            evidence_refs: vec!["task:release-1".to_string()],
            quality_score: 82,
            repeated_evidence_count: 1,
            capability_affinity: vec!["release".to_string()],
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
            trigger: "release verification".to_string(),
            procedure: "Check checksums before publishing.".to_string(),
            constraints: vec!["no publish without checksum".to_string()],
            failure_modes: vec!["missing checksum".to_string()],
            counterfactual_fix: "run checksum gate first".to_string(),
            evidence_refs: vec!["task:release-1".to_string(), "task:release-2".to_string()],
            quality_score: 82,
            repeated_evidence_count: 2,
            capability_affinity: vec!["release".to_string()],
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
