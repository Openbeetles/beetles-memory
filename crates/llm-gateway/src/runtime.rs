use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bm_entry::{EntryRuntime, EntryRuntimeBudgetLease, EntryRuntimeManager, EntryRuntimeScope};
use bm_sdk::RuntimeBudgetReport;

use crate::{
    GatewayConfig, GatewayError, GatewayProviderConfig, GatewayUpstreamResponseBudget, Result,
};

pub struct GatewayRuntime {
    config: GatewayConfig,
    manager: EntryRuntimeManager,
    admission: Arc<GatewayRequestAdmission>,
}

#[derive(Clone)]
pub struct GatewayRequestBudgetContext {
    inner: Arc<GatewayRequestBudgetContextInner>,
}

struct GatewayRequestBudgetContextInner {
    lease: EntryRuntimeBudgetLease,
    report: RuntimeBudgetReport,
    response_budget: GatewayUpstreamResponseBudget,
    _permit: GatewayRequestPermit,
}

#[derive(Default)]
struct GatewayRequestAdmission {
    active: AtomicUsize,
}

struct GatewayRequestPermit {
    admission: Arc<GatewayRequestAdmission>,
}

impl GatewayRuntime {
    pub fn open(config: GatewayConfig) -> Result<Self> {
        config.validate()?;
        let manager = EntryRuntimeManager::open_with_requested_max_runtimes(
            config.entry.clone(),
            config.runtime_cache.max_runtimes,
        )
        .map_err(|error| GatewayError::runtime_unavailable(error.to_string()))?;
        Ok(Self {
            config,
            manager,
            admission: Arc::new(GatewayRequestAdmission::default()),
        })
    }

    pub(crate) fn config(&self) -> &GatewayConfig {
        &self.config
    }

    pub fn default_provider_name(&self) -> &str {
        &self.config.default_provider
    }

    pub fn provider_config(&self, name: &str) -> Result<GatewayProviderConfig> {
        self.config.providers.get(name).cloned().ok_or_else(|| {
            GatewayError::invalid_config(format!("gateway provider is not configured: {name}"))
        })
    }

    pub fn runtime_budget(&self) -> RuntimeBudgetReport {
        self.manager.runtime_budget()
    }

    pub fn begin_request(&self) -> Result<GatewayRequestBudgetContext> {
        let lease = self
            .manager
            .acquire_budget_lease()
            .map_err(|error| GatewayError::runtime_unavailable(error.to_string()))?;
        let report = lease.report().clone();
        let response_budget = GatewayUpstreamResponseBudget::from_report(&report);
        let permit = match self
            .admission
            .try_acquire(report.runtime_job_budget.max_concurrent_jobs)
        {
            Ok(permit) => permit,
            Err(error) => {
                self.manager
                    .execute_with_budget_lease(&lease, || Ok(()))
                    .map_err(|lease_error| {
                        GatewayError::runtime_unavailable(lease_error.to_string())
                    })?;
                return Err(error);
            }
        };
        Ok(GatewayRequestBudgetContext {
            inner: Arc::new(GatewayRequestBudgetContextInner {
                lease,
                report,
                response_budget,
                _permit: permit,
            }),
        })
    }

    pub(crate) fn execute_with_request_context<T>(
        &self,
        context: &GatewayRequestBudgetContext,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let mut outcome = None;
        self.manager
            .execute_with_budget_lease(&context.inner.lease, || {
                outcome = Some(operation());
                Ok(())
            })
            .map_err(|error| GatewayError::runtime_unavailable(error.to_string()))?;
        outcome.ok_or_else(|| {
            GatewayError::runtime_unavailable("gateway budget execution produced no outcome")
        })?
    }

    pub fn max_cached_runtimes(&self) -> usize {
        self.manager.max_runtimes()
    }

    pub fn runtime_for_scope(&self, scope: EntryRuntimeScope) -> Result<Arc<EntryRuntime>> {
        self.manager
            .runtime_for_scope(scope)
            .map_err(|error| GatewayError::runtime_unavailable(error.to_string()))
    }

    pub(crate) fn runtime_for_scope_in_request(
        &self,
        context: &GatewayRequestBudgetContext,
        scope: EntryRuntimeScope,
    ) -> Result<Arc<EntryRuntime>> {
        let runtime = self.runtime_for_scope(scope)?;
        context
            .inner
            .response_budget
            .assert_report(&runtime.runtime_budget(), "runtime_scope_cache")?;
        Ok(runtime)
    }
}

impl GatewayRequestBudgetContext {
    pub fn report(&self) -> &RuntimeBudgetReport {
        &self.inner.report
    }

    pub fn report_id(&self) -> &str {
        &self.inner.report.report_id
    }

    pub fn response_budget(&self) -> &GatewayUpstreamResponseBudget {
        &self.inner.response_budget
    }
}

impl std::fmt::Debug for GatewayRequestBudgetContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GatewayRequestBudgetContext")
            .field("report_id", &self.report_id())
            .finish_non_exhaustive()
    }
}

impl GatewayRequestAdmission {
    fn try_acquire(self: &Arc<Self>, limit: usize) -> Result<GatewayRequestPermit> {
        let mut active = self.active.load(Ordering::Acquire);
        loop {
            if active >= limit {
                return Err(GatewayError::capacity_exceeded(format!(
                    "gateway runtime job budget exhausted at {limit} active requests"
                )));
            }
            match self.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(GatewayRequestPermit {
                        admission: Arc::clone(self),
                    })
                }
                Err(observed) => active = observed,
            }
        }
    }
}

impl Drop for GatewayRequestPermit {
    fn drop(&mut self) {
        self.admission.active.fetch_sub(1, Ordering::AcqRel);
    }
}
