use bm_sdk::{
    GovernedRuntimeSkillWriteInput, MemoryIdentity, MemoryPrivacyClass, MemoryProjectionRequest,
    MemoryRecallRequest, MemoryRuntime, MemoryScope, MemoryStoreHandle, MemoryWriteRequest,
    PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillCreationRef,
    RuntimeSkillOwningScope, RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
};

fn main() -> bm_sdk::Result<()> {
    let profile = ProfileId::EspEmbeddedSdk;
    let store = MemoryStoreHandle::open(StoreBackendConfig::embedded(profile)?)?;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("esp-host-agent", "owner-default")?)
        .scope(MemoryScope::new("device", "chat-1")?)
        .store(store)
        .build()?;

    smoke(&runtime)?;
    println!("esp-embedded-sdk smoke passed");
    Ok(())
}

fn smoke(runtime: &MemoryRuntime) -> bm_sdk::Result<()> {
    runtime.write(MemoryWriteRequest::Procedural {
        writes: vec![GovernedRuntimeSkillWriteInput {
            write: RuntimeSkillWrite {
                name: "host_sdk_guard".to_string(),
                topic: "host-sdk".to_string(),
                title: "Host SDK guard".to_string(),
                summary: "ESP embedded SDK uses the host process boundary and embedded store."
                    .to_string(),
                content: "1. keep sqlite disabled\n2. call SDK memory methods\n3. keep host logic outside memory kernel"
                    .to_string(),
                citations: vec!["esp embedded sdk example".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_700_000_000,
            },
            creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
                candidate_ref: "example:esp-embedded-sdk-guard".to_string(),
                verification_receipt_digest:
                    "sha256:3333333333333333333333333333333333333333333333333333333333333333"
                        .to_string(),
            },
            privacy_class: MemoryPrivacyClass::PublicRuntime,
        }],
        owning_scope: RuntimeSkillOwningScope::SharedProgram,
        source: RuntimeSkillWriteSource::Manual,
    })?;
    let recall = runtime.recall(MemoryRecallRequest {
        temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
        query: "host sdk".to_string(),
        limit: 2,
        structured_query_facets: Vec::new(),
        tool_registry_refs: Vec::new(),
    })?;
    assert_eq!(recall.query, "host sdk");
    let projection = runtime.project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
        user_query: "How should the ESP host call memory?".to_string(),
        system_max_len: 1024,
        recent_messages_limit: 2,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
        structured_query_facets: Vec::new(),
        tool_registry_refs: Vec::new(),
    })?;
    assert!(
        projection
            .provider_payload()
            .system_memory_block()
            .len()
            <= 1024
    );
    Ok(())
}
