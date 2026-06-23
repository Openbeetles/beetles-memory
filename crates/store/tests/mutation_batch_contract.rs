use bm_core::budget::StoreRuntimeBudget;
use bm_core::feature_gate::ProfileId;
use bm_core::platform::Platform as _;
use bm_store::{
    MemoryStoreEventKind, StoreBackendConfig, StoreEventScope, StoreMutation, StoreMutationBatch,
    StorePlatform,
};
use serde_json::json;

fn transaction_budget(event_log_max_items: usize, kv_max_entries: usize) -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        event_log_max_items,
        kv_max_entries,
        blob_max_bytes: 1024,
        snapshot_max_bytes: 4096,
        logical_namespace_max_bytes: 64,
        logical_key_max_bytes: 64,
        event_record_key_max_bytes: 64,
        export_max_bytes: 4096,
        import_max_bytes: 4096,
    }
}

fn put_blob(key: &str, value: &[u8]) -> StoreMutation {
    StoreMutation::PutBlob {
        namespace: "state_fs".to_string(),
        key: key.to_string(),
        value: value.to_vec(),
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: "state_fs".to_string(),
        record_key: key.to_string(),
    }
}

fn put_graph_json(namespace: &str, key: &str) -> StoreMutation {
    StoreMutation::PutJson {
        namespace: namespace.to_string(),
        key: key.to_string(),
        value: json!({
            "id": key,
            "namespace": namespace,
            "evidence_refs": ["turn:1"]
        }),
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: "memory_graph".to_string(),
        record_key: key.to_string(),
    }
}

fn put_facet_index_json(key: &str) -> StoreMutation {
    StoreMutation::PutJson {
        namespace: "memory_facet_indexes".to_string(),
        key: key.to_string(),
        value: json!({
            "owner_record_id": key,
            "owner_plane": "long_term",
            "schema_version": 1,
            "facet_index_revision": 1,
            "memory_space_id": "space:main",
            "subject_ids": ["subject:user"],
            "status": "active",
            "exact_facets": [],
            "expanded_facets": [],
            "canonical_evidence_refs": [],
            "source_revision": 1,
            "updated_at": 1
        }),
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: "memory_facet_indexes".to_string(),
        record_key: key.to_string(),
    }
}

#[test]
fn in_memory_batch_rejects_event_overflow_without_partial_state() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(2, 8));
    let platform = StorePlatform::open(config).expect("platform");
    let before_events = platform.read_events().expect("events before");
    assert_eq!(before_events.len(), 1, "open emits one lifecycle event");

    let err = platform
        .commit_mutation_batch(StoreMutationBatch {
            transaction_id: "txn-overflow".to_string(),
            operation: "test.batch".to_string(),
            scope: StoreEventScope::system("test.batch"),
            mutations: vec![put_blob("first", b"1"), put_blob("second", b"2")],
        })
        .expect_err("event budget must reject the whole batch before mutation");

    assert_eq!(err.stage(), "memory_write_transaction_preflight_failed");
    assert_eq!(platform.state_fs().read("first").unwrap(), None);
    assert_eq!(platform.state_fs().read("second").unwrap(), None);
    assert_eq!(platform.read_events().unwrap(), before_events);
}

#[test]
fn in_memory_batch_commits_all_mutations_with_transaction_lineage() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(8, 8));
    let platform = StorePlatform::open(config).expect("platform");

    let report = platform
        .commit_mutation_batch(StoreMutationBatch {
            transaction_id: "txn-success".to_string(),
            operation: "test.batch".to_string(),
            scope: StoreEventScope::system("test.batch"),
            mutations: vec![put_blob("first", b"1"), put_blob("second", b"2")],
        })
        .expect("batch commit");

    assert!(report.admitted);
    assert!(report.committed);
    assert_eq!(report.transaction_id, "txn-success");
    assert_eq!(report.mutations, 2);
    assert_eq!(report.events, 2);
    assert_eq!(report.event_ids.len(), 2);
    assert_eq!(
        platform.state_fs().read("first").unwrap(),
        Some(b"1".to_vec())
    );
    assert_eq!(
        platform.state_fs().read("second").unwrap(),
        Some(b"2".to_vec())
    );

    let events = platform.read_events().unwrap();
    let transaction_events = events
        .iter()
        .filter(|event| {
            event.payload.get("transaction_id").map(String::as_str) == Some("txn-success")
        })
        .collect::<Vec<_>>();
    assert_eq!(transaction_events.len(), 2);
    assert!(transaction_events.iter().all(|event| event
        .payload
        .get("operation")
        .map(String::as_str)
        == Some("test.batch")));
}

#[test]
fn in_memory_batch_commits_temporal_memory_graph_namespaces_atomically() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(8, 8));
    let platform = StorePlatform::open(config).expect("platform");

    let report = platform
        .commit_mutation_batch(StoreMutationBatch {
            transaction_id: "txn-graph".to_string(),
            operation: "memory_graph.write".to_string(),
            scope: StoreEventScope::system("memory_graph.write"),
            mutations: vec![
                put_graph_json("memory_graph_nodes", "node:release"),
                put_graph_json("memory_graph_edges", "edge:release"),
                put_graph_json("memory_graph_backlinks", "backlink:release"),
                put_graph_json("memory_graph_revisions", "revision:release"),
            ],
        })
        .expect("graph batch commit");

    assert!(report.admitted);
    assert!(report.committed);
    assert_eq!(report.changed_json, 4);
    assert_eq!(report.events, 4);

    let snapshot = platform.export_store_snapshot().expect("snapshot");
    for namespace in [
        "memory_graph_nodes",
        "memory_graph_edges",
        "memory_graph_backlinks",
        "memory_graph_revisions",
    ] {
        assert!(
            snapshot
                .json_docs
                .iter()
                .any(|doc| doc.namespace == namespace),
            "missing graph namespace {namespace}"
        );
    }
}

#[test]
fn json_namespace_read_exposes_admitted_docs_without_store_graph_semantics() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(8, 8));
    let platform = StorePlatform::open(config).expect("platform");

    platform
        .commit_mutation_batch(StoreMutationBatch {
            transaction_id: "txn-read-namespace".to_string(),
            operation: "memory_graph.write".to_string(),
            scope: StoreEventScope::system("memory_graph.write"),
            mutations: vec![put_graph_json("memory_graph_nodes", "node:release")],
        })
        .expect("graph batch commit");

    let docs = platform
        .read_json_namespace("memory_graph_nodes")
        .expect("read graph namespace");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].namespace, "memory_graph_nodes");
    assert_eq!(docs[0].key, "node:release");
    assert_eq!(docs[0].value["id"], "node:release");

    let err = platform
        .read_json_namespace("memory_graph_unowned_semantics")
        .expect_err("unsupported namespace must fail closed");
    assert_eq!(err.stage(), "store_json_namespace_read");
}

#[test]
fn memory_facet_index_namespace_is_admitted_without_store_semantics() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(8, 8));
    let platform = StorePlatform::open(config).expect("platform");

    platform
        .commit_mutation_batch(StoreMutationBatch {
            transaction_id: "txn-facet-index".to_string(),
            operation: "memory_facet_index.write".to_string(),
            scope: StoreEventScope::system("memory_facet_index.write"),
            mutations: vec![put_facet_index_json("facet-index:ltm:project")],
        })
        .expect("facet index batch commit");

    let docs = platform
        .read_json_namespace("memory_facet_indexes")
        .expect("read facet index namespace");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].namespace, "memory_facet_indexes");
    assert_eq!(docs[0].key, "facet-index:ltm:project");
    assert_eq!(docs[0].value["owner_plane"], "long_term");
}
