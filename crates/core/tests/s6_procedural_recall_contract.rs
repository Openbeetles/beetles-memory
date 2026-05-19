use bm_core::{
    score_procedural_skill_record, MemoryPlane, MemoryRecord, MemoryRecordMeta, NewMemoryRecord,
    ProceduralSkillDraft, ProceduralSkillOrigin, ProceduralSkillState, RecallScoreBreakdown,
};

#[test]
fn procedural_scoring_prefers_active_validated_user_skill() {
    let active = record(
        "mem-1",
        ProceduralSkillOrigin::UserProvided,
        ProceduralSkillState::Active,
        2,
    );
    let quarantined = record(
        "mem-2",
        ProceduralSkillOrigin::UserProvided,
        ProceduralSkillState::Quarantined,
        5,
    );

    let active_score = score_procedural_skill_record(&active, "release checklist", "project:s6");
    let quarantine_score =
        score_procedural_skill_record(&quarantined, "release checklist", "project:s6");

    assert!(active_score.total_score > quarantine_score.total_score);
    assert!(active_score
        .reason_fragments
        .iter()
        .any(|reason| reason == "active"));
    assert!(quarantine_score
        .reason_fragments
        .iter()
        .any(|reason| reason == "quarantined"));
}

fn record(
    id: &str,
    origin: ProceduralSkillOrigin,
    state: ProceduralSkillState,
    validated: u32,
) -> MemoryRecord {
    let draft = ProceduralSkillDraft::new(
        "agent:s6",
        "project:s6",
        origin,
        "Release checklist",
        "release checklist",
        "When preparing a release, first verify status, then run tests, then commit.",
    );
    let mut meta = MemoryRecordMeta::default_for_plane(MemoryPlane::Procedural);
    let mut procedural = bm_core::procedural_skill_meta_from_draft(&draft, state, 10);
    procedural.validated_success_count = validated;
    procedural.quality_score = bm_core::compute_procedural_skill_quality(&procedural);
    meta.procedural = Some(procedural);
    let new = NewMemoryRecord {
        identity: "agent:s6".to_owned(),
        scope: "project:s6".to_owned(),
        content: draft.procedure,
        source: "unit-test".to_owned(),
        domain: MemoryPlane::Procedural.domain(),
        plane: MemoryPlane::Procedural,
        meta,
    };
    MemoryRecord {
        id: id.to_owned(),
        identity: new.identity,
        scope: new.scope,
        content: new.content,
        source: new.source,
        domain: new.domain,
        plane: new.plane,
        meta: new.meta,
    }
}

fn _assert_breakdown_is_public(_: RecallScoreBreakdown) {}
