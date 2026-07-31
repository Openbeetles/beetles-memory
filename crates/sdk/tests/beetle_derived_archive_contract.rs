#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_sdk::nonproduction_replay_harness::{export_memory_space, import_memory_space};
use bm_sdk::{
    MemoryArchiveScope, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpacePrivateMaterialPolicy, MemoryWriteCandidate, MemoryWriteRequest,
};
use serde::Deserialize;

use support::{empty_store_platform, test_runtime_with_scope};

#[derive(Debug, Deserialize)]
struct SdkHostMigrationFixture {
    fixture_id: String,
    target_memory_space_id: String,
    channel: String,
    chat_id: String,
    candidates: Vec<MemoryWriteCandidate>,
}

#[test]
fn generic_and_beetle_derived_fixtures_restore_same_scope_and_reject_cross_identity() {
    let generic = load_fixture(include_str!(
        "../../../fixtures/sdk-host-readiness/generic-rust-host/host-turn-lifecycle.json"
    ));
    let beetle = load_fixture(include_str!(
        "../../../fixtures/sdk-host-readiness/beetle-derived/host-turn-lifecycle.json"
    ));

    let generic_report = exercise_fixture_through_public_sdk(&generic);
    let beetle_report = exercise_fixture_through_public_sdk(&beetle);

    assert_eq!(generic_report.helper_path, beetle_report.helper_path);
    assert!(generic_report.exported_json_docs > 0);
    assert!(beetle_report.exported_json_docs >= generic_report.exported_json_docs);
    assert!(beetle_report.soul_handoffs >= 1);
    assert!(beetle_report.deferred_candidates >= 1);
    assert!(generic_report.facet_index_present);
    assert!(beetle_report.facet_index_present);
    assert!(generic_report.same_scope_restored);
    assert!(beetle_report.same_scope_restored);
    assert_eq!(generic_report.import_error_stage, "memory_archive_scope");
    assert_eq!(beetle_report.import_error_stage, "memory_archive_scope");
}

#[derive(Debug)]
struct FixtureExerciseReport {
    helper_path: &'static str,
    exported_json_docs: usize,
    soul_handoffs: usize,
    deferred_candidates: usize,
    facet_index_present: bool,
    same_scope_restored: bool,
    import_error_stage: &'static str,
}

fn load_fixture(raw: &str) -> SdkHostMigrationFixture {
    serde_json::from_str(raw).expect("sdk host readiness fixture")
}

fn exercise_fixture_through_public_sdk(fixture: &SdkHostMigrationFixture) -> FixtureExerciseReport {
    let profile = support::host_test_profile();
    let source = empty_store_platform(profile);
    let source_runtime =
        test_runtime_with_scope(source.clone(), profile, &fixture.channel, &fixture.chat_id);

    let write = source_runtime
        .write(MemoryWriteRequest::Candidates {
            runtime_skill_owning_scope: Some(support::runtime_skill_subject_scope()),
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

    let source_scope = MemoryArchiveScope::subject(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .expect("source archive scope");
    let target_scope =
        MemoryArchiveScope::subject(&fixture.target_memory_space_id, source_runtime.subject_id())
            .expect("cross-identity archive scope");
    let exported = export_memory_space(
        &source,
        MemorySpaceExportRequest {
            scope: source_scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export memory space");
    let facet_index_present = exported
        .archive
        .contains_json_namespace("memory_facet_indexes");
    let exported_json_docs = exported.archive.root().json_doc_count as usize;

    let cross_identity_target = empty_store_platform(profile);
    let import_error = import_memory_space(
        &cross_identity_target,
        MemorySpaceImportRequest {
            scope: target_scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive.clone(),
        },
    )
    .expect_err("direct import must fail closed");

    let same_scope_target = empty_store_platform(profile);
    let restored = import_memory_space(
        &same_scope_target,
        MemorySpaceImportRequest {
            scope: source_scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        },
    )
    .expect("same-scope fixture restore");
    let restored_projection = export_memory_space(
        &same_scope_target,
        MemorySpaceExportRequest {
            scope: source_scope,
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        },
    )
    .expect("export restored fixture projection");

    FixtureExerciseReport {
        helper_path: "public-sdk-write-export-import-same-scope",
        exported_json_docs,
        soul_handoffs,
        deferred_candidates,
        facet_index_present,
        same_scope_restored: restored.inserted_json_docs > 0
            && restored.archive_root == *restored_projection.archive.root()
            && restored_projection
                .archive
                .contains_json_namespace("memory_facet_indexes"),
        import_error_stage: import_error.stage(),
    }
}
