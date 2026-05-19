use crate::{MemoryDomain, MemoryPlane, RuntimeProfile, SourceRef};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WriteDecision {
    Accepted,
    Rejected,
    Deferred,
    Merged,
    Superseded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WriteRejectReason {
    EmptyContent,
    MissingSource,
    RawPayloadOrLog,
    StructuredMaterial,
    WeakCanonicalStatement,
    NeedsDistillation,
    ProfileRejected,
    RoutedToProcedural,
}

impl WriteRejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyContent => "empty_content",
            Self::MissingSource => "missing_source",
            Self::RawPayloadOrLog => "raw_payload_or_log",
            Self::StructuredMaterial => "structured_material",
            Self::WeakCanonicalStatement => "weak_canonical_statement",
            Self::NeedsDistillation => "needs_distillation",
            Self::ProfileRejected => "profile_rejected",
            Self::RoutedToProcedural => "routed_to_procedural",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteCandidate {
    pub identity: String,
    pub scope: String,
    pub content: String,
    pub source: Option<String>,
    pub plane_hint: Option<MemoryPlane>,
}

impl WriteCandidate {
    pub fn new(
        identity: impl Into<String>,
        scope: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            identity: identity.into(),
            scope: scope.into(),
            content: content.into(),
            source: None,
            plane_hint: None,
        }
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn plane_hint(mut self, plane: MemoryPlane) -> Self {
        self.plane_hint = Some(plane);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceReport {
    pub reason: String,
    pub detail: Option<String>,
    pub reject_reason: Option<WriteRejectReason>,
}

impl GovernanceReport {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            detail: None,
            reject_reason: None,
        }
    }

    pub fn rejected(reason: WriteRejectReason) -> Self {
        Self {
            reason: reason.as_str().to_owned(),
            detail: None,
            reject_reason: Some(reason),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteReport {
    pub decision: WriteDecision,
    pub domain: Option<MemoryDomain>,
    pub plane: Option<MemoryPlane>,
    pub record_id: Option<String>,
    pub governance: GovernanceReport,
    pub source: Option<SourceRef>,
    pub profile: Option<RuntimeProfile>,
}

impl WriteReport {
    pub fn accepted(record: &MemoryRecord) -> Self {
        Self {
            decision: WriteDecision::Accepted,
            domain: Some(record.domain),
            plane: Some(record.plane),
            record_id: Some(record.id.clone()),
            governance: GovernanceReport::new("accepted"),
            source: None,
            profile: None,
        }
    }

    pub fn accepted_with_profile(record: &MemoryRecord, profile: RuntimeProfile) -> Self {
        Self {
            profile: Some(profile),
            ..Self::accepted(record)
        }
    }

    pub fn rejected(reason: impl Into<String>) -> Self {
        Self {
            decision: WriteDecision::Rejected,
            domain: None,
            plane: None,
            record_id: None,
            governance: GovernanceReport::new(reason),
            source: None,
            profile: None,
        }
    }

    pub fn rejected_with_reason(reason: WriteRejectReason, profile: RuntimeProfile) -> Self {
        Self {
            decision: WriteDecision::Rejected,
            domain: None,
            plane: None,
            record_id: None,
            governance: GovernanceReport::rejected(reason),
            source: None,
            profile: Some(profile),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewMemoryRecord {
    pub identity: String,
    pub scope: String,
    pub content: String,
    pub source: String,
    pub domain: MemoryDomain,
    pub plane: MemoryPlane,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryRecord {
    pub id: String,
    pub identity: String,
    pub scope: String,
    pub content: String,
    pub source: String,
    pub domain: MemoryDomain,
    pub plane: MemoryPlane,
}
