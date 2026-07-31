# 回放与归档

Replay 和受治理 archive 是验证与连续性工具。它们不替代正常的 write、recall、project 或 maintain 路径。

## 受治理的 Memory-space Export And Import

```rust
use bm_sdk::{
    MemoryArchiveScope, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy,
};

let scope = MemoryArchiveScope::subject(
    runtime.memory_space_id(),
    runtime.subject_id(),
)?;
let private_material_policy = MemorySpacePrivateMaterialPolicy::ExcludePrivate;
let exported = runtime.export_memory_space(MemorySpaceExportRequest {
    scope: scope.clone(),
    private_material_policy,
})?;

let imported = runtime.import_memory_space(MemorySpaceImportRequest {
    scope,
    expected_private_material_policy: private_material_policy,
    archive: exported.archive,
})?;
```

request scope 必须与 runtime 当前挂载的 `(memory_space_id, mounted_subject_id)` 精确一致，archive 也必须声明相同 typed scope 与 private-material policy。三者在任何 store read、import planning 或 replacement 之前完成校验。Continuity snapshot 只作为 Soul recovery 的内部载荷，不再是 SDK 公开传输格式。

## Replay Inspection

```rust
use bm_sdk::MemoryReplayRequest;

let replay = runtime.replay(MemoryReplayRequest {
    chat_id: "chat-1".to_string(),
    limit: 32,
})?;
```

Replay 用来解释历史 continuity state，适合 inspection、archive validation 和 release gates。

## Redacted Transcript Replay

Conversation Transcript replay 是 Memory Evidence System 当前的 evidence-facing replay surface。它和现有 `MemoryReplayRequest` 分开：后者仍是基于 legacy `chat_id` scope 的 turn-ledger inspection surface。

Transcript replay 合同使用 `ConversationKey`：

```rust
pub struct ConversationKey {
    pub memory_space_id: String,
    pub channel_id: String,
    pub conversation_id: String,
}
```

发布面 replay 视图：

| View | 消费者 | 边界 |
| --- | --- | --- |
| `RawOwnerOnly` | Runtime-owned governance 和 repair path | 仅内部使用，不是普通宿主或模型 payload。 |
| `ModelContext` | 面向模型的 projection | 经过预算和隐私过滤，不含 backend trace、operator-only audit 或 raw tool payload。 |
| `HostUi` | 宿主展示面 | 只返回已脱敏 conversation evidence，不暴露 private garden、inner-life 或 soul-private raw material。 |
| `OperatorAudit` | 诊断与合规检查 | 默认返回结构化原因、ref 和 audit marker，不返回完整 raw content。 |
| `Export` | 受治理归档与可携带性 | 受 `MemorySpacePrivateMaterialPolicy`、profile、permission 和 retention policy 控制。 |

删除或 mask raw transcript content 时，report 必须把 downstream impact 和已接受的 long-term、shared factual、procedural、private、soul-related memory planes 分开说明。Lifecycle report 通过 SDK 的 operator-audit view 暴露 host refs，因此 internal/model-only refs 和 raw labels 在离开 runtime 前会被脱敏。Redacted replay 必须 fail closed：某个视图不能证明字段可见时，返回 redaction marker 和 audit reason，而不是 raw content。SDK runtime consumer 会优先使用 transcript-backed evidence，而不是 legacy session shadow；transcript 被 mask 或 legacy alias 不可信时，不会再从 `SessionStore(chat_id)` 回填原文。

SDK transcript replay/export request 通过 `cursor`、`next_cursor` 和 `has_more` 支持有界分页。Host ref visibility 会按 view 执行，host ref 的 `label` 只在 owner 允许视图中保留，其他视图会做字段级脱敏，并在 redaction report 中记录 `HostRefLabel`。

Transcript attrs 会跟随 target turn/message 一起 replay。`TranscriptAttrEnvelope` 只用于模型用量、latency、retry status、附件摘要、provenance 标签等轻量 metadata；它不替代宿主拥有的 task、capability call、artifact、human gate、file workspace 或 governance command/report 本体。`HostUi` 只看到 HostUi-visible attrs，`ModelContext` 只看到 model-context attrs，`Export` 只看到 export-visible 且 `export_allowed=true` 的 attrs。Profile budget 可以裁剪每 turn/message 可见 attrs，并在 `TranscriptRedactionReportItem` 中用 `AttrValueBudget`、`attr_id`、`attr_key` 记录；当裁剪来自 profile ceiling 时，replay audit 也会记录 `ProfileBudget`。

`HostUi` transcript replay 由 SDK `transcript_replay` capability 控制。桌面和 embedded SDK 宿主可以提交 transcript turn 后，把同一个 conversation 读回给 UI 展示；这不要求打开 `MemoryRuntime::replay`、仅开发验收的 `nonproduction-replay-harness`、raw owner replay 或 deep inspection。

`TranscriptLifecycleReport.derived_memory_refs` 可以作为下一步长期记忆控制的 target 来源。比如 raw transcript 被 mask 或 delete 后，report 会列出受影响的 `DerivedMemoryRef`；宿主或 operator 若要撤销某条 accepted long-term memory，应把对应 `DerivedMemoryRef` 传给 `MemoryLongTermTarget::TranscriptDerivedRef`，再调用 `MemoryRuntime::mutate_long_term_memory`。Transcript lifecycle 不会自动级联删除 accepted long-term memory、shared fact、procedural skill、private garden 或 soul handoff。

## 受治理的 Archive Replacement

替换同一个精确 typed memory-space projection 时，使用直接同 scope import：

```rust
use bm_sdk::{
    MemoryArchiveScope, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy,
};

let scope = MemoryArchiveScope::subject(
    source_runtime.memory_space_id(),
    source_runtime.subject_id(),
)?;
let private_material_policy = MemorySpacePrivateMaterialPolicy::ExcludePrivate;
let exported = source_runtime.export_memory_space(MemorySpaceExportRequest {
    scope: scope.clone(),
    private_material_policy,
})?;

assert_eq!(&exported.archive.root().scope, &scope);
assert_eq!(
    exported.archive.root().private_material_policy,
    private_material_policy,
);

target_runtime.import_memory_space(MemorySpaceImportRequest {
    scope,
    expected_private_material_policy: private_material_policy,
    archive: exported.archive,
})?;
```

source 与 target runtime 必须暴露完全相同的 `MemoryArchiveScope`。request scope、archive root scope 与 private-material policy 会在 replacement 前精确校验。Import 会在任何 backend mutation 之前重算 canonical archive root，并且只原子替换该 scope。

`ExcludePrivate` 会按受治理 owner closure 成组排除 private material。policy 不一致、dependency closure 不完整、root 不一致或 scope 不一致时，都会在任何写入前 fail closed。Opaque archive 不公开 payload；调用方通过 `GovernedScopeArchiveRootV1` 读取 schema、精确 scope、policy、JSON/event count 与 byte count，以及 canonical `closure_sha256`。

当 memory-space storage 中存在 transcript evidence 时，`ExcludePrivate` 会把 private transcript material 及其依赖的 `conversation_transcript_attr`、`conversation_transcript_derived_ref` 等 export-visible index 作为一个经过验证的 closure 成组排除。Archive diagnostics 必须保留 raw transcript、redacted transcript slice、accepted memory planes、derived refs 和 opaque host refs 的分层。宿主对象 payload 不由 Beetle Memory 导出；只有在请求 view 允许时才携带 `HostOpaqueRef` metadata 和 relation。`RedactedTranscriptSlice` 会报告 message、attr 和 host-ref redactions，让调用方知道哪些内容被省略，但看不到 raw material。`TranscriptLifecycleReport.derived_memory_refs` 是复核从受影响 transcript evidence 派生出的已接受 Memory material 的清单。

Transcript replay 和 archive tooling 可以用 `TranscriptTurnPage` 做有界分页。`MemoryTranscriptRepairRequest` 提供 SDK 层 transcript repair inspection，`TranscriptRepairReport` 会把 Memory-owned derived refs 和 transcript source turns/messages 对齐检查；source turn 缺失、`MissingSourceMessage`、orphan derived refs、corrupt transcript records、mismatched source keys 和 duplicate sequence/cursor evidence 都必须以 fail-closed repair issue 报告，而不是返回看似干净的空影响结果。

Transcript attr repair 也是同一个 fail-closed inspection surface 的一部分。`MissingAttrTargetTurn`、`MissingAttrTargetMessage`、attr source key 不匹配、非法 attr key、超限 attr value、非法 attr visibility、corrupt transcript attr record 都必须报告，不能静默丢 metadata。

Compact profile 可以按 `TranscriptGovernanceBudget` 裁剪 transcript turns、host refs、attrs、redaction report items、lifecycle derived refs 或 repair issues 数量。Replay audit 会在 replay redactions 受预算裁剪时记录 `ProfileBudget`；lifecycle 和 repair report 的列表被裁剪时会设置 `profile_budget_applied=true`。这只是数量裁剪：profile budget 不让 private data 变可见，不跳过 lifecycle audit，也不授权删除宿主业务对象。

## Harness And Proposal Sandbox

- `bm-replay` 提供 fixture runner、cross-store replay、memory harness gate 和 benchmark gate。
- `nonproduction-replay-harness` 是 fixture 和合同验证的开发验收 feature，不是部署能力、协议表面或宿主 runtime dependency。
- `bm-evolve` 提供 proposal-only sandbox。Proposal 仍需要经过 SDK 写入路径才能改变记忆状态。
- ESP profile 暴露 compact validation；`profile-server-linux-dev-full` 暴露完整 replay 和 benchmark surface。
- `fixtures/sdk-host-readiness/generic-rust-host/` 与 `fixtures/sdk-host-readiness/beetle-derived/` 由 `scripts/check_sdk_host_integration_readiness.sh` 覆盖。

## 验证

```bash
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_conversation_transcript_substrate.sh
bash scripts/check_release_surface.sh
```
