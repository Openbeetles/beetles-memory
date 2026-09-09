#![cfg(feature = "nonproduction-replay-harness")]
mod support;
use bm_sdk::*;
use support::{empty_store_platform, test_runtime_with_scope};

fn llm_accept(target: MemoryCandidateTarget) -> MemoryCandidateSemanticJudgment {
    MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision: MemoryCandidateSemanticDecision::Accept,
        governed_target: Some(target),
        reason: "synthetic untrusted verdict".to_string(),
    }
}

fn runtime_skill_candidate() -> MemoryWriteCandidate {
    MemoryWriteCandidate {
        candidate_id: "candidate-transaction-skill".to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: MemoryCandidateTarget::ProceduralMemory {
            name: String::new(),
            topic: "transaction_skill".to_string(),
        },
        long_term_subject_visibility: None,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::RuntimeSkill {
            name: "runtime_skill__transaction_contract".to_string(),
            topic: "transaction_skill".to_string(),
            title: "transaction contract".to_string(),
            summary: "Reject an entire memory write batch when admission fails.".to_string(),
            content:
                "- preflight every mutation\n- commit the batch once\n- report transaction ids"
                    .to_string(),
            citations: vec!["fixture:transaction-contract".to_string()],
        },
        evidence_refs: vec!["chat-a:turn-2".to_string()],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(llm_accept(MemoryCandidateTarget::ProceduralMemory {
            name: String::new(),
            topic: "transaction_skill".to_string(),
        })),
    }
}

#[test]
fn public_candidate_cannot_self_assert_runtime_skill_creation_governance() {
    let platform = empty_store_platform(support::host_test_profile());
    let runtime = test_runtime_with_scope(
        platform.clone(),
        support::host_test_profile(),
        "llm.gateway",
        "chat-a",
    );
    let before = platform.export_replay_snapshot().expect("before");
    let result = runtime.write(MemoryWriteRequest::Candidates {
        candidates: vec![runtime_skill_candidate()],
    });
    let error = result.expect_err("a host-authored semantic verdict must not mint a runtime skill");
    assert_transition_rejected(error);
    assert_eq!(platform.export_replay_snapshot().expect("after"), before);
}

fn assert_transition_rejected(error: Error) {
    let Error::Other { source, .. } = error else {
        panic!("expected typed procedural authority rejection")
    };
    let typed = source
        .downcast_ref::<ProceduralLearningSdkError>()
        .expect("typed SDK error source");
    assert_eq!(
        typed.key,
        ProceduralLearningErrorKeyV1::TransitionRequiresGovernance
    );
    assert_eq!(
        typed.disposition,
        ProceduralLearningSdkErrorDisposition::AuthorityRejected
    );
}

#[test]
fn semantic_retarget_and_operation_write_cannot_bypass_the_public_authority_gate() {
    let profile = support::host_test_profile();
    let platform = support::seeded_store_platform(profile);
    let runtime = test_runtime_with_scope(platform.clone(), profile, "llm.gateway", "chat-a");
    let before = platform.export_replay_snapshot().unwrap();
    for retarget in [false, true] {
        let mut candidate = runtime_skill_candidate();
        if retarget {
            candidate.target = MemoryCandidateTarget::LongTermMemory {
                kind: LongTermMemoryKind::Project,
                topic: "factual disguise".into(),
            };
            candidate.long_term_subject_visibility =
                Some(MemorySubjectVisibilityPolicy::AllSubjects);
            candidate.content = MemoryCandidateContent::Text {
                topic: "factual disguise".into(),
                body: "The host supplies a procedural governance verdict.".into(),
                keywords: vec![],
            };
        }
        let result = runtime.write_operation(
            format!("forged-governance-{retarget}"),
            MemoryWriteRequest::Candidates {
                candidates: vec![candidate],
            },
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("operation receipt must not grant procedural creation authority"),
        };
        assert_transition_rejected(error);
        assert_eq!(platform.export_replay_snapshot().unwrap(), before);
    }
}

#[test]
fn extraction_cannot_smuggle_runtime_skill_creation_alongside_factual_writes() {
    let profile = support::host_test_profile();
    let platform = support::seeded_store_platform(profile);
    let runtime = test_runtime_with_scope(platform.clone(), profile, "llm.gateway", "chat-a");
    let before = platform.export_replay_snapshot().unwrap();
    let request = MemoryWriteRequest::LongTermExtraction {
        extraction: ParsedLongTermMemoryExtraction {
            upserts: vec![],
            deletes: vec![],
            skill_writes: vec![RuntimeSkillWrite {
                name: "untrusted-procedure".into(),
                topic: "untrusted".into(),
                title: "untrusted".into(),
                summary: "Host cannot assert a governance result".into(),
                content: "Run an ungoverned procedure".into(),
                citations: vec![],
                source_chat_id: Some("chat-a".into()),
                observed_at: 1,
            }],
        },
    };
    assert_transition_rejected(
        runtime
            .write(request)
            .expect_err("creation authority cannot be smuggled"),
    );
    assert_eq!(platform.export_replay_snapshot().unwrap(), before);
}
