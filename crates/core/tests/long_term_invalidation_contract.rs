use bm_core::memory::{
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, GovernedOwnerRevisionRef,
    LongTermControlOperation, LongTermInvalidationContract, LongTermInvalidationReasonCode,
    MemoryLongTermTarget,
};

#[test]
fn long_term_control_operation_round_trips_typed_invalidate() {
    assert_eq!(LongTermControlOperation::Invalidate.as_str(), "invalidate");
    assert_eq!(
        LongTermControlOperation::from_label("invalidate"),
        Some(LongTermControlOperation::Invalidate)
    );
    assert_ne!(
        LongTermControlOperation::Invalidate,
        LongTermControlOperation::MarkStale
    );
}

#[test]
fn long_term_invalidation_requires_governed_evidence_revision_and_actor_audit() {
    let evidence = GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::EvidenceDocument, "evidence-1"),
        4,
    )
    .expect("evidence ref");
    let contract = LongTermInvalidationContract {
        target: MemoryLongTermTarget::RecordId("state-1".into()),
        reason_code: LongTermInvalidationReasonCode::FactuallyIncorrect,
        governed_evidence_refs: vec![evidence],
        actor_subject_id: "operator-1".into(),
        audit_reason: "governed evidence disproves the stored state".into(),
    };
    assert!(contract.validate_contract().accepted);

    let mut missing_evidence = contract.clone();
    missing_evidence.governed_evidence_refs.clear();
    assert!(!missing_evidence.validate_contract().accepted);

    let mut wrong_plane = contract;
    wrong_plane.governed_evidence_refs = vec![GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "state-2"),
        1,
    )
    .expect("revision")];
    assert!(!wrong_plane.validate_contract().accepted);
}
