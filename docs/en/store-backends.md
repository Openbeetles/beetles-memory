# Store Backends

The private `bm-sdk` persistence kernel owns memory persistence. Integrators select a backend and capacity posture through `MemoryStoreHandle`; they do not define memory tables, event lineage, snapshot envelopes, or repair semantics.

## Backends

| Backend | Constructor | Suitable for | Constraints |
| --- | --- | --- | --- |
| In-memory | `StoreBackendConfig::in_memory(profile)` | Tests, examples, short-lived hosts, compact ESP smoke paths | Volatile process-local state |
| File | `StoreBackendConfig::file(root, profile)` | Linux devices, desktop hosts, lightweight standalone deployments | Rejected by ESP profiles |
| SQLite | `StoreBackendConfig::sqlite(path, profile)` | Desktop/server hosts that need durable indexed storage | Requires sqlite-capable profile/store features; rejected by ESP profiles |
| Embedded | `StoreBackendConfig::embedded(profile)` | ESP and small devices | Uses embedded capacity budgets |

## Opening A Store

```rust
use bm_sdk::{MemoryStoreHandle, ProfileId, StoreBackendConfig};

let profile = ProfileId::ServerLinuxMemoryGateway;
let store = MemoryStoreHandle::open(StoreBackendConfig::sqlite(
    "/var/lib/beetle-memory/memory.sqlite3",
    profile,
)?)?;
let open_report = store.open_report();
```

Keep the `StoreOpenReport` in startup diagnostics. It carries schema and repair findings that operators need before the runtime starts accepting writes.

## Repair Policy

`StoreRepairPolicy::ReportOnly` is the default and is appropriate for diagnosis and release gates. Use `StoreRepairPolicy::RepairSafe` only when the runtime is allowed to perform safe repairs after schema and snapshot checks pass.

```rust
use bm_sdk::{StoreBackendConfig, StoreRepairPolicy};

let config = StoreBackendConfig::file("/var/lib/beetle-memory", profile)?
    .with_repair_policy(StoreRepairPolicy::ReportOnly)
    .with_fsync(true);
```

## 0.12.0 Schema Admission: Store v17

The 0.12.0 source accepts only Store v17. Ordered assistant messages, per-message settlement, exact turn identity, Session shadow, transcript attrs/auxiliary material, host-reference labels, and indexes are validated as one governed closure. `DeleteRaw` clears affected raw content and dependent session summary text in that closure. Store v16 fails closed on open: no compatibility read, automatic migration, dual write, or silent replacement. A complete old Store remains with its matching old source tag; any data transition requires a separate owner decision. Archive import does not migrate schema. See the [0.12.0 source release notes](release-notes-0.12.0.md).

## Historical 0.11.2 Schema Admission: Store v16

The 0.11.2 source accepts only Store v16, with immutable long-term material v6 and control revision v4. Canonical turns persist factual admission order; long-term owners retain the control barrier and transition basis. Governance policies bind exact identity/revision to atomic operation receipts. Transcript source withdrawal updates factual evidence and governed indexes in the same transaction. Adapter V2 remains the envelope version, but policy requests and responses have changed; all Beetle crates must come from the same source tag.

Store v15 and earlier are rejected without writing, automatic migration, or creating an empty replacement. Real data is not migrated or deleted by this release. Use a new synthetic Store for development; rollback requires the old source tag and its matching complete Store. Archive import is not a schema migration.

## Historical 0.10.0 Schema Admission: Store v15

The 0.10.0 source accepts only Store v15. Required method provenance, task contributions and persona source review use post-turn input/evidence V3, method material V4, application ledger V3, semantic job/scope index/receipt V4, projection receipt V2, Soul schema 2, source-dependents schema 2 and reconciliation checkpoint schema 2. Long-term material v5 and Adapter V2 are unchanged by this stage.

Opening Store v14 or older is rejected without writing, migrating or silently creating an empty replacement. Validation uses synthetic Stores; real data has not been deleted or migrated. InMemory, File and SQLite share atomic owner/material/head/source/receipt/audit validation and budgeted dependency reads. Missing sources or partial state require repair; recomputing digests does not create authority.

## Historical 0.9.0 Schema Admission

The 0.9.0 source accepts Store v14, the same format as 0.8.0; upgrading from 0.8.0 does not itself require migration or rebuilding data. Store v13 from 0.7.0 and earlier formats remain rejected by the existing clean break, with no migration, compatibility reader or automatic replacement Store. Rejection does not rewrite the original bytes.

Store v14 binds durable producer revisions, current subject validity, exact governed-source dependencies, execution/method contributions and bounded reconciliation checkpoints to the existing atomic mutation/receipt/audit closure. Procedural evidence/job/index/ledger/receipt use V2; Agent Tool material/head use V3, while Runtime Skill owner records use schema 2. Unchanged long-term material remains v5. Reopen checks the same closure, including checkpoint publication receipts and retained contribution references; missing or contradictory authority requires repair rather than trusting a resealed digest. Capacity blocks remain durable until explicit authorized recovery. These changes do not expand backend/profile projection support.

## Published 0.7.0 Schema Admission

The 0.7.0 source accepts Store v13 and immutable long-term material v5 only. Store v13 preserves semantic Post-Turn Governance Job V3 and immutable binding closure, and adds subject-owned Agent Tool experience material/manifests, procedural evidence/jobs/receipts/application ledgers, protected signing authority and governed Runtime Skill dependencies. File, SQLite and in-memory admission validate typed post-images in each transaction; reopen and snapshot import verify complete owner/dependency closure. Canonical Session/Transcript intake is atomic and retains an exact turn digest.

There is no migration API, compatibility reader, dual write, or automatic migration. Store v12 and earlier generations, partial v13 state, orphaned owners/dependencies and foreign schemas fail closed. Older disposable development data must be explicitly recreated by its owner; this release does not delete real data. Public archive export/import is neither a schema migration nor a way to carry signing authority or revive protected Runtime Skill owners.

## File Path Budget

Logical store keys are not filesystem paths. The file backend maps each logical key to a bounded physical address using the profile's `StorePathBudget`, with short digest file names plus a sidecar key index. `list_*_keys`, snapshot export/import, replay, and delete still operate on logical keys.

Do not encode transcript IDs, conversation IDs, attr IDs, or host refs directly into file names. Platform-specific filename and relative-path budgets belong to the private `bm-sdk` persistence kernel, not adapter crates.

## Capacity And Key Budget

`StoreRuntimeBudget` is compiled by Beetle Memory and converted into `StoreCapacityBudget` before the backend opens. The budget covers KV, blob, snapshot, event count, logical namespace/key bytes, event record key bytes, and dedicated export/import byte limits.

Every backend enforces the same budget contract. Oversized logical keys, event record keys, snapshot imports, exports, or cumulative blobs fail with structured `store_budget_exceeded` errors; backends must not truncate keys or silently drop memory.

## Ownership Rules

Allowed:

- Choose backend type, data path, fsync, and repair policy.
- Read `StoreOpenReport`, `StoreRepairReport`, lifecycle reports, and operator diagnosis.
- Use `MemoryRuntime::export_memory_space` / `import_memory_space` with an exact `MemoryArchiveScope` for atomic archive replacement.

Not allowed:

- Write memory state by bypassing `MemoryRuntime`.
- Define a separate memory schema or snapshot envelope.
- Add a second store implementation inside adapter crates.
- Enable sqlite for ESP profiles.
