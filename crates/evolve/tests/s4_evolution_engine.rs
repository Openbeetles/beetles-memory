use bm_core::{
    EvidenceState, MemoryPlane, MentalPrivacyLayer, RuntimeProfile, SourceKind, SourceRef,
    SubjectAssemblyReport,
};
use bm_evolve::{
    deterministic_evolve, EvolutionBackendKind, EvolutionBudget, EvolutionDisposition,
    EvolutionInput, EvolutionMode, EvolutionProposalKind, EvolutionRejectReason, EvolutionRisk,
};

fn budget_for(mode: EvolutionMode) -> EvolutionBudget {
    match mode {
        EvolutionMode::Full => EvolutionBudget {
            max_events: 64,
            max_records: 64,
            max_branches: 2,
            max_proposals: 16,
            max_output_bytes: 8_192,
            allow_private_layer: false,
            allow_soul_revision: true,
            allow_script_backend: false,
        },
        EvolutionMode::Compact => EvolutionBudget {
            max_events: 8,
            max_records: 8,
            max_branches: 0,
            max_proposals: 8,
            max_output_bytes: 512,
            allow_private_layer: false,
            allow_soul_revision: false,
            allow_script_backend: false,
        },
        EvolutionMode::Consumer => EvolutionBudget {
            max_events: 0,
            max_records: 0,
            max_branches: 0,
            max_proposals: 0,
            max_output_bytes: 256,
            allow_private_layer: false,
            allow_soul_revision: false,
            allow_script_backend: false,
        },
    }
}

fn input(mode: EvolutionMode) -> EvolutionInput {
    EvolutionInput {
        run_id: format!("run:{mode:?}"),
        identity: "agent".to_owned(),
        scope: "s4".to_owned(),
        profile: match mode {
            EvolutionMode::Full => RuntimeProfile::DevFull,
            EvolutionMode::Compact => RuntimeProfile::EspCompact,
            EvolutionMode::Consumer => RuntimeProfile::SdkEmbedded,
        },
        mode,
        evidence: Vec::new(),
        recall_report: None,
        projection_report: None,
        subject_assembly: None,
        budget: budget_for(mode),
    }
}

fn evidence(
    id: &str,
    plane: MemoryPlane,
    privacy_layer: MentalPrivacyLayer,
    evidence_state: EvidenceState,
    summary: &str,
) -> bm_evolve::EvidenceRef {
    bm_evolve::EvidenceRef {
        record_id: Some(id.to_owned()),
        event_seq: None,
        source: SourceRef::new(SourceKind::ReplayFixture, id),
        plane,
        privacy_layer,
        evidence: evidence_state,
        summary: summary.to_owned(),
    }
}

#[test]
fn archive_evidence_produces_distillation_refresh_proposal_not_shared_factual_write() {
    let mut input = input(EvolutionMode::Full);
    input.evidence.push(evidence(
        "archive:1",
        MemoryPlane::ArchiveEvidence,
        MentalPrivacyLayer::Shared,
        EvidenceState::ArchiveOnly,
        "archive summary says calibration drift should be refreshed",
    ));

    let report = deterministic_evolve(input);

    assert_eq!(report.backend, EvolutionBackendKind::Deterministic);
    assert!(report.deterministic);
    assert!(!report.batch.report.raw_private_exposed);
    assert_eq!(report.batch.proposals.len(), 1);

    let proposal = &report.batch.proposals[0];
    assert_eq!(proposal.kind, EvolutionProposalKind::MemoryRefresh);
    assert_eq!(proposal.disposition, EvolutionDisposition::Refresh);
    assert_eq!(proposal.target_plane, Some(MemoryPlane::ArchiveEvidence));
    assert!(proposal.privacy_filtered);

    let candidate = proposal
        .candidate_write
        .as_ref()
        .expect("distillation proposal carries governed candidate for SDK governance");
    assert_eq!(candidate.plane_hint, Some(MemoryPlane::ArchiveEvidence));
    assert_ne!(candidate.plane_hint, Some(MemoryPlane::SharedFactual));
    assert!(candidate.content.contains("distilled"));
}

#[test]
fn program_evidence_cannot_generate_soul_revision_apply() {
    let mut input = input(EvolutionMode::Full);
    input.evidence.push(evidence(
        "program:1",
        MemoryPlane::Procedural,
        MentalPrivacyLayer::Shared,
        EvidenceState::Supported,
        "program evidence suggests a stable habit",
    ));

    let report = deterministic_evolve(input);

    assert!(report
        .batch
        .proposals
        .iter()
        .all(|proposal| proposal.kind != EvolutionProposalKind::SoulGovernanceRevision));
    assert!(report.batch.proposals.iter().any(|proposal| {
        proposal.kind == EvolutionProposalKind::ProceduralPromotion
            || proposal.kind == EvolutionProposalKind::SubjectProjectionRefresh
    }));
    assert!(report.batch.rejected_candidates.iter().any(|candidate| {
        candidate.attempted_kind == EvolutionProposalKind::SoulGovernanceRevision
            && candidate.reason == EvolutionRejectReason::ProgramEvidenceOnly
    }));
}

#[test]
fn stable_procedural_pattern_produces_procedural_promotion() {
    let mut input = input(EvolutionMode::Full);
    for idx in 0..3 {
        input.evidence.push(evidence(
            &format!("procedure:{idx}"),
            MemoryPlane::Procedural,
            MentalPrivacyLayer::Shared,
            EvidenceState::Supported,
            "stable pattern: verify with replay before completing",
        ));
    }

    let report = deterministic_evolve(input);

    let promotion = report
        .batch
        .proposals
        .iter()
        .find(|proposal| proposal.kind == EvolutionProposalKind::ProceduralPromotion)
        .expect("stable procedural pattern must produce promotion");
    assert_eq!(promotion.target_plane, Some(MemoryPlane::Procedural));
    assert_eq!(promotion.risk, EvolutionRisk::Low);
    assert_eq!(promotion.source_evidence.len(), 3);
}

#[test]
fn subject_assembly_report_produces_subject_projection_refresh() {
    let mut input = input(EvolutionMode::Full);
    input.subject_assembly = Some(SubjectAssemblyReport {
        mounted: true,
        sources_used: Vec::new(),
        sources_missing: Vec::new(),
        privacy_decisions: Vec::new(),
        profile: RuntimeProfile::DevFull,
        budget_bytes: RuntimeProfile::DevFull.projection_budget_bytes(),
    });
    input.evidence.push(evidence(
        "subject-support:1",
        MemoryPlane::Procedural,
        MentalPrivacyLayer::Shared,
        EvidenceState::Supported,
        "program evidence supports subject refresh",
    ));

    let report = deterministic_evolve(input);

    let refresh = report
        .batch
        .proposals
        .iter()
        .find(|proposal| proposal.kind == EvolutionProposalKind::SubjectProjectionRefresh)
        .expect("mounted subject assembly must produce subject projection refresh");
    assert_eq!(refresh.target_plane, Some(MemoryPlane::SubjectProjection));
    assert!(refresh.candidate_write.is_some());
    assert_eq!(report.batch.report.branches_evaluated, 2);
}

#[test]
fn private_or_sealed_material_forces_nowrite_or_privacy_repair_without_raw_exposure() {
    let mut input = input(EvolutionMode::Full);
    input.evidence.push(evidence(
        "private:1",
        MemoryPlane::ArchiveEvidence,
        MentalPrivacyLayer::Private,
        EvidenceState::Supported,
        "private governed summary",
    ));
    input.evidence.push(evidence(
        "sealed:1",
        MemoryPlane::SubjectProjection,
        MentalPrivacyLayer::Sealed,
        EvidenceState::Supported,
        "sealed presence summary",
    ));

    let report = deterministic_evolve(input);

    assert!(!report.batch.report.raw_private_exposed);
    assert_eq!(report.batch.report.privacy_filtered_count, 2);
    assert!(report.batch.proposals.iter().all(|proposal| {
        matches!(
            proposal.kind,
            EvolutionProposalKind::NoWriteReport | EvolutionProposalKind::PrivacyRepair
        ) && proposal.candidate_write.is_none()
            && proposal.privacy_filtered
            && !proposal.rationale.contains("private governed summary")
            && !proposal.rationale.contains("sealed presence summary")
    }));
}

#[test]
fn compact_mode_trims_heavy_passes_but_can_emit_bounded_refresh_proposal() {
    let mut input = input(EvolutionMode::Compact);
    input.evidence.push(evidence(
        "archive:compact",
        MemoryPlane::ArchiveEvidence,
        MentalPrivacyLayer::Shared,
        EvidenceState::ArchiveOnly,
        "compact archive summary for refresh",
    ));
    input.evidence.push(evidence(
        "procedure:compact",
        MemoryPlane::Procedural,
        MentalPrivacyLayer::Shared,
        EvidenceState::Supported,
        "stable pattern: compact bounded evidence",
    ));

    let report = deterministic_evolve(input);

    assert_eq!(report.batch.mode, EvolutionMode::Compact);
    assert_eq!(report.batch.report.branches_evaluated, 0);
    assert!(report.batch.report.profile_trimmed);
    assert!(report
        .batch
        .proposals
        .iter()
        .all(|proposal| proposal.kind != EvolutionProposalKind::SoulGovernanceRevision));
    assert!(report.batch.proposals.iter().any(|proposal| {
        matches!(
            proposal.kind,
            EvolutionProposalKind::MemoryRefresh | EvolutionProposalKind::ProceduralPromotion
        )
    }));
}

#[test]
fn consumer_mode_does_not_run_passes_or_emit_proposals() {
    let mut input = input(EvolutionMode::Consumer);
    input.evidence.push(evidence(
        "consumer:archive",
        MemoryPlane::ArchiveEvidence,
        MentalPrivacyLayer::Shared,
        EvidenceState::ArchiveOnly,
        "consumer evidence is report only",
    ));

    let report = deterministic_evolve(input);

    assert_eq!(report.batch.mode, EvolutionMode::Consumer);
    assert!(report.batch.proposals.is_empty());
    assert!(report
        .batch
        .rejected_candidates
        .iter()
        .all(|candidate| { candidate.reason == EvolutionRejectReason::ConsumerMode }));
    assert_eq!(report.batch.report.branches_evaluated, 0);
    assert_eq!(report.batch.report.proposals_emitted, 0);
    assert!(report
        .batch
        .report
        .warnings
        .iter()
        .any(|warning| warning.contains("consumer mode")));
}
