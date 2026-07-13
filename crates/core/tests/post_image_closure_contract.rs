use bm_core::memory::{
    build_long_term_memory_facet_index_doc, build_memory_graph_persistence_plan,
    memory_facet_manifest_key, memory_graph_backlink_key, scoped_long_term_control_storage_key,
    scoped_long_term_memory_storage_key, scoped_memory_facet_owner_storage_key,
    validate_long_term_control_post_image, validate_memory_facet_post_image,
    validate_memory_graph_post_image, ControlEffectRef, EvidenceBacklink, GovernedDocumentImage,
    LongTermControlOperation, LongTermControlPostImageClosure, LongTermMemoryConfidence,
    LongTermMemoryControlAuditEvent, LongTermMemoryControlRevision, LongTermMemoryEntry,
    LongTermMemoryFreshness, LongTermMemoryKind, LongTermMemorySourceScope,
    LongTermMemorySourceType, MemoryFacetIndexManifest, MemoryFacetOwnerVersion,
    MemoryFacetPostImageClosure, MemoryFacetPostingDoc, MemoryFacetPostingRevision,
    MemoryGraphEdge, MemoryGraphNode, MemoryGraphNodeKind, MemoryGraphOwnerBinding,
    MemoryGraphPostImageClosure, MemoryPrivacyClass, LONG_TERM_CONTROL_AUDIT_NAMESPACE,
    LONG_TERM_CONTROL_REVISION_NAMESPACE, MEMORY_FACET_SCHEMA_VERSION,
};

const SPACE: &str = "space:main";
const SUBJECT: &str = "subject:user";

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
        confidence: LongTermMemoryConfidence::High,
        freshness: LongTermMemoryFreshness::Dynamic,
        stale_hint: Default::default(),
        supporting_citations: vec!["turn:p7".to_string()],
        canonical_entities: Vec::new(),
        evidence_count: 1,
        created_at: 10,
        updated_at: 10,
        observed_at: 10,
        last_confirmed_at: 10,
        source_revision: Some(owner_revision),
        owner_revision,
        last_used_at: 0,
    }
}

fn facet_closure() -> MemoryFacetPostImageClosure {
    let owner = owner_entry("ltm:p7", 1);
    let facet = build_long_term_memory_facet_index_doc(&owner, SPACE, vec![SUBJECT.to_string()], 1);
    let owner_version = MemoryFacetOwnerVersion {
        owner_record_id: owner.id.clone(),
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
        mounted_subject_id: SUBJECT.to_string(),
        owner_records: vec![GovernedDocumentImage::created(
            scoped_long_term_memory_storage_key(SPACE, &owner.id).expect("owner key"),
            owner,
        )],
        facet_owners: vec![GovernedDocumentImage::created(
            scoped_memory_facet_owner_storage_key(SPACE, SUBJECT, &facet.owner_record_id)
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
    wrong_key.owner_records[0].physical_key = "owner:forged".to_string();
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
        mounted_subject_id: created.mounted_subject_id,
        owner_records: created
            .owner_records
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

fn graph_closure(generation: u64) -> MemoryGraphPostImageClosure {
    let nodes = vec![MemoryGraphNode {
        node_id: "ltm:p7".to_string(),
        kind: MemoryGraphNodeKind::MemoryRecord,
        label: "P7 closure".to_string(),
        evidence_refs: vec!["evidence:p7".to_string()],
    }];
    let edges: Vec<MemoryGraphEdge> = Vec::new();
    let backlinks = vec![EvidenceBacklink {
        source_kind: "long_term_memory".to_string(),
        source_id: "evidence:p7".to_string(),
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
            owner_record_id: "ltm:p7".to_string(),
            owner_revision: 1,
            visible: true,
        }],
    );
    assert!(plan.accepted, "{:?}", plan.failures);

    MemoryGraphPostImageClosure {
        memory_space_id: SPACE.to_string(),
        mounted_subject_id: SUBJECT.to_string(),
        allow_missing_before_owners: false,
        validate_transition_successors: true,
        owner_records: vec![GovernedDocumentImage::created(
            scoped_long_term_memory_storage_key(SPACE, "ltm:p7").expect("owner key"),
            owner_entry("ltm:p7", 1),
        )],
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
fn graph_post_image_accepts_complete_scope_deletion() {
    let created = graph_closure(1);
    let deleted = MemoryGraphPostImageClosure {
        memory_space_id: created.memory_space_id,
        mounted_subject_id: created.mounted_subject_id,
        allow_missing_before_owners: false,
        validate_transition_successors: true,
        owner_records: created
            .owner_records
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
    let document = LongTermMemoryControlRevision::for_owner_change(
        "revision:p7",
        LongTermControlOperation::Correct,
        &before_owner,
        &after_owner,
        "corrected",
        None,
        SPACE,
        20,
    );
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
            record_id: revision.after.as_ref().expect("revision").record_id.clone(),
            successor_record_id: None,
            owner_revision: revision.after.as_ref().expect("revision").owner_revision,
            source_revision: revision.after.as_ref().expect("revision").source_revision,
        }],
        "corrected",
        None,
        SPACE,
        20,
    );
    LongTermControlPostImageClosure {
        transaction_id: "tx:p7".to_string(),
        operation: LongTermControlOperation::Correct,
        memory_space_id: SPACE.to_string(),
        actor_subject_id: None,
        owner_records: vec![GovernedDocumentImage::updated(
            scoped_long_term_memory_storage_key(SPACE, &after_owner.id).expect("owner key"),
            before_owner,
            after_owner,
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
        .contains(&"long_term_control_audit_transaction_operation_scope_drift".to_string()));

    let mut extra_effect = closure.clone();
    extra_effect.audits[0]
        .after
        .as_mut()
        .expect("audit")
        .effects
        .push(ControlEffectRef::Tombstone {
            tombstone_id: "tombstone:forged".to_string(),
            record_id: "ltm:forged".to_string(),
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
        .new_digest = "forged".to_string();
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
    let before_owner = owner_image.before.as_ref().expect("before owner").clone();
    let mut after_owner = owner_image.after.as_ref().expect("after owner").clone();
    after_owner.owner_revision = 9;
    after_owner.source_revision = Some(9);
    owner_image.after = Some(after_owner.clone());

    let revision = LongTermMemoryControlRevision::for_owner_change(
        "revision:p7",
        LongTermControlOperation::Correct,
        &before_owner,
        &after_owner,
        "forged jump",
        None,
        SPACE,
        20,
    );
    closure.revisions[0].after = Some(revision.clone());
    closure.audits[0].after.as_mut().expect("audit").effects = vec![ControlEffectRef::Revision {
        revision_id: revision.revision_id,
        record_id: revision.record_id,
        successor_record_id: None,
        owner_revision: revision.owner_revision,
        source_revision: revision.source_revision,
    }];

    let validation = validate_long_term_control_post_image(&closure);
    assert!(!validation.accepted);
    assert!(validation
        .failures
        .contains(&"long_term_control_owner_revision_successor_drift".to_string()));
}
