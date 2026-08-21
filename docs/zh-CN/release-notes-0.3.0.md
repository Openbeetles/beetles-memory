# Beetle Memory 0.3.0 发布说明

发布身份：`v0.3.0`。精确源码 commit 以该不可变 tag 指向为准；Git 托管平台和 crates.io 的实际发布状态以各平台回执为准，本说明不预声明外部动作结果。

## Breaking Release

Beetle Memory 0.3.0 是 SemVer breaking 的源码与持久化版本。它在宿主无关的 Core/SDK/Store owner 中收口持久主体可见性、revision 1 长期记忆 intake integrity、持久 mutation operation receipt、Store authoritative audit，以及 typed HumanUser confirmation authority。

持久化合同为 exact admission：

- Store v9 是唯一接受的 Store schema。
- material v5 是唯一接受的 immutable long-term material schema。
- Adapter V2 是可写协议合同。Adapter V1 只保留读取；所有 V1 mutation 都 fail closed。
- mutation receipt schema v1 保持为 receipt schema；协议/持久化 schema 版本不随机械 SemVer 改号。

## 必须执行的升级动作

0.3.0 没有 automatic migration。

- Store v7/v8 不会被打开、重写或猜测成 Store v9。
- material v4、pre-v5 material，以及缺少 exact subject visibility、provenance、correction evidence 或 typed confirmation evidence 的 material 会 fail closed，并返回 typed migration/repair requirement。
- 部署任何可能打开持久 Store 的 0.3.0 二进制前，必须在数据路径之外备份该 exact Store。
- 回滚必须同时恢复旧二进制和与其匹配的 Store 备份；只回滚二进制不等于数据回滚。
- Archive export/import 是 exact governed archive operation，不是 schema migration 路径。
- 本版本不包含、也不宣称已验证真实用户 Store migration tool。不得拿真实数据的唯一副本试迁移。

## Rust API 变更

- 长期记忆创建在 revision 1 就必须带 typed `subject_visibility` 和持久 provenance；不会产生临时 `AllSubjects` 状态。
- `last_confirmed_at` 是从 typed confirmation evidence 派生的 `Option<u64>` projection；caller 不能通过提交 timestamp 宣称确认。
- long-term candidate intent 携带 trusted-host visibility 与 provenance；模型输出不能改写 ACL。
- `Correct` 是中性的 correction transition。只有当前 `SubjectRegistry` 中 exact active `HumanUser` 才能在同一事务增加 typed human confirmation。模型推断在后续人类确认后仍保持 model-inferred provenance。
- `Supersede` 会清除新 factual owner 的 confirmation，除非 successor 之后通过 governed authority path 独立确认。
- operation-aware mutation API 返回 typed committed/replayed receipt 或 identity conflict，不依赖 entry process cache。

## Adapter V2 与持久 mutation

Adapter V2 的 durable `Write` 和 `LongTermMutate` 必须提供稳定、非敏感的 caller operation key；缺失 key 会在 mutation planning 前失败。不要把用户正文、对话内容、邮箱、token 或其他隐私值作为 key。

- `Committed` 表示 effect、Store receipt、authoritative audit 与 lifecycle event 在一个 Store transaction 中提交。
- `Replayed` 表示相同 scoped actor、operation kind、operation identity 与 intent digest 已提交；返回已存 safe receipt，不产生第二次 effect。
- 同一 operation identity 被用于不同 intent 时返回 typed conflict，且零变更。
- 其他 public mutation family 必须明确分类为 durable、domain-owned receipt 或 non-durable；Beetle Memory 不宣称全局 exactly-once。
- durable receipt 会被 pinned，不会静默淘汰；容量耗尽时新 mutation 原子失败。In-memory Store receipt 不承诺跨进程重启持久化。

## 主体可见性与 Provenance

Accepted shared long-term fact 仍由 `MemorySpace` 单份拥有。`AllSubjects`、`OnlySubjects` 和 `HiddenFromSubjects` 控制 exact mounted subject 能否进入 recall、facet、graph、delivery、evidence、body 与 projection 路径。current 与 historical read 使用所选 revision 的 exact policy/provenance；拒绝读取只返回固定 safe audit 信息，不泄漏被拒记忆内容。

## 发布表面

crates.io 发布集合为：

```text
bm-core
bm-sdk
bm-replay
bm-evolve
bm-adapter
bm-entry
bm-ollama-transparent
bm-cli
bm-llm-gateway
bm-http
bm-wss
bm-mcp
bm-a2a
```

`bm-store-contract-tests` 和 `bm-desktop` 不发布到 crates.io。桌面安装包、签名/公证、容器镜像和 release attachment 需要独立发布证据。

## 验证边界

本地 release candidate 必须通过 formatting、workspace tests、clippy、文档、跨 backend reopen/crash/multiprocess contract、strict target compilation，以及每个发布 crate 的 staged `cargo publish --dry-run`。这些门禁只证明本地源码/包平面。

不属于本源码包证据：真实旧数据迁移、真实 Provider/服务调用、宿主 UAT、trusted Linux P7/P8 external-quality execution、硬件验收、桌面安装包和签名/公证。Git commit/tag/push、crates.io publish 与公开 Release 的当前状态必须从 `v0.3.0` tag 和对应平台回执核验，不能由本地测试或本文替代。
