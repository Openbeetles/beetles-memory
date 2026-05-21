#![cfg(feature = "server-std")]

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{handle_http_request, HttpMethod, HttpRuntimeRequest};
use bm_sdk::{MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId, StoreBackendKind};
use serde_json::Value;

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        profile: ProfileId::ServerLinuxDevFull,
        identity: EntryIdentity {
            agent_id: "http-console-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "console".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: EntryStoreConfig {
            backend: StoreBackendKind::InMemory,
            data_path: None,
            fsync: false,
        },
        transports: EntryTransportConfig::all_disabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

#[test]
fn console_http_routes_are_served_outside_memory_operation_routes() {
    let runtime = runtime();

    let overview = handle_http_request(&runtime, HttpRuntimeRequest::get("/console/overview"))
        .expect("overview");
    assert_eq!(overview.status_code, 200);
    let overview: Value = serde_json::from_str(&overview.body).expect("overview json");
    assert_eq!(overview["status"], "accepted");
    assert_eq!(
        overview["overview"]["runtimeShape"]["shell"],
        "HTTP console"
    );
    assert!(overview["overview"]["systemInfo"]["name"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(overview["overview"]["systemInfo"]["cpu"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(overview["overview"]["systemInfo"]["memory"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(overview["overview"]["systemInfo"]["timeUnixSecs"]
        .as_u64()
        .is_some_and(|value| value > 0));
    assert!(overview["overview"]["storage"]["value"]
        .as_str()
        .is_some_and(|value| value.contains(" / ")));
    let capabilities = overview["overview"]["capabilities"]
        .as_array()
        .expect("capabilities");
    assert!(capabilities
        .iter()
        .any(|row| row["title"] == "Write governance"));
    assert!(capabilities
        .iter()
        .any(|row| row["title"] == "Soul and subject memory"));
    assert!(capabilities
        .iter()
        .any(|row| row["title"] == "Device allowlist"));
    let kernel = overview["overview"]["kernel"].as_array().expect("kernel");
    assert!(kernel.iter().any(|row| row["label"] == "Profile"));
    assert!(kernel.iter().any(|row| row["label"] == "Store backend"));
    assert!(kernel.iter().any(|row| row["label"] == "Console shell"));

    let transports = handle_http_request(&runtime, HttpRuntimeRequest::get("/console/transports"))
        .expect("transports");
    assert_eq!(transports.status_code, 200);
    assert!(transports.body.contains("\"transports\""));
}

#[test]
fn console_http_device_keys_are_only_returned_on_create_or_rotate() {
    let runtime = runtime();

    let created = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/console/devices",
            r#"{"deviceId":"edge-node-01","label":"Edge node"}"#,
        ),
    )
    .expect("create");
    assert_eq!(created.status_code, 200);
    let created: Value = serde_json::from_str(&created.body).expect("created json");
    let app_key_once = created["appKeyOnce"].as_str().expect("app key once");
    assert!(app_key_once.starts_with("bm-api-"));
    assert_ne!(
        created["device"]["appKeyFingerprint"].as_str(),
        Some(app_key_once)
    );

    let listed = handle_http_request(&runtime, HttpRuntimeRequest::get("/console/devices"))
        .expect("devices");
    assert_eq!(listed.status_code, 200);
    assert!(!listed.body.contains(app_key_once));

    let rotated = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json("/console/devices/edge-node-01/rotate-key", "{}"),
    )
    .expect("rotate");
    assert_eq!(rotated.status_code, 200);
    let rotated: Value = serde_json::from_str(&rotated.body).expect("rotated json");
    assert!(rotated["appKeyOnce"]
        .as_str()
        .expect("rotated app key")
        .starts_with("bm-api-"));
}

#[test]
fn console_http_updates_transport_and_device_state() {
    let runtime = runtime();

    let transport = handle_http_request(
        &runtime,
        HttpRuntimeRequest::patch_json(
            "/console/transports/http",
            r#"{"enabled":true,"endpoint":"127.0.0.1:8718"}"#,
        ),
    )
    .expect("transport patch");
    assert_eq!(transport.status_code, 200);
    assert!(transport.body.contains("\"endpoint\":\"127.0.0.1:8718\""));

    let device = handle_http_request(
        &runtime,
        HttpRuntimeRequest::patch_json(
            "/console/devices/http-console-agent",
            r#"{"status":"disabled"}"#,
        ),
    )
    .expect("device patch");
    assert_eq!(device.status_code, 200);
    assert!(device.body.contains("\"status\":\"disabled\""));
}

#[test]
fn console_overview_reflects_real_memory_operations() {
    let runtime = runtime();

    let before = handle_http_request(&runtime, HttpRuntimeRequest::get("/console/overview"))
        .expect("overview before");
    let before: Value = serde_json::from_str(&before.body).expect("overview before json");
    assert_eq!(before["overview"]["writesToday"]["value"], "0");

    let write = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/write",
            r#"{"name":"release_patch_flow","topic":"release_patch_flow","title":"Release patch flow","summary":"Patch the release and verify the result","content":"1. inspect release diff\n2. patch rollback guards\n3. verify logs","source":"task_learning"}"#,
        ),
    )
    .expect("write");
    assert_eq!(write.status_code, 200, "{}", write.body);

    let recall = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/recall",
            r#"{"query":"release_patch_flow","limit":4}"#,
        ),
    )
    .expect("recall");
    assert_eq!(recall.status_code, 200, "{}", recall.body);

    let after = handle_http_request(&runtime, HttpRuntimeRequest::get("/console/overview"))
        .expect("overview after");
    let after: Value = serde_json::from_str(&after.body).expect("overview after json");
    assert_eq!(after["overview"]["writesToday"]["value"], "1");
    assert_eq!(after["overview"]["recall"]["value"], "100.0%");
    assert!(after["overview"]["recentEvents"]
        .as_array()
        .expect("events")
        .iter()
        .any(|event| event["text"]
            .as_str()
            .is_some_and(|text| text.contains("Memory write accepted"))));
}

#[test]
fn http_parser_accepts_delete_for_console_skill_routes() {
    let runtime = runtime();
    let response = handle_http_request(
        &runtime,
        HttpRuntimeRequest {
            method: HttpMethod::Delete,
            path: "/console/skills/runtime_skill__missing".to_string(),
            body: String::new(),
            request_id: "http-delete-req".to_string(),
            idempotency_key: "http-delete-idem".to_string(),
            audit_id: "http-delete-audit".to_string(),
            authenticated: true,
        },
    )
    .expect("delete");
    assert_eq!(response.status_code, 404);
}

#[test]
fn console_http_skill_routes_support_crud_without_store_shortcut() {
    let runtime = runtime();

    let created = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/console/skills",
            r#"{"title":"Release guard","topic":"release","summary":"Check artifacts before publishing.","procedure":"1. run gates\n2. inspect artifacts\n3. dry run publish","citations":["http-test"]}"#,
        ),
    )
    .expect("create");
    assert_eq!(created.status_code, 200, "{}", created.body);

    let list =
        handle_http_request(&runtime, HttpRuntimeRequest::get("/console/skills")).expect("list");
    assert_eq!(list.status_code, 200);
    assert!(list.body.contains("Release guard"));
    assert!(list.body.contains("user_provided"));

    let detail = handle_http_request(
        &runtime,
        HttpRuntimeRequest::get("/console/skills/runtime_skill__release"),
    )
    .expect("detail");
    assert_eq!(detail.status_code, 200, "{}", detail.body);
    assert!(detail.body.contains("run gates"));

    let edited = handle_http_request(
        &runtime,
        HttpRuntimeRequest::patch_json(
            "/console/skills/runtime_skill__release",
            r#"{"title":"Release guard","topic":"release","summary":"Check artifacts and changelog before publishing.","procedure":"1. run gates\n2. inspect artifacts\n3. inspect changelog","citations":["http-test-edit"]}"#,
        ),
    )
    .expect("edit");
    assert_eq!(edited.status_code, 200, "{}", edited.body);

    let disabled = handle_http_request(
        &runtime,
        HttpRuntimeRequest::patch_json(
            "/console/skills/runtime_skill__release/enabled",
            r#"{"enabled":false}"#,
        ),
    )
    .expect("disable");
    assert_eq!(disabled.status_code, 200, "{}", disabled.body);

    let deleted = handle_http_request(
        &runtime,
        HttpRuntimeRequest::delete("/console/skills/runtime_skill__release"),
    )
    .expect("delete");
    assert_eq!(deleted.status_code, 200, "{}", deleted.body);

    let detail_after_delete = handle_http_request(
        &runtime,
        HttpRuntimeRequest::get("/console/skills/runtime_skill__release"),
    )
    .expect("detail after delete");
    assert_eq!(detail_after_delete.status_code, 404);
}
