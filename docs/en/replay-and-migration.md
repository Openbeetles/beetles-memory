# Replay And Migration

Replay and migration are validation and continuity tools. They do not replace the normal write, recall, project, or maintain path.

## Snapshot Export And Import

```rust
use bm_sdk::{ContinuitySnapshotImportMode, MemoryExportRequest, MemoryImportRequest};

let exported = runtime.export(MemoryExportRequest {
    chat_id: "chat-1".to_string(),
})?;

let imported = runtime.import(MemoryImportRequest {
    snapshot: exported.snapshot,
    target_chat_id: "chat-2".to_string(),
    mode: ContinuitySnapshotImportMode::FullRestore,
})?;
```

Available import modes are:

- `ContinuitySnapshotImportMode::BootstrapImport`
- `ContinuitySnapshotImportMode::FullRestore`

Store import validates schema id, memory system kind, namespace, lineage, state fingerprint, and event fingerprint. Failed imports must surface as a report or error instead of silently truncating data.

## Replay Inspection

```rust
use bm_sdk::MemoryReplayRequest;

let replay = runtime.replay(MemoryReplayRequest {
    chat_id: "chat-1".to_string(),
    limit: 32,
})?;
```

Replay explains historical continuity state. It should be used for inspection, migration validation, and release gates.

## Memory-Space Migration Dry-Run

Use memory-space migration when replacing a host memory implementation or moving a configured SDK store:

```rust
use bm_sdk::{
    apply_memory_space_migration, export_memory_space, preview_memory_space_migration,
    MemorySpaceExportRequest, MemorySpaceMigrateApplyRequest,
    MemorySpaceMigratePreviewRequest,
};

let exported = export_memory_space(&store, MemorySpaceExportRequest {
    memory_space_id: "space-main".to_string(),
    include_private: false,
})?;
let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
    source_memory_space_id: "space-main".to_string(),
    target_memory_space_id: "space-copy".to_string(),
    snapshot: exported.snapshot.clone(),
});
assert!(!preview.loss_risk);
assert!(preview.manifest.whole_space_snapshot);

apply_memory_space_migration(&target_store, MemorySpaceMigrateApplyRequest {
    target_memory_space_id: "space-copy".to_string(),
    snapshot: exported.snapshot,
})?;
```

`include_private=false` must redact private snapshot entries. Beetle-derived replacement fixtures must use the same public migrator as generic fixtures.
`preview.manifest` is the dry-run diagnostic source of truth. It lists
plane/privacy counts, schema id, whole-space snapshot mode, conflict/loss risk,
and subject remap state. Apply does not rewrite subject keys yet; if source and
target spaces differ, the manifest reports `subject_remap.required=true` and
`applied=false`.

## Harness And Proposal Sandbox

- `bm-replay` provides fixture runner, cross-store replay, memory harness gate, and benchmark gate.
- `bm-evolve` provides a proposal-only sandbox. A proposal still needs the SDK write path before it changes memory state.
- ESP profiles expose compact validation. `profile-server-linux-dev-full` exposes the full replay and benchmark surface.
- `fixtures/sdk-host-readiness/generic-rust-host/` and `fixtures/sdk-host-readiness/beetle-derived/` are covered by `scripts/check_sdk_host_integration_readiness.sh`.

## Verification

```bash
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_release_surface.sh
```
