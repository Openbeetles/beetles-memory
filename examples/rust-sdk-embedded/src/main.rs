use bm_sdk::{
    apply_memory_space_migration, export_memory_space, import_memory_space,
    preview_memory_space_migration, LongTermMemoryKind, MemoryCandidateContent,
    MemoryCandidateTarget, MemoryEvidenceAuthority, MemoryIdentity, MemoryInspectionRequest,
    MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemorySemanticJudgmentSource, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpaceMigrateApplyRequest, MemorySpaceMigratePreviewRequest, MemoryWriteCandidate,
    MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput, StoreBackendConfig,
    MemoryStoreHandle,
};
use bm_sdk::{
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment,
};

fn main() -> bm_sdk::Result<()> {
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    let store = MemoryStoreHandle::open(StoreBackendConfig::in_memory(profile)?)?;
    let runtime = runtime(profile, store.clone())?;
    run_host_turn_lifecycle(&runtime, &store, profile)?;
    println!("rust-sdk-embedded host lifecycle smoke passed");
    Ok(())
}

fn runtime(profile: ProfileId, store: MemoryStoreHandle) -> bm_sdk::Result<MemoryRuntime> {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default")?)
        .scope(MemoryScope::new("local", "chat-1")?)
        .profile(profile)
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

fn run_host_turn_lifecycle(
    runtime: &MemoryRuntime,
    store: &MemoryStoreHandle,
    profile: ProfileId,
) -> bm_sdk::Result<()> {
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
                    body: "SDK hosts validate migration through public export, dry-run, apply, inspect, and replay.".to_string(),
                    keywords: vec!["sdk".to_string(), "migration".to_string()],
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
                    content: "- run candidate governance before release\n- verify recall, projection, operator inspect, migration dry-run, and replay\n- cite gate output before claiming readiness".to_string(),
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

    let exported = export_memory_space(
        store,
        MemorySpaceExportRequest {
            memory_space_id: "space-main".to_string(),
            include_private: true,
        },
    )?;
    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_memory_space_id: "space-main".to_string(),
        target_memory_space_id: "space-copy".to_string(),
        source_profile: profile,
        target_profile: profile,
        archive: exported.archive.clone(),
    });
    assert!(!preview.loss_risk);
    assert!(preview.manifest.subject_remap.required);
    assert!(!preview.manifest.subject_remap.applied);
    assert!(!preview.vault_preflight.passed);

    let target_store = MemoryStoreHandle::open(StoreBackendConfig::in_memory(profile)?)?;
    let apply_error = apply_memory_space_migration(
        &target_store,
        MemorySpaceMigrateApplyRequest {
            plan: preview.plan.clone(),
        },
    )
    .expect_err("facet index migration must require an explicit remap");
    assert_eq!(apply_error.stage(), "memory_space_migration");

    let import_error = import_memory_space(
        &target_store,
        MemorySpaceImportRequest {
            memory_space_id: "space-copy".to_string(),
            archive: exported.archive,
        },
    )
    .expect_err("direct import must not bypass facet index remap");
    assert_eq!(import_error.stage(), "memory_space_import");
    Ok(())
}
