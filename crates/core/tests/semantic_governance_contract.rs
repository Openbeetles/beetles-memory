use bm_core::memory::{
    govern_write_candidates, GovernedWriteDecision, LongTermMemoryKind, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryEvidenceAuthority, MemoryPrivacyClass, MemorySemanticJudgmentSource,
    MemoryWriteAuthority, MemoryWriteCandidate, MemoryWriteDomain, SoulCandidateDisposition,
};

fn text_candidate(
    id: &str,
    authority: MemoryEvidenceAuthority,
    target: MemoryCandidateTarget,
    privacy: MemoryPrivacyClass,
    body: &str,
    semantic_judgment: Option<MemoryCandidateSemanticJudgment>,
) -> MemoryWriteCandidate {
    MemoryWriteCandidate {
        candidate_id: id.to_string(),
        authority,
        target,
        privacy,
        content: MemoryCandidateContent::Text {
            topic: "candidate_topic".to_string(),
            body: body.to_string(),
            keywords: vec!["candidate".to_string()],
        },
        evidence_refs: vec![format!("turn:{id}")],
        semantic_judgment,
    }
}

fn llm_accept(target: MemoryCandidateTarget) -> MemoryCandidateSemanticJudgment {
    MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision: MemoryCandidateSemanticDecision::Accept,
        governed_target: Some(target),
        reason: "llm_semantic_judgment".to_string(),
    }
}

#[test]
fn llm_semantic_judgment_not_host_target_decides_plane_mutation() {
    let report = govern_write_candidates(&[text_candidate(
        "candidate-rerouted",
        MemoryEvidenceAuthority::UserAsserted,
        MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Profile,
            topic: "host_claimed_plane".to_string(),
        },
        MemoryPrivacyClass::SharedWithSubject,
        "The exchange produced a reusable release verification routine.",
        Some(llm_accept(MemoryCandidateTarget::ProceduralMemory {
            name: "runtime_skill__release_verification".to_string(),
            topic: "release_verification".to_string(),
        })),
    )]);

    assert_eq!(report.proposal_count, 1);
    assert_eq!(report.accepted_count, 1);
    assert_eq!(report.rejected_count, 0);
    assert_eq!(report.deferred_count, 0);
    let plane = report.plane_reports.first().expect("plane report");
    assert_eq!(plane.domain, MemoryWriteDomain::Procedural);
    assert_eq!(plane.plane, "runtime_skill");
    assert_eq!(plane.authority, MemoryWriteAuthority::LlmGovernedSemantic);
    assert_eq!(plane.decision, GovernedWriteDecision::Accepted);
    assert_eq!(plane.reason, "llm_semantic_judgment");
}

#[test]
fn missing_llm_semantic_judgment_defers_candidate_without_plane_mutation() {
    let report = govern_write_candidates(&[text_candidate(
        "candidate-host-only",
        MemoryEvidenceAuthority::UserAsserted,
        MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Profile,
            topic: "preferred_name".to_string(),
        },
        MemoryPrivacyClass::SharedWithSubject,
        "The user prefers to be called Qingchuan.",
        None,
    )]);

    assert_eq!(report.accepted_count, 0);
    assert_eq!(report.rejected_count, 0);
    assert_eq!(report.deferred_count, 1);
    let plane = report.plane_reports.first().expect("plane report");
    assert_eq!(plane.decision, GovernedWriteDecision::Deferred);
    assert_eq!(
        plane.reason,
        "llm_semantic_judgment_required_before_plane_mutation"
    );
}

#[test]
fn private_garden_candidate_is_deferred_to_private_governance_not_common_candidate_write() {
    let report = govern_write_candidates(&[text_candidate(
        "candidate-private-garden",
        MemoryEvidenceAuthority::PrivateGardenInternal,
        MemoryCandidateTarget::PrivateGarden {
            path: "journal/today.md".to_string(),
        },
        MemoryPrivacyClass::PrivateGarden,
        "Raw private freeform note.",
        Some(llm_accept(MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Profile,
            topic: "leaked_private_note".to_string(),
        })),
    )]);

    assert_eq!(report.accepted_count, 0);
    assert_eq!(report.deferred_count, 1);
    let plane = report.plane_reports.first().expect("plane report");
    assert_eq!(plane.plane, "private_garden");
    assert_eq!(plane.decision, GovernedWriteDecision::Deferred);
    assert_eq!(
        plane.reason,
        "private_garden_uses_private_garden_governance_not_candidate_write"
    );
}

#[test]
fn soul_candidate_hands_off_without_common_memory_plane_mutation() {
    let report = govern_write_candidates(&[text_candidate(
        "candidate-soul",
        MemoryEvidenceAuthority::SoulGovernance,
        MemoryCandidateTarget::Soul {
            surface: "relationship_posture".to_string(),
        },
        MemoryPrivacyClass::SoulPrivate,
        "The assistant should become quieter in this relationship.",
        Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::LlmGovernance,
            decision: MemoryCandidateSemanticDecision::HandoffToSoulGovernance,
            governed_target: Some(MemoryCandidateTarget::Soul {
                surface: "relationship_posture".to_string(),
            }),
            reason: "llm_detected_soul_candidate".to_string(),
        }),
    )]);

    assert_eq!(report.accepted_count, 0);
    assert_eq!(report.deferred_count, 0);
    assert!(report.plane_reports.is_empty());
    assert_eq!(report.soul_candidate_handoffs.len(), 1);
    let handoff = &report.soul_candidate_handoffs[0];
    assert_eq!(handoff.surface, "relationship_posture");
    assert_eq!(handoff.disposition, SoulCandidateDisposition::HandedOff);
    assert_eq!(handoff.reason, "llm_detected_soul_candidate");
}
