# Beetle Memory 0.15.0 macOS SDK 源码发布说明

本次源码发布修复正式回合后学习任务的故障处理。正式身份以完成回读的 `v0.15.0` Git tag 为准。交付范围是 macOS `bm-sdk` 源码及其所需的 Core、Store、Entry 合同；不包含 crates.io、安装包、托管 Release 或部署。

## 后台治理恢复

- HTTP 响应正文读取失败或中断按传输故障进入现有有界重试。
- 完整模型响应的 JSON、协议形状、人格解释、私域整理或长期提取输出不合格式时，按类型化模型输出错误进入有界重试。
- 可信输入、来源、身份、配置及响应字节上限违约仍立即拒绝。凭证故障不再被误判为模型输出错误。
- 治理任务及受控队列摘要携带固定枚举的失败阶段与原因码，不保存原始响应、模型文本、凭证或私密错误消息。成功、取消和阻断会清除过期失败诊断。

## 源码与 Store 合同

公开 Rust 任务类型为 `PostTurnGovernanceJob`；新增诊断类型为 `PostTurnGovernanceFailureStage` 和 `PostTurnGovernanceFailureCode`，名称不带修订后缀。持久 Job 和 Store 仍保留数值 schema 代次，以便明确拒绝 v0.14.0 的旧 Store，且不会改动旧数据。本版不含兼容读取或自动迁移。旧数据如何处理须由数据 owner 另行决定，此前应使用新 Store。

本次源码也修正了 Entry 控制台及 Store 合同样例中的陈旧归档 root 类型引用；归档行为不变。

## 验收边界

Core、Store、Entry、SDK 定向测试覆盖正文中断、模型格式错误、合同硬拒绝、重试状态、安全诊断和旧 schema 拒读；macOS embedded SDK 正式配置编译通过。构建与测试缓存放在外盘。没有运行完整打包或全平台矩阵、真实 Provider、真实用户数据及下游 App 集成。旧任务记录缺少安全阶段码，无法事后确定此前偶发失败的具体路径。
