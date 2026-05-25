# 快速开始

当 Rust 宿主通过 SDK 内嵌 Beetle Memory 时，从本文开始。协议部署场景先读本文，再读 [Adapter 合同](adapters.md)。

## 依赖

在本仓库内开发：

```toml
[dependencies]
bm-sdk = { path = "crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

外部仓库可以调整 path；crates 发布后使用版本号：

```toml
[dependencies]
bm-sdk = { version = "0.1.0", features = ["profile-desktop-macos-embedded-sdk"] }
```

## 构建 Runtime

```rust
use bm_sdk::{
    MemoryIdentity, MemoryRuntime, MemoryScope, ProfileId, StoreBackendConfig, StorePlatform,
};

fn build_runtime() -> bm_sdk::Result<MemoryRuntime> {
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    let store = StorePlatform::open(StoreBackendConfig::in_memory(profile)?)?;

    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default")?)
        .subject_id("subject-default")
        .scope(MemoryScope::new("local", "chat-1")?)
        .profile(profile)
        .store_platform(store)
        .build()
}
```

## 写入、召回、投影

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
```

## 运行示例

```bash
cargo run --manifest-path examples/rust-sdk-embedded/Cargo.toml
cargo run --manifest-path examples/server-runtime/Cargo.toml
cargo run --manifest-path examples/linux-device/Cargo.toml
cargo run --manifest-path examples/esp-standalone-memory/Cargo.toml
cargo run --manifest-path examples/esp-embedded-sdk/Cargo.toml
cargo run --manifest-path examples/memory-gateway/Cargo.toml
```

## 下一步文档

- 改 crate 边界或新增 transport 前，先读 [架构文档](architecture.md)。
- 把 SDK 内嵌到 Rust 宿主时，读 [集成文档](integration.md)。
- 通过 `bm-entry` 和协议 adapter 部署时，读 [部署文档](deployment.md)。
- 本地 operator 命令和 file/sqlite smoke test，读 [CLI 使用](cli-usage.md)。
