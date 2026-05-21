use bm_sdk::{
    MemoryIdentity, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig, StorePlatform,
};

fn main() -> bm_sdk::Result<()> {
    let runtime = runtime(ProfileId::DesktopMacosEmbeddedSdk)?;
    write_recall_project(&runtime)?;
    println!("rust-sdk-embedded smoke passed");
    Ok(())
}

fn runtime(profile: ProfileId) -> bm_sdk::Result<MemoryRuntime> {
    let store = StorePlatform::open(StoreBackendConfig::in_memory(profile)?)?;
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default")?)
        .scope(MemoryScope::new("local", "chat-1")?)
        .profile(profile)
        .store_platform(store)
        .build()
}

fn write_recall_project(runtime: &MemoryRuntime) -> bm_sdk::Result<()> {
    let write = runtime.write(MemoryWriteRequest::Procedural {
        writes: vec![RuntimeSkillWrite {
            name: "release_guard".to_string(),
            topic: "release".to_string(),
            title: "Release guard".to_string(),
            summary: "Verify release artifacts before publishing.".to_string(),
            content: "1. run docs and examples\n2. run platform gates\n3. run publish dry-run"
                .to_string(),
            citations: vec!["rust sdk embedded example".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        }],
        source: RuntimeSkillWriteSource::Manual,
    })?;
    assert!(write.accepted);

    let recall = runtime.recall(MemoryRecallRequest {
        query: "release artifacts".to_string(),
        limit: 4,
    })?;
    assert!(!recall.procedural_hits.is_empty());

    let projection = runtime.project(MemoryProjectionRequest {
        user_query: "How should this host release?".to_string(),
        system_max_len: 4096,
        recent_messages_limit: 8,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    })?;
    assert!(projection.system_memory_block.len() <= 4096);
    Ok(())
}
