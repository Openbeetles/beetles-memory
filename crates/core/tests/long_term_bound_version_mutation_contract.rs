use bm_core::memory::{
    bind_long_term_control_audit_batch, bind_long_term_version_creation,
    bind_long_term_version_mutation, BoundLongTermVersionRetention, ControlEffectRef,
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, GovernedOwnerRevisionRef,
    GovernedOwnerTermination, LongTermControlOperation, LongTermInvalidationReasonCode,
    LongTermMemoryConfidence, LongTermMemoryControlRevisionIntent, LongTermMemoryFreshness,
    LongTermMemoryGovernedContent, LongTermMemoryHeadManifest, LongTermMemoryKind,
    LongTermMemoryRetainedRevisionDigest, LongTermMemorySourceScope, LongTermMemorySourceType,
    LongTermMemoryStaleHint, LongTermMemoryVersionCreateIntent, LongTermMemoryVersionMaterial,
    LongTermMemoryVersionMutationIntent, LongTermMemoryVersionOrigin, LongTermVersionOwnerSnapshot,
    LongTermVersionRetentionLease, MemoryPrivacyClass, LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
};

const MEMORY_SPACE_ID: &str = "space-1";
const FACTUAL_OWNER_ID: &str = "space-1";
const OWNER_ID: &str = "state-1";

fn owner_ref(owner_id: &str) -> GovernedMemoryOwnerRef {
    GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner_id)
}

#[test]
fn bound_creation_owns_revision_one_material_head_and_effective_time() {
    let material = predecessor_material();
    let projection = material
        .to_current_projection()
        .expect("revision-one projection");
    let lease = LongTermVersionRetentionLease::try_new(2).expect("retention lease");
    let bound = bind_long_term_version_creation(
        LongTermMemoryVersionCreateIntent {
            memory_space_id: MEMORY_SPACE_ID.into(),
            factual_owner_id: FACTUAL_OWNER_ID.into(),
            projection,
            governed_evidence_refs: vec![evidence_ref()],
            requested_at: 5,
        },
        lease,
    )
    .expect("bound creation");

    assert_eq!(bound.effective_at, 20);
    assert_eq!(bound.material.owner_revision, 1);
    assert_eq!(bound.material.origin.valid_from, 20);
    assert_eq!(bound.head.current_revision, 1);
    assert_eq!(
        bound.head.retained_revision_digests[0].content_digest,
        bound.material.content_digest
    );
}

#[test]
fn bound_creation_rejects_a_non_space_factual_owner() {
    let projection = predecessor_material()
        .to_current_projection()
        .expect("revision-one projection");
    let error = bind_long_term_version_creation(
        LongTermMemoryVersionCreateIntent {
            memory_space_id: MEMORY_SPACE_ID.into(),
            factual_owner_id: "agent:alpha".into(),
            projection,
            governed_evidence_refs: vec![evidence_ref()],
            requested_at: 5,
        },
        LongTermVersionRetentionLease::try_new(2).expect("retention lease"),
    )
    .expect_err("shared factual owner must be the exact MemorySpace");

    assert_eq!(error.stage(), "long_term_version_creation");
}

fn evidence_ref() -> GovernedOwnerRevisionRef {
    GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::EvidenceDocument, "evidence-1"),
        1,
    )
    .expect("evidence revision ref")
}

fn predecessor_material() -> LongTermMemoryVersionMaterial {
    let mut material = LongTermMemoryVersionMaterial {
        schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
        memory_space_id: MEMORY_SPACE_ID.into(),
        factual_owner_id: FACTUAL_OWNER_ID.into(),
        owner_ref: owner_ref(OWNER_ID),
        owner_revision: 1,
        governed_content: LongTermMemoryGovernedContent {
            kind: LongTermMemoryKind::Fact,
            topic: "deployment".into(),
            content: "The active region is cn-east-2".into(),
            keywords: vec!["region".into()],
            source_chat_id: None,
            source_type: LongTermMemorySourceType::SystemRuntime,
            source_scope: LongTermMemorySourceScope::World,
            confidence: LongTermMemoryConfidence::High,
            freshness: LongTermMemoryFreshness::Dynamic,
            stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
            supporting_citations: Vec::new(),
            canonical_entities: Vec::new(),
            evidence_count: 1,
            created_at: 10,
            updated_at: 20,
            observed_at: 20,
            last_confirmed_at: 20,
            source_revision: Some(3),
            last_used_at: 0,
        },
        governed_evidence_refs: vec![evidence_ref()],
        origin: LongTermMemoryVersionOrigin {
            valid_from: 20,
            observed_at: 20,
            predecessor: None,
        },
        privacy_class: MemoryPrivacyClass::PublicRuntime,
        content_digest: String::new(),
    };
    material.content_digest = material
        .canonical_content_digest()
        .expect("predecessor digest");
    material
}

fn owner_snapshot(material: LongTermMemoryVersionMaterial) -> LongTermVersionOwnerSnapshot {
    LongTermVersionOwnerSnapshot {
        head: LongTermMemoryHeadManifest {
            schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
            memory_space_id: MEMORY_SPACE_ID.into(),
            factual_owner_id: FACTUAL_OWNER_ID.into(),
            owner_ref: material.owner_ref.clone(),
            current_revision: material.owner_revision,
            retained_revision_digests: vec![LongTermMemoryRetainedRevisionDigest {
                owner_revision: material.owner_revision,
                content_digest: material.content_digest.clone(),
            }],
            terminal_transition_ref: None,
            manifest_revision: 1,
        },
        retained_materials: vec![material],
        transitions: Vec::new(),
    }
}

fn advance_intent(
    predecessor: &LongTermMemoryVersionMaterial,
) -> LongTermMemoryVersionMutationIntent {
    let before = predecessor
        .to_current_projection()
        .expect("predecessor projection");
    let mut after = before.clone();
    after.content = "The active region is cn-east-3".into();
    after.owner_revision = 2;
    after.updated_at = 20;
    after.observed_at = 20;
    after.last_confirmed_at = 20;
    let control_revision_intent = LongTermMemoryControlRevisionIntent::for_owner_change(
        "revision-2",
        LongTermControlOperation::Correct,
        &before,
        Some(&after),
        "operator correction",
        FACTUAL_OWNER_ID.into(),
        Some("operator-1".into()),
        MEMORY_SPACE_ID,
        20,
        vec![evidence_ref()],
    )
    .expect("control revision intent");
    LongTermMemoryVersionMutationIntent {
        control_revision_intent,
        successor_projection: Some(after),
        audit_transaction_id: "pending".into(),
    }
}

fn terminal_intent(
    predecessor: &LongTermMemoryVersionMaterial,
    operation: LongTermControlOperation,
    governed_evidence_refs: Vec<GovernedOwnerRevisionRef>,
    actor_subject_id: Option<String>,
) -> LongTermMemoryVersionMutationIntent {
    let before = predecessor
        .to_current_projection()
        .expect("predecessor projection");
    let control_revision_intent = if operation == LongTermControlOperation::Invalidate {
        let mut intent = LongTermMemoryControlRevisionIntent::for_invalidation(
            format!("terminal-{}", operation.as_str()),
            &before,
            LongTermInvalidationReasonCode::ContradictedByGovernedEvidence,
            "operator terminal decision",
            FACTUAL_OWNER_ID.into(),
            "operator-1".into(),
            MEMORY_SPACE_ID,
            20,
            vec![evidence_ref()],
        )
        .expect("typed invalidation intent");
        intent.governed_evidence_refs = governed_evidence_refs;
        intent.actor_subject_id = actor_subject_id;
        intent
    } else {
        LongTermMemoryControlRevisionIntent::for_owner_change(
            format!("terminal-{}", operation.as_str()),
            operation,
            &before,
            None,
            "operator terminal decision",
            FACTUAL_OWNER_ID.into(),
            actor_subject_id,
            MEMORY_SPACE_ID,
            20,
            governed_evidence_refs,
        )
        .expect("terminal control revision intent")
    };
    LongTermMemoryVersionMutationIntent {
        control_revision_intent,
        successor_projection: None,
        audit_transaction_id: "pending".into(),
    }
}

#[test]
fn bound_advance_owns_monotonic_time_revision_audit_and_retention() {
    let predecessor = predecessor_material();
    let intent = advance_intent(&predecessor);
    let snapshot = owner_snapshot(predecessor.clone());
    let lease = LongTermVersionRetentionLease::try_new(2).expect("retention lease");
    let bound = bind_long_term_version_mutation(intent, &snapshot, lease).expect("bound advance");

    let successor = bound.successor_material.expect("successor material");
    assert_eq!(successor.origin.valid_from, 21);
    assert_eq!(bound.control_revision.transition.terminated_at, 21);
    assert_eq!(
        bound.control_revision.transition.successor,
        Some(successor.owner_revision_ref())
    );
    assert_eq!(
        bound.retention,
        BoundLongTermVersionRetention::AppendSuccessor
    );
    assert!(bound.tombstone.is_none());
    assert_eq!(
        bound.audit.effects,
        vec![ControlEffectRef::Revision {
            revision_id: bound.control_revision.revision_id.clone(),
            transition: bound.control_revision.transition.clone(),
            factual_owner_id: FACTUAL_OWNER_ID.into(),
        }]
    );
    assert_ne!(bound.audit.event_id, "pending");
}

#[test]
fn invalidate_retains_operator_only_closure_without_tombstone() {
    let predecessor = predecessor_material();
    let intent = terminal_intent(
        &predecessor,
        LongTermControlOperation::Invalidate,
        vec![evidence_ref()],
        Some("operator-1".into()),
    );
    let snapshot = owner_snapshot(predecessor.clone());
    let lease = LongTermVersionRetentionLease::try_new(2).expect("retention lease");
    let bound =
        bind_long_term_version_mutation(intent, &snapshot, lease).expect("bound invalidation");

    assert!(bound.successor_material.is_none());
    assert!(bound.tombstone.is_none());
    assert_eq!(
        bound.control_revision.transition.termination,
        GovernedOwnerTermination::Invalidated
    );
    assert_eq!(
        bound.retention,
        BoundLongTermVersionRetention::RetainOperatorOnly
    );
}

#[test]
fn invalidate_requires_governed_evidence_and_actor() {
    let predecessor = predecessor_material();
    let intent = terminal_intent(
        &predecessor,
        LongTermControlOperation::Invalidate,
        Vec::new(),
        None,
    );
    let snapshot = owner_snapshot(predecessor);
    let lease = LongTermVersionRetentionLease::try_new(2).expect("retention lease");
    let error = bind_long_term_version_mutation(intent, &snapshot, lease)
        .expect_err("ungoverned invalidation must fail");

    assert!(error.to_string().contains("invalidation"));
}

#[test]
fn delete_binds_typed_tombstone_to_material_digest_and_purge() {
    let predecessor = predecessor_material();
    let intent = terminal_intent(
        &predecessor,
        LongTermControlOperation::Delete,
        Vec::new(),
        Some("operator-1".into()),
    );
    let snapshot = owner_snapshot(predecessor.clone());
    let lease = LongTermVersionRetentionLease::try_new(2).expect("retention lease");
    let bound = bind_long_term_version_mutation(intent, &snapshot, lease).expect("bound delete");

    let tombstone = bound.tombstone.expect("typed tombstone");
    assert_eq!(tombstone.operation, LongTermControlOperation::Delete);
    assert_eq!(tombstone.previous_digest, predecessor.content_digest);
    assert_eq!(bound.retention, BoundLongTermVersionRetention::PurgeOwner);
    assert_eq!(bound.audit.effects.len(), 2);
    assert!(bound.audit.effects.iter().any(|effect| matches!(
        effect,
        ControlEffectRef::Tombstone { tombstone_id, .. }
            if tombstone_id == &tombstone.tombstone_id
    )));
}

#[test]
fn bound_advance_rejects_an_exhausted_request_pinned_retention_lease() {
    let predecessor = predecessor_material();
    let intent = advance_intent(&predecessor);
    let snapshot = owner_snapshot(predecessor);
    let lease = LongTermVersionRetentionLease::try_new(1).expect("retention lease");
    let error = bind_long_term_version_mutation(intent, &snapshot, lease)
        .expect_err("exhausted retention lease must fail");

    assert!(error.to_string().contains("retention"));
}

#[test]
fn core_batches_exact_bound_effects_into_one_canonical_transaction_audit() {
    let predecessor = predecessor_material();
    let intent = terminal_intent(
        &predecessor,
        LongTermControlOperation::Delete,
        Vec::new(),
        Some("operator-1".into()),
    );
    let snapshot = owner_snapshot(predecessor);
    let lease = LongTermVersionRetentionLease::try_new(2).expect("retention lease");
    let first =
        bind_long_term_version_mutation(intent, &snapshot, lease).expect("first bound delete");
    let mut second_audit = first.audit.clone();
    second_audit.effects.push(first.audit.effects[0].clone());
    second_audit
        .bind_canonical_event_id()
        .expect("second audit");

    let batch =
        bind_long_term_control_audit_batch("transaction-1", &[first.audit.clone(), second_audit])
            .expect("canonical audit batch");

    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].transaction_id, "transaction-1");
    assert_eq!(batch[0].effects, first.audit.effects);
    assert_ne!(batch[0].event_id, "pending");
}
