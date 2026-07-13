use bm_core::memory::{
    build_memory_graph_persistence_plan, memory_graph_backlink_key, memory_graph_scope_digest,
    scoped_memory_graph_storage_key, validate_memory_graph_read_chain, EvidenceBacklink,
    MemoryGraphEdge, MemoryGraphEdgeKind, MemoryGraphNode, MemoryGraphNodeKind,
    MemoryGraphOwnerBinding, MemoryGraphScopeManifest, TemporalValidity,
    MEMORY_GRAPH_SCHEMA_VERSION,
};

fn node(id: &str, evidence_ref: &str) -> MemoryGraphNode {
    MemoryGraphNode {
        node_id: id.to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: format!("governed {id}"),
        evidence_refs: vec![evidence_ref.to_string()],
    }
}

fn edge(id: &str, from: &str, to: &str, evidence_ref: &str) -> MemoryGraphEdge {
    MemoryGraphEdge {
        edge_id: id.to_string(),
        kind: MemoryGraphEdgeKind::Supports,
        from_node_id: from.to_string(),
        to_node_id: to.to_string(),
        validity: TemporalValidity {
            valid_from: 10,
            valid_until: None,
            observed_at: 10,
            superseded_by: None,
        },
        evidence_refs: vec![evidence_ref.to_string()],
    }
}

fn backlink(evidence_ref: &str) -> EvidenceBacklink {
    EvidenceBacklink {
        source_kind: "long_term_memory".to_string(),
        source_id: evidence_ref.to_string(),
        fingerprint: format!("fp:{evidence_ref}"),
    }
}

fn owner(id: &str, owner_revision: u64) -> MemoryGraphOwnerBinding {
    MemoryGraphOwnerBinding {
        owner_record_id: id.to_string(),
        owner_revision,
        visible: true,
    }
}

#[test]
fn graph_v2_manifest_and_read_chain_bind_scope_generation_revision_and_owner() {
    let nodes = vec![
        node("ltm:a", "evidence:shared"),
        node("ltm:b", "evidence:shared"),
    ];
    let edges = vec![edge("edge:a:b", "ltm:a", "ltm:b", "evidence:shared")];
    let backlinks = vec![backlink("evidence:shared")];
    let owners = vec![owner("ltm:a", 3), owner("ltm:b", 7)];

    let plan = build_memory_graph_persistence_plan(
        "space:main",
        "subject:user",
        4,
        nodes.clone(),
        edges.clone(),
        backlinks.clone(),
        owners.clone(),
    );

    assert!(plan.accepted, "{:?}", plan.failures);
    let manifest = plan.scope_manifest.as_ref().expect("scope manifest");
    assert_eq!(manifest.schema_version, MEMORY_GRAPH_SCHEMA_VERSION);
    assert_eq!(manifest.manifest_generation, 4);
    assert_eq!(manifest.node_count, 2);
    assert_eq!(manifest.edge_count, 1);
    assert_eq!(manifest.backlink_count, 1);
    assert_eq!(manifest.index_count, 2);
    assert!(!manifest.scope_digest.is_empty());
    assert!(!manifest.graph_revision.is_empty());
    assert!(!manifest.dependency_digest.is_empty());
    assert!(plan.recall_indexes.iter().all(|index| {
        index.schema_version == MEMORY_GRAPH_SCHEMA_VERSION
            && index.manifest_generation == manifest.manifest_generation
            && index.graph_revision == manifest.graph_revision
            && !index.dependency_digest.is_empty()
            && index.memory_space_id == manifest.memory_space_id
            && index.mounted_subject_id == manifest.mounted_subject_id
            && index.owner_revision > 0
    }));

    let validation = validate_memory_graph_read_chain(
        manifest,
        &plan.recall_indexes,
        &plan.node_memberships,
        &plan.edge_memberships,
        &plan.backlink_memberships,
        &nodes,
        &edges,
        &backlinks,
        &owners,
    );
    assert!(validation.verified, "{:?}", validation.failures);

    let mut drifted_owner = owners;
    drifted_owner[0].owner_revision += 1;
    let drifted = validate_memory_graph_read_chain(
        manifest,
        &plan.recall_indexes,
        &plan.node_memberships,
        &plan.edge_memberships,
        &plan.backlink_memberships,
        &nodes,
        &edges,
        &backlinks,
        &drifted_owner,
    );
    assert!(!drifted.verified);
    assert!(drifted
        .failures
        .contains(&"memory_graph_owner_revision_drift".to_string()));
}

#[test]
fn graph_v2_rejects_manifest_count_generation_digest_drift_and_old_schema() {
    let nodes = vec![node("ltm:a", "evidence:a")];
    let edges = Vec::new();
    let backlinks = vec![backlink("evidence:a")];
    let owners = vec![owner("ltm:a", 1)];
    let plan = build_memory_graph_persistence_plan(
        "space:main",
        "subject:user",
        1,
        nodes.clone(),
        edges.clone(),
        backlinks.clone(),
        owners.clone(),
    );
    assert!(plan.accepted, "{:?}", plan.failures);
    let manifest = plan.scope_manifest.as_ref().expect("scope manifest");

    let mut count_drift = manifest.clone();
    count_drift.node_count += 1;
    let validation = validate_memory_graph_read_chain(
        &count_drift,
        &plan.recall_indexes,
        &plan.node_memberships,
        &plan.edge_memberships,
        &plan.backlink_memberships,
        &nodes,
        &edges,
        &backlinks,
        &owners,
    );
    assert!(!validation.verified);
    assert!(validation
        .failures
        .contains(&"memory_graph_manifest_count_drift".to_string()));

    let mut generation_drift = plan.recall_indexes.clone();
    generation_drift[0].manifest_generation += 1;
    let validation = validate_memory_graph_read_chain(
        manifest,
        &generation_drift,
        &plan.node_memberships,
        &plan.edge_memberships,
        &plan.backlink_memberships,
        &nodes,
        &edges,
        &backlinks,
        &owners,
    );
    assert!(!validation.verified);
    assert!(validation
        .failures
        .contains(&"memory_graph_index_manifest_generation_drift".to_string()));

    let mut digest_drift = plan.node_memberships.clone();
    digest_drift[0].document_digest.push_str("-drift");
    let validation = validate_memory_graph_read_chain(
        manifest,
        &plan.recall_indexes,
        &digest_drift,
        &plan.edge_memberships,
        &plan.backlink_memberships,
        &nodes,
        &edges,
        &backlinks,
        &owners,
    );
    assert!(!validation.verified);
    assert!(validation
        .failures
        .iter()
        .any(|failure| failure.contains("dependency_digest")));

    let mut old_schema = serde_json::to_value(manifest).expect("manifest json");
    old_schema["schema_version"] = serde_json::json!(1);
    let old_manifest =
        serde_json::from_value::<MemoryGraphScopeManifest>(old_schema).expect("typed old manifest");
    let validation = validate_memory_graph_read_chain(
        &old_manifest,
        &plan.recall_indexes,
        &plan.node_memberships,
        &plan.edge_memberships,
        &plan.backlink_memberships,
        &nodes,
        &edges,
        &backlinks,
        &owners,
    );
    assert!(!validation.verified);
    assert!(validation
        .failures
        .contains(&"memory_graph_schema_version_unsupported".to_string()));

    let mut missing_schema = serde_json::to_value(manifest).expect("manifest json");
    missing_schema
        .as_object_mut()
        .expect("manifest object")
        .remove("schema_version");
    assert!(serde_json::from_value::<MemoryGraphScopeManifest>(missing_schema).is_err());
}

#[test]
fn graph_v2_persistent_keys_and_dependency_digests_use_explicit_sha256_contract() {
    let scope = memory_graph_scope_digest("space:main", "subject:user");
    let storage_key = scoped_memory_graph_storage_key("space:main", "subject:user", "node:ltm:a");
    let backlink_key = memory_graph_backlink_key("long_term_memory", "evidence:a");

    assert!(scope.starts_with("sha256:"));
    assert_eq!(scope.len(), "sha256:".len() + 64);
    assert!(storage_key.contains(":doc:sha256:"));
    assert!(backlink_key.starts_with("sha256:"));
    assert_eq!(backlink_key.len(), "sha256:".len() + 64);

    let plan = build_memory_graph_persistence_plan(
        "space:main",
        "subject:user",
        1,
        vec![node("ltm:a", "evidence:a")],
        Vec::new(),
        vec![backlink("evidence:a")],
        vec![owner("ltm:a", 1)],
    );
    let manifest = plan.scope_manifest.expect("manifest");
    assert!(manifest.dependency_digest.starts_with("sha256:"));
    assert!(manifest
        .graph_revision
        .starts_with("graph_revision:sha256:"));
    assert!(plan
        .node_memberships
        .iter()
        .all(|membership| membership.dependency_digest.starts_with("sha256:")));
}

#[test]
fn graph_v2_plans_ten_thousand_node_chain_with_bounded_two_hop_dependencies() {
    const NODE_COUNT: usize = 10_000;
    let nodes = (0..NODE_COUNT)
        .map(|index| node(&format!("ltm:{index}"), &format!("evidence:{index}")))
        .collect::<Vec<_>>();
    let edges = (1..NODE_COUNT)
        .map(|index| {
            edge(
                &format!("edge:{}:{index}", index - 1),
                &format!("ltm:{}", index - 1),
                &format!("ltm:{index}"),
                &format!("evidence:{index}"),
            )
        })
        .collect::<Vec<_>>();
    let backlinks = (0..NODE_COUNT)
        .map(|index| backlink(&format!("evidence:{index}")))
        .collect::<Vec<_>>();
    let owners = (0..NODE_COUNT)
        .map(|index| owner(&format!("ltm:{index}"), 1))
        .collect::<Vec<_>>();

    let plan = build_memory_graph_persistence_plan(
        "space:large-chain",
        "subject:user",
        1,
        nodes,
        edges,
        backlinks,
        owners,
    );

    assert!(plan.accepted, "{:?}", plan.failures);
    assert_eq!(plan.node_memberships.len(), NODE_COUNT);
    assert_eq!(plan.edge_memberships.len(), NODE_COUNT - 1);
    assert_eq!(plan.backlink_memberships.len(), NODE_COUNT);
    assert_eq!(plan.recall_indexes.len(), NODE_COUNT);
    assert!(plan
        .recall_indexes
        .iter()
        .all(|index| index.node_memberships.len() <= 5));
}
