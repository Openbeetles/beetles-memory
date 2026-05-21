# Beetle Memory

[English](README.md) | 中文

Beetle Memory 是面向 agent 系统的 Rust 记忆运行时。它提供 SDK-first 集成入口、自有存储后端、基于 profile 的平台裁剪、回放与迁移工具，以及用于独立部署的轻量协议 adapter。

它不是向量数据库、通用 RAG 框架、聊天历史归档、workflow runner 或工具执行运行时。它负责记忆状态、记忆操作、生命周期报告、profile 能力可见性，以及迁移和回放合同。

## 仓库内容

| 领域 | Crates |
| --- | --- |
| SDK 与记忆核心 | `bm-sdk`, `bm-core` |
| 存储 | `bm-store` |
| 回放与提案沙箱 | `bm-replay`, `bm-evolve` |
| 协议合同与入口运行时 | `bm-adapter`, `bm-entry` |
| Adapters | `bm-cli`, `bm-http`, `bm-wss`, `bm-mcp`, `bm-a2a` |

当前 Cargo workspace 的发布面版本为 `0.1.0`。仓库包含 `examples/` 下的五个 smoke 示例，以及 `fixtures/platform/capabilities/` 下的 profile capability fixtures。

## 能力范围

- 通过 identity、scope、profile 和 store backend 构建 `MemoryRuntime`。
- 写入受规则约束的 procedural memory 和 long-term extraction 结果。
- 从 working、procedural、long-term、continuity 等表面召回记忆。
- 生成受长度限制的模型上下文 memory block。
- 检查运行状态、生命周期报告和 operator-safe recovery action。
- 导出、导入并回放 continuity snapshot。
- 通过 SDK、CLI、HTTP、WebSocket、MCP 或 A2A adapter shell 进入同一套记忆语义。
- 面向 ESP、Linux 硬件设备、macOS/Windows SDK 宿主和 Linux server gateway profile 编译。

## 快速开始

在本仓库内做本地开发时：

```toml
[dependencies]
bm-sdk = { path = "crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

发布到 registry 后，使用 crate 版本号替代 path dependency。

```rust
use bm_sdk::{
    MemoryIdentity, MemoryProjectionRequest, MemoryRecallRequest, MemoryRuntime, MemoryScope,
    MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendConfig, StorePlatform,
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

fn smoke(runtime: &MemoryRuntime) -> bm_sdk::Result<()> {
    runtime.write(MemoryWriteRequest::Procedural {
        writes: vec![RuntimeSkillWrite {
            name: "release_guard".to_string(),
            topic: "release".to_string(),
            title: "Release guard".to_string(),
            summary: "Verify release artifacts before publishing.".to_string(),
            content: "Run examples, platform gates, and publish dry-run.".to_string(),
            citations: vec!["quickstart".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            observed_at: 1_800_000_000,
        }],
        source: RuntimeSkillWriteSource::Manual,
    })?;

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
```

## 文档

中文文档：

- [架构文档](docs/zh-CN/architecture.md)
- [集成文档](docs/zh-CN/integration.md)
- [部署文档](docs/zh-CN/deployment.md)
- [CLI 使用](docs/zh-CN/cli-usage.md)
- [快速开始](docs/zh-CN/getting-started.md)
- [API 表面](docs/zh-CN/api.md)
- [Profile 矩阵](docs/zh-CN/profiles.md)
- [存储后端](docs/zh-CN/store-backends.md)
- [Adapter 合同](docs/zh-CN/adapters.md)
- [回放与迁移](docs/zh-CN/replay-and-migration.md)
- [运维与检查](docs/zh-CN/operator-guide.md)
- [发布清单](docs/zh-CN/release-checklist.md)

English documentation:

- [Architecture](docs/en/architecture.md)
- [Integration Guide](docs/en/integration.md)
- [Deployment Guide](docs/en/deployment.md)
- [CLI Usage](docs/en/cli-usage.md)
- [Getting Started](docs/en/getting-started.md)
- [API Surface](docs/en/api.md)
- [Profiles](docs/en/profiles.md)
- [Store Backends](docs/en/store-backends.md)
- [Adapters](docs/en/adapters.md)
- [Replay and Migration](docs/en/replay-and-migration.md)
- [Operator Guide](docs/en/operator-guide.md)
- [Release Checklist](docs/en/release-checklist.md)

文档索引见 [docs/README.md](docs/README.md)。

## Profiles

| Profile feature | 目标 | 运行角色 | 默认 store 姿态 |
| --- | --- | --- | --- |
| `profile-esp-standalone-memory` | ESP | standalone memory runtime | embedded 或 in-memory |
| `profile-esp-embedded-sdk` | ESP | embedded SDK | embedded 或 in-memory |
| `profile-linux-device-standalone-memory` | Linux 硬件设备 | standalone memory runtime | file 或 sqlite |
| `profile-desktop-macos-embedded-sdk` | macOS | embedded SDK | file、sqlite 或 in-memory |
| `profile-desktop-windows-embedded-sdk` | Windows | embedded SDK | file、sqlite 或 in-memory |
| `profile-server-linux-memory-gateway` | Linux server | memory gateway | sqlite 或 file |
| `profile-server-linux-dev-full` | Linux server | development profile | sqlite、file 或 in-memory |

ESP profile 会在配置时拒绝 file 和 sqlite store。server、desktop 和 Linux-device profile 在启用对应 profile/store feature 后可以使用 sqlite。

## 示例

```bash
cargo run --manifest-path examples/rust-sdk-embedded/Cargo.toml
cargo run --manifest-path examples/server-runtime/Cargo.toml
cargo run --manifest-path examples/linux-device/Cargo.toml
cargo run --manifest-path examples/esp-standalone-memory/Cargo.toml
cargo run --manifest-path examples/esp-embedded-sdk/Cargo.toml
cargo run --manifest-path examples/memory-gateway/Cargo.toml
```

## 验证

常用本地检查：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/check_profile_matrix.sh
bash scripts/check_release_surface.sh
```

具备额外目标工具链的 release 环境还应运行：

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```

## License

Apache-2.0. See [LICENSE](LICENSE).
