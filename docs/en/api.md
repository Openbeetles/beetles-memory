# API Surface

The SDK API is the primary entry point. Host projects should enter through `bm-sdk` or through `bm-entry` plus a protocol adapter. They should not implement their own memory schema, store envelope, replay format, or adapter dispatch rules.

## Crates

| Crate | Responsibility |
| --- | --- |
| `bm-core` | Memory planes, recall, projection, lifecycle, feature contracts, and core error model. |
| `bm-store` | In-memory, file, sqlite, and embedded backends; schema manifest; event log; snapshots; repair reports. |
| `bm-sdk` | `MemoryRuntime` facade, request/report types, capability catalog, profile snapshots, and store opening re-exports. |
| `bm-replay` | Fixture runner, cross-store replay, harness gate, and benchmark gate. |
| `bm-evolve` | Proposal-only evolution sandbox and SDK write helper. |
| `bm-adapter` | Protocol-independent envelope, command, policy, dispatch, and response contracts. |
| `bm-entry` | Process-level runtime opening, profile/auth/source/idempotency normalization, and adapter response envelope. |
| `bm-cli` | CLI commands, capability rendering, platform snapshots, and memory command execution. |
| `bm-http`, `bm-wss`, `bm-mcp`, `bm-a2a` | Thin transport shells that consume `bm-entry` or `bm-adapter` and do not own memory semantics. |

## Runtime Operations

| Operation | SDK method | Purpose |
| --- | --- | --- |
| Write | `MemoryRuntime::write` | Store procedural memory or long-term extraction results. |
| Recall | `MemoryRuntime::recall` | Retrieve memory hits for a query. |
| Project | `MemoryRuntime::project` | Build a bounded memory block for model context. |
| Maintain | `MemoryRuntime::maintain` | Run explicit post-reply memory maintenance when an LLM client is configured. |
| Inspect | `MemoryRuntime::inspect` | Return recall/operator/lifecycle inspection data. |
| Skill List / Detail | `MemoryRuntime::list_skills` / `MemoryRuntime::get_skill` | List and inspect procedural memory / skill records without executing them. |
| Skill Mutation | `MemoryRuntime::upsert_skill` / `MemoryRuntime::set_skill_enabled` / `MemoryRuntime::delete_skill` | Create, import, edit, enable, disable, or delete skills through procedural memory governance. |
| Replay | `MemoryRuntime::replay` | Inspect turn ledger history for a chat. |
| Export / Import | `MemoryRuntime::export` / `MemoryRuntime::import` | Move continuity snapshots between scopes. |
| Recover / Close | `MemoryRuntime::recover` / `MemoryRuntime::close` | Control runtime lifecycle and emit lifecycle reports. |

## Request Shapes

The most common SDK request types are:

| Request type | Required fields | Notes |
| --- | --- | --- |
| `MemoryWriteRequest::Procedural` | `writes`, `source` | Each `RuntimeSkillWrite` includes `name`, `topic`, `title`, `summary`, `content`, `citations`, `source_chat_id`, and `observed_at`. |
| `MemoryWriteRequest::LongTermExtraction` | `extraction` | Use when an extraction pipeline has produced a validated long-term memory extraction. |
| `MemoryRecallRequest` | `query`, `limit` | Returns procedural hits plus working recall inspection data. |
| `MemoryProjectionRequest` | `user_query`, `system_max_len`, `recent_messages_limit`, `pressure`, `mode_input` | Returns `system_memory_block` bounded by `system_max_len`. |
| `MemoryInspectionRequest` | `query`, `system_max_len`, `pressure`, `mode_input` | Returns capability, lifecycle, and operator inspection data. |
| `MemorySkillListRequest` | `query`, `include_disabled`, `include_retired`, `limit` | Returns `MemorySkillListReport` with total, active, disabled, runtime_learned, user_provided, and skill summaries. |
| `MemorySkillDetailRequest` | `name` | Returns `MemorySkillDetailReport` with summary/procedure/citations/lineage/strategy diffs/raw content. |
| `MemorySkillUpsertRequest` | `title`, `topic`, `summary`, `procedure` | Creates, imports, or edits a skill; `name` is optional and defaults from the normalized topic. |
| `MemorySkillSetEnabledRequest` | `name`, `enabled` | Changes only the enabled state; it does not rewrite skill content. |
| `MemorySkillDeleteRequest` | `name` | Deletes the procedural memory from skill storage without adding a console tombstone. |
| `MemoryReplayRequest` | `chat_id`, `limit` | Inspection-only replay surface. |
| `MemoryExportRequest` | `chat_id` | Exports a continuity snapshot. |
| `MemoryImportRequest` | `snapshot`, `target_chat_id`, `mode` | Import mode is `BootstrapImport` or `FullRestore`. |
| `MemoryRecoverRequest` | `trigger`, `mode_input` | Runs recoverable lifecycle recovery. |
| `MemoryCloseRequest` | `reason` | Emits a close lifecycle report. |

Generic adapter dispatch supports write, recall, project, inspect, recover, replay, export, import, capabilities, and close. Maintain is supported only through dispatch paths that supply `AdapterRuntimeServices` with explicit LLM/HTTP services; dispatch without services returns a structured rejection.

Transport helper crates use the shared JSON adapter decoder for their declared memory operations, while stream-only operations such as subscribe stay transport-specific. Check [Deployment Guide](deployment.md) for each protocol's route/frame/tool/message surface.

## Console API

The Console API is only for standalone deployments that serve the Beetle Memory configuration console. SDK hosts still consume `bm-sdk` or a memory adapter surface; host-owned configuration pages, accounts, and UI remain the host's responsibility.

`bm-entry` owns console state. `bm-http` only routes `/console/*` requests into entry console operations. The Console API does not write memory planes, does not define another memory semantic path, and does not replace `/memory/*`.

`/console/overview` metrics come from real runtime state in the same process: system info reads the active OS, CPU, memory, and system time; storage usage reads the active store path usage and the currently available capacity on that path's system disk; write, recall, and projection metrics are recorded from `/memory/*` operation results. The console frontend must not hard-code observable metrics except as a local fallback when the backend is unreachable.

| Route | Method | Purpose |
| --- | --- | --- |
| `/console/overview` | `GET` | System info, runtime shape, observable metrics, kernel summary, session overview, and current memory context. |
| `/console/skills` | `GET` | Skill Memory list and summary counts. |
| `/console/skills/{name}` | `GET` | Single Skill Memory detail. |
| `/console/skills` | `POST` | Create or import a skill through `MemoryRuntime::upsert_skill`. |
| `/console/skills/{name}` | `PATCH` | Edit a skill through `MemoryRuntime::upsert_skill`. |
| `/console/skills/{name}/enabled` | `PATCH` | Enable or disable a skill. |
| `/console/skills/{name}` | `DELETE` | Delete a Skill Memory record. |
| `/console/llm-gateway` | `GET` | Return the LLM Gateway operator surface: OpenAI/Ollama/MCP endpoints, rule export commands, and smoke checks. |
| `/console/llm-gateway/smoke-checks/{id}/run` | `POST` | Run a backend-whitelisted LLM Gateway smoke check and return exit code, duration, and bounded stdout/stderr. |
| `/console/transports` | `GET` | List configurable communication entries. |
| `/console/transports/{id}` | `PATCH` | Update a communication entry's enabled state or endpoint. |
| `/console/devices` | `GET` | List allowed devices with app_key fingerprints only. |
| `/console/devices` | `POST` | Add a device and let the runtime generate a one-time app_key. |
| `/console/devices/{id}` | `PATCH` | Update device state or label. |
| `/console/devices/{id}/rotate-key` | `POST` | Rotate a device app_key and return the plaintext once. |
| `/console/session` | `GET` | Return paired session account and owner summary. |

Security boundaries:

- List endpoints never return plaintext app_keys; they return `appKeyFingerprint`.
- Device creation and key rotation return `appKeyOnce` only in that response.
- The HTTP switch in the communication page controls the external memory HTTP API, not the HTTP console entry itself.
- The Skill Memory page manages procedural memory. It does not execute skills and does not provide a marketplace, executor, or workflow runner.
- The LLM Gateway smoke runner accepts only backend-known smoke check IDs; it never executes arbitrary command strings supplied by the frontend.

## Capability Catalog

Every runtime exposes a `MemoryCapabilityCatalog`. Visibility is derived from the selected profile, compiled features, runtime policy, and privacy policy.

```rust
let capabilities = runtime.capabilities();
assert!(capabilities.write.visible);
assert!(capabilities.recall.visible);
```

Use the CLI to render a stable platform snapshot:

```bash
cargo run -p bm-cli --bin bm -- \
  platform capability-snapshot \
  --profile profile-server-linux-memory-gateway
```

## Boundary

External code may choose a profile, open a supported store backend, call SDK operations, and consume reports. External code must not bypass `MemoryRuntime` to write memory state or implement a parallel adapter/store path with different semantics.
