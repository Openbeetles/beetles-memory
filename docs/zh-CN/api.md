# API 表面

SDK API 是主要入口。宿主项目应通过 `bm-sdk` 进入，或通过 `bm-entry` 加协议 adapter 进入；不应自行实现记忆 schema、store envelope、replay 格式或 adapter dispatch 规则。

## Crates

| Crate | 责任 |
| --- | --- |
| `bm-core` | 记忆平面、召回、投影、生命周期、feature 合同和核心错误模型。 |
| `bm-store` | in-memory、file、sqlite、embedded 后端；schema manifest；event log；snapshot；repair report。 |
| `bm-sdk` | `MemoryRuntime` facade、request/report 类型、capability catalog、profile snapshot、store opening re-export。 |
| `bm-replay` | fixture runner、cross-store replay、harness gate 和 benchmark gate。 |
| `bm-evolve` | proposal-only evolution sandbox 和 SDK 写入 helper。 |
| `bm-adapter` | 协议无关 envelope、command、policy、dispatch 和 response 合同。 |
| `bm-entry` | 进程级 runtime opening、profile/auth/source/idempotency 归一化和 adapter response envelope。 |
| `bm-cli` | CLI 命令、capability rendering、platform snapshot 和 memory command execution。 |
| `bm-http`, `bm-wss`, `bm-mcp`, `bm-a2a` | 消费 `bm-entry` 或 `bm-adapter` 的轻量 transport shell，不拥有记忆语义。 |

## Runtime 操作

| 操作 | SDK method | 用途 |
| --- | --- | --- |
| Write | `MemoryRuntime::write` | 存储 procedural memory 或 long-term extraction 结果。 |
| Recall | `MemoryRuntime::recall` | 按 query 取回 memory hits。 |
| Project | `MemoryRuntime::project` | 生成受长度限制的模型上下文 memory block。 |
| Maintain | `MemoryRuntime::maintain` | 在显式配置 LLM client 后执行 post-reply memory maintenance。 |
| Inspect | `MemoryRuntime::inspect` | 返回 recall/operator/lifecycle inspection 数据。 |
| Replay | `MemoryRuntime::replay` | 检查某个 chat 的 turn ledger 历史。 |
| Export / Import | `MemoryRuntime::export` / `MemoryRuntime::import` | 在 scope 间迁移 continuity snapshot。 |
| Recover / Close | `MemoryRuntime::recover` / `MemoryRuntime::close` | 控制 runtime lifecycle 并产生 lifecycle report。 |

## Request Shapes

最常用的 SDK request types：

| Request type | 必填字段 | 说明 |
| --- | --- | --- |
| `MemoryWriteRequest::Procedural` | `writes`, `source` | 每个 `RuntimeSkillWrite` 包含 `name`、`topic`、`title`、`summary`、`content`、`citations`、`source_chat_id`、`observed_at`。 |
| `MemoryWriteRequest::LongTermExtraction` | `extraction` | 用于 extraction pipeline 已经产出 validated long-term memory extraction 的场景。 |
| `MemoryRecallRequest` | `query`, `limit` | 返回 procedural hits 和 working recall inspection 数据。 |
| `MemoryProjectionRequest` | `user_query`, `system_max_len`, `recent_messages_limit`, `pressure`, `mode_input` | 返回受 `system_max_len` 限制的 `system_memory_block`。 |
| `MemoryInspectionRequest` | `query`, `system_max_len`, `pressure`, `mode_input` | 返回 capability、lifecycle、operator inspection 数据。 |
| `MemoryReplayRequest` | `chat_id`, `limit` | 只做 inspection 的 replay surface。 |
| `MemoryExportRequest` | `chat_id` | 导出 continuity snapshot。 |
| `MemoryImportRequest` | `snapshot`, `target_chat_id`, `mode` | Import mode 是 `BootstrapImport` 或 `FullRestore`。 |
| `MemoryRecoverRequest` | `trigger`, `mode_input` | 执行可恢复 lifecycle recovery。 |
| `MemoryCloseRequest` | `reason` | 发出 close lifecycle report。 |

通用 adapter dispatch 支持 write、recall、project、inspect、recover、replay、export、import、capabilities、close。Maintain 只在调用方通过 `AdapterRuntimeServices` 显式提供 LLM/HTTP services 时执行；未注入 services 的 dispatch 会返回结构化拒绝。

Transport helper crates 会对其声明的 memory operations 使用共享 JSON adapter decoder；subscribe 这类 stream-only operation 仍属于 transport-specific 行为。每种协议的 route/frame/tool/message 表面见 [部署文档](deployment.md)。

## Capability Catalog

每个 runtime 都暴露 `MemoryCapabilityCatalog`。能力可见性由所选 profile、compiled features、runtime policy 和 privacy policy 共同决定。

```rust
let capabilities = runtime.capabilities();
assert!(capabilities.write.visible);
assert!(capabilities.recall.visible);
```

通过 CLI 输出稳定 platform snapshot：

```bash
cargo run -p bm-cli --bin bm -- \
  platform capability-snapshot \
  --profile profile-server-linux-memory-gateway
```

## 边界

外部代码可以选择 profile、打开受支持的 store backend、调用 SDK 操作并消费 report。外部代码不能绕过 `MemoryRuntime` 写记忆状态，也不能实现一条语义不同的 adapter/store 并行路径。
