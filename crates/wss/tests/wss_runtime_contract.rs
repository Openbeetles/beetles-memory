#![cfg(feature = "server-std")]

mod support;

use bm_entry::{
    EntryAuthConfig, EntryBearerPrincipal, EntryIdempotencyConfig, EntryIdentity,
    EntryOperationCapability, EntryRuntime, EntryRuntimeConfig, EntryScope, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig};
use bm_wss::{WssRuntimeFrame, WssRuntimeSession};

fn runtime() -> EntryRuntime {
    runtime_with_auth(EntryAuthConfig::disabled_for_local())
}

fn runtime_with_auth(auth: EntryAuthConfig) -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    let profile = support::native_runtime_profile();
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "wss-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "wss".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(profile)
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth,
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[test]
fn wss_server_feature_enables_entry_governance_model_client() {
    assert!(bm_entry::entry_governance_model_client_compiled());
}

fn write_payload(name: &str, summary: &str) -> String {
    serde_json::json!({
        "name": name,
        "topic": "wss-idempotency",
        "title": format!("WSS write {name}"),
        "summary": summary,
        "content": "1. Decode the WSS write payload.\n2. Dispatch it through the governed EntryRuntime path and verify the receipt.",
        "owning_scope": {
            "kind": "subject",
            "mounted_subject_id": "agent:wss-agent",
        },
        "creation_ref": {
            "kind": "replay_promotion",
            "candidate_ref": format!("test:wss:{name}"),
            "verification_receipt_digest":
                "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        },
        "privacy_class": "shared_with_subject",
    })
    .to_string()
}

fn finalize_payload() -> String {
    serde_json::json!({
        "turn": {
            "turn_id": "turn-wss-pl1d",
            "conversation": {
                "channel": "wss",
                "chat_id": "chat-1",
                "conversation_id": "conversation-wss-pl1d"
            },
            "subject": "agent:wss-agent",
            "delivery_status": "delivered",
            "source": {
                "ingress": "user",
                "channel": "wss",
                "provider": null,
                "protocol": "native",
                "endpoint": "command.finalize_turn",
                "model_alias": null,
                "model_resolved": null,
                "request_id": "request-wss-pl1d",
                "client_conversation_hint": "conversation-wss-pl1d"
            },
            "input_messages": [{
                "role": "user",
                "content": "WSS queued contract",
                "authority": "user_asserted",
                "observed_at": 1,
                "speaker_id": "user",
                "speaker_kind": "human"
            }],
            "external_content_used": false
        }
    })
    .to_string()
}

#[test]
fn wss_finalize_returns_typed_queued_report() {
    let runtime = runtime();
    let mut session = WssRuntimeSession::new(
        &runtime,
        "session-finalize",
        support::trusted_auth("peer-finalize"),
    );
    let response = session
        .handle_frame(WssRuntimeFrame::command(
            "command.finalize_turn",
            finalize_payload(),
        ))
        .expect("WSS finalize");
    let value: serde_json::Value =
        serde_json::from_str(&response.payload).expect("WSS finalize response");
    assert_eq!(value["operation"], "finalize_turn");
    assert_eq!(value["result"]["memoryConsolidation"]["state"], "queued");
    assert!(value["result"]["memoryConsolidation"]["jobId"].is_string());
}

fn response_status(response: &bm_wss::WssRuntimeEvent) -> String {
    let payload: serde_json::Value =
        serde_json::from_str(&response.payload).expect("WSS response JSON");
    payload["status"]
        .as_str()
        .expect("WSS response status")
        .to_string()
}

fn assert_exact_governed_result(payload: &serde_json::Value) {
    let result = payload["result"].clone();
    let dto: bm_adapter::AdapterGovernedSafeReportV1 =
        serde_json::from_value(result.clone()).expect("strict adapter governed safe DTO");
    assert_eq!(
        serde_json::to_value(dto).expect("serialize adapter governed safe DTO"),
        result
    );
}

#[test]
fn wss_durable_write_without_caller_operation_key_fails_closed() {
    let runtime = runtime();
    let mut session = WssRuntimeSession::new(
        &runtime,
        "session-auto",
        support::trusted_auth("principal-auto"),
    );

    let first = session
        .handle_frame(WssRuntimeFrame::command(
            "command.write",
            write_payload("runtime_skill__wss_auto_first", "first automatic write"),
        ))
        .expect("first write");
    let second = session
        .handle_frame(WssRuntimeFrame::command(
            "command.write",
            write_payload("runtime_skill__wss_auto_second", "second automatic write"),
        ))
        .expect("second write");

    assert_eq!(response_status(&first), "rejected", "{}", first.payload);
    assert_eq!(response_status(&second), "rejected", "{}", second.payload);
    assert!(
        first.payload.contains("mutation_operation_id"),
        "{}",
        first.payload
    );
    assert!(
        second.payload.contains("mutation_operation_id"),
        "{}",
        second.payload
    );
}

#[test]
fn wss_explicit_identity_replays_same_payload_and_rejects_conflict() {
    let runtime = runtime();
    let mut initial_session = WssRuntimeSession::new(
        &runtime,
        "session-explicit",
        support::trusted_auth("principal-explicit"),
    );
    let mut retry_session = WssRuntimeSession::new(
        &runtime,
        "session-explicit-retry",
        support::trusted_auth("principal-explicit"),
    );
    let payload = write_payload("runtime_skill__wss_explicit", "stable payload");

    let first = initial_session
        .handle_frame(
            WssRuntimeFrame::command("command.write", payload.clone())
                .with_idempotency_key("wss-caller-key"),
        )
        .expect("first write");
    let replay = retry_session
        .handle_frame(
            WssRuntimeFrame::command("command.write", payload)
                .with_idempotency_key("wss-caller-key"),
        )
        .expect("replay write");
    let conflict = retry_session
        .handle_frame(
            WssRuntimeFrame::command(
                "command.write",
                write_payload("runtime_skill__wss_conflict", "different payload"),
            )
            .with_idempotency_key("wss-caller-key"),
        )
        .expect("conflicting write");

    assert_eq!(response_status(&first), "accepted", "{}", first.payload);
    assert_eq!(response_status(&replay), "replayed", "{}", replay.payload);
    assert_eq!(
        response_status(&conflict),
        "rejected",
        "{}",
        conflict.payload
    );
    let first_payload: serde_json::Value =
        serde_json::from_str(&first.payload).expect("first WSS response JSON");
    let replay_payload: serde_json::Value =
        serde_json::from_str(&replay.payload).expect("replay WSS response JSON");
    assert!(first_payload["receipt"].is_object(), "{first_payload}");
    assert_eq!(
        first_payload["receipt"]["transaction_id"],
        replay_payload["receipt"]["transaction_id"]
    );
}

#[test]
fn wss_explicit_identity_isolated_by_authenticated_principal() {
    let runtime = runtime();
    let mut principal_a = WssRuntimeSession::new(
        &runtime,
        "shared-session",
        support::trusted_auth("principal-a"),
    );
    let mut principal_b = WssRuntimeSession::new(
        &runtime,
        "shared-session",
        support::trusted_auth("principal-b"),
    );

    let first = principal_a
        .handle_frame(
            WssRuntimeFrame::command(
                "command.write",
                write_payload("runtime_skill__wss_principal_a", "principal A"),
            )
            .with_idempotency_key("shared-caller-key"),
        )
        .expect("principal A write");
    let second = principal_b
        .handle_frame(
            WssRuntimeFrame::command(
                "command.write",
                write_payload("runtime_skill__wss_principal_b", "principal B"),
            )
            .with_idempotency_key("shared-caller-key"),
        )
        .expect("principal B write");

    assert_eq!(response_status(&first), "accepted");
    assert_eq!(response_status(&second), "accepted");
}

#[test]
fn wss_session_dispatches_command_frame_through_entry_runtime() {
    let runtime = runtime();
    let mut session =
        WssRuntimeSession::new(&runtime, "session-1", support::trusted_auth("peer-1"));
    let response = session
        .handle_frame(WssRuntimeFrame::command(
            "command.recall",
            r#"{"temporal_operation":{"kind":"current"},"query":"release","limit":2}"#,
        ))
        .expect("wss frame");

    assert_eq!(response.kind, "event.report");
    assert!(response.payload.contains("\"status\""));
    assert!(!response.private_raw_allowed);
    assert!(response.budget_report_id.starts_with("rtb-v2-"));
    let payload: serde_json::Value = serde_json::from_str(&response.payload).expect("event JSON");
    assert_eq!(
        payload["runtime_budget_report_id"],
        response.budget_report_id
    );
    assert_exact_governed_result(&payload);
}

#[test]
fn wss_subscription_limit_comes_from_bound_runtime_report() {
    let runtime = runtime();
    let max_subscriptions = runtime
        .runtime_budget()
        .adapter_budget
        .wss_max_subscriptions;
    let mut session =
        WssRuntimeSession::new(&runtime, "session-1", support::trusted_auth("peer-1"));

    for index in 0..max_subscriptions {
        let event = session
            .handle_frame(WssRuntimeFrame::subscribe("subscribe.projection"))
            .unwrap_or_else(|error| panic!("subscription {index} failed: {error}"));
        assert_eq!(event.kind, "event.lifecycle");
        assert!(!event.private_raw_allowed);
    }

    let rejected = session
        .handle_frame(WssRuntimeFrame::subscribe("subscribe.inspection"))
        .expect("over-budget subscription");
    assert_eq!(rejected.kind, "event.error");
    assert!(rejected.payload.contains("subscription_budget_exceeded"));
    assert!(!rejected.private_raw_allowed);
}

#[test]
fn wss_subscription_requires_authenticated_subscribe_capability() {
    let limited_config = EntryAuthConfig::required_bearer_principal(
        "secret-token",
        EntryBearerPrincipal::new(
            "limited-peer",
            "owner-default",
            [EntryOperationCapability::Recall],
        ),
    );
    let (unauthenticated, response) = support::serve_network_frame(
        runtime_with_auth(limited_config.clone()),
        None,
        r#"{"kind":"subscribe.projection","payload":""}"#,
    );
    let error = unauthenticated.expect_err("missing bearer must fail during WSS handshake");
    assert_eq!(error.stage(), "wss_handshake_auth");
    assert!(error.to_string().contains("missing_bearer_token"));
    assert!(response.is_empty());

    let (limited, response) = support::serve_network_frame(
        runtime_with_auth(limited_config),
        Some("Bearer secret-token"),
        r#"{"kind":"subscribe.projection","payload":""}"#,
    );
    limited.expect("limited bearer handshake and structured event");
    assert!(response.contains("\"kind\":\"event.error\""), "{response}");
    assert!(
        response.contains("subscription_not_authorized"),
        "{response}"
    );
}

#[test]
fn wss_network_session_processes_multiple_frames_ping_and_close_under_one_lease() {
    let (result, responses) = support::serve_network_sequence(
        runtime_with_auth(EntryAuthConfig::required_bearer_principal(
            "secret-token",
            EntryBearerPrincipal::new(
                "sequence-peer",
                "owner-default",
                [
                    EntryOperationCapability::Capabilities,
                    EntryOperationCapability::Recall,
                ],
            ),
        )),
        "Bearer secret-token",
        &[
            r#"{"kind":"command.capabilities","payload":"{}"}"#,
            r#"{"kind":"command.recall","payload":"{\"temporal_operation\":{\"kind\":\"current\"},\"query\":\"sequence\",\"limit\":2}"}"#,
        ],
    );

    result.expect("bounded persistent WSS session");
    assert_eq!(responses.len(), 2);
    assert!(responses[0].contains("event.report"), "{}", responses[0]);
    assert!(
        responses[0].contains("mutation_operation_inventory"),
        "{}",
        responses[0]
    );
    assert!(
        responses[0].contains("mutation_receipt_policy"),
        "{}",
        responses[0]
    );
    assert!(!responses[0].contains("\"receipt\""), "{}", responses[0]);
    assert!(responses[1].contains("event.report"), "{}", responses[1]);
}

#[test]
fn wss_frame_limit_comes_from_bound_runtime_report() {
    let runtime = runtime();
    let max_frame_bytes = runtime.runtime_budget().adapter_budget.wss_frame_max_bytes;
    let mut session = WssRuntimeSession::new(
        &runtime,
        "session-frame-budget",
        support::trusted_auth("peer-frame-budget"),
    );

    let rejected = session
        .handle_frame(WssRuntimeFrame::command(
            "command.capabilities",
            "x".repeat(max_frame_bytes + 1),
        ))
        .expect("over-budget frame");

    assert_eq!(rejected.kind, "event.error");
    assert!(rejected.payload.contains("frame_budget_exceeded"));
}

#[test]
fn wss_frame_accepts_payload_at_exact_pinned_budget() {
    let runtime = runtime();
    let max_frame_bytes = runtime.runtime_budget().adapter_budget.wss_frame_max_bytes;
    let prefix = r#"{"padding":""#;
    let suffix = r#""}"#;
    assert!(prefix.len() + suffix.len() <= max_frame_bytes);
    let payload = format!(
        "{prefix}{}{suffix}",
        "x".repeat(max_frame_bytes - prefix.len() - suffix.len())
    );
    let mut session = WssRuntimeSession::new(
        &runtime,
        "session-exact-frame-budget",
        support::trusted_auth("peer-exact-frame-budget"),
    );

    let accepted = session
        .handle_frame(WssRuntimeFrame::command("command.capabilities", payload))
        .expect("exact boundary frame");

    assert_eq!(accepted.kind, "event.report");
    assert!(accepted.budget_report_id.starts_with("rtb-v2-"));
}

#[test]
fn wss_runtime_decodes_declared_command_operations() {
    let runtime = runtime();
    let mut session =
        WssRuntimeSession::new(&runtime, "session-ops", support::trusted_auth("peer-ops"));
    let frames = [
        (
            "command.write",
            r#"{"name":"runtime_skill__wss_write","topic":"wss","title":"WSS write","summary":"WSS write summary","content":"1. Decode WSS write.\n2. Dispatch through EntryRuntime.","owning_scope":{"kind":"subject","mounted_subject_id":"agent:wss-agent"},"creation_ref":{"kind":"replay_promotion","candidate_ref":"test:wss:runtime_skill__wss_write","verification_receipt_digest":"sha256:2222222222222222222222222222222222222222222222222222222222222222"},"privacy_class":"shared_with_subject"}"#,
        ),
        (
            "command.project",
            r#"{"temporal_operation":{"kind":"current"},"user_query":"release","system_max_len":1024}"#,
        ),
        (
            "command.inspect",
            r#"{"query":"release","system_max_len":1024}"#,
        ),
        ("command.long_term.list", r#"{"query":{},"limit":2}"#),
        ("command.capabilities", r#"{}"#),
    ];

    for (kind, payload) in frames {
        let response = session
            .handle_frame(WssRuntimeFrame::command(kind, payload))
            .unwrap_or_else(|err| panic!("{kind} failed: {err}"));
        assert_eq!(response.kind, "event.report");
        assert!(
            response.payload.contains("\"status\""),
            "{kind}: {}",
            response.payload
        );
        assert!(!response.private_raw_allowed);
    }
}
