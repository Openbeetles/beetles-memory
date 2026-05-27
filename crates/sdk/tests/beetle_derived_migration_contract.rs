mod support;

use bm_sdk::{
    apply_memory_space_migration, export_memory_space, import_memory_space,
    preview_memory_space_migration, MemoryInspectionRequest, MemoryProjectionRequest,
    MemoryRecallRequest, MemorySpaceExportRequest, MemorySpaceImportRequest,
    MemorySpaceMigrateApplyRequest, MemorySpaceMigratePreviewRequest, MemoryWriteCandidate,
    MemoryWriteRequest, PressureLevel, ProfileId, RuntimeLifecycleModeInput,
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
    recall_query: String,
    projection_query: String,
    expected_runtime_skill: String,
    expected_fragments: Vec<String>,
    candidates: Vec<MemoryWriteCandidate>,
}

#[test]
fn generic_and_beetle_derived_fixtures_use_the_same_public_sdk_migrator_path() {
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
}

#[derive(Debug)]
struct FixtureExerciseReport {
    helper_path: &'static str,
    preview_json_docs: usize,
    soul_handoffs: usize,
    deferred_candidates: usize,
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

    let preview = preview_memory_space_migration(MemorySpaceMigratePreviewRequest {
        source_memory_space_id: fixture.source_memory_space_id.clone(),
        target_memory_space_id: fixture.target_memory_space_id.clone(),
        source_profile: profile,
        target_profile: ProfileId::DesktopMacosEmbeddedSdk,
        snapshot: exported.snapshot.clone(),
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
    let apply = apply_memory_space_migration(
        &target,
        MemorySpaceMigrateApplyRequest {
            target_memory_space_id: fixture.target_memory_space_id.clone(),
            snapshot: exported.snapshot.clone(),
            preflight: preview.vault_preflight.clone(),
        },
    )
    .expect("apply memory-space migration");
    assert_eq!(
        apply.import_report.state_fingerprint,
        preview.state_fingerprint
    );

    let import = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            memory_space_id: fixture.target_memory_space_id.clone(),
            snapshot: exported.snapshot,
        },
    )
    .expect("import memory space");
    assert_eq!(
        import.import_report.state_fingerprint,
        preview.state_fingerprint
    );

    let target_runtime = test_runtime_with_scope(
        target,
        profile,
        &fixture.channel,
        &format!("{}-post-migration", fixture.chat_id),
    );
    let recall = target_runtime
        .recall(MemoryRecallRequest {
            query: fixture.recall_query.clone(),
            limit: 8,
        })
        .expect("recall after migration");
    let recalled_skill_names = recall
        .procedural_hits
        .iter()
        .map(|hit| hit.record.name.as_str())
        .collect::<Vec<_>>();
    assert!(
        recalled_skill_names
            .iter()
            .any(|name| *name == fixture.expected_runtime_skill),
        "fixture {} did not recall expected runtime skill {}; recalled {:?}",
        fixture.fixture_id,
        fixture.expected_runtime_skill,
        recalled_skill_names
    );

    let inspect = target_runtime
        .inspect(MemoryInspectionRequest {
            query: fixture.recall_query.clone(),
            system_max_len: 4096,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("operator inspect after migration");
    assert!(inspect.capabilities.inspection.visible);
    assert!(inspect.working.runtime_skill_report.selected_count > 0);

    let projection = target_runtime
        .project(MemoryProjectionRequest {
            user_query: fixture.projection_query.clone(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("project after migration");
    let operator_text = format!(
        "{}\n{}\n{}",
        projection.system_memory_block,
        inspect.working.runtime_skill_text.unwrap_or_default(),
        inspect.working.long_term_memory_text.unwrap_or_default()
    );
    for fragment in &fixture.expected_fragments {
        assert!(
            operator_text.contains(fragment),
            "fixture {} missing post-migration fragment {fragment}",
            fixture.fixture_id
        );
    }

    FixtureExerciseReport {
        helper_path: "public-sdk-write-export-preview-apply-import-inspect",
        preview_json_docs: preview.json_docs,
        soul_handoffs,
        deferred_candidates,
    }
}
