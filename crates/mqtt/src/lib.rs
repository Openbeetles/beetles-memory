//! MQTT adapter contracts for Beetle Memory.

use bm_adapter::AdapterOperation;

#[cfg(feature = "bridge-std")]
use bm_adapter::{
    decode_json_adapter_command, AdapterJsonCommandOptions, AdapterResponse, AdapterSdkReport,
    TransportKind, TransportMode,
};
#[cfg(feature = "bridge-std")]
use bm_entry::{EntryAuthDecision, EntryRuntime, EntryTransportContext};
#[cfg(feature = "bridge-std")]
use serde::Deserialize;
#[cfg(feature = "bridge-std")]
use serde_json::json;
#[cfg(feature = "bridge-std")]
use std::io::{Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MqttEnvelopeFields {
    RequestId,
    Source,
    IdempotencyKey,
    AuditId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MqttTopicSpec {
    pub topic: &'static str,
    pub operation: AdapterOperation,
    pub required_fields: &'static [MqttEnvelopeFields],
    pub private_raw_allowed: bool,
}

const REQUIRED_FIELDS: &[MqttEnvelopeFields] = &[
    MqttEnvelopeFields::RequestId,
    MqttEnvelopeFields::Source,
    MqttEnvelopeFields::IdempotencyKey,
    MqttEnvelopeFields::AuditId,
];

const TOPIC_SPECS: &[MqttTopicSpec] = &[
    topic("memory/write_candidate", AdapterOperation::Write),
    topic("memory/write_report", AdapterOperation::Write),
    topic("memory/profile_capability", AdapterOperation::Capabilities),
    topic("memory/projection_hint", AdapterOperation::Project),
    topic("memory/health", AdapterOperation::Inspect),
    topic("memory/lifecycle", AdapterOperation::Inspect),
];

const fn topic(topic: &'static str, operation: AdapterOperation) -> MqttTopicSpec {
    MqttTopicSpec {
        topic,
        operation,
        required_fields: REQUIRED_FIELDS,
        private_raw_allowed: false,
    }
}

pub const fn topic_specs() -> &'static [MqttTopicSpec] {
    TOPIC_SPECS
}

#[cfg(feature = "bridge-std")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MqttInboundMessage {
    pub topic: String,
    pub payload: String,
}

#[cfg(feature = "bridge-std")]
impl MqttInboundMessage {
    pub fn json(topic: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            topic: topic.into(),
            payload: payload.into(),
        }
    }
}

#[cfg(feature = "bridge-std")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MqttOutboundMessage {
    pub topic: String,
    pub payload: String,
    pub private_raw_allowed: bool,
}

#[cfg(feature = "bridge-std")]
pub struct MqttBridge {
    bridge_id: String,
}

#[cfg(feature = "bridge-std")]
impl MqttBridge {
    pub fn new(bridge_id: impl Into<String>) -> Self {
        Self {
            bridge_id: bridge_id.into(),
        }
    }

    pub fn consume(
        &self,
        runtime: &EntryRuntime,
        message: MqttInboundMessage,
    ) -> bm_sdk::Result<MqttOutboundMessage> {
        let spec = topic_specs()
            .iter()
            .find(|spec| spec.topic == message.topic)
            .copied()
            .ok_or_else(|| bm_sdk::Error::config("mqtt_bridge", "unsupported topic"))?;
        let command = decode_json_adapter_command(
            spec.operation,
            &message.payload,
            &AdapterJsonCommandOptions::new("bm-mqtt").with_default_source_chat_id("chat-1"),
        )?;
        let envelope: MqttEnvelopePayload = serde_json::from_str(&message.payload)
            .map_err(|err| bm_sdk::Error::config("mqtt_bridge_json", err.to_string()))?;
        let response = runtime.handle(
            EntryTransportContext {
                request_id: envelope.request_id,
                transport: TransportKind::Mqtt,
                mode: TransportMode::Bidirectional,
                operation: spec.operation,
                source_id: self.bridge_id.clone(),
                source_kind: "mqtt_device".to_string(),
                idempotency_key: envelope.idempotency_key,
                audit_id: envelope.audit_id,
                auth: EntryAuthDecision::authenticated("mqtt", "device"),
            },
            command,
        )?;
        Ok(MqttOutboundMessage {
            topic: publish_topic(spec.operation).to_string(),
            payload: render_response(response.adapter),
            private_raw_allowed: false,
        })
    }
}

#[cfg(feature = "bridge-std")]
pub fn run_mqtt_bridge_once(
    runtime: &EntryRuntime,
    mut stream: impl Read + Write,
    bridge_id: impl Into<String>,
) -> bm_sdk::Result<()> {
    let bridge_id = bridge_id.into();
    write_connect(&mut stream, &bridge_id)?;
    read_connack(&mut stream)?;
    write_subscribe(&mut stream, 1, "memory/write_candidate")?;
    read_suback(&mut stream, 1)?;

    let publish = read_mqtt_packet(&mut stream)?;
    if publish.packet_type != 0x30 {
        return Err(bm_sdk::Error::config(
            "mqtt_bridge_packet",
            "expected broker publish",
        ));
    }
    let (topic, payload) = parse_publish(&publish.body)?;
    let bridge = MqttBridge::new(bridge_id);
    let outbound = bridge.consume(runtime, MqttInboundMessage::json(topic, payload))?;
    write_publish(&mut stream, &outbound.topic, &outbound.payload)
}

#[cfg(feature = "bridge-std")]
struct MqttPacket {
    packet_type: u8,
    body: Vec<u8>,
}

#[cfg(feature = "bridge-std")]
fn write_connect(stream: &mut impl Write, client_id: &str) -> bm_sdk::Result<()> {
    let mut body = Vec::new();
    write_utf8(&mut body, "MQTT");
    body.push(0x04);
    body.push(0x02);
    body.extend_from_slice(&30_u16.to_be_bytes());
    write_utf8(&mut body, client_id);
    write_packet(stream, 0x10, &body)
}

#[cfg(feature = "bridge-std")]
fn read_connack(stream: &mut impl Read) -> bm_sdk::Result<()> {
    let packet = read_mqtt_packet(stream)?;
    if packet.packet_type != 0x20 || packet.body != [0x00, 0x00] {
        return Err(bm_sdk::Error::config(
            "mqtt_connack",
            "broker rejected connect",
        ));
    }
    Ok(())
}

#[cfg(feature = "bridge-std")]
fn write_subscribe(stream: &mut impl Write, packet_id: u16, topic: &str) -> bm_sdk::Result<()> {
    let mut body = Vec::new();
    body.extend_from_slice(&packet_id.to_be_bytes());
    write_utf8(&mut body, topic);
    body.push(0x00);
    write_packet(stream, 0x82, &body)
}

#[cfg(feature = "bridge-std")]
fn read_suback(stream: &mut impl Read, packet_id: u16) -> bm_sdk::Result<()> {
    let packet = read_mqtt_packet(stream)?;
    if packet.packet_type != 0x90 || packet.body.len() != 3 {
        return Err(bm_sdk::Error::config("mqtt_suback", "invalid suback"));
    }
    let returned_id = u16::from_be_bytes([packet.body[0], packet.body[1]]);
    if returned_id != packet_id || packet.body[2] == 0x80 {
        return Err(bm_sdk::Error::config(
            "mqtt_suback",
            "broker rejected subscribe",
        ));
    }
    Ok(())
}

#[cfg(feature = "bridge-std")]
fn write_publish(stream: &mut impl Write, topic: &str, payload: &str) -> bm_sdk::Result<()> {
    let mut body = Vec::new();
    write_utf8(&mut body, topic);
    body.extend_from_slice(payload.as_bytes());
    write_packet(stream, 0x30, &body)
}

#[cfg(feature = "bridge-std")]
fn parse_publish(body: &[u8]) -> bm_sdk::Result<(String, String)> {
    if body.len() < 2 {
        return Err(bm_sdk::Error::config(
            "mqtt_publish",
            "missing topic length",
        ));
    }
    let topic_len = u16::from_be_bytes([body[0], body[1]]) as usize;
    if body.len() < 2 + topic_len {
        return Err(bm_sdk::Error::config("mqtt_publish", "truncated topic"));
    }
    let topic = String::from_utf8(body[2..2 + topic_len].to_vec())
        .map_err(|err| bm_sdk::Error::config("mqtt_publish_topic", err.to_string()))?;
    let payload = String::from_utf8(body[2 + topic_len..].to_vec())
        .map_err(|err| bm_sdk::Error::config("mqtt_publish_payload", err.to_string()))?;
    Ok((topic, payload))
}

#[cfg(feature = "bridge-std")]
fn read_mqtt_packet(stream: &mut impl Read) -> bm_sdk::Result<MqttPacket> {
    let mut first = [0_u8; 1];
    stream
        .read_exact(&mut first)
        .map_err(|err| bm_sdk::Error::config("mqtt_packet_type", err.to_string()))?;
    let remaining = read_remaining_len(stream)?;
    let mut body = vec![0_u8; remaining];
    stream
        .read_exact(&mut body)
        .map_err(|err| bm_sdk::Error::config("mqtt_packet_body", err.to_string()))?;
    Ok(MqttPacket {
        packet_type: first[0] & 0xf0,
        body,
    })
}

#[cfg(feature = "bridge-std")]
fn read_remaining_len(stream: &mut impl Read) -> bm_sdk::Result<usize> {
    let mut multiplier = 1_usize;
    let mut value = 0_usize;
    loop {
        let mut byte = [0_u8; 1];
        stream
            .read_exact(&mut byte)
            .map_err(|err| bm_sdk::Error::config("mqtt_remaining_len", err.to_string()))?;
        value += ((byte[0] & 0x7f) as usize) * multiplier;
        if byte[0] & 0x80 == 0 {
            break;
        }
        multiplier *= 128;
        if multiplier > 128 * 128 * 128 {
            return Err(bm_sdk::Error::config(
                "mqtt_remaining_len",
                "remaining length exceeds MQTT limit",
            ));
        }
    }
    Ok(value)
}

#[cfg(feature = "bridge-std")]
fn write_packet(stream: &mut impl Write, packet_type: u8, body: &[u8]) -> bm_sdk::Result<()> {
    let mut packet = vec![packet_type];
    encode_remaining_len(body.len(), &mut packet);
    packet.extend_from_slice(body);
    stream
        .write_all(&packet)
        .and_then(|_| stream.flush())
        .map_err(|err| bm_sdk::Error::config("mqtt_packet_write", err.to_string()))
}

#[cfg(feature = "bridge-std")]
fn write_utf8(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u16).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
}

#[cfg(feature = "bridge-std")]
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

#[cfg(feature = "bridge-std")]
fn publish_topic(operation: AdapterOperation) -> &'static str {
    match operation {
        AdapterOperation::Write => "memory/write_report",
        AdapterOperation::Capabilities => "memory/profile_capability",
        _ => "memory/health",
    }
}

#[cfg(feature = "bridge-std")]
fn render_response(response: AdapterResponse<AdapterSdkReport>) -> String {
    match response {
        AdapterResponse::Accepted { report, .. } => match report {
            AdapterSdkReport::Write(report) => json!({
                "status": "accepted",
                "operation": report.operation,
                "accepted": report.accepted,
                "changed": report.changed,
            })
            .to_string(),
            AdapterSdkReport::Capabilities(catalog) => json!({
                "status": "accepted",
                "profile": catalog.profile.as_str(),
            })
            .to_string(),
            other => json!({"status":"accepted","report":format!("{other:?}")}).to_string(),
        },
        AdapterResponse::Rejected { reason, .. } => {
            json!({"status":"rejected","reason":reason}).to_string()
        }
        AdapterResponse::Queued { queue, .. } => {
            json!({"status":"queued","queue":queue}).to_string()
        }
        AdapterResponse::Duplicated {
            idempotency_key, ..
        } => json!({"status":"duplicated","idempotency_key":idempotency_key}).to_string(),
    }
}

#[cfg(feature = "bridge-std")]
#[derive(Deserialize)]
struct MqttEnvelopePayload {
    request_id: String,
    idempotency_key: String,
    audit_id: String,
}
