#![cfg(all(feature = "server-std", unix))]

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use bm_entry::{
    EntryAcceptedTcpStream, EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime,
    EntryRuntimeConfig, EntryScope, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig};
use bm_wss::serve_wss_accepted_stream;

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let profile = support::native_runtime_profile();
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "wss-backend-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "wss-backend".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(profile)
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

#[test]
fn websocket_backend_upgrades_and_dispatches_text_frame() {
    let runtime = runtime();
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let mut client = TcpStream::connect(listener.local_addr().expect("listener addr"))
        .expect("client connection");
    let mut server_stream = EntryAcceptedTcpStream::accept(&listener).expect("accepted connection");

    let server = thread::spawn(move || {
        serve_wss_accepted_stream(&runtime, &mut server_stream, "wss-backend-session")
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
    assert!(
        handshake.contains("x-bm-runtime-budget-report-id: rtb-v2-"),
        "{handshake}"
    );
    let handshake_budget_report_id = handshake
        .lines()
        .find_map(|line| line.strip_prefix("x-bm-runtime-budget-report-id: "))
        .expect("handshake budget report id")
        .trim()
        .to_string();

    write_masked_text_frame(
        &mut client,
        r#"{"kind":"command.capabilities","payload":""}"#,
    );
    let payload = read_unmasked_text_frame(&mut client);
    write_masked_close_frame(&mut client);
    read_unmasked_close_frame(&mut client);
    server.join().expect("server thread");

    let payload: serde_json::Value = serde_json::from_str(&payload).expect("event JSON");
    assert_eq!(payload["kind"], "event.report");
    let report: serde_json::Value = serde_json::from_str(
        payload["payload"]
            .as_str()
            .expect("typed WSS event payload"),
    )
    .expect("WSS report JSON");
    assert_eq!(report["status"], "accepted");
    assert!(report.get("profile").is_some(), "{report}");
    assert_eq!(payload["budget_report_id"], handshake_budget_report_id);
}

fn read_until(stream: &mut TcpStream, needle: &[u8], out: &mut Vec<u8>) {
    let mut byte = [0_u8; 1];
    while stream.read_exact(&mut byte).is_ok() {
        out.push(byte[0]);
        if out.ends_with(needle) {
            break;
        }
    }
}

fn write_masked_text_frame(stream: &mut TcpStream, text: &str) {
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

fn write_masked_close_frame(stream: &mut TcpStream) {
    stream
        .write_all(&[0x88, 0x80, 0x11, 0x22, 0x33, 0x44])
        .expect("write masked close frame");
}

fn read_unmasked_close_frame(stream: &mut TcpStream) {
    let mut frame = [0_u8; 2];
    stream.read_exact(&mut frame).expect("read close response");
    assert_eq!(frame, [0x88, 0x00]);
}

fn read_unmasked_text_frame(stream: &mut TcpStream) -> String {
    let mut header = [0_u8; 2];
    stream.read_exact(&mut header).expect("frame header");
    assert_eq!(header[0] & 0x0f, 0x01);
    let len = match header[1] & 0x7f {
        126 => {
            let mut extended = [0_u8; 2];
            stream.read_exact(&mut extended).expect("extended length");
            u16::from_be_bytes(extended) as usize
        }
        127 => panic!("test server must not emit 64-bit frame lengths"),
        len => len as usize,
    };
    let mut payload = vec![0_u8; len];
    stream.read_exact(&mut payload).expect("frame payload");
    String::from_utf8(payload).expect("payload utf8")
}
