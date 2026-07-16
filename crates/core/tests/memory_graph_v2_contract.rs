use bm_core::memory::{
    build_memory_graph_persistence_plan, governed_memory_recall_candidate_id,
    memory_graph_backlink_key, memory_graph_recall_index_key, memory_graph_scope_digest,
    scoped_memory_graph_storage_key, validate_memory_graph_read_chain, EvidenceBacklink,
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, MemoryGraphEdge, MemoryGraphEdgeKind,
    MemoryGraphNode, MemoryGraphNodeKind, MemoryGraphNodeMembership, MemoryGraphOwnerBinding,
    MemoryGraphScopeManifest, TemporalValidity, MEMORY_GRAPH_SCHEMA_VERSION,
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
    typed_owner(id, GovernedMemoryOwnerPlane::LongTerm, id, owner_revision)
}

fn typed_owner(
    node_id: &str,
    plane: GovernedMemoryOwnerPlane,
    owner_id: &str,
    owner_revision: u64,
) -> MemoryGraphOwnerBinding {
    MemoryGraphOwnerBinding {
        node_id: node_id.to_string(),
        owner_ref: GovernedMemoryOwnerRef::new(plane, owner_id),
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
    old_schema["schema_version"] = serde_json::json!(2);
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

    let old_owner_membership = serde_json::json!({
        "schema_version": 2,
        "memory_space_id": "space:main",
        "mounted_subject_id": "subject:user",
        "scope_digest": "sha256:old",
        "manifest_generation": 1,
        "graph_revision": "graph_revision:sha256:old",
        "membership_key": "node-membership:old",
        "node_id": "node:a",
        "document_key": "node:a",
        "document_digest": "sha256:old",
        "owner_record_id": "ltm:a",
        "owner_revision": 1,
        "index_key": "index:a",
        "backlink_membership_keys": [],
        "dependency_digest": "sha256:old"
    });
    assert!(serde_json::from_value::<MemoryGraphNodeMembership>(old_owner_membership).is_err());
}

#[test]
fn graph_v4_accepts_same_owner_id_across_planes_as_distinct_composite_identities() {
    let nodes = vec![
        node("node:long-term", "evidence:shared"),
        node("node:evidence", "evidence:shared"),
    ];
    let backlinks = vec![backlink("evidence:shared")];
    let owners = vec![
        typed_owner(
            "node:long-term",
            GovernedMemoryOwnerPlane::LongTerm,
            "shared:id",
            3,
        ),
        typed_owner(
            "node:evidence",
            GovernedMemoryOwnerPlane::EvidenceDocument,
            "shared:id",
            5,
        ),
    ];

    let plan = build_memory_graph_persistence_plan(
        "space:main",
        "subject:user",
        1,
        nodes.clone(),
        Vec::new(),
        backlinks.clone(),
        owners.clone(),
    );

    assert!(plan.accepted, "{:?}", plan.failures);
    assert_eq!(MEMORY_GRAPH_SCHEMA_VERSION, 4);
    for owner in &owners {
        let membership = plan
            .node_memberships
            .iter()
            .find(|membership| membership.node_id == owner.node_id)
            .expect("node membership");
        assert_eq!(membership.owner_ref, owner.owner_ref);
    }
    assert!(
        validate_memory_graph_read_chain(
            plan.scope_manifest.as_ref().expect("manifest"),
            &plan.recall_indexes,
            &plan.node_memberships,
            &plan.edge_memberships,
            &plan.backlink_memberships,
            &nodes,
            &[],
            &backlinks,
            &owners,
        )
        .verified
    );
}

#[test]
fn graph_v4_decouples_node_identity_from_typed_owner_identity() {
    let nodes = vec![node("node:project-summary", "evidence:project")];
    let backlinks = vec![backlink("evidence:project")];
    let owners = vec![typed_owner(
        "node:project-summary",
        GovernedMemoryOwnerPlane::LongTerm,
        "ltm:project-owner",
        7,
    )];

    let plan = build_memory_graph_persistence_plan(
        "space:main",
        "subject:user",
        1,
        nodes.clone(),
        Vec::new(),
        backlinks.clone(),
        owners.clone(),
    );

    assert!(plan.accepted, "{:?}", plan.failures);
    assert_eq!(plan.node_memberships[0].node_id, "node:project-summary");
    assert_eq!(plan.node_memberships[0].owner_ref, owners[0].owner_ref);
    assert_eq!(plan.recall_indexes[0].owner_ref, owners[0].owner_ref);
    assert!(
        validate_memory_graph_read_chain(
            plan.scope_manifest.as_ref().expect("manifest"),
            &plan.recall_indexes,
            &plan.node_memberships,
            &plan.edge_memberships,
            &plan.backlink_memberships,
            &nodes,
            &[],
            &backlinks,
            &owners,
        )
        .verified
    );
}

#[test]
fn graph_v4_groups_multiple_anchor_nodes_under_one_typed_owner_index() {
    let owner_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "ltm:multi-node-owner");
    let owner_candidate_id = governed_memory_recall_candidate_id(&owner_ref);
    let nodes = vec![
        node("node:owner-view-a", "evidence:shared"),
        node("node:owner-view-b", "evidence:shared"),
    ];
    let owners = vec![
        MemoryGraphOwnerBinding {
            node_id: nodes[0].node_id.clone(),
            owner_ref: owner_ref.clone(),
            owner_revision: 9,
            visible: true,
        },
        MemoryGraphOwnerBinding {
            node_id: nodes[1].node_id.clone(),
            owner_ref: owner_ref.clone(),
            owner_revision: 9,
            visible: true,
        },
    ];
    let backlinks = vec![backlink("evidence:shared")];

    let plan = build_memory_graph_persistence_plan(
        "space:main",
        "subject:user",
        1,
        nodes.clone(),
        Vec::new(),
        backlinks.clone(),
        owners.clone(),
    );

    assert!(plan.accepted, "{:?}", plan.failures);
    assert_eq!(plan.recall_indexes.len(), 1);
    let index = &plan.recall_indexes[0];
    assert_eq!(index.owner_ref, owner_ref);
    assert_eq!(index.owner_candidate_id, owner_candidate_id);
    assert_eq!(
        index.index_key,
        memory_graph_recall_index_key("space:main", "subject:user", &owner_candidate_id)
    );
    assert_eq!(
        index.source_anchor_node_ids,
        vec!["node:owner-view-a", "node:owner-view-b"]
    );
    assert_eq!(index.node_count, 2);
    assert!(plan
        .node_memberships
        .iter()
        .all(|membership| membership.index_key == index.index_key));
    assert!(
        validate_memory_graph_read_chain(
            plan.scope_manifest.as_ref().expect("manifest"),
            &plan.recall_indexes,
            &plan.node_memberships,
            &plan.edge_memberships,
            &plan.backlink_memberships,
            &nodes,
            &[],
            &backlinks,
            &owners,
        )
        .verified
    );
}

#[test]
fn graph_v4_rejects_revision_drift_between_nodes_of_the_same_owner() {
    let owner_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "ltm:multi-node-owner");
    let plan = build_memory_graph_persistence_plan(
        "space:main",
        "subject:user",
        1,
        vec![
            node("node:owner-view-a", "evidence:shared"),
            node("node:owner-view-b", "evidence:shared"),
        ],
        Vec::new(),
        vec![backlink("evidence:shared")],
        vec![
            MemoryGraphOwnerBinding {
                node_id: "node:owner-view-a".to_string(),
                owner_ref: owner_ref.clone(),
                owner_revision: 9,
                visible: true,
            },
            MemoryGraphOwnerBinding {
                node_id: "node:owner-view-b".to_string(),
                owner_ref,
                owner_revision: 10,
                visible: true,
            },
        ],
    );

    assert!(!plan.accepted);
    assert!(plan
        .failures
        .contains(&"memory_graph_owner_revision_inconsistent".to_string()));
}

#[test]
fn graph_v4_rejects_evidence_owner_plane_revision_and_visibility_drift() {
    let nodes = vec![node("node:evidence", "evidence:document")];
    let backlinks = vec![backlink("evidence:document")];
    let owners = vec![typed_owner(
        "node:evidence",
        GovernedMemoryOwnerPlane::EvidenceDocument,
        "document:release-evidence",
        11,
    )];
    let plan = build_memory_graph_persistence_plan(
        "space:main",
        "subject:user",
        1,
        nodes.clone(),
        Vec::new(),
        backlinks.clone(),
        owners.clone(),
    );
    assert!(plan.accepted, "{:?}", plan.failures);
    let manifest = plan.scope_manifest.as_ref().expect("manifest");

    let validate = |actual_owners: &[MemoryGraphOwnerBinding]| {
        validate_memory_graph_read_chain(
            manifest,
            &plan.recall_indexes,
            &plan.node_memberships,
            &plan.edge_memberships,
            &plan.backlink_memberships,
            &nodes,
            &[],
            &backlinks,
            actual_owners,
        )
    };

    let mut wrong_plane = owners.clone();
    let wrong_plane_owner_id = wrong_plane[0].owner_ref.owner_id.clone();
    wrong_plane[0].owner_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, wrong_plane_owner_id);
    assert!(validate(&wrong_plane)
        .failures
        .contains(&"memory_graph_owner_missing".to_string()));

    let mut wrong_revision = owners.clone();
    wrong_revision[0].owner_revision += 1;
    assert!(validate(&wrong_revision)
        .failures
        .contains(&"memory_graph_owner_revision_drift".to_string()));

    let mut hidden = owners;
    hidden[0].visible = false;
    assert!(validate(&hidden)
        .failures
        .contains(&"memory_graph_owner_privacy_scope_restricted".to_string()));
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
