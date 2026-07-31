//! P8 semantic producer artifact contracts.
//!
//! This module owns typed v1 artifacts and producer-side verified-shard folding. It never owns
//! production recall decisions: those arrive only as the SDK safe off-run report.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use bm_core::feature_gate::ProfileId;
use bm_sdk::{
    P8SemanticOffRunKey, P8SemanticOffRunReport, P8SemanticOffRunReportDigest,
    P8SemanticSafeCandidateBinding,
};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

pub const P8_SEMANTIC_PRODUCER_IDENTITY_SCHEMA: &str =
    "beetle-memory.p8.semantic-producer-identity.v1";
pub const P8_SEMANTIC_RUN_PLAN_SCHEMA: &str = "beetle-memory.p8.semantic-run-plan.v1";
pub const P8_SEMANTIC_QUESTION_DETAIL_SCHEMA: &str = "beetle-memory.p8.semantic-question-detail.v1";
pub const P8_SEMANTIC_SHARD_MANIFEST_SCHEMA: &str = "beetle-memory.p8.semantic-shard-manifest.v1";
pub const P8_SEMANTIC_BENCHMARK_SUMMARY_SCHEMA: &str =
    "beetle-memory.p8.semantic-benchmark-summary.v1";
pub const P8_SEMANTIC_VERIFIER_IDENTITY_SCHEMA: &str =
    "beetle-memory.p8.semantic-verifier-identity.v1";
pub const P8_SEMANTIC_GATE_COMMAND_RECEIPT_SCHEMA: &str =
    "beetle-memory.p8.semantic-gate-command-receipt.v1";
pub const P8_SEMANTIC_VERIFICATION_RECEIPT_SCHEMA: &str =
    "beetle-memory.p8.semantic-verification-receipt.v1";
pub const P8_SEMANTIC_OPERATOR_REPORT_SCHEMA: &str = "beetle-memory.p8.semantic-operator-report.v1";
pub(crate) const P8_SEMANTIC_GATE_SELF_TEST_ARG: &str = "--gate-self-test";
pub(crate) const P8_SEMANTIC_GATE_SELF_TEST_NAME: &str = "p8-semantic-operator-self-test-v1";
pub(crate) const P8_SEMANTIC_GATE_EXPECTED_STDOUT: &[u8] = b"running 1 test\n\
p8-semantic-operator-self-test-v1 ... ok\n\
test result: ok. 1 passed; 0 failed\n";

const SHA256_PREFIX: &str = "sha256:";
const P8_BUILD_SOURCE_ATTESTATION: &str = env!("BM_P8_BUILD_SOURCE_ATTESTATION");
const P8_WORKSPACE_BUILD_SOURCE_ATTESTATION: &str = "workspace_source";
const P8_VERIFIER_SOURCE_FINGERPRINT: &str = env!("BM_P8_VERIFIER_SOURCE_FINGERPRINT");

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct P8Sha256Digest(String);

impl P8Sha256Digest {
    pub fn parse(value: impl Into<String>) -> Result<Self, P8ArtifactContractFailure> {
        let value = value.into();
        if is_sha256(&value) {
            Ok(Self(value))
        } else {
            Err(P8ArtifactContractFailure::DigestInvalid)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn derive(domain: &str, value: &impl Serialize) -> Self {
        let bytes = serde_json::to_vec(value)
            .expect("P8 canonical identity serialization must be infallible");
        Self(format!(
            "{SHA256_PREFIX}{}",
            domain_separated_sha256(domain, &[bytes.as_slice()])
        ))
    }

    pub(crate) fn derive_bytes(domain: &str, bytes: &[u8]) -> Self {
        Self(format!(
            "{SHA256_PREFIX}{}",
            domain_separated_sha256(domain, &[bytes])
        ))
    }
}

impl<'de> Deserialize<'de> for P8Sha256Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if is_sha256(&value) {
            Ok(Self(value))
        } else {
            Err(D::Error::custom("invalid lower-case sha256 digest"))
        }
    }
}

macro_rules! p8_domain_ref {
    ($name:ident, $prefix:literal, $domain:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        #[allow(dead_code)]
        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub(crate) fn derive(value: &impl Serialize) -> Self {
                let bytes = serde_json::to_vec(value)
                    .expect("P8 typed identity serialization must be infallible");
                Self(format!(
                    "{}{}",
                    $prefix,
                    domain_separated_sha256($domain, &[bytes.as_slice()])
                ))
            }

            pub(crate) fn derive_bytes(bytes: &[u8]) -> Self {
                Self(format!(
                    "{}{}",
                    $prefix,
                    domain_separated_sha256($domain, &[bytes])
                ))
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                if has_typed_sha256_prefix(&value, $prefix) {
                    Ok(Self(value))
                } else {
                    Err(D::Error::custom(concat!(
                        stringify!($name),
                        " has an invalid domain or digest"
                    )))
                }
            }
        }
    };
}

p8_domain_ref!(
    P8VerifierSourceIdentityRef,
    "p8_verifier_source_identity:sha256:",
    "p8_verifier_source_identity_v1"
);
p8_domain_ref!(
    P8VerifierBuildIdentityRef,
    "p8_verifier_build_identity:sha256:",
    "p8_verifier_build_identity_v1"
);
p8_domain_ref!(
    P8VerifierExecutableIdentityRef,
    "p8_verifier_executable_identity:sha256:",
    "p8_verifier_executable_identity_v1"
);
p8_domain_ref!(
    P8VerifierIdentityRef,
    "p8_verifier_identity:sha256:",
    "p8_verifier_identity_v1"
);
p8_domain_ref!(P8GateArgvRef, "p8_gate_argv:sha256:", "p8_gate_argv_v1");
p8_domain_ref!(
    P8GateCwdSourceIdentityRef,
    "p8_gate_cwd_source_identity:sha256:",
    "p8_gate_cwd_source_identity_v1"
);
p8_domain_ref!(
    P8GateBuildProfileIdentityRef,
    "p8_gate_build_profile_identity:sha256:",
    "p8_gate_build_profile_identity_v1"
);
p8_domain_ref!(
    P8ClosedStdoutRef,
    "p8_closed_stdout:sha256:",
    "p8_gate_closed_stdout_v1"
);
p8_domain_ref!(
    P8ClosedStderrRef,
    "p8_closed_stderr:sha256:",
    "p8_gate_closed_stderr_v1"
);
p8_domain_ref!(
    P8GateCommandReceiptRef,
    "p8_gate_command_receipt:sha256:",
    "p8_gate_command_receipt_v1"
);
p8_domain_ref!(
    P8VerificationReceiptRef,
    "p8_verification_receipt:sha256:",
    "p8_verification_receipt_v1"
);
p8_domain_ref!(
    P8SemanticOperatorReportRef,
    "p8_semantic_operator_report:sha256:",
    "p8_semantic_operator_report_v1"
);
p8_domain_ref!(
    P8ReaderReceiptRef,
    "p8_reader_receipt:sha256:",
    "p8_reader_receipt_v1"
);
p8_domain_ref!(
    P8JudgeReceiptRef,
    "p8_judge_receipt:sha256:",
    "p8_judge_receipt_v1"
);
p8_domain_ref!(
    P8BenchmarkJoinReceiptRef,
    "p8_benchmark_join_receipt:sha256:",
    "p8_benchmark_join_receipt_v1"
);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct P8ArtifactId(String);

impl P8ArtifactId {
    pub fn parse(value: impl Into<String>) -> Result<Self, P8ArtifactContractFailure> {
        let value = value.into();
        if is_canonical_id(&value) {
            Ok(Self(value))
        } else {
            Err(P8ArtifactContractFailure::IdentityInvalid)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for P8ArtifactId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if is_canonical_id(&value) {
            Ok(Self(value))
        } else {
            Err(D::Error::custom("invalid P8 typed artifact id"))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8BenchmarkFamily {
    LongMemEvalV2,
    MemoraStyle,
    TemporalIncremental,
    BeetleInternal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8LongMemEvalDomain {
    Web,
    Enterprise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8DatasetScale {
    Small,
    Medium,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8MemoryCondition {
    Active,
    Obsolete,
    Invalidated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8TemporalAbstraction {
    Specific,
    Abstract,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8InternalSuite {
    Safety,
    Resource,
    Procedural,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "family", rename_all = "snake_case", deny_unknown_fields)]
pub enum P8DatasetStratum {
    LongMemEvalV2 {
        domain: P8LongMemEvalDomain,
        scale: P8DatasetScale,
    },
    MemoraStyle {
        memory_condition: P8MemoryCondition,
    },
    TemporalIncremental {
        abstraction: P8TemporalAbstraction,
    },
    BeetleInternal {
        suite: P8InternalSuite,
    },
}

impl P8DatasetStratum {
    pub const fn family(&self) -> P8BenchmarkFamily {
        match self {
            Self::LongMemEvalV2 { .. } => P8BenchmarkFamily::LongMemEvalV2,
            Self::MemoraStyle { .. } => P8BenchmarkFamily::MemoraStyle,
            Self::TemporalIncremental { .. } => P8BenchmarkFamily::TemporalIncremental,
            Self::BeetleInternal { .. } => P8BenchmarkFamily::BeetleInternal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8CapabilitySlice {
    StaticState,
    DynamicState,
    WorkflowKnowledge,
    EnvironmentGotcha,
    PremiseAwareness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8TaskKind {
    Recall,
    Remember,
    Reason,
    Recommend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8QueryOperationKind {
    CurrentQuery,
    AsOfQuery,
    ProcedureEvidence,
    PremiseDecision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8TemporalCorpusSlice {
    NotApplicable,
    BaseQueryBaseCorpus,
    BaseQueryUpdatedCorpus,
    NewQueryUpdatedCorpus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8SafetySlice {
    Privacy,
    CrossSubject,
    Soul,
    ProfileBudget,
    NoFullScan,
    Invalidation,
    Forgetting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8AccuracyDecision {
    Correct,
    Incorrect,
    ExpectedRefusal,
    NotApplicable,
}

impl P8AccuracyDecision {
    pub(crate) const fn is_correct(self) -> bool {
        matches!(self, Self::Correct | Self::ExpectedRefusal)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8MemoryUseDecision {
    CurrentUsed,
    ObsoleteRejected,
    InvalidatedRejected,
    ObsoleteUsed,
    InvalidatedUsed,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8SemanticFailure {
    ProductionReportRejected,
    ReaderRejected,
    JudgeRejected,
    ResourceLimitExceeded,
    SafetyViolation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8ArtifactContractFailure {
    SchemaMismatch,
    IdentityInvalid,
    DigestInvalid,
    ProducerIdentityMismatch,
    RunPlanMismatch,
    ShardIndexMismatch,
    ShardCoverageMismatch,
    QuestionCoverageMismatch,
    DetailDigestMismatch,
    DetailBytesMismatch,
    DetailRowsMismatch,
    DuplicateQuestion,
    DuplicateArtifact,
    PhysicalIdentityMismatch,
    ReadPassMismatch,
    ArtifactLimitExceeded,
    OperatorWallTimeExceeded,
    ArithmeticOverflow,
    SdkReportInvalid,
    SdkReportDigestMismatch,
    AblationSetMismatch,
    ReceiptInvalid,
    ArtifactIoFailure,
    SummaryMismatch,
    QualityThresholdsNotFrozen,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticProducerIdentityV1 {
    schema: String,
    source_identity: P8Sha256Digest,
    sdk_identity: P8Sha256Digest,
    runner_identity: P8Sha256Digest,
    lock_identity: P8Sha256Digest,
    identity_digest: P8Sha256Digest,
}

impl P8SemanticProducerIdentityV1 {
    pub(crate) fn build(
        source_identity: P8Sha256Digest,
        sdk_identity: P8Sha256Digest,
        runner_identity: P8Sha256Digest,
        lock_identity: P8Sha256Digest,
    ) -> Self {
        let mut value = Self {
            schema: P8_SEMANTIC_PRODUCER_IDENTITY_SCHEMA.into(),
            source_identity,
            sdk_identity,
            runner_identity,
            lock_identity,
            identity_digest: P8Sha256Digest::derive("p8_producer_identity_v1", &()),
        };
        value.identity_digest = value.derived_digest();
        value
    }

    pub fn identity_digest(&self) -> &P8Sha256Digest {
        &self.identity_digest
    }

    pub fn validate_contract(&self) -> Vec<P8ArtifactContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_SEMANTIC_PRODUCER_IDENTITY_SCHEMA {
            failures.push(P8ArtifactContractFailure::SchemaMismatch);
        }
        if self.identity_digest != self.derived_digest() {
            failures.push(P8ArtifactContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn derived_digest(&self) -> P8Sha256Digest {
        P8Sha256Digest::derive(
            "p8_producer_identity_v1",
            &(
                &self.schema,
                &self.source_identity,
                &self.sdk_identity,
                &self.runner_identity,
                &self.lock_identity,
            ),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticQuestionPlanV1 {
    pub(crate) question_id: P8ArtifactId,
    pub(crate) question_manifest_digest: P8Sha256Digest,
    pub(crate) shard_index: u32,
}

impl P8SemanticQuestionPlanV1 {
    pub(crate) fn new(
        question_id: P8ArtifactId,
        question_manifest_digest: P8Sha256Digest,
        shard_index: u32,
    ) -> Self {
        Self {
            question_id,
            question_manifest_digest,
            shard_index,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticRunPlanV1 {
    pub(crate) schema: String,
    pub(crate) run_id: P8ArtifactId,
    pub(crate) producer_identity_digest: P8Sha256Digest,
    pub(crate) dataset_manifest_digest: P8Sha256Digest,
    pub(crate) shard_total: u32,
    pub(crate) ordered_questions: Vec<P8SemanticQuestionPlanV1>,
    pub(crate) run_plan_digest: P8Sha256Digest,
}

impl P8SemanticRunPlanV1 {
    pub(crate) fn build(
        run_id: P8ArtifactId,
        producer_identity_digest: P8Sha256Digest,
        dataset_manifest_digest: P8Sha256Digest,
        shard_total: u32,
        ordered_questions: Vec<P8SemanticQuestionPlanV1>,
    ) -> Result<Self, Vec<P8ArtifactContractFailure>> {
        let mut value = Self {
            schema: P8_SEMANTIC_RUN_PLAN_SCHEMA.into(),
            run_id,
            producer_identity_digest,
            dataset_manifest_digest,
            shard_total,
            ordered_questions,
            run_plan_digest: P8Sha256Digest::derive("p8_run_plan_v1", &()),
        };
        value.run_plan_digest = value.derived_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub fn run_plan_digest(&self) -> &P8Sha256Digest {
        &self.run_plan_digest
    }

    pub fn validate_contract(&self) -> Vec<P8ArtifactContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_SEMANTIC_RUN_PLAN_SCHEMA {
            failures.push(P8ArtifactContractFailure::SchemaMismatch);
        }
        if self.shard_total == 0
            || self.ordered_questions.is_empty()
            || self
                .ordered_questions
                .iter()
                .any(|question| question.shard_index >= self.shard_total)
            || self
                .ordered_questions
                .iter()
                .map(|question| &question.question_id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.ordered_questions.len()
        {
            failures.push(P8ArtifactContractFailure::QuestionCoverageMismatch);
        }
        if self.run_plan_digest != self.derived_digest() {
            failures.push(P8ArtifactContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn derived_digest(&self) -> P8Sha256Digest {
        P8Sha256Digest::derive(
            "p8_run_plan_v1",
            &(
                &self.schema,
                &self.run_id,
                &self.producer_identity_digest,
                &self.dataset_manifest_digest,
                self.shard_total,
                &self.ordered_questions,
            ),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8ReaderReceiptV1 {
    question_id: P8ArtifactId,
    reader_identity: P8Sha256Digest,
    answer_digest: P8Sha256Digest,
    receipt_digest: P8ReaderReceiptRef,
}

impl P8ReaderReceiptV1 {
    pub(crate) fn build(
        question_id: P8ArtifactId,
        reader_identity: P8Sha256Digest,
        answer_digest: P8Sha256Digest,
    ) -> Self {
        let receipt_digest =
            P8ReaderReceiptRef::derive(&(&question_id, &reader_identity, &answer_digest));
        Self {
            question_id,
            reader_identity,
            answer_digest,
            receipt_digest,
        }
    }

    fn is_valid_for(&self, question_id: &P8ArtifactId) -> bool {
        &self.question_id == question_id
            && self.receipt_digest
                == P8ReaderReceiptRef::derive(&(
                    &self.question_id,
                    &self.reader_identity,
                    &self.answer_digest,
                ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8JudgeReceiptV1 {
    question_id: P8ArtifactId,
    judge_identity: P8Sha256Digest,
    decision: P8AccuracyDecision,
    receipt_digest: P8JudgeReceiptRef,
}

impl P8JudgeReceiptV1 {
    pub(crate) fn build(
        question_id: P8ArtifactId,
        judge_identity: P8Sha256Digest,
        decision: P8AccuracyDecision,
    ) -> Self {
        let receipt_digest = P8JudgeReceiptRef::derive(&(&question_id, &judge_identity, decision));
        Self {
            question_id,
            judge_identity,
            decision,
            receipt_digest,
        }
    }

    fn is_valid_for(&self, question_id: &P8ArtifactId, decision: P8AccuracyDecision) -> bool {
        &self.question_id == question_id
            && self.decision == decision
            && self.receipt_digest
                == P8JudgeReceiptRef::derive(&(
                    &self.question_id,
                    &self.judge_identity,
                    self.decision,
                ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8BenchmarkJoinReceiptV1 {
    question_id: P8ArtifactId,
    dataset_manifest_digest: P8Sha256Digest,
    question_manifest_digest: P8Sha256Digest,
    rubric_digest: P8Sha256Digest,
    receipt_digest: P8BenchmarkJoinReceiptRef,
}

impl P8BenchmarkJoinReceiptV1 {
    pub(crate) fn build(
        question_id: P8ArtifactId,
        dataset_manifest_digest: P8Sha256Digest,
        question_manifest_digest: P8Sha256Digest,
        rubric_digest: P8Sha256Digest,
    ) -> Self {
        let receipt_digest = P8BenchmarkJoinReceiptRef::derive(&(
            &question_id,
            &dataset_manifest_digest,
            &question_manifest_digest,
            &rubric_digest,
        ));
        Self {
            question_id,
            dataset_manifest_digest,
            question_manifest_digest,
            rubric_digest,
            receipt_digest,
        }
    }

    fn is_valid_for(
        &self,
        question_id: &P8ArtifactId,
        dataset_manifest_digest: &P8Sha256Digest,
        question_manifest_digest: &P8Sha256Digest,
    ) -> bool {
        &self.question_id == question_id
            && &self.dataset_manifest_digest == dataset_manifest_digest
            && &self.question_manifest_digest == question_manifest_digest
            && self.receipt_digest
                == P8BenchmarkJoinReceiptRef::derive(&(
                    &self.question_id,
                    &self.dataset_manifest_digest,
                    &self.question_manifest_digest,
                    &self.rubric_digest,
                ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8AblationEvaluationV1 {
    pub(crate) baseline_decision: P8AccuracyDecision,
    pub(crate) off_run_decision: P8AccuracyDecision,
}

impl P8AblationEvaluationV1 {
    pub(crate) const fn new(
        baseline_decision: P8AccuracyDecision,
        off_run_decision: P8AccuracyDecision,
    ) -> Self {
        Self {
            baseline_decision,
            off_run_decision,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8ResourceMeasurement {
    pub elapsed_millis: u64,
    pub peak_rss_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticQuestionDetailV1 {
    pub(crate) schema: String,
    pub(crate) producer_identity_digest: P8Sha256Digest,
    pub(crate) run_plan_digest: P8Sha256Digest,
    pub(crate) run_id: P8ArtifactId,
    pub(crate) shard_index: u32,
    pub(crate) shard_total: u32,
    pub(crate) question_id: P8ArtifactId,
    pub(crate) dataset_manifest_digest: P8Sha256Digest,
    pub(crate) question_manifest_digest: P8Sha256Digest,
    pub(crate) benchmark_family: P8BenchmarkFamily,
    pub(crate) dataset_stratum: P8DatasetStratum,
    pub(crate) capability_slices: Vec<P8CapabilitySlice>,
    pub(crate) task_kind: P8TaskKind,
    pub(crate) query_operation_kind: P8QueryOperationKind,
    pub(crate) temporal_corpus_slice: P8TemporalCorpusSlice,
    pub(crate) safety_slices: Vec<P8SafetySlice>,
    pub(crate) profile: ProfileId,
    pub(crate) capability_identity: P8Sha256Digest,
    pub(crate) budget_identity: P8Sha256Digest,
    pub(crate) sdk_off_run_report: P8SemanticOffRunReport,
    pub(crate) sdk_off_run_report_digest: P8SemanticOffRunReportDigest,
    pub(crate) baseline_candidate_bindings: Vec<P8SemanticSafeCandidateBinding>,
    pub(crate) reader_receipt: P8ReaderReceiptV1,
    pub(crate) judge_receipt: P8JudgeReceiptV1,
    pub(crate) benchmark_join_receipt: P8BenchmarkJoinReceiptV1,
    pub(crate) output_digest: P8Sha256Digest,
    pub(crate) accuracy_decision: P8AccuracyDecision,
    pub(crate) memory_use_decision: P8MemoryUseDecision,
    pub(crate) resource: P8ResourceMeasurement,
    pub(crate) ablation_evaluations: BTreeMap<P8SemanticOffRunKey, P8AblationEvaluationV1>,
    pub(crate) failures: Vec<P8SemanticFailure>,
}

#[derive(Clone, Debug)]
pub struct P8SemanticQuestionDetailInputV1 {
    pub producer_identity_digest: P8Sha256Digest,
    pub run_plan_digest: P8Sha256Digest,
    pub run_id: P8ArtifactId,
    pub shard_index: u32,
    pub shard_total: u32,
    pub question_id: P8ArtifactId,
    pub dataset_manifest_digest: P8Sha256Digest,
    pub question_manifest_digest: P8Sha256Digest,
    pub benchmark_family: P8BenchmarkFamily,
    pub dataset_stratum: P8DatasetStratum,
    pub capability_slices: Vec<P8CapabilitySlice>,
    pub task_kind: P8TaskKind,
    pub query_operation_kind: P8QueryOperationKind,
    pub temporal_corpus_slice: P8TemporalCorpusSlice,
    pub safety_slices: Vec<P8SafetySlice>,
    pub profile: ProfileId,
    pub capability_identity: P8Sha256Digest,
    pub budget_identity: P8Sha256Digest,
    pub sdk_off_run_report: P8SemanticOffRunReport,
    pub reader_receipt: P8ReaderReceiptV1,
    pub judge_receipt: P8JudgeReceiptV1,
    pub benchmark_join_receipt: P8BenchmarkJoinReceiptV1,
    pub output_digest: P8Sha256Digest,
    pub accuracy_decision: P8AccuracyDecision,
    pub memory_use_decision: P8MemoryUseDecision,
    pub resource: P8ResourceMeasurement,
    pub ablation_evaluations: BTreeMap<P8SemanticOffRunKey, P8AblationEvaluationV1>,
    pub failures: Vec<P8SemanticFailure>,
}

impl P8SemanticQuestionDetailV1 {
    pub(crate) fn build(
        input: P8SemanticQuestionDetailInputV1,
    ) -> Result<Self, Vec<P8ArtifactContractFailure>> {
        let sdk_off_run_report_digest = input.sdk_off_run_report.report_digest().clone();
        let baseline_candidate_bindings = input
            .sdk_off_run_report
            .baseline_candidate_bindings()
            .to_vec();
        let value = Self {
            schema: P8_SEMANTIC_QUESTION_DETAIL_SCHEMA.into(),
            producer_identity_digest: input.producer_identity_digest,
            run_plan_digest: input.run_plan_digest,
            run_id: input.run_id,
            shard_index: input.shard_index,
            shard_total: input.shard_total,
            question_id: input.question_id,
            dataset_manifest_digest: input.dataset_manifest_digest,
            question_manifest_digest: input.question_manifest_digest,
            benchmark_family: input.benchmark_family,
            dataset_stratum: input.dataset_stratum,
            capability_slices: input.capability_slices,
            task_kind: input.task_kind,
            query_operation_kind: input.query_operation_kind,
            temporal_corpus_slice: input.temporal_corpus_slice,
            safety_slices: input.safety_slices,
            profile: input.profile,
            capability_identity: input.capability_identity,
            budget_identity: input.budget_identity,
            sdk_off_run_report: input.sdk_off_run_report,
            sdk_off_run_report_digest,
            baseline_candidate_bindings,
            reader_receipt: input.reader_receipt,
            judge_receipt: input.judge_receipt,
            benchmark_join_receipt: input.benchmark_join_receipt,
            output_digest: input.output_digest,
            accuracy_decision: input.accuracy_decision,
            memory_use_decision: input.memory_use_decision,
            resource: input.resource,
            ablation_evaluations: input.ablation_evaluations,
            failures: input.failures,
        };
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub fn question_id(&self) -> &P8ArtifactId {
        &self.question_id
    }

    pub const fn shard_index(&self) -> u32 {
        self.shard_index
    }

    pub const fn shard_total(&self) -> u32 {
        self.shard_total
    }

    pub fn validate_contract(&self) -> Vec<P8ArtifactContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_SEMANTIC_QUESTION_DETAIL_SCHEMA {
            failures.push(P8ArtifactContractFailure::SchemaMismatch);
        }
        if self.shard_total == 0 || self.shard_index >= self.shard_total {
            failures.push(P8ArtifactContractFailure::ShardIndexMismatch);
        }
        if self.dataset_stratum.family() != self.benchmark_family
            || (self.benchmark_family == P8BenchmarkFamily::TemporalIncremental)
                != (self.temporal_corpus_slice != P8TemporalCorpusSlice::NotApplicable)
            || !is_strict_sorted_unique_nonempty(&self.capability_slices)
            || !is_strict_sorted_unique(&self.safety_slices)
        {
            failures.push(P8ArtifactContractFailure::QuestionCoverageMismatch);
        }
        if !self.sdk_off_run_report.validate_contract().is_empty() {
            failures.push(P8ArtifactContractFailure::SdkReportInvalid);
        }
        if self.sdk_off_run_report.report_digest() != &self.sdk_off_run_report_digest {
            failures.push(P8ArtifactContractFailure::SdkReportDigestMismatch);
        }
        if self
            .ablation_evaluations
            .keys()
            .copied()
            .collect::<Vec<_>>()
            != P8SemanticOffRunKey::ALL
        {
            failures.push(P8ArtifactContractFailure::AblationSetMismatch);
        }
        if !is_strict_sorted_unique(&self.baseline_candidate_bindings) {
            failures.push(P8ArtifactContractFailure::SdkReportInvalid);
        }
        if !self.reader_receipt.is_valid_for(&self.question_id)
            || !self
                .judge_receipt
                .is_valid_for(&self.question_id, self.accuracy_decision)
            || !self.benchmark_join_receipt.is_valid_for(
                &self.question_id,
                &self.dataset_manifest_digest,
                &self.question_manifest_digest,
            )
        {
            failures.push(P8ArtifactContractFailure::ReceiptInvalid);
        }
        failures
    }
}

pub(crate) fn p8_semantic_detail_digest(detail: &P8SemanticQuestionDetailV1) -> P8Sha256Digest {
    P8Sha256Digest::derive("p8_semantic_question_detail_v1", detail)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8ArtifactIdentityV1 {
    pub(crate) artifact_id: P8ArtifactId,
    pub(crate) content_digest: P8Sha256Digest,
    pub(crate) physical_identity: P8Sha256Digest,
    pub(crate) declared_bytes: u64,
    pub(crate) declared_rows: u64,
}

impl P8ArtifactIdentityV1 {
    pub(crate) fn build(
        artifact_id: P8ArtifactId,
        content_digest: P8Sha256Digest,
        physical_identity: P8Sha256Digest,
        declared_bytes: u64,
        declared_rows: u64,
    ) -> Self {
        Self {
            artifact_id,
            content_digest,
            physical_identity,
            declared_bytes,
            declared_rows,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticShardManifestV1 {
    pub(crate) schema: String,
    pub(crate) producer_identity_digest: P8Sha256Digest,
    pub(crate) run_plan_digest: P8Sha256Digest,
    pub(crate) run_id: P8ArtifactId,
    pub(crate) shard_index: u32,
    pub(crate) shard_total: u32,
    pub(crate) detail_artifact: P8ArtifactIdentityV1,
    pub(crate) ordered_question_ids: Vec<P8ArtifactId>,
    pub(crate) ordered_detail_digests: Vec<P8Sha256Digest>,
    pub(crate) detail_bytes: u64,
    pub(crate) detail_rows: u64,
    pub(crate) read_pass_count: u64,
}

impl P8SemanticShardManifestV1 {
    pub(crate) fn build(
        run_plan: &P8SemanticRunPlanV1,
        shard_index: u32,
        detail_artifact: P8ArtifactIdentityV1,
        details: &[P8SemanticQuestionDetailV1],
    ) -> Result<Self, Vec<P8ArtifactContractFailure>> {
        let value = Self {
            schema: P8_SEMANTIC_SHARD_MANIFEST_SCHEMA.into(),
            producer_identity_digest: run_plan.producer_identity_digest.clone(),
            run_plan_digest: run_plan.run_plan_digest.clone(),
            run_id: run_plan.run_id.clone(),
            shard_index,
            shard_total: run_plan.shard_total,
            detail_bytes: detail_artifact.declared_bytes,
            detail_rows: detail_artifact.declared_rows,
            detail_artifact,
            ordered_question_ids: details
                .iter()
                .map(|detail| detail.question_id.clone())
                .collect(),
            ordered_detail_digests: details.iter().map(p8_semantic_detail_digest).collect(),
            read_pass_count: 1,
        };
        let failures = value.validate_with_details(run_plan, details);
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub const fn shard_index(&self) -> u32 {
        self.shard_index
    }

    pub const fn shard_total(&self) -> u32 {
        self.shard_total
    }

    pub fn manifest_digest(&self) -> P8Sha256Digest {
        P8Sha256Digest::derive("p8_semantic_shard_manifest_v1", self)
    }

    pub fn validate_contract(&self) -> Vec<P8ArtifactContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_SEMANTIC_SHARD_MANIFEST_SCHEMA {
            failures.push(P8ArtifactContractFailure::SchemaMismatch);
        }
        if self.shard_total == 0 || self.shard_index >= self.shard_total {
            failures.push(P8ArtifactContractFailure::ShardIndexMismatch);
        }
        if self.ordered_question_ids.is_empty()
            || self.ordered_question_ids.len() != self.ordered_detail_digests.len()
            || self
                .ordered_question_ids
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.ordered_question_ids.len()
        {
            failures.push(P8ArtifactContractFailure::QuestionCoverageMismatch);
        }
        if self.detail_bytes != self.detail_artifact.declared_bytes {
            failures.push(P8ArtifactContractFailure::DetailBytesMismatch);
        }
        if self.detail_rows != self.detail_artifact.declared_rows {
            failures.push(P8ArtifactContractFailure::DetailRowsMismatch);
        }
        let expected_detail_artifact_id = format!("shard-{:05}.details.jsonl", self.shard_index);
        if self.detail_artifact.artifact_id.as_str() != expected_detail_artifact_id {
            failures.push(P8ArtifactContractFailure::IdentityInvalid);
        }
        if self.read_pass_count != 1 {
            failures.push(P8ArtifactContractFailure::ReadPassMismatch);
        }
        failures
    }

    pub fn validate_with_details(
        &self,
        run_plan: &P8SemanticRunPlanV1,
        details: &[P8SemanticQuestionDetailV1],
    ) -> Vec<P8ArtifactContractFailure> {
        let mut failures = self.validate_contract();
        if self.producer_identity_digest != run_plan.producer_identity_digest
            || self.run_plan_digest != run_plan.run_plan_digest
            || self.run_id != run_plan.run_id
            || self.shard_total != run_plan.shard_total
        {
            failures.push(P8ArtifactContractFailure::RunPlanMismatch);
        }
        let expected_questions = run_plan
            .ordered_questions
            .iter()
            .filter(|question| question.shard_index == self.shard_index)
            .map(|question| question.question_id.clone())
            .collect::<Vec<_>>();
        if expected_questions != self.ordered_question_ids
            || details
                .iter()
                .map(|detail| detail.question_id.clone())
                .collect::<Vec<_>>()
                != self.ordered_question_ids
        {
            failures.push(P8ArtifactContractFailure::QuestionCoverageMismatch);
        }
        if details.iter().any(|detail| {
            detail.producer_identity_digest != self.producer_identity_digest
                || detail.run_plan_digest != self.run_plan_digest
                || detail.run_id != self.run_id
                || detail.shard_index != self.shard_index
                || detail.shard_total != self.shard_total
                || !detail.validate_contract().is_empty()
        }) {
            failures.push(P8ArtifactContractFailure::RunPlanMismatch);
        }
        if details
            .iter()
            .map(p8_semantic_detail_digest)
            .collect::<Vec<_>>()
            != self.ordered_detail_digests
        {
            failures.push(P8ArtifactContractFailure::DetailDigestMismatch);
        }
        if u64::try_from(details.len()).ok() != Some(self.detail_rows) {
            failures.push(P8ArtifactContractFailure::DetailRowsMismatch);
        }
        failures.sort();
        failures.dedup();
        failures
    }
}

#[derive(Clone, Debug)]
pub(crate) struct P8VerifiedShardSet {
    manifests: Vec<P8SemanticShardManifestV1>,
    details: Vec<P8SemanticQuestionDetailV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticShardSubmissionV1 {
    manifest: P8SemanticShardManifestV1,
    details: Vec<P8SemanticQuestionDetailV1>,
    manifest_bytes: u64,
    detail_bytes: u64,
    manifest_physical_identity: P8Sha256Digest,
    detail_physical_identity: P8Sha256Digest,
    manifest_read_pass_count: u64,
    detail_read_pass_count: u64,
}

impl P8SemanticShardSubmissionV1 {
    pub(crate) fn from_single_read(
        manifest: P8SemanticShardManifestV1,
        details: Vec<P8SemanticQuestionDetailV1>,
        manifest_bytes: u64,
        detail_bytes: u64,
        manifest_physical_identity: P8Sha256Digest,
        detail_physical_identity: P8Sha256Digest,
    ) -> Self {
        Self {
            manifest,
            details,
            manifest_bytes,
            detail_bytes,
            manifest_physical_identity,
            detail_physical_identity,
            manifest_read_pass_count: 1,
            detail_read_pass_count: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "axis", content = "value", rename_all = "snake_case")]
pub enum P8AggregationSlice {
    BenchmarkFamily(P8BenchmarkFamily),
    DatasetStratum(P8DatasetStratum),
    Capability(P8CapabilitySlice),
    Task(P8TaskKind),
    QueryOperation(P8QueryOperationKind),
    TemporalCorpus(P8TemporalCorpusSlice),
    Safety(P8SafetySlice),
    Profile(ProfileId),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticMetricCounts {
    pub question_count: u64,
    pub correct_count: u64,
    pub current_use_count: u64,
    pub obsolete_rejected_count: u64,
    pub obsolete_used_count: u64,
    pub invalidated_rejected_count: u64,
    pub invalidated_used_count: u64,
    pub safety_failure_count: u64,
    pub elapsed_millis: u64,
    pub peak_rss_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticMetricSlice {
    pub slice: P8AggregationSlice,
    pub metrics: P8SemanticMetricCounts,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8AblationAggregate {
    pub applicable_count: u64,
    pub executed_count: u64,
    pub baseline_correct_count: u64,
    pub off_run_correct_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticBenchmarkSummaryV1 {
    pub(crate) schema: String,
    pub(crate) producer_identity_digest: P8Sha256Digest,
    pub(crate) run_plan_digest: P8Sha256Digest,
    pub(crate) run_id: P8ArtifactId,
    pub(crate) admitted_shard_manifest_digests: Vec<P8Sha256Digest>,
    pub(crate) ordered_detail_digests: Vec<P8Sha256Digest>,
    pub(crate) overall: P8SemanticMetricCounts,
    pub(crate) slices: Vec<P8SemanticMetricSlice>,
    pub(crate) ablation_deltas: BTreeMap<P8SemanticOffRunKey, P8AblationAggregate>,
    pub(crate) summary_digest: P8Sha256Digest,
}

impl P8SemanticBenchmarkSummaryV1 {
    pub fn overall(&self) -> &P8SemanticMetricCounts {
        &self.overall
    }

    pub fn slices(&self) -> &[P8SemanticMetricSlice] {
        &self.slices
    }

    pub fn ablation_deltas(&self) -> &BTreeMap<P8SemanticOffRunKey, P8AblationAggregate> {
        &self.ablation_deltas
    }

    pub fn summary_digest(&self) -> &P8Sha256Digest {
        &self.summary_digest
    }

    pub(crate) fn derived_digest(&self) -> P8Sha256Digest {
        P8Sha256Digest::derive(
            "p8_semantic_benchmark_summary_v1",
            &(
                &self.schema,
                &self.producer_identity_digest,
                &self.run_plan_digest,
                &self.run_id,
                &self.admitted_shard_manifest_digests,
                &self.ordered_detail_digests,
                &self.overall,
                &self.slices,
                &self.ablation_deltas,
            ),
        )
    }
}

pub(crate) fn produce_p8_semantic_summary(
    run_plan: &P8SemanticRunPlanV1,
    submissions: Vec<P8SemanticShardSubmissionV1>,
) -> Result<P8SemanticBenchmarkSummaryV1, Vec<P8ArtifactContractFailure>> {
    let verified = verify_shard_submissions(run_plan, submissions)?;
    producer_fold_verified_shards(run_plan, verified)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P8PublishedSemanticBundleV1 {
    root_identity: P8Sha256Digest,
    artifact_count: u64,
    total_bytes: u64,
    summary: P8SemanticBenchmarkSummaryV1,
}

impl P8PublishedSemanticBundleV1 {
    pub fn root_identity(&self) -> &P8Sha256Digest {
        &self.root_identity
    }

    pub const fn artifact_count(&self) -> u64 {
        self.artifact_count
    }

    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub fn summary(&self) -> &P8SemanticBenchmarkSummaryV1 {
        &self.summary
    }
}

pub(crate) fn publish_p8_semantic_bundle_no_clobber(
    root: &Path,
    producer_identity: &P8SemanticProducerIdentityV1,
    run_plan: &P8SemanticRunPlanV1,
    shard_details: Vec<Vec<P8SemanticQuestionDetailV1>>,
) -> Result<P8PublishedSemanticBundleV1, Vec<P8ArtifactContractFailure>> {
    let mut failures = producer_identity.validate_contract();
    failures.extend(run_plan.validate_contract());
    if producer_identity.identity_digest != run_plan.producer_identity_digest
        || shard_details.len() != usize::try_from(run_plan.shard_total).unwrap_or(usize::MAX)
    {
        failures.push(P8ArtifactContractFailure::RunPlanMismatch);
    }
    failures.sort();
    failures.dedup();
    if !failures.is_empty() {
        return Err(failures);
    }
    fs::create_dir(root).map_err(|_| vec![P8ArtifactContractFailure::DuplicateArtifact])?;
    let mut ledger = P8ArtifactAdmissionLedger::new(P8ArtifactLimits::V1);
    let mut artifact_count = 0_u64;
    let mut total_bytes = 0_u64;

    for (name, bytes) in [
        (
            "producer-identity.json",
            serialize_json_bounded(producer_identity, P8ArtifactLimits::V1.control_json_bytes())?,
        ),
        (
            "run-plan.json",
            serialize_json_bounded(run_plan, P8ArtifactLimits::V1.control_json_bytes())?,
        ),
    ] {
        let byte_count = u64::try_from(bytes.len())
            .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
        ledger
            .admit_declared(P8ArtifactAdmissionKind::ControlJson, byte_count)
            .map_err(|failure| vec![failure])?;
        write_new_file(&root.join(name), &bytes)?;
        checked_add_bundle_count(&mut artifact_count, 1)?;
        checked_add_bundle_count(&mut total_bytes, byte_count)?;
    }

    let mut submissions = Vec::with_capacity(shard_details.len());
    for (shard_index, details) in shard_details.into_iter().enumerate() {
        let shard_index = u32::try_from(shard_index)
            .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
        let mut detail_bytes = Vec::new();
        let mut shard_detail_byte_count = 0_u64;
        for detail in &details {
            let line_payload_limit = P8ArtifactLimits::V1
                .detail_line_bytes()
                .checked_sub(1)
                .ok_or_else(|| vec![P8ArtifactContractFailure::ArtifactLimitExceeded])?;
            let line = serialize_json_bounded(detail, line_payload_limit)?;
            let line_bytes = u64::try_from(line.len())
                .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
            let row_bytes = line_bytes
                .checked_add(1)
                .ok_or_else(|| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
            ledger
                .admit_declared(P8ArtifactAdmissionKind::DetailLine, row_bytes)
                .map_err(|failure| vec![failure])?;
            shard_detail_byte_count = shard_detail_byte_count
                .checked_add(row_bytes)
                .ok_or_else(|| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
            if shard_detail_byte_count > P8ArtifactLimits::V1.shard_detail_bytes() {
                return Err(vec![P8ArtifactContractFailure::ArtifactLimitExceeded]);
            }
            ledger
                .admit_declared(P8ArtifactAdmissionKind::ShardDetails, row_bytes)
                .map_err(|failure| vec![failure])?;
            detail_bytes.extend_from_slice(&line);
            detail_bytes.push(b'\n');
        }
        let detail_byte_count = u64::try_from(detail_bytes.len())
            .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
        if detail_byte_count != shard_detail_byte_count {
            return Err(vec![P8ArtifactContractFailure::DetailBytesMismatch]);
        }
        let detail_name = format!("shard-{shard_index:05}.details.jsonl");
        let detail_path = root.join(&detail_name);
        write_new_file(&detail_path, &detail_bytes)?;
        let detail_metadata = fs::metadata(&detail_path)
            .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
        if detail_metadata.len() != detail_byte_count {
            return Err(vec![P8ArtifactContractFailure::DetailBytesMismatch]);
        }
        let detail_identity_file = File::open(&detail_path)
            .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
        let detail_physical_identity =
            physical_file_identity(&detail_identity_file, &detail_metadata)?;
        let detail_artifact = P8ArtifactIdentityV1::build(
            P8ArtifactId::parse(detail_name).map_err(|failure| vec![failure])?,
            P8Sha256Digest::derive_bytes("p8_shard_detail_artifact_v1", &detail_bytes),
            detail_physical_identity.clone(),
            detail_byte_count,
            u64::try_from(details.len())
                .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?,
        );
        let manifest =
            P8SemanticShardManifestV1::build(run_plan, shard_index, detail_artifact, &details)?;
        let manifest_bytes =
            serialize_json_bounded(&manifest, P8ArtifactLimits::V1.control_json_bytes())?;
        let manifest_byte_count = u64::try_from(manifest_bytes.len())
            .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
        ledger
            .admit_declared(P8ArtifactAdmissionKind::ControlJson, manifest_byte_count)
            .map_err(|failure| vec![failure])?;
        let manifest_path = root.join(format!("shard-{shard_index:05}.manifest.json"));
        write_new_file(&manifest_path, &manifest_bytes)?;
        let manifest_metadata = fs::metadata(&manifest_path)
            .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
        if manifest_metadata.len() != manifest_byte_count {
            return Err(vec![P8ArtifactContractFailure::DetailBytesMismatch]);
        }
        let manifest_identity_file = File::open(&manifest_path)
            .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
        submissions.push(P8SemanticShardSubmissionV1::from_single_read(
            manifest,
            details,
            manifest_byte_count,
            detail_byte_count,
            physical_file_identity(&manifest_identity_file, &manifest_metadata)?,
            detail_physical_identity,
        ));
        checked_add_bundle_count(&mut artifact_count, 2)?;
        checked_add_bundle_count(&mut total_bytes, manifest_byte_count)?;
        checked_add_bundle_count(&mut total_bytes, detail_byte_count)?;
    }
    let summary = produce_p8_semantic_summary(run_plan, submissions)?;
    let summary_bytes =
        serialize_json_bounded(&summary, P8ArtifactLimits::V1.control_json_bytes())?;
    let summary_byte_count = u64::try_from(summary_bytes.len())
        .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
    ledger
        .admit_declared(P8ArtifactAdmissionKind::ControlJson, summary_byte_count)
        .map_err(|failure| vec![failure])?;
    write_new_file(&root.join("summary.json"), &summary_bytes)?;
    checked_add_bundle_count(&mut artifact_count, 1)?;
    checked_add_bundle_count(&mut total_bytes, summary_byte_count)?;
    let root_identity = P8Sha256Digest::derive(
        "p8_published_semantic_bundle_v1",
        &(
            producer_identity.identity_digest(),
            run_plan.run_plan_digest(),
            summary.summary_digest(),
            artifact_count,
            total_bytes,
        ),
    );
    Ok(P8PublishedSemanticBundleV1 {
        root_identity,
        artifact_count,
        total_bytes,
        summary,
    })
}

struct BoundedVecWriter {
    bytes: Vec<u8>,
    limit: u64,
    exceeded: bool,
}

impl BoundedVecWriter {
    fn new(limit: u64) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            exceeded: false,
        }
    }
}

impl Write for BoundedVecWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let next = u64::try_from(self.bytes.len())
            .ok()
            .and_then(|current| {
                u64::try_from(buf.len())
                    .ok()
                    .and_then(|amount| current.checked_add(amount))
            })
            .ok_or_else(|| std::io::Error::other("P8 bounded writer size overflow"))?;
        if next > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::other("P8 bounded writer limit exceeded"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn serialize_json_bounded<T: Serialize>(
    value: &T,
    limit: u64,
) -> Result<Vec<u8>, Vec<P8ArtifactContractFailure>> {
    let mut writer = BoundedVecWriter::new(limit);
    if serde_json::to_writer(&mut writer, value).is_err() {
        return Err(vec![if writer.exceeded {
            P8ArtifactContractFailure::ArtifactLimitExceeded
        } else {
            P8ArtifactContractFailure::SchemaMismatch
        }]);
    }
    Ok(writer.bytes)
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), Vec<P8ArtifactContractFailure>> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|_| vec![P8ArtifactContractFailure::DuplicateArtifact])?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])
}

pub(crate) fn physical_file_identity(
    file: &File,
    metadata: &fs::Metadata,
) -> Result<P8Sha256Digest, Vec<P8ArtifactContractFailure>> {
    #[cfg(unix)]
    {
        let _ = file;
        use std::os::unix::fs::MetadataExt;
        Ok(P8Sha256Digest::derive(
            "p8_physical_file_identity_v1",
            &(metadata.dev(), metadata.ino(), metadata.len()),
        ))
    }
    #[cfg(windows)]
    {
        use std::mem::{size_of, MaybeUninit};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
        };
        let mut info = MaybeUninit::<FILE_ID_INFO>::uninit();
        // SAFETY: file owns a live handle and info has the exact layout requested by FileIdInfo.
        let result = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                info.as_mut_ptr().cast(),
                u32::try_from(size_of::<FILE_ID_INFO>())
                    .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?,
            )
        };
        if result == 0 {
            return Err(vec![P8ArtifactContractFailure::ArtifactIoFailure]);
        }
        // SAFETY: successful GetFileInformationByHandleEx initialized info.
        let info = unsafe { info.assume_init() };
        Ok(P8Sha256Digest::derive(
            "p8_physical_file_identity_v1",
            &(
                info.VolumeSerialNumber,
                info.FileId.Identifier,
                metadata.len(),
            ),
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, metadata);
        Err(vec![P8ArtifactContractFailure::IdentityInvalid])
    }
}

fn checked_add_bundle_count(
    target: &mut u64,
    amount: u64,
) -> Result<(), Vec<P8ArtifactContractFailure>> {
    *target = target
        .checked_add(amount)
        .ok_or_else(|| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
    Ok(())
}

fn verify_shard_submissions(
    run_plan: &P8SemanticRunPlanV1,
    mut submissions: Vec<P8SemanticShardSubmissionV1>,
) -> Result<P8VerifiedShardSet, Vec<P8ArtifactContractFailure>> {
    let mut failures = run_plan.validate_contract();
    submissions.sort_by_key(|submission| submission.manifest.shard_index);
    if submissions.len() != usize::try_from(run_plan.shard_total).unwrap_or(usize::MAX)
        || submissions.iter().enumerate().any(|(index, submission)| {
            usize::try_from(submission.manifest.shard_index).ok() != Some(index)
        })
    {
        failures.push(P8ArtifactContractFailure::ShardCoverageMismatch);
    }
    let mut ledger = P8ArtifactAdmissionLedger::new(P8ArtifactLimits::V1);
    let mut physical_identities = BTreeSet::new();
    let mut question_ids = BTreeSet::new();
    let mut manifests = Vec::with_capacity(submissions.len());
    let mut details = Vec::new();
    for submission in submissions {
        failures.extend(
            submission
                .manifest
                .validate_with_details(run_plan, &submission.details),
        );
        if submission.manifest_bytes == 0
            || submission.detail_bytes != submission.manifest.detail_bytes
            || submission.detail_bytes != submission.manifest.detail_artifact.declared_bytes
        {
            failures.push(P8ArtifactContractFailure::DetailBytesMismatch);
        }
        if submission.detail_physical_identity
            != submission.manifest.detail_artifact.physical_identity
        {
            failures.push(P8ArtifactContractFailure::PhysicalIdentityMismatch);
        }
        if submission.manifest_read_pass_count != 1 || submission.detail_read_pass_count != 1 {
            failures.push(P8ArtifactContractFailure::ReadPassMismatch);
        }
        if !physical_identities.insert(submission.manifest_physical_identity)
            || !physical_identities.insert(submission.detail_physical_identity)
        {
            failures.push(P8ArtifactContractFailure::DuplicateArtifact);
        }
        if ledger
            .admit_declared(
                P8ArtifactAdmissionKind::ControlJson,
                submission.manifest_bytes,
            )
            .is_err()
            || ledger
                .admit_declared(
                    P8ArtifactAdmissionKind::ShardDetails,
                    submission.detail_bytes,
                )
                .is_err()
        {
            failures.push(P8ArtifactContractFailure::ArtifactLimitExceeded);
        }
        for detail in &submission.details {
            if !question_ids.insert(detail.question_id.clone()) {
                failures.push(P8ArtifactContractFailure::DuplicateQuestion);
            }
        }
        manifests.push(submission.manifest);
        details.extend(submission.details);
    }
    let planned_questions = run_plan
        .ordered_questions
        .iter()
        .map(|question| &question.question_id)
        .collect::<BTreeSet<_>>();
    if question_ids.iter().collect::<BTreeSet<_>>() != planned_questions {
        failures.push(P8ArtifactContractFailure::QuestionCoverageMismatch);
    }
    failures.sort();
    failures.dedup();
    if failures.is_empty() {
        Ok(P8VerifiedShardSet { manifests, details })
    } else {
        Err(failures)
    }
}

fn producer_fold_verified_shards(
    run_plan: &P8SemanticRunPlanV1,
    verified: P8VerifiedShardSet,
) -> Result<P8SemanticBenchmarkSummaryV1, Vec<P8ArtifactContractFailure>> {
    let mut overall = P8SemanticMetricCounts::default();
    let mut slices = BTreeMap::<P8AggregationSlice, P8SemanticMetricCounts>::new();
    let mut ablation_deltas = P8SemanticOffRunKey::ALL
        .into_iter()
        .map(|key| (key, P8AblationAggregate::default()))
        .collect::<BTreeMap<_, _>>();
    for detail in &verified.details {
        observe_metrics(&mut overall, detail)?;
        let mut memberships = BTreeSet::from([
            P8AggregationSlice::BenchmarkFamily(detail.benchmark_family),
            P8AggregationSlice::DatasetStratum(detail.dataset_stratum.clone()),
            P8AggregationSlice::Task(detail.task_kind),
            P8AggregationSlice::QueryOperation(detail.query_operation_kind),
            P8AggregationSlice::TemporalCorpus(detail.temporal_corpus_slice),
            P8AggregationSlice::Profile(detail.profile),
        ]);
        memberships.extend(
            detail
                .capability_slices
                .iter()
                .copied()
                .map(P8AggregationSlice::Capability),
        );
        memberships.extend(
            detail
                .safety_slices
                .iter()
                .copied()
                .map(P8AggregationSlice::Safety),
        );
        for membership in memberships {
            observe_metrics(slices.entry(membership).or_default(), detail)?;
        }
        for observation in detail.sdk_off_run_report.observations() {
            let aggregate = ablation_deltas
                .get_mut(&observation.key())
                .expect("exact eight-key aggregate exists");
            checked_add_assign(
                &mut aggregate.applicable_count,
                u64::from(observation.applicable()),
            )?;
            checked_add_assign(
                &mut aggregate.executed_count,
                u64::from(observation.executed()),
            )?;
            let evaluation = detail
                .ablation_evaluations
                .get(&observation.key())
                .expect("detail validation requires exact eight evaluations");
            checked_add_assign(
                &mut aggregate.baseline_correct_count,
                u64::from(evaluation.baseline_decision.is_correct() && observation.applicable()),
            )?;
            checked_add_assign(
                &mut aggregate.off_run_correct_count,
                u64::from(evaluation.off_run_decision.is_correct() && observation.executed()),
            )?;
        }
    }
    let mut summary = P8SemanticBenchmarkSummaryV1 {
        schema: P8_SEMANTIC_BENCHMARK_SUMMARY_SCHEMA.into(),
        producer_identity_digest: run_plan.producer_identity_digest.clone(),
        run_plan_digest: run_plan.run_plan_digest.clone(),
        run_id: run_plan.run_id.clone(),
        admitted_shard_manifest_digests: verified
            .manifests
            .iter()
            .map(P8SemanticShardManifestV1::manifest_digest)
            .collect(),
        ordered_detail_digests: verified
            .details
            .iter()
            .map(p8_semantic_detail_digest)
            .collect(),
        overall,
        slices: slices
            .into_iter()
            .map(|(slice, metrics)| P8SemanticMetricSlice { slice, metrics })
            .collect(),
        ablation_deltas,
        summary_digest: P8Sha256Digest::derive("p8_semantic_benchmark_summary_v1", &()),
    };
    summary.summary_digest = summary.derived_digest();
    Ok(summary)
}

fn observe_metrics(
    metrics: &mut P8SemanticMetricCounts,
    detail: &P8SemanticQuestionDetailV1,
) -> Result<(), Vec<P8ArtifactContractFailure>> {
    checked_add_assign(&mut metrics.question_count, 1)?;
    checked_add_assign(
        &mut metrics.correct_count,
        u64::from(detail.accuracy_decision.is_correct()),
    )?;
    match detail.memory_use_decision {
        P8MemoryUseDecision::CurrentUsed => checked_add_assign(&mut metrics.current_use_count, 1)?,
        P8MemoryUseDecision::ObsoleteRejected => {
            checked_add_assign(&mut metrics.obsolete_rejected_count, 1)?
        }
        P8MemoryUseDecision::ObsoleteUsed => {
            checked_add_assign(&mut metrics.obsolete_used_count, 1)?
        }
        P8MemoryUseDecision::InvalidatedRejected => {
            checked_add_assign(&mut metrics.invalidated_rejected_count, 1)?
        }
        P8MemoryUseDecision::InvalidatedUsed => {
            checked_add_assign(&mut metrics.invalidated_used_count, 1)?
        }
        P8MemoryUseDecision::NotApplicable => {}
    }
    let safety_failures = u64::try_from(
        detail
            .failures
            .iter()
            .filter(|failure| **failure == P8SemanticFailure::SafetyViolation)
            .count(),
    )
    .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
    checked_add_assign(&mut metrics.safety_failure_count, safety_failures)?;
    checked_add_assign(&mut metrics.elapsed_millis, detail.resource.elapsed_millis)?;
    metrics.peak_rss_bytes = metrics.peak_rss_bytes.max(detail.resource.peak_rss_bytes);
    Ok(())
}

fn checked_add_assign(target: &mut u64, amount: u64) -> Result<(), Vec<P8ArtifactContractFailure>> {
    *target = target
        .checked_add(amount)
        .ok_or_else(|| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8VerifierExecutionEvidenceV1 {
    ObservedPathStable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8VerifierIdentityV1 {
    pub(crate) schema: String,
    pub(crate) source_identity: P8VerifierSourceIdentityRef,
    pub(crate) build_identity: P8VerifierBuildIdentityRef,
    pub(crate) executable_identity: P8VerifierExecutableIdentityRef,
    pub(crate) execution_evidence: P8VerifierExecutionEvidenceV1,
    pub(crate) identity_digest: P8VerifierIdentityRef,
}

impl P8VerifierIdentityV1 {
    pub(crate) fn for_current_process() -> Result<Self, Vec<P8ArtifactContractFailure>> {
        let executable = std::env::current_exe()
            .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
        Self::for_executable(&executable)
    }

    pub(crate) fn for_executable(
        executable: &Path,
    ) -> Result<Self, Vec<P8ArtifactContractFailure>> {
        validate_p8_build_source_attestation(P8_BUILD_SOURCE_ATTESTATION)?;
        let source_identity = P8VerifierSourceIdentityRef::derive(&(
            P8_VERIFIER_SOURCE_FINGERPRINT,
            P8_SEMANTIC_OPERATOR_REPORT_SCHEMA,
        ));
        let build_identity = P8VerifierBuildIdentityRef::derive(&(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            cfg!(debug_assertions),
            cfg!(feature = "sqlite-store"),
            env!("BM_P8_OPERATOR_BUILD_FINGERPRINT"),
            env!("BM_P8_OPERATOR_BUILD_PROFILE"),
            env!("BM_P8_OPERATOR_BUILD_FEATURES"),
            env!("BM_P8_OPERATOR_BUILD_TARGET"),
            env!("BM_P8_OPERATOR_BUILD_HOST"),
            P8_BUILD_SOURCE_ATTESTATION,
        ));
        let executable_identity = verifier_executable_identity(executable)?;
        let mut value = Self {
            schema: P8_SEMANTIC_VERIFIER_IDENTITY_SCHEMA.into(),
            source_identity,
            build_identity,
            executable_identity,
            execution_evidence: P8VerifierExecutionEvidenceV1::ObservedPathStable,
            identity_digest: P8VerifierIdentityRef::derive(&()),
        };
        value.identity_digest = value.derived_digest();
        Ok(value)
    }

    pub fn identity_digest(&self) -> &P8VerifierIdentityRef {
        &self.identity_digest
    }

    pub fn validate_contract(&self) -> Vec<P8ArtifactContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_SEMANTIC_VERIFIER_IDENTITY_SCHEMA {
            failures.push(P8ArtifactContractFailure::SchemaMismatch);
        }
        if self.execution_evidence != P8VerifierExecutionEvidenceV1::ObservedPathStable {
            failures.push(P8ArtifactContractFailure::IdentityInvalid);
        }
        if self.identity_digest != self.derived_digest() {
            failures.push(P8ArtifactContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8VerifierIdentityRef {
        P8VerifierIdentityRef::derive(&(
            &self.schema,
            &self.source_identity,
            &self.build_identity,
            &self.executable_identity,
            &self.execution_evidence,
        ))
    }
}

fn validate_p8_build_source_attestation(
    attestation: &str,
) -> Result<(), Vec<P8ArtifactContractFailure>> {
    if attestation == P8_WORKSPACE_BUILD_SOURCE_ATTESTATION {
        Ok(())
    } else {
        Err(vec![P8ArtifactContractFailure::IdentityInvalid])
    }
}

pub(crate) fn p8_trusted_source_root() -> Result<PathBuf, Vec<P8ArtifactContractFailure>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| vec![P8ArtifactContractFailure::IdentityInvalid])?;
    fs::canonicalize(root).map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])
}

fn verifier_executable_identity(
    executable: &Path,
) -> Result<P8VerifierExecutableIdentityRef, Vec<P8ArtifactContractFailure>> {
    const DOMAIN: &str = "p8_verifier_executable_identity_v1";
    const PREFIX: &str = "p8_verifier_executable_identity:sha256:";
    let mut file =
        File::open(executable).map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
    let declared_bytes = file
        .metadata()
        .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?
        .len();
    if declared_bytes > P8ArtifactLimits::V1.total_operator_artifact_bytes() {
        return Err(vec![P8ArtifactContractFailure::ArtifactLimitExceeded]);
    }
    let mut hasher = Sha256::new();
    hasher.update(
        u64::try_from(DOMAIN.len())
            .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?
            .to_be_bytes(),
    );
    hasher.update(DOMAIN.as_bytes());
    hasher.update(declared_bytes.to_be_bytes());
    let mut observed_bytes = 0_u64;
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
        if read == 0 {
            break;
        }
        observed_bytes = observed_bytes
            .checked_add(
                u64::try_from(read)
                    .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?,
            )
            .ok_or_else(|| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
        if observed_bytes > declared_bytes {
            return Err(vec![P8ArtifactContractFailure::DetailBytesMismatch]);
        }
        hasher.update(&chunk[..read]);
    }
    if observed_bytes != declared_bytes {
        return Err(vec![P8ArtifactContractFailure::DetailBytesMismatch]);
    }
    Ok(P8VerifierExecutableIdentityRef(format!(
        "{PREFIX}{:x}",
        hasher.finalize()
    )))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P8GateContractId {
    P8SemanticArtifactVerification,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8GateCommandReceiptV1 {
    pub(crate) schema: String,
    pub(crate) contract: P8GateContractId,
    pub(crate) exact_test_name: P8ArtifactId,
    pub(crate) verifier_identity_digest: P8VerifierIdentityRef,
    pub(crate) argv_digest: P8GateArgvRef,
    pub(crate) cwd_source_identity: P8GateCwdSourceIdentityRef,
    pub(crate) build_profile_identity: P8GateBuildProfileIdentityRef,
    pub(crate) exit_code: i32,
    pub(crate) stdout_bytes: u64,
    pub(crate) stdout_digest: P8ClosedStdoutRef,
    pub(crate) stderr_bytes: u64,
    pub(crate) stderr_digest: P8ClosedStderrRef,
    pub(crate) receipt_digest: P8GateCommandReceiptRef,
}

pub(crate) struct P8ClosedChildObservation<'a> {
    pub(crate) cwd: &'a Path,
    pub(crate) exit_code: i32,
    pub(crate) closed_stdout: &'a [u8],
    pub(crate) closed_stderr: &'a [u8],
}

impl P8GateCommandReceiptV1 {
    pub(crate) fn from_parent_observation(
        verifier_identity: &P8VerifierIdentityV1,
        observation: P8ClosedChildObservation<'_>,
    ) -> Result<Self, Vec<P8ArtifactContractFailure>> {
        if !verifier_identity.validate_contract().is_empty() {
            return Err(vec![P8ArtifactContractFailure::IdentityInvalid]);
        }
        let canonical_cwd = fs::canonicalize(observation.cwd)
            .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
        let stdout_bytes = u64::try_from(observation.closed_stdout.len())
            .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
        let stderr_bytes = u64::try_from(observation.closed_stderr.len())
            .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?;
        let mut value = Self {
            schema: P8_SEMANTIC_GATE_COMMAND_RECEIPT_SCHEMA.into(),
            contract: P8GateContractId::P8SemanticArtifactVerification,
            exact_test_name: P8ArtifactId::parse(P8_SEMANTIC_GATE_SELF_TEST_NAME)
                .map_err(|failure| vec![failure])?,
            verifier_identity_digest: verifier_identity.identity_digest.clone(),
            argv_digest: expected_gate_argv_digest(verifier_identity),
            cwd_source_identity: P8GateCwdSourceIdentityRef::derive(&(
                canonical_cwd.as_os_str().as_encoded_bytes(),
                &verifier_identity.source_identity,
            )),
            build_profile_identity: P8GateBuildProfileIdentityRef::derive(
                &verifier_identity.build_identity,
            ),
            exit_code: observation.exit_code,
            stdout_bytes,
            stdout_digest: P8ClosedStdoutRef::derive_bytes(observation.closed_stdout),
            stderr_bytes,
            stderr_digest: P8ClosedStderrRef::derive_bytes(observation.closed_stderr),
            receipt_digest: P8GateCommandReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_digest();
        Ok(value)
    }

    pub fn receipt_digest(&self) -> &P8GateCommandReceiptRef {
        &self.receipt_digest
    }

    pub fn exact_test_name(&self) -> &P8ArtifactId {
        &self.exact_test_name
    }

    pub fn validate_contract(
        &self,
        verifier_identity: &P8VerifierIdentityV1,
        expected_cwd: &Path,
    ) -> Vec<P8ArtifactContractFailure> {
        let mut failures = Vec::new();
        let expected_cwd_source_identity =
            fs::canonicalize(expected_cwd).ok().map(|canonical_cwd| {
                P8GateCwdSourceIdentityRef::derive(&(
                    canonical_cwd.as_os_str().as_encoded_bytes(),
                    &verifier_identity.source_identity,
                ))
            });
        if self.schema != P8_SEMANTIC_GATE_COMMAND_RECEIPT_SCHEMA {
            failures.push(P8ArtifactContractFailure::SchemaMismatch);
        }
        if self.contract != P8GateContractId::P8SemanticArtifactVerification
            || self.exact_test_name.as_str() != P8_SEMANTIC_GATE_SELF_TEST_NAME
            || self.verifier_identity_digest != verifier_identity.identity_digest
            || self.argv_digest != expected_gate_argv_digest(verifier_identity)
            || self.build_profile_identity
                != P8GateBuildProfileIdentityRef::derive(&verifier_identity.build_identity)
            || self.exit_code != 0
        {
            failures.push(P8ArtifactContractFailure::ReceiptInvalid);
        }
        if expected_cwd_source_identity.as_ref() != Some(&self.cwd_source_identity) {
            failures.push(P8ArtifactContractFailure::IdentityInvalid);
        }
        if self.receipt_digest != self.derived_digest() {
            failures.push(P8ArtifactContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8GateCommandReceiptRef {
        P8GateCommandReceiptRef::derive(&(
            &self.schema,
            self.contract,
            &self.exact_test_name,
            &self.verifier_identity_digest,
            &self.argv_digest,
            &self.cwd_source_identity,
            &self.build_profile_identity,
            self.exit_code,
            self.stdout_bytes,
            &self.stdout_digest,
            self.stderr_bytes,
            &self.stderr_digest,
        ))
    }
}

fn expected_gate_argv_digest(verifier_identity: &P8VerifierIdentityV1) -> P8GateArgvRef {
    P8GateArgvRef::derive(&(
        &verifier_identity.executable_identity,
        [P8_SEMANTIC_GATE_SELF_TEST_ARG],
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8VerificationReceiptV1 {
    pub(crate) schema: String,
    pub(crate) verifier_identity_digest: P8VerifierIdentityRef,
    pub(crate) gate_command_receipt_digest: P8GateCommandReceiptRef,
    pub(crate) admitted_artifact_bytes: u64,
    pub(crate) bytes_read: u64,
    pub(crate) artifact_read_count: u64,
    pub(crate) receipt_digest: P8VerificationReceiptRef,
}

impl P8VerificationReceiptV1 {
    pub(crate) fn build(
        verifier_identity: &P8VerifierIdentityV1,
        gate_receipt: &P8GateCommandReceiptV1,
        admitted_artifact_bytes: u64,
        bytes_read: u64,
        artifact_read_count: u64,
    ) -> Self {
        let mut value = Self {
            schema: P8_SEMANTIC_VERIFICATION_RECEIPT_SCHEMA.into(),
            verifier_identity_digest: verifier_identity.identity_digest.clone(),
            gate_command_receipt_digest: gate_receipt.receipt_digest.clone(),
            admitted_artifact_bytes,
            bytes_read,
            artifact_read_count,
            receipt_digest: P8VerificationReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_digest();
        value
    }

    pub fn receipt_digest(&self) -> &P8VerificationReceiptRef {
        &self.receipt_digest
    }

    pub const fn admitted_artifact_bytes(&self) -> u64 {
        self.admitted_artifact_bytes
    }

    pub const fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    pub const fn artifact_read_count(&self) -> u64 {
        self.artifact_read_count
    }

    pub fn validate_contract(
        &self,
        verifier_identity: &P8VerifierIdentityV1,
        gate_receipt: Option<&P8GateCommandReceiptV1>,
    ) -> Vec<P8ArtifactContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_SEMANTIC_VERIFICATION_RECEIPT_SCHEMA {
            failures.push(P8ArtifactContractFailure::SchemaMismatch);
        }
        if self.verifier_identity_digest != verifier_identity.identity_digest
            || gate_receipt
                .is_some_and(|gate| self.gate_command_receipt_digest != gate.receipt_digest)
            || self.admitted_artifact_bytes != self.bytes_read
            || self.artifact_read_count == 0
        {
            failures.push(P8ArtifactContractFailure::ReceiptInvalid);
        }
        if self.receipt_digest != self.derived_digest() {
            failures.push(P8ArtifactContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8VerificationReceiptRef {
        P8VerificationReceiptRef::derive(&(
            &self.schema,
            &self.verifier_identity_digest,
            &self.gate_command_receipt_digest,
            self.admitted_artifact_bytes,
            self.bytes_read,
            self.artifact_read_count,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct P8SemanticOperatorReportV1 {
    pub(crate) schema: String,
    pub(crate) run_id: P8ArtifactId,
    pub(crate) supplied_summary_digest: P8Sha256Digest,
    pub(crate) recomputed_summary_digest: P8Sha256Digest,
    pub(crate) verifier_identity: P8VerifierIdentityV1,
    pub(crate) verification_receipt: P8VerificationReceiptV1,
    pub(crate) mismatches: Vec<P8ArtifactContractFailure>,
    pub(crate) release_eligible: bool,
    pub(crate) report_digest: P8SemanticOperatorReportRef,
}

impl P8SemanticOperatorReportV1 {
    pub(crate) fn from_independent_recomputation(
        run_id: P8ArtifactId,
        supplied_summary_digest: P8Sha256Digest,
        recomputed_summary_digest: P8Sha256Digest,
        verifier_identity: P8VerifierIdentityV1,
        verification_receipt: P8VerificationReceiptV1,
        mut mismatches: Vec<P8ArtifactContractFailure>,
    ) -> Self {
        mismatches.push(P8ArtifactContractFailure::QualityThresholdsNotFrozen);
        mismatches.sort();
        mismatches.dedup();
        let mut value = Self {
            schema: P8_SEMANTIC_OPERATOR_REPORT_SCHEMA.into(),
            run_id,
            supplied_summary_digest,
            recomputed_summary_digest,
            verifier_identity,
            verification_receipt,
            mismatches,
            release_eligible: false,
            report_digest: P8SemanticOperatorReportRef::derive(&()),
        };
        value.report_digest = value.derived_digest();
        value
    }

    pub fn mismatches(&self) -> &[P8ArtifactContractFailure] {
        &self.mismatches
    }

    pub const fn release_eligible(&self) -> bool {
        self.release_eligible
    }

    pub fn verification_receipt(&self) -> &P8VerificationReceiptV1 {
        &self.verification_receipt
    }

    pub fn validate_contract(&self) -> Vec<P8ArtifactContractFailure> {
        let mut failures = self.verifier_identity.validate_contract();
        failures.extend(
            self.verification_receipt
                .validate_contract(&self.verifier_identity, None),
        );
        if self.schema != P8_SEMANTIC_OPERATOR_REPORT_SCHEMA {
            failures.push(P8ArtifactContractFailure::SchemaMismatch);
        }
        let canonical_mismatches = self
            .mismatches
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if self.release_eligible
            || canonical_mismatches != self.mismatches
            || !self
                .mismatches
                .contains(&P8ArtifactContractFailure::QualityThresholdsNotFrozen)
        {
            failures.push(P8ArtifactContractFailure::ReceiptInvalid);
        }
        if self.report_digest != self.derived_digest() {
            failures.push(P8ArtifactContractFailure::DigestInvalid);
        }
        failures.sort();
        failures.dedup();
        failures
    }

    fn derived_digest(&self) -> P8SemanticOperatorReportRef {
        P8SemanticOperatorReportRef::derive(&(
            &self.schema,
            &self.run_id,
            &self.supplied_summary_digest,
            &self.recomputed_summary_digest,
            &self.verifier_identity,
            &self.verification_receipt,
            &self.mismatches,
            self.release_eligible,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P8ArtifactLimits {
    detail_line_bytes: u64,
    control_json_bytes: u64,
    shard_detail_bytes: u64,
    total_detail_bytes: u64,
    total_operator_artifact_bytes: u64,
    retained_handles: u64,
    operator_wall_millis: u64,
}

impl P8ArtifactLimits {
    pub const V1: Self = Self {
        detail_line_bytes: 16 * 1024 * 1024,
        control_json_bytes: 64 * 1024 * 1024,
        shard_detail_bytes: 2 * 1024 * 1024 * 1024,
        total_detail_bytes: 8 * 1024 * 1024 * 1024,
        total_operator_artifact_bytes: 10 * 1024 * 1024 * 1024,
        retained_handles: 4096,
        operator_wall_millis: 30 * 60 * 1000,
    };

    pub const fn detail_line_bytes(self) -> u64 {
        self.detail_line_bytes
    }
    pub const fn control_json_bytes(self) -> u64 {
        self.control_json_bytes
    }
    pub const fn shard_detail_bytes(self) -> u64 {
        self.shard_detail_bytes
    }
    pub const fn total_detail_bytes(self) -> u64 {
        self.total_detail_bytes
    }
    pub const fn total_operator_artifact_bytes(self) -> u64 {
        self.total_operator_artifact_bytes
    }
    pub const fn retained_handles(self) -> u64 {
        self.retained_handles
    }
    pub const fn operator_wall_millis(self) -> u64 {
        self.operator_wall_millis
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P8ArtifactAdmissionKind {
    DetailLine,
    ControlJson,
    ShardDetails,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P8ArtifactAdmissionLedger {
    limits: P8ArtifactLimits,
    detail_bytes: u64,
    operator_artifact_bytes: u64,
    retained_handle_count: u64,
    read_pass_count: u64,
    parsed_document_count: u64,
}

impl P8ArtifactAdmissionLedger {
    pub const fn new(limits: P8ArtifactLimits) -> Self {
        Self {
            limits,
            detail_bytes: 0,
            operator_artifact_bytes: 0,
            retained_handle_count: 0,
            read_pass_count: 0,
            parsed_document_count: 0,
        }
    }

    pub fn admit_declared(
        &mut self,
        kind: P8ArtifactAdmissionKind,
        bytes: u64,
    ) -> Result<(), P8ArtifactContractFailure> {
        let per_artifact_limit = match kind {
            P8ArtifactAdmissionKind::DetailLine => self.limits.detail_line_bytes,
            P8ArtifactAdmissionKind::ControlJson => self.limits.control_json_bytes,
            P8ArtifactAdmissionKind::ShardDetails => self.limits.shard_detail_bytes,
        };
        if bytes > per_artifact_limit {
            return Err(P8ArtifactContractFailure::ArtifactLimitExceeded);
        }
        if kind == P8ArtifactAdmissionKind::DetailLine {
            return Ok(());
        }
        let next_operator = self
            .operator_artifact_bytes
            .checked_add(bytes)
            .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?;
        if next_operator > self.limits.total_operator_artifact_bytes {
            return Err(P8ArtifactContractFailure::ArtifactLimitExceeded);
        }
        let next_detail = if kind == P8ArtifactAdmissionKind::ShardDetails {
            let next = self
                .detail_bytes
                .checked_add(bytes)
                .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?;
            if next > self.limits.total_detail_bytes {
                return Err(P8ArtifactContractFailure::ArtifactLimitExceeded);
            }
            next
        } else {
            self.detail_bytes
        };
        self.operator_artifact_bytes = next_operator;
        self.detail_bytes = next_detail;
        Ok(())
    }

    pub fn admit_retained_handle(&mut self) -> Result<(), P8ArtifactContractFailure> {
        let next = self
            .retained_handle_count
            .checked_add(1)
            .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?;
        if next > self.limits.retained_handles {
            return Err(P8ArtifactContractFailure::ArtifactLimitExceeded);
        }
        self.retained_handle_count = next;
        Ok(())
    }

    pub(crate) fn record_read_pass(&mut self) -> Result<(), P8ArtifactContractFailure> {
        self.read_pass_count = self
            .read_pass_count
            .checked_add(1)
            .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?;
        Ok(())
    }

    pub(crate) fn record_parsed_document(&mut self) -> Result<(), P8ArtifactContractFailure> {
        self.parsed_document_count = self
            .parsed_document_count
            .checked_add(1)
            .ok_or(P8ArtifactContractFailure::ArithmeticOverflow)?;
        Ok(())
    }

    pub const fn retained_handle_count(&self) -> u64 {
        self.retained_handle_count
    }

    pub const fn read_pass_count(&self) -> u64 {
        self.read_pass_count
    }

    pub const fn parsed_document_count(&self) -> u64 {
        self.parsed_document_count
    }
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix(SHA256_PREFIX).is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn has_typed_sha256_prefix(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn is_canonical_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

fn domain_separated_sha256(domain: &str, parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(
        u64::try_from(domain.len())
            .expect("in-memory domain length fits u64")
            .to_be_bytes(),
    );
    hasher.update(domain.as_bytes());
    for part in parts {
        hasher.update(
            u64::try_from(part.len())
                .expect("in-memory digest part length fits u64")
                .to_be_bytes(),
        );
        hasher.update(part);
    }
    format!("{:x}", hasher.finalize())
}

fn is_strict_sorted_unique<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|window| window[0] < window[1])
}

fn is_strict_sorted_unique_nonempty<T: Ord>(values: &[T]) -> bool {
    !values.is_empty() && is_strict_sorted_unique(values)
}

#[cfg(test)]
mod tests {
    use super::{
        serialize_json_bounded, validate_p8_build_source_attestation,
        P8_WORKSPACE_BUILD_SOURCE_ATTESTATION,
    };
    use crate::p8_semantic::P8ArtifactContractFailure;

    #[test]
    fn p8_json_writer_accepts_exact_and_rejects_n_plus_one_before_growth() {
        assert_eq!(
            serialize_json_bounded(&"1234", 6).expect("exact JSON"),
            br#""1234""#
        );
        assert_eq!(
            serialize_json_bounded(&"1234", 5),
            Err(vec![P8ArtifactContractFailure::ArtifactLimitExceeded])
        );
    }

    #[test]
    fn p8_verifier_identity_rejects_packaged_unattested_builds() {
        assert_eq!(
            validate_p8_build_source_attestation("packaged_unattested"),
            Err(vec![P8ArtifactContractFailure::IdentityInvalid])
        );
        assert!(
            validate_p8_build_source_attestation(P8_WORKSPACE_BUILD_SOURCE_ATTESTATION).is_ok()
        );
    }
}
