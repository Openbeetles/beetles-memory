use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::{
    GatewayFrontReport, ManagedRunnerReport, OfficialOllamaStopPlan, OllamaAppReport,
    OllamaTransparentConfig, OllamaTransparentError, OllamaTransparentPreflightReport,
    OllamaTransparentState, OllamaTransparentStatus, OllamaTransparentTransitionReport,
    PortBindingReport, PortOwnerKind, PortOwnerObserver, PreflightBlocker, PreflightBlockerCode,
    ProcessManager, Result, RollbackReport, RunnerInstaller, TransitionOutcome, TransitionStep,
    TransitionStepReport,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnableOllamaTransparentRequest {
    pub open_app: Option<bool>,
    pub allow_stop_official_ollama: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisableOllamaTransparentRequest {
    pub restore_official_app: Option<bool>,
}

pub trait OllamaTransparentController {
    fn preflight(&self) -> Result<OllamaTransparentPreflightReport>;

    fn enable(
        &self,
        request: EnableOllamaTransparentRequest,
    ) -> Result<OllamaTransparentTransitionReport>;

    fn disable(
        &self,
        request: DisableOllamaTransparentRequest,
    ) -> Result<OllamaTransparentTransitionReport>;

    fn status(&self) -> Result<OllamaTransparentStatus>;

    fn open_app(&self) -> Result<crate::ProcessActionReport>;
}

pub struct TransparentController<P, R, M> {
    config: OllamaTransparentConfig,
    ports: P,
    runner: R,
    processes: M,
    state: Mutex<ControllerState>,
}

#[derive(Clone, Debug)]
struct ControllerState {
    state: OllamaTransparentState,
    last_transition: Option<OllamaTransparentTransitionReport>,
}

#[derive(Clone, Copy, Debug, Default)]
struct EnableProgress {
    official_stopped: bool,
    upstream_started: bool,
    public_front_started: bool,
}

impl<P, R, M> TransparentController<P, R, M>
where
    P: PortOwnerObserver,
    R: RunnerInstaller,
    M: ProcessManager,
{
    pub fn new(config: OllamaTransparentConfig, ports: P, runner: R, processes: M) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            ports,
            runner,
            processes,
            state: Mutex::new(ControllerState {
                state: OllamaTransparentState::Disabled,
                last_transition: None,
            }),
        })
    }

    pub fn config(&self) -> &OllamaTransparentConfig {
        &self.config
    }

    pub fn ports(&self) -> &P {
        &self.ports
    }

    pub fn runner(&self) -> &R {
        &self.runner
    }

    pub fn processes(&self) -> &M {
        &self.processes
    }

    #[doc(hidden)]
    pub fn force_state_for_test(&self, state: OllamaTransparentState) {
        self.state.lock().expect("controller state").state = state;
    }

    fn set_state(&self, state: OllamaTransparentState) {
        self.state.lock().expect("controller state").state = state;
    }

    fn remember_transition(&self, report: OllamaTransparentTransitionReport) {
        let mut state = self.state.lock().expect("controller state");
        state.state = report.to_state;
        state.last_transition = Some(report);
    }
}

impl<P, R, M> OllamaTransparentController for TransparentController<P, R, M>
where
    P: PortOwnerObserver,
    R: RunnerInstaller,
    M: ProcessManager,
{
    fn preflight(&self) -> Result<OllamaTransparentPreflightReport> {
        let report = build_preflight_report(&self.config, &self.ports, &self.runner)?;
        if !report.accepted {
            self.set_state(OllamaTransparentState::PreflightFailed);
        }
        Ok(report)
    }

    fn enable(
        &self,
        request: EnableOllamaTransparentRequest,
    ) -> Result<OllamaTransparentTransitionReport> {
        let from_state = self.state.lock().expect("controller state").state;
        self.set_state(OllamaTransparentState::Enabling);
        let mut effective_config = self.config.clone();
        if request.allow_stop_official_ollama {
            effective_config.allow_stop_official_ollama = true;
        }
        let mut steps = Vec::new();
        let mut progress = EnableProgress::default();

        let preflight = match build_preflight_report(&effective_config, &self.ports, &self.runner) {
            Ok(report) => report,
            Err(error) => {
                let failed =
                    TransitionStepReport::failed(TransitionStep::Preflight, error.to_string());
                let report = OllamaTransparentTransitionReport {
                    from_state,
                    to_state: OllamaTransparentState::PreflightFailed,
                    outcome: TransitionOutcome::Rejected,
                    steps: vec![failed.clone()],
                    failing_step: Some(failed),
                    rollback: None,
                };
                self.remember_transition(report.clone());
                return Ok(report);
            }
        };
        if !preflight.accepted {
            let failed = TransitionStepReport::failed(
                TransitionStep::Preflight,
                preflight
                    .blockers
                    .iter()
                    .map(|blocker| blocker.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            );
            let report = OllamaTransparentTransitionReport {
                from_state,
                to_state: OllamaTransparentState::PreflightFailed,
                outcome: TransitionOutcome::Rejected,
                steps: vec![failed.clone()],
                failing_step: Some(failed),
                rollback: None,
            };
            self.remember_transition(report.clone());
            return Ok(report);
        }
        steps.push(TransitionStepReport::ok(TransitionStep::Preflight));
        if preflight.resulting_state == OllamaTransparentState::Active {
            let report = OllamaTransparentTransitionReport::completed(
                from_state,
                OllamaTransparentState::Active,
                steps,
            );
            self.remember_transition(report.clone());
            return Ok(report);
        }

        if let Some(stop_plan) = preflight.stop_plan.as_ref() {
            match self.processes.stop_official_ollama(stop_plan) {
                Ok(_) => {
                    progress.official_stopped = true;
                    steps.push(TransitionStepReport::ok(TransitionStep::StopOfficialOllama));
                }
                Err(error) => {
                    return Ok(self.finish_failed_enable(
                        from_state,
                        steps,
                        TransitionStep::StopOfficialOllama,
                        error,
                        progress,
                    ));
                }
            }
        }

        match self.runner.ensure_installed(&effective_config) {
            Ok(_) => steps.push(TransitionStepReport::ok(
                TransitionStep::InstallManagedRunner,
            )),
            Err(error) => {
                return Ok(self.finish_failed_enable(
                    from_state,
                    steps,
                    TransitionStep::InstallManagedRunner,
                    error,
                    progress,
                ));
            }
        }

        match self.processes.start_managed_upstream(&effective_config) {
            Ok(_) => {
                progress.upstream_started = true;
                steps.push(TransitionStepReport::ok(
                    TransitionStep::StartManagedUpstream,
                ));
            }
            Err(error) => {
                return Ok(self.finish_failed_enable(
                    from_state,
                    steps,
                    TransitionStep::StartManagedUpstream,
                    error,
                    progress,
                ));
            }
        }

        match self.processes.probe_managed_upstream(&effective_config) {
            Ok(_) => steps.push(TransitionStepReport::ok(
                TransitionStep::ProbeManagedUpstream,
            )),
            Err(error) => {
                return Ok(self.finish_failed_enable(
                    from_state,
                    steps,
                    TransitionStep::ProbeManagedUpstream,
                    error,
                    progress,
                ));
            }
        }

        match self.processes.start_transparent_front(&effective_config) {
            Ok(_) => {
                progress.public_front_started = true;
                steps.push(TransitionStepReport::ok(
                    TransitionStep::StartTransparentFront,
                ));
            }
            Err(error) => {
                return Ok(self.finish_failed_enable(
                    from_state,
                    steps,
                    TransitionStep::StartTransparentFront,
                    error,
                    progress,
                ));
            }
        }

        match self.processes.probe_public_front(&effective_config) {
            Ok(_) => steps.push(TransitionStepReport::ok(TransitionStep::ProbePublicFront)),
            Err(error) => {
                return Ok(self.finish_failed_enable(
                    from_state,
                    steps,
                    TransitionStep::ProbePublicFront,
                    error,
                    progress,
                ));
            }
        }

        if request
            .open_app
            .unwrap_or(effective_config.open_app_after_enable)
        {
            match self.processes.open_official_app(&effective_config) {
                Ok(_) => steps.push(TransitionStepReport::ok(TransitionStep::OpenOfficialApp)),
                Err(error) => {
                    return Ok(self.finish_failed_enable(
                        from_state,
                        steps,
                        TransitionStep::OpenOfficialApp,
                        error,
                        progress,
                    ));
                }
            }
        }

        let report = OllamaTransparentTransitionReport::completed(
            from_state,
            OllamaTransparentState::Active,
            steps,
        );
        self.remember_transition(report.clone());
        Ok(report)
    }

    fn disable(
        &self,
        request: DisableOllamaTransparentRequest,
    ) -> Result<OllamaTransparentTransitionReport> {
        let from_state = self.state.lock().expect("controller state").state;
        self.set_state(OllamaTransparentState::Disabling);
        let mut steps = Vec::new();
        let mut failing_step = None;

        match self.processes.stop_transparent_front(&self.config) {
            Ok(_) => steps.push(TransitionStepReport::ok(
                TransitionStep::StopTransparentFront,
            )),
            Err(error) => {
                let failed = TransitionStepReport::failed(
                    TransitionStep::StopTransparentFront,
                    error.to_string(),
                );
                steps.push(failed.clone());
                failing_step = Some(failed);
            }
        }
        match self.processes.stop_managed_upstream(&self.config) {
            Ok(_) => steps.push(TransitionStepReport::ok(
                TransitionStep::StopManagedUpstream,
            )),
            Err(error) => {
                let failed = TransitionStepReport::failed(
                    TransitionStep::StopManagedUpstream,
                    error.to_string(),
                );
                steps.push(failed.clone());
                failing_step.get_or_insert(failed);
            }
        }
        if request
            .restore_official_app
            .unwrap_or(self.config.restore_official_after_disable)
        {
            match self.processes.open_official_app(&self.config) {
                Ok(_) => steps.push(TransitionStepReport::ok(TransitionStep::RestoreOfficialApp)),
                Err(error) => {
                    let failed = TransitionStepReport::failed(
                        TransitionStep::RestoreOfficialApp,
                        error.to_string(),
                    );
                    steps.push(failed.clone());
                    failing_step.get_or_insert(failed);
                }
            }
        }

        let (to_state, outcome) = if failing_step.is_some() {
            (OllamaTransparentState::Degraded, TransitionOutcome::Failed)
        } else {
            (
                OllamaTransparentState::Disabled,
                TransitionOutcome::Completed,
            )
        };
        let report = OllamaTransparentTransitionReport {
            from_state,
            to_state,
            outcome,
            steps,
            failing_step,
            rollback: None,
        };
        self.remember_transition(report.clone());
        Ok(report)
    }

    fn status(&self) -> Result<OllamaTransparentStatus> {
        let public_port = self.ports.inspect(self.config.public_bind)?;
        let upstream_port = self.ports.inspect(self.config.upstream_bind)?;
        let managed_runner = self.runner.inspect(&self.config)?;
        let state = {
            let mut state = self.state.lock().expect("controller state");
            state.state = reconcile_observed_state(state.state, &public_port, &upstream_port);
            state.clone()
        };
        Ok(OllamaTransparentStatus {
            state: state.state,
            public_port: public_port.clone(),
            upstream_port,
            app: OllamaAppReport::from_config(&self.config),
            managed_runner,
            gateway_front: GatewayFrontReport::from_public_port(&public_port),
            last_transition: state.last_transition,
        })
    }

    fn open_app(&self) -> Result<crate::ProcessActionReport> {
        self.processes.open_official_app(&self.config)
    }
}

fn reconcile_observed_state(
    current: OllamaTransparentState,
    public_port: &PortBindingReport,
    upstream_port: &PortBindingReport,
) -> OllamaTransparentState {
    let public_owned = public_port.owner == PortOwnerKind::BeetleMemoryTransparentFront;
    let upstream_owned = upstream_port.owner == PortOwnerKind::ManagedOllamaRunner;
    match (public_owned, upstream_owned) {
        (true, true) => OllamaTransparentState::Active,
        (true, false) | (false, true) => {
            if matches!(
                current,
                OllamaTransparentState::Enabling
                    | OllamaTransparentState::Disabling
                    | OllamaTransparentState::RollingBack
            ) {
                current
            } else {
                OllamaTransparentState::Degraded
            }
        }
        (false, false) => {
            if matches!(
                current,
                OllamaTransparentState::Enabling
                    | OllamaTransparentState::Disabling
                    | OllamaTransparentState::RollingBack
                    | OllamaTransparentState::PreflightFailed
            ) {
                current
            } else {
                OllamaTransparentState::Disabled
            }
        }
    }
}

impl<P, R, M> TransparentController<P, R, M>
where
    P: PortOwnerObserver,
    R: RunnerInstaller,
    M: ProcessManager,
{
    fn finish_failed_enable(
        &self,
        from_state: OllamaTransparentState,
        mut steps: Vec<TransitionStepReport>,
        failing_step: TransitionStep,
        error: OllamaTransparentError,
        progress: EnableProgress,
    ) -> OllamaTransparentTransitionReport {
        let failed = TransitionStepReport::failed(failing_step, error.to_string());
        steps.push(failed.clone());
        self.set_state(OllamaTransparentState::RollingBack);
        let rollback = self.rollback_enable_failure(progress);
        let to_state = if rollback.completed {
            OllamaTransparentState::Disabled
        } else {
            OllamaTransparentState::Degraded
        };
        let report = OllamaTransparentTransitionReport {
            from_state,
            to_state,
            outcome: if rollback.completed {
                TransitionOutcome::RolledBack
            } else {
                TransitionOutcome::Failed
            },
            steps,
            failing_step: Some(failed),
            rollback: Some(rollback),
        };
        self.remember_transition(report.clone());
        report
    }

    fn rollback_enable_failure(&self, progress: EnableProgress) -> RollbackReport {
        let mut steps = Vec::new();
        if progress.public_front_started {
            match self.processes.stop_transparent_front(&self.config) {
                Ok(_) => steps.push(TransitionStepReport::ok(
                    TransitionStep::StopTransparentFront,
                )),
                Err(error) => steps.push(TransitionStepReport::failed(
                    TransitionStep::StopTransparentFront,
                    error.to_string(),
                )),
            }
        }
        if progress.upstream_started {
            match self.processes.stop_managed_upstream(&self.config) {
                Ok(_) => steps.push(TransitionStepReport::ok(
                    TransitionStep::StopManagedUpstream,
                )),
                Err(error) => steps.push(TransitionStepReport::failed(
                    TransitionStep::StopManagedUpstream,
                    error.to_string(),
                )),
            }
        }
        if progress.official_stopped && self.config.restore_official_after_disable {
            match self.processes.open_official_app(&self.config) {
                Ok(_) => steps.push(TransitionStepReport::ok(TransitionStep::RestoreOfficialApp)),
                Err(error) => steps.push(TransitionStepReport::failed(
                    TransitionStep::RestoreOfficialApp,
                    error.to_string(),
                )),
            }
        }
        let attempted = !steps.is_empty();
        let completed = steps.iter().all(|step| step.ok);
        RollbackReport {
            attempted,
            completed,
            steps,
        }
    }
}

fn build_preflight_report<P, R>(
    config: &OllamaTransparentConfig,
    ports: &P,
    runner: &R,
) -> Result<OllamaTransparentPreflightReport>
where
    P: PortOwnerObserver,
    R: RunnerInstaller,
{
    config.validate()?;
    let public_port = ports.inspect(config.public_bind)?;
    let upstream_port = ports.inspect(config.upstream_bind)?;
    let managed_runner = runner.inspect(config)?;
    let mut blockers = Vec::new();
    let mut stop_plan = None;

    match public_port.owner {
        PortOwnerKind::NoListener | PortOwnerKind::BeetleMemoryTransparentFront => {}
        PortOwnerKind::OfficialOllama => {
            let Some(process) = public_port.process.clone() else {
                blockers.push(PreflightBlocker::new(
                    PreflightBlockerCode::PublicPortOwnedByUnknownProcess,
                    "official Ollama owner was reported without process detail",
                ));
                return Ok(report_from_parts(
                    public_port,
                    upstream_port,
                    managed_runner,
                    stop_plan,
                    blockers,
                ));
            };
            if config.allow_stop_official_ollama {
                stop_plan = Some(OfficialOllamaStopPlan {
                    allowed: true,
                    processes: vec![process],
                    reason: "official Ollama owns the public transparent port and user allowed stopping it".to_string(),
                });
            } else {
                blockers.push(PreflightBlocker::new(
                    PreflightBlockerCode::OfficialOllamaStopNotAllowed,
                    "official Ollama owns 11434 but allow_stop_official_ollama is false",
                ));
            }
        }
        PortOwnerKind::ManagedOllamaRunner => blockers.push(PreflightBlocker::new(
            PreflightBlockerCode::PublicPortOwnedByManagedRunner,
            "managed upstream runner must not own the public Ollama App port",
        )),
        PortOwnerKind::Unknown => blockers.push(PreflightBlocker::new(
            PreflightBlockerCode::PublicPortOwnedByUnknownProcess,
            "public transparent port is owned by an unknown process",
        )),
    }

    match upstream_port.owner {
        PortOwnerKind::NoListener | PortOwnerKind::ManagedOllamaRunner => {}
        _ => blockers.push(PreflightBlocker::new(
            PreflightBlockerCode::UpstreamPortUnavailable,
            "managed upstream port is owned by a non-managed process",
        )),
    }

    if !managed_runner.source_exists {
        blockers.push(PreflightBlocker::new(
            PreflightBlockerCode::ManagedRunnerUnavailable,
            "official Ollama binary is missing; managed runner cannot be installed",
        ));
    }
    if !config.gateway_binary_path.is_file() {
        blockers.push(PreflightBlocker::new(
            PreflightBlockerCode::GatewayFrontUnavailable,
            format!(
                "bm-llm-gateway transparent front binary is missing: {}",
                config.gateway_binary_path.display()
            ),
        ));
    }

    Ok(report_from_parts(
        public_port,
        upstream_port,
        managed_runner,
        stop_plan,
        blockers,
    ))
}

fn report_from_parts(
    public_port: PortBindingReport,
    upstream_port: PortBindingReport,
    managed_runner: ManagedRunnerReport,
    stop_plan: Option<OfficialOllamaStopPlan>,
    blockers: Vec<PreflightBlocker>,
) -> OllamaTransparentPreflightReport {
    let accepted = blockers.is_empty();
    let resulting_state = if !accepted {
        OllamaTransparentState::PreflightFailed
    } else if public_port.owner == PortOwnerKind::BeetleMemoryTransparentFront
        && upstream_port.owner == PortOwnerKind::ManagedOllamaRunner
    {
        OllamaTransparentState::Active
    } else {
        OllamaTransparentState::Disabled
    };
    OllamaTransparentPreflightReport {
        accepted,
        resulting_state,
        public_port,
        upstream_port,
        managed_runner,
        stop_plan,
        blockers,
    }
}
