# Store Backend Guide

Beetle Memory 自己实现存储层。集成方只选择 backend、profile、容量和修复策略，不实现 memory schema、event lineage、snapshot envelope 或写入语义。

## Backends

| Backend | 入口 | 适用场景 | 约束 |
| --- | --- | --- | --- |
| in-memory | `StoreBackendConfig::in_memory(profile)` | 测试、短生命周期宿主、examples | 进程退出后丢失 |
| file | `StoreBackendConfig::file(root, profile)` | Linux device、desktop、轻量独立部署 | ESP profile 禁止 |
| sqlite | `StoreBackendConfig::sqlite(path, profile)` | Linux server、desktop、较强查询和持久化 | 需要 `sqlite-store` / profile feature，ESP profile 禁止 |
| embedded | `StoreBackendConfig::embedded(profile)` | ESP standalone、ESP embedded SDK、小容量设备 | 受 snapshot byte budget 约束 |

## Opening Store

```rust
use bm_sdk::{ProfileId, StoreBackendConfig, StorePlatform};

let profile = ProfileId::ServerLinuxMemoryGateway;
let store = StorePlatform::open(StoreBackendConfig::sqlite(
    "/var/lib/beetle-memory/memory.sqlite3",
    profile,
)?)?;
```

## Repair Policy

`StoreRepairPolicy::ReportOnly` 是默认策略，适合 release gate 和诊断；`StoreRepairPolicy::RepairSafe` 只能用于确定 schema / snapshot contract 已通过的运行环境。

```rust
use bm_sdk::{StoreBackendConfig, StoreRepairPolicy};

let config = StoreBackendConfig::file("/var/lib/beetle-memory", profile)?
    .with_repair_policy(StoreRepairPolicy::ReportOnly)
    .with_fsync(true);
```

## Ownership Boundary

调用方不能：

- 自己定义 memory table / snapshot envelope。
- 绕过 `MemoryRuntime` 直接写长期记忆或 procedural memory。
- 在 adapter crate 中实现第二套 store。
- 给 ESP profile 打开 sqlite。

调用方可以：

- 选择 backend。
- 配置数据路径和 fsync。
- 读取 `StoreOpenReport`、`StoreRepairReport` 和 operator diagnosis。
- 使用 SDK export / import 做迁移。
