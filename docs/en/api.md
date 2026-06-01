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
| Runtime Skill List / Detail | `MemoryRuntime::list_runtime_skills` / `MemoryRuntime::get_runtime_skill` | List and inspect runtime-learned procedural memory records without executing them. |
| Runtime Skill Mutation | `MemoryRuntime::edit_runtime_skill` / `MemoryRuntime::set_runtime_skill_enabled` / `MemoryRuntime::delete_runtime_skill` | Edit, enable, disable, or delete existing runtime skills only; it does not create, import, or manage standard Agent Skills. |
| Agent Skill Directory | `MemoryRuntimeBuilder::agent_skill_dirs` / `add_agent_skill_dir` | Hosts can mount standard Agent Skill directories for read-only SDK scanning; the SDK recalls and projects summaries only, and never adds, edits, or executes directory resources. |
| Agent Tool Registry | `MemoryRuntimeBuilder::agent_tool_registry` / `MemoryRuntime::upsert_agent_tool_registry` | Hosts register tool indexes and fingerprints. The SDK returns `agent_tool_hints` only from governed tool experience; no experience means empty hints, not tool routing. |
| Replay | `MemoryRuntime::replay` | Inspect turn ledger history for a chat. |
| Export / Import | `MemoryRuntime::export` / `MemoryRuntime::import` | Move continuity snapshots between scopes. |
| Recover / Close | `MemoryRuntime::recover` / `MemoryRuntime::close` | Control runtime lifecycle and emit lifecycle reports. |

## Memory Evidence System

The Conversation Transcript Substrate release surface is the current base evidence contract for hosts that need governed transcript commit, redacted replay, lifecycle review, and migration-ready evidence handling. It is not a host task system and it does not replace Soul Governance, Subject Projection, Program Memory, procedural memory, or accepted long-term memory planes.

The owner remains `MemoryRuntime`: hosts and adapters provide delivered turn deltas, actor attribution, and opaque host references; Beetle Memory commits evidence, applies governance, and returns reports. External code must not write a parallel transcript store or infer memory facts from raw conversation history.

`MemoryScope::new(channel, chat_id)` remains the single-agent default. Hosts that have a stable conversation id distinct from the legacy chat id can set `MemoryScope::with_conversation_id(...)`; `finalize_turn_and_maintain` and `commit_transcript` also remember the last committed transcript conversation for subsequent recall, projection, maintenance, and inspection calls.

SDK-facing transcript operations:

| Operation | SDK surface | Purpose |
| --- | --- | --- |
| Transcript Commit | `MemoryRuntime::finalize_turn_and_maintain` with `CanonicalTurnDelta`; manual commits use `MemoryTranscriptCommitRequest` / `MemoryTranscriptCommitReport` via `MemoryRuntime::commit_transcript` | Commit a delivered turn as governed evidence under `memory_space_id + channel_id + conversation_id`. |
| Redacted Transcript Replay | `MemoryTranscriptReplayRequest` / `MemoryTranscriptReplayReport` via `MemoryRuntime::replay_transcript` | Read transcript evidence through a scoped view such as model context, host UI, operator audit, or export. |
| Transcript Lifecycle | `MemoryTranscriptLifecycleRequest` / `MemoryTranscriptLifecycleReport` via `MemoryRuntime::request_transcript_lifecycle` | Archive, mask, delete raw content, or run lifecycle review with audit output. |
| Transcript Repair | `MemoryTranscriptRepairRequest` / `MemoryTranscriptRepairReport` via `MemoryRuntime::repair_transcript` | Inspect broken Memory-owned evidence links without scanning host business databases. |
| Transcript Export | `MemoryTranscriptExportRequest` / `MemoryTranscriptExportReport` via `MemoryRuntime::export_transcript`; `MemorySpaceExportRequest { include_private: false }` redacts raw transcript, `conversation_transcript_alias`, and `conversation_transcript_derived_ref` manifests by default | Export a redacted transcript slice, and keep raw transcript plus transcript owner/derived metadata out of public memory-space exports unless the caller explicitly requests private material. |

`MemoryTranscriptReplayRequest` and `MemoryTranscriptExportRequest` take `limit` plus optional `cursor`; their reports return `next_cursor` and `has_more`. SDK callers should page through transcript replay/export through `MemoryRuntime` instead of reaching into the core/store trait. Runtime profile budgets may clamp page size, visible host refs per turn, redaction items, lifecycle derived refs, and repair issues, but they do not relax redaction, lifecycle, or privacy policy. Lifecycle and repair reports set `profile_budget_applied=true` when those report lists are clipped.

Core release-surface concepts:

| Concept | Contract |
| --- | --- |
| `ConversationKey` | `memory_space_id`, `channel_id`, and `conversation_id`; `chat_id` is only a legacy alias or migration source. |
| `ActorAttribution` | Preserves speaker, subject, actor subject, mounted subject, agent id, and trigger source without collapsing them into one identity. |
| `HostOpaqueRef` | Carries host object references such as task, project, ticket, document, or order ids without letting Memory parse host business state machines; `HostRefVisibility` is enforced per replay/export view, and `label` is field-redacted outside owner-approved views with `HostRefLabel` in the redaction report. |
| `RedactedTranscriptSlice` | Separates raw owner-only, model-context, host-UI, operator-audit, and export views, and returns structured `TranscriptRedactionReportItem` entries plus `TranscriptReplayAudit` counts for redacted messages and host refs. |
| `TranscriptLifecycleRequest` | Produces reports and audit events; deleting raw transcript content does not silently delete accepted long-term memory. `TranscriptLifecycleReport` includes affected turn ids, message ids, view-sanitized host refs, host-ref redaction items, and Memory-owned derived refs when they are known. A completed lifecycle request is not the same as a changed transcript: no matching turns means `affected_turns=0` and the SDK lifecycle report has `changed=false`. |
| `TranscriptEvidenceRef` / `DerivedMemoryRef` | Structured Memory-owned evidence references for linking transcript evidence to accepted long-term, shared factual, procedural, private, or soul-handoff material. Display citations may still be strings, but governance should not depend on string parsing alone. |
| `TranscriptTurnPage` / `TranscriptRepairReport` | Bounded transcript paging and repair diagnostics for missing derived-ref source turns, `MissingSourceMessage`, orphan derived ref, corrupt transcript record, mismatched source key, or duplicate sequence/cursor evidence. Repair reports fail closed instead of hiding broken Memory-owned evidence links. |
| `TranscriptGovernanceBudget` | Runtime-budget/profile-owned ceilings for transcript page size, visible host refs, redaction report items, lifecycle derived refs, and repair issues. Store backends persist and page data; they do not own profile budget policy. |

Privacy and projection boundaries:

- Transcript evidence is not automatically a canonical fact, soul mutation, procedural skill, or task experience.
- Accepted long-term, shared factual, procedural skill, private garden, and soul-candidate handoff writes produced through governed candidate writes, manual extraction, or automatic post-turn extraction record structured transcript-derived refs for lifecycle impact review.
- Runtime recall, projection, maintenance, long-term refresh, and operator inspection use transcript-backed evidence before the legacy `SessionStore(chat_id)` shadow; if transcript content is masked, raw-deleted, or its legacy `chat_id` alias cannot be trusted, these paths fail closed instead of falling back to the session shadow.
- Assistant self-claims remain low-authority transcript evidence until governed by the relevant memory plane.
- `HostUi` replay must not expose private garden, inner-life, soul-private raw material, backend traces, or operator-only audit content.
- `ModelContext` replay must pass through privacy gates, profile budget, and model-facing projection policy.
- Host references stay opaque by default; replay can show metadata and relation, not host object payloads. `Export` returns only export-visible refs, and `ModelContext` returns only model-context refs.
- `MemoryRuntime::finalize_turn_and_maintain` reports both session commit and transcript commit status, so transcript backfill is not treated as a no-op when the legacy session shadow already contains the turn.

## Request Shapes

The most common SDK request types are:

| Request type | Required fields | Notes |
| --- | --- | --- |
| `MemoryWriteRequest::Procedural` | `writes`, `source` | Each `RuntimeSkillWrite` includes `name`, `topic`, `title`, `summary`, `content`, `citations`, `source_chat_id`, and `observed_at`. |
| `MemoryWriteRequest::AgentToolUsageFeedback` | `feedback` | Host reports tool execution observations with `registry_ref`; the SDK may turn repeated governed evidence into tool experience. |
| `MemoryWriteRequest::LongTermExtraction` | `extraction` | Use when an extraction pipeline has produced a validated long-term memory extraction. |
| `MemoryRecallRequest` | `query`, `limit`, `tool_registry_refs` | Returns runtime skill hits, standard Agent Skill hits, working recall inspection data, and experience-backed `agent_tool_hints`; without governed experience, hints are empty. |
| `MemoryProjectionRequest` | `user_query`, `system_max_len`, `recent_messages_limit`, `pressure`, `mode_input`, `tool_registry_refs` | Returns `system_memory_block` bounded by `system_max_len`; standard Agent Skills enter only as read-only hint summaries, and Agent Tools enter only as experience hints without full schemas. |
| `MemoryInspectionRequest` | `query`, `system_max_len`, `pressure`, `mode_input` | Returns capability, lifecycle, operator inspection data, the Agent Skill directory report, and the Agent Tool registry report. |
| `RuntimeSkillListRequest` | `query`, `include_disabled`, `include_retired`, `limit` | Returns `RuntimeSkillListReport` with total, active, disabled, runtime_skills, and runtime skill summaries. |
| `RuntimeSkillDetailRequest` | `name` | Returns `RuntimeSkillDetailReport` with summary/procedure/citations/lineage/strategy diffs/raw content. |
| `RuntimeSkillEditRequest` | `name`, `title`, `topic`, `summary`, `procedure`, `edit_reason` | Edits only an existing runtime skill whose name starts with `runtime_skill__`. |
| `RuntimeSkillSetEnabledRequest` | `name`, `enabled` | Changes only the runtime skill enabled state; it does not rewrite skill content. |
| `RuntimeSkillDeleteRequest` | `name` | Deletes the runtime procedural memory from skill storage without adding a console tombstone. |
| `MemoryReplayRequest` | `chat_id`, `limit` | Inspection-only replay surface. |
| `MemoryExportRequest` | `chat_id` | Exports a continuity snapshot. |
| `MemoryImportRequest` | `snapshot`, `target_chat_id`, `mode` | Import mode is `BootstrapImport` or `FullRestore`. |
| `MemoryRecoverRequest` | `trigger`, `mode_input` | Runs recoverable lifecycle recovery. |
| `MemoryCloseRequest` | `reason` | Emits a close lifecycle report. |

Generic adapter dispatch supports write, recall, project, inspect, recover, replay, export, import, capabilities, and close. Maintain is supported only through dispatch paths that supply `AdapterRuntimeServices` with explicit LLM/HTTP services; dispatch without services returns a structured rejection.

Transport helper crates use the shared JSON adapter decoder for their declared memory operations, while stream-only operations such as subscribe stay transport-specific. Check [Deployment Guide](deployment.md) for each protocol's route/frame/tool/message surface.

## Agent Tool API

Agent Tools are host-owned executable tools. Beetle Memory does not manage, install, execute, or store complete tool schemas. The host registers only compact registry snapshots and fingerprints so Memory can bind historical tool experience to the current host tool contract.

SDK and HTTP share the same semantics:

- With governed experience, recall/project returns `agent_tool_hints` containing `tool_id`, `registry_id`, `schema_fingerprint`, the experience reason, permission/risk tags, and `host_execution_required=true`.
- Without governed experience, Memory returns `agent_tool_hints=[]` and `tool_experience_status.reason="no_governed_tool_experience"`; the host decides cold-start tool exposure.
- The host uses `tool_id` / `registry_ref` from the hint to fetch its own complete schema and then builds the real LLM tools payload.
- A Memory hint is not authorization. Permissions, user confirmation, execution, error handling, and provider payloads remain host responsibilities.

Standalone HTTP deployments expose these registry routes:

| Route | Method | Purpose |
| --- | --- | --- |
| `/agent-tool-registries/{id}` | `PUT` | Register or replace a compact registry snapshot; payload `registry_id` must match the path id. |
| `/agent-tool-registries` | `GET` | Return current registry snapshots and the registry report. |
| `/agent-tool-registries/{id}` | `GET` | Return one registry snapshot. |
| `/agent-tool-registries/{id}` | `DELETE` | Delete a registry snapshot. Historical experience remains stored, but future projection rejects it if the registry is missing or the fingerprint drifts. |

`/memory/write` can submit:

```json
{
  "tool_usage_feedback": {
    "registry_ref": {
      "registry_id": "host-tools",
      "fingerprint": "current-fingerprint",
      "scope": "global"
    },
    "observations": [],
    "user_visible_result_summary": "Tool execution summary",
    "reuse_outcome": "succeeded",
    "operator_note": null
  }
}
```

`observations` must be structured execution summaries, not raw full results, secrets, or complete schemas.

## Console API

The Console API is only for standalone deployments that serve the Beetle Memory configuration console. SDK hosts still consume `bm-sdk` or a memory adapter surface; host-owned configuration pages, accounts, and UI remain the host's responsibility.

`bm-entry` owns console state. `bm-http` only routes `/console/*` requests into entry console operations. The Console API does not write memory planes, does not define another memory semantic path, and does not replace `/memory/*`.

`/console/overview` metrics come from real runtime state in the same process: system info reads the active OS, CPU, memory, and system time; storage usage reads the active store path usage and the currently available capacity on that path's system disk; write, recall, and projection metrics are recorded from `/memory/*` operation results. The console frontend must not hard-code observable metrics except as a local fallback when the backend is unreachable.

| Route | Method | Purpose |
| --- | --- | --- |
| `/console/overview` | `GET` | System info, runtime shape, observable metrics, kernel summary, session overview, and current memory context. |
| `/console/skills` | `GET` | Runtime Skill list and summary counts. |
| `/console/skills/{name}` | `GET` | Single runtime Skill detail. |
| `/console/skills/{name}` | `PATCH` | Edit an existing runtime Skill. |
| `/console/skills/{name}/enabled` | `PATCH` | Enable or disable a runtime Skill. |
| `/console/skills/{name}` | `DELETE` | Delete a runtime Skill Memory record. |
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
- The Skill page manages runtime procedural memory only. It does not create or import standard Agent Skills and does not provide a marketplace, executor, or workflow runner.
- Standard Agent Skills are mounted read-only through the SDK builder or the standalone `BM_AGENT_SKILL_DIRS` deployment setting. Runtime scanning reads `SKILL.md` summaries, resource counts, and fingerprints; recall does not read or execute scripts/assets.
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
