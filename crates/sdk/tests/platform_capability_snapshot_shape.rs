use bm_sdk::{
    platform_capability_snapshot, platform_capability_snapshot_file_name,
    platform_profile_feature_id, resolve_memory_capabilities, MemoryCapabilityPolicy,
    MemoryPrivacyPolicy, ProfileId,
};

#[test]
fn snapshot_uses_cargo_profile_feature_ids_not_internal_display_strings() {
    assert_eq!(
        platform_profile_feature_id(ProfileId::LinuxDeviceStandaloneMemory),
        "profile-linux-device-standalone-memory"
    );
    assert_eq!(
        platform_capability_snapshot_file_name(ProfileId::ServerLinuxMemoryGateway),
        "profile-server-linux-memory-gateway"
    );
    assert_eq!(
        platform_profile_feature_id(ProfileId::DesktopLinuxEmbeddedSdk),
        "profile-desktop-linux-embedded-sdk"
    );
}

#[test]
fn snapshot_shape_is_stable_and_reviewable() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();
    let catalog = resolve_memory_capabilities(ProfileId::EspStandaloneMemory, &policy, &privacy)
        .expect("capability catalog");
    let snapshot = platform_capability_snapshot(&catalog);
    let value = serde_json::to_value(&snapshot).expect("snapshot json");

    assert_eq!(value["schema"], "beetle-memory.platform.capability.v3");
    assert_eq!(value["profile"], "profile-esp-standalone-memory");
    assert_eq!(value["target"], "target-esp");
    assert_eq!(value["role"], "role-standalone-memory");
    assert_eq!(value["compiled"]["sqlite_index_compiled"], false);
    assert_eq!(value["memory"]["write"], true);
    assert!(value["compiled"].get("target_desktop_linux").is_some());
    assert!(value["memory"].get("transcript_export").is_some());
    assert_eq!(value["adapter"]["wss"]["client_allowed"], true);
    assert_eq!(value["adapter"]["wss"]["server_allowed"], false);
    assert_eq!(value["entry"]["llm_gateway_server"]["visible"], false);
    assert_eq!(
        value["entry"]["llm_gateway_server"]["server_allowed"],
        false
    );
    assert_eq!(
        value["governed_state"]["dynamic_state_recall"]["profile_allowed"],
        true
    );
    assert_eq!(
        value["governed_state"]["dynamic_state_recall"]["visible"],
        false
    );
    assert_eq!(
        value["governed_state"]["runtime_skill_recall_transport"],
        "unavailable"
    );
}
