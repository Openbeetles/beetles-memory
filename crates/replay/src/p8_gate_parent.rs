//! Parent-owned P8 gate process and sealed command receipt.
//!
//! The child never constructs or emits the receipt. This owner waits for process exit and both
//! output pipes to reach EOF before it writes bounded sidecars and commits the receipt last.

use std::fs::{self, File};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::bounded_process::{run_bounded_command, BoundedProcessLimits};
use crate::p8_artifact_dir::{observed_content_identity_for_bytes, P8RetainedArtifactDirectory};
use crate::p8_semantic::{
    P8ArtifactContractFailure, P8ArtifactLimits, P8ClosedChildObservation, P8GateCommandReceiptV1,
    P8VerifierIdentityV1, P8_SEMANTIC_GATE_EXPECTED_STDOUT, P8_SEMANTIC_GATE_SELF_TEST_ARG,
};

#[derive(Clone, Debug)]
pub struct P8GateParentCommand {
    pub cwd: PathBuf,
    pub verifier_executable: PathBuf,
    pub receipt_path: PathBuf,
}

struct OwnedGateStage<'a> {
    parent: &'a P8RetainedArtifactDirectory,
    file: File,
    staged_name: String,
    final_name: String,
    content_identity: crate::p8_artifact_dir::P8ObservedContentIdentity,
    committed: bool,
    armed: bool,
}

impl<'a> OwnedGateStage<'a> {
    fn create(
        parent: &'a P8RetainedArtifactDirectory,
        final_name: &str,
        bytes: &[u8],
        limit: u64,
    ) -> Result<Self, Vec<P8ArtifactContractFailure>> {
        let staged_name = unique_staged_receipt_name(final_name)?;
        let mut file = parent
            .create_terminal_stage(&staged_name)
            .map_err(|_| vec![P8ArtifactContractFailure::DuplicateArtifact])?;
        let initialization = (|| {
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
            file.rewind()
                .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
            let mut observed = Vec::with_capacity(bytes.len());
            std::io::Read::by_ref(&mut file)
                .take(
                    u64::try_from(bytes.len())
                        .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?
                        .checked_add(1)
                        .ok_or_else(|| vec![P8ArtifactContractFailure::ArithmeticOverflow])?,
                )
                .read_to_end(&mut observed)
                .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
            if observed != bytes {
                return Err(vec![P8ArtifactContractFailure::ReceiptInvalid]);
            }
            observed_content_identity_for_bytes(&file, bytes, limit)
                .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])
        })();
        let content_identity = match initialization {
            Ok(identity) => identity,
            Err(failures) => {
                let _ = parent.discard_same_file(&file, &staged_name);
                return Err(failures);
            }
        };
        Ok(Self {
            parent,
            file,
            staged_name,
            final_name: final_name.to_string(),
            content_identity,
            committed: false,
            armed: true,
        })
    }

    fn commit(
        &mut self,
        content_limit: u64,
        verify_deadline: impl FnMut() -> std::io::Result<()>,
    ) -> Result<(), Vec<P8ArtifactContractFailure>> {
        self.parent
            .install_file_no_replace_terminal(
                &self.file,
                &self.staged_name,
                &self.final_name,
                &self.content_identity,
                content_limit,
                verify_deadline,
            )
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::TimedOut {
                    vec![P8ArtifactContractFailure::OperatorWallTimeExceeded]
                } else {
                    vec![P8ArtifactContractFailure::ArtifactIoFailure]
                }
            })?;
        self.committed = true;
        Ok(())
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for OwnedGateStage<'_> {
    fn drop(&mut self) {
        if self.armed {
            let current_name = if self.committed {
                &self.final_name
            } else {
                &self.staged_name
            };
            let _ = self.parent.discard_same_file(&self.file, current_name);
        }
    }
}

pub fn run_p8_gate_parent(
    command: P8GateParentCommand,
) -> Result<P8GateCommandReceiptV1, Vec<P8ArtifactContractFailure>> {
    let started = Instant::now();
    let receipt_parent = command
        .receipt_path
        .parent()
        .ok_or_else(|| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
    let canonical_parent = fs::canonicalize(receipt_parent)
        .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
    if canonical_parent != receipt_parent {
        return Err(vec![P8ArtifactContractFailure::ArtifactIoFailure]);
    }
    let receipt_name = command
        .receipt_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
    let retained_parent = P8RetainedArtifactDirectory::open(&canonical_parent)
        .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
    let stdout_path = sidecar_path(&command.receipt_path, "stdout");
    let stderr_path = sidecar_path(&command.receipt_path, "stderr");
    for target in [&command.receipt_path, &stdout_path, &stderr_path] {
        if target
            .try_exists()
            .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?
        {
            return Err(vec![P8ArtifactContractFailure::DuplicateArtifact]);
        }
    }

    let verifier_identity = P8VerifierIdentityV1::for_executable(&command.verifier_executable)?;
    let output_limit = P8ArtifactLimits::V1.control_json_bytes();
    let output = run_bounded_command(
        Command::new(&command.verifier_executable)
            .arg(P8_SEMANTIC_GATE_SELF_TEST_ARG)
            .current_dir(&command.cwd),
        BoundedProcessLimits {
            stdout_bytes: output_limit,
            stderr_bytes: output_limit,
            total_bytes: output_limit
                .checked_mul(2)
                .ok_or_else(|| vec![P8ArtifactContractFailure::ArithmeticOverflow])?,
            timeout: remaining_wall(&started)?,
        },
    )
    .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?;
    if !output.succeeded()
        || output.stdout.as_slice() != P8_SEMANTIC_GATE_EXPECTED_STDOUT
        || !output.stderr.is_empty()
    {
        return Err(vec![P8ArtifactContractFailure::ReceiptInvalid]);
    }
    let verifier_identity_after =
        P8VerifierIdentityV1::for_executable(&command.verifier_executable)?;
    if verifier_identity_after != verifier_identity {
        return Err(vec![P8ArtifactContractFailure::IdentityInvalid]);
    }

    let receipt = P8GateCommandReceiptV1::from_parent_observation(
        &verifier_identity,
        P8ClosedChildObservation {
            cwd: &command.cwd,
            exit_code: output.status.code().unwrap_or(-1),
            closed_stdout: &output.stdout,
            closed_stderr: &output.stderr,
        },
    )?;
    let trusted_source_root = crate::p8_semantic::p8_trusted_source_root()?;
    let receipt_failures = receipt.validate_contract(&verifier_identity, &trusted_source_root);
    if !receipt_failures.is_empty() {
        return Err(receipt_failures);
    }
    let receipt_bytes = serde_json::to_vec(&receipt)
        .map_err(|_| vec![P8ArtifactContractFailure::SchemaMismatch])?;
    if u64::try_from(receipt_bytes.len())
        .map_err(|_| vec![P8ArtifactContractFailure::ArithmeticOverflow])?
        > P8ArtifactLimits::V1.control_json_bytes()
    {
        return Err(vec![P8ArtifactContractFailure::ArtifactLimitExceeded]);
    }
    let stdout_name = format!("{receipt_name}.stdout");
    let stderr_name = format!("{receipt_name}.stderr");
    let mut stdout_stage =
        OwnedGateStage::create(&retained_parent, &stdout_name, &output.stdout, output_limit)?;
    let mut stderr_stage =
        OwnedGateStage::create(&retained_parent, &stderr_name, &output.stderr, output_limit)?;
    let mut receipt_stage =
        OwnedGateStage::create(&retained_parent, receipt_name, &receipt_bytes, output_limit)?;
    let deadline = || {
        remaining_wall(&started).map(|_| ()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "P8 gate command wall elapsed")
        })
    };
    stdout_stage.commit(output_limit, deadline)?;
    stderr_stage.commit(output_limit, deadline)?;
    receipt_stage.commit(output_limit, deadline)?;
    stdout_stage.disarm();
    stderr_stage.disarm();
    receipt_stage.disarm();
    Ok(receipt)
}

fn remaining_wall(started: &Instant) -> Result<Duration, Vec<P8ArtifactContractFailure>> {
    Duration::from_millis(P8ArtifactLimits::V1.operator_wall_millis())
        .checked_sub(started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| vec![P8ArtifactContractFailure::OperatorWallTimeExceeded])
}

fn unique_staged_receipt_name(
    receipt_name: &str,
) -> Result<String, Vec<P8ArtifactContractFailure>> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| vec![P8ArtifactContractFailure::ArtifactIoFailure])?
        .as_nanos();
    Ok(format!(
        "{receipt_name}.p8-staged.{}.{nonce}",
        std::process::id()
    ))
}

fn sidecar_path(receipt_path: &Path, suffix: &str) -> PathBuf {
    let mut value = receipt_path.as_os_str().to_os_string();
    value.push(".");
    value.push(suffix);
    value.into()
}
