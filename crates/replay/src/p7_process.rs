use std::{
    io,
    process::{Command, ExitStatus},
    time::Duration,
};

use crate::bounded_process::{
    run_bounded_command, BoundedProcessLimits, BoundedProcessTermination,
};
use crate::p7_secure_fs::P7RetainedFile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P7ProcessLimits {
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub total_bytes: u64,
    pub timeout: Duration,
}

impl P7ProcessLimits {
    pub fn control_json() -> Self {
        Self {
            stdout_bytes: 64 * 1024 * 1024,
            stderr_bytes: 16 * 1024 * 1024,
            total_bytes: 80 * 1024 * 1024,
            timeout: Duration::from_secs(5 * 60),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P7ProcessTermination {
    Exited,
    TimedOut,
    StdoutLimitExceeded,
    StderrLimitExceeded,
    TotalLimitExceeded,
}

#[derive(Debug)]
pub struct P7ProcessOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub termination: P7ProcessTermination,
    pub elapsed: Duration,
    pub receipt: P7ProcessReceipt,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct P7ProcessReceipt {
    pub schema_version: String,
    pub sealed_executable_sha256: Option<String>,
    pub pid: u32,
    pub process_group: i64,
    pub maximum_rss_bytes: u64,
    pub elapsed_millis: u64,
}

impl P7ProcessOutput {
    pub fn succeeded(&self) -> bool {
        self.termination == P7ProcessTermination::Exited && self.status.success()
    }
}

pub fn run_p7_bounded_command(
    command: &mut Command,
    limits: P7ProcessLimits,
) -> io::Result<P7ProcessOutput> {
    let output = run_bounded_command(
        command,
        BoundedProcessLimits {
            stdout_bytes: limits.stdout_bytes,
            stderr_bytes: limits.stderr_bytes,
            total_bytes: limits.total_bytes,
            timeout: limits.timeout,
        },
    )?;
    let termination = match output.termination {
        BoundedProcessTermination::Exited => P7ProcessTermination::Exited,
        BoundedProcessTermination::TimedOut => P7ProcessTermination::TimedOut,
        BoundedProcessTermination::StdoutLimitExceeded => P7ProcessTermination::StdoutLimitExceeded,
        BoundedProcessTermination::StderrLimitExceeded => P7ProcessTermination::StderrLimitExceeded,
        BoundedProcessTermination::TotalLimitExceeded => P7ProcessTermination::TotalLimitExceeded,
    };
    Ok(P7ProcessOutput {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
        termination,
        elapsed: output.elapsed,
        receipt: P7ProcessReceipt {
            schema_version: "p7_sealed_process_receipt_v1".to_string(),
            sealed_executable_sha256: None,
            pid: output.pid,
            process_group: output.process_group,
            maximum_rss_bytes: output.maximum_rss_bytes,
            elapsed_millis: u64::try_from(output.elapsed.as_millis()).unwrap_or(u64::MAX),
        },
    })
}

pub fn run_p7_bounded_retained_executable(
    executable_path: &std::path::Path,
    args: &[&str],
    limits: P7ProcessLimits,
) -> io::Result<P7ProcessOutput> {
    let mut executable = P7RetainedFile::open_executable(executable_path)?;
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let (mut command, inherited_guard, identity) = executable.executable_command(&args)?;
    let mut output = run_p7_bounded_command(&mut command, limits)?;
    executable.verify_content(&identity)?;
    output.receipt.sealed_executable_sha256 = Some(identity.sha256);
    drop(inherited_guard);
    Ok(output)
}

pub fn run_p7_retained_executable(
    executable_path: &std::path::Path,
    args: &[String],
) -> io::Result<ExitStatus> {
    let mut executable = P7RetainedFile::open_executable(executable_path)?;
    let (mut command, inherited_guard, identity) = executable.executable_command(args)?;
    let status = command.status()?;
    executable.verify_content(&identity)?;
    drop(inherited_guard);
    Ok(status)
}

#[cfg(unix)]
pub fn exec_p7_retained_executable(
    executable_path: &std::path::Path,
    args: &[String],
) -> io::Result<()> {
    use std::os::unix::process::CommandExt;

    let mut executable = P7RetainedFile::open_executable(executable_path)?;
    let (mut command, inherited_guard, _identity) = executable.executable_command(args)?;
    let error = command.exec();
    drop(inherited_guard);
    Err(error)
}

#[cfg(windows)]
pub fn exec_p7_retained_executable(
    _executable_path: &std::path::Path,
    _args: &[String],
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "P7 retained exec mode is unavailable on Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    use std::io::{Read as _, Seek as _, Write as _};
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    #[test]
    fn exact_stream_cap_passes_and_plus_one_kills_the_group() {
        let limits = P7ProcessLimits {
            stdout_bytes: 4,
            stderr_bytes: 4,
            total_bytes: 8,
            timeout: Duration::from_secs(2),
        };
        let exact =
            run_p7_bounded_command(Command::new("/bin/sh").args(["-c", "printf 1234"]), limits)
                .expect("exact bounded output");
        assert!(exact.succeeded());
        assert_eq!(exact.stdout, b"1234");

        let plus_one =
            run_p7_bounded_command(Command::new("/bin/sh").args(["-c", "printf 12345"]), limits)
                .expect("bounded overflow result");
        assert_eq!(
            plus_one.termination,
            P7ProcessTermination::StdoutLimitExceeded
        );
        assert!(plus_one.stdout.len() <= 4);
    }

    #[cfg(unix)]
    #[test]
    fn timeout_terminates_a_process_group() {
        let output = run_p7_bounded_command(
            Command::new("/bin/sh").args(["-c", "sleep 5 & wait"]),
            P7ProcessLimits {
                stdout_bytes: 1024,
                stderr_bytes: 1024,
                total_bytes: 2048,
                timeout: Duration::from_millis(50),
            },
        )
        .expect("timeout result");
        assert_eq!(output.termination, P7ProcessTermination::TimedOut);
        assert!(output.elapsed < Duration::from_secs(2));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sealed_execution_fails_closed_without_a_darwin_execution_broker() {
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let error = run_p7_bounded_retained_executable(
            &executable,
            &["--list"],
            P7ProcessLimits {
                stdout_bytes: 1024,
                stderr_bytes: 1024,
                total_bytes: 2048,
                timeout: Duration::from_secs(2),
            },
        )
        .expect_err("Darwin pathname execution must not claim sealed-byte authority");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(error.to_string().contains("execution broker"));
    }

    #[cfg(windows)]
    #[test]
    fn sealed_execution_fails_closed_without_a_windows_execution_broker() {
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let error = run_p7_bounded_retained_executable(
            &executable,
            &["--list"],
            P7ProcessLimits {
                stdout_bytes: 1024,
                stderr_bytes: 1024,
                total_bytes: 2048,
                timeout: Duration::from_secs(2),
            },
        )
        .expect_err("Windows pathname execution must not claim sealed-byte authority");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(error.to_string().contains("execution broker"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn retained_descriptor_executes_admitted_bytes_after_path_replacement() {
        let root =
            std::env::temp_dir().join(format!("bm-p7-retained-launch-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("create retained launch fixture");
        let root = std::fs::canonicalize(root).expect("canonical retained launch fixture");
        let executable = root.join("runner");
        std::fs::copy(
            std::env::current_exe().expect("test executable"),
            &executable,
        )
        .expect("copy admitted executable");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("make admitted executable runnable");

        let mut retained =
            P7RetainedFile::open_executable(&executable).expect("retain admitted executable");
        let args = vec!["--list".to_string()];
        let (mut command, inherited_guard, _) = retained
            .executable_command(&args)
            .expect("build retained descriptor command");
        std::fs::rename(&executable, root.join("admitted")).expect("displace admitted path");
        std::fs::write(&executable, b"replacement executable bytes")
            .expect("write replacement executable");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("make replacement executable runnable");

        let output = command.output().expect("execute retained descriptor");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout)
            .contains("retained_descriptor_executes_admitted_bytes_after_path_replacement"));
        assert!(
            retained.verify_unchanged().is_err(),
            "path replacement must be detected after retained execution"
        );
        drop(inherited_guard);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn retained_descriptor_launches_a_native_executable() {
        let executable = std::fs::canonicalize(std::env::current_exe().expect("test executable"))
            .expect("canonical test executable");
        let output = run_p7_bounded_retained_executable(
            &executable,
            &["--list"],
            P7ProcessLimits {
                stdout_bytes: 1024 * 1024,
                stderr_bytes: 64 * 1024,
                total_bytes: 1024 * 1024 + 64 * 1024,
                timeout: Duration::from_secs(2),
            },
        )
        .expect("launch retained native executable");
        assert!(
            output.succeeded(),
            "status={:?} termination={:?} stderr={}",
            output.status,
            output.termination,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains("retained_descriptor_launches_a_native_executable"),
            "retained test harness did not list its native tests"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sealed_native_execution_survives_same_inode_equal_length_source_mutation() {
        let root = std::env::temp_dir().join(format!(
            "bm-p7-sealed-equal-length-mutation-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("create sealed mutation fixture");
        let root = std::fs::canonicalize(root).expect("canonical sealed mutation fixture");
        let executable = root.join("runner");
        std::fs::copy(
            std::env::current_exe().expect("test executable"),
            &executable,
        )
        .expect("copy native executable");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("make native executable runnable");

        let mut retained =
            P7RetainedFile::open_executable(&executable).expect("retain admitted executable");
        let args = vec!["--list".to_string()];
        let (mut command, inherited_guard, sealed_identity) = retained
            .executable_command(&args)
            .expect("seal admitted executable");

        let mut mutable_source = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&executable)
            .expect("open original inode for mutation");
        let mut original = [0_u8; 1];
        mutable_source
            .read_exact(&mut original)
            .expect("read original byte");
        mutable_source
            .seek(std::io::SeekFrom::Start(0))
            .expect("rewind original inode");
        mutable_source
            .write_all(&[original[0] ^ 0xff])
            .expect("mutate one byte without changing length");
        mutable_source.sync_all().expect("sync same-inode mutation");

        let output = command.output().expect("execute sealed native object");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout)
            .contains("sealed_native_execution_survives_same_inode_equal_length_source_mutation"));
        assert!(retained.verify_content(&sealed_identity).is_err());

        drop(inherited_guard);
        let _ = std::fs::remove_dir_all(root);
    }
}
