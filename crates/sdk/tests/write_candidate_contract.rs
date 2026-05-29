mod support;

use bm_core::memory::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPrivacyClass, MemorySemanticJudgmentSource, MemoryWriteCandidate,
};
use bm_sdk::{
    MemoryProjectionRequest, MemoryRecallRequest, MemoryWriteRequest, PressureLevel,
    ProceduralMemoryPromotionInput, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillWrite,
    RuntimeSkillWriteSource,
};

use support::{empty_store_platform, test_runtime_with_scope};

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
            user_query: "我叫什么？".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
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
                semantic_judgment: Some(llm_accept(MemoryCandidateTarget::ProceduralMemory {
                    name: String::new(),
                    topic: "release_checklist".to_string(),
                })),
            }],
        })
        .expect("candidate write");

    assert!(report.accepted);
    assert_eq!(report.changed, 1);
    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "release checklist evidence".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    assert_eq!(recall.procedural_hits.len(), 1);
    assert_eq!(
        recall.procedural_hits[0].record.name,
        "runtime_skill__release_checklist"
    );
}

#[test]
fn runtime_learned_procedural_promotion_requires_repeated_evidence_before_write() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "llm.gateway", "chat-a");

    let blocked = runtime
        .write(MemoryWriteRequest::ProceduralPromotions {
            promotions: vec![ProceduralMemoryPromotionInput {
                task_id: "task-single".to_string(),
                trigger: "release checklist".to_string(),
                procedure: "Run gates before release.".to_string(),
                constraints: vec!["stay inside SDK reports".to_string()],
                failure_modes: vec!["claimed readiness without gate output".to_string()],
                counterfactual_fix: "rerun gate and cite output".to_string(),
                evidence_refs: vec!["task:single".to_string()],
                quality_score: 90,
                repeated_evidence_count: 1,
                capability_affinity: vec!["sdk".to_string()],
            }],
            source: RuntimeSkillWriteSource::TaskLearning,
        })
        .expect("blocked promotion");
    assert!(!blocked.accepted);
    assert_eq!(blocked.changed, 0);
    assert_eq!(blocked.procedural_promotions.len(), 1);
    assert!(!blocked.procedural_promotions[0].promoted);

    let mixed = runtime
        .write(MemoryWriteRequest::ProceduralPromotions {
            promotions: vec![
                ProceduralMemoryPromotionInput {
                    task_id: "task-mixed-single".to_string(),
                    trigger: "deployment warmup checklist".to_string(),
                    procedure: "Do not promote from one observation.".to_string(),
                    constraints: vec!["stay inside SDK reports".to_string()],
                    failure_modes: vec!["single observation".to_string()],
                    counterfactual_fix: "wait for repeated evidence".to_string(),
                    evidence_refs: vec!["task:mixed-single".to_string()],
                    quality_score: 90,
                    repeated_evidence_count: 1,
                    capability_affinity: vec!["sdk".to_string()],
                },
                ProceduralMemoryPromotionInput {
                    task_id: "task-mixed-repeated".to_string(),
                    trigger: "deployment warmup checklist".to_string(),
                    procedure: "Promote only the repeated evidence item.".to_string(),
                    constraints: vec!["stay inside SDK reports".to_string()],
                    failure_modes: vec!["missing repeated evidence".to_string()],
                    counterfactual_fix: "cite both observations".to_string(),
                    evidence_refs: vec![
                        "task:mixed-first".to_string(),
                        "task:mixed-second".to_string(),
                    ],
                    quality_score: 90,
                    repeated_evidence_count: 2,
                    capability_affinity: vec!["sdk".to_string()],
                },
            ],
            source: RuntimeSkillWriteSource::TaskLearning,
        })
        .expect("mixed promotion");
    assert!(!mixed.accepted);
    assert_eq!(mixed.procedural_promotions.len(), 2);
    assert_eq!(
        mixed
            .procedural_promotions
            .iter()
            .filter(|report| report.promoted)
            .count(),
        1
    );

    let accepted = runtime
        .write(MemoryWriteRequest::ProceduralPromotions {
            promotions: vec![ProceduralMemoryPromotionInput {
                task_id: "task-repeated".to_string(),
                trigger: "release checklist".to_string(),
                procedure: "Run gates before release.".to_string(),
                constraints: vec!["stay inside SDK reports".to_string()],
                failure_modes: vec!["claimed readiness without gate output".to_string()],
                counterfactual_fix: "rerun gate and cite output".to_string(),
                evidence_refs: vec!["task:first".to_string(), "task:second".to_string()],
                quality_score: 90,
                repeated_evidence_count: 2,
                capability_affinity: vec!["sdk".to_string()],
            }],
            source: RuntimeSkillWriteSource::TaskLearning,
        })
        .expect("accepted promotion");
    assert!(accepted.accepted);
    assert_eq!(accepted.changed, 1);
    assert!(accepted.procedural_promotions[0].promoted);
    assert!(accepted
        .procedural_evolution
        .as_ref()
        .expect("evolution")
        .reasons
        .iter()
        .any(|reason| reason.contains("promotion_policy_passed")));

    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "release checklist".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    assert!(recall
        .procedural_hits
        .iter()
        .any(|hit| hit.record.name == "runtime_skill__release_checklist"));
}

#[test]
fn direct_runtime_learned_procedural_write_is_rejected_without_promotion_gate() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = test_runtime_with_scope(platform, profile, "llm.gateway", "chat-a");

    let report = runtime
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "runtime_skill__unsafe_runtime_learned".to_string(),
                topic: "release".to_string(),
                title: "Unsafe runtime learned write".to_string(),
                summary: "This write bypasses repeated evidence.".to_string(),
                content: "1. run one command\n2. claim it worked".to_string(),
                citations: vec!["single-observation".to_string()],
                source_chat_id: Some("chat-a".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::TaskLearning,
        })
        .expect("write report");

    assert!(!report.accepted);
    assert_eq!(report.changed, 0);
    assert!(report
        .reason
        .contains("runtime_learned_procedural_write_requires_promotion"));
    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "unsafe runtime learned".to_string(),
            limit: 4,
            tool_registry_refs: Vec::new(),
        })
        .expect("recall");
    assert!(recall.procedural_hits.is_empty());
}
