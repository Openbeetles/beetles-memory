use bm_core::{
    MemoryPlane, ProceduralSkillDraft, ProceduralSkillOrigin, ProceduralSkillState,
    ProceduralSkillWriteAction, ProceduralSkillWriteReason, ProjectionSurface, PromptRecallIntent,
    RecallQuery, RuntimeProfile, WriteCandidate, WriteDecision, WriteRejectReason,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn user_provided_skill_writes_recalls_and_projects_as_hint() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    let write = runtime.write_procedural_skill(user_skill("Release checklist"));

    assert_eq!(write.decision, WriteDecision::Accepted);
    assert_eq!(write.plane, Some(MemoryPlane::Procedural));
    assert_eq!(
        write.procedural.as_ref().map(|report| report.action),
        Some(ProceduralSkillWriteAction::Inserted)
    );
    assert_eq!(
        write.procedural.as_ref().map(|report| report.reason),
        Some(ProceduralSkillWriteReason::UserProvidedAccepted)
    );

    let recall = runtime.recall_procedural_skills(bm_core::ProceduralSkillRecallQuery::new(
        "project:s6",
        "release checklist",
    ));
    assert_eq!(recall.selected_count, 1);
    assert_eq!(recall.selected[0].state, ProceduralSkillState::Active);

    let general_recall = runtime.recall(
        RecallQuery::new("project:s6")
            .intent(PromptRecallIntent::Procedural)
            .plane(MemoryPlane::Procedural),
    );
    let projection = runtime.project(&general_recall, ProjectionSurface::Prompt);
    assert_eq!(projection.blocks.len(), 1);
    assert!(projection.blocks[0]
        .content
        .contains("Procedural skill hint"));
    assert!(projection.blocks[0]
        .content
        .contains("not execution authority"));
}

#[test]
fn procedural_content_is_not_written_as_canonical_fact() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    let write = runtime.write(
        WriteCandidate::new(
            "agent:s6",
            "project:s6",
            "When the release gate fails, first inspect the failing contract, then run the narrow test.",
        )
        .source("task-learning")
        .plane_hint(MemoryPlane::SharedFactual),
    );

    assert_eq!(write.decision, WriteDecision::Rejected);
    assert_eq!(
        write.governance.reject_reason,
        Some(WriteRejectReason::RoutedToProcedural)
    );
}

fn user_skill(title: &str) -> ProceduralSkillDraft {
    ProceduralSkillDraft::new(
        "agent:s6",
        "project:s6",
        ProceduralSkillOrigin::UserProvided,
        title,
        title,
        "When preparing a release, first verify status, then run tests, then commit.",
    )
}
