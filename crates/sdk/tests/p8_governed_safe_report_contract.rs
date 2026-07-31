mod support;

use bm_sdk::{
    GovernedRecallEligibility, GovernedRecallEligibilityReason, LongTermMemoryDraft,
    LongTermMemoryKind, MemoryPrivacyClass, MemoryProjectionRequest, MemoryRecallRequest,
    MemoryRecallTemporalOperation, MemoryWriteRequest, ParsedLongTermMemoryExtraction,
    PremiseEvaluationDecision, PremiseTypedSource, PressureLevel, RuntimeLifecycleModeInput,
};
use support::{empty_store_platform, host_test_profile, seeded_store_platform, test_runtime};

fn assert_opaque_ref(value: &str, prefix: &str) {
    let digest = value
        .strip_prefix(prefix)
        .expect("opaque ref must use its contract domain");
    assert_eq!(digest.len(), 64);
    assert!(digest
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
}

#[test]
fn governed_recall_safe_reports_bind_one_operation_authority_and_receipt() {
    let profile = host_test_profile();
    let runtime = test_runtime(empty_store_platform(profile), profile);
    let report = runtime
        .recall(MemoryRecallRequest {
            query: "release evidence".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation: MemoryRecallTemporalOperation::Current,
        })
        .expect("current recall");

    let public = report.governed_public_report();
    let operator = report.governed_operator_report();
    assert!(public.validate_contract().is_empty());
    assert!(operator.validate_contract().is_empty());
    assert_eq!(operator.payload().public_report(), public);
    assert_eq!(public.authority().profile(), profile);
    assert_eq!(
        public.authority().temporal_operation(),
        MemoryRecallTemporalOperation::Current
    );
    assert_eq!(operator.payload().session_open_count(), 1);
    assert_eq!(operator.payload().receipt_count(), 1);
    assert!(operator.payload().manifest_verified());
    assert!(operator.payload().read_set_exact());

    assert_opaque_ref(
        public.authority().authority_ref().as_str(),
        "recall_operation_authority:sha256:",
    );
    assert_opaque_ref(
        operator.payload().store_snapshot_receipt().as_str(),
        "recall_store_snapshot:sha256:",
    );
    assert_opaque_ref(
        operator.report_digest(),
        "governed_recall_operator_report:sha256:",
    );

    for eligibility in [
        GovernedRecallEligibility::EligibleCurrent,
        GovernedRecallEligibility::EligibleHistoricalAsOf,
        GovernedRecallEligibility::Excluded,
    ] {
        assert_eq!(public.eligibility_counts().get(&eligibility), Some(&0));
    }
    for reason in [
        GovernedRecallEligibilityReason::PrivacyBlocked,
        GovernedRecallEligibilityReason::Forgotten,
        GovernedRecallEligibilityReason::Deleted,
        GovernedRecallEligibilityReason::Invalidated,
        GovernedRecallEligibilityReason::Superseded,
        GovernedRecallEligibilityReason::Obsolete,
        GovernedRecallEligibilityReason::Stale,
        GovernedRecallEligibilityReason::PremiseBlocked,
        GovernedRecallEligibilityReason::ProfileBlocked,
        GovernedRecallEligibilityReason::BudgetBlocked,
        GovernedRecallEligibilityReason::Tombstoned,
        GovernedRecallEligibilityReason::Redacted,
    ] {
        assert_eq!(public.reason_counts().get(&reason), Some(&0));
    }
    for source in [
        PremiseTypedSource::RegisteredCapability,
        PremiseTypedSource::OpaquePresenceAttestation,
        PremiseTypedSource::GovernedEnvironmentEvidence,
        PremiseTypedSource::TaskLearning,
        PremiseTypedSource::TaskRun,
        PremiseTypedSource::TaskArtifact,
    ] {
        assert_eq!(public.premise().source_counts().get(&source), Some(&0));
    }
    for decision in [
        PremiseEvaluationDecision::Satisfied,
        PremiseEvaluationDecision::Unsatisfied,
        PremiseEvaluationDecision::Unknown,
        PremiseEvaluationDecision::Expired,
        PremiseEvaluationDecision::PrivacyBlocked,
    ] {
        assert_eq!(public.premise().decision_counts().get(&decision), Some(&0));
    }
}

#[test]
fn projection_embeds_the_same_safe_recall_authority_without_raw_material() {
    let profile = host_test_profile();
    let runtime = test_runtime(empty_store_platform(profile), profile);
    let report = runtime
        .project_safe(MemoryProjectionRequest {
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "release evidence".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("safe projection");

    let public = report.governed_public_report();
    let operator = report.governed_operator_report();
    assert_eq!(operator.payload().public_report(), public);
    assert_eq!(
        public.authority().temporal_operation(),
        report.temporal_operation()
    );

    let serialized = serde_json::to_string(operator).expect("operator safe JSON");
    for forbidden in [
        "private-owner-sentinel",
        "private-space-sentinel",
        "private-subject-sentinel",
        "raw-procedure-sentinel",
        "credential-sentinel",
        "\"state_digest\"",
        "\"content_digest\"",
        "\"scope_digest\"",
        "\"path\"",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "safe report leaked forbidden material: {forbidden}"
        );
    }
}

#[test]
fn authority_and_operator_envelopes_reject_forged_or_drifting_json() {
    let profile = host_test_profile();
    let runtime = test_runtime(empty_store_platform(profile), profile);
    let report = runtime
        .recall(MemoryRecallRequest {
            query: "release evidence".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation: MemoryRecallTemporalOperation::Current,
        })
        .expect("current recall");
    let canonical =
        serde_json::to_value(report.governed_operator_report()).expect("operator JSON value");

    let mut forged_authority = canonical.clone();
    forged_authority["payload"]["public_report"]["authority"]["authority_ref"] =
        serde_json::Value::String(format!(
            "recall_operation_authority:sha256:{}",
            "0".repeat(64)
        ));
    let forged_authority =
        serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(forged_authority)
            .expect("well-shaped forged authority JSON");
    let failures = forged_authority.validate_contract();
    assert!(failures.contains(&bm_sdk::GovernedRecallIntegrityFailureV1::AuthorityMismatch));
    assert!(failures.contains(&bm_sdk::GovernedRecallIntegrityFailureV1::DigestMismatch));

    let mut forged_commitment = canonical.clone();
    forged_commitment["payload"]["public_report"]["authority"]["private_admission_commitment"] =
        serde_json::Value::String(format!(
            "recall_private_admission:sha256:{}",
            "1".repeat(64)
        ));
    let forged_commitment =
        serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(forged_commitment)
            .expect("well-shaped forged commitment JSON");
    let failures = forged_commitment.validate_contract();
    assert!(failures.contains(&bm_sdk::GovernedRecallIntegrityFailureV1::AuthorityMismatch));
    assert!(failures.contains(&bm_sdk::GovernedRecallIntegrityFailureV1::DigestMismatch));

    let mut forged_digest = canonical.clone();
    forged_digest["report_digest"] = serde_json::Value::String(format!(
        "governed_recall_operator_report:sha256:{}",
        "2".repeat(64)
    ));
    let forged_digest =
        serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(forged_digest)
            .expect("well-shaped forged digest JSON");
    assert_eq!(
        forged_digest.validate_contract(),
        vec![bm_sdk::GovernedRecallIntegrityFailureV1::DigestMismatch]
    );

    let mut malformed_receipt = canonical.clone();
    malformed_receipt["payload"]["store_snapshot_receipt"] =
        serde_json::Value::String(format!("wrong:sha256:{}", "3".repeat(64)));
    assert!(
        serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(malformed_receipt)
            .is_err()
    );

    let mut unknown_field = canonical.clone();
    unknown_field["payload"]["public_report"]["authority"]["raw_policy"] =
        serde_json::Value::Bool(true);
    assert!(
        serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(unknown_field).is_err()
    );
}

#[test]
fn temporal_operation_json_is_required_typed_and_strict() {
    assert_eq!(
        serde_json::from_value::<MemoryRecallTemporalOperation>(serde_json::json!({
            "kind": "current"
        }))
        .expect("current"),
        MemoryRecallTemporalOperation::Current
    );
    assert_eq!(
        serde_json::from_value::<MemoryRecallTemporalOperation>(serde_json::json!({
            "kind": "historical_as_of",
            "as_of_time": 42
        }))
        .expect("historical"),
        MemoryRecallTemporalOperation::HistoricalAsOf { as_of_time: 42 }
    );
    for invalid in [
        serde_json::json!({}),
        serde_json::json!({"kind": "current", "as_of_time": 42}),
        serde_json::json!({"kind": "historical_as_of"}),
        serde_json::json!({"kind": "HistoricalAsOf", "as_of_time": 42}),
        serde_json::json!("current"),
    ] {
        assert!(
            serde_json::from_value::<MemoryRecallTemporalOperation>(invalid.clone()).is_err(),
            "accepted invalid temporal operation: {invalid}"
        );
    }
}

#[test]
fn non_empty_long_term_safe_bindings_exactly_follow_delivery_decisions() {
    let profile = host_test_profile();
    let runtime = test_runtime(seeded_store_platform(profile), profile);
    let recall = runtime
        .recall(MemoryRecallRequest {
            query: "release artifact".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation: MemoryRecallTemporalOperation::Current,
        })
        .expect("seeded recall");
    let public =
        serde_json::to_value(recall.governed_public_report()).expect("public safe report JSON");
    assert_eq!(public["eligibility_counts"]["eligible_current"], 1);
    assert_eq!(
        public["validity_candidate_bindings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let binding = &public["validity_candidate_bindings"][0];
    assert_eq!(binding["eligibility"], "eligible_current");
    assert_eq!(binding["matched"], true);
    assert_eq!(binding["selected"], true);
    assert_eq!(binding["rendered"], true);
    assert_opaque_ref(
        binding["candidate_ref"]
            .as_str()
            .expect("safe candidate ref"),
        "governed_owner_revision:sha256:",
    );

    let projection = runtime
        .project_safe(MemoryProjectionRequest {
            temporal_operation: MemoryRecallTemporalOperation::Current,
            user_query: "release artifact".to_string(),
            system_max_len: 4096,
            recent_messages_limit: 4,
            pressure: PressureLevel::Normal,
            mode_input: RuntimeLifecycleModeInput::default(),
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
        })
        .expect("seeded projection");
    let projected = serde_json::to_value(projection.governed_public_report())
        .expect("projection safe report JSON");
    assert_eq!(projected["validity_candidate_bindings"][0]["matched"], true);
    assert_eq!(
        projected["validity_candidate_bindings"][0]["selected"],
        true
    );
    assert_eq!(
        projected["validity_candidate_bindings"][0]["rendered"],
        true
    );
}

#[test]
fn private_long_term_material_never_enters_the_safe_recall_closure() {
    let profile = host_test_profile();
    let runtime = test_runtime(empty_store_platform(profile), profile);
    runtime
        .write(MemoryWriteRequest::LongTermExtraction {
            governed_skill_writes: Vec::new(),
            runtime_skill_owning_scope: None,
            extraction: ParsedLongTermMemoryExtraction {
                upserts: vec![LongTermMemoryDraft {
                    kind: LongTermMemoryKind::Project,
                    topic: "private-owner-sentinel".to_string(),
                    content: "credential-sentinel raw private content".to_string(),
                    keywords: vec!["private".to_string()],
                    privacy: MemoryPrivacyClass::PrivateGarden,
                    source_chat_id: Some("private-space-sentinel".to_string()),
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: vec!["private-subject-sentinel".to_string()],
                    canonical_entities: Vec::new(),
                    evidence_count: Some(1),
                    observed_at: Some(1_800_000_000),
                    last_confirmed_at: Some(1_800_000_000),
                    source_revision: Some(1),
                }],
                deletes: Vec::new(),
                skill_writes: Vec::new(),
            },
        })
        .expect("seed private long-term owner");

    let report = runtime
        .recall(MemoryRecallRequest {
            query: "private".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation: MemoryRecallTemporalOperation::Current,
        })
        .expect("private-safe recall");
    let public = report.governed_public_report();
    assert_eq!(
        public
            .eligibility_counts()
            .get(&GovernedRecallEligibility::Excluded),
        Some(&0)
    );
    assert_eq!(
        public
            .reason_counts()
            .get(&GovernedRecallEligibilityReason::PrivacyBlocked),
        Some(&0)
    );
    let serialized =
        serde_json::to_string(report.governed_operator_report()).expect("operator safe JSON");
    for forbidden in [
        "private-owner-sentinel",
        "private-space-sentinel",
        "private-subject-sentinel",
        "credential-sentinel",
        "raw private content",
        "\"state_digest\"",
        "\"content_digest\"",
        "\"scope_digest\"",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "private safe report leaked forbidden material: {forbidden}"
        );
    }
}

#[test]
fn historical_safe_report_binds_typed_as_of_and_bounded_lineage() {
    let profile = host_test_profile();
    let runtime = test_runtime(seeded_store_platform(profile), profile);
    let report = runtime
        .recall(MemoryRecallRequest {
            query: "release artifact".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation: MemoryRecallTemporalOperation::HistoricalAsOf {
                as_of_time: 1_800_000_000,
            },
        })
        .expect("historical recall");
    assert!(report
        .governed_operator_report()
        .validate_contract()
        .is_empty());
    let operator =
        serde_json::to_value(report.governed_operator_report()).expect("historical operator JSON");
    assert_eq!(
        operator["payload"]["public_report"]["authority"]["temporal_operation"],
        serde_json::json!({
            "kind": "historical_as_of",
            "as_of_time": 1_800_000_000u64
        })
    );
    assert_eq!(
        operator["payload"]["public_report"]["eligibility_counts"]["eligible_historical_as_of"],
        1
    );
    let item_count = operator["payload"]["public_report"]["lineage"]["item_count"]
        .as_u64()
        .expect("lineage item count");
    assert!(item_count >= 1);
    assert_eq!(
        u64::try_from(
            operator["payload"]["bounded_lineage_items"]
                .as_array()
                .expect("bounded lineage")
                .len(),
        )
        .expect("bounded lineage count"),
        item_count
    );
    assert_eq!(
        operator["payload"]["public_report"]["lineage"]["complete"],
        true
    );
}

#[test]
fn opaque_refs_and_required_fields_reject_the_full_malformed_matrix() {
    let profile = host_test_profile();
    let runtime = test_runtime(empty_store_platform(profile), profile);
    let report = runtime
        .recall(MemoryRecallRequest {
            query: "release evidence".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation: MemoryRecallTemporalOperation::Current,
        })
        .expect("current recall");
    let canonical = serde_json::to_value(report.governed_operator_report()).expect("operator JSON");

    for invalid in [
        format!("wrong_domain:sha256:{}", "0".repeat(64)),
        format!("recall_operation_authority:sha512:{}", "0".repeat(64)),
        format!("recall_operation_authority:sha256:{}", "A".repeat(64)),
        format!("recall_operation_authority:sha256:{}", "0".repeat(63)),
        "recall_operation_authority:sha256:not-hex".to_string(),
    ] {
        let mut value = canonical.clone();
        value["payload"]["public_report"]["authority"]["authority_ref"] =
            serde_json::Value::String(invalid);
        assert!(serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(value).is_err());
    }
    for invalid in [
        format!("wrong_domain:sha256:{}", "0".repeat(64)),
        format!("recall_store_snapshot:sha512:{}", "0".repeat(64)),
        format!("recall_store_snapshot:sha256:{}", "A".repeat(64)),
        format!("recall_store_snapshot:sha256:{}", "0".repeat(65)),
        "recall_store_snapshot:sha256:not-hex".to_string(),
    ] {
        let mut value = canonical.clone();
        value["payload"]["store_snapshot_receipt"] = serde_json::Value::String(invalid);
        assert!(serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(value).is_err());
    }

    let mut missing_authority = canonical.clone();
    missing_authority["payload"]["public_report"]["authority"]
        .as_object_mut()
        .expect("authority object")
        .remove("authority_ref");
    assert!(
        serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(missing_authority)
            .is_err()
    );

    let mut missing_receipt = canonical.clone();
    missing_receipt["payload"]
        .as_object_mut()
        .expect("payload object")
        .remove("store_snapshot_receipt");
    assert!(
        serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(missing_receipt).is_err()
    );

    let serialized =
        serde_json::to_string(report.governed_operator_report()).expect("operator JSON");
    let duplicate_authority = serialized.replacen(
        "\"authority_ref\":",
        &format!(
            "\"authority_ref\":\"recall_operation_authority:sha256:{}\",\"authority_ref\":",
            "0".repeat(64)
        ),
        1,
    );
    assert!(
        serde_json::from_str::<bm_sdk::GovernedRecallOperatorReportV1>(&duplicate_authority)
            .is_err()
    );
    let duplicate_receipt = serialized.replacen(
        "\"store_snapshot_receipt\":",
        &format!(
            "\"store_snapshot_receipt\":\"recall_store_snapshot:sha256:{}\",\"store_snapshot_receipt\":",
            "0".repeat(64)
        ),
        1,
    );
    assert!(
        serde_json::from_str::<bm_sdk::GovernedRecallOperatorReportV1>(&duplicate_receipt).is_err()
    );
    let duplicate_digest = serialized.replacen(
        "\"report_digest\":",
        &format!(
            "\"report_digest\":\"governed_recall_operator_report:sha256:{}\",\"report_digest\":",
            "0".repeat(64)
        ),
        1,
    );
    assert!(
        serde_json::from_str::<bm_sdk::GovernedRecallOperatorReportV1>(&duplicate_digest).is_err()
    );

    let mut forged_receipt = canonical;
    forged_receipt["payload"]["store_snapshot_receipt"] =
        serde_json::Value::String(format!("recall_store_snapshot:sha256:{}", "4".repeat(64)));
    let forged_receipt =
        serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(forged_receipt)
            .expect("well-shaped forged receipt");
    assert!(!forged_receipt.validate_contract().is_empty());
}

#[test]
fn duplicate_safe_bindings_and_failures_are_rejected() {
    let profile = host_test_profile();
    let runtime = test_runtime(seeded_store_platform(profile), profile);
    let report = runtime
        .recall(MemoryRecallRequest {
            query: "release artifact".to_string(),
            limit: 4,
            structured_query_facets: Vec::new(),
            tool_registry_refs: Vec::new(),
            temporal_operation: MemoryRecallTemporalOperation::Current,
        })
        .expect("seeded recall");
    let canonical = serde_json::to_value(report.governed_operator_report()).expect("operator JSON");

    let mut duplicate_binding = canonical.clone();
    let bindings = duplicate_binding["payload"]["public_report"]["validity_candidate_bindings"]
        .as_array_mut()
        .expect("candidate bindings");
    bindings.push(bindings[0].clone());
    let duplicate_binding =
        serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(duplicate_binding)
            .expect("well-shaped duplicate binding");
    let failures = duplicate_binding.validate_contract();
    assert!(failures.contains(&bm_sdk::GovernedRecallIntegrityFailureV1::CandidateBindingMismatch));

    let mut duplicate_failure = canonical;
    duplicate_failure["payload"]["public_report"]["integrity_failures"] =
        serde_json::json!(["authority_mismatch", "authority_mismatch"]);
    let duplicate_failure =
        serde_json::from_value::<bm_sdk::GovernedRecallOperatorReportV1>(duplicate_failure)
            .expect("well-shaped duplicate failure");
    assert!(duplicate_failure
        .validate_contract()
        .contains(&bm_sdk::GovernedRecallIntegrityFailureV1::CanonicalOrderMismatch));
}
