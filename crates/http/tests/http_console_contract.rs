#![cfg(feature = "server-std")]

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use bm_entry::{
    EntryAuthConfig, EntryGovernanceModelProtocol, EntryIdempotencyConfig, EntryIdentity,
    EntryRuntime, EntryRuntimeConfig, EntryScope, EntryTransportConfig,
    GovernanceModelConnectionProbe, GovernanceModelConnectionReport,
    ReqwestGovernanceModelConnectionProbe,
};
use bm_http::{
    console_route_specs, handle_http_in_process_request,
    handle_http_in_process_request_with_console, HttpConsoleServices, HttpMethod,
    HttpRuntimeRequest,
};
use bm_ollama_transparent::{
    DisableOllamaTransparentRequest, EnableOllamaTransparentRequest, GatewayFrontReport,
    ManagedRunnerReport, OllamaAppReport, OllamaTransparentController,
    OllamaTransparentPreflightReport, OllamaTransparentState, OllamaTransparentStatus,
    OllamaTransparentTransitionReport, PortBindingReport, PortOwnerKind, ProcessActionReport,
    TransitionOutcome, TransitionStep, TransitionStepReport,
};
use bm_sdk::{
    default_agent_subject_id, LongTermMemoryKind, MemoryCandidateContent,
    MemoryCandidateSemanticDecision, MemoryCandidateSemanticJudgment, MemoryCandidateTarget,
    MemoryCapabilityPolicy, MemoryEvidenceAuthority, MemoryPrivacyClass, MemoryPrivacyPolicy,
    MemoryProjectionRequest, MemorySemanticJudgmentSource, MemoryWriteCandidate,
    MemoryWriteRequest, PressureLevel, RuntimeLifecycleModeInput, RuntimeSkillCreationRef,
    StoreBackendConfig,
};
use serde_json::Value;

fn runtime() -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "http-console-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "console".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::in_memory(support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_disabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

fn runtime_skill_http_write_body(name: &str, summary: &str, content: &str) -> String {
    serde_json::json!({
        "name": name,
        "topic": "release",
        "title": "Release guard",
        "summary": summary,
        "content": content,
        "source": "manual",
        "citations": ["http-test"],
        "owning_scope": {
            "kind": "subject",
            "mounted_subject_id": default_agent_subject_id("http-console-agent"),
        },
        "creation_ref": RuntimeSkillCreationRef::ReplayPromotion {
            candidate_ref: format!("http-test:{name}"),
            verification_receipt_digest:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
        },
        "privacy_class": MemoryPrivacyClass::SharedWithSubject,
    })
    .to_string()
}

fn runtime_with_file_store(path: &Path) -> EntryRuntime {
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    EntryRuntime::open(EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "http-console-agent".to_string(),
            owner_id: "owner-default".to_string(),
        },
        scope: EntryScope {
            channel: "console".to_string(),
            chat_id: "chat-1".to_string(),
        },
        store: StoreBackendConfig::file(path, support::native_runtime_profile())
            .expect("store config")
            .with_fsync(false),
        transports: EntryTransportConfig::all_disabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 32 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    })
    .expect("entry runtime")
}

fn seed_memory_runtime_activity(runtime: &EntryRuntime) {
    let target = MemoryCandidateTarget::LongTermMemory {
        kind: LongTermMemoryKind::Fact,
        topic: "transparent ollama metrics".to_string(),
    };
    runtime
        .runtime()
        .write(MemoryWriteRequest::Candidates {
            candidates: vec![MemoryWriteCandidate {
                candidate_id: "transparent-ollama-metrics".to_string(),
                authority: MemoryEvidenceAuthority::UserAsserted,
                target: target.clone(),
                privacy: MemoryPrivacyClass::SharedWithSubject,
                content: MemoryCandidateContent::Text {
                    topic: "transparent ollama metrics".to_string(),
                    body: "Console metrics consume the SDK/Core event report.".to_string(),
                    keywords: vec!["console".to_string(), "metrics".to_string()],
                },
                evidence_refs: vec!["http console overview contract".to_string()],
                canonical_entities: Vec::new(),
                semantic_judgment: Some(MemoryCandidateSemanticJudgment {
                    source: MemorySemanticJudgmentSource::LlmGovernance,
                    decision: MemoryCandidateSemanticDecision::Accept,
                    governed_target: Some(target),
                    reason: "http_console_metrics_fixture".to_string(),
                }),
            }],
            runtime_skill_owning_scope: None,
        })
        .expect("write");
    runtime
        .runtime()
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
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

#[derive(Default)]
struct MockGovernanceModelProbe {
    urls: Mutex<Vec<String>>,
}

impl GovernanceModelConnectionProbe for MockGovernanceModelProbe {
    fn probe(
        &self,
        plan: &bm_entry::EntryGovernanceModelProbePlan,
    ) -> bm_sdk::Result<GovernanceModelConnectionReport> {
        self.urls.lock().expect("probe urls").push(plan.url.clone());
        Ok(GovernanceModelConnectionReport {
            status: "ready".to_string(),
            protocol: plan.protocol,
            model: plan.model.clone(),
            credential_used: plan.auth_mode.credential_env().is_some(),
            response_bytes: 42,
            duration_ms: 3,
            reason: "model_protocol_probe_succeeded".to_string(),
        })
    }
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
        gateway_executable: None,
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
        (HttpMethod::Get, "/console/governance-model"),
        (HttpMethod::Patch, "/console/governance-model"),
        (
            HttpMethod::Post,
            "/console/governance-model/test-connection",
        ),
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
fn governance_model_console_routes_persist_safe_config_and_probe_exact_protocol() {
    let runtime = runtime();
    let initial = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::get("/console/governance-model"),
    )
    .expect("initial config");
    let initial_body: Value = serde_json::from_str(&initial.body).expect("initial json");
    assert_eq!(initial_body["governanceModel"]["configured"], false);

    let raw_secret = "sk-must-never-appear";
    let save = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::patch_json(
            "/console/governance-model",
            serde_json::json!({
                "enabled": true,
                "protocol": "ollama_native",
                "endpoint": "http://127.0.0.1:11434/api",
                "model": "qwen3:8b",
                "authMode": {
                    "kind": "credential_env",
                    "credentialEnv": "BEETLE_MEMORY_LLM_API_KEY"
                },
                "requestTimeoutMs": 30000,
                "maxInputTokens": 8192,
                "maxOutputTokens": 1024
            })
            .to_string(),
        ),
    )
    .expect("save config");
    assert_eq!(save.status_code, 200, "{}", save.body);
    assert!(!save.body.contains(raw_secret));
    let body: Value = serde_json::from_str(&save.body).expect("save json");
    assert_eq!(body["governanceModel"]["protocol"], "ollama_native");
    assert_eq!(body["governanceModel"]["credentialConfigured"], true);

    let probe = MockGovernanceModelProbe::default();
    let tested = handle_http_in_process_request_with_console(
        &runtime,
        HttpRuntimeRequest::post_json("/console/governance-model/test-connection", "{}"),
        HttpConsoleServices::none().with_governance_model_probe(&probe),
    )
    .expect("test connection");
    assert_eq!(tested.status_code, 200, "{}", tested.body);
    let tested_body: Value = serde_json::from_str(&tested.body).expect("test json");
    assert_eq!(tested_body["result"]["status"], "ready");
    assert_eq!(tested_body["result"]["protocol"], "ollama_native");
    assert_eq!(
        probe.urls.lock().expect("probe urls").as_slice(),
        ["http://127.0.0.1:11434/api/chat"]
    );
    assert_eq!(
        tested_body["result"]["protocol"],
        serde_json::to_value(EntryGovernanceModelProtocol::OllamaNative).expect("protocol json")
    );
    assert!(!tested.body.contains(raw_secret));
}

#[test]
fn governance_model_console_patch_rejects_raw_key_shaped_credential_without_leaking_it() {
    let runtime = runtime();
    let raw_secret = "sk-live-pl1-secret-sentinel";
    let response = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::patch_json(
            "/console/governance-model",
            serde_json::json!({
                "enabled": true,
                "protocol": "open_ai_compatible",
                "endpoint": "https://example.invalid/v1",
                "model": "governance-model",
                "authMode": {
                    "kind": "credential_env",
                    "credentialEnv": raw_secret
                },
                "requestTimeoutMs": 30000,
                "maxInputTokens": 8192,
                "maxOutputTokens": 1024
            })
            .to_string(),
        ),
    )
    .expect("invalid config is a typed HTTP response");

    assert_eq!(response.status_code, 422, "{}", response.body);
    assert!(!response.body.contains(raw_secret));
    assert!(!format!("{response:?}").contains(raw_secret));
    assert!(!runtime.console_governance_model().configured);
}

#[test]
fn governance_model_probe_error_never_leaks_resolved_credential() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind probe fixture");
    let endpoint = format!("http://{}/v1", listener.local_addr().expect("fixture addr"));
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept probe request");
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).expect("read probe request");
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .expect("write probe rejection");
    });
    let runtime = runtime();
    let env_name = format!(
        "BEETLE_MEMORY_PL1_PROBE_TOKEN_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let raw_secret = "sk-probe-error-secret-sentinel";
    std::env::set_var(&env_name, raw_secret);
    let save = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::patch_json(
            "/console/governance-model",
            serde_json::json!({
                "enabled": true,
                "protocol": "open_ai_compatible",
                "endpoint": endpoint,
                "model": "governance-model",
                "authMode": {
                    "kind": "credential_env",
                    "credentialEnv": env_name
                },
                "requestTimeoutMs": 1000,
                "maxInputTokens": 8192,
                "maxOutputTokens": 1024
            })
            .to_string(),
        ),
    )
    .expect("save probe fixture config");
    assert_eq!(save.status_code, 200, "{}", save.body);

    let probe = ReqwestGovernanceModelConnectionProbe;
    let error = handle_http_in_process_request_with_console(
        &runtime,
        HttpRuntimeRequest::post_json("/console/governance-model/test-connection", "{}"),
        HttpConsoleServices::none().with_governance_model_probe(&probe),
    )
    .expect_err("fixture rejects the probe");
    std::env::remove_var(&env_name);
    fixture.join().expect("probe fixture");

    assert!(!error.to_string().contains(raw_secret));
    assert!(!format!("{error:?}").contains(raw_secret));
}

#[test]
fn governance_model_console_route_accepts_remote_endpoint_without_secondary_scope_switch() {
    let runtime = runtime();
    let response = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::patch_json(
            "/console/governance-model",
            serde_json::json!({
                "enabled": true,
                "protocol": "open_ai_compatible",
                "endpoint": "https://api.openai.com/v1",
                "model": "gpt-4.1-mini",
                "authMode": {
                    "kind": "credential_env",
                    "credentialEnv": "OPENAI_API_KEY"
                },
                "requestTimeoutMs": 30000,
                "maxInputTokens": 8192,
                "maxOutputTokens": 1024
            })
            .to_string(),
        ),
    )
    .expect("save remote endpoint");
    assert_eq!(response.status_code, 200, "{}", response.body);
    assert_eq!(
        runtime.console_governance_model().endpoint.as_deref(),
        Some("https://api.openai.com/v1")
    );
}

#[test]
fn console_workbench_api_map_route_exposes_entry_owned_report_apis() {
    let runtime = runtime();

    let response = handle_http_in_process_request(
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

    let response = handle_http_in_process_request(
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
    let report_object = report.as_object().expect("workbench report object");
    assert!(!report_object.contains_key("vaultMigration"));
    let archive = &report["archiveRestore"];
    #[cfg(feature = "nonproduction-replay-harness")]
    {
        assert_eq!(archive["status"]["status"], "ready");
        assert_eq!(archive["status"]["available"], true);
        assert_eq!(archive["status"]["reason"], "typed_archive_export_ready");
        assert!(archive["archiveRoot"].is_object());
    }
    #[cfg(not(feature = "nonproduction-replay-harness"))]
    {
        assert_eq!(archive["status"]["status"], "blocked");
        assert_eq!(archive["status"]["available"], false);
        assert_eq!(
            archive["status"]["reason"],
            "config: export.memory_space is not visible for current profile (stage: memory_runtime_operation)"
        );
        assert!(archive["archiveRoot"].is_null());
    }
    assert_eq!(archive["scope"]["kind"], "subject");
    assert_eq!(archive["scope"]["memory_space_id"], "space:owner-default");
    assert_eq!(
        archive["scope"]["mounted_subject_id"],
        "agent:http-console-agent"
    );
    assert_eq!(archive["privateMaterialPolicy"], "exclude_private");
    let archive_object = archive.as_object().expect("archive summary object");
    assert_eq!(archive_object.len(), 4);
}

#[test]
fn console_ollama_transparent_routes_delegate_to_controller_trait() {
    let runtime = runtime();
    let controller = MockOllamaTransparentController::default();
    let services = HttpConsoleServices::with_ollama_transparent(&controller);

    let capabilities = handle_http_in_process_request_with_console(
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

    let status = handle_http_in_process_request_with_console(
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

    let preflight = handle_http_in_process_request_with_console(
        &runtime,
        HttpRuntimeRequest::post_json("/console/ollama-transparent/preflight", "{}"),
        services,
    )
    .expect("preflight");
    assert_eq!(preflight.status_code, 200, "{}", preflight.body);
    let preflight: Value = serde_json::from_str(&preflight.body).expect("preflight json");
    assert_eq!(preflight["status"], "accepted");
    assert_eq!(preflight["preflight"]["accepted"], true);

    let enable = handle_http_in_process_request_with_console(
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

    let disable = handle_http_in_process_request_with_console(
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

    let open_app = handle_http_in_process_request_with_console(
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
    let response =
        handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/console/capabilities"))
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
        let error = handle_http_in_process_request(&runtime, request)
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

    let overview =
        handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/console/overview"))
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

    let transports =
        handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/console/transports"))
            .expect("transports");
    assert_eq!(transports.status_code, 200);
    assert!(transports.body.contains("\"transports\""));

    let llm_gateway =
        handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/console/llm-gateway"))
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

    let smoke_run = handle_http_in_process_request(
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

    let unknown_smoke = handle_http_in_process_request(
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

    let created = handle_http_in_process_request(
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

    let listed =
        handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/console/devices"))
            .expect("devices");
    assert_eq!(listed.status_code, 200);
    assert!(!listed.body.contains(app_key_once));

    let rotated = handle_http_in_process_request(
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

    let transport = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::patch_json(
            "/console/transports/http",
            r#"{"enabled":true,"endpoint":"127.0.0.1:8718"}"#,
        ),
    )
    .expect("transport patch");
    assert_eq!(transport.status_code, 200);
    assert!(transport.body.contains("\"endpoint\":\"127.0.0.1:8718\""));

    let device = handle_http_in_process_request(
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

    let before =
        handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/console/overview"))
            .expect("overview before");
    let before: Value = serde_json::from_str(&before.body).expect("overview before json");
    assert_eq!(before["overview"]["writesToday"]["value"], "0");

    let write_body = runtime_skill_http_write_body(
        "release_patch_flow",
        "Patch the release and verify the result",
        "1. inspect release diff\n2. patch rollback guards\n3. verify logs",
    );
    let write = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/write", &write_body),
    )
    .expect("write");
    assert_eq!(write.status_code, 200, "{}", write.body);

    let duplicate_write = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/write", &write_body),
    )
    .expect("duplicate write");
    assert_eq!(duplicate_write.status_code, 200, "{}", duplicate_write.body);

    let recall = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json(
            "/memory/recall",
            r#"{"temporal_operation":{"kind":"current"},"query":"release_patch_flow","limit":4}"#,
        ),
    )
    .expect("recall");
    assert_eq!(recall.status_code, 200, "{}", recall.body);

    let after =
        handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/console/overview"))
            .expect("overview after");
    let after: Value = serde_json::from_str(&after.body).expect("overview after json");
    let safe_overview_json = after.to_string();
    assert!(!safe_overview_json.contains("release_patch_flow"));
    assert!(!safe_overview_json.contains("Patch the release and verify the result"));
    assert!(!safe_overview_json.contains("patch rollback guards"));
    assert_eq!(after["overview"]["writesToday"]["value"], "1");
    assert_eq!(after["overview"]["recall"]["value"], "0.0%");
    assert!(after["overview"]["recentEvents"]
        .as_array()
        .expect("events")
        .iter()
        .any(|event| event["text"]
            .as_str()
            .is_some_and(|text| text.contains("Memory write accepted"))));
    let recent_events = after["overview"]["recentEvents"]
        .as_array()
        .expect("events");
    assert!(recent_events
        .iter()
        .any(|event| event["text"] == "Recall completed"));
    assert!(recent_events.iter().all(|event| event["text"]
        .as_str()
        .is_some_and(|text| !text.contains("release_patch_flow"))));
}

#[test]
fn console_overview_aggregates_one_external_memory_event_authority_root() {
    let transparent_store_path = test_store_dir("transparent-ollama-events");
    let transparent_runtime = runtime_with_file_store(&transparent_store_path);
    seed_memory_runtime_activity(&transparent_runtime);

    let runtime = runtime();
    let event_store_paths = vec![transparent_store_path];
    let services = HttpConsoleServices::none().with_memory_event_store_paths(&event_store_paths);

    let overview = handle_http_in_process_request_with_console(
        &runtime,
        HttpRuntimeRequest::get("/console/overview"),
        services,
    )
    .expect("overview");

    assert_eq!(overview.status_code, 200, "{}", overview.body);
    let body: Value = serde_json::from_str(&overview.body).expect("overview json");
    assert_eq!(body["overview"]["writesToday"]["value"], "1");
    assert_eq!(body["overview"]["recall"]["value"], "0.0%");
    assert_eq!(
        body["overview"]["recall"]["desc"],
        "0 recall requests / 0 with hits"
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
fn console_overview_rejects_metric_authority_roots_beyond_the_active_budget() {
    let transparent_store_path = test_store_dir("transparent-ollama-events-over-budget");
    let transparent_runtime = runtime_with_file_store(&transparent_store_path);
    seed_memory_runtime_activity(&transparent_runtime);

    let runtime = runtime();
    let event_store_paths = vec![transparent_store_path.clone(), transparent_store_path];
    let services = HttpConsoleServices::none().with_memory_event_store_paths(&event_store_paths);

    let error = handle_http_in_process_request_with_console(
        &runtime,
        HttpRuntimeRequest::get("/console/overview"),
        services,
    )
    .expect_err("raw authority roots beyond the active budget must fail closed");

    assert_eq!(error.stage(), "runtime_metrics_source_store_capacity");
}

#[test]
fn console_overview_fails_closed_when_external_metric_evidence_is_unreadable() {
    let runtime = runtime();
    let missing_store = test_store_dir("missing-metric-evidence");
    let event_store_paths = vec![missing_store];
    let services = HttpConsoleServices::none().with_memory_event_store_paths(&event_store_paths);

    let error = handle_http_in_process_request_with_console(
        &runtime,
        HttpRuntimeRequest::get("/console/overview"),
        services,
    )
    .expect_err("unreadable metric evidence must not become a zero report");

    assert_eq!(error.stage(), "runtime_metrics_event_store_root");
}

#[test]
fn legacy_name_based_console_skill_route_is_not_available() {
    let runtime = runtime();
    let response = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest {
            method: HttpMethod::Delete,
            path: "/console/skills/runtime_skill__missing".to_string(),
            body: String::new(),
            request_id: "http-delete-req".to_string(),
            idempotency_key: "http-delete-idem".to_string(),
            audit_id: "http-delete-audit".to_string(),
            authorization: None,
        },
    )
    .expect("delete");
    assert_eq!(response.status_code, 405);
}

#[test]
fn console_http_skill_routes_edit_runtime_skills_without_store_shortcut() {
    let runtime = runtime();

    let create_forbidden = handle_http_in_process_request(
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

    let seed_body = runtime_skill_http_write_body(
        "runtime_skill__release",
        "Check artifacts before publishing.",
        "1. run gates\n2. inspect artifacts\n3. dry run publish",
    );
    let seeded = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/memory/write", &seed_body),
    )
    .expect("seed runtime skill");
    assert_eq!(seeded.status_code, 200, "{}", seeded.body);

    let list = handle_http_in_process_request(&runtime, HttpRuntimeRequest::get("/console/skills"))
        .expect("list");
    assert_eq!(list.status_code, 200);
    assert!(list.body.contains("Release guard"));
    assert!(list.body.contains("runtimeLearned"));
    let list_body: Value = serde_json::from_str(&list.body).expect("list json");
    let locator = list_body["skills"]["skills"][0]["locator"].clone();
    assert!(locator.get("owner_revision_ref").is_none());

    let detail = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/console/skills/detail", locator.to_string()),
    )
    .expect("detail");
    assert_eq!(detail.status_code, 200, "{}", detail.body);
    assert!(detail.body.contains("run gates"));

    let mut missing_locator = locator.clone();
    missing_locator["owner_id"] = Value::String(format!("runtime_skill:sha256:{}", "0".repeat(64)));
    let missing = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/console/skills/detail", missing_locator.to_string()),
    )
    .expect("missing detail response");
    assert_eq!(missing.status_code, 404, "{}", missing.body);

    let mut invalid_locator = locator.clone();
    invalid_locator["owner_revision_ref"] = serde_json::json!({
        "owner_ref": {
            "owner_plane": "long_term",
            "owner_id": "legacy-raw-owner"
        },
        "owner_revision": 1
    });
    let invalid = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/console/skills/detail", invalid_locator.to_string()),
    )
    .expect_err("legacy raw locator must fail strict JSON decoding");
    assert_eq!(invalid.stage(), "console_json");

    let edit_body = serde_json::json!({
        "locator": locator,
        "title": "Release guard",
        "topic": "release",
        "summary": "Check artifacts and changelog before publishing.",
        "procedure": "1. run gates\n2. inspect artifacts\n3. inspect changelog",
        "editReason": "http_test_edit",
    })
    .to_string();
    let edited = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::patch_json("/console/skills", &edit_body),
    )
    .expect("edit");
    assert_eq!(edited.status_code, 200, "{}", edited.body);
    let edited_body: Value = serde_json::from_str(&edited.body).expect("edit json");
    let edited_locator = edited_body["mutation"]["currentLocator"].clone();

    let stale_edit = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::patch_json("/console/skills", &edit_body),
    )
    .expect("stale edit response");
    assert_eq!(stale_edit.status_code, 409, "{}", stale_edit.body);

    let disabled_body = serde_json::json!({
        "locator": edited_locator,
        "enabled": false,
    })
    .to_string();
    let disabled = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::patch_json("/console/skills/enabled", &disabled_body),
    )
    .expect("disable");
    assert_eq!(disabled.status_code, 200, "{}", disabled.body);
    let disabled_body: Value = serde_json::from_str(&disabled.body).expect("disable json");
    let disabled_locator = disabled_body["mutation"]["currentLocator"].clone();

    let retired = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/console/skills/retire", disabled_locator.to_string()),
    )
    .expect("retire");
    assert_eq!(retired.status_code, 200, "{}", retired.body);
    let retired_body: Value = serde_json::from_str(&retired.body).expect("retire json");
    let retired_locator = retired_body["mutation"]["currentLocator"].clone();

    let detail_after_retire = handle_http_in_process_request(
        &runtime,
        HttpRuntimeRequest::post_json("/console/skills/detail", retired_locator.to_string()),
    )
    .expect("detail after retire");
    assert_eq!(detail_after_retire.status_code, 200);
    assert!(detail_after_retire.body.contains("\"status\":\"retired\""));
}
