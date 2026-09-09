use bm_evolve::{
    validate_evolution_proposal, EvolutionCandidate, EvolutionProposal, EvolutionSandboxPolicy,
};
use bm_sdk::{
    default_agent_subject_id, MemoryPrivacyClass, ProfileId, RuntimeSkillOwningScope,
    RuntimeSkillWrite,
};

#[test]
fn full_profile_accepts_well_formed_procedural_proposal_without_committing_it() {
    let profile = ProfileId::native_dev_full().expect("supported host-native dev-full profile");
    let proposal = procedural_proposal(profile);
    let policy = EvolutionSandboxPolicy::for_profile(profile).expect("full profile policy");

    let validation = validate_evolution_proposal(&policy, &proposal);

    assert!(validation.accepted, "{validation:?}");
    assert_eq!(validation.accepted_candidates, 1);
    assert_eq!(validation.rejected_candidates, 0);
}

#[test]
fn governance_note_is_a_valid_proposal_but_not_a_store_mutation_contract() {
    let profile = ProfileId::native_dev_full().expect("supported host-native dev-full profile");
    let proposal = EvolutionProposal {
        proposal_id: "governance-note".to_string(),
        profile,
        owning_scope: RuntimeSkillOwningScope::Subject {
            mounted_subject_id: default_agent_subject_id("evolve-agent"),
        },
        verification_receipt_digest:
            "sha256:4444444444444444444444444444444444444444444444444444444444444444".to_string(),
        privacy_class: MemoryPrivacyClass::SharedWithSubject,
        candidates: vec![EvolutionCandidate::GovernanceNote {
            target: "self-authored-core".to_string(),
            summary: "Candidate evidence only, not direct write authority.".to_string(),
        }],
        evidence_refs: vec!["counterfactual-trace".to_string()],
        rationale: "Sandbox submits a proposal for an authoritative governor.".to_string(),
    };
    let policy = EvolutionSandboxPolicy::for_profile(profile).expect("full profile policy");

    let validation = validate_evolution_proposal(&policy, &proposal);

    assert!(validation.accepted, "{validation:?}");
    assert_eq!(validation.decisions[0].reason, "accepted");
}

#[test]
fn embedded_sdk_profile_can_preview_but_rejects_procedural_sandbox_candidates() {
    let profile = ProfileId::EspEmbeddedSdk;
    let proposal = procedural_proposal(profile);
    let policy = EvolutionSandboxPolicy::for_profile(profile).expect("embedded policy");
    let validation = validate_evolution_proposal(&policy, &proposal);

    assert!(policy.proposal_preview_allowed);
    assert!(!policy.compact_sandbox_allowed);
    assert!(!policy.proposal_submission_allowed);
    assert!(!validation.accepted);
    assert_eq!(
        validation.decisions[0].reason,
        "procedural_candidate_sandbox_not_allowed"
    );
}

fn procedural_proposal(profile: ProfileId) -> EvolutionProposal {
    EvolutionProposal {
        proposal_id: "release-skill-proposal".to_string(),
        profile,
        owning_scope: RuntimeSkillOwningScope::Subject {
            mounted_subject_id: default_agent_subject_id("evolve-agent"),
        },
        verification_receipt_digest:
            "sha256:5555555555555555555555555555555555555555555555555555555555555555".to_string(),
        privacy_class: MemoryPrivacyClass::SharedWithSubject,
        candidates: vec![EvolutionCandidate::ProceduralMemory {
            write: RuntimeSkillWrite {
                name: "release_guard".to_string(),
                topic: "release".to_string(),
                title: "Release artifact guard".to_string(),
                summary: "Verify release artifacts before publishing.".to_string(),
                content: "1. inspect artifacts\n2. verify manifest\n3. publish".to_string(),
                citations: vec!["evolution proposal".to_string()],
                source_chat_id: Some("evolve-chat".to_string()),
                observed_at: 1_800_000_000,
            },
        }],
        evidence_refs: vec!["proposal-fixture".to_string()],
        rationale: "Promote repeated release checks into a governed proposal.".to_string(),
    }
}
