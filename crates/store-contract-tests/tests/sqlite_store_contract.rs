#![cfg(feature = "sqlite-store")]

mod support;

use bm_core::budget::StoreRuntimeBudget;
use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    canonical_evidence_ref_from_source, CanonicalEntityKey, CanonicalEntityKind,
    CanonicalEntityRef, LongTermMemoryConfidence, LongTermMemoryDraft, LongTermMemoryEntry,
    LongTermMemoryFreshness, LongTermMemoryKind, LongTermMemoryProvenance,
    LongTermMemorySourceScope, LongTermMemorySourceType, LongTermMemoryStaleHint,
    MemoryEvidenceAuthority, MemoryPrivacyClass, MemorySemanticJudgmentSource,
    MemorySubjectVisibilityPolicy,
};
use bm_core::platform::Platform;
use bm_sdk::nonproduction_replay_harness::{StoreBackendConfig, StorePlatform};

fn seed_governed_archive(
    platform: &StorePlatform,
    memory_space_id: &str,
    draft: &LongTermMemoryDraft,
    now_secs: u64,
) -> LongTermMemoryEntry {
    support::seed_scoped_long_term(platform, memory_space_id, draft, now_secs)
}

fn tiny_store_budget() -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        metric_source_max_items: 1,
        event_log_max_items: 2,
        kv_max_entries: 256,
        blob_max_bytes: 4,
        snapshot_max_bytes: 1024 * 1024,
        logical_namespace_max_bytes: 64,
        logical_key_max_bytes: 64,
        event_record_key_max_bytes: 64,
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
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile()).unwrap();

    let skill = {
        let platform = support::open_store(config.clone()).unwrap();
        platform
            .state_fs()
            .write("runtime/state.json", b"state")
            .unwrap();
        let skill = support::seed_runtime_skill(&platform, "runtime_skill__alpha");
        platform
            .session_store()
            .append("chat-a", "user", "hello")
            .unwrap();
        seed_governed_archive(
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
                privacy: MemoryPrivacyClass::SharedWithSubject,
                source_chat_id: Some("chat-a".to_string()),
                source_type: Some(LongTermMemorySourceType::Conversation),
                source_scope: Some(LongTermMemorySourceScope::User),
                subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
                provenance: LongTermMemoryProvenance {
                    source_authority: MemoryEvidenceAuthority::UserAsserted,
                    semantic_judgment_source: Some(MemorySemanticJudgmentSource::RuntimeGate),
                },
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
                source_revision: Some(1),
            },
            100,
        );
        assert!(platform
            .read_events()
            .unwrap()
            .iter()
            .any(|event| event.kind_name == "memory.write"));
        skill
    };

    let reopened = support::open_store(config).unwrap();
    assert_eq!(
        reopened.state_fs().read("runtime/state.json").unwrap(),
        Some(b"state".to_vec())
    );
    assert_eq!(
        support::read_runtime_skill_owner(&reopened, &skill.physical_key),
        skill
    );
    assert_eq!(
        reopened
            .memory_space_long_term_memory_read_store("space:test")
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

    support::open_store(
        StoreBackendConfig::sqlite(&path, support::native_persistent_profile()).unwrap(),
    )
    .unwrap();

    let err = match support::open_store(
        StoreBackendConfig::sqlite(
            &path,
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .unwrap(),
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
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .unwrap()
        .try_with_nonproduction_store_budget_limit(tiny_store_budget())
        .expect("tiny sqlite budget must be a valid semantic contraction");
    let platform = support::open_store(config).unwrap();

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
    let config = StoreBackendConfig::sqlite(&path, support::native_persistent_profile())
        .unwrap()
        .try_with_nonproduction_store_budget_limit(blob_budget)
        .expect("blob sqlite budget must be a valid semantic contraction");
    let platform = support::open_store(config).unwrap();

    platform.state_fs().write("a", b"12").unwrap();
    platform.state_fs().write("b", b"12").unwrap();
    let err = platform
        .state_fs()
        .write("c", b"1")
        .expect_err("sqlite must enforce cumulative blob budget");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("blob bytes"));

    let oversized_key = "k".repeat(65);
    let err = platform
        .state_fs()
        .write(&oversized_key, b"1")
        .expect_err("sqlite must reject oversized logical keys");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("logical key"));
}
