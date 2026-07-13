#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_sdk::{
    apply_memory_space_migration, export_memory_space, import_memory_space,
    preview_memory_space_migration, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpaceMigrateApplyRequest, MemorySpaceMigratePreviewRequest, MemoryWriteCandidate,
    MemoryWriteRequest, ProfileId,
};
use serde::Deserialize;

use support::{empty_store_platform, test_runtime_with_scope};

#[derive(Debug, Deserialize)]
struct SdkHostMigrationFixture {
    fixture_id: String,
    source_memory_space_id: String,
    target_memory_space_id: String,
    channel: String,
    chat_id: String,
    candidates: Vec<MemoryWriteCandidate>,
}

#[test]
fn generic_and_beetle_derived_fixtures_fail_closed_until_facet_remap() {
    let generic = load_fixture(include_str!(
        "../../../fixtures/sdk-host-readiness/generic-rust-host/host-turn-lifecycle.json"
    ));
    let beetle = load_fixture(include_str!(
        "../../../fixtures/sdk-host-readiness/beetle-derived/host-turn-lifecycle.json"
    ));

    let generic_report = exercise_fixture_through_public_sdk(&generic);
    let beetle_report = exercise_fixture_through_public_sdk(&beetle);

    assert_eq!(generic_report.helper_path, beetle_report.helper_path);
    assert!(generic_report.preview_json_docs > 0);
    assert!(beetle_report.preview_json_docs >= generic_report.preview_json_docs);
    assert!(beetle_report.soul_handoffs >= 1);
    assert!(beetle_report.deferred_candidates >= 1);
    assert!(generic_report.facet_index_present);
    assert!(beetle_report.facet_index_present);
    assert!(!generic_report.preflight_passed);
    assert!(!beetle_report.preflight_passed);
    assert_eq!(generic_report.apply_error_stage, "memory_space_migration");
    assert_eq!(beetle_report.apply_error_stage, "memory_space_migration");
    assert_eq!(generic_report.import_error_stage, "memory_space_import");
    assert_eq!(beetle_report.import_error_stage, "memory_space_import");
    assert!(generic_report.target_unchanged);
    assert!(beetle_report.target_unchanged);
}

#[derive(Debug)]
struct FixtureExerciseReport {
    helper_path: &'static str,
    preview_json_docs: usize,
    soul_handoffs: usize,
    deferred_candidates: usize,
    facet_index_present: bool,
    preflight_passed: bool,
    apply_error_stage: &'static str,
    import_error_stage: &'static str,
    target_unchanged: bool,
}

fn load_fixture(raw: &str) -> SdkHostMigrationFixture {
    serde_json::from_str(raw).expect("sdk host readiness fixture")
}

fn exercise_fixture_through_public_sdk(fixture: &SdkHostMigrationFixture) -> FixtureExerciseReport {
    let profile = ProfileId::ServerLinuxDevFull;
    let source = empty_store_platform(profile);
    let source_runtime =
        test_runtime_with_scope(source.clone(), profile, &fixture.channel, &fixture.chat_id);

    let write = source_runtime
        .write(MemoryWriteRequest::Candidates {
            candidates: fixture.candidates.clone(),
        })
        .expect("write fixture candidates");
    assert_eq!(write.operation, "write.candidates");
    assert!(
        write.changed > 0,
        "fixture {} changed no memory",
        fixture.fixture_id
    );

    let semantic = write
        .semantic_governance
        .as_ref()
        .expect("candidate governance report");
    let soul_handoffs = semantic.soul_candidate_handoffs.len();
    let deferred_candidates = semantic.deferred_count;

    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            memory_space_id: fixture.source_memory_space_id.clone(),
            include_private: true,
        },
    )
    .expect("export memory space");
    let facet_index_present = exported
        .archive
        .contains_json_namespace("memory_facet_indexes");

    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_memory_space_id: fixture.source_memory_space_id.clone(),
        target_memory_space_id: fixture.target_memory_space_id.clone(),
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosEmbeddedSdk,
        archive: exported.archive.clone(),
    });
    assert!(
        !preview.loss_risk,
        "fixture {} has loss risk",
        fixture.fixture_id
    );
    assert_eq!(
        preview.state_fingerprint,
        exported.export_report.state_fingerprint
    );

    let target = empty_store_platform(profile);
    let before = target
        .replay_harness()
        .export_store_snapshot()
        .expect("before");
    let preflight_passed = preview.vault_preflight.passed;
    let apply_error = apply_memory_space_migration(
        &target,
        MemorySpaceMigrateApplyRequest {
            plan: preview.plan.clone(),
        },
    )
    .expect_err("facet index remap preflight must fail closed");

    let import_error = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            memory_space_id: fixture.target_memory_space_id.clone(),
            archive: exported.archive,
        },
    )
    .expect_err("direct import must fail closed");
    let after = target
        .replay_harness()
        .export_store_snapshot()
        .expect("after");

    FixtureExerciseReport {
        helper_path: "public-sdk-write-export-preview-facet-remap-preflight",
        preview_json_docs: preview.json_docs,
        soul_handoffs,
        deferred_candidates,
        facet_index_present,
        preflight_passed,
        apply_error_stage: apply_error.stage(),
        import_error_stage: import_error.stage(),
        target_unchanged: before.state_fingerprint() == after.state_fingerprint()
            && before.event_fingerprint() == after.event_fingerprint(),
    }
}
