#![cfg(all(feature = "bridge-http", unix))]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::thread;

use bm_a2a::{serve_a2a_http_stream, A2aBridge};
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxMemoryGateway,
        identity: EntryIdentity {
            agent_id: "a2a-http-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "a2a-http".to_string(),
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

#[test]
fn a2a_http_bridge_serves_memory_message_without_executor_permissions() {
    let runtime = runtime();
    let bridge = A2aBridge::new("a2a-http");
    let (mut client, mut server_stream) = UnixStream::pair().expect("socket pair");

    let server = thread::spawn(move || {
        serve_a2a_http_stream(&runtime, &bridge, &mut server_stream).expect("serve a2a http");
    });

    let body =
        r#"{"name":"memory_recall_request","payload":{"query":"deployment runtime","limit":2}}"#;
    let request = format!(
        "POST /a2a/message HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    client.write_all(request.as_bytes()).expect("write request");
    client
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown write");

    let mut response = String::new();
    client.read_to_string(&mut response).expect("read response");
    server.join().expect("server thread");

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains(r#""kind":"memory_report""#), "{response}");
    assert!(response.contains(r#""MemoryReport""#), "{response}");
    assert!(!response.contains("Executor"), "{response}");
    assert!(!response.contains("Workflow"), "{response}");
}
