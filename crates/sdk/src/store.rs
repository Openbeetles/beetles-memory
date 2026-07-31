use crate::store_internal::{StoreMetricEventSourceRead, StorePlatform};

use crate::{Result, RuntimeBudgetLease};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bm_core::budget::RuntimeBudgetReport;
#[cfg(feature = "nonproduction-replay-harness")]
use bm_core::budget::StoreRuntimeBudget;
use bm_core::metrics::RuntimeMetricEvidenceSummary;
use bm_core::resource::RuntimeResourceProbe;
use bm_core::Error;

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
    pub fn from_nonproduction_store_platform(inner: StorePlatform) -> Self {
        Self { inner }
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

pub(crate) struct RuntimeMetricEventAcquisition {
    pub events: Vec<MemoryStoreEvent>,
    pub evidence: RuntimeMetricEvidenceSummary,
}

pub(crate) fn acquire_runtime_metric_events(
    platform: &StorePlatform,
    external_file_store_roots: &[PathBuf],
    runtime_budget: &RuntimeBudgetReport,
) -> Result<RuntimeMetricEventAcquisition> {
    runtime_budget.validate_for_admission(current_runtime_resource_unix_secs())?;
    let capacity = StoreCapacityBudget::from_runtime_budget(runtime_budget.store_budget);
    validate_external_metric_store_root_count(
        external_file_store_roots.len(),
        capacity.metric_source_max_items,
    )?;
    let mut accumulator = RuntimeMetricEventAccumulator::new(
        capacity.metric_source_max_items,
        capacity.event_log_max_items,
        capacity.snapshot_max_bytes,
    );
    let primary_capacity = accumulator.remaining_source_capacity(capacity)?;
    accumulator.admit_source(platform.read_metric_events(primary_capacity)?)?;

    let primary_root = platform
        .config()
        .data_path()
        .map(canonical_metric_root)
        .transpose()?;
    let mut admitted_roots = BTreeSet::new();
    for root in external_file_store_roots {
        let canonical_root = canonical_metric_root(root)?;
        if primary_root.as_ref() == Some(&canonical_root)
            || !admitted_roots.insert(canonical_root.clone())
        {
            continue;
        }
        let source_capacity = accumulator.remaining_source_capacity(capacity)?;
        accumulator.admit_source(StorePlatform::read_file_metric_events(
            &canonical_root,
            source_capacity,
        )?)?;
    }
    accumulator.finish()
}

fn validate_external_metric_store_root_count(
    external_store_count: usize,
    max_source_stores: usize,
) -> Result<()> {
    if max_source_stores == 0 || external_store_count > max_source_stores.saturating_sub(1) {
        return Err(Error::config(
            "runtime_metrics_source_store_capacity",
            "runtime metric source stores exceed the active store evidence budget",
        ));
    }
    Ok(())
}

fn canonical_metric_root(root: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(root)
        .map_err(|error| Error::io("runtime_metrics_event_store_root", error))
}

struct RuntimeMetricEventAccumulator {
    max_source_stores: usize,
    max_input_events: usize,
    max_snapshot_bytes: usize,
    source_store_count: usize,
    input_event_count: usize,
    accounted_snapshot_bytes: usize,
    duplicate_event_count: usize,
    events_by_id: BTreeMap<String, MemoryStoreEvent>,
}

impl RuntimeMetricEventAccumulator {
    fn new(max_source_stores: usize, max_input_events: usize, max_snapshot_bytes: usize) -> Self {
        Self {
            max_source_stores,
            max_input_events,
            max_snapshot_bytes,
            source_store_count: 0,
            input_event_count: 0,
            accounted_snapshot_bytes: 0,
            duplicate_event_count: 0,
            events_by_id: BTreeMap::new(),
        }
    }

    fn remaining_source_capacity(&self, base: StoreCapacityBudget) -> Result<StoreCapacityBudget> {
        if self.source_store_count >= self.max_source_stores {
            return Err(Error::config(
                "runtime_metrics_source_store_capacity",
                "runtime metric source stores exceed the active store evidence budget",
            ));
        }
        Ok(StoreCapacityBudget {
            metric_source_max_items: self
                .max_source_stores
                .checked_sub(self.source_store_count)
                .ok_or_else(|| {
                    Error::config(
                        "runtime_metrics_source_store_capacity",
                        "runtime metric source budget underflow",
                    )
                })?,
            event_log_max_items: self
                .max_input_events
                .checked_sub(self.input_event_count)
                .ok_or_else(|| {
                    Error::config(
                        "runtime_metrics_event_capacity",
                        "runtime metric event budget underflow",
                    )
                })?,
            snapshot_max_bytes: self
                .max_snapshot_bytes
                .checked_sub(self.accounted_snapshot_bytes)
                .ok_or_else(|| {
                    Error::config(
                        "runtime_metrics_event_bytes",
                        "runtime metric byte budget underflow",
                    )
                })?,
            ..base
        })
    }

    fn admit_source(&mut self, read: StoreMetricEventSourceRead) -> Result<()> {
        let next_source_count = self.source_store_count.checked_add(1).ok_or_else(|| {
            Error::config(
                "runtime_metrics_source_store_capacity",
                "runtime metric source count overflow",
            )
        })?;
        let next_input_event_count = self
            .input_event_count
            .checked_add(read.events.len())
            .ok_or_else(|| {
                Error::config(
                    "runtime_metrics_event_capacity",
                    "runtime metric event count overflow",
                )
            })?;
        let next_snapshot_bytes = self
            .accounted_snapshot_bytes
            .checked_add(read.accounted_snapshot_bytes)
            .ok_or_else(|| {
                Error::config(
                    "runtime_metrics_event_bytes",
                    "runtime metric byte count overflow",
                )
            })?;
        if next_source_count > self.max_source_stores {
            return Err(Error::config(
                "runtime_metrics_source_store_capacity",
                "runtime metric source stores exceed the active store evidence budget",
            ));
        }
        if next_input_event_count > self.max_input_events {
            return Err(Error::config(
                "runtime_metrics_event_capacity",
                "runtime metric event acquisition exceeds the active store event budget",
            ));
        }
        if next_snapshot_bytes > self.max_snapshot_bytes {
            return Err(Error::config(
                "runtime_metrics_event_bytes",
                "runtime metric event acquisition exceeds the active store byte budget",
            ));
        }
        self.source_store_count = next_source_count;
        self.input_event_count = next_input_event_count;
        self.accounted_snapshot_bytes = next_snapshot_bytes;
        for event in read.events {
            event.validate_current_schema("runtime_metrics_event_schema")?;
            match self.events_by_id.get(&event.event_id) {
                Some(existing) if existing == &event => {
                    self.duplicate_event_count =
                        self.duplicate_event_count.checked_add(1).ok_or_else(|| {
                            Error::config(
                                "runtime_metrics_evidence",
                                "runtime metric duplicate count overflow",
                            )
                        })?;
                }
                Some(_) => {
                    return Err(Error::config(
                        "runtime_metric_event_identity_conflict",
                        "one runtime metric event id resolves to different event bytes",
                    ));
                }
                None => {
                    self.events_by_id.insert(event.event_id.clone(), event);
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<RuntimeMetricEventAcquisition> {
        let mut events = self.events_by_id.into_values().collect::<Vec<_>>();
        events.sort_by(|left, right| {
            (left.timestamp_unix_secs, left.event_id.as_str())
                .cmp(&(right.timestamp_unix_secs, right.event_id.as_str()))
        });
        let admitted_unique_event_count = u64::try_from(events.len()).map_err(|_| {
            Error::config(
                "runtime_metrics_evidence",
                "runtime metric event count exceeds u64",
            )
        })?;
        let source_store_count = u64::try_from(self.source_store_count).map_err(|_| {
            Error::config(
                "runtime_metrics_evidence",
                "runtime metric source count exceeds u64",
            )
        })?;
        let input_event_count = u64::try_from(self.input_event_count).map_err(|_| {
            Error::config(
                "runtime_metrics_evidence",
                "runtime metric input event count exceeds u64",
            )
        })?;
        let duplicate_event_count = u64::try_from(self.duplicate_event_count).map_err(|_| {
            Error::config(
                "runtime_metrics_evidence",
                "runtime metric duplicate event count exceeds u64",
            )
        })?;
        Ok(RuntimeMetricEventAcquisition {
            events,
            evidence: RuntimeMetricEvidenceSummary {
                source_store_count,
                input_event_count,
                admitted_unique_event_count,
                duplicate_event_count,
            },
        })
    }
}

#[cfg(test)]
mod runtime_metric_event_acquisition_tests {
    use super::*;
    use crate::store_internal::{
        MemoryStoreEventKind, StoreBackendConfig, StoreBackendKind, StoreEventScope,
        StoreSchemaManifest,
    };

    fn metric_test_profile() -> crate::ProfileId {
        #[cfg(feature = "nonproduction-replay-harness")]
        return crate::ProfileId::native_dev_full().expect("native dev-full profile");
        #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "macos"))]
        return crate::ProfileId::DesktopMacosEmbeddedSdk;
        #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "windows"))]
        return crate::ProfileId::DesktopWindowsEmbeddedSdk;
        #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "linux"))]
        return crate::ProfileId::ServerLinuxMemoryGateway;
        #[cfg(all(
            not(feature = "nonproduction-replay-harness"),
            not(any(target_os = "macos", target_os = "windows", target_os = "linux"))
        ))]
        compile_error!("runtime metric store tests require a supported host target");
    }

    fn metric_store_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "bm-runtime-metrics-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ))
    }

    fn write_metric_event_store(
        root: &Path,
        profile: crate::ProfileId,
        events: &[MemoryStoreEvent],
    ) {
        std::fs::create_dir_all(root.join("events")).expect("create event store");
        let manifest = StoreSchemaManifest::new(StoreBackendKind::File, profile, 1);
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest).expect("manifest"),
        )
        .expect("write manifest");
        let mut event_bytes = Vec::new();
        for event in events {
            serde_json::to_writer(&mut event_bytes, event).expect("event");
            event_bytes.push(b'\n');
        }
        std::fs::write(root.join("events/events.jsonl"), event_bytes).expect("write events");
    }

    fn event(event_id: &str, timestamp: u64) -> MemoryStoreEvent {
        MemoryStoreEvent::new(
            event_id,
            MemoryStoreEventKind::RuntimeLifecycle,
            StoreEventScope::system("metrics"),
            timestamp,
        )
    }

    fn source_read(events: Vec<MemoryStoreEvent>) -> StoreMetricEventSourceRead {
        StoreMetricEventSourceRead {
            events,
            accounted_snapshot_bytes: 0,
        }
    }

    fn runtime_budget(platform: &StorePlatform) -> RuntimeBudgetReport {
        platform.current_runtime_budget(current_runtime_resource_unix_secs())
    }

    #[test]
    fn exact_duplicates_are_deduplicated_and_conflicts_fail_closed() {
        let original = event("same-id", 1);
        let mut accumulator = RuntimeMetricEventAccumulator::new(4, 4, usize::MAX);
        accumulator
            .admit_source(source_read(vec![original.clone()]))
            .expect("first source");
        accumulator
            .admit_source(source_read(vec![original.clone()]))
            .expect("exact duplicate");
        let report = accumulator.finish().expect("acquisition");
        assert_eq!(report.events, vec![original.clone()]);
        assert_eq!(report.evidence.source_store_count, 2);
        assert_eq!(report.evidence.input_event_count, 2);
        assert_eq!(report.evidence.admitted_unique_event_count, 1);
        assert_eq!(report.evidence.duplicate_event_count, 1);

        let mut conflict = RuntimeMetricEventAccumulator::new(4, 4, usize::MAX);
        conflict
            .admit_source(source_read(vec![original]))
            .expect("first source");
        let error = conflict
            .admit_source(source_read(vec![event("same-id", 2)]))
            .expect_err("identity conflict");
        assert_eq!(error.stage(), "runtime_metric_event_identity_conflict");
    }

    #[test]
    fn acquisition_order_is_deterministic_and_capacity_is_exact() {
        let mut forward = RuntimeMetricEventAccumulator::new(2, 2, usize::MAX);
        forward
            .admit_source(source_read(vec![event("z", 1), event("a", 1)]))
            .expect("exact capacity");
        let forward = forward.finish().expect("forward");

        let mut reverse = RuntimeMetricEventAccumulator::new(2, 2, usize::MAX);
        reverse
            .admit_source(source_read(vec![event("a", 1), event("z", 1)]))
            .expect("reverse");
        let reverse = reverse.finish().expect("reverse");
        assert_eq!(forward.events, reverse.events);

        let mut overflow = RuntimeMetricEventAccumulator::new(1, 1, usize::MAX);
        let error = overflow
            .admit_source(source_read(vec![event("a", 1), event("b", 2)]))
            .expect_err("N plus one");
        assert_eq!(error.stage(), "runtime_metrics_event_capacity");
    }

    #[test]
    fn source_store_capacity_is_exact_and_checked_before_path_access() {
        assert!(validate_external_metric_store_root_count(2, 3).is_ok());
        let error = validate_external_metric_store_root_count(3, 3)
            .expect_err("primary plus N external stores exceeds N");
        assert_eq!(error.stage(), "runtime_metrics_source_store_capacity");

        let mut accumulator = RuntimeMetricEventAccumulator::new(1, 4, usize::MAX);
        accumulator
            .admit_source(source_read(Vec::new()))
            .expect("exact source");
        let error = accumulator
            .admit_source(source_read(Vec::new()))
            .expect_err("empty sources cannot bypass source capacity");
        assert_eq!(error.stage(), "runtime_metrics_source_store_capacity");
    }

    #[test]
    fn current_and_external_file_sources_deduplicate_exact_event_bytes() {
        let profile = metric_test_profile();
        let platform =
            StorePlatform::open(StoreBackendConfig::in_memory(profile).expect("in-memory config"))
                .expect("open platform");
        let current = platform.read_events().expect("current events");
        assert_eq!(current.len(), 1);
        let external_root = metric_store_root("duplicate");
        write_metric_event_store(&external_root, profile, &current);

        let acquisition = acquire_runtime_metric_events(
            &platform,
            std::slice::from_ref(&external_root),
            &runtime_budget(&platform),
        )
        .expect("deduplicated acquisition");

        assert_eq!(acquisition.events, current);
        assert_eq!(acquisition.evidence.source_store_count, 2);
        assert_eq!(acquisition.evidence.input_event_count, 2);
        assert_eq!(acquisition.evidence.admitted_unique_event_count, 1);
        assert_eq!(acquisition.evidence.duplicate_event_count, 1);
        std::fs::remove_dir_all(external_root).expect("remove event store");
    }

    #[test]
    fn aggregate_event_budget_rejects_before_parsing_the_first_excess_source_line() {
        let profile = metric_test_profile();
        let platform =
            StorePlatform::open(StoreBackendConfig::in_memory(profile).expect("in-memory config"))
                .expect("open platform");
        let root = metric_store_root("aggregate-event-budget");
        write_metric_event_store(&root, profile, &[]);
        let budget = runtime_budget(&platform);
        let primary_event_count = platform.read_events().expect("primary events").len();
        let external_exact = budget
            .store_budget
            .event_log_max_items
            .checked_sub(primary_event_count)
            .expect("primary fits active event budget");
        let mut event_bytes = Vec::new();
        for index in 0..external_exact {
            serde_json::to_writer(
                &mut event_bytes,
                &event(&format!("external-{index}"), index as u64 + 1),
            )
            .expect("event");
            event_bytes.push(b'\n');
        }
        event_bytes.extend_from_slice(b"{not-json}\n");
        std::fs::write(root.join("events/events.jsonl"), event_bytes)
            .expect("write exact events plus excess corrupt line");

        let error =
            match acquire_runtime_metric_events(&platform, std::slice::from_ref(&root), &budget) {
                Ok(_) => panic!("the aggregate N+1 event must fail before JSON parsing"),
                Err(error) => error,
            };

        assert_eq!(error.stage(), "runtime_metrics_event_capacity");
        std::fs::remove_dir_all(root).expect("remove event store");
    }

    #[test]
    fn aggregate_source_bytes_reject_from_metadata_before_event_parsing() {
        let profile = metric_test_profile();
        let platform =
            StorePlatform::open(StoreBackendConfig::in_memory(profile).expect("in-memory config"))
                .expect("open platform");
        let root = metric_store_root("aggregate-byte-budget");
        write_metric_event_store(&root, profile, &[]);
        std::fs::write(
            root.join("events/events.jsonl"),
            vec![b'x'; runtime_budget(&platform).store_budget.snapshot_max_bytes],
        )
        .expect("write oversized invalid event source");
        let error = match acquire_runtime_metric_events(
            &platform,
            std::slice::from_ref(&root),
            &runtime_budget(&platform),
        ) {
            Ok(_) => panic!("source metadata must exceed the aggregate byte budget before parsing"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "runtime_metrics_event_bytes");
        std::fs::remove_dir_all(root).expect("remove event store");
    }

    #[test]
    fn primary_and_external_source_with_one_event_id_and_different_bytes_fail_closed() {
        let profile = metric_test_profile();
        let platform =
            StorePlatform::open(StoreBackendConfig::in_memory(profile).expect("in-memory config"))
                .expect("open platform");
        let root = metric_store_root("conflict");
        let mut conflicting = platform.read_events().expect("primary events")[0].clone();
        conflicting.timestamp_unix_secs = conflicting.timestamp_unix_secs.saturating_add(1);
        write_metric_event_store(&root, profile, &[conflicting]);

        let error = match acquire_runtime_metric_events(
            &platform,
            std::slice::from_ref(&root),
            &runtime_budget(&platform),
        ) {
            Ok(_) => panic!("event identity conflict must fail closed"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "runtime_metric_event_identity_conflict");
        std::fs::remove_dir_all(root).expect("remove event store");
    }

    #[test]
    fn file_event_source_rejects_corrupt_old_event_and_old_manifest_shapes() {
        let profile = metric_test_profile();
        let capacity = crate::StoreCapacityBudget::full();

        let corrupt_root = metric_store_root("corrupt");
        write_metric_event_store(&corrupt_root, profile, &[]);
        std::fs::write(corrupt_root.join("events/events.jsonl"), b"{not-json}\n")
            .expect("write corrupt event");
        assert!(
            StorePlatform::read_file_metric_events(&corrupt_root, capacity).is_err(),
            "corrupt JSONL must fail closed"
        );

        let old_event_root = metric_store_root("old-event");
        let mut old_event = serde_json::to_value(event("old-event", 1)).expect("event value");
        old_event["schema_version"] = serde_json::json!(0);
        write_metric_event_store(&old_event_root, profile, &[]);
        std::fs::write(
            old_event_root.join("events/events.jsonl"),
            format!("{}\n", old_event),
        )
        .expect("write old event");
        assert!(
            StorePlatform::read_file_metric_events(&old_event_root, capacity).is_err(),
            "old event schema must fail closed"
        );

        let old_manifest_root = metric_store_root("old-manifest");
        write_metric_event_store(&old_manifest_root, profile, &[]);
        let mut old_manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(old_manifest_root.join("manifest.json")).expect("manifest bytes"),
        )
        .expect("manifest value");
        old_manifest["schema_version"] = serde_json::json!(0);
        std::fs::write(
            old_manifest_root.join("manifest.json"),
            serde_json::to_vec(&old_manifest).expect("old manifest"),
        )
        .expect("write old manifest");
        assert!(
            StorePlatform::read_file_metric_events(&old_manifest_root, capacity).is_err(),
            "old manifest schema must fail closed"
        );

        for root in [corrupt_root, old_event_root, old_manifest_root] {
            std::fs::remove_dir_all(root).expect("remove event store");
        }
    }
}
