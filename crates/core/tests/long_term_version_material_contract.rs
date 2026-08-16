use bm_core::memory::{
    build_current_dynamic_state_resolution_report,
    build_historical_dynamic_state_resolution_report, build_long_term_current_recall_authority,
    build_long_term_historical_recall_authority, decide_governed_recall_eligibility,
    long_term_version_head_key, long_term_version_material_key,
    long_term_version_scope_manifest_key, project_current_long_term_recall_lifecycle_facts,
    project_historical_long_term_recall_lifecycle_facts, scoped_long_term_control_storage_key,
    select_long_term_historical_recall_query_time, select_long_term_version_as_of,
    select_long_term_version_current, validate_long_term_version_head_closure,
    GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef, GovernedOwnerRevisionRef,
    GovernedOwnerTermination, GovernedOwnerTransition, GovernedProfileBudgetDrop,
    GovernedRecallAuthorityGates, GovernedRecallDisclosure, GovernedRecallEligibility,
    GovernedRecallEligibilityReason, GovernedRecallTemporalQuery, GovernedRequiredPremiseGate,
    LongTermControlOperation, LongTermMemoryConfidence, LongTermMemoryControlRevision,
    LongTermMemoryFreshness, LongTermMemoryGovernedContent, LongTermMemoryHeadManifest,
    LongTermMemoryKind, LongTermMemoryRetainedRevisionDigest, LongTermMemorySourceScope,
    LongTermMemorySourceType, LongTermMemoryStaleHint, LongTermMemoryVersionMaterial,
    LongTermMemoryVersionOrigin, LongTermMemoryVersionScopeManifest,
    LongTermMemoryVersionTransitionBinding, MemoryPrivacyClass, MemorySubjectVisibilityDecision,
    MemorySubjectVisibilityPolicy, LONG_TERM_CONTROL_REVISION_NAMESPACE,
    LONG_TERM_CONTROL_SCHEMA_VERSION, LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
};

const MEMORY_SPACE_ID: &str = "space-1";
const FACTUAL_OWNER_ID: &str = "space-1";
const OWNER_ID: &str = "state-1";

fn owner_ref() -> GovernedMemoryOwnerRef {
    GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, OWNER_ID)
}

fn revision_ref(owner_revision: u64) -> GovernedOwnerRevisionRef {
    GovernedOwnerRevisionRef::try_new(owner_ref(), owner_revision).expect("owner revision ref")
}

fn evidence_ref() -> GovernedOwnerRevisionRef {
    GovernedOwnerRevisionRef::try_new(
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::EvidenceDocument, "evidence-1"),
        1,
    )
    .expect("evidence revision ref")
}

fn material(
    owner_revision: u64,
    valid_from: u64,
    predecessor: Option<GovernedOwnerRevisionRef>,
    content: &str,
) -> LongTermMemoryVersionMaterial {
    let mut material = LongTermMemoryVersionMaterial {
        schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
        memory_space_id: MEMORY_SPACE_ID.into(),
        factual_owner_id: FACTUAL_OWNER_ID.into(),
        owner_ref: owner_ref(),
        owner_revision,
        governed_content: LongTermMemoryGovernedContent {
            kind: LongTermMemoryKind::Fact,
            topic: "deployment".into(),
            content: content.into(),
            keywords: vec!["region".into()],
            source_chat_id: None,
            source_type: LongTermMemorySourceType::SystemRuntime,
            source_scope: LongTermMemorySourceScope::World,
            confidence: LongTermMemoryConfidence::High,
            freshness: LongTermMemoryFreshness::Dynamic,
            stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
            supporting_citations: Vec::new(),
            canonical_entities: Vec::new(),
            evidence_count: 0,
            created_at: 10,
            updated_at: valid_from,
            observed_at: valid_from.saturating_add(2),
            last_confirmed_at: valid_from.saturating_add(2),
            source_revision: Some(owner_revision.saturating_add(2)),
            last_used_at: 0,
        },
        governed_evidence_refs: vec![evidence_ref()],
        origin: LongTermMemoryVersionOrigin {
            valid_from,
            observed_at: valid_from.saturating_add(2),
            predecessor,
        },
        privacy_class: MemoryPrivacyClass::PublicRuntime,
        subject_visibility: MemorySubjectVisibilityPolicy::AllSubjects,
        content_digest: String::new(),
    };
    material.content_digest = material
        .canonical_content_digest()
        .expect("canonical digest");
    material
}

#[test]
fn subject_visibility_policy_is_canonical_and_exact_subject_bound() {
    let only_a = MemorySubjectVisibilityPolicy::OnlySubjects(vec!["agent-a".into()]);
    assert_eq!(
        only_a
            .decision_for_subject("agent-a")
            .expect("canonical allow decision"),
        MemorySubjectVisibilityDecision::Allowed
    );
    assert_eq!(
        only_a
            .decision_for_subject("agent-b")
            .expect("canonical deny decision"),
        MemorySubjectVisibilityDecision::Denied
    );
    let hidden_b = MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec!["agent-b".into()]);
    assert_eq!(
        hidden_b
            .decision_for_subject("agent-a")
            .expect("canonical hidden allow decision"),
        MemorySubjectVisibilityDecision::Allowed
    );
    assert_eq!(
        hidden_b
            .decision_for_subject("agent-b")
            .expect("canonical hidden deny decision"),
        MemorySubjectVisibilityDecision::Denied
    );

    for invalid in [
        MemorySubjectVisibilityPolicy::OnlySubjects(Vec::new()),
        MemorySubjectVisibilityPolicy::OnlySubjects(vec!["agent-b".into(), "agent-a".into()]),
        MemorySubjectVisibilityPolicy::OnlySubjects(vec!["agent-a".into(), "agent-a".into()]),
        MemorySubjectVisibilityPolicy::HiddenFromSubjects(vec![" agent-b".into()]),
        MemorySubjectVisibilityPolicy::HiddenFromSubjects(Vec::new()),
    ] {
        assert!(invalid.decision_for_subject("agent-a").is_err());
    }
    assert!(only_a.decision_for_subject(" agent-a").is_err());
}

#[test]
fn version_v3_requires_and_digests_exact_subject_visibility() {
    let all = material(1, 10, None, "visibility-bound material");
    let mut only_a = all.clone();
    only_a.subject_visibility = MemorySubjectVisibilityPolicy::OnlySubjects(vec!["agent-a".into()]);
    only_a.content_digest = only_a
        .canonical_content_digest()
        .expect("visibility-bound digest");
    assert!(only_a.validate_contract().accepted);
    assert_ne!(all.content_digest, only_a.content_digest);

    let mut missing = serde_json::to_value(&only_a).expect("material JSON");
    missing
        .as_object_mut()
        .expect("material object")
        .remove("subject_visibility");
    assert!(serde_json::from_value::<LongTermMemoryVersionMaterial>(missing).is_err());

    let mut invalid = only_a;
    invalid.subject_visibility = MemorySubjectVisibilityPolicy::OnlySubjects(Vec::new());
    invalid.content_digest = invalid
        .canonical_content_digest()
        .expect("invalid payload still has bytes");
    assert!(!invalid.validate_contract().accepted);
}

fn transition(
    predecessor: u64,
    terminated_at: u64,
    termination: GovernedOwnerTermination,
    successor: Option<u64>,
) -> GovernedOwnerTransition {
    GovernedOwnerTransition {
        predecessor: revision_ref(predecessor),
        terminated_at,
        termination,
        successor: successor.map(revision_ref),
    }
}

fn control_revision(
    revision_id: &str,
    operation: LongTermControlOperation,
    transition: GovernedOwnerTransition,
    predecessor: &LongTermMemoryVersionMaterial,
    successor: Option<&LongTermMemoryVersionMaterial>,
) -> LongTermMemoryControlRevision {
    let mut revision = LongTermMemoryControlRevision {
        schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
        revision_id: revision_id.into(),
        memory_space_id: MEMORY_SPACE_ID.into(),
        factual_owner_id: FACTUAL_OWNER_ID.into(),
        operation,
        invalidation_reason_code: None,
        transition,
        predecessor_material_digest: predecessor.content_digest.clone(),
        successor_material_digest: successor.map(|material| material.content_digest.clone()),
        governed_evidence_refs: Vec::new(),
        reason: "governed historical transition".into(),
        actor_subject_id: None,
        created_at: successor
            .map(|material| material.origin.valid_from)
            .unwrap_or(30),
        content_digest: String::new(),
    };
    revision.content_digest = revision
        .canonical_content_digest()
        .expect("control revision digest");
    revision.validate_contract().expect("control revision");
    revision
}

#[test]
fn shared_fact_material_rejects_a_subject_scoped_factual_owner() {
    let mut invalid = material(1, 10, None, "shared fact");
    invalid.factual_owner_id = "agent:alpha".to_string();
    invalid.content_digest = invalid
        .canonical_content_digest()
        .expect("digest for invalid owner fixture");

    assert!(!invalid.validate_contract().accepted);
}

fn exact_transition_binding(
    revision: &LongTermMemoryControlRevision,
) -> LongTermMemoryVersionTransitionBinding {
    LongTermMemoryVersionTransitionBinding::new(
        revision.transition.predecessor.clone(),
        scoped_long_term_control_storage_key(
            MEMORY_SPACE_ID,
            LONG_TERM_CONTROL_REVISION_NAMESPACE,
            &revision.revision_id,
        )
        .expect("control key"),
        revision.content_digest.clone(),
    )
    .expect("exact transition binding")
}

fn transition_binding(
    transition: &GovernedOwnerTransition,
) -> LongTermMemoryVersionTransitionBinding {
    LongTermMemoryVersionTransitionBinding::new(
        transition.predecessor.clone(),
        format!(
            "scope:control:owner:{}:revision:{}",
            transition.predecessor.owner_ref.owner_id, transition.predecessor.owner_revision
        ),
        "a".repeat(64),
    )
    .expect("canonical transition binding")
}

fn head(
    materials: &[LongTermMemoryVersionMaterial],
    current_revision: u64,
    terminal_transition_ref: Option<GovernedOwnerRevisionRef>,
) -> LongTermMemoryHeadManifest {
    LongTermMemoryHeadManifest {
        schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
        memory_space_id: MEMORY_SPACE_ID.into(),
        factual_owner_id: FACTUAL_OWNER_ID.into(),
        owner_ref: owner_ref(),
        current_revision,
        retained_revision_digests: materials
            .iter()
            .map(|material| LongTermMemoryRetainedRevisionDigest {
                owner_revision: material.owner_revision,
                content_digest: material.content_digest.clone(),
            })
            .collect(),
        terminal_transition_ref,
        manifest_revision: current_revision,
    }
}

#[test]
fn as_of_selection_uses_the_exact_half_open_validity_interval() {
    let first = material(1, 10, None, "The active region is cn-east-2");
    let second = material(
        2,
        20,
        Some(first.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    let materials = vec![first, second];
    let transition = transition(1, 20, GovernedOwnerTermination::Corrected, Some(2));
    let head = head(&materials, 2, None);

    assert!(select_long_term_version_as_of(
        &head,
        &materials,
        std::slice::from_ref(&transition),
        9,
        4,
    )
    .expect("select before validity")
    .is_none());
    let first_projection =
        select_long_term_version_as_of(&head, &materials, std::slice::from_ref(&transition), 19, 4)
            .expect("select first interval")
            .expect("first projection");
    assert_eq!(first_projection.material.owner_revision, 1);
    assert_eq!(first_projection.validity.valid_from, 10);
    assert_eq!(first_projection.validity.valid_until, Some(20));
    assert_eq!(
        first_projection.validity.termination,
        Some(GovernedOwnerTermination::Corrected)
    );

    let second_projection = select_long_term_version_as_of(&head, &materials, &[transition], 20, 4)
        .expect("select second interval")
        .expect("second projection");
    assert_eq!(second_projection.material.owner_revision, 2);
    assert_eq!(second_projection.validity.valid_from, 20);
    assert_eq!(second_projection.validity.valid_until, None);
}

#[test]
fn historical_authority_binds_exact_scope_material_control_and_complete_lineage() {
    let first = material(1, 10, None, "The active region is cn-east-2");
    let second = material(
        2,
        20,
        Some(first.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    let transition = transition(1, 20, GovernedOwnerTermination::Revised, Some(2));
    let control = control_revision(
        "refresh-state-1-r1",
        LongTermControlOperation::Refresh,
        transition.clone(),
        &first,
        Some(&second),
    );
    let materials = vec![first.clone(), second.clone()];
    let head = head(&materials, 2, None);
    let scope = LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        7,
        std::slice::from_ref(&head),
        &materials,
        std::slice::from_ref(&transition),
        std::slice::from_ref(&exact_transition_binding(&control)),
        4,
    )
    .expect("exact historical scope");

    assert!(build_long_term_historical_recall_authority(
        &scope,
        std::slice::from_ref(&head),
        &materials,
        std::slice::from_ref(&control),
        &owner_ref(),
        9,
        4,
        2,
    )
    .expect("before first validity")
    .is_none());

    let predecessor = build_long_term_historical_recall_authority(
        &scope,
        std::slice::from_ref(&head),
        &materials,
        std::slice::from_ref(&control),
        &owner_ref(),
        19,
        4,
        2,
    )
    .expect("historical authority")
    .expect("predecessor projection");
    assert_eq!(predecessor.projection().material.owner_revision, 1);
    assert!(predecessor.lineage_report().complete);
    assert_eq!(predecessor.lineage_report().manifest_revision, 7);
    assert_eq!(predecessor.lineage_report().items.len(), 2);
    assert_eq!(
        select_long_term_historical_recall_query_time([&predecessor], 5)
            .expect("historical logical frontier"),
        20
    );
    assert!(select_long_term_historical_recall_query_time([&predecessor], 0).is_err());
    let lifecycle = project_historical_long_term_recall_lifecycle_facts(&predecessor)
        .expect("historical lifecycle");
    let report = build_historical_dynamic_state_resolution_report(
        &lifecycle,
        30,
        19,
        GovernedRecallAuthorityGates {
            disclosure: GovernedRecallDisclosure::Allowed,
            required_premise: GovernedRequiredPremiseGate::NotApplicable,
            profile_budget_drop: GovernedProfileBudgetDrop::None,
        },
    )
    .expect("historical report");
    assert_eq!(
        report.as_of_decision.expect("as-of decision").eligibility,
        GovernedRecallEligibility::EligibleHistoricalAsOf
    );
    assert_eq!(
        report.current_decision.eligibility,
        GovernedRecallEligibility::Excluded
    );

    let current_interval = build_long_term_historical_recall_authority(
        &scope,
        std::slice::from_ref(&head),
        &materials,
        std::slice::from_ref(&control),
        &owner_ref(),
        20,
        4,
        2,
    )
    .expect("current interval historical authority")
    .expect("current projection");
    assert_eq!(current_interval.projection().material.owner_revision, 2);

    assert!(build_long_term_historical_recall_authority(
        &scope,
        std::slice::from_ref(&head),
        &materials,
        std::slice::from_ref(&control),
        &owner_ref(),
        19,
        4,
        1,
    )
    .is_err());
}

#[test]
fn historical_authority_rejects_privacy_drift_and_corrected_model_revival() {
    let first = material(1, 10, None, "The active region is cn-east-2");
    let mut private_second = material(
        2,
        20,
        Some(first.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    private_second.privacy_class = MemoryPrivacyClass::PrivateGarden;
    private_second.content_digest = private_second
        .canonical_content_digest()
        .expect("private successor digest");
    let revised = transition(1, 20, GovernedOwnerTermination::Revised, Some(2));
    let privacy_control = control_revision(
        "change-privacy-state-1-r1",
        LongTermControlOperation::ChangePrivacy,
        revised.clone(),
        &first,
        Some(&private_second),
    );
    let privacy_materials = vec![first.clone(), private_second];
    let privacy_head = head(&privacy_materials, 2, None);
    let privacy_scope = LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        8,
        std::slice::from_ref(&privacy_head),
        &privacy_materials,
        std::slice::from_ref(&revised),
        std::slice::from_ref(&exact_transition_binding(&privacy_control)),
        4,
    )
    .expect("privacy transition scope");
    assert!(build_long_term_historical_recall_authority(
        &privacy_scope,
        std::slice::from_ref(&privacy_head),
        &privacy_materials,
        std::slice::from_ref(&privacy_control),
        &owner_ref(),
        19,
        4,
        2,
    )
    .is_err());

    let corrected_second = material(
        2,
        20,
        Some(first.owner_revision_ref()),
        "The corrected active region is cn-east-3",
    );
    let corrected = transition(1, 20, GovernedOwnerTermination::Corrected, Some(2));
    let corrected_control = control_revision(
        "correct-state-1-r1",
        LongTermControlOperation::Correct,
        corrected.clone(),
        &first,
        Some(&corrected_second),
    );
    let corrected_materials = vec![first, corrected_second];
    let corrected_head = head(&corrected_materials, 2, None);
    let corrected_scope = LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        9,
        std::slice::from_ref(&corrected_head),
        &corrected_materials,
        std::slice::from_ref(&corrected),
        std::slice::from_ref(&exact_transition_binding(&corrected_control)),
        4,
    )
    .expect("corrected scope");
    let corrected_authority = build_long_term_historical_recall_authority(
        &corrected_scope,
        std::slice::from_ref(&corrected_head),
        &corrected_materials,
        std::slice::from_ref(&corrected_control),
        &owner_ref(),
        19,
        4,
        2,
    )
    .expect("corrected historical authority")
    .expect("corrected predecessor projection");
    let corrected_lifecycle =
        project_historical_long_term_recall_lifecycle_facts(&corrected_authority)
            .expect("corrected historical lifecycle");
    let corrected_report = build_historical_dynamic_state_resolution_report(
        &corrected_lifecycle,
        30,
        19,
        GovernedRecallAuthorityGates {
            disclosure: GovernedRecallDisclosure::Allowed,
            required_premise: GovernedRequiredPremiseGate::NotApplicable,
            profile_budget_drop: GovernedProfileBudgetDrop::None,
        },
    )
    .expect("corrected material produces a canonical exclusion report");
    let corrected_as_of = corrected_report
        .as_of_decision
        .expect("corrected as-of decision");
    assert_eq!(
        corrected_as_of.eligibility,
        GovernedRecallEligibility::Excluded
    );
    assert_eq!(
        corrected_as_of.primary_reason,
        Some(GovernedRecallEligibilityReason::Obsolete)
    );
}

#[test]
fn current_selection_preserves_exact_terminal_control_for_eligibility() {
    let first = material(1, 10, None, "The active region is cn-east-2");
    let second = material(
        2,
        20,
        Some(first.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    let corrected = transition(1, 20, GovernedOwnerTermination::Corrected, Some(2));
    let active_head = head(&[first.clone(), second.clone()], 2, None);
    let active = select_long_term_version_current(
        &active_head,
        &[first.clone(), second],
        std::slice::from_ref(&corrected),
        4,
    )
    .expect("active current projection");
    assert_eq!(active.material.owner_revision, 2);
    assert_eq!(active.validity.valid_until, None);

    let invalidated = transition(1, 30, GovernedOwnerTermination::Invalidated, None);
    let terminal_head = head(
        std::slice::from_ref(&first),
        1,
        Some(first.owner_revision_ref()),
    );
    let terminal = select_long_term_version_current(
        &terminal_head,
        &[first],
        std::slice::from_ref(&invalidated),
        4,
    )
    .expect("terminal current projection");
    assert_eq!(terminal.material.owner_revision, 1);
    assert_eq!(terminal.validity.valid_until, Some(30));
    assert_eq!(
        terminal.validity.termination,
        Some(GovernedOwnerTermination::Invalidated)
    );
}

#[test]
fn governed_current_eligibility_is_derived_from_exact_long_term_owner_control() {
    let mut active = material(1, 10, None, "The active region is cn-east-2");
    active.governed_content.stale_hint = LongTermMemoryStaleHint::None;
    active.content_digest = active.canonical_content_digest().expect("active digest");
    let allowed = GovernedRecallAuthorityGates {
        disclosure: GovernedRecallDisclosure::Allowed,
        required_premise: GovernedRequiredPremiseGate::NotApplicable,
        profile_budget_drop: GovernedProfileBudgetDrop::None,
    };
    let active_head = head(std::slice::from_ref(&active), 1, None);
    let active_scope = LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        1,
        std::slice::from_ref(&active_head),
        std::slice::from_ref(&active),
        &[],
        &[],
        4,
    )
    .expect("active scope");
    let authority = build_long_term_current_recall_authority(
        &active_scope,
        &active_head,
        std::slice::from_ref(&active),
        &[],
        &[],
        &[],
        4,
    )
    .expect("current authority");
    let current = project_current_long_term_recall_lifecycle_facts(&authority)
        .expect("current lifecycle facts");
    assert_eq!(
        decide_governed_recall_eligibility(
            &current,
            GovernedRecallTemporalQuery::Current { query_time: 30 },
            allowed.clone(),
        )
        .expect("current decision")
        .eligibility,
        GovernedRecallEligibility::EligibleCurrent
    );
    assert!(decide_governed_recall_eligibility(
        &current,
        GovernedRecallTemporalQuery::Current { query_time: 9 },
        allowed.clone(),
    )
    .is_err());

    let private_facts = project_current_long_term_recall_lifecycle_facts(&authority)
        .expect("private lifecycle facts");
    assert_eq!(
        decide_governed_recall_eligibility(
            &private_facts,
            GovernedRecallTemporalQuery::Current { query_time: 30 },
            GovernedRecallAuthorityGates {
                disclosure: GovernedRecallDisclosure::PrivacyBlocked,
                required_premise: GovernedRequiredPremiseGate::NotApplicable,
                profile_budget_drop: GovernedProfileBudgetDrop::None,
            },
        )
        .expect("privacy decision")
        .primary_reason,
        Some(GovernedRecallEligibilityReason::PrivacyBlocked)
    );
}

#[test]
fn governed_current_authority_accepts_exact_cross_owner_supersede_dependency() {
    let predecessor = material(1, 10, None, "The active region is cn-east-2");
    let mut successor = material(
        1,
        20,
        Some(predecessor.owner_revision_ref()),
        "A replacement owner now governs the active region",
    );
    successor.owner_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "replacement-state-1");
    successor.content_digest = successor
        .canonical_content_digest()
        .expect("successor digest");
    let transition = GovernedOwnerTransition {
        predecessor: predecessor.owner_revision_ref(),
        terminated_at: 20,
        termination: GovernedOwnerTermination::Superseded,
        successor: Some(successor.owner_revision_ref()),
    };
    let mut control = LongTermMemoryControlRevision {
        schema_version: LONG_TERM_CONTROL_SCHEMA_VERSION,
        revision_id: "supersede-state-1-r1".into(),
        memory_space_id: MEMORY_SPACE_ID.into(),
        factual_owner_id: FACTUAL_OWNER_ID.into(),
        operation: LongTermControlOperation::Supersede,
        invalidation_reason_code: None,
        transition: transition.clone(),
        predecessor_material_digest: predecessor.content_digest.clone(),
        successor_material_digest: Some(successor.content_digest.clone()),
        governed_evidence_refs: Vec::new(),
        reason: "replace the governed owner".into(),
        actor_subject_id: None,
        created_at: 20,
        content_digest: String::new(),
    };
    control.content_digest = control.canonical_content_digest().expect("control digest");
    control.validate_contract().expect("control contract");
    let control_key = scoped_long_term_control_storage_key(
        MEMORY_SPACE_ID,
        LONG_TERM_CONTROL_REVISION_NAMESPACE,
        &control.revision_id,
    )
    .expect("control key");
    let binding = LongTermMemoryVersionTransitionBinding::new(
        predecessor.owner_revision_ref(),
        control_key,
        control.content_digest.clone(),
    )
    .expect("control binding");
    let predecessor_head = head(
        std::slice::from_ref(&predecessor),
        1,
        Some(predecessor.owner_revision_ref()),
    );
    let successor_head = LongTermMemoryHeadManifest {
        schema_version: LONG_TERM_MEMORY_VERSION_SCHEMA_VERSION,
        memory_space_id: MEMORY_SPACE_ID.into(),
        factual_owner_id: FACTUAL_OWNER_ID.into(),
        owner_ref: successor.owner_ref.clone(),
        current_revision: 1,
        retained_revision_digests: vec![LongTermMemoryRetainedRevisionDigest {
            owner_revision: 1,
            content_digest: successor.content_digest.clone(),
        }],
        terminal_transition_ref: None,
        manifest_revision: 1,
    };
    let scope = LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        2,
        &[predecessor_head.clone(), successor_head.clone()],
        &[predecessor.clone(), successor.clone()],
        std::slice::from_ref(&transition),
        std::slice::from_ref(&binding),
        4,
    )
    .expect("cross-owner scope");
    let authority = build_long_term_current_recall_authority(
        &scope,
        &predecessor_head,
        std::slice::from_ref(&predecessor),
        std::slice::from_ref(&successor_head),
        std::slice::from_ref(&successor),
        std::slice::from_ref(&control),
        4,
    )
    .expect("cross-owner current authority");
    let facts = project_current_long_term_recall_lifecycle_facts(&authority)
        .expect("superseded lifecycle facts");
    let report = build_current_dynamic_state_resolution_report(
        &facts,
        30,
        GovernedRecallAuthorityGates {
            disclosure: GovernedRecallDisclosure::Allowed,
            required_premise: GovernedRequiredPremiseGate::NotApplicable,
            profile_budget_drop: GovernedProfileBudgetDrop::None,
        },
    )
    .expect("superseded dynamic-state report");
    assert_eq!(report.validity, authority.projection().validity);
    assert_eq!(
        report.predecessor,
        authority.projection().validity.predecessor
    );
    assert_eq!(report.successor, authority.projection().validity.successor);
    assert_eq!(
        report.current_decision.owner_revision_ref,
        predecessor.owner_revision_ref()
    );
    assert_eq!(
        report.current_decision.primary_reason,
        Some(GovernedRecallEligibilityReason::Superseded)
    );
    assert!(report.as_of_decision.is_none());
    assert_eq!(report.conflict_count, 0);
    assert_eq!(report.unknown_count, 0);

    assert!(build_long_term_current_recall_authority(
        &scope,
        &predecessor_head,
        std::slice::from_ref(&predecessor),
        &[],
        std::slice::from_ref(&successor),
        std::slice::from_ref(&control),
        4,
    )
    .is_err());
    assert!(build_long_term_current_recall_authority(
        &scope,
        &predecessor_head,
        std::slice::from_ref(&predecessor),
        std::slice::from_ref(&successor_head),
        &[],
        std::slice::from_ref(&control),
        4,
    )
    .is_err());
    let mut extra_dependency = successor.clone();
    extra_dependency.owner_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "unexpected-state-1");
    extra_dependency.content_digest = extra_dependency
        .canonical_content_digest()
        .expect("extra dependency digest");
    assert!(build_long_term_current_recall_authority(
        &scope,
        &predecessor_head,
        std::slice::from_ref(&predecessor),
        std::slice::from_ref(&successor_head),
        &[successor.clone(), extra_dependency],
        std::slice::from_ref(&control),
        4,
    )
    .is_err());
    let mut missing_successor_head_binding = scope.clone();
    missing_successor_head_binding
        .head_bindings
        .retain(|head_binding| head_binding.owner_ref != successor.owner_ref);
    missing_successor_head_binding.head_count -= 1;
    assert!(build_long_term_current_recall_authority(
        &missing_successor_head_binding,
        &predecessor_head,
        std::slice::from_ref(&predecessor),
        std::slice::from_ref(&successor_head),
        std::slice::from_ref(&successor),
        std::slice::from_ref(&control),
        4,
    )
    .is_err());
    let mut wrong_control_binding = scope.clone();
    wrong_control_binding.transition_bindings[0].control_revision_content_digest = "b".repeat(64);
    assert!(build_long_term_current_recall_authority(
        &wrong_control_binding,
        &predecessor_head,
        std::slice::from_ref(&predecessor),
        std::slice::from_ref(&successor_head),
        std::slice::from_ref(&successor),
        std::slice::from_ref(&control),
        4,
    )
    .is_err());
    let mut over_cap_successor_head = successor_head.clone();
    over_cap_successor_head.current_revision = 2;
    over_cap_successor_head
        .retained_revision_digests
        .push(LongTermMemoryRetainedRevisionDigest {
            owner_revision: 2,
            content_digest: "e".repeat(64),
        });
    over_cap_successor_head.manifest_revision = 2;
    let mut over_cap_scope = scope.clone();
    let successor_binding = over_cap_scope
        .head_bindings
        .iter_mut()
        .find(|head_binding| head_binding.owner_ref == successor.owner_ref)
        .expect("successor binding");
    *successor_binding =
        bm_core::memory::LongTermMemoryVersionHeadBinding::from_head(&over_cap_successor_head)
            .expect("over-cap successor binding");
    over_cap_scope.head_bindings.sort();
    over_cap_scope.material_count += 1;
    assert!(build_long_term_current_recall_authority(
        &over_cap_scope,
        &predecessor_head,
        std::slice::from_ref(&predecessor),
        std::slice::from_ref(&over_cap_successor_head),
        std::slice::from_ref(&successor),
        std::slice::from_ref(&control),
        1,
    )
    .is_err());
}

#[test]
fn long_term_version_material_binds_origin_authority_fields_and_digest() {
    let material = material(1, 10, None, "The active region is cn-east-2");
    assert!(material.validate_contract().accepted);
    assert_eq!(material.origin.valid_from, 10);
    assert_eq!(material.origin.observed_at, 12);
    assert_eq!(material.origin.predecessor, None);

    let mut drifted = material;
    drifted.governed_content.content.push_str(" changed");
    assert!(!drifted.validate_contract().accepted);
}

#[test]
fn lifecycle_transition_does_not_rewrite_immutable_predecessor_material() {
    let predecessor = material(1, 10, None, "The active region is cn-east-2");
    let predecessor_digest = predecessor.content_digest.clone();
    let successor = material(
        2,
        20,
        Some(predecessor.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    let corrected = transition(1, 20, GovernedOwnerTermination::Corrected, Some(2));

    assert!(
        corrected
            .validate_contract(&predecessor, Some(&successor))
            .accepted
    );
    assert_eq!(predecessor.content_digest, predecessor_digest);
    assert!(predecessor.validate_contract().accepted);
    assert!(successor.validate_contract().accepted);
}

#[test]
fn owner_transition_requires_exact_successor_and_contiguous_validity_origin() {
    let predecessor = material(1, 10, None, "The active region is cn-east-2");
    let exact_successor = material(
        2,
        20,
        Some(predecessor.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    let corrected = transition(1, 20, GovernedOwnerTermination::Corrected, Some(2));
    assert!(
        corrected
            .validate_contract(&predecessor, Some(&exact_successor))
            .accepted
    );

    let discontinuous_successor = material(
        2,
        21,
        Some(predecessor.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    assert!(
        !corrected
            .validate_contract(&predecessor, Some(&discontinuous_successor))
            .accepted
    );

    let terminal_with_successor = transition(1, 20, GovernedOwnerTermination::Invalidated, Some(2));
    assert!(
        !terminal_with_successor
            .validate_contract(&predecessor, Some(&exact_successor))
            .accepted
    );
}

#[test]
fn superseded_transition_requires_a_cross_owner_revision_one_successor() {
    let predecessor = material(1, 10, None, "The active region is cn-east-2");
    let mut cross_owner_successor = material(
        1,
        20,
        Some(predecessor.owner_revision_ref()),
        "A replacement owner now governs the active region",
    );
    cross_owner_successor.owner_ref =
        GovernedMemoryOwnerRef::new(GovernedMemoryOwnerPlane::LongTerm, "replacement-state-1");
    cross_owner_successor.content_digest = cross_owner_successor
        .canonical_content_digest()
        .expect("cross-owner successor digest");
    let superseded = GovernedOwnerTransition {
        predecessor: predecessor.owner_revision_ref(),
        terminated_at: 20,
        termination: GovernedOwnerTermination::Superseded,
        successor: Some(cross_owner_successor.owner_revision_ref()),
    };

    assert!(
        superseded
            .validate_contract(&predecessor, Some(&cross_owner_successor))
            .accepted
    );

    let mut wrong_revision = cross_owner_successor;
    wrong_revision.owner_revision = 2;
    wrong_revision.content_digest = wrong_revision
        .canonical_content_digest()
        .expect("wrong revision digest");
    assert!(
        !superseded
            .validate_contract(&predecessor, Some(&wrong_revision))
            .accepted
    );
}

#[test]
fn active_head_requires_exact_material_and_transition_closure() {
    let first = material(1, 10, None, "The active region is cn-east-2");
    let second = material(
        2,
        20,
        Some(first.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    let corrected = transition(1, 20, GovernedOwnerTermination::Corrected, Some(2));
    let manifest = head(&[first.clone(), second.clone()], 2, None);

    assert!(
        validate_long_term_version_head_closure(
            &manifest,
            &[first.clone(), second.clone()],
            std::slice::from_ref(&corrected),
            2,
        )
        .accepted
    );
    assert!(
        !validate_long_term_version_head_closure(
            &manifest,
            std::slice::from_ref(&second),
            std::slice::from_ref(&corrected),
            2,
        )
        .accepted
    );
    assert!(
        !validate_long_term_version_head_closure(&manifest, &[first, second], &[], 2,).accepted
    );
}

#[test]
fn terminal_head_must_reference_its_exact_append_only_transition() {
    let material = material(1, 10, None, "The active region is cn-east-2");
    let invalidated = transition(1, 20, GovernedOwnerTermination::Invalidated, None);
    let terminal_head = head(
        std::slice::from_ref(&material),
        1,
        Some(material.owner_revision_ref()),
    );

    assert!(
        validate_long_term_version_head_closure(
            &terminal_head,
            std::slice::from_ref(&material),
            std::slice::from_ref(&invalidated),
            1,
        )
        .accepted
    );

    let missing_ref = head(std::slice::from_ref(&material), 1, None);
    assert!(
        !validate_long_term_version_head_closure(
            &missing_ref,
            std::slice::from_ref(&material),
            std::slice::from_ref(&invalidated),
            1,
        )
        .accepted
    );

    let wrong_ref = head(std::slice::from_ref(&material), 1, Some(revision_ref(2)));
    assert!(
        !validate_long_term_version_head_closure(
            &wrong_ref,
            std::slice::from_ref(&material),
            std::slice::from_ref(&invalidated),
            1,
        )
        .accepted
    );
}

#[test]
fn scope_manifest_requires_exact_heads_materials_and_transitions() {
    let first = material(1, 10, None, "The active region is cn-east-2");
    let second = material(
        2,
        20,
        Some(first.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    let corrected = transition(1, 20, GovernedOwnerTermination::Corrected, Some(2));
    let corrected_binding = transition_binding(&corrected);
    let head = head(&[first.clone(), second.clone()], 2, None);
    let manifest = LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        1,
        std::slice::from_ref(&head),
        &[first.clone(), second.clone()],
        std::slice::from_ref(&corrected),
        std::slice::from_ref(&corrected_binding),
        2,
    )
    .expect("exact scope manifest");

    assert!(
        manifest
            .validate_exact(
                std::slice::from_ref(&head),
                &[first.clone(), second.clone()],
                std::slice::from_ref(&corrected),
                std::slice::from_ref(&corrected_binding),
                2,
            )
            .accepted
    );
    assert!(
        !manifest
            .validate_exact(
                std::slice::from_ref(&head),
                std::slice::from_ref(&second),
                std::slice::from_ref(&corrected),
                std::slice::from_ref(&corrected_binding),
                2,
            )
            .accepted
    );
}

#[test]
fn scope_manifest_rejects_an_extra_unreferenced_material() {
    let retained = material(1, 10, None, "The active region is cn-east-2");
    let head = head(std::slice::from_ref(&retained), 1, None);
    let manifest = LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        1,
        std::slice::from_ref(&head),
        std::slice::from_ref(&retained),
        &[],
        &[],
        2,
    )
    .expect("single-revision scope manifest");
    let extra = material(
        2,
        20,
        Some(retained.owner_revision_ref()),
        "Unreferenced material must not survive exact closure",
    );

    assert!(
        !manifest
            .validate_exact(std::slice::from_ref(&head), &[retained, extra], &[], &[], 2,)
            .accepted
    );
}

#[test]
fn head_and_scope_manifest_enforce_profile_retention_bound() {
    let first = material(1, 10, None, "The active region is cn-east-2");
    let second = material(
        2,
        20,
        Some(first.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    let corrected = transition(1, 20, GovernedOwnerTermination::Corrected, Some(2));
    let corrected_binding = transition_binding(&corrected);
    let head = head(&[first.clone(), second.clone()], 2, None);

    assert!(
        !validate_long_term_version_head_closure(
            &head,
            &[first.clone(), second.clone()],
            std::slice::from_ref(&corrected),
            1,
        )
        .accepted
    );
    assert!(LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        1,
        std::slice::from_ref(&head),
        &[first, second],
        std::slice::from_ref(&corrected),
        std::slice::from_ref(&corrected_binding),
        1,
    )
    .is_err());
}

#[test]
fn scope_manifest_is_a_known_key_root_for_heads_and_existing_control_records() {
    let first = material(1, 10, None, "The active region is cn-east-2");
    let second = material(
        2,
        20,
        Some(first.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    let corrected = transition(1, 20, GovernedOwnerTermination::Corrected, Some(2));
    let corrected_binding = transition_binding(&corrected);
    let head = head(&[first.clone(), second.clone()], 2, None);

    let manifest = LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        1,
        std::slice::from_ref(&head),
        &[first, second],
        std::slice::from_ref(&corrected),
        std::slice::from_ref(&corrected_binding),
        2,
    )
    .expect("addressable scope manifest");

    assert_eq!(
        manifest.physical_key,
        long_term_version_scope_manifest_key(MEMORY_SPACE_ID, FACTUAL_OWNER_ID)
            .expect("scope manifest key")
    );
    assert_eq!(manifest.head_bindings.len(), 1);
    assert_eq!(manifest.head_bindings[0].owner_ref, owner_ref());
    assert_eq!(
        manifest.head_bindings[0].head_physical_key,
        long_term_version_head_key(MEMORY_SPACE_ID, FACTUAL_OWNER_ID, &owner_ref())
            .expect("head key")
    );
    assert_eq!(
        manifest.head_bindings[0].head_manifest_revision,
        head.manifest_revision
    );
    assert_eq!(
        manifest.transition_bindings,
        vec![corrected_binding.clone()]
    );

    let mut drifted = manifest;
    drifted.head_bindings[0].head_content_digest = "b".repeat(64);
    assert!(
        !drifted
            .validate_exact(
                std::slice::from_ref(&head),
                &[
                    material(1, 10, None, "The active region is cn-east-2"),
                    material(
                        2,
                        20,
                        Some(revision_ref(1)),
                        "The active region is cn-east-3",
                    ),
                ],
                std::slice::from_ref(&corrected),
                std::slice::from_ref(&corrected_binding),
                2,
            )
            .accepted
    );
}

#[test]
fn scope_manifest_rejects_missing_or_extra_control_transition_binding() {
    let first = material(1, 10, None, "The active region is cn-east-2");
    let second = material(
        2,
        20,
        Some(first.owner_revision_ref()),
        "The active region is cn-east-3",
    );
    let corrected = transition(1, 20, GovernedOwnerTermination::Corrected, Some(2));
    let head = head(&[first.clone(), second.clone()], 2, None);

    assert!(LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        1,
        std::slice::from_ref(&head),
        &[first.clone(), second.clone()],
        std::slice::from_ref(&corrected),
        &[],
        2,
    )
    .is_err());

    let expected = transition_binding(&corrected);
    let extra = LongTermMemoryVersionTransitionBinding::new(
        revision_ref(2),
        "scope:control:extra".to_string(),
        "c".repeat(64),
    )
    .expect("syntactically canonical extra binding");
    assert!(LongTermMemoryVersionScopeManifest::build(
        MEMORY_SPACE_ID,
        FACTUAL_OWNER_ID,
        1,
        std::slice::from_ref(&head),
        &[first, second],
        std::slice::from_ref(&corrected),
        &[expected, extra],
        2,
    )
    .is_err());
}

#[test]
fn long_term_version_keys_bind_space_factual_owner_and_revision() {
    let owner = owner_ref();
    let material_1 =
        long_term_version_material_key(MEMORY_SPACE_ID, FACTUAL_OWNER_ID, &owner, 1).expect("key");
    let material_2 =
        long_term_version_material_key(MEMORY_SPACE_ID, FACTUAL_OWNER_ID, &owner, 2).expect("key");
    let other_subject =
        long_term_version_material_key(MEMORY_SPACE_ID, "subject-2", &owner, 1).expect("key");
    let head = long_term_version_head_key(MEMORY_SPACE_ID, FACTUAL_OWNER_ID, &owner).expect("key");
    let scope =
        long_term_version_scope_manifest_key(MEMORY_SPACE_ID, FACTUAL_OWNER_ID).expect("key");
    let other_scope =
        long_term_version_scope_manifest_key(MEMORY_SPACE_ID, "subject-2").expect("key");
    assert_ne!(material_1, material_2);
    assert_ne!(material_1, other_subject);
    assert_ne!(material_1, head);
    assert_ne!(scope, other_scope);
}
