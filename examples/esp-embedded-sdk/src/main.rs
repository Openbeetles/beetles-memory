use bm_sdk::{
    MemoryIdentity, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig, StorePlatform,
};

fn main() -> bm_sdk::Result<()> {
    let profile = ProfileId::EspEmbeddedSdk;
    let store = StorePlatform::open(StoreBackendConfig::embedded(profile)?)?;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("esp-host-agent", "owner-default")?)
        .scope(MemoryScope::new("device", "chat-1")?)
        .profile(profile)
        .store_platform(store)
        .build()?;

    smoke(&runtime)?;
    println!("esp-embedded-sdk smoke passed");
    Ok(())
}

fn smoke(runtime: &MemoryRuntime) -> bm_sdk::Result<()> {
    runtime.write(MemoryWriteRequest::Procedural {
        writes: vec![RuntimeSkillWrite {
            name: "host_sdk_guard".to_string(),
            topic: "host-sdk".to_string(),
            title: "Host SDK guard".to_string(),
            summary: "ESP embedded SDK uses the host process boundary and embedded store.".to_string(),
            content: "1. keep sqlite disabled\n2. call SDK memory methods\n3. keep host logic outside memory kernel"
                .to_string(),
            citations: vec!["esp embedded sdk example".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        }],
        source: RuntimeSkillWriteSource::Manual,
    })?;
    let recall = runtime.recall(MemoryRecallRequest {
        query: "host sdk".to_string(),
        limit: 2,
    })?;
    assert!(!recall.procedural_hits.is_empty());
    let projection = runtime.project(MemoryProjectionRequest {
        user_query: "How should the ESP host call memory?".to_string(),
        system_max_len: 1024,
        recent_messages_limit: 2,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    })?;
    assert!(projection.system_memory_block.len() <= 1024);
    Ok(())
}
