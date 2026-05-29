# API 表面

SDK API 是主要入口。宿主项目应通过 `bm-sdk` 进入，或通过 `bm-entry` 加协议 adapter 进入；不应自行实现记忆 schema、store envelope、replay 格式或 adapter dispatch 规则。

## Crates

| Crate | 责任 |
| --- | --- |
| `bm-core` | 记忆平面、召回、投影、生命周期、feature 合同和核心错误模型。 |
| `bm-store` | in-memory、file、sqlite、embedded 后端；schema manifest；event log；snapshot；repair report。 |
| `bm-sdk` | `MemoryRuntime` facade、request/report 类型、capability catalog、profile snapshot、store opening re-export。 |
| `bm-replay` | fixture runner、cross-store replay、harness gate 和 benchmark gate。 |
| `bm-evolve` | proposal-only evolution sandbox 和 SDK 写入 helper。 |
| `bm-adapter` | 协议无关 envelope、command、policy、dispatch 和 response 合同。 |
| `bm-entry` | 进程级 runtime opening、profile/auth/source/idempotency 归一化和 adapter response envelope。 |
| `bm-cli` | CLI 命令、capability rendering、platform snapshot 和 memory command execution。 |
| `bm-http`, `bm-wss`, `bm-mcp`, `bm-a2a` | 消费 `bm-entry` 或 `bm-adapter` 的轻量 transport shell，不拥有记忆语义。 |

## Runtime 操作

| 操作 | SDK method | 用途 |
| --- | --- | --- |
| Write | `MemoryRuntime::write` | 存储 procedural memory 或 long-term extraction 结果。 |
| Recall | `MemoryRuntime::recall` | 按 query 取回 memory hits。 |
| Project | `MemoryRuntime::project` | 生成受长度限制的模型上下文 memory block。 |
| Maintain | `MemoryRuntime::maintain` | 在显式配置 LLM client 后执行 post-reply memory maintenance。 |
| Inspect | `MemoryRuntime::inspect` | 返回 recall/operator/lifecycle inspection 数据。 |
| Runtime Skill List / Detail | `MemoryRuntime::list_runtime_skills` / `MemoryRuntime::get_runtime_skill` | 列出和查看运行时沉淀的 procedural memory record，不执行 skill。 |
| Runtime Skill Mutation | `MemoryRuntime::edit_runtime_skill` / `MemoryRuntime::set_runtime_skill_enabled` / `MemoryRuntime::delete_runtime_skill` | 只允许编辑、启停、删除已存在的运行时 Skill；不提供新建、导入或托管标准 Agent Skill。 |
| Agent Skill Directory | `MemoryRuntimeBuilder::agent_skill_dirs` / `add_agent_skill_dir` | 宿主把标准 Agent Skill 目录交给 SDK 只读扫描；SDK 只召回和投影摘要，不添加、不编辑、不执行目录资源。 |
| Agent Tool Registry | `MemoryRuntimeBuilder::agent_tool_registry` / `MemoryRuntime::upsert_agent_tool_registry` | 宿主注册工具索引和 fingerprint；SDK 只基于已治理工具经验返回 `agent_tool_hints`，无经验返回空，不做工具路由。 |
| Replay | `MemoryRuntime::replay` | 检查某个 chat 的 turn ledger 历史。 |
| Export / Import | `MemoryRuntime::export` / `MemoryRuntime::import` | 在 scope 间迁移 continuity snapshot。 |
| Recover / Close | `MemoryRuntime::recover` / `MemoryRuntime::close` | 控制 runtime lifecycle 并产生 lifecycle report。 |

## Request Shapes

最常用的 SDK request types：

| Request type | 必填字段 | 说明 |
| --- | --- | --- |
| `MemoryWriteRequest::Procedural` | `writes`, `source` | 每个 `RuntimeSkillWrite` 包含 `name`、`topic`、`title`、`summary`、`content`、`citations`、`source_chat_id`、`observed_at`。 |
| `MemoryWriteRequest::AgentToolUsageFeedback` | `feedback` | 宿主执行工具后回传 `registry_ref` 和 observation 摘要；SDK 治理后才可能沉淀工具经验。 |
| `MemoryWriteRequest::LongTermExtraction` | `extraction` | 用于 extraction pipeline 已经产出 validated long-term memory extraction 的场景。 |
| `MemoryRecallRequest` | `query`, `limit`, `tool_registry_refs` | 返回运行时 Skill hits、标准 Agent Skill hits、working recall inspection 和经验型 `agent_tool_hints`；无治理经验时 `agent_tool_hints=[]`。 |
| `MemoryProjectionRequest` | `user_query`, `system_max_len`, `recent_messages_limit`, `pressure`, `mode_input`, `tool_registry_refs` | 返回受 `system_max_len` 限制的 `system_memory_block`；标准 Agent Skill 只以只读提示摘要进入上下文，Agent Tool 只以经验 hint 进入，不包含完整 schema。 |
| `MemoryInspectionRequest` | `query`, `system_max_len`, `pressure`, `mode_input` | 返回 capability、lifecycle、operator inspection 数据、Agent Skill 目录扫描报告和 Agent Tool registry 报告。 |
| `RuntimeSkillListRequest` | `query`, `include_disabled`, `include_retired`, `limit` | 返回 `RuntimeSkillListReport`，含 total、active、disabled、runtime_skills 和运行时 Skill 摘要。 |
| `RuntimeSkillDetailRequest` | `name` | 返回 `RuntimeSkillDetailReport`，含 summary/procedure/citations/lineage/strategy diffs/raw content。 |
| `RuntimeSkillEditRequest` | `name`, `title`, `topic`, `summary`, `procedure`, `edit_reason` | 只能编辑已存在且名称以 `runtime_skill__` 开头的运行时 Skill。 |
| `RuntimeSkillSetEnabledRequest` | `name`, `enabled` | 只改变运行时 Skill 启用状态，不改内容。 |
| `RuntimeSkillDeleteRequest` | `name` | 从 skill storage 删除该运行时 procedural memory，不建立配置台墓碑。 |
| `MemoryReplayRequest` | `chat_id`, `limit` | 只做 inspection 的 replay surface。 |
| `MemoryExportRequest` | `chat_id` | 导出 continuity snapshot。 |
| `MemoryImportRequest` | `snapshot`, `target_chat_id`, `mode` | Import mode 是 `BootstrapImport` 或 `FullRestore`。 |
| `MemoryRecoverRequest` | `trigger`, `mode_input` | 执行可恢复 lifecycle recovery。 |
| `MemoryCloseRequest` | `reason` | 发出 close lifecycle report。 |

通用 adapter dispatch 支持 write、recall、project、inspect、recover、replay、export、import、capabilities、close。Maintain 只在调用方通过 `AdapterRuntimeServices` 显式提供 LLM/HTTP services 时执行；未注入 services 的 dispatch 会返回结构化拒绝。

Transport helper crates 会对其声明的 memory operations 使用共享 JSON adapter decoder；subscribe 这类 stream-only operation 仍属于 transport-specific 行为。每种协议的 route/frame/tool/message 表面见 [部署文档](deployment.md)。

## Agent Tool API

Agent Tool 是宿主真实可执行工具。Beetle Memory 不管理、不安装、不执行工具，也不保存完整工具 schema。宿主只把 compact registry snapshot 和 fingerprint 注册给 Memory，用于把历史工具经验绑定到当前工具合同。

SDK / HTTP 的共同语义：

- 有已治理经验时，recall/project 返回 `agent_tool_hints`，其中包含 `tool_id`、`registry_id`、`schema_fingerprint`、经验原因、权限/风险标签和 `host_execution_required=true`。
- 没有已治理经验时，返回 `agent_tool_hints=[]` 和 `tool_experience_status.reason="no_governed_tool_experience"`；宿主自行决定冷启动时暴露哪些工具。
- 宿主按 hint 里的 `tool_id` / `registry_ref` 取自己的完整 schema，再拼接到真正的 LLM tools 参数。
- Memory 返回的 hint 不是授权；权限、用户确认、执行、错误处理和 provider payload 都由宿主负责。

独立 HTTP 部署支持以下 registry 路由：

| Route | Method | 用途 |
| --- | --- | --- |
| `/agent-tool-registries/{id}` | `PUT` | 注册或更新 compact registry snapshot，payload 的 `registry_id` 必须和 path 一致。 |
| `/agent-tool-registries` | `GET` | 返回当前 registry snapshots 和 registry 报告。 |
| `/agent-tool-registries/{id}` | `GET` | 返回单个 registry snapshot。 |
| `/agent-tool-registries/{id}` | `DELETE` | 删除 registry snapshot；不会删除已沉淀的历史经验，但后续 projection 会因 registry 缺失或 fingerprint mismatch 拒绝旧经验。 |

`/memory/write` 可提交：

```json
{
  "tool_usage_feedback": {
    "registry_ref": {
      "registry_id": "host-tools",
      "fingerprint": "current-fingerprint",
      "scope": "global"
    },
    "observations": [],
    "user_visible_result_summary": "工具执行摘要",
    "reuse_outcome": "succeeded",
    "operator_note": null
  }
}
```

`observations` 必须是工具执行后的结构化摘要，不要放完整原始结果、secret 或完整 schema。

## Console API

Console API 只服务独立部署形态的配置台，不属于 SDK 集成方必须暴露的接口。SDK 宿主仍只消费 `bm-sdk` 或 memory adapter surface；宿主自己的配置页、账户页和 UI 由宿主负责。

`bm-entry` 持有 console 状态，`bm-http` 只负责把 `/console/*` 请求路由到 entry console 操作。Console API 不写 memory plane，不实现第二套记忆语义，也不替代 `/memory/*`。

`/console/overview` 的指标来自同一进程内的真实 runtime 状态：系统信息读取当前运行系统、CPU、内存和系统时间；存储占用前半段读取当前 Beetle Memory store 路径实际占用，后半段读取当前系统总存储；写入、召回、投影指标由 `/memory/*` 操作结果回写。配置台前端不得硬编码这些观测值，只能在后端不可达时使用本地占位数据。

| Route | Method | 用途 |
| --- | --- | --- |
| `/console/overview` | `GET` | 返回系统信息、运行形态、观测指标、内核摘要、session 概览和当前记忆上下文。 |
| `/console/skills` | `GET` | 返回运行时 Skill 列表和统计。 |
| `/console/skills/{name}` | `GET` | 返回单条运行时 Skill 详情。 |
| `/console/skills/{name}` | `PATCH` | 编辑已存在的运行时 Skill。 |
| `/console/skills/{name}/enabled` | `PATCH` | 启用或停用运行时 Skill。 |
| `/console/skills/{name}` | `DELETE` | 删除运行时 Skill 记忆。 |
| `/console/llm-gateway` | `GET` | 返回 LLM Gateway 操作面：OpenAI/Ollama/MCP 端点、规则导出命令和 smoke checks。 |
| `/console/llm-gateway/smoke-checks/{id}/run` | `POST` | 运行后端白名单中的 LLM Gateway 验收项，并返回退出码、耗时和受限 stdout/stderr。 |
| `/console/transports` | `GET` | 返回可配置通信入口。 |
| `/console/transports/{id}` | `PATCH` | 更新通信入口开关或 endpoint。 |
| `/console/devices` | `GET` | 返回允许访问设备列表，只包含 app_key 指纹。 |
| `/console/devices` | `POST` | 添加设备，由 runtime 生成一次性 app_key。 |
| `/console/devices/{id}` | `PATCH` | 更新设备状态或名称。 |
| `/console/devices/{id}/rotate-key` | `POST` | 轮换设备 app_key，并仅在响应中返回一次明文。 |
| `/console/session` | `GET` | 返回已配对 session 的账户和主体摘要。 |

安全边界：

- 列表接口永远不返回 app_key 明文，只返回 `appKeyFingerprint`。
- 新增和轮换设备时，`appKeyOnce` 只在该次响应中返回。
- 通信页的 HTTP 开关表示外部 memory HTTP API，不表示配置台自身的 HTTP console 入口。
- Skill 管理页只管理运行时 procedural memory，不新增、不导入标准 Agent Skill，不提供 marketplace、executor 或 workflow runner。
- 标准 Agent Skill 通过 SDK builder 或独立部署的 `BM_AGENT_SKILL_DIRS` 配置只读挂载；运行时只扫描 `SKILL.md` 摘要、资源计数和指纹，召回时不读取或执行 scripts/assets。
- LLM Gateway 验收运行接口只接受后端已知的 smoke check `id`，不执行前端传入的任意命令。

## Capability Catalog

每个 runtime 都暴露 `MemoryCapabilityCatalog`。能力可见性由所选 profile、compiled features、runtime policy 和 privacy policy 共同决定。

```rust
let capabilities = runtime.capabilities();
assert!(capabilities.write.visible);
assert!(capabilities.recall.visible);
```

通过 CLI 输出稳定 platform snapshot：

```bash
cargo run -p bm-cli --bin bm -- \
  platform capability-snapshot \
  --profile profile-server-linux-memory-gateway
```

## 边界

外部代码可以选择 profile、打开受支持的 store backend、调用 SDK 操作并消费 report。外部代码不能绕过 `MemoryRuntime` 写记忆状态，也不能实现一条语义不同的 adapter/store 并行路径。
