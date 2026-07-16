#![cfg(feature = "bridge-http")]

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use bm_a2a::serve_a2a_http_accepted_stream;
use bm_entry::{
    EntryAcceptedTcpStream, EntryAuthConfig, EntryBearerPrincipal, EntryIdempotencyConfig,
    EntryIdentity, EntryOperationCapability, EntryRuntime, EntryRuntimeConfig, EntryScope,
    EntryTransportConfig,
};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendConfig};

const TEST_BEARER: &str = "Bearer a2a-test-token";

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
    runtime_for_profile(native_runtime_profile())
}

fn runtime_for_profile(profile: ProfileId) -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "a2a-http-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "a2a-http".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(profile)
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig {
            cli: false,
            http_server: false,
            wss_client: false,
            wss_server: false,
            mcp_server: false,
            a2a_bridge: true,
            llm_gateway_server: false,
        },
        auth: EntryAuthConfig::required_bearer_principal(
            "a2a-test-token",
            EntryBearerPrincipal::new(
                "a2a-http-test-principal",
                "owner-default",
                EntryOperationCapability::all().iter().copied(),
            ),
        ),
        idempotency: EntryIdempotencyConfig { max_keys: 64 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

fn send_request(runtime: EntryRuntime, request: String) -> (String, bm_sdk::Result<()>) {
    send_request_with_bridge(runtime, Arc::new(support::bridge("a2a-http")), request)
}

fn send_request_with_bridge(
    runtime: EntryRuntime,
    bridge: Arc<bm_a2a::A2aBridge>,
    request: String,
) -> (String, bm_sdk::Result<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind A2A contract listener");
    let address = listener.local_addr().expect("A2A listener address");
    let server = thread::spawn(move || {
        let mut accepted = EntryAcceptedTcpStream::accept(&listener).expect("accept A2A peer");
        serve_a2a_http_accepted_stream(&runtime, bridge.as_ref(), &mut accepted)
    });
    let mut client = TcpStream::connect(address).expect("connect A2A contract listener");
    client
        .write_all(request.as_bytes())
        .expect("write A2A request");
    client
        .shutdown(std::net::Shutdown::Write)
        .expect("shutdown A2A request");
    let mut response = String::new();
    client
        .read_to_string(&mut response)
        .expect("read A2A response");
    (response, server.join().expect("A2A server thread"))
}

fn recall_request(authorization: Option<&str>) -> String {
    let body =
        r#"{"name":"memory_recall_request","payload":{"query":"deployment runtime","limit":2}}"#;
    let authorization = authorization
        .map(|value| format!("Authorization: {value}\r\n"))
        .unwrap_or_default();
    format!(
        "POST /a2a/message HTTP/1.1\r\nHost: localhost\r\n{authorization}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
}

#[test]
fn a2a_http_rejects_runtime_over_budget_body_before_read_or_json_parse() {
    let runtime = runtime_for_profile(ProfileId::EspEmbeddedSdk);
    let body_max_bytes = runtime.runtime_budget().adapter_budget.http_body_max_bytes;
    assert!(body_max_bytes > 0);
    assert!(body_max_bytes <= 8 * 1024);
    let request = format!(
        "POST /a2a/message HTTP/1.1\r\nHost: localhost\r\nAuthorization: {}\r\nContent-Length: {}\r\n\r\n",
        TEST_BEARER,
        body_max_bytes + 1
    );

    let (response, result) = send_request(runtime, request);
    let error = result.expect_err("runtime-over-budget body must fail before body read");

    assert_eq!(error.stage(), "a2a_http_read");
    assert!(error.to_string().contains("pinned runtime adapter budget"));
    assert!(response.is_empty());
}

#[test]
fn a2a_http_rejects_ambiguous_framing_before_body_read() {
    let request = format!(
        "POST /a2a/message HTTP/1.1\r\nHost: localhost\r\nAuthorization: {}\r\nTransfer-Encoding: chunked\r\nContent-Length: 1\r\n\r\n",
        TEST_BEARER
    );
    let (response, result) = send_request(runtime(), request);
    let error = result.expect_err("ambiguous framing must fail before body read");

    assert_eq!(error.stage(), "a2a_http_header");
    assert!(error.to_string().contains("transfer-encoding"));
    assert!(response.is_empty());
}

#[test]
fn a2a_http_rejects_noncanonical_content_length_before_body_read() {
    let request = format!(
        "POST /a2a/message HTTP/1.1\r\nHost: localhost\r\nAuthorization: {}\r\nContent-Length: +1\r\n\r\n",
        TEST_BEARER
    );
    let (response, result) = send_request(runtime(), request);
    let error = result.expect_err("noncanonical content length must fail before body read");

    assert_eq!(error.stage(), "a2a_http_header");
    assert!(error.to_string().contains("content-length"));
    assert!(response.is_empty());
}

#[test]
fn a2a_http_bridge_authenticates_current_request_bearer() {
    let (response, result) = send_request(runtime(), recall_request(Some(TEST_BEARER)));
    result.expect("serve authenticated A2A HTTP request");

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains(r#""kind":"memory_report""#), "{response}");
    assert!(
        response.contains(r#""permissions":["memory_report"]"#),
        "{response}"
    );
    assert!(!response.contains("executor"), "{response}");
    assert!(!response.contains("workflow"), "{response}");
}

#[test]
fn a2a_http_missing_bearer_is_not_satisfied_by_reused_bridge_identity() {
    let bridge = Arc::new(support::bridge("reused-a2a-http"));
    let (first, first_result) = send_request_with_bridge(
        runtime(),
        Arc::clone(&bridge),
        recall_request(Some(TEST_BEARER)),
    );
    first_result.expect("authenticated request");
    assert!(first.starts_with("HTTP/1.1 200 OK"), "{first}");

    let (second, second_result) = send_request_with_bridge(runtime(), bridge, recall_request(None));
    second_result.expect("write request-scoped auth rejection");
    assert!(second.starts_with("HTTP/1.1 401 Unauthorized"), "{second}");
    assert!(second.contains("missing_bearer_token"), "{second}");
    assert!(!second.contains("memory_report"), "{second}");
}

#[test]
fn a2a_http_missing_bearer_is_rejected_before_waiting_for_body() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind A2A contract listener");
    let address = listener.local_addr().expect("A2A listener address");
    let server = thread::spawn(move || {
        let runtime = runtime();
        let bridge = support::bridge("missing-bearer-before-body");
        let mut accepted = EntryAcceptedTcpStream::accept(&listener).expect("accept A2A peer");
        serve_a2a_http_accepted_stream(&runtime, &bridge, &mut accepted)
    });
    let mut client = TcpStream::connect(address).expect("connect A2A contract listener");
    client
        .set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .expect("set client read timeout");
    client
        .write_all(b"POST /a2a/message HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1024\r\n\r\n")
        .expect("write A2A headers without body");

    let mut response = String::new();
    client
        .read_to_string(&mut response)
        .expect("missing bearer response must not wait for request body");
    server
        .join()
        .expect("A2A server thread")
        .expect("write request-scoped auth rejection");

    assert!(
        response.starts_with("HTTP/1.1 401 Unauthorized"),
        "{response}"
    );
    assert!(response.contains("missing_bearer_token"), "{response}");
}
