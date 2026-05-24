use bm_core::feature_gate::{profile_capability_catalog, ProfileId};

#[test]
fn every_first_class_profile_has_a_catalog_entry() {
    let profiles = [
        ProfileId::EspStandaloneMemory,
        ProfileId::EspEmbeddedSdk,
        ProfileId::LinuxDeviceStandaloneMemory,
        ProfileId::DesktopMacosStandaloneMemory,
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
        ProfileId::ServerLinuxMemoryGateway,
        ProfileId::ServerLinuxDevFull,
    ];
    let catalog = profile_capability_catalog();

    for profile in profiles {
        assert!(
            catalog.iter().any(|entry| entry.profile == profile),
            "missing profile catalog entry for {:?}",
            profile
        );
    }
}

#[test]
fn macos_desktop_standalone_and_embedded_sdk_keep_distinct_runtime_roles() {
    let catalog = profile_capability_catalog();
    let standalone = catalog
        .iter()
        .find(|entry| entry.profile == ProfileId::DesktopMacosStandaloneMemory)
        .expect("macOS standalone desktop profile");
    let embedded = catalog
        .iter()
        .find(|entry| entry.profile == ProfileId::DesktopMacosEmbeddedSdk)
        .expect("macOS embedded sdk profile");

    assert_ne!(standalone.role, embedded.role);
    assert!(standalone.communication_adapter_allowed);
    assert!(standalone.adapter.cli.allowed);
    assert!(standalone.adapter.http.server_allowed);
    assert!(standalone.adapter.wss.allowed);
    assert!(!embedded.adapter.a2a.allowed);
}

#[test]
fn esp_standalone_and_embedded_sdk_keep_distinct_runtime_roles() {
    let catalog = profile_capability_catalog();
    let standalone = catalog
        .iter()
        .find(|entry| entry.profile == ProfileId::EspStandaloneMemory)
        .expect("esp standalone profile");
    let embedded = catalog
        .iter()
        .find(|entry| entry.profile == ProfileId::EspEmbeddedSdk)
        .expect("esp embedded sdk profile");

    assert_ne!(standalone.role, embedded.role);
    assert!(standalone.communication_adapter_allowed);
    assert!(!standalone.llm_gateway_server_allowed);
    assert!(!embedded.communication_adapter_allowed);
    assert!(!embedded.llm_gateway_server_allowed);
    assert!(standalone.adapter.wss.client_allowed);
    assert!(!embedded.adapter.wss.allowed);
    assert!(!standalone.sqlite_index_allowed);
    assert!(!embedded.sqlite_index_allowed);
}
