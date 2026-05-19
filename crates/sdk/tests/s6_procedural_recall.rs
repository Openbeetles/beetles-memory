use bm_core::{ProceduralEvidenceRef, ProceduralSkillDraft, ProceduralSkillOrigin, RuntimeProfile};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn procedural_recall_reports_selected_and_skipped_candidates() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    runtime.write_procedural_skill(user_skill("Release checklist"));
    runtime.write_procedural_skill(
        ProceduralSkillDraft::new(
            "agent:s6",
            "project:s6",
            ProceduralSkillOrigin::RuntimeLearned,
            "Serial recovery",
            "serial recovery",
            "When serial framing stalls, first reset the reader, then retry one narrow probe.",
        )
        .evidence(vec![ProceduralEvidenceRef::new("replay:s6", "worked once")]),
    );

    let report = runtime.recall_procedural_skills(bm_core::ProceduralSkillRecallQuery::new(
        "project:s6",
        "release checklist",
    ));

    assert_eq!(report.backend, "store_scan");
    assert_eq!(report.candidate_count, 2);
    assert_eq!(report.selected_count, 1);
    assert!(!report.skipped.is_empty());
    assert!(report.selected[0]
        .score
        .reason_fragments
        .iter()
        .any(|reason| reason == "trigger_match"));
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
