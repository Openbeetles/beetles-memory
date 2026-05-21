use bm_adapter::AdapterOperation;
use bm_mqtt::{topic_specs, MqttEnvelopeFields};
use bm_sdk::{resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId};

#[test]
fn topics_require_source_idempotency_and_audit_fields() {
    for topic in topic_specs() {
        assert!(topic
            .required_fields
            .contains(&MqttEnvelopeFields::RequestId));
        assert!(topic.required_fields.contains(&MqttEnvelopeFields::Source));
        assert!(topic
            .required_fields
            .contains(&MqttEnvelopeFields::IdempotencyKey));
        assert!(topic.required_fields.contains(&MqttEnvelopeFields::AuditId));
    }
}

#[test]
fn mqtt_does_not_carry_deep_replay_or_archive_import_export() {
    for topic in topic_specs() {
        assert_ne!(topic.operation, AdapterOperation::Replay);
        assert_ne!(topic.operation, AdapterOperation::Export);
        assert_ne!(topic.operation, AdapterOperation::Import);
        assert!(!topic.private_raw_allowed);
    }
}

#[test]
fn esp_embedded_sdk_does_not_surface_mqtt_by_default() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    policy.adapter.mqtt_enabled = true;
    let catalog = resolve_memory_capabilities(
        ProfileId::EspEmbeddedSdk,
        &policy,
        &MemoryPrivacyPolicy::standard_private_boundary(),
    )
    .expect("catalog");

    assert!(!catalog.adapter.mqtt.visible);
}
