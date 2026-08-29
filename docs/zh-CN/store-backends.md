# 存储后端

私有 `bm-sdk` persistence kernel 拥有记忆持久化。集成方通过 `MemoryStoreHandle` 选择 backend 和容量姿态；不定义 memory table、event lineage、snapshot envelope 或 repair 语义。

## Backends

| Backend | Constructor | 适用场景 | 约束 |
| --- | --- | --- | --- |
| In-memory | `StoreBackendConfig::in_memory(profile)` | 测试、示例、短生命周期宿主、ESP compact smoke 路径 | 进程内易失状态 |
| File | `StoreBackendConfig::file(root, profile)` | Linux device、desktop host、轻量 standalone deployment | ESP profile 拒绝 |
| SQLite | `StoreBackendConfig::sqlite(path, profile)` | 需要持久化 indexed storage 的 desktop/server host | 需要 sqlite-capable profile/store feature；ESP profile 拒绝 |
| Embedded | `StoreBackendConfig::embedded(profile)` | ESP 和小容量设备 | 使用 embedded capacity budgets |

## 打开 Store

```rust
use bm_sdk::{MemoryStoreHandle, ProfileId, StoreBackendConfig};

let profile = ProfileId::ServerLinuxMemoryGateway;
let store = MemoryStoreHandle::open(StoreBackendConfig::sqlite(
    "/var/lib/beetle-memory/memory.sqlite3",
    profile,
)?)?;
let open_report = store.open_report();
```

启动诊断里要保留 `StoreOpenReport`。它包含 schema 与 repair finding，operator 需要在 runtime 接受写入前看到这些信息。

## Repair Policy

`StoreRepairPolicy::ReportOnly` 是默认策略，适合诊断和 release gate。只有当 runtime 允许在 schema 和 snapshot 检查通过后执行安全修复时，才使用 `StoreRepairPolicy::RepairSafe`。

```rust
use bm_sdk::{StoreBackendConfig, StoreRepairPolicy};

let config = StoreBackendConfig::file("/var/lib/beetle-memory", profile)?
    .with_repair_policy(StoreRepairPolicy::ReportOnly)
    .with_fsync(true);
```

## 0.6.0 源码候选 Schema Admission

0.6.0 源码候选只接受 Store v12 与 immutable long-term material v5。Store v12 增加 exact Post-Turn Governance Job V3 / Scope Index V3 / Job Ref V2 closure，以及 Store-owned immutable binding snapshot 和有界 binding revision index。File、SQLite 与 in-memory admission 会在每个相关 transaction 内验证变更的 Job/Index/Binding owner；持久 reopen 与 snapshot import 会验证全量 closure。

不提供 v11→v12 migration API、compatibility reader、双写或 automatic migration。Store v11、governance V2、partial v12 state、orphaned binding/job/index document 与 foreign schema 都会 fail closed。旧代开发数据只能由其 owner 明确删除并重建。Archive export/import 不是 schema migration 或 compatibility path，本版本也不声明任何真实用户 Store migration。

## File Path Budget

logical store key 不是 filesystem path。file backend 会按 profile 的 `StorePathBudget` 把 logical key 映射到受限 physical address：短 digest 文件名加 sidecar key index。`list_*_keys`、snapshot export/import、replay 和 delete 仍然只暴露 logical key。

不要把 transcript ID、conversation ID、attr ID 或 host ref 直接编码进文件名。平台差异化的 filename / relative-path budget 属于私有 `bm-sdk` persistence kernel，不属于 adapter crate。

## Capacity And Key Budget

`StoreRuntimeBudget` 由 Beetle Memory 编译，并在打开 backend 前转换成 `StoreCapacityBudget`。预算覆盖 KV、blob、snapshot、event count、logical namespace/key bytes、event record key bytes，以及独立的 export/import byte limit。

所有 backend 执行同一套预算合同。超长 logical key、event record key、snapshot import、export 或累计 blob 超限时，必须返回结构化 `store_budget_exceeded`；backend 不能截断 key，也不能静默丢记忆。

## Ownership Rules

允许：

- 选择 backend type、data path、fsync 和 repair policy。
- 读取 `StoreOpenReport`、`StoreRepairReport`、lifecycle report 和 operator diagnosis。
- 使用 `MemoryRuntime::export_memory_space` / `import_memory_space` 和精确 `MemoryArchiveScope` 做原子 archive replacement。

不要这样接入：

- 绕过 `MemoryRuntime` 写记忆状态。
- 定义另一套 memory schema 或 snapshot envelope。
- 在 adapter crates 里添加第二套 store 实现。
- 给 ESP profile 启用 sqlite。
