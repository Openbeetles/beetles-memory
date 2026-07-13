use bm_core::budget::StoreRuntimeBudget;
use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    build_memory_graph_persistence_plan, memory_graph_scope_manifest_key,
    scoped_long_term_memory_storage_key, scoped_memory_graph_storage_key, EvidenceBacklink,
    LongTermMemoryConfidence, LongTermMemoryEntry, LongTermMemoryFreshness, LongTermMemoryKind,
    LongTermMemorySourceScope, LongTermMemorySourceType, MemoryGraphNode, MemoryGraphNodeKind,
    MemoryGraphOwnerBinding, MemoryPrivacyClass, LONG_TERM_CONTROL_AUDIT_NAMESPACE,
    LONG_TERM_CONTROL_REVISION_NAMESPACE, LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
    LONG_TERM_GOVERNANCE_POLICY_NAMESPACE, MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
    MEMORY_GRAPH_BACKLINK_NAMESPACE, MEMORY_GRAPH_INDEX_NAMESPACE, MEMORY_GRAPH_MANIFEST_NAMESPACE,
    MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE, MEMORY_GRAPH_NODE_NAMESPACE,
    MEMORY_GRAPH_REVISION_NAMESPACE,
};
use bm_core::platform::Platform as _;
use std::sync::{Arc, Barrier};
use std::thread;

use bm_sdk::nonproduction_replay_harness::{
    MemoryStoreEventKind, StoreBackendConfig, StoreEventScope, StoreJsonPrecondition,
    StoreMutation, StoreMutationBatch, StorePlatform, StoreSnapshotJsonDoc,
};
use serde_json::json;

fn transaction_budget(event_log_max_items: usize, kv_max_entries: usize) -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        event_log_max_items,
        kv_max_entries,
        blob_max_bytes: 1024,
        snapshot_max_bytes: 16_384,
        logical_namespace_max_bytes: 64,
        logical_key_max_bytes: 64,
        event_record_key_max_bytes: 64,
        export_max_bytes: 16_384,
        import_max_bytes: 16_384,
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

fn graph_owner_for(owner_id: &str) -> LongTermMemoryEntry {
    LongTermMemoryEntry {
        id: owner_id.to_string(),
        kind: LongTermMemoryKind::Project,
        topic: "graph exact closure".to_string(),
        content: "Graph exact closure owner.".to_string(),
        keywords: vec!["graph".to_string()],
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: Some("chat:graph".to_string()),
        source_type: LongTermMemorySourceType::Conversation,
        source_scope: LongTermMemorySourceScope::User,
        confidence: LongTermMemoryConfidence::High,
        freshness: LongTermMemoryFreshness::Dynamic,
        stale_hint: Default::default(),
        supporting_citations: vec!["evidence:graph".to_string()],
        canonical_entities: Vec::new(),
        evidence_count: 1,
        created_at: 1,
        updated_at: 1,
        observed_at: 1,
        last_confirmed_at: 1,
        source_revision: Some(1),
        owner_revision: 1,
        last_used_at: 0,
    }
}

fn graph_owner() -> LongTermMemoryEntry {
    graph_owner_for("owner:graph")
}

fn typed_graph_closure_for(
    memory_space_id: &str,
    subject_id: &str,
    owner_id: &str,
) -> Vec<StoreMutation> {
    let node = MemoryGraphNode {
        node_id: owner_id.to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: "Graph exact closure owner".to_string(),
        evidence_refs: vec!["evidence:graph".to_string()],
    };
    let backlink = EvidenceBacklink {
        source_kind: "long_term_memory".to_string(),
        source_id: "evidence:graph".to_string(),
        fingerprint: "fingerprint:graph".to_string(),
    };
    let plan = build_memory_graph_persistence_plan(
        memory_space_id,
        subject_id,
        1,
        vec![node.clone()],
        Vec::new(),
        vec![backlink.clone()],
        vec![MemoryGraphOwnerBinding {
            owner_record_id: owner_id.to_string(),
            owner_revision: 1,
            visible: true,
        }],
    );
    assert!(plan.accepted, "{:?}", plan.failures);
    let manifest = plan.scope_manifest.expect("scope manifest");
    let revision = plan.revision.expect("revision");
    let node_membership = plan
        .node_memberships
        .into_iter()
        .next()
        .expect("node membership");
    let backlink_membership = plan
        .backlink_memberships
        .into_iter()
        .next()
        .expect("backlink membership");
    let index = plan
        .recall_indexes
        .into_iter()
        .next()
        .expect("recall index");

    vec![
        put_json(
            MEMORY_GRAPH_NODE_NAMESPACE,
            &node_membership.document_key,
            serde_json::to_value(node).expect("serialize node"),
        ),
        put_json(
            MEMORY_GRAPH_BACKLINK_NAMESPACE,
            &backlink_membership.document_key,
            serde_json::to_value(backlink).expect("serialize backlink"),
        ),
        put_json(
            MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
            &node_membership.membership_key,
            serde_json::to_value(&node_membership).expect("serialize node membership"),
        ),
        put_json(
            MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
            &backlink_membership.membership_key,
            serde_json::to_value(&backlink_membership).expect("serialize backlink membership"),
        ),
        put_json(
            MEMORY_GRAPH_INDEX_NAMESPACE,
            &index.index_key,
            serde_json::to_value(&index).expect("serialize recall index"),
        ),
        put_json(
            MEMORY_GRAPH_REVISION_NAMESPACE,
            &revision.revision_key,
            serde_json::to_value(&revision).expect("serialize revision"),
        ),
        put_json(
            MEMORY_GRAPH_MANIFEST_NAMESPACE,
            &memory_graph_scope_manifest_key(memory_space_id, subject_id),
            serde_json::to_value(manifest).expect("serialize manifest"),
        ),
    ]
}

fn typed_graph_closure() -> Vec<StoreMutation> {
    typed_graph_closure_for("system", "system", "owner:graph")
}

fn assert_graph_batch_rejected_without_partial_state(
    transaction_id: &str,
    mutations: Vec<StoreMutation>,
) {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config");
    let platform = StorePlatform::open(config).expect("platform");
    let owner = graph_owner();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot.json_docs.push(StoreSnapshotJsonDoc {
        namespace: "long_term".to_string(),
        key: scoped_long_term_memory_storage_key("system", &owner.id).expect("owner key"),
        value: serde_json::to_value(owner).expect("serialize owner"),
    });
    platform
        .import_store_snapshot(&snapshot)
        .expect("seed graph owner");
    let batch = StoreMutationBatch {
        transaction_id: transaction_id.to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write"),
        mutations,
    };
    let preconditions = absent_json_preconditions(&batch);

    platform
        .commit_governed_memory_transaction_with_preconditions(batch, &preconditions)
        .expect_err("non-exact graph closure must reject the whole batch");

    let snapshot = platform.export_store_snapshot().expect("snapshot");
    assert!(!snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace.starts_with("memory_graph_")));
}

fn put_json(namespace: &str, key: &str, value: serde_json::Value) -> StoreMutation {
    StoreMutation::PutJson {
        namespace: namespace.to_string(),
        key: key.to_string(),
        value,
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: namespace.to_string(),
        record_key: key.to_string(),
    }
}

fn mutation_batch(transaction_id: &str, mutation: StoreMutation) -> StoreMutationBatch {
    StoreMutationBatch {
        transaction_id: transaction_id.to_string(),
        operation: "test.json_cas".to_string(),
        scope: StoreEventScope::system("test.json_cas"),
        mutations: vec![mutation],
    }
}

fn absent_json_preconditions(batch: &StoreMutationBatch) -> Vec<StoreJsonPrecondition> {
    batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. } => {
                Some(StoreJsonPrecondition::Absent {
                    namespace: namespace.clone(),
                    key: key.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

fn exact_json_preconditions(
    json_docs: &[StoreSnapshotJsonDoc],
    batch: &StoreMutationBatch,
) -> Vec<StoreJsonPrecondition> {
    batch
        .mutations
        .iter()
        .filter_map(|mutation| match mutation {
            StoreMutation::PutJson { namespace, key, .. }
            | StoreMutation::DeleteJson { namespace, key, .. } => {
                let value = json_docs
                    .iter()
                    .find(|doc| doc.namespace == *namespace && doc.key == *key)
                    .unwrap_or_else(|| {
                        panic!("missing exact precondition source for {namespace}/{key}")
                    })
                    .value
                    .clone();
                Some(StoreJsonPrecondition::Exact {
                    namespace: namespace.clone(),
                    key: key.clone(),
                    value,
                })
            }
            _ => None,
        })
        .collect()
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
        .commit_governed_memory_transaction(StoreMutationBatch {
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
        .commit_governed_memory_transaction(StoreMutationBatch {
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
fn in_memory_batch_rejects_untyped_temporal_memory_graph_closure_atomically() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(16, 16));
    let platform = StorePlatform::open(config).expect("platform");

    let batch = StoreMutationBatch {
        transaction_id: "txn-graph".to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write"),
        mutations: vec![
            put_graph_json("memory_graph_nodes", "node:release"),
            put_graph_json("memory_graph_edges", "edge:release"),
            put_graph_json("memory_graph_backlinks", "backlink:release"),
            put_graph_json("memory_graph_indexes", "index:release"),
            put_graph_json("memory_graph_revisions", "revision:release"),
            put_graph_json("memory_graph_manifests", "manifest:release"),
            put_graph_json("memory_graph_node_memberships", "node-membership:release"),
            put_graph_json("memory_graph_edge_memberships", "edge-membership:release"),
            put_graph_json(
                "memory_graph_backlink_memberships",
                "backlink-membership:release",
            ),
        ],
    };
    let preconditions = absent_json_preconditions(&batch);
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(batch, &preconditions)
        .expect_err("untyped graph closure must fail closed");
    assert_eq!(
        error.stage(),
        "memory_write_transaction_graph_scope_mismatch"
    );

    let snapshot = platform.export_store_snapshot().expect("snapshot");
    for namespace in [
        "memory_graph_nodes",
        "memory_graph_edges",
        "memory_graph_backlinks",
        "memory_graph_indexes",
        "memory_graph_revisions",
        "memory_graph_manifests",
        "memory_graph_node_memberships",
        "memory_graph_edge_memberships",
        "memory_graph_backlink_memberships",
    ] {
        assert!(!snapshot
            .json_docs
            .iter()
            .any(|doc| doc.namespace == namespace));
    }
}

#[test]
fn typed_graph_closure_rejects_an_extra_scope_manifest_atomically() {
    let mut mutations = typed_graph_closure();
    let extra_manifest = mutations
        .iter()
        .find_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == MEMORY_GRAPH_MANIFEST_NAMESPACE => Some(value.clone()),
            _ => None,
        })
        .expect("typed manifest");
    mutations.push(put_json(
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        "zz-extra-graph-manifest",
        extra_manifest,
    ));

    assert_graph_batch_rejected_without_partial_state("txn-extra-graph-manifest", mutations);
}

#[test]
fn typed_graph_closure_rejects_an_extra_revision_atomically() {
    let mut mutations = typed_graph_closure();
    let extra_revision = mutations
        .iter()
        .find_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace, value, ..
            } if namespace == MEMORY_GRAPH_REVISION_NAMESPACE => Some(value.clone()),
            _ => None,
        })
        .expect("typed revision");
    mutations.push(put_json(
        MEMORY_GRAPH_REVISION_NAMESPACE,
        "zz-extra-graph-revision",
        extra_revision,
    ));

    assert_graph_batch_rejected_without_partial_state("txn-extra-graph-revision", mutations);
}

#[test]
fn typed_graph_transaction_rejects_a_cross_scope_document_delete_atomically() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config");
    let platform = StorePlatform::open(config).expect("platform");
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    for (memory_space_id, owner) in [
        ("system", graph_owner_for("owner:graph")),
        ("space:b", graph_owner_for("owner:graph:b")),
    ] {
        snapshot.json_docs.push(StoreSnapshotJsonDoc {
            namespace: "long_term".to_string(),
            key: scoped_long_term_memory_storage_key(memory_space_id, &owner.id)
                .expect("owner key"),
            value: serde_json::to_value(owner).expect("serialize owner"),
        });
    }
    platform
        .import_store_snapshot(&snapshot)
        .expect("seed graph owners");

    let scope_a_mutations = typed_graph_closure();
    let scope_a_batch = StoreMutationBatch {
        transaction_id: "txn-seed-scope-a".to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write"),
        mutations: scope_a_mutations.clone(),
    };
    platform
        .commit_governed_memory_transaction_with_preconditions(
            scope_a_batch.clone(),
            &absent_json_preconditions(&scope_a_batch),
        )
        .expect("seed scope A graph");

    let scope_b_mutations = typed_graph_closure_for("space:b", "subject:b", "owner:graph:b");
    let scope_b_batch = StoreMutationBatch {
        transaction_id: "txn-seed-scope-b".to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write")
            .with_memory_space("space:b")
            .with_subject("subject:b"),
        mutations: scope_b_mutations.clone(),
    };
    platform
        .commit_governed_memory_transaction_with_preconditions(
            scope_b_batch.clone(),
            &absent_json_preconditions(&scope_b_batch),
        )
        .expect("seed scope B graph");

    let cross_scope_delete = scope_b_mutations
        .iter()
        .find_map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                event_kind,
                plane,
                record_key,
                ..
            } if namespace == MEMORY_GRAPH_NODE_NAMESPACE => Some(StoreMutation::DeleteJson {
                namespace: namespace.clone(),
                key: key.clone(),
                event_kind: event_kind.clone(),
                plane: plane.clone(),
                record_key: record_key.clone(),
            }),
            _ => None,
        })
        .expect("scope B node delete");
    let mut replacement_mutations = scope_a_mutations;
    replacement_mutations.push(cross_scope_delete);
    let replacement_batch = StoreMutationBatch {
        transaction_id: "txn-scope-a-with-scope-b-delete".to_string(),
        operation: "memory_graph.maintain".to_string(),
        scope: StoreEventScope::system("memory_graph.maintain"),
        mutations: replacement_mutations,
    };
    let before = platform
        .export_store_snapshot()
        .expect("before replacement");
    let preconditions = exact_json_preconditions(&before.json_docs, &replacement_batch);
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(replacement_batch, &preconditions)
        .expect_err("scope A graph transaction must not delete a scope B document");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_graph_scope_mismatch"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn typed_graph_transaction_rejects_a_same_scope_noop_document_delete_atomically() {
    let mut mutations = typed_graph_closure();
    let key = scoped_memory_graph_storage_key("system", "system", "node:unrelated-noop");
    mutations.push(StoreMutation::DeleteJson {
        namespace: MEMORY_GRAPH_NODE_NAMESPACE.to_string(),
        key,
        event_kind: MemoryStoreEventKind::MemoryDelete,
        plane: MEMORY_GRAPH_NODE_NAMESPACE.to_string(),
        record_key: "owner:unrelated-noop".to_string(),
    });

    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config");
    let platform = StorePlatform::open(config).expect("platform");
    let owner = graph_owner();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot.json_docs.push(StoreSnapshotJsonDoc {
        namespace: "long_term".to_string(),
        key: scoped_long_term_memory_storage_key("system", &owner.id).expect("owner key"),
        value: serde_json::to_value(owner).expect("serialize owner"),
    });
    platform
        .import_store_snapshot(&snapshot)
        .expect("seed graph owner");
    let batch = StoreMutationBatch {
        transaction_id: "txn-same-scope-noop-delete".to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write"),
        mutations,
    };
    let before = platform.export_store_snapshot().expect("before write");
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            batch.clone(),
            &absent_json_preconditions(&batch),
        )
        .expect_err("same-scope no-op delete is not part of the exact graph effects");

    assert_eq!(error.stage(), "memory_write_transaction_commit_failed");
    assert!(error
        .to_string()
        .contains("memory_write_transaction_graph_post_image_invalid"));
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn typed_graph_transaction_rejects_deleting_a_preexisting_orphan_as_an_extra_effect() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config");
    let platform = StorePlatform::open(config).expect("platform");
    let owner = graph_owner();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot.json_docs.push(StoreSnapshotJsonDoc {
        namespace: "long_term".to_string(),
        key: scoped_long_term_memory_storage_key("system", &owner.id).expect("owner key"),
        value: serde_json::to_value(owner).expect("serialize owner"),
    });
    platform
        .import_store_snapshot(&snapshot)
        .expect("seed graph owner");

    let seed_mutations = typed_graph_closure();
    let seed_batch = StoreMutationBatch {
        transaction_id: "txn-seed-before-orphan-delete".to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write"),
        mutations: seed_mutations.clone(),
    };
    platform
        .commit_governed_memory_transaction_with_preconditions(
            seed_batch.clone(),
            &absent_json_preconditions(&seed_batch),
        )
        .expect("seed exact graph closure");

    let orphan = MemoryGraphNode {
        node_id: "owner:orphan-delete".to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: "Orphan graph node selected for deletion".to_string(),
        evidence_refs: vec!["evidence:orphan-delete".to_string()],
    };
    let orphan_key =
        scoped_memory_graph_storage_key("system", "system", &format!("node:{}", orphan.node_id));
    let mut corrupted = platform.export_store_snapshot().expect("graph snapshot");
    corrupted.json_docs.push(StoreSnapshotJsonDoc {
        namespace: MEMORY_GRAPH_NODE_NAMESPACE.to_string(),
        key: orphan_key.clone(),
        value: serde_json::to_value(orphan).expect("serialize orphan"),
    });
    platform
        .import_store_snapshot(&corrupted)
        .expect("inject scoped orphan");
    let before = platform
        .export_store_snapshot()
        .expect("before replacement");

    let mut replacement_mutations = seed_mutations;
    replacement_mutations.push(StoreMutation::DeleteJson {
        namespace: MEMORY_GRAPH_NODE_NAMESPACE.to_string(),
        key: orphan_key,
        event_kind: MemoryStoreEventKind::MemoryDelete,
        plane: MEMORY_GRAPH_NODE_NAMESPACE.to_string(),
        record_key: "owner:orphan-delete".to_string(),
    });
    let replacement_batch = StoreMutationBatch {
        transaction_id: "txn-replacement-with-orphan-delete".to_string(),
        operation: "memory_graph.maintain".to_string(),
        scope: StoreEventScope::system("memory_graph.maintain"),
        mutations: replacement_mutations,
    };
    let preconditions = exact_json_preconditions(&before.json_docs, &replacement_batch);
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(replacement_batch, &preconditions)
        .expect_err("orphan delete is not part of the manifest exact effects");

    assert_eq!(error.stage(), "memory_write_transaction_commit_failed");
    assert!(error
        .to_string()
        .contains("memory_write_transaction_graph_post_image_invalid"));
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn typed_graph_delete_rejects_a_noncanonical_before_dependency_closure() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config");
    let platform = StorePlatform::open(config).expect("platform");
    let owner = graph_owner();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot.json_docs.push(StoreSnapshotJsonDoc {
        namespace: "long_term".to_string(),
        key: scoped_long_term_memory_storage_key("system", &owner.id).expect("owner key"),
        value: serde_json::to_value(owner).expect("serialize owner"),
    });
    platform
        .import_store_snapshot(&snapshot)
        .expect("seed graph owner");

    let seed_mutations = typed_graph_closure();
    let seed_batch = StoreMutationBatch {
        transaction_id: "txn-seed-before-forged-dependency".to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write"),
        mutations: seed_mutations,
    };
    platform
        .commit_governed_memory_transaction_with_preconditions(
            seed_batch.clone(),
            &absent_json_preconditions(&seed_batch),
        )
        .expect("seed exact graph closure");

    let forged_key = scoped_memory_graph_storage_key("system", "system", "node_membership:forged");
    let mut corrupted = platform.export_store_snapshot().expect("graph snapshot");
    let membership = corrupted
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE)
        .expect("node membership");
    membership.key = forged_key.clone();
    membership.value["membership_key"] = json!(forged_key.clone());
    let manifest = corrupted
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == MEMORY_GRAPH_MANIFEST_NAMESPACE)
        .expect("manifest");
    manifest.value["node_memberships"][0]["storage_key"] = json!(forged_key);
    platform
        .import_store_snapshot(&corrupted)
        .expect("inject noncanonical dependency closure");
    let before = platform.export_store_snapshot().expect("before delete");

    let delete_mutations = before
        .json_docs
        .iter()
        .filter(|doc| doc.namespace.starts_with("memory_graph_"))
        .map(|doc| StoreMutation::DeleteJson {
            namespace: doc.namespace.clone(),
            key: doc.key.clone(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: doc.namespace.clone(),
            record_key: doc.key.clone(),
        })
        .collect::<Vec<_>>();
    let delete_batch = StoreMutationBatch {
        transaction_id: "txn-delete-forged-before-dependency".to_string(),
        operation: "memory_graph.maintain".to_string(),
        scope: StoreEventScope::system("memory_graph.maintain"),
        mutations: delete_mutations,
    };
    let preconditions = exact_json_preconditions(&before.json_docs, &delete_batch);
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(delete_batch, &preconditions)
        .expect_err("noncanonical before dependency closure must fail closed");

    assert_eq!(error.stage(), "memory_write_transaction_commit_failed");
    assert!(error
        .to_string()
        .contains("memory_write_transaction_graph_post_image_invalid"));
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn raw_graph_batch_cannot_forge_integrity_repair_authority_with_operation_text() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config");
    let platform = StorePlatform::open(config).expect("platform");
    let owner = graph_owner();
    let owner_key = scoped_long_term_memory_storage_key("system", &owner.id).expect("owner key");
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot.json_docs.push(StoreSnapshotJsonDoc {
        namespace: "long_term".to_string(),
        key: owner_key.clone(),
        value: serde_json::to_value(owner).expect("serialize owner"),
    });
    platform
        .import_store_snapshot(&snapshot)
        .expect("seed graph owner");

    let seed_mutations = typed_graph_closure();
    let seed_batch = StoreMutationBatch {
        transaction_id: "txn-seed-before-forged-repair".to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write"),
        mutations: seed_mutations,
    };
    platform
        .commit_governed_memory_transaction_with_preconditions(
            seed_batch.clone(),
            &absent_json_preconditions(&seed_batch),
        )
        .expect("seed exact graph closure");

    let mut owner_missing = platform.export_store_snapshot().expect("graph snapshot");
    owner_missing
        .json_docs
        .retain(|doc| !(doc.namespace == "long_term" && doc.key == owner_key));
    platform
        .import_store_snapshot(&owner_missing)
        .expect("inject owner-missing graph");
    let before = platform.export_store_snapshot().expect("before raw repair");
    let delete_mutations = before
        .json_docs
        .iter()
        .filter(|doc| doc.namespace.starts_with("memory_graph_"))
        .map(|doc| StoreMutation::DeleteJson {
            namespace: doc.namespace.clone(),
            key: doc.key.clone(),
            event_kind: MemoryStoreEventKind::MemoryDelete,
            plane: doc.namespace.clone(),
            record_key: doc.key.clone(),
        })
        .collect::<Vec<_>>();
    let forged_repair_batch = StoreMutationBatch {
        transaction_id: "txn-forged-repair-operation".to_string(),
        operation: "memory_graph.integrity_maintenance".to_string(),
        scope: StoreEventScope::system("memory_graph.integrity_maintenance"),
        mutations: delete_mutations,
    };
    let preconditions = exact_json_preconditions(&before.json_docs, &forged_repair_batch);
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(forged_repair_batch, &preconditions)
        .expect_err("operation text cannot grant graph repair authority");

    assert_eq!(error.stage(), "memory_write_transaction_commit_failed");
    assert!(error
        .to_string()
        .contains("memory_graph_before_image_invalid"));
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn typed_graph_closure_rejects_a_preexisting_scoped_orphan_atomically() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config");
    let platform = StorePlatform::open(config).expect("platform");
    let owner = graph_owner();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot.json_docs.push(StoreSnapshotJsonDoc {
        namespace: "long_term".to_string(),
        key: scoped_long_term_memory_storage_key("system", &owner.id).expect("owner key"),
        value: serde_json::to_value(owner).expect("serialize owner"),
    });
    platform
        .import_store_snapshot(&snapshot)
        .expect("seed graph owner");

    let seed_mutations = typed_graph_closure();
    let seed_batch = StoreMutationBatch {
        transaction_id: "txn-seed-exact-graph".to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write"),
        mutations: seed_mutations.clone(),
    };
    let seed_preconditions = absent_json_preconditions(&seed_batch);
    platform
        .commit_governed_memory_transaction_with_preconditions(seed_batch, &seed_preconditions)
        .expect("seed exact graph closure");

    let mut corrupted = platform.export_store_snapshot().expect("graph snapshot");
    let orphan = MemoryGraphNode {
        node_id: "owner:orphan".to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: "Orphan graph node".to_string(),
        evidence_refs: vec!["evidence:orphan".to_string()],
    };
    let orphan_key =
        scoped_memory_graph_storage_key("system", "system", &format!("node:{}", orphan.node_id));
    corrupted.json_docs.push(StoreSnapshotJsonDoc {
        namespace: MEMORY_GRAPH_NODE_NAMESPACE.to_string(),
        key: orphan_key.clone(),
        value: serde_json::to_value(orphan).expect("serialize orphan"),
    });
    platform
        .import_store_snapshot(&corrupted)
        .expect("inject scoped orphan");
    let before = platform.export_store_snapshot().expect("before deletion");

    let delete_mutations = seed_mutations
        .into_iter()
        .map(|mutation| match mutation {
            StoreMutation::PutJson {
                namespace,
                key,
                event_kind,
                plane,
                record_key,
                ..
            } => StoreMutation::DeleteJson {
                namespace,
                key,
                event_kind,
                plane,
                record_key,
            },
            _ => panic!("typed graph closure contains only JSON mutations"),
        })
        .collect::<Vec<_>>();
    let delete_batch = StoreMutationBatch {
        transaction_id: "txn-delete-with-orphan".to_string(),
        operation: "memory_graph.maintain".to_string(),
        scope: StoreEventScope::system("memory_graph.maintain"),
        mutations: delete_mutations,
    };
    let preconditions = exact_json_preconditions(&before.json_docs, &delete_batch);
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(delete_batch, &preconditions)
        .expect_err("scoped orphan must reject graph deletion");

    assert_eq!(error.stage(), "memory_write_transaction_commit_failed");
    assert!(error
        .to_string()
        .contains("memory_write_transaction_graph_post_image_invalid"));
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
    assert!(before
        .json_docs
        .iter()
        .any(|doc| doc.namespace == MEMORY_GRAPH_NODE_NAMESPACE && doc.key == orphan_key));
}

#[test]
fn graph_v2_namespace_admission_rejects_the_entire_closure_atomically() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(5, 16));
    let platform = StorePlatform::open(config).expect("platform");
    let before_events = platform.read_events().expect("events before");

    let batch = StoreMutationBatch {
        transaction_id: "txn-graph-v2-overflow".to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write"),
        mutations: typed_graph_closure(),
    };
    let preconditions = absent_json_preconditions(&batch);
    let err = platform
        .commit_governed_memory_transaction_with_preconditions(batch, &preconditions)
        .expect_err("event admission must reject the complete graph closure");

    assert_eq!(err.stage(), "memory_write_transaction_preflight_failed");
    assert_eq!(platform.read_events().unwrap(), before_events);
    let snapshot = platform.export_store_snapshot().expect("snapshot");
    assert!(!snapshot
        .json_docs
        .iter()
        .any(|doc| doc.namespace.starts_with("memory_graph_")));
}

#[test]
fn json_namespace_read_exposes_admitted_docs_without_store_graph_semantics() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(8, 8));
    let platform = StorePlatform::open(config).expect("platform");

    let batch = StoreMutationBatch {
        transaction_id: "txn-read-namespace".to_string(),
        operation: "session_summary.write".to_string(),
        scope: StoreEventScope::system("session_summary.write"),
        mutations: vec![put_json(
            "session_summary",
            "summary:release",
            json!({"summary": "release"}),
        )],
    };
    let preconditions = absent_json_preconditions(&batch);
    platform
        .commit_governed_memory_transaction_with_preconditions(batch, &preconditions)
        .expect("summary batch commit");

    let docs = platform
        .read_json_namespace("session_summary")
        .expect("read summary namespace");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].namespace, "session_summary");
    assert_eq!(docs[0].key, "summary:release");
    assert_eq!(docs[0].value["summary"], "release");

    let err = platform
        .read_json_namespace("memory_graph_unowned_semantics")
        .expect_err("unsupported namespace must fail closed");
    assert_eq!(err.stage(), "store_json_namespace_read");
}

#[test]
fn memory_facet_index_rejects_untyped_owner_without_full_closure() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(8, 8));
    let platform = StorePlatform::open(config).expect("platform");

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: "txn-facet-index".to_string(),
                operation: "memory_facet_index.write".to_string(),
                scope: StoreEventScope::system("memory_facet_index.write"),
                mutations: vec![put_facet_index_json("facet-index:ltm:project")],
            },
            &[StoreJsonPrecondition::Absent {
                namespace: "memory_facet_indexes".to_string(),
                key: "facet-index:ltm:project".to_string(),
            }],
        )
        .expect_err("facet owner without governed owner/posting/manifest must fail closed");
    assert_eq!(error.stage(), "memory_write_transaction_commit_failed");

    let docs = platform
        .read_json_namespace("memory_facet_indexes")
        .expect("read facet index namespace");
    assert!(docs.is_empty());
}

#[test]
fn governed_transaction_rejects_owner_mutation_without_facet_closure() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");
    let key = "scoped-owner-key";
    let batch = mutation_batch(
        "txn-owner-without-facet",
        StoreMutation::PutJson {
            namespace: "long_term".to_string(),
            key: key.to_string(),
            value: json!({"id": "owner-1"}),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: "long_term".to_string(),
            record_key: "owner-1".to_string(),
        },
    );

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            batch,
            &[StoreJsonPrecondition::Absent {
                namespace: "long_term".to_string(),
                key: key.to_string(),
            }],
        )
        .expect_err("owner mutation without facet closure must fail");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_owner_facet_closure_missing"
    );
    assert!(platform
        .read_json_namespace("long_term")
        .expect("long-term namespace")
        .is_empty());
}

#[test]
fn long_term_control_namespaces_require_read_set_preconditions() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(16, 16));
    let platform = StorePlatform::open(config).expect("platform");

    for (index, namespace) in [
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
    ]
    .into_iter()
    .enumerate()
    {
        let key = format!("control-{index}");
        let error = platform
            .commit_governed_memory_transaction(mutation_batch(
                &format!("txn-control-{index}"),
                put_json(namespace, &key, json!({"id": key})),
            ))
            .expect_err("unconditional control mutation must fail closed");
        assert_eq!(
            error.stage(),
            "memory_write_transaction_precondition_missing",
            "{error}"
        );
    }
}

#[test]
fn governed_transaction_rejects_control_mutation_without_audit_closure() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");
    let key = "control-revision-without-audit";
    let batch = mutation_batch(
        "txn-control-without-audit",
        put_json(
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
            key,
            json!({"revision_id": key}),
        ),
    );

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            batch,
            &[StoreJsonPrecondition::Absent {
                namespace: LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
                key: key.to_string(),
            }],
        )
        .expect_err("control mutation without audit closure must fail");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_control_audit_closure_missing"
    );
    assert!(platform
        .read_json_namespace(LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .expect("control revision namespace")
        .is_empty());
}

#[test]
fn governed_transaction_rejects_mismatched_control_audit_binding() {
    let platform = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");
    let revision_key = "control-revision-bound-to-owner-1";
    let audit_key = "control-audit-bound-to-owner-2";
    let batch = StoreMutationBatch {
        transaction_id: "txn-control-mismatched-audit".to_string(),
        operation: "test.control_audit_binding".to_string(),
        scope: StoreEventScope::system("test.control_audit_binding"),
        mutations: vec![
            put_json(
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                revision_key,
                json!({
                    "revision_id": revision_key,
                    "record_id": "owner-1",
                    "operation": "correct",
                    "owner_revision": 2,
                    "source_revision": 1,
                    "previous_digest": "before",
                    "new_digest": "after",
                    "reason": "test",
                    "created_at": 1
                }),
            ),
            put_json(
                LONG_TERM_CONTROL_AUDIT_NAMESPACE,
                audit_key,
                json!({
                    "event_id": audit_key,
                    "operation": "correct",
                    "record_ids": ["owner-2"],
                    "record_versions": [],
                    "policy_ids": [],
                    "reason": "test",
                    "created_at": 1
                }),
            ),
        ],
    };
    let preconditions = absent_json_preconditions(&batch);

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(batch, &preconditions)
        .expect_err("mismatched control audit binding must fail");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_control_audit_binding_invalid"
    );
    assert!(platform
        .read_json_namespace(LONG_TERM_CONTROL_REVISION_NAMESPACE)
        .expect("control revision namespace")
        .is_empty());
    assert!(platform
        .read_json_namespace(LONG_TERM_CONTROL_AUDIT_NAMESPACE)
        .expect("control audit namespace")
        .is_empty());
}

#[test]
fn conditional_batch_exact_precondition_serializes_competing_writers_without_lost_update() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(8, 8));
    let platform = StorePlatform::open(config).expect("platform");
    let namespace = "session_summary";
    let key = "manifest:release";
    let v1 = json!({ "generation": 1 });
    let first_v2 = json!({ "generation": 2, "writer": "first" });
    let second_v2 = json!({ "generation": 2, "writer": "second" });

    let seed = mutation_batch("txn-manifest-v1", put_json(namespace, key, v1.clone()));
    platform
        .commit_governed_memory_transaction_with_preconditions(
            seed,
            &[StoreJsonPrecondition::Absent {
                namespace: namespace.to_string(),
                key: key.to_string(),
            }],
        )
        .expect("seed manifest v1");
    let events_before_race = platform.read_events().expect("events before race");
    let barrier = Arc::new(Barrier::new(2));
    let preconditions = vec![StoreJsonPrecondition::Exact {
        namespace: namespace.to_string(),
        key: key.to_string(),
        value: v1,
    }];

    let first_platform = platform.clone();
    let first_barrier = barrier.clone();
    let first_preconditions = preconditions.clone();
    let first = thread::spawn(move || {
        first_barrier.wait();
        first_platform.commit_governed_memory_transaction_with_preconditions(
            mutation_batch("txn-manifest-v2-first", put_json(namespace, key, first_v2)),
            &first_preconditions,
        )
    });

    let second_platform = platform.clone();
    let second_barrier = barrier.clone();
    let second = thread::spawn(move || {
        second_barrier.wait();
        second_platform.commit_governed_memory_transaction_with_preconditions(
            mutation_batch(
                "txn-manifest-v2-second",
                put_json(namespace, key, second_v2),
            ),
            &preconditions,
        )
    });

    let outcomes = [
        first.join().expect("first writer"),
        second.join().expect("second writer"),
    ];
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| {
                outcome.as_ref().is_err_and(|error| {
                    error.stage() == "memory_write_transaction_precondition_failed"
                })
            })
            .count(),
        1
    );

    let manifests = platform
        .read_json_docs_by_keys(namespace, &[key.to_string()])
        .expect("read final manifest");
    assert_eq!(manifests.len(), 1);
    assert!(
        manifests[0].value == json!({ "generation": 2, "writer": "first" })
            || manifests[0].value == json!({ "generation": 2, "writer": "second" })
    );
    assert_eq!(
        platform.read_events().unwrap().len(),
        events_before_race.len() + 1
    );
}

#[test]
fn conditional_batch_absent_precondition_rejects_existing_json_without_changes() {
    let config = StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull)
        .expect("config")
        .with_runtime_store_budget(transaction_budget(8, 8));
    let platform = StorePlatform::open(config).expect("platform");
    let namespace = "session_summary";
    let key = "manifest:absent";
    let preconditions = vec![StoreJsonPrecondition::Absent {
        namespace: namespace.to_string(),
        key: key.to_string(),
    }];

    platform
        .commit_governed_memory_transaction_with_preconditions(
            mutation_batch(
                "txn-absent-create",
                put_json(namespace, key, json!({ "generation": 1 })),
            ),
            &preconditions,
        )
        .expect("absent precondition creates manifest");
    let events_before_failure = platform.read_events().expect("events before failure");
    let json_before_failure = platform
        .read_json_docs_by_keys(namespace, &[key.to_string()])
        .expect("manifest before failure");

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            mutation_batch(
                "txn-absent-replace",
                put_json(namespace, key, json!({ "generation": 2 })),
            ),
            &preconditions,
        )
        .expect_err("existing manifest violates absent precondition");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_precondition_failed"
    );
    assert_eq!(platform.read_events().unwrap(), events_before_failure);
    assert_eq!(
        platform
            .read_json_docs_by_keys(namespace, &[key.to_string()])
            .unwrap(),
        json_before_failure
    );
}
