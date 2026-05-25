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
    .subject_id("subject-default")
    .scope(MemoryScope::new("local", "chat-1")?)
    .profile(ProfileId::DesktopMacosEmbeddedSdk)
    .store_platform(store)
    .build()?;
```

`agent_id` identifies the agent instance. `owner_id` identifies the owner or tenant. `subject_id` binds the runtime to the active subject; when omitted, the SDK initializes the local subject from `owner_id`. `channel` and `chat_id` define the default memory scope for runtime operations.

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
`memory/governance_jobs/pending.json`. Run `MemoryRuntime::run_due_governance`
when services recover. The queue is isolated by memory space / subject / channel
/ chat / turn; hosts must not reimplement this queue or retry with host-owned
semantics.
Operator surfaces should use `MemoryRuntime::deferred_governance_report()` or
`inspect.deferred_governance` for pending / retrying / failed / terminal counts,
recent jobs, scope, subject, turn, reason, and last error for the current runtime
scope.

`project()` returns `MemoryProjectionReport.audit` as the projection diagnostic
source of truth. It includes source planes, selected ids, section chars,
source/render budgets, scope, and private gate decisions. Hosts may display
these fields, but must not infer projection behavior by reading store internals.

For conservative compaction, call `MemoryRuntime::run_retention_compaction()`.
It only runs SDK-owned hygiene, factual evidence metadata compaction, and runtime
skill governance, and reports `host_direct_deletion_allowed=false`; quota
pressure must not let hosts delete accepted memory.

## 9. Host Turn Lifecycle

A complete SDK host turn uses one public path:

1. Open a `StorePlatform` or inject `Arc<dyn Platform>` into `MemoryRuntime`.
2. Build `MemoryIdentity` and `MemoryScope` from stable host owner, agent, channel, and conversation ids.
3. Submit `MemoryWriteRequest::Candidates` for facts, preferences, procedures, diagnostics, subject hints, and soul candidates.
4. Finalize the turn through canonical turn semantics when transcript governance is required.
5. Use `recall` and `project` to build model context; do not assemble memory planes in the host.
6. Use `inspect` for operator visibility and safe recovery context.
7. Use memory-space export, migration dry-run, apply/import, and replay for replacement or release gates.

Generic host fixtures and Beetle-derived fixtures under `fixtures/sdk-host-readiness/` follow this same path. Beetle-derived data is legacy-shaped evidence only, not a special SDK branch.

## 10. Migration Dry-Run, Import, And Replay

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

Dry-run reports must be inspected before apply. Treat `loss_risk`, schema id, record counts, state fingerprint, event fingerprint, and privacy redaction count as release evidence. A host replacing its own memory implementation should keep a generic fixture and one host-derived legacy fixture in the readiness gate.
`preview.manifest` also reports whole-space snapshot mode, plane/privacy counts,
and subject remap state. Apply is still a whole-space snapshot import. When
source and target memory spaces differ, the manifest marks
`subject_remap.required=true` and `applied=false`; do not treat that as completed
subject key rewrite.

## 11. Operator Inspect

```rust
use bm_sdk::{MemoryInspectionRequest, PressureLevel, RuntimeLifecycleModeInput};

let inspect = runtime.inspect(MemoryInspectionRequest {
    query: "migration readiness".to_string(),
    system_max_len: 4096,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
})?;

assert!(inspect.capabilities.inspection.visible);
```

Operator inspect is the supported path for selected ids, plane evidence,
capability visibility, deferred governance queue state, lifecycle diagnosis, and
safe actions. A host UI may display this report, but it must not infer write
decisions, replay state, or projection contents from private store files.

## 12. Host Forbidden Zones

Hosts must not:

- write memory plane files directly;
- decide plane routing outside `MemoryRuntime`;
- maintain a second long-term extraction, subject, soul, private garden, or procedural write policy;
- build memory projection by reading store internals;
- treat Beetle, an IDE, Ollama, or a device channel as a kernel source kind;
- swallow deferred governance jobs or retry them with host-owned semantics;
- keep compatibility fields that pollute the current SDK contract.

## 13. Check Capabilities Before Exposing UI Or Tools

```rust
let catalog = runtime.capabilities();
if catalog.adapter.http.visible {
    // It is safe for this profile/policy/privacy combination to expose HTTP.
}
```

Do not expose a protocol or operation just because the crate compiles. The capability catalog is the runtime truth.

## 14. Suggested Host Tests

Add a smoke test in the integrating project that:

1. Opens the selected store backend.
2. Builds `MemoryRuntime`.
3. Injects `Arc<dyn Platform>` into `MemoryRuntime`.
4. Writes one `MemoryWriteCandidate` and checks the governance report.
5. Finalizes one turn with maintenance unavailable and verifies a deferred job.
6. Checks `deferred_governance_report()` and `inspect.deferred_governance`.
7. Recalls or projects the candidate-backed memory from a different chat and checks `MemoryProjectionReport.audit`.
8. Calls `run_retention_compaction()` and verifies that host deletion of accepted memory is not allowed.
9. Runs migration dry-run and apply/import through the public memory-space migrator and checks `preview.manifest`.
10. Runs operator inspect and replay against the migrated store.
