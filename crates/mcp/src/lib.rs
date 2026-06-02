//! MCP adapter contracts for Beetle Memory.

#[cfg(all(
    feature = "server-stdio",
    any(
        feature = "profile-esp-standalone-memory",
        feature = "profile-esp-embedded-sdk"
    )
))]
compile_error!("bm-mcp server-stdio is forbidden for ESP profiles.");

use bm_adapter::AdapterOperation;

#[cfg(feature = "server-stdio")]
use bm_adapter::{
    decode_json_adapter_command, AdapterJsonCommandOptions, AdapterResponse, AdapterSdkReport,
    TransportKind, TransportMode,
};
#[cfg(feature = "server-stdio")]
use bm_entry::{EntryAuthDecision, EntryResponse, EntryRuntime, EntryTransportContext};
#[cfg(feature = "server-stdio")]
use serde_json::{json, Value};
#[cfg(feature = "server-stdio")]
use std::collections::BTreeMap;
#[cfg(feature = "server-stdio")]
use std::io::{BufRead, Read, Write};

#[cfg(feature = "server-stdio")]
const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
#[cfg(feature = "server-stdio")]
const MCP_SERVER_NAME: &str = "bm-mcp-server";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpToolSpec {
    pub name: &'static str,
    pub operation: AdapterOperation,
    pub schema_fields: Vec<String>,
    pub private_raw_allowed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpResourceSpec {
    pub uri: &'static str,
    pub name: &'static str,
    pub mime_type: &'static str,
    pub private_raw_allowed: bool,
}

pub fn tool_specs() -> Vec<McpToolSpec> {
    vec![
        tool("memory_capabilities", AdapterOperation::Capabilities, &[]),
        tool(
            "memory_recall",
            AdapterOperation::Recall,
            &["query", "limit"],
        ),
        tool(
            "memory_project",
            AdapterOperation::Project,
            &["query", "max_len"],
        ),
        tool("memory_inspect", AdapterOperation::Inspect, &["query"]),
        tool(
            "memory_replay",
            AdapterOperation::Replay,
            &["chat_id", "limit"],
        ),
        tool(
            "memory_write_candidate",
            AdapterOperation::Write,
            &["name", "topic", "title", "summary", "content"],
        ),
        tool("memory_export", AdapterOperation::Export, &["chat_id"]),
        tool(
            "memory_import",
            AdapterOperation::Import,
            &["snapshot", "target_chat_id"],
        ),
    ]
}

pub fn resource_specs() -> Vec<McpResourceSpec> {
    vec![
        resource("memory://profile", "memory_profile"),
        resource("memory://scope", "memory_scope"),
        resource("memory://projection-preview", "memory_projection_preview"),
    ]
}

fn tool(name: &'static str, operation: AdapterOperation, fields: &[&str]) -> McpToolSpec {
    McpToolSpec {
        name,
        operation,
        schema_fields: fields.iter().map(|field| (*field).to_string()).collect(),
        private_raw_allowed: false,
    }
}

fn resource(uri: &'static str, name: &'static str) -> McpResourceSpec {
    McpResourceSpec {
        uri,
        name,
        mime_type: "application/json",
        private_raw_allowed: false,
    }
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpToolCall {
    pub name: String,
    pub arguments: String,
}

#[cfg(feature = "server-stdio")]
impl McpToolCall {
    pub fn json(name: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            arguments: arguments.into(),
        }
    }
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpToolResult {
    pub status: String,
    pub content: String,
    pub private_raw_allowed: bool,
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpResourceRead {
    pub uri: String,
}

#[cfg(feature = "server-stdio")]
impl McpResourceRead {
    pub fn new(uri: impl Into<String>) -> Self {
        Self { uri: uri.into() }
    }
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpResourceContent {
    pub uri: String,
    pub mime_type: String,
    pub content: String,
    pub private_raw_allowed: bool,
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpStreamableHttpResponse {
    pub status: u16,
    pub content_type: String,
    pub body: String,
}

#[cfg(feature = "server-stdio")]
pub struct McpToolServer {
    server_id: String,
}

#[cfg(feature = "server-stdio")]
impl McpToolServer {
    pub fn new(server_id: impl Into<String>) -> Self {
        Self {
            server_id: server_id.into(),
        }
    }

    pub fn call(&self, runtime: &EntryRuntime, call: McpToolCall) -> bm_sdk::Result<McpToolResult> {
        let spec = tool_specs()
            .into_iter()
            .find(|spec| spec.name == call.name)
            .ok_or_else(|| bm_sdk::Error::config("mcp_runtime", "unsupported tool"))?;
        reject_missing_remote_source_scope(runtime, spec.operation, &call.arguments)?;
        let command = decode_json_adapter_command(
            spec.operation,
            &call.arguments,
            &mcp_command_options(runtime),
        )?;
        let response = runtime.handle(
            EntryTransportContext {
                request_id: format!("mcp-{}-{}", self.server_id, spec.name),
                transport: TransportKind::Mcp,
                mode: TransportMode::Server,
                operation: spec.operation,
                source_id: self.server_id.clone(),
                source_kind: "mcp_tool".to_string(),
                idempotency_key: format!("mcp-{}-{}", self.server_id, spec.name),
                audit_id: format!("audit-mcp-{}-{}", self.server_id, spec.name),
                auth: EntryAuthDecision::authenticated("mcp", "tool-client"),
            },
            command,
        )?;
        Ok(render_tool_result(response.adapter))
    }

    pub fn read_resource(
        &self,
        runtime: &EntryRuntime,
        read: McpResourceRead,
    ) -> bm_sdk::Result<McpResourceContent> {
        let spec = resource_specs()
            .into_iter()
            .find(|spec| spec.uri == read.uri)
            .ok_or_else(|| bm_sdk::Error::config("mcp_resource", "unsupported resource"))?;
        let content = match spec.uri {
            "memory://profile" => {
                let overview = runtime.console_overview();
                json!({
                    "profile": overview.runtime_shape.profile,
                    "runtime_shape": overview.runtime_shape,
                    "capabilities": overview.capabilities,
                    "private_raw_allowed": false,
                })
            }
            "memory://scope" => {
                let session = runtime.console_session();
                json!({
                    "owner": session.owner,
                    "memory_scope": session.memory_scope,
                    "session_state": session.session_state,
                    "private_raw_allowed": false,
                })
            }
            "memory://projection-preview" => {
                let response = self.dispatch_project_preview(runtime)?;
                render_projection_preview_resource(response.adapter)?
            }
            _ => unreachable!("resource spec list already matched supported URIs"),
        };
        Ok(McpResourceContent {
            uri: spec.uri.to_string(),
            mime_type: spec.mime_type.to_string(),
            content: content.to_string(),
            private_raw_allowed: spec.private_raw_allowed,
        })
    }

    fn dispatch_project_preview(&self, runtime: &EntryRuntime) -> bm_sdk::Result<EntryResponse> {
        let command = decode_json_adapter_command(
            AdapterOperation::Project,
            r#"{"query":"projection preview","max_len":1200}"#,
            &mcp_command_options(runtime),
        )?;
        runtime.handle(
            EntryTransportContext {
                request_id: format!("mcp-{}-resource-projection-preview", self.server_id),
                transport: TransportKind::Mcp,
                mode: TransportMode::Server,
                operation: AdapterOperation::Project,
                source_id: self.server_id.clone(),
                source_kind: "mcp_resource".to_string(),
                idempotency_key: format!("mcp-{}-resource-projection-preview", self.server_id),
                audit_id: format!("audit-mcp-{}-resource-projection-preview", self.server_id),
                auth: EntryAuthDecision::authenticated("mcp", "resource-client"),
            },
            command,
        )
    }
}

#[cfg(feature = "server-stdio")]
fn mcp_command_options(runtime: &EntryRuntime) -> AdapterJsonCommandOptions {
    let options = AdapterJsonCommandOptions::new("bm-mcp");
    if runtime.uses_local_default_scope_policy() {
        options.with_default_source_chat_id(runtime.runtime().scope().chat_id.clone())
    } else {
        options
    }
}

#[cfg(feature = "server-stdio")]
fn reject_missing_remote_source_scope(
    runtime: &EntryRuntime,
    operation: AdapterOperation,
    body: &str,
) -> bm_sdk::Result<()> {
    if runtime.uses_local_default_scope_policy() || operation != AdapterOperation::Write {
        return Ok(());
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|err| bm_sdk::Error::config("adapter_json_command", err.to_string()))?;
    if value
        .get("source_chat_id")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(());
    }
    Err(bm_sdk::Error::config(
        "adapter_json_command",
        "remote adapter write payload missing source_chat_id; refusing implicit chat-1 scope",
    ))
}

#[cfg(feature = "server-stdio")]
pub fn serve_mcp_stdio_once<R: BufRead, W: Write>(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    reader: &mut R,
    writer: &mut W,
) -> bm_sdk::Result<()> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|err| bm_sdk::Error::config("mcp_stdio_read", err.to_string()))?;
    if line.trim().is_empty() {
        return Err(bm_sdk::Error::config(
            "mcp_stdio_read",
            "empty JSON-RPC line",
        ));
    }
    let request: Value = serde_json::from_str(&line)
        .map_err(|err| bm_sdk::Error::config("mcp_stdio_json", err.to_string()))?;
    if let Some(response) = handle_mcp_json_rpc_request(server, runtime, request)? {
        writeln!(writer, "{response}")
            .map_err(|err| bm_sdk::Error::config("mcp_stdio_write", err.to_string()))?;
    }
    Ok(())
}

#[cfg(feature = "server-stdio")]
pub fn handle_mcp_streamable_http_request(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    body: &str,
) -> bm_sdk::Result<McpStreamableHttpResponse> {
    let request: Value = serde_json::from_str(body)
        .map_err(|err| bm_sdk::Error::config("mcp_http_json", err.to_string()))?;
    match handle_mcp_json_rpc_request(server, runtime, request)? {
        Some(response) => Ok(McpStreamableHttpResponse {
            status: 200,
            content_type: "application/json".to_string(),
            body: response.to_string(),
        }),
        None => Ok(McpStreamableHttpResponse {
            status: 202,
            content_type: String::new(),
            body: String::new(),
        }),
    }
}

#[cfg(feature = "server-stdio")]
pub fn serve_mcp_streamable_http_stream<S: Read + Write>(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    stream: &mut S,
) -> bm_sdk::Result<()> {
    let request = read_http_request(stream)?;
    if let Some(origin) = request.headers.get("origin") {
        if !mcp_origin_allowed(origin) {
            return write_http_json_response(
                stream,
                403,
                json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32600,
                        "message": "invalid MCP Origin",
                    }
                })
                .to_string(),
            );
        }
    }
    if request.method != "POST" {
        return write_http_json_response(
            stream,
            405,
            json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": {
                    "code": -32600,
                    "message": "MCP Streamable HTTP requires POST",
                }
            })
            .to_string(),
        );
    }
    if request.path != "/mcp" {
        return write_http_json_response(
            stream,
            404,
            json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": {
                    "code": -32601,
                    "message": "unsupported MCP HTTP path",
                }
            })
            .to_string(),
        );
    }
    let response = handle_mcp_streamable_http_request(server, runtime, &request.body)?;
    write_http_response(
        stream,
        response.status,
        response.content_type.as_str(),
        response.body.as_str(),
    )
}

#[cfg(feature = "server-stdio")]
fn handle_mcp_json_rpc_request(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    request: Value,
) -> bm_sdk::Result<Option<Value>> {
    let Some(object) = request.as_object() else {
        return Ok(Some(mcp_error(
            Value::Null,
            -32600,
            "invalid JSON-RPC request",
        )));
    };
    let id = object.get("id").cloned();
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        if object.contains_key("result") || object.contains_key("error") {
            return Ok(None);
        }
        return Ok(Some(mcp_error(Value::Null, -32600, "missing method")));
    };
    if id.is_none() {
        return Ok(None);
    }
    let id = id.unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let requested = request
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(MCP_PROTOCOL_VERSION);
            if requested != MCP_PROTOCOL_VERSION {
                return Ok(Some(mcp_error(
                    id,
                    -32602,
                    format!("unsupported MCP protocol version: {requested}"),
                )));
            }
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {
                        "listChanged": false,
                    },
                    "resources": {
                        "subscribe": false,
                        "listChanged": false,
                    },
                },
                "serverInfo": {
                    "name": MCP_SERVER_NAME,
                    "title": "Beetle Memory MCP Server",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": "Use Beetle Memory MCP tools for governed memory recall, projection, inspection, and write candidates. Raw private memory is not exposed.",
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({
            "tools": tool_specs()
                .into_iter()
                .map(|tool| mcp_tool_descriptor(&tool))
                .collect::<Vec<_>>(),
        }),
        "tools/call" => {
            let Some(params) = request.get("params") else {
                return Ok(Some(mcp_error(id, -32602, "missing params")));
            };
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Ok(Some(mcp_error(id, -32602, "missing tool name")));
            };
            if !tool_specs().iter().any(|spec| spec.name == name) {
                return Ok(Some(mcp_error(id, -32602, format!("Unknown tool: {name}"))));
            }
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}))
                .to_string();
            match server.call(runtime, McpToolCall::json(name, arguments)) {
                Ok(tool_result) => render_mcp_tool_call_result(tool_result),
                Err(error) => render_mcp_tool_execution_error(error.to_string()),
            }
        }
        "resources/list" => json!({
            "resources": resource_specs()
                .into_iter()
                .map(|resource| json!({
                    "uri": resource.uri,
                    "name": resource.name,
                    "title": resource.name.replace('_', " "),
                    "mimeType": resource.mime_type,
                    "_meta": {
                        "private_raw_allowed": resource.private_raw_allowed,
                    },
                }))
                .collect::<Vec<_>>(),
        }),
        "resources/read" => {
            let Some(params) = request.get("params") else {
                return Ok(Some(mcp_error(id, -32602, "missing params")));
            };
            let Some(uri) = params.get("uri").and_then(Value::as_str) else {
                return Ok(Some(mcp_error(id, -32602, "missing resource uri")));
            };
            let resource = server.read_resource(runtime, McpResourceRead::new(uri))?;
            json!({
                "contents": [{
                    "uri": resource.uri,
                    "mimeType": resource.mime_type,
                    "text": resource.content,
                    "_meta": {
                        "private_raw_allowed": resource.private_raw_allowed,
                    },
                }],
            })
        }
        other => {
            return Ok(Some(mcp_error(
                id,
                -32601,
                format!("unsupported MCP method: {other}"),
            )));
        }
    };
    Ok(Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })))
}

#[cfg(feature = "server-stdio")]
struct HttpRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: String,
}

#[cfg(feature = "server-stdio")]
fn read_http_request(stream: &mut impl Read) -> bm_sdk::Result<HttpRequest> {
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    while !buffer.ends_with(b"\r\n\r\n") {
        let read = stream
            .read(&mut byte)
            .map_err(|error| bm_sdk::Error::config("mcp_http_read", error.to_string()))?;
        if read == 0 {
            return Err(bm_sdk::Error::config(
                "mcp_http_read",
                "unexpected EOF while reading HTTP headers",
            ));
        }
        buffer.push(byte[0]);
        if buffer.len() > 64 * 1024 {
            return Err(bm_sdk::Error::config(
                "mcp_http_read",
                "HTTP headers are too large",
            ));
        }
    }
    let header_text = std::str::from_utf8(&buffer)
        .map_err(|error| bm_sdk::Error::config("mcp_http_read", error.to_string()))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| bm_sdk::Error::config("mcp_http_read", "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| bm_sdk::Error::config("mcp_http_read", "missing method"))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| bm_sdk::Error::config("mcp_http_read", "missing path"))?
        .to_string();
    let mut content_length = 0usize;
    let mut headers = BTreeMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let header_name = name.trim().to_ascii_lowercase();
        let header_value = value.trim().to_string();
        if header_name == "content-length" {
            content_length = value
                .trim()
                .parse::<usize>()
                .map_err(|error| bm_sdk::Error::config("mcp_http_read", error.to_string()))?;
        }
        if !header_name.is_empty() {
            headers.insert(header_name, header_value);
        }
    }
    let mut body_bytes = vec![0u8; content_length];
    if content_length > 0 {
        stream
            .read_exact(&mut body_bytes)
            .map_err(|error| bm_sdk::Error::config("mcp_http_read", error.to_string()))?;
    }
    let body = String::from_utf8(body_bytes)
        .map_err(|error| bm_sdk::Error::config("mcp_http_read", error.to_string()))?;
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

#[cfg(feature = "server-stdio")]
fn write_http_json_response(
    stream: &mut impl Write,
    status: u16,
    body: String,
) -> bm_sdk::Result<()> {
    write_http_response(stream, status, "application/json", body.as_str())
}

#[cfg(feature = "server-stdio")]
fn write_http_response(
    stream: &mut impl Write,
    status: u16,
    content_type: &str,
    body: &str,
) -> bm_sdk::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "OK",
    };
    if content_type.is_empty() {
        write!(
            stream,
            "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .map_err(|error| bm_sdk::Error::config("mcp_http_write", error.to_string()))
    } else {
        write!(
            stream,
            "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .map_err(|error| bm_sdk::Error::config("mcp_http_write", error.to_string()))
    }
}

#[cfg(feature = "server-stdio")]
fn mcp_origin_allowed(origin: &str) -> bool {
    let normalized = origin.trim().to_ascii_lowercase();
    normalized == "null"
        || normalized.starts_with("file://")
        || normalized.starts_with("http://localhost")
        || normalized.starts_with("https://localhost")
        || normalized.starts_with("http://127.0.0.1")
        || normalized.starts_with("https://127.0.0.1")
        || normalized.starts_with("http://[::1]")
        || normalized.starts_with("https://[::1]")
}

#[cfg(feature = "server-stdio")]
fn mcp_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        },
    })
}

#[cfg(feature = "server-stdio")]
fn mcp_tool_descriptor(tool: &McpToolSpec) -> Value {
    json!({
        "name": tool.name,
        "title": tool.name.replace('_', " "),
        "description": tool_description(tool.name),
        "inputSchema": mcp_tool_input_schema(&tool.schema_fields),
        "_meta": {
            "adapter_operation": format!("{:?}", tool.operation),
            "private_raw_allowed": tool.private_raw_allowed,
        },
    })
}

#[cfg(feature = "server-stdio")]
fn mcp_tool_input_schema(fields: &[String]) -> Value {
    let properties = fields
        .iter()
        .map(|field| {
            (
                field.clone(),
                match field.as_str() {
                    "limit" | "max_len" | "system_max_len" | "recent_messages_limit" => json!({
                        "type": "integer",
                        "minimum": 1,
                    }),
                    "snapshot" => json!({
                        "type": "object",
                    }),
                    "writes" => json!({
                        "type": "array",
                        "items": {
                            "type": "object",
                        },
                    }),
                    _ => json!({
                        "type": "string",
                    }),
                },
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let required = fields
        .iter()
        .filter(|field| {
            !matches!(
                field.as_str(),
                "limit" | "max_len" | "system_max_len" | "recent_messages_limit"
            )
        })
        .cloned()
        .collect::<Vec<_>>();

    if fields.is_empty() {
        json!({
            "type": "object",
            "additionalProperties": false,
        })
    } else {
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        })
    }
}

#[cfg(feature = "server-stdio")]
fn tool_description(name: &str) -> &'static str {
    match name {
        "memory_capabilities" => "Return the visible Beetle Memory capability surface.",
        "memory_recall" => "Recall governed memory snippets for an explicit query.",
        "memory_project" => "Build a bounded memory projection preview for an explicit query.",
        "memory_inspect" => "Inspect governed memory state without exposing raw private planes.",
        "memory_replay" => "Replay safe memory timeline events for a chat scope.",
        "memory_write_candidate" => "Submit a governed procedural memory write candidate.",
        "memory_export" => "Export a governed continuity snapshot for a chat scope.",
        "memory_import" => "Import a governed continuity snapshot into a target chat scope.",
        _ => "Beetle Memory MCP tool.",
    }
}

#[cfg(feature = "server-stdio")]
fn render_mcp_tool_call_result(tool_result: McpToolResult) -> Value {
    let structured_content = serde_json::from_str::<Value>(&tool_result.content)
        .unwrap_or_else(|_| json!({"status": tool_result.status, "text": tool_result.content}));
    json!({
        "content": [{
            "type": "text",
            "text": tool_result.content,
        }],
        "structuredContent": structured_content,
        "isError": tool_result.status == "rejected",
        "_meta": {
            "private_raw_allowed": tool_result.private_raw_allowed,
        },
    })
}

#[cfg(feature = "server-stdio")]
fn render_mcp_tool_execution_error(message: String) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": message,
        }],
        "structuredContent": {
            "status": "error",
            "private_raw_allowed": false,
        },
        "isError": true,
        "_meta": {
            "private_raw_allowed": false,
        },
    })
}

#[cfg(feature = "server-stdio")]
fn render_tool_result(response: AdapterResponse<AdapterSdkReport>) -> McpToolResult {
    match response {
        AdapterResponse::Accepted { report, .. } => {
            let content = match report {
                AdapterSdkReport::Recall(report) => json!({
                    "status": "accepted",
                    "query": report.query,
                    "procedural_hits": report.procedural_hits.len(),
                })
                .to_string(),
                AdapterSdkReport::Capabilities(catalog) => json!({
                    "status": "accepted",
                    "profile": catalog.profile.as_str(),
                })
                .to_string(),
                other => json!({"status":"accepted","report":format!("{other:?}")}).to_string(),
            };
            McpToolResult {
                status: "accepted".to_string(),
                content,
                private_raw_allowed: false,
            }
        }
        AdapterResponse::Rejected { reason, .. } => McpToolResult {
            status: "rejected".to_string(),
            content: json!({"status":"rejected","reason":reason}).to_string(),
            private_raw_allowed: false,
        },
        AdapterResponse::Queued { queue, .. } => McpToolResult {
            status: "queued".to_string(),
            content: json!({"status":"queued","queue":queue}).to_string(),
            private_raw_allowed: false,
        },
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => McpToolResult {
            status: "duplicated".to_string(),
            content: json!({"status":"duplicated","idempotency_key":idempotency_key}).to_string(),
            private_raw_allowed: false,
        },
    }
}

#[cfg(feature = "server-stdio")]
fn render_projection_preview_resource(
    response: AdapterResponse<AdapterSdkReport>,
) -> bm_sdk::Result<Value> {
    match response {
        AdapterResponse::Accepted { report, .. } => match report {
            AdapterSdkReport::Project(report) => Ok(json!({
                "status": "accepted",
                "preview": report.system_memory_block,
                "chars": report.system_memory_block.chars().count(),
                "private_raw_allowed": false,
            })),
            _ => Err(bm_sdk::Error::config(
                "mcp_resource",
                "projection preview returned non-project report",
            )),
        },
        AdapterResponse::Rejected { reason, .. } => Ok(json!({
            "status": "rejected",
            "reason": reason,
            "private_raw_allowed": false,
        })),
        AdapterResponse::Queued { queue, .. } => Ok(json!({
            "status": "queued",
            "queue": queue,
            "private_raw_allowed": false,
        })),
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => Ok(json!({
            "status": "duplicated",
            "idempotency_key": idempotency_key,
            "private_raw_allowed": false,
        })),
    }
}
