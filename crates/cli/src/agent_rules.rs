use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRulesTarget {
    Continue,
    Cline,
    Aider,
    Zed,
    Opencode,
    OpenWebui,
    Vscode,
}

impl AgentRulesTarget {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "continue" => Ok(Self::Continue),
            "cline" => Ok(Self::Cline),
            "aider" => Ok(Self::Aider),
            "zed" => Ok(Self::Zed),
            "opencode" => Ok(Self::Opencode),
            "open-webui" => Ok(Self::OpenWebui),
            "vscode" => Ok(Self::Vscode),
            other => Err(format!(
                "unsupported agent rules target: {other}; supported targets: {}",
                Self::supported_targets().join(", ")
            )),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Cline => "cline",
            Self::Aider => "aider",
            Self::Zed => "zed",
            Self::Opencode => "opencode",
            Self::OpenWebui => "open-webui",
            Self::Vscode => "vscode",
        }
    }

    pub fn supported_targets() -> Vec<&'static str> {
        vec![
            "continue",
            "cline",
            "aider",
            "zed",
            "opencode",
            "open-webui",
            "vscode",
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRulesExportRequest {
    pub target: AgentRulesTarget,
    pub gateway_url: String,
    pub mcp_url: String,
}

pub fn render_agent_rules_export(request: &AgentRulesExportRequest) -> Result<String, String> {
    match request.target {
        AgentRulesTarget::Continue => Ok(render_continue(request)),
        AgentRulesTarget::Cline => Ok(render_cline(request)),
        AgentRulesTarget::Aider => Ok(render_aider(request)),
        AgentRulesTarget::Zed => render_zed(request),
        AgentRulesTarget::Opencode => render_opencode(request),
        AgentRulesTarget::OpenWebui => Ok(render_open_webui(request)),
        AgentRulesTarget::Vscode => render_vscode(request),
    }
}

fn render_continue(request: &AgentRulesExportRequest) -> String {
    format!(
        r#"models:
  - name: Beetle Memory Gateway
    provider: openai
    model: beetle-memory
    apiBase: {gateway_url}
    apiKey: beetle-memory-local
mcpServers:
  beetle-memory:
    transport: streamable-http
    url: {mcp_url}
rules:
  - Route model traffic through the Beetle Memory Gateway so projection happens before the upstream model request.
  - Use MCP tools for explicit memory operations: memory_project, memory_recall, memory_write_candidate, memory_inspect.
  - Do not paste raw memory into prompts, files, logs, or chat history.
  - Treat MCP as an explicit tool surface; the gateway is the deterministic projection path.
"#,
        gateway_url = request.gateway_url,
        mcp_url = request.mcp_url
    )
}

fn render_cline(request: &AgentRulesExportRequest) -> String {
    format!(
        r#"# Beetle Memory Rules for Cline

## Gateway
- Provider: OpenAI Compatible.
- Base URL: {gateway_url}
- Model ID: beetle-memory
- API key: beetle-memory-local

## MCP
- MCP endpoint: {mcp_url}
- Available memory tools: memory_project, memory_recall, memory_write_candidate, memory_inspect.

## Constraints
- Do not paste raw memory into Cline rules, prompts, files, logs, or Memory Bank.
- Use the gateway for deterministic projection before model requests.
- Use MCP only for explicit recall, inspection, projection preview, and governed write candidates.
"#,
        gateway_url = request.gateway_url,
        mcp_url = request.mcp_url
    )
}

fn render_aider(request: &AgentRulesExportRequest) -> String {
    format!(
        r#"# Beetle Memory Rules for Aider

## Gateway
```bash
export OPENAI_API_BASE={gateway_url}
export OPENAI_API_KEY=beetle-memory-local
aider --model openai/beetle-memory
```

## MCP
- MCP endpoint for companion tools: {mcp_url}
- Available memory tools: memory_project, memory_recall, memory_write_candidate, memory_inspect.

## CONVENTIONS.md
- Keep long-term context in Beetle Memory through the gateway and MCP tool surface.
- Do not paste raw memory into repository files, prompts, commit messages, or Aider chat.
- Use repo-map for code navigation only; it is not the long-term memory store.
"#,
        gateway_url = request.gateway_url,
        mcp_url = request.mcp_url
    )
}

fn render_zed(request: &AgentRulesExportRequest) -> Result<String, String> {
    render_json(json!({
        "language_models": {
            "openai_compatible": {
                "beetle-memory-gateway": {
                    "api_url": request.gateway_url,
                    "available_models": [
                        {
                            "name": "beetle-memory",
                            "display_name": "Beetle Memory Gateway"
                        }
                    ]
                }
            }
        },
        "context_servers": {
            "beetle-memory": {
                "source": "custom",
                "url": request.mcp_url
            }
        },
        "rules": shared_rules(),
        "memory_tools": memory_tools(),
    }))
}

fn render_opencode(request: &AgentRulesExportRequest) -> Result<String, String> {
    render_json(json!({
        "provider": {
            "beetle-memory": {
                "type": "openai-compatible",
                "baseURL": request.gateway_url,
                "models": {
                    "beetle-memory": {
                        "name": "Beetle Memory Gateway"
                    }
                }
            }
        },
        "mcp": {
            "beetle-memory": {
                "type": "remote",
                "url": request.mcp_url
            }
        },
        "rules": shared_rules(),
        "memory_tools": memory_tools(),
    }))
}

fn render_open_webui(request: &AgentRulesExportRequest) -> String {
    format!(
        r#""""Beetle Memory Open WebUI Filter/Pipe recipe.

Configure an OpenAI-compatible provider with base URL:
{gateway_url}

Configure MCP Streamable HTTP or OpenAPI tools at:
{mcp_url}
"""

GATEWAY_URL = "{gateway_url}"
MCP_URL = "{mcp_url}"
MEMORY_TOOLS = ["memory_project", "memory_recall", "memory_write_candidate", "memory_inspect"]


class Filter:
    async def inlet(self, body, user=None):
        body.setdefault("metadata", {{}})
        body["metadata"]["beetle_memory_gateway"] = GATEWAY_URL
        body["metadata"]["beetle_memory_mcp"] = MCP_URL
        body["metadata"]["beetle_memory_constraint"] = "Do not paste raw memory into prompts, files, logs, or chat history."
        body["metadata"]["beetle_memory_tools"] = MEMORY_TOOLS
        return body

    async def outlet(self, body, user=None):
        body.setdefault("metadata", {{}})
        body["metadata"]["beetle_memory_maintenance"] = "Send final responses through governed gateway maintenance or MCP write candidates."
        return body
"#,
        gateway_url = request.gateway_url,
        mcp_url = request.mcp_url
    )
}

fn render_vscode(request: &AgentRulesExportRequest) -> Result<String, String> {
    render_json(json!({
        "servers": {
            "beetle-memory": {
                "type": "http",
                "url": request.mcp_url
            }
        },
        "inputs": [],
        "beetleMemory": {
            "gateway": {
                "provider": "openai-compatible",
                "baseUrl": request.gateway_url,
                "model": "beetle-memory"
            },
            "rules": shared_rules(),
            "memoryTools": memory_tools(),
        }
    }))
}

fn memory_tools() -> Vec<&'static str> {
    vec![
        "memory_project",
        "memory_recall",
        "memory_write_candidate",
        "memory_inspect",
    ]
}

fn shared_rules() -> Vec<&'static str> {
    vec![
        "Route model traffic through the Beetle Memory Gateway for deterministic projection.",
        "Use MCP tools only for explicit memory operations: memory_project, memory_recall, memory_write_candidate, memory_inspect.",
        "Do not paste raw memory into prompts, files, logs, or chat history.",
        "Rules point to gateway and MCP endpoints only; they do not embed remembered facts.",
    ]
}

fn render_json(value: serde_json::Value) -> Result<String, String> {
    serde_json::to_string_pretty(&value).map_err(|err| err.to_string())
}
