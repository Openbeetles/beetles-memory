#![cfg(feature = "server-std")]

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{handle_http_request, HttpRuntimeRequest};
use bm_sdk::{
    CanonicalTurnDelta, ConversationScope, MemoryCapabilityPolicy, MemoryPrivacyPolicy,
    MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource,
    ProfileId, StoreBackendKind, TranscriptInputMessage,
};
use serde_json::Value;

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "operator-metrics-agent".to_string(),
            owner_id: "operator-owner".to_string(),
        },
        scope: EntryScope {
            channel: "operator.metrics".to_string(),
            chat_id: "operator-chat".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_disabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

fn turn_source() -> MemoryTurnSource {
    MemoryTurnSource {
        ingress: bm_sdk::IngressKind::User,
        channel: "operator.metrics".to_string(),
        provider: Some("sdk".to_string()),
        protocol: MemoryTurnProtocol::Native,
        endpoint: None,
        model_alias: None,
        model_resolved: None,
        request_id: Some("operator-req-1".to_string()),
        client_conversation_hint: Some("operator-conversation".to_string()),
    }
}

fn finalize_request() -> MemoryTurnFinalizeRequest {
    MemoryTurnFinalizeRequest {
        turn: CanonicalTurnDelta {
            turn_id: "operator-deferred-turn".to_string(),
            conversation: ConversationScope {
                channel: "operator.metrics".to_string(),
                chat_id: "operator-chat".to_string(),
                conversation_id: Some("operator-conversation".to_string()),
            },
            subject: "operator-owner".to_string(),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            input_messages: vec![TranscriptInputMessage::user("记住 operator queue 必须可见")],
            assistant_message: Some(TranscriptInputMessage::assistant("已记录。")),
            tool_observations: Vec::new(),
            external_content_used: false,
            candidate_ids: Vec::new(),
        },
        tool_calls: 0,
        runtime_skill_selected_ids: Vec::new(),
        task_learning_selected_ids: Vec::new(),
        reuse_outcome_note: String::new(),
        tool_usage_feedback: None,
        pressure: bm_sdk::PressureLevel::Normal,
        mode_input: bm_sdk::RuntimeLifecycleModeInput::default(),
    }
}

#[test]
fn operator_overview_exposes_stable_runtime_metrics_fields() {
    let runtime = runtime();
    let write = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/write",
            r#"{"name":"operator_metrics_contract","topic":"operator metrics","title":"Operator metrics contract","summary":"Operator API displays metrics from runtime reports.","content":"Write count and hit count are runtime metrics fields.","source":"manual"}"#,
        ),
    )
    .expect("write");
    assert_eq!(write.status_code, 200, "{}", write.body);

    let response = handle_http_request(&runtime, HttpRuntimeRequest::get("/console/overview"))
        .expect("overview");
    assert_eq!(response.status_code, 200, "{}", response.body);
    let body: Value = serde_json::from_str(&response.body).expect("json");
    let overview = &body["overview"];

    assert!(overview["storage"]["value"].as_str().is_some());
    assert!(overview["writesToday"]["value"]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .is_some());
    assert!(overview["writesToday"]["desc"]
        .as_str()
        .is_some_and(|desc| desc.contains("runtime event stream")));
    assert!(overview["recall"]["desc"]
        .as_str()
        .is_some_and(|desc| desc.contains("recall requests")));
    assert!(overview["runtimeBudget"]["storeSnapshotMaxBytes"]
        .as_u64()
        .is_some_and(|value| value > 0));
    assert!(overview["runtimeBudget"]["projectionRenderMaxChars"]
        .as_u64()
        .is_some_and(|value| value > 0));
}

#[test]
fn operator_overview_exposes_deferred_governance_queue_from_sdk_report() {
    let runtime = runtime();
    runtime
        .runtime()
        .finalize_turn_and_maintain(None, None, finalize_request())
        .expect("deferred finalize");

    let response = handle_http_request(&runtime, HttpRuntimeRequest::get("/console/overview"))
        .expect("overview");
    assert_eq!(response.status_code, 200, "{}", response.body);
    let body: Value = serde_json::from_str(&response.body).expect("json");
    let queue = &body["overview"]["deferredGovernance"];

    assert_eq!(queue["pending"], 1);
    assert_eq!(queue["retrying"], 0);
    assert_eq!(queue["failed"], 0);
    assert_eq!(queue["terminal"], 0);
    assert_eq!(
        queue["recentJobs"][0]["reason"],
        "maintenance_http_unavailable"
    );
    assert_eq!(queue["recentJobs"][0]["turnId"], "operator-deferred-turn");
}
