use bm_sdk::{MemoryPrivacyClass, ProfileId, RuntimeSkillOwningScope, RuntimeSkillWrite};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionProposal {
    pub proposal_id: String,
    pub profile: ProfileId,
    pub owning_scope: RuntimeSkillOwningScope,
    pub verification_receipt_digest: String,
    pub privacy_class: MemoryPrivacyClass,
    pub candidates: Vec<EvolutionCandidate>,
    pub evidence_refs: Vec<String>,
    pub rationale: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvolutionCandidate {
    ProceduralMemory { write: RuntimeSkillWrite },
    GovernanceNote { target: String, summary: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionProposalValidation {
    pub proposal_id: String,
    pub profile: ProfileId,
    pub accepted: bool,
    pub accepted_candidates: usize,
    pub rejected_candidates: usize,
    pub decisions: Vec<EvolutionCandidateDecision>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionCandidateDecision {
    pub index: usize,
    pub accepted: bool,
    pub reason: String,
}
