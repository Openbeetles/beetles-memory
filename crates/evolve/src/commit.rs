use bm_sdk::{
    MemoryRuntime, MemoryWriteRequest, ProceduralMemoryPromotionInput, RuntimeSkillWriteSource,
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

    let promotions = proposal
        .candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| match candidate {
            EvolutionCandidate::ProceduralMemory { write } => {
                let evidence_refs = write
                    .citations
                    .iter()
                    .cloned()
                    .chain(proposal.evidence_refs.iter().cloned())
                    .collect::<Vec<_>>();
                Some(ProceduralMemoryPromotionInput {
                    task_id: format!("{}:{index}", proposal.proposal_id),
                    trigger: if !write.name.trim().is_empty() {
                        write.name.clone()
                    } else if write.topic.trim().is_empty() {
                        write.title.clone()
                    } else {
                        write.topic.clone()
                    },
                    procedure: write.content.clone(),
                    constraints: vec!["evolution_sandbox_submission_policy".to_string()],
                    failure_modes: vec![proposal.rationale.clone()],
                    counterfactual_fix: write.summary.clone(),
                    repeated_evidence_count: evidence_refs.len(),
                    evidence_refs,
                    quality_score: 80,
                    capability_affinity: vec![write.topic.clone()],
                })
            }
            EvolutionCandidate::GovernanceNote { .. } => None,
        })
        .collect::<Vec<_>>();
    if promotions.len() != proposal.candidates.len() {
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
    let write_report = runtime.write(MemoryWriteRequest::ProceduralPromotions {
        promotions,
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
