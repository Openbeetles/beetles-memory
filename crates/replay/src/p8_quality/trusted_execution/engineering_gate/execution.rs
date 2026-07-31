use std::{collections::BTreeMap, io, os::fd::AsRawFd as _, path::Path};

use super::*;
use crate::{
    bounded_process::{
        run_bounded_command_closed, BoundedProcessLimits, BoundedProcessTermination,
        ClosedBoundedProcess,
    },
    p8_quality::source_release::P8HarnessExecutableRoleV1,
    p8_quality::trusted_execution::supervisor_session::{
        csprng_session_nonce, directory_identity_digest, P8ImmutableRootMutationWitness,
        P8TrustedSupervisorInputs,
    },
    p8_quality::trusted_execution::{
        run_retained_harness_role_self_test, P8SealedProcessReceiptV1,
    },
    sealed_execution::{SealedContentIdentity, SealedExecutionDomain},
};

const TOOL_VERSION_STDOUT_BYTES: u64 = 64 * 1024;
const TOOL_VERSION_STDERR_BYTES: u64 = 64 * 1024;
const TOOL_VERSION_TOTAL_BYTES: u64 = 128 * 1024;

/// 唯一能把四个真实 gate 闭包升级为后续 StageReady 的 authority。
///
/// 它故意不实现 Clone/Serialize/Deserialize，也不提供 raw receipt 到 authority 的反向构造。
pub(crate) struct VerifiedP8EngineeringGateSet {
    receipt: P8EngineeringGateReceiptV1,
    role_receipts: Vec<P8SealedProcessReceiptV1>,
    trusted_inputs: P8TrustedSupervisorInputs,
}

impl VerifiedP8EngineeringGateSet {
    pub(crate) fn receipt(&self) -> &P8EngineeringGateReceiptV1 {
        &self.receipt
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        P8EngineeringGateReceiptV1,
        Vec<P8SealedProcessReceiptV1>,
        P8TrustedSupervisorInputs,
    ) {
        (self.receipt, self.role_receipts, self.trusted_inputs)
    }
}

pub(crate) fn execute_trusted_gate_set(
    mut inputs: P8TrustedSupervisorInputs,
) -> io::Result<VerifiedP8EngineeringGateSet> {
    inputs.execution.verify()?;
    inputs.verify_source_exact_and_quiet()?;
    inputs.verify_dependency_roots_exact_and_quiet()?;

    let toolchain = observe_exact_toolchain(&mut inputs)?;
    inputs.verify_source_exact_and_quiet()?;
    inputs.verify_dependency_roots_exact_and_quiet()?;

    // Candidate role self-tests run before any gate target is populated. Every real gate then
    // rechecks exact source/dependency inputs, requires an initially empty retained target, and
    // produces the final target receipt after all untrusted candidate-role execution is over.
    let mut role_receipts = Vec::with_capacity(P8HarnessExecutableRoleV1::ALL.len());
    for role in P8HarnessExecutableRoleV1::ALL {
        let timeout = inputs.parent_channel.remaining_time()?;
        let executable = inputs
            .role_executables
            .get_mut(&role)
            .ok_or_else(|| invalid_data("P8 harness role executable is missing"))?;
        role_receipts.push(run_retained_harness_role_self_test(
            inputs.source_input.source_input_digest().clone(),
            role,
            executable,
            BoundedProcessLimits {
                stdout_bytes: inputs.stdout_bytes,
                stderr_bytes: inputs.stderr_bytes,
                total_bytes: inputs.total_bytes,
                timeout,
            },
        )?);
    }
    inputs.verify_source_exact_and_quiet()?;
    inputs.verify_dependency_roots_exact_and_quiet()?;

    let supervisor_session_nonce = inputs.admitted_session.audit_binding().1.clone();
    let mut command_receipts = Vec::with_capacity(P8EngineeringGateIdV1::ALL.len());
    for gate in P8EngineeringGateIdV1::ALL {
        command_receipts.push(execute_gate(
            &mut inputs,
            gate,
            &supervisor_session_nonce,
            &toolchain,
        )?);
    }

    inputs.verify_source_exact_and_quiet()?;
    inputs.verify_dependency_roots_exact_and_quiet()?;
    inputs.execution.verify()?;

    let mut receipt = P8EngineeringGateReceiptV1 {
        schema: P8_ENGINEERING_GATE_RECEIPT_SCHEMA.into(),
        evidence: P8EngineeringGateEvidenceV1::LinuxTrustedSupervisorClosedProcess,
        source_input_digest: inputs.source_input.source_input_digest().clone(),
        supervisor_session_nonce,
        command_receipts,
        receipt_digest: P8EngineeringGateReceiptRef::derive(&()),
    };
    receipt.receipt_digest = receipt.derived_digest();
    if !receipt.validate_against(&inputs.source_input).is_empty() {
        return Err(invalid_data(
            "P8 verified engineering gate-set failed self-validation",
        ));
    }

    Ok(VerifiedP8EngineeringGateSet {
        receipt,
        role_receipts,
        trusted_inputs: inputs,
    })
}

fn observe_exact_toolchain(
    inputs: &mut P8TrustedSupervisorInputs,
) -> io::Result<P8EngineeringToolchainObservationV1> {
    let mut tools = Vec::with_capacity(P8EngineeringToolRoleV1::ALL.len());
    for role in P8EngineeringToolRoleV1::ALL {
        let timeout = inputs.parent_channel.remaining_time()?;
        let executable = inputs
            .tool_executables
            .get_mut(&role)
            .ok_or_else(|| invalid_data("P8 engineering tool descriptor is missing"))?;
        let version_args = match role {
            P8EngineeringToolRoleV1::RustLld => {
                vec!["-flavor".into(), "gnu".into(), "--version".into()]
            }
            _ => vec!["--version".into()],
        };
        let prepared = executable.prepare_unclaimed_with_linux_exact_environment(
            engineering_tool_domain(role),
            &version_args,
            &[],
            Vec::new(),
        )?;
        let (command, guard, identity) = prepared.into_parts();
        let output = run_bounded_command_closed(
            command,
            BoundedProcessLimits {
                stdout_bytes: TOOL_VERSION_STDOUT_BYTES,
                stderr_bytes: TOOL_VERSION_STDERR_BYTES,
                total_bytes: TOOL_VERSION_TOTAL_BYTES,
                timeout,
            },
        );
        drop(guard);
        let output = output?;
        executable.verify_content(&identity)?;
        require_successful_closed_process(&output, "P8 engineering tool version")?;
        if output.stdout().is_empty() {
            return Err(invalid_data("P8 engineering tool version stdout is empty"));
        }
        tools.push(P8EngineeringToolIdentityV1 {
            role,
            executable_byte_len: identity.byte_len(),
            executable_digest: sealed_identity_digest(&identity)?,
            version_child_pid: output.pid(),
            version_exit_code: output.status().code(),
            version_stdout_byte_len: exact_len(output.stdout())?,
            version_stdout_digest: P8QualityDigest::derive(
                "p8_engineering_tool_version_stdout_v1",
                &output.stdout(),
            ),
            version_stdout_eof_observed: output.stdout_eof_observed(),
            version_stderr_byte_len: exact_len(output.stderr())?,
            version_stderr_digest: P8QualityDigest::derive(
                "p8_engineering_tool_version_stderr_v1",
                &output.stderr(),
            ),
            version_stderr_eof_observed: output.stderr_eof_observed(),
        });
    }
    let mut observation = P8EngineeringToolchainObservationV1 {
        tools,
        observation_digest: P8QualityDigest::derive("p8_engineering_toolchain_observation_v1", &()),
    };
    observation.observation_digest = observation.derived_digest();
    if !observation.validate_contract().is_empty() {
        return Err(invalid_data(
            "P8 engineering toolchain observation failed self-validation",
        ));
    }
    Ok(observation)
}

fn execute_gate(
    inputs: &mut P8TrustedSupervisorInputs,
    gate: P8EngineeringGateIdV1,
    supervisor_session_nonce: &P8QualityDigest,
    toolchain: &P8EngineeringToolchainObservationV1,
) -> io::Result<P8EngineeringGateCommandReceiptV1> {
    let source_before = inputs.verify_source_exact_and_quiet()?;
    inputs.verify_dependency_roots_exact_and_quiet()?;

    let (
        target_path,
        target_identity,
        initial_inventory_digest,
        initial_entry_count,
        target_capability,
    ) = {
        let target = inputs
            .target_roots
            .get(&gate)
            .ok_or_else(|| invalid_data("P8 engineering gate target is missing"))?;
        target.verify_unchanged()?;
        let retained_target_path = target.retained_observation_path();
        let watched_target_path = target.path().to_path_buf();
        let (initial_inventory_digest, initial_entry_count) =
            observe_target_inventory_exact(&watched_target_path, &retained_target_path)?;
        target.verify_unchanged()?;
        (
            retained_target_path,
            directory_identity_digest(target)?,
            initial_inventory_digest,
            initial_entry_count,
            target.inheritable_directory_file()?,
        )
    };
    if initial_entry_count != 0 || initial_inventory_digest != empty_target_inventory_digest() {
        return Err(invalid_data(
            "P8 engineering gate target is not initially empty",
        ));
    }

    let command_spec = P8EngineeringGateCommandSpecV1::canonical(gate);
    let mut inherited_files = Vec::new();
    let source_fd = push_directory_capability(
        &mut inherited_files,
        inputs.source_root.inheritable_directory_file()?,
    );
    let target_fd = push_directory_capability(&mut inherited_files, target_capability);
    let sysroot_fd = push_directory_capability(
        &mut inherited_files,
        inputs.rust_sysroot_root.inheritable_directory_file()?,
    );
    let cache_fd = push_directory_capability(
        &mut inherited_files,
        inputs
            .cargo_dependency_cache_root
            .inheritable_directory_file()?,
    );
    let mut tool_fds = BTreeMap::new();
    for role in &command_spec.required_tools {
        if *role == command_spec.launcher_tool && *role != P8EngineeringToolRoleV1::Cargo {
            continue;
        }
        let file = inputs
            .tool_executables
            .get(role)
            .ok_or_else(|| invalid_data("P8 engineering tool capability is missing"))?
            .inheritable_duplicate()?;
        let fd = push_directory_capability(&mut inherited_files, file);
        if tool_fds.insert(*role, fd).is_some() {
            return Err(invalid_data("P8 engineering tool capability is aliased"));
        }
    }

    let attempt_nonce = P8QualityDigest::derive(
        "p8_engineering_gate_attempt_nonce_v1",
        &csprng_session_nonce()?.to_vec(),
    );
    let exact_environment = exact_gate_environment(
        &inputs.source_input,
        gate,
        &attempt_nonce,
        GateEnvironmentCapabilities {
            source_fd,
            target_fd,
            sysroot_fd,
            cache_fd,
            tool_fds: &tool_fds,
        },
    )?;
    let launcher = inputs
        .tool_executables
        .get_mut(&command_spec.launcher_tool)
        .ok_or_else(|| invalid_data("P8 engineering gate launcher is missing"))?;
    let prepared = launcher.prepare_unclaimed_with_linux_exact_environment(
        engineering_tool_domain(command_spec.launcher_tool),
        &command_spec.argv[1..],
        &exact_environment,
        inherited_files,
    )?;
    let (mut command, guard, launcher_identity) = prepared.into_parts();
    command.current_dir(proc_fd_path(source_fd));
    let timeout = inputs.parent_channel.remaining_time()?;
    let output = run_bounded_command_closed(
        command,
        BoundedProcessLimits {
            stdout_bytes: inputs.stdout_bytes,
            stderr_bytes: inputs.stderr_bytes,
            total_bytes: inputs.total_bytes,
            timeout,
        },
    );
    drop(guard);
    let output = output?;
    launcher.verify_content(&launcher_identity)?;
    require_successful_closed_process(&output, "P8 engineering gate")?;

    let source_after = inputs.verify_source_exact_and_quiet()?;
    inputs.verify_dependency_roots_exact_and_quiet()?;
    if source_before != source_after {
        return Err(invalid_data("P8 engineering source drifted across gate"));
    }
    let target = inputs
        .target_roots
        .get(&gate)
        .ok_or_else(|| invalid_data("P8 engineering gate target disappeared"))?;
    target.verify_unchanged()?;
    let (final_inventory_digest, final_entry_count) =
        observe_target_inventory_exact(target.path(), &target_path)?;
    target.verify_unchanged()?;
    let target_observation = P8EngineeringTargetObservationV1 {
        root_identity_digest: target_identity,
        initial_inventory_digest,
        initial_entry_count,
        final_inventory_digest,
        final_entry_count,
    };
    let source = P8EngineeringSourceObservationV1 {
        source_input_digest: inputs.source_input.source_input_digest().clone(),
        retained_source_lease_digest: source_before.retained_source_lease_digest,
        physical_identity_digest: source_before.physical_identity_digest,
        inventory_before_digest: source_before.inventory_digest,
        inventory_after_digest: source_after.inventory_digest,
    };
    let mut receipt = P8EngineeringGateCommandReceiptV1 {
        schema: P8_ENGINEERING_GATE_COMMAND_RECEIPT_SCHEMA.into(),
        evidence: P8EngineeringGateEvidenceV1::LinuxTrustedSupervisorClosedProcess,
        supervisor_session_nonce: supervisor_session_nonce.clone(),
        attempt_nonce,
        command_spec,
        source,
        toolchain: toolchain.clone(),
        build_plan: P8EngineeringBuildPlanV1::from_source(&inputs.source_input),
        target: target_observation,
        exact_argv_digest: P8QualityDigest::derive(
            "p8_engineering_gate_exact_argv_v1",
            &gate.exact_argv(),
        ),
        exact_environment,
        exact_environment_digest: P8QualityDigest::derive(
            "p8_engineering_gate_exact_environment_v1",
            &(),
        ),
        parent_pid: std::process::id(),
        child_pid: output.pid(),
        termination: P8EngineeringProcessTerminationV1::Exited,
        exit_code: output.status().code(),
        stdout: closed_pipe_observation("p8_engineering_gate_stdout_v1", output.stdout())?,
        stderr: closed_pipe_observation("p8_engineering_gate_stderr_v1", output.stderr())?,
        elapsed_millis: u64::try_from(output.elapsed().as_millis())
            .map_err(|_| invalid_data("P8 engineering gate elapsed time overflow"))?,
        maximum_rss_bytes: output.maximum_rss_bytes(),
        receipt_digest: P8EngineeringGateCommandReceiptRef::derive(&()),
    };
    receipt.exact_environment_digest = receipt.derived_environment_digest();
    receipt.receipt_digest = receipt.derived_digest();
    if !receipt.validate_against(&inputs.source_input).is_empty() {
        return Err(invalid_data(
            "P8 engineering gate receipt failed self-validation",
        ));
    }
    Ok(receipt)
}

struct GateEnvironmentCapabilities<'a> {
    source_fd: i32,
    target_fd: i32,
    sysroot_fd: i32,
    cache_fd: i32,
    tool_fds: &'a BTreeMap<P8EngineeringToolRoleV1, i32>,
}

fn exact_gate_environment(
    source: &P8HarnessSourceInputManifestV1,
    gate: P8EngineeringGateIdV1,
    attempt_nonce: &P8QualityDigest,
    capabilities: GateEnvironmentCapabilities<'_>,
) -> io::Result<Vec<(String, String)>> {
    let GateEnvironmentCapabilities {
        source_fd,
        target_fd,
        sysroot_fd,
        cache_fd,
        tool_fds,
    } = capabilities;
    let tool_path = |role| {
        tool_fds
            .get(&role)
            .copied()
            .map(proc_fd_path)
            .ok_or_else(|| invalid_data("P8 engineering environment tool is missing"))
    };
    let target_triple = source.target_triple().as_str();
    let sysroot = proc_fd_path(sysroot_fd);
    let mut environment = vec![
        ("CARGO_BUILD_TARGET".into(), target_triple.to_string()),
        ("CARGO_CACHE_RUSTC_INFO".into(), "0".into()),
        ("CARGO_HOME".into(), proc_fd_path(cache_fd)),
        ("CARGO_NET_OFFLINE".into(), "true".into()),
        ("CARGO_TARGET_DIR".into(), proc_fd_path(target_fd)),
        (
            "P8_GATE_ATTEMPT_NONCE".into(),
            attempt_nonce.as_str().to_string(),
        ),
        ("RUST_SYSROOT".into(), sysroot.clone()),
        ("SOURCE_ROOT".into(), proc_fd_path(source_fd)),
    ];
    if tool_fds.contains_key(&P8EngineeringToolRoleV1::RustLld) {
        let linker = tool_path(P8EngineeringToolRoleV1::RustLld)?;
        environment.push((
            format!(
                "CARGO_TARGET_{}_LINKER",
                target_triple.replace('-', "_").to_ascii_uppercase()
            ),
            linker.clone(),
        ));
        environment.push((
            "RUSTFLAGS".into(),
            format!("-C linker={linker} -C linker-flavor=gnu-lld --sysroot={sysroot}"),
        ));
    }
    if tool_fds.contains_key(&P8EngineeringToolRoleV1::Cargo) {
        environment.push(("CARGO".into(), tool_path(P8EngineeringToolRoleV1::Cargo)?));
    }
    if tool_fds.contains_key(&P8EngineeringToolRoleV1::Rustc) {
        environment.push(("RUSTC".into(), tool_path(P8EngineeringToolRoleV1::Rustc)?));
    }
    if tool_fds.contains_key(&P8EngineeringToolRoleV1::Rustdoc) {
        environment.push((
            "RUSTDOC".into(),
            tool_path(P8EngineeringToolRoleV1::Rustdoc)?,
        ));
        environment.push(("RUSTDOCFLAGS".into(), format!("--sysroot={sysroot}")));
    }
    if tool_fds.contains_key(&P8EngineeringToolRoleV1::Rustfmt) {
        environment.push((
            "RUSTFMT".into(),
            tool_path(P8EngineeringToolRoleV1::Rustfmt)?,
        ));
    }
    if gate == P8EngineeringGateIdV1::Clippy {
        environment.push((
            "RUSTC_WORKSPACE_WRAPPER".into(),
            tool_path(P8EngineeringToolRoleV1::ClippyDriver)?,
        ));
    }
    environment.sort_by(|left, right| left.0.cmp(&right.0));
    if environment.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(invalid_data(
            "P8 engineering exact environment contains duplicate keys",
        ));
    }
    Ok(environment)
}

fn engineering_tool_domain(role: P8EngineeringToolRoleV1) -> SealedExecutionDomain {
    let argv0 = match role {
        P8EngineeringToolRoleV1::Cargo => "cargo",
        P8EngineeringToolRoleV1::Rustc => "rustc",
        P8EngineeringToolRoleV1::Rustdoc => "rustdoc",
        P8EngineeringToolRoleV1::Rustfmt => "rustfmt",
        P8EngineeringToolRoleV1::CargoFmt => "cargo-fmt",
        P8EngineeringToolRoleV1::CargoClippy => "cargo-clippy",
        P8EngineeringToolRoleV1::ClippyDriver => "clippy-driver",
        P8EngineeringToolRoleV1::RustLld => "rust-lld",
    };
    SealedExecutionDomain::new(
        "bm-p8-engineering-tool",
        argv0,
        "BM_P8_ENGINEERING_TOOL_FD",
        "BM_P8_ENGINEERING_TOOL_LOCATOR",
        "BM_P8_ENGINEERING_TOOL_SHA256",
        &["BM_P8_ENGINEERING_TOOL_"],
    )
}

fn require_successful_closed_process(
    output: &ClosedBoundedProcess,
    owner: &'static str,
) -> io::Result<()> {
    if output.termination() != BoundedProcessTermination::Exited
        || !output.status().success()
        || !output.stdout_eof_observed()
        || !output.stderr_eof_observed()
    {
        return Err(io::Error::other(format!(
            "{owner} did not close with exit zero and two EOF observations"
        )));
    }
    Ok(())
}

fn closed_pipe_observation(
    domain: &'static str,
    bytes: &[u8],
) -> io::Result<P8ClosedPipeObservationV1> {
    Ok(P8ClosedPipeObservationV1 {
        byte_len: exact_len(bytes)?,
        content_digest: P8QualityDigest::derive(domain, &bytes),
        eof_observed: true,
        truncated: false,
    })
}

fn sealed_identity_digest(identity: &SealedContentIdentity) -> io::Result<P8QualityDigest> {
    P8QualityDigest::parse(format!("sha256:{}", identity.sha256()))
        .map_err(|_| invalid_data("P8 sealed engineering tool digest is invalid"))
}

fn push_directory_capability(files: &mut Vec<std::fs::File>, file: std::fs::File) -> i32 {
    let fd = file.as_raw_fd();
    files.push(file);
    fd
}

fn proc_fd_path(fd: i32) -> String {
    format!("/proc/self/fd/{fd}")
}

fn observe_target_inventory(root: &Path) -> io::Result<(P8QualityDigest, u64)> {
    use sha2::Digest as _;

    fn visit(
        root: &Path,
        path: &Path,
        entries: &mut Vec<(String, String, u64, String)>,
    ) -> io::Result<()> {
        let mut children = std::fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(std::fs::DirEntry::path);
        for child in children {
            let path = child.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            let relative = path
                .strip_prefix(root)
                .ok()
                .and_then(Path::to_str)
                .map(|value| value.replace('\\', "/"))
                .ok_or_else(|| invalid_data("P8 target inventory path is invalid"))?;
            if metadata.file_type().is_symlink() {
                return Err(invalid_data("P8 target inventory contains a symlink"));
            }
            if metadata.is_dir() {
                entries.push((relative, "directory".into(), 0, String::new()));
                visit(root, &path, entries)?;
            } else if metadata.is_file() {
                let bytes = crate::build_support::read_regular_file_stable(&path)
                    .map_err(|_| invalid_data("P8 target file drifted during inventory"))?;
                entries.push((
                    relative,
                    "file".into(),
                    exact_len(&bytes)?,
                    format!("{:x}", sha2::Sha256::digest(&bytes)),
                ));
            } else {
                return Err(invalid_data(
                    "P8 target inventory contains a non-regular entry",
                ));
            }
        }
        Ok(())
    }

    let mut entries = Vec::new();
    visit(root, root, &mut entries)?;
    let count = u64::try_from(entries.len())
        .map_err(|_| invalid_data("P8 target inventory entry count overflow"))?;
    let digest = if entries.is_empty() {
        empty_target_inventory_digest()
    } else {
        P8QualityDigest::derive("p8_engineering_target_inventory_v1", &entries)
    };
    Ok((digest, count))
}

fn observe_target_inventory_exact(
    watched_root: &Path,
    retained_observation_root: &Path,
) -> io::Result<(P8QualityDigest, u64)> {
    let mut witness = P8ImmutableRootMutationWitness::establish(watched_root)?;
    let observation = observe_target_inventory(retained_observation_root)?;
    witness.verify_quiet()?;
    Ok(observation)
}

fn exact_len(bytes: &[u8]) -> io::Result<u64> {
    u64::try_from(bytes.len()).map_err(|_| invalid_data("P8 byte length overflow"))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
