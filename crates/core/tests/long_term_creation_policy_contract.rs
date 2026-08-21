use bm_core::memory::{
    govern_write_candidates, parse_long_term_memory_extraction_response,
    plan_long_term_memory_owner_mutation, plan_long_term_memory_upsert, GovernedWriteDecision,
    LongTermMemoryDraft, LongTermMemoryEntryPlan, LongTermMemoryOwnerMutation,
    LongTermMemoryProvenance, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPrivacyClass, MemorySemanticJudgmentSource, MemorySubjectVisibilityPolicy,
    MemoryWriteCandidate, LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
};

const NOW: u64 = 1_900_000_000;

fn draft_from_public_json(policy: serde_json::Value) -> LongTermMemoryDraft {
    serde_json::from_value(serde_json::json!({
        "kind": "fact",
        "topic": "shared_location",
        "content": "The deployment region is cn-west-1.",
        "keywords": ["deployment", "region"],
        "privacy": "shared_with_subject",
        "source_chat_id": "chat-a",
        "source_type": "conversation",
        "source_scope": "user",
        "subject_visibility": policy,
        "provenance": {
            "source_authority": "user_asserted",
            "semantic_judgment_source": "runtime_gate"
        },
        "supporting_citations": ["transcript:space-a/channel-a/chat-a#turn=turn-1"],
        "canonical_entities": [],
        "evidence_count": 1,
        "observed_at": NOW
    }))
    .expect("public long-term draft")
}

fn created(plan: LongTermMemoryEntryPlan) -> bm_core::memory::LongTermMemoryEntry {
    match plan {
        LongTermMemoryEntryPlan::Created(entry) => entry,
        other => panic!("expected Created, got {other:?}"),
    }
}

#[test]
fn rev1_create_persists_exact_only_and_hidden_visibility() {
    for policy in [
        MemorySubjectVisibilityPolicy::OnlySubjects(vec!["agent-a".into()]),
        MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec!["agent-b".into()]),
    ] {
        let draft = draft_from_public_json(serde_json::to_value(&policy).unwrap());
        let entry = created(plan_long_term_memory_upsert(None, &draft, NOW));
        assert_eq!(entry.owner_revision, 1);
        assert_eq!(entry.subject_visibility, policy);
    }
}

#[test]
fn ordinary_same_slot_policy_transition_is_typed_rejection_without_revision_change() {
    let first = draft_from_public_json(
        serde_json::to_value(MemorySubjectVisibilityPolicy::OnlySubjects(vec![
            "agent-a".into()
        ]))
        .unwrap(),
    );
    let existing = created(plan_long_term_memory_upsert(None, &first, NOW));
    let second = draft_from_public_json(
        serde_json::to_value(MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec![
            "agent-b".into(),
        ]))
        .unwrap(),
    );

    let plan = plan_long_term_memory_upsert(Some(&existing), &second, NOW + 1);
    match plan {
        LongTermMemoryEntryPlan::Rejected(reason) => assert_eq!(
            format!("{reason:?}"),
            "SubjectVisibilityTransitionRequiresControl"
        ),
        other => panic!("expected typed policy rejection, got {other:?}"),
    }
    assert_eq!(existing.owner_revision, 1);
}

#[test]
fn same_content_model_inference_remains_unconfirmed_without_a_correct_lifecycle() {
    let initial = draft_from_public_json(serde_json::json!("all_subjects"));
    let existing = created(plan_long_term_memory_upsert(None, &initial, NOW));
    let mut inferred = initial;
    inferred.provenance = LongTermMemoryProvenance {
        source_authority: MemoryEvidenceAuthority::ModelInferred,
        semantic_judgment_source: Some(MemorySemanticJudgmentSource::LlmGovernance),
    };
    inferred.source_revision = Some(2);
    inferred.observed_at = Some(NOW + 1);

    let updated = match plan_long_term_memory_upsert(Some(&existing), &inferred, NOW + 1) {
        LongTermMemoryEntryPlan::Updated(entry) => entry,
        other => panic!("expected Updated, got {other:?}"),
    };

    assert_eq!(
        updated.provenance.source_authority,
        MemoryEvidenceAuthority::ModelInferred
    );
    assert_eq!(updated.last_confirmed_at, None);
}

#[test]
fn same_content_provenance_transition_clears_an_existing_confirmation() {
    let initial = draft_from_public_json(serde_json::json!("all_subjects"));
    let mut existing = created(plan_long_term_memory_upsert(None, &initial, NOW));
    existing.last_confirmed_at = Some(NOW);
    let mut inferred = initial;
    inferred.provenance = LongTermMemoryProvenance {
        source_authority: MemoryEvidenceAuthority::ModelInferred,
        semantic_judgment_source: Some(MemorySemanticJudgmentSource::LlmGovernance),
    };
    inferred.source_revision = Some(2);
    inferred.observed_at = Some(NOW + 1);

    let updated = match plan_long_term_memory_upsert(Some(&existing), &inferred, NOW + 1) {
        LongTermMemoryEntryPlan::Updated(entry) => entry,
        other => panic!("expected Updated, got {other:?}"),
    };

    assert_eq!(
        updated.provenance.source_authority,
        MemoryEvidenceAuthority::ModelInferred
    );
    assert_eq!(updated.last_confirmed_at, None);
}

#[test]
fn correct_replaces_provenance_and_clears_low_level_confirmation() {
    let mut inferred = draft_from_public_json(serde_json::json!("all_subjects"));
    inferred.provenance = LongTermMemoryProvenance {
        source_authority: MemoryEvidenceAuthority::ModelInferred,
        semantic_judgment_source: Some(MemorySemanticJudgmentSource::LlmGovernance),
    };
    let existing = created(plan_long_term_memory_upsert(None, &inferred, NOW));
    let mut correction = inferred;
    correction.content = "The user confirmed deployment region cn-west-1.".to_string();
    correction.provenance = LongTermMemoryProvenance {
        source_authority: MemoryEvidenceAuthority::UserAsserted,
        semantic_judgment_source: Some(MemorySemanticJudgmentSource::RuntimeGate),
    };
    correction.observed_at = Some(NOW + 1);

    let updated = match plan_long_term_memory_owner_mutation(
        &existing,
        &LongTermMemoryOwnerMutation::Correct(Box::new(correction)),
        NOW + 1,
    ) {
        LongTermMemoryEntryPlan::Updated(entry) => entry,
        other => panic!("expected corrected entry, got {other:?}"),
    };

    assert_eq!(
        updated.provenance.source_authority,
        MemoryEvidenceAuthority::UserAsserted
    );
    assert_eq!(updated.last_confirmed_at, None);
}

#[test]
fn correct_cannot_implicitly_change_subject_visibility() {
    let initial = draft_from_public_json(serde_json::json!("all_subjects"));
    let existing = created(plan_long_term_memory_upsert(None, &initial, NOW));
    let mut correction = initial;
    correction.subject_visibility =
        MemorySubjectVisibilityPolicy::OnlySubjects(vec!["agent-a".to_string()]);

    assert!(matches!(
        plan_long_term_memory_owner_mutation(
            &existing,
            &LongTermMemoryOwnerMutation::Correct(Box::new(correction)),
            NOW + 1,
        ),
        LongTermMemoryEntryPlan::Rejected(
            bm_core::memory::LongTermMemoryEntryRejection::SubjectVisibilityTransitionRequiresControl
        )
    ));
}

#[test]
fn candidate_carries_exact_visibility_and_provenance_without_false_confirmation() {
    let target: MemoryCandidateTarget = serde_json::from_value(serde_json::json!({
        "target": "long_term_memory",
        "kind": "task",
        "topic": "current_focus"
    }))
    .expect("candidate target");
    let candidate = MemoryWriteCandidate {
        candidate_id: "candidate-runtime-observation".into(),
        authority: MemoryEvidenceAuthority::RuntimeObservation,
        target: target.clone(),
        long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::OnlySubjects(vec![
            "agent-a".into(),
        ])),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "current_focus".into(),
            body: "The runtime observed an active task.".into(),
            keywords: vec!["active task".into()],
        },
        evidence_refs: vec!["transcript:space-a/channel-a/chat-a#turn=turn-1".into()],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::RuntimeGate,
            decision: MemoryCandidateSemanticDecision::Accept,
            governed_target: Some(target),
            reason: "runtime observation accepted".into(),
        }),
    };

    let draft = candidate
        .to_long_term_draft_for_target(
            candidate.governed_target().expect("governed target"),
            "chat-a",
            NOW,
        )
        .expect("long-term draft");
    let json = serde_json::to_value(&draft).unwrap();
    assert_eq!(
        json.get("subject_visibility"),
        Some(&serde_json::json!({"only_subjects": ["agent-a"]}))
    );
    assert_eq!(
        json.pointer("/provenance/source_authority"),
        Some(&serde_json::json!("runtime_observation"))
    );
    assert_eq!(
        json.pointer("/provenance/semantic_judgment_source"),
        Some(&serde_json::json!("runtime_gate"))
    );
    assert!(json.get("last_confirmed_at").is_none());
}

#[test]
fn model_inferred_candidate_is_accepted_but_never_marked_confirmed() {
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: bm_core::memory::LongTermMemoryKind::Fact,
        topic: "inferred_region".into(),
    };
    let candidate = MemoryWriteCandidate {
        candidate_id: "candidate-model-inferred".into(),
        authority: MemoryEvidenceAuthority::ModelInferred,
        target: target.clone(),
        long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::OnlySubjects(vec![
            "agent-a".into(),
        ])),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "inferred_region".into(),
            body: "The model inferred the likely deployment region.".into(),
            keywords: vec!["deployment".into()],
        },
        evidence_refs: vec!["transcript:space-a/channel-a/chat-a#turn=turn-1".into()],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::LlmGovernance,
            decision: MemoryCandidateSemanticDecision::Accept,
            governed_target: Some(target),
            reason: "derived conclusion".into(),
        }),
    };

    let report = govern_write_candidates(std::slice::from_ref(&candidate));
    assert_eq!(report.accepted_count, 1);
    let draft = candidate
        .to_long_term_draft("chat-a", NOW)
        .expect("model-inferred long-term draft");
    assert_eq!(
        draft.provenance.source_authority,
        MemoryEvidenceAuthority::ModelInferred
    );
    assert!(serde_json::to_value(&draft)
        .expect("draft json")
        .get("last_confirmed_at")
        .is_none());
}

#[test]
fn candidate_visibility_shape_is_fail_closed_for_missing_invalid_and_non_long_term_intent() {
    let long_term_target = MemoryCandidateTarget::LongTermMemory {
        kind: bm_core::memory::LongTermMemoryKind::Fact,
        topic: "visibility-shape".into(),
    };
    let base = MemoryWriteCandidate {
        candidate_id: "candidate-missing-policy".into(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: long_term_target.clone(),
        long_term_subject_visibility: None,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "visibility-shape".into(),
            body: "Visibility must come from the trusted candidate owner.".into(),
            keywords: Vec::new(),
        },
        evidence_refs: vec!["transcript:space-a/channel-a/chat-a#turn=turn-1".into()],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::LlmGovernance,
            decision: MemoryCandidateSemanticDecision::Accept,
            governed_target: Some(long_term_target),
            reason: "semantic acceptance cannot supply ACL".into(),
        }),
    };
    let mut duplicate = base.clone();
    duplicate.candidate_id = "candidate-duplicate-policy".into();
    duplicate.long_term_subject_visibility =
        Some(MemorySubjectVisibilityPolicy::OnlySubjects(vec![
            "agent-a".into(),
            "agent-a".into(),
        ]));
    let mut non_long_term = base.clone();
    non_long_term.candidate_id = "candidate-non-long-term-policy".into();
    non_long_term.target = MemoryCandidateTarget::OperatorDiagnostic {
        name: "diagnostic".into(),
    };
    non_long_term.semantic_judgment = Some(MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::RuntimeGate,
        decision: MemoryCandidateSemanticDecision::Accept,
        governed_target: Some(non_long_term.target.clone()),
        reason: "non-long-term target".into(),
    });
    non_long_term.long_term_subject_visibility = Some(MemorySubjectVisibilityPolicy::AllSubjects);

    let report = govern_write_candidates(&[base, duplicate, non_long_term]);
    assert_eq!(report.accepted_count, 0);
    assert_eq!(report.rejected_count, 3);
    assert!(report.plane_reports.iter().all(|item| {
        item.decision == GovernedWriteDecision::Rejected
            && item.reason == "long_term_subject_visibility_intent_invalid"
    }));
}

#[test]
fn extraction_carries_exact_visibility_and_provenance() {
    let parsed = parse_long_term_memory_extraction_response(
        r#"[{"plane":"factual","op":"upsert","kind":"preference","topic":"response_style","content":"Use concise answers.","source_authority":"user_asserted"}]"#,
        "chat-a",
        &MemorySubjectVisibilityPolicy::OnlySubjects(vec!["agent-a".into()]),
    );
    let draft = parsed.upserts.first().expect("extracted draft");
    let json = serde_json::to_value(draft).unwrap();
    assert_eq!(
        json.get("subject_visibility"),
        Some(&serde_json::json!({"only_subjects": ["agent-a"]}))
    );
    assert_eq!(
        json.pointer("/provenance/source_authority"),
        Some(&serde_json::json!("user_asserted"))
    );
    assert_eq!(
        json.pointer("/provenance/semantic_judgment_source"),
        Some(&serde_json::json!("llm_governance"))
    );
}

#[test]
fn extraction_preserves_model_inferred_provenance_without_confirmation() {
    let parsed = parse_long_term_memory_extraction_response(
        r#"[{"plane":"factual","op":"upsert","kind":"fact","topic":"inferred_region","content":"The region is likely cn-west-1.","source_authority":"model_inferred"}]"#,
        "chat-a",
        &MemorySubjectVisibilityPolicy::OnlySubjects(vec!["agent-a".into()]),
    );
    let draft = parsed.upserts.first().expect("model-inferred draft");
    assert_eq!(
        draft.provenance.source_authority,
        MemoryEvidenceAuthority::ModelInferred
    );
    assert!(serde_json::to_value(draft)
        .expect("draft json")
        .get("last_confirmed_at")
        .is_none());
}

#[test]
fn model_inferred_is_a_distinct_unconfirmed_authority() {
    let authority: MemoryEvidenceAuthority =
        serde_json::from_str("\"model_inferred\"").expect("model inferred authority");
    assert_eq!(
        serde_json::to_string(&authority).unwrap(),
        "\"model_inferred\""
    );
}

#[test]
fn immutable_long_term_material_contract_is_v5() {
    assert_eq!(LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION, 5);
}
