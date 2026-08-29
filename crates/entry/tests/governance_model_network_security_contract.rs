#![cfg(feature = "governance-model-client-std")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use bm_entry::ReqwestGovernanceLlmHttpClient;
use bm_sdk::LlmHttpClient;

fn one_shot_server(response: String) -> (String, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind synthetic server");
    let address = listener.local_addr().expect("server address");
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept synthetic request");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap_or(0);
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(response.as_bytes())
            .expect("write synthetic response");
        request
    });
    (format!("http://{address}/v1"), handle)
}

#[test]
fn redirect_is_not_followed_and_never_replays_secret_or_body_to_another_origin() {
    let redirect_sink = TcpListener::bind("127.0.0.1:0").expect("bind redirect sink");
    redirect_sink
        .set_nonblocking(true)
        .expect("nonblocking redirect sink");
    let sink_address = redirect_sink.local_addr().expect("sink address");
    let redirect_response = format!(
        "HTTP/1.1 302 Found\r\nlocation: http://{sink_address}/capture\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    );
    let (endpoint, server) = one_shot_server(redirect_response);
    let mut client =
        ReqwestGovernanceLlmHttpClient::for_endpoint(&endpoint, 2_000, 1024).expect("client");
    let (status, _) = client
        .do_post(
            &format!("{endpoint}/chat/completions"),
            &[
                ("content-type", "application/json"),
                ("authorization", "Bearer synthetic-secret"),
            ],
            br#"{"private":"synthetic-body"}"#,
        )
        .expect("redirect response");
    assert_eq!(status, 302);
    server.join().expect("redirect server");
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        matches!(redirect_sink.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "redirect target must receive zero connections"
    );
}

#[test]
fn immutable_origin_is_checked_before_network_egress() {
    let expected = "http://127.0.0.1:18080/v1";
    let sink = TcpListener::bind("127.0.0.1:0").expect("bind mismatch sink");
    sink.set_nonblocking(true)
        .expect("nonblocking mismatch sink");
    let target = format!(
        "http://{}/v1/chat",
        sink.local_addr().expect("sink address")
    );
    let mut client =
        ReqwestGovernanceLlmHttpClient::for_endpoint(expected, 2_000, 1024).expect("client");
    let error = client
        .do_post(&target, &[("authorization", "Bearer secret")], b"private")
        .expect_err("origin mismatch must fail closed");
    assert_eq!(error.stage(), "governance_model_http");
    assert!(
        matches!(sink.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "origin mismatch must fail before opening a socket"
    );
}

#[test]
fn remote_plain_http_is_rejected_but_loopback_http_is_allowed() {
    assert!(
        ReqwestGovernanceLlmHttpClient::for_endpoint("http://example.com/v1", 2_000, 1024).is_err()
    );
    assert!(ReqwestGovernanceLlmHttpClient::for_endpoint(
        "http://127.0.0.1:11434/api",
        2_000,
        1024
    )
    .is_ok());
}

#[test]
fn response_byte_budget_fails_closed_before_body_is_accepted() {
    let response =
        "HTTP/1.1 200 OK\r\ncontent-length: 16\r\nconnection: close\r\n\r\n0123456789abcdef";
    let (endpoint, server) = one_shot_server(response.to_string());
    let mut client =
        ReqwestGovernanceLlmHttpClient::for_endpoint(&endpoint, 2_000, 8).expect("client");
    let error = client
        .do_post(&format!("{endpoint}/chat"), &[], b"{}")
        .expect_err("oversized response must fail closed");
    assert_eq!(error.stage(), "governance_model_http");
    server.join().expect("budget server");
}
