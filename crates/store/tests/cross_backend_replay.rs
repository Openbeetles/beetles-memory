use bm_core::feature_gate::ProfileId;
use bm_core::memory::{LongTermMemoryDraft, LongTermMemoryKind};
use bm_core::platform::Platform;
use bm_store::{StoreBackendConfig, StorePlatform};

fn seed(platform: &StorePlatform) {
    platform
        .state_fs()
        .write("runtime/state.json", b"state")
        .unwrap();
    platform
        .skill_storage()
        .write("runtime-alpha", b"skill")
        .unwrap();
    platform
        .session_store()
        .append("chat-a", "user", "hello")
        .unwrap();
    platform
        .long_term_memory_store()
        .upsert_many(
            &[LongTermMemoryDraft {
                kind: LongTermMemoryKind::Fact,
                topic: "tripod".to_string(),
                content: "Use a tripod for long exposure".to_string(),
                keywords: vec!["tripod".to_string()],
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
}

#[test]
fn snapshot_import_keeps_state_consistent_across_backends() {
    let (source, open_report) = StorePlatform::open_with_report(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    assert_eq!(open_report.backend, "in_memory");
    assert!(open_report.repair.checked);
    seed(&source);
    let (snapshot, export_report) = source.export_store_snapshot_with_report().unwrap();
    let expected_state = snapshot.state_fingerprint();
    let expected_events = snapshot.event_fingerprint();
    assert_eq!(snapshot.schema_manifest.backend, "in_memory");
    assert_eq!(
        snapshot.schema_manifest.profile,
        "target-server-linux+role-dev-full"
    );
    assert_eq!(export_report.state_fingerprint, expected_state);
    assert_eq!(export_report.event_fingerprint, expected_events);

    let root = std::env::temp_dir().join(format!(
        "beetle-memory-cross-backend-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);

    let file = StorePlatform::open(
        StoreBackendConfig::file(root.join("file"), ProfileId::DesktopMacosEmbeddedSdk).unwrap(),
    )
    .unwrap();
    file.state_fs().write("stale-target-key", b"stale").unwrap();
    let import_report = file.import_store_snapshot_with_report(&snapshot).unwrap();
    assert_eq!(import_report.state_fingerprint, expected_state);
    assert_eq!(import_report.event_fingerprint, expected_events);
    assert_eq!(import_report.blobs_deleted, 1);
    assert_eq!(import_report.events_imported, snapshot.events.len());
    assert_eq!(import_report.events_skipped, 0);
    let file_snapshot = file.export_store_snapshot().unwrap();
    assert_eq!(file_snapshot.state_fingerprint(), expected_state);
    assert_eq!(file_snapshot.event_fingerprint(), expected_events);
    assert_eq!(file.state_fs().read("stale-target-key").unwrap(), None);

    let embedded =
        StorePlatform::open(StoreBackendConfig::embedded(ProfileId::EspStandaloneMemory).unwrap())
            .unwrap();
    let embedded_import_report = embedded
        .import_store_snapshot_with_report(&snapshot)
        .unwrap();
    assert_eq!(embedded_import_report.state_fingerprint, expected_state);
    assert_eq!(embedded_import_report.event_fingerprint, expected_events);
    let embedded_snapshot = embedded.export_store_snapshot().unwrap();
    assert_eq!(embedded_snapshot.state_fingerprint(), expected_state);
    assert_eq!(embedded_snapshot.event_fingerprint(), expected_events);

    #[cfg(feature = "sqlite-store")]
    {
        let sqlite = StorePlatform::open(
            StoreBackendConfig::sqlite(
                root.join("sqlite").join("memory.sqlite3"),
                ProfileId::ServerLinuxMemoryGateway,
            )
            .unwrap(),
        )
        .unwrap();
        let sqlite_import_report = sqlite.import_store_snapshot_with_report(&snapshot).unwrap();
        assert_eq!(sqlite_import_report.state_fingerprint, expected_state);
        assert_eq!(sqlite_import_report.event_fingerprint, expected_events);
        let sqlite_snapshot = sqlite.export_store_snapshot().unwrap();
        assert_eq!(sqlite_snapshot.state_fingerprint(), expected_state);
        assert_eq!(sqlite_snapshot.event_fingerprint(), expected_events);
    }
}

#[test]
fn snapshot_import_rejects_bad_lineage_before_touching_target_state() {
    let source = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    seed(&source);
    let mut snapshot = source.export_store_snapshot().unwrap();
    assert!(snapshot.events.len() >= 2);
    snapshot.events[1].event_id = snapshot.events[0].event_id.clone();

    let target = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    target
        .state_fs()
        .write("runtime/state.json", b"keep")
        .unwrap();
    let before = target.export_store_snapshot().unwrap();

    let err = target
        .import_store_snapshot(&snapshot)
        .expect_err("duplicate snapshot event lineage must be rejected");

    assert_eq!(err.stage(), "store_snapshot_import");
    assert!(err.to_string().contains("duplicate event id"));
    let after = target.export_store_snapshot().unwrap();
    assert_eq!(after.state_fingerprint(), before.state_fingerprint());
    assert_eq!(after.event_fingerprint(), before.event_fingerprint());
}

#[test]
fn snapshot_import_rejects_manifest_schema_drift() {
    let source = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    seed(&source);
    let mut snapshot = source.export_store_snapshot().unwrap();
    snapshot.schema_manifest.schema_version = 999;

    let target = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).unwrap(),
    )
    .unwrap();
    let err = target
        .import_store_snapshot(&snapshot)
        .expect_err("manifest schema version drift must be rejected");

    assert_eq!(err.stage(), "store_snapshot_import");
    assert!(err.to_string().contains("schema version"));
}
