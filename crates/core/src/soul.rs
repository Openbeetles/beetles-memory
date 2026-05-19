use crate::{MemoryPlane, RuntimeProfile};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MentalPrivacyLayer {
    Shared,
    Relational,
    Private,
    Sealed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MentalPrivacyVisibility {
    Direct,
    SummaryOnly,
    RequestOnly,
    Sealed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MentalPrivacyOwnerAccessMode {
    Direct,
    RequestOnly,
    DenyByDefault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MentalPrivacyQuotePolicy {
    Raw,
    SummaryOnly,
    NeverQuote,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DisclosureSurface {
    Prompt,
    ToolContext,
    OperatorInspection,
    Adapter,
    Replay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SoulFeedbackLane {
    Reply,
    Initiative,
    Strategy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SubjectAssemblySource {
    SelfCore,
    SelfContinuity,
    Relationship,
    ProgramMemory,
    World,
    Task,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SoulGovernanceDecision {
    Accepted,
    Rejected,
    Deferred,
    RevisionRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SoulGovernanceReason {
    StableIdentity,
    RelationshipBoundary,
    PrivacyFiltered,
    RawPrivateRejected,
    WeakEvidence,
    ProfileRejected,
    ProgramEvidenceOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentalPrivacyPolicy {
    pub layer: MentalPrivacyLayer,
    pub visibility: MentalPrivacyVisibility,
    pub owner_access_mode: MentalPrivacyOwnerAccessMode,
    pub quote_policy: MentalPrivacyQuotePolicy,
}

impl MentalPrivacyPolicy {
    pub fn allows_raw_default_surface(&self) -> bool {
        matches!(self.layer, MentalPrivacyLayer::Shared)
            && matches!(self.visibility, MentalPrivacyVisibility::Direct)
            && matches!(self.quote_policy, MentalPrivacyQuotePolicy::Raw)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SoulSourceKind {
    SelfAuthoredCore,
    RelationshipConstitution,
    PersonaPriority,
    MentalPrivacy,
    PrivateKernel,
    PrivateGarden,
    SoulKernelStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivacyDisclosureDecision {
    pub surface: DisclosureSurface,
    pub layer: MentalPrivacyLayer,
    pub allowed: bool,
    pub quote_policy: MentalPrivacyQuotePolicy,
    pub reason: SoulGovernanceReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubjectAssemblySourceRef {
    pub source: SubjectAssemblySource,
    pub record_id: String,
    pub plane: MemoryPlane,
    pub privacy_layer: MentalPrivacyLayer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubjectAssemblyReport {
    pub mounted: bool,
    pub sources_used: Vec<SubjectAssemblySourceRef>,
    pub sources_missing: Vec<SubjectAssemblySource>,
    pub privacy_decisions: Vec<PrivacyDisclosureDecision>,
    pub profile: RuntimeProfile,
    pub budget_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoulGovernanceRecord {
    pub source_id: String,
    pub source_kind: SoulSourceKind,
    pub layer: MentalPrivacyLayer,
    pub policy: MentalPrivacyPolicy,
    pub decision: SoulGovernanceDecision,
    pub reason: SoulGovernanceReason,
    pub feedback_lanes: Vec<SoulFeedbackLane>,
    pub revision: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoulGovernanceRef {
    pub source_id: String,
    pub source_kind: SoulSourceKind,
    pub layer: MentalPrivacyLayer,
    pub revision: Option<String>,
    pub policy: MentalPrivacyPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubjectShellReport {
    pub grounded: bool,
    pub sources_used: Vec<String>,
    pub sources_missing: Vec<String>,
    pub profile: RuntimeProfile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubjectProjectionReport {
    pub mounted: bool,
    pub summary: String,
    pub privacy_filtered: bool,
    pub budget_bytes: usize,
    pub shell: SubjectShellReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoulFeedbackReport {
    pub reply: Option<String>,
    pub initiative: Option<String>,
    pub strategy: Option<String>,
    pub privacy_filtered: bool,
}
