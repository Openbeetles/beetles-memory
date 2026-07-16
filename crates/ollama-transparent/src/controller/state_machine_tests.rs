use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use super::ControllerCore;
#[cfg(any(target_os = "linux", windows))]
use crate::inspect_executable_identity;
use crate::process::ProcessManager;
use crate::runner::RunnerInstaller;
use crate::{
    ExecutableFileIdentity, ManagedRunnerReport, OllamaTransparentConfig,
    OllamaTransparentController, OllamaTransparentMemoryAuthority, OllamaTransparentState,
    PortBindingReport, PortOwnerKind, PortOwnerObserver, TransitionOutcome, TransitionStep,
};

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[test]
#[cfg(any(target_os = "linux", windows))]
fn enable_success_stops_official_starts_runner_front_and_opens_app() {
    let config = test_config(true);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::OfficialOllama,
                official_process(&config, 501, "start-501"),
            ),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );

    let report = controller
        .enable(crate::EnableOllamaTransparentRequest::default())
        .expect("enable report");

    assert_eq!(report.outcome, TransitionOutcome::Completed);
    assert_eq!(report.to_state, OllamaTransparentState::Active);
    assert!(report.rollback.is_none());
    assert_eq!(
        controller.processes().actions(),
        vec![
            "stop_official",
            "start_upstream",
            "probe_upstream",
            "start_front",
            "probe_public",
            "open_app",
        ]
    );
    assert_eq!(
        controller.runner().actions(),
        vec!["inspect_runner", "install_runner"]
    );
}

#[test]
#[cfg(any(target_os = "linux", windows))]
fn enable_failure_after_public_front_starts_rolls_back_front_upstream_and_app() {
    let config = test_config(true);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::OfficialOllama,
                official_process(&config, 601, "start-601"),
            ),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::fail_on(TransitionStep::ProbePublicFront),
    );

    let report = controller
        .enable(crate::EnableOllamaTransparentRequest::default())
        .expect("enable report");

    assert_eq!(report.outcome, TransitionOutcome::RolledBack);
    assert_eq!(report.to_state, OllamaTransparentState::Disabled);
    assert_eq!(
        report.failing_step.as_ref().expect("failing step").step,
        TransitionStep::ProbePublicFront
    );
    let rollback = report.rollback.expect("rollback report");
    assert!(rollback.attempted);
    assert!(rollback.completed);
    assert_eq!(
        rollback
            .steps
            .iter()
            .map(|step| step.step)
            .collect::<Vec<_>>(),
        vec![
            TransitionStep::StopTransparentFront,
            TransitionStep::StopManagedUpstream,
            TransitionStep::RestoreOfficialApp,
        ]
    );
    assert_eq!(
        controller.processes().actions(),
        vec![
            "stop_official",
            "start_upstream",
            "probe_upstream",
            "start_front",
            "probe_public",
            "stop_front",
            "stop_upstream",
            "open_app",
        ]
    );
    assert_eq!(
        controller.runner().actions(),
        vec!["inspect_runner", "install_runner"]
    );
}

#[test]
fn enable_failure_before_any_owned_process_starts_returns_disabled_without_rollback_steps() {
    let config = test_config(false);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::empty(config.public_bind),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::fail_on(TransitionStep::StartManagedUpstream),
    );

    let report = controller
        .enable(crate::EnableOllamaTransparentRequest::default())
        .expect("enable report");

    assert_eq!(report.outcome, TransitionOutcome::RolledBack);
    assert_eq!(report.to_state, OllamaTransparentState::Disabled);
    let rollback = report.rollback.expect("rollback report");
    assert!(!rollback.attempted);
    assert!(rollback.completed);
    assert!(rollback.steps.is_empty());
    assert_eq!(controller.processes().actions(), vec!["start_upstream"]);
}

#[test]
fn enable_is_idempotent_when_ports_are_already_transparently_owned() {
    let config = test_config(false);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::BeetleMemoryTransparentFront,
                crate::ObservedProcess::new(701, "bm-llm-gateway", "/tmp/front"),
            ),
            PortBindingReport::owned(
                config.upstream_bind,
                PortOwnerKind::ManagedOllamaRunner,
                crate::ObservedProcess::new(
                    702,
                    "bm-real-ollama",
                    config.managed_runner_path.clone(),
                ),
            ),
        ),
        MockRunner::installed(),
        MockProcesses::with_owned_processes(),
    );

    let report = controller
        .enable(crate::EnableOllamaTransparentRequest::default())
        .expect("enable report");

    assert_eq!(report.outcome, TransitionOutcome::Completed);
    assert_eq!(report.to_state, OllamaTransparentState::Active);
    assert_eq!(controller.processes().actions(), Vec::<String>::new());
    assert_eq!(controller.runner().actions(), vec!["inspect_runner"]);
}

#[test]
fn status_rejects_managed_looking_ports_without_persisted_launch_receipts_after_restart() {
    let config = test_config(false);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::BeetleMemoryTransparentFront,
                crate::ObservedProcess::new(801, "bm-llm-gateway", "/tmp/front"),
            ),
            PortBindingReport::owned(
                config.upstream_bind,
                PortOwnerKind::ManagedOllamaRunner,
                crate::ObservedProcess::new(
                    802,
                    "bm-real-ollama",
                    config.managed_runner_path.clone(),
                ),
            ),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );

    let status = controller.status().expect("status");

    assert_eq!(status.state, OllamaTransparentState::Degraded);
}

#[test]
fn disable_releases_public_front_stops_managed_upstream_and_restores_official_app() {
    let config = test_config(true);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::BeetleMemoryTransparentFront,
                crate::ObservedProcess::new(701, "bm-llm-gateway", "/tmp/front"),
            ),
            PortBindingReport::owned(
                config.upstream_bind,
                PortOwnerKind::ManagedOllamaRunner,
                crate::ObservedProcess::new(
                    702,
                    "bm-real-ollama",
                    config.managed_runner_path.clone(),
                ),
            ),
        ),
        MockRunner::installed(),
        MockProcesses::with_owned_processes(),
    );
    let enabled = controller
        .enable(crate::EnableOllamaTransparentRequest::default())
        .expect("establish active state through enable");
    assert_eq!(enabled.to_state, OllamaTransparentState::Active);

    let report = controller
        .disable(crate::DisableOllamaTransparentRequest::default())
        .expect("disable report");

    assert_eq!(report.outcome, TransitionOutcome::Completed);
    assert_eq!(report.to_state, OllamaTransparentState::Disabled);
    assert_eq!(
        controller.processes().actions(),
        vec!["stop_front", "stop_upstream", "open_app"]
    );
}

#[test]
fn concurrent_disable_is_rejected_while_enable_owns_transition_lease() {
    let gate = Arc::new(TransitionGate::default());
    let config = test_config(false);
    let controller = Arc::new(controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::empty(config.public_bind),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::blocking_at_start(gate.clone()),
    ));
    let enabling = {
        let controller = Arc::clone(&controller);
        std::thread::spawn(move || {
            controller.enable(crate::EnableOllamaTransparentRequest::default())
        })
    };
    gate.wait_until_entered();

    let error = controller
        .disable(crate::DisableOllamaTransparentRequest::default())
        .expect_err("disable must not overlap enable");

    assert_eq!(
        error.key(),
        crate::OllamaTransparentErrorKey::PreflightRejected
    );
    gate.release();
    enabling
        .join()
        .expect("enable thread")
        .expect("enable report");
}

#[test]
fn concurrent_second_enable_is_rejected_by_unique_transition_lease() {
    let gate = Arc::new(TransitionGate::default());
    let config = test_config(false);
    let controller = Arc::new(controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::empty(config.public_bind),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::blocking_at_start(gate.clone()),
    ));
    let first = {
        let controller = Arc::clone(&controller);
        std::thread::spawn(move || {
            controller.enable(crate::EnableOllamaTransparentRequest::default())
        })
    };
    gate.wait_until_entered();

    let error = controller
        .enable(crate::EnableOllamaTransparentRequest::default())
        .expect_err("second enable must not overlap first enable");

    assert_eq!(
        error.key(),
        crate::OllamaTransparentErrorKey::PreflightRejected
    );
    gate.release();
    first.join().expect("enable thread").expect("enable report");
}

#[test]
fn transition_lease_drop_releases_controller_after_unexpected_early_error() {
    let config = test_config(false);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::BeetleMemoryTransparentFront,
                crate::ObservedProcess::new(901, "bm-llm-gateway", "/tmp/front"),
            ),
            PortBindingReport::owned(
                config.upstream_bind,
                PortOwnerKind::ManagedOllamaRunner,
                crate::ObservedProcess::new(
                    902,
                    "bm-real-ollama",
                    config.managed_runner_path.clone(),
                ),
            ),
        ),
        MockRunner::installed(),
        MockProcesses::with_owned_processes_and_ownership_failure_once(),
    );

    let first = controller
        .enable(crate::EnableOllamaTransparentRequest::default())
        .expect_err("first ownership inspection must fail");
    assert_eq!(
        first.key(),
        crate::OllamaTransparentErrorKey::ProcessActionFailed
    );

    let second = controller
        .enable(crate::EnableOllamaTransparentRequest::default())
        .expect("RAII drop must release the failed transition lease");
    assert_eq!(second.outcome, TransitionOutcome::Completed);
    assert_eq!(second.from_state, OllamaTransparentState::Disabled);
    assert_eq!(second.to_state, OllamaTransparentState::Active);
}

fn controller(
    config: OllamaTransparentConfig,
    ports: MockPorts,
    runner: MockRunner,
    processes: MockProcesses,
) -> ControllerCore<MockPorts, MockRunner, MockProcesses> {
    ControllerCore::new(config, ports, runner, processes).expect("controller")
}

fn test_config(allow_stop_official_ollama: bool) -> OllamaTransparentConfig {
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let data_dir = std::env::temp_dir().join(format!(
        "bm-ollama-state-machine-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_dir).expect("test data dir");
    let data_dir = std::fs::canonicalize(data_dir).expect("canonical test data dir");
    let executable = std::env::current_exe().expect("test executable path");
    let authority = OllamaTransparentMemoryAuthority::new(
        "test-owner",
        "test-agent",
        "test-channel",
        data_dir.join("store"),
    )
    .expect("test memory authority");
    let mut config = OllamaTransparentConfig::new(&data_dir, &executable, authority)
        .expect("test transparent config");
    config.official_ollama_binary = executable;
    config.allow_stop_official_ollama = allow_stop_official_ollama;
    config
}

#[cfg(any(target_os = "linux", windows))]
fn official_process(
    config: &OllamaTransparentConfig,
    pid: u32,
    start_identity: &str,
) -> crate::ObservedProcess {
    crate::ObservedProcess::new(pid, "ollama", config.official_ollama_binary.clone())
        .with_start_identity(start_identity)
        .with_executable_identity(
            inspect_executable_identity(&config.official_ollama_binary)
                .expect("official executable identity"),
        )
}

struct MockPorts {
    public: PortBindingReport,
    upstream: PortBindingReport,
}

impl MockPorts {
    fn new(public: PortBindingReport, upstream: PortBindingReport) -> Self {
        Self { public, upstream }
    }
}

impl PortOwnerObserver for MockPorts {
    fn inspect(&self, bind: SocketAddr) -> crate::Result<PortBindingReport> {
        if bind == self.public.bind {
            Ok(self.public.clone())
        } else if bind == self.upstream.bind {
            Ok(self.upstream.clone())
        } else {
            Ok(PortBindingReport::empty(bind))
        }
    }
}

struct MockRunner {
    report: ManagedRunnerReport,
    actions: Mutex<Vec<String>>,
}

impl MockRunner {
    fn installed() -> Self {
        Self {
            report: ManagedRunnerReport::installed(
                PathBuf::from("/Applications/Ollama.app/Contents/Resources/ollama"),
                PathBuf::from("/tmp/beetle-memory/ollama/bin/bm-real-ollama"),
                Some("sha256:test".to_string()),
            ),
            actions: Mutex::new(Vec::new()),
        }
    }

    fn actions(&self) -> Vec<String> {
        self.actions.lock().expect("runner actions").clone()
    }
}

impl RunnerInstaller for MockRunner {
    fn inspect(&self, _config: &OllamaTransparentConfig) -> crate::Result<ManagedRunnerReport> {
        self.actions
            .lock()
            .expect("runner actions")
            .push("inspect_runner".to_string());
        Ok(self.report.clone())
    }

    fn ensure_installed(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> crate::Result<ManagedRunnerReport> {
        self.actions
            .lock()
            .expect("runner actions")
            .push("install_runner".to_string());
        Ok(self.report.clone())
    }
}

#[derive(Default)]
struct MockProcesses {
    actions: Mutex<Vec<String>>,
    owned: Mutex<(bool, bool)>,
    ownership_failures_remaining: Mutex<usize>,
    fail_on: Option<TransitionStep>,
    start_gate: Option<Arc<TransitionGate>>,
}

impl MockProcesses {
    fn with_owned_processes() -> Self {
        Self {
            actions: Mutex::new(Vec::new()),
            owned: Mutex::new((true, true)),
            ownership_failures_remaining: Mutex::new(0),
            fail_on: None,
            start_gate: None,
        }
    }

    fn with_owned_processes_and_ownership_failure_once() -> Self {
        Self {
            actions: Mutex::new(Vec::new()),
            owned: Mutex::new((true, true)),
            ownership_failures_remaining: Mutex::new(1),
            fail_on: None,
            start_gate: None,
        }
    }

    fn fail_on(step: TransitionStep) -> Self {
        Self {
            actions: Mutex::new(Vec::new()),
            owned: Mutex::new((false, false)),
            ownership_failures_remaining: Mutex::new(0),
            fail_on: Some(step),
            start_gate: None,
        }
    }

    fn blocking_at_start(start_gate: Arc<TransitionGate>) -> Self {
        Self {
            actions: Mutex::new(Vec::new()),
            owned: Mutex::new((false, false)),
            ownership_failures_remaining: Mutex::new(0),
            fail_on: None,
            start_gate: Some(start_gate),
        }
    }

    fn actions(&self) -> Vec<String> {
        self.actions.lock().expect("actions").clone()
    }

    fn maybe_fail(&self, step: TransitionStep) -> crate::Result<()> {
        if self.fail_on == Some(step) {
            Err(crate::OllamaTransparentError::process_action_failed(
                format!("{step:?} failed"),
            ))
        } else {
            Ok(())
        }
    }
}

impl ProcessManager for MockProcesses {
    fn inspect_managed_process_ownership(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> crate::Result<crate::ManagedProcessOwnershipReport> {
        let mut failures = self
            .ownership_failures_remaining
            .lock()
            .expect("ownership failures");
        if *failures > 0 {
            *failures -= 1;
            return Err(crate::OllamaTransparentError::process_action_failed(
                "deterministic ownership inspection failure",
            ));
        }
        let owned = *self.owned.lock().expect("owned processes");
        Ok(crate::ManagedProcessOwnershipReport {
            managed_upstream_authorized: owned.0,
            transparent_front_authorized: owned.1,
        })
    }

    fn stop_official_ollama(
        &self,
        _plan: &crate::OfficialOllamaStopPlan,
    ) -> crate::Result<Vec<crate::ProcessActionReport>> {
        self.actions
            .lock()
            .expect("actions")
            .push("stop_official".to_string());
        self.maybe_fail(TransitionStep::StopOfficialOllama)?;
        Ok(vec![crate::ProcessActionReport::ok("stop_official")])
    }

    fn start_managed_upstream(
        &self,
        _config: &OllamaTransparentConfig,
        _runner: &ManagedRunnerReport,
    ) -> crate::Result<crate::ManagedProcessReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("start_upstream".to_string());
        if let Some(gate) = &self.start_gate {
            gate.enter_and_wait();
        }
        self.maybe_fail(TransitionStep::StartManagedUpstream)?;
        self.owned.lock().expect("owned processes").0 = true;
        Ok(crate::ManagedProcessReport::started(
            crate::ManagedProcessKind::ManagedUpstream,
            Some(801),
        ))
    }

    fn probe_managed_upstream(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> crate::Result<crate::ProbeReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("probe_upstream".to_string());
        self.maybe_fail(TransitionStep::ProbeManagedUpstream)?;
        Ok(crate::ProbeReport::ok("upstream_api"))
    }

    fn start_transparent_front(
        &self,
        _config: &OllamaTransparentConfig,
        _gateway_executable: &ExecutableFileIdentity,
    ) -> crate::Result<crate::ManagedProcessReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("start_front".to_string());
        self.maybe_fail(TransitionStep::StartTransparentFront)?;
        self.owned.lock().expect("owned processes").1 = true;
        Ok(crate::ManagedProcessReport::started(
            crate::ManagedProcessKind::TransparentFront,
            Some(802),
        ))
    }

    fn probe_public_front(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> crate::Result<crate::ProbeReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("probe_public".to_string());
        self.maybe_fail(TransitionStep::ProbePublicFront)?;
        Ok(crate::ProbeReport::ok("public_api"))
    }

    fn stop_transparent_front(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> crate::Result<crate::ProcessActionReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("stop_front".to_string());
        self.maybe_fail(TransitionStep::StopTransparentFront)?;
        self.owned.lock().expect("owned processes").1 = false;
        Ok(crate::ProcessActionReport::ok("stop_front"))
    }

    fn stop_managed_upstream(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> crate::Result<crate::ProcessActionReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("stop_upstream".to_string());
        self.maybe_fail(TransitionStep::StopManagedUpstream)?;
        self.owned.lock().expect("owned processes").0 = false;
        Ok(crate::ProcessActionReport::ok("stop_upstream"))
    }

    fn open_official_app(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> crate::Result<crate::ProcessActionReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("open_app".to_string());
        self.maybe_fail(TransitionStep::RestoreOfficialApp)?;
        Ok(crate::ProcessActionReport::ok("open_app"))
    }
}

#[derive(Default)]
struct TransitionGate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl TransitionGate {
    fn enter_and_wait(&self) {
        let mut state = self.state.lock().expect("transition gate");
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            state = self.changed.wait(state).expect("transition gate wait");
        }
    }

    fn wait_until_entered(&self) {
        let mut state = self.state.lock().expect("transition gate");
        while !state.0 {
            state = self.changed.wait(state).expect("transition gate wait");
        }
    }

    fn release(&self) {
        let mut state = self.state.lock().expect("transition gate");
        state.1 = true;
        self.changed.notify_all();
    }
}
