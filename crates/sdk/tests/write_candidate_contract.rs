mod support;

use bm_core::memory::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPrivacyClass, MemorySemanticJudgmentSource, MemorySubjectVisibilityPolicy,
    MemoryWriteCandidate,
};
use bm_sdk::{
    MemoryProjectionRequest, MemoryRecallRequest, MemoryWriteRequest, PressureLevel,
    RuntimeLifecycleModeInput,
};

use support::{empty_store_platform, test_runtime_with_scope};

fn procedural_test_profile() -> bm_sdk::ProfileId {
    bm_sdk::ProfileId::EspStandaloneMemory
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
fn sdk_candidate_write_persists_subject_memory_for_cross_chat_projection() {
    let profile = support::host_test_profile();
    let platform = empty_store_platform(profile);
    let runtime_a = test_runtime_with_scope(platform.clone(), profile, "llm.gateway", "chat-a");

    let report = runtime_a
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-preferred-name".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Profile,
                    topic: "preferred_name".to_string(),
                },
                long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
                privacy: MemoryPrivacyClass::SharedWithSubject,
                content: MemoryCandidateContent::Text {
                    topic: "preferred_name".to_string(),
                    body: "The user prefers to be called Qingchuan.".to_string(),
                    keywords: vec!["name".to_string(), "qingchuan".to_string()],
                },
                evidence_refs: vec!["chat-a:turn-1".to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(llm_accept(MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Profile,
                    topic: "preferred_name".to_string(),
                })),
            }],
        })
        .expect("candidate write");

    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let semantic = report
        .semantic_governance
        .expect("semantic governance report");
    assert_eq!(semantic.accepted_count, 1);
    assert_eq!(semantic.rejected_count, 0);

    let runtime_b = test_runtime_with_scope(platform, profile, "llm.gateway", "chat-b");
    let projection = runtime_b
        .project(MemoryProjectionRequest {
            binding: bm_sdk::ProceduralProjectionBindingV1::Preview,
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "我叫什么？".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");

    assert!(
        projection
            .provider_payload()
            .system_memory_block()
            .contains("Qingchuan"),
        "projection should include candidate-backed subject memory: {}",
        projection.provider_payload().system_memory_block()
    );
}

#[test]
fn sdk_candidate_write_rejects_procedural_targets_before_mutation() {
    let profile = procedural_test_profile();
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "llm.gateway", "chat-a");

    let error = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-release-checklist".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: MemoryCandidateTarget::ProceduralMemory {
                    name: String::new(),
                    topic: "release_checklist".to_string(),
                },
                long_term_subject_visibility: None,
                privacy: MemoryPrivacyClass::SharedWithSubject,
                content: MemoryCandidateContent::RuntimeSkill {
                    name: "runtime_skill__release_checklist".to_string(),
                    topic: "release_checklist".to_string(),
                    title: "release checklist".to_string(),
                    summary: "Run release checks before claiming readiness.".to_string(),
                    content: "- run tests\n- verify artifacts\n- cite evidence".to_string(),
                    citations: vec!["fixture:generic-rust-host".to_string()],
                },
                evidence_refs: vec!["chat-a:turn-2".to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(llm_accept(MemoryCandidateTarget::ProceduralMemory {
                    name: String::new(),
                    topic: "release_checklist".to_string(),
                })),
            }],
        })
        .expect_err("public candidate writes cannot own procedural creation");
    let bm_sdk::Error::Other { source, stage } = error else {
        panic!("expected typed procedural authority rejection");
    };
    assert_eq!(stage, "public_memory_write_authority");
    let typed = source
        .downcast_ref::<bm_sdk::ProceduralLearningSdkError>()
        .expect("typed procedural learning error");
    assert_eq!(
        typed.key,
        bm_sdk::ProceduralLearningErrorKeyV1::TransitionRequiresGovernance
    );
    assert_eq!(
        typed.disposition,
        bm_sdk::ProceduralLearningSdkErrorDisposition::AuthorityRejected
    );
    let recall = runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release checklist evidence".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    assert!(recall.procedural_delivery_reports.is_empty());
}
