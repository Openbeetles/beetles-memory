# Beetle Memory 0.8.0 源码发布说明

本文描述 0.8.0 源码合同。正式发布身份由 `v0.8.0` Git tag 与其精确 commit 确定，
工作树版本号不代表已经发布。上一代源码合同见 [0.7.0](release-notes-0.7.0.md)。

## 学习证据与声明权

程序性反馈使用 `PostTurnLearningInputV2`，工具反馈使用 `AgentToolUsageFeedbackV3`。
执行事实与方法证据分别提交：事实包含 typed outcome 和有界调用引用，不携带任意结果正文或错误摘要；
方法另有规范化内容、来源分类与精确执行引用。模型提议、用户声明与执行见证不互相冒充，
工具成功不自动代表任务完成或方法有效。方法的激活和 Runtime Skill 晋升继续分别遵循各自证据门槛。

受信宿主初始化代码显式管理持久 producer binding，并通过公共 SDK 签发不可序列化的提交能力。
该能力的公共类型为 `MemoryProceduralSubmissionCapability`。
有反馈时调用 `finalize_turn_with_procedural_evidence`；普通无反馈回合仍使用 `finalize_turn`，无需 producer grant。
Bearer 身份、操作权限和 selection receipt 均不能自行替代证据声明权。远端 payload 不接收提交能力，
宿主不得在 UI、adapter 或数据库维护第二份权限规则。

合法执行事实与被拒方法可以产生部分接受，消费者必须读取 `partially_accepted_count` 和 `method_dispositions`，
不能把提交成功显示为所有内容都已接受。重试复用精确 canonical payload；新增真实执行使用新的调用身份。

## 撤回、恢复与读取

Producer 权能收窄、撤销或来源可见性变化会立即限制受影响的 current/as-of 读取，
后台通过原有官方学习服务进行有界重算。不可用状态明确区分 `Reconciling`、`Blocked` 等，
未读取的统计为 `None`，不伪装成零或沿用旧方法正文。

容量阻断需要解决资源问题后显式获权恢复；wake 或重开不自行清除它。
`procedural_reconciliation_status` 在完全重开后发现精确持久恢复引用，不需要保存旧任务报告；
Entry attachment 状态直接消费同一 SDK 真源。容量耗尽不会误报为 Store 损坏。
永久删除、producer 撤权与已退役 Skill 不会因为重试、重算或旧归档恢复而自动复活。
公开归档不携带源 Store 的内部 producer 权威和派生审计；目标受保护学习来源若无法保留，
恢复会 typed 拒绝且不部分写入。具体边界见 [归档说明](replay-and-archive.md)。

## Breaking Store 与接入

0.8.0 源码只接受 Store v14。Procedural evidence/job/index/ledger/receipt 为 V2，Agent Tool material/head 为 V3，
Runtime Skill owner record 为 schema 2；long-term material v5 与 Adapter V2 保持原职责。
旧代 Store 拒开，不提供 automatic migration（自动迁移）、另开空库、删除数据或兼容 reader。
可丢弃开发数据的重建必须由其 owner 显式执行；回滚必须配对旧源码和对应的未损坏旧 Store。

嵌入式消费者把 `bm-entry::MemoryLearningService` 附着到已有 `Arc<MemoryRuntime>`；
Entry 自建 Runtime 也消费同一个服务。Store、registry、MemorySpace 及控制能力必须一致。
产品模型配置与 Key 仍只有宿主一份真源，不新增第二套治理模型设置。
见 [可编译接入示例](integration.md#可编译的官方学习服务示例) 与 [公共 API](api.md)。

## 发布与验证边界

自动合同、合成持久重开和静态平台检查不代表真实 Provider、GUI/UAT、真实数据、
可信 Linux 动态质量或外部 benchmark 已验证。拟交付形态为源码版本；不包含 crates.io 上传、
安装包、签名/公证、托管 Release 或部署。正式发布身份以最终精确 tag 和远端回执为准。
