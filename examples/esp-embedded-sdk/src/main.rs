use bm_sdk::{
    MemoryIdentity, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemoryStoreHandle, PressureLevel, ProceduralProjectionBindingV1, ProfileId,
    RuntimeLifecycleModeInput, StoreBackendConfig,
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
    let recall = runtime.recall(MemoryRecallRequest {
        temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
        query: "host sdk".to_string(),
        limit: 2,
        structured_query_facets: Vec::new(),
        tool_registry_refs: Vec::new(),
    })?;
    assert_eq!(recall.query, "host sdk");
    let projection = runtime.project(MemoryProjectionRequest {
        binding: ProceduralProjectionBindingV1::Preview,
        temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
        user_query: "How should the ESP host call memory?".to_string(),
        system_max_len: 1024,
        recent_messages_limit: 2,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
        structured_query_facets: Vec::new(),
        tool_registry_refs: Vec::new(),
    })?;
    assert!(projection.provider_payload().system_memory_block().len() <= 1024);
    Ok(())
}
