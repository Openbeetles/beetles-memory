#![cfg(all(feature = "server-std", unix))]

mod support;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;

use bm_entry::{
    EntryAcceptedTcpStream, EntryAuthConfig, EntryBearerPrincipal, EntryIdempotencyConfig,
    EntryIdentity, EntryOperationCapability, EntryRuntime, EntryRuntimeConfig, EntryScope,
    EntryTransportConfig,
};
use bm_http::{serve_http_accepted_stream, HttpConsoleServices};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "http-backend-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            conversation_id: None,
            channel: "http-backend".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
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
fn http_server_feature_enables_entry_governance_model_client() {
    assert!(bm_entry::entry_governance_model_client_compiled());
}

#[test]
fn real_http_socket_finalize_queues_then_reports_durable_configuration_block() {
    let runtime = runtime();
    let body = serde_json::json!({
        "turn": {
            "turn_id": "turn-http-pl1d",
            "conversation": {
                "channel": "http-backend",
                "chat_id": "chat-1",
                "conversation_id": "conversation-http-pl1d"
            },
            "subject": "agent:http-backend-agent",
            "delivery_status": "delivered",
            "source": {
                "ingress": "user",
                "channel": "http-backend",
                "provider": null,
                "protocol": "native",
                "endpoint": "/memory/finalize-turn",
                "model_alias": null,
                "model_resolved": null,
                "request_id": "request-http-pl1d",
                "client_conversation_hint": "conversation-http-pl1d"
            },
            "input_messages": [{
                "role": "user",
                "content": "请记住 HTTP 真实套接字闭环。",
                "authority": "user_asserted",
                "observed_at": 1,
                "speaker_id": "user",
                "speaker_kind": "human"
            }],
            "assistant_message": {
                "role": "assistant",
                "content": "已记录。",
                "authority": "assistant_utterance",
                "observed_at": 1,
                "speaker_id": "assistant",
                "speaker_kind": "assistant"
            },
            "external_content_used": false
        },
        "learning": bm_sdk::PostTurnLearningInputV2::empty()
    })
    .to_string();
    let response = serve_memory_request(
        &runtime,
        format!(
            "POST /memory/finalize-turn HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let payload: serde_json::Value = serde_json::from_str(
        response
            .split_once("\r\n\r\n")
            .expect("HTTP response body")
            .1,
    )
    .expect("finalize response JSON");
    assert_eq!(
        payload["result"]["memoryConsolidation"]["state"], "queued",
        "{payload}"
    );
    let job_id = payload["result"]["memoryConsolidation"]["jobId"]
        .as_str()
        .expect("queued job id")
        .to_string();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let job = runtime
            .runtime()
            .governance_job_status(bm_sdk::MemoryGovernanceJobStatusRequest {
                job_id: job_id.clone(),
            })
            .expect("job status")
            .job;
        if job.status == bm_sdk::PostTurnGovernanceJobStatusV2::BlockedConfiguration {
            assert_eq!(
                job.blocking_reason.as_deref(),
                Some("governance_execution_binding_unavailable")
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "job remained {:?}",
            job.status
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn remote_runtime(
    capabilities: impl IntoIterator<Item = EntryOperationCapability>,
) -> EntryRuntime {
    remote_runtime_with_maintenance(capabilities, true)
}

fn remote_runtime_with_maintenance(
    capabilities: impl IntoIterator<Item = EntryOperationCapability>,
    maintenance: bool,
) -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    capability.maintenance_enabled = maintenance;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "http-backend-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            conversation_id: None,
            channel: "http-backend".to_string(),
            chat_id: "chat-remote".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::required_bearer_principal(
            "secret-token",
            EntryBearerPrincipal::new("http-wire-principal", "owner-default", capabilities),
        ),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_finalize_body(runtime: &EntryRuntime, id: &str, feedback: bool) -> serde_json::Value {
    use bm_sdk::*;
    let memory = runtime.runtime();
    let mut learning = PostTurnLearningInputV2::empty();
    let mut observations = Vec::new();
    if feedback {
        let now = memory.config().clock.now_secs();
        observations.push(ToolObservationDigest {
            observation_id: format!("obs-{id}"),
            call_id: format!("call-{id}"),
            tool_name: "inspect".into(),
            summary: "Synthetic HTTP execution result".into(),
            external_content: false,
        });
        learning.tool_call_count = 1;
        learning.agent_tool_feedback.push(AgentToolUsageFeedbackV3 {
            registry_ref: memory.agent_tool_registries()[0].registry_ref(),
            tool_id: "inspect".into(),
            schema_fingerprint: "schema-1".into(),
            execution_facts: vec![ToolExecutionFactV1 {
                observation_id: format!("obs-{id}"),
                call_id: format!("call-{id}"),
                outcome: ToolExecutionOutcome::Failed,
                source_sensitivity: ProceduralSourceSensitivity::Private,
                started_at: Some(now),
                completed_at: Some(now),
            }],
            method_evidence: vec![ToolMethodEvidenceV1 {
                method_id: format!("method-{id}"),
                task_signature: "private-http-input".into(),
                body: "PRIVATE_HTTP_METHOD_MUST_NOT_PERSIST".into(),
                execution_refs: vec![format!("obs-{id}")],
                source_sensitivity: ProceduralSourceSensitivity::Private,
                external_content: false,
            }],
        });
    }
    let turn = CanonicalTurnDelta {
        turn_id: id.into(),
        conversation: ConversationScope {
            channel: memory.scope().channel.clone(),
            chat_id: memory.scope().chat_id.clone(),
            conversation_id: Some(memory.scope().conversation_id_or_chat_id().into()),
        },
        subject: memory.subject_id().into(),
        delivery_status: MemoryTurnDeliveryStatus::Delivered,
        source: MemoryTurnSource {
            ingress: IngressKind::User,
            channel: memory.scope().channel.clone(),
            provider: None,
            protocol: MemoryTurnProtocol::Native,
            endpoint: Some("/memory/finalize-turn".into()),
            model_alias: None,
            model_resolved: None,
            request_id: Some(format!("request-{id}")),
            client_conversation_hint: None,
        },
        actor: None,
        input_messages: vec![TranscriptInputMessage::user(
            "Synthetic authorized HTTP turn",
        )],
        assistant_message: Some(TranscriptInputMessage::assistant(
            "Synthetic response delivered",
        )),
        tool_observations: observations,
        external_content_used: false,
        candidate_ids: Vec::new(),
    };
    serde_json::json!({ "turn": turn, "learning": learning })
}

#[test]
#[cfg(feature = "nonproduction-replay-harness")]
fn pfi2_real_http_body_and_bearer_cannot_mint_procedural_submission_authority() {
    use bm_sdk::*;
    // No background worker can race the exact Store snapshot assertions.
    let runtime = remote_runtime_with_maintenance([EntryOperationCapability::FinalizeTurn], false);
    let tools = AgentToolRegistrySnapshot::compact(
        "http-authority-tools",
        "host",
        vec![AgentToolDescriptor::compact(
            "inspect", "Inspect", "schema-1",
        )],
        runtime.runtime().config().clock.now_secs(),
    );
    runtime
        .runtime()
        .upsert_agent_tool_registry(tools.clone())
        .unwrap();
    let registered = runtime
        .control_procedural_producer(MemoryProceduralProducerControlRequest {
            operation_id: "http-explicit-producer".into(),
            expected_revision: None,
            state: ProceduralProducerStateV1::Active,
            spec: ProceduralProducerSpecV1 {
                binding_id: "http-executor".into(),
                scope: ProceduralProducerScopeV1 {
                    memory_space_id: runtime.runtime().memory_space_id().into(),
                    mounted_subject_id: runtime.runtime().subject_id().into(),
                    channel_id: "http-backend".into(),
                    chat_id: "chat-remote".into(),
                },
                principal: ProceduralProducerPrincipalV1::EntryPrincipal {
                    principal_id: "http-wire-principal".into(),
                    owner_id: "owner-default".into(),
                },
                source_authority: ProceduralProducerSourceAuthorityV1::RuntimeObservation,
                claims: ProceduralProducerClaimsV1 {
                    execution_facts: true,
                    method_declarations: true,
                    usage_feedback: false,
                    source_classifications: vec![
                        ProceduralSourceSensitivity::NonPrivate,
                        ProceduralSourceSensitivity::Private,
                    ],
                },
                tools: vec![ProceduralProducerToolV1 {
                    registry_ref: tools.registry_ref(),
                    tool_id: "inspect".into(),
                    schema_fingerprint: "schema-1".into(),
                }],
                source_config_ref: "synthetic-http-executor".into(),
            },
        })
        .unwrap();
    let _not_passed_to_http = runtime
        .procedural_submission_capability(&registered.binding.revision_ref().unwrap())
        .unwrap();
    let wire = |id: &str, body: &serde_json::Value| {
        let body = body.to_string();
        format!("POST /memory/finalize-turn HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer secret-token\r\nx-idempotency-key: {id}\r\nContent-Length: {}\r\n\r\n{body}", body.len())
    };
    let response = serve_memory_request(
        &runtime,
        wire(
            "pfi2-http-ordinary",
            &pfi2_finalize_body(&runtime, "pfi2-http-ordinary", false),
        ),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let before = runtime
        .runtime()
        .replay_harness()
        .export_store_snapshot()
        .unwrap();
    assert!(before
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "conversation_transcript"
            && doc.value["turn_id"] == "pfi2-http-ordinary"));
    for field in ["authority", "producer", "procedural_submission"] {
        let mut body = pfi2_finalize_body(&runtime, field, true);
        if field == "procedural_submission" {
            body[field] = serde_json::json!({"binding_id": "http-executor"});
        } else {
            body["learning"][field] = serde_json::json!({"kind": "host_runtime_observation"});
        }
        let (result, response) = serve_memory_request_result(&runtime, wire(field, &body));
        let error = result.expect_err("real HTTP decoder must reject authority smuggling");
        assert_eq!(error.stage(), "adapter_json_command");
        assert!(!response.starts_with("HTTP/1.1 200"));
        assert!(!response.contains("PRIVATE_HTTP_METHOD_MUST_NOT_PERSIST"));
        assert!(!error
            .to_string()
            .contains("PRIVATE_HTTP_METHOD_MUST_NOT_PERSIST"));
        assert_eq!(
            runtime
                .runtime()
                .replay_harness()
                .export_store_snapshot()
                .unwrap(),
            before
        );
    }
    let body = pfi2_finalize_body(&runtime, "pfi2-http-feedback-no-cap", true);
    assert!(
        bm_adapter::decode_json_adapter_command(
            bm_adapter::AdapterOperation::FinalizeTurn,
            &body.to_string()
        )
        .is_ok(),
        "the producer negative must reach Entry through a valid typed body"
    );
    let (result, response) =
        serve_memory_request_result(&runtime, wire("pfi2-http-feedback-no-cap", &body));
    let error = result.expect_err(
        "durable producer metadata and bearer alone cannot replace out-of-band capability",
    );
    assert_eq!(error.stage(), "post_turn_learning_evidence");
    let Error::Other { source, .. } = &error else {
        panic!("expected the typed procedural error")
    };
    assert_eq!(
        source
            .downcast_ref::<ProceduralLearningSdkError>()
            .unwrap()
            .key,
        ProceduralLearningErrorKeyV1::ProducerAuthorityRequired
    );
    assert!(!response.starts_with("HTTP/1.1 200"));
    assert!(!response.contains("PRIVATE_HTTP_METHOD_MUST_NOT_PERSIST"));
    assert!(!error
        .to_string()
        .contains("PRIVATE_HTTP_METHOD_MUST_NOT_PERSIST"));
    assert_eq!(
        runtime
            .runtime()
            .replay_harness()
            .export_store_snapshot()
            .unwrap(),
        before
    );
}

fn serve_memory_request(runtime: &EntryRuntime, request: String) -> String {
    let (result, response) = serve_memory_request_result(runtime, request);
    result.expect("serve HTTP request");
    response
}

fn serve_memory_request_result(
    runtime: &EntryRuntime,
    request: String,
) -> (bm_sdk::Result<()>, String) {
    serve_memory_request_result_with_console(runtime, request, HttpConsoleServices::none())
}

fn serve_memory_request_result_with_console(
    runtime: &EntryRuntime,
    request: String,
    console_services: HttpConsoleServices<'_>,
) -> (bm_sdk::Result<()>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind HTTP test listener");
    let addr = listener.local_addr().expect("HTTP test address");
    let client = thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).expect("connect HTTP test listener");
        stream.write_all(request.as_bytes()).expect("write request");
        stream.shutdown(Shutdown::Write).expect("shutdown request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        response
    });
    let mut accepted = EntryAcceptedTcpStream::accept(&listener).expect("accept HTTP test peer");
    let result = serve_http_accepted_stream(runtime, &mut accepted, console_services);
    drop(accepted);
    (result, client.join().expect("HTTP test client"))
}

#[test]
fn console_metric_failure_is_a_sanitized_structured_wire_response() {
    let runtime = runtime();
    let private_path_marker =
        std::env::temp_dir().join("bm-http-private-path-marker-that-must-not-leak");
    let event_store_paths = vec![private_path_marker.clone()];
    let services = HttpConsoleServices::none().with_memory_event_store_paths(&event_store_paths);

    let (result, response) = serve_memory_request_result_with_console(
        &runtime,
        "GET /console/overview?query=private-query-marker HTTP/1.1\r\nHost: localhost\r\n\r\n"
            .to_string(),
        services,
    );

    result.expect("structured console rejection must be written");
    assert!(
        response.starts_with("HTTP/1.1 500 Internal Server Error"),
        "{response}"
    );
    assert!(
        response.contains("content-type: application/json"),
        "{response}"
    );
    assert!(
        response.contains("x-bm-runtime-budget-report-id: rtb-v2-"),
        "{response}"
    );
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("HTTP response body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON error body");
    assert_eq!(body["status"], "rejected");
    assert_eq!(body["errorKey"], "RuntimeRejected");
    assert_eq!(body.as_object().expect("error object").len(), 2);
    for forbidden in [
        private_path_marker.to_string_lossy().as_ref(),
        "private-query-marker",
        "runtime_metrics_event_store_root",
        "No such file",
    ] {
        assert!(!response.contains(forbidden), "{forbidden}: {response}");
    }
}

#[test]
fn std_http_stream_serves_profile_capabilities_through_entry_runtime() {
    let runtime = runtime();
    let response = serve_memory_request(
        &runtime,
        "GET /memory/profile/capabilities HTTP/1.1\r\nHost: localhost\r\nx-request-id: req-http-backend\r\nx-idempotency-key: idem-http-backend\r\nx-audit-id: audit-http-backend\r\n\r\n".to_string(),
    );

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains("\"profile\""), "{response}");
    assert!(
        response.contains("mutation_operation_inventory"),
        "{response}"
    );
    assert!(response.contains("sdk_mutation_inventory"), "{response}");
    assert!(response.contains("mutation_receipt_policy"), "{response}");
    assert!(
        response.contains("x-bm-runtime-budget-report-id: rtb-v2-"),
        "{response}"
    );
}

#[test]
fn std_http_stream_rejects_declared_body_before_reading_payload() {
    let runtime = runtime();
    let max_bytes = runtime.runtime_budget().adapter_budget.http_body_max_bytes;
    let (result, response) = serve_memory_request_result(
        &runtime,
        format!(
            "POST /memory/recall HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            max_bytes + 1
        ),
    );
    let error = result.expect_err("oversized declared request must fail closed");
    assert_eq!(error.stage(), "http_body");
    assert!(error.to_string().contains("exceeds runtime"));
    assert!(response.is_empty());
}

#[test]
fn http_wire_rejects_noncanonical_length_and_invalid_header_names() {
    let runtime = runtime();
    for request in [
        "GET /memory/profile/capabilities HTTP/1.1\r\nHost: localhost\r\nContent-Length: +0\r\n\r\n",
        "GET /memory/profile/capabilities HTTP/1.1\r\nHost: localhost\r\n Content-Length: 0\r\n\r\n",
        "GET /memory/profile/capabilities HTTP/1.1\r\nHost: localhost\r\nContent-Length : 0\r\n\r\n",
        "GET\t/memory/profile/capabilities\tHTTP/1.1\r\nHost: localhost\r\n\r\n",
    ] {
        let (result, response) = serve_memory_request_result(&runtime, request.to_string());
        let error = result.expect_err("noncanonical HTTP framing must fail closed");
        assert!(matches!(error.stage(), "http_body" | "http_headers"));
        assert!(response.is_empty());
    }
}

#[test]
fn http_wire_accepts_exact_body_budget_and_reports_pinned_id() {
    let runtime = runtime();
    let max_bytes = runtime.runtime_budget().adapter_budget.http_body_max_bytes;
    let mut body = br#"{"temporal_operation":{"kind":"current"},"query":"exact"}"#.to_vec();
    body.resize(max_bytes, b' ');
    let body = String::from_utf8(body).expect("body");
    let response = serve_memory_request(
        &runtime,
        format!(
            "POST /memory/recall HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        ),
    );

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(
        response.contains("x-bm-runtime-budget-report-id: rtb-v2-"),
        "{response}"
    );
}

#[test]
fn forged_loopback_and_auth_subject_headers_cannot_authenticate_remote_http() {
    let runtime = remote_runtime([EntryOperationCapability::Recall]);
    let body = r#"{"query":"wire"}"#;
    let response = serve_memory_request(
        &runtime,
        format!(
            "POST /memory/recall HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nx-loopback: true\r\nx-bm-auth-subject: forged-owner\r\n\r\n{}",
            body.len(),
            body
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 401 Unauthorized"),
        "{response}"
    );
    assert!(response.contains("missing_bearer_token"), "{response}");
}

#[test]
fn missing_http_operation_key_fails_closed_and_explicit_key_is_safe_and_replayable() {
    let runtime = remote_runtime([EntryOperationCapability::Write]);
    let body = support::factual_memory_write_body(
        "http-wire-idempotency",
        "HTTP durable mutation identity commits one factual effect and receipt.",
    );
    let request = || {
        format!(
            "POST /memory/write HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer secret-token\r\nx-bm-auth-subject: forged-owner\r\nx-idempotency-key: caller-secret-key\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    };

    let missing_operation_key = serve_memory_request(
        &runtime,
        format!(
            "POST /memory/write HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer secret-token\r\nx-bm-auth-subject: forged-owner\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    assert!(
        missing_operation_key.starts_with("HTTP/1.1 422 Unprocessable Entity"),
        "{missing_operation_key}"
    );
    assert!(
        missing_operation_key.contains("mutation_operation_id_required"),
        "{missing_operation_key}"
    );
    assert!(
        !missing_operation_key.contains("automatic:v1:sha256:"),
        "{missing_operation_key}"
    );

    let first = serve_memory_request(&runtime, request());
    let replay = serve_memory_request(&runtime, request());

    assert!(first.starts_with("HTTP/1.1 200 OK"), "{first}");
    assert!(first.contains("\"receipt\""), "{first}");
    assert!(first.contains("explicit:v1:sha256:"), "{first}");
    assert!(replay.starts_with("HTTP/1.1 200 OK"), "{replay}");
    assert!(replay.contains("\"status\":\"replayed\""), "{replay}");
    assert!(!first.contains("caller-secret-key"), "{first}");
    assert!(!replay.contains("caller-secret-key"), "{replay}");
    assert!(replay.contains("explicit:v1:sha256:"), "{replay}");
    let records = runtime
        .runtime()
        .list_long_term_memory(bm_sdk::MemoryLongTermListRequest {
            query: Default::default(),
            cursor: None,
            limit: 8,
            view: bm_sdk::MemoryLongTermControlView::RawOwner,
        })
        .unwrap()
        .records;
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].record.source_chat_id.as_deref(),
        Some("chat-remote")
    );
    assert_eq!(
        records[0].record.content,
        "HTTP durable mutation identity commits one factual effect and receipt."
    );
    #[cfg(feature = "nonproduction-replay-harness")]
    let before = runtime
        .runtime()
        .replay_harness()
        .export_store_snapshot()
        .unwrap();
    for field in ["source_chat_id", "owner_id", "subject_id"] {
        let mut payload: serde_json::Value = serde_json::from_str(&body).unwrap();
        payload[field] = "forged-scope".into();
        let mut request =
            bm_http::HttpRuntimeRequest::post_json("/memory/write", payload.to_string())
                .with_idempotency_key("forged-scope");
        request.authorization = Some("Bearer secret-token".into());
        assert_eq!(
            bm_http::handle_http_in_process_request(&runtime, request)
                .expect_err("scope override rejected")
                .stage(),
            "adapter_json_command"
        );
    }
    #[cfg(feature = "nonproduction-replay-harness")]
    assert!(
        runtime
            .runtime()
            .replay_harness()
            .export_store_snapshot()
            .unwrap()
            == before,
        "scope injection must not write"
    );
}
