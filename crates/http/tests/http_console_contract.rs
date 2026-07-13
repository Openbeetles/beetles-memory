#![cfg(feature = "server-std")]

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryStoreConfig, EntryTransportConfig,
};
use bm_http::{
    console_route_specs, handle_http_request, handle_http_request_with_console,
    HttpConsoleServices, HttpMethod, HttpRuntimeRequest,
};
use bm_ollama_transparent::{
    DisableOllamaTransparentRequest, EnableOllamaTransparentRequest, GatewayFrontReport,
    ManagedRunnerReport, OllamaAppReport, OllamaTransparentController,
    OllamaTransparentPreflightReport, OllamaTransparentState, OllamaTransparentStatus,
    OllamaTransparentTransitionReport, PortBindingReport, PortOwnerKind, ProcessActionReport,
    TransitionOutcome, TransitionStep, TransitionStepReport,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryWriteRequest,
    PressureLevel, ProfileId, RuntimeLifecycleModeInput, RuntimeSkillWrite,
    RuntimeSkillWriteSource, StoreBackendKind,
};
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

fn runtime_with_file_store(path: &Path) -> EntryRuntime {
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
            backend: StoreBackendKind::File,
            data_path: Some(path.to_path_buf()),
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

fn seed_memory_runtime_activity(runtime: &EntryRuntime) {
    runtime
        .runtime()
        .write(MemoryWriteRequest::Procedural {
            writes: vec![RuntimeSkillWrite {
                name: "transparent_ollama_metrics".to_string(),
                topic: "transparent ollama metrics".to_string(),
                title: "Transparent Ollama metrics".to_string(),
                summary: "Transparent Ollama memory activity must be visible in overview."
                    .to_string(),
                content:
                    "1. read transparent Ollama store events\n2. merge them into Console Overview\n3. report writes and projection hits from the shared telemetry stream"
                        .to_string(),
                citations: vec!["http console overview contract".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 1_800_000_000,
            }],
            source: RuntimeSkillWriteSource::Manual,
        })
        .expect("write");
    runtime
        .runtime()
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "How should transparent Ollama metrics appear?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project");
}

fn test_store_dir(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("bm-http-{label}-{nanos}"))
}

#[derive(Default)]
struct MockOllamaTransparentController {
    calls: Mutex<Vec<String>>,
}

impl MockOllamaTransparentController {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("mock calls").clone()
    }

    fn record(&self, call: impl Into<String>) {
        self.calls.lock().expect("mock calls").push(call.into());
    }
}

impl OllamaTransparentController for MockOllamaTransparentController {
    fn preflight(&self) -> bm_ollama_transparent::Result<OllamaTransparentPreflightReport> {
        self.record("preflight");
        Ok(mock_preflight_report())
    }

    fn enable(
        &self,
        request: EnableOllamaTransparentRequest,
    ) -> bm_ollama_transparent::Result<OllamaTransparentTransitionReport> {
        self.record(format!(
            "enable:open_app={:?}:allow_stop={}",
            request.open_app, request.allow_stop_official_ollama
        ));
        Ok(mock_transition_report(
            OllamaTransparentState::Disabled,
            OllamaTransparentState::Active,
            TransitionStep::StartTransparentFront,
        ))
    }

    fn disable(
        &self,
        request: DisableOllamaTransparentRequest,
    ) -> bm_ollama_transparent::Result<OllamaTransparentTransitionReport> {
        self.record(format!(
            "disable:restore={:?}",
            request.restore_official_app
        ));
        Ok(mock_transition_report(
            OllamaTransparentState::Active,
            OllamaTransparentState::Disabled,
            TransitionStep::StopTransparentFront,
        ))
    }

    fn status(&self) -> bm_ollama_transparent::Result<OllamaTransparentStatus> {
        self.record("status");
        Ok(mock_status_report())
    }

    fn open_app(&self) -> bm_ollama_transparent::Result<ProcessActionReport> {
        self.record("open_app");
        Ok(ProcessActionReport::ok("open_official_app"))
    }
}

fn mock_status_report() -> OllamaTransparentStatus {
    let public_port = PortBindingReport::owned(
        "127.0.0.1:11434".parse().expect("public bind"),
        PortOwnerKind::BeetleMemoryTransparentFront,
        bm_ollama_transparent::ObservedProcess::new(11434, "bm-llm-gateway", "/tmp/bm-llm-gateway"),
    );
    OllamaTransparentStatus {
        state: OllamaTransparentState::Active,
        public_port: public_port.clone(),
        upstream_port: PortBindingReport::owned(
            "127.0.0.1:11435".parse().expect("upstream bind"),
            PortOwnerKind::ManagedOllamaRunner,
            bm_ollama_transparent::ObservedProcess::new(
                11435,
                "bm-real-ollama",
                "/tmp/bm-real-ollama",
            ),
        ),
        app: OllamaAppReport {
            bundle_path: "/Applications/Ollama.app".into(),
            allow_stop_official_ollama: false,
            open_app_after_enable: true,
            restore_official_after_disable: true,
            last_action: None,
        },
        managed_runner: mock_runner_report(),
        gateway_front: GatewayFrontReport::from_public_port(&public_port),
        last_transition: None,
    }
}

fn mock_preflight_report() -> OllamaTransparentPreflightReport {
    OllamaTransparentPreflightReport {
        accepted: true,
        resulting_state: OllamaTransparentState::Disabled,
        public_port: PortBindingReport::empty("127.0.0.1:11434".parse().expect("public bind")),
        upstream_port: PortBindingReport::empty("127.0.0.1:11435".parse().expect("upstream bind")),
        managed_runner: mock_runner_report(),
        stop_plan: None,
        blockers: Vec::new(),
    }
}

fn mock_runner_report() -> ManagedRunnerReport {
    ManagedRunnerReport::installed(
        "/Applications/Ollama.app/Contents/Resources/ollama".into(),
        "/tmp/beetle-memory/ollama-runner".into(),
        Some("fnv1a64:test".to_string()),
    )
}

fn mock_transition_report(
    from_state: OllamaTransparentState,
    to_state: OllamaTransparentState,
    step: TransitionStep,
) -> OllamaTransparentTransitionReport {
    OllamaTransparentTransitionReport {
        from_state,
        to_state,
        outcome: TransitionOutcome::Completed,
        steps: vec![TransitionStepReport::ok(step)],
        failing_step: None,
        rollback: None,
    }
}

#[test]
fn console_ollama_transparent_routes_are_registered_as_thin_console_routes() {
    let routes = console_route_specs();

    for (method, path) in [
        (HttpMethod::Get, "/console/capabilities"),
        (HttpMethod::Get, "/console/workbench/api-map"),
        (HttpMethod::Get, "/console/workbench/report"),
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
}

#[test]
fn console_workbench_api_map_route_exposes_entry_owned_report_apis() {
    let runtime = runtime();

    let response = handle_http_request(
        &runtime,
        HttpRuntimeRequest::get("/console/workbench/api-map"),
    )
    .expect("workbench api map");

    assert_eq!(response.status_code, 200, "{}", response.body);
    let body: Value = serde_json::from_str(&response.body).expect("workbench json");
    assert_eq!(body["status"], "accepted");
    let surfaces = body["workbench"]["surfaces"].as_array().expect("surfaces");
    assert_eq!(surfaces.len(), 8);
    assert!(surfaces
        .iter()
        .any(|surface| surface["reportApi"] == "sdk.project.subject_projection"));
    let facet_surface = surfaces
        .iter()
        .find(|surface| surface["surfaceId"] == "facet_inspector")
        .expect("facet surface");
    assert_eq!(facet_surface["reportApi"], "sdk.recall.facet_index_report");
    assert_eq!(facet_surface["privateRawAllowed"], false);
    assert!(surfaces
        .iter()
        .all(|surface| surface["privateRawAllowed"] == false));
    let missing = body["workbench"]["missingReportApis"]
        .as_array()
        .expect("missing");
    #[cfg(feature = "nonproduction-replay-harness")]
    assert!(missing.is_empty());
    #[cfg(not(feature = "nonproduction-replay-harness"))]
    assert_eq!(missing.len(), 1);
}

#[test]
fn console_workbench_report_route_exposes_runtime_report_summaries() {
    let runtime = runtime();

    let response = handle_http_request(
        &runtime,
        HttpRuntimeRequest::get("/console/workbench/report"),
    )
    .expect("workbench report");

    assert_eq!(response.status_code, 200, "{}", response.body);
    let body: Value = serde_json::from_str(&response.body).expect("workbench report json");
    assert_eq!(body["status"], "accepted");
    let report = &body["workbenchReport"];
    assert_eq!(
        report["apiMap"]["surfaces"]
            .as_array()
            .expect("surfaces")
            .len(),
        8
    );
    assert_eq!(report["facetInspector"]["status"]["status"], "ready");
    assert_eq!(report["facetInspector"]["reportOnly"], true);
    assert_eq!(report["facetInspector"]["directMutationAllowed"], false);
    assert_eq!(
        report["facetInspector"]["auditMarkdownFormat"],
        "obsidian-style-facet-audit-markdown"
    );
    #[cfg(feature = "nonproduction-replay-harness")]
    assert_eq!(report["benchmarkWall"]["report"]["passed"], true);
    #[cfg(not(feature = "nonproduction-replay-harness"))]
    {
        assert!(report["benchmarkWall"]["report"].is_null());
        assert_eq!(
            report["benchmarkWall"]["status"]["reason"],
            "replay_harness_not_compiled"
        );
    }
    assert_eq!(report["projectionInspector"]["rawPrivateViolationCount"], 0);
    assert_eq!(
        report["projectionInspector"]["disclosureIntegrityPassed"],
        true
    );
    assert_eq!(report["vaultMigration"]["preflightPassed"], true);
}

#[test]
fn console_ollama_transparent_routes_delegate_to_controller_trait() {
    let runtime = runtime();
    let controller = MockOllamaTransparentController::default();
    let services = HttpConsoleServices::with_ollama_transparent(&controller);

    let capabilities = handle_http_request_with_console(
        &runtime,
        HttpRuntimeRequest::get("/console/capabilities"),
        services,
    )
    .expect("capabilities");
    assert_eq!(capabilities.status_code, 200, "{}", capabilities.body);
    let capabilities: Value = serde_json::from_str(&capabilities.body).expect("capabilities json");
    assert_eq!(capabilities["status"], "accepted");
    assert_eq!(
        capabilities["capabilities"]["schema"],
        "beetle-memory.console.capabilities.v1"
    );
    assert_eq!(
        capabilities["capabilities"]["features"]["ollamaTransparentApp"]["visible"],
        true
    );
    assert_eq!(
        capabilities["capabilities"]["features"]["ollamaTransparentApp"]["owner"],
        "desktop-shell"
    );
    assert_eq!(
        capabilities["capabilities"]["features"]["ollamaTransparentApp"]["routes"]["status"],
        "/console/ollama-transparent/status"
    );

    let status = handle_http_request_with_console(
        &runtime,
        HttpRuntimeRequest::get("/console/ollama-transparent/status"),
        services,
    )
    .expect("status");
    assert_eq!(status.status_code, 200, "{}", status.body);
    let status: Value = serde_json::from_str(&status.body).expect("status json");
    assert_eq!(status["status"], "accepted");
    assert_eq!(status["ollamaTransparent"]["state"], "Active");
    assert_eq!(
        status["ollamaTransparent"]["gatewayFront"]["expectedOwner"],
        "BeetleMemoryTransparentFront"
    );

    let preflight = handle_http_request_with_console(
        &runtime,
        HttpRuntimeRequest::post_json("/console/ollama-transparent/preflight", "{}"),
        services,
    )
    .expect("preflight");
    assert_eq!(preflight.status_code, 200, "{}", preflight.body);
    let preflight: Value = serde_json::from_str(&preflight.body).expect("preflight json");
    assert_eq!(preflight["status"], "accepted");
    assert_eq!(preflight["preflight"]["accepted"], true);

    let enable = handle_http_request_with_console(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/console/ollama-transparent/enable",
            r#"{"openApp":false,"allowStopOfficialOllama":true}"#,
        ),
        services,
    )
    .expect("enable");
    assert_eq!(enable.status_code, 200, "{}", enable.body);
    let enable: Value = serde_json::from_str(&enable.body).expect("enable json");
    assert_eq!(enable["transition"]["outcome"], "Completed");
    assert_eq!(enable["transition"]["toState"], "Active");

    let disable = handle_http_request_with_console(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/console/ollama-transparent/disable",
            r#"{"restoreOfficialApp":false}"#,
        ),
        services,
    )
    .expect("disable");
    assert_eq!(disable.status_code, 200, "{}", disable.body);
    let disable: Value = serde_json::from_str(&disable.body).expect("disable json");
    assert_eq!(disable["transition"]["outcome"], "Completed");
    assert_eq!(disable["transition"]["toState"], "Disabled");

    let open_app = handle_http_request_with_console(
        &runtime,
        HttpRuntimeRequest::post_json("/console/ollama-transparent/open-app", "{}"),
        services,
    )
    .expect("open app");
    assert_eq!(open_app.status_code, 200, "{}", open_app.body);
    let open_app: Value = serde_json::from_str(&open_app.body).expect("open app json");
    assert_eq!(open_app["action"]["action"], "open_official_app");
    assert_eq!(open_app["action"]["ok"], true);

    assert_eq!(
        controller.calls(),
        vec![
            "status",
            "preflight",
            "enable:open_app=Some(false):allow_stop=true",
            "disable:restore=Some(false)",
            "open_app",
        ]
    );
}

#[test]
fn console_capabilities_hide_ollama_transparent_when_controller_is_not_wired() {
    let runtime = runtime();
    let response = handle_http_request(&runtime, HttpRuntimeRequest::get("/console/capabilities"))
        .expect("capabilities should be available");

    assert_eq!(response.status_code, 200, "{}", response.body);
    let body: Value = serde_json::from_str(&response.body).expect("capabilities json");
    assert_eq!(body["status"], "accepted");
    assert_eq!(
        body["capabilities"]["features"]["ollamaTransparentApp"]["visible"],
        false
    );
    assert_eq!(
        body["capabilities"]["features"]["ollamaTransparentApp"]["reason"],
        "DesktopShellOnly"
    );
    assert!(
        body["capabilities"]["features"]["ollamaTransparentApp"]["routes"]
            .as_object()
            .is_some_and(|routes| routes.is_empty()),
        "{}",
        response.body
    );
}

#[test]
fn console_ollama_transparent_status_and_actions_fail_when_controller_is_not_wired() {
    let runtime = runtime();

    for request in [
        HttpRuntimeRequest::get("/console/ollama-transparent/status"),
        HttpRuntimeRequest::post_json("/console/ollama-transparent/preflight", "{}"),
        HttpRuntimeRequest::post_json(
            "/console/ollama-transparent/enable",
            r#"{"openApp":false,"allowStopOfficialOllama":true}"#,
        ),
        HttpRuntimeRequest::post_json(
            "/console/ollama-transparent/disable",
            r#"{"restoreOfficialApp":false}"#,
        ),
        HttpRuntimeRequest::post_json("/console/ollama-transparent/open-app", "{}"),
    ] {
        let error = handle_http_request(&runtime, request)
            .expect_err("missing controller should reject mutating transparent routes");
        assert!(
            error
                .to_string()
                .contains("ollama transparent controller is not configured"),
            "{error}"
        );
    }
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
    let memory_context = overview["overview"]["memoryContext"]
        .as_array()
        .expect("memory context");
    assert!(memory_context
        .iter()
        .any(|row| row["label"] == "Owner" && row["value"] == "owner-default"));
    assert!(memory_context
        .iter()
        .any(|row| row["label"] == "Chat" && row["value"] == "chat-1"));

    let transports = handle_http_request(&runtime, HttpRuntimeRequest::get("/console/transports"))
        .expect("transports");
    assert_eq!(transports.status_code, 200);
    assert!(transports.body.contains("\"transports\""));

    let llm_gateway =
        handle_http_request(&runtime, HttpRuntimeRequest::get("/console/llm-gateway"))
            .expect("llm gateway");
    assert_eq!(llm_gateway.status_code, 200);
    let llm_gateway: Value = serde_json::from_str(&llm_gateway.body).expect("llm gateway json");
    assert_eq!(llm_gateway["status"], "accepted");
    assert_eq!(
        llm_gateway["llmGateway"]["openaiBaseUrl"],
        "http://127.0.0.1:8787/v1"
    );
    assert_eq!(
        llm_gateway["llmGateway"]["ollamaBaseUrl"],
        "http://127.0.0.1:8787/api"
    );
    assert_eq!(
        llm_gateway["llmGateway"]["providerCapabilitiesUrl"],
        "http://127.0.0.1:8787/v1/bm/provider-capabilities"
    );
    assert_eq!(
        llm_gateway["llmGateway"]["mcpStreamableHttpUrl"],
        "http://127.0.0.1:8788/mcp"
    );
    assert!(llm_gateway["llmGateway"]
        .as_object()
        .expect("llm gateway object")
        .get("sharedRuntime")
        .is_none());
    let protocols = llm_gateway["llmGateway"]["protocols"]
        .as_array()
        .expect("protocols");
    assert!(protocols
        .iter()
        .any(|protocol| protocol["id"] == "openai-compatible"));
    assert!(protocols
        .iter()
        .any(|protocol| protocol["id"] == "ollama-native"));
    assert!(protocols
        .iter()
        .any(|protocol| protocol["id"] == "mcp-streamable-http"));
    assert!(llm_gateway["llmGateway"]["ruleExports"]
        .as_array()
        .expect("rule exports")
        .iter()
        .any(|rule| rule["target"] == "continue"
            && rule["command"]
                .as_str()
                .is_some_and(|command| command.contains("--target continue"))));
    assert!(llm_gateway["llmGateway"]["smokeChecks"]
        .as_array()
        .expect("smoke checks")
        .iter()
        .any(|check| check["id"] == "provider-capabilities"
            && check["command"]
                .as_str()
                .is_some_and(|command| command.contains("/v1/bm/provider-capabilities"))));

    let smoke_run = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/console/llm-gateway/smoke-checks/provider-capabilities/run",
            "{}",
        ),
    )
    .expect("smoke run");
    assert_eq!(smoke_run.status_code, 200);
    let smoke_run: Value = serde_json::from_str(&smoke_run.body).expect("smoke run json");
    assert_eq!(smoke_run["status"], "accepted");
    assert_eq!(smoke_run["result"]["id"], "provider-capabilities");
    assert!(smoke_run["result"]["command"]
        .as_str()
        .is_some_and(|command| command.contains("/v1/bm/provider-capabilities")));

    let unknown_smoke = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/console/llm-gateway/smoke-checks/not-a-smoke-check/run",
            "{}",
        ),
    )
    .expect("unknown smoke");
    assert_eq!(unknown_smoke.status_code, 404);
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
            r#"{"name":"release_patch_flow","topic":"release_patch_flow","title":"Release patch flow","summary":"Patch the release and verify the result","content":"1. inspect release diff\n2. patch rollback guards\n3. verify logs","source":"manual"}"#,
        ),
    )
    .expect("write");
    assert_eq!(write.status_code, 200, "{}", write.body);

    let duplicate_write = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/write",
            r#"{"name":"release_patch_flow","topic":"release_patch_flow","title":"Release patch flow","summary":"Patch the release and verify the result","content":"1. inspect release diff\n2. patch rollback guards\n3. verify logs","source":"manual"}"#,
        ),
    )
    .expect("duplicate write");
    assert_eq!(duplicate_write.status_code, 200, "{}", duplicate_write.body);

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
fn console_overview_aggregates_extra_memory_event_store_paths() {
    let transparent_store_path = test_store_dir("transparent-ollama-events");
    let transparent_runtime = runtime_with_file_store(&transparent_store_path);
    seed_memory_runtime_activity(&transparent_runtime);

    let runtime = runtime();
    let event_store_paths = vec![transparent_store_path];
    let services = HttpConsoleServices::none().with_memory_event_store_paths(&event_store_paths);

    let overview = handle_http_request_with_console(
        &runtime,
        HttpRuntimeRequest::get("/console/overview"),
        services,
    )
    .expect("overview");

    assert_eq!(overview.status_code, 200, "{}", overview.body);
    let body: Value = serde_json::from_str(&overview.body).expect("overview json");
    assert_eq!(body["overview"]["writesToday"]["value"], "1");
    assert_eq!(body["overview"]["recall"]["value"], "100.0%");
    assert_eq!(
        body["overview"]["recall"]["desc"],
        "1 recall requests / 1 with hits"
    );
    assert!(body["overview"]["projection"]["desc"]
        .as_str()
        .unwrap_or_default()
        .starts_with("1 conversations received memory context"));
    assert_ne!(body["overview"]["projection"]["value"], "0");
    assert!(
        body["overview"]["runtimeBudget"]["projectionRenderMaxChars"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
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
fn console_http_skill_routes_edit_runtime_skills_without_store_shortcut() {
    let runtime = runtime();

    let create_forbidden = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/console/skills",
            r#"{"title":"Release guard","topic":"release","summary":"Check artifacts before publishing.","procedure":"1. run gates\n2. inspect artifacts\n3. dry run publish","citations":["http-test"]}"#,
        ),
    )
    .expect("create forbidden");
    assert_eq!(
        create_forbidden.status_code, 405,
        "{}",
        create_forbidden.body
    );

    let seeded = handle_http_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/write",
            r#"{"name":"runtime_skill__release","topic":"release","title":"Release guard","summary":"Check artifacts before publishing.","content":"1. run gates\n2. inspect artifacts\n3. dry run publish","source":"manual","citations":["http-test"]}"#,
        ),
    )
    .expect("seed runtime skill");
    assert_eq!(seeded.status_code, 200, "{}", seeded.body);

    let list =
        handle_http_request(&runtime, HttpRuntimeRequest::get("/console/skills")).expect("list");
    assert_eq!(list.status_code, 200);
    assert!(list.body.contains("Release guard"));
    assert!(list.body.contains("runtimeLearned"));

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
