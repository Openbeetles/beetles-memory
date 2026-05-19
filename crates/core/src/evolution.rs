use crate::{
    Confidence, EvidenceState, MemoryPlane, MentalPrivacyLayer, ProjectionReport,
    RecallSelectionReport, RuntimeProfile, SourceRef, SubjectAssemblyReport, WriteCandidate,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvolutionInput {
    pub run_id: String,
    pub identity: String,
    pub scope: String,
    pub profile: RuntimeProfile,
    pub mode: EvolutionMode,
    pub evidence: Vec<EvidenceRef>,
    pub recall_report: Option<RecallSelectionReport>,
    pub projection_report: Option<ProjectionReport>,
    pub subject_assembly: Option<SubjectAssemblyReport>,
    pub budget: EvolutionBudget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EvolutionMode {
    Full,
    Compact,
    Consumer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvolutionBudget {
    pub max_events: usize,
    pub max_records: usize,
    pub max_branches: usize,
    pub max_proposals: usize,
    pub max_output_bytes: usize,
    pub allow_private_layer: bool,
    pub allow_soul_revision: bool,
    pub allow_script_backend: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRef {
    pub record_id: Option<String>,
    pub event_seq: Option<u64>,
    pub source: SourceRef,
    pub plane: MemoryPlane,
    pub privacy_layer: MentalPrivacyLayer,
    pub evidence: EvidenceState,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EvolutionDisposition {
    NoWrite,
    Observe,
    Defer,
    Refresh,
    Merge,
    Split,
    Retire,
    PromoteProcedural,
    RefreshSubjectProjection,
    ProposeSoulRevision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EvolutionProposalKind {
    NoWriteReport,
    MemoryMerge,
    MemorySplit,
    MemoryRefresh,
    MemoryRetire,
    ProceduralPromotion,
    SubjectProjectionRefresh,
    SoulGovernanceRevision,
    PrivacyRepair,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvolutionProposal {
    pub proposal_id: String,
    pub kind: EvolutionProposalKind,
    pub disposition: EvolutionDisposition,
    pub target_plane: Option<MemoryPlane>,
    pub source_evidence: Vec<EvidenceRef>,
    pub confidence: Confidence,
    pub risk: EvolutionRisk,
    pub privacy_filtered: bool,
    pub candidate_write: Option<WriteCandidate>,
    pub rationale: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EvolutionRisk {
    Low,
    Medium,
    High,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvolutionRejectedCandidate {
    pub candidate_id: String,
    pub attempted_kind: EvolutionProposalKind,
    pub reason: EvolutionRejectReason,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EvolutionRejectReason {
    WeakEvidence,
    PrivacyFiltered,
    ProfileBudget,
    ProgramEvidenceOnly,
    ArchiveNeedsDistillation,
    SoulRevisionNotAllowed,
    ConsumerMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvolutionProposalBatch {
    pub run_id: String,
    pub profile: RuntimeProfile,
    pub mode: EvolutionMode,
    pub proposals: Vec<EvolutionProposal>,
    pub rejected_candidates: Vec<EvolutionRejectedCandidate>,
    pub report: EvolutionRunReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvolutionRunReport {
    pub run_id: String,
    pub mode: EvolutionMode,
    pub evidence_read: usize,
    pub branches_evaluated: usize,
    pub proposals_emitted: usize,
    pub rejected_candidates: usize,
    pub privacy_filtered_count: usize,
    pub profile_trimmed: bool,
    pub raw_private_exposed: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvolutionAdjudication {
    pub selected_proposal_id: Option<String>,
    pub strongest_rejected_candidate_id: Option<String>,
    pub disposition: EvolutionAdjudicationDisposition,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EvolutionAdjudicationDisposition {
    UpholdProposal,
    ReviseProposal,
    HoldForMoreEvidence,
    ForceNoWrite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvolutionBackendReport {
    pub batch: EvolutionProposalBatch,
    pub backend: EvolutionBackendKind,
    pub deterministic: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EvolutionBackendKind {
    Deterministic,
    Script,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvolutionProposalSummary {
    pub run_id: String,
    pub profile: RuntimeProfile,
    pub mode: EvolutionMode,
    pub proposals_count: usize,
    pub blocked_count: usize,
    pub privacy_filtered_count: usize,
    pub profile_trimmed: bool,
}
