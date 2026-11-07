# Beetle Memory 开发者文档

Beetle Memory 通过 `bm-sdk` 集成，或通过 `bm-entry` 加协议 adapter 独立部署。[0.14.0 macOS SDK 源码说明](release-notes-0.14.0.md)介绍视觉感知、受治理的记忆与 Store v18；[0.13.0 源码说明](release-notes-0.13.0.md)记录第一人称宿主体交付。更广的 SDK 合同见[集成文档](integration.md)、[API 表面](api.md)与[快速开始](getting-started.md)。版本只有完成 Git tag 验证后才算发布。

- [架构文档](architecture.md)
- [集成文档](integration.md)
- [LLM Gateway 集成](llm-gateway-integrations.md)
- [部署文档](deployment.md)
- [CLI 使用](cli-usage.md)
- [快速开始](getting-started.md)
- [API 表面](api.md)
- [Profile 矩阵](profiles.md)
- [存储后端](store-backends.md)
- [Adapter 合同](adapters.md)
- [回放与归档](replay-and-archive.md)
- [运维与检查](operator-guide.md)
- [发布清单](release-checklist.md)
- [0.14.0 macOS SDK 源码发布说明](release-notes-0.14.0.md)
- [0.13.0 源码发布说明](release-notes-0.13.0.md)
- [0.12.0 源码发布说明](release-notes-0.12.0.md)
- [0.11.2 源码发布说明](release-notes-0.11.2.md)
- [0.11.1 源码发布说明](release-notes-0.11.1.md)
- [0.11.0 源码发布说明](release-notes-0.11.0.md)
- [0.10.0 源码发布说明](release-notes-0.10.0.md)
- [0.9.0 源码发布说明](release-notes-0.9.0.md)
- [0.8.0 源码发布说明](release-notes-0.8.0.md)
- [0.7.0 源码候选说明](release-notes-0.7.0.md)
- [0.6.0 源码候选说明](release-notes-0.6.0.md)
- [0.5.0 发布说明](release-notes-0.5.0.md)
- [0.4.0 发布说明](release-notes-0.4.0.md)
- [0.3.0 发布说明](release-notes-0.3.0.md)

Memory Evidence System 和 Conversation Transcript Substrate 发布面由 [API 表面](api.md) 与 [回放与归档](replay-and-archive.md) 承载。Transcript commit、lifecycle、redacted replay 和 transcript export 是由 `MemoryRuntime` 拥有的 SDK surface。
