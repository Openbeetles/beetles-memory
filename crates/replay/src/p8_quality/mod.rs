//! P8.5 quality contracts.
//!
//! This private module is the sole owner of quality schemas and decisions. It deliberately does
//! not expose raw constructors through the `bm-replay` public surface.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

mod policy_anchor;
mod source_publisher;
mod source_release;
mod trusted_execution;
use self::policy_anchor::P8FrozenQualityPolicyV1;
use self::source_release::{
    P8ArmReleaseRef, P8CommonHarnessSemanticSourceRef, P8P84RawSourceAuditManifestV1,
    P8P84SemanticSourceAnchorV1, P8SourceReleaseEvidenceClassV1, P8SourceReleaseSetV1,
};
use self::trusted_execution::{
    P8TrustedDomainResourceReceiptRef, P8TrustedDomainResourceReceiptV1,
};
use super::p8_semantic::{
    P8BenchmarkFamily, P8CapabilitySlice, P8DatasetStratum, P8QueryOperationKind, P8SafetySlice,
    P8TaskKind, P8TemporalCorpusSlice,
};
use bm_core::feature_gate::ProfileId;

#[doc(hidden)]
pub fn try_run_trusted_supervisor_session_entry() -> Option<std::io::Result<()>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.as_slice()
        != [std::ffi::OsStr::new(
            trusted_execution::supervisor_session::P8_TRUSTED_SUPERVISOR_SESSION_ARG,
        )]
    {
        return None;
    }
    Some(
        trusted_execution::supervisor_session::claim_inputs_from_parent().and_then(
            |availability| match availability {
                trusted_execution::supervisor_session::P8TrustedSupervisorAvailability::Established(
                    inputs,
                ) => complete_trusted_supervisor_session(inputs),
                trusted_execution::supervisor_session::P8TrustedSupervisorAvailability::NotApplicable(
                    trusted_execution::supervisor_session::P8TrustedSupervisorNaReason::TrustedLinuxAuthorityUnavailable,
                ) => Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "P8 TrustedSupervisor retained-FD authority is unavailable on this platform",
                )),
            },
        ),
    )
}

#[doc(hidden)]
pub fn try_run_source_publisher_session_entry() -> Option<std::io::Result<()>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.as_slice()
        != [std::ffi::OsStr::new(
            trusted_execution::publication::P8_SOURCE_PUBLISHER_SESSION_ARG,
        )]
    {
        return None;
    }
    Some(trusted_execution::publication::run_source_publisher_session_entry())
}

/// Parent-owned input for one trusted P8 quality session.
///
/// This type intentionally contains only launch capabilities. It cannot carry, clone, or
/// serialize the opaque publication authority minted after the child and publisher closures.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct P8TrustedSupervisorParentPlan {
    pub source_root: PathBuf,
    pub releases_root: PathBuf,
    pub source_publisher_executable: PathBuf,
    pub quality_runner_executable: PathBuf,
    pub quality_operator_executable: PathBuf,
    pub trusted_supervisor_executable: PathBuf,
    pub cargo_executable: PathBuf,
    pub rustc_executable: PathBuf,
    pub rustdoc_executable: PathBuf,
    pub rustfmt_executable: PathBuf,
    pub cargo_fmt_executable: PathBuf,
    pub cargo_clippy_executable: PathBuf,
    pub clippy_driver_executable: PathBuf,
    pub rust_lld_executable: PathBuf,
    pub target_root: PathBuf,
    pub rust_sysroot_root: PathBuf,
    pub cargo_dependency_cache_root: PathBuf,
    pub timeout: Duration,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub total_bytes: u64,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P8TrustedSupervisorParentResult {
    PublishedAndLiveVerified,
    NotApplicableOnThisPlatform,
}

/// Runs the outer-parent half of the trusted supervisor protocol.
///
/// A committed-but-unattested release is an error. The opaque published capability is consumed
/// inside this function and live-verified again before success is returned; no serializable
/// receipt or caller-constructible token can substitute for that capability.
#[doc(hidden)]
pub fn run_trusted_supervisor_parent_session(
    plan: P8TrustedSupervisorParentPlan,
) -> std::io::Result<P8TrustedSupervisorParentResult> {
    use source_release::P8HarnessExecutableRoleV1;
    use trusted_execution::engineering_gate::P8EngineeringToolRoleV1;
    use trusted_execution::supervisor_session::{
        launch_peer_bound_session, P8TrustedSupervisorAvailability, P8TrustedSupervisorLaunchInput,
        P8TrustedSupervisorPublicationOutcome,
    };

    let availability = launch_peer_bound_session(P8TrustedSupervisorLaunchInput {
        source_root: plan.source_root,
        releases_root: plan.releases_root,
        role_executables: vec![
            (
                P8HarnessExecutableRoleV1::SourcePublisher,
                plan.source_publisher_executable,
            ),
            (
                P8HarnessExecutableRoleV1::QualityRunner,
                plan.quality_runner_executable,
            ),
            (
                P8HarnessExecutableRoleV1::QualityOperator,
                plan.quality_operator_executable,
            ),
            (
                P8HarnessExecutableRoleV1::TrustedSupervisor,
                plan.trusted_supervisor_executable,
            ),
        ],
        tool_executables: vec![
            (P8EngineeringToolRoleV1::Cargo, plan.cargo_executable),
            (P8EngineeringToolRoleV1::Rustc, plan.rustc_executable),
            (P8EngineeringToolRoleV1::Rustdoc, plan.rustdoc_executable),
            (P8EngineeringToolRoleV1::Rustfmt, plan.rustfmt_executable),
            (P8EngineeringToolRoleV1::CargoFmt, plan.cargo_fmt_executable),
            (
                P8EngineeringToolRoleV1::CargoClippy,
                plan.cargo_clippy_executable,
            ),
            (
                P8EngineeringToolRoleV1::ClippyDriver,
                plan.clippy_driver_executable,
            ),
            (P8EngineeringToolRoleV1::RustLld, plan.rust_lld_executable),
        ],
        target_root: plan.target_root,
        rust_sysroot_root: plan.rust_sysroot_root,
        cargo_dependency_cache_root: plan.cargo_dependency_cache_root,
        timeout: plan.timeout,
        stdout_bytes: plan.stdout_bytes,
        stderr_bytes: plan.stderr_bytes,
        total_bytes: plan.total_bytes,
    })?;

    match availability {
        P8TrustedSupervisorAvailability::NotApplicable(_) => {
            Ok(P8TrustedSupervisorParentResult::NotApplicableOnThisPlatform)
        }
        P8TrustedSupervisorAvailability::Established(
            P8TrustedSupervisorPublicationOutcome::Published(mut published),
        ) => {
            #[cfg(target_os = "linux")]
            {
                published.verify_live()?;
                published.verify_live()?;
                Ok(P8TrustedSupervisorParentResult::PublishedAndLiveVerified)
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = &mut published;
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "P8 published authority cannot exist outside trusted Linux",
                ))
            }
        }
        P8TrustedSupervisorAvailability::Established(
            P8TrustedSupervisorPublicationOutcome::CommittedUnattested(_),
        ) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "P8 release committed without outer-parent attestation",
        )),
        P8TrustedSupervisorAvailability::Established(
            P8TrustedSupervisorPublicationOutcome::PreCommitFailed(_),
        ) => Err(std::io::Error::other(
            "P8 trusted publication failed before commit",
        )),
    }
}

#[cfg(target_os = "linux")]
fn complete_trusted_supervisor_session(
    inputs: trusted_execution::supervisor_session::P8TrustedSupervisorInputs,
) -> std::io::Result<()> {
    let verified = trusted_execution::engineering_gate::execute_trusted_gate_set(inputs)?;
    let _ = trusted_execution::publication::publish_verified_gate_set(verified)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn complete_trusted_supervisor_session(
    _inputs: trusted_execution::supervisor_session::P8TrustedSupervisorInputs,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "P8 TrustedSupervisor execution is unavailable on this platform",
    ))
}

const SHA256_PREFIX: &str = "sha256:";
const P8_HARD_POLICY_SCHEMA: &str = "beetle-memory.p8.quality-hard-policy.v1";
const P8_HYPOTHESIS_REGISTRY_SCHEMA: &str = "beetle-memory.p8.quality-hypothesis-registry.v1";
const P8_TRIAL_CLOSURE_SCHEMA: &str = "beetle-memory.p8.quality-trial-closure.v1";
const P8_PROTOCOL_LOCK_SCHEMA: &str = "beetle-memory.p8.evaluation-protocol-lock.v1";
const P8_THRESHOLD_LOCK_SCHEMA: &str = "beetle-memory.p8.quality-threshold-lock.v1";
const P8_EXPERIMENT_PLAN_SCHEMA: &str = "beetle-memory.p8.quality-experiment-plan.v1";
const P8_HARD_GATE_EVALUATION_SCHEMA: &str = "beetle-memory.p8.quality-hard-gate-evaluation.v1";
const P8_THRESHOLD_EVALUATION_SCHEMA: &str = "beetle-memory.p8.quality-threshold-evaluation.v1";
const P8_TRIAL_SET_SCHEMA: &str = "beetle-memory.p8.quality-trial-set.v1";
const P8_RESOURCE_CLOSURE_SCHEMA: &str = "beetle-memory.p8.quality-resource-closure.v1";
const P8_CORE_VALIDATOR_FINGERPRINT: &str = env!("BM_P8_CORE_VALIDATOR_FINGERPRINT");
const P8_SDK_VALIDATOR_FINGERPRINT: &str = env!("BM_P8_SDK_VALIDATOR_FINGERPRINT");
const P8_POST_IMAGE_VALIDATOR_FINGERPRINT: &str = env!("BM_P8_POST_IMAGE_VALIDATOR_FINGERPRINT");
const P8_REPLAY_VALIDATOR_FINGERPRINT: &str = env!("BM_P8_VERIFIER_SOURCE_FINGERPRINT");
const P8_VALIDATOR_SOURCE_ATTESTATION: &str = env!("BM_P8_VALIDATOR_SOURCE_ATTESTATION");

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum P8QualityContractFailure {
    SchemaMismatch,
    DigestInvalid,
    IdentityInvalid,
    DuplicateEntry,
    CoverageMismatch,
    HardPolicyMismatch,
    PurposeMismatch,
    ArmSetMismatch,
    ArmIdentityAlias,
    RoleSetMismatch,
    RoleIdentityAlias,
    SelfReference,
    HypothesisMismatch,
    OutcomeMismatch,
    DenominatorMismatch,
    StateMatrixMismatch,
    ArithmeticOverflow,
    DuplicateJsonKey,
    ArtifactKindMismatch,
    ThresholdMismatch,
    TrustedSourceMissing,
    TrustedExecutionMissing,
    CommandMismatch,
    EnvironmentMismatch,
    SourceDrift,
    ToolchainMismatch,
    BuildPlanMismatch,
    TargetIsolationMismatch,
    GateAttemptAlias,
    ExitMismatch,
    PipeClosureMissing,
    PeerBindingMismatch,
    NonceMismatch,
    CommitPermitMissing,
    PublicationStateMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct P8QualityDigest(String);

impl P8QualityDigest {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, P8QualityContractFailure> {
        let value = value.into();
        if is_sha256(&value) {
            Ok(Self(value))
        } else {
            Err(P8QualityContractFailure::DigestInvalid)
        }
    }

    fn derive(domain: &str, value: &impl Serialize) -> Self {
        let bytes = serde_json::to_vec(value)
            .expect("P8 quality canonical serialization must be infallible");
        Self(format!(
            "{SHA256_PREFIX}{}",
            domain_separated_sha256(domain, &[bytes.as_slice()])
        ))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for P8QualityDigest {
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct P8QualityId(String);

impl P8QualityId {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, P8QualityContractFailure> {
        let value = value.into();
        if is_canonical_id(&value) {
            Ok(Self(value))
        } else {
            Err(P8QualityContractFailure::IdentityInvalid)
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for P8QualityId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if is_canonical_id(&value) {
            Ok(Self(value))
        } else {
            Err(D::Error::custom("invalid P8 quality id"))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct P8RuntimeBudgetReportIdV1(String);

impl P8RuntimeBudgetReportIdV1 {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, P8QualityContractFailure> {
        let value = value.into();
        if value.strip_prefix("rtb-v2-").is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        }) {
            Ok(Self(value))
        } else {
            Err(P8QualityContractFailure::IdentityInvalid)
        }
    }
}

impl<'de> Deserialize<'de> for P8RuntimeBudgetReportIdV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(|_| D::Error::custom("invalid RuntimeBudgetReport.report_id"))
    }
}

macro_rules! p8_quality_domain_ref {
    ($name:ident, $prefix:literal, $domain:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub(crate) struct $name(String);

        impl $name {
            fn derive(value: &impl Serialize) -> Self {
                let bytes = serde_json::to_vec(value)
                    .expect("P8 quality typed identity serialization must be infallible");
                Self(format!(
                    "{}{}",
                    $prefix,
                    domain_separated_sha256($domain, &[bytes.as_slice()])
                ))
            }

            #[cfg(test)]
            fn derive_for_test(value: &str) -> Self {
                Self::derive(&value)
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

p8_quality_domain_ref!(
    P8HardPolicyRef,
    "p8_hard_policy:sha256:",
    "p8_quality_hard_policy_v1"
);
p8_quality_domain_ref!(
    P8HypothesisRegistryRef,
    "p8_hypothesis_registry:sha256:",
    "p8_quality_hypothesis_registry_v1"
);
p8_quality_domain_ref!(
    P8ProtocolLockRef,
    "p8_protocol_lock:sha256:",
    "p8_quality_protocol_lock_v1"
);
p8_quality_domain_ref!(
    P8ThresholdLockRef,
    "p8_threshold_lock:sha256:",
    "p8_quality_threshold_lock_v1"
);
p8_quality_domain_ref!(
    P8PolicyAnchorRef,
    "p8_policy_anchor:sha256:",
    "p8_quality_policy_anchor_v1"
);
p8_quality_domain_ref!(
    P8TrustedExecutionReceiptRef,
    "p8_trusted_execution_receipt:sha256:",
    "p8_quality_trusted_execution_receipt_v1"
);
p8_quality_domain_ref!(
    P8TrialClosureRef,
    "p8_trial_closure:sha256:",
    "p8_quality_trial_closure_v1"
);
p8_quality_domain_ref!(
    P8BaselineManifestRef,
    "p8_baseline_manifest:sha256:",
    "p8_quality_baseline_manifest_v1"
);
p8_quality_domain_ref!(
    P8ExperimentPlanRef,
    "p8_experiment_plan:sha256:",
    "p8_quality_experiment_plan_v1"
);
p8_quality_domain_ref!(
    P8QualityRunRef,
    "p8_quality_run:sha256:",
    "p8_quality_run_v1"
);
p8_quality_domain_ref!(
    P8TrustedExecutionLeaseRef,
    "p8_trusted_execution_lease:sha256:",
    "p8_trusted_execution_lease_v1"
);
p8_quality_domain_ref!(
    P8HardGateEvaluationRef,
    "p8_hard_gate_evaluation:sha256:",
    "p8_hard_gate_evaluation_v1"
);
p8_quality_domain_ref!(
    P8ThresholdEvaluationRef,
    "p8_threshold_evaluation:sha256:",
    "p8_threshold_evaluation_v1"
);
p8_quality_domain_ref!(
    P8SemanticExecutionReceiptV2Ref,
    "p8_semantic_execution_receipt_v2:sha256:",
    "p8_semantic_execution_receipt_v2"
);
p8_quality_domain_ref!(
    P8QualityTrialSetRef,
    "p8_quality_trial_set:sha256:",
    "p8_quality_trial_set_v1"
);
p8_quality_domain_ref!(
    P8NoMemoryProjectionProofRef,
    "p8_no_memory_projection_proof:sha256:",
    "p8_no_memory_projection_proof_v1"
);
p8_quality_domain_ref!(
    P8PublicSafeOutputReceiptRef,
    "p8_public_safe_output_receipt:sha256:",
    "p8_public_safe_output_receipt_v1"
);
p8_quality_domain_ref!(
    P8ProviderSafeProjectionRef,
    "p8_provider_safe_projection:sha256:",
    "p8_provider_safe_projection_v1"
);
p8_quality_domain_ref!(
    P8ModelRequestRef,
    "p8_model_request:sha256:",
    "p8_model_request_v1"
);
p8_quality_domain_ref!(
    P8PairedJudgeReceiptRef,
    "p8_paired_judge_receipt:sha256:",
    "p8_paired_judge_receipt_v1"
);
p8_quality_domain_ref!(
    P8SafetyProofReceiptRef,
    "p8_safety_proof_receipt:sha256:",
    "p8_safety_proof_receipt_v1"
);
p8_quality_domain_ref!(
    P8CompositionReceiptRef,
    "p8_composition_receipt:sha256:",
    "p8_composition_receipt_v1"
);
p8_quality_domain_ref!(
    P8ModelExecutionReceiptRef,
    "p8_model_execution_receipt:sha256:",
    "p8_model_execution_receipt_v1"
);
p8_quality_domain_ref!(
    P8BenchmarkJoinExecutionReceiptRef,
    "p8_benchmark_join_execution_receipt:sha256:",
    "p8_benchmark_join_execution_receipt_v1"
);
p8_quality_domain_ref!(
    P8JudgeExecutionReceiptRef,
    "p8_judge_execution_receipt:sha256:",
    "p8_judge_execution_receipt_v1"
);
p8_quality_domain_ref!(
    P8AttemptLatencyReceiptRef,
    "p8_attempt_latency_receipt:sha256:",
    "p8_attempt_latency_receipt_v1"
);
p8_quality_domain_ref!(
    P8QuestionLatencyReceiptRef,
    "p8_question_latency_receipt:sha256:",
    "p8_question_latency_receipt_v1"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8QualityPurpose {
    BaselineEstablishment,
    QualityCandidate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ExecutionMode {
    FixtureContract,
    TrustedFull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8QualityArmKind {
    NoMemory,
    PublicReference,
    FrozenP84Baseline,
    P8Candidate,
}

impl P8QualityArmKind {
    const BASELINE: [Self; 3] = [
        Self::NoMemory,
        Self::PublicReference,
        Self::FrozenP84Baseline,
    ];
    const CANDIDATE: [Self; 4] = [
        Self::NoMemory,
        Self::PublicReference,
        Self::FrozenP84Baseline,
        Self::P8Candidate,
    ];

    fn expected_for(purpose: P8QualityPurpose) -> &'static [Self] {
        match purpose {
            P8QualityPurpose::BaselineEstablishment => &Self::BASELINE,
            P8QualityPurpose::QualityCandidate => &Self::CANDIDATE,
        }
    }

    fn beetle_arms_for(purpose: P8QualityPurpose) -> &'static [Self] {
        match purpose {
            P8QualityPurpose::BaselineEstablishment => &[P8QualityArmKind::FrozenP84Baseline],
            P8QualityPurpose::QualityCandidate => &[
                P8QualityArmKind::FrozenP84Baseline,
                P8QualityArmKind::P8Candidate,
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8QualityHardGateId {
    IneligibleOwnerProjection,
    NonCurrentMaterialProjection,
    CrossSubjectPrivateSoulLeak,
    RawProcedureCredentialPathOrPrematureGoldPersistence,
    UnexpectedRuntimeOrIntegrityFailure,
    FullStoreScanSecondPlatformOrLiveFallback,
    PostImageClosureCoverage,
    UpdateLineageViolation,
    UnmetPremiseProcedureDelivery,
    RequiredReportOperatorCoverage,
    ProfileBudgetRenderCeilingBreach,
}

impl P8QualityHardGateId {
    pub(crate) const ALL: [Self; 11] = [
        Self::IneligibleOwnerProjection,
        Self::NonCurrentMaterialProjection,
        Self::CrossSubjectPrivateSoulLeak,
        Self::RawProcedureCredentialPathOrPrematureGoldPersistence,
        Self::UnexpectedRuntimeOrIntegrityFailure,
        Self::FullStoreScanSecondPlatformOrLiveFallback,
        Self::PostImageClosureCoverage,
        Self::UpdateLineageViolation,
        Self::UnmetPremiseProcedureDelivery,
        Self::RequiredReportOperatorCoverage,
        Self::ProfileBudgetRenderCeilingBreach,
    ];

    const fn canonical_requirement(self) -> P8HardGateRequirement {
        match self {
            Self::PostImageClosureCoverage | Self::RequiredReportOperatorCoverage => {
                P8HardGateRequirement::ExactFull
            }
            Self::IneligibleOwnerProjection
            | Self::NonCurrentMaterialProjection
            | Self::CrossSubjectPrivateSoulLeak
            | Self::RawProcedureCredentialPathOrPrematureGoldPersistence
            | Self::UnexpectedRuntimeOrIntegrityFailure
            | Self::FullStoreScanSecondPlatformOrLiveFallback
            | Self::UpdateLineageViolation
            | Self::UnmetPremiseProcedureDelivery
            | Self::ProfileBudgetRenderCeilingBreach => P8HardGateRequirement::ExactZero,
        }
    }

    const fn validator_contract_ref(self) -> &'static str {
        match self {
            Self::IneligibleOwnerProjection => "sdk.governed-report.ineligible-owner-zero.v1",
            Self::NonCurrentMaterialProjection => {
                "sdk.governed-report.non-current-material-zero.v1"
            }
            Self::CrossSubjectPrivateSoulLeak => {
                "sdk.governed-report.cross-subject-private-soul-zero.v1"
            }
            Self::RawProcedureCredentialPathOrPrematureGoldPersistence => {
                "replay.artifact.raw-procedure-credential-path-gold-zero.v1"
            }
            Self::UnexpectedRuntimeOrIntegrityFailure => {
                "replay.quality.unexpected-runtime-integrity-zero.v1"
            }
            Self::FullStoreScanSecondPlatformOrLiveFallback => {
                "sdk.governed-report.full-scan-second-platform-live-fallback-zero.v1"
            }
            Self::PostImageClosureCoverage => "store.post-image-closure-coverage-full.v1",
            Self::UpdateLineageViolation => "core.memory-update-lineage-violation-zero.v1",
            Self::UnmetPremiseProcedureDelivery => {
                "sdk.governed-report.unmet-premise-procedure-delivery-zero.v1"
            }
            Self::RequiredReportOperatorCoverage => {
                "replay.quality.required-report-operator-coverage-full.v1"
            }
            Self::ProfileBudgetRenderCeilingBreach => {
                "sdk.governed-report.profile-budget-render-breach-zero.v1"
            }
        }
    }

    const fn validator_source_fingerprint(self) -> &'static str {
        match self {
            Self::PostImageClosureCoverage => P8_POST_IMAGE_VALIDATOR_FINGERPRINT,
            Self::UpdateLineageViolation => P8_CORE_VALIDATOR_FINGERPRINT,
            Self::RawProcedureCredentialPathOrPrematureGoldPersistence
            | Self::UnexpectedRuntimeOrIntegrityFailure
            | Self::RequiredReportOperatorCoverage => P8_REPLAY_VALIDATOR_FINGERPRINT,
            Self::IneligibleOwnerProjection
            | Self::NonCurrentMaterialProjection
            | Self::CrossSubjectPrivateSoulLeak
            | Self::FullStoreScanSecondPlatformOrLiveFallback
            | Self::UnmetPremiseProcedureDelivery
            | Self::ProfileBudgetRenderCeilingBreach => P8_SDK_VALIDATOR_FINGERPRINT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8HardGateRequirement {
    ExactZero,
    ExactFull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ValidatorSourceAttestationV1 {
    WorkspaceSource,
    PackagedUnattested,
}

impl P8ValidatorSourceAttestationV1 {
    fn compiled() -> Self {
        match P8_VALIDATOR_SOURCE_ATTESTATION {
            "workspace_source" => Self::WorkspaceSource,
            "packaged_unattested" => Self::PackagedUnattested,
            _ => panic!("build.rs emitted an unsupported P8 validator source attestation"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityHardGateRuleV1 {
    gate_id: P8QualityHardGateId,
    requirement: P8HardGateRequirement,
    validator_contract_ref: String,
    validator_source_fingerprint: P8QualityDigest,
    validator_source_attestation: P8ValidatorSourceAttestationV1,
    validator_contract_digest: P8QualityDigest,
}

impl P8QualityHardGateRuleV1 {
    fn canonical(gate_id: P8QualityHardGateId) -> Self {
        let validator_contract_ref = gate_id.validator_contract_ref().to_string();
        let requirement = gate_id.canonical_requirement();
        let validator_source_fingerprint = P8QualityDigest::parse(format!(
            "{SHA256_PREFIX}{}",
            gate_id.validator_source_fingerprint()
        ))
        .expect("build.rs must emit lower-case SHA-256 P8 validator fingerprints");
        let validator_source_attestation = P8ValidatorSourceAttestationV1::compiled();
        let validator_contract_digest = P8QualityDigest::derive(
            "p8_quality_hard_gate_validator_contract_v1",
            &(
                gate_id,
                requirement,
                &validator_contract_ref,
                &validator_source_fingerprint,
                validator_source_attestation,
            ),
        );
        Self {
            gate_id,
            requirement,
            validator_contract_ref,
            validator_source_fingerprint,
            validator_source_attestation,
            validator_contract_digest,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityHardPolicyV1 {
    schema: String,
    rules: Vec<P8QualityHardGateRuleV1>,
    policy_digest: P8HardPolicyRef,
}

impl P8QualityHardPolicyV1 {
    pub(crate) fn canonical() -> Self {
        let rules = P8QualityHardGateId::ALL
            .into_iter()
            .map(P8QualityHardGateRuleV1::canonical)
            .collect::<Vec<_>>();
        let mut value = Self {
            schema: P8_HARD_POLICY_SCHEMA.into(),
            rules,
            policy_digest: P8HardPolicyRef::derive(&()),
        };
        value.policy_digest = value.derived_digest();
        value
    }

    pub(crate) fn rules(&self) -> &[P8QualityHardGateRuleV1] {
        &self.rules
    }

    pub(crate) fn policy_digest(&self) -> &P8HardPolicyRef {
        &self.policy_digest
    }

    pub(crate) fn requirement_for(
        &self,
        gate_id: P8QualityHardGateId,
    ) -> Option<P8HardGateRequirement> {
        self.rules
            .iter()
            .find(|rule| rule.gate_id == gate_id)
            .map(|rule| rule.requirement)
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_HARD_POLICY_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        let canonical_rules = P8QualityHardGateId::ALL
            .into_iter()
            .map(P8QualityHardGateRuleV1::canonical)
            .collect::<Vec<_>>();
        if self.rules != canonical_rules {
            failures.push(P8QualityContractFailure::HardPolicyMismatch);
        }
        if self.policy_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8HardPolicyRef {
        P8HardPolicyRef::derive(&(&self.schema, &self.rules))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8TargetThresholdSlotV1 {
    AbsoluteFloorAndImprovement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8UntouchedThresholdSlotV1 {
    NonInferiorityMargin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum P8HypothesisRoleV1 {
    Target {
        threshold_slot: P8TargetThresholdSlotV1,
    },
    Untouched {
        threshold_slot: P8UntouchedThresholdSlotV1,
    },
}

impl P8HypothesisRoleV1 {
    pub(crate) const fn target() -> Self {
        Self::Target {
            threshold_slot: P8TargetThresholdSlotV1::AbsoluteFloorAndImprovement,
        }
    }

    pub(crate) const fn untouched() -> Self {
        Self::Untouched {
            threshold_slot: P8UntouchedThresholdSlotV1::NonInferiorityMargin,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8HypothesisAxisV1 {
    Family(P8BenchmarkFamily),
    Stratum(P8DatasetStratum),
    Capability(P8CapabilitySlice),
    Task(P8TaskKind),
    Query(P8QueryOperationKind),
    Temporal(P8TemporalCorpusSlice),
    Safety(P8SafetySlice),
    Profile(ProfileId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8MembershipExclusionReasonV1 {
    OutsideRegisteredSlice,
    NotApplicableToCapability,
    NotApplicableToProfile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum P8HypothesisMembershipDispositionV1 {
    Included,
    Excluded {
        reason: P8MembershipExclusionReasonV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8HypothesisQuestionMembershipV1 {
    question_id: P8QualityId,
    ordered_question_set_digest: P8QualityDigest,
    disposition: P8HypothesisMembershipDispositionV1,
}

impl P8HypothesisQuestionMembershipV1 {
    pub(crate) fn included(
        question_id: P8QualityId,
        ordered_question_set_digest: P8QualityDigest,
    ) -> Self {
        Self {
            question_id,
            ordered_question_set_digest,
            disposition: P8HypothesisMembershipDispositionV1::Included,
        }
    }

    pub(crate) fn excluded(
        question_id: P8QualityId,
        ordered_question_set_digest: P8QualityDigest,
        reason: P8MembershipExclusionReasonV1,
    ) -> Self {
        Self {
            question_id,
            ordered_question_set_digest,
            disposition: P8HypothesisMembershipDispositionV1::Excluded { reason },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityHypothesisSpecV1 {
    hypothesis_id: P8QualityId,
    role: P8HypothesisRoleV1,
    axes: Vec<P8HypothesisAxisV1>,
    memberships: Vec<P8HypothesisQuestionMembershipV1>,
}

impl P8QualityHypothesisSpecV1 {
    pub(crate) fn new(
        hypothesis_id: P8QualityId,
        role: P8HypothesisRoleV1,
        mut axes: Vec<P8HypothesisAxisV1>,
        memberships: Vec<P8HypothesisQuestionMembershipV1>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        axes.sort();
        let value = Self {
            hypothesis_id,
            role,
            axes,
            memberships,
        };
        let failures = value.validate_local();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    fn validate_local(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.axes.is_empty() || !is_strict_sorted_unique(&self.axes) {
            failures.push(P8QualityContractFailure::HypothesisMismatch);
        }
        let membership_ids = self
            .memberships
            .iter()
            .map(|membership| &membership.question_id)
            .collect::<BTreeSet<_>>();
        if self.memberships.is_empty()
            || membership_ids.len() != self.memberships.len()
            || !self.memberships.iter().any(|membership| {
                matches!(
                    membership.disposition,
                    P8HypothesisMembershipDispositionV1::Included
                )
            })
        {
            failures.push(P8QualityContractFailure::HypothesisMismatch);
        }
        failures
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QuestionEvaluationExpectationV1 {
    question_id: P8QualityId,
    ordered_question_set_digest: P8QualityDigest,
    expected_capability_outcomes: P8ExpectedCapabilityOutcomesV1,
}

impl P8QuestionEvaluationExpectationV1 {
    pub(crate) fn new(
        question_id: P8QualityId,
        ordered_question_set_digest: P8QualityDigest,
        expected_capability_outcomes: P8ExpectedCapabilityOutcomesV1,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let value = Self {
            question_id,
            ordered_question_set_digest,
            expected_capability_outcomes,
        };
        if expected_capability_outcomes_are_consistent(&value.expected_capability_outcomes) {
            Ok(value)
        } else {
            Err(vec![P8QualityContractFailure::OutcomeMismatch])
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityHypothesisRegistryV1 {
    schema: String,
    ordered_questions: Vec<P8QualityId>,
    question_expectations: Vec<P8QuestionEvaluationExpectationV1>,
    hypotheses: Vec<P8QualityHypothesisSpecV1>,
    ordered_question_set_digest: P8QualityDigest,
    correction_family_digest: P8QualityDigest,
    registry_digest: P8HypothesisRegistryRef,
}

impl P8QualityHypothesisRegistryV1 {
    pub(crate) fn build(
        ordered_questions: Vec<P8QualityId>,
        question_expectations: Vec<P8QuestionEvaluationExpectationV1>,
        hypotheses: Vec<P8QualityHypothesisSpecV1>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let ordered_question_set_digest =
            P8QualityDigest::derive("p8_quality_ordered_question_set_v1", &ordered_questions);
        let correction_family_digest =
            P8QualityDigest::derive("p8_quality_hypothesis_correction_family_v1", &hypotheses);
        let mut value = Self {
            schema: P8_HYPOTHESIS_REGISTRY_SCHEMA.into(),
            ordered_questions,
            question_expectations,
            hypotheses,
            ordered_question_set_digest,
            correction_family_digest,
            registry_digest: P8HypothesisRegistryRef::derive(&()),
        };
        value.registry_digest = value.derived_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_HYPOTHESIS_REGISTRY_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        let question_set = self.ordered_questions.iter().collect::<BTreeSet<_>>();
        if self.ordered_questions.is_empty() || question_set.len() != self.ordered_questions.len() {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        let expected_question_order = self
            .question_expectations
            .iter()
            .map(|expectation| &expectation.question_id)
            .collect::<Vec<_>>();
        if expected_question_order != self.ordered_questions.iter().collect::<Vec<_>>()
            || self.question_expectations.iter().any(|expectation| {
                expectation.ordered_question_set_digest != self.ordered_question_set_digest
                    || !expected_capability_outcomes_are_consistent(
                        &expectation.expected_capability_outcomes,
                    )
            })
        {
            failures.push(P8QualityContractFailure::OutcomeMismatch);
        }
        let hypothesis_ids = self
            .hypotheses
            .iter()
            .map(|hypothesis| &hypothesis.hypothesis_id)
            .collect::<BTreeSet<_>>();
        if self.hypotheses.is_empty() || hypothesis_ids.len() != self.hypotheses.len() {
            failures.push(P8QualityContractFailure::DuplicateEntry);
        }
        for hypothesis in &self.hypotheses {
            failures.extend(hypothesis.validate_local());
            let membership_order = hypothesis
                .memberships
                .iter()
                .map(|membership| &membership.question_id)
                .collect::<Vec<_>>();
            let expected_order = self.ordered_questions.iter().collect::<Vec<_>>();
            if membership_order != expected_order
                || hypothesis.memberships.iter().any(|membership| {
                    membership.ordered_question_set_digest != self.ordered_question_set_digest
                })
            {
                failures.push(P8QualityContractFailure::CoverageMismatch);
            }
        }
        if self.ordered_question_set_digest
            != P8QualityDigest::derive(
                "p8_quality_ordered_question_set_v1",
                &self.ordered_questions,
            )
            || self.correction_family_digest
                != P8QualityDigest::derive(
                    "p8_quality_hypothesis_correction_family_v1",
                    &self.hypotheses,
                )
            || self.registry_digest != self.derived_digest()
        {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn registry_digest(&self) -> &P8HypothesisRegistryRef {
        &self.registry_digest
    }

    pub(crate) fn ordered_question_ids_digest(&self) -> &P8QualityDigest {
        &self.ordered_question_set_digest
    }

    fn expected_outcomes_for(
        &self,
        question_id: &P8QualityId,
    ) -> Option<&P8ExpectedCapabilityOutcomesV1> {
        self.question_expectations
            .iter()
            .find(|expectation| &expectation.question_id == question_id)
            .map(|expectation| &expectation.expected_capability_outcomes)
    }

    fn derived_digest(&self) -> P8HypothesisRegistryRef {
        P8HypothesisRegistryRef::derive(&(
            &self.schema,
            &self.ordered_questions,
            &self.question_expectations,
            &self.hypotheses,
            &self.ordered_question_set_digest,
            &self.correction_family_digest,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ExpectedRefusalReasonV1 {
    NotApplicable,
    PrivacyBlocked,
    PremiseUnsatisfied,
    PremiseUnknown,
    PremiseExpired,
    Forgotten,
    Invalidated,
    Obsolete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8UnexpectedRefusalReasonV1 {
    ProviderRefused,
    JudgeRefused,
    RuntimeBlocked,
    EmptyResponse,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum P8AccuracyOutcomeV1 {
    Correct,
    Incorrect,
    ExpectedRefusal { reason: P8ExpectedRefusalReasonV1 },
    UnexpectedRefusal { reason: P8UnexpectedRefusalReasonV1 },
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum P8ExpectedAccuracyV1 {
    Correct,
    ExpectedRefusal { reason: P8ExpectedRefusalReasonV1 },
    NotApplicable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8MemoryUseOutcomeV1 {
    NotApplicable,
    CurrentUsed,
    CurrentRejected,
    ObsoleteRejected,
    ObsoleteUsed,
    InvalidatedRejected,
    InvalidatedUsed,
    ForgottenRejected,
    ForgottenUsed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8LineageFailureV1 {
    Cycle,
    Gap,
    ScopeMismatch,
    PrivacyMismatch,
    DepthExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum P8LineageOutcomeV1 {
    NotApplicable,
    Exact,
    Inexact { reason: P8LineageFailureV1 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8PremiseOutcomeV1 {
    NotApplicable,
    SatisfiedDelivered,
    UnsatisfiedRefused,
    UnknownRefused,
    ExpiredRefused,
    PrivacyBlockedRefused,
    RequiredUnmetDelivered,
    SatisfiedRefused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ProceduralOutcomeV1 {
    NotApplicable,
    SafeEvidenceDelivered,
    ExpectedRefusal,
    MissingEvidence,
    UnsafeEvidenceDelivered,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ExpectedCapabilityOutcomesV1 {
    accuracy: P8ExpectedAccuracyV1,
    memory_use: P8MemoryUseOutcomeV1,
    lineage: P8LineageOutcomeV1,
    premise: P8PremiseOutcomeV1,
    procedural: P8ProceduralOutcomeV1,
}

impl P8ExpectedCapabilityOutcomesV1 {
    pub(crate) const fn current_procedural() -> Self {
        Self {
            accuracy: P8ExpectedAccuracyV1::Correct,
            memory_use: P8MemoryUseOutcomeV1::CurrentUsed,
            lineage: P8LineageOutcomeV1::NotApplicable,
            premise: P8PremiseOutcomeV1::NotApplicable,
            procedural: P8ProceduralOutcomeV1::SafeEvidenceDelivered,
        }
    }

    pub(crate) const fn obsolete_rejected() -> Self {
        Self {
            accuracy: P8ExpectedAccuracyV1::ExpectedRefusal {
                reason: P8ExpectedRefusalReasonV1::Obsolete,
            },
            memory_use: P8MemoryUseOutcomeV1::ObsoleteRejected,
            lineage: P8LineageOutcomeV1::NotApplicable,
            premise: P8PremiseOutcomeV1::NotApplicable,
            procedural: P8ProceduralOutcomeV1::NotApplicable,
        }
    }

    pub(crate) fn into_actual(self) -> P8ActualCapabilityOutcomesV1 {
        P8ActualCapabilityOutcomesV1 {
            memory_use: self.memory_use,
            lineage: self.lineage,
            premise: self.premise,
            procedural: self.procedural,
        }
    }
}

fn expected_capability_outcomes_are_consistent(expected: &P8ExpectedCapabilityOutcomesV1) -> bool {
    let all_capability_axes_are_not_applicable =
        matches!(expected.memory_use, P8MemoryUseOutcomeV1::NotApplicable)
            && matches!(expected.lineage, P8LineageOutcomeV1::NotApplicable)
            && matches!(expected.premise, P8PremiseOutcomeV1::NotApplicable)
            && matches!(expected.procedural, P8ProceduralOutcomeV1::NotApplicable);
    matches!(expected.accuracy, P8ExpectedAccuracyV1::NotApplicable)
        == all_capability_axes_are_not_applicable
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ActualCapabilityOutcomesV1 {
    memory_use: P8MemoryUseOutcomeV1,
    lineage: P8LineageOutcomeV1,
    premise: P8PremiseOutcomeV1,
    procedural: P8ProceduralOutcomeV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8CapabilityScoreV1 {
    denominator: u64,
    successes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8CapabilityScoreBreakdownV1 {
    memory_use: P8CapabilityScoreV1,
    lineage: P8CapabilityScoreV1,
    premise: P8CapabilityScoreV1,
    procedural: P8CapabilityScoreV1,
}

impl P8CapabilityScoreV1 {
    pub(crate) const fn denominator(&self) -> u64 {
        self.denominator
    }

    pub(crate) const fn successes(&self) -> u64 {
        self.successes
    }
}

pub(crate) fn derive_capability_score(
    registry: &P8QualityHypothesisRegistryV1,
    hypothesis_id: &P8QualityId,
    actual: &BTreeMap<P8QualityId, P8ActualCapabilityOutcomesV1>,
    accuracy: &BTreeMap<P8QualityId, P8AccuracyOutcomeV1>,
) -> Result<P8CapabilityScoreBreakdownV1, Vec<P8QualityContractFailure>> {
    let registry_failures = registry.validate_contract();
    if !registry_failures.is_empty() {
        return Err(registry_failures);
    }
    let hypothesis = registry
        .hypotheses
        .iter()
        .find(|hypothesis| &hypothesis.hypothesis_id == hypothesis_id)
        .ok_or_else(|| vec![P8QualityContractFailure::HypothesisMismatch])?;
    let mut included = Vec::new();
    for membership in &hypothesis.memberships {
        if matches!(
            membership.disposition,
            P8HypothesisMembershipDispositionV1::Included
        ) {
            let Some(expected) = registry.expected_outcomes_for(&membership.question_id) else {
                return Err(vec![P8QualityContractFailure::OutcomeMismatch]);
            };
            included.push((membership.question_id.clone(), expected));
        }
    }
    let included_ids = included
        .iter()
        .map(|(question_id, _)| question_id.clone())
        .collect::<BTreeSet<_>>();
    if included.is_empty()
        || actual.keys().cloned().collect::<BTreeSet<_>>() != included_ids
        || accuracy.keys().cloned().collect::<BTreeSet<_>>()
            != included
                .iter()
                .map(|(question_id, _)| question_id.clone())
                .collect()
    {
        return Err(vec![P8QualityContractFailure::DenominatorMismatch]);
    }
    let mut memory_use = P8CapabilityScoreV1 {
        denominator: 0,
        successes: 0,
    };
    let mut lineage = memory_use.clone();
    let mut premise = memory_use.clone();
    let mut procedural = memory_use.clone();
    for (question_id, expected) in included {
        let actual = &actual[&question_id];
        let accuracy = &accuracy[&question_id];
        if !actual_applicability_matches(expected, accuracy, actual) {
            return Err(vec![P8QualityContractFailure::OutcomeMismatch]);
        }
        let accuracy_succeeds = expected_accuracy_succeeds(expected, accuracy);
        accumulate_axis(
            &mut memory_use,
            true,
            expected.memory_use == actual.memory_use
                && memory_use_outcome_succeeds(actual.memory_use)
                && (!matches!(actual.memory_use, P8MemoryUseOutcomeV1::CurrentUsed)
                    || accuracy_succeeds),
        )?;
        accumulate_axis(
            &mut lineage,
            true,
            expected.lineage == actual.lineage && lineage_outcome_succeeds(actual.lineage),
        )?;
        accumulate_axis(
            &mut premise,
            true,
            expected.premise == actual.premise && premise_outcome_succeeds(actual.premise),
        )?;
        accumulate_axis(
            &mut procedural,
            true,
            expected.procedural == actual.procedural
                && procedural_outcome_succeeds(actual.procedural)
                && (!matches!(
                    actual.procedural,
                    P8ProceduralOutcomeV1::SafeEvidenceDelivered
                ) || accuracy_succeeds),
        )?;
    }
    Ok(P8CapabilityScoreBreakdownV1 {
        memory_use,
        lineage,
        premise,
        procedural,
    })
}

fn actual_applicability_matches(
    expected: &P8ExpectedCapabilityOutcomesV1,
    accuracy: &P8AccuracyOutcomeV1,
    actual: &P8ActualCapabilityOutcomesV1,
) -> bool {
    let expected_accuracy_is_na = matches!(expected.accuracy, P8ExpectedAccuracyV1::NotApplicable);
    let actual_accuracy_is_na = matches!(accuracy, P8AccuracyOutcomeV1::NotApplicable);
    expected_accuracy_is_na == actual_accuracy_is_na
        && capability_applicability_matches(expected, actual)
}

fn capability_applicability_matches(
    expected: &P8ExpectedCapabilityOutcomesV1,
    actual: &P8ActualCapabilityOutcomesV1,
) -> bool {
    matches!(expected.memory_use, P8MemoryUseOutcomeV1::NotApplicable)
        == matches!(actual.memory_use, P8MemoryUseOutcomeV1::NotApplicable)
        && matches!(expected.lineage, P8LineageOutcomeV1::NotApplicable)
            == matches!(actual.lineage, P8LineageOutcomeV1::NotApplicable)
        && matches!(expected.premise, P8PremiseOutcomeV1::NotApplicable)
            == matches!(actual.premise, P8PremiseOutcomeV1::NotApplicable)
        && matches!(expected.procedural, P8ProceduralOutcomeV1::NotApplicable)
            == matches!(actual.procedural, P8ProceduralOutcomeV1::NotApplicable)
}

fn expected_accuracy_succeeds(
    expected: &P8ExpectedCapabilityOutcomesV1,
    accuracy: &P8AccuracyOutcomeV1,
) -> bool {
    match (&expected.accuracy, accuracy) {
        (P8ExpectedAccuracyV1::Correct, P8AccuracyOutcomeV1::Correct) => true,
        (
            P8ExpectedAccuracyV1::ExpectedRefusal { reason: expected },
            P8AccuracyOutcomeV1::ExpectedRefusal { reason: actual },
        ) => expected == actual,
        (P8ExpectedAccuracyV1::NotApplicable, P8AccuracyOutcomeV1::NotApplicable) => true,
        _ => false,
    }
}

fn expected_accuracy_outcome(expected: &P8ExpectedAccuracyV1) -> P8AccuracyOutcomeV1 {
    match expected {
        P8ExpectedAccuracyV1::Correct => P8AccuracyOutcomeV1::Correct,
        P8ExpectedAccuracyV1::ExpectedRefusal { reason } => P8AccuracyOutcomeV1::ExpectedRefusal {
            reason: reason.clone(),
        },
        P8ExpectedAccuracyV1::NotApplicable => P8AccuracyOutcomeV1::NotApplicable,
    }
}

fn accumulate_axis(
    score: &mut P8CapabilityScoreV1,
    applicable: bool,
    succeeds: bool,
) -> Result<(), Vec<P8QualityContractFailure>> {
    if applicable {
        score.denominator = score
            .denominator
            .checked_add(1)
            .ok_or_else(|| vec![P8QualityContractFailure::ArithmeticOverflow])?;
        if succeeds {
            score.successes = score
                .successes
                .checked_add(1)
                .ok_or_else(|| vec![P8QualityContractFailure::ArithmeticOverflow])?;
        }
    }
    Ok(())
}

fn memory_use_outcome_succeeds(outcome: P8MemoryUseOutcomeV1) -> bool {
    matches!(
        outcome,
        P8MemoryUseOutcomeV1::NotApplicable
            | P8MemoryUseOutcomeV1::CurrentUsed
            | P8MemoryUseOutcomeV1::ObsoleteRejected
            | P8MemoryUseOutcomeV1::InvalidatedRejected
            | P8MemoryUseOutcomeV1::ForgottenRejected
    )
}

fn lineage_outcome_succeeds(outcome: P8LineageOutcomeV1) -> bool {
    matches!(
        outcome,
        P8LineageOutcomeV1::NotApplicable | P8LineageOutcomeV1::Exact
    )
}

fn premise_outcome_succeeds(outcome: P8PremiseOutcomeV1) -> bool {
    matches!(
        outcome,
        P8PremiseOutcomeV1::NotApplicable
            | P8PremiseOutcomeV1::SatisfiedDelivered
            | P8PremiseOutcomeV1::UnsatisfiedRefused
            | P8PremiseOutcomeV1::UnknownRefused
            | P8PremiseOutcomeV1::ExpiredRefused
            | P8PremiseOutcomeV1::PrivacyBlockedRefused
    )
}

fn procedural_outcome_succeeds(outcome: P8ProceduralOutcomeV1) -> bool {
    matches!(
        outcome,
        P8ProceduralOutcomeV1::NotApplicable
            | P8ProceduralOutcomeV1::SafeEvidenceDelivered
            | P8ProceduralOutcomeV1::ExpectedRefusal
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ThresholdStateV1 {
    UnfrozenExpected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8CandidateQualityDecisionV1 {
    QualityFailed,
    QualityPassed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum P8QualityOperatorStateV1 {
    NotRun,
    Blocked {
        reason: P8QualityId,
    },
    StructurallyInvalid {
        reason: P8QualityId,
    },
    ExecutedBaselineRejected {
        hard_gate_failures: Vec<P8QualityHardGateId>,
        release_eligible: bool,
    },
    ExecutedValidBaseline {
        threshold_state: P8ThresholdStateV1,
        release_eligible: bool,
    },
    ExecutedValidCandidate {
        threshold_digest: P8ThresholdLockRef,
        decision: P8CandidateQualityDecisionV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8HardGateObservationV1 {
    gate_id: P8QualityHardGateId,
    observed_count: u64,
    required_total_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8HardGateEvaluationV1 {
    schema: String,
    evidence_scope: P8HardGateEvidenceScopeV1,
    hard_policy: P8QualityHardPolicyV1,
    resource_closure: Box<P8QualityResourceClosureV1>,
    fixture_failed_gates: Vec<P8QualityHardGateId>,
    observations: Vec<P8HardGateObservationV1>,
    hard_gate_failures: Vec<P8QualityHardGateId>,
    evaluation_digest: P8HardGateEvaluationRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8HardGateEvidenceScopeV1 {
    FixtureContractOnlyNoTrustedOwnerEvidence,
}

impl P8HardGateEvaluationV1 {
    pub(crate) fn evaluate(
        policy: &P8QualityHardPolicyV1,
        resource_closure: &P8QualityResourceClosureV1,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        Self::evaluate_fixture(policy, resource_closure, Vec::new())
    }

    #[cfg(test)]
    pub(crate) fn evaluate_fixture_with_failures(
        policy: &P8QualityHardPolicyV1,
        resource_closure: &P8QualityResourceClosureV1,
        fixture_failed_gates: Vec<P8QualityHardGateId>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        Self::evaluate_fixture(policy, resource_closure, fixture_failed_gates)
    }

    fn evaluate_fixture(
        policy: &P8QualityHardPolicyV1,
        resource_closure: &P8QualityResourceClosureV1,
        fixture_failed_gates: Vec<P8QualityHardGateId>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let mut prerequisite_failures = policy.validate_contract();
        prerequisite_failures.extend(resource_closure.validate_contract());
        if policy != &resource_closure.trial_set.experiment_plan.hard_policy {
            prerequisite_failures.push(P8QualityContractFailure::HardPolicyMismatch);
        }
        if !fixture_failed_gates.is_empty() && !is_strict_sorted_unique(&fixture_failed_gates) {
            prerequisite_failures.push(P8QualityContractFailure::DuplicateEntry);
        }
        if !prerequisite_failures.is_empty() {
            return Err(prerequisite_failures);
        }
        let observations = Self::derive_observations(resource_closure, &fixture_failed_gates)
            .map_err(|failure| vec![failure])?;
        let hard_gate_failures = observations
            .iter()
            .filter_map(|observation| {
                let requirement = policy.requirement_for(observation.gate_id)?;
                let passed = match requirement {
                    P8HardGateRequirement::ExactZero => observation.observed_count == 0,
                    P8HardGateRequirement::ExactFull => {
                        observation.required_total_count > 0
                            && observation.observed_count == observation.required_total_count
                    }
                };
                (!passed).then_some(observation.gate_id)
            })
            .collect();
        let mut value = Self {
            schema: P8_HARD_GATE_EVALUATION_SCHEMA.into(),
            evidence_scope: P8HardGateEvidenceScopeV1::FixtureContractOnlyNoTrustedOwnerEvidence,
            hard_policy: policy.clone(),
            resource_closure: Box::new(resource_closure.clone()),
            fixture_failed_gates,
            observations,
            hard_gate_failures,
            evaluation_digest: P8HardGateEvaluationRef::derive(&()),
        };
        value.evaluation_digest = value.derived_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.hard_policy.validate_contract();
        failures.extend(self.resource_closure.validate_contract());
        if self.schema != P8_HARD_GATE_EVALUATION_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.evidence_scope
            != P8HardGateEvidenceScopeV1::FixtureContractOnlyNoTrustedOwnerEvidence
        {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        if !self.fixture_failed_gates.is_empty()
            && !is_strict_sorted_unique(&self.fixture_failed_gates)
        {
            failures.push(P8QualityContractFailure::DuplicateEntry);
        }
        let derived_observations =
            Self::derive_observations(&self.resource_closure, &self.fixture_failed_gates);
        if self.hard_policy != self.resource_closure.trial_set.experiment_plan.hard_policy {
            failures.push(P8QualityContractFailure::HardPolicyMismatch);
        }
        match derived_observations {
            Ok(observations) if self.observations == observations => {}
            Ok(_) => failures.push(P8QualityContractFailure::HardPolicyMismatch),
            Err(failure) => failures.push(failure),
        }
        let actual_ids = self
            .observations
            .iter()
            .map(|observation| observation.gate_id)
            .collect::<Vec<_>>();
        if actual_ids != P8QualityHardGateId::ALL {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        let expected_failures = self
            .observations
            .iter()
            .filter_map(|observation| {
                let requirement = self.hard_policy.requirement_for(observation.gate_id)?;
                let passed = match requirement {
                    P8HardGateRequirement::ExactZero => observation.observed_count == 0,
                    P8HardGateRequirement::ExactFull => {
                        observation.required_total_count > 0
                            && observation.observed_count == observation.required_total_count
                    }
                };
                (!passed).then_some(observation.gate_id)
            })
            .collect::<Vec<_>>();
        if self.hard_gate_failures != expected_failures {
            failures.push(P8QualityContractFailure::HardPolicyMismatch);
        }
        if self.evaluation_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8HardGateEvaluationRef {
        P8HardGateEvaluationRef::derive(&(
            &self.schema,
            self.evidence_scope,
            &self.hard_policy,
            &self.resource_closure,
            &self.fixture_failed_gates,
            &self.observations,
            &self.hard_gate_failures,
        ))
    }

    fn derive_observations(
        resource_closure: &P8QualityResourceClosureV1,
        fixture_failed_gates: &[P8QualityHardGateId],
    ) -> Result<Vec<P8HardGateObservationV1>, P8QualityContractFailure> {
        let trial_set = &resource_closure.trial_set;
        let exact_full_total = u64::try_from(
            trial_set.main_trials.len()
                + trial_set.ablation_trials.len()
                + trial_set.negative_proof_trials.len()
                + resource_closure.observations.len(),
        )
        .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?;
        P8QualityHardGateId::ALL
            .into_iter()
            .map(|gate_id| {
                Ok(match gate_id.canonical_requirement() {
                    P8HardGateRequirement::ExactZero => P8HardGateObservationV1 {
                        gate_id,
                        observed_count: u64::from(fixture_failed_gates.contains(&gate_id)),
                        required_total_count: u64::try_from(trial_set.main_trials.len())
                            .map_err(|_| P8QualityContractFailure::ArithmeticOverflow)?,
                    },
                    P8HardGateRequirement::ExactFull => P8HardGateObservationV1 {
                        gate_id,
                        observed_count: if fixture_failed_gates.contains(&gate_id) {
                            exact_full_total
                                .checked_sub(1)
                                .ok_or(P8QualityContractFailure::ArithmeticOverflow)?
                        } else {
                            exact_full_total
                        },
                        required_total_count: exact_full_total,
                    },
                })
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8CandidateThresholdSlotV1 {
    TargetAbsoluteFloor,
    TargetImprovement,
    UntouchedNonInferiority,
    RenderedCharsFrontier,
    LatencyFrontier,
    PeakDomainMemoryAbsoluteFrontier,
    PeakDomainMemoryDeltaFrontier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ThresholdEvaluationScopeV1 {
    FixtureContractOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8CandidateThresholdEvaluationV1 {
    schema: String,
    threshold: P8QualityThresholdLockV1,
    scope: P8ThresholdEvaluationScopeV1,
    failed_slots: Vec<P8CandidateThresholdSlotV1>,
    evaluation_digest: P8ThresholdEvaluationRef,
}

impl P8CandidateThresholdEvaluationV1 {
    #[cfg(test)]
    pub(crate) fn fixture(
        threshold: &P8QualityThresholdLockV1,
        mut failed_slots: Vec<P8CandidateThresholdSlotV1>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let failures = threshold.validate_contract();
        if !failures.is_empty() {
            return Err(failures);
        }
        failed_slots.sort();
        if !is_strict_sorted_unique(&failed_slots) && !failed_slots.is_empty() {
            return Err(vec![P8QualityContractFailure::DuplicateEntry]);
        }
        let mut value = Self {
            schema: P8_THRESHOLD_EVALUATION_SCHEMA.into(),
            threshold: threshold.clone(),
            scope: P8ThresholdEvaluationScopeV1::FixtureContractOnly,
            failed_slots,
            evaluation_digest: P8ThresholdEvaluationRef::derive(&()),
        };
        value.evaluation_digest = value.derived_digest();
        Ok(value)
    }

    fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.threshold.validate_contract();
        if self.schema != P8_THRESHOLD_EVALUATION_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if (!self.failed_slots.is_empty() && !is_strict_sorted_unique(&self.failed_slots))
            || self.evaluation_digest != self.derived_digest()
        {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8ThresholdEvaluationRef {
        P8ThresholdEvaluationRef::derive(&(
            &self.schema,
            &self.threshold,
            self.scope,
            &self.failed_slots,
        ))
    }
}

pub(crate) enum P8OperatorExecutionInputV1<'a> {
    NotRun,
    Blocked {
        reason: P8QualityId,
    },
    StructurallyInvalid {
        reason: P8QualityId,
    },
    Executed {
        plan: &'a P8QualityExperimentPlanV1,
        trial_set: &'a P8QualityTrialSetV1,
        resource_closure: &'a P8QualityResourceClosureV1,
        hard_gate_evaluation: &'a P8HardGateEvaluationV1,
        threshold_evaluation: Option<&'a P8CandidateThresholdEvaluationV1>,
        trusted_execution_receipt: Option<&'a P8TrustedExecutionReceiptRef>,
    },
}

pub(crate) fn derive_operator_state(
    input: P8OperatorExecutionInputV1<'_>,
) -> P8QualityOperatorStateV1 {
    let P8OperatorExecutionInputV1::Executed {
        plan,
        trial_set,
        resource_closure,
        hard_gate_evaluation,
        threshold_evaluation,
        trusted_execution_receipt,
    } = input
    else {
        return match input {
            P8OperatorExecutionInputV1::NotRun => P8QualityOperatorStateV1::NotRun,
            P8OperatorExecutionInputV1::Blocked { reason } => {
                P8QualityOperatorStateV1::Blocked { reason }
            }
            P8OperatorExecutionInputV1::StructurallyInvalid { reason } => {
                P8QualityOperatorStateV1::StructurallyInvalid { reason }
            }
            P8OperatorExecutionInputV1::Executed { .. } => unreachable!(),
        };
    };
    if !plan.validate_contract().is_empty()
        || !trial_set.validate_against(plan).is_empty()
        || !resource_closure.validate_contract().is_empty()
        || resource_closure.trial_set != *trial_set
        || !hard_gate_evaluation.validate_contract().is_empty()
        || hard_gate_evaluation.resource_closure.as_ref() != resource_closure
        || hard_gate_evaluation.hard_policy != plan.hard_policy
        || (plan.execution_mode == P8ExecutionMode::TrustedFull
            && trusted_execution_receipt.is_none())
    {
        return structurally_invalid_state("quality_prerequisite_invalid");
    }
    match plan.purpose {
        P8QualityPurpose::BaselineEstablishment => {
            if threshold_evaluation.is_some() {
                return structurally_invalid_state("baseline_threshold_evaluation_forbidden");
            }
            if hard_gate_evaluation.hard_gate_failures.is_empty() {
                P8QualityOperatorStateV1::ExecutedValidBaseline {
                    threshold_state: P8ThresholdStateV1::UnfrozenExpected,
                    release_eligible: false,
                }
            } else {
                P8QualityOperatorStateV1::ExecutedBaselineRejected {
                    hard_gate_failures: hard_gate_evaluation.hard_gate_failures.clone(),
                    release_eligible: false,
                }
            }
        }
        P8QualityPurpose::QualityCandidate => {
            let (Some(threshold), Some(evaluation)) = (&plan.threshold, threshold_evaluation)
            else {
                return structurally_invalid_state("candidate_threshold_missing");
            };
            if !evaluation.validate_contract().is_empty() || evaluation.threshold != *threshold {
                return structurally_invalid_state("candidate_threshold_evaluation_invalid");
            }
            if plan.execution_mode == P8ExecutionMode::TrustedFull {
                return structurally_invalid_state("trusted_threshold_operator_not_materialized");
            }
            P8QualityOperatorStateV1::ExecutedValidCandidate {
                threshold_digest: threshold.threshold_digest().clone(),
                decision: P8CandidateQualityDecisionV1::QualityFailed,
            }
        }
    }
}

fn structurally_invalid_state(reason: &str) -> P8QualityOperatorStateV1 {
    P8QualityOperatorStateV1::StructurallyInvalid {
        reason: P8QualityId::parse(reason)
            .expect("internal P8 operator state reason must be canonical"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8SameClosureSafeCounterfactualKindV1 {
    TemporalValidity,
    UpdateLineage,
    ObsoleteSuppression,
    ProceduralEvidence,
    DynamicState,
}

impl P8SameClosureSafeCounterfactualKindV1 {
    const ALL: [Self; 5] = [
        Self::TemporalValidity,
        Self::UpdateLineage,
        Self::ObsoleteSuppression,
        Self::ProceduralEvidence,
        Self::DynamicState,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8SafetyNegativeProofKindV1 {
    Invalidated,
    Forgetting,
    EnvironmentPremise,
}

impl P8SafetyNegativeProofKindV1 {
    const ALL: [Self; 3] = [
        Self::Invalidated,
        Self::Forgetting,
        Self::EnvironmentPremise,
    ];
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityTrialKeyV1 {
    question_id: P8QualityId,
    arm: P8QualityArmKind,
    reader_repeat_index: u32,
    judge_repeat_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ReaderTrialKeyV1 {
    question_id: P8QualityId,
    arm: P8QualityArmKind,
    reader_repeat_index: u32,
}

impl P8QualityTrialKeyV1 {
    fn reader_key(&self) -> P8ReaderTrialKeyV1 {
        P8ReaderTrialKeyV1 {
            question_id: self.question_id.clone(),
            arm: self.arm,
            reader_repeat_index: self.reader_repeat_index,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityAblationKeyV1 {
    question_id: P8QualityId,
    arm: P8QualityArmKind,
    counterfactual: P8SameClosureSafeCounterfactualKindV1,
    reader_repeat_index: u32,
    judge_repeat_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8SafetyNegativeProofKeyV1 {
    question_id: P8QualityId,
    arm: P8QualityArmKind,
    proof: P8SafetyNegativeProofKindV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityTrialClosureV1 {
    schema: String,
    purpose: P8QualityPurpose,
    ordered_questions: Vec<P8QualityId>,
    reader_repeats: u32,
    judge_repeats: u32,
    applicable_arms: Vec<P8QualityArmKind>,
    safe_counterfactuals: Vec<P8SameClosureSafeCounterfactualKindV1>,
    negative_proofs: Vec<P8SafetyNegativeProofKindV1>,
    main_trial_count: u64,
    safe_ablation_count: u64,
    negative_proof_count: u64,
    closure_digest: P8TrialClosureRef,
}

impl P8QualityTrialClosureV1 {
    pub(crate) fn derive(
        purpose: P8QualityPurpose,
        ordered_questions: Vec<P8QualityId>,
        reader_repeats: u32,
        judge_repeats: u32,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let question_count = u64::try_from(ordered_questions.len())
            .map_err(|_| vec![P8QualityContractFailure::ArithmeticOverflow])?;
        let repeat_count = u64::from(reader_repeats)
            .checked_mul(u64::from(judge_repeats))
            .ok_or_else(|| vec![P8QualityContractFailure::ArithmeticOverflow])?;
        let arm_count = u64::try_from(P8QualityArmKind::expected_for(purpose).len())
            .map_err(|_| vec![P8QualityContractFailure::ArithmeticOverflow])?;
        let beetle_arm_count = u64::try_from(P8QualityArmKind::beetle_arms_for(purpose).len())
            .map_err(|_| vec![P8QualityContractFailure::ArithmeticOverflow])?;
        let main_trial_count = question_count
            .checked_mul(arm_count)
            .and_then(|count| count.checked_mul(repeat_count))
            .ok_or_else(|| vec![P8QualityContractFailure::ArithmeticOverflow])?;
        let safe_ablation_count = question_count
            .checked_mul(beetle_arm_count)
            .and_then(|count| {
                count.checked_mul(
                    u64::try_from(P8SameClosureSafeCounterfactualKindV1::ALL.len()).ok()?,
                )
            })
            .and_then(|count| count.checked_mul(repeat_count))
            .ok_or_else(|| vec![P8QualityContractFailure::ArithmeticOverflow])?;
        let negative_proof_count = question_count
            .checked_mul(beetle_arm_count)
            .and_then(|count| {
                count.checked_mul(u64::try_from(P8SafetyNegativeProofKindV1::ALL.len()).ok()?)
            })
            .ok_or_else(|| vec![P8QualityContractFailure::ArithmeticOverflow])?;
        let mut value = Self {
            schema: P8_TRIAL_CLOSURE_SCHEMA.into(),
            purpose,
            ordered_questions,
            reader_repeats,
            judge_repeats,
            applicable_arms: P8QualityArmKind::expected_for(purpose).to_vec(),
            safe_counterfactuals: P8SameClosureSafeCounterfactualKindV1::ALL.to_vec(),
            negative_proofs: P8SafetyNegativeProofKindV1::ALL.to_vec(),
            main_trial_count,
            safe_ablation_count,
            negative_proof_count,
            closure_digest: P8TrialClosureRef::derive(&()),
        };
        value.closure_digest = value.derived_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub(crate) const fn main_trial_count(&self) -> u64 {
        self.main_trial_count
    }

    pub(crate) const fn safe_ablation_count(&self) -> u64 {
        self.safe_ablation_count
    }

    pub(crate) const fn negative_proof_count(&self) -> u64 {
        self.negative_proof_count
    }

    pub(crate) const fn purpose(&self) -> P8QualityPurpose {
        self.purpose
    }

    pub(crate) const fn reader_repeats(&self) -> u32 {
        self.reader_repeats
    }

    pub(crate) const fn judge_repeats(&self) -> u32 {
        self.judge_repeats
    }

    pub(crate) fn ordered_question_ids_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_quality_ordered_question_set_v1",
            &self.ordered_questions,
        )
    }

    pub(crate) fn closure_digest(&self) -> &P8TrialClosureRef {
        &self.closure_digest
    }

    pub(crate) fn expected_main_trial_keys(&self) -> Vec<P8QualityTrialKeyV1> {
        let mut keys = Vec::new();
        for question_id in &self.ordered_questions {
            for arm in P8QualityArmKind::expected_for(self.purpose) {
                for reader_repeat_index in 0..self.reader_repeats {
                    for judge_repeat_index in 0..self.judge_repeats {
                        keys.push(P8QualityTrialKeyV1 {
                            question_id: question_id.clone(),
                            arm: *arm,
                            reader_repeat_index,
                            judge_repeat_index,
                        });
                    }
                }
            }
        }
        keys
    }

    pub(crate) fn expected_ablation_keys(&self) -> Vec<P8QualityAblationKeyV1> {
        let mut keys = Vec::new();
        for question_id in &self.ordered_questions {
            for arm in P8QualityArmKind::beetle_arms_for(self.purpose) {
                for counterfactual in P8SameClosureSafeCounterfactualKindV1::ALL {
                    for reader_repeat_index in 0..self.reader_repeats {
                        for judge_repeat_index in 0..self.judge_repeats {
                            keys.push(P8QualityAblationKeyV1 {
                                question_id: question_id.clone(),
                                arm: *arm,
                                counterfactual,
                                reader_repeat_index,
                                judge_repeat_index,
                            });
                        }
                    }
                }
            }
        }
        keys
    }

    pub(crate) fn expected_negative_proof_keys(&self) -> Vec<P8SafetyNegativeProofKeyV1> {
        let mut keys = Vec::new();
        for question_id in &self.ordered_questions {
            for arm in P8QualityArmKind::beetle_arms_for(self.purpose) {
                for proof in P8SafetyNegativeProofKindV1::ALL {
                    keys.push(P8SafetyNegativeProofKeyV1 {
                        question_id: question_id.clone(),
                        arm: *arm,
                        proof,
                    });
                }
            }
        }
        keys
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_TRIAL_CLOSURE_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.ordered_questions.is_empty()
            || self.ordered_questions.iter().collect::<BTreeSet<_>>().len()
                != self.ordered_questions.len()
            || self.reader_repeats == 0
            || self.judge_repeats == 0
            || self.judge_repeats.is_multiple_of(2)
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        if self.applicable_arms != P8QualityArmKind::expected_for(self.purpose)
            || self.safe_counterfactuals != P8SameClosureSafeCounterfactualKindV1::ALL
            || self.negative_proofs != P8SafetyNegativeProofKindV1::ALL
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        let expected = Self::derive_counts(
            self.purpose,
            self.ordered_questions.len(),
            self.reader_repeats,
            self.judge_repeats,
        );
        if expected
            != Some((
                self.main_trial_count,
                self.safe_ablation_count,
                self.negative_proof_count,
            ))
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        if self.closure_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derive_counts(
        purpose: P8QualityPurpose,
        question_count: usize,
        reader_repeats: u32,
        judge_repeats: u32,
    ) -> Option<(u64, u64, u64)> {
        let questions = u64::try_from(question_count).ok()?;
        let repeats = u64::from(reader_repeats).checked_mul(u64::from(judge_repeats))?;
        let arms = u64::try_from(P8QualityArmKind::expected_for(purpose).len()).ok()?;
        let beetle_arms = u64::try_from(P8QualityArmKind::beetle_arms_for(purpose).len()).ok()?;
        Some((
            questions.checked_mul(arms)?.checked_mul(repeats)?,
            questions
                .checked_mul(beetle_arms)?
                .checked_mul(u64::try_from(P8SameClosureSafeCounterfactualKindV1::ALL.len()).ok()?)?
                .checked_mul(repeats)?,
            questions
                .checked_mul(beetle_arms)?
                .checked_mul(u64::try_from(P8SafetyNegativeProofKindV1::ALL.len()).ok()?)?,
        ))
    }

    fn derived_digest(&self) -> P8TrialClosureRef {
        P8TrialClosureRef::derive(&(
            &self.schema,
            self.purpose,
            &self.ordered_questions,
            self.reader_repeats,
            self.judge_repeats,
            &self.applicable_arms,
            &self.safe_counterfactuals,
            &self.negative_proofs,
            self.main_trial_count,
            self.safe_ablation_count,
            self.negative_proof_count,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8DatasetProtocolV1 {
    dataset_identity_digest: P8QualityDigest,
    dataset_version_digest: P8QualityDigest,
    dataset_license_digest: P8QualityDigest,
    input_manifest_digest: P8QualityDigest,
    ordered_question_rubric_gold_manifest_digest: P8QualityDigest,
    ordered_question_ids_digest: P8QualityDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8CandidateSlotDomainV1 {
    DistinctBeetleSemanticSourceAndRelease,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ArmUniverseProtocolV1 {
    arm_universe: Vec<P8QualityArmKind>,
    baseline_applicable_arms: Vec<P8QualityArmKind>,
    candidate_applicable_arms: Vec<P8QualityArmKind>,
    no_memory_release: P8ArmReleaseRef,
    public_reference_release: P8ArmReleaseRef,
    frozen_p84_release: P8ArmReleaseRef,
    candidate_slot_domain: P8CandidateSlotDomainV1,
    common_harness_semantic_source_digest: P8CommonHarnessSemanticSourceRef,
    toolchain_contract_digest: P8QualityDigest,
    build_contract_digest: P8QualityDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8BenchmarkBackendKindV1 {
    InMemory,
    Embedded,
    File,
    Sqlite,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8RuntimeIdentityProtocolV1 {
    profile: ProfileId,
    backend: P8BenchmarkBackendKindV1,
    capability_snapshot_digest: P8QualityDigest,
    runtime_budget_report_id: P8RuntimeBudgetReportIdV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ModelLockV1 {
    provider_identity_digest: P8QualityDigest,
    model_revision_digest: P8QualityDigest,
    prompt_contract_digest: P8QualityDigest,
    configuration_digest: P8QualityDigest,
    tool_schema_digest: P8QualityDigest,
    generation_policy_digest: P8QualityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ReaderJudgeProtocolV1 {
    reader: P8ModelLockV1,
    judge: P8ModelLockV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8TrialOrderPolicyV1 {
    ProtocolOrderedQuestionArmReaderJudge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8RepeatAggregationPolicyV1 {
    JudgeMajorityThenReaderExactRational,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8MissingnessPolicyV1 {
    RequiredPairInvalidatesHypothesis,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8TrialProtocolV1 {
    reader_repeats: u32,
    judge_repeats: u32,
    trial_order: P8TrialOrderPolicyV1,
    repeat_aggregation: P8RepeatAggregationPolicyV1,
    missingness: P8MissingnessPolicyV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ConfidenceIntervalAlgorithmV1 {
    QuestionClusterPairedBootstrap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ConfidenceTailV1 {
    OneSidedLowerForQuality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8RoundingPolicyV1 {
    ExactRationalThenOutwardIntegerBound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8MultipleComparisonCorrectionV1 {
    HolmFamilyWise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8BootstrapSeedDerivationV1 {
    ExperimentPlanDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ThresholdDerivationPolicyV1 {
    FirstVerifiedBaselineDistribution,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8StatisticalProtocolV1 {
    confidence_level_basis_points: u16,
    family_alpha_parts_per_million: u32,
    bootstrap_resamples: u32,
    minimum_effective_questions: u32,
    ci_algorithm: P8ConfidenceIntervalAlgorithmV1,
    tail: P8ConfidenceTailV1,
    rounding: P8RoundingPolicyV1,
    multiple_comparison_correction: P8MultipleComparisonCorrectionV1,
    bootstrap_seed_derivation: P8BootstrapSeedDerivationV1,
    threshold_derivation: P8ThresholdDerivationPolicyV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8RenderedCharsMeasurePolicyV1 {
    SdkMemoryProjectionUnicodeScalarCount,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8LatencyMeasurePolicyV1 {
    ArmInputReadyToFinalReaderResponse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8MemoryMeasurePolicyV1 {
    ExclusiveArmCgroupV2RunRootMemoryPeak,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ResourceProtocolV1 {
    rendered_chars_measure: P8RenderedCharsMeasurePolicyV1,
    rendered_chars_quantile_basis_points: u16,
    latency_measure: P8LatencyMeasurePolicyV1,
    latency_quantile_basis_points: u16,
    memory_measure: P8MemoryMeasurePolicyV1,
    shard_count: u32,
    maximum_concurrent_shards_per_arm: u32,
    arms_execute_serially: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ProtocolFreezeInputsV1 {
    dataset: P8DatasetProtocolV1,
    arm_universe: P8ArmUniverseProtocolV1,
    runtime_identity: P8RuntimeIdentityProtocolV1,
    models: P8ReaderJudgeProtocolV1,
    trial: P8TrialProtocolV1,
    statistics: P8StatisticalProtocolV1,
    resources: P8ResourceProtocolV1,
    hard_policy_digest: P8HardPolicyRef,
    hypothesis_registry: P8QualityHypothesisRegistryV1,
    execution_mode: P8ExecutionMode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8EvaluationProtocolLockV1 {
    schema: String,
    frozen: P8ProtocolFreezeInputsV1,
    protocol_digest: P8ProtocolLockRef,
}

impl P8EvaluationProtocolLockV1 {
    pub(crate) fn build(
        frozen: P8ProtocolFreezeInputsV1,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let mut value = Self {
            schema: P8_PROTOCOL_LOCK_SCHEMA.into(),
            frozen,
            protocol_digest: P8ProtocolLockRef::derive(&()),
        };
        value.protocol_digest = value.derived_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub(crate) fn protocol_digest(&self) -> &P8ProtocolLockRef {
        &self.protocol_digest
    }

    pub(crate) fn execution_mode(&self) -> P8ExecutionMode {
        self.frozen.execution_mode
    }

    pub(crate) fn hard_policy_digest(&self) -> &P8HardPolicyRef {
        &self.frozen.hard_policy_digest
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.frozen.hypothesis_registry.validate_contract();
        if self.schema != P8_PROTOCOL_LOCK_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        let arm = &self.frozen.arm_universe;
        if arm.arm_universe != P8QualityArmKind::CANDIDATE
            || arm.baseline_applicable_arms != P8QualityArmKind::BASELINE
            || arm.candidate_applicable_arms != P8QualityArmKind::CANDIDATE
            || [
                &arm.no_memory_release,
                &arm.public_reference_release,
                &arm.frozen_p84_release,
            ]
            .into_iter()
            .collect::<BTreeSet<_>>()
            .len()
                != 3
        {
            failures.push(P8QualityContractFailure::ArmSetMismatch);
        }
        let trial = &self.frozen.trial;
        if trial.reader_repeats == 0
            || trial.judge_repeats == 0
            || trial.judge_repeats.is_multiple_of(2)
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        let statistics = &self.frozen.statistics;
        if statistics.confidence_level_basis_points == 0
            || statistics.confidence_level_basis_points > 10_000
            || statistics.family_alpha_parts_per_million == 0
            || statistics.family_alpha_parts_per_million >= 1_000_000
            || statistics.bootstrap_resamples == 0
            || statistics.minimum_effective_questions == 0
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        let resources = &self.frozen.resources;
        if resources.rendered_chars_quantile_basis_points == 0
            || resources.rendered_chars_quantile_basis_points > 10_000
            || resources.latency_quantile_basis_points == 0
            || resources.latency_quantile_basis_points > 10_000
            || resources.shard_count == 0
            || resources.maximum_concurrent_shards_per_arm == 0
            || !resources.arms_execute_serially
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        if self.frozen.dataset.ordered_question_ids_digest
            != *self
                .frozen
                .hypothesis_registry
                .ordered_question_ids_digest()
        {
            failures.push(P8QualityContractFailure::HypothesisMismatch);
        }
        if self.protocol_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8ProtocolLockRef {
        P8ProtocolLockRef::derive(&(&self.schema, &self.frozen))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ThresholdDerivationOutputsV1 {
    target_thresholds_digest: P8QualityDigest,
    untouched_thresholds_digest: P8QualityDigest,
    rendered_chars_frontier_digest: P8QualityDigest,
    latency_frontier_digest: P8QualityDigest,
    peak_domain_memory_frontier_digest: P8QualityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityThresholdLockV1 {
    schema: String,
    protocol_digest: P8ProtocolLockRef,
    hard_policy_digest: P8HardPolicyRef,
    baseline_manifest_digest: P8BaselineManifestRef,
    derivation_outputs: P8ThresholdDerivationOutputsV1,
    threshold_digest: P8ThresholdLockRef,
}

impl P8QualityThresholdLockV1 {
    pub(crate) fn build(
        protocol_digest: P8ProtocolLockRef,
        hard_policy_digest: P8HardPolicyRef,
        baseline_manifest_digest: P8BaselineManifestRef,
        derivation_outputs: P8ThresholdDerivationOutputsV1,
    ) -> Self {
        let mut value = Self {
            schema: P8_THRESHOLD_LOCK_SCHEMA.into(),
            protocol_digest,
            hard_policy_digest,
            baseline_manifest_digest,
            derivation_outputs,
            threshold_digest: P8ThresholdLockRef::derive(&()),
        };
        value.threshold_digest = value.derived_digest();
        value
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_THRESHOLD_LOCK_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.threshold_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn protocol_digest(&self) -> &P8ProtocolLockRef {
        &self.protocol_digest
    }

    pub(crate) fn threshold_digest(&self) -> &P8ThresholdLockRef {
        &self.threshold_digest
    }

    pub(crate) fn hard_policy_digest(&self) -> &P8HardPolicyRef {
        &self.hard_policy_digest
    }

    fn derived_digest(&self) -> P8ThresholdLockRef {
        P8ThresholdLockRef::derive(&(
            &self.schema,
            &self.protocol_digest,
            &self.hard_policy_digest,
            &self.baseline_manifest_digest,
            &self.derivation_outputs,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityExperimentPlanV1 {
    schema: String,
    purpose: P8QualityPurpose,
    execution_mode: P8ExecutionMode,
    source_release_set: P8SourceReleaseSetV1,
    hard_policy: P8QualityHardPolicyV1,
    protocol: P8EvaluationProtocolLockV1,
    trial_closure: P8QualityTrialClosureV1,
    threshold: Option<P8QualityThresholdLockV1>,
    frozen_quality_policy: Option<P8FrozenQualityPolicyV1>,
    trusted_execution_lease: Option<P8TrustedExecutionLeaseRef>,
    run_id: P8QualityRunRef,
    plan_digest: P8ExperimentPlanRef,
}

impl P8QualityExperimentPlanV1 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build(
        purpose: P8QualityPurpose,
        execution_mode: P8ExecutionMode,
        source_release_set: P8SourceReleaseSetV1,
        hard_policy: P8QualityHardPolicyV1,
        protocol: P8EvaluationProtocolLockV1,
        trial_closure: P8QualityTrialClosureV1,
        threshold: Option<P8QualityThresholdLockV1>,
        frozen_quality_policy: Option<P8FrozenQualityPolicyV1>,
        trusted_execution_lease: Option<P8TrustedExecutionLeaseRef>,
    ) -> Result<Self, Vec<P8QualityContractFailure>> {
        let mut value = Self {
            schema: P8_EXPERIMENT_PLAN_SCHEMA.into(),
            purpose,
            execution_mode,
            source_release_set,
            hard_policy,
            protocol,
            trial_closure,
            threshold,
            frozen_quality_policy,
            trusted_execution_lease,
            run_id: P8QualityRunRef::derive(&()),
            plan_digest: P8ExperimentPlanRef::derive(&()),
        };
        value.run_id = value.derived_run_id();
        value.plan_digest = value.derived_plan_digest();
        let failures = value.validate_contract();
        if failures.is_empty() {
            Ok(value)
        } else {
            Err(failures)
        }
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.source_release_set.validate_contract();
        failures.extend(self.hard_policy.validate_contract());
        failures.extend(self.protocol.validate_contract());
        failures.extend(self.trial_closure.validate_contract());
        if self.schema != P8_EXPERIMENT_PLAN_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.purpose != self.source_release_set.purpose()
            || self.purpose != self.trial_closure.purpose()
            || self.execution_mode != self.protocol.execution_mode()
        {
            failures.push(P8QualityContractFailure::PurposeMismatch);
        }
        match (
            self.execution_mode,
            self.source_release_set.evidence_class(),
        ) {
            (
                P8ExecutionMode::FixtureContract,
                P8SourceReleaseEvidenceClassV1::FixtureContractOnlyNoSourceOrExclusionProof,
            ) => {}
            (P8ExecutionMode::TrustedFull, P8SourceReleaseEvidenceClassV1::TrustedSealed) => {}
            _ => failures.push(P8QualityContractFailure::TrustedExecutionMissing),
        }
        if self.hard_policy.policy_digest() != self.protocol.hard_policy_digest() {
            failures.push(P8QualityContractFailure::HardPolicyMismatch);
        }
        let arm_protocol = &self.protocol.frozen.arm_universe;
        if self
            .source_release_set
            .arm_release_digest(P8QualityArmKind::NoMemory)
            != Some(&arm_protocol.no_memory_release)
            || self
                .source_release_set
                .arm_release_digest(P8QualityArmKind::PublicReference)
                != Some(&arm_protocol.public_reference_release)
            || self
                .source_release_set
                .arm_release_digest(P8QualityArmKind::FrozenP84Baseline)
                != Some(&arm_protocol.frozen_p84_release)
            || self
                .source_release_set
                .common_harness_semantic_source_digest()
                != &arm_protocol.common_harness_semantic_source_digest
            || self.source_release_set.harness_toolchain_digest()
                != &arm_protocol.toolchain_contract_digest
            || self.source_release_set.harness_build_contract_digest()
                != &arm_protocol.build_contract_digest
        {
            failures.push(P8QualityContractFailure::ArmIdentityAlias);
        }
        if self.trial_closure.reader_repeats() != self.protocol.frozen.trial.reader_repeats
            || self.trial_closure.judge_repeats() != self.protocol.frozen.trial.judge_repeats
            || self.trial_closure.ordered_question_ids_digest()
                != self.protocol.frozen.dataset.ordered_question_ids_digest
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        match self.purpose {
            P8QualityPurpose::BaselineEstablishment => {
                if self.threshold.is_some() || self.frozen_quality_policy.is_some() {
                    failures.push(P8QualityContractFailure::PurposeMismatch);
                }
            }
            P8QualityPurpose::QualityCandidate => {
                let Some(threshold) = &self.threshold else {
                    failures.push(P8QualityContractFailure::ThresholdMismatch);
                    return failures;
                };
                failures.extend(threshold.validate_contract());
                if threshold.protocol_digest() != self.protocol.protocol_digest()
                    || threshold.hard_policy_digest() != self.hard_policy.policy_digest()
                {
                    failures.push(P8QualityContractFailure::ThresholdMismatch);
                }
                match &self.frozen_quality_policy {
                    Some(policy) => failures.extend(policy.validate_against(
                        self.protocol.protocol_digest(),
                        threshold.threshold_digest(),
                    )),
                    None => failures.push(P8QualityContractFailure::ThresholdMismatch),
                }
            }
        }
        if self.execution_mode == P8ExecutionMode::TrustedFull
            && self.trusted_execution_lease.is_none()
        {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        if self.run_id != self.derived_run_id() || self.plan_digest != self.derived_plan_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn run_id(&self) -> &P8QualityRunRef {
        &self.run_id
    }

    pub(crate) fn arm_release_digest(&self, arm: P8QualityArmKind) -> Option<&P8ArmReleaseRef> {
        self.source_release_set.arm_release_digest(arm)
    }

    fn expected_outcomes_for(
        &self,
        question_id: &P8QualityId,
    ) -> Option<&P8ExpectedCapabilityOutcomesV1> {
        self.protocol
            .frozen
            .hypothesis_registry
            .expected_outcomes_for(question_id)
    }

    fn derived_run_id(&self) -> P8QualityRunRef {
        P8QualityRunRef::derive(&(
            &self.schema,
            self.purpose,
            self.execution_mode,
            self.source_release_set.release_set_digest(),
            self.protocol.protocol_digest(),
            self.trial_closure.closure_digest(),
            self.threshold
                .as_ref()
                .map(P8QualityThresholdLockV1::threshold_digest),
            &self.frozen_quality_policy,
            &self.trusted_execution_lease,
        ))
    }

    fn derived_plan_digest(&self) -> P8ExperimentPlanRef {
        P8ExperimentPlanRef::derive(&(
            &self.schema,
            self.purpose,
            self.execution_mode,
            &self.source_release_set,
            &self.hard_policy,
            &self.protocol,
            &self.trial_closure,
            &self.threshold,
            &self.frozen_quality_policy,
            &self.trusted_execution_lease,
            &self.run_id,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ModelEvaluatedTrialDetailV1 {
    key: P8QualityTrialKeyV1,
    run_id: P8QualityRunRef,
    arm_release_digest: P8ArmReleaseRef,
    arm_receipt: P8QualityArmReceiptV1,
    execution_chain: P8ModelEvaluatedExecutionChainV1,
    accuracy: P8AccuracyOutcomeV1,
    capability_outcomes: P8ActualCapabilityOutcomesV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ModelEvaluatedExecutionChainV1 {
    composition_receipt: P8CompositionReceiptRef,
    model_execution_receipt: P8ModelExecutionReceiptRef,
    benchmark_join_receipt: P8BenchmarkJoinExecutionReceiptRef,
    judge_execution_receipt: P8JudgeExecutionReceiptRef,
    attempt_latency_receipt: P8AttemptLatencyReceiptRef,
    question_latency_receipt: P8QuestionLatencyReceiptRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum P8QualityArmReceiptV1 {
    NoMemory {
        no_memory_projection_proof: P8NoMemoryProjectionProofRef,
    },
    PublicReference {
        safe_output_receipt: P8PublicSafeOutputReceiptRef,
    },
    BeetleSemantic {
        semantic_execution_receipt_v2: P8SemanticExecutionReceiptV2Ref,
    },
}

impl P8QualityArmReceiptV1 {
    fn matches_arm(&self, arm: P8QualityArmKind) -> bool {
        matches!(
            (self, arm),
            (Self::NoMemory { .. }, P8QualityArmKind::NoMemory)
                | (
                    Self::PublicReference { .. },
                    P8QualityArmKind::PublicReference
                )
                | (
                    Self::BeetleSemantic { .. },
                    P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate
                )
        )
    }

    fn fixture(
        run_id: &P8QualityRunRef,
        reader_key: &P8ReaderTrialKeyV1,
        arm_release_digest: &P8ArmReleaseRef,
    ) -> Self {
        match reader_key.arm {
            P8QualityArmKind::NoMemory => Self::NoMemory {
                no_memory_projection_proof: P8NoMemoryProjectionProofRef::derive(&(
                    "fixture-main",
                    run_id,
                    reader_key,
                    arm_release_digest,
                )),
            },
            P8QualityArmKind::PublicReference => Self::PublicReference {
                safe_output_receipt: P8PublicSafeOutputReceiptRef::derive(&(
                    "fixture-main",
                    run_id,
                    reader_key,
                    arm_release_digest,
                )),
            },
            P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate => {
                Self::BeetleSemantic {
                    semantic_execution_receipt_v2: P8SemanticExecutionReceiptV2Ref::derive(&(
                        "fixture-main",
                        run_id,
                        reader_key,
                        arm_release_digest,
                    )),
                }
            }
        }
    }
}

impl P8ModelEvaluatedExecutionChainV1 {
    fn fixture(
        run_id: &P8QualityRunRef,
        key: &P8QualityTrialKeyV1,
        arm_release_digest: &P8ArmReleaseRef,
        arm_receipt: &P8QualityArmReceiptV1,
    ) -> Self {
        let reader_key = key.reader_key();
        Self {
            composition_receipt: P8CompositionReceiptRef::derive(&(
                "fixture-composition",
                run_id,
                &reader_key,
                arm_release_digest,
                arm_receipt,
            )),
            model_execution_receipt: P8ModelExecutionReceiptRef::derive(&(
                "fixture-model",
                run_id,
                &reader_key,
                arm_release_digest,
                arm_receipt,
            )),
            benchmark_join_receipt: P8BenchmarkJoinExecutionReceiptRef::derive(&(
                "fixture-join",
                run_id,
                &reader_key,
                arm_release_digest,
            )),
            judge_execution_receipt: P8JudgeExecutionReceiptRef::derive(&(
                "fixture-judge",
                run_id,
                key,
                arm_release_digest,
            )),
            attempt_latency_receipt: P8AttemptLatencyReceiptRef::derive(&(
                "fixture-attempt-latency",
                run_id,
                &reader_key,
                arm_release_digest,
            )),
            question_latency_receipt: P8QuestionLatencyReceiptRef::derive(&(
                "fixture-question-latency",
                run_id,
                &reader_key,
                arm_release_digest,
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityAblationTrialV1 {
    key: P8QualityAblationKeyV1,
    run_id: P8QualityRunRef,
    arm_release_digest: P8ArmReleaseRef,
    receipt_pair: P8AblationReceiptPairV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8AblationReceiptPairV1 {
    baseline_semantic_execution_receipt_v2: P8SemanticExecutionReceiptV2Ref,
    off_run_semantic_execution_receipt_v2: P8SemanticExecutionReceiptV2Ref,
    baseline_projection_digest: P8ProviderSafeProjectionRef,
    off_run_projection_digest: P8ProviderSafeProjectionRef,
    baseline_request_digest: P8ModelRequestRef,
    off_run_request_digest: P8ModelRequestRef,
    paired_judge_receipt: P8PairedJudgeReceiptRef,
}

impl P8AblationReceiptPairV1 {
    fn is_distinct_pair(&self) -> bool {
        self.baseline_semantic_execution_receipt_v2 != self.off_run_semantic_execution_receipt_v2
            && self.baseline_projection_digest != self.off_run_projection_digest
            && self.baseline_request_digest != self.off_run_request_digest
    }

    fn fixture(
        run_id: &P8QualityRunRef,
        key: &P8QualityAblationKeyV1,
        arm_release_digest: &P8ArmReleaseRef,
    ) -> Self {
        Self {
            baseline_semantic_execution_receipt_v2: P8SemanticExecutionReceiptV2Ref::derive(&(
                "fixture-ablation-baseline",
                run_id,
                key,
                arm_release_digest,
            )),
            off_run_semantic_execution_receipt_v2: P8SemanticExecutionReceiptV2Ref::derive(&(
                "fixture-ablation-off-run",
                run_id,
                key,
                arm_release_digest,
            )),
            baseline_projection_digest: P8ProviderSafeProjectionRef::derive(&(
                "fixture-ablation-baseline-projection",
                run_id,
                key,
                arm_release_digest,
            )),
            off_run_projection_digest: P8ProviderSafeProjectionRef::derive(&(
                "fixture-ablation-off-run-projection",
                run_id,
                key,
                arm_release_digest,
            )),
            baseline_request_digest: P8ModelRequestRef::derive(&(
                "fixture-ablation-baseline-request",
                run_id,
                key,
                arm_release_digest,
            )),
            off_run_request_digest: P8ModelRequestRef::derive(&(
                "fixture-ablation-off-run-request",
                run_id,
                key,
                arm_release_digest,
            )),
            paired_judge_receipt: P8PairedJudgeReceiptRef::derive(&(
                "fixture-ablation-paired-judge",
                run_id,
                key,
                arm_release_digest,
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8NegativeProofModelBoundaryV1 {
    NoReaderModelJudgeOrAccuracy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8SafetyNegativeProofTrialV1 {
    key: P8SafetyNegativeProofKeyV1,
    run_id: P8QualityRunRef,
    arm_release_digest: P8ArmReleaseRef,
    semantic_execution_receipt_v2: P8SemanticExecutionReceiptV2Ref,
    safe_binding_digest: P8ProviderSafeProjectionRef,
    proof_receipt: P8SafetyProofReceiptRef,
    proof_outcome: P8SafetyNegativeProofOutcomeV1,
    coverage: P8SafetyProofCoverageV1,
    model_boundary: P8NegativeProofModelBoundaryV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8SafetyNegativeProofOutcomeV1 {
    ProvedAbsentOrBlocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8SafetyProofCoverageV1 {
    exact_owner_path_count: u64,
    verified_owner_path_count: u64,
}

impl P8SafetyNegativeProofTrialV1 {
    fn fixture(plan: &P8QualityExperimentPlanV1, key: P8SafetyNegativeProofKeyV1) -> Self {
        let arm_release_digest = plan
            .arm_release_digest(key.arm)
            .expect("planned arm")
            .clone();
        Self::fixture_for_release(plan, key, arm_release_digest)
    }

    fn fixture_for_release(
        plan: &P8QualityExperimentPlanV1,
        key: P8SafetyNegativeProofKeyV1,
        arm_release_digest: P8ArmReleaseRef,
    ) -> Self {
        Self {
            semantic_execution_receipt_v2: P8SemanticExecutionReceiptV2Ref::derive(&(
                "fixture-negative",
                plan.run_id(),
                &key,
                &arm_release_digest,
            )),
            safe_binding_digest: P8ProviderSafeProjectionRef::derive(&(
                "fixture-negative-safe-binding",
                plan.run_id(),
                &key,
                &arm_release_digest,
            )),
            proof_receipt: P8SafetyProofReceiptRef::derive(&(
                "fixture-negative-proof",
                plan.run_id(),
                &key,
                &arm_release_digest,
            )),
            proof_outcome: P8SafetyNegativeProofOutcomeV1::ProvedAbsentOrBlocked,
            coverage: P8SafetyProofCoverageV1 {
                exact_owner_path_count: 1,
                verified_owner_path_count: 1,
            },
            run_id: plan.run_id().clone(),
            arm_release_digest,
            key,
            model_boundary: P8NegativeProofModelBoundaryV1::NoReaderModelJudgeOrAccuracy,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityTrialSetV1 {
    schema: String,
    experiment_plan: P8QualityExperimentPlanV1,
    scope: P8TrialSetScopeV1,
    main_trials: Vec<P8ModelEvaluatedTrialDetailV1>,
    ablation_trials: Vec<P8QualityAblationTrialV1>,
    negative_proof_trials: Vec<P8SafetyNegativeProofTrialV1>,
    trial_set_digest: P8QualityTrialSetRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8TrialSetScopeV1 {
    FixtureContractOnly,
}

impl P8QualityTrialSetV1 {
    #[cfg(test)]
    pub(crate) fn fixture(plan: &P8QualityExperimentPlanV1) -> Self {
        let main_trials = plan
            .trial_closure
            .expected_main_trial_keys()
            .into_iter()
            .map(|key| {
                let arm_release_digest = plan
                    .arm_release_digest(key.arm)
                    .expect("planned arm")
                    .clone();
                let arm_receipt = P8QualityArmReceiptV1::fixture(
                    plan.run_id(),
                    &key.reader_key(),
                    &arm_release_digest,
                );
                let execution_chain = P8ModelEvaluatedExecutionChainV1::fixture(
                    plan.run_id(),
                    &key,
                    &arm_release_digest,
                    &arm_receipt,
                );
                let expected = plan
                    .expected_outcomes_for(&key.question_id)
                    .expect("fixture question has frozen expected outcomes")
                    .clone();
                let accuracy = match &expected.accuracy {
                    P8ExpectedAccuracyV1::Correct => P8AccuracyOutcomeV1::Correct,
                    P8ExpectedAccuracyV1::ExpectedRefusal { reason } => {
                        P8AccuracyOutcomeV1::ExpectedRefusal {
                            reason: reason.clone(),
                        }
                    }
                    P8ExpectedAccuracyV1::NotApplicable => P8AccuracyOutcomeV1::NotApplicable,
                };
                P8ModelEvaluatedTrialDetailV1 {
                    arm_release_digest,
                    arm_receipt,
                    execution_chain,
                    accuracy,
                    capability_outcomes: expected.into_actual(),
                    run_id: plan.run_id().clone(),
                    key,
                }
            })
            .collect();
        let ablation_trials = plan
            .trial_closure
            .expected_ablation_keys()
            .into_iter()
            .map(|key| {
                let arm_release_digest = plan
                    .arm_release_digest(key.arm)
                    .expect("planned arm")
                    .clone();
                let receipt_pair =
                    P8AblationReceiptPairV1::fixture(plan.run_id(), &key, &arm_release_digest);
                P8QualityAblationTrialV1 {
                    arm_release_digest,
                    receipt_pair,
                    run_id: plan.run_id().clone(),
                    key,
                }
            })
            .collect();
        let negative_proof_trials = plan
            .trial_closure
            .expected_negative_proof_keys()
            .into_iter()
            .map(|key| P8SafetyNegativeProofTrialV1::fixture(plan, key))
            .collect();
        let mut value = Self {
            schema: P8_TRIAL_SET_SCHEMA.into(),
            experiment_plan: plan.clone(),
            scope: P8TrialSetScopeV1::FixtureContractOnly,
            main_trials,
            ablation_trials,
            negative_proof_trials,
            trial_set_digest: P8QualityTrialSetRef::derive(&()),
        };
        value.trial_set_digest = value.derived_digest();
        value
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let plan = &self.experiment_plan;
        let mut failures = plan.validate_contract();
        if self.schema != P8_TRIAL_SET_SCHEMA {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if plan.execution_mode != P8ExecutionMode::FixtureContract
            || self.scope != P8TrialSetScopeV1::FixtureContractOnly
        {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        let main_keys = self
            .main_trials
            .iter()
            .map(|trial| trial.key.clone())
            .collect::<Vec<_>>();
        let ablation_keys = self
            .ablation_trials
            .iter()
            .map(|trial| trial.key.clone())
            .collect::<Vec<_>>();
        let negative_keys = self
            .negative_proof_trials
            .iter()
            .map(|trial| trial.key.clone())
            .collect::<Vec<_>>();
        if main_keys != plan.trial_closure.expected_main_trial_keys()
            || ablation_keys != plan.trial_closure.expected_ablation_keys()
            || negative_keys != plan.trial_closure.expected_negative_proof_keys()
        {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        for (arm, release_digest, run_id) in self
            .main_trials
            .iter()
            .map(|trial| (trial.key.arm, &trial.arm_release_digest, &trial.run_id))
            .chain(
                self.ablation_trials
                    .iter()
                    .map(|trial| (trial.key.arm, &trial.arm_release_digest, &trial.run_id)),
            )
            .chain(
                self.negative_proof_trials
                    .iter()
                    .map(|trial| (trial.key.arm, &trial.arm_release_digest, &trial.run_id)),
            )
        {
            if plan.arm_release_digest(arm) != Some(release_digest) || run_id != plan.run_id() {
                failures.push(P8QualityContractFailure::OutcomeMismatch);
            }
        }
        for trial in &self.main_trials {
            let Some(expected_release) = plan.arm_release_digest(trial.key.arm) else {
                failures.push(P8QualityContractFailure::ArmSetMismatch);
                continue;
            };
            let expected_arm_receipt = P8QualityArmReceiptV1::fixture(
                plan.run_id(),
                &trial.key.reader_key(),
                expected_release,
            );
            let expected_chain = P8ModelEvaluatedExecutionChainV1::fixture(
                plan.run_id(),
                &trial.key,
                expected_release,
                &expected_arm_receipt,
            );
            let Some(expected_outcomes) = plan.expected_outcomes_for(&trial.key.question_id) else {
                failures.push(P8QualityContractFailure::HypothesisMismatch);
                continue;
            };
            if !trial.arm_receipt.matches_arm(trial.key.arm)
                || trial.arm_receipt != expected_arm_receipt
                || trial.execution_chain != expected_chain
                || !actual_applicability_matches(
                    expected_outcomes,
                    &trial.accuracy,
                    &trial.capability_outcomes,
                )
            {
                failures.push(P8QualityContractFailure::OutcomeMismatch);
            }
        }
        for trial in &self.ablation_trials {
            let Some(expected_release) = plan.arm_release_digest(trial.key.arm) else {
                failures.push(P8QualityContractFailure::ArmSetMismatch);
                continue;
            };
            if !trial.receipt_pair.is_distinct_pair()
                || trial.receipt_pair
                    != P8AblationReceiptPairV1::fixture(plan.run_id(), &trial.key, expected_release)
            {
                failures.push(P8QualityContractFailure::OutcomeMismatch);
            }
        }
        for trial in &self.negative_proof_trials {
            let Some(expected_release) = plan.arm_release_digest(trial.key.arm) else {
                failures.push(P8QualityContractFailure::ArmSetMismatch);
                continue;
            };
            let expected = P8SafetyNegativeProofTrialV1::fixture_for_release(
                plan,
                trial.key.clone(),
                expected_release.clone(),
            );
            if trial != &expected {
                failures.push(P8QualityContractFailure::OutcomeMismatch);
            }
        }
        if self.trial_set_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn validate_against(
        &self,
        plan: &P8QualityExperimentPlanV1,
    ) -> Vec<P8QualityContractFailure> {
        let mut failures = self.validate_contract();
        if &self.experiment_plan != plan {
            failures.push(P8QualityContractFailure::PurposeMismatch);
        }
        failures
    }

    pub(crate) fn experiment_plan(&self) -> &P8QualityExperimentPlanV1 {
        &self.experiment_plan
    }

    fn derived_digest(&self) -> P8QualityTrialSetRef {
        P8QualityTrialSetRef::derive(&(
            &self.schema,
            &self.experiment_plan,
            self.scope,
            &self.main_trials,
            &self.ablation_trials,
            &self.negative_proof_trials,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ResourceMeasureKindV1 {
    MemoryProjectionRenderedChars,
    QuestionEndToEndLatency,
    PeakDomainMemoryBytes,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum P8ResourceObservationStateV1 {
    FixtureNotTrusted,
    TrustedObserved {
        value: u64,
        receipt_digest: P8TrustedDomainResourceReceiptRef,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8ResourceObservationV1 {
    grain: P8ResourceObservationGrainV1,
    state: P8ResourceObservationStateV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum P8ResourceObservationGrainV1 {
    Trial {
        measure: P8ResourceMeasureKindV1,
        key: P8ReaderTrialKeyV1,
        run_id: P8QualityRunRef,
        arm_release_digest: P8ArmReleaseRef,
    },
    ArmRunRoot {
        measure: P8ResourceMeasureKindV1,
        arm: P8QualityArmKind,
        run_id: P8QualityRunRef,
        arm_release_digest: P8ArmReleaseRef,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8QualityResourceClosureV1 {
    schema: String,
    trial_set: P8QualityTrialSetV1,
    scope: P8ResourceClosureScopeV1,
    observations: Vec<P8ResourceObservationV1>,
    closure_digest: P8QualityDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ResourceClosureScopeV1 {
    FixtureContractOnly,
}

impl P8QualityResourceClosureV1 {
    #[cfg(test)]
    pub(crate) fn fixture(trial_set: &P8QualityTrialSetV1) -> Self {
        let plan = trial_set.experiment_plan();
        let mut observations = trial_set
            .main_trials
            .iter()
            .filter(|trial| trial.key.judge_repeat_index == 0)
            .flat_map(|trial| {
                let measures = if matches!(
                    trial.key.arm,
                    P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate
                ) {
                    vec![
                        P8ResourceMeasureKindV1::MemoryProjectionRenderedChars,
                        P8ResourceMeasureKindV1::QuestionEndToEndLatency,
                    ]
                } else {
                    vec![P8ResourceMeasureKindV1::QuestionEndToEndLatency]
                };
                measures
                    .into_iter()
                    .map(move |measure| P8ResourceObservationV1 {
                        grain: P8ResourceObservationGrainV1::Trial {
                            measure,
                            key: trial.key.reader_key(),
                            run_id: trial.run_id.clone(),
                            arm_release_digest: trial.arm_release_digest.clone(),
                        },
                        state: P8ResourceObservationStateV1::FixtureNotTrusted,
                    })
            })
            .collect::<Vec<_>>();
        observations.extend(
            P8QualityArmKind::expected_for(plan.purpose)
                .iter()
                .map(|arm| P8ResourceObservationV1 {
                    grain: P8ResourceObservationGrainV1::ArmRunRoot {
                        measure: P8ResourceMeasureKindV1::PeakDomainMemoryBytes,
                        arm: *arm,
                        run_id: plan.run_id().clone(),
                        arm_release_digest: plan
                            .arm_release_digest(*arm)
                            .expect("planned arm")
                            .clone(),
                    },
                    state: P8ResourceObservationStateV1::FixtureNotTrusted,
                }),
        );
        let mut value = Self {
            schema: P8_RESOURCE_CLOSURE_SCHEMA.into(),
            trial_set: trial_set.clone(),
            scope: P8ResourceClosureScopeV1::FixtureContractOnly,
            observations,
            closure_digest: P8QualityDigest::derive("p8_quality_resource_closure_v1", &()),
        };
        value.closure_digest = value.derived_digest();
        value
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.trial_set.validate_contract();
        let plan = self.trial_set.experiment_plan();
        let mut expected = self
            .trial_set
            .main_trials
            .iter()
            .filter(|trial| trial.key.judge_repeat_index == 0)
            .flat_map(|trial| {
                let measures = if matches!(
                    trial.key.arm,
                    P8QualityArmKind::FrozenP84Baseline | P8QualityArmKind::P8Candidate
                ) {
                    vec![
                        P8ResourceMeasureKindV1::MemoryProjectionRenderedChars,
                        P8ResourceMeasureKindV1::QuestionEndToEndLatency,
                    ]
                } else {
                    vec![P8ResourceMeasureKindV1::QuestionEndToEndLatency]
                };
                measures
                    .into_iter()
                    .map(move |measure| P8ResourceObservationGrainV1::Trial {
                        measure,
                        key: trial.key.reader_key(),
                        run_id: trial.run_id.clone(),
                        arm_release_digest: trial.arm_release_digest.clone(),
                    })
            })
            .collect::<Vec<_>>();
        expected.extend(
            P8QualityArmKind::expected_for(plan.purpose)
                .iter()
                .filter_map(|arm| {
                    plan.arm_release_digest(*arm).map(|release| {
                        P8ResourceObservationGrainV1::ArmRunRoot {
                            measure: P8ResourceMeasureKindV1::PeakDomainMemoryBytes,
                            arm: *arm,
                            run_id: plan.run_id().clone(),
                            arm_release_digest: release.clone(),
                        }
                    })
                }),
        );
        let actual = self
            .observations
            .iter()
            .map(|observation| observation.grain.clone())
            .collect::<Vec<_>>();
        if self.schema != P8_RESOURCE_CLOSURE_SCHEMA || actual != expected {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        if self.scope != P8ResourceClosureScopeV1::FixtureContractOnly
            || plan.execution_mode != P8ExecutionMode::FixtureContract
        {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        let states_match_mode = self.observations.iter().all(|observation| {
            matches!(
                (plan.execution_mode, &observation.state),
                (
                    P8ExecutionMode::FixtureContract,
                    P8ResourceObservationStateV1::FixtureNotTrusted
                ) | (
                    P8ExecutionMode::TrustedFull,
                    P8ResourceObservationStateV1::TrustedObserved { .. }
                )
            )
        });
        if !states_match_mode {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        if self.closure_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_quality_resource_closure_v1",
            &(
                &self.schema,
                &self.trial_set,
                self.scope,
                &self.observations,
            ),
        )
    }
}

#[derive(Debug)]
struct P8StrictJsonValue(serde_json::Value);

impl<'de> Deserialize<'de> for P8StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(P8StrictJsonVisitor)
    }
}

struct P8StrictJsonVisitor;

impl<'de> Visitor<'de> for P8StrictJsonVisitor {
    type Value = P8StrictJsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(P8StrictJsonValue(serde_json::Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(P8StrictJsonValue(serde_json::Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(P8StrictJsonValue(serde_json::Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .map(P8StrictJsonValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(P8StrictJsonValue(serde_json::Value::String(
            value.to_owned(),
        )))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(P8StrictJsonValue(serde_json::Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(P8StrictJsonValue(serde_json::Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(P8StrictJsonValue(serde_json::Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        P8StrictJsonValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(P8StrictJsonValue(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(P8StrictJsonValue(serde_json::Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(A::Error::custom(format!("duplicate JSON key: {key}")));
            }
            let P8StrictJsonValue(value) = map.next_value()?;
            values.insert(key, value);
        }
        Ok(P8StrictJsonValue(serde_json::Value::Object(values)))
    }
}

pub(crate) fn deserialize_p8_quality_artifact<T>(bytes: &[u8]) -> serde_json::Result<T>
where
    T: DeserializeOwned,
{
    let P8StrictJsonValue(value) = serde_json::from_slice(bytes)?;
    serde_json::from_value(value)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "artifact_kind",
    content = "artifact",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum P8QualityArtifactEnvelopeV1 {
    RawSourceAudit(Box<P8P84RawSourceAuditManifestV1>),
    SemanticSourceAnchor(Box<P8P84SemanticSourceAnchorV1>),
    SourceReleaseSet(Box<P8SourceReleaseSetV1>),
    HardPolicy(Box<P8QualityHardPolicyV1>),
    HypothesisRegistry(Box<P8QualityHypothesisRegistryV1>),
    TrialClosure(Box<P8QualityTrialClosureV1>),
    EvaluationProtocol(Box<P8EvaluationProtocolLockV1>),
    ThresholdLock(Box<P8QualityThresholdLockV1>),
    ExperimentPlan(Box<P8QualityExperimentPlanV1>),
    HardGateEvaluation(Box<P8HardGateEvaluationV1>),
    ThresholdEvaluation(Box<P8CandidateThresholdEvaluationV1>),
    TrialSet(Box<P8QualityTrialSetV1>),
    ResourceClosure(Box<P8QualityResourceClosureV1>),
    TrustedDomainResourceReceipt(Box<P8TrustedDomainResourceReceiptV1>),
}

impl P8QualityArtifactEnvelopeV1 {
    fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        match self {
            Self::RawSourceAudit(value) => value.validate_contract(),
            Self::SemanticSourceAnchor(value) => value.validate_contract(),
            Self::SourceReleaseSet(value) => value.validate_contract(),
            Self::HardPolicy(value) => value.validate_contract(),
            Self::HypothesisRegistry(value) => value.validate_contract(),
            Self::TrialClosure(value) => value.validate_contract(),
            Self::EvaluationProtocol(value) => value.validate_contract(),
            Self::ThresholdLock(value) => value.validate_contract(),
            Self::ExperimentPlan(value) => value.validate_contract(),
            Self::HardGateEvaluation(value) => value.validate_contract(),
            Self::ThresholdEvaluation(value) => value.validate_contract(),
            Self::TrialSet(value) => value.validate_contract(),
            Self::ResourceClosure(value) => value.validate_contract(),
            Self::TrustedDomainResourceReceipt(value) => value.validate_structure(),
        }
    }
}

fn admit_p8_quality_artifact(bytes: &[u8]) -> serde_json::Result<P8QualityArtifactEnvelopeV1> {
    reject_p8_quality_raw_sentinels(bytes)?;
    let artifact: P8QualityArtifactEnvelopeV1 = deserialize_p8_quality_artifact(bytes)?;
    let failures = artifact.validate_contract();
    if failures.is_empty() {
        Ok(artifact)
    } else {
        Err(<serde_json::Error as serde::de::Error>::custom(format!(
            "P8 quality contract rejected artifact: {failures:?}"
        )))
    }
}

fn reject_p8_quality_raw_sentinels(bytes: &[u8]) -> serde_json::Result<()> {
    const RAW_SENTINEL_CANARIES: [&str; 7] = [
        "private-owner-sentinel",
        "private-space-sentinel",
        "private-subject-sentinel",
        "raw-procedure-sentinel",
        "raw-soul-sentinel",
        "credential-sentinel",
        "path-sentinel",
    ];

    fn contains_raw_sentinel(value: &serde_json::Value, canaries: &[&str]) -> bool {
        match value {
            serde_json::Value::String(value) => {
                canaries.iter().any(|canary| value.contains(canary))
            }
            serde_json::Value::Array(values) => values
                .iter()
                .any(|value| contains_raw_sentinel(value, canaries)),
            serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
                canaries.iter().any(|canary| key.contains(canary))
                    || contains_raw_sentinel(value, canaries)
            }),
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
                false
            }
        }
    }

    let P8StrictJsonValue(value) = serde_json::from_slice(bytes)?;
    if contains_raw_sentinel(&value, &RAW_SENTINEL_CANARIES) {
        Err(<serde_json::Error as serde::de::Error>::custom(
            "P8 quality artifact contains a forbidden raw-material sentinel",
        ))
    } else {
        Ok(())
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
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn is_strict_sorted_unique<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|window| window[0] < window[1])
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

#[cfg(test)]
mod tests;
