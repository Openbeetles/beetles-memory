#![cfg(feature = "sqlite-store")]

use bm_core::budget::StoreRuntimeBudget;
use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    build_long_term_memory_facet_index_doc, canonical_evidence_ref_from_source,
    plan_long_term_memory_upsert, scoped_long_term_memory_storage_key,
    scoped_memory_facet_owner_storage_key, CanonicalEntityKey, CanonicalEntityKind,
    CanonicalEntityRef, LongTermMemoryConfidence, LongTermMemoryDraft, LongTermMemoryEntry,
    LongTermMemoryEntryPlan, LongTermMemoryFreshness, LongTermMemoryKind,
    LongTermMemorySourceScope, LongTermMemorySourceType, LongTermMemoryStaleHint,
    MemoryPrivacyClass, MEMORY_FACET_INDEX_NAMESPACE,
};
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{
    MemoryStoreEventKind, StoreBackendConfig, StoreEventScope, StoreJsonPrecondition,
    StoreMutation, StoreMutationBatch, StorePlatform,
};

fn seed_private_archive(
    platform: &StorePlatform,
    memory_space_id: &str,
    draft: &LongTermMemoryDraft,
    now_secs: u64,
) -> LongTermMemoryEntry {
    let entry = match plan_long_term_memory_upsert(None, draft, now_secs) {
        LongTermMemoryEntryPlan::Created(entry) => entry,
        other => panic!("new archive fixture must be created, got {other:?}"),
    };
    let subject_id = "subject:test";
    let owner_key =
        scoped_long_term_memory_storage_key(memory_space_id, &entry.id).expect("owner key");
    let facet = build_long_term_memory_facet_index_doc(
        &entry,
        memory_space_id,
        vec![subject_id.to_string()],
        1,
    );
    let facet_key = scoped_memory_facet_owner_storage_key(memory_space_id, subject_id, &entry.id)
        .expect("facet key");
    platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: format!("seed-private-archive-{}", entry.id),
                operation: "test.seed_private_archive".to_string(),
                scope: StoreEventScope::new("agent:test", "owner:test", "test", "chat-a")
                    .with_memory_space(memory_space_id)
                    .with_subject(subject_id),
                mutations: vec![
                    StoreMutation::PutJson {
                        namespace: "long_term".to_string(),
                        key: owner_key.clone(),
                        value: serde_json::to_value(&entry).expect("serialize archive owner"),
                        event_kind: MemoryStoreEventKind::MemoryWrite,
                        plane: "long_term".to_string(),
                        record_key: entry.id.clone(),
                    },
                    StoreMutation::PutJson {
                        namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                        key: facet_key.clone(),
                        value: serde_json::to_value(&facet).expect("serialize archive facet"),
                        event_kind: MemoryStoreEventKind::MemoryWrite,
                        plane: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                        record_key: format!("facet-owner:{}", entry.id),
                    },
                ],
            },
            &[
                StoreJsonPrecondition::Absent {
                    namespace: "long_term".to_string(),
                    key: owner_key,
                },
                StoreJsonPrecondition::Absent {
                    namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                    key: facet_key,
                },
            ],
        )
        .expect("seed private archive transaction");
    entry
}

fn tiny_store_budget() -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        event_log_max_items: 2,
        kv_max_entries: 8,
        blob_max_bytes: 4,
        snapshot_max_bytes: 1024 * 1024,
        logical_namespace_max_bytes: 64,
        logical_key_max_bytes: 32,
        event_record_key_max_bytes: 32,
        export_max_bytes: 1024 * 1024,
        import_max_bytes: 1024 * 1024,
    }
}

#[test]
fn sqlite_store_persists_core_runtime_paths_across_reopen() {
    let root =
        std::env::temp_dir().join(format!("beetle-memory-sqlite-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let path = root.join("memory.sqlite3");
    let config = StoreBackendConfig::sqlite(&path, ProfileId::ServerLinuxMemoryGateway).unwrap();

    {
        let platform = StorePlatform::open(config.clone()).unwrap();
        platform
            .state_fs()
            .write("runtime/state.json", b"state")
            .unwrap();
        platform.skill_storage().write("alpha", b"skill").unwrap();
        platform
            .session_store()
            .append("chat-a", "user", "hello")
            .unwrap();
        seed_private_archive(
            &platform,
            "space:test",
            &LongTermMemoryDraft {
                kind: LongTermMemoryKind::Fact,
                topic: "sqlite archive recall".to_string(),
                content: "Archive memory remains searchable after a SQLite store reopen."
                    .to_string(),
                keywords: vec![
                    "archive".to_string(),
                    "sqlite".to_string(),
                    "reopen".to_string(),
                ],
                privacy: MemoryPrivacyClass::PrivateGarden,
                source_chat_id: Some("chat-a".to_string()),
                source_type: Some(LongTermMemorySourceType::Conversation),
                source_scope: Some(LongTermMemorySourceScope::User),
                confidence: Some(LongTermMemoryConfidence::High),
                freshness: Some(LongTermMemoryFreshness::Stable),
                stale_hint: Some(LongTermMemoryStaleHint::ReviewBeforeUse),
                supporting_citations: vec!["transcript:space:test:chat-a:turn-archive".to_string()],
                canonical_entities: vec![CanonicalEntityRef {
                    key: CanonicalEntityKey {
                        kind: CanonicalEntityKind::System,
                        canonical_id: "sqlite-memory-archive".to_string(),
                    },
                    display_label: Some("SQLite memory archive".to_string()),
                    aliases: vec!["archive store".to_string(), "sqlite archive".to_string()],
                    evidence_refs: vec![canonical_evidence_ref_from_source(
                        "transcript:space:test:chat-a:turn-archive",
                    )
                    .expect("canonical archive evidence")],
                }],
                evidence_count: Some(1),
                observed_at: Some(100),
                last_confirmed_at: Some(100),
                source_revision: Some(1),
            },
            100,
        );
        assert!(platform
            .read_events()
            .unwrap()
            .iter()
            .any(|event| event.kind_name == "memory.write"));
    }

    let reopened = StorePlatform::open(config).unwrap();
    assert_eq!(
        reopened.state_fs().read("runtime/state.json").unwrap(),
        Some(b"state".to_vec())
    );
    assert_eq!(reopened.skill_storage().read("alpha").unwrap(), b"skill");
    assert_eq!(
        reopened
            .scoped_long_term_memory_read_store("space:test")
            .expect("scoped long-term read store")
            .recall("archive", Some("chat-a"), 4)
            .unwrap()
            .len(),
        1
    );
    assert!(reopened.read_events().unwrap().len() >= 5);
}

#[test]
fn sqlite_store_rejects_profile_mismatch_on_existing_database() {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-sqlite-store-mismatch-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let path = root.join("memory.sqlite3");

    StorePlatform::open(
        StoreBackendConfig::sqlite(&path, ProfileId::DesktopMacosEmbeddedSdk).unwrap(),
    )
    .unwrap();

    let err = match StorePlatform::open(
        StoreBackendConfig::sqlite(&path, ProfileId::ServerLinuxMemoryGateway).unwrap(),
    ) {
        Ok(_) => panic!("existing sqlite store must reject a different profile"),
        Err(error) => error,
    };

    assert_eq!(err.stage(), "sqlite_store_schema");
    assert!(err.to_string().contains("profile"));
}

#[test]
fn sqlite_store_consumes_event_key_and_blob_budgets() {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-sqlite-store-budget-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let path = root.join("memory.sqlite3");
    let config = StoreBackendConfig::sqlite(&path, ProfileId::ServerLinuxMemoryGateway)
        .unwrap()
        .with_runtime_store_budget(tiny_store_budget());
    let platform = StorePlatform::open(config).unwrap();

    platform
        .state_fs()
        .write("first", b"12")
        .expect("open event plus first write reaches the event cap");
    let err = platform
        .state_fs()
        .write("second", b"1")
        .expect_err("sqlite event cap must reject the write before mutating state");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("event log"));
    assert_eq!(platform.state_fs().read("second").unwrap(), None);

    let root = std::env::temp_dir().join(format!(
        "beetle-memory-sqlite-store-blob-budget-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let path = root.join("memory.sqlite3");
    let mut blob_budget = tiny_store_budget();
    blob_budget.event_log_max_items = 8;
    let config = StoreBackendConfig::sqlite(&path, ProfileId::ServerLinuxMemoryGateway)
        .unwrap()
        .with_runtime_store_budget(blob_budget);
    let platform = StorePlatform::open(config).unwrap();

    platform.state_fs().write("a", b"12").unwrap();
    platform.state_fs().write("b", b"12").unwrap();
    let err = platform
        .state_fs()
        .write("c", b"1")
        .expect_err("sqlite must enforce cumulative blob budget");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("blob bytes"));

    let oversized_key = "k".repeat(33);
    let err = platform
        .state_fs()
        .write(&oversized_key, b"1")
        .expect_err("sqlite must reject oversized logical keys");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("logical key"));
}
