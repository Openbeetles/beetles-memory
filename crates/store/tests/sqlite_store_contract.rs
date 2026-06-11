#![cfg(feature = "sqlite-store")]

use bm_core::budget::StoreRuntimeBudget;
use bm_core::feature_gate::ProfileId;
use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryKind};
use bm_core::platform::Platform;
use bm_store::{StoreBackendConfig, StorePlatform};

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
        platform
            .long_term_memory_store()
            .upsert_many(
                &[LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Fact,
                    topic: "archive".to_string(),
                    content: "Archive memory must remain searchable".to_string(),
                    keywords: vec!["archive".to_string()],
                    source_chat_id: Some("chat-a".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: Vec::new(),
                    evidence_count: None,
                    observed_at: None,
                    last_confirmed_at: None,
                    source_revision: None,
                }],
                100,
            )
            .unwrap();
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
            .long_term_memory_store()
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
