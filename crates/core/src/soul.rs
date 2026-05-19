use crate::RuntimeProfile;

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
