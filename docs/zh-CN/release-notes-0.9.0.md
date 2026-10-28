# Beetle Memory 0.9.0 源码候选说明

本文描述继 [0.8.0](release-notes-0.8.0.md) 之后的 0.9.0 源码候选。
工作树版本号不代表已发布；发布身份须由 `v0.9.0` Git tag 和经核验的 commit 确定。
本候选合并来源读取证明修复与受治理主体投影修复，不包含比较学习或新的工具执行器。

## 共享来源读取证明修复

合法读取 MemorySpace 共享来源时，可能需要同空间另一主体产生的 operation proof。
SDK 现在从真实 Store 前像推导精确来源地址与依赖根，再准入这次读取，不再将该合法路径误报为
`procedural_source_closure_repair_required`。跨空间 identity、非 canonical 地址、调用方提议的后像
和无关 operation receipt 仍被拒绝。读取证明不赋予修改其他主体来源或扩大 mutation fanout 的权限。

## 受治理的主体投影

被接受的 OuterVoice 六项表达信息完整进入 typed expression guidance。表达只负责风格，不能定义
身份、工具权限或披露权限；关系宪法仍可进一步收紧。合法 partial founding charter 不再借用内部
展示 Markdown 编造回复姿态、主动性或边界模式；unseeded 主体不会自动得到默认人格。

FeltSignificance、TemperamentContinuity、InnerConflict 的派生状态只参与获准的私域运行时上下文。
主体、关系、generation、current/history 和 profile 边界保持生效。预算不足时整块省略可选表达或
私域内容，报告按实际交付 payload 计算，不把早期编译预览当最终交付。

关系运行时的人格继承来自精确验证过的 Soul material；真实、双方边界、修复及自我保存承诺继续是
关系约束，不再被误当人格优先级。unseeded 关系不伪造 core revision。受保护的关系正文及表达指导
不进入公开 UI、operator 或 Gateway audit 表面；这些结构门禁不宣称能防住模型的一切语义泄漏。

## 公共 API breaking 变更

- `LLMRuntimeProjectionEnvelope` 新增 `expression_guidance: Option<SubjectExpressionDiscipline>`；
  `runtime_constraints` 从 `Vec<String>` 改为 `Vec<RuntimeProjectionSourceBlock>`。
- Core `InhabitedSubjectProjection` 同样新增 `expression_guidance`。
- `SubjectSoulRelationshipRuntimeViewV1` 新增 `inherited_core_revision`、
  `inherited_priority_constitution`、`inherited_response_mode`、`inherited_initiative_posture`、
  `inherited_relationship_posture`。它的序列化运行时 view 形状也改变；该临时 view 不是持久 Soul
  revision material 或归档格式。
- Core 和 SDK 公开 `SubjectExpressionDiscipline`，消费既有 OuterVoice 与 typed 关系上限，
  不建立第二份持久人格模型。

使用显式 struct literal 的调用方需要更新。直接遍历 constraints 的代码现在得到带来源、保护标记
和 `.content` 的 source block；不能丢掉元数据后将正文公开，也不要自行渲染第二份投影或增加旧路径
fallback。模型交付优先使用 `projection.provider_payload().system_memory_block()`；公开消费分别使用
`projection.report().ui_api_projection()`、`.operator_projection()` 或 `.gateway_audit()`。
不要通过 `Debug` 把 raw envelope 直接写入日志或 UI。

## Store 与学习服务接入

沿用 0.8.0 的 Store v14、long-term material v5 和 Adapter V2。本候选没有引入 Store 迁移或清库要求。
Store v13 及更早仍按原准入规则拒绝，不提供 automatic migration、兼容 reader 或静默删除；
本轮候选准备不读取或处理真实数据。实际部署前应在数据路径之外保存精确 Store 备份；回退使用
此前验证过的源码及其匹配、完好的 Store 备份，archive import 不是回滚或 schema migration。

既有 `PostTurnLearningInputV2`、`AgentToolUsageFeedbackV3`、`MemoryProceduralSubmissionCapability`
与 `procedural_reconciliation_status` 合同保持不变。普通回合使用 `finalize_turn`，有声明权的证据
使用 `finalize_turn_with_procedural_evidence`。嵌入式宿主继续将 `bm-entry::MemoryLearningService`
附着到同一 Runtime、Store 和 registry；宿主仍唯一拥有配置和凭证，不增加第二套治理模型配置或 worker。
见[接入说明](integration.md)和[公开 API](api.md)。

## 交付边界

候选目标是 source/tag 交付。package 与 `cargo publish --dry-run` 仅证明打包，不表示上传 crates.io。
安装包、签名/公证、托管 Release 和部署分别交付。合成模型、File/SQLite 重开与本机 HTTP 捕获
不能替代真实 Provider 效果、GUI/UAT、真实数据、可信 Linux 动态质量或外部 benchmark 验证。
完整流程见[发布清单](release-checklist.md)。
