use bm_core::{
    Confidence, EvidenceRef, EvidenceState, EvolutionAdjudication,
    EvolutionAdjudicationDisposition, EvolutionBackendKind, EvolutionBackendReport,
    EvolutionBudget, EvolutionDisposition, EvolutionInput, EvolutionMode, EvolutionProposal,
    EvolutionProposalBatch, EvolutionProposalKind, EvolutionProposalSummary, EvolutionRejectReason,
    EvolutionRejectedCandidate, EvolutionRisk, EvolutionRunReport, MemoryPlane, MentalPrivacyLayer,
    ProjectionReport, ProjectionSurface, RecallQuery, RecallSelectionReport, RuntimeProfile,
    SourceKind, SourceRef, SubjectAssemblyReport, WriteCandidate,
};

fn governed_evidence() -> EvidenceRef {
    EvidenceRef {
        record_id: Some("record-1".to_owned()),
        event_seq: Some(7),
        source: SourceRef::new(SourceKind::ReplayFixture, "s4-contract"),
        plane: MemoryPlane::ArchiveEvidence,
        privacy_layer: MentalPrivacyLayer::Shared,
        evidence: EvidenceState::ArchiveOnly,
        summary: "governed archive summary".to_owned(),
    }
}

#[test]
fn evolution_input_reuses_s2_s3_reports_and_budget_contracts() {
    let recall_report = RecallSelectionReport {
        query: RecallQuery::new("core").identity("agent"),
        profile: RuntimeProfile::DevFull,
        selected: Vec::new(),
        skipped: Vec::new(),
        plane_reports: Vec::new(),
        rerank: bm_core::CrossPlaneRerankReport::empty(bm_core::PromptRecallIntent::Mixed),
        warnings: Vec::new(),
    };
    let subject_assembly = SubjectAssemblyReport {
        mounted: true,
        sources_used: Vec::new(),
        sources_missing: Vec::new(),
        privacy_decisions: Vec::new(),
        profile: RuntimeProfile::DevFull,
        budget_bytes: RuntimeProfile::DevFull.projection_budget_bytes(),
    };
    let projection_report = ProjectionReport {
        surface: ProjectionSurface::Replay,
        blocks: Vec::new(),
        privacy_filtered_count: 0,
        subject_assembly: Some(subject_assembly.clone()),
        warnings: Vec::new(),
    };

    let input = EvolutionInput {
        run_id: "s4-run".to_owned(),
        identity: "agent".to_owned(),
        scope: "core".to_owned(),
        profile: RuntimeProfile::DevFull,
        mode: EvolutionMode::Full,
        evidence: vec![governed_evidence()],
        recall_report: Some(recall_report),
        projection_report: Some(projection_report),
        subject_assembly: Some(subject_assembly),
        budget: EvolutionBudget {
            max_events: 128,
            max_records: 64,
            max_branches: 4,
            max_proposals: 8,
            max_output_bytes: 8192,
            allow_private_layer: false,
            allow_soul_revision: true,
            allow_script_backend: false,
        },
    };

    assert_eq!(input.mode, EvolutionMode::Full);
    assert_eq!(input.profile, RuntimeProfile::DevFull);
    assert_eq!(input.evidence[0].plane, MemoryPlane::ArchiveEvidence);
    assert!(input.recall_report.is_some());
    assert!(input.projection_report.is_some());
    assert!(input.subject_assembly.is_some());
    assert!(!input.budget.allow_script_backend);
}

#[test]
fn compact_budget_rejects_script_backend_and_soul_revision_apply() {
    let compact_budget = EvolutionBudget {
        max_events: 16,
        max_records: 8,
        max_branches: 0,
        max_proposals: 2,
        max_output_bytes: 512,
        allow_private_layer: false,
        allow_soul_revision: false,
        allow_script_backend: false,
    };

    assert_eq!(EvolutionMode::Compact, EvolutionMode::Compact);
    assert_eq!(compact_budget.max_branches, 0);
    assert!(!compact_budget.allow_script_backend);
    assert!(!compact_budget.allow_soul_revision);
    assert!(!RuntimeProfile::EspCompact.allows_plane(MemoryPlane::SoulGovernance));
}

#[test]
fn proposal_is_candidate_write_only_not_store_mutation() {
    let candidate_write = WriteCandidate::new("agent", "core", "distilled procedural pattern")
        .source("s4-proposal")
        .plane_hint(MemoryPlane::Procedural)
        .privacy_layer(MentalPrivacyLayer::Shared)
        .evidence(EvidenceState::Supported);

    let proposal = EvolutionProposal {
        proposal_id: "proposal-1".to_owned(),
        kind: EvolutionProposalKind::ProceduralPromotion,
        disposition: EvolutionDisposition::PromoteProcedural,
        target_plane: Some(MemoryPlane::Procedural),
        source_evidence: vec![governed_evidence()],
        confidence: Confidence::Medium,
        risk: EvolutionRisk::Low,
        privacy_filtered: true,
        candidate_write: Some(candidate_write),
        rationale: "repeatable governed pattern".to_owned(),
    };

    assert_eq!(proposal.target_plane, Some(MemoryPlane::Procedural));
    assert!(proposal.candidate_write.is_some());
    assert!(!proposal.rationale.contains("raw private"));
}

#[test]
fn consumer_mode_summarizes_without_candidate_write_or_branch_detail() {
    let report = EvolutionRunReport {
        run_id: "consumer-run".to_owned(),
        mode: EvolutionMode::Consumer,
        evidence_read: 0,
        branches_evaluated: 0,
        proposals_emitted: 0,
        rejected_candidates: 1,
        privacy_filtered_count: 0,
        profile_trimmed: true,
        raw_private_exposed: false,
        warnings: vec!["consumer mode does not run evolution pass".to_owned()],
    };
    let rejected = EvolutionRejectedCandidate {
        candidate_id: "consumer-candidate".to_owned(),
        attempted_kind: EvolutionProposalKind::SoulGovernanceRevision,
        reason: EvolutionRejectReason::ConsumerMode,
        evidence: vec![governed_evidence()],
    };
    let batch = EvolutionProposalBatch {
        run_id: "consumer-run".to_owned(),
        profile: RuntimeProfile::SdkEmbedded,
        mode: EvolutionMode::Consumer,
        proposals: Vec::new(),
        rejected_candidates: vec![rejected],
        report,
    };
    let summary = EvolutionProposalSummary {
        run_id: batch.run_id.clone(),
        profile: batch.profile,
        mode: batch.mode,
        proposals_count: batch.proposals.len(),
        blocked_count: batch
            .proposals
            .iter()
            .filter(|proposal| proposal.risk == EvolutionRisk::Blocked)
            .count(),
        privacy_filtered_count: batch.report.privacy_filtered_count,
        profile_trimmed: batch.report.profile_trimmed,
    };

    assert_eq!(summary.mode, EvolutionMode::Consumer);
    assert_eq!(summary.proposals_count, 0);
    assert_eq!(batch.report.branches_evaluated, 0);
    assert!(!batch.report.raw_private_exposed);
    assert_eq!(
        batch.rejected_candidates[0].reason,
        EvolutionRejectReason::ConsumerMode
    );
}

#[test]
fn backend_and_adjudication_do_not_own_write_authority() {
    let batch = EvolutionProposalBatch {
        run_id: "deterministic-run".to_owned(),
        profile: RuntimeProfile::DevFull,
        mode: EvolutionMode::Full,
        proposals: vec![EvolutionProposal {
            proposal_id: "proposal-1".to_owned(),
            kind: EvolutionProposalKind::NoWriteReport,
            disposition: EvolutionDisposition::NoWrite,
            target_plane: None,
            source_evidence: vec![governed_evidence()],
            confidence: Confidence::Low,
            risk: EvolutionRisk::Blocked,
            privacy_filtered: true,
            candidate_write: None,
            rationale: "blocked proposal remains inspection-only".to_owned(),
        }],
        rejected_candidates: Vec::new(),
        report: EvolutionRunReport {
            run_id: "deterministic-run".to_owned(),
            mode: EvolutionMode::Full,
            evidence_read: 1,
            branches_evaluated: 1,
            proposals_emitted: 1,
            rejected_candidates: 0,
            privacy_filtered_count: 1,
            profile_trimmed: false,
            raw_private_exposed: false,
            warnings: Vec::new(),
        },
    };
    let backend_report = EvolutionBackendReport {
        batch,
        backend: EvolutionBackendKind::Deterministic,
        deterministic: true,
    };
    let adjudication = EvolutionAdjudication {
        selected_proposal_id: Some("proposal-1".to_owned()),
        strongest_rejected_candidate_id: None,
        disposition: EvolutionAdjudicationDisposition::ForceNoWrite,
        summary: "adjudication explains proposal handling only".to_owned(),
    };

    assert_eq!(backend_report.backend, EvolutionBackendKind::Deterministic);
    assert!(backend_report.deterministic);
    assert_eq!(
        adjudication.disposition,
        EvolutionAdjudicationDisposition::ForceNoWrite
    );
    assert!(backend_report.batch.proposals[0].candidate_write.is_none());
    assert!(!backend_report.batch.report.raw_private_exposed);
}
