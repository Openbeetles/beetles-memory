use bm_sdk::{
    platform_capability_snapshot, platform_capability_snapshot_file_name,
    resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId,
    PLATFORM_CAPABILITY_SNAPSHOT_SCHEMA,
};
use std::path::PathBuf;

fn profiles() -> [ProfileId; 8] {
    [
        ProfileId::EspStandaloneMemory,
        ProfileId::EspEmbeddedSdk,
        ProfileId::LinuxDeviceStandaloneMemory,
        ProfileId::DesktopMacosStandaloneMemory,
        ProfileId::DesktopMacosEmbeddedSdk,
        ProfileId::DesktopWindowsEmbeddedSdk,
        ProfileId::ServerLinuxMemoryGateway,
        ProfileId::ServerLinuxDevFull,
    ]
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn active_profile_feature() -> Option<ProfileId> {
    if cfg!(feature = "profile-esp-standalone-memory") {
        Some(ProfileId::EspStandaloneMemory)
    } else if cfg!(feature = "profile-esp-embedded-sdk") {
        Some(ProfileId::EspEmbeddedSdk)
    } else if cfg!(feature = "profile-linux-device-standalone-memory") {
        Some(ProfileId::LinuxDeviceStandaloneMemory)
    } else if cfg!(feature = "profile-desktop-macos-standalone-memory") {
        Some(ProfileId::DesktopMacosStandaloneMemory)
    } else if cfg!(feature = "profile-desktop-macos-embedded-sdk") {
        Some(ProfileId::DesktopMacosEmbeddedSdk)
    } else if cfg!(feature = "profile-desktop-windows-embedded-sdk") {
        Some(ProfileId::DesktopWindowsEmbeddedSdk)
    } else if cfg!(feature = "profile-server-linux-memory-gateway") {
        Some(ProfileId::ServerLinuxMemoryGateway)
    } else if cfg!(feature = "profile-server-linux-dev-full") {
        Some(ProfileId::ServerLinuxDevFull)
    } else {
        None
    }
}

#[test]
fn platform_capability_snapshots_match_committed_fixtures() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();
    let fixture_root = workspace_root().join("fixtures/platform/capabilities");
    let active_profile = active_profile_feature();

    for profile in profiles() {
        let expected = std::fs::read_to_string(fixture_root.join(format!(
            "{}.json",
            platform_capability_snapshot_file_name(profile)
        )))
        .expect("committed snapshot fixture");
        let expected_json: serde_json::Value =
            serde_json::from_str(&expected).expect("fixture json");
        assert_eq!(expected_json["schema"], PLATFORM_CAPABILITY_SNAPSHOT_SCHEMA);
        assert_eq!(
            expected_json["profile"],
            platform_capability_snapshot_file_name(profile)
        );

        if Some(profile) != active_profile {
            continue;
        }

        let catalog =
            resolve_memory_capabilities(profile, &policy, &privacy).expect("capability catalog");
        let snapshot = platform_capability_snapshot(&catalog);
        let actual = serde_json::to_string_pretty(&snapshot).expect("snapshot json");

        assert_eq!(
            actual.trim(),
            expected.trim(),
            "capability snapshot drifted for {}",
            platform_capability_snapshot_file_name(profile)
        );
    }
}
