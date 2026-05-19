use bm_core::{
    MemoryPlane, ProceduralSkillReuseOutcome, ProceduralSkillState, ProjectionSurface,
    PromptRecallIntent, RecallQuery, RuntimeProfile, WriteCandidate,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn procedural_candidate_is_routed_to_procedural_plane_without_hint() {
    let store = InMemoryStore::default();
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(store)
        .build();

    let write = runtime.write(
        WriteCandidate::new(
            "agent:1",
            "task:s1",
            "下次遇到 S1 合同漂移时，先补 core report 测试，再改 SDK。",
        )
        .source("task-learning"),
    );

    assert_eq!(write.plane, Some(MemoryPlane::Procedural));
    assert_eq!(
        write.procedural.as_ref().map(|report| report.state),
        Some(ProceduralSkillState::Candidate)
    );

    let before_feedback = runtime.recall(
        RecallQuery::new("task:s1")
            .intent(PromptRecallIntent::Procedural)
            .plane(MemoryPlane::Procedural),
    );
    assert!(before_feedback.selected.is_empty());

    let record_id = write.record_id.expect("procedural record id");
    runtime.record_procedural_skill_outcome(
        std::slice::from_ref(&record_id),
        ProceduralSkillReuseOutcome::Succeeded,
        10,
        "validated by replay",
    );

    let recall = runtime.recall(
        RecallQuery::new("task:s1")
            .intent(PromptRecallIntent::Procedural)
            .plane(MemoryPlane::Procedural),
    );

    assert_eq!(recall.selected.len(), 1);
    assert_eq!(recall.selected[0].plane, MemoryPlane::Procedural);
    assert!(recall.selected[0].canonical);

    let projection = runtime.project(&recall, ProjectionSurface::Prompt);
    assert_eq!(projection.blocks.len(), 1);
    assert!(projection.blocks[0]
        .content
        .starts_with("Procedural skill hint, not execution authority: "));
}
