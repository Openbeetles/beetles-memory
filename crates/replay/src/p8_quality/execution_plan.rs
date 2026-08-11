//! Mechanical P8 quality runner plan.
//!
//! This module deliberately owns scheduling only. It derives an exact, immutable work list and
//! artifact set from already-validated quality semantics, a repo-owned zero-origin fixture
//! manifest, and a supervisor-minted one-shot generation. It does not own scoring, slices,
//! confidence intervals, thresholds, or quality decisions.

use std::collections::{BTreeMap, BTreeSet};

use serde::{de::Error as _, Deserialize, Serialize};

use super::source_release::P8ArmReleaseRef;
use super::{
    deserialize_p8_quality_artifact, reject_p8_quality_raw_sentinels, P8ExecutionMode,
    P8ExperimentPlanRef, P8HardPolicyRef, P8ProtocolLockRef, P8QualityAblationKeyV1,
    P8QualityArmKind, P8QualityContractFailure, P8QualityDigest, P8QualityExperimentPlanV1,
    P8QualityId, P8QualityPurpose, P8QualityRunRef, P8QualityTrialKeyV1,
    P8SafetyNegativeProofKeyV1, P8SafetyNegativeProofKindV1, P8SameClosureSafeCounterfactualKindV1,
    P8ThresholdLockRef,
};

const P8_TINY_DATASET_SCHEMA: &str = "beetle-memory.p8.zero-origin-tiny-dataset-manifest.v1";
const P8_TINY_QUESTION_SCHEMA: &str = "beetle-memory.p8.zero-origin-tiny-question.v1";
const P8_RUN_GENERATION_SCHEMA: &str = "beetle-memory.p8.supervisor-run-generation.v1";
const P8_EXECUTION_PLAN_SCHEMA: &str = "beetle-memory.p8.quality-execution-plan.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum P8TinyDatasetOriginV1 {
    RepoOwnedZeroOriginSynthetic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum P8TinyReaderBehaviorV1 {
    Deterministic,
    SeededRepeat,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8ZeroOriginTinyQuestionSourceV1 {
    schema: String,
    question_id: P8QualityId,
    origin: P8TinyDatasetOriginV1,
    reader_behavior: P8TinyReaderBehaviorV1,
    question: String,
    rubric: String,
    gold: String,
}

impl P8ZeroOriginTinyQuestionSourceV1 {
    pub(super) fn question_id(&self) -> &P8QualityId {
        &self.question_id
    }

    pub(super) const fn reader_behavior(&self) -> P8TinyReaderBehaviorV1 {
        self.reader_behavior
    }

    pub(super) fn question(&self) -> &str {
        &self.question
    }

    pub(super) fn rubric(&self) -> &str {
        &self.rubric
    }

    pub(super) fn gold(&self) -> &str {
        &self.gold
    }

    fn matches_manifest(&self, manifest: &P8ZeroOriginTinyQuestionManifestV1) -> bool {
        self.schema == P8_TINY_QUESTION_SCHEMA
            && self.origin == P8TinyDatasetOriginV1::RepoOwnedZeroOriginSynthetic
            && !self.question.trim().is_empty()
            && !self.rubric.trim().is_empty()
            && !self.gold.trim().is_empty()
            && self.question_id == manifest.question_id
            && P8QualityDigest::derive(
                "p8_zero_origin_tiny_question_input_bytes_v1",
                &self.question.as_bytes(),
            ) == manifest.question_input_digest
            && P8QualityDigest::derive(
                "p8_zero_origin_tiny_rubric_bytes_v1",
                &self.rubric.as_bytes(),
            ) == manifest.rubric_digest
            && P8QualityDigest::derive("p8_zero_origin_tiny_gold_bytes_v1", &self.gold.as_bytes())
                == manifest.gold_digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8ZeroOriginTinyQuestionManifestV1 {
    question_id: P8QualityId,
    question_input_digest: P8QualityDigest,
    rubric_digest: P8QualityDigest,
    gold_digest: P8QualityDigest,
}

impl P8ZeroOriginTinyQuestionManifestV1 {
    pub(super) fn new(
        question_id: P8QualityId,
        question_input_digest: P8QualityDigest,
        rubric_digest: P8QualityDigest,
        gold_digest: P8QualityDigest,
    ) -> Self {
        Self {
            question_id,
            question_input_digest,
            rubric_digest,
            gold_digest,
        }
    }

    pub(super) fn question_id(&self) -> &P8QualityId {
        &self.question_id
    }

    pub(super) fn question_input_digest(&self) -> &P8QualityDigest {
        &self.question_input_digest
    }

    pub(super) fn rubric_digest(&self) -> &P8QualityDigest {
        &self.rubric_digest
    }

    pub(super) fn gold_digest(&self) -> &P8QualityDigest {
        &self.gold_digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8ZeroOriginTinyDatasetManifestV1 {
    schema: String,
    origin: P8TinyDatasetOriginV1,
    dataset_identity_digest: P8QualityDigest,
    dataset_version_digest: P8QualityDigest,
    dataset_license_digest: P8QualityDigest,
    ordered_questions: Vec<P8ZeroOriginTinyQuestionManifestV1>,
    ordered_question_ids_digest: P8QualityDigest,
    ordered_question_rubric_gold_manifest_digest: P8QualityDigest,
    input_manifest_digest: P8QualityDigest,
}

impl P8ZeroOriginTinyDatasetManifestV1 {
    pub(super) fn build(
        dataset_identity_digest: P8QualityDigest,
        dataset_version_digest: P8QualityDigest,
        dataset_license_digest: P8QualityDigest,
        ordered_questions: Vec<P8ZeroOriginTinyQuestionManifestV1>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let ordered_question_ids = ordered_questions
            .iter()
            .map(|question| question.question_id.clone())
            .collect::<Vec<_>>();
        let ordered_question_ids_digest =
            P8QualityDigest::derive("p8_quality_ordered_question_set_v1", &ordered_question_ids);
        let ordered_question_rubric_gold_manifest_digest = P8QualityDigest::derive(
            "p8_zero_origin_tiny_question_rubric_gold_manifest_v1",
            &ordered_questions,
        );
        let mut value = Self {
            schema: P8_TINY_DATASET_SCHEMA.into(),
            origin: P8TinyDatasetOriginV1::RepoOwnedZeroOriginSynthetic,
            dataset_identity_digest,
            dataset_version_digest,
            dataset_license_digest,
            ordered_questions,
            ordered_question_ids_digest,
            ordered_question_rubric_gold_manifest_digest,
            input_manifest_digest: P8QualityDigest::derive(
                "p8_zero_origin_tiny_dataset_manifest_v1",
                &(),
            ),
        };
        value.input_manifest_digest = value.derived_input_manifest_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub(super) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        let ordered_question_ids = self
            .ordered_questions
            .iter()
            .map(|question| &question.question_id)
            .collect::<Vec<_>>();
        if self.schema != P8_TINY_DATASET_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.origin != P8TinyDatasetOriginV1::RepoOwnedZeroOriginSynthetic
            || self.ordered_questions.is_empty()
            || ordered_question_ids.iter().collect::<BTreeSet<_>>().len()
                != ordered_question_ids.len()
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        let owned_question_ids = ordered_question_ids
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        if self.ordered_question_ids_digest
            != P8QualityDigest::derive("p8_quality_ordered_question_set_v1", &owned_question_ids)
            || self.ordered_question_rubric_gold_manifest_digest
                != P8QualityDigest::derive(
                    "p8_zero_origin_tiny_question_rubric_gold_manifest_v1",
                    &self.ordered_questions,
                )
            || self.input_manifest_digest != self.derived_input_manifest_digest()
        {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_input_manifest_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_zero_origin_tiny_dataset_manifest_v1",
            &(
                &self.schema,
                self.origin,
                &self.dataset_identity_digest,
                &self.dataset_version_digest,
                &self.dataset_license_digest,
                &self.ordered_questions,
                &self.ordered_question_ids_digest,
                &self.ordered_question_rubric_gold_manifest_digest,
            ),
        )
    }

    fn ordered_question_ids(&self) -> Vec<P8QualityId> {
        self.ordered_questions
            .iter()
            .map(|question| question.question_id.clone())
            .collect()
    }

    pub(super) fn ordered_questions(&self) -> &[P8ZeroOriginTinyQuestionManifestV1] {
        &self.ordered_questions
    }

    pub(super) fn input_manifest_digest(&self) -> &P8QualityDigest {
        &self.input_manifest_digest
    }

    pub(super) fn dataset_identity_digest(&self) -> &P8QualityDigest {
        &self.dataset_identity_digest
    }

    pub(super) fn dataset_version_digest(&self) -> &P8QualityDigest {
        &self.dataset_version_digest
    }

    pub(super) fn dataset_license_digest(&self) -> &P8QualityDigest {
        &self.dataset_license_digest
    }

    pub(super) fn ordered_question_ids_digest(&self) -> &P8QualityDigest {
        &self.ordered_question_ids_digest
    }

    pub(super) fn ordered_question_rubric_gold_manifest_digest(&self) -> &P8QualityDigest {
        &self.ordered_question_rubric_gold_manifest_digest
    }
}

pub(super) fn admit_zero_origin_tiny_dataset_manifest(
    bytes: &[u8],
) -> serde_json::Result<P8ZeroOriginTinyDatasetManifestV1> {
    reject_p8_quality_raw_sentinels(bytes)?;
    let manifest: P8ZeroOriginTinyDatasetManifestV1 = deserialize_p8_quality_artifact(bytes)?;
    let failures = manifest.validate_contract();
    if failures.is_empty() {
        Ok(manifest)
    } else {
        Err(serde_json::Error::custom(format!(
            "P8 zero-origin tiny dataset manifest rejected: {failures:?}"
        )))
    }
}

pub(super) fn admit_zero_origin_tiny_dataset_questions(
    bytes: &[u8],
    manifest: &P8ZeroOriginTinyDatasetManifestV1,
) -> serde_json::Result<Vec<P8ZeroOriginTinyQuestionSourceV1>> {
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return Err(serde_json::Error::custom(
            "P8 zero-origin tiny question JSONL is not record-terminated",
        ));
    }
    let mut questions: Vec<P8ZeroOriginTinyQuestionSourceV1> = Vec::new();
    for line in bytes[..bytes.len() - 1].split(|byte| *byte == b'\n') {
        if line.is_empty() {
            return Err(serde_json::Error::custom(
                "P8 zero-origin tiny question JSONL contains an empty record",
            ));
        }
        reject_p8_quality_raw_sentinels(line)?;
        questions.push(deserialize_p8_quality_artifact(line)?);
    }
    if questions.len() != manifest.ordered_questions.len()
        || questions
            .iter()
            .zip(&manifest.ordered_questions)
            .any(|(question, expected)| !question.matches_manifest(expected))
    {
        return Err(serde_json::Error::custom(
            "P8 zero-origin tiny question bytes differ from the admitted manifest",
        ));
    }
    Ok(questions)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8SupervisorRunGenerationBindingV1 {
    schema: String,
    supervisor_session_receipt_digest: P8QualityDigest,
    generation_ordinal: u64,
    fresh_nonce_digest: P8QualityDigest,
    binding_digest: P8QualityDigest,
}

impl P8SupervisorRunGenerationBindingV1 {
    fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_RUN_GENERATION_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.generation_ordinal == 0 {
            failures.push(P8QualityContractFailure::IdentityInvalid);
        }
        if self.binding_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_supervisor_run_generation_binding_v1",
            &(
                &self.schema,
                &self.supervisor_session_receipt_digest,
                self.generation_ordinal,
                &self.fresh_nonce_digest,
            ),
        )
    }
}

/// One-shot capability minted by the supervisor owner and consumed by plan derivation.
///
/// It intentionally implements neither `Clone` nor serde. The runner receives only the resulting
/// immutable binding and therefore has no API for choosing a run id.
#[derive(Debug)]
pub(super) struct P8SupervisorOwnedRunGeneration {
    binding: P8SupervisorRunGenerationBindingV1,
}

impl P8SupervisorOwnedRunGeneration {
    pub(super) fn mint_for_supervisor(
        supervisor_session_receipt_digest: P8QualityDigest,
        generation_ordinal: u64,
        fresh_nonce: [u8; 32],
    ) -> Result<Self, P8QualityContractFailure> {
        if generation_ordinal == 0 {
            return Err(P8QualityContractFailure::IdentityInvalid);
        }
        let mut binding = P8SupervisorRunGenerationBindingV1 {
            schema: P8_RUN_GENERATION_SCHEMA.into(),
            supervisor_session_receipt_digest,
            generation_ordinal,
            fresh_nonce_digest: P8QualityDigest::derive(
                "p8_supervisor_run_generation_nonce_v1",
                &fresh_nonce,
            ),
            binding_digest: P8QualityDigest::derive("p8_supervisor_run_generation_binding_v1", &()),
        };
        binding.binding_digest = binding.derived_digest();
        Ok(Self { binding })
    }

    fn consume(self) -> P8SupervisorRunGenerationBindingV1 {
        self.binding
    }

    pub(super) fn admit_supervisor_binding(
        bytes: &[u8],
    ) -> serde_json::Result<P8SupervisorOwnedRunGeneration> {
        reject_p8_quality_raw_sentinels(bytes)?;
        let binding: P8SupervisorRunGenerationBindingV1 = deserialize_p8_quality_artifact(bytes)?;
        let failures = binding.validate_contract();
        if failures.is_empty() {
            Ok(Self { binding })
        } else {
            Err(serde_json::Error::custom(format!(
                "P8 supervisor run generation binding rejected: {failures:?}"
            )))
        }
    }

    pub(super) fn serialized_binding(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(&self.binding)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum P8MechanicalWorkKindV1 {
    Main,
    SameClosureAblation {
        counterfactual: P8SameClosureSafeCounterfactualKindV1,
    },
    SafetyNegativeProof {
        proof: P8SafetyNegativeProofKindV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8MechanicalWorkItemV1 {
    schedule_ordinal: u64,
    shard_index: u32,
    question_id: P8QualityId,
    arm: P8QualityArmKind,
    arm_release_digest: P8ArmReleaseRef,
    reader_repeat_index: Option<u32>,
    judge_repeat_index: Option<u32>,
    kind: P8MechanicalWorkKindV1,
}

impl P8MechanicalWorkItemV1 {
    pub(super) const fn schedule_ordinal(&self) -> u64 {
        self.schedule_ordinal
    }

    pub(super) const fn shard_index(&self) -> u32 {
        self.shard_index
    }

    pub(super) fn question_id(&self) -> &P8QualityId {
        &self.question_id
    }

    pub(super) const fn arm(&self) -> P8QualityArmKind {
        self.arm
    }

    pub(super) fn arm_release_digest(&self) -> &P8ArmReleaseRef {
        &self.arm_release_digest
    }

    pub(super) const fn reader_repeat_index(&self) -> Option<u32> {
        self.reader_repeat_index
    }

    pub(super) const fn judge_repeat_index(&self) -> Option<u32> {
        self.judge_repeat_index
    }

    pub(super) const fn kind(&self) -> P8MechanicalWorkKindV1 {
        self.kind
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8QuestionShardAssignmentV1 {
    question_id: P8QualityId,
    shard_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8QualityExecutionPlanV1 {
    schema: String,
    purpose: P8QualityPurpose,
    execution_mode: P8ExecutionMode,
    experiment_plan_digest: P8ExperimentPlanRef,
    experiment_plan_run_id: P8QualityRunRef,
    protocol_digest: P8ProtocolLockRef,
    hard_policy_digest: P8HardPolicyRef,
    threshold_digest: Option<P8ThresholdLockRef>,
    dataset_manifest_digest: P8QualityDigest,
    run_generation: P8SupervisorRunGenerationBindingV1,
    run_id: P8QualityRunRef,
    shard_total: u32,
    question_shards: Vec<P8QuestionShardAssignmentV1>,
    work_items: Vec<P8MechanicalWorkItemV1>,
    exact_artifact_names: Vec<String>,
    execution_plan_digest: P8QualityDigest,
}

impl P8QualityExecutionPlanV1 {
    pub(super) const fn purpose(&self) -> P8QualityPurpose {
        self.purpose
    }

    pub(super) fn derive(
        experiment_plan: &P8QualityExperimentPlanV1,
        dataset: &P8ZeroOriginTinyDatasetManifestV1,
        run_generation: P8SupervisorOwnedRunGeneration,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let mut failures = experiment_plan.validate_contract();
        failures.extend(dataset.validate_contract());
        if experiment_plan.execution_mode != P8ExecutionMode::FixtureContract {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        let view = P8ValidatedMechanicalPlanView::from_experiment_plan(experiment_plan);
        failures.extend(view.validate_dataset(dataset));
        failures.sort();
        failures.dedup();
        if !failures.is_empty() {
            return Err(failures);
        }
        Self::derive_from_validated_view(view, dataset, run_generation)
    }

    pub(super) fn validate_against(
        &self,
        experiment_plan: &P8QualityExperimentPlanV1,
        dataset: &P8ZeroOriginTinyDatasetManifestV1,
    ) -> Vec<P8QualityContractFailure> {
        let mut failures = experiment_plan.validate_contract();
        failures.extend(dataset.validate_contract());
        let view = P8ValidatedMechanicalPlanView::from_experiment_plan(experiment_plan);
        failures.extend(view.validate_dataset(dataset));
        failures.extend(self.validate_against_view(&view, dataset));
        failures.sort();
        failures.dedup();
        failures
    }

    pub(super) fn run_id(&self) -> &P8QualityRunRef {
        &self.run_id
    }

    pub(super) const fn shard_total(&self) -> u32 {
        self.shard_total
    }

    pub(super) fn work_items(&self) -> &[P8MechanicalWorkItemV1] {
        &self.work_items
    }

    pub(super) fn exact_artifact_names(&self) -> &[String] {
        &self.exact_artifact_names
    }

    pub(super) fn execution_plan_digest(&self) -> &P8QualityDigest {
        &self.execution_plan_digest
    }

    fn derive_from_validated_view(
        view: P8ValidatedMechanicalPlanView,
        dataset: &P8ZeroOriginTinyDatasetManifestV1,
        run_generation: P8SupervisorOwnedRunGeneration,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let run_generation = run_generation.consume();
        let run_id = P8QualityRunRef::derive(&(
            "p8_quality_execution_run_v1",
            &view.experiment_plan_run_id,
            &view.experiment_plan_digest,
            &dataset.input_manifest_digest,
            &run_generation,
        ));
        let question_shards = derive_question_shards(&view.ordered_questions, view.shard_total);
        let shard_by_question = question_shards
            .iter()
            .map(|assignment| (assignment.question_id.clone(), assignment.shard_index))
            .collect::<BTreeMap<_, _>>();
        let mut work_items = Vec::new();
        append_main_work_items(&view, &shard_by_question, &mut work_items)
            .map_err(|failure| vec![failure])?;
        append_ablation_work_items(&view, &shard_by_question, &mut work_items)
            .map_err(|failure| vec![failure])?;
        append_negative_work_items(&view, &shard_by_question, &mut work_items)
            .map_err(|failure| vec![failure])?;
        let exact_artifact_names =
            exact_runner_artifact_names(view.shard_total, view.threshold_digest.is_some());
        let mut value = Self {
            schema: P8_EXECUTION_PLAN_SCHEMA.into(),
            purpose: view.purpose,
            execution_mode: view.execution_mode,
            experiment_plan_digest: view.experiment_plan_digest.clone(),
            experiment_plan_run_id: view.experiment_plan_run_id.clone(),
            protocol_digest: view.protocol_digest.clone(),
            hard_policy_digest: view.hard_policy_digest.clone(),
            threshold_digest: view.threshold_digest.clone(),
            dataset_manifest_digest: dataset.input_manifest_digest.clone(),
            run_generation,
            run_id,
            shard_total: view.shard_total,
            question_shards,
            work_items,
            exact_artifact_names,
            execution_plan_digest: P8QualityDigest::derive("p8_quality_execution_plan_v1", &()),
        };
        value.execution_plan_digest = value.derived_digest();
        let failures = value.validate_against_view(&view, dataset);
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    fn validate_against_view(
        &self,
        view: &P8ValidatedMechanicalPlanView,
        dataset: &P8ZeroOriginTinyDatasetManifestV1,
    ) -> Vec<P8QualityContractFailure> {
        let mut failures = self.run_generation.validate_contract();
        if self.schema != P8_EXECUTION_PLAN_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.purpose != view.purpose
            || self.execution_mode != P8ExecutionMode::FixtureContract
            || self.execution_mode != view.execution_mode
            || self.experiment_plan_digest != view.experiment_plan_digest
            || self.experiment_plan_run_id != view.experiment_plan_run_id
            || self.protocol_digest != view.protocol_digest
            || self.hard_policy_digest != view.hard_policy_digest
            || self.threshold_digest != view.threshold_digest
            || self.dataset_manifest_digest != dataset.input_manifest_digest
            || self.shard_total != view.shard_total
        {
            failures.push(P8QualityContractFailure::PurposeMismatch);
        }
        let expected_run_id = P8QualityRunRef::derive(&(
            "p8_quality_execution_run_v1",
            &self.experiment_plan_run_id,
            &self.experiment_plan_digest,
            &self.dataset_manifest_digest,
            &self.run_generation,
        ));
        if self.run_id != expected_run_id {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        let expected_assignments =
            derive_question_shards(&view.ordered_questions, view.shard_total);
        if self.question_shards != expected_assignments {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        let shard_by_question = expected_assignments
            .iter()
            .map(|assignment| (assignment.question_id.clone(), assignment.shard_index))
            .collect::<BTreeMap<_, _>>();
        let expected_work_items = expected_work_items(view, &shard_by_question);
        match expected_work_items {
            Ok(expected) if self.work_items == expected => {}
            Ok(_) => failures.push(P8QualityContractFailure::CoverageMismatch),
            Err(failure) => failures.push(failure),
        }
        let expected_names =
            exact_runner_artifact_names(view.shard_total, view.threshold_digest.is_some());
        if self.exact_artifact_names != expected_names
            || self
                .exact_artifact_names
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.exact_artifact_names.len()
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        if self.execution_plan_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures.sort();
        failures.dedup();
        failures
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_quality_execution_plan_v1",
            &(
                &self.schema,
                self.purpose,
                self.execution_mode,
                &self.experiment_plan_digest,
                &self.experiment_plan_run_id,
                &self.protocol_digest,
                &self.hard_policy_digest,
                &self.threshold_digest,
                &self.dataset_manifest_digest,
                &self.run_generation,
                &self.run_id,
                self.shard_total,
                &self.question_shards,
                &self.work_items,
                &self.exact_artifact_names,
            ),
        )
    }
}

pub(super) fn admit_quality_execution_plan(
    bytes: &[u8],
    experiment_plan: &P8QualityExperimentPlanV1,
    dataset: &P8ZeroOriginTinyDatasetManifestV1,
) -> serde_json::Result<P8QualityExecutionPlanV1> {
    reject_p8_quality_raw_sentinels(bytes)?;
    let plan: P8QualityExecutionPlanV1 = deserialize_p8_quality_artifact(bytes)?;
    let failures = plan.validate_against(experiment_plan, dataset);
    if failures.is_empty() {
        Ok(plan)
    } else {
        Err(serde_json::Error::custom(format!(
            "P8 quality execution plan rejected: {failures:?}"
        )))
    }
}

struct P8ValidatedMechanicalPlanView {
    purpose: P8QualityPurpose,
    execution_mode: P8ExecutionMode,
    experiment_plan_digest: P8ExperimentPlanRef,
    experiment_plan_run_id: P8QualityRunRef,
    protocol_digest: P8ProtocolLockRef,
    hard_policy_digest: P8HardPolicyRef,
    threshold_digest: Option<P8ThresholdLockRef>,
    dataset_identity_digest: P8QualityDigest,
    dataset_version_digest: P8QualityDigest,
    dataset_license_digest: P8QualityDigest,
    dataset_input_manifest_digest: P8QualityDigest,
    ordered_question_rubric_gold_manifest_digest: P8QualityDigest,
    ordered_question_ids_digest: P8QualityDigest,
    ordered_questions: Vec<P8QualityId>,
    shard_total: u32,
    main_keys: Vec<P8QualityTrialKeyV1>,
    ablation_keys: Vec<P8QualityAblationKeyV1>,
    negative_keys: Vec<P8SafetyNegativeProofKeyV1>,
    arm_releases: BTreeMap<P8QualityArmKind, P8ArmReleaseRef>,
}

impl P8ValidatedMechanicalPlanView {
    fn from_experiment_plan(plan: &P8QualityExperimentPlanV1) -> Self {
        let arm_releases = P8QualityArmKind::expected_for(plan.purpose)
            .iter()
            .map(|arm| {
                (
                    *arm,
                    plan.arm_release_digest(*arm)
                        .expect("validated plan has every applicable arm")
                        .clone(),
                )
            })
            .collect();
        Self {
            purpose: plan.purpose,
            execution_mode: plan.execution_mode,
            experiment_plan_digest: plan.plan_digest.clone(),
            experiment_plan_run_id: plan.run_id.clone(),
            protocol_digest: plan.protocol.protocol_digest().clone(),
            hard_policy_digest: plan.hard_policy.policy_digest().clone(),
            threshold_digest: plan
                .threshold
                .as_ref()
                .map(|threshold| threshold.threshold_digest().clone()),
            dataset_identity_digest: plan.protocol.frozen.dataset.dataset_identity_digest.clone(),
            dataset_version_digest: plan.protocol.frozen.dataset.dataset_version_digest.clone(),
            dataset_license_digest: plan.protocol.frozen.dataset.dataset_license_digest.clone(),
            dataset_input_manifest_digest: plan
                .protocol
                .frozen
                .dataset
                .input_manifest_digest
                .clone(),
            ordered_question_rubric_gold_manifest_digest: plan
                .protocol
                .frozen
                .dataset
                .ordered_question_rubric_gold_manifest_digest
                .clone(),
            ordered_question_ids_digest: plan
                .protocol
                .frozen
                .dataset
                .ordered_question_ids_digest
                .clone(),
            ordered_questions: plan.trial_closure.ordered_questions.clone(),
            shard_total: plan.protocol.frozen.resources.shard_count,
            main_keys: plan.trial_closure.expected_main_trial_keys(),
            ablation_keys: plan.trial_closure.expected_ablation_keys(),
            negative_keys: plan.trial_closure.expected_negative_proof_keys(),
            arm_releases,
        }
    }

    fn validate_dataset(
        &self,
        dataset: &P8ZeroOriginTinyDatasetManifestV1,
    ) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.execution_mode != P8ExecutionMode::FixtureContract {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        if self.dataset_identity_digest != dataset.dataset_identity_digest
            || self.dataset_version_digest != dataset.dataset_version_digest
            || self.dataset_license_digest != dataset.dataset_license_digest
            || self.dataset_input_manifest_digest != dataset.input_manifest_digest
            || self.ordered_question_rubric_gold_manifest_digest
                != dataset.ordered_question_rubric_gold_manifest_digest
            || self.ordered_question_ids_digest != dataset.ordered_question_ids_digest
            || self.ordered_questions != dataset.ordered_question_ids()
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        failures
    }
}

fn derive_question_shards(
    ordered_questions: &[P8QualityId],
    shard_total: u32,
) -> Vec<P8QuestionShardAssignmentV1> {
    ordered_questions
        .iter()
        .enumerate()
        .map(|(index, question_id)| P8QuestionShardAssignmentV1 {
            question_id: question_id.clone(),
            shard_index: u32::try_from(index).expect("question index fits u32") % shard_total,
        })
        .collect()
}

fn expected_work_items(
    view: &P8ValidatedMechanicalPlanView,
    shard_by_question: &BTreeMap<P8QualityId, u32>,
) -> Result<Vec<P8MechanicalWorkItemV1>, P8QualityContractFailure> {
    let mut work_items = Vec::new();
    append_main_work_items(view, shard_by_question, &mut work_items)?;
    append_ablation_work_items(view, shard_by_question, &mut work_items)?;
    append_negative_work_items(view, shard_by_question, &mut work_items)?;
    Ok(work_items)
}

fn append_main_work_items(
    view: &P8ValidatedMechanicalPlanView,
    shard_by_question: &BTreeMap<P8QualityId, u32>,
    work_items: &mut Vec<P8MechanicalWorkItemV1>,
) -> Result<(), P8QualityContractFailure> {
    for key in &view.main_keys {
        push_work_item(
            view,
            shard_by_question,
            work_items,
            key.question_id.clone(),
            key.arm,
            Some(key.reader_repeat_index),
            Some(key.judge_repeat_index),
            P8MechanicalWorkKindV1::Main,
        )?;
    }
    Ok(())
}

fn append_ablation_work_items(
    view: &P8ValidatedMechanicalPlanView,
    shard_by_question: &BTreeMap<P8QualityId, u32>,
    work_items: &mut Vec<P8MechanicalWorkItemV1>,
) -> Result<(), P8QualityContractFailure> {
    for key in &view.ablation_keys {
        push_work_item(
            view,
            shard_by_question,
            work_items,
            key.question_id.clone(),
            key.arm,
            Some(key.reader_repeat_index),
            Some(key.judge_repeat_index),
            P8MechanicalWorkKindV1::SameClosureAblation {
                counterfactual: key.counterfactual,
            },
        )?;
    }
    Ok(())
}

fn append_negative_work_items(
    view: &P8ValidatedMechanicalPlanView,
    shard_by_question: &BTreeMap<P8QualityId, u32>,
    work_items: &mut Vec<P8MechanicalWorkItemV1>,
) -> Result<(), P8QualityContractFailure> {
    for key in &view.negative_keys {
        push_work_item(
            view,
            shard_by_question,
            work_items,
            key.question_id.clone(),
            key.arm,
            None,
            None,
            P8MechanicalWorkKindV1::SafetyNegativeProof { proof: key.proof },
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_work_item(
    view: &P8ValidatedMechanicalPlanView,
    shard_by_question: &BTreeMap<P8QualityId, u32>,
    work_items: &mut Vec<P8MechanicalWorkItemV1>,
    question_id: P8QualityId,
    arm: P8QualityArmKind,
    reader_repeat_index: Option<u32>,
    judge_repeat_index: Option<u32>,
    kind: P8MechanicalWorkKindV1,
) -> Result<(), P8QualityContractFailure> {
    let shard_index = shard_by_question
        .get(&question_id)
        .copied()
        .ok_or(P8QualityContractFailure::CoverageMismatch)?;
    let arm_release_digest = view
        .arm_releases
        .get(&arm)
        .cloned()
        .ok_or(P8QualityContractFailure::ArmSetMismatch)?;
    let schedule_ordinal = u64::try_from(work_items.len())
        .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?;
    work_items.push(P8MechanicalWorkItemV1 {
        schedule_ordinal,
        shard_index,
        question_id,
        arm,
        arm_release_digest,
        reader_repeat_index,
        judge_repeat_index,
        kind,
    });
    Ok(())
}

fn exact_runner_artifact_names(shard_total: u32, has_fixture_threshold: bool) -> Vec<String> {
    let mut names = vec![
        "experiment-plan.json".into(),
        "dataset-manifest.json".into(),
        "execution-plan.json".into(),
    ];
    if has_fixture_threshold {
        names.push("fixture-threshold.json".into());
    }
    for shard_index in 0..shard_total {
        names.extend([
            format!("shard-{shard_index:05}.main.jsonl"),
            format!("shard-{shard_index:05}.ablation.jsonl"),
            format!("shard-{shard_index:05}.negative.jsonl"),
            format!("shard-{shard_index:05}.manifest.json"),
        ]);
    }
    names.push("cohort-manifest.json".into());
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: &str) -> P8QualityDigest {
        P8QualityDigest::derive("p8_execution_plan_test_digest", &seed)
    }

    fn question(value: &str) -> P8QualityId {
        P8QualityId::parse(value).expect("canonical question")
    }

    fn dataset(questions: &[P8QualityId]) -> P8ZeroOriginTinyDatasetManifestV1 {
        P8ZeroOriginTinyDatasetManifestV1::build(
            digest("dataset"),
            digest("version"),
            digest("license"),
            questions
                .iter()
                .map(|question_id| {
                    P8ZeroOriginTinyQuestionManifestV1::new(
                        question_id.clone(),
                        P8QualityDigest::derive("question", question_id),
                        P8QualityDigest::derive("rubric", question_id),
                        P8QualityDigest::derive("gold", question_id),
                    )
                })
                .collect(),
        )
        .expect("zero-origin dataset")
    }

    fn view(
        purpose: P8QualityPurpose,
        dataset: &P8ZeroOriginTinyDatasetManifestV1,
    ) -> P8ValidatedMechanicalPlanView {
        let ordered_questions = dataset.ordered_question_ids();
        let closure =
            super::super::P8QualityTrialClosureV1::derive(purpose, ordered_questions.clone(), 2, 3)
                .expect("trial closure");
        let arm_releases = P8QualityArmKind::expected_for(purpose)
            .iter()
            .map(|arm| (*arm, P8ArmReleaseRef::derive_for_test(&format!("{arm:?}"))))
            .collect();
        P8ValidatedMechanicalPlanView {
            purpose,
            execution_mode: P8ExecutionMode::FixtureContract,
            experiment_plan_digest: P8ExperimentPlanRef::derive(&("experiment", purpose)),
            experiment_plan_run_id: P8QualityRunRef::derive(&("semantic-run", purpose)),
            protocol_digest: P8ProtocolLockRef::derive(&("protocol", purpose)),
            hard_policy_digest: P8HardPolicyRef::derive(&"hard-policy"),
            threshold_digest: (purpose == P8QualityPurpose::QualityCandidate)
                .then(|| P8ThresholdLockRef::derive(&"threshold")),
            dataset_identity_digest: dataset.dataset_identity_digest.clone(),
            dataset_version_digest: dataset.dataset_version_digest.clone(),
            dataset_license_digest: dataset.dataset_license_digest.clone(),
            dataset_input_manifest_digest: dataset.input_manifest_digest.clone(),
            ordered_question_rubric_gold_manifest_digest: dataset
                .ordered_question_rubric_gold_manifest_digest
                .clone(),
            ordered_question_ids_digest: dataset.ordered_question_ids_digest.clone(),
            ordered_questions,
            shard_total: 2,
            main_keys: closure.expected_main_trial_keys(),
            ablation_keys: closure.expected_ablation_keys(),
            negative_keys: closure.expected_negative_proof_keys(),
            arm_releases,
        }
    }

    fn generation(seed: u8) -> P8SupervisorOwnedRunGeneration {
        P8SupervisorOwnedRunGeneration::mint_for_supervisor(
            digest("supervisor-session"),
            u64::from(seed) + 1,
            [seed; 32],
        )
        .expect("supervisor generation")
    }

    #[test]
    fn mechanical_plan_derives_exact_baseline_and_candidate_work_without_runner_choices() {
        let questions = vec![question("q-1"), question("q-2")];
        let dataset = dataset(&questions);
        for (purpose, expected_arms, expected_beetle_arms) in [
            (P8QualityPurpose::BaselineEstablishment, 3_u64, 1_u64),
            (P8QualityPurpose::QualityCandidate, 4_u64, 2_u64),
        ] {
            let view = view(purpose, &dataset);
            assert!(view.validate_dataset(&dataset).is_empty());
            let plan = P8QualityExecutionPlanV1::derive_from_validated_view(
                view,
                &dataset,
                generation(expected_arms as u8),
            )
            .expect("mechanical plan");
            let main = plan
                .work_items
                .iter()
                .filter(|item| item.kind == P8MechanicalWorkKindV1::Main)
                .count() as u64;
            let ablation = plan
                .work_items
                .iter()
                .filter(|item| {
                    matches!(
                        item.kind,
                        P8MechanicalWorkKindV1::SameClosureAblation { .. }
                    )
                })
                .count() as u64;
            let negative = plan
                .work_items
                .iter()
                .filter(|item| {
                    matches!(
                        item.kind,
                        P8MechanicalWorkKindV1::SafetyNegativeProof { .. }
                    )
                })
                .count() as u64;
            assert_eq!(main, 2 * expected_arms * 2 * 3);
            assert_eq!(ablation, 2 * expected_beetle_arms * 5 * 2 * 3);
            assert_eq!(negative, 2 * expected_beetle_arms * 3);
            assert_eq!(
                plan.question_shards,
                vec![
                    P8QuestionShardAssignmentV1 {
                        question_id: question("q-1"),
                        shard_index: 0,
                    },
                    P8QuestionShardAssignmentV1 {
                        question_id: question("q-2"),
                        shard_index: 1,
                    },
                ]
            );
            assert_eq!(
                plan.exact_artifact_names,
                exact_runner_artifact_names(2, purpose == P8QualityPurpose::QualityCandidate)
            );
            assert!(plan.work_items.iter().enumerate().all(|(index, item)| {
                item.schedule_ordinal == u64::try_from(index).expect("test index fits u64")
            }));
        }
    }

    #[test]
    fn supervisor_generation_is_consumed_and_deterministically_owns_run_id() {
        let dataset = dataset(&[question("q-1"), question("q-2")]);
        let first = P8QualityExecutionPlanV1::derive_from_validated_view(
            view(P8QualityPurpose::BaselineEstablishment, &dataset),
            &dataset,
            generation(1),
        )
        .expect("first plan");
        let same = P8QualityExecutionPlanV1::derive_from_validated_view(
            view(P8QualityPurpose::BaselineEstablishment, &dataset),
            &dataset,
            generation(1),
        )
        .expect("same plan");
        let next = P8QualityExecutionPlanV1::derive_from_validated_view(
            view(P8QualityPurpose::BaselineEstablishment, &dataset),
            &dataset,
            generation(2),
        )
        .expect("next plan");
        assert_eq!(first.run_id, same.run_id);
        assert_eq!(first.execution_plan_digest, same.execution_plan_digest);
        assert_ne!(first.run_id, next.run_id);
        assert_ne!(first.execution_plan_digest, next.execution_plan_digest);
    }

    #[test]
    fn runner_admits_only_exact_supervisor_generation_binding_bytes() {
        let generation = P8SupervisorOwnedRunGeneration::mint_for_supervisor(
            digest("supervisor session"),
            41,
            [41; 32],
        )
        .expect("supervisor generation");
        let bytes = generation
            .serialized_binding()
            .expect("serialize generation binding");
        P8SupervisorOwnedRunGeneration::admit_supervisor_binding(&bytes)
            .expect("admit exact immutable binding");

        let mut tampered: serde_json::Value =
            serde_json::from_slice(&bytes).expect("generation JSON");
        tampered["generation_ordinal"] = serde_json::json!(42);
        assert!(P8SupervisorOwnedRunGeneration::admit_supervisor_binding(
            &serde_json::to_vec(&tampered).expect("tampered binding")
        )
        .is_err());
    }

    #[test]
    fn plan_validation_rejects_schedule_shard_artifact_and_run_identity_drift() {
        let dataset = dataset(&[question("q-1"), question("q-2")]);
        let baseline_view = view(P8QualityPurpose::BaselineEstablishment, &dataset);
        let plan = P8QualityExecutionPlanV1::derive_from_validated_view(
            view(P8QualityPurpose::BaselineEstablishment, &dataset),
            &dataset,
            generation(7),
        )
        .expect("plan");

        let mut schedule = plan.clone();
        schedule.work_items.swap(0, 1);
        schedule.execution_plan_digest = schedule.derived_digest();
        assert!(schedule
            .validate_against_view(&baseline_view, &dataset)
            .contains(&P8QualityContractFailure::CoverageMismatch));

        let mut shard = plan.clone();
        shard.question_shards[0].shard_index = 1;
        shard.execution_plan_digest = shard.derived_digest();
        assert!(shard
            .validate_against_view(&baseline_view, &dataset)
            .contains(&P8QualityContractFailure::CoverageMismatch));

        let mut artifact = plan.clone();
        artifact.exact_artifact_names.pop();
        artifact.execution_plan_digest = artifact.derived_digest();
        assert!(artifact
            .validate_against_view(&baseline_view, &dataset)
            .contains(&P8QualityContractFailure::CoverageMismatch));

        let mut run = plan;
        run.run_id = P8QualityRunRef::derive(&"caller-selected-run");
        run.execution_plan_digest = run.derived_digest();
        assert!(run
            .validate_against_view(&baseline_view, &dataset)
            .contains(&P8QualityContractFailure::DigestInvalid));
    }

    #[test]
    fn zero_origin_dataset_and_plan_bind_every_protocol_dataset_identity() {
        let dataset = dataset(&[question("q-1"), question("q-2")]);
        let mut drifted = dataset.clone();
        drifted.ordered_questions[0].gold_digest = digest("different-gold");
        assert!(drifted
            .validate_contract()
            .contains(&P8QualityContractFailure::DigestInvalid));

        let mut wrong_view = view(P8QualityPurpose::BaselineEstablishment, &dataset);
        wrong_view.dataset_license_digest = digest("different-license");
        assert!(wrong_view
            .validate_dataset(&dataset)
            .contains(&P8QualityContractFailure::CoverageMismatch));
    }

    #[test]
    fn tiny_dataset_manifest_admission_is_strict_and_rejects_raw_material() {
        let dataset = dataset(&[question("q-1"), question("q-2")]);
        let bytes = serde_json::to_vec(&dataset).expect("dataset JSON");
        assert_eq!(
            admit_zero_origin_tiny_dataset_manifest(&bytes).expect("admitted dataset"),
            dataset
        );

        let mut unknown = serde_json::to_value(&dataset).expect("dataset value");
        unknown["runner_selected_threshold"] = serde_json::json!(1);
        assert!(admit_zero_origin_tiny_dataset_manifest(
            &serde_json::to_vec(&unknown).expect("unknown JSON")
        )
        .is_err());

        let text = String::from_utf8(bytes).expect("dataset UTF-8");
        let duplicate = text.replacen(
            "\"schema\":",
            "\"schema\":\"beetle-memory.p8.zero-origin-tiny-dataset-manifest.v1\",\"schema\":",
            1,
        );
        assert!(admit_zero_origin_tiny_dataset_manifest(duplicate.as_bytes()).is_err());
        assert!(admit_zero_origin_tiny_dataset_manifest(b"\"raw-soul-sentinel\"").is_err());
    }

    #[test]
    fn serialized_execution_plan_has_no_runner_owned_statistics_slices_or_decisions() {
        let dataset = dataset(&[question("q-1"), question("q-2")]);
        let plan = P8QualityExecutionPlanV1::derive_from_validated_view(
            view(P8QualityPurpose::QualityCandidate, &dataset),
            &dataset,
            generation(9),
        )
        .expect("candidate plan");
        let json = serde_json::to_string(&plan).expect("plan JSON");
        for forbidden in [
            "confidence_level",
            "bootstrap_resamples",
            "hypothesis_registry",
            "quality_passed",
            "threshold_evaluation",
            "candidate_decision",
            "observed_score",
        ] {
            assert!(!json.contains(forbidden), "runner plan owns {forbidden}");
        }
        assert!(json.contains("threshold_digest"));
    }

    #[test]
    fn repo_tiny_dataset_files_are_zero_origin_and_admitted() {
        #[derive(Deserialize)]
        struct RawQuestion {
            question_id: String,
            question: String,
            rubric: String,
            gold: String,
        }
        let questions = include_str!("../../fixtures/p8-quality-tiny/questions.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<RawQuestion>(line).expect("raw tiny question"))
            .map(|raw| {
                P8ZeroOriginTinyQuestionManifestV1::new(
                    P8QualityId::parse(raw.question_id).expect("question id"),
                    P8QualityDigest::derive(
                        "p8_zero_origin_tiny_question_input_bytes_v1",
                        &raw.question.as_bytes(),
                    ),
                    P8QualityDigest::derive(
                        "p8_zero_origin_tiny_rubric_bytes_v1",
                        &raw.rubric.as_bytes(),
                    ),
                    P8QualityDigest::derive(
                        "p8_zero_origin_tiny_gold_bytes_v1",
                        &raw.gold.as_bytes(),
                    ),
                )
            })
            .collect::<Vec<_>>();
        let manifest = P8ZeroOriginTinyDatasetManifestV1::build(
            P8QualityDigest::derive(
                "p8_zero_origin_tiny_dataset_identity_v1",
                &"beetle-memory-p8-quality-tiny",
            ),
            P8QualityDigest::derive("p8_zero_origin_tiny_dataset_version_v1", &"v1"),
            P8QualityDigest::derive(
                "p8_zero_origin_tiny_dataset_license_v1",
                &"repo-owned-zero-origin-synthetic",
            ),
            questions,
        )
        .expect("tiny manifest");
        let admitted = admit_zero_origin_tiny_dataset_manifest(include_bytes!(
            "../../fixtures/p8-quality-tiny/manifest.json"
        ))
        .expect("admit checked-in tiny manifest");
        assert_eq!(admitted, manifest);

        let admitted_questions = admit_zero_origin_tiny_dataset_questions(
            include_bytes!("../../fixtures/p8-quality-tiny/questions.jsonl"),
            &admitted,
        )
        .expect("admit exact checked-in tiny question bytes");
        assert_eq!(admitted_questions.len(), admitted.ordered_questions().len());
        let tampered = include_str!("../../fixtures/p8-quality-tiny/questions.jsonl")
            .replacen("amber", "violet", 1);
        assert!(admit_zero_origin_tiny_dataset_questions(tampered.as_bytes(), &admitted).is_err());

        for line in include_str!("../../fixtures/p8-quality-tiny/questions.jsonl").lines() {
            let value: serde_json::Value = serde_json::from_str(line).expect("tiny question JSON");
            assert_eq!(
                value.get("schema").and_then(serde_json::Value::as_str),
                Some("beetle-memory.p8.zero-origin-tiny-question.v1")
            );
            assert_eq!(
                value.get("origin").and_then(serde_json::Value::as_str),
                Some("repo_owned_zero_origin_synthetic")
            );
            assert!(matches!(
                value
                    .get("reader_behavior")
                    .and_then(serde_json::Value::as_str),
                Some("deterministic" | "seeded_repeat")
            ));
        }
    }
}
