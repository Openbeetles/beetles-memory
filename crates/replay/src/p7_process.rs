use std::{
    io::{self, Read},
    process::{Command, ExitStatus, Stdio},
    sync::mpsc::{self, SyncSender},
    thread,
    time::{Duration, Instant},
};

use crate::p7_secure_fs::P7RetainedFile;
use serde::{Deserialize, Serialize};

const DRAIN_CHUNK_BYTES: usize = 64 * 1024;
const DRAIN_QUEUE_DEPTH: usize = 8;

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

enum DrainEvent {
    Chunk(Stream, Vec<u8>),
    Done(Stream, io::Result<()>),
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

pub fn run_p7_bounded_command(
    command: &mut Command,
    limits: P7ProcessLimits,
) -> io::Result<P7ProcessOutput> {
    validate_limits(limits)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    platform::prepare(command);
    let started = Instant::now();
    let mut child = command.spawn()?;
    let pid = child.id();
    let tree = match platform::ProcessTree::attach(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("P7 supervisor did not receive child stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("P7 supervisor did not receive child stderr"))?;
    let (sender, receiver) = mpsc::sync_channel(DRAIN_QUEUE_DEPTH);
    let stdout_thread = spawn_drain(stdout, Stream::Stdout, sender.clone());
    let stderr_thread = spawn_drain(stderr, Stream::Stderr, sender);

    let mut stdout_body = Vec::new();
    let mut stderr_body = Vec::new();
    let mut stdout_done = false;
    let mut stderr_done = false;
    let mut drain_error = None;
    let mut termination = P7ProcessTermination::Exited;
    let mut status = None;

    while status.is_none() || !stdout_done || !stderr_done {
        if termination == P7ProcessTermination::Exited && started.elapsed() > limits.timeout {
            termination = P7ProcessTermination::TimedOut;
            tree.kill(&mut child);
        }
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(DrainEvent::Chunk(stream, chunk)) => {
                let stdout_len = stdout_body.len();
                let stderr_len = stderr_body.len();
                let target = match stream {
                    Stream::Stdout => &mut stdout_body,
                    Stream::Stderr => &mut stderr_body,
                };
                let stream_limit = match stream {
                    Stream::Stdout => limits.stdout_bytes,
                    Stream::Stderr => limits.stderr_bytes,
                };
                let next_stream = u64::try_from(target.len())
                    .ok()
                    .and_then(|size| size.checked_add(u64::try_from(chunk.len()).ok()?));
                let next_total = u64::try_from(stdout_len)
                    .ok()
                    .and_then(|size| size.checked_add(u64::try_from(stderr_len).ok()?))
                    .and_then(|size| size.checked_add(u64::try_from(chunk.len()).ok()?));
                if termination == P7ProcessTermination::Exited
                    && next_stream.is_none_or(|size| size > stream_limit)
                {
                    termination = match stream {
                        Stream::Stdout => P7ProcessTermination::StdoutLimitExceeded,
                        Stream::Stderr => P7ProcessTermination::StderrLimitExceeded,
                    };
                    tree.kill(&mut child);
                } else if termination == P7ProcessTermination::Exited
                    && next_total.is_none_or(|size| size > limits.total_bytes)
                {
                    termination = P7ProcessTermination::TotalLimitExceeded;
                    tree.kill(&mut child);
                } else if termination == P7ProcessTermination::Exited {
                    target.extend_from_slice(&chunk);
                }
            }
            Ok(DrainEvent::Done(stream, result)) => {
                if let Err(error) = result {
                    tree.kill(&mut child);
                    drain_error.get_or_insert(error);
                }
                match stream {
                    Stream::Stdout => stdout_done = true,
                    Stream::Stderr => stderr_done = true,
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) if stdout_done && stderr_done => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tree.kill(&mut child);
                return Err(io::Error::other("P7 supervisor drain workers disconnected"));
            }
        }
        if status.is_none() {
            status = tree.try_wait(&mut child)?;
        }
    }
    let (status, maximum_rss_bytes) =
        status.ok_or_else(|| io::Error::other("P7 wait4 supervisor lost its child status"))?;
    stdout_thread
        .join()
        .map_err(|_| io::Error::other("P7 stdout drain worker panicked"))?;
    stderr_thread
        .join()
        .map_err(|_| io::Error::other("P7 stderr drain worker panicked"))?;
    if let Some(error) = drain_error {
        return Err(error);
    }
    let elapsed = started.elapsed();
    Ok(P7ProcessOutput {
        status,
        stdout: stdout_body,
        stderr: stderr_body,
        termination,
        elapsed,
        receipt: P7ProcessReceipt {
            schema_version: "p7_sealed_process_receipt_v1".to_string(),
            sealed_executable_sha256: None,
            pid,
            process_group: tree.process_group(),
            maximum_rss_bytes,
            elapsed_millis: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
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

fn validate_limits(limits: P7ProcessLimits) -> io::Result<()> {
    if limits.stdout_bytes == 0
        || limits.stderr_bytes == 0
        || limits.total_bytes == 0
        || limits.timeout.is_zero()
        || limits.stdout_bytes > limits.total_bytes
        || limits.stderr_bytes > limits.total_bytes
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "P7 supervisor limits are invalid",
        ));
    }
    Ok(())
}

fn spawn_drain<R: Read + Send + 'static>(
    mut reader: R,
    stream: Stream,
    sender: SyncSender<DrainEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let result = (|| -> io::Result<()> {
            let mut buffer = vec![0_u8; DRAIN_CHUNK_BYTES];
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    return Ok(());
                }
                sender
                    .send(DrainEvent::Chunk(stream, buffer[..read].to_vec()))
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::BrokenPipe, "P7 supervisor stopped")
                    })?;
            }
        })();
        let _ = sender.send(DrainEvent::Done(stream, result));
    })
}

#[cfg(unix)]
mod platform {
    use std::{
        io,
        os::unix::process::CommandExt,
        process::{Child, Command},
    };

    pub(super) fn prepare(command: &mut Command) {
        command.process_group(0);
    }

    pub(super) struct ProcessTree {
        process_group: i32,
    }

    impl ProcessTree {
        pub(super) fn attach(child: &Child) -> io::Result<Self> {
            let process_group = i32::try_from(child.id())
                .map_err(|_| io::Error::other("P7 child process id exceeds i32"))?;
            Ok(Self { process_group })
        }

        pub(super) fn kill(&self, child: &mut Child) {
            // SAFETY: a negative pid targets only the process group created for this child.
            unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
            let _ = child.kill();
        }

        pub(super) fn process_group(&self) -> i64 {
            i64::from(self.process_group)
        }

        pub(super) fn try_wait(
            &self,
            child: &mut Child,
        ) -> io::Result<Option<(std::process::ExitStatus, u64)>> {
            use std::os::unix::process::ExitStatusExt;

            let mut status = 0;
            let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
            // SAFETY: wait4 writes to the live status/rusage storage for this direct child only.
            let waited = unsafe {
                libc::wait4(
                    i32::try_from(child.id())
                        .map_err(|_| io::Error::other("P7 child pid exceeds i32"))?,
                    &mut status,
                    libc::WNOHANG,
                    &mut usage,
                )
            };
            if waited == 0 {
                return Ok(None);
            }
            if waited < 0 {
                return Err(io::Error::last_os_error());
            }
            #[cfg(target_os = "macos")]
            let maximum_rss_bytes = u64::try_from(usage.ru_maxrss).unwrap_or(u64::MAX);
            #[cfg(not(target_os = "macos"))]
            let maximum_rss_bytes = u64::try_from(usage.ru_maxrss)
                .unwrap_or(u64::MAX)
                .saturating_mul(1024);
            Ok(Some((ExitStatusExt::from_raw(status), maximum_rss_bytes)))
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        io,
        mem::size_of,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        os::windows::process::CommandExt,
        process::{Child, Command},
        ptr::null,
    };
    use windows_sys::Win32::{
        Foundation::HANDLE,
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::CREATE_SUSPENDED,
        },
    };

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtResumeProcess(process_handle: HANDLE) -> i32;
    }

    pub(super) fn prepare(command: &mut Command) {
        command.creation_flags(CREATE_SUSPENDED);
    }

    pub(super) struct ProcessTree(OwnedHandle);

    impl ProcessTree {
        pub(super) fn attach(child: &Child) -> io::Result<Self> {
            // SAFETY: null security/name arguments request a private unnamed Job object.
            let raw = unsafe { CreateJobObjectW(null(), null()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: CreateJobObjectW returned a new owned handle.
            let job = unsafe { OwnedHandle::from_raw_handle(raw) };
            let mut info = unsafe { std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: the handle and fixed-layout information buffer are live for the call.
            if unsafe {
                SetInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if unsafe {
                AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle() as HANDLE)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: Command spawned this process suspended; it is now contained by the Job.
            if unsafe { NtResumeProcess(child.as_raw_handle() as HANDLE) } < 0 {
                // SAFETY: this private Job contains only the still-suspended child process.
                unsafe { TerminateJobObject(job.as_raw_handle(), 1) };
                return Err(io::Error::other(
                    "P7 supervisor failed to resume Job-contained child",
                ));
            }
            Ok(Self(job))
        }

        pub(super) fn kill(&self, child: &mut Child) {
            // SAFETY: this private Job contains only the supervised process tree.
            unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) };
            let _ = child.kill();
        }

        pub(super) fn process_group(&self) -> i64 {
            i64::from(child_process_group_unsupported())
        }

        pub(super) fn try_wait(
            &self,
            child: &mut Child,
        ) -> io::Result<Option<(std::process::ExitStatus, u64)>> {
            Ok(child.try_wait()?.map(|status| (status, 0)))
        }
    }

    fn child_process_group_unsupported() -> i32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    use std::io::{Seek as _, Write as _};
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
