use bm_core::{
    compute_procedural_skill_quality, inspect_procedural_skill_draft,
    procedural_skill_meta_from_draft, procedural_skill_slot_id, ProceduralEvidenceRef,
    ProceduralSkillDraft, ProceduralSkillOrigin, ProceduralSkillState, ProceduralSkillWriteReason,
};

#[test]
fn procedural_origin_is_only_user_provided_or_runtime_learned() {
    let user = procedural_skill("Use release checklist", ProceduralSkillOrigin::UserProvided);
    let runtime = procedural_skill(
        "Recover serial framing after timeout",
        ProceduralSkillOrigin::RuntimeLearned,
    )
    .evidence(vec![ProceduralEvidenceRef::new(
        "replay:s6",
        "same recovery worked twice",
    )]);

    assert_eq!(user.origin, ProceduralSkillOrigin::UserProvided);
    assert_eq!(runtime.origin, ProceduralSkillOrigin::RuntimeLearned);
    assert_eq!(
        procedural_skill_slot_id(&user),
        "procedural::agent:s6::project:s6::use_release_checklist"
    );
}

#[test]
fn procedural_inspection_rejects_raw_logs_and_plain_facts() {
    let raw = procedural_skill("Raw payload", ProceduralSkillOrigin::UserProvided)
        .procedure("[2026-05-19] level=info key=value\n{\"secret\":\"abc\"}");
    let raw_report = inspect_procedural_skill_draft(&raw);
    assert!(!raw_report.accepted);
    assert_eq!(
        raw_report.reason,
        ProceduralSkillWriteReason::RawPayloadOrLog
    );

    let fact = procedural_skill("Fact", ProceduralSkillOrigin::UserProvided)
        .procedure("The device timezone is Asia/Shanghai.");
    let fact_report = inspect_procedural_skill_draft(&fact);
    assert!(!fact_report.accepted);
    assert_eq!(
        fact_report.reason,
        ProceduralSkillWriteReason::FactualRoutedAway
    );
}

#[test]
fn runtime_learned_requires_evidence_and_user_provided_can_be_active() {
    let runtime = procedural_skill("Recover network", ProceduralSkillOrigin::RuntimeLearned);
    let runtime_report = inspect_procedural_skill_draft(&runtime);
    assert!(!runtime_report.accepted);
    assert_eq!(
        runtime_report.reason,
        ProceduralSkillWriteReason::WeakProcedure
    );

    let user = procedural_skill("Recover network", ProceduralSkillOrigin::UserProvided);
    let user_report = inspect_procedural_skill_draft(&user);
    assert!(user_report.accepted);

    let meta = procedural_skill_meta_from_draft(&user, ProceduralSkillState::Active, 42);
    assert_eq!(meta.origin, ProceduralSkillOrigin::UserProvided);
    assert_eq!(meta.state, ProceduralSkillState::Active);
    assert!(compute_procedural_skill_quality(&meta) >= 40);
}

fn procedural_skill(title: &str, origin: ProceduralSkillOrigin) -> ProceduralSkillDraft {
    ProceduralSkillDraft::new(
        "agent:s6",
        "project:s6",
        origin,
        title,
        title,
        "When this condition appears, first inspect the contract, then run the narrow test, then apply the smallest fix.",
    )
}
