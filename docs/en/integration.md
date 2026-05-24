# Integration Guide

This guide describes how to embed Beetle Memory into a Rust project through `bm-sdk`.

## 1. Choose A Profile

Choose the profile that matches the deployment target and runtime role:

| Use case | Profile feature | `ProfileId` |
| --- | --- | --- |
| Beetle Memory macOS standalone desktop app | `profile-desktop-macos-standalone-memory` | `ProfileId::DesktopMacosStandaloneMemory` |
| Rust desktop host on macOS | `profile-desktop-macos-embedded-sdk` | `ProfileId::DesktopMacosEmbeddedSdk` |
| Rust desktop host on Windows | `profile-desktop-windows-embedded-sdk` | `ProfileId::DesktopWindowsEmbeddedSdk` |
| Linux hardware device runtime | `profile-linux-device-standalone-memory` | `ProfileId::LinuxDeviceStandaloneMemory` |
| Linux server memory gateway | `profile-server-linux-memory-gateway` | `ProfileId::ServerLinuxMemoryGateway` |
| ESP embedded SDK host | `profile-esp-embedded-sdk` | `ProfileId::EspEmbeddedSdk` |
| ESP standalone memory runtime | `profile-esp-standalone-memory` | `ProfileId::EspStandaloneMemory` |

## 2. Add Dependencies

From this repository:

```toml
[dependencies]
bm-sdk = { path = "crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

After the crates are published:

```toml
[dependencies]
bm-sdk = { version = "0.1.0", features = ["profile-desktop-macos-embedded-sdk"] }
```

Use exactly one profile feature for a build.

## 3. Open A Store

For tests and short-lived sessions:

```rust
use bm_sdk::{ProfileId, StoreBackendConfig, StorePlatform};

let profile = ProfileId::DesktopMacosEmbeddedSdk;
let store = StorePlatform::open(StoreBackendConfig::in_memory(profile)?)?;
```

For durable desktop or server storage:

```rust
let store = StorePlatform::open(StoreBackendConfig::file(
    "/var/lib/beetle-memory",
    ProfileId::ServerLinuxMemoryGateway,
)?)?;
```

For sqlite-backed storage:

```rust
let store = StorePlatform::open(StoreBackendConfig::sqlite(
    "/var/lib/beetle-memory/memory.sqlite3",
    ProfileId::ServerLinuxMemoryGateway,
)?)?;
```

ESP profiles should use `StoreBackendConfig::embedded(profile)` or `in_memory(profile)`.

## 4. Build The Runtime

```rust
use bm_sdk::{MemoryIdentity, MemoryRuntime, MemoryScope, ProfileId};

let runtime = MemoryRuntime::builder()
    .identity(MemoryIdentity::new("agent-main", "owner-default")?)
    .scope(MemoryScope::new("local", "chat-1")?)
    .profile(ProfileId::DesktopMacosEmbeddedSdk)
    .store_platform(store)
    .build()?;
```

`agent_id` identifies the agent instance. `owner_id` identifies the owner or tenant. `channel` and `chat_id` define the default memory scope for runtime operations.

## 5. Write Memory

Procedural memory is the current direct write path for reusable runtime knowledge:

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

Long-term extraction writes should be produced by the extraction pipeline and passed through `MemoryWriteRequest::LongTermExtraction`.

## 6. Recall And Project

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

Use the projected memory block as part of your model-context assembly. Keep your host prompt assembly responsible for final ordering with system, developer, user, and tool messages.

## 7. Maintain With Explicit LLM Injection

`MemoryRuntime::maintain` is available for hosts that configure an LLM client. Generic adapters reject maintain because they cannot safely invent the LLM/HTTP boundary for the application.

```rust
let capabilities = runtime.capabilities();
if capabilities.lifecycle.maintain_lightweight.visible {
    // Call runtime.maintain(...) from the host path that owns LLM injection.
}
```

## 8. Submit Memory Candidates, Not Store Mutations

Hosts should submit candidate facts or procedures and let Beetle Memory decide
which memory plane may change. This keeps SDK, HTTP, gateway, and future hosts
on the same memory-governance contract.

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

If post-turn LLM services are unavailable, `finalize_turn_and_maintain` still
commits the transcript and writes a deferred governance job under
`memory/governance_jobs/pending.json`; hosts must not reimplement this queue.

## 9. Export, Import, And Replay

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

Use `BootstrapImport` for limited bootstrap migration and `FullRestore` when restoring full continuity state.
Use memory-space export/preview/apply when replacing a host memory implementation or moving a configured SDK store.

## 10. Check Capabilities Before Exposing UI Or Tools

```rust
let catalog = runtime.capabilities();
if catalog.adapter.http.visible {
    // It is safe for this profile/policy/privacy combination to expose HTTP.
}
```

Do not expose a protocol or operation just because the crate compiles. The capability catalog is the runtime truth.

## 11. Suggested Host Tests

Add a smoke test in the integrating project that:

1. Opens the selected store backend.
2. Builds `MemoryRuntime`.
3. Injects `Arc<dyn Platform>` into `MemoryRuntime`.
4. Writes one `MemoryWriteCandidate` and checks the governance report.
5. Finalizes one turn with maintenance unavailable and verifies a deferred job.
6. Recalls or projects the candidate-backed memory from a different chat.
7. Exports and imports a snapshot if migration is part of the product.
