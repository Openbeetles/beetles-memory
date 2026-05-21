#![cfg(all(feature = "server-std", unix))]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::thread;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::serve_http_stream;
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxMemoryGateway,
        identity: EntryIdentity {
            agent_id: "http-backend-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "http-backend".to_string(),
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
fn std_http_stream_serves_profile_capabilities_through_entry_runtime() {
    let runtime = runtime();
    let (mut client, mut server_stream) = UnixStream::pair().expect("socket pair");

    let server = thread::spawn(move || {
        serve_http_stream(&runtime, &mut server_stream).expect("serve one request");
    });

    client
        .write_all(
            b"GET /memory/profile/capabilities HTTP/1.1\r\nHost: localhost\r\nx-request-id: req-http-backend\r\nx-idempotency-key: idem-http-backend\r\nx-audit-id: audit-http-backend\r\n\r\n",
        )
        .expect("write request");
    client
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown");

    let mut response = String::new();
    client.read_to_string(&mut response).expect("read response");
    server.join().expect("server thread");

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains("\"profile\""), "{response}");
    assert!(response.contains("\"entry\""), "{response}");
}
