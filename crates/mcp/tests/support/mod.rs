use bm_sdk::ProfileId;

pub fn native_runtime_profile() -> ProfileId {
    #[cfg(target_os = "macos")]
    {
        ProfileId::DesktopMacosEmbeddedSdk
    }
    #[cfg(target_os = "windows")]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(target_os = "linux")]
    {
        ProfileId::DesktopLinuxEmbeddedSdk
    }
}
#[allow(dead_code)]
pub fn factual_write_body() -> serde_json::Value {
    use bm_sdk::*;
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Project,
        topic: "remote factual project".into(),
    };
    serde_json::to_value(MemoryWriteRequest::Candidates {
        candidates: vec![MemoryWriteCandidate {
            candidate_id: "remote-fact".into(),
            authority: MemoryEvidenceAuthority::UserAsserted,
            target: target.clone(),
            long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
            privacy: MemoryPrivacyClass::SharedWithSubject,
            content: MemoryCandidateContent::Text {
                topic: "remote factual project".into(),
                body: "The remote project uses the authenticated Entry scope.".into(),
                keywords: vec![],
            },
            evidence_refs: vec![],
            canonical_entities: vec![],
            semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                source: MemorySemanticJudgmentSource::RuntimeGate,
                decision: MemoryCandidateSemanticDecision::Accept,
                governed_target: Some(target),
                reason: "remote factual fixture".into(),
            }),
        }],
    })
    .unwrap()
}
