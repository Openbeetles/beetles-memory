mod support;

use bm_sdk::{
    default_memory_space_id, LongTermMemoryKind, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryEvidenceAuthority, MemoryIdentity, MemoryPrivacyClass, MemoryRuntime, MemoryScope,
    MemorySemanticJudgmentSource, MemoryWriteCandidate, MemoryWriteRequest, ProfileId,
};

use support::empty_store_platform;

fn accepted_fact_candidate(id: &str, body: &str) -> MemoryWriteCandidate {
    MemoryWriteCandidate {
        candidate_id: id.to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Fact,
            topic: "multi_subject_owner".to_string(),
        },
        privacy: MemoryPrivacyClass::PublicRuntime,
        content: MemoryCandidateContent::Text {
            topic: "multi_subject_owner".to_string(),
            body: body.to_string(),
            keywords: vec!["multi-subject".to_string()],
        },
        evidence_refs: vec![format!("turn:{id}")],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::LlmGovernance,
            decision: MemoryCandidateSemanticDecision::Accept,
            governed_target: Some(MemoryCandidateTarget::LongTermMemory {
                kind: LongTermMemoryKind::Fact,
                topic: "multi_subject_owner".to_string(),
            }),
            reason: "llm_confirmed_shared_fact_candidate".to_string(),
        }),
    }
}

fn accepted_procedural_candidate(id: &str, body: &str) -> MemoryWriteCandidate {
    let target = MemoryCandidateTarget::ProceduralMemory {
        name: format!("runtime_skill__{id}"),
        topic: "release_procedure".to_string(),
    };
    MemoryWriteCandidate {
        candidate_id: id.to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: target.clone(),
        privacy: MemoryPrivacyClass::PublicRuntime,
        content: MemoryCandidateContent::Text {
            topic: "release_procedure".to_string(),
            body: body.to_string(),
            keywords: vec!["release".to_string()],
        },
        evidence_refs: vec![format!("turn:{id}")],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::LlmGovernance,
            decision: MemoryCandidateSemanticDecision::Accept,
            governed_target: Some(target),
            reason: "llm_confirmed_procedural_candidate".to_string(),
        }),
    }
}

#[test]
fn subject_candidate_shared_fact_is_owned_by_memory_space_governance() {
    let profile = ProfileId::ServerLinuxDevFull;
    let platform = empty_store_platform(profile);
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-alpha", "owner-a").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "chat-a").expect("scope"))
        .profile(profile)
        .store(platform)
        .build()
        .expect("runtime");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![accepted_fact_candidate(
                "candidate-fact-1",
                "Shared factual records belong to the MemorySpace governance plane.",
            )],
        })
        .expect("write");

    let governance = report
        .shared_fact_governance
        .expect("shared fact governance report");
    assert_eq!(governance.owner_layer, "memory_space");
    assert_eq!(
        governance.memory_space_id,
        default_memory_space_id("owner-a")
    );
    assert_eq!(
        governance.origin_subject_id.as_deref(),
        Some("agent:agent-alpha")
    );
    assert_eq!(
        governance.actor_subject_id.as_deref(),
        Some("agent:agent-alpha")
    );
    assert_eq!(governance.accepted, 1);
    assert_eq!(governance.rejected, 0);
}

#[test]
fn candidate_report_rejects_semantically_accepted_but_non_durable_shared_fact() {
    let profile = ProfileId::ServerLinuxDevFull;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-alpha", "owner-a").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "chat-a").expect("scope"))
        .profile(profile)
        .store(empty_store_platform(profile))
        .build()
        .expect("runtime");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![accepted_fact_candidate(
                "candidate-structured-1",
                "# Copied block\n- first item\n- second item\n- third item\n- fourth item",
            )],
        })
        .expect("write");

    assert!(!report.accepted, "{report:#?}");
    assert_eq!(report.changed, 0);
    let governance = report
        .shared_fact_governance
        .expect("shared fact governance report");
    assert_eq!(governance.accepted, 0);
    assert_eq!(governance.rejected, 1);
}

#[test]
fn candidate_report_exposes_partial_durable_shared_fact_admission() {
    let profile = ProfileId::ServerLinuxDevFull;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-alpha", "owner-a").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "chat-a").expect("scope"))
        .profile(profile)
        .store(empty_store_platform(profile))
        .build()
        .expect("runtime");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![
                accepted_fact_candidate(
                    "candidate-fact-accepted",
                    "The release owner is the memory-space governance plane.",
                ),
                accepted_fact_candidate(
                    "candidate-fact-rejected",
                    "# Copied block\n- first item\n- second item\n- third item\n- fourth item",
                ),
            ],
        })
        .expect("write");

    assert!(!report.accepted, "{report:#?}");
    assert_eq!(report.changed, 1);
    let governance = report
        .shared_fact_governance
        .expect("shared fact governance report");
    assert_eq!(governance.submitted, 2);
    assert_eq!(governance.accepted, 1);
    assert_eq!(governance.rejected, 1);
}

#[test]
fn candidate_report_rejects_semantically_accepted_but_weak_procedural_candidate() {
    let profile = ProfileId::ServerLinuxDevFull;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-alpha", "owner-a").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "chat-a").expect("scope"))
        .profile(profile)
        .store(empty_store_platform(profile))
        .build()
        .expect("runtime");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![accepted_procedural_candidate(
                "weak-procedure",
                "This is a bare factual sentence.",
            )],
        })
        .expect("write");

    assert!(!report.accepted, "{report:#?}");
    assert_eq!(report.changed, 0);
    assert_eq!(
        report
            .procedural_evolution
            .expect("procedural governance")
            .rejected,
        vec!["runtime_skill__weak-procedure".to_string()]
    );
}

#[test]
fn candidate_report_rejects_mixed_batch_with_a_final_plane_rejection() {
    let profile = ProfileId::ServerLinuxDevFull;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-alpha", "owner-a").expect("identity"))
        .scope(MemoryScope::new("sdk.direct", "chat-a").expect("scope"))
        .profile(profile)
        .store(empty_store_platform(profile))
        .build()
        .expect("runtime");

    let report = runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![
                accepted_fact_candidate(
                    "candidate-fact-accepted",
                    "The release owner is the memory-space governance plane.",
                ),
                accepted_procedural_candidate("weak-procedure", "This is a bare factual sentence."),
            ],
        })
        .expect("write");

    assert!(!report.accepted, "{report:#?}");
    assert_eq!(report.changed, 1);
    assert_eq!(
        report
            .shared_fact_governance
            .expect("shared fact governance")
            .accepted,
        1
    );
    assert_eq!(
        report
            .procedural_evolution
            .expect("procedural governance")
            .rejected,
        vec!["runtime_skill__weak-procedure".to_string()]
    );
}
