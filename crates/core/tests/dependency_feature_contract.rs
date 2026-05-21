use bm_core::feature_gate::{compiled_feature_report, profile_capability_catalog, ProfileId};

#[test]
fn sqlite_index_dependency_is_controlled_by_explicit_feature() {
    let report = compiled_feature_report();

    assert_eq!(report.sqlite_index_compiled, cfg!(feature = "sqlite-index"));
    assert_eq!(
        report.rusqlite_dependency_compiled,
        cfg!(feature = "sqlite-index")
    );
}

#[test]
fn compiled_feature_report_names_target_role_and_profile_features() {
    let report = compiled_feature_report();

    assert_eq!(report.target_esp, cfg!(feature = "target-esp"));
    assert_eq!(
        report.target_linux_device,
        cfg!(feature = "target-linux-device")
    );
    assert_eq!(
        report.target_desktop_macos,
        cfg!(feature = "target-desktop-macos")
    );
    assert_eq!(
        report.target_desktop_windows,
        cfg!(feature = "target-desktop-windows")
    );
    assert_eq!(
        report.target_server_linux,
        cfg!(feature = "target-server-linux")
    );
    assert_eq!(
        report.role_standalone_memory,
        cfg!(feature = "role-standalone-memory")
    );
    assert_eq!(
        report.role_embedded_sdk,
        cfg!(feature = "role-embedded-sdk")
    );
    assert_eq!(
        report.role_memory_gateway,
        cfg!(feature = "role-memory-gateway")
    );
    assert_eq!(report.role_dev_full, cfg!(feature = "role-dev-full"));
    assert_eq!(
        report.profile_esp_standalone_memory,
        cfg!(feature = "profile-esp-standalone-memory")
    );
    assert_eq!(
        report.profile_esp_embedded_sdk,
        cfg!(feature = "profile-esp-embedded-sdk")
    );
    assert_eq!(
        report.profile_linux_device_standalone_memory,
        cfg!(feature = "profile-linux-device-standalone-memory")
    );
    assert_eq!(
        report.profile_desktop_macos_embedded_sdk,
        cfg!(feature = "profile-desktop-macos-embedded-sdk")
    );
    assert_eq!(
        report.profile_desktop_windows_embedded_sdk,
        cfg!(feature = "profile-desktop-windows-embedded-sdk")
    );
    assert_eq!(
        report.profile_server_linux_memory_gateway,
        cfg!(feature = "profile-server-linux-memory-gateway")
    );
    assert_eq!(
        report.profile_server_linux_dev_full,
        cfg!(feature = "profile-server-linux-dev-full")
    );
    assert_eq!(
        report.replay_harness_compiled,
        cfg!(feature = "replay-harness")
    );
}

#[test]
fn profile_catalog_declares_esp_sqlite_absence_independent_of_host_os() {
    let catalog = profile_capability_catalog();
    for profile in [ProfileId::EspStandaloneMemory, ProfileId::EspEmbeddedSdk] {
        let entry = catalog
            .iter()
            .find(|entry| entry.profile == profile)
            .expect("esp profile catalog entry");
        assert!(!entry.sqlite_index_allowed);
        assert!(!entry.indexed_archive_recall_allowed);
        assert!(!entry.indexed_continuity_capsule_recall_allowed);
        assert!(!entry.indexed_runtime_skill_recall_allowed);
        assert!(!entry.indexed_task_learning_recall_allowed);
    }
}
