use bm_cli::{command_specs, render_capabilities, run_cli};
use bm_sdk::{resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId};

#[cfg(any(
    feature = "profile-desktop-macos-dev-full",
    feature = "profile-desktop-windows-dev-full",
    feature = "profile-server-linux-dev-full"
))]
use bm_sdk::{
    default_agent_subject_id, GovernedRuntimeSkillWriteInput, MemoryIdentity, MemoryPrivacyClass,
    MemoryRuntime, MemoryScope, MemoryStoreHandle, RuntimeSkillCreationRef,
    RuntimeSkillOwningScope, RuntimeSkillWrite, StoreBackendConfig,
};

#[cfg(any(
    feature = "profile-desktop-macos-dev-full",
    feature = "profile-desktop-windows-dev-full",
    feature = "profile-server-linux-dev-full"
))]
fn assert_exact_governed_result(value: &serde_json::Value) {
    let result = value["result"].clone();
    let dto: bm_adapter::AdapterGovernedSafeReportV1 =
        serde_json::from_value(result.clone()).expect("strict adapter governed safe DTO");
    assert_eq!(
        serde_json::to_value(dto).expect("serialize adapter governed safe DTO"),
        result
    );
}

fn host_profile_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "profile-desktop-macos-standalone-memory"
    }
    #[cfg(target_os = "windows")]
    {
        "profile-desktop-windows-embedded-sdk"
    }
    #[cfg(target_os = "linux")]
    {
        "profile-desktop-linux-embedded-sdk"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "profile-esp-embedded-sdk"
    }
}

#[cfg(any(
    feature = "profile-desktop-macos-dev-full",
    feature = "profile-desktop-windows-dev-full",
    feature = "profile-server-linux-dev-full"
))]
fn host_profile_id() -> ProfileId {
    #[cfg(target_os = "macos")]
    {
        ProfileId::DesktopMacosStandaloneMemory
    }
    #[cfg(target_os = "windows")]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(target_os = "linux")]
    {
        ProfileId::ServerLinuxMemoryGateway
    }
}

#[cfg(any(
    feature = "profile-desktop-macos-dev-full",
    feature = "profile-desktop-windows-dev-full",
    feature = "profile-server-linux-dev-full"
))]
fn seed_runtime_skill_fixture(
    store_root: &std::path::Path,
    agent_id: &str,
    name: &str,
    title: &str,
    topic: &str,
    summary: &str,
    content: &str,
) {
    let profile = host_profile_id();
    let store = MemoryStoreHandle::open(
        StoreBackendConfig::file(store_root, profile)
            .expect("fixture store config")
            .with_fsync(false),
    )
    .expect("fixture store");
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.replay_harness_enabled = true;
    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new(agent_id, "owner-default").expect("fixture identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("fixture scope"))
        .store(store)
        .capability_policy(capability)
        .build()
        .expect("fixture runtime");
    let report = runtime
        .seed_runtime_skills_for_replay(
            vec![GovernedRuntimeSkillWriteInput {
                write: RuntimeSkillWrite {
                    name: name.to_string(),
                    title: title.to_string(),
                    topic: topic.to_string(),
                    summary: summary.to_string(),
                    content: content.to_string(),
                    citations: vec!["cli-test-fixture".to_string()],
                    source_chat_id: Some("chat-1".to_string()),
                    observed_at: 1_800_000_000,
                },
                creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
                    candidate_ref: format!("cli-test:{name}"),
                    verification_receipt_digest:
                        "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                            .to_string(),
                },
                privacy_class: MemoryPrivacyClass::SharedWithSubject,
            }],
            RuntimeSkillOwningScope::Subject {
                mounted_subject_id: default_agent_subject_id(agent_id),
            },
        )
        .expect("seed runtime skill fixture");
    assert!(report.accepted);
}

#[test]
fn cli_default_build_enables_entry_governance_model_client() {
    assert!(bm_entry::entry_governance_model_client_compiled());
}

#[test]
fn command_catalog_covers_adapter_plan_without_core_store_bypass() {
    let commands: Vec<_> = command_specs().iter().map(|spec| spec.name).collect();
    assert_eq!(
        commands,
        vec![
            "write",
            "capabilities",
            "inspect",
            "recall",
            "project",
            "replay",
            "finalize-turn",
            "long-term-list",
            "long-term-detail",
            "long-term-delete",
            "long-term-policy-suppress",
            "transcript-attr-write",
            "skill-list",
            "skill-show",
            "skill-edit",
            "skill-enable",
            "skill-disable",
            "skill-retire",
            "close",
        ]
    );

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

#[test]
fn legacy_public_procedural_write_command_is_rejected_before_runtime_open() {
    let error = run_cli(
        ["memory", "write-procedural"]
            .into_iter()
            .map(str::to_string),
    )
    .expect_err("public procedural creation must not remain exposed");
    assert_eq!(error, "unsupported memory command: write-procedural");
}

#[test]
fn legacy_continuity_transfer_commands_are_rejected_before_runtime_open() {
    for command in ["export", "import"] {
        let error = run_cli(["memory", command].into_iter().map(str::to_string))
            .expect_err("legacy continuity transfer command must not remain public");
        assert_eq!(error, format!("unsupported memory command: {command}"));
    }
}

#[test]
fn runtime_skill_management_rejects_legacy_name_before_runtime_open() {
    for command in [
        "skill-list",
        "skill-show",
        "skill-edit",
        "skill-enable",
        "skill-disable",
        "skill-retire",
    ] {
        let error = run_cli(
            [
                "memory",
                command,
                "--profile",
                host_profile_name(),
                "--name",
                "runtime_skill__legacy_identity",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .expect_err("management name compatibility must be rejected");
        assert!(
            error.contains("--name is not accepted"),
            "{command}: {error}"
        );
    }
}

#[test]
fn long_term_destructive_commands_require_explicit_reason() {
    let delete = run_cli(
        [
            "memory",
            "long-term-delete",
            "--profile",
            host_profile_name(),
            "--idempotency-key",
            "cli-delete-missing-reason",
            "--record-id",
            "ltm-test",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect_err("delete without reason should fail");
    assert!(delete.contains("--reason is required"));

    let suppress = run_cli(
        [
            "memory",
            "long-term-policy-suppress",
            "--profile",
            host_profile_name(),
            "--topic",
            "temporary-*",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect_err("policy suppress without reason should fail");
    assert!(suppress.contains("--reason is required"));

    let attr_write = run_cli(
        [
            "memory",
            "transcript-attr-write",
            "--profile",
            host_profile_name(),
            "--input",
            "/tmp/missing-transcript-attrs.json",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect_err("transcript attr write without reason should fail");
    assert!(attr_write.contains("--reason is required"));
}

#[test]
fn transcript_attr_write_command_is_public_and_requires_explicit_reason() {
    let spec = command_specs()
        .iter()
        .find(|spec| spec.name == "transcript-attr-write")
        .expect("transcript attr write command");
    assert_eq!(
        spec.operation,
        bm_adapter::AdapterOperation::TranscriptAttrWrite
    );

    let attr_write = run_cli(
        [
            "memory",
            "transcript-attr-write",
            "--profile",
            host_profile_name(),
            "--input",
            "/tmp/missing-transcript-attrs.json",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect_err("transcript attr write without reason should fail");
    assert!(attr_write.contains("--reason is required"));
}

#[test]
fn finalize_turn_command_is_public_and_uses_the_shared_json_contract() {
    let spec = command_specs()
        .iter()
        .find(|spec| spec.name == "finalize-turn")
        .expect("finalize turn command");
    assert_eq!(spec.operation, bm_adapter::AdapterOperation::FinalizeTurn);

    let error = run_cli(
        [
            "memory",
            "finalize-turn",
            "--profile",
            host_profile_name(),
            "--input",
            "/tmp/missing-finalize-turn.json",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect_err("missing finalize request must fail before dispatch");
    assert!(error.contains("failed to read finalize turn request"));
}

#[cfg(any(
    feature = "profile-desktop-macos-dev-full",
    feature = "profile-desktop-windows-dev-full",
    feature = "profile-server-linux-dev-full"
))]
#[test]
fn memory_cli_skill_management_uses_entry_runtime_facade() {
    let root = unique_temp_dir("bm-cli-skill-management");
    let store = root.to_string_lossy().to_string();

    seed_runtime_skill_fixture(
        &root,
        "cli-skill-agent",
        "runtime_skill__release",
        "Release guard",
        "release",
        "Check release artifacts before publishing.",
        "1. run gates\n2. inspect artifacts\n3. dry run publish",
    );

    let initial_list = run_cli(
        [
            "memory",
            "skill-list",
            "--profile",
            host_profile_name(),
            "--store-file",
            &store,
            "--agent",
            "cli-skill-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--runtime-skill-subject",
            "agent:cli-skill-agent",
            "--query",
            "release",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("initial skill list");
    let initial_list_json: serde_json::Value =
        serde_json::from_str(&initial_list).expect("initial list json");
    let owner_id = initial_list_json["skills"]["skills"][0]["ownerId"]
        .as_str()
        .expect("owner id")
        .to_string();

    let edited = run_cli(
        [
            "memory",
            "skill-edit",
            "--profile",
            host_profile_name(),
            "--store-file",
            &store,
            "--agent",
            "cli-skill-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--runtime-skill-subject",
            "agent:cli-skill-agent",
            "--runtime-skill-owner-id",
            &owner_id,
            "--runtime-skill-owner-revision",
            "1",
            "--title",
            "Release guard",
            "--topic",
            "release",
            "--summary",
            "Check release artifacts and changelog before publishing.",
            "--content",
            "1. run gates\n2. inspect artifacts\n3. inspect changelog",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("skill edit");
    let edited_json: serde_json::Value = serde_json::from_str(&edited).expect("edit json");
    assert_eq!(edited_json["mutation"]["accepted"], true);

    let list = run_cli(
        [
            "memory",
            "skill-list",
            "--profile",
            host_profile_name(),
            "--store-file",
            &store,
            "--agent",
            "cli-skill-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--runtime-skill-subject",
            "agent:cli-skill-agent",
            "--query",
            "release",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("skill list");
    let list_json: serde_json::Value = serde_json::from_str(&list).expect("list json");
    assert_eq!(list_json["skills"]["total"], 1);
    assert_eq!(list_json["skills"]["runtimeLearned"], 1);
    let revision_after_edit = list_json["skills"]["skills"][0]["locator"]["owner_revision"]
        .as_u64()
        .expect("revision after edit")
        .to_string();

    let disabled = run_cli(
        [
            "memory",
            "skill-disable",
            "--profile",
            host_profile_name(),
            "--store-file",
            &store,
            "--agent",
            "cli-skill-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--runtime-skill-subject",
            "agent:cli-skill-agent",
            "--runtime-skill-owner-id",
            &owner_id,
            "--runtime-skill-owner-revision",
            &revision_after_edit,
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("skill disable");
    let disabled_json: serde_json::Value = serde_json::from_str(&disabled).expect("disable json");
    assert_eq!(disabled_json["mutation"]["accepted"], true);
    let revision_after_disable = disabled_json["mutation"]["currentLocator"]["owner_revision"]
        .as_u64()
        .expect("revision after disable")
        .to_string();

    let retired = run_cli(
        [
            "memory",
            "skill-retire",
            "--profile",
            host_profile_name(),
            "--store-file",
            &store,
            "--agent",
            "cli-skill-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--runtime-skill-subject",
            "agent:cli-skill-agent",
            "--runtime-skill-owner-id",
            &owner_id,
            "--runtime-skill-owner-revision",
            &revision_after_disable,
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("skill retire");
    let retired_json: serde_json::Value = serde_json::from_str(&retired).expect("retire json");
    assert_eq!(retired_json["mutation"]["accepted"], true);
}

#[test]
fn capabilities_output_contains_runtime_validation_and_adapter_catalog() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    policy.adapter.http_enabled = true;
    let catalog = resolve_memory_capabilities(
        ProfileId::ServerLinuxMemoryGateway,
        &policy,
        &MemoryPrivacyPolicy::standard_private_boundary(),
    )
    .expect("catalog");

    let output = render_capabilities(&catalog).expect("json");
    assert!(output.contains("\"profile\""));
    assert!(output.contains("\"adapter\""));
    assert!(output.contains("\"entry\""));
    assert!(output.contains("\"llm_gateway_server\""));
    assert!(output.contains("\"lifecycle\""));
    assert!(output.contains("\"validation\""));
    assert!(!output.contains("private_garden_raw"));
    assert!(!output.contains("subject_state_raw"));
    assert!(!output.contains("soul_governance_raw"));
}

#[cfg(any(
    feature = "profile-desktop-macos-dev-full",
    feature = "profile-desktop-windows-dev-full",
    feature = "profile-server-linux-dev-full"
))]
#[test]
fn memory_cli_file_runtime_skill_projection_respects_transport_capability() {
    let root = unique_temp_dir("bm-cli-entry-runtime");
    let store = root.to_string_lossy().to_string();

    seed_runtime_skill_fixture(
        &root,
        "cli-agent",
        "runtime_skill__cli_entry",
        "CLI entry runtime",
        "cli-entry",
        "CLI fixture exercises the EntryRuntime read facade.",
        "1. Open EntryRuntime with an explicit profile and store.\n2. Reopen through the CLI.\n3. Consume governed reports only.",
    );

    let recall = run_cli(
        [
            "memory",
            "recall",
            "--profile",
            host_profile_name(),
            "--store-file",
            &store,
            "--agent",
            "cli-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--query",
            "entry runtime cli",
            "--limit",
            "4",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("recall");
    let recall_json: serde_json::Value = serde_json::from_str(&recall).expect("recall json");
    assert_eq!(recall_json["status"], "accepted");
    assert_eq!(recall_json["result"]["operation"], "recall");
    assert_exact_governed_result(&recall_json);
    assert_eq!(
        recall_json["result"]["report"]["governed_recall"]["authority"]["runtime_skill_transport"],
        "unavailable"
    );
    assert_eq!(
        recall_json["result"]["report"]["governed_recall"]["procedural_delivery"],
        serde_json::json!([])
    );

    let long_term = run_cli(
        [
            "memory",
            "long-term-list",
            "--profile",
            host_profile_name(),
            "--store-file",
            &store,
            "--agent",
            "cli-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--limit",
            "4",
        ]
        .into_iter()
        .map(str::to_string),
    )
    .expect("long-term list");
    let long_term_json: serde_json::Value =
        serde_json::from_str(&long_term).expect("long-term json");
    assert_eq!(long_term_json["status"], "accepted");
    assert!(long_term_json.get("total_visible").is_some());
}

#[test]
fn memory_cli_typed_factual_write_reopens_and_replays_without_duplicate_mutation() {
    use bm_sdk::{
        LongTermMemoryKind, MemoryCandidateContent, MemoryCandidateTarget, MemoryEvidenceAuthority,
        MemoryPrivacyClass, MemorySubjectVisibilityPolicy, MemoryWriteCandidate,
        MemoryWriteRequest,
    };
    let root = unique_temp_dir("bm-cli-factual-wire");
    std::fs::create_dir_all(&root).unwrap();
    let input = root.join("request.json");
    let store = root.join("store");
    let request = MemoryWriteRequest::Candidates {
        candidates: vec![MemoryWriteCandidate {
            candidate_id: "cli-factual-evidence".into(),
            authority: MemoryEvidenceAuthority::UserAsserted,
            target: MemoryCandidateTarget::LongTermMemory {
                kind: LongTermMemoryKind::Project,
                topic: "cli-write-project".into(),
            },
            long_term_subject_visibility: Some(MemorySubjectVisibilityPolicy::AllSubjects),
            privacy: MemoryPrivacyClass::SharedWithSubject,
            content: MemoryCandidateContent::Text {
                topic: "cli-write-project".into(),
                body: "The CLI write project stores durable factual evidence.".into(),
                keywords: vec!["cli-write-project".into()],
            },
            evidence_refs: vec!["fixture:cli-factual-evidence".into()],
            canonical_entities: vec![],
            semantic_judgment: Some(bm_sdk::MemoryCandidateSemanticJudgment {
                source: bm_sdk::MemorySemanticJudgmentSource::RuntimeGate,
                decision: bm_sdk::MemoryCandidateSemanticDecision::Accept,
                governed_target: Some(MemoryCandidateTarget::LongTermMemory {
                    kind: LongTermMemoryKind::Project,
                    topic: "cli-write-project".into(),
                }),
                reason: "explicit factual intake".into(),
            }),
        }],
    };
    std::fs::write(&input, serde_json::to_vec(&request).unwrap()).unwrap();
    let invoke = |command: &str, extra: &[&str]| {
        let mut args = vec![
            "memory".to_string(),
            command.into(),
            "--profile".into(),
            host_profile_name().into(),
            "--store-file".into(),
            store.to_string_lossy().into_owned(),
        ];
        args.extend(extra.iter().map(|arg| arg.to_string()));
        let output = run_cli(args).expect("public CLI dispatch");
        serde_json::from_str::<serde_json::Value>(&output).unwrap()
    };
    let input_arg = input.to_string_lossy();
    let extra = [
        "--input",
        input_arg.as_ref(),
        "--idempotency-key",
        "cli-factual-operation",
    ];
    let written = invoke("write", &extra);
    assert_eq!(written["status"], "accepted", "{written}");
    assert_eq!(written["changed"], 1);
    let replayed = invoke("write", &extra);
    assert_eq!(replayed["status"], "replayed");
    let recalled = invoke("recall", &["--query", "cli-write-project", "--limit", "8"]);
    assert_eq!(recalled["status"], "accepted");
    assert!(
        !recalled["result"]["report"]["governed_recall"]["validity_candidate_bindings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let projected = invoke("project", &["--query", "cli-write-project"]);
    assert_eq!(projected["status"], "accepted");
    assert!(projected["result"]["report"]["projection_block"]
        .as_str()
        .unwrap()
        .contains("The CLI write project stores durable factual evidence."));
    std::fs::remove_dir_all(root).unwrap();
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

#[cfg(any(
    feature = "profile-desktop-macos-dev-full",
    feature = "profile-desktop-windows-dev-full",
    feature = "profile-server-linux-dev-full"
))]
#[test]
fn memory_cli_binary_can_reopen_file_store_across_processes() {
    let root = unique_temp_dir("bm-cli-binary-entry-runtime");
    let store = root.to_string_lossy().to_string();

    seed_runtime_skill_fixture(
        &root,
        "cli-agent",
        "runtime_skill__cli_binary_entry",
        "CLI binary entry runtime",
        "cli-entry",
        "CLI binary reopens a fixture persisted through the SDK owner.",
        "1. Persist through the configured file store.\n2. Reopen from a second process.\n3. Consume governed recall only.",
    );

    let recall = std::process::Command::new(env!("CARGO_BIN_EXE_bm"))
        .args([
            "memory",
            "recall",
            "--profile",
            host_profile_name(),
            "--store-file",
            &store,
            "--agent",
            "cli-agent",
            "--owner",
            "owner-default",
            "--channel",
            "local",
            "--chat",
            "chat-1",
            "--query",
            "cli binary entry runtime",
            "--limit",
            "4",
        ])
        .output()
        .expect("recall command");
    assert!(
        recall.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recall.stderr)
    );

    let value: serde_json::Value = serde_json::from_slice(&recall.stdout).expect("recall json");
    assert_eq!(value["result"]["operation"], "recall");
    assert_eq!(
        value["result"]["report"]["governed_recall"]["authority"]["runtime_skill_transport"],
        "unavailable"
    );
    assert_eq!(
        value["result"]["report"]["governed_recall"]["procedural_delivery"],
        serde_json::json!([])
    );
}
