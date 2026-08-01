# 部署文档

当 Beetle Memory 作为独立进程或协议入口组件运行时，使用 `bm-entry`。`bm-entry` 负责 runtime opening，并把协议命令派发到 SDK 宿主使用的同一套 `MemoryRuntime`。

## 部署形态

| 形态 | Profile | Entry surface |
| --- | --- | --- |
| 本地 CLI/operator 进程 | 与实际编译 host target 一致的 macOS、Windows 或 Linux dev-full profile | `bm-cli` |
| Linux server memory gateway | `profile-server-linux-memory-gateway` | HTTP、WebSocket、MCP、A2A、LLM gateway server；具体 visible 取决于 enabled capability policy 和 `EntryTransportConfig` |
| Linux 硬件设备 | `profile-linux-device-standalone-memory` | local CLI、loopback HTTP/WebSocket |
| ESP standalone memory | `profile-esp-standalone-memory` | compact local/client surfaces with embedded store |

## 构建 Entry Runtime

```rust
use bm_entry::{
    EntryAuthConfig, EntryIdentity, EntryIdempotencyConfig, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendConfig,
};

let mut capability = MemoryCapabilityPolicy::strict_profile();
capability.communication_adapter_enabled = true;

let runtime = EntryRuntime::open(EntryRuntimeConfig {
    identity: EntryIdentity {
        agent_id: "gateway-agent".to_string(),
        owner_id: "owner-default".to_string(),
    },
    scope: EntryScope {
        channel: "gateway".to_string(),
        chat_id: "chat-1".to_string(),
    },
    store: StoreBackendConfig::sqlite(
        "/var/lib/beetle-memory/memory.sqlite3",
        ProfileId::ServerLinuxMemoryGateway,
    )?
    .with_fsync(true),
    transports: EntryTransportConfig::all_enabled(),
    auth: EntryAuthConfig::disabled_for_local(),
    idempotency: EntryIdempotencyConfig { max_keys: 1024 },
    privacy: MemoryPrivacyPolicy::standard_private_boundary(),
    capability,
})?;
```

上面的运行时 view 之所以可以暴露 server entry surfaces，是因为示例显式设置了 `capability.communication_adapter_enabled = true`，并使用 `EntryTransportConfig::all_enabled()` 打开 transport。Profile 只表达允许关系；strict capability snapshot 仍可能显示 `server_allowed=true` 但 `visible=false`。

生产环境应把 `disabled_for_local()` 替换为进程拥有的认证边界。网络流量必须通过 listener 或显式 `*_from_peer` adapter 进入，并从 accept 得到的真实 socket peer 派生信任；只有同进程宿主可以调用名称明确的 `handle_http_in_process_request*` API。

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
| `/memory/long-term/list` | `POST` | accepted long-term memory list/search |
| `/memory/long-term/detail` | `POST` | accepted long-term memory detail |
| `/memory/long-term/mutate` | `POST` | accepted long-term memory mutation |
| `/memory/long-term/policy` | `POST` | long-term governance policy mutation |

`server-std` decoder 使用共享 JSON adapter decoder，支持 write、recall、project、maintain、inspect、recover、replay、long-term list/detail/mutate/policy、capabilities、close。通用 continuity snapshot export/import 不属于协议操作。`Subscribe` 是 stream operation，不是 HTTP memory command。`Maintain` 需要通过 `handle_http_in_process_request_with_services` 注入 LLM/HTTP services；`handle_http_in_process_request` 不注入 services，会对 maintain 返回结构化拒绝。project 与 inspect payload 必须显式提供 `system_max_len`，adapter 不得发明 fallback render budget。

Standard-library HTTP helper 会读取这些 headers：

| Header | 用途 |
| --- | --- |
| `x-idempotency-key` | 可选的调用方幂等材料；transport 派生内部 key，响应不会回显原值。 |
| `authorization: Bearer ...` | 按已配置的 typed principal、owner、tenant 与 operation capabilities 验证。 |

loopback 信任只来自已接受 socket 的真实 peer address。转发的 identity、subject 与 `x-loopback` headers 均是不可信输入，不能授予认证或 capability。非 loopback listener 必须配置 `BM_HTTP_BEARER_TOKEN`、全局唯一的 `BM_HTTP_BEARER_OWNER_ID` 与 `BM_HTTP_BEARER_CAPABILITIES`；`owner_id` 是唯一租户命名空间。缺少 verifier 或非零 read/write timeout 时启动必须 fail closed。

HTTP adapter 在 ingress 固定一份 fresh runtime budget report，并让 header 解析、body admission、dispatch 与 response rendering 共用该身份。超过预算的 declared body 会在分配前被拒绝；响应通过 `HttpRuntimeResponse.budget_report_id` 和 `x-bm-runtime-budget-report-id` 返回同一身份。

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
  "temporal_operation": {"kind": "current"},
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
cargo run --locked -p bm-http --features server-std --bin bm-http-console -- \
  --addr 127.0.0.1:8718 \
  --store-path /var/lib/beetle-memory/http-console-store
```

独立部署需要挂载宿主自管的标准 Agent Skill 目录时，在启动进程前设置 `BM_AGENT_SKILL_DIRS`。多个目录使用当前平台的 path separator 分隔。运行时只读扫描这些目录用于召回和投影，不管理、不执行其中的文件。

```bash
BM_AGENT_SKILL_DIRS=/path/to/project/.agents/skills:/path/to/user/skills \
  cargo run --locked -p bm-http --features server-std --bin bm-http-console -- \
  --addr 127.0.0.1:8718 \
  --store-path /var/lib/beetle-memory/http-console-store
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
- `command.long_term.list`
- `command.long_term.detail`
- `command.long_term.mutate`
- `command.long_term.policy`
- `command.capabilities`

Subscription frame kinds：

- `subscribe.projection`
- `subscribe.inspection`
- `subscribe.replay`
- `subscribe.capability`

Command frames 使用共享 JSON adapter decoder。Subscription frames 只更新 session subscription state 并返回 lifecycle/error events；它们不是 SDK memory commands。

每个 frame 固定一份 fresh runtime budget report；frame admission、subscription admission、dispatch 和返回事件共用该身份，并通过 `WssRuntimeEvent.budget_report_id` 与 `runtime_budget_report_id` 暴露。

召回 frame 示例：

```json
{
  "kind": "command.recall",
  "payload": "{\"query\":\"gateway\",\"limit\":2}"
}
```

## MCP

使用 `bm-mcp` 的 `server-stdio` feature。

Stdio server 使用 bounded streaming read，不缓存无界长行。Stdio 与 Streamable HTTP 为每个 JSON-RPC request 固定一份 fresh runtime budget report；Streamable HTTP 在分配前拒绝超限 header 或 declared body，并返回 `x-bm-runtime-budget-report-id`。

Tool specs 包含：

- `memory_capabilities`
- `memory_recall`
- `memory_project`
- `memory_inspect`
- `memory_replay`
- `memory_write_candidate`
- `memory_long_term_list`
- `memory_long_term_detail`
- `memory_long_term_mutate`
- `memory_long_term_policy`

Stdio helper 对所有已列 tools 使用共享 JSON adapter decoder。Maintain 不在 MCP tool list 中，因为 maintain 需要显式 service injection。

Tool call arguments 示例：

```json
{
  "temporal_operation": {"kind": "current"},
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
      "temporal_operation": {"kind": "current"},
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
- `memory_long_term_list_request`
- `memory_long_term_detail_request`
- `memory_long_term_mutation_request`
- `memory_long_term_policy_request`
- `memory_report`
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
    "temporal_operation": {"kind": "current"},
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
5. 在构造 `EntryTransportContext` 前，从真实 transport 边界派生 `EntryAuthDecision`；禁止把调用方可控布尔值或 loopback header 当成信任证据。
6. Mutation operations 使用 idempotency keys。
7. file/sqlite store 持久化到 runtime 进程拥有的路径。
8. 暴露协议前运行 `memory capabilities` 或 platform capability snapshot。
9. 增加部署 smoke test：通过选定 entry surface 写入、召回并检查 capabilities。
