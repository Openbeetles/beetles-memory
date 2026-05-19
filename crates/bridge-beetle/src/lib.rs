//! Beetle migration contracts for Beetle Memory.

use bm_core::{
    EvidenceState, Freshness, MemoryDomain, MemoryPlane, MentalPrivacyLayer, WriteCandidate,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BeetleLongTermKind {
    SharedFact,
    ContinuityCapsule,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BeetleOriginKind {
    LongTerm {
        kind: BeetleLongTermKind,
        freshness: Freshness,
        evidence: EvidenceState,
    },
    RuntimeSkill,
    ArchiveEvidence,
    SubjectProjection,
    SoulGovernance,
    PrivatePresence,
}

impl Default for BeetleOriginKind {
    fn default() -> Self {
        Self::LongTerm {
            kind: BeetleLongTermKind::SharedFact,
            freshness: Freshness::Unknown,
            evidence: EvidenceState::Supported,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ContentHandling {
    MigrateContent,
    FixtureOnly,
    PresenceOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeetleMemorySource {
    pub system: String,
    pub record_id: String,
    pub content: String,
    pub origin_path: Option<String>,
    pub origin_kind: BeetleOriginKind,
}

impl BeetleMemorySource {
    pub fn new(
        system: impl Into<String>,
        record_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            system: system.into(),
            record_id: record_id.into(),
            content: content.into(),
            origin_path: None,
            origin_kind: BeetleOriginKind::default(),
        }
    }

    pub fn origin_path(mut self, origin_path: impl Into<String>) -> Self {
        self.origin_path = Some(origin_path.into());
        self
    }

    pub fn origin_kind(mut self, origin_kind: BeetleOriginKind) -> Self {
        self.origin_kind = origin_kind;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationPlan {
    pub source: BeetleMemorySource,
    pub origin_path: String,
    pub origin_kind: BeetleOriginKind,
    pub canonical: bool,
    pub privacy_layer: MentalPrivacyLayer,
    pub content_handling: ContentHandling,
    pub target_domain: MemoryDomain,
    pub target_plane: MemoryPlane,
    pub candidate: WriteCandidate,
}

#[derive(Clone, Debug, Default)]
pub struct MigrationPlanner;

impl MigrationPlanner {
    pub fn plan(&self, source: BeetleMemorySource) -> MigrationPlan {
        let (target_plane, canonical, privacy_layer, content_handling) =
            map_source_inventory(source.origin_kind);
        let origin_path = source
            .origin_path
            .clone()
            .unwrap_or_else(|| "unknown".to_owned());
        let origin_kind = source.origin_kind;
        let content = match content_handling {
            ContentHandling::MigrateContent | ContentHandling::FixtureOnly => {
                source.content.clone()
            }
            ContentHandling::PresenceOnly => {
                format!(
                    "private source present: {}:{}",
                    source.system, source.record_id
                )
            }
        };
        let candidate = WriteCandidate::new("beetle:migration", "beetle:migration", content)
            .source(format!("beetle:{}", source.record_id))
            .plane_hint(target_plane);

        MigrationPlan {
            source,
            origin_path,
            origin_kind,
            canonical,
            privacy_layer,
            content_handling,
            target_domain: target_plane.domain(),
            target_plane,
            candidate,
        }
    }
}

fn map_source_inventory(
    origin_kind: BeetleOriginKind,
) -> (MemoryPlane, bool, MentalPrivacyLayer, ContentHandling) {
    match origin_kind {
        BeetleOriginKind::LongTerm {
            kind,
            freshness,
            evidence,
        } => {
            let target_plane = match kind {
                BeetleLongTermKind::SharedFact => MemoryPlane::SharedFactual,
                BeetleLongTermKind::ContinuityCapsule => MemoryPlane::ContinuityCapsule,
            };
            (
                target_plane,
                is_canonical_long_term(freshness, evidence),
                MentalPrivacyLayer::Shared,
                ContentHandling::MigrateContent,
            )
        }
        BeetleOriginKind::RuntimeSkill => (
            MemoryPlane::Procedural,
            true,
            MentalPrivacyLayer::Shared,
            ContentHandling::MigrateContent,
        ),
        BeetleOriginKind::ArchiveEvidence => (
            MemoryPlane::ArchiveEvidence,
            false,
            MentalPrivacyLayer::Shared,
            ContentHandling::MigrateContent,
        ),
        BeetleOriginKind::SubjectProjection => (
            MemoryPlane::SubjectProjection,
            false,
            MentalPrivacyLayer::Relational,
            ContentHandling::FixtureOnly,
        ),
        BeetleOriginKind::SoulGovernance => (
            MemoryPlane::SoulGovernance,
            true,
            MentalPrivacyLayer::Relational,
            ContentHandling::MigrateContent,
        ),
        BeetleOriginKind::PrivatePresence => (
            MemoryPlane::SoulGovernance,
            false,
            MentalPrivacyLayer::Private,
            ContentHandling::PresenceOnly,
        ),
    }
}

fn is_canonical_long_term(freshness: Freshness, evidence: EvidenceState) -> bool {
    matches!(
        freshness,
        Freshness::Current | Freshness::Recent | Freshness::Unknown
    ) && matches!(evidence, EvidenceState::Canonical)
}
