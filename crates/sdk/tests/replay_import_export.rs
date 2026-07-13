#![cfg(feature = "nonproduction-replay-harness")]

mod support;

use bm_core::memory::{
    memory_facet_manifest_key, scoped_memory_facet_owner_storage_key, MEMORY_FACET_INDEX_NAMESPACE,
    MEMORY_FACET_POSTING_NAMESPACE,
};
use bm_sdk::{
    ContinuitySnapshotImportMode, LongTermMemoryDraft, LongTermMemoryKind, MemoryExportRequest,
    MemoryImportRequest, MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryReplayRequest, MemoryWriteRequest, ParsedLongTermMemoryExtraction, PressureLevel,
    ProfileId, RuntimeLifecycleModeInput,
};

use support::{
    empty_store_platform, seeded_store_platform, test_runtime, test_runtime_with_identity_scope,
};

#[test]
fn runtime_replay_export_import_go_through_sdk_contract() {
    let platform = seeded_store_platform(ProfileId::ServerLinuxDevFull);
    let runtime = test_runtime(platform, ProfileId::ServerLinuxDevFull);

    let replay = runtime
        .replay(MemoryReplayRequest {
            chat_id: "chat-1".to_string(),
            limit: 8,
        })
        .expect("replay");
    assert_eq!(replay.chat_id, "chat-1");

    let exported = runtime
        .export(MemoryExportRequest {
            chat_id: "chat-1".to_string(),
        })
        .expect("export");
    assert_eq!(exported.snapshot.chat_id, "chat-1");

    let imported = runtime
        .import(MemoryImportRequest {
            snapshot: exported.snapshot,
            target_chat_id: "chat-2".to_string(),
            mode: ContinuitySnapshotImportMode::FullRestore,
        })
        .expect("import");
    assert!(imported
        .outcome
        .decisions
        .iter()
        .any(|decision| decision.layer == "long_term_memory"));
}

#[test]
fn continuity_import_commits_owner_facet_and_lifecycle_atomically() {
    let source_platform = seeded_store_platform(ProfileId::ServerLinuxDevFull);
    let source_runtime = test_runtime(source_platform, ProfileId::ServerLinuxDevFull);
    let snapshot = source_runtime
        .export(MemoryExportRequest {
            chat_id: "chat-1".to_string(),
        })
        .expect("export source snapshot")
        .snapshot;
    assert_eq!(snapshot.long_term_memory.len(), 1);

    let target_platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let target_runtime = test_runtime_with_identity_scope(
        target_platform.clone(),
        ProfileId::ServerLinuxDevFull,
        "target-agent",
        "target-owner",
        "llm.gateway",
        "chat-target",
    );
    let report = target_runtime
        .import(MemoryImportRequest {
            snapshot,
            target_chat_id: "chat-target".to_string(),
            mode: ContinuitySnapshotImportMode::FullRestore,
        })
        .expect("import target snapshot");

    assert_eq!(report.outcome.long_term_imported, 1);
    assert_eq!(
        target_platform
            .replay_harness()
            .scoped_long_term_memory_read_store("space:target-owner")
            .expect("target owner store")
            .count()
            .expect("target owner count"),
        1
    );
    let manifest_key = memory_facet_manifest_key("space:target-owner", target_runtime.subject_id())
        .expect("target manifest key");
    assert_eq!(
        target_platform
            .replay_harness()
            .read_json_docs_by_keys(
                MEMORY_FACET_POSTING_NAMESPACE,
                std::slice::from_ref(&manifest_key),
            )
            .expect("target manifest read")
            .len(),
        1
    );
}

#[test]
fn continuity_import_preserves_soul_private_without_public_delivery_or_graph_membership() {
    let source_platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let source_runtime = test_runtime(source_platform, ProfileId::ServerLinuxDevFull);
    source_runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Fact,
                    topic: "continuity_private_owner".to_string(),
                    content: "CONTINUITY_SOUL_PRIVATE_SENTINEL must remain private.".to_string(),
                    keywords: vec!["continuity".to_string(), "private".to_string()],
                    privacy: MemoryPrivacyClass::SoulPrivate,
                    source_chat_id: Some("chat-1".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["private://continuity-owner".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    last_confirmed_at: Some(1_800_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed soul-private source owner");
    let snapshot = source_runtime
        .export(MemoryExportRequest {
            chat_id: "chat-1".to_string(),
        })
        .expect("export soul-private snapshot")
        .snapshot;
    assert!(snapshot
        .long_term_memory
        .iter()
        .any(|entry| entry.content.contains("CONTINUITY_SOUL_PRIVATE_SENTINEL")));

    let target_platform = empty_store_platform(ProfileId::ServerLinuxDevFull);
    let target_runtime = test_runtime_with_identity_scope(
        target_platform.clone(),
        ProfileId::ServerLinuxDevFull,
        "target-agent",
        "target-owner",
        "llm.gateway",
        "chat-target",
    );
    let report = target_runtime
        .import(MemoryImportRequest {
            snapshot,
            target_chat_id: "chat-target".to_string(),
            mode: ContinuitySnapshotImportMode::FullRestore,
        })
        .expect("import soul-private snapshot");
    let transaction = report.transaction.expect("import transaction proof");
    assert_eq!(transaction.operation, "import");
    assert_eq!(
        transaction.planned_mutations,
        transaction.committed_mutations
    );
    assert!(!transaction.partial_write);

    let owners = target_platform
        .replay_harness()
        .scoped_long_term_memory_read_store("space:target-owner")
        .expect("target owner store")
        .list(usize::MAX)
        .expect("target owners");
    let private_owner = owners
        .iter()
        .find(|entry| entry.content.contains("CONTINUITY_SOUL_PRIVATE_SENTINEL"))
        .expect("imported private owner");
    assert_eq!(private_owner.privacy, MemoryPrivacyClass::SoulPrivate);
    let facet_key = scoped_memory_facet_owner_storage_key(
        "space:target-owner",
        target_runtime.subject_id(),
        &private_owner.id,
    )
    .expect("private owner facet key");
    let facet_docs = target_platform
        .replay_harness()
        .read_json_docs_by_keys(MEMORY_FACET_INDEX_NAMESPACE, &[facet_key])
        .expect("private owner facet read");
    assert_eq!(facet_docs.len(), 1);
    assert_eq!(facet_docs[0].value["privacy"], "soul_private");
    let manifest_key = memory_facet_manifest_key("space:target-owner", target_runtime.subject_id())
        .expect("target manifest key");
    let manifest_docs = target_platform
        .replay_harness()
        .read_json_docs_by_keys(MEMORY_FACET_POSTING_NAMESPACE, &[manifest_key])
        .expect("target manifest read");
    assert!(manifest_docs.is_empty());
    assert!(target_platform
        .replay_harness()
        .read_json_namespace(MEMORY_FACET_POSTING_NAMESPACE)
        .expect("target posting namespace")
        .iter()
        .all(|doc| !serde_json::to_string(&doc.value)
            .expect("serialize posting doc")
            .contains(&private_owner.id)));
    assert!(target_platform
        .replay_harness()
        .export_store_snapshot()
        .expect("target store snapshot")
        .json_docs
        .iter()
        .filter(|doc| doc.namespace.starts_with("memory_graph_"))
        .all(|doc| !serde_json::to_string(&doc.value)
            .expect("serialize graph doc")
            .contains(&private_owner.id)));

    let recall = target_runtime
        .recall(MemoryRecallRequest {
            structured_query_facets: Vec::new(),
            query: "continuity private owner".to_string(),
            limit: 8,
            tool_registry_refs: Vec::new(),
        })
        .expect("private import recall");
    assert!(
        !format!("{:?}{:?}", recall.compact_graph, recall.delivery_report)
            .contains("CONTINUITY_SOUL_PRIVATE_SENTINEL")
    );
    let projection = target_runtime
        .project(MemoryProjectionRequest {
            structured_query_facets: Vec::new(),
            user_query: "What continuity owner is available?".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 8,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            tool_registry_refs: Vec::new(),
        })
        .expect("private import projection");
    assert!(!projection
        .system_memory_block
        .contains("CONTINUITY_SOUL_PRIVATE_SENTINEL"));
}
