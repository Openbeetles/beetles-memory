use bm_sdk::{resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId};

#[test]
fn esp_embedded_sdk_does_not_surface_adapter_transports() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let catalog = resolve_memory_capabilities(ProfileId::EspEmbeddedSdk, &policy, &privacy)
        .expect("esp embedded capability catalog");

    assert!(!catalog.communication_adapter.profile_allowed);
    assert!(!catalog.communication_adapter.visible);
    assert!(!catalog.adapter.cli.profile_allowed);
    assert!(!catalog.adapter.http.profile_allowed);
    assert!(!catalog.adapter.wss.profile_allowed);
    assert!(!catalog.adapter.mcp.profile_allowed);
    assert!(!catalog.adapter.a2a.profile_allowed);
}

#[test]
fn server_gateway_adapter_visibility_stays_sdk_catalog_owned() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let catalog =
        resolve_memory_capabilities(ProfileId::ServerLinuxMemoryGateway, &policy, &privacy)
            .expect("server gateway capability catalog");

    assert!(catalog.communication_adapter.visible);
    assert!(catalog.adapter.cli.visible);
    assert!(catalog.adapter.http.visible);
    assert!(catalog.adapter.http.server_allowed);
    assert!(catalog.adapter.wss.visible);
    assert!(catalog.adapter.mcp.visible);
    assert!(catalog.adapter.mcp.server_allowed);
    assert!(catalog.adapter.a2a.visible);
    assert!(catalog.entry.llm_gateway_server.visible);
    assert!(catalog.entry.llm_gateway_server.server_allowed);
}
