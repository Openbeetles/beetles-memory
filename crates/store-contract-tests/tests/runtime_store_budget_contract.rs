mod support;
use bm_core::budget::StoreRuntimeBudget;
use bm_core::feature_gate::ProfileId;
use bm_core::memory::LongTermMemoryVersionScopeManifest;
use bm_core::platform::Platform as _;
#[cfg(feature = "sqlite-store")]
use bm_sdk::nonproduction_replay_harness::SqliteStoreEngine;
use bm_sdk::nonproduction_replay_harness::{
    EmbeddedStoreEngine, FileStoreEngine, InMemoryStoreEngine, MemoryStoreEvent,
    MemoryStoreEventKind, StoreBackendConfig, StoreBackendKind, StoreCapacityBudget, StoreEngine,
    StoreEventLog, StoreEventScope, StoreScopedProjectionReplaceRequest,
    StoreScopedProjectionScope, StoreSnapshot, StoreSnapshotBlob, StoreSnapshotJsonDoc,
    LONG_TERM_HEAD_MANIFEST_NAMESPACE, LONG_TERM_VERSION_MATERIAL_NAMESPACE,
    LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
};
use serde_json::Value;

const TYPED_RESTORE_MEMORY_SPACE_ID: &str = "space:typed-restore";
const TYPED_RESTORE_SUBJECT_ID: &str = "subject:typed-restore";

fn tiny_store_budget() -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        metric_source_max_items: 1,
        event_log_max_items: 8,
        kv_max_entries: 256,
        blob_max_bytes: 4,
        snapshot_max_bytes: 128,
        logical_namespace_max_bytes: 64,
        logical_key_max_bytes: 64,
        event_record_key_max_bytes: 64,
        export_max_bytes: 128,
        import_max_bytes: 128,
    }
}

fn typed_restore_engine_capacity() -> StoreCapacityBudget {
    StoreCapacityBudget {
        metric_source_max_items: 1,
        event_log_max_items: 8,
        kv_max_entries: 16,
        blob_max_bytes: 8,
        snapshot_max_bytes: 4096,
        logical_namespace_max_bytes: 128,
        logical_key_max_bytes: 128,
        event_record_key_max_bytes: 128,
        export_max_bytes: 4096,
        import_max_bytes: 4096,
    }
}

fn typed_restore_event(event_id: &str, revision: &str) -> MemoryStoreEvent {
    MemoryStoreEvent::new(
        event_id,
        MemoryStoreEventKind::MemoryProjection,
        StoreEventScope::new("agent", "owner", "test", "typed-restore")
            .with_memory_space(TYPED_RESTORE_MEMORY_SPACE_ID)
            .with_subject(TYPED_RESTORE_SUBJECT_ID),
        1,
    )
    .with_plane(LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE)
    .with_record_key(typed_restore_document(1).key)
    .with_payload("revision", revision)
}

fn typed_restore_document(revision: u64) -> StoreSnapshotJsonDoc {
    let manifest = LongTermMemoryVersionScopeManifest::build(
        TYPED_RESTORE_MEMORY_SPACE_ID,
        TYPED_RESTORE_MEMORY_SPACE_ID,
        revision,
        &[],
        &[],
        &[],
        &[],
        1,
    )
    .expect("typed restore manifest");
    StoreSnapshotJsonDoc {
        namespace: LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE.to_string(),
        key: manifest.physical_key.clone(),
        value: serde_json::to_value(manifest).expect("serialize typed restore manifest"),
    }
}

fn typed_restore_request() -> StoreScopedProjectionReplaceRequest {
    StoreScopedProjectionReplaceRequest {
        scope: StoreScopedProjectionScope::subject(
            TYPED_RESTORE_MEMORY_SPACE_ID,
            TYPED_RESTORE_SUBJECT_ID,
        )
        .expect("typed restore scope"),
        json_namespaces: vec![
            LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE.to_string(),
            LONG_TERM_HEAD_MANIFEST_NAMESPACE.to_string(),
            LONG_TERM_VERSION_MATERIAL_NAMESPACE.to_string(),
        ],
        json_docs: vec![typed_restore_document(2)],
        events: vec![typed_restore_event("typed-restore:new", "replacement")],
    }
}

fn seed_typed_restore(engine: &dyn StoreEngine) {
    let before = typed_restore_document(1);
    engine
        .put_json_value(&before.namespace, &before.key, before.value)
        .expect("seed typed restore JSON");
    engine
        .put_blob("retained_private", "keep", b"12345")
        .expect("seed typed restore retained blob");
    engine
        .append_event(typed_restore_event("typed-restore:before", "before"))
        .expect("seed typed restore event");
}

fn typed_restore_state(
    engine: &dyn StoreEngine,
) -> (Option<Value>, Option<Vec<u8>>, Vec<MemoryStoreEvent>) {
    (
        {
            let address = typed_restore_document(1);
            engine
                .get_json_value(&address.namespace, &address.key)
                .expect("read typed restore JSON")
        },
        engine
            .get_blob("retained_private", "keep")
            .expect("read typed restore blob"),
        engine.read_events().expect("read typed restore events"),
    )
}

fn assert_typed_restore_success(
    backend: &str,
    engine: &dyn StoreEngine,
    capacity: StoreCapacityBudget,
) {
    let request = typed_restore_request();
    engine
        .replace_scoped_projection_with_capacity(&request, capacity)
        .unwrap_or_else(|error| panic!("{backend} typed restore exact budget: {error}"));
    let state = typed_restore_state(engine);
    assert_eq!(state.0, Some(typed_restore_document(2).value), "{backend}");
    assert_eq!(state.1, Some(b"12345".to_vec()), "{backend}");
    assert_eq!(state.2, request.events, "{backend}");
}

fn assert_typed_restore_rejected_unchanged(
    backend: &str,
    engine: &dyn StoreEngine,
    capacity: StoreCapacityBudget,
) {
    let before = typed_restore_state(engine);
    let error = engine
        .replace_scoped_projection_with_capacity(&typed_restore_request(), capacity)
        .expect_err("typed restore +1 budget must fail closed");
    assert_eq!(error.stage(), "store_budget_exceeded", "backend={backend}");
    assert_eq!(typed_restore_state(engine), before, "backend={backend}");
}

fn assert_typed_restore_budget_matrix(
    backend: &str,
    mut open: impl FnMut(&str) -> Box<dyn StoreEngine>,
) {
    let request = typed_restore_request();
    let json_event_bytes = request
        .json_docs
        .iter()
        .map(|doc| serde_json::to_vec(&doc.value).expect("JSON bytes").len())
        .sum::<usize>()
        + request
            .events
            .iter()
            .map(|event| serde_json::to_vec(event).expect("event bytes").len())
            .sum::<usize>();
    let import_bytes = serde_json::to_vec(&request)
        .expect("typed import bytes")
        .len();
    let base = typed_restore_engine_capacity();

    let exact_event = open("event-exact");
    seed_typed_restore(exact_event.as_ref());
    let mut capacity = base;
    capacity.snapshot_max_bytes = json_event_bytes;
    assert_typed_restore_success(backend, exact_event.as_ref(), capacity);

    let event_plus_one = open("event-plus-one");
    seed_typed_restore(event_plus_one.as_ref());
    capacity.snapshot_max_bytes = json_event_bytes - 1;
    assert_typed_restore_rejected_unchanged(backend, event_plus_one.as_ref(), capacity);

    let exact_import = open("import-exact");
    seed_typed_restore(exact_import.as_ref());
    capacity = base;
    capacity.import_max_bytes = import_bytes;
    assert_typed_restore_success(backend, exact_import.as_ref(), capacity);

    let import_plus_one = open("import-plus-one");
    seed_typed_restore(import_plus_one.as_ref());
    capacity.import_max_bytes = import_bytes - 1;
    assert_typed_restore_rejected_unchanged(backend, import_plus_one.as_ref(), capacity);

    let exact_blob = open("blob-exact");
    seed_typed_restore(exact_blob.as_ref());
    capacity = base;
    capacity.blob_max_bytes = 5;
    assert_typed_restore_success(backend, exact_blob.as_ref(), capacity);

    let blob_plus_one = open("blob-plus-one");
    seed_typed_restore(blob_plus_one.as_ref());
    capacity.blob_max_bytes = 4;
    assert_typed_restore_rejected_unchanged(backend, blob_plus_one.as_ref(), capacity);
}

#[test]
fn typed_restore_post_image_budgets_are_atomic_across_all_backends() {
    let capacity = typed_restore_engine_capacity();
    assert_typed_restore_budget_matrix("in_memory", |_| {
        Box::new(InMemoryStoreEngine::new(capacity))
    });
    assert_typed_restore_budget_matrix("embedded", |_| {
        Box::new(EmbeddedStoreEngine::new(capacity))
    });

    let root = std::env::temp_dir().join(format!(
        "beetle-memory-restore-budget-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    let profile = support::native_persistent_profile();
    assert_typed_restore_budget_matrix("file", |case| {
        let config = StoreBackendConfig::file(root.join(format!("file-{case}")), profile)
            .expect("file restore config");
        let (engine, _, _) = FileStoreEngine::open_with_capacity(&config, capacity)
            .expect("open file restore engine");
        Box::new(engine)
    });
    #[cfg(feature = "sqlite-store")]
    assert_typed_restore_budget_matrix("sqlite", |case| {
        let config = StoreBackendConfig::sqlite(root.join(format!("sqlite-{case}.db")), profile)
            .expect("sqlite restore config");
        let (engine, _) = SqliteStoreEngine::open_with_capacity(&config, capacity)
            .expect("open sqlite restore engine");
        Box::new(engine)
    });
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn in_memory_store_consumes_compiled_blob_budget() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(tiny_store_budget())
    .expect("valid store budget");
    let platform = support::open_store(config).expect("store");

    let err = platform
        .state_fs()
        .write("too-large.bin", b"12345")
        .expect_err("blob budget must reject oversized writes");

    assert_eq!(err.stage(), "store_budget_exceeded");
}

#[test]
fn file_store_consumes_compiled_snapshot_budget() {
    let root = std::env::temp_dir().join(format!(
        "bm-store-budget-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut budget = tiny_store_budget();
    budget.snapshot_max_bytes = 1024;
    let config = StoreBackendConfig::file(
        &root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(budget)
    .expect("valid store budget");
    let platform = support::open_store(config).expect("store");
    let err = platform
        .export_store_snapshot()
        .expect_err("snapshot budget must apply to file backend");
    assert_eq!(err.stage(), "store_budget_exceeded");
}

#[test]
fn store_capacity_comes_from_the_open_runtime_budget_authority() {
    let config = StoreBackendConfig::in_memory(ProfileId::EspEmbeddedSdk).expect("config");
    let platform = support::open_store(config).expect("store");
    assert!(platform.capacity().snapshot_max_bytes <= 256 * 1024);
    let config = platform.config();
    assert_eq!(config.backend(), StoreBackendKind::InMemory);
}

#[test]
fn store_platform_rejects_logical_keys_that_exceed_runtime_budget() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(tiny_store_budget())
    .expect("valid store budget");
    let platform = support::open_store(config).expect("store");
    let oversized_key = "k".repeat(65);

    let err = platform
        .state_fs()
        .write(&oversized_key, b"1")
        .expect_err("logical key budget must reject oversized keys before persistence");

    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("logical key"));
    assert!(!platform
        .state_fs()
        .list_dir("")
        .unwrap()
        .contains(&oversized_key));
}

#[test]
fn in_memory_store_platform_rejects_event_log_overflow_without_persisting_state() {
    let mut budget = tiny_store_budget();
    budget.event_log_max_items = 2;
    let config = StoreBackendConfig::in_memory(ProfileId::EspEmbeddedSdk)
        .expect("config")
        .try_with_nonproduction_store_budget_limit(budget)
        .expect("valid store budget");
    let platform = support::open_store(config).expect("store");

    platform
        .state_fs()
        .write("first", b"1")
        .expect("open event plus one write reaches the cap");
    let err = platform
        .state_fs()
        .write("second", b"2")
        .expect_err("event log cap must reject the write before mutating state");

    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("event log"));
    assert_eq!(platform.state_fs().read("second").unwrap(), None);
}

#[test]
fn direct_in_memory_engine_consumes_capacity_budget() {
    let capacity = StoreCapacityBudget {
        metric_source_max_items: 1,
        event_log_max_items: 1,
        kv_max_entries: 1,
        blob_max_bytes: 1,
        snapshot_max_bytes: 256,
        logical_namespace_max_bytes: 16,
        logical_key_max_bytes: 16,
        event_record_key_max_bytes: 16,
        export_max_bytes: 256,
        import_max_bytes: 256,
    };
    let engine = InMemoryStoreEngine::new(capacity);

    engine
        .put_blob("state_fs", "small", b"1")
        .expect("first byte fits budget");
    let err = engine
        .put_blob("state_fs", "second", b"1")
        .expect_err("direct backend API must enforce cumulative blob budget");
    assert_eq!(err.stage(), "store_budget_exceeded");

    engine
        .append_event(MemoryStoreEvent::new(
            "evt-1",
            MemoryStoreEventKind::RuntimeLifecycle,
            StoreEventScope::system("direct"),
            1,
        ))
        .expect("first event fits budget");
    let err = engine
        .append_event(MemoryStoreEvent::new(
            "evt-2",
            MemoryStoreEventKind::RuntimeLifecycle,
            StoreEventScope::system("direct"),
            2,
        ))
        .expect_err("direct backend API must enforce event budget");
    assert_eq!(err.stage(), "store_budget_exceeded");
}

#[test]
fn export_and_import_use_dedicated_runtime_budgets() {
    let source = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("source");
    source
        .state_fs()
        .write("payload", b"payload large enough for snapshot budget split")
        .expect("seed source");
    let snapshot = source.export_store_snapshot().expect("snapshot");

    let mut export_budget = tiny_store_budget();
    export_budget.blob_max_bytes = 1024 * 1024;
    export_budget.snapshot_max_bytes = 1024 * 1024;
    export_budget.export_max_bytes = 64;
    let export_config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(export_budget)
    .expect("valid export budget");
    let export_target = support::open_store(export_config).expect("export target");
    export_target
        .state_fs()
        .write("payload", b"payload large enough for snapshot budget split")
        .expect("seed target");
    let err = export_target
        .export_store_snapshot()
        .expect_err("export must use export_max_bytes, not snapshot_max_bytes");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("export"));

    let mut import_budget = tiny_store_budget();
    import_budget.blob_max_bytes = 1024 * 1024;
    import_budget.snapshot_max_bytes = 1024 * 1024;
    import_budget.import_max_bytes = 64;
    let import_config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(import_budget)
    .expect("valid import budget");
    let import_target = support::open_store(import_config).expect("import target");
    let err = import_target
        .import_store_snapshot(&snapshot)
        .expect_err("import must use import_max_bytes, not snapshot_max_bytes");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("import"));
}

#[test]
fn snapshot_import_rejects_oversized_logical_key_before_replacing_state() {
    let mut budget = tiny_store_budget();
    budget.blob_max_bytes = 1024 * 1024;
    budget.snapshot_max_bytes = 1024 * 1024;
    budget.import_max_bytes = 1024 * 1024;
    budget.export_max_bytes = 1024 * 1024;
    let target = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config")
        .try_with_nonproduction_store_budget_limit(budget)
        .expect("valid import budget"),
    )
    .expect("target");
    target
        .state_fs()
        .write("keep", b"state")
        .expect("seed state before failed import");
    let before = target.export_store_snapshot().expect("before");

    let snapshot = StoreSnapshot::new(
        before.schema_manifest.clone(),
        Vec::new(),
        vec![StoreSnapshotBlob {
            namespace: "state_fs".to_string(),
            key: "x".repeat(65),
            value: b"1".to_vec(),
        }],
        Vec::new(),
    );

    let err = target
        .import_store_snapshot(&snapshot)
        .expect_err("oversized import key must be rejected before replacement");
    assert_eq!(err.stage(), "store_budget_exceeded");
    assert!(err.to_string().contains("logical key"));
    assert_eq!(
        target.state_fs().read("keep").unwrap(),
        Some(b"state".to_vec())
    );
}
