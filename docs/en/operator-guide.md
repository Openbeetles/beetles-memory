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

## Long-Term Memory Control

Operators can inspect and govern accepted long-term memory through the SDK:

- `MemoryRuntime::list_long_term_memory`
- `MemoryRuntime::get_long_term_memory`
- `MemoryRuntime::mutate_long_term_memory`
- `MemoryRuntime::mutate_memory_governance_policy`

These entry points return affected records, tombstones, transcript refs, projection impact, deferred governance impact, policy decision, and lifecycle reports. Operator surfaces may display the report or trigger follow-up governance, but they must not read the `long_term` namespace and mutate content directly.

`forget_by_query` must run a dry-run preview before execution and then pass the confirmation token. `mutate_memory_governance_policy` handles pause/resume/suppress rules for future long-term memory writes; policies do not automatically delete already accepted memory. `DerivedMemoryRef` entries in transcript lifecycle reports are impact lists only; revoking derived long-term memory still calls `mutate_long_term_memory`.

## Runtime Skill And Standard Agent Skill

Standalone consoles and CLI operators manage runtime Skill Memory only: procedural memory records learned during runtime. They are not executors, marketplaces, workflow runners, or managers for standard Agent Skill directories.

SDK entry points:

- `MemoryRuntime::list_runtime_skills`
- `MemoryRuntime::get_runtime_skill`
- `MemoryRuntime::edit_runtime_skill`
- `MemoryRuntime::set_runtime_skill_enabled`
- `MemoryRuntime::delete_runtime_skill`

Every mutation enters `MemoryRuntime`, then core skill governance and the configured store backend. The HTTP console only routes runtime Skill view/edit/enable/delete operations under `/console/skills*`; the CLI only calls the entry facade. Neither path reads or writes skill files directly.

Standard Agent Skills stay host-managed. The SDK provides `MemoryRuntimeBuilder::agent_skill_dirs` / `add_agent_skill_dir` for read-only mounts, and standalone HTTP/CLI deployments can use `BM_AGENT_SKILL_DIRS`. Recall and projection use only `SKILL.md` summaries, resource counts, and fingerprints; scripts are not executed, assets are not read, and the directory never becomes a memory-owned management object. ESP profiles reject standard Agent Skill directory mounts.

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
