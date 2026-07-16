use bm_sdk::{
    apply_memory_space_migration, export_memory_space, preview_memory_space_migration,
    MemoryIdentity, MemoryProjectionRequest, MemoryScope, MemorySpaceExportRequest,
    MemorySpaceMigrateApplyRequest, MemorySpaceMigratePreviewRequest,
    MemorySpacePrivateMaterialPolicy, MemorySpaceScope, MemoryStoreHandle, MemoryWriteCandidate,
    MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput, StoreBackendConfig,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SdkHostReplayFixture {
    target_memory_space_id: String,
    channel: String,
    chat_id: String,
    projection_query: String,
    candidates: Vec<MemoryWriteCandidate>,
}

#[test]
fn generic_and_beetle_derived_archives_reject_cross_identity_restore() {
    let generic = load_fixture(include_str!(
        "../../../fixtures/sdk-host-readiness/generic-rust-host/host-turn-lifecycle.json"
    ));
    let beetle = load_fixture(include_str!(
        "../../../fixtures/sdk-host-readiness/beetle-derived/host-turn-lifecycle.json"
    ));

    let generic_report = migrate_then_expect_identity_remap_preflight(&generic);
    let beetle_report = migrate_then_expect_identity_remap_preflight(&beetle);

    assert_eq!(generic_report.apply_error_stage, "memory_space_migration");
    assert_eq!(beetle_report.apply_error_stage, "memory_space_migration");
    assert!(generic_report.target_unchanged);
    assert!(beetle_report.target_unchanged);
    assert!(generic_report.facet_index_present);
    assert!(beetle_report.facet_index_present);
    assert!(generic_report.identity_remap_required);
    assert!(beetle_report.identity_remap_required);
    assert!(!generic_report.identity_remap_applied);
    assert!(!beetle_report.identity_remap_applied);
    assert!(!generic_report.preflight_passed);
    assert!(!beetle_report.preflight_passed);
}

fn load_fixture(raw: &str) -> SdkHostReplayFixture {
    serde_json::from_str(raw).expect("sdk host readiness fixture")
}

#[derive(Debug)]
struct MigrationFailClosedReport {
    facet_index_present: bool,
    identity_remap_required: bool,
    identity_remap_applied: bool,
    preflight_passed: bool,
    apply_error_stage: &'static str,
    target_unchanged: bool,
}

fn migrate_then_expect_identity_remap_preflight(
    fixture: &SdkHostReplayFixture,
) -> MigrationFailClosedReport {
    let profile = ProfileId::native_dev_full().expect("host-native dev-full profile");
    let source =
        MemoryStoreHandle::open(StoreBackendConfig::in_memory(profile).expect("source config"))
            .expect("source platform");
    let source_runtime = bm_sdk::MemoryRuntime::builder()
        .identity(MemoryIdentity::new("sdk-host-agent", "sdk-host-owner").expect("identity"))
        .scope(MemoryScope::new(&fixture.channel, &fixture.chat_id).expect("scope"))
        .store(source.clone())
        .build()
        .expect("runtime");

    source_runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: fixture.candidates.clone(),
        })
        .expect("write fixture candidates");
    let projection = source_runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: fixture.projection_query.clone(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project before migration");
    assert!(!projection.system_memory_block.is_empty());

    let source_scope = MemorySpaceScope {
        memory_space_id: source_runtime.memory_space_id().to_string(),
        mounted_subject_id: source_runtime.subject_id().to_string(),
    };
    let target_scope = MemorySpaceScope {
        memory_space_id: fixture.target_memory_space_id.clone(),
        mounted_subject_id: source_runtime.subject_id().to_string(),
    };
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            include_private: true,
        },
    )
    .expect("export memory space");
    let facet_index_present = exported
        .archive
        .contains_json_namespace("memory_facet_indexes");
    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_scope,
        target_scope,
        expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosEmbeddedSdk,
        archive: exported.archive.clone(),
    })
    .expect("preview migration");
    assert!(!preview.loss_risk);
    let preflight_passed = preview.vault_preflight.passed;
    let identity_remap_required = preview.manifest.identity_remap.required;
    let identity_remap_applied = preview.manifest.identity_remap.applied;

    let migrated =
        MemoryStoreHandle::open(StoreBackendConfig::in_memory(profile).expect("target config"))
            .expect("target platform");
    let before = migrated.export_replay_snapshot().expect("before");
    let apply_error = apply_memory_space_migration(
        &migrated,
        MemorySpaceMigrateApplyRequest { plan: preview.plan },
    )
    .expect_err("typed memory-space identity remap preflight must fail closed");
    let after = migrated.export_replay_snapshot().expect("after");

    MigrationFailClosedReport {
        facet_index_present,
        identity_remap_required,
        identity_remap_applied,
        preflight_passed,
        apply_error_stage: apply_error.stage(),
        target_unchanged: before.state_fingerprint() == after.state_fingerprint()
            && before.event_fingerprint() == after.event_fingerprint(),
    }
}
