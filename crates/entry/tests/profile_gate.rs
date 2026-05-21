use bm_entry::{entry_capability_view, EntryTransportConfig};
use bm_sdk::{
    MemoryAdapterCapabilityPolicy, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId,
};

fn policy() -> MemoryCapabilityPolicy {
    MemoryCapabilityPolicy {
        communication_adapter_enabled: true,
        adapter: MemoryAdapterCapabilityPolicy::all_enabled(),
        ..MemoryCapabilityPolicy::strict_profile()
    }
}

#[test]
fn esp_standalone_has_compact_entry_but_no_server_listener() {
    let view = entry_capability_view(
        ProfileId::EspStandaloneMemory,
        &policy(),
        &MemoryPrivacyPolicy::standard_private_boundary(),
        &EntryTransportConfig::all_enabled(),
    )
    .expect("view");

    assert!(view.cli.visible);
    assert!(view.wss_client.visible);
    assert!(!view.http_server.visible);
    assert!(!view.mcp_server.visible);
    assert!(!view.a2a_bridge.visible);
}

#[test]
fn esp_embedded_sdk_hides_listener_entry_by_default() {
    let view = entry_capability_view(
        ProfileId::EspEmbeddedSdk,
        &policy(),
        &MemoryPrivacyPolicy::standard_private_boundary(),
        &EntryTransportConfig::all_enabled(),
    )
    .expect("view");

    assert!(!view.cli.visible);
    assert!(!view.http_server.visible);
    assert!(!view.wss_server.visible);
    assert!(!view.mcp_server.visible);
    assert!(!view.a2a_bridge.visible);
}

#[test]
fn linux_server_gateway_exposes_full_server_entry_set() {
    let view = entry_capability_view(
        ProfileId::ServerLinuxMemoryGateway,
        &policy(),
        &MemoryPrivacyPolicy::standard_private_boundary(),
        &EntryTransportConfig::all_enabled(),
    )
    .expect("view");

    assert!(view.cli.visible);
    assert!(view.http_server.visible);
    assert!(view.wss_server.visible);
    assert!(view.mcp_server.visible);
    assert!(view.a2a_bridge.visible);
}
