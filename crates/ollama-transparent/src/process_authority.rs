use std::path::{Path, PathBuf};
#[cfg(not(target_os = "macos"))]
use std::process::Child;
use std::process::{Command, Stdio};
#[cfg(target_os = "macos")]
use std::time::Duration;

use crate::runner::{validate_published_executable, PublishedExecutable, PublishedExecutableKind};
use crate::{
    ExecutableFileIdentity, ManagedProcessKind, OllamaTransparentConfig, OllamaTransparentError,
    Result,
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PersistedProcessAuthority {
    LaunchdJob {
        label: String,
        service_target: String,
        executable_path: PathBuf,
        executable_identity: ExecutableFileIdentity,
    },
}

pub(crate) struct SpawnedProcess {
    pid: u32,
    owner: SpawnOwner,
}

enum SpawnOwner {
    #[cfg(not(target_os = "macos"))]
    Direct {
        child: Child,
        authority: DirectProcessAuthority,
    },
    #[cfg(target_os = "macos")]
    Launchd(LaunchdJobAuthority),
}

impl std::fmt::Debug for SpawnedProcess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SpawnedProcess")
            .field("pid", &self.pid)
            .field("authority", &self.authority_name())
            .finish()
    }
}

impl SpawnedProcess {
    pub(crate) fn spawn(
        command: Command,
        executable: &PublishedExecutable,
        kind: ManagedProcessKind,
        stdout_path: &Path,
        stderr_path: &Path,
    ) -> Result<Self> {
        #[cfg(not(target_os = "macos"))]
        let _ = kind;

        #[cfg(target_os = "macos")]
        {
            let authority =
                LaunchdJobAuthority::submit(&command, executable, kind, stdout_path, stderr_path)?;
            Ok(Self {
                pid: authority.pid,
                owner: SpawnOwner::Launchd(authority),
            })
        }

        #[cfg(not(target_os = "macos"))]
        {
            if command.get_program() != executable.path() {
                return Err(process_error(
                    "spawn command is not bound to the published executable",
                ));
            }
            let mut command = command;
            let stdout = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(stdout_path)
                .map_err(|error| process_error(format!("open stdout log failed: {error}")))?;
            let stderr = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(stderr_path)
                .map_err(|error| process_error(format!("open stderr log failed: {error}")))?;
            command
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr));
            let mut child = command
                .spawn()
                .map_err(|error| process_error(format!("process spawn failed: {error}")))?;
            let pid = child.id();
            let authority = match DirectProcessAuthority::for_spawned(&child) {
                Ok(authority) => authority,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            };
            Ok(Self {
                pid,
                owner: SpawnOwner::Direct { child, authority },
            })
        }
    }

    pub(crate) const fn id(&self) -> u32 {
        self.pid
    }

    pub(crate) fn persisted_authority(&self) -> Option<PersistedProcessAuthority> {
        match &self.owner {
            #[cfg(not(target_os = "macos"))]
            SpawnOwner::Direct { .. } => None,
            #[cfg(target_os = "macos")]
            SpawnOwner::Launchd(authority) => Some(authority.persisted()),
        }
    }

    pub(crate) fn recover(
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
        receipt: &crate::ObservedProcess,
        authority: &PersistedProcessAuthority,
    ) -> Result<Option<Self>> {
        #[cfg(target_os = "macos")]
        {
            let authority = LaunchdJobAuthority::recover(config, kind, receipt, authority)?;
            Ok(Some(Self {
                pid: authority.pid,
                owner: SpawnOwner::Launchd(authority),
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (config, kind, receipt, authority);
            Ok(None)
        }
    }

    pub(crate) fn try_wait(&mut self) -> Result<Option<String>> {
        match &mut self.owner {
            #[cfg(not(target_os = "macos"))]
            SpawnOwner::Direct { child, .. } => child
                .try_wait()
                .map(|status| status.map(|status| status.to_string()))
                .map_err(|error| process_error(format!("inspect child failed: {error}"))),
            #[cfg(target_os = "macos")]
            SpawnOwner::Launchd(authority) => authority.try_wait(),
        }
    }

    pub(crate) fn terminate(&mut self) -> Result<()> {
        match &mut self.owner {
            #[cfg(not(target_os = "macos"))]
            SpawnOwner::Direct { child, authority } => authority.terminate(child),
            #[cfg(target_os = "macos")]
            SpawnOwner::Launchd(authority) => authority.terminate(),
        }
    }

    pub(crate) fn wait_after_terminate(&mut self) {
        match &mut self.owner {
            #[cfg(not(target_os = "macos"))]
            SpawnOwner::Direct { child, .. } => {
                let _ = child.wait();
            }
            #[cfg(target_os = "macos")]
            SpawnOwner::Launchd(authority) => authority.wait_until_stopped(),
        }
    }

    fn authority_name(&self) -> &'static str {
        match &self.owner {
            #[cfg(target_os = "linux")]
            SpawnOwner::Direct { .. } => "pidfd",
            #[cfg(windows)]
            SpawnOwner::Direct { .. } => "retained_process_handle",
            #[cfg(all(not(target_os = "macos"), not(target_os = "linux"), not(windows)))]
            SpawnOwner::Direct { .. } => "unsupported",
            #[cfg(target_os = "macos")]
            SpawnOwner::Launchd(_) => "launchd_job_identity",
        }
    }
}

#[cfg(target_os = "linux")]
struct DirectProcessAuthority {
    pidfd: std::os::fd::OwnedFd,
}

#[cfg(target_os = "linux")]
impl DirectProcessAuthority {
    fn for_spawned(child: &Child) -> Result<Self> {
        Self::attach(child.id())
    }

    fn attach(pid: u32) -> Result<Self> {
        use std::os::fd::FromRawFd;

        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            return Err(process_error(format!(
                "pidfd_open({pid}) failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self {
            pidfd: unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) },
        })
    }

    fn terminate(&self, _child: &mut Child) -> Result<()> {
        self.signal()
    }

    fn signal(&self) -> Result<()> {
        use std::os::fd::AsRawFd;

        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.pidfd.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(process_error(format!("pidfd_send_signal failed: {error}")))
        }
    }
}

#[cfg(windows)]
struct DirectProcessAuthority {
    handle: std::os::windows::io::OwnedHandle,
}

#[cfg(windows)]
impl DirectProcessAuthority {
    fn for_spawned(child: &Child) -> Result<Self> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let current = unsafe { GetCurrentProcess() };
        let mut handle: HANDLE = std::ptr::null_mut();
        let duplicated = unsafe {
            DuplicateHandle(
                current,
                child.as_raw_handle() as HANDLE,
                current,
                &mut handle,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if duplicated == 0 {
            return Err(process_error(format!(
                "DuplicateHandle failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self {
            handle: unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) },
        })
    }

    fn terminate(&self, _child: &mut Child) -> Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::Threading::TerminateProcess;

        if unsafe { TerminateProcess(self.handle.as_raw_handle() as HANDLE, 1) } != 0 {
            Ok(())
        } else {
            Err(process_error(format!(
                "TerminateProcess failed: {}",
                std::io::Error::last_os_error()
            )))
        }
    }
}

#[cfg(all(not(target_os = "macos"), not(target_os = "linux"), not(windows)))]
struct DirectProcessAuthority;

#[cfg(all(not(target_os = "macos"), not(target_os = "linux"), not(windows)))]
impl DirectProcessAuthority {
    fn for_spawned(child: &Child) -> Result<Self> {
        let _ = child;
        Err(process_error(
            "platform has no stable process authority for transparent children",
        ))
    }

    fn terminate(&self, _child: &mut Child) -> Result<()> {
        Err(process_error(
            "platform has no stable process authority for transparent children",
        ))
    }
}

pub(crate) struct AttachedProcessAuthority {
    #[cfg(target_os = "linux")]
    authority: DirectProcessAuthority,
    #[cfg(windows)]
    handle: std::os::windows::io::OwnedHandle,
}

impl AttachedProcessAuthority {
    pub(crate) const fn is_supported() -> bool {
        cfg!(any(target_os = "linux", windows))
    }

    pub(crate) fn attach(pid: u32) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            Ok(Self {
                authority: DirectProcessAuthority::attach(pid)?,
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::FromRawHandle;
            use windows_sys::Win32::System::Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
            };
            let handle = unsafe {
                OpenProcess(
                    PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pid,
                )
            };
            if handle.is_null() {
                return Err(process_error(format!(
                    "OpenProcess({pid}) failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(Self {
                handle: unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) },
            })
        }
        #[cfg(target_os = "macos")]
        {
            let _ = pid;
            Err(process_error(
                "refusing to stop an externally launched process on macOS without launchd/XPC job authority",
            ))
        }
        #[cfg(all(not(target_os = "macos"), not(target_os = "linux"), not(windows)))]
        {
            let _ = pid;
            Err(process_error(
                "platform cannot attach a stable process authority",
            ))
        }
    }

    pub(crate) fn terminate(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            self.authority.signal()
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Foundation::HANDLE;
            use windows_sys::Win32::System::Threading::TerminateProcess;

            let ok = unsafe { TerminateProcess(self.handle.as_raw_handle() as HANDLE, 1) };
            if ok != 0 {
                return Ok(());
            }
            Err(process_error(format!(
                "TerminateProcess failed: {}",
                std::io::Error::last_os_error()
            )))
        }
        #[cfg(any(target_os = "macos", all(not(target_os = "linux"), not(windows))))]
        Err(process_error("stable process authority is unavailable"))
    }
}

#[cfg(target_os = "macos")]
struct LaunchdJobAuthority {
    service_target: String,
    pid: u32,
    executable: PublishedExecutable,
}

#[cfg(target_os = "macos")]
impl LaunchdJobAuthority {
    fn submit(
        command: &Command,
        executable: &PublishedExecutable,
        kind: ManagedProcessKind,
        stdout_path: &Path,
        stderr_path: &Path,
    ) -> Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static JOB_SEQUENCE: AtomicU64 = AtomicU64::new(1);

        if command.get_program() != executable.path() {
            return Err(process_error(
                "launchd command is not bound to the published executable",
            ));
        }
        let digest_tag = executable
            .identity()
            .sha256
            .strip_prefix("sha256:")
            .and_then(|digest| digest.get(..16))
            .ok_or_else(|| process_error("published executable digest is non-canonical"))?;
        let label = format!(
            "com.beetlememory.ollama-transparent.{}.{digest_tag}.{}.{}",
            match kind {
                ManagedProcessKind::ManagedUpstream => "upstream",
                ManagedProcessKind::TransparentFront => "front",
            },
            std::process::id(),
            JOB_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let service_target = format!("gui/{}/{label}", unsafe { libc::getuid() });
        let mut launchctl = Command::new("launchctl");
        launchctl
            .arg("submit")
            .args(["-l", &label, "-o"])
            .arg(stdout_path)
            .arg("-e")
            .arg(stderr_path)
            .arg("--")
            .arg(command.get_program())
            .args(command.get_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        for (key, value) in command.get_envs() {
            match value {
                Some(value) => {
                    launchctl.env(key, value);
                }
                None => {
                    launchctl.env_remove(key);
                }
            }
        }
        let output = launchctl
            .output()
            .map_err(|error| process_error(format!("launchctl submit failed: {error}")))?;
        if !output.status.success() {
            return Err(process_error(format!(
                "launchctl submit rejected job {label} with status {}; stdout: {}; stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim(),
            )));
        }
        for _ in 0..100 {
            if let Some(snapshot) = launchd_job_snapshot(&service_target)? {
                if snapshot.program != executable.path() {
                    let _ = Command::new("launchctl")
                        .args(["bootout", &service_target])
                        .status();
                    return Err(process_error(
                        "launchd published a program path different from the immutable executable",
                    ));
                }
                return Ok(Self {
                    service_target,
                    pid: snapshot.pid,
                    executable: executable.clone(),
                });
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = Command::new("launchctl")
            .args(["bootout", &service_target])
            .status();
        Err(process_error(format!(
            "launchd job {service_target} did not publish a pid"
        )))
    }

    fn persisted(&self) -> PersistedProcessAuthority {
        let label = self.service_target.rsplit_once('/').map_or_else(
            || self.service_target.clone(),
            |(_, label)| label.to_string(),
        );
        PersistedProcessAuthority::LaunchdJob {
            label,
            service_target: self.service_target.clone(),
            executable_path: self.executable.path().to_path_buf(),
            executable_identity: self.executable.identity().clone(),
        }
    }

    fn recover(
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
        receipt: &crate::ObservedProcess,
        persisted: &PersistedProcessAuthority,
    ) -> Result<Self> {
        let PersistedProcessAuthority::LaunchdJob {
            label,
            service_target,
            executable_path,
            executable_identity,
        } = persisted;
        let published_kind = match kind {
            ManagedProcessKind::ManagedUpstream => PublishedExecutableKind::ManagedUpstream,
            ManagedProcessKind::TransparentFront => PublishedExecutableKind::TransparentFront,
        };
        let executable = validate_published_executable(
            config,
            published_kind,
            executable_path,
            executable_identity,
        )?;
        if receipt.executable != *executable_path
            || receipt.executable_identity.as_ref() != Some(executable_identity)
        {
            return Err(process_error(
                "persisted process receipt is not bound to its launchd executable authority",
            ));
        }
        let digest_tag = executable_identity
            .sha256
            .strip_prefix("sha256:")
            .and_then(|digest| digest.get(..16))
            .ok_or_else(|| process_error("persisted executable digest is non-canonical"))?;
        let expected_prefix = format!(
            "com.beetlememory.ollama-transparent.{}.{digest_tag}.",
            match kind {
                ManagedProcessKind::ManagedUpstream => "upstream",
                ManagedProcessKind::TransparentFront => "front",
            }
        );
        if !label.starts_with(&expected_prefix)
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        {
            return Err(process_error(
                "persisted launchd authority has a non-canonical job label",
            ));
        }
        let expected_target = format!("gui/{}/{label}", unsafe { libc::getuid() });
        if service_target != &expected_target {
            return Err(process_error(
                "persisted launchd authority is outside the current user bootstrap domain",
            ));
        }
        let Some(snapshot) = launchd_job_snapshot(service_target)? else {
            return Ok(Self {
                service_target: service_target.clone(),
                pid: receipt.pid,
                executable,
            });
        };
        if snapshot.program != *executable_path {
            return Err(process_error(
                "launchd job program is not the executable bound in the persisted authority",
            ));
        }
        let pid = snapshot.pid;
        if pid != receipt.pid {
            return Err(process_error(format!(
                "launchd job pid {pid} differs from persisted process pid {}",
                receipt.pid
            )));
        }
        let observed = crate::port_owner::observe_process(pid, Some(receipt.command.clone()))
            .ok_or_else(|| process_error("launchd job process identity is not observable"))?;
        if observed != *receipt {
            return Err(process_error(
                "launchd job process identity differs from the persisted launch receipt",
            ));
        }
        Ok(Self {
            service_target: service_target.clone(),
            pid,
            executable,
        })
    }

    fn try_wait(&self) -> Result<Option<String>> {
        if launchd_job_snapshot(&self.service_target)?.is_some() {
            Ok(None)
        } else {
            Ok(Some("launchd job exited".to_string()))
        }
    }

    fn terminate(&mut self) -> Result<()> {
        let output = Command::new("launchctl")
            .args(["bootout", &self.service_target])
            .output()
            .map_err(|error| process_error(format!("launchctl bootout failed: {error}")))?;
        if !output.status.success() && launchd_job_snapshot(&self.service_target)?.is_some() {
            return Err(process_error(format!(
                "launchctl bootout rejected {}: {}",
                self.service_target,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        self.wait_until_stopped();
        Ok(())
    }

    fn wait_until_stopped(&self) {
        for _ in 0..100 {
            if launchd_job_snapshot(&self.service_target)
                .ok()
                .flatten()
                .is_none()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(target_os = "macos")]
struct LaunchdJobSnapshot {
    pid: u32,
    program: PathBuf,
}

#[cfg(target_os = "macos")]
fn launchd_job_snapshot(service_target: &str) -> Result<Option<LaunchdJobSnapshot>> {
    let output = Command::new("launchctl")
        .args(["print", service_target])
        .output()
        .map_err(|error| process_error(format!("launchctl print failed: {error}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let pid = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("pid = ")
            .and_then(|pid| pid.parse().ok())
    });
    let program = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("program = ").map(PathBuf::from));
    match (pid, program) {
        (Some(pid), Some(program)) => Ok(Some(LaunchdJobSnapshot { pid, program })),
        (None, _) => Ok(None),
        (Some(_), None) => Err(process_error(
            "launchd job has a pid but does not expose its program path",
        )),
    }
}

fn process_error(message: impl Into<String>) -> OllamaTransparentError {
    OllamaTransparentError::process_action_failed(message)
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_external_pid_cannot_be_promoted_to_stop_authority() {
        let error = AttachedProcessAuthority::attach(std::process::id())
            .err()
            .expect("macOS must reject external pid authority");

        assert!(error
            .message()
            .contains("without launchd/XPC job authority"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_launchd_runs_published_fixture_and_recovers_after_controller_restart() {
        let root = std::env::temp_dir().join(format!(
            "bm-launchd-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create launchd test root");
        let root = std::fs::canonicalize(root).expect("canonical test root");
        let executable = std::env::current_exe().expect("test executable");
        let authority = crate::OllamaTransparentMemoryAuthority::new(
            "test-owner",
            "test-agent",
            "test-channel",
            root.join("store"),
        )
        .expect("memory authority");
        let config = OllamaTransparentConfig::new(&root, executable, authority).expect("config");
        let fixture = compile_authority_fixture(&root);
        let expected = crate::inspect_executable_identity(&fixture).expect("fixture identity");
        let published = crate::runner::publish_executable(
            &config,
            PublishedExecutableKind::ManagedUpstream,
            &fixture,
            &expected,
        )
        .expect("publish fixture");
        let command = Command::new(published.path());
        let spawned = SpawnedProcess::spawn(
            command,
            &published,
            ManagedProcessKind::ManagedUpstream,
            &root.join("stdout.log"),
            &root.join("stderr.log"),
        )
        .expect("submit launchd fixture");
        let pid = spawned.id();
        let receipt = (0..50)
            .find_map(|_| {
                let observed =
                    crate::port_owner::observe_process(pid, Some("authority-fixture".to_string()))?;
                if observed.executable == published.path() {
                    Some(observed)
                } else {
                    std::thread::sleep(Duration::from_millis(10));
                    None
                }
            })
            .expect("observe executed launchd fixture");
        let address = (0..100)
            .find_map(|_| {
                let address = std::fs::read_to_string(root.join("stdout.log"))
                    .ok()
                    .and_then(|output| {
                        output
                            .lines()
                            .find_map(|line| line.strip_prefix("READY "))
                            .and_then(|address| address.parse::<std::net::SocketAddr>().ok())
                    });
                if address.is_none() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                address
            })
            .expect("fixture ready address");
        let mut stream = std::net::TcpStream::connect(address).expect("connect fixture");
        use std::io::{Read, Write};
        stream
            .write_all(b"GET /authority HTTP/1.0\r\n\r\n")
            .expect("request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response");
        assert!(response.ends_with("authority-ok"));
        let persisted = spawned
            .persisted_authority()
            .expect("persist launchd authority");
        drop(spawned);

        let mut recovered = SpawnedProcess::recover(
            &config,
            ManagedProcessKind::ManagedUpstream,
            &receipt,
            &persisted,
        )
        .expect("recover launchd authority")
        .expect("macOS recovery authority");
        assert_eq!(recovered.id(), pid);
        recovered.terminate().expect("terminate recovered job");
        recovered.wait_after_terminate();
        let mut permissions = std::fs::metadata(
            published
                .path()
                .parent()
                .expect("published digest directory"),
        )
        .expect("digest directory metadata")
        .permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o700);
        std::fs::set_permissions(
            published
                .path()
                .parent()
                .expect("published digest directory"),
            permissions,
        )
        .expect("unseal fixture digest directory");
        std::fs::remove_dir_all(root).expect("cleanup launchd test root");
    }

    #[cfg(target_os = "macos")]
    fn compile_authority_fixture(root: &Path) -> PathBuf {
        let source = root.join("authority-fixture.rs");
        let binary = root.join("authority-fixture");
        std::fs::write(
            &source,
            r#"
use std::io::{Read, Write};
use std::net::TcpListener;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    println!("READY {}", listener.local_addr().expect("address"));
    std::io::stdout().flush().expect("flush");
    for stream in listener.incoming() {
        let mut stream = stream.expect("accept");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 12\r\n\r\nauthority-ok")
            .expect("response");
    }
}
"#,
        )
        .expect("fixture source");
        let output = Command::new("rustc")
            .args(["--edition=2021", "-o"])
            .arg(&binary)
            .arg(&source)
            .output()
            .expect("compile fixture");
        assert!(
            output.status.success(),
            "fixture compile failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        binary
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_spawned_child_is_terminated_through_retained_pidfd() {
        let mut child = Command::new("sh")
            .args(["-c", "sleep 30"])
            .spawn()
            .expect("spawn pidfd fixture");
        let authority = DirectProcessAuthority::for_spawned(&child).expect("retain pidfd");

        authority.terminate(&mut child).expect("pidfd terminate");
        let status = child.wait().expect("wait pidfd fixture");

        assert!(!status.success());
    }
}
