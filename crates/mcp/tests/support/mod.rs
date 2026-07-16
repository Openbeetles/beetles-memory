use bm_sdk::ProfileId;

pub fn native_runtime_profile() -> ProfileId {
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
        ProfileId::ServerLinuxMemoryGateway
    }
}
