use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::{
    runner::inspect_executable_identity, ExecutableFileIdentity, OllamaTransparentError, Result,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedProcess {
    pub pid: u32,
    pub command: String,
    pub executable: PathBuf,
    pub start_identity: Option<String>,
    pub executable_identity: Option<ExecutableFileIdentity>,
}

impl ObservedProcess {
    pub fn new(pid: u32, command: impl Into<String>, executable: impl Into<PathBuf>) -> Self {
        Self {
            pid,
            command: command.into(),
            executable: executable.into(),
            start_identity: None,
            executable_identity: None,
        }
    }

    pub fn with_start_identity(mut self, start_identity: impl Into<String>) -> Self {
        self.start_identity = Some(start_identity.into());
        self
    }

    pub fn with_executable_identity(mut self, identity: ExecutableFileIdentity) -> Self {
        self.executable_identity = Some(identity);
        self
    }

    pub fn has_complete_identity(&self) -> bool {
        !self.command.trim().is_empty()
            && !self.executable.as_os_str().is_empty()
            && self
                .start_identity
                .as_deref()
                .is_some_and(|identity| !identity.trim().is_empty())
            && self.executable_identity.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortOwnerKind {
    NoListener,
    OfficialOllama,
    BeetleMemoryTransparentFront,
    ManagedOllamaRunner,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortBindingReport {
    pub bind: SocketAddr,
    pub owner: PortOwnerKind,
    pub process: Option<ObservedProcess>,
    pub detail: Option<String>,
}

impl PortBindingReport {
    pub fn empty(bind: SocketAddr) -> Self {
        Self {
            bind,
            owner: PortOwnerKind::NoListener,
            process: None,
            detail: None,
        }
    }

    pub fn owned(bind: SocketAddr, owner: PortOwnerKind, process: ObservedProcess) -> Self {
        Self {
            bind,
            owner,
            process: Some(process),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifyPortOwnerRequest {
    pub bind: SocketAddr,
    pub process: Option<ObservedProcess>,
}

impl ClassifyPortOwnerRequest {
    pub fn no_listener(bind: SocketAddr) -> Self {
        Self {
            bind,
            process: None,
        }
    }

    pub fn process(bind: SocketAddr, process: ObservedProcess) -> Self {
        Self {
            bind,
            process: Some(process),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortOwnerClassifier {
    official_ollama_binary: PathBuf,
    managed_runner_path: PathBuf,
    managed_objects_root: PathBuf,
    transparent_front_markers: Vec<String>,
}

impl PortOwnerClassifier {
    pub fn new(official_ollama_binary: PathBuf, managed_runner_path: PathBuf) -> Self {
        let managed_objects_root = managed_runner_path
            .parent()
            .unwrap_or_else(|| Path::new("/"))
            .join("objects");
        Self {
            official_ollama_binary,
            managed_runner_path,
            managed_objects_root,
            transparent_front_markers: vec![
                "bm-llm-gateway".to_string(),
                "bm-ollama-transparent".to_string(),
            ],
        }
    }

    pub fn classify(&self, request: ClassifyPortOwnerRequest) -> PortOwnerKind {
        let Some(process) = request.process else {
            return PortOwnerKind::NoListener;
        };
        if path_is_under(
            &process.executable,
            &self.managed_objects_root.join("upstream"),
        ) || same_path(&process.executable, &self.managed_runner_path)
            || process.command == "bm-real-ollama"
        {
            return PortOwnerKind::ManagedOllamaRunner;
        }
        if same_path(&process.executable, &self.official_ollama_binary)
            || (process.command == "ollama"
                && !same_path(&process.executable, &self.managed_runner_path))
            || (process
                .executable
                .to_string_lossy()
                .contains("Ollama.app/Contents/Resources/ollama"))
        {
            return PortOwnerKind::OfficialOllama;
        }
        if path_is_under(
            &process.executable,
            &self.managed_objects_root.join("front"),
        ) {
            return PortOwnerKind::BeetleMemoryTransparentFront;
        }
        let command = process.command.as_str();
        let executable = process.executable.to_string_lossy();
        if self
            .transparent_front_markers
            .iter()
            .any(|marker| command.contains(marker) || executable.contains(marker))
        {
            return PortOwnerKind::BeetleMemoryTransparentFront;
        }
        PortOwnerKind::Unknown
    }
}

fn path_is_under(path: &Path, parent: &Path) -> bool {
    path.starts_with(parent)
}

pub trait PortOwnerObserver {
    fn inspect(&self, bind: SocketAddr) -> Result<PortBindingReport>;
}

#[derive(Clone, Debug)]
pub struct SystemPortOwnerObserver {
    classifier: PortOwnerClassifier,
}

impl SystemPortOwnerObserver {
    pub fn new(classifier: PortOwnerClassifier) -> Self {
        Self { classifier }
    }
}

impl PortOwnerObserver for SystemPortOwnerObserver {
    fn inspect(&self, bind: SocketAddr) -> Result<PortBindingReport> {
        let Some(process) = inspect_process_at_bind(bind)? else {
            return Ok(PortBindingReport::empty(bind));
        };
        let owner = self
            .classifier
            .classify(ClassifyPortOwnerRequest::process(bind, process.clone()));
        Ok(PortBindingReport::owned(bind, owner, process))
    }
}

pub(crate) fn inspect_process_at_bind(bind: SocketAddr) -> Result<Option<ObservedProcess>> {
    let output = Command::new("lsof")
        .args([
            "-nP",
            &format!("-iTCP:{}", bind.port()),
            "-sTCP:LISTEN",
            "-Fpcn",
        ])
        .output()
        .map_err(|error| {
            OllamaTransparentError::port_inspection_failed(format!(
                "failed to run lsof for {bind}: {error}"
            ))
        })?;
    if output.stdout.is_empty() {
        return Ok(None);
    }
    parse_lsof_process(&output.stdout).map(Some).ok_or_else(|| {
        OllamaTransparentError::port_inspection_failed(format!(
            "lsof returned an incomplete process identity for {bind}"
        ))
    })
}

fn parse_lsof_process(output: &[u8]) -> Option<ObservedProcess> {
    let text = String::from_utf8_lossy(output);
    let mut pid = None;
    let mut command = None;
    for line in text.lines() {
        let (tag, value) = line.split_at(1);
        match tag {
            "p" => {
                if pid.is_some() {
                    break;
                }
                pid = value.parse::<u32>().ok();
            }
            "c" => {
                if command.is_none() {
                    command = Some(value.to_string());
                }
            }
            _ => {}
        }
    }
    let pid = pid?;
    let command = command.unwrap_or_else(|| "unknown".to_string());
    observe_process(pid, Some(command))
}

pub(crate) fn observe_process(pid: u32, command_hint: Option<String>) -> Option<ObservedProcess> {
    let executable = process_executable_path(pid)?;
    let executable_identity = inspect_executable_identity(&executable).ok()?;
    let start_identity = ps_start_identity(pid)?;
    let command = command_hint.unwrap_or_else(|| {
        executable
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string()
    });
    Some(
        ObservedProcess::new(pid, command, executable)
            .with_start_identity(start_identity)
            .with_executable_identity(executable_identity),
    )
}

#[cfg(target_os = "linux")]
fn process_executable_path(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[cfg(target_os = "macos")]
fn process_executable_path(pid: u32) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let mut buffer = Vec::<u8>::with_capacity(libc::PROC_PIDPATHINFO_MAXSIZE as usize);
    let length = unsafe {
        libc::proc_pidpath(
            pid as libc::c_int,
            buffer.as_mut_ptr().cast(),
            libc::PROC_PIDPATHINFO_MAXSIZE as u32,
        )
    };
    if length <= 0 {
        return None;
    }
    unsafe {
        buffer.set_len(length as usize);
    }
    Some(PathBuf::from(OsString::from_vec(buffer)))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_executable_path(pid: u32) -> Option<PathBuf> {
    let output = Command::new("ps")
        .args(["-ww", "-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

fn ps_start_identity(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-ww", "-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right || left.to_string_lossy() == right.to_string_lossy()
}
