use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{
    OfficialOllamaStopPlan, OllamaTransparentConfig, OllamaTransparentError, PortOwnerKind,
    PortOwnerObserver, Result, SystemPortOwnerObserver,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManagedProcessKind {
    ManagedUpstream,
    TransparentFront,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProcessReport {
    pub kind: ManagedProcessKind,
    pub pid: Option<u32>,
    pub started: bool,
    pub message: Option<String>,
}

impl ManagedProcessReport {
    pub fn started(kind: ManagedProcessKind, pid: Option<u32>) -> Self {
        Self {
            kind,
            pid,
            started: true,
            message: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessActionReport {
    pub action: String,
    pub ok: bool,
    pub pid: Option<u32>,
    pub message: Option<String>,
}

impl ProcessActionReport {
    pub fn ok(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            ok: true,
            pid: None,
            message: None,
        }
    }

    pub fn failed(action: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            ok: false,
            pid: None,
            message: Some(message.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub target: String,
    pub ok: bool,
    pub message: Option<String>,
}

impl ProbeReport {
    pub fn ok(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            ok: true,
            message: None,
        }
    }
}

pub trait ProcessManager {
    fn stop_official_ollama(
        &self,
        plan: &OfficialOllamaStopPlan,
    ) -> Result<Vec<ProcessActionReport>>;

    fn start_managed_upstream(
        &self,
        config: &OllamaTransparentConfig,
    ) -> Result<ManagedProcessReport>;

    fn probe_managed_upstream(&self, config: &OllamaTransparentConfig) -> Result<ProbeReport>;

    fn start_transparent_front(
        &self,
        config: &OllamaTransparentConfig,
    ) -> Result<ManagedProcessReport>;

    fn probe_public_front(&self, config: &OllamaTransparentConfig) -> Result<ProbeReport>;

    fn stop_transparent_front(
        &self,
        config: &OllamaTransparentConfig,
    ) -> Result<ProcessActionReport>;

    fn stop_managed_upstream(
        &self,
        config: &OllamaTransparentConfig,
    ) -> Result<ProcessActionReport>;

    fn open_official_app(&self, config: &OllamaTransparentConfig) -> Result<ProcessActionReport>;
}

#[derive(Debug, Default)]
pub struct SystemProcessManager {
    children: Mutex<SystemProcessChildren>,
}

#[derive(Debug, Default)]
struct SystemProcessChildren {
    managed_upstream: Option<Child>,
    transparent_front: Option<Child>,
}

impl ProcessManager for SystemProcessManager {
    fn stop_official_ollama(
        &self,
        plan: &OfficialOllamaStopPlan,
    ) -> Result<Vec<ProcessActionReport>> {
        let mut reports = Vec::new();
        if let Some(report) = stop_official_app_processes()? {
            reports.push(report);
        }
        for process in &plan.processes {
            kill_pid_allow_missing(process.pid)?;
            reports.push(ProcessActionReport {
                action: "stop_official_ollama".to_string(),
                ok: true,
                pid: Some(process.pid),
                message: None,
            });
        }
        Ok(reports)
    }

    fn start_managed_upstream(
        &self,
        config: &OllamaTransparentConfig,
    ) -> Result<ManagedProcessReport> {
        if let Some(pid) = self.running_pid(ManagedProcessKind::ManagedUpstream)? {
            return Ok(ManagedProcessReport::started(
                ManagedProcessKind::ManagedUpstream,
                Some(pid),
            ));
        }
        if let Some(pid) = self.existing_owned_process(
            config,
            ManagedProcessKind::ManagedUpstream,
            config.upstream_bind,
            PortOwnerKind::ManagedOllamaRunner,
        )? {
            return Ok(ManagedProcessReport::started(
                ManagedProcessKind::ManagedUpstream,
                Some(pid),
            ));
        }
        ensure_managed_dirs(config)?;
        let stdout = log_stdio(config, "managed-upstream.stdout.log")?;
        let stderr = log_stdio(config, "managed-upstream.stderr.log")?;
        let child = Command::new(&config.managed_runner_path)
            .arg("serve")
            .env("OLLAMA_HOST", config.upstream_bind.to_string())
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .map_err(|error| {
                OllamaTransparentError::process_action_failed(format!(
                    "failed to start managed Ollama upstream: {error}"
                ))
            })?;
        let pid = child.id();
        self.children
            .lock()
            .expect("system process children")
            .managed_upstream = Some(child);
        Ok(ManagedProcessReport::started(
            ManagedProcessKind::ManagedUpstream,
            Some(pid),
        ))
    }

    fn probe_managed_upstream(&self, config: &OllamaTransparentConfig) -> Result<ProbeReport> {
        probe_ollama_http_with_retry(
            "managed_upstream_api",
            config.upstream_bind,
            Duration::from_secs(30),
        )
        .map_err(|error| {
            self.probe_error_with_child_exit_context(
                config,
                ManagedProcessKind::ManagedUpstream,
                "managed-upstream.stderr.log",
                error,
            )
        })
    }

    fn start_transparent_front(
        &self,
        config: &OllamaTransparentConfig,
    ) -> Result<ManagedProcessReport> {
        if let Some(pid) = self.running_pid(ManagedProcessKind::TransparentFront)? {
            return Ok(ManagedProcessReport::started(
                ManagedProcessKind::TransparentFront,
                Some(pid),
            ));
        }
        if let Some(pid) = self.existing_owned_process(
            config,
            ManagedProcessKind::TransparentFront,
            config.public_bind,
            PortOwnerKind::BeetleMemoryTransparentFront,
        )? {
            return Ok(ManagedProcessReport::started(
                ManagedProcessKind::TransparentFront,
                Some(pid),
            ));
        }
        ensure_managed_dirs(config)?;
        let stdout = log_stdio(config, "transparent-front.stdout.log")?;
        let stderr = log_stdio(config, "transparent-front.stderr.log")?;
        let child = Command::new(&config.gateway_binary_path)
            .env("BM_LLM_GATEWAY_BIND", config.public_bind.to_string())
            .env(
                "BM_LLM_GATEWAY_OLLAMA_BASE_URL",
                format!("http://{}/api", config.upstream_bind),
            )
            .env("BM_LLM_GATEWAY_DEFAULT_PROVIDER", "ollama")
            .env("BM_LLM_GATEWAY_MAINTENANCE_PROVIDER", "ollama")
            .env(
                "BM_LLM_GATEWAY_MAINTENANCE_MODEL",
                config.maintenance_model.trim(),
            )
            .env("BM_MEMORY_STORE_FILE", &config.memory_store_path)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .map_err(|error| {
                OllamaTransparentError::process_action_failed(format!(
                    "failed to start transparent gateway front {}: {error}",
                    config.gateway_binary_path.display()
                ))
            })?;
        let pid = child.id();
        self.children
            .lock()
            .expect("system process children")
            .transparent_front = Some(child);
        Ok(ManagedProcessReport::started(
            ManagedProcessKind::TransparentFront,
            Some(pid),
        ))
    }

    fn probe_public_front(&self, config: &OllamaTransparentConfig) -> Result<ProbeReport> {
        probe_ollama_http_with_retry(
            "public_front_api",
            config.public_bind,
            Duration::from_secs(15),
        )
        .map_err(|error| {
            self.probe_error_with_child_exit_context(
                config,
                ManagedProcessKind::TransparentFront,
                "transparent-front.stderr.log",
                error,
            )
        })
    }

    fn stop_transparent_front(
        &self,
        config: &OllamaTransparentConfig,
    ) -> Result<ProcessActionReport> {
        self.stop_owned_process(
            config,
            ManagedProcessKind::TransparentFront,
            config.public_bind,
            PortOwnerKind::BeetleMemoryTransparentFront,
        )
    }

    fn stop_managed_upstream(
        &self,
        config: &OllamaTransparentConfig,
    ) -> Result<ProcessActionReport> {
        self.stop_owned_process(
            config,
            ManagedProcessKind::ManagedUpstream,
            config.upstream_bind,
            PortOwnerKind::ManagedOllamaRunner,
        )
    }

    fn open_official_app(&self, config: &OllamaTransparentConfig) -> Result<ProcessActionReport> {
        #[cfg(target_os = "macos")]
        {
            let mut command = Command::new("open");
            if config
                .app_bundle_path
                .file_name()
                .is_some_and(|name| name == "Ollama.app")
            {
                command.args(["-a", "Ollama"]);
            } else {
                command.arg(&config.app_bundle_path);
            }
            let status = command.status().map_err(|error| {
                OllamaTransparentError::process_action_failed(format!(
                    "failed to open Ollama app: {error}"
                ))
            })?;
            if status.success() {
                return Ok(ProcessActionReport::ok("open_official_app"));
            }
            Err(OllamaTransparentError::process_action_failed(
                "open returned non-zero status for Ollama app",
            ))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = config;
            Err(OllamaTransparentError::unsupported(
                "opening Ollama.app is only implemented on macOS",
            ))
        }
    }
}

#[cfg(target_os = "macos")]
fn stop_official_app_processes() -> Result<Option<ProcessActionReport>> {
    let pgrep = Command::new("pgrep")
        .args(["-x", "Ollama"])
        .output()
        .map_err(|error| {
            OllamaTransparentError::process_action_failed(format!(
                "failed to inspect Ollama app process: {error}"
            ))
        })?;
    if !pgrep.status.success() || pgrep.stdout.is_empty() {
        return Ok(None);
    }

    let mut stopped = 0_u32;
    for line in String::from_utf8_lossy(&pgrep.stdout).lines() {
        let Some(pid) = line.trim().parse::<u32>().ok() else {
            continue;
        };
        kill_pid_allow_missing(pid)?;
        stopped += 1;
    }
    std::thread::sleep(Duration::from_millis(600));
    Ok(Some(ProcessActionReport {
        action: "stop_official_ollama_app".to_string(),
        ok: true,
        pid: None,
        message: Some(format!("stopped {stopped} Ollama app process(es)")),
    }))
}

#[cfg(not(target_os = "macos"))]
fn stop_official_app_processes() -> Result<Option<ProcessActionReport>> {
    Ok(None)
}

impl SystemProcessManager {
    fn existing_owned_process(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
        bind: SocketAddr,
        expected_owner: PortOwnerKind,
    ) -> Result<Option<u32>> {
        let observer = SystemPortOwnerObserver::new(config.port_owner_classifier());
        let report = observer.inspect(bind)?;
        match (report.owner, report.process) {
            (PortOwnerKind::NoListener, _) => Ok(None),
            (owner, Some(process)) if owner == expected_owner => Ok(Some(process.pid)),
            (owner, process) => Err(OllamaTransparentError::process_action_failed(format!(
                "refusing to start {kind:?}; expected free port or {expected_owner:?} at {bind}, found {owner:?} {process:?}"
            ))),
        }
    }

    fn running_pid(&self, kind: ManagedProcessKind) -> Result<Option<u32>> {
        let mut children = self.children.lock().expect("system process children");
        let child = match kind {
            ManagedProcessKind::ManagedUpstream => &mut children.managed_upstream,
            ManagedProcessKind::TransparentFront => &mut children.transparent_front,
        };
        let Some(process) = child.as_mut() else {
            return Ok(None);
        };
        match process.try_wait() {
            Ok(None) => Ok(Some(process.id())),
            Ok(Some(_)) => {
                *child = None;
                Ok(None)
            }
            Err(error) => Err(OllamaTransparentError::process_action_failed(format!(
                "failed to inspect child process: {error}"
            ))),
        }
    }

    fn probe_error_with_child_exit_context(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
        stderr_log_name: &str,
        error: OllamaTransparentError,
    ) -> OllamaTransparentError {
        let Ok(Some(exited)) = self.take_exited_child(kind) else {
            return error;
        };
        let stderr_path = config.managed_log_dir.join(stderr_log_name);
        let stderr_tail = read_log_tail(&stderr_path, 4096)
            .ok()
            .map(|tail| compact_log_tail(&tail))
            .filter(|tail| !tail.is_empty());
        let mut message = format!(
            "{}; tracked {kind:?} pid {} exited before probe completed with {}",
            error, exited.pid, exited.status
        );
        if let Some(tail) = stderr_tail {
            message.push_str("; stderr tail: ");
            message.push_str(&tail);
        }
        OllamaTransparentError::process_action_failed(message)
    }

    fn take_exited_child(&self, kind: ManagedProcessKind) -> Result<Option<ExitedChildReport>> {
        let mut children = self.children.lock().expect("system process children");
        let child = match kind {
            ManagedProcessKind::ManagedUpstream => &mut children.managed_upstream,
            ManagedProcessKind::TransparentFront => &mut children.transparent_front,
        };
        let Some(process) = child.as_mut() else {
            return Ok(None);
        };
        let pid = process.id();
        match process.try_wait() {
            Ok(None) => Ok(None),
            Ok(Some(status)) => {
                *child = None;
                Ok(Some(ExitedChildReport {
                    pid,
                    status: status.to_string(),
                }))
            }
            Err(error) => Err(OllamaTransparentError::process_action_failed(format!(
                "failed to inspect tracked {kind:?} pid {pid}: {error}"
            ))),
        }
    }

    fn stop_owned_process(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
        bind: SocketAddr,
        expected_owner: PortOwnerKind,
    ) -> Result<ProcessActionReport> {
        if let Some(pid) = self.stop_tracked_child(kind)? {
            return Ok(ProcessActionReport {
                action: stop_action(kind).to_string(),
                ok: true,
                pid: Some(pid),
                message: None,
            });
        }

        let observer = SystemPortOwnerObserver::new(config.port_owner_classifier());
        let report = observer.inspect(bind)?;
        match (report.owner, report.process) {
            (PortOwnerKind::NoListener, _) => Ok(ProcessActionReport::ok(stop_action(kind))),
            (owner, Some(process)) if owner == expected_owner => {
                kill_pid(process.pid)?;
                Ok(ProcessActionReport {
                    action: stop_action(kind).to_string(),
                    ok: true,
                    pid: Some(process.pid),
                    message: None,
                })
            }
            (owner, process) => Err(OllamaTransparentError::process_action_failed(format!(
                "refusing to stop {kind:?}; expected {expected_owner:?} at {bind}, found {owner:?} {process:?}"
            ))),
        }
    }

    fn stop_tracked_child(&self, kind: ManagedProcessKind) -> Result<Option<u32>> {
        let mut children = self.children.lock().expect("system process children");
        let child = match kind {
            ManagedProcessKind::ManagedUpstream => &mut children.managed_upstream,
            ManagedProcessKind::TransparentFront => &mut children.transparent_front,
        };
        let Some(mut process) = child.take() else {
            return Ok(None);
        };
        let pid = process.id();
        match process.try_wait() {
            Ok(Some(_)) => Ok(Some(pid)),
            Ok(None) => {
                process.kill().map_err(|error| {
                    OllamaTransparentError::process_action_failed(format!(
                        "failed to kill tracked {kind:?} pid {pid}: {error}"
                    ))
                })?;
                let _ = process.wait();
                Ok(Some(pid))
            }
            Err(error) => Err(OllamaTransparentError::process_action_failed(format!(
                "failed to inspect tracked {kind:?} pid {pid}: {error}"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExitedChildReport {
    pid: u32,
    status: String,
}

fn stop_action(kind: ManagedProcessKind) -> &'static str {
    match kind {
        ManagedProcessKind::ManagedUpstream => "stop_managed_upstream",
        ManagedProcessKind::TransparentFront => "stop_transparent_front",
    }
}

#[cfg(unix)]
fn kill_pid(pid: u32) -> Result<()> {
    let status = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .map_err(|error| {
            OllamaTransparentError::process_action_failed(format!("failed to run kill: {error}"))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(OllamaTransparentError::process_action_failed(format!(
            "kill returned non-zero status for pid {pid}"
        )))
    }
}

#[cfg(unix)]
fn kill_pid_allow_missing(pid: u32) -> Result<()> {
    match kill_pid(pid) {
        Ok(()) => Ok(()),
        Err(error) => {
            let still_running = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
            if still_running {
                Err(error)
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(windows)]
fn kill_pid(pid: u32) -> Result<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status()
        .map_err(|error| {
            OllamaTransparentError::process_action_failed(format!(
                "failed to run taskkill: {error}"
            ))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(OllamaTransparentError::process_action_failed(format!(
            "taskkill returned non-zero status for pid {pid}"
        )))
    }
}

#[cfg(windows)]
fn kill_pid_allow_missing(pid: u32) -> Result<()> {
    kill_pid(pid)
}

fn ensure_managed_dirs(config: &OllamaTransparentConfig) -> Result<()> {
    fs::create_dir_all(&config.managed_log_dir).map_err(|error| {
        OllamaTransparentError::process_action_failed(format!(
            "failed to create transparent mode log dir {}: {error}",
            config.managed_log_dir.display()
        ))
    })?;
    if let Some(parent) = config.memory_store_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            OllamaTransparentError::process_action_failed(format!(
                "failed to create gateway memory store dir {}: {error}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}

fn log_stdio(config: &OllamaTransparentConfig, name: &str) -> Result<Stdio> {
    let path = config.managed_log_dir.join(name);
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map(Stdio::from)
        .map_err(|error| {
            OllamaTransparentError::process_action_failed(format!(
                "failed to open log file {}: {error}",
                path.display()
            ))
        })
}

fn read_log_tail(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len > max_bytes {
        file.seek(SeekFrom::Start(len - max_bytes))?;
    }
    let mut tail = String::new();
    file.read_to_string(&mut tail)?;
    Ok(tail)
}

fn compact_log_tail(text: &str) -> String {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .rev()
        .take(6)
        .collect::<Vec<_>>();
    lines.reverse();
    lines.join(" | ")
}

fn probe_ollama_http(target: &str, bind: SocketAddr) -> Result<ProbeReport> {
    let mut stream =
        TcpStream::connect_timeout(&bind, Duration::from_secs(2)).map_err(|error| {
            OllamaTransparentError::process_action_failed(format!(
                "failed to probe {target} at {bind}: {error}"
            ))
        })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| {
            OllamaTransparentError::process_action_failed(format!(
                "failed to set probe read timeout for {target}: {error}"
            ))
        })?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| {
            OllamaTransparentError::process_action_failed(format!(
                "failed to set probe write timeout for {target}: {error}"
            ))
        })?;
    let request = "GET /api/version HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream.write_all(request.as_bytes()).map_err(|error| {
        OllamaTransparentError::process_action_failed(format!(
            "failed to write probe request to {target}: {error}"
        ))
    })?;
    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(|error| {
        OllamaTransparentError::process_action_failed(format!(
            "failed to read probe response from {target}: {error}"
        ))
    })?;
    let status = response.lines().next().unwrap_or_default();
    let ok = status.contains(" 2");
    if !ok {
        return Err(OllamaTransparentError::process_action_failed(format!(
            "probe {target} at {bind} returned non-2xx status: {status}"
        )));
    }
    Ok(ProbeReport::ok(target))
}

fn probe_ollama_http_with_retry(
    target: &str,
    bind: SocketAddr,
    timeout: Duration,
) -> Result<ProbeReport> {
    let started = Instant::now();
    let mut last_error = None;
    while started.elapsed() < timeout {
        match probe_ollama_http(target, bind) {
            Ok(report) => return Ok(report),
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(400));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        OllamaTransparentError::process_action_failed(format!(
            "timed out probing {target} at {bind}"
        ))
    }))
}
