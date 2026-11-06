# Beetle Memory 0.13.0 源码发布说明

本次源码版本收口模型交付的宿主体语义。正式源码身份以完成远端回读的 `v0.13.0` Git tag 为准，工作树版本号不能单独证明发布。本次源码发布不包含 crates.io 上传、安装包或部署。

## 主体投影

`MemoryRuntime::project()` 继续由 Beetle Memory 单独完成来源准入、主体挂载、隐私与工作边界、预算和递送证明。`provider_payload().system_memory_block()` 现在以当前挂载主体的视角组织模型上下文。Soul 与 System Memory 仍共同形成同一主体；设备、世界和资源状态仍参与感知。身份、身体归属、可信设备事实、当下感受分别编译，避免把人格倾向写成身体或把 `subject_id` 写成设备。

原始设备诊断字段、关系治理分数、内部 envelope 标题及 opaque 来源引用不再作为普通模型交付文本；它们仍可供受控诊断、来源追溯和审计。关系边界、披露规则与工作完整性继续进入模型上下文。来源事实保留其原语言，宿主不得自行重渲染记忆。

预算按完整语义单元取舍；容不下必要主体及边界时投影失败，不交付尾部截断的半份协议。最终交付文本仍受来源证明和 Turn 回执完整块摘要约束，宿主须原样注入并核对实际送往 Provider 的字节。

## 持久化与验收边界

本改动不引入旧投影 renderer 的兼容路径，也不更改 Store v17 的持久 schema。已接受长期记忆的删除/禁记、来源撤回、私域和跨主体准入继续由原治理 owner 决定；Transcript `DeleteRaw` 不自动遗忘已接受的长期事实。

macOS SDK 生产 profile 的 `cargo check` 与 Clippy（拒绝 warning）已通过；Core 主体、前台投影、程序性与长期记忆及 SDK 投影、召回、预算定向合同已通过。Windows、Linux、ESP、真实 Provider、GUI 效果和 Companion 重新集成均未在本次发布中验收。
