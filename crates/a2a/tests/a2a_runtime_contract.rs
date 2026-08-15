#![cfg(feature = "bridge-http")]

mod support;

use bm_a2a::{A2aPeerCapability, A2aRuntimeMessage};
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendConfig};

fn native_runtime_profile() -> ProfileId {
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
        ProfileId::LinuxDeviceStandaloneMemory
    }
}

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "a2a-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "a2a".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[test]
fn a2a_bridge_feature_enables_entry_governance_model_client() {
    assert!(bm_entry::entry_governance_model_client_compiled());
}

fn write_payload(name: &str, summary: &str) -> String {
    serde_json::json!({
        "name": name,
        "topic": "a2a-idempotency",
        "title": format!("A2A write {name}"),
        "summary": summary,
        "content": "Dispatch this write through the governed EntryRuntime path.",
        "source_chat_id": "chat-1",
        "owning_scope": {
            "kind": "subject",
            "mounted_subject_id": "agent:a2a-agent",
        },
        "creation_ref": {
            "kind": "replay_promotion",
            "candidate_ref": format!("test:a2a:{name}"),
            "verification_receipt_digest":
                "sha256:5555555555555555555555555555555555555555555555555555555555555555",
        },
        "privacy_class": "shared_with_subject",
    })
    .to_string()
}

fn finalize_payload() -> String {
    serde_json::json!({
        "turn": {
            "turn_id": "turn-a2a-pl1d",
            "conversation": {
                "channel": "a2a",
                "chat_id": "chat-1",
                "conversation_id": "conversation-a2a-pl1d"
            },
            "subject": "agent:a2a-agent",
            "delivery_status": "delivered",
            "source": {
                "ingress": "user",
                "channel": "a2a",
                "provider": null,
                "protocol": "native",
                "endpoint": "memory_finalize_turn_request",
                "model_alias": null,
                "model_resolved": null,
                "request_id": "request-a2a-pl1d",
                "client_conversation_hint": "conversation-a2a-pl1d"
            },
            "input_messages": [{
                "role": "user",
                "content": "A2A queued contract",
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
fn a2a_finalize_returns_typed_queued_report() {
    let runtime = runtime();
    let response = support::bridge("bridge-finalize")
        .handle_in_process_request(
            &runtime,
            "a2a-finalize-principal",
            A2aRuntimeMessage::json("memory_finalize_turn_request", finalize_payload()),
        )
        .expect("A2A finalize");
    let value: serde_json::Value =
        serde_json::from_str(&response.payload).expect("A2A finalize response");
    assert_eq!(value["operation"], "finalize_turn");
    assert_eq!(value["result"]["memoryConsolidation"]["state"], "queued");
    assert!(value["result"]["memoryConsolidation"]["jobId"].is_string());
}

fn response_status(response: &bm_a2a::A2aRuntimeResponse) -> String {
    let payload: serde_json::Value =
        serde_json::from_str(&response.payload).expect("A2A response JSON");
    payload["status"]
        .as_str()
        .expect("A2A response status")
        .to_string()
}

fn assert_exact_governed_result(payload: &str) {
    let value: serde_json::Value = serde_json::from_str(payload).expect("governed response JSON");
    let result = value["result"].clone();
    let dto: bm_adapter::AdapterGovernedSafeReportV1 =
        serde_json::from_value(result.clone()).expect("strict adapter governed safe DTO");
    assert_eq!(
        serde_json::to_value(dto).expect("serialize adapter governed safe DTO"),
        result
    );
}

#[test]
fn a2a_automatic_identity_accepts_two_distinct_writes_on_one_bridge() {
    let runtime = runtime();
    let bridge = support::bridge("bridge-auto");

    let first = bridge
        .handle_in_process_request(
            &runtime,
            "a2a-in-process-principal",
            A2aRuntimeMessage::json(
                "memory_write_candidate",
                write_payload("runtime_skill__a2a_auto_first", "first automatic write"),
            ),
        )
        .expect("first write");
    let second = bridge
        .handle_in_process_request(
            &runtime,
            "a2a-in-process-principal",
            A2aRuntimeMessage::json(
                "memory_write_candidate",
                write_payload("runtime_skill__a2a_auto_second", "second automatic write"),
            ),
        )
        .expect("second write");

    assert_eq!(response_status(&first), "accepted");
    assert_eq!(response_status(&second), "accepted");
}

#[test]
fn a2a_explicit_identity_replays_same_payload_and_rejects_conflict() {
    let runtime = runtime();
    let initial_bridge = support::bridge("bridge-explicit");
    let retry_bridge = support::bridge("bridge-explicit-retry");
    let payload = write_payload("runtime_skill__a2a_explicit", "stable payload");

    let first = initial_bridge
        .handle_in_process_request(
            &runtime,
            "a2a-in-process-principal",
            A2aRuntimeMessage::json("memory_write_candidate", payload.clone())
                .with_idempotency_key("a2a-caller-key"),
        )
        .expect("first write");
    let replay = retry_bridge
        .handle_in_process_request(
            &runtime,
            "a2a-in-process-principal",
            A2aRuntimeMessage::json("memory_write_candidate", payload)
                .with_idempotency_key("a2a-caller-key"),
        )
        .expect("replay write");
    let conflict = retry_bridge
        .handle_in_process_request(
            &runtime,
            "a2a-in-process-principal",
            A2aRuntimeMessage::json(
                "memory_write_candidate",
                write_payload("runtime_skill__a2a_conflict", "different payload"),
            )
            .with_idempotency_key("a2a-caller-key"),
        )
        .expect("conflicting write");

    assert_eq!(response_status(&first), "accepted");
    assert_eq!(response_status(&replay), "duplicated");
    assert_eq!(response_status(&conflict), "rejected");
}

#[test]
fn a2a_bridge_peer_capability_only_narrows_local_visibility() {
    let bridge = support::bridge("bridge-1");
    assert!(bridge.merge_peer_visibility(A2aPeerCapability {
        memory_report_visible: true,
    }));
    assert!(!bridge.merge_peer_visibility(A2aPeerCapability {
        memory_report_visible: false,
    }));
}

#[test]
fn a2a_bridge_dispatches_memory_request_without_executor_permissions() {
    let runtime = runtime();
    let bridge = support::bridge("bridge-1");
    let response = bridge
        .handle_in_process_request(
            &runtime,
            "a2a-in-process-principal",
            A2aRuntimeMessage::json(
                "memory_recall_request",
                r#"{"temporal_operation":{"kind":"current"},"query":"release","limit":2}"#,
            ),
        )
        .expect("a2a request");

    assert_eq!(response.kind, "memory_report");
    assert!(!response.permissions.iter().any(|permission| {
        matches!(
            permission,
            bm_a2a::A2aPermission::Executor
                | bm_a2a::A2aPermission::Tool
                | bm_a2a::A2aPermission::Workflow
        )
    }));
    assert!(response.payload.contains("\"status\""));
    assert_exact_governed_result(&response.payload);
}

#[test]
fn a2a_bridge_decodes_declared_memory_operation_messages() {
    let runtime = runtime();
    let bridge = support::bridge("bridge-ops");
    let messages = [
        (
            "memory_write_candidate",
            r#"{"name":"runtime_skill__a2a_write","topic":"a2a","title":"A2A write","summary":"A2A write summary","content":"1. Decode A2A write.\n2. Dispatch through EntryRuntime.","source_chat_id":"chat-1","owning_scope":{"kind":"subject","mounted_subject_id":"agent:a2a-agent"},"creation_ref":{"kind":"replay_promotion","candidate_ref":"test:a2a:runtime_skill__a2a_write","verification_receipt_digest":"sha256:6666666666666666666666666666666666666666666666666666666666666666"},"privacy_class":"shared_with_subject"}"#,
        ),
        (
            "memory_projection_request",
            r#"{"temporal_operation":{"kind":"current"},"user_query":"release","system_max_len":1024}"#,
        ),
        ("memory_long_term_list_request", r#"{"query":{},"limit":2}"#),
    ];

    for (name, payload) in messages {
        let response = bridge
            .handle_in_process_request(
                &runtime,
                "a2a-in-process-principal",
                A2aRuntimeMessage::json(name, payload),
            )
            .unwrap_or_else(|err| panic!("{name} failed: {err}"));
        assert_eq!(response.kind, "memory_report");
        assert!(
            response.payload.contains("\"status\""),
            "{name}: {}",
            response.payload
        );
        assert!(!response.permissions.iter().any(|permission| {
            matches!(
                permission,
                bm_a2a::A2aPermission::Executor
                    | bm_a2a::A2aPermission::Tool
                    | bm_a2a::A2aPermission::Workflow
            )
        }));
    }
}
