# Beetle Memory 0.4.0 发布说明

发布身份：`v0.4.0`。精确源码 commit 以该不可变 tag 指向为准；Git 托管、crates.io、二进制与托管 Release 页面都需要各自的平台回执，本文不预声明这些外部动作已经发生。

## Breaking Release

Beetle Memory 0.4.0 是 SemVer breaking 的源码与持久化版本。`SPV1 Subject Soul Provisioning` 将 AgentPersona Soul 收口为宿主无关、受治理的完整生命周期，而不是一组可独立直写的 JSON 记录。

持久化合同为 exact admission：

- Store v10 是唯一接受的 Store schema。
- long-term material v5 继续是唯一接受的 immutable long-term memory material schema。
- Subject Soul material、lifecycle head、manifest、generation tombstone、Relationship Source root、projection、durable operation receipt、authoritative audit 与 event 只能经 typed owner 和 backend-native 原子事务写入。
- Adapter V2 继续是可写 adapter 协议；Adapter V1 mutation fail closed。
- durable mutation 继续强制使用稳定且不敏感的 operation key。

## Subject Soul 生命周期

- `seed=None` 是合法 implicit unseeded，且零 mutation；Beetle Memory 不生成默认人格。
- exact active HumanUser 提供的 partial typed founding charter 会原子创建 revision 1、provenance、immutable material、current Core、revision ledger、lifecycle head、manifest、audit、event 与 durable receipt。
- founding material 明确标记为 human-sourced，不能伪装 self-authored；后续变化只能进入 Soul revision governance，宿主不能每轮重放 founding charter。
- active/archived 的 current/exact read 精确绑定 generation、revision 与 digest；terminated generation 只返回安全 tombstone metadata，永不返回 Soul 正文。
- archive/restore 保持同一 generation；reset/reseed 创建新 generation；delete 对当前 Soul identity 终态。reset、reseed、delete 在同一事务清除 terminated generation 的全部 raw 与 derived owner records。
- Relationship Source Constitution 继续由独立 relationship owner 持有。其 Soul projection 采用 MentalPrivacy、relationship source 与 Soul self-boundary 中最严格的 disclosure ceiling，并在两份 root 的 exact CAS 下提交。

## 隐私与导出

public operator inspection 只返回 lifecycle state、generation/revision、digest、provenance class、安全 tombstone metadata 与 typed failure。raw founding charter、SelfAuthoredCore 正文、Private Garden、Inner Life、private docs、relationship secrets 和其他内在材料不进入该表面。

governed disclosure 只能返回 disclosure governance 批准的摘要、改写或拒绝。SPV1 不定义 raw Portable Vault、加密 wire format、key lifecycle、identity remap 或跨 Store Soul import；这些能力继续由 deferred EAP2 owner 持有。

## 升级与迁移边界

0.4.0 没有 automatic migration。

- Store v9 及更早 Store 不会被打开、重写或猜测成 Store v10。
- 缺少 exact lifecycle root、manifest、owner/generation envelope 与 closure digest 的旧 opaque Soul record 会 fail closed，并返回 typed migration/repair requirement。
- 允许 0.4.0 二进制打开持久 Store 前，必须在数据路径之外备份该 exact Store。
- 回滚必须恢复上一版二进制及其匹配的 Store 备份。只回滚二进制不等于数据回滚，archive export/import 既不是 schema migration，也不是回滚路径。
- 本 source candidate 不包含、也未验证真实用户 migration tool；不得拿真实数据的唯一副本试迁移。

## 发布表面与证据边界

预期 crates.io 集合仍为 `bm-core`、`bm-sdk`、`bm-replay`、`bm-evolve`、`bm-adapter`、`bm-entry`、`bm-ollama-transparent`、`bm-cli`、`bm-llm-gateway`、`bm-http`、`bm-wss`、`bm-mcp` 和 `bm-a2a`。`bm-store-contract-tests` 与 `bm-desktop` 不发布到 crates.io。

本地 source candidate 必须通过 formatting、tests、clippy、文档、跨 backend reopen/crash/multiprocess contract、strict target compilation 和 staged publish dry-run。真实数据迁移、Provider/服务调用、宿主或 GUI UAT、trusted Linux external-quality execution、安装包、签名/公证、Git commit/tag/push、crates.io publish 与托管 Release publish 都需要独立证据。
