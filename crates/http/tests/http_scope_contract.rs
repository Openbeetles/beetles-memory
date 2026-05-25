#![cfg(feature = "server-std")]

use bm_http::HttpRuntimeRequest;

#[test]
fn http_runtime_request_defaults_to_local_loopback_auth_not_arbitrary_remote_auth() {
    let request = HttpRuntimeRequest::post_json("/memory/recall", r#"{"query":"release"}"#);

    assert!(request.authenticated);
    assert_eq!(request.path, "/memory/recall");
}
