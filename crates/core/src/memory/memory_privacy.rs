use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPrivacyClass {
    PublicRuntime,
    SharedWithSubject,
    PrivateGarden,
    SoulPrivate,
    OperatorDiagnostic,
}

impl MemoryPrivacyClass {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PublicRuntime => "public_runtime",
            Self::SharedWithSubject => "shared_with_subject",
            Self::PrivateGarden => "private_garden",
            Self::SoulPrivate => "soul_private",
            Self::OperatorDiagnostic => "operator_diagnostic",
        }
    }

    pub const fn projection_content_allowed(self) -> bool {
        matches!(self, Self::PublicRuntime | Self::SharedWithSubject)
    }

    pub const fn shared_fact_surface_allowed(self) -> bool {
        matches!(self, Self::PublicRuntime)
    }
}
