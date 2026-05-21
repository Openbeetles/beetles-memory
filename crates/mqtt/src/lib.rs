//! MQTT adapter contracts for Beetle Memory.

use bm_adapter::AdapterOperation;

#[cfg(feature = "bridge-rumqttc")]
use bm_adapter::{AdapterCommand, AdapterResponse, AdapterSdkReport, TransportKind, TransportMode};
#[cfg(feature = "bridge-rumqttc")]
use bm_entry::{EntryAuthDecision, EntryRuntime, EntryTransportContext};
#[cfg(feature = "bridge-rumqttc")]
use bm_sdk::{MemoryWriteRequest, RuntimeSkillWrite, RuntimeSkillWriteSource};
#[cfg(feature = "bridge-rumqttc")]
use serde::Deserialize;
#[cfg(feature = "bridge-rumqttc")]
use serde_json::json;

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

#[cfg(feature = "bridge-rumqttc")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MqttInboundMessage {
    pub topic: String,
    pub payload: String,
}

#[cfg(feature = "bridge-rumqttc")]
impl MqttInboundMessage {
    pub fn json(topic: impl Into<String>, payload: impl Into<String>) -> Self {
        Self {
            topic: topic.into(),
            payload: payload.into(),
        }
    }
}

#[cfg(feature = "bridge-rumqttc")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MqttOutboundMessage {
    pub topic: String,
    pub payload: String,
    pub private_raw_allowed: bool,
}

#[cfg(feature = "bridge-rumqttc")]
pub struct MqttBridge {
    bridge_id: String,
}

#[cfg(feature = "bridge-rumqttc")]
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
        let command = decode_command(spec.operation, &message.payload)?;
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

#[cfg(feature = "bridge-rumqttc")]
fn decode_command(operation: AdapterOperation, payload: &str) -> bm_sdk::Result<AdapterCommand> {
    match operation {
        AdapterOperation::Write => {
            let payload: WriteCandidatePayload = serde_json::from_str(payload)
                .map_err(|err| bm_sdk::Error::config("mqtt_bridge_json", err.to_string()))?;
            Ok(AdapterCommand::Write(MemoryWriteRequest::Procedural {
                writes: vec![RuntimeSkillWrite {
                    name: payload.name,
                    topic: payload.topic,
                    title: payload.title,
                    summary: payload.summary,
                    content: payload.content,
                    citations: vec!["bm-mqtt".to_string()],
                    source_chat_id: Some("chat-1".to_string()),
                    observed_at: 1_800_000_000,
                }],
                source: RuntimeSkillWriteSource::Manual,
            }))
        }
        AdapterOperation::Capabilities => Ok(AdapterCommand::Capabilities),
        other => Err(bm_sdk::Error::config(
            "mqtt_bridge",
            format!("unsupported MQTT bridge operation: {other:?}"),
        )),
    }
}

#[cfg(feature = "bridge-rumqttc")]
fn publish_topic(operation: AdapterOperation) -> &'static str {
    match operation {
        AdapterOperation::Write => "memory/write_report",
        AdapterOperation::Capabilities => "memory/profile_capability",
        _ => "memory/health",
    }
}

#[cfg(feature = "bridge-rumqttc")]
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

#[cfg(feature = "bridge-rumqttc")]
#[derive(Deserialize)]
struct MqttEnvelopePayload {
    request_id: String,
    idempotency_key: String,
    audit_id: String,
}

#[cfg(feature = "bridge-rumqttc")]
#[derive(Deserialize)]
struct WriteCandidatePayload {
    name: String,
    topic: String,
    title: String,
    summary: String,
    content: String,
}
