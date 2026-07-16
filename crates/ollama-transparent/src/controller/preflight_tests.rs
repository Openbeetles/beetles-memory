use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use super::ControllerCore;
use crate::process::ProcessManager;
use crate::runner::RunnerInstaller;
use crate::{
    inspect_executable_identity, ClassifyPortOwnerRequest, ExecutableFileIdentity,
    ManagedRunnerReport, ObservedProcess, OllamaTransparentConfig, OllamaTransparentController,
    OllamaTransparentMemoryAuthority, OllamaTransparentState, PortBindingReport, PortOwnerKind,
    PortOwnerObserver, PreflightBlockerCode,
};

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[test]
fn explicit_config_uses_ollama_app_transparent_ports_and_requires_stop_consent() {
    let config = explicit_config();

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
#[cfg(any(target_os = "linux", windows))]
fn preflight_builds_stop_plan_for_official_ollama_when_allowed() {
    let config = test_config(true);
    let official = official_process(&config, 301, "start-301");
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

    #[cfg(any(target_os = "linux", windows))]
    {
        assert!(report.accepted);
        assert_eq!(report.resulting_state, OllamaTransparentState::Disabled);
        let stop_plan = report.stop_plan.expect("official stop plan");
        assert_eq!(stop_plan.targets.len(), 1);
        assert_eq!(stop_plan.targets[0].bind, config.public_bind);
        assert_eq!(stop_plan.targets[0].process, official);
        assert!(stop_plan.allowed);
    }
    #[cfg(target_os = "macos")]
    {
        assert!(!report.accepted);
        assert!(report.stop_plan.is_none());
        assert!(report
            .blockers
            .iter()
            .any(|blocker| blocker.message.contains("cannot retain a stable authority")));
    }
}

#[test]
#[cfg(target_os = "macos")]
fn preflight_rejects_external_official_ollama_without_stable_process_authority() {
    let config = test_config(true);
    let official = official_process(&config, 301, "start-301");
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(config.public_bind, PortOwnerKind::OfficialOllama, official),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );

    let report = controller.preflight().expect("preflight report");

    assert!(!report.accepted);
    assert!(report.stop_plan.is_none());
    assert!(report
        .blockers
        .iter()
        .any(|blocker| blocker.message.contains("cannot retain a stable authority")));
}

#[test]
fn preflight_rejects_stop_plan_without_process_start_identity() {
    let config = test_config(true);
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::owned(
                config.public_bind,
                PortOwnerKind::OfficialOllama,
                ObservedProcess::new(302, "ollama", config.official_ollama_binary.clone()),
            ),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );

    let report = controller.preflight().expect("preflight report");

    assert!(!report.accepted);
    assert!(report.stop_plan.is_none());
    assert_eq!(
        report.blockers[0].code,
        PreflightBlockerCode::PublicPortOwnedByUnknownProcess
    );
}

#[test]
fn preflight_rejects_missing_gateway_front_binary() {
    let mut config = test_config(true);
    config.gateway_binary_path =
        std::env::temp_dir().join("beetle-memory-missing-bm-llm-gateway-for-preflight-test");
    let controller = controller(
        config.clone(),
        MockPorts::new(
            PortBindingReport::empty(config.public_bind),
            PortBindingReport::empty(config.upstream_bind),
        ),
        MockRunner::installed(),
        MockProcesses::default(),
    );

    let report = controller.preflight().expect("preflight report");

    assert!(!report.accepted);
    assert!(report
        .blockers
        .iter()
        .any(|blocker| blocker.code == PreflightBlockerCode::GatewayFrontUnavailable));
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
    let mut config = explicit_config();
    config.official_ollama_binary = std::env::current_exe().expect("test executable path");
    config.allow_stop_official_ollama = allow_stop_official_ollama;
    config
}

fn explicit_config() -> OllamaTransparentConfig {
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let data_dir = std::env::temp_dir().join(format!(
        "bm-ollama-preflight-{}-{sequence}",
        std::process::id()
    ));
    let executable = std::env::current_exe().expect("test executable path");
    let authority = OllamaTransparentMemoryAuthority::new(
        "test-owner",
        "test-agent",
        "test-channel",
        data_dir.join("store"),
    )
    .expect("test memory authority");
    OllamaTransparentConfig::new(&data_dir, executable, authority).expect("test transparent config")
}

fn official_process(
    config: &OllamaTransparentConfig,
    pid: u32,
    start_identity: &str,
) -> ObservedProcess {
    ObservedProcess::new(pid, "ollama", config.official_ollama_binary.clone())
        .with_start_identity(start_identity)
        .with_executable_identity(
            inspect_executable_identity(&config.official_ollama_binary)
                .expect("official executable identity"),
        )
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
}

impl MockRunner {
    fn installed() -> Self {
        Self {
            report: ManagedRunnerReport::installed(
                PathBuf::from("/Applications/Ollama.app/Contents/Resources/ollama"),
                PathBuf::from("/tmp/beetle-memory/ollama/bin/bm-real-ollama"),
                Some("sha256:test".to_string()),
            ),
        }
    }
}

impl RunnerInstaller for MockRunner {
    fn inspect(&self, _config: &OllamaTransparentConfig) -> crate::Result<ManagedRunnerReport> {
        Ok(self.report.clone())
    }

    fn ensure_installed(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> crate::Result<ManagedRunnerReport> {
        Ok(self.report.clone())
    }
}

#[derive(Default)]
struct MockProcesses {
    actions: Mutex<Vec<String>>,
}

impl ProcessManager for MockProcesses {
    fn inspect_managed_process_ownership(
        &self,
        _config: &OllamaTransparentConfig,
    ) -> crate::Result<crate::ManagedProcessOwnershipReport> {
        Ok(crate::ManagedProcessOwnershipReport::default())
    }

    fn stop_official_ollama(
        &self,
        _plan: &crate::OfficialOllamaStopPlan,
    ) -> crate::Result<Vec<crate::ProcessActionReport>> {
        self.actions
            .lock()
            .expect("actions")
            .push("stop_official".to_string());
        Ok(vec![])
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
        Ok(crate::ManagedProcessReport::started(
            crate::ManagedProcessKind::ManagedUpstream,
            Some(401),
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
        Ok(crate::ManagedProcessReport::started(
            crate::ManagedProcessKind::TransparentFront,
            Some(402),
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
        Ok(crate::ProcessActionReport::ok("open_app"))
    }
}
