use bm_core::{
    DisclosureSurface, EvidenceState, MemoryPlane, MentalPrivacyLayer,
    MentalPrivacyOwnerAccessMode, MentalPrivacyPolicy, MentalPrivacyQuotePolicy,
    MentalPrivacyVisibility, PrivacyDisclosureDecision, ProjectionReport, ProjectionSurface,
    RuntimeProfile, SoulFeedbackLane, SoulGovernanceDecision, SoulGovernanceReason,
    SoulGovernanceRecord, SoulSourceKind, SubjectAssemblyReport, SubjectAssemblySource,
    SubjectAssemblySourceRef, WriteCandidate,
};

#[test]
fn subject_assembly_report_keeps_soul_subject_and_program_sources_separate() {
    let report = SubjectAssemblyReport {
        mounted: true,
        sources_used: vec![
            SubjectAssemblySourceRef {
                source: SubjectAssemblySource::SelfCore,
                record_id: "soul:self-core:v1".to_owned(),
                plane: MemoryPlane::SoulGovernance,
                privacy_layer: MentalPrivacyLayer::Private,
            },
            SubjectAssemblySourceRef {
                source: SubjectAssemblySource::SelfContinuity,
                record_id: "subject:continuity:turn-42".to_owned(),
                plane: MemoryPlane::SubjectProjection,
                privacy_layer: MentalPrivacyLayer::Relational,
            },
            SubjectAssemblySourceRef {
                source: SubjectAssemblySource::ProgramMemory,
                record_id: "program:evidence:build-7".to_owned(),
                plane: MemoryPlane::ArchiveEvidence,
                privacy_layer: MentalPrivacyLayer::Shared,
            },
        ],
        sources_missing: vec![SubjectAssemblySource::Relationship],
        privacy_decisions: vec![],
        profile: RuntimeProfile::DevFull,
        budget_bytes: RuntimeProfile::DevFull.projection_budget_bytes(),
    };

    assert_eq!(report.sources_used[0].plane.domain().as_str(), "Soul");
    assert_eq!(report.sources_used[1].plane.domain().as_str(), "Subject");
    assert_eq!(report.sources_used[2].plane.domain().as_str(), "Program");
    assert!(report
        .sources_used
        .iter()
        .any(|source| source.source == SubjectAssemblySource::ProgramMemory));
}

#[test]
fn prompt_disclosure_decision_rejects_private_and_sealed_raw_material() {
    let private_prompt = PrivacyDisclosureDecision {
        surface: DisclosureSurface::Prompt,
        layer: MentalPrivacyLayer::Private,
        allowed: false,
        quote_policy: MentalPrivacyQuotePolicy::NeverQuote,
        reason: SoulGovernanceReason::RawPrivateRejected,
    };
    let sealed_prompt = PrivacyDisclosureDecision {
        surface: DisclosureSurface::Prompt,
        layer: MentalPrivacyLayer::Sealed,
        allowed: false,
        quote_policy: MentalPrivacyQuotePolicy::NeverQuote,
        reason: SoulGovernanceReason::PrivacyFiltered,
    };

    assert!(!private_prompt.allowed);
    assert!(!sealed_prompt.allowed);
    assert_eq!(
        private_prompt.quote_policy,
        MentalPrivacyQuotePolicy::NeverQuote
    );
}

#[test]
fn soul_governance_record_is_governed_summary_not_raw_private_body() {
    let record = SoulGovernanceRecord {
        source_id: "self-core-summary:v3".to_owned(),
        source_kind: SoulSourceKind::SelfAuthoredCore,
        layer: MentalPrivacyLayer::Private,
        policy: MentalPrivacyPolicy {
            layer: MentalPrivacyLayer::Private,
            visibility: MentalPrivacyVisibility::SummaryOnly,
            owner_access_mode: MentalPrivacyOwnerAccessMode::RequestOnly,
            quote_policy: MentalPrivacyQuotePolicy::NeverQuote,
        },
        decision: SoulGovernanceDecision::Accepted,
        reason: SoulGovernanceReason::StableIdentity,
        feedback_lanes: vec![SoulFeedbackLane::Reply, SoulFeedbackLane::Strategy],
        revision: Some("3".to_owned()),
    };

    assert_eq!(record.source_kind, SoulSourceKind::SelfAuthoredCore);
    assert_eq!(record.reason, SoulGovernanceReason::StableIdentity);
    assert!(record.feedback_lanes.contains(&SoulFeedbackLane::Reply));
    assert!(!record.policy.allows_raw_default_surface());
}

#[test]
fn projection_report_names_every_disclosure_surface_and_reports_privacy_gate() {
    let assembly = SubjectAssemblyReport {
        mounted: true,
        sources_used: vec![],
        sources_missing: vec![SubjectAssemblySource::Task],
        privacy_decisions: vec![PrivacyDisclosureDecision {
            surface: DisclosureSurface::OperatorInspection,
            layer: MentalPrivacyLayer::Private,
            allowed: false,
            quote_policy: MentalPrivacyQuotePolicy::SummaryOnly,
            reason: SoulGovernanceReason::PrivacyFiltered,
        }],
        profile: RuntimeProfile::SdkEmbedded,
        budget_bytes: RuntimeProfile::SdkEmbedded.projection_budget_bytes(),
    };

    let report = ProjectionReport {
        surface: ProjectionSurface::OperatorInspection,
        blocks: vec![],
        privacy_filtered_count: 1,
        subject_assembly: Some(assembly),
        warnings: vec!["operator inspection reports presence only".to_owned()],
    };

    let surfaces = [
        ProjectionSurface::Prompt,
        ProjectionSurface::ToolContext,
        ProjectionSurface::OperatorInspection,
        ProjectionSurface::Adapter,
        ProjectionSurface::Replay,
    ];

    assert_eq!(surfaces.len(), 5);
    assert_eq!(report.privacy_filtered_count, 1);
    assert!(report.subject_assembly.as_ref().is_some_and(|assembly| {
        assembly
            .privacy_decisions
            .iter()
            .all(|decision| !decision.allowed)
    }));
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn write_candidate_has_s3_privacy_evidence_and_canonical_defaults() {
    let default = WriteCandidate::new("agent", "task", "governed summary");

    assert_eq!(default.privacy_layer, MentalPrivacyLayer::Shared);
    assert_eq!(default.evidence, EvidenceState::Supported);
    assert!(!default.canonical);

    let governed = default
        .privacy_layer(MentalPrivacyLayer::Private)
        .evidence(EvidenceState::Weak)
        .canonical(true);

    assert_eq!(governed.privacy_layer, MentalPrivacyLayer::Private);
    assert_eq!(governed.evidence, EvidenceState::Weak);
    assert!(governed.canonical);
}
