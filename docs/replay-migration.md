# Replay And Migration Guide

Replay / migration 是 Beetle Memory 的验证和迁移表面，不是某个宿主项目的兼容层。

## Snapshot Migration

SDK 入口：

```rust
use bm_sdk::{ContinuitySnapshotImportMode, MemoryExportRequest, MemoryImportRequest};

let exported = runtime.export(MemoryExportRequest {
    chat_id: "chat-1".to_string(),
})?;

let imported = runtime.import(MemoryImportRequest {
    snapshot: exported.snapshot,
    target_chat_id: "chat-2".to_string(),
    mode: ContinuitySnapshotImportMode::Replace,
})?;
```

`bm-store` 会校验 schema id、memory system kind、namespace、lineage、state fingerprint 和 event fingerprint。迁移失败必须暴露为 report / error，不能静默截断。

## Replay Inspection

```rust
use bm_sdk::MemoryReplayRequest;

let replay = runtime.replay(MemoryReplayRequest {
    chat_id: "chat-1".to_string(),
    limit: 32,
})?;
```

Replay inspection 用于解释历史上下文如何影响当前召回和投影，不能替代 runtime write / recall / project 的主路径。

## Harness And Evolution

- `bm-replay` 负责 fixture runner、cross-store replay、memory harness gate 和 benchmark gate。
- `bm-evolve` 负责 proposal-only sandbox。sandbox 产出 `EvolutionProposal`，提交仍必须经 SDK write governance。
- ESP profile 只暴露 compact validation；server dev full profile 才暴露 full replay suite 和 benchmark gate。

## Release Use

发布前运行：

```bash
bash scripts/check_replay_sandbox_contract.sh
bash scripts/check_release_surface.sh
```
