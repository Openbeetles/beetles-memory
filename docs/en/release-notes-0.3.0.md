# Beetle Memory 0.3.0 Release Notes

Release identity: `v0.3.0`. The exact source commit is the immutable commit referenced by that tag. Actual Git-hosting and crates.io publication state is owned by each provider's receipt; these notes do not predeclare external-action results.

## Breaking Release

Beetle Memory 0.3.0 is a SemVer-breaking source and persistence release. It closes persistent subject visibility, revision-one long-term intake integrity, durable mutation operation receipts, Store-authoritative audit, and typed HumanUser confirmation authority in the host-neutral Core/SDK/Store owners.

The persistent contracts are exact:

- Store v9 is the only accepted Store schema.
- Long-term material v5 is the only accepted immutable long-term material schema.
- Adapter V2 is the writable protocol contract. Adapter V1 remains available only for read operations; every V1 mutation fails closed.
- Mutation receipt schema v1 remains the receipt schema; protocol and persistence schema numbers do not track the crate SemVer mechanically.

## Required Upgrade Actions

There is no automatic migration in 0.3.0.

- Store v7/v8 state is not opened, rewritten, or guessed as Store v9.
- Material v4, pre-v5 material, and material missing exact subject visibility, provenance, correction evidence, or typed confirmation evidence fail closed with a typed migration/repair requirement.
- Back up the exact persistent Store outside its data path before deploying any 0.3.0 binary that may open it.
- Rollback requires the previous binary and its matching Store backup. Rolling back only the binary is not a data rollback.
- Archive export/import is an exact governed archive operation, not a schema migration path.
- This release does not include or claim validation of a real-user Store migration tool. Do not test a migration against the only copy of real data.

## Rust API Changes

- Long-term creation requires typed initial `subject_visibility` and persistent provenance at revision 1; no temporary `AllSubjects` state is created.
- `last_confirmed_at` is an `Option<u64>` projection derived from typed confirmation evidence. Callers cannot submit confirmation by setting a timestamp.
- Long-term candidate intent carries trusted-host visibility and provenance; model output cannot rewrite the ACL.
- `Correct` is a neutral correction transition. Only an exact active `HumanUser` in the current `SubjectRegistry` can add typed human confirmation in the same transaction. Model-inferred provenance remains model-inferred after later human confirmation.
- `Supersede` clears confirmation on the new factual owner unless that successor is independently confirmed through the governed authority path.
- Operation-aware mutation APIs return typed committed/replayed receipts or an identity conflict instead of relying on an entry-process cache.

## Adapter V2 And Durable Mutation

Durable Adapter V2 `Write` and `LongTermMutate` calls require a stable, non-sensitive caller operation key. A missing key fails before mutation planning. Do not use user text, conversation content, email addresses, tokens, or other private values as the key.

- `Committed` means the effect, Store receipt, authoritative audit, and lifecycle event committed in one Store transaction.
- `Replayed` means the same scoped actor, operation kind, operation identity, and intent digest already committed; the stored safe receipt is returned without a second effect.
- Reusing an operation identity for another intent returns a typed conflict and changes nothing.
- Other public mutation families are explicitly classified as durable, domain-owned receipt, or non-durable; Beetle Memory does not claim global exactly-once semantics.
- Durable receipts are pinned and are not silently evicted. Capacity exhaustion fails the new mutation atomically. In-memory Store receipts do not survive a process restart.

## Subject Visibility And Provenance

Accepted shared long-term facts remain owned once by their `MemorySpace`. `AllSubjects`, `OnlySubjects`, and `HiddenFromSubjects` control whether an exact mounted subject can enter recall, facet, graph, delivery, evidence, body, and projection paths. Current and historical reads use the exact policy and provenance of the selected revision; denied reads return safe fixed audit information without rejected memory content.

## Published Surface

The crates.io publish set is:

```text
bm-core
bm-sdk
bm-replay
bm-evolve
bm-adapter
bm-entry
bm-ollama-transparent
bm-cli
bm-llm-gateway
bm-http
bm-wss
bm-mcp
bm-a2a
```

`bm-store-contract-tests` and `bm-desktop` are not crates.io packages. Desktop installers, signing/notarization, container images, and release attachments require separate release evidence.

## Verification Boundary

The local release candidate requires formatting, workspace tests, clippy, documentation, cross-backend reopen/crash/multiprocess contracts, strict target compilation, and staged `cargo publish --dry-run` for every published crate. Passing those gates proves the local source/package plane only.

Outside this source-package evidence boundary: real old-data migration, real Provider/service calls, host UAT, trusted Linux P7/P8 external-quality execution, hardware acceptance, desktop installers, and signing/notarization. Current Git commit/tag/push, crates.io publication, and public Release status must be verified from the `v0.3.0` tag and the corresponding provider receipts; local tests and this document cannot substitute for them.
