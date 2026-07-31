use bm_sdk::{
    resolve_memory_capabilities, MemoryCapabilityPolicy, MemoryPrivacyPolicy, ProfileId,
    RuntimeSkillCreationRef, RuntimeSkillOwningScope,
};
use serde::{Deserialize, Serialize};

use crate::{
    EvolutionCandidate, EvolutionCandidateDecision, EvolutionProposal, EvolutionProposalValidation,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionSandboxTier {
    Preview,
    Compact,
    Full,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionSandboxPolicy {
    pub profile: ProfileId,
    pub proposal_preview_allowed: bool,
    pub compact_sandbox_allowed: bool,
    pub full_sandbox_allowed: bool,
    pub proposal_submission_allowed: bool,
}

impl EvolutionSandboxPolicy {
    pub fn for_profile(profile: ProfileId) -> bm_core::Result<Self> {
        let capabilities = resolve_memory_capabilities(
            profile,
            &MemoryCapabilityPolicy::strict_profile(),
            &MemoryPrivacyPolicy::standard_private_boundary(),
        )?;
        Ok(Self {
            profile,
            proposal_preview_allowed: capabilities.validation.proposal_preview.visible,
            compact_sandbox_allowed: capabilities.validation.compact_proposal_sandbox.visible,
            full_sandbox_allowed: capabilities.validation.full_proposal_sandbox.visible,
            proposal_submission_allowed: capabilities.validation.proposal_submission.visible,
        })
    }

    pub fn allows_tier(&self, tier: EvolutionSandboxTier) -> bool {
        match tier {
            EvolutionSandboxTier::Preview => self.proposal_preview_allowed,
            EvolutionSandboxTier::Compact => self.compact_sandbox_allowed,
            EvolutionSandboxTier::Full => self.full_sandbox_allowed,
        }
    }
}

pub fn validate_evolution_proposal(
    policy: &EvolutionSandboxPolicy,
    proposal: &EvolutionProposal,
) -> EvolutionProposalValidation {
    let mut decisions = Vec::new();
    let mut global_rejection = None;
    if !policy.proposal_preview_allowed {
        global_rejection = Some("proposal_preview_not_allowed_for_profile");
    } else if policy.profile != proposal.profile {
        global_rejection = Some("proposal_profile_mismatch");
    } else if proposal.proposal_id.trim().is_empty() {
        global_rejection = Some("proposal_id_empty");
    } else if matches!(
        &proposal.owning_scope,
        RuntimeSkillOwningScope::Subject { mounted_subject_id }
            if mounted_subject_id.trim().is_empty()
    ) {
        global_rejection = Some("proposal_runtime_skill_scope_invalid");
    } else if !(RuntimeSkillCreationRef::ReplayPromotion {
        candidate_ref: proposal.proposal_id.clone(),
        verification_receipt_digest: proposal.verification_receipt_digest.clone(),
    })
    .validate_contract()
    {
        global_rejection = Some("proposal_verification_receipt_invalid");
    } else if proposal.evidence_refs.is_empty() {
        global_rejection = Some("proposal_evidence_empty");
    } else if proposal.rationale.trim().is_empty() {
        global_rejection = Some("proposal_rationale_empty");
    }

    for (index, candidate) in proposal.candidates.iter().enumerate() {
        let reason = if let Some(reason) = global_rejection {
            reason.to_string()
        } else {
            candidate_rejection_reason(policy, candidate)
        };
        decisions.push(EvolutionCandidateDecision {
            index,
            accepted: reason == "accepted",
            reason,
        });
    }

    if proposal.candidates.is_empty() {
        decisions.push(EvolutionCandidateDecision {
            index: 0,
            accepted: false,
            reason: "proposal_candidates_empty".to_string(),
        });
    }

    let accepted_candidates = decisions
        .iter()
        .filter(|decision| decision.accepted)
        .count();
    let rejected_candidates = decisions.len().saturating_sub(accepted_candidates);
    EvolutionProposalValidation {
        proposal_id: proposal.proposal_id.clone(),
        profile: proposal.profile,
        accepted: accepted_candidates > 0 && rejected_candidates == 0,
        accepted_candidates,
        rejected_candidates,
        decisions,
    }
}

fn candidate_rejection_reason(
    policy: &EvolutionSandboxPolicy,
    candidate: &EvolutionCandidate,
) -> String {
    match candidate {
        EvolutionCandidate::ProceduralMemory { write } => {
            if !policy.compact_sandbox_allowed && !policy.full_sandbox_allowed {
                return "procedural_candidate_sandbox_not_allowed".to_string();
            }
            if write.topic.trim().is_empty()
                || write.summary.trim().is_empty()
                || write.content.trim().is_empty()
            {
                return "procedural_candidate_incomplete".to_string();
            }
            "accepted".to_string()
        }
        EvolutionCandidate::GovernanceNote { target, summary } => {
            if target.trim().is_empty() || summary.trim().is_empty() {
                return "governance_note_incomplete".to_string();
            }
            "accepted".to_string()
        }
    }
}
