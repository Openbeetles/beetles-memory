#![cfg(feature = "bridge-rumqttc")]

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_mqtt::{MqttBridge, MqttInboundMessage};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxMemoryGateway,
        identity: EntryIdentity {
            agent_id: "mqtt-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "mqtt".to_string(),
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
fn mqtt_bridge_consumes_write_candidate_and_publishes_report() {
    let runtime = runtime();
    let bridge = MqttBridge::new("bridge-1");
    let outgoing = bridge
        .consume(
            &runtime,
            MqttInboundMessage::json(
                "memory/write_candidate",
                r#"{
                  "request_id":"mqtt-req-1",
                  "idempotency_key":"mqtt-idem-1",
                  "audit_id":"mqtt-audit-1",
                  "name":"runtime_skill__mqtt_bridge_entry",
                  "topic":"mqtt-bridge",
                  "title":"MQTT bridge entry",
                  "summary":"MQTT bridge writes procedural memory through EntryRuntime.",
                  "content":"1. Consume the MQTT write candidate topic.\n2. Validate required envelope fields.\n3. Dispatch through EntryRuntime.\n4. Publish only the write report topic."
                }"#,
            ),
        )
        .expect("consume");

    assert_eq!(outgoing.topic, "memory/write_report");
    assert!(outgoing.payload.contains("\"status\""));
    assert!(!outgoing.private_raw_allowed);
}
