use crate::{
    ArchiveEvidenceLink, Confidence, EvidenceState, Freshness, LongTermMemoryKind,
    LongTermMergeReport, MemoryDomain, MemoryPlane, MentalPrivacyLayer, ProceduralSkillMeta,
    ProceduralSkillWriteReport, RuntimeProfile, SourceRef,
};
use serde::{Deserialize, Serialize};

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
    RawPrivateRejected,
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
            Self::RawPrivateRejected => "raw_private_rejected",
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
    pub privacy_layer: MentalPrivacyLayer,
    pub evidence: EvidenceState,
    pub canonical: bool,
    pub long_term_kind: Option<LongTermMemoryKind>,
    pub topic: Option<String>,
    pub keywords: Vec<String>,
    pub confidence: Option<Confidence>,
    pub freshness: Option<Freshness>,
    pub observed_at: Option<u64>,
    pub archive_links: Vec<ArchiveEvidenceLink>,
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
            privacy_layer: MentalPrivacyLayer::Shared,
            evidence: EvidenceState::Supported,
            canonical: false,
            long_term_kind: None,
            topic: None,
            keywords: Vec::new(),
            confidence: None,
            freshness: None,
            observed_at: None,
            archive_links: Vec::new(),
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

    pub fn privacy_layer(mut self, layer: MentalPrivacyLayer) -> Self {
        self.privacy_layer = layer;
        self
    }

    pub fn evidence(mut self, evidence: EvidenceState) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn canonical(mut self, canonical: bool) -> Self {
        self.canonical = canonical;
        self
    }

    pub fn long_term_kind(mut self, kind: LongTermMemoryKind) -> Self {
        self.long_term_kind = Some(kind);
        self
    }

    pub fn topic(mut self, topic: impl Into<String>) -> Self {
        self.topic = Some(topic.into());
        self
    }

    pub fn keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = keywords;
        self
    }

    pub fn confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn freshness(mut self, freshness: Freshness) -> Self {
        self.freshness = Some(freshness);
        self
    }

    pub fn observed_at(mut self, observed_at: u64) -> Self {
        self.observed_at = Some(observed_at);
        self
    }

    pub fn archive_links(mut self, links: Vec<ArchiveEvidenceLink>) -> Self {
        self.archive_links = links;
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
    pub long_term: Option<LongTermMergeReport>,
    pub procedural: Option<ProceduralSkillWriteReport>,
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
            long_term: None,
            procedural: None,
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
            long_term: None,
            procedural: None,
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
            long_term: None,
            procedural: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryRecordMeta {
    pub long_term_kind: Option<LongTermMemoryKind>,
    pub topic: Option<String>,
    pub keywords: Vec<String>,
    pub evidence: EvidenceState,
    pub confidence: Confidence,
    pub freshness: Freshness,
    pub canonical: bool,
    pub slot_id: Option<String>,
    pub observed_at: Option<u64>,
    pub updated_at: u64,
    pub archive_links: Vec<ArchiveEvidenceLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub procedural: Option<ProceduralSkillMeta>,
}

impl MemoryRecordMeta {
    pub fn default_for_plane(plane: MemoryPlane) -> Self {
        Self {
            long_term_kind: crate::default_long_term_kind_for_plane(plane),
            topic: None,
            keywords: Vec::new(),
            evidence: if matches!(plane, MemoryPlane::ArchiveEvidence) {
                EvidenceState::ArchiveOnly
            } else {
                EvidenceState::Supported
            },
            confidence: Confidence::Medium,
            freshness: Freshness::Unknown,
            canonical: !matches!(plane, MemoryPlane::ArchiveEvidence),
            slot_id: None,
            observed_at: None,
            updated_at: 0,
            archive_links: Vec::new(),
            procedural: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NewMemoryRecord {
    pub identity: String,
    pub scope: String,
    pub content: String,
    pub source: String,
    pub domain: MemoryDomain,
    pub plane: MemoryPlane,
    #[serde(default = "default_meta_for_deserialize")]
    pub meta: MemoryRecordMeta,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub identity: String,
    pub scope: String,
    pub content: String,
    pub source: String,
    pub domain: MemoryDomain,
    pub plane: MemoryPlane,
    #[serde(default = "default_meta_for_deserialize")]
    pub meta: MemoryRecordMeta,
}

fn default_meta_for_deserialize() -> MemoryRecordMeta {
    MemoryRecordMeta::default_for_plane(MemoryPlane::SharedFactual)
}
