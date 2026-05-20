//! Evolution sandbox-facing contracts for Beetle Memory.
//!
//! The executable sandbox is host-provided. This crate exposes the memory
//! reports and proposal types that a sandbox can consume without writing stores
//! directly.

pub use bm_core::memory::{
    inspect_memory_hygiene, inspect_personality_governance, MemoryGovernanceContext,
    MemoryGovernanceInput, MemoryGovernanceOutcome, MemoryHygieneInspection,
    PersonalityGovernanceInspection,
};
pub use bm_core::skills::{
    CapabilityAtomImportOutcome, CapabilityAtomSyncOutcome, RuntimeSkillGovernanceOutcome,
};
