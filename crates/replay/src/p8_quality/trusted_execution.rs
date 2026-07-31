//! P8-owned wrapper over generation-neutral sealed execution.
//!
//! A sealed process receipt proves immutable Linux execution bytes and parent-observed process
//! closure. It deliberately remains engineering evidence until a separate trusted Linux lease and
//! cgroup process-domain receipt are bound.

pub(super) mod engineering_gate;
pub(super) mod publication;
pub(super) mod supervisor_session;

use std::io;
#[cfg(test)]
use std::{path::Path, time::Duration};

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
pub(super) struct P8ReleaseAssemblyAuthority {
    _private: (),
}

#[cfg(target_os = "linux")]
fn assemble_verified_gate_set_release(
    verified: engineering_gate::VerifiedP8EngineeringGateSet,
) -> Result<
    (
        super::source_release::P8HarnessReleaseManifestV1,
        supervisor_session::P8TrustedSupervisorInputs,
    ),
    Vec<super::P8QualityContractFailure>,
> {
    let (engineering_gate_receipt, process_receipts, trusted_inputs) =
        engineering_gate::consume_verified_gate_set(verified);
    let release = super::source_release::P8HarnessReleaseManifestV1::from_trusted_execution_parts(
        P8ReleaseAssemblyAuthority { _private: () },
        trusted_inputs.source_input.clone(),
        engineering_gate_receipt,
        process_receipts,
    )?;
    Ok((release, trusted_inputs))
}

#[cfg(target_os = "linux")]
use crate::bounded_process::run_bounded_command_closed;
#[cfg(any(target_os = "linux", test))]
use crate::bounded_process::BoundedProcessLimits;
use crate::{
    bounded_process::linux_cgroup_v2::{
        parse_cgroup_populated, parse_cgroup_procs, parse_memory_peak,
        parse_unified_proc_membership, MemoryEventCounters,
    },
    p8_quality_process::claim_p8_quality_execution,
    sealed_execution::ClaimedSealedExecution,
};
#[cfg(test)]
use crate::{
    bounded_process::{run_bounded_command, BoundedProcessOutput, BoundedProcessTermination},
    p8_quality_process::{
        p8_quality_execution_domain, role_self_test_stdout, P8_ENGINEERING_GATE_SEALED_STDOUT,
        P8_ENGINEERING_GATE_SELF_TEST_ARG, P8_ROLE_SELF_TEST_ARG,
    },
    sealed_execution::RetainedExecutable,
};
#[cfg(all(target_os = "linux", not(test)))]
use crate::{
    p8_quality_process::{
        p8_quality_execution_domain, role_self_test_stdout, P8_ROLE_SELF_TEST_ARG,
    },
    sealed_execution::RetainedExecutable,
};

use super::{
    domain_separated_sha256, has_typed_sha256_prefix,
    source_release::{P8ArmReleaseRef, P8HarnessExecutableRoleV1, P8HarnessSourceInputRef},
    P8QualityArmKind, P8QualityContractFailure, P8QualityDigest, P8QualityRunRef,
    P8TrustedExecutionLeaseRef,
};

const P8_SEALED_PROCESS_RECEIPT_SCHEMA: &str = "beetle-memory.p8.quality-sealed-process-receipt.v1";
const P8_SEALED_EXECUTION_CONTRACT: &str = "beetle-memory.p8.linux-memfd-fexecve-parent-closure.v1";
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct P8SealedProcessReceiptRef(String);

impl P8SealedProcessReceiptRef {
    fn derive(value: &impl Serialize) -> Self {
        let bytes = serde_json::to_vec(value)
            .expect("P8 sealed process receipt serialization must be infallible");
        Self(format!(
            "p8_sealed_process_receipt:sha256:{}",
            domain_separated_sha256("p8_quality_sealed_process_receipt_v1", &[bytes.as_slice()])
        ))
    }
}

impl<'de> Deserialize<'de> for P8SealedProcessReceiptRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        let value = String::deserialize(deserializer)?;
        if has_typed_sha256_prefix(&value, "p8_sealed_process_receipt:sha256:") {
            Ok(Self(value))
        } else {
            Err(D::Error::custom(
                "invalid P8 sealed process receipt identity",
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8SealedProcessEvidenceClassV1 {
    EngineeringSealedNoTrustedLeaseOrCgroup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8SealedProcessTerminationV1 {
    Exited,
    TimedOut,
    StdoutLimitExceeded,
    StderrLimitExceeded,
    TotalLimitExceeded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8SealedProcessReceiptV1 {
    schema: String,
    evidence_class: P8SealedProcessEvidenceClassV1,
    source_input_digest: P8HarnessSourceInputRef,
    role: P8HarnessExecutableRoleV1,
    executable_digest: P8QualityDigest,
    executable_byte_len: u64,
    sealing_contract_digest: P8QualityDigest,
    pid: u32,
    process_group: i64,
    termination: P8SealedProcessTerminationV1,
    exit_code: Option<i32>,
    stdout_bytes: u64,
    stdout_digest: P8QualityDigest,
    stdout_eof_observed: bool,
    stderr_bytes: u64,
    stderr_digest: P8QualityDigest,
    stderr_eof_observed: bool,
    elapsed_millis: u64,
    direct_child_maximum_rss_bytes_diagnostic_only: u64,
    receipt_digest: P8SealedProcessReceiptRef,
}

impl P8SealedProcessReceiptV1 {
    pub(crate) fn validate_contract(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != P8_SEALED_PROCESS_RECEIPT_SCHEMA
            || self.evidence_class
                != P8SealedProcessEvidenceClassV1::EngineeringSealedNoTrustedLeaseOrCgroup
        {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        if self.executable_byte_len == 0 {
            failures.push(P8QualityContractFailure::CoverageMismatch);
        }
        if !self.stdout_eof_observed || !self.stderr_eof_observed {
            failures.push(P8QualityContractFailure::PipeClosureMissing);
        }
        if self.receipt_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn is_successfully_closed(&self) -> bool {
        self.validate_contract().is_empty()
            && self.termination == P8SealedProcessTerminationV1::Exited
            && self.exit_code == Some(0)
    }

    pub(crate) fn role(&self) -> P8HarnessExecutableRoleV1 {
        self.role
    }

    pub(crate) fn source_input_digest(&self) -> &P8HarnessSourceInputRef {
        &self.source_input_digest
    }

    pub(crate) fn executable_digest(&self) -> &P8QualityDigest {
        &self.executable_digest
    }

    pub(crate) fn receipt_digest(&self) -> &P8SealedProcessReceiptRef {
        &self.receipt_digest
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn stdout_observation(&self) -> (u64, &P8QualityDigest) {
        (self.stdout_bytes, &self.stdout_digest)
    }

    pub(crate) fn stderr_observation(&self) -> (u64, &P8QualityDigest) {
        (self.stderr_bytes, &self.stderr_digest)
    }

    fn derived_digest(&self) -> P8SealedProcessReceiptRef {
        P8SealedProcessReceiptRef::derive(&(
            (
                &self.schema,
                self.evidence_class,
                &self.source_input_digest,
                self.role,
                &self.executable_digest,
                self.executable_byte_len,
                &self.sealing_contract_digest,
            ),
            (
                self.pid,
                self.process_group,
                self.termination,
                self.exit_code,
                self.stdout_bytes,
                &self.stdout_digest,
                self.stdout_eof_observed,
                self.stderr_bytes,
                &self.stderr_digest,
                self.stderr_eof_observed,
                self.elapsed_millis,
                self.direct_child_maximum_rss_bytes_diagnostic_only,
            ),
        ))
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct P8SealedProcessLimits {
    pub(crate) stdout_bytes: u64,
    pub(crate) stderr_bytes: u64,
    pub(crate) total_bytes: u64,
    pub(crate) timeout: Duration,
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct P8SealedProcessOutput {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) receipt: P8SealedProcessReceiptV1,
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct P8LinuxCgroupEngineeringObservationV1 {
    pub(crate) membership_path: String,
    pub(crate) mount_id: u64,
    pub(crate) device: u64,
    pub(crate) inode: u64,
    pub(crate) initial_cgroup_procs: Vec<u8>,
    pub(crate) initial_cgroup_events: Vec<u8>,
    pub(crate) barrier_cgroup_procs: Vec<u8>,
    pub(crate) child_proc_cgroup: Vec<u8>,
    pub(crate) final_cgroup_procs: Vec<u8>,
    pub(crate) final_cgroup_events: Vec<u8>,
    pub(crate) memory_peak: Vec<u8>,
    pub(crate) memory_events_before: Vec<u8>,
    pub(crate) memory_events_after: Vec<u8>,
    pub(crate) memory_events_local_before: Vec<u8>,
    pub(crate) memory_events_local_after: Vec<u8>,
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) struct P8LinuxCgroupSealedOutput {
    pub(crate) process: P8SealedProcessOutput,
    pub(crate) cgroup: P8LinuxCgroupEngineeringObservationV1,
}

#[cfg(test)]
pub(crate) struct P8SealedProcessLauncher {
    source_input_digest: P8HarnessSourceInputRef,
    role: P8HarnessExecutableRoleV1,
    executable: RetainedExecutable,
}

#[cfg(test)]
impl P8SealedProcessLauncher {
    pub(crate) fn open(
        source_input_digest: P8HarnessSourceInputRef,
        role: P8HarnessExecutableRoleV1,
        executable_path: &Path,
    ) -> io::Result<Self> {
        Ok(Self {
            source_input_digest,
            role,
            executable: RetainedExecutable::open(executable_path)?,
        })
    }

    pub(crate) fn from_retained_executable(
        source_input_digest: P8HarnessSourceInputRef,
        role: P8HarnessExecutableRoleV1,
        executable: RetainedExecutable,
    ) -> Self {
        Self {
            source_input_digest,
            role,
            executable,
        }
    }

    pub(crate) fn run(
        &mut self,
        args: &[String],
        limits: P8SealedProcessLimits,
    ) -> io::Result<P8SealedProcessOutput> {
        let expected_stdout = self.expected_engineering_stdout(args)?;
        let prepared = self
            .executable
            .prepare(p8_quality_execution_domain(), args)?;
        let (mut command, guard, identity) = prepared.into_parts();
        let output = run_bounded_command(
            &mut command,
            BoundedProcessLimits {
                stdout_bytes: limits.stdout_bytes,
                stderr_bytes: limits.stderr_bytes,
                total_bytes: limits.total_bytes,
                timeout: limits.timeout,
            },
        )?;
        self.executable.verify_content(&identity)?;
        drop(guard);
        self.finalize_process_output(expected_stdout, identity, output)
    }

    pub(crate) fn run_linux_cgroup_v2_engineering(
        &mut self,
        args: &[String],
        run_root: &Path,
        limits: P8SealedProcessLimits,
        barrier_timeout: Duration,
        attempt_nonce: u64,
    ) -> io::Result<P8LinuxCgroupSealedOutput> {
        let expected_stdout = self.expected_engineering_stdout(args)?;
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (
                run_root,
                limits,
                barrier_timeout,
                attempt_nonce,
                expected_stdout,
            );
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "P8 cgroup v2 sealed execution requires Linux",
            ))
        }
        #[cfg(target_os = "linux")]
        {
            use crate::bounded_process::{
                linux_cgroup_v2::LinuxCgroupV2RunRoot, run_bounded_prepared_sealed_linux_cgroup_v2,
            };

            let (root, initial) = LinuxCgroupV2RunRoot::open_existing_fresh(run_root)?;
            let prepared = self.executable.prepare_with_linux_pre_exec_barrier(
                p8_quality_execution_domain(),
                args,
                attempt_nonce,
            )?;
            let output = run_bounded_prepared_sealed_linux_cgroup_v2(
                prepared,
                root,
                initial,
                BoundedProcessLimits {
                    stdout_bytes: limits.stdout_bytes,
                    stderr_bytes: limits.stderr_bytes,
                    total_bytes: limits.total_bytes,
                    timeout: limits.timeout,
                },
                barrier_timeout,
            )?;
            self.executable
                .verify_content(&output.executable_identity)?;
            let cgroup = P8LinuxCgroupEngineeringObservationV1 {
                membership_path: output.initial.membership_path,
                mount_id: output.initial.mount_id,
                device: output.initial.device,
                inode: output.initial.inode,
                initial_cgroup_procs: output.initial.cgroup_procs,
                initial_cgroup_events: output.initial.cgroup_events,
                barrier_cgroup_procs: output.barrier.cgroup_procs,
                child_proc_cgroup: output.barrier.child_proc_cgroup,
                final_cgroup_procs: output.final_cgroup_procs,
                final_cgroup_events: output.final_cgroup_events,
                memory_peak: output.memory_peak,
                memory_events_before: output.initial.memory_events,
                memory_events_after: output.memory_events_after,
                memory_events_local_before: output.initial.memory_events_local,
                memory_events_local_after: output.memory_events_local_after,
            };
            let process = self.finalize_process_output(
                expected_stdout,
                output.executable_identity,
                output.process,
            )?;
            Ok(P8LinuxCgroupSealedOutput { process, cgroup })
        }
    }

    fn expected_engineering_stdout(&self, args: &[String]) -> io::Result<&'static [u8]> {
        match args {
            [arg] if arg == P8_ROLE_SELF_TEST_ARG => role_self_test_stdout(self.role.schema_name())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "P8 sealed role is unknown")
                }),
            [arg]
                if arg == P8_ENGINEERING_GATE_SELF_TEST_ARG
                    && self.role == P8HarnessExecutableRoleV1::TrustedSupervisor =>
            {
                Ok(P8_ENGINEERING_GATE_SEALED_STDOUT)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "P8 sealed engineering launcher command is not exact",
            )),
        }
    }

    fn finalize_process_output(
        &self,
        expected_stdout: &[u8],
        identity: crate::sealed_execution::SealedContentIdentity,
        output: BoundedProcessOutput,
    ) -> io::Result<P8SealedProcessOutput> {
        if output.termination != BoundedProcessTermination::Exited
            || !output.status.success()
            || output.stdout != expected_stdout
            || !output.stderr.is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "P8 sealed role handshake does not match the expected executable role",
            ));
        }
        let termination = match output.termination {
            BoundedProcessTermination::Exited => P8SealedProcessTerminationV1::Exited,
            BoundedProcessTermination::TimedOut => P8SealedProcessTerminationV1::TimedOut,
            BoundedProcessTermination::StdoutLimitExceeded => {
                P8SealedProcessTerminationV1::StdoutLimitExceeded
            }
            BoundedProcessTermination::StderrLimitExceeded => {
                P8SealedProcessTerminationV1::StderrLimitExceeded
            }
            BoundedProcessTermination::TotalLimitExceeded => {
                P8SealedProcessTerminationV1::TotalLimitExceeded
            }
        };
        let executable_digest = P8QualityDigest::parse(format!("sha256:{}", identity.sha256()))
            .expect("neutral sealed SHA256 is canonical");
        let stdout_bytes = u64::try_from(output.stdout.len())
            .map_err(|_| io::Error::other("P8 sealed stdout length overflow"))?;
        let stderr_bytes = u64::try_from(output.stderr.len())
            .map_err(|_| io::Error::other("P8 sealed stderr length overflow"))?;
        let mut receipt = P8SealedProcessReceiptV1 {
            schema: P8_SEALED_PROCESS_RECEIPT_SCHEMA.into(),
            evidence_class: P8SealedProcessEvidenceClassV1::EngineeringSealedNoTrustedLeaseOrCgroup,
            source_input_digest: self.source_input_digest.clone(),
            role: self.role,
            executable_digest,
            executable_byte_len: identity.byte_len(),
            sealing_contract_digest: P8QualityDigest::derive(
                "p8_sealed_execution_contract_v1",
                &P8_SEALED_EXECUTION_CONTRACT,
            ),
            pid: output.pid,
            process_group: output.process_group,
            termination,
            exit_code: output.status.code(),
            stdout_bytes,
            stdout_digest: P8QualityDigest::derive("p8_sealed_process_stdout_v1", &output.stdout),
            stdout_eof_observed: true,
            stderr_bytes,
            stderr_digest: P8QualityDigest::derive("p8_sealed_process_stderr_v1", &output.stderr),
            stderr_eof_observed: true,
            elapsed_millis: u64::try_from(output.elapsed.as_millis()).unwrap_or(u64::MAX),
            direct_child_maximum_rss_bytes_diagnostic_only: output.maximum_rss_bytes,
            receipt_digest: P8SealedProcessReceiptRef::derive(&()),
        };
        receipt.receipt_digest = receipt.derived_digest();
        if !receipt.validate_contract().is_empty() {
            return Err(io::Error::other(
                "P8 sealed process receipt failed self-validation",
            ));
        }
        Ok(P8SealedProcessOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            receipt,
        })
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn run_retained_harness_role_self_test(
    source_input_digest: P8HarnessSourceInputRef,
    role: P8HarnessExecutableRoleV1,
    executable: &mut RetainedExecutable,
    limits: BoundedProcessLimits,
) -> io::Result<P8SealedProcessReceiptV1> {
    let expected_stdout = role_self_test_stdout(role.schema_name())
        .ok_or_else(|| invalid_data("P8 sealed role is unknown"))?;
    let prepared = executable.prepare_with_linux_exact_environment(
        p8_quality_execution_domain(),
        &[P8_ROLE_SELF_TEST_ARG.to_string()],
        &[],
        Vec::new(),
    )?;
    let (command, guard, identity) = prepared.into_parts();
    let output = run_bounded_command_closed(command, limits);
    drop(guard);
    let output = output?;
    executable.verify_content(&identity)?;
    if output.termination() != crate::bounded_process::BoundedProcessTermination::Exited
        || !output.status().success()
        || output.stdout() != expected_stdout
        || !output.stderr().is_empty()
        || !output.stdout_eof_observed()
        || !output.stderr_eof_observed()
    {
        return Err(invalid_data(
            "P8 sealed role self-test did not close exactly",
        ));
    }
    let executable_digest = P8QualityDigest::parse(format!("sha256:{}", identity.sha256()))
        .map_err(|_| invalid_data("P8 sealed role digest is invalid"))?;
    let mut receipt = P8SealedProcessReceiptV1 {
        schema: P8_SEALED_PROCESS_RECEIPT_SCHEMA.into(),
        evidence_class: P8SealedProcessEvidenceClassV1::EngineeringSealedNoTrustedLeaseOrCgroup,
        source_input_digest,
        role,
        executable_digest,
        executable_byte_len: identity.byte_len(),
        sealing_contract_digest: P8QualityDigest::derive(
            "p8_sealed_execution_contract_v1",
            &P8_SEALED_EXECUTION_CONTRACT,
        ),
        pid: output.pid(),
        process_group: output.process_group(),
        termination: P8SealedProcessTerminationV1::Exited,
        exit_code: output.status().code(),
        stdout_bytes: u64::try_from(output.stdout().len())
            .map_err(|_| invalid_data("P8 sealed role stdout length overflow"))?,
        stdout_digest: P8QualityDigest::derive("p8_sealed_process_stdout_v1", &output.stdout()),
        stdout_eof_observed: output.stdout_eof_observed(),
        stderr_bytes: u64::try_from(output.stderr().len())
            .map_err(|_| invalid_data("P8 sealed role stderr length overflow"))?,
        stderr_digest: P8QualityDigest::derive("p8_sealed_process_stderr_v1", &output.stderr()),
        stderr_eof_observed: output.stderr_eof_observed(),
        elapsed_millis: u64::try_from(output.elapsed().as_millis())
            .map_err(|_| invalid_data("P8 sealed role elapsed overflow"))?,
        direct_child_maximum_rss_bytes_diagnostic_only: output.maximum_rss_bytes(),
        receipt_digest: P8SealedProcessReceiptRef::derive(&()),
    };
    receipt.receipt_digest = receipt.derived_digest();
    if !receipt.validate_contract().is_empty() || !receipt.is_successfully_closed() {
        return Err(invalid_data(
            "P8 sealed role receipt failed self-validation",
        ));
    }
    Ok(receipt)
}

pub(crate) struct P8QualityExecutionAuthority {
    claimed: ClaimedSealedExecution,
    role: P8HarnessExecutableRoleV1,
}

impl P8QualityExecutionAuthority {
    pub(crate) fn claim_source_publisher() -> io::Result<Self> {
        Ok(Self {
            claimed: claim_p8_quality_execution()?,
            role: P8HarnessExecutableRoleV1::SourcePublisher,
        })
    }

    pub(crate) fn claim_trusted_supervisor() -> io::Result<Self> {
        Ok(Self {
            claimed: claim_p8_quality_execution()?,
            role: P8HarnessExecutableRoleV1::TrustedSupervisor,
        })
    }

    pub(crate) fn role(&self) -> P8HarnessExecutableRoleV1 {
        self.role
    }

    pub(crate) fn verify(&mut self) -> io::Result<()> {
        self.claimed.verify()
    }

    pub(crate) fn executable_digest(&self) -> P8QualityDigest {
        P8QualityDigest::parse(format!("sha256:{}", self.claimed.identity().sha256()))
            .expect("neutral sealed SHA256 is canonical")
    }

    pub(crate) fn copy_executable_to(&mut self, destination: &mut dyn io::Write) -> io::Result<()> {
        self.claimed.copy_to(destination)?;
        Ok(())
    }
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct P8TrustedDomainResourceReceiptRef(String);

impl P8TrustedDomainResourceReceiptRef {
    fn derive(value: &impl Serialize) -> Self {
        let bytes = serde_json::to_vec(value)
            .expect("P8 trusted domain receipt serialization must be infallible");
        Self(format!(
            "p8_trusted_domain_resource_receipt:sha256:{}",
            domain_separated_sha256(
                "p8_quality_trusted_domain_resource_receipt_v1",
                &[bytes.as_slice()]
            )
        ))
    }

    #[cfg(test)]
    pub(crate) fn derive_for_test(value: &str) -> Self {
        Self::derive(&value)
    }
}

impl<'de> Deserialize<'de> for P8TrustedDomainResourceReceiptRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        let value = String::deserialize(deserializer)?;
        if has_typed_sha256_prefix(&value, "p8_trusted_domain_resource_receipt:sha256:") {
            Ok(Self(value))
        } else {
            Err(D::Error::custom(
                "invalid P8 trusted domain resource receipt identity",
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum P8DomainResourceEvidenceV1 {
    ContractFixtureOnlyNoTrustedAuthority,
    TrustedLinuxObserved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8CgroupV2RawObservationV1 {
    child_pid: u32,
    run_root_membership_path: String,
    initial_cgroup_procs: Vec<u8>,
    barrier_cgroup_procs: Vec<u8>,
    child_proc_cgroup: Vec<u8>,
    observed_process_ids: Vec<u32>,
    membership_trace: Vec<Vec<u8>>,
    final_cgroup_procs: Vec<u8>,
    memory_peak: Vec<u8>,
    memory_events_before: Vec<u8>,
    memory_events_after: Vec<u8>,
    memory_events_local_before: Vec<u8>,
    memory_events_local_after: Vec<u8>,
    final_cgroup_events: Vec<u8>,
    stdout_bytes: u64,
    stdout_digest: P8QualityDigest,
    stderr_bytes: u64,
    stderr_digest: P8QualityDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct P8TrustedDomainResourceReceiptV1 {
    schema: String,
    evidence: P8DomainResourceEvidenceV1,
    run_id: P8QualityRunRef,
    arm: P8QualityArmKind,
    arm_release_digest: P8ArmReleaseRef,
    trusted_lease_digest: P8TrustedExecutionLeaseRef,
    supervisor_executable_digest: P8QualityDigest,
    runner_executable_digest: P8QualityDigest,
    host_policy_digest: P8QualityDigest,
    delegation_root_digest: P8QualityDigest,
    run_root_mount_id: u64,
    run_root_device: u64,
    run_root_inode: u64,
    run_root_name_digest: P8QualityDigest,
    runner_process_receipt: Option<Box<P8SealedProcessReceiptV1>>,
    raw: P8CgroupV2RawObservationV1,
    peak_domain_memory_bytes: u64,
    receipt_digest: P8TrustedDomainResourceReceiptRef,
}

impl P8TrustedDomainResourceReceiptV1 {
    pub(crate) fn validate_structure(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = Vec::new();
        if self.schema != "beetle-memory.p8.trusted-domain-resource-receipt.v1"
            || self.raw.child_pid == 0
            || self.run_root_mount_id == 0
            || self.run_root_device == 0
            || self.run_root_inode == 0
        {
            failures.push(P8QualityContractFailure::SchemaMismatch);
        }
        let parsed = (|| -> io::Result<()> {
            if !parse_cgroup_procs(&self.raw.initial_cgroup_procs)?.is_empty() {
                return Err(io::Error::other("cgroup run-root was not initially empty"));
            }
            if parse_cgroup_procs(&self.raw.barrier_cgroup_procs)?
                != std::collections::BTreeSet::from([self.raw.child_pid])
            {
                return Err(io::Error::other("cgroup barrier membership is not exact"));
            }
            if parse_unified_proc_membership(&self.raw.child_proc_cgroup)?
                != self.raw.run_root_membership_path
            {
                return Err(io::Error::other(
                    "child /proc cgroup membership differs from run-root",
                ));
            }
            let observed_process_ids = self
                .raw
                .observed_process_ids
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            if observed_process_ids.len() != self.raw.observed_process_ids.len()
                || !observed_process_ids.contains(&self.raw.child_pid)
                || !self
                    .raw
                    .observed_process_ids
                    .windows(2)
                    .all(|window| window[0] < window[1])
            {
                return Err(io::Error::other(
                    "cgroup observed process identity set is invalid",
                ));
            }
            let mut traced_process_ids = std::collections::BTreeSet::new();
            for snapshot in &self.raw.membership_trace {
                let members = parse_cgroup_procs(snapshot)?;
                if !members.is_subset(&observed_process_ids) {
                    return Err(io::Error::other(
                        "cgroup membership trace contains a foreign PID",
                    ));
                }
                traced_process_ids.extend(members);
            }
            if traced_process_ids != observed_process_ids {
                return Err(io::Error::other(
                    "cgroup membership trace does not close the observed process set",
                ));
            }
            if !parse_cgroup_procs(&self.raw.final_cgroup_procs)?.is_empty()
                || parse_cgroup_populated(&self.raw.final_cgroup_events)?
            {
                return Err(io::Error::other("cgroup run-root was not empty at closure"));
            }
            if parse_memory_peak(&self.raw.memory_peak)? != self.peak_domain_memory_bytes {
                return Err(io::Error::other(
                    "cgroup memory.peak differs from typed resource value",
                ));
            }
            let hierarchical_before = MemoryEventCounters::parse(&self.raw.memory_events_before)?;
            let hierarchical_after = MemoryEventCounters::parse(&self.raw.memory_events_after)?;
            let local_before = MemoryEventCounters::parse(&self.raw.memory_events_local_before)?;
            let local_after = MemoryEventCounters::parse(&self.raw.memory_events_local_after)?;
            if !hierarchical_after
                .checked_delta(&hierarchical_before)?
                .is_zero()
                || !local_after.checked_delta(&local_before)?.is_zero()
            {
                return Err(io::Error::other("cgroup run-root observed an OOM event"));
            }
            Ok(())
        })();
        if parsed.is_err() {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        match &self.runner_process_receipt {
            Some(receipt) => {
                failures.extend(receipt.validate_contract());
                if receipt.role() != P8HarnessExecutableRoleV1::QualityRunner
                    || receipt.executable_digest() != &self.runner_executable_digest
                    || receipt.stdout_observation()
                        != (self.raw.stdout_bytes, &self.raw.stdout_digest)
                    || receipt.stderr_observation()
                        != (self.raw.stderr_bytes, &self.raw.stderr_digest)
                    || !receipt.is_successfully_closed()
                {
                    failures.push(P8QualityContractFailure::TrustedExecutionMissing);
                }
            }
            None if self.evidence == P8DomainResourceEvidenceV1::TrustedLinuxObserved => {
                failures.push(P8QualityContractFailure::TrustedExecutionMissing);
            }
            None => {}
        }
        if self.receipt_digest != self.derived_digest() {
            failures.push(P8QualityContractFailure::DigestInvalid);
        }
        failures
    }

    pub(crate) fn trusted_admission_failures(&self) -> Vec<P8QualityContractFailure> {
        let mut failures = self.validate_structure();
        if self.evidence != P8DomainResourceEvidenceV1::TrustedLinuxObserved {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        // No concrete trusted runner lease/CI/hardware attestation owner exists yet. A
        // self-consistent Linux observation must therefore remain ineligible.
        if !failures.contains(&P8QualityContractFailure::TrustedExecutionMissing) {
            failures.push(P8QualityContractFailure::TrustedExecutionMissing);
        }
        failures
    }

    pub(crate) fn receipt_digest(&self) -> &P8TrustedDomainResourceReceiptRef {
        &self.receipt_digest
    }

    pub(crate) fn peak_domain_memory_bytes(&self) -> u64 {
        self.peak_domain_memory_bytes
    }

    fn derived_digest(&self) -> P8TrustedDomainResourceReceiptRef {
        P8TrustedDomainResourceReceiptRef::derive(&(
            (
                &self.schema,
                self.evidence,
                &self.run_id,
                self.arm,
                &self.arm_release_digest,
                &self.trusted_lease_digest,
                &self.supervisor_executable_digest,
                &self.runner_executable_digest,
                &self.host_policy_digest,
                &self.delegation_root_digest,
            ),
            (
                self.run_root_mount_id,
                self.run_root_device,
                self.run_root_inode,
                &self.run_root_name_digest,
                &self.runner_process_receipt,
                &self.raw,
                self.peak_domain_memory_bytes,
            ),
        ))
    }

    #[cfg(test)]
    pub(crate) fn fixture(
        run_id: P8QualityRunRef,
        arm: P8QualityArmKind,
        arm_release_digest: P8ArmReleaseRef,
    ) -> Self {
        let events = b"low 0\noom 0\noom_kill 0\noom_group_kill 0\n".to_vec();
        let mut value = Self {
            schema: "beetle-memory.p8.trusted-domain-resource-receipt.v1".into(),
            evidence: P8DomainResourceEvidenceV1::ContractFixtureOnlyNoTrustedAuthority,
            run_id,
            arm,
            arm_release_digest,
            trusted_lease_digest: P8TrustedExecutionLeaseRef::derive_for_test("lease"),
            supervisor_executable_digest: P8QualityDigest::derive(
                "p8_resource_fixture",
                &"supervisor",
            ),
            runner_executable_digest: P8QualityDigest::derive("p8_resource_fixture", &"runner"),
            host_policy_digest: P8QualityDigest::derive("p8_resource_fixture", &"host"),
            delegation_root_digest: P8QualityDigest::derive("p8_resource_fixture", &"root"),
            run_root_mount_id: 1,
            run_root_device: 2,
            run_root_inode: 3,
            run_root_name_digest: P8QualityDigest::derive("p8_resource_fixture", &"run-root"),
            runner_process_receipt: None,
            raw: P8CgroupV2RawObservationV1 {
                child_pid: 41,
                run_root_membership_path: "/beetle/p8/run-arm".into(),
                initial_cgroup_procs: Vec::new(),
                barrier_cgroup_procs: b"41\n".to_vec(),
                child_proc_cgroup: b"0::/beetle/p8/run-arm\n".to_vec(),
                observed_process_ids: vec![41],
                membership_trace: vec![b"41\n".to_vec()],
                final_cgroup_procs: Vec::new(),
                memory_peak: b"4096\n".to_vec(),
                memory_events_before: events.clone(),
                memory_events_after: events.clone(),
                memory_events_local_before: events.clone(),
                memory_events_local_after: events,
                final_cgroup_events: b"populated 0\nfrozen 0\n".to_vec(),
                stdout_bytes: 0,
                stdout_digest: P8QualityDigest::derive("p8_resource_fixture", &"stdout"),
                stderr_bytes: 0,
                stderr_digest: P8QualityDigest::derive("p8_resource_fixture", &"stderr"),
            },
            peak_domain_memory_bytes: 4096,
            receipt_digest: P8TrustedDomainResourceReceiptRef::derive(&()),
        };
        value.receipt_digest = value.derived_digest();
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p8_quality::source_release::P8ArmReleaseRef;
    #[cfg(not(target_os = "linux"))]
    use crate::p8_quality::source_release::P8HarnessSourceInputManifestV1;

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_p8_sealed_launcher_fails_before_process_creation() {
        let source = P8HarnessSourceInputManifestV1::fixture(&P8QualityDigest::derive(
            "p8_non_linux_sealed_fixture",
            &"source",
        ));
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical executable");
        let mut launcher = P8SealedProcessLauncher::open(
            source.source_input_digest().clone(),
            P8HarnessExecutableRoleV1::SourcePublisher,
            &executable,
        )
        .expect("retain executable");
        let error = launcher
            .run(
                &[P8_ROLE_SELF_TEST_ARG.to_string()],
                P8SealedProcessLimits {
                    stdout_bytes: 1024,
                    stderr_bytes: 1024,
                    total_bytes: 2048,
                    timeout: Duration::from_secs(1),
                },
            )
            .expect_err("non-Linux cannot produce a sealed process receipt");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        let cgroup_error = launcher
            .run_linux_cgroup_v2_engineering(
                &[P8_ROLE_SELF_TEST_ARG.to_string()],
                Path::new("/definitely-missing-p8-cgroup-run-root"),
                P8SealedProcessLimits {
                    stdout_bytes: 1024,
                    stderr_bytes: 1024,
                    total_bytes: 2048,
                    timeout: Duration::from_secs(1),
                },
                Duration::from_millis(100),
                1,
            )
            .expect_err("non-Linux cgroup authority must fail before opening or spawning");
        assert_eq!(cgroup_error.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn sealed_process_evidence_class_is_not_a_trusted_linux_claim() {
        assert_eq!(
            P8SealedProcessEvidenceClassV1::EngineeringSealedNoTrustedLeaseOrCgroup,
            P8SealedProcessEvidenceClassV1::EngineeringSealedNoTrustedLeaseOrCgroup
        );
        assert_ne!(
            P8_SEALED_PROCESS_RECEIPT_SCHEMA,
            "beetle-memory.p7.sealed-process-receipt.v1"
        );
    }

    #[test]
    fn cgroup_resource_fixture_is_structurally_exact_but_never_trusted() {
        let receipt = P8TrustedDomainResourceReceiptV1::fixture(
            P8QualityRunRef::derive_for_test("run"),
            P8QualityArmKind::FrozenP84Baseline,
            P8ArmReleaseRef::derive_for_test("arm"),
        );
        assert!(receipt.validate_structure().is_empty());
        assert_eq!(receipt.peak_domain_memory_bytes(), 4096);
        assert_eq!(receipt.receipt_digest(), &receipt.derived_digest());
        assert!(receipt
            .trusted_admission_failures()
            .contains(&P8QualityContractFailure::TrustedExecutionMissing));

        let mut oom = receipt.clone();
        oom.raw.memory_events_after = b"low 0\noom 1\noom_kill 0\noom_group_kill 0\n".to_vec();
        oom.receipt_digest = oom.derived_digest();
        assert!(oom
            .validate_structure()
            .contains(&P8QualityContractFailure::TrustedExecutionMissing));

        let mut foreign = receipt;
        foreign.raw.membership_trace = vec![b"41\n99\n".to_vec()];
        foreign.receipt_digest = foreign.derived_digest();
        assert!(foreign
            .validate_structure()
            .contains(&P8QualityContractFailure::TrustedExecutionMissing));
    }
}
