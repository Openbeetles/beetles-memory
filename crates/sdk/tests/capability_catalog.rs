use bm_sdk::{resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId};

#[test]
fn esp_standalone_and_embedded_sdk_have_distinct_visible_catalogs() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let standalone = resolve_memory_capabilities(ProfileId::EspStandaloneMemory, &policy, &privacy)
        .expect("standalone catalog");
    let embedded = resolve_memory_capabilities(ProfileId::EspEmbeddedSdk, &policy, &privacy)
        .expect("embedded catalog");

    assert_ne!(standalone.profile, embedded.profile);
    assert!(standalone.write.visible);
    assert!(standalone.recall.visible);
    assert!(standalone.projection.visible);
    assert!(!standalone.sqlite_index_recall.archive.visible);
    assert!(!standalone.communication_adapter.visible);

    assert!(embedded.write.visible);
    assert!(embedded.recall.visible);
    assert!(embedded.projection.visible);
    assert!(!embedded.maintenance.visible);
    assert!(!embedded.replay.visible);
    assert!(!embedded.sqlite_index_recall.archive.visible);
    assert!(!embedded.communication_adapter.visible);
}

#[test]
fn server_gateway_can_surface_adapter_permission_without_creating_adapter_code() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let catalog =
        resolve_memory_capabilities(ProfileId::ServerLinuxMemoryGateway, &policy, &privacy)
            .expect("server gateway catalog");

    assert!(catalog.communication_adapter.profile_allowed);
    assert!(catalog.communication_adapter.config_enabled);
    assert!(catalog.communication_adapter.visible);
}

#[test]
fn privacy_gate_blocks_projection_and_export_visibility() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy {
        prompt_projection_allowed: false,
        private_plane_projection_allowed: false,
        operator_inspection_allowed: true,
        export_allowed: false,
        import_allowed: true,
    };

    let catalog = resolve_memory_capabilities(ProfileId::ServerLinuxDevFull, &policy, &privacy)
        .expect("dev full catalog");

    assert!(!catalog.projection.visible);
    assert!(!catalog.export.visible);
    assert!(catalog.import.visible);
}
