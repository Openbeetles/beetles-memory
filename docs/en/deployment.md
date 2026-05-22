# Deployment Guide

Use `bm-entry` when Beetle Memory runs as a standalone process or protocol-facing component. `bm-entry` owns runtime opening and dispatches protocol commands into the same `MemoryRuntime` used by SDK hosts.

## Deployment Shapes

| Shape | Profile | Entry surface |
| --- | --- | --- |
| Local CLI/operator process | `profile-server-linux-dev-full` or host profile | `bm-cli` |
| Linux server memory gateway | `profile-server-linux-memory-gateway` | HTTP, WebSocket, MCP, A2A, LLM gateway server; concrete visibility depends on enabled capability policy and `EntryTransportConfig` |
| Linux hardware device | `profile-linux-device-standalone-memory` | local CLI, loopback HTTP/WebSocket |
| ESP standalone memory | `profile-esp-standalone-memory` | compact local/client surfaces with embedded store |

## Build An Entry Runtime

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

This runtime view can expose server entry surfaces because the example explicitly sets `capability.communication_adapter_enabled = true` and uses `EntryTransportConfig::all_enabled()` to enable transports. The profile only expresses allowance; strict capability snapshots can still show `server_allowed=true` while `visible=false`.

For production, replace `disabled_for_local()` with an auth boundary owned by your process. The current crate exposes the config boundary; your deployment should enforce token, mTLS, or gateway authentication before requests are marked authenticated.

## HTTP

Enable `bm-http` with `server-std` for the standard-library listener/helper surface.

Memory routes declared by the crate:

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

The `server-std` decoder uses the shared JSON adapter decoder for write, recall, project, maintain, inspect, recover, replay, export, import, capabilities, and close. `Subscribe` is a stream operation, not an HTTP memory command. `Maintain` needs `handle_http_request_with_services` with injected LLM/HTTP services; `handle_http_request` uses no injected services and returns a structured rejection for maintain.

Headers consumed by the standard-library HTTP helper:

| Header | Purpose |
| --- | --- |
| `x-request-id` | Request id used in adapter response reports. |
| `x-idempotency-key` | Idempotency key for mutation requests. |
| `x-audit-id` | Audit id carried into adapter events. |
| `authorization` | Marks the request authenticated. |
| `x-loopback: true` or `x-loopback: 1` | Marks local loopback requests authenticated. |

Example write body:

```json
{
  "name": "runtime_skill__server_entry_guard",
  "topic": "server-entry",
  "title": "Server entry guard",
  "summary": "Server runtime accepts HTTP entry requests through bm-entry.",
  "content": "Decode HTTP requests into adapter commands and dispatch through the SDK runtime."
}
```

Example recall body:

```json
{
  "query": "server entry",
  "limit": 4
}
```

## macOS Desktop App

The macOS standalone desktop shape is hosted by the Tauri app. It is not a wrapper around an external HTTP console; the app process opens `EntryRuntime`, the file store, and the local lifecycle directly, then calls the same console facade through Tauri commands. Users do not need to start `bm-http-console` first.

Development:

```bash
npm --prefix apps/desktop run dev
```

Production bundle:

```bash
npm --prefix apps/desktop run build
```

Tauri development mode starts the shared `apps/console` frontend automatically. Production bundling builds `apps/console/dist` and embeds those static assets in the desktop app.

## HTTP Console

Standalone deployments may expose the configuration console on the same HTTP listener. Console routes use the same authentication boundary, but they are not memory operation routes; they manage entry process configuration and console observability state.

This repository provides the formal `bm-http-console` executable for Linux server, Linux device, non-desktop deployments, device HTTP consoles, and local development scenarios that explicitly verify the HTTP shell. It is not the macOS desktop production entry, it is not an example entry, and it does not bypass the kernel; all `/console/*` and `/memory/*` requests enter the same `EntryRuntime`.

```bash
cargo run -p bm-http --features server-std --bin bm-http-console -- \
  --addr 127.0.0.1:8718 \
  --store-path target/bm-http-console-store
```

The HTTP shell frontend dev server uses port `5176` and proxies `/console/*` and `/memory/*` to `127.0.0.1:8718`. This verifies the HTTP shell only; the macOS desktop production shape should use the Tauri commands above:

```bash
npm --prefix apps/console run dev
```

| Route | Method | Notes |
| --- | --- | --- |
| `/console/overview` | `GET` | System info, runtime shape, observable metrics, capability summary, and kernel summary. |
| `/console/transports` | `GET` | Communication entry list. |
| `/console/transports/{id}` | `PATCH` | Update a communication entry. |
| `/console/devices` | `GET` | Allowed device list with app_key fingerprints only. |
| `/console/devices` | `POST` | Add a device and return one-time `appKeyOnce`. |
| `/console/devices/{id}` | `PATCH` | Update device state. |
| `/console/devices/{id}/rotate-key` | `POST` | Rotate a device key and return one-time `appKeyOnce`. |
| `/console/session` | `GET` | Current paired session summary. |

The HTTP console entry cannot be disabled by the communication page's HTTP switch. That switch only controls whether the external memory HTTP API is exposed.

## WebSocket

Enable `bm-wss` with `server-std` or `client-compact`.

Inbound command frame kinds:

- `command.write`
- `command.recall`
- `command.project`
- `command.inspect`
- `command.replay`
- `command.capabilities`

Subscription frame kinds:

- `subscribe.projection`
- `subscribe.inspection`
- `subscribe.replay`
- `subscribe.capability`

Command frames use the shared JSON adapter decoder. Subscription frames update session subscription state and return lifecycle/error events; they are not SDK memory commands.

Example recall frame:

```json
{
  "kind": "command.recall",
  "payload": "{\"query\":\"gateway\",\"limit\":2}"
}
```

## MCP

Enable `bm-mcp` with `server-stdio`.

Published tool specs include:

- `memory_capabilities`
- `memory_recall`
- `memory_project`
- `memory_inspect`
- `memory_replay`
- `memory_write_candidate`
- `memory_export`
- `memory_import`

The stdio helper uses the shared JSON adapter decoder for all listed tools. Maintain is not listed as an MCP tool because maintain requires explicit service injection.

Example tool call arguments:

```json
{
  "query": "gateway",
  "limit": 2
}
```

Example JSON-RPC call:

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

Enable `bm-a2a` with `bridge-http`.

Bridge message names:

- `peer_capability`
- `memory_write_candidate`
- `memory_recall_request`
- `memory_projection_request`
- `memory_report`
- `memory_migration_chunk`
- `runtime_lifecycle_event`

The HTTP bridge helper uses the shared JSON adapter decoder for bridge messages that map to memory operations. A2A messages only carry memory-report permissions and must not grant executor, tool, or workflow permissions.

The HTTP bridge route is:

```text
POST /a2a/message
```

Example message:

```json
{
  "name": "memory_recall_request",
  "payload": {
    "query": "gateway",
    "limit": 2
  }
}
```

## Deployment Checklist

1. Choose one profile and compile with the matching feature.
2. Choose a supported store backend for that profile.
3. Set a stable `agent_id`, `owner_id`, `channel`, and `chat_id` policy.
4. Configure transport visibility through `EntryTransportConfig`.
5. Enforce authentication before constructing authenticated `EntryTransportContext` values.
6. Use idempotency keys for mutation operations.
7. Persist file/sqlite stores under a path owned by the runtime process.
8. Run `memory capabilities` or the platform capability snapshot before exposing a protocol.
9. Add a deployment smoke test that writes, recalls, and checks capabilities through the selected entry surface.
