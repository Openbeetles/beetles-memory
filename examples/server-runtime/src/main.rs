use bm_sdk::{
    MemoryIdentity, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig, StorePlatform,
};

fn main() -> bm_sdk::Result<()> {
    let profile = ProfileId::ServerLinuxDevFull;
    let db_path = unique_temp_path("beetle-memory-server-runtime", "sqlite3");
    let store = StorePlatform::open(StoreBackendConfig::sqlite(db_path, profile)?)?;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("server-agent", "owner-default")?)
        .scope(MemoryScope::new("server", "chat-1")?)
        .profile(profile)
        .store_platform(store)
        .build()?;

    smoke(&runtime)?;
    println!("server-runtime smoke passed");
    Ok(())
}

fn unique_temp_path(prefix: &str, extension: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{nanos}.{extension}",
        std::process::id()
    ))
}

fn smoke(runtime: &MemoryRuntime) -> bm_sdk::Result<()> {
    let write = runtime.write(MemoryWriteRequest::Procedural {
        writes: vec![RuntimeSkillWrite {
            name: "server_release_guard".to_string(),
            topic: "release".to_string(),
            title: "Server release guard".to_string(),
            summary: "Server runtime must verify store, docs, tests, and package surface.".to_string(),
            content: "1. open the configured store\n2. run release surface gate\n3. expose only adapter contracts"
                .to_string(),
            citations: vec!["server runtime example".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        }],
        source: RuntimeSkillWriteSource::Manual,
    })?;
    assert!(write.accepted);
    assert!(!runtime
        .recall(MemoryRecallRequest {
            query: "server release".to_string(),
            limit: 4,
        })?
        .procedural_hits
        .is_empty());
    assert!(!runtime
        .project(MemoryProjectionRequest {
            user_query: "What must the server runtime check?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })?
        .system_memory_block
        .is_empty());
    Ok(())
}
