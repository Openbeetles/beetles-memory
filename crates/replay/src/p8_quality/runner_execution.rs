//! P8.5-C runner-owned execution receipts.
//!
//! This module records only parent-observed execution facts and predecessor bindings. It does not
//! aggregate scores, compute confidence intervals, read thresholds, or make release decisions.

use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use super::execution_plan::{
    P8MechanicalWorkItemV1, P8MechanicalWorkKindV1, P8QualityExecutionPlanV1,
    P8TinyReaderBehaviorV1, P8ZeroOriginTinyQuestionSourceV1,
};
use super::{
    P8AccuracyOutcomeV1, P8ActualCapabilityOutcomesV1, P8ArmReleaseRef,
    P8BenchmarkJoinExecutionReceiptRef, P8ClosedProcessReceiptRef, P8CompletedTrialReceiptRef,
    P8CompositionReceiptRef, P8JudgeExecutionReceiptRef, P8ModelRequestRef,
    P8ProviderSafeProjectionRef, P8QualityArmKind, P8QualityContractFailure, P8QualityDigest,
    P8QualityRunRef, P8QualityTrialKeyV1, P8ReaderExecutionReceiptRef, P8ReaderTrialKeyV1,
};

const COMPOSITION_SCHEMA: &str = "beetle-memory.p8.provider-request-composition-receipt.v1";
const CLOSED_PROCESS_SCHEMA: &str = "beetle-memory.p8.closed-model-process-receipt.v1";
const READER_RECEIPT_SCHEMA: &str = "beetle-memory.p8.reader-execution-receipt.v1";
const BENCHMARK_JOIN_SCHEMA: &str = "beetle-memory.p8.benchmark-join-execution-receipt.v1";
const JUDGE_RECEIPT_SCHEMA: &str = "beetle-memory.p8.judge-execution-receipt.v1";
const COMPLETED_MAIN_TRIAL_SCHEMA: &str = "beetle-memory.p8.completed-main-trial-receipt.v1";
const PAIRED_JUDGE_RECEIPT_SCHEMA: &str = "beetle-memory.p8.paired-judge-execution-receipt.v1";
const COMPLETED_ABLATION_TRIAL_SCHEMA: &str =
    "beetle-memory.p8.completed-same-closure-ablation-receipt.v1";
const COMPLETED_NEGATIVE_PROOF_SCHEMA: &str =
    "beetle-memory.p8.completed-negative-only-proof-receipt.v1";
const SEMANTIC_EXECUTION_SCHEMA: &str = "beetle-memory.p8.replay-semantic-execution-receipt.v2";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ReplaySemanticHardGateEvidenceV1 {
    pub(super) ineligible_owner_projection_count: u64,
    pub(super) non_current_material_projection_count: u64,
    pub(super) cross_subject_private_soul_leak_count: u64,
    pub(super) full_store_scan_second_platform_or_live_fallback_count: u64,
    pub(super) post_image_closure_covered: bool,
    pub(super) update_lineage_violation_count: u64,
    pub(super) unmet_premise_procedure_delivery_count: u64,
    pub(super) profile_budget_render_ceiling_breach_count: u64,
    pub(super) unexpected_runtime_or_integrity_failure_count: u64,
}

impl P8ReplaySemanticHardGateEvidenceV1 {
    fn from_live_sdk(execution: &bm_sdk::P8SemanticClosureExecutionV2) -> Self {
        let report = execution.off_run_report();
        let baseline = report.baseline();
        let failures = baseline.validate_contract();
        let bindings = report.baseline_candidate_bindings();
        let delivered_with_reason = |reason| {
            u64::try_from(
                bindings
                    .iter()
                    .filter(|binding| {
                        (binding.selected() || binding.rendered())
                            && binding.suppression_reasons().contains(&reason)
                    })
                    .count(),
            )
            .unwrap_or(u64::MAX)
        };
        let ineligible_owner_projection_count = u64::try_from(
            bindings
                .iter()
                .filter(|binding| {
                    (binding.selected() || binding.rendered())
                        && !binding.suppression_reasons().is_empty()
                })
                .count(),
        )
        .unwrap_or(u64::MAX);
        let non_current_material_projection_count = [
            bm_sdk::GovernedRecallEligibilityReason::Stale,
            bm_sdk::GovernedRecallEligibilityReason::Obsolete,
            bm_sdk::GovernedRecallEligibilityReason::Superseded,
            bm_sdk::GovernedRecallEligibilityReason::Invalidated,
            bm_sdk::GovernedRecallEligibilityReason::Forgotten,
        ]
        .into_iter()
        .map(&delivered_with_reason)
        .fold(0_u64, u64::saturating_add);
        let payload = baseline.payload();
        let exact_read = payload.manifest_verified()
            && payload.read_set_exact()
            && payload.session_open_count() == 1
            && payload.receipt_count() == 1;
        Self {
            ineligible_owner_projection_count,
            non_current_material_projection_count,
            cross_subject_private_soul_leak_count: delivered_with_reason(
                bm_sdk::GovernedRecallEligibilityReason::PrivacyBlocked,
            ),
            full_store_scan_second_platform_or_live_fallback_count: u64::from(!exact_read),
            post_image_closure_covered: execution.negative_only_proofs().iter().any(|proof| {
                proof.kind() == bm_sdk::P8SemanticNegativeOnlyProofKindV2::ForgettingSuppression
                    && proof.applicable()
                    && proof.provider_payload_count() == 0
            }),
            update_lineage_violation_count: u64::from(
                failures.contains(&bm_sdk::GovernedRecallIntegrityFailureV1::LineageIncomplete),
            ),
            unmet_premise_procedure_delivery_count: delivered_with_reason(
                bm_sdk::GovernedRecallEligibilityReason::PremiseBlocked,
            ),
            profile_budget_render_ceiling_breach_count: u64::from(
                failures.contains(&bm_sdk::GovernedRecallIntegrityFailureV1::BudgetExceeded),
            ),
            unexpected_runtime_or_integrity_failure_count: u64::try_from(failures.len())
                .unwrap_or(u64::MAX),
        }
    }
}

fn observed_model_request_bytes_digest(bytes: &[u8]) -> P8QualityDigest {
    P8QualityDigest::derive("p8_observed_model_request_bytes_v1", &bytes)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ReplaySemanticExecutionReceiptV2 {
    schema: String,
    sdk_closure_receipt_digest: P8QualityDigest,
    profile: bm_core::feature_gate::ProfileId,
    capability_catalog_identity_digest: P8QualityDigest,
    runtime_budget_report_id: super::P8RuntimeBudgetReportIdV1,
    authority_identity_digest: P8QualityDigest,
    store_snapshot_identity_digest: P8QualityDigest,
    materialization_count: u64,
    selected_projection_digest: P8ProviderSafeProjectionRef,
    selected_projection_receipt_digest: P8QualityDigest,
    memory_projection_rendered_chars: u64,
    pub(super) hard_gate_evidence: P8ReplaySemanticHardGateEvidenceV1,
    receipt_digest: super::P8SemanticExecutionReceiptV2Ref,
}

impl P8ReplaySemanticExecutionReceiptV2 {
    fn from_live_sdk(
        execution: &bm_sdk::P8SemanticClosureExecutionV2,
        projection: &bm_sdk::P8ProviderSafeProjectionReceiptV2,
    ) -> Result<Self, P8QualityContractFailure> {
        if !execution.validate_contract().is_empty()
            || projection.authority_ref() != execution.receipt().authority_ref()
            || projection.store_snapshot_receipt() != execution.receipt().store_snapshot_receipt()
            || execution.receipt().materialization_count() != 1
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        let mut value = Self {
            schema: SEMANTIC_EXECUTION_SCHEMA.into(),
            sdk_closure_receipt_digest: P8QualityDigest::derive(
                "p8_sdk_semantic_closure_receipt_v2",
                execution.receipt().receipt_digest(),
            ),
            profile: execution.receipt().profile(),
            capability_catalog_identity_digest: P8QualityDigest::derive(
                "p8_sdk_capability_catalog_identity_v1",
                &execution.receipt().capability_catalog_identity(),
            ),
            runtime_budget_report_id: super::P8RuntimeBudgetReportIdV1::parse(
                execution.receipt().budget_report_identity(),
            )?,
            authority_identity_digest: P8QualityDigest::derive(
                "p8_sdk_recall_authority_identity_v1",
                execution.receipt().authority_ref(),
            ),
            store_snapshot_identity_digest: P8QualityDigest::derive(
                "p8_sdk_store_snapshot_identity_v1",
                execution.receipt().store_snapshot_receipt(),
            ),
            materialization_count: execution.receipt().materialization_count(),
            selected_projection_digest: P8ProviderSafeProjectionRef::derive(
                projection.projection_digest(),
            ),
            selected_projection_receipt_digest: P8QualityDigest::derive(
                "p8_sdk_provider_safe_projection_receipt_v2",
                projection.receipt_digest(),
            ),
            memory_projection_rendered_chars: u64::try_from(projection.rendered_chars())
                .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?,
            hard_gate_evidence: P8ReplaySemanticHardGateEvidenceV1::from_live_sdk(execution),
            receipt_digest: super::P8SemanticExecutionReceiptV2Ref::derive(&()),
        };
        value.receipt_digest = value.derived_receipt();
        value.validate_contract().map(|()| value)
    }

    fn validate_contract(&self) -> Result<(), P8QualityContractFailure> {
        if self.schema != SEMANTIC_EXECUTION_SCHEMA || self.materialization_count != 1 {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        if self.receipt_digest != self.derived_receipt() {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    fn derived_receipt(&self) -> super::P8SemanticExecutionReceiptV2Ref {
        super::P8SemanticExecutionReceiptV2Ref::derive(&(
            &self.schema,
            &self.sdk_closure_receipt_digest,
            self.profile,
            &self.capability_catalog_identity_digest,
            &self.runtime_budget_report_id,
            &self.authority_identity_digest,
            &self.store_snapshot_identity_digest,
            self.materialization_count,
            &self.selected_projection_digest,
            &self.selected_projection_receipt_digest,
            self.memory_projection_rendered_chars,
            &self.hard_gate_evidence,
        ))
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn with_sdk_beetle_provider_request<R>(
    execution_plan: &P8QualityExecutionPlanV1,
    work_item: &P8MechanicalWorkItemV1,
    execution: &mut bm_sdk::P8SemanticClosureExecutionV2,
    counterfactual: Option<super::P8SameClosureSafeCounterfactualKindV1>,
    question_safe_input: &str,
    prompt_template: &str,
    tool_schema_digest: P8QualityDigest,
    consumer: impl FnOnce(
        &str,
        &P8ProviderRequestCompositionReceiptV1,
        &P8ReplaySemanticExecutionReceiptV2,
    ) -> Result<R, P8QualityContractFailure>,
) -> Result<(P8ReplaySemanticExecutionReceiptV2, R), P8QualityContractFailure> {
    let selected_projection = match counterfactual {
        None => execution.baseline_projection_receipt().clone(),
        Some(kind) => execution
            .safe_counterfactual_projection_receipts()
            .into_iter()
            .find(|receipt| receipt.counterfactual() == Some(sdk_counterfactual(kind)))
            .cloned()
            .ok_or(P8QualityContractFailure::ReceiptChainMismatch)?,
    };
    let semantic_receipt =
        P8ReplaySemanticExecutionReceiptV2::from_live_sdk(execution, &selected_projection)?;
    let baseline_projection_digest = P8ProviderSafeProjectionRef::derive(
        execution.baseline_projection_receipt().projection_digest(),
    );
    let input = match counterfactual {
        None => P8CompositionInputV1::BeetleProviderSafeProjection {
            semantic_execution_receipt_v2: semantic_receipt.receipt_digest.clone(),
            projection_digest: semantic_receipt.selected_projection_digest.clone(),
        },
        Some(_) => P8CompositionInputV1::SameClosureSafeCounterfactual {
            semantic_execution_receipt_v2: semantic_receipt.receipt_digest.clone(),
            baseline_projection_digest,
            off_run_projection_digest: semantic_receipt.selected_projection_digest.clone(),
        },
    };
    let rendered_chars = semantic_receipt.memory_projection_rendered_chars;
    let mut consumer = Some(consumer);
    let consume_result = match counterfactual {
        None => execution.consume_baseline_provider_safe_projection(|payload| {
            consume_provider_request(
                execution_plan,
                work_item,
                input,
                question_safe_input,
                prompt_template,
                tool_schema_digest,
                rendered_chars,
                payload,
                &semantic_receipt,
                consumer.take().expect("consumer is called exactly once"),
            )
        }),
        Some(kind) => execution.consume_same_closure_provider_safe_projection(
            sdk_counterfactual(kind),
            |payload| {
                consume_provider_request(
                    execution_plan,
                    work_item,
                    input,
                    question_safe_input,
                    prompt_template,
                    tool_schema_digest,
                    rendered_chars,
                    payload,
                    &semantic_receipt,
                    consumer.take().expect("consumer is called exactly once"),
                )
            },
        ),
    }
    .map_err(|_| P8QualityContractFailure::ReceiptChainMismatch)?;
    let result = consume_result?;
    Ok((semantic_receipt, result))
}

#[allow(clippy::too_many_arguments)]
fn consume_provider_request<R>(
    execution_plan: &P8QualityExecutionPlanV1,
    work_item: &P8MechanicalWorkItemV1,
    input: P8CompositionInputV1,
    question_safe_input: &str,
    prompt_template: &str,
    tool_schema_digest: P8QualityDigest,
    rendered_chars: u64,
    payload: &str,
    semantic_receipt: &P8ReplaySemanticExecutionReceiptV2,
    consumer: impl FnOnce(
        &str,
        &P8ProviderRequestCompositionReceiptV1,
        &P8ReplaySemanticExecutionReceiptV2,
    ) -> Result<R, P8QualityContractFailure>,
) -> Result<R, P8QualityContractFailure> {
    if u64::try_from(payload.chars().count()).ok() != Some(rendered_chars) {
        return Err(P8QualityContractFailure::ReceiptChainMismatch);
    }
    let mut request = serde_json::to_string(&(
        "beetle-memory.p8.fixture-provider-request.v1",
        prompt_template,
        question_safe_input,
        payload,
    ))
    .map_err(|_| P8QualityContractFailure::RawMaterialPresent)?;
    let composition = P8ProviderRequestCompositionReceiptV1::build_for_work_item(
        execution_plan,
        work_item,
        input,
        P8QualityDigest::derive(
            "p8_question_safe_input_bytes_v1",
            &question_safe_input.as_bytes(),
        ),
        P8QualityDigest::derive("p8_prompt_template_bytes_v1", &prompt_template.as_bytes()),
        tool_schema_digest,
        observed_model_request_bytes_digest(request.as_bytes()),
        u64::try_from(request.len()).map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?,
        rendered_chars,
        u64::try_from(request.chars().count())
            .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?,
    )?;
    let result = consumer(&request, &composition, semantic_receipt);
    // SAFETY: request is exclusively owned here; zero bytes remain valid UTF-8 before clear.
    for byte in unsafe { request.as_mut_vec() } {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    request.clear();
    result
}

fn sdk_counterfactual(
    kind: super::P8SameClosureSafeCounterfactualKindV1,
) -> bm_sdk::P8SameClosureSafeCounterfactualV1 {
    match kind {
        super::P8SameClosureSafeCounterfactualKindV1::TemporalValidity => {
            bm_sdk::P8SameClosureSafeCounterfactualV1::TemporalValidity
        }
        super::P8SameClosureSafeCounterfactualKindV1::UpdateLineage => {
            bm_sdk::P8SameClosureSafeCounterfactualV1::UpdateLineage
        }
        super::P8SameClosureSafeCounterfactualKindV1::ObsoleteSuppression => {
            bm_sdk::P8SameClosureSafeCounterfactualV1::ObsoleteSuppression
        }
        super::P8SameClosureSafeCounterfactualKindV1::ProceduralEvidence => {
            bm_sdk::P8SameClosureSafeCounterfactualV1::ProceduralEvidence
        }
        super::P8SameClosureSafeCounterfactualKindV1::DynamicState => {
            bm_sdk::P8SameClosureSafeCounterfactualV1::DynamicState
        }
    }
}

fn sdk_negative_proof_kind(
    kind: super::P8SafetyNegativeProofKindV1,
) -> bm_sdk::P8SemanticNegativeOnlyProofKindV2 {
    match kind {
        super::P8SafetyNegativeProofKindV1::Invalidated => {
            bm_sdk::P8SemanticNegativeOnlyProofKindV2::InvalidatedSuppression
        }
        super::P8SafetyNegativeProofKindV1::Forgetting => {
            bm_sdk::P8SemanticNegativeOnlyProofKindV2::ForgettingSuppression
        }
        super::P8SafetyNegativeProofKindV1::EnvironmentPremise => {
            bm_sdk::P8SemanticNegativeOnlyProofKindV2::EnvironmentPremise
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8CompletedNegativeOnlyProofReceiptV1 {
    schema: String,
    run_id: P8QualityRunRef,
    key: super::P8SafetyNegativeProofKeyV1,
    arm_release_digest: P8ArmReleaseRef,
    sdk_closure_receipt_digest: P8QualityDigest,
    authority_identity_digest: P8QualityDigest,
    store_snapshot_identity_digest: P8QualityDigest,
    sdk_off_run_digest: P8QualityDigest,
    sdk_negative_proof_digest: P8QualityDigest,
    applicable: bool,
    provider_payload_count: u64,
    model_boundary: super::P8NegativeProofModelBoundaryV1,
    receipt_digest: super::P8SafetyProofReceiptRef,
}

impl P8CompletedNegativeOnlyProofReceiptV1 {
    pub(crate) fn close_from_live_sdk(
        execution_plan: &P8QualityExecutionPlanV1,
        work_item: &P8MechanicalWorkItemV1,
        execution: &bm_sdk::P8SemanticClosureExecutionV2,
    ) -> Result<Self, P8QualityContractFailure> {
        let P8MechanicalWorkKindV1::SafetyNegativeProof { proof } = work_item.kind() else {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        };
        if !execution_plan.work_items().contains(work_item)
            || work_item.reader_repeat_index().is_some()
            || work_item.judge_repeat_index().is_some()
            || !execution.validate_contract().is_empty()
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        let sdk_proof = execution
            .negative_only_proofs()
            .iter()
            .find(|candidate| candidate.kind() == sdk_negative_proof_kind(proof))
            .ok_or(P8QualityContractFailure::CoverageMismatch)?;
        if sdk_proof.authority_ref() != execution.receipt().authority_ref()
            || sdk_proof.store_snapshot_receipt() != execution.receipt().store_snapshot_receipt()
            || sdk_proof.provider_payload_count() != 0
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        let mut value = Self {
            schema: COMPLETED_NEGATIVE_PROOF_SCHEMA.into(),
            run_id: execution_plan.run_id().clone(),
            key: super::P8SafetyNegativeProofKeyV1 {
                question_id: work_item.question_id().clone(),
                arm: work_item.arm(),
                proof,
            },
            arm_release_digest: work_item.arm_release_digest().clone(),
            sdk_closure_receipt_digest: P8QualityDigest::derive(
                "p8_sdk_semantic_closure_receipt_v2",
                execution.receipt().receipt_digest(),
            ),
            authority_identity_digest: P8QualityDigest::derive(
                "p8_sdk_recall_authority_identity_v1",
                execution.receipt().authority_ref(),
            ),
            store_snapshot_identity_digest: P8QualityDigest::derive(
                "p8_sdk_store_snapshot_identity_v1",
                execution.receipt().store_snapshot_receipt(),
            ),
            sdk_off_run_digest: P8QualityDigest::derive(
                "p8_sdk_negative_off_run_digest_v1",
                sdk_proof.off_run_digest(),
            ),
            sdk_negative_proof_digest: P8QualityDigest::derive(
                "p8_sdk_negative_only_proof_digest_v2",
                sdk_proof.proof_digest(),
            ),
            applicable: sdk_proof.applicable(),
            provider_payload_count: sdk_proof.provider_payload_count(),
            model_boundary: super::P8NegativeProofModelBoundaryV1::NoReaderModelJudgeOrAccuracy,
            receipt_digest: super::P8SafetyProofReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_receipt();
        value
            .validate_against_work_item(execution_plan, work_item)
            .map(|()| value)
    }

    pub(crate) fn validate_against_work_item(
        &self,
        execution_plan: &P8QualityExecutionPlanV1,
        work_item: &P8MechanicalWorkItemV1,
    ) -> Result<(), P8QualityContractFailure> {
        let P8MechanicalWorkKindV1::SafetyNegativeProof { proof } = work_item.kind() else {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        };
        let expected_key = super::P8SafetyNegativeProofKeyV1 {
            question_id: work_item.question_id().clone(),
            arm: work_item.arm(),
            proof,
        };
        if self.schema != COMPLETED_NEGATIVE_PROOF_SCHEMA
            || !execution_plan.work_items().contains(work_item)
            || &self.run_id != execution_plan.run_id()
            || self.key != expected_key
            || &self.arm_release_digest != work_item.arm_release_digest()
            || self.provider_payload_count != 0
            || self.model_boundary
                != super::P8NegativeProofModelBoundaryV1::NoReaderModelJudgeOrAccuracy
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        if self.receipt_digest != self.derived_receipt() {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    fn derived_receipt(&self) -> super::P8SafetyProofReceiptRef {
        super::P8SafetyProofReceiptRef::derive(&(
            &self.schema,
            &self.run_id,
            &self.key,
            &self.arm_release_digest,
            &self.sdk_closure_receipt_digest,
            &self.authority_identity_digest,
            &self.store_snapshot_identity_digest,
            &self.sdk_off_run_digest,
            &self.sdk_negative_proof_digest,
            self.applicable,
            self.provider_payload_count,
            self.model_boundary,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ModelProcessRoleV1 {
    Reader,
    Judge,
    PairedJudge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum P8CompositionInputV1 {
    NoMemoryEmpty,
    PublicReferenceSafeOutput {
        safe_output_digest: P8QualityDigest,
    },
    BeetleProviderSafeProjection {
        semantic_execution_receipt_v2: super::P8SemanticExecutionReceiptV2Ref,
        projection_digest: P8ProviderSafeProjectionRef,
    },
    SameClosureSafeCounterfactual {
        semantic_execution_receipt_v2: super::P8SemanticExecutionReceiptV2Ref,
        baseline_projection_digest: P8ProviderSafeProjectionRef,
        off_run_projection_digest: P8ProviderSafeProjectionRef,
    },
}

impl P8CompositionInputV1 {
    fn matches_arm(&self, arm: P8QualityArmKind) -> bool {
        matches!(
            (self, arm),
            (Self::NoMemoryEmpty, P8QualityArmKind::NoMemory)
                | (
                    Self::PublicReferenceSafeOutput { .. },
                    P8QualityArmKind::PublicReference
                )
                | (
                    Self::BeetleProviderSafeProjection { .. }
                        | Self::SameClosureSafeCounterfactual { .. },
                    P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate
                )
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ProviderRequestCompositionReceiptV1 {
    schema: String,
    run_id: P8QualityRunRef,
    reader_key: P8ReaderTrialKeyV1,
    arm_release_digest: P8ArmReleaseRef,
    input: P8CompositionInputV1,
    question_safe_input_digest: P8QualityDigest,
    prompt_template_digest: P8QualityDigest,
    tool_schema_digest: P8QualityDigest,
    final_request_bytes_digest: P8QualityDigest,
    final_request_byte_count: u64,
    memory_projection_rendered_chars: u64,
    final_request_chars: u64,
    request_digest: P8ModelRequestRef,
    receipt_digest: P8CompositionReceiptRef,
}

impl P8ProviderRequestCompositionReceiptV1 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_for_work_item(
        execution_plan: &P8QualityExecutionPlanV1,
        work_item: &P8MechanicalWorkItemV1,
        input: P8CompositionInputV1,
        question_safe_input_digest: P8QualityDigest,
        prompt_template_digest: P8QualityDigest,
        tool_schema_digest: P8QualityDigest,
        final_request_bytes_digest: P8QualityDigest,
        final_request_byte_count: u64,
        memory_projection_rendered_chars: u64,
        final_request_chars: u64,
    ) -> Result<Self, P8QualityContractFailure> {
        if !execution_plan.work_items().contains(work_item) {
            return Err(P8QualityContractFailure::CoverageMismatch);
        }
        match (work_item.kind(), &input) {
            (
                P8MechanicalWorkKindV1::Main,
                P8CompositionInputV1::NoMemoryEmpty
                | P8CompositionInputV1::PublicReferenceSafeOutput { .. }
                | P8CompositionInputV1::BeetleProviderSafeProjection { .. },
            )
            | (
                P8MechanicalWorkKindV1::SameClosureAblation { .. },
                P8CompositionInputV1::SameClosureSafeCounterfactual { .. },
            ) => {}
            (
                P8MechanicalWorkKindV1::Main
                | P8MechanicalWorkKindV1::SameClosureAblation { .. }
                | P8MechanicalWorkKindV1::SafetyNegativeProof { .. },
                _,
            ) => return Err(P8QualityContractFailure::ReceiptChainMismatch),
        }
        let reader_repeat_index = work_item
            .reader_repeat_index()
            .ok_or(P8QualityContractFailure::CoverageMismatch)?;
        Self::build_bound(
            execution_plan.run_id().clone(),
            P8ReaderTrialKeyV1 {
                question_id: work_item.question_id().clone(),
                arm: work_item.arm(),
                reader_repeat_index,
            },
            work_item.arm_release_digest().clone(),
            input,
            question_safe_input_digest,
            prompt_template_digest,
            tool_schema_digest,
            final_request_bytes_digest,
            final_request_byte_count,
            memory_projection_rendered_chars,
            final_request_chars,
        )
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn build_fixture(
        run_id: P8QualityRunRef,
        reader_key: P8ReaderTrialKeyV1,
        arm_release_digest: P8ArmReleaseRef,
        input: P8CompositionInputV1,
        question_safe_input_digest: P8QualityDigest,
        prompt_template_digest: P8QualityDigest,
        tool_schema_digest: P8QualityDigest,
        final_request_bytes_digest: P8QualityDigest,
        final_request_byte_count: u64,
        memory_projection_rendered_chars: u64,
        final_request_chars: u64,
    ) -> Result<Self, P8QualityContractFailure> {
        Self::build_bound(
            run_id,
            reader_key,
            arm_release_digest,
            input,
            question_safe_input_digest,
            prompt_template_digest,
            tool_schema_digest,
            final_request_bytes_digest,
            final_request_byte_count,
            memory_projection_rendered_chars,
            final_request_chars,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_bound(
        run_id: P8QualityRunRef,
        reader_key: P8ReaderTrialKeyV1,
        arm_release_digest: P8ArmReleaseRef,
        input: P8CompositionInputV1,
        question_safe_input_digest: P8QualityDigest,
        prompt_template_digest: P8QualityDigest,
        tool_schema_digest: P8QualityDigest,
        final_request_bytes_digest: P8QualityDigest,
        final_request_byte_count: u64,
        memory_projection_rendered_chars: u64,
        final_request_chars: u64,
    ) -> Result<Self, P8QualityContractFailure> {
        let request_digest = P8ModelRequestRef::derive(&(
            &run_id,
            &reader_key,
            &arm_release_digest,
            &input,
            &question_safe_input_digest,
            &prompt_template_digest,
            &tool_schema_digest,
            &final_request_bytes_digest,
            final_request_byte_count,
            memory_projection_rendered_chars,
            final_request_chars,
        ));
        let mut value = Self {
            schema: COMPOSITION_SCHEMA.into(),
            run_id,
            reader_key,
            arm_release_digest,
            input,
            question_safe_input_digest,
            prompt_template_digest,
            tool_schema_digest,
            final_request_bytes_digest,
            final_request_byte_count,
            memory_projection_rendered_chars,
            final_request_chars,
            request_digest,
            receipt_digest: P8CompositionReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_receipt();
        value.validate_contract().map(|()| value)
    }

    pub(crate) fn validate_contract(&self) -> Result<(), P8QualityContractFailure> {
        if self.schema != COMPOSITION_SCHEMA {
            return Err(P8QualityContractFailure::SchemaMismatch);
        }
        if !self.input.matches_arm(self.reader_key.arm)
            || matches!(self.input, P8CompositionInputV1::NoMemoryEmpty)
                && self.memory_projection_rendered_chars != 0
            || self.final_request_byte_count == 0
            || self.final_request_chars < self.memory_projection_rendered_chars
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        let expected_request = P8ModelRequestRef::derive(&(
            &self.run_id,
            &self.reader_key,
            &self.arm_release_digest,
            &self.input,
            &self.question_safe_input_digest,
            &self.prompt_template_digest,
            &self.tool_schema_digest,
            &self.final_request_bytes_digest,
            self.final_request_byte_count,
            self.memory_projection_rendered_chars,
            self.final_request_chars,
        ));
        if self.request_digest != expected_request || self.receipt_digest != self.derived_receipt()
        {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    fn derived_receipt(&self) -> P8CompositionReceiptRef {
        P8CompositionReceiptRef::derive(&(
            &self.schema,
            &self.run_id,
            &self.reader_key,
            &self.arm_release_digest,
            &self.input,
            &self.question_safe_input_digest,
            &self.prompt_template_digest,
            &self.tool_schema_digest,
            &self.final_request_bytes_digest,
            self.final_request_byte_count,
            self.memory_projection_rendered_chars,
            self.final_request_chars,
            &self.request_digest,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ObservedModelAttemptV1 {
    attempt_index: u32,
    request_digest: P8ModelRequestRef,
    request_bytes_digest: P8QualityDigest,
    request_byte_count: u64,
    response_bytes_digest: P8QualityDigest,
    response_byte_count: u64,
    stderr_bytes_digest: P8QualityDigest,
    stderr_byte_count: u64,
    exit_code: i32,
    stdout_eof: bool,
    stderr_eof: bool,
    elapsed_nanoseconds: u64,
}

impl P8ObservedModelAttemptV1 {
    #[cfg(test)]
    fn fixture(
        attempt_index: u32,
        request_digest: P8ModelRequestRef,
        request_bytes_digest: P8QualityDigest,
        request_byte_count: u64,
        response_label: &str,
        exit_code: i32,
    ) -> Self {
        Self {
            attempt_index,
            request_digest,
            request_bytes_digest,
            request_byte_count,
            response_bytes_digest: P8QualityDigest::derive(
                "p8_fixture_observed_response_bytes_v1",
                &response_label,
            ),
            response_byte_count: u64::try_from(response_label.len()).expect("fixture response"),
            stderr_bytes_digest: P8QualityDigest::derive("p8_model_stderr_bytes_v1", &b""),
            stderr_byte_count: 0,
            exit_code,
            stdout_eof: true,
            stderr_eof: true,
            elapsed_nanoseconds: 1,
        }
    }

    fn observed(
        attempt_index: u32,
        request_digest: P8ModelRequestRef,
        request_bytes: &[u8],
        response_bytes: &[u8],
        stderr_bytes: &[u8],
        exit_code: i32,
        elapsed_nanoseconds: u64,
    ) -> Result<Self, P8QualityContractFailure> {
        Ok(Self {
            attempt_index,
            request_digest,
            request_bytes_digest: observed_model_request_bytes_digest(request_bytes),
            request_byte_count: u64::try_from(request_bytes.len())
                .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?,
            response_bytes_digest: P8QualityDigest::derive(
                "p8_model_response_observed_bytes_v1",
                &response_bytes,
            ),
            response_byte_count: u64::try_from(response_bytes.len())
                .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?,
            stderr_bytes_digest: P8QualityDigest::derive("p8_model_stderr_bytes_v1", &stderr_bytes),
            stderr_byte_count: u64::try_from(stderr_bytes.len())
                .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?,
            exit_code,
            stdout_eof: true,
            stderr_eof: true,
            elapsed_nanoseconds: elapsed_nanoseconds.max(1),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ClosedModelProcessReceiptV1 {
    schema: String,
    role: P8ModelProcessRoleV1,
    executable_digest: P8QualityDigest,
    provider_identity_digest: P8QualityDigest,
    model_revision_digest: P8QualityDigest,
    attempts: Vec<P8ObservedModelAttemptV1>,
    final_response_bytes_digest: P8QualityDigest,
    receipt_digest: P8ClosedProcessReceiptRef,
}

impl P8ClosedModelProcessReceiptV1 {
    pub(crate) fn close(
        role: P8ModelProcessRoleV1,
        executable_digest: P8QualityDigest,
        provider_identity_digest: P8QualityDigest,
        model_revision_digest: P8QualityDigest,
        attempts: Vec<P8ObservedModelAttemptV1>,
    ) -> Result<Self, P8QualityContractFailure> {
        let final_response_bytes_digest = attempts
            .last()
            .ok_or(P8QualityContractFailure::CoverageMismatch)?
            .response_bytes_digest
            .clone();
        let mut value = Self {
            schema: CLOSED_PROCESS_SCHEMA.into(),
            role,
            executable_digest,
            provider_identity_digest,
            model_revision_digest,
            attempts,
            final_response_bytes_digest,
            receipt_digest: P8ClosedProcessReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_receipt();
        value.validate_contract().map(|()| value)
    }

    pub(crate) fn validate_contract(&self) -> Result<(), P8QualityContractFailure> {
        if self.schema != CLOSED_PROCESS_SCHEMA {
            return Err(P8QualityContractFailure::SchemaMismatch);
        }
        let Some(last) = self.attempts.last() else {
            return Err(P8QualityContractFailure::CoverageMismatch);
        };
        let request_digest = &self.attempts[0].request_digest;
        let ordered = self.attempts.iter().enumerate().all(|(index, attempt)| {
            attempt.attempt_index == u32::try_from(index).unwrap_or(u32::MAX)
                && &attempt.request_digest == request_digest
                && attempt.request_byte_count > 0
                && attempt.response_byte_count > 0
                && attempt.stdout_eof
                && attempt.stderr_eof
                && attempt.elapsed_nanoseconds > 0
                && if index + 1 == self.attempts.len() {
                    attempt.exit_code == 0
                } else {
                    attempt.exit_code != 0
                }
        });
        if !ordered || self.final_response_bytes_digest != last.response_bytes_digest {
            return Err(P8QualityContractFailure::PipeClosureMissing);
        }
        if self.receipt_digest != self.derived_receipt() {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    fn request_digest(&self) -> &P8ModelRequestRef {
        &self.attempts[0].request_digest
    }

    fn derived_receipt(&self) -> P8ClosedProcessReceiptRef {
        P8ClosedProcessReceiptRef::derive(&(
            &self.schema,
            self.role,
            &self.executable_digest,
            &self.provider_identity_digest,
            &self.model_revision_digest,
            &self.attempts,
            &self.final_response_bytes_digest,
        ))
    }
}

pub(super) fn execute_fixture_model_process(
    executable: &Path,
    args: &[&str],
    role: P8ModelProcessRoleV1,
    request_digest: P8ModelRequestRef,
    request_bytes: &[u8],
) -> std::io::Result<(P8ClosedModelProcessReceiptV1, Vec<u8>)> {
    let executable_bytes = std::fs::read(executable)?;
    let started = Instant::now();
    let mut child = Command::new(executable)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("fixture model stdin was not piped"))?
        .write_all(request_bytes)?;
    let output = child.wait_with_output()?;
    let elapsed_nanoseconds = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let attempt = P8ObservedModelAttemptV1::observed(
        0,
        request_digest,
        request_bytes,
        &output.stdout,
        &output.stderr,
        output.status.code().unwrap_or(-1),
        elapsed_nanoseconds,
    )
    .map_err(|failure| std::io::Error::other(format!("{failure:?}")))?;
    let receipt = P8ClosedModelProcessReceiptV1::close(
        role,
        P8QualityDigest::derive("p8_fixture_model_executable_bytes_v1", &executable_bytes),
        P8QualityDigest::derive(
            "p8_fixture_model_provider_identity_v1",
            &"repo-owned-fixture",
        ),
        P8QualityDigest::derive("p8_fixture_model_revision_v1", &(args, &executable_bytes)),
        vec![attempt],
    )
    .map_err(|failure| std::io::Error::other(format!("{failure:?}")))?;
    Ok((receipt, output.stdout))
}

pub(super) struct P8FixtureModelBinary {
    root: std::path::PathBuf,
    pub(super) executable: std::path::PathBuf,
}

impl P8FixtureModelBinary {
    pub(super) fn compile() -> std::io::Result<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "beetle-p8-quality-fixture-model-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root)?;
        let executable = root.join(if cfg!(windows) {
            "p8-quality-fixture-model.exe"
        } else {
            "p8-quality-fixture-model"
        });
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/p8-quality-tiny/model.rs");
        let rustc = std::path::Path::new(env!("BM_P8_FIXTURE_RUSTC"));
        if !rustc.is_absolute() || !rustc.is_file() {
            let _ = std::fs::remove_dir_all(&root);
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "build-bound fixture rustc is unavailable",
            ));
        }
        let output = std::process::Command::new(rustc)
            .arg("--edition=2021")
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()?;
        if !output.status.success() {
            let _ = std::fs::remove_dir_all(&root);
            return Err(std::io::Error::other(format!(
                "fixture model compile failed (status={})",
                output.status.code().unwrap_or(-1)
            )));
        }
        Ok(Self { root, executable })
    }
}

impl Drop for P8FixtureModelBinary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) struct P8RealFixtureReceiptSet {
    pub(super) main: Vec<P8CompletedMainTrialReceiptV1>,
    pub(super) ablation: Vec<P8CompletedAblationTrialReceiptV1>,
    pub(super) negative: Vec<P8CompletedNegativeOnlyProofReceiptV1>,
}

const FIXTURE_SHARD_MANIFEST_SCHEMA: &str = "beetle-memory.p8.fixture-runner-shard-manifest.v1";
const FIXTURE_COHORT_MANIFEST_SCHEMA: &str = "beetle-memory.p8.fixture-runner-cohort-manifest.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8FixtureArtifactFileEvidenceV1 {
    pub(super) file_name: String,
    pub(super) content_digest: P8QualityDigest,
    pub(super) record_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8FixtureShardManifestV1 {
    schema: String,
    pub(super) run_id: P8QualityRunRef,
    pub(super) shard_index: u32,
    pub(super) receipt_files: Vec<P8FixtureArtifactFileEvidenceV1>,
    pub(super) manifest_digest: P8QualityDigest,
}

impl P8FixtureShardManifestV1 {
    fn build(
        run_id: P8QualityRunRef,
        shard_index: u32,
        receipt_files: Vec<P8FixtureArtifactFileEvidenceV1>,
    ) -> Self {
        let mut value = Self {
            schema: FIXTURE_SHARD_MANIFEST_SCHEMA.into(),
            run_id,
            shard_index,
            receipt_files,
            manifest_digest: P8QualityDigest::derive("p8_fixture_shard_manifest_v1", &()),
        };
        value.manifest_digest = value.derived_digest();
        value
    }

    pub(super) fn validate_contract(&self) -> bool {
        self.schema == FIXTURE_SHARD_MANIFEST_SCHEMA
            && self.receipt_files.len() == 3
            && self
                .receipt_files
                .windows(2)
                .all(|pair| pair[0].file_name < pair[1].file_name)
            && self.receipt_files.iter().all(|file| file.record_count > 0)
            && self.manifest_digest == self.derived_digest()
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_fixture_shard_manifest_v1",
            &(
                &self.schema,
                &self.run_id,
                self.shard_index,
                &self.receipt_files,
            ),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8FixtureCohortShardRefV1 {
    pub(super) shard_index: u32,
    pub(super) manifest_file_name: String,
    pub(super) manifest_digest: P8QualityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct P8FixtureCohortManifestV1 {
    schema: String,
    pub(super) run_id: P8QualityRunRef,
    pub(super) shards: Vec<P8FixtureCohortShardRefV1>,
    pub(super) main_receipt_count: u64,
    pub(super) ablation_receipt_count: u64,
    pub(super) negative_receipt_count: u64,
    pub(super) manifest_digest: P8QualityDigest,
}

impl P8FixtureCohortManifestV1 {
    fn build(
        run_id: P8QualityRunRef,
        shards: Vec<P8FixtureCohortShardRefV1>,
        main_receipt_count: u64,
        ablation_receipt_count: u64,
        negative_receipt_count: u64,
    ) -> Self {
        let mut value = Self {
            schema: FIXTURE_COHORT_MANIFEST_SCHEMA.into(),
            run_id,
            shards,
            main_receipt_count,
            ablation_receipt_count,
            negative_receipt_count,
            manifest_digest: P8QualityDigest::derive("p8_fixture_cohort_manifest_v1", &()),
        };
        value.manifest_digest = value.derived_digest();
        value
    }

    pub(super) fn validate_contract(&self) -> bool {
        self.schema == FIXTURE_COHORT_MANIFEST_SCHEMA
            && !self.shards.is_empty()
            && self
                .shards
                .iter()
                .enumerate()
                .all(|(index, shard)| shard.shard_index == u32::try_from(index).unwrap_or(u32::MAX))
            && self.main_receipt_count > 0
            && self.ablation_receipt_count > 0
            && self.negative_receipt_count > 0
            && self.manifest_digest == self.derived_digest()
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_fixture_cohort_manifest_v1",
            &(
                &self.schema,
                &self.run_id,
                &self.shards,
                self.main_receipt_count,
                self.ablation_receipt_count,
                self.negative_receipt_count,
            ),
        )
    }
}

pub(super) fn execute_real_fixture_receipt_set(
    execution_plan: &P8QualityExecutionPlanV1,
    dataset: &super::execution_plan::P8ZeroOriginTinyDatasetManifestV1,
    source_questions: &[P8ZeroOriginTinyQuestionSourceV1],
) -> Result<P8RealFixtureReceiptSet, P8QualityContractFailure> {
    fn process_failure(_: std::io::Error) -> P8QualityContractFailure {
        P8QualityContractFailure::ReceiptChainMismatch
    }

    fn reader_args(
        question: &P8ZeroOriginTinyQuestionSourceV1,
        reader_repeat_index: u32,
    ) -> [String; 3] {
        match question.reader_behavior() {
            P8TinyReaderBehaviorV1::Deterministic => [
                "reader".into(),
                "deterministic".into(),
                question.gold().into(),
            ],
            P8TinyReaderBehaviorV1::SeededRepeat => [
                "reader".into(),
                "seeded_repeat".into(),
                reader_repeat_index.to_string(),
            ],
        }
    }

    fn judge_args(question: &P8ZeroOriginTinyQuestionSourceV1) -> [String; 3] {
        match question.reader_behavior() {
            P8TinyReaderBehaviorV1::Deterministic => {
                ["judge".into(), "exact".into(), question.gold().into()]
            }
            P8TinyReaderBehaviorV1::SeededRepeat => [
                "judge".into(),
                "repeat_index".into(),
                question.gold().into(),
            ],
        }
    }

    fn execute_judge(
        model: &P8FixtureModelBinary,
        question: &P8ZeroOriginTinyQuestionSourceV1,
        work_item: &P8MechanicalWorkItemV1,
        join: &P8RealBenchmarkJoinExecutionReceiptV1,
        response: &[u8],
    ) -> Result<P8JudgeExecutionReceiptV1, P8QualityContractFailure> {
        let args = judge_args(question);
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        let (process, output) = execute_fixture_model_process(
            &model.executable,
            &arg_refs,
            P8ModelProcessRoleV1::Judge,
            join.judge_input_digest.clone(),
            response,
        )
        .map_err(process_failure)?;
        if output != b"correct" {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        P8JudgeExecutionReceiptV1::close_after_join(
            P8QualityTrialKeyV1 {
                question_id: work_item.question_id().clone(),
                arm: work_item.arm(),
                reader_repeat_index: work_item
                    .reader_repeat_index()
                    .ok_or(P8QualityContractFailure::CoverageMismatch)?,
                judge_repeat_index: work_item
                    .judge_repeat_index()
                    .ok_or(P8QualityContractFailure::CoverageMismatch)?,
            },
            join,
            process,
        )
    }

    let model = P8FixtureModelBinary::compile().map_err(process_failure)?;
    let mut main_by_ordinal = BTreeMap::new();
    let mut ablation_by_ordinal = BTreeMap::new();
    let mut negative_by_ordinal = BTreeMap::new();
    let arms = P8QualityArmKind::expected_for(execution_plan.purpose());

    for question in dataset.ordered_questions() {
        let source_question = source_questions
            .iter()
            .find(|candidate| candidate.question_id() == question.question_id())
            .ok_or(P8QualityContractFailure::CoverageMismatch)?;
        for arm in arms {
            for reader_repeat_index in 0..2_u32 {
                let main_items = execution_plan
                    .work_items()
                    .iter()
                    .filter(|item| {
                        item.kind() == P8MechanicalWorkKindV1::Main
                            && item.question_id() == question.question_id()
                            && item.arm() == *arm
                            && item.reader_repeat_index() == Some(reader_repeat_index)
                    })
                    .collect::<Vec<_>>();
                let first_main = *main_items
                    .first()
                    .ok_or(P8QualityContractFailure::CoverageMismatch)?;
                let args = reader_args(source_question, reader_repeat_index);
                let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();

                let mut baseline_semantic = None;
                let mut off_runs = Vec::new();
                let (baseline_reader, baseline_join, baseline_response) = if matches!(
                    arm,
                    P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate
                ) {
                    let profile = bm_sdk::ProfileId::native_dev_full()
                        .ok_or(P8QualityContractFailure::IdentityInvalid)?;
                    let store = bm_sdk::MemoryStoreHandle::open(
                        bm_sdk::StoreBackendConfig::in_memory(profile)
                            .map_err(|_| P8QualityContractFailure::IdentityInvalid)?,
                    )
                    .map_err(|_| P8QualityContractFailure::IdentityInvalid)?;
                    let runtime = bm_sdk::MemoryRuntime::builder()
                        .identity(
                            bm_sdk::MemoryIdentity::new(
                                "p8-real-fixture",
                                format!(
                                    "{}-{arm:?}-{reader_repeat_index}",
                                    question.question_id().as_str()
                                ),
                            )
                            .map_err(|_| P8QualityContractFailure::IdentityInvalid)?,
                        )
                        .scope(
                            bm_sdk::MemoryScope::new(
                                "p8-real-fixture",
                                question.question_id().as_str(),
                            )
                            .map_err(|_| P8QualityContractFailure::IdentityInvalid)?,
                        )
                        .store(store)
                        .build()
                        .map_err(|_| P8QualityContractFailure::IdentityInvalid)?;
                    runtime
                        .write(bm_sdk::MemoryWriteRequest::LongTermExtraction {
                            extraction: bm_sdk::ParsedLongTermMemoryExtraction {
                                upserts: vec![bm_sdk::LongTermMemoryDraft {
                                    kind: bm_sdk::LongTermMemoryKind::Project,
                                    topic: "p8 fixture post image".into(),
                                    content: "fixture post-image closure probe".into(),
                                    keywords: vec!["fixture".into(), "post-image".into()],
                                    privacy: bm_sdk::MemoryPrivacyClass::PublicRuntime,
                                    source_chat_id: None,
                                    source_type: None,
                                    source_scope: None,
                                    subject_visibility:
                                        bm_sdk::MemorySubjectVisibilityPolicy::AllSubjects,
                                    provenance: bm_sdk::LongTermMemoryProvenance {
                                        source_authority:
                                            bm_sdk::MemoryEvidenceAuthority::ProgramMemoryCanonical,
                                        semantic_judgment_source: None,
                                    },
                                    confidence: None,
                                    freshness: None,
                                    stale_hint: None,
                                    supporting_citations: vec![
                                        "repo-owned synthetic fixture".into()
                                    ],
                                    canonical_entities: Vec::new(),
                                    evidence_count: Some(1),
                                    observed_at: Some(1_800_000_000),
                                    source_revision: Some(1),
                                }],
                                deletes: Vec::new(),
                                skill_writes: Vec::new(),
                            },
                            governed_skill_writes: Vec::new(),
                            runtime_skill_owning_scope: None,
                        })
                        .map_err(|_| P8QualityContractFailure::ReceiptChainMismatch)?;
                    let forgetting_authority = runtime
                        .p8_prepare_forgetting_pre_operation(bm_sdk::MemoryLongTermSelector {
                            query: bm_sdk::LongTermMemoryQuery {
                                topic: Some("p8 fixture post image".into()),
                                limit: 4,
                                ..bm_sdk::LongTermMemoryQuery::default()
                            },
                            evidence_ref: None,
                        })
                        .map_err(|_| P8QualityContractFailure::ReceiptChainMismatch)?;
                    let mut sdk_execution = runtime
                        .p8_semantic_closure_execution_v2(
                            bm_sdk::P8SemanticOffRunRequest::with_forgetting_authority(
                                bm_sdk::MemoryRecallRequest {
                                    query: source_question.question().into(),
                                    limit: 4,
                                    structured_query_facets: Vec::new(),
                                    tool_registry_refs: Vec::new(),
                                    temporal_operation:
                                        bm_sdk::MemoryRecallTemporalOperation::Current,
                                },
                                forgetting_authority,
                            ),
                        )
                        .map_err(|_| P8QualityContractFailure::ReceiptChainMismatch)?;
                    if reader_repeat_index == 0 {
                        for negative_work in execution_plan.work_items().iter().filter(|item| {
                            matches!(
                                item.kind(),
                                P8MechanicalWorkKindV1::SafetyNegativeProof { .. }
                            ) && item.question_id() == question.question_id()
                                && item.arm() == *arm
                        }) {
                            negative_by_ordinal.insert(
                                negative_work.schedule_ordinal(),
                                P8CompletedNegativeOnlyProofReceiptV1::close_from_live_sdk(
                                    execution_plan,
                                    negative_work,
                                    &sdk_execution,
                                )?,
                            );
                        }
                    }
                    let (semantic, (composition, process, response)) =
                        with_sdk_beetle_provider_request(
                            execution_plan,
                            first_main,
                            &mut sdk_execution,
                            None,
                            source_question.question(),
                            source_question.rubric(),
                            P8QualityDigest::derive("p8_real_fixture_tool_schema_v1", &"none"),
                            |request, composition, _| {
                                let (process, response) = execute_fixture_model_process(
                                    &model.executable,
                                    &arg_refs,
                                    P8ModelProcessRoleV1::Reader,
                                    composition.request_digest.clone(),
                                    request.as_bytes(),
                                )
                                .map_err(process_failure)?;
                                Ok((composition.clone(), process, response))
                            },
                        )?;
                    let reader = P8ReaderExecutionReceiptV1::close(composition, process)?;
                    let join = P8RealBenchmarkJoinExecutionReceiptV1::after_reader(
                        &reader,
                        question.question_input_digest().clone(),
                        P8QualityDigest::derive(
                            "p8_real_fixture_rubric_gold_join_v1",
                            &(question.rubric_digest(), question.gold_digest()),
                        ),
                        &response,
                    )?;
                    baseline_semantic = Some(semantic);

                    for counterfactual in super::P8SameClosureSafeCounterfactualKindV1::ALL {
                        let first_off = execution_plan
                            .work_items()
                            .iter()
                            .find(|item| {
                                item.question_id() == question.question_id()
                                    && item.arm() == *arm
                                    && item.reader_repeat_index() == Some(reader_repeat_index)
                                    && item.judge_repeat_index() == Some(0)
                                    && item.kind()
                                        == P8MechanicalWorkKindV1::SameClosureAblation {
                                            counterfactual,
                                        }
                            })
                            .ok_or(P8QualityContractFailure::CoverageMismatch)?;
                        let (off_semantic, (composition, process, off_response)) =
                            with_sdk_beetle_provider_request(
                                execution_plan,
                                first_off,
                                &mut sdk_execution,
                                Some(counterfactual),
                                source_question.question(),
                                source_question.rubric(),
                                P8QualityDigest::derive("p8_real_fixture_tool_schema_v1", &"none"),
                                |request, composition, _| {
                                    let (process, response) = execute_fixture_model_process(
                                        &model.executable,
                                        &arg_refs,
                                        P8ModelProcessRoleV1::Reader,
                                        composition.request_digest.clone(),
                                        request.as_bytes(),
                                    )
                                    .map_err(process_failure)?;
                                    Ok((composition.clone(), process, response))
                                },
                            )?;
                        let off_reader = P8ReaderExecutionReceiptV1::close(composition, process)?;
                        let off_join = P8RealBenchmarkJoinExecutionReceiptV1::after_reader(
                            &off_reader,
                            question.question_input_digest().clone(),
                            P8QualityDigest::derive(
                                "p8_real_fixture_rubric_gold_join_v1",
                                &(question.rubric_digest(), question.gold_digest()),
                            ),
                            &off_response,
                        )?;
                        off_runs.push((
                            counterfactual,
                            off_semantic,
                            off_reader,
                            off_join,
                            off_response,
                        ));
                    }
                    (reader, join, response)
                } else {
                    let request = serde_json::to_vec(&(
                        "beetle-memory.p8.zero-origin-direct-reader.v1",
                        source_question.question(),
                        arm,
                        reader_repeat_index,
                    ))
                    .map_err(|_| P8QualityContractFailure::RawMaterialPresent)?;
                    let input = match arm {
                        P8QualityArmKind::NoMemory => P8CompositionInputV1::NoMemoryEmpty,
                        P8QualityArmKind::PublicReference => {
                            P8CompositionInputV1::PublicReferenceSafeOutput {
                                safe_output_digest: P8QualityDigest::derive(
                                    "p8_real_fixture_public_reference_v1",
                                    question.question_id(),
                                ),
                            }
                        }
                        _ => unreachable!(),
                    };
                    let composition = P8ProviderRequestCompositionReceiptV1::build_for_work_item(
                        execution_plan,
                        first_main,
                        input,
                        question.question_input_digest().clone(),
                        P8QualityDigest::derive(
                            "p8_real_fixture_prompt_v1",
                            &source_question.rubric(),
                        ),
                        P8QualityDigest::derive("p8_real_fixture_tool_schema_v1", &"none"),
                        observed_model_request_bytes_digest(&request),
                        u64::try_from(request.len())
                            .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?,
                        0,
                        u64::try_from(request.len())
                            .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?,
                    )?;
                    let (process, response) = execute_fixture_model_process(
                        &model.executable,
                        &arg_refs,
                        P8ModelProcessRoleV1::Reader,
                        composition.request_digest.clone(),
                        &request,
                    )
                    .map_err(process_failure)?;
                    let reader = P8ReaderExecutionReceiptV1::close(composition, process)?;
                    let join = P8RealBenchmarkJoinExecutionReceiptV1::after_reader(
                        &reader,
                        question.question_input_digest().clone(),
                        P8QualityDigest::derive(
                            "p8_real_fixture_rubric_gold_join_v1",
                            &(question.rubric_digest(), question.gold_digest()),
                        ),
                        &response,
                    )?;
                    (reader, join, response)
                };

                for main_work in &main_items {
                    let judge = execute_judge(
                        &model,
                        source_question,
                        main_work,
                        &baseline_join,
                        &baseline_response,
                    )?;
                    main_by_ordinal.insert(
                        main_work.schedule_ordinal(),
                        P8CompletedMainTrialReceiptV1::close_for_work_item(
                            execution_plan,
                            main_work,
                            baseline_reader.clone(),
                            baseline_join.clone(),
                            judge,
                            P8AccuracyOutcomeV1::Correct,
                            super::P8ExpectedCapabilityOutcomesV1::current_procedural()
                                .into_actual(),
                        )?,
                    );
                }

                if let Some(baseline_semantic) = baseline_semantic {
                    for (counterfactual, off_semantic, off_reader, off_join, off_response) in
                        off_runs
                    {
                        for off_work in execution_plan.work_items().iter().filter(|item| {
                            item.question_id() == question.question_id()
                                && item.arm() == *arm
                                && item.reader_repeat_index() == Some(reader_repeat_index)
                                && item.kind()
                                    == P8MechanicalWorkKindV1::SameClosureAblation {
                                        counterfactual,
                                    }
                        }) {
                            let off_judge = execute_judge(
                                &model,
                                source_question,
                                off_work,
                                &off_join,
                                &off_response,
                            )?;
                            let baseline_main = main_by_ordinal
                                .values()
                                .find(|receipt| {
                                    receipt.trial_key.question_id == *off_work.question_id()
                                        && receipt.trial_key.arm == off_work.arm()
                                        && Some(receipt.trial_key.reader_repeat_index)
                                            == off_work.reader_repeat_index()
                                        && Some(receipt.trial_key.judge_repeat_index)
                                            == off_work.judge_repeat_index()
                                })
                                .ok_or(P8QualityContractFailure::CoverageMismatch)?;
                            let mut pair_request = baseline_response.clone();
                            pair_request.push(b'|');
                            pair_request.extend_from_slice(&off_response);
                            let pair_expected = std::str::from_utf8(&pair_request)
                                .map_err(|_| P8QualityContractFailure::RawMaterialPresent)?;
                            let pair_args = ["judge", "exact", pair_expected];
                            let pair_digest = P8PairedJudgeExecutionReceiptV1::request_digest(
                                &baseline_main.judge,
                                &off_judge,
                                &pair_request,
                            )?;
                            let (pair_process, pair_output) = execute_fixture_model_process(
                                &model.executable,
                                &pair_args,
                                P8ModelProcessRoleV1::PairedJudge,
                                pair_digest,
                                &pair_request,
                            )
                            .map_err(process_failure)?;
                            if pair_output != b"correct" {
                                return Err(P8QualityContractFailure::ReceiptChainMismatch);
                            }
                            let paired = P8PairedJudgeExecutionReceiptV1::close(
                                &baseline_main.judge,
                                &off_judge,
                                &pair_request,
                                pair_process,
                                P8PairedJudgeOutcomeV1::Equivalent,
                            )?;
                            ablation_by_ordinal.insert(
                                off_work.schedule_ordinal(),
                                P8CompletedAblationTrialReceiptV1::close_for_work_item(
                                    execution_plan,
                                    off_work,
                                    baseline_main,
                                    baseline_semantic.clone(),
                                    off_semantic.clone(),
                                    off_reader.clone(),
                                    off_join.clone(),
                                    off_judge,
                                    paired,
                                    P8AccuracyOutcomeV1::Correct,
                                    super::P8ExpectedCapabilityOutcomesV1::current_procedural()
                                        .into_actual(),
                                )?,
                            );
                        }
                    }
                }
            }
        }
    }

    let expected_main = execution_plan
        .work_items()
        .iter()
        .filter(|item| item.kind() == P8MechanicalWorkKindV1::Main)
        .count();
    let expected_ablation = execution_plan
        .work_items()
        .iter()
        .filter(|item| {
            matches!(
                item.kind(),
                P8MechanicalWorkKindV1::SameClosureAblation { .. }
            )
        })
        .count();
    let expected_negative = execution_plan
        .work_items()
        .iter()
        .filter(|item| {
            matches!(
                item.kind(),
                P8MechanicalWorkKindV1::SafetyNegativeProof { .. }
            )
        })
        .count();
    if main_by_ordinal.len() != expected_main
        || ablation_by_ordinal.len() != expected_ablation
        || negative_by_ordinal.len() != expected_negative
    {
        return Err(P8QualityContractFailure::CoverageMismatch);
    }
    Ok(P8RealFixtureReceiptSet {
        main: main_by_ordinal.into_values().collect(),
        ablation: ablation_by_ordinal.into_values().collect(),
        negative: negative_by_ordinal.into_values().collect(),
    })
}

pub(super) fn build_real_fixture_runner_bundle_files(
    experiment_plan: &super::P8QualityExperimentPlanV1,
    execution_plan: &P8QualityExecutionPlanV1,
    dataset: &super::execution_plan::P8ZeroOriginTinyDatasetManifestV1,
    receipts: &P8RealFixtureReceiptSet,
    fixture_threshold_bytes: Option<&[u8]>,
) -> std::io::Result<BTreeMap<String, Vec<u8>>> {
    let mut files = execution_plan
        .exact_artifact_names()
        .iter()
        .map(|name| (name.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    files.insert(
        "experiment-plan.json".into(),
        serde_json::to_vec(experiment_plan)
            .map_err(|_| std::io::Error::other("serialize P8 fixture experiment plan"))?,
    );
    files.insert(
        "execution-plan.json".into(),
        serde_json::to_vec(execution_plan)
            .map_err(|_| std::io::Error::other("serialize P8 fixture execution plan"))?,
    );
    files.insert(
        "dataset-manifest.json".into(),
        serde_json::to_vec(dataset)
            .map_err(|_| std::io::Error::other("serialize P8 fixture dataset manifest"))?,
    );
    match (experiment_plan.purpose, fixture_threshold_bytes) {
        (super::P8QualityPurpose::BaselineEstablishment, None) => {}
        (super::P8QualityPurpose::QualityCandidate, Some(bytes)) => {
            files.insert("fixture-threshold.json".into(), bytes.to_vec());
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "P8 fixture threshold presence differs from experiment purpose",
            ));
        }
    }

    append_receipts_to_shards(
        execution_plan,
        execution_plan
            .work_items()
            .iter()
            .filter(|item| item.kind() == P8MechanicalWorkKindV1::Main),
        &receipts.main,
        "main",
        &mut files,
    )?;
    append_receipts_to_shards(
        execution_plan,
        execution_plan.work_items().iter().filter(|item| {
            matches!(
                item.kind(),
                P8MechanicalWorkKindV1::SameClosureAblation { .. }
            )
        }),
        &receipts.ablation,
        "ablation",
        &mut files,
    )?;
    append_receipts_to_shards(
        execution_plan,
        execution_plan.work_items().iter().filter(|item| {
            matches!(
                item.kind(),
                P8MechanicalWorkKindV1::SafetyNegativeProof { .. }
            )
        }),
        &receipts.negative,
        "negative",
        &mut files,
    )?;
    append_typed_fixture_manifests(execution_plan, receipts, &mut files)?;
    Ok(files)
}

fn append_typed_fixture_manifests(
    execution_plan: &P8QualityExecutionPlanV1,
    receipts: &P8RealFixtureReceiptSet,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> std::io::Result<()> {
    let shard_indices = execution_plan
        .work_items()
        .iter()
        .map(P8MechanicalWorkItemV1::shard_index)
        .collect::<std::collections::BTreeSet<_>>();
    if shard_indices
        .iter()
        .copied()
        .ne(0..u32::try_from(shard_indices.len()).unwrap_or(0))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "P8 fixture shard indices are not contiguous",
        ));
    }
    let mut shard_refs = Vec::with_capacity(shard_indices.len());
    for shard_index in shard_indices {
        let mut receipt_files = Vec::with_capacity(3);
        for suffix in ["ablation", "main", "negative"] {
            let file_name = format!("shard-{shard_index:05}.{suffix}.jsonl");
            let bytes = files.get(&file_name).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "P8 fixture receipt file is absent",
                )
            })?;
            let record_count = u64::try_from(
                bytes
                    .split(|byte| *byte == b'\n')
                    .filter(|line| !line.is_empty())
                    .count(),
            )
            .map_err(|_| std::io::Error::other("P8 fixture record count overflow"))?;
            receipt_files.push(P8FixtureArtifactFileEvidenceV1 {
                file_name,
                content_digest: P8QualityDigest::derive("p8_fixture_receipt_file_bytes_v1", &bytes),
                record_count,
            });
        }
        let manifest = P8FixtureShardManifestV1::build(
            execution_plan.run_id().clone(),
            shard_index,
            receipt_files,
        );
        if !manifest.validate_contract() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "P8 fixture shard manifest is invalid",
            ));
        }
        let file_name = format!("shard-{shard_index:05}.manifest.json");
        shard_refs.push(P8FixtureCohortShardRefV1 {
            shard_index,
            manifest_file_name: file_name.clone(),
            manifest_digest: manifest.manifest_digest.clone(),
        });
        files.insert(
            file_name,
            serde_json::to_vec(&manifest)
                .map_err(|_| std::io::Error::other("serialize P8 fixture shard manifest"))?,
        );
    }
    let cohort = P8FixtureCohortManifestV1::build(
        execution_plan.run_id().clone(),
        shard_refs,
        u64::try_from(receipts.main.len())
            .map_err(|_| std::io::Error::other("P8 fixture main count overflow"))?,
        u64::try_from(receipts.ablation.len())
            .map_err(|_| std::io::Error::other("P8 fixture ablation count overflow"))?,
        u64::try_from(receipts.negative.len())
            .map_err(|_| std::io::Error::other("P8 fixture negative count overflow"))?,
    );
    if !cohort.validate_contract() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "P8 fixture cohort manifest is invalid",
        ));
    }
    files.insert(
        "cohort-manifest.json".into(),
        serde_json::to_vec(&cohort)
            .map_err(|_| std::io::Error::other("serialize P8 fixture cohort manifest"))?,
    );
    Ok(())
}

fn append_receipts_to_shards<'a, T: Serialize + 'a>(
    execution_plan: &P8QualityExecutionPlanV1,
    work_items: impl Iterator<Item = &'a P8MechanicalWorkItemV1>,
    receipts: &'a [T],
    suffix: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> std::io::Result<()> {
    let work_items = work_items.collect::<Vec<_>>();
    if work_items.len() != receipts.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "P8 fixture receipt count differs from execution work items",
        ));
    }
    for (work_item, receipt) in work_items.into_iter().zip(receipts) {
        if !execution_plan.work_items().contains(work_item) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "P8 fixture receipt work item is outside execution plan",
            ));
        }
        let target = files
            .get_mut(&format!(
                "shard-{:05}.{suffix}.jsonl",
                work_item.shard_index()
            ))
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "P8 fixture shard target is absent",
                )
            })?;
        serde_json::to_writer(&mut *target, receipt)
            .map_err(|_| std::io::Error::other("serialize P8 fixture receipt"))?;
        target.push(b'\n');
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ReaderExecutionReceiptV1 {
    schema: String,
    composition: P8ProviderRequestCompositionReceiptV1,
    process: P8ClosedModelProcessReceiptV1,
    receipt_digest: P8ReaderExecutionReceiptRef,
}

impl P8ReaderExecutionReceiptV1 {
    pub(crate) fn close(
        composition: P8ProviderRequestCompositionReceiptV1,
        process: P8ClosedModelProcessReceiptV1,
    ) -> Result<Self, P8QualityContractFailure> {
        let mut value = Self {
            schema: READER_RECEIPT_SCHEMA.into(),
            composition,
            process,
            receipt_digest: P8ReaderExecutionReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_receipt();
        value.validate_contract().map(|()| value)
    }

    pub(crate) fn validate_contract(&self) -> Result<(), P8QualityContractFailure> {
        self.composition.validate_contract()?;
        self.process.validate_contract()?;
        if self.schema != READER_RECEIPT_SCHEMA
            || self.process.role != P8ModelProcessRoleV1::Reader
            || self.process.request_digest() != &self.composition.request_digest
            || self.process.attempts[0].request_bytes_digest
                != self.composition.final_request_bytes_digest
            || self.process.attempts[0].request_byte_count
                != self.composition.final_request_byte_count
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        if self.receipt_digest != self.derived_receipt() {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    fn derived_receipt(&self) -> P8ReaderExecutionReceiptRef {
        P8ReaderExecutionReceiptRef::derive(&(&self.schema, &self.composition, &self.process))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8RealBenchmarkJoinExecutionReceiptV1 {
    schema: String,
    run_id: P8QualityRunRef,
    reader_key: P8ReaderTrialKeyV1,
    reader_receipt: P8ReaderExecutionReceiptRef,
    reader_response_bytes_digest: P8QualityDigest,
    dataset_membership_digest: P8QualityDigest,
    ordered_rubric_gold_digest: P8QualityDigest,
    judge_input_digest: P8ModelRequestRef,
    judge_input_bytes_digest: P8QualityDigest,
    judge_input_byte_count: u64,
    receipt_digest: P8BenchmarkJoinExecutionReceiptRef,
}

impl P8RealBenchmarkJoinExecutionReceiptV1 {
    pub(crate) fn after_reader(
        reader: &P8ReaderExecutionReceiptV1,
        dataset_membership_digest: P8QualityDigest,
        ordered_rubric_gold_digest: P8QualityDigest,
        judge_input_bytes: &[u8],
    ) -> Result<Self, P8QualityContractFailure> {
        reader.validate_contract()?;
        let judge_input_byte_count = u64::try_from(judge_input_bytes.len())
            .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?;
        if judge_input_byte_count == 0 {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        let judge_input_digest = P8ModelRequestRef::derive(&(
            &reader.process.final_response_bytes_digest,
            &dataset_membership_digest,
            &ordered_rubric_gold_digest,
            observed_model_request_bytes_digest(judge_input_bytes),
            judge_input_byte_count,
        ));
        let mut value = Self {
            schema: BENCHMARK_JOIN_SCHEMA.into(),
            run_id: reader.composition.run_id.clone(),
            reader_key: reader.composition.reader_key.clone(),
            reader_receipt: reader.receipt_digest.clone(),
            reader_response_bytes_digest: reader.process.final_response_bytes_digest.clone(),
            dataset_membership_digest,
            ordered_rubric_gold_digest,
            judge_input_digest,
            judge_input_bytes_digest: observed_model_request_bytes_digest(judge_input_bytes),
            judge_input_byte_count,
            receipt_digest: P8BenchmarkJoinExecutionReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_receipt();
        value.validate_against_reader(reader).map(|()| value)
    }

    pub(crate) fn validate_against_reader(
        &self,
        reader: &P8ReaderExecutionReceiptV1,
    ) -> Result<(), P8QualityContractFailure> {
        reader.validate_contract()?;
        if self.schema != BENCHMARK_JOIN_SCHEMA
            || self.run_id != reader.composition.run_id
            || self.reader_key != reader.composition.reader_key
            || self.reader_receipt != reader.receipt_digest
            || self.reader_response_bytes_digest != reader.process.final_response_bytes_digest
        {
            return Err(P8QualityContractFailure::JoinOrderMismatch);
        }
        let expected_input = P8ModelRequestRef::derive(&(
            &self.reader_response_bytes_digest,
            &self.dataset_membership_digest,
            &self.ordered_rubric_gold_digest,
            &self.judge_input_bytes_digest,
            self.judge_input_byte_count,
        ));
        if self.judge_input_byte_count == 0
            || self.judge_input_digest != expected_input
            || self.receipt_digest != self.derived_receipt()
        {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    fn derived_receipt(&self) -> P8BenchmarkJoinExecutionReceiptRef {
        P8BenchmarkJoinExecutionReceiptRef::derive(&(
            &self.schema,
            &self.run_id,
            &self.reader_key,
            &self.reader_receipt,
            &self.reader_response_bytes_digest,
            &self.dataset_membership_digest,
            &self.ordered_rubric_gold_digest,
            &self.judge_input_digest,
            &self.judge_input_bytes_digest,
            self.judge_input_byte_count,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8JudgeExecutionReceiptV1 {
    schema: String,
    trial_key: P8QualityTrialKeyV1,
    join_receipt: P8BenchmarkJoinExecutionReceiptRef,
    process: P8ClosedModelProcessReceiptV1,
    receipt_digest: P8JudgeExecutionReceiptRef,
}

impl P8JudgeExecutionReceiptV1 {
    pub(crate) fn close_after_join(
        trial_key: P8QualityTrialKeyV1,
        join: &P8RealBenchmarkJoinExecutionReceiptV1,
        process: P8ClosedModelProcessReceiptV1,
    ) -> Result<Self, P8QualityContractFailure> {
        let mut value = Self {
            schema: JUDGE_RECEIPT_SCHEMA.into(),
            trial_key,
            join_receipt: join.receipt_digest.clone(),
            process,
            receipt_digest: P8JudgeExecutionReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_receipt();
        value.validate_against_join(join).map(|()| value)
    }

    pub(crate) fn validate_against_join(
        &self,
        join: &P8RealBenchmarkJoinExecutionReceiptV1,
    ) -> Result<(), P8QualityContractFailure> {
        self.process.validate_contract()?;
        if self.schema != JUDGE_RECEIPT_SCHEMA
            || self.process.role != P8ModelProcessRoleV1::Judge
            || self.trial_key.reader_key() != join.reader_key
            || self.join_receipt != join.receipt_digest
            || self.process.request_digest() != &join.judge_input_digest
            || self.process.attempts[0].request_bytes_digest != join.judge_input_bytes_digest
            || self.process.attempts[0].request_byte_count != join.judge_input_byte_count
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        if self.receipt_digest != self.derived_receipt() {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    fn derived_receipt(&self) -> P8JudgeExecutionReceiptRef {
        P8JudgeExecutionReceiptRef::derive(&(
            &self.schema,
            &self.trial_key,
            &self.join_receipt,
            &self.process,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8PairedJudgeOutcomeV1 {
    BaselinePreferred,
    OffRunPreferred,
    Equivalent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8PairedJudgeExecutionReceiptV1 {
    schema: String,
    baseline_judge_receipt: P8JudgeExecutionReceiptRef,
    off_run_judge_receipt: P8JudgeExecutionReceiptRef,
    request_digest: P8ModelRequestRef,
    request_bytes_digest: P8QualityDigest,
    request_byte_count: u64,
    process: P8ClosedModelProcessReceiptV1,
    outcome: P8PairedJudgeOutcomeV1,
    receipt_digest: super::P8PairedJudgeReceiptRef,
}

impl P8PairedJudgeExecutionReceiptV1 {
    pub(crate) fn request_digest(
        baseline: &P8JudgeExecutionReceiptV1,
        off_run: &P8JudgeExecutionReceiptV1,
        request_bytes: &[u8],
    ) -> Result<P8ModelRequestRef, P8QualityContractFailure> {
        let request_byte_count = u64::try_from(request_bytes.len())
            .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?;
        if request_byte_count == 0 {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        Ok(P8ModelRequestRef::derive(&(
            &baseline.receipt_digest,
            &off_run.receipt_digest,
            observed_model_request_bytes_digest(request_bytes),
            request_byte_count,
        )))
    }

    pub(crate) fn close(
        baseline: &P8JudgeExecutionReceiptV1,
        off_run: &P8JudgeExecutionReceiptV1,
        request_bytes: &[u8],
        process: P8ClosedModelProcessReceiptV1,
        outcome: P8PairedJudgeOutcomeV1,
    ) -> Result<Self, P8QualityContractFailure> {
        let request_digest = Self::request_digest(baseline, off_run, request_bytes)?;
        let mut value = Self {
            schema: PAIRED_JUDGE_RECEIPT_SCHEMA.into(),
            baseline_judge_receipt: baseline.receipt_digest.clone(),
            off_run_judge_receipt: off_run.receipt_digest.clone(),
            request_digest,
            request_bytes_digest: observed_model_request_bytes_digest(request_bytes),
            request_byte_count: u64::try_from(request_bytes.len())
                .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?,
            process,
            outcome,
            receipt_digest: super::P8PairedJudgeReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_receipt();
        value.validate_against(baseline, off_run).map(|()| value)
    }

    pub(crate) fn validate_against(
        &self,
        baseline: &P8JudgeExecutionReceiptV1,
        off_run: &P8JudgeExecutionReceiptV1,
    ) -> Result<(), P8QualityContractFailure> {
        self.process.validate_contract()?;
        if self.schema != PAIRED_JUDGE_RECEIPT_SCHEMA
            || self.baseline_judge_receipt != baseline.receipt_digest
            || self.off_run_judge_receipt != off_run.receipt_digest
            || self.process.role != P8ModelProcessRoleV1::PairedJudge
            || self.process.request_digest() != &self.request_digest
            || self.process.attempts[0].request_bytes_digest != self.request_bytes_digest
            || self.process.attempts[0].request_byte_count != self.request_byte_count
            || self.request_digest
                != P8ModelRequestRef::derive(&(
                    &self.baseline_judge_receipt,
                    &self.off_run_judge_receipt,
                    &self.request_bytes_digest,
                    self.request_byte_count,
                ))
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        if self.receipt_digest != self.derived_receipt() {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    fn derived_receipt(&self) -> super::P8PairedJudgeReceiptRef {
        super::P8PairedJudgeReceiptRef::derive(&(
            &self.schema,
            &self.baseline_judge_receipt,
            &self.off_run_judge_receipt,
            &self.request_digest,
            &self.request_bytes_digest,
            self.request_byte_count,
            &self.process,
            self.outcome,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8CompletedAblationTrialReceiptV1 {
    schema: String,
    run_id: P8QualityRunRef,
    key: super::P8QualityAblationKeyV1,
    arm_release_digest: P8ArmReleaseRef,
    baseline_main_receipt: P8CompletedTrialReceiptRef,
    baseline_semantic: P8ReplaySemanticExecutionReceiptV2,
    off_run_semantic: P8ReplaySemanticExecutionReceiptV2,
    off_run_reader: P8ReaderExecutionReceiptV1,
    off_run_benchmark_join: P8RealBenchmarkJoinExecutionReceiptV1,
    off_run_judge: P8JudgeExecutionReceiptV1,
    paired_judge: P8PairedJudgeExecutionReceiptV1,
    off_run_accuracy: P8AccuracyOutcomeV1,
    off_run_capability_outcomes: P8ActualCapabilityOutcomesV1,
    receipt_digest: P8CompletedTrialReceiptRef,
}

impl P8CompletedAblationTrialReceiptV1 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn close_for_work_item(
        execution_plan: &P8QualityExecutionPlanV1,
        work_item: &P8MechanicalWorkItemV1,
        baseline_main: &P8CompletedMainTrialReceiptV1,
        baseline_semantic: P8ReplaySemanticExecutionReceiptV2,
        off_run_semantic: P8ReplaySemanticExecutionReceiptV2,
        off_run_reader: P8ReaderExecutionReceiptV1,
        off_run_benchmark_join: P8RealBenchmarkJoinExecutionReceiptV1,
        off_run_judge: P8JudgeExecutionReceiptV1,
        paired_judge: P8PairedJudgeExecutionReceiptV1,
        off_run_accuracy: P8AccuracyOutcomeV1,
        off_run_capability_outcomes: P8ActualCapabilityOutcomesV1,
    ) -> Result<Self, P8QualityContractFailure> {
        let P8MechanicalWorkKindV1::SameClosureAblation { counterfactual } = work_item.kind()
        else {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        };
        let mut value = Self {
            schema: COMPLETED_ABLATION_TRIAL_SCHEMA.into(),
            run_id: execution_plan.run_id().clone(),
            key: super::P8QualityAblationKeyV1 {
                question_id: work_item.question_id().clone(),
                arm: work_item.arm(),
                counterfactual,
                reader_repeat_index: work_item
                    .reader_repeat_index()
                    .ok_or(P8QualityContractFailure::CoverageMismatch)?,
                judge_repeat_index: work_item
                    .judge_repeat_index()
                    .ok_or(P8QualityContractFailure::CoverageMismatch)?,
            },
            arm_release_digest: work_item.arm_release_digest().clone(),
            baseline_main_receipt: baseline_main.receipt_digest.clone(),
            baseline_semantic,
            off_run_semantic,
            off_run_reader,
            off_run_benchmark_join,
            off_run_judge,
            paired_judge,
            off_run_accuracy,
            off_run_capability_outcomes,
            receipt_digest: P8CompletedTrialReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_receipt();
        value
            .validate_against_work_item(execution_plan, work_item, baseline_main)
            .map(|()| value)
    }

    pub(crate) fn validate_against_work_item(
        &self,
        execution_plan: &P8QualityExecutionPlanV1,
        work_item: &P8MechanicalWorkItemV1,
        baseline_main: &P8CompletedMainTrialReceiptV1,
    ) -> Result<(), P8QualityContractFailure> {
        let P8MechanicalWorkKindV1::SameClosureAblation { counterfactual } = work_item.kind()
        else {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        };
        let expected_key = super::P8QualityAblationKeyV1 {
            question_id: work_item.question_id().clone(),
            arm: work_item.arm(),
            counterfactual,
            reader_repeat_index: work_item
                .reader_repeat_index()
                .ok_or(P8QualityContractFailure::CoverageMismatch)?,
            judge_repeat_index: work_item
                .judge_repeat_index()
                .ok_or(P8QualityContractFailure::CoverageMismatch)?,
        };
        let baseline_work_item = execution_plan
            .work_items()
            .iter()
            .find(|candidate| {
                candidate.kind() == P8MechanicalWorkKindV1::Main
                    && candidate.question_id() == work_item.question_id()
                    && candidate.arm() == work_item.arm()
                    && candidate.reader_repeat_index() == work_item.reader_repeat_index()
                    && candidate.judge_repeat_index() == work_item.judge_repeat_index()
            })
            .ok_or(P8QualityContractFailure::CoverageMismatch)?;
        baseline_main.validate_against_work_item(execution_plan, baseline_work_item)?;
        let (baseline_semantic_ref, baseline_projection) =
            match &baseline_main.reader.composition.input {
                P8CompositionInputV1::BeetleProviderSafeProjection {
                    semantic_execution_receipt_v2,
                    projection_digest,
                } => (semantic_execution_receipt_v2, projection_digest),
                _ => return Err(P8QualityContractFailure::ReceiptChainMismatch),
            };
        let (off_semantic_ref, off_baseline_projection, off_projection) =
            match &self.off_run_reader.composition.input {
                P8CompositionInputV1::SameClosureSafeCounterfactual {
                    semantic_execution_receipt_v2,
                    baseline_projection_digest,
                    off_run_projection_digest,
                } => (
                    semantic_execution_receipt_v2,
                    baseline_projection_digest,
                    off_run_projection_digest,
                ),
                _ => return Err(P8QualityContractFailure::ReceiptChainMismatch),
            };
        if self.schema != COMPLETED_ABLATION_TRIAL_SCHEMA
            || !execution_plan.work_items().contains(work_item)
            || &self.run_id != execution_plan.run_id()
            || self.key != expected_key
            || &self.arm_release_digest != work_item.arm_release_digest()
            || self.baseline_main_receipt != baseline_main.receipt_digest
            || baseline_semantic_ref != &self.baseline_semantic.receipt_digest
            || baseline_projection != &self.baseline_semantic.selected_projection_digest
            || off_semantic_ref != &self.off_run_semantic.receipt_digest
            || off_baseline_projection != &self.baseline_semantic.selected_projection_digest
            || off_projection != &self.off_run_semantic.selected_projection_digest
            || self.baseline_semantic.sdk_closure_receipt_digest
                != self.off_run_semantic.sdk_closure_receipt_digest
            || self.baseline_semantic.authority_identity_digest
                != self.off_run_semantic.authority_identity_digest
            || self.baseline_semantic.store_snapshot_identity_digest
                != self.off_run_semantic.store_snapshot_identity_digest
            || self.baseline_semantic.profile != self.off_run_semantic.profile
            || self.baseline_semantic.capability_catalog_identity_digest
                != self.off_run_semantic.capability_catalog_identity_digest
            || self.baseline_semantic.runtime_budget_report_id
                != self.off_run_semantic.runtime_budget_report_id
            || self.baseline_semantic.selected_projection_digest
                == self.off_run_semantic.selected_projection_digest
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        self.baseline_semantic.validate_contract()?;
        self.off_run_semantic.validate_contract()?;
        self.off_run_reader.validate_contract()?;
        self.off_run_benchmark_join
            .validate_against_reader(&self.off_run_reader)?;
        self.off_run_judge
            .validate_against_join(&self.off_run_benchmark_join)?;
        if self.off_run_judge.trial_key.question_id != self.key.question_id
            || self.off_run_judge.trial_key.arm != self.key.arm
            || self.off_run_judge.trial_key.reader_repeat_index != self.key.reader_repeat_index
            || self.off_run_judge.trial_key.judge_repeat_index != self.key.judge_repeat_index
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        self.paired_judge
            .validate_against(&baseline_main.judge, &self.off_run_judge)?;
        if self.receipt_digest != self.derived_receipt() {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    pub(crate) fn key(&self) -> &super::P8QualityAblationKeyV1 {
        &self.key
    }

    pub(crate) const fn paired_outcome(&self) -> P8PairedJudgeOutcomeV1 {
        self.paired_judge.outcome
    }

    pub(crate) fn off_run_accuracy(&self) -> &P8AccuracyOutcomeV1 {
        &self.off_run_accuracy
    }

    pub(super) fn hard_gate_evidences(&self) -> [&P8ReplaySemanticHardGateEvidenceV1; 2] {
        [
            &self.baseline_semantic.hard_gate_evidence,
            &self.off_run_semantic.hard_gate_evidence,
        ]
    }

    #[cfg(test)]
    pub(super) fn baseline_hard_gate_evidence_mut(
        &mut self,
    ) -> &mut P8ReplaySemanticHardGateEvidenceV1 {
        &mut self.baseline_semantic.hard_gate_evidence
    }

    fn derived_receipt(&self) -> P8CompletedTrialReceiptRef {
        P8CompletedTrialReceiptRef::derive(&(
            &self.schema,
            &self.run_id,
            &self.key,
            &self.arm_release_digest,
            &self.baseline_main_receipt,
            &self.baseline_semantic,
            &self.off_run_semantic,
            &self.off_run_reader,
            &self.off_run_benchmark_join,
            &self.off_run_judge,
            &self.paired_judge,
            &self.off_run_accuracy,
            &self.off_run_capability_outcomes,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8CompletedMainTrialReceiptV1 {
    schema: String,
    run_id: P8QualityRunRef,
    trial_key: P8QualityTrialKeyV1,
    arm_release_digest: P8ArmReleaseRef,
    reader: P8ReaderExecutionReceiptV1,
    benchmark_join: P8RealBenchmarkJoinExecutionReceiptV1,
    judge: P8JudgeExecutionReceiptV1,
    accuracy: P8AccuracyOutcomeV1,
    capability_outcomes: P8ActualCapabilityOutcomesV1,
    receipt_digest: P8CompletedTrialReceiptRef,
}

impl P8CompletedMainTrialReceiptV1 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn close_for_work_item(
        execution_plan: &P8QualityExecutionPlanV1,
        work_item: &P8MechanicalWorkItemV1,
        reader: P8ReaderExecutionReceiptV1,
        benchmark_join: P8RealBenchmarkJoinExecutionReceiptV1,
        judge: P8JudgeExecutionReceiptV1,
        accuracy: P8AccuracyOutcomeV1,
        capability_outcomes: P8ActualCapabilityOutcomesV1,
    ) -> Result<Self, P8QualityContractFailure> {
        if work_item.kind() != P8MechanicalWorkKindV1::Main {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        let trial_key = P8QualityTrialKeyV1 {
            question_id: work_item.question_id().clone(),
            arm: work_item.arm(),
            reader_repeat_index: work_item
                .reader_repeat_index()
                .ok_or(P8QualityContractFailure::CoverageMismatch)?,
            judge_repeat_index: work_item
                .judge_repeat_index()
                .ok_or(P8QualityContractFailure::CoverageMismatch)?,
        };
        let mut value = Self {
            schema: COMPLETED_MAIN_TRIAL_SCHEMA.into(),
            run_id: execution_plan.run_id().clone(),
            trial_key,
            arm_release_digest: work_item.arm_release_digest().clone(),
            reader,
            benchmark_join,
            judge,
            accuracy,
            capability_outcomes,
            receipt_digest: P8CompletedTrialReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_receipt();
        value
            .validate_against_work_item(execution_plan, work_item)
            .map(|()| value)
    }

    pub(crate) fn validate_against_work_item(
        &self,
        execution_plan: &P8QualityExecutionPlanV1,
        work_item: &P8MechanicalWorkItemV1,
    ) -> Result<(), P8QualityContractFailure> {
        if self.schema != COMPLETED_MAIN_TRIAL_SCHEMA
            || work_item.kind() != P8MechanicalWorkKindV1::Main
            || !execution_plan.work_items().contains(work_item)
            || &self.run_id != execution_plan.run_id()
            || &self.trial_key.question_id != work_item.question_id()
            || self.trial_key.arm != work_item.arm()
            || Some(self.trial_key.reader_repeat_index) != work_item.reader_repeat_index()
            || Some(self.trial_key.judge_repeat_index) != work_item.judge_repeat_index()
            || &self.arm_release_digest != work_item.arm_release_digest()
            || self.reader.composition.run_id != self.run_id
            || self.reader.composition.reader_key != self.trial_key.reader_key()
            || self.reader.composition.arm_release_digest != self.arm_release_digest
        {
            return Err(P8QualityContractFailure::ReceiptChainMismatch);
        }
        self.reader.validate_contract()?;
        self.benchmark_join.validate_against_reader(&self.reader)?;
        self.judge.validate_against_join(&self.benchmark_join)?;
        if self.judge.trial_key != self.trial_key || self.receipt_digest != self.derived_receipt() {
            return Err(P8QualityContractFailure::DigestInvalid);
        }
        Ok(())
    }

    pub(crate) fn trial_key(&self) -> &P8QualityTrialKeyV1 {
        &self.trial_key
    }

    pub(crate) fn accuracy(&self) -> &P8AccuracyOutcomeV1 {
        &self.accuracy
    }

    pub(crate) fn capability_outcomes(&self) -> &P8ActualCapabilityOutcomesV1 {
        &self.capability_outcomes
    }

    pub(crate) const fn memory_projection_rendered_chars(&self) -> u64 {
        self.reader.composition.memory_projection_rendered_chars
    }

    pub(crate) fn reader_elapsed_nanoseconds(&self) -> Result<u64, P8QualityContractFailure> {
        self.reader
            .process
            .attempts
            .iter()
            .try_fold(0_u64, |total, attempt| {
                total
                    .checked_add(attempt.elapsed_nanoseconds)
                    .ok_or(P8QualityContractFailure::ArithmeticOverflow)
            })
    }

    fn derived_receipt(&self) -> P8CompletedTrialReceiptRef {
        P8CompletedTrialReceiptRef::derive(&(
            &self.schema,
            &self.run_id,
            &self.trial_key,
            &self.arm_release_digest,
            &self.reader,
            &self.benchmark_join,
            &self.judge,
            &self.accuracy,
            &self.capability_outcomes,
        ))
    }
}

#[cfg(test)]
fn fixture_semantic_receipt(
    execution_plan: &P8QualityExecutionPlanV1,
    work_item: &P8MechanicalWorkItemV1,
    counterfactual: Option<super::P8SameClosureSafeCounterfactualKindV1>,
) -> P8ReplaySemanticExecutionReceiptV2 {
    let mut value = P8ReplaySemanticExecutionReceiptV2 {
        schema: SEMANTIC_EXECUTION_SCHEMA.into(),
        sdk_closure_receipt_digest: P8QualityDigest::derive(
            "p8_fixture_sdk_closure_receipt_v2",
            &(
                execution_plan.run_id(),
                work_item.question_id(),
                work_item.arm(),
                work_item.reader_repeat_index(),
            ),
        ),
        profile: bm_core::feature_gate::ProfileId::ServerLinuxDevFull,
        capability_catalog_identity_digest: P8QualityDigest::derive(
            "p8_fixture_capability_catalog_v1",
            &"fixture",
        ),
        runtime_budget_report_id: super::P8RuntimeBudgetReportIdV1::parse(format!(
            "rtb-v2-{}",
            "7".repeat(64)
        ))
        .expect("fixture budget id"),
        authority_identity_digest: P8QualityDigest::derive(
            "p8_fixture_authority_v1",
            &(execution_plan.run_id(), work_item.question_id()),
        ),
        store_snapshot_identity_digest: P8QualityDigest::derive(
            "p8_fixture_store_snapshot_v1",
            &(execution_plan.run_id(), work_item.question_id()),
        ),
        materialization_count: 1,
        selected_projection_digest: P8ProviderSafeProjectionRef::derive(&(
            execution_plan.run_id(),
            work_item.question_id(),
            work_item.arm(),
            work_item.reader_repeat_index(),
            counterfactual,
        )),
        selected_projection_receipt_digest: P8QualityDigest::derive(
            "p8_fixture_projection_receipt_v1",
            &(
                execution_plan.run_id(),
                work_item.question_id(),
                work_item.arm(),
                work_item.reader_repeat_index(),
                counterfactual,
            ),
        ),
        memory_projection_rendered_chars: 16,
        hard_gate_evidence: P8ReplaySemanticHardGateEvidenceV1 {
            ineligible_owner_projection_count: 0,
            non_current_material_projection_count: 0,
            cross_subject_private_soul_leak_count: 0,
            full_store_scan_second_platform_or_live_fallback_count: 0,
            post_image_closure_covered: true,
            update_lineage_violation_count: 0,
            unmet_premise_procedure_delivery_count: 0,
            profile_budget_render_ceiling_breach_count: 0,
            unexpected_runtime_or_integrity_failure_count: 0,
        },
        receipt_digest: super::P8SemanticExecutionReceiptV2Ref::derive(&()),
    };
    value.receipt_digest = value.derived_receipt();
    value
}

#[cfg(test)]
pub(super) fn fixture_completed_main_receipt(
    execution_plan: &P8QualityExecutionPlanV1,
    work_item: &P8MechanicalWorkItemV1,
    succeeds: bool,
) -> P8CompletedMainTrialReceiptV1 {
    let input = match work_item.arm() {
        P8QualityArmKind::NoMemory => P8CompositionInputV1::NoMemoryEmpty,
        P8QualityArmKind::PublicReference => P8CompositionInputV1::PublicReferenceSafeOutput {
            safe_output_digest: P8QualityDigest::derive(
                "p8_fixture_public_safe_output_v1",
                work_item.question_id(),
            ),
        },
        P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate => {
            let semantic = fixture_semantic_receipt(execution_plan, work_item, None);
            P8CompositionInputV1::BeetleProviderSafeProjection {
                semantic_execution_receipt_v2: semantic.receipt_digest,
                projection_digest: semantic.selected_projection_digest,
            }
        }
    };
    let memory_chars = matches!(
        work_item.arm(),
        P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate
    )
    .then_some(16)
    .unwrap_or(0);
    let fixture_request = b"fixture-provider-request-with-safe-memory-projection";
    let fixture_request_digest = observed_model_request_bytes_digest(fixture_request);
    let fixture_request_len = u64::try_from(fixture_request.len()).expect("fixture request");
    let composition = P8ProviderRequestCompositionReceiptV1::build_for_work_item(
        execution_plan,
        work_item,
        input,
        P8QualityDigest::derive("p8_fixture_question_safe_input_v1", work_item.question_id()),
        P8QualityDigest::derive("p8_fixture_prompt_template_v1", &"template"),
        P8QualityDigest::derive("p8_fixture_tool_schema_v1", &"tools"),
        fixture_request_digest.clone(),
        fixture_request_len,
        memory_chars,
        fixture_request_len,
    )
    .expect("fixture composition");
    let reader_process = P8ClosedModelProcessReceiptV1::close(
        P8ModelProcessRoleV1::Reader,
        P8QualityDigest::derive("p8_fixture_reader_executable_v1", &"reader"),
        P8QualityDigest::derive("p8_fixture_provider_v1", &"provider"),
        P8QualityDigest::derive("p8_fixture_reader_model_v1", &"reader-model"),
        vec![P8ObservedModelAttemptV1::fixture(
            0,
            composition.request_digest.clone(),
            fixture_request_digest,
            fixture_request_len,
            if succeeds {
                "reader-success"
            } else {
                "reader-failure"
            },
            0,
        )],
    )
    .expect("fixture reader process");
    let reader = P8ReaderExecutionReceiptV1::close(composition, reader_process)
        .expect("fixture reader receipt");
    let fixture_judge_request = b"fixture-judge-request-after-reader-close";
    let fixture_judge_request_digest = observed_model_request_bytes_digest(fixture_judge_request);
    let fixture_judge_request_len =
        u64::try_from(fixture_judge_request.len()).expect("fixture judge request");
    let join = P8RealBenchmarkJoinExecutionReceiptV1::after_reader(
        &reader,
        P8QualityDigest::derive("p8_fixture_dataset_membership_v1", work_item.question_id()),
        P8QualityDigest::derive("p8_fixture_rubric_gold_v1", work_item.question_id()),
        fixture_judge_request,
    )
    .expect("fixture join");
    let judge_process = P8ClosedModelProcessReceiptV1::close(
        P8ModelProcessRoleV1::Judge,
        P8QualityDigest::derive("p8_fixture_judge_executable_v1", &"judge"),
        P8QualityDigest::derive("p8_fixture_provider_v1", &"provider"),
        P8QualityDigest::derive("p8_fixture_judge_model_v1", &"judge-model"),
        vec![P8ObservedModelAttemptV1::fixture(
            0,
            join.judge_input_digest.clone(),
            fixture_judge_request_digest,
            fixture_judge_request_len,
            if succeeds {
                "judge-correct"
            } else {
                "judge-incorrect"
            },
            0,
        )],
    )
    .expect("fixture judge process");
    let trial_key = P8QualityTrialKeyV1 {
        question_id: work_item.question_id().clone(),
        arm: work_item.arm(),
        reader_repeat_index: work_item.reader_repeat_index().expect("main reader repeat"),
        judge_repeat_index: work_item.judge_repeat_index().expect("main judge repeat"),
    };
    let judge = P8JudgeExecutionReceiptV1::close_after_join(trial_key, &join, judge_process)
        .expect("fixture judge receipt");
    P8CompletedMainTrialReceiptV1::close_for_work_item(
        execution_plan,
        work_item,
        reader,
        join,
        judge,
        if succeeds {
            P8AccuracyOutcomeV1::Correct
        } else {
            P8AccuracyOutcomeV1::Incorrect
        },
        super::P8ExpectedCapabilityOutcomesV1::current_procedural().into_actual(),
    )
    .expect("fixture completed main receipt")
}

#[cfg(test)]
pub(super) fn fixture_completed_ablation_receipt(
    execution_plan: &P8QualityExecutionPlanV1,
    work_item: &P8MechanicalWorkItemV1,
    baseline_main: &P8CompletedMainTrialReceiptV1,
) -> P8CompletedAblationTrialReceiptV1 {
    let counterfactual = match work_item.kind() {
        P8MechanicalWorkKindV1::SameClosureAblation { counterfactual } => counterfactual,
        _ => panic!("fixture ablation requires an ablation work item"),
    };
    let baseline_semantic = fixture_semantic_receipt(execution_plan, work_item, None);
    let off_run_semantic =
        fixture_semantic_receipt(execution_plan, work_item, Some(counterfactual));
    let request_bytes = b"fixture-off-run-provider-request-with-safe-projection";
    let request_digest = observed_model_request_bytes_digest(request_bytes);
    let request_len = u64::try_from(request_bytes.len()).expect("fixture request");
    let composition = P8ProviderRequestCompositionReceiptV1::build_for_work_item(
        execution_plan,
        work_item,
        P8CompositionInputV1::SameClosureSafeCounterfactual {
            semantic_execution_receipt_v2: off_run_semantic.receipt_digest.clone(),
            baseline_projection_digest: baseline_semantic.selected_projection_digest.clone(),
            off_run_projection_digest: off_run_semantic.selected_projection_digest.clone(),
        },
        P8QualityDigest::derive("p8_fixture_question_safe_input_v1", work_item.question_id()),
        P8QualityDigest::derive("p8_fixture_prompt_template_v1", &"template"),
        P8QualityDigest::derive("p8_fixture_tool_schema_v1", &"tools"),
        request_digest.clone(),
        request_len,
        16,
        request_len,
    )
    .expect("fixture off-run composition");
    let reader_process = P8ClosedModelProcessReceiptV1::close(
        P8ModelProcessRoleV1::Reader,
        P8QualityDigest::derive("p8_fixture_reader_executable_v1", &"reader"),
        P8QualityDigest::derive("p8_fixture_provider_v1", &"provider"),
        P8QualityDigest::derive("p8_fixture_reader_model_v1", &"reader-model"),
        vec![P8ObservedModelAttemptV1::fixture(
            0,
            composition.request_digest.clone(),
            request_digest,
            request_len,
            "off-reader-success",
            0,
        )],
    )
    .expect("fixture off-run reader process");
    let reader =
        P8ReaderExecutionReceiptV1::close(composition, reader_process).expect("off reader");
    let judge_request = b"fixture-off-run-judge-request";
    let join = P8RealBenchmarkJoinExecutionReceiptV1::after_reader(
        &reader,
        P8QualityDigest::derive("p8_fixture_dataset_membership_v1", work_item.question_id()),
        P8QualityDigest::derive("p8_fixture_rubric_gold_v1", work_item.question_id()),
        judge_request,
    )
    .expect("fixture off-run join");
    let judge_request_digest = observed_model_request_bytes_digest(judge_request);
    let judge_request_len = u64::try_from(judge_request.len()).expect("judge request");
    let judge_process = P8ClosedModelProcessReceiptV1::close(
        P8ModelProcessRoleV1::Judge,
        P8QualityDigest::derive("p8_fixture_judge_executable_v1", &"judge"),
        P8QualityDigest::derive("p8_fixture_provider_v1", &"provider"),
        P8QualityDigest::derive("p8_fixture_judge_model_v1", &"judge-model"),
        vec![P8ObservedModelAttemptV1::fixture(
            0,
            join.judge_input_digest.clone(),
            judge_request_digest,
            judge_request_len,
            "off-judge-correct",
            0,
        )],
    )
    .expect("fixture off-run judge process");
    let judge = P8JudgeExecutionReceiptV1::close_after_join(
        P8QualityTrialKeyV1 {
            question_id: work_item.question_id().clone(),
            arm: work_item.arm(),
            reader_repeat_index: work_item.reader_repeat_index().expect("reader repeat"),
            judge_repeat_index: work_item.judge_repeat_index().expect("judge repeat"),
        },
        &join,
        judge_process,
    )
    .expect("fixture off-run judge");
    let pair_request = b"fixture-paired-judge-request";
    let pair_request_digest =
        P8PairedJudgeExecutionReceiptV1::request_digest(&baseline_main.judge, &judge, pair_request)
            .expect("pair request digest");
    let pair_process = P8ClosedModelProcessReceiptV1::close(
        P8ModelProcessRoleV1::PairedJudge,
        P8QualityDigest::derive("p8_fixture_judge_executable_v1", &"paired-judge"),
        P8QualityDigest::derive("p8_fixture_provider_v1", &"provider"),
        P8QualityDigest::derive("p8_fixture_judge_model_v1", &"paired-judge-model"),
        vec![P8ObservedModelAttemptV1::fixture(
            0,
            pair_request_digest,
            observed_model_request_bytes_digest(pair_request),
            u64::try_from(pair_request.len()).expect("pair request"),
            "paired-equivalent",
            0,
        )],
    )
    .expect("fixture pair process");
    let paired = P8PairedJudgeExecutionReceiptV1::close(
        &baseline_main.judge,
        &judge,
        pair_request,
        pair_process,
        P8PairedJudgeOutcomeV1::Equivalent,
    )
    .expect("fixture paired judge");
    P8CompletedAblationTrialReceiptV1::close_for_work_item(
        execution_plan,
        work_item,
        baseline_main,
        baseline_semantic,
        off_run_semantic,
        reader,
        join,
        judge,
        paired,
        P8AccuracyOutcomeV1::Correct,
        super::P8ExpectedCapabilityOutcomesV1::current_procedural().into_actual(),
    )
    .expect("fixture completed ablation")
}

#[cfg(test)]
pub(super) fn fixture_completed_negative_receipt(
    execution_plan: &P8QualityExecutionPlanV1,
    work_item: &P8MechanicalWorkItemV1,
) -> P8CompletedNegativeOnlyProofReceiptV1 {
    let proof = match work_item.kind() {
        P8MechanicalWorkKindV1::SafetyNegativeProof { proof } => proof,
        _ => panic!("fixture negative receipt requires a negative work item"),
    };
    let mut value = P8CompletedNegativeOnlyProofReceiptV1 {
        schema: COMPLETED_NEGATIVE_PROOF_SCHEMA.into(),
        run_id: execution_plan.run_id().clone(),
        key: super::P8SafetyNegativeProofKeyV1 {
            question_id: work_item.question_id().clone(),
            arm: work_item.arm(),
            proof,
        },
        arm_release_digest: work_item.arm_release_digest().clone(),
        sdk_closure_receipt_digest: P8QualityDigest::derive(
            "p8_fixture_negative_closure_v1",
            &(
                execution_plan.run_id(),
                work_item.question_id(),
                work_item.arm(),
            ),
        ),
        authority_identity_digest: P8QualityDigest::derive(
            "p8_fixture_negative_authority_v1",
            &(execution_plan.run_id(), work_item.question_id()),
        ),
        store_snapshot_identity_digest: P8QualityDigest::derive(
            "p8_fixture_negative_snapshot_v1",
            &(execution_plan.run_id(), work_item.question_id()),
        ),
        sdk_off_run_digest: P8QualityDigest::derive(
            "p8_fixture_negative_off_run_v1",
            &(work_item.question_id(), proof),
        ),
        sdk_negative_proof_digest: P8QualityDigest::derive(
            "p8_fixture_negative_sdk_proof_v1",
            &(work_item.question_id(), proof),
        ),
        applicable: true,
        provider_payload_count: 0,
        model_boundary: super::P8NegativeProofModelBoundaryV1::NoReaderModelJudgeOrAccuracy,
        receipt_digest: super::P8SafetyProofReceiptRef::derive(&()),
    };
    value.receipt_digest = value.derived_receipt();
    value
        .validate_against_work_item(execution_plan, work_item)
        .expect("fixture negative contract");
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p8_quality::{P8QualityId, P8SemanticExecutionReceiptV2Ref};

    fn digest(label: &str) -> P8QualityDigest {
        P8QualityDigest::derive("p8_runner_execution_test_digest_v1", &label)
    }

    fn fixture_reader() -> P8ReaderExecutionReceiptV1 {
        let request_bytes = b"fixture-reader-request-with-projection";
        let request_bytes_digest = observed_model_request_bytes_digest(request_bytes);
        let request_byte_count = u64::try_from(request_bytes.len()).expect("fixture request");
        let reader_key = P8ReaderTrialKeyV1 {
            question_id: P8QualityId::parse("q-1").expect("question id"),
            arm: P8QualityArmKind::FrozenP84Baseline,
            reader_repeat_index: 0,
        };
        let composition = P8ProviderRequestCompositionReceiptV1::build_fixture(
            P8QualityRunRef::derive(&"run"),
            reader_key,
            P8ArmReleaseRef::derive_for_test("arm"),
            P8CompositionInputV1::BeetleProviderSafeProjection {
                semantic_execution_receipt_v2: P8SemanticExecutionReceiptV2Ref::derive(&"v2"),
                projection_digest: P8ProviderSafeProjectionRef::derive(&"projection"),
            },
            digest("question"),
            digest("template"),
            digest("tools"),
            request_bytes_digest.clone(),
            request_byte_count,
            10,
            request_byte_count,
        )
        .expect("composition");
        let process = P8ClosedModelProcessReceiptV1::close(
            P8ModelProcessRoleV1::Reader,
            digest("reader executable"),
            digest("provider"),
            digest("reader model"),
            vec![P8ObservedModelAttemptV1::fixture(
                0,
                composition.request_digest.clone(),
                request_bytes_digest,
                request_byte_count,
                "reader response",
                0,
            )],
        )
        .expect("reader process");
        P8ReaderExecutionReceiptV1::close(composition, process).expect("reader receipt")
    }

    #[test]
    fn reader_must_close_before_benchmark_join_and_judge_must_consume_join() {
        let reader = fixture_reader();
        let judge_request_bytes = b"fixture-judge-request";
        let join = P8RealBenchmarkJoinExecutionReceiptV1::after_reader(
            &reader,
            digest("dataset member"),
            digest("rubric gold"),
            judge_request_bytes,
        )
        .expect("post-reader join");
        let trial_key = P8QualityTrialKeyV1 {
            question_id: reader.composition.reader_key.question_id.clone(),
            arm: reader.composition.reader_key.arm,
            reader_repeat_index: 0,
            judge_repeat_index: 0,
        };
        let judge_request = join.judge_input_digest.clone();
        let process = P8ClosedModelProcessReceiptV1::close(
            P8ModelProcessRoleV1::Judge,
            digest("judge executable"),
            digest("provider"),
            digest("judge model"),
            vec![P8ObservedModelAttemptV1::fixture(
                0,
                judge_request,
                observed_model_request_bytes_digest(judge_request_bytes),
                u64::try_from(judge_request_bytes.len()).expect("judge request"),
                "judge response",
                0,
            )],
        )
        .expect("judge process");

        P8JudgeExecutionReceiptV1::close_after_join(trial_key, &join, process)
            .expect("judge after join");
    }

    #[test]
    fn parent_observed_retry_exit_and_double_eof_are_exact() {
        let reader = fixture_reader();
        let mut process = reader.process.clone();
        process.attempts[0].stderr_eof = false;
        assert_eq!(
            process.validate_contract(),
            Err(P8QualityContractFailure::PipeClosureMissing)
        );

        let mut process = reader.process;
        process.attempts[0].exit_code = 9;
        assert_eq!(
            process.validate_contract(),
            Err(P8QualityContractFailure::PipeClosureMissing)
        );
    }

    #[test]
    fn join_rejects_cross_question_reader_receipt() {
        let reader = fixture_reader();
        let mut join = P8RealBenchmarkJoinExecutionReceiptV1::after_reader(
            &reader,
            digest("dataset member"),
            digest("rubric gold"),
            b"fixture-cross-question-judge-request",
        )
        .expect("join");
        join.reader_key.question_id = P8QualityId::parse("q-2").expect("question id");
        assert_eq!(
            join.validate_against_reader(&reader),
            Err(P8QualityContractFailure::JoinOrderMismatch)
        );
    }

    #[test]
    fn repo_owned_fixture_reader_and_judge_are_parent_observed_real_processes() {
        let model = P8FixtureModelBinary::compile().expect("compile fixture model");
        let request_bytes = b"zero-origin synthetic current-token request";
        let request_byte_count = u64::try_from(request_bytes.len()).expect("request length");
        let reader_key = P8ReaderTrialKeyV1 {
            question_id: P8QualityId::parse("q-synthetic-current-token").expect("question id"),
            arm: P8QualityArmKind::NoMemory,
            reader_repeat_index: 0,
        };
        let composition = P8ProviderRequestCompositionReceiptV1::build_fixture(
            P8QualityRunRef::derive(&"real-fixture-process-run"),
            reader_key.clone(),
            P8ArmReleaseRef::derive_for_test("real-fixture-no-memory-arm"),
            P8CompositionInputV1::NoMemoryEmpty,
            observed_model_request_bytes_digest(b"current-token-question"),
            observed_model_request_bytes_digest(b"tiny-reader-template"),
            observed_model_request_bytes_digest(b"no-tools"),
            observed_model_request_bytes_digest(request_bytes),
            request_byte_count,
            0,
            request_byte_count,
        )
        .expect("actual fixture composition");
        let (reader_process, reader_response) = execute_fixture_model_process(
            &model.executable,
            &["reader", "deterministic", "amber"],
            P8ModelProcessRoleV1::Reader,
            composition.request_digest.clone(),
            request_bytes,
        )
        .expect("execute actual fixture reader");
        assert_eq!(reader_response, b"amber");
        let reader = P8ReaderExecutionReceiptV1::close(composition, reader_process)
            .expect("close actual reader receipt");
        let join = P8RealBenchmarkJoinExecutionReceiptV1::after_reader(
            &reader,
            digest("actual dataset member"),
            digest("actual rubric gold"),
            &reader_response,
        )
        .expect("join actual reader response after close");
        let (judge_process, judge_response) = execute_fixture_model_process(
            &model.executable,
            &["judge", "exact", "amber"],
            P8ModelProcessRoleV1::Judge,
            join.judge_input_digest.clone(),
            &reader_response,
        )
        .expect("execute actual fixture judge");
        assert_eq!(judge_response, b"correct");
        let judge = P8JudgeExecutionReceiptV1::close_after_join(
            P8QualityTrialKeyV1 {
                question_id: reader_key.question_id,
                arm: reader_key.arm,
                reader_repeat_index: reader_key.reader_repeat_index,
                judge_repeat_index: 0,
            },
            &join,
            judge_process,
        )
        .expect("close actual judge receipt");

        reader.validate_contract().expect("actual reader contract");
        judge
            .validate_against_join(&join)
            .expect("actual judge contract");
        let receipt_json = serde_json::to_string(&(reader, join, judge)).expect("receipt JSON");
        for raw in [
            "amber",
            "zero-origin synthetic current-token request",
            "tiny-reader-template",
        ] {
            assert!(!receipt_json.contains(raw), "receipt persisted raw {raw}");
        }
    }

    #[test]
    fn live_sdk_negative_only_proof_closes_without_reader_judge_or_payload() {
        let dataset = crate::p8_quality::execution_plan::admit_zero_origin_tiny_dataset_manifest(
            include_bytes!("../../fixtures/p8-quality-tiny/manifest.json"),
        )
        .expect("tiny dataset");
        let experiment = crate::p8_quality::tests::fixture_plan_for_zero_origin_dataset(
            crate::p8_quality::P8QualityPurpose::BaselineEstablishment,
            &dataset,
        );
        let generation =
            crate::p8_quality::execution_plan::P8SupervisorOwnedRunGeneration::mint_for_supervisor(
                digest("negative supervisor session"),
                1,
                [19; 32],
            )
            .expect("supervisor generation");
        let execution_plan = P8QualityExecutionPlanV1::derive(&experiment, &dataset, generation)
            .expect("execution plan");
        let work_item = execution_plan
            .work_items()
            .iter()
            .find(|item| {
                matches!(
                    item.kind(),
                    P8MechanicalWorkKindV1::SafetyNegativeProof { .. }
                )
            })
            .expect("negative work item");

        let profile = bm_sdk::ProfileId::native_dev_full().expect("native profile");
        let store = bm_sdk::MemoryStoreHandle::open(
            bm_sdk::StoreBackendConfig::in_memory(profile).expect("store config"),
        )
        .expect("store");
        let runtime = bm_sdk::MemoryRuntime::builder()
            .identity(
                bm_sdk::MemoryIdentity::new("p8-fixture", "negative-owner").expect("identity"),
            )
            .scope(bm_sdk::MemoryScope::new("p8-fixture", "negative-chat").expect("scope"))
            .store(store)
            .build()
            .expect("runtime");
        let execution = runtime
            .p8_semantic_closure_execution_v2(bm_sdk::P8SemanticOffRunRequest::new(
                bm_sdk::MemoryRecallRequest {
                    query: "zero-origin synthetic negative proof".into(),
                    limit: 4,
                    structured_query_facets: Vec::new(),
                    tool_registry_refs: Vec::new(),
                    temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                },
            ))
            .expect("live SDK closure");
        let receipt = P8CompletedNegativeOnlyProofReceiptV1::close_from_live_sdk(
            &execution_plan,
            work_item,
            &execution,
        )
        .expect("negative-only receipt");
        receipt
            .validate_against_work_item(&execution_plan, work_item)
            .expect("negative-only contract");
        assert_eq!(receipt.provider_payload_count, 0);
        assert_eq!(
            receipt.model_boundary,
            crate::p8_quality::P8NegativeProofModelBoundaryV1::NoReaderModelJudgeOrAccuracy
        );

        let bytes = serde_json::to_vec(&receipt).expect("negative receipt JSON");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("negative receipt");
        let object = value.as_object().expect("negative receipt object");
        for forbidden_field in ["reader", "judge", "accuracy", "model_request"] {
            assert!(!object.contains_key(forbidden_field));
        }
        for forbidden in ["private", "soul", "credential", "absolute_path"] {
            assert!(
                !String::from_utf8_lossy(&bytes).contains(forbidden),
                "negative-only receipt leaked {forbidden}"
            );
        }
    }

    #[test]
    fn live_sdk_same_closure_ablation_runs_real_reader_judges_and_pair() {
        let model = P8FixtureModelBinary::compile().expect("compile fixture model");
        let dataset = crate::p8_quality::execution_plan::admit_zero_origin_tiny_dataset_manifest(
            include_bytes!("../../fixtures/p8-quality-tiny/manifest.json"),
        )
        .expect("tiny dataset");
        let experiment = crate::p8_quality::tests::fixture_plan_for_zero_origin_dataset(
            crate::p8_quality::P8QualityPurpose::BaselineEstablishment,
            &dataset,
        );
        let generation =
            crate::p8_quality::execution_plan::P8SupervisorOwnedRunGeneration::mint_for_supervisor(
                digest("ablation supervisor session"),
                1,
                [29; 32],
            )
            .expect("supervisor generation");
        let plan = P8QualityExecutionPlanV1::derive(&experiment, &dataset, generation)
            .expect("execution plan");
        let ablation_work = plan
            .work_items()
            .iter()
            .find(|item| {
                matches!(
                    item.kind(),
                    P8MechanicalWorkKindV1::SameClosureAblation {
                        counterfactual:
                            crate::p8_quality::P8SameClosureSafeCounterfactualKindV1::TemporalValidity
                    }
                ) && item.reader_repeat_index() == Some(0)
                    && item.judge_repeat_index() == Some(0)
            })
            .expect("temporal ablation work");
        let main_work = plan
            .work_items()
            .iter()
            .find(|item| {
                item.kind() == P8MechanicalWorkKindV1::Main
                    && item.question_id() == ablation_work.question_id()
                    && item.arm() == ablation_work.arm()
                    && item.reader_repeat_index() == ablation_work.reader_repeat_index()
                    && item.judge_repeat_index() == ablation_work.judge_repeat_index()
            })
            .expect("paired main work");

        let profile = bm_sdk::ProfileId::native_dev_full().expect("native profile");
        let store = bm_sdk::MemoryStoreHandle::open(
            bm_sdk::StoreBackendConfig::in_memory(profile).expect("store config"),
        )
        .expect("store");
        let runtime = bm_sdk::MemoryRuntime::builder()
            .identity(
                bm_sdk::MemoryIdentity::new("p8-fixture", "ablation-owner").expect("identity"),
            )
            .scope(bm_sdk::MemoryScope::new("p8-fixture", "ablation-chat").expect("scope"))
            .store(store)
            .build()
            .expect("runtime");
        let mut execution = runtime
            .p8_semantic_closure_execution_v2(bm_sdk::P8SemanticOffRunRequest::new(
                bm_sdk::MemoryRecallRequest {
                    query: "zero-origin synthetic ablation".into(),
                    limit: 4,
                    structured_query_facets: Vec::new(),
                    tool_registry_refs: Vec::new(),
                    temporal_operation: bm_sdk::MemoryRecallTemporalOperation::Current,
                },
            ))
            .expect("live SDK closure");

        let tool_digest = digest("actual fixture tool schema");
        let (baseline_semantic, (baseline_composition, baseline_process, baseline_response)) =
            with_sdk_beetle_provider_request(
                &plan,
                main_work,
                &mut execution,
                None,
                "Return the zero-origin token.",
                "tiny reader prompt",
                tool_digest.clone(),
                |request, composition, _| {
                    let (process, response) = execute_fixture_model_process(
                        &model.executable,
                        &["reader", "deterministic", "amber"],
                        P8ModelProcessRoleV1::Reader,
                        composition.request_digest.clone(),
                        request.as_bytes(),
                    )
                    .map_err(|_| P8QualityContractFailure::ReceiptChainMismatch)?;
                    Ok((composition.clone(), process, response))
                },
            )
            .expect("baseline provider request and reader");
        let baseline_reader =
            P8ReaderExecutionReceiptV1::close(baseline_composition, baseline_process)
                .expect("baseline reader");
        let baseline_join = P8RealBenchmarkJoinExecutionReceiptV1::after_reader(
            &baseline_reader,
            digest("ablation dataset membership"),
            digest("ablation rubric gold"),
            &baseline_response,
        )
        .expect("baseline join");
        let (baseline_judge_process, baseline_judge_output) = execute_fixture_model_process(
            &model.executable,
            &["judge", "exact", "amber"],
            P8ModelProcessRoleV1::Judge,
            baseline_join.judge_input_digest.clone(),
            &baseline_response,
        )
        .expect("baseline judge process");
        assert_eq!(baseline_judge_output, b"correct");
        let baseline_trial_key = P8QualityTrialKeyV1 {
            question_id: main_work.question_id().clone(),
            arm: main_work.arm(),
            reader_repeat_index: 0,
            judge_repeat_index: 0,
        };
        let baseline_judge = P8JudgeExecutionReceiptV1::close_after_join(
            baseline_trial_key,
            &baseline_join,
            baseline_judge_process,
        )
        .expect("baseline judge");
        let baseline_main = P8CompletedMainTrialReceiptV1::close_for_work_item(
            &plan,
            main_work,
            baseline_reader,
            baseline_join,
            baseline_judge,
            P8AccuracyOutcomeV1::Correct,
            crate::p8_quality::P8ExpectedCapabilityOutcomesV1::current_procedural().into_actual(),
        )
        .expect("baseline main receipt");

        let counterfactual = match ablation_work.kind() {
            P8MechanicalWorkKindV1::SameClosureAblation { counterfactual } => counterfactual,
            _ => unreachable!(),
        };
        let (off_semantic, (off_composition, off_process, off_response)) =
            with_sdk_beetle_provider_request(
                &plan,
                ablation_work,
                &mut execution,
                Some(counterfactual),
                "Return the zero-origin token.",
                "tiny reader prompt",
                tool_digest,
                |request, composition, _| {
                    let (process, response) = execute_fixture_model_process(
                        &model.executable,
                        &["reader", "deterministic", "amber"],
                        P8ModelProcessRoleV1::Reader,
                        composition.request_digest.clone(),
                        request.as_bytes(),
                    )
                    .map_err(|_| P8QualityContractFailure::ReceiptChainMismatch)?;
                    Ok((composition.clone(), process, response))
                },
            )
            .expect("off-run provider request and reader");
        let off_reader = P8ReaderExecutionReceiptV1::close(off_composition, off_process)
            .expect("off-run reader");
        let off_join = P8RealBenchmarkJoinExecutionReceiptV1::after_reader(
            &off_reader,
            digest("ablation dataset membership"),
            digest("ablation rubric gold"),
            &off_response,
        )
        .expect("off-run join");
        let (off_judge_process, off_judge_output) = execute_fixture_model_process(
            &model.executable,
            &["judge", "exact", "amber"],
            P8ModelProcessRoleV1::Judge,
            off_join.judge_input_digest.clone(),
            &off_response,
        )
        .expect("off-run judge process");
        assert_eq!(off_judge_output, b"correct");
        let off_judge = P8JudgeExecutionReceiptV1::close_after_join(
            P8QualityTrialKeyV1 {
                question_id: ablation_work.question_id().clone(),
                arm: ablation_work.arm(),
                reader_repeat_index: 0,
                judge_repeat_index: 0,
            },
            &off_join,
            off_judge_process,
        )
        .expect("off-run judge");
        let mut pair_request = baseline_response.clone();
        pair_request.push(b'|');
        pair_request.extend_from_slice(&off_response);
        let pair_request_digest = P8PairedJudgeExecutionReceiptV1::request_digest(
            &baseline_main.judge,
            &off_judge,
            &pair_request,
        )
        .expect("paired request digest");
        let (pair_process, pair_output) = execute_fixture_model_process(
            &model.executable,
            &["judge", "exact", "amber|amber"],
            P8ModelProcessRoleV1::PairedJudge,
            pair_request_digest,
            &pair_request,
        )
        .expect("paired judge process");
        assert_eq!(pair_output, b"correct");
        let paired = P8PairedJudgeExecutionReceiptV1::close(
            &baseline_main.judge,
            &off_judge,
            &pair_request,
            pair_process,
            P8PairedJudgeOutcomeV1::Equivalent,
        )
        .expect("paired judge receipt");
        let receipt = P8CompletedAblationTrialReceiptV1::close_for_work_item(
            &plan,
            ablation_work,
            &baseline_main,
            baseline_semantic,
            off_semantic,
            off_reader,
            off_join,
            off_judge,
            paired,
            P8AccuracyOutcomeV1::Correct,
            crate::p8_quality::P8ExpectedCapabilityOutcomesV1::current_procedural().into_actual(),
        )
        .expect("completed same-closure ablation");
        receipt
            .validate_against_work_item(&plan, ablation_work, &baseline_main)
            .expect("ablation contract");
        assert_eq!(
            receipt.baseline_semantic.sdk_closure_receipt_digest,
            receipt.off_run_semantic.sdk_closure_receipt_digest
        );
        assert_ne!(
            receipt.baseline_semantic.selected_projection_digest,
            receipt.off_run_semantic.selected_projection_digest
        );
    }
}
