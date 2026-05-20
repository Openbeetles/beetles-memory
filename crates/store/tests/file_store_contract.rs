use bm_core::feature_gate::ProfileId;
use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryKind};
use bm_core::platform::Platform;
use bm_store::{StoreBackendConfig, StoreEventLog, StorePlatform};

fn temp_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "beetle-memory-file-store-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

#[test]
fn file_store_persists_core_runtime_paths_across_reopen() {
    let root = temp_root("persist");
    let config = StoreBackendConfig::file(&root, ProfileId::DesktopMacosEmbeddedSdk).unwrap();

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
                    topic: "lens".to_string(),
                    content: "Use a fast prime lens indoors".to_string(),
                    keywords: vec!["lens".to_string()],
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
        reopened.session_store().load_recent("chat-a", 1).unwrap()[0].content,
        "hello"
    );
    assert_eq!(
        reopened
            .long_term_memory_store()
            .recall("lens", Some("chat-a"), 4)
            .unwrap()
            .len(),
        1
    );
    assert!(reopened.read_events().unwrap().len() >= 5);
}

#[test]
fn file_store_rejects_profile_mismatch_on_existing_root() {
    let root = temp_root("manifest-profile-mismatch");
    StorePlatform::open(
        StoreBackendConfig::file(&root, ProfileId::DesktopMacosEmbeddedSdk).unwrap(),
    )
    .unwrap();

    let err = match StorePlatform::open(
        StoreBackendConfig::file(&root, ProfileId::ServerLinuxDevFull).unwrap(),
    ) {
        Ok(_) => panic!("existing store root must reject a different profile"),
        Err(error) => error,
    };

    assert_eq!(err.stage(), "file_store_manifest");
    assert!(err.to_string().contains("profile"));
}
