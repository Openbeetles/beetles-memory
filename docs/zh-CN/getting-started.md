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
bm-sdk = { version = "0.6.0", features = ["profile-desktop-macos-embedded-sdk"] }
```

宿主必须按操作系统只选择一个 desktop embedded feature：`profile-desktop-macos-embedded-sdk`、`profile-desktop-windows-embedded-sdk` 或 `profile-desktop-linux-embedded-sdk`。Linux 桌面宿主禁止借用 server 或 dev-full profile。

## 构建 Runtime

```rust
use bm_sdk::{
    AgentSkillDirConfig, MemoryIdentity, MemoryRuntime, MemoryScope, MemoryStoreHandle,
    ProfileId, StoreBackendConfig,
};

fn build_runtime() -> bm_sdk::Result<MemoryRuntime> {
    // Windows 或 Linux 桌面宿主分别使用对应的 EmbeddedSdk ProfileId。
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    let store = MemoryStoreHandle::open(StoreBackendConfig::in_memory(profile)?)?;

    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default")?)
        .scope(MemoryScope::new("local", "chat-1")?)
        .store(store)
        .add_agent_skill_dir(AgentSkillDirConfig::read_only(
            "./skills",
            "host-project",
        ))
        .build()
}
```

默认 single-agent 入口只需要 `owner_id + agent_id`。SDK 会自动建立 `space:<owner_id>`、隐藏的 `system_governor`、主 `human_user` 和默认 `agent:<agent_id>` 主体；只有高级多主体宿主才需要显式提供 subject registry、relationship graph 或自定义 mounted subject。

`add_agent_skill_dir` 是可选项。它把标准 Agent Skill 目录只读挂载给召回和投影使用，Beetle Memory 只读取 `SKILL.md` 摘要，不添加、不编辑、不导入、不删除、不执行这些 skill。

## 写入、召回、投影

```rust
use bm_sdk::{
    MemoryProjectionRequest, MemoryRecallRequest, MemoryRecallTemporalOperation,
    MemoryWriteRequest, PressureLevel, RuntimeLifecycleModeInput, RuntimeSkillWrite,
    RuntimeSkillWriteSource,
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
    temporal_operation: MemoryRecallTemporalOperation::Current,
    query: "release artifacts".to_string(),
    limit: 4,
    structured_query_facets: Vec::new(),
    tool_registry_refs: Vec::new(),
})?;
assert!(recall
    .procedural_delivery_reports
    .iter()
    .any(|delivery| delivery.selected));

let projection = runtime.project(MemoryProjectionRequest {
    temporal_operation: MemoryRecallTemporalOperation::Current,
    user_query: "How should this host release?".to_string(),
    system_max_len: 4096,
    recent_messages_limit: 8,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
    structured_query_facets: Vec::new(),
    tool_registry_refs: Vec::new(),
})?;
assert!(projection.system_memory_block.len() <= 4096);
```

## 运行示例

```bash
cargo generate-lockfile --manifest-path examples/rust-sdk-embedded/Cargo.toml
cargo generate-lockfile --manifest-path examples/server-runtime/Cargo.toml
cargo generate-lockfile --manifest-path examples/linux-device/Cargo.toml
cargo generate-lockfile --manifest-path examples/esp-standalone-memory/Cargo.toml
cargo generate-lockfile --manifest-path examples/esp-embedded-sdk/Cargo.toml

cargo run --manifest-path examples/rust-sdk-embedded/Cargo.toml
cargo run --manifest-path examples/server-runtime/Cargo.toml
cargo run --manifest-path examples/linux-device/Cargo.toml
cargo run --manifest-path examples/esp-standalone-memory/Cargo.toml
cargo run --manifest-path examples/esp-embedded-sdk/Cargo.toml
```

每个 standalone example 拥有本机忽略的独立 lockfile。依赖变化后只生成一次，后续执行全部使用 `--locked`；这些 example lockfile 是本机构建产物，不属于 release 输入。

## 下一步文档

- 改 crate 边界或新增 transport 前，先读 [架构文档](architecture.md)。
- 把 SDK 内嵌到 Rust 宿主时，读 [集成文档](integration.md)。
- 通过 `bm-entry` 和协议 adapter 部署时，读 [部署文档](deployment.md)。
- 本地 operator 命令和 file/sqlite smoke test，读 [CLI 使用](cli-usage.md)。
