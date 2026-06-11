# LLM Gateway Integrations

`bm-llm-gateway` lets existing IDEs, coding agents, and local model tools connect to Beetle Memory through protocols they already support. This page covers the explicit gateway mode only. Ollama App transparent port takeover is a later mode and is not part of this release surface.

## Endpoints

Default local endpoints:

```text
OpenAI-compatible gateway: http://127.0.0.1:8787/v1
Ollama native gateway:     http://127.0.0.1:8787/api
MCP Streamable HTTP:       http://127.0.0.1:8788/mcp
```

OpenAI-compatible routes:

- `GET /v1/models`
- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/embeddings`
- `GET /v1/bm/provider-capabilities`

Ollama native routes:

- `GET /api/tags`
- `GET /api/version`
- `POST /api/chat`
- `POST /api/generate`
- `POST /api/embed`
- `POST /api/embeddings`
- `POST /api/show`

Chat, generate, and stateless responses requests receive deterministic Beetle Memory projection before the upstream model request. Embeddings and model-management routes are passthrough and do not trigger projection or maintenance.

## Console Surface

The shared console includes a dedicated LLM Gateway page backed by `GET /console/llm-gateway`. The page only reports protocol endpoints, `bm agent-rules export` commands, and local smoke gates; the system-wide memory context is owned by Overview through `GET /console/overview`. `bm-llm-gateway` still owns the OpenAI and Ollama protocol logic.

## Shared Runtime

Run `bm-llm-gateway` and `bm-mcp-server` against the same Beetle Memory runtime by using the shared `BM_MEMORY_*` environment variables:

```bash
export BM_MEMORY_STORE_FILE=/var/lib/beetle-memory/gateway-store
export BM_MEMORY_OWNER_ID=owner-default
export BM_MEMORY_AGENT_ID=agent-main
export BM_MEMORY_CHANNEL=llm.gateway
export BM_MEMORY_CHAT_ID=chat-1
```

Both binaries also accept explicit local overrides where applicable:

```bash
bm-mcp-server stdio --store-file /var/lib/beetle-memory/gateway-store --chat-id chat-1
bm-mcp-server http --addr 127.0.0.1:8788 --store-file /var/lib/beetle-memory/gateway-store --chat-id chat-1
```

Use `BM_MEMORY_STORE_SQLITE=/path/to/memory.sqlite3` for sqlite deployments, or `BM_MEMORY_STORE_MEMORY=1` only for disposable local tests. Persistent file/sqlite paths must be explicit absolute paths; the binaries do not default to a repository-local `target/` store.

## MCP Server

`bm-mcp-server` implements the MCP lifecycle handshake, `tools/list` with `inputSchema`, `tools/call` with MCP `content` plus `structuredContent`, and safe `resources/list` / `resources/read` results. It exposes only governed memory tools and safe resources; raw private memory planes are not exposed.

## Rule Export

Generate tool-specific configuration without embedding remembered facts:

```bash
bm agent-rules export \
  --target continue \
  --gateway-url http://127.0.0.1:8787/v1 \
  --mcp-url http://127.0.0.1:8788/mcp
```

Supported targets:

- `continue`
- `cline`
- `aider`
- `zed`
- `opencode`
- `open-webui`
- `vscode`

The generated output points tools to the gateway and MCP endpoint. It must not contain raw memory, projection payloads, or private store data.

## Recipes

### Aider

Use the OpenAI-compatible gateway:

```bash
export OPENAI_API_BASE=http://127.0.0.1:8787/v1
export OPENAI_API_KEY=beetle-memory-local
aider --model openai/beetle-memory
```

Use `bm agent-rules export --target aider ...` to generate a `CONVENTIONS.md` snippet that points to the gateway and forbids raw memory paste.

### Continue

Use `bm agent-rules export --target continue ...` and merge the generated `models` and `mcpServers` snippets into Continue configuration. Model traffic goes to the OpenAI-compatible gateway; explicit memory tools use MCP.

### Cline

Use OpenAI Compatible as the model provider with base URL `http://127.0.0.1:8787/v1`. Put the `cline` export output under `.clinerules/memory.md`. The rules should point to MCP tools instead of embedding memory content.

### Zed

Use `bm agent-rules export --target zed ...` to generate a JSON settings snippet with an OpenAI-compatible provider and MCP context server.

### OpenCode

Use `bm agent-rules export --target opencode ...` to generate a custom OpenAI-compatible provider and MCP server snippet.

### Open WebUI

Use an OpenAI-compatible provider with base URL `http://127.0.0.1:8787/v1`. Use `bm agent-rules export --target open-webui ...` for a trusted Filter/Pipe recipe. Do not install unreviewed server-side Python code.

### VS Code / VSCodium

Use `bm agent-rules export --target vscode ...` for an `.vscode/mcp.json` style snippet. Model provider setup still depends on the specific extension.

## Smoke Gates

Run the release integration gate:

```bash
bash scripts/check_llm_gateway_release_integrations.sh
```

The script runs local contract checks for gateway, CLI rule export, MCP stdio, MCP Streamable HTTP, MCP resources, and `bm-mcp-server` binary usage. Third-party client checks are reported as structured skip unless the tool or endpoint is installed and explicitly configured.
