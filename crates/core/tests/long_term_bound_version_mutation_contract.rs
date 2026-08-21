use bm_core::memory::{
    bind_long_term_control_audit_batch, bind_long_term_version_creation,
    bind_long_term_version_mutation, long_term_version_material_key,
    scoped_long_term_control_storage_key, validate_long_term_control_post_image,
    BoundLongTermVersionRetention, ControlEffectRef, GovernedDocumentImage,
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, GovernedOwnerRevisionRef,
    GovernedOwnerTermination, LongTermControlOperation, LongTermControlPostImageClosure,
    LongTermInvalidationReasonCode, LongTermMemoryConfidence, LongTermMemoryControlRevisionIntent,
    LongTermMemoryFreshness, LongTermMemoryGovernedContent, LongTermMemoryHeadManifest,
    LongTermMemoryHumanConfirmationAuthority, LongTermMemoryKind, LongTermMemoryProvenance,
    LongTermMemoryRetainedRevisionDigest, LongTermMemorySourceScope, LongTermMemorySourceType,
    LongTermMemoryStaleHint, LongTermMemoryVersionCreateIntent, LongTermMemoryVersionMaterial,
    LongTermMemoryVersionMaterialImage, LongTermMemoryVersionMutationIntent,
    LongTermMemoryVersionOrigin, LongTermVersionOwnerSnapshot, LongTermVersionRetentionLease,
    MemoryEvidenceAuthority, MemoryPrivacyClass, MemorySemanticJudgmentSource,
    MemorySubjectVisibilityPolicy, SubjectDescriptor, SubjectKind, SubjectRegistry,
    SubjectVisibility, LONG_TERM_CONTROL_AUDIT_NAMESPACE, LONG_TERM_CONTROL_REVISION_NAMESPACE,
    LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE, LONG_TERM_CONTROL_TOMBSTONE_SCHEMA_VERSION,
    LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
};

const MEMORY_SPACE_ID: &str = "space-1";
const FACTUAL_OWNER_ID: &str = "space-1";
const OWNER_ID: &str = "state-1";

fn human_confirmation_authority(
    actor_subject_id: &str,
) -> LongTermMemoryHumanConfirmationAuthority {
    let mut registry = SubjectRegistry::empty(MEMORY_SPACE_ID);
    registry
        .upsert_subject(SubjectDescriptor::new(
            actor_subject_id,
            SubjectKind::HumanUser,
            "Contract Human",
            SubjectVisibility::Visible,
        ))
        .expect("register contract human");
    LongTermMemoryHumanConfirmationAuthority::try_from_registry(&registry, actor_subject_id)
        .expect("issue exact human confirmation authority")
}

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
            provenance: bm_core::memory::LongTermMemoryProvenance {
                source_authority: bm_core::memory::MemoryEvidenceAuthority::RuntimeObservation,
                semantic_judgment_source: Some(
                    bm_core::memory::MemorySemanticJudgmentSource::RuntimeGate,
                ),
            },
            confidence: LongTermMemoryConfidence::High,
            freshness: LongTermMemoryFreshness::Dynamic,
            stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
            supporting_citations: Vec::new(),
            canonical_entities: Vec::new(),
            evidence_count: 1,
            created_at: 10,
            updated_at: 20,
            observed_at: 20,
            correction_evidence: None,
            confirmation_evidence: None,
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
        subject_visibility: bm_core::memory::MemorySubjectVisibilityPolicy::AllSubjects,
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
    after.last_confirmed_at = None;
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
        human_confirmation_authority: None,
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
        human_confirmation_authority: None,
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
    assert!(
        successor.governed_content.confirmation_evidence.is_none(),
        "an arbitrary actor string must not mint human confirmation evidence"
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
fn bound_correct_adds_human_confirmation_only_for_the_exact_authorized_actor() {
    let predecessor = predecessor_material();
    let mut intent = advance_intent(&predecessor);
    intent.human_confirmation_authority = Some(human_confirmation_authority("operator-1"));
    let snapshot = owner_snapshot(predecessor.clone());
    let bound = bind_long_term_version_mutation(
        intent,
        &snapshot,
        LongTermVersionRetentionLease::try_new(2).expect("retention lease"),
    )
    .expect("bind authorized human confirmation");
    let successor = bound.successor_material.expect("successor material");
    let correction = successor
        .governed_content
        .correction_evidence
        .as_ref()
        .expect("neutral correction evidence");
    let confirmation = successor
        .governed_content
        .confirmation_evidence
        .as_ref()
        .expect("authorized human confirmation evidence");
    assert_eq!(confirmation.correction, *correction);
    assert_eq!(confirmation.confirmed_at, correction.corrected_at);
    assert_eq!(
        successor.governed_content.provenance, predecessor.governed_content.provenance,
        "confirmation must not rewrite source provenance"
    );

    let mut mismatched = advance_intent(&predecessor);
    mismatched.human_confirmation_authority = Some(human_confirmation_authority("different-human"));
    let error = bind_long_term_version_mutation(
        mismatched,
        &snapshot,
        LongTermVersionRetentionLease::try_new(2).expect("retention lease"),
    )
    .expect_err("confirmation actor must exactly match the Correct transition actor");
    assert_eq!(error.stage(), "long_term_human_confirmation_authority");
}

#[test]
fn bound_refresh_carries_confirmation_only_when_the_successor_projection_preserves_it() {
    let base = predecessor_material();
    let mut correction_intent = advance_intent(&base);
    correction_intent.human_confirmation_authority =
        Some(human_confirmation_authority("operator-1"));
    let confirmed = bind_long_term_version_mutation(
        correction_intent,
        &owner_snapshot(base),
        LongTermVersionRetentionLease::try_new(3).expect("retention lease"),
    )
    .expect("bind confirmed correction")
    .successor_material
    .expect("confirmed successor");
    let before = confirmed
        .to_current_projection()
        .expect("confirmed projection");
    let confirmed_at = before.last_confirmed_at.expect("confirmation time");

    let refresh_intent = |preserve_confirmation: bool| {
        let mut after = before.clone();
        after.owner_revision = before.owner_revision + 1;
        after.updated_at = before.updated_at + 1;
        after.observed_at = before.observed_at + 1;
        after.provenance = LongTermMemoryProvenance {
            source_authority: MemoryEvidenceAuthority::ModelInferred,
            semantic_judgment_source: Some(MemorySemanticJudgmentSource::RuntimeGate),
        };
        after.last_confirmed_at = preserve_confirmation.then_some(confirmed_at);
        LongTermMemoryVersionMutationIntent {
            control_revision_intent: LongTermMemoryControlRevisionIntent::for_owner_change(
                if preserve_confirmation {
                    "refresh-preserve-confirmation"
                } else {
                    "refresh-clear-confirmation"
                },
                LongTermControlOperation::Refresh,
                &before,
                Some(&after),
                "refresh same content with explicit provenance",
                FACTUAL_OWNER_ID.into(),
                Some("operator-1".into()),
                MEMORY_SPACE_ID,
                before.updated_at + 1,
                vec![evidence_ref()],
            )
            .expect("refresh control intent"),
            successor_projection: Some(after),
            human_confirmation_authority: None,
            audit_transaction_id: "pending".into(),
        }
    };

    let cleared = bind_long_term_version_mutation(
        refresh_intent(false),
        &owner_snapshot(confirmed.clone()),
        LongTermVersionRetentionLease::try_new(3).expect("retention lease"),
    )
    .expect("bind refresh that clears confirmation")
    .successor_material
    .expect("refresh successor");
    assert!(cleared.governed_content.confirmation_evidence.is_none());
    assert_eq!(
        cleared.to_current_projection().unwrap().last_confirmed_at,
        None
    );

    let preserved = bind_long_term_version_mutation(
        refresh_intent(true),
        &owner_snapshot(confirmed),
        LongTermVersionRetentionLease::try_new(3).expect("retention lease"),
    )
    .expect("bind refresh that explicitly preserves confirmation")
    .successor_material
    .expect("refresh successor");
    assert_eq!(
        preserved
            .governed_content
            .confirmation_evidence
            .as_ref()
            .map(|evidence| evidence.confirmed_at),
        Some(confirmed_at)
    );
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
    assert_eq!(
        tombstone.schema_version,
        LONG_TERM_CONTROL_TOMBSTONE_SCHEMA_VERSION
    );
    assert_eq!(tombstone.operation, LongTermControlOperation::Delete);
    assert_eq!(tombstone.previous_digest, predecessor.content_digest);
    assert_eq!(tombstone.subject_visibility, predecessor.subject_visibility);
    assert_eq!(bound.retention, BoundLongTermVersionRetention::PurgeOwner);
    assert_eq!(bound.audit.effects.len(), 2);
    assert!(bound.audit.effects.iter().any(|effect| matches!(
        effect,
        ControlEffectRef::Tombstone { tombstone_id, .. }
            if tombstone_id == &tombstone.tombstone_id
    )));
}

#[test]
fn control_post_image_rejects_terminal_visibility_drift_from_exact_predecessor() {
    let predecessor = predecessor_material();
    let intent = terminal_intent(
        &predecessor,
        LongTermControlOperation::Delete,
        Vec::new(),
        Some("operator-1".into()),
    );
    let snapshot = owner_snapshot(predecessor.clone());
    let bound = bind_long_term_version_mutation(
        intent,
        &snapshot,
        LongTermVersionRetentionLease::try_new(2).expect("retention lease"),
    )
    .expect("bound delete");
    let owner_key = long_term_version_material_key(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        &predecessor.owner_ref,
        predecessor.owner_revision,
    )
    .expect("owner key");
    let revision_key = scoped_long_term_control_storage_key(
        MEMORY_SPACE_ID,
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        &bound.control_revision.revision_id,
    )
    .expect("revision key");
    let tombstone = bound.tombstone.expect("typed tombstone");
    let tombstone_key = scoped_long_term_control_storage_key(
        MEMORY_SPACE_ID,
        LONG_TERM_CONTROL_TOMBSTONE_NAMESPACE,
        &tombstone.record_id,
    )
    .expect("tombstone key");
    let audit_key = scoped_long_term_control_storage_key(
        MEMORY_SPACE_ID,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        &bound.audit.event_id,
    )
    .expect("audit key");
    let closure = LongTermControlPostImageClosure {
        transaction_id: bound.audit.transaction_id.clone(),
        operation: LongTermControlOperation::Delete,
        memory_space_id: MEMORY_SPACE_ID.into(),
        factual_owner_id: FACTUAL_OWNER_ID.into(),
        actor_subject_id: Some("operator-1".into()),
        owner_records: vec![LongTermMemoryVersionMaterialImage::deleted(
            owner_key,
            predecessor,
        )],
        revisions: vec![GovernedDocumentImage::created(
            revision_key,
            bound.control_revision,
        )],
        tombstones: vec![GovernedDocumentImage::created(tombstone_key, tombstone)],
        policies: Vec::new(),
        audits: vec![GovernedDocumentImage::created(audit_key, bound.audit)],
    };
    let valid = validate_long_term_control_post_image(&closure);
    assert!(valid.accepted, "{:?}", valid.failures);

    let mut drifted = closure.clone();
    drifted.tombstones[0]
        .after
        .as_mut()
        .expect("tombstone")
        .subject_visibility =
        MemorySubjectVisibilityPolicy::OnlySubjects(vec!["operator-1".into()]);
    let invalid = validate_long_term_control_post_image(&drifted);
    assert!(invalid
        .failures
        .contains(&"long_term_control_tombstone_owner_version_or_digest_drift".to_string()));

    let mut old_schema = closure;
    old_schema.tombstones[0]
        .after
        .as_mut()
        .expect("tombstone")
        .schema_version = 3;
    let invalid = validate_long_term_control_post_image(&old_schema);
    assert!(invalid
        .failures
        .contains(&"long_term_control_tombstone_schema_version_drift".to_string()));
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

fn correct_post_image_closure() -> LongTermControlPostImageClosure {
    let predecessor = predecessor_material();
    let bound = bind_long_term_version_mutation(
        advance_intent(&predecessor),
        &owner_snapshot(predecessor.clone()),
        LongTermVersionRetentionLease::try_new(2).expect("retention lease"),
    )
    .expect("bound correction");
    let successor = bound.successor_material.expect("successor material");
    let predecessor_key = long_term_version_material_key(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        &predecessor.owner_ref,
        predecessor.owner_revision,
    )
    .expect("predecessor key");
    let successor_key = long_term_version_material_key(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        &successor.owner_ref,
        successor.owner_revision,
    )
    .expect("successor key");
    let revision_key = scoped_long_term_control_storage_key(
        MEMORY_SPACE_ID,
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        &bound.control_revision.revision_id,
    )
    .expect("revision key");
    let audit_key = scoped_long_term_control_storage_key(
        MEMORY_SPACE_ID,
        LONG_TERM_CONTROL_AUDIT_NAMESPACE,
        &bound.audit.event_id,
    )
    .expect("audit key");
    LongTermControlPostImageClosure {
        transaction_id: bound.audit.transaction_id.clone(),
        operation: LongTermControlOperation::Correct,
        memory_space_id: MEMORY_SPACE_ID.into(),
        factual_owner_id: FACTUAL_OWNER_ID.into(),
        actor_subject_id: Some("operator-1".into()),
        owner_records: vec![LongTermMemoryVersionMaterialImage::updated(
            predecessor_key,
            predecessor,
            successor_key,
            successor,
        )],
        revisions: vec![GovernedDocumentImage::created(
            revision_key,
            bound.control_revision,
        )],
        tombstones: Vec::new(),
        policies: Vec::new(),
        audits: vec![GovernedDocumentImage::created(audit_key, bound.audit)],
    }
}

fn refresh_correction_successor_digests(closure: &mut LongTermControlPostImageClosure) {
    let successor = closure.owner_records[0].after.as_mut().expect("successor");
    successor.content_digest = successor
        .canonical_content_digest()
        .expect("successor digest");
    let revision = closure.revisions[0].after.as_mut().expect("revision");
    revision.successor_material_digest = Some(successor.content_digest.clone());
    revision.content_digest = revision
        .canonical_content_digest()
        .expect("revision digest");
}

#[test]
fn correction_post_image_rejects_every_correction_binding_drift() {
    let closure = correct_post_image_closure();
    assert!(
        validate_long_term_control_post_image(&closure).accepted,
        "valid correction closure"
    );

    type EvidenceMutator = fn(&mut bm_core::memory::LongTermMemoryCorrectionEvidence);
    let cases: [(&str, EvidenceMutator); 6] = [
        (
            "space",
            |evidence: &mut bm_core::memory::LongTermMemoryCorrectionEvidence| {
                evidence.memory_space_id = "space:other".into();
            },
        ),
        (
            "actor",
            |evidence: &mut bm_core::memory::LongTermMemoryCorrectionEvidence| {
                evidence.actor_subject_id = "operator:other".into();
            },
        ),
        (
            "predecessor",
            |evidence: &mut bm_core::memory::LongTermMemoryCorrectionEvidence| {
                evidence.predecessor.owner_revision = 9;
            },
        ),
        (
            "successor",
            |evidence: &mut bm_core::memory::LongTermMemoryCorrectionEvidence| {
                evidence.successor.owner_revision = 9;
            },
        ),
        (
            "corrected_at",
            |evidence: &mut bm_core::memory::LongTermMemoryCorrectionEvidence| {
                evidence.corrected_at += 1;
            },
        ),
        (
            "control_revision_id",
            |evidence: &mut bm_core::memory::LongTermMemoryCorrectionEvidence| {
                evidence.control_revision_id = "revision:other".into();
            },
        ),
    ];
    for (field, mutate) in cases {
        let mut drifted = closure.clone();
        let evidence = drifted.owner_records[0]
            .after
            .as_mut()
            .expect("successor")
            .governed_content
            .correction_evidence
            .as_mut()
            .expect("correction evidence");
        mutate(evidence);
        refresh_correction_successor_digests(&mut drifted);
        let invalid = validate_long_term_control_post_image(&drifted);
        assert!(
            invalid
                .failures
                .contains(&"long_term_control_correction_evidence_drift".to_string()),
            "{field}: {:?}",
            invalid.failures
        );
    }
}

#[test]
fn unknown_correction_lifecycle_fails_closed_at_v5_decode() {
    let closure = correct_post_image_closure();
    let material = closure.owner_records[0].after.as_ref().expect("successor");
    let mut encoded = serde_json::to_value(material).expect("material json");
    encoded["governed_content"]["correction_evidence"]["lifecycle"] = serde_json::json!("refresh");
    assert!(serde_json::from_value::<LongTermMemoryVersionMaterial>(encoded).is_err());
}
