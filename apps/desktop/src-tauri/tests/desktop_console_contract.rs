use std::time::{SystemTime, UNIX_EPOCH};

use bm_desktop::{
    DesktopConsoleInvokeRequest, DesktopConsoleRequest, DesktopConsoleState,
    DesktopMemoryAuthority, DesktopRuntimeConfig,
};
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
use bm_sdk::{
    default_agent_subject_id, GovernedRuntimeSkillWriteInput, MemoryCapabilityPolicy,
    MemoryPrivacyClass, MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryWriteRequest,
    PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillCreationRef,
    RuntimeSkillOwningScope, RuntimeSkillWrite, RuntimeSkillWriteSource, StoreBackendConfig,
};
use serde_json::Value;

#[test]
fn desktop_console_serves_skills_without_http_listener() {
    let state = desktop_state("skills-list");

    let response = state
        .handle_console_request(DesktopConsoleRequest::get("/console/skills"))
        .unwrap();

    assert_eq!(response.status_code, 200);
    assert!(response.body.contains(r#""status":"accepted""#));
    assert!(response.body.contains(r#""skills""#));
}

#[test]
fn desktop_console_serves_ollama_transparent_status_without_404() {
    let state = desktop_state("ollama-transparent-status");

    let capabilities = state
        .handle_console_request(DesktopConsoleRequest::get("/console/capabilities"))
        .unwrap();
    assert_eq!(capabilities.status_code, 200, "{}", capabilities.body);
    let capabilities: Value = serde_json::from_str(&capabilities.body).expect("capabilities json");
    assert_eq!(
        capabilities["capabilities"]["features"]["ollamaTransparentApp"]["visible"],
        true
    );

    let response = state
        .handle_console_request(DesktopConsoleRequest::get(
            "/console/ollama-transparent/status",
        ))
        .unwrap();

    assert_eq!(response.status_code, 200, "{}", response.body);
    let body: Value = serde_json::from_str(&response.body).expect("status json");
    assert_eq!(body["status"], "accepted");
    assert!(body.get("ollamaTransparent").is_some(), "{}", response.body);
}

#[test]
fn desktop_console_mutates_skills_through_entry_runtime() {
    let state = desktop_state("skills-mutation");

    let create_forbidden = state
        .handle_console_request(DesktopConsoleRequest::post_json(
            "/console/skills",
            r#"{
              "title":"Desktop direct skill",
              "topic":"desktop_console",
              "summary":"Desktop commands must use the in-process entry runtime.",
              "procedure":"1. open the Tauri app\n2. call the shared console API\n3. verify the returned report",
              "citations":["desktop contract test"]
            }"#,
        ))
        .unwrap();
    assert_eq!(create_forbidden.status_code, 405);

    let seed_body = serde_json::json!({
        "name": "runtime_skill__desktop_console",
        "topic": "desktop_console",
        "title": "Desktop direct skill",
        "summary": "Desktop commands must use the in-process entry runtime.",
        "content": "1. open the Tauri app\n2. call the shared console API\n3. verify the returned report",
        "source": "manual",
        "citations": ["desktop contract test"],
        "owning_scope": {
            "kind": "subject",
            "mounted_subject_id": default_agent_subject_id("bm-desktop"),
        },
        "creation_ref": RuntimeSkillCreationRef::ReplayPromotion {
            candidate_ref: "desktop-test:runtime-skill".to_string(),
            verification_receipt_digest:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
        },
        "privacy_class": MemoryPrivacyClass::SharedWithSubject,
    })
    .to_string();
    let seed_request = DesktopConsoleRequest::try_from(DesktopConsoleInvokeRequest {
        method: "POST".to_string(),
        path: "/memory/write".to_string(),
        body: seed_body,
        idempotency_key: "desktop-console-runtime-skill-seed".to_string(),
    })
    .expect("desktop invoke request");
    let seeded = state.handle_console_request(seed_request).unwrap();
    assert_eq!(seeded.status_code, 200, "{}", seeded.body);

    let list = state
        .handle_console_request(DesktopConsoleRequest::get("/console/skills?query=desktop"))
        .unwrap();
    assert_eq!(list.status_code, 200);
    let list_body: Value = serde_json::from_str(&list.body).expect("skill list json");
    let locator = list_body["skills"]["skills"][0]["locator"].clone();
    let edit_body = serde_json::json!({
        "locator": locator,
        "title": "Desktop direct skill",
        "topic": "desktop_console",
        "summary": "Desktop commands must use the in-process entry runtime.",
        "procedure": "1. open the Tauri app\n2. call the shared console API\n3. verify the returned report\n4. keep edits inside runtime skill management",
        "editReason": "desktop_contract_edit",
    })
    .to_string();
    let mutation = state
        .handle_console_request(DesktopConsoleRequest::patch_json(
            "/console/skills",
            &edit_body,
        ))
        .unwrap();
    assert_eq!(mutation.status_code, 200, "{}", mutation.body);
    assert!(
        mutation.body.contains(r#""accepted":true"#),
        "{}",
        mutation.body
    );
    let mutation_body: Value = serde_json::from_str(&mutation.body).expect("edit mutation json");
    let edited_locator = mutation_body["mutation"]["currentLocator"].clone();

    let stale_edit = state
        .handle_console_request(DesktopConsoleRequest::patch_json(
            "/console/skills",
            &edit_body,
        ))
        .expect("stale edit response");
    assert_eq!(stale_edit.status_code, 409, "{}", stale_edit.body);

    let disable_body = serde_json::json!({
        "locator": edited_locator,
        "enabled": false,
    })
    .to_string();
    let disabled = state
        .handle_console_request(DesktopConsoleRequest::patch_json(
            "/console/skills/enabled",
            &disable_body,
        ))
        .expect("disable response");
    assert_eq!(disabled.status_code, 200, "{}", disabled.body);
    let disabled_body: Value = serde_json::from_str(&disabled.body).expect("disable mutation json");
    let disabled_locator = disabled_body["mutation"]["currentLocator"].clone();

    let retired = state
        .handle_console_request(DesktopConsoleRequest::post_json(
            "/console/skills/retire",
            disabled_locator.to_string(),
        ))
        .expect("retire response");
    assert_eq!(retired.status_code, 200, "{}", retired.body);
    let retired_body: Value = serde_json::from_str(&retired.body).expect("retire mutation json");
    let retired_locator = retired_body["mutation"]["currentLocator"].clone();

    let retired_detail = state
        .handle_console_request(DesktopConsoleRequest::post_json(
            "/console/skills/detail",
            retired_locator.to_string(),
        ))
        .expect("retired detail response");
    assert_eq!(retired_detail.status_code, 200, "{}", retired_detail.body);
    assert!(retired_detail.body.contains(r#""status":"retired""#));

    let list = state
        .handle_console_request(DesktopConsoleRequest::get("/console/skills?query=desktop"))
        .unwrap();
    assert_eq!(list.status_code, 200);
    assert!(list.body.contains("Desktop direct skill"));
    assert!(list.body.contains(r#""runtimeLearned":1"#));
}

#[test]
fn desktop_console_overview_includes_ollama_transparent_memory_store_events() {
    let data_dir = test_store_dir("ollama-transparent-overview");
    let transparent_runtime = runtime_for_store(data_dir.join("store"));
    seed_memory_runtime_activity(&transparent_runtime);
    let state = DesktopConsoleState::open(desktop_config(data_dir)).unwrap();

    let response = state
        .handle_console_request(DesktopConsoleRequest::get("/console/overview"))
        .unwrap();

    assert_eq!(response.status_code, 200, "{}", response.body);
    let body: Value = serde_json::from_str(&response.body).expect("overview json");
    assert_eq!(body["overview"]["writesToday"]["value"], "1");
    assert_eq!(body["overview"]["recall"]["value"], "0.0%");
    assert!(body["overview"]["projection"]["desc"]
        .as_str()
        .unwrap_or_default()
        .starts_with("1 conversations received memory context"));
    assert!(
        body["overview"]["runtimeBudget"]["projectionRenderMaxChars"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
}

#[test]
fn desktop_and_transparent_gateway_share_one_memory_authority() {
    let state = desktop_state("shared-memory-authority");
    let desktop = state.memory_authority();
    let transparent = &state.ollama_transparent_config().memory_authority;

    assert_eq!(transparent.owner_id, desktop.owner_id);
    assert_eq!(transparent.agent_id, desktop.agent_id);
    assert_eq!(transparent.channel, desktop.channel);
    assert_eq!(transparent.store_path, desktop.store_path);
}

#[test]
fn desktop_rejects_relative_gateway_and_store_paths() {
    let data_dir = test_store_dir("relative-path-rejection");
    let mut config = desktop_config(data_dir);
    config.gateway_binary_path = "bm-llm-gateway".into();
    assert!(DesktopConsoleState::open(config).is_err());

    let data_dir = test_store_dir("relative-store-rejection");
    let mut config = desktop_config(data_dir);
    config.memory.store_path = "store".into();
    assert!(DesktopConsoleState::open(config).is_err());
}

#[test]
fn desktop_tauri_bundle_declares_ollama_gateway_sidecar() {
    let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let config = std::fs::read_to_string(&config_path).expect("tauri config");
    let config: Value = serde_json::from_str(&config).expect("tauri config json");

    assert_eq!(
        config["build"]["beforeBuildCommand"],
        "node scripts/build-sidecars.mjs"
    );
    let external_bins = config["bundle"]["externalBin"]
        .as_array()
        .expect("externalBin array");
    assert!(external_bins
        .iter()
        .any(|entry| { entry.as_str() == Some("../../../target/release/bm-llm-gateway") }));
}

fn test_store_dir(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("bm-desktop-{label}-{nanos}"));
    std::fs::create_dir_all(&path).expect("desktop test data dir");
    std::fs::canonicalize(path).expect("canonical desktop test data dir")
}

fn desktop_state(label: &str) -> DesktopConsoleState {
    DesktopConsoleState::open(desktop_config(test_store_dir(label))).expect("desktop state")
}

fn desktop_config(data_dir: std::path::PathBuf) -> DesktopRuntimeConfig {
    DesktopRuntimeConfig {
        gateway_binary_path: std::env::current_exe().expect("desktop test executable"),
        memory: DesktopMemoryAuthority {
            owner_id: "local-owner".to_string(),
            agent_id: "bm-desktop".to_string(),
            channel: "desktop".to_string(),
            chat_id: "local-desktop".to_string(),
            store_path: data_dir.join("store"),
        },
        data_dir,
    }
}

fn runtime_for_store(path: std::path::PathBuf) -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "bm-desktop".to_string(),
            owner_id: "local-owner".to_string(),
        },
        scope: EntryScope {
            channel: "desktop".to_string(),
            chat_id: "local-desktop".to_string(),
        },
        store: StoreBackendConfig::file(path, ProfileId::DesktopMacosStandaloneMemory)
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_disabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 128 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

fn seed_memory_runtime_activity(runtime: &EntryRuntime) {
    runtime
        .runtime()
        .write(MemoryWriteRequest::Procedural {
            writes: vec![GovernedRuntimeSkillWriteInput {
                write: RuntimeSkillWrite {
                    name: "desktop_ollama_overview".to_string(),
                    topic: "desktop ollama overview".to_string(),
                    title: "Desktop Ollama overview".to_string(),
                    summary: "Desktop overview must include transparent Ollama memory events."
                        .to_string(),
                    content: "1. read the transparent Ollama memory store\n2. merge store events into the Desktop overview\n3. keep writes and projection hits on the shared metrics path"
                        .to_string(),
                    citations: vec!["desktop console overview contract".to_string()],
                    source_chat_id: Some("local-desktop".to_string()),
                    observed_at: 1_800_000_000,
                },
                creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
                    candidate_ref: "desktop-test:ollama-overview".to_string(),
                    verification_receipt_digest:
                        "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                            .to_string(),
                },
                privacy_class: MemoryPrivacyClass::SharedWithSubject,
            }],
            owning_scope: RuntimeSkillOwningScope::Subject {
                mounted_subject_id: default_agent_subject_id("bm-desktop"),
            },
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write");
    runtime
        .runtime()
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "How should Desktop overview count transparent Ollama?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");
}
