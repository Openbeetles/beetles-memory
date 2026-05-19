use bm_core::{MemoryPlane, PromptRecallIntent, RecallQuery, RuntimeProfile, WriteCandidate};
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
    runtime.write(
        WriteCandidate::new(
            "agent:1",
            "task:s1",
            "下次遇到合同漂移，先补 core 测试再改 SDK",
        )
        .source("task-learning")
        .plane_hint(MemoryPlane::Procedural),
    );

    let report = runtime.recall(
        RecallQuery::new("task:s1")
            .intent(PromptRecallIntent::Procedural)
            .limit(1),
    );

    assert_eq!(report.selected.len(), 1);
    assert_eq!(report.selected[0].plane, MemoryPlane::Procedural);
    assert!(!report.skipped.is_empty());
    assert!(report
        .plane_reports
        .iter()
        .any(|plane| plane.plane == MemoryPlane::Procedural && plane.selected == 1));
    assert_eq!(report.rerank.intent, PromptRecallIntent::Procedural);
    assert_eq!(
        report.rerank.top_candidates[0].plane,
        MemoryPlane::Procedural
    );
}
