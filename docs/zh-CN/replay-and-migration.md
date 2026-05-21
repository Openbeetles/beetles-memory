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

## Harness And Proposal Sandbox

- `bm-replay` 提供 fixture runner、cross-store replay、memory harness gate 和 benchmark gate。
- `bm-evolve` 提供 proposal-only sandbox。Proposal 仍需要经过 SDK 写入路径才能改变记忆状态。
- ESP profile 暴露 compact validation；`profile-server-linux-dev-full` 暴露完整 replay 和 benchmark surface。

## 验证

```bash
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_release_surface.sh
```
