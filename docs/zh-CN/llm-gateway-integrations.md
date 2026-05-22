# LLM Gateway 集成文档

`bm-llm-gateway` 让现有 IDE、coding agent 和本地模型工具通过它们已经支持的协议接入 Beetle Memory。本页只描述显式 gateway 模式。Ollama App 透明端口接管是后续独立模式，不属于当前发布面。

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
export BM_MEMORY_STORE_FILE=target/bm-memory-gateway-store
export BM_MEMORY_OWNER_ID=owner-default
export BM_MEMORY_AGENT_ID=agent-main
export BM_MEMORY_CHANNEL=llm.gateway
export BM_MEMORY_CHAT_ID=chat-1
```

`bm-mcp-server` 也支持显式本地覆盖：

```bash
bm-mcp-server stdio --store-file target/bm-memory-gateway-store --chat-id chat-1
bm-mcp-server http --addr 127.0.0.1:8788 --store-file target/bm-memory-gateway-store --chat-id chat-1
```

sqlite 部署用 `BM_MEMORY_STORE_SQLITE=/path/to/memory.sqlite3`；`BM_MEMORY_STORE_MEMORY=1` 只用于一次性本地测试。binary 默认是 `target/bm-memory-gateway-store` 本地 file store，不是临时 in-memory store。

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
