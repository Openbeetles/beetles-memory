# Beetle Memory

`Beetle Memory` 是一个 Rust SDK-first 的 Agent Memory Runtime。目录名 `agent-memory` 只是当前工程目录名，不是项目名称。

项目目标是把 Beetle 已经验证过的记忆体系抽象成独立、可部署、可硬件裁剪、可 SDK 集成、可协议调度、可迁移、可审计、可回放的 Agent 记忆基础设施。

它不是普通 RAG、不是向量数据库、不是聊天历史模块，也不是 Beetle 的子模块。Beetle 是第一抽取源、第一迁移对象和第一回接验收目标，不是产品边界。

## 当前真源

- [AGENTS.md](AGENTS.md)：本项目 agent 宪法入口。
- [dev-docs/README.md](dev-docs/README.md)：内部开发文档索引。
- [dev-docs/project-initiation.md](dev-docs/project-initiation.md)：当前立项真源。
- [dev-docs/agent-constitution.md](dev-docs/agent-constitution.md)：架构宪法。
- [dev-docs/engineering-workflow.md](dev-docs/engineering-workflow.md)：工程工作流与验收门禁。
- [dev-docs/procedural-memory-and-skill.md](dev-docs/procedural-memory-and-skill.md)：skill / procedural memory 边界。
- [dev-docs/profile-and-platform-boundary.md](dev-docs/profile-and-platform-boundary.md)：profile 与平台边界。
- [dev-docs/soul-and-subject-memory-boundary.md](dev-docs/soul-and-subject-memory-boundary.md)：灵魂治理与主体记忆边界。
- [dev-docs/communication-and-adapter-boundary.md](dev-docs/communication-and-adapter-boundary.md)：通信与 adapter 边界。
- [dev-docs/s0-implementation-plan.md](dev-docs/s0-implementation-plan.md)：S0 Rust workspace 实施计划。

## 一等目标

- 硬件编译：ESP、Linux 硬件设备。
- 端和服务器编译：macOS、Windows、Linux server。
- SDK 集成：Beetle 回接、agent-tools 协同、任意其他 AI 项目作为记忆系统集成 SDK。

## 首版边界

首版聚焦 memory kernel、memory planes、write governance、recall report、projection preview、privacy gate、procedural memory、profile compiler、event recorder、replay fixture、Rust SDK builder、CLI inspection 和 Beetle bridge 入口。

首版不实现 skill executor、workflow runner、tool runtime、skill marketplace、完整 Web 控制台、非 Rust SDK 或分布式商业权限平台。

## 当前 S0 骨架

当前 Rust workspace 已按短目录建立：

- `crates/core`：核心合同、domain、plane、profile、write、recall、projection、adapter 预留类型。
- `crates/store`：store trait 与 `InMemoryStore`。
- `crates/sdk`：`MemoryRuntime` 与 `MemoryRuntimeBuilder`。
- `crates/replay`：基础 replay fixture。
- `crates/bridge-beetle`：Beetle 迁移合同与 source provenance。

S0 已跑通 `WriteCandidate -> MemoryRuntime -> write governance -> store -> WriteReport -> RecallSelectionReport -> ProjectionReport -> replay` 的最小闭环。Beetle 旧 memory 主体实现还没有迁入；后续只能按当前合同逐段吸收。
