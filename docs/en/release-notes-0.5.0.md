# Beetle Memory 0.5.0 Release Notes

Release identity: `v0.5.0`. The exact source commit is identified by that immutable tag. Git hosting, crates.io packages, binaries, and hosted Release pages each require their own platform receipt; this document does not pre-claim those external actions.

## Breaking Release

Beetle Memory 0.5.0 is a SemVer-breaking public SDK and persistence release. CTQ1 makes evidenceful conversation discovery, latest/older/newer timeline paging, governed text search, and UTC date-range activity Memory-owned contracts instead of host-owned indexes.

The exact persistent contracts are:

- Store v11 is the only accepted Store schema; long-term material v5 remains exact.
- Platform capability snapshots use `beetle-memory.platform.capability.v4`.
- Adapter V2 remains the only writable adapter protocol.
- Stable durable operation identity remains mandatory for mutation receipts.
- Normal Store open never performs automatic migration.

## Conversation Catalog, Timeline, Search, And Date Navigation

- `MemoryRuntime::list_conversations` lists evidenceful conversations for the exact MemorySpace and mounted subject. Product titles, pinning, draft state, and current selection remain host UX.
- `MemoryRuntime::query_transcript_timeline` supports latest-first presentation, symmetric older/newer paging, durable anchors, exact sequence/time positions, and the first visible message in an explicit UTC half-open range.
- `MemoryRuntime::search_transcripts` searches canonical visible user/assistant text and returns governed Unicode-safe excerpts plus durable anchors into the same timeline.
- `MemoryRuntime::query_transcript_activity` returns visible counts and first/last anchors for explicit UTC ranges, including host-defined calendar days. Beetle Memory does not guess a timezone or own calendar presentation.
- `TranscriptQueryCursor` is opaque, Store-signed, tamper-evident, and bound to operation, scope, mounted subject, disclosure view, query, direction, snapshot, owner generation, keyring incarnation, and expiry.
- `HostUi` remains only a redacted disclosure view. It is not a product-specific API, conversation owner, cursor authority, or authorization token.

## Privacy, Archive, And Repair

`PrivateGardenInternal`, `SoulGovernance`, `OperatorDiagnostic`, denied-subject, masked, and raw-deleted messages are excluded before time/search candidates and postings form. Runtime hydration repeats lifecycle, subject, privacy, capability, and disclosure checks. Public archives omit the private query keyring; same-scope import validates the full CTQ closure and creates fresh cursor authority, so source cursors cannot cross into the restored Store.

File, SQLite, and in-memory contracts cover CTQ closure, reopen, lifecycle exact-zero, cursor tamper/scope/staleness, archive replacement, and repair-required failures without host-side fallback indexes.

## Explicit v10 To v11 Migration

Exact v10 persistent Stores require the offline `MemoryStoreHandle::migrate_v10_to_v11` operation. Close every handle and back up the exact Store first. File migration builds and verifies a sibling v11 Store before an atomic directory swap; SQLite commits schema, CTQ closure, and migration event in one transaction. Success returns `StoreMigrationReport`; partial, foreign, or failed inputs preserve v10 and fail closed. In-memory and embedded backends are not migration targets.

Current evidence uses synthetic Stores only. Real user data was not read or migrated, and archive export/import is not schema migration or rollback.

## Evidence Boundary

The local source candidate must pass formatting, tests, clippy, documentation, profile/cross-target gates, cross-backend reopen contracts, and package dry-run checks before Git publication can be proposed. Provider/service calls, GUI or host UAT, trusted Linux external-quality execution, installers, signing/notarization, real-data migration, Git commit/tag/push, crates.io publication, and hosted Release publication remain separately evidenced actions.
