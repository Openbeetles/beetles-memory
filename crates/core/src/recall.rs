use crate::{MemoryDomain, MemoryPlane, RuntimeProfile, SourceKind, SourceRef};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PromptRecallIntent {
    Factual,
    Procedural,
    Continuity,
    Evidence,
    Mixed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallQuery {
    pub scope: String,
    pub identity: Option<String>,
    pub domain: Option<MemoryDomain>,
    pub plane: Option<MemoryPlane>,
    pub intent: PromptRecallIntent,
    pub limit: usize,
}

impl RecallQuery {
    pub fn new(scope: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            identity: None,
            domain: None,
            plane: None,
            intent: PromptRecallIntent::Mixed,
            limit: 8,
        }
    }

    pub fn identity(mut self, identity: impl Into<String>) -> Self {
        self.identity = Some(identity.into());
        self
    }

    pub fn domain(mut self, domain: MemoryDomain) -> Self {
        self.domain = Some(domain);
        self
    }

    pub fn plane(mut self, plane: MemoryPlane) -> Self {
        self.plane = Some(plane);
        self
    }

    pub fn intent(mut self, intent: PromptRecallIntent) -> Self {
        self.intent = intent;
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallScoreBreakdown {
    pub lexical: u32,
    pub semantic: u32,
    pub recency: u32,
    pub provenance: u32,
    pub intent: u32,
    pub total: u32,
}

impl RecallScoreBreakdown {
    pub fn exact_match() -> Self {
        Self {
            lexical: 20,
            semantic: 20,
            recency: 10,
            provenance: 10,
            intent: 20,
            total: 80,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallSelection {
    pub record_id: String,
    pub domain: MemoryDomain,
    pub plane: MemoryPlane,
    pub content: String,
    pub source: SourceRef,
    pub score: RecallScoreBreakdown,
    pub canonical: bool,
    pub privacy_filtered: bool,
}

impl From<crate::MemoryRecord> for RecallSelection {
    fn from(record: crate::MemoryRecord) -> Self {
        Self {
            record_id: record.id,
            domain: record.domain,
            plane: record.plane,
            content: record.content,
            source: SourceRef::new(SourceKind::AdapterEvent, record.source),
            score: RecallScoreBreakdown::exact_match(),
            canonical: !matches!(record.plane, MemoryPlane::ArchiveEvidence),
            privacy_filtered: matches!(
                record.plane,
                MemoryPlane::SubjectProjection | MemoryPlane::SoulGovernance
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RecallSkipReason {
    LowerScore,
    ProfileBudget,
    PrivacyPolicy,
    PlaneFiltered,
    DomainFiltered,
    ScopeMismatch,
    LimitReached,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkippedRecallCandidate {
    pub record_id: String,
    pub plane: MemoryPlane,
    pub reason: RecallSkipReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallPlaneReport {
    pub plane: MemoryPlane,
    pub available: usize,
    pub selected: usize,
    pub skipped: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrossPlanePlaneSignal {
    pub plane: MemoryPlane,
    pub score: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrossPlaneRerankCandidate {
    pub record_id: String,
    pub plane: MemoryPlane,
    pub score: u32,
    pub source: SourceRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrossPlaneRerankReport {
    pub intent: PromptRecallIntent,
    pub top_planes: Vec<CrossPlanePlaneSignal>,
    pub top_candidates: Vec<CrossPlaneRerankCandidate>,
}

impl CrossPlaneRerankReport {
    pub fn empty(intent: PromptRecallIntent) -> Self {
        Self {
            intent,
            top_planes: Vec::new(),
            top_candidates: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecallWarning {
    ProfileBudgetTrimmed {
        profile: RuntimeProfile,
        before: usize,
        after: usize,
    },
    PrivacyFiltered {
        plane: MemoryPlane,
    },
    EvidenceNotCanonical {
        record_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallSelectionReport {
    pub query: RecallQuery,
    pub profile: RuntimeProfile,
    pub selected: Vec<RecallSelection>,
    pub skipped: Vec<SkippedRecallCandidate>,
    pub plane_reports: Vec<RecallPlaneReport>,
    pub rerank: CrossPlaneRerankReport,
    pub warnings: Vec<RecallWarning>,
}
