# Beetle Memory 0.14.0 macOS SDK 源码发布说明

本版将视觉作为通用感官合同的首个实现。交付与验收范围仅为 macOS `bm-sdk` 及其所需的 Core/Store 源码；仓库源码 tag 不代表 crates.io 上传、安装包、其他平台构建、真实 Provider 或下游 App 已发布。正式源码身份须以发布时验证的 `v0.14.0` Git tag 为准。

## 感官输入与模型交付

- 通用 `PerceptionInputEnvelope` 承载来源、时间、主体范围和撤权代次；媒体及摘要只在视觉载荷 `VisualInput` 中。视觉可提交图片、按时间排序的连续帧或无音轨短视频。宿主采集的画面与用户主动分享的照片使用不同来源；后者必须绑定真实的用户消息和附件。
- 宿主通过 `register_perception_host`、`register_visual_tool_snapshot`、`authorize_visual_source` 和 `submit_visual_input` 登记设备、工具状态、来源授权与媒体。能力按宿主和设备呈现为不存在、已安装但不可用、当前可用；关闭视觉能力不禁止处理用户主动分享的照片。
- `project_for_perception_host` 只为当前宿主设备声明可调用的视觉工具。回合中途可用 `append_visual_delivery` 取得新增媒体包；宿主用 `attest_visual_delivery` 回传最终请求的实际媒体与转换记录，并可用 `record_visual_provider_response` 补上 Provider 响应证明。画面作为受限感官来源，不应伪装成用户发言。
- `prepare_turn_visual_external_uses` 只准备 Beetle 已知的视觉来源。若视觉回复要进入持久学习，宿主须用 `submit_turn_external_content_manifest` 声明该回合**全部**外部内容和最终请求；其他工具结果及外部材料也须列入。清单完整性是宿主声明，Beetle 会核对已知视觉回执、回放 owner 与工具观察，不能独立证明宿主没有添加未声明内容。来源或回执不闭合时，视觉影响的回复不得作为无来源文本学习。

## 视觉记忆与撤源

宿主决定何时查看，并负责摄像头驱动、系统权限、界面提示、发送前敏感画面拦截、去元数据、压缩、音轨移除及发送给模型的接收方授权。原始临时媒体在记忆准入前只驻内存。Beetle 通过受授权的 `BeetleVisualInferenceClient` 自主解释画面，决定普通瞬时画面是否值得形成语义候选，以及是否确有需要保留受限媒体证据；宿主不能提交最终事实或替 Beetle 决定记什么。

`list_visual_memory`、`read_visual_memory_media`、`forget_visual_memory` 和 `forbid_visual_source_memory` 提供受治理的查看、媒体读取、单次删除与来源禁记。来源链覆盖视觉经历、受影响的 Transcript/Session 回放、长期事实及 Soul/关系成长；撤权、删除和禁记必须使依赖来源失效。普通家庭背景本身不等于私密内容；第三方、未成年人、私密内容及身份不确定性由类型化规则限制持久化。默认不做人脸识别。能力关闭与来源撤权按宿主、设备和代次分别生效。

视觉参与主体成长需要无关系范围的来源、至少两个原始回合的视觉证据，以及本轮独立的规范用户或运行时文字观察；单次画面、视觉模型自己的描述与助手回复都不能单独改变 Soul。关系范围的视觉证据只参与该关系的成长。

跨关系回放使用本轮挂载的关系身份复核来源；原关系的 Transcript 消息不能在另一关系冒充当前对话，仅已准入的用户范围长期事实可跨关系使用。

## Store 与归档

Store schema 升为 **v18**，只接受精确的 v18 身份。没有 v17 兼容读取或自动迁移；打开旧 Store 应明确失败且不改写旧数据。Archive import 也不承担 schema migration。

`export_memory_space` 默认导出有来源的**语义**视觉记录，不附带媒体字节；`export_memory_space_with_visual_media` 是显式媒体导出入口，受私密材料策略、精确 owner、SHA、作用域和预算约束。`import_memory_space` 原子替换 JSON 与获准媒体，并检查本地墓碑、撤权、禁记和当前更严格的保留上限；已有视觉 episode 或来源依赖根不能由归档无声删除或改写。用户界面的视觉记忆管理读取与普通主体投影、operator raw 检查分开。

跨设备同步须先同步来源撤销、禁记与墓碑权威；一个从未收到这些权威的新 Store 无法凭旧归档自行推知远端发生过删除。本版不声称提供跨设备同步服务。

## 验收边界

本次发布只验收 macOS SDK 的定向合同和源码身份。真实摄像头、视觉 Provider、宿主完整请求回执、下游界面及多设备实机行为需要各宿主单独集成验收；源码 API 本身不证明这些宿主侧步骤已经完成。
