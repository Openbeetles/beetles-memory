# Getting Started

Use this guide when embedding Beetle Memory into a Rust host through the SDK. For protocol deployment, start here and then read [Adapters](adapters.md).

## Dependency

From this repository:

```toml
[dependencies]
bm-sdk = { path = "crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

From an external repository, adjust the path or use the published version once the crates are released:

```toml
[dependencies]
bm-sdk = { version = "0.1.0", features = ["profile-desktop-macos-embedded-sdk"] }
```

## Build A Runtime

```rust
use bm_sdk::{
    AgentSkillDirConfig, MemoryIdentity, MemoryRuntime, MemoryScope, ProfileId,
    StoreBackendConfig, StorePlatform,
};

fn build_runtime() -> bm_sdk::Result<MemoryRuntime> {
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    let store = StorePlatform::open(StoreBackendConfig::in_memory(profile)?)?;

    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default")?)
        .scope(MemoryScope::new("local", "chat-1")?)
        .profile(profile)
        .store_platform(store)
        .add_agent_skill_dir(AgentSkillDirConfig::read_only(
            "./skills",
            "host-project",
        ))
        .build()
}
```

The default single-agent entry only needs `owner_id + agent_id`. The SDK creates `space:<owner_id>`, the hidden `system_governor`, the primary `human_user`, and the default `agent:<agent_id>` subject automatically. Only advanced multi-subject hosts need to pass a custom subject registry, relationship graph, or mounted subject.

`add_agent_skill_dir` is optional. It mounts a standard Agent Skill directory read-only so recall and projection can use `SKILL.md` summaries without letting Beetle Memory add, edit, import, delete, or execute those skills.

## Write, Recall, And Project

```rust
use bm_sdk::{
    MemoryProjectionRequest, MemoryRecallRequest, MemoryWriteRequest, PressureLevel,
    RuntimeLifecycleModeInput, RuntimeSkillWrite, RuntimeSkillWriteSource,
};

let runtime = build_runtime()?;

let write = runtime.write(MemoryWriteRequest::Procedural {
    writes: vec![RuntimeSkillWrite {
        name: "release_guard".to_string(),
        topic: "release".to_string(),
        title: "Release guard".to_string(),
        summary: "Verify release artifacts before publishing.".to_string(),
        content: "Run examples, platform gates, and publish dry-run.".to_string(),
        citations: vec!["getting-started".to_string()],
        source_chat_id: Some("chat-1".to_string()),
        observed_at: 1_800_000_000,
    }],
    source: RuntimeSkillWriteSource::Manual,
})?;
assert!(write.accepted);

let recall = runtime.recall(MemoryRecallRequest {
    query: "release artifacts".to_string(),
    limit: 4,
    tool_registry_refs: Vec::new(),
})?;
assert!(!recall.procedural_hits.is_empty());

let projection = runtime.project(MemoryProjectionRequest {
    user_query: "How should this host release?".to_string(),
    system_max_len: 4096,
    recent_messages_limit: 8,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
    tool_registry_refs: Vec::new(),
})?;
assert!(projection.system_memory_block.len() <= 4096);
```

## Run Examples

```bash
cargo run --manifest-path examples/rust-sdk-embedded/Cargo.toml
cargo run --manifest-path examples/server-runtime/Cargo.toml
cargo run --manifest-path examples/linux-device/Cargo.toml
cargo run --manifest-path examples/esp-standalone-memory/Cargo.toml
cargo run --manifest-path examples/esp-embedded-sdk/Cargo.toml
```

## Next Documents

- Read [Architecture](architecture.md) before changing crate boundaries or adding a transport.
- Read [Integration Guide](integration.md) to embed the SDK into a Rust host.
- Read [Deployment Guide](deployment.md) to run through `bm-entry` and protocol adapters.
- Read [CLI Usage](cli-usage.md) for local operator commands and file/sqlite smoke tests.
