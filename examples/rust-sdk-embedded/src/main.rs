use bm_sdk::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateTarget, MemoryCapabilityPolicy,
    MemoryEvidenceAuthority, MemoryIdentity, MemoryInspectionRequest, MemoryPrivacyClass,
    MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemorySemanticJudgmentSource, MemoryStoreHandle, MemorySubjectVisibilityPolicy,
    MemoryWriteCandidate, MemoryWriteRequest, PressureLevel, ProceduralProjectionBindingV1,
    ProfileId, RuntimeLifecycleModeInput, StoreBackendConfig,
};
use bm_sdk::{MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment};

fn main() -> bm_sdk::Result<()> {
    let profile = desktop_profile();
    let runtime = build_runtime(MemoryStoreHandle::open(StoreBackendConfig::in_memory(
        profile,
    )?)?)?;
    run_host_turn_lifecycle(&runtime)?;
    println!("rust-sdk-embedded host lifecycle smoke passed");
    Ok(())
}

#[cfg(any(
    all(feature = "desktop-macos", feature = "desktop-windows"),
    all(feature = "desktop-macos", feature = "desktop-linux"),
    all(feature = "desktop-windows", feature = "desktop-linux")
))]
compile_error!("select exactly one rust-sdk-embedded desktop feature");

#[cfg(feature = "desktop-macos")]
fn desktop_profile() -> ProfileId {
    ProfileId::DesktopMacosEmbeddedSdk
}

#[cfg(feature = "desktop-windows")]
fn desktop_profile() -> ProfileId {
    ProfileId::DesktopWindowsEmbeddedSdk
}

#[cfg(feature = "desktop-linux")]
fn desktop_profile() -> ProfileId {
    ProfileId::DesktopLinuxEmbeddedSdk
}

#[cfg(not(any(
    feature = "desktop-macos",
    feature = "desktop-windows",
    feature = "desktop-linux"
)))]
compile_error!("select one of desktop-macos, desktop-windows, or desktop-linux");

fn build_runtime(store: MemoryStoreHandle) -> bm_sdk::Result<MemoryRuntime> {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default")?)
        .scope(MemoryScope::new("local", "chat-1")?)
        .store(store)
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .build()
}

fn runtime_accept(target: MemoryCandidateTarget) -> MemoryCandidateSemanticJudgment {
    MemoryCandidateSemanticJudgment {
        source: MemorySemanticJudgmentSource::RuntimeGate,
        decision: MemoryCandidateSemanticDecision::Accept,
        governed_target: Some(target),
        reason: "sdk_embedded_example_runtime_gate".to_string(),
    }
}

fn run_host_turn_lifecycle(runtime: &MemoryRuntime) -> bm_sdk::Result<()> {
    let write = runtime.write(MemoryWriteRequest::Candidates {
        candidates: vec![MemoryWriteCandidate {
            candidate_id: "turn-1:project-readiness".to_string(),
            authority: MemoryEvidenceAuthority::ProgramMemoryCanonical,
            target: MemoryCandidateTarget::LongTermMemory {
                kind: LongTermMemoryKind::Project,
                topic: "sdk_host_readiness".to_string(),
            },
            long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
            privacy: MemoryPrivacyClass::SharedWithSubject,
            content: MemoryCandidateContent::Text {
                topic: "sdk_host_readiness".to_string(),
                body: "Embedded SDK hosts validate governed writes, recall, projection, and inspection through MemoryRuntime.".to_string(),
                keywords: vec!["sdk".to_string(), "runtime".to_string()],
            },
            evidence_refs: vec!["rust-sdk-embedded:turn-1".to_string()],
            canonical_entities: Vec::new(),
            semantic_judgment: Some(runtime_accept(MemoryCandidateTarget::LongTermMemory {
                kind: LongTermMemoryKind::Project,
                topic: "sdk_host_readiness".to_string(),
            })),
        }],
    })?;
    assert!(write.accepted);
    let recall = runtime.recall(MemoryRecallRequest {
        temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
        query: "release artifacts".to_string(),
        limit: 4,
        structured_query_facets: Vec::new(),
        tool_registry_refs: Vec::new(),
    })?;
    assert_eq!(recall.query, "release artifacts");

    let inspect = runtime.inspect(MemoryInspectionRequest {
        query: "release artifacts".to_string(),
        system_max_len: 4096,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    })?;
    assert!(inspect.capabilities.inspection.visible);

    let projection = runtime.project(MemoryProjectionRequest {
        binding: ProceduralProjectionBindingV1::Preview,
        temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
        user_query: "How should this host release?".to_string(),
        system_max_len: 4096,
        recent_messages_limit: 8,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
        structured_query_facets: Vec::new(),
        tool_registry_refs: Vec::new(),
    })?;
    assert!(projection.provider_payload().system_memory_block().len() <= 4096);

    Ok(())
}
