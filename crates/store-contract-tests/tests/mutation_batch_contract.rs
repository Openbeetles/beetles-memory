mod support;
use bm_core::budget::StoreRuntimeBudget;
use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    bind_long_term_version_creation, build_governed_evidence_document_facet_index_doc,
    build_long_term_memory_facet_index_doc, build_memory_graph_persistence_plan,
    canonical_recall_evidence_group, governed_evidence_document_content_digest,
    governed_evidence_source_ref_from_document, long_term_version_head_key,
    long_term_version_material_key, memory_facet_manifest_key, memory_graph_scope_manifest_key,
    scoped_governed_evidence_document_key, scoped_long_term_control_storage_key,
    scoped_memory_facet_owner_storage_key, scoped_memory_graph_storage_key, ControlEffectRef,
    EvidenceBacklink, GovernedEvidenceDocument, GovernedEvidenceDocumentChunk,
    GovernedEvidenceDocumentSourceKind, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    GovernedOwnerRevisionRef, GovernedOwnerTermination, GovernedOwnerTransition,
    LongTermControlOperation, LongTermMemoryConfidence, LongTermMemoryControlAuditEvent,
    LongTermMemoryControlRevision, LongTermMemoryEntry, LongTermMemoryFreshness,
    LongTermMemoryKind, LongTermMemorySourceScope, LongTermMemorySourceType,
    LongTermMemoryTombstone, LongTermMemoryVersionCreateIntent, LongTermMemoryVersionScopeManifest,
    LongTermVersionRetentionLease, MemoryEvidenceAuthority, MemoryFacetIndexDoc,
    MemoryFacetIndexManifest, MemoryFacetOwnerVersion, MemoryFacetPostingDoc,
    MemoryFacetPostingRevision, MemoryGovernanceSelector, MemoryGovernanceSuppressionDuration,
    MemoryGraphNode, MemoryGraphNodeKind, MemoryGraphOwnerBinding, MemoryLongTermGovernancePolicy,
    MemoryPrivacyClass, LONG_TERM_CONTROL_AUDIT_NAMESPACE, LONG_TERM_CONTROL_REVISION_NAMESPACE,
    LONG_TERM_CONTROL_SCHEMA_VERSION, LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
    LONG_TERM_GOVERNANCE_POLICY_NAMESPACE, MEMORY_FACET_INDEX_NAMESPACE,
    MEMORY_FACET_POSTING_NAMESPACE, MEMORY_FACET_SCHEMA_VERSION,
    MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE, MEMORY_GRAPH_BACKLINK_NAMESPACE,
    MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE, MEMORY_GRAPH_INDEX_NAMESPACE,
    MEMORY_GRAPH_MANIFEST_NAMESPACE, MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
    MEMORY_GRAPH_NODE_NAMESPACE, MEMORY_GRAPH_REVISION_NAMESPACE,
};
use bm_core::platform::Platform as _;
use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};
use std::thread;

use bm_sdk::nonproduction_replay_harness::{
    GovernedEvidenceOwnerClaimBinding, GovernedEvidenceSourceClaimManifest, MemoryStoreEvent,
    MemoryStoreEventKind, StoreBackendConfig, StoreEventScope, StoreJsonPrecondition,
    StoreMutation, StoreMutationBatch, StorePlatform, StoreSnapshotJsonDoc,
    GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE, LONG_TERM_HEAD_MANIFEST_NAMESPACE,
    LONG_TERM_VERSION_MATERIAL_NAMESPACE, LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
};
use serde_json::json;

fn transaction_budget(event_log_max_items: usize) -> StoreRuntimeBudget {
    StoreRuntimeBudget {
        metric_source_max_items: 1,
        event_log_max_items,
        kv_max_entries: 256,
        blob_max_bytes: 1024,
        snapshot_max_bytes: 16_384,
        logical_namespace_max_bytes: 64,
        logical_key_max_bytes: 64,
        event_record_key_max_bytes: 64,
        export_max_bytes: 16_384,
        import_max_bytes: 16_384,
    }
}

fn typed_control_revision(
    revision_id: &str,
    record_id: &str,
    mounted_subject_id: &str,
) -> LongTermMemoryControlRevision {
    let owner_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, record_id.to_string());
    let transition = GovernedOwnerTransition {
        predecessor: GovernedOwnerRevisionRef::try_new(owner_ref.clone(), 1)
            .expect("predecessor revision"),
        terminated_at: 2,
        termination: GovernedOwnerTermination::Corrected,
        successor: Some(
            GovernedOwnerRevisionRef::try_new(owner_ref, 2).expect("successor revision"),
        ),
    };
    let mut revision = LongTermMemoryControlRevision {
        schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
        revision_id: revision_id.to_string(),
        memory_space_id: "system".to_string(),
        mounted_subject_id: mounted_subject_id.to_string(),
        operation: LongTermControlOperation::Correct,
        invalidation_reason_code: None,
        transition,
        predecessor_material_digest: "a".repeat(64),
        successor_material_digest: Some("b".repeat(64)),
        governed_evidence_refs: Vec::new(),
        reason: "test".to_string(),
        actor_subject_id: None,
        created_at: 1,
        content_digest: String::new(),
    };
    revision.content_digest = revision
        .canonical_content_digest()
        .expect("control revision digest");
    revision
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

fn typed_long_term_scope_docs(
    memory_space_id: &str,
    mounted_subject_id: &str,
    owners: Vec<LongTermMemoryEntry>,
) -> Vec<StoreSnapshotJsonDoc> {
    let lease = LongTermVersionRetentionLease::try_new(1).expect("retention lease");
    let mut materials = Vec::with_capacity(owners.len());
    let mut heads = Vec::with_capacity(owners.len());
    let mut facets = Vec::with_capacity(owners.len());
    let mut posting_owners = BTreeMap::<String, Vec<MemoryFacetOwnerVersion>>::new();
    for owner in owners {
        let facet = build_long_term_memory_facet_index_doc(
            &owner,
            memory_space_id,
            vec![mounted_subject_id.to_string()],
            owner.owner_revision,
        )
        .expect("build typed long-term facet owner");
        let owner_version = MemoryFacetOwnerVersion {
            owner_ref: facet.owner_ref.clone(),
            owner_revision: facet.owner_revision,
            facet_index_revision: facet.facet_index_revision,
        };
        for posting_key in facet
            .posting_keys_for_subject(mounted_subject_id)
            .expect("build typed long-term facet postings")
        {
            posting_owners
                .entry(posting_key)
                .or_default()
                .push(owner_version.clone());
        }
        facets.push(facet);
        let requested_at = owner.updated_at;
        let creation = bind_long_term_version_creation(
            LongTermMemoryVersionCreateIntent {
                memory_space_id: memory_space_id.to_string(),
                mounted_subject_id: mounted_subject_id.to_string(),
                projection: owner,
                governed_evidence_refs: Vec::new(),
                requested_at,
            },
            lease,
        )
        .expect("bind typed long-term owner");
        materials.push(creation.material);
        heads.push(creation.head);
    }
    let root = LongTermMemoryVersionScopeManifest::build(
        memory_space_id,
        mounted_subject_id,
        1,
        &heads,
        &materials,
        &[],
        &[],
        lease.max_retained_revisions_per_owner(),
    )
    .expect("build typed long-term scope root");
    let postings = posting_owners
        .into_iter()
        .map(|(posting_key, mut owner_versions)| {
            owner_versions.sort();
            MemoryFacetPostingDoc {
                schema_version: MEMORY_FACET_SCHEMA_VERSION,
                memory_space_id: memory_space_id.to_string(),
                subject_id: mounted_subject_id.to_string(),
                posting_key,
                revision: 1,
                owner_versions,
            }
        })
        .collect::<Vec<_>>();
    let mut owner_versions = facets
        .iter()
        .map(|facet| MemoryFacetOwnerVersion {
            owner_ref: facet.owner_ref.clone(),
            owner_revision: facet.owner_revision,
            facet_index_revision: facet.facet_index_revision,
        })
        .collect::<Vec<_>>();
    owner_versions.sort();
    let facet_manifest = MemoryFacetIndexManifest {
        schema_version: MEMORY_FACET_SCHEMA_VERSION,
        memory_space_id: memory_space_id.to_string(),
        subject_id: mounted_subject_id.to_string(),
        owner_doc_count: facets.len(),
        posting_doc_count: postings.len(),
        revision: 1,
        owner_versions,
        posting_revisions: postings
            .iter()
            .map(|posting| MemoryFacetPostingRevision {
                posting_key: posting.posting_key.clone(),
                revision: posting.revision,
            })
            .collect(),
    };

    let mut docs =
        Vec::with_capacity(materials.len() + heads.len() + facets.len() + postings.len() + 2);
    docs.extend(materials.iter().map(|material| {
        StoreSnapshotJsonDoc {
            namespace: LONG_TERM_VERSION_MATERIAL_NAMESPACE.to_string(),
            key: long_term_version_material_key(
                memory_space_id,
                mounted_subject_id,
                &material.owner_ref,
                material.owner_revision,
            )
            .expect("material key"),
            value: serde_json::to_value(material).expect("serialize material"),
        }
    }));
    docs.extend(heads.iter().map(|head| {
        StoreSnapshotJsonDoc {
            namespace: LONG_TERM_HEAD_MANIFEST_NAMESPACE.to_string(),
            key: long_term_version_head_key(memory_space_id, mounted_subject_id, &head.owner_ref)
                .expect("head key"),
            value: serde_json::to_value(head).expect("serialize head"),
        }
    }));
    docs.push(StoreSnapshotJsonDoc {
        namespace: LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE.to_string(),
        key: root.physical_key.clone(),
        value: serde_json::to_value(root).expect("serialize scope root"),
    });
    docs.extend(facets.iter().map(|facet| {
        StoreSnapshotJsonDoc {
            namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
            key: scoped_memory_facet_owner_storage_key(
                memory_space_id,
                mounted_subject_id,
                &facet.owner_ref,
            )
            .expect("facet owner key"),
            value: serde_json::to_value(facet).expect("serialize facet owner"),
        }
    }));
    docs.extend(postings.iter().map(|posting| StoreSnapshotJsonDoc {
        namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
        key: posting.posting_key.clone(),
        value: serde_json::to_value(posting).expect("serialize facet posting"),
    }));
    docs.push(StoreSnapshotJsonDoc {
        namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
        key: memory_facet_manifest_key(memory_space_id, mounted_subject_id)
            .expect("facet manifest key"),
        value: serde_json::to_value(facet_manifest).expect("serialize facet manifest"),
    });
    docs
}

fn typed_long_term_scope_docs_without_facets(
    memory_space_id: &str,
    mounted_subject_id: &str,
    owners: Vec<LongTermMemoryEntry>,
) -> Vec<StoreSnapshotJsonDoc> {
    typed_long_term_scope_docs(memory_space_id, mounted_subject_id, owners)
        .into_iter()
        .filter(|doc| {
            doc.namespace != MEMORY_FACET_INDEX_NAMESPACE
                && doc.namespace != MEMORY_FACET_POSTING_NAMESPACE
        })
        .collect()
}

fn raw_graph_docs(platform: &StorePlatform) -> Vec<StoreSnapshotJsonDoc> {
    let mut docs = [
        MEMORY_GRAPH_MANIFEST_NAMESPACE,
        MEMORY_GRAPH_REVISION_NAMESPACE,
        MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_EDGE_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_BACKLINK_MEMBERSHIP_NAMESPACE,
        MEMORY_GRAPH_INDEX_NAMESPACE,
        MEMORY_GRAPH_NODE_NAMESPACE,
        bm_core::memory::MEMORY_GRAPH_EDGE_NAMESPACE,
        bm_core::memory::MEMORY_GRAPH_BACKLINK_NAMESPACE,
    ]
    .into_iter()
    .flat_map(|namespace| {
        platform
            .read_json_namespace_unchecked_for_nonproduction_harness(namespace)
            .unwrap_or_else(|error| panic!("read raw graph namespace {namespace}: {error}"))
    })
    .collect::<Vec<_>>();
    docs.sort_by(|left, right| {
        left.namespace
            .cmp(&right.namespace)
            .then_with(|| left.key.cmp(&right.key))
    });
    docs
}

fn typed_graph_closure_for(
    memory_space_id: &str,
    subject_id: &str,
    owner_id: &str,
) -> Vec<StoreMutation> {
    typed_graph_closure_for_owner(
        memory_space_id,
        subject_id,
        owner_id,
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner_id),
        1,
    )
}

fn typed_graph_closure_for_owner(
    memory_space_id: &str,
    subject_id: &str,
    node_id: &str,
    owner_ref: GovernedMemoryOwnerRef,
    owner_revision: u64,
) -> Vec<StoreMutation> {
    let node = MemoryGraphNode {
        node_id: node_id.to_string(),
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
            node_id: node_id.to_string(),
            owner_ref,
            owner_revision,
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

const GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE: &str = "governed_evidence_documents";
const GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE: &str = "governed_evidence_source_refs";

fn governed_evidence_document(document_id: &str, owner_revision: u64) -> GovernedEvidenceDocument {
    let memory_space_id = "system";
    let source_locator = "opaque://store-contract/evidence-owner".to_string();
    let canonical_evidence_group = canonical_recall_evidence_group("evidence:store-contract:owner");
    let body = "Evidence owners require facet and graph projection cascades.".to_string();
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "section:owner-cascade".to_string(),
        ordinal: 0,
        body: "typed owner closure".to_string(),
    }];
    GovernedEvidenceDocument {
        schema_version: bm_core::memory::GOVERNED_EVIDENCE_DOCUMENT_SCHEMA_VERSION,
        physical_key: scoped_governed_evidence_document_key(memory_space_id, document_id)
            .expect("evidence owner key"),
        memory_space_id: memory_space_id.to_string(),
        mounted_subject_id: "system".to_string(),
        document_id: document_id.to_string(),
        source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
        source_locator: source_locator.clone(),
        canonical_evidence_group: canonical_evidence_group.clone(),
        evidence_family_group: None,
        source_revision: owner_revision,
        owner_revision,
        content_digest: governed_evidence_document_content_digest(
            &source_locator,
            &canonical_evidence_group,
            None,
            &body,
            &chunks,
        ),
        body,
        chunks,
        authority: MemoryEvidenceAuthority::UserAsserted,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        observed_at: 10,
        created_at: 10,
        updated_at: 10 + owner_revision - 1,
    }
}

fn evidence_facet_state(
    document: &GovernedEvidenceDocument,
) -> (
    MemoryFacetIndexDoc,
    Vec<MemoryFacetPostingDoc>,
    MemoryFacetIndexManifest,
) {
    let facet = build_governed_evidence_document_facet_index_doc(
        document,
        vec![document.mounted_subject_id.clone()],
        document.owner_revision,
    )
    .expect("valid evidence facet owner");
    let owner_version = MemoryFacetOwnerVersion {
        owner_ref: facet.owner_ref.clone(),
        owner_revision: facet.owner_revision,
        facet_index_revision: facet.facet_index_revision,
    };
    let postings = facet
        .posting_keys_for_subject(&document.mounted_subject_id)
        .expect("evidence facet posting keys")
        .into_iter()
        .map(|posting_key| MemoryFacetPostingDoc {
            schema_version: MEMORY_FACET_SCHEMA_VERSION,
            memory_space_id: document.memory_space_id.clone(),
            subject_id: document.mounted_subject_id.clone(),
            posting_key,
            revision: document.owner_revision,
            owner_versions: vec![owner_version.clone()],
        })
        .collect::<Vec<_>>();
    let manifest = MemoryFacetIndexManifest {
        schema_version: MEMORY_FACET_SCHEMA_VERSION,
        memory_space_id: document.memory_space_id.clone(),
        subject_id: document.mounted_subject_id.clone(),
        owner_doc_count: 1,
        posting_doc_count: postings.len(),
        revision: document.owner_revision,
        owner_versions: vec![owner_version],
        posting_revisions: postings
            .iter()
            .map(|posting| MemoryFacetPostingRevision {
                posting_key: posting.posting_key.clone(),
                revision: posting.revision,
            })
            .collect(),
    };
    (facet, postings, manifest)
}

fn evidence_facet_cascade(
    before: Option<&GovernedEvidenceDocument>,
    after: &GovernedEvidenceDocument,
) -> (Vec<StoreMutation>, Vec<StoreJsonPrecondition>) {
    let (after_facet, after_postings, after_manifest) = evidence_facet_state(after);
    let before_state = before.map(evidence_facet_state);
    if let Some((_, before_postings, _)) = before_state.as_ref() {
        assert_eq!(
            before_postings
                .iter()
                .map(|posting| &posting.posting_key)
                .collect::<Vec<_>>(),
            after_postings
                .iter()
                .map(|posting| &posting.posting_key)
                .collect::<Vec<_>>(),
            "revision-only evidence update must preserve posting identities"
        );
    }

    let facet_key = scoped_memory_facet_owner_storage_key(
        &after.memory_space_id,
        &after.mounted_subject_id,
        &after_facet.owner_ref,
    )
    .expect("evidence facet owner key");
    let manifest_key = memory_facet_manifest_key(&after.memory_space_id, &after.mounted_subject_id)
        .expect("evidence facet manifest key");
    let after_source_ref =
        governed_evidence_source_ref_from_document(after).expect("valid evidence source ref");
    let before_source_ref = before.map(|document| {
        governed_evidence_source_ref_from_document(document)
            .expect("valid prior evidence source ref")
    });
    let after_source_claim_manifest = GovernedEvidenceSourceClaimManifest::build(
        &after.memory_space_id,
        &after.mounted_subject_id,
        [
            GovernedEvidenceOwnerClaimBinding::from_document_claim(after, &after_source_ref)
                .expect("valid evidence owner-claim binding"),
        ],
        256,
    )
    .expect("valid evidence source claim manifest");
    let before_source_claim_manifest =
        before
            .zip(before_source_ref.as_ref())
            .map(|(document, source_ref)| {
                GovernedEvidenceSourceClaimManifest::build(
                    &document.memory_space_id,
                    &document.mounted_subject_id,
                    [
                        GovernedEvidenceOwnerClaimBinding::from_document_claim(
                            document, source_ref,
                        )
                        .expect("valid prior evidence owner-claim binding"),
                    ],
                    256,
                )
                .expect("valid prior evidence source claim manifest")
            });
    let mut mutations = vec![StoreMutation::PutJson {
        namespace: GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
        key: after.physical_key.clone(),
        value: serde_json::to_value(after).expect("serialize evidence owner"),
        event_kind: MemoryStoreEventKind::MemoryWrite,
        plane: GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
        record_key: after.document_id.clone(),
    }];
    if let Some(before_source_ref) = before_source_ref.as_ref() {
        if before_source_ref.physical_key != after_source_ref.physical_key {
            mutations.push(StoreMutation::DeleteJson {
                namespace: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                key: before_source_ref.physical_key.clone(),
                event_kind: MemoryStoreEventKind::MemoryDelete,
                plane: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                record_key: after.document_id.clone(),
            });
        }
    }
    mutations.extend([
        StoreMutation::PutJson {
            namespace: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
            key: after_source_ref.physical_key.clone(),
            value: serde_json::to_value(&after_source_ref).expect("serialize evidence source ref"),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
            record_key: after.document_id.clone(),
        },
        StoreMutation::PutJson {
            namespace: GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
            key: after_source_claim_manifest.physical_key.clone(),
            value: serde_json::to_value(&after_source_claim_manifest)
                .expect("serialize evidence source claim manifest"),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
            record_key: after_source_claim_manifest.physical_key.clone(),
        },
        put_json(
            MEMORY_FACET_INDEX_NAMESPACE,
            &facet_key,
            serde_json::to_value(&after_facet).expect("serialize evidence facet owner"),
        ),
    ]);
    mutations.extend(after_postings.iter().map(|posting| {
        put_json(
            MEMORY_FACET_POSTING_NAMESPACE,
            &posting.posting_key,
            serde_json::to_value(posting).expect("serialize evidence facet posting"),
        )
    }));
    mutations.push(put_json(
        MEMORY_FACET_POSTING_NAMESPACE,
        &manifest_key,
        serde_json::to_value(&after_manifest).expect("serialize evidence facet manifest"),
    ));

    let mut preconditions = Vec::new();
    match before {
        Some(before) => {
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                key: before.physical_key.clone(),
                value: serde_json::to_value(before).expect("serialize prior evidence owner"),
            });
            let before_source_ref = before_source_ref
                .as_ref()
                .expect("prior evidence source ref");
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                key: before_source_ref.physical_key.clone(),
                value: serde_json::to_value(before_source_ref)
                    .expect("serialize prior evidence source ref"),
            });
            let before_source_claim_manifest = before_source_claim_manifest
                .as_ref()
                .expect("prior evidence source claim manifest");
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
                key: before_source_claim_manifest.physical_key.clone(),
                value: serde_json::to_value(before_source_claim_manifest)
                    .expect("serialize prior evidence source claim manifest"),
            });
            if before_source_ref.physical_key != after_source_ref.physical_key {
                preconditions.push(StoreJsonPrecondition::Absent {
                    namespace: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                    key: after_source_ref.physical_key.clone(),
                });
            }
        }
        None => {
            preconditions.push(StoreJsonPrecondition::Absent {
                namespace: GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                key: after.physical_key.clone(),
            });
            preconditions.push(StoreJsonPrecondition::Absent {
                namespace: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                key: after_source_ref.physical_key.clone(),
            });
            preconditions.push(StoreJsonPrecondition::Absent {
                namespace: GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
                key: after_source_claim_manifest.physical_key.clone(),
            });
        }
    }
    match before_state {
        Some((before_facet, before_postings, before_manifest)) => {
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: MEMORY_FACET_INDEX_NAMESPACE.to_string(),
                key: facet_key,
                value: serde_json::to_value(before_facet)
                    .expect("serialize prior evidence facet owner"),
            });
            preconditions.extend(before_postings.into_iter().map(|posting| {
                let key = posting.posting_key.clone();
                StoreJsonPrecondition::Exact {
                    namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                    key,
                    value: serde_json::to_value(posting)
                        .expect("serialize prior evidence facet posting"),
                }
            }));
            preconditions.push(StoreJsonPrecondition::Exact {
                namespace: MEMORY_FACET_POSTING_NAMESPACE.to_string(),
                key: manifest_key,
                value: serde_json::to_value(before_manifest)
                    .expect("serialize prior evidence facet manifest"),
            });
        }
        None => preconditions.extend(absent_json_preconditions(&StoreMutationBatch {
            transaction_id: "evidence-facet-preconditions".to_string(),
            operation: "test.evidence_facet_preconditions".to_string(),
            scope: StoreEventScope::system("test.evidence_facet_preconditions"),
            mutations: mutations
                .iter()
                .filter(|mutation| {
                    !matches!(
                        mutation,
                        StoreMutation::PutJson { namespace, .. }
                            if namespace == GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE
                                || namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE
                                || namespace
                                    == GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE
                    )
                })
                .cloned()
                .collect(),
        })),
    }
    (mutations, preconditions)
}

fn evidence_lifecycle_mutation(
    event_id: &str,
    transaction_id: &str,
    operation: &str,
) -> StoreMutation {
    StoreMutation::AppendEvent {
        event: Box::new(
            MemoryStoreEvent::new(
                event_id,
                MemoryStoreEventKind::RuntimeLifecycle,
                StoreEventScope::system("maintain"),
                1,
            )
            .with_plane("runtime_lifecycle")
            .with_record_key("maintain")
            .with_content_hash("test-runtime-lifecycle-content-hash")
            .with_payload("runtime_operation", "maintain")
            .with_payload("operation", operation)
            .with_payload("trigger", "sdk_call")
            .with_payload("disposition", "execute_now")
            .with_payload("effect", "run_maintenance")
            .with_payload("transaction_id", transaction_id),
        ),
    }
}

fn complete_evidence_graph_closure(
    owner: &GovernedEvidenceDocument,
) -> (Vec<StoreMutation>, Vec<StoreJsonPrecondition>) {
    let owner_ref = GovernedMemoryOwnerRef::new(
        GovernedMemoryOwnerPlane::EvidenceDocument,
        owner.document_id.clone(),
    );
    let (mut mutations, mut preconditions) = evidence_facet_cascade(None, owner);
    let graph_mutations = typed_graph_closure_for_owner(
        &owner.memory_space_id,
        &owner.mounted_subject_id,
        "node:evidence-owner",
        owner_ref,
        owner.owner_revision,
    );
    preconditions.extend(absent_json_preconditions(&StoreMutationBatch {
        transaction_id: "txn-seed-evidence-graph-preconditions".to_string(),
        operation: "test.seed_evidence_graph_preconditions".to_string(),
        scope: StoreEventScope::system("test.seed_evidence_graph_preconditions"),
        mutations: graph_mutations.clone(),
    }));
    mutations.extend(graph_mutations);
    (mutations, preconditions)
}

fn commit_complete_evidence_graph_closure(
    platform: &StorePlatform,
    owner: &GovernedEvidenceDocument,
) {
    let transaction_id = "txn-seed-evidence-owner-graph";
    let operation = "write.governed_evidence_documents";
    let (mut mutations, preconditions) = complete_evidence_graph_closure(owner);
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-seed-evidence-owner-graph",
        transaction_id,
        operation,
    ));
    platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: transaction_id.to_string(),
                operation: operation.to_string(),
                scope: StoreEventScope::system(operation),
                mutations,
            },
            &preconditions,
        )
        .expect("commit evidence owner with complete facet and graph cascades");
}

fn typed_graph_closure() -> Vec<StoreMutation> {
    typed_graph_closure_for("system", "system", "owner:graph")
}

fn assert_graph_batch_rejected_without_partial_state(
    transaction_id: &str,
    mutations: Vec<StoreMutation>,
) {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let platform = support::open_store(config).expect("platform");
    let owner = graph_owner();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot
        .json_docs
        .extend(typed_long_term_scope_docs("system", "system", vec![owner]));
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
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(transaction_budget(2))
    .expect("transaction budget must be a valid semantic contraction");
    let platform = support::open_store(config).expect("platform");
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
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(transaction_budget(8))
    .expect("transaction budget must be a valid semantic contraction");
    let platform = support::open_store(config).expect("platform");

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
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(transaction_budget(16))
    .expect("transaction budget must be a valid semantic contraction");
    let platform = support::open_store(config).expect("platform");

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
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let platform = support::open_store(config).expect("platform");
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot.json_docs.extend(typed_long_term_scope_docs(
        "system",
        "system",
        vec![graph_owner_for("owner:graph")],
    ));
    snapshot.json_docs.extend(typed_long_term_scope_docs(
        "space:b",
        "subject:b",
        vec![graph_owner_for("owner:graph:b")],
    ));
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

    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let platform = support::open_store(config).expect("platform");
    let owner = graph_owner();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot
        .json_docs
        .extend(typed_long_term_scope_docs("system", "system", vec![owner]));
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
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let platform = support::open_store(config).expect("platform");
    let owner = graph_owner();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot
        .json_docs
        .extend(typed_long_term_scope_docs("system", "system", vec![owner]));
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
    platform
        .tamper_json_document_for_nonproduction_harness(
            MEMORY_GRAPH_NODE_NAMESPACE,
            &orphan_key,
            serde_json::to_value(orphan).expect("serialize orphan"),
        )
        .expect("inject scoped orphan");
    let before = raw_graph_docs(&platform);

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
    let preconditions = exact_json_preconditions(&before, &replacement_batch);
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(replacement_batch, &preconditions)
        .expect_err("orphan delete is not part of the manifest exact effects");

    assert_eq!(error.stage(), "memory_write_transaction_commit_failed");
    assert!(error
        .to_string()
        .contains("memory_write_transaction_graph_post_image_invalid"));
    assert_eq!(raw_graph_docs(&platform), before);
}

#[test]
fn typed_graph_delete_rejects_a_noncanonical_before_dependency_closure() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let platform = support::open_store(config).expect("platform");
    let owner = graph_owner();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot
        .json_docs
        .extend(typed_long_term_scope_docs("system", "system", vec![owner]));
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
    let mut corrupted = raw_graph_docs(&platform);
    let membership = corrupted
        .iter_mut()
        .find(|doc| doc.namespace == MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE)
        .expect("node membership");
    let old_membership_key = membership.key.clone();
    membership.key = forged_key.clone();
    membership.value["membership_key"] = json!(forged_key.clone());
    let manifest = corrupted
        .iter_mut()
        .find(|doc| doc.namespace == MEMORY_GRAPH_MANIFEST_NAMESPACE)
        .expect("manifest");
    manifest.value["node_memberships"][0]["storage_key"] = json!(forged_key);
    let forged_membership = corrupted
        .iter()
        .find(|doc| {
            doc.namespace == MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE && doc.key != old_membership_key
        })
        .expect("forged membership")
        .clone();
    let forged_manifest = corrupted
        .iter()
        .find(|doc| doc.namespace == MEMORY_GRAPH_MANIFEST_NAMESPACE)
        .expect("forged manifest")
        .clone();
    platform
        .delete_json_document_for_nonproduction_harness(
            MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE,
            &old_membership_key,
        )
        .expect("remove canonical membership");
    platform
        .tamper_json_document_for_nonproduction_harness(
            &forged_membership.namespace,
            &forged_membership.key,
            forged_membership.value,
        )
        .expect("inject forged membership");
    platform
        .tamper_json_document_for_nonproduction_harness(
            &forged_manifest.namespace,
            &forged_manifest.key,
            forged_manifest.value,
        )
        .expect("inject forged manifest");
    let before = raw_graph_docs(&platform);

    let delete_mutations = before
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
    let preconditions = exact_json_preconditions(&before, &delete_batch);
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(delete_batch, &preconditions)
        .expect_err("noncanonical before dependency closure must fail closed");

    assert_eq!(error.stage(), "memory_write_transaction_commit_failed");
    assert!(error
        .to_string()
        .contains("memory_write_transaction_graph_post_image_invalid"));
    assert_eq!(raw_graph_docs(&platform), before);
}

#[test]
fn raw_graph_batch_cannot_forge_integrity_repair_authority_with_operation_text() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    let platform = support::open_store(config).expect("platform");
    let owner = graph_owner();
    let owner_docs = typed_long_term_scope_docs("system", "system", vec![owner]);
    let owner_addresses = owner_docs
        .iter()
        .map(|doc| (doc.namespace.clone(), doc.key.clone()))
        .collect::<Vec<_>>();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot.json_docs.extend(owner_docs);
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

    for (namespace, key) in &owner_addresses {
        platform
            .delete_json_document_for_nonproduction_harness(namespace, key)
            .expect("remove governed owner closure");
    }
    let empty_root =
        LongTermMemoryVersionScopeManifest::build("system", "system", 2, &[], &[], &[], &[], 1)
            .expect("build empty typed long-term scope root");
    platform
        .tamper_json_document_for_nonproduction_harness(
            LONG_TERM_VERSION_SCOPE_MANIFEST_NAMESPACE,
            &empty_root.physical_key,
            serde_json::to_value(&empty_root).expect("serialize empty scope root"),
        )
        .expect("inject empty typed long-term scope root");
    let before = raw_graph_docs(&platform);
    let delete_mutations = before
        .iter()
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
    let preconditions = exact_json_preconditions(&before, &forged_repair_batch);
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(forged_repair_batch, &preconditions)
        .expect_err("operation text cannot grant graph repair authority");

    assert_eq!(error.stage(), "memory_write_transaction_commit_failed");
    assert!(error
        .to_string()
        .contains("memory_graph_before_image_invalid"));
    assert_eq!(raw_graph_docs(&platform), before);
}

#[test]
fn typed_graph_closure_rejects_a_preexisting_scoped_orphan_atomically() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    assert_scoped_graph_orphan_rejected(config, "in-memory");
}

#[test]
fn embedded_graph_closure_rejects_a_preexisting_scoped_orphan_atomically() {
    let config = StoreBackendConfig::embedded(ProfileId::EspEmbeddedSdk).expect("config");
    assert_scoped_graph_orphan_rejected(config, "embedded");
}

#[test]
fn file_graph_closure_rejects_a_preexisting_scoped_orphan_atomically() {
    let root = evidence_source_claim_race_root("file-graph-orphan");
    let config = StoreBackendConfig::file(
        &root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    assert_scoped_graph_orphan_rejected(config, "file");
    std::fs::remove_dir_all(root).expect("remove file graph orphan store");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn sqlite_graph_closure_rejects_a_preexisting_scoped_orphan_atomically() {
    let root = evidence_source_claim_race_root("sqlite-graph-orphan");
    let config = StoreBackendConfig::sqlite(
        root.join("memory.sqlite3"),
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config");
    assert_scoped_graph_orphan_rejected(config, "sqlite");
    std::fs::remove_dir_all(root).expect("remove sqlite graph orphan store");
}

fn assert_scoped_graph_orphan_rejected(config: StoreBackendConfig, backend: &str) {
    let platform = support::open_store(config).expect("platform");
    let owner = graph_owner();
    let mut snapshot = platform.export_store_snapshot().expect("seed snapshot");
    snapshot
        .json_docs
        .extend(typed_long_term_scope_docs("system", "system", vec![owner]));
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

    let orphan = MemoryGraphNode {
        node_id: "owner:orphan".to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: "Orphan graph node".to_string(),
        evidence_refs: vec!["evidence:orphan".to_string()],
    };
    let orphan_key =
        scoped_memory_graph_storage_key("system", "system", &format!("node:{}", orphan.node_id));
    platform
        .tamper_json_document_for_nonproduction_harness(
            MEMORY_GRAPH_NODE_NAMESPACE,
            &orphan_key,
            serde_json::to_value(orphan).expect("serialize orphan"),
        )
        .expect("inject scoped orphan");
    let before = raw_graph_docs(&platform);

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
    let preconditions = exact_json_preconditions(&before, &delete_batch);
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(delete_batch, &preconditions)
        .expect_err("scoped orphan must reject graph deletion");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_commit_failed",
        "backend={backend}"
    );
    assert!(error
        .to_string()
        .contains("memory_write_transaction_graph_post_image_invalid"));
    assert_eq!(raw_graph_docs(&platform), before, "backend={backend}");
    assert!(
        before
            .iter()
            .any(|doc| doc.namespace == MEMORY_GRAPH_NODE_NAMESPACE && doc.key == orphan_key),
        "backend={backend}"
    );
}

#[test]
fn graph_v2_namespace_admission_rejects_the_entire_closure_atomically() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(transaction_budget(5))
    .expect("transaction budget must be a valid semantic contraction");
    let platform = support::open_store(config).expect("platform");
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
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(transaction_budget(8))
    .expect("transaction budget must be a valid semantic contraction");
    let platform = support::open_store(config).expect("platform");

    let batch = StoreMutationBatch {
        transaction_id: "txn-read-namespace".to_string(),
        operation: "skill_meta.write".to_string(),
        scope: StoreEventScope::system("skill_meta.write"),
        mutations: vec![put_json("skill_meta", "order", json!(["release"]))],
    };
    let preconditions = absent_json_preconditions(&batch);
    platform
        .commit_governed_memory_transaction_with_preconditions(batch, &preconditions)
        .expect("summary batch commit");

    let docs = platform
        .read_json_namespace("skill_meta")
        .expect("read summary namespace");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].namespace, "skill_meta");
    assert_eq!(docs[0].key, "order");
    assert_eq!(docs[0].value, json!(["release"]));

    let err = platform
        .read_json_namespace("memory_graph_unowned_semantics")
        .expect_err("unsupported namespace must fail closed");
    assert_eq!(err.stage(), "store_json_namespace_read");
}

#[test]
fn memory_facet_index_rejects_untyped_owner_without_full_closure() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(transaction_budget(8))
    .expect("transaction budget must be a valid semantic contraction");
    let platform = support::open_store(config).expect("platform");

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
    assert_eq!(
        error.stage(),
        "memory_write_transaction_dependency_read_set"
    );

    let docs = platform
        .read_json_namespace("memory_facet_indexes")
        .expect("read facet index namespace");
    assert!(docs.is_empty());
}

#[test]
fn governed_transaction_rejects_owner_mutation_without_facet_closure() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner_docs = typed_long_term_scope_docs_without_facets(
        "system",
        "system",
        vec![graph_owner_for("owner-1")],
    );
    let mut mutations = owner_docs
        .into_iter()
        .map(|doc| put_json(&doc.namespace, &doc.key, doc.value))
        .collect::<Vec<_>>();
    mutations.extend(typed_graph_closure_for("system", "system", "owner-1"));
    let batch = StoreMutationBatch {
        transaction_id: "txn-owner-without-facet".to_string(),
        operation: "long_term.write".to_string(),
        scope: StoreEventScope::system("long_term.write"),
        mutations,
    };
    let preconditions = absent_json_preconditions(&batch);
    let before = platform.export_store_snapshot().expect("snapshot before");

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(batch, &preconditions)
        .expect_err("owner mutation without facet closure must fail");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_owner_facet_closure_missing"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn governed_transaction_rejects_evidence_owner_mutation_without_facet_cascade() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document("evidence-owner:missing-facet", 1);
    let source_ref =
        governed_evidence_source_ref_from_document(&owner).expect("valid evidence source ref");
    let source_claim_manifest = GovernedEvidenceSourceClaimManifest::build(
        &owner.memory_space_id,
        &owner.mounted_subject_id,
        [
            GovernedEvidenceOwnerClaimBinding::from_document_claim(&owner, &source_ref)
                .expect("valid evidence owner-claim binding"),
        ],
        256,
    )
    .expect("valid evidence source claim manifest");
    let batch = StoreMutationBatch {
        transaction_id: "txn-evidence-owner-without-facet".to_string(),
        operation: "governed_evidence_document.write".to_string(),
        scope: StoreEventScope::system("governed_evidence_document.write"),
        mutations: vec![
            StoreMutation::PutJson {
                namespace: GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                key: owner.physical_key.clone(),
                value: serde_json::to_value(&owner).expect("serialize evidence owner"),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                record_key: owner.document_id.clone(),
            },
            StoreMutation::PutJson {
                namespace: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                key: source_ref.physical_key.clone(),
                value: serde_json::to_value(&source_ref).expect("serialize evidence source ref"),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                record_key: owner.document_id.clone(),
            },
            StoreMutation::PutJson {
                namespace: GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
                key: source_claim_manifest.physical_key.clone(),
                value: serde_json::to_value(&source_claim_manifest)
                    .expect("serialize evidence source claim manifest"),
                event_kind: MemoryStoreEventKind::MemoryWrite,
                plane: GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
                record_key: source_claim_manifest.physical_key.clone(),
            },
        ],
    };

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(batch.clone(), &[])
        .expect_err("evidence owner namespace must require a read-set precondition");
    assert_eq!(
        error.stage(),
        "memory_write_transaction_precondition_missing"
    );

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            batch,
            &[
                StoreJsonPrecondition::Absent {
                    namespace: GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE.to_string(),
                    key: owner.physical_key,
                },
                StoreJsonPrecondition::Absent {
                    namespace: GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE.to_string(),
                    key: source_ref.physical_key,
                },
                StoreJsonPrecondition::Absent {
                    namespace: GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
                    key: source_claim_manifest.physical_key,
                },
            ],
        )
        .expect_err("evidence owner mutation without facet cascade must fail");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_owner_facet_closure_missing"
    );
    assert!(platform
        .read_json_namespace(GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE)
        .expect("evidence owner namespace")
        .is_empty());
}

#[test]
fn governed_transaction_rejects_evidence_owner_without_typed_source_ref_atomically() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document("evidence-owner:missing-source-ref", 1);
    let (mut mutations, mut preconditions) = evidence_facet_cascade(None, &owner);
    mutations.retain(|mutation| {
        !matches!(
            mutation,
            StoreMutation::PutJson { namespace, .. }
                if namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE
        )
    });
    preconditions.retain(|precondition| {
        !matches!(
            precondition,
            StoreJsonPrecondition::Absent { namespace, .. }
                if namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE
        )
    });
    let graph_mutations = typed_graph_closure_for_owner(
        &owner.memory_space_id,
        &owner.mounted_subject_id,
        "node:evidence-owner-missing-source-ref",
        GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::EvidenceDocument,
            owner.document_id.clone(),
        ),
        owner.owner_revision,
    );
    preconditions.extend(absent_json_preconditions(&StoreMutationBatch {
        transaction_id: "txn-missing-source-ref-graph-preconditions".to_string(),
        operation: "test.missing_source_ref_graph_preconditions".to_string(),
        scope: StoreEventScope::system("test.missing_source_ref_graph_preconditions"),
        mutations: graph_mutations.clone(),
    }));
    mutations.extend(graph_mutations);
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-evidence-owner-without-source-ref",
        "txn-evidence-owner-without-source-ref",
        "write.governed_evidence_documents",
    ));

    let before = platform.export_store_snapshot().expect("snapshot before");
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: "txn-evidence-owner-without-source-ref".to_string(),
                operation: "write.governed_evidence_documents".to_string(),
                scope: StoreEventScope::system("write.governed_evidence_documents"),
                mutations,
            },
            &preconditions,
        )
        .expect_err("evidence owner without typed source ref must fail");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_evidence_source_ref_closure_invalid"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn governed_transaction_rejects_forged_evidence_source_claim_atomically() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document("evidence-owner:forged-source-claim", 1);
    let transaction_id = "txn-forged-evidence-source-claim";
    let operation = "write.governed_evidence_documents";
    let (mut mutations, preconditions) = complete_evidence_graph_closure(&owner);
    for mutation in &mut mutations {
        if let StoreMutation::PutJson {
            namespace, value, ..
        } = mutation
        {
            if namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE {
                value["owner_ref"]["owner_id"] =
                    serde_json::json!("evidence-owner:forged-source-claim:other");
            }
        }
    }
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-forged-evidence-source-claim",
        transaction_id,
        operation,
    ));
    let before = platform.export_store_snapshot().expect("snapshot before");

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: transaction_id.to_string(),
                operation: operation.to_string(),
                scope: StoreEventScope::system(operation),
                mutations,
            },
            &preconditions,
        )
        .expect_err("forged source claim owner must fail closed");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_evidence_source_ref_post_image_invalid"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn governed_transaction_rejects_unknown_evidence_source_claim_fields_atomically() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document("evidence-owner:unknown-source-claim-field", 1);
    let transaction_id = "txn-unknown-evidence-source-claim-field";
    let operation = "write.governed_evidence_documents";
    let (mut mutations, preconditions) = complete_evidence_graph_closure(&owner);
    for mutation in &mut mutations {
        if let StoreMutation::PutJson {
            namespace, value, ..
        } = mutation
        {
            if namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE {
                value["legacy_owner_key"] = serde_json::json!(owner.physical_key);
            }
        }
    }
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-unknown-evidence-source-claim-field",
        transaction_id,
        operation,
    ));
    let before = platform.export_store_snapshot().expect("snapshot before");

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: transaction_id.to_string(),
                operation: operation.to_string(),
                scope: StoreEventScope::system(operation),
                mutations,
            },
            &preconditions,
        )
        .expect_err("unknown source claim fields must fail closed");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_dependency_read_set"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

fn assert_extra_evidence_effect_address_rejected(namespace: &str) {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document(&format!("evidence-owner:extra-address:{namespace}"), 1);
    let transaction_id = format!("txn-extra-evidence-address-{namespace}");
    let operation = "write.governed_evidence_documents";
    let (mut mutations, mut preconditions) = complete_evidence_graph_closure(&owner);
    let mut forged = mutations
        .iter()
        .find(|mutation| {
            matches!(mutation, StoreMutation::PutJson { namespace: actual, .. } if actual == namespace)
        })
        .cloned()
        .expect("typed evidence mutation");
    let forged_key = format!("forged:{namespace}");
    match &mut forged {
        StoreMutation::PutJson { key, .. } => *key = forged_key.clone(),
        _ => unreachable!("evidence fixture only creates JSON puts"),
    }
    mutations.push(forged);
    mutations.push(evidence_lifecycle_mutation(
        &format!("lifecycle-extra-evidence-address-{namespace}"),
        &transaction_id,
        operation,
    ));
    preconditions.push(StoreJsonPrecondition::Absent {
        namespace: namespace.to_string(),
        key: forged_key,
    });

    let before = platform.export_store_snapshot().expect("snapshot before");
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id,
                operation: operation.to_string(),
                scope: StoreEventScope::system(operation),
                mutations,
            },
            &preconditions,
        )
        .expect_err("extra noncanonical evidence effect address must fail closed");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_evidence_source_ref_closure_invalid"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn governed_transaction_rejects_extra_noncanonical_evidence_owner_address_atomically() {
    assert_extra_evidence_effect_address_rejected(GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE);
}

#[test]
fn governed_transaction_rejects_extra_noncanonical_evidence_source_ref_address_atomically() {
    assert_extra_evidence_effect_address_rejected(GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE);
}

#[test]
fn evidence_source_claim_json_excludes_raw_locator_and_uses_digest_metadata() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document("evidence-owner:claim-json-redaction", 1);
    commit_complete_evidence_graph_closure(&platform, &owner);

    let claims = platform
        .read_json_namespace(GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE)
        .expect("evidence source claims");
    assert_eq!(claims.len(), 1);
    let claim = &claims[0].value;
    assert!(claim.get("source_locator").is_none());
    let locator_digest = claim["source_locator_digest"]
        .as_str()
        .expect("source locator digest");
    assert_eq!(locator_digest.len(), 64);
    assert!(locator_digest.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert!(!claim.to_string().contains(&owner.source_locator));
}

#[test]
fn snapshot_import_rejects_legacy_evidence_source_ref_shape() {
    let source = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("source store");
    let target = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("target store");
    let owner = governed_evidence_document("evidence-owner:legacy-source-ref-import", 1);
    commit_complete_evidence_graph_closure(&source, &owner);
    let mut snapshot = source.export_store_snapshot().expect("snapshot");
    let source_ref = snapshot
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE)
        .expect("source ref doc");
    source_ref.value["schema_version"] = serde_json::json!(1);
    source_ref
        .value
        .as_object_mut()
        .expect("source ref object")
        .remove("source_locator_digest");
    let before = target.export_store_snapshot().expect("target before");

    let error = target
        .import_store_snapshot(&snapshot)
        .expect_err("legacy source ref shape must be rejected");

    assert_eq!(error.stage(), "store_snapshot_import");
    assert_eq!(target.export_store_snapshot().unwrap(), before);
}

#[test]
fn snapshot_import_rejects_unknown_evidence_source_ref_fields() {
    let source = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("source store");
    let target = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("target store");
    let owner = governed_evidence_document("evidence-owner:unknown-source-ref-import", 1);
    commit_complete_evidence_graph_closure(&source, &owner);
    let mut snapshot = source.export_store_snapshot().expect("snapshot");
    let source_ref = snapshot
        .json_docs
        .iter_mut()
        .find(|doc| doc.namespace == GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE)
        .expect("source ref doc");
    source_ref.value["legacy_owner_key"] = serde_json::json!(owner.physical_key);
    let before = target.export_store_snapshot().expect("target before");

    let error = target
        .import_store_snapshot(&snapshot)
        .expect_err("unknown source ref fields must be rejected");

    assert_eq!(error.stage(), "store_snapshot_import");
    assert_eq!(target.export_store_snapshot().unwrap(), before);
}

#[test]
fn backend_cas_rejects_second_document_claiming_existing_source_identity() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let first = governed_evidence_document("evidence-owner:source-cas:first", 1);
    let second = governed_evidence_document("evidence-owner:source-cas:second", 1);
    commit_complete_evidence_graph_closure(&platform, &first);
    let before = platform.export_store_snapshot().expect("snapshot before");
    let transaction_id = "txn-source-claim-cas-second-owner";
    let operation = "write.governed_evidence_documents";
    let (mut mutations, preconditions) = complete_evidence_graph_closure(&second);
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-source-claim-cas-second-owner",
        transaction_id,
        operation,
    ));

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: transaction_id.to_string(),
                operation: operation.to_string(),
                scope: StoreEventScope::system(operation),
                mutations,
            },
            &preconditions,
        )
        .expect_err("backend CAS must reject occupied source identity claim");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_precondition_failed"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn governed_transaction_rejects_evidence_owner_without_lifecycle_event_atomically() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document("evidence-owner:missing-lifecycle", 1);
    let (mutations, preconditions) = complete_evidence_graph_closure(&owner);
    let before = platform.export_store_snapshot().expect("snapshot before");

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: "txn-evidence-owner-without-lifecycle".to_string(),
                operation: "write.governed_evidence_documents".to_string(),
                scope: StoreEventScope::system("write.governed_evidence_documents"),
                mutations,
            },
            &preconditions,
        )
        .expect_err("evidence owner without lifecycle event must fail closed");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_evidence_lifecycle_closure_invalid"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn governed_transaction_rejects_wrongly_bound_evidence_lifecycle_event_atomically() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document("evidence-owner:wrong-lifecycle-binding", 1);
    let (mut mutations, preconditions) = complete_evidence_graph_closure(&owner);
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-wrong-evidence-owner-binding",
        "txn-forged-owner-binding",
        "write.governed_evidence_documents",
    ));
    let before = platform.export_store_snapshot().expect("snapshot before");

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: "txn-evidence-owner-wrong-lifecycle-binding".to_string(),
                operation: "write.governed_evidence_documents".to_string(),
                scope: StoreEventScope::system("write.governed_evidence_documents"),
                mutations,
            },
            &preconditions,
        )
        .expect_err("wrongly bound evidence lifecycle event must fail closed");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_evidence_lifecycle_closure_invalid"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn governed_transaction_rejects_extra_forged_evidence_lifecycle_event_atomically() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document("evidence-owner:extra-lifecycle", 1);
    let transaction_id = "txn-evidence-owner-extra-lifecycle";
    let operation = "write.governed_evidence_documents";
    let (mut mutations, preconditions) = complete_evidence_graph_closure(&owner);
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-evidence-owner-exact",
        transaction_id,
        operation,
    ));
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-evidence-owner-forged-extra",
        "txn-forged-extra-lifecycle",
        operation,
    ));
    let before = platform.export_store_snapshot().expect("snapshot before");

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: transaction_id.to_string(),
                operation: operation.to_string(),
                scope: StoreEventScope::system(operation),
                mutations,
            },
            &preconditions,
        )
        .expect_err("extra forged evidence lifecycle event must fail closed");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_evidence_lifecycle_closure_invalid"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn governed_transaction_accepts_complete_evidence_graph_closure() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document("evidence-owner:complete-graph", 1);

    commit_complete_evidence_graph_closure(&platform, &owner);

    let persisted = platform
        .read_json_namespace(GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE)
        .expect("evidence owner namespace");
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].value["document_id"], json!(owner.document_id));
    assert_eq!(
        platform
            .read_json_namespace(MEMORY_GRAPH_NODE_MEMBERSHIP_NAMESPACE)
            .expect("graph node memberships")
            .len(),
        1
    );
}

#[test]
fn governed_transaction_rejects_evidence_owner_creation_without_graph_closure() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let owner = governed_evidence_document("evidence-owner:create-without-graph", 1);
    let transaction_id = "txn-evidence-owner-create-without-graph";
    let operation = "write.governed_evidence_documents";
    let (mut mutations, preconditions) = evidence_facet_cascade(None, &owner);
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-evidence-owner-create-without-graph",
        transaction_id,
        operation,
    ));
    let before = platform.export_store_snapshot().expect("snapshot before");

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: transaction_id.to_string(),
                operation: operation.to_string(),
                scope: StoreEventScope::system(operation),
                mutations,
            },
            &preconditions,
        )
        .expect_err("evidence owner creation without graph closure must fail");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_graph_manifest_closure_missing"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn governed_transaction_rejects_unbound_evidence_owner_in_existing_graph() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let graph_owner = graph_owner();
    let mut seed_snapshot = platform.export_store_snapshot().expect("seed snapshot");
    seed_snapshot.json_docs.extend(typed_long_term_scope_docs(
        "system",
        "system",
        vec![graph_owner],
    ));
    platform
        .import_store_snapshot(&seed_snapshot)
        .expect("seed graph owner");
    let graph_batch = StoreMutationBatch {
        transaction_id: "txn-seed-existing-graph".to_string(),
        operation: "memory_graph.write".to_string(),
        scope: StoreEventScope::system("memory_graph.write"),
        mutations: typed_graph_closure(),
    };
    platform
        .commit_governed_memory_transaction_with_preconditions(
            graph_batch.clone(),
            &absent_json_preconditions(&graph_batch),
        )
        .expect("seed existing graph closure");

    let owner = governed_evidence_document("evidence-owner:unbound-existing-graph", 1);
    let transaction_id = "txn-unbound-evidence-owner-existing-graph";
    let operation = "write.governed_evidence_documents";
    let (mut mutations, preconditions) = evidence_facet_cascade(None, &owner);
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-unbound-evidence-owner-existing-graph",
        transaction_id,
        operation,
    ));
    let before = platform.export_store_snapshot().expect("snapshot before");

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: transaction_id.to_string(),
                operation: operation.to_string(),
                scope: StoreEventScope::system(operation),
                mutations,
            },
            &preconditions,
        )
        .expect_err("an evidence owner without graph membership must fail closed");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_graph_manifest_closure_missing"
    );
    assert_eq!(platform.export_store_snapshot().unwrap(), before);
}

#[test]
fn governed_transaction_rejects_evidence_owner_revision_without_graph_cascade() {
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let before = governed_evidence_document("evidence-owner:missing-graph", 1);
    commit_complete_evidence_graph_closure(&platform, &before);

    let after = governed_evidence_document("evidence-owner:missing-graph", 2);
    let transaction_id = "txn-evidence-owner-without-graph-cascade";
    let operation = "write.governed_evidence_documents";
    let (mut mutations, preconditions) = evidence_facet_cascade(Some(&before), &after);
    mutations.push(evidence_lifecycle_mutation(
        "lifecycle-evidence-owner-without-graph-cascade",
        transaction_id,
        operation,
    ));
    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            StoreMutationBatch {
                transaction_id: transaction_id.to_string(),
                operation: operation.to_string(),
                scope: StoreEventScope::system(operation),
                mutations,
            },
            &preconditions,
        )
        .expect_err("evidence owner revision without graph cascade must fail");

    assert_eq!(
        error.stage(),
        "memory_write_transaction_graph_manifest_closure_missing"
    );
    let persisted = platform
        .read_json_namespace(GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE)
        .expect("evidence owner namespace");
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].value["owner_revision"], json!(1));
}

#[test]
fn long_term_control_namespaces_require_read_set_preconditions() {
    let mut budget = transaction_budget(16);
    budget.logical_key_max_bytes = 256;
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(budget)
    .expect("transaction budget must be a valid semantic contraction");
    let platform = support::open_store(config).expect("platform");

    for (index, namespace) in [
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        LONG_TERM_GOVERNANCE_POLICY_NAMESPACE,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
    ]
    .into_iter()
    .enumerate()
    {
        let logical_key = format!("control-{index}");
        let value =
            match namespace {
                LONG_TERM_CONTROL_REVISION_NAMESPACE => serde_json::to_value(
                    typed_control_revision(&logical_key, "owner-precondition", "system"),
                )
                .expect("revision value"),
                LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE => {
                    serde_json::to_value(LongTermMemoryTombstone {
                        schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
                        tombstone_id: "tombstone-precondition".to_string(),
                        record_id: logical_key.clone(),
                        operation: LongTermControlOperation::Delete,
                        last_owner_revision: 1,
                        last_source_revision: Some(1),
                        previous_digest: "a".repeat(64),
                        reason: "precondition contract".to_string(),
                        owner_subject_id: "system".to_string(),
                        actor_subject_id: None,
                        memory_space_id: "system".to_string(),
                        created_at: 1,
                    })
                    .expect("tombstone value")
                }
                LONG_TERM_GOVERNANCE_POLICY_NAMESPACE => {
                    serde_json::to_value(MemoryLongTermGovernancePolicy {
                        schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
                        policy_revision: 1,
                        memory_space_id: "system".to_string(),
                        policy_id: logical_key.clone(),
                        kind: "suppress".to_string(),
                        selector: MemoryGovernanceSelector {
                            memory_space_id: Some("system".to_string()),
                            subject_id: Some("system".to_string()),
                            kind: None,
                            topic_pattern: None,
                            source_chat_id: None,
                            source_scope: None,
                        },
                        duration: Some(MemoryGovernanceSuppressionDuration::UntilManualResume),
                        expires_at: None,
                        reason: "precondition contract".to_string(),
                        created_at: 1,
                        updated_at: 1,
                    })
                    .expect("policy value")
                }
                LONG_TERM_CONTROL_AUDIT_NAMESPACE => {
                    serde_json::to_value(LongTermMemoryControlAuditEvent::new(
                        logical_key.clone(),
                        "txn-audit-precondition",
                        LongTermControlOperation::Correct,
                        Vec::new(),
                        "precondition contract",
                        "system".to_string(),
                        None,
                        "system",
                        1,
                    ))
                    .expect("audit value")
                }
                _ => unreachable!(),
            };
        let key = scoped_long_term_control_storage_key("system", namespace, &logical_key)
            .expect("canonical control key");
        let mutation = StoreMutation::PutJson {
            namespace: namespace.to_string(),
            key,
            value,
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: namespace.to_string(),
            record_key: logical_key,
        };
        let error = platform
            .commit_governed_memory_transaction(mutation_batch(
                &format!("txn-control-{index}"),
                mutation,
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
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let revision_id = "control-revision-without-audit";
    let key = scoped_long_term_control_storage_key(
        "system",
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        revision_id,
    )
    .expect("canonical revision key");
    let revision = typed_control_revision(revision_id, "owner-1", "system");
    let batch = StoreMutationBatch {
        transaction_id: "txn-control-without-audit".to_string(),
        operation: "test.control_audit_closure".to_string(),
        scope: StoreEventScope::system("test.control_audit_closure"),
        mutations: vec![StoreMutation::PutJson {
            namespace: LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
            key: key.clone(),
            value: serde_json::to_value(revision).expect("revision value"),
            event_kind: MemoryStoreEventKind::MemoryWrite,
            plane: LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
            record_key: revision_id.to_string(),
        }],
    };

    let error = platform
        .commit_governed_memory_transaction_with_preconditions(
            batch,
            &[StoreJsonPrecondition::Absent {
                namespace: LONG_TERM_CONTROL_REVISION_NAMESPACE.to_string(),
                key: key.clone(),
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
    let platform = support::open_store_in_memory(
        StoreBackendConfig::in_memory(
            ProfileId::native_dev_full().expect("native dev-full profile"),
        )
        .expect("config"),
    )
    .expect("store");
    let revision_id = "control-revision-bound-to-owner-1";
    let audit_id = "control-audit-bound-to-owner-2";
    let revision_key = scoped_long_term_control_storage_key(
        "system",
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        revision_id,
    )
    .expect("canonical revision key");
    let audit_key =
        scoped_long_term_control_storage_key("system", LONG_TERM_CONTROL_AUDIT_NAMESPACE, audit_id)
            .expect("canonical audit key");
    let revision = typed_control_revision(revision_id, "owner-1", "system");
    let other_revision = typed_control_revision("other-revision", "owner-2", "system");
    let audit = LongTermMemoryControlAuditEvent::new(
        audit_id,
        "txn-control-mismatched-audit",
        LongTermControlOperation::Correct,
        vec![ControlEffectRef::Revision {
            revision_id: "other-revision".to_string(),
            transition: other_revision.transition,
            mounted_subject_id: "system".to_string(),
        }],
        "test",
        "system".to_string(),
        None,
        "system",
        1,
    );
    let mut batch = StoreMutationBatch {
        transaction_id: "txn-control-mismatched-audit".to_string(),
        operation: "test.control_audit_binding".to_string(),
        scope: StoreEventScope::system("test.control_audit_binding"),
        mutations: vec![
            put_json(
                LONG_TERM_CONTROL_REVISION_NAMESPACE,
                &revision_key,
                serde_json::to_value(revision).expect("revision value"),
            ),
            put_json(
                LONG_TERM_CONTROL_AUDIT_NAMESPACE,
                &audit_key,
                serde_json::to_value(audit).expect("audit value"),
            ),
        ],
    };
    if let StoreMutation::PutJson { record_key, .. } = &mut batch.mutations[0] {
        *record_key = revision_id.to_string();
    }
    if let StoreMutation::PutJson { record_key, .. } = &mut batch.mutations[1] {
        *record_key = audit_id.to_string();
    }
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
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(transaction_budget(8))
    .expect("transaction budget must be a valid semantic contraction");
    let platform = support::open_store(config).expect("platform");
    let namespace = "skill_meta";
    let key = "order";
    let v1 = json!(["alpha"]);
    let first_v2 = json!(["beta-first"]);
    let second_v2 = json!(["beta-second"]);

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
        manifests[0].value == json!(["beta-first"]) || manifests[0].value == json!(["beta-second"])
    );
    assert_eq!(
        platform.read_events().unwrap().len(),
        events_before_race.len() + 1
    );
}

#[test]
fn conditional_batch_absent_precondition_rejects_existing_json_without_changes() {
    let config = StoreBackendConfig::in_memory(
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("config")
    .try_with_nonproduction_store_budget_limit(transaction_budget(8))
    .expect("transaction budget must be a valid semantic contraction");
    let platform = support::open_store(config).expect("platform");
    let namespace = "skill_meta";
    let key = "order";
    let preconditions = vec![StoreJsonPrecondition::Absent {
        namespace: namespace.to_string(),
        key: key.to_string(),
    }];

    platform
        .commit_governed_memory_transaction_with_preconditions(
            mutation_batch(
                "txn-absent-create",
                put_json(namespace, key, json!(["alpha"])),
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
                put_json(namespace, key, json!(["beta"])),
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

fn json_contains_owner_ref(value: &serde_json::Value, expected: &GovernedMemoryOwnerRef) -> bool {
    match value {
        serde_json::Value::Object(fields) => {
            let matches = fields
                .get("owner_plane")
                .and_then(serde_json::Value::as_str)
                == Some(expected.owner_plane.as_str())
                && fields.get("owner_id").and_then(serde_json::Value::as_str)
                    == Some(expected.owner_id.as_str());
            matches
                || fields
                    .values()
                    .any(|value| json_contains_owner_ref(value, expected))
        }
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| json_contains_owner_ref(value, expected)),
        _ => false,
    }
}

fn evidence_source_claim_race_plan(
    owner: &GovernedEvidenceDocument,
    transaction_id: &str,
) -> (StoreMutationBatch, Vec<StoreJsonPrecondition>) {
    let operation = "write.governed_evidence_documents";
    let (mut mutations, preconditions) = complete_evidence_graph_closure(owner);
    mutations.push(evidence_lifecycle_mutation(
        &format!("lifecycle-{transaction_id}"),
        transaction_id,
        operation,
    ));
    (
        StoreMutationBatch {
            transaction_id: transaction_id.to_string(),
            operation: operation.to_string(),
            scope: StoreEventScope::system(operation),
            mutations,
        },
        preconditions,
    )
}

fn assert_independent_store_platform_evidence_source_claim_race(
    config: StoreBackendConfig,
    cleanup_root: &std::path::Path,
    backend: &str,
) {
    let first_platform = support::open_store(config.clone()).expect("first independent platform");
    let second_platform = support::open_store(config.clone()).expect("second independent platform");
    let observer = support::open_store(config).expect("independent observer platform");
    let owners = [
        governed_evidence_document(&format!("evidence-owner:{backend}-source-race:first"), 1),
        governed_evidence_document(&format!("evidence-owner:{backend}-source-race:second"), 1),
    ];
    let owner_refs = owners
        .iter()
        .map(|owner| {
            GovernedMemoryOwnerRef::new(
                GovernedMemoryOwnerPlane::EvidenceDocument,
                owner.document_id.clone(),
            )
        })
        .collect::<Vec<_>>();
    let source_claim_keys = owners
        .iter()
        .map(|owner| {
            governed_evidence_source_ref_from_document(owner)
                .expect("valid source claim")
                .physical_key
        })
        .collect::<Vec<_>>();
    assert_eq!(
        source_claim_keys[0], source_claim_keys[1],
        "writers must compete for the same governed source claim"
    );
    let transaction_ids = [
        format!("txn-{backend}-evidence-source-race-first"),
        format!("txn-{backend}-evidence-source-race-second"),
    ];
    let first_plan = evidence_source_claim_race_plan(&owners[0], &transaction_ids[0]);
    let second_plan = evidence_source_claim_race_plan(&owners[1], &transaction_ids[1]);
    let barrier = Arc::new(Barrier::new(2));

    let first_barrier = Arc::clone(&barrier);
    let first = thread::spawn(move || {
        first_barrier.wait();
        first_platform
            .commit_governed_memory_transaction_with_preconditions(first_plan.0, &first_plan.1)
    });
    let second_barrier = Arc::clone(&barrier);
    let second = thread::spawn(move || {
        second_barrier.wait();
        second_platform
            .commit_governed_memory_transaction_with_preconditions(second_plan.0, &second_plan.1)
    });
    let outcomes = [
        first.join().expect("first evidence writer"),
        second.join().expect("second evidence writer"),
    ];

    let winner_index = outcomes
        .iter()
        .position(Result::is_ok)
        .expect("one evidence writer succeeds");
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
        1,
        "backend={backend}"
    );
    let loser = outcomes[1 - winner_index]
        .as_ref()
        .expect_err("the competing writer must lose CAS");
    assert_eq!(
        loser.stage(),
        "memory_write_transaction_precondition_failed",
        "backend={backend}: {loser}"
    );
    assert!(
        loser
            .to_string()
            .contains(GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE),
        "source-claim CAS must be the failing precondition, backend={backend}: {loser}"
    );

    let winner = &owners[winner_index];
    let winner_ref = &owner_refs[winner_index];
    let loser_ref = &owner_refs[1 - winner_index];
    let snapshot = observer
        .export_store_snapshot()
        .expect("final store snapshot");
    let docs_in = |namespace: &str| {
        snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == namespace)
            .collect::<Vec<_>>()
    };
    let owner_docs = docs_in(GOVERNED_EVIDENCE_DOCUMENT_NAMESPACE);
    let source_claims = docs_in(GOVERNED_EVIDENCE_SOURCE_REF_NAMESPACE);
    let facet_owners = docs_in(MEMORY_FACET_INDEX_NAMESPACE);
    let facet_postings = docs_in(MEMORY_FACET_POSTING_NAMESPACE);
    assert_eq!(owner_docs.len(), 1, "backend={backend}");
    assert_eq!(owner_docs[0].key, winner.physical_key, "backend={backend}");
    assert_eq!(
        owner_docs[0].value,
        serde_json::to_value(winner).expect("serialize expected winner"),
        "backend={backend}"
    );

    let expected_source_claim =
        governed_evidence_source_ref_from_document(winner).expect("expected source claim");
    assert_eq!(source_claims.len(), 1, "backend={backend}");
    assert_eq!(
        source_claims[0].key, expected_source_claim.physical_key,
        "backend={backend}"
    );
    assert_eq!(
        source_claims[0].value,
        serde_json::to_value(expected_source_claim).expect("serialize expected source claim"),
        "backend={backend}"
    );

    let (expected_facet_owner, expected_facet_postings, expected_facet_manifest) =
        evidence_facet_state(winner);
    let expected_facet_owner_key = scoped_memory_facet_owner_storage_key(
        &winner.memory_space_id,
        &winner.mounted_subject_id,
        &expected_facet_owner.owner_ref,
    )
    .expect("expected facet owner key");
    assert_eq!(facet_owners.len(), 1, "backend={backend}");
    assert_eq!(
        facet_owners[0].key, expected_facet_owner_key,
        "backend={backend}"
    );
    assert_eq!(
        facet_owners[0].value,
        serde_json::to_value(expected_facet_owner).expect("serialize expected facet owner"),
        "backend={backend}"
    );
    let expected_facet_manifest_key =
        memory_facet_manifest_key(&winner.memory_space_id, &winner.mounted_subject_id)
            .expect("expected facet manifest key");
    assert_eq!(
        facet_postings.len(),
        expected_facet_postings.len() + 1,
        "backend={backend}"
    );
    for expected_posting in &expected_facet_postings {
        let actual = facet_postings
            .iter()
            .find(|doc| doc.key == expected_posting.posting_key)
            .expect("expected facet posting");
        assert_eq!(
            actual.value,
            serde_json::to_value(expected_posting).expect("serialize expected facet posting"),
            "backend={backend}"
        );
    }
    let actual_manifest = facet_postings
        .iter()
        .find(|doc| doc.key == expected_facet_manifest_key)
        .expect("expected facet manifest");
    assert_eq!(
        actual_manifest.value,
        serde_json::to_value(expected_facet_manifest).expect("serialize expected facet manifest"),
        "backend={backend}"
    );

    let expected_graph = typed_graph_closure_for_owner(
        &winner.memory_space_id,
        &winner.mounted_subject_id,
        "node:evidence-owner",
        winner_ref.clone(),
        winner.owner_revision,
    );
    let graph_docs = snapshot
        .json_docs
        .iter()
        .filter(|doc| doc.namespace.starts_with("memory_graph_"))
        .collect::<Vec<_>>();
    assert_eq!(graph_docs.len(), expected_graph.len(), "backend={backend}");
    for mutation in expected_graph {
        let StoreMutation::PutJson {
            namespace,
            key,
            value,
            ..
        } = mutation
        else {
            panic!("expected graph JSON mutation")
        };
        let actual = graph_docs
            .iter()
            .find(|doc| doc.namespace == namespace && doc.key == key)
            .unwrap_or_else(|| panic!("missing graph closure {namespace}/{key}"));
        assert_eq!(actual.value, value, "backend={backend}");
    }
    assert!(
        snapshot
            .json_docs
            .iter()
            .all(|doc| !json_contains_owner_ref(&doc.value, loser_ref)),
        "loser left partial JSON state, backend={backend}"
    );

    let race_events = snapshot
        .events
        .iter()
        .filter(|event| {
            event
                .payload
                .get("transaction_id")
                .is_some_and(|id| transaction_ids.contains(id))
        })
        .collect::<Vec<_>>();
    assert!(!race_events.is_empty(), "backend={backend}");
    assert!(
        race_events.iter().all(|event| {
            event.payload.get("transaction_id") == Some(&transaction_ids[winner_index])
        }),
        "loser left transaction events, backend={backend}"
    );
    let lifecycle_events = race_events
        .iter()
        .filter(|event| {
            event.kind == MemoryStoreEventKind::RuntimeLifecycle
                && event.plane == "runtime_lifecycle"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        lifecycle_events.len(),
        1,
        "winner must have one lifecycle closure, backend={backend}"
    );

    drop(observer);
    std::fs::remove_dir_all(cleanup_root).expect("remove evidence race store");
}

fn evidence_source_claim_race_root(backend: &str) -> std::path::PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "beetle-memory-{backend}-evidence-source-race-{}-{suffix}",
        std::process::id()
    ))
}

#[test]
fn independent_file_store_platforms_allow_one_complete_evidence_source_claim_closure() {
    let root = evidence_source_claim_race_root("file");
    let config = StoreBackendConfig::file(
        &root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("file backend config");

    assert_independent_store_platform_evidence_source_claim_race(config, &root, "file");
}

#[cfg(feature = "sqlite-store")]
#[test]
fn independent_sqlite_store_platforms_allow_one_complete_evidence_source_claim_closure() {
    let root = evidence_source_claim_race_root("sqlite");
    let config = StoreBackendConfig::sqlite(
        root.join("memory.sqlite3"),
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("sqlite backend config");

    assert_independent_store_platform_evidence_source_claim_race(config, &root, "sqlite");
}
