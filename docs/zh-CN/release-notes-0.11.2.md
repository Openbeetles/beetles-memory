# Beetle Memory 0.11.2 源码发布说明

0.11.2 修复已提交的方法学习回合在方法版本前进或 producer 撤销后无法精确重放的问题。发布身份以 `v0.11.2` 源码 tag 和对应 commit 为准。

精确重放必须同时匹配规范化回合、Store 私有的原始输入承诺、历史封存的 producer 绑定、主体及 Store 实例。两个持久任务都存在时，只返回原有任务标识，不写 Store，也不唤醒 worker。已提交回合如缺少任务，进入独立恢复路径：先验证规范化重复回合，再以当前 producer 和来源授权及精确 Store 前置条件创建缺项。输入变化、原文撤回或授权撤回都不能借重放生成新任务。

本版没有改变公共 API、Store v16、长期记忆 material v6、control revision v4 或 Adapter V2。0.11.0 的[控制完整性合同](release-notes-0.11.0.md)仍适用，不提供旧 Store 的 automatic migration。所有 Beetle crate 应使用同一源码 tag。

已通过 SDK 入场、producer 撤权、来源授权、方法版本、原始输入承诺及缺任务恢复的定向合同，以及格式和静态检查。Windows/Linux/ESP 尚未实际对接，本版未运行其全量矩阵；未验证真实数据、Provider 或 GUI。此交付仅为 Git 源码和 tag，未上传 crates.io，也未创建托管 Release、安装包或部署。
