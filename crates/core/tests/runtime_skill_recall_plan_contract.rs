use bm_core::memory::{
    build_procedural_memory_delivery_report, build_public_safe_procedural_memory_delivery_report,
    finalize_procedural_memory_delivery_report, GovernedMemoryOwnerPlane, GovernedMemoryOwnerRef,
    GovernedOwnerRevisionRef, MemoryPrivacyClass, PremiseEvaluationDecision, PremiseTypedSource,
    ProceduralMemoryDeliveryReport,
};
use bm_core::skills::{
    build_runtime_skill_projection_material, build_runtime_skill_recall_plan,
    canonical_runtime_skill_owner_id, canonical_runtime_skill_owner_key,
    runtime_skill_scope_manifest_key, RuntimeSkillApplicability, RuntimeSkillApplicabilityContext,
    RuntimeSkillApplicabilityTarget, RuntimeSkillAvailability, RuntimeSkillCapabilityAffinity,
    RuntimeSkillCreationRef, RuntimeSkillDeliveryDropReason, RuntimeSkillEvidenceBinding,
    RuntimeSkillEvidenceKind, RuntimeSkillFailureMode, RuntimeSkillIntrinsicContract,
    RuntimeSkillLifecycle, RuntimeSkillLifecycleLineage, RuntimeSkillLifecycleState,
    RuntimeSkillOperationAuthorityRef, RuntimeSkillOwnerBinding, RuntimeSkillOwnerLocator,
    RuntimeSkillOwnerRecord, RuntimeSkillOwningScope, RuntimeSkillPremise,
    RuntimeSkillPremiseObservation, RuntimeSkillPremiseRequirement, RuntimeSkillProceduralContent,
    RuntimeSkillProjectionPolicy, RuntimeSkillProjectionRenderReceipt, RuntimeSkillRecallAuthority,
    RuntimeSkillRecallBudgetAuthority, RuntimeSkillRecallQuery, RuntimeSkillScopeManifest,
    RuntimeSkillUsageOutcome, RuntimeSkillUsageOutcomeSummary,
    RUNTIME_SKILL_GOVERNED_CONTRACT_SCHEMA_VERSION,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn budget() -> RuntimeSkillRecallBudgetAuthority {
    RuntimeSkillRecallBudgetAuthority::try_new(4, 4, 4, 4, 4).expect("runtime skill recall budget")
}

fn authority(subject_id: &str) -> RuntimeSkillRecallAuthority {
    RuntimeSkillRecallAuthority::try_new(true, true, true, Some(subject_id.into()))
        .expect("runtime skill recall authority")
}

fn operation_authority_ref() -> RuntimeSkillOperationAuthorityRef {
    RuntimeSkillOperationAuthorityRef::try_new(format!(
        "runtime_skill_operation:sha256:{}",
        "e".repeat(64)
    ))
    .expect("operation authority ref")
}

fn manifest_for(record: &RuntimeSkillOwnerRecord) -> RuntimeSkillScopeManifest {
    RuntimeSkillScopeManifest::build(
        1,
        &record.memory_space_id,
        record.owning_scope.clone(),
        [RuntimeSkillOwnerBinding::from_record(record).expect("owner binding")],
        4,
    )
    .expect("scope manifest")
}

fn plan_for(
    record: &RuntimeSkillOwnerRecord,
    observations: Vec<RuntimeSkillPremiseObservation>,
    authority: RuntimeSkillRecallAuthority,
) -> bm_core::skills::RuntimeSkillRecallPlan {
    plan_for_at(record, observations, authority, 120)
}

fn plan_for_at(
    record: &RuntimeSkillOwnerRecord,
    observations: Vec<RuntimeSkillPremiseObservation>,
    authority: RuntimeSkillRecallAuthority,
    query_time: u64,
) -> bm_core::skills::RuntimeSkillRecallPlan {
    let manifest = manifest_for(record);
    build_runtime_skill_recall_plan(
        record,
        &manifest,
        operation_authority_ref(),
        query_time,
        RuntimeSkillRecallQuery::try_from_text("deployment").expect("canonical query"),
        RuntimeSkillApplicabilityContext::try_new(Vec::new()).expect("applicability context"),
        observations,
        authority,
        budget(),
    )
    .expect("canonical per-owner plan")
}

fn creation_ref() -> RuntimeSkillCreationRef {
    RuntimeSkillCreationRef::TaskLearningPromotion {
        learning_id: "learning-1".into(),
        learning_digest: digest('b'),
    }
}

fn intrinsic() -> RuntimeSkillIntrinsicContract {
    RuntimeSkillIntrinsicContract {
        schema_version: RUNTIME_SKILL_GOVERNED_CONTRACT_SCHEMA_VERSION,
        applicability: RuntimeSkillApplicability::Global,
        triggers: Vec::new(),
        constraints: Vec::new(),
        premises: vec![RuntimeSkillPremiseRequirement {
            premise: RuntimeSkillPremise::OpaquePresenceAttestation {
                handle_ref: "credential-presence-handle".into(),
            },
            required: true,
            valid_from: 10,
            valid_until: None,
            privacy_class: MemoryPrivacyClass::SharedWithSubject,
            governed_evidence_refs: Vec::new(),
        }],
        failure_modes: vec![RuntimeSkillFailureMode::RequiredPremiseUnsatisfied],
        evidence_bindings: vec![RuntimeSkillEvidenceBinding {
            kind: RuntimeSkillEvidenceKind::TaskLearning,
            safe_ref: "learning-1".into(),
            source_digest: digest('b'),
        }],
        projection_policy: RuntimeSkillProjectionPolicy {
            privacy_class: MemoryPrivacyClass::SharedWithSubject,
            model_projection_allowed: true,
            require_all_mandatory_premises: true,
        },
        capability_affinities: vec![RuntimeSkillCapabilityAffinity::ProceduralRecall],
    }
}

fn procedural_content() -> RuntimeSkillProceduralContent {
    RuntimeSkillProceduralContent {
        title: "Deploy safely".into(),
        topic: "deployment".into(),
        summary: "Use a governed release sequence.".into(),
        procedure: "Verify evidence, deploy, then inspect the receipt.".into(),
    }
}

#[test]
fn runtime_skill_management_locator_binds_physical_scope_and_exact_owner_revision() {
    let record = owner_record(RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-a".into(),
    });
    let locator = RuntimeSkillOwnerLocator::from_record(&record);
    assert!(locator.validate_for("space-1"));

    assert!(RuntimeSkillOwnerLocator::try_new(
        locator.owning_scope().clone(),
        locator.owner_id(),
        0,
    )
    .is_err());

    let canonical = serde_json::to_value(&locator).expect("locator JSON");
    assert_eq!(canonical["owner_id"], locator.owner_id());
    assert_eq!(canonical["owner_revision"], locator.owner_revision());
    assert!(canonical.get("owner_revision_ref").is_none());

    let mut wrong_revision = canonical.clone();
    wrong_revision["owner_revision"] = serde_json::json!(0);
    assert!(serde_json::from_value::<RuntimeSkillOwnerLocator>(wrong_revision).is_err());

    let legacy_raw = serde_json::json!({
        "owning_scope": locator.owning_scope(),
        "owner_revision_ref": {
            "owner_ref": {
                "owner_plane": GovernedMemoryOwnerPlane::RuntimeSkill,
                "owner_id": locator.owner_id(),
            },
            "owner_revision": locator.owner_revision(),
        },
    });
    assert!(serde_json::from_value::<RuntimeSkillOwnerLocator>(legacy_raw).is_err());

    let mut unknown = canonical;
    unknown
        .as_object_mut()
        .expect("locator object")
        .insert("name".into(), serde_json::json!("legacy-name"));
    assert!(serde_json::from_value::<RuntimeSkillOwnerLocator>(unknown).is_err());
}

fn owner_record(scope: RuntimeSkillOwningScope) -> RuntimeSkillOwnerRecord {
    owner_record_with_intrinsic(scope, intrinsic())
}

fn owner_record_with_intrinsic(
    scope: RuntimeSkillOwningScope,
    intrinsic_contract: RuntimeSkillIntrinsicContract,
) -> RuntimeSkillOwnerRecord {
    RuntimeSkillOwnerRecord::build(
        "space-1",
        scope,
        creation_ref(),
        1,
        intrinsic_contract,
        procedural_content(),
        RuntimeSkillLifecycle::created(100).expect("created lifecycle"),
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("typed runtime skill owner")
}

#[test]
fn required_premise_matrix_and_optional_failure_are_derived_in_core() {
    let subject_scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-1".into(),
    };
    let current = owner_record(subject_scope.clone());
    let cases = [
        (
            vec![RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
                handle_ref: "credential-presence-handle".into(),
                present: true,
            }],
            authority("subject-1"),
            PremiseEvaluationDecision::Satisfied,
            true,
        ),
        (
            vec![RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
                handle_ref: "credential-presence-handle".into(),
                present: false,
            }],
            authority("subject-1"),
            PremiseEvaluationDecision::Unsatisfied,
            false,
        ),
        (
            Vec::new(),
            authority("subject-1"),
            PremiseEvaluationDecision::Unknown,
            false,
        ),
        (
            vec![RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
                handle_ref: "credential-presence-handle".into(),
                present: true,
            }],
            authority("other-subject"),
            PremiseEvaluationDecision::PrivacyBlocked,
            false,
        ),
    ];
    for (observations, authority, expected, selected) in cases {
        let plan = plan_for(&current, observations, authority);
        assert_eq!(plan.premise_report().items[0].decision, expected);
        assert_eq!(plan.selected(), selected);
    }

    let mut expired_intrinsic = intrinsic();
    expired_intrinsic.premises[0].valid_until = Some(100);
    let expired = owner_record_with_intrinsic(subject_scope.clone(), expired_intrinsic);
    let expired_plan = plan_for(
        &expired,
        vec![RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
            handle_ref: "credential-presence-handle".into(),
            present: true,
        }],
        authority("subject-1"),
    );
    assert_eq!(
        expired_plan.premise_report().items[0].decision,
        PremiseEvaluationDecision::Expired
    );
    assert!(!expired_plan.selected());

    let mut optional_intrinsic = intrinsic();
    optional_intrinsic.premises[0].required = false;
    let optional = owner_record_with_intrinsic(subject_scope, optional_intrinsic);
    let optional_plan = plan_for(&optional, Vec::new(), authority("subject-1"));
    assert_eq!(
        optional_plan.premise_report().items[0].decision,
        PremiseEvaluationDecision::Unknown
    );
    assert_eq!(optional_plan.premise_report().required_failure_count, 0);
    assert!(optional_plan.selected());
}

fn owner_record_with_lifecycle(
    previous: &RuntimeSkillOwnerRecord,
    availability: RuntimeSkillAvailability,
    state: RuntimeSkillLifecycleState,
) -> RuntimeSkillOwnerRecord {
    RuntimeSkillOwnerRecord::build(
        &previous.memory_space_id,
        previous.owning_scope.clone(),
        previous.creation_ref.clone(),
        2,
        previous.intrinsic_contract.clone(),
        previous.procedural_content.clone(),
        RuntimeSkillLifecycle {
            availability,
            state,
            lineage: RuntimeSkillLifecycleLineage {
                predecessor: Some(
                    RuntimeSkillOwnerBinding::from_record(previous).expect("predecessor binding"),
                ),
                successor: None,
            },
            observed_at: previous.lifecycle.observed_at,
            updated_at: previous.lifecycle.updated_at + 1,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        previous.privacy_class,
    )
    .expect("valid non-terminal lifecycle revision")
}

#[test]
fn only_active_enabled_runtime_skill_is_selected() {
    let scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-1".into(),
    };
    let active = owner_record_with_intrinsic(
        scope,
        RuntimeSkillIntrinsicContract {
            premises: Vec::new(),
            ..intrinsic()
        },
    );
    assert!(plan_for(&active, Vec::new(), authority("subject-1")).selected());

    let disabled = active
        .revise_availability(RuntimeSkillAvailability::Disabled, 101)
        .expect("disabled revision");
    let stale = owner_record_with_lifecycle(
        &active,
        RuntimeSkillAvailability::Enabled,
        RuntimeSkillLifecycleState::Stale,
    );
    let low_value = owner_record_with_lifecycle(
        &active,
        RuntimeSkillAvailability::Enabled,
        RuntimeSkillLifecycleState::LowValue,
    );
    let retired = active.retire(101).expect("retired revision");
    let successor = RuntimeSkillOwnerRecord::build(
        "space-1",
        active.owning_scope.clone(),
        RuntimeSkillCreationRef::GovernedCandidate {
            candidate_id: "successor-active".into(),
            candidate_digest: digest('f'),
        },
        1,
        active.intrinsic_contract.clone(),
        active.procedural_content.clone(),
        RuntimeSkillLifecycle::created(101).expect("successor lifecycle"),
        active.privacy_class,
    )
    .expect("successor owner");
    let superseded = RuntimeSkillOwnerRecord::build(
        "space-1",
        active.owning_scope.clone(),
        active.creation_ref.clone(),
        2,
        active.intrinsic_contract.clone(),
        active.procedural_content.clone(),
        RuntimeSkillLifecycle {
            availability: RuntimeSkillAvailability::Disabled,
            state: RuntimeSkillLifecycleState::Superseded,
            lineage: RuntimeSkillLifecycleLineage {
                predecessor: Some(
                    RuntimeSkillOwnerBinding::from_record(&active).expect("predecessor binding"),
                ),
                successor: Some(
                    RuntimeSkillOwnerBinding::from_record(&successor).expect("successor binding"),
                ),
            },
            observed_at: active.lifecycle.observed_at,
            updated_at: 102,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        active.privacy_class,
    )
    .expect("superseded revision");
    for blocked in [disabled, stale, low_value, retired, superseded] {
        let plan = plan_for(&blocked, Vec::new(), authority("subject-1"));
        assert!(!plan.selected());
        assert!(plan
            .drop_reasons()
            .contains(&RuntimeSkillDeliveryDropReason::LifecycleBlocked));
    }
}

fn runtime_skill_owner_with_privacy(
    owning_scope: RuntimeSkillOwningScope,
    privacy_class: MemoryPrivacyClass,
) -> RuntimeSkillOwnerRecord {
    let mut intrinsic_contract = intrinsic();
    intrinsic_contract.premises.clear();
    intrinsic_contract.projection_policy.privacy_class = privacy_class;
    RuntimeSkillOwnerRecord::build(
        "space-1",
        owning_scope,
        creation_ref(),
        1,
        intrinsic_contract,
        procedural_content(),
        RuntimeSkillLifecycle::created(100).expect("privacy lifecycle"),
        privacy_class,
    )
    .expect("privacy-scoped owner")
}

#[test]
fn privacy_subject_and_shared_program_matrix_is_exact_zero_outside_authority() {
    for privacy_class in [
        MemoryPrivacyClass::PrivateGarden,
        MemoryPrivacyClass::SoulPrivate,
        MemoryPrivacyClass::OperatorDiagnostic,
    ] {
        for owning_scope in [
            RuntimeSkillOwningScope::Subject {
                mounted_subject_id: "subject-1".into(),
            },
            RuntimeSkillOwningScope::SharedProgram,
        ] {
            let owner = runtime_skill_owner_with_privacy(owning_scope, privacy_class);
            let plan = plan_for(&owner, Vec::new(), authority("subject-1"));
            assert!(!plan.selected());
            assert!(plan
                .drop_reasons()
                .contains(&RuntimeSkillDeliveryDropReason::PrivacyBlocked));
            assert!(build_public_safe_procedural_memory_delivery_report(&plan)
                .expect("safe report decision")
                .is_none());
        }
    }

    let cross_subject = runtime_skill_owner_with_privacy(
        RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "other-subject".into(),
        },
        MemoryPrivacyClass::PublicRuntime,
    );
    let cross_subject_plan = plan_for(&cross_subject, Vec::new(), authority("subject-1"));
    assert!(!cross_subject_plan.selected());
    assert!(
        build_public_safe_procedural_memory_delivery_report(&cross_subject_plan)
            .expect("cross-subject safe report decision")
            .is_none()
    );

    let shared_private = runtime_skill_owner_with_privacy(
        RuntimeSkillOwningScope::SharedProgram,
        MemoryPrivacyClass::SharedWithSubject,
    );
    let shared_private_plan = plan_for(&shared_private, Vec::new(), authority("subject-1"));
    assert!(!shared_private_plan.selected());
    assert!(
        build_public_safe_procedural_memory_delivery_report(&shared_private_plan)
            .expect("shared private safe report decision")
            .is_none()
    );

    for owner in [
        runtime_skill_owner_with_privacy(
            RuntimeSkillOwningScope::Subject {
                mounted_subject_id: "subject-1".into(),
            },
            MemoryPrivacyClass::SharedWithSubject,
        ),
        runtime_skill_owner_with_privacy(
            RuntimeSkillOwningScope::SharedProgram,
            MemoryPrivacyClass::PublicRuntime,
        ),
    ] {
        let plan = plan_for(&owner, Vec::new(), authority("subject-1"));
        assert!(plan.selected());
        let report = build_public_safe_procedural_memory_delivery_report(&plan)
            .expect("allowed safe report")
            .expect("allowed report");
        assert!(report.selected);
        assert!(!report.rendered);
    }
}

#[test]
fn runtime_skill_recall_budgets_accept_exact_ceiling_and_reject_n_plus_one() {
    let scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-1".into(),
    };
    let record = owner_record(scope.clone());
    let manifest = manifest_for(&record);
    let exact_budget =
        RuntimeSkillRecallBudgetAuthority::try_new(1, 1, 1, 1, 1).expect("exact budget");
    let observation = RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
        handle_ref: "credential-presence-handle".into(),
        present: true,
    };
    let build = |owner: &RuntimeSkillOwnerRecord,
                 manifest: &RuntimeSkillScopeManifest,
                 observations: Vec<RuntimeSkillPremiseObservation>,
                 budget: RuntimeSkillRecallBudgetAuthority| {
        build_runtime_skill_recall_plan(
            owner,
            manifest,
            operation_authority_ref(),
            120,
            RuntimeSkillRecallQuery::try_from_text("deployment").expect("query"),
            RuntimeSkillApplicabilityContext::try_new(Vec::new()).expect("context"),
            observations,
            authority("subject-1"),
            budget,
        )
    };
    assert!(build(&record, &manifest, vec![observation.clone()], exact_budget).is_ok());

    let mut two_premises = intrinsic();
    two_premises.premises.push(RuntimeSkillPremiseRequirement {
        premise: RuntimeSkillPremise::OpaquePresenceAttestation {
            handle_ref: "second-presence-handle".into(),
        },
        required: true,
        valid_from: 10,
        valid_until: None,
        privacy_class: MemoryPrivacyClass::SharedWithSubject,
        governed_evidence_refs: Vec::new(),
    });
    let two_premise_owner = owner_record_with_intrinsic(scope.clone(), two_premises);
    assert!(build(
        &two_premise_owner,
        &manifest_for(&two_premise_owner),
        vec![observation.clone()],
        exact_budget
    )
    .is_err());
    assert!(build(
        &record,
        &manifest,
        vec![
            observation,
            RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
                handle_ref: "irrelevant-presence-handle".into(),
                present: true,
            },
        ],
        exact_budget
    )
    .is_err());

    let revised = record
        .revise_availability(RuntimeSkillAvailability::Enabled, 101)
        .expect("revision two");
    assert!(build(
        &revised,
        &manifest_for(&revised),
        vec![RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
            handle_ref: "credential-presence-handle".into(),
            present: true,
        }],
        exact_budget
    )
    .is_err());

    let second = RuntimeSkillOwnerRecord::build(
        "space-1",
        scope.clone(),
        RuntimeSkillCreationRef::GovernedCandidate {
            candidate_id: "candidate-two".into(),
            candidate_digest: digest('a'),
        },
        1,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle::created(100).expect("second lifecycle"),
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("second owner");
    let two_owner_manifest = RuntimeSkillScopeManifest::build(
        1,
        "space-1",
        scope,
        [
            RuntimeSkillOwnerBinding::from_record(&record).expect("first binding"),
            RuntimeSkillOwnerBinding::from_record(&second).expect("second binding"),
        ],
        2,
    )
    .expect("two-owner manifest");
    assert!(build(
        &record,
        &two_owner_manifest,
        vec![RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
            handle_ref: "credential-presence-handle".into(),
            present: true,
        }],
        RuntimeSkillRecallBudgetAuthority::try_new(2, 1, 1, 1, 1).expect("two retained owners")
    )
    .is_err());
}

#[test]
fn task_evidence_requires_typed_source_and_source_match() {
    let missing_source = serde_json::json!({
        "kind": "task_evidence",
        "evidence_kind": "task_run",
        "safe_ref": "run-1"
    });
    assert!(serde_json::from_value::<RuntimeSkillPremise>(missing_source).is_err());

    let mut task_intrinsic = intrinsic();
    task_intrinsic.premises = vec![RuntimeSkillPremiseRequirement {
        premise: RuntimeSkillPremise::TaskEvidence {
            source: PremiseTypedSource::TaskRun,
            evidence_kind: RuntimeSkillEvidenceKind::TaskRun,
            safe_ref: "run-1".into(),
        },
        required: true,
        valid_from: 10,
        valid_until: None,
        privacy_class: MemoryPrivacyClass::SharedWithSubject,
        governed_evidence_refs: Vec::new(),
    }];
    let record = owner_record_with_intrinsic(
        RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-1".into(),
        },
        task_intrinsic,
    );
    let plan = plan_for(
        &record,
        vec![RuntimeSkillPremiseObservation::TaskEvidence {
            source: PremiseTypedSource::TaskRun,
            evidence_kind: RuntimeSkillEvidenceKind::TaskRun,
            safe_ref: "run-1".into(),
            present: true,
        }],
        authority("subject-1"),
    );
    assert_eq!(
        plan.premise_report().items[0].decision,
        PremiseEvaluationDecision::Satisfied
    );
    assert!(plan.selected());

    let esp_authority =
        RuntimeSkillRecallAuthority::try_new(true, false, false, Some("subject-1".into()))
            .expect("ESP shallow premise authority");
    let esp_plan = plan_for(&record, Vec::new(), esp_authority.clone());
    assert_eq!(
        esp_plan.premise_report().items[0].decision,
        PremiseEvaluationDecision::Unknown
    );
    assert!(!esp_plan.selected());
    assert!(build_runtime_skill_recall_plan(
        &record,
        &manifest_for(&record),
        operation_authority_ref(),
        120,
        RuntimeSkillRecallQuery::try_from_text("deployment").expect("query"),
        RuntimeSkillApplicabilityContext::try_new(Vec::new()).expect("context"),
        vec![RuntimeSkillPremiseObservation::TaskEvidence {
            source: PremiseTypedSource::TaskRun,
            evidence_kind: RuntimeSkillEvidenceKind::TaskRun,
            safe_ref: "run-1".into(),
            present: true,
        }],
        esp_authority,
        budget(),
    )
    .is_err());
}

#[test]
fn governed_evidence_bindings_are_evaluated_from_typed_observations() {
    let evidence_ref = GovernedOwnerRevisionRef {
        owner_ref: GovernedMemoryOwnerRef::new(
            GovernedMemoryOwnerPlane::EvidenceDocument,
            "environment-1",
        ),
        owner_revision: 2,
    };
    let mut evidence_intrinsic = intrinsic();
    evidence_intrinsic.premises[0].governed_evidence_refs = vec![evidence_ref.clone()];
    let record = owner_record_with_intrinsic(
        RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-1".into(),
        },
        evidence_intrinsic,
    );
    let base_observation = RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
        handle_ref: "credential-presence-handle".into(),
        present: true,
    };

    let unknown = plan_for(
        &record,
        vec![base_observation.clone()],
        authority("subject-1"),
    );
    assert_eq!(
        unknown.premise_report().items[0].decision,
        PremiseEvaluationDecision::Unknown
    );
    assert!(!unknown.selected());

    let unsatisfied = plan_for(
        &record,
        vec![
            base_observation.clone(),
            RuntimeSkillPremiseObservation::GovernedEnvironmentEvidence {
                evidence_revision_ref: evidence_ref.clone(),
                present: false,
            },
        ],
        authority("subject-1"),
    );
    assert_eq!(
        unsatisfied.premise_report().items[0].decision,
        PremiseEvaluationDecision::Unsatisfied
    );
    assert!(!unsatisfied.selected());

    let satisfied = plan_for(
        &record,
        vec![
            base_observation,
            RuntimeSkillPremiseObservation::GovernedEnvironmentEvidence {
                evidence_revision_ref: evidence_ref,
                present: true,
            },
        ],
        authority("subject-1"),
    );
    assert_eq!(
        satisfied.premise_report().items[0].decision,
        PremiseEvaluationDecision::Satisfied
    );
    assert!(satisfied.selected());
}

#[test]
fn procedural_delivery_report_is_builder_owned_safe_and_tamper_evident() {
    let record = owner_record(RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-1".into(),
    });
    let plan = plan_for(
        &record,
        vec![RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
            handle_ref: "credential-presence-handle".into(),
            present: true,
        }],
        authority("subject-1"),
    );
    let report = build_procedural_memory_delivery_report(&plan).expect("canonical delivery report");
    assert!(report.validate_contract(&plan).accepted);
    assert!(report.selected);
    assert!(!report.rendered);
    assert_eq!(report.safe_evidence_refs.len(), 1);
    let encoded = serde_json::to_value(&report).expect("serialize delivery report");
    for forbidden in [
        "procedure",
        "body",
        "content_digest",
        "materialized_view_ref",
        "immutable_snapshot_receipt",
    ] {
        assert!(!encoded
            .as_object()
            .expect("delivery report object")
            .contains_key(forbidden));
    }

    let mut decision_drift = report.clone();
    decision_drift.premise_report.items[0].decision = PremiseEvaluationDecision::Unknown;
    assert!(!decision_drift.validate_contract(&plan).accepted);

    let mut owner_drift = report.clone();
    owner_drift.owner_revision_ref.owner_revision += 1;
    assert!(!owner_drift.validate_contract(&plan).accepted);

    let mut unknown_field = encoded;
    unknown_field
        .as_object_mut()
        .expect("delivery report object")
        .insert("caller_selected".into(), serde_json::json!(true));
    assert!(serde_json::from_value::<ProceduralMemoryDeliveryReport>(unknown_field).is_err());
}

#[test]
fn runtime_skill_projection_material_and_actual_receipt_finalize_delivery_in_core() {
    let record = owner_record(RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-1".into(),
    });
    let manifest = manifest_for(&record);
    let plan = plan_for(
        &record,
        vec![RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
            handle_ref: "credential-presence-handle".into(),
            present: true,
        }],
        authority("subject-1"),
    );
    let initial =
        build_procedural_memory_delivery_report(&plan).expect("initial canonical delivery report");
    assert!(initial.selected);
    assert!(!initial.rendered);

    let material = build_runtime_skill_projection_material(&record, &manifest, &plan)
        .expect("canonical projection material")
        .expect("selected plan material");
    assert!(material
        .candidate_ref()
        .starts_with("runtime_skill_projection_candidate:sha256:"));
    assert_eq!(material.provider_content(), &record.procedural_content);
    assert_eq!(material.content_digest(), record.content_digest);

    let rendered_receipt = RuntimeSkillProjectionRenderReceipt::try_rendered(
        material.candidate_ref(),
        material.content_digest(),
    )
    .expect("typed rendered receipt");
    let rendered =
        finalize_procedural_memory_delivery_report(&plan, Some(&material), &rendered_receipt)
            .expect("canonical rendered delivery report");
    assert!(rendered.selected);
    assert!(rendered.rendered);
    assert!(rendered.drop_reasons.is_empty());
    assert!(
        rendered
            .validate_finalized_contract(&plan, Some(&material), &rendered_receipt)
            .accepted
    );

    let forged_candidate = RuntimeSkillProjectionRenderReceipt::try_rendered(
        format!(
            "runtime_skill_projection_candidate:sha256:{}",
            "f".repeat(64)
        ),
        material.content_digest(),
    )
    .expect("shape-valid forged candidate receipt");
    assert!(
        finalize_procedural_memory_delivery_report(&plan, Some(&material), &forged_candidate)
            .is_err()
    );

    let forged_digest =
        RuntimeSkillProjectionRenderReceipt::try_rendered(material.candidate_ref(), digest('f'))
            .expect("shape-valid forged digest receipt");
    assert!(
        finalize_procedural_memory_delivery_report(&plan, Some(&material), &forged_digest).is_err()
    );

    let dropped_receipt = RuntimeSkillProjectionRenderReceipt::try_dropped_budget(
        material.candidate_ref(),
        material.content_digest(),
    )
    .expect("typed budget-drop receipt");
    let dropped =
        finalize_procedural_memory_delivery_report(&plan, Some(&material), &dropped_receipt)
            .expect("canonical budget-drop delivery report");
    let mut expected_dropped = initial;
    expected_dropped
        .drop_reasons
        .push(RuntimeSkillDeliveryDropReason::RenderBudgetExceeded);
    assert_eq!(dropped, expected_dropped);
    assert!(!dropped.rendered);
}

#[test]
fn privacy_blocked_runtime_skill_has_no_projection_material() {
    let record = runtime_skill_owner_with_privacy(
        RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-1".into(),
        },
        MemoryPrivacyClass::PrivateGarden,
    );
    let manifest = manifest_for(&record);
    let plan = plan_for(&record, Vec::new(), authority("subject-1"));
    assert!(!plan.selected());
    assert!(plan
        .drop_reasons()
        .contains(&RuntimeSkillDeliveryDropReason::PrivacyBlocked));
    assert!(
        build_runtime_skill_projection_material(&record, &manifest, &plan)
            .expect("privacy-blocked material decision")
            .is_none()
    );
    let finalized = finalize_procedural_memory_delivery_report(
        &plan,
        None,
        &RuntimeSkillProjectionRenderReceipt::not_requested(),
    )
    .expect("privacy-blocked canonical finalization");
    assert!(!finalized.selected);
    assert!(!finalized.rendered);
}

#[test]
fn runtime_skill_recall_plan_is_opaque_canonical_and_exact_owner_bound() {
    let record = owner_record(RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-1".into(),
    });
    let manifest = manifest_for(&record);
    let plan = plan_for(
        &record,
        vec![RuntimeSkillPremiseObservation::OpaquePresenceAttestation {
            handle_ref: "credential-presence-handle".into(),
            present: true,
        }],
        authority("subject-1"),
    );
    assert!(plan.validate_for(&record, &manifest).accepted);
    assert!(plan.matched());
    assert!(plan.selected());

    let other = RuntimeSkillOwnerRecord::build(
        "space-1",
        record.owning_scope.clone(),
        RuntimeSkillCreationRef::GovernedCandidate {
            candidate_id: "different-owner".into(),
            candidate_digest: digest('f'),
        },
        1,
        record.intrinsic_contract.clone(),
        record.procedural_content.clone(),
        RuntimeSkillLifecycle::created(100).expect("other lifecycle"),
        record.privacy_class,
    )
    .expect("different exact owner");
    assert!(!plan.validate_for(&other, &manifest_for(&other)).accepted);
}

#[test]
fn runtime_skill_owner_id_is_derived_from_creation_ref_and_physical_owning_scope() {
    let creation_ref = RuntimeSkillCreationRef::GovernedCandidate {
        candidate_id: "candidate-1".into(),
        candidate_digest: digest('a'),
    };
    let subject_a = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-a".into(),
    };
    let subject_b = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-b".into(),
    };

    let first = canonical_runtime_skill_owner_id("space-1", &subject_a, &creation_ref)
        .expect("canonical runtime skill owner id");
    let repeated = canonical_runtime_skill_owner_id("space-1", &subject_a, &creation_ref)
        .expect("same creation identity remains stable");
    let other_subject = canonical_runtime_skill_owner_id("space-1", &subject_b, &creation_ref)
        .expect("other subject owner id");
    let shared_program = canonical_runtime_skill_owner_id(
        "space-1",
        &RuntimeSkillOwningScope::SharedProgram,
        &creation_ref,
    )
    .expect("shared program owner id");
    let other_space = canonical_runtime_skill_owner_id("space-2", &subject_a, &creation_ref)
        .expect("other memory space owner id");

    assert_eq!(first, repeated);
    assert!(first.starts_with("runtime_skill:sha256:"));
    assert_ne!(first, other_subject);
    assert_ne!(first, shared_program);
    assert_ne!(first, other_space);

    let invalid_creation_ref = RuntimeSkillCreationRef::GovernedCandidate {
        candidate_id: "candidate-1".into(),
        candidate_digest: "not-a-sha256".into(),
    };
    assert!(
        canonical_runtime_skill_owner_id("space-1", &subject_a, &invalid_creation_ref).is_err()
    );
}

#[test]
fn runtime_skill_applicability_is_a_canonical_all_of_not_a_storage_scope() {
    let project = RuntimeSkillApplicabilityTarget::Project {
        project_id: "project-a".into(),
    };
    let device = RuntimeSkillApplicabilityTarget::Device {
        device_id: "device-a".into(),
    };
    let forward = RuntimeSkillApplicability::try_all_of(vec![project.clone(), device.clone()])
        .expect("canonical all-of applicability");
    let reversed = RuntimeSkillApplicability::try_all_of(vec![device.clone(), project.clone()])
        .expect("input order must not change applicability identity");

    assert_eq!(forward, reversed);
    assert_eq!(forward.required_targets().len(), 2);
    assert!(RuntimeSkillApplicability::try_all_of(Vec::new()).is_err());
    assert!(RuntimeSkillApplicability::try_all_of(vec![project.clone(), project]).is_err());

    let creation_ref = RuntimeSkillCreationRef::TaskLearningPromotion {
        learning_id: "learning-1".into(),
        learning_digest: digest('b'),
    };
    let subject_scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-a".into(),
    };
    let owner_before = canonical_runtime_skill_owner_id("space-1", &subject_scope, &creation_ref)
        .expect("owner before applicability change");
    let _global = RuntimeSkillApplicability::Global;
    let owner_after = canonical_runtime_skill_owner_id("space-1", &subject_scope, &creation_ref)
        .expect("applicability is not part of storage identity");
    assert_eq!(owner_before, owner_after);
}

#[test]
fn runtime_skill_intrinsic_contract_excludes_operation_scoped_recall_state() {
    let intrinsic = RuntimeSkillIntrinsicContract {
        schema_version: RUNTIME_SKILL_GOVERNED_CONTRACT_SCHEMA_VERSION,
        applicability: RuntimeSkillApplicability::Global,
        triggers: Vec::new(),
        constraints: Vec::new(),
        premises: vec![RuntimeSkillPremiseRequirement {
            premise: RuntimeSkillPremise::OpaquePresenceAttestation {
                handle_ref: "credential-presence-handle".into(),
            },
            required: true,
            valid_from: 10,
            valid_until: None,
            privacy_class: MemoryPrivacyClass::SharedWithSubject,
            governed_evidence_refs: Vec::new(),
        }],
        failure_modes: vec![RuntimeSkillFailureMode::RequiredPremiseUnsatisfied],
        evidence_bindings: Vec::new(),
        projection_policy: RuntimeSkillProjectionPolicy {
            privacy_class: MemoryPrivacyClass::SharedWithSubject,
            model_projection_allowed: true,
            require_all_mandatory_premises: true,
        },
        capability_affinities: vec![RuntimeSkillCapabilityAffinity::ProceduralRecall],
    };
    assert!(intrinsic.validate_contract().accepted);

    let encoded = serde_json::to_value(&intrinsic).expect("serialize intrinsic contract");
    let object = encoded.as_object().expect("intrinsic contract JSON object");
    for forbidden in [
        "immutable_snapshot_receipt",
        "candidate_owner_revisions",
        "premise_decisions",
        "max_procedural_candidates",
        "max_premises_per_skill",
        "max_premise_evidence_reads",
    ] {
        assert!(
            !object.contains_key(forbidden),
            "forbidden field {forbidden}"
        );
    }

    let mut forged = encoded;
    forged
        .as_object_mut()
        .expect("intrinsic contract JSON object")
        .insert(
            "immutable_snapshot_receipt".into(),
            serde_json::Value::String("stale-operation-receipt".into()),
        );
    assert!(serde_json::from_value::<RuntimeSkillIntrinsicContract>(forged).is_err());
}

#[test]
fn runtime_skill_owner_record_closes_identity_key_content_and_privacy() {
    let subject_scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-a".into(),
    };
    let record = owner_record(subject_scope.clone());

    assert!(record.validate_contract().accepted);
    assert_eq!(
        record.physical_key,
        canonical_runtime_skill_owner_key("space-1", &subject_scope, &record.owner_ref.owner_id)
            .expect("canonical physical owner key")
    );
    assert_eq!(
        record.owner_ref.owner_id,
        canonical_runtime_skill_owner_id("space-1", &subject_scope, &creation_ref())
            .expect("canonical owner id")
    );
    assert_eq!(
        record.content_digest,
        record
            .canonical_content_digest()
            .expect("canonical content digest")
    );

    let mut digest_drift = record.clone();
    digest_drift
        .procedural_content
        .procedure
        .push_str(" changed");
    assert!(!digest_drift.validate_contract().accepted);

    let mut key_drift = record.clone();
    key_drift.physical_key = canonical_runtime_skill_owner_key(
        "space-1",
        &RuntimeSkillOwningScope::SharedProgram,
        &key_drift.owner_ref.owner_id,
    )
    .expect("other scope key");
    assert!(!key_drift.validate_contract().accepted);

    let mut privacy_drift = record;
    privacy_drift.privacy_class = MemoryPrivacyClass::PublicRuntime;
    assert!(!privacy_drift.validate_contract().accepted);

    let mut lifecycle_digest_drift = owner_record(RuntimeSkillOwningScope::SharedProgram);
    lifecycle_digest_drift.lifecycle.availability = RuntimeSkillAvailability::Disabled;
    assert!(lifecycle_digest_drift
        .validate_contract()
        .failures
        .contains(&bm_core::skills::RuntimeSkillOwnerContractFailure::ContentDigestMismatch));
    lifecycle_digest_drift.content_digest = lifecycle_digest_drift
        .canonical_content_digest()
        .expect("lifecycle-bound digest");
    assert!(lifecycle_digest_drift.validate_contract().accepted);
}

#[test]
fn runtime_skill_owner_record_rejects_operation_state_and_unknown_fields() {
    let record = owner_record(RuntimeSkillOwningScope::SharedProgram);
    let mut encoded = serde_json::to_value(&record).expect("serialize typed owner");
    let object = encoded.as_object().expect("typed owner object");
    for forbidden in [
        "agent_id",
        "immutable_snapshot_receipt",
        "candidate_owner_revisions",
        "premise_decisions",
        "max_procedural_candidates",
    ] {
        assert!(
            !object.contains_key(forbidden),
            "forbidden field {forbidden}"
        );
    }

    encoded.as_object_mut().expect("typed owner object").insert(
        "agent_id".into(),
        serde_json::Value::String("legacy-agent".into()),
    );
    assert!(serde_json::from_value::<RuntimeSkillOwnerRecord>(encoded).is_err());
}

#[test]
fn runtime_skill_physical_keys_bind_memory_space_and_physical_scope_only() {
    let owner_id = canonical_runtime_skill_owner_id(
        "space-1",
        &RuntimeSkillOwningScope::SharedProgram,
        &creation_ref(),
    )
    .expect("owner id");
    let shared = canonical_runtime_skill_owner_key(
        "space-1",
        &RuntimeSkillOwningScope::SharedProgram,
        &owner_id,
    )
    .expect("shared key");
    let subject = canonical_runtime_skill_owner_key(
        "space-1",
        &RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-a".into(),
        },
        &owner_id,
    )
    .expect("subject key");
    let other_space = canonical_runtime_skill_owner_key(
        "space-2",
        &RuntimeSkillOwningScope::SharedProgram,
        &owner_id,
    )
    .expect("other space key");

    assert_ne!(shared, subject);
    assert_ne!(shared, other_space);
    assert!(!shared.contains("agent"));
}

#[test]
fn runtime_skill_scope_manifest_is_bounded_canonical_and_exact() {
    let scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-a".into(),
    };
    let first = owner_record(scope.clone());
    let mut second = RuntimeSkillOwnerRecord::build(
        "space-1",
        scope.clone(),
        RuntimeSkillCreationRef::GovernedCandidate {
            candidate_id: "candidate-2".into(),
            candidate_digest: digest('c'),
        },
        1,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle::created(100).expect("created lifecycle"),
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("second owner");
    second.procedural_content.topic = "release".into();
    second.content_digest = second
        .canonical_content_digest()
        .expect("second canonical digest");

    let manifest = RuntimeSkillScopeManifest::build(
        1,
        "space-1",
        scope.clone(),
        [
            RuntimeSkillOwnerBinding::from_record(&second).expect("second binding"),
            RuntimeSkillOwnerBinding::from_record(&first).expect("first binding"),
        ],
        2,
    )
    .expect("scope manifest");
    assert_eq!(
        manifest.physical_key,
        runtime_skill_scope_manifest_key("space-1", &scope).expect("manifest key")
    );
    assert_eq!(manifest.owner_count, 2);
    assert!(manifest
        .validate_exact(
            "space-1",
            &scope,
            [
                RuntimeSkillOwnerBinding::from_record(&first).expect("first binding"),
                RuntimeSkillOwnerBinding::from_record(&second).expect("second binding"),
            ],
            2,
        )
        .is_ok());
    let encoded = serde_json::to_value(&manifest).expect("serialize scope manifest");
    assert!(!encoded
        .as_object()
        .expect("scope manifest object")
        .contains_key("agent_id"));
    let mut forged = encoded;
    forged
        .as_object_mut()
        .expect("scope manifest object")
        .insert(
            "agent_id".into(),
            serde_json::Value::String("legacy-agent".into()),
        );
    assert!(serde_json::from_value::<RuntimeSkillScopeManifest>(forged).is_err());

    assert!(RuntimeSkillScopeManifest::build(
        1,
        "space-1",
        scope.clone(),
        [RuntimeSkillOwnerBinding::from_record(&first).expect("binding")],
        0,
    )
    .is_err());
    assert!(RuntimeSkillScopeManifest::build(
        1,
        "space-1",
        RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-a".into(),
        },
        [
            RuntimeSkillOwnerBinding::from_record(&first).expect("first binding"),
            RuntimeSkillOwnerBinding::from_record(&second).expect("second binding"),
        ],
        1,
    )
    .is_err());
    assert!(RuntimeSkillScopeManifest::build(
        1,
        "space-1",
        scope,
        [
            RuntimeSkillOwnerBinding::from_record(&first).expect("binding"),
            RuntimeSkillOwnerBinding::from_record(&first).expect("duplicate binding"),
        ],
        2,
    )
    .is_err());
}

#[test]
fn runtime_skill_lifecycle_closes_availability_state_lineage_time_and_usage() {
    let scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-a".into(),
    };
    let created = owner_record(scope.clone());
    assert_eq!(
        created.lifecycle.availability,
        RuntimeSkillAvailability::Enabled
    );
    assert_eq!(created.lifecycle.state, RuntimeSkillLifecycleState::Active);
    assert_eq!(created.lifecycle.observed_at, 100);
    assert_eq!(created.lifecycle.updated_at, 100);
    assert_eq!(
        created.lifecycle.usage_outcome,
        RuntimeSkillUsageOutcomeSummary::default()
    );

    let revised_lifecycle = RuntimeSkillLifecycle {
        availability: RuntimeSkillAvailability::Enabled,
        state: RuntimeSkillLifecycleState::Active,
        lineage: RuntimeSkillLifecycleLineage {
            predecessor: Some(
                RuntimeSkillOwnerBinding::from_record(&created).expect("predecessor binding"),
            ),
            successor: None,
        },
        observed_at: 100,
        updated_at: 120,
        usage_outcome: RuntimeSkillUsageOutcomeSummary {
            observation_count: 1,
            succeeded_count: 1,
            mismatch_count: 0,
            last_outcome: Some(RuntimeSkillUsageOutcome::Succeeded),
            last_outcome_at: Some(115),
        },
    };
    let revised = RuntimeSkillOwnerRecord::build(
        "space-1",
        scope.clone(),
        creation_ref(),
        2,
        intrinsic(),
        procedural_content(),
        revised_lifecycle,
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("strictly linked owner revision");
    assert!(revised.validate_contract().accepted);
    assert_eq!(revised.owner_ref, created.owner_ref);
    assert_ne!(revised.content_digest, created.content_digest);

    let mut predecessor_drift = revised.clone();
    predecessor_drift
        .lifecycle
        .lineage
        .predecessor
        .as_mut()
        .expect("predecessor")
        .owner_revision = 9;
    predecessor_drift.content_digest = predecessor_drift
        .canonical_content_digest()
        .expect("digest with forged predecessor");
    assert!(!predecessor_drift.validate_contract().accepted);

    let mut invalid_usage = revised.clone();
    invalid_usage.lifecycle.usage_outcome.observation_count = 0;
    invalid_usage.content_digest = invalid_usage
        .canonical_content_digest()
        .expect("digest with invalid usage");
    assert!(!invalid_usage.validate_contract().accepted);

    let successor = RuntimeSkillOwnerRecord::build(
        "space-1",
        scope.clone(),
        RuntimeSkillCreationRef::GovernedCandidate {
            candidate_id: "successor-candidate".into(),
            candidate_digest: digest('d'),
        },
        1,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle::created(125).expect("successor lifecycle"),
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("successor owner");
    let superseded = RuntimeSkillOwnerRecord::build(
        "space-1",
        scope,
        creation_ref(),
        2,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle {
            availability: RuntimeSkillAvailability::Disabled,
            state: RuntimeSkillLifecycleState::Superseded,
            lineage: RuntimeSkillLifecycleLineage {
                predecessor: Some(
                    RuntimeSkillOwnerBinding::from_record(&created).expect("predecessor binding"),
                ),
                successor: Some(
                    RuntimeSkillOwnerBinding::from_record(&successor).expect("successor binding"),
                ),
            },
            observed_at: 100,
            updated_at: 130,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("superseded owner has an exact successor");
    assert!(superseded.validate_contract().accepted);

    let mut later_successor_binding =
        RuntimeSkillOwnerBinding::from_record(&successor).expect("successor binding");
    later_successor_binding.owner_revision = 2;
    assert!(RuntimeSkillOwnerRecord::build(
        "space-1",
        RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-a".into(),
        },
        creation_ref(),
        2,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle {
            availability: RuntimeSkillAvailability::Disabled,
            state: RuntimeSkillLifecycleState::Superseded,
            lineage: RuntimeSkillLifecycleLineage {
                predecessor: Some(
                    RuntimeSkillOwnerBinding::from_record(&created).expect("predecessor binding"),
                ),
                successor: Some(later_successor_binding),
            },
            observed_at: 100,
            updated_at: 130,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        MemoryPrivacyClass::SharedWithSubject,
    )
    .is_err());
}

#[test]
fn runtime_skill_management_transitions_are_append_only_and_terminal() {
    let created = owner_record(RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-a".into(),
    });
    let mut revised_content = created.procedural_content.clone();
    revised_content.summary = "Use an exact governed release sequence.".into();
    let revised = created
        .revise_procedural_content(revised_content, created.lifecycle.updated_at)
        .expect("same-second content revision");
    assert_eq!(revised.owner_ref, created.owner_ref);
    assert_eq!(revised.owner_revision, 2);
    assert_eq!(
        revised.lifecycle.updated_at,
        created.lifecycle.updated_at + 1
    );
    assert_eq!(
        revised.lifecycle.lineage.predecessor,
        Some(RuntimeSkillOwnerBinding::from_record(&created).expect("predecessor"))
    );

    let disabled = revised
        .revise_availability(
            RuntimeSkillAvailability::Disabled,
            revised.lifecycle.updated_at,
        )
        .expect("same-second availability revision");
    assert_eq!(disabled.owner_revision, 3);
    assert_eq!(
        disabled.lifecycle.availability,
        RuntimeSkillAvailability::Disabled
    );

    let retired = disabled
        .retire(disabled.lifecycle.updated_at)
        .expect("retire revision");
    assert_eq!(retired.owner_revision, 4);
    assert_eq!(retired.lifecycle.state, RuntimeSkillLifecycleState::Retired);
    assert!(retired
        .revise_availability(RuntimeSkillAvailability::Enabled, 200)
        .is_err());
    assert!(retired
        .revise_procedural_content(retired.procedural_content.clone(), 200)
        .is_err());
}

#[test]
fn runtime_skill_management_revision_and_timestamp_overflow_fail_closed() {
    let scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-a".into(),
    };
    let first_revision = RuntimeSkillOwnerRecord::build(
        "space-1",
        scope.clone(),
        creation_ref(),
        1,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle::created(100).expect("created lifecycle"),
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("first owner revision");
    let exhausted_revision_lifecycle = RuntimeSkillLifecycle {
        availability: RuntimeSkillAvailability::Enabled,
        state: RuntimeSkillLifecycleState::Active,
        lineage: RuntimeSkillLifecycleLineage {
            predecessor: Some(RuntimeSkillOwnerBinding {
                owner_ref: first_revision.owner_ref.clone(),
                owner_revision: u64::MAX - 1,
                owner_physical_key: first_revision.physical_key.clone(),
                privacy_class: first_revision.privacy_class,
                content_digest: digest('f'),
            }),
            successor: None,
        },
        observed_at: 100,
        updated_at: 101,
        usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
    };
    let exhausted_revision = RuntimeSkillOwnerRecord::build(
        "space-1",
        scope.clone(),
        creation_ref(),
        u64::MAX,
        intrinsic(),
        procedural_content(),
        exhausted_revision_lifecycle,
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("canonical max-revision owner");
    let revision_error = exhausted_revision
        .revise_procedural_content(exhausted_revision.procedural_content.clone(), 101)
        .expect_err("owner revision exhaustion must fail");
    assert_eq!(revision_error.stage(), "runtime_skill_owner_advance");

    let exhausted_timestamp = RuntimeSkillOwnerRecord::build(
        "space-1",
        scope,
        creation_ref(),
        1,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle::created(u64::MAX).expect("max timestamp lifecycle"),
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("canonical max-timestamp owner");
    let timestamp_error = exhausted_timestamp
        .revise_availability(RuntimeSkillAvailability::Disabled, u64::MAX)
        .expect_err("lifecycle timestamp exhaustion must fail");
    assert_eq!(timestamp_error.stage(), "runtime_skill_owner_advance");
}

#[test]
fn runtime_skill_lifecycle_rejects_impossible_state_and_nested_unknown_fields() {
    let scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-a".into(),
    };
    let created = owner_record(scope.clone());

    assert!(RuntimeSkillOwnerRecord::build(
        "space-1",
        scope.clone(),
        creation_ref(),
        2,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle {
            availability: RuntimeSkillAvailability::Enabled,
            state: RuntimeSkillLifecycleState::Retired,
            lineage: RuntimeSkillLifecycleLineage {
                predecessor: Some(
                    RuntimeSkillOwnerBinding::from_record(&created).expect("predecessor binding"),
                ),
                successor: None,
            },
            observed_at: 100,
            updated_at: 120,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        MemoryPrivacyClass::SharedWithSubject,
    )
    .is_err());

    assert!(RuntimeSkillOwnerRecord::build(
        "space-1",
        scope,
        creation_ref(),
        2,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle {
            availability: RuntimeSkillAvailability::Disabled,
            state: RuntimeSkillLifecycleState::Superseded,
            lineage: RuntimeSkillLifecycleLineage {
                predecessor: Some(
                    RuntimeSkillOwnerBinding::from_record(&created).expect("predecessor binding"),
                ),
                successor: None,
            },
            observed_at: 100,
            updated_at: 120,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        MemoryPrivacyClass::SharedWithSubject,
    )
    .is_err());

    let mut encoded = serde_json::to_value(&created).expect("serialize owner");
    encoded
        .get_mut("lifecycle")
        .and_then(serde_json::Value::as_object_mut)
        .expect("lifecycle object")
        .insert(
            "legacy_status".into(),
            serde_json::Value::String("active".into()),
        );
    assert!(serde_json::from_value::<RuntimeSkillOwnerRecord>(encoded).is_err());

    let mut nested_usage = serde_json::to_value(&created).expect("serialize owner");
    nested_usage
        .get_mut("lifecycle")
        .and_then(|value| value.get_mut("usage_outcome"))
        .and_then(serde_json::Value::as_object_mut)
        .expect("usage outcome object")
        .insert("raw_outcomes".into(), serde_json::json!(["success"]));
    assert!(serde_json::from_value::<RuntimeSkillOwnerRecord>(nested_usage).is_err());

    let cross_scope_successor = owner_record(RuntimeSkillOwningScope::SharedProgram);
    assert!(RuntimeSkillOwnerRecord::build(
        "space-1",
        RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-a".into(),
        },
        creation_ref(),
        2,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle {
            availability: RuntimeSkillAvailability::Disabled,
            state: RuntimeSkillLifecycleState::Superseded,
            lineage: RuntimeSkillLifecycleLineage {
                predecessor: Some(
                    RuntimeSkillOwnerBinding::from_record(&created).expect("predecessor binding"),
                ),
                successor: Some(
                    RuntimeSkillOwnerBinding::from_record(&cross_scope_successor)
                        .expect("cross-scope successor binding"),
                ),
            },
            observed_at: 100,
            updated_at: 120,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        MemoryPrivacyClass::SharedWithSubject,
    )
    .is_err());

    assert!(RuntimeSkillOwnerRecord::build(
        "space-1",
        RuntimeSkillOwningScope::SharedProgram,
        creation_ref(),
        1,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle {
            availability: RuntimeSkillAvailability::Disabled,
            state: RuntimeSkillLifecycleState::Retired,
            lineage: RuntimeSkillLifecycleLineage::default(),
            observed_at: 100,
            updated_at: 100,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        MemoryPrivacyClass::SharedWithSubject,
    )
    .is_err());
}

#[test]
fn runtime_skill_supersede_rejects_cross_privacy_successor() {
    let scope = RuntimeSkillOwningScope::Subject {
        mounted_subject_id: "subject-a".into(),
    };
    let created = owner_record(scope.clone());
    let successor = RuntimeSkillOwnerRecord::build(
        "space-1",
        scope.clone(),
        RuntimeSkillCreationRef::GovernedCandidate {
            candidate_id: "replacement-candidate".into(),
            candidate_digest: digest('c'),
        },
        1,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle::created(110).expect("successor lifecycle"),
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("successor owner");
    let mut cross_privacy =
        RuntimeSkillOwnerBinding::from_record(&successor).expect("successor binding");
    cross_privacy.privacy_class = MemoryPrivacyClass::PublicRuntime;

    assert!(RuntimeSkillOwnerRecord::build(
        "space-1",
        scope,
        creation_ref(),
        2,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle {
            availability: RuntimeSkillAvailability::Disabled,
            state: RuntimeSkillLifecycleState::Superseded,
            lineage: RuntimeSkillLifecycleLineage {
                predecessor: Some(
                    RuntimeSkillOwnerBinding::from_record(&created).expect("predecessor binding"),
                ),
                successor: Some(cross_privacy),
            },
            observed_at: 100,
            updated_at: 120,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        MemoryPrivacyClass::SharedWithSubject,
    )
    .is_err());

    let mut cross_privacy_predecessor =
        RuntimeSkillOwnerBinding::from_record(&created).expect("predecessor binding");
    cross_privacy_predecessor.privacy_class = MemoryPrivacyClass::PublicRuntime;
    assert!(RuntimeSkillOwnerRecord::build(
        "space-1",
        RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-a".into(),
        },
        creation_ref(),
        2,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle {
            availability: RuntimeSkillAvailability::Disabled,
            state: RuntimeSkillLifecycleState::Superseded,
            lineage: RuntimeSkillLifecycleLineage {
                predecessor: Some(cross_privacy_predecessor),
                successor: Some(
                    RuntimeSkillOwnerBinding::from_record(&successor).expect("successor binding"),
                ),
            },
            observed_at: 100,
            updated_at: 120,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        MemoryPrivacyClass::SharedWithSubject,
    )
    .is_err());

    let valid = RuntimeSkillOwnerRecord::build(
        "space-1",
        RuntimeSkillOwningScope::Subject {
            mounted_subject_id: "subject-a".into(),
        },
        creation_ref(),
        2,
        intrinsic(),
        procedural_content(),
        RuntimeSkillLifecycle {
            availability: RuntimeSkillAvailability::Disabled,
            state: RuntimeSkillLifecycleState::Superseded,
            lineage: RuntimeSkillLifecycleLineage {
                predecessor: Some(
                    RuntimeSkillOwnerBinding::from_record(&created).expect("predecessor binding"),
                ),
                successor: Some(
                    RuntimeSkillOwnerBinding::from_record(&successor).expect("successor binding"),
                ),
            },
            observed_at: 100,
            updated_at: 120,
            usage_outcome: RuntimeSkillUsageOutcomeSummary::default(),
        },
        MemoryPrivacyClass::SharedWithSubject,
    )
    .expect("valid superseded owner");
    let mut encoded = serde_json::to_value(valid).expect("serialize valid owner");
    encoded["lifecycle"]["lineage"]["predecessor"]["privacy_class"] =
        serde_json::json!("public_runtime");
    let decoded =
        serde_json::from_value::<RuntimeSkillOwnerRecord>(encoded).expect("decode drifted owner");
    assert!(!decoded.validate_contract().accepted);
}
