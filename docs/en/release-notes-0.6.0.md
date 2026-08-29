# Beetle Memory 0.6.0 Release Notes

Release identity: `v0.6.0`. The immutable annotated tag identifies the exact source commit. Git hosting, crates.io packages, binaries, and hosted Release pages each require separate platform receipts; this document does not pre-claim those actions.

## Breaking Release

Beetle Memory 0.6.0 is a SemVer-breaking SDK and persistence release. PL2 makes post-turn durable governance and long-term learning one Memory-owned service for both Entry-created runtimes and already-constructed embedded multi-subject `MemoryRuntime` consumers.

The exact release contracts are:

- Store v12, Post-Turn Governance Job V3, Scope Index V3, Job Ref V2, immutable binding snapshots, and the binding revision index are the only accepted generation.
- Long-term material v5 remains the exact immutable material contract, platform capability snapshots remain v4, and Adapter V2 remains the only writable adapter protocol.
- Store v11 and governance V2 are rejected. There is no v11-to-v12 migration API, compatibility reader, dual write, or automatic migration. Existing development data must be explicitly discarded and recreated by its owner.
- Binding history is bounded to 256 revisions. The Store prunes only unreferenced revisions and applies backpressure when all retained revisions are referenced.
- Credential and provider-permission recovery are actor-, operation-kind-, job-, authority-, and intent-bound operation-aware durable mutations with one authoritative receipt/audit pair.

## Universal Long-Term Learning

- `bm-sdk::MemoryLearningEngine` owns bounded due discovery, lease/CAS fencing, current transcript/subject/privacy admission, minimum governed disclosure, strict candidate validation, long-term mutation, decision receipt, retry/block/cancel, and terminal completion.
- `bm-entry::MemoryLearningService` owns the bounded process worker, wake/poll lifecycle, immutable binding ingestion, host-neutral credential resolution, official OpenAI-compatible/Ollama execution, and bounded shutdown.
- `MemoryLearningService::attach_runtime` accepts an existing runtime only when Store authority, Subject Registry, and MemorySpace authority match, and it requires operation-scoped control authorities minted by an exact active governing `SystemGovernor` Runtime. It never reconstructs or replaces the host's multi-subject runtime.
- `finalize_turn` commits the canonical transcript and one durable governance intent. A wake is only a hint; Store state remains the recovery truth across File/SQLite reopen and multi-instance claims.
- Hosts provide the delivered-turn fact, one current Provider configuration source, an opaque credential resolver, and process lifecycle. They must not build a second queue, worker, accepted-memory policy, Store schema, or write-after filtering layer.

## Provider, Credential, And Inspection Authority

The product configuration remains host-owned. Beetle persists immutable non-secret execution binding snapshots only to prove what a job was authorized to use. Raw credentials are resolved per attempt, are never serialized or logged, and are destroyed before retry or shutdown boundaries.

Missing or locked credentials cause durable configuration blocking with zero network. A 401 becomes credential rejection; a 403 becomes provider permission blocking; 429, transient I/O, and retryable 5xx use bounded durable backoff. Recovery notifications carry no secret and must advance the exact credential or permission generation.

Learning status is no longer an unguarded report getter. Every read uses a typed inspection authority: service-wide status requires the Runtime actor itself to be the exact active governing `SystemGovernor`, while attachment status requires the exact active mounted-subject authority. Credential and permission recovery use distinct opaque control capabilities bound to Store, registry, MemorySpace, mounted subject, scope, and operation kind. Cross-subject or cross-operation access fails before job identity, reason detail, or mutation is returned.

## Store Closure, Recovery, And Privacy

Every governed transaction validates the changed Job/Scope Index/Binding authority as one post-image; new or changed index references cannot point to missing jobs, index-reference deletion must carry the exact job deletion, binding snapshot/index deletion must be paired, newly bound jobs require the exact immutable binding snapshot and referenced revision, and a referenced binding revision cannot be downgraded or removed. Concurrent first installation of the same canonical binding is idempotent after one exact reread; a divergent payload under the same immutable identity remains a typed conflict. Store open and snapshot import validate the complete closure and fail closed on orphaned or mixed-generation state.

Provider egress is revalidated immediately before the first network byte against the current lease, transcript lifecycle, subject, privacy, and binding authority. Denied, revoked, malformed, or stale work has zero accepted memory mutation. Safe service reports contain only bounded aggregate state; credentials, transcript bodies, private evidence, and denied-subject job details are excluded.

## Evidence Boundary

Release gates cover formatting, checks, clippy, public docs, SDK/Entry/Store contracts, InMemory/File/SQLite persistence and reopen, crash/CAS behavior, profile/cross-target gates, and package dry-runs. Real Provider calls, real user data, GUI or host UAT, trusted Linux external-quality execution, installers, signing/notarization, crates.io publication, and hosted Release publication remain separately evidenced and are not claimed by the source tag.
