//! Deterministic S4 memory evolution sandbox.

pub use bm_core::{
    EvidenceRef, EvolutionBackendKind, EvolutionBackendReport, EvolutionBudget,
    EvolutionDisposition, EvolutionInput, EvolutionMode, EvolutionProposal, EvolutionProposalBatch,
    EvolutionProposalKind, EvolutionRejectReason, EvolutionRejectedCandidate, EvolutionRisk,
    EvolutionRunReport,
};

use bm_core::{
    Confidence, EvidenceState, MemoryDomain, MemoryPlane, MentalPrivacyLayer, RuntimeProfile,
    WriteCandidate,
};

pub fn deterministic_evolve(input: EvolutionInput) -> EvolutionBackendReport {
    let batch = match input.mode {
        EvolutionMode::Consumer => consumer_report(input),
        EvolutionMode::Compact | EvolutionMode::Full => run_deterministic_passes(input),
    };

    EvolutionBackendReport {
        batch,
        backend: EvolutionBackendKind::Deterministic,
        deterministic: true,
    }
}

fn consumer_report(input: EvolutionInput) -> EvolutionProposalBatch {
    let rejected_candidates = input
        .evidence
        .iter()
        .enumerate()
        .map(|(idx, evidence)| {
            rejected_candidate(
                idx,
                EvolutionProposalKind::MemoryRefresh,
                EvolutionRejectReason::ConsumerMode,
                vec![evidence.clone()],
            )
        })
        .collect::<Vec<_>>();

    let report = EvolutionRunReport {
        run_id: input.run_id.clone(),
        mode: input.mode,
        evidence_read: 0,
        branches_evaluated: 0,
        proposals_emitted: 0,
        rejected_candidates: rejected_candidates.len(),
        privacy_filtered_count: 0,
        profile_trimmed: true,
        raw_private_exposed: false,
        warnings: vec!["consumer mode does not run evolution pass".to_owned()],
    };

    EvolutionProposalBatch {
        run_id: input.run_id,
        profile: input.profile,
        mode: input.mode,
        proposals: Vec::new(),
        rejected_candidates,
        report,
    }
}

fn run_deterministic_passes(input: EvolutionInput) -> EvolutionProposalBatch {
    let mut warnings = Vec::new();
    if input.budget.allow_script_backend {
        warnings.push("script backend request ignored by deterministic engine".to_owned());
    }

    let evidence = bounded_evidence(&input);
    let privacy_filtered_count = evidence
        .iter()
        .filter(|evidence| is_private_or_sealed(evidence))
        .count();

    let (proposals, rejected_candidates) = if privacy_filtered_count > 0 {
        privacy_repair_proposals(&input, &evidence)
    } else {
        proposal_passes(&input, &evidence)
    };

    let report = EvolutionRunReport {
        run_id: input.run_id.clone(),
        mode: input.mode,
        evidence_read: evidence.len(),
        branches_evaluated: branches_evaluated(&input),
        proposals_emitted: proposals.len(),
        rejected_candidates: rejected_candidates.len(),
        privacy_filtered_count,
        profile_trimmed: is_profile_trimmed(&input),
        raw_private_exposed: false,
        warnings,
    };

    EvolutionProposalBatch {
        run_id: input.run_id,
        profile: input.profile,
        mode: input.mode,
        proposals,
        rejected_candidates,
        report,
    }
}

fn proposal_passes(
    input: &EvolutionInput,
    evidence: &[EvidenceRef],
) -> (Vec<EvolutionProposal>, Vec<EvolutionRejectedCandidate>) {
    let mut proposals = Vec::new();
    let mut rejected = Vec::new();

    let archive_evidence = evidence
        .iter()
        .filter(|evidence| evidence.plane == MemoryPlane::ArchiveEvidence)
        .cloned()
        .collect::<Vec<_>>();
    for source in &archive_evidence {
        rejected.push(rejected_candidate(
            rejected.len(),
            EvolutionProposalKind::MemoryRefresh,
            EvolutionRejectReason::ArchiveNeedsDistillation,
            vec![source.clone()],
        ));
        let proposal_idx = proposals.len();
        push_capped(
            &mut proposals,
            archive_refresh_proposal(input, source.clone(), proposal_idx),
            input.budget.max_proposals,
        );
    }

    let procedural_evidence = evidence
        .iter()
        .filter(|evidence| evidence.plane == MemoryPlane::Procedural)
        .cloned()
        .collect::<Vec<_>>();
    if !procedural_evidence.is_empty() {
        rejected.push(rejected_candidate(
            rejected.len(),
            EvolutionProposalKind::SoulGovernanceRevision,
            EvolutionRejectReason::ProgramEvidenceOnly,
            procedural_evidence.clone(),
        ));
        let proposal_idx = proposals.len();
        push_capped(
            &mut proposals,
            procedural_promotion_proposal(input, procedural_evidence, proposal_idx),
            input.budget.max_proposals,
        );
    }

    if input
        .subject_assembly
        .as_ref()
        .is_some_and(|report| report.mounted)
    {
        let proposal_idx = proposals.len();
        push_capped(
            &mut proposals,
            subject_projection_refresh_proposal(input, evidence.to_vec(), proposal_idx),
            input.budget.max_proposals,
        );
    }

    if input.budget.allow_soul_revision && input.mode == EvolutionMode::Full {
        for source in evidence.iter().filter(|evidence| {
            evidence.plane.domain() == MemoryDomain::Program
                && evidence.plane != MemoryPlane::Procedural
                && evidence.plane != MemoryPlane::ArchiveEvidence
        }) {
            rejected.push(rejected_candidate(
                rejected.len(),
                EvolutionProposalKind::SoulGovernanceRevision,
                EvolutionRejectReason::ProgramEvidenceOnly,
                vec![source.clone()],
            ));
        }
    }

    (proposals, rejected)
}

fn subject_projection_refresh_proposal(
    input: &EvolutionInput,
    evidence: Vec<EvidenceRef>,
    idx: usize,
) -> EvolutionProposal {
    let candidate = WriteCandidate::new(
        input.identity.clone(),
        input.scope.clone(),
        "subject projection refresh from mounted assembly report",
    )
    .source(proposal_source(input, idx))
    .plane_hint(MemoryPlane::SubjectProjection)
    .privacy_layer(MentalPrivacyLayer::Shared)
    .evidence(EvidenceState::Supported)
    .canonical(false);

    EvolutionProposal {
        proposal_id: proposal_id(input, idx, "subject-refresh"),
        kind: EvolutionProposalKind::SubjectProjectionRefresh,
        disposition: EvolutionDisposition::RefreshSubjectProjection,
        target_plane: Some(MemoryPlane::SubjectProjection),
        source_evidence: evidence,
        confidence: Confidence::Medium,
        risk: EvolutionRisk::Medium,
        privacy_filtered: true,
        candidate_write: Some(candidate),
        rationale: "mounted subject assembly can refresh subject projection".to_owned(),
    }
}

fn archive_refresh_proposal(
    input: &EvolutionInput,
    evidence: EvidenceRef,
    idx: usize,
) -> EvolutionProposal {
    let candidate = WriteCandidate::new(
        input.identity.clone(),
        input.scope.clone(),
        format!("distilled archive evidence: {}", evidence.summary),
    )
    .source(proposal_source(input, idx))
    .plane_hint(MemoryPlane::ArchiveEvidence)
    .privacy_layer(MentalPrivacyLayer::Shared)
    .evidence(EvidenceState::Supported)
    .canonical(false);

    EvolutionProposal {
        proposal_id: proposal_id(input, idx, "archive-refresh"),
        kind: EvolutionProposalKind::MemoryRefresh,
        disposition: EvolutionDisposition::Refresh,
        target_plane: Some(MemoryPlane::ArchiveEvidence),
        source_evidence: vec![evidence],
        confidence: Confidence::Medium,
        risk: EvolutionRisk::Medium,
        privacy_filtered: true,
        candidate_write: Some(candidate),
        rationale: "archive evidence requires governed distillation before write".to_owned(),
    }
}

fn procedural_promotion_proposal(
    input: &EvolutionInput,
    evidence: Vec<EvidenceRef>,
    idx: usize,
) -> EvolutionProposal {
    let confidence = if evidence.len() >= 3 {
        Confidence::High
    } else {
        Confidence::Medium
    };
    let risk = if evidence.len() >= 3 {
        EvolutionRisk::Low
    } else {
        EvolutionRisk::Medium
    };
    let candidate = WriteCandidate::new(
        input.identity.clone(),
        input.scope.clone(),
        format!(
            "distilled procedural pattern from {} governed evidence refs",
            evidence.len()
        ),
    )
    .source(proposal_source(input, idx))
    .plane_hint(MemoryPlane::Procedural)
    .privacy_layer(MentalPrivacyLayer::Shared)
    .evidence(EvidenceState::Supported)
    .canonical(true);

    EvolutionProposal {
        proposal_id: proposal_id(input, idx, "procedural-promotion"),
        kind: EvolutionProposalKind::ProceduralPromotion,
        disposition: EvolutionDisposition::PromoteProcedural,
        target_plane: Some(MemoryPlane::Procedural),
        source_evidence: evidence,
        confidence,
        risk,
        privacy_filtered: true,
        candidate_write: Some(candidate),
        rationale: "stable governed procedural evidence can become a proposal".to_owned(),
    }
}

fn privacy_repair_proposals(
    input: &EvolutionInput,
    evidence: &[EvidenceRef],
) -> (Vec<EvolutionProposal>, Vec<EvolutionRejectedCandidate>) {
    let mut proposals = Vec::new();
    let mut rejected = Vec::new();

    for source in evidence
        .iter()
        .filter(|evidence| is_private_or_sealed(evidence))
    {
        let kind = if source.privacy_layer == MentalPrivacyLayer::Sealed {
            EvolutionProposalKind::PrivacyRepair
        } else {
            EvolutionProposalKind::NoWriteReport
        };
        let disposition = if kind == EvolutionProposalKind::PrivacyRepair {
            EvolutionDisposition::Refresh
        } else {
            EvolutionDisposition::NoWrite
        };
        rejected.push(rejected_candidate(
            rejected.len(),
            kind,
            EvolutionRejectReason::PrivacyFiltered,
            vec![source.clone()],
        ));
        let proposal_idx = proposals.len();
        push_capped(
            &mut proposals,
            EvolutionProposal {
                proposal_id: proposal_id(input, proposal_idx, "privacy"),
                kind,
                disposition,
                target_plane: None,
                source_evidence: vec![source.clone()],
                confidence: Confidence::Low,
                risk: EvolutionRisk::Blocked,
                privacy_filtered: true,
                candidate_write: None,
                rationale: "private or sealed evidence was blocked by privacy attack pass"
                    .to_owned(),
            },
            input.budget.max_proposals,
        );
    }

    (proposals, rejected)
}

fn bounded_evidence(input: &EvolutionInput) -> Vec<EvidenceRef> {
    let mut limit = input.evidence.len();
    if input.budget.max_events > 0 {
        limit = limit.min(input.budget.max_events);
    }
    if input.budget.max_records > 0 {
        limit = limit.min(input.budget.max_records);
    }
    input.evidence.iter().take(limit).cloned().collect()
}

fn branches_evaluated(input: &EvolutionInput) -> usize {
    match input.mode {
        EvolutionMode::Full => input.budget.max_branches.min(2),
        EvolutionMode::Compact | EvolutionMode::Consumer => 0,
    }
}

fn is_profile_trimmed(input: &EvolutionInput) -> bool {
    input.mode != EvolutionMode::Full
        || input.budget.max_branches == 0
        || input.evidence.len() > bounded_evidence(input).len()
        || matches!(
            input.profile,
            RuntimeProfile::EspCompact | RuntimeProfile::SdkEmbedded
        )
}

fn is_private_or_sealed(evidence: &EvidenceRef) -> bool {
    matches!(
        evidence.privacy_layer,
        MentalPrivacyLayer::Private | MentalPrivacyLayer::Sealed
    )
}

fn push_capped(
    proposals: &mut Vec<EvolutionProposal>,
    proposal: EvolutionProposal,
    max_proposals: usize,
) {
    if max_proposals == 0 || proposals.len() < max_proposals {
        proposals.push(proposal);
    }
}

fn rejected_candidate(
    idx: usize,
    attempted_kind: EvolutionProposalKind,
    reason: EvolutionRejectReason,
    evidence: Vec<EvidenceRef>,
) -> EvolutionRejectedCandidate {
    EvolutionRejectedCandidate {
        candidate_id: format!("rejected:{idx}"),
        attempted_kind,
        reason,
        evidence,
    }
}

fn proposal_id(input: &EvolutionInput, idx: usize, suffix: &str) -> String {
    format!("{}:{idx}:{suffix}", input.run_id)
}

fn proposal_source(input: &EvolutionInput, idx: usize) -> String {
    format!("s4-deterministic:{}:{idx}", input.run_id)
}
