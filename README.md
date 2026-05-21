# Beetle Memory

English | [中文](README.zh-CN.md)

Beetle Memory is a Rust memory runtime for agent systems. It provides an SDK-first integration path, owned storage backends, profile-based platform trimming, replay and migration tools, and thin protocol adapters for standalone deployment.

The project is not a vector database, a generic RAG framework, a chat-history dump, a workflow runner, or a tool execution runtime. Its job is to own memory state, memory operations, lifecycle reports, profile capability visibility, and migration/replay contracts.

## What Is In This Repository

| Area | Crates |
| --- | --- |
| SDK and memory core | `bm-sdk`, `bm-core` |
| Storage | `bm-store` |
| Replay and proposal sandbox | `bm-replay`, `bm-evolve` |
| Protocol contract and entry runtime | `bm-adapter`, `bm-entry` |
| Adapters | `bm-cli`, `bm-http`, `bm-wss`, `bm-mcp`, `bm-a2a` |

The Cargo workspace is versioned as `0.1.0`. The repository includes five smoke-test examples under `examples/` and platform capability fixtures under `fixtures/platform/capabilities/`.

## Capabilities

- Build a `MemoryRuntime` from an identity, scope, profile, and store backend.
- Write policy-checked procedural memory and long-term extraction results.
- Recall memory across working, procedural, long-term, and continuity surfaces.
- Project a bounded memory block for model context assembly.
- Inspect runtime state, lifecycle reports, and operator-safe recovery actions.
- Export, import, and replay continuity snapshots.
- Run through SDK, CLI, HTTP, WebSocket, MCP, or A2A adapter shells without duplicating memory semantics.
- Compile for ESP, Linux hardware devices, macOS/Windows SDK hosts, and Linux server gateway profiles.

## Quick Start

For local development from this repository:

```toml
[dependencies]
bm-sdk = { path = "crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

After publishing, use the crate version instead of a path dependency.

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

## Documentation

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

The documentation index is [docs/README.md](docs/README.md).

## Profiles

| Profile feature | Target | Runtime role | Default store posture |
| --- | --- | --- | --- |
| `profile-esp-standalone-memory` | ESP | standalone memory runtime | embedded or in-memory |
| `profile-esp-embedded-sdk` | ESP | embedded SDK | embedded or in-memory |
| `profile-linux-device-standalone-memory` | Linux hardware device | standalone memory runtime | file or sqlite |
| `profile-desktop-macos-embedded-sdk` | macOS | embedded SDK | file, sqlite, or in-memory |
| `profile-desktop-windows-embedded-sdk` | Windows | embedded SDK | file, sqlite, or in-memory |
| `profile-server-linux-memory-gateway` | Linux server | memory gateway | sqlite or file |
| `profile-server-linux-dev-full` | Linux server | development profile | sqlite, file, or in-memory |

ESP profiles reject file and sqlite stores at configuration time. Server, desktop, and Linux-device profiles can use sqlite when the matching profile/store feature is enabled.

## Examples

```bash
cargo run --manifest-path examples/rust-sdk-embedded/Cargo.toml
cargo run --manifest-path examples/server-runtime/Cargo.toml
cargo run --manifest-path examples/linux-device/Cargo.toml
cargo run --manifest-path examples/esp-standalone-memory/Cargo.toml
cargo run --manifest-path examples/esp-embedded-sdk/Cargo.toml
cargo run --manifest-path examples/memory-gateway/Cargo.toml
```

## Verification

Common local checks:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/check_profile_matrix.sh
bash scripts/check_release_surface.sh
```

Release environments with additional target toolchains should also run:

```bash
bash scripts/check_cross_target_compile_gates.sh --strict
```

## License

Apache-2.0. See [LICENSE](LICENSE).
