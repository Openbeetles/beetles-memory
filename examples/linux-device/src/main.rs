use bm_sdk::{
    MemoryIdentity, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig, StorePlatform,
};

fn main() -> bm_sdk::Result<()> {
    let profile = ProfileId::LinuxDeviceStandaloneMemory;
    let root = unique_temp_dir("beetle-memory-linux-device-store");
    let store = StorePlatform::open(StoreBackendConfig::file(root, profile)?)?;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("device-agent", "owner-default")?)
        .scope(MemoryScope::new("device", "chat-1")?)
        .profile(profile)
        .store_platform(store)
        .build()?;

    smoke(&runtime)?;
    println!("linux-device smoke passed");
    Ok(())
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

fn smoke(runtime: &MemoryRuntime) -> bm_sdk::Result<()> {
    runtime.write(MemoryWriteRequest::Procedural {
        writes: vec![RuntimeSkillWrite {
            name: "device_inspection_guard".to_string(),
            topic: "inspection".to_string(),
            title: "Device inspection guard".to_string(),
            summary: "Linux device deployments keep local inspection available.".to_string(),
            content: "1. inspect lifecycle report\n2. inspect operator report\n3. restart device only after reports are clean"
                .to_string(),
            citations: vec!["linux device example".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        }],
        source: RuntimeSkillWriteSource::Manual,
    })?;
    let recall = runtime.recall(MemoryRecallRequest {
        query: "device inspection".to_string(),
        limit: 4,
    })?;
    assert!(!recall.procedural_hits.is_empty());
    let projection = runtime.project(MemoryProjectionRequest {
        user_query: "What should the device check?".to_string(),
        system_max_len: 2048,
        recent_messages_limit: 4,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    })?;
    assert!(projection.system_memory_block.len() <= 2048);
    Ok(())
}
