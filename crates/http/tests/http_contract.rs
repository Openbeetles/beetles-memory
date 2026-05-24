use bm_adapter::{AdapterErrorKey, AdapterOperation, TransportKind};
use bm_http::{console_route_specs, route_specs, HttpMethod, RouteAuth, RouteBodyMode};

#[test]
fn route_catalog_declares_method_body_auth_and_profile_gate() {
    let routes = route_specs();
    assert_eq!(routes.len(), 10);
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
}

#[test]
fn console_route_catalog_is_separate_from_memory_operations() {
    let routes = console_route_specs();
    assert!(routes
        .iter()
        .any(|route| route.method == HttpMethod::Get && route.path == "/console/overview"));
    assert!(routes
        .iter()
        .any(|route| route.method == HttpMethod::Get && route.path == "/console/skills"));
    assert!(routes.iter().any(|route| {
        route.method == HttpMethod::Delete && route.path == "/console/skills/{name}"
    }));
    assert!(routes.iter().any(|route| {
        route.method == HttpMethod::Patch && route.path == "/console/transports/{id}"
    }));
    for (method, path) in [
        (HttpMethod::Get, "/console/capabilities"),
        (HttpMethod::Get, "/console/ollama-transparent/status"),
        (HttpMethod::Post, "/console/ollama-transparent/preflight"),
        (HttpMethod::Post, "/console/ollama-transparent/enable"),
        (HttpMethod::Post, "/console/ollama-transparent/disable"),
        (HttpMethod::Post, "/console/ollama-transparent/open-app"),
    ] {
        assert!(
            routes
                .iter()
                .any(|route| route.method == method && route.path == path),
            "missing console route {method:?} {path}"
        );
    }
    assert!(routes
        .iter()
        .all(|route| matches!(route.auth, RouteAuth::TokenOrLoopback)));
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
