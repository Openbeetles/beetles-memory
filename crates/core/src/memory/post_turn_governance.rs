use serde::{Deserialize, Serialize};

use super::SessionTurnCommitReport;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryWriteAuthority {
    RuntimeDeterministic,
    LlmPrivateGardenFreeform,
    LlmGovernedSemantic,
    SoulGovernedCore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivateGardenAdmissionDecision {
    Applied,
    Skipped,
    BoundaryRejected,
    BoundaryTruncated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostTurnPrivateGardenReport {
    pub attempted: bool,
    pub executed: bool,
    pub authority: MemoryWriteAuthority,
    pub admission: PrivateGardenAdmissionDecision,
    pub writes: usize,
    pub moves: usize,
    pub deletes: usize,
    pub skipped_reason: Option<String>,
}

impl PostTurnPrivateGardenReport {
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            attempted: false,
            executed: false,
            authority: MemoryWriteAuthority::LlmPrivateGardenFreeform,
            admission: PrivateGardenAdmissionDecision::Skipped,
            writes: 0,
            moves: 0,
            deletes: 0,
            skipped_reason: Some(reason.into()),
        }
    }

    pub fn no_change(reason: impl Into<String>) -> Self {
        Self {
            attempted: true,
            executed: false,
            authority: MemoryWriteAuthority::LlmPrivateGardenFreeform,
            admission: PrivateGardenAdmissionDecision::Skipped,
            writes: 0,
            moves: 0,
            deletes: 0,
            skipped_reason: Some(reason.into()),
        }
    }

    pub fn applied(writes: usize, moves: usize, deletes: usize) -> Self {
        Self {
            attempted: true,
            executed: true,
            authority: MemoryWriteAuthority::LlmPrivateGardenFreeform,
            admission: PrivateGardenAdmissionDecision::Applied,
            writes,
            moves,
            deletes,
            skipped_reason: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryWriteDomain {
    Program,
    Subject,
    Procedural,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedWriteDecision {
    Accepted,
    Rejected,
    Merged,
    Superseded,
    Deferred,
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryPlaneGovernanceReport {
    pub domain: MemoryWriteDomain,
    pub plane: String,
    pub authority: MemoryWriteAuthority,
    pub decision: GovernedWriteDecision,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub privacy_decision: String,
    pub profile_decision: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoulCandidateDisposition {
    HandedOff,
    RejectedAsMemoryOnly,
    Deferred,
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoulCandidateHandoffReport {
    pub surface: String,
    pub disposition: SoulCandidateDisposition,
    pub existing_gate: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostTurnSemanticGovernanceReport {
    pub attempted: bool,
    pub executed: bool,
    pub skipped_reason: Option<String>,
    pub proposal_count: usize,
    pub accepted_count: usize,
    pub rejected_count: usize,
    pub deferred_count: usize,
    pub plane_reports: Vec<MemoryPlaneGovernanceReport>,
    pub soul_candidate_handoffs: Vec<SoulCandidateHandoffReport>,
}

impl PostTurnSemanticGovernanceReport {
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            attempted: false,
            executed: false,
            skipped_reason: Some(reason.into()),
            proposal_count: 0,
            accepted_count: 0,
            rejected_count: 0,
            deferred_count: 0,
            plane_reports: Vec::new(),
            soul_candidate_handoffs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostTurnMemoryGovernanceReport<MaintenanceReport, LifecycleReport> {
    pub session_commit: SessionTurnCommitReport,
    pub maintenance: Option<MaintenanceReport>,
    pub private_garden_self_work: PostTurnPrivateGardenReport,
    pub semantic_governance: PostTurnSemanticGovernanceReport,
    pub lifecycle_report: LifecycleReport,
}
