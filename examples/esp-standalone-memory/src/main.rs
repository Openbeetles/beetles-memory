use bm_sdk::{
    MemoryIdentity, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig, StorePlatform,
};

fn main() -> bm_sdk::Result<()> {
    let runtime = runtime(ProfileId::EspStandaloneMemory)?;
    smoke(&runtime)?;
    println!("esp-standalone-memory smoke passed");
    Ok(())
}

fn runtime(profile: ProfileId) -> bm_sdk::Result<MemoryRuntime> {
    let store = StorePlatform::open(StoreBackendConfig::embedded(profile)?)?;
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("esp-memory-agent", "owner-default")?)
        .scope(MemoryScope::new("device", "chat-1")?)
        .profile(profile)
        .store_platform(store)
        .build()
}

fn smoke(runtime: &MemoryRuntime) -> bm_sdk::Result<()> {
    runtime.write(MemoryWriteRequest::Procedural {
        writes: vec![RuntimeSkillWrite {
            name: "compact_release_guard".to_string(),
            topic: "release".to_string(),
            title: "Compact release guard".to_string(),
            summary: "ESP standalone memory keeps compact local recall available.".to_string(),
            content: "1. use embedded store\n2. keep sqlite disabled\n3. run compact validation before release"
                .to_string(),
            citations: vec!["esp standalone example".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        }],
        source: RuntimeSkillWriteSource::Manual,
    })?;
    let recall = runtime.recall(MemoryRecallRequest {
        query: "compact release".to_string(),
        limit: 2,
    })?;
    assert!(!recall.procedural_hits.is_empty());
    let projection = runtime.project(MemoryProjectionRequest {
        user_query: "What is the compact release rule?".to_string(),
        system_max_len: 1024,
        recent_messages_limit: 2,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    })?;
    assert!(projection.system_memory_block.len() <= 1024);
    Ok(())
}
