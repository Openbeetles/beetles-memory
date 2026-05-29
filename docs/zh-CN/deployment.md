# 部署文档

当 Beetle Memory 作为独立进程或协议入口组件运行时，使用 `bm-entry`。`bm-entry` 负责 runtime opening，并把协议命令派发到 SDK 宿主使用的同一套 `MemoryRuntime`。

## 部署形态

| 形态 | Profile | Entry surface |
| --- | --- | --- |
| 本地 CLI/operator 进程 | `profile-server-linux-dev-full` 或 host profile | `bm-cli` |
| Linux server memory gateway | `profile-server-linux-memory-gateway` | HTTP、WebSocket、MCP、A2A、LLM gateway server；具体 visible 取决于 enabled capability policy 和 `EntryTransportConfig` |
| Linux 硬件设备 | `profile-linux-device-standalone-memory` | local CLI、loopback HTTP/WebSocket |
| ESP standalone memory | `profile-esp-standalone-memory` | compact local/client surfaces with embedded store |

## 构建 Entry Runtime

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

let runtime = EntryRuntime::open(EntryRuntimeConfig {
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
        backend: StoreBackendKind::Sqlite,
        data_path: Some("/var/lib/beetle-memory/memory.sqlite3".into()),
        fsync: true,
    },
    transports: EntryTransportConfig::all_enabled(),
    auth: EntryAuthConfig::disabled_for_local(),
    idempotency: EntryIdempotencyConfig { max_keys: 1024 },
    privacy: MemoryPrivacyPolicy::standard_private_boundary(),
    capability,
})?;
```

上面的运行时 view 之所以可以暴露 server entry surfaces，是因为示例显式设置了 `capability.communication_adapter_enabled = true`，并使用 `EntryTransportConfig::all_enabled()` 打开 transport。Profile 只表达允许关系；strict capability snapshot 仍可能显示 `server_allowed=true` 但 `visible=false`。

生产环境应把 `disabled_for_local()` 替换为进程拥有的认证边界。当前 crate 暴露配置边界；你的部署应在请求被标记为 authenticated 之前完成 token、mTLS 或 gateway 认证。

## HTTP

使用 `bm-http` 的 `server-std` feature 可以启用 standard-library listener/helper surface。

Crate 声明的 memory routes：

| Route | Method | Operation |
| --- | --- | --- |
| `/memory/profile/capabilities` | `GET` | capabilities |
| `/memory/write` | `POST` | write procedural memory |
| `/memory/recall` | `POST` | recall |
| `/memory/project` | `POST` | project contract |
| `/memory/maintain` | `POST` | maintain contract |
| `/memory/inspect` | `POST` | inspect contract |
| `/memory/recover` | `POST` | recover contract |
| `/memory/replay` | `POST` | replay contract |
| `/memory/export` | `POST` | export contract |
| `/memory/import` | `POST` | import contract |

`server-std` decoder 使用共享 JSON adapter decoder，支持 write、recall、project、maintain、inspect、recover、replay、export、import、capabilities、close。`Subscribe` 是 stream operation，不是 HTTP memory command。`Maintain` 需要通过 `handle_http_request_with_services` 注入 LLM/HTTP services；`handle_http_request` 不注入 services，会对 maintain 返回结构化拒绝。

Standard-library HTTP helper 会读取这些 headers：

| Header | 用途 |
| --- | --- |
| `x-request-id` | 进入 adapter response reports 的 request id。 |
| `x-idempotency-key` | mutation requests 的 idempotency key。 |
| `x-audit-id` | 进入 adapter events 的 audit id。 |
| `authorization` | 标记请求已认证。 |
| `x-loopback: true` 或 `x-loopback: 1` | 标记本地 loopback 请求已认证。 |

写入 body 示例：

```json
{
  "name": "runtime_skill__server_entry_guard",
  "topic": "server-entry",
  "title": "Server entry guard",
  "summary": "Server runtime accepts HTTP entry requests through bm-entry.",
  "content": "Decode HTTP requests into adapter commands and dispatch through the SDK runtime."
}
```

召回 body 示例：

```json
{
  "query": "server entry",
  "limit": 4
}
```

## macOS Desktop App

macOS 独立桌面形态由 Tauri App 承载。它不是打开外部 HTTP console 的空壳；App 进程内直接打开 `EntryRuntime`、file store 和本地 lifecycle，并通过 Tauri command 调用同一套 console facade。用户不需要先启动 `bm-http-console`。

开发态启动：

```bash
npm --prefix apps/desktop run dev
```

生产打包：

```bash
npm --prefix apps/desktop run build
```

Tauri 开发态会自动启动共享 `apps/console` 前端；生产打包会先构建 `apps/console/dist` 并把静态资源装入桌面 App。

## HTTP Console

独立部署形态可以在同一 HTTP listener 上暴露配置台接口。Console routes 使用同一认证边界，但它们不是 memory operation routes；它们管理 entry 进程级配置和配置台观测状态。

本仓库提供正式可执行入口 `bm-http-console`，用于 Linux server、Linux device、非桌面部署、设备 HTTP console，以及需要显式验证 HTTP shell 的本地开发场景。它不是 macOS 桌面生产入口，不是 example，也不绕过内核；所有 `/console/*` 与 `/memory/*` 请求都进入同一个 `EntryRuntime`。

```bash
cargo run -p bm-http --features server-std --bin bm-http-console -- \
  --addr 127.0.0.1:8718 \
  --store-path target/bm-http-console-store
```

独立部署需要挂载宿主自管的标准 Agent Skill 目录时，在启动进程前设置 `BM_AGENT_SKILL_DIRS`。多个目录使用当前平台的 path separator 分隔。运行时只读扫描这些目录用于召回和投影，不管理、不执行其中的文件。

```bash
BM_AGENT_SKILL_DIRS=/path/to/project/.agents/skills:/path/to/user/skills \
  cargo run -p bm-http --features server-std --bin bm-http-console -- \
  --addr 127.0.0.1:8718 \
  --store-path target/bm-http-console-store
```

HTTP shell 的前端开发态固定使用 `5176`，并把 `/console/*`、`/memory/*` 代理到 `127.0.0.1:8718`。这只验证 HTTP shell；macOS 桌面生产形态应使用上面的 Tauri 启动方式：

```bash
npm --prefix apps/console run dev
```

| Route | Method | 说明 |
| --- | --- | --- |
| `/console/overview` | `GET` | 系统信息、运行形态、观测指标、能力摘要、内核摘要和当前记忆上下文。 |
| `/console/llm-gateway` | `GET` | LLM Gateway 协议端点、规则导出命令和 smoke checks。 |
| `/console/llm-gateway/smoke-checks/{id}/run` | `POST` | 运行后端白名单中的 LLM Gateway smoke check，返回受限输出和退出状态。 |
| `/console/transports` | `GET` | 通信入口列表。 |
| `/console/transports/{id}` | `PATCH` | 更新通信入口。 |
| `/console/devices` | `GET` | 开放设备列表，仅返回 app_key 指纹。 |
| `/console/devices` | `POST` | 添加设备并返回一次性 `appKeyOnce`。 |
| `/console/devices/{id}` | `PATCH` | 更新设备状态。 |
| `/console/devices/{id}/rotate-key` | `POST` | 轮换设备密钥并返回一次性 `appKeyOnce`。 |
| `/console/session` | `GET` | 当前已配对 session 摘要。 |

配置台 HTTP 入口不能被通信页的 HTTP 开关关闭。通信页中的 HTTP 开关只控制外部 memory HTTP API 是否对外开放。

## WebSocket

使用 `bm-wss` 的 `server-std` 或 `client-compact` feature。

Inbound command frame kinds：

- `command.write`
- `command.recall`
- `command.project`
- `command.inspect`
- `command.replay`
- `command.capabilities`

Subscription frame kinds：

- `subscribe.projection`
- `subscribe.inspection`
- `subscribe.replay`
- `subscribe.capability`

Command frames 使用共享 JSON adapter decoder。Subscription frames 只更新 session subscription state 并返回 lifecycle/error events；它们不是 SDK memory commands。

召回 frame 示例：

```json
{
  "kind": "command.recall",
  "payload": "{\"query\":\"gateway\",\"limit\":2}"
}
```

## MCP

使用 `bm-mcp` 的 `server-stdio` feature。

Tool specs 包含：

- `memory_capabilities`
- `memory_recall`
- `memory_project`
- `memory_inspect`
- `memory_replay`
- `memory_write_candidate`
- `memory_export`
- `memory_import`

Stdio helper 对所有已列 tools 使用共享 JSON adapter decoder。Maintain 不在 MCP tool list 中，因为 maintain 需要显式 service injection。

Tool call arguments 示例：

```json
{
  "query": "gateway",
  "limit": 2
}
```

JSON-RPC call 示例：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "memory_recall",
    "arguments": {
      "query": "gateway",
      "limit": 2
    }
  }
}
```

## A2A

使用 `bm-a2a` 的 `bridge-http` feature。

Bridge message names：

- `peer_capability`
- `memory_write_candidate`
- `memory_recall_request`
- `memory_projection_request`
- `memory_report`
- `memory_migration_chunk`
- `runtime_lifecycle_event`

HTTP bridge helper 对映射到 memory operation 的 bridge messages 使用共享 JSON adapter decoder。A2A messages 只携带 memory-report permissions，不能授予 executor、tool 或 workflow permissions。

HTTP bridge route：

```text
POST /a2a/message
```

Message 示例：

```json
{
  "name": "memory_recall_request",
  "payload": {
    "query": "gateway",
    "limit": 2
  }
}
```

## 部署清单

1. 选择一个 profile，并用对应 feature 编译。
2. 为该 profile 选择受支持的 store backend。
3. 制定稳定的 `agent_id`、`owner_id`、`channel`、`chat_id` 策略。
4. 通过 `EntryTransportConfig` 配置 transport visibility。
5. 在构造 authenticated `EntryTransportContext` 前完成认证。
6. Mutation operations 使用 idempotency keys。
7. file/sqlite store 持久化到 runtime 进程拥有的路径。
8. 暴露协议前运行 `memory capabilities` 或 platform capability snapshot。
9. 增加部署 smoke test：通过选定 entry surface 写入、召回并检查 capabilities。
