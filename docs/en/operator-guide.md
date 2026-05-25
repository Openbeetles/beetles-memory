# Operator Guide

Operator-facing APIs explain runtime state, safe recovery actions, lifecycle reports, and capability visibility. They do not provide a UI or a separate management plane.

## Inspect

```rust
use bm_sdk::{MemoryInspectionRequest, PressureLevel, RuntimeLifecycleModeInput};

let report = runtime.inspect(MemoryInspectionRequest {
    query: "release status".to_string(),
    system_max_len: 4096,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
})?;

assert!(report.capabilities.inspection.visible);
```

`MemoryInspectionReport` includes working recall inspection, capability catalog data, deferred governance queue state for the current runtime scope, operator action report, and lifecycle report.

Use inspect after migration dry-run/apply to confirm selected ids, recall planes, deferred job status, lifecycle diagnosis, and safe recovery actions. Operator surfaces may show this report, but they must not read store internals or invent their own plane-count logic.

Projection diagnostics come from `MemoryProjectionReport.audit`, including source planes, selected ids, section chars, budget, and private gate decisions. Conservative compaction comes from `MemoryRuntime::run_retention_compaction()`, whose report explicitly forbids host-side deletion of accepted memory.

## Skill Memory Management

Standalone consoles and CLI operators may manage Skill Memory, but they manage procedural memory records, not executors, marketplaces, or workflow runners.

SDK entry points:

- `MemoryRuntime::list_skills`
- `MemoryRuntime::get_skill`
- `MemoryRuntime::upsert_skill`
- `MemoryRuntime::set_skill_enabled`
- `MemoryRuntime::delete_skill`

Every mutation enters `MemoryRuntime`, then core skill governance and the configured store backend. The HTTP console only routes `/console/skills*`; the CLI only calls the entry facade. Neither path reads or writes skill files directly.

## Recover

```rust
use bm_sdk::{MemoryRecoverRequest, RuntimeLifecycleModeInput, RuntimeLifecycleTrigger};

let recovered = runtime.recover(MemoryRecoverRequest {
    trigger: RuntimeLifecycleTrigger::OperatorRequested,
    mode_input: RuntimeLifecycleModeInput::default(),
})?;
```

Recover acts on recoverable runtime/lifecycle state. It does not skip store repair reports or rewrite the persistence schema.

## Close

```rust
use bm_sdk::MemoryCloseRequest;

let closed = runtime.close(MemoryCloseRequest {
    reason: "release smoke complete".to_string(),
})?;
```

Close emits a lifecycle event. A process supervisor can decide whether to exit based on the returned report.

## CLI Inspection

```bash
cargo run -p bm-cli --bin bm -- \
  platform capability-snapshot \
  --profile profile-esp-standalone-memory
```

Memory commands also go through `bm-entry`:

```bash
cargo run -p bm-cli --bin bm -- \
  memory capabilities \
  --profile profile-server-linux-dev-full
```
