use bm_core::memory::{
    plan_long_term_memory_owner_mutation, plan_long_term_memory_upsert, LongTermMemoryDraft,
    LongTermMemoryEntryPlan, LongTermMemoryKind, LongTermMemoryOwnerMutation,
    LongTermMemoryProvenance, LongTermMemorySourceScope, LongTermMemorySourceType,
    MemoryEvidenceAuthority, MemoryPrivacyClass, MemorySubjectVisibilityPolicy,
};

const NOW: u64 = 1_900_000_000;

fn user_draft(content: &str) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind: LongTermMemoryKind::Fact,
        topic: "correction-confirmation-owner".to_string(),
        content: content.to_string(),
        keywords: vec!["correction".to_string()],
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: Some("chat:correction".to_string()),
        source_type: Some(LongTermMemorySourceType::Conversation),
        source_scope: Some(LongTermMemorySourceScope::User),
        subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
        provenance: LongTermMemoryProvenance::new(MemoryEvidenceAuthority::UserAsserted),
        confidence: None,
        freshness: None,
        stale_hint: None,
        supporting_citations: Vec::new(),
        canonical_entities: Vec::new(),
        evidence_count: None,
        observed_at: Some(NOW),
        source_revision: None,
    }
}

fn created(plan: LongTermMemoryEntryPlan) -> bm_core::memory::LongTermMemoryEntry {
    match plan {
        LongTermMemoryEntryPlan::Created(entry) => entry,
        other => panic!("expected Created, got {other:?}"),
    }
}

#[test]
fn caller_timestamp_is_not_part_of_the_draft_contract() {
    let mut encoded = serde_json::to_value(user_draft("The deployment region is cn-east-3."))
        .expect("draft json");
    encoded
        .as_object_mut()
        .expect("draft object")
        .insert("last_confirmed_at".to_string(), serde_json::json!(NOW + 1));

    let error = serde_json::from_value::<LongTermMemoryDraft>(encoded)
        .expect_err("caller-owned confirmation timestamp must fail closed");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn low_level_correct_is_a_neutral_transition_not_a_human_confirmation() {
    let original = created(plan_long_term_memory_upsert(
        None,
        &user_draft("The deployment region is cn-east-2."),
        NOW,
    ));
    let mut forged = user_draft("The deployment region is cn-east-3.");
    forged.provenance = LongTermMemoryProvenance::new(MemoryEvidenceAuthority::ModelInferred);
    forged.supporting_citations = vec!["host-log:any-string-is-not-confirmation".to_string()];

    let plan = plan_long_term_memory_owner_mutation(
        &original,
        &LongTermMemoryOwnerMutation::Correct(Box::new(forged)),
        NOW + 1,
    );

    let LongTermMemoryEntryPlan::Updated(updated) = plan else {
        panic!("explicit Correct must produce its lifecycle-owned successor: {plan:?}");
    };
    assert_eq!(updated.last_confirmed_at, None);
    assert_eq!(
        updated.provenance.source_authority,
        MemoryEvidenceAuthority::ModelInferred
    );
}

#[test]
fn arbitrary_citation_cannot_forge_creation_confirmation() {
    let mut forged = user_draft("The deployment region is cn-east-3.");
    forged.supporting_citations = vec!["host-log:any-string-is-not-confirmation".to_string()];
    let plan = plan_long_term_memory_upsert(None, &forged, NOW + 1);
    let created = created(plan);
    assert_eq!(created.last_confirmed_at, None);
}
