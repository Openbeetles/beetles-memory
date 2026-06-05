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

Deleting or masking raw transcript content must report downstream impact separately from accepted long-term, shared factual, procedural, private, or soul-related memory planes. Lifecycle reports expose host refs through the SDK's operator-audit view, so internal/model-only refs and raw labels are redacted before the report leaves the runtime. Redacted replay must fail closed: when a view cannot prove a field is visible, it returns a redaction marker and audit reason instead of raw content. SDK runtime consumers use transcript-backed evidence ahead of the legacy session shadow, so a masked transcript or untrusted legacy alias is not rehydrated from `SessionStore(chat_id)`.

SDK transcript replay/export requests support bounded cursor pages through `cursor`, `next_cursor`, and `has_more`. Host ref visibility is view-aware, and host ref `label` is field-redacted outside owner-approved views with `HostRefLabel` in the redaction report.

`HostUi` transcript replay is controlled by the SDK `transcript_replay` capability. Desktop and embedded SDK hosts can commit a transcript turn and read that same conversation back for UI display without enabling `MemoryRuntime::replay`, replay harnesses, raw owner replay, or deep inspection.

`TranscriptLifecycleReport.derived_memory_refs` can be used as the target source for the next long-term memory control action. For example, after raw transcript content is masked or deleted, the report lists affected `DerivedMemoryRef` values; a host or operator that wants to revoke the corresponding accepted long-term memory should pass the ref through `MemoryLongTermTarget::TranscriptDerivedRef` and call `MemoryRuntime::mutate_long_term_memory`. Transcript lifecycle never automatically cascades deletion into accepted long-term memory, shared facts, procedural skills, private garden material, or soul handoffs.

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

When transcript evidence is present in memory-space storage, `include_private=false` removes raw transcript documents and `conversation_transcript_derived_ref` manifests from export by default. Migration diagnostics must preserve the split between raw transcript, redacted transcript slices, accepted memory planes, derived refs, and opaque host refs. Host object payloads are not exported by Beetle Memory; only `HostOpaqueRef` metadata and relation are portable when the ref is visible for the requested view. `RedactedTranscriptSlice` reports message and host-ref redactions so callers can audit what was omitted without seeing the raw material. `TranscriptLifecycleReport.derived_memory_refs` is the review list for accepted Memory material that came from the affected transcript evidence.

Transcript replay and migration tooling can use `TranscriptTurnPage` for bounded paging. `MemoryTranscriptRepairRequest` exposes SDK-level transcript repair inspection, and `TranscriptRepairReport` checks Memory-owned derived refs against transcript source turns/messages. Missing source turns, `MissingSourceMessage`, orphan derived refs, corrupt transcript records, mismatched source keys, and duplicate sequence/cursor evidence are fail-closed repair issues instead of a clean report with hidden evidence loss.

Compact profiles may return fewer transcript turns, host refs, redaction report items, lifecycle derived refs, or repair issues according to `TranscriptGovernanceBudget`. Replay audit records `ProfileBudget` when replay redactions are budget-limited; lifecycle and repair reports set `profile_budget_applied=true` when their report lists are clipped. This is quantity clipping only: profile budget does not make private data visible, skip lifecycle audit, or authorize host business deletion.

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
