mod support;

use bm_core::memory::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPrivacyClass, MemoryWriteCandidate,
};
use bm_sdk::{
    MemoryProjectionRequest, MemoryRecallRequest, MemoryWriteRequest, PressureLevel, ProfileId,
    RuntimeLifecycleModeInput,
};

use support::{empty_store_platform, test_runtime_with_scope};

#[test]
fn sdk_candidate_write_persists_subject_memory_for_cross_chat_projection() {
    let profile = ProfileId::ServerLinuxDevFull;
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
                privacy: MemoryPrivacyClass::SharedWithSubject,
                content: MemoryCandidateContent::Text {
                    topic: "preferred_name".to_string(),
                    body: "The user prefers to be called Qingchuan.".to_string(),
                    keywords: vec!["name".to_string(), "qingchuan".to_string()],
                },
                evidence_refs: vec!["chat-a:turn-1".to_string()],
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
            user_query: "我叫什么？".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("project");

    assert!(
        projection.system_memory_block.contains("Qingchuan"),
        "projection should include candidate-backed subject memory: {}",
        projection.system_memory_block
    );
}

#[test]
fn sdk_candidate_write_persists_procedural_memory_through_same_governance_entry() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "llm.gateway", "chat-a");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "candidate-release-checklist".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: MemoryCandidateTarget::ProceduralMemory {
                    name: String::new(),
                    topic: "release_checklist".to_string(),
                },
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
            }],
        })
        .expect("candidate write");

    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "release checklist evidence".to_string(),
            limit: 4,
        })
        .expect("recall");
    assert_eq!(recall.procedural_hits.len(), 1);
    assert_eq!(
        recall.procedural_hits[0].record.name,
        "runtime_skill__release_checklist"
    );
}
