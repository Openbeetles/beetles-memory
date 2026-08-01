use bm_adapter::{AdapterBudget, AdapterPolicy};
use bm_sdk::{MemoryStoreHandle, ProfileId, StoreBackendConfig};

fn host_profile() -> ProfileId {
    #[cfg(target_os = "macos")]
    {
        ProfileId::DesktopMacosEmbeddedSdk
    }
    #[cfg(target_os = "windows")]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(target_os = "linux")]
    {
        ProfileId::DesktopLinuxEmbeddedSdk
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        ProfileId::EspEmbeddedSdk
    }
}

#[test]
fn adapter_policy_budget_is_the_runtime_report_budget() {
    let report = MemoryStoreHandle::open(
        StoreBackendConfig::in_memory(host_profile()).expect("store config"),
    )
    .expect("store")
    .runtime_budget();
    let policy = AdapterPolicy::authenticated();

    let budget: &AdapterBudget = policy.runtime_budget(&report);
    assert_eq!(budget, &report.adapter_budget);
}

#[test]
fn adapter_policy_has_no_profile_owned_budget_constructors() {
    let source = include_str!("../src/policy.rs");

    assert!(!source.contains("standard_server"));
    assert!(!source.contains("compact_device"));
    assert!(!source.contains("budget: AdapterBudget"));
    assert!(!source.contains("1024 * 1024"));
    assert!(!source.contains("64 * 1024"));
}
