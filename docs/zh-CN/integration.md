# 集成文档

本文描述如何通过 `bm-sdk` 把 Beetle Memory 内嵌到 Rust 项目里。

## 1. 选择 Profile

按部署目标和运行角色选择 profile：

| 场景 | Profile feature | `ProfileId` |
| --- | --- | --- |
| Beetle Memory macOS 独立桌面 App | `profile-desktop-macos-standalone-memory` | `ProfileId::DesktopMacosStandaloneMemory` |
| macOS Rust desktop host | `profile-desktop-macos-embedded-sdk` | `ProfileId::DesktopMacosEmbeddedSdk` |
| Windows Rust desktop host | `profile-desktop-windows-embedded-sdk` | `ProfileId::DesktopWindowsEmbeddedSdk` |
| Linux 硬件设备 runtime | `profile-linux-device-standalone-memory` | `ProfileId::LinuxDeviceStandaloneMemory` |
| Linux server memory gateway | `profile-server-linux-memory-gateway` | `ProfileId::ServerLinuxMemoryGateway` |
| ESP embedded SDK host | `profile-esp-embedded-sdk` | `ProfileId::EspEmbeddedSdk` |
| ESP standalone memory runtime | `profile-esp-standalone-memory` | `ProfileId::EspStandaloneMemory` |

## 2. 添加依赖

在本仓库内开发：

```toml
[dependencies]
bm-sdk = { path = "crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

crates 发布后：

```toml
[dependencies]
bm-sdk = { version = "0.1.0", features = ["profile-desktop-macos-embedded-sdk"] }
```

每次构建只使用一个 profile feature。

## 3. 打开 Store

测试和短生命周期 session：

```rust
use bm_sdk::{ProfileId, StoreBackendConfig, StorePlatform};

let profile = ProfileId::DesktopMacosEmbeddedSdk;
let store = StorePlatform::open(StoreBackendConfig::in_memory(profile)?)?;
```

持久化 desktop 或 server storage：

```rust
let store = StorePlatform::open(StoreBackendConfig::file(
    "/var/lib/beetle-memory",
    ProfileId::ServerLinuxMemoryGateway,
)?)?;
```

SQLite storage：

```rust
let store = StorePlatform::open(StoreBackendConfig::sqlite(
    "/var/lib/beetle-memory/memory.sqlite3",
    ProfileId::ServerLinuxMemoryGateway,
)?)?;
```

ESP profile 应使用 `StoreBackendConfig::embedded(profile)` 或 `in_memory(profile)`。

## 4. 构建 Runtime

```rust
use bm_sdk::{MemoryIdentity, MemoryRuntime, MemoryScope, ProfileId};

let runtime = MemoryRuntime::builder()
    .identity(MemoryIdentity::new("agent-main", "owner-default")?)
    .scope(MemoryScope::new("local", "chat-1")?)
    .profile(ProfileId::DesktopMacosEmbeddedSdk)
    .store_platform(store)
    .build()?;
```

`agent_id` 标识 agent 实例。`owner_id` 标识 owner 或 tenant。`channel` 和 `chat_id` 定义 runtime 操作的默认 memory scope。

## 5. 写入记忆

Procedural memory 是当前可直接写入的 reusable runtime knowledge 路径：

```rust
use bm_sdk::{MemoryWriteRequest, RuntimeSkillWrite, RuntimeSkillWriteSource};

let report = runtime.write(MemoryWriteRequest::Procedural {
    writes: vec![RuntimeSkillWrite {
        name: "release_guard".to_string(),
        topic: "release".to_string(),
        title: "Release guard".to_string(),
        summary: "Verify release artifacts before publishing.".to_string(),
        content: "Run examples, platform gates, and publish dry-run.".to_string(),
        citations: vec!["integration-guide".to_string()],
        source_chat_id: Some("chat-1".to_string()),
        observed_at: 1_800_000_000,
    }],
    source: RuntimeSkillWriteSource::Manual,
})?;

assert!(report.accepted);
```

Long-term extraction 写入应由 extraction pipeline 产生，然后通过 `MemoryWriteRequest::LongTermExtraction` 进入 runtime。

## 6. 召回与投影

```rust
use bm_sdk::{
    MemoryProjectionRequest, MemoryRecallRequest, PressureLevel, RuntimeLifecycleModeInput,
};

let recall = runtime.recall(MemoryRecallRequest {
    query: "release artifacts".to_string(),
    limit: 4,
})?;

let projection = runtime.project(MemoryProjectionRequest {
    user_query: "How should this host release?".to_string(),
    system_max_len: 4096,
    recent_messages_limit: 8,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
})?;

let memory_block = projection.system_memory_block;
```

把 projected memory block 放进你的模型上下文组装流程。最终 system、developer、user、tool message 的排序仍由宿主 prompt assembly 负责。

## 7. 显式注入 LLM 后维护

`MemoryRuntime::maintain` 面向已经配置 LLM client 的宿主。通用 adapter 会拒绝 maintain，因为它不能替应用擅自决定 LLM/HTTP 边界。

```rust
let capabilities = runtime.capabilities();
if capabilities.lifecycle.maintain_lightweight.visible {
    // 在拥有 LLM injection 的宿主路径里调用 runtime.maintain(...)。
}
```

## 8. 提交记忆候选，不直接改存储面

宿主应该提交候选事实或流程，由 Beetle Memory 判断能不能写、写到哪个记忆面。
这样 SDK、HTTP、gateway、后续任意宿主都会走同一套记忆治理合同。

```rust
use bm_sdk::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateTarget,
    MemoryEvidenceAuthority, MemoryPrivacyClass, MemoryWriteCandidate,
    MemoryWriteRequest,
};

runtime.write(MemoryWriteRequest::Candidates {
    candidates: vec![MemoryWriteCandidate {
        candidate_id: "turn-1:preferred-name".to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Profile,
            topic: "preferred_name".to_string(),
        },
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "preferred_name".to_string(),
            body: "The user prefers to be called Qingchuan.".to_string(),
            keywords: vec!["name".to_string()],
        },
        evidence_refs: vec!["chat-1:turn-1".to_string()],
    }],
})?;
```

如果 post-turn LLM 服务暂时不可用，`finalize_turn_and_maintain` 仍会先提交会话，
并在 `memory/governance_jobs/pending.json` 写入待恢复治理任务；宿主不能自己重做这条队列。

## 9. Export、Import、Replay

```rust
use bm_sdk::{
    apply_memory_space_migration, export_memory_space, preview_memory_space_migration,
    ContinuitySnapshotImportMode, MemoryExportRequest, MemoryImportRequest,
    MemoryReplayRequest, MemorySpaceExportRequest, MemorySpaceMigrateApplyRequest,
    MemorySpaceMigratePreviewRequest,
};

let exported = runtime.export(MemoryExportRequest {
    chat_id: "chat-1".to_string(),
})?;

runtime.import(MemoryImportRequest {
    snapshot: exported.snapshot,
    target_chat_id: "chat-2".to_string(),
    mode: ContinuitySnapshotImportMode::FullRestore,
})?;

let replay = runtime.replay(MemoryReplayRequest {
    chat_id: "chat-2".to_string(),
    limit: 32,
})?;

let space = export_memory_space(
    &store_platform,
    MemorySpaceExportRequest {
        memory_space_id: "space-main".to_string(),
        include_private: true,
    },
)?;
let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
    source_memory_space_id: "space-main".to_string(),
    target_memory_space_id: "space-copy".to_string(),
    snapshot: space.snapshot.clone(),
});
if !preview.loss_risk {
    apply_memory_space_migration(
        &target_store_platform,
        MemorySpaceMigrateApplyRequest {
            target_memory_space_id: "space-copy".to_string(),
            snapshot: space.snapshot,
        },
    )?;
}
```

有限启动迁移使用 `BootstrapImport`，完整连续性恢复使用 `FullRestore`。
替换宿主记忆实现或迁移一份已配置 SDK store 时，使用 memory-space export/preview/apply。

## 10. 暴露 UI 或工具前检查能力

```rust
let catalog = runtime.capabilities();
if catalog.adapter.http.visible {
    // 当前 profile/policy/privacy 组合可以暴露 HTTP。
}
```

不要因为 crate 能编译就暴露某个协议或操作。Capability catalog 才是运行时真相。

## 11. 建议宿主测试

集成项目至少增加一个 smoke test：

1. 打开选定 store backend。
2. 构建 `MemoryRuntime`。
3. 把 `Arc<dyn Platform>` 注入 `MemoryRuntime`。
4. 写入一条 `MemoryWriteCandidate`，检查 governance report。
5. 在维护不可用时 finalize 一轮 turn，验证 deferred job。
6. 从另一个 chat 召回或投影 candidate 写入的记忆。
7. 如果产品包含迁移，测试 snapshot export/import。
