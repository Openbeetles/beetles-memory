use bm_core::memory::{
    build_governed_recall_eligibility_report, build_memory_update_lineage_report,
    primary_governed_recall_reason, GovernedContractFailure, GovernedMemoryOwnerPlane,
    GovernedMemoryOwnerRef, GovernedOwnerRevisionRef, GovernedOwnerTermination,
    GovernedOwnerValidity, GovernedProfileBudgetDrop, GovernedRecallEligibility,
    GovernedRecallEligibilityDecision, GovernedRecallEligibilityReason,
    GovernedRecallTemporalQuery, GovernedUpdateLineageItem, MemoryPrivacyClass,
    MemoryUpdateLineageFailure, MemoryUpdateLineageReport, MAX_GOVERNED_ELIGIBILITY_REASONS,
};

fn revision(owner_id: &str, owner_revision: u64) -> GovernedOwnerRevisionRef {
    GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, owner_id),
        owner_revision,
    )
    .expect("valid revision ref")
}

fn digest(byte: char) -> String {
    byte.to_string().repeat(64)
}

#[test]
fn governed_owner_revision_ref_rejects_invalid_owner_or_zero_revision() {
    assert!(GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, ""),
        1,
    )
    .is_err());
    assert!(GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, " owner "),
        1,
    )
    .is_err());
    assert!(GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "owner"),
        0,
    )
    .is_err());
}

#[test]
fn governed_owner_validity_enforces_half_open_interval_and_terminal_semantics() {
    let current = revision("state", 1);
    let open = GovernedOwnerValidity {
        valid_from: 10,
        valid_until: None,
        observed_at: 99,
        predecessor: None,
        successor: None,
        termination: None,
    };
    assert!(open.validate_for(&current).accepted);

    let invalid_interval = GovernedOwnerValidity {
        valid_until: Some(10),
        termination: Some(GovernedOwnerTermination::Invalidated),
        ..open.clone()
    };
    assert!(invalid_interval
        .validate_for(&current)
        .failures
        .contains(&GovernedContractFailure::ValidityIntervalInvalid));

    let corrected = GovernedOwnerValidity {
        valid_until: Some(20),
        successor: Some(revision("state", 2)),
        termination: Some(GovernedOwnerTermination::Corrected),
        ..open.clone()
    };
    assert!(corrected.validate_for(&current).accepted);

    let superseded = GovernedOwnerValidity {
        valid_until: Some(20),
        successor: Some(revision("replacement", 1)),
        termination: Some(GovernedOwnerTermination::Superseded),
        ..open.clone()
    };
    assert!(superseded.validate_for(&current).accepted);

    let superseded_to_later_revision = GovernedOwnerValidity {
        successor: Some(revision("replacement", 7)),
        ..superseded
    };
    assert!(superseded_to_later_revision
        .validate_for(&current)
        .failures
        .contains(&GovernedContractFailure::ValiditySuccessorMismatch));

    let invalidated_with_successor = GovernedOwnerValidity {
        valid_until: Some(20),
        successor: Some(revision("replacement", 7)),
        termination: Some(GovernedOwnerTermination::Invalidated),
        ..open
    };
    assert!(invalidated_with_successor
        .validate_for(&current)
        .failures
        .contains(&GovernedContractFailure::ValiditySuccessorMismatch));
}

#[test]
fn governed_recall_eligibility_primary_reason_uses_fixed_precedence() {
    use GovernedRecallEligibilityReason as Reason;
    let ordered = [
        Reason::PrivacyBlocked,
        Reason::Forgotten,
        Reason::Deleted,
        Reason::Invalidated,
        Reason::Superseded,
        Reason::Obsolete,
        Reason::Stale,
        Reason::PremiseBlocked,
        Reason::ProfileBlocked,
        Reason::BudgetBlocked,
        Reason::Tombstoned,
        Reason::Redacted,
    ];
    for (index, expected) in ordered.iter().copied().enumerate() {
        assert_eq!(
            primary_governed_recall_reason(&ordered[index..]),
            Some(expected)
        );
    }
}

#[test]
fn governed_recall_eligibility_rejects_duplicate_unbounded_or_inconsistent_reasons() {
    let base = GovernedRecallEligibilityDecision {
        eligibility: GovernedRecallEligibility::Excluded,
        primary_reason: Some(GovernedRecallEligibilityReason::Stale),
        reasons: vec![GovernedRecallEligibilityReason::Stale],
        owner_revision_ref: revision("state", 1),
        query_time: 30,
        effective_time: 20,
        premise_decision_ref: None,
        profile_budget_drop: GovernedProfileBudgetDrop::None,
    };
    assert!(base.validate_contract().accepted);

    let mut duplicate = base.clone();
    duplicate
        .reasons
        .push(GovernedRecallEligibilityReason::Stale);
    assert!(duplicate
        .validate_contract()
        .failures
        .contains(&GovernedContractFailure::EligibilityReasonDuplicate));

    let mut unbounded = base.clone();
    unbounded.reasons =
        vec![GovernedRecallEligibilityReason::Stale; MAX_GOVERNED_ELIGIBILITY_REASONS + 1];
    assert!(unbounded
        .validate_contract()
        .failures
        .contains(&GovernedContractFailure::EligibilityReasonLimitExceeded));

    let mut inconsistent = base;
    inconsistent.primary_reason = Some(GovernedRecallEligibilityReason::BudgetBlocked);
    assert!(inconsistent
        .validate_contract()
        .failures
        .contains(&GovernedContractFailure::EligibilityPrimaryReasonMismatch));

    let missing_premise_ref = GovernedRecallEligibilityDecision {
        eligibility: GovernedRecallEligibility::Excluded,
        primary_reason: Some(GovernedRecallEligibilityReason::PremiseBlocked),
        reasons: vec![GovernedRecallEligibilityReason::PremiseBlocked],
        owner_revision_ref: revision("premise", 1),
        query_time: 30,
        effective_time: 30,
        premise_decision_ref: None,
        profile_budget_drop: GovernedProfileBudgetDrop::None,
    };
    assert!(missing_premise_ref
        .validate_contract()
        .failures
        .contains(&GovernedContractFailure::PremiseGateInvalid));
}

#[test]
fn governed_recall_eligibility_report_recomputes_counts_and_rejects_duplicates() {
    let current = GovernedRecallEligibilityDecision {
        eligibility: GovernedRecallEligibility::EligibleCurrent,
        primary_reason: None,
        reasons: Vec::new(),
        owner_revision_ref: revision("current", 2),
        query_time: 30,
        effective_time: 30,
        premise_decision_ref: None,
        profile_budget_drop: GovernedProfileBudgetDrop::None,
    };
    let blocked = GovernedRecallEligibilityDecision {
        eligibility: GovernedRecallEligibility::Excluded,
        primary_reason: Some(GovernedRecallEligibilityReason::Forgotten),
        reasons: vec![GovernedRecallEligibilityReason::Forgotten],
        owner_revision_ref: revision("forgotten", 1),
        query_time: 30,
        effective_time: 30,
        premise_decision_ref: None,
        profile_budget_drop: GovernedProfileBudgetDrop::None,
    };
    let report = build_governed_recall_eligibility_report(
        vec![current.clone(), blocked],
        GovernedRecallTemporalQuery::Current { query_time: 30 },
        2,
    )
    .expect("canonical report");
    assert!(report.validate_contract().accepted);
    assert_eq!(
        report
            .eligibility_counts
            .get(&GovernedRecallEligibility::EligibleCurrent),
        Some(&1)
    );
    assert_eq!(
        report
            .reason_counts
            .get(&GovernedRecallEligibilityReason::Forgotten),
        Some(&1)
    );
    assert!(build_governed_recall_eligibility_report(
        vec![current.clone(), current],
        GovernedRecallTemporalQuery::Current { query_time: 30 },
        2,
    )
    .is_err());

    let drifted_current = GovernedRecallEligibilityDecision {
        effective_time: 29,
        ..GovernedRecallEligibilityDecision {
            eligibility: GovernedRecallEligibility::EligibleCurrent,
            primary_reason: None,
            reasons: Vec::new(),
            owner_revision_ref: revision("drifted-current", 1),
            query_time: 30,
            effective_time: 30,
            premise_decision_ref: None,
            profile_budget_drop: GovernedProfileBudgetDrop::None,
        }
    };
    assert!(build_governed_recall_eligibility_report(
        vec![drifted_current.clone()],
        GovernedRecallTemporalQuery::Current { query_time: 30 },
        1,
    )
    .is_err());
    let drifted_report = bm_core::memory::GovernedRecallEligibilityReport {
        schema_version: bm_core::memory::GOVERNED_MEMORY_LIFECYCLE_SCHEMA_VERSION,
        decisions: vec![drifted_current],
        eligibility_counts: [(GovernedRecallEligibility::EligibleCurrent, 1)].into(),
        reason_counts: Default::default(),
        as_of_time: None,
        profile_budget_drop_count: 0,
    };
    assert!(drifted_report
        .validate_contract()
        .failures
        .contains(&GovernedContractFailure::EligibilityQueryMismatch));
}

#[test]
fn memory_update_lineage_report_rejects_cycle_gap_scope_privacy_and_depth_drift() {
    let first = revision("state", 1);
    let second = revision("state", 2);
    let items = vec![
        GovernedUpdateLineageItem {
            owner_revision_ref: first.clone(),
            predecessor: None,
            successor: Some(second.clone()),
            scope_digest: digest('a'),
            privacy_class: MemoryPrivacyClass::PublicRuntime,
            content_digest: digest('c'),
        },
        GovernedUpdateLineageItem {
            owner_revision_ref: second,
            predecessor: None,
            successor: None,
            scope_digest: digest('b'),
            privacy_class: MemoryPrivacyClass::PrivateGarden,
            content_digest: digest('d'),
        },
    ];
    let report = MemoryUpdateLineageReport {
        schema_version: 1,
        items,
        failures: vec![
            MemoryUpdateLineageFailure::Gap,
            MemoryUpdateLineageFailure::ScopeMismatch,
            MemoryUpdateLineageFailure::PrivacyMismatch,
            MemoryUpdateLineageFailure::DepthExceeded,
        ],
        manifest_revision: 1,
        max_lineage_depth: 1,
        complete: false,
    };
    let validation = report.validate_contract();
    assert!(!validation.accepted);
    assert!(validation
        .failures
        .contains(&GovernedContractFailure::LineageGap));
    assert!(validation
        .failures
        .contains(&GovernedContractFailure::LineageScopeMismatch));
    assert!(validation
        .failures
        .contains(&GovernedContractFailure::LineagePrivacyMismatch));
    assert!(validation
        .failures
        .contains(&GovernedContractFailure::LineageDepthExceeded));
}

#[test]
fn memory_update_lineage_builder_derives_order_completeness_and_failures() {
    let first = revision("state", 1);
    let second = revision("state", 2);
    let report = build_memory_update_lineage_report(
        vec![
            GovernedUpdateLineageItem {
                owner_revision_ref: second.clone(),
                predecessor: Some(first.clone()),
                successor: None,
                scope_digest: digest('a'),
                privacy_class: MemoryPrivacyClass::PublicRuntime,
                content_digest: digest('d'),
            },
            GovernedUpdateLineageItem {
                owner_revision_ref: first.clone(),
                predecessor: None,
                successor: Some(second.clone()),
                scope_digest: digest('a'),
                privacy_class: MemoryPrivacyClass::PublicRuntime,
                content_digest: digest('c'),
            },
        ],
        7,
        2,
    )
    .expect("canonical lineage");
    assert!(report.complete);
    assert!(report.failures.is_empty());
    assert_eq!(report.items[0].owner_revision_ref, first);
    assert_eq!(report.items[1].owner_revision_ref, second);

    let gap = build_memory_update_lineage_report(
        vec![GovernedUpdateLineageItem {
            owner_revision_ref: revision("gap", 1),
            predecessor: None,
            successor: Some(revision("gap", 2)),
            scope_digest: digest('a'),
            privacy_class: MemoryPrivacyClass::PublicRuntime,
            content_digest: digest('c'),
        }],
        8,
        2,
    )
    .expect("bounded incomplete lineage report");
    assert!(!gap.complete);
    assert_eq!(gap.failures, vec![MemoryUpdateLineageFailure::Gap]);

    let missing_predecessor = build_memory_update_lineage_report(
        vec![GovernedUpdateLineageItem {
            owner_revision_ref: revision("missing-predecessor", 2),
            predecessor: Some(revision("missing-predecessor", 1)),
            successor: None,
            scope_digest: digest('a'),
            privacy_class: MemoryPrivacyClass::PublicRuntime,
            content_digest: digest('d'),
        }],
        9,
        2,
    )
    .expect("missing predecessor is a bounded diagnostic report");
    assert!(!missing_predecessor.complete);
    assert_eq!(
        missing_predecessor.failures,
        vec![MemoryUpdateLineageFailure::Gap]
    );

    let invalid_digest = build_memory_update_lineage_report(
        vec![GovernedUpdateLineageItem {
            owner_revision_ref: revision("invalid-digest", 1),
            predecessor: None,
            successor: None,
            scope_digest: "not-a-digest".into(),
            privacy_class: MemoryPrivacyClass::PublicRuntime,
            content_digest: digest('c'),
        }],
        10,
        1,
    );
    assert!(invalid_digest.is_err());
}

#[test]
fn memory_update_lineage_report_detects_multi_revision_cycle() {
    let first = revision("state", 1);
    let second = revision("state", 2);
    let item = |owner_revision_ref: GovernedOwnerRevisionRef,
                predecessor: GovernedOwnerRevisionRef,
                successor: GovernedOwnerRevisionRef| GovernedUpdateLineageItem {
        owner_revision_ref,
        predecessor: Some(predecessor),
        successor: Some(successor),
        scope_digest: digest('a'),
        privacy_class: MemoryPrivacyClass::PublicRuntime,
        content_digest: digest('c'),
    };
    let report = MemoryUpdateLineageReport {
        schema_version: 1,
        items: vec![
            item(first.clone(), second.clone(), second.clone()),
            item(second, first.clone(), first),
        ],
        failures: vec![MemoryUpdateLineageFailure::Cycle],
        manifest_revision: 1,
        max_lineage_depth: 4,
        complete: false,
    };
    assert!(report
        .validate_contract()
        .failures
        .contains(&GovernedContractFailure::LineageCycle));
}
