use crate::store_internal::StorePlatform;

use crate::{Result, RuntimeBudgetLease};
use std::collections::HashSet;
use std::sync::Arc;

use bm_core::budget::RuntimeBudgetReport;
#[cfg(feature = "nonproduction-replay-harness")]
use bm_core::budget::StoreRuntimeBudget;
use bm_core::resource::RuntimeResourceProbe;

use crate::store_internal::MemoryStoreEvent;
#[cfg(feature = "nonproduction-replay-harness")]
use crate::store_internal::StorePlatformPreparation;
pub use crate::store_internal::{
    profile_memory_system_kind, StoreBackendConfig, StoreBackendKind, StoreCapacityBudget,
    StoreOpenReport, StorePathBudget, StoreRepairPolicy, StoreRepairReport,
};

/// Opaque host capability for a Beetle Memory store.
///
/// The handle can be opened, cloned, and passed to [`crate::MemoryRuntimeBuilder`]. It does not
/// expose persistence engines, physical snapshots, mutation batches, or writable store traits.
#[derive(Clone)]
pub struct MemoryStoreHandle {
    inner: StorePlatform,
}

/// Privacy-safe aggregate derived from the store event log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryStoreTelemetryReport {
    pub writes_since: u64,
    pub recall_requests: u64,
    pub recall_hits: u64,
    pub projection_requests: u64,
    pub last_projection_chars: usize,
}

#[cfg(feature = "nonproduction-replay-harness")]
pub struct ReplayStoreHarness<'a> {
    platform: &'a StorePlatform,
}

#[cfg(feature = "nonproduction-replay-harness")]
pub struct NonproductionStorePreparation {
    inner: StorePlatformPreparation,
}

#[cfg(feature = "nonproduction-replay-harness")]
impl NonproductionStorePreparation {
    pub fn runtime_budget(&self) -> &RuntimeBudgetReport {
        self.inner.runtime_budget()
    }

    pub fn open_with_benchmark_store_capacity(
        self,
        capacity: StoreRuntimeBudget,
    ) -> Result<MemoryStoreHandle> {
        self.inner
            .open_with_benchmark_store_capacity(capacity)
            .map(|inner| MemoryStoreHandle { inner })
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
impl std::ops::Deref for ReplayStoreHarness<'_> {
    type Target = StorePlatform;

    fn deref(&self) -> &Self::Target {
        self.platform
    }
}

#[cfg(feature = "nonproduction-replay-harness")]
impl ReplayStoreHarness<'_> {
    pub(crate) fn new(platform: &StorePlatform) -> ReplayStoreHarness<'_> {
        ReplayStoreHarness { platform }
    }

    pub fn seed_private_doc_workspace(
        &self,
        scope_id: &str,
        workspace: &bm_core::memory::PrivateDocWorkspace,
    ) -> Result<()> {
        use bm_core::platform::Platform as _;
        self.platform.private_doc_store().set(scope_id, workspace)
    }

    pub fn seed_private_garden_doc(
        &self,
        scope_id: &str,
        path: &str,
        content: &str,
        updated_at: u64,
    ) -> Result<()> {
        use bm_core::platform::Platform as _;
        self.platform
            .private_garden_store()
            .write(scope_id, path, content, updated_at)
            .map(|_| ())
    }
}

impl MemoryStoreHandle {
    pub fn open(config: StoreBackendConfig) -> Result<Self> {
        Ok(Self {
            inner: StorePlatform::open(config)?,
        })
    }

    pub fn open_in_memory(config: StoreBackendConfig) -> Result<Self> {
        Ok(Self {
            inner: StorePlatform::open_in_memory(config)?,
        })
    }

    pub fn open_with_firmware_resource_probe(
        config: StoreBackendConfig,
        probe: Arc<dyn RuntimeResourceProbe>,
    ) -> Result<Self> {
        Ok(Self {
            inner: StorePlatform::open_with_firmware_resource_probe(config, probe)?,
        })
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn open_for_nonproduction_harness(config: StoreBackendConfig) -> Result<Self> {
        Ok(Self {
            inner: StorePlatform::open(config)?,
        })
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn prepare_for_nonproduction_harness(
        config: StoreBackendConfig,
    ) -> Result<NonproductionStorePreparation> {
        StorePlatform::prepare_for_nonproduction_harness(config)
            .map(|inner| NonproductionStorePreparation { inner })
    }

    pub fn config(&self) -> &StoreBackendConfig {
        self.inner.config()
    }

    pub fn open_report(&self) -> &StoreOpenReport {
        self.inner.open_report()
    }

    pub fn runtime_budget(&self) -> RuntimeBudgetReport {
        let authority = self.inner.runtime_budget_authority();
        RuntimeBudgetLease::active_report(&authority).unwrap_or_else(|| {
            self.inner
                .current_runtime_budget(current_runtime_resource_unix_secs())
        })
    }

    pub fn acquire_runtime_budget_lease(&self) -> Result<RuntimeBudgetLease> {
        RuntimeBudgetLease::issue(self.inner.runtime_budget_authority())
    }

    pub fn execute_with_runtime_budget_lease<T>(
        &self,
        lease: &RuntimeBudgetLease,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        lease.execute(&self.inner.runtime_budget_authority(), operation)
    }

    pub fn capacity(&self) -> StoreCapacityBudget {
        self.inner.capacity()
    }

    pub fn telemetry_report(&self, since_unix_secs: u64) -> Result<MemoryStoreTelemetryReport> {
        Ok(telemetry_report_from_events(
            &self.inner.read_events()?,
            since_unix_secs,
        ))
    }

    pub fn telemetry_report_with_file_stores(
        &self,
        roots: &[std::path::PathBuf],
        since_unix_secs: u64,
    ) -> Result<MemoryStoreTelemetryReport> {
        let mut events = Vec::new();
        let mut seen = HashSet::new();
        for event in self.inner.read_events()? {
            if seen.insert(event.event_id.clone()) {
                events.push(event);
            }
        }
        for root in roots {
            for event in StorePlatform::read_file_store_events(root, self.capacity())? {
                if seen.insert(event.event_id.clone()) {
                    events.push(event);
                }
            }
        }
        Ok(telemetry_report_from_events(&events, since_unix_secs))
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub(crate) fn platform(&self) -> &StorePlatform {
        &self.inner
    }

    pub(crate) fn into_platform(self) -> StorePlatform {
        self.inner
    }

    #[cfg(test)]
    pub(crate) fn from_platform(inner: StorePlatform) -> Self {
        Self { inner }
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn export_replay_snapshot(&self) -> Result<crate::store_internal::StoreSnapshot> {
        self.inner.export_store_snapshot()
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn import_replay_snapshot(
        &self,
        snapshot: &crate::store_internal::StoreSnapshot,
    ) -> Result<()> {
        self.inner.import_store_snapshot(snapshot)
    }

    #[cfg(feature = "nonproduction-replay-harness")]
    pub fn replay_harness(&self) -> ReplayStoreHarness<'_> {
        ReplayStoreHarness::new(&self.inner)
    }
}

fn current_runtime_resource_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn telemetry_report_from_events(
    events: &[MemoryStoreEvent],
    since_unix_secs: u64,
) -> MemoryStoreTelemetryReport {
    let mut report = MemoryStoreTelemetryReport::default();
    let mut lifecycle_writes = 0_u64;
    let mut lifecycle_write_timestamps = Vec::new();
    let mut raw_write_timestamps = Vec::new();
    let mut latest_projection_timestamp = 0_u64;

    for event in events {
        if event.kind_name == "memory.write" && event.timestamp_unix_secs >= since_unix_secs {
            raw_write_timestamps.push(event.timestamp_unix_secs);
            continue;
        }
        if event.kind_name != "runtime.lifecycle" {
            continue;
        }
        let Some(operation) = event.payload.get("operation").map(String::as_str) else {
            continue;
        };
        let result_summary = event
            .payload
            .get("result_summary")
            .map(String::as_str)
            .unwrap_or_default();
        if result_summary.starts_with("write.")
            && event.timestamp_unix_secs >= since_unix_secs
            && event_payload_bool(event, "success")
        {
            let changed_count = event_payload_u64(event, "changed_count").unwrap_or(1);
            lifecycle_writes = lifecycle_writes.saturating_add(changed_count);
            if changed_count > 0 {
                lifecycle_write_timestamps.push(event.timestamp_unix_secs);
            }
        }
        if operation == "project" && result_summary == "projection_completed" {
            report.projection_requests = report.projection_requests.saturating_add(1);
            if event.timestamp_unix_secs >= latest_projection_timestamp {
                latest_projection_timestamp = event.timestamp_unix_secs;
                report.last_projection_chars =
                    event_payload_usize(event, "system_memory_chars").unwrap_or_default();
            }
        }
        let has_hit_telemetry =
            event.payload.contains_key("memory_hit") || event.payload.contains_key("hit_count");
        if has_hit_telemetry
            && ((operation == "project" && result_summary == "projection_completed")
                || (operation == "inspect" && result_summary == "recall_completed"))
        {
            report.recall_requests = report.recall_requests.saturating_add(1);
            if event_payload_bool(event, "memory_hit")
                || event_payload_usize(event, "hit_count").unwrap_or_default() > 0
            {
                report.recall_hits = report.recall_hits.saturating_add(1);
            }
        }
    }

    let raw_writes_without_lifecycle = raw_write_timestamps
        .into_iter()
        .filter(|timestamp| {
            !lifecycle_write_timestamps
                .iter()
                .any(|candidate| candidate.abs_diff(*timestamp) <= 5)
        })
        .count() as u64;
    report.writes_since = lifecycle_writes.saturating_add(raw_writes_without_lifecycle);
    report
}

fn event_payload_usize(event: &MemoryStoreEvent, key: &str) -> Option<usize> {
    event.payload.get(key)?.parse().ok()
}

fn event_payload_u64(event: &MemoryStoreEvent, key: &str) -> Option<u64> {
    event.payload.get(key)?.parse().ok()
}

fn event_payload_bool(event: &MemoryStoreEvent, key: &str) -> bool {
    matches!(event.payload.get(key).map(String::as_str), Some("true"))
}
