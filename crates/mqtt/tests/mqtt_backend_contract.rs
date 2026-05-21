#![cfg(all(feature = "bridge-std", unix))]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::thread;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_mqtt::run_mqtt_bridge_once;
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxMemoryGateway,
        identity: EntryIdentity {
            agent_id: "mqtt-backend-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "mqtt-backend".to_string(),
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
fn mqtt_bridge_connects_to_external_broker_and_publishes_write_report() {
    let runtime = runtime();
    let (client_stream, mut broker_stream) = UnixStream::pair().expect("socket pair");

    let broker = thread::spawn(move || {
        let connect = read_packet(&mut broker_stream);
        assert_eq!(connect.packet_type, 0x10, "CONNECT packet");
        broker_stream
            .write_all(&[0x20, 0x02, 0x00, 0x00])
            .expect("connack");

        let subscribe = read_packet(&mut broker_stream);
        assert_eq!(subscribe.packet_type, 0x80, "SUBSCRIBE packet");
        assert!(String::from_utf8_lossy(&subscribe.body).contains("memory/write_candidate"));
        let packet_id = [subscribe.body[0], subscribe.body[1]];
        broker_stream
            .write_all(&[0x90, 0x03, packet_id[0], packet_id[1], 0x00])
            .expect("suback");

        let payload = r#"{
          "request_id":"mqtt-req-1",
          "idempotency_key":"mqtt-idem-1",
          "audit_id":"mqtt-audit-1",
          "name":"runtime_skill__mqtt_backend",
          "topic":"mqtt-backend",
          "title":"MQTT backend",
          "summary":"MQTT backend writes procedural memory through EntryRuntime.",
          "content":"1. Connect to an external broker.\n2. Subscribe for write candidates.\n3. Dispatch through EntryRuntime.\n4. Publish write reports."
        }"#;
        broker_stream
            .write_all(&publish_packet("memory/write_candidate", payload))
            .expect("broker publish");

        let report = read_packet(&mut broker_stream);
        assert_eq!(report.packet_type, 0x30, "PUBLISH report packet");
        let (topic, body) = parse_publish(&report.body);
        assert_eq!(topic, "memory/write_report");
        assert!(body.contains("\"status\":\"accepted\""), "{body}");
        assert!(body.contains("write.procedural"), "{body}");
    });

    run_mqtt_bridge_once(&runtime, client_stream, "mqtt-backend").expect("bridge once");
    broker.join().expect("broker thread");
}

struct Packet {
    packet_type: u8,
    body: Vec<u8>,
}

fn read_packet(stream: &mut UnixStream) -> Packet {
    let mut first = [0_u8; 1];
    stream.read_exact(&mut first).expect("packet type");
    let mut multiplier = 1_usize;
    let mut remaining = 0_usize;
    loop {
        let mut byte = [0_u8; 1];
        stream.read_exact(&mut byte).expect("remaining len");
        remaining += ((byte[0] & 0x7f) as usize) * multiplier;
        if byte[0] & 0x80 == 0 {
            break;
        }
        multiplier *= 128;
    }
    let mut body = vec![0_u8; remaining];
    stream.read_exact(&mut body).expect("packet body");
    Packet {
        packet_type: first[0] & 0xf0,
        body,
    }
}

fn publish_packet(topic: &str, payload: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&(topic.len() as u16).to_be_bytes());
    body.extend_from_slice(topic.as_bytes());
    body.extend_from_slice(payload.as_bytes());
    let mut packet = vec![0x30];
    encode_remaining_len(body.len(), &mut packet);
    packet.extend_from_slice(&body);
    packet
}

fn parse_publish(body: &[u8]) -> (String, String) {
    let topic_len = u16::from_be_bytes([body[0], body[1]]) as usize;
    let topic = String::from_utf8(body[2..2 + topic_len].to_vec()).expect("topic utf8");
    let payload = String::from_utf8(body[2 + topic_len..].to_vec()).expect("payload utf8");
    (topic, payload)
}

fn encode_remaining_len(mut len: usize, out: &mut Vec<u8>) {
    loop {
        let mut byte = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if len == 0 {
            break;
        }
    }
}
