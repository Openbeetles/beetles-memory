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

## Redacted Transcript Replay

Conversation Transcript replay is the current evidence-facing replay surface for the Memory Evidence System. It is separate from the existing `MemoryReplayRequest`, which remains an inspection-oriented turn-ledger surface keyed by legacy `chat_id` scope.

The transcript replay contract is keyed by `ConversationKey`:

```rust
pub struct ConversationKey {
    pub memory_space_id: String,
    pub channel_id: String,
    pub conversation_id: String,
}
```

Release-surface replay views:

| View | Intended consumer | Boundary |
| --- | --- | --- |
| `RawOwnerOnly` | Runtime-owned governance and repair paths | Internal only; not a normal host or model payload. |
| `ModelContext` | Model-facing projection | Budgeted and privacy-filtered; no backend trace, operator-only audit, or raw tool payload. |
| `HostUi` | Host display surfaces | Redacted conversation evidence; no private garden, inner-life, or soul-private raw material. |
| `OperatorAudit` | Diagnostics and compliance review | Structured reasons, refs, and audit markers by default, not full raw content. |
| `Export` | Migration and portability | Controlled by `include_private`, profile, permission, and retention policy. |

Deleting or masking raw transcript content must report downstream impact separately from accepted long-term, procedural, private, or soul-related memory planes. Redacted replay must fail closed: when a view cannot prove a field is visible, it returns a redaction marker and audit reason instead of raw content.

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

When transcript evidence is present in memory-space storage, `include_private=false` removes raw transcript documents from export by default. Migration diagnostics must preserve the split between raw transcript, redacted transcript slices, accepted memory planes, and opaque host refs. Host object payloads are not exported by Beetle Memory; only `HostOpaqueRef` metadata and relation are portable.

## Harness And Proposal Sandbox

- `bm-replay` provides fixture runner, cross-store replay, memory harness gate, and benchmark gate.
- `bm-evolve` provides a proposal-only sandbox. A proposal still needs the SDK write path before it changes memory state.
- ESP profiles expose compact validation. `profile-server-linux-dev-full` exposes the full replay and benchmark surface.
- `fixtures/sdk-host-readiness/generic-rust-host/` and `fixtures/sdk-host-readiness/beetle-derived/` are covered by `scripts/check_sdk_host_integration_readiness.sh`.

## Verification

```bash
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_conversation_transcript_substrate.sh
bash scripts/check_release_surface.sh
```
