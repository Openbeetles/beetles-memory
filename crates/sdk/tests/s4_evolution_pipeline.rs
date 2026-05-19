use bm_core::{
    Confidence, EvidenceRef, EvidenceState, EvolutionBudget, EvolutionDisposition, EvolutionInput,
    EvolutionMode, EvolutionProposal, EvolutionProposalKind, EvolutionRisk, MemoryPlane,
    MentalPrivacyLayer, RuntimeProfile, SourceKind, SourceRef, WriteCandidate, WriteDecision,
    WriteRejectReason,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn propose_evolution_returns_proposals_without_mutating_store_until_submit() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let batch = runtime.propose_evolution(EvolutionInput {
        run_id: "s4-sdk-proposal-only".to_owned(),
        identity: "agent:s4".to_owned(),
        scope: "task:s4:sdk".to_owned(),
        profile: RuntimeProfile::DevFull,
        mode: EvolutionMode::Full,
        evidence: vec![EvidenceRef {
            record_id: Some("evidence-1".to_owned()),
            event_seq: Some(1),
            source: SourceRef::new(SourceKind::TaskLearning, "task-learning:s4"),
            plane: MemoryPlane::Procedural,
            privacy_layer: MentalPrivacyLayer::Shared,
            evidence: EvidenceState::Supported,
            summary: "下次遇到 S4 proposal apply 时，必须先走 SDK governance。".to_owned(),
        }],
        recall_report: None,
        projection_report: None,
        subject_assembly: None,
        budget: EvolutionBudget {
            max_events: 16,
            max_records: 16,
            max_branches: 2,
            max_proposals: 4,
            max_output_bytes: 2048,
            allow_private_layer: false,
            allow_soul_revision: true,
            allow_script_backend: false,
        },
    });

    assert_eq!(batch.mode, EvolutionMode::Full);
    assert!(!batch.report.raw_private_exposed);
    assert!(batch
        .proposals
        .iter()
        .any(|proposal| proposal.kind == EvolutionProposalKind::ProceduralPromotion));

    let before_submit = runtime.recall(bm_core::RecallQuery::new("task:s4:sdk").limit(8));
    assert!(before_submit.selected.is_empty());

    let proposal = batch
        .proposals
        .iter()
        .find(|proposal| proposal.candidate_write.is_some())
        .expect("proposal with candidate write");
    let report = runtime.submit_evolution_proposal(proposal);

    assert_eq!(report.decision, WriteDecision::Accepted);
    assert_eq!(report.plane, Some(MemoryPlane::Procedural));
}

#[test]
fn proposal_apply_still_obeys_compact_profile_soul_governance_gate() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::EspCompact)
        .store(store)
        .build();
    let proposal = EvolutionProposal {
        proposal_id: "s4-compact-soul-revision".to_owned(),
        kind: EvolutionProposalKind::SoulGovernanceRevision,
        disposition: EvolutionDisposition::ProposeSoulRevision,
        target_plane: Some(MemoryPlane::SoulGovernance),
        source_evidence: Vec::new(),
        confidence: Confidence::High,
        risk: EvolutionRisk::High,
        privacy_filtered: true,
        candidate_write: Some(
            WriteCandidate::new(
                "agent:s4",
                "task:s4:compact",
                "compact cannot apply thick soul",
            )
            .source("evolution:s4")
            .plane_hint(MemoryPlane::SoulGovernance),
        ),
        rationale: "presence-only soul revision proposal".to_owned(),
    };

    let report = runtime.submit_evolution_proposal(&proposal);

    assert_eq!(report.decision, WriteDecision::Rejected);
    assert_eq!(
        report.governance.reject_reason,
        Some(WriteRejectReason::ProfileRejected)
    );
}

#[test]
fn consumer_mode_does_not_run_evolution_passes() {
    let store = InMemoryStore::default();
    let runtime = MemoryRuntimeBuilder::new(RuntimeProfile::SdkEmbedded)
        .store(store)
        .build();

    let batch = runtime.propose_evolution(EvolutionInput {
        run_id: "s4-consumer".to_owned(),
        identity: "agent:s4".to_owned(),
        scope: "task:s4:consumer".to_owned(),
        profile: RuntimeProfile::SdkEmbedded,
        mode: EvolutionMode::Consumer,
        evidence: vec![EvidenceRef {
            record_id: Some("evidence-1".to_owned()),
            event_seq: Some(1),
            source: SourceRef::new(SourceKind::ReplayFixture, "replay:s4-consumer"),
            plane: MemoryPlane::SharedFactual,
            privacy_layer: MentalPrivacyLayer::Shared,
            evidence: EvidenceState::Supported,
            summary: "consumer mode evidence is visible only as summary".to_owned(),
        }],
        recall_report: None,
        projection_report: None,
        subject_assembly: None,
        budget: EvolutionBudget {
            max_events: 0,
            max_records: 0,
            max_branches: 0,
            max_proposals: 0,
            max_output_bytes: 256,
            allow_private_layer: false,
            allow_soul_revision: false,
            allow_script_backend: false,
        },
    });

    assert_eq!(batch.mode, EvolutionMode::Consumer);
    assert!(batch.proposals.is_empty());
    assert_eq!(batch.report.evidence_read, 0);
    assert_eq!(batch.report.proposals_emitted, 0);
}

#[test]
fn evolution_proposal_uses_runtime_profile_not_caller_claim() {
    let store = InMemoryStore::default();
    let runtime = MemoryRuntimeBuilder::new(RuntimeProfile::EspCompact)
        .store(store)
        .build();

    let batch = runtime.propose_evolution(EvolutionInput {
        run_id: "s4-profile-owned-by-runtime".to_owned(),
        identity: "agent:s4".to_owned(),
        scope: "task:s4:profile".to_owned(),
        profile: RuntimeProfile::DevFull,
        mode: EvolutionMode::Full,
        evidence: vec![EvidenceRef {
            record_id: Some("evidence-1".to_owned()),
            event_seq: Some(1),
            source: SourceRef::new(SourceKind::TaskLearning, "task-learning:s4-profile"),
            plane: MemoryPlane::Procedural,
            privacy_layer: MentalPrivacyLayer::Shared,
            evidence: EvidenceState::Supported,
            summary: "runtime profile owns evolution budget posture".to_owned(),
        }],
        recall_report: None,
        projection_report: None,
        subject_assembly: None,
        budget: EvolutionBudget {
            max_events: 16,
            max_records: 16,
            max_branches: 2,
            max_proposals: 4,
            max_output_bytes: 2048,
            allow_private_layer: false,
            allow_soul_revision: true,
            allow_script_backend: false,
        },
    });

    assert_eq!(batch.profile, RuntimeProfile::EspCompact);
    assert!(batch.report.profile_trimmed);
    assert!(batch
        .proposals
        .iter()
        .all(|proposal| proposal.target_plane != Some(MemoryPlane::SoulGovernance)));
}
