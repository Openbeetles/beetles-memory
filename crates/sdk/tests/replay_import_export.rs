#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    memory_facet_manifest_key, LongTermMemoryVersionMaterial, MEMORY_FACET_POSTING_NAMESPACE,
};
use bm_sdk::{
    LongTermMemoryQuery, MemoryArchiveScope, MemoryLongTermControlView, MemoryLongTermListRequest,
    MemoryLongTermMutation, MemoryLongTermMutationRequest, MemoryLongTermTarget,
    MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest, MemorySpaceExportRequest,
    MemorySpaceImportRequest, MemorySpacePrivateMaterialPolicy, PressureLevel,
    RuntimeLifecycleModeInput,
};

use support::{empty_store_platform, seeded_store_platform, test_runtime};

#[test]
fn typed_memory_space_archive_round_trips_only_with_the_runtime_scope() {
    let profile = support::host_test_profile();
    let source_runtime = test_runtime(seeded_store_platform(profile), profile);
    let scope = MemoryArchiveScope::subject(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .expect("source archive scope");
    let exported = source_runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .expect("typed export");
    assert_eq!(exported.projection_scope.scope, scope);
    assert_eq!(
        exported.projection_scope.private_material_policy,
        MemorySpacePrivateMaterialPolicy::IncludePrivate
    );

    let target_platform = empty_store_platform(profile);
    let target_runtime = test_runtime(target_platform.clone(), profile);
    let imported = target_runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope: scope.clone(),
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        })
        .expect("typed import");

    assert_eq!(imported.imported_scope, scope);
    assert_eq!(
        target_runtime
            .list_long_term_memory(MemoryLongTermListRequest {
                query: LongTermMemoryQuery {
                    limit: 20,
                    ..LongTermMemoryQuery::default()
                },
                cursor: None,
                limit: 20,
                view: MemoryLongTermControlView::HostUi,
            })
            .expect("target owner list")
            .records
            .len(),
        1
    );
    let manifest_key = memory_facet_manifest_key(
        target_runtime.memory_space_id(),
        target_runtime.subject_id(),
    )
    .expect("target facet manifest key");
    assert_eq!(
        target_platform
            .replay_harness()
            .read_json_docs_by_keys(
                MEMORY_FACET_POSTING_NAMESPACE,
                std::slice::from_ref(&manifest_key),
            )
            .expect("target facet manifest")
            .len(),
        1
    );
}

#[test]
fn continuity_import_preserves_soul_private_without_public_delivery_or_graph_membership() {
    const PRIVATE_SENTINEL: &str = "Verify release artifacts before publishing.";

    let profile = support::host_test_profile();
    let source_platform = seeded_store_platform(profile);
    let source_runtime = test_runtime(source_platform.clone(), profile);
    let owner_id = source_runtime
        .list_long_term_memory(MemoryLongTermListRequest {
            query: LongTermMemoryQuery {
                limit: 20,
                ..LongTermMemoryQuery::default()
            },
            cursor: None,
            limit: 20,
            view: MemoryLongTermControlView::RawOwner,
        })
        .expect("source owners")
        .records
        .into_iter()
        .find(|entry| entry.record.content == PRIVATE_SENTINEL)
        .expect("seeded owner")
        .record
        .id;
    let privacy_transition = source_runtime
        .mutate_long_term_memory(MemoryLongTermMutationRequest {
            operation: MemoryLongTermMutation::ChangePrivacy {
                target: MemoryLongTermTarget::RecordId(owner_id.clone()),
                privacy: MemoryPrivacyClass::SoulPrivate,
            },
            reason: "exercise private continuity import boundary".to_string(),
            dry_run: false,
            mode_input: RuntimeLifecycleModeInput::default(),
        })
        .expect("move source owner behind SoulPrivate boundary");
    assert!(privacy_transition.accepted);

    let scope = MemoryArchiveScope::subject(
        source_runtime.memory_space_id(),
        source_runtime.subject_id(),
    )
    .expect("source archive scope");
    let exported = source_runtime
        .export_memory_space(MemorySpaceExportRequest {
            scope: scope.clone(),
            private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
        })
        .expect("private typed export");
    assert_eq!(exported.projection_scope.scope, scope);
    assert_eq!(
        exported.projection_scope.private_material_policy,
        MemorySpacePrivateMaterialPolicy::IncludePrivate
    );
    let target_platform = empty_store_platform(profile);
    let target_runtime = test_runtime(target_platform.clone(), profile);
    target_runtime
        .import_memory_space(MemorySpaceImportRequest {
            scope,
            expected_private_material_policy: MemorySpacePrivateMaterialPolicy::IncludePrivate,
            archive: exported.archive,
        })
        .expect("private typed import");

    let imported_owner = target_platform
        .replay_harness()
        .read_json_namespace("long_term_version_materials")
        .expect("target immutable long-term materials")
        .into_iter()
        .map(|doc| {
            serde_json::from_value::<LongTermMemoryVersionMaterial>(doc.value)
                .expect("typed long-term material")
        })
        .find(|material| material.owner_ref.owner_id == owner_id && material.owner_revision == 2)
        .expect("private owner preserved");
    assert_eq!(
        imported_owner.privacy_class,
        MemoryPrivacyClass::SoulPrivate
    );
    let imported_facet = target_platform
        .replay_harness()
        .read_json_namespace("memory_facet_indexes")
        .expect("target facet indexes")
        .into_iter()
        .find(|doc| doc.value["owner_ref"]["owner_id"].as_str() == Some(owner_id.as_str()))
        .expect("private owner facet preserved");
    assert_eq!(
        imported_facet.value["privacy"].as_str(),
        Some("soul_private")
    );
    assert!(target_platform
        .replay_harness()
        .read_json_namespace("memory_facet_postings")
        .expect("target facet postings")
        .into_iter()
        .all(|doc| !serde_json::to_string(&doc.value)
            .expect("serialize posting")
            .contains(&owner_id)));
    assert!(target_platform
        .replay_harness()
        .export_store_snapshot()
        .expect("target graph closure")
        .json_docs
        .into_iter()
        .filter(|doc| doc.namespace.starts_with("memory_graph_"))
        .all(|doc| !serde_json::to_string(&doc.value)
            .expect("serialize graph document")
            .contains(&owner_id)));

    let recall = target_runtime
        .recall(MemoryRecallRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            query: "release artifacts publishing".to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .expect("target recall");
    let delivered = format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
        recall.working.long_term_memory_text,
        recall.working.shared_factual_plane,
        recall.working.continuity_capsule_text,
        recall.working.continuity_capsules,
        recall.working.archive_evidence_text,
        recall.compact_graph,
        recall.delivery_report,
    );
    assert!(!delivered.contains(PRIVATE_SENTINEL));
    let projection = target_runtime
        .project(MemoryProjectionRequest {
            temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
            structured_query_facets: Vec::new(),
            user_query: "How should release artifacts be published?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("target projection");
    assert!(!projection
        .provider_payload()
        .system_memory_block()
        .contains(PRIVATE_SENTINEL));
}

#[test]
fn legacy_continuity_snapshot_transfer_is_absent_from_the_sdk_surface() {
    let ops = include_str!("../src/ops.rs");
    let lib = include_str!("../src/lib.rs");
    let runtime = include_str!("../src/runtime.rs");

    for legacy_type in [
        "pub struct MemoryExportRequest",
        "pub struct MemoryExportReport",
        "pub struct MemoryImportRequest",
        "pub struct MemoryImportReport",
    ] {
        assert!(!ops.contains(legacy_type));
        assert!(!lib.contains(legacy_type));
    }
    assert!(!runtime.contains("pub fn export(&self, request: MemoryExportRequest)"));
    assert!(!runtime.contains("pub fn import(&self, request: MemoryImportRequest)"));
}
