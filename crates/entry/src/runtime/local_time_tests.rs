use super::*;
use bm_sdk::UserLocalTimeContext;

struct PerInstantLocalTime;

impl UserLocalTimeProvider for PerInstantLocalTime {
    fn local_time_at(&self, unix_secs: u64) -> Result<Option<UserLocalTimeContext>> {
        // Synthetic host context deliberately varies by instant. Entry must
        // retain the provider, not snapshot an offset while opening a runtime.
        let offset = if unix_secs < 1_720_000_000 {
            28_800
        } else {
            32_400
        };
        UserLocalTimeContext::new(unix_secs, offset, "Synthetic/UserZone").map(Some)
    }
}

struct UnknownLocalTime;

impl UserLocalTimeProvider for UnknownLocalTime {
    fn local_time_at(&self, _unix_secs: u64) -> Result<Option<UserLocalTimeContext>> {
        Ok(None)
    }
}

fn config(chat_id: &str) -> EntryRuntimeConfig {
    #[cfg(target_os = "macos")]
    let profile = ProfileId::DesktopMacosEmbeddedSdk;
    #[cfg(target_os = "linux")]
    let profile = ProfileId::DesktopLinuxEmbeddedSdk;
    #[cfg(target_os = "windows")]
    let profile = ProfileId::DesktopWindowsEmbeddedSdk;
    let mut capability = MemoryCapabilityPolicy::strict_profile();
    capability.communication_adapter_enabled = true;
    capability.maintenance_enabled = false;
    EntryRuntimeConfig {
        identity: EntryIdentity {
            agent_id: "local-time-agent".into(),
            owner_id: "local-time-owner".into(),
        },
        scope: EntryScope {
            channel: "local-time-test".into(),
            chat_id: chat_id.into(),
            conversation_id: None,
        },
        store: StoreBackendConfig::in_memory(profile).unwrap(),
        transports: EntryTransportConfig::all_enabled(),
        auth: EntryAuthConfig::disabled_for_local(),
        idempotency: EntryIdempotencyConfig { max_keys: 8 },
        privacy: MemoryPrivacyPolicy::standard_private_boundary(),
        capability,
    }
}

fn assert_provider(runtime: &MemoryRuntime, provider: &Arc<dyn UserLocalTimeProvider>) {
    assert!(Arc::ptr_eq(
        &runtime.config().user_local_time_provider,
        provider
    ));
    for (instant, expected_offset) in [(1_700_000_000, 28_800), (1_730_000_000, 32_400)] {
        let local = runtime
            .config()
            .user_local_time_provider
            .local_time_at(instant)
            .unwrap()
            .unwrap();
        assert_eq!(local.unix_secs(), instant);
        assert_eq!(local.utc_offset_seconds(), expected_offset);
        assert_eq!(local.time_zone(), "Synthetic/UserZone");
    }
}

#[test]
fn standalone_and_governor_remount_keep_the_same_live_provider() {
    let provider: Arc<dyn UserLocalTimeProvider> = Arc::new(PerInstantLocalTime);
    let entry = EntryRuntime::open_with_user_local_time_provider(
        config("standalone"),
        Arc::clone(&provider),
    )
    .unwrap();
    assert_provider(entry.runtime(), &provider);
    let governor = governor_control_runtime(entry.runtime(), entry.store.clone()).unwrap();
    assert_provider(&governor, &provider);
    assert_eq!(
        governor.memory_space_id(),
        entry.runtime().memory_space_id()
    );
    assert_eq!(
        governor.scoped_runtime().mounted_subject_id,
        entry.runtime().scoped_runtime().mounted_subject_id
    );
    assert_ne!(
        governor.scoped_runtime().actor_subject_id,
        entry.runtime().scoped_runtime().actor_subject_id
    );
}

#[test]
fn factory_and_manager_remounts_preserve_provider_after_cache_eviction() {
    let provider: Arc<dyn UserLocalTimeProvider> = Arc::new(PerInstantLocalTime);
    let first = config("first");
    let second = config("second");
    let factory = EntryRuntimeFactory::open_with_user_local_time_provider(
        first.base_config(),
        Arc::clone(&provider),
    )
    .unwrap();
    let directly_mounted = factory.runtime_for_scope(first.runtime_scope()).unwrap();
    assert_provider(directly_mounted.runtime(), &provider);
    drop(directly_mounted);

    let manager = EntryRuntimeManager::from_factory(factory, Some(1)).unwrap();
    let original = manager.runtime_for_scope(first.runtime_scope()).unwrap();
    assert_provider(original.runtime(), &provider);
    let original_weak = Arc::downgrade(&original);
    drop(original);
    let next = manager.runtime_for_scope(second.runtime_scope()).unwrap();
    assert_provider(next.runtime(), &provider);
    assert!(
        original_weak.upgrade().is_none(),
        "the first runtime must really be evicted"
    );
    drop(next);
    let remounted = manager.runtime_for_scope(first.runtime_scope()).unwrap();
    assert_provider(remounted.runtime(), &provider);
    let governor = governor_control_runtime(remounted.runtime(), remounted.store.clone()).unwrap();
    assert_provider(&governor, &provider);
}

#[test]
fn explicit_unknown_context_is_not_replaced_by_the_entry_host_time_zone() {
    let provider: Arc<dyn UserLocalTimeProvider> = Arc::new(UnknownLocalTime);
    let entry =
        EntryRuntime::open_with_user_local_time_provider(config("unknown"), Arc::clone(&provider))
            .unwrap();
    let governor = governor_control_runtime(entry.runtime(), entry.store.clone()).unwrap();
    for runtime in [entry.runtime(), &governor] {
        assert!(Arc::ptr_eq(
            &runtime.config().user_local_time_provider,
            &provider
        ));
        assert!(runtime
            .config()
            .user_local_time_provider
            .local_time_at(1_700_000_000)
            .unwrap()
            .is_none());
    }
}

#[test]
fn manager_rejects_zero_capacity_with_an_injected_factory() {
    let factory = EntryRuntimeFactory::open_with_user_local_time_provider(
        config("zero-capacity").base_config(),
        Arc::new(UnknownLocalTime),
    )
    .unwrap();
    assert!(EntryRuntimeManager::from_factory(factory, Some(0)).is_err());
}
