# 运维与检查

Operator-facing APIs 用于解释 runtime state、safe recovery actions、lifecycle reports 和 capability visibility。它们不提供 UI，也不引入另一套管理平面。

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

`MemoryInspectionReport` 包含 working recall inspection、capability catalog data、operator action report 和 lifecycle report。

## Skill 记忆管理

独立部署配置台和 CLI 可以管理 Skill 记忆，但它们管理的是 procedural memory record，不是执行器、插件市场或 workflow runner。

SDK 侧入口：

- `MemoryRuntime::list_skills`
- `MemoryRuntime::get_skill`
- `MemoryRuntime::upsert_skill`
- `MemoryRuntime::set_skill_enabled`
- `MemoryRuntime::delete_skill`

所有 mutation 都进入 `MemoryRuntime`，再进入 core skill governance 和 store backend。HTTP console 只路由 `/console/skills*`，CLI 只调用 entry facade；两者都不能直接读写 skill 文件。

## Recover

```rust
use bm_sdk::{MemoryRecoverRequest, RuntimeLifecycleModeInput, RuntimeLifecycleTrigger};

let recovered = runtime.recover(MemoryRecoverRequest {
    trigger: RuntimeLifecycleTrigger::OperatorRequested,
    mode_input: RuntimeLifecycleModeInput::default(),
})?;
```

Recover 只作用于可恢复的 runtime/lifecycle state。它不会跳过 store repair report，也不会重写 persistence schema。

## Close

```rust
use bm_sdk::MemoryCloseRequest;

let closed = runtime.close(MemoryCloseRequest {
    reason: "release smoke complete".to_string(),
})?;
```

Close 会发出 lifecycle event。进程 supervisor 可以根据返回 report 决定是否退出。

## CLI Inspection

```bash
cargo run -p bm-cli --bin bm -- \
  platform capability-snapshot \
  --profile profile-esp-standalone-memory
```

Memory commands 同样通过 `bm-entry`：

```bash
cargo run -p bm-cli --bin bm -- \
  memory capabilities \
  --profile profile-server-linux-dev-full
```
