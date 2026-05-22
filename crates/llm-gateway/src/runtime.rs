use std::sync::Arc;

use bm_entry::{EntryRuntime, EntryRuntimeManager, EntryRuntimeScope};

use crate::{GatewayConfig, GatewayError, Result};

pub struct GatewayRuntime {
    manager: EntryRuntimeManager,
}

impl GatewayRuntime {
    pub fn open(config: GatewayConfig) -> Result<Self> {
        config.validate()?;
        let manager =
            EntryRuntimeManager::with_max_runtimes(config.entry, config.runtime_cache.max_runtimes)
                .map_err(|error| GatewayError::runtime_unavailable(error.to_string()))?;
        Ok(Self { manager })
    }

    pub fn runtime_for_scope(&self, scope: EntryRuntimeScope) -> Result<Arc<EntryRuntime>> {
        self.manager
            .runtime_for_scope(scope)
            .map_err(|error| GatewayError::runtime_unavailable(error.to_string()))
    }
}
