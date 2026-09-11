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

## 0.7.0 Schema Admission

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
