use bm_evolve::{EvolutionSandboxPolicy, EvolutionSandboxTier};
use bm_sdk::ProfileId;

#[test]
fn sandbox_policy_matches_standalone_vs_embedded_profiles() {
    let esp_standalone = EvolutionSandboxPolicy::for_profile(ProfileId::EspStandaloneMemory)
        .expect("esp standalone policy");
    assert!(esp_standalone.allows_tier(EvolutionSandboxTier::Preview));
    assert!(esp_standalone.allows_tier(EvolutionSandboxTier::Compact));
    assert!(!esp_standalone.allows_tier(EvolutionSandboxTier::Full));
    assert!(esp_standalone.proposal_submission_allowed);

    let esp_embedded =
        EvolutionSandboxPolicy::for_profile(ProfileId::EspEmbeddedSdk).expect("esp sdk policy");
    assert!(esp_embedded.allows_tier(EvolutionSandboxTier::Preview));
    assert!(!esp_embedded.allows_tier(EvolutionSandboxTier::Compact));
    assert!(!esp_embedded.proposal_submission_allowed);

    let server =
        EvolutionSandboxPolicy::for_profile(ProfileId::ServerLinuxDevFull).expect("server policy");
    assert!(server.allows_tier(EvolutionSandboxTier::Full));
    assert!(server.proposal_submission_allowed);
}
