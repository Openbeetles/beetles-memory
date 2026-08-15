//! MCP adapter contracts for Beetle Memory.

#[cfg(all(
    feature = "server-stdio",
    any(
        feature = "profile-esp-standalone-memory",
        feature = "profile-esp-embedded-sdk"
    )
))]
compile_error!("bm-mcp server-stdio is forbidden for ESP profiles.");

use bm_adapter::{governed_adapter_json_command_schema, AdapterOperation};

#[cfg(feature = "server-stdio")]
use bm_adapter::{
    decode_json_adapter_command, AdapterJsonCommandOptions, AdapterRequestIdentityOwner,
    AdapterResponse, AdapterSdkReport, TransportKind, TransportMode,
};
#[cfg(feature = "server-stdio")]
use bm_entry::{
    read_authorized_http_request, EntryAcceptedTcpStream, EntryAuthDecision,
    EntryHttpAuthorization, EntryHttpIngressErrorKind, EntryHttpIngressLimits, EntryLocalTransport,
    EntryOperationCapability, EntryResponse, EntryRuntime, EntryRuntimeBudgetLease,
    EntryTransportContext,
};
#[cfg(feature = "server-stdio")]
use bm_sdk::{MemoryCapabilityCatalog, RuntimeBudgetReport};
#[cfg(feature = "server-stdio")]
use serde_json::{json, Value};
#[cfg(feature = "server-stdio")]
use std::collections::BTreeMap;
#[cfg(feature = "server-stdio")]
use std::io::{BufRead, Write};
#[cfg(feature = "server-stdio")]
use std::net::SocketAddr;

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
        governed_tool("memory_finalize_turn", AdapterOperation::FinalizeTurn),
        governed_tool("memory_recall", AdapterOperation::Recall),
        governed_tool("memory_project", AdapterOperation::Project),
        tool(
            "memory_inspect",
            AdapterOperation::Inspect,
            &["query", "system_max_len"],
        ),
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
        tool(
            "memory_long_term_list",
            AdapterOperation::LongTermList,
            &["query", "limit"],
        ),
        tool(
            "memory_long_term_detail",
            AdapterOperation::LongTermDetail,
            &["target"],
        ),
        tool(
            "memory_long_term_mutate",
            AdapterOperation::LongTermMutate,
            &["operation", "reason", "dry_run"],
        ),
        tool(
            "memory_long_term_policy",
            AdapterOperation::LongTermPolicy,
            &["operation", "reason", "dry_run"],
        ),
        tool(
            "memory_transcript_attr_write",
            AdapterOperation::TranscriptAttrWrite,
            &[
                "memory_space_id",
                "channel_id",
                "conversation_id",
                "attrs",
                "idempotency_key",
                "dry_run",
            ],
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

fn governed_tool(name: &'static str, operation: AdapterOperation) -> McpToolSpec {
    let schema = governed_adapter_json_command_schema(operation)
        .expect("governed MCP tool operation must have one adapter-owned schema");
    McpToolSpec {
        name,
        operation,
        schema_fields: schema
            .field_names
            .iter()
            .map(|field| (*field).to_string())
            .collect(),
        private_raw_allowed: false,
    }
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
    pub idempotency_key: Option<String>,
}

#[cfg(feature = "server-stdio")]
impl McpToolCall {
    pub fn json(name: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            arguments: arguments.into(),
            idempotency_key: None,
        }
    }

    pub fn with_idempotency_key(mut self, idempotency_key: impl Into<String>) -> Self {
        self.idempotency_key = Some(idempotency_key.into());
        self
    }
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpToolResult {
    pub status: String,
    pub content: String,
    pub private_raw_allowed: bool,
    pub budget_report_id: String,
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
    pub budget_report_id: String,
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpStreamableHttpResponse {
    pub status: u16,
    pub content_type: String,
    pub body: String,
    pub budget_report_id: String,
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum McpRequestTransport {
    InProcess,
    Stdio,
    StreamableHttp,
}

#[cfg(feature = "server-stdio")]
#[derive(Clone, Debug)]
struct McpCapabilitySnapshot {
    principal: String,
    transport: McpRequestTransport,
    tools: Vec<McpToolSpec>,
    resources: Vec<McpResourceSpec>,
}

#[cfg(feature = "server-stdio")]
impl McpCapabilitySnapshot {
    fn for_request(
        runtime: &EntryRuntime,
        auth: &EntryAuthDecision,
        transport: McpRequestTransport,
    ) -> bm_sdk::Result<Self> {
        if !auth.is_authenticated() || auth.principal_id().is_empty() {
            return Err(bm_sdk::Error::config(
                "mcp_capability_snapshot",
                auth.rejection_reason()
                    .unwrap_or("MCP request requires an authenticated principal"),
            ));
        }
        let transport_visible = runtime.capability().mcp_server.visible
            && auth.allows(bm_entry::EntryOperationCapability::McpProtocol);
        let catalog = runtime.runtime().capabilities();
        let tools = if transport_visible {
            tool_specs()
                .into_iter()
                .filter(|spec| {
                    auth.allows(bm_entry::EntryOperationCapability::for_adapter_operation(
                        spec.operation,
                    )) && mcp_operation_visible(catalog, spec.operation)
                })
                .collect()
        } else {
            Vec::new()
        };
        let resources = if transport_visible {
            resource_specs()
                .into_iter()
                .filter(|spec| mcp_resource_visible(catalog, auth, spec.uri))
                .collect()
        } else {
            Vec::new()
        };
        Ok(Self {
            principal: auth.principal_id().to_string(),
            transport,
            tools,
            resources,
        })
    }

    fn tool(&self, name: &str) -> Option<&McpToolSpec> {
        self.tools.iter().find(|spec| spec.name == name)
    }

    fn resource(&self, uri: &str) -> Option<&McpResourceSpec> {
        self.resources.iter().find(|spec| spec.uri == uri)
    }
}

#[cfg(feature = "server-stdio")]
fn mcp_operation_visible(catalog: &MemoryCapabilityCatalog, operation: AdapterOperation) -> bool {
    match operation {
        AdapterOperation::Write
        | AdapterOperation::FinalizeTurn
        | AdapterOperation::TranscriptAttrWrite => catalog.write.visible,
        AdapterOperation::Recall => catalog.recall.visible,
        AdapterOperation::Project => catalog.projection.visible,
        AdapterOperation::Maintain => catalog.maintenance.visible,
        AdapterOperation::Inspect => catalog.inspection.visible,
        AdapterOperation::Recover => catalog.inspection.visible,
        AdapterOperation::Replay => catalog.replay.visible,
        AdapterOperation::LongTermList | AdapterOperation::LongTermDetail => {
            catalog.long_term_control_inspect.visible
        }
        AdapterOperation::LongTermMutate => catalog.long_term_control_mutation.visible,
        AdapterOperation::LongTermPolicy => catalog.long_term_control_policy.visible,
        AdapterOperation::Capabilities => catalog.communication_adapter.visible,
        AdapterOperation::Subscribe | AdapterOperation::Close => false,
    }
}

#[cfg(feature = "server-stdio")]
fn mcp_resource_visible(
    catalog: &MemoryCapabilityCatalog,
    auth: &EntryAuthDecision,
    uri: &str,
) -> bool {
    let (capability, visible) = match uri {
        "memory://profile" => (
            bm_entry::EntryOperationCapability::Capabilities,
            catalog.communication_adapter.visible,
        ),
        "memory://scope" => (
            bm_entry::EntryOperationCapability::Inspect,
            catalog.inspection.visible,
        ),
        "memory://projection-preview" => (
            bm_entry::EntryOperationCapability::Project,
            catalog.projection.visible,
        ),
        _ => return false,
    };
    auth.allows(capability) && visible
}

#[cfg(feature = "server-stdio")]
pub struct McpToolServer {
    server_id: String,
    local_principal: String,
}

#[cfg(feature = "server-stdio")]
impl McpToolServer {
    pub fn new(server_id: impl Into<String>, authenticated_principal: impl Into<String>) -> Self {
        Self {
            server_id: server_id.into(),
            local_principal: authenticated_principal.into(),
        }
    }

    pub fn call(&self, runtime: &EntryRuntime, call: McpToolCall) -> bm_sdk::Result<McpToolResult> {
        let lease = acquire_runtime_budget_lease(runtime)?;
        let auth = runtime
            .authenticate_local_transport(EntryLocalTransport::InProcess, &self.local_principal);
        let snapshot =
            McpCapabilitySnapshot::for_request(runtime, &auth, McpRequestTransport::InProcess)?;
        runtime.execute_with_budget_lease(&lease, || {
            self.call_with_budget_lease(runtime, call, &lease, &auth, &snapshot)
        })
    }

    fn call_with_budget_lease(
        &self,
        runtime: &EntryRuntime,
        call: McpToolCall,
        lease: &EntryRuntimeBudgetLease,
        auth: &EntryAuthDecision,
        snapshot: &McpCapabilitySnapshot,
    ) -> bm_sdk::Result<McpToolResult> {
        let budget_report = lease.report();
        let call_material_bytes = call
            .arguments
            .len()
            .saturating_add(call.idempotency_key.as_ref().map_or(0, |key| key.len()));
        if call_material_bytes > budget_report.adapter_budget.http_body_max_bytes {
            return Err(bm_sdk::Error::config(
                "mcp_tool_budget",
                "MCP tool arguments and metadata exceed pinned runtime adapter budget",
            ));
        }
        let spec = snapshot
            .tool(&call.name)
            .ok_or_else(|| bm_sdk::Error::config("mcp_runtime", "unsupported tool"))?;
        reject_missing_remote_source_scope(runtime, spec.operation, &call.arguments)?;
        let arguments = normalize_typed_tool_arguments(spec.name, &call.arguments)?;
        let command =
            decode_json_adapter_command(spec.operation, &arguments, &mcp_command_options(runtime))?;
        let request_identity = AdapterRequestIdentityOwner::new(
            TransportKind::Mcp,
            &self.server_id,
            &snapshot.principal,
        )
        .issue(call.idempotency_key.as_deref())
        .map_err(|error| bm_sdk::Error::config("mcp_request_identity", error.to_string()))?;
        let response = runtime.handle_with_budget_lease(
            EntryTransportContext::new(
                request_identity.request_id,
                TransportKind::Mcp,
                match snapshot.transport {
                    McpRequestTransport::InProcess => TransportMode::InProcess,
                    McpRequestTransport::Stdio | McpRequestTransport::StreamableHttp => {
                        TransportMode::Server
                    }
                },
                spec.operation,
                self.server_id.clone(),
                "mcp_tool",
                request_identity.idempotency_key,
                request_identity.audit_id,
                auth.clone(),
            ),
            command,
            lease,
        )?;
        if response.budget_report != *budget_report {
            return Err(bm_sdk::Error::config(
                "mcp_tool_budget",
                "entry_response_budget_lease_identity_mismatch",
            ));
        }
        let mut result = render_tool_result(response.adapter);
        if result.content.len() > budget_report.adapter_budget.http_body_max_bytes {
            return Err(bm_sdk::Error::config(
                "mcp_tool_budget",
                "MCP tool result exceeds pinned runtime adapter budget",
            ));
        }
        result.budget_report_id.clone_from(&budget_report.report_id);
        Ok(result)
    }

    pub fn read_resource(
        &self,
        runtime: &EntryRuntime,
        read: McpResourceRead,
    ) -> bm_sdk::Result<McpResourceContent> {
        let lease = acquire_runtime_budget_lease(runtime)?;
        let auth = runtime
            .authenticate_local_transport(EntryLocalTransport::InProcess, &self.local_principal);
        let snapshot =
            McpCapabilitySnapshot::for_request(runtime, &auth, McpRequestTransport::InProcess)?;
        runtime.execute_with_budget_lease(&lease, || {
            self.read_resource_with_budget_lease(runtime, read, &lease, &auth, &snapshot)
        })
    }

    fn read_resource_with_budget_lease(
        &self,
        runtime: &EntryRuntime,
        read: McpResourceRead,
        lease: &EntryRuntimeBudgetLease,
        auth: &EntryAuthDecision,
        snapshot: &McpCapabilitySnapshot,
    ) -> bm_sdk::Result<McpResourceContent> {
        let budget_report = lease.report();
        let spec = snapshot
            .resource(&read.uri)
            .ok_or_else(|| bm_sdk::Error::config("mcp_resource", "unsupported resource"))?;
        let content = match spec.uri {
            "memory://profile" => {
                let overview = runtime.console_overview()?;
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
                let response = self.dispatch_project_preview(runtime, lease, auth)?;
                if response.budget_report != *budget_report {
                    return Err(bm_sdk::Error::config(
                        "mcp_resource_budget",
                        "entry_response_budget_lease_identity_mismatch",
                    ));
                }
                render_projection_preview_resource(response.adapter)?
            }
            _ => unreachable!("resource spec list already matched supported URIs"),
        };
        let content = content.to_string();
        if content.len() > budget_report.adapter_budget.http_body_max_bytes {
            return Err(bm_sdk::Error::config(
                "mcp_resource_budget",
                "MCP resource content exceeds pinned runtime adapter budget",
            ));
        }
        Ok(McpResourceContent {
            uri: spec.uri.to_string(),
            mime_type: spec.mime_type.to_string(),
            content,
            private_raw_allowed: spec.private_raw_allowed,
            budget_report_id: budget_report.report_id.clone(),
        })
    }

    fn dispatch_project_preview(
        &self,
        runtime: &EntryRuntime,
        lease: &EntryRuntimeBudgetLease,
        auth: &EntryAuthDecision,
    ) -> bm_sdk::Result<EntryResponse> {
        let command = decode_json_adapter_command(
            AdapterOperation::Project,
            r#"{"temporal_operation":{"kind":"current"},"user_query":"projection preview","system_max_len":1200}"#,
            &mcp_command_options(runtime),
        )?;
        let request_identity = AdapterRequestIdentityOwner::new(
            TransportKind::Mcp,
            &self.server_id,
            auth.principal_id(),
        )
        .issue(None)
        .map_err(|error| bm_sdk::Error::config("mcp_request_identity", error.to_string()))?;
        runtime.handle_with_budget_lease(
            EntryTransportContext::new(
                request_identity.request_id,
                TransportKind::Mcp,
                TransportMode::Server,
                AdapterOperation::Project,
                self.server_id.clone(),
                "mcp_resource",
                request_identity.idempotency_key,
                request_identity.audit_id,
                auth.clone(),
            ),
            command,
            lease,
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
fn acquire_runtime_budget_lease(runtime: &EntryRuntime) -> bm_sdk::Result<EntryRuntimeBudgetLease> {
    runtime.acquire_budget_lease()
}

#[cfg(feature = "server-stdio")]
fn read_bounded_json_line<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> bm_sdk::Result<Option<String>> {
    let mut bytes = Vec::with_capacity(max_bytes.min(8 * 1024));
    loop {
        let available = reader
            .fill_buf()
            .map_err(|error| bm_sdk::Error::config("mcp_stdio_read", error.to_string()))?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.unwrap_or(available.len());
        if bytes.len().saturating_add(take) > max_bytes {
            return Err(bm_sdk::Error::config(
                "mcp_stdio_read",
                "JSON-RPC line exceeds pinned runtime adapter budget",
            ));
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take + usize::from(newline.is_some()));
        if newline.is_some() {
            break;
        }
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| bm_sdk::Error::config("mcp_stdio_read", error.to_string()))
}

#[cfg(feature = "server-stdio")]
fn bind_mcp_budget_meta(mut response: Value, report: &RuntimeBudgetReport) -> Value {
    if let Some(object) = response.as_object_mut() {
        object.insert(
            "_meta".to_string(),
            json!({"runtimeBudgetReportId": report.report_id}),
        );
    }
    response
}

#[cfg(feature = "server-stdio")]
pub fn serve_mcp_stdio_once<R: BufRead, W: Write>(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    reader: &mut R,
    writer: &mut W,
) -> bm_sdk::Result<()> {
    let lease = acquire_runtime_budget_lease(runtime)?;
    let budget_report = lease.report();
    let Some(line) =
        read_bounded_json_line(reader, budget_report.adapter_budget.http_body_max_bytes)?
    else {
        return Err(bm_sdk::Error::config(
            "mcp_stdio_read",
            "unexpected EOF before JSON-RPC line",
        ));
    };
    runtime.execute_with_budget_lease(&lease, || {
        serve_mcp_stdio_line(server, runtime, writer, line, &lease)
    })
}

#[cfg(feature = "server-stdio")]
pub fn serve_mcp_stdio<R: BufRead, W: Write>(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    reader: &mut R,
    writer: &mut W,
) -> bm_sdk::Result<()> {
    loop {
        let lease = acquire_runtime_budget_lease(runtime)?;
        let budget_report = lease.report();
        let Some(line) =
            read_bounded_json_line(reader, budget_report.adapter_budget.http_body_max_bytes)?
        else {
            return Ok(());
        };
        if line.trim().is_empty() {
            continue;
        }
        runtime.execute_with_budget_lease(&lease, || {
            serve_mcp_stdio_line(server, runtime, writer, line, &lease)
        })?;
    }
}

#[cfg(feature = "server-stdio")]
fn serve_mcp_stdio_line<W: Write>(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    writer: &mut W,
    line: String,
    lease: &EntryRuntimeBudgetLease,
) -> bm_sdk::Result<()> {
    let budget_report = lease.report();
    if line.trim().is_empty() {
        return Err(bm_sdk::Error::config(
            "mcp_stdio_read",
            "empty JSON-RPC line",
        ));
    }
    let request: Value = serde_json::from_str(&line)
        .map_err(|err| bm_sdk::Error::config("mcp_stdio_json", err.to_string()))?;
    let auth =
        runtime.authenticate_local_transport(EntryLocalTransport::Stdio, &server.local_principal);
    if let Some(response) = handle_mcp_json_rpc_request(
        server,
        runtime,
        request,
        lease,
        &auth,
        McpRequestTransport::Stdio,
    )? {
        let response = bind_mcp_budget_meta(response, budget_report);
        let response = response.to_string();
        if response.len() > budget_report.adapter_budget.http_body_max_bytes {
            return Err(bm_sdk::Error::config(
                "mcp_stdio_write",
                "JSON-RPC response exceeds pinned runtime adapter budget",
            ));
        }
        writeln!(writer, "{response}")
            .map_err(|err| bm_sdk::Error::config("mcp_stdio_write", err.to_string()))?;
    }
    Ok(())
}

#[cfg(feature = "server-stdio")]
pub fn handle_mcp_streamable_http_in_process_request(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    body: &str,
) -> bm_sdk::Result<McpStreamableHttpResponse> {
    let lease = acquire_runtime_budget_lease(runtime)?;
    let auth = runtime
        .authenticate_local_transport(EntryLocalTransport::InProcess, &server.local_principal);
    runtime.execute_with_budget_lease(&lease, || {
        handle_mcp_streamable_http_in_process_request_with_budget_lease(
            server,
            runtime,
            body,
            &lease,
            &auth,
            McpRequestTransport::InProcess,
        )
    })
}

#[cfg(feature = "server-stdio")]
fn handle_mcp_streamable_http_in_process_request_with_budget_lease(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    body: &str,
    lease: &EntryRuntimeBudgetLease,
    auth: &EntryAuthDecision,
    transport: McpRequestTransport,
) -> bm_sdk::Result<McpStreamableHttpResponse> {
    let budget_report = lease.report();
    if body.len() > budget_report.adapter_budget.http_body_max_bytes {
        return Err(bm_sdk::Error::config(
            "mcp_http_body",
            "MCP HTTP body exceeds pinned runtime adapter budget",
        ));
    }
    let request: Value = serde_json::from_str(body)
        .map_err(|err| bm_sdk::Error::config("mcp_http_json", err.to_string()))?;
    match handle_mcp_json_rpc_request(server, runtime, request, lease, auth, transport)? {
        Some(response) => {
            let body = bind_mcp_budget_meta(response, budget_report).to_string();
            if body.len() > budget_report.adapter_budget.http_body_max_bytes {
                return Err(bm_sdk::Error::config(
                    "mcp_http_write",
                    "JSON-RPC response exceeds pinned runtime adapter budget",
                ));
            }
            Ok(McpStreamableHttpResponse {
                status: 200,
                content_type: "application/json".to_string(),
                body,
                budget_report_id: budget_report.report_id.clone(),
            })
        }
        None => Ok(McpStreamableHttpResponse {
            status: 202,
            content_type: String::new(),
            body: String::new(),
            budget_report_id: budget_report.report_id.clone(),
        }),
    }
}

#[cfg(feature = "server-stdio")]
pub fn serve_mcp_streamable_http_accepted_stream(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    stream: &mut EntryAcceptedTcpStream,
) -> bm_sdk::Result<()> {
    let lease = acquire_runtime_budget_lease(runtime)?;
    let budget_report = lease.report();
    let ingress = read_authorized_http_request(
        stream,
        EntryHttpIngressLimits::new(
            budget_report.adapter_budget.http_header_max_bytes,
            budget_report.adapter_budget.http_body_max_bytes,
        )
        .map_err(|error| bm_sdk::Error::config("mcp_http_read", error.to_string()))?,
        |accepted, head| {
            let auth = runtime.authenticate_accepted_tcp_stream(
                accepted,
                head.header("authorization"),
                &server.local_principal,
            );
            EntryHttpAuthorization::require(auth, EntryOperationCapability::McpProtocol)
        },
    );
    let ingress = match ingress {
        Ok(ingress) => ingress,
        Err(error) if error.kind() == EntryHttpIngressErrorKind::Unauthorized => {
            return write_http_json_rejection_response(
                stream,
                401,
                mcp_auth_error(-32001, error.message()),
                budget_report.report_id.as_str(),
            );
        }
        Err(error) if error.kind() == EntryHttpIngressErrorKind::Forbidden => {
            return write_http_json_rejection_response(
                stream,
                403,
                mcp_auth_error(-32003, error.message()),
                budget_report.report_id.as_str(),
            );
        }
        Err(error) => {
            if error.kind() == EntryHttpIngressErrorKind::PayloadTooLarge {
                return Err(bm_sdk::Error::config(
                    "mcp_http_read",
                    "HTTP body exceeds pinned runtime adapter budget",
                ));
            }
            return Err(bm_sdk::Error::config("mcp_http_read", error.to_string()));
        }
    };
    let (head, body_bytes, auth) = ingress.into_parts();
    let request = HttpRequest {
        method: head.method().to_string(),
        path: head.target().to_string(),
        headers: head.headers().clone(),
        body: String::from_utf8(body_bytes)
            .map_err(|error| bm_sdk::Error::config("mcp_http_read", error.to_string()))?,
    };
    if request.method == "POST" && !request.headers.contains_key("content-length") {
        return Err(bm_sdk::Error::config(
            "mcp_http_read",
            "content-length is required for MCP Streamable HTTP",
        ));
    }
    if !auth.is_authenticated() {
        return write_http_json_response(
            stream,
            401,
            mcp_auth_error(
                -32001,
                auth.rejection_reason()
                    .unwrap_or("MCP HTTP authentication failed"),
            ),
            budget_report.report_id.as_str(),
        );
    }
    if !auth.allows(EntryOperationCapability::McpProtocol) {
        return write_http_json_response(
            stream,
            403,
            mcp_auth_error(-32003, "principal lacks MCP protocol capability"),
            budget_report.report_id.as_str(),
        );
    }
    if request
        .headers
        .get("content-type")
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        != Some("application/json")
    {
        return write_http_json_response(
            stream,
            415,
            mcp_auth_error(-32600, "MCP Streamable HTTP requires application/json"),
            budget_report.report_id.as_str(),
        );
    }
    if request
        .headers
        .get("mcp-protocol-version")
        .is_some_and(|version| version != MCP_PROTOCOL_VERSION)
    {
        return write_http_json_response(
            stream,
            400,
            mcp_auth_error(-32600, "unsupported MCP protocol version header"),
            budget_report.report_id.as_str(),
        );
    }
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
                budget_report.report_id.as_str(),
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
            budget_report.report_id.as_str(),
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
            budget_report.report_id.as_str(),
        );
    }
    let response = runtime.execute_with_budget_lease(&lease, || {
        handle_mcp_streamable_http_in_process_request_with_budget_lease(
            server,
            runtime,
            &request.body,
            &lease,
            &auth,
            McpRequestTransport::StreamableHttp,
        )
    })?;
    write_http_response(
        stream,
        response.status,
        response.content_type.as_str(),
        response.body.as_str(),
        response.budget_report_id.as_str(),
    )
}

#[cfg(feature = "server-stdio")]
pub fn validate_mcp_http_listener_security(
    runtime: &EntryRuntime,
    local_addr: SocketAddr,
) -> bm_sdk::Result<()> {
    if !local_addr.ip().is_loopback() && !runtime.has_bearer_verifier() {
        return Err(bm_sdk::Error::config(
            "mcp_http_listener_auth",
            "non-loopback MCP HTTP bind requires a configured bearer verifier",
        ));
    }
    Ok(())
}

#[cfg(feature = "server-stdio")]
fn handle_mcp_json_rpc_request(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    request: Value,
    lease: &EntryRuntimeBudgetLease,
    auth: &EntryAuthDecision,
    transport: McpRequestTransport,
) -> bm_sdk::Result<Option<Value>> {
    let budget_report = lease.report();
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
    let snapshot = McpCapabilitySnapshot::for_request(runtime, auth, transport)?;
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
            let mut capabilities = serde_json::Map::new();
            if !snapshot.tools.is_empty() {
                capabilities.insert("tools".to_string(), json!({"listChanged": false}));
            }
            if !snapshot.resources.is_empty() {
                capabilities.insert(
                    "resources".to_string(),
                    json!({"subscribe": false, "listChanged": false}),
                );
            }
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": capabilities,
                "serverInfo": {
                    "name": MCP_SERVER_NAME,
                    "title": "Beetle Memory MCP Server",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": "Use only the tools and resources advertised for this authenticated request. Raw private memory is not exposed.",
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({
            "tools": snapshot.tools
                .iter()
                .map(mcp_tool_descriptor)
                .collect::<Vec<_>>(),
        }),
        "tools/call" => {
            let Some(params) = request.get("params") else {
                return Ok(Some(mcp_error(id, -32602, "missing params")));
            };
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Ok(Some(mcp_error(id, -32602, "missing tool name")));
            };
            if snapshot.tool(name).is_none() {
                return Ok(Some(mcp_error(id, -32602, format!("Unknown tool: {name}"))));
            }
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}))
                .to_string();
            if arguments.len() > budget_report.adapter_budget.http_body_max_bytes {
                return Ok(Some(mcp_error(
                    id,
                    -32602,
                    "tool arguments exceed pinned runtime adapter budget",
                )));
            }
            let mut call = McpToolCall::json(name, arguments);
            if let Some(idempotency_key) = params
                .pointer("/_meta/idempotencyKey")
                .and_then(Value::as_str)
            {
                call = call.with_idempotency_key(idempotency_key);
            }
            match server.call_with_budget_lease(runtime, call, lease, auth, &snapshot) {
                Ok(tool_result) => render_mcp_tool_call_result(tool_result),
                Err(error) => render_mcp_tool_execution_error(error.to_string()),
            }
        }
        "resources/list" => json!({
            "resources": snapshot.resources
                .iter()
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
            if snapshot.resource(uri).is_none() {
                return Ok(Some(mcp_error(
                    id,
                    -32602,
                    format!("Unknown resource: {uri}"),
                )));
            }
            let resource = server.read_resource_with_budget_lease(
                runtime,
                McpResourceRead::new(uri),
                lease,
                auth,
                &snapshot,
            )?;
            json!({
                "contents": [{
                    "uri": resource.uri,
                    "mimeType": resource.mime_type,
                    "text": resource.content,
                    "_meta": {
                        "private_raw_allowed": resource.private_raw_allowed,
                        "runtimeBudgetReportId": resource.budget_report_id,
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
#[cfg(feature = "server-stdio")]
fn write_http_json_response(
    stream: &mut impl Write,
    status: u16,
    body: String,
    budget_report_id: &str,
) -> bm_sdk::Result<()> {
    write_http_response(
        stream,
        status,
        "application/json",
        body.as_str(),
        budget_report_id,
    )
}

#[cfg(feature = "server-stdio")]
fn write_http_json_rejection_response(
    stream: &mut EntryAcceptedTcpStream,
    status: u16,
    body: String,
    budget_report_id: &str,
) -> bm_sdk::Result<()> {
    write_http_json_response(stream, status, body, budget_report_id)?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .and_then(|_| stream.discard_currently_buffered_input())
        .map_err(|error| bm_sdk::Error::config("mcp_http_rejection_close", error.to_string()))
}

#[cfg(feature = "server-stdio")]
fn write_http_response(
    stream: &mut impl Write,
    status: u16,
    content_type: &str,
    body: &str,
    budget_report_id: &str,
) -> bm_sdk::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        415 => "Unsupported Media Type",
        _ => "OK",
    };
    if content_type.is_empty() {
        write!(
            stream,
            "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nx-bm-runtime-budget-report-id: {budget_report_id}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .map_err(|error| bm_sdk::Error::config("mcp_http_write", error.to_string()))
    } else {
        write!(
            stream,
            "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nx-bm-runtime-budget-report-id: {budget_report_id}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .map_err(|error| bm_sdk::Error::config("mcp_http_write", error.to_string()))
    }
}

#[cfg(feature = "server-stdio")]
fn mcp_origin_allowed(origin: &str) -> bool {
    let normalized = origin.trim().to_ascii_lowercase();
    if normalized == "null" || normalized == "file://" {
        return true;
    }
    let Some((scheme, authority)) = normalized.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https")
        || authority.is_empty()
        || authority
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'?' | b'#' | b'@'))
    {
        return false;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return false;
        };
        if host != "::1" {
            return false;
        }
        let port = if suffix.is_empty() {
            None
        } else {
            let Some(port) = suffix.strip_prefix(':') else {
                return false;
            };
            Some(port)
        };
        ("::1", port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host, Some(port))
    } else {
        (authority, None)
    };
    if !matches!(host, "localhost" | "127.0.0.1" | "::1") {
        return false;
    }
    port.is_none_or(valid_origin_port)
}

#[cfg(feature = "server-stdio")]
fn valid_origin_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|port| port != 0)
}

#[cfg(feature = "server-stdio")]
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
fn mcp_auth_error(code: i64, message: impl Into<String>) -> String {
    mcp_error(Value::Null, code, message).to_string()
}

#[cfg(feature = "server-stdio")]
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct McpInspectArguments {
    query: String,
    system_max_len: usize,
}

#[cfg(feature = "server-stdio")]
impl McpInspectArguments {
    fn input_schema() -> Value {
        bounded_query_schema()
    }
}

#[cfg(feature = "server-stdio")]
fn bounded_query_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"},
            "system_max_len": {"type": "integer", "minimum": 1}
        },
        "required": ["query", "system_max_len"],
        "additionalProperties": false
    })
}

#[cfg(feature = "server-stdio")]
fn normalize_typed_tool_arguments(name: &str, arguments: &str) -> bm_sdk::Result<String> {
    match name {
        "memory_inspect" => serde_json::from_str::<McpInspectArguments>(arguments)
            .and_then(|arguments| serde_json::to_string(&arguments))
            .map_err(|error| bm_sdk::Error::config("mcp_tool_arguments", error.to_string())),
        _ => Ok(arguments.to_string()),
    }
}

#[cfg(feature = "server-stdio")]
fn mcp_tool_descriptor(tool: &McpToolSpec) -> Value {
    json!({
        "name": tool.name,
        "title": tool.name.replace('_', " "),
        "description": tool_description(tool.name),
        "inputSchema": mcp_tool_input_schema(tool),
        "_meta": {
            "adapter_operation": tool.operation.as_str(),
            "private_raw_allowed": tool.private_raw_allowed,
        },
    })
}

#[cfg(feature = "server-stdio")]
fn mcp_tool_input_schema(tool: &McpToolSpec) -> Value {
    if let Some(schema) = governed_adapter_json_command_schema(tool.operation) {
        return schema.input_schema;
    }
    if tool.name == "memory_inspect" {
        return McpInspectArguments::input_schema();
    }
    let fields = &tool.schema_fields;
    let properties = fields
        .iter()
        .map(|field| {
            (
                field.clone(),
                match field.as_str() {
                    "limit" | "system_max_len" | "recent_messages_limit" => json!({
                        "type": "integer",
                        "minimum": 1,
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
                "limit" | "system_max_len" | "recent_messages_limit"
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
        "memory_finalize_turn" => {
            "Commit one canonical turn through the runtime-owned transcript and post-turn path."
        }
        "memory_recall" => "Recall governed memory snippets for an explicit query.",
        "memory_project" => "Build a bounded memory projection preview for an explicit query.",
        "memory_inspect" => "Inspect governed memory state without exposing raw private planes.",
        "memory_replay" => "Replay safe memory timeline events for a chat scope.",
        "memory_write_candidate" => "Submit a governed procedural memory write candidate.",
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
            "runtimeBudgetReportId": tool_result.budget_report_id,
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
            let content = if let Some(governed) = report.governed_safe_report() {
                json!({"status":"accepted","result":governed}).to_string()
            } else {
                match report {
                    AdapterSdkReport::Recall(_) | AdapterSdkReport::Project(_) => {
                        unreachable!("governed DTO handled above")
                    }
                    AdapterSdkReport::Capabilities(catalog) => json!({
                        "status": "accepted",
                        "profile": catalog.profile.as_str(),
                    })
                    .to_string(),
                    AdapterSdkReport::FinalizeTurn(report) => json!({
                        "status": "accepted",
                        "operation": "finalize_turn",
                        "result": report,
                    })
                    .to_string(),
                    AdapterSdkReport::TranscriptAttrWrite(report) => json!({
                        "status": "accepted",
                        "memory_space_id": report.key.memory_space_id,
                        "channel_id": report.key.channel_id,
                        "conversation_id": report.key.conversation_id,
                        "accepted_attrs": report.accepted_attrs,
                        "rejected_attrs": report.rejected_attrs,
                        "redactions_preview": report.redactions_preview,
                        "profile_budget_applied": report.profile_budget_applied,
                        "audit_event_id": report.audit_event_id,
                        "dry_run": report.dry_run,
                        "lifecycle": report.lifecycle_report.result_summary,
                    })
                    .to_string(),
                    other => json!({
                        "status": "accepted",
                        "report_kind": other.public_kind(),
                    })
                    .to_string(),
                }
            };
            McpToolResult {
                status: "accepted".to_string(),
                content,
                private_raw_allowed: false,
                budget_report_id: String::new(),
            }
        }
        AdapterResponse::Rejected { reason, .. } => McpToolResult {
            status: "rejected".to_string(),
            content: json!({"status":"rejected","reason":reason}).to_string(),
            private_raw_allowed: false,
            budget_report_id: String::new(),
        },
        AdapterResponse::Queued { queue, .. } => McpToolResult {
            status: "queued".to_string(),
            content: json!({"status":"queued","queue":queue}).to_string(),
            private_raw_allowed: false,
            budget_report_id: String::new(),
        },
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => McpToolResult {
            status: "duplicated".to_string(),
            content: json!({"status":"duplicated","idempotency_key":idempotency_key}).to_string(),
            private_raw_allowed: false,
            budget_report_id: String::new(),
        },
    }
}

#[cfg(feature = "server-stdio")]
fn render_projection_preview_resource(
    response: AdapterResponse<AdapterSdkReport>,
) -> bm_sdk::Result<Value> {
    match response {
        AdapterResponse::Accepted { report, .. } => match report.governed_safe_report() {
            Some(bm_adapter::AdapterGovernedSafeReportV1::Project(project)) => Ok(json!({
                "status": "accepted",
                "result": bm_adapter::AdapterGovernedSafeReportV1::Project(project),
                "private_raw_allowed": false,
            })),
            _ => Err(bm_sdk::Error::config(
                "mcp_resource",
                "projection preview returned non-project governed DTO",
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
