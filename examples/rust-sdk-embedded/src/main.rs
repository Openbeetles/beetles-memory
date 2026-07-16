use bm_sdk::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryIdentity, MemoryInspectionRequest, MemoryPrivacyClass, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryRuntime, MemoryScope, MemorySemanticJudgmentSource,
    MemoryStoreHandle, MemoryWriteCandidate, MemoryWriteRequest, PressureLevel, ProfileId,
    RuntimeLifecycleModeInput, StoreBackendConfig,
};
use bm_sdk::{MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment};

fn main() -> bm_sdk::Result<()> {
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    let runtime = build_runtime(MemoryStoreHandle::open(StoreBackendConfig::in_memory(
        profile,
    )?)?)?;
    run_host_turn_lifecycle(&runtime)?;
    println!("rust-sdk-embedded host lifecycle smoke passed");
    Ok(())
}

fn build_runtime(store: MemoryStoreHandle) -> bm_sdk::Result<MemoryRuntime> {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default")?)
        .scope(MemoryScope::new("local", "chat-1")?)
        .store(store)
        .build()
}

fn llm_accept(target: MemoryCandidateTarget) -> MemoryCandidateSemanticJudgment {
    MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::LlmGovernance,
        decision: MemoryCandidateSemanticDecision::Accept,
        governed_target: Some(target),
        reason: "sdk_embedded_example_llm_governance".to_string(),
    }
}

fn run_host_turn_lifecycle(runtime: &MemoryRuntime) -> bm_sdk::Result<()> {
    let write = runtime.write(MemoryWriteRequest::Candidates {
        candidates: vec![
            MemoryWriteCandidate {
                candidate_id: "turn-1:project-readiness".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Project,
                    topic: "sdk_host_readiness".to_string(),
                },
                privacy: MemoryPrivacyClass::SharedWithSubject,
                content: MemoryCandidateContent::Text {
                    topic: "sdk_host_readiness".to_string(),
                    body: "Embedded SDK hosts validate governed writes, recall, projection, and inspection through MemoryRuntime.".to_string(),
                    keywords: vec!["sdk".to_string(), "runtime".to_string()],
                },
                evidence_refs: vec!["rust-sdk-embedded:turn-1".to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(llm_accept(MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Project,
                    topic: "sdk_host_readiness".to_string(),
                })),
            },
            MemoryWriteCandidate {
                candidate_id: "turn-1:release-guard".to_string(),
                authority: MemoryEvidenceAuthority::ProgramMemoryCanonical,
                target: MemoryCandidateTarget::ProceduralMemory {
                    name: "runtime_skill__sdk_release_guard".to_string(),
                    topic: "sdk_release".to_string(),
                },
                privacy: MemoryPrivacyClass::PublicRuntime,
                content: MemoryCandidateContent::RuntimeSkill {
                    name: "runtime_skill__sdk_release_guard".to_string(),
                    topic: "sdk_release".to_string(),
                    title: "SDK release guard".to_string(),
                    summary: "Validate SDK host lifecycle before release.".to_string(),
                    content: "- run candidate governance before release\n- verify recall, projection, and operator inspection\n- cite gate output before claiming readiness".to_string(),
                    citations: vec!["rust-sdk-embedded example".to_string()],
                },
                evidence_refs: vec!["rust-sdk-embedded:release-guard".to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(llm_accept(MemoryCandidateTarget::ProceduralMemory {
                    name: "runtime_skill__sdk_release_guard".to_string(),
                    topic: "sdk_release".to_string(),
                })),
            },
        ],
    })?;
    assert!(write.accepted);

    let recall = runtime.recall(MemoryRecallRequest {
        query: "release artifacts".to_string(),
        limit: 4,
        structured_query_facets: Vec::new(),
        tool_registry_refs: Vec::new(),
    })?;
    assert!(!recall.procedural_hits.is_empty());

    let inspect = runtime.inspect(MemoryInspectionRequest {
        query: "release artifacts".to_string(),
        system_max_len: 4096,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    })?;
    assert!(inspect.capabilities.inspection.visible);

    let projection = runtime.project(MemoryProjectionRequest {
        user_query: "How should this host release?".to_string(),
        system_max_len: 4096,
        recent_messages_limit: 8,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
        structured_query_facets: Vec::new(),
        tool_registry_refs: Vec::new(),
    })?;
    assert!(projection.system_memory_block.len() <= 4096);

    Ok(())
}
