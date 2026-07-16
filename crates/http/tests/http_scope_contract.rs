#![cfg(feature = "server-std")]

use bm_http::HttpRuntimeRequest;

#[test]
fn http_runtime_request_does_not_expose_caller_controlled_trust_state() {
    let request = HttpRuntimeRequest::post_json("/memory/recall", r#"{"query":"release"}"#);

    assert!(request.authorization.is_none());
    assert_eq!(request.path, "/memory/recall");
}
