use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum MemoryDomain {
    Program,
    Subject,
    Soul,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum MemoryPlane {
    SharedFactual,
    Procedural,
    ContinuityCapsule,
    ArchiveEvidence,
    TaskRecall,
    SubjectProjection,
    SoulGovernance,
}

impl MemoryPlane {
    pub fn domain(self) -> MemoryDomain {
        match self {
            Self::SharedFactual
            | Self::Procedural
            | Self::ContinuityCapsule
            | Self::ArchiveEvidence
            | Self::TaskRecall => MemoryDomain::Program,
            Self::SubjectProjection => MemoryDomain::Subject,
            Self::SoulGovernance => MemoryDomain::Soul,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SharedFactual => "SharedFactual",
            Self::Procedural => "Procedural",
            Self::ContinuityCapsule => "ContinuityCapsule",
            Self::ArchiveEvidence => "ArchiveEvidence",
            Self::TaskRecall => "TaskRecall",
            Self::SubjectProjection => "SubjectProjection",
            Self::SoulGovernance => "SoulGovernance",
        }
    }
}

impl MemoryDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Program => "Program",
            Self::Subject => "Subject",
            Self::Soul => "Soul",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
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
                | (Self::SdkEmbedded, MemoryPlane::SoulGovernance)
        )
    }

    pub fn projection_budget_bytes(self) -> usize {
        match self {
            Self::EspCompact | Self::SdkEmbedded => 512,
            Self::LinuxDevice => 2_048,
            Self::DesktopMacos | Self::DesktopWindows | Self::ServerLinux => 4_096,
            Self::SdkFull | Self::MemoryGateway | Self::DevFull => 8_192,
        }
    }
}
