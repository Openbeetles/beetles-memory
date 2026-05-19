use bm_core::{
    CrossPlanePlaneSignal, CrossPlaneRerankCandidate, CrossPlaneRerankReport, MemoryDomain,
    MemoryPlane, PromptRecallIntent, RecallPlaneReport, RecallQuery, RecallScoreBreakdown,
    RecallSelection, RecallSelectionReport, RecallSkipReason, RecallWarning, RuntimeProfile,
    SkippedRecallCandidate, SourceKind, SourceRef,
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
    };

    let skipped = SkippedRecallCandidate {
        record_id: "fact-1".to_owned(),
        plane: MemoryPlane::SharedFactual,
        reason: RecallSkipReason::LowerScore,
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
        }],
        rerank: CrossPlaneRerankReport {
            intent: PromptRecallIntent::Procedural,
            top_planes: vec![CrossPlanePlaneSignal {
                plane: MemoryPlane::Procedural,
                score: 86,
            }],
            top_candidates: vec![CrossPlaneRerankCandidate {
                record_id: "proc-1".to_owned(),
                plane: MemoryPlane::Procedural,
                score: 86,
                source,
            }],
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
    assert_eq!(report.plane_reports[0].plane, MemoryPlane::Procedural);
    assert_eq!(report.rerank.top_planes[0].plane, MemoryPlane::Procedural);
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
