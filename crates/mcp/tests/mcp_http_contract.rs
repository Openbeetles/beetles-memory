#![cfg(feature = "server-stdio")]

mod support;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};

use bm_entry::{
    EntryAcceptedTcpStream, EntryAuthConfig, EntryBearerPrincipal, EntryIdempotencyConfig,
    EntryIdentity, EntryOperationCapability, EntryRuntime, EntryRuntimeConfig, EntryScope,
    EntryTransportConfig,
};
use bm_mcp::{
    serve_mcp_streamable_http_accepted_stream, validate_mcp_http_listener_security, McpToolServer,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "mcp-http-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "mcp-http".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: mcp_only_transport(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

fn remote_runtime(
    capabilities: impl IntoIterator<Item = EntryOperationCapability>,
) -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    remote_runtime_with(
        capabilities,
        capability,
        MemoryPrivacyPolicy::standard_private_boundary(),
    )
}

fn remote_runtime_with(
    capabilities: impl IntoIterator<Item = EntryOperationCapability>,
    capability: MemoryCapabilityPolicy,
    privacy: MemoryPrivacyPolicy,
) -> EntryRuntime {
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "mcp-http-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "mcp-http".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: mcp_only_transport(),
        auth: EntryAuthConfig::required_bearer_principal(
            "secret-token",
            EntryBearerPrincipal::new("mcp-http-principal", "owner-default", capabilities),
        ),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy,
        capability,
    })
    .expect("entry runtime")
}

fn mcp_only_transport() -> EntryTransportConfig {
    EntryTransportConfig {
        cli: false,
        http_server: false,
        wss_client: false,
        wss_server: false,
        mcp_server: true,
        a2a_bridge: false,
        llm_gateway_server: false,
    }
}

fn authorized_json_request(body: &str) -> String {
    format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer secret-token\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
}

fn response_json(response: &str) -> serde_json::Value {
    let (_, body) = response
        .split_once("\r\n\r\n")
        .expect("HTTP response body separator");
    serde_json::from_str(body).expect("MCP JSON response")
}

fn serve_mcp_request(
    server: &McpToolServer,
    runtime: &EntryRuntime,
    request: String,
) -> (bm_sdk::Result<()>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind MCP test listener");
    let addr = listener.local_addr().expect("MCP test address");
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).expect("connect MCP test listener");
        stream
            .write_all(request.as_bytes())
            .expect("write MCP request");
        stream
            .shutdown(Shutdown::Write)
            .expect("shutdown MCP request");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("read MCP response");
        response
    });
    let mut accepted = EntryAcceptedTcpStream::accept(&listener).expect("accept MCP test peer");
    let result = serve_mcp_streamable_http_accepted_stream(server, runtime, &mut accepted);
    drop(accepted);
    (result, client.join().expect("MCP test client"))
}

#[test]
fn streamable_http_stream_serves_json_rpc_resources() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http-stream", "mcp-http-client");
    let body = r#"{"jsonrpc":"2.0","id":"r1","method":"resources/list"}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let (result, response) = serve_mcp_request(&server, &runtime, request);
    result.expect("serve streamable http");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(
        response.contains("content-type: application/json\r\n"),
        "{response}"
    );
    assert!(
        response.contains("x-bm-runtime-budget-report-id: rtb-v2-"),
        "{response}"
    );
    assert!(
        response.contains("memory://projection-preview"),
        "{response}"
    );
    assert!(
        !response.contains("private_raw_allowed\":true"),
        "{response}"
    );
}

#[test]
fn streamable_http_stream_returns_accepted_for_notifications() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http-notification", "mcp-http-client");
    let body = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let (result, response) = serve_mcp_request(&server, &runtime, request);
    result.expect("serve streamable http notification");
    assert!(
        response.starts_with("HTTP/1.1 202 Accepted\r\n"),
        "{response}"
    );
    assert!(response.ends_with("\r\n\r\n"), "{response}");
    assert!(!response.contains("\"jsonrpc\""), "{response}");
}

#[test]
fn streamable_http_stream_rejects_invalid_origin() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http-origin", "mcp-http-client");
    let body = r#"{"jsonrpc":"2.0","id":"r1","method":"resources/list"}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nOrigin: https://evil.example\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let (result, response) = serve_mcp_request(&server, &runtime, request);
    result.expect("serve streamable http invalid origin");
    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden\r\n"),
        "{response}"
    );
    assert!(response.contains("invalid MCP Origin"), "{response}");
}

#[test]
fn streamable_http_rejects_declared_body_before_allocating_or_reading_it() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http-declared-limit", "mcp-http-client");
    let over_budget = runtime.runtime_budget().adapter_budget.http_body_max_bytes + 1;
    let request =
        format!("POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Length: {over_budget}\r\n\r\n");
    let (result, response) = serve_mcp_request(&server, &runtime, request);
    let error = result.expect_err("declared oversized body must fail before body allocation");

    assert_eq!(error.stage(), "mcp_http_read");
    assert!(error.to_string().contains("exceeds pinned"));
    assert!(response.is_empty());
}

#[test]
fn streamable_http_rejects_noncanonical_length_and_invalid_header_names() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http-framing", "mcp-http-local");
    for request in [
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Length: +0\r\n\r\n",
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\n Content-Length: 0\r\n\r\n",
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Length : 0\r\n\r\n",
        "POST\t/mcp\tHTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
    ] {
        let (result, response) = serve_mcp_request(&server, &runtime, request.to_string());
        let error = result.expect_err("noncanonical HTTP framing must fail closed");
        assert_eq!(error.stage(), "mcp_http_read");
        assert!(response.is_empty());
    }
}

#[test]
fn streamable_http_origin_requires_exact_loopback_authority() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http-origin-authority", "mcp-http-local");
    let body = r#"{"jsonrpc":"2.0","id":"origin","method":"ping"}"#;

    for origin in [
        "http://localhost.evil.example",
        "http://localhost@evil.example",
        "http://127.0.0.1.evil.example",
        "http://[::1].evil.example",
    ] {
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let (result, response) = serve_mcp_request(&server, &runtime, request);
        result.expect("structured rejection");
        assert!(
            response.starts_with("HTTP/1.1 403 Forbidden"),
            "origin {origin} was not rejected"
        );
    }

    for origin in [
        "http://localhost:8788",
        "https://127.0.0.1:443",
        "http://[::1]:8788",
    ] {
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let (result, response) = serve_mcp_request(&server, &runtime, request);
        result.expect("valid loopback origin");
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "origin {origin} was not accepted"
        );
    }
}

#[test]
fn streamable_http_accepts_body_at_exact_pinned_budget() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http-exact", "mcp-http-local");
    let max_bytes = runtime.runtime_budget().adapter_budget.http_body_max_bytes;
    let mut body = br#"{"jsonrpc":"2.0","id":"exact","method":"ping"}"#.to_vec();
    body.resize(max_bytes, b' ');
    let body = String::from_utf8(body).expect("body");
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let (result, response) = serve_mcp_request(&server, &runtime, request);
    result.expect("exact boundary request");
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(
        response.contains("x-bm-runtime-budget-report-id: rtb-v2-"),
        "{response}"
    );
}

#[test]
fn remote_mcp_http_requires_real_authorization_even_for_loopback_peer() {
    let runtime = remote_runtime([
        EntryOperationCapability::McpProtocol,
        EntryOperationCapability::Capabilities,
    ]);
    let server = McpToolServer::new("mcp-http-auth", "unused-local-principal");
    let body = r#"{"jsonrpc":"2.0","id":"auth","method":"tools/list"}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nx-loopback: true\r\nx-bm-auth-subject: forged-owner\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let (result, response) = serve_mcp_request(&server, &runtime, request);
    result.expect("structured unauthorized response");
    assert!(
        response.starts_with("HTTP/1.1 401 Unauthorized"),
        "{response}"
    );
    assert!(response.contains("missing_bearer_token"), "{response}");
}

#[test]
fn remote_mcp_http_uses_configured_principal_and_capabilities() {
    let runtime = remote_runtime([EntryOperationCapability::McpProtocol]);
    let server = McpToolServer::new("mcp-http-auth", "unused-local-principal");
    let body = r#"{"jsonrpc":"2.0","id":"auth","method":"tools/list"}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer secret-token\r\nx-bm-auth-subject: forged-owner\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let (result, response) = serve_mcp_request(&server, &runtime, request);
    result.expect("authorized MCP response");
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(!response.contains("forged-owner"), "{response}");
}

#[test]
fn remote_mcp_catalog_and_dispatch_share_one_request_capability_snapshot() {
    let runtime = remote_runtime([
        EntryOperationCapability::McpProtocol,
        EntryOperationCapability::Recall,
    ]);
    let server = McpToolServer::new("mcp-http-snapshot", "unused-local-principal");

    let initialize = r#"{"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#;
    let (result, response) =
        serve_mcp_request(&server, &runtime, authorized_json_request(initialize));
    result.expect("initialize with restricted snapshot");
    let response = response_json(&response);
    assert!(response.pointer("/result/capabilities/tools").is_some());
    assert!(response.pointer("/result/capabilities/resources").is_none());

    let tools = r#"{"jsonrpc":"2.0","id":"tools","method":"tools/list"}"#;
    let (result, response) = serve_mcp_request(&server, &runtime, authorized_json_request(tools));
    result.expect("tools/list with restricted snapshot");
    let response = response_json(&response).to_string();
    assert!(response.contains("memory_recall"), "{response}");
    assert!(!response.contains("memory_write_candidate"), "{response}");
    assert!(!response.contains("memory_project"), "{response}");

    let resources = r#"{"jsonrpc":"2.0","id":"resources","method":"resources/list"}"#;
    let (result, response) =
        serve_mcp_request(&server, &runtime, authorized_json_request(resources));
    result.expect("resources/list with restricted snapshot");
    let response = response_json(&response);
    assert_eq!(
        response
            .pointer("/result/resources")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    let forbidden_read = r#"{"jsonrpc":"2.0","id":"read","method":"resources/read","params":{"uri":"memory://profile"}}"#;
    let (result, response) =
        serve_mcp_request(&server, &runtime, authorized_json_request(forbidden_read));
    result.expect("resources/read rejection");
    let response = response_json(&response);
    assert_eq!(
        response
            .pointer("/error/code")
            .and_then(serde_json::Value::as_i64),
        Some(-32602)
    );

    let forbidden_call = r#"{"jsonrpc":"2.0","id":"write","method":"tools/call","params":{"name":"memory_write_candidate","arguments":{}}}"#;
    let (result, response) =
        serve_mcp_request(&server, &runtime, authorized_json_request(forbidden_call));
    result.expect("tools/call rejection");
    let response = response_json(&response);
    assert_eq!(
        response
            .pointer("/error/code")
            .and_then(serde_json::Value::as_i64),
        Some(-32602)
    );

    let recall = r#"{"jsonrpc":"2.0","id":"recall","method":"tools/call","params":{"name":"memory_recall","arguments":{"query":"release","limit":1}}}"#;
    let (result, response) = serve_mcp_request(&server, &runtime, authorized_json_request(recall));
    result.expect("authorized tools/call");
    let response = response_json(&response);
    assert_eq!(
        response
            .pointer("/result/structuredContent/status")
            .and_then(serde_json::Value::as_str),
        Some("accepted")
    );
}

#[test]
fn remote_mcp_snapshot_intersects_policy_transport_and_privacy() {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    capability.projection_enabled = false;
    let mut privacy = MemoryPrivacyPolicy::standard_private_boundary();
    privacy.operator_inspection_allowed = false;
    privacy.export_allowed = false;
    let runtime = remote_runtime_with(
        [
            EntryOperationCapability::McpProtocol,
            EntryOperationCapability::Capabilities,
            EntryOperationCapability::Recall,
            EntryOperationCapability::Project,
            EntryOperationCapability::Inspect,
        ],
        capability,
        privacy,
    );
    let server = McpToolServer::new("mcp-http-policy-snapshot", "unused-local-principal");

    let tools = r#"{"jsonrpc":"2.0","id":"tools","method":"tools/list"}"#;
    let (result, response) = serve_mcp_request(&server, &runtime, authorized_json_request(tools));
    result.expect("tools/list with policy/privacy snapshot");
    let response = response_json(&response).to_string();
    assert!(response.contains("memory_recall"), "{response}");
    assert!(response.contains("memory_capabilities"), "{response}");
    assert!(!response.contains("memory_project"), "{response}");
    assert!(!response.contains("memory_inspect"), "{response}");

    let resources = r#"{"jsonrpc":"2.0","id":"resources","method":"resources/list"}"#;
    let (result, response) =
        serve_mcp_request(&server, &runtime, authorized_json_request(resources));
    result.expect("resources/list with policy/privacy snapshot");
    let response = response_json(&response).to_string();
    assert!(response.contains("memory://profile"), "{response}");
    assert!(!response.contains("memory://scope"), "{response}");
    assert!(
        !response.contains("memory://projection-preview"),
        "{response}"
    );
}

#[test]
fn non_loopback_mcp_bind_without_bearer_verifier_fails_before_accept() {
    let runtime = runtime();
    let error = validate_mcp_http_listener_security(
        &runtime,
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8788)),
    )
    .expect_err("wildcard MCP bind without verifier must fail closed");

    assert_eq!(error.stage(), "mcp_http_listener_auth");
}
