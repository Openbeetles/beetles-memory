#![cfg(feature = "server-stdio")]

use std::io::{Cursor, Read, Write};

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_mcp::{serve_mcp_streamable_http_stream, McpToolServer};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "mcp-http-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "mcp-http".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

struct MemoryStream {
    read: Cursor<Vec<u8>>,
    written: Vec<u8>,
}

impl MemoryStream {
    fn new(input: String) -> Self {
        Self {
            read: Cursor::new(input.into_bytes()),
            written: Vec::new(),
        }
    }

    fn written_string(&self) -> String {
        String::from_utf8(self.written.clone()).expect("utf8 response")
    }
}

impl Read for MemoryStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.read.read(buf)
    }
}

impl Write for MemoryStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.written.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn streamable_http_stream_serves_json_rpc_resources() {
    let runtime = runtime();
    let server = McpToolServer::new("mcp-http-stream");
    let body = r#"{"jsonrpc":"2.0","id":"r1","method":"resources/list"}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let mut stream = MemoryStream::new(request);

    serve_mcp_streamable_http_stream(&server, &runtime, &mut stream)
        .expect("serve streamable http");

    let response = stream.written_string();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(
        response.contains("content-type: application/json\r\n"),
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
    let server = McpToolServer::new("mcp-http-notification");
    let body = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let mut stream = MemoryStream::new(request);

    serve_mcp_streamable_http_stream(&server, &runtime, &mut stream)
        .expect("serve streamable http notification");

    let response = stream.written_string();
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
    let server = McpToolServer::new("mcp-http-origin");
    let body = r#"{"jsonrpc":"2.0","id":"r1","method":"resources/list"}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nOrigin: https://evil.example\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let mut stream = MemoryStream::new(request);

    serve_mcp_streamable_http_stream(&server, &runtime, &mut stream)
        .expect("serve streamable http invalid origin");

    let response = stream.written_string();
    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden\r\n"),
        "{response}"
    );
    assert!(response.contains("invalid MCP Origin"), "{response}");
}
