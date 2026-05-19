use bm_core::{
    MemoryPlane, ProceduralEvidenceRef, ProceduralSkillDraft, ProceduralSkillOrigin,
    ProceduralSkillReuseOutcome, ProceduralSkillState, ProceduralSkillWriteAction,
    ProceduralSkillWriteReason, RecallQuery, RuntimeProfile, WriteDecision,
};
use bm_sdk::MemoryRuntimeBuilder;
use bm_store::InMemoryStore;

#[test]
fn runtime_learned_skill_requires_evidence_then_feedback_promotes_quality() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    let rejected = runtime.write_procedural_skill(runtime_skill(Vec::new()));
    assert_eq!(rejected.decision, WriteDecision::Rejected);

    let accepted = runtime.write_procedural_skill(runtime_skill(vec![ProceduralEvidenceRef::new(
        "replay:s6",
        "same recovery worked in replay",
    )]));
    assert_eq!(accepted.decision, WriteDecision::Accepted);
    assert_eq!(
        accepted.procedural.as_ref().map(|report| report.state),
        Some(ProceduralSkillState::Candidate)
    );
    assert_eq!(
        accepted.procedural.as_ref().map(|report| report.reason),
        Some(ProceduralSkillWriteReason::RuntimeEvidenceAccepted)
    );

    let record_id = accepted.record_id.expect("record id");
    let outcome = runtime.record_procedural_skill_outcome(
        std::slice::from_ref(&record_id),
        ProceduralSkillReuseOutcome::Succeeded,
        50,
        "reused successfully",
    );
    assert_eq!(outcome.updated, 1);

    let recall = runtime.recall_procedural_skills(bm_core::ProceduralSkillRecallQuery::new(
        "project:s6",
        "serial recovery",
    ));
    assert_eq!(recall.selected_count, 1);
    assert_eq!(recall.selected[0].validated_success_count, 1);
    assert_eq!(recall.selected[0].state, ProceduralSkillState::Active);

    let mismatch = runtime.record_procedural_skill_outcome(
        std::slice::from_ref(&record_id),
        ProceduralSkillReuseOutcome::Mismatch,
        60,
        "needs revision",
    );
    assert_eq!(mismatch.updated, 1);

    let generic = runtime.recall(
        RecallQuery::new("project:s6")
            .plane(MemoryPlane::Procedural)
            .limit(1),
    );
    let meta = generic.selected[0]
        .meta
        .procedural
        .as_ref()
        .expect("procedural metadata");
    assert_eq!(meta.use_count, 2);
    assert_eq!(meta.validated_success_count, 1);
    assert_eq!(meta.mismatch_count, 1);
    assert!(meta.revision_pending);
}

#[test]
fn higher_quality_same_slot_supersedes_lower_quality_runtime_skill() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    runtime.write_procedural_skill(runtime_skill(vec![ProceduralEvidenceRef::new(
        "replay:s6:1",
        "worked once",
    )]));
    let better =
        runtime.write_procedural_skill(runtime_skill_with_procedure(
            vec![
                ProceduralEvidenceRef::new("replay:s6:1", "worked once"),
                ProceduralEvidenceRef::new("replay:s6:2", "worked twice"),
                ProceduralEvidenceRef::new("replay:s6:3", "worked three times"),
            ],
            "When serial framing stalls, first reset the reader, then verify negotiated baud, then retry one narrow probe.",
        ));

    assert_eq!(
        better.procedural.as_ref().map(|report| report.action),
        Some(ProceduralSkillWriteAction::Superseded)
    );
}

#[test]
fn same_strategy_same_slot_refreshes_instead_of_superseding() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    runtime.write_procedural_skill(runtime_skill(vec![ProceduralEvidenceRef::new(
        "replay:s6:1",
        "worked once",
    )]));
    let refreshed = runtime.write_procedural_skill(runtime_skill(vec![
        ProceduralEvidenceRef::new("replay:s6:1", "worked once"),
        ProceduralEvidenceRef::new("replay:s6:2", "worked twice"),
    ]));

    assert_eq!(
        refreshed.procedural.as_ref().map(|report| report.action),
        Some(ProceduralSkillWriteAction::Refreshed)
    );
}

#[test]
fn lower_quality_same_slot_skill_does_not_replace_active_record() {
    let mut runtime = MemoryRuntimeBuilder::new(RuntimeProfile::DevFull)
        .store(InMemoryStore::default())
        .build();

    let strong = runtime.write_procedural_skill(user_skill(vec![
        ProceduralEvidenceRef::new("user:s6:1", "verified in project replay"),
        ProceduralEvidenceRef::new("user:s6:2", "verified in live dry run"),
    ]));
    let strong_quality = strong
        .procedural
        .as_ref()
        .map(|report| report.quality_score)
        .unwrap_or_default();
    assert_eq!(strong.decision, WriteDecision::Accepted);

    let weaker = runtime.write_procedural_skill(user_skill(Vec::new()));

    assert_eq!(weaker.decision, WriteDecision::Rejected);
    assert_eq!(
        weaker.procedural.as_ref().map(|report| report.action),
        Some(ProceduralSkillWriteAction::Rejected)
    );

    let recall = runtime.recall_procedural_skills(bm_core::ProceduralSkillRecallQuery::new(
        "project:s6",
        "serial recovery",
    ));
    assert_eq!(recall.selected_count, 1);
    assert_eq!(recall.selected[0].quality_score, strong_quality);
}

fn runtime_skill(evidence: Vec<ProceduralEvidenceRef>) -> ProceduralSkillDraft {
    runtime_skill_with_procedure(
        evidence,
        "When serial framing stalls, first reset the reader, then retry one narrow probe.",
    )
}

fn runtime_skill_with_procedure(
    evidence: Vec<ProceduralEvidenceRef>,
    procedure: &str,
) -> ProceduralSkillDraft {
    ProceduralSkillDraft::new(
        "agent:s6",
        "project:s6",
        ProceduralSkillOrigin::RuntimeLearned,
        "Serial recovery",
        "serial recovery",
        procedure,
    )
    .evidence(evidence)
}

fn user_skill(evidence: Vec<ProceduralEvidenceRef>) -> ProceduralSkillDraft {
    ProceduralSkillDraft::new(
        "agent:s6",
        "project:s6",
        ProceduralSkillOrigin::UserProvided,
        "Serial recovery",
        "serial recovery",
        "When serial framing stalls, first reset the reader, then retry one narrow probe.",
    )
    .evidence(evidence)
}
