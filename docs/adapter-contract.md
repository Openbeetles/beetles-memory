# Adapter Contract Guide

Adapter 层把外部通信转换为统一的 SDK runtime 调用。它不是第二套 memory runtime，也不是某个来源项目的兼容入口。

## Contract

所有协议入口都必须形成：

```rust
use bm_adapter::{AdapterCommand, AdapterEnvelope};

let envelope: AdapterEnvelope<AdapterCommand> = /* transport decoded command */;
let response = bm_adapter::dispatch_adapter_command(&runtime, envelope)?;
```

`AdapterEnvelope` 固定 request id、transport、mode、operation、source、auth、idempotency key、audit id 和 payload。`dispatch_adapter_command` 会拒绝 operation / payload 不一致的请求。

独立部署入口统一由 `bm-entry` 打开 store/runtime 并生成 envelope：

```rust
use bm_entry::{EntryRuntime, EntryRuntimeConfig};

let entry = EntryRuntime::open(config)?;
let response = entry.handle(transport_context, adapter_command)?;
```

## Transports

| Transport | Crate | 当前阶段 |
| --- | --- | --- |
| SDK method | `bm-sdk` | 已落地 |
| Entry runtime | `bm-entry` | store/runtime/profile/auth/source/idempotency 入口已落地 |
| CLI | `bm-cli` | command spec、platform capability snapshot 和真实 memory command execution 已落地 |
| HTTP | `bm-http` | HTTP request runtime shell + std listener backend 已落地 |
| Webhook | `bm-http` + `bm-entry` | inbound write candidate runtime shell + std listener backend 已落地 |
| WSS | `bm-wss` | command frame、subscription、budget runtime shell + WebSocket backend 已落地 |
| MQTT | `bm-mqtt` | topic consume/publish runtime shell + external broker client bridge 已落地 |
| MCP | `bm-mcp` | tool-call runtime shell + stdio JSON-RPC backend 已落地 |
| A2A | `bm-a2a` | peer capability + memory message runtime shell + HTTP bridge backend 已落地 |

部署入口已经能把各协议外部字节流 decode 成 adapter command 并调用同一个 runtime。后续替换或增强 TLS、broker、反向代理或 async runtime 时，仍只能沿用这条入口链路。

## Security And Privacy

- `AdapterAuthContext` 表示外部认证结果，不替代 SDK privacy policy。
- adapter visibility 由 profile capability catalog 决定。
- server-side adapter 能力不能反向打开 ESP embedded SDK 的私有数据出口。
- `Maintain` 需要显式 LLM / HTTP service 注入，通用 dispatch 不自动执行。
