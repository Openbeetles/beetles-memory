# Integration Guide

This guide describes how to embed Beetle Memory into a Rust project through `bm-sdk`.

## 1. Choose A Profile

Choose the profile that matches the deployment target and runtime role:

| Use case | Profile feature | `ProfileId` |
| --- | --- | --- |
| Beetle Memory macOS standalone desktop app | `profile-desktop-macos-standalone-memory` | `ProfileId::DesktopMacosStandaloneMemory` |
| Rust desktop host on macOS | `profile-desktop-macos-embedded-sdk` | `ProfileId::DesktopMacosEmbeddedSdk` |
| Rust desktop host on Windows | `profile-desktop-windows-embedded-sdk` | `ProfileId::DesktopWindowsEmbeddedSdk` |
| Rust desktop host on Linux | `profile-desktop-linux-embedded-sdk` | `ProfileId::DesktopLinuxEmbeddedSdk` |
| Linux hardware device runtime | `profile-linux-device-standalone-memory` | `ProfileId::LinuxDeviceStandaloneMemory` |
| Linux server memory gateway | `profile-server-linux-memory-gateway` | `ProfileId::ServerLinuxMemoryGateway` |
| ESP embedded SDK host | `profile-esp-embedded-sdk` | `ProfileId::EspEmbeddedSdk` |
| ESP standalone memory runtime | `profile-esp-standalone-memory` | `ProfileId::EspStandaloneMemory` |

## 2. Add Dependencies

From this repository:

```toml
[dependencies]
bm-sdk = { path = "crates/sdk", features = ["profile-desktop-macos-embedded-sdk"] }
```

After the crates are published:

```toml
[dependencies]
bm-sdk = { version = "0.3.0", features = ["profile-desktop-macos-embedded-sdk"] }
```

Use exactly one profile feature for a build. Linux desktop, Linux device, and Linux server are distinct deployment targets; do not substitute one for another.

## 3. Open A Store

For tests and short-lived sessions:

```rust
use bm_sdk::{MemoryStoreHandle, ProfileId, StoreBackendConfig};

let profile = ProfileId::DesktopMacosEmbeddedSdk;
let store = MemoryStoreHandle::open(StoreBackendConfig::in_memory(profile)?)?;
```

For durable desktop or server storage:

```rust
let store = MemoryStoreHandle::open(StoreBackendConfig::file(
    "/var/lib/beetle-memory",
    ProfileId::ServerLinuxMemoryGateway,
)?)?;
```

For sqlite-backed storage:

```rust
let store = MemoryStoreHandle::open(StoreBackendConfig::sqlite(
    "/var/lib/beetle-memory/memory.sqlite3",
    ProfileId::ServerLinuxMemoryGateway,
)?)?;
```

ESP profiles should use `StoreBackendConfig::embedded(profile)` or `in_memory(profile)`.

## 4. Build The Runtime

```rust
use bm_sdk::{AgentSkillDirConfig, MemoryIdentity, MemoryRuntime, MemoryScope};

let runtime = MemoryRuntime::builder()
    .identity(MemoryIdentity::new("agent-main", "owner-default")?)
    .scope(MemoryScope::new("local", "chat-1")?)
    .store(store)
    .add_agent_skill_dir(AgentSkillDirConfig::read_only("./skills", "host-project"))
    .build()?;
```

`agent_id` identifies the agent instance. `owner_id` identifies the owner or tenant. Normal single-agent hosts do not pass `subject_id`: the SDK creates `space:<owner_id>` and the default `agent:<agent_id>` subject automatically, while hiding the `system_governor` / `human_user` / relationship graph details. Only advanced multi-subject hosts configure a custom subject registry, relationship graph, or mounted subject. `channel` and `chat_id` define the default memory scope for runtime operations.

`add_agent_skill_dir` is optional and read-only. The host still owns standard Agent Skill add/edit/import/delete/execute flows; Beetle Memory only scans `SKILL.md` summaries for recall and projection.

## 5. Write Memory

Procedural memory is the current direct write path for reusable runtime knowledge:

```rust
use bm_sdk::{MemoryWriteRequest, RuntimeSkillWrite, RuntimeSkillWriteSource};

let report = runtime.write(MemoryWriteRequest::Procedural {
    writes: vec![RuntimeSkillWrite {
        name: "release_guard".to_string(),
        topic: "release".to_string(),
        title: "Release guard".to_string(),
        summary: "Verify release artifacts before publishing.".to_string(),
        content: "Run examples, platform gates, and publish dry-run.".to_string(),
        citations: vec!["integration-guide".to_string()],
        source_chat_id: Some("chat-1".to_string()),
        observed_at: 1_800_000_000,
    }],
    source: RuntimeSkillWriteSource::Manual,
})?;

assert!(report.accepted);
```

Long-term extraction writes should be produced by the extraction pipeline and passed through `MemoryWriteRequest::LongTermExtraction`.

## 6. Recall And Project

```rust
use bm_sdk::{
    MemoryProjectionRequest, MemoryRecallRequest, MemoryRecallTemporalOperation, PressureLevel,
    RuntimeLifecycleModeInput,
};

let recall = runtime.recall(MemoryRecallRequest {
    temporal_operation: MemoryRecallTemporalOperation::Current,
    query: "release artifacts".to_string(),
    limit: 4,
    structured_query_facets: Vec::new(),
    tool_registry_refs: Vec::new(),
})?;

let projection = runtime.project(MemoryProjectionRequest {
    temporal_operation: MemoryRecallTemporalOperation::Current,
    user_query: "How should this host release?".to_string(),
    system_max_len: 4096,
    recent_messages_limit: 8,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
    structured_query_facets: Vec::new(),
    tool_registry_refs: Vec::new(),
})?;

let memory_block = projection.system_memory_block;
```

Use the projected memory block as part of your model-context assembly. Keep your host prompt assembly responsible for final ordering with system, developer, user, and tool messages.

## 7. Maintain With Explicit LLM Injection

`MemoryRuntime::maintain` is available for hosts that configure an LLM client. Generic adapters reject maintain because they cannot safely invent the LLM/HTTP boundary for the application.

```rust
let capabilities = runtime.capabilities();
if capabilities.lifecycle.maintain_lightweight.visible {
    // Call runtime.maintain(...) from the host path that owns LLM injection.
}
```

## 8. Submit Memory Candidates, Not Store Mutations

Hosts should submit candidate facts or procedures and let Beetle Memory decide
which memory plane may change. This keeps SDK, HTTP, gateway, and future hosts
on the same memory-governance contract.

```rust
use bm_sdk::{
    LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateSemanticDecision,
    MemoryCandidateSemanticJudgment, MemoryCandidateTarget, MemoryEvidenceAuthority,
    MemoryPrivacyClass, MemorySemanticJudgmentSource, MemorySubjectVisibilityPolicy,
    MemoryWriteCandidate, MemoryWriteRequest,
};

runtime.write(MemoryWriteRequest::Candidates {
    runtime_skill_owning_scope: None,
    candidates: vec![MemoryWriteCandidate {
        candidate_id: "turn-1:preferred-name".to_string(),
        authority: MemoryEvidenceAuthority::UserAsserted,
        target: MemoryCandidateTarget::LongTermMemory {
            kind: LongTermMemoryKind::Profile,
            topic: "preferred_name".to_string(),
        },
        long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
        privacy: MemoryPrivacyClass::SharedWithSubject,
        content: MemoryCandidateContent::Text {
            topic: "preferred_name".to_string(),
            body: "The user prefers to be called Qingchuan.".to_string(),
            keywords: vec!["name".to_string()],
        },
        evidence_refs: vec!["chat-1:turn-1".to_string()],
        canonical_entities: Vec::new(),
        semantic_judgment: Some(MemoryCandidateSemanticJudgment {
            source: MemorySemanticJudgmentSource::RuntimeGate,
            decision: MemoryCandidateSemanticDecision::Accept,
            governed_target: None,
            reason: "explicit user statement accepted by the runtime gate".to_string(),
        }),
    }],
})?;
```

If post-turn LLM services are unavailable, `finalize_turn` still
commits the transcript and atomically creates a V2 intent in the StorePlatform
`post_turn_governance_jobs` and `post_turn_governance_scope_indexes` typed
namespaces. Once services recover, the SDK first acquires an exact lease through
`MemoryRuntime::claim_governance_job`, then calls
`MemoryRuntime::run_claimed_governance`. The latter revalidates transcript and
privacy authority before the first network byte and commits the memory
post-image, `succeeded` job, scope index, and receipt in one backend transaction.
The legacy `memory/governance_jobs/pending.json` payload is never read or
migrated; a non-empty legacy file fails with
`legacy_governance_queue_reset_required` and requires explicit operator action.
Hosts must not reimplement the queue, assemble memory mutations, or retry with
host-owned semantics.
If a canonical transcript commit exists but intent admission was interrupted,
call `MemoryRuntime::reconcile_governance_intents` with a bounded page of 1 to
32 turns for the current exact conversation. Its cursor and repaired intents
advance in one CAS and never scan another subject or conversation.
Operator surfaces should use `MemoryRuntime::deferred_governance_report()` or
`inspect.deferred_governance` for pending / retrying / failed / terminal counts,
recent jobs, scope, subject, turn, reason, and last error for the current runtime
scope.

`project()` returns `MemoryProjectionReport.audit` as the projection diagnostic
source of truth. It includes source planes, selected ids, section chars,
source/render budgets, scope, and private gate decisions. Hosts may display
these fields, but must not infer projection behavior by reading store internals.

For conservative compaction, call `MemoryRuntime::run_retention_compaction()`.
It only runs SDK-owned hygiene, factual evidence metadata compaction, and runtime
skill governance, and reports `host_direct_deletion_allowed=false`; quota
pressure must not let hosts delete accepted memory.

## 9. Manage Accepted Long-Term Memory

When users later ask to inspect, correct, delete, forget, or restrict long-term memory, hosts should call the long-term memory control surface. Hosts may own natural-language command interpretation and UI display, but they must not maintain a shadow memory editor in their own local database.

```rust
use bm_sdk::{
    MemoryLongTermControlView, MemoryLongTermListRequest, MemoryLongTermMutation,
    MemoryLongTermMutationRequest, MemoryLongTermTarget, LongTermMemoryQuery,
    RuntimeLifecycleModeInput,
};

let page = runtime.list_long_term_memory(MemoryLongTermListRequest {
    query: LongTermMemoryQuery {
        topic: Some("preferred_editor".to_string()),
        limit: 8,
        ..LongTermMemoryQuery::default()
    },
    cursor: None,
    limit: 8,
    view: MemoryLongTermControlView::HostUi,
})?;

if let Some(record) = page.records.first() {
    let report = runtime.mutate_long_term_memory(MemoryLongTermMutationRequest {
        operation: MemoryLongTermMutation::Delete {
            target: MemoryLongTermTarget::RecordId(record.record.id.clone()),
        },
        reason: "user requested deletion".to_string(),
        dry_run: false,
        mode_input: RuntimeLifecycleModeInput::default(),
    })?;
    assert!(report.accepted);
}
```

Hosts cannot declare confirmation on `LongTermMemoryDraft`. Ordinary create, upsert,
extraction, Adapter metadata, citations, and observer logs remain unconfirmed. Every
explicit `MemoryLongTermMutation::Correct` creates immutable typed correction evidence,
including when the replacement text is unchanged. Confirmation is separate: the SDK adds
typed human-confirmation evidence only when the exact actor is an active `HumanUser` in
the current `SubjectRegistry`. Corrections by an `AgentPersona`, `SystemGovernor`, or a
suspended human remain unconfirmed. Both evidence records bind the exact memory space,
actor, predecessor/successor, time, and same-transaction control revision. `Supersede`
clears confirmation instead of transferring it to another owner.

### Durable mutation results (Adapter V2)

Adapter V2 gives `Write` and `LongTermMutate` a Store-owned durable operation receipt.
The first successful execution returns `Accepted` with a receipt. Retrying the same
operation identity with the same canonical intent returns terminal-success `Replayed`
with that same receipt and does not add another effect, revision, audit, or event. Reusing
the identity for a different intent is a conflict.

Durable `Write` and `LongTermMutate` requests must provide a stable, non-sensitive
idempotency key through the transport. The Adapter hashes that key before persistence and
rejects a durable mutation when the key is absent; it never substitutes a one-shot generated
identity that the caller could not reproduce after a lost response. Adapter V1 accepts reads
only. Other V2 mutations are explicitly non-durable (or have a separate domain-owned
receipt); inspect both the Adapter capability report and the SDK mutation inventory instead
of assuming global exactly-once behavior. Receipts remain pinned until Store capacity is
exhausted; the Store then fails the whole mutation closed and never evicts an older receipt
silently.

Bulk `forget_by_query` must run a dry-run preview before execution and then pass the confirmation token. Use `MemoryLongTermPolicyRequest` for "do not remember this kind of thing again" or pausing future long-term memory updates for a scope; policies do not retroactively delete accepted records.

Transcript lifecycle raw delete/mask only affects conversation evidence. It reports affected `DerivedMemoryRef` values, but revoking the corresponding long-term memory still goes through `mutate_long_term_memory`. Runtime Skill edit/retire only manages procedural runtime skill memory and is not the control surface for ordinary long-term memory; retire appends a terminal revision and preserves owner lineage.

## 10. Host Turn Lifecycle

A complete SDK host turn uses one public path:

1. Open a `MemoryStoreHandle` and pass it through `MemoryRuntime::builder().store(...)`; persistence engines, raw transactions, and writable store traits are not public runtime paths.
2. Build `MemoryIdentity` and `MemoryScope` from stable host owner, agent, channel, and conversation ids.
3. Submit `MemoryWriteRequest::Candidates` for facts, preferences, procedures, diagnostics, subject hints, and soul candidates.
4. Finalize the turn through canonical turn semantics when transcript governance is required.
5. Use `recall` and `project` to build model context; do not assemble memory planes in the host.
6. Use `inspect` for operator visibility and safe recovery context.
7. Use typed memory-space export, direct same-scope import, and replay for replacement or release gates.

Generic host fixtures and Beetle-derived fixtures under `fixtures/sdk-host-readiness/` follow this same path. Beetle-derived data is current-contract host evidence only, not a special SDK or compatibility branch.

## 11. Archive Import And Replay

```rust
use bm_sdk::{
    MemoryArchiveScope, MemoryReplayRequest, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy,
};

let replay = runtime.replay(MemoryReplayRequest {
    chat_id: "chat-1".to_string(),
    limit: 32,
})?;

let scope = MemoryArchiveScope::subject(
    runtime.memory_space_id(),
    runtime.subject_id(),
)?;
let private_material_policy = MemorySpacePrivateMaterialPolicy::IncludePrivate;
let exported = runtime.export_memory_space(MemorySpaceExportRequest {
    scope: scope.clone(),
    private_material_policy,
})?;

assert_eq!(&exported.archive.root().scope, &scope);
target_runtime.import_memory_space(MemorySpaceImportRequest {
    scope,
    expected_private_material_policy: private_material_policy,
    archive: exported.archive,
})?;
```

Public recovery never accepts a free-form continuity snapshot. The source and target runtimes, request, and opaque archive root must declare the exact same `MemoryArchiveScope` and private-material policy before replacement begins.

The archive root is the portable integrity report. Inspect its schema id/version, exact scope, private-material policy, JSON/event counts and byte counts, and canonical `closure_sha256`. Import recomputes that root before any backend mutation and atomically replaces only the declared scope. Bootstrap and full continuity modes belong only to the internal Soul-recovery bundle.

## 11. Operator Inspect

```rust
use bm_sdk::{MemoryInspectionRequest, PressureLevel, RuntimeLifecycleModeInput};

let inspect = runtime.inspect(MemoryInspectionRequest {
    query: "archive readiness".to_string(),
    system_max_len: 4096,
    pressure: PressureLevel::Normal,
    mode_input: RuntimeLifecycleModeInput::default(),
})?;

assert!(inspect.capabilities.inspection.visible);
```

Operator inspect is the supported path for selected ids, plane evidence,
capability visibility, deferred governance queue state, lifecycle diagnosis, and
safe actions. A host UI may display this report, but it must not infer write
decisions, replay state, or projection contents from private store files.

## 12. Host Forbidden Zones

Hosts must not:

- write memory plane files directly;
- decide plane routing outside `MemoryRuntime`;
- maintain a second long-term extraction, subject, soul, private garden, or procedural write policy;
- build memory projection by reading store internals;
- treat Beetle, an IDE, Ollama, or a device channel as a kernel source kind;
- swallow deferred governance jobs or retry them with host-owned semantics;
- keep compatibility fields that pollute the current SDK contract.

## 13. Check Capabilities Before Exposing UI Or Tools

```rust
let catalog = runtime.capabilities();
if catalog.adapter.http.visible {
    // It is safe for this profile/policy/privacy combination to expose HTTP.
}
```

Do not expose a protocol or operation just because the crate compiles. The capability catalog is the runtime truth.

## 14. Suggested Host Tests

Add a smoke test in the integrating project that:

1. Opens the selected backend through `MemoryStoreHandle`.
2. Builds `MemoryRuntime` through `MemoryRuntime::builder().store(handle)`.
3. Writes one `MemoryWriteCandidate` and checks the governance report.
4. Finalizes one turn with maintenance unavailable and verifies a deferred job.
5. Checks `deferred_governance_report()` and `inspect.deferred_governance`.
6. Recalls or projects the candidate-backed memory from a different chat and checks `MemoryProjectionReport.audit`.
7. Calls `run_retention_compaction()` and verifies that host deletion of accepted memory is not allowed.
8. Exports an opaque archive, imports it into a runtime with the same exact typed scope and policy, and checks the governed archive root.
9. Runs operator inspect and replay against the replaced scope.
