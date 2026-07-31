mod support;

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    MemoryGraphNode, MemoryGraphNodeKind, MEMORY_FACET_INDEX_NAMESPACE, MEMORY_GRAPH_NODE_NAMESPACE,
};
use bm_sdk::nonproduction_replay_harness::StoreBackendConfig;

fn empty_store() -> bm_sdk::nonproduction_replay_harness::StorePlatform {
    support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("in-memory config"),
    )
    .expect("empty store")
}

#[test]
fn direct_read_rejects_malformed_typed_facet_document() {
    let store = empty_store();
    let key = "forged-facet";
    store
        .tamper_json_document_for_nonproduction_harness(
            MEMORY_FACET_INDEX_NAMESPACE,
            key,
            serde_json::json!({"unexpected": true}),
        )
        .expect("inject malformed typed facet");

    let error = store
        .read_json_docs_by_keys(MEMORY_FACET_INDEX_NAMESPACE, &[key.to_string()])
        .expect_err("known-key read must decode typed facet values");

    assert_eq!(error.stage(), "store_json_known_key_read");
}

#[test]
fn complete_snapshot_rejects_a_noncanonical_orphan_graph_document() {
    let store = empty_store();
    let orphan = MemoryGraphNode {
        node_id: "orphan-node".to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: "orphan graph node".to_string(),
        evidence_refs: vec!["evidence:orphan".to_string()],
    };
    store
        .tamper_json_document_for_nonproduction_harness(
            MEMORY_GRAPH_NODE_NAMESPACE,
            "noncanonical-orphan-key",
            serde_json::to_value(orphan).expect("serialize graph node"),
        )
        .expect("inject orphan graph node");

    let error = store
        .export_store_snapshot()
        .expect_err("complete snapshot must reject graph documents absent from memberships");

    assert_eq!(error.stage(), "store_snapshot_export");
}
