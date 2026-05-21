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
- [dev-docs/code-quality-governance.md](dev-docs/code-quality-governance.md)：完整搬迁后的代码质量治理标准。
- [dev-docs/code-quality-audit.md](dev-docs/code-quality-audit.md)：代码质量治理第一刀审计报告与收敛台账。
- [dev-docs/sdk-profile-contract-plan.md](dev-docs/sdk-profile-contract-plan.md)：SDK/Profile 阶段实施真源。
- [dev-docs/store-backend-schema-plan.md](dev-docs/store-backend-schema-plan.md)：Store Backend + Schema 阶段实施真源。
- [dev-docs/runtime-lifecycle-plan.md](dev-docs/runtime-lifecycle-plan.md)：Runtime Lifecycle 阶段实施真源。
- [dev-docs/replay-sandbox-plan.md](dev-docs/replay-sandbox-plan.md)：Replay / Harness / Evolution Sandbox 阶段实施真源。
- [dev-docs/adapter-communication-plan.md](dev-docs/adapter-communication-plan.md)：Adapter / Communication 阶段实施与验收真源。
- [dev-docs/platform-compile-gates-plan.md](dev-docs/platform-compile-gates-plan.md)：Platform Compile Gates 阶段验收真源。
- [dev-docs/release-surface-plan.md](dev-docs/release-surface-plan.md)：Release Surface 阶段实施与验收真源。
- [dev-docs/procedural-memory-and-skill.md](dev-docs/procedural-memory-and-skill.md)：skill / procedural memory 边界。
- [dev-docs/profile-and-platform-boundary.md](dev-docs/profile-and-platform-boundary.md)：profile 与平台边界。
- [dev-docs/soul-and-subject-memory-boundary.md](dev-docs/soul-and-subject-memory-boundary.md)：灵魂治理与主体记忆边界。
- [dev-docs/communication-and-adapter-boundary.md](dev-docs/communication-and-adapter-boundary.md)：通信与 adapter 边界。
- [dev-docs/beetle-source-audit-and-capability-map.md](dev-docs/beetle-source-audit-and-capability-map.md)：Beetle 代码真源审计。
- [dev-docs/full-port-plan.md](dev-docs/full-port-plan.md)：Beetle 记忆真源完整搬迁与验收真源。
- [dev-docs/post-port-roadmap.md](dev-docs/post-port-roadmap.md)：完整搬迁后的后续实施路线图。

## Public Docs

- [docs/api.md](docs/api.md)：公开 crate / API surface。
- [docs/sdk-quickstart.md](docs/sdk-quickstart.md)：非来源项目 SDK 接入 quickstart。
- [docs/profile-matrix.md](docs/profile-matrix.md)：七个 first-class profile 的裁剪矩阵。
- [docs/store-backends.md](docs/store-backends.md)：store backend 选择、约束和 ownership boundary。
- [docs/replay-migration.md](docs/replay-migration.md)：replay、snapshot migration 和 evolution proposal 边界。
- [docs/adapter-contract.md](docs/adapter-contract.md)：SDK / CLI / HTTP / Webhook / WSS / MQTT / MCP / A2A adapter 合同。
- [docs/operator-inspection.md](docs/operator-inspection.md)：operator inspection、recover、close 和 lifecycle diagnosis。
- [docs/release-checklist.md](docs/release-checklist.md)：release metadata、feature matrix、package audit 和 publish dry-run 清单。

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
- `crates/sdk`：当前提供 SDK-first `MemoryRuntime` facade，并 re-export `StorePlatform` / `StoreBackendConfig` 作为普通宿主 store opening 入口；普通宿主通过 `MemoryRuntime::builder().store_platform(platform)` 接入，不手写 core store trait，也不通过 SDK facade 操作 store snapshot envelope internals。
- `crates/store`：当前提供 in-memory、file、sqlite、embedded 四类后端、schema manifest、event log、snapshot envelope、repair report 和跨后端一致性测试。
- `crates/replay`：当前提供 fixture schema、SDK-driven runner、cross-store replay、memory harness gate、benchmark gate 和 profile validation capability，不引入某个宿主特权。
- `crates/evolve`：当前提供 proposal-only evolution sandbox 合同、profile policy 和 SDK write governance commit helper；sandbox 不直接写 store。

Adapter / Communication 协议合同层已落地，见 [dev-docs/adapter-communication-plan.md](dev-docs/adapter-communication-plan.md)。`bm-adapter` 是协议无关 command/envelope/policy/report 合同层；`bm-cli`、`bm-http`、`bm-wss`、`bm-mqtt`、`bm-mcp`、`bm-a2a` 只作为 thin adapter 消费 `MemoryRuntime`，不能在内核外分叉记忆语义。当前不引入真实网络 server/listener 依赖。

当前第一轮代码质量治理已经完成：SDK-only host contract 已补齐，sqlite/index 后端改为显式 `sqlite-index` feature，ESP standalone / embedded SDK profile 不再拉入 `rusqlite`，未使用的 `base64` / `urlencoding` 已移除。

当前验收已经通过 `cargo fmt --all -- --check`、`cargo check --workspace`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`bash scripts/check_profile_matrix.sh`、`cargo test -p bm-core --features sqlite-index`、`cargo clippy -p bm-core --features sqlite-index --all-targets -- -D warnings`，并完成 Beetle 专属 adapter/source kind 与外部通信服务实现的漂移扫描。

Store Backend + Schema 已落地：`bm-store` 已从内存辅助工具推进为 Beetle Memory 自有持久化层，由本项目实现 in-memory、file、sqlite、embedded 后端、schema、event log、snapshot 和 repair report；manifest 强校验 schema/backend/profile/memory_system_kind，snapshot 同时校验 state fingerprint 与 event fingerprint，导入前校验 envelope/manifest/namespace/lineage，embedded 执行 snapshot byte budget 且不静默截断 event lineage。集成方只配置后端与容量，不实现记忆 schema、写入语义或恢复逻辑。本阶段验收脚本为 `bash scripts/check_store_backend_contract.sh`。

Runtime Lifecycle 已落地：`bm-core::runtime` 已提供 `RuntimeLifecycleEngine`、mode/admission、report、event sink 和 operator diagnosis；`MemoryRuntime` 的 open/close/recover/write/recall/project/maintain/inspect/replay/export/import 都返回 lifecycle report 并写入 `runtime.lifecycle` / `operator.action` event；`StorePlatform` 持久化生命周期事件并保持 snapshot import/export event lineage；capability catalog 已区分 ESP standalone 与 ESP embedded SDK 的 lifecycle 能力。本阶段验收脚本为 `bash scripts/check_runtime_lifecycle_contract.sh`。

Replay / Harness / Evolution Sandbox 已落地：`bm-replay` 已提供 fixture/runner/harness/benchmark gate，`bm-evolve` 已提供 proposal-only sandbox 与 SDK commit helper，validation capability 已区分 ESP standalone、ESP embedded SDK、Linux device、desktop、server profile。本阶段验收脚本为 `bash scripts/check_replay_sandbox_contract.sh`。

Platform Compile Gates 已落地，见 [dev-docs/platform-compile-gates-plan.md](dev-docs/platform-compile-gates-plan.md)：SDK / adapter profile feature forwarding、七个 first-class profile 的 capability snapshot、dependency budget、cross-target host gate 和总验收脚本已经进入工程门禁。具备 ESP、Linux、Windows 目标工具链的 CI / release 环境继续运行 strict target gate。

Release Surface 已落地，见 [dev-docs/release-surface-plan.md](dev-docs/release-surface-plan.md)：公开 API 文档、SDK quickstart、profile matrix、store / replay / adapter / operator guide、六个非来源项目 examples、license/package metadata、publish dry-run 和 `scripts/check_release_surface.sh` 已进入发布面门禁。

当前主线已经完成完整搬迁后的 SDK/Profile、Store、Runtime、Replay/Sandbox、Adapter/Communication、Platform Gates 和 Release Surface 闭环；下一阶段必须另起真源文档定义，不能把 UI、管理控制台、executor、workflow runner、skill marketplace、来源项目专属 adapter 或真实网络 listener 直接塞入当前主线。
