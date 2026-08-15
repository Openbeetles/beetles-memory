use std::sync::Arc;

use bm_adapter::{
    dispatch_adapter_command, AdapterAuthContext, AdapterCommand, AdapterEnvelope, AdapterErrorKey,
    AdapterProtocolBinding, AdapterResponse, AdapterSdkReport, AdapterSource,
    ExternalAiMemoryProtocolVersion, TransportKind, TransportMode,
};
use bm_sdk::{
    CanonicalTurnDelta, ConversationScope, MemoryCapabilityPolicy, MemoryClock, MemoryIdentity,
    MemoryPrivacyPolicy, MemoryRuntime, MemoryScope, MemoryStoreHandle, MemoryTurnDeliveryStatus,
    MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource, NoopMemoryAuditSink,
    PressureLevel, ProfileId, RuntimeLifecycleModeInput, StoreBackendConfig,
    TranscriptInputMessage,
};

struct FixedClock;

impl MemoryClock for FixedClock {
    fn now_secs(&self) -> u64 {
        1_900_000_000
    }
}

fn host_profile() -> ProfileId {
    #[cfg(target_os = "macos")]
    {
        ProfileId::DesktopMacosStandaloneMemory
    }
    #[cfg(target_os = "windows")]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(target_os = "linux")]
    {
        ProfileId::DesktopLinuxEmbeddedSdk
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        ProfileId::EspEmbeddedSdk
    }
}

fn runtime() -> MemoryRuntime {
    let profile = host_profile();
    let store = MemoryStoreHandle::open_in_memory(
        StoreBackendConfig::in_memory(profile).expect("store config"),
    )
    .expect("store");
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .store(store)
        .clock(Arc::new(FixedClock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .expect("runtime")
}

fn turn(turn_id: &str) -> CanonicalTurnDelta {
    CanonicalTurnDelta {
        turn_id: turn_id.to_string(),
        conversation: ConversationScope {
            channel: "local".to_string(),
            chat_id: "chat-1".to_string(),
            conversation_id: Some("conversation-eap1".to_string()),
        },
        subject: bm_sdk::default_agent_subject_id("agent-main"),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: MemoryTurnSource {
            ingress: bm_sdk::IngressKind::User,
            channel: "local".to_string(),
            provider: None,
            protocol: MemoryTurnProtocol::Native,
            endpoint: None,
            model_alias: None,
            model_resolved: None,
            request_id: Some(format!("request-{turn_id}")),
            client_conversation_hint: Some("conversation-eap1".to_string()),
        },
        actor: None,
        input_messages: vec![TranscriptInputMessage::user("synthetic protocol user")],
        assistant_message: Some(TranscriptInputMessage::assistant(
            "synthetic protocol assistant",
        )),
        tool_observations: Vec::new(),
        external_content_used: false,
        candidate_ids: Vec::new(),
    }
}

fn finalize(turn_id: &str) -> AdapterCommand {
    AdapterCommand::FinalizeTurn(Box::new(MemoryTurnFinalizeRequest {
        turn: turn(turn_id),
        tool_calls: 0,
        runtime_skill_selected_ids: Vec::new(),
        task_learning_selected_ids: Vec::new(),
        reuse_outcome_note: String::new(),
        tool_usage_feedback: None,
        pressure: PressureLevel::Normal,
        mode_input: RuntimeLifecycleModeInput::default(),
    }))
}

fn envelope(runtime: &MemoryRuntime, payload: AdapterCommand) -> AdapterEnvelope<AdapterCommand> {
    let lease = runtime
        .acquire_runtime_budget_lease()
        .expect("budget lease");
    let operation = payload.operation();
    AdapterEnvelope {
        protocol_version: ExternalAiMemoryProtocolVersion::V1,
        runtime_binding: AdapterProtocolBinding::for_runtime(runtime, &lease),
        request_id: "request-eap1".to_string(),
        transport: TransportKind::Http,
        mode: TransportMode::Server,
        operation,
        source: AdapterSource {
            source_id: "source-eap1".to_string(),
            source_kind: "synthetic_http_client".to_string(),
            agent_id: runtime.identity().agent_id.clone(),
            owner_id: runtime.identity().owner_id.clone(),
            channel: runtime.scope().channel.clone(),
            chat_id: runtime.scope().chat_id.clone(),
        },
        auth: AdapterAuthContext {
            authenticated: true,
            auth_kind: "synthetic_token".to_string(),
            principal: "synthetic-operator".to_string(),
        },
        idempotency_key: "idempotency-eap1".to_string(),
        audit_id: "audit-eap1".to_string(),
        payload,
    }
}

#[test]
fn protocol_version_rejects_unknown_wire_versions() {
    let encoded = serde_json::to_string(&ExternalAiMemoryProtocolVersion::V1)
        .expect("serialize protocol version");
    assert_eq!(encoded, "\"beetle-memory.external-ai.v1\"");

    let error =
        serde_json::from_str::<ExternalAiMemoryProtocolVersion>("\"beetle-memory.external-ai.v0\"")
            .expect_err("unknown protocol version must fail closed");
    assert!(error.to_string().contains("unknown variant"), "{error}");
}

#[test]
fn protocol_envelope_requires_version_and_runtime_binding_without_legacy_defaults() {
    let runtime = runtime();
    let lease = runtime
        .acquire_runtime_budget_lease()
        .expect("runtime budget lease");
    let complete = serde_json::json!({
        "protocol_version": ExternalAiMemoryProtocolVersion::V1,
        "runtime_binding": AdapterProtocolBinding::for_runtime(&runtime, &lease),
        "request_id": "request-wire-contract",
        "transport": "http",
        "mode": "server",
        "operation": "recall",
        "source": {
            "source_id": "source-wire-contract",
            "source_kind": "synthetic_http_client",
            "agent_id": runtime.identity().agent_id,
            "owner_id": runtime.identity().owner_id,
            "channel": runtime.scope().channel,
            "chat_id": runtime.scope().chat_id,
        },
        "auth": {
            "authenticated": true,
            "auth_kind": "synthetic_token",
            "principal": "synthetic-operator",
        },
        "idempotency_key": "idempotency-wire-contract",
        "audit_id": "audit-wire-contract",
        "payload": {"query": "synthetic"},
    });
    serde_json::from_value::<AdapterEnvelope<serde_json::Value>>(complete.clone())
        .expect("complete versioned envelope");

    for required in ["protocol_version", "runtime_binding"] {
        let mut incomplete = complete.clone();
        incomplete
            .as_object_mut()
            .expect("envelope object")
            .remove(required);
        let error = serde_json::from_value::<AdapterEnvelope<serde_json::Value>>(incomplete)
            .expect_err("legacy envelope must not deserialize");
        assert!(error.to_string().contains("missing field"), "{error}");
    }

    let mut unknown = complete;
    unknown["legacy_scope"] = serde_json::json!("board.self");
    let error = serde_json::from_value::<AdapterEnvelope<serde_json::Value>>(unknown)
        .expect_err("unknown envelope fields must fail closed");
    assert!(error.to_string().contains("unknown field"), "{error}");
}

#[test]
fn exact_runtime_binding_is_required_before_finalize_turn_reaches_sdk() {
    let runtime = runtime();
    let mut rejected = envelope(&runtime, finalize("turn-binding"));
    rejected.runtime_binding.memory_space_id = "space:foreign".to_string();

    let response = dispatch_adapter_command(&runtime, rejected).expect("typed rejection");
    match response {
        AdapterResponse::Rejected {
            error_key, reason, ..
        } => {
            assert_eq!(error_key, AdapterErrorKey::RuntimeBindingMismatch);
            assert_eq!(reason, "memory_space_id_mismatch");
        }
        other => panic!("unexpected response: {other:?}"),
    }

    let accepted = dispatch_adapter_command(&runtime, envelope(&runtime, finalize("turn-binding")))
        .expect("canonical binding accepted");
    match accepted {
        AdapterResponse::Accepted {
            report: AdapterSdkReport::FinalizeTurn(report),
            ..
        } => {
            assert_eq!(report.turn_id, "turn-binding");
            assert!(report.session_committed);
            assert!(report.transcript_committed);
            assert!(!report.maintenance_performed);
            let safe_json = serde_json::to_string(&report).expect("safe finalize report");
            for forbidden in ["private_garden", "semantic_governance", "raw_material"] {
                assert!(
                    !safe_json.contains(forbidden),
                    "safe report leaked {forbidden}"
                );
            }
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn subject_identity_scope_profile_privacy_and_budget_drift_fail_closed() {
    type BindingMutation = Box<dyn Fn(&mut AdapterEnvelope<AdapterCommand>)>;
    let cases: Vec<(&str, BindingMutation)> = vec![
        (
            "mounted_subject_id_mismatch",
            Box::new(|envelope| {
                envelope.runtime_binding.mounted_subject_id = "agent:foreign".to_string();
            }),
        ),
        (
            "agent_id_mismatch",
            Box::new(|envelope| {
                envelope.runtime_binding.agent_id = "foreign-agent".to_string();
            }),
        ),
        (
            "owner_id_mismatch",
            Box::new(|envelope| {
                envelope.runtime_binding.owner_id = "foreign-owner".to_string();
            }),
        ),
        (
            "conversation_scope_mismatch",
            Box::new(|envelope| {
                envelope.runtime_binding.chat_id = "foreign-chat".to_string();
            }),
        ),
        (
            "profile_mismatch",
            Box::new(|envelope| {
                envelope.runtime_binding.profile = ProfileId::EspEmbeddedSdk;
            }),
        ),
        (
            "privacy_policy_mismatch",
            Box::new(|envelope| {
                envelope
                    .runtime_binding
                    .privacy
                    .private_plane_projection_allowed = true;
            }),
        ),
        (
            "capability_snapshot_mismatch",
            Box::new(|envelope| {
                envelope.runtime_binding.capabilities.write_visible = false;
            }),
        ),
        (
            "render_budget_mismatch",
            Box::new(|envelope| {
                envelope
                    .runtime_binding
                    .render_budget
                    .system_block_max_chars -= 1;
            }),
        ),
        (
            "budget_report_id_mismatch",
            Box::new(|envelope| {
                envelope.runtime_binding.budget_report_id = "rtb-v2-forged".to_string();
            }),
        ),
    ];

    for (expected_reason, mutate) in cases {
        let runtime = runtime();
        let mut request = envelope(&runtime, finalize(&format!("turn-{expected_reason}")));
        mutate(&mut request);
        let response = dispatch_adapter_command(&runtime, request).expect("typed rejection");
        match response {
            AdapterResponse::Rejected {
                error_key, reason, ..
            } => {
                assert_eq!(error_key, AdapterErrorKey::RuntimeBindingMismatch);
                assert_eq!(reason, expected_reason);
            }
            other => panic!("unexpected response for {expected_reason}: {other:?}"),
        }
    }
}

#[test]
fn source_identity_cannot_override_the_bound_runtime_identity() {
    let runtime = runtime();
    let mut request = envelope(&runtime, finalize("turn-source"));
    request.source.agent_id = "foreign-agent".to_string();

    let response = dispatch_adapter_command(&runtime, request).expect("typed rejection");
    match response {
        AdapterResponse::Rejected {
            error_key, reason, ..
        } => {
            assert_eq!(error_key, AdapterErrorKey::RuntimeBindingMismatch);
            assert_eq!(reason, "source_identity_mismatch");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}
