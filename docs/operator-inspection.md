# Operator And Inspection Guide

Operator surface 用于运行状态解释、恢复、关闭和发布前检查。它不是管理控制台，也不引入 UI。

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

`MemoryInspectionReport` 包含 working recall inspection、capability catalog、operator action report 和 lifecycle report。

## Recover

```rust
use bm_sdk::{MemoryRecoverRequest, RuntimeLifecycleModeInput, RuntimeLifecycleTrigger};

let recovered = runtime.recover(MemoryRecoverRequest {
    trigger: RuntimeLifecycleTrigger::OperatorRequested,
    mode_input: RuntimeLifecycleModeInput::default(),
})?;
```

Recover 只恢复 soul kernel / lifecycle 可恢复状态，不重写 store schema，也不跳过 repair report。

## Close

```rust
use bm_sdk::MemoryCloseRequest;

let closed = runtime.close(MemoryCloseRequest {
    reason: "release smoke complete".to_string(),
})?;
```

Close 会写 lifecycle event。独立部署时上层 supervisor 可以根据 lifecycle report 决定是否退出进程。

## CLI Snapshot

```bash
cargo run -p bm-cli --bin bm -- \
  platform capability-snapshot \
  --profile profile-esp-standalone-memory
```

这个命令用于检查 profile 编译能力和 release fixture 是否一致。
