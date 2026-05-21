use bm_a2a::{A2aBridge, A2aRuntimeMessage};
use bm_entry::{
    EntryAuthConfig, EntryIdentity, EntryIdempotencyConfig, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{handle_http_request, HttpRuntimeRequest};
use bm_mcp::{McpToolCall, McpToolServer};
use bm_mqtt::{MqttBridge, MqttInboundMessage};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};
use bm_wss::{WssRuntimeFrame, WssRuntimeSession};

fn main() -> bm_sdk::Result<()> {
    let runtime = runtime()?;

    let http = handle_http_request(
        &runtime,
        HttpRuntimeRequest::get("/memory/profile/capabilities"),
    )?;
    assert_eq!(http.status_code, 200);

    let mut wss = WssRuntimeSession::new("gateway-wss", bm_wss::WssBudget::server_gateway());
    let wss_event = wss.handle_frame(
        &runtime,
        WssRuntimeFrame::command("command.recall", r#"{"query":"gateway","limit":2}"#),
    )?;
    assert_eq!(wss_event.kind, "event.report");

    let mqtt = MqttBridge::new("gateway-mqtt").consume(
        &runtime,
        MqttInboundMessage::json(
            "memory/write_candidate",
            r#"{
              "request_id":"gateway-mqtt-req",
              "idempotency_key":"gateway-mqtt-idem",
              "audit_id":"gateway-mqtt-audit",
              "name":"runtime_skill__gateway_entry",
              "topic":"gateway",
              "title":"Gateway entry",
              "summary":"Memory gateway accepts MQTT candidates through EntryRuntime.",
              "content":"1. Consume gateway topic.\n2. Normalize envelope fields.\n3. Dispatch through EntryRuntime.\n4. Publish memory report."
            }"#,
        ),
    )?;
    assert_eq!(mqtt.topic, "memory/write_report");

    let mcp = McpToolServer::new("gateway-mcp").call(
        &runtime,
        McpToolCall::json("memory_recall", r#"{"query":"gateway","limit":2}"#),
    )?;
    assert_eq!(mcp.status, "accepted");

    let a2a = A2aBridge::new("gateway-a2a").handle(
        &runtime,
        A2aRuntimeMessage::json("memory_recall_request", r#"{"query":"gateway","limit":2}"#),
    )?;
    assert_eq!(a2a.kind, "memory_report");

    println!("memory-gateway entry smoke passed");
    Ok(())
}

fn runtime() -> bm_sdk::Result<EntryRuntime> {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxMemoryGateway,
        identity: EntryIdentity {
            agent_id: "gateway-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "gateway".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 128 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
}
