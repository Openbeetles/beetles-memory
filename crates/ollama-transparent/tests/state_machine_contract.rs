use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Mutex;

use bm_ollama_transparent::{
    ManagedRunnerReport, OllamaTransparentConfig, OllamaTransparentController,
    OllamaTransparentState, PortBindingReport, PortOwnerKind, PortOwnerObserver, ProcessManager,
    RunnerInstaller, TransitionOutcome, TransitionStep, TransparentController,
};

#[test]
fn enable_success_stops_official_starts_runner_front_and_opens_app() {
    let config = test_config(true);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::OfficialOllama,
                bm_ollama_transparent::ObservedProcess::new(
                    501,
                    "ollama",
                    config.official_ollama_binary.clone(),
                ),
            ),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );

    let report = controller
        .enable(bm_ollama_transparent::EnableOllamaTransparentRequest::default())
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
fn enable_failure_after_public_front_starts_rolls_back_front_upstream_and_app() {
    let config = test_config(true);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::OfficialOllama,
                bm_ollama_transparent::ObservedProcess::new(
                    601,
                    "ollama",
                    config.official_ollama_binary.clone(),
                ),
            ),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::fail_on(TransitionStep::ProbePublicFront),
    );

    let report = controller
        .enable(bm_ollama_transparent::EnableOllamaTransparentRequest::default())
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
        .enable(bm_ollama_transparent::EnableOllamaTransparentRequest::default())
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
                bm_ollama_transparent::ObservedProcess::new(701, "bm-llm-gateway", "/tmp/front"),
            ),
            PortBindingReport::owned(
                config.upstream_bind,
                PortOwnerKind::ManagedOllamaRunner,
                bm_ollama_transparent::ObservedProcess::new(
                    702,
                    "bm-real-ollama",
                    config.managed_runner_path.clone(),
                ),
            ),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );

    let report = controller
        .enable(bm_ollama_transparent::EnableOllamaTransparentRequest::default())
        .expect("enable report");

    assert_eq!(report.outcome, TransitionOutcome::Completed);
    assert_eq!(report.to_state, OllamaTransparentState::Active);
    assert_eq!(controller.processes().actions(), Vec::<String>::new());
    assert_eq!(controller.runner().actions(), vec!["inspect_runner"]);
}

#[test]
fn status_reconciles_active_from_observed_ports_after_controller_restart() {
    let config = test_config(false);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::BeetleMemoryTransparentFront,
                bm_ollama_transparent::ObservedProcess::new(801, "bm-llm-gateway", "/tmp/front"),
            ),
            PortBindingReport::owned(
                config.upstream_bind,
                PortOwnerKind::ManagedOllamaRunner,
                bm_ollama_transparent::ObservedProcess::new(
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

    assert_eq!(status.state, OllamaTransparentState::Active);
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
                bm_ollama_transparent::ObservedProcess::new(701, "bm-llm-gateway", "/tmp/front"),
            ),
            PortBindingReport::owned(
                config.upstream_bind,
                PortOwnerKind::ManagedOllamaRunner,
                bm_ollama_transparent::ObservedProcess::new(
                    702,
                    "bm-real-ollama",
                    config.managed_runner_path.clone(),
                ),
            ),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );
    controller.force_state_for_test(OllamaTransparentState::Active);

    let report = controller
        .disable(bm_ollama_transparent::DisableOllamaTransparentRequest::default())
        .expect("disable report");

    assert_eq!(report.outcome, TransitionOutcome::Completed);
    assert_eq!(report.to_state, OllamaTransparentState::Disabled);
    assert_eq!(
        controller.processes().actions(),
        vec!["stop_front", "stop_upstream", "open_app"]
    );
}

fn controller(
    config: OllamaTransparentConfig,
    ports: MockPorts,
    runner: MockRunner,
    processes: MockProcesses,
) -> TransparentController<MockPorts, MockRunner, MockProcesses> {
    TransparentController::new(config, ports, runner, processes).expect("controller")
}

fn test_config(allow_stop_official_ollama: bool) -> OllamaTransparentConfig {
    OllamaTransparentConfig {
        app_bundle_path: PathBuf::from("/Applications/Ollama.app"),
        official_ollama_binary: PathBuf::from("/Applications/Ollama.app/Contents/Resources/ollama"),
        managed_runner_path: PathBuf::from("/tmp/beetle-memory/ollama/bin/bm-real-ollama"),
        public_bind: loopback(11434),
        upstream_bind: loopback(11435),
        allow_stop_official_ollama,
        ..OllamaTransparentConfig::default()
    }
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
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
    fn inspect(&self, bind: SocketAddr) -> bm_ollama_transparent::Result<PortBindingReport> {
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
                Some("fnv1a64:test".to_string()),
            ),
            actions: Mutex::new(Vec::new()),
        }
    }

    fn actions(&self) -> Vec<String> {
        self.actions.lock().expect("runner actions").clone()
    }
}

impl RunnerInstaller for MockRunner {
    fn inspect(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<ManagedRunnerReport> {
        self.actions
            .lock()
            .expect("runner actions")
            .push("inspect_runner".to_string());
        Ok(self.report.clone())
    }

    fn ensure_installed(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<ManagedRunnerReport> {
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
    fail_on: Option<TransitionStep>,
}

impl MockProcesses {
    fn fail_on(step: TransitionStep) -> Self {
        Self {
            actions: Mutex::new(Vec::new()),
            fail_on: Some(step),
        }
    }

    fn actions(&self) -> Vec<String> {
        self.actions.lock().expect("actions").clone()
    }

    fn maybe_fail(&self, step: TransitionStep) -> bm_ollama_transparent::Result<()> {
        if self.fail_on == Some(step) {
            Err(
                bm_ollama_transparent::OllamaTransparentError::process_action_failed(format!(
                    "{step:?} failed"
                )),
            )
        } else {
            Ok(())
        }
    }
}

impl ProcessManager for MockProcesses {
    fn stop_official_ollama(
        &self,
        _plan: &bm_ollama_transparent::OfficialOllamaStopPlan,
    ) -> bm_ollama_transparent::Result<Vec<bm_ollama_transparent::ProcessActionReport>> {
        self.actions
            .lock()
            .expect("actions")
            .push("stop_official".to_string());
        self.maybe_fail(TransitionStep::StopOfficialOllama)?;
        Ok(vec![bm_ollama_transparent::ProcessActionReport::ok(
            "stop_official",
        )])
    }

    fn start_managed_upstream(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<bm_ollama_transparent::ManagedProcessReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("start_upstream".to_string());
        self.maybe_fail(TransitionStep::StartManagedUpstream)?;
        Ok(bm_ollama_transparent::ManagedProcessReport::started(
            bm_ollama_transparent::ManagedProcessKind::ManagedUpstream,
            Some(801),
        ))
    }

    fn probe_managed_upstream(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<bm_ollama_transparent::ProbeReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("probe_upstream".to_string());
        self.maybe_fail(TransitionStep::ProbeManagedUpstream)?;
        Ok(bm_ollama_transparent::ProbeReport::ok("upstream_api"))
    }

    fn start_transparent_front(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<bm_ollama_transparent::ManagedProcessReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("start_front".to_string());
        self.maybe_fail(TransitionStep::StartTransparentFront)?;
        Ok(bm_ollama_transparent::ManagedProcessReport::started(
            bm_ollama_transparent::ManagedProcessKind::TransparentFront,
            Some(802),
        ))
    }

    fn probe_public_front(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<bm_ollama_transparent::ProbeReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("probe_public".to_string());
        self.maybe_fail(TransitionStep::ProbePublicFront)?;
        Ok(bm_ollama_transparent::ProbeReport::ok("public_api"))
    }

    fn stop_transparent_front(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<bm_ollama_transparent::ProcessActionReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("stop_front".to_string());
        self.maybe_fail(TransitionStep::StopTransparentFront)?;
        Ok(bm_ollama_transparent::ProcessActionReport::ok("stop_front"))
    }

    fn stop_managed_upstream(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<bm_ollama_transparent::ProcessActionReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("stop_upstream".to_string());
        self.maybe_fail(TransitionStep::StopManagedUpstream)?;
        Ok(bm_ollama_transparent::ProcessActionReport::ok(
            "stop_upstream",
        ))
    }

    fn open_official_app(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<bm_ollama_transparent::ProcessActionReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("open_app".to_string());
        self.maybe_fail(TransitionStep::RestoreOfficialApp)?;
        Ok(bm_ollama_transparent::ProcessActionReport::ok("open_app"))
    }
}
