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

## Harness And Proposal Sandbox

- `bm-replay` provides fixture runner, cross-store replay, memory harness gate, and benchmark gate.
- `bm-evolve` provides a proposal-only sandbox. A proposal still needs the SDK write path before it changes memory state.
- ESP profiles expose compact validation. `profile-server-linux-dev-full` exposes the full replay and benchmark surface.

## Verification

```bash
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_release_surface.sh
```
