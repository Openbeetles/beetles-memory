#![cfg(feature = "server-stdio")]

mod support;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
use bm_mcp::{
    handle_mcp_streamable_http_in_process_request, McpResourceRead, McpToolCall, McpToolServer,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig};
use serde_json::Value;

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "mcp-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "mcp".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

fn write_arguments(name: &str, summary: &str) -> String {
    serde_json::json!({
        "name": name,
        "topic": "mcp-idempotency",
        "title": format!("MCP write {name}"),
        "summary": summary,
        "content": "Dispatch this write through the governed EntryRuntime path.",
        "owning_scope": {
            "kind": "subject",
            "mounted_subject_id": "agent:mcp-agent",
        },
        "creation_ref": {
            "kind": "replay_promotion",
            "candidate_ref": format!("test:mcp:{name}"),
            "verification_receipt_digest":
                "sha256:3333333333333333333333333333333333333333333333333333333333333333",
        },
        "privacy_class": "shared_with_subject",
    })
    .to_string()
}

fn assert_exact_governed_result(content: &str) {
    let value: Value = serde_json::from_str(content).expect("governed response JSON");
    let result = value["result"].clone();
    let dto: bm_adapter::AdapterGovernedSafeReportV1 =
        serde_json::from_value(result.clone()).expect("strict adapter governed safe DTO");
    assert_eq!(
        serde_json::to_value(dto).expect("serialize adapter governed safe DTO"),
        result
    );
}

#[test]
fn mcp_automatic_identity_accepts_two_distinct_writes_on_one_server() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-auto", "principal-auto");

    let first = server
        .call(
            &runtime,
            McpToolCall::json(
                "memory_write_candidate",
                write_arguments("runtime_skill__mcp_auto_first", "first automatic write"),
            ),
        )
        .expect("first write");
    let second = server
        .call(
            &runtime,
            McpToolCall::json(
                "memory_write_candidate",
                write_arguments("runtime_skill__mcp_auto_second", "second automatic write"),
            ),
        )
        .expect("second write");

    assert_eq!(first.status, "accepted");
    assert_eq!(second.status, "accepted");
}

#[test]
fn mcp_explicit_identity_replays_same_payload_and_rejects_conflict() {
    let runtime = runtime();
    let initial_server = McpToolServer::new("mcp-explicit", "principal-explicit");
    let retry_server = McpToolServer::new("mcp-explicit-retry", "principal-explicit");
    let arguments = write_arguments("runtime_skill__mcp_explicit", "stable payload");

    let first = initial_server
        .call(
            &runtime,
            McpToolCall::json("memory_write_candidate", arguments.clone())
                .with_idempotency_key("mcp-caller-key"),
        )
        .expect("first write");
    let replay = retry_server
        .call(
            &runtime,
            McpToolCall::json("memory_write_candidate", arguments)
                .with_idempotency_key("mcp-caller-key"),
        )
        .expect("replay write");
    let conflict = retry_server
        .call(
            &runtime,
            McpToolCall::json(
                "memory_write_candidate",
                write_arguments("runtime_skill__mcp_conflict", "different payload"),
            )
            .with_idempotency_key("mcp-caller-key"),
        )
        .expect("conflicting write");

    assert_eq!(first.status, "accepted");
    assert_eq!(replay.status, "duplicated");
    assert_eq!(conflict.status, "rejected");
    assert!(
        !replay.content.contains("mcp-caller-key"),
        "{}",
        replay.content
    );
    assert!(
        replay.content.contains("explicit:v1:sha256:"),
        "{}",
        replay.content
    );
}

#[test]
fn mcp_explicit_identity_isolated_by_authenticated_principal() {
    let runtime = runtime();
    let principal_a = McpToolServer::new("shared-server", "principal-a");
    let principal_b = McpToolServer::new("shared-server", "principal-b");

    let first = principal_a
        .call(
            &runtime,
            McpToolCall::json(
                "memory_write_candidate",
                write_arguments("runtime_skill__mcp_principal_a", "principal A"),
            )
            .with_idempotency_key("shared-caller-key"),
        )
        .expect("principal A write");
    let second = principal_b
        .call(
            &runtime,
            McpToolCall::json(
                "memory_write_candidate",
                write_arguments("runtime_skill__mcp_principal_b", "principal B"),
            )
            .with_idempotency_key("shared-caller-key"),
        )
        .expect("principal B write");

    assert_eq!(first.status, "accepted");
    assert_eq!(second.status, "accepted");
}

#[test]
fn mcp_tool_call_dispatches_through_entry_runtime_without_private_raw() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-server-1", "mcp-client-1");
    let result = server
        .call(
            &runtime,
            McpToolCall::json(
                "memory_recall",
                r#"{"temporal_operation":{"kind":"current"},"query":"release","limit":2}"#,
            ),
        )
        .expect("tool call");

    assert_eq!(result.status, "accepted");
    assert!(!result.private_raw_allowed);
    assert!(result.content.contains("\"query\""));
    assert!(result.budget_report_id.starts_with("rtb-v2-"));
    assert_exact_governed_result(&result.content);
}

#[test]
fn mcp_tool_server_decodes_declared_memory_tools() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-server-ops", "mcp-client-ops");
    let calls = [
        ("memory_capabilities", r#"{}"#),
        (
            "memory_project",
            r#"{"temporal_operation":{"kind":"current"},"user_query":"release","system_max_len":1024}"#,
        ),
        (
            "memory_inspect",
            r#"{"query":"release","system_max_len":1024}"#,
        ),
        ("memory_long_term_list", r#"{"query":{},"limit":2}"#),
        (
            "memory_write_candidate",
            r#"{"name":"runtime_skill__mcp_write","topic":"mcp","title":"MCP write","summary":"MCP write summary","content":"1. Decode MCP tool payload.\n2. Dispatch through EntryRuntime.","owning_scope":{"kind":"subject","mounted_subject_id":"agent:mcp-agent"},"creation_ref":{"kind":"replay_promotion","candidate_ref":"test:mcp:runtime_skill__mcp_write","verification_receipt_digest":"sha256:4444444444444444444444444444444444444444444444444444444444444444"},"privacy_class":"shared_with_subject"}"#,
        ),
    ];

    for (name, args) in calls {
        let result = server
            .call(&runtime, McpToolCall::json(name, args))
            .unwrap_or_else(|err| panic!("{name} failed: {err}"));
        assert_eq!(result.status, "accepted", "{name}: {}", result.content);
        assert!(!result.private_raw_allowed);
    }
}

#[test]
fn mcp_fallback_uses_stable_public_report_kind_without_debug_wire() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-public-kind", "mcp-public-kind-client");
    let result = server
        .call(
            &runtime,
            McpToolCall::json(
                "memory_inspect",
                r#"{"query":"release","system_max_len":1024}"#,
            ),
        )
        .expect("inspect tool call");
    let content: Value = serde_json::from_str(&result.content).expect("inspect result JSON");

    assert_eq!(content["status"], "accepted");
    assert_eq!(content["report_kind"], "inspect");
    assert!(!result.content.contains("MemoryInspectionReport"));
}

#[test]
fn mcp_project_tool_exposes_only_the_adapter_projection_surface() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-project-boundary", "mcp-project-client");
    let result = server
        .call(
            &runtime,
            McpToolCall::json(
                "memory_project",
                r#"{"temporal_operation":{"kind":"current"},"user_query":"release","system_max_len":1024}"#,
            ),
        )
        .expect("project tool call");
    let content: Value = serde_json::from_str(&result.content).expect("project tool json");

    assert_eq!(content["status"], "accepted");
    assert_eq!(content["result"]["operation"], "project");
    assert_eq!(
        content["result"]["report"]["temporal_operation"]["kind"],
        "current"
    );
    assert!(content["result"]["report"]
        .get("projection_block")
        .is_some());
    assert!(content["result"]["report"].get("chars").is_some());
    assert!(content["result"]["report"].get("governed_recall").is_some());
    assert!(content.get("system_memory_block").is_none());
    assert!(content.get("projection_surface").is_none());
    assert!(!result.content.contains("runtime_projection"));
    assert!(!result.content.contains("delivery_digest_manifest"));
}

#[test]
fn mcp_resource_read_uses_entry_runtime_safe_reports_without_private_raw() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-resource", "mcp-resource-client");

    for uri in [
        "memory://profile",
        "memory://scope",
        "memory://projection-preview",
    ] {
        let resource = server
            .read_resource(&runtime, McpResourceRead::new(uri))
            .unwrap_or_else(|err| panic!("{uri} failed: {err}"));
        assert_eq!(resource.uri, uri);
        assert_eq!(resource.mime_type, "application/json");
        assert!(!resource.private_raw_allowed);
        assert!(resource.budget_report_id.starts_with("rtb-v2-"));
        assert!(!resource.content.contains("\"private_raw\":true"), "{uri}");
        assert!(!resource.content.contains("raw_content"), "{uri}");
        assert!(!resource.content.contains("store_schema"), "{uri}");
        assert!(!resource.content.contains("system_memory_block"), "{uri}");
    }
}

#[test]
fn streamable_http_handles_single_json_rpc_resource_request() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http", "mcp-http-client");
    let response = handle_mcp_streamable_http_in_process_request(
        &server,
        &runtime,
        r#"{"jsonrpc":"2.0","id":"r1","method":"resources/list"}"#,
    )
    .expect("streamable http response");

    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "application/json");
    assert!(response.budget_report_id.starts_with("rtb-v2-"));
    assert!(
        response.body.contains(r#""jsonrpc":"2.0""#),
        "{}",
        response.body
    );
    assert!(response.body.contains(r#""id":"r1""#), "{}", response.body);
    assert!(
        response.body.contains("memory://profile"),
        "{}",
        response.body
    );
    assert!(
        !response.body.contains("private_raw\":true"),
        "{}",
        response.body
    );
}

#[test]
fn json_rpc_initialize_negotiates_mcp_capabilities() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http", "mcp-http-client");
    let response = handle_mcp_streamable_http_in_process_request(
        &server,
        &runtime,
        r#"{"jsonrpc":"2.0","id":"init-1","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"contract","version":"1.0.0"}}}"#,
    )
    .expect("initialize response");
    let body: Value = serde_json::from_str(&response.body).expect("json response");
    let result = body.get("result").expect("initialize result");

    assert_eq!(
        result.get("protocolVersion").and_then(Value::as_str),
        Some("2025-11-25")
    );
    assert!(result.pointer("/capabilities/tools").is_some(), "{body}");
    assert!(
        result.pointer("/capabilities/resources").is_some(),
        "{body}"
    );
    assert_eq!(
        result.pointer("/serverInfo/name").and_then(Value::as_str),
        Some("bm-mcp-server")
    );
}

#[test]
fn json_rpc_tools_list_uses_mcp_input_schema_shape() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http", "mcp-http-client");
    let response = handle_mcp_streamable_http_in_process_request(
        &server,
        &runtime,
        r#"{"jsonrpc":"2.0","id":"tools-1","method":"tools/list"}"#,
    )
    .expect("tools list response");
    let body: Value = serde_json::from_str(&response.body).expect("json response");
    let tools = body
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .expect("tools array");
    let recall = tools
        .iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some("memory_recall"))
        .expect("memory_recall tool");

    assert!(recall.get("inputSchema").is_some(), "{recall}");
    assert!(
        recall.pointer("/inputSchema/properties/query").is_some(),
        "{recall}"
    );
    assert!(
        recall
            .pointer("/inputSchema/properties/temporal_operation")
            .is_some(),
        "{recall}"
    );
    assert!(
        recall.pointer("/inputSchema/properties/limit").is_some(),
        "{recall}"
    );
    assert!(recall.get("schema_fields").is_none(), "{recall}");
}

#[test]
fn json_rpc_tools_call_uses_mcp_content_and_structured_content_shape() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http", "mcp-http-client");
    let response = handle_mcp_streamable_http_in_process_request(
        &server,
        &runtime,
        r#"{"jsonrpc":"2.0","id":"call-1","method":"tools/call","params":{"name":"memory_capabilities","arguments":{}}}"#,
    )
    .expect("tools call response");
    let body: Value = serde_json::from_str(&response.body).expect("json response");
    let result = body.get("result").expect("tool result");
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .expect("content array");

    assert_eq!(content.len(), 1);
    assert_eq!(content[0].get("type").and_then(Value::as_str), Some("text"));
    assert!(content[0].get("text").and_then(Value::as_str).is_some());
    assert_eq!(
        result
            .pointer("/structuredContent/status")
            .and_then(Value::as_str),
        Some("accepted")
    );
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(false));
    assert!(result.get("status").is_none(), "{result}");
}

#[test]
fn json_rpc_tool_call_uses_only_explicit_meta_key_for_retry_deduplication() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http-idempotency", "mcp-http-principal");
    let arguments: Value = serde_json::from_str(&write_arguments(
        "runtime_skill__mcp_http_explicit",
        "stable JSON-RPC payload",
    ))
    .expect("write arguments");
    let request = |id: &str| {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": "memory_write_candidate",
                "arguments": arguments.clone(),
                "_meta": {"idempotencyKey": "mcp-http-caller-key"},
            },
        })
        .to_string()
    };

    let first =
        handle_mcp_streamable_http_in_process_request(&server, &runtime, &request("write-1"))
            .expect("first JSON-RPC write");
    let replay =
        handle_mcp_streamable_http_in_process_request(&server, &runtime, &request("write-2"))
            .expect("replayed JSON-RPC write");
    let first: Value = serde_json::from_str(&first.body).expect("first response JSON");
    let replay: Value = serde_json::from_str(&replay.body).expect("replay response JSON");

    assert!(!first.to_string().contains("mcp-http-caller-key"));
    assert!(!replay.to_string().contains("mcp-http-caller-key"));

    assert_eq!(
        first
            .pointer("/result/structuredContent/status")
            .and_then(Value::as_str),
        Some("accepted")
    );
    assert_eq!(
        replay
            .pointer("/result/structuredContent/status")
            .and_then(Value::as_str),
        Some("duplicated")
    );
}

#[test]
fn json_rpc_resource_read_returns_text_resource_contents() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http", "mcp-http-client");
    let response = handle_mcp_streamable_http_in_process_request(
        &server,
        &runtime,
        r#"{"jsonrpc":"2.0","id":"res-1","method":"resources/read","params":{"uri":"memory://scope"}}"#,
    )
    .expect("resource read response");
    let body: Value = serde_json::from_str(&response.body).expect("json response");
    let content = body
        .pointer("/result/contents/0")
        .expect("first resource content");

    assert_eq!(
        content.get("uri").and_then(Value::as_str),
        Some("memory://scope")
    );
    assert_eq!(
        content.get("mimeType").and_then(Value::as_str),
        Some("application/json")
    );
    assert!(
        content.get("text").and_then(Value::as_str).is_some(),
        "{content}"
    );
    assert_eq!(
        content
            .pointer("/_meta/private_raw_allowed")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(
        body.pointer("/result/private_raw_allowed").is_none(),
        "{body}"
    );
}
