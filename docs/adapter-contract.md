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

## Transports

| Transport | Crate | 当前阶段 |
| --- | --- | --- |
| SDK method | `bm-sdk` | 已落地 |
| CLI | `bm-cli` | command spec 和 platform capability snapshot 已落地 |
| HTTP | `bm-http` | 合同层已落地 |
| Webhook | `bm-adapter` + HTTP shell | 合同层已落地 |
| WSS | `bm-wss` | 合同层已落地 |
| MQTT | `bm-mqtt` | 合同层已落地 |
| MCP | `bm-mcp` | 合同层已落地 |
| A2A | `bm-a2a` | 合同层已落地 |

本阶段不启动真实网络 listener。后续 listener 只能把网络消息 decode 成 adapter envelope，再调用同一个 runtime。

## Security And Privacy

- `AdapterAuthContext` 表示外部认证结果，不替代 SDK privacy policy。
- adapter visibility 由 profile capability catalog 决定。
- server-side adapter 能力不能反向打开 ESP embedded SDK 的私有数据出口。
- `Maintain` 需要显式 LLM / HTTP service 注入，通用 dispatch 不自动执行。
