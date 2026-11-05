# Beetle Memory 0.12.0 源码发布说明

本源码版本是以 SDK 为入口的破坏性版本。正式发布应以 `v0.12.0` Git tag、远端回读及已记录的检查结果为证据；本文本身不证明这些结果。源码发布不等于 crates.io 上传、二进制、安装包、托管 Release 或部署。

## SDK 回合合同

`CanonicalTurnDelta.assistant_messages: Vec<TranscriptAssistantMessage>` 取代单值 `assistant_message`。助手消息保持顺序，每条分别标记 `Complete` 或 `InterruptedDeliveredPrefix`。`Delivered` 回合至少有一条完整助手消息。`Cancelled`、`UpstreamFailed` 和 `IncompleteStream` 可保存已完整结算的消息，以及末尾最多一条已结算的中断已交付前缀；未结算流式片段不提交。`UserOnly` 与 `RejectedByPolicy` 不含助手消息。超过 16 条助手消息或任一消息正文超过 4 KiB 时，intake 拒绝整个回合，不截断。精确重试必须复用原 canonical payload，包括消息顺序与结算状态。

Replay、timeline、search、export 和来源引用消费有序消息。后置学习仅接纳与回合状态相符的已结算证据；中断不会把暂态文本晋升为行动或工具能力。`DeleteRaw` 在同一受治理闭包内清除受影响的 transcript 原文、Session shadow、attrs/aux、host ref label、索引及依赖摘要文本。已接受的长期事实仍须通过其独立治理面处理。

## Store 与接入

Store v17 是 clean break，不读取 Store v16，也不提供兼容读取、双写或自动迁移。打开旧 Store 必须明确失败且不改写。不要将本版本指向真实 Store 的唯一副本；独立数据过渡由 owner 决定。Archive import 不是 schema migration。同一宿主的全部 Beetle crate 必须来自同一源码 tag。接入入口见[快速开始](getting-started.md)、[集成文档](integration.md)、[API 表面](api.md)和[存储后端](store-backends.md)。

已发布的 v0.11.2 tag 仍对应 Store v16 合同，本版本不改写该历史版本。Core/SDK/Store 本地检查、发布门禁、tag/远端回执、真实数据、Provider、GUI 与目标平台证据必须分别报告；本文不声称未经记录的验收或发布已完成。
