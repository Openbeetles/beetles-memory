use bm_core::budget::{
    compile_runtime_budget, RuntimeBudgetInput, RuntimeBudgetReport, RuntimeStoreMedium,
    StaticPlatformManifest,
};
use bm_core::feature_gate::{profile_capability_catalog, ProfileId, RoleFeature, TargetFeature};
use bm_core::resource::{
    RuntimeResourceProbeSource, RuntimeResourceSnapshot, RuntimeResourceUnavailableReason,
};

fn compile_fixture(profile: ProfileId, store_medium: RuntimeStoreMedium) -> RuntimeBudgetReport {
    compile_runtime_budget(RuntimeBudgetInput {
        profile,
        resource_snapshot: RuntimeResourceSnapshot::unavailable(
            10,
            RuntimeResourceProbeSource::StaticManifest,
            RuntimeResourceUnavailableReason::ProbeNotConfigured,
        ),
        static_platform_manifest: StaticPlatformManifest::for_profile(profile, store_medium),
        provider_model_context_limit: None,
    })
}

#[test]
fn every_first_class_profile_has_a_catalog_entry() {
    let profiles = [
        ProfileId::EspStandaloneMemory,
        ProfileId::EspEmbeddedSdk,
        ProfileId::LinuxDeviceStandaloneMemory,
        ProfileId::DesktopMacosStandaloneMemory,
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopMacosDevFull,
        ProfileId::DesktopLinuxEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
        ProfileId::DesktopWindowsDevFull,
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
fn profile_identity_exposes_canonical_target_and_role() {
    let cases = [
        (
            ProfileId::DesktopMacosDevFull,
            "target-desktop-macos+role-dev-full",
            TargetFeature::DesktopMacos,
            RoleFeature::DevFull,
        ),
        (
            ProfileId::DesktopLinuxEmbeddedSdk,
            "target-desktop-linux+role-embedded-sdk",
            TargetFeature::DesktopLinux,
            RoleFeature::EmbeddedSdk,
        ),
        (
            ProfileId::DesktopWindowsDevFull,
            "target-desktop-windows+role-dev-full",
            TargetFeature::DesktopWindows,
            RoleFeature::DevFull,
        ),
        (
            ProfileId::ServerLinuxDevFull,
            "target-server-linux+role-dev-full",
            TargetFeature::ServerLinux,
            RoleFeature::DevFull,
        ),
    ];

    for (profile, canonical_id, target, role) in cases {
        assert_eq!(profile.as_str(), canonical_id);
        assert_eq!(profile.target(), target);
        assert_eq!(profile.role(), role);
    }
}

#[test]
fn linux_desktop_embedded_sdk_is_not_a_server_gateway_role() {
    let catalog = profile_capability_catalog();
    let embedded = catalog
        .iter()
        .find(|entry| entry.profile == ProfileId::DesktopLinuxEmbeddedSdk)
        .expect("Linux desktop embedded SDK profile");

    assert_eq!(embedded.target, TargetFeature::DesktopLinux);
    assert_eq!(embedded.role, RoleFeature::EmbeddedSdk);
    assert!(!embedded.llm_gateway_server_allowed);
    assert!(!embedded.adapter.a2a.allowed);
}

#[test]
fn native_dev_full_uses_the_compilation_target() {
    #[cfg(target_os = "macos")]
    assert_eq!(
        ProfileId::native_dev_full(),
        Some(ProfileId::DesktopMacosDevFull)
    );
    #[cfg(target_os = "windows")]
    assert_eq!(
        ProfileId::native_dev_full(),
        Some(ProfileId::DesktopWindowsDevFull)
    );
    #[cfg(target_os = "linux")]
    assert_eq!(
        ProfileId::native_dev_full(),
        Some(ProfileId::ServerLinuxDevFull)
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    assert_eq!(ProfileId::native_dev_full(), None);
}

#[test]
fn manifest_and_report_expose_structured_deployment_identity() {
    let profile = ProfileId::DesktopMacosDevFull;
    let manifest = StaticPlatformManifest::for_profile(profile, RuntimeStoreMedium::VolatileMemory);
    let report = compile_fixture(profile, RuntimeStoreMedium::VolatileMemory);

    assert_eq!(manifest.deployment_target, TargetFeature::DesktopMacos);
    assert_eq!(manifest.deployment_role, RoleFeature::DevFull);
    assert_eq!(report.deployment_target, TargetFeature::DesktopMacos);
    assert_eq!(report.deployment_role, RoleFeature::DevFull);

    let value = serde_json::to_value(&report).expect("serialize runtime budget report");
    assert_eq!(value["deploymentTarget"], "target-desktop-macos");
    assert_eq!(value["deploymentRole"], "role-dev-full");
    assert_eq!(
        value["staticPlatformManifest"]["deploymentTarget"],
        "target-desktop-macos"
    );
    assert_eq!(
        value["staticPlatformManifest"]["deploymentRole"],
        "role-dev-full"
    );
}

#[test]
fn desktop_dev_full_profiles_share_server_dev_full_budget_semantics() {
    let server = compile_fixture(
        ProfileId::ServerLinuxDevFull,
        RuntimeStoreMedium::VolatileMemory,
    );
    for desktop_profile in [
        ProfileId::DesktopMacosDevFull,
        ProfileId::DesktopWindowsDevFull,
    ] {
        let desktop = compile_fixture(desktop_profile, RuntimeStoreMedium::VolatileMemory);
        assert_eq!(desktop.memory_core_budget, server.memory_core_budget);
        assert_eq!(
            desktop.graph_expansion_budget,
            server.graph_expansion_budget
        );
        assert_eq!(desktop.facet_recall_budget, server.facet_recall_budget);
        assert_eq!(
            desktop.recall_delivery_budget,
            server.recall_delivery_budget
        );
        assert_eq!(
            desktop.evidence_document_budget,
            server.evidence_document_budget
        );
        assert_eq!(desktop.store_budget, server.store_budget);
        assert_eq!(desktop.adapter_budget, server.adapter_budget);
        assert_eq!(
            desktop.projection_source_budget,
            server.projection_source_budget
        );
        assert_eq!(
            desktop.projection_render_budget,
            server.projection_render_budget
        );
        assert_eq!(desktop.maintenance_budget, server.maintenance_budget);
        assert_eq!(desktop.runtime_job_budget, server.runtime_job_budget);
        assert_eq!(desktop.llm_gateway_budget, server.llm_gateway_budget);
        assert_eq!(
            desktop.transcript_governance_budget,
            server.transcript_governance_budget
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
    assert!(standalone.llm_gateway_server_allowed);
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
