# SDK Quickstart

这个 quickstart 面向任意 AI 项目。宿主只负责创建 runtime、选择 profile 和 store backend，然后调用 SDK 方法；记忆写入、召回、投影、生命周期事件和持久化语义都在 Beetle Memory 内部完成。

## 1. Add Dependency

```toml
[dependencies]
bm-sdk = { path = "../agent-memory/crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

发布到 registry 后，把 `path` 换成版本号即可。

## 2. Build Runtime

```rust
use bm_sdk::{
    MemoryIdentity, MemoryRuntime, MemoryScope, ProfileId, StoreBackendConfig, StorePlatform,
};

fn build_runtime() -> bm_sdk::Result<MemoryRuntime> {
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    let store = StorePlatform::open(StoreBackendConfig::in_memory(profile)?)?;

    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default")?)
        .scope(MemoryScope::new("local", "chat-1")?)
        .profile(profile)
        .store_platform(store)
        .build()
}
```

## 3. Write / Recall / Projection Smoke

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
        content: "1. run docs and examples\n2. run platform gates\n3. run publish dry-run".to_string(),
        citations: vec!["quickstart".to_string()],
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
    user_query: "How should I release this project?".to_string(),
    system_max_len: 4096,
    recent_messages_limit: 8,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
})?;
assert!(projection.system_memory_block.len() <= 4096);
```

## 4. Choose Profile

- `profile-esp-standalone-memory`：ESP 独立部署完整记忆系统，使用 embedded store。
- `profile-esp-embedded-sdk`：ESP 现成项目内嵌 SDK，使用 embedded store，不拉入 sqlite。
- `profile-linux-device-standalone-memory`：Linux 硬件设备独立部署，适合 file / sqlite store。
- `profile-desktop-macos-embedded-sdk`：macOS 宿主内嵌 SDK。
- `profile-desktop-windows-embedded-sdk`：Windows 宿主内嵌 SDK。
- `profile-server-linux-memory-gateway`：Linux server memory gateway，可组合 adapter contracts。
- `profile-server-linux-dev-full`：Linux server 开发全量 profile，包含 replay harness。

## 5. Run Examples

```bash
cargo run --manifest-path examples/rust-sdk-embedded/Cargo.toml
cargo run --manifest-path examples/server-runtime/Cargo.toml
cargo run --manifest-path examples/linux-device/Cargo.toml
cargo run --manifest-path examples/esp-standalone-memory/Cargo.toml
cargo run --manifest-path examples/esp-embedded-sdk/Cargo.toml
cargo run --manifest-path examples/memory-gateway/Cargo.toml
```
