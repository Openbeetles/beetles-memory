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

`MemoryInspectionReport` 包含 working recall inspection、capability catalog data、当前 runtime scope 的 deferred governance queue、operator action report 和 lifecycle report。

迁移 dry-run/apply 后用 inspect 确认 selected id、recall plane、deferred job status、lifecycle diagnosis 和 safe recovery action。Operator surface 可以展示这个 report，但不能读取 store internal，也不能自造 plane count 逻辑。

Projection 诊断来自 `MemoryProjectionReport.audit`，包括 source plane、selected ids、section chars、budget 和 private gate。保守压缩来自 `MemoryRuntime::run_retention_compaction()`，该 report 明确宿主不能直接删除已接受记忆。

## 长期记忆控制

Operator 可以通过 SDK 查看和治理已接受长期记忆：

- `MemoryRuntime::list_long_term_memory`
- `MemoryRuntime::get_long_term_memory`
- `MemoryRuntime::mutate_long_term_memory`
- `MemoryRuntime::mutate_memory_governance_policy`

这些入口返回 affected records、tombstones、transcript refs、projection impact、deferred governance impact、policy decision 和 lifecycle report。Operator surface 可以展示 report 或发起下一步治理，但不能读取 `long_term` namespace 后直接改内容。

`forget_by_query` 必须先 dry-run preview，再用 confirmation token 执行。`mutate_memory_governance_policy` 用于 pause/resume/suppress 未来长期记忆写入；policy 不会自动删除已有 accepted memory。Transcript lifecycle report 中的 `DerivedMemoryRef` 只是影响清单，撤销派生长期记忆仍需调用 `mutate_long_term_memory`。

## Transcript Attr 检查

Transcript attrs 通过 `MemoryRuntime::replay_transcript`、`MemoryRuntime::export_transcript` 和 `MemoryRuntime::repair_transcript` 检查。Operator 可以用它们展示每条消息的模型用量、latency/status、附件摘要和 provenance 标签，但它们只是 transcript metadata。它们不是任务编辑器、artifact registry、human-gate queue、capability-call ledger、file workspace，也不是 Memory governance command log。

`MemoryRuntime::record_transcript_attrs` 写入前会校验 target turn/message 存在。Dry-run 返回 accepted/rejected attrs 和 `redactions_preview`，不落库。Repair report 会把 attr target/key/value 损坏作为 fail-closed issue。`DeleteRaw` 默认隐藏 attrs，`OperatorAuditOnlyAfterMask` 在 raw deletion 后只返回脱敏审计 metadata。Operator surface 不能绕过 SDK 直接读 `conversation_transcript_attr`，也不能把业务 SQLite record 拼进 replay 结果冒充 Memory 返回。

## 运行时 Skill 与标准 Agent Skill

独立部署配置台和 CLI 只管理运行时 Skill 记忆，也就是系统运行中沉淀出来的 procedural memory record。它们不是执行器、插件市场或 workflow runner，也不管理标准 Agent Skill 目录。

SDK 侧入口：

- `MemoryRuntime::list_runtime_skills`
- `MemoryRuntime::get_runtime_skill`
- `MemoryRuntime::edit_runtime_skill`
- `MemoryRuntime::set_runtime_skill_enabled`
- `MemoryRuntime::delete_runtime_skill`

所有 mutation 都进入 `MemoryRuntime`，再进入 core skill governance 和 store backend。HTTP console 只路由运行时 Skill 的 `/console/skills*` 查看、编辑、启停、删除；CLI 只调用 entry facade；两者都不能直接读写 skill 文件。

标准 Agent Skill 由宿主项目自己添加、编辑、导入和删除。SDK 只提供 `MemoryRuntimeBuilder::agent_skill_dirs` / `add_agent_skill_dir` 只读挂载，独立 HTTP/CLI 部署可通过 `BM_AGENT_SKILL_DIRS` 配置目录。召回和投影只使用 `SKILL.md` 摘要、资源计数和指纹，不执行 scripts，不读取 assets，不把目录变成记忆系统管理对象。ESP profile 禁止挂载标准 Agent Skill 目录。

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
