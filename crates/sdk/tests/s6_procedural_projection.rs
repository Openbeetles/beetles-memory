use bm_core::{ProceduralSkillDraft, ProceduralSkillOrigin, ProjectionSurface, RuntimeProfile};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn procedural_projection_is_hint_not_execution_authority() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();
    runtime.write_procedural_skill(ProceduralSkillDraft::new(
        "agent:s6",
        "project:s6",
        ProceduralSkillOrigin::UserProvided,
        "Release checklist",
        "release checklist",
        "When preparing a release, first verify status, then run tests, then commit.",
    ));

    let recall = runtime.recall_procedural_skills(bm_core::ProceduralSkillRecallQuery::new(
        "project:s6",
        "release checklist",
    ));
    let projection = runtime.project_procedural_skills(&recall, ProjectionSurface::ToolContext);

    assert_eq!(projection.blocks.len(), 1);
    assert!(projection.blocks[0]
        .content
        .contains("Procedural skill hint"));
    assert!(projection.blocks[0]
        .content
        .contains("not execution authority"));
    assert!(!projection.blocks[0]
        .content
        .contains(concat!("market", "place")));
}
