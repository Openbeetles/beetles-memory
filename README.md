# Beetle Memory

`Beetle Memory` 是一个 Rust SDK-first 的 Agent Memory Runtime。目录名 `agent-memory` 只是当前工程目录名，不是项目名称。

项目目标是把 Beetle 项目里已经验证过的记忆模块单独提出来，整理成独立、可部署、可硬件裁剪、可 SDK 集成、可协议调度、可迁移、可审计、可回放的 Agent 记忆基础设施。

它不是普通 RAG、不是向量数据库、不是聊天历史模块，也不是 Beetle 的子模块。在当前抽离工作中，Beetle 是记忆系统的唯一代码真源、参考源和代码提炼源；这是研发来源事实，不是调用方绑定、部署绑定或项目身份绑定。

代码真源不等于架构上限。Beetle Memory 必须以独立记忆基础设施的姿态设计自己的 SDK、runtime/server、profile、store、adapter、replay、inspection、operator 和硬件裁剪能力；如果项目改名、开源发布，或原项目永远不作为宿主接入 SDK，这套架构仍必须成立。

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
- [dev-docs/beetle-source-audit-and-capability-map.md](dev-docs/beetle-source-audit-and-capability-map.md)：Beetle 代码真源审计。
- [dev-docs/full-port-plan.md](dev-docs/full-port-plan.md)：Beetle 记忆真源完整搬迁与验收真源。

## 一等目标

- 硬件编译：ESP、Linux 硬件设备。
- 端和服务器编译：macOS、Windows、Linux server。
- SDK 集成：任意 AI 项目作为记忆系统集成 SDK；Beetle 只作为当前代码真源和工程取证来源。

## 首版边界

首版聚焦 memory kernel、memory planes、write governance、recall report、projection preview、privacy gate、procedural memory、profile compiler、event recorder、replay fixture、Rust SDK builder、CLI inspection，以及 Beetle 真源审计和对照 fixture 边界。

首版不实现 skill executor、workflow runner、tool runtime、skill marketplace、完整 Web 控制台、非 Rust SDK 或分布式商业权限平台。

## 当前状态

旧轻量实现和旧编号测试已经清场。当前核心迁入已经按 Beetle 记忆代码真源落到 `bm-core`：

- `crates/core/src/memory`：长期记忆、archive、召回、投影、维护、harness、ledger、灵魂治理、主体记忆、隐私、profile。
- `crates/core/src/skills`：manual skill、runtime skill、capability atom、prompt cache。
- `crates/core/src/agent`：subject state、soul feedback、active work、context assembly facade。
- `crates/core/src/runtime`：soul kernel、runtime mode、workflow audit、bounded system inbound scheduling facade。
- `crates/core/src/platform`：memory operator surface 与通用 store/platform trait。
- `crates/sdk`、`crates/store`、`crates/replay`、`crates/evolve`：当前提供通用 facade / 内存 store / replay/evolve 出口，不引入某个宿主特权。

当前未实现外部通信 adapter；HTTP、Webhook、WSS、MQTT、MCP、CLI 仍只在文档中固定边界，不能在内核里分叉记忆语义。

当前验收已经通过 `cargo fmt --all -- --check`、`cargo check --workspace`、`cargo test --workspace`、`cargo test -p bm-store --features sqlite`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo clippy -p bm-store --features sqlite --all-targets -- -D warnings`，并完成 Beetle 专属 adapter/source kind 与外部通信服务实现的漂移扫描。
