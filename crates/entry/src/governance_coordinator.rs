use std::sync::{Arc, Condvar, Mutex};

use bm_sdk::MemoryRuntime;

use crate::governance_model::EntryGovernanceModelStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryGovernanceCoordinatorState {
    Idle,
    Processing,
    BlockedConfiguration,
    BlockedCapability,
    BlockedPolicy,
    Retrying,
    Stopping,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryGovernanceCoordinatorReport {
    pub state: EntryGovernanceCoordinatorState,
    pub worker_id: String,
    pub cycles: u64,
    pub completed_jobs: u64,
    pub retried_jobs: u64,
    pub blocked_jobs: u64,
    pub last_job_id: Option<String>,
    pub reason: String,
    pub durable_queue: bm_sdk::DeferredGovernanceQueueReport,
    pub binding_id: Option<String>,
    pub model_id: Option<String>,
    pub config_revision: Option<u64>,
    pub service_ready: bool,
}

struct CoordinatorControl {
    stop: std::sync::atomic::AtomicBool,
    wake_generation: Mutex<u64>,
    wake: Condvar,
    report: Mutex<EntryGovernanceCoordinatorReport>,
    active_claim: Mutex<Option<ActiveClaim>>,
}

#[derive(Clone)]
struct ActiveClaim {
    job_id: String,
    lease_owner: String,
    lease_epoch: u64,
}

pub(crate) struct EntryGovernanceCoordinator {
    control: Arc<CoordinatorControl>,
    runtime: Arc<MemoryRuntime>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    done: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl EntryGovernanceCoordinator {
    pub(crate) fn start(
        runtime: Arc<MemoryRuntime>,
        governance_model: Arc<EntryGovernanceModelStore>,
    ) -> Self {
        let worker_id = format!(
            "entry-governance-{}-{}",
            std::process::id(),
            COORDINATOR_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let initial_state = if cfg!(feature = "governance-model-client-std") {
            EntryGovernanceCoordinatorState::Idle
        } else {
            EntryGovernanceCoordinatorState::BlockedCapability
        };
        let control = Arc::new(CoordinatorControl {
            stop: std::sync::atomic::AtomicBool::new(false),
            wake_generation: Mutex::new(1),
            wake: Condvar::new(),
            report: Mutex::new(EntryGovernanceCoordinatorReport {
                state: initial_state,
                worker_id: worker_id.clone(),
                cycles: 0,
                completed_jobs: 0,
                retried_jobs: 0,
                blocked_jobs: 0,
                last_job_id: None,
                reason: if cfg!(feature = "governance-model-client-std") {
                    "coordinator_started".to_string()
                } else {
                    "governance_model_client_not_compiled".to_string()
                },
                durable_queue: bm_sdk::DeferredGovernanceQueueReport::default(),
                binding_id: None,
                model_id: None,
                config_revision: None,
                service_ready: false,
            }),
            active_claim: Mutex::new(None),
        });

        let thread_control = Arc::clone(&control);
        let thread_runtime = Arc::clone(&runtime);
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        let handle = std::thread::Builder::new()
            .name(worker_id)
            .spawn(move || {
                coordinator_loop(thread_runtime, governance_model, &thread_control);
                let _ = done_tx.send(());
            })
            .expect("entry governance coordinator thread must start");
        Self {
            control,
            runtime,
            handle: Mutex::new(Some(handle)),
            done: Mutex::new(Some(done_rx)),
        }
    }

    pub(crate) fn wake(&self) {
        let mut generation = self
            .control
            .wake_generation
            .lock()
            .expect("entry governance coordinator wake lock");
        *generation = generation.saturating_add(1);
        self.control.wake.notify_one();
    }

    pub(crate) fn report(&self) -> EntryGovernanceCoordinatorReport {
        self.control
            .report
            .lock()
            .expect("entry governance coordinator report lock")
            .clone()
    }

    pub(crate) fn shutdown(&self) {
        use std::sync::atomic::Ordering;

        if self.control.stop.swap(true, Ordering::AcqRel) {
            return;
        }
        update_report(&self.control, |report| {
            report.state = EntryGovernanceCoordinatorState::Stopping;
            report.reason = "coordinator_shutdown_requested".to_string();
        });
        let active = {
            self.control
                .active_claim
                .lock()
                .expect("entry governance coordinator active claim lock")
                .clone()
        };
        if let Some(active) = active {
            if self
                .runtime
                .retry_governance_job(bm_sdk::MemoryGovernanceJobRetryRequest {
                    job_id: active.job_id,
                    lease_owner: active.lease_owner,
                    lease_epoch: active.lease_epoch,
                    error_class: bm_sdk::PostTurnGovernanceErrorClassV2::ServiceUnavailable,
                })
                .is_ok()
            {
                *self
                    .control
                    .active_claim
                    .lock()
                    .expect("entry governance coordinator active claim lock") = None;
            }
        }
        self.control.wake.notify_all();
        let completed = self
            .done
            .lock()
            .expect("entry governance coordinator done lock")
            .take()
            .is_some_and(|done| done.recv_timeout(std::time::Duration::from_secs(2)).is_ok());
        let handle = self
            .handle
            .lock()
            .expect("entry governance coordinator handle lock")
            .take();
        if completed {
            if let Some(handle) = handle {
                let _ = handle.join();
            }
            update_report(&self.control, |report| {
                report.state = EntryGovernanceCoordinatorState::Stopped;
                report.reason = "coordinator_stopped".to_string();
            });
        } else if let Some(handle) = handle {
            handoff_coordinator_to_reaper(handle);
        }
    }
}

impl Drop for EntryGovernanceCoordinator {
    fn drop(&mut self) {
        self.shutdown();
    }
}

static COORDINATOR_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn handoff_coordinator_to_reaper(handle: std::thread::JoinHandle<()>) {
    static REAPER: std::sync::OnceLock<std::sync::mpsc::Sender<std::thread::JoinHandle<()>>> =
        std::sync::OnceLock::new();
    let sender = REAPER.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel::<std::thread::JoinHandle<()>>();
        std::thread::Builder::new()
            .name("entry-governance-reaper".to_string())
            .spawn(move || {
                while let Ok(handle) = receiver.recv() {
                    let _ = handle.join();
                }
            })
            .expect("entry governance reaper thread must start");
        sender
    });
    if let Err(error) = sender.send(handle) {
        let _ = error.0.join();
    }
}

fn update_report(
    control: &CoordinatorControl,
    update: impl FnOnce(&mut EntryGovernanceCoordinatorReport),
) {
    let mut report = control
        .report
        .lock()
        .expect("entry governance coordinator report lock");
    update(&mut report);
}

fn coordinator_loop(
    runtime: Arc<MemoryRuntime>,
    governance_model: Arc<EntryGovernanceModelStore>,
    control: &Arc<CoordinatorControl>,
) {
    use std::sync::atomic::Ordering;

    let mut observed_generation = 0;
    loop {
        if control.stop.load(Ordering::Acquire) {
            break;
        }
        let progressed = run_coordinator_cycle(&runtime, &governance_model, control);
        if progressed {
            continue;
        }
        let generation = control
            .wake_generation
            .lock()
            .expect("entry governance coordinator wake lock");
        if *generation != observed_generation {
            observed_generation = *generation;
            continue;
        }
        let (generation, _) = control
            .wake
            .wait_timeout(generation, std::time::Duration::from_secs(5))
            .expect("entry governance coordinator wait lock");
        observed_generation = *generation;
    }
    update_report(control, |report| {
        report.state = EntryGovernanceCoordinatorState::Stopped;
        report.reason = "coordinator_stopped".to_string();
    });
}

#[cfg(not(feature = "governance-model-client-std"))]
fn run_coordinator_cycle(
    runtime: &MemoryRuntime,
    _governance_model: &EntryGovernanceModelStore,
    control: &Arc<CoordinatorControl>,
) -> bool {
    use std::sync::atomic::Ordering;

    if control.stop.load(Ordering::Acquire) {
        return false;
    }
    update_report(control, |report| {
        report.cycles = report.cycles.saturating_add(1);
        report.state = EntryGovernanceCoordinatorState::BlockedCapability;
        report.service_ready = false;
        report.reason = "governance_model_client_not_compiled".to_string();
    });
    let _ = runtime
        .reconcile_governance_intents(bm_sdk::MemoryGovernanceReconcileRequest { limit: 32 });
    let active = match runtime
        .active_governance_jobs(bm_sdk::MemoryGovernanceActiveJobsRequest { limit: 32 })
    {
        Ok(active) => active,
        Err(error) => {
            update_report(control, |report| {
                report.reason = format!("active_job_query_failed:{}", error.stage());
            });
            return false;
        }
    };
    if let Ok(queue) = runtime.deferred_governance_report() {
        update_report(control, |report| report.durable_queue = queue);
    }

    let mut progressed = false;
    for job in active.jobs {
        if control.stop.load(Ordering::Acquire) {
            return false;
        }
        if job.status == bm_sdk::PostTurnGovernanceJobStatusV2::BlockedCapability
            && job.blocking_reason.as_deref() == Some("governance_model_client_not_compiled")
        {
            continue;
        }
        let blocked = runtime
            .block_governance_job(bm_sdk::MemoryGovernanceJobBlockRequest {
                job_id: job.job_id.clone(),
                kind: bm_sdk::MemoryGovernanceBlockKind::Capability,
                reason: "governance_model_client_not_compiled".to_string(),
            })
            .is_ok();
        if blocked {
            progressed = true;
            update_report(control, |report| {
                report.blocked_jobs = report.blocked_jobs.saturating_add(1);
                report.last_job_id = Some(job.job_id);
            });
        }
    }
    progressed
}

#[cfg(feature = "governance-model-client-std")]
fn run_coordinator_cycle(
    runtime: &MemoryRuntime,
    governance_model: &EntryGovernanceModelStore,
    control: &Arc<CoordinatorControl>,
) -> bool {
    use std::sync::atomic::Ordering;

    if control.stop.load(Ordering::Acquire) {
        return false;
    }
    update_report(control, |report| {
        report.cycles = report.cycles.saturating_add(1);
    });
    let _ = runtime
        .reconcile_governance_intents(bm_sdk::MemoryGovernanceReconcileRequest { limit: 32 });
    let active = match runtime
        .active_governance_jobs(bm_sdk::MemoryGovernanceActiveJobsRequest { limit: 32 })
    {
        Ok(active) => active,
        Err(error) => {
            update_report(control, |report| {
                report.state = EntryGovernanceCoordinatorState::Retrying;
                report.reason = format!("active_job_query_failed:{}", error.stage());
            });
            return false;
        }
    };
    if let Ok(queue) = runtime.deferred_governance_report() {
        update_report(control, |report| report.durable_queue = queue);
    }
    let now_secs = current_unix_secs();
    let mut progressed = false;
    for mut job in active.jobs {
        if control.stop.load(Ordering::Acquire) {
            return false;
        }
        if job.status == bm_sdk::PostTurnGovernanceJobStatusV2::BlockedConfiguration
            && job.blocking_reason.as_deref() == Some("governance_model_authentication_rejected")
        {
            update_report(control, |report| {
                report.state = EntryGovernanceCoordinatorState::BlockedConfiguration;
                report.service_ready = false;
                report.last_job_id = Some(job.job_id.clone());
                report.reason = "governance_model_authentication_rejected".to_string();
            });
            continue;
        }
        if job.status == bm_sdk::PostTurnGovernanceJobStatusV2::Leased
            && job.lease_until.is_some_and(|deadline| deadline > now_secs)
        {
            continue;
        }
        if job.status == bm_sdk::PostTurnGovernanceJobStatusV2::RetryWaiting
            && job
                .next_attempt_at
                .is_some_and(|eligible_at| eligible_at > now_secs)
        {
            continue;
        }
        let binding = match job.attempt_authority.as_ref() {
            Some(authority) => governance_model
                .execution_binding_for_revision(authority.config_revision)
                .and_then(|binding| {
                    if binding.binding_id != authority.binding_id
                        || binding.model != authority.model_id
                    {
                        return Err(bm_sdk::Error::conflict(
                            "entry_governance_coordinator",
                            "pinned model binding identity differs from immutable revision",
                        ));
                    }
                    Ok(binding)
                }),
            None => governance_model
                .execution_binding_for_revision(job.governance_model_policy_revision),
        };
        let binding = match binding.and_then(validate_binding_readiness) {
            Ok(binding) => binding,
            Err(_) => {
                let newly_blocked =
                    job.status != bm_sdk::PostTurnGovernanceJobStatusV2::BlockedConfiguration;
                let block_succeeded = newly_blocked
                    && runtime
                        .block_governance_job(bm_sdk::MemoryGovernanceJobBlockRequest {
                            job_id: job.job_id.clone(),
                            kind: bm_sdk::MemoryGovernanceBlockKind::Configuration,
                            reason: "governance_model_binding_unavailable".to_string(),
                        })
                        .is_ok();
                progressed |= block_succeeded;
                update_report(control, |report| {
                    report.state = EntryGovernanceCoordinatorState::BlockedConfiguration;
                    report.service_ready = false;
                    if block_succeeded {
                        report.blocked_jobs = report.blocked_jobs.saturating_add(1);
                    }
                    report.last_job_id = Some(job.job_id.clone());
                    report.reason = "governance_model_binding_unavailable".to_string();
                });
                continue;
            }
        };
        update_report(control, |report| {
            report.binding_id = Some(binding.binding_id.clone());
            report.model_id = Some(binding.model.clone());
            report.config_revision = Some(binding.config_revision);
            report.service_ready = true;
        });
        let authority = match runtime.prepare_governance_attempt_authority(
            bm_sdk::MemoryGovernanceAttemptAuthorityRequest {
                job_id: job.job_id.clone(),
                binding_id: binding.binding_id.clone(),
                config_revision: binding.config_revision,
                model_id: binding.model.clone(),
            },
        ) {
            Ok(report) => report.authority,
            Err(_) => {
                let newly_blocked =
                    job.status != bm_sdk::PostTurnGovernanceJobStatusV2::BlockedPolicy;
                let block_succeeded = newly_blocked
                    && runtime
                        .block_governance_job(bm_sdk::MemoryGovernanceJobBlockRequest {
                            job_id: job.job_id.clone(),
                            kind: bm_sdk::MemoryGovernanceBlockKind::Policy,
                            reason: "governance_disclosure_authority_unavailable".to_string(),
                        })
                        .is_ok();
                progressed |= block_succeeded;
                update_report(control, |report| {
                    report.state = EntryGovernanceCoordinatorState::BlockedPolicy;
                    if block_succeeded {
                        report.blocked_jobs = report.blocked_jobs.saturating_add(1);
                    }
                    report.last_job_id = Some(job.job_id.clone());
                    report.reason = "governance_disclosure_authority_unavailable".to_string();
                });
                continue;
            }
        };
        if matches!(
            job.status,
            bm_sdk::PostTurnGovernanceJobStatusV2::BlockedConfiguration
                | bm_sdk::PostTurnGovernanceJobStatusV2::BlockedCapability
                | bm_sdk::PostTurnGovernanceJobStatusV2::BlockedPolicy
        ) {
            job = match runtime.resume_governance_job(bm_sdk::MemoryGovernanceJobResumeRequest {
                job_id: job.job_id.clone(),
            }) {
                Ok(report) => report.job,
                Err(_) => continue,
            };
        }
        let lease_seconds = binding
            .request_timeout_ms
            .saturating_add(999)
            .saturating_div(1_000)
            .saturating_mul(4)
            .clamp(30, 3_600);
        let worker_id = control
            .report
            .lock()
            .expect("entry governance coordinator report lock")
            .worker_id
            .clone();
        let claimed = match runtime.claim_governance_job(bm_sdk::MemoryGovernanceJobClaimRequest {
            job_id: job.job_id.clone(),
            lease_owner: worker_id.clone(),
            lease_until: now_secs.saturating_add(lease_seconds),
            authority,
        }) {
            Ok(report) => report.job,
            Err(_) => continue,
        };
        *control
            .active_claim
            .lock()
            .expect("entry governance coordinator active claim lock") = Some(ActiveClaim {
            job_id: claimed.job_id.clone(),
            lease_owner: worker_id.clone(),
            lease_epoch: claimed.lease_epoch,
        });
        update_report(control, |report| {
            report.state = EntryGovernanceCoordinatorState::Processing;
            report.last_job_id = Some(claimed.job_id.clone());
            report.reason = "governance_job_claimed".to_string();
        });
        let mut http = match crate::ReqwestGovernanceLlmHttpClient::new(
            binding.request_timeout_ms,
            4 * 1024 * 1024,
        ) {
            Ok(http) => http,
            Err(error) => {
                let progressed =
                    record_execution_failure(runtime, control, &claimed, &worker_id, error);
                *control
                    .active_claim
                    .lock()
                    .expect("entry governance coordinator active claim lock") = None;
                return progressed;
            }
        };
        let configured = crate::ConfiguredGovernanceLlmClient::new(binding);
        let stoppable = StoppableGovernanceLlmClient {
            inner: configured,
            control: Arc::clone(control),
        };
        match runtime.run_claimed_governance(
            &mut http,
            Some(&stoppable),
            bm_sdk::MemoryGovernanceJobRunRequest {
                job_id: claimed.job_id.clone(),
                lease_owner: worker_id.clone(),
                lease_epoch: claimed.lease_epoch,
            },
        ) {
            Ok(_) => {
                *control
                    .active_claim
                    .lock()
                    .expect("entry governance coordinator active claim lock") = None;
                update_report(control, |report| {
                    report.state = EntryGovernanceCoordinatorState::Idle;
                    report.completed_jobs = report.completed_jobs.saturating_add(1);
                    report.last_job_id = Some(claimed.job_id.clone());
                    report.reason = "governance_job_succeeded".to_string();
                });
                return true;
            }
            Err(error) => {
                if control.stop.load(Ordering::Acquire) {
                    return false;
                }
                let progressed =
                    record_execution_failure(runtime, control, &claimed, &worker_id, error);
                *control
                    .active_claim
                    .lock()
                    .expect("entry governance coordinator active claim lock") = None;
                return progressed;
            }
        }
    }
    if !control.stop.load(Ordering::Acquire) {
        update_report(control, |report| {
            if !matches!(
                report.state,
                EntryGovernanceCoordinatorState::BlockedConfiguration
                    | EntryGovernanceCoordinatorState::BlockedCapability
                    | EntryGovernanceCoordinatorState::BlockedPolicy
            ) {
                report.state = EntryGovernanceCoordinatorState::Idle;
                report.reason = "no_due_governance_job".to_string();
            }
        });
    }
    progressed
}

#[cfg(feature = "governance-model-client-std")]
fn record_execution_failure(
    runtime: &MemoryRuntime,
    control: &CoordinatorControl,
    claimed: &bm_sdk::PostTurnGovernanceJobV2,
    worker_id: &str,
    error: bm_sdk::Error,
) -> bool {
    if matches!(
        error,
        bm_sdk::Error::Http {
            status_code: 401 | 403,
            ..
        }
    ) {
        if runtime
            .block_claimed_governance_job(bm_sdk::MemoryGovernanceClaimedJobBlockRequest {
                job_id: claimed.job_id.clone(),
                lease_owner: worker_id.to_string(),
                lease_epoch: claimed.lease_epoch,
                kind: bm_sdk::MemoryGovernanceBlockKind::Configuration,
                reason: "governance_model_authentication_rejected".to_string(),
            })
            .is_err()
        {
            update_report(control, |report| {
                report.state = EntryGovernanceCoordinatorState::Retrying;
                report.last_job_id = Some(claimed.job_id.clone());
                report.reason = "governance_auth_block_transition_failed".to_string();
            });
            return false;
        }
        update_report(control, |report| {
            report.state = EntryGovernanceCoordinatorState::BlockedConfiguration;
            report.service_ready = false;
            report.blocked_jobs = report.blocked_jobs.saturating_add(1);
            report.last_job_id = Some(claimed.job_id.clone());
            report.reason = "governance_model_authentication_rejected".to_string();
        });
        return true;
    }
    let error_class = classify_execution_error(&error);
    if error_class.is_retryable() {
        if runtime
            .retry_governance_job(bm_sdk::MemoryGovernanceJobRetryRequest {
                job_id: claimed.job_id.clone(),
                lease_owner: worker_id.to_string(),
                lease_epoch: claimed.lease_epoch,
                error_class,
            })
            .is_err()
        {
            update_report(control, |report| {
                report.state = EntryGovernanceCoordinatorState::Retrying;
                report.last_job_id = Some(claimed.job_id.clone());
                report.reason = "governance_retry_transition_failed".to_string();
            });
            return false;
        }
        update_report(control, |report| {
            report.state = EntryGovernanceCoordinatorState::Retrying;
            report.retried_jobs = report.retried_jobs.saturating_add(1);
            report.last_job_id = Some(claimed.job_id.clone());
            report.reason = error_class.as_str().to_string();
        });
    } else {
        if runtime
            .fail_governance_job(bm_sdk::MemoryGovernanceJobFailRequest {
                job_id: claimed.job_id.clone(),
                lease_owner: worker_id.to_string(),
                lease_epoch: claimed.lease_epoch,
                error_class,
                reason: error_class.as_str().to_string(),
            })
            .is_err()
        {
            update_report(control, |report| {
                report.state = EntryGovernanceCoordinatorState::Retrying;
                report.last_job_id = Some(claimed.job_id.clone());
                report.reason = "governance_dead_letter_transition_failed".to_string();
            });
            return false;
        }
        update_report(control, |report| {
            report.state = EntryGovernanceCoordinatorState::Idle;
            report.last_job_id = Some(claimed.job_id.clone());
            report.reason = "governance_job_dead_lettered".to_string();
        });
    }
    true
}

#[cfg(feature = "governance-model-client-std")]
fn classify_execution_error(error: &bm_sdk::Error) -> bm_sdk::PostTurnGovernanceErrorClassV2 {
    match error {
        bm_sdk::Error::Http {
            status_code: 429, ..
        } => bm_sdk::PostTurnGovernanceErrorClassV2::RateLimited,
        bm_sdk::Error::Http {
            status_code: 408 | 504,
            ..
        } => bm_sdk::PostTurnGovernanceErrorClassV2::Timeout,
        bm_sdk::Error::Http { .. } | bm_sdk::Error::Io { .. } | bm_sdk::Error::Other { .. } => {
            bm_sdk::PostTurnGovernanceErrorClassV2::ServiceUnavailable
        }
        bm_sdk::Error::Config { stage, .. }
            if matches!(
                *stage,
                "governance_model_llm"
                    | "private_garden_governance_output"
                    | "long_term_memory_extraction_output"
            ) =>
        {
            bm_sdk::PostTurnGovernanceErrorClassV2::MalformedModelOutput
        }
        bm_sdk::Error::Conflict { .. } | bm_sdk::Error::NotFound { .. } => {
            bm_sdk::PostTurnGovernanceErrorClassV2::IdentityMismatch
        }
        bm_sdk::Error::InvalidInput { .. } | bm_sdk::Error::Config { .. } => {
            bm_sdk::PostTurnGovernanceErrorClassV2::SchemaViolation
        }
        bm_sdk::Error::Nvs { .. } | bm_sdk::Error::Storage { .. } | bm_sdk::Error::Esp { .. } => {
            bm_sdk::PostTurnGovernanceErrorClassV2::ServiceUnavailable
        }
    }
}

#[cfg(feature = "governance-model-client-std")]
fn validate_binding_readiness(
    binding: crate::EntryGovernanceModelExecutionBinding,
) -> bm_sdk::Result<crate::EntryGovernanceModelExecutionBinding> {
    if let crate::EntryGovernanceModelAuthMode::CredentialEnv { credential_env } =
        &binding.auth_mode
    {
        if std::env::var_os(credential_env).is_none() {
            return Err(bm_sdk::Error::config(
                "entry_governance_model_readiness",
                "credential environment variable is unavailable",
            ));
        }
    }
    Ok(binding)
}

#[cfg(feature = "governance-model-client-std")]
struct StoppableGovernanceLlmClient {
    inner: crate::ConfiguredGovernanceLlmClient,
    control: Arc<CoordinatorControl>,
}

#[cfg(feature = "governance-model-client-std")]
impl bm_sdk::LlmClient for StoppableGovernanceLlmClient {
    fn model_compat(&self) -> bm_sdk::LlmModelCompat {
        self.inner.model_compat()
    }

    fn chat(
        &self,
        http: &mut dyn bm_sdk::LlmHttpClient,
        system: &str,
        messages: &[bm_sdk::Message],
        tools: Option<&[bm_sdk::ToolSpec]>,
        tool_choice: bm_sdk::ToolChoicePolicy,
    ) -> bm_sdk::Result<bm_sdk::LlmResponse> {
        use std::sync::atomic::Ordering;

        if self.control.stop.load(Ordering::Acquire) {
            return Err(bm_sdk::Error::conflict(
                "entry_governance_coordinator",
                "coordinator is stopping before model disclosure",
            ));
        }
        let response = self
            .inner
            .chat(http, system, messages, tools, tool_choice)?;
        if self.control.stop.load(Ordering::Acquire) {
            return Err(bm_sdk::Error::conflict(
                "entry_governance_coordinator",
                "coordinator stopped before memory mutation planning",
            ));
        }
        Ok(response)
    }
}

#[cfg(feature = "governance-model-client-std")]
fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .max(1)
}
