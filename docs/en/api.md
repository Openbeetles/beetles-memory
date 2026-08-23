# API Surface

The SDK API is the primary entry point. Host projects should enter through `bm-sdk` or through `bm-entry` plus a protocol adapter. They should not implement their own memory schema, store envelope, replay format, or adapter dispatch rules.

## Crates

| Crate | Responsibility |
| --- | --- |
| `bm-core` | Memory planes, recall, projection, lifecycle, feature contracts, and core error model. |
| `bm-sdk` | `MemoryRuntime` facade, opaque `MemoryStoreHandle`, request/report types, capability catalog, profile snapshots, and its private persistence kernel. |
| `bm-store-contract-tests` | Non-published development contract tests for the `bm-sdk` persistence kernel. |
| `bm-replay` | Development fixture runner, cross-store replay, harness gate, and benchmark gate; `nonproduction-replay-harness` is not a deployment capability. |
| `bm-evolve` | Proposal-only evolution sandbox and SDK write helper. |
| `bm-adapter` | Protocol-independent envelope, command, policy, dispatch, and response contracts. |
| `bm-entry` | Process-level runtime opening, profile/auth/source/idempotency normalization, and adapter response envelope. |
| `bm-cli` | CLI commands, capability rendering, platform snapshots, and memory command execution. |
| `bm-http`, `bm-wss`, `bm-mcp`, `bm-a2a` | Thin transport shells that consume `bm-entry` or `bm-adapter` and do not own memory semantics. |
| `bm-ollama-transparent` | Published macOS-local controller for Ollama App transparent mode: cross-process OS transition lease, exact PID/start/executable receipts, verified executable launch with recoverable `launchd` job authority, and bounded process/probe reports. Its caller must provide an explicit absolute gateway path and typed memory authority; model and memory semantics remain owned by `bm-llm-gateway`. |

## Runtime Operations

| Operation | SDK method | Purpose |
| --- | --- | --- |
| Write | `MemoryRuntime::write` | Store procedural memory or long-term extraction results. |
| Recall | `MemoryRuntime::recall` | Retrieve memory hits for a query. |
| Project | `MemoryRuntime::project` | Build a bounded memory block for model context. |
| Maintain | `MemoryRuntime::maintain` | Run explicit post-reply memory maintenance when an LLM client is configured. |
| Inspect | `MemoryRuntime::inspect` | Return recall/operator/lifecycle inspection data. |
| Runtime Skill List / Detail | `MemoryRuntime::list_runtime_skills` / `MemoryRuntime::get_runtime_skill` | List and inspect runtime-learned procedural memory records without executing them. |
| Runtime Skill Mutation | `MemoryRuntime::edit_runtime_skill` / `MemoryRuntime::set_runtime_skill_enabled` / `MemoryRuntime::retire_runtime_skill` | Edit, enable, disable, or retire existing runtime skills only; it does not create, import, or manage standard Agent Skills. |
| Long-Term Memory List / Detail | `MemoryRuntime::list_long_term_memory` / `MemoryRuntime::get_long_term_memory` | List, search, and inspect accepted long-term memory with redacted views, evidence summaries, revisions, and tombstone metadata. |
| Long-Term Memory Mutation | `MemoryRuntime::mutate_long_term_memory` | Correct, supersede, delete, forget_by_query, mark_stale, or change_scope accepted long-term memory, and return affected records, tombstones, projection impact, and lifecycle reports. |
| Long-Term Governance Policy | `MemoryRuntime::mutate_memory_governance_policy` | Pause, resume, or suppress future long-term memory updates. Policies affect future write governance and do not silently delete accepted memory. |
| Agent Skill Directory | `MemoryRuntimeBuilder::agent_skill_dirs` / `add_agent_skill_dir` | Hosts can mount standard Agent Skill directories for read-only SDK scanning; the SDK recalls and projects summaries only, and never adds, edits, or executes directory resources. |
| Agent Tool Registry | `MemoryRuntimeBuilder::agent_tool_registry` / `MemoryRuntime::upsert_agent_tool_registry` | Hosts register tool indexes and fingerprints. The SDK returns `agent_tool_hints` only from governed tool experience; no experience means empty hints, not tool routing. |
| Replay | `MemoryRuntime::replay` | Inspect turn ledger history for a chat. |
| Transcript Attr Write | `MemoryRuntime::record_transcript_attrs` | Attach governed turn/message metadata to transcript evidence for replay, export, redaction, repair, and profile budgeting. |
| Memory-Space Export / Import | `MemoryRuntime::export_memory_space` / `MemoryRuntime::import_memory_space` | Export an opaque archive and atomically replace the same exact `MemoryArchiveScope` under an explicit private-material policy. |
| Recover / Close | `MemoryRuntime::recover` / `MemoryRuntime::close` | Control runtime lifecycle and emit lifecycle reports. |

## Subject Soul Provisioning

`bm-sdk` 0.4.0 exposes a host-neutral Subject Soul provisioning and lifecycle contract. Hosts submit typed intent only; Core, SDK, and Store own Soul revisions, generations, material, manifests, ledgers, audit records, events, and durable operation receipts in one transaction. Adapters, HTTP, MCP, Console, and host databases must not maintain a second Soul state or create a default personality and overwrite it later.

| Operation | SDK surface | Contract |
| --- | --- | --- |
| Optional provisioning | `MemoryRuntime::provision_subject_soul` + `SubjectSoulProvisionIntentV1` | `Unseeded` is a legal zero-mutation state. `Founding` accepts only a canonical partial charter from an active `HumanUser` in the same MemorySpace and atomically creates generation 1 / revision 1. |
| Safe read | `MemoryRuntime::read_subject_soul` + `SubjectSoulReadRequestV1` | Public reads expose only `OperatorSafe` metadata. `Current` and `Exact` selectors are verified through an immutable closure; terminated generations return tombstone metadata only. |
| Safe export | `MemoryRuntime::export_subject_soul_operator_safe` | Returns only state, generation, revision, digests, origin, and safe tombstones. It never returns the founding charter, SelfAuthoredCore, Private Garden, Inner Life, private documents, or relationship-private bodies. |
| Governed disclosure | `MemoryRuntime::disclose_subject_soul_governed` | Consumes only Store-verified Soul/relationship closure data and applies the effective MentalPrivacy and Relationship Source disclosure ceiling to return a governed summary, rewrite, or refusal. Hosts cannot submit a purportedly safe summary. |
| Lifecycle | `MemoryRuntime::archive_subject_soul_self_governed` / `restore_subject_soul_self_governed` / `mutate_subject_soul` | SDK injects the capability for self-governed archive/restore and never exposes it to callers. Maintenance archive/restore uses a typed `SystemGovernor`. Reset/reseed/delete bind a `SystemGovernor`, an active same-space `HumanUser` confirmation, and the exact generation/head/manifest. |
| Relationship source | `MemoryRuntime::control_relationship_source` / `read_relationship_source` | Public contributions accept only an exact relationship-member `HumanUser`; SDK-internal capability owners apply Agent self-boundaries and SystemGovernor floors. Relationship Source Constitution uses independent source and manifest roots with dual-root/four-CAS closure; Soul lifecycle does not replace relationship governance. |
| Governed projection | `MemoryRuntime::project_with_subject_soul_selector` | Current projection reads the verified current Soul. Historical projection requires an explicit exact Soul selector and never applies the current Soul to historical memory projection. |

A founding charter is an optional, partial constitutional seed, not a raw character profile. It may contain an identity anchor, character tendencies, priority/non-negotiable constitution, default response/initiative/relationship postures, and boundary, truth-seeking, self-preservation, repair, and change principles. Display names, forms of address, appearance/background, task roles, tool habits, and host presentation remain with their respective host owners and cannot be promoted into Soul through provisioning.

After provisioning, personality changes must enter the existing self-authored revision proposal and governance path; a host must not provision on every turn. Reset/reseed/delete are separate destructive lifecycle operations: old-generation raw material and derived private data are removed in the same transaction, and old exact selectors can return safe tombstones only. SPV1 does not define raw Soul import, Portable Vault, encrypted wire formats, or key lifecycle; EAP2 continues to own those concerns.

Failures are returned as typed `SubjectSoulSdkError { operation, key, disposition }` values. A caller may re-read verified state and retry an `ExpectedStateConflict`; it must not bypass `RepairRequired`, `AuthorityRejected`, `CapacityRejected`, or `StoreCommitRejected` by writing directly to Store.

Generation-owned Soul layer envelopes, autonomous capabilities, Core revision plans, and Store post-images are not public host write surfaces. SDK runs autonomous adjudication from a durable governance job, a verified Soul snapshot, and typed evidence, then commits one operation-aware Store batch. A host cannot submit `origin`, `revision`, `next_core`, a ledger, or raw private-layer JSON and claim that it is self-authored growth.

## Memory Evidence System

The Conversation Transcript Substrate release surface is the current base evidence contract for hosts that need governed transcript commit, redacted replay, lifecycle review, and archive-ready evidence handling. It is not a host task system and it does not replace Soul Governance, Subject Projection, Program Memory, procedural memory, or accepted long-term memory planes.

The owner remains `MemoryRuntime`: hosts and adapters provide delivered turn deltas, actor attribution, and opaque host references; Beetle Memory commits evidence, applies governance, and returns reports. External code must not write a parallel transcript store or infer memory facts from raw conversation history.

`MemoryScope::new(channel, chat_id)` remains the single-agent default. Hosts that have a stable conversation id distinct from the legacy chat id can set `MemoryScope::with_conversation_id(...)`; `finalize_turn` and `commit_transcript` also remember the last committed transcript conversation for subsequent recall, projection, maintenance, and inspection calls.

SDK-facing transcript operations:

| Operation | SDK surface | Purpose |
| --- | --- | --- |
| Transcript Commit | `MemoryRuntime::finalize_turn` with `CanonicalTurnDelta`; manual commits use `MemoryTranscriptCommitRequest` / `MemoryTranscriptCommitReport` via `MemoryRuntime::commit_transcript` | Commit a delivered turn as governed evidence under `memory_space_id + channel_id + conversation_id`. |
| Redacted Transcript Replay | `MemoryTranscriptReplayRequest` / `MemoryTranscriptReplayReport` via `MemoryRuntime::replay_transcript` | Read transcript evidence through a scoped view such as model context, host UI, operator audit, or export. |
| Transcript Lifecycle | `MemoryTranscriptLifecycleRequest` / `MemoryTranscriptLifecycleReport` via `MemoryRuntime::request_transcript_lifecycle` | Archive, mask, delete raw content, or run lifecycle review with audit output. |
| Transcript Repair | `MemoryTranscriptRepairRequest` / `MemoryTranscriptRepairReport` via `MemoryRuntime::repair_transcript` | Inspect broken Memory-owned evidence links without scanning host business databases. |
| Transcript Attr Write | `MemoryTranscriptAttrWriteRequest` / `MemoryTranscriptAttrWriteReport` via `MemoryRuntime::record_transcript_attrs` | Write turn/message `TranscriptAttrEnvelope` records after the transcript target exists. This is for lightweight metadata such as per-message model usage, runtime latency/status, attachment summaries, and provenance tags. |
| Transcript Export | `MemoryTranscriptExportRequest` / `MemoryTranscriptExportReport` via `MemoryRuntime::export_transcript`; `MemorySpaceExportRequest { private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate, .. }` excludes private transcript material and its dependent export-visible indexes as one governed closure | Export a redacted transcript slice, and keep private transcript material out of public memory-space archives unless the caller explicitly selects `IncludePrivate`. |

`MemoryTranscriptReplayRequest` and `MemoryTranscriptExportRequest` take `limit` plus optional `cursor`; their reports return `next_cursor` and `has_more`. SDK callers should page through transcript replay/export through `MemoryRuntime` instead of reaching into the core/store trait. Runtime profile budgets may clamp page size, visible host refs per turn, visible attrs per turn/message, redaction items, lifecycle derived refs, and repair issues, but they do not relax redaction, lifecycle, or privacy policy. Lifecycle and repair reports set `profile_budget_applied=true` when those report lists are clipped.

Transcript attrs are Memory-owned transcript metadata, not a host business object store. Every attr has a `TranscriptAttrTarget`, namespaced key, `TranscriptAttrValueKind`, JSON value, `HostRefVisibility`, `TranscriptAttrSource`, `TranscriptAttrGovernance`, and optional `TranscriptAttrLink` refs. `HostUi` replay returns only HostUi-visible attrs, `ModelContext` returns only model-context attrs, `OperatorAudit` returns audit-visible attrs, and `Export` returns only export-visible attrs with `export_allowed=true`. `RawOwnerOnly` remains internal. Store repair reports missing target turns/messages, mismatched attr source keys, invalid keys, oversized values, and corrupt attr records as fail-closed issues. `DeleteRaw` hides attrs by default; `OperatorAuditOnlyAfterMask` may leave only redacted audit metadata and never returns the original attr value after raw deletion.

`MemoryTranscriptAttrWriteReport` returns `accepted_attrs`, `rejected_attrs`, `redactions_preview`, `profile_budget_applied`, and `audit_event_id` in addition to the lifecycle report. Transport adapters must return those SDK fields instead of collapsing the report to counts.

Do not put host-owned records or raw payloads into attrs. `Task`, `TaskDelegation`, `PolicyDecision`, `HumanGate`, `CapabilityCall`, `ArtifactRecord`, `FileWorkspace`, file revisions, and Memory governance command/report bodies remain owner records elsewhere; attrs may only link to them or provide lightweight display labels. Attr values must not contain raw prompts, provider secrets, raw memory values, local filesystem paths, complete attachment contents, or private host database payloads.

`HostUi` transcript replay is the safe conversation readback surface for host UI. It is governed by `capabilities.transcript_replay`, not by the debug/inspection-oriented `capabilities.replay` used by `MemoryRuntime::replay`.

Core release-surface concepts:

| Concept | Contract |
| --- | --- |
| `ConversationKey` | `memory_space_id`, `channel_id`, and `conversation_id`; `chat_id` remains the turn-ledger inspection key for `MemoryReplayRequest`. |
| `ActorAttribution` | Preserves speaker, subject, actor subject, mounted subject, agent id, and trigger source without collapsing them into one identity. |
| `HostOpaqueRef` | Carries host object references such as task, project, ticket, document, or order ids without letting Memory parse host business state machines; `HostRefVisibility` is enforced per replay/export view, and `label` is field-redacted outside owner-approved views with `HostRefLabel` in the redaction report. |
| `TranscriptAttrEnvelope` | Carries governed turn/message metadata. `TranscriptAttrScope` is `turn` or `message`; keys must be namespaced under `host.*` or `memory.*`; values are typed by `TranscriptAttrValueKind`; visibility, export policy, redaction policy, source, links, and value-size budgets are enforced by Memory. |
| `RedactedTranscriptSlice` | Separates raw owner-only, model-context, host-UI, operator-audit, and export views, and returns structured `TranscriptRedactionReportItem` entries plus `TranscriptReplayAudit` counts for redacted messages and host refs. |
| `TranscriptLifecycleRequest` | Produces reports and audit events; deleting raw transcript content does not silently delete accepted long-term memory. `TranscriptLifecycleReport` includes affected turn ids, message ids, view-sanitized host refs, host-ref redaction items, and Memory-owned derived refs when they are known. A completed lifecycle request is not the same as a changed transcript: no matching turns means `affected_turns=0` and the SDK lifecycle report has `changed=false`. |
| `TranscriptEvidenceRef` / `DerivedMemoryRef` | Structured Memory-owned evidence references for linking transcript evidence to accepted long-term, shared factual, procedural, private, or soul-handoff material. Display citations may still be strings, but governance should not depend on string parsing alone. |
| `TranscriptTurnPage` / `TranscriptRepairReport` | Bounded transcript paging and repair diagnostics for missing derived-ref source turns, `MissingSourceMessage`, orphan derived ref, corrupt transcript record, mismatched source key, or duplicate sequence/cursor evidence. Repair reports fail closed instead of hiding broken Memory-owned evidence links. |
| `TranscriptGovernanceBudget` | Runtime-budget/profile-owned ceilings for transcript page size, visible host refs, turn/message attrs, redaction report items, lifecycle derived refs, and repair issues. Store backends persist and page data; they do not own profile budget policy. |

Privacy and projection boundaries:

- Transcript evidence is not automatically a canonical fact, soul mutation, procedural skill, or task experience.
- Accepted long-term, shared factual, procedural skill, private garden, and soul-candidate handoff writes produced through governed candidate writes, manual extraction, or automatic post-turn extraction record structured transcript-derived refs for lifecycle impact review.
- Runtime recall, projection, maintenance, long-term refresh, and operator inspection use transcript-backed evidence before the legacy `SessionStore(chat_id)` shadow; if transcript content is masked, raw-deleted, or its legacy `chat_id` alias cannot be trusted, these paths fail closed instead of falling back to the session shadow.
- Assistant self-claims remain low-authority transcript evidence until governed by the relevant memory plane.
- `HostUi` replay must not expose private garden, inner-life, soul-private raw material, backend traces, or operator-only audit content.
- `ModelContext` replay must pass through privacy gates, profile budget, and model-facing projection policy.
- Host references stay opaque by default; replay can show metadata and relation, not host object payloads. `Export` returns only export-visible refs, and `ModelContext` returns only model-context refs.
- `MemoryRuntime::finalize_turn` reports both session commit and transcript commit status, so transcript backfill is not treated as a no-op when the legacy session shadow already contains the turn.

## Request Shapes

The most common SDK request types are:

| Request type | Required fields | Notes |
| --- | --- | --- |
| `MemoryWriteRequest::Procedural` | `writes`, `owning_scope`, `source` | Every item carries a `RuntimeSkillWrite`, typed creation ref, and privacy class. `name` is display input and is not owner identity. |
| `MemoryWriteRequest::AgentToolUsageFeedback` | `feedback` | Host reports tool execution observations with `registry_ref`; the SDK may turn repeated governed evidence into tool experience. |
| `MemoryWriteRequest::LongTermExtraction` | `extraction` | Use when an extraction pipeline has produced a validated long-term memory extraction. |
| `MemoryWriteRequest::GovernedEvidenceDocuments` | `mutations` | Atomically creates, revises, or deletes governed evidence owners together with source claims and derived indexes. `Upsert` carries a bounded `GovernedEvidenceDocumentDraft`; `Delete` requires an expected owner revision. |
| `MemoryRecallRequest` | `temporal_operation`, `query`, `limit`, `structured_query_facets`, `tool_registry_refs` | Returns runtime skill hits, standard Agent Skill hits, working recall inspection data, and experience-backed `agent_tool_hints`; structured facets are typed query constraints, and without governed experience tool hints are empty. |
| `MemoryProjectionRequest` | `temporal_operation`, `user_query`, `system_max_len`, `recent_messages_limit`, `pressure`, `mode_input`, `structured_query_facets`, `tool_registry_refs` | Returns `system_memory_block` bounded by `system_max_len`; structured facets use the same governed query contract as recall, standard Agent Skills enter only as read-only hint summaries, and Agent Tools enter only as experience hints without full schemas. |
| `MemoryEvidenceDocumentReadRequest` | `memory_space_id`, `document_ids` | Reads an exact bounded set of governed evidence documents through `MemoryRuntime::read_governed_evidence_documents(request)`. The runtime rejects a memory-space mismatch, empty/duplicate document ids, and requests above the current profile read budget; each result is privacy-filtered and carries typed owner identity, revision, canonical evidence binding, safe source metadata, and bounded body/chunks. |
| `MemoryInspectionRequest` | `query`, `system_max_len`, `pressure`, `mode_input` | Returns capability, lifecycle, operator inspection data, the Agent Skill directory report, and the Agent Tool registry report. |
| `RuntimeSkillListRequest` | `owning_scope`, `query`, `include_disabled`, `include_retired`, `limit` | Lists only exact typed owners bound by the explicit Subject or SharedProgram scope manifest. |
| `RuntimeSkillDetailRequest` | `locator` | The locator binds owning scope, owner ref, and expected revision. A display name is never translated into identity. |
| `RuntimeSkillEditRequest` | `locator`, `title`, `topic`, `summary`, `procedure`, `edit_reason`, `observed_at` | Uses the locator revision as the concurrency precondition, appends an immutable owner revision, and returns `current_locator`. |
| `RuntimeSkillSetEnabledRequest` | `locator`, `enabled`, `observed_at` | Appends a lifecycle revision and never writes `skill_meta`. |
| `RuntimeSkillRetireRequest` | `locator`, `observed_at` | Appends a disabled and retired revision while retaining lineage; it does not physically delete the owner. |
| `MemoryLongTermListRequest` | `query`, `limit`, `view` | Lists accepted long-term memory through `MemoryRuntime::list_long_term_memory`; supports `cursor` paging and redacts source metadata from embedded records for `HostUi` by default. |
| `MemoryLongTermDetailRequest` | `target`, `view` | Inspects one long-term memory record by record id, slot, or transcript derived ref, including revisions, tombstone data, and evidence refs. |
| `MemoryLongTermMutationRequest` | `operation`, `reason`, `dry_run`, `mode_input` | Runs correct, supersede, delete, forget_by_query, mark_stale, or change_scope. Bulk forget requires a dry-run preview plus confirmation token. |
| `MemoryLongTermPolicyRequest` | `operation`, `reason`, `dry_run`, `mode_input` | Runs pause, resume, suppress, or remove_suppression. Writes blocked by the policy appear in SDK governance reports. |
| `MemoryTranscriptAttrWriteRequest` | `memory_space_id`, `channel_id`, `conversation_id`, `attrs`, `dry_run` | Writes governed `TranscriptAttrEnvelope` metadata to existing transcript turns/messages. `idempotency_key` is accepted for host/adapter correlation; dry-run validates target existence and attr envelope rules without persisting and returns rejected attrs plus `redactions_preview`. |
| `MemoryReplayRequest` | `chat_id`, `limit` | Inspection-only replay surface. |
| `MemorySpaceExportRequest` | `scope`, `private_material_policy` | Uses `MemoryArchiveScope::subject(...)` or `MemoryArchiveScope::shared_program(...)` and returns an opaque archive with a canonical governed root. |
| `MemorySpaceImportRequest` | `scope`, `expected_private_material_policy`, `archive` | Recomputes the archive root and atomically replaces only when runtime, request, and archive have the same exact scope and private-material policy before store mutation. |
| `MemoryRecoverRequest` | `trigger`, `mode_input` | Runs recoverable lifecycle recovery. |
| `MemoryCloseRequest` | `reason` | Emits a close lifecycle report. |

Generic adapter dispatch supports write, recall, project, inspect, recover, replay, long-term list/detail/mutate/policy, transcript attr write, capabilities, and close. Governed memory-space export/import is runtime-scoped and is not exposed through the legacy free-form snapshot commands. Maintain is supported only through dispatch paths that supply `AdapterRuntimeServices` with explicit LLM/HTTP services; dispatch without services returns a structured rejection.

Transport helper crates use the shared JSON adapter decoder for their declared memory operations, while stream-only operations such as subscribe stay transport-specific. Check [Deployment Guide](deployment.md) for each protocol's route/frame/tool/message surface.

## Accepted Long-Term Memory Control

Accepted long-term memory is owned by `MemoryRuntime`. Hosts may translate user-facing natural-language commands into SDK requests, but they must not maintain a shadow memory editor in their own SQLite database, local JSON files, or UI state.

The control plane is separate from the automatic write path:

- `MemoryWriteRequest::Candidates` / `LongTermExtraction` submit candidate content for Memory-owned governance, merge, and storage.
- `MemoryLongTermMutationRequest` handles user or operator correction, supersede, delete, forget, and scope-change actions for already accepted long-term memory.
- `MemoryLongTermPolicyRequest` handles "do not remember this kind of thing again" and "pause memory updates for this scope".
- Transcript lifecycle `DeleteRaw` / `Mask` affects conversation evidence only. It reports `DerivedMemoryRef` impact, but it does not automatically delete accepted long-term memory. Revoking derived long-term memory must go through the long-term control surface.
- Runtime Skill management is limited to procedural runtime skill memory and is not the edit/retire surface for ordinary long-term memory.

Every mutation report must be audit-ready: affected records, tombstones, transcript refs, projection impact, deferred governance impact, policy decision, and lifecycle report are returned by the SDK. If a profile denies an operation, the SDK returns a structured rejection; the host must not fall back to direct local store edits.

Long-term control visibility is exposed in `MemoryCapabilityCatalog`:

```rust
let capabilities = runtime.capabilities();
assert!(capabilities.long_term_control_inspect.visible);
assert!(capabilities.long_term_control_mutation.visible);
assert!(capabilities.long_term_control_policy.visible);
```

`long_term_control_bulk_forget` is a high-risk capability. Compact or embedded profiles may expose targeted inspect/mutation/policy while hiding destructive bulk forget.

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
| `/console/skills/detail` | `POST` | Read one runtime Skill by typed owner locator. |
| `/console/skills` | `PATCH` | Append an immutable revision by typed owner locator. |
| `/console/skills/enabled` | `PATCH` | Enable or disable a runtime Skill by typed owner locator. |
| `/console/skills/retire` | `POST` | Append a retired revision by typed owner locator. |
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
assert!(capabilities.transcript_replay.visible);
```

Use the CLI to render a stable platform snapshot:

```bash
cargo run --locked -p bm-cli --bin bm --no-default-features \
  --features profile-server-linux-memory-gateway -- \
  platform capability-snapshot \
  --profile profile-server-linux-memory-gateway
```

## Boundary

External code may choose a profile, open a supported store backend, call SDK operations, and consume reports. External code must not bypass `MemoryRuntime` to write memory state or implement a parallel adapter/store path with different semantics.
