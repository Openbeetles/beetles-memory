use bm_core::{
    ProceduralSkillDraft, ProceduralSkillImportEnvelope, ProceduralSkillOrigin,
    ProceduralSkillState, RuntimeProfile,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn imported_skill_is_quarantined_until_adjudicated() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();
    let envelope = ProceduralSkillImportEnvelope::new(skill(), "digest-1");
    let json = serde_json::to_string(&envelope).expect("serialize envelope");

    let imported = runtime.import_procedural_skill(&json, false);
    assert_eq!(imported.imported, 1);
    assert_eq!(imported.quarantined, 1);

    let recall = runtime.recall_procedural_skills(bm_core::ProceduralSkillRecallQuery::new(
        "project:s6",
        "serial recovery",
    ));
    assert_eq!(recall.selected_count, 0);
    assert_eq!(recall.skipped.len(), 1);

    let adopted = runtime.import_procedural_skill(&json, true);
    assert_eq!(adopted.adopted, 1);
    let recall = runtime.recall_procedural_skills(bm_core::ProceduralSkillRecallQuery::new(
        "project:s6",
        "serial recovery",
    ));
    assert_eq!(recall.selected_count, 1);
    assert_eq!(recall.selected[0].state, ProceduralSkillState::Active);
}

fn skill() -> ProceduralSkillDraft {
    ProceduralSkillDraft::new(
        "agent:s6",
        "project:s6",
        ProceduralSkillOrigin::RuntimeLearned,
        "Serial recovery",
        "serial recovery",
        "When serial framing stalls, first reset the reader, then retry one narrow probe.",
    )
}
