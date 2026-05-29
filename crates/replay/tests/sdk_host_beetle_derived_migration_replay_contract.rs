use bm_replay::{
    run_replay_fixture, ReplayExpectedOutcome, ReplayFixture, ReplayOperation, ReplayRunnerConfig,
};
use bm_sdk::{
    apply_memory_space_migration, export_memory_space, preview_memory_space_migration,
    MemoryIdentity, MemoryProjectionRequest, MemoryScope, MemorySpaceExportRequest,
    MemorySpaceMigrateApplyRequest, MemorySpaceMigratePreviewRequest, MemoryWriteCandidate,
    MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput, StoreBackendConfig,
    StorePlatform,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SdkHostReplayFixture {
    fixture_id: String,
    source_memory_space_id: String,
    target_memory_space_id: String,
    channel: String,
    chat_id: String,
    recall_query: String,
    projection_query: String,
    expected_report_fragments: Vec<String>,
    candidates: Vec<MemoryWriteCandidate>,
}

#[test]
fn generic_and_beetle_derived_migration_outputs_replay_through_the_same_sdk_host_fixture_path() {
    let generic = load_fixture(include_str!(
        "../../../fixtures/sdk-host-readiness/generic-rust-host/host-turn-lifecycle.json"
    ));
    let beetle = load_fixture(include_str!(
        "../../../fixtures/sdk-host-readiness/beetle-derived/host-turn-lifecycle.json"
    ));

    let generic_report = migrate_then_replay(&generic);
    let beetle_report = migrate_then_replay(&beetle);

    assert!(generic_report.passed, "{:?}", generic_report.failures);
    assert!(beetle_report.passed, "{:?}", beetle_report.failures);
    assert_eq!(generic_report.operations_run, beetle_report.operations_run);
}

fn load_fixture(raw: &str) -> SdkHostReplayFixture {
    serde_json::from_str(raw).expect("sdk host readiness fixture")
}

fn migrate_then_replay(fixture: &SdkHostReplayFixture) -> bm_replay::ReplayRunReport {
    let profile = ProfileId::ServerLinuxDevFull;
    let source =
        StorePlatform::open(StoreBackendConfig::in_memory(profile).expect("source config"))
            .expect("source platform");
    let source_runtime = bm_sdk::MemoryRuntime::builder()
        .identity(MemoryIdentity::new("sdk-host-agent", "sdk-host-owner").expect("identity"))
        .scope(MemoryScope::new(&fixture.channel, &fixture.chat_id).expect("scope"))
        .profile(profile)
        .store_platform(source.clone())
        .build()
        .expect("runtime");

    source_runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: fixture.candidates.clone(),
        })
        .expect("write fixture candidates");
    let projection = source_runtime
        .project(MemoryProjectionRequest {
            user_query: fixture.projection_query.clone(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project before migration");
    assert!(!projection.system_memory_block.is_empty());

    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            memory_space_id: fixture.source_memory_space_id.clone(),
            include_private: true,
        },
    )
    .expect("export memory space");
    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_memory_space_id: fixture.source_memory_space_id.clone(),
        target_memory_space_id: fixture.target_memory_space_id.clone(),
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosEmbeddedSdk,
        snapshot: exported.snapshot.clone(),
    });
    assert!(!preview.loss_risk);

    let migrated =
        StorePlatform::open(StoreBackendConfig::in_memory(profile).expect("target config"))
            .expect("target platform");
    apply_memory_space_migration(
        &migrated,
        MemorySpaceMigrateApplyRequest {
            target_memory_space_id: fixture.target_memory_space_id.clone(),
            snapshot: exported.snapshot,
            preflight: preview.vault_preflight,
        },
    )
    .expect("apply migration");
    let migrated_snapshot = migrated.export_store_snapshot().expect("migrated snapshot");

    let replay_fixture = ReplayFixture {
        fixture_id: fixture.fixture_id.clone(),
        profile,
        store_snapshot: migrated_snapshot,
        operations: vec![
            ReplayOperation::Recall {
                query: fixture.recall_query.clone(),
                limit: 8,
            },
            ReplayOperation::Project {
                user_query: fixture.projection_query.clone(),
                system_max_len: 4096,
            },
            ReplayOperation::Inspect {
                query: fixture.recall_query.clone(),
                system_max_len: 4096,
            },
            ReplayOperation::Replay {
                chat_id: fixture.chat_id.clone(),
                limit: 8,
            },
        ],
        expected: ReplayExpectedOutcome {
            state_fingerprint: String::new(),
            event_fingerprint: String::new(),
            lifecycle_operations: vec![
                "recall".to_string(),
                "project".to_string(),
                "inspect".to_string(),
                "replay".to_string(),
            ],
            min_reports: 4,
            required_report_fragments: fixture.expected_report_fragments.clone(),
        },
    };

    let mut config = ReplayRunnerConfig::for_backend(
        StoreBackendConfig::in_memory(profile).expect("replay config"),
    )
    .expect("runner config");
    config.identity = MemoryIdentity::new("sdk-host-agent", "sdk-host-owner").expect("identity");
    config.scope = MemoryScope::new(&fixture.channel, &fixture.chat_id).expect("scope");
    run_replay_fixture(&replay_fixture, config).expect("run replay fixture")
}
