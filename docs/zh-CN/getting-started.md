# 快速开始

Rust 宿主接入 0.14.0 macOS SDK 源码合同时，从本文开始。[0.14.0 源码说明](release-notes-0.14.0.md)记录感知与 Store v18；0.13.0 的第一人称主体投影仍是基础。`v0.14.0` Git tag 须完成验证后才能视为已发布。协议部署场景先读本文，再读 [Adapter 合同](adapters.md)。

## 依赖

在本仓库内开发：

```toml
[dependencies]
bm-sdk = { path = "crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

只有 `v0.14.0` tag 完成发布与回读后，外部仓库才可使用该源码 tag；crates.io 发布另算：

```toml
[dependencies]
bm-sdk = { version = "0.14.0", git = "https://github.com/Openbeetles/beetles-memory", tag = "v0.14.0", features = ["profile-desktop-macos-embedded-sdk"] }
```

宿主必须按操作系统只选择一个 desktop embedded feature：`profile-desktop-macos-embedded-sdk`、`profile-desktop-windows-embedded-sdk` 或 `profile-desktop-linux-embedded-sdk`。Linux 桌面宿主禁止借用 server 或 dev-full profile。

Store v18 不读取 v17，也不自动迁移。旧 Store 的处理方式由数据 owner 决定；此前应使用新 Store，archive import 不是 schema migration。

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

## 召回与投影

```rust
use bm_sdk::{
    MemoryProjectionRequest, MemoryRecallRequest, MemoryRecallTemporalOperation,
    PressureLevel, ProceduralProjectionBindingV1, RuntimeLifecycleModeInput,
};

let runtime = build_runtime()?;

let recall = runtime.recall(MemoryRecallRequest {
    temporal_operation: MemoryRecallTemporalOperation::Current,
    query: "release artifacts".to_string(),
    limit: 4,
    structured_query_facets: Vec::new(),
    tool_registry_refs: Vec::new(),
})?;
assert_eq!(recall.query, "release artifacts");

let projection = runtime.project(MemoryProjectionRequest {
    binding: ProceduralProjectionBindingV1::Preview,
    temporal_operation: MemoryRecallTemporalOperation::Current,
    user_query: "How should this host release?".to_string(),
    system_max_len: 4096,
    recent_messages_limit: 8,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
    structured_query_facets: Vec::new(),
    tool_registry_refs: Vec::new(),
})?;
assert!(projection.provider_payload().system_memory_block().len() <= 4096);
```

宿主提交 typed long-term candidate 或 canonical turn。Runtime Skill 与 Agent Tool
经验只能由 Beetle Memory 的受治理 post-turn learning worker 生成；公共 API 不再提供
手工 procedural write 旁路。

## macOS 可选视觉接入

宿主负责采集、相机权限和界面、发送前筛查、媒体转换、接收方同意及最终 Provider 请求。先登记 `PerceptionHostHandle` 与 `VisualToolRegistrySnapshot`，授权来源，再提交 `PerceptionInputEnvelope<VisualInput>`。本轮用 `project_for_perception_host` 得到主体能力及初始 `provider_payload().visual_delivery()`；中途新画面用 `append_visual_delivery`。用 `attest_visual_delivery` 回传实际请求，有 Provider 响应时可再调用 `record_visual_provider_response`。精确顺序见[集成文档](integration.md#macos-sdk-视觉感知)与[API 合同](api.md#0140-macos-sdk-视觉感知)。

Beetle 通过获授权的视觉推理桥 `infer_visual_for_memory` 自主决定记忆候选。视觉包本身不是记忆事实；若视觉回复要进入持久学习，宿主还须提交完整的 `TranscriptExternalContentManifestV1`。本文不配置真实相机或视觉 Provider。

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
