use bm_sdk::{resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId};

#[test]
fn esp_standalone_and_embedded_sdk_have_distinct_visible_catalogs() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let standalone = resolve_memory_capabilities(ProfileId::EspStandaloneMemory, &policy, &privacy)
        .expect("standalone catalog");
    let embedded = resolve_memory_capabilities(ProfileId::EspEmbeddedSdk, &policy, &privacy)
        .expect("embedded catalog");

    assert_ne!(standalone.profile, embedded.profile);
    assert!(standalone.write.visible);
    assert!(standalone.recall.visible);
    assert!(standalone.projection.visible);
    assert!(standalone.lifecycle.recover.visible);
    assert!(!standalone.lifecycle.maintain_full.visible);
    assert!(standalone.lifecycle.maintain_lightweight.visible);
    assert!(standalone.lifecycle.operator_diagnosis.visible);
    assert!(!standalone.sqlite_index_recall.archive.visible);
    assert!(!standalone.communication_adapter.visible);
    assert_eq!(
        standalone.validation.compact_replay_fixture.visible,
        standalone.validation.compact_replay_fixture.compiled
    );
    assert_eq!(
        standalone.validation.memory_harness.visible,
        standalone.validation.memory_harness.compiled
    );
    assert!(!standalone.validation.full_replay_suite.visible);
    assert!(standalone.validation.compact_proposal_sandbox.visible);
    assert!(!standalone.validation.full_proposal_sandbox.visible);
    assert!(standalone.validation.proposal_submission.visible);

    assert!(embedded.write.visible);
    assert!(embedded.recall.visible);
    assert!(embedded.projection.visible);
    assert!(!embedded.maintenance.visible);
    assert!(!embedded.lifecycle.recover.visible);
    assert!(!embedded.lifecycle.maintain_full.visible);
    assert!(!embedded.lifecycle.maintain_lightweight.visible);
    assert!(embedded.lifecycle.operator_diagnosis.visible);
    assert!(!embedded.replay.visible);
    assert!(!embedded.sqlite_index_recall.archive.visible);
    assert!(!embedded.communication_adapter.visible);
    assert!(!embedded.validation.compact_replay_fixture.visible);
    assert!(embedded.validation.proposal_preview.visible);
    assert!(!embedded.validation.compact_proposal_sandbox.visible);
    assert!(!embedded.validation.proposal_submission.visible);
}

#[test]
fn server_gateway_can_surface_adapter_permission_without_creating_adapter_code() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.communication_adapter_enabled = true;
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let catalog =
        resolve_memory_capabilities(ProfileId::ServerLinuxMemoryGateway, &policy, &privacy)
            .expect("server gateway catalog");

    assert!(catalog.communication_adapter.profile_allowed);
    assert!(catalog.communication_adapter.config_enabled);
    assert!(catalog.communication_adapter.visible);
    assert_eq!(
        catalog.validation.full_replay_suite.visible,
        catalog.validation.full_replay_suite.compiled
    );
    assert!(catalog.validation.full_proposal_sandbox.visible);
    assert!(catalog.validation.proposal_submission.visible);
}

#[test]
fn privacy_gate_blocks_projection_and_export_visibility() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy {
        prompt_projection_allowed: false,
        private_plane_projection_allowed: false,
        operator_inspection_allowed: true,
        export_allowed: false,
        import_allowed: true,
    };

    let catalog = resolve_memory_capabilities(ProfileId::ServerLinuxDevFull, &policy, &privacy)
        .expect("dev full catalog");

    assert!(!catalog.projection.visible);
    assert!(!catalog.export.visible);
    assert!(!catalog.lifecycle.export_snapshot.visible);
    assert!(catalog.import.visible);
    assert!(catalog.lifecycle.import_snapshot.visible);
}

#[test]
fn policy_can_disable_replay_harness_and_evolution_sandbox_independently() {
    let mut policy = MemoryCapabilityPolicy::strict_profile();
    policy.replay_harness_enabled = false;
    policy.evolution_sandbox_enabled = false;
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let catalog = resolve_memory_capabilities(ProfileId::ServerLinuxDevFull, &policy, &privacy)
        .expect("dev full catalog");

    assert!(catalog.validation.full_replay_suite.profile_allowed);
    assert!(!catalog.validation.full_replay_suite.config_enabled);
    assert!(!catalog.validation.full_replay_suite.visible);
    assert!(catalog.validation.full_proposal_sandbox.profile_allowed);
    assert!(!catalog.validation.full_proposal_sandbox.config_enabled);
    assert!(!catalog.validation.full_proposal_sandbox.visible);
    assert!(!catalog.validation.proposal_submission.visible);
}
