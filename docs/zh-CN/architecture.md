# 架构文档

Beetle Memory 只有一套 memory runtime，但有多个入口表面。SDK、CLI、HTTP、WebSocket、MCP、A2A 最终都进入 `MemoryRuntime`；协议 crates 不能实现第二套记忆语义。

Workspace crates 是并列 crate。核心依赖方向是：

```text
bm-core <- bm-sdk（私有 persistence kernel）
bm-sdk <- bm-adapter <- bm-entry
bm-entry <- bm-cli / bm-http / bm-wss / bm-mcp / bm-a2a
```

`bm-entry` 同时依赖 `bm-sdk` 和 `bm-adapter`：它打开 SDK runtime，再把 adapter envelope 派发进这套 runtime。

## 分层图

```text
宿主应用或独立部署进程
  -> bm-sdk 或 bm-entry
    -> bm-adapter（协议入口场景）
      -> bm-sdk::MemoryRuntime
        -> bm-core memory / skill / lifecycle / profile / recall contracts
        -> 私有 bm-sdk persistence kernel
```

| 层 | Crates | 责任 |
| --- | --- | --- |
| 记忆内核 | `bm-core` | 记忆平面、召回、投影、生命周期、feature/profile 合同、skill as procedural memory、task 与 continuity primitives。 |
| 持久化 | 私有 `bm-sdk` 模块 | in-memory、file、sqlite、embedded stores；event log；schema manifest；snapshot envelopes；repair reports。宿主只拿到 `MemoryStoreHandle`。 |
| SDK facade | `bm-sdk` | 公开 runtime builder、不透明 `MemoryStoreHandle`、operation request/report types 和 capability catalog。 |
| Replay/evolution | `bm-replay`, `bm-evolve` | 开发用 fixture replay、cross-store validation、harness/benchmark 验收门禁和 proposal-only evolution sandbox。`nonproduction-replay-harness` 不是部署能力。 |
| Entry runtime | `bm-entry` | 进程级 store/runtime opening，以及 identity、scope、auth、transport、idempotency 归一化。 |
| Adapter contract | `bm-adapter` | 协议无关 envelope、command、operation、dispatch 和 response model。 |
| Transport shells | `bm-cli`, `bm-http`, `bm-wss`, `bm-mcp`, `bm-a2a` | 解码 transport input，构造 adapter command，调用 `EntryRuntime`，渲染协议输出。 |

## 主调用链

内嵌 SDK 路径：

```text
host code
  -> MemoryStoreHandle::open(StoreBackendConfig)
  -> MemoryRuntime::builder().store(handle)
  -> runtime.write / recall / project / maintain / inspect / replay / export / import / recover / close
```

独立入口路径：

```text
transport request
  -> transport crate decoder
  -> EntryTransportContext + AdapterCommand
  -> EntryRuntime::handle()
  -> AdapterEnvelope<AdapterCommand>
  -> dispatch_adapter_command()
  -> MemoryRuntime operation
  -> AdapterResponse
```

## Memory Operations

| Operation | Runtime method | 典型调用方 |
| --- | --- | --- |
| Write | `MemoryRuntime::write` | SDK host、CLI、HTTP write candidate |
| Recall | `MemoryRuntime::recall` | SDK host、CLI、HTTP、WebSocket、MCP、A2A |
| Project | `MemoryRuntime::project` | 组装模型上下文的 SDK host 或 CLI |
| Maintain | `MemoryRuntime::maintain` | 显式注入 LLM client 的 SDK host |
| Inspect | `MemoryRuntime::inspect` | 运维工具和健康检查 |
| Replay | `MemoryRuntime::replay` | 迁移验证和调试 |
| Export / Import | `MemoryRuntime::export` / `MemoryRuntime::import` | Snapshot migration |
| Recover / Close | `MemoryRuntime::recover` / `MemoryRuntime::close` | Runtime lifecycle control |

`Maintain` 不由通用 adapter dispatch 执行，因为它需要显式 LLM/HTTP service injection。协议集成只有在自己拥有这个依赖注入边界后才应暴露 maintain。

## 数据流

1. 宿主选择 `ProfileId` 并打开受支持的 store backend。
2. `MemoryRuntime` 根据 profile、compiled features、runtime policy、privacy policy 解析 capability catalog。
3. Write 操作通过 `bm-core` 更新受规则约束的记忆状态，并由私有 `bm-sdk` persistence kernel 持久化。
4. Recall 和 projection 通过 runtime facade 读取 recent/session/procedural/long-term/continuity 数据。
5. Lifecycle events 和 operator reports 以结构化 report 返回，不隐藏为不可见副作用。
6. Export/import 和 replay 使用 snapshot 与 event-lineage 合同，让迁移可解释。

## Profile And Store Boundaries

Profile 不是标签，而是编译和运行合同：

- ESP profile 可以使用 `embedded` 或 `in-memory` store，并拒绝 `file`/`sqlite`。
- Linux device、desktop、server profile 在启用对应 features 后可以使用 file 或 sqlite store。
- Server gateway profile 可以暴露协议 adapters；embedded SDK profile 默认走进程内 SDK。
- `profile-server-linux-dev-full` 是具有 replay 和 benchmark 验收表面的开发 profile；`nonproduction-replay-harness` 不可部署。

## 部署边界

Beetle Memory 提供 memory runtime、SDK、私有 persistence kernel、entry runtime 和 adapter shells。产品专属表面和部署基础设施由宿主系统提供；记忆状态仍经过 `MemoryRuntime`。
