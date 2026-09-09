//! Evolution sandbox-facing contracts for Beetle Memory.
//!
//! The executable sandbox is host-provided. This crate exposes proposal-only
//! contracts and never grants direct Store mutation authority.
//!
//! ```compile_fail
//! use bm_evolve::commit_evolution_proposal;
//! ```

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
pub use policy::{validate_evolution_proposal, EvolutionSandboxPolicy, EvolutionSandboxTier};
pub use proposal::{
    EvolutionCandidate, EvolutionCandidateDecision, EvolutionProposal, EvolutionProposalValidation,
};
