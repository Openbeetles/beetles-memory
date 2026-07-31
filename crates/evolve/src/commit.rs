use bm_sdk::{
    GovernedRuntimeSkillWriteInput, MemoryRuntime, MemoryWriteRequest, RuntimeSkillCreationRef,
    RuntimeSkillWriteSource,
};
use serde::{Deserialize, Serialize};

use crate::{
    validate_evolution_proposal, EvolutionCandidate, EvolutionProposal,
    EvolutionProposalValidation, EvolutionSandboxPolicy,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionProposalReport {
    pub proposal_id: String,
    pub profile: bm_sdk::ProfileId,
    pub validation: EvolutionProposalValidation,
    pub submitted_candidates: usize,
    pub committed_writes: usize,
    pub write_operation: Option<String>,
    pub lifecycle_operations: Vec<String>,
    pub accepted: bool,
    pub reason: String,
}

pub fn commit_evolution_proposal(
    runtime: &MemoryRuntime,
    proposal: EvolutionProposal,
) -> bm_core::Result<EvolutionProposalReport> {
    let policy = EvolutionSandboxPolicy::for_profile(runtime.config().profile)?;
    let validation = validate_evolution_proposal(&policy, &proposal);
    if !policy.proposal_submission_allowed {
        return Ok(EvolutionProposalReport {
            proposal_id: proposal.proposal_id,
            profile: proposal.profile,
            validation,
            submitted_candidates: 0,
            committed_writes: 0,
            write_operation: None,
            lifecycle_operations: Vec::new(),
            accepted: false,
            reason: "proposal_submission_not_allowed_for_profile".to_string(),
        });
    }
    if !validation.accepted {
        return Ok(EvolutionProposalReport {
            proposal_id: proposal.proposal_id,
            profile: proposal.profile,
            validation,
            submitted_candidates: 0,
            committed_writes: 0,
            write_operation: None,
            lifecycle_operations: Vec::new(),
            accepted: false,
            reason: "proposal_validation_failed".to_string(),
        });
    }

    let writes = proposal
        .candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| match candidate {
            EvolutionCandidate::ProceduralMemory { write } => {
                Some(GovernedRuntimeSkillWriteInput {
                    write: write.clone(),
                    creation_ref: RuntimeSkillCreationRef::ReplayPromotion {
                        candidate_ref: format!("{}:{index}", proposal.proposal_id),
                        verification_receipt_digest: proposal.verification_receipt_digest.clone(),
                    },
                    privacy_class: proposal.privacy_class,
                })
            }
            EvolutionCandidate::GovernanceNote { .. } => None,
        })
        .collect::<Vec<_>>();
    if writes.len() != proposal.candidates.len() {
        return Ok(EvolutionProposalReport {
            proposal_id: proposal.proposal_id,
            profile: proposal.profile,
            validation,
            submitted_candidates: proposal.candidates.len(),
            committed_writes: 0,
            write_operation: None,
            lifecycle_operations: Vec::new(),
            accepted: false,
            reason: "governance_note_requires_future_sdk_operation".to_string(),
        });
    }
    let write_report = runtime.write(MemoryWriteRequest::Procedural {
        writes,
        owning_scope: proposal.owning_scope,
        source: RuntimeSkillWriteSource::ProgrammableReasoning,
    })?;
    Ok(EvolutionProposalReport {
        proposal_id: proposal.proposal_id,
        profile: proposal.profile,
        validation,
        submitted_candidates: proposal.candidates.len(),
        committed_writes: write_report.changed,
        write_operation: Some(write_report.operation.to_string()),
        lifecycle_operations: vec![write_report.lifecycle_report.operation.as_str().to_string()],
        accepted: write_report.accepted,
        reason: write_report.reason,
    })
}
