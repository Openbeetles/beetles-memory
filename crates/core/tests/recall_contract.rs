use bm_core::{
    CrossPlanePlaneSignal, CrossPlaneRerankCandidate, CrossPlaneRerankReport, MemoryDomain,
    MemoryPlane, MemoryRecordMeta, PromptRecallIntent, RecallPlaneReport, RecallQuery,
    RecallScoreBreakdown, RecallSelection, RecallSelectionReport, RecallSkipReason, RecallWarning,
    RuntimeProfile, SkippedRecallCandidate, SourceKind, SourceRef,
};

#[test]
fn recall_report_explains_selection_skips_rerank_and_warnings() {
    let query = RecallQuery::new("task:s1")
        .identity("agent:beetle-memory")
        .intent(PromptRecallIntent::Procedural)
        .limit(3);
    let source = SourceRef::new(SourceKind::TaskLearning, "beetle-memory:s1")
        .origin_path("crates/sdk/tests/procedural_memory.rs");

    let score = RecallScoreBreakdown {
        lexical: 10,
        semantic: 18,
        recency: 6,
        provenance: 12,
        intent: 40,
        total: 86,
    };

    let selected = RecallSelection {
        record_id: "proc-1".to_owned(),
        domain: MemoryDomain::Program,
        plane: MemoryPlane::Procedural,
        content: "遇到 S1 合同漂移时，先补 core report 测试再改 SDK。".to_owned(),
        source: source.clone(),
        score: score.clone(),
        canonical: true,
        privacy_filtered: false,
        meta: MemoryRecordMeta::default_for_plane(MemoryPlane::Procedural),
        reason_fragments: vec![
            "intent=Procedural".to_owned(),
            "plane=Procedural".to_owned(),
        ],
    };

    let skipped = SkippedRecallCandidate {
        record_id: "fact-1".to_owned(),
        plane: MemoryPlane::SharedFactual,
        reason: RecallSkipReason::LowerScore,
        reason_fragments: vec!["lower_score_than_procedural_candidate".to_owned()],
    };

    let report = RecallSelectionReport {
        query: query.clone(),
        profile: RuntimeProfile::DevFull,
        selected: vec![selected],
        skipped: vec![skipped],
        plane_reports: vec![RecallPlaneReport {
            plane: MemoryPlane::Procedural,
            available: 1,
            selected: 1,
            skipped: 0,
            top_score: Some(86),
            top_reason: Some("intent=Procedural;plane=Procedural".to_owned()),
        }],
        rerank: CrossPlaneRerankReport {
            intent: PromptRecallIntent::Procedural,
            top_planes: vec![CrossPlanePlaneSignal {
                plane: MemoryPlane::Procedural,
                score: 86,
                candidate_count: 1,
                selected_count: 1,
                top_reason: Some("intent=Procedural;plane=Procedural".to_owned()),
            }],
            top_candidates: vec![CrossPlaneRerankCandidate {
                record_id: "proc-1".to_owned(),
                plane: MemoryPlane::Procedural,
                selected: true,
                original_score: 86,
                rerank_score: 96,
                score: 86,
                source,
                reason_fragments: vec![
                    "intent=Procedural".to_owned(),
                    "plane=Procedural".to_owned(),
                    "rerank:intent=Procedural".to_owned(),
                ],
            }],
            skipped_candidates: vec![SkippedRecallCandidate {
                record_id: "fact-1".to_owned(),
                plane: MemoryPlane::SharedFactual,
                reason: RecallSkipReason::LowerScore,
                reason_fragments: vec!["lower_score_than_procedural_candidate".to_owned()],
            }],
            warnings: vec![
                "profile_budget_trimmed:profile=DevFull;before=2048;after=1536".to_owned(),
            ],
        },
        warnings: vec![RecallWarning::ProfileBudgetTrimmed {
            profile: RuntimeProfile::DevFull,
            before: 2048,
            after: 1536,
        }],
    };

    assert_eq!(report.query, query);
    assert_eq!(report.selected.len(), 1);
    assert_eq!(report.skipped[0].reason, RecallSkipReason::LowerScore);
    assert!(report.skipped[0].reason_fragments[0].contains("lower_score"));
    assert_eq!(report.plane_reports[0].plane, MemoryPlane::Procedural);
    assert_eq!(report.plane_reports[0].top_score, Some(86));
    assert_eq!(report.rerank.top_planes[0].plane, MemoryPlane::Procedural);
    assert_eq!(report.rerank.top_planes[0].candidate_count, 1);
    assert_eq!(report.rerank.top_candidates[0].original_score, 86);
    assert_eq!(report.rerank.top_candidates[0].rerank_score, 96);
    assert!(report.rerank.top_candidates[0]
        .reason_fragments
        .iter()
        .any(|reason| reason.contains("rerank")));
    assert_eq!(report.rerank.skipped_candidates.len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn memory_planes_keep_s1_domain_boundaries() {
    assert_eq!(MemoryPlane::SharedFactual.domain(), MemoryDomain::Program);
    assert_eq!(MemoryPlane::Procedural.domain(), MemoryDomain::Program);
    assert_eq!(
        MemoryPlane::ContinuityCapsule.domain(),
        MemoryDomain::Program
    );
    assert_eq!(MemoryPlane::ArchiveEvidence.domain(), MemoryDomain::Program);
    assert_eq!(MemoryPlane::TaskRecall.domain(), MemoryDomain::Program);
    assert_eq!(
        MemoryPlane::SubjectProjection.domain(),
        MemoryDomain::Subject
    );
    assert_eq!(MemoryPlane::SoulGovernance.domain(), MemoryDomain::Soul);
}
