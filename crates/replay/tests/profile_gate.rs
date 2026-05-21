use bm_sdk::{resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId};

#[test]
fn replay_validation_capabilities_distinguish_esp_standalone_and_embedded_sdk() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();

    let standalone = resolve_memory_capabilities(ProfileId::EspStandaloneMemory, &policy, &privacy)
        .expect("standalone catalog");
    let embedded = resolve_memory_capabilities(ProfileId::EspEmbeddedSdk, &policy, &privacy)
        .expect("embedded catalog");

    assert!(standalone.validation.compact_proposal_sandbox.visible);
    assert!(standalone.validation.proposal_submission.visible);
    assert!(!standalone.validation.full_proposal_sandbox.visible);
    assert!(!standalone.validation.full_replay_suite.visible);

    assert!(embedded.validation.proposal_preview.visible);
    assert!(!embedded.validation.compact_replay_fixture.visible);
    assert!(!embedded.validation.memory_harness.visible);
    assert!(!embedded.validation.proposal_submission.visible);
}

#[test]
fn server_dev_full_surfaces_full_validation_when_replay_harness_is_compiled() {
    let policy = MemoryCapabilityPolicy::strict_profile();
    let privacy = MemoryPrivacyPolicy::standard_private_boundary();
    let catalog = resolve_memory_capabilities(ProfileId::ServerLinuxDevFull, &policy, &privacy)
        .expect("dev full catalog");

    assert!(catalog.validation.full_proposal_sandbox.visible);
    assert!(catalog.validation.proposal_submission.visible);
    assert!(catalog.validation.full_replay_suite.profile_allowed);
    assert!(catalog.validation.memory_harness.profile_allowed);
    assert!(catalog.validation.full_replay_suite.visible);
    assert!(catalog.validation.memory_harness.visible);
}
