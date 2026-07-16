# LLM Gateway 集成文档

`bm-llm-gateway` 让现有 IDE、coding agent 和本地模型工具通过它们已经支持的协议接入 Beetle Memory。当前发布面同时包含显式 gateway 模式，以及由 `bm-ollama-transparent` 提供的 macOS 本地 Ollama App 透明 controller；两种模式的模型流量都进入同一个 `bm-llm-gateway` owner。

## Ollama App 透明模式

`bm-ollama-transparent` 是已发布的本地 Ollama App 透明模式 controller。它把官方 listener 从 `127.0.0.1:11434` 移到 `127.0.0.1:11435` 的 managed upstream，再让 `bm-llm-gateway` 监听公开 Ollama endpoint；它不实现第二套模型 gateway 或 memory runtime。

Enable 流程 fail closed：

- 停止官方 listener 必须获得显式同意；
- preflight 把 stop plan 绑定到端口 owner 的精确 PID、进程启动身份、command，以及 executable 的内容/device/inode 身份，执行 signal 前会立即重验同一 receipt；进程名和 classifier 只用于诊断，绝不授权 signal；
- macOS managed child 由唯一 identity 的 `launchd` job 承载。owner-only 控制记录把诊断 process receipt 与可恢复 job authority 分开；controller 重启后，只有 canonical label、当前用户 bootstrap target、launchd live PID、start identity 与 executable identity 全部精确一致才允许重新接管。单独 PID receipt 仍不得授权 stop；
- 唯一的非阻塞 OS 文件 lease 对完整 enable、rollback 或 disable 流程做跨 controller 进程 fence。retained lock file 会持久化并回读验证 holder 的 PID、启动身份、executable path、device/inode 和 SHA-256 receipt，并发 transition 会被拒绝，不会交错执行；
- managed runner 通过 retained directory 无覆盖发布，以 SHA-256 标识，并在执行前重验身份后从 retained descriptor 启动；
- gateway sidecar path 必须由配置显式传入绝对路径。controller 以 no-follow 打开、验证 SHA-256 和 metadata，并从 retained descriptor 执行；环境变量、相对路径和当前目录发现都不是生产路径；
- 本地 HTTP probe 有固定 response byte budget，会拒绝超限或持续不结束的响应。

透明 controller 仅用于 macOS 本地 loopback。Desktop 把同一个 typed memory authority（`owner_id`、`agent_id`、`channel` 和唯一绝对 store path）同时交给 `EntryRuntime` 与 transparent gateway；controller 不提供 fallback owner、agent、federation 或独立 store。调用方必须消费 typed preflight 和 transition report，不能只根据进程名或端口已打开推断成功。

## Endpoint

默认本地 endpoint：

```text
OpenAI-compatible gateway: http://127.0.0.1:8787/v1
Ollama native gateway:     http://127.0.0.1:8787/api
MCP Streamable HTTP:       http://127.0.0.1:8788/mcp
```

OpenAI-compatible route：

- `GET /v1/models`
- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/embeddings`
- `GET /v1/bm/provider-capabilities`

Ollama native route：

- `GET /api/tags`
- `GET /api/version`
- `POST /api/chat`
- `POST /api/generate`
- `POST /api/embed`
- `POST /api/embeddings`
- `POST /api/show`

Chat、generate 和无状态 responses 请求会在上游模型请求前注入确定性的 Beetle Memory projection。Embeddings 和模型管理 route 是 passthrough，不触发 projection 或 maintenance。

## Console 操作面

共享配置台提供独立的 LLM 网关页面，数据来自 `GET /console/llm-gateway`。页面只展示协议端点、`bm agent-rules export` 命令和本地 smoke gate；系统级记忆上下文归首页总览的 `GET /console/overview` 持有。模型协议仍由 `bm-llm-gateway` 持有，配置台不实现 OpenAI 或 Ollama 协议逻辑。

## 共享 Runtime

用同一组 `BM_MEMORY_*` 环境变量让 `bm-llm-gateway` 和 `bm-mcp-server` 命中同一套 Beetle Memory runtime：

```bash
export BM_MEMORY_STORE_FILE=/var/lib/beetle-memory/gateway-store
export BM_MEMORY_OWNER_ID=owner-default
export BM_MEMORY_AGENT_ID=agent-main
export BM_MEMORY_CHANNEL=llm.gateway
export BM_MEMORY_CHAT_ID=chat-1
```

`bm-mcp-server` 也支持显式本地覆盖：

```bash
bm-mcp-server stdio --store-file /var/lib/beetle-memory/gateway-store --chat-id chat-1
bm-mcp-server http --addr 127.0.0.1:8788 --store-file /var/lib/beetle-memory/gateway-store --chat-id chat-1
```

sqlite 部署用 `BM_MEMORY_STORE_SQLITE=/path/to/memory.sqlite3`；`BM_MEMORY_STORE_MEMORY=1` 只用于一次性本地测试。持久化 file/sqlite 路径必须显式传入绝对路径；binary 不再默认写入源码树下的 `target/` store。

## MCP Server

`bm-mcp-server` 实现 MCP lifecycle handshake、带 `inputSchema` 的 `tools/list`、带 MCP `content` 和 `structuredContent` 的 `tools/call`，以及安全的 `resources/list` / `resources/read` 结果。它只暴露受治理的 memory tools 和 safe resources，不暴露 raw private memory planes。

## 规则导出

生成面向具体工具的配置，但不嵌入任何真实记忆内容：

```bash
bm agent-rules export \
  --target continue \
  --gateway-url http://127.0.0.1:8787/v1 \
  --mcp-url http://127.0.0.1:8788/mcp
```

支持的 target：

- `continue`
- `cline`
- `aider`
- `zed`
- `opencode`
- `open-webui`
- `vscode`

导出内容只指向 gateway 和 MCP endpoint。它不能包含 raw memory、projection payload 或 private store data。

## Recipe

### Aider

使用 OpenAI-compatible gateway：

```bash
export OPENAI_API_BASE=http://127.0.0.1:8787/v1
export OPENAI_API_KEY=beetle-memory-local
aider --model openai/beetle-memory
```

用 `bm agent-rules export --target aider ...` 生成 `CONVENTIONS.md` 片段，指向 gateway 并禁止粘贴 raw memory。

### Continue

用 `bm agent-rules export --target continue ...` 生成 `models` 和 `mcpServers` 片段并合入 Continue 配置。模型流量走 OpenAI-compatible gateway；显式记忆工具走 MCP。

### Cline

模型 provider 选择 OpenAI Compatible，base URL 设置为 `http://127.0.0.1:8787/v1`。把 `cline` 导出内容放入 `.clinerules/memory.md`。规则应指向 MCP tools，而不是写入记忆内容。

### Zed

用 `bm agent-rules export --target zed ...` 生成 JSON settings 片段，包含 OpenAI-compatible provider 和 MCP context server。

### OpenCode

用 `bm agent-rules export --target opencode ...` 生成 custom OpenAI-compatible provider 和 MCP server 片段。

### Open WebUI

配置 OpenAI-compatible provider，base URL 为 `http://127.0.0.1:8787/v1`。用 `bm agent-rules export --target open-webui ...` 生成可信 Filter/Pipe recipe。不要安装未经审查的服务端 Python 代码。

### VS Code / VSCodium

用 `bm agent-rules export --target vscode ...` 生成 `.vscode/mcp.json` 风格片段。模型 provider 配置取决于具体扩展。

## Smoke Gate

运行发布集成门禁：

```bash
bash scripts/check_llm_gateway_release_integrations.sh
```

脚本会运行 gateway、CLI 规则导出、MCP stdio、MCP Streamable HTTP、MCP resources 和 `bm-mcp-server` binary usage 的本地合同检查。第三方客户端未安装或未显式配置时，检查会结构化 skip，不会伪装成真实 smoke 通过。
