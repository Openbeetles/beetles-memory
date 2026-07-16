use bm_core::budget::{
    compile_runtime_budget, RuntimeBudgetInput, RuntimeBudgetReport, RuntimeStoreMedium,
    StaticPlatformManifest,
};
use bm_core::feature_gate::ProfileId;
use bm_core::orchestrator::PressureLevel;
use bm_core::resource::{
    RuntimeResourceProbeSource, RuntimeResourceSnapshot, RuntimeResourceUnavailableReason,
};

const GIB: u64 = 1024 * 1024 * 1024;

fn host_snapshot(
    memory_available_bytes: u64,
    storage_available_bytes: u64,
) -> RuntimeResourceSnapshot {
    let mut snapshot = RuntimeResourceSnapshot::unavailable(
        10,
        RuntimeResourceProbeSource::StaticManifest,
        RuntimeResourceUnavailableReason::ProbeNotConfigured,
    );
    snapshot.pressure = PressureLevel::Normal;
    snapshot.memory_total_bytes = Some(memory_available_bytes.saturating_mul(2));
    snapshot.memory_available_bytes = Some(memory_available_bytes);
    snapshot.storage_total_bytes = Some(storage_available_bytes.saturating_mul(2));
    snapshot.storage_available_bytes = Some(storage_available_bytes);
    snapshot.unavailable_reason = None;
    snapshot
}

fn compile_host_store(
    medium: RuntimeStoreMedium,
    memory_available_bytes: u64,
    storage_available_bytes: u64,
) -> RuntimeBudgetReport {
    let profile = ProfileId::DesktopMacosStandaloneMemory;
    compile_runtime_budget(RuntimeBudgetInput {
        profile,
        resource_snapshot: host_snapshot(memory_available_bytes, storage_available_bytes),
        static_platform_manifest: StaticPlatformManifest::for_profile(profile, medium),
        provider_model_context_limit: None,
    })
}

#[test]
fn volatile_memory_store_budget_ignores_filesystem_capacity() {
    let constrained_disk = compile_host_store(RuntimeStoreMedium::VolatileMemory, 8 * GIB, 1);
    let abundant_disk = compile_host_store(RuntimeStoreMedium::VolatileMemory, 8 * GIB, 128 * GIB);

    assert_eq!(constrained_disk.store_budget, abundant_disk.store_budget);
}

#[test]
fn persistent_filesystem_store_budget_tracks_observed_storage() {
    let constrained_disk = compile_host_store(RuntimeStoreMedium::PersistentFilesystem, 8 * GIB, 1);
    let abundant_disk =
        compile_host_store(RuntimeStoreMedium::PersistentFilesystem, 8 * GIB, 128 * GIB);

    assert_ne!(constrained_disk.store_budget, abundant_disk.store_budget);
}

#[test]
fn embedded_flash_store_budget_tracks_memory_working_set_and_storage() {
    let profile = ProfileId::EspStandaloneMemory;
    let compile = |memory_available_bytes: u64, storage_available_bytes: u64| {
        let mut snapshot = RuntimeResourceSnapshot::unavailable(
            10,
            RuntimeResourceProbeSource::FirmwareManifest,
            RuntimeResourceUnavailableReason::ProbeNotConfigured,
        );
        snapshot.pressure = PressureLevel::Normal;
        snapshot.internal_heap_free_bytes = Some(memory_available_bytes);
        snapshot.internal_heap_minimum_free_bytes = Some(memory_available_bytes);
        snapshot.internal_heap_largest_block_bytes = Some(memory_available_bytes);
        snapshot.storage_total_bytes = Some(storage_available_bytes.saturating_mul(2));
        snapshot.storage_available_bytes = Some(storage_available_bytes);
        snapshot.unavailable_reason = None;
        compile_runtime_budget(RuntimeBudgetInput {
            profile,
            resource_snapshot: snapshot,
            static_platform_manifest: StaticPlatformManifest::for_profile(
                profile,
                RuntimeStoreMedium::EmbeddedFlash,
            ),
            provider_model_context_limit: None,
        })
    };

    let constrained_memory = compile(1, 128 * GIB);
    let constrained_storage = compile(8 * GIB, 1);
    let abundant = compile(8 * GIB, 128 * GIB);

    assert_ne!(constrained_memory.store_budget, abundant.store_budget);
    assert_ne!(constrained_storage.store_budget, abundant.store_budget);
}

#[test]
fn report_identity_includes_store_medium() {
    let volatile = compile_host_store(RuntimeStoreMedium::VolatileMemory, 8 * GIB, 128 * GIB);
    let persistent =
        compile_host_store(RuntimeStoreMedium::PersistentFilesystem, 8 * GIB, 128 * GIB);

    assert_eq!(volatile.store_medium, RuntimeStoreMedium::VolatileMemory);
    assert_eq!(
        volatile.static_platform_manifest.store_medium,
        RuntimeStoreMedium::VolatileMemory
    );
    assert_eq!(
        persistent.store_medium,
        RuntimeStoreMedium::PersistentFilesystem
    );
    assert_ne!(volatile.report_id, persistent.report_id);
}

#[test]
fn machine_resource_snapshot_does_not_claim_runtime_activity_or_health() {
    let report = compile_host_store(RuntimeStoreMedium::VolatileMemory, 8 * GIB, 128 * GIB);
    let value = serde_json::to_value(&report.resource_snapshot)
        .expect("serialize runtime resource snapshot");

    for forbidden in [
        "activeHttpCount",
        "activeWssCount",
        "activeRuntimeJobs",
        "inboundQueueDepth",
        "outboundQueueDepth",
        "tlsFragmentationRisk",
        "storageContentionRisk",
    ] {
        assert!(
            value.get(forbidden).is_none(),
            "machine snapshot must not own {forbidden}"
        );
    }
}

#[test]
fn production_budget_surface_has_no_static_runtime_report_constructor() {
    let source = include_str!("../src/budget.rs");

    assert!(source.contains("runtime-budget-public-surface: authority-only-report"));
    assert!(!source.contains("pub fn static_for_profile"));
}
