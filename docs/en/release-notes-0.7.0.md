# Beetle Memory 0.7.0 Release Notes

Release identity: `v0.7.0`. The annotated tag identifies the exact source commit once published. This document does not pre-claim a Git push, crates.io upload, hosted Release, binary distribution, or deployment.

## Governed procedural learning

Agent Tool experience now has an exact MemorySpace and subject owner with immutable material, scope manifests, current/as-of reads, and durable application evidence. `MemoryLearningEngine` and `MemoryLearningService` run procedural feedback independently of semantic-model configuration: a missing key or failed semantic Provider does not prevent eligible local feedback from completing. Existing semantic Job V3 remains distinct from procedural jobs and receipts.

Projection takes an explicit Preview or Turn binding. Only the final governed delivery can issue a `ProceduralSelectionReceiptV1`; it binds exact subject, turn, source owners/revisions and observed exposure. `finalize_turn` accepts typed learning evidence and execution feedback, not caller-authored selected IDs. Host observations, model inference and active HumanUser confirmation retain distinct authority. A receipt proves exposure, not successful execution.

Runtime Skill first creation is governed by repeated, accepted tool-method evidence. It does not invent steps from tool descriptions or execute tools. Existing revision-checked editing and lifecycle control remain available. Standard Agent Skill packages remain read-only host mounts. Subject feedback never grants shared-program mutation authority.

## Breaking contracts and persistence

- Store v13 is the only accepted Store generation; immutable long-term material v5 and Adapter V2 retain their existing responsibilities. Old and partial stores fail closed. There is no automatic migration, compatibility reader, dual write or old-blob identity inference.
- Public procedural creation, legacy feedback writes, bare selected-ID carry and the unsafe Evolve commit helper have been removed. Use the governed projection/finalize/learning path; do not recreate those owners in adapters.
- Store-held signing authority and protected Runtime Skill owners do not leave through public raw archive export/import. Import is neither a schema-upgrade path nor a way to revive revoked sources. Controlled same-generation Store recovery and public archive disclosure remain separate contracts.
- The operation-aware receipt/audit closure and typed inspection authority remain Memory-owned. Source withdrawal, privacy, capability and subject checks continue to govern current/as-of projection.

## Transcript integrity

Canonical turn intake commits Session shadow and Transcript atomically. Exact owner, turn id and canonical digest identify retries; changed payloads conflict. Different turns containing identical user or assistant text remain separate evidence, including after the bounded Session window fills and after reopen.

`CanonicalTurnDelta.input_messages` is current-turn input only. Decode full protocol history and use `protocol_window_user_delta` before intake; do not compare text with stored history. Empty assistant/tool-call boundaries are preserved, and tool continuation without a new user does not replay an old user message. The Core Session-only commit and text-overlap helpers were removed. Message identity binds owner, subject, turn and ordinal, not the Session count.

## Adoption and evidence boundaries

This is a clean-break source release, not a patch over 0.6.0. Consumers must update public call sites and explicitly recreate disposable older development stores. No real user data is deleted, migrated or reconstructed by this release process. For rollback, use the earlier source tag with its matching untouched Store; never open new-generation data with old code or move an existing release tag.

External benchmark reproduction requires an explicit work root outside the source checkout; it does not recreate a large default benchmark directory locally. Automated contracts, synthetic File/SQLite reopen, strict cross-target compilation and staged package/publish dry-run are separate from real Provider, GUI/UAT, hardware runtime, Linux dynamic quality and external benchmark results. The latter are not certified by this source tag. No crates.io publication, installer, signing/notarization, hosted Release or deployment is implied.
