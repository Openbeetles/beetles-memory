# Beetle Memory 0.11.0 源码发布说明

0.11.0 完成长期记忆控制完整性修复：列表不漏条，禁记规则可以持久查询并精确解除，
聊天原文撤回后关联证据不再流入模型，旧后台学习任务不能覆盖人工纠正。
发布身份由 `v0.11.0` 源码 tag 与对应 commit 确定。接入见 [API](api.md) 和[集成文档](integration.md)。

## 长期记忆列表与禁记规则

同批 intake 出现重复 durable owner 时整批拒绝，不会静默保留最后一个值。

长期记忆列表先执行 mounted subject 可见性过滤，再分页；cursor 绑定查询、主体和快照，
失效时重新查询。没有结果与没有可见结果都不会泄漏被拒绝记忆的正文或证据。

通过 `list_memory_governance_policies` / `get_memory_governance_policy` 查询持久规则。
`mutate_memory_governance_policy(operation_id, request)` 要求稳定 operation ID；解除规则必须使用
查询返回的 `MemoryGovernancePolicyRef { policy_id, policy_revision }`，不能按条件批量猜测解除。
成功为 `Committed`，同 intent 重试为 `Replayed`；旧 revision、不同 intent 复用 ID 或跨主体请求被拒绝。
原事实删除后仍可管理规则，重建同条件规则不会被旧解除请求误删。

规则只限制当前 mounted subject 的未来事实写入。多个 Agent 共用 HumanUser actor 不会共享禁记范围；
省略 selector 的空间或主体会绑定当前 Runtime，不代表全体主体。规则不使既有事实自动隐身。

## 来源撤回与人工纠正

Transcript `Mask` / `DeleteRaw` 在同一事务撤回派生事实证据及关联 facet/graph。
current、as-of、最终 recall delivery 和 projection 都不能复活已撤回来源；独立有效证据仍可用。
accepted fact 正文保留，删除事实需要显式长期记忆治理。

Canonical turn 的 `factual_admission_revision` 记录来源入场顺序。人工 Correct/Supersede 建立
owner 控制屏障；旧排队任务、执行中模型返回、重试及 dedupe 删除均不能绕过屏障。
真正后续入场的来源仍可依法学习，不能把该保护理解为永久冻结事实。
`LongTermTransitionBasis` 保留转换依据，显式控制写入需要 SDK 内部权威，不能由宿主伪造。

## Breaking 公共合同与 Store

- 只接受 Store v16；长期记忆 immutable material v6、control revision v4。Store v15 及更早
  只读拒绝，无 automatic migration、宽松缺字段读取或双写；本发布不删除或迁移真实数据。
- policy mutation 必须携带 operation ID，解除目标改为 exact policy ID/revision；列表/详情成为
  SDK 和薄 adapter 的正式公共入口。Adapter V2 envelope 不变，不代表旧请求结构兼容。
- 公共 `CanonicalTurnDelta` intake 不接受宿主提供的入场计数；Store 在首次持久化
  `TranscriptTurnRecord` 时赋值 `factual_admission_revision`。显式构造持久 record 或长期记忆
  material/control fixture 的测试／工具需更新必填字段。使用正式 SDK intake/control，由同一
  事务生成 owner/head/material/index/audit/event/receipt。
- 从源码集成时，所有 Beetle crate 使用同一 tag。旧版本回退必须同时使用匹配的完整旧 Store；archive import
  不是 schema 回滚或迁移工具。开发用新合成 Store，真实数据处置由 operator 单独决定。

0.10.0 的生产方法学习、受治理人格成长和用户设备本地时间继续保留；本版未改变其职责边界。

## 验收与发布边界

自动验收覆盖 InMemory/File/SQLite、重开、current/as-of、主体隔离、来源撤回、旧任务竞争、
精确规则 CAS/幂等和失败事务全量不变，另核对 macOS 桌面合同。严格跨目标编译虽已单独执行，
不等于 Windows/Linux/ESP 已对接。合成模型证明接线，不代表真实 Provider 学习质量。
完整多方法对照实验、真实数据、GUI/UAT、可信 Linux 动态、外部 benchmark、托管 Release、
安装包及部署不属于本次源码发布。13 个 crate 的 publish dry-run 留到 crates.io 发布进入范围时执行；本轮没有上传。
