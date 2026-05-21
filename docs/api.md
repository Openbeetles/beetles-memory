# Public API Surface

Beetle Memory 的公开 API 以 SDK-first 为主线，独立部署通过 runtime + store + adapter contract 组合，不要求调用方实现记忆写入、schema、召回、投影或 lifecycle 语义。

## Crates

| Crate | 责任 | 对外姿态 |
| --- | --- | --- |
| `bm-sdk` | `MemoryRuntime` facade、write / recall / project / maintain / inspect / replay / export / import、capability catalog、platform capability snapshot | 普通宿主和独立 runtime 的首选入口 |
| `bm-store` | in-memory、file、sqlite、embedded store、schema manifest、event log、snapshot、repair report | 本项目自有持久化层，调用方只选后端和容量 |
| `bm-replay` | replay fixture、SDK-driven runner、cross-store replay、harness / benchmark gate | 验证、迁移和回放入口 |
| `bm-evolve` | proposal-only evolution sandbox、profile policy、SDK commit helper | 记忆演化提案入口，不直接绕过 store 写入 |
| `bm-adapter` | 协议无关 `AdapterEnvelope`、`AdapterCommand`、`AdapterPolicy`、dispatch | 独立部署的协议合同层 |
| `bm-entry` | process-level entry runtime、store opening、profile/auth/source/idempotency normalization、adapter response envelope | 独立部署和本地入口的共享地基 |
| `bm-cli` | CLI command spec、capability rendering、platform snapshot、真实 memory command execution | 运维检查、本地迁移和 release gate 入口 |
| `bm-http` / `bm-wss` / `bm-mqtt` / `bm-mcp` / `bm-a2a` | thin adapter crates + feature-gated entry runtime shells | HTTP/Webhook、WSS、MQTT、MCP、A2A 入口层，不拥有记忆语义 |

## SDK Entry

最小宿主入口固定为：

```rust
use bm_sdk::{
    MemoryIdentity, MemoryRuntime, MemoryScope, ProfileId, StoreBackendConfig, StorePlatform,
};

let profile = ProfileId::DesktopMacosEmbeddedSdk;
let store = StorePlatform::open(StoreBackendConfig::in_memory(profile)?)?;
let runtime = MemoryRuntime::builder()
    .identity(MemoryIdentity::new("agent-main", "owner-default")?)
    .scope(MemoryScope::new("local", "chat-1")?)
    .profile(profile)
    .store_platform(store)
    .build()?;
```

调用方不能把 core store trait 当成宿主扩展点；store schema、event lineage、snapshot envelope、repair report 和 lifecycle event 均由 `bm-store` / `bm-sdk` 负责。

## Runtime Operations

| Operation | SDK method | 说明 |
| --- | --- | --- |
| Write | `MemoryRuntime::write` | 写入 procedural memory 或 long-term extraction |
| Recall | `MemoryRuntime::recall` | 跨 working / procedural / long-term 等平面召回 |
| Projection | `MemoryRuntime::project` | 生成可喂给模型上下文的 memory block |
| Maintenance | `MemoryRuntime::maintain` | post-reply 记忆维护，需要显式 LLM / HTTP 注入 |
| Inspection | `MemoryRuntime::inspect` | operator 诊断和 working recall inspection |
| Replay | `MemoryRuntime::replay` | turn ledger replay inspection |
| Export / Import | `MemoryRuntime::export` / `MemoryRuntime::import` | continuity snapshot 迁移 |
| Recover / Close | `MemoryRuntime::recover` / `MemoryRuntime::close` | lifecycle 控制 |

## Capability Contract

每个 runtime 都有 `MemoryCapabilityCatalog`。profile feature、runtime policy、privacy policy 和 compiled feature 会共同决定能力是否可见：

```rust
let capabilities = runtime.capabilities();
assert!(capabilities.write.visible);
assert!(capabilities.recall.visible);
```

发布和跨平台验收使用稳定 JSON：

```bash
cargo run -p bm-cli --bin bm -- \
  platform capability-snapshot \
  --profile profile-server-linux-memory-gateway
```

## Adapter Boundary

协议层必须通过 `bm-adapter` 的 `AdapterEnvelope<AdapterCommand>` 进入 SDK runtime。HTTP、Webhook、WSS、MQTT、MCP、A2A 是 transport shell，不能复制一套记忆写入、召回或投影语义。

入口运行时阶段已经把 CLI、HTTP/Webhook、WSS、MQTT、MCP 和 A2A 的请求转换为 `bm-entry` / `bm-adapter` 调用。协议 crate 仍然只是入口外壳；socket、broker 或 stdio framing 的具体部署形态可以继续演进，但不得绕过 `MemoryRuntime`。
