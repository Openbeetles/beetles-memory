use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum SourceKind {
    Manual,
    AdapterEvent,
    Extraction,
    LongTermExtraction,
    TaskLearning,
    SnapshotImport,
    ArchiveEvidence,
    ArchiveImport,
    ReplayFixture,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum SourceScope {
    User,
    Agent,
    Task,
    Board,
    Relation,
    System,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum EvidenceState {
    Canonical,
    Supported,
    Weak,
    Conflict,
    Stale,
    ArchiveOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Freshness {
    Current,
    Recent,
    Aging,
    Stale,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub fn rank(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceRef {
    pub kind: SourceKind,
    pub id: String,
    pub origin_path: Option<String>,
}

impl SourceRef {
    pub fn new(kind: SourceKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
            origin_path: None,
        }
    }

    pub fn origin_path(mut self, origin_path: impl Into<String>) -> Self {
        self.origin_path = Some(origin_path.into());
        self
    }
}
