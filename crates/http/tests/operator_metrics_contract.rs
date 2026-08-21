#![cfg(feature = "server-std")]

mod support;

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
use bm_http::{handle_http_in_process_request, HttpRuntimeRequest};
use bm_sdk::{
    default_agent_subject_id, CanonicalTurnDelta, ConversationScope, MemoryCapabilityPolicy,
    MemoryPrivacyPolicy, MemoryTurnDeliveryStatus, MemoryTurnFinalizeRequest, MemoryTurnProtocol,
    MemoryTurnSource, StoreBackendConfig, TranscriptInputMessage,
};
use serde_json::Value;

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "operator-metrics-agent".to_string(),
            owner_id: "operator-owner".to_string(),
        },
        scope: EntryScope {
            channel: "operator.metrics".to_string(),
            chat_id: "operator-chat".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
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
            subject: default_agent_subject_id("operator-metrics-agent"),
            delivery_status: MemoryTurnDeliveryStatus::Delivered,
            source: turn_source(),
            actor: None,
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
    let write_body = serde_json::json!({
        "name": "operator_metrics_contract",
        "topic": "operator metrics",
        "title": "Operator metrics contract",
        "summary": "Operator API displays metrics from runtime reports.",
        "content": "- record the accepted write from the runtime event stream\n- render write and recall counters from the validated metrics report",
        "source": "manual",
        "citations": ["operator-metrics-contract"],
        "owning_scope": {
            "kind": "subject",
            "mounted_subject_id": default_agent_subject_id("operator-metrics-agent"),
        },
        "creation_ref": {
            "kind": "replay_promotion",
            "candidate_ref": "operator-metrics-contract:write",
            "verification_receipt_digest":
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        },
        "privacy_class": "shared_with_subject",
    })
    .to_string();
    let write = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/write", &write_body)
            .with_idempotency_key("operator-metrics-contract-write"),
    )
    .expect("write");
    assert_eq!(write.status_code, 200, "{}", write.body);

    let response =
        handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/console/overview"))
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
        .finalize_turn(finalize_request())
        .expect("deferred finalize");

    let response =
        handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/console/overview"))
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
