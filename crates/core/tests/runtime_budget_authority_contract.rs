#![cfg(feature = "nonproduction-replay-harness")]

use bm_core::budget::{
    compile_nonproduction_runtime_budget, compile_runtime_budget, BenchmarkStoreCapacityExtension,
    FacetRecallRuntimeBudget, GraphExpansionRuntimeBudget, NonproductionRuntimeBudgetLimits,
    ProviderModelContextLimit, RecallDeliveryRuntimeBudget, RuntimeBudgetInput, RuntimeStoreMedium,
    StaticPlatformManifest, StoreRuntimeBudget, TranscriptGovernanceBudget,
};
use bm_core::feature_gate::{ProfileId, TargetFeature};
use bm_core::orchestrator::PressureLevel;
use bm_core::resource::{
    RuntimeResourceObservation, RuntimeResourceProbe, RuntimeResourceProbeSource,
    RuntimeResourceSnapshot, RuntimeResourceUnavailableReason,
};
use bm_core::{RuntimeBudgetAuthority, RuntimeBudgetReport};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const GIB: u64 = 1024 * 1024 * 1024;

fn native_profile() -> ProfileId {
    ProfileId::native_dev_full().expect("supported host has a native dev-full profile")
}

fn native_manifest() -> StaticPlatformManifest {
    StaticPlatformManifest::for_profile(native_profile(), RuntimeStoreMedium::VolatileMemory)
}

fn compiler_fixture(profile: ProfileId) -> RuntimeBudgetInput {
    let store_medium = if profile.target() == TargetFeature::Esp {
        RuntimeStoreMedium::EmbeddedFlash
    } else {
        RuntimeStoreMedium::VolatileMemory
    };
    RuntimeBudgetInput {
        profile,
        resource_snapshot: RuntimeResourceSnapshot::unavailable(
            10,
            RuntimeResourceProbeSource::StaticManifest,
            RuntimeResourceUnavailableReason::ProbeNotConfigured,
        ),
        static_platform_manifest: StaticPlatformManifest::for_profile(profile, store_medium),
        provider_model_context_limit: None,
    }
}

fn compiler_report(profile: ProfileId) -> RuntimeBudgetReport {
    compile_runtime_budget(compiler_fixture(profile))
}

fn available_observation(
    observed_at_unix_secs: u64,
    ttl_ms: u64,
    pressure: PressureLevel,
    memory_available_bytes: u64,
) -> RuntimeResourceObservation {
    let mut observation = RuntimeResourceObservation::unavailable(
        observed_at_unix_secs,
        RuntimeResourceUnavailableReason::ProbeNotConfigured,
    );
    observation.ttl_ms = ttl_ms;
    observation.pressure = pressure;
    observation.available_parallelism = Some(8);
    observation.memory_total_bytes = Some(memory_available_bytes.saturating_mul(2));
    observation.memory_available_bytes = Some(memory_available_bytes);
    observation.unavailable_reason = None;
    observation
}

#[derive(Clone)]
struct SequenceProbe {
    calls: Arc<AtomicUsize>,
    observations: Arc<Mutex<VecDeque<RuntimeResourceObservation>>>,
}

impl SequenceProbe {
    fn new(observations: impl IntoIterator<Item = RuntimeResourceObservation>) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            observations: Arc::new(Mutex::new(observations.into_iter().collect())),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl RuntimeResourceProbe for SequenceProbe {
    fn probe(&self, _now_secs: u64) -> bm_core::Result<RuntimeResourceObservation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.observations
            .lock()
            .expect("sequence probe")
            .pop_front()
            .ok_or_else(|| bm_core::Error::config("test_sequence_probe", "observation_exhausted"))
    }
}

fn authority(
    probe: impl RuntimeResourceProbe + 'static,
    limits: NonproductionRuntimeBudgetLimits,
    now_secs: u64,
) -> bm_core::Result<RuntimeBudgetAuthority> {
    RuntimeBudgetAuthority::with_nonproduction_host_probe(
        native_profile(),
        native_manifest(),
        None,
        Arc::new(probe),
        limits,
        now_secs,
    )
}

#[test]
fn current_report_refreshes_atomically_at_ttl_boundary() {
    let probe = SequenceProbe::new([
        available_observation(10, 1_000, PressureLevel::Normal, 8 * GIB),
        available_observation(11, 1_000, PressureLevel::Critical, 128 * 1024 * 1024),
    ]);
    let authority = authority(probe.clone(), NonproductionRuntimeBudgetLimits::new(), 10).unwrap();

    let fresh = authority.current_report(10);
    assert!(!fresh.resource_snapshot.stale);
    assert_eq!(probe.calls(), 1);

    let refreshed = authority.current_report(11);
    assert!(!refreshed.resource_snapshot.stale);
    assert_eq!(refreshed.resource_snapshot.observed_at_unix_secs, 11);
    assert_eq!(
        refreshed.resource_snapshot.pressure,
        PressureLevel::Critical
    );
    assert_ne!(refreshed.report_id, fresh.report_id);
    assert_eq!(authority.current_snapshot(11), refreshed.resource_snapshot);
    assert_eq!(probe.calls(), 2);
}

#[test]
fn wall_clock_rollback_refreshes_the_authority_report() {
    let probe = SequenceProbe::new([
        available_observation(100, 60_000, PressureLevel::Normal, 8 * GIB),
        available_observation(99, 60_000, PressureLevel::Cautious, 4 * GIB),
    ]);
    let authority = authority(probe.clone(), NonproductionRuntimeBudgetLimits::new(), 100).unwrap();
    let before = authority.current_report(100);

    let refreshed = authority.current_report(99);

    assert_eq!(probe.calls(), 2);
    assert_ne!(refreshed.report_id, before.report_id);
    assert_eq!(refreshed.resource_snapshot.observed_at_unix_secs, 99);
    assert_eq!(
        refreshed.resource_snapshot.pressure,
        PressureLevel::Cautious
    );
}

#[test]
fn automatic_refresh_failure_publishes_a_fresh_cautious_report_never_stale_admission() {
    let probe = SequenceProbe::new([available_observation(
        10,
        1_000,
        PressureLevel::Normal,
        8 * GIB,
    )]);
    let authority = authority(probe.clone(), NonproductionRuntimeBudgetLimits::new(), 10).unwrap();
    let before = authority.current_report(10);

    let unavailable = authority.current_report(11);

    assert_eq!(probe.calls(), 2);
    assert!(!unavailable.resource_snapshot.stale);
    assert_eq!(unavailable.resource_snapshot.observed_at_unix_secs, 11);
    assert_eq!(
        unavailable.resource_snapshot.pressure,
        PressureLevel::Cautious
    );
    assert_eq!(
        unavailable.resource_snapshot.unavailable_reason,
        Some(RuntimeResourceUnavailableReason::ProbeFailed)
    );
    assert_ne!(unavailable.report_id, before.report_id);
}

#[test]
fn refresh_atomically_recompiles_snapshot_and_report() {
    let probe = SequenceProbe::new([
        available_observation(10, 30_000, PressureLevel::Normal, 8 * GIB),
        available_observation(20, 30_000, PressureLevel::Critical, 128 * 1024 * 1024),
    ]);
    let authority = authority(probe.clone(), NonproductionRuntimeBudgetLimits::new(), 10).unwrap();
    let before = authority.current_report(10);

    let refreshed = authority.refresh(20).unwrap();
    let current = authority.current_report(20);

    assert_eq!(probe.calls(), 2);
    assert_eq!(refreshed, current);
    assert_eq!(current.resource_snapshot.observed_at_unix_secs, 20);
    assert_ne!(current.report_id, before.report_id);
    assert!(
        current.memory_core_budget.profile_max_records
            < before.memory_core_budget.profile_max_records
    );
    assert!(current.store_budget.kv_max_entries < before.store_budget.kv_max_entries);
}

#[test]
fn failed_refresh_keeps_previous_snapshot_and_report_together() {
    let probe = SequenceProbe::new([
        available_observation(10, 30_000, PressureLevel::Normal, 8 * GIB),
        available_observation(21, 30_000, PressureLevel::Normal, 8 * GIB),
    ]);
    let authority = authority(probe, NonproductionRuntimeBudgetLimits::new(), 10).unwrap();
    let before = authority.current_report(10);

    let error = authority.refresh(20).unwrap_err();
    assert_eq!(error.stage(), "runtime_budget_resource_snapshot");
    assert_eq!(authority.current_report(20), before);
}

#[test]
fn store_budget_never_recovers_above_open_admission_ceiling() {
    let manifest = native_manifest();
    let floor = manifest.memory_floor_bytes.max(1);
    let probe = SequenceProbe::new([
        available_observation(10, 30_000, PressureLevel::Normal, floor.saturating_mul(2)),
        available_observation(20, 30_000, PressureLevel::Normal, floor),
        available_observation(30, 30_000, PressureLevel::Normal, floor.saturating_mul(4)),
    ]);
    let authority = authority(probe, NonproductionRuntimeBudgetLimits::new(), 10).unwrap();
    let admission = authority.admission_store_ceiling();

    let degraded = authority.refresh(20).unwrap();
    assert!(degraded.store_budget.kv_max_entries < admission.kv_max_entries);

    let recovered = authority.refresh(30).unwrap();
    assert_eq!(recovered.store_budget, admission);
    assert_eq!(
        recovered
            .evidence_document_budget
            .max_total_bytes_per_transaction,
        recovered.store_budget.snapshot_max_bytes / 2
    );
    assert_eq!(
        recovered
            .retention_quota_report()
            .compaction
            .store_snapshot_max_bytes,
        recovered.store_budget.snapshot_max_bytes
    );
}

#[derive(Clone)]
struct AlternatingProbe {
    calls: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    normal: RuntimeResourceObservation,
    critical: RuntimeResourceObservation,
}

impl AlternatingProbe {
    fn new(normal: RuntimeResourceObservation, critical: RuntimeResourceObservation) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            active: Arc::new(AtomicUsize::new(0)),
            max_active: Arc::new(AtomicUsize::new(0)),
            normal,
            critical,
        }
    }

    fn max_active(&self) -> usize {
        self.max_active.load(Ordering::SeqCst)
    }
}

impl RuntimeResourceProbe for AlternatingProbe {
    fn probe(&self, _now_secs: u64) -> bm_core::Result<RuntimeResourceObservation> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(1));
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.active.fetch_sub(1, Ordering::SeqCst);
        if call.is_multiple_of(2) {
            Ok(self.normal.clone())
        } else {
            Ok(self.critical.clone())
        }
    }
}

#[test]
fn concurrent_readers_observe_atomic_reports_and_refresh_probe_is_serialized() {
    let probe = AlternatingProbe::new(
        available_observation(10, 60_000, PressureLevel::Normal, 8 * GIB),
        available_observation(10, 60_000, PressureLevel::Critical, 128 * 1024 * 1024),
    );
    let authority =
        Arc::new(authority(probe.clone(), NonproductionRuntimeBudgetLimits::new(), 10).unwrap());
    let normal_report = authority.current_report(10);
    let critical_report = authority.refresh(10).unwrap();
    authority.refresh(10).unwrap();
    let expected = Arc::new([
        (
            normal_report.report_id.clone(),
            PressureLevel::Normal,
            normal_report.memory_core_budget,
            normal_report.store_budget,
        ),
        (
            critical_report.report_id.clone(),
            PressureLevel::Critical,
            critical_report.memory_core_budget,
            critical_report.store_budget,
        ),
    ]);

    let mut threads = Vec::new();
    for _ in 0..4 {
        let authority = Arc::clone(&authority);
        let expected = Arc::clone(&expected);
        threads.push(thread::spawn(move || {
            for _ in 0..1_000 {
                let report = authority.current_report(10);
                let matching = expected
                    .iter()
                    .find(|(report_id, _, _, _)| *report_id == report.report_id)
                    .expect("reader observed a partial authority state");
                assert_eq!(report.resource_snapshot.pressure, matching.1);
                assert_eq!(report.memory_core_budget, matching.2);
                assert_eq!(report.store_budget, matching.3);
            }
        }));
    }
    for _ in 0..8 {
        let authority = Arc::clone(&authority);
        threads.push(thread::spawn(move || {
            for _ in 0..20 {
                authority.refresh(10).unwrap();
            }
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(probe.max_active(), 1, "refresh probes must be serialized");
}

fn extend_store_budget(mut budget: StoreRuntimeBudget) -> StoreRuntimeBudget {
    budget.event_log_max_items += 1;
    budget.kv_max_entries += 1;
    budget.blob_max_bytes += 1;
    budget.snapshot_max_bytes += 1;
    budget.logical_namespace_max_bytes += 1;
    budget.logical_key_max_bytes += 1;
    budget.event_record_key_max_bytes += 1;
    budget.export_max_bytes += 1;
    budget.import_max_bytes += 1;
    budget
}

#[test]
fn semantic_limit_constructors_reject_zero_and_cross_field_impossibilities() {
    let compiled = compiler_report(native_profile());
    let mut zero = compiled.graph_expansion_budget;
    zero.max_seed_candidates = 0;
    assert!(NonproductionRuntimeBudgetLimits::new()
        .try_with_graph_expansion_budget(zero)
        .is_err());

    let mut impossible = compiled.graph_expansion_budget;
    impossible.max_hops = 1;
    impossible.default_recall_multi_hop_allowed = true;
    assert!(NonproductionRuntimeBudgetLimits::new()
        .try_with_graph_expansion_budget(impossible)
        .is_err());

    assert!(NonproductionRuntimeBudgetLimits::new()
        .try_with_facet_recall_budget(FacetRecallRuntimeBudget {
            max_query_facets: 1,
            max_facet_index_docs_read: 1,
            max_facet_anchor_candidates: 1,
            max_facet_expanded_candidates: 0,
        })
        .is_err());
    assert!(NonproductionRuntimeBudgetLimits::new()
        .try_with_recall_delivery_budget(RecallDeliveryRuntimeBudget {
            max_selected_candidates: 1,
            max_rendered_capsules: 2,
            max_capsule_chars: 1,
            max_loss_ledger_entries: 1,
        })
        .is_err());
    assert!(NonproductionRuntimeBudgetLimits::new()
        .try_with_transcript_governance_budget(TranscriptGovernanceBudget {
            transcript_page_size: 1,
            host_refs_per_turn: 1,
            max_attrs_per_turn: 1,
            max_attrs_per_message: 2,
            redaction_items_per_page: 1,
            derived_refs_per_report: 1,
            repair_issues_per_report: 1,
        })
        .is_err());
}

#[test]
fn store_limit_recompiles_store_derived_semantics() {
    let input = compiler_fixture(native_profile());
    let compiled = compile_runtime_budget(input.clone());
    let mut store_limit = compiled.store_budget;
    store_limit.event_log_max_items /= 2;
    store_limit.kv_max_entries /= 2;
    store_limit.blob_max_bytes /= 2;
    store_limit.snapshot_max_bytes /= 2;
    store_limit.logical_namespace_max_bytes /= 2;
    store_limit.logical_key_max_bytes /= 2;
    store_limit.event_record_key_max_bytes /= 2;
    store_limit.export_max_bytes /= 2;
    store_limit.import_max_bytes /= 2;
    let limits = NonproductionRuntimeBudgetLimits::new()
        .try_with_store_budget_limit(store_limit)
        .unwrap();

    let report = compile_nonproduction_runtime_budget(input, limits).unwrap();
    assert_eq!(report.store_budget, store_limit);
    assert_eq!(
        report
            .evidence_document_budget
            .max_total_bytes_per_transaction,
        store_limit.snapshot_max_bytes / 2
    );
    assert_eq!(
        report
            .retention_quota_report()
            .compaction
            .store_snapshot_max_bytes,
        store_limit.snapshot_max_bytes
    );
}

#[test]
fn benchmark_capacity_is_report_external_physical_envelope() {
    let report = compiler_report(native_profile());
    let original = report.clone();
    let capacity = extend_store_budget(report.store_budget);

    let extension = BenchmarkStoreCapacityExtension::try_new(&report, capacity).unwrap();

    assert_eq!(extension.report_id(), report.report_id);
    assert_eq!(extension.capacity(), capacity);
    assert_eq!(report, original);
    assert_eq!(report.store_budget, original.store_budget);
    assert_eq!(
        report.evidence_document_budget,
        original.evidence_document_budget
    );
    assert_eq!(
        report.retention_quota_report(),
        original.retention_quota_report()
    );
}

#[test]
fn nonproduction_ceiling_never_expands_canonical_report() {
    let input = compiler_fixture(native_profile());
    let compiled = compile_runtime_budget(input.clone());
    let mut expanded = compiled.store_budget;
    expanded.kv_max_entries += 1;
    let limits = NonproductionRuntimeBudgetLimits::new()
        .try_with_store_budget_limit(expanded)
        .unwrap();

    let report = compile_nonproduction_runtime_budget(input, limits).unwrap();
    assert_eq!(report.store_budget, compiled.store_budget);
}

#[test]
fn nonproduction_authority_reapplies_limits_after_refresh() {
    let probe = SequenceProbe::new([
        available_observation(10, 30_000, PressureLevel::Normal, 8 * GIB),
        available_observation(20, 30_000, PressureLevel::Normal, 8 * GIB),
    ]);
    let production = compiler_report(native_profile());
    let mut graph_limit: GraphExpansionRuntimeBudget = production.graph_expansion_budget;
    graph_limit.max_hops = 1;
    graph_limit.default_recall_multi_hop_allowed = false;
    graph_limit.eval_recall_multi_hop_allowed = false;
    let limits = NonproductionRuntimeBudgetLimits::new()
        .try_with_graph_expansion_budget(graph_limit)
        .unwrap();
    let authority = authority(probe, limits, 10).unwrap();

    assert_eq!(
        authority.current_report(10).graph_expansion_budget,
        graph_limit
    );
    assert_eq!(
        authority.refresh(20).unwrap().graph_expansion_budget,
        graph_limit
    );
}

#[test]
fn default_nonproduction_constructor_reuses_core_registration() {
    let profile = ProfileId::EspEmbeddedSdk;
    let production = compiler_report(profile);
    let mut graph_limit = production.graph_expansion_budget;
    graph_limit.max_seed_candidates = 1;
    let limits = NonproductionRuntimeBudgetLimits::new()
        .try_with_graph_expansion_budget(graph_limit)
        .unwrap();

    let authority = RuntimeBudgetAuthority::with_default_probe_nonproduction(
        profile,
        StaticPlatformManifest::for_profile(profile, RuntimeStoreMedium::EmbeddedFlash),
        Some(ProviderModelContextLimit::default()),
        limits,
        10,
    )
    .unwrap();
    let report = authority.current_report(10);

    assert_eq!(
        report.resource_snapshot.source,
        RuntimeResourceProbeSource::FirmwareManifest
    );
    assert_eq!(report.graph_expansion_budget.max_seed_candidates, 1);
}
