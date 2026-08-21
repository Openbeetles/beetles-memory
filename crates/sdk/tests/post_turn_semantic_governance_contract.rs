#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    GovernedWriteDecision, LongTermMemoryKind, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryEvidenceAuthority, MemoryPrivacyClass, MemorySemanticJudgmentSource,
    MemoryWriteCandidate, MemoryWriteDomain, SoulCandidateDisposition,
};
use bm_sdk::{
    MemoryProjectionRequest, MemorySubjectVisibilityPolicy, MemoryWriteRequest, PressureLevel,
    RuntimeLifecycleModeInput, RuntimeSkillListRequest,
};

use support::{empty_store_platform, test_runtime_with_scope};

fn llm_judgment(
    decision: MemoryCandidateSemanticDecision,
    target: MemoryCandidateTarget,
    reason: &str,
) -> MemoryCandidateSemanticJudgment {
    MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision,
        governed_target: Some(target),
        reason: reason.to_string(),
    }
}

fn text_candidate(
    id: &str,
    target: MemoryCandidateTarget,
    privacy: MemoryPrivacyClass,
    body: &str,
    semantic_judgment: Option<MemoryCandidateSemanticJudgment>,
) -> MemoryWriteCandidate {
    let governed_target = semantic_judgment
        .as_ref()
        .and_then(|judgment| judgment.governed_target.as_ref())
        .unwrap_or(&target);
    let long_term_subject_visibility = matches!(
        governed_target,
        MemoryCandidateTarget::LongTermMemory { .. }
    )
    .then_some(MemorySubjectVisibilityPolicy::AllSubjects);
    MemoryWriteCandidate {
        candidate_id: id.to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target,
        long_term_subject_visibility,
        privacy,
        content: MemoryCandidateContent::Text {
            topic: "semantic_candidate".to_string(),
            body: body.to_string(),
            keywords: vec!["semantic".to_string()],
        },
        evidence_refs: vec![format!("turn:{id}")],
        canonical_entities: Vec::new(),
        semantic_judgment,
    }
}

#[test]
fn sdk_candidate_write_mutates_only_llm_governed_plane_not_host_claimed_target() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform.clone(), profile, "llm.gateway", "chat-a");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: Some(support::runtime_skill_subject_scope()),
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-release-routine".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Profile,
                    topic: "host_claimed_profile".to_string(),
                },
                long_term_subject_visibility: None,
                privacy: MemoryPrivacyClass::SharedWithSubject,
                content: MemoryCandidateContent::RuntimeSkill {
                    name: "runtime_skill__sdk_host_readiness_check".to_string(),
                    topic: "sdk_host_readiness_check".to_string(),
                    title: "SDK host readiness check".to_string(),
                    summary: "Run readiness checks before claiming SDK host readiness.".to_string(),
                    content: "- run cargo test\n- verify artifacts\n- cite evidence".to_string(),
                    citations: vec!["turn:candidate-release-routine".to_string()],
                },
                evidence_refs: vec![
                    "turn:candidate-release-routine".to_string(),
                    "candidate-release-routine".to_string(),
                ],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(llm_judgment(
                    MemoryCandidateSemanticDecision::Accept,
                    MemoryCandidateTarget::ProceduralMemory {
                        name: "runtime_skill__sdk_host_readiness_check".to_string(),
                        topic: "sdk_host_readiness_check".to_string(),
                    },
                    "llm_routed_to_procedural_memory",
                )),
            }],
        })
        .expect("candidate write");

    let semantic = report
        .semantic_governance
        .expect("semantic governance report");
    assert_eq!(semantic.accepted_count, 1);
    let plane = semantic.plane_reports.first().expect("plane report");
    assert_eq!(plane.domain, MemoryWriteDomain::Procedural);
    assert_eq!(plane.plane, "runtime_skill");
    assert_eq!(plane.decision, GovernedWriteDecision::Accepted);

    assert_eq!(report.changed, 1);
    let records = runtime
        .list_runtime_skills(RuntimeSkillListRequest {
            owning_scope: support::runtime_skill_subject_scope(),
            query: None,
            include_disabled: false,
            include_retired: false,
            limit: 10,
        })
        .expect("typed RuntimeSkill list");
    assert!(records
        .skills
        .iter()
        .any(|record| record.title == "SDK host readiness check"));
}

#[test]
fn sdk_candidate_write_without_llm_judgment_reports_deferred_and_does_not_mutate_plane() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform.clone(), profile, "llm.gateway", "chat-a");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: None,
            candidates: vec![text_candidate(
                "candidate-host-only",
                MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Profile,
                    topic: "preferred_name".to_string(),
                },
                MemoryPrivacyClass::SharedWithSubject,
                "The user prefers to be called Qingchuan.",
                None,
            )],
        })
        .expect("candidate write");

    let semantic = report
        .semantic_governance
        .expect("semantic governance report");
    assert_eq!(semantic.accepted_count, 0);
    assert_eq!(semantic.deferred_count, 1);
    assert_eq!(report.changed, 0);

    let runtime_b = test_runtime_with_scope(platform, profile, "llm.gateway", "chat-b");
    let projection = runtime_b
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "我叫什么？".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("projection");
    assert!(!projection
        .provider_payload()
        .system_memory_block()
        .contains("Qingchuan"));
}

#[test]
fn sdk_candidate_write_reports_soul_handoff_without_long_term_or_procedural_mutation() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "llm.gateway", "chat-a");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: None,
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-soul".to_string(),
                authority: MemoryEvidenceAuthority::SoulGovernance,
                target: MemoryCandidateTarget::Soul {
                    surface: "relationship_posture".to_string(),
                },
                long_term_subject_visibility: None,
                privacy: MemoryPrivacyClass::SoulPrivate,
                content: MemoryCandidateContent::Text {
                    topic: "relationship_posture".to_string(),
                    body: "A possible long-lived relationship posture change.".to_string(),
                    keywords: vec!["relationship".to_string()],
                },
                evidence_refs: vec!["turn:candidate-soul".to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(llm_judgment(
                    MemoryCandidateSemanticDecision::HandoffToSoulGovernance,
                    MemoryCandidateTarget::Soul {
                        surface: "relationship_posture".to_string(),
                    },
                    "llm_detected_soul_candidate",
                )),
            }],
        })
        .expect("candidate write");

    let semantic = report
        .semantic_governance
        .expect("semantic governance report");
    assert_eq!(report.changed, 0);
    assert_eq!(semantic.accepted_count, 0);
    assert!(semantic.plane_reports.is_empty());
    assert_eq!(semantic.soul_candidate_handoffs.len(), 1);
    assert_eq!(
        semantic.soul_candidate_handoffs[0].disposition,
        SoulCandidateDisposition::HandedOff
    );
}

#[test]
fn sdk_candidate_write_keeps_private_garden_out_of_common_candidate_mutation() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "llm.gateway", "chat-a");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: None,
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-private".to_string(),
                authority: MemoryEvidenceAuthority::PrivateGardenInternal,
                target: MemoryCandidateTarget::PrivateGarden {
                    path: "journal/today.md".to_string(),
                },
                long_term_subject_visibility: None,
                privacy: MemoryPrivacyClass::PrivateGarden,
                content: MemoryCandidateContent::Text {
                    topic: "private_garden".to_string(),
                    body: "raw private garden content".to_string(),
                    keywords: Vec::new(),
                },
                evidence_refs: vec!["turn:candidate-private".to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(llm_judgment(
                    MemoryCandidateSemanticDecision::Accept,
                    MemoryCandidateTarget::LongTermMemory {
                        kind: LongTermMemoryKind::Profile,
                        topic: "should_not_leak".to_string(),
                    },
                    "llm_must_not_override_private_garden_boundary",
                )),
            }],
        })
        .expect("candidate write");

    let semantic = report
        .semantic_governance
        .expect("semantic governance report");
    assert_eq!(report.changed, 0);
    assert_eq!(semantic.accepted_count, 0);
    assert_eq!(semantic.deferred_count, 1);
    assert_eq!(
        semantic.plane_reports[0].reason,
        "private_garden_uses_private_garden_governance_not_candidate_write"
    );
}
