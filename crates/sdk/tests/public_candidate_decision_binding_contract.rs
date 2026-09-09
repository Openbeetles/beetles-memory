#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    govern_write_candidates, validate_write_candidate_identities, GovernedWriteDecision,
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPrivacyClass, MemorySemanticJudgmentSource, MemorySubjectVisibilityPolicy,
    MemoryWriteCandidate, SoulCandidateDisposition,
};
use bm_sdk::{
    ErrorClass, LongTermMemoryQuery, MemoryLongTermControlView, MemoryLongTermListRequest,
    MemoryWriteRequest,
};

use support::{empty_store_platform, test_runtime_with_scope};

fn long_term_candidate(
    candidate_id: &str,
    topic: &str,
    body: &str,
    decision: MemoryCandidateSemanticDecision,
    evidence_refs: Vec<&str>,
) -> MemoryWriteCandidate {
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Project,
        topic: topic.to_string(),
    };
    MemoryWriteCandidate {
        candidate_id: candidate_id.to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: target.clone(),
        long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: topic.to_string(),
            body: body.to_string(),
            keywords: vec![topic.to_string()],
        },
        evidence_refs: evidence_refs.into_iter().map(str::to_string).collect(),
        canonical_entities: Vec::new(),
        semantic_judgment: Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::LlmGovernance,
            decision,
            governed_target: Some(target),
            reason: format!("candidate_decision_{decision:?}").to_lowercase(),
        }),
    }
}

fn facet_topic_count(platform: &bm_sdk::MemoryStoreHandle, topic: &str) -> usize {
    platform
        .replay_harness()
        .read_json_namespace("memory_facet_indexes")
        .expect("facet index namespace")
        .into_iter()
        .filter(|document| {
            serde_json::to_string(&document.value)
                .expect("facet index json")
                .contains(topic)
        })
        .count()
}

fn assert_core_candidate_identity_rejected(candidates: Vec<MemoryWriteCandidate>) {
    let error = validate_write_candidate_identities(&candidates)
        .expect_err("invalid candidate identities must fail closed");
    assert_eq!(error.class(), Some(bm_core::ErrorClass::InvalidInput));
    assert_eq!(error.stage(), "memory_candidate_identity");

    let report = govern_write_candidates(&candidates);
    assert!(report.attempted);
    assert!(!report.executed);
    assert_eq!(report.accepted_count, 0);
    assert!(report.accepted_candidate_ids.is_empty());
    assert_eq!(report.rejected_count, candidates.len());
    assert!(report.plane_reports.is_empty());
    assert!(report.soul_candidate_handoffs.is_empty());
    assert_eq!(
        report.skipped_reason.as_deref(),
        Some("memory_candidate_identity_invalid")
    );
}

#[test]
fn core_candidate_identity_is_canonical_nonempty_unique_and_not_report_position() {
    let valid_id = "candidate-factual-owner";
    let valid = long_term_candidate(
        valid_id,
        "factual_owner",
        "A canonical factual candidate remains independently accepted.",
        MemoryCandidateSemanticDecision::Accept,
        vec!["turn:factual-owner"],
    );
    validate_write_candidate_identities(std::slice::from_ref(&valid))
        .expect("canonical nonempty candidate identity");
    assert_eq!(
        govern_write_candidates(std::slice::from_ref(&valid)).accepted_candidate_ids,
        vec![valid_id.to_string()]
    );

    let duplicate = valid.clone();
    assert_core_candidate_identity_rejected(vec![valid.clone(), duplicate]);

    let mut empty = valid.clone();
    empty.candidate_id.clear();
    assert_core_candidate_identity_rejected(vec![empty]);

    let mut noncanonical = valid.clone();
    noncanonical.candidate_id = " candidate-factual-owner ".to_string();
    assert_core_candidate_identity_rejected(vec![noncanonical]);

    let mut soul_handoff = valid.clone();
    soul_handoff.candidate_id = "candidate-soul-handoff".to_string();
    soul_handoff.target = MemoryCandidateTarget::Soul {
        surface: "self_authored_core".to_string(),
    };
    soul_handoff.long_term_subject_visibility = None;
    soul_handoff.semantic_judgment = Some(MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision: MemoryCandidateSemanticDecision::HandoffToSoulGovernance,
        governed_target: Some(MemoryCandidateTarget::Soul {
            surface: "self_authored_core".to_string(),
        }),
        reason: "requires_soul_governance".to_string(),
    });
    soul_handoff.evidence_refs = vec![valid_id.to_string()];

    let report = govern_write_candidates(&[soul_handoff, valid]);
    assert!(report.executed);
    assert_eq!(report.accepted_candidate_ids, vec![valid_id.to_string()]);
    assert_eq!(report.plane_reports.len(), 1);
    assert_eq!(report.soul_candidate_handoffs.len(), 1);
    assert_eq!(
        report.soul_candidate_handoffs[0].disposition,
        SoulCandidateDisposition::HandedOff
    );
}

#[test]
fn accepted_candidate_cannot_promote_rejected_candidate_via_evidence_refs() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform.clone(), profile, "llm.gateway", "chat-a");
    let accepted_id = "candidate-accepted-release-guard";
    let rejected_id = "candidate-rejected-secret-route";
    let accepted_topic = "accepted_release_guard";
    let rejected_topic = "rejected_secret_route";
    let accepted_body = "The release guard requires verified evidence.";
    let rejected_body = "This rejected route must never enter governed memory.";

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![
                long_term_candidate(
                    accepted_id,
                    accepted_topic,
                    accepted_body,
                    MemoryCandidateSemanticDecision::Accept,
                    vec!["turn:accepted-release-guard", rejected_id],
                ),
                long_term_candidate(
                    rejected_id,
                    rejected_topic,
                    rejected_body,
                    MemoryCandidateSemanticDecision::Reject,
                    vec!["turn:rejected-secret-route"],
                ),
            ],
        })
        .expect("mixed candidate write");

    let semantic = report
        .semantic_governance
        .as_ref()
        .expect("semantic governance report");
    assert_eq!(semantic.accepted_count, 1);
    assert_eq!(semantic.rejected_count, 1);
    assert!(semantic.plane_reports.iter().any(|plane| {
        plane.decision == GovernedWriteDecision::Rejected
            && plane.evidence_refs.iter().any(|item| item == rejected_id)
    }));
    assert_eq!(report.changed, 1);

    let records = runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery::default(),
            cursor: None,
            limit: 10,
            view: MemoryLongTermControlView::HostUi,
        })
        .expect("list governed long-term memory");
    assert!(records.records.iter().any(|item| {
        item.record.topic == accepted_topic && item.record.content == accepted_body
    }));
    assert!(!records.records.iter().any(|item| {
        item.record.topic == rejected_topic || item.record.content.contains(rejected_body)
    }));
    assert!(facet_topic_count(&platform, accepted_topic) > 0);
    assert_eq!(facet_topic_count(&platform, rejected_topic), 0);
}

#[test]
fn duplicate_candidate_ids_fail_before_any_store_or_event_mutation() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform.clone(), profile, "llm.gateway", "chat-a");
    let before = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot before duplicate candidate ids");
    let duplicate_id = "candidate-duplicate-owner-identity";

    let error = match runtime.write(MemoryWriteRequest::Candidates {
        candidates: vec![
            long_term_candidate(
                duplicate_id,
                "duplicate_identity_first",
                "The first candidate has a distinct governed body.",
                MemoryCandidateSemanticDecision::Accept,
                vec!["turn:duplicate-first"],
            ),
            long_term_candidate(
                duplicate_id,
                "duplicate_identity_second",
                "The second candidate must not share the first identity.",
                MemoryCandidateSemanticDecision::Accept,
                vec!["turn:duplicate-second"],
            ),
        ],
    }) {
        Ok(_) => panic!("duplicate candidate ids must fail closed"),
        Err(error) => error,
    };

    assert_eq!(error.class(), Some(ErrorClass::InvalidInput));
    assert_eq!(error.stage(), "memory_candidate_identity");
    let after = platform
        .replay_harness()
        .export_store_snapshot()
        .expect("snapshot after duplicate candidate ids");
    assert_eq!(after, before);
}

#[test]
fn invalid_candidate_ids_fail_before_any_store_or_event_mutation() {
    for invalid_id in ["", " candidate-leading-space", "candidate-control\n"] {
        let profile = support::host_test_profile();
        let platform = empty_store_platform(profile);
        let runtime = test_runtime_with_scope(platform.clone(), profile, "llm.gateway", "chat-a");
        let before = platform
            .replay_harness()
            .export_store_snapshot()
            .expect("snapshot before invalid candidate id");

        let error = match runtime.write(MemoryWriteRequest::Candidates {
            candidates: vec![long_term_candidate(
                invalid_id,
                "invalid_candidate_identity",
                "An invalid identity must never reach the mutation lifecycle.",
                MemoryCandidateSemanticDecision::Accept,
                vec!["turn:invalid-candidate-identity"],
            )],
        }) {
            Ok(_) => panic!("invalid candidate id must fail closed"),
            Err(error) => error,
        };

        assert_eq!(error.class(), Some(ErrorClass::InvalidInput));
        assert_eq!(error.stage(), "memory_candidate_identity");
        let after = platform
            .replay_harness()
            .export_store_snapshot()
            .expect("snapshot after invalid candidate id");
        assert_eq!(after, before);
    }
}
