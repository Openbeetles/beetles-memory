use bm_adapter::{AdapterErrorKey, AdapterOperation, TransportKind};
use bm_http::{route_specs, HttpMethod, RouteAuth, RouteBodyMode};

#[test]
fn route_catalog_declares_method_body_auth_and_profile_gate() {
    let routes = route_specs();
    assert_eq!(routes.len(), 12);
    let capabilities = routes
        .iter()
        .find(|route| route.path == "/memory/profile/capabilities")
        .expect("capabilities route");
    assert_eq!(capabilities.method, HttpMethod::Get);
    assert_eq!(capabilities.operation, AdapterOperation::Capabilities);
    assert_eq!(capabilities.transport, TransportKind::Http);
    assert!(matches!(capabilities.body, RouteBodyMode::None));
    assert!(matches!(capabilities.auth, RouteAuth::TokenOrLoopback));
    assert!(capabilities.profile_gate_required);

    let webhook = routes
        .iter()
        .find(|route| route.path == "/webhook/write-candidate")
        .expect("webhook route");
    assert_eq!(webhook.method, HttpMethod::Post);
    assert_eq!(webhook.operation, AdapterOperation::Write);
    assert_eq!(webhook.transport, TransportKind::Webhook);
    assert!(matches!(webhook.auth, RouteAuth::WebhookSignature));
    assert!(matches!(webhook.body, RouteBodyMode::Json { max_bytes } if max_bytes <= 64 * 1024));
}

#[test]
fn http_adapter_exposes_stable_error_keys() {
    assert_eq!(bm_http::invalid_json_error(), AdapterErrorKey::InvalidJson);
    assert_eq!(bm_http::unauthorized_error(), AdapterErrorKey::Unauthorized);
    assert_eq!(
        bm_http::duplicate_idempotency_error(),
        AdapterErrorKey::Duplicated
    );
    assert_eq!(
        bm_http::payload_too_large_error(),
        AdapterErrorKey::PayloadTooLarge
    );
}

#[test]
fn http_crate_manifest_has_no_direct_core_or_store_dependency() {
    let manifest = std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")))
        .expect("manifest");
    let dependencies = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or_default()
        .split('[')
        .next()
        .unwrap_or_default();
    assert!(!dependencies.contains("bm-core"));
    assert!(!dependencies.contains("bm-store"));
    assert!(dependencies.contains("bm-adapter"));
}
