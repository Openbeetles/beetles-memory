use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "../../../../crates/sdk/tests/support/procedural.rs"]
mod procedural;

use bm_desktop::{
    DesktopConsoleRequest, DesktopConsoleState, DesktopMemoryAuthority, DesktopRuntimeConfig,
};
use bm_entry::{
    EntryAuthConfig, EntryIdempotencyConfig, EntryIdentity, EntryRuntime, EntryRuntimeConfig,
    EntryScope, EntryTransportConfig,
};
use bm_sdk::{
    AgentToolDescriptor, AgentToolRegistrySnapshot, AgentToolUsageFeedbackV3,
    AuthorizedGovernanceEnvelope, CanonicalTurnDelta, ConversationScope, GovernanceEgressAuthority,
    GovernanceExecutionOperation, GovernanceExecutionPort, GovernanceExecutionPortFailure,
    ImmutableGovernanceExecutionBinding, IngressKind, LongTermMemoryDraft, LongTermMemoryKind,
    LongTermMemoryProvenance, MemoryCapabilityPolicy, MemoryEvidenceAuthority, MemoryIdentity,
    MemoryLearningCycleOutcome, MemoryLearningCycleRequest, MemoryLearningEngine,
    MemoryPrivacyClass, MemoryPrivacyPolicy, MemoryProjectionRequest, MemoryRuntime, MemoryScope,
    MemoryStoreHandle, MemorySubjectVisibilityPolicy, MemoryTurnDeliveryStatus,
    MemoryTurnFinalizeRequest, MemoryTurnProtocol, MemoryTurnSource, MemoryWriteRequest,
    ParsedLongTermMemoryExtraction, PostTurnLearningInputV2, PressureLevel,
    ProceduralProjectionBindingV1, ProceduralSourceSensitivity, ProfileId,
    RuntimeLifecycleModeInput, RuntimeSkillListRequest, RuntimeSkillOwningScope,
    StoreBackendConfig, ToolExecutionFactV1, ToolExecutionOutcome, ToolMethodEvidenceV1,
    TranscriptInputMessage,
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
    let data_dir = test_store_dir("skills-mutation");
    let learning_runtime = learn_runtime_skill(data_dir.join("store"));
    let state = DesktopConsoleState::open(desktop_config(data_dir)).expect("desktop state");

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

    let list = state
        .handle_console_request(DesktopConsoleRequest::get("/console/skills?query=desktop"))
        .unwrap();
    assert_eq!(list.status_code, 200);
    let list_body: Value = serde_json::from_str(&list.body).expect("skill list json");
    assert_eq!(list_body["skills"]["skills"].as_array().unwrap().len(), 1);
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
    finish_procedural_reconciliation(&learning_runtime);

    let retired = state
        .handle_console_request(DesktopConsoleRequest::post_json(
            "/console/skills/retire",
            disabled_locator.to_string(),
        ))
        .expect("retire response");
    assert_eq!(retired.status_code, 200, "{}", retired.body);
    let retired_body: Value = serde_json::from_str(&retired.body).expect("retire mutation json");
    let retired_locator = retired_body["mutation"]["currentLocator"].clone();
    finish_procedural_reconciliation(&learning_runtime);

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
    let changed = seed_memory_runtime_activity(&transparent_runtime);
    let metrics = transparent_runtime
        .runtime()
        .runtime_metrics_report()
        .unwrap();
    assert_eq!(
        metrics.counters.write_changed_count,
        u64::try_from(changed).unwrap()
    );
    drop(transparent_runtime);
    let state = DesktopConsoleState::open(desktop_config(data_dir)).unwrap();

    let response = state
        .handle_console_request(DesktopConsoleRequest::get("/console/overview"))
        .unwrap();

    assert_eq!(response.status_code, 200, "{}", response.body);
    let body: Value = serde_json::from_str(&response.body).expect("overview json");
    assert_eq!(
        body["overview"]["writesToday"]["value"],
        changed.to_string()
    );
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
            conversation_id: None,
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

fn seed_memory_runtime_activity(runtime: &EntryRuntime) -> usize {
    let write = runtime
        .runtime()
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    topic: "desktop_overview".into(),
                    content: "The desktop and transparent gateway share one memory store.".into(),
                    keywords: vec!["desktop".into()],
                    privacy: MemoryPrivacyClass::SharedWithSubject,
                    source_chat_id: Some("local-desktop".into()),
                    source_type: None,
                    source_scope: None,
                    subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
                    provenance: LongTermMemoryProvenance::new(
                        MemoryEvidenceAuthority::RuntimeObservation,
                    ),
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["fixture:desktop-overview".into()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: None,
                    source_revision: None,
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("write");
    assert_eq!(
        write.changed, 1,
        "one real accepted fact, not a fabricated event"
    );
    runtime
        .runtime()
        .project(MemoryProjectionRequest {
            binding: ProceduralProjectionBindingV1::Preview,
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
    write.changed
}

struct NoProvider;

impl GovernanceExecutionPort for NoProvider {
    fn execute(
        &mut self,
        _: &AuthorizedGovernanceEnvelope,
        _: &ImmutableGovernanceExecutionBinding,
        _: &GovernanceEgressAuthority,
        _: &mut dyn GovernanceExecutionOperation,
    ) -> Result<(), GovernanceExecutionPortFailure> {
        panic!("procedural fixture must not invoke a Provider")
    }
}

fn learn_runtime_skill(path: std::path::PathBuf) -> Arc<MemoryRuntime> {
    let registry = AgentToolRegistrySnapshot::compact(
        "desktop-tools",
        "host",
        vec![AgentToolDescriptor::compact(
            "desktop.inspect",
            "Inspect desktop",
            "desktop-v1",
        )],
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );
    let store = MemoryStoreHandle::open(
        StoreBackendConfig::file(path, ProfileId::DesktopMacosStandaloneMemory)
            .unwrap()
            .with_fsync(false),
    )
    .unwrap();
    let memory = Arc::new(
        MemoryRuntime::builder()
            .identity(MemoryIdentity::new("bm-desktop", "local-owner").unwrap())
            .scope(MemoryScope::new("desktop", "local-desktop").unwrap())
            .store(store.clone())
            .agent_tool_registry(registry.clone())
            .build()
            .unwrap(),
    );
    let governor = procedural::governor(&memory, store);
    let capability = procedural::register_and_issue(
        &governor,
        procedural::runtime_observer_spec(&memory, "desktop-contract-observer"),
        "register-desktop-contract-observer",
    );
    let list_request = RuntimeSkillListRequest {
        owning_scope: RuntimeSkillOwningScope::Subject {
            mounted_subject_id: memory.subject_id().into(),
        },
        query: None,
        include_disabled: true,
        include_retired: true,
        limit: 8,
    };
    assert!(memory
        .list_runtime_skills(list_request.clone())
        .unwrap()
        .skills
        .is_empty());
    for (index, turn_id) in ["desktop-method-first", "desktop-method-second"]
        .into_iter()
        .enumerate()
    {
        let facts: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|call| ToolExecutionFactV1 {
                observation_id: format!("{turn_id}-{call}-observation"),
                call_id: format!("{turn_id}-{call}"),
                outcome: ToolExecutionOutcome::Succeeded,
                source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
                started_at: None,
                completed_at: None,
            })
            .collect();
        let method = ToolMethodEvidenceV1 {
            method_id: format!("{turn_id}-method"),
            task_signature: "desktop_console".into(),
            body: "1. open the desktop app\n2. call the shared console API\n3. verify the returned report".into(),
            execution_refs: facts.iter().map(|fact| fact.observation_id.clone()).collect(),
            source_sensitivity: ProceduralSourceSensitivity::NonPrivate,
            external_content: false,
        };
        let finalized = memory
            .finalize_turn_with_procedural_evidence(
                &capability,
                MemoryTurnFinalizeRequest {
                    turn: CanonicalTurnDelta {
                        turn_id: turn_id.into(),
                        conversation: ConversationScope {
                            channel: "desktop".into(),
                            chat_id: "local-desktop".into(),
                            conversation_id: Some("local-desktop".into()),
                        },
                        subject: memory.subject_id().into(),
                        delivery_status: MemoryTurnDeliveryStatus::Delivered,
                        source: MemoryTurnSource {
                            ingress: IngressKind::User,
                            channel: "desktop".into(),
                            provider: None,
                            protocol: MemoryTurnProtocol::Native,
                            endpoint: None,
                            model_alias: None,
                            model_resolved: None,
                            request_id: None,
                            client_conversation_hint: None,
                        },
                        actor: None,
                        input_messages: vec![TranscriptInputMessage::user(
                            "Inspect desktop console",
                        )],
                        assistant_message: Some(TranscriptInputMessage::assistant(
                            "Desktop console verified",
                        )),
                        tool_observations: procedural::canonical_observations(
                            "desktop.inspect",
                            &facts,
                        ),
                        external_content_used: false,
                        candidate_ids: Vec::new(),
                    },
                    learning: PostTurnLearningInputV2 {
                        tool_call_count: 2,
                        agent_tool_feedback: vec![AgentToolUsageFeedbackV3 {
                            registry_ref: registry.registry_ref(),
                            tool_id: "desktop.inspect".into(),
                            schema_fingerprint: "desktop-v1".into(),
                            execution_facts: facts,
                            method_evidence: vec![method],
                        }],
                        ..PostTurnLearningInputV2::empty()
                    },
                    pressure: PressureLevel::Normal,
                    mode_input: RuntimeLifecycleModeInput::default(),
                },
            )
            .unwrap();
        let outcome = MemoryLearningEngine::attach(memory.clone())
            .unwrap()
            .run_due_cycle(
                MemoryLearningCycleRequest {
                    lease_owner: "desktop-contract-worker".into(),
                    lease_duration_secs: 60,
                },
                &mut NoProvider,
            )
            .unwrap();
        let MemoryLearningCycleOutcome::ProceduralCompleted(report) = outcome else {
            panic!("official learning completion required: {outcome:?}")
        };
        assert_eq!(
            Some(report.job.job_id),
            finalized.procedural_learning.job_id
        );
        assert_eq!(report.receipt.accepted_count, 1);
        assert!(report.receipt.changed_count > 0);
        let skills = memory
            .list_runtime_skills(list_request.clone())
            .unwrap()
            .skills;
        assert_eq!(
            skills.len(),
            index,
            "only two independent accepted turns create a skill"
        );
        if let Some(skill) = skills.first() {
            assert_eq!(skill.locator.owner_revision(), 1);
        }
    }
    memory
}

fn finish_procedural_reconciliation(memory: &Arc<MemoryRuntime>) {
    // DesktopConsoleState owns EntryRuntime's official background service.
    // Observe its committed result instead of racing it with a second worker.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let report = memory
            .list_runtime_skills(RuntimeSkillListRequest {
                owning_scope: RuntimeSkillOwningScope::Subject {
                    mounted_subject_id: memory.subject_id().into(),
                },
                query: None,
                include_disabled: true,
                include_retired: true,
                limit: 8,
            })
            .unwrap();
        if report.read_availability.is_ready() {
            assert!(
                !report.skills.is_empty(),
                "completed learning owner stays inspectable"
            );
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "official desktop reconciliation did not become readable: {:?}",
            report.read_availability
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
