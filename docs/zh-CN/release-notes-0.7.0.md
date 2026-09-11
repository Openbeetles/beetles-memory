# Beetle Memory 0.7.0 发布说明

发布身份：`v0.7.0`。发布后的 annotated tag 标识精确源码 commit；本文不提前声明 Git 推送、crates.io 上传、托管 Release、二进制分发或部署已完成。

## 受治理的程序性学习

Agent Tool 经验现在有精确的 MemorySpace 与 subject owner、不可变 material、scope manifest、current/as-of 读取与持久 application evidence。`MemoryLearningEngine` / `MemoryLearningService` 将 procedural feedback 与语义模型配置分开调度：缺 Key 或 semantic Provider 失败不会阻止合格的本地反馈完成。既有 semantic Job V3 与 procedural job/receipt 职责独立。

Projection 明确区分 Preview 与 Turn。只有最终受治理交付才能签发 `ProceduralSelectionReceiptV1`，绑定 exact subject、turn、source owner/revision 与实际交付。`finalize_turn` 接收 typed learning evidence 和真实执行反馈，不接收调用方自报的 selected IDs。宿主观察、模型推断、active HumanUser 确认保留不同 authority；receipt 只证明曾交付，不证明执行成功。

Runtime Skill 首次创建由重复、已接受的工具方法证据治理，不根据工具描述发明步骤，也不执行工具。已有按 revision 校验的编辑与 lifecycle 控制保留；标准 Agent Skill 包继续只读挂载。主体反馈不能取得共享程序修改权。

## Breaking 合同与持久化

- 唯一 Store generation 为 Store v13；immutable long-term material v5 与 Adapter V2 保持原职责。旧代、部分或损坏 Store fail closed；无 automatic migration、compatibility reader、双写或旧 blob 主体猜测。
- 删除公开 procedural 初建、旧 feedback write、裸 selected-ID carry 与不安全 Evolve commit helper。消费者改用受治理 projection/finalize/learning 路径，adapter 不得重造这些 owner。
- Store signing authority 与 protected Runtime Skill owner 不通过公开 raw archive 导出/导入。Import 不是 schema 升级，也不能复活已撤回来源；受控同代 Store 恢复与公开 archive disclosure 是不同合同。
- operation-aware receipt/audit closure 与 typed inspection authority 继续由 Memory 拥有。来源撤回、隐私、能力和主体校验仍约束 current/as-of projection。

## Transcript 完整性

Canonical turn intake 原子提交 Session shadow 与 Transcript。exact owner、turn id 和 canonical digest 判定重试，异 payload 冲突。不同回合即使 user/assistant 正文完全相同也保留独立证据，Session 有界窗口满额与重开后不丢失。

`CanonicalTurnDelta.input_messages` 只接收本轮输入；full-history 协议先解码再调用 `protocol_window_user_delta`，不能与已存正文比较去重。空 assistant/tool-call 保留历史边界，只有工具续接、没有新 user 时不重记旧发言。旧 Core Session-only commit 和正文重叠 helper 已删除。message identity 绑定 owner、subject、turn 与 ordinal，不依赖 Session 计数。

## 接入与证据边界

这是 0.6.0 之后的 clean-break source release，不是 patch。消费者需要更新公共调用，并明确重建可丢弃的旧开发 Store；发布流程不删除、迁移或重建真实用户数据。回滚应使用旧 source tag 及其匹配且未受损的 Store，不用旧代码打开新代数据，不移动既有 release tag。

外部 benchmark 重现要求显式传入源码 checkout 之外的工作根，不再默认在本机仓库创建大目录。自动合同、合成 File/SQLite reopen、strict cross-target 编译及 staged package/publish dry-run，与真实 Provider、GUI/UAT、硬件运行、Linux 动态质量、外部 benchmark 实跑是独立证据；后者不因 source tag 而获得认证。本轮不上传 crates.io，不发布安装包、签名/公证、托管 Release 或部署。
