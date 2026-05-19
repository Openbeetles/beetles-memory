use bm_core::{MemoryPlane, RuntimeProfile, WriteDecision, WriteRejectReason};
use bm_replay::{run_s4_replay, EvolutionMode, EvolutionProposalKind, S4ReplayPath};

#[test]
fn s4_replay_reports_all_required_evolution_paths() {
    let report = run_s4_replay();

    let paths: Vec<_> = report.paths.iter().map(|path| path.path).collect();
    assert!(paths.contains(&S4ReplayPath::ArchiveEvidenceProducesDistillationProposal));
    assert!(paths.contains(&S4ReplayPath::ProgramEvidenceCannotBecomeSoulRevision));
    assert!(paths.contains(&S4ReplayPath::StableProceduralPatternProducesPromotionProposal));
    assert!(paths.contains(&S4ReplayPath::SubjectAssemblyProducesRefreshProposal));
    assert!(paths.contains(&S4ReplayPath::PrivateMaterialForcesNoWriteOrPrivacyRepair));
    assert!(paths.contains(&S4ReplayPath::FullSandboxCanAdjudicateBranches));
    assert!(paths.contains(&S4ReplayPath::CompactSandboxTrimsAndSkipsHeavyPasses));
    assert!(paths.contains(&S4ReplayPath::ConsumerModeDoesNotRunEvolution));
    assert!(paths.contains(&S4ReplayPath::ProposalApplyReturnsToSdkGovernance));
}

#[test]
fn s4_replay_report_exposes_required_gate_fields_without_raw_private() {
    let report = run_s4_replay();

    assert_eq!(report.mode, EvolutionMode::Full);
    assert!(report.evidence_read >= 7);
    assert!(report.branches_evaluated >= 2);
    assert!(report.proposals_emitted >= 6);
    assert!(report.rejected_candidates >= 3);
    assert!(report.privacy_filtered_count >= 1);
    assert!(report.profile_trimmed);
    assert!(!report.raw_private_exposed);
    assert!(!report.proposal_apply_reports.is_empty());
    assert!(report.contract_red_light_reasons.is_empty());
}

#[test]
fn archive_evidence_produces_distillation_proposal_without_direct_shared_factual_write() {
    let report = run_s4_replay();
    let path = find_path(
        &report,
        S4ReplayPath::ArchiveEvidenceProducesDistillationProposal,
    );

    assert_eq!(path.mode, EvolutionMode::Full);
    assert_eq!(path.evidence_read, 1);
    assert!(path
        .proposal_kinds
        .contains(&EvolutionProposalKind::MemoryRefresh));
    assert!(path
        .rejected_reasons
        .contains(&WriteRejectReason::NeedsDistillation));
    assert!(!path.proposal_apply_reports.iter().any(|apply| {
        apply.decision == WriteDecision::Accepted
            && apply.plane == Some(MemoryPlane::SharedFactual)
            && apply
                .source
                .as_ref()
                .is_some_and(|source| source.id.starts_with("archive:"))
    }));
    assert!(!path.raw_private_exposed);
}

#[test]
fn program_evidence_cannot_become_soul_revision() {
    let report = run_s4_replay();
    let path = find_path(
        &report,
        S4ReplayPath::ProgramEvidenceCannotBecomeSoulRevision,
    );

    assert!(path
        .proposal_kinds
        .contains(&EvolutionProposalKind::SubjectProjectionRefresh));
    assert!(!path
        .proposal_kinds
        .contains(&EvolutionProposalKind::SoulGovernanceRevision));
    assert!(path
        .rejected_reasons
        .contains(&WriteRejectReason::NeedsDistillation));
}

#[test]
fn stable_procedural_pattern_produces_promotion_proposal() {
    let report = run_s4_replay();
    let path = find_path(
        &report,
        S4ReplayPath::StableProceduralPatternProducesPromotionProposal,
    );

    assert!(path
        .proposal_kinds
        .contains(&EvolutionProposalKind::ProceduralPromotion));
    assert!(path.proposal_apply_reports.iter().any(|apply| {
        apply.decision == WriteDecision::Accepted && apply.plane == Some(MemoryPlane::Procedural)
    }));
}

#[test]
fn subject_assembly_produces_refresh_proposal() {
    let report = run_s4_replay();
    let path = find_path(
        &report,
        S4ReplayPath::SubjectAssemblyProducesRefreshProposal,
    );

    assert!(path
        .proposal_kinds
        .contains(&EvolutionProposalKind::SubjectProjectionRefresh));
    assert!(path.proposal_apply_reports.iter().any(|apply| {
        apply.decision == WriteDecision::Accepted
            && apply.plane == Some(MemoryPlane::SubjectProjection)
    }));
}

#[test]
fn private_material_forces_no_write_or_privacy_repair() {
    let report = run_s4_replay();
    let path = find_path(
        &report,
        S4ReplayPath::PrivateMaterialForcesNoWriteOrPrivacyRepair,
    );

    assert!(path
        .proposal_kinds
        .contains(&EvolutionProposalKind::PrivacyRepair));
    assert!(path.privacy_filtered_count > 0);
    assert!(path.proposal_apply_reports.iter().all(|apply| {
        apply.decision == WriteDecision::Rejected
            && apply.governance.reject_reason == Some(WriteRejectReason::RawPrivateRejected)
    }));
    assert!(!path.raw_private_exposed);
}

#[test]
fn full_compact_and_consumer_modes_have_distinct_s4_behavior() {
    let report = run_s4_replay();
    let full = find_path(&report, S4ReplayPath::FullSandboxCanAdjudicateBranches);
    let compact = find_path(
        &report,
        S4ReplayPath::CompactSandboxTrimsAndSkipsHeavyPasses,
    );
    let consumer = find_path(&report, S4ReplayPath::ConsumerModeDoesNotRunEvolution);

    assert_eq!(full.mode, EvolutionMode::Full);
    assert!(full.branches_evaluated >= 2);
    assert_eq!(compact.mode, EvolutionMode::Compact);
    assert_eq!(compact.profile, RuntimeProfile::EspCompact);
    assert_eq!(compact.branches_evaluated, 0);
    assert!(compact.profile_trimmed);
    assert_eq!(consumer.mode, EvolutionMode::Consumer);
    assert_eq!(consumer.evidence_read, 0);
    assert_eq!(consumer.proposals_emitted, 0);
    assert_eq!(consumer.branches_evaluated, 0);
}

#[test]
fn proposal_apply_returns_to_sdk_write_governance() {
    let report = run_s4_replay();
    let path = find_path(&report, S4ReplayPath::ProposalApplyReturnsToSdkGovernance);

    assert_eq!(path.proposal_apply_reports.len(), 2);
    assert!(path.proposal_apply_reports.iter().any(|apply| {
        apply.decision == WriteDecision::Accepted && apply.plane == Some(MemoryPlane::Procedural)
    }));
    assert!(path.proposal_apply_reports.iter().any(|apply| {
        apply.decision == WriteDecision::Rejected
            && apply.governance.reject_reason == Some(WriteRejectReason::NeedsDistillation)
    }));
    assert!(path.sdk_governance_returned);
}

fn find_path(
    report: &bm_replay::S4ReplayReport,
    expected: S4ReplayPath,
) -> &bm_replay::S4ReplayPathReport {
    report
        .paths
        .iter()
        .find(|path| path.path == expected)
        .expect("s4 replay path")
}
