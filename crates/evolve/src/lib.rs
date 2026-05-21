//! Evolution sandbox-facing contracts for Beetle Memory.
//!
//! The executable sandbox is host-provided. This crate exposes proposal-only
//! contracts and SDK commit helpers without writing stores directly.

mod commit;
mod policy;
mod proposal;

pub use bm_core::memory::{
    inspect_memory_hygiene, inspect_personality_governance, MemoryGovernanceContext,
    MemoryGovernanceInput, MemoryGovernanceOutcome, MemoryHygieneInspection,
    PersonalityGovernanceInspection,
};
pub use bm_core::skills::{
    CapabilityAtomImportOutcome, CapabilityAtomSyncOutcome, RuntimeSkillGovernanceOutcome,
};
pub use commit::{commit_evolution_proposal, EvolutionProposalReport};
pub use policy::{validate_evolution_proposal, EvolutionSandboxPolicy, EvolutionSandboxTier};
pub use proposal::{
    EvolutionCandidate, EvolutionCandidateDecision, EvolutionProposal, EvolutionProposalValidation,
};
