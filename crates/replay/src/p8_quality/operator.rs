//! Independent P8.5-C fixture operator.
//!
//! The operator consumes strict artifact bytes and recomputes main-trial coverage, question-cluster
//! statistics, hypotheses, ablations, fixture hard gates and local resource frontiers. It never
//! accepts a runner aggregate. TrustedFull and release eligibility remain fail closed; peak-domain
//! memory is explicitly N/A while no trusted Linux authority exists.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::retained_artifact_fs::RetainedArtifactDirectory;

use super::artifact_publisher::{
    open_verified_quality_bundle, publish_quality_bundle_no_replace, P8QualityBundleKindV1,
};
use super::execution_plan::{
    admit_quality_execution_plan, admit_zero_origin_tiny_dataset_manifest, P8MechanicalWorkKindV1,
};
use super::runner_execution::{
    P8CompletedAblationTrialReceiptV1, P8CompletedMainTrialReceiptV1,
    P8CompletedNegativeOnlyProofReceiptV1, P8FixtureCohortManifestV1, P8FixtureShardManifestV1,
    P8PairedJudgeOutcomeV1,
};
use super::{
    actual_applicability_matches, deserialize_p8_quality_artifact, expected_accuracy_succeeds,
    reject_p8_quality_raw_sentinels, P8CandidateQualityDecisionV1, P8ExecutionMode,
    P8HardGateRequirement, P8HypothesisAxisV1, P8HypothesisMembershipDispositionV1,
    P8HypothesisRoleV1, P8QualityArmKind, P8QualityContractFailure, P8QualityDigest,
    P8QualityExperimentPlanV1, P8QualityHardGateId, P8QualityId, P8QualityPurpose,
    P8SameClosureSafeCounterfactualKindV1, P8ThresholdLockRef,
};

const FIXTURE_THRESHOLD_SCHEMA: &str = "beetle-memory.p8.fixture-quality-threshold.v1";
const FIXTURE_OPERATOR_REPORT_SCHEMA: &str = "beetle-memory.p8.fixture-quality-operator-report.v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum P8IndependentOperatorFailureV1 {
    Contract(P8QualityContractFailure),
    InvalidJson,
    ArtifactSetMismatch,
    ReceiptInvalid,
    ThresholdInvalid,
    TrustedExecutionNotSupported,
    ArithmeticOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ExactRationalV1 {
    numerator: u64,
    denominator: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8SignedExactRationalV1 {
    numerator: i64,
    denominator: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8FixtureQualityThresholdV1 {
    schema: String,
    experiment_plan_digest: super::P8ExperimentPlanRef,
    threshold_digest: P8ThresholdLockRef,
    minimum_candidate_score: P8ExactRationalV1,
    minimum_paired_delta: P8SignedExactRationalV1,
    rule_digest: P8QualityDigest,
}

impl P8FixtureQualityThresholdV1 {
    #[cfg(test)]
    fn fixture(
        plan: &P8QualityExperimentPlanV1,
        minimum_candidate_score: P8ExactRationalV1,
        minimum_paired_delta: P8SignedExactRationalV1,
    ) -> Self {
        let mut value = Self {
            schema: FIXTURE_THRESHOLD_SCHEMA.into(),
            experiment_plan_digest: plan.plan_digest.clone(),
            threshold_digest: plan
                .threshold
                .as_ref()
                .expect("candidate fixture threshold")
                .threshold_digest()
                .clone(),
            minimum_candidate_score,
            minimum_paired_delta,
            rule_digest: P8QualityDigest::derive("p8_fixture_quality_threshold_v1", &()),
        };
        value.rule_digest = value.derived_digest();
        value
    }

    fn validate_against(
        &self,
        plan: &P8QualityExperimentPlanV1,
    ) -> Result<(), P8IndependentOperatorFailureV1> {
        let expected_threshold = plan
            .threshold
            .as_ref()
            .map(|threshold| threshold.threshold_digest());
        if self.schema != FIXTURE_THRESHOLD_SCHEMA
            || self.experiment_plan_digest != plan.plan_digest
            || expected_threshold != Some(&self.threshold_digest)
            || self.minimum_candidate_score.denominator == 0
            || self.minimum_paired_delta.denominator == 0
            || self.rule_digest != self.derived_digest()
        {
            return Err(P8IndependentOperatorFailureV1::ThresholdInvalid);
        }
        Ok(())
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_fixture_quality_threshold_v1",
            &(
                &self.schema,
                &self.experiment_plan_digest,
                &self.threshold_digest,
                &self.minimum_candidate_score,
                &self.minimum_paired_delta,
            ),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8FixtureArmScoreV1 {
    arm: P8QualityArmKind,
    score: P8ExactRationalV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8FixtureBootstrapIntervalV1 {
    confidence_level_basis_points: u16,
    resamples: u32,
    observed_delta: P8SignedExactRationalV1,
    one_sided_lower_bound: P8SignedExactRationalV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8FixtureHolmDecisionV1 {
    rank: u32,
    family_size: u32,
    raw_p_value: P8ExactRationalV1,
    adjusted_alpha: P8ExactRationalV1,
    passed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8FixtureHypothesisEvaluationV1 {
    hypothesis_id: P8QualityId,
    role: P8HypothesisRoleV1,
    axes: Vec<P8HypothesisAxisV1>,
    included_questions: Vec<P8QualityId>,
    excluded_questions: Vec<P8QualityId>,
    arm_scores: Vec<P8FixtureArmScoreV1>,
    paired_bootstrap: P8FixtureBootstrapIntervalV1,
    holm: P8FixtureHolmDecisionV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8FixtureAblationSummaryV1 {
    counterfactual: P8SameClosureSafeCounterfactualKindV1,
    pair_count: u64,
    baseline_preferred_count: u64,
    off_run_preferred_count: u64,
    equivalent_count: u64,
    off_run_accuracy_success_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8FixtureHardGateObservationV1 {
    gate_id: P8QualityHardGateId,
    requirement: P8HardGateRequirement,
    observed_count: u64,
    required_total_count: u64,
    passed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8FixturePeakDomainMemoryFrontierV1 {
    NotApplicableNoTrustedLinuxAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8FixtureArmResourceFrontierV1 {
    arm: P8QualityArmKind,
    rendered_chars_quantile_basis_points: u16,
    rendered_chars_quantile: Option<u64>,
    latency_quantile_basis_points: u16,
    question_latency_nanoseconds_quantile: u64,
    peak_domain_memory: P8FixturePeakDomainMemoryFrontierV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8FixtureQualityOperatorReportV1 {
    schema: String,
    experiment_plan_digest: super::P8ExperimentPlanRef,
    execution_plan_digest: P8QualityDigest,
    execution_run_id: super::P8QualityRunRef,
    purpose: P8QualityPurpose,
    main_receipt_count: u64,
    ablation_receipt_count: u64,
    negative_receipt_count: u64,
    arm_scores: Vec<P8FixtureArmScoreV1>,
    candidate_vs_frozen_paired_delta: P8SignedExactRationalV1,
    hypothesis_correction_family_digest: P8QualityDigest,
    hypothesis_evaluations: Vec<P8FixtureHypothesisEvaluationV1>,
    ablation_summaries: Vec<P8FixtureAblationSummaryV1>,
    hard_gate_observations: Vec<P8FixtureHardGateObservationV1>,
    hard_gate_failures: Vec<P8QualityHardGateId>,
    resource_frontiers: Vec<P8FixtureArmResourceFrontierV1>,
    candidate_decision: Option<P8CandidateQualityDecisionV1>,
    fixture_contract_only: bool,
    release_eligible: bool,
    report_digest: P8QualityDigest,
}

impl P8FixtureQualityOperatorReportV1 {
    pub(crate) fn run_id(&self) -> &super::P8QualityRunRef {
        &self.execution_run_id
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_fixture_quality_operator_report_v1",
            &(
                (
                    &self.schema,
                    &self.experiment_plan_digest,
                    &self.execution_plan_digest,
                    &self.execution_run_id,
                    self.purpose,
                ),
                (
                    self.main_receipt_count,
                    self.ablation_receipt_count,
                    self.negative_receipt_count,
                    &self.arm_scores,
                    &self.candidate_vs_frozen_paired_delta,
                ),
                (
                    &self.hypothesis_correction_family_digest,
                    &self.hypothesis_evaluations,
                    &self.ablation_summaries,
                    &self.hard_gate_observations,
                    &self.hard_gate_failures,
                ),
                (
                    &self.resource_frontiers,
                    self.candidate_decision,
                    self.fixture_contract_only,
                    self.release_eligible,
                ),
            ),
        )
    }
}

pub(crate) fn recompute_fixture_operator_from_bytes(
    experiment_plan_bytes: &[u8],
    execution_plan_bytes: &[u8],
    dataset_manifest_bytes: &[u8],
    main_receipts_jsonl: &[u8],
    ablation_receipts_jsonl: &[u8],
    negative_receipts_jsonl: &[u8],
    fixture_threshold_bytes: Option<&[u8]>,
) -> Result<P8FixtureQualityOperatorReportV1, Vec<P8IndependentOperatorFailureV1>> {
    let raw_artifact_violation_count = [
        experiment_plan_bytes,
        execution_plan_bytes,
        dataset_manifest_bytes,
    ]
    .into_iter()
    .chain(fixture_threshold_bytes)
    .filter(|bytes| reject_p8_quality_raw_sentinels(bytes).is_err())
    .count()
        + [
            main_receipts_jsonl,
            ablation_receipts_jsonl,
            negative_receipts_jsonl,
        ]
        .into_iter()
        .map(raw_jsonl_violation_count)
        .sum::<usize>();
    if raw_artifact_violation_count != 0 {
        return Err(vec![P8IndependentOperatorFailureV1::InvalidJson]);
    }
    let experiment_plan: P8QualityExperimentPlanV1 = strict_parse(experiment_plan_bytes)?;
    let plan_failures = experiment_plan.validate_contract();
    if !plan_failures.is_empty() {
        return Err(plan_failures
            .into_iter()
            .map(P8IndependentOperatorFailureV1::Contract)
            .collect());
    }
    if experiment_plan.execution_mode != P8ExecutionMode::FixtureContract {
        return Err(vec![
            P8IndependentOperatorFailureV1::TrustedExecutionNotSupported,
        ]);
    }
    let dataset = admit_zero_origin_tiny_dataset_manifest(dataset_manifest_bytes)
        .map_err(|_| vec![P8IndependentOperatorFailureV1::InvalidJson])?;
    let execution_plan =
        admit_quality_execution_plan(execution_plan_bytes, &experiment_plan, &dataset)
            .map_err(|_| vec![P8IndependentOperatorFailureV1::InvalidJson])?;
    let receipts = parse_jsonl::<P8CompletedMainTrialReceiptV1>(main_receipts_jsonl)?;
    let main_work = execution_plan
        .work_items()
        .iter()
        .filter(|item| item.kind() == P8MechanicalWorkKindV1::Main)
        .collect::<Vec<_>>();
    if receipts.len() != main_work.len() {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    let ablation_receipts =
        parse_jsonl::<P8CompletedAblationTrialReceiptV1>(ablation_receipts_jsonl)?;
    let ablation_work = execution_plan
        .work_items()
        .iter()
        .filter(|item| {
            matches!(
                item.kind(),
                P8MechanicalWorkKindV1::SameClosureAblation { .. }
            )
        })
        .collect::<Vec<_>>();
    let negative_receipts =
        parse_jsonl::<P8CompletedNegativeOnlyProofReceiptV1>(negative_receipts_jsonl)?;
    let negative_work = execution_plan
        .work_items()
        .iter()
        .filter(|item| {
            matches!(
                item.kind(),
                P8MechanicalWorkKindV1::SafetyNegativeProof { .. }
            )
        })
        .collect::<Vec<_>>();
    if ablation_receipts.len() != ablation_work.len()
        || negative_receipts.len() != negative_work.len()
    {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }

    let mut judge_successes = Vec::with_capacity(receipts.len());
    for (receipt, work_item) in receipts.iter().zip(&main_work) {
        receipt
            .validate_against_work_item(&execution_plan, work_item)
            .map_err(|_| vec![P8IndependentOperatorFailureV1::ReceiptInvalid])?;
        let expected = experiment_plan
            .expected_outcomes_for(&receipt.trial_key().question_id)
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ReceiptInvalid])?;
        if !actual_applicability_matches(
            expected,
            receipt.accuracy(),
            receipt.capability_outcomes(),
        ) {
            return Err(vec![P8IndependentOperatorFailureV1::ReceiptInvalid]);
        }
        judge_successes.push(expected_accuracy_succeeds(expected, receipt.accuracy()));
    }
    for (receipt, work_item) in ablation_receipts.iter().zip(&ablation_work) {
        let baseline = receipts
            .iter()
            .find(|candidate| {
                candidate.trial_key().question_id == *work_item.question_id()
                    && candidate.trial_key().arm == work_item.arm()
                    && Some(candidate.trial_key().reader_repeat_index)
                        == work_item.reader_repeat_index()
                    && Some(candidate.trial_key().judge_repeat_index)
                        == work_item.judge_repeat_index()
            })
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ReceiptInvalid])?;
        receipt
            .validate_against_work_item(&execution_plan, work_item, baseline)
            .map_err(|_| vec![P8IndependentOperatorFailureV1::ReceiptInvalid])?;
    }
    for (receipt, work_item) in negative_receipts.iter().zip(&negative_work) {
        receipt
            .validate_against_work_item(&execution_plan, work_item)
            .map_err(|_| vec![P8IndependentOperatorFailureV1::ReceiptInvalid])?;
    }

    let judge_repeats = usize::try_from(experiment_plan.trial_closure.judge_repeats())
        .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
    if judge_repeats == 0 || receipts.len() % judge_repeats != 0 {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    let mut scores = P8QualityArmKind::expected_for(experiment_plan.purpose)
        .iter()
        .map(|arm| (*arm, (0_u64, 0_u64)))
        .collect::<BTreeMap<_, _>>();
    let mut reader_majorities = BTreeMap::new();
    for (receipt_chunk, success_chunk) in receipts
        .chunks(judge_repeats)
        .zip(judge_successes.chunks(judge_repeats))
    {
        let first = receipt_chunk[0].trial_key();
        if receipt_chunk.iter().enumerate().any(|(index, receipt)| {
            let key = receipt.trial_key();
            key.question_id != first.question_id
                || key.arm != first.arm
                || key.reader_repeat_index != first.reader_repeat_index
                || key.judge_repeat_index != u32::try_from(index).unwrap_or(u32::MAX)
        }) {
            return Err(vec![P8IndependentOperatorFailureV1::ReceiptInvalid]);
        }
        let successes = success_chunk.iter().filter(|value| **value).count();
        let entry = scores
            .get_mut(&first.arm)
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ReceiptInvalid])?;
        entry.1 = entry
            .1
            .checked_add(1)
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
        if successes > judge_repeats / 2 {
            entry.0 = entry
                .0
                .checked_add(1)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
        }
        if reader_majorities
            .insert(
                (
                    first.question_id.clone(),
                    first.arm,
                    first.reader_repeat_index,
                ),
                successes > judge_repeats / 2,
            )
            .is_some()
        {
            return Err(vec![P8IndependentOperatorFailureV1::ReceiptInvalid]);
        }
    }
    if scores.values().any(|(_, denominator)| *denominator == 0) {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }

    let arm_scores = P8QualityArmKind::expected_for(experiment_plan.purpose)
        .iter()
        .map(|arm| {
            let (numerator, denominator) = scores[arm];
            P8FixtureArmScoreV1 {
                arm: *arm,
                score: P8ExactRationalV1 {
                    numerator,
                    denominator,
                },
            }
        })
        .collect::<Vec<_>>();
    let paired = paired_candidate_delta(experiment_plan.purpose, &scores)?;
    let hypothesis_evaluations = evaluate_hypotheses(&experiment_plan, &reader_majorities)?;
    let ablation_summaries = derive_ablation_summaries(&experiment_plan, &ablation_receipts)?;
    let hard_gate_observations = derive_fixture_hard_gates(
        &experiment_plan,
        &receipts,
        &ablation_receipts,
        &negative_receipts,
        u64::try_from(raw_artifact_violation_count)
            .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
    )?;
    let hard_gate_failures = hard_gate_observations
        .iter()
        .filter_map(|observation| (!observation.passed).then_some(observation.gate_id))
        .collect::<Vec<_>>();
    let resource_frontiers = derive_resource_frontiers(&experiment_plan, &receipts)?;
    let candidate_decision = match experiment_plan.purpose {
        P8QualityPurpose::BaselineEstablishment => {
            if fixture_threshold_bytes.is_some() {
                return Err(vec![P8IndependentOperatorFailureV1::ThresholdInvalid]);
            }
            None
        }
        P8QualityPurpose::QualityCandidate => {
            let threshold: P8FixtureQualityThresholdV1 = strict_parse(
                fixture_threshold_bytes
                    .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ThresholdInvalid])?,
            )?;
            threshold
                .validate_against(&experiment_plan)
                .map_err(|failure| vec![failure])?;
            let candidate = scores
                .get(&P8QualityArmKind::P8Candidate)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ReceiptInvalid])?;
            Some(
                if unsigned_fraction_at_least(
                    *candidate,
                    (
                        threshold.minimum_candidate_score.numerator,
                        threshold.minimum_candidate_score.denominator,
                    ),
                ) && signed_fraction_at_least(
                    (paired.numerator, paired.denominator),
                    (
                        threshold.minimum_paired_delta.numerator,
                        threshold.minimum_paired_delta.denominator,
                    ),
                ) {
                    P8CandidateQualityDecisionV1::QualityPassed
                } else {
                    P8CandidateQualityDecisionV1::QualityFailed
                },
            )
        }
    };
    let mut report = P8FixtureQualityOperatorReportV1 {
        schema: FIXTURE_OPERATOR_REPORT_SCHEMA.into(),
        experiment_plan_digest: experiment_plan.plan_digest.clone(),
        execution_plan_digest: execution_plan.execution_plan_digest().clone(),
        execution_run_id: execution_plan.run_id().clone(),
        purpose: experiment_plan.purpose,
        main_receipt_count: u64::try_from(receipts.len())
            .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
        ablation_receipt_count: u64::try_from(ablation_receipts.len())
            .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
        negative_receipt_count: u64::try_from(negative_receipts.len())
            .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
        arm_scores,
        candidate_vs_frozen_paired_delta: paired,
        hypothesis_correction_family_digest: experiment_plan
            .protocol
            .frozen
            .hypothesis_registry
            .correction_family_digest
            .clone(),
        hypothesis_evaluations,
        ablation_summaries,
        hard_gate_observations,
        hard_gate_failures,
        resource_frontiers,
        candidate_decision,
        fixture_contract_only: true,
        release_eligible: false,
        report_digest: P8QualityDigest::derive("p8_fixture_quality_operator_report_v1", &()),
    };
    report.report_digest = report.derived_digest();
    Ok(report)
}

fn raw_jsonl_violation_count(bytes: &[u8]) -> usize {
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return 1;
    }
    bytes[..bytes.len() - 1]
        .split(|byte| *byte == b'\n')
        .map(|line| usize::from(line.is_empty() || reject_p8_quality_raw_sentinels(line).is_err()))
        .sum()
}

pub(crate) fn recompute_fixture_operator_from_retained_runner_bundle(
    root: &RetainedArtifactDirectory,
    final_name: &str,
) -> Result<P8FixtureQualityOperatorReportV1, Vec<P8IndependentOperatorFailureV1>> {
    let (bundle_manifest, files) = open_verified_quality_bundle(root, final_name)
        .map_err(|_| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
    if bundle_manifest.kind() != P8QualityBundleKindV1::RunnerCohort {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    let experiment_bytes = files
        .get("experiment-plan.json")
        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
    let execution_bytes = files
        .get("execution-plan.json")
        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
    let dataset_bytes = files
        .get("dataset-manifest.json")
        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
    let experiment: P8QualityExperimentPlanV1 = strict_parse(experiment_bytes)?;
    let dataset = admit_zero_origin_tiny_dataset_manifest(dataset_bytes)
        .map_err(|_| vec![P8IndependentOperatorFailureV1::InvalidJson])?;
    let execution = admit_quality_execution_plan(execution_bytes, &experiment, &dataset)
        .map_err(|_| vec![P8IndependentOperatorFailureV1::InvalidJson])?;
    if bundle_manifest.run_id() != execution.run_id()
        || files
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            != execution
                .exact_artifact_names()
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
    {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    verify_fixture_runner_manifests(&files, &execution)?;

    let mut main_receipts = Vec::new();
    let mut ablation_receipts = Vec::new();
    let mut negative_receipts = Vec::new();
    for name in execution.exact_artifact_names().iter() {
        let target = if name.ends_with(".main.jsonl") {
            Some(&mut main_receipts)
        } else if name.ends_with(".ablation.jsonl") {
            Some(&mut ablation_receipts)
        } else if name.ends_with(".negative.jsonl") {
            Some(&mut negative_receipts)
        } else {
            None
        };
        if let Some(target) = target {
            target.extend_from_slice(
                files
                    .get(name)
                    .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?,
            );
        }
    }
    let threshold = files.get("fixture-threshold.json").map(Vec::as_slice);
    recompute_fixture_operator_from_bytes(
        experiment_bytes,
        execution_bytes,
        dataset_bytes,
        &main_receipts,
        &ablation_receipts,
        &negative_receipts,
        threshold,
    )
}

fn verify_fixture_runner_manifests(
    files: &BTreeMap<String, Vec<u8>>,
    execution: &super::execution_plan::P8QualityExecutionPlanV1,
) -> Result<(), Vec<P8IndependentOperatorFailureV1>> {
    let cohort: P8FixtureCohortManifestV1 = strict_parse(
        files
            .get("cohort-manifest.json")
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?,
    )?;
    if !cohort.validate_contract() || &cohort.run_id != execution.run_id() {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    let expected_shards = execution
        .work_items()
        .iter()
        .map(super::execution_plan::P8MechanicalWorkItemV1::shard_index)
        .collect::<BTreeSet<_>>();
    if cohort.shards.len() != expected_shards.len() {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    let mut totals = BTreeMap::from([("main", 0_u64), ("ablation", 0_u64), ("negative", 0_u64)]);
    for (shard_ref, expected_index) in cohort.shards.iter().zip(expected_shards) {
        if shard_ref.shard_index != expected_index
            || shard_ref.manifest_file_name != format!("shard-{expected_index:05}.manifest.json")
        {
            return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
        }
        let manifest: P8FixtureShardManifestV1 = strict_parse(
            files
                .get(&shard_ref.manifest_file_name)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?,
        )?;
        if !manifest.validate_contract()
            || manifest.run_id != cohort.run_id
            || manifest.shard_index != expected_index
            || manifest.manifest_digest != shard_ref.manifest_digest
        {
            return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
        }
        for evidence in &manifest.receipt_files {
            let bytes = files
                .get(&evidence.file_name)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
            let expected_prefix = format!("shard-{expected_index:05}.");
            let suffix = evidence
                .file_name
                .strip_prefix(&expected_prefix)
                .and_then(|name| name.strip_suffix(".jsonl"))
                .filter(|name| matches!(*name, "main" | "ablation" | "negative"))
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
            let record_count = u64::try_from(
                bytes
                    .split(|byte| *byte == b'\n')
                    .filter(|line| !line.is_empty())
                    .count(),
            )
            .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
            if evidence.content_digest
                != P8QualityDigest::derive("p8_fixture_receipt_file_bytes_v1", &bytes)
                || evidence.record_count != record_count
            {
                return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
            }
            let total = totals
                .get_mut(suffix)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
            *total = total
                .checked_add(record_count)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
        }
    }
    if totals["main"] != cohort.main_receipt_count
        || totals["ablation"] != cohort.ablation_receipt_count
        || totals["negative"] != cohort.negative_receipt_count
    {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    Ok(())
}

pub(crate) fn publish_fixture_operator_report_from_retained_runner_bundle(
    root: &RetainedArtifactDirectory,
    runner_final_name: &str,
    operator_stage_name: &str,
    operator_final_name: &str,
) -> std::io::Result<P8FixtureQualityOperatorReportV1> {
    let report = recompute_fixture_operator_from_retained_runner_bundle(root, runner_final_name)
        .map_err(|failures| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("P8 fixture operator rejected retained runner bytes: {failures:?}"),
            )
        })?;
    let report_bytes = serde_json::to_vec(&report)
        .map_err(|_| std::io::Error::other("P8 fixture operator report serialization failed"))?;
    publish_quality_bundle_no_replace(
        root,
        operator_stage_name,
        operator_final_name,
        report.run_id().clone(),
        P8QualityBundleKindV1::OperatorReport,
        BTreeMap::from([("operator-report.json".into(), report_bytes)]),
    )?;
    Ok(report)
}

fn strict_parse<T>(bytes: &[u8]) -> Result<T, Vec<P8IndependentOperatorFailureV1>>
where
    T: serde::de::DeserializeOwned,
{
    reject_p8_quality_raw_sentinels(bytes)
        .map_err(|_| vec![P8IndependentOperatorFailureV1::InvalidJson])?;
    deserialize_p8_quality_artifact(bytes)
        .map_err(|_| vec![P8IndependentOperatorFailureV1::InvalidJson])
}

fn parse_jsonl<T>(bytes: &[u8]) -> Result<Vec<T>, Vec<P8IndependentOperatorFailureV1>>
where
    T: serde::de::DeserializeOwned,
{
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return Err(vec![P8IndependentOperatorFailureV1::InvalidJson]);
    }
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(strict_parse)
        .collect()
}

fn paired_candidate_delta(
    purpose: P8QualityPurpose,
    scores: &BTreeMap<P8QualityArmKind, (u64, u64)>,
) -> Result<P8SignedExactRationalV1, Vec<P8IndependentOperatorFailureV1>> {
    if purpose == P8QualityPurpose::BaselineEstablishment {
        return Ok(P8SignedExactRationalV1 {
            numerator: 0,
            denominator: scores[&P8QualityArmKind::FrozenP84Baseline].1,
        });
    }
    let candidate = scores[&P8QualityArmKind::P8Candidate];
    let frozen = scores[&P8QualityArmKind::FrozenP84Baseline];
    if candidate.1 != frozen.1 {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    let candidate_numerator = i64::try_from(candidate.0)
        .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
    let frozen_numerator = i64::try_from(frozen.0)
        .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
    Ok(P8SignedExactRationalV1 {
        numerator: candidate_numerator
            .checked_sub(frozen_numerator)
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
        denominator: candidate.1,
    })
}

fn evaluate_hypotheses(
    plan: &P8QualityExperimentPlanV1,
    reader_majorities: &BTreeMap<(P8QualityId, P8QualityArmKind, u32), bool>,
) -> Result<Vec<P8FixtureHypothesisEvaluationV1>, Vec<P8IndependentOperatorFailureV1>> {
    let registry = &plan.protocol.frozen.hypothesis_registry;
    let statistics = &plan.protocol.frozen.statistics;
    let reader_repeats = plan.trial_closure.reader_repeats();
    let family_size = u32::try_from(registry.hypotheses.len())
        .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
    if family_size == 0 {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }

    let mut evaluations = Vec::with_capacity(registry.hypotheses.len());
    for hypothesis in &registry.hypotheses {
        let included_questions = hypothesis
            .memberships
            .iter()
            .filter_map(|membership| {
                matches!(
                    membership.disposition,
                    P8HypothesisMembershipDispositionV1::Included
                )
                .then_some(membership.question_id.clone())
            })
            .collect::<Vec<_>>();
        let excluded_questions = hypothesis
            .memberships
            .iter()
            .filter_map(|membership| {
                (!matches!(
                    membership.disposition,
                    P8HypothesisMembershipDispositionV1::Included
                ))
                .then_some(membership.question_id.clone())
            })
            .collect::<Vec<_>>();
        if included_questions.is_empty()
            || included_questions.iter().collect::<BTreeSet<_>>().len() != included_questions.len()
        {
            return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
        }

        let mut arm_scores = Vec::new();
        for arm in P8QualityArmKind::expected_for(plan.purpose) {
            let mut numerator = 0_u64;
            let mut denominator = 0_u64;
            for question_id in &included_questions {
                for reader_repeat_index in 0..reader_repeats {
                    let success = reader_majorities
                        .get(&(question_id.clone(), *arm, reader_repeat_index))
                        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
                    denominator = denominator
                        .checked_add(1)
                        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
                    if *success {
                        numerator = numerator.checked_add(1).ok_or_else(|| {
                            vec![P8IndependentOperatorFailureV1::ArithmeticOverflow]
                        })?;
                    }
                }
            }
            arm_scores.push(P8FixtureArmScoreV1 {
                arm: *arm,
                score: P8ExactRationalV1 {
                    numerator,
                    denominator,
                },
            });
        }

        let (question_differences, denominator) = if plan.purpose
            == P8QualityPurpose::QualityCandidate
        {
            let mut differences = Vec::with_capacity(included_questions.len());
            for question_id in &included_questions {
                let mut difference = 0_i64;
                for reader_repeat_index in 0..reader_repeats {
                    let candidate = *reader_majorities
                        .get(&(
                            question_id.clone(),
                            P8QualityArmKind::P8Candidate,
                            reader_repeat_index,
                        ))
                        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
                    let frozen = *reader_majorities
                        .get(&(
                            question_id.clone(),
                            P8QualityArmKind::FrozenP84Baseline,
                            reader_repeat_index,
                        ))
                        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
                    let candidate_value = if candidate { 1_i64 } else { 0_i64 };
                    let frozen_value = if frozen { 1_i64 } else { 0_i64 };
                    difference = difference
                        .checked_add(candidate_value - frozen_value)
                        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
                }
                differences.push(difference);
            }
            let denominator = u64::try_from(included_questions.len())
                .ok()
                .and_then(|questions| questions.checked_mul(u64::from(reader_repeats)))
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
            (differences, denominator)
        } else {
            let denominator = u64::try_from(included_questions.len())
                .ok()
                .and_then(|questions| questions.checked_mul(u64::from(reader_repeats)))
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
            (vec![0; included_questions.len()], denominator)
        };
        if denominator == 0
            || u32::try_from(included_questions.len()).ok()
                < Some(statistics.minimum_effective_questions)
        {
            return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
        }

        let observed_numerator = question_differences
            .iter()
            .try_fold(0_i64, |total, value| {
                total
                    .checked_add(*value)
                    .ok_or(P8IndependentOperatorFailureV1::ArithmeticOverflow)
            })
            .map_err(|failure| vec![failure])?;
        let mut rng =
            P8FixtureBootstrapRng::from_plan_and_hypothesis(plan, &hypothesis.hypothesis_id);
        let mut bootstrap_numerators = Vec::with_capacity(
            usize::try_from(statistics.bootstrap_resamples)
                .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
        );
        for _ in 0..statistics.bootstrap_resamples {
            let mut numerator = 0_i64;
            for _ in 0..question_differences.len() {
                let index = rng.next_index(question_differences.len());
                numerator = numerator
                    .checked_add(question_differences[index])
                    .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
            }
            bootstrap_numerators.push(numerator);
        }
        bootstrap_numerators.sort_unstable();
        let lower_rank = (u64::from(10_000_u16 - statistics.confidence_level_basis_points)
            .checked_mul(u64::from(statistics.bootstrap_resamples))
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?
            .checked_add(9_999)
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?
            / 10_000)
            .saturating_sub(1);
        let lower_index = usize::try_from(lower_rank)
            .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?
            .min(bootstrap_numerators.len().saturating_sub(1));
        let non_positive = bootstrap_numerators
            .iter()
            .filter(|numerator| **numerator <= 0)
            .count();
        let raw_p_value = P8ExactRationalV1 {
            numerator: u64::try_from(non_positive)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
            denominator: u64::from(statistics.bootstrap_resamples)
                .checked_add(1)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
        };
        evaluations.push(P8FixtureHypothesisEvaluationV1 {
            hypothesis_id: hypothesis.hypothesis_id.clone(),
            role: hypothesis.role,
            axes: hypothesis.axes.clone(),
            included_questions,
            excluded_questions,
            arm_scores,
            paired_bootstrap: P8FixtureBootstrapIntervalV1 {
                confidence_level_basis_points: statistics.confidence_level_basis_points,
                resamples: statistics.bootstrap_resamples,
                observed_delta: P8SignedExactRationalV1 {
                    numerator: observed_numerator,
                    denominator,
                },
                one_sided_lower_bound: P8SignedExactRationalV1 {
                    numerator: bootstrap_numerators[lower_index],
                    denominator,
                },
            },
            holm: P8FixtureHolmDecisionV1 {
                rank: 0,
                family_size,
                raw_p_value,
                adjusted_alpha: P8ExactRationalV1 {
                    numerator: 0,
                    denominator: 1,
                },
                passed: false,
            },
        });
    }

    apply_holm_family(
        &mut evaluations,
        plan.protocol
            .frozen
            .statistics
            .family_alpha_parts_per_million,
    )?;
    Ok(evaluations)
}

fn apply_holm_family(
    evaluations: &mut [P8FixtureHypothesisEvaluationV1],
    family_alpha_parts_per_million: u32,
) -> Result<(), Vec<P8IndependentOperatorFailureV1>> {
    let family_size = u64::try_from(evaluations.len())
        .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
    let mut order = (0..evaluations.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| {
        compare_unsigned_fraction(
            &evaluations[*left].holm.raw_p_value,
            &evaluations[*right].holm.raw_p_value,
        )
        .then_with(|| {
            evaluations[*left]
                .hypothesis_id
                .cmp(&evaluations[*right].hypothesis_id)
        })
    });
    let mut family_still_passing = true;
    for (zero_rank, evaluation_index) in order.into_iter().enumerate() {
        let rank = u64::try_from(zero_rank)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
        let remaining = family_size
            .checked_sub(rank)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
        let adjusted_alpha = P8ExactRationalV1 {
            numerator: u64::from(family_alpha_parts_per_million),
            denominator: 1_000_000_u64
                .checked_mul(remaining)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
        };
        let local_pass = compare_unsigned_fraction(
            &evaluations[evaluation_index].holm.raw_p_value,
            &adjusted_alpha,
        ) != Ordering::Greater;
        family_still_passing &= local_pass;
        evaluations[evaluation_index].holm = P8FixtureHolmDecisionV1 {
            rank: u32::try_from(rank)
                .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
            family_size: u32::try_from(family_size)
                .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
            raw_p_value: evaluations[evaluation_index].holm.raw_p_value.clone(),
            adjusted_alpha,
            passed: family_still_passing,
        };
    }
    Ok(())
}

fn derive_ablation_summaries(
    plan: &P8QualityExperimentPlanV1,
    receipts: &[P8CompletedAblationTrialReceiptV1],
) -> Result<Vec<P8FixtureAblationSummaryV1>, Vec<P8IndependentOperatorFailureV1>> {
    let mut summaries = P8SameClosureSafeCounterfactualKindV1::ALL
        .into_iter()
        .map(|counterfactual| {
            (
                counterfactual,
                P8FixtureAblationSummaryV1 {
                    counterfactual,
                    pair_count: 0,
                    baseline_preferred_count: 0,
                    off_run_preferred_count: 0,
                    equivalent_count: 0,
                    off_run_accuracy_success_count: 0,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for receipt in receipts {
        let expected = plan
            .expected_outcomes_for(&receipt.key().question_id)
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ReceiptInvalid])?;
        let summary = summaries
            .get_mut(&receipt.key().counterfactual)
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ReceiptInvalid])?;
        summary.pair_count = summary
            .pair_count
            .checked_add(1)
            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
        match receipt.paired_outcome() {
            P8PairedJudgeOutcomeV1::BaselinePreferred => {
                summary.baseline_preferred_count = summary
                    .baseline_preferred_count
                    .checked_add(1)
                    .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
            }
            P8PairedJudgeOutcomeV1::OffRunPreferred => {
                summary.off_run_preferred_count = summary
                    .off_run_preferred_count
                    .checked_add(1)
                    .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
            }
            P8PairedJudgeOutcomeV1::Equivalent => {
                summary.equivalent_count = summary
                    .equivalent_count
                    .checked_add(1)
                    .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
            }
        }
        if expected_accuracy_succeeds(expected, receipt.off_run_accuracy()) {
            summary.off_run_accuracy_success_count = summary
                .off_run_accuracy_success_count
                .checked_add(1)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
        }
    }
    if summaries.values().any(|summary| summary.pair_count == 0) {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    Ok(summaries.into_values().collect())
}

fn derive_fixture_hard_gates(
    plan: &P8QualityExperimentPlanV1,
    main_receipts: &[P8CompletedMainTrialReceiptV1],
    ablation_receipts: &[P8CompletedAblationTrialReceiptV1],
    negative_receipts: &[P8CompletedNegativeOnlyProofReceiptV1],
    raw_artifact_violation_count: u64,
) -> Result<Vec<P8FixtureHardGateObservationV1>, Vec<P8IndependentOperatorFailureV1>> {
    let all_receipt_count = u64::try_from(main_receipts.len())
        .ok()
        .and_then(|value| value.checked_add(u64::try_from(ablation_receipts.len()).ok()?))
        .and_then(|value| value.checked_add(u64::try_from(negative_receipts.len()).ok()?))
        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
    let semantic_evidence = ablation_receipts
        .iter()
        .flat_map(P8CompletedAblationTrialReceiptV1::hard_gate_evidences)
        .collect::<Vec<_>>();
    let semantic_receipt_count = u64::try_from(semantic_evidence.len())
        .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
    if all_receipt_count == 0 || semantic_receipt_count == 0 {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    P8QualityHardGateId::ALL
        .into_iter()
        .map(|gate_id| {
            let requirement = plan
                .hard_policy
                .requirement_for(gate_id)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ReceiptInvalid])?;
            let required_total_count = match gate_id {
                P8QualityHardGateId::RawProcedureCredentialPathOrPrematureGoldPersistence
                | P8QualityHardGateId::UnexpectedRuntimeOrIntegrityFailure
                | P8QualityHardGateId::RequiredReportOperatorCoverage => all_receipt_count,
                P8QualityHardGateId::IneligibleOwnerProjection
                | P8QualityHardGateId::NonCurrentMaterialProjection
                | P8QualityHardGateId::CrossSubjectPrivateSoulLeak
                | P8QualityHardGateId::FullStoreScanSecondPlatformOrLiveFallback
                | P8QualityHardGateId::PostImageClosureCoverage
                | P8QualityHardGateId::UpdateLineageViolation
                | P8QualityHardGateId::UnmetPremiseProcedureDelivery
                | P8QualityHardGateId::ProfileBudgetRenderCeilingBreach => semantic_receipt_count,
            };
            let sum =
                |value: fn(&super::runner_execution::P8ReplaySemanticHardGateEvidenceV1) -> u64| {
                    semantic_evidence.iter().try_fold(0_u64, |total, evidence| {
                        total
                            .checked_add(value(evidence))
                            .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])
                    })
                };
            let observed_count = match gate_id {
                P8QualityHardGateId::IneligibleOwnerProjection => {
                    sum(|evidence| evidence.ineligible_owner_projection_count)?
                }
                P8QualityHardGateId::NonCurrentMaterialProjection => {
                    sum(|evidence| evidence.non_current_material_projection_count)?
                }
                P8QualityHardGateId::CrossSubjectPrivateSoulLeak => {
                    sum(|evidence| evidence.cross_subject_private_soul_leak_count)?
                }
                P8QualityHardGateId::RawProcedureCredentialPathOrPrematureGoldPersistence => {
                    raw_artifact_violation_count
                }
                P8QualityHardGateId::UnexpectedRuntimeOrIntegrityFailure => {
                    sum(|evidence| evidence.unexpected_runtime_or_integrity_failure_count)?
                }
                P8QualityHardGateId::FullStoreScanSecondPlatformOrLiveFallback => {
                    sum(|evidence| evidence.full_store_scan_second_platform_or_live_fallback_count)?
                }
                P8QualityHardGateId::PostImageClosureCoverage => u64::try_from(
                    semantic_evidence
                        .iter()
                        .filter(|evidence| evidence.post_image_closure_covered)
                        .count(),
                )
                .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
                P8QualityHardGateId::UpdateLineageViolation => {
                    sum(|evidence| evidence.update_lineage_violation_count)?
                }
                P8QualityHardGateId::UnmetPremiseProcedureDelivery => {
                    sum(|evidence| evidence.unmet_premise_procedure_delivery_count)?
                }
                P8QualityHardGateId::RequiredReportOperatorCoverage => all_receipt_count,
                P8QualityHardGateId::ProfileBudgetRenderCeilingBreach => {
                    sum(|evidence| evidence.profile_budget_render_ceiling_breach_count)?
                }
            };
            Ok(P8FixtureHardGateObservationV1 {
                gate_id,
                requirement,
                observed_count,
                required_total_count,
                passed: match requirement {
                    P8HardGateRequirement::ExactZero => observed_count == 0,
                    P8HardGateRequirement::ExactFull => observed_count == required_total_count,
                },
            })
        })
        .collect()
}

fn derive_resource_frontiers(
    plan: &P8QualityExperimentPlanV1,
    receipts: &[P8CompletedMainTrialReceiptV1],
) -> Result<Vec<P8FixtureArmResourceFrontierV1>, Vec<P8IndependentOperatorFailureV1>> {
    let resources = &plan.protocol.frozen.resources;
    let mut unique_readers = BTreeSet::new();
    let mut rendered = BTreeMap::<P8QualityArmKind, Vec<u64>>::new();
    let mut latency = BTreeMap::<P8QualityArmKind, Vec<u64>>::new();
    for receipt in receipts {
        let key = receipt.trial_key();
        if !unique_readers.insert((key.question_id.clone(), key.arm, key.reader_repeat_index)) {
            continue;
        }
        latency.entry(key.arm).or_default().push(
            receipt
                .reader_elapsed_nanoseconds()
                .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
        );
        if matches!(
            key.arm,
            P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate
        ) {
            rendered
                .entry(key.arm)
                .or_default()
                .push(receipt.memory_projection_rendered_chars());
        }
    }
    P8QualityArmKind::expected_for(plan.purpose)
        .iter()
        .map(|arm| {
            let latency_values = latency
                .get(arm)
                .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])?;
            let rendered_chars_quantile = rendered
                .get(arm)
                .map(|values| {
                    nearest_rank_quantile(values, resources.rendered_chars_quantile_basis_points)
                })
                .transpose()?;
            Ok(P8FixtureArmResourceFrontierV1 {
                arm: *arm,
                rendered_chars_quantile_basis_points: resources
                    .rendered_chars_quantile_basis_points,
                rendered_chars_quantile,
                latency_quantile_basis_points: resources.latency_quantile_basis_points,
                question_latency_nanoseconds_quantile: nearest_rank_quantile(
                    latency_values,
                    resources.latency_quantile_basis_points,
                )?,
                peak_domain_memory:
                    P8FixturePeakDomainMemoryFrontierV1::NotApplicableNoTrustedLinuxAuthority,
            })
        })
        .collect()
}

fn nearest_rank_quantile(
    values: &[u64],
    quantile_basis_points: u16,
) -> Result<u64, Vec<P8IndependentOperatorFailureV1>> {
    if values.is_empty() || quantile_basis_points == 0 || quantile_basis_points > 10_000 {
        return Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch]);
    }
    let mut ordered = values.to_vec();
    ordered.sort_unstable();
    let numerator = u64::from(quantile_basis_points)
        .checked_mul(
            u64::try_from(ordered.len())
                .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?,
        )
        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
    let rank = numerator
        .checked_add(9_999)
        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?
        / 10_000;
    let index = usize::try_from(rank.saturating_sub(1))
        .map_err(|_| vec![P8IndependentOperatorFailureV1::ArithmeticOverflow])?;
    ordered
        .get(index)
        .copied()
        .ok_or_else(|| vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])
}

fn compare_unsigned_fraction(left: &P8ExactRationalV1, right: &P8ExactRationalV1) -> Ordering {
    (u128::from(left.numerator) * u128::from(right.denominator))
        .cmp(&(u128::from(right.numerator) * u128::from(left.denominator)))
}

struct P8FixtureBootstrapRng {
    state: u64,
}

impl P8FixtureBootstrapRng {
    fn from_plan_and_hypothesis(
        plan: &P8QualityExperimentPlanV1,
        hypothesis_id: &P8QualityId,
    ) -> Self {
        let digest = P8QualityDigest::derive(
            "p8_fixture_question_cluster_bootstrap_seed_v1",
            &(&plan.plan_digest, hypothesis_id),
        );
        let state = u64::from_str_radix(&digest.as_str()[7..23], 16)
            .unwrap_or(0x9e37_79b9_7f4a_7c15)
            .max(1);
        Self { state }
    }

    fn next_index(&mut self, upper: usize) -> usize {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        usize::try_from(self.state % u64::try_from(upper).expect("non-empty cluster fits u64"))
            .expect("bootstrap index fits usize")
    }
}

fn unsigned_fraction_at_least(actual: (u64, u64), minimum: (u64, u64)) -> bool {
    u128::from(actual.0) * u128::from(minimum.1) >= u128::from(minimum.0) * u128::from(actual.1)
}

fn signed_fraction_at_least(actual: (i64, u64), minimum: (i64, u64)) -> bool {
    i128::from(actual.0) * i128::from(minimum.1) >= i128::from(minimum.0) * i128::from(actual.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p8_quality::execution_plan::{
        P8QualityExecutionPlanV1, P8SupervisorOwnedRunGeneration,
        P8ZeroOriginTinyDatasetManifestV1, P8ZeroOriginTinyQuestionManifestV1,
    };
    use crate::p8_quality::runner_execution::{
        build_real_fixture_runner_bundle_files, execute_real_fixture_receipt_set,
        fixture_completed_ablation_receipt, fixture_completed_main_receipt,
        fixture_completed_negative_receipt,
    };
    use crate::p8_quality::P8QualityId;

    fn digest(label: &str) -> P8QualityDigest {
        P8QualityDigest::derive("p8_operator_test_digest_v1", &label)
    }

    fn fixture_dataset() -> P8ZeroOriginTinyDatasetManifestV1 {
        let questions = ["q-1", "q-2"]
            .into_iter()
            .map(|value| P8QualityId::parse(value).expect("question id"))
            .map(|question_id| {
                P8ZeroOriginTinyQuestionManifestV1::new(
                    question_id.clone(),
                    P8QualityDigest::derive("question", &question_id),
                    P8QualityDigest::derive("rubric", &question_id),
                    P8QualityDigest::derive("gold", &question_id),
                )
            })
            .collect();
        P8ZeroOriginTinyDatasetManifestV1::build(
            digest("dataset"),
            digest("version"),
            digest("license"),
            questions,
        )
        .expect("dataset")
    }

    fn fixture_inputs(
        purpose: P8QualityPurpose,
    ) -> (
        P8QualityExperimentPlanV1,
        P8QualityExecutionPlanV1,
        P8ZeroOriginTinyDatasetManifestV1,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
    ) {
        let dataset = fixture_dataset();
        let experiment_plan =
            crate::p8_quality::tests::fixture_plan_for_zero_origin_dataset(purpose, &dataset);
        let generation = P8SupervisorOwnedRunGeneration::mint_for_supervisor(
            digest("supervisor"),
            if purpose == P8QualityPurpose::BaselineEstablishment {
                1
            } else {
                2
            },
            if purpose == P8QualityPurpose::BaselineEstablishment {
                [1; 32]
            } else {
                [2; 32]
            },
        )
        .expect("generation");
        let execution_plan =
            P8QualityExecutionPlanV1::derive(&experiment_plan, &dataset, generation)
                .expect("execution plan");
        let main_work = execution_plan
            .work_items()
            .iter()
            .filter(|item| item.kind() == P8MechanicalWorkKindV1::Main)
            .collect::<Vec<_>>();
        let main_receipts = main_work
            .iter()
            .map(|work_item| {
                let succeeds = match work_item.arm() {
                    P8QualityArmKind::NoMemory => false,
                    P8QualityArmKind::PublicReference => work_item.question_id().as_str() == "q-1",
                    P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate => true,
                };
                fixture_completed_main_receipt(&execution_plan, work_item, succeeds)
            })
            .collect::<Vec<_>>();
        let mut main_jsonl = Vec::new();
        for receipt in &main_receipts {
            serde_json::to_writer(&mut main_jsonl, receipt).expect("receipt json");
            main_jsonl.push(b'\n');
        }
        let mut ablation_jsonl = Vec::new();
        for work_item in execution_plan.work_items().iter().filter(|item| {
            matches!(
                item.kind(),
                P8MechanicalWorkKindV1::SameClosureAblation { .. }
            )
        }) {
            let baseline = main_receipts
                .iter()
                .find(|receipt| {
                    receipt.trial_key().question_id == *work_item.question_id()
                        && receipt.trial_key().arm == work_item.arm()
                        && Some(receipt.trial_key().reader_repeat_index)
                            == work_item.reader_repeat_index()
                        && Some(receipt.trial_key().judge_repeat_index)
                            == work_item.judge_repeat_index()
                })
                .expect("baseline receipt for ablation");
            serde_json::to_writer(
                &mut ablation_jsonl,
                &fixture_completed_ablation_receipt(&execution_plan, work_item, baseline),
            )
            .expect("ablation receipt json");
            ablation_jsonl.push(b'\n');
        }
        let mut negative_jsonl = Vec::new();
        for work_item in execution_plan.work_items().iter().filter(|item| {
            matches!(
                item.kind(),
                P8MechanicalWorkKindV1::SafetyNegativeProof { .. }
            )
        }) {
            serde_json::to_writer(
                &mut negative_jsonl,
                &fixture_completed_negative_receipt(&execution_plan, work_item),
            )
            .expect("negative receipt json");
            negative_jsonl.push(b'\n');
        }
        (
            experiment_plan,
            execution_plan,
            dataset,
            main_jsonl,
            ablation_jsonl,
            negative_jsonl,
        )
    }

    #[test]
    fn independent_operator_recomputes_baseline_and_rejects_missing_raw_receipt() {
        let (experiment_plan, execution_plan, dataset, jsonl, ablation_jsonl, negative_jsonl) =
            fixture_inputs(P8QualityPurpose::BaselineEstablishment);
        let report = recompute_fixture_operator_from_bytes(
            &serde_json::to_vec(&experiment_plan).expect("plan bytes"),
            &serde_json::to_vec(&execution_plan).expect("execution bytes"),
            &serde_json::to_vec(&dataset).expect("dataset bytes"),
            &jsonl,
            &ablation_jsonl,
            &negative_jsonl,
            None,
        )
        .expect("operator report");
        assert_eq!(report.purpose, P8QualityPurpose::BaselineEstablishment);
        assert!(report.candidate_decision.is_none());
        assert!(!report.release_eligible);
        assert_eq!(report.execution_run_id, execution_plan.run_id().clone());

        let final_line = jsonl
            .iter()
            .rposition(|byte| *byte == b'\n')
            .expect("newline");
        let previous_line = jsonl[..final_line]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .expect("previous newline");
        assert_eq!(
            recompute_fixture_operator_from_bytes(
                &serde_json::to_vec(&experiment_plan).expect("plan bytes"),
                &serde_json::to_vec(&execution_plan).expect("execution bytes"),
                &serde_json::to_vec(&dataset).expect("dataset bytes"),
                &jsonl[..=previous_line],
                &ablation_jsonl,
                &negative_jsonl,
                None,
            ),
            Err(vec![P8IndependentOperatorFailureV1::ArtifactSetMismatch])
        );

        let first_ablation_end = ablation_jsonl
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("first ablation record");
        let mut first_ablation: serde_json::Value =
            serde_json::from_slice(&ablation_jsonl[..first_ablation_end]).expect("ablation value");
        first_ablation["key"]["question_id"] = serde_json::json!("q-cross-binding");
        let mut cross_bound = serde_json::to_vec(&first_ablation).expect("tampered ablation");
        cross_bound.push(b'\n');
        cross_bound.extend_from_slice(&ablation_jsonl[first_ablation_end + 1..]);
        assert_eq!(
            recompute_fixture_operator_from_bytes(
                &serde_json::to_vec(&experiment_plan).expect("plan bytes"),
                &serde_json::to_vec(&execution_plan).expect("execution bytes"),
                &serde_json::to_vec(&dataset).expect("dataset bytes"),
                &jsonl,
                &cross_bound,
                &negative_jsonl,
                None,
            ),
            Err(vec![P8IndependentOperatorFailureV1::ReceiptInvalid])
        );

        let first_negative_end = negative_jsonl
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("first negative record");
        let mut first_negative: serde_json::Value =
            serde_json::from_slice(&negative_jsonl[..first_negative_end]).expect("negative value");
        first_negative["provider_payload_count"] = serde_json::json!(1);
        let mut payload_tampered = serde_json::to_vec(&first_negative).expect("tampered negative");
        payload_tampered.push(b'\n');
        payload_tampered.extend_from_slice(&negative_jsonl[first_negative_end + 1..]);
        assert_eq!(
            recompute_fixture_operator_from_bytes(
                &serde_json::to_vec(&experiment_plan).expect("plan bytes"),
                &serde_json::to_vec(&execution_plan).expect("execution bytes"),
                &serde_json::to_vec(&dataset).expect("dataset bytes"),
                &jsonl,
                &ablation_jsonl,
                &payload_tampered,
                None,
            ),
            Err(vec![P8IndependentOperatorFailureV1::ReceiptInvalid])
        );
    }

    #[test]
    fn candidate_fixture_threshold_is_recomputed_and_never_release_eligible() {
        let (experiment_plan, execution_plan, dataset, jsonl, ablation_jsonl, negative_jsonl) =
            fixture_inputs(P8QualityPurpose::QualityCandidate);
        let threshold = P8FixtureQualityThresholdV1::fixture(
            &experiment_plan,
            P8ExactRationalV1 {
                numerator: 1,
                denominator: 1,
            },
            P8SignedExactRationalV1 {
                numerator: 0,
                denominator: 1,
            },
        );
        let report = recompute_fixture_operator_from_bytes(
            &serde_json::to_vec(&experiment_plan).expect("plan bytes"),
            &serde_json::to_vec(&execution_plan).expect("execution bytes"),
            &serde_json::to_vec(&dataset).expect("dataset bytes"),
            &jsonl,
            &ablation_jsonl,
            &negative_jsonl,
            Some(&serde_json::to_vec(&threshold).expect("threshold bytes")),
        )
        .expect("candidate report");
        assert_eq!(
            report.candidate_decision,
            Some(P8CandidateQualityDecisionV1::QualityPassed)
        );
        assert!(!report.release_eligible);
        assert!(report.fixture_contract_only);
        assert_eq!(report.hypothesis_evaluations.len(), 2);
        assert!(report.hypothesis_evaluations.iter().all(|evaluation| {
            evaluation.paired_bootstrap.resamples == 10_000
                && evaluation.included_questions.len() == 2
                && evaluation.holm.rank > 0
        }));
        assert_eq!(report.ablation_summaries.len(), 5);
        assert!(report
            .ablation_summaries
            .iter()
            .all(|summary| summary.pair_count > 0));
        assert_eq!(report.hard_gate_observations.len(), 11);
        assert!(report.hard_gate_failures.is_empty());
        assert!(report
            .hard_gate_observations
            .iter()
            .all(|observation| observation.passed));
        assert_eq!(report.resource_frontiers.len(), 4);
        assert!(report.resource_frontiers.iter().all(|frontier| {
            frontier.question_latency_nanoseconds_quantile > 0
                && frontier.peak_domain_memory
                    == P8FixturePeakDomainMemoryFrontierV1::NotApplicableNoTrustedLinuxAuthority
        }));
    }

    #[test]
    fn hard_gates_are_recomputed_from_retained_semantic_evidence() {
        let (experiment, _execution, _dataset, main, ablation, negative) =
            fixture_inputs(P8QualityPurpose::BaselineEstablishment);
        let main_receipts =
            parse_jsonl::<P8CompletedMainTrialReceiptV1>(&main).expect("main receipt evidence");
        let mut ablation_receipts = parse_jsonl::<P8CompletedAblationTrialReceiptV1>(&ablation)
            .expect("ablation receipt evidence");
        let negative_receipts = parse_jsonl::<P8CompletedNegativeOnlyProofReceiptV1>(&negative)
            .expect("negative receipt evidence");
        ablation_receipts[0]
            .baseline_hard_gate_evidence_mut()
            .cross_subject_private_soul_leak_count = 1;

        let observations = derive_fixture_hard_gates(
            &experiment,
            &main_receipts,
            &ablation_receipts,
            &negative_receipts,
            0,
        )
        .expect("derive hard gates from raw receipt evidence");
        let privacy = observations
            .iter()
            .find(|observation| {
                observation.gate_id == P8QualityHardGateId::CrossSubjectPrivateSoulLeak
            })
            .expect("privacy hard gate");
        assert_eq!(privacy.observed_count, 1);
        assert!(!privacy.passed);
    }

    #[cfg(unix)]
    #[test]
    fn operator_reopens_retained_runner_bundle_and_recomputes_all_raw_receipts() {
        let (experiment, execution, dataset, jsonl, ablation_jsonl, negative_jsonl) =
            fixture_inputs(P8QualityPurpose::BaselineEstablishment);
        let receipts = super::super::runner_execution::P8RealFixtureReceiptSet {
            main: parse_jsonl(&jsonl).expect("main receipts"),
            ablation: parse_jsonl(&ablation_jsonl).expect("ablation receipts"),
            negative: parse_jsonl(&negative_jsonl).expect("negative receipts"),
        };
        let files = super::super::runner_execution::build_real_fixture_runner_bundle_files(
            &experiment,
            &execution,
            &dataset,
            &receipts,
            None,
        )
        .expect("typed runner bundle files");
        verify_fixture_runner_manifests(&files, &execution)
            .expect("typed shard and cohort manifests");
        let mut empty_manifest = files.clone();
        empty_manifest.insert("cohort-manifest.json".into(), b"{}".to_vec());
        assert_eq!(
            verify_fixture_runner_manifests(&empty_manifest, &execution),
            Err(vec![P8IndependentOperatorFailureV1::InvalidJson])
        );

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "beetle-p8-operator-retained-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("retained root");
        let path = std::fs::canonicalize(path).expect("canonical retained root");
        let root = RetainedArtifactDirectory::open_root(&path).expect("open retained root");
        super::super::artifact_publisher::publish_quality_runner_bundle_no_replace(
            &root,
            "runner.stage",
            "runner-cohort",
            &execution,
            files,
        )
        .expect("publish runner cohort");
        let report = recompute_fixture_operator_from_retained_runner_bundle(&root, "runner-cohort")
            .expect("operator recomputation from retained bytes");
        assert_eq!(report.execution_run_id, execution.run_id().clone());
        assert_eq!(report.purpose, P8QualityPurpose::BaselineEstablishment);
        assert_eq!(
            report.ablation_receipt_count,
            experiment.trial_closure.safe_ablation_count()
        );
        assert_eq!(
            report.negative_receipt_count,
            experiment.trial_closure.negative_proof_count()
        );
        assert!(!report.release_eligible);

        drop(root);
        std::fs::remove_dir_all(path).expect("remove exact retained test root");
    }

    #[test]
    fn real_process_baseline_and_candidate_close_exact_receipts_before_operator_decision() {
        let dataset = crate::p8_quality::execution_plan::admit_zero_origin_tiny_dataset_manifest(
            include_bytes!("../../fixtures/p8-quality-tiny/manifest.json"),
        )
        .expect("repo tiny dataset");
        let source_questions =
            crate::p8_quality::execution_plan::admit_zero_origin_tiny_dataset_questions(
                include_bytes!("../../fixtures/p8-quality-tiny/questions.jsonl"),
                &dataset,
            )
            .expect("repo tiny question bytes");
        for (purpose, ordinal, nonce) in [
            (P8QualityPurpose::BaselineEstablishment, 71, [71; 32]),
            (P8QualityPurpose::QualityCandidate, 72, [72; 32]),
        ] {
            let experiment =
                crate::p8_quality::tests::fixture_plan_for_zero_origin_dataset(purpose, &dataset);
            let generation = P8SupervisorOwnedRunGeneration::mint_for_supervisor(
                digest("real process operator supervisor"),
                ordinal,
                nonce,
            )
            .expect("one-shot generation");
            let execution = P8QualityExecutionPlanV1::derive(&experiment, &dataset, generation)
                .expect("execution plan");
            let receipts =
                execute_real_fixture_receipt_set(&execution, &dataset, &source_questions)
                    .expect("complete real fixture process receipt set");
            let mut main = Vec::new();
            for receipt in &receipts.main {
                serde_json::to_writer(&mut main, receipt).expect("main receipt");
                main.push(b'\n');
            }
            let mut ablation = Vec::new();
            for receipt in &receipts.ablation {
                serde_json::to_writer(&mut ablation, receipt).expect("ablation receipt");
                ablation.push(b'\n');
            }
            let mut negative = Vec::new();
            for receipt in &receipts.negative {
                serde_json::to_writer(&mut negative, receipt).expect("negative receipt");
                negative.push(b'\n');
            }
            let threshold = (purpose == P8QualityPurpose::QualityCandidate).then(|| {
                serde_json::to_vec(&P8FixtureQualityThresholdV1::fixture(
                    &experiment,
                    P8ExactRationalV1 {
                        numerator: 1,
                        denominator: 1,
                    },
                    P8SignedExactRationalV1 {
                        numerator: 0,
                        denominator: 1,
                    },
                ))
                .expect("fixture threshold")
            });
            let files = build_real_fixture_runner_bundle_files(
                &experiment,
                &execution,
                &dataset,
                &receipts,
                threshold.as_deref(),
            )
            .expect("runner bundle files from raw receipts");
            let path = std::env::temp_dir().join(format!(
                "beetle-p8-real-retained-{}-{ordinal}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("real retained root");
            let path = std::fs::canonicalize(path).expect("canonical real retained root");
            let root = RetainedArtifactDirectory::open_root(&path).expect("retained root");
            super::super::artifact_publisher::publish_quality_runner_bundle_no_replace(
                &root,
                "runner.stage",
                "runner-cohort",
                &execution,
                files.clone(),
            )
            .expect("atomically publish real process runner bundle");
            assert!(
                super::super::artifact_publisher::publish_quality_runner_bundle_no_replace(
                    &root,
                    "replacement.stage",
                    "runner-cohort",
                    &execution,
                    files.clone(),
                )
                .is_err()
            );
            assert!(!path.join("replacement.stage").exists());
            let report = publish_fixture_operator_report_from_retained_runner_bundle(
                &root,
                "runner-cohort",
                "operator.stage",
                "operator-report",
            )
            .expect("operator independently reopens and publishes real process receipts");
            let (operator_manifest, operator_files) =
                super::super::artifact_publisher::open_verified_quality_bundle(
                    &root,
                    "operator-report",
                )
                .expect("reopen atomic operator report bundle");
            assert_eq!(
                operator_manifest.kind(),
                super::super::artifact_publisher::P8QualityBundleKindV1::OperatorReport
            );
            assert_eq!(
                operator_files.keys().cloned().collect::<Vec<_>>(),
                vec!["operator-report.json".to_string()]
            );
            assert_eq!(
                report.main_receipt_count,
                experiment.trial_closure.main_trial_count()
            );
            assert_eq!(
                report.ablation_receipt_count,
                experiment.trial_closure.safe_ablation_count()
            );
            assert_eq!(
                report.negative_receipt_count,
                experiment.trial_closure.negative_proof_count()
            );
            assert!(!report.release_eligible);
            assert_eq!(
                report.candidate_decision,
                (purpose == P8QualityPurpose::QualityCandidate)
                    .then_some(P8CandidateQualityDecisionV1::QualityPassed)
            );
            let persistent = [main.as_slice(), ablation.as_slice(), negative.as_slice()].concat();
            for forbidden in [
                "private-owner-sentinel",
                "raw-procedure-sentinel",
                "raw-soul-sentinel",
                "credential-sentinel",
                "path-sentinel",
            ] {
                assert!(!String::from_utf8_lossy(&persistent).contains(forbidden));
            }
            drop(root);
            std::fs::remove_dir_all(path).expect("remove exact real retained test root");
        }
    }
}
