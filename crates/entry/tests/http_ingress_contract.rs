use std::io::Write;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use bm_entry::{
    read_authorized_http_request, EntryAcceptedTcpStream, EntryAuthConfig, EntryBearerPrincipal,
    EntryHttpAuthorization, EntryHttpIngressErrorKind, EntryHttpIngressLimits,
    EntryOperationCapability,
};

fn auth_config(
    capabilities: impl IntoIterator<Item = EntryOperationCapability>,
) -> EntryAuthConfig {
    EntryAuthConfig::required_bearer_principal(
        "ingress-token",
        EntryBearerPrincipal::new("ingress-principal", "owner-default", capabilities),
    )
}

fn accepted_head_then_stall(head: String) -> (EntryAcceptedTcpStream, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ingress listener");
    let addr = listener.local_addr().expect("ingress listener address");
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).expect("connect ingress listener");
        stream
            .write_all(head.as_bytes())
            .expect("write request head");
        std::thread::sleep(Duration::from_millis(500));
    });
    let accepted = EntryAcceptedTcpStream::accept(&listener).expect("accept ingress stream");
    (accepted, client)
}

#[test]
fn unauthenticated_request_is_rejected_before_body_read() {
    let auth = auth_config([EntryOperationCapability::Recall]);
    let head = "POST /v1/recall HTTP/1.1\r\nhost: localhost\r\nauthorization: Bearer wrong\r\ncontent-length: 1024\r\n\r\n".to_string();
    let (mut stream, client) = accepted_head_then_stall(head);
    let started = Instant::now();
    let error = read_authorized_http_request(
        &mut stream,
        EntryHttpIngressLimits::new(4096, 2048).expect("limits"),
        |accepted, head| {
            EntryHttpAuthorization::require(
                auth.authenticate_accepted_tcp_stream(
                    accepted,
                    head.header("authorization"),
                    "loopback",
                ),
                EntryOperationCapability::Recall,
            )
        },
    )
    .expect_err("wrong bearer must fail before body");
    assert_eq!(error.kind(), EntryHttpIngressErrorKind::Unauthorized);
    assert!(started.elapsed() < Duration::from_millis(250));
    client.join().expect("client");
}

#[test]
fn missing_capability_is_rejected_before_body_read() {
    let auth = auth_config([EntryOperationCapability::Project]);
    let head = "POST /v1/recall HTTP/1.1\r\nhost: localhost\r\nauthorization: Bearer ingress-token\r\ncontent-length: 1024\r\n\r\n".to_string();
    let (mut stream, client) = accepted_head_then_stall(head);
    let started = Instant::now();
    let error = read_authorized_http_request(
        &mut stream,
        EntryHttpIngressLimits::new(4096, 2048).expect("limits"),
        |accepted, head| {
            EntryHttpAuthorization::require(
                auth.authenticate_accepted_tcp_stream(
                    accepted,
                    head.header("authorization"),
                    "loopback",
                ),
                EntryOperationCapability::Recall,
            )
        },
    )
    .expect_err("missing capability must fail before body");
    assert_eq!(error.kind(), EntryHttpIngressErrorKind::Forbidden);
    assert!(started.elapsed() < Duration::from_millis(250));
    client.join().expect("client");
}

#[test]
fn authorized_request_reads_exact_declared_body() {
    let auth = auth_config([EntryOperationCapability::Recall]);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ingress listener");
    let addr = listener.local_addr().expect("ingress listener address");
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).expect("connect ingress listener");
        stream
            .write_all(
                b"POST /v1/recall HTTP/1.1\r\nhost: localhost\r\nauthorization: Bearer ingress-token\r\ncontent-length: 2\r\n\r\n{}",
            )
            .expect("write request");
        stream.shutdown(Shutdown::Write).expect("shutdown request");
    });
    let mut stream = EntryAcceptedTcpStream::accept(&listener).expect("accept ingress stream");
    let request = read_authorized_http_request(
        &mut stream,
        EntryHttpIngressLimits::new(4096, 2048).expect("limits"),
        |accepted, head| {
            EntryHttpAuthorization::require(
                auth.authenticate_accepted_tcp_stream(
                    accepted,
                    head.header("authorization"),
                    "loopback",
                ),
                EntryOperationCapability::Recall,
            )
        },
    )
    .expect("authorized request");
    let (head, body, decision) = request.into_parts();
    assert_eq!(head.method(), "POST");
    assert_eq!(head.target(), "/v1/recall");
    assert_eq!(body, b"{}");
    assert!(decision.is_authenticated());
    client.join().expect("client");
}
