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

## Harness And Proposal Sandbox

- `bm-replay` 提供 fixture runner、cross-store replay、memory harness gate 和 benchmark gate。
- `bm-evolve` 提供 proposal-only sandbox。Proposal 仍需要经过 SDK 写入路径才能改变记忆状态。
- ESP profile 暴露 compact validation；`profile-server-linux-dev-full` 暴露完整 replay 和 benchmark surface。
- `fixtures/sdk-host-readiness/generic-rust-host/` 与 `fixtures/sdk-host-readiness/beetle-derived/` 由 `scripts/check_sdk_host_integration_readiness.sh` 覆盖。

## 验证

```bash
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_release_surface.sh
```
