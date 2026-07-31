use bm_sdk::nonproduction_replay_harness::{export_memory_space, import_memory_space};
use bm_sdk::{
    MemoryArchiveScope, MemoryIdentity, MemoryProjectionRequest, MemoryScope,
    MemorySpaceExportRequest, MemorySpaceImportRequest, MemorySpacePrivateMaterialPolicy,
    MemoryStoreHandle, MemoryWriteCandidate, MemoryWriteRequest, PressureLevel, ProfileId,
    RuntimeLifecycleModeInput, RuntimeSkillOwningScope, StoreBackendConfig,
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

    let generic_report = restore_then_expect_exact_identity_rejection(&generic);
    let beetle_report = restore_then_expect_exact_identity_rejection(&beetle);

    assert_eq!(generic_report.restore_error_stage, "memory_archive_scope");
    assert_eq!(beetle_report.restore_error_stage, "memory_archive_scope");
    assert!(generic_report.target_unchanged);
    assert!(beetle_report.target_unchanged);
    assert!(generic_report.facet_index_present);
    assert!(beetle_report.facet_index_present);
}

fn load_fixture(raw: &str) -> SdkHostReplayFixture {
    serde_json::from_str(raw).expect("sdk host readiness fixture")
}

#[derive(Debug)]
struct ArchiveFailClosedReport {
    facet_index_present: bool,
    restore_error_stage: &'static str,
    target_unchanged: bool,
}

fn restore_then_expect_exact_identity_rejection(
    fixture: &SdkHostReplayFixture,
) -> ArchiveFailClosedReport {
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
            runtime_skill_owning_scope: Some(RuntimeSkillOwningScope::Subject {
                mounted_subject_id: source_runtime.subject_id().to_string(),
            }),
            candidates: fixture.candidates.clone(),
        })
        .expect("write fixture candidates");
    let projection = source_runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: fixture.projection_query.clone(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("project before migration");
    assert!(!projection
        .provider_payload()
        .system_memory_block()
        .is_empty());

    let source_scope = MemoryArchiveScope::subject(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .expect("source archive scope");
    let target_scope =
        MemoryArchiveScope::subject(&fixture.target_memory_space_id, source_runtime.subject_id())
            .expect("target archive scope");
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
    let target =
        MemoryStoreHandle::open(StoreBackendConfig::in_memory(profile).expect("target config"))
            .expect("target platform");
    let before = target.export_replay_snapshot().expect("before");
    let restore_error = import_memory_space(
        &target,
        MemorySpaceImportRequest {
            scope: target_scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        },
    )
    .expect_err("cross-identity archive restore must fail closed");
    let after = target.export_replay_snapshot().expect("after");

    ArchiveFailClosedReport {
        facet_index_present,
        restore_error_stage: restore_error.stage(),
        target_unchanged: before == after,
    }
}
