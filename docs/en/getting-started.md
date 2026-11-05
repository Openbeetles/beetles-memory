# Getting Started

Use this guide when embedding the 0.12.0 source version into a Rust host through the SDK. Its Store v17 and ordered `assistant_messages` contract is described in the [release notes](release-notes-0.12.0.md). For protocol deployment, start here and then read [Adapters](adapters.md).

## Dependency

From this repository:

```toml
[dependencies]
bm-sdk = { path = "crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

After `v0.12.0` has been published and verified, an external repository can use the source tag (this does not require a crates.io publication):

```toml
[dependencies]
bm-sdk = { version = "0.12.0", git = "https://github.com/Openbeetles/beetles-memory", tag = "v0.12.0", features = ["profile-desktop-macos-embedded-sdk"] }
```

Select exactly one desktop embedded feature for the host OS: `profile-desktop-macos-embedded-sdk`, `profile-desktop-windows-embedded-sdk`, or `profile-desktop-linux-embedded-sdk`. Linux desktop hosts must not use a server or dev-full profile.

The published v0.11.2 tag remains a Store v16 release. Store v17 does not read v16; use a new Store for this version until the data owner has decided how to handle existing data.

## Build A Runtime

```rust
use bm_sdk::{
    AgentSkillDirConfig, MemoryIdentity, MemoryRuntime, MemoryScope, MemoryStoreHandle,
    ProfileId, StoreBackendConfig,
};

fn build_runtime() -> bm_sdk::Result<MemoryRuntime> {
    // Use DesktopWindowsEmbeddedSdk or DesktopLinuxEmbeddedSdk on those desktop hosts.
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

The default single-agent entry only needs `owner_id + agent_id`. The SDK creates `space:<owner_id>`, the hidden `system_governor`, the primary `human_user`, and the default `agent:<agent_id>` subject automatically. Only advanced multi-subject hosts need to pass a custom subject registry, relationship graph, or mounted subject.

`add_agent_skill_dir` is optional. It mounts a standard Agent Skill directory read-only so recall and projection can use `SKILL.md` summaries without letting Beetle Memory add, edit, import, delete, or execute those skills.

## Recall And Project

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

Hosts submit typed long-term candidates or canonical turns. Runtime Skill and
Agent Tool experience are created only by Beetle Memory's governed post-turn
learning worker; there is no public manual procedural-write shortcut.

## Run Examples

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

Each standalone example owns a local ignored lockfile. Generate it once after dependency changes; all subsequent runs are locked. These example lockfiles are local build artifacts and are not release inputs.

## Next Documents

- Read [Architecture](architecture.md) before changing crate boundaries or adding a transport.
- Read [Integration Guide](integration.md) to embed the SDK into a Rust host.
- Read [Deployment Guide](deployment.md) to run through `bm-entry` and protocol adapters.
- Read [CLI Usage](cli-usage.md) for local operator commands and file/sqlite smoke tests.
