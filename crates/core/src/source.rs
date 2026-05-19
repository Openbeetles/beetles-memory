#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SourceKind {
    Manual,
    AdapterEvent,
    Extraction,
    TaskLearning,
    SnapshotImport,
    ArchiveEvidence,
    ReplayFixture,
    BeetleMigration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SourceScope {
    User,
    Agent,
    Task,
    Board,
    Relation,
    System,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EvidenceState {
    Canonical,
    Supported,
    Weak,
    Conflict,
    Stale,
    ArchiveOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Freshness {
    Current,
    Recent,
    Aging,
    Stale,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
