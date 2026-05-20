use bm_core::{
    MemoryPlane, ProceduralSkillReuseOutcome, PromptRecallIntent, RecallQuery, RuntimeProfile,
    WriteCandidate,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn recall_report_contains_plane_reports_rerank_and_skip_reasons() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    runtime.write(
        WriteCandidate::new("agent:1", "task:s1", "项目名称是 Beetle Memory")
            .source("operator")
            .plane_hint(MemoryPlane::SharedFactual),
    );
    let procedural = runtime.write(
        WriteCandidate::new(
            "agent:1",
            "task:s1",
            "下次遇到合同漂移，先补 core 测试再改 SDK",
        )
        .source("task-learning")
        .plane_hint(MemoryPlane::Procedural),
    );
    let record_id = procedural.record_id.expect("procedural record id");
    runtime.record_procedural_skill_outcome(
        std::slice::from_ref(&record_id),
        ProceduralSkillReuseOutcome::Succeeded,
        10,
        "validated by recall report test",
    );

    let report = runtime.recall(
        RecallQuery::new("task:s1")
            .intent(PromptRecallIntent::Procedural)
            .limit(1),
    );

    assert_eq!(report.selected.len(), 1);
    assert_eq!(report.selected[0].plane, MemoryPlane::Procedural);
    assert!(report.selected[0]
        .reason_fragments
        .iter()
        .any(|reason| reason.contains("intent=Procedural")));
    assert!(!report.skipped.is_empty());
    assert!(report.skipped[0]
        .reason_fragments
        .iter()
        .any(|reason| reason.contains("limit_reached")));
    assert!(report
        .plane_reports
        .iter()
        .any(|plane| plane.plane == MemoryPlane::Procedural
            && plane.selected == 1
            && plane.top_score.is_some()
            && plane.top_reason.is_some()));
    assert_eq!(report.rerank.intent, PromptRecallIntent::Procedural);
    assert_eq!(report.rerank.top_planes[0].candidate_count, 1);
    assert_eq!(report.rerank.top_planes[0].selected_count, 1);
    assert_eq!(
        report.rerank.top_candidates[0].plane,
        MemoryPlane::Procedural
    );
    assert!(report.rerank.top_candidates[0].selected);
    assert!(
        report.rerank.top_candidates[0].rerank_score
            >= report.rerank.top_candidates[0].original_score
    );
    assert!(!report.rerank.skipped_candidates.is_empty());
}
