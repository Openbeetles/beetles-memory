use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Mutex;

use bm_ollama_transparent::{
    ClassifyPortOwnerRequest, ManagedRunnerReport, ObservedProcess, OllamaTransparentConfig,
    OllamaTransparentController, OllamaTransparentState, PortBindingReport, PortOwnerKind,
    PortOwnerObserver, PreflightBlockerCode, ProcessManager, RunnerInstaller,
    TransparentController,
};

#[test]
fn default_config_uses_ollama_app_transparent_ports_and_requires_stop_consent() {
    let config = OllamaTransparentConfig::default();

    assert_eq!(config.public_bind, loopback(11434));
    assert_eq!(config.upstream_bind, loopback(11435));
    assert!(config.open_app_after_enable);
    assert!(config.restore_official_after_disable);
    assert!(!config.allow_stop_official_ollama);
    assert!(config.app_bundle_path.ends_with("Ollama.app"));
    assert!(config
        .official_ollama_binary
        .ends_with("Contents/Resources/ollama"));
    assert!(config.managed_runner_path.ends_with("bm-real-ollama"));
}

#[test]
fn port_owner_classifier_recognizes_empty_official_transparent_managed_and_unknown() {
    let config = test_config(false);
    let classifier = config.port_owner_classifier();

    assert_eq!(
        classifier.classify(ClassifyPortOwnerRequest::no_listener(config.public_bind)),
        PortOwnerKind::NoListener
    );
    assert_eq!(
        classifier.classify(ClassifyPortOwnerRequest::process(
            config.public_bind,
            ObservedProcess::new(101, "ollama", config.official_ollama_binary.clone()),
        )),
        PortOwnerKind::OfficialOllama
    );
    assert_eq!(
        classifier.classify(ClassifyPortOwnerRequest::process(
            config.public_bind,
            ObservedProcess::new(102, "bm-llm-gateway", "/tmp/bm-llm-gateway"),
        )),
        PortOwnerKind::BeetleMemoryTransparentFront
    );
    assert_eq!(
        classifier.classify(ClassifyPortOwnerRequest::process(
            config.upstream_bind,
            ObservedProcess::new(103, "bm-real-ollama", config.managed_runner_path.clone()),
        )),
        PortOwnerKind::ManagedOllamaRunner
    );
    assert_eq!(
        classifier.classify(ClassifyPortOwnerRequest::process(
            config.public_bind,
            ObservedProcess::new(104, "other-daemon", "/tmp/other-daemon"),
        )),
        PortOwnerKind::Unknown
    );
}

#[test]
fn preflight_rejects_unknown_public_port_owner_without_stop_plan() {
    let config = test_config(true);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::Unknown,
                ObservedProcess::new(200, "unknown", "/tmp/unknown"),
            ),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );

    let report = controller.preflight().expect("preflight report");

    assert!(!report.accepted);
    assert_eq!(
        report.resulting_state,
        OllamaTransparentState::PreflightFailed
    );
    assert_eq!(
        report.blockers[0].code,
        PreflightBlockerCode::PublicPortOwnedByUnknownProcess
    );
    assert!(report.stop_plan.is_none());
}

#[test]
fn preflight_rejects_official_ollama_owner_until_user_allows_stop() {
    let config = test_config(false);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::OfficialOllama,
                ObservedProcess::new(300, "ollama", config.official_ollama_binary.clone()),
            ),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );

    let report = controller.preflight().expect("preflight report");

    assert!(!report.accepted);
    assert_eq!(
        report.resulting_state,
        OllamaTransparentState::PreflightFailed
    );
    assert_eq!(
        report.blockers[0].code,
        PreflightBlockerCode::OfficialOllamaStopNotAllowed
    );
    assert!(report.stop_plan.is_none());
}

#[test]
fn preflight_builds_stop_plan_for_official_ollama_when_allowed() {
    let config = test_config(true);
    let official = ObservedProcess::new(301, "ollama", config.official_ollama_binary.clone());
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::OfficialOllama,
                official.clone(),
            ),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );

    let report = controller.preflight().expect("preflight report");

    assert!(report.accepted);
    assert_eq!(report.resulting_state, OllamaTransparentState::Disabled);
    let stop_plan = report.stop_plan.expect("official stop plan");
    assert_eq!(stop_plan.processes, vec![official]);
    assert!(stop_plan.allowed);
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
}

impl MockRunner {
    fn installed() -> Self {
        Self {
            report: ManagedRunnerReport::installed(
                PathBuf::from("/Applications/Ollama.app/Contents/Resources/ollama"),
                PathBuf::from("/tmp/beetle-memory/ollama/bin/bm-real-ollama"),
                Some("fnv1a64:test".to_string()),
            ),
        }
    }
}

impl RunnerInstaller for MockRunner {
    fn inspect(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<ManagedRunnerReport> {
        Ok(self.report.clone())
    }

    fn ensure_installed(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<ManagedRunnerReport> {
        Ok(self.report.clone())
    }
}

#[derive(Default)]
struct MockProcesses {
    actions: Mutex<Vec<String>>,
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
        Ok(vec![])
    }

    fn start_managed_upstream(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> bm_ollama_transparent::Result<bm_ollama_transparent::ManagedProcessReport> {
        self.actions
            .lock()
            .expect("actions")
            .push("start_upstream".to_string());
        Ok(bm_ollama_transparent::ManagedProcessReport::started(
            bm_ollama_transparent::ManagedProcessKind::ManagedUpstream,
            Some(401),
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
        Ok(bm_ollama_transparent::ManagedProcessReport::started(
            bm_ollama_transparent::ManagedProcessKind::TransparentFront,
            Some(402),
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
        Ok(bm_ollama_transparent::ProcessActionReport::ok("open_app"))
    }
}
