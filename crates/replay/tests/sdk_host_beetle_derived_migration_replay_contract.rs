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
    source_memory_space_id: String,
    target_memory_space_id: String,
    channel: String,
    chat_id: String,
    projection_query: String,
    candidates: Vec<MemoryWriteCandidate>,
}

#[test]
fn generic_and_beetle_derived_migration_outputs_fail_closed_until_facet_remap() {
    let generic = load_fixture(include_str!(
        "../../../fixtures/sdk-host-readiness/generic-rust-host/host-turn-lifecycle.json"
    ));
    let beetle = load_fixture(include_str!(
        "../../../fixtures/sdk-host-readiness/beetle-derived/host-turn-lifecycle.json"
    ));

    let generic_report = migrate_then_expect_facet_remap_preflight(&generic);
    let beetle_report = migrate_then_expect_facet_remap_preflight(&beetle);

    assert_eq!(generic_report.apply_error_stage, "memory_space_migration");
    assert_eq!(beetle_report.apply_error_stage, "memory_space_migration");
    assert!(generic_report.target_unchanged);
    assert!(beetle_report.target_unchanged);
    assert!(generic_report.facet_index_present);
    assert!(beetle_report.facet_index_present);
    assert!(!generic_report.preflight_passed);
    assert!(!beetle_report.preflight_passed);
}

fn load_fixture(raw: &str) -> SdkHostReplayFixture {
    serde_json::from_str(raw).expect("sdk host readiness fixture")
}

#[derive(Debug)]
struct MigrationFailClosedReport {
    facet_index_present: bool,
    preflight_passed: bool,
    apply_error_stage: &'static str,
    target_unchanged: bool,
}

fn migrate_then_expect_facet_remap_preflight(
    fixture: &SdkHostReplayFixture,
) -> MigrationFailClosedReport {
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
    let facet_index_present = exported
        .snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace == "memory_facet_indexes");
    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_memory_space_id: fixture.source_memory_space_id.clone(),
        target_memory_space_id: fixture.target_memory_space_id.clone(),
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosEmbeddedSdk,
        snapshot: exported.snapshot.clone(),
    });
    assert!(!preview.loss_risk);
    let preflight_passed = preview.vault_preflight.passed;

    let migrated =
        StorePlatform::open(StoreBackendConfig::in_memory(profile).expect("target config"))
            .expect("target platform");
    let before = migrated.export_store_snapshot().expect("before");
    let apply_error = apply_memory_space_migration(
        &migrated,
        MemorySpaceMigrateApplyRequest {
            target_memory_space_id: fixture.target_memory_space_id.clone(),
            snapshot: exported.snapshot,
            preflight: preview.vault_preflight,
        },
    )
    .expect_err("facet index remap preflight must fail closed");
    let after = migrated.export_store_snapshot().expect("after");

    MigrationFailClosedReport {
        facet_index_present,
        preflight_passed,
        apply_error_stage: apply_error.stage(),
        target_unchanged: before.state_fingerprint() == after.state_fingerprint()
            && before.event_fingerprint() == after.event_fingerprint(),
    }
}
