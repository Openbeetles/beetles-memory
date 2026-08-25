# Beetle Memory 0.5.0 发布说明

发布身份：`v0.5.0`。精确源码 commit 以该不可变 tag 指向为准；Git 托管、crates.io、二进制与托管 Release 页面都需要各自的平台回执，本文不预声明这些外部动作已经发生。

## Breaking Release

Beetle Memory 0.5.0 是 SemVer breaking 的公共 SDK 与持久化版本。CTQ1 将 evidenceful conversation 发现、最新/更旧/更新 timeline 分页、受治理正文搜索和 UTC 日期区间 activity 收口为 Memory-owned 合同，不再要求宿主另建索引。

精确持久化合同为：

- Store v11 是唯一接受的 Store schema；long-term material v5 继续 exact admission。
- Platform capability snapshot 使用 `beetle-memory.platform.capability.v4`。
- Adapter V2 继续是唯一可写 adapter 协议。
- durable mutation receipt 继续要求稳定 operation identity。
- 普通 Store open 绝不执行 automatic migration。

## Conversation Catalog、Timeline、搜索与日期导航

- `MemoryRuntime::list_conversations` 按 exact MemorySpace 与 mounted subject 列出 evidenceful conversations；产品标题、置顶、空白 draft 和当前选择继续属于宿主 UX。
- `MemoryRuntime::query_transcript_timeline` 支持首屏 latest、对称 older/newer 分页、durable anchor、精确 sequence/time，以及调用方明确 UTC 半开区间内的第一条可见消息。
- `MemoryRuntime::search_transcripts` 只搜索 canonical、可见的 user/assistant 正文，返回受治理的 Unicode-safe excerpt 与可进入同一 timeline 的 durable anchor。
- `MemoryRuntime::query_transcript_activity` 为明确 UTC range 返回可见计数和首末 anchor，可用于宿主定义的日历日；Beetle Memory 不猜测 timezone，也不拥有日历展示。
- `TranscriptQueryCursor` 是 opaque、Store-signed、tamper-evident 的值，并绑定 operation、scope、mounted subject、disclosure view、query、direction、snapshot、owner generation、keyring incarnation 与 expiry。
- `HostUi` 仍然只是脱敏 disclosure view，不是宿主专用 API、conversation owner、cursor authority 或 authorization token。

## 隐私、归档与 Repair

`PrivateGardenInternal`、`SoulGovernance`、`OperatorDiagnostic`、无权主体、masked 和 raw-deleted message 在 time/search candidate 与 posting 形成前即被排除；Runtime hydration 还会重新执行 lifecycle、subject、privacy、capability 与 disclosure 检查。公共 archive 不携带 private query keyring；同 scope import 验证完整 CTQ closure 并生成全新 cursor authority，因此 source cursor 不能跨入恢复后的 Store。

File、SQLite、in-memory 合同覆盖 CTQ closure、reopen、lifecycle exact-zero、cursor tamper/scope/stale、archive replacement 与 repair-required failure，且不允许宿主 fallback index。

## 显式 v10 到 v11 迁移

Exact v10 persistent Store 必须离线调用 `MemoryStoreHandle::migrate_v10_to_v11`。先关闭全部 handle，并在数据路径之外备份 exact Store。File migration 先构建并验证 sibling v11 Store，再原子交换目录；SQLite 在一个事务内提交 schema、CTQ closure 与 migration event。成功返回 `StoreMigrationReport`；partial、foreign 或失败输入保留 v10 并 fail closed。In-memory 与 embedded backend 不是 migration target。

当前只有合成 Store 证据，没有读取或迁移真实用户数据；archive export/import 也不是 schema migration 或 rollback。

## 证据边界

本地 source candidate 必须通过 formatting、tests、clippy、文档、profile/cross-target gate、跨 backend reopen contract 和 package dry-run，之后才能提议 Git 发布。Provider/服务调用、GUI 或宿主 UAT、trusted Linux 外部质量、安装包、签名/公证、真实数据迁移、Git commit/tag/push、crates.io publication 和托管 Release publication 都需要独立证据。
