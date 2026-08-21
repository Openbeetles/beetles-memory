use bm_core::memory::{
    build_governed_evidence_document_facet_index_doc,
    build_long_term_memory_facet_index_doc as try_build_long_term_memory_facet_index_doc,
    build_memory_graph_persistence_plan, canonical_recall_evidence_group,
    governed_evidence_document_content_digest, long_term_version_material_key,
    memory_facet_manifest_key, memory_graph_backlink_key, scoped_governed_evidence_document_key,
    scoped_long_term_control_storage_key, scoped_memory_facet_owner_storage_key,
    validate_long_term_control_post_image, validate_memory_facet_post_image,
    validate_memory_graph_post_image, ControlEffectRef, EvidenceBacklink, GovernedDocumentImage,
    GovernedEvidenceDocument, GovernedEvidenceDocumentChunk, GovernedEvidenceDocumentSourceKind,
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, GovernedOwnerRevisionRef,
    LongTermControlOperation, LongTermControlPostImageClosure, LongTermMemoryConfidence,
    LongTermMemoryControlAuditEvent, LongTermMemoryControlRevision,
    LongTermMemoryControlRevisionIntent, LongTermMemoryCorrectionEvidence,
    LongTermMemoryCorrectionLifecycle, LongTermMemoryEntry, LongTermMemoryFreshness,
    LongTermMemoryKind, LongTermMemorySourceScope, LongTermMemorySourceType,
    LongTermMemoryVersionMaterial, LongTermMemoryVersionMaterialImage, MemoryEvidenceAuthority,
    MemoryFacetIndexDoc, MemoryFacetIndexManifest, MemoryFacetOwnerVersion,
    MemoryFacetPostImageClosure, MemoryFacetPostingDoc, MemoryFacetPostingRevision,
    MemoryGraphEdge, MemoryGraphNode, MemoryGraphNodeKind, MemoryGraphOwnerBinding,
    MemoryGraphPostImageClosure, MemoryPrivacyClass, MemorySubjectVisibilityPolicy,
    LONG_TERM_CONTROL_AUDIT_NAMESPACE, LONG_TERM_CONTROL_REVISION_NAMESPACE,
    MEMORY_FACET_SCHEMA_VERSION,
};

const SPACE: &str = "space:main";
const SUBJECT: &str = "subject:user";

fn build_long_term_memory_facet_index_doc(
    entry: &LongTermMemoryEntry,
    memory_space_id: impl Into<String>,
    subject_ids: Vec<String>,
    facet_index_revision: u64,
) -> MemoryFacetIndexDoc {
    try_build_long_term_memory_facet_index_doc(
        entry,
        memory_space_id,
        subject_ids,
        facet_index_revision,
    )
    .expect("fixture long-term owner must produce a valid governed facet document")
}

fn owner_entry(id: &str, owner_revision: u64) -> LongTermMemoryEntry {
    LongTermMemoryEntry {
        id: id.to_string(),
        kind: LongTermMemoryKind::Project,
        topic: "agent-memory/p7/post-image".to_string(),
        content: "Schema owners validate complete post-image closure.".to_string(),
        keywords: vec!["post-image".to_string()],
        privacy: MemoryPrivacyClass::SharedWithSubject,
        source_chat_id: Some("chat:p7".to_string()),
        source_type: LongTermMemorySourceType::Conversation,
        source_scope: LongTermMemorySourceScope::User,
        subject_visibility: bm_core::memory::MemorySubjectVisibilityPolicy::AllSubjects,
        provenance: bm_core::memory::LongTermMemoryProvenance::new(
            MemoryEvidenceAuthority::UserAsserted,
        ),
        confidence: LongTermMemoryConfidence::High,
        freshness: LongTermMemoryFreshness::Dynamic,
        stale_hint: Default::default(),
        supporting_citations: vec!["turn:p7".to_string()],
        canonical_entities: Vec::new(),
        evidence_count: 1,
        created_at: 10,
        updated_at: 10,
        observed_at: 10,
        last_confirmed_at: Some(10),
        source_revision: Some(owner_revision),
        owner_revision,
        last_used_at: 0,
    }
}

fn version_material(
    entry: &LongTermMemoryEntry,
    factual_owner_id: &str,
    valid_from: u64,
    predecessor: Option<GovernedOwnerRevisionRef>,
) -> LongTermMemoryVersionMaterial {
    LongTermMemoryVersionMaterial::from_current_projection(
        SPACE,
        factual_owner_id,
        entry,
        valid_from,
        predecessor,
        Vec::new(),
    )
    .expect("typed long-term material")
}

fn created_material_image(
    entry: &LongTermMemoryEntry,
    factual_owner_id: &str,
) -> LongTermMemoryVersionMaterialImage {
    let material = version_material(entry, factual_owner_id, entry.updated_at, None);
    let key = long_term_version_material_key(
        SPACE,
        factual_owner_id,
        &material.owner_ref,
        material.owner_revision,
    )
    .expect("material key");
    LongTermMemoryVersionMaterialImage::created(key, material)
}

fn deleted_material_image(
    image: LongTermMemoryVersionMaterialImage,
) -> LongTermMemoryVersionMaterialImage {
    LongTermMemoryVersionMaterialImage::deleted(
        image
            .after_physical_key
            .expect("created fixture material key"),
        image.after.expect("created fixture material"),
    )
}

fn facet_closure() -> MemoryFacetPostImageClosure {
    let owner = owner_entry("ltm:p7", 1);
    let facet = build_long_term_memory_facet_index_doc(&owner, SPACE, vec![SUBJECT.to_string()], 1);
    let owner_version = MemoryFacetOwnerVersion {
        owner_ref: facet.owner_ref.clone(),
        owner_revision: owner.owner_revision,
        facet_index_revision: facet.facet_index_revision,
    };
    let postings = facet
        .posting_keys_for_subject(SUBJECT)
        .expect("posting keys")
        .into_iter()
        .map(|posting_key| {
            let posting = MemoryFacetPostingDoc {
                schema_version: MEMORY_FACET_SCHEMA_VERSION,
                memory_space_id: SPACE.to_string(),
                subject_id: SUBJECT.to_string(),
                posting_key: posting_key.clone(),
                revision: 1,
                owner_versions: vec![owner_version.clone()],
            };
            GovernedDocumentImage::created(posting_key, posting)
        })
        .collect::<Vec<_>>();
    let manifest = MemoryFacetIndexManifest {
        schema_version: MEMORY_FACET_SCHEMA_VERSION,
        memory_space_id: SPACE.to_string(),
        subject_id: SUBJECT.to_string(),
        owner_doc_count: 1,
        posting_doc_count: postings.len(),
        revision: 1,
        owner_versions: vec![owner_version],
        posting_revisions: postings
            .iter()
            .map(|posting| MemoryFacetPostingRevision {
                posting_key: posting.after.as_ref().expect("posting").posting_key.clone(),
                revision: posting.after.as_ref().expect("posting").revision,
            })
            .collect(),
    };

    MemoryFacetPostImageClosure {
        memory_space_id: SPACE.to_string(),
        long_term_owner_id: SPACE.to_string(),
        mounted_subject_id: SUBJECT.to_string(),
        long_term_owners: vec![created_material_image(&owner, SPACE)],
        evidence_document_owners: Vec::new(),
        facet_owners: vec![GovernedDocumentImage::created(
            scoped_memory_facet_owner_storage_key(SPACE, SUBJECT, &facet.owner_ref)
                .expect("facet owner key"),
            facet,
        )],
        postings,
        manifest: GovernedDocumentImage::created(
            memory_facet_manifest_key(SPACE, SUBJECT).expect("manifest key"),
            manifest,
        ),
    }
}

#[test]
fn facet_post_image_rejects_scoped_key_and_exact_posting_closure_drift() {
    let closure = facet_closure();
    let valid = validate_memory_facet_post_image(&closure);
    assert!(valid.accepted, "{:?}", valid.failures);

    let mut wrong_key = closure.clone();
    wrong_key.long_term_owners[0].after_physical_key = Some("owner:forged".to_string());
    let invalid = validate_memory_facet_post_image(&wrong_key);
    assert!(invalid
        .failures
        .contains(&"memory_facet_owner_physical_key_drift".to_string()));

    let mut missing_posting = closure;
    missing_posting.postings.pop();
    let invalid = validate_memory_facet_post_image(&missing_posting);
    assert!(invalid
        .failures
        .contains(&"memory_facet_posting_exact_closure_drift".to_string()));

    let mut stale_revision = facet_closure();
    stale_revision
        .manifest
        .after
        .as_mut()
        .expect("manifest")
        .revision = 2;
    let invalid = validate_memory_facet_post_image(&stale_revision);
    assert!(invalid
        .failures
        .contains(&"memory_facet_manifest_revision_successor_drift".to_string()));
}

fn deleted_image<T>(image: GovernedDocumentImage<T>) -> GovernedDocumentImage<T> {
    GovernedDocumentImage::deleted(
        image.physical_key,
        image.after.expect("created fixture post-image"),
    )
}

#[test]
fn facet_post_image_accepts_complete_last_owner_scope_deletion() {
    let created = facet_closure();
    let deleted = MemoryFacetPostImageClosure {
        memory_space_id: created.memory_space_id,
        long_term_owner_id: created.long_term_owner_id,
        mounted_subject_id: created.mounted_subject_id,
        long_term_owners: created
            .long_term_owners
            .into_iter()
            .map(deleted_material_image)
            .collect(),
        evidence_document_owners: created
            .evidence_document_owners
            .into_iter()
            .map(deleted_image)
            .collect(),
        facet_owners: created
            .facet_owners
            .into_iter()
            .map(deleted_image)
            .collect(),
        postings: created.postings.into_iter().map(deleted_image).collect(),
        manifest: deleted_image(created.manifest),
    };

    let validation = validate_memory_facet_post_image(&deleted);
    assert!(validation.accepted, "{:?}", validation.failures);
}

fn evidence_owner_entry(id: &str, owner_revision: u64) -> GovernedEvidenceDocument {
    let source_locator = "opaque://release/import-only".to_string();
    let canonical_evidence_group = canonical_recall_evidence_group("evidence:p7.4.1:release");
    let body = "Schema owners validate complete post-image closure.".to_string();
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "section:facet".to_string(),
        ordinal: 0,
        body: "typed owner posting manifest".to_string(),
    }];
    GovernedEvidenceDocument {
        schema_version: bm_core::memory::GOVERNED_EVIDENCE_DOCUMENT_SCHEMA_VERSION,
        physical_key: scoped_governed_evidence_document_key(SPACE, id).expect("evidence key"),
        memory_space_id: SPACE.to_string(),
        mounted_subject_id: SUBJECT.to_string(),
        document_id: id.to_string(),
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
        updated_at: 10,
    }
}

fn mixed_plane_facet_closure() -> MemoryFacetPostImageClosure {
    let shared_id = "owner:shared-id";
    let long_term_owner = owner_entry(shared_id, 1);
    let evidence_owner = evidence_owner_entry(shared_id, 1);
    let long_term_facet = build_long_term_memory_facet_index_doc(
        &long_term_owner,
        SPACE,
        vec![SUBJECT.to_string()],
        1,
    );
    let evidence_facet = build_governed_evidence_document_facet_index_doc(
        &evidence_owner,
        vec![SUBJECT.to_string()],
        1,
    )
    .expect("valid evidence facet owner");
    let facet_docs: Vec<MemoryFacetIndexDoc> = vec![long_term_facet, evidence_facet];
    let mut posting_owners =
        std::collections::BTreeMap::<String, Vec<MemoryFacetOwnerVersion>>::new();
    for facet in &facet_docs {
        let owner_version = MemoryFacetOwnerVersion {
            owner_ref: facet.owner_ref.clone(),
            owner_revision: facet.owner_revision,
            facet_index_revision: facet.facet_index_revision,
        };
        for posting_key in facet
            .posting_keys_for_subject(SUBJECT)
            .expect("posting keys")
        {
            posting_owners
                .entry(posting_key)
                .or_default()
                .push(owner_version.clone());
        }
    }
    let postings = posting_owners
        .into_iter()
        .map(|(posting_key, mut owner_versions)| {
            owner_versions.sort();
            let posting = MemoryFacetPostingDoc {
                schema_version: MEMORY_FACET_SCHEMA_VERSION,
                memory_space_id: SPACE.to_string(),
                subject_id: SUBJECT.to_string(),
                posting_key: posting_key.clone(),
                revision: 1,
                owner_versions,
            };
            GovernedDocumentImage::created(posting_key, posting)
        })
        .collect::<Vec<_>>();
    let mut owner_versions = facet_docs
        .iter()
        .map(|facet| MemoryFacetOwnerVersion {
            owner_ref: facet.owner_ref.clone(),
            owner_revision: facet.owner_revision,
            facet_index_revision: facet.facet_index_revision,
        })
        .collect::<Vec<_>>();
    owner_versions.sort();
    let manifest = MemoryFacetIndexManifest {
        schema_version: MEMORY_FACET_SCHEMA_VERSION,
        memory_space_id: SPACE.to_string(),
        subject_id: SUBJECT.to_string(),
        owner_doc_count: owner_versions.len(),
        posting_doc_count: postings.len(),
        revision: 1,
        owner_versions,
        posting_revisions: postings
            .iter()
            .map(|posting| MemoryFacetPostingRevision {
                posting_key: posting.after.as_ref().expect("posting").posting_key.clone(),
                revision: posting.after.as_ref().expect("posting").revision,
            })
            .collect(),
    };
    let facet_owners = facet_docs
        .into_iter()
        .map(|facet| {
            GovernedDocumentImage::created(
                scoped_memory_facet_owner_storage_key(SPACE, SUBJECT, &facet.owner_ref)
                    .expect("facet owner key"),
                facet,
            )
        })
        .collect();

    MemoryFacetPostImageClosure {
        memory_space_id: SPACE.to_string(),
        long_term_owner_id: SPACE.to_string(),
        mounted_subject_id: SUBJECT.to_string(),
        long_term_owners: vec![created_material_image(&long_term_owner, SPACE)],
        evidence_document_owners: vec![GovernedDocumentImage::created(
            scoped_governed_evidence_document_key(SPACE, shared_id).expect("evidence key"),
            evidence_owner,
        )],
        facet_owners,
        postings,
        manifest: GovernedDocumentImage::created(
            memory_facet_manifest_key(SPACE, SUBJECT).expect("manifest key"),
            manifest,
        ),
    }
}

#[test]
fn facet_post_image_supports_same_id_in_long_term_and_evidence_planes() {
    let closure = mixed_plane_facet_closure();
    let valid = validate_memory_facet_post_image(&closure);
    assert!(valid.accepted, "{:?}", valid.failures);
    assert_eq!(
        closure
            .manifest
            .after
            .as_ref()
            .expect("manifest")
            .owner_versions
            .iter()
            .map(|owner| owner.owner_ref.owner_plane)
            .collect::<Vec<_>>(),
        vec![
            GovernedMemoryOwnerPlane::LongTerm,
            GovernedMemoryOwnerPlane::EvidenceDocument,
        ]
    );

    let mut wrong_key = closure.clone();
    wrong_key.evidence_document_owners[0].physical_key = "owner:forged".to_string();
    let invalid = validate_memory_facet_post_image(&wrong_key);
    assert!(invalid
        .failures
        .contains(&"memory_facet_owner_physical_key_drift".to_string()));

    let mut wrong_revision = closure.clone();
    wrong_revision.evidence_document_owners[0]
        .after
        .as_mut()
        .expect("evidence owner")
        .owner_revision += 1;
    let invalid = validate_memory_facet_post_image(&wrong_revision);
    assert!(invalid
        .failures
        .contains(&"memory_facet_owner_doc_owner_binding_drift".to_string()));

    let mut wrong_privacy = closure;
    wrong_privacy.evidence_document_owners[0]
        .after
        .as_mut()
        .expect("evidence owner")
        .privacy = MemoryPrivacyClass::PrivateGarden;
    let invalid = validate_memory_facet_post_image(&wrong_privacy);
    assert!(invalid
        .failures
        .contains(&"memory_facet_owner_doc_owner_binding_drift".to_string()));
}

fn graph_closure_for_owner(
    generation: u64,
    node_id: &str,
    owner_ref: GovernedMemoryOwnerRef,
    owner_revision: u64,
    long_term_owners: Vec<LongTermMemoryVersionMaterialImage>,
    evidence_document_owners: Vec<GovernedDocumentImage<GovernedEvidenceDocument>>,
) -> MemoryGraphPostImageClosure {
    let evidence_ref = format!("evidence:{node_id}");
    let nodes = vec![MemoryGraphNode {
        node_id: node_id.to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: "P7 closure".to_string(),
        evidence_refs: vec![evidence_ref.clone()],
    }];
    let edges: Vec<MemoryGraphEdge> = Vec::new();
    let backlinks = vec![EvidenceBacklink {
        source_kind: "long_term_memory".to_string(),
        source_id: evidence_ref,
        fingerprint: "fp:p7".to_string(),
    }];
    let plan = build_memory_graph_persistence_plan(
        SPACE,
        SUBJECT,
        generation,
        nodes.clone(),
        edges.clone(),
        backlinks.clone(),
        vec![MemoryGraphOwnerBinding {
            node_id: node_id.to_string(),
            owner_ref,
            owner_revision,
            visible: true,
        }],
    );
    assert!(plan.accepted, "{:?}", plan.failures);

    MemoryGraphPostImageClosure {
        memory_space_id: SPACE.to_string(),
        long_term_owner_id: SPACE.to_string(),
        mounted_subject_id: SUBJECT.to_string(),
        allow_missing_before_owners: false,
        validate_transition_successors: true,
        long_term_owners,
        evidence_document_owners,
        manifest: GovernedDocumentImage::created(
            bm_core::memory::memory_graph_scope_manifest_key(SPACE, SUBJECT),
            plan.scope_manifest.expect("manifest"),
        ),
        revision: GovernedDocumentImage::created(
            plan.revision
                .as_ref()
                .expect("revision")
                .revision_key
                .clone(),
            plan.revision.expect("revision"),
        ),
        node_memberships: plan
            .node_memberships
            .iter()
            .cloned()
            .map(|doc| GovernedDocumentImage::created(doc.membership_key.clone(), doc))
            .collect(),
        edge_memberships: plan
            .edge_memberships
            .iter()
            .cloned()
            .map(|doc| GovernedDocumentImage::created(doc.membership_key.clone(), doc))
            .collect(),
        backlink_memberships: plan
            .backlink_memberships
            .iter()
            .cloned()
            .map(|doc| GovernedDocumentImage::created(doc.membership_key.clone(), doc))
            .collect(),
        indexes: plan
            .recall_indexes
            .iter()
            .cloned()
            .map(|doc| GovernedDocumentImage::created(doc.index_key.clone(), doc))
            .collect(),
        nodes: nodes
            .into_iter()
            .map(|node| {
                let physical_key = plan
                    .node_memberships
                    .iter()
                    .find(|membership| membership.node_id == node.node_id)
                    .expect("node membership")
                    .document_key
                    .clone();
                GovernedDocumentImage::created(physical_key, node)
            })
            .collect(),
        edges: edges
            .into_iter()
            .map(|edge| {
                let physical_key = plan
                    .edge_memberships
                    .iter()
                    .find(|membership| membership.edge_id == edge.edge_id)
                    .expect("edge membership")
                    .document_key
                    .clone();
                GovernedDocumentImage::created(physical_key, edge)
            })
            .collect(),
        backlinks: backlinks
            .into_iter()
            .map(|backlink| {
                let key = memory_graph_backlink_key(&backlink.source_kind, &backlink.source_id);
                let physical_key = plan
                    .backlink_memberships
                    .iter()
                    .find(|membership| membership.backlink_key == key)
                    .expect("backlink membership")
                    .document_key
                    .clone();
                GovernedDocumentImage::created(physical_key, backlink)
            })
            .collect(),
    }
}

fn graph_closure(generation: u64) -> MemoryGraphPostImageClosure {
    graph_closure_for_owner(
        generation,
        "ltm:p7",
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "ltm:p7"),
        1,
        vec![created_material_image(&owner_entry("ltm:p7", 1), SPACE)],
        Vec::new(),
    )
}

fn evidence_graph_closure(generation: u64) -> MemoryGraphPostImageClosure {
    let owner = evidence_owner_entry("evidence-document:p7", 1);
    graph_closure_for_owner(
        generation,
        "node:evidence-document:p7",
        GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::EvidenceDocument,
            owner.document_id.clone(),
        ),
        owner.owner_revision,
        Vec::new(),
        vec![GovernedDocumentImage::created(
            owner.physical_key.clone(),
            owner,
        )],
    )
}

#[test]
fn graph_post_image_rejects_evidence_owner_without_node_membership() {
    let mut closure = graph_closure(1);
    let owner = evidence_owner_entry("evidence-document:unbound", 1);
    closure
        .evidence_document_owners
        .push(GovernedDocumentImage::created(
            owner.physical_key.clone(),
            owner,
        ));

    let invalid = validate_memory_graph_post_image(&closure);

    assert!(
        invalid
            .failures
            .contains(&"memory_graph_evidence_owner_membership_missing".to_string()),
        "{:?}",
        invalid.failures
    );
}

#[test]
fn graph_post_image_rejects_physical_key_and_exact_dependency_closure_drift() {
    let closure = graph_closure(1);
    let valid = validate_memory_graph_post_image(&closure);
    assert!(valid.accepted, "{:?}", valid.failures);

    let mut missing = closure.clone();
    missing.node_memberships.clear();
    let invalid = validate_memory_graph_post_image(&missing);
    assert!(
        invalid
            .failures
            .contains(&"memory_graph_node_membership_effect_closure_drift".to_string()),
        "{:?}",
        invalid.failures
    );

    let mut wrong_key = closure;
    wrong_key.nodes[0].physical_key = "node:forged".to_string();
    let invalid = validate_memory_graph_post_image(&wrong_key);
    assert!(invalid
        .failures
        .contains(&"memory_graph_node_document_physical_key_drift".to_string()));

    let mut reused_generation = graph_closure(2);
    let mut previous_manifest = reused_generation
        .manifest
        .after
        .as_ref()
        .expect("manifest")
        .clone();
    previous_manifest.graph_revision = "graph_revision:previous".to_string();
    reused_generation.manifest.before = Some(previous_manifest);
    let invalid = validate_memory_graph_post_image(&reused_generation);
    assert!(invalid
        .failures
        .contains(&"memory_graph_manifest_generation_successor_drift".to_string()));
}

#[test]
fn graph_post_image_accepts_complete_evidence_owner_closure() {
    let closure = evidence_graph_closure(1);
    let valid = validate_memory_graph_post_image(&closure);
    assert!(valid.accepted, "{:?}", valid.failures);
}

#[test]
fn graph_post_image_rejects_evidence_owner_physical_key_drift() {
    let mut closure = evidence_graph_closure(1);
    closure.evidence_document_owners[0].physical_key = "owner:forged".to_string();
    let invalid = validate_memory_graph_post_image(&closure);
    assert!(
        invalid
            .failures
            .contains(&"memory_graph_owner_physical_key_drift".to_string()),
        "{:?}",
        invalid.failures
    );
}

#[test]
fn graph_post_image_rejects_evidence_owner_revision_drift() {
    let mut closure = evidence_graph_closure(1);
    closure.evidence_document_owners[0]
        .after
        .as_mut()
        .expect("evidence owner")
        .owner_revision += 1;
    let invalid = validate_memory_graph_post_image(&closure);
    assert!(
        invalid
            .failures
            .contains(&"memory_graph_owner_revision_drift".to_string()),
        "{:?}",
        invalid.failures
    );
}

#[test]
fn graph_post_image_rejects_evidence_owner_revision_jump() {
    let mut closure = evidence_graph_closure(1);
    let owner_image = &mut closure.evidence_document_owners[0];
    let before = owner_image.after.as_ref().expect("evidence owner").clone();
    let mut after = before.clone();
    after.owner_revision = 3;
    after.source_revision = 3;
    after.updated_at += 1;
    owner_image.before = Some(before);
    owner_image.after = Some(after);
    let invalid = validate_memory_graph_post_image(&closure);
    assert!(
        invalid
            .failures
            .contains(&"memory_graph_owner_revision_successor_drift".to_string()),
        "{:?}",
        invalid.failures
    );
}

#[test]
fn graph_post_image_rejects_evidence_owner_privacy_drift() {
    let mut closure = evidence_graph_closure(1);
    closure.evidence_document_owners[0]
        .after
        .as_mut()
        .expect("evidence owner")
        .privacy = MemoryPrivacyClass::PrivateGarden;
    let invalid = validate_memory_graph_post_image(&closure);
    assert!(
        invalid
            .failures
            .contains(&"memory_graph_persistent_node_owner_not_visible".to_string()),
        "{:?}",
        invalid.failures
    );
}

#[test]
fn graph_post_image_rejects_long_term_owner_hidden_from_mounted_subject() {
    for policy in [
        MemorySubjectVisibilityPolicy::OnlySubjects(vec!["subject:other".to_string()]),
        MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec![SUBJECT.to_string()]),
    ] {
        let mut closure = graph_closure(1);
        closure.long_term_owners[0]
            .after
            .as_mut()
            .expect("long-term owner")
            .subject_visibility = policy;
        let material = closure.long_term_owners[0]
            .after
            .as_mut()
            .expect("long-term owner");
        material.content_digest = material
            .canonical_content_digest()
            .expect("canonical visibility digest");
        assert!(
            material.validate_contract().accepted,
            "hidden-owner fixture must remain a canonical material: {material:#?}"
        );
        let invalid = validate_memory_graph_post_image(&closure);
        assert!(
            invalid
                .failures
                .contains(&"memory_graph_persistent_node_owner_not_visible".to_string()),
            "{:?}",
            invalid.failures
        );
    }
}

#[test]
fn graph_post_image_accepts_complete_scope_deletion() {
    let created = graph_closure(1);
    let deleted = MemoryGraphPostImageClosure {
        memory_space_id: created.memory_space_id,
        long_term_owner_id: created.long_term_owner_id,
        mounted_subject_id: created.mounted_subject_id,
        allow_missing_before_owners: false,
        validate_transition_successors: true,
        long_term_owners: created
            .long_term_owners
            .into_iter()
            .map(deleted_material_image)
            .collect(),
        evidence_document_owners: created
            .evidence_document_owners
            .into_iter()
            .map(deleted_image)
            .collect(),
        manifest: deleted_image(created.manifest),
        revision: deleted_image(created.revision),
        node_memberships: created
            .node_memberships
            .into_iter()
            .map(deleted_image)
            .collect(),
        edge_memberships: created
            .edge_memberships
            .into_iter()
            .map(deleted_image)
            .collect(),
        backlink_memberships: created
            .backlink_memberships
            .into_iter()
            .map(deleted_image)
            .collect(),
        indexes: created.indexes.into_iter().map(deleted_image).collect(),
        nodes: created.nodes.into_iter().map(deleted_image).collect(),
        edges: created.edges.into_iter().map(deleted_image).collect(),
        backlinks: created.backlinks.into_iter().map(deleted_image).collect(),
    };

    let validation = validate_memory_graph_post_image(&deleted);
    assert!(validation.accepted, "{:?}", validation.failures);

    let mut incomplete = deleted;
    incomplete.node_memberships.clear();
    let validation = validate_memory_graph_post_image(&incomplete);
    assert!(!validation.accepted);
    assert!(validation
        .failures
        .contains(&"memory_graph_delete_exact_closure_drift".to_string()));
}

fn control_closure() -> LongTermControlPostImageClosure {
    let before_owner = owner_entry("ltm:p7", 1);
    let mut after_owner = before_owner.clone();
    after_owner.content = "Corrected post-image closure.".to_string();
    after_owner.owner_revision = 2;
    after_owner.source_revision = Some(2);
    after_owner.updated_at = 20;
    after_owner.observed_at = 20;
    let before_material = version_material(&before_owner, SPACE, 10, None);
    let mut after_material = version_material(
        &after_owner,
        SPACE,
        20,
        Some(before_material.owner_revision_ref()),
    );
    after_material.governed_content.correction_evidence = Some(
        LongTermMemoryCorrectionEvidence::try_new(
            SPACE,
            "governor:p7",
            before_material.owner_revision_ref(),
            after_material.owner_revision_ref(),
            LongTermMemoryCorrectionLifecycle::Correct,
            20,
            "revision:p7",
        )
        .expect("canonical correction evidence"),
    );
    after_material.content_digest = after_material
        .canonical_content_digest()
        .expect("correction material digest");
    let intent = LongTermMemoryControlRevisionIntent::for_owner_change(
        "revision:p7",
        LongTermControlOperation::Correct,
        &before_owner,
        Some(&after_owner),
        "corrected",
        SPACE.to_string(),
        Some("governor:p7".to_string()),
        SPACE,
        20,
        Vec::new(),
    )
    .expect("revision intent");
    let document =
        LongTermMemoryControlRevision::bind(intent, &before_material, Some(&after_material))
            .expect("bound revision");
    let revision = GovernedDocumentImage::created(
        scoped_long_term_control_storage_key(
            SPACE,
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
            &document.revision_id,
        )
        .expect("revision key"),
        document,
    );
    let audit = LongTermMemoryControlAuditEvent::new(
        "audit:p7",
        "tx:p7",
        LongTermControlOperation::Correct,
        vec![ControlEffectRef::Revision {
            revision_id: revision
                .after
                .as_ref()
                .expect("revision")
                .revision_id
                .clone(),
            transition: revision
                .after
                .as_ref()
                .expect("revision")
                .transition
                .clone(),
            factual_owner_id: SPACE.to_string(),
        }],
        "corrected",
        SPACE.to_string(),
        Some("governor:p7".to_string()),
        SPACE,
        20,
    );
    LongTermControlPostImageClosure {
        transaction_id: "tx:p7".to_string(),
        operation: LongTermControlOperation::Correct,
        memory_space_id: SPACE.to_string(),
        factual_owner_id: SPACE.to_string(),
        actor_subject_id: Some("governor:p7".to_string()),
        owner_records: vec![LongTermMemoryVersionMaterialImage::updated(
            long_term_version_material_key(
                SPACE,
                SPACE,
                &before_material.owner_ref,
                before_material.owner_revision,
            )
            .expect("before material key"),
            before_material,
            long_term_version_material_key(
                SPACE,
                SPACE,
                &after_material.owner_ref,
                after_material.owner_revision,
            )
            .expect("after material key"),
            after_material,
        )],
        revisions: vec![revision],
        tombstones: Vec::new(),
        policies: Vec::new(),
        audits: vec![GovernedDocumentImage::created(
            scoped_long_term_control_storage_key(
                SPACE,
                LONG_TERM_CONTROL_AUDIT_NAMESPACE,
                &audit.event_id,
            )
            .expect("audit key"),
            audit,
        )],
    }
}

#[test]
fn control_post_image_binds_typed_audit_effect_operation_version_and_physical_key() {
    let closure = control_closure();
    let valid = validate_long_term_control_post_image(&closure);
    assert!(valid.accepted, "{:?}", valid.failures);

    let mut wrong_operation = closure.clone();
    wrong_operation.audits[0]
        .after
        .as_mut()
        .expect("audit")
        .operation = LongTermControlOperation::Delete;
    let invalid = validate_long_term_control_post_image(&wrong_operation);
    assert!(invalid
        .failures
        .contains(&"long_term_control_audit_operation_drift".to_string()));

    let mut extra_effect = closure.clone();
    extra_effect.audits[0]
        .after
        .as_mut()
        .expect("audit")
        .effects
        .push(ControlEffectRef::Tombstone {
            tombstone_id: "tombstone:forged".to_string(),
            record_id: "ltm:forged".to_string(),
            factual_owner_id: SPACE.to_string(),
            owner_revision: 1,
            source_revision: Some(1),
        });
    let invalid = validate_long_term_control_post_image(&extra_effect);
    assert!(invalid
        .failures
        .contains(&"long_term_control_audit_effect_exact_closure_drift".to_string()));

    let mut wrong_key = closure;
    wrong_key.revisions[0].physical_key = "revision:forged".to_string();
    let invalid = validate_long_term_control_post_image(&wrong_key);
    assert!(invalid
        .failures
        .contains(&"long_term_control_revision_physical_key_drift".to_string()));

    let mut wrong_digest = control_closure();
    wrong_digest.revisions[0]
        .after
        .as_mut()
        .expect("revision")
        .successor_material_digest = Some("0".repeat(64));
    let invalid = validate_long_term_control_post_image(&wrong_digest);
    assert!(invalid
        .failures
        .contains(&"long_term_control_revision_owner_version_or_digest_drift".to_string()));

    let mut old_schema = control_closure();
    old_schema.audits[0]
        .after
        .as_mut()
        .expect("audit")
        .schema_version = 0;
    let invalid = validate_long_term_control_post_image(&old_schema);
    assert!(invalid
        .failures
        .contains(&"long_term_control_audit_schema_version_drift".to_string()));
}

#[test]
fn control_audit_is_append_only() {
    let mut closure = control_closure();
    let audit = closure.audits[0].after.take().expect("audit");
    let mut rewritten = audit.clone();
    rewritten.reason = "rewritten".to_string();
    closure.audits[0] =
        GovernedDocumentImage::updated(closure.audits[0].physical_key.clone(), audit, rewritten);

    let invalid = validate_long_term_control_post_image(&closure);
    assert!(invalid
        .failures
        .contains(&"long_term_control_audit_append_only_violation".to_string()));
}

#[test]
fn control_post_image_rejects_forged_owner_revision_jump() {
    let mut closure = control_closure();
    let owner_image = &mut closure.owner_records[0];
    let mut after_owner = owner_image.after.as_ref().expect("after owner").clone();
    after_owner.owner_revision = 9;
    after_owner.governed_content.source_revision = Some(9);
    after_owner.content_digest = after_owner
        .canonical_content_digest()
        .expect("forged owner digest");
    owner_image.after_physical_key = Some(
        long_term_version_material_key(
            SPACE,
            SPACE,
            &after_owner.owner_ref,
            after_owner.owner_revision,
        )
        .expect("forged owner key"),
    );
    owner_image.after = Some(after_owner.clone());

    let revision = closure.revisions[0].after.as_mut().expect("revision");
    revision.transition.successor = Some(after_owner.owner_revision_ref());
    revision.successor_material_digest = Some(after_owner.content_digest.clone());
    revision.content_digest = revision
        .canonical_content_digest()
        .expect("forged revision digest");
    let revision = revision.clone();
    closure.audits[0].after.as_mut().expect("audit").effects = vec![ControlEffectRef::Revision {
        revision_id: revision.revision_id,
        transition: revision.transition,
        factual_owner_id: SPACE.to_string(),
    }];

    let validation = validate_long_term_control_post_image(&closure);
    assert!(!validation.accepted);
    assert!(validation
        .failures
        .contains(&"long_term_control_owner_revision_successor_drift".to_string()));
}
