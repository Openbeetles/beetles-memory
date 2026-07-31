//! Peer-bound SourcePublisher admission and parent-owned publication closure.

#[cfg(target_os = "linux")]
use std::{
    collections::BTreeMap,
    os::fd::AsRawFd as _,
    sync::{mpsc, Arc, OnceLock},
    thread,
    time::{Duration, Instant},
};
use std::{collections::BTreeSet, io, path::PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
use super::super::source_publisher::P8HarnessPublicationDraftV1;
use super::super::trusted_execution::supervisor_session::P8TrustedSupervisorSessionRef;
use super::super::{
    source_publisher::{prepared_harness_stage_binding, P8HarnessPublicationDraftRef},
    source_release::{P8HarnessExecutableRoleV1, P8HarnessReleaseManifestV1, P8HarnessReleaseRef},
    P8QualityContractFailure, P8QualityDigest,
};
#[cfg(target_os = "linux")]
use crate::{
    bounded_process::{
        supervise_spawned_child_closed, supervise_spawned_child_closed_before,
        BoundedProcessLimits, BoundedProcessTermination, ClosedBoundedProcess,
    },
    p8_quality::{
        source_publisher::{
            commit_prepared_harness_release_no_replace, prepare_harness_release_stage,
            verify_committed_harness_release, verify_harness_publication_draft,
            verify_staged_harness_release,
        },
        trusted_execution::{
            engineering_gate::VerifiedP8EngineeringGateSet,
            supervisor_session::{
                csprng_session_nonce, directory_identity_digest, P8TrustedSupervisorInputs,
            },
            P8QualityExecutionAuthority,
        },
    },
    p8_quality_process::p8_quality_execution_domain,
    retained_artifact_fs::RetainedArtifactDirectory,
    sealed_execution::{LinuxPeerBoundFdChannel, RetainedExecutable, SealedContentIdentity},
};

pub(crate) const P8_SOURCE_PUBLISHER_SESSION_ARG: &str = "--p8-source-publisher-session-v1";
const P8_SOURCE_PUBLISHER_CHANNEL_FD_ENV: &str = "BM_P8_SOURCE_PUBLISHER_CHANNEL_FD";
const P8_SOURCE_PUBLISHER_CHANNEL_DEADLINE_ENV: &str = "BM_P8_SOURCE_PUBLISHER_CHANNEL_DEADLINE_NS";
const HELLO_SCHEMA: &str = "beetle-memory.p8.source-publisher-hello.v1";
const PUBLICATION_INTENT_SCHEMA: &str = "beetle-memory.p8.harness-publication-intent.v1";
const PUBLICATION_INTENT_ACK_SCHEMA: &str = "beetle-memory.p8.harness-publication-intent-ack.v1";
const STAGE_READY_SCHEMA: &str = "beetle-memory.p8.source-publisher-stage-ready.v1";
const COMMIT_PERMIT_SCHEMA: &str = "beetle-memory.p8.source-publisher-commit-permit.v1";
const CLOSURE_DRAFT_SCHEMA: &str = "beetle-memory.p8.harness-publication-parent-closure-draft.v1";
const PUBLISHER_HELLO_FD_COUNT: usize = P8HarnessExecutableRoleV1::ALL.len() + 1;
const PUBLISHER_STAGE_READY_FD_COUNT: usize = 1;
const PUBLISHER_COMMIT_PERMIT_FD_COUNT: usize = 0;
const PUBLISHER_DRAFT_FD_COUNT: usize = 0;
#[cfg(target_os = "linux")]
const PUBLISHER_TEARDOWN_TIMEOUT: Duration = Duration::from_secs(2);

fn publisher_fd_count_is_exact(observed: usize, expected: usize) -> bool {
    observed == expected
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8HarnessPublicationIntentV1 {
    schema: String,
    release: P8HarnessReleaseManifestV1,
    supervisor_session_digest: P8TrustedSupervisorSessionRef,
    supervisor_plan_digest: P8QualityDigest,
    supervisor_descriptor_manifest_digest: P8QualityDigest,
    publisher_pid: u32,
    publisher_executable_digest: P8QualityDigest,
    publisher_plan_digest: P8QualityDigest,
    stage_name: String,
    channel_deadline_monotonic_nanos: u64,
    intent_digest: P8QualityDigest,
}

impl P8HarnessPublicationIntentV1 {
    fn from_hello(hello: &P8PublisherHelloV1) -> Self {
        let mut value = Self {
            schema: PUBLICATION_INTENT_SCHEMA.into(),
            release: hello.release.clone(),
            supervisor_session_digest: hello.supervisor_session_digest.clone(),
            supervisor_plan_digest: hello.supervisor_plan_digest.clone(),
            supervisor_descriptor_manifest_digest: hello
                .supervisor_descriptor_manifest_digest
                .clone(),
            publisher_pid: hello.expected_publisher_pid,
            publisher_executable_digest: hello
                .release
                .role_executable_digest(P8HarnessExecutableRoleV1::SourcePublisher)
                .expect("validated release has SourcePublisher")
                .clone(),
            publisher_plan_digest: hello.plan_digest.clone(),
            stage_name: hello.stage_name.clone(),
            channel_deadline_monotonic_nanos: hello.channel_deadline_monotonic_nanos,
            intent_digest: P8QualityDigest::derive("p8_harness_publication_intent_v1", &()),
        };
        value.intent_digest = value.derived_digest();
        value
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_harness_publication_intent_v1",
            &(
                &self.schema,
                &self.release,
                &self.supervisor_session_digest,
                &self.supervisor_plan_digest,
                &self.supervisor_descriptor_manifest_digest,
                self.publisher_pid,
                &self.publisher_executable_digest,
                &self.publisher_plan_digest,
                &self.stage_name,
                self.channel_deadline_monotonic_nanos,
            ),
        )
    }

    pub(crate) fn validate_contract(&self) -> bool {
        self.schema == PUBLICATION_INTENT_SCHEMA
            && self.release.validate_contract().is_empty()
            && self.publisher_pid != 0
            && self
                .release
                .role_executable_digest(P8HarnessExecutableRoleV1::SourcePublisher)
                == Some(&self.publisher_executable_digest)
            && valid_stage_name(
                &self.stage_name,
                self.release.content_address(),
                self.publisher_pid,
            )
            && self.channel_deadline_monotonic_nanos != 0
            && self.intent_digest == self.derived_digest()
    }

    pub(crate) fn release(&self) -> &P8HarnessReleaseManifestV1 {
        &self.release
    }

    pub(crate) fn supervisor_binding(
        &self,
    ) -> (
        &P8TrustedSupervisorSessionRef,
        &P8QualityDigest,
        &P8QualityDigest,
    ) {
        (
            &self.supervisor_session_digest,
            &self.supervisor_plan_digest,
            &self.supervisor_descriptor_manifest_digest,
        )
    }

    pub(crate) fn digest(&self) -> &P8QualityDigest {
        &self.intent_digest
    }

    pub(crate) fn stage_name(&self) -> &str {
        &self.stage_name
    }

    pub(crate) fn publisher_executable_digest(&self) -> &P8QualityDigest {
        &self.publisher_executable_digest
    }

    pub(crate) fn matches_closure(&self, closure: &P8HarnessPublicationClosureDraftV1) -> bool {
        let (session, plan, descriptors) = closure.supervisor_binding();
        closure.release() == &self.release
            && session == &self.supervisor_session_digest
            && plan == &self.supervisor_plan_digest
            && descriptors == &self.supervisor_descriptor_manifest_digest
            && closure.publisher_pid == self.publisher_pid
            && closure.publisher_executable_digest() == &self.publisher_executable_digest
            && closure.publisher_plan_digest == self.publisher_plan_digest
            && closure.stage_name == self.stage_name
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8HarnessPublicationIntentAckV1 {
    schema: String,
    intent_digest: P8QualityDigest,
    parent_pid: u32,
    supervisor_pid: u32,
    release_absent_observed: bool,
    channel_deadline_monotonic_nanos: u64,
    ack_digest: P8QualityDigest,
}

impl P8HarnessPublicationIntentAckV1 {
    pub(crate) fn new(
        intent: &P8HarnessPublicationIntentV1,
        parent_pid: u32,
        supervisor_pid: u32,
        channel_deadline_monotonic_nanos: u64,
    ) -> Self {
        let mut value = Self {
            schema: PUBLICATION_INTENT_ACK_SCHEMA.into(),
            intent_digest: intent.intent_digest.clone(),
            parent_pid,
            supervisor_pid,
            release_absent_observed: true,
            channel_deadline_monotonic_nanos,
            ack_digest: P8QualityDigest::derive("p8_harness_publication_intent_ack_v1", &()),
        };
        value.ack_digest = value.derived_digest();
        value
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_harness_publication_intent_ack_v1",
            &(
                &self.schema,
                &self.intent_digest,
                self.parent_pid,
                self.supervisor_pid,
                self.release_absent_observed,
                self.channel_deadline_monotonic_nanos,
            ),
        )
    }

    fn validate_against(
        &self,
        intent: &P8HarnessPublicationIntentV1,
        parent_pid: u32,
        supervisor_pid: u32,
        channel_deadline_monotonic_nanos: u64,
    ) -> bool {
        self.schema == PUBLICATION_INTENT_ACK_SCHEMA
            && self.intent_digest == intent.intent_digest
            && self.parent_pid == parent_pid
            && self.supervisor_pid == supervisor_pid
            && self.release_absent_observed
            && self.channel_deadline_monotonic_nanos == channel_deadline_monotonic_nanos
            && self.ack_digest == self.derived_digest()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8PublisherRoleCapabilityV1 {
    role: P8HarnessExecutableRoleV1,
    locator: PathBuf,
    executable_byte_len: u64,
    executable_digest: P8QualityDigest,
    physical_identity_digest: P8QualityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8PublisherHelloV1 {
    schema: String,
    nonce: Vec<u8>,
    parent_pid: u32,
    expected_publisher_pid: u32,
    releases_root_locator: PathBuf,
    releases_root_physical_identity: P8QualityDigest,
    release: P8HarnessReleaseManifestV1,
    roles: Vec<P8PublisherRoleCapabilityV1>,
    supervisor_session_digest: P8TrustedSupervisorSessionRef,
    supervisor_plan_digest: P8QualityDigest,
    supervisor_descriptor_manifest_digest: P8QualityDigest,
    stage_name: String,
    channel_deadline_monotonic_nanos: u64,
    plan_digest: P8QualityDigest,
}

impl P8PublisherHelloV1 {
    fn derived_plan_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_source_publisher_plan_v1",
            &(
                &self.nonce,
                self.parent_pid,
                self.expected_publisher_pid,
                &self.releases_root_locator,
                &self.releases_root_physical_identity,
                self.release.release_digest(),
                &self.roles,
                &self.supervisor_session_digest,
                &self.supervisor_plan_digest,
                &self.supervisor_descriptor_manifest_digest,
                &self.stage_name,
                self.channel_deadline_monotonic_nanos,
            ),
        )
    }

    fn validate_contract(&self) -> bool {
        self.schema == HELLO_SCHEMA
            && self.nonce.len() == 32
            && self.parent_pid != 0
            && self.expected_publisher_pid != 0
            && self.parent_pid != self.expected_publisher_pid
            && self.releases_root_locator.is_absolute()
            && self.channel_deadline_monotonic_nanos != 0
            && self.release.validate_contract().is_empty()
            && self.roles.len() == P8HarnessExecutableRoleV1::ALL.len()
            && valid_stage_name(
                &self.stage_name,
                self.release.content_address(),
                self.expected_publisher_pid,
            )
            && self
                .roles
                .iter()
                .map(|entry| entry.role)
                .eq(P8HarnessExecutableRoleV1::ALL)
            && self.roles.iter().all(|entry| {
                entry.locator.is_absolute()
                    && entry.executable_byte_len != 0
                    && self.release.role_executable_digest(entry.role)
                        == Some(&entry.executable_digest)
            })
            && self
                .roles
                .iter()
                .map(|entry| &entry.executable_digest)
                .collect::<BTreeSet<_>>()
                .len()
                == self.roles.len()
            && self
                .roles
                .iter()
                .map(|entry| &entry.physical_identity_digest)
                .collect::<BTreeSet<_>>()
                .len()
                == self.roles.len()
            && self.plan_digest == self.derived_plan_digest()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8PublisherStageReadyV1 {
    schema: String,
    plan_digest: P8QualityDigest,
    nonce_digest: P8QualityDigest,
    parent_pid: u32,
    publisher_pid: u32,
    release_digest: P8HarnessReleaseRef,
    releases_root_physical_identity: P8QualityDigest,
    exact_role_digests: Vec<(P8HarnessExecutableRoleV1, P8QualityDigest)>,
    stage_name: String,
    stage_directory_device: u64,
    stage_directory_inode: u64,
    stage_binding: P8QualityDigest,
    channel_deadline_monotonic_nanos: u64,
    stage_ready_digest: P8QualityDigest,
}

impl P8PublisherStageReadyV1 {
    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_source_publisher_stage_ready_v1",
            &(
                &self.schema,
                &self.plan_digest,
                &self.nonce_digest,
                self.parent_pid,
                self.publisher_pid,
                &self.release_digest,
                &self.releases_root_physical_identity,
                &self.exact_role_digests,
                &self.stage_name,
                self.stage_directory_device,
                self.stage_directory_inode,
                &self.stage_binding,
                self.channel_deadline_monotonic_nanos,
            ),
        )
    }

    fn validate_against(&self, hello: &P8PublisherHelloV1) -> bool {
        self.schema == STAGE_READY_SCHEMA
            && self.plan_digest == hello.plan_digest
            && self.nonce_digest
                == P8QualityDigest::derive("p8_source_publisher_nonce_v1", &hello.nonce)
            && self.parent_pid == hello.parent_pid
            && self.publisher_pid == hello.expected_publisher_pid
            && &self.release_digest == hello.release.release_digest()
            && self.releases_root_physical_identity == hello.releases_root_physical_identity
            && self.channel_deadline_monotonic_nanos == hello.channel_deadline_monotonic_nanos
            && self.stage_name == hello.stage_name
            && valid_stage_name(
                &self.stage_name,
                hello.release.content_address(),
                self.publisher_pid,
            )
            && self.exact_role_digests
                == hello
                    .roles
                    .iter()
                    .map(|entry| (entry.role, entry.executable_digest.clone()))
                    .collect::<Vec<_>>()
            && self.stage_binding
                == prepared_harness_stage_binding(
                    &hello.release,
                    self.stage_directory_device,
                    self.stage_directory_inode,
                )
            && self.stage_ready_digest == self.derived_digest()
    }
}

fn valid_stage_name(name: &str, address: &str, publisher_pid: u32) -> bool {
    let mut components = std::path::Path::new(name).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
        && name.starts_with(&format!(".p8-harness-{address}-{publisher_pid}-"))
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct P8PublisherCommitPermitMessageV1 {
    schema: String,
    plan_digest: P8QualityDigest,
    stage_ready_digest: P8QualityDigest,
    nonce_digest: P8QualityDigest,
    parent_pid: u32,
    publisher_pid: u32,
    release_binding: P8QualityDigest,
    stage_binding: P8QualityDigest,
    channel_deadline_monotonic_nanos: u64,
    permit_digest: P8QualityDigest,
}

impl P8PublisherCommitPermitMessageV1 {
    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_source_publisher_commit_permit_v1",
            &(
                &self.schema,
                &self.plan_digest,
                &self.stage_ready_digest,
                &self.nonce_digest,
                self.parent_pid,
                self.publisher_pid,
                &self.release_binding,
                &self.stage_binding,
                self.channel_deadline_monotonic_nanos,
            ),
        )
    }

    fn validate_against(
        &self,
        hello: &P8PublisherHelloV1,
        stage_ready: &P8PublisherStageReadyV1,
    ) -> bool {
        self.schema == COMMIT_PERMIT_SCHEMA
            && self.plan_digest == hello.plan_digest
            && self.stage_ready_digest == stage_ready.stage_ready_digest
            && self.nonce_digest == stage_ready.nonce_digest
            && self.parent_pid == hello.parent_pid
            && self.publisher_pid == hello.expected_publisher_pid
            && self.release_binding
                == P8QualityDigest::derive(
                    "p8_harness_publisher_commit_plan_binding_v1",
                    hello.release.release_digest(),
                )
            && self.stage_binding == stage_ready.stage_binding
            && self.channel_deadline_monotonic_nanos == hello.channel_deadline_monotonic_nanos
            && self.permit_digest == self.derived_digest()
    }
}

/// One-shot child authority minted only after the live peer-bound StageReady/permit exchange.
///
/// It deliberately implements neither Clone nor serde and cannot be reconstructed from a raw
/// receipt or caller-supplied digest.
pub(crate) struct P8AdmittedPublisherCommit {
    release_binding: P8QualityDigest,
    stage_binding: P8QualityDigest,
    deadline_monotonic_nanos: u64,
    _permit_digest: P8QualityDigest,
}

impl P8AdmittedPublisherCommit {
    pub(crate) fn release_binding(&self) -> &P8QualityDigest {
        &self.release_binding
    }

    pub(crate) fn stage_binding(&self) -> &P8QualityDigest {
        &self.stage_binding
    }

    pub(crate) fn verify_live(&self) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            crate::sealed_execution::remaining_until_monotonic_deadline(
                self.deadline_monotonic_nanos,
            )
            .map(|_| ())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "P8 publisher CommitPermit requires Linux monotonic authority",
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8ParentPublicationStateV1 {
    PreCommitFailed,
    CommittedAwaitingOuterClosure,
    CommittedUnattested,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8HarnessPublicationClosureDraftV1 {
    schema: String,
    state: P8ParentPublicationStateV1,
    release: P8HarnessReleaseManifestV1,
    supervisor_session_digest: P8TrustedSupervisorSessionRef,
    supervisor_plan_digest: P8QualityDigest,
    supervisor_descriptor_manifest_digest: P8QualityDigest,
    publisher_plan_digest: P8QualityDigest,
    stage_ready_digest: Option<P8QualityDigest>,
    commit_permit_digest: Option<P8QualityDigest>,
    stage_binding: Option<P8QualityDigest>,
    commit_permit_sent: bool,
    stage_name: String,
    stage_absent_observed: bool,
    draft_digest: Option<P8HarnessPublicationDraftRef>,
    publisher_executable_digest: P8QualityDigest,
    publisher_pid: u32,
    publisher_exit_code: Option<i32>,
    stdout_byte_len: u64,
    stdout_digest: P8QualityDigest,
    stdout_eof_observed: bool,
    stderr_byte_len: u64,
    stderr_digest: P8QualityDigest,
    stderr_eof_observed: bool,
    release_directory_identity: Option<(u64, u64)>,
    closure_draft_digest: P8QualityDigest,
}

impl P8HarnessPublicationClosureDraftV1 {
    pub(crate) fn state(&self) -> P8ParentPublicationStateV1 {
        self.state
    }

    pub(crate) fn release(&self) -> &P8HarnessReleaseManifestV1 {
        &self.release
    }

    pub(crate) fn release_directory_identity(&self) -> Option<(u64, u64)> {
        self.release_directory_identity
    }

    pub(crate) fn supervisor_binding(
        &self,
    ) -> (
        &P8TrustedSupervisorSessionRef,
        &P8QualityDigest,
        &P8QualityDigest,
    ) {
        (
            &self.supervisor_session_digest,
            &self.supervisor_plan_digest,
            &self.supervisor_descriptor_manifest_digest,
        )
    }

    pub(crate) fn publisher_executable_digest(&self) -> &P8QualityDigest {
        &self.publisher_executable_digest
    }

    fn has_complete_inner_publication_closure(&self) -> bool {
        self.commit_permit_sent
            && self.stage_ready_digest.is_some()
            && self.commit_permit_digest.is_some()
            && self.stage_binding.is_some()
            && self.draft_digest.is_some()
            && self
                .release
                .role_executable_digest(P8HarnessExecutableRoleV1::SourcePublisher)
                == Some(&self.publisher_executable_digest)
            && self.publisher_exit_code == Some(0)
            && self.stdout_byte_len == 0
            && self.stderr_byte_len == 0
            && self.stdout_digest
                == P8QualityDigest::derive(
                    "p8_source_publisher_closed_stdout_v1",
                    &Vec::<u8>::new(),
                )
            && self.stderr_digest
                == P8QualityDigest::derive(
                    "p8_source_publisher_closed_stderr_v1",
                    &Vec::<u8>::new(),
                )
            && self.stdout_eof_observed
            && self.stderr_eof_observed
            && self.release_directory_identity.is_some()
    }

    fn derived_digest(&self) -> P8QualityDigest {
        P8QualityDigest::derive(
            "p8_harness_publication_parent_closure_draft_v1",
            &(
                (
                    &self.schema,
                    &self.state,
                    &self.release,
                    &self.supervisor_session_digest,
                    &self.supervisor_plan_digest,
                    &self.supervisor_descriptor_manifest_digest,
                    &self.publisher_plan_digest,
                    &self.stage_ready_digest,
                    &self.commit_permit_digest,
                    &self.stage_binding,
                    self.commit_permit_sent,
                    &self.stage_name,
                    self.stage_absent_observed,
                ),
                (
                    &self.draft_digest,
                    &self.publisher_executable_digest,
                    self.publisher_pid,
                    self.publisher_exit_code,
                    self.stdout_byte_len,
                    &self.stdout_digest,
                    self.stdout_eof_observed,
                    self.stderr_byte_len,
                    &self.stderr_digest,
                    self.stderr_eof_observed,
                    self.release_directory_identity,
                ),
            ),
        )
    }

    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        failures.extend(self.release.validate_contract());
        if self.schema != CLOSURE_DRAFT_SCHEMA || self.publisher_pid == 0 {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if !valid_stage_name(
            &self.stage_name,
            self.release.content_address(),
            self.publisher_pid,
        ) {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        let complete = self.has_complete_inner_publication_closure();
        match self.state {
            P8ParentPublicationStateV1::PreCommitFailed => {
                if self.commit_permit_sent
                    || self.stage_ready_digest.is_some()
                    || self.commit_permit_digest.is_some()
                    || self.stage_binding.is_some()
                    || self.draft_digest.is_some()
                    || self.release_directory_identity.is_some()
                    || !self.stage_absent_observed
                {
                    failures.push(P8QualityContractFailure::TrustedExecutionMissing);
                }
            }
            P8ParentPublicationStateV1::CommittedAwaitingOuterClosure => {
                if !complete || self.stage_absent_observed {
                    failures.push(P8QualityContractFailure::PipeClosureMissing);
                }
            }
            P8ParentPublicationStateV1::CommittedUnattested => {
                if !self.commit_permit_sent
                    || self.stage_ready_digest.is_none()
                    || self.commit_permit_digest.is_none()
                    || self.stage_binding.is_none()
                    || complete
                    || self.stage_absent_observed
                {
                    failures.push(P8QualityContractFailure::TrustedExecutionMissing);
                }
            }
        }
        if self.closure_draft_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn publish_verified_gate_set(
    verified: VerifiedP8EngineeringGateSet,
) -> io::Result<P8HarnessPublicationClosureDraftV1> {
    let (release, mut inputs) = super::assemble_verified_gate_set_release(verified)
        .map_err(|_| invalid_data("P8 verified gate-set could not form a harness release"))?;
    inputs.execution.verify()?;
    inputs.verify_source_exact_and_quiet()?;
    inputs.verify_dependency_roots_exact_and_quiet()?;
    inputs.releases_root.verify_unchanged()?;
    let supervisor_pid = inputs.admitted_session.audit_binding().4;
    // SAFETY: getpgrp has no failure mode. TrustedSupervisor itself was admitted through the
    // outer sealed barrier as process-group leader; SourcePublisher must join that exact domain.
    let supervisor_process_group = u32::try_from(unsafe { libc::getpgrp() })
        .map_err(|_| invalid_data("P8 TrustedSupervisor process group is invalid"))?;
    if supervisor_pid != std::process::id() || supervisor_process_group != supervisor_pid {
        return Err(invalid_data(
            "P8 TrustedSupervisor does not own its outer process domain",
        ));
    }

    let releases_root_identity = directory_identity_digest(&inputs.releases_root)?;
    let mut descriptor_files = vec![inputs.releases_root.try_clone_directory_file()?];
    let mut roles = Vec::with_capacity(P8HarnessExecutableRoleV1::ALL.len());
    for role in P8HarnessExecutableRoleV1::ALL {
        let retained = inputs
            .role_executables
            .get_mut(&role)
            .ok_or_else(|| invalid_data("P8 publisher role capability is missing"))?;
        let (device, inode) = retained.unix_physical_identity()?;
        let physical_identity_digest = publisher_role_physical_identity(device, inode);
        let observed = retained.copy_to_verified(&mut io::sink())?;
        let executable_digest = P8QualityDigest::parse(format!("sha256:{}", observed.sha256()))
            .map_err(|_| invalid_data("P8 publisher role digest is invalid"))?;
        if release.role_executable_digest(role) != Some(&executable_digest) {
            return Err(invalid_data(
                "P8 publisher role bytes differ from the verified release",
            ));
        }
        descriptor_files.push(retained.try_clone_file()?);
        roles.push(P8PublisherRoleCapabilityV1 {
            role,
            locator: retained.locator().to_path_buf(),
            executable_byte_len: observed.byte_len(),
            executable_digest,
            physical_identity_digest,
        });
    }
    if !publisher_fd_count_is_exact(descriptor_files.len(), PUBLISHER_HELLO_FD_COUNT) {
        return Err(invalid_data(
            "P8 SourcePublisher Hello descriptor set is not exact",
        ));
    }

    let (parent_channel, child_channel) =
        LinuxPeerBoundFdChannel::pair_with_deadline_monotonic_nanos(
            inputs.parent_channel.deadline_monotonic_nanos(),
        )?;
    let child_channel_file = child_channel.inheritable_duplicate()?;
    let child_channel_fd = child_channel_file.as_raw_fd();
    let nonce = csprng_session_nonce()?;
    let attempt_nonce = u64::from_le_bytes(
        nonce[..8]
            .try_into()
            .expect("32-byte nonce contains an eight-byte prefix"),
    );
    let publisher = inputs
        .role_executables
        .get_mut(&P8HarnessExecutableRoleV1::SourcePublisher)
        .ok_or_else(|| invalid_data("P8 SourcePublisher executable is missing"))?;
    let prepared = publisher
        .prepare_with_linux_pre_exec_barrier_joining_process_group_and_environment(
            p8_quality_execution_domain(),
            &[P8_SOURCE_PUBLISHER_SESSION_ARG.to_string()],
            attempt_nonce,
            supervisor_process_group,
            &[
                (
                    P8_SOURCE_PUBLISHER_CHANNEL_FD_ENV.to_string(),
                    child_channel_fd.to_string(),
                ),
                (
                    P8_SOURCE_PUBLISHER_CHANNEL_DEADLINE_ENV.to_string(),
                    child_channel.deadline_monotonic_nanos().to_string(),
                ),
            ],
            vec![child_channel_file],
        )?;
    let expected_publisher_digest = release
        .role_executable_digest(P8HarnessExecutableRoleV1::SourcePublisher)
        .ok_or_else(|| invalid_data("P8 SourcePublisher release digest is missing"))?
        .clone();
    let started = Instant::now();
    let (mut broker, launch) = prepared.into_broker_and_launch();
    let (sender, receiver) = mpsc::sync_channel(1);
    let cleanup_limits = BoundedProcessLimits {
        stdout_bytes: inputs.stdout_bytes,
        stderr_bytes: inputs.stderr_bytes,
        total_bytes: inputs.total_bytes,
        timeout: PUBLISHER_TEARDOWN_TIMEOUT,
    };
    let teardown_deadline = Arc::new(OnceLock::new());
    let launch_thread_teardown_deadline = Arc::clone(&teardown_deadline);
    let launch_thread = thread::spawn(move || {
        if let Err(mpsc::SendError(Ok(spawned))) = sender.send(launch.spawn_piped()) {
            let deadline = claim_publisher_teardown_deadline(&launch_thread_teardown_deadline);
            cleanup_spawned_publisher_or_abort(spawned, cleanup_limits, deadline);
        }
    });
    let barrier_timeout = match parent_channel.remaining_time() {
        Ok(timeout) => timeout,
        Err(_) => {
            drop(broker);
            abort_publisher_launch_worker(
                receiver,
                launch_thread,
                cleanup_limits,
                &teardown_deadline,
            );
        }
    };
    let publisher_pid = match broker.wait_ready(barrier_timeout) {
        Ok(pid) => pid,
        Err(_) => {
            drop(broker);
            abort_publisher_launch_worker(
                receiver,
                launch_thread,
                cleanup_limits,
                &teardown_deadline,
            );
        }
    };
    if broker.release(publisher_pid).is_err() {
        abort_publisher_launch_worker(receiver, launch_thread, cleanup_limits, &teardown_deadline);
    }
    let spawned = match receiver.recv_timeout(
        parent_channel
            .remaining_time()
            .unwrap_or_else(|_| std::time::Duration::from_millis(1)),
    ) {
        Ok(Ok(spawned)) => spawned,
        Ok(Err(_)) => {
            let deadline = claim_publisher_teardown_deadline(&teardown_deadline);
            if join_publisher_launch_worker_before(launch_thread, deadline).is_err() {
                std::process::abort();
            }
            std::process::abort();
        }
        Err(_) => {
            abort_publisher_launch_worker(
                receiver,
                launch_thread,
                cleanup_limits,
                &teardown_deadline,
            );
        }
    };
    let deadline = claim_publisher_teardown_deadline(&teardown_deadline);
    if join_publisher_launch_worker_before(launch_thread, deadline).is_err() {
        cleanup_spawned_publisher_or_abort(spawned, cleanup_limits, deadline);
        std::process::abort();
    }
    if spawned.child.id() != publisher_pid {
        cleanup_spawned_publisher_or_abort(spawned, cleanup_limits, deadline);
        std::process::abort();
    }
    let (publisher_child, guard, publisher_identity) = spawned.into_parts();
    drop(guard);
    drop(child_channel);
    let mut publisher_child = Some(publisher_child);

    let (session_ref, _, supervisor_plan, descriptor_manifest, observed_supervisor_pid) =
        inputs.admitted_session.audit_binding();
    if observed_supervisor_pid != supervisor_pid {
        drop(parent_channel);
        settle_publisher_or_abort(
            publisher_child
                .take()
                .expect("publisher child retained for supervisor binding"),
            &inputs,
            started,
        );
        std::process::abort();
    }
    let stage_name = format!(
        ".p8-harness-{}-{}-{}",
        release.content_address(),
        publisher_pid,
        attempt_nonce
    );
    let mut hello = P8PublisherHelloV1 {
        schema: HELLO_SCHEMA.into(),
        nonce: nonce.to_vec(),
        parent_pid: std::process::id(),
        expected_publisher_pid: publisher_pid,
        releases_root_locator: inputs.releases_root.path().to_path_buf(),
        releases_root_physical_identity: releases_root_identity,
        release: release.clone(),
        roles,
        supervisor_session_digest: session_ref.clone(),
        supervisor_plan_digest: supervisor_plan.clone(),
        supervisor_descriptor_manifest_digest: descriptor_manifest.clone(),
        stage_name,
        channel_deadline_monotonic_nanos: parent_channel.deadline_monotonic_nanos(),
        plan_digest: P8QualityDigest::derive("p8_source_publisher_plan_v1", &()),
    };
    hello.plan_digest = hello.derived_plan_digest();
    if !hello.validate_contract()
        || &hello.supervisor_plan_digest != supervisor_plan
        || &hello.supervisor_descriptor_manifest_digest != descriptor_manifest
    {
        drop(parent_channel);
        settle_publisher_or_abort(
            publisher_child
                .take()
                .expect("publisher child retained for Hello validation"),
            &inputs,
            started,
        );
        std::process::abort();
    }
    let intent = P8HarnessPublicationIntentV1::from_hello(&hello);
    let intent_bytes = match serde_json::to_vec(&intent) {
        Ok(bytes) => bytes,
        Err(_) => {
            drop(parent_channel);
            settle_publisher_or_abort(
                publisher_child
                    .take()
                    .expect("publisher child retained for intent serialization"),
                &inputs,
                started,
            );
            std::process::abort();
        }
    };
    if inputs
        .parent_channel
        .send_with_files(&intent_bytes, &[])
        .is_err()
    {
        drop(parent_channel);
        settle_publisher_or_abort(
            publisher_child
                .take()
                .expect("publisher child retained for outer intent send"),
            &inputs,
            started,
        );
        std::process::abort();
    }
    let outer_parent_pid = inputs.outer_parent_pid();
    let intent_ack = inputs
        .parent_channel
        .receive_with_files(outer_parent_pid, 0)
        .ok()
        .and_then(|(bytes, files)| {
            files
                .is_empty()
                .then(|| serde_json::from_slice::<P8HarnessPublicationIntentAckV1>(&bytes).ok())
                .flatten()
        });
    if intent_ack.as_ref().is_none_or(|ack| {
        !ack.validate_against(
            &intent,
            outer_parent_pid,
            std::process::id(),
            inputs.parent_channel.deadline_monotonic_nanos(),
        )
    }) {
        return close_precommit_failure(
            publisher_child
                .take()
                .expect("publisher child retained for outer intent acknowledgement"),
            parent_channel,
            &inputs,
            started,
            &hello,
            &publisher_identity,
        );
    }
    if inputs
        .releases_root
        .verify_subdirectory_absent(release.content_address())
        .is_err()
        || inputs
            .releases_root
            .verify_subdirectory_absent(&hello.stage_name)
            .is_err()
    {
        return close_precommit_failure(
            publisher_child
                .take()
                .expect("publisher child retained for pre-Hello absence verification"),
            parent_channel,
            &inputs,
            started,
            &hello,
            &publisher_identity,
        );
    }
    let hello_bytes = match serde_json::to_vec(&hello) {
        Ok(bytes) => bytes,
        Err(_) => {
            return close_precommit_failure(
                publisher_child
                    .take()
                    .expect("publisher child retained before hello"),
                parent_channel,
                &inputs,
                started,
                &hello,
                &publisher_identity,
            );
        }
    };
    let descriptor_refs = descriptor_files.iter().collect::<Vec<_>>();
    if parent_channel
        .send_with_files(&hello_bytes, &descriptor_refs)
        .is_err()
    {
        return close_precommit_failure(
            publisher_child
                .take()
                .expect("publisher child retained for hello send"),
            parent_channel,
            &inputs,
            started,
            &hello,
            &publisher_identity,
        );
    }
    let (stage_ready_bytes, stage_files) =
        match parent_channel.receive_with_files(publisher_pid, PUBLISHER_STAGE_READY_FD_COUNT) {
            Ok(value) => value,
            Err(_) => {
                return close_precommit_failure(
                    publisher_child
                        .take()
                        .expect("publisher child retained before permit"),
                    parent_channel,
                    &inputs,
                    started,
                    &hello,
                    &publisher_identity,
                );
            }
        };
    let permit = (|| -> io::Result<(P8PublisherStageReadyV1, P8PublisherCommitPermitMessageV1)> {
        let stage_ready: P8PublisherStageReadyV1 = serde_json::from_slice(&stage_ready_bytes)
            .map_err(|_| invalid_data("P8 SourcePublisher StageReady is invalid"))?;
        if !stage_ready.validate_against(&hello)
            || !publisher_fd_count_is_exact(stage_files.len(), PUBLISHER_STAGE_READY_FD_COUNT)
        {
            return Err(invalid_data(
                "P8 SourcePublisher StageReady differs from parent plan",
            ));
        }
        let stage_path = inputs.releases_root.path().join(&stage_ready.stage_name);
        let stage = RetainedArtifactDirectory::from_retained_directory_file(
            &stage_path,
            stage_files
                .into_iter()
                .next()
                .expect("exact StageReady FD count"),
        )?;
        let (stage_device, stage_inode) = verify_staged_harness_release(&stage, &release)?;
        if (stage_device, stage_inode)
            != (
                stage_ready.stage_directory_device,
                stage_ready.stage_directory_inode,
            )
            || prepared_harness_stage_binding(&release, stage_device, stage_inode)
                != stage_ready.stage_binding
        {
            return Err(invalid_data(
                "P8 SourcePublisher prepared stage failed parent verification",
            ));
        }
        inputs.verify_source_exact_and_quiet()?;
        inputs.verify_dependency_roots_exact_and_quiet()?;
        inputs.releases_root.verify_unchanged()?;
        let release_binding = P8QualityDigest::derive(
            "p8_harness_publisher_commit_plan_binding_v1",
            release.release_digest(),
        );
        let mut permit = P8PublisherCommitPermitMessageV1 {
            schema: COMMIT_PERMIT_SCHEMA.into(),
            plan_digest: hello.plan_digest.clone(),
            stage_ready_digest: stage_ready.stage_ready_digest.clone(),
            nonce_digest: stage_ready.nonce_digest.clone(),
            parent_pid: std::process::id(),
            publisher_pid,
            release_binding,
            stage_binding: stage_ready.stage_binding.clone(),
            channel_deadline_monotonic_nanos: parent_channel.deadline_monotonic_nanos(),
            permit_digest: P8QualityDigest::derive("p8_source_publisher_commit_permit_v1", &()),
        };
        permit.permit_digest = permit.derived_digest();
        Ok((stage_ready, permit))
    })();
    let (stage_ready, permit) = match permit {
        Ok(value) => value,
        Err(_) => {
            return close_precommit_failure(
                publisher_child
                    .take()
                    .expect("publisher child retained before CommitPermit"),
                parent_channel,
                &inputs,
                started,
                &hello,
                &publisher_identity,
            );
        }
    };
    let permit_bytes = match serde_json::to_vec(&permit) {
        Ok(bytes) => bytes,
        Err(_) => {
            return close_precommit_failure(
                publisher_child
                    .take()
                    .expect("publisher child retained before CommitPermit serialization"),
                parent_channel,
                &inputs,
                started,
                &hello,
                &publisher_identity,
            );
        }
    };
    let permit_files: [&std::fs::File; PUBLISHER_COMMIT_PERMIT_FD_COUNT] = [];
    if parent_channel
        .send_with_files(&permit_bytes, &permit_files)
        .is_err()
    {
        return close_precommit_failure(
            publisher_child
                .take()
                .expect("publisher child retained for CommitPermit send"),
            parent_channel,
            &inputs,
            started,
            &hello,
            &publisher_identity,
        );
    }
    let draft_result = parent_channel.receive_with_files(publisher_pid, PUBLISHER_DRAFT_FD_COUNT);
    drop(parent_channel);
    let output = settle_publisher_or_abort(
        publisher_child
            .take()
            .expect("publisher child retained through parent closure"),
        &inputs,
        started,
    );
    let publisher_digest = observed_sealed_identity_digest(&publisher_identity);
    let publisher_identity_verified = publisher_digest
        .as_ref()
        .is_ok_and(|actual| actual == &expected_publisher_digest)
        && inputs
            .role_executables
            .get_mut(&P8HarnessExecutableRoleV1::SourcePublisher)
            .is_some_and(|publisher| publisher.verify_content(&publisher_identity).is_ok());
    let publisher_digest = publisher_digest.unwrap_or_else(|_| expected_publisher_digest.clone());
    let draft = match draft_result {
        Ok((draft_bytes, draft_files))
            if publisher_fd_count_is_exact(draft_files.len(), PUBLISHER_DRAFT_FD_COUNT) =>
        {
            serde_json::from_slice::<P8HarnessPublicationDraftV1>(&draft_bytes).ok()
        }
        _ => None,
    };
    let verified_draft = draft.filter(|draft| {
        verify_harness_publication_draft(&inputs.releases_root, &release, draft).is_ok()
    });
    let final_identity = verify_committed_harness_release(&inputs.releases_root, &release).ok();
    let complete = output.termination() == BoundedProcessTermination::Exited
        && output.status().success()
        && output.stdout().is_empty()
        && output.stderr().is_empty()
        && output.stdout_eof_observed()
        && output.stderr_eof_observed()
        && publisher_identity_verified
        && verified_draft
            .as_ref()
            .zip(final_identity)
            .is_some_and(|(draft, identity)| draft.release_directory_identity() == identity);
    let state = if complete {
        P8ParentPublicationStateV1::CommittedAwaitingOuterClosure
    } else {
        P8ParentPublicationStateV1::CommittedUnattested
    };
    let closure = build_closure_draft(P8PublisherClosureObservation {
        state,
        hello: &hello,
        stage_ready: Some(&stage_ready),
        permit: Some(&permit),
        draft_digest: verified_draft
            .as_ref()
            .map(|draft| draft.draft_digest().clone()),
        publisher_executable_digest: publisher_digest,
        output: &output,
        release_directory_identity: final_identity,
        stage_absent_observed: false,
    });
    send_outer_closure(&inputs, &closure)?;
    Ok(closure)
}

#[cfg(target_os = "linux")]
fn settle_publisher_or_abort(
    child: std::process::Child,
    inputs: &P8TrustedSupervisorInputs,
    started: Instant,
) -> ClosedBoundedProcess {
    let timeout = inputs
        .parent_channel
        .remaining_time()
        .unwrap_or_else(|_| std::time::Duration::from_millis(1));
    supervise_spawned_child_closed(
        child,
        BoundedProcessLimits {
            stdout_bytes: inputs.stdout_bytes,
            stderr_bytes: inputs.stderr_bytes,
            total_bytes: inputs.total_bytes,
            timeout,
        },
        started,
    )
    .unwrap_or_else(|_| std::process::abort())
}

#[cfg(target_os = "linux")]
fn cleanup_spawned_publisher_or_abort(
    spawned: crate::sealed_execution::SpawnedLinuxBarrierSealedExecutable,
    mut limits: BoundedProcessLimits,
    deadline: Instant,
) {
    let (child, guard, _) = spawned.into_parts();
    drop(guard);
    let started = Instant::now();
    limits.timeout = deadline.saturating_duration_since(started);
    if limits.timeout.is_zero()
        || supervise_spawned_child_closed_before(child, limits, started, deadline).is_err()
    {
        std::process::abort();
    }
}

#[cfg(target_os = "linux")]
fn claim_publisher_teardown_deadline(owner: &OnceLock<Instant>) -> Instant {
    *owner.get_or_init(|| {
        Instant::now()
            .checked_add(PUBLISHER_TEARDOWN_TIMEOUT)
            .unwrap_or_else(|| std::process::abort())
    })
}

#[cfg(target_os = "linux")]
fn join_publisher_launch_worker_before(
    launch_thread: thread::JoinHandle<()>,
    deadline: Instant,
) -> Result<(), ()> {
    while !launch_thread.is_finished() {
        if Instant::now() >= deadline {
            return Err(());
        }
        thread::sleep(Duration::from_millis(1));
    }
    launch_thread.join().map_err(|_| ())
}

#[cfg(target_os = "linux")]
fn abort_publisher_launch_worker(
    receiver: mpsc::Receiver<
        io::Result<crate::sealed_execution::SpawnedLinuxBarrierSealedExecutable>,
    >,
    launch_thread: thread::JoinHandle<()>,
    cleanup_limits: BoundedProcessLimits,
    teardown_deadline: &OnceLock<Instant>,
) -> ! {
    let deadline = claim_publisher_teardown_deadline(teardown_deadline);
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        std::process::abort();
    }
    match receiver.recv_timeout(remaining) {
        Ok(Ok(spawned)) => cleanup_spawned_publisher_or_abort(spawned, cleanup_limits, deadline),
        Ok(Err(_)) => {}
        Err(_) => std::process::abort(),
    }
    if join_publisher_launch_worker_before(launch_thread, deadline).is_err() {
        std::process::abort();
    }
    std::process::abort()
}

#[cfg(target_os = "linux")]
fn observed_sealed_identity_digest(
    identity: &SealedContentIdentity,
) -> io::Result<P8QualityDigest> {
    P8QualityDigest::parse(format!("sha256:{}", identity.sha256()))
        .map_err(|_| invalid_data("P8 SourcePublisher sealed identity is invalid"))
}

#[cfg(target_os = "linux")]
fn close_precommit_failure(
    child: std::process::Child,
    parent_channel: LinuxPeerBoundFdChannel,
    inputs: &P8TrustedSupervisorInputs,
    started: Instant,
    hello: &P8PublisherHelloV1,
    publisher_identity: &SealedContentIdentity,
) -> io::Result<P8HarnessPublicationClosureDraftV1> {
    drop(parent_channel);
    let output = settle_publisher_or_abort(child, inputs, started);
    if inputs
        .releases_root
        .verify_subdirectory_absent(&hello.stage_name)
        .is_err()
    {
        // Pre-commit cleanup is part of the state transition. Unknown residue must not return.
        std::process::abort();
    }
    let observed = observed_sealed_identity_digest(publisher_identity)
        .unwrap_or_else(|_| std::process::abort());
    let closure = build_closure_draft(P8PublisherClosureObservation {
        state: P8ParentPublicationStateV1::PreCommitFailed,
        hello,
        stage_ready: None,
        permit: None,
        draft_digest: None,
        publisher_executable_digest: observed,
        output: &output,
        release_directory_identity: None,
        stage_absent_observed: true,
    });
    send_outer_closure(inputs, &closure)?;
    Ok(closure)
}

#[cfg(target_os = "linux")]
fn publisher_role_physical_identity(device: u64, inode: u64) -> P8QualityDigest {
    P8QualityDigest::derive(
        "p8_source_publisher_role_physical_identity_v1",
        &(device, inode),
    )
}

#[cfg(target_os = "linux")]
struct P8PublisherClosureObservation<'a> {
    state: P8ParentPublicationStateV1,
    hello: &'a P8PublisherHelloV1,
    stage_ready: Option<&'a P8PublisherStageReadyV1>,
    permit: Option<&'a P8PublisherCommitPermitMessageV1>,
    draft_digest: Option<P8HarnessPublicationDraftRef>,
    publisher_executable_digest: P8QualityDigest,
    output: &'a ClosedBoundedProcess,
    release_directory_identity: Option<(u64, u64)>,
    stage_absent_observed: bool,
}

#[cfg(target_os = "linux")]
fn build_closure_draft(
    observation: P8PublisherClosureObservation<'_>,
) -> P8HarnessPublicationClosureDraftV1 {
    let P8PublisherClosureObservation {
        state,
        hello,
        stage_ready,
        permit,
        draft_digest,
        publisher_executable_digest,
        output,
        release_directory_identity,
        stage_absent_observed,
    } = observation;
    let mut draft = P8HarnessPublicationClosureDraftV1 {
        schema: CLOSURE_DRAFT_SCHEMA.into(),
        state,
        release: hello.release.clone(),
        supervisor_session_digest: hello.supervisor_session_digest.clone(),
        supervisor_plan_digest: hello.supervisor_plan_digest.clone(),
        supervisor_descriptor_manifest_digest: hello.supervisor_descriptor_manifest_digest.clone(),
        publisher_plan_digest: hello.plan_digest.clone(),
        stage_ready_digest: stage_ready.map(|ready| ready.stage_ready_digest.clone()),
        commit_permit_digest: permit.map(|permit| permit.permit_digest.clone()),
        stage_binding: stage_ready.map(|ready| ready.stage_binding.clone()),
        commit_permit_sent: permit.is_some(),
        stage_name: hello.stage_name.clone(),
        stage_absent_observed,
        draft_digest,
        publisher_executable_digest,
        publisher_pid: hello.expected_publisher_pid,
        publisher_exit_code: output.status().code(),
        stdout_byte_len: u64::try_from(output.stdout().len()).unwrap_or(u64::MAX),
        stdout_digest: P8QualityDigest::derive(
            "p8_source_publisher_closed_stdout_v1",
            &output.stdout(),
        ),
        stdout_eof_observed: output.stdout_eof_observed(),
        stderr_byte_len: u64::try_from(output.stderr().len()).unwrap_or(u64::MAX),
        stderr_digest: P8QualityDigest::derive(
            "p8_source_publisher_closed_stderr_v1",
            &output.stderr(),
        ),
        stderr_eof_observed: output.stderr_eof_observed(),
        release_directory_identity,
        closure_draft_digest: P8QualityDigest::derive(
            "p8_harness_publication_parent_closure_draft_v1",
            &(),
        ),
    };
    draft.closure_draft_digest = draft.derived_digest();
    if !draft.validate_contract().is_empty() {
        std::process::abort();
    }
    draft
}

#[cfg(target_os = "linux")]
fn send_outer_closure(
    inputs: &P8TrustedSupervisorInputs,
    closure: &P8HarnessPublicationClosureDraftV1,
) -> io::Result<()> {
    inputs.parent_channel.send_with_files(
        &serde_json::to_vec(closure)
            .map_err(|_| invalid_data("P8 publication closure draft serialization failed"))?,
        &[],
    )
}

#[cfg(target_os = "linux")]
pub(crate) fn run_source_publisher_session_entry() -> io::Result<()> {
    let mut authority = P8QualityExecutionAuthority::claim_source_publisher()?;
    authority.verify()?;
    let channel = LinuxPeerBoundFdChannel::claim_inherited(
        P8_SOURCE_PUBLISHER_CHANNEL_FD_ENV,
        P8_SOURCE_PUBLISHER_CHANNEL_DEADLINE_ENV,
    )?;
    let parent_pid = u32::try_from(unsafe { libc::getppid() })
        .ok()
        .filter(|pid| *pid != 0)
        .ok_or_else(|| invalid_data("P8 SourcePublisher parent PID is invalid"))?;
    let (hello_bytes, files) = channel.receive_with_files(parent_pid, PUBLISHER_HELLO_FD_COUNT)?;
    if !publisher_fd_count_is_exact(files.len(), PUBLISHER_HELLO_FD_COUNT) {
        return Err(invalid_data(
            "P8 SourcePublisher Hello descriptor set is not exact",
        ));
    }
    let hello: P8PublisherHelloV1 = serde_json::from_slice(&hello_bytes)
        .map_err(|_| invalid_data("P8 SourcePublisher hello is invalid"))?;
    // SAFETY: getpgrp has no failure mode. The child barrier joined the sealed parent's process
    // group before readiness; the child independently confirms that containment after fexecve.
    let process_group = u32::try_from(unsafe { libc::getpgrp() })
        .map_err(|_| invalid_data("P8 SourcePublisher process group is invalid"))?;
    if !hello.validate_contract()
        || !hello.release.has_exact_engineering_sealed_processes()
        || hello.parent_pid != parent_pid
        || hello.expected_publisher_pid != std::process::id()
        || process_group != parent_pid
    {
        return Err(invalid_data(
            "P8 SourcePublisher hello differs from live peer observations",
        ));
    }
    let mut files = files.into_iter();
    let root_file = files
        .next()
        .ok_or_else(|| invalid_data("P8 SourcePublisher releases-root FD is missing"))?;
    let root = RetainedArtifactDirectory::from_retained_directory_file(
        &hello.releases_root_locator,
        root_file,
    )?;
    if directory_identity_digest(&root)? != hello.releases_root_physical_identity {
        return Err(invalid_data(
            "P8 SourcePublisher releases-root identity drifted",
        ));
    }
    let mut roles = BTreeMap::new();
    for expected in &hello.roles {
        let file = files
            .next()
            .ok_or_else(|| invalid_data("P8 SourcePublisher role FD is missing"))?;
        let mut executable = RetainedExecutable::from_retained_file(
            &expected.locator,
            file,
            expected.executable_byte_len,
        )?;
        let (device, inode) = executable.unix_physical_identity()?;
        let physical_identity = publisher_role_physical_identity(device, inode);
        let observed = executable.copy_to_verified(&mut io::sink())?;
        let digest = P8QualityDigest::parse(format!("sha256:{}", observed.sha256()))
            .map_err(|_| invalid_data("P8 SourcePublisher role digest is invalid"))?;
        if physical_identity != expected.physical_identity_digest
            || digest != expected.executable_digest
            || roles.insert(expected.role, executable).is_some()
        {
            return Err(invalid_data(
                "P8 SourcePublisher role capability set is not exact",
            ));
        }
    }
    if files.next().is_some()
        || roles.len() != P8HarnessExecutableRoleV1::ALL.len()
        || authority.executable_digest()
            != *hello
                .release
                .role_executable_digest(P8HarnessExecutableRoleV1::SourcePublisher)
                .ok_or_else(|| invalid_data("P8 SourcePublisher release role is missing"))?
    {
        return Err(invalid_data(
            "P8 SourcePublisher retained capabilities are not exact",
        ));
    }
    authority.verify()?;
    let prepared = prepare_harness_release_stage(
        &mut authority,
        &root,
        &hello.release,
        roles,
        hello.stage_name.clone(),
    )?;
    let (stage_directory_device, stage_directory_inode) = prepared.physical_identity()?;
    let stage_binding = prepared_harness_stage_binding(
        &hello.release,
        stage_directory_device,
        stage_directory_inode,
    );
    let stage_file = prepared.stage_directory_file()?;
    let mut stage_ready = P8PublisherStageReadyV1 {
        schema: STAGE_READY_SCHEMA.into(),
        plan_digest: hello.plan_digest.clone(),
        nonce_digest: P8QualityDigest::derive("p8_source_publisher_nonce_v1", &hello.nonce),
        parent_pid,
        publisher_pid: std::process::id(),
        release_digest: hello.release.release_digest().clone(),
        releases_root_physical_identity: hello.releases_root_physical_identity.clone(),
        exact_role_digests: hello
            .roles
            .iter()
            .map(|entry| (entry.role, entry.executable_digest.clone()))
            .collect(),
        stage_name: prepared.stage_name().to_string(),
        stage_directory_device,
        stage_directory_inode,
        stage_binding,
        channel_deadline_monotonic_nanos: channel.deadline_monotonic_nanos(),
        stage_ready_digest: P8QualityDigest::derive("p8_source_publisher_stage_ready_v1", &()),
    };
    stage_ready.stage_ready_digest = stage_ready.derived_digest();
    channel.send_with_files(
        &serde_json::to_vec(&stage_ready)
            .map_err(|_| invalid_data("P8 SourcePublisher StageReady serialization failed"))?,
        &[&stage_file],
    )?;
    let (permit_bytes, permit_files) =
        channel.receive_with_files(parent_pid, PUBLISHER_COMMIT_PERMIT_FD_COUNT)?;
    if !publisher_fd_count_is_exact(permit_files.len(), PUBLISHER_COMMIT_PERMIT_FD_COUNT) {
        return Err(invalid_data(
            "P8 SourcePublisher CommitPermit carried unexpected FDs",
        ));
    }
    let permit_message: P8PublisherCommitPermitMessageV1 = serde_json::from_slice(&permit_bytes)
        .map_err(|_| invalid_data("P8 SourcePublisher CommitPermit is invalid"))?;
    if !permit_message.validate_against(&hello, &stage_ready) {
        return Err(invalid_data(
            "P8 SourcePublisher CommitPermit differs from the live admitted stage",
        ));
    }
    let permit = P8AdmittedPublisherCommit {
        release_binding: permit_message.release_binding,
        stage_binding: permit_message.stage_binding,
        deadline_monotonic_nanos: permit_message.channel_deadline_monotonic_nanos,
        _permit_digest: permit_message.permit_digest,
    };
    let draft = commit_prepared_harness_release_no_replace(permit, &mut authority, prepared)?;
    channel.send_with_files(
        &serde_json::to_vec(&draft)
            .map_err(|_| invalid_data("P8 SourcePublisher draft serialization failed"))?,
        &[],
    )?;
    authority.verify()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn run_source_publisher_session_entry() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "P8 SourcePublisher session requires Linux",
    ))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> P8PublisherHelloV1 {
        let release = P8HarnessReleaseManifestV1::test_fixture(P8QualityDigest::derive(
            "p8_publication_fixture_release_v1",
            &"release",
        ));
        let roles = P8HarnessExecutableRoleV1::ALL
            .into_iter()
            .enumerate()
            .map(|(index, role)| P8PublisherRoleCapabilityV1 {
                role,
                locator: PathBuf::from(format!("/fixture/{}", role.executable_file_name())),
                executable_byte_len: u64::try_from(index + 1).expect("fixture length"),
                executable_digest: release
                    .role_executable_digest(role)
                    .expect("fixture role digest")
                    .clone(),
                physical_identity_digest: P8QualityDigest::derive(
                    "p8_publication_fixture_physical_identity_v1",
                    &index,
                ),
            })
            .collect();
        let stage_name = format!(".p8-harness-{}-42-1", release.content_address());
        let mut hello = P8PublisherHelloV1 {
            schema: HELLO_SCHEMA.into(),
            nonce: vec![7; 32],
            parent_pid: 41,
            expected_publisher_pid: 42,
            releases_root_locator: PathBuf::from("/fixture/releases"),
            releases_root_physical_identity: P8QualityDigest::derive(
                "p8_publication_fixture_root_v1",
                &"root",
            ),
            release,
            roles,
            supervisor_session_digest: serde_json::from_str(&format!(
                "\"p8_trusted_supervisor_session:sha256:{}\"",
                "b".repeat(64)
            ))
            .expect("typed supervisor session ref"),
            supervisor_plan_digest: P8QualityDigest::derive(
                "p8_publication_fixture_supervisor_plan_v1",
                &"plan",
            ),
            supervisor_descriptor_manifest_digest: P8QualityDigest::derive(
                "p8_publication_fixture_descriptor_manifest_v1",
                &"descriptors",
            ),
            stage_name,
            channel_deadline_monotonic_nanos: 99,
            plan_digest: P8QualityDigest::derive("p8_source_publisher_plan_v1", &()),
        };
        hello.plan_digest = hello.derived_plan_digest();
        hello
    }

    fn stage_ready(hello: &P8PublisherHelloV1) -> P8PublisherStageReadyV1 {
        let device = 17;
        let inode = 23;
        let mut value = P8PublisherStageReadyV1 {
            schema: STAGE_READY_SCHEMA.into(),
            plan_digest: hello.plan_digest.clone(),
            nonce_digest: P8QualityDigest::derive("p8_source_publisher_nonce_v1", &hello.nonce),
            parent_pid: hello.parent_pid,
            publisher_pid: hello.expected_publisher_pid,
            release_digest: hello.release.release_digest().clone(),
            releases_root_physical_identity: hello.releases_root_physical_identity.clone(),
            exact_role_digests: hello
                .roles
                .iter()
                .map(|entry| (entry.role, entry.executable_digest.clone()))
                .collect(),
            stage_name: hello.stage_name.clone(),
            stage_directory_device: device,
            stage_directory_inode: inode,
            stage_binding: prepared_harness_stage_binding(&hello.release, device, inode),
            channel_deadline_monotonic_nanos: hello.channel_deadline_monotonic_nanos,
            stage_ready_digest: P8QualityDigest::derive("p8_source_publisher_stage_ready_v1", &()),
        };
        value.stage_ready_digest = value.derived_digest();
        value
    }

    #[test]
    fn hello_and_stage_ready_reject_exact_plan_role_stage_and_deadline_drift() {
        assert_eq!(PUBLISHER_HELLO_FD_COUNT, 5);
        assert_eq!(PUBLISHER_STAGE_READY_FD_COUNT, 1);
        assert_eq!(PUBLISHER_COMMIT_PERMIT_FD_COUNT, 0);
        assert_eq!(PUBLISHER_DRAFT_FD_COUNT, 0);
        let hello = hello();
        assert!(hello.validate_contract());
        let ready = stage_ready(&hello);
        assert!(ready.validate_against(&hello));
        let intent = P8HarnessPublicationIntentV1::from_hello(&hello);
        assert!(intent.validate_contract());
        let ack = P8HarnessPublicationIntentAckV1::new(&intent, 40, 41, 99);
        assert!(ack.validate_against(&intent, 40, 41, 99));
        let mut permit = P8PublisherCommitPermitMessageV1 {
            schema: COMMIT_PERMIT_SCHEMA.into(),
            plan_digest: hello.plan_digest.clone(),
            stage_ready_digest: ready.stage_ready_digest.clone(),
            nonce_digest: ready.nonce_digest.clone(),
            parent_pid: hello.parent_pid,
            publisher_pid: hello.expected_publisher_pid,
            release_binding: P8QualityDigest::derive(
                "p8_harness_publisher_commit_plan_binding_v1",
                hello.release.release_digest(),
            ),
            stage_binding: ready.stage_binding.clone(),
            channel_deadline_monotonic_nanos: hello.channel_deadline_monotonic_nanos,
            permit_digest: P8QualityDigest::derive("p8_source_publisher_commit_permit_v1", &()),
        };
        permit.permit_digest = permit.derived_digest();
        assert!(permit.validate_against(&hello, &ready));

        let mut wrong_order = hello.clone();
        wrong_order.roles.reverse();
        wrong_order.plan_digest = wrong_order.derived_plan_digest();
        assert!(!wrong_order.validate_contract());

        let mut aliased = hello.clone();
        aliased.roles[1].physical_identity_digest =
            aliased.roles[0].physical_identity_digest.clone();
        aliased.plan_digest = aliased.derived_plan_digest();
        assert!(!aliased.validate_contract());

        let mut wrong_stage = ready.clone();
        wrong_stage.stage_directory_inode += 1;
        wrong_stage.stage_ready_digest = wrong_stage.derived_digest();
        assert!(!wrong_stage.validate_against(&hello));

        let mut wrong_selected_stage = ready.clone();
        wrong_selected_stage.stage_name = format!(
            ".p8-harness-{}-{}-2",
            hello.release.content_address(),
            hello.expected_publisher_pid
        );
        wrong_selected_stage.stage_ready_digest = wrong_selected_stage.derived_digest();
        assert!(!wrong_selected_stage.validate_against(&hello));

        let mut wrong_deadline = ready.clone();
        wrong_deadline.channel_deadline_monotonic_nanos += 1;
        wrong_deadline.stage_ready_digest = wrong_deadline.derived_digest();
        assert!(!wrong_deadline.validate_against(&hello));

        permit.stage_binding =
            P8QualityDigest::derive("p8_publication_fixture_wrong_stage_v1", &"wrong");
        permit.permit_digest = permit.derived_digest();
        assert!(!permit.validate_against(&hello, &ready));

        let mut wrong_ack = ack;
        wrong_ack.release_absent_observed = false;
        wrong_ack.ack_digest = wrong_ack.derived_digest();
        assert!(!wrong_ack.validate_against(&intent, 40, 41, 99));
    }

    #[test]
    fn publication_protocol_fault_matrix_rejects_every_admission_boundary_drift() {
        let hello = hello();
        let ready = stage_ready(&hello);
        let intent = P8HarnessPublicationIntentV1::from_hello(&hello);
        let ack = P8HarnessPublicationIntentAckV1::new(&intent, 40, 41, 99);
        let mut permit = P8PublisherCommitPermitMessageV1 {
            schema: COMMIT_PERMIT_SCHEMA.into(),
            plan_digest: hello.plan_digest.clone(),
            stage_ready_digest: ready.stage_ready_digest.clone(),
            nonce_digest: ready.nonce_digest.clone(),
            parent_pid: hello.parent_pid,
            publisher_pid: hello.expected_publisher_pid,
            release_binding: P8QualityDigest::derive(
                "p8_harness_publisher_commit_plan_binding_v1",
                hello.release.release_digest(),
            ),
            stage_binding: ready.stage_binding.clone(),
            channel_deadline_monotonic_nanos: hello.channel_deadline_monotonic_nanos,
            permit_digest: P8QualityDigest::derive("p8_source_publisher_commit_permit_v1", &()),
        };
        permit.permit_digest = permit.derived_digest();

        for expected in [
            PUBLISHER_HELLO_FD_COUNT,
            PUBLISHER_STAGE_READY_FD_COUNT,
            PUBLISHER_COMMIT_PERMIT_FD_COUNT,
            PUBLISHER_DRAFT_FD_COUNT,
        ] {
            assert!(publisher_fd_count_is_exact(expected, expected));
            for observed in 0..=expected + 1 {
                if observed != expected {
                    assert!(!publisher_fd_count_is_exact(observed, expected));
                }
            }
        }

        let mut hello_faults = Vec::new();
        let mut value = hello.clone();
        value.nonce.pop();
        hello_faults.push(value);
        let mut value = hello.clone();
        value.parent_pid = 0;
        hello_faults.push(value);
        let mut value = hello.clone();
        value.expected_publisher_pid = value.parent_pid;
        hello_faults.push(value);
        let mut value = hello.clone();
        value.releases_root_locator = PathBuf::from("relative");
        hello_faults.push(value);
        let mut value = hello.clone();
        value.roles.pop();
        hello_faults.push(value);
        let mut value = hello.clone();
        value.channel_deadline_monotonic_nanos = 0;
        hello_faults.push(value);
        assert!(hello_faults
            .into_iter()
            .all(|value| !value.validate_contract()));

        let mut ready_faults = Vec::new();
        let mut value = ready.clone();
        value.plan_digest = P8QualityDigest::derive("p8_fault_plan_v1", &"wrong");
        value.stage_ready_digest = value.derived_digest();
        ready_faults.push(value);
        let mut value = ready.clone();
        value.nonce_digest = P8QualityDigest::derive("p8_fault_nonce_v1", &"wrong");
        value.stage_ready_digest = value.derived_digest();
        ready_faults.push(value);
        let mut value = ready.clone();
        value.publisher_pid += 1;
        value.stage_ready_digest = value.derived_digest();
        ready_faults.push(value);
        let mut value = ready.clone();
        value.exact_role_digests.pop();
        value.stage_ready_digest = value.derived_digest();
        ready_faults.push(value);
        let mut value = ready.clone();
        value.stage_name.push_str("-alias");
        value.stage_ready_digest = value.derived_digest();
        ready_faults.push(value);
        assert!(ready_faults
            .into_iter()
            .all(|value| !value.validate_against(&hello)));

        let mut ack_faults = Vec::new();
        let mut value = ack.clone();
        value.intent_digest = P8QualityDigest::derive("p8_fault_intent_v1", &"wrong");
        value.ack_digest = value.derived_digest();
        ack_faults.push(value);
        let mut value = ack.clone();
        value.parent_pid += 1;
        value.ack_digest = value.derived_digest();
        ack_faults.push(value);
        let mut value = ack.clone();
        value.supervisor_pid += 1;
        value.ack_digest = value.derived_digest();
        ack_faults.push(value);
        let mut value = ack.clone();
        value.release_absent_observed = false;
        value.ack_digest = value.derived_digest();
        ack_faults.push(value);
        let mut value = ack;
        value.channel_deadline_monotonic_nanos += 1;
        value.ack_digest = value.derived_digest();
        ack_faults.push(value);
        assert!(ack_faults
            .into_iter()
            .all(|value| !value.validate_against(&intent, 40, 41, 99)));

        let mut permit_faults = Vec::new();
        let mut value = permit.clone();
        value.plan_digest = P8QualityDigest::derive("p8_fault_permit_plan_v1", &"wrong");
        value.permit_digest = value.derived_digest();
        permit_faults.push(value);
        let mut value = permit.clone();
        value.stage_ready_digest = P8QualityDigest::derive("p8_fault_stage_ready_v1", &"wrong");
        value.permit_digest = value.derived_digest();
        permit_faults.push(value);
        let mut value = permit.clone();
        value.publisher_pid += 1;
        value.permit_digest = value.derived_digest();
        permit_faults.push(value);
        let mut value = permit.clone();
        value.stage_binding = P8QualityDigest::derive("p8_fault_stage_binding_v1", &"wrong");
        value.permit_digest = value.derived_digest();
        permit_faults.push(value);
        let mut value = permit;
        value.channel_deadline_monotonic_nanos += 1;
        value.permit_digest = value.derived_digest();
        permit_faults.push(value);
        assert!(permit_faults
            .into_iter()
            .all(|value| !value.validate_against(&hello, &ready)));
    }

    #[test]
    fn raw_closure_state_machine_rejects_published_and_exactly_separates_commit_boundary() {
        let hello = hello();
        let draft_ref: P8HarnessPublicationDraftRef = serde_json::from_str(&format!(
            "\"p8_harness_publication_draft:sha256:{}\"",
            "a".repeat(64)
        ))
        .expect("typed draft ref");
        let mut awaiting = P8HarnessPublicationClosureDraftV1 {
            schema: CLOSURE_DRAFT_SCHEMA.into(),
            state: P8ParentPublicationStateV1::CommittedAwaitingOuterClosure,
            release: hello.release.clone(),
            supervisor_session_digest: hello.supervisor_session_digest.clone(),
            supervisor_plan_digest: hello.supervisor_plan_digest.clone(),
            supervisor_descriptor_manifest_digest: hello
                .supervisor_descriptor_manifest_digest
                .clone(),
            publisher_plan_digest: hello.plan_digest.clone(),
            stage_ready_digest: Some(P8QualityDigest::derive(
                "p8_publication_fixture_stage_ready_v1",
                &"ready",
            )),
            commit_permit_digest: Some(P8QualityDigest::derive(
                "p8_publication_fixture_commit_permit_v1",
                &"permit",
            )),
            stage_binding: Some(P8QualityDigest::derive(
                "p8_publication_fixture_stage_binding_v1",
                &"stage",
            )),
            commit_permit_sent: true,
            stage_name: hello.stage_name.clone(),
            stage_absent_observed: false,
            draft_digest: Some(draft_ref),
            publisher_executable_digest: hello.roles[0].executable_digest.clone(),
            publisher_pid: hello.expected_publisher_pid,
            publisher_exit_code: Some(0),
            stdout_byte_len: 0,
            stdout_digest: P8QualityDigest::derive(
                "p8_source_publisher_closed_stdout_v1",
                &Vec::<u8>::new(),
            ),
            stdout_eof_observed: true,
            stderr_byte_len: 0,
            stderr_digest: P8QualityDigest::derive(
                "p8_source_publisher_closed_stderr_v1",
                &Vec::<u8>::new(),
            ),
            stderr_eof_observed: true,
            release_directory_identity: Some((17, 23)),
            closure_draft_digest: P8QualityDigest::derive(
                "p8_harness_publication_parent_closure_draft_v1",
                &(),
            ),
        };
        awaiting.closure_draft_digest = awaiting.derived_digest();
        assert!(awaiting.validate_contract().is_empty());
        assert!(!serde_json::to_string(&awaiting)
            .expect("closure serialization")
            .contains("\"published\""));

        let mut unattested = awaiting.clone();
        unattested.state = P8ParentPublicationStateV1::CommittedUnattested;
        unattested.draft_digest = None;
        unattested.closure_draft_digest = unattested.derived_digest();
        assert!(unattested.validate_contract().is_empty());

        let mut complete_but_unattested = unattested.clone();
        complete_but_unattested.draft_digest = awaiting.draft_digest.clone();
        complete_but_unattested.closure_draft_digest = complete_but_unattested.derived_digest();
        assert!(!complete_but_unattested.validate_contract().is_empty());

        let mut draft_received_but_final_unattested = unattested.clone();
        draft_received_but_final_unattested.draft_digest = awaiting.draft_digest.clone();
        draft_received_but_final_unattested.release_directory_identity = None;
        draft_received_but_final_unattested.closure_draft_digest =
            draft_received_but_final_unattested.derived_digest();
        assert!(draft_received_but_final_unattested
            .validate_contract()
            .is_empty());

        let mut committed_faults = Vec::new();
        let mut value = awaiting.clone();
        value.draft_digest = None;
        committed_faults.push(value);
        let mut value = awaiting.clone();
        value.publisher_exit_code = Some(1);
        committed_faults.push(value);
        let mut value = awaiting.clone();
        value.stdout_eof_observed = false;
        committed_faults.push(value);
        let mut value = awaiting.clone();
        value.stderr_eof_observed = false;
        committed_faults.push(value);
        let mut value = awaiting.clone();
        value.release_directory_identity = None;
        committed_faults.push(value);
        for mut value in committed_faults {
            value.state = P8ParentPublicationStateV1::CommittedUnattested;
            value.closure_draft_digest = value.derived_digest();
            assert!(value.validate_contract().is_empty());
            value.state = P8ParentPublicationStateV1::CommittedAwaitingOuterClosure;
            value.closure_draft_digest = value.derived_digest();
            assert!(!value.validate_contract().is_empty());
        }

        let mut precommit = unattested;
        precommit.state = P8ParentPublicationStateV1::PreCommitFailed;
        precommit.stage_ready_digest = None;
        precommit.commit_permit_digest = None;
        precommit.stage_binding = None;
        precommit.commit_permit_sent = false;
        precommit.stage_absent_observed = true;
        precommit.release_directory_identity = None;
        precommit.closure_draft_digest = precommit.derived_digest();
        assert!(precommit.validate_contract().is_empty());

        precommit.stage_absent_observed = false;
        precommit.closure_draft_digest = precommit.derived_digest();
        assert!(!precommit.validate_contract().is_empty());
    }

    #[test]
    fn raw_gate_receipts_cannot_bypass_the_trusted_execution_release_authority() {
        let execution_source = include_str!("engineering_gate/execution.rs");
        let release_source = include_str!("../source_release.rs");
        assert!(!execution_source.contains("pub(crate) fn into_parts"));
        assert!(execution_source.contains("pub(super) fn into_parts"));
        assert!(release_source
            .contains("_authority: super::trusted_execution::P8ReleaseAssemblyAuthority"));
        assert!(!release_source.contains("pub(crate) fn from_trusted_execution_parts"));
    }
}
