use bm_core::feature_gate::{profile_capability_catalog, ProfileId};

#[test]
fn esp_profiles_keep_sqlite_index_out_but_retain_fallback_recall() {
    let catalog = profile_capability_catalog();

    let standalone = catalog
        .iter()
        .find(|entry| entry.profile == ProfileId::EspStandaloneMemory)
        .expect("esp standalone profile catalog entry");
    assert!(!standalone.sqlite_index_allowed);
    assert!(standalone.lexical_archive_recall);
    assert!(standalone.heuristic_runtime_skill_recall);
    assert!(standalone.heuristic_task_learning_recall);
    assert!(!standalone.indexed_archive_recall_allowed);
    assert!(!standalone.indexed_continuity_capsule_recall_allowed);
    assert!(!standalone.indexed_runtime_skill_recall_allowed);
    assert!(!standalone.indexed_task_learning_recall_allowed);
    assert!(!standalone.communication_adapter_allowed);

    let embedded = catalog
        .iter()
        .find(|entry| entry.profile == ProfileId::EspEmbeddedSdk)
        .expect("esp embedded sdk profile catalog entry");
    assert!(!embedded.sqlite_index_allowed);
    assert!(embedded.lexical_archive_recall);
    assert!(embedded.heuristic_runtime_skill_recall);
    assert!(embedded.heuristic_task_learning_recall);
    assert!(!embedded.indexed_archive_recall_allowed);
    assert!(!embedded.indexed_continuity_capsule_recall_allowed);
    assert!(!embedded.indexed_runtime_skill_recall_allowed);
    assert!(!embedded.indexed_task_learning_recall_allowed);
    assert!(!embedded.communication_adapter_allowed);
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
    assert!(server_gateway.heuristic_runtime_skill_recall);
    assert!(server_gateway.heuristic_task_learning_recall);
    assert!(server_gateway.indexed_archive_recall_allowed);
    assert!(server_gateway.indexed_continuity_capsule_recall_allowed);
    assert!(server_gateway.indexed_runtime_skill_recall_allowed);
    assert!(server_gateway.indexed_task_learning_recall_allowed);
    assert!(server_gateway.communication_adapter_allowed);
}
