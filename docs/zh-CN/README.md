# Beetle Memory 开发者文档

Beetle Memory 通过 `bm-sdk` 集成，或通过 `bm-entry` 加协议 adapter 独立部署。先看架构，再按需要进入 SDK 集成、独立部署或 CLI 运维路径。

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

Memory Evidence System 和 Conversation Transcript Substrate 发布面由 [API 表面](api.md) 与 [回放与归档](replay-and-archive.md) 承载。Transcript commit、lifecycle、redacted replay 和 transcript export 是由 `MemoryRuntime` 拥有的 SDK surface。
