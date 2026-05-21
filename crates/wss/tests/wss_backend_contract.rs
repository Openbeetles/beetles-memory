#![cfg(all(feature = "server-std", unix))]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::thread;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};
use bm_wss::{serve_wss_stream, WssBudget};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxMemoryGateway,
        identity: EntryIdentity {
            agent_id: "wss-backend-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "wss-backend".to_string(),
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
fn websocket_backend_upgrades_and_dispatches_text_frame() {
    let runtime = runtime();
    let (mut client, mut server_stream) = UnixStream::pair().expect("socket pair");

    let server = thread::spawn(move || {
        serve_wss_stream(
            &runtime,
            &mut server_stream,
            "wss-backend-session",
            WssBudget::server_gateway(),
        )
        .expect("serve websocket stream");
    });

    client
        .write_all(
            b"GET /memory/ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        )
        .expect("write handshake");

    let mut handshake = Vec::new();
    read_until(&mut client, b"\r\n\r\n", &mut handshake);
    let handshake = String::from_utf8(handshake).expect("handshake utf8");
    assert!(
        handshake.starts_with("HTTP/1.1 101 Switching Protocols"),
        "{handshake}"
    );
    assert!(
        handshake.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
        "{handshake}"
    );

    write_masked_text_frame(
        &mut client,
        r#"{"kind":"command.capabilities","payload":""}"#,
    );
    let payload = read_unmasked_text_frame(&mut client);
    server.join().expect("server thread");

    assert!(payload.contains("\"status\":\"accepted\""), "{payload}");
    assert!(payload.contains("\"profile\""), "{payload}");
}

fn read_until(stream: &mut UnixStream, needle: &[u8], out: &mut Vec<u8>) {
    let mut byte = [0_u8; 1];
    while stream.read_exact(&mut byte).is_ok() {
        out.push(byte[0]);
        if out.ends_with(needle) {
            break;
        }
    }
}

fn write_masked_text_frame(stream: &mut UnixStream, text: &str) {
    let payload = text.as_bytes();
    assert!(payload.len() < 126);
    let mask = [0x11_u8, 0x22, 0x33, 0x44];
    let mut frame = vec![0x81, 0x80 | payload.len() as u8];
    frame.extend_from_slice(&mask);
    for (idx, byte) in payload.iter().enumerate() {
        frame.push(byte ^ mask[idx % 4]);
    }
    stream.write_all(&frame).expect("write frame");
}

fn read_unmasked_text_frame(stream: &mut UnixStream) -> String {
    let mut header = [0_u8; 2];
    stream.read_exact(&mut header).expect("frame header");
    assert_eq!(header[0] & 0x0f, 0x01);
    let len = (header[1] & 0x7f) as usize;
    let mut payload = vec![0_u8; len];
    stream.read_exact(&mut payload).expect("frame payload");
    String::from_utf8(payload).expect("payload utf8")
}
