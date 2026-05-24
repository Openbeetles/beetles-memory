use bm_core::memory::{
    govern_write_candidates, GovernedWriteDecision, LongTermMemoryKind, MemoryCandidateContent,
    MemoryCandidateTarget, MemoryEvidenceAuthority, MemoryPrivacyClass, MemoryWriteCandidate,
    MemoryWriteDomain, SoulCandidateDisposition,
};

fn text_candidate(
    id: &str,
    authority: MemoryEvidenceAuthority,
    target: MemoryCandidateTarget,
    content: &str,
) -> MemoryWriteCandidate {
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
