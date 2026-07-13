use bm_core::memory::{
    canonical_evidence_ref_from_source, govern_write_candidates, CanonicalEntityKey,
    CanonicalEntityKind, CanonicalEntityRef, GovernedWriteDecision, LongTermMemoryKind,
    MemoryCandidateContent, MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment,
    MemoryCandidateTarget, MemoryEvidenceAuthority, MemoryPrivacyClass,
    MemorySemanticJudgmentSource, MemoryWriteCandidate, MemoryWriteDomain,
    SoulCandidateDisposition,
};

fn text_candidate(
    id: &str,
    authority: MemoryEvidenceAuthority,
    target: MemoryCandidateTarget,
    content: &str,
) -> MemoryWriteCandidate {
    let semantic_judgment = Some(MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision: if matches!(target, MemoryCandidateTarget::Soul { .. }) {
            MemoryCandidateSemanticDecision::HandoffToSoulGovernance
        } else {
            MemoryCandidateSemanticDecision::Accept
        },
        governed_target: Some(target.clone()),
        reason: "test_llm_judgment".to_string(),
    });
    MemoryWriteCandidate {
        candidate_id: id.to_string(),
        authority,
        target,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "preferred_name".to_string(),
            body: content.to_string(),
            keywords: vec!["name".to_string()],
        },
        evidence_refs: vec![format!("turn:{id}")],
        canonical_entities: Vec::new(),
        semantic_judgment,
    }
}

#[test]
fn candidate_authority_and_domain_route_to_expected_disposition() {
    let report = govern_write_candidates(&[
        text_candidate(
            "candidate-user-fact",
            MemoryEvidenceAuthority::UserAsserted,
            MemoryCandidateTarget::LongTermMemory {
                kind: LongTermMemoryKind::Profile,
                topic: "preferred_name".to_string(),
            },
            "The user prefers to be called Qingchuan.",
        ),
        text_candidate(
            "candidate-runtime",
            MemoryEvidenceAuthority::RuntimeObservation,
            MemoryCandidateTarget::LongTermMemory {
                kind: LongTermMemoryKind::Project,
                topic: "runtime_pressure".to_string(),
            },
            "The current runtime is under constrained memory pressure.",
        ),
        text_candidate(
            "candidate-self-claim",
            MemoryEvidenceAuthority::AssistantSelfClaim,
            MemoryCandidateTarget::LongTermMemory {
                kind: LongTermMemoryKind::Profile,
                topic: "assistant_identity".to_string(),
            },
            "The assistant says it is only a memory helper.",
        ),
        text_candidate(
            "candidate-soul",
            MemoryEvidenceAuthority::SoulGovernance,
            MemoryCandidateTarget::Soul {
                surface: "relationship_posture".to_string(),
            },
            "The assistant should face this person with a quieter posture.",
        ),
    ]);

    assert!(report.attempted);
    assert!(report.executed);
    assert_eq!(report.proposal_count, 4);
    assert_eq!(report.accepted_count, 2);
    assert_eq!(report.rejected_count, 1);
    assert_eq!(report.soul_candidate_handoffs.len(), 1);
    assert_eq!(
        report.soul_candidate_handoffs[0].disposition,
        SoulCandidateDisposition::HandedOff
    );

    let user_fact = report
        .plane_reports
        .iter()
        .find(|item| {
            item.evidence_refs
                .iter()
                .any(|r| r == "turn:candidate-user-fact")
        })
        .expect("user fact report");
    assert_eq!(user_fact.domain, MemoryWriteDomain::Subject);
    assert_eq!(user_fact.decision, GovernedWriteDecision::Accepted);

    let runtime_fact = report
        .plane_reports
        .iter()
        .find(|item| {
            item.evidence_refs
                .iter()
                .any(|r| r == "turn:candidate-runtime")
        })
        .expect("runtime fact report");
    assert_eq!(runtime_fact.domain, MemoryWriteDomain::Program);
    assert_eq!(runtime_fact.decision, GovernedWriteDecision::Accepted);

    let self_claim = report
        .plane_reports
        .iter()
        .find(|item| {
            item.evidence_refs
                .iter()
                .any(|r| r == "turn:candidate-self-claim")
        })
        .expect("self claim report");
    assert_eq!(self_claim.decision, GovernedWriteDecision::Rejected);
    assert!(self_claim.reason.contains("assistant_self_claim"));
}

#[test]
fn long_term_draft_preserves_governed_privacy_class() {
    let mut candidate = text_candidate(
        "candidate-private-contract",
        MemoryEvidenceAuthority::UserAsserted,
        MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Profile,
            topic: "private-profile".to_string(),
        },
        "A governed subject-visible profile fact.",
    );
    candidate.privacy = MemoryPrivacyClass::SharedWithSubject;

    let draft = candidate
        .to_long_term_draft("privacy-contract-chat", 1_800_000_000)
        .expect("long-term draft");

    assert_eq!(draft.privacy, MemoryPrivacyClass::SharedWithSubject);
}

#[test]
fn candidate_canonical_entities_flow_into_long_term_draft_without_text_inference() {
    let mut candidate = text_candidate(
        "candidate-entity",
        MemoryEvidenceAuthority::UserAsserted,
        MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Project,
            topic: "agent_memory".to_string(),
        },
        "Alice maintains the Agent Memory repository.",
    );
    let source_ref = candidate.evidence_refs[0].clone();
    candidate.canonical_entities = vec![CanonicalEntityRef {
        key: CanonicalEntityKey {
            kind: CanonicalEntityKind::Person,
            canonical_id: "alice".to_string(),
        },
        display_label: Some("Alice".to_string()),
        aliases: Vec::new(),
        evidence_refs: vec![
            canonical_evidence_ref_from_source(&source_ref).expect("canonical evidence")
        ],
    }];

    let draft = candidate
        .to_long_term_draft("chat-a", 1_900_000_000)
        .expect("long-term draft");

    assert_eq!(draft.canonical_entities, candidate.canonical_entities);
    assert_eq!(draft.source_revision, None);
}
