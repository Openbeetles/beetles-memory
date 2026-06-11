# 存储后端

`bm-store` 拥有记忆持久化。集成方选择 backend 和容量姿态；不定义 memory table、event lineage、snapshot envelope 或 repair 语义。

## Backends

| Backend | Constructor | 适用场景 | 约束 |
| --- | --- | --- | --- |
| In-memory | `StoreBackendConfig::in_memory(profile)` | 测试、示例、短生命周期宿主、ESP compact smoke 路径 | 进程内易失状态 |
| File | `StoreBackendConfig::file(root, profile)` | Linux device、desktop host、轻量 standalone deployment | ESP profile 拒绝 |
| SQLite | `StoreBackendConfig::sqlite(path, profile)` | 需要持久化 indexed storage 的 desktop/server host | 需要 sqlite-capable profile/store feature；ESP profile 拒绝 |
| Embedded | `StoreBackendConfig::embedded(profile)` | ESP 和小容量设备 | 使用 embedded capacity budgets |

## 打开 Store

```rust
use bm_sdk::{ProfileId, StoreBackendConfig, StorePlatform};

let profile = ProfileId::ServerLinuxMemoryGateway;
let store = StorePlatform::open(StoreBackendConfig::sqlite(
    "/var/lib/beetle-memory/memory.sqlite3",
    profile,
)?)?;
```

## Repair Policy

`StoreRepairPolicy::ReportOnly` 是默认策略，适合诊断和 release gate。只有当 runtime 允许在 schema 和 snapshot 检查通过后执行安全修复时，才使用 `StoreRepairPolicy::RepairSafe`。

```rust
use bm_sdk::{StoreBackendConfig, StoreRepairPolicy};

let config = StoreBackendConfig::file("/var/lib/beetle-memory", profile)?
    .with_repair_policy(StoreRepairPolicy::ReportOnly)
    .with_fsync(true);
```

## File Path Budget

logical store key 不是 filesystem path。file backend 会按 profile 的 `StorePathBudget` 把 logical key 映射到受限 physical address：短 digest 文件名加 sidecar key index。`list_*_keys`、snapshot export/import、replay 和 delete 仍然只暴露 logical key。

不要把 transcript ID、conversation ID、attr ID 或 host ref 直接编码进文件名。平台差异化的 filename / relative-path budget 属于 `bm-store`，不属于 adapter crate。

## Ownership Rules

允许：

- 选择 backend type、data path、fsync 和 repair policy。
- 读取 `StoreOpenReport`、`StoreRepairReport`、lifecycle report 和 operator diagnosis。
- 使用 SDK export/import 做迁移。

不要这样接入：

- 绕过 `MemoryRuntime` 写记忆状态。
- 定义另一套 memory schema 或 snapshot envelope。
- 在 adapter crates 里添加第二套 store 实现。
- 给 ESP profile 启用 sqlite。
