use bm_adapter::{AdapterResponse, AdapterSdkReport};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryResponseStatus {
    Accepted,
    Rejected,
    Queued,
    Duplicated,
}

impl EntryResponseStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Queued => "queued",
            Self::Duplicated => "duplicated",
        }
    }
}

pub struct EntryResponse {
    pub status: EntryResponseStatus,
    pub adapter: AdapterResponse<AdapterSdkReport>,
}

impl EntryResponse {
    pub(crate) fn from_adapter(adapter: AdapterResponse<AdapterSdkReport>) -> Self {
        let status = match &adapter {
            AdapterResponse::Accepted { .. } => EntryResponseStatus::Accepted,
            AdapterResponse::Rejected { .. } => EntryResponseStatus::Rejected,
            AdapterResponse::Queued { .. } => EntryResponseStatus::Queued,
            AdapterResponse::Duplicated { .. } => EntryResponseStatus::Duplicated,
        };
        Self { status, adapter }
    }
}
