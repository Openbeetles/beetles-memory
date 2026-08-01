use bm_core::feature_gate::{profile_capability_catalog, ProfileId};

#[test]
fn historical_as_of_profile_participation_is_exactly_eight_allowed_and_three_blocked() {
    let catalog = profile_capability_catalog();
    let expectations = [
        (ProfileId::EspStandaloneMemory, false),
        (ProfileId::EspEmbeddedSdk, false),
        (ProfileId::LinuxDeviceStandaloneMemory, false),
        (ProfileId::DesktopMacosStandaloneMemory, true),
        (ProfileId::DesktopMacosEmbeddedSdk, true),
        (ProfileId::DesktopLinuxEmbeddedSdk, true),
        (ProfileId::DesktopMacosDevFull, true),
        (ProfileId::DesktopWindowsEmbeddedSdk, true),
        (ProfileId::DesktopWindowsDevFull, true),
        (ProfileId::ServerLinuxMemoryGateway, true),
        (ProfileId::ServerLinuxDevFull, true),
    ];

    assert_eq!(catalog.len(), expectations.len());
    for (profile, historical_allowed) in expectations {
        let matching = catalog
            .iter()
            .filter(|entry| entry.profile == profile)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "{profile:?} must have exactly one entry");
        assert_eq!(
            matching[0].historical_as_of_recall_allowed, historical_allowed,
            "{profile:?} historical as-of participation drifted"
        );
    }
    assert_eq!(
        catalog
            .iter()
            .filter(|entry| entry.historical_as_of_recall_allowed)
            .count(),
        8
    );
    assert_eq!(
        catalog
            .iter()
            .filter(|entry| !entry.historical_as_of_recall_allowed)
            .count(),
        3
    );
}

#[test]
fn procedural_and_premise_participation_cover_all_profiles() {
    let catalog = profile_capability_catalog();
    assert_eq!(catalog.len(), 11);
    assert!(catalog.iter().all(|entry| entry.procedural_recall_allowed));
    assert_eq!(
        catalog
            .iter()
            .filter(|entry| entry.environment_premise_evaluation_allowed)
            .count(),
        11
    );
    assert!(catalog
        .iter()
        .all(|entry| entry.environment_premise_evaluation_allowed));
}

#[test]
fn esp_profiles_keep_sqlite_index_out_and_declare_compact_typed_recall_participation() {
    let catalog = profile_capability_catalog();

    let standalone = catalog
        .iter()
        .find(|entry| entry.profile == ProfileId::EspStandaloneMemory)
        .expect("esp standalone profile catalog entry");
    assert!(!standalone.sqlite_index_allowed);
    assert!(standalone.lexical_archive_recall);
    assert!(standalone.compact_runtime_skill_recall_allowed);
    assert!(standalone.dynamic_state_recall_allowed);
    assert!(!standalone.historical_as_of_recall_allowed);
    assert!(standalone.procedural_recall_allowed);
    assert!(standalone.environment_premise_evaluation_allowed);
    assert!(!standalone.update_lineage_inspection_allowed);
    assert!(standalone.heuristic_task_learning_recall);
    assert!(!standalone.indexed_archive_recall_allowed);
    assert!(!standalone.indexed_continuity_capsule_recall_allowed);
    assert!(!standalone.indexed_runtime_skill_recall_allowed);
    assert!(!standalone.indexed_task_learning_recall_allowed);
    assert!(standalone.communication_adapter_allowed);
    assert!(!standalone.llm_gateway_server_allowed);
    assert!(standalone.adapter.cli.allowed);
    assert!(standalone.adapter.wss.client_allowed);
    assert!(!standalone.adapter.wss.server_allowed);
    assert!(!standalone.adapter.http.allowed);

    let embedded = catalog
        .iter()
        .find(|entry| entry.profile == ProfileId::EspEmbeddedSdk)
        .expect("esp embedded sdk profile catalog entry");
    assert!(!embedded.sqlite_index_allowed);
    assert!(embedded.lexical_archive_recall);
    assert!(embedded.compact_runtime_skill_recall_allowed);
    assert!(embedded.dynamic_state_recall_allowed);
    assert!(!embedded.historical_as_of_recall_allowed);
    assert!(embedded.procedural_recall_allowed);
    assert!(embedded.environment_premise_evaluation_allowed);
    assert!(!embedded.update_lineage_inspection_allowed);
    assert!(embedded.heuristic_task_learning_recall);
    assert!(!embedded.indexed_archive_recall_allowed);
    assert!(!embedded.indexed_continuity_capsule_recall_allowed);
    assert!(!embedded.indexed_runtime_skill_recall_allowed);
    assert!(!embedded.indexed_task_learning_recall_allowed);
    assert!(!embedded.communication_adapter_allowed);
    assert!(!embedded.llm_gateway_server_allowed);
    assert!(!embedded.adapter.cli.allowed);
    assert!(!embedded.adapter.wss.allowed);
}

#[test]
fn server_gateway_profile_can_opt_into_sqlite_index_without_making_it_os_default() {
    let catalog = profile_capability_catalog();
    let server_gateway = catalog
        .iter()
        .find(|entry| entry.profile == ProfileId::ServerLinuxMemoryGateway)
        .expect("server linux memory gateway profile catalog entry");

    assert!(server_gateway.sqlite_index_allowed);
    assert!(server_gateway.lexical_archive_recall);
    assert!(!server_gateway.compact_runtime_skill_recall_allowed);
    assert!(server_gateway.dynamic_state_recall_allowed);
    assert!(server_gateway.historical_as_of_recall_allowed);
    assert!(server_gateway.procedural_recall_allowed);
    assert!(server_gateway.environment_premise_evaluation_allowed);
    assert!(server_gateway.update_lineage_inspection_allowed);
    assert!(server_gateway.heuristic_task_learning_recall);
    assert!(server_gateway.indexed_archive_recall_allowed);
    assert!(server_gateway.indexed_continuity_capsule_recall_allowed);
    assert!(server_gateway.indexed_runtime_skill_recall_allowed);
    assert!(server_gateway.indexed_task_learning_recall_allowed);
    assert!(server_gateway.communication_adapter_allowed);
    assert!(server_gateway.llm_gateway_server_allowed);
    assert!(server_gateway.adapter.http.server_allowed);
    assert!(server_gateway.adapter.wss.client_allowed);
    assert!(server_gateway.adapter.wss.server_allowed);
    assert!(server_gateway.adapter.mcp.server_allowed);
    assert!(server_gateway.adapter.a2a.client_allowed);
    assert!(server_gateway.adapter.a2a.server_allowed);
}
