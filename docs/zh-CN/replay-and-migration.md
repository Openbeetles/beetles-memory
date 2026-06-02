# 回放与迁移

Replay 和 migration 是验证与连续性工具。它们不替代正常的 write、recall、project 或 maintain 路径。

## Snapshot Export And Import

```rust
use bm_sdk::{ContinuitySnapshotImportMode, MemoryExportRequest, MemoryImportRequest};

let exported = runtime.export(MemoryExportRequest {
    chat_id: "chat-1".to_string(),
})?;

let imported = runtime.import(MemoryImportRequest {
    snapshot: exported.snapshot,
    target_chat_id: "chat-2".to_string(),
    mode: ContinuitySnapshotImportMode::FullRestore,
})?;
```

可用 import modes：

- `ContinuitySnapshotImportMode::BootstrapImport`
- `ContinuitySnapshotImportMode::FullRestore`

Store import 会校验 schema id、memory system kind、namespace、lineage、state fingerprint 和 event fingerprint。失败的导入必须暴露为 report 或 error，不能静默截断数据。

## Replay Inspection

```rust
use bm_sdk::MemoryReplayRequest;

let replay = runtime.replay(MemoryReplayRequest {
    chat_id: "chat-1".to_string(),
    limit: 32,
})?;
```

Replay 用来解释历史 continuity state，适合 inspection、migration validation 和 release gates。

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
| `Export` | 迁移与可携带性 | 受 `include_private`、profile、permission 和 retention policy 控制。 |

删除或 mask raw transcript content 时，report 必须把 downstream impact 和已接受的 long-term、shared factual、procedural、private、soul-related memory planes 分开说明。Lifecycle report 通过 SDK 的 operator-audit view 暴露 host refs，因此 internal/model-only refs 和 raw labels 在离开 runtime 前会被脱敏。Redacted replay 必须 fail closed：某个视图不能证明字段可见时，返回 redaction marker 和 audit reason，而不是 raw content。SDK runtime consumer 会优先使用 transcript-backed evidence，而不是 legacy session shadow；transcript 被 mask 或 legacy alias 不可信时，不会再从 `SessionStore(chat_id)` 回填原文。

SDK transcript replay/export request 通过 `cursor`、`next_cursor` 和 `has_more` 支持有界分页。Host ref visibility 会按 view 执行，host ref 的 `label` 只在 owner 允许视图中保留，其他视图会做字段级脱敏，并在 redaction report 中记录 `HostRefLabel`。

`HostUi` transcript replay 由 SDK `transcript_replay` capability 控制。桌面和 embedded SDK 宿主可以提交 transcript turn 后，把同一个 conversation 读回给 UI 展示；这不要求打开 `MemoryRuntime::replay`、replay harness、raw owner replay 或 deep inspection。

## Memory-space 迁移 dry-run

替换宿主记忆实现或迁移一份已配置 SDK store 时，使用 memory-space migration：

```rust
use bm_sdk::{
    apply_memory_space_migration, export_memory_space, preview_memory_space_migration,
    MemorySpaceExportRequest, MemorySpaceMigrateApplyRequest,
    MemorySpaceMigratePreviewRequest,
};

let exported = export_memory_space(&store, MemorySpaceExportRequest {
    memory_space_id: "space-main".to_string(),
    include_private: false,
})?;
let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
    source_memory_space_id: "space-main".to_string(),
    target_memory_space_id: "space-copy".to_string(),
    snapshot: exported.snapshot.clone(),
});
assert!(!preview.loss_risk);
assert!(preview.manifest.whole_space_snapshot);

apply_memory_space_migration(&target_store, MemorySpaceMigrateApplyRequest {
    target_memory_space_id: "space-copy".to_string(),
    snapshot: exported.snapshot,
})?;
```

`include_private=false` 必须剔除 private snapshot entry。Beetle-derived replacement fixture 必须和 generic fixture 使用同一个 public migrator。
`preview.manifest` 是 dry-run 诊断真源：它列出 plane/privacy count、schema id、whole-space snapshot
状态、conflict/loss risk 和 subject remap 状态。当前 apply 不做 subject key rewrite；如果 source/target
space 不同，manifest 会报告 `subject_remap.required=true`、`applied=false`。

当 memory-space storage 中存在 transcript evidence 时，`include_private=false` 默认会把 raw transcript document 和 `conversation_transcript_derived_ref` manifest 从 export 中剔除。Migration diagnostics 必须保留 raw transcript、redacted transcript slice、accepted memory planes、derived refs 和 opaque host refs 的分层。宿主对象 payload 不由 Beetle Memory 导出；只有在请求 view 允许时才携带 `HostOpaqueRef` metadata 和 relation。`RedactedTranscriptSlice` 会报告 message 和 host-ref redactions，让调用方知道哪些内容被省略，但看不到 raw material。`TranscriptLifecycleReport.derived_memory_refs` 是复核从受影响 transcript evidence 派生出的已接受 Memory material 的清单。

Transcript replay 和 migration tooling 可以用 `TranscriptTurnPage` 做有界分页。`MemoryTranscriptRepairRequest` 提供 SDK 层 transcript repair inspection，`TranscriptRepairReport` 会把 Memory-owned derived refs 和 transcript source turns/messages 对齐检查；source turn 缺失、`MissingSourceMessage`、orphan derived refs、corrupt transcript records、mismatched source keys 和 duplicate sequence/cursor evidence 都必须以 fail-closed repair issue 报告，而不是返回看似干净的空影响结果。

Compact profile 可以按 `TranscriptGovernanceBudget` 裁剪 transcript turns、host refs、redaction report items、lifecycle derived refs 或 repair issues 数量。Replay audit 会在 replay redactions 受预算裁剪时记录 `ProfileBudget`；lifecycle 和 repair report 的列表被裁剪时会设置 `profile_budget_applied=true`。这只是数量裁剪：profile budget 不让 private data 变可见，不跳过 lifecycle audit，也不授权删除宿主业务对象。

## Harness And Proposal Sandbox

- `bm-replay` 提供 fixture runner、cross-store replay、memory harness gate 和 benchmark gate。
- `bm-evolve` 提供 proposal-only sandbox。Proposal 仍需要经过 SDK 写入路径才能改变记忆状态。
- ESP profile 暴露 compact validation；`profile-server-linux-dev-full` 暴露完整 replay 和 benchmark surface。
- `fixtures/sdk-host-readiness/generic-rust-host/` 与 `fixtures/sdk-host-readiness/beetle-derived/` 由 `scripts/check_sdk_host_integration_readiness.sh` 覆盖。

## 验证

```bash
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_conversation_transcript_substrate.sh
bash scripts/check_release_surface.sh
```
