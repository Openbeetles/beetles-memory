use bm_entry::{EntryAuthConfig, EntryAuthDecision, EntryLocalTransport};
use bm_sdk::ProfileId;

#[allow(dead_code)]
pub fn trusted_local_auth(principal: &str) -> EntryAuthDecision {
    EntryAuthConfig::disabled_for_local()
        .authenticate_local_transport(EntryLocalTransport::InProcess, principal)
}

pub fn host_production_profile() -> ProfileId {
    #[cfg(target_os = "macos")]
    {
        ProfileId::DesktopMacosStandaloneMemory
    }
    #[cfg(target_os = "windows")]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(target_os = "linux")]
    {
        ProfileId::ServerLinuxMemoryGateway
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        ProfileId::EspEmbeddedSdk
    }
}
