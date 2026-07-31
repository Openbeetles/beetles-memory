#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path = "../bounded_process.rs"]
mod bounded_process;
#[path = "../p8_artifact_dir.rs"]
mod p8_artifact_dir;
#[path = "../p8_process_authority.rs"]
mod p8_process_authority;
#[path = "../p8_semantic.rs"]
mod p8_semantic;
#[path = "../p8_semantic_operator.rs"]
mod p8_semantic_operator;
#[path = "../retained_artifact_fs.rs"]
mod retained_artifact_fs;
#[path = "../sealed_execution.rs"]
mod sealed_execution;

use bounded_process::{run_bounded_command, BoundedProcessLimits, BoundedProcessTermination};
use p8_artifact_dir::{observed_content_identity_for_bytes, P8RetainedArtifactDirectory};
use p8_process_authority::{authorize_internal_child, claim_internal_child_authority};
use p8_semantic::{
    P8ArtifactContractFailure, P8ArtifactLimits, P8SemanticOperatorReportV1,
    P8_SEMANTIC_GATE_EXPECTED_STDOUT, P8_SEMANTIC_GATE_SELF_TEST_ARG,
};
use p8_semantic_operator::run_p8_semantic_operator;

const INTERNAL_VERIFY_ARG: &str = "--p8-internal-verify";

struct OwnedSupervisorStage<'a> {
    parent: &'a P8RetainedArtifactDirectory,
    file: File,
    name: String,
    armed: bool,
}

impl<'a> OwnedSupervisorStage<'a> {
    fn write(
        parent: &'a P8RetainedArtifactDirectory,
        name: &str,
        bytes: &[u8],
    ) -> Result<Self, String> {
        let name = name.to_string();
        let file = parent
            .create_terminal_stage(&name)
            .map_err(|_| "P8 staged report already exists or cannot be created".to_string())?;
        let mut value = Self {
            parent,
            file,
            name,
            armed: true,
        };
        value
            .file
            .write_all(bytes)
            .and_then(|()| value.file.sync_all())
            .map_err(|_| "P8 staged report could not be sealed".to_string())?;
        Ok(value)
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for OwnedSupervisorStage<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.parent.discard_same_file(&self.file, &self.name);
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.len() == 2 && args[1] == OsStr::new(P8_SEMANTIC_GATE_SELF_TEST_ARG) {
        run_gate_contract()?;
        std::io::stdout()
            .write_all(P8_SEMANTIC_GATE_EXPECTED_STDOUT)
            .map_err(|_| "P8 gate self-test stdout could not be sealed".to_string())?;
        return Ok(());
    }
    if args.len() == 4 && args[1] == OsStr::new(INTERNAL_VERIFY_ARG) {
        return run_authorized_internal_verifier(&args[2], &args[3]);
    }
    if args.len() != 4 {
        return Err(
            "usage: bm-p8-semantic-operator <bundle-root> <gate-receipt.json> <operator-report.json>"
                .into(),
        );
    }
    run_public_supervisor(
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    )
}

fn run_public_supervisor(
    bundle_root: &Path,
    gate_receipt_path: &Path,
    report_path: &Path,
) -> Result<(), String> {
    let started = Instant::now();
    let report_parent = report_path
        .parent()
        .ok_or_else(|| "P8 operator report parent is missing".to_string())?;
    let canonical_parent = fs::canonicalize(report_parent)
        .map_err(|_| "P8 operator report parent is unavailable".to_string())?;
    if canonical_parent != report_parent {
        return Err("P8 operator report parent must be canonical".into());
    }
    let report_name = report_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "P8 operator report name must be one UTF-8 component".to_string())?;
    let retained_parent = P8RetainedArtifactDirectory::open(&canonical_parent)
        .map_err(|_| "P8 operator report parent cannot be retained".to_string())?;
    if report_path
        .try_exists()
        .map_err(|_| "P8 operator report path cannot be inspected".to_string())?
    {
        return Err("P8 operator report path already exists".into());
    }
    let staged_name = unique_staged_report_name(report_name)?;
    let staged_path = canonical_parent.join(&staged_name);
    if staged_path
        .try_exists()
        .map_err(|_| "P8 operator staged path cannot be inspected".to_string())?
    {
        return Err("P8 operator staged path already exists".into());
    }

    let executable =
        std::env::current_exe().map_err(|_| "P8 operator executable path is unavailable")?;
    let internal_arguments = [
        OsStr::new(INTERNAL_VERIFY_ARG),
        bundle_root.as_os_str(),
        gate_receipt_path.as_os_str(),
    ];
    let mut child = Command::new(&executable);
    child.args(internal_arguments);
    let parent_identity = authorize_internal_child(&mut child, &internal_arguments)?;
    let output = run_bounded_command(
        &mut child,
        BoundedProcessLimits {
            stdout_bytes: P8ArtifactLimits::V1.control_json_bytes(),
            stderr_bytes: P8ArtifactLimits::V1.control_json_bytes(),
            total_bytes: P8ArtifactLimits::V1
                .control_json_bytes()
                .checked_mul(2)
                .ok_or_else(|| "P8 operator pipe limit overflow".to_string())?,
            timeout: remaining_wall(&started)?,
        },
    )
    .map_err(|error| format!("P8 operator supervisor failed: {error}"))?;

    if output.termination != BoundedProcessTermination::Exited || !output.status.success() {
        return Err(format!(
            "P8 verifier child failed after closed pipes: termination={:?}, status={}, stderr={}",
            output.termination,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !output.stderr.is_empty() {
        return Err("P8 verifier child output contract is invalid".into());
    }
    let report_bytes = output.stdout;
    if report_bytes.is_empty()
        || u64::try_from(report_bytes.len())
            .map_err(|_| "P8 operator report size overflow".to_string())?
            > P8ArtifactLimits::V1.control_json_bytes()
    {
        return Err("P8 verifier child report size is invalid".into());
    }
    let report: P8SemanticOperatorReportV1 = serde_json::from_slice(&report_bytes)
        .map_err(|_| "P8 verifier child report schema is invalid".to_string())?;
    let mut failures = report.validate_contract();
    if report.verifier_identity.source_identity != parent_identity.source_identity
        || report.verifier_identity.build_identity != parent_identity.build_identity
        || report.verifier_identity.executable_identity != parent_identity.executable_identity
    {
        failures.push(P8ArtifactContractFailure::IdentityInvalid);
    }
    failures.sort();
    failures.dedup();
    if !failures.is_empty() {
        return Err(format!(
            "P8 verifier child report failed parent validation: {failures:?}"
        ));
    }
    if report
        .mismatches()
        .iter()
        .any(|failure| *failure != P8ArtifactContractFailure::QualityThresholdsNotFrozen)
    {
        return Err(format!(
            "P8 semantic artifacts failed independent verification: {:?}",
            report.mismatches()
        ));
    }
    let mut staged_report =
        OwnedSupervisorStage::write(&retained_parent, &staged_name, &report_bytes)?;
    let staged_content_identity = observed_content_identity_for_bytes(
        &staged_report.file,
        &report_bytes,
        P8ArtifactLimits::V1.control_json_bytes(),
    )
    .map_err(|_| "P8 staged report content identity is unavailable".to_string())?;
    let commit = retained_parent.install_file_no_replace_terminal(
        &staged_report.file,
        &staged_name,
        report_name,
        &staged_content_identity,
        P8ArtifactLimits::V1.control_json_bytes(),
        || {
            ensure_wall(&started)
                .map_err(|message| std::io::Error::new(std::io::ErrorKind::TimedOut, message))
        },
    );
    match commit {
        Ok(()) => {
            staged_report.disarm();
            Ok(())
        }
        Err(error) => {
            if error.kind() == std::io::ErrorKind::TimedOut {
                Err("P8 operator exceeded the 30 minute command wall".to_string())
            } else {
                Err("P8 operator terminal no-replace commit failed".to_string())
            }
        }
    }
}

fn run_authorized_internal_verifier(
    bundle_root: &OsStr,
    gate_receipt_path: &OsStr,
) -> Result<(), String> {
    let child_identity = claim_internal_child_authority()?;
    let report = run_p8_semantic_operator(
        Path::new(bundle_root),
        Path::new(gate_receipt_path),
        child_identity.clone(),
    )
    .map_err(|failures| format!("operator verification failed before report: {failures:?}"))?;
    if report.verifier_identity != child_identity {
        return Err("P8 verifier report identity differs from claimed child authority".into());
    }
    let validation = report.validate_contract();
    if !validation.is_empty() {
        return Err(format!(
            "P8 verifier report failed semantic validation: {validation:?}"
        ));
    }
    if report
        .mismatches()
        .iter()
        .any(|failure| *failure != P8ArtifactContractFailure::QualityThresholdsNotFrozen)
    {
        return Err(format!(
            "P8 semantic artifacts failed independent verification: {:?}",
            report.mismatches()
        ));
    }
    let report_bytes = serde_json::to_vec(&report)
        .map_err(|_| "P8 operator report serialization failed".to_string())?;
    if u64::try_from(report_bytes.len()).map_err(|_| "operator report size overflow".to_string())?
        > P8ArtifactLimits::V1.control_json_bytes()
    {
        return Err("operator report exceeds the control artifact limit".into());
    }
    let mut stdout = std::io::stdout();
    stdout
        .write_all(&report_bytes)
        .and_then(|()| stdout.flush())
        .map_err(|_| "P8 internal report bytes could not be sealed".to_string())
}

fn run_gate_contract() -> Result<(), String> {
    p8_semantic_operator::run_p8_gate_contract()
}

fn unique_staged_report_name(report_name: &str) -> Result<String, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "P8 staged report clock is before the Unix epoch".to_string())?
        .as_nanos();
    Ok(format!(
        "{report_name}.p8-staged.{}.{nonce}",
        std::process::id()
    ))
}

fn remaining_wall(started: &Instant) -> Result<Duration, String> {
    Duration::from_millis(P8ArtifactLimits::V1.operator_wall_millis())
        .checked_sub(started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| "P8 operator exceeded the 30 minute command wall".to_string())
}

fn ensure_wall(started: &Instant) -> Result<(), String> {
    remaining_wall(started).map(|_| ())
}

#[cfg(test)]
mod terminal_stage_tests {
    use super::*;

    #[test]
    fn p8_supervisor_stage_guard_discards_before_successful_commit() {
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp root")
            .join(format!(
                "bm-p8-internal-stage-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("wall clock")
                    .as_nanos()
            ));
        fs::create_dir(&root).expect("create stage root");
        let retained = P8RetainedArtifactDirectory::open(&root).expect("retain stage root");
        {
            let _guard = OwnedSupervisorStage::write(&retained, "report.stage", b"report")
                .expect("write guarded stage");
        }
        assert!(!root.join("report.stage").exists());

        let mut handed_off = OwnedSupervisorStage::write(&retained, "handoff.stage", b"report")
            .expect("write handoff stage");
        handed_off.disarm();
        drop(handed_off);
        assert_eq!(
            fs::read(root.join("handoff.stage")).expect("read handed-off stage"),
            b"report"
        );
        fs::remove_dir_all(root).expect("remove stage root");
    }
}
