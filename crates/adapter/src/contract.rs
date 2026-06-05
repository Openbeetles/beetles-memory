use bm_sdk::{
    MemoryCapabilityCatalog, MemoryCloseReport, MemoryCloseRequest, MemoryExportReport,
    MemoryExportRequest, MemoryGovernancePolicyMutationReport, MemoryImportReport,
    MemoryImportRequest, MemoryInspectionReport, MemoryInspectionRequest,
    MemoryLongTermDetailReport, MemoryLongTermDetailRequest, MemoryLongTermListReport,
    MemoryLongTermListRequest, MemoryLongTermMutationReport, MemoryLongTermMutationRequest,
    MemoryLongTermPolicyRequest, MemoryMaintenanceReport, MemoryMaintenanceRequest,
    MemoryProjectionReport, MemoryProjectionRequest, MemoryRecallReport, MemoryRecallRequest,
    MemoryRecoverReport, MemoryRecoverRequest, MemoryReplayReport, MemoryReplayRequest,
    MemoryWriteReport, MemoryWriteRequest,
};
use serde::{Deserialize, Serialize};

use crate::AdapterErrorKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    Sdk,
    Cli,
    Http,
    Wss,
    Mcp,
    A2a,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportMode {
    InProcess,
    Client,
    Server,
    Bidirectional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterOperation {
    Write,
    Recall,
    Project,
    Maintain,
    Inspect,
    Recover,
    Replay,
    Export,
    Import,
    LongTermList,
    LongTermDetail,
    LongTermMutate,
    LongTermPolicy,
    Capabilities,
    Subscribe,
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterSource {
    pub source_id: String,
    pub source_kind: String,
    pub agent_id: String,
    pub owner_id: String,
    pub channel: String,
    pub chat_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterAuthContext {
    pub authenticated: bool,
    pub auth_kind: String,
    pub principal: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterEnvelope<T> {
    pub request_id: String,
    pub transport: TransportKind,
    pub mode: TransportMode,
    pub operation: AdapterOperation,
    pub source: AdapterSource,
    pub auth: AdapterAuthContext,
    pub idempotency_key: String,
    pub audit_id: String,
    pub payload: T,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterEvent {
    pub request_id: String,
    pub audit_id: String,
    pub transport: TransportKind,
    pub operation: AdapterOperation,
    pub event_kind: String,
    pub summary: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AdapterResponse<T> {
    Accepted {
        request_id: String,
        audit_id: String,
        report: T,
    },
    Rejected {
        request_id: String,
        audit_id: String,
        error_key: AdapterErrorKey,
        reason: String,
    },
    Queued {
        request_id: String,
        audit_id: String,
        queue: String,
    },
    Duplicated {
        request_id: String,
        audit_id: String,
        idempotency_key: String,
    },
}

pub enum AdapterCommand {
    Write(MemoryWriteRequest),
    Recall(MemoryRecallRequest),
    Project(MemoryProjectionRequest),
    Maintain(MemoryMaintenanceRequest),
    Inspect(MemoryInspectionRequest),
    Recover(MemoryRecoverRequest),
    Replay(MemoryReplayRequest),
    Export(MemoryExportRequest),
    Import(Box<MemoryImportRequest>),
    LongTermList(MemoryLongTermListRequest),
    LongTermDetail(MemoryLongTermDetailRequest),
    LongTermMutate(MemoryLongTermMutationRequest),
    LongTermPolicy(MemoryLongTermPolicyRequest),
    Capabilities,
    Close(MemoryCloseRequest),
}

impl std::fmt::Debug for AdapterCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AdapterCommand")
            .field(&self.operation())
            .finish()
    }
}

impl AdapterCommand {
    pub const fn operation(&self) -> AdapterOperation {
        match self {
            Self::Write(_) => AdapterOperation::Write,
            Self::Recall(_) => AdapterOperation::Recall,
            Self::Project(_) => AdapterOperation::Project,
            Self::Maintain(_) => AdapterOperation::Maintain,
            Self::Inspect(_) => AdapterOperation::Inspect,
            Self::Recover(_) => AdapterOperation::Recover,
            Self::Replay(_) => AdapterOperation::Replay,
            Self::Export(_) => AdapterOperation::Export,
            Self::Import(_) => AdapterOperation::Import,
            Self::LongTermList(_) => AdapterOperation::LongTermList,
            Self::LongTermDetail(_) => AdapterOperation::LongTermDetail,
            Self::LongTermMutate(_) => AdapterOperation::LongTermMutate,
            Self::LongTermPolicy(_) => AdapterOperation::LongTermPolicy,
            Self::Capabilities => AdapterOperation::Capabilities,
            Self::Close(_) => AdapterOperation::Close,
        }
    }
}

pub enum AdapterSdkReport {
    Write(Box<MemoryWriteReport>),
    Recall(Box<MemoryRecallReport>),
    Project(Box<MemoryProjectionReport>),
    Maintain(Box<MemoryMaintenanceReport>),
    Inspect(Box<MemoryInspectionReport>),
    Recover(Box<MemoryRecoverReport>),
    Replay(Box<MemoryReplayReport>),
    Export(Box<MemoryExportReport>),
    Import(Box<MemoryImportReport>),
    LongTermList(Box<MemoryLongTermListReport>),
    LongTermDetail(Box<MemoryLongTermDetailReport>),
    LongTermMutate(Box<MemoryLongTermMutationReport>),
    LongTermPolicy(Box<MemoryGovernancePolicyMutationReport>),
    Capabilities(Box<MemoryCapabilityCatalog>),
    Close(Box<MemoryCloseReport>),
}

impl std::fmt::Debug for AdapterSdkReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Write(_) => "Write",
            Self::Recall(_) => "Recall",
            Self::Project(_) => "Project",
            Self::Maintain(_) => "Maintain",
            Self::Inspect(_) => "Inspect",
            Self::Recover(_) => "Recover",
            Self::Replay(_) => "Replay",
            Self::Export(_) => "Export",
            Self::Import(_) => "Import",
            Self::LongTermList(_) => "LongTermList",
            Self::LongTermDetail(_) => "LongTermDetail",
            Self::LongTermMutate(_) => "LongTermMutate",
            Self::LongTermPolicy(_) => "LongTermPolicy",
            Self::Capabilities(_) => "Capabilities",
            Self::Close(_) => "Close",
        };
        f.write_str(name)
    }
}
