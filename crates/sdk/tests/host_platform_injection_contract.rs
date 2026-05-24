use std::sync::Arc;

use bm_sdk::{
    MemoryIdentity, MemoryRuntime, MemoryScope, Platform, ProfileId, StoreBackendConfig,
    StorePlatform,
};

#[test]
fn custom_host_platform_can_build_runtime_without_store_platform_routing() {
    let store = StorePlatform::open_in_memory(
        StoreBackendConfig::in_memory(ProfileId::ServerLinuxDevFull).expect("config"),
    )
    .expect("store");
    let platform: Arc<dyn Platform> = store.into_arc();

    let runtime = MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("local", "chat-1").expect("scope"))
        .profile(ProfileId::ServerLinuxDevFull)
        .platform(platform)
        .build()
        .expect("runtime");

    assert_eq!(runtime.identity().agent_id, "agent-main");
    assert_eq!(runtime.scope().chat_id, "chat-1");
}
