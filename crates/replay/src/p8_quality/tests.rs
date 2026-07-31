use std::collections::{BTreeMap, BTreeSet};

use super::source_release::*;
use super::trusted_execution::engineering_gate::P8EngineeringGateReceiptV1;
use super::*;

fn id(value: &str) -> P8QualityId {
    P8QualityId::parse(value).expect("canonical P8 quality id")
}

fn digest(byte: char) -> P8QualityDigest {
    P8QualityDigest::parse(format!("sha256:{}", byte.to_string().repeat(64)))
        .expect("canonical digest")
}

fn anchor() -> P8P84SemanticSourceAnchorV1 {
    let audit = P8P84RawSourceAuditManifestV1::materialized_from_cutover_audit();
    P8P84SemanticSourceAnchorV1::build(&audit).expect("materialized semantic anchor")
}

#[derive(Clone)]
struct SourceFixture {
    harness: P8HarnessReleaseManifestV1,
    no_memory: P8ArmImplementationReleaseV1,
    public_reference: P8ArmImplementationReleaseV1,
    frozen: P8ArmImplementationReleaseV1,
    candidate: P8ArmImplementationReleaseV1,
}

impl SourceFixture {
    fn new() -> Self {
        let harness = P8HarnessReleaseManifestV1::test_fixture(digest('1'));
        let no_memory_input = P8ArmImplementationInputManifestV1::no_memory(
            harness.quality_runner_executable_digest().clone(),
        );
        let no_memory = P8ArmImplementationReleaseV1::no_memory(no_memory_input, &harness)
            .expect("no-memory release");

        let public_config = digest('4');
        let public_input = P8ArmImplementationInputManifestV1::public_reference(
            digest('2'),
            digest('3'),
            public_config.clone(),
        );
        let public_executable = digest('5');
        let public_receipt = harness.arm_sealed_execution_receipt(
            &public_input,
            public_executable.clone(),
            digest('6'),
        );
        let public_reference = P8ArmImplementationReleaseV1::public_reference(
            public_input,
            &harness,
            digest('7'),
            public_executable,
            public_config,
            digest('8'),
            public_receipt,
        )
        .expect("public-reference release");

        let frozen_input = P8ArmImplementationInputManifestV1::beetle(
            P8QualityArmKind::FrozenP84Baseline,
            anchor().anchor_digest().clone(),
        )
        .expect("frozen input");
        let frozen_executable = digest('9');
        let frozen_receipt = harness.arm_sealed_execution_receipt(
            &frozen_input,
            frozen_executable.clone(),
            digest('a'),
        );
        let frozen = P8ArmImplementationReleaseV1::beetle(
            frozen_input,
            &harness,
            frozen_executable,
            frozen_receipt,
        )
        .expect("frozen release");

        let candidate_input = P8ArmImplementationInputManifestV1::beetle(
            P8QualityArmKind::P8Candidate,
            P8SemanticSourceAnchorRef::derive_for_test("candidate-source"),
        )
        .expect("candidate input");
        let candidate_executable = digest('b');
        let candidate_receipt = harness.arm_sealed_execution_receipt(
            &candidate_input,
            candidate_executable.clone(),
            digest('c'),
        );
        let candidate = P8ArmImplementationReleaseV1::beetle(
            candidate_input,
            &harness,
            candidate_executable,
            candidate_receipt,
        )
        .expect("candidate release");

        Self {
            harness,
            no_memory,
            public_reference,
            frozen,
            candidate,
        }
    }

    fn baseline_set(&self) -> P8SourceReleaseSetV1 {
        P8SourceReleaseSetV1::build(
            P8QualityPurpose::BaselineEstablishment,
            self.harness.clone(),
            vec![
                self.no_memory.clone(),
                self.public_reference.clone(),
                self.frozen.clone(),
            ],
        )
        .expect("baseline source set")
    }

    fn candidate_set(&self) -> P8SourceReleaseSetV1 {
        P8SourceReleaseSetV1::build(
            P8QualityPurpose::QualityCandidate,
            self.harness.clone(),
            vec![
                self.no_memory.clone(),
                self.public_reference.clone(),
                self.frozen.clone(),
                self.candidate.clone(),
            ],
        )
        .expect("candidate source set")
    }
}

fn hypotheses(questions: &[P8QualityId]) -> P8QualityHypothesisRegistryV1 {
    let expected = questions
        .iter()
        .cloned()
        .map(|question_id| {
            (
                question_id,
                P8ExpectedCapabilityOutcomesV1::current_procedural(),
            )
        })
        .collect();
    hypotheses_with_expected(questions, &expected)
}

fn hypotheses_with_expected(
    questions: &[P8QualityId],
    expected: &BTreeMap<P8QualityId, P8ExpectedCapabilityOutcomesV1>,
) -> P8QualityHypothesisRegistryV1 {
    let question_set_digest =
        P8QualityDigest::derive("p8_quality_ordered_question_set_v1", &questions);
    let question_expectations = questions
        .iter()
        .map(|question_id| {
            P8QuestionEvaluationExpectationV1::new(
                question_id.clone(),
                question_set_digest.clone(),
                expected[question_id].clone(),
            )
            .expect("question expectation")
        })
        .collect();
    let memberships = questions
        .iter()
        .map(|question_id| {
            P8HypothesisQuestionMembershipV1::included(
                question_id.clone(),
                question_set_digest.clone(),
            )
        })
        .collect();
    let target = P8QualityHypothesisSpecV1::new(
        id("dynamic-state"),
        P8HypothesisRoleV1::target(),
        vec![P8HypothesisAxisV1::Capability(
            P8CapabilitySlice::DynamicState,
        )],
        memberships,
    )
    .expect("target hypothesis");
    let untouched = P8QualityHypothesisSpecV1::new(
        id("privacy-wall"),
        P8HypothesisRoleV1::untouched(),
        vec![P8HypothesisAxisV1::Safety(P8SafetySlice::Privacy)],
        questions
            .iter()
            .map(|question_id| {
                P8HypothesisQuestionMembershipV1::included(
                    question_id.clone(),
                    question_set_digest.clone(),
                )
            })
            .collect(),
    )
    .expect("untouched hypothesis");
    P8QualityHypothesisRegistryV1::build(
        questions.to_vec(),
        question_expectations,
        vec![target, untouched],
    )
    .expect("hypothesis registry")
}

fn model_lock(seed: char) -> P8ModelLockV1 {
    P8ModelLockV1 {
        provider_identity_digest: digest(seed),
        model_revision_digest: P8QualityDigest::derive("model-revision", &seed),
        prompt_contract_digest: P8QualityDigest::derive("prompt-contract", &seed),
        configuration_digest: P8QualityDigest::derive("model-config", &seed),
        tool_schema_digest: P8QualityDigest::derive("tool-schema", &seed),
        generation_policy_digest: P8QualityDigest::derive("generation-policy", &seed),
    }
}

fn protocol(
    source: &P8SourceReleaseSetV1,
    questions: &[P8QualityId],
    mode: P8ExecutionMode,
) -> P8EvaluationProtocolLockV1 {
    let policy = P8QualityHardPolicyV1::canonical();
    let registry = hypotheses(questions);
    let frozen = P8ProtocolFreezeInputsV1 {
        dataset: P8DatasetProtocolV1 {
            dataset_identity_digest: digest('1'),
            dataset_version_digest: digest('2'),
            dataset_license_digest: digest('3'),
            input_manifest_digest: digest('4'),
            ordered_question_rubric_gold_manifest_digest: digest('5'),
            ordered_question_ids_digest: P8QualityDigest::derive(
                "p8_quality_ordered_question_set_v1",
                &questions,
            ),
        },
        arm_universe: P8ArmUniverseProtocolV1 {
            arm_universe: P8QualityArmKind::CANDIDATE.to_vec(),
            baseline_applicable_arms: P8QualityArmKind::BASELINE.to_vec(),
            candidate_applicable_arms: P8QualityArmKind::CANDIDATE.to_vec(),
            no_memory_release: source
                .arm_release_digest(P8QualityArmKind::NoMemory)
                .expect("no-memory release")
                .clone(),
            public_reference_release: source
                .arm_release_digest(P8QualityArmKind::PublicReference)
                .expect("public release")
                .clone(),
            frozen_p84_release: source
                .arm_release_digest(P8QualityArmKind::FrozenP84Baseline)
                .expect("frozen release")
                .clone(),
            candidate_slot_domain: P8CandidateSlotDomainV1::DistinctBeetleSemanticSourceAndRelease,
            common_harness_semantic_source_digest: source
                .common_harness_semantic_source_digest()
                .clone(),
            toolchain_contract_digest: source.harness_toolchain_digest().clone(),
            build_contract_digest: source.harness_build_contract_digest().clone(),
        },
        runtime_identity: P8RuntimeIdentityProtocolV1 {
            profile: ProfileId::ServerLinuxDevFull,
            backend: P8BenchmarkBackendKindV1::Sqlite,
            capability_snapshot_digest: digest('7'),
            runtime_budget_report_id: P8RuntimeBudgetReportIdV1::parse(format!(
                "rtb-v2-{}",
                "8".repeat(64)
            ))
            .expect("budget report id"),
        },
        models: P8ReaderJudgeProtocolV1 {
            reader: model_lock('9'),
            judge: model_lock('a'),
        },
        trial: P8TrialProtocolV1 {
            reader_repeats: 2,
            judge_repeats: 3,
            trial_order: P8TrialOrderPolicyV1::ProtocolOrderedQuestionArmReaderJudge,
            repeat_aggregation: P8RepeatAggregationPolicyV1::JudgeMajorityThenReaderExactRational,
            missingness: P8MissingnessPolicyV1::RequiredPairInvalidatesHypothesis,
        },
        statistics: P8StatisticalProtocolV1 {
            confidence_level_basis_points: 9_500,
            family_alpha_parts_per_million: 50_000,
            bootstrap_resamples: 10_000,
            minimum_effective_questions: 2,
            ci_algorithm: P8ConfidenceIntervalAlgorithmV1::QuestionClusterPairedBootstrap,
            tail: P8ConfidenceTailV1::OneSidedLowerForQuality,
            rounding: P8RoundingPolicyV1::ExactRationalThenOutwardIntegerBound,
            multiple_comparison_correction: P8MultipleComparisonCorrectionV1::HolmFamilyWise,
            bootstrap_seed_derivation: P8BootstrapSeedDerivationV1::ExperimentPlanDigest,
            threshold_derivation: P8ThresholdDerivationPolicyV1::FirstVerifiedBaselineDistribution,
        },
        resources: P8ResourceProtocolV1 {
            rendered_chars_measure:
                P8RenderedCharsMeasurePolicyV1::SdkMemoryProjectionUnicodeScalarCount,
            rendered_chars_quantile_basis_points: 9_500,
            latency_measure: P8LatencyMeasurePolicyV1::ArmInputReadyToFinalReaderResponse,
            latency_quantile_basis_points: 9_500,
            memory_measure: P8MemoryMeasurePolicyV1::ExclusiveArmCgroupV2RunRootMemoryPeak,
            shard_count: 2,
            maximum_concurrent_shards_per_arm: 2,
            arms_execute_serially: true,
        },
        hard_policy_digest: policy.policy_digest().clone(),
        hypothesis_registry: registry,
        execution_mode: mode,
    };
    P8EvaluationProtocolLockV1::build(frozen).expect("protocol lock")
}

fn threshold(protocol: &P8EvaluationProtocolLockV1) -> P8QualityThresholdLockV1 {
    P8QualityThresholdLockV1::build(
        protocol.protocol_digest().clone(),
        protocol.hard_policy_digest().clone(),
        P8BaselineManifestRef::derive_for_test("baseline"),
        P8ThresholdDerivationOutputsV1 {
            target_thresholds_digest: digest('b'),
            untouched_thresholds_digest: digest('c'),
            rendered_chars_frontier_digest: digest('d'),
            latency_frontier_digest: digest('e'),
            peak_domain_memory_frontier_digest: digest('f'),
        },
    )
}

fn hard_policy_oracle() -> Vec<(P8QualityHardGateId, P8HardGateRequirement, &'static str)> {
    vec![
        (
            P8QualityHardGateId::IneligibleOwnerProjection,
            P8HardGateRequirement::ExactZero,
            "sdk.governed-report.ineligible-owner-zero.v1",
        ),
        (
            P8QualityHardGateId::NonCurrentMaterialProjection,
            P8HardGateRequirement::ExactZero,
            "sdk.governed-report.non-current-material-zero.v1",
        ),
        (
            P8QualityHardGateId::CrossSubjectPrivateSoulLeak,
            P8HardGateRequirement::ExactZero,
            "sdk.governed-report.cross-subject-private-soul-zero.v1",
        ),
        (
            P8QualityHardGateId::RawProcedureCredentialPathOrPrematureGoldPersistence,
            P8HardGateRequirement::ExactZero,
            "replay.artifact.raw-procedure-credential-path-gold-zero.v1",
        ),
        (
            P8QualityHardGateId::UnexpectedRuntimeOrIntegrityFailure,
            P8HardGateRequirement::ExactZero,
            "replay.quality.unexpected-runtime-integrity-zero.v1",
        ),
        (
            P8QualityHardGateId::FullStoreScanSecondPlatformOrLiveFallback,
            P8HardGateRequirement::ExactZero,
            "sdk.governed-report.full-scan-second-platform-live-fallback-zero.v1",
        ),
        (
            P8QualityHardGateId::PostImageClosureCoverage,
            P8HardGateRequirement::ExactFull,
            "store.post-image-closure-coverage-full.v1",
        ),
        (
            P8QualityHardGateId::UpdateLineageViolation,
            P8HardGateRequirement::ExactZero,
            "core.memory-update-lineage-violation-zero.v1",
        ),
        (
            P8QualityHardGateId::UnmetPremiseProcedureDelivery,
            P8HardGateRequirement::ExactZero,
            "sdk.governed-report.unmet-premise-procedure-delivery-zero.v1",
        ),
        (
            P8QualityHardGateId::RequiredReportOperatorCoverage,
            P8HardGateRequirement::ExactFull,
            "replay.quality.required-report-operator-coverage-full.v1",
        ),
        (
            P8QualityHardGateId::ProfileBudgetRenderCeilingBreach,
            P8HardGateRequirement::ExactZero,
            "sdk.governed-report.profile-budget-render-breach-zero.v1",
        ),
    ]
}

fn hard_policy_source_oracle(gate_id: P8QualityHardGateId) -> &'static str {
    match gate_id {
        P8QualityHardGateId::PostImageClosureCoverage => P8_POST_IMAGE_VALIDATOR_FINGERPRINT,
        P8QualityHardGateId::UpdateLineageViolation => P8_CORE_VALIDATOR_FINGERPRINT,
        P8QualityHardGateId::RawProcedureCredentialPathOrPrematureGoldPersistence
        | P8QualityHardGateId::UnexpectedRuntimeOrIntegrityFailure
        | P8QualityHardGateId::RequiredReportOperatorCoverage => P8_REPLAY_VALIDATOR_FINGERPRINT,
        P8QualityHardGateId::IneligibleOwnerProjection
        | P8QualityHardGateId::NonCurrentMaterialProjection
        | P8QualityHardGateId::CrossSubjectPrivateSoulLeak
        | P8QualityHardGateId::FullStoreScanSecondPlatformOrLiveFallback
        | P8QualityHardGateId::UnmetPremiseProcedureDelivery
        | P8QualityHardGateId::ProfileBudgetRenderCeilingBreach => P8_SDK_VALIDATOR_FINGERPRINT,
    }
}

fn fixture_plan(purpose: P8QualityPurpose) -> P8QualityExperimentPlanV1 {
    let source = SourceFixture::new();
    let questions = vec![id("q-1"), id("q-2")];
    let policy = P8QualityHardPolicyV1::canonical();
    let comparator_source = source.baseline_set();
    let protocol = protocol(
        &comparator_source,
        &questions,
        P8ExecutionMode::FixtureContract,
    );
    let source_set = match purpose {
        P8QualityPurpose::BaselineEstablishment => comparator_source,
        P8QualityPurpose::QualityCandidate => source.candidate_set(),
    };
    let closure = P8QualityTrialClosureV1::derive(purpose, questions, 2, 3).expect("closure");
    let threshold = (purpose == P8QualityPurpose::QualityCandidate).then(|| threshold(&protocol));
    let frozen_quality_policy = threshold.as_ref().map(|threshold| {
        P8FrozenQualityPolicyV1::build(
            protocol.protocol_digest().clone(),
            threshold.threshold_digest().clone(),
        )
    });
    P8QualityExperimentPlanV1::build(
        purpose,
        P8ExecutionMode::FixtureContract,
        source_set,
        policy,
        protocol,
        closure,
        threshold,
        frozen_quality_policy,
        None,
    )
    .expect("fixture plan")
}

fn valid_quality_envelopes() -> Vec<P8QualityArtifactEnvelopeV1> {
    let audit = P8P84RawSourceAuditManifestV1::materialized_from_cutover_audit();
    let anchor = P8P84SemanticSourceAnchorV1::build(&audit).expect("anchor");
    let source = SourceFixture::new().candidate_set();
    let policy = P8QualityHardPolicyV1::canonical();
    let registry = hypotheses(&[id("q-1"), id("q-2")]);
    let closure = P8QualityTrialClosureV1::derive(
        P8QualityPurpose::QualityCandidate,
        vec![id("q-1"), id("q-2")],
        2,
        3,
    )
    .expect("closure");
    let plan = fixture_plan(P8QualityPurpose::QualityCandidate);
    let protocol = plan.protocol.clone();
    let threshold = plan.threshold.clone().expect("threshold");
    let threshold_evaluation =
        P8CandidateThresholdEvaluationV1::fixture(&threshold, vec![]).expect("threshold eval");
    let trial_set = P8QualityTrialSetV1::fixture(&plan);
    let resources = P8QualityResourceClosureV1::fixture(&trial_set);
    let hard_evaluation =
        P8HardGateEvaluationV1::evaluate(&policy, &resources).expect("hard gates");
    let trusted_domain_resource = P8TrustedDomainResourceReceiptV1::fixture(
        plan.run_id().clone(),
        P8QualityArmKind::FrozenP84Baseline,
        plan.arm_release_digest(P8QualityArmKind::FrozenP84Baseline)
            .expect("frozen release")
            .clone(),
    );
    vec![
        P8QualityArtifactEnvelopeV1::RawSourceAudit(Box::new(audit)),
        P8QualityArtifactEnvelopeV1::SemanticSourceAnchor(Box::new(anchor)),
        P8QualityArtifactEnvelopeV1::SourceReleaseSet(Box::new(source)),
        P8QualityArtifactEnvelopeV1::HardPolicy(Box::new(policy)),
        P8QualityArtifactEnvelopeV1::HypothesisRegistry(Box::new(registry)),
        P8QualityArtifactEnvelopeV1::TrialClosure(Box::new(closure)),
        P8QualityArtifactEnvelopeV1::EvaluationProtocol(Box::new(protocol)),
        P8QualityArtifactEnvelopeV1::ThresholdLock(Box::new(threshold)),
        P8QualityArtifactEnvelopeV1::ExperimentPlan(Box::new(plan)),
        P8QualityArtifactEnvelopeV1::HardGateEvaluation(Box::new(hard_evaluation)),
        P8QualityArtifactEnvelopeV1::ThresholdEvaluation(Box::new(threshold_evaluation)),
        P8QualityArtifactEnvelopeV1::TrialSet(Box::new(trial_set)),
        P8QualityArtifactEnvelopeV1::ResourceClosure(Box::new(resources)),
        P8QualityArtifactEnvelopeV1::TrustedDomainResourceReceipt(Box::new(
            trusted_domain_resource,
        )),
    ]
}

#[test]
fn p8_source_anchor_consumes_exact_raw_audit_evidence() {
    let audit = P8P84RawSourceAuditManifestV1::materialized_from_cutover_audit();
    assert!(audit.validate_contract().is_empty());
    let anchor = P8P84SemanticSourceAnchorV1::build(&audit).expect("anchor");
    assert!(anchor.validate_contract().is_empty());

    let mut drift = serde_json::to_value(&anchor).expect("anchor json");
    drift["raw_source_audit"]["source_file_count"] = serde_json::json!(617);
    let drift: P8P84SemanticSourceAnchorV1 = serde_json::from_value(drift).expect("strict shape");
    assert!(!drift.validate_contract().is_empty());
}

#[test]
fn p8_strict_json_admission_rejects_unknown_missing_and_duplicate_keys() {
    let policy = P8QualityHardPolicyV1::canonical();
    let envelope = P8QualityArtifactEnvelopeV1::HardPolicy(Box::new(policy.clone()));
    let bytes = serde_json::to_vec(&envelope).expect("envelope");
    assert!(admit_p8_quality_artifact(&bytes).is_ok());

    let mut unknown = serde_json::to_value(&envelope).expect("json");
    unknown["extra"] = serde_json::json!(true);
    assert!(
        admit_p8_quality_artifact(&serde_json::to_vec(&unknown).expect("unknown json")).is_err()
    );

    let mut missing = serde_json::to_value(&policy).expect("policy");
    missing.as_object_mut().expect("object").remove("schema");
    assert!(deserialize_p8_quality_artifact::<P8QualityHardPolicyV1>(
        &serde_json::to_vec(&missing).expect("missing json")
    )
    .is_err());

    for duplicate in [
        br#"{"x":1,"x":2}"#.as_slice(),
        br#"{"outer":{"x":1,"x":2}}"#.as_slice(),
    ] {
        assert!(deserialize_p8_quality_artifact::<serde_json::Value>(duplicate).is_err());
    }

    for envelope in valid_quality_envelopes() {
        let bytes = serde_json::to_vec(&envelope).expect("valid envelope");
        assert!(admit_p8_quality_artifact(&bytes).is_ok());

        for forbidden_field in ["credential", "raw_private_material", "absolute_path"] {
            let mut forbidden = serde_json::to_value(&envelope).expect("envelope");
            forbidden["artifact"][forbidden_field] = serde_json::json!("must-not-cross-admission");
            assert!(
                admit_p8_quality_artifact(
                    &serde_json::to_vec(&forbidden).expect("forbidden sentinel")
                )
                .is_err(),
                "{forbidden_field}"
            );
        }

        let mut nested_unknown = serde_json::to_value(&envelope).expect("envelope");
        nested_unknown["artifact"]["unexpected"] = serde_json::json!(true);
        assert!(admit_p8_quality_artifact(
            &serde_json::to_vec(&nested_unknown).expect("nested unknown")
        )
        .is_err());

        let mut nested_missing = serde_json::to_value(&envelope).expect("envelope");
        nested_missing["artifact"]
            .as_object_mut()
            .expect("artifact object")
            .remove("schema");
        assert!(admit_p8_quality_artifact(
            &serde_json::to_vec(&nested_missing).expect("nested missing")
        )
        .is_err());
    }

    let source = SourceFixture::new().candidate_set();
    let source_envelope = P8QualityArtifactEnvelopeV1::SourceReleaseSet(Box::new(source));
    let mut deep_unknown = serde_json::to_value(source_envelope).expect("source envelope");
    deep_unknown["artifact"]["harness_release"]["source_input"]["common_harness_semantic_source"]
        ["unexpected"] = serde_json::json!(true);
    assert!(
        admit_p8_quality_artifact(&serde_json::to_vec(&deep_unknown).expect("deep unknown"))
            .is_err()
    );
}

#[test]
fn p8_raw_sentinel_canaries_are_rejected_even_in_valid_id_boundaries() {
    for sentinel in [
        "private-owner-sentinel",
        "private-space-sentinel",
        "private-subject-sentinel",
        "raw-procedure-sentinel",
        "raw-soul-sentinel",
        "credential-sentinel",
        "path-sentinel",
    ] {
        assert!(reject_p8_quality_raw_sentinels(sentinel.as_bytes()).is_err());

        let question_registry = hypotheses(&[id(sentinel)]);
        assert!(question_registry.validate_contract().is_empty());
        let question_envelope =
            P8QualityArtifactEnvelopeV1::HypothesisRegistry(Box::new(question_registry));
        assert!(admit_p8_quality_artifact(
            &serde_json::to_vec(&question_envelope).expect("question canary envelope")
        )
        .is_err());

        let question = id("q-1");
        let questions = vec![question.clone()];
        let question_set_digest =
            P8QualityDigest::derive("p8_quality_ordered_question_set_v1", &questions);
        let expectation = P8QuestionEvaluationExpectationV1::new(
            question.clone(),
            question_set_digest.clone(),
            P8ExpectedCapabilityOutcomesV1::current_procedural(),
        )
        .expect("expectation");
        let hypothesis = P8QualityHypothesisSpecV1::new(
            id(sentinel),
            P8HypothesisRoleV1::target(),
            vec![P8HypothesisAxisV1::Capability(
                P8CapabilitySlice::DynamicState,
            )],
            vec![P8HypothesisQuestionMembershipV1::included(
                question,
                question_set_digest,
            )],
        )
        .expect("hypothesis canary is a structurally valid ID");
        let hypothesis_registry =
            P8QualityHypothesisRegistryV1::build(questions, vec![expectation], vec![hypothesis])
                .expect("hypothesis registry");
        let hypothesis_envelope =
            P8QualityArtifactEnvelopeV1::HypothesisRegistry(Box::new(hypothesis_registry));
        assert!(admit_p8_quality_artifact(
            &serde_json::to_vec(&hypothesis_envelope).expect("hypothesis canary envelope")
        )
        .is_err());
    }
}

#[test]
fn p8_quality_admission_rejects_p7_and_p8_semantic_v1_injection() {
    for bytes in [
        br#"{"schema":"beetle-memory.p7.cohort.v1"}"#.as_slice(),
        br#"{"artifact_kind":"p7_frozen_runner_identity","artifact":null}"#.as_slice(),
        br#"{"artifact_kind":"semantic_question_detail","artifact":{"schema":"beetle-memory.p8.semantic-question-detail.v1"}}"#.as_slice(),
        br#"{"artifact_kind":"semantic_shard_manifest","artifact":{"schema":"beetle-memory.p8.semantic-shard-manifest.v1"}}"#.as_slice(),
        br#"{"artifact_kind":"semantic_operator_report","artifact":{"schema":"beetle-memory.p8.semantic-operator-report.v1"}}"#.as_slice(),
    ] {
        assert!(admit_p8_quality_artifact(bytes).is_err());
    }

    let envelopes = valid_quality_envelopes();
    let kinds = envelopes
        .iter()
        .map(|envelope| {
            serde_json::to_value(envelope).expect("envelope")["artifact_kind"]
                .as_str()
                .expect("artifact kind")
                .to_string()
        })
        .collect::<Vec<_>>();
    for (index, envelope) in envelopes.iter().enumerate() {
        for injected_schema in [
            "beetle-memory.p7.cohort.v1",
            "beetle-memory.p8.semantic-question-detail.v1",
        ] {
            let mut injected = serde_json::to_value(envelope).expect("envelope");
            injected["artifact"]["schema"] = serde_json::json!(injected_schema);
            assert!(
                admit_p8_quality_artifact(
                    &serde_json::to_vec(&injected).expect("recognized injection")
                )
                .is_err(),
                "{index}:{injected_schema}"
            );
        }

        let mut kind_mismatch = serde_json::to_value(envelope).expect("envelope");
        kind_mismatch["artifact_kind"] =
            serde_json::json!(kinds[(index + 1) % kinds.len()].clone());
        assert!(
            admit_p8_quality_artifact(&serde_json::to_vec(&kind_mismatch).expect("kind mismatch"))
                .is_err(),
            "kind mismatch {index}"
        );
    }
}

#[test]
fn p8_hard_policy_is_exact_and_source_fingerprint_bound() {
    let policy = P8QualityHardPolicyV1::canonical();
    assert!(policy.validate_contract().is_empty());
    assert_eq!(policy.rules().len(), 11);
    for (index, (gate_id, requirement, contract_ref)) in
        hard_policy_oracle().into_iter().enumerate()
    {
        let rule = &policy.rules()[index];
        assert_eq!(rule.gate_id, gate_id);
        assert_eq!(rule.requirement, requirement);
        assert_eq!(rule.validator_contract_ref, contract_ref);
        assert_eq!(
            rule.validator_source_fingerprint.as_str(),
            format!("sha256:{}", hard_policy_source_oracle(gate_id))
        );
        assert_eq!(
            rule.validator_source_attestation,
            P8ValidatorSourceAttestationV1::compiled()
        );
    }

    for index in 0..11 {
        for field in [
            "requirement",
            "validator_contract_ref",
            "validator_source_fingerprint",
            "validator_source_attestation",
            "validator_contract_digest",
        ] {
            let mut drift = serde_json::to_value(&policy).expect("policy");
            drift["rules"][index][field] = match field {
                "requirement" => {
                    if index == 6 || index == 9 {
                        serde_json::json!("exact_zero")
                    } else {
                        serde_json::json!("exact_full")
                    }
                }
                "validator_contract_ref" => serde_json::json!("wrong.contract"),
                "validator_source_attestation" => serde_json::json!("packaged_unattested"),
                _ => serde_json::json!(digest('f')),
            };
            let drift: P8QualityHardPolicyV1 =
                serde_json::from_value(drift).expect("strict policy shape");
            assert!(!drift.validate_contract().is_empty(), "{index}:{field}");
        }
    }
    for mutation in ["missing", "duplicate", "order", "policy_digest"] {
        let mut drift = serde_json::to_value(&policy).expect("policy");
        match mutation {
            "missing" => {
                drift["rules"].as_array_mut().expect("rules").pop();
            }
            "duplicate" => {
                let duplicate = drift["rules"][0].clone();
                drift["rules"]
                    .as_array_mut()
                    .expect("rules")
                    .push(duplicate);
            }
            "order" => drift["rules"].as_array_mut().expect("rules").swap(0, 1),
            "policy_digest" => {
                drift["policy_digest"] =
                    serde_json::json!(P8HardPolicyRef::derive_for_test("wrong-policy"))
            }
            _ => unreachable!(),
        }
        let drift: P8QualityHardPolicyV1 =
            serde_json::from_value(drift).expect("strict policy shape");
        assert!(!drift.validate_contract().is_empty(), "{mutation}");
    }
}

#[test]
fn p8_hypotheses_use_typed_axes_and_exact_ordered_membership() {
    let questions = vec![id("q-1"), id("q-2")];
    let question_set_digest =
        P8QualityDigest::derive("p8_quality_ordered_question_set_v1", &questions);
    let registry = hypotheses(&questions);
    assert!(registry.validate_contract().is_empty());

    let reversed = P8QualityHypothesisSpecV1::new(
        id("reversed"),
        P8HypothesisRoleV1::target(),
        vec![P8HypothesisAxisV1::Family(
            P8BenchmarkFamily::BeetleInternal,
        )],
        vec![
            P8HypothesisQuestionMembershipV1::included(id("q-2"), question_set_digest.clone()),
            P8HypothesisQuestionMembershipV1::excluded(
                id("q-1"),
                question_set_digest,
                P8MembershipExclusionReasonV1::OutsideRegisteredSlice,
            ),
        ],
    )
    .expect("locally valid");
    let question_expectations = registry.question_expectations.clone();
    assert!(
        P8QualityHypothesisRegistryV1::build(questions, question_expectations, vec![reversed])
            .is_err()
    );

    for mutation in [
        "empty_questions",
        "duplicate_question",
        "duplicate_hypothesis",
        "empty_axes",
        "duplicate_axis",
        "missing_membership",
        "duplicate_membership",
        "membership_digest",
        "question_expectation_missing",
        "question_expectation_duplicate",
        "question_expectation_order",
        "question_expectation_digest",
        "inconsistent_expected",
        "correction_digest",
        "registry_digest",
    ] {
        let mut drift = serde_json::to_value(&registry).expect("registry");
        match mutation {
            "empty_questions" => drift["ordered_questions"] = serde_json::json!([]),
            "duplicate_question" => {
                let duplicate = drift["ordered_questions"][0].clone();
                drift["ordered_questions"]
                    .as_array_mut()
                    .expect("questions")
                    .push(duplicate);
            }
            "duplicate_hypothesis" => {
                let duplicate = drift["hypotheses"][0].clone();
                drift["hypotheses"]
                    .as_array_mut()
                    .expect("hypotheses")
                    .push(duplicate);
            }
            "empty_axes" => drift["hypotheses"][0]["axes"] = serde_json::json!([]),
            "duplicate_axis" => {
                let duplicate = drift["hypotheses"][0]["axes"][0].clone();
                drift["hypotheses"][0]["axes"]
                    .as_array_mut()
                    .expect("axes")
                    .push(duplicate);
            }
            "missing_membership" => {
                drift["hypotheses"][0]["memberships"]
                    .as_array_mut()
                    .expect("memberships")
                    .pop();
            }
            "duplicate_membership" => {
                let duplicate = drift["hypotheses"][0]["memberships"][0].clone();
                drift["hypotheses"][0]["memberships"]
                    .as_array_mut()
                    .expect("memberships")
                    .push(duplicate);
            }
            "membership_digest" => {
                drift["hypotheses"][0]["memberships"][0]["ordered_question_set_digest"] =
                    serde_json::json!(digest('f'));
            }
            "question_expectation_missing" => {
                drift["question_expectations"]
                    .as_array_mut()
                    .expect("question expectations")
                    .pop();
            }
            "question_expectation_duplicate" => {
                let duplicate = drift["question_expectations"][0].clone();
                drift["question_expectations"]
                    .as_array_mut()
                    .expect("question expectations")
                    .push(duplicate);
            }
            "question_expectation_order" => drift["question_expectations"]
                .as_array_mut()
                .expect("question expectations")
                .swap(0, 1),
            "question_expectation_digest" => {
                drift["question_expectations"][0]["ordered_question_set_digest"] =
                    serde_json::json!(digest('f'));
            }
            "inconsistent_expected" => {
                drift["question_expectations"][0]["expected_capability_outcomes"]["accuracy"] =
                    serde_json::json!("not_applicable");
            }
            "correction_digest" => {
                drift["correction_family_digest"] = serde_json::json!(digest('f'));
            }
            "registry_digest" => {
                drift["registry_digest"] =
                    serde_json::json!(P8HypothesisRegistryRef::derive_for_test("wrong"));
            }
            _ => unreachable!(),
        }
        let drift: P8QualityHypothesisRegistryV1 =
            serde_json::from_value(drift).expect("strict registry shape");
        assert!(!drift.validate_contract().is_empty(), "{mutation}");
    }

    let mut malformed_role = serde_json::to_value(&registry).expect("registry");
    malformed_role["hypotheses"][0]["role"]["target"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<P8QualityHypothesisRegistryV1>(malformed_role).is_err());

    let mut malformed_disposition = serde_json::to_value(&registry).expect("registry");
    malformed_disposition["hypotheses"][0]["memberships"][0]["disposition"] =
        serde_json::json!({"excluded":{"reason":"obsolete","unexpected":true}});
    assert!(
        serde_json::from_value::<P8QualityHypothesisRegistryV1>(malformed_disposition).is_err()
    );

    let mut membership_with_expected = serde_json::to_value(&registry).expect("registry");
    membership_with_expected["hypotheses"][0]["memberships"][0]["expected_capability_outcomes"] =
        serde_json::to_value(P8ExpectedCapabilityOutcomesV1::current_procedural())
            .expect("expected outcomes");
    assert!(
        serde_json::from_value::<P8QualityHypothesisRegistryV1>(membership_with_expected).is_err()
    );
}

#[test]
fn p8_question_expectations_are_independent_from_hypothesis_membership() {
    let source = SourceFixture::new();
    let questions = vec![id("q-1"), id("q-2")];
    let question_set_digest =
        P8QualityDigest::derive("p8_quality_ordered_question_set_v1", &questions);
    let seed_protocol = protocol(
        &source.baseline_set(),
        &questions,
        P8ExecutionMode::FixtureContract,
    );
    let globally_excluding_hypothesis = P8QualityHypothesisSpecV1::new(
        id("dynamic-state"),
        P8HypothesisRoleV1::target(),
        vec![P8HypothesisAxisV1::Capability(
            P8CapabilitySlice::DynamicState,
        )],
        vec![
            P8HypothesisQuestionMembershipV1::included(
                questions[0].clone(),
                question_set_digest.clone(),
            ),
            P8HypothesisQuestionMembershipV1::excluded(
                questions[1].clone(),
                question_set_digest,
                P8MembershipExclusionReasonV1::OutsideRegisteredSlice,
            ),
        ],
    )
    .expect("hypothesis");
    let registry = P8QualityHypothesisRegistryV1::build(
        questions.clone(),
        seed_protocol
            .frozen
            .hypothesis_registry
            .question_expectations
            .clone(),
        vec![globally_excluding_hypothesis],
    )
    .expect("registry with a globally excluded question");
    assert!(registry.expected_outcomes_for(&questions[1]).is_some());

    let mut frozen = seed_protocol.frozen;
    frozen.hypothesis_registry = registry;
    let protocol = P8EvaluationProtocolLockV1::build(frozen).expect("protocol");
    let plan = P8QualityExperimentPlanV1::build(
        P8QualityPurpose::BaselineEstablishment,
        P8ExecutionMode::FixtureContract,
        source.baseline_set(),
        P8QualityHardPolicyV1::canonical(),
        protocol,
        P8QualityTrialClosureV1::derive(
            P8QualityPurpose::BaselineEstablishment,
            questions.clone(),
            2,
            3,
        )
        .expect("closure"),
        None,
        None,
        None,
    )
    .expect("plan");
    let trials = P8QualityTrialSetV1::fixture(&plan);
    assert!(trials.validate_contract().is_empty());
    assert!(trials
        .main_trials
        .iter()
        .any(|trial| trial.key.question_id == questions[1]));
}

#[test]
fn p8_capability_score_freezes_denominator_and_refusal_reason() {
    let questions = vec![id("q-1"), id("q-2")];
    let expected = BTreeMap::from([
        (
            id("q-1"),
            P8ExpectedCapabilityOutcomesV1::current_procedural(),
        ),
        (
            id("q-2"),
            P8ExpectedCapabilityOutcomesV1::obsolete_rejected(),
        ),
    ]);
    let actual = expected
        .clone()
        .into_iter()
        .map(|(key, value)| (key, value.into_actual()))
        .collect::<BTreeMap<_, _>>();
    let accuracy = BTreeMap::from([
        (id("q-1"), P8AccuracyOutcomeV1::Correct),
        (
            id("q-2"),
            P8AccuracyOutcomeV1::ExpectedRefusal {
                reason: P8ExpectedRefusalReasonV1::Obsolete,
            },
        ),
    ]);
    let registry = hypotheses_with_expected(&questions, &expected);
    let score = derive_capability_score(&registry, &id("dynamic-state"), &actual, &accuracy)
        .expect("score");
    assert_eq!(
        (score.memory_use.successes(), score.memory_use.denominator()),
        (2, 2)
    );
    assert_eq!(
        (score.procedural.successes(), score.procedural.denominator()),
        (2, 2)
    );

    let mut wrong_reason = accuracy.clone();
    wrong_reason.insert(
        id("q-2"),
        P8AccuracyOutcomeV1::ExpectedRefusal {
            reason: P8ExpectedRefusalReasonV1::Invalidated,
        },
    );
    assert_eq!(
        derive_capability_score(&registry, &id("dynamic-state"), &actual, &wrong_reason)
            .expect("closed score")
            .memory_use
            .successes(),
        2
    );
    let mut wrong_current_accuracy = accuracy.clone();
    wrong_current_accuracy.insert(id("q-1"), P8AccuracyOutcomeV1::Incorrect);
    let wrong_current = derive_capability_score(
        &registry,
        &id("dynamic-state"),
        &actual,
        &wrong_current_accuracy,
    )
    .expect("closed score");
    assert_eq!(wrong_current.memory_use.successes(), 1);
    assert_eq!(wrong_current.procedural.successes(), 1);

    let mut not_applicable = accuracy;
    not_applicable.insert(id("q-1"), P8AccuracyOutcomeV1::NotApplicable);
    assert!(
        derive_capability_score(&registry, &id("dynamic-state"), &actual, &not_applicable).is_err()
    );

    let mut missing = actual.clone();
    missing.remove(&id("q-2"));
    assert!(
        derive_capability_score(&registry, &id("dynamic-state"), &missing, &wrong_reason).is_err()
    );

    let mut extra_actual = actual.clone();
    extra_actual.insert(
        id("q-3"),
        P8ExpectedCapabilityOutcomesV1::current_procedural().into_actual(),
    );
    assert!(derive_capability_score(
        &registry,
        &id("dynamic-state"),
        &extra_actual,
        &wrong_reason
    )
    .is_err());
    let mut missing_accuracy = wrong_reason.clone();
    missing_accuracy.remove(&id("q-1"));
    assert!(
        derive_capability_score(&registry, &id("dynamic-state"), &actual, &missing_accuracy)
            .is_err()
    );
    let mut extra_accuracy = wrong_reason.clone();
    extra_accuracy.insert(id("q-3"), P8AccuracyOutcomeV1::Correct);
    assert!(
        derive_capability_score(&registry, &id("dynamic-state"), &actual, &extra_accuracy).is_err()
    );
    assert!(
        derive_capability_score(&registry, &id("unknown-hypothesis"), &actual, &wrong_reason)
            .is_err()
    );

    for axis in ["memory_use", "lineage", "premise", "procedural"] {
        let mut mismatch = actual.clone();
        let q1 = mismatch.get_mut(&id("q-1")).expect("q-1");
        match axis {
            "memory_use" => q1.memory_use = P8MemoryUseOutcomeV1::NotApplicable,
            "lineage" => q1.lineage = P8LineageOutcomeV1::Exact,
            "premise" => q1.premise = P8PremiseOutcomeV1::SatisfiedDelivered,
            "procedural" => q1.procedural = P8ProceduralOutcomeV1::NotApplicable,
            _ => unreachable!(),
        }
        assert!(
            derive_capability_score(&registry, &id("dynamic-state"), &mismatch, &wrong_reason)
                .is_err(),
            "{axis}"
        );
    }

    let mut unexpected_refusal = wrong_reason.clone();
    unexpected_refusal.insert(
        id("q-1"),
        P8AccuracyOutcomeV1::UnexpectedRefusal {
            reason: P8UnexpectedRefusalReasonV1::ProviderRefused,
        },
    );
    let unexpected = derive_capability_score(
        &registry,
        &id("dynamic-state"),
        &actual,
        &unexpected_refusal,
    )
    .expect("typed failure remains scoreable");
    assert_eq!(unexpected.memory_use.successes(), 1);
    assert_eq!(unexpected.procedural.successes(), 1);

    for bytes in [
        br#"{"expected_refusal":{"reason":"obsolete","unexpected":true}}"#.as_slice(),
        br#"{"expected_refusal":{}}"#.as_slice(),
        br#"{"unexpected_refusal":{"reason":"provider_refused","reason":"judge_refused"}}"#
            .as_slice(),
    ] {
        assert!(deserialize_p8_quality_artifact::<P8AccuracyOutcomeV1>(bytes).is_err());
    }

    let expected_outcomes = P8ExpectedCapabilityOutcomesV1::current_procedural();
    for mutation in ["missing", "extra"] {
        let mut value = serde_json::to_value(&expected_outcomes).expect("expected outcomes");
        match mutation {
            "missing" => {
                value
                    .as_object_mut()
                    .expect("expected object")
                    .remove("procedural");
            }
            "extra" => value["unexpected"] = serde_json::json!(true),
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<P8ExpectedCapabilityOutcomesV1>(value).is_err(),
            "{mutation}"
        );
    }
    let actual_outcomes = expected_outcomes.into_actual();
    for mutation in ["missing", "extra"] {
        let mut value = serde_json::to_value(&actual_outcomes).expect("actual outcomes");
        match mutation {
            "missing" => {
                value
                    .as_object_mut()
                    .expect("actual object")
                    .remove("lineage");
            }
            "extra" => value["unexpected"] = serde_json::json!(true),
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<P8ActualCapabilityOutcomesV1>(value).is_err(),
            "{mutation}"
        );
    }
    for bytes in [
        br#"{"inexact":{"reason":"gap","unexpected":true}}"#.as_slice(),
        br#"{"inexact":{}}"#.as_slice(),
        br#"{"memory_use":"current_used","memory_use":"obsolete_used","lineage":"not_applicable","premise":"not_applicable","procedural":"safe_evidence_delivered"}"#.as_slice(),
    ] {
        if bytes.starts_with(br#"{"inexact"#) {
            assert!(deserialize_p8_quality_artifact::<P8LineageOutcomeV1>(bytes).is_err());
        } else {
            assert!(
                deserialize_p8_quality_artifact::<P8ActualCapabilityOutcomesV1>(bytes).is_err()
            );
        }
    }
}

#[test]
fn p8_fixed_failure_outcomes_never_score_as_success() {
    let question = id("q-1");
    let cases = [
        (
            P8MemoryUseOutcomeV1::ObsoleteUsed,
            P8PremiseOutcomeV1::NotApplicable,
            P8ProceduralOutcomeV1::NotApplicable,
        ),
        (
            P8MemoryUseOutcomeV1::InvalidatedUsed,
            P8PremiseOutcomeV1::NotApplicable,
            P8ProceduralOutcomeV1::NotApplicable,
        ),
        (
            P8MemoryUseOutcomeV1::ForgottenUsed,
            P8PremiseOutcomeV1::NotApplicable,
            P8ProceduralOutcomeV1::NotApplicable,
        ),
        (
            P8MemoryUseOutcomeV1::NotApplicable,
            P8PremiseOutcomeV1::RequiredUnmetDelivered,
            P8ProceduralOutcomeV1::NotApplicable,
        ),
        (
            P8MemoryUseOutcomeV1::NotApplicable,
            P8PremiseOutcomeV1::SatisfiedRefused,
            P8ProceduralOutcomeV1::NotApplicable,
        ),
        (
            P8MemoryUseOutcomeV1::NotApplicable,
            P8PremiseOutcomeV1::NotApplicable,
            P8ProceduralOutcomeV1::MissingEvidence,
        ),
        (
            P8MemoryUseOutcomeV1::NotApplicable,
            P8PremiseOutcomeV1::NotApplicable,
            P8ProceduralOutcomeV1::UnsafeEvidenceDelivered,
        ),
    ];
    for (memory_use, premise, procedural) in cases {
        let expected_outcome = P8ExpectedCapabilityOutcomesV1 {
            accuracy: P8ExpectedAccuracyV1::Correct,
            memory_use,
            lineage: P8LineageOutcomeV1::NotApplicable,
            premise,
            procedural,
        };
        let expected = BTreeMap::from([(question.clone(), expected_outcome.clone())]);
        let registry = hypotheses_with_expected(std::slice::from_ref(&question), &expected);
        let actual = BTreeMap::from([(question.clone(), expected_outcome.into_actual())]);
        let accuracy = BTreeMap::from([(question.clone(), P8AccuracyOutcomeV1::Correct)]);
        let score = derive_capability_score(&registry, &id("dynamic-state"), &actual, &accuracy)
            .expect("fixed failure stays in denominator");
        let failed_axis_success = match (memory_use, premise, procedural) {
            (P8MemoryUseOutcomeV1::ObsoleteUsed, _, _)
            | (P8MemoryUseOutcomeV1::InvalidatedUsed, _, _)
            | (P8MemoryUseOutcomeV1::ForgottenUsed, _, _) => score.memory_use.successes,
            (_, P8PremiseOutcomeV1::RequiredUnmetDelivered, _)
            | (_, P8PremiseOutcomeV1::SatisfiedRefused, _) => score.premise.successes,
            (_, _, P8ProceduralOutcomeV1::MissingEvidence)
            | (_, _, P8ProceduralOutcomeV1::UnsafeEvidenceDelivered) => score.procedural.successes,
            _ => unreachable!("fixed failure case"),
        };
        assert_eq!(failed_axis_success, 0);
    }
}

#[test]
fn p8_engineering_gate_rejects_legacy_aggregate_only_receipt() {
    let source = P8HarnessSourceInputManifestV1::fixture(&digest('1'));
    let legacy = serde_json::json!({
        "schema": "beetle-memory.p8.quality-engineering-gate-receipt.v1",
        "source_input_digest": source.source_input_digest(),
        "toolchain_digest": source.toolchain_digest(),
        "build_fingerprint": source.build_contract_digest(),
        "passed_gates": [
            "format",
            "unit_tests",
            "clippy",
            "workspace_check"
        ],
        "evidence": "fixture_contract_only",
        "receipt_digest": format!(
            "p8_engineering_gate_receipt:sha256:{}",
            "0".repeat(64)
        )
    });

    assert!(
        serde_json::from_value::<P8EngineeringGateReceiptV1>(legacy).is_err(),
        "a four-name aggregate without parent-observed command closures must be rejected"
    );
}

#[test]
fn p8_source_release_chain_is_typed_exact_and_alias_free() {
    let source = SourceFixture::new();
    let baseline = source.baseline_set();
    let candidate = source.candidate_set();
    assert_eq!(baseline.arms().len(), 3);
    assert_eq!(candidate.arms().len(), 4);
    assert!(baseline.validate_contract().is_empty());
    assert!(candidate.validate_contract().is_empty());

    let missing_candidate = P8SourceReleaseSetV1::build(
        P8QualityPurpose::QualityCandidate,
        source.harness.clone(),
        vec![
            source.no_memory.clone(),
            source.public_reference.clone(),
            source.frozen.clone(),
        ],
    );
    assert!(missing_candidate.is_err());

    let mut missing_role = serde_json::to_value(&source.harness).expect("harness json");
    missing_role["roles"].as_array_mut().expect("roles").pop();
    let missing_role: P8HarnessReleaseManifestV1 =
        serde_json::from_value(missing_role).expect("shape");
    assert!(missing_role
        .validate_contract()
        .contains(&P8QualityContractFailure::RoleSetMismatch));

    for mutation in [
        "duplicate_role",
        "role_order",
        "role_executable_alias",
        "role_receipt_alias",
        "common_source_drift",
        "common_component_missing",
        "common_component_duplicate",
        "common_component_order",
        "fixture_evidence_boundary",
        "common_inventory_digest",
    ] {
        let mut drift = serde_json::to_value(&source.harness).expect("harness json");
        match mutation {
            "duplicate_role" => {
                let duplicate = drift["roles"][0].clone();
                drift["roles"]
                    .as_array_mut()
                    .expect("roles")
                    .push(duplicate);
            }
            "role_order" => drift["roles"].as_array_mut().expect("roles").swap(0, 1),
            "role_executable_alias" => {
                drift["roles"][1]["executable_digest"] =
                    drift["roles"][0]["executable_digest"].clone();
            }
            "role_receipt_alias" => {
                drift["roles"][1]["sealed_execution_receipt"] =
                    drift["roles"][0]["sealed_execution_receipt"].clone();
            }
            "common_source_drift" => {
                drift["source_input"]["common_harness_semantic_source"]["source_inventory"][1]
                    ["source_digest"] = serde_json::json!(digest('f'));
            }
            "common_component_missing" => {
                drift["source_input"]["common_harness_semantic_source"]["source_inventory"]
                    .as_array_mut()
                    .expect("components")
                    .pop();
            }
            "common_component_duplicate" => {
                let duplicate = drift["source_input"]["common_harness_semantic_source"]
                    ["source_inventory"][0]
                    .clone();
                drift["source_input"]["common_harness_semantic_source"]["source_inventory"]
                    .as_array_mut()
                    .expect("components")
                    .push(duplicate);
            }
            "common_component_order" => drift["source_input"]["common_harness_semantic_source"]
                ["source_inventory"]
                .as_array_mut()
                .expect("components")
                .swap(0, 1),
            "fixture_evidence_boundary" => {
                drift["source_input"]["common_harness_semantic_source"]["evidence_boundary"] =
                    serde_json::json!("trusted_source_and_exclusion_proof");
            }
            "common_inventory_digest" => {
                drift["source_input"]["common_harness_semantic_source"]["inventory_digest"] =
                    serde_json::json!(digest('f'));
            }
            _ => unreachable!(),
        }
        match serde_json::from_value::<P8HarnessReleaseManifestV1>(drift) {
            Ok(drift) => assert!(!drift.validate_contract().is_empty(), "{mutation}"),
            Err(_) => assert_eq!(mutation, "fixture_evidence_boundary"),
        }
    }

    assert!(P8SourceReleaseSetV1::build(
        P8QualityPurpose::BaselineEstablishment,
        source.harness.clone(),
        vec![
            source.no_memory.clone(),
            source.public_reference.clone(),
            source.frozen.clone(),
            source.candidate.clone(),
        ],
    )
    .is_err());
    assert!(P8SourceReleaseSetV1::build(
        P8QualityPurpose::BaselineEstablishment,
        source.harness.clone(),
        vec![
            source.no_memory.clone(),
            source.no_memory.clone(),
            source.public_reference.clone(),
            source.frozen.clone(),
        ],
    )
    .is_err());

    for mutation in [
        "missing_arm",
        "wrong_purpose",
        "trusted_without_authority",
        "release_set_digest",
    ] {
        let mut drift = serde_json::to_value(&candidate).expect("candidate set");
        match mutation {
            "missing_arm" => {
                drift["arms"]
                    .as_object_mut()
                    .expect("arms")
                    .remove("p8_candidate");
            }
            "wrong_purpose" => {
                drift["purpose"] = serde_json::json!("baseline_establishment");
            }
            "trusted_without_authority" => {
                drift["evidence_class"] = serde_json::json!("trusted_sealed");
            }
            "release_set_digest" => {
                drift["release_set_digest"] =
                    serde_json::json!(P8SourceReleaseSetRef::derive_for_test("wrong"));
            }
            _ => unreachable!(),
        }
        let drift: P8SourceReleaseSetV1 =
            serde_json::from_value(drift).expect("strict source-set shape");
        assert!(!drift.validate_contract().is_empty(), "{mutation}");
    }
}

#[test]
fn p8_source_release_rejects_toolchain_receipt_and_underlying_alias_drift() {
    let source = SourceFixture::new();
    let mut toolchain_drift = serde_json::to_value(&source.harness).expect("harness json");
    toolchain_drift["source_input"]["toolchain_digest"] = serde_json::json!(digest('f'));
    let toolchain_drift: P8HarnessReleaseManifestV1 =
        serde_json::from_value(toolchain_drift).expect("shape");
    assert!(!toolchain_drift.validate_contract().is_empty());

    let frozen_executable = source.frozen.executable_digest().clone();
    let candidate_input = P8ArmImplementationInputManifestV1::beetle(
        P8QualityArmKind::P8Candidate,
        P8SemanticSourceAnchorRef::derive_for_test("candidate-alias"),
    )
    .expect("candidate input");
    let receipt = source.harness.arm_sealed_execution_receipt(
        &candidate_input,
        frozen_executable.clone(),
        digest('e'),
    );
    let aliased_candidate = P8ArmImplementationReleaseV1::beetle(
        candidate_input,
        &source.harness,
        frozen_executable,
        receipt,
    )
    .expect("locally valid candidate");
    assert!(P8SourceReleaseSetV1::build(
        P8QualityPurpose::QualityCandidate,
        source.harness,
        vec![
            source.no_memory,
            source.public_reference,
            source.frozen,
            aliased_candidate,
        ],
    )
    .is_err());

    assert!(P8ArmImplementationInputManifestV1::beetle(
        P8QualityArmKind::P8Candidate,
        anchor().anchor_digest().clone(),
    )
    .is_err());
}

#[test]
fn p8_trial_closure_materializes_exact_three_and_four_arm_key_sets() {
    let questions = vec![id("q-1"), id("q-2")];
    let baseline = P8QualityTrialClosureV1::derive(
        P8QualityPurpose::BaselineEstablishment,
        questions.clone(),
        2,
        3,
    )
    .expect("baseline closure");
    assert_eq!(baseline.expected_main_trial_keys().len(), 36);
    assert_eq!(baseline.expected_ablation_keys().len(), 60);
    assert_eq!(baseline.expected_negative_proof_keys().len(), 6);
    assert_eq!(
        baseline
            .expected_main_trial_keys()
            .into_iter()
            .collect::<BTreeSet<_>>()
            .len(),
        36
    );

    let candidate =
        P8QualityTrialClosureV1::derive(P8QualityPurpose::QualityCandidate, questions, 2, 3)
            .expect("candidate closure");
    assert_eq!(candidate.expected_main_trial_keys().len(), 48);
    assert_eq!(candidate.expected_ablation_keys().len(), 120);
    assert_eq!(candidate.expected_negative_proof_keys().len(), 12);
}

#[test]
fn p8_trial_closure_rejects_zero_even_and_duplicate_schedule() {
    for (reader_repeats, judge_repeats) in [(0, 3), (1, 0), (1, 2)] {
        assert!(P8QualityTrialClosureV1::derive(
            P8QualityPurpose::BaselineEstablishment,
            vec![id("q-1")],
            reader_repeats,
            judge_repeats,
        )
        .is_err());
    }
    assert!(P8QualityTrialClosureV1::derive(
        P8QualityPurpose::BaselineEstablishment,
        vec![id("q-1"), id("q-1")],
        1,
        3,
    )
    .is_err());

    let closure = P8QualityTrialClosureV1::derive(
        P8QualityPurpose::QualityCandidate,
        vec![id("q-1"), id("q-2")],
        2,
        3,
    )
    .expect("closure");
    for mutation in [
        "purpose",
        "arms_missing",
        "arms_duplicate",
        "arms_order",
        "counterfactual_missing",
        "counterfactual_duplicate",
        "counterfactual_order",
        "proof_missing",
        "proof_duplicate",
        "proof_order",
        "main_count",
        "ablation_count",
        "proof_count",
        "digest",
    ] {
        let mut drift = serde_json::to_value(&closure).expect("closure");
        match mutation {
            "purpose" => drift["purpose"] = serde_json::json!("baseline_establishment"),
            "arms_missing" => {
                drift["applicable_arms"].as_array_mut().expect("arms").pop();
            }
            "arms_duplicate" => {
                let duplicate = drift["applicable_arms"][0].clone();
                drift["applicable_arms"]
                    .as_array_mut()
                    .expect("arms")
                    .push(duplicate);
            }
            "arms_order" => drift["applicable_arms"]
                .as_array_mut()
                .expect("arms")
                .swap(0, 1),
            "counterfactual_missing" => {
                drift["safe_counterfactuals"]
                    .as_array_mut()
                    .expect("counterfactuals")
                    .pop();
            }
            "counterfactual_duplicate" => {
                let duplicate = drift["safe_counterfactuals"][0].clone();
                drift["safe_counterfactuals"]
                    .as_array_mut()
                    .expect("counterfactuals")
                    .push(duplicate);
            }
            "counterfactual_order" => drift["safe_counterfactuals"]
                .as_array_mut()
                .expect("counterfactuals")
                .swap(0, 1),
            "proof_missing" => {
                drift["negative_proofs"]
                    .as_array_mut()
                    .expect("proofs")
                    .pop();
            }
            "proof_duplicate" => {
                let duplicate = drift["negative_proofs"][0].clone();
                drift["negative_proofs"]
                    .as_array_mut()
                    .expect("proofs")
                    .push(duplicate);
            }
            "proof_order" => drift["negative_proofs"]
                .as_array_mut()
                .expect("proofs")
                .swap(0, 1),
            "main_count" => drift["main_trial_count"] = serde_json::json!(47),
            "ablation_count" => drift["safe_ablation_count"] = serde_json::json!(119),
            "proof_count" => drift["negative_proof_count"] = serde_json::json!(11),
            "digest" => {
                drift["closure_digest"] =
                    serde_json::json!(P8TrialClosureRef::derive_for_test("wrong"))
            }
            _ => unreachable!(),
        }
        let drift: P8QualityTrialClosureV1 =
            serde_json::from_value(drift).expect("strict closure shape");
        assert!(!drift.validate_contract().is_empty(), "{mutation}");
    }
}

#[test]
fn p8_actual_trial_set_is_exact_typed_and_fixture_only() {
    for purpose in [
        P8QualityPurpose::BaselineEstablishment,
        P8QualityPurpose::QualityCandidate,
    ] {
        let plan = fixture_plan(purpose);
        let trial_set = P8QualityTrialSetV1::fixture(&plan);
        assert!(trial_set.validate_contract().is_empty());
        assert!(trial_set.validate_against(&plan).is_empty());
        assert_eq!(
            trial_set.main_trials.len(),
            usize::try_from(plan.trial_closure.main_trial_count()).expect("count")
        );
        assert_eq!(
            trial_set.ablation_trials.len(),
            usize::try_from(plan.trial_closure.safe_ablation_count()).expect("count")
        );
        assert_eq!(
            trial_set.negative_proof_trials.len(),
            usize::try_from(plan.trial_closure.negative_proof_count()).expect("count")
        );
        assert_eq!(
            trial_set.main_trials[0].arm_receipt,
            trial_set.main_trials[1].arm_receipt
        );
        assert_eq!(
            trial_set.main_trials[0]
                .execution_chain
                .model_execution_receipt,
            trial_set.main_trials[1]
                .execution_chain
                .model_execution_receipt
        );
        assert_ne!(
            trial_set.main_trials[0]
                .execution_chain
                .judge_execution_receipt,
            trial_set.main_trials[1]
                .execution_chain
                .judge_execution_receipt
        );

        for mutation in [
            "main_missing",
            "main_duplicate",
            "main_order",
            "main_run",
            "main_release",
            "main_arm_receipt",
            "main_composition_receipt",
            "main_model_receipt",
            "main_join_receipt",
            "main_judge_receipt",
            "main_attempt_latency_receipt",
            "main_question_latency_receipt",
            "ablation_missing",
            "ablation_duplicate",
            "ablation_order",
            "ablation_run",
            "ablation_release",
            "ablation_pair_alias",
            "ablation_receipt",
            "negative_missing",
            "negative_duplicate",
            "negative_order",
            "negative_run",
            "negative_release",
            "negative_receipt",
            "negative_coverage",
            "digest",
        ] {
            let mut drift = serde_json::to_value(&trial_set).expect("trial set");
            match mutation {
                "main_missing" => {
                    drift["main_trials"].as_array_mut().expect("main").pop();
                }
                "main_duplicate" => {
                    let duplicate = drift["main_trials"][0].clone();
                    drift["main_trials"]
                        .as_array_mut()
                        .expect("main")
                        .push(duplicate);
                }
                "main_order" => drift["main_trials"]
                    .as_array_mut()
                    .expect("main")
                    .swap(0, 1),
                "main_run" => {
                    drift["main_trials"][0]["run_id"] =
                        serde_json::json!(P8QualityRunRef::derive_for_test("wrong-run"));
                }
                "main_release" => {
                    drift["main_trials"][0]["arm_release_digest"] =
                        serde_json::json!(P8ArmReleaseRef::derive_for_test("wrong-release"));
                }
                "main_arm_receipt" => {
                    drift["main_trials"][0]["arm_receipt"] = serde_json::json!({"public_reference":{
                        "safe_output_receipt":
                            P8PublicSafeOutputReceiptRef::derive_for_test("wrong-arm")
                    }});
                }
                "main_model_receipt" => {
                    drift["main_trials"][0]["execution_chain"]["model_execution_receipt"] =
                        serde_json::json!(P8ModelExecutionReceiptRef::derive_for_test("wrong"));
                }
                "main_composition_receipt" => {
                    drift["main_trials"][0]["execution_chain"]["composition_receipt"] =
                        serde_json::json!(P8CompositionReceiptRef::derive_for_test("wrong"));
                }
                "main_join_receipt" => {
                    drift["main_trials"][0]["execution_chain"]["benchmark_join_receipt"] = serde_json::json!(
                        P8BenchmarkJoinExecutionReceiptRef::derive_for_test("wrong")
                    );
                }
                "main_judge_receipt" => {
                    drift["main_trials"][0]["execution_chain"]["judge_execution_receipt"] =
                        serde_json::json!(P8JudgeExecutionReceiptRef::derive_for_test("wrong"));
                }
                "main_attempt_latency_receipt" => {
                    drift["main_trials"][0]["execution_chain"]["attempt_latency_receipt"] =
                        serde_json::json!(P8AttemptLatencyReceiptRef::derive_for_test("wrong"));
                }
                "main_question_latency_receipt" => {
                    drift["main_trials"][0]["execution_chain"]["question_latency_receipt"] =
                        serde_json::json!(P8QuestionLatencyReceiptRef::derive_for_test("wrong"));
                }
                "ablation_missing" => {
                    drift["ablation_trials"]
                        .as_array_mut()
                        .expect("ablation")
                        .pop();
                }
                "ablation_duplicate" => {
                    let duplicate = drift["ablation_trials"][0].clone();
                    drift["ablation_trials"]
                        .as_array_mut()
                        .expect("ablation")
                        .push(duplicate);
                }
                "ablation_order" => drift["ablation_trials"]
                    .as_array_mut()
                    .expect("ablation")
                    .swap(0, 1),
                "ablation_run" => {
                    drift["ablation_trials"][0]["run_id"] =
                        serde_json::json!(P8QualityRunRef::derive_for_test("wrong-ablation-run"));
                }
                "ablation_release" => {
                    drift["ablation_trials"][0]["arm_release_digest"] = serde_json::json!(
                        P8ArmReleaseRef::derive_for_test("wrong-ablation-release")
                    );
                }
                "ablation_pair_alias" => {
                    drift["ablation_trials"][0]["receipt_pair"]
                        ["off_run_semantic_execution_receipt_v2"] = drift["ablation_trials"][0]
                        ["receipt_pair"]["baseline_semantic_execution_receipt_v2"]
                        .clone();
                }
                "ablation_receipt" => {
                    drift["ablation_trials"][0]["receipt_pair"]["paired_judge_receipt"] =
                        serde_json::json!(P8PairedJudgeReceiptRef::derive_for_test("wrong"));
                }
                "negative_missing" => {
                    drift["negative_proof_trials"]
                        .as_array_mut()
                        .expect("negative")
                        .pop();
                }
                "negative_duplicate" => {
                    let duplicate = drift["negative_proof_trials"][0].clone();
                    drift["negative_proof_trials"]
                        .as_array_mut()
                        .expect("negative")
                        .push(duplicate);
                }
                "negative_order" => drift["negative_proof_trials"]
                    .as_array_mut()
                    .expect("negative")
                    .swap(0, 1),
                "negative_run" => {
                    drift["negative_proof_trials"][0]["run_id"] =
                        serde_json::json!(P8QualityRunRef::derive_for_test("wrong-negative-run"));
                }
                "negative_release" => {
                    drift["negative_proof_trials"][0]["arm_release_digest"] = serde_json::json!(
                        P8ArmReleaseRef::derive_for_test("wrong-negative-release")
                    );
                }
                "negative_receipt" => {
                    drift["negative_proof_trials"][0]["proof_receipt"] =
                        serde_json::json!(P8SafetyProofReceiptRef::derive_for_test("wrong"));
                }
                "negative_coverage" => {
                    drift["negative_proof_trials"][0]["coverage"]["verified_owner_path_count"] =
                        serde_json::json!(0);
                }
                "digest" => {
                    drift["trial_set_digest"] =
                        serde_json::json!(P8QualityTrialSetRef::derive_for_test("wrong"));
                }
                _ => unreachable!(),
            }
            let drift: P8QualityTrialSetV1 =
                serde_json::from_value(drift).expect("strict trial-set shape");
            assert!(
                !drift.validate_contract().is_empty(),
                "{purpose:?}:{mutation}"
            );
        }
        let mut invalid_model_boundary = serde_json::to_value(&trial_set).expect("trial set");
        invalid_model_boundary["negative_proof_trials"][0]["model_boundary"] =
            serde_json::json!("reader_model_allowed");
        assert!(serde_json::from_value::<P8QualityTrialSetV1>(invalid_model_boundary).is_err());

        let mut observed_quality_failure = trial_set.clone();
        observed_quality_failure.main_trials[0].accuracy = P8AccuracyOutcomeV1::UnexpectedRefusal {
            reason: P8UnexpectedRefusalReasonV1::ProviderRefused,
        };
        observed_quality_failure.main_trials[0]
            .capability_outcomes
            .memory_use = P8MemoryUseOutcomeV1::CurrentRejected;
        observed_quality_failure.trial_set_digest = observed_quality_failure.derived_digest();
        assert!(
            observed_quality_failure.validate_contract().is_empty(),
            "closed actual quality failures are data, not structural corruption: {purpose:?}"
        );

        let mut invalid_applicability = trial_set.clone();
        invalid_applicability.main_trials[0]
            .capability_outcomes
            .memory_use = P8MemoryUseOutcomeV1::NotApplicable;
        invalid_applicability.trial_set_digest = invalid_applicability.derived_digest();
        assert!(invalid_applicability
            .validate_contract()
            .contains(&P8QualityContractFailure::OutcomeMismatch));
    }

    let baseline = fixture_plan(P8QualityPurpose::BaselineEstablishment);
    let candidate = fixture_plan(P8QualityPurpose::QualityCandidate);
    let baseline_trials = P8QualityTrialSetV1::fixture(&baseline);
    assert!(!baseline_trials.validate_against(&candidate).is_empty());
}

#[test]
fn p8_resource_closure_binds_trial_and_arm_run_grains() {
    for purpose in [
        P8QualityPurpose::BaselineEstablishment,
        P8QualityPurpose::QualityCandidate,
    ] {
        let plan = fixture_plan(purpose);
        let trial_set = P8QualityTrialSetV1::fixture(&plan);
        let resources = P8QualityResourceClosureV1::fixture(&trial_set);
        assert!(resources.validate_contract().is_empty());
        let reader_trials = trial_set
            .main_trials
            .iter()
            .filter(|trial| trial.key.judge_repeat_index == 0)
            .collect::<Vec<_>>();
        let beetle_reader_trials = reader_trials
            .iter()
            .filter(|trial| {
                matches!(
                    trial.key.arm,
                    P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate
                )
            })
            .count();
        assert_eq!(
            resources.observations.len(),
            reader_trials.len()
                + beetle_reader_trials
                + P8QualityArmKind::expected_for(purpose).len()
        );
        assert!(resources.observations.iter().all(|observation| {
            !matches!(
                &observation.grain,
                P8ResourceObservationGrainV1::Trial {
                    measure: P8ResourceMeasureKindV1::MemoryProjectionRenderedChars,
                    key,
                    ..
                } if matches!(
                    key.arm,
                    P8QualityArmKind::NoMemory | P8QualityArmKind::PublicReference
                )
            )
        }));

        for mutation in [
            "missing",
            "duplicate",
            "order",
            "cross_run",
            "trusted_state",
            "nested_missing_arm",
            "digest",
        ] {
            let mut drift = serde_json::to_value(&resources).expect("resources");
            match mutation {
                "missing" => {
                    drift["observations"]
                        .as_array_mut()
                        .expect("observations")
                        .pop();
                }
                "duplicate" => {
                    let duplicate = drift["observations"][0].clone();
                    drift["observations"]
                        .as_array_mut()
                        .expect("observations")
                        .push(duplicate);
                }
                "order" => drift["observations"]
                    .as_array_mut()
                    .expect("observations")
                    .swap(0, 1),
                "cross_run" => {
                    drift["observations"][0]["grain"]["trial"]["run_id"] =
                        serde_json::json!(P8QualityRunRef::derive_for_test("cross-run"));
                }
                "trusted_state" => {
                    drift["observations"][0]["state"] = serde_json::json!({
                        "trusted_observed": {
                            "value": 1,
                            "receipt_digest":
                                P8TrustedDomainResourceReceiptRef::derive_for_test("fake")
                        }
                    });
                }
                "nested_missing_arm" => {
                    let arm = if purpose == P8QualityPurpose::QualityCandidate {
                        "p8_candidate"
                    } else {
                        "frozen_p84_baseline"
                    };
                    drift["trial_set"]["experiment_plan"]["source_release_set"]["arms"]
                        .as_object_mut()
                        .expect("arms")
                        .remove(arm);
                }
                "digest" => drift["closure_digest"] = serde_json::json!(digest('f')),
                _ => unreachable!(),
            }
            let drift: P8QualityResourceClosureV1 =
                serde_json::from_value(drift).expect("strict resource shape");
            assert!(
                !drift.validate_contract().is_empty(),
                "{purpose:?}:{mutation}"
            );
        }
    }
}

#[test]
fn p8_protocol_is_purpose_neutral_and_freezes_all_rule_owners() {
    let source = SourceFixture::new();
    let questions = vec![id("q-1"), id("q-2")];
    let protocol = protocol(
        &source.baseline_set(),
        &questions,
        P8ExecutionMode::FixtureContract,
    );
    assert!(protocol.validate_contract().is_empty());
    let json = serde_json::to_string(&protocol).expect("protocol");
    assert!(!json.contains("\"purpose\""));
    assert!(!json.contains("source_release_set_digest"));
    assert!(!json.contains("threshold_digest"));

    let mut even_judge = serde_json::to_value(&protocol).expect("protocol");
    even_judge["frozen"]["trial"]["judge_repeats"] = serde_json::json!(2);
    let even_judge: P8EvaluationProtocolLockV1 = serde_json::from_value(even_judge).expect("shape");
    assert!(!even_judge.validate_contract().is_empty());

    let mut question_set_drift = serde_json::to_value(&protocol).expect("protocol");
    question_set_drift["frozen"]["dataset"]["ordered_question_ids_digest"] =
        serde_json::json!(digest('f'));
    let question_set_drift: P8EvaluationProtocolLockV1 =
        serde_json::from_value(question_set_drift).expect("shape");
    assert!(question_set_drift
        .validate_contract()
        .contains(&P8QualityContractFailure::HypothesisMismatch));
}

#[test]
fn p8_experiment_plan_enforces_baseline_candidate_boundary() {
    let source = SourceFixture::new();
    let questions = vec![id("q-1"), id("q-2")];
    let policy = P8QualityHardPolicyV1::canonical();
    let protocol = protocol(
        &source.baseline_set(),
        &questions,
        P8ExecutionMode::FixtureContract,
    );
    let baseline_closure = P8QualityTrialClosureV1::derive(
        P8QualityPurpose::BaselineEstablishment,
        questions.clone(),
        2,
        3,
    )
    .expect("baseline closure");
    let baseline = P8QualityExperimentPlanV1::build(
        P8QualityPurpose::BaselineEstablishment,
        P8ExecutionMode::FixtureContract,
        source.baseline_set(),
        policy.clone(),
        protocol.clone(),
        baseline_closure.clone(),
        None,
        None,
        None,
    )
    .expect("baseline plan");
    assert!(baseline.validate_contract().is_empty());
    assert!(P8QualityExperimentPlanV1::build(
        P8QualityPurpose::BaselineEstablishment,
        P8ExecutionMode::FixtureContract,
        source.baseline_set(),
        policy.clone(),
        protocol.clone(),
        baseline_closure,
        Some(threshold(&protocol)),
        None,
        None,
    )
    .is_err());

    let candidate_closure =
        P8QualityTrialClosureV1::derive(P8QualityPurpose::QualityCandidate, questions, 2, 3)
            .expect("candidate closure");
    let candidate_threshold = threshold(&protocol);
    let candidate_policy = P8FrozenQualityPolicyV1::build(
        protocol.protocol_digest().clone(),
        candidate_threshold.threshold_digest().clone(),
    );
    assert!(P8QualityExperimentPlanV1::build(
        P8QualityPurpose::QualityCandidate,
        P8ExecutionMode::FixtureContract,
        source.candidate_set(),
        policy.clone(),
        protocol.clone(),
        candidate_closure.clone(),
        None,
        Some(candidate_policy.clone()),
        None,
    )
    .is_err());
    assert!(P8QualityExperimentPlanV1::build(
        P8QualityPurpose::QualityCandidate,
        P8ExecutionMode::FixtureContract,
        source.candidate_set(),
        policy,
        protocol.clone(),
        candidate_closure,
        Some(candidate_threshold),
        Some(candidate_policy),
        None,
    )
    .is_ok());

    let wrong_policy = P8FrozenQualityPolicyV1::build(
        protocol.protocol_digest().clone(),
        P8ThresholdLockRef::derive_for_test("wrong-threshold"),
    );
    assert!(P8QualityExperimentPlanV1::build(
        P8QualityPurpose::QualityCandidate,
        P8ExecutionMode::FixtureContract,
        source.candidate_set(),
        P8QualityHardPolicyV1::canonical(),
        protocol.clone(),
        P8QualityTrialClosureV1::derive(
            P8QualityPurpose::QualityCandidate,
            vec![id("q-1"), id("q-2")],
            2,
            3,
        )
        .expect("candidate closure"),
        Some(threshold(&protocol)),
        Some(wrong_policy),
        None,
    )
    .is_err());
}

#[test]
fn p8_hard_gate_fixture_evaluation_is_explicitly_untrusted_and_exact() {
    let policy = P8QualityHardPolicyV1::canonical();
    let plan = fixture_plan(P8QualityPurpose::BaselineEstablishment);
    let trials = P8QualityTrialSetV1::fixture(&plan);
    let resources = P8QualityResourceClosureV1::fixture(&trials);
    let passed = P8HardGateEvaluationV1::evaluate(&policy, &resources).expect("hard gates");
    assert!(passed.hard_gate_failures.is_empty());
    assert_eq!(
        passed.evidence_scope,
        P8HardGateEvidenceScopeV1::FixtureContractOnlyNoTrustedOwnerEvidence
    );
    for gate_id in [
        P8QualityHardGateId::UpdateLineageViolation,
        P8QualityHardGateId::PostImageClosureCoverage,
    ] {
        let failed = P8HardGateEvaluationV1::evaluate_fixture_with_failures(
            &policy,
            &resources,
            vec![gate_id],
        )
        .expect("fixture hard-gate failure");
        assert_eq!(failed.hard_gate_failures, vec![gate_id]);
        assert_eq!(
            failed.evidence_scope,
            P8HardGateEvidenceScopeV1::FixtureContractOnlyNoTrustedOwnerEvidence
        );
    }
    assert!(P8HardGateEvaluationV1::evaluate_fixture_with_failures(
        &policy,
        &resources,
        vec![
            P8QualityHardGateId::UpdateLineageViolation,
            P8QualityHardGateId::UpdateLineageViolation,
        ],
    )
    .is_err());

    let mut missing = serde_json::to_value(&passed).expect("evaluation");
    missing["observations"]
        .as_array_mut()
        .expect("observations")
        .pop();
    let missing: P8HardGateEvaluationV1 = serde_json::from_value(missing).expect("shape");
    assert!(!missing.validate_contract().is_empty());

    let full_gates = [
        P8QualityHardGateId::PostImageClosureCoverage,
        P8QualityHardGateId::RequiredReportOperatorCoverage,
    ];
    for gate_id in full_gates {
        for (observed_count, required_total_count) in [(0, 1), (1, 2), (2, 1), (0, 0)] {
            let mut drift = serde_json::to_value(&passed).expect("evaluation");
            let observation = drift["observations"]
                .as_array_mut()
                .expect("observations")
                .iter_mut()
                .find(|observation| {
                    observation["gate_id"] == serde_json::to_value(gate_id).expect("gate id")
                })
                .expect("full gate");
            observation["observed_count"] = serde_json::json!(observed_count);
            observation["required_total_count"] = serde_json::json!(required_total_count);
            let drift: P8HardGateEvaluationV1 = serde_json::from_value(drift).expect("shape");
            assert!(!drift.validate_contract().is_empty());
        }
    }

    let mut duplicate = serde_json::to_value(&passed).expect("evaluation");
    let duplicate_observation = duplicate["observations"][0].clone();
    duplicate["observations"]
        .as_array_mut()
        .expect("observations")
        .push(duplicate_observation);
    let duplicate: P8HardGateEvaluationV1 = serde_json::from_value(duplicate).expect("shape");
    assert!(!duplicate.validate_contract().is_empty());

    let mut drift = serde_json::to_value(&passed).expect("evaluation");
    drift["hard_gate_failures"] = serde_json::json!(["update_lineage_violation"]);
    let drift: P8HardGateEvaluationV1 = serde_json::from_value(drift).expect("shape");
    assert!(!drift.validate_contract().is_empty());

    let mut unbound_failure = serde_json::to_value(&passed).expect("evaluation");
    unbound_failure["fixture_failed_gates"] = serde_json::json!(["update_lineage_violation"]);
    let unbound_failure: P8HardGateEvaluationV1 =
        serde_json::from_value(unbound_failure).expect("shape");
    assert!(!unbound_failure.validate_contract().is_empty());

    let mut digest_drift = serde_json::to_value(&passed).expect("evaluation");
    digest_drift["evaluation_digest"] =
        serde_json::json!(P8HardGateEvaluationRef::derive_for_test("wrong"));
    let digest_drift: P8HardGateEvaluationV1 = serde_json::from_value(digest_drift).expect("shape");
    assert!(!digest_drift.validate_contract().is_empty());
}

#[test]
fn p8_operator_state_uses_validated_artifacts_not_caller_bools() {
    assert_eq!(
        derive_operator_state(P8OperatorExecutionInputV1::NotRun),
        P8QualityOperatorStateV1::NotRun
    );
    assert!(matches!(
        derive_operator_state(P8OperatorExecutionInputV1::Blocked {
            reason: id("provider-unavailable")
        }),
        P8QualityOperatorStateV1::Blocked { .. }
    ));
    assert!(matches!(
        derive_operator_state(P8OperatorExecutionInputV1::StructurallyInvalid {
            reason: id("artifact-invalid")
        }),
        P8QualityOperatorStateV1::StructurallyInvalid { .. }
    ));

    let source = SourceFixture::new();
    let questions = vec![id("q-1"), id("q-2")];
    let policy = P8QualityHardPolicyV1::canonical();
    let fixture_protocol = protocol(
        &source.baseline_set(),
        &questions,
        P8ExecutionMode::FixtureContract,
    );
    let baseline_plan = P8QualityExperimentPlanV1::build(
        P8QualityPurpose::BaselineEstablishment,
        P8ExecutionMode::FixtureContract,
        source.baseline_set(),
        policy.clone(),
        fixture_protocol.clone(),
        P8QualityTrialClosureV1::derive(
            P8QualityPurpose::BaselineEstablishment,
            questions.clone(),
            2,
            3,
        )
        .expect("closure"),
        None,
        None,
        None,
    )
    .expect("baseline plan");
    let baseline_trials = P8QualityTrialSetV1::fixture(&baseline_plan);
    let baseline_resources = P8QualityResourceClosureV1::fixture(&baseline_trials);
    let hard_pass =
        P8HardGateEvaluationV1::evaluate(&policy, &baseline_resources).expect("hard pass");
    let baseline_state = derive_operator_state(P8OperatorExecutionInputV1::Executed {
        plan: &baseline_plan,
        trial_set: &baseline_trials,
        resource_closure: &baseline_resources,
        hard_gate_evaluation: &hard_pass,
        threshold_evaluation: None,
        trusted_execution_receipt: None,
    });
    assert!(matches!(
        baseline_state,
        P8QualityOperatorStateV1::ExecutedValidBaseline {
            release_eligible: false,
            ..
        }
    ));
    let hard_fail = P8HardGateEvaluationV1::evaluate_fixture_with_failures(
        &policy,
        &baseline_resources,
        vec![P8QualityHardGateId::UpdateLineageViolation],
    )
    .expect("fixture hard fail");
    assert!(matches!(
        derive_operator_state(P8OperatorExecutionInputV1::Executed {
            plan: &baseline_plan,
            trial_set: &baseline_trials,
            resource_closure: &baseline_resources,
            hard_gate_evaluation: &hard_fail,
            threshold_evaluation: None,
            trusted_execution_receipt: None,
        }),
        P8QualityOperatorStateV1::ExecutedBaselineRejected {
            hard_gate_failures,
            release_eligible: false,
        } if hard_gate_failures == vec![P8QualityHardGateId::UpdateLineageViolation]
    ));
    let candidate_threshold = threshold(&fixture_protocol);
    let candidate_policy = P8FrozenQualityPolicyV1::build(
        fixture_protocol.protocol_digest().clone(),
        candidate_threshold.threshold_digest().clone(),
    );
    let fixture_candidate = P8QualityExperimentPlanV1::build(
        P8QualityPurpose::QualityCandidate,
        P8ExecutionMode::FixtureContract,
        source.candidate_set(),
        policy.clone(),
        fixture_protocol,
        P8QualityTrialClosureV1::derive(
            P8QualityPurpose::QualityCandidate,
            questions.clone(),
            2,
            3,
        )
        .expect("closure"),
        Some(candidate_threshold.clone()),
        Some(candidate_policy),
        None,
    )
    .expect("fixture candidate");
    let candidate_trials = P8QualityTrialSetV1::fixture(&fixture_candidate);
    let candidate_resources = P8QualityResourceClosureV1::fixture(&candidate_trials);
    let candidate_hard_pass =
        P8HardGateEvaluationV1::evaluate(&policy, &candidate_resources).expect("hard pass");
    let threshold_pass = P8CandidateThresholdEvaluationV1::fixture(&candidate_threshold, vec![])
        .expect("threshold pass");
    assert!(matches!(
        derive_operator_state(P8OperatorExecutionInputV1::Executed {
            plan: &fixture_candidate,
            trial_set: &candidate_trials,
            resource_closure: &candidate_resources,
            hard_gate_evaluation: &candidate_hard_pass,
            threshold_evaluation: Some(&threshold_pass),
            trusted_execution_receipt: None,
        }),
        P8QualityOperatorStateV1::ExecutedValidCandidate {
            decision: P8CandidateQualityDecisionV1::QualityFailed,
            ..
        }
    ));
    let wrong_threshold = P8QualityThresholdLockV1::build(
        fixture_candidate.protocol.protocol_digest().clone(),
        fixture_candidate.hard_policy.policy_digest().clone(),
        P8BaselineManifestRef::derive_for_test("different-baseline"),
        P8ThresholdDerivationOutputsV1 {
            target_thresholds_digest: digest('1'),
            untouched_thresholds_digest: digest('2'),
            rendered_chars_frontier_digest: digest('3'),
            latency_frontier_digest: digest('4'),
            peak_domain_memory_frontier_digest: digest('5'),
        },
    );
    let wrong_evaluation =
        P8CandidateThresholdEvaluationV1::fixture(&wrong_threshold, vec![]).expect("evaluation");
    assert!(matches!(
        derive_operator_state(P8OperatorExecutionInputV1::Executed {
            plan: &fixture_candidate,
            trial_set: &candidate_trials,
            resource_closure: &candidate_resources,
            hard_gate_evaluation: &candidate_hard_pass,
            threshold_evaluation: Some(&wrong_evaluation),
            trusted_execution_receipt: None,
        }),
        P8QualityOperatorStateV1::StructurallyInvalid { .. }
    ));

    let mut missing_threshold = serde_json::to_value(&fixture_candidate).expect("plan");
    missing_threshold["threshold"] = serde_json::Value::Null;
    let missing_threshold: P8QualityExperimentPlanV1 =
        serde_json::from_value(missing_threshold).expect("shape");
    assert!(matches!(
        derive_operator_state(P8OperatorExecutionInputV1::Executed {
            plan: &missing_threshold,
            trial_set: &candidate_trials,
            resource_closure: &candidate_resources,
            hard_gate_evaluation: &candidate_hard_pass,
            threshold_evaluation: None,
            trusted_execution_receipt: None,
        }),
        P8QualityOperatorStateV1::StructurallyInvalid { .. }
    ));

    let mut baseline_with_threshold = serde_json::to_value(&baseline_plan).expect("plan");
    baseline_with_threshold["threshold"] =
        serde_json::to_value(&candidate_threshold).expect("threshold");
    let baseline_with_threshold: P8QualityExperimentPlanV1 =
        serde_json::from_value(baseline_with_threshold).expect("shape");
    assert!(matches!(
        derive_operator_state(P8OperatorExecutionInputV1::Executed {
            plan: &baseline_with_threshold,
            trial_set: &baseline_trials,
            resource_closure: &baseline_resources,
            hard_gate_evaluation: &hard_pass,
            threshold_evaluation: Some(&threshold_pass),
            trusted_execution_receipt: None,
        }),
        P8QualityOperatorStateV1::StructurallyInvalid { .. }
    ));

    let trusted_protocol = protocol(
        &source.baseline_set(),
        &questions,
        P8ExecutionMode::TrustedFull,
    );
    let trusted_threshold = threshold(&trusted_protocol);
    let trusted_policy = P8FrozenQualityPolicyV1::build(
        trusted_protocol.protocol_digest().clone(),
        trusted_threshold.threshold_digest().clone(),
    );
    assert!(P8QualityExperimentPlanV1::build(
        P8QualityPurpose::QualityCandidate,
        P8ExecutionMode::TrustedFull,
        source.candidate_set(),
        policy,
        trusted_protocol,
        P8QualityTrialClosureV1::derive(P8QualityPurpose::QualityCandidate, questions, 2, 3)
            .expect("closure"),
        Some(trusted_threshold.clone()),
        Some(trusted_policy),
        Some(P8TrustedExecutionLeaseRef::derive_for_test("lease")),
    )
    .is_err());
}

#[test]
fn p8_quality_module_has_no_v1_upgrade_or_raw_public_reexport() {
    let library = include_str!("../lib.rs");
    let quality = include_str!("mod.rs");
    assert!(!library.contains("pub use p8_semantic"));
    for forbidden in ["from_v1", "upgrade_v1", "dual_read"] {
        assert!(!quality.contains(forbidden), "{forbidden}");
    }
}
