# Replay And Archive

Replay and governed archives are validation and continuity tools. They do not replace the normal write, recall, project, or maintain path.

## Governed Memory-Space Export And Import

```rust
use bm_sdk::{
    MemoryArchiveScope, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy,
};

let scope = MemoryArchiveScope::subject(
    runtime.memory_space_id(),
    runtime.subject_id(),
)?;
let private_material_policy = MemorySpacePrivateMaterialPolicy::ExcludePrivate;
let exported = runtime.export_memory_space(MemorySpaceExportRequest {
    scope: scope.clone(),
    private_material_policy,
})?;

let imported = runtime.import_memory_space(MemorySpaceImportRequest {
    scope,
    expected_private_material_policy: private_material_policy,
    archive: exported.archive,
})?;
```

The request scope must exactly match the runtime's mounted `(memory_space_id, mounted_subject_id)`, and the archive must declare that same typed scope and private-material policy. All three identities are validated before store reads, import planning, or replacement. Continuity snapshots are internal Soul-recovery payloads and are not a public SDK transfer format.

## Replay Inspection

```rust
use bm_sdk::MemoryReplayRequest;

let replay = runtime.replay(MemoryReplayRequest {
    chat_id: "chat-1".to_string(),
    limit: 32,
})?;
```

Replay explains historical continuity state. It should be used for inspection, archive validation, and release gates.

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
| `Export` | Governed archive and portability | Controlled by `MemorySpacePrivateMaterialPolicy`, profile, permission, and retention policy. |

Deleting or masking raw transcript content must report downstream impact separately from accepted long-term, shared factual, procedural, private, or soul-related memory planes. Lifecycle reports expose host refs through the SDK's operator-audit view, so internal/model-only refs and raw labels are redacted before the report leaves the runtime. Redacted replay must fail closed: when a view cannot prove a field is visible, it returns a redaction marker and audit reason instead of raw content. SDK runtime consumers use transcript-backed evidence ahead of the legacy session shadow, so a masked transcript or untrusted legacy alias is not rehydrated from `SessionStore(chat_id)`.

SDK transcript replay/export requests support bounded cursor pages through `cursor`, `next_cursor`, and `has_more`. Host ref visibility is view-aware, and host ref `label` is field-redacted outside owner-approved views with `HostRefLabel` in the redaction report.

Conversation discovery and navigation use four host-neutral SDK surfaces: `MemoryRuntime::list_conversations`, `query_transcript_timeline`, `search_transcripts`, and `query_transcript_activity`. The catalog lists Memory-owned conversations that already contain governed evidence; an empty host draft is not a transcript conversation. Timeline accepts `TranscriptTimelineAnchor::{Latest, Before, After, Around, AroundSequence, FirstVisibleInRange}` and keeps turns in sequence order. Search returns governed excerpts and a durable `TranscriptAnchor`, which can be passed to `Around` to reopen the same evidence location. Calendar navigation supplies an explicit canonical UTC half-open range to `FirstVisibleInRange`; Memory does not guess a nearest-time window. Activity evaluates bounded UTC ranges and returns visible counts plus first/last anchors for the same timeline.

All CTQ1 continuation values use the Store-owned opaque `TranscriptQueryCursor`. A host must not decode, mint, sign, or inject secret authority for these cursors. Runtime validation binds the operation, exact MemorySpace, mounted subject, filters, view, query digest, direction/anchor, Store incarnation, and owner/index generation, and every page rechecks current capability, lifecycle, privacy, and disclosure. Cursor tampering, cross-subject/conversation/query reuse, expiry, Store replacement, or stale owner/index generation fails closed. Existing forward replay/export cursors remain their current bounded surfaces and are not a substitute for the conversation catalog or timeline.

`HostUi` is only the host-presentable redacted disclosure view. It is not a chat panel, history manager, pagination direction, index owner, or authorization capability. Catalog and timeline remain under `transcript_replay`; indexed search and activity have independent `transcript_search` and `transcript_activity` switches. Platform capability snapshots expose them under `beetle-memory.platform.capability.v4`.

For date navigation, the host converts the user's local date in the user's IANA time zone into a UTC `[start_inclusive, end_exclusive)` range. Memory accepts that canonical range and never guesses or persists the host time zone. Local DST days can span 23 or 25 hours, so hosts must not add a fixed 86,400 seconds. Search and activity hydrate canonical turns, reapply the requested disclosure view, and produce exact-zero visible results for masked or raw-deleted material; neither surface falls back to legacy archive search or host-maintained indexes.

CTQ1 engineering is complete in the local 0.5.0 source candidate. InMemory/File/SQLite atomic head/catalog/time/search closure, persistent reopen, explicit synthetic v10-to-v11 migration, repair/archive closure, private-authority exact-zero, and strict regression evidence are GREEN. This does not claim a Git tag, crates.io or hosted Release publication, real-data migration, or runtime/UAT execution.

Transcript attrs replay with their target turn/message. `TranscriptAttrEnvelope` is for lightweight metadata such as model usage, latency, retry status, attachment summaries, and provenance tags; it is not a replacement for host-owned tasks, capability calls, artifacts, human gates, file workspaces, or governance command/report bodies. `HostUi` sees only HostUi-visible attrs, `ModelContext` sees only model-context attrs, and `Export` sees only export-visible attrs with `export_allowed=true`. Profile budget may clip visible attrs per turn/message and records `AttrValueBudget` with `attr_id` / `attr_key` in `TranscriptRedactionReportItem`; the replay audit also records `ProfileBudget` when profile ceilings caused the clipping.

`HostUi` transcript replay is controlled by the SDK `transcript_replay` capability. Desktop and embedded SDK hosts can commit a transcript turn and read that same conversation back for UI display without enabling `MemoryRuntime::replay`, the development-only `nonproduction-replay-harness`, raw owner replay, or deep inspection.

`TranscriptLifecycleReport.derived_memory_refs` can be used as the target source for the next long-term memory control action. For example, after raw transcript content is masked or deleted, the report lists affected `DerivedMemoryRef` values; a host or operator that wants to revoke the corresponding accepted long-term memory should pass the ref through `MemoryLongTermTarget::TranscriptDerivedRef` and call `MemoryRuntime::mutate_long_term_memory`. Transcript lifecycle never automatically cascades deletion into accepted long-term memory, shared facts, procedural skills, private garden material, or soul handoffs.

## Governed Archive Replacement

Use direct same-scope import when replacing one exact typed memory-space projection:

```rust
use bm_sdk::{
    MemoryArchiveScope, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy,
};

let scope = MemoryArchiveScope::subject(
    source_runtime.memory_space_id(),
    source_runtime.subject_id(),
)?;
let private_material_policy = MemorySpacePrivateMaterialPolicy::ExcludePrivate;
let exported = source_runtime.export_memory_space(MemorySpaceExportRequest {
    scope: scope.clone(),
    private_material_policy,
})?;

assert_eq!(&exported.archive.root().scope, &scope);
assert_eq!(
    exported.archive.root().private_material_policy,
    private_material_policy,
);

target_runtime.import_memory_space(MemorySpaceImportRequest {
    scope,
    expected_private_material_policy: private_material_policy,
    archive: exported.archive,
})?;
```

The source and target runtimes must expose the same exact `MemoryArchiveScope`. The requested scope, the archive root scope, and the private-material policy are validated before replacement. Import recomputes the canonical archive root before any backend mutation and atomically replaces only that scope.

`ExcludePrivate` removes private material as a governed owner closure. A policy mismatch, incomplete dependency closure, root mismatch, or scope mismatch fails closed before any write. The opaque archive keeps its payload private; callers use `GovernedScopeArchiveRootV1` for schema, exact scope, policy, JSON/event counts and byte counts, and the canonical `closure_sha256`.

When transcript evidence is present in memory-space storage, `ExcludePrivate` removes private transcript material and dependent export-visible indexes such as `conversation_transcript_attr` and `conversation_transcript_derived_ref` as one validated closure. Archive diagnostics preserve the split between raw transcript, redacted transcript slices, accepted memory planes, derived refs, and opaque host refs. Host object payloads are not exported by Beetle Memory; only `HostOpaqueRef` metadata and relation are portable when the ref is visible for the requested view. `RedactedTranscriptSlice` reports message, attr, and host-ref redactions so callers can audit what was omitted without seeing the raw material. `TranscriptLifecycleReport.derived_memory_refs` is the review list for accepted Memory material that came from the affected transcript evidence.

Transcript replay and archive tooling can use `TranscriptTurnPage` for bounded paging. `MemoryTranscriptRepairRequest` exposes SDK-level transcript repair inspection, and `TranscriptRepairReport` checks Memory-owned derived refs against transcript source turns/messages. Missing source turns, `MissingSourceMessage`, orphan derived refs, corrupt transcript records, mismatched source keys, and duplicate sequence/cursor evidence are fail-closed repair issues instead of a clean report with hidden evidence loss.

Transcript attr repair is part of the same fail-closed inspection surface. `MissingAttrTargetTurn`, `MissingAttrTargetMessage`, mismatched attr source keys, invalid attr keys, oversized attr values, invalid attr visibility, and corrupt transcript attr records must be reported instead of silently dropping metadata.

Compact profiles may return fewer transcript turns, host refs, attrs, redaction report items, lifecycle derived refs, or repair issues according to `TranscriptGovernanceBudget`. Replay audit records `ProfileBudget` when replay redactions are budget-limited; lifecycle and repair reports set `profile_budget_applied=true` when their report lists are clipped. This is quantity clipping only: profile budget does not make private data visible, skip lifecycle audit, or authorize host business deletion.

## Harness And Proposal Sandbox

- `bm-replay` provides fixture runner, cross-store replay, memory harness gate, and benchmark gate.
- `nonproduction-replay-harness` is a development acceptance feature for fixture and contract validation. It is not a deployment capability, protocol surface, or host runtime dependency.
- `bm-evolve` provides a proposal-only sandbox. A proposal still needs the SDK write path before it changes memory state.
- ESP profiles expose compact validation. `profile-server-linux-dev-full` exposes the full replay and benchmark surface.
- `fixtures/sdk-host-readiness/generic-rust-host/` and `fixtures/sdk-host-readiness/beetle-derived/` are covered by `scripts/check_sdk_host_integration_readiness.sh`.

## Verification

```bash
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_conversation_transcript_substrate.sh
bash scripts/check_release_surface.sh
```
