use bm_core::budget::{RuntimeStoreMedium, StaticPlatformManifest};
use bm_core::feature_gate::ProfileId;
use bm_core::orchestrator::PressureLevel;
use bm_core::resource::{
    HostRuntimeResourceProbe, RuntimeResourceObservation, RuntimeResourceProbe,
    RuntimeResourceProbeSource, RuntimeResourceUnavailableReason,
};
use bm_core::RuntimeBudgetAuthority;
use std::sync::Arc;

#[derive(Clone)]
struct FirmwareProbe {
    observation: RuntimeResourceObservation,
}

impl RuntimeResourceProbe for FirmwareProbe {
    fn probe(&self, _now_secs: u64) -> bm_core::Result<RuntimeResourceObservation> {
        Ok(self.observation.clone())
    }
}

fn valid_firmware_observation() -> RuntimeResourceObservation {
    let mut observation = RuntimeResourceObservation::unavailable(
        10,
        RuntimeResourceUnavailableReason::ProbeNotConfigured,
    );
    observation.pressure = PressureLevel::Normal;
    observation.internal_heap_free_bytes = Some(8 * 1024 * 1024);
    observation.internal_heap_minimum_free_bytes = Some(6 * 1024 * 1024);
    observation.internal_heap_largest_block_bytes = Some(4 * 1024 * 1024);
    observation.psram_total_bytes = Some(16 * 1024 * 1024);
    observation.psram_free_bytes = Some(12 * 1024 * 1024);
    observation.psram_largest_block_bytes = Some(8 * 1024 * 1024);
    observation.storage_total_bytes = Some(64 * 1024 * 1024);
    observation.storage_available_bytes = Some(32 * 1024 * 1024);
    observation.unavailable_reason = None;
    observation
}

fn firmware_authority(
    observation: RuntimeResourceObservation,
) -> bm_core::Result<RuntimeBudgetAuthority> {
    let profile = ProfileId::EspStandaloneMemory;
    RuntimeBudgetAuthority::with_firmware_probe(
        profile,
        StaticPlatformManifest::for_profile(profile, RuntimeStoreMedium::EmbeddedFlash),
        None,
        Arc::new(FirmwareProbe { observation }),
        10,
    )
}

#[test]
fn probe_payload_has_no_source_and_firmware_registration_attests_it() {
    let observation = valid_firmware_observation();
    let payload = serde_json::to_value(&observation).expect("serialize resource observation");
    assert!(payload.get("source").is_none());

    let authority = firmware_authority(observation).unwrap();
    assert_eq!(
        authority.current_snapshot(10).source,
        RuntimeResourceProbeSource::FirmwareManifest
    );
}

#[test]
fn valid_esp_available_snapshot_is_accepted() {
    let authority = firmware_authority(valid_firmware_observation()).unwrap();
    let snapshot = authority.current_snapshot(10);

    assert_eq!(snapshot.unavailable_reason, None);
    assert_eq!(snapshot.available_parallelism, None);
    assert_eq!(snapshot.memory_total_bytes, None);
    assert_eq!(snapshot.memory_available_bytes, None);
}

#[test]
fn esp_available_snapshot_requires_complete_consistent_firmware_facts() {
    let mut missing_heap_fact = valid_firmware_observation();
    missing_heap_fact.internal_heap_largest_block_bytes = None;
    let error = firmware_authority(missing_heap_fact).unwrap_err();
    assert_eq!(error.stage(), "runtime_budget_resource_snapshot");

    let mut impossible_heap = valid_firmware_observation();
    impossible_heap.internal_heap_minimum_free_bytes = Some(9 * 1024 * 1024);
    let error = firmware_authority(impossible_heap).unwrap_err();
    assert_eq!(error.stage(), "runtime_budget_resource_snapshot");

    let mut partial_psram = valid_firmware_observation();
    partial_psram.psram_largest_block_bytes = None;
    let error = firmware_authority(partial_psram).unwrap_err();
    assert_eq!(error.stage(), "runtime_budget_resource_snapshot");

    let mut impossible_storage = valid_firmware_observation();
    impossible_storage.storage_available_bytes = Some(65 * 1024 * 1024);
    let error = firmware_authority(impossible_storage).unwrap_err();
    assert_eq!(error.stage(), "runtime_budget_resource_snapshot");
}

#[test]
fn esp_unavailable_snapshot_is_legal_but_rejects_host_facts() {
    let unavailable = RuntimeResourceObservation::unavailable(
        10,
        RuntimeResourceUnavailableReason::ProbeNotConfigured,
    );
    assert!(firmware_authority(unavailable).is_ok());

    let mut leaked_host_cpu = RuntimeResourceObservation::unavailable(
        10,
        RuntimeResourceUnavailableReason::ProbeNotConfigured,
    );
    leaked_host_cpu.available_parallelism = Some(8);
    let error = firmware_authority(leaked_host_cpu).unwrap_err();
    assert_eq!(error.stage(), "runtime_budget_resource_snapshot");
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn native_production_profile() -> ProfileId {
    #[cfg(target_os = "macos")]
    {
        ProfileId::DesktopMacosStandaloneMemory
    }
    #[cfg(target_os = "linux")]
    {
        ProfileId::LinuxDeviceStandaloneMemory
    }
    #[cfg(target_os = "windows")]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
#[test]
fn production_host_authority_accepts_only_core_owned_probe() {
    let profile = native_production_profile();
    let authority = RuntimeBudgetAuthority::with_host_probe(
        profile,
        StaticPlatformManifest::for_profile(profile, RuntimeStoreMedium::VolatileMemory),
        None,
        HostRuntimeResourceProbe::for_volatile_memory(),
        10,
    )
    .unwrap();

    #[cfg(target_os = "macos")]
    assert_eq!(
        authority.current_snapshot(10).source,
        RuntimeResourceProbeSource::HostMacos
    );
    #[cfg(target_os = "linux")]
    assert_eq!(
        authority.current_snapshot(10).source,
        RuntimeResourceProbeSource::HostLinux
    );
    #[cfg(target_os = "windows")]
    assert_eq!(
        authority.current_snapshot(10).source,
        RuntimeResourceProbeSource::HostWindows
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn trusted_host_probe_binds_storage_observation_to_data_path() {
    let profile = native_production_profile();
    let authority = RuntimeBudgetAuthority::with_host_probe(
        profile,
        StaticPlatformManifest::for_profile(profile, RuntimeStoreMedium::PersistentFilesystem),
        None,
        HostRuntimeResourceProbe::for_persistent_filesystem(std::env::temp_dir()).unwrap(),
        10,
    )
    .unwrap();
    let snapshot = authority.current_snapshot(10);

    assert!(snapshot.storage_total_bytes.is_some());
    assert!(snapshot.storage_available_bytes.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn server_linux_profile_cannot_fake_a_macos_host_registration() {
    let profile = ProfileId::ServerLinuxMemoryGateway;
    let error = RuntimeBudgetAuthority::with_host_probe(
        profile,
        StaticPlatformManifest::for_profile(profile, RuntimeStoreMedium::VolatileMemory),
        None,
        HostRuntimeResourceProbe::for_volatile_memory(),
        10,
    )
    .unwrap_err();

    assert_eq!(error.stage(), "runtime_budget_authority_config");
}

#[cfg(all(target_os = "macos", feature = "nonproduction-replay-harness"))]
#[test]
fn macos_dev_full_passes_but_server_dev_full_fails_on_real_macos_host() {
    let mac_profile = ProfileId::DesktopMacosDevFull;
    RuntimeBudgetAuthority::with_host_probe(
        mac_profile,
        StaticPlatformManifest::for_profile(mac_profile, RuntimeStoreMedium::VolatileMemory),
        None,
        HostRuntimeResourceProbe::for_volatile_memory(),
        10,
    )
    .unwrap();

    let linux_profile = ProfileId::ServerLinuxDevFull;
    let error = RuntimeBudgetAuthority::with_host_probe(
        linux_profile,
        StaticPlatformManifest::for_profile(linux_profile, RuntimeStoreMedium::VolatileMemory),
        None,
        HostRuntimeResourceProbe::for_volatile_memory(),
        10,
    )
    .unwrap_err();
    assert_eq!(error.stage(), "runtime_budget_authority_config");
}

#[test]
fn production_source_declares_core_owned_host_registration_surface() {
    let source = include_str!("../src/resource.rs");

    assert!(source.contains("runtime-resource-public-surface: core-owned-host-probe"));
    assert!(!source.contains("pub struct RuntimeResourceProbeRegistration"));
    assert!(!source.contains("pub fn host(probe: Arc<dyn RuntimeResourceProbe>)"));
}
