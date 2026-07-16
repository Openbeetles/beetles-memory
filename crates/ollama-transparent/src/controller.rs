use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::process::{ProcessManager, SystemProcessManager};
use crate::process_authority::AttachedProcessAuthority;
use crate::runner::{FileSystemRunnerInstaller, RunnerInstaller};
use crate::{
    lease::OsTransitionLease, runner::inspect_executable_identity, GatewayFrontReport,
    ManagedProcessOwnershipReport, ManagedRunnerReport, OfficialOllamaStopPlan,
    OfficialOllamaStopTarget, OllamaAppReport, OllamaTransparentConfig, OllamaTransparentError,
    OllamaTransparentPreflightReport, OllamaTransparentState, OllamaTransparentStatus,
    OllamaTransparentTransitionReport, PortBindingReport, PortOwnerKind, PortOwnerObserver,
    PreflightBlocker, PreflightBlockerCode, Result, RollbackReport, SystemPortOwnerObserver,
    TransitionOutcome, TransitionStep, TransitionStepReport,
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

struct ControllerCore<P, R, M> {
    config: OllamaTransparentConfig,
    ports: P,
    runner: R,
    processes: M,
    state: Mutex<ControllerState>,
}

pub struct TransparentController {
    inner: ControllerCore<SystemPortOwnerObserver, FileSystemRunnerInstaller, SystemProcessManager>,
}

impl TransparentController {
    pub fn new(config: OllamaTransparentConfig) -> Result<Self> {
        let ports = SystemPortOwnerObserver::new(config.port_owner_classifier());
        Ok(Self {
            inner: ControllerCore::new(
                config,
                ports,
                FileSystemRunnerInstaller,
                SystemProcessManager::default(),
            )?,
        })
    }

    pub fn config(&self) -> &OllamaTransparentConfig {
        self.inner.config()
    }
}

impl OllamaTransparentController for TransparentController {
    fn preflight(&self) -> Result<OllamaTransparentPreflightReport> {
        self.inner.preflight()
    }

    fn enable(
        &self,
        request: EnableOllamaTransparentRequest,
    ) -> Result<OllamaTransparentTransitionReport> {
        self.inner.enable(request)
    }

    fn disable(
        &self,
        request: DisableOllamaTransparentRequest,
    ) -> Result<OllamaTransparentTransitionReport> {
        self.inner.disable(request)
    }

    fn status(&self) -> Result<OllamaTransparentStatus> {
        self.inner.status()
    }

    fn open_app(&self) -> Result<crate::ProcessActionReport> {
        self.inner.open_app()
    }
}

#[derive(Clone, Debug)]
struct ControllerState {
    state: OllamaTransparentState,
    last_transition: Option<OllamaTransparentTransitionReport>,
    next_lease_id: u64,
    active_transition: Option<TransitionLeaseRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransitionKind {
    Enable,
    Disable,
    OpenApp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransitionLeaseRecord {
    id: u64,
    kind: TransitionKind,
}

struct TransitionLease<'a> {
    record: TransitionLeaseRecord,
    from_state: OllamaTransparentState,
    controller_state: &'a Mutex<ControllerState>,
    committed: bool,
    _os_lease: OsTransitionLease,
}

impl TransitionLease<'_> {
    fn set_state(&self, next: OllamaTransparentState) -> Result<()> {
        let mut state = self.controller_state.lock().expect("controller state");
        if state.active_transition != Some(self.record) {
            return Err(OllamaTransparentError::preflight_rejected(format!(
                "transition lease {} lost ownership",
                self.record.id
            )));
        }
        state.state = next;
        Ok(())
    }

    fn commit_report(
        mut self,
        report: OllamaTransparentTransitionReport,
    ) -> Result<OllamaTransparentTransitionReport> {
        {
            let mut state = self.controller_state.lock().expect("controller state");
            if state.active_transition != Some(self.record) {
                return Err(OllamaTransparentError::preflight_rejected(format!(
                    "transition lease {} lost ownership before completion",
                    self.record.id
                )));
            }
            state.state = report.to_state;
            state.last_transition = Some(report.clone());
            state.active_transition = None;
        }
        self.committed = true;
        Ok(report)
    }

    fn commit_exclusive(mut self) -> Result<()> {
        {
            let mut state = self.controller_state.lock().expect("controller state");
            if state.active_transition != Some(self.record) {
                return Err(OllamaTransparentError::preflight_rejected(format!(
                    "transition lease {} lost ownership before operation completion",
                    self.record.id
                )));
            }
            state.active_transition = None;
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for TransitionLease<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut state = self
            .controller_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active_transition == Some(self.record) {
            state.state = self.from_state;
            state.active_transition = None;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct EnableProgress {
    official_stopped: bool,
    upstream_started: bool,
    public_front_started: bool,
}

impl<P, R, M> ControllerCore<P, R, M>
where
    P: PortOwnerObserver,
    R: RunnerInstaller,
    M: ProcessManager,
{
    fn new(config: OllamaTransparentConfig, ports: P, runner: R, processes: M) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            ports,
            runner,
            processes,
            state: Mutex::new(ControllerState {
                state: OllamaTransparentState::Disabled,
                last_transition: None,
                next_lease_id: 1,
                active_transition: None,
            }),
        })
    }

    fn config(&self) -> &OllamaTransparentConfig {
        &self.config
    }

    #[cfg(test)]
    fn runner(&self) -> &R {
        &self.runner
    }

    #[cfg(test)]
    fn processes(&self) -> &M {
        &self.processes
    }

    fn begin_transition(&self, kind: TransitionKind) -> Result<TransitionLease<'_>> {
        let mut state = self.state.lock().expect("controller state");
        if let Some(active) = state.active_transition {
            return Err(OllamaTransparentError::preflight_rejected(format!(
                "transparent transition {:?} lease {} is already active",
                active.kind, active.id
            )));
        }
        let lease_id = state.next_lease_id;
        let next_lease_id = lease_id.checked_add(1).ok_or_else(|| {
            OllamaTransparentError::preflight_rejected("transition lease id exhausted")
        })?;
        let os_lease = OsTransitionLease::acquire(&self.config.transition_lease_path)?;
        let lease = TransitionLease {
            record: TransitionLeaseRecord { id: lease_id, kind },
            from_state: state.state,
            controller_state: &self.state,
            committed: false,
            _os_lease: os_lease,
        };
        state.next_lease_id = next_lease_id;
        state.active_transition = Some(lease.record);
        state.state = match kind {
            TransitionKind::Enable => OllamaTransparentState::Enabling,
            TransitionKind::Disable => OllamaTransparentState::Disabling,
            TransitionKind::OpenApp => state.state,
        };
        Ok(lease)
    }
}

#[cfg(test)]
#[path = "controller/preflight_tests.rs"]
mod preflight_tests;

#[cfg(test)]
#[path = "controller/state_machine_tests.rs"]
mod state_machine_tests;

impl<P, R, M> OllamaTransparentController for ControllerCore<P, R, M>
where
    P: PortOwnerObserver,
    R: RunnerInstaller,
    M: ProcessManager,
{
    fn preflight(&self) -> Result<OllamaTransparentPreflightReport> {
        let report = build_preflight_report(&self.config, &self.ports, &self.runner)?;
        if !report.accepted {
            let mut state = self.state.lock().expect("controller state");
            if state.active_transition.is_none() {
                state.state = OllamaTransparentState::PreflightFailed;
            }
        }
        Ok(report)
    }

    fn enable(
        &self,
        request: EnableOllamaTransparentRequest,
    ) -> Result<OllamaTransparentTransitionReport> {
        let lease = self.begin_transition(TransitionKind::Enable)?;
        let from_state = lease.from_state;
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
                return lease.commit_report(report);
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
            return lease.commit_report(report);
        }
        steps.push(TransitionStepReport::ok(TransitionStep::Preflight));
        if preflight.resulting_state == OllamaTransparentState::Active {
            let ownership = self
                .processes
                .inspect_managed_process_ownership(&effective_config)?;
            if !ownership.fully_authorized() {
                let failed = TransitionStepReport::failed(
                    TransitionStep::Preflight,
                    "managed-looking listeners lack exact persisted launch receipts",
                );
                let report = OllamaTransparentTransitionReport {
                    from_state,
                    to_state: OllamaTransparentState::Degraded,
                    outcome: TransitionOutcome::Rejected,
                    steps: vec![failed.clone()],
                    failing_step: Some(failed),
                    rollback: None,
                };
                return lease.commit_report(report);
            }
            let report = OllamaTransparentTransitionReport::completed(
                from_state,
                OllamaTransparentState::Active,
                steps,
            );
            return lease.commit_report(report);
        }

        if let Some(stop_plan) = preflight.stop_plan.as_ref() {
            match self.processes.stop_official_ollama(stop_plan) {
                Ok(_) => {
                    progress.official_stopped = true;
                    steps.push(TransitionStepReport::ok(TransitionStep::StopOfficialOllama));
                }
                Err(error) => {
                    return self.finish_failed_enable(
                        lease,
                        from_state,
                        steps,
                        TransitionStep::StopOfficialOllama,
                        error,
                        progress,
                    );
                }
            }
        }

        let installed_runner = match self.runner.ensure_installed(&effective_config) {
            Ok(report) => {
                steps.push(TransitionStepReport::ok(
                    TransitionStep::InstallManagedRunner,
                ));
                report
            }
            Err(error) => {
                return self.finish_failed_enable(
                    lease,
                    from_state,
                    steps,
                    TransitionStep::InstallManagedRunner,
                    error,
                    progress,
                );
            }
        };

        match self
            .processes
            .start_managed_upstream(&effective_config, &installed_runner)
        {
            Ok(_) => {
                progress.upstream_started = true;
                steps.push(TransitionStepReport::ok(
                    TransitionStep::StartManagedUpstream,
                ));
            }
            Err(error) => {
                return self.finish_failed_enable(
                    lease,
                    from_state,
                    steps,
                    TransitionStep::StartManagedUpstream,
                    error,
                    progress,
                );
            }
        }

        match self.processes.probe_managed_upstream(&effective_config) {
            Ok(_) => steps.push(TransitionStepReport::ok(
                TransitionStep::ProbeManagedUpstream,
            )),
            Err(error) => {
                return self.finish_failed_enable(
                    lease,
                    from_state,
                    steps,
                    TransitionStep::ProbeManagedUpstream,
                    error,
                    progress,
                );
            }
        }

        let gateway_executable = preflight.gateway_executable.as_ref().ok_or_else(|| {
            OllamaTransparentError::preflight_rejected(
                "accepted preflight omitted gateway executable identity",
            )
        })?;
        match self
            .processes
            .start_transparent_front(&effective_config, gateway_executable)
        {
            Ok(_) => {
                progress.public_front_started = true;
                steps.push(TransitionStepReport::ok(
                    TransitionStep::StartTransparentFront,
                ));
            }
            Err(error) => {
                return self.finish_failed_enable(
                    lease,
                    from_state,
                    steps,
                    TransitionStep::StartTransparentFront,
                    error,
                    progress,
                );
            }
        }

        match self.processes.probe_public_front(&effective_config) {
            Ok(_) => steps.push(TransitionStepReport::ok(TransitionStep::ProbePublicFront)),
            Err(error) => {
                return self.finish_failed_enable(
                    lease,
                    from_state,
                    steps,
                    TransitionStep::ProbePublicFront,
                    error,
                    progress,
                );
            }
        }

        if request
            .open_app
            .unwrap_or(effective_config.open_app_after_enable)
        {
            match self.processes.open_official_app(&effective_config) {
                Ok(_) => steps.push(TransitionStepReport::ok(TransitionStep::OpenOfficialApp)),
                Err(error) => {
                    return self.finish_failed_enable(
                        lease,
                        from_state,
                        steps,
                        TransitionStep::OpenOfficialApp,
                        error,
                        progress,
                    );
                }
            }
        }

        let report = OllamaTransparentTransitionReport::completed(
            from_state,
            OllamaTransparentState::Active,
            steps,
        );
        lease.commit_report(report)
    }

    fn disable(
        &self,
        request: DisableOllamaTransparentRequest,
    ) -> Result<OllamaTransparentTransitionReport> {
        let lease = self.begin_transition(TransitionKind::Disable)?;
        let from_state = lease.from_state;
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
        lease.commit_report(report)
    }

    fn status(&self) -> Result<OllamaTransparentStatus> {
        let public_port = self.ports.inspect(self.config.public_bind)?;
        let upstream_port = self.ports.inspect(self.config.upstream_bind)?;
        let managed_runner = self.runner.inspect(&self.config)?;
        let ownership = self
            .processes
            .inspect_managed_process_ownership(&self.config)?;
        let state = {
            let mut state = self.state.lock().expect("controller state");
            if state.active_transition.is_none() {
                state.state =
                    reconcile_observed_state(state.state, &public_port, &upstream_port, ownership);
            }
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
        let lease = self.begin_transition(TransitionKind::OpenApp)?;
        let result = self.processes.open_official_app(&self.config)?;
        lease.commit_exclusive()?;
        Ok(result)
    }
}

fn reconcile_observed_state(
    current: OllamaTransparentState,
    public_port: &PortBindingReport,
    upstream_port: &PortBindingReport,
    ownership: ManagedProcessOwnershipReport,
) -> OllamaTransparentState {
    let public_owned = public_port.owner == PortOwnerKind::BeetleMemoryTransparentFront;
    let upstream_owned = upstream_port.owner == PortOwnerKind::ManagedOllamaRunner;
    match (public_owned, upstream_owned) {
        (true, true) if ownership.fully_authorized() => OllamaTransparentState::Active,
        (true, true) => OllamaTransparentState::Degraded,
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

impl<P, R, M> ControllerCore<P, R, M>
where
    P: PortOwnerObserver,
    R: RunnerInstaller,
    M: ProcessManager,
{
    fn finish_failed_enable(
        &self,
        lease: TransitionLease<'_>,
        from_state: OllamaTransparentState,
        mut steps: Vec<TransitionStepReport>,
        failing_step: TransitionStep,
        error: OllamaTransparentError,
        progress: EnableProgress,
    ) -> Result<OllamaTransparentTransitionReport> {
        let failed = TransitionStepReport::failed(failing_step, error.to_string());
        steps.push(failed.clone());
        lease.set_state(OllamaTransparentState::RollingBack)?;
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
        lease.commit_report(report)
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
    let gateway_executable = match inspect_executable_identity(&config.gateway_binary_path) {
        Ok(identity) => Some(identity),
        Err(error) => {
            blockers.push(PreflightBlocker::new(
                PreflightBlockerCode::GatewayFrontUnavailable,
                error.to_string(),
            ));
            None
        }
    };

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
                    gateway_executable,
                    stop_plan,
                    blockers,
                ));
            };
            if config.allow_stop_official_ollama {
                if !process.has_complete_identity() {
                    blockers.push(PreflightBlocker::new(
                        PreflightBlockerCode::PublicPortOwnedByUnknownProcess,
                        "official Ollama owner lacks a complete pid/start/command/executable identity",
                    ));
                    return Ok(report_from_parts(
                        public_port,
                        upstream_port,
                        managed_runner,
                        gateway_executable,
                        stop_plan,
                        blockers,
                    ));
                }
                let official_identity =
                    match inspect_executable_identity(&config.official_ollama_binary) {
                        Ok(identity) => identity,
                        Err(error) => {
                            blockers.push(PreflightBlocker::new(
                                PreflightBlockerCode::PublicPortOwnedByUnknownProcess,
                                format!(
                                    "official Ollama executable identity is unavailable: {error}"
                                ),
                            ));
                            return Ok(report_from_parts(
                                public_port,
                                upstream_port,
                                managed_runner,
                                gateway_executable,
                                stop_plan,
                                blockers,
                            ));
                        }
                    };
                if process.executable_identity.as_ref() != Some(&official_identity) {
                    blockers.push(PreflightBlocker::new(
                        PreflightBlockerCode::PublicPortOwnedByUnknownProcess,
                        "classifier matched Ollama, but exact executable identity does not match the governed official binary",
                    ));
                    return Ok(report_from_parts(
                        public_port,
                        upstream_port,
                        managed_runner,
                        gateway_executable,
                        stop_plan,
                        blockers,
                    ));
                }
                if !AttachedProcessAuthority::is_supported() {
                    blockers.push(PreflightBlocker::new(
                        PreflightBlockerCode::PublicPortOwnedByUnknownProcess,
                        "platform cannot retain a stable authority for externally launched Ollama; close the official app before enabling transparent mode",
                    ));
                    return Ok(report_from_parts(
                        public_port,
                        upstream_port,
                        managed_runner,
                        gateway_executable,
                        stop_plan,
                        blockers,
                    ));
                }
                stop_plan = Some(OfficialOllamaStopPlan {
                    allowed: true,
                    targets: vec![OfficialOllamaStopTarget {
                        bind: public_port.bind,
                        process,
                    }],
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
    Ok(report_from_parts(
        public_port,
        upstream_port,
        managed_runner,
        gateway_executable,
        stop_plan,
        blockers,
    ))
}

fn report_from_parts(
    public_port: PortBindingReport,
    upstream_port: PortBindingReport,
    managed_runner: ManagedRunnerReport,
    gateway_executable: Option<crate::ExecutableFileIdentity>,
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
        gateway_executable,
        stop_plan,
        blockers,
    }
}
