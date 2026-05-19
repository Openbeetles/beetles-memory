//! Core Beetle Memory contracts.

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MemoryDomain {
    Program,
    Subject,
    Soul,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MemoryPlane {
    SharedFactual,
    Procedural,
    Subject,
    SoulGovernance,
    Archive,
}

impl MemoryPlane {
    pub fn domain(self) -> MemoryDomain {
        match self {
            Self::SharedFactual | Self::Procedural | Self::Archive => MemoryDomain::Program,
            Self::Subject => MemoryDomain::Subject,
            Self::SoulGovernance => MemoryDomain::Soul,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RuntimeProfile {
    EspCompact,
    LinuxDevice,
    DesktopMacos,
    DesktopWindows,
    ServerLinux,
    SdkEmbedded,
    SdkFull,
    MemoryGateway,
    DevFull,
}

impl RuntimeProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EspCompact => "EspCompact",
            Self::LinuxDevice => "LinuxDevice",
            Self::DesktopMacos => "DesktopMacos",
            Self::DesktopWindows => "DesktopWindows",
            Self::ServerLinux => "ServerLinux",
            Self::SdkEmbedded => "SdkEmbedded",
            Self::SdkFull => "SdkFull",
            Self::MemoryGateway => "MemoryGateway",
            Self::DevFull => "DevFull",
        }
    }

    pub fn allows_plane(self, plane: MemoryPlane) -> bool {
        !matches!(
            (self, plane),
            (Self::EspCompact, MemoryPlane::SoulGovernance)
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WriteDecision {
    Accepted,
    Rejected,
    Deferred,
    Merged,
    Superseded,
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
}

impl GovernanceReport {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            detail: None,
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
}

impl WriteReport {
    pub fn accepted(record: &MemoryRecord) -> Self {
        Self {
            decision: WriteDecision::Accepted,
            domain: Some(record.domain),
            plane: Some(record.plane),
            record_id: Some(record.id.clone()),
            governance: GovernanceReport::new("accepted"),
        }
    }

    pub fn rejected(reason: impl Into<String>) -> Self {
        Self {
            decision: WriteDecision::Rejected,
            domain: None,
            plane: None,
            record_id: None,
            governance: GovernanceReport::new(reason),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallQuery {
    pub scope: String,
    pub domain: Option<MemoryDomain>,
    pub plane: Option<MemoryPlane>,
    pub limit: usize,
}

impl RecallQuery {
    pub fn new(scope: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            domain: None,
            plane: None,
            limit: 8,
        }
    }

    pub fn domain(mut self, domain: MemoryDomain) -> Self {
        self.domain = Some(domain);
        self
    }

    pub fn plane(mut self, plane: MemoryPlane) -> Self {
        self.plane = Some(plane);
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallSelection {
    pub record_id: String,
    pub domain: MemoryDomain,
    pub plane: MemoryPlane,
    pub content: String,
    pub source: String,
}

impl From<MemoryRecord> for RecallSelection {
    fn from(record: MemoryRecord) -> Self {
        Self {
            record_id: record.id,
            domain: record.domain,
            plane: record.plane,
            content: record.content,
            source: record.source,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecallSelectionReport {
    pub selected: Vec<RecallSelection>,
    pub skipped: usize,
    pub profile: RuntimeProfile,
    pub query: RecallQuery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProjectionSurface {
    Prompt,
    Inspection,
    Adapter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionBlock {
    pub record_id: String,
    pub domain: MemoryDomain,
    pub plane: MemoryPlane,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionReport {
    pub surface: ProjectionSurface,
    pub blocks: Vec<ProjectionBlock>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TransportKind {
    Sdk,
    HttpApi,
    Webhook,
    Wss,
    Mqtt,
    Mcp,
    Cli,
    A2aBridge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TransportMode {
    InProcess,
    LocalOnly,
    Client,
    Server,
    Bridge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterCapability {
    pub kind: TransportKind,
    pub mode: TransportMode,
    pub profile: RuntimeProfile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterPolicy {
    pub capability: AdapterCapability,
    pub requires_auth: bool,
    pub max_payload_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterEvent {
    pub kind: TransportKind,
    pub source: String,
    pub scope: String,
}
