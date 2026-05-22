use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::{OllamaTransparentError, Result};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedProcess {
    pub pid: u32,
    pub command: String,
    pub executable: PathBuf,
}

impl ObservedProcess {
    pub fn new(pid: u32, command: impl Into<String>, executable: impl Into<PathBuf>) -> Self {
        Self {
            pid,
            command: command.into(),
            executable: executable.into(),
        }
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
    transparent_front_markers: Vec<String>,
}

impl PortOwnerClassifier {
    pub fn new(official_ollama_binary: PathBuf, managed_runner_path: PathBuf) -> Self {
        Self {
            official_ollama_binary,
            managed_runner_path,
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
        if same_path(&process.executable, &self.managed_runner_path)
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
            return Ok(PortBindingReport::empty(bind));
        }
        let Some(process) = parse_lsof_process(&output.stdout) else {
            return Ok(PortBindingReport::empty(bind).with_detail("lsof returned no process"));
        };
        let owner = self
            .classifier
            .classify(ClassifyPortOwnerRequest::process(bind, process.clone()));
        Ok(PortBindingReport::owned(bind, owner, process))
    }
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
    let executable = ps_executable(pid).unwrap_or_else(|| PathBuf::from(&command));
    Some(ObservedProcess::new(pid, command, executable))
}

fn ps_executable(pid: u32) -> Option<PathBuf> {
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

fn same_path(left: &Path, right: &Path) -> bool {
    left == right || left.to_string_lossy() == right.to_string_lossy()
}
