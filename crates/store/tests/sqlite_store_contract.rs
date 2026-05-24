#![cfg(feature = "sqlite-store")]

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryKind};
use bm_core::platform::Platform;
use bm_store::{StoreBackendConfig, StorePlatform};

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
