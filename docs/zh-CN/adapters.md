# Adapter 合同

Adapter 把外部协议输入转换为同一套 SDK runtime 操作。它们是 transport shell，不是第二套 memory runtime。

协议级 route、frame、tool 和 bridge message 见 [部署文档](deployment.md)。本文定义共享 adapter contract。

## 统一链路

所有协议部署都应遵守这条链路：

```text
transport bytes -> adapter command -> bm-entry -> bm-adapter -> MemoryRuntime -> adapter response
```

`bm-entry` 打开 store 和 runtime，归一化 identity/scope/auth/source/idempotency 数据，然后通过 `bm-adapter` 派发 `AdapterCommand`。

## Entry Runtime 形态

```rust
use bm_entry::{
    EntryAuthConfig, EntryIdentity, EntryIdempotencyConfig, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind,
};

let mut capability = MemoryCapabilityPolicy::strict_profile();
capability.communication_adapter_enabled = true;

let entry = EntryRuntime::open(EntryRuntimeConfig {
    profile: ProfileId::ServerLinuxMemoryGateway,
    identity: EntryIdentity {
        agent_id: "gateway-agent".to_string(),
        owner_id: "owner-default".to_string(),
    },
    scope: EntryScope {
        channel: "gateway".to_string(),
        chat_id: "chat-1".to_string(),
    },
    store: EntryStoreConfig {
        backend: StoreBackendKind::InMemory,
        data_path: None,
        fsync: false,
    },
    transports: EntryTransportConfig::all_enabled(),
    auth: EntryAuthConfig::disabled_for_local(),
    idempotency: EntryIdempotencyConfig { max_keys: 128 },
    privacy: MemoryPrivacyPolicy::standard_private_boundary(),
    capability,
})?;
```

## Transport Crates

| Transport | Crate | 当前接口 |
| --- | --- | --- |
| CLI | `bm-cli` | 本地 memory commands 和 platform capability snapshots。 |
| HTTP | `bm-http` | Request decoding、runtime shell 和 standard-library listener backend。 |
| WebSocket | `bm-wss` | Command frames、subscriptions、budgets 和 WebSocket backend。 |
| MCP | `bm-mcp` | Stdio JSON-RPC tool-call bridge。 |
| A2A | `bm-a2a` | 面向 peer memory messages 的 HTTP bridge。 |

Transport helpers 会对声明的 memory operations 使用共享 JSON adapter decoder。`AdapterCommand` 是共享语义合同，每个 transport crate 的 route/frame/tool/message catalog 是该 crate 当前可执行的协议表面。

## 安全边界

`AdapterAuthContext` 和 `EntryAuthConfig` 表示 entry layer 的认证判断。它们不能替代 SDK privacy policy 或 profile capability checks。Adapter visibility 仍必须来自 runtime capability catalog。
