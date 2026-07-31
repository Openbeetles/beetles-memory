//! P8 TrustedSupervisor 的 peer-bound retained-FD 会话 owner。
//!
//! Linux 动态路径只接受 sealed TrustedSupervisor、内核观察的发送方 PID、CSPRNG nonce 与
//! 一次 SCM_RIGHTS exact descriptor set。非 Linux 只能返回 typed N/A，不能降级为 pathname。

use std::{io, path::PathBuf, time::Duration};

#[cfg(target_os = "linux")]
use std::{collections::BTreeMap, os::fd::OwnedFd};

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

#[cfg(target_os = "linux")]
use crate::{
    p8_quality::source_publisher::{
        verify_committed_harness_release, verify_retained_harness_release,
    },
    retained_artifact_fs::RetainedArtifactDirectory,
    sealed_execution::RetainedExecutable,
};

#[cfg(target_os = "linux")]
use crate::sealed_execution::LinuxPeerBoundFdChannel;

use super::super::source_release::{
    P8HarnessExecutableRoleV1, P8HarnessSourceInputManifestV1, P8HarnessSourceInputRef,
    P8WorkspaceSourceObservationV1,
};
use super::super::{
    domain_separated_sha256, has_typed_sha256_prefix, P8QualityContractFailure, P8QualityDigest,
};
#[cfg(target_os = "linux")]
use super::engineering_gate::P8EngineeringGateIdV1;
use super::engineering_gate::{
    canonical_engineering_build_plan_digest, canonical_engineering_gate_registry_digest,
    P8EngineeringToolRoleV1,
};
#[cfg(target_os = "linux")]
use super::publication::P8HarnessPublicationIntentAckV1;
use super::publication::{
    P8HarnessPublicationClosureDraftV1, P8HarnessPublicationIntentV1, P8ParentPublicationStateV1,
};
use super::P8QualityExecutionAuthority;

const P8_TRUSTED_SUPERVISOR_HELLO_SCHEMA: &str = "beetle-memory.p8.trusted-supervisor-hello.v1";
const P8_TRUSTED_SUPERVISOR_SESSION_SCHEMA: &str =
    "beetle-memory.p8.trusted-supervisor-session-receipt.v1";
const P8_TRUSTED_SUPERVISOR_LAUNCH_SCHEMA: &str =
    "beetle-memory.p8.trusted-supervisor-launch-receipt.v1";
pub(crate) const P8_TRUSTED_SUPERVISOR_SESSION_ARG: &str =
    "--p8-quality-trusted-supervisor-session";
#[cfg(target_os = "linux")]
const P8_TRUSTED_SUPERVISOR_CHANNEL_FD_ENV: &str = "BM_P8_TRUSTED_SUPERVISOR_CHANNEL_FD";
#[cfg(target_os = "linux")]
const P8_TRUSTED_SUPERVISOR_CHANNEL_DEADLINE_ENV: &str =
    "BM_P8_TRUSTED_SUPERVISOR_CHANNEL_DEADLINE_MONOTONIC_NANOS";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct P8TrustedSupervisorSessionRef(String);

impl P8TrustedSupervisorSessionRef {
    fn derive(value: &impl Serialize) -> Self {
        let bytes = serde_json::to_vec(value)
            .expect("P8 trusted supervisor session serialization must be infallible");
        Self(format!(
            "p8_trusted_supervisor_session:sha256:{}",
            domain_separated_sha256(
                "p8_quality_trusted_supervisor_session_v1",
                &[bytes.as_slice()]
            )
        ))
    }
}

impl<'de> Deserialize<'de> for P8TrustedSupervisorSessionRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if has_typed_sha256_prefix(&value, "p8_trusted_supervisor_session:sha256:") {
            Ok(Self(value))
        } else {
            Err(D::Error::custom(
                "invalid P8 trusted supervisor session identity",
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8TrustedSupervisorEvidenceV1 {
    LinuxPeerCredentialsAndScmRights,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8TrustedSupervisorSessionReceiptV1 {
    schema: String,
    evidence: P8TrustedSupervisorEvidenceV1,
    nonce_digest: P8QualityDigest,
    parent_pid: u32,
    supervisor_pid: u32,
    supervisor_executable_digest: P8QualityDigest,
    source_input_digest: P8HarnessSourceInputRef,
    source_observation_digest: P8QualityDigest,
    session_plan_digest: P8QualityDigest,
    descriptor_manifest_digest: P8QualityDigest,
    source_root_physical_identity: P8QualityDigest,
    releases_root_physical_identity: P8QualityDigest,
    exact_role_digests: Vec<(P8HarnessExecutableRoleV1, P8QualityDigest)>,
    received_descriptor_count: u8,
    session_digest: P8TrustedSupervisorSessionRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8TrustedSupervisorLaunchReceiptV1 {
    schema: String,
    session_receipt: P8TrustedSupervisorSessionReceiptV1,
    publication_intent: P8HarnessPublicationIntentV1,
    publication_closure: Option<P8HarnessPublicationClosureDraftV1>,
    outer_state: P8OuterPublicationStateV1,
    outer_final_reopen_observed: bool,
    outer_release_directory_identity: Option<(u64, u64)>,
    child_pid: u32,
    exit_code: Option<i32>,
    stdout_byte_len: u64,
    stdout_digest: P8QualityDigest,
    stdout_eof_observed: bool,
    stderr_byte_len: u64,
    stderr_digest: P8QualityDigest,
    stderr_eof_observed: bool,
    launch_digest: P8QualityDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum P8OuterPublicationStateV1 {
    PreCommitFailed,
    CommittedUnattested,
    AwaitingOpaquePublication,
}

fn classify_outer_publication_state(
    closure_state: Option<P8ParentPublicationStateV1>,
    process_closed_cleanly: bool,
    outer_final_reopen_observed: bool,
    closure_identity_matches: bool,
    publisher_digest_matches: bool,
) -> Option<P8OuterPublicationStateV1> {
    match closure_state {
        Some(P8ParentPublicationStateV1::PreCommitFailed) if !outer_final_reopen_observed => {
            Some(P8OuterPublicationStateV1::PreCommitFailed)
        }
        Some(P8ParentPublicationStateV1::CommittedAwaitingOuterClosure)
            if process_closed_cleanly
                && outer_final_reopen_observed
                && closure_identity_matches
                && publisher_digest_matches =>
        {
            Some(P8OuterPublicationStateV1::AwaitingOpaquePublication)
        }
        Some(P8ParentPublicationStateV1::CommittedAwaitingOuterClosure)
            if outer_final_reopen_observed =>
        {
            Some(P8OuterPublicationStateV1::CommittedUnattested)
        }
        Some(P8ParentPublicationStateV1::CommittedUnattested) | None => {
            Some(P8OuterPublicationStateV1::CommittedUnattested)
        }
        Some(P8ParentPublicationStateV1::PreCommitFailed)
        | Some(P8ParentPublicationStateV1::CommittedAwaitingOuterClosure) => None,
    }
}

impl P8TrustedSupervisorLaunchReceiptV1 {
    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.session_receipt.validate_contract();
        if !self.publication_intent.validate_contract() {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if let Some(closure) = &self.publication_closure {
            failures.extend(closure.validate_contract());
            if !self.publication_intent.matches_closure(closure) {
                failures.push(P8QualityContractFailure::PeerBindingMismatch);
            }
        }
        if self.schema != P8_TRUSTED_SUPERVISOR_LAUNCH_SCHEMA
            || self.child_pid != self.session_receipt.supervisor_pid
        {
            failures.push(P8QualityContractFailure::PeerBindingMismatch);
        }
        let (intent_session, intent_plan, intent_descriptors) =
            self.publication_intent.supervisor_binding();
        let publisher_digest_matches = self
            .session_receipt
            .exact_role_digests
            .iter()
            .find(|(role, _)| *role == P8HarnessExecutableRoleV1::SourcePublisher)
            .is_some_and(|(_, digest)| {
                digest == self.publication_intent.publisher_executable_digest()
            });
        if intent_session != self.session_receipt.session_digest()
            || intent_plan != &self.session_receipt.session_plan_digest
            || intent_descriptors != &self.session_receipt.descriptor_manifest_digest
        {
            failures.push(P8QualityContractFailure::PeerBindingMismatch);
        }
        let closed_cleanly = self.exit_code == Some(0)
            && self.stdout_byte_len == 0
            && self.stderr_byte_len == 0
            && self.stdout_digest
                == P8QualityDigest::derive(
                    "p8_trusted_supervisor_closed_stdout_v1",
                    &Vec::<u8>::new(),
                );
        let closed_cleanly = closed_cleanly
            && self.stderr_digest
                == P8QualityDigest::derive(
                    "p8_trusted_supervisor_closed_stderr_v1",
                    &Vec::<u8>::new(),
                )
            && self.stdout_eof_observed
            && self.stderr_eof_observed;
        let closure_identity_matches = self
            .publication_closure
            .as_ref()
            .and_then(P8HarnessPublicationClosureDraftV1::release_directory_identity)
            == self.outer_release_directory_identity;
        if !self.stdout_eof_observed || !self.stderr_eof_observed {
            failures.push(P8QualityContractFailure::PipeClosureMissing);
        }
        match self.outer_state {
            P8OuterPublicationStateV1::PreCommitFailed => {
                if self.publication_closure.as_ref().is_some_and(|closure| {
                    closure.state() != P8ParentPublicationStateV1::PreCommitFailed
                }) || self.outer_final_reopen_observed
                    || self.outer_release_directory_identity.is_some()
                {
                    failures.push(P8QualityContractFailure::TrustedExecutionMissing);
                }
            }
            P8OuterPublicationStateV1::CommittedUnattested => {
                if self.publication_closure.as_ref().is_some_and(|closure| {
                    closure.state() == P8ParentPublicationStateV1::PreCommitFailed
                }) || (self.publication_closure.as_ref().is_some_and(|closure| {
                    closure.state() == P8ParentPublicationStateV1::CommittedAwaitingOuterClosure
                }) && self.outer_final_reopen_observed
                    && closure_identity_matches
                    && publisher_digest_matches
                    && closed_cleanly)
                {
                    failures.push(P8QualityContractFailure::TrustedExecutionMissing);
                }
            }
            P8OuterPublicationStateV1::AwaitingOpaquePublication => {
                if !self.publication_closure.as_ref().is_some_and(|closure| {
                    closure.state() == P8ParentPublicationStateV1::CommittedAwaitingOuterClosure
                }) || !self.outer_final_reopen_observed
                    || self
                        .publication_closure
                        .as_ref()
                        .and_then(P8HarnessPublicationClosureDraftV1::release_directory_identity)
                        != self.outer_release_directory_identity
                    || !publisher_digest_matches
                    || !closed_cleanly
                {
                    failures.push(P8QualityContractFailure::TrustedExecutionMissing);
                }
            }
        }
        if self.launch_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_trusted_supervisor_launch_receipt_v1",
            &(
                &self.schema,
                &self.session_receipt,
                &self.publication_intent,
                &self.publication_closure,
                self.outer_state,
                self.outer_final_reopen_observed,
                self.outer_release_directory_identity,
                self.child_pid,
                self.exit_code,
                self.stdout_byte_len,
                &self.stdout_digest,
                self.stdout_eof_observed,
                self.stderr_byte_len,
                &self.stderr_digest,
                self.stderr_eof_observed,
            ),
        )
    }
}

/// Outer-parent result. `Published` exists only as this non-Clone, non-serde authority minted
/// after both publisher and TrustedSupervisor closure plus an outer retained-root reopen.
pub(crate) enum P8TrustedSupervisorPublicationOutcome {
    Published(P8PublishedHarnessRelease),
    CommittedUnattested(P8TrustedSupervisorLaunchReceiptV1),
    PreCommitFailed(P8TrustedSupervisorLaunchReceiptV1),
}

pub(crate) struct P8PublishedHarnessRelease {
    audit_receipt: P8TrustedSupervisorLaunchReceiptV1,
    #[cfg(target_os = "linux")]
    retained_releases_root: RetainedArtifactDirectory,
    #[cfg(target_os = "linux")]
    retained_release: RetainedArtifactDirectory,
    #[cfg(target_os = "linux")]
    mutation_witness: P8ImmutableRootMutationWitness,
}

impl P8PublishedHarnessRelease {
    #[cfg(target_os = "linux")]
    pub(crate) fn verify_live(&mut self) -> io::Result<&P8TrustedSupervisorLaunchReceiptV1> {
        let expected = self
            .audit_receipt
            .publication_closure
            .as_ref()
            .and_then(P8HarnessPublicationClosureDraftV1::release_directory_identity)
            .ok_or_else(|| invalid_data("P8 published release identity is missing"))?;
        let release = self.audit_receipt.publication_intent.release();
        if verify_retained_harness_release(&self.retained_release, release)? != expected
            || verify_committed_harness_release(&self.retained_releases_root, release)? != expected
        {
            return Err(invalid_data("P8 published retained release drifted"));
        }
        self.mutation_witness.verify_quiet()?;
        Ok(&self.audit_receipt)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct P8TrustedSupervisorLaunchInput {
    pub(crate) source_root: PathBuf,
    pub(crate) releases_root: PathBuf,
    pub(crate) role_executables: Vec<(P8HarnessExecutableRoleV1, PathBuf)>,
    pub(crate) tool_executables: Vec<(P8EngineeringToolRoleV1, PathBuf)>,
    pub(crate) target_root: PathBuf,
    pub(crate) rust_sysroot_root: PathBuf,
    pub(crate) cargo_dependency_cache_root: PathBuf,
    pub(crate) timeout: Duration,
    pub(crate) stdout_bytes: u64,
    pub(crate) stderr_bytes: u64,
    pub(crate) total_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum P8TrustedSupervisorNaReason {
    TrustedLinuxAuthorityUnavailable,
}

pub(crate) enum P8TrustedSupervisorAvailability<T> {
    Established(T),
    NotApplicable(P8TrustedSupervisorNaReason),
}

impl P8TrustedSupervisorSessionReceiptV1 {
    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_TRUSTED_SUPERVISOR_SESSION_SCHEMA
            || self.evidence != P8TrustedSupervisorEvidenceV1::LinuxPeerCredentialsAndScmRights
            || self.parent_pid == 0
            || self.supervisor_pid == 0
            || self.parent_pid == self.supervisor_pid
            || usize::from(self.received_descriptor_count)
                != P8HarnessExecutableRoleV1::ALL.len() + P8EngineeringToolRoleV1::ALL.len() + 5
            || self.exact_role_digests.len() != P8HarnessExecutableRoleV1::ALL.len()
            || self
                .exact_role_digests
                .iter()
                .map(|(role, _)| *role)
                .ne(P8HarnessExecutableRoleV1::ALL)
        {
            failures.push(P8QualityContractFailure::PeerBindingMismatch);
        }
        if self.session_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn session_digest(&self) -> &P8TrustedSupervisorSessionRef {
        &self.session_digest
    }

    fn derived_digest(&self) -> P8TrustedSupervisorSessionRef {
        P8TrustedSupervisorSessionRef::derive(&(
            (
                &self.schema,
                self.evidence,
                &self.nonce_digest,
                self.parent_pid,
                self.supervisor_pid,
                &self.supervisor_executable_digest,
                &self.source_input_digest,
                &self.source_observation_digest,
                &self.session_plan_digest,
                &self.descriptor_manifest_digest,
            ),
            (
                &self.source_root_physical_identity,
                &self.releases_root_physical_identity,
                &self.exact_role_digests,
                self.received_descriptor_count,
            ),
        ))
    }
}

pub(crate) struct P8TrustedSupervisorSessionAuthority {
    session_digest: P8TrustedSupervisorSessionRef,
    nonce_digest: P8QualityDigest,
    session_plan_digest: P8QualityDigest,
    descriptor_manifest_digest: P8QualityDigest,
    supervisor_pid: u32,
    supervisor_executable_digest: P8QualityDigest,
}

impl P8TrustedSupervisorSessionAuthority {
    pub(crate) fn admit(
        self,
        execution: &mut P8QualityExecutionAuthority,
    ) -> io::Result<P8AdmittedTrustedSupervisorSession> {
        execution.verify()?;
        if execution.role() != P8HarnessExecutableRoleV1::TrustedSupervisor
            || self.supervisor_pid != std::process::id()
            || self.supervisor_executable_digest != execution.executable_digest()
        {
            return Err(invalid_data(
                "P8 TrustedSupervisor session no longer matches sealed execution",
            ));
        }
        Ok(P8AdmittedTrustedSupervisorSession {
            session_digest: self.session_digest,
            nonce_digest: self.nonce_digest,
            session_plan_digest: self.session_plan_digest,
            descriptor_manifest_digest: self.descriptor_manifest_digest,
            supervisor_pid: self.supervisor_pid,
        })
    }
}

pub(crate) struct P8AdmittedTrustedSupervisorSession {
    session_digest: P8TrustedSupervisorSessionRef,
    nonce_digest: P8QualityDigest,
    session_plan_digest: P8QualityDigest,
    descriptor_manifest_digest: P8QualityDigest,
    supervisor_pid: u32,
}

impl P8AdmittedTrustedSupervisorSession {
    pub(crate) fn audit_binding(
        &self,
    ) -> (
        &P8TrustedSupervisorSessionRef,
        &P8QualityDigest,
        &P8QualityDigest,
        &P8QualityDigest,
        u32,
    ) {
        (
            &self.session_digest,
            &self.nonce_digest,
            &self.session_plan_digest,
            &self.descriptor_manifest_digest,
            self.supervisor_pid,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8TrustedSupervisorRoleInputV1 {
    role: P8HarnessExecutableRoleV1,
    locator: PathBuf,
    executable_byte_len: u64,
    executable_digest: P8QualityDigest,
    physical_identity_digest: P8QualityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8TrustedSupervisorToolInputV1 {
    role: P8EngineeringToolRoleV1,
    locator: PathBuf,
    executable_byte_len: u64,
    executable_digest: P8QualityDigest,
    physical_identity_digest: P8QualityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8TrustedSupervisorTargetRootInputV1 {
    locator: PathBuf,
    physical_identity_digest: P8QualityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8TrustedSupervisorImmutableRootInputV1 {
    locator: PathBuf,
    physical_identity_digest: P8QualityDigest,
    inventory_digest: P8QualityDigest,
    entry_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8TrustedSupervisorSessionPlanV1 {
    source_input_digest: P8HarnessSourceInputRef,
    source_observation_digest: P8QualityDigest,
    engineering_gate_registry_digest: P8QualityDigest,
    engineering_build_plan_digest: P8QualityDigest,
    descriptor_manifest_digest: P8QualityDigest,
    channel_deadline_monotonic_nanos: u64,
    stdout_bytes: u64,
    stderr_bytes: u64,
    total_bytes: u64,
    plan_digest: P8QualityDigest,
}

impl P8TrustedSupervisorSessionPlanV1 {
    fn new(
        source_input: &P8HarnessSourceInputManifestV1,
        source_observation_digest: P8QualityDigest,
        descriptor_manifest_digest: P8QualityDigest,
        channel_deadline_monotonic_nanos: u64,
        stdout_bytes: u64,
        stderr_bytes: u64,
        total_bytes: u64,
    ) -> Self {
        let mut value = Self {
            source_input_digest: source_input.source_input_digest().clone(),
            source_observation_digest,
            engineering_gate_registry_digest: canonical_engineering_gate_registry_digest(),
            engineering_build_plan_digest: canonical_engineering_build_plan_digest(source_input),
            descriptor_manifest_digest,
            channel_deadline_monotonic_nanos,
            stdout_bytes,
            stderr_bytes,
            total_bytes,
            plan_digest: P8QualityDigest::derive("p8_trusted_supervisor_session_plan_v1", &()),
        };
        value.plan_digest = value.derived_digest();
        value
    }

    fn validate_against(
        &self,
        source_input: &P8HarnessSourceInputManifestV1,
        source_observation_digest: &P8QualityDigest,
        descriptor_manifest_digest: &P8QualityDigest,
        channel_deadline_monotonic_nanos: u64,
    ) -> bool {
        self.source_input_digest == *source_input.source_input_digest()
            && self.source_observation_digest == *source_observation_digest
            && self.engineering_gate_registry_digest == canonical_engineering_gate_registry_digest()
            && self.engineering_build_plan_digest
                == canonical_engineering_build_plan_digest(source_input)
            && self.descriptor_manifest_digest == *descriptor_manifest_digest
            && self.channel_deadline_monotonic_nanos == channel_deadline_monotonic_nanos
            && self.stdout_bytes != 0
            && self.stderr_bytes != 0
            && self.total_bytes >= self.stdout_bytes
            && self.total_bytes >= self.stderr_bytes
            && self.plan_digest == self.derived_digest()
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_trusted_supervisor_session_plan_v1",
            &(
                &self.source_input_digest,
                &self.source_observation_digest,
                &self.engineering_gate_registry_digest,
                &self.engineering_build_plan_digest,
                &self.descriptor_manifest_digest,
                self.channel_deadline_monotonic_nanos,
                self.stdout_bytes,
                self.stderr_bytes,
                self.total_bytes,
            ),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8TrustedSupervisorHelloV1 {
    schema: String,
    nonce: Vec<u8>,
    parent_pid: u32,
    expected_supervisor_pid: u32,
    source_root_locator: PathBuf,
    releases_root_locator: PathBuf,
    source_input: P8HarnessSourceInputManifestV1,
    session_plan: P8TrustedSupervisorSessionPlanV1,
    roles: Vec<P8TrustedSupervisorRoleInputV1>,
    tools: Vec<P8TrustedSupervisorToolInputV1>,
    target_root: P8TrustedSupervisorTargetRootInputV1,
    rust_sysroot_root: P8TrustedSupervisorImmutableRootInputV1,
    cargo_dependency_cache_root: P8TrustedSupervisorImmutableRootInputV1,
}

pub(crate) struct P8TrustedSupervisorInputs {
    #[cfg(target_os = "linux")]
    pub(crate) execution: P8QualityExecutionAuthority,
    #[cfg(target_os = "linux")]
    pub(crate) admitted_session: P8AdmittedTrustedSupervisorSession,
    #[cfg(target_os = "linux")]
    pub(crate) session_receipt: P8TrustedSupervisorSessionReceiptV1,
    #[cfg(target_os = "linux")]
    pub(crate) source_input: P8HarnessSourceInputManifestV1,
    #[cfg(target_os = "linux")]
    pub(crate) source_root: RetainedArtifactDirectory,
    #[cfg(target_os = "linux")]
    source_mutation_witness: P8ImmutableRootMutationWitness,
    #[cfg(target_os = "linux")]
    pub(crate) releases_root: RetainedArtifactDirectory,
    #[cfg(target_os = "linux")]
    pub(crate) role_executables: BTreeMap<P8HarnessExecutableRoleV1, RetainedExecutable>,
    #[cfg(target_os = "linux")]
    pub(crate) tool_executables: BTreeMap<P8EngineeringToolRoleV1, RetainedExecutable>,
    #[cfg(target_os = "linux")]
    pub(crate) target_roots: BTreeMap<P8EngineeringGateIdV1, RetainedArtifactDirectory>,
    #[cfg(target_os = "linux")]
    pub(crate) rust_sysroot_root: P8WitnessedImmutableRoot,
    #[cfg(target_os = "linux")]
    pub(crate) cargo_dependency_cache_root: P8WitnessedImmutableRoot,
    #[cfg(target_os = "linux")]
    pub(crate) parent_channel: LinuxPeerBoundFdChannel,
    #[cfg(target_os = "linux")]
    pub(crate) stdout_bytes: u64,
    #[cfg(target_os = "linux")]
    pub(crate) stderr_bytes: u64,
    #[cfg(target_os = "linux")]
    pub(crate) total_bytes: u64,
}

#[cfg(target_os = "linux")]
pub(crate) fn claim_inputs_from_parent(
) -> io::Result<P8TrustedSupervisorAvailability<P8TrustedSupervisorInputs>> {
    let mut execution = P8QualityExecutionAuthority::claim_trusted_supervisor()?;
    execution.verify()?;
    let parent_pid = observed_parent_pid()?;
    let channel = LinuxPeerBoundFdChannel::claim_inherited(
        P8_TRUSTED_SUPERVISOR_CHANNEL_FD_ENV,
        P8_TRUSTED_SUPERVISOR_CHANNEL_DEADLINE_ENV,
    )?;
    let descriptor_count =
        P8HarnessExecutableRoleV1::ALL.len() + P8EngineeringToolRoleV1::ALL.len() + 5;
    let (hello_bytes, files) = channel.receive_with_files(parent_pid, descriptor_count)?;
    let hello: P8TrustedSupervisorHelloV1 = serde_json::from_slice(&hello_bytes)
        .map_err(|_| invalid_data("P8 TrustedSupervisor hello is invalid"))?;
    if hello.schema != P8_TRUSTED_SUPERVISOR_HELLO_SCHEMA
        || hello.nonce.len() != 32
        || hello.parent_pid != parent_pid
        || hello.expected_supervisor_pid != std::process::id()
        || hello.roles.len() != P8HarnessExecutableRoleV1::ALL.len()
        || hello
            .roles
            .iter()
            .map(|role| role.role)
            .ne(P8HarnessExecutableRoleV1::ALL)
        || hello
            .tools
            .iter()
            .map(|tool| tool.role)
            .ne(P8EngineeringToolRoleV1::ALL)
        || !hello.source_input.validate_contract().is_empty()
    {
        return Err(invalid_data(
            "P8 TrustedSupervisor peer, nonce, source, or role contract is invalid",
        ));
    }

    let mut files = files.into_iter();
    let source_root_file = files
        .next()
        .ok_or_else(|| invalid_data("P8 source-root descriptor is missing"))?;
    let releases_root_file = files
        .next()
        .ok_or_else(|| invalid_data("P8 releases-root descriptor is missing"))?;
    let source_root = RetainedArtifactDirectory::from_retained_directory_file(
        &hello.source_root_locator,
        source_root_file,
    )?;
    let releases_root = RetainedArtifactDirectory::from_retained_directory_file(
        &hello.releases_root_locator,
        releases_root_file,
    )?;
    let source_identity = directory_identity_digest(&source_root)?;
    let releases_identity = directory_identity_digest(&releases_root)?;
    if source_identity == releases_identity {
        return Err(invalid_data(
            "P8 retained source and releases roots are physically aliased",
        ));
    }

    let mut exact_role_digests = Vec::with_capacity(hello.roles.len());
    let mut role_executables = BTreeMap::new();
    let mut role_physical_identities = std::collections::BTreeSet::new();
    let mut role_byte_digests = std::collections::BTreeSet::new();
    for role_input in &hello.roles {
        let file = files
            .next()
            .ok_or_else(|| invalid_data("P8 retained role descriptor is missing"))?;
        let mut executable = RetainedExecutable::from_retained_file(
            &role_input.locator,
            file,
            role_input.executable_byte_len,
        )?;
        let physical_identity = executable_identity_digest(&executable)?;
        let observed = executable.copy_to_verified(&mut io::sink())?;
        let observed_digest = P8QualityDigest::parse(format!("sha256:{}", observed.sha256()))
            .map_err(|_| invalid_data("P8 retained role digest is invalid"))?;
        if observed.byte_len() != role_input.executable_byte_len
            || observed_digest != role_input.executable_digest
            || physical_identity != role_input.physical_identity_digest
            || !role_physical_identities.insert(physical_identity)
            || !role_byte_digests.insert(observed_digest.clone())
            || role_executables
                .insert(role_input.role, executable)
                .is_some()
        {
            return Err(invalid_data("P8 retained role descriptor set is not exact"));
        }
        exact_role_digests.push((role_input.role, observed_digest));
    }
    if role_executables.len() != P8HarnessExecutableRoleV1::ALL.len() {
        return Err(invalid_data(
            "P8 retained role descriptor coverage is incomplete",
        ));
    }
    let mut tool_executables = BTreeMap::new();
    for tool_input in &hello.tools {
        let file = files
            .next()
            .ok_or_else(|| invalid_data("P8 retained tool descriptor is missing"))?;
        let mut executable = RetainedExecutable::from_retained_file(
            &tool_input.locator,
            file,
            tool_input.executable_byte_len,
        )?;
        let physical_identity = executable_identity_digest(&executable)?;
        let observed = executable.copy_to_verified(&mut io::sink())?;
        let observed_digest = P8QualityDigest::parse(format!("sha256:{}", observed.sha256()))
            .map_err(|_| invalid_data("P8 retained tool digest is invalid"))?;
        if observed.byte_len() != tool_input.executable_byte_len
            || observed_digest != tool_input.executable_digest
            || physical_identity != tool_input.physical_identity_digest
            || !role_physical_identities.insert(physical_identity)
            || !role_byte_digests.insert(observed_digest)
            || tool_executables
                .insert(tool_input.role, executable)
                .is_some()
        {
            return Err(invalid_data("P8 retained tool descriptor set is not exact"));
        }
    }
    if tool_executables.len() != P8EngineeringToolRoleV1::ALL.len() {
        return Err(invalid_data(
            "P8 retained tool descriptor coverage is incomplete",
        ));
    }
    let mut directory_identities =
        std::collections::BTreeSet::from([source_identity.clone(), releases_identity.clone()]);
    let target_root_file = files
        .next()
        .ok_or_else(|| invalid_data("P8 retained target-root descriptor is missing"))?;
    let target_root = RetainedArtifactDirectory::from_retained_directory_file(
        &hello.target_root.locator,
        target_root_file,
    )?;
    if !target_root.exact_regular_file_names()?.is_empty() {
        return Err(invalid_data(
            "P8 retained target root is not initially empty",
        ));
    }
    let target_root_identity = directory_identity_digest(&target_root)?;
    if target_root_identity != hello.target_root.physical_identity_digest
        || !directory_identities.insert(target_root_identity)
    {
        return Err(invalid_data("P8 retained descriptor coverage is not exact"));
    }
    let rust_sysroot_root = claim_immutable_root(
        &hello.rust_sysroot_root,
        files
            .next()
            .ok_or_else(|| invalid_data("P8 retained rust-sysroot descriptor is missing"))?,
    )?;
    if !directory_identities.insert(directory_identity_digest(rust_sysroot_root.directory())?) {
        return Err(invalid_data(
            "P8 retained rust-sysroot authority is aliased",
        ));
    }
    let cargo_dependency_cache_root = claim_immutable_root(
        &hello.cargo_dependency_cache_root,
        files
            .next()
            .ok_or_else(|| invalid_data("P8 retained dependency-cache descriptor is missing"))?,
    )?;
    if !directory_identities.insert(directory_identity_digest(
        cargo_dependency_cache_root.directory(),
    )?) || files.next().is_some()
    {
        return Err(invalid_data(
            "P8 retained dependency capability set is not exact",
        ));
    }
    let descriptor_manifest_digest =
        trusted_supervisor_descriptor_manifest_digest(P8TrustedSupervisorDescriptorManifestInput {
            source_root_locator: &hello.source_root_locator,
            source_root_identity: &source_identity,
            releases_root_locator: &hello.releases_root_locator,
            releases_root_identity: &releases_identity,
            roles: &hello.roles,
            tools: &hello.tools,
            target_root: &hello.target_root,
            rust_sysroot_root: &hello.rust_sysroot_root,
            cargo_dependency_cache_root: &hello.cargo_dependency_cache_root,
        });
    source_root.verify_unchanged()?;
    let mut source_mutation_witness =
        P8ImmutableRootMutationWitness::establish(source_root.path())?;
    let source_observation = hello
        .source_input
        .observe_workspace_source(source_root.path())
        .map_err(|_| invalid_data("P8 retained source observation drifted during session claim"))?;
    source_mutation_witness.verify_quiet()?;
    source_root.verify_unchanged()?;
    if source_observation.physical_identity_digest != source_identity {
        return Err(invalid_data(
            "P8 retained source observation is not bound to the claimed root",
        ));
    }
    let source_observation_digest =
        trusted_supervisor_source_observation_digest(&source_observation);
    if !hello.session_plan.validate_against(
        &hello.source_input,
        &source_observation_digest,
        &descriptor_manifest_digest,
        channel.deadline_monotonic_nanos(),
    ) {
        return Err(invalid_data(
            "P8 TrustedSupervisor session plan or descriptor manifest is invalid",
        ));
    }
    let nonce_digest =
        P8QualityDigest::derive("p8_trusted_supervisor_csprng_nonce_v1", &hello.nonce);
    let mut session_receipt = P8TrustedSupervisorSessionReceiptV1 {
        schema: P8_TRUSTED_SUPERVISOR_SESSION_SCHEMA.into(),
        evidence: P8TrustedSupervisorEvidenceV1::LinuxPeerCredentialsAndScmRights,
        nonce_digest: nonce_digest.clone(),
        parent_pid,
        supervisor_pid: std::process::id(),
        supervisor_executable_digest: execution.executable_digest(),
        source_input_digest: hello.source_input.source_input_digest().clone(),
        source_observation_digest,
        session_plan_digest: hello.session_plan.plan_digest.clone(),
        descriptor_manifest_digest,
        source_root_physical_identity: source_identity,
        releases_root_physical_identity: releases_identity,
        exact_role_digests,
        received_descriptor_count: u8::try_from(descriptor_count)
            .map_err(|_| invalid_data("P8 descriptor count overflow"))?,
        session_digest: P8TrustedSupervisorSessionRef::derive(&()),
    };
    session_receipt.session_digest = session_receipt.derived_digest();
    if !session_receipt.validate_contract().is_empty() {
        return Err(invalid_data(
            "P8 TrustedSupervisor session receipt failed self-validation",
        ));
    }
    let session_authority = P8TrustedSupervisorSessionAuthority {
        session_digest: session_receipt.session_digest.clone(),
        nonce_digest,
        session_plan_digest: session_receipt.session_plan_digest.clone(),
        descriptor_manifest_digest: session_receipt.descriptor_manifest_digest.clone(),
        supervisor_pid: std::process::id(),
        supervisor_executable_digest: execution.executable_digest(),
    };
    let admitted_session = session_authority.admit(&mut execution)?;
    let receipt_bytes = serde_json::to_vec(&session_receipt)
        .map_err(|_| invalid_data("P8 TrustedSupervisor session receipt serialization failed"))?;
    let target_roots = allocate_gate_targets(&target_root)?;
    if let Err(send_error) = channel.send_with_files(&receipt_bytes, &[]) {
        let rollback_error = discard_gate_targets(&target_root, &target_roots).err();
        return Err(match rollback_error {
            Some(rollback_error) => io::Error::new(
                send_error.kind(),
                format!(
                    "{send_error}; P8 supervisor target rollback also failed: {rollback_error}"
                ),
            ),
            None => send_error,
        });
    }
    Ok(P8TrustedSupervisorAvailability::Established(
        P8TrustedSupervisorInputs {
            execution,
            admitted_session,
            session_receipt,
            source_input: hello.source_input,
            source_root,
            source_mutation_witness,
            releases_root,
            role_executables,
            tool_executables,
            target_roots,
            rust_sysroot_root,
            cargo_dependency_cache_root,
            parent_channel: channel,
            stdout_bytes: hello.session_plan.stdout_bytes,
            stderr_bytes: hello.session_plan.stderr_bytes,
            total_bytes: hello.session_plan.total_bytes,
        },
    ))
}

#[cfg(target_os = "linux")]
impl P8TrustedSupervisorInputs {
    pub(crate) fn outer_parent_pid(&self) -> u32 {
        self.session_receipt.parent_pid
    }

    pub(crate) fn verify_source_exact_and_quiet(
        &mut self,
    ) -> io::Result<P8WorkspaceSourceObservationV1> {
        self.source_root.verify_unchanged()?;
        let observation = self
            .source_input
            .observe_workspace_source(self.source_root.path())
            .map_err(|_| invalid_data("P8 retained source observation drifted"))?;
        self.source_mutation_witness.verify_quiet()?;
        self.source_root.verify_unchanged()?;
        if trusted_supervisor_source_observation_digest(&observation)
            != self.session_receipt.source_observation_digest
        {
            return Err(invalid_data(
                "P8 retained source observation differs from session admission",
            ));
        }
        Ok(observation)
    }

    pub(crate) fn verify_dependency_roots_exact_and_quiet(&mut self) -> io::Result<()> {
        self.rust_sysroot_root.verify_exact_and_quiet()?;
        self.cargo_dependency_cache_root.verify_exact_and_quiet()?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn launch_peer_bound_session(
    mut input: P8TrustedSupervisorLaunchInput,
) -> io::Result<P8TrustedSupervisorAvailability<P8TrustedSupervisorPublicationOutcome>> {
    use std::{collections::BTreeSet, sync::mpsc, thread, time::Instant};

    use crate::{
        bounded_process::{
            supervise_spawned_child_closed, BoundedProcessLimits, BoundedProcessTermination,
        },
        p8_quality_process::p8_quality_execution_domain,
    };

    if input.timeout.is_zero()
        || input.stdout_bytes == 0
        || input.stderr_bytes == 0
        || input.total_bytes < input.stdout_bytes
        || input.total_bytes < input.stderr_bytes
    {
        return Err(invalid_input(
            "P8 TrustedSupervisor launch limits are invalid",
        ));
    }
    input.role_executables.sort_by_key(|(role, _)| *role);
    if input
        .role_executables
        .iter()
        .map(|(role, _)| *role)
        .ne(P8HarnessExecutableRoleV1::ALL)
    {
        return Err(invalid_input(
            "P8 TrustedSupervisor launch role set is not exact",
        ));
    }
    input.tool_executables.sort_by_key(|(role, _)| *role);
    if input
        .tool_executables
        .iter()
        .map(|(role, _)| *role)
        .ne(P8EngineeringToolRoleV1::ALL)
    {
        return Err(invalid_input(
            "P8 TrustedSupervisor launch tool set is not exact",
        ));
    }
    let source_root = RetainedArtifactDirectory::open_root(&input.source_root)?;
    let releases_root = RetainedArtifactDirectory::open_root(&input.releases_root)?;
    let source_root_identity = directory_identity_digest(&source_root)?;
    let releases_root_identity = directory_identity_digest(&releases_root)?;
    if source_root_identity == releases_root_identity {
        return Err(invalid_data(
            "P8 source and releases roots are physically aliased",
        ));
    }
    let source_input = P8HarnessSourceInputManifestV1::materialized_from_workspace_build()
        .map_err(|_| invalid_data("P8 workspace source input is unavailable"))?;
    source_root.verify_unchanged()?;
    let source_observation = source_input
        .observe_workspace_source(source_root.path())
        .map_err(|_| invalid_data("P8 workspace source observation drifted before launch"))?;
    source_root.verify_unchanged()?;
    if source_observation.physical_identity_digest != source_root_identity {
        return Err(invalid_data(
            "P8 workspace source observation is not bound to the retained root",
        ));
    }
    let source_observation_digest =
        trusted_supervisor_source_observation_digest(&source_observation);

    let mut retained_roles = Vec::with_capacity(input.role_executables.len());
    let mut role_inputs = Vec::with_capacity(input.role_executables.len());
    let mut descriptor_files = vec![
        source_root.try_clone_directory_file()?,
        releases_root.try_clone_directory_file()?,
    ];
    let mut role_digests = BTreeSet::new();
    let mut role_physical_identities = BTreeSet::new();
    for (role, path) in &input.role_executables {
        let mut executable = RetainedExecutable::open(path)?;
        let physical_identity_digest = executable_identity_digest(&executable)?;
        let observed = executable.copy_to_verified(&mut io::sink())?;
        let digest = P8QualityDigest::parse(format!("sha256:{}", observed.sha256()))
            .map_err(|_| invalid_data("P8 retained role digest is invalid"))?;
        if !role_digests.insert(digest.clone())
            || !role_physical_identities.insert(physical_identity_digest.clone())
        {
            return Err(invalid_data(
                "P8 TrustedSupervisor role executable authority is aliased",
            ));
        }
        descriptor_files.push(executable.try_clone_file()?);
        role_inputs.push(P8TrustedSupervisorRoleInputV1 {
            role: *role,
            locator: path.clone(),
            executable_byte_len: observed.byte_len(),
            executable_digest: digest,
            physical_identity_digest,
        });
        retained_roles.push((*role, executable));
    }
    let mut _retained_tools = Vec::with_capacity(input.tool_executables.len());
    let mut tool_inputs = Vec::with_capacity(input.tool_executables.len());
    for (role, path) in &input.tool_executables {
        let mut executable = RetainedExecutable::open(path)?;
        let physical_identity_digest = executable_identity_digest(&executable)?;
        let observed = executable.copy_to_verified(&mut io::sink())?;
        let digest = P8QualityDigest::parse(format!("sha256:{}", observed.sha256()))
            .map_err(|_| invalid_data("P8 retained tool digest is invalid"))?;
        if !role_physical_identities.insert(physical_identity_digest.clone())
            || !role_digests.insert(digest.clone())
        {
            return Err(invalid_data(
                "P8 TrustedSupervisor executable capability is aliased across roles",
            ));
        }
        descriptor_files.push(executable.try_clone_file()?);
        tool_inputs.push(P8TrustedSupervisorToolInputV1 {
            role: *role,
            locator: path.clone(),
            executable_byte_len: observed.byte_len(),
            executable_digest: digest,
            physical_identity_digest,
        });
        _retained_tools.push((*role, executable));
    }
    let mut directory_identities =
        BTreeSet::from([source_root_identity.clone(), releases_root_identity.clone()]);
    let target_root = RetainedArtifactDirectory::open_root(&input.target_root)?;
    if !target_root.exact_regular_file_names()?.is_empty() {
        return Err(invalid_data(
            "P8 TrustedSupervisor target root is not initially empty",
        ));
    }
    let target_root_identity = directory_identity_digest(&target_root)?;
    if !directory_identities.insert(target_root_identity.clone()) {
        return Err(invalid_data(
            "P8 TrustedSupervisor target root authority is aliased",
        ));
    }
    descriptor_files.push(target_root.try_clone_directory_file()?);
    let target_root_input = P8TrustedSupervisorTargetRootInputV1 {
        locator: input.target_root.clone(),
        physical_identity_digest: target_root_identity,
    };
    let (rust_sysroot_root, rust_sysroot_root_input) =
        prepare_immutable_root(&input.rust_sysroot_root)?;
    let rust_sysroot_identity = directory_identity_digest(&rust_sysroot_root)?;
    if !directory_identities.insert(rust_sysroot_identity) {
        return Err(invalid_data(
            "P8 TrustedSupervisor rust-sysroot authority is aliased",
        ));
    }
    descriptor_files.push(rust_sysroot_root.try_clone_directory_file()?);
    let (cargo_dependency_cache_root, cargo_dependency_cache_root_input) =
        prepare_immutable_root(&input.cargo_dependency_cache_root)?;
    let cargo_dependency_cache_identity = directory_identity_digest(&cargo_dependency_cache_root)?;
    if !directory_identities.insert(cargo_dependency_cache_identity) {
        return Err(invalid_data(
            "P8 TrustedSupervisor dependency-cache authority is aliased",
        ));
    }
    descriptor_files.push(cargo_dependency_cache_root.try_clone_directory_file()?);
    let supervisor_index = retained_roles
        .iter()
        .position(|(role, _)| *role == P8HarnessExecutableRoleV1::TrustedSupervisor)
        .ok_or_else(|| invalid_data("P8 TrustedSupervisor executable is missing"))?;

    let nonce = csprng_session_nonce()?;
    let attempt_nonce = u64::from_le_bytes(
        nonce[..8]
            .try_into()
            .expect("32-byte nonce contains an eight-byte prefix"),
    );
    let started = Instant::now();
    let (parent_channel, child_channel) =
        LinuxPeerBoundFdChannel::pair_with_timeout(input.timeout)?;
    let child_channel_file = child_channel.inheritable_duplicate()?;
    use std::os::fd::AsRawFd as _;
    let child_channel_fd = child_channel_file.as_raw_fd();
    let prepared = retained_roles[supervisor_index]
        .1
        .prepare_with_linux_pre_exec_barrier_and_environment(
            p8_quality_execution_domain(),
            &[P8_TRUSTED_SUPERVISOR_SESSION_ARG.to_string()],
            attempt_nonce,
            &[
                (
                    P8_TRUSTED_SUPERVISOR_CHANNEL_FD_ENV.to_string(),
                    child_channel_fd.to_string(),
                ),
                (
                    P8_TRUSTED_SUPERVISOR_CHANNEL_DEADLINE_ENV.to_string(),
                    child_channel.deadline_monotonic_nanos().to_string(),
                ),
            ],
            vec![child_channel_file],
        )?;
    let expected_supervisor_digest = role_inputs[supervisor_index].executable_digest.clone();
    let (mut broker, launch) = prepared.into_broker_and_launch();
    let (sender, receiver) = mpsc::sync_channel(1);
    let launch_thread = thread::spawn(move || {
        if let Err(mpsc::SendError(Ok(mut spawned))) = sender.send(launch.spawn_piped()) {
            let _ = spawned.child.kill();
            let _ = spawned.child.wait();
        }
    });
    let barrier_timeout = match remaining_launch_timeout(input.timeout, started) {
        Ok(timeout) => timeout,
        Err(error) => {
            drop(broker);
            return Err(settle_failed_supervisor_launch(
                receiver,
                launch_thread,
                None,
                error,
                &input,
                started,
            ));
        }
    };
    let child_pid = match broker.wait_ready(barrier_timeout) {
        Ok(child_pid) => child_pid,
        Err(barrier_error) => {
            drop(broker);
            return Err(settle_failed_supervisor_launch(
                receiver,
                launch_thread,
                None,
                barrier_error,
                &input,
                started,
            ));
        }
    };
    if let Err(release_error) = broker.release(child_pid) {
        return Err(settle_failed_supervisor_launch(
            receiver,
            launch_thread,
            Some(child_pid),
            release_error,
            &input,
            started,
        ));
    }
    let spawn_timeout = match remaining_launch_timeout(input.timeout, started) {
        Ok(timeout) => timeout,
        Err(error) => {
            return Err(settle_failed_supervisor_launch(
                receiver,
                launch_thread,
                Some(child_pid),
                error,
                &input,
                started,
            ));
        }
    };
    let spawned = match receiver.recv_timeout(spawn_timeout) {
        Ok(Ok(spawned)) => spawned,
        Ok(Err(spawn_error)) => {
            if launch_thread.join().is_err() {
                return Err(io::Error::new(
                    spawn_error.kind(),
                    format!("{spawn_error}; P8 supervisor launch worker also panicked"),
                ));
            }
            return Err(spawn_error);
        }
        Err(_) => {
            return Err(settle_failed_supervisor_launch(
                receiver,
                launch_thread,
                Some(child_pid),
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "P8 TrustedSupervisor sealed spawn timed out",
                ),
                &input,
                started,
            ));
        }
    };
    if launch_thread.join().is_err() {
        cleanup_spawned_supervisor_or_abort(spawned, &input, started);
        return Err(invalid_data("P8 TrustedSupervisor launch worker panicked"));
    }
    let (child, guard, executable_identity) = spawned.into_parts();
    drop(guard);
    drop(child_channel);
    let mut child = Some(child);
    if child
        .as_ref()
        .expect("P8 supervisor child is retained")
        .id()
        != child_pid
    {
        supervise_supervisor_child_or_abort(
            child
                .take()
                .expect("P8 supervisor child is retained for mismatch cleanup"),
            BoundedProcessLimits {
                stdout_bytes: input.stdout_bytes,
                stderr_bytes: input.stderr_bytes,
                total_bytes: input.total_bytes,
                timeout: Duration::from_millis(1),
            },
            started,
        );
        return Err(invalid_data(
            "P8 TrustedSupervisor barrier PID differs from spawned child",
        ));
    }

    let descriptor_manifest_digest =
        trusted_supervisor_descriptor_manifest_digest(P8TrustedSupervisorDescriptorManifestInput {
            source_root_locator: &input.source_root,
            source_root_identity: &source_root_identity,
            releases_root_locator: &input.releases_root,
            releases_root_identity: &releases_root_identity,
            roles: &role_inputs,
            tools: &tool_inputs,
            target_root: &target_root_input,
            rust_sysroot_root: &rust_sysroot_root_input,
            cargo_dependency_cache_root: &cargo_dependency_cache_root_input,
        });
    let session_plan = P8TrustedSupervisorSessionPlanV1::new(
        &source_input,
        source_observation_digest,
        descriptor_manifest_digest.clone(),
        parent_channel.deadline_monotonic_nanos(),
        input.stdout_bytes,
        input.stderr_bytes,
        input.total_bytes,
    );
    let hello = P8TrustedSupervisorHelloV1 {
        schema: P8_TRUSTED_SUPERVISOR_HELLO_SCHEMA.into(),
        nonce: nonce.to_vec(),
        parent_pid: std::process::id(),
        expected_supervisor_pid: child_pid,
        source_root_locator: input.source_root,
        releases_root_locator: input.releases_root,
        source_input,
        session_plan: session_plan.clone(),
        roles: role_inputs,
        tools: tool_inputs,
        target_root: target_root_input,
        rust_sysroot_root: rust_sysroot_root_input,
        cargo_dependency_cache_root: cargo_dependency_cache_root_input,
    };
    let session_receipt = (|| -> io::Result<P8TrustedSupervisorSessionReceiptV1> {
        let hello_bytes = serde_json::to_vec(&hello)
            .map_err(|_| invalid_data("P8 TrustedSupervisor hello serialization failed"))?;
        let descriptor_refs = descriptor_files.iter().collect::<Vec<_>>();
        parent_channel.send_with_files(&hello_bytes, &descriptor_refs)?;
        let (session_bytes, returned_files) = parent_channel.receive_with_files(child_pid, 0)?;
        if !returned_files.is_empty() {
            return Err(invalid_data(
                "P8 TrustedSupervisor session receipt returned unexpected descriptors",
            ));
        }
        let session_receipt: P8TrustedSupervisorSessionReceiptV1 =
            serde_json::from_slice(&session_bytes)
                .map_err(|_| invalid_data("P8 TrustedSupervisor session receipt is invalid"))?;
        if !session_receipt.validate_contract().is_empty()
            || session_receipt.parent_pid != std::process::id()
            || session_receipt.supervisor_pid != child_pid
            || session_receipt.supervisor_executable_digest != expected_supervisor_digest
            || session_receipt.session_plan_digest != session_plan.plan_digest
            || session_receipt.descriptor_manifest_digest != descriptor_manifest_digest
            || session_receipt.nonce_digest
                != P8QualityDigest::derive("p8_trusted_supervisor_csprng_nonce_v1", &nonce.to_vec())
        {
            return Err(invalid_data(
                "P8 TrustedSupervisor session receipt differs from parent observations",
            ));
        }
        Ok(session_receipt)
    })();
    let session_receipt = match session_receipt {
        Ok(receipt) => receipt,
        Err(protocol_error) => {
            supervise_supervisor_child_or_abort(
                child
                    .take()
                    .expect("P8 supervisor child is owned until bounded closure"),
                BoundedProcessLimits {
                    stdout_bytes: input.stdout_bytes,
                    stderr_bytes: input.stderr_bytes,
                    total_bytes: input.total_bytes,
                    timeout: Duration::from_millis(1),
                },
                started,
            );
            return Err(protocol_error);
        }
    };
    let publication_intent = (|| -> io::Result<P8HarnessPublicationIntentV1> {
        let (intent_bytes, intent_files) = parent_channel.receive_with_files(child_pid, 0)?;
        if !intent_files.is_empty() {
            return Err(invalid_data(
                "P8 publication intent carried unexpected descriptors",
            ));
        }
        let intent: P8HarnessPublicationIntentV1 = serde_json::from_slice(&intent_bytes)
            .map_err(|_| invalid_data("P8 publication intent is invalid"))?;
        let (intent_session, intent_plan, intent_descriptors) = intent.supervisor_binding();
        if !intent.validate_contract()
            || intent_session != session_receipt.session_digest()
            || intent_plan != &session_receipt.session_plan_digest
            || intent_descriptors != &session_receipt.descriptor_manifest_digest
            || session_receipt
                .exact_role_digests
                .iter()
                .find(|(role, _)| *role == P8HarnessExecutableRoleV1::SourcePublisher)
                .is_none_or(|(_, digest)| digest != intent.publisher_executable_digest())
        {
            return Err(invalid_data(
                "P8 publication intent differs from the admitted supervisor session",
            ));
        }
        releases_root.verify_subdirectory_absent(intent.release().content_address())?;
        releases_root.verify_subdirectory_absent(intent.stage_name())?;
        let ack = P8HarnessPublicationIntentAckV1::new(
            &intent,
            std::process::id(),
            child_pid,
            parent_channel.deadline_monotonic_nanos(),
        );
        parent_channel.send_with_files(
            &serde_json::to_vec(&ack)
                .map_err(|_| invalid_data("P8 publication intent ack serialization failed"))?,
            &[],
        )?;
        Ok(intent)
    })();
    let publication_intent = match publication_intent {
        Ok(intent) => intent,
        Err(protocol_error) => {
            supervise_supervisor_child_or_abort(
                child
                    .take()
                    .expect("P8 supervisor child is retained until publication intent"),
                BoundedProcessLimits {
                    stdout_bytes: input.stdout_bytes,
                    stderr_bytes: input.stderr_bytes,
                    total_bytes: input.total_bytes,
                    timeout: Duration::from_millis(1),
                },
                started,
            );
            return Err(protocol_error);
        }
    };
    let publication_closure = (|| -> io::Result<P8HarnessPublicationClosureDraftV1> {
        let (closure_bytes, closure_files) = parent_channel.receive_with_files(child_pid, 0)?;
        if !closure_files.is_empty() {
            return Err(invalid_data(
                "P8 TrustedSupervisor publication closure carried unexpected descriptors",
            ));
        }
        let closure: P8HarnessPublicationClosureDraftV1 = serde_json::from_slice(&closure_bytes)
            .map_err(|_| invalid_data("P8 publication closure draft is invalid"))?;
        if !closure.validate_contract().is_empty() {
            return Err(invalid_data(
                "P8 publication closure draft failed outer validation",
            ));
        }
        if !publication_intent.matches_closure(&closure) {
            return Err(invalid_data(
                "P8 publication closure differs from the acknowledged intent",
            ));
        }
        let (closure_session, closure_plan, closure_descriptors) = closure.supervisor_binding();
        if closure_session != session_receipt.session_digest()
            || closure_plan != &session_receipt.session_plan_digest
            || closure_descriptors != &session_receipt.descriptor_manifest_digest
        {
            return Err(invalid_data(
                "P8 publication closure differs from the admitted supervisor session",
            ));
        }
        Ok(closure)
    })()
    .ok();
    let mut retained_publication = publication_closure
        .as_ref()
        .and_then(P8HarnessPublicationClosureDraftV1::release_directory_identity)
        .and_then(|expected_identity| {
            let directory = releases_root
                .open_existing_subdirectory(publication_intent.release().content_address())
                .ok()?;
            if verify_retained_harness_release(&directory, publication_intent.release()).ok()?
                != expected_identity
            {
                return None;
            }
            let mut witness = P8ImmutableRootMutationWitness::establish(directory.path()).ok()?;
            if verify_retained_harness_release(&directory, publication_intent.release()).ok()?
                != expected_identity
                || witness.verify_quiet().is_err()
            {
                return None;
            }
            Some((directory, witness, expected_identity))
        });
    let final_timeout =
        remaining_launch_timeout(input.timeout, started).unwrap_or(Duration::from_millis(1));
    let output = match supervise_spawned_child_closed(
        child
            .take()
            .expect("P8 supervisor child is owned until bounded closure"),
        BoundedProcessLimits {
            stdout_bytes: input.stdout_bytes,
            stderr_bytes: input.stderr_bytes,
            total_bytes: input.total_bytes,
            timeout: final_timeout,
        },
        started,
    ) {
        Ok(output) => output,
        Err(_) => std::process::abort(),
    };
    let supervisor_identity_verified = expected_supervisor_digest
        .as_str()
        .strip_prefix("sha256:")
        .is_some_and(|expected| executable_identity.sha256() == expected)
        && retained_roles[supervisor_index]
            .1
            .verify_content(&executable_identity)
            .is_ok();
    let process_closed_cleanly = output.termination() == BoundedProcessTermination::Exited
        && output.status().success()
        && output.stdout().is_empty()
        && output.stderr().is_empty()
        && output.stdout_eof_observed()
        && output.stderr_eof_observed()
        && supervisor_identity_verified;
    let declared_precommit = publication_closure
        .as_ref()
        .is_some_and(|closure| closure.state() == P8ParentPublicationStateV1::PreCommitFailed);
    if retained_publication.is_none() && !declared_precommit {
        if let Ok(expected_identity) =
            verify_committed_harness_release(&releases_root, publication_intent.release())
        {
            let directory = releases_root
                .open_existing_subdirectory(publication_intent.release().content_address())?;
            let mut witness = P8ImmutableRootMutationWitness::establish(directory.path())?;
            if verify_retained_harness_release(&directory, publication_intent.release())?
                != expected_identity
                || witness.verify_quiet().is_err()
                || verify_retained_harness_release(&directory, publication_intent.release())?
                    != expected_identity
                || witness.verify_quiet().is_err()
            {
                std::process::abort();
            }
            retained_publication = Some((directory, witness, expected_identity));
        } else if releases_root
            .verify_subdirectory_absent(publication_intent.release().content_address())
            .is_err()
        {
            // The content address is neither exact nor absent after direct-child closure.
            std::process::abort();
        }
    }
    if declared_precommit
        && releases_root
            .verify_subdirectory_absent(publication_intent.release().content_address())
            .is_err()
    {
        std::process::abort();
    }
    let outer_final_reopen_observed =
        retained_publication
            .as_mut()
            .is_some_and(|(directory, witness, expected_identity)| {
                witness.verify_quiet().is_ok()
                    && verify_retained_harness_release(directory, publication_intent.release())
                        .is_ok_and(|observed| observed == *expected_identity)
                    && verify_committed_harness_release(
                        &releases_root,
                        publication_intent.release(),
                    )
                    .is_ok_and(|observed| observed == *expected_identity)
                    && witness.verify_quiet().is_ok()
                    && verify_retained_harness_release(directory, publication_intent.release())
                        .is_ok_and(|observed| observed == *expected_identity)
                    && witness.verify_quiet().is_ok()
            });
    let outer_release_directory_identity = outer_final_reopen_observed.then(|| {
        retained_publication
            .as_ref()
            .expect("successful outer reopen retains exact identity")
            .2
    });
    let closure_identity_matches = publication_closure
        .as_ref()
        .and_then(P8HarnessPublicationClosureDraftV1::release_directory_identity)
        == outer_release_directory_identity;
    let publisher_digest_matches = session_receipt
        .exact_role_digests
        .iter()
        .find(|(role, _)| *role == P8HarnessExecutableRoleV1::SourcePublisher)
        .is_some_and(|(_, digest)| digest == publication_intent.publisher_executable_digest());
    let outer_state = classify_outer_publication_state(
        publication_closure.as_ref().map(|closure| closure.state()),
        process_closed_cleanly,
        outer_final_reopen_observed,
        closure_identity_matches,
        publisher_digest_matches,
    )
    .unwrap_or_else(|| std::process::abort());
    let mut receipt = P8TrustedSupervisorLaunchReceiptV1 {
        schema: P8_TRUSTED_SUPERVISOR_LAUNCH_SCHEMA.into(),
        session_receipt,
        publication_intent,
        publication_closure,
        outer_state,
        outer_final_reopen_observed,
        outer_release_directory_identity,
        child_pid,
        exit_code: output.status().code(),
        stdout_byte_len: u64::try_from(output.stdout().len())
            .map_err(|_| invalid_data("P8 supervisor stdout length overflow"))?,
        stdout_digest: P8QualityDigest::derive(
            "p8_trusted_supervisor_closed_stdout_v1",
            &output.stdout(),
        ),
        stdout_eof_observed: output.stdout_eof_observed(),
        stderr_byte_len: u64::try_from(output.stderr().len())
            .map_err(|_| invalid_data("P8 supervisor stderr length overflow"))?,
        stderr_digest: P8QualityDigest::derive(
            "p8_trusted_supervisor_closed_stderr_v1",
            &output.stderr(),
        ),
        stderr_eof_observed: output.stderr_eof_observed(),
        launch_digest: P8QualityDigest::derive("p8_trusted_supervisor_launch_receipt_v1", &()),
    };
    receipt.launch_digest = receipt.derived_digest();
    if !receipt.validate_contract().is_empty() {
        return Err(invalid_data(
            "P8 TrustedSupervisor launch receipt failed self-validation",
        ));
    }
    let outcome = match receipt.outer_state {
        P8OuterPublicationStateV1::AwaitingOpaquePublication => {
            let (retained_release, mutation_witness, _) = retained_publication
                .take()
                .expect("opaque publication requires retained final capability");
            P8TrustedSupervisorPublicationOutcome::Published(P8PublishedHarnessRelease {
                audit_receipt: receipt,
                retained_releases_root: releases_root,
                retained_release,
                mutation_witness,
            })
        }
        P8OuterPublicationStateV1::CommittedUnattested => {
            P8TrustedSupervisorPublicationOutcome::CommittedUnattested(receipt)
        }
        P8OuterPublicationStateV1::PreCommitFailed => {
            P8TrustedSupervisorPublicationOutcome::PreCommitFailed(receipt)
        }
    };
    Ok(P8TrustedSupervisorAvailability::Established(outcome))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn launch_peer_bound_session(
    _input: P8TrustedSupervisorLaunchInput,
) -> io::Result<P8TrustedSupervisorAvailability<P8TrustedSupervisorPublicationOutcome>> {
    Ok(P8TrustedSupervisorAvailability::NotApplicable(
        P8TrustedSupervisorNaReason::TrustedLinuxAuthorityUnavailable,
    ))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn claim_inputs_from_parent(
) -> io::Result<P8TrustedSupervisorAvailability<P8TrustedSupervisorInputs>> {
    Ok(P8TrustedSupervisorAvailability::NotApplicable(
        P8TrustedSupervisorNaReason::TrustedLinuxAuthorityUnavailable,
    ))
}

#[cfg(target_os = "linux")]
fn observed_parent_pid() -> io::Result<u32> {
    u32::try_from(unsafe { libc::getppid() })
        .ok()
        .filter(|pid| *pid != 0)
        .ok_or_else(|| invalid_data("P8 TrustedSupervisor parent PID is invalid"))
}

#[cfg(target_os = "linux")]
pub(crate) fn directory_identity_digest(
    directory: &RetainedArtifactDirectory,
) -> io::Result<P8QualityDigest> {
    let (device, inode) = directory.unix_physical_identity()?;
    Ok(P8QualityDigest::derive(
        "p8_trusted_supervisor_retained_directory_v1",
        &(device, inode),
    ))
}

#[cfg(target_os = "linux")]
fn executable_identity_digest(executable: &RetainedExecutable) -> io::Result<P8QualityDigest> {
    let (device, inode) = executable.unix_physical_identity()?;
    Ok(P8QualityDigest::derive(
        "p8_trusted_supervisor_retained_executable_v1",
        &(device, inode),
    ))
}

#[derive(Serialize)]
struct P8TrustedSupervisorDescriptorManifestInput<'a> {
    source_root_locator: &'a std::path::Path,
    source_root_identity: &'a P8QualityDigest,
    releases_root_locator: &'a std::path::Path,
    releases_root_identity: &'a P8QualityDigest,
    roles: &'a [P8TrustedSupervisorRoleInputV1],
    tools: &'a [P8TrustedSupervisorToolInputV1],
    target_root: &'a P8TrustedSupervisorTargetRootInputV1,
    rust_sysroot_root: &'a P8TrustedSupervisorImmutableRootInputV1,
    cargo_dependency_cache_root: &'a P8TrustedSupervisorImmutableRootInputV1,
}

fn trusted_supervisor_descriptor_manifest_digest(
    input: P8TrustedSupervisorDescriptorManifestInput<'_>,
) -> P8QualityDigest {
    P8QualityDigest::derive("p8_trusted_supervisor_descriptor_manifest_v1", &input)
}

fn trusted_supervisor_source_observation_digest(
    observation: &P8WorkspaceSourceObservationV1,
) -> P8QualityDigest {
    P8QualityDigest::derive(
        "p8_trusted_supervisor_source_observation_v1",
        &(
            &observation.retained_source_lease_digest,
            &observation.physical_identity_digest,
            &observation.inventory_digest,
        ),
    )
}

#[cfg(target_os = "linux")]
fn prepare_immutable_root(
    path: &std::path::Path,
) -> io::Result<(
    RetainedArtifactDirectory,
    P8TrustedSupervisorImmutableRootInputV1,
)> {
    let root = RetainedArtifactDirectory::open_root(path)?;
    root.verify_unchanged()?;
    let (inventory_digest, entry_count) = observe_immutable_root_inventory(root.path())?;
    root.verify_unchanged()?;
    let input = P8TrustedSupervisorImmutableRootInputV1 {
        locator: path.to_path_buf(),
        physical_identity_digest: directory_identity_digest(&root)?,
        inventory_digest,
        entry_count,
    };
    Ok((root, input))
}

#[cfg(target_os = "linux")]
fn claim_immutable_root(
    input: &P8TrustedSupervisorImmutableRootInputV1,
    file: std::fs::File,
) -> io::Result<P8WitnessedImmutableRoot> {
    let root = RetainedArtifactDirectory::from_retained_directory_file(&input.locator, file)?;
    root.verify_unchanged()?;
    let mut mutation_witness = P8ImmutableRootMutationWitness::establish(root.path())?;
    let (inventory_digest, entry_count) = observe_immutable_root_inventory(root.path())?;
    mutation_witness.verify_quiet()?;
    root.verify_unchanged()?;
    if directory_identity_digest(&root)? != input.physical_identity_digest
        || inventory_digest != input.inventory_digest
        || entry_count != input.entry_count
        || entry_count == 0
    {
        return Err(invalid_data(
            "P8 retained immutable-root observation is not exact",
        ));
    }
    Ok(P8WitnessedImmutableRoot {
        root,
        expected_inventory_digest: input.inventory_digest.clone(),
        expected_entry_count: input.entry_count,
        mutation_witness,
    })
}

/// Linux inotify 是 retained directory FD 之外的后代变更见证者。它不声称阻止同 UID
/// 写入；任何写入、替换、属性变更、目录拓扑变化或队列溢出都会让本次 session 永久失效。
/// 该 owner 不实现 Clone/Serialize/Deserialize，不能由 receipt 重建。
#[cfg(target_os = "linux")]
pub(crate) struct P8WitnessedImmutableRoot {
    root: RetainedArtifactDirectory,
    expected_inventory_digest: P8QualityDigest,
    expected_entry_count: u64,
    mutation_witness: P8ImmutableRootMutationWitness,
}

#[cfg(target_os = "linux")]
impl P8WitnessedImmutableRoot {
    pub(crate) fn directory(&self) -> &RetainedArtifactDirectory {
        &self.root
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        self.root.path()
    }

    pub(crate) fn inheritable_directory_file(&self) -> io::Result<std::fs::File> {
        self.root.inheritable_directory_file()
    }

    pub(crate) fn verify_exact_and_quiet(&mut self) -> io::Result<()> {
        self.root.verify_unchanged()?;
        let (inventory_digest, entry_count) = observe_immutable_root_inventory(self.root.path())?;
        self.mutation_witness.verify_quiet()?;
        self.root.verify_unchanged()?;
        if inventory_digest != self.expected_inventory_digest
            || entry_count != self.expected_entry_count
        {
            return Err(invalid_data(
                "P8 witnessed immutable-root inventory drifted",
            ));
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub(crate) struct P8ImmutableRootMutationWitness {
    inotify: OwnedFd,
    watched_directory_count: usize,
    invalidated: bool,
}

#[cfg(target_os = "linux")]
impl P8ImmutableRootMutationWitness {
    pub(crate) fn establish(root: &std::path::Path) -> io::Result<Self> {
        use std::os::fd::FromRawFd as _;

        let raw = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: inotify_init1 returned a newly owned descriptor.
        let inotify = unsafe { OwnedFd::from_raw_fd(raw) };
        let directories = collect_immutable_root_directories(root)?;
        if directories.is_empty() {
            return Err(invalid_data(
                "P8 immutable-root directory inventory is empty",
            ));
        }
        let mask = libc::IN_ATTRIB
            | libc::IN_CLOSE_WRITE
            | libc::IN_CREATE
            | libc::IN_DELETE
            | libc::IN_DELETE_SELF
            | libc::IN_MODIFY
            | libc::IN_MOVE_SELF
            | libc::IN_MOVED_FROM
            | libc::IN_MOVED_TO
            | libc::IN_UNMOUNT
            | libc::IN_ONLYDIR
            | libc::IN_DONT_FOLLOW
            | libc::IN_EXCL_UNLINK;
        use std::os::fd::AsRawFd as _;
        for directory in &directories {
            use std::os::unix::ffi::OsStrExt as _;

            let c_path = std::ffi::CString::new(directory.as_os_str().as_bytes())
                .map_err(|_| invalid_data("P8 immutable-root directory contains NUL"))?;
            if unsafe { libc::inotify_add_watch(inotify.as_raw_fd(), c_path.as_ptr(), mask) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let witness = Self {
            inotify,
            watched_directory_count: directories.len(),
            invalidated: false,
        };
        let observed_directories = collect_immutable_root_directories(root)?;
        if observed_directories != directories {
            return Err(invalid_data(
                "P8 immutable-root directory topology drifted during watch admission",
            ));
        }
        Ok(witness)
    }

    pub(crate) fn verify_quiet(&mut self) -> io::Result<()> {
        use std::os::fd::AsRawFd as _;

        if self.invalidated || self.watched_directory_count == 0 {
            self.invalidated = true;
            return Err(invalid_data(
                "P8 immutable-root mutation witness is permanently invalid",
            ));
        }
        let mut buffer = [0_u8; 4096];
        loop {
            let read = unsafe {
                libc::read(
                    self.inotify.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if read > 0 {
                self.invalidated = true;
                return Err(invalid_data(
                    "P8 immutable-root mutation witness observed drift",
                ));
            }
            if read == 0 {
                self.invalidated = true;
                return Err(invalid_data(
                    "P8 immutable-root mutation witness closed unexpectedly",
                ));
            }
            let error = io::Error::last_os_error();
            match error.kind() {
                io::ErrorKind::Interrupted => continue,
                io::ErrorKind::WouldBlock => return Ok(()),
                _ => {
                    self.invalidated = true;
                    return Err(error);
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn collect_immutable_root_directories(
    root: &std::path::Path,
) -> io::Result<Vec<std::path::PathBuf>> {
    fn visit(path: &std::path::Path, directories: &mut Vec<std::path::PathBuf>) -> io::Result<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(invalid_data("P8 immutable-root contains a symlink"));
        }
        if metadata.is_file() {
            return Ok(());
        }
        if !metadata.is_dir() {
            return Err(invalid_data(
                "P8 immutable-root contains a non-regular entry",
            ));
        }
        directories.push(path.to_path_buf());
        let mut entries = std::fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::path);
        for entry in entries {
            visit(&entry.path(), directories)?;
        }
        Ok(())
    }

    let mut directories = Vec::new();
    visit(root, &mut directories)?;
    Ok(directories)
}

#[cfg(target_os = "linux")]
fn observe_immutable_root_inventory(root: &std::path::Path) -> io::Result<(P8QualityDigest, u64)> {
    use sha2::Digest as _;

    let mut files = Vec::new();
    crate::build_support::collect_regular_files(root, &mut files)
        .map_err(|_| invalid_data("P8 immutable-root inventory is invalid"))?;
    crate::build_support::sort_regular_files_relative_to(root, &mut files)
        .map_err(|_| invalid_data("P8 immutable-root inventory escaped its authority"))?;
    if files.is_empty() || files.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invalid_data(
            "P8 immutable-root inventory is empty or duplicated",
        ));
    }
    let mut entries = Vec::with_capacity(files.len());
    for file in files {
        let relative = file
            .strip_prefix(root)
            .ok()
            .and_then(std::path::Path::to_str)
            .map(|value| value.replace('\\', "/"))
            .ok_or_else(|| invalid_data("P8 immutable-root relative path is invalid"))?;
        let bytes = crate::build_support::read_regular_file_stable(&file)
            .map_err(|_| invalid_data("P8 immutable-root file drifted"))?;
        entries.push((
            relative,
            u64::try_from(bytes.len())
                .map_err(|_| invalid_data("P8 immutable-root file length overflow"))?,
            format!("{:x}", sha2::Sha256::digest(&bytes)),
        ));
    }
    let entry_count = u64::try_from(entries.len())
        .map_err(|_| invalid_data("P8 immutable-root entry count overflow"))?;
    Ok((
        P8QualityDigest::derive(
            "p8_trusted_supervisor_immutable_root_inventory_v1",
            &entries,
        ),
        entry_count,
    ))
}

#[cfg(target_os = "linux")]
fn allocate_gate_targets(
    target_root: &RetainedArtifactDirectory,
) -> io::Result<BTreeMap<P8EngineeringGateIdV1, RetainedArtifactDirectory>> {
    let mut targets = BTreeMap::new();
    for gate in P8EngineeringGateIdV1::ALL {
        match target_root.create_new_subdirectory(gate.schema_name()) {
            Ok(target) => {
                if targets.insert(gate, target).is_some() {
                    return Err(invalid_data("P8 supervisor target allocation is not exact"));
                }
            }
            Err(error) => {
                let mut rollback_error = None;
                for (created_gate, target) in targets.iter().rev() {
                    if let Err(discard_error) =
                        target_root.discard_empty_same_directory(target, created_gate.schema_name())
                    {
                        rollback_error.get_or_insert(discard_error);
                    }
                }
                return Err(rollback_error.unwrap_or(error));
            }
        }
    }
    Ok(targets)
}

#[cfg(target_os = "linux")]
fn discard_gate_targets(
    target_root: &RetainedArtifactDirectory,
    targets: &BTreeMap<P8EngineeringGateIdV1, RetainedArtifactDirectory>,
) -> io::Result<()> {
    let mut errors = Vec::new();
    for (gate, target) in targets.iter().rev() {
        if let Err(error) = target_root.discard_empty_same_directory(target, gate.schema_name()) {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "P8 supervisor target rollback failed: {}",
            errors.join("; ")
        )))
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn csprng_session_nonce() -> io::Result<[u8; 32]> {
    loop {
        let mut nonce = [0_u8; 32];
        let mut offset = 0;
        while offset < nonce.len() {
            let result = unsafe {
                libc::getrandom(nonce[offset..].as_mut_ptr().cast(), nonce.len() - offset, 0)
            };
            if result > 0 {
                offset += usize::try_from(result)
                    .map_err(|_| invalid_data("P8 CSPRNG result length is invalid"))?;
                continue;
            }
            if result == 0 {
                return Err(invalid_data("P8 CSPRNG returned no entropy"));
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        if nonce[..8] != [0_u8; 8] {
            return Ok(nonce);
        }
    }
}

#[cfg(target_os = "linux")]
fn remaining_launch_timeout(
    timeout: Duration,
    started: std::time::Instant,
) -> io::Result<Duration> {
    timeout
        .checked_sub(started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "P8 TrustedSupervisor launch wall elapsed",
            )
        })
}

#[cfg(target_os = "linux")]
fn cleanup_spawned_supervisor(
    spawned: crate::sealed_execution::SpawnedLinuxBarrierSealedExecutable,
    input: &P8TrustedSupervisorLaunchInput,
    started: std::time::Instant,
) -> io::Result<()> {
    let (child, guard, _) = spawned.into_parts();
    drop(guard);
    crate::bounded_process::supervise_spawned_child(
        child,
        crate::bounded_process::BoundedProcessLimits {
            stdout_bytes: input.stdout_bytes,
            stderr_bytes: input.stderr_bytes,
            total_bytes: input.total_bytes,
            timeout: Duration::from_millis(1),
        },
        started,
    )
    .map(|_| ())
}

#[cfg(target_os = "linux")]
fn cleanup_spawned_supervisor_or_abort(
    spawned: crate::sealed_execution::SpawnedLinuxBarrierSealedExecutable,
    input: &P8TrustedSupervisorLaunchInput,
    started: std::time::Instant,
) {
    if cleanup_spawned_supervisor(spawned, input, started).is_err() {
        // A caller must never regain control while a process that crossed the pre-exec barrier
        // lacks a positive direct-child wait/reap plus two-EOF closure.
        std::process::abort();
    }
}

#[cfg(target_os = "linux")]
fn supervise_supervisor_child_or_abort(
    child: std::process::Child,
    limits: crate::bounded_process::BoundedProcessLimits,
    started: std::time::Instant,
) {
    if crate::bounded_process::supervise_spawned_child_closed(child, limits, started).is_err() {
        std::process::abort();
    }
}

#[cfg(target_os = "linux")]
fn settle_failed_supervisor_launch(
    receiver: std::sync::mpsc::Receiver<
        io::Result<crate::sealed_execution::SpawnedLinuxBarrierSealedExecutable>,
    >,
    launch_thread: std::thread::JoinHandle<()>,
    ready_pid: Option<u32>,
    primary_error: io::Error,
    input: &P8TrustedSupervisorLaunchInput,
    started: std::time::Instant,
) -> io::Error {
    let kill_error = ready_pid.and_then(|pid| kill_preexec_process_group(pid).err());
    let launch_result = match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(_) => {
            // Returning while the launch worker may still own a pre-exec child would violate the
            // fail-closed process contract. This path is reachable only after the barrier owner
            // has been dropped and an admitted process group has been SIGKILLed.
            std::process::abort();
        }
    };
    let cleanup_error = match launch_result {
        Ok(spawned) => {
            cleanup_spawned_supervisor_or_abort(spawned, input, started);
            None
        }
        Err(error) => Some(error),
    };
    let join_error = launch_thread
        .join()
        .err()
        .map(|_| "P8 supervisor launch worker panicked");
    let mut details = vec![primary_error.to_string()];
    if let Some(error) = kill_error {
        details.push(format!("process-group kill failed: {error}"));
    }
    if let Some(error) = cleanup_error {
        details.push(format!("spawn cleanup failed: {error}"));
    }
    if let Some(error) = join_error {
        details.push(error.to_string());
    }
    io::Error::new(primary_error.kind(), details.join("; "))
}

#[cfg(target_os = "linux")]
fn kill_preexec_process_group(child_pid: u32) -> io::Result<()> {
    let process_group = i32::try_from(child_pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| invalid_data("P8 supervisor process-group PID is invalid"))?;
    // SAFETY: the barrier child called setpgid(0, 0) before emitting READY, so the negative PID
    // names exactly that process group.
    if unsafe { libc::kill(-process_group, libc::SIGKILL) } != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error);
        }
    }
    Ok(())
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_source() -> P8HarnessSourceInputManifestV1 {
        P8HarnessSourceInputManifestV1::fixture(&P8QualityDigest::derive(
            "p8_supervisor_source_fixture_v1",
            &"source",
        ))
    }

    fn fixture_role_inputs() -> Vec<P8TrustedSupervisorRoleInputV1> {
        P8HarnessExecutableRoleV1::ALL
            .into_iter()
            .map(|role| P8TrustedSupervisorRoleInputV1 {
                role,
                locator: PathBuf::from(format!("/fixture/{role:?}")),
                executable_byte_len: 1,
                executable_digest: P8QualityDigest::derive(
                    "p8_supervisor_role_bytes_fixture_v1",
                    &role,
                ),
                physical_identity_digest: P8QualityDigest::derive(
                    "p8_supervisor_role_identity_fixture_v1",
                    &role,
                ),
            })
            .collect()
    }

    fn fixture_tool_inputs() -> Vec<P8TrustedSupervisorToolInputV1> {
        P8EngineeringToolRoleV1::ALL
            .into_iter()
            .map(|role| P8TrustedSupervisorToolInputV1 {
                role,
                locator: PathBuf::from(format!("/fixture/{role:?}")),
                executable_byte_len: 1,
                executable_digest: P8QualityDigest::derive(
                    "p8_supervisor_tool_bytes_fixture_v1",
                    &role,
                ),
                physical_identity_digest: P8QualityDigest::derive(
                    "p8_supervisor_tool_identity_fixture_v1",
                    &role,
                ),
            })
            .collect()
    }

    fn fixture_target_root_input() -> P8TrustedSupervisorTargetRootInputV1 {
        P8TrustedSupervisorTargetRootInputV1 {
            locator: PathBuf::from("/fixture/targets"),
            physical_identity_digest: P8QualityDigest::derive(
                "p8_supervisor_target_identity_fixture_v1",
                &"targets",
            ),
        }
    }

    fn fixture_immutable_root_input(seed: &str) -> P8TrustedSupervisorImmutableRootInputV1 {
        P8TrustedSupervisorImmutableRootInputV1 {
            locator: PathBuf::from(format!("/fixture/{seed}")),
            physical_identity_digest: P8QualityDigest::derive(
                "p8_supervisor_immutable_root_identity_fixture_v1",
                &seed,
            ),
            inventory_digest: P8QualityDigest::derive(
                "p8_supervisor_immutable_root_inventory_fixture_v1",
                &seed,
            ),
            entry_count: 1,
        }
    }

    fn fixture_session_receipt() -> P8TrustedSupervisorSessionReceiptV1 {
        let mut receipt = P8TrustedSupervisorSessionReceiptV1 {
            schema: P8_TRUSTED_SUPERVISOR_SESSION_SCHEMA.into(),
            evidence: P8TrustedSupervisorEvidenceV1::LinuxPeerCredentialsAndScmRights,
            nonce_digest: P8QualityDigest::derive("p8_session_fixture", &"nonce"),
            parent_pid: 41,
            supervisor_pid: 42,
            supervisor_executable_digest: P8QualityDigest::derive(
                "p8_session_fixture",
                &"supervisor",
            ),
            source_input_digest: P8HarnessSourceInputRef::derive_for_test("source"),
            source_observation_digest: P8QualityDigest::derive(
                "p8_session_fixture",
                &"source-observation",
            ),
            session_plan_digest: P8QualityDigest::derive("p8_session_fixture", &"plan"),
            descriptor_manifest_digest: P8QualityDigest::derive(
                "p8_session_fixture",
                &"descriptor-manifest",
            ),
            source_root_physical_identity: P8QualityDigest::derive(
                "p8_session_fixture",
                &"source-root",
            ),
            releases_root_physical_identity: P8QualityDigest::derive(
                "p8_session_fixture",
                &"releases-root",
            ),
            exact_role_digests: P8HarnessExecutableRoleV1::ALL
                .into_iter()
                .map(|role| {
                    (
                        role,
                        P8QualityDigest::derive("p8_session_fixture_role", &role),
                    )
                })
                .collect(),
            received_descriptor_count: 17,
            session_digest: P8TrustedSupervisorSessionRef::derive(&()),
        };
        receipt.session_digest = receipt.derived_digest();
        receipt
    }

    #[test]
    fn peer_channel_owner_freezes_the_supervisor_message_limit() {
        assert_eq!(
            crate::sealed_execution::PEER_CHANNEL_MAX_MESSAGE_BYTES,
            64 * 1024
        );
    }

    #[test]
    fn session_receipt_rejects_peer_and_descriptor_drift() {
        let mut receipt = fixture_session_receipt();
        assert!(receipt.validate_contract().is_empty());

        receipt.received_descriptor_count = 5;
        receipt.session_digest = receipt.derived_digest();
        assert!(receipt
            .validate_contract()
            .contains(&P8QualityContractFailure::PeerBindingMismatch));
    }

    #[test]
    fn legacy_session_receipt_without_plan_or_descriptor_manifest_is_rejected() {
        for required_field in ["session_plan_digest", "descriptor_manifest_digest"] {
            let mut legacy =
                serde_json::to_value(fixture_session_receipt()).expect("serialize legacy receipt");
            legacy
                .as_object_mut()
                .expect("session receipt object")
                .remove(required_field);
            assert!(
                serde_json::from_value::<P8TrustedSupervisorSessionReceiptV1>(legacy).is_err(),
                "legacy session receipt without {required_field} must be rejected"
            );
        }
    }

    #[test]
    fn session_plan_binds_canonical_gate_build_descriptor_and_limits() {
        let source = fixture_source();
        let source_identity = P8QualityDigest::derive("p8_root_fixture_v1", &"source");
        let releases_identity = P8QualityDigest::derive("p8_root_fixture_v1", &"releases");
        let roles = fixture_role_inputs();
        let tools = fixture_tool_inputs();
        let target_root = fixture_target_root_input();
        let rust_sysroot_root = fixture_immutable_root_input("rust-sysroot");
        let cargo_dependency_cache_root = fixture_immutable_root_input("cargo-cache");
        let descriptor_manifest = trusted_supervisor_descriptor_manifest_digest(
            P8TrustedSupervisorDescriptorManifestInput {
                source_root_locator: std::path::Path::new("/fixture/source"),
                source_root_identity: &source_identity,
                releases_root_locator: std::path::Path::new("/fixture/releases"),
                releases_root_identity: &releases_identity,
                roles: &roles,
                tools: &tools,
                target_root: &target_root,
                rust_sysroot_root: &rust_sysroot_root,
                cargo_dependency_cache_root: &cargo_dependency_cache_root,
            },
        );
        let source_observation =
            P8QualityDigest::derive("p8_source_observation_fixture", &"source");
        let plan = P8TrustedSupervisorSessionPlanV1::new(
            &source,
            source_observation.clone(),
            descriptor_manifest.clone(),
            99,
            1,
            1,
            2,
        );
        assert!(plan.validate_against(&source, &source_observation, &descriptor_manifest, 99));

        let reordered_roles = roles.into_iter().rev().collect::<Vec<_>>();
        let reordered_manifest = trusted_supervisor_descriptor_manifest_digest(
            P8TrustedSupervisorDescriptorManifestInput {
                source_root_locator: std::path::Path::new("/fixture/source"),
                source_root_identity: &source_identity,
                releases_root_locator: std::path::Path::new("/fixture/releases"),
                releases_root_identity: &releases_identity,
                roles: &reordered_roles,
                tools: &tools,
                target_root: &target_root,
                rust_sysroot_root: &rust_sysroot_root,
                cargo_dependency_cache_root: &cargo_dependency_cache_root,
            },
        );
        assert_ne!(descriptor_manifest, reordered_manifest);
        assert!(!plan.validate_against(&source, &source_observation, &reordered_manifest, 99));
        assert!(!plan.validate_against(&source, &source_observation, &descriptor_manifest, 100));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_session_is_typed_na_before_any_descriptor_or_staging() {
        assert!(matches!(
            claim_inputs_from_parent().expect("non-Linux typed availability"),
            P8TrustedSupervisorAvailability::NotApplicable(
                P8TrustedSupervisorNaReason::TrustedLinuxAuthorityUnavailable
            )
        ));
        let launch = launch_peer_bound_session(P8TrustedSupervisorLaunchInput {
            source_root: PathBuf::from("/definitely-missing-source"),
            releases_root: PathBuf::from("/definitely-missing-releases"),
            role_executables: Vec::new(),
            tool_executables: Vec::new(),
            target_root: PathBuf::from("/definitely-missing-target"),
            rust_sysroot_root: PathBuf::from("/definitely-missing-sysroot"),
            cargo_dependency_cache_root: PathBuf::from("/definitely-missing-cargo-cache"),
            timeout: Duration::from_secs(1),
            stdout_bytes: 1,
            stderr_bytes: 1,
            total_bytes: 2,
        })
        .expect("non-Linux launch availability must precede path access");
        assert!(matches!(
            launch,
            P8TrustedSupervisorAvailability::NotApplicable(
                P8TrustedSupervisorNaReason::TrustedLinuxAuthorityUnavailable
            )
        ));
    }

    #[test]
    fn outer_state_is_conservative_when_closure_is_missing_or_commit_is_unattested() {
        for process_closed in [false, true] {
            for final_reopened in [false, true] {
                for identity_matches in [false, true] {
                    for publisher_matches in [false, true] {
                        assert_eq!(
                            classify_outer_publication_state(
                                None,
                                process_closed,
                                final_reopened,
                                identity_matches,
                                publisher_matches,
                            ),
                            Some(P8OuterPublicationStateV1::CommittedUnattested)
                        );
                    }
                }
            }
        }
        assert_eq!(
            classify_outer_publication_state(
                Some(P8ParentPublicationStateV1::PreCommitFailed),
                false,
                false,
                false,
                true,
            ),
            Some(P8OuterPublicationStateV1::PreCommitFailed)
        );
        assert_eq!(
            classify_outer_publication_state(
                Some(P8ParentPublicationStateV1::CommittedUnattested),
                false,
                false,
                false,
                true,
            ),
            Some(P8OuterPublicationStateV1::CommittedUnattested)
        );
        assert_eq!(
            classify_outer_publication_state(
                Some(P8ParentPublicationStateV1::CommittedAwaitingOuterClosure),
                true,
                true,
                true,
                true,
            ),
            Some(P8OuterPublicationStateV1::AwaitingOpaquePublication)
        );
        for (closed, reopened, identity, publisher, expected) in [
            (true, false, true, true, None),
            (
                false,
                true,
                true,
                true,
                Some(P8OuterPublicationStateV1::CommittedUnattested),
            ),
            (
                true,
                true,
                false,
                true,
                Some(P8OuterPublicationStateV1::CommittedUnattested),
            ),
            (
                true,
                true,
                true,
                false,
                Some(P8OuterPublicationStateV1::CommittedUnattested),
            ),
        ] {
            assert_eq!(
                classify_outer_publication_state(
                    Some(P8ParentPublicationStateV1::CommittedAwaitingOuterClosure),
                    closed,
                    reopened,
                    identity,
                    publisher,
                ),
                expected
            );
        }
        assert_eq!(
            classify_outer_publication_state(
                Some(P8ParentPublicationStateV1::PreCommitFailed),
                true,
                true,
                true,
                true,
            ),
            None
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mutation_witness_is_permanently_invalid_after_first_observed_event() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let root = std::env::temp_dir().join(format!(
            "bm-p8-sticky-witness-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir(&root).expect("create fixture root");
        let file = root.join("artifact");
        std::fs::write(&file, b"before").expect("write fixture");
        let mut witness =
            P8ImmutableRootMutationWitness::establish(&root).expect("establish witness");
        std::fs::write(&file, b"after").expect("mutate fixture");
        assert!(witness.verify_quiet().is_err());
        std::fs::write(&file, b"before").expect("restore fixture bytes");
        assert!(witness.verify_quiet().is_err());
        std::fs::remove_file(file).expect("remove fixture file");
        std::fs::remove_dir(root).expect("remove fixture root");
    }
}
