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
| Write | `MemoryRuntime::write` | Submit typed factual candidates, long-term extraction results, or governed evidence. Public writes cannot create procedural owners. |
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

## Universal Long-Term Learning

`finalize_turn` commits the delivered canonical turn and one durable governance intent. `bm-sdk::MemoryLearningEngine` then owns the complete due-job cycle: exact scope discovery, lease/CAS, current transcript/subject/privacy admission, minimum provider disclosure, strict candidate validation, accepted long-term mutation, receipt/audit, retry, blocking, cancellation, and terminal completion. Production hosts must not compose the former low-level worker transitions into a second worker.

`bm-entry::MemoryLearningService` is the official process owner for asynchronous execution. Build it with an existing `Arc<MemoryRuntime>`, one `GovernanceBindingSource`, and one `GovernanceCredentialResolver`; attach additional runtimes only when they share the exact Store authority, Subject Registry, and MemorySpace. `EntryRuntime` consumes the same service internally, so server and embedded consumers do not have separate governance state machines.

```rust,ignore
let control_authorities = governor_runtime.learning_service_control_authorities()?;
let (service, attachment) = MemoryLearningService::builder(runtime.clone())
    .control_authorities(control_authorities)
    .binding_source(binding_source)
    .credential_resolver(credential_resolver)
    .start()?;

let another_control = another_governor_runtime.learning_service_control_authorities()?;
let another = service.attach_runtime(another_runtime, another_control)?;
service.wake();
service.credential_changed("product.primary-key", 2, "credential-op-2")?;
```

Product Provider configuration remains host-owned. Beetle persists immutable non-secret execution binding snapshots as historical job authority; it never persists a raw credential. `provider_config_changed`, `credential_changed`, and `provider_permission_changed` are typed notifications that re-read the same source and use operation-aware Store receipts. They are not a second configuration payload.

Status and recovery controls require opaque SDK-minted capabilities. `MemoryRuntime::learning_service_status_authority` and `MemoryRuntime::learning_service_control_authorities` require the Runtime actor itself to be the exact active governing `SystemGovernor`; the per-operation recovery authorities are bound to the exact Store, registry, MemorySpace, mounted subject, scope, and recovery kind. `MemoryRuntime::learning_attachment_status_authority` requires the exact active mounted-subject actor. Cross-subject, cross-operation, or foreign Store/registry authorities fail before job identity, reason detail, or mutation is returned.

## Procedural Evidence Authority

The 0.8.0 source uses `PostTurnLearningInputV2`. The published 0.7.0 API is not interchangeable with this contract. Ordinary turns use `PostTurnLearningInputV2::empty()` or `with_tool_call_count(n)` and `finalize_turn`; no producer registration is required, and an empty feedback set creates no procedural job. A selection receipt proves an exact projection, not permission to report execution.

For feedback, a trusted composition root uses an active governing `SystemGovernor` runtime with the same Store, SubjectRegistry, MemorySpace and mounted scope:

1. Call `control_procedural_producer(MemoryProceduralProducerControlRequest)` with an operation id, exact `ProceduralProducerSpecV1`, `Active` state and expected predecessor revision. First registration uses `expected_revision: None`; updates and revocation use the exact existing revision. The spec declares the actual principal, provenance, allowed claims, source classifications and exact tool references. It contains no credentials.
2. Pass the returned binding revision to `procedural_submission_capability`. This issues a non-serializable `MemoryProceduralSubmissionCapability`; a persisted binding or JSON field cannot mint it.
3. Give only that restricted capability to the intended in-process producer, which calls `finalize_turn_with_procedural_evidence(&capability, request)`. The SDK revalidates Store incarnation, registry, scope, current producer authority, source visibility and canonical turn/call bindings before its atomic intake.

`EntryRuntime` exposes the same control and issuance operations for trusted process setup. Trusted ingress can pass the capability through `AdapterRuntimeServices::procedural_submission` to `EntryRuntime::handle_with_services`; Entry additionally matches the authenticated principal and owner. Generic HTTP/MCP/WSS/A2A payloads cannot supply this capability. Bearer authentication and `FinalizeTurn` permission alone do not grant evidence authority; without trusted out-of-band provisioning, feedback is rejected rather than automatically trusted. Do not persist a second grant table or register/restore a grant for every request or reopen.

Execution facts have typed outcomes and bounded call references, not arbitrary result text, error messages or summaries. Explicit method evidence has separate canonical content and source references. `Private`/`Unknown` methods cannot be made public with a hash, a sanitized label or Human confirmation. Legitimately admitted execution facts can remain accepted when a method is rejected; inspect `partially_accepted_count` and `method_dispositions` in the governed feedback receipt. Its accepted, partially accepted, deferred and rejected counts partition submitted groups, not changed owners. A transport retry must reuse the exact canonical payload; a real additional execution has a distinct call identity.

Producer restriction/revocation and governed-source withdrawal immediately fence affected current and as-of reads. The existing `MemoryLearningEngine` / `MemoryLearningService` performs bounded reconciliation in the same durable job lane; hosts must not build another worker or recompute counts. `RuntimeSkillListReport.read_availability` distinguishes `Ready`, `Reconciling`, `Blocked`, `SubjectUnavailable`, `ProfileUnavailable` and `NotMaterialized`. Optional totals are `None` when unavailable, not a fabricated zero; do not show old method bodies or statistics while fenced. Projection remains subject to the existing backend/profile support matrix.

Capacity exhaustion durably blocks reconciliation. After the resource problem is actually resolved, the governing runtime explicitly calls `resume_procedural_reconciliation` with an operation id, job id and exact expected state revision. Wake/reopen alone does not clear this block. Retryable Store contention, expected-state conflicts, authority rejection and damaged closure remain distinct typed outcomes in `ProceduralLearningSdkError`; never parse private error text to choose recovery. Permanent producer revocation or owner deletion cannot be reversed by retry, archive restore or reconciliation. A retired Runtime Skill remains retired even when retained usage contributions are recomputed.

A valid budget admission that expires or is superseded requires fresh admission and replanning through the runtime. Core exposes `RuntimeBudgetReadmissionRequired`; the procedural boundary reports `StoreUnavailable/StoreCommitRejected`, which the official worker handles with its existing bounded retry policy. This is not evidence of damaged persistent data. Payload tampering, foreign Store authority and capacity rejection are not readmission signals; never reuse a rejected admission or bypass CAS.

To discover a capacity block after a complete restart, call `procedural_reconciliation_status(MemoryProceduralReconciliationStatusRequest { authority: runtime.learning_attachment_status_authority()? })`. It returns `read_availability`, a typed `block_reason`, and an optional `recovery` containing `job_id` and `expected_state_revision`. The capability is bound to the actual mounted Runtime; callers cannot provide another subject or arbitrary job key. The result is one immutable Store snapshot, without raw jobs, source evidence or memory bodies. `MemoryLearningAttachment::status` exposes the same fresh result as `procedural_reconciliation`, so an idle worker cannot hide a persisted block. Pass the exact recovery target to the separately authorized governing Runtime only after resolving capacity. A stale target fails CAS; a recovery target does not grant mutation authority.

Only `RuntimeObservation` and `HumanUser` producer authorities may claim execution facts or usage feedback. `GovernedSource` and `ModelInferred` may provide admitted method declarations, not execution success/mismatch evidence. A later declaration cannot inherit execution contributions from a revoked or withdrawn source. Owner reference/revision capacity is a typed capacity error, not evidence of Store corruption.

## Subject Soul Provisioning

`bm-sdk` 0.8.0 exposes a host-neutral Subject Soul provisioning and lifecycle contract. Hosts submit typed intent only; Core, SDK, and Store own Soul revisions, generations, material, manifests, ledgers, audit records, events, and durable operation receipts in one transaction. Adapters, HTTP, MCP, Console, and host databases must not maintain a second Soul state or create a default personality and overwrite it later.

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
| Conversation Catalog | `MemoryConversationListRequest` / `MemoryConversationListReport` via `MemoryRuntime::list_conversations` | List Memory-owned conversations that contain governed evidence for the mounted subject, optionally scoped by channel and lifecycle. |
| Transcript Timeline | `MemoryTranscriptTimelineRequest` / `MemoryTranscriptTimelineReport` via `MemoryRuntime::query_transcript_timeline` | Read one conversation around `Latest`, `Before`, `After`, a durable anchor, sequence, UTC time, or the first visible message in a UTC range. |
| Transcript Search | `MemoryTranscriptSearchRequest` / `MemoryTranscriptSearchReport` via `MemoryRuntime::search_transcripts` | Search visible transcript text in one conversation or across the mounted subject and return governed excerpts plus anchors that feed the timeline API. |
| Transcript Activity | `MemoryTranscriptActivityRequest` / `MemoryTranscriptActivityReport` via `MemoryRuntime::query_transcript_activity` | Evaluate bounded UTC half-open ranges and return visible message counts plus first/last anchors for date navigation. |
| Transcript Lifecycle | `MemoryTranscriptLifecycleRequest` / `MemoryTranscriptLifecycleReport` via `MemoryRuntime::request_transcript_lifecycle` | Archive, mask, delete raw content, or run lifecycle review with audit output. |
| Transcript Repair | `MemoryTranscriptRepairRequest` / `MemoryTranscriptRepairReport` via `MemoryRuntime::repair_transcript` | Inspect broken Memory-owned evidence links without scanning host business databases. |
| Transcript Attr Write | `MemoryTranscriptAttrWriteRequest` / `MemoryTranscriptAttrWriteReport` via `MemoryRuntime::record_transcript_attrs` | Write turn/message `TranscriptAttrEnvelope` records after the transcript target exists. This is for lightweight metadata such as per-message model usage, runtime latency/status, attachment summaries, and provenance tags. |
| Transcript Export | `MemoryTranscriptExportRequest` / `MemoryTranscriptExportReport` via `MemoryRuntime::export_transcript`; `MemorySpaceExportRequest { private_material_policy: MemorySpacePrivateMaterialPolicy::ExcludePrivate, .. }` excludes private transcript material and its dependent export-visible indexes as one governed closure | Export a redacted transcript slice, and keep private transcript material out of public memory-space archives unless the caller explicitly selects `IncludePrivate`. |

`MemoryTranscriptReplayRequest` and `MemoryTranscriptExportRequest` take `limit` plus optional `cursor`; their reports return `next_cursor` and `has_more`. SDK callers should page through transcript replay/export through `MemoryRuntime` instead of reaching into the core/store trait. Runtime profile budgets may clamp page size, visible host refs per turn, visible attrs per turn/message, redaction items, lifecycle derived refs, and repair issues, but they do not relax redaction, lifecycle, or privacy policy. Lifecycle and repair reports set `profile_budget_applied=true` when those report lists are clipped.

CTQ1 query surfaces use the Store-owned `TranscriptQueryCursor`. Treat it as opaque: do not decode, mint, sign, persist claims, or inject host cursor authority. Cursor validation binds the operation, exact MemorySpace, mounted subject, filters, view, query digest, direction/anchor, Store incarnation, and owner/index generation. Every page re-runs capability, subject, lifecycle, privacy, and disclosure checks. Catalog and timeline remain governed by `transcript_replay`; indexed search and activity additionally require their own `transcript_search` and `transcript_activity` capability switches. Platform capability snapshots expose these switches in `beetle-memory.platform.capability.v5`.

`HostUi` is only the host-presentable redacted disclosure view. It is not a chat-window API, pagination direction, product name, transcript index owner, or authorization token. Catalog, timeline, search, and activity return only candidates that remain visible after Runtime hydration and redaction for the requested view. Search hits contain a governed Unicode-safe excerpt and durable `TranscriptAnchor`; pass that anchor to `TranscriptTimelineAnchor::Around` instead of asking the host UI to scan or re-match transcript text.

0.8.0 accepts Store v14 only; 0.7.0 uses Store v13. There is no public Store migration API, compatibility reader, dual write, or automatic migration. Older, partial, and foreign schema payloads fail closed. Development data from an older generation must be explicitly discarded and recreated by its owner; archive export/import is not schema migration.

Timeline supports latest, before, after, around-anchor, around-sequence, around-time, and first-visible-in-range queries. Page turns stay in sequence order and reports may carry opaque older/newer cursors. Calendar conversion stays with the host: resolve the user's IANA time zone and local date into a canonical UTC `[start_inclusive, end_exclusive)` range before calling Memory. Do not assume every local day is 86,400 seconds; DST days can be 23 or 25 hours. Beetle Memory does not store or guess the host time zone.

The current working source keeps the CTQ1 public query shapes and capability snapshot v5. Store v14 strengthens procedural evidence authority and contribution/reconciliation closure; it does not add a second transcript query model. Local automated contracts do not establish real-data, Provider, GUI/UAT, crates.io or hosted Release evidence.

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
- Accepted long-term, shared factual, private garden, soul-candidate handoff, and governed post-turn procedural writes record structured transcript-derived refs for lifecycle impact review.
- Runtime recall, projection, maintenance, long-term refresh, and operator inspection use transcript-backed evidence before the legacy `SessionStore(chat_id)` shadow; if transcript content is masked, raw-deleted, or its legacy `chat_id` alias cannot be trusted, these paths fail closed instead of falling back to the session shadow.
- Assistant self-claims remain low-authority transcript evidence until governed by the relevant memory plane.
- `HostUi` replay must not expose private garden, inner-life, soul-private raw material, backend traces, or operator-only audit content.
- `ModelContext` replay must pass through privacy gates, profile budget, and model-facing projection policy.
- Host references stay opaque by default; replay can show metadata and relation, not host object payloads. `Export` returns only export-visible refs, and `ModelContext` returns only model-context refs.
- `MemoryRuntime::finalize_turn` reports both session and transcript commit status. Transcript owner identity, turn id, and canonical digest determine retries and conflicts; matching text in the session shadow never proves a turn was committed and is not used to backfill a new turn.

`CanonicalTurnDelta.input_messages` contains only the current turn's inputs. For a full protocol history, decode messages and use `bm_sdk::protocol_window_user_delta(&messages)` before intake: it selects nonempty user messages after the last assistant entry, including an empty assistant/tool-call boundary. Without an assistant boundary, all user entries form the current unanswered group. A tool continuation with no new user has no new user input. Do not compare message text against stored history: different turn ids preserve repeated text as distinct evidence. Retry the same turn with its original id and canonical payload; a changed payload conflicts. The former Core session-only `commit_canonical_turn_delta` and text-overlap `canonical_user_delta` helpers have been removed; SDK hosts use `finalize_turn` or `commit_transcript`.

## Request Shapes

The most common SDK request types are:

| Request type | Required fields | Notes |
| --- | --- | --- |
| `MemoryWriteRequest::Candidates` | `candidates` | Submits typed factual candidates. Procedural targets are rejected on the public write surface; Runtime Skill and Agent Tool experience can only be created by governed post-turn learning. |
| `MemoryWriteRequest::LongTermExtraction` | `extraction` | Use when an extraction pipeline has produced a validated long-term memory extraction. |
| `MemoryWriteRequest::GovernedEvidenceDocuments` | `mutations` | Atomically creates, revises, or deletes governed evidence owners together with source claims and derived indexes. `Upsert` carries a bounded `GovernedEvidenceDocumentDraft`; `Delete` requires an expected owner revision. |
| `MemoryRecallRequest` | `temporal_operation`, `query`, `limit`, `structured_query_facets`, `tool_registry_refs` | Returns runtime skill hits, standard Agent Skill hits, working recall inspection data, and experience-backed `agent_tool_hints`; structured facets are typed query constraints, and without governed experience tool hints are empty. |
| `MemoryProjectionRequest` | `binding`, `temporal_operation`, `user_query`, `system_max_len`, `recent_messages_limit`, `pressure`, `mode_input`, `structured_query_facets`, `tool_registry_refs` | `Preview` performs a read-only projection. `Turn { turn_id }` binds a current execution projection and its signed selection receipt to the exact turn later passed to `finalize_turn`. |
| `MemoryTurnFinalizeRequest` | `turn`, `learning`, `pressure`, `mode_input` | Atomically commits the canonical turn and durable post-turn intent. `learning: PostTurnLearningInputV2` carries the optional signed selection receipt and typed feedback; reporting authority is a separate non-wire capability. Callers cannot write procedural owners directly. |
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
| `MemoryConversationListRequest` | `channel_id`, `lifecycle`, `limit`, `cursor`, `view` | Lists visible evidenceful conversations for the runtime's mounted subject. `TranscriptCatalogLifecycle` controls active-only versus active-and-archived results. |
| `MemoryTranscriptTimelineRequest` | `channel_id`, `conversation_id`, `anchor`, `limit`, `cursor`, `view` | Queries a single conversation with `TranscriptTimelineAnchor`; search/activity anchors can return directly to this timeline. |
| `MemoryTranscriptSearchRequest` | `scope`, `query_text`, `sort`, `lifecycle`, `limit`, `cursor`, `view` | Runs governed transcript text search for the mounted subject or an exact conversation. Empty, punctuation-only, malformed, or over-budget queries fail before Store search. |
| `MemoryTranscriptActivityRequest` | `channel_id`, `conversation_id`, `ranges`, `lifecycle`, `view` | Evaluates sorted, non-overlapping UTC half-open ranges and returns visible counts and first/last `TranscriptAnchor` values. |
| `MemoryTranscriptAttrWriteRequest` | `memory_space_id`, `channel_id`, `conversation_id`, `attrs`, `dry_run` | Writes governed `TranscriptAttrEnvelope` metadata to existing transcript turns/messages. `idempotency_key` is accepted for host/adapter correlation; dry-run validates target existence and attr envelope rules without persisting and returns rejected attrs plus `redactions_preview`. |
| `MemoryReplayRequest` | `chat_id`, `limit` | Inspection-only replay surface. |
| `MemorySpaceExportRequest` | `scope`, `private_material_policy` | Uses `MemoryArchiveScope::subject(...)` or `MemoryArchiveScope::shared_program(...)` and returns an opaque archive with a canonical governed root. |
| `MemorySpaceImportRequest` | `scope`, `expected_private_material_policy`, `archive` | Recomputes the archive root and atomically replaces only when runtime, request, and archive have the same exact scope and private-material policy before store mutation. |
| `MemoryRecoverRequest` | `trigger`, `mode_input` | Runs recoverable lifecycle recovery. |
| `MemoryCloseRequest` | `reason` | Emits a close lifecycle report. |

`Preview` never mints a selection receipt and must not be used as an execution
identity. For an actual execution, the host chooses one stable turn id at request
ingress, uses `Turn { turn_id }`, keeps the returned receipt intact, and submits it
with the same canonical turn to `finalize_turn`. Retries of that turn reuse the id
and receipt; a later turn receives a new id even when its text is identical. Hosts
must not reconstruct a receipt from selected ids, digests, or logs.

Generic adapter dispatch supports write, recall, project, inspect, recover, replay, long-term list/detail/mutate/policy, transcript attr write, capabilities, and close. Governed memory-space export/import is runtime-scoped and is not exposed through the legacy free-form snapshot commands. Maintain is supported only through dispatch paths that supply `AdapterRuntimeServices` with explicit LLM/HTTP services; dispatch without services returns a structured rejection.

`governed_adapter_json_command_schema` is a top-level transport discovery schema,
not a second schema for nested Core requests. Fields such as `turn`, `learning`,
`mode_input`, and registry refs remain shallow objects there; the strict typed serde
decoder is authoritative and rejects missing or unknown nested fields. Consumers
must use the exported SDK request types rather than treating the discovery schema as
a complete payload generator.

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

For an actual turn, `/memory/project` uses `binding: {"kind":"turn","turn_id":"..."}`.
The host executes tools, then submits the same canonical turn id and the returned signed
`selection_receipt`. Feedback through `/memory/finalize-turn` additionally requires trusted
ingress to supply the matching non-wire producer capability; the generic route does not grant one.
`/memory/write` does not accept tool feedback or procedural owner creation.
Typed execution facts have no free-text summary/result slot. Method evidence follows separate source and privacy admission.

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
