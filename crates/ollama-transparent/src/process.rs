use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{
    port_owner::{inspect_process_at_bind, observe_process},
    process_authority::{AttachedProcessAuthority, SpawnedProcess},
    process_receipt::{read_receipt_book, write_receipt_book, ManagedProcessControlRecord},
    runner::{publish_gateway_executable, published_managed_runner},
    ExecutableFileIdentity, ManagedRunnerReport, OfficialOllamaStopPlan, OllamaTransparentConfig,
    OllamaTransparentError, PortOwnerKind, PortOwnerObserver, Result, SystemPortOwnerObserver,
};

const MAX_PROBE_RESPONSE_BYTES: usize = 64 * 1024;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProcessOwnershipReport {
    pub managed_upstream_authorized: bool,
    pub transparent_front_authorized: bool,
}

impl ManagedProcessOwnershipReport {
    pub const fn fully_authorized(self) -> bool {
        self.managed_upstream_authorized && self.transparent_front_authorized
    }
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

pub(crate) trait ProcessManager {
    fn inspect_managed_process_ownership(
        &self,
        config: &OllamaTransparentConfig,
    ) -> Result<ManagedProcessOwnershipReport>;

    fn stop_official_ollama(
        &self,
        plan: &OfficialOllamaStopPlan,
    ) -> Result<Vec<ProcessActionReport>>;

    fn start_managed_upstream(
        &self,
        config: &OllamaTransparentConfig,
        runner: &ManagedRunnerReport,
    ) -> Result<ManagedProcessReport>;

    fn probe_managed_upstream(&self, config: &OllamaTransparentConfig) -> Result<ProbeReport>;

    fn start_transparent_front(
        &self,
        config: &OllamaTransparentConfig,
        gateway_executable: &ExecutableFileIdentity,
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
pub(crate) struct SystemProcessManager {
    children: Mutex<SystemProcessChildren>,
}

#[derive(Debug, Default)]
struct SystemProcessChildren {
    managed_upstream: Option<TrackedChild>,
    transparent_front: Option<TrackedChild>,
}

#[derive(Debug)]
struct TrackedChild {
    process: SpawnedProcess,
    receipt: crate::ObservedProcess,
    persisted_authority: Option<crate::process_authority::PersistedProcessAuthority>,
}

impl ProcessManager for SystemProcessManager {
    fn inspect_managed_process_ownership(
        &self,
        config: &OllamaTransparentConfig,
    ) -> Result<ManagedProcessOwnershipReport> {
        let mut children = self.children.lock().expect("system process children");
        self.recover_persisted_child(
            config,
            ManagedProcessKind::ManagedUpstream,
            &mut children.managed_upstream,
        )?;
        self.recover_persisted_child(
            config,
            ManagedProcessKind::TransparentFront,
            &mut children.transparent_front,
        )?;
        Ok(ManagedProcessOwnershipReport {
            managed_upstream_authorized: retained_authority_is_live(
                &mut children.managed_upstream,
            )?,
            transparent_front_authorized: retained_authority_is_live(
                &mut children.transparent_front,
            )?,
        })
    }

    fn stop_official_ollama(
        &self,
        plan: &OfficialOllamaStopPlan,
    ) -> Result<Vec<ProcessActionReport>> {
        if !plan.allowed || plan.targets.is_empty() {
            return Err(OllamaTransparentError::preflight_rejected(
                "official Ollama stop plan must be explicitly allowed and non-empty",
            ));
        }
        let mut reports = Vec::with_capacity(plan.targets.len());
        for target in &plan.targets {
            let authority = AttachedProcessAuthority::attach(target.process.pid)?;
            let observed = inspect_process_at_bind(target.bind)?.ok_or_else(|| {
                OllamaTransparentError::process_action_failed(format!(
                    "refusing to stop pid {}; port {} no longer has the preflight owner",
                    target.process.pid, target.bind
                ))
            })?;
            validate_stop_target(target, &observed)?;
            authority.terminate()?;
            reports.push(ProcessActionReport {
                action: "stop_official_ollama".to_string(),
                ok: true,
                pid: Some(target.process.pid),
                message: None,
            });
        }
        Ok(reports)
    }

    fn start_managed_upstream(
        &self,
        config: &OllamaTransparentConfig,
        runner: &ManagedRunnerReport,
    ) -> Result<ManagedProcessReport> {
        if let Some(pid) = self.running_pid(config, ManagedProcessKind::ManagedUpstream)? {
            return Ok(ManagedProcessReport::started(
                ManagedProcessKind::ManagedUpstream,
                Some(pid),
            ));
        }
        self.require_unowned_port(
            config,
            ManagedProcessKind::ManagedUpstream,
            config.upstream_bind,
        )?;
        ensure_managed_dirs(config)?;
        let published_runner = published_managed_runner(config, runner)?;
        let mut command = Command::new(published_runner.path());
        command
            .arg("serve")
            .env("OLLAMA_HOST", config.upstream_bind.to_string());
        let process = SpawnedProcess::spawn(
            command,
            &published_runner,
            ManagedProcessKind::ManagedUpstream,
            &config.managed_log_dir.join("managed-upstream.stdout.log"),
            &config.managed_log_dir.join("managed-upstream.stderr.log"),
        )?;
        let mut tracked = track_spawned_child(
            process,
            ManagedProcessKind::ManagedUpstream,
            published_runner.identity(),
        )?;
        let pid = tracked.process.id();
        if let Err(error) = self.persist_receipt(
            config,
            ManagedProcessKind::ManagedUpstream,
            Some(ManagedProcessControlRecord::new(
                tracked.receipt.clone(),
                tracked.persisted_authority.clone(),
            )),
        ) {
            let _ = tracked.process.terminate();
            tracked.process.wait_after_terminate();
            return Err(error);
        }
        self.children
            .lock()
            .expect("system process children")
            .managed_upstream = Some(tracked);
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
        gateway_executable: &ExecutableFileIdentity,
    ) -> Result<ManagedProcessReport> {
        if let Some(pid) = self.running_pid(config, ManagedProcessKind::TransparentFront)? {
            return Ok(ManagedProcessReport::started(
                ManagedProcessKind::TransparentFront,
                Some(pid),
            ));
        }
        self.require_unowned_port(
            config,
            ManagedProcessKind::TransparentFront,
            config.public_bind,
        )?;
        ensure_managed_dirs(config)?;
        let published_gateway = publish_gateway_executable(config, gateway_executable)?;
        let mut command = Command::new(published_gateway.path());
        command.envs(transparent_front_env(config));
        let process = SpawnedProcess::spawn(
            command,
            &published_gateway,
            ManagedProcessKind::TransparentFront,
            &config.managed_log_dir.join("transparent-front.stdout.log"),
            &config.managed_log_dir.join("transparent-front.stderr.log"),
        )?;
        let mut tracked = track_spawned_child(
            process,
            ManagedProcessKind::TransparentFront,
            published_gateway.identity(),
        )?;
        let pid = tracked.process.id();
        if let Err(error) = self.persist_receipt(
            config,
            ManagedProcessKind::TransparentFront,
            Some(ManagedProcessControlRecord::new(
                tracked.receipt.clone(),
                tracked.persisted_authority.clone(),
            )),
        ) {
            let _ = tracked.process.terminate();
            tracked.process.wait_after_terminate();
            return Err(error);
        }
        self.children
            .lock()
            .expect("system process children")
            .transparent_front = Some(tracked);
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

fn validate_stop_target(
    target: &crate::OfficialOllamaStopTarget,
    observed: &crate::ObservedProcess,
) -> Result<()> {
    if observed != &target.process {
        return Err(OllamaTransparentError::process_action_failed(format!(
            "refusing to stop pid {}; pid/start/command/executable identity changed since preflight",
            target.process.pid
        )));
    }
    Ok(())
}

fn track_spawned_child(
    mut process: SpawnedProcess,
    kind: ManagedProcessKind,
    expected_executable: &ExecutableFileIdentity,
) -> Result<TrackedChild> {
    let pid = process.id();
    let persisted_authority = process.persisted_authority();
    for _ in 0..50 {
        if let Some(observed) = observe_process(pid, Some(format!("{kind:?}"))) {
            if observed.executable_identity.as_ref() == Some(expected_executable) {
                return Ok(TrackedChild {
                    process,
                    receipt: observed,
                    persisted_authority,
                });
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = process.terminate();
    process.wait_after_terminate();
    Err(OllamaTransparentError::process_action_failed(format!(
        "spawned {kind:?} pid {pid} did not reach the published executable identity within the bounded launch window"
    )))
}

fn receipt_matches_live_process(receipt: &crate::ObservedProcess) -> bool {
    observe_process(receipt.pid, Some(receipt.command.clone())).is_some_and(|observed| {
        observed.pid == receipt.pid
            && observed.start_identity == receipt.start_identity
            && observed.executable == receipt.executable
            && observed.executable_identity == receipt.executable_identity
    })
}

fn retained_authority_is_live(process: &mut Option<TrackedChild>) -> Result<bool> {
    match process.as_mut() {
        Some(process) => Ok(process.process.try_wait()?.is_none()),
        None => Ok(false),
    }
}

impl SystemProcessManager {
    fn require_unowned_port(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
        bind: SocketAddr,
    ) -> Result<()> {
        let observer = SystemPortOwnerObserver::new(config.port_owner_classifier());
        let report = observer.inspect(bind)?;
        reject_untracked_listener(kind, bind, &report)
    }

    fn running_pid(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
    ) -> Result<Option<u32>> {
        let mut children = self.children.lock().expect("system process children");
        let child = match kind {
            ManagedProcessKind::ManagedUpstream => &mut children.managed_upstream,
            ManagedProcessKind::TransparentFront => &mut children.transparent_front,
        };
        self.recover_persisted_child(config, kind, child)?;
        let Some(process) = child.as_mut() else {
            let records = read_receipt_book(&config.managed_process_receipt_path)?;
            if let Some(record) = records.get(kind) {
                if receipt_matches_live_process(&record.process) {
                    return Err(OllamaTransparentError::process_action_failed(format!(
                        "persisted {kind:?} receipt for pid {} is diagnostic only; this controller has no retained process authority and cannot adopt it",
                        record.process.pid
                    )));
                }
                self.persist_receipt(config, kind, None)?;
            }
            return Ok(None);
        };
        match process.process.try_wait()? {
            None => Ok(Some(process.process.id())),
            Some(_) => {
                *child = None;
                self.persist_receipt(config, kind, None)?;
                Ok(None)
            }
        }
    }

    fn probe_error_with_child_exit_context(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
        stderr_log_name: &str,
        error: OllamaTransparentError,
    ) -> OllamaTransparentError {
        let Ok(Some(exited)) = self.take_exited_child(config, kind) else {
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

    fn take_exited_child(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
    ) -> Result<Option<ExitedChildReport>> {
        let mut children = self.children.lock().expect("system process children");
        let child = match kind {
            ManagedProcessKind::ManagedUpstream => &mut children.managed_upstream,
            ManagedProcessKind::TransparentFront => &mut children.transparent_front,
        };
        let Some(process) = child.as_mut() else {
            return Ok(None);
        };
        let pid = process.process.id();
        match process.process.try_wait()? {
            None => Ok(None),
            Some(status) => {
                *child = None;
                self.persist_receipt(config, kind, None)?;
                Ok(Some(ExitedChildReport { pid, status }))
            }
        }
    }

    fn stop_owned_process(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
        bind: SocketAddr,
    ) -> Result<ProcessActionReport> {
        {
            let mut children = self.children.lock().expect("system process children");
            let child = match kind {
                ManagedProcessKind::ManagedUpstream => &mut children.managed_upstream,
                ManagedProcessKind::TransparentFront => &mut children.transparent_front,
            };
            self.recover_persisted_child(config, kind, child)?;
        }
        if let Some(pid) = self.stop_tracked_child(config, kind)? {
            return Ok(ProcessActionReport {
                action: stop_action(kind).to_string(),
                ok: true,
                pid: Some(pid),
                message: None,
            });
        }

        let records = read_receipt_book(&config.managed_process_receipt_path)?;
        if let Some(record) = records.get(kind) {
            if receipt_matches_live_process(&record.process) {
                return Err(OllamaTransparentError::process_action_failed(format!(
                    "refusing to stop persisted {kind:?} pid {}; receipt is diagnostic only and this controller has no retained process authority",
                    record.process.pid
                )));
            }
            self.persist_receipt(config, kind, None)?;
        }

        let observer = SystemPortOwnerObserver::new(config.port_owner_classifier());
        let report = observer.inspect(bind)?;
        reject_untracked_listener(kind, bind, &report)?;
        Ok(ProcessActionReport::ok(stop_action(kind)))
    }

    fn stop_tracked_child(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
    ) -> Result<Option<u32>> {
        let mut children = self.children.lock().expect("system process children");
        let child = match kind {
            ManagedProcessKind::ManagedUpstream => &mut children.managed_upstream,
            ManagedProcessKind::TransparentFront => &mut children.transparent_front,
        };
        let Some(mut process) = child.take() else {
            return Ok(None);
        };
        let pid = process.process.id();
        let status = match process.process.try_wait() {
            Ok(status) => status,
            Err(error) => {
                *child = Some(process);
                return Err(error);
            }
        };
        match status {
            Some(_) => {
                self.persist_receipt(config, kind, None)?;
                Ok(Some(pid))
            }
            None => {
                if let Err(error) = process.process.terminate() {
                    *child = Some(process);
                    return Err(error);
                }
                process.process.wait_after_terminate();
                self.persist_receipt(config, kind, None)?;
                Ok(Some(pid))
            }
        }
    }

    fn persist_receipt(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
        record: Option<ManagedProcessControlRecord>,
    ) -> Result<()> {
        let mut receipts = read_receipt_book(&config.managed_process_receipt_path)?;
        receipts.set(kind, record);
        write_receipt_book(&config.managed_process_receipt_path, &receipts)
    }

    fn recover_persisted_child(
        &self,
        config: &OllamaTransparentConfig,
        kind: ManagedProcessKind,
        child: &mut Option<TrackedChild>,
    ) -> Result<()> {
        if child.is_some() {
            return Ok(());
        }
        let records = read_receipt_book(&config.managed_process_receipt_path)?;
        let Some(record) = records.get(kind) else {
            return Ok(());
        };
        let Some(authority) = record.authority.as_ref() else {
            return Ok(());
        };
        let Some(process) = SpawnedProcess::recover(config, kind, &record.process, authority)?
        else {
            return Ok(());
        };
        *child = Some(TrackedChild {
            process,
            receipt: record.process.clone(),
            persisted_authority: Some(authority.clone()),
        });
        Ok(())
    }
}

fn reject_untracked_listener(
    kind: ManagedProcessKind,
    bind: SocketAddr,
    report: &crate::PortBindingReport,
) -> Result<()> {
    if report.owner == PortOwnerKind::NoListener {
        return Ok(());
    }
    Err(OllamaTransparentError::process_action_failed(format!(
        "refusing untracked {kind:?} listener at {bind}; classifier {:?} is diagnostic only and no exact launch receipt authorizes adoption or kill: {:?}",
        report.owner, report.process
    )))
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

fn ensure_managed_dirs(config: &OllamaTransparentConfig) -> Result<()> {
    fs::create_dir_all(&config.managed_log_dir).map_err(|error| {
        OllamaTransparentError::process_action_failed(format!(
            "failed to create transparent mode log dir {}: {error}",
            config.managed_log_dir.display()
        ))
    })?;
    if let Some(parent) = config.memory_authority.store_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            OllamaTransparentError::process_action_failed(format!(
                "failed to create gateway memory store dir {}: {error}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}

fn transparent_front_env(config: &OllamaTransparentConfig) -> Vec<(String, String)> {
    vec![
        (
            "BM_LLM_GATEWAY_BIND".to_string(),
            config.public_bind.to_string(),
        ),
        (
            "BM_LLM_GATEWAY_OLLAMA_BASE_URL".to_string(),
            format!("http://{}/api", config.upstream_bind),
        ),
        (
            "BM_LLM_GATEWAY_DEFAULT_PROVIDER".to_string(),
            "ollama".to_string(),
        ),
        (
            "BM_MEMORY_STORE_FILE".to_string(),
            config.memory_authority.store_path.display().to_string(),
        ),
        (
            "BM_LLM_GATEWAY_SCOPE_PROFILE".to_string(),
            "ollama_app".to_string(),
        ),
        (
            "BM_LLM_GATEWAY_OLLAMA_APP_ID".to_string(),
            "ollama-app".to_string(),
        ),
        (
            "BM_MEMORY_OWNER_ID".to_string(),
            config.memory_authority.owner_id.clone(),
        ),
        (
            "BM_MEMORY_AGENT_ID".to_string(),
            config.memory_authority.agent_id.clone(),
        ),
        (
            "BM_MEMORY_CHANNEL".to_string(),
            config.memory_authority.channel.clone(),
        ),
    ]
}

fn read_log_tail(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if len > max_bytes {
        file.seek(SeekFrom::Start(len - max_bytes))?;
    }
    read_bounded_log_tail(&mut file, max_bytes)
}

fn read_bounded_log_tail(file: &mut File, max_bytes: u64) -> std::io::Result<String> {
    let limit = max_bytes.checked_add(1).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "log tail byte budget overflowed",
        )
    })?;
    let mut bytes = Vec::with_capacity(max_bytes.min(8192) as usize);
    file.take(limit).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("log tail exceeded {max_bytes} bytes while being read"),
        ));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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
    let mut response = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stream.read(&mut buffer).map_err(|error| {
            OllamaTransparentError::process_action_failed(format!(
                "failed to read probe response from {target}: {error}"
            ))
        })?;
        if read == 0 {
            break;
        }
        let next_len = response.len().checked_add(read).ok_or_else(|| {
            OllamaTransparentError::process_action_failed("probe response byte count overflowed")
        })?;
        if next_len > MAX_PROBE_RESPONSE_BYTES {
            return Err(OllamaTransparentError::process_action_failed(format!(
                "probe response from {target} exceeded {MAX_PROBE_RESPONSE_BYTES} bytes"
            )));
        }
        response.extend_from_slice(&buffer[..read]);
    }
    let response = String::from_utf8_lossy(&response);
    let status = response.lines().next().unwrap_or_default();
    let status_code = status
        .split_ascii_whitespace()
        .nth(1)
        .filter(|code| code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|code| code.parse::<u16>().ok());
    if !status_code.is_some_and(|code| (200..300).contains(&code)) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn diagnostic_receipts_never_restore_process_authority_after_controller_restart() {
        let root = std::env::temp_dir().join(format!(
            "bm-process-ownership-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir(&root).expect("test root");
        let root = std::fs::canonicalize(root).expect("canonical test root");
        let executable = std::env::current_exe().expect("test executable");
        let authority = crate::OllamaTransparentMemoryAuthority::new(
            "test-owner",
            "test-agent",
            "test-channel",
            root.join("store"),
        )
        .expect("test memory authority");
        let config =
            OllamaTransparentConfig::new(&root, &executable, authority).expect("test config");
        let live = observe_process(std::process::id(), Some("test-owner".to_string()))
            .expect("observe current process");
        let mut receipts = crate::process_receipt::ManagedProcessReceiptBook::default();
        receipts.set(
            ManagedProcessKind::ManagedUpstream,
            Some(ManagedProcessControlRecord::new(live.clone(), None)),
        );
        receipts.set(
            ManagedProcessKind::TransparentFront,
            Some(ManagedProcessControlRecord::new(live.clone(), None)),
        );
        write_receipt_book(&config.managed_process_receipt_path, &receipts)
            .expect("persist exact receipts");

        let manager = SystemProcessManager::default();
        let ownership = manager
            .inspect_managed_process_ownership(&config)
            .expect("inspect retained authorities");
        assert!(!ownership.managed_upstream_authorized);
        assert!(!ownership.transparent_front_authorized);

        let upstream_error = manager
            .stop_owned_process(
                &config,
                ManagedProcessKind::ManagedUpstream,
                config.upstream_bind,
            )
            .expect_err("receipt must not authorize upstream stop");
        assert!(upstream_error.message().contains("diagnostic only"));
        assert!(upstream_error
            .message()
            .contains("no retained process authority"));

        let front_error = manager
            .stop_owned_process(
                &config,
                ManagedProcessKind::TransparentFront,
                config.public_bind,
            )
            .expect_err("receipt must not authorize front stop");
        assert!(front_error.message().contains("diagnostic only"));

        let mut stale = live;
        stale.start_identity = Some("stale-process".to_string());
        receipts.set(
            ManagedProcessKind::TransparentFront,
            Some(ManagedProcessControlRecord::new(stale, None)),
        );
        write_receipt_book(&config.managed_process_receipt_path, &receipts)
            .expect("persist stale receipt");
        let stopped = manager
            .stop_owned_process(
                &config,
                ManagedProcessKind::TransparentFront,
                config.public_bind,
            )
            .expect("stale diagnostic receipt must not retain control");
        assert!(stopped.ok);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn transparent_front_env_enables_ollama_app_scope_with_stable_local_identity() {
        let authority = crate::OllamaTransparentMemoryAuthority::new(
            "test-owner",
            "test-agent",
            "test-channel",
            "/tmp/beetle-memory-test-data/store",
        )
        .expect("test memory authority");
        let config = OllamaTransparentConfig::new(
            "/tmp/beetle-memory-test-data",
            std::env::current_exe().expect("test executable"),
            authority,
        )
        .expect("test config");

        let env = transparent_front_env(&config);

        assert!(env.contains(&(
            "BM_LLM_GATEWAY_SCOPE_PROFILE".to_string(),
            "ollama_app".to_string()
        )));
        assert!(env.contains(&(
            "BM_LLM_GATEWAY_OLLAMA_APP_ID".to_string(),
            "ollama-app".to_string()
        )));
        assert!(env.contains(&("BM_MEMORY_OWNER_ID".to_string(), "test-owner".to_string())));
        assert!(env.contains(&("BM_MEMORY_AGENT_ID".to_string(), "test-agent".to_string())));
        assert!(env.contains(&("BM_MEMORY_CHANNEL".to_string(), "test-channel".to_string())));
        assert!(!env.iter().any(|(name, _)| name == "BM_MEMORY_CHAT_ID"));
    }

    #[test]
    fn probe_accepts_response_at_exact_byte_budget() {
        let (bind, server) = probe_server_with_response_len(MAX_PROBE_RESPONSE_BYTES);

        let report = probe_ollama_http("exact_budget", bind).expect("exact budget response");

        assert!(report.ok);
        server.join().expect("probe server");
    }

    #[test]
    fn probe_rejects_response_one_byte_over_budget() {
        let (bind, server) = probe_server_with_response_len(MAX_PROBE_RESPONSE_BYTES + 1);

        let error = probe_ollama_http("over_budget", bind).expect_err("over budget response");

        assert!(error.message().contains("exceeded 65536 bytes"));
        server.join().expect("probe server");
    }

    #[test]
    fn probe_rejects_high_speed_response_without_waiting_for_eof() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("probe listener");
        let bind = listener.local_addr().expect("listener address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("probe connection");
            let mut request = [0_u8; 256];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                .expect("response headers");
            let chunk = [b'x'; 8192];
            while stream.write_all(&chunk).is_ok() {}
        });

        let error = probe_ollama_http("continuous", bind).expect_err("continuous response");

        assert!(error.message().contains("exceeded 65536 bytes"));
        server.join().expect("probe server");
    }

    #[test]
    fn stop_target_revalidation_rejects_pid_reuse_command_drift_and_executable_aba() {
        let identity = ExecutableFileIdentity {
            sha256: "original".to_string(),
            byte_len: 10,
            device: 1,
            inode: 2,
            unix_mode: 0o755,
        };
        let expected = crate::ObservedProcess::new(42, "ollama", "/official/ollama")
            .with_start_identity("start-a")
            .with_executable_identity(identity.clone());
        let target = crate::OfficialOllamaStopTarget {
            bind: "127.0.0.1:11434".parse().expect("bind"),
            process: expected.clone(),
        };
        let reused = crate::ObservedProcess::new(42, "ollama", "/official/ollama")
            .with_start_identity("start-b")
            .with_executable_identity(identity.clone());
        let command_drift = crate::ObservedProcess::new(42, "other", "/official/ollama")
            .with_start_identity("start-a")
            .with_executable_identity(identity.clone());
        let mut replacement_identity = identity;
        replacement_identity.inode += 1;
        let executable_aba = crate::ObservedProcess::new(42, "ollama", "/official/ollama")
            .with_start_identity("start-a")
            .with_executable_identity(replacement_identity);

        assert!(validate_stop_target(&target, &expected).is_ok());
        assert!(validate_stop_target(&target, &reused).is_err());
        assert!(validate_stop_target(&target, &command_drift).is_err());
        assert!(validate_stop_target(&target, &executable_aba).is_err());
    }

    #[test]
    fn same_name_untracked_process_never_authorizes_adoption_or_kill() {
        let bind = "127.0.0.1:11435".parse().expect("bind");
        let report = crate::PortBindingReport::owned(
            bind,
            PortOwnerKind::ManagedOllamaRunner,
            crate::ObservedProcess::new(77, "bm-real-ollama", "/managed/bm-real-ollama")
                .with_start_identity("start-77")
                .with_executable_identity(ExecutableFileIdentity {
                    sha256: "same-name-untracked".to_string(),
                    byte_len: 1,
                    device: 1,
                    inode: 77,
                    unix_mode: 0o755,
                }),
        );

        let error = reject_untracked_listener(ManagedProcessKind::ManagedUpstream, bind, &report)
            .expect_err("untracked same-name process must fail closed");

        assert!(error.message().contains("diagnostic only"));
        assert!(error.message().contains("no exact launch receipt"));
    }

    #[test]
    fn log_tail_accepts_exact_budget_and_rejects_one_byte_over() {
        let path = std::env::temp_dir().join(format!(
            "bm-ollama-log-tail-{}-{}",
            std::process::id(),
            crate::runner::test_sequence()
        ));
        fs::write(&path, b"12345678").expect("exact log");
        assert_eq!(read_log_tail(&path, 8).expect("exact tail"), "12345678");

        let mut retained = File::open(&path).expect("retained log");
        fs::write(&path, b"123456789").expect("grown log");
        let error = read_bounded_log_tail(&mut retained, 8).expect_err("grown log must fail");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        fs::remove_file(path).expect("remove log fixture");
    }

    fn probe_server_with_response_len(response_len: usize) -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("probe listener");
        let bind = listener.local_addr().expect("listener address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("probe connection");
            let mut request = [0_u8; 256];
            let _ = stream.read(&mut request);
            let prefix = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";
            assert!(response_len >= prefix.len());
            stream.write_all(prefix).expect("response prefix");
            stream
                .write_all(&vec![b'x'; response_len - prefix.len()])
                .expect("response body");
        });
        (bind, server)
    }
}
