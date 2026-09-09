use bm_sdk::{
    default_agent_subject_id, LongTermMemoryKind, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryEvidenceAuthority, MemoryPrivacyClass, MemorySemanticJudgmentSource,
    MemorySubjectVisibilityPolicy, MemoryWriteCandidate, MemoryWriteRequest, ProfileId,
    RuntimeSkillOwningScope,
};
#[cfg(feature = "nonproduction-replay-harness")]
use bm_sdk::{GovernedRuntimeSkillWriteInput, RuntimeSkillCreationRef, RuntimeSkillWrite};

#[allow(dead_code)]
pub fn factual_memory_write_body(candidate_id: &str, body: &str) -> String {
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Fact,
        topic: "http factual transport".to_string(),
    };
    serde_json::to_string(&MemoryWriteRequest::Candidates {
        candidates: vec![MemoryWriteCandidate {
            candidate_id: candidate_id.to_string(),
            authority: MemoryEvidenceAuthority::ProgramMemoryCanonical,
            target: target.clone(),
            long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
            privacy: MemoryPrivacyClass::SharedWithSubject,
            content: MemoryCandidateContent::Text {
                topic: "http factual transport".to_string(),
                body: body.to_string(),
                keywords: vec!["http".to_string(), "factual".to_string()],
            },
            evidence_refs: vec!["synthetic:http-factual-transport".to_string()],
            canonical_entities: Vec::new(),
            semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                source: MemorySemanticJudgmentSource::RuntimeGate,
                decision: MemoryCandidateSemanticDecision::Accept,
                governed_target: Some(target),
                reason: "typed factual HTTP transport contract".to_string(),
            }),
        }],
    })
    .expect("serialize factual HTTP write")
}

pub fn native_runtime_profile() -> ProfileId {
    #[cfg(feature = "nonproduction-replay-harness")]
    {
        ProfileId::native_dev_full().expect("native dev-full profile")
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "macos"))]
    {
        ProfileId::DesktopMacosEmbeddedSdk
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "windows"))]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "linux"))]
    {
        ProfileId::DesktopLinuxEmbeddedSdk
    }
    #[cfg(all(
        not(feature = "nonproduction-replay-harness"),
        not(any(target_os = "macos", target_os = "windows", target_os = "linux"))
    ))]
    {
        compile_error!("HTTP contract tests require a supported production host target");
    }
}

#[allow(dead_code)]
#[cfg(feature = "nonproduction-replay-harness")]
pub fn governed_runtime_skill_write(write: RuntimeSkillWrite) -> GovernedRuntimeSkillWriteInput {
    GovernedRuntimeSkillWriteInput {
        write,
        creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
            candidate_ref: "test:http-console-runtime-skill".to_string(),
            verification_receipt_digest:
                "sha256:8888888888888888888888888888888888888888888888888888888888888888"
                    .to_string(),
        },
        privacy_class: MemoryPrivacyClass::SharedWithSubject,
    }
}

#[allow(dead_code)]
pub fn runtime_skill_subject_scope(agent_id: &str) -> RuntimeSkillOwningScope {
    RuntimeSkillOwningScope::Subject {
        mounted_subject_id: default_agent_subject_id(agent_id),
    }
}
