use bm_sdk::ProfileId;

pub fn native_runtime_profile() -> ProfileId {
    #[cfg(feature = "nonproduction-replay-harness")]
    {
        ProfileId::native_dev_full().expect("native dev-full profile")
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "macos"))]
    {
        ProfileId::DesktopMacosEmbeddedSdk
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "windows"))]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "linux"))]
    {
        ProfileId::ServerLinuxMemoryGateway
    }
    #[cfg(all(
        not(feature = "nonproduction-replay-harness"),
        not(any(target_os = "macos", target_os = "windows", target_os = "linux"))
    ))]
    {
        compile_error!("HTTP contract tests require a supported production host target");
    }
}
