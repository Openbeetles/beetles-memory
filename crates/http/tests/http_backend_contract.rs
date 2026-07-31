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
use bm_sdk::{
    default_agent_subject_id, MemoryCapabilityPolicy, MemoryPrivacyPolicy, StoreBackendConfig,
};

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "http-backend-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
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

fn remote_runtime(
    capabilities: impl IntoIterator<Item = EntryOperationCapability>,
) -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "http-backend-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "http-backend".to_string(),
            chat_id: "chat-1".to_string(),
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
    assert!(response.contains("\"entry\""), "{response}");
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
fn explicit_http_idempotency_key_is_hashed_and_never_returned_verbatim() {
    let runtime = remote_runtime([EntryOperationCapability::Write]);
    let body = serde_json::json!({
        "name": "runtime_skill__http_wire_idem",
        "topic": "http",
        "title": "HTTP idempotency",
        "summary": "Stable payload",
        "content": "Hash the caller key before cache admission.",
        "source": "manual",
        "citations": ["http-backend-contract"],
        "source_chat_id": "chat-1",
        "owning_scope": {
            "kind": "subject",
            "mounted_subject_id": default_agent_subject_id("http-backend-agent"),
        },
        "creation_ref": {
            "kind": "replay_promotion",
            "candidate_ref": "http-backend-contract:idempotency",
            "verification_receipt_digest":
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        },
        "privacy_class": "shared_with_subject",
    })
    .to_string();
    let request = || {
        format!(
            "POST /memory/write HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer secret-token\r\nx-bm-auth-subject: forged-owner\r\nx-idempotency-key: caller-secret-key\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    };

    let first = serve_memory_request(&runtime, request());
    let replay = serve_memory_request(&runtime, request());

    assert!(first.starts_with("HTTP/1.1 200 OK"), "{first}");
    assert!(replay.starts_with("HTTP/1.1 409 Conflict"), "{replay}");
    assert!(!first.contains("caller-secret-key"), "{first}");
    assert!(!replay.contains("caller-secret-key"), "{replay}");
    assert!(replay.contains("explicit:v1:sha256:"), "{replay}");
}
