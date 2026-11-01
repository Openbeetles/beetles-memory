//! User-device time presentation. Persistent timestamps remain UTC instants.

use std::sync::Arc;

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use crate::Error;
use crate::{ProfileId, Result, UserLocalTimeContext};

/// Trusted, user-scoped time-zone source. Resolve each requested instant anew:
/// an offset cached at runtime startup is wrong across DST or device-zone changes.
/// Remote consumers must supply the user's zone, not their server's zone.
pub trait UserLocalTimeProvider: Send + Sync {
    fn local_time_at(&self, unix_secs: u64) -> Result<Option<UserLocalTimeContext>>;
}

/// No user-device time-zone information is available. This never means UTC.
pub struct UnavailableUserLocalTimeProvider;

impl UserLocalTimeProvider for UnavailableUserLocalTimeProvider {
    fn local_time_at(&self, _unix_secs: u64) -> Result<Option<UserLocalTimeContext>> {
        Ok(None)
    }
}

/// Uses the device operating system's time zone for this instant. Intended
/// for a runtime on the user's device, never as a remote user's inferred zone.
/// Both zone discovery and rules lookup are fallible; unavailable means no field.
/// This does not guess an IANA region from a numeric offset or honor process TZ.
pub struct SystemUserLocalTimeProvider;

impl UserLocalTimeProvider for SystemUserLocalTimeProvider {
    fn local_time_at(&self, unix_secs: u64) -> Result<Option<UserLocalTimeContext>> {
        #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
        {
            let seconds = i64::try_from(unix_secs)
                .map_err(|_| Error::config("user_local_time", "invalid presentation instant"))?;
            let instant = jiff::Timestamp::from_second(seconds)
                .map_err(|_| Error::config("user_local_time", "invalid presentation instant"))?;
            // Read the OS's device zone every time. Do not use Local/system
            // helpers that silently return UTC, cache the selected zone, or
            // substitute the server/process TZ environment for the user's OS.
            let Ok(name) = iana_time_zone::get_timezone() else {
                return Ok(None);
            };
            let Ok(zone) = jiff::tz::TimeZone::get(&name) else {
                return Ok(None);
            };
            if zone.is_unknown() {
                return Ok(None);
            }
            UserLocalTimeContext::new(unix_secs, zone.to_offset(instant).seconds(), name).map(Some)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            let _ = unix_secs;
            Ok(None)
        }
    }
}

pub(crate) fn default_provider(profile: ProfileId) -> Arc<dyn UserLocalTimeProvider> {
    match profile {
        ProfileId::DesktopMacosStandaloneMemory
        | ProfileId::DesktopMacosEmbeddedSdk
        | ProfileId::DesktopMacosDevFull
        | ProfileId::DesktopLinuxEmbeddedSdk
        | ProfileId::DesktopWindowsEmbeddedSdk
        | ProfileId::DesktopWindowsDevFull => Arc::new(SystemUserLocalTimeProvider),
        ProfileId::EspStandaloneMemory
        | ProfileId::EspEmbeddedSdk
        | ProfileId::LinuxDeviceStandaloneMemory
        | ProfileId::ServerLinuxMemoryGateway
        | ProfileId::ServerLinuxDevFull => Arc::new(UnavailableUserLocalTimeProvider),
    }
}

pub(crate) fn resolve(
    provider: &dyn UserLocalTimeProvider,
    unix_secs: u64,
) -> Result<Option<UserLocalTimeContext>> {
    let context = provider.local_time_at(unix_secs)?;
    if let Some(context) = &context {
        context.validate_for_epoch(unix_secs)?;
    }
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct WrongInstant;
    impl UserLocalTimeProvider for WrongInstant {
        fn local_time_at(&self, unix_secs: u64) -> Result<Option<UserLocalTimeContext>> {
            UserLocalTimeContext::new(unix_secs + 1, 28_800, "Asia/Shanghai").map(Some)
        }
    }

    #[test]
    fn remote_and_device_profiles_never_inherit_development_machine_zone() {
        for profile in [
            ProfileId::ServerLinuxMemoryGateway,
            ProfileId::ServerLinuxDevFull,
            ProfileId::EspEmbeddedSdk,
            ProfileId::EspStandaloneMemory,
            ProfileId::LinuxDeviceStandaloneMemory,
        ] {
            assert_eq!(
                resolve(default_provider(profile).as_ref(), 1_700_000_000).unwrap(),
                None
            );
        }
    }

    #[test]
    fn stale_provider_instant_is_rejected() {
        assert!(resolve(&WrongInstant, 1_700_000_000).is_err());
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    #[test]
    fn desktop_default_uses_actual_environment_offset_for_each_instant() {
        let detected = iana_time_zone::get_timezone().ok().and_then(|name| {
            jiff::tz::TimeZone::get(&name)
                .ok()
                .filter(|zone| !zone.is_unknown())
                .map(|zone| (name, zone))
        });
        for epoch in [1_700_000_000, 1_720_000_000] {
            let local = resolve(
                default_provider(ProfileId::DesktopMacosEmbeddedSdk).as_ref(),
                epoch,
            )
            .unwrap();
            match &detected {
                Some((name, zone)) => {
                    let local = local.expect("a discovered device zone must be presented");
                    let expected = zone
                        .to_offset(jiff::Timestamp::from_second(epoch as i64).unwrap())
                        .seconds();
                    assert_eq!(local.utc_offset_seconds(), expected);
                    assert_eq!(local.time_zone(), name);
                    assert_eq!(local.unix_secs(), epoch);
                }
                None => assert!(
                    local.is_none(),
                    "unavailable device zone must not become UTC"
                ),
            }
        }
    }
}
