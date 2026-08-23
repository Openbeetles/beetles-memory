# Beetle Memory 0.4.0 Release Notes

Release identity: `v0.4.0`. The exact source commit is the immutable commit referenced by that tag. Git hosting, crates.io, binaries, and hosted Release pages require their own provider receipts; this document does not claim those external actions have happened.

## Breaking Release

Beetle Memory 0.4.0 is a SemVer-breaking source and persistence release. `SPV1 Subject Soul Provisioning` makes an AgentPersona Soul a host-neutral, governed lifecycle rather than a collection of independently writable JSON records.

The exact persistent contracts are:

- Store v10 is the only accepted Store schema.
- Long-term material v5 remains the only accepted immutable long-term-memory material schema.
- Subject Soul material, lifecycle heads, manifests, generation tombstones, Relationship Source roots, projections, durable operation receipts, authoritative audit, and events are admitted through typed owners and backend-native atomic transactions.
- Adapter V2 remains the writable adapter protocol. Adapter V1 mutations fail closed.
- Stable, non-sensitive operation key semantics remain mandatory for durable mutations.

## Subject Soul Lifecycle

- `seed=None` is a legal implicit unseeded state and performs no mutation. Beetle Memory does not invent a default personality.
- A partial typed founding charter from an exact active HumanUser creates revision 1 atomically with provenance, immutable material, current Core, revision ledger, lifecycle head, manifest, audit, event, and durable receipt.
- Founding material is marked human-sourced, never self-authored. Later changes use Soul revision governance; a host cannot reapply the founding charter on every turn.
- Active and archived current/exact reads bind the selected generation, revision, and digest. Terminated generations return safe tombstone metadata and never return Soul bodies.
- Archive/restore preserves one generation. Reset and reseed start a new generation; delete is terminal for the current Soul identity. Reset, reseed, and delete purge all raw and derived records owned by the terminated generation in the same transaction.
- Relationship Source Constitution remains an independent relationship-owned root. Its compiled Soul projection uses the most restrictive MentalPrivacy, relationship-source, and Soul self-boundary ceiling and commits under both roots' exact CAS.

## Privacy And Export

Public operator inspection exposes lifecycle state, generation/revision, digests, provenance class, safe tombstone metadata, and typed failure information only. Raw founding charters, SelfAuthoredCore bodies, Private Garden, Inner Life, private documents, relationship secrets, and other inward material are excluded.

Governed disclosure returns only a disclosure-governance-approved summary, rewrite, or refusal. SPV1 does not define a raw Portable Vault, encryption wire format, key lifecycle, identity remapping, or cross-Store Soul import; those remain owned by the deferred EAP2 design.

## Upgrade And Migration Boundary

There is no automatic migration in 0.4.0.

- Store v9 and older stores are not opened, rewritten, or guessed as Store v10.
- Existing opaque Soul records without exact lifecycle roots, manifests, owner/generation envelopes, and closure digests fail closed with a typed migration/repair requirement.
- Back up the exact persistent Store outside its data path before allowing a 0.4.0 binary to open it.
- Rollback requires the previous binary and its matching Store backup. Rolling back only the binary does not roll back data, and archive export/import is neither a schema migration nor a rollback path.
- This source candidate does not include or validate a real-user migration tool. Never test migration against the only copy of real data.

## Published Surface And Evidence Boundary

The intended crates.io set remains `bm-core`, `bm-sdk`, `bm-replay`, `bm-evolve`, `bm-adapter`, `bm-entry`, `bm-ollama-transparent`, `bm-cli`, `bm-llm-gateway`, `bm-http`, `bm-wss`, `bm-mcp`, and `bm-a2a`. `bm-store-contract-tests` and `bm-desktop` are not crates.io packages.

The local source candidate requires formatting, tests, clippy, documentation, cross-backend reopen/crash/multiprocess contracts, strict target compilation, and staged publish dry-runs. Real data migration, Provider/service calls, host or GUI UAT, trusted Linux external-quality execution, installers, signing/notarization, Git commit/tag/push, crates.io publication, and hosted Release publication require separate evidence.
