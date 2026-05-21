//! MQTT adapter contracts for Beetle Memory.

use bm_adapter::AdapterOperation;

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
