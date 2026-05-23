use std::sync::Arc;

use bm_entry::{EntryRuntime, EntryRuntimeManager, EntryRuntimeScope};
use bm_sdk::RuntimeBudgetReport;

use crate::{GatewayConfig, GatewayError, Result};

pub struct GatewayRuntime {
    manager: EntryRuntimeManager,
}

impl GatewayRuntime {
    pub fn open(config: GatewayConfig) -> Result<Self> {
        config.validate()?;
        let budget_max = RuntimeBudgetReport::static_for_profile(config.entry.profile)
            .llm_gateway_budget
            .runtime_cache_max_runtimes;
        let max_runtimes = config.runtime_cache.max_runtimes.min(budget_max).max(1);
        let manager = EntryRuntimeManager::with_max_runtimes(config.entry, max_runtimes)
            .map_err(|error| GatewayError::runtime_unavailable(error.to_string()))?;
        Ok(Self { manager })
    }

    pub fn runtime_for_scope(&self, scope: EntryRuntimeScope) -> Result<Arc<EntryRuntime>> {
        self.manager
            .runtime_for_scope(scope)
            .map_err(|error| GatewayError::runtime_unavailable(error.to_string()))
    }
}
